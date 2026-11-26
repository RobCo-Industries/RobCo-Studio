// SPDX-License-Identifier: GPL-3.0-or-later

//! The command prints the workspace version, and fails on anything else.
#![allow(
    clippy::expect_used,
    reason = "a test file; a failed expect is a failed test"
)]

use std::process::Command;

fn robco(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_robco"))
        .args(args)
        .output()
        .expect("robco runs")
}

#[test]
fn version_flag_and_command_print_the_version() {
    let expected = format!("robco {}\n", studio_core::VERSION);
    for args in [&["--version"][..], &["-V"], &["version"]] {
        let out = robco(args);
        assert!(out.status.success(), "{args:?}");
        let stdout = String::from_utf8(out.stdout).expect("utf-8");
        assert_eq!(stdout.replace("\r\n", "\n"), expected, "{args:?}");
        assert!(out.stderr.is_empty(), "{args:?}");
    }
}

#[test]
fn version_matches_the_workspace_manifest() {
    assert_eq!(studio_core::VERSION, env!("CARGO_PKG_VERSION"));
}

#[test]
fn anything_else_fails() {
    for args in [&[][..], &["bogus"], &["--version", "extra"]] {
        let out = robco(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
    }
}
