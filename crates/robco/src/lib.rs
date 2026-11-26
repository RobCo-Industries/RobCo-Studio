// SPDX-License-Identifier: GPL-3.0-or-later

//! The `robco` command. `main.rs` only calls [`run`]; the program's tests
//! call it too, with a transport that trusts a test certificate.
//!
//! It implements `robco build`, `robco new`, `robco sdk list` and `robco sdk install` of RobCo-Core
//! `docs/spec/STUDIO_CLI.md`, and prints the version for `robco --version`
//! and `robco version`, with the RobCo-Core commit it was built with. Every action is delegated to `studio-core`. It must
//! never depend on the window program or any window or toolkit crate. It
//! holds no `unsafe` code.
#![forbid(unsafe_code)]

mod args;
mod build;
mod cache;
mod index;
mod install;
mod lines;
mod list;
mod net;
mod new;

use std::ffi::OsString;
use std::fmt::Display;
use std::io::Write;

pub use net::Net;

/// The RobCo-Core commit this program was built with: 40 hex digits, read
/// from the submodule by the build script, or `unknown` when the source
/// had no git files.
pub const ROBCO_CORE_COMMIT: &str = env!("ROBCO_CORE_COMMIT");

use studio_core::codes::{self, ErrorCode};

use crate::args::Command;
use crate::lines::Out;

/// The one failure of a run: a code of the spec's table and its detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The code, such as `index: read`.
    pub code: &'static str,
    /// Text for a person.
    pub detail: String,
}

impl Failure {
    /// A failure with this code and detail.
    pub fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }

    /// The failure of a `studio-core` error, whose text is
    /// `<code>: <detail>`.
    pub fn of<E: ErrorCode + Display>(e: &E) -> Self {
        let code = e.error_code();
        Self::new(code, lines::detail_of(code, &e.to_string()))
    }

    /// The exit code of the code. A code the table lacks is a fault of the
    /// tool.
    pub fn exit(&self) -> u8 {
        codes::exit_code(self.code).unwrap_or(10)
    }
}

/// Runs one command line and returns its exit code. `net` downloads from
/// `https://` URLs, and `var` reads the environment.
pub fn run(
    args: &[String],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    net: &Net,
    var: &dyn Fn(&str) -> Option<OsString>,
) -> u8 {
    let mut out = Out::new(args::wants_json(args), stdout, stderr);
    let command = match args::parse(args) {
        Ok(Command::Version) => {
            out.line(
                "version",
                vec![
                    ("version", lines::Val::s(studio_core::VERSION)),
                    ("robco_core", lines::Val::s(ROBCO_CORE_COMMIT)),
                ],
                &format!(
                    "robco {} robco-core {ROBCO_CORE_COMMIT}",
                    studio_core::VERSION
                ),
            );
            return 0;
        }
        Ok(c) => c,
        Err(u) => return out.fail(u.command, "usage", &u.detail, 2),
    };
    let name = match command {
        Command::SdkInstall { .. } => "sdk install",
        Command::Build { .. } => "build",
        Command::New { .. } => "new",
        _ => "sdk list",
    };
    let cache = match cache::folder(var) {
        Ok(c) => c,
        Err(d) => return out.fail(name, "cache: path", &d, 4),
    };
    let result = match &command {
        Command::SdkList { index: given } => list_command(&mut out, &cache, given.as_deref(), net),
        Command::SdkInstall {
            argument,
            name,
            pick,
            index,
        } => install::run(
            &mut out,
            &cache,
            net,
            &install::Request {
                argument,
                name,
                pick: pick.clone(),
                index: index.as_deref(),
                host: studio_core::resolve::running_host(),
            },
        ),
        Command::Build { index } => match std::env::current_dir() {
            Ok(project) => build::run(
                &mut out,
                &cache,
                net,
                &build::Request {
                    index: index.as_deref(),
                    project: &project,
                    host: studio_core::resolve::running_host(),
                },
            ),
            Err(e) => Err(Failure::new(
                "io: read",
                format!("the current folder: {}", e.kind()),
            )),
        },
        Command::New { template, name } => match std::env::current_dir() {
            Ok(cwd) => new::run(
                &mut out,
                &cache,
                &new::Request {
                    template,
                    name,
                    cwd: &cwd,
                },
            ),
            Err(e) => Err(Failure::new(
                "io: read",
                format!("the current folder: {}", e.kind()),
            )),
        },
        Command::Version => Ok(()),
    };
    match result {
        Ok(()) => 0,
        Err(f) => {
            let exit = f.exit();
            out.fail(name, f.code, &f.detail, exit)
        }
    }
}

/// `robco sdk list`: reads the cache and the index before it writes any
/// line, so a failure writes no `item` line.
fn list_command(
    out: &mut Out<'_>,
    cache: &std::path::Path,
    given: Option<&str>,
    net: &Net,
) -> Result<(), Failure> {
    let idx = match given {
        None => None,
        Some(g) => Some(index::read(&index::locate(Some(g))?, net)?.0),
    };
    let items = list::collect(cache, idx.as_ref())?;
    list::write(out, &items);
    Ok(())
}
