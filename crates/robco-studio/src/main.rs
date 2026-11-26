// SPDX-License-Identifier: GPL-3.0-or-later

//! `robco-studio [--theme NAME] [FOLDER]` opens the Studio window, on
//! FOLDER as the project when one is given, with the theme NAME, one of the
//! themes that ship, in any case. It writes nothing. `robco-studio
//! --version` prints the version.
#![forbid(unsafe_code)]

use robco_studio::Studio;
use robco_studio::screen::{self, Adapter};
use robco_studio::theme;
use std::ffi::OsString;
use std::path::PathBuf;

const USAGE: &str = "usage: robco-studio [--theme green|amber|blue|white|arcade|flat] [FOLDER]";

/// What the command line asks for.
#[derive(Debug, PartialEq)]
enum Args {
    /// Print the version.
    Version,
    /// Open the window.
    Open {
        /// The project's folder.
        folder: Option<PathBuf>,
        /// The theme, by its place among the themes that ship.
        theme: Option<usize>,
    },
}

/// Read the arguments, the program's name left out. An error is the
/// message to print, usage line included.
fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
    let mut args = args.into_iter();
    let mut folder = None;
    let mut asked = None;
    while let Some(a) = args.next() {
        if a == "--version" {
            return Ok(Args::Version);
        }
        if a == "--theme" {
            let name = args.next();
            match name
                .as_ref()
                .and_then(|n| n.to_str())
                .and_then(theme::shipped)
            {
                Some(i) => asked = Some(i),
                None => {
                    return Err(format!(
                        "robco-studio: --theme takes green, amber, blue, white, arcade or flat\n{USAGE}"
                    ));
                }
            }
        } else if folder.is_none() {
            folder = Some(PathBuf::from(a));
        } else {
            return Err(format!("robco-studio: one folder at most\n{USAGE}"));
        }
    }
    Ok(Args::Open {
        folder,
        theme: asked,
    })
}

fn main() -> std::process::ExitCode {
    let (folder, asked) = match parse(std::env::args_os().skip(1)) {
        Ok(Args::Version) => {
            println!("robco-studio {}", studio_core::VERSION);
            return std::process::ExitCode::SUCCESS;
        }
        Ok(Args::Open { folder, theme }) => (folder, theme),
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::FAILURE;
        }
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
            let mut studio = studio;
            let rs = cc.wgpu_render_state.as_ref();
            let adapter = rs.map_or(Adapter::Unknown, |rs| {
                Adapter::of(rs.adapter.get_info().device_type)
            });
            // The pass is made only where it can run.
            let gpu = rs.filter(|_| adapter == Adapter::Hardware);
            let reduce = screen::system_reduce_motion();
            studio.start_screen(&cc.egui_ctx, adapter, gpu, reduce, asked);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Result<Args, String> {
        parse(a.iter().map(OsString::from))
    }

    #[test]
    fn the_usage_line() {
        assert_eq!(
            USAGE,
            "usage: robco-studio [--theme green|amber|blue|white|arcade|flat] [FOLDER]"
        );
        let e = args(&["a", "b"]).unwrap_err();
        assert!(e.ends_with(USAGE), "{e}");
    }

    #[test]
    fn a_shipped_name_in_any_case() {
        assert_eq!(
            args(&["--theme", "Arcade", "pipes"]),
            Ok(Args::Open {
                folder: Some(PathBuf::from("pipes")),
                theme: Some(4)
            })
        );
        assert_eq!(
            args(&["--theme", "FLAT"]),
            Ok(Args::Open {
                folder: None,
                theme: Some(5)
            })
        );
        assert_eq!(
            args(&[]),
            Ok(Args::Open {
                folder: None,
                theme: None
            })
        );
        assert_eq!(args(&["--version"]), Ok(Args::Version));
    }

    #[test]
    fn a_missing_name_fails_with_the_usage_line() {
        for a in [
            &["--theme"][..],
            &["--theme", "ember"],
            &["--theme", "live"],
        ] {
            let e = args(a).unwrap_err();
            assert_eq!(
                e,
                format!(
                    "robco-studio: --theme takes green, amber, blue, white, arcade or flat\n{USAGE}"
                ),
                "{a:?}"
            );
        }
    }
}
