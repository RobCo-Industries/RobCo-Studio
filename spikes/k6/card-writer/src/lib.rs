//! Spike K6: list disks, refuse the wrong ones, write and read back the right one.
#![forbid(unsafe_code)]

pub mod linux;
pub mod macos;
pub mod model;
pub mod rules;
pub mod windows;
pub mod write;

/// Runs a program with fixed arguments and returns its standard output.
pub fn run(prog: &str, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(prog)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{prog}: {e}"))?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(input.as_bytes())
            .map_err(|e| format!("{prog}: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("{prog}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{prog} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|_| format!("{prog}: output is not UTF-8"))
}

/// The live disk list of the running system. Read only.
pub fn live_list() -> model::DiskList {
    #[cfg(target_os = "linux")]
    return linux::live();
    #[cfg(target_os = "macos")]
    return macos::live();
    #[cfg(windows)]
    return windows::live();
    #[allow(unreachable_code)]
    model::DiskList {
        problems: vec!["no lister for this system".into()],
        ..Default::default()
    }
}
