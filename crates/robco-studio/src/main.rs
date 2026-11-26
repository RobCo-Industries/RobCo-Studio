// SPDX-License-Identifier: GPL-3.0-or-later

//! `robco-studio [--theme NAME] [FOLDER]` opens the Studio window, on
//! FOLDER as the project when one is given, with the theme NAME, a theme
//! that ships or one of the person's own, in any case. With a theme asked
//! for, it writes no settings in the run. `robco-studio --version` prints
//! the version.
#![forbid(unsafe_code)]

use robco_studio::Studio;
use robco_studio::screen::{self, Adapter};
use robco_studio::theme;
use std::ffi::OsString;
use std::path::PathBuf;

const USAGE: &str = "usage: robco-studio [--theme NAME] [FOLDER]";

/// What the command line asks for.
#[derive(Debug, PartialEq)]
enum Args {
    /// Print the version.
    Version,
    /// Open the window.
    Open {
        /// The project's folder.
        folder: Option<PathBuf>,
        /// The theme's name, as typed.
        theme: Option<String>,
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
            match args.next().and_then(|n| n.into_string().ok()) {
                Some(n) => asked = Some(n),
                None => return Err(format!("robco-studio: --theme takes a name\n{USAGE}")),
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

/// The place of the theme named `name` in `studio`'s list. The error is
/// the message to print, usage line included.
fn find(studio: &Studio, name: &str) -> Result<usize, String> {
    studio
        .theme_index(name)
        .ok_or_else(|| format!("robco-studio: no theme is named {name}\n{USAGE}"))
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
    let mut studio = Studio::new(folder.as_deref());
    match robco_studio::store::folder(|n| std::env::var_os(n)) {
        Ok(dir) => studio = studio.with_settings(dir),
        Err(e) => eprintln!("robco-studio: no settings folder, nothing is kept: {e}"),
    }
    let asked = match asked.map(|n| find(&studio, &n)).transpose() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    if asked.is_some() {
        studio.hold_settings();
    }
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

    fn on_exit(&mut self) {
        self.studio.save_settings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Result<Args, String> {
        parse(a.iter().map(OsString::from))
    }

    fn open(folder: Option<&str>, theme: Option<&str>) -> Result<Args, String> {
        Ok(Args::Open {
            folder: folder.map(PathBuf::from),
            theme: theme.map(String::from),
        })
    }

    #[test]
    fn the_usage_line() {
        assert_eq!(USAGE, "usage: robco-studio [--theme NAME] [FOLDER]");
        let e = args(&["a", "b"]).unwrap_err();
        assert!(e.ends_with(USAGE), "{e}");
        assert_eq!(args(&["--version"]), Ok(Args::Version));
        assert_eq!(args(&[]), open(None, None));
    }

    #[test]
    fn a_shipped_name_in_any_case() {
        assert_eq!(
            args(&["--theme", "Arcade", "pipes"]),
            open(Some("pipes"), Some("Arcade"))
        );
        let s = Studio::new(None);
        assert_eq!(find(&s, "Arcade"), Ok(4));
        assert_eq!(find(&s, "FLAT"), Ok(5));
    }

    /// A theme of the person's own, from a settings folder made for the
    /// test and removed after it.
    #[test]
    fn a_persons_theme() {
        let d = std::env::temp_dir().join(format!("robco-studio-main-{}", std::process::id()));
        let themes = d.join(robco_studio::store::THEMES);
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(
            themes.join("ember.toml"),
            "version = 1\nname = \"Ember\"\nbase = \"#ff7a1a\"\n",
        )
        .unwrap();
        let s = Studio::new(None).with_settings(d.clone());
        let got = find(&s, "ember");
        std::fs::remove_dir_all(&d).unwrap();
        assert_eq!(got, Ok(6));
    }

    #[test]
    fn a_missing_name_fails_with_the_usage_line() {
        assert_eq!(
            args(&["--theme"]).unwrap_err(),
            format!("robco-studio: --theme takes a name\n{USAGE}")
        );
        assert_eq!(
            find(&Studio::new(None), "ember"),
            Err(format!("robco-studio: no theme is named ember\n{USAGE}"))
        );
    }
}
