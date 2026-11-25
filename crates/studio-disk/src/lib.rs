// SPDX-License-Identifier: GPL-3.0-or-later

//! Disk listing and raw disk access for each system.
//!
//! This crate will hold the list of disks, raw block reads and writes on
//! macOS, Windows and Linux, the checks that refuse a system disk, and the
//! elevated helper mode. Today it is an empty shell: spike K6 comes first.
//! It must never depend on the core, the programs, or any window or
//! toolkit crate.
//!
//! `unsafe` is allowed here because raw disk access calls each system's
//! own interface through `libc` and `windows-sys`. Every `unsafe` block
//! carries a `SAFETY` comment, which clippy checks.
#![allow(unsafe_code, reason = "raw disk access calls system interfaces")]
