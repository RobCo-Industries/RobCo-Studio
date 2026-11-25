// SPDX-License-Identifier: GPL-3.0-or-later

//! The headless core of Studio.
//!
//! This crate will hold projects, SDK install, builds, asset packing, card
//! images, simulator sessions, the serial monitor and debug sessions. The
//! `robco` command and the window both call it, and every feature lands
//! here with a test before the window shows it. Today it holds the version
//! and [`guard`], which starts a child process that dies with Studio. It must never depend on a window or toolkit crate, nor on the
//! two programs. It holds no `unsafe` code.
#![forbid(unsafe_code)]

pub mod boot;
pub mod guard;

/// The version of Studio, from `workspace.package.version` in the root
/// `Cargo.toml`. Every program prints this one value.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// The core sits on the two libraries below it. Nothing calls them yet;
// these lines keep the dependency direction in the build from day one.
use studio_disk as _;
use studio_sys as _;
