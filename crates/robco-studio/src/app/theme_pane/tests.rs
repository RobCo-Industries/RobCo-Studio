// SPDX-License-Identifier: GPL-3.0-or-later

//! The states of a theme in the editor, one test for each row of the
//! table of states, the two questions, the name row and Add a theme file.

use super::*;
use crate::app::{ASKING, Command, Front, Verdict, verdict};
use crate::chooser::{Ask, Choice, FolderChooser};
use crate::menus::{self, ThemeId};
use crate::store::{self, tests::TempDir};
use std::path::{Path, PathBuf};

const EMBER: &str = "version = 1\nname = \"Ember\"\nbase = \"#ff7a1a\"\n";
const DUSK: &str = "version = 1\nname = \"Dusk\"\nbase = \"#66c9ff\"\n";

fn themes(d: &TempDir) -> PathBuf {
    let t = d.0.join(store::THEMES);
    std::fs::create_dir_all(&t).unwrap();
    t
}

/// A window with its settings in `d`, on Green.
fn window(d: &TempDir) -> Studio {
    let mut s = Studio::new(None).with_settings(d.0.clone());
    s.pick_theme(0);
    s
}

/// A window with Ember, a theme of the person's own, in use.
fn on_ember(d: &TempDir) -> Studio {
    std::fs::write(themes(d).join("ember.toml"), EMBER).unwrap();
    let mut s = window(d);
    let i = s.theme_index("Ember").unwrap();
    s.pick_theme(i);
    s
}

/// Press what the pane asks, as a click in the pane does.
fn ask(s: &mut Studio, a: Asked) {
    let ctx = egui::Context::default();
    s.theme_editor.open = true;
    s.apply(&ctx, a);
}

fn nudge(s: &mut Studio) {
    s.edit_theme(|t| t.strengths.noise += 0.05);
}

#[test]
fn a_pick_says_the_theme() {
    let d = TempDir::new("pick");
    let mut s = window(&d);
    ask(
        &mut s,
        Asked {
            pick: Some(1),
            ..Asked::default()
        },
    );
    assert_eq!(s.status.1, "Theme: Amber.");
    assert_eq!(s.theme_name(), "Amber");
}

#[test]
fn a_change_to_a_theme_that_ships_cannot_be_saved() {
    let d = TempDir::new("ships");
    let mut s = window(&d);
    nudge(&mut s);
    assert_eq!(s.status.1, "Theme: Green, changed. Not saved.");
    assert_eq!(s.theme_name(), "Green, changed");
    assert!(s.own_file().is_none());
    ask(
        &mut s,
        Asked {
            save: true,
            ..Asked::default()
        },
    );
    assert_eq!(s.status.1, SHIPPED_SAVE);
    assert!(s.changed.is_some());
    assert!(store::toml_files(&themes(&d)).is_empty(), "nothing written");
}

#[test]
fn a_change_to_your_theme_saves() {
    let d = TempDir::new("save");
    let mut s = on_ember(&d);
    nudge(&mut s);
    assert_eq!(s.status.1, "Theme: Ember, changed. Not saved.");
    ask(
        &mut s,
        Asked {
            save: true,
            ..Asked::default()
        },
    );
    assert_eq!(
        s.status.1,
        "%STUDIO-S-THEMESAVED, theme Ember saved as ember.toml"
    );
    assert!(s.changed.is_none());
    let file = std::fs::read_to_string(themes(&d).join("ember.toml")).unwrap();
    assert!(file.contains("noise = 0.05"), "{file}");
}

#[test]
fn save_as_opens_the_name_row_with_the_next_free_name() {
    let d = TempDir::new("save-as");
    let mut s = window(&d);
    nudge(&mut s);
    ask(
        &mut s,
        Asked {
            save_as: true,
            ..Asked::default()
        },
    );
    assert_eq!(s.theme_editor.naming.as_ref().unwrap().text, "Green 2");
    assert!(!s.theme_editor.naming.as_ref().unwrap().rename);
    assert_eq!(s.name_fault(), None);
    ask(
        &mut s,
        Asked {
            named: true,
            ..Asked::default()
        },
    );
    assert!(s.theme_editor.naming.is_none());
    assert_eq!(
        s.status.1,
        "%STUDIO-S-THEMESAVED, theme Green 2 saved as green-2.toml"
    );
    assert_eq!(s.theme_name(), "Green 2");
    s.pick_theme(0);
    s.start_naming(false);
    assert_eq!(s.theme_editor.naming.as_ref().unwrap().text, "Green 3");
}

#[test]
fn a_name_that_exists_offers_replace() {
    let d = TempDir::new("replace");
    std::fs::write(themes(&d).join("dusk.toml"), DUSK).unwrap();
    let mut s = window(&d);
    nudge(&mut s);
    s.start_naming(false);
    s.theme_editor.naming.as_mut().unwrap().text = "dusk".into();
    let f = s.name_fault().unwrap();
    assert_eq!(f, ThemeFault::Exists("Dusk".into()));
    assert_eq!(
        f.message(),
        "A theme named Dusk exists. Replace writes over it."
    );
    let n = s.themes.len();
    s.finish_naming();
    assert!(s.theme_editor.naming.is_none());
    assert_eq!(s.themes.len(), n, "written over, not added");
    assert_eq!(s.own_file().as_deref(), Some("dusk.toml"));
    let file = std::fs::read_to_string(themes(&d).join("dusk.toml")).unwrap();
    assert!(file.contains("#5aff7f"), "{file}");
}

#[test]
fn a_name_that_ships_is_refused() {
    let d = TempDir::new("green");
    let mut s = window(&d);
    nudge(&mut s);
    s.start_naming(false);
    s.theme_editor.naming.as_mut().unwrap().text = "GREEN".into();
    let f = s.name_fault().unwrap();
    assert_eq!(f.message(), "Green ships with Studio. Choose another name.");
    s.finish_naming();
    assert!(s.theme_editor.naming.is_some(), "the row stays");
    assert!(store::toml_files(&themes(&d)).is_empty());
    s.theme_editor.naming.as_mut().unwrap().text = "a/b".into();
    assert_eq!(s.name_fault(), Some(ThemeFault::BadName));
}

#[test]
fn rename_changes_the_name_and_keeps_the_file() {
    let d = TempDir::new("rename");
    let mut s = on_ember(&d);
    ask(
        &mut s,
        Asked {
            rename: true,
            ..Asked::default()
        },
    );
    let n = s.theme_editor.naming.as_mut().unwrap();
    assert!(n.rename);
    assert_eq!(n.text, "Ember");
    n.text = "Dusk".into();
    ask(
        &mut s,
        Asked {
            named: true,
            ..Asked::default()
        },
    );
    assert_eq!(s.status.1, "Theme: Dusk.");
    assert_eq!(s.own_file().as_deref(), Some("ember.toml"));
    // A theme that ships has no Rename.
    s.pick_theme(0);
    s.start_naming(true);
    assert!(s.theme_editor.naming.is_none());
}

#[test]
fn revert_drops_the_change_and_asks_nothing() {
    let d = TempDir::new("revert");
    let mut s = window(&d);
    nudge(&mut s);
    ask(
        &mut s,
        Asked {
            revert: true,
            ..Asked::default()
        },
    );
    assert!(s.changed.is_none());
    assert!(s.theme_editor.question.is_none());
    assert_eq!(s.status.1, "Theme: Green.");
}

#[test]
fn delete_asks_then_deletes() {
    let d = TempDir::new("delete");
    let mut s = on_ember(&d);
    let del = Asked {
        delete: true,
        ..Asked::default()
    };
    ask(&mut s, del);
    assert_eq!(s.theme_editor.question, Some(ThemeQuestion::Delete));
    assert_eq!(s.front(), Front::Question);
    assert_eq!(delete_title("Ember"), "Delete the theme Ember?");
    s.answer_theme(Answer::Cancel);
    assert!(s.theme_editor.question.is_none());
    assert!(themes(&d).join("ember.toml").exists());
    ask(
        &mut s,
        Asked {
            delete: true,
            ..Asked::default()
        },
    );
    s.answer_theme(Answer::Save);
    assert_eq!(s.status.1, "Theme Ember deleted. Theme: Green.");
    assert!(!themes(&d).join("ember.toml").exists());
    assert_eq!(s.current, 0);
    // A theme that ships asks nothing.
    ask(
        &mut s,
        Asked {
            delete: true,
            ..Asked::default()
        },
    );
    assert!(s.theme_editor.question.is_none());
}

/// Delete never reaches outside the themes folder: a file name with a
/// path in it is refused, and the file outside stays.
#[test]
fn delete_stays_in_the_themes_folder() {
    let d = TempDir::new("outside");
    let mut s = on_ember(&d);
    let outside = d.0.join("outside.toml");
    std::fs::write(&outside, EMBER).unwrap();
    let i = s.current;
    s.theme_files[i] = Some(["..", "outside.toml"].join(std::path::MAIN_SEPARATOR_STR));
    assert!(matches!(s.delete_theme(), Err(ThemeFault::Write(_))));
    assert!(outside.exists());
    s.theme_files[i] = Some(store::THEMES.to_owned());
    assert!(matches!(s.delete_theme(), Err(ThemeFault::Write(_))));
    assert!(themes(&d).exists());
}

/// A link in the themes folder is no theme, and Delete does not follow
/// one.
#[cfg(unix)]
#[test]
fn delete_follows_no_link() {
    let d = TempDir::new("link");
    let mut s = on_ember(&d);
    let outside = d.0.join("outside.toml");
    std::fs::write(&outside, EMBER).unwrap();
    std::os::unix::fs::symlink(&outside, themes(&d).join("link.toml")).unwrap();
    s.reload_themes();
    assert!(
        !s.theme_files.iter().flatten().any(|f| f == "link.toml"),
        "a link is not listed"
    );
    let i = s.current;
    s.theme_files[i] = Some("link.toml".into());
    assert!(matches!(s.delete_theme(), Err(ThemeFault::Write(_))));
    assert!(outside.exists());
    assert!(themes(&d).join("link.toml").exists());
}

/// Leaving a changed theme asks, from the dropdown and from the menu:
/// Cancel keeps it, Do not save drops it, Save as keeps it under the
/// proposed name; then the theme picked takes its place.
#[test]
fn leaving_a_changed_theme_asks() {
    let ctx = egui::Context::default();
    let from_dropdown = |s: &mut Studio| {
        ask(
            s,
            Asked {
                pick: Some(1),
                ..Asked::default()
            },
        );
    };
    let from_menu = |s: &mut Studio| s.run(&ctx, Command::Theme(ThemeId::At(1)));
    for (k, leave) in [
        &from_dropdown as &dyn Fn(&mut Studio),
        &from_menu as &dyn Fn(&mut Studio),
    ]
    .into_iter()
    .enumerate()
    {
        let d = TempDir::new(&format!("leave-{k}"));
        let mut s = window(&d);
        nudge(&mut s);
        leave(&mut s);
        assert_eq!(
            s.theme_editor.question,
            Some(ThemeQuestion::Leave {
                to: "Amber".into(),
                name: "Green 2".into()
            })
        );
        assert_eq!(s.front(), Front::Question);
        assert_eq!(
            leave_text("Green", "Amber"),
            "Green was changed and not saved. Amber takes its place now."
        );
        // Cancel: the changed theme stays on screen.
        s.answer_theme(Answer::Cancel);
        assert_eq!(s.theme_name(), "Green, changed");
        // Do not save: Amber, and the change is gone.
        leave(&mut s);
        s.answer_theme(Answer::DoNotSave);
        assert_eq!(s.theme_name(), "Amber");
        assert!(store::toml_files(&themes(&d)).is_empty());
        // Save as: kept as Green 2, then Amber.
        s.pick_theme(0);
        nudge(&mut s);
        leave(&mut s);
        s.answer_theme(Answer::Save);
        assert!(s.theme_editor.question.is_none());
        assert_eq!(s.theme_name(), "Amber");
        assert_eq!(store::toml_files(&themes(&d)), ["green-2.toml"]);
    }
}

/// Leaving your own changed theme: Save writes its file, then the theme
/// picked takes its place. A name that ships in the question's Name field
/// keeps the question open.
#[test]
fn leaving_your_changed_theme_saves_it() {
    let d = TempDir::new("leave-own");
    let mut s = on_ember(&d);
    nudge(&mut s);
    s.ask_pick(1);
    assert!(matches!(
        &s.theme_editor.question,
        Some(ThemeQuestion::Leave { name, .. }) if name.is_empty()
    ));
    s.answer_theme(Answer::Save);
    assert_eq!(s.theme_name(), "Amber");
    let file = std::fs::read_to_string(themes(&d).join("ember.toml")).unwrap();
    assert!(file.contains("noise = 0.05"), "{file}");

    s.pick_theme(0);
    nudge(&mut s);
    s.ask_pick(1);
    if let Some(ThemeQuestion::Leave { name, .. }) = &mut s.theme_editor.question {
        "green".clone_into(name);
    }
    assert_eq!(s.name_fault(), Some(ThemeFault::Shipped("Green".into())));
    s.answer_theme(Answer::Save);
    assert!(s.theme_editor.question.is_some(), "the question stays");
    assert_eq!(s.theme_name(), "Green, changed");
}

/// Closing the editor keeps a changed theme on screen and in the menu.
#[test]
fn closing_keeps_the_change() {
    let d = TempDir::new("close");
    let mut s = window(&d);
    nudge(&mut s);
    ask(
        &mut s,
        Asked {
            close: true,
            ..Asked::default()
        },
    );
    assert!(!s.theme_editor.open);
    assert_eq!(s.theme_name(), "Green, changed");
    assert!(
        menus::native_items(&s)
            .iter()
            .any(|i| i.title == "Green, changed")
    );
}

/// Quit asks nothing about a changed theme; the settings keep it, and it
/// is back at the next start.
#[test]
fn quit_keeps_the_change_in_the_settings() {
    let d = TempDir::new("quit");
    let ctx = egui::Context::default();
    let mut s = window(&d);
    nudge(&mut s);
    s.run(&ctx, Command::Quit);
    assert_eq!(s.front(), Front::Nothing);
    // What the window does as it closes.
    s.save_settings();
    let text = std::fs::read_to_string(d.0.join(store::SETTINGS)).unwrap();
    assert!(text.contains("[unsaved]"), "{text}");
    let back = Studio::new(None).with_settings(d.0.clone());
    assert_eq!(back.theme_name(), "Green, changed");
}

/// While a question about a theme is open, every command is refused and
/// changes nothing; while the name row is open, the editor's own table
/// holds.
#[test]
fn the_front_table_holds_for_the_theme_questions() {
    let d = TempDir::new("table");
    let ctx = egui::Context::default();
    let items: Vec<menus::Item> = menus::menus().into_iter().flat_map(|m| m.items).collect();
    for it in &items {
        let mut s = on_ember(&d);
        nudge(&mut s);
        s.theme_editor.open = true;
        s.ask_pick(0);
        assert_eq!(s.front(), Front::Question);
        assert_eq!(
            verdict(it.command, Front::Question),
            Verdict::Refused(ASKING)
        );
        let before = (s.theme_name(), s.theme_editor.question.clone(), s.page);
        s.run(&ctx, it.command);
        assert_eq!(
            (s.theme_name(), s.theme_editor.question.clone(), s.page),
            before,
            "{:?}",
            it.command
        );
        assert_eq!(s.unavailable(it).as_deref(), Some(ASKING));
    }
    let mut s = on_ember(&d);
    s.theme_editor.open = true;
    s.start_naming(false);
    assert_eq!(s.front(), Front::Theme);
    for it in &items {
        assert_eq!(
            verdict(it.command, s.front()),
            verdict(it.command, Front::Theme)
        );
    }
}

/// The name row takes Escape before the pane: the first Escape goes back
/// to the buttons, the second closes the editor. Enter in the field
/// saves.
#[test]
fn the_name_row_takes_escape_first_and_enter_saves() {
    let d = TempDir::new("escape");
    let mut s = window(&d);
    nudge(&mut s);
    s.theme_editor.open = true;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
    h.run_steps(3);
    h.state_mut().start_naming(false);
    h.run_steps(3);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().theme_editor.naming.is_none());
    assert!(h.state().theme_editor.open, "the editor stays");
    h.state_mut().start_naming(false);
    h.run_steps(3);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert!(h.state().theme_editor.naming.is_none());
    assert_eq!(h.state().theme_name(), "Green 2");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().theme_editor.open);
}

/// Escape cancels a question and leaves the editor open behind it.
#[test]
fn escape_cancels_the_question_first() {
    let d = TempDir::new("escape-q");
    let mut s = on_ember(&d);
    s.theme_editor.open = true;
    s.theme_editor.question = Some(ThemeQuestion::Delete);
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
    h.run_steps(3);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().theme_editor.question.is_none());
    assert!(h.state().theme_editor.open);
    assert!(themes(&d).join("ember.toml").exists());
}

/// A chooser that answers one file, asked for a theme file.
struct OneFile(Option<PathBuf>);

impl FolderChooser for OneFile {
    fn start(&mut self, _ctx: &egui::Context, _start: Option<&Path>, ask: Ask) {
        assert_eq!(ask, Ask::ThemeFile);
    }
    fn poll(&mut self) -> Option<Choice> {
        self.0.take().map(Choice::Chosen)
    }
}

/// Add a theme file asks the chooser for a file and writes a clean file
/// from what it understood: no unknown key, no hostile character.
#[test]
fn an_added_file_comes_out_clean() {
    let d = TempDir::new("add-file");
    let src = d.0.join("from.toml");
    std::fs::write(
        &src,
        "version = 1\nname = \"Ev\\u202Eil\\u0000 \\n One\"\nbase = \"#ff7a1a\"\nsparkle = \"rm -rf\"\n[screen]\nbloom = 7\n",
    )
    .unwrap();
    let ctx = egui::Context::default();
    let mut s = Studio::new(None)
        .with_settings(d.0.clone())
        .with_chooser(Box::new(OneFile(Some(src))));
    s.run(&ctx, Command::ThemeAdd);
    assert_eq!(s.front(), Front::Chooser);
    s.choose_folder(&ctx);
    assert_eq!(s.front(), Front::Nothing);
    assert_eq!(s.theme_name(), "Evil One");
    let file = std::fs::read_to_string(themes(&d).join("evil-one.toml")).unwrap();
    assert!(!file.contains("sparkle") && !file.contains("rm"), "{file}");
    assert!(!file.contains('\u{202e}') && !file.contains('\0'), "{file}");
    assert!(file.contains("bloom = 1.00"), "{file}");
}
