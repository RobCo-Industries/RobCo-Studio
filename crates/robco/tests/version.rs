// SPDX-License-Identifier: GPL-3.0-or-later

//! The command prints the workspace version and the RobCo-Core commit it
//! was built with, and fails on anything else.
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
    let expected = format!(
        "robco {} robco-core {}\n",
        studio_core::VERSION,
        robco::ROBCO_CORE_COMMIT
    );
    for args in [&["--version"][..], &["-V"], &["version"]] {
        let out = robco(args);
        assert!(out.status.success(), "{args:?}");
        let stdout = String::from_utf8(out.stdout).expect("utf-8");
        assert_eq!(stdout.replace("\r\n", "\n"), expected, "{args:?}");
        assert!(out.stderr.is_empty(), "{args:?}");
    }
}

#[test]
fn json_form_is_one_version_line() {
    let expected = format!(
        "{{\"v\":1,\"kind\":\"version\",\"version\":\"{}\",\"robco_core\":\"{}\"}}\n",
        studio_core::VERSION,
        robco::ROBCO_CORE_COMMIT
    );
    for args in [
        &["--version", "--json"][..],
        &["--json", "-V"],
        &["version", "--json"],
    ] {
        let out = robco(args);
        assert!(out.status.success(), "{args:?}");
        let stdout = String::from_utf8(out.stdout).expect("utf-8");
        assert_eq!(stdout.replace("\r\n", "\n"), expected, "{args:?}");
        assert!(out.stderr.is_empty(), "{args:?}");
    }
}

/// The commit is the submodule's checked-out commit, read by the build
/// script. This test asks git, which the program itself never does. A
/// source archive without git files gives `unknown`, and git fails there.
#[test]
fn commit_is_the_submodules_head() {
    let core = concat!(env!("CARGO_MANIFEST_DIR"), "/../../external/robco-core");
    let Ok(out) = Command::new("git")
        .args(["-C", core, "rev-parse", "HEAD"])
        .output()
    else {
        assert_eq!(robco::ROBCO_CORE_COMMIT, "unknown");
        return;
    };
    if !out.status.success() {
        assert_eq!(robco::ROBCO_CORE_COMMIT, "unknown");
        return;
    }
    let head = String::from_utf8(out.stdout).expect("utf-8");
    assert_eq!(robco::ROBCO_CORE_COMMIT, head.trim());
    assert_eq!(robco::ROBCO_CORE_COMMIT.len(), 40);
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
    // With --json the usage failure is a result line, not a version line.
    let out = robco(&["--version", "--json", "x"]);
    assert_eq!(out.status.code(), Some(2));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(stdout.contains("\"kind\":\"result\""), "{stdout}");
}
