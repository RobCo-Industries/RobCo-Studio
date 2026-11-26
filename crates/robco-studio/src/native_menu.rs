// SPDX-License-Identifier: GPL-3.0-or-later

//! The macOS menu bar, built from the same model as the row in the window.
//! About, Settings and Quit move into the application menu, where macOS
//! expects them.

use crate::menus::{self, Command, Mods, NativeItem, NativeKind};
use muda::accelerator::{Accelerator, Code, Modifiers};

/// Commands that live in the application menu on macOS.
const APP_MENU: [Command; 3] = [Command::About, Command::Settings, Command::Quit];

/// The installed menu bar. Keep it alive for as long as the window runs.
pub struct NativeMenu {
    _menu: muda::Menu,
    chosen: std::sync::mpsc::Receiver<String>,
    /// Every item, in the order of [`menus::native_items`].
    items: Vec<Built>,
}

fn code(key: &str) -> Option<Code> {
    let c = match key {
        "A" => Code::KeyA,
        "B" => Code::KeyB,
        "D" => Code::KeyD,
        "E" => Code::KeyE,
        "F" => Code::KeyF,
        "G" => Code::KeyG,
        "I" => Code::KeyI,
        "J" => Code::KeyJ,
        "K" => Code::KeyK,
        "L" => Code::KeyL,
        "M" => Code::KeyM,
        "N" => Code::KeyN,
        "O" => Code::KeyO,
        "P" => Code::KeyP,
        "Q" => Code::KeyQ,
        "R" => Code::KeyR,
        "S" => Code::KeyS,
        "T" => Code::KeyT,
        "W" => Code::KeyW,
        "Z" => Code::KeyZ,
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "=" => Code::Equal,
        "-" => Code::Minus,
        "," => Code::Comma,
        "." => Code::Period,
        "F1" => Code::F1,
        "F3" => Code::F3,
        "F5" => Code::F5,
        "F6" => Code::F6,
        "F8" => Code::F8,
        "F9" => Code::F9,
        "F10" => Code::F10,
        _ => return None,
    };
    Some(c)
}

fn accelerator(it: &NativeItem) -> Option<Accelerator> {
    let s = it.shortcut?;
    let mods = match s.mods {
        Mods::None => None,
        Mods::Cmd => Some(Modifiers::SUPER),
        Mods::CmdShift => Some(Modifiers::SUPER | Modifiers::SHIFT),
        Mods::CmdAlt => Some(Modifiers::SUPER | Modifiers::ALT),
        Mods::Shift => Some(Modifiers::SHIFT),
        Mods::CtrlAlt => Some(Modifiers::CONTROL | Modifiers::ALT),
        Mods::Ctrl => Some(Modifiers::CONTROL),
    };
    Some(Accelerator::new(mods, code(s.key)?))
}

/// A built item of the bar.
enum Built {
    Plain(muda::MenuItem),
    Check(muda::CheckMenuItem),
}

impl Built {
    fn new(it: &NativeItem) -> Built {
        let id = it.id.clone();
        match it.kind {
            NativeKind::Plain => Built::Plain(muda::MenuItem::with_id(
                id,
                it.title,
                it.enabled,
                accelerator(it),
            )),
            NativeKind::Check => Built::Check(muda::CheckMenuItem::with_id(
                id,
                it.title,
                it.enabled,
                it.checked,
                accelerator(it),
            )),
        }
    }

    fn entry(&self) -> &dyn muda::IsMenuItem {
        match self {
            Built::Plain(m) => m,
            Built::Check(m) => m,
        }
    }

    fn is_enabled(&self) -> bool {
        match self {
            Built::Plain(m) => m.is_enabled(),
            Built::Check(m) => m.is_enabled(),
        }
    }

    fn is_checked(&self) -> bool {
        match self {
            Built::Plain(_) => false,
            Built::Check(m) => m.is_checked(),
        }
    }

    /// Give the item the state of `it`. The mark is read back from the
    /// item, not from a copy: a click flips it before the program hears
    /// of it, and a refused command must not leave it flipped.
    fn sync(&self, it: &NativeItem) {
        if self.is_enabled() != it.enabled {
            match self {
                Built::Plain(m) => m.set_enabled(it.enabled),
                Built::Check(m) => m.set_enabled(it.enabled),
            }
        }
        if self.is_checked() != it.checked
            && let Built::Check(m) = self
        {
            m.set_checked(it.checked);
        }
    }
}

/// Build the items of `list`, unattached to any menu.
fn build(list: &[NativeItem]) -> Vec<Built> {
    list.iter().map(Built::new).collect()
}

impl NativeMenu {
    /// Build the bar from the model and give it to the application. Call
    /// once, on the main thread, after the window's event loop exists.
    pub fn install(
        ctx: eframe::egui::Context,
        studio: &crate::Studio,
    ) -> Result<NativeMenu, String> {
        let model = menus::menus();
        let list = menus::native_items(studio);
        let items = build(&list);
        let bar = muda::Menu::new();
        let app = muda::Submenu::new("RobCo Studio", true);
        for (it, b) in list.iter().zip(&items) {
            if APP_MENU.contains(&it.command) {
                app.append(b.entry()).map_err(|e| e.to_string())?;
            }
        }
        bar.append(&app).map_err(|e| e.to_string())?;
        for (m, menu) in model.iter().enumerate() {
            let sub = muda::Submenu::new(menu.title, true);
            for (it, b) in list.iter().zip(&items) {
                if it.menu == m && !APP_MENU.contains(&it.command) {
                    sub.append(b.entry()).map_err(|e| e.to_string())?;
                }
            }
            bar.append(&sub).map_err(|e| e.to_string())?;
        }
        bar.init_for_nsapp();
        // A choice wakes the window, which is idle until something happens.
        let (tx, chosen) = std::sync::mpsc::channel();
        muda::MenuEvent::set_event_handler(Some(move |e: muda::MenuEvent| {
            let _ = tx.send(e.id().as_ref().to_owned());
            ctx.request_repaint();
        }));
        Ok(NativeMenu {
            _menu: bar,
            chosen,
            items,
        })
    }

    /// Give each item the state and mark the model gives it now, by
    /// [`menus::native_items`]. Call after each frame, whatever changed
    /// the state.
    pub fn sync(&mut self, studio: &crate::Studio) {
        for (it, b) in menus::native_items(studio).iter().zip(&self.items) {
            b.sync(it);
        }
    }

    /// The commands chosen in the bar since the last call.
    pub fn take_commands(&self) -> Vec<Command> {
        let mut out = Vec::new();
        while let Ok(id) = self.chosen.try_recv() {
            if let Some(c) = menus::command_for_id(&id) {
                out.push(c);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shortcut_has_a_key() {
        for m in menus::menus() {
            for it in m.items {
                if let Some(s) = it.shortcut {
                    assert!(code(s.key).is_some(), "{}", s.key);
                }
            }
        }
    }

    /// The `muda` items built from the list carry its kind, enabled state
    /// and mark, and keep the model's mark after a click flipped it.
    /// `muda` makes items without a run loop as long as they are not
    /// attached to a menu.
    #[test]
    fn built_items_follow_the_list() {
        let mut s = crate::Studio::new(None);
        let list = menus::native_items(&s);
        let items = build(&list);
        for (it, b) in list.iter().zip(&items) {
            assert_eq!(
                matches!(b, Built::Check(_)),
                it.kind == NativeKind::Check,
                "{}",
                it.title
            );
            assert_eq!(b.is_enabled(), it.enabled, "{}", it.title);
            assert_eq!(b.is_checked(), it.checked, "{}", it.title);
        }
        let hl = list
            .iter()
            .position(|i| i.command == Command::HighLegibility)
            .unwrap();
        assert!(!items[hl].is_checked());
        s.theme.high_legibility = true;
        let list = menus::native_items(&s);
        for (it, b) in list.iter().zip(&items) {
            b.sync(it);
        }
        assert!(items[hl].is_checked());
        // A click flips the mark by itself; a sync puts the model's back.
        if let Built::Check(m) = &items[hl] {
            m.set_checked(false);
        }
        items[hl].sync(&list[hl]);
        assert!(items[hl].is_checked());
        let z3 = list
            .iter()
            .position(|i| i.command == Command::SimZoom(3))
            .unwrap();
        assert!(!items[z3].is_enabled());
        assert!(!items[z3].is_checked());
    }
}
