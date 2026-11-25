// SPDX-License-Identifier: GPL-3.0-or-later

//! RobCo-Core's C, linked into Studio, with a thin Rust binding.
//!
//! This crate will hold a `build.rs` that compiles a fixed list of
//! RobCo-Core's C files from a pinned submodule, the hand-written
//! `extern "C"` declarations, and safe wrappers around them: image sealing
//! and checks, CRC-32, INI and FAT through FatFs. It holds no logic of its
//! own beyond the binding. Today it is an empty shell: spike K3 decides how
//! the C gets in, and no C is linked yet. It must never depend on another
//! crate of this workspace, nor on any window or toolkit crate.
//!
//! `unsafe` is allowed here because calling C through `extern "C"` needs
//! it. Every `unsafe` block carries a `SAFETY` comment, which clippy checks.
#![allow(unsafe_code, reason = "calls into RobCo-Core's C need unsafe")]
