// SPDX-License-Identifier: GPL-3.0-or-later

//! The Studio window.
//!
//! Views are drawn through [`kit`], a thin layer of our own over the window
//! toolkit; every colour, size and font comes from [`theme`]. The window
//! holds no logic a test cannot reach: the folder model, the documents and
//! the menus are plain types, and the headless tests drive the whole window
//! through the accessibility tree. It must never be a dependency of the core
//! or of the `robco` command. It opens no network connection, starts no
//! process, and writes no file but the ones a person saves, a theme of
//! their own and its settings file, `settings.toml`.
#![forbid(unsafe_code)]

pub mod app;
pub mod art;
pub mod c_lex;
pub mod chooser;
pub mod document;
pub mod editor;
pub mod kit;
pub mod menus;
#[cfg(target_os = "macos")]
pub mod native_menu;
pub mod palette;
pub mod pass;
pub mod project;
pub mod screen;
pub mod store;
pub mod theme;

pub use app::Studio;
