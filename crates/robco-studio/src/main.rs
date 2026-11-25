// SPDX-License-Identifier: GPL-3.0-or-later

//! The Studio window.
//!
//! This program will hold the views and editors, drawn through a thin layer
//! of our own over the window toolkit. It holds no logic that a test cannot
//! reach through `studio-core`. Today it has no window: it prints its
//! version, and spike K4 tries the toolkits first. It must never be a
//! dependency of the core or of the `robco` command, and it must never
//! call disks, downloads or QEMU except through the core. It holds no
//! `unsafe` code.
#![forbid(unsafe_code)]

fn main() {
    println!("robco-studio {}", studio_core::VERSION);
}
