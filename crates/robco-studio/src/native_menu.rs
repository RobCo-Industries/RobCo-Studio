// SPDX-License-Identifier: GPL-3.0-or-later

//! The macOS menu bar, built from the same model as the row in the window.
//! About, Settings and Quit move into the application menu, where macOS
//! expects them.

use crate::menus::{self, Command, Item, Mods};
use muda::accelerator::{Accelerator, Code, Modifiers};

/// Commands that live in the application menu on macOS.
const APP_MENU: [Command; 3] = [Command::About, Command::Settings, Command::Quit];

/// The installed menu bar. Keep it alive for as long as the window runs.
pub struct NativeMenu {
    _menu: muda::Menu,
    chosen: std::sync::mpsc::Receiver<String>,
    /// Every item, with the state it was last given.
    items: Vec<(Item, muda::MenuItem, bool)>,
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

fn accelerator(it: &Item) -> Option<Accelerator> {
    // An item that cannot act carries no key, so the key still reaches the window.
    if it.unavailable.is_some() {
        return None;
    }
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

fn item(id: String, it: &Item) -> muda::MenuItem {
    muda::MenuItem::with_id(id, it.text, it.unavailable.is_none(), accelerator(it))
}

impl NativeMenu {
    /// Build the bar from the model and give it to the application. Call
    /// once, on the main thread, after the window's event loop exists.
    pub fn install(ctx: eframe::egui::Context) -> Result<NativeMenu, String> {
        let model = menus::menus();
        let bar = muda::Menu::new();
        let mut items = Vec::new();
        let app = muda::Submenu::new("RobCo Studio", true);
        for (m, menu) in model.iter().enumerate() {
            for (i, it) in menu.items.iter().enumerate() {
                if APP_MENU.contains(&it.command) {
                    let mi = item(menus::item_id(m, i), it);
                    app.append(&mi).map_err(|e| e.to_string())?;
                    items.push((*it, mi, it.unavailable.is_none()));
                }
            }
        }
        bar.append(&app).map_err(|e| e.to_string())?;
        for (m, menu) in model.iter().enumerate() {
            let sub = muda::Submenu::new(menu.title, true);
            for (i, it) in menu.items.iter().enumerate() {
                if !APP_MENU.contains(&it.command) {
                    let mi = item(menus::item_id(m, i), it);
                    sub.append(&mi).map_err(|e| e.to_string())?;
                    items.push((*it, mi, it.unavailable.is_none()));
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

    /// Give each item the state the window's own menu gives it now, by
    /// [`crate::Studio::unavailable`]. Call after each frame.
    pub fn sync(&mut self, studio: &crate::Studio) {
        for (it, mi, on) in &mut self.items {
            let now = studio.unavailable(it).is_none();
            if now != *on {
                mi.set_enabled(now);
                *on = now;
            }
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
}
