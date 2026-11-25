// SPDX-License-Identifier: GPL-3.0-or-later

//! The `robco` command.
//!
//! This program will hold the command line: argument parsing and JSON
//! output, with every action delegated to `studio-core`. Today it prints
//! the version for `robco --version` and `robco version`, and nothing else
//! is implemented. It must never depend on the window program or any window
//! or toolkit crate. It holds no `unsafe` code.
#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--version"] | ["-V"] | ["version"] => {
            println!("robco {}", studio_core::VERSION);
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: robco --version | robco version");
            eprintln!("No other command exists yet.");
            ExitCode::from(2)
        }
    }
}
