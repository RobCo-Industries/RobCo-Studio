// SPDX-License-Identifier: GPL-3.0-or-later

//! The `robco` command.
//!
//! It implements `robco sdk list` and `robco sdk install` of RobCo-Core
//! `docs/spec/STUDIO_CLI.md`, and prints the version for `robco --version`
//! and `robco version`. Every action is delegated to `studio-core`. It must
//! never depend on the window program or any window or toolkit crate. It
//! holds no `unsafe` code.
#![forbid(unsafe_code)]

mod args;
mod cache;
mod index;
mod install;
mod lines;
mod list;

use std::fmt::Display;
use std::io::Write;
use std::process::ExitCode;

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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut stdout, mut stderr) = (std::io::stdout().lock(), std::io::stderr().lock());
    let code = run(&args, &mut stdout, &mut stderr);
    ExitCode::from(code)
}

/// Runs one command line and returns its exit code.
fn run(args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let mut out = Out::new(args::wants_json(args), stdout, stderr);
    let command = match args::parse(args) {
        Ok(Command::Version) => {
            out.text(&format!("robco {}", studio_core::VERSION));
            return 0;
        }
        Ok(c) => c,
        Err(u) => return out.fail(u.command, "usage", &u.detail, 2),
    };
    let name = match command {
        Command::SdkInstall { .. } => "sdk install",
        _ => "sdk list",
    };
    let cache = match cache::folder(|k| std::env::var_os(k)) {
        Ok(c) => c,
        Err(d) => return out.fail(name, "cache: path", &d, 4),
    };
    let result = match &command {
        Command::SdkList { index: given } => list_command(&mut out, &cache, given.as_deref()),
        Command::SdkInstall {
            argument,
            name,
            pick,
            index,
        } => install::run(
            &mut out,
            &cache,
            &install::Request {
                argument,
                name,
                pick: pick.clone(),
                index: index.as_deref(),
                host: studio_core::resolve::running_host(),
            },
        ),
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
) -> Result<(), Failure> {
    let idx = match given {
        None => None,
        Some(g) => Some(index::read(&index::locate(Some(g))?)?.0),
    };
    let items = list::collect(cache, idx.as_ref())?;
    list::write(out, &items);
    Ok(())
}
