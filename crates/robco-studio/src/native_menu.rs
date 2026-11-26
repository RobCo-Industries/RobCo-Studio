// SPDX-License-Identifier: GPL-3.0-or-later

//! The macOS menu bar, built from the same model as the row in the window.
//! About, Settings and Quit move into the application menu, where macOS
//! expects them. Items of a group go into a submenu, and every item with a
//! state is a check item whose mark follows the model. When the list of
//! items changes shape, as when the list of themes grows, the bar is built
//! again.

use crate::menus::{self, Command, Mods, NativeItem, NativeKind};
use muda::accelerator::{Accelerator, Code, Modifiers};

/// Commands that live in the application menu on macOS.
const APP_MENU: [Command; 3] = [Command::About, Command::Settings, Command::Quit];

/// The installed menu bar. Keep it alive for as long as the window runs.
pub struct NativeMenu {
    _menu: muda::Menu,
    chosen: std::sync::mpsc::Receiver<String>,
    /// The list the bar was built from.
    list: Vec<NativeItem>,
    /// Every item, in the order of `list`.
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
                &it.title,
                it.enabled,
                accelerator(it),
            )),
            NativeKind::Check => Built::Check(muda::CheckMenuItem::with_id(
                id,
                &it.title,
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

/// Build the bar from `list`: the application menu, then each menu of
/// `model` with its submenus.
fn bar(model: &[menus::Menu], list: &[NativeItem]) -> Result<(muda::Menu, Vec<Built>), String> {
    let items = build(list);
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
        let mut group: Option<(&str, muda::Submenu)> = None;
        for (it, b) in list.iter().zip(&items) {
            if it.menu != m || APP_MENU.contains(&it.command) {
                continue;
            }
            // Close the open group when this item is not part of it.
            if group.as_ref().map(|(g, _)| Some(*g)) != Some(it.group)
                && let Some((_, g)) = group.take()
            {
                sub.append(&g).map_err(|e| e.to_string())?;
            }
            if group.is_none()
                && let Some(g) = it.group
            {
                group = Some((g, muda::Submenu::new(g, true)));
            }
            let into: &muda::Submenu = match &group {
                Some((_, g)) => g,
                None => &sub,
            };
            if it.separator {
                into.append(&muda::PredefinedMenuItem::separator())
                    .map_err(|e| e.to_string())?;
            }
            into.append(b.entry()).map_err(|e| e.to_string())?;
        }
        if let Some((_, g)) = group.take() {
            sub.append(&g).map_err(|e| e.to_string())?;
        }
        bar.append(&sub).map_err(|e| e.to_string())?;
    }
    Ok((bar, items))
}

/// What a sync does with the bar.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// Give each item its state and mark.
    Sync,
    /// Build the bar again from the new list.
    Rebuild,
}

/// What the bar built from `old` does with `new`.
fn step(old: &[NativeItem], new: &[NativeItem]) -> Step {
    if menus::shape_changed(old, new) {
        Step::Rebuild
    } else {
        Step::Sync
    }
}

impl NativeMenu {
    /// Build the bar from the model and give it to the application. Call
    /// once, on the main thread, after the window's event loop exists.
    pub fn install(
        ctx: eframe::egui::Context,
        studio: &crate::Studio,
    ) -> Result<NativeMenu, String> {
        let list = menus::native_items(studio);
        let (menu, items) = bar(&studio.menus(), &list)?;
        menu.init_for_nsapp();
        // A choice wakes the window, which is idle until something happens.
        let (tx, chosen) = std::sync::mpsc::channel();
        muda::MenuEvent::set_event_handler(Some(move |e: muda::MenuEvent| {
            let _ = tx.send(e.id().as_ref().to_owned());
            ctx.request_repaint();
        }));
        Ok(NativeMenu {
            _menu: menu,
            chosen,
            list,
            items,
        })
    }

    /// Give each item the state and mark the model gives it now, by
    /// [`menus::native_items`], or build the bar again when the items
    /// changed. Call after each frame, whatever changed the state.
    pub fn sync(&mut self, studio: &crate::Studio) {
        let list = menus::native_items(studio);
        match step(&self.list, &list) {
            Step::Sync => {
                for (it, b) in list.iter().zip(&self.items) {
                    b.sync(it);
                }
            }
            Step::Rebuild => match bar(&studio.menus(), &list) {
                Ok((menu, items)) => {
                    menu.init_for_nsapp();
                    self._menu = menu;
                    self.items = items;
                }
                Err(e) => eprintln!("robco-studio: the menu bar was not built again: {e}"),
            },
        }
        self.list = list;
    }

    /// The commands chosen in the bar since the last call.
    pub fn take_commands(&self) -> Vec<Command> {
        let mut out = Vec::new();
        while let Ok(id) = self.chosen.try_recv() {
            if let Some(it) = self.list.iter().find(|i| i.id == id) {
                out.push(it.command);
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
        let ch = list
            .iter()
            .position(|i| i.command == Command::CompactHeader)
            .unwrap();
        assert!(!items[ch].is_checked());
        s.compact_header = true;
        let list = menus::native_items(&s);
        for (it, b) in list.iter().zip(&items) {
            b.sync(it);
        }
        assert!(items[ch].is_checked());
        // A click flips the mark by itself; a sync puts the model's back.
        if let Built::Check(m) = &items[ch] {
            m.set_checked(false);
        }
        items[ch].sync(&list[ch]);
        assert!(items[ch].is_checked());
        // The theme in use is marked, and Edit theme has its key.
        let green = list
            .iter()
            .position(|i| i.command == Command::Theme(menus::ThemeId::At(0)))
            .unwrap();
        assert!(items[green].is_checked());
        let edit = list
            .iter()
            .find(|i| i.command == Command::ThemeEdit)
            .unwrap();
        assert!(accelerator(edit).is_some());
        let z3 = list
            .iter()
            .position(|i| i.command == Command::SimZoom(3))
            .unwrap();
        assert!(!items[z3].is_enabled());
        assert!(!items[z3].is_checked());
    }

    /// A list of themes that grows builds the bar again, with the new row
    /// in View, Theme; a change of mark alone is a sync. Pure data: no
    /// menu is put on the screen.
    #[test]
    fn a_growing_list_builds_the_bar_again() {
        let mut s = crate::Studio::new(None);
        let before = menus::native_items(&s);
        s.compact_header = true;
        let marked = menus::native_items(&s);
        assert_eq!(step(&before, &marked), Step::Sync);
        let mut grown = s.themes.clone();
        for name in ["Ember", "Dusk"] {
            grown.push(crate::theme::Spec {
                name: name.to_owned().into(),
                ..crate::theme::SHIPPED[1].clone()
            });
            s.themes = grown.clone();
            let after = menus::native_items(&s);
            assert_eq!(step(&marked, &after), Step::Rebuild, "{name}");
            let row = after
                .iter()
                .find(|i| i.title == name)
                .expect("the new theme has its row");
            assert_eq!(row.group, Some(menus::THEME));
            assert_eq!(row.kind, NativeKind::Check);
            // The items after it moved on: their ids follow.
            let hl = |l: &[NativeItem]| {
                l.iter()
                    .find(|i| i.command == Command::HighLegibility)
                    .map(|i| i.id.clone())
            };
            assert_ne!(hl(&marked), hl(&after));
        }
        // A note that comes, as high legibility brings, builds it again.
        let before = menus::native_items(&s);
        s.theme.high_legibility = true;
        assert_eq!(step(&before, &menus::native_items(&s)), Step::Rebuild);
        // A changed theme adds its row too.
        let now = menus::native_items(&s);
        s.edit_theme(|t| t.strengths.bloom = 0.9);
        let changed = menus::native_items(&s);
        assert_eq!(step(&now, &changed), Step::Rebuild);
        let row = changed.iter().find(|i| i.unsaved).unwrap();
        assert_eq!(row.title, "Green, changed");
        assert!(row.checked);
        // The built items follow the new list.
        let items = build(&changed);
        assert_eq!(items.len(), changed.len());
    }
}
