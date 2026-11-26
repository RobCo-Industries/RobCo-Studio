// SPDX-License-Identifier: GPL-3.0-or-later

//! The `robco` program. Everything is in the library of this crate.
#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut stdout, mut stderr) = (std::io::stdout().lock(), std::io::stderr().lock());
    let net = robco::Net::system();
    let code = robco::run(&args, &mut stdout, &mut stderr, &net, &|k| {
        std::env::var_os(k)
    });
    ExitCode::from(code)
}
