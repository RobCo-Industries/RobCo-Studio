// SPDX-License-Identifier: GPL-3.0-or-later

//! `robco-studio [FOLDER]` opens the Studio window, on FOLDER as the project
//! when one is given. `robco-studio --version` prints the version.
#![forbid(unsafe_code)]

use robco_studio::{Studio, theme};
use std::path::PathBuf;

fn main() -> std::process::ExitCode {
    let mut args = std::env::args_os().skip(1);
    let folder = match args.next() {
        Some(a) if a == "--version" => {
            println!("robco-studio {}", studio_core::VERSION);
            return std::process::ExitCode::SUCCESS;
        }
        Some(a) => Some(PathBuf::from(a)),
        None => None,
    };
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size(theme::size::WINDOW)
        .with_min_inner_size(theme::size::WINDOW)
        .with_title("RobCo Studio");
    if let Some(icon) = robco_studio::art::app_icon(theme::size::APP_ICON) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let studio = Studio::new(folder.as_deref());
    let start = Box::new(
        |cc: &eframe::CreationContext<'_>| -> Result<Box<dyn eframe::App>, _> {
            Ok(Box::new(Window::new(studio, &cc.egui_ctx)))
        },
    );
    match eframe::run_native("RobCo Studio", options, start) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("robco-studio: the window did not open: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// The window, with the native menu bar where the system has one.
struct Window {
    studio: Studio,
    #[cfg(target_os = "macos")]
    _menu: Option<robco_studio::native_menu::NativeMenu>,
}

impl Window {
    #[cfg(target_os = "macos")]
    fn new(mut studio: Studio, ctx: &eframe::egui::Context) -> Self {
        let menu = match robco_studio::native_menu::NativeMenu::install(ctx.clone(), &studio) {
            Ok(m) => {
                studio.menu_row = false;
                Some(m)
            }
            Err(e) => {
                eprintln!("robco-studio: no menu bar, the menus stay in the window: {e}");
                None
            }
        };
        Self {
            studio,
            _menu: menu,
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn new(studio: Studio, _ctx: &eframe::egui::Context) -> Self {
        Self { studio }
    }
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        if let Some(m) = &self._menu {
            self.studio.pending.extend(m.take_commands());
        }
        self.studio.ui(ui);
        #[cfg(target_os = "macos")]
        if let Some(m) = &mut self._menu {
            m.sync(&self.studio);
        }
    }
}
