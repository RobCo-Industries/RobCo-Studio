// SPDX-License-Identifier: GPL-3.0-or-later

//! A guarded child dies with the process that started it.
//!
//! This test program plays three roles, picked by an environment variable:
//! the test itself, a helper that starts a stand-in through the guard, and
//! the stand-in, which only sleeps. No QEMU runs.
//!
//! The test reads the helper's output. The stand-in writes to the same pipe,
//! so the test reads end of file only once the helper and the stand-in are
//! both gone. A zombie has closed its files, so it counts as gone, and no
//! process id is ever read or signalled.
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test file; a failed expect is a failed test"
)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use studio_core::guard;

const ROLE: &str = "STUDIO_GUARD_TEST_ROLE";
const ENTRY: &str = "guard_role_entry";
/// The stand-in exits on its own after this, so a leak does not last.
const STAND_IN_LIFE: Duration = Duration::from_secs(60);
/// How long the test gives the stand-in to be gone.
const GONE_WITHIN: Duration = Duration::from_secs(5);

/// One helper at a time, so no helper inherits another's pipe on Windows.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// This test program, in the given role.
fn this_program(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("the test program's path"));
    command
        .args([
            ENTRY,
            "--exact",
            "--nocapture",
            "--test-threads=1",
            "--quiet",
        ])
        .env(ROLE, role);
    command
}

/// Runs the role named by the environment, or nothing in a normal run.
#[test]
fn guard_role_entry() {
    match std::env::var(ROLE).as_deref() {
        Ok("stand-in") => stand_in(),
        Ok(case) => helper(case),
        Err(_) => {}
    }
}

fn stand_in() {
    println!("stand-in up");
    let start = Instant::now();
    while start.elapsed() < STAND_IN_LIFE {
        thread::sleep(Duration::from_millis(100));
    }
}

fn helper(case: &str) {
    let mut guarded = guard::spawn(
        &this_program("stand-in"),
        Stdio::inherit(),
        Stdio::inherit(),
    )
    .expect("the guard is in place");
    println!("helper ready");
    // The test says when: once it saw the stand-in up.
    let mut go = String::new();
    std::io::stdin().read_line(&mut go).expect("the test's go");
    match case {
        "wait" => loop {
            thread::park();
        },
        "exit" => std::process::exit(0),
        "panic" => panic!("the helper panics on purpose"),
        "stop" => {
            guarded.stop(Duration::from_secs(3)).expect("stop");
            println!("helper stopped it");
            std::process::exit(0);
        }
        other => panic!("unknown case {other}"),
    }
}

/// The helper's output, line by line, with `None` at end of file.
fn lines(helper: &mut Child) -> Receiver<Option<String>> {
    let out = helper.stdout.take().expect("the helper's output");
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(out).lines() {
            match line {
                Ok(line) => {
                    if send.send(Some(line)).is_err() {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = send.send(None);
    });
    receive
}

/// Starts a helper and waits until both it and the stand-in are up.
fn start_helper(case: &str) -> (Child, Receiver<Option<String>>) {
    let mut helper = this_program(case)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the helper");
    let output = lines(&mut helper);
    let deadline = Instant::now() + Duration::from_secs(20);
    let (mut helper_up, mut stand_in_up) = (false, false);
    while !(helper_up && stand_in_up) {
        let left = deadline.saturating_duration_since(Instant::now());
        match output.recv_timeout(left) {
            Ok(Some(line)) => {
                helper_up |= line == "helper ready";
                stand_in_up |= line == "stand-in up";
            }
            Ok(None) => panic!("the helper ended before the stand-in was up"),
            Err(RecvTimeoutError::Timeout) => panic!("the helper and the stand-in did not start"),
            Err(RecvTimeoutError::Disconnected) => panic!("the reader stopped"),
        }
    }
    let mut go = helper.stdin.take().expect("the helper's input");
    go.write_all(b"go\n").expect("tell the helper to go on");
    (helper, output)
}

/// Waits for end of file on the shared pipe: the stand-in is gone.
fn assert_stand_in_gone(output: &Receiver<Option<String>>) -> Vec<String> {
    let deadline = Instant::now() + GONE_WITHIN;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match output.recv_timeout(left) {
            Ok(Some(line)) => seen.push(line),
            Ok(None) => return seen,
            Err(_) => panic!("the stand-in still runs {GONE_WITHIN:?} after; it said {seen:?}"),
        }
    }
}

#[test]
fn guard_child_dies_when_the_helper_is_killed_hard() {
    let _serial = serial();
    let (mut helper, output) = start_helper("wait");
    // SIGKILL on Linux and macOS, TerminateProcess on Windows.
    helper.kill().expect("kill the helper");
    helper.wait().expect("reap the helper");
    assert_stand_in_gone(&output);
}

#[test]
fn guard_child_dies_when_the_helper_exits_without_stopping_it() {
    let _serial = serial();
    let (mut helper, output) = start_helper("exit");
    assert_stand_in_gone(&output);
    assert!(helper.wait().expect("reap the helper").success());
}

#[test]
fn guard_child_dies_when_the_helper_panics() {
    let _serial = serial();
    let (mut helper, output) = start_helper("panic");
    assert_stand_in_gone(&output);
    assert!(!helper.wait().expect("reap the helper").success());
}

#[test]
fn guard_stop_ends_the_child_and_the_helper_goes_on() {
    let _serial = serial();
    let (mut helper, output) = start_helper("stop");
    let seen = assert_stand_in_gone(&output);
    assert!(
        seen.iter().any(|line| line == "helper stopped it"),
        "{seen:?}"
    );
    assert!(helper.wait().expect("reap the helper").success());
}

#[test]
fn guard_child_that_exits_by_itself_is_reaped() {
    let _serial = serial();
    let mut command = this_program("none");
    command.env_remove(ROLE);
    let mut guarded = guard::spawn(&command, Stdio::null(), Stdio::null()).expect("spawn");
    let status = guarded.wait().expect("wait");
    assert!(status.success());
    assert_eq!(guarded.try_wait().expect("try_wait"), Some(status));
}

/// Starts the stand-in in this process, reads its first line, stops it.
#[cfg(unix)]
fn stop_stand_in(command: &Command, grace: Duration) -> (std::process::ExitStatus, Duration) {
    let mut guarded = guard::spawn(command, Stdio::piped(), Stdio::null()).expect("spawn");
    let out = guarded.take_stdout().expect("the stand-in's output");
    let up = BufReader::new(out)
        .lines()
        .map_while(Result::ok)
        .any(|line| line == "stand-in up");
    assert!(up, "the stand-in never said it is up");
    let start = Instant::now();
    let status = guarded.stop(grace).expect("stop");
    (status, start.elapsed())
}

#[cfg(unix)]
#[test]
fn guard_stop_is_polite_first() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = serial();
    let grace = Duration::from_secs(3);
    let (status, took) = stop_stand_in(&this_program("stand-in"), grace);
    assert_eq!(status.signal(), Some(15), "{status}");
    assert!(took < grace, "{took:?}");
}

#[cfg(unix)]
#[test]
fn guard_stop_kills_a_child_that_ignores_the_polite_signal() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = serial();
    let program = this_program("stand-in");
    // An ignored signal stays ignored across exec.
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "trap '' TERM; exec \"$0\" \"$@\""])
        .arg(program.get_program())
        .args(program.get_args())
        .env(ROLE, "stand-in");
    let grace = Duration::from_millis(300);
    let (status, took) = stop_stand_in(&command, grace);
    assert_eq!(status.signal(), Some(9), "{status}");
    assert!(took >= grace, "{took:?}");
}
