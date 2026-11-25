// SPDX-License-Identifier: GPL-3.0-or-later

//! Start a child process that dies with Studio.
//!
//! [`spawn`] starts a child, such as QEMU, so that it cannot outlive the
//! process that started it. The guarantee holds when that process is killed
//! with `SIGKILL` or `TerminateProcess`, with no chance to run a line of
//! code: a crash, a closed terminal or a parent that dies all end that way
//! or a gentler one. A guard that cannot be put in place is an error, and
//! then the child does not run.
//!
//! # Windows
//!
//! The first call puts the calling process in a new Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and keeps the only handle to it for
//! the life of the process. A process that a member of a job creates is a
//! member from its first instruction, so every child starts inside the job.
//! When the calling process ends, in any way, Windows closes its handles, the
//! job's last handle closes, and Windows kills every process left in the job.
//! The job does not allow breakaway. The handle is not inheritable, so no
//! child keeps the job open.
//!
//! This also covers every other process Studio starts after the first call:
//! none of them outlives Studio.
//!
//! | Step | If Studio dies right after it |
//! | --- | --- |
//! | 1. Create the job, kill on close | The job closes with no member. No child yet |
//! | 2. Put Studio in the job | The job kills Studio's members. No child yet |
//! | 3. Create the child | The child is in the job: the job kills it |
//!
//! # Linux and macOS
//!
//! A small `/bin/sh` watcher leads a new process group and reads a pipe from
//! Studio. The child runs in the watcher's group. When Studio ends in any
//! way, the kernel closes the pipe, the watcher reads end of file and sends
//! `SIGKILL` to its whole group: the child, anything the child started that
//! stayed in the group, and the watcher itself. The watcher ignores
//! `SIGINT`, `SIGHUP` and `SIGTERM`, so a signal sent to the group from
//! outside leaves it in place. The child's group is not the terminal's
//! foreground group, so a Ctrl-C reaches Studio only.
//!
//! The child first runs a small launcher shell, which waits for one line from
//! Studio before it replaces itself with the real program, keeping its pid.
//! Studio sends that line only after it checked that the watcher still lives.
//! So the program never runs in a group with no watcher.
//!
//! Every pipe is closed on exec, so after its exec no other process holds the
//! watcher's pipe. The start sequence, step by step. "Dies" means `SIGKILL`
//! to Studio right after that step.
//!
//! | Step | If Studio dies here |
//! | --- | --- |
//! | 1. Start the watcher as leader of a new group, stdin a pipe from Studio | The pipe closes. The watcher kills its group, itself only. No child yet |
//! | 2. The watcher ignores the three signals, then writes `ready` | Same |
//! | 3. Studio reads `ready` and checks that the watcher lives | Same |
//! | 4. Fork the launcher | Until its exec, the forked child holds the watcher's pipe, so the watcher waits |
//! | 5. The child joins the watcher's group, before its exec | Same |
//! | 6. The child execs the launcher shell | The pipe closes now. The watcher kills the group, the launcher with it. The launcher also reads end of file and exits |
//! | 7. Studio checks again that the watcher lives | Same |
//! | 8. Studio sends `go` | The launcher is in a group with a live watcher: the watcher kills it |
//! | 9. The launcher execs the program, same pid | The watcher kills the group |
//!
//! Step 7 is what makes a dead watcher a hard error: if the watcher is gone,
//! Studio kills the launcher, which never ran the program, and [`spawn`]
//! fails. A watcher that died and was not reaped yet leaves its group in
//! place, so step 5 can succeed with a dead watcher: step 7 catches it. If
//! the group is gone, step 5 fails, and so does [`spawn`].
//!
//! # Stopping
//!
//! [`Guarded::stop`] sends `SIGTERM` to the child, waits up to the grace
//! time, then sends `SIGKILL`, reaps the child, closes the watcher's pipe
//! and reaps the watcher. The watcher's last `SIGKILL` takes anything the
//! child left in its group. On Windows there is no polite signal for a
//! process with no window, so `stop` kills the child at once. Dropping a
//! [`Guarded`] that was not stopped kills the child at once.
//!
//! # Process ids
//!
//! Nothing here kills by a number it read from somewhere. Studio signals only
//! its own child, which it has not reaped, so that pid cannot belong to
//! another process. Both checks of the watcher use `waitpid`, which sees a
//! zombie as dead. A signal check such as `kill(pid, 0)` would see a zombie as
//! alive. This holds as long as Studio does not set `SIGCHLD` to ignored,
//! which would make the kernel reap children behind its back.
//!
//! # What it does not cover
//!
//! - A power loss, or a kill of the whole machine or session.
//! - On Linux and macOS, `SIGKILL` sent from outside to the watcher alone.
//!   The child then runs on until Studio stops it, or until Studio dies.
//! - A child that leaves the watcher's group itself, with `setsid` or
//!   `setpgid`. QEMU does not. On Windows a job member cannot leave.
//! - [`Command::env_clear`] on the given command: only the variables it sets
//!   or removes are carried to the child.
//! - A program that does not exist. On Linux and macOS the launcher's exec
//!   fails after [`spawn`] returned, and the child exits with status 127.

use std::fmt;
use std::io;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
#[cfg(unix)]
use std::thread;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

/// Why a guarded child could not be started or stopped.
#[derive(Debug)]
pub enum GuardError {
    /// The guard could not be put in place, so the child is not running.
    Unavailable(String),
    /// The child could not be started.
    Spawn(io::Error),
    /// The child could not be stopped or waited for.
    Stop(io::Error),
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(why) => write!(f, "the process guard is not in place: {why}"),
            Self::Spawn(err) => write!(f, "could not start the guarded process: {err}"),
            Self::Stop(err) => write!(f, "could not stop the guarded process: {err}"),
        }
    }
}

impl std::error::Error for GuardError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(_) => None,
            Self::Spawn(err) | Self::Stop(err) => Some(err),
        }
    }
}

/// How often [`Guarded::stop`] checks whether the child has exited.
#[cfg(unix)]
const POLL: Duration = Duration::from_millis(10);

/// A child process that dies with the process that started it.
#[derive(Debug)]
pub struct Guarded {
    child: Child,
    status: Option<ExitStatus>,
    #[cfg(unix)]
    watcher: Option<unix::Watcher>,
}

/// Starts `command` so that it dies with the calling process.
///
/// The program, its arguments, the variables it sets or removes and its
/// working directory are taken from `command`. Its standard input is closed.
/// Standard output and error go to `stdout` and `stderr`.
///
/// # Errors
///
/// [`GuardError::Unavailable`] when the guard cannot be put in place, and
/// [`GuardError::Spawn`] when the child cannot be started. In both cases no
/// child runs.
pub fn spawn(command: &Command, stdout: Stdio, stderr: Stdio) -> Result<Guarded, GuardError> {
    #[cfg(unix)]
    {
        let (child, watcher) = unix::spawn(command, stdout, stderr, unix::SHELL, &mut |_| {})?;
        Ok(Guarded {
            child,
            status: None,
            watcher: Some(watcher),
        })
    }
    #[cfg(windows)]
    {
        let _ = windows::ensure_job;
        let mut target = Command::new(command.get_program());
        target.args(command.get_args());
        copy_setup(&mut target, command);
        let child = target
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .map_err(GuardError::Spawn)?;
        Ok(Guarded {
            child,
            status: None,
        })
    }
}

/// Carries the variables and the working directory of `source` to `target`.
fn copy_setup(target: &mut Command, source: &Command) {
    for (key, value) in source.get_envs() {
        match value {
            Some(value) => target.env(key, value),
            None => target.env_remove(key),
        };
    }
    if let Some(dir) = source.get_current_dir() {
        target.current_dir(dir);
    }
}

impl Guarded {
    /// The child's process id. On Linux and macOS it is the program's own
    /// id: the launcher becomes the program and keeps its pid.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Takes the child's standard output, when it was [`Stdio::piped`].
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    /// Takes the child's standard error, when it was [`Stdio::piped`].
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    /// Returns the child's exit status if it has exited, without waiting.
    ///
    /// # Errors
    ///
    /// The error of the system's wait call.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.status.is_none()
            && let Some(status) = self.child.try_wait()?
        {
            self.finish(status)?;
        }
        Ok(self.status)
    }

    /// Waits for the child to exit and returns its status.
    ///
    /// # Errors
    ///
    /// The error of the system's wait call.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        let status = self.child.wait()?;
        self.finish(status)?;
        Ok(status)
    }

    /// Stops the child and returns its exit status.
    ///
    /// On Linux and macOS it sends `SIGTERM`, waits up to `grace`, then
    /// sends `SIGKILL`. On Windows it kills the child at once. A child that
    /// already exited is only reaped.
    ///
    /// # Errors
    ///
    /// [`GuardError::Stop`] when a signal or a wait fails.
    pub fn stop(&mut self, grace: Duration) -> Result<ExitStatus, GuardError> {
        if let Some(status) = self.try_wait().map_err(GuardError::Stop)? {
            return Ok(status);
        }
        #[cfg(unix)]
        {
            // The child is ours and not reaped, so its pid is still its own.
            unix::terminate(self.child.id()).map_err(GuardError::Stop)?;
            let deadline = Instant::now() + grace;
            while Instant::now() < deadline {
                if let Some(status) = self.try_wait().map_err(GuardError::Stop)? {
                    return Ok(status);
                }
                thread::sleep(POLL);
            }
        }
        #[cfg(windows)]
        let _ = grace;
        self.child.kill().map_err(GuardError::Stop)?;
        self.wait().map_err(GuardError::Stop)
    }

    /// Records the exit status and lets the watcher go.
    fn finish(&mut self, status: ExitStatus) -> io::Result<()> {
        self.status = Some(status);
        #[cfg(unix)]
        if let Some(mut watcher) = self.watcher.take() {
            watcher.close()?;
        }
        Ok(())
    }
}

impl Drop for Guarded {
    fn drop(&mut self) {
        if self.status.is_none() {
            let _ = self.stop(Duration::ZERO);
        }
    }
}

#[cfg(unix)]
mod unix {
    //! The watcher and the launcher on Linux and macOS.

    use super::GuardError;
    use std::ffi::OsStr;
    use std::io::{self, Read, Write};
    use std::os::unix::process::CommandExt;
    use std::process::{Child, ChildStdin, Command, Stdio};

    /// The shell that runs the watcher and the launcher.
    pub(super) const SHELL: &str = "/bin/sh";

    /// Ignores the polite signals, says it is ready, waits for end of file
    /// on its input, then kills its whole group, itself included.
    pub(super) const WATCHER: &str =
        "trap '' INT HUP TERM; echo ready; while read -r _; do :; done; kill -KILL 0";

    /// Waits for one `go` line, then becomes the program, same pid.
    const LAUNCHER: &str =
        "IFS= read -r line || exit 125; [ \"$line\" = go ] || exit 125; exec \"$0\" \"$@\"";

    /// What the watcher writes once its trap is set.
    const READY: &[u8] = b"ready\n";

    /// The watcher process and the write end of its pipe.
    #[derive(Debug)]
    pub(super) struct Watcher {
        child: Child,
        pipe: Option<ChildStdin>,
    }

    impl Watcher {
        /// Whether the watcher still runs. `waitpid` sees a zombie as dead.
        fn alive(&mut self) -> bool {
            matches!(self.child.try_wait(), Ok(None))
        }

        /// Closes the pipe, so the watcher kills its group, and reaps it.
        pub(super) fn close(&mut self) -> io::Result<()> {
            drop(self.pipe.take());
            self.child.wait().map(|_| ())
        }
    }

    impl Drop for Watcher {
        fn drop(&mut self) {
            let _ = self.close();
        }
    }

    /// Starts `command` in a watched group. `before_release` runs between
    /// the join and the release; tests use it to kill the watcher there.
    pub(super) fn spawn(
        command: &Command,
        stdout: Stdio,
        stderr: Stdio,
        shell: &str,
        before_release: &mut dyn FnMut(&mut Child),
    ) -> Result<(Child, Watcher), GuardError> {
        let unavailable =
            |what: &str, err: io::Error| GuardError::Unavailable(format!("{what}: {err}"));

        let mut child = Command::new(shell)
            .arg("-c")
            .arg(WATCHER)
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| unavailable("could not start the watcher", err))?;
        let pipe = child.stdin.take();
        let ready = child.stdout.take();
        let mut watcher = Watcher { child, pipe };

        let mut said = [0u8; READY.len()];
        ready
            .ok_or_else(|| io::Error::other("no pipe from the watcher"))
            .and_then(|mut out| out.read_exact(&mut said))
            .map_err(|err| unavailable("the watcher did not start", err))?;
        if said != READY {
            return Err(GuardError::Unavailable(
                "the watcher said something else".into(),
            ));
        }
        if !watcher.alive() {
            return Err(GuardError::Unavailable(
                "the watcher died before the start".into(),
            ));
        }
        let group = i32::try_from(watcher.child.id()).map_err(|_| {
            GuardError::Unavailable("the watcher's pid does not fit a group id".into())
        })?;

        let mut launcher = Command::new(shell);
        launcher
            .arg("-c")
            .arg(LAUNCHER)
            .arg(command.get_program())
            .args(command.get_args())
            .process_group(group)
            .stdin(Stdio::piped())
            .stdout(stdout)
            .stderr(stderr);
        super::copy_setup(&mut launcher, command);
        let mut child = launcher.spawn().map_err(GuardError::Spawn)?;

        before_release(&mut watcher.child);

        let released = if watcher.alive() {
            release(&mut child)
        } else {
            Err(io::Error::other("the watcher died before the release"))
        };
        if let Err(err) = released {
            // The launcher never ran the program. Kill and reap it.
            let _ = child.kill();
            let _ = child.wait();
            return Err(unavailable("the guard is gone", err));
        }
        Ok((child, watcher))
    }

    /// Sends the launcher its `go` line and closes its input.
    fn release(child: &mut Child) -> io::Result<()> {
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("no pipe to the launcher"))?;
        input.write_all(b"go\n")
    }

    /// Sends `SIGTERM` to `pid` through the shell's `kill`.
    pub(super) fn terminate(pid: u32) -> io::Result<()> {
        let status = Command::new(SHELL)
            .args([
                OsStr::new("-c"),
                OsStr::new("kill -TERM \"$1\""),
                OsStr::new("sh"),
            ])
            .arg(pid.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "kill -TERM {pid} failed: {status}"
            )))
        }
    }
}

#[cfg(windows)]
mod windows {
    //! The Job Object on Windows.

    use super::GuardError;
    use std::sync::OnceLock;
    use win32job::{ExtendedLimitInfo, Job};

    /// The process's one job. Its handle lives as long as the process.
    static JOB: OnceLock<Result<Job, String>> = OnceLock::new();

    /// Puts the calling process in a kill-on-close job, once.
    pub(super) fn ensure_job() -> Result<(), GuardError> {
        let job = JOB.get_or_init(|| {
            let mut info = ExtendedLimitInfo::new();
            info.limit_kill_on_job_close();
            let job = Job::create_with_limit_info(&info).map_err(|err| err.to_string())?;
            job.assign_current_process()
                .map_err(|err| err.to_string())?;
            Ok(job)
        });
        match job {
            Ok(_) => Ok(()),
            Err(why) => Err(GuardError::Unavailable(format!(
                "no kill-on-close job: {why}"
            ))),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Read;

    fn sleeper() -> Command {
        let mut command = Command::new(unix::SHELL);
        command.args(["-c", "echo up; exec sleep 30"]);
        command
    }

    #[test]
    fn guard_refuses_a_missing_shell() {
        let err = unix::spawn(
            &sleeper(),
            Stdio::null(),
            Stdio::null(),
            "/nonexistent/sh",
            &mut |_| {},
        )
        .expect_err("no shell, no guard");
        assert!(matches!(err, GuardError::Unavailable(_)), "{err}");
    }

    #[test]
    fn guard_refuses_a_watcher_that_dies_before_the_release() {
        let mut command = sleeper();
        command.stdout(Stdio::piped());
        let (read, write) = io::pipe().expect("a pipe");
        let result = unix::spawn(
            &command,
            Stdio::from(write),
            Stdio::null(),
            unix::SHELL,
            &mut |watcher| {
                watcher.kill().expect("kill the watcher");
                watcher.wait().expect("reap the watcher");
            },
        );
        let err = result.expect_err("a dead watcher is a hard error");
        assert!(matches!(err, GuardError::Unavailable(_)), "{err}");
        // The launcher was killed before it ran the program: the pipe holds
        // no "up" and is closed.
        let mut said = String::new();
        let mut read = read;
        read.read_to_string(&mut said).expect("read to end of file");
        assert_eq!(said, "");
    }
}
