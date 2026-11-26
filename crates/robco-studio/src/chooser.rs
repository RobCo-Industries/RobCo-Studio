// SPDX-License-Identifier: GPL-3.0-or-later

//! The folder chooser behind File, Open folder.
//!
//! The window asks a [`FolderChooser`] for a folder and polls it for the
//! answer. [`SystemChooser`] shows the system's own dialog through `rfd`.
//! Tests pass a fake, since a real dialog cannot be driven headless.
//!
//! How the system dialog runs:
//!
//! - macOS and Windows: the dialog runs on the main thread, at the top of
//!   the frame after the command, before anything of that frame is drawn.
//!   macOS requires the main thread. The dialog is modal and runs its own
//!   event loop while open, so the last frame stays whole on screen and the
//!   window gets no input. The answer is there when [`FolderChooser::start`]
//!   returns.
//! - Linux and the other systems: the dialog runs on a thread of its own,
//!   through the desktop portal, with zenity as `rfd`'s fallback. The window
//!   keeps drawing and polls for the answer each frame; the thread asks for
//!   a repaint when it ends. At run time this needs a D-Bus session with
//!   `xdg-desktop-portal` and a backend, or `zenity` on the path. When
//!   neither answers, `rfd` gives up at once and reports nothing; an empty
//!   answer that fast is told as an error, since no person cancels a
//!   dialog in that time.
//!
//! While a dialog is open the window runs no command.

use std::path::{Path, PathBuf};

/// What came back from a chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// A folder was chosen.
    Chosen(PathBuf),
    /// The person cancelled.
    Cancelled,
    /// No dialog could be shown, with the sentence for the status bar.
    Failed(String),
}

/// Something that asks a person for a folder.
pub trait FolderChooser {
    /// Show the chooser, starting in `start` when it is given. It may block
    /// until the person answers.
    fn start(&mut self, ctx: &eframe::egui::Context, start: Option<&Path>);
    /// The answer, once there is one. None while the chooser is open.
    fn poll(&mut self) -> Option<Choice>;
}

/// The title of the system dialog.
const TITLE: &str = "Open folder";

/// The system's folder dialog.
#[derive(Debug, Default)]
pub struct SystemChooser {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    answer: Option<Choice>,
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    rx: Option<std::sync::mpsc::Receiver<Choice>>,
}

fn dialog(start: Option<&Path>) -> rfd::FileDialog {
    let d = rfd::FileDialog::new().set_title(TITLE);
    match start {
        Some(s) => d.set_directory(s),
        None => d,
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl FolderChooser for SystemChooser {
    fn start(&mut self, _ctx: &eframe::egui::Context, start: Option<&Path>) {
        self.answer = Some(match dialog(start).pick_folder() {
            Some(p) => Choice::Chosen(p),
            None => Choice::Cancelled,
        });
    }

    fn poll(&mut self) -> Option<Choice> {
        self.answer.take()
    }
}

/// An empty answer faster than this came from no dialog at all.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const NO_DIALOG_WITHIN: std::time::Duration = std::time::Duration::from_millis(300);

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl FolderChooser for SystemChooser {
    fn start(&mut self, ctx: &eframe::egui::Context, start: Option<&Path>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let start = start.map(Path::to_path_buf);
        std::thread::spawn(move || {
            let t = std::time::Instant::now();
            let c = match dialog(start.as_deref()).pick_folder() {
                Some(p) => Choice::Chosen(p),
                None if t.elapsed() < NO_DIALOG_WITHIN => Choice::Failed(
                    "No folder dialog opened. Studio needs xdg-desktop-portal or zenity."
                        .to_owned(),
                ),
                None => Choice::Cancelled,
            };
            let _ = tx.send(c);
            ctx.request_repaint();
        });
        self.rx = Some(rx);
    }

    fn poll(&mut self) -> Option<Choice> {
        use std::sync::mpsc::TryRecvError;
        let r = self.rx.as_ref()?.try_recv();
        match r {
            Ok(c) => {
                self.rx = None;
                Some(c)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                Some(Choice::Failed(
                    "The folder dialog stopped without an answer.".to_owned(),
                ))
            }
        }
    }
}

/// Where the chooser starts: the parent of the open project, or the home
/// folder when no project is open.
pub fn start_folder(project: Option<&Path>) -> Option<PathBuf> {
    match project {
        Some(p) => p.parent().map(Path::to_path_buf),
        None => std::env::home_dir(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_beside_the_project_or_at_home() {
        assert_eq!(
            start_folder(Some(Path::new("/a/b"))),
            Some(PathBuf::from("/a"))
        );
        assert_eq!(start_folder(None), std::env::home_dir());
    }
}
