// SPDX-License-Identifier: GPL-3.0-or-later

//! The states of a theme in the editor, one test for each row of the
//! table of states, the two questions, the name row and Add a theme file.

use super::*;
use crate::app::{ASKING, Command, Front, Verdict, verdict};
use crate::chooser::{Ask, Choice, FolderChooser};
use crate::menus::{self, ThemeId};
use crate::store::{self, tests::TempDir};
use egui_kittest::kittest::{NodeT, Queryable};
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
    assert!(
        store::toml_files(&themes(&d)).files().is_empty(),
        "nothing written"
    );
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
    assert!(store::toml_files(&themes(&d)).files().is_empty());
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
                to: Leaving::Theme("Amber".into()),
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
        assert!(store::toml_files(&themes(&d)).files().is_empty());
        // Save as: kept as Green 2, then Amber.
        s.pick_theme(0);
        nudge(&mut s);
        leave(&mut s);
        s.answer_theme(Answer::Save);
        assert!(s.theme_editor.question.is_none());
        assert_eq!(s.theme_name(), "Amber");
        assert_eq!(store::toml_files(&themes(&d)).files(), ["green-2.toml"]);
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
    s.pick_theme(1);
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
    s.pick_theme(1);
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
        s.pick_theme(0);
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

/// What happens to a change not saved when the theme in use may stop
/// being current.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    /// The leave question is open.
    Ask,
    /// No question, and the change is kept: still changed, or saved.
    Keep,
    /// No question, and the change is gone: the person asked for that.
    Drop,
    /// No question, and there was no change to lose.
    Go,
}

/// Every way the theme in use can stop being current, or its change can
/// be dropped, with a changed theme and without one.
#[test]
fn every_way_to_leave_a_theme_passes_one_check() {
    type Way = fn(&mut Studio, &TempDir);
    let ways: [(&str, Way); 15] = [
        ("menu pick", |s, _| {
            s.run(&egui::Context::default(), Command::Theme(ThemeId::At(1)));
        }),
        ("dropdown pick", |s, _| {
            ask(
                s,
                Asked {
                    pick: Some(1),
                    ..Asked::default()
                },
            );
        }),
        ("add a theme file", |s, d| {
            let src = d.0.join("from.toml");
            std::fs::write(&src, DUSK).unwrap();
            let _ = s.add_theme_file(&src);
        }),
        ("add through the chooser", |s, d| {
            let src = d.0.join("from.toml");
            std::fs::write(&src, DUSK).unwrap();
            let ctx = egui::Context::default();
            s.chooser = Box::new(OneFile(Some(src)));
            s.run(&ctx, Command::ThemeAdd);
            s.choose_folder(&ctx);
        }),
        ("delete", |s, _| {
            ask(
                s,
                Asked {
                    delete: true,
                    ..Asked::default()
                },
            );
            s.answer_theme(Answer::Save);
        }),
        ("rename", |s, _| {
            s.start_naming(true);
            s.theme_editor.naming.as_mut().unwrap().text = "Dusk".into();
            s.finish_naming();
        }),
        ("save as", |s, _| {
            s.start_naming(false);
            s.finish_naming();
        }),
        ("replace", |s, d| {
            std::fs::write(themes(d).join("dusk.toml"), DUSK).unwrap();
            s.reload_themes();
            s.start_naming(false);
            s.theme_editor.naming.as_mut().unwrap().text = "Dusk".into();
            s.finish_naming();
        }),
        ("save", |s, _| {
            let _ = s.save_theme();
        }),
        ("revert", |s, _| {
            ask(
                s,
                Asked {
                    revert: true,
                    ..Asked::default()
                },
            );
        }),
        ("reload the folder", |s, _| s.reload_themes()),
        ("close the editor", |s, _| {
            ask(
                s,
                Asked {
                    close: true,
                    ..Asked::default()
                },
            );
        }),
        ("open another folder", |s, d| {
            let p = d.0.join("project");
            std::fs::create_dir_all(&p).unwrap();
            s.use_project(crate::project::Project::open(&p).unwrap());
        }),
        ("quit", |s, d| {
            s.run(&egui::Context::default(), Command::Quit);
            s.save_settings();
            *s = Studio::new(None).with_settings(d.0.clone());
        }),
        ("a tab", |s, _| {
            ask(
                s,
                Asked {
                    tab: Some(ThemeTab::Screen),
                    ..Asked::default()
                },
            );
        }),
    ];
    let leaves = [
        "menu pick",
        "dropdown pick",
        "add a theme file",
        "add through the chooser",
    ];
    let drops = ["delete", "revert"];
    let mut rows = 0;
    for (way, run) in ways {
        for changed in [true, false] {
            let want = match (leaves.contains(&way), drops.contains(&way), changed) {
                (_, _, false) => Then::Go,
                (true, _, true) => Then::Ask,
                (_, true, true) => Then::Drop,
                _ => Then::Keep,
            };
            let row = format!("{way}, changed {changed}");
            let d = TempDir::new(&format!("way-{rows}"));
            let mut s = on_ember(&d);
            let noise = s.spec().strengths.noise;
            if changed {
                nudge(&mut s);
            }
            run(&mut s, &d);
            rows += 1;
            let asked = matches!(s.theme_editor.question, Some(ThemeQuestion::Leave { .. }));
            let kept = (s.spec().strengths.noise - noise - 0.05).abs() < 1e-4;
            let got = if asked {
                Then::Ask
            } else if !changed {
                Then::Go
            } else if kept {
                Then::Keep
            } else {
                Then::Drop
            };
            assert_eq!(got, want, "{row}");
            if !changed {
                assert!(s.theme_editor.question.is_none(), "{row}");
                assert!(s.changed.is_none(), "{row}");
            }
            if got == Then::Ask {
                // Cancel keeps the change on screen and adds nothing.
                let files = store::toml_files(&themes(&d)).files().to_vec();
                assert_eq!(s.answer_theme(Answer::Cancel), None, "{row}");
                assert_eq!(s.theme_name(), "Ember, changed", "{row}");
                assert_eq!(store::toml_files(&themes(&d)).files(), files, "{row}");
            }
        }
    }
    assert_eq!(rows, 30);
}

/// Add a theme file over a change not saved asks the question of a pick.
/// Cancel adds nothing; Do not save adds the file and drops the change;
/// Save keeps the change in its file first, then adds the file.
#[test]
fn adding_a_file_over_a_change_asks_like_a_pick() {
    for (k, a) in [Answer::Cancel, Answer::DoNotSave, Answer::Save]
        .into_iter()
        .enumerate()
    {
        let d = TempDir::new(&format!("add-ask-{k}"));
        let src = d.0.join("from.toml");
        std::fs::write(&src, DUSK).unwrap();
        let mut s = on_ember(&d);
        nudge(&mut s);
        assert_eq!(s.add_theme_file(&src), Ok(None));
        assert_eq!(
            s.theme_editor.question,
            Some(ThemeQuestion::Leave {
                to: Leaving::File {
                    path: src.clone(),
                    name: "Dusk".into()
                },
                name: String::new(),
            })
        );
        assert_eq!(s.front(), Front::Question);
        s.answer_theme(a);
        assert!(s.theme_editor.question.is_none(), "{a:?}");
        let added = themes(&d).join("dusk.toml").exists();
        let ember = std::fs::read_to_string(themes(&d).join("ember.toml")).unwrap();
        match a {
            Answer::Cancel => {
                assert!(!added);
                assert_eq!(s.theme_name(), "Ember, changed");
            }
            Answer::DoNotSave => {
                assert!(added);
                assert_eq!(s.theme_name(), "Dusk");
                assert!(!ember.contains("noise = 0.05"), "{ember}");
            }
            _ => {
                assert!(added);
                assert_eq!(s.theme_name(), "Dusk");
                assert!(ember.contains("noise = 0.05"), "{ember}");
            }
        }
        // The settings keep no change that was dropped or saved.
        s.save_settings();
        let text = std::fs::read_to_string(d.0.join(store::SETTINGS)).unwrap();
        assert_eq!(text.contains("[unsaved]"), a == Answer::Cancel, "{text}");
    }
}

/// A field's text, typed and not yet committed when an action comes in
/// the same frame.
#[derive(Debug, Clone, Copy)]
enum Pending {
    /// The base colour field, with three digits.
    Hex3,
    /// The base colour field, with six digits.
    Hex6,
    /// The base colour field, with text that is not a colour.
    HexBad,
    /// The number box of the Noise slider.
    Number,
}

impl Pending {
    /// What the pane reports for the field as it loses focus: a colour of
    /// 3 or 6 digits, or the slider's typed number.
    fn asked(self) -> Asked {
        let hex = |t: &str| palette::parse_hex(t).map(|(c, _)| c);
        match self {
            Pending::Hex3 => Asked {
                base: hex("f80"),
                ..Asked::default()
            },
            Pending::Hex6 => Asked {
                base: hex("#FF8800"),
                ..Asked::default()
            },
            Pending::HexBad => Asked {
                base: hex("f8"),
                ..Asked::default()
            },
            Pending::Number => {
                let mut s = crate::theme::SHIPPED[0].strengths;
                s.noise = 0.3;
                Asked {
                    strengths: Some(s),
                    ..Asked::default()
                }
            }
        }
    }

    /// The line of a theme file that holds the value.
    fn line(self) -> &'static str {
        match self {
            Pending::Number => "noise = 0.30",
            _ => "#ff8800",
        }
    }

    fn valid(self) -> bool {
        !matches!(self, Pending::HexBad)
    }
}

/// Each field with pending text against each action of the same frame.
/// The edits apply first: a valid value is in what the action saves or
/// keeps, an invalid one is not and the action still happens. Revert
/// drops both.
#[test]
fn a_pending_field_applies_before_the_action_of_its_frame() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Do {
        Save,
        SaveAs,
        Replace,
        Rename,
        Delete,
        Revert,
        Pick,
        Close,
        Tab,
    }
    let acts = [
        Do::Save,
        Do::SaveAs,
        Do::Replace,
        Do::Rename,
        Do::Delete,
        Do::Revert,
        Do::Pick,
        Do::Close,
        Do::Tab,
    ];
    let mut rows = 0;
    for field in [
        Pending::Hex3,
        Pending::Hex6,
        Pending::HexBad,
        Pending::Number,
    ] {
        for act in acts {
            let row = format!("{field:?} {act:?}");
            let d = TempDir::new(&format!("order-{rows}"));
            rows += 1;
            std::fs::write(themes(&d).join("dusk.toml"), DUSK).unwrap();
            let mut s = on_ember(&d);
            let saved = s.spec().clone();
            let mut a = field.asked();
            match act {
                Do::Save => a.save = true,
                Do::SaveAs => {
                    s.start_naming(false);
                    a.named = true;
                }
                Do::Replace => {
                    s.start_naming(false);
                    s.theme_editor.naming.as_mut().unwrap().text = "Dusk".into();
                    a.named = true;
                }
                Do::Rename => {
                    s.start_naming(true);
                    s.theme_editor.naming.as_mut().unwrap().text = "Coal".into();
                    a.named = true;
                }
                Do::Delete => a.delete = true,
                Do::Revert => a.revert = true,
                Do::Pick => a.pick = Some(1),
                Do::Close => a.close = true,
                Do::Tab => a.tab = Some(ThemeTab::Screen),
            }
            ask(&mut s, a);
            let file = |f: &str| std::fs::read_to_string(themes(&d).join(f)).unwrap_or_default();
            let has = |t: &str| t.contains(field.line());
            let spec_has = |s: &Studio| match field {
                Pending::Number => (s.spec().strengths.noise - 0.3).abs() < 1e-4,
                _ => s.spec().base == palette::parse_hex("ff8800").unwrap().0,
            };
            match act {
                Do::Save => assert_eq!(has(&file("ember.toml")), field.valid(), "{row}"),
                Do::SaveAs => {
                    assert_eq!(s.theme_name(), "Ember 2", "{row}");
                    assert_eq!(has(&file("ember-2.toml")), field.valid(), "{row}");
                }
                Do::Replace => {
                    assert_eq!(s.own_file().as_deref(), Some("dusk.toml"), "{row}");
                    assert_eq!(has(&file("dusk.toml")), field.valid(), "{row}");
                }
                Do::Rename => {
                    assert_eq!(s.own_name(s.current), "Coal", "{row}");
                    assert_eq!(spec_has(&s), field.valid(), "{row}");
                    assert_eq!(s.changed.is_some(), field.valid(), "{row}");
                }
                Do::Delete => {
                    assert_eq!(
                        s.theme_editor.question,
                        Some(ThemeQuestion::Delete),
                        "{row}"
                    );
                    s.answer_theme(Answer::Save);
                    assert!(file("ember.toml").is_empty(), "{row}");
                    assert_eq!(s.theme_name(), "Green", "{row}");
                }
                Do::Revert => {
                    assert!(s.changed.is_none(), "{row}");
                    assert_eq!(*s.spec(), saved, "{row}");
                }
                Do::Pick => {
                    if field.valid() {
                        assert!(s.theme_editor.question.is_some(), "{row}");
                        s.answer_theme(Answer::Save);
                        assert!(has(&file("ember.toml")), "{row}");
                    }
                    assert_eq!(s.theme_name(), "Amber", "{row}");
                }
                Do::Close => {
                    assert!(!s.theme_editor.open, "{row}");
                    assert_eq!(spec_has(&s), field.valid(), "{row}");
                }
                Do::Tab => {
                    assert_eq!(s.theme_editor.tab, ThemeTab::Screen, "{row}");
                    assert_eq!(spec_has(&s), field.valid(), "{row}");
                }
            }
        }
    }
    assert_eq!(rows, 36);
}

/// The name row's text is the name its action uses, and Cancel or close
/// in the same frame drops it with nothing written.
#[test]
fn the_name_row_text_goes_with_its_action() {
    for (k, named) in [true, false].into_iter().enumerate() {
        let d = TempDir::new(&format!("name-row-{k}"));
        let mut s = on_ember(&d);
        nudge(&mut s);
        s.start_naming(false);
        s.theme_editor.naming.as_mut().unwrap().text = "Coal".into();
        ask(
            &mut s,
            Asked {
                named,
                close: !named,
                ..Asked::default()
            },
        );
        assert!(s.theme_editor.naming.is_none());
        assert_eq!(themes(&d).join("coal.toml").exists(), named);
    }
}

/// The finding itself, in the window: three digits typed, then a click on
/// Save without Enter. The field commits as it loses focus in the frame
/// of the click, and Save writes the typed colour.
#[test]
fn three_digits_then_save_writes_the_typed_colour() {
    let d = TempDir::new("hex-save");
    let s = on_ember(&d);
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
    h.state_mut().theme_editor.open = true;
    h.run_steps(3);
    h.get_by_role_and_label(Role::TextInput, BASE).focus();
    h.run_steps(2);
    h.state_mut().theme_editor.hex = "f80".into();
    h.run_steps(1);
    h.get_by_role_and_label(Role::Button, BUTTONS[4]).click();
    h.run_steps(3);
    let file = std::fs::read_to_string(themes(&d).join("ember.toml")).unwrap();
    assert!(file.contains("#ff8800"), "{file}");
    assert!(h.state().changed.is_none());
    assert_eq!(
        h.state().theme_editor.hex(),
        "f80",
        "the field keeps what was typed"
    );
}

/// How a test reaches the last theme of a long list.
#[derive(Debug, Clone, Copy)]
enum Reach {
    /// Enter opens the list, End then Enter picks.
    Keys,
    /// A click opens the list, the wheel scrolls it, a click picks.
    Pointer,
}

/// A themes folder with `n` themes, "Theme 00" on, and `bad` files that
/// are not themes, which sort first.
fn many(d: &TempDir, n: usize, bad: usize) {
    let t = themes(d);
    for i in 0..bad {
        std::fs::write(t.join(format!("a{i:02}.toml")), "version = 1\n").unwrap();
    }
    for i in 0..n {
        std::fs::write(
            t.join(format!("t{i:02}.toml")),
            format!("version = 1\nname = \"Theme {i:02}\"\nbase = \"#5aff7f\"\n"),
        )
        .unwrap();
    }
}

fn press_at(h: &egui_kittest::Harness<'_, Studio>, at: egui::Pos2) {
    h.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
}

/// The window at the smallest size the app allows.
fn smallest() -> egui::Vec2 {
    egui::vec2(crate::theme::size::WINDOW[0], crate::theme::size::WINDOW[1])
}

/// The rect of the option `label` of the open list.
fn option(h: &egui_kittest::Harness<'_, Studio>, label: &str) -> egui::Rect {
    use egui::accesskit::Role;
    h.get_by_role_and_label(Role::ListBoxOption, label).rect()
}

/// The editor's Theme list is as tall as the window allows and scrolls,
/// and the last theme of a full folder can be picked with the keys and
/// with the pointer, in a window of the smallest size.
#[test]
fn the_last_of_a_long_list_of_themes_can_be_picked() {
    use egui::accesskit::Role;
    let rows = [
        (64, 0, Reach::Keys),
        (64, 0, Reach::Pointer),
        (56, 8, Reach::Keys),
        (56, 8, Reach::Pointer),
    ];
    let room = egui::Rect::from_min_size(egui::Pos2::ZERO, smallest());
    for (n, bad, reach) in rows {
        let row = format!("{n} themes, {bad} refused, {reach:?}");
        let d = TempDir::new("long-list");
        many(&d, n, bad);
        let mut s = window(&d);
        assert_eq!(s.themes.len(), 6 + n, "{row}");
        assert_eq!(s.refused.len(), bad, "{row}");
        s.theme_editor.open = true;
        let mut h = egui_kittest::Harness::builder()
            .with_size(smallest())
            .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
        h.run_steps(3);
        let field = h
            .get_by_role_and_label(Role::ComboBox, "Theme: Green")
            .rect();
        match reach {
            Reach::Keys => h.key_press(Key::Enter),
            Reach::Pointer => press_at(&h, field.center()),
        }
        h.run_steps(3);
        let list = h.get_by_role(Role::ListBox).rect();
        assert!(
            room.contains_rect(list),
            "{row}: the list {list:?} leaves the window"
        );
        let last = format!("Theme {:02}", n - 1);
        match reach {
            Reach::Keys => {
                h.key_press(Key::End);
                h.run_steps(3);
                assert!(
                    room.contains_rect(option(&h, &last)),
                    "{row}: End shows the row"
                );
                h.key_press(Key::Enter);
            }
            Reach::Pointer => {
                for _ in 0..40 {
                    h.event(egui::Event::PointerMoved(list.center()));
                    h.event(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -200.0),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Wheel,
                        modifiers: egui::Modifiers::NONE,
                    });
                    h.run_steps(1);
                }
                h.run_steps(10);
                let r = option(&h, &last);
                assert!(
                    list.contains(r.center()),
                    "{row}: the wheel shows the row {r:?} in {list:?}"
                );
                press_at(&h, r.center());
            }
        }
        h.run_steps(3);
        assert_eq!(h.state().theme_name(), last, "{row}");
        // Opened again, the list shows the theme in use.
        h.key_press(Key::Enter);
        h.run_steps(4);
        let list = h.get_by_role(Role::ListBox).rect();
        assert!(
            list.contains(option(&h, &last).center()),
            "{row}: opens on the row in use"
        );
        // Escape closes the list and leaves the editor open.
        h.key_press(Key::Escape);
        h.run_steps(3);
        assert!(h.query_by_role(Role::ListBox).is_none(), "{row}");
        assert!(h.state().theme_editor.open, "{row}");
    }
}

/// The keys of an open list: Down, Up, Page Down, Page Up, Home and End
/// move the selected option, as a screen reader hears it.
#[test]
fn the_keys_move_the_selected_option() {
    use egui::accesskit::Role;
    let d = TempDir::new("list-keys");
    many(&d, 64, 0);
    let mut s = window(&d);
    s.theme_editor.open = true;
    let mut h = egui_kittest::Harness::builder()
        .with_size(smallest())
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
    h.run_steps(3);
    h.key_press(Key::Enter);
    h.run_steps(3);
    let selected = |h: &egui_kittest::Harness<'_, Studio>| {
        h.query_all_by_role(Role::ListBoxOption)
            .filter(|n| n.accesskit_node().is_selected() == Some(true))
            .map(|n| n.accesskit_node().label().unwrap_or_default())
            .collect::<Vec<_>>()
    };
    assert_eq!(selected(&h), ["Green"]);
    for (key, want) in [
        (Key::ArrowDown, "Amber"),
        (Key::ArrowUp, "Green"),
        (Key::ArrowUp, "Green"),
        (Key::End, "Theme 63"),
        (Key::ArrowDown, "Theme 63"),
        (Key::Home, "Green"),
    ] {
        h.key_press(key);
        h.run_steps(3);
        assert_eq!(selected(&h), [want], "{key:?}");
    }
    h.key_press(Key::PageDown);
    h.run_steps(3);
    let down = selected(&h);
    assert_ne!(down, ["Green"]);
    h.key_press(Key::PageUp);
    h.run_steps(3);
    assert_eq!(selected(&h), ["Green"]);
    // A key does not leak to the window: the theme is as it was.
    assert_eq!(h.state().theme_name(), "Green");
}

/// A name of 32 wide letters stays inside the question that names it and
/// inside the status line, in a window of the smallest size.
#[test]
fn a_long_name_stays_inside_the_question_and_the_status_line() {
    use egui::accesskit::Role;
    let name = "W".repeat(store::NAME_MAX);
    let d = TempDir::new("long-name");
    std::fs::write(
        themes(&d).join("w.toml"),
        format!("version = 1\nname = \"{name}\"\nbase = \"#5aff7f\"\n"),
    )
    .unwrap();
    let mut s = window(&d);
    let i = s.theme_index(&name).unwrap();
    s.pick_theme(i);
    nudge(&mut s);
    s.theme_editor.open = true;
    s.pick_theme(1);
    assert!(s.theme_editor.question.is_some());
    let mut h = egui_kittest::Harness::builder()
        .with_size(smallest())
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
    h.run_steps(4);
    let room = egui::Rect::from_min_size(egui::Pos2::ZERO, smallest());
    // The question is a fixed width, centred. Each line that names the
    // theme stays inside it and above its buttons.
    let half = crate::theme::size::DIALOG_W / 2.0;
    let across = room.center().x - half..=room.center().x + half;
    let cancel = h.get_by_role_and_label(Role::Button, "Cancel").rect();
    let lines: Vec<egui::Rect> = h
        .query_all_by(|n| {
            matches!(n.role(), Role::Label | Role::TextRun)
                && n.value().is_some_and(|v| v.contains(&name))
                && !n.value().is_some_and(|v| v.starts_with("Theme:"))
        })
        .map(|n| n.rect())
        .collect();
    assert!(lines.len() >= 2, "the title and the text: {lines:?}");
    for r in lines {
        assert!(
            across.contains(&r.left()) && across.contains(&r.right()),
            "{r:?}"
        );
        assert!(
            r.bottom() <= cancel.top(),
            "{r:?} runs into the buttons at {cancel:?}"
        );
    }
    let said = h
        .query_all_by(|n| {
            n.role() == Role::Label && n.value().is_some_and(|v| v.starts_with("Theme:"))
        })
        .next()
        .unwrap()
        .rect();
    // The status line cuts a long line with an ellipsis and keeps it in
    // the window.
    assert!(room.contains_rect(said), "{said:?} leaves the window");
}

/// One state of the theme picker, and what its open list shows.
struct Picker {
    row: &'static str,
    /// Make the state, from a window on Green.
    make: fn(&TempDir) -> Studio,
    /// The row lit when the list opens, or none.
    lit: Option<&'static str>,
    /// The field's value.
    label: &'static str,
    /// What Enter right after opening asks to leave for, or None when it
    /// changes nothing.
    enter: Option<&'static str>,
}

/// The theme picker lights the row the theme in use sits on, a changed
/// theme too, and no row when the change sits on none. Enter picks the
/// lit row, and nothing when no row is lit.
#[test]
fn the_picker_lights_the_row_of_the_theme_in_use() {
    use egui::accesskit::Role;
    let rows = [
        Picker {
            row: "an unchanged theme that ships",
            make: |d| {
                let mut s = window(d);
                s.pick_theme(1);
                s
            },
            lit: Some("Amber"),
            label: "Amber",
            enter: None,
        },
        Picker {
            row: "an unchanged theme of the person's own",
            make: on_ember,
            lit: Some("Ember"),
            label: "Ember",
            enter: None,
        },
        Picker {
            row: "a changed theme that ships",
            make: |d| {
                let mut s = window(d);
                s.pick_theme(1);
                nudge(&mut s);
                s
            },
            lit: Some("Amber"),
            label: "Amber, changed",
            enter: Some("Amber"),
        },
        Picker {
            row: "a changed theme of the person's own",
            make: |d| {
                let mut s = on_ember(d);
                nudge(&mut s);
                s
            },
            lit: Some("Ember"),
            label: "Ember, changed",
            enter: Some("ember.toml"),
        },
        Picker {
            row: "a changed theme with no row",
            make: |d| {
                let mut s = on_ember(d);
                nudge(&mut s);
                std::fs::remove_file(themes(d).join("ember.toml")).unwrap();
                s.reload_themes();
                s
            },
            lit: None,
            label: "Ember, changed",
            enter: None,
        },
    ];
    for p in rows {
        let row = p.row;
        let d = TempDir::new("picker");
        let mut s = (p.make)(&d);
        s.theme_editor.open = true;
        let mut h = egui_kittest::Harness::builder()
            .with_size(smallest())
            .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
        h.run_steps(3);
        let field = format!("{}: {}", crate::app::THEME_TITLE, p.label);
        assert!(
            h.query_by_role_and_label(Role::ComboBox, &field).is_some(),
            "{row}: the field reads {field}"
        );
        h.key_press(Key::Enter);
        h.run_steps(3);
        let selected: Vec<String> = h
            .query_all_by_role(Role::ListBoxOption)
            .filter(|n| n.accesskit_node().is_selected() == Some(true))
            .map(|n| n.accesskit_node().label().unwrap_or_default())
            .collect();
        let want: Vec<String> = p.lit.iter().map(|l| (*l).to_owned()).collect();
        assert_eq!(selected, want, "{row}: the lit row");
        h.key_press(Key::Enter);
        h.run_steps(3);
        let st = h.state();
        match p.enter {
            Some(key) => assert!(
                matches!(
                    &st.theme_editor.question,
                    Some(ThemeQuestion::Leave { to: Leaving::Theme(k), .. }) if k == key
                ),
                "{row}: Enter leaves for {key}: {:?}",
                st.theme_editor.question
            ),
            None => {
                assert!(st.theme_editor.question.is_none(), "{row}: Enter asks");
                assert_eq!(st.theme_name(), p.label, "{row}: Enter changes the theme");
            }
        }
    }
}

/// How a test reaches a changed theme.
#[derive(Clone, Copy, Debug)]
enum Way {
    /// Green, changed.
    Ships,
    /// Ember, changed, with its file.
    Own,
    /// Ember, changed, then its file went and the folder was read again.
    GoneAtReload,
    /// A start whose `[unsaved]` names ember.toml, which is gone.
    GoneAtStart,
    /// A start whose `[unsaved]` names ember.toml, of a newer version.
    NewerAtStart,
    /// A start whose `[unsaved]` names ember.toml, which cannot be read.
    #[cfg(unix)]
    UnreadableAtStart,
}

/// What every part of the window gives for one changed theme.
struct Row {
    way: Way,
    name: &'static str,
    lit: Option<&'static str>,
    save_as: &'static str,
    /// The file Save writes, or None when Save waits.
    save: Option<&'static str>,
    /// Rename and Delete work.
    file_acts: Option<ThemeFault>,
    revert: &'static str,
    /// The name in the leave question and its Name field.
    leave: (&'static str, &'static str),
    /// The file Keep writes, and the name inside it.
    keep: (&'static str, &'static str),
    /// `theme` and `[unsaved]` `from` in `settings.toml`.
    stored: &'static str,
}

/// The settings of a start on Ember, changed and not saved.
const EMBER_CHANGED: &str = "version = 1\ntheme = \"ember.toml\"\n\n[unsaved]\nfrom = \"ember.toml\"\nname = \"Ember\"\nbase = \"#123456\"\nbloom = 0.25\n";

/// A window on the changed theme `way` gives, with its settings in `d`.
fn reach(d: &TempDir, way: Way) -> Studio {
    let start = |d: &TempDir, file: Option<&str>| {
        if let Some(f) = file {
            std::fs::write(themes(d).join("ember.toml"), f).unwrap();
        }
        std::fs::write(d.0.join(store::SETTINGS), EMBER_CHANGED).unwrap();
        Studio::new(None).with_settings(d.0.clone())
    };
    match way {
        Way::Ships => {
            let mut s = window(d);
            nudge(&mut s);
            s
        }
        Way::Own => {
            let mut s = on_ember(d);
            nudge(&mut s);
            s
        }
        Way::GoneAtReload => {
            let mut s = on_ember(d);
            nudge(&mut s);
            std::fs::remove_file(themes(d).join("ember.toml")).unwrap();
            s.reload_themes();
            s
        }
        Way::GoneAtStart => start(d, None),
        Way::NewerAtStart => start(d, Some(&EMBER.replace("version = 1", "version = 2"))),
        #[cfg(unix)]
        Way::UnreadableAtStart => {
            use std::os::unix::fs::PermissionsExt;
            let f = themes(d).join("ember.toml");
            std::fs::write(&f, EMBER).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o000)).unwrap();
            let s = start(d, None);
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
            s
        }
    }
}

fn rows() -> Vec<Row> {
    let gone = |way| Row {
        way,
        name: "Ember",
        lit: None,
        save_as: "Ember",
        save: Some("ember-2.toml"),
        file_acts: Some(ThemeFault::FileGone),
        revert: "Green",
        leave: ("Ember", "Ember"),
        keep: ("ember-2.toml", "Ember"),
        stored: "ember.toml",
    };
    let mut v = vec![
        Row {
            way: Way::Ships,
            name: "Green",
            lit: Some("Green"),
            save_as: "Green 2",
            save: None,
            file_acts: Some(ThemeFault::ShippedTheme),
            revert: "Green",
            leave: ("Green", "Green 2"),
            keep: ("green-2.toml", "Green 2"),
            stored: "Green",
        },
        Row {
            way: Way::Own,
            name: "Ember",
            lit: Some("Ember"),
            save_as: "Ember 2",
            save: Some("ember.toml"),
            file_acts: None,
            revert: "Ember",
            leave: ("Ember", ""),
            keep: ("ember.toml", "Ember"),
            stored: "ember.toml",
        },
        Row {
            save: Some("ember.toml"),
            keep: ("ember.toml", "Ember"),
            ..gone(Way::GoneAtReload)
        },
        Row {
            save: Some("ember.toml"),
            keep: ("ember.toml", "Ember"),
            ..gone(Way::GoneAtStart)
        },
        // The file is there but not listed: a new file goes beside it.
        gone(Way::NewerAtStart),
    ];
    #[cfg(unix)]
    if lock_bites() {
        v.push(gone(Way::UnreadableAtStart));
    } else {
        eprintln!("skipped, the permission does not bite: UnreadableAtStart");
    }
    v
}

/// A file with no permission cannot be opened by this user, as it can by
/// root.
#[cfg(unix)]
fn lock_bites() -> bool {
    use std::os::unix::fs::PermissionsExt;
    let d = TempDir::new("bites");
    let f = d.0.join("probe");
    std::fs::write(&f, "").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o000)).unwrap();
    let bites = std::fs::File::open(&f).is_err();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
    bites
}

/// The text of the file `file` of the themes folder, or None.
fn theme_text(d: &TempDir, file: &str) -> Option<String> {
    std::fs::read_to_string(themes(d).join(file)).ok()
}

/// Every part of the window that names the theme in use agrees, for a
/// changed theme that ships, one of the person's own, and one whose own
/// theme is gone however that came.
#[test]
fn every_reader_of_the_theme_in_use_agrees() {
    let rows = rows();
    for r in &rows {
        let way = format!("{:?}", r.way);
        let changed = format!("{}, changed", r.name);
        let used = |s: &Studio| s.in_use();

        // Status line, tag, picker label and lit row, menu rows and mark.
        let d = TempDir::new("reader");
        let s = reach(&d, r.way);
        assert_eq!(s.theme_name(), changed, "{way}: status");
        assert_eq!(s.pane_tag(), TAG_CHANGED, "{way}: tag");
        let lit = s.theme_row().map(|i| s.themes[i].name.to_string());
        assert_eq!(lit.as_deref(), r.lit, "{way}: lit row");
        assert!(s.checked(Command::Theme(ThemeId::Changed)), "{way}: mark");
        for i in 0..s.themes.len() {
            assert!(!s.checked(Command::Theme(ThemeId::At(i))), "{way}: row {i}");
        }
        assert!(
            menus::native_items(&s).iter().any(|i| i.title == changed),
            "{way}: menu"
        );
        assert_eq!(used(&s).name, r.name, "{way}: name");

        // Save as.
        let mut s = s;
        s.start_naming(false);
        assert_eq!(
            s.theme_editor.naming.unwrap().text,
            r.save_as,
            "{way}: save as"
        );

        // Save.
        let d = TempDir::new("reader");
        let mut s = reach(&d, r.way);
        let (why_file, why_save) = s.why_waits();
        assert_eq!(why_save.is_none(), r.save.is_some(), "{way}: save waits");
        assert_eq!(
            why_file,
            r.file_acts.as_ref().map(ThemeFault::message),
            "{way}: rename and delete wait"
        );
        ask(
            &mut s,
            Asked {
                save: true,
                ..Asked::default()
            },
        );
        if used(&s).row.is_none() {
            // No file to write over: Save asks for a name, as Save as.
            assert_eq!(
                s.theme_editor.naming.as_ref().map(|n| n.text.as_str()),
                Some(r.save_as),
                "{way}: save names"
            );
            ask(
                &mut s,
                Asked {
                    named: true,
                    ..Asked::default()
                },
            );
        }
        match r.save {
            Some(f) => {
                assert!(s.changed.is_none(), "{way}: saved");
                assert!(s.adrift.is_none(), "{way}: saved");
                let t = theme_text(&d, f).unwrap();
                assert!(t.contains(&format!("name = \"{}\"", r.name)), "{way}: {t}");
                assert_eq!(s.own_file().as_deref(), Some(f), "{way}: save");
            }
            None => assert_eq!(s.theme_name(), changed, "{way}: save waits"),
        }

        // Rename and Delete.
        let d = TempDir::new("reader");
        let mut s = reach(&d, r.way);
        s.start_naming(true);
        assert_eq!(
            s.theme_editor.naming.is_some(),
            r.file_acts.is_none(),
            "{way}: rename row"
        );
        s.theme_editor.naming = None;
        ask(
            &mut s,
            Asked {
                delete: true,
                ..Asked::default()
            },
        );
        assert_eq!(
            s.theme_editor.question.is_some(),
            r.file_acts.is_none(),
            "{way}: delete question"
        );
        s.theme_editor.question = None;
        let renamed = s.rename_theme("Night");
        assert_eq!(renamed.err(), r.file_acts, "{way}: rename");
        let d = TempDir::new("reader");
        let mut s = reach(&d, r.way);
        let deleted = s.delete_theme();
        assert_eq!(deleted.err(), r.file_acts, "{way}: delete");

        // Revert.
        let d = TempDir::new("reader");
        let mut s = reach(&d, r.way);
        ask(
            &mut s,
            Asked {
                revert: true,
                ..Asked::default()
            },
        );
        assert_eq!(s.theme_name(), r.revert, "{way}: revert");
        assert!(s.adrift.is_none(), "{way}: revert");
        assert!(s.theme_row().is_some(), "{way}: revert");

        // The leave question, then Keep.
        let d = TempDir::new("reader");
        let mut s = reach(&d, r.way);
        s.pick_theme(1);
        let Some(ThemeQuestion::Leave { name, .. }) = s.theme_editor.question.clone() else {
            panic!("{way}: no leave question");
        };
        assert_eq!(
            (used(&s).name.as_str(), name.as_str()),
            r.leave,
            "{way}: leave"
        );
        assert!(s.answer_theme(Answer::Save).unwrap().is_ok(), "{way}: keep");
        let t = theme_text(&d, r.keep.0).unwrap();
        assert!(
            t.contains(&format!("name = \"{}\"", r.keep.1)),
            "{way}: {t}"
        );
        assert_eq!(s.theme_name(), "Amber", "{way}: keep");

        // settings.toml after an edit and after close, and the next start.
        let d = TempDir::new("reader");
        let mut s = reach(&d, r.way);
        nudge(&mut s);
        for when in ["edit", "close"] {
            if when == "close" {
                ask(
                    &mut s,
                    Asked {
                        close: true,
                        ..Asked::default()
                    },
                );
            }
            s.save_settings();
            let text = std::fs::read(d.0.join(store::SETTINGS)).unwrap();
            let st = store::Settings::parse(&text).unwrap();
            assert_eq!(st.theme(), Some(r.stored), "{way}: theme after {when}");
            let u = st.unsaved().unwrap();
            assert_eq!(u.from, r.stored, "{way}: from after {when}");
            assert_eq!(u.name.as_deref(), Some(r.name), "{way}: name after {when}");
        }
        let back = Studio::new(None).with_settings(d.0.clone());
        assert_eq!(back.theme_name(), changed, "{way}: next start");
        // The unreadable file is readable again by now, so the change sits
        // on its row.
        #[cfg(unix)]
        let home = matches!(r.way, Way::UnreadableAtStart);
        #[cfg(not(unix))]
        let home = false;
        let adrift = r.lit.is_none() && !home;
        assert_eq!(back.in_use().adrift(), adrift, "{way}: next start");
    }
    assert!(rows.len() >= 5, "rows: {}", rows.len());
}

/// A change whose own name is taken proposes that name with a number.
#[test]
fn a_change_adrift_proposes_a_free_name() {
    let d = TempDir::new("adrift");
    let mut s = reach(&d, Way::GoneAtReload);
    std::fs::write(themes(&d).join("other.toml"), EMBER).unwrap();
    s.reload_themes();
    assert!(s.in_use().adrift());
    assert_eq!(s.save_as_name(), "Ember 2");
}

/// A change adrift sits on its row again when its file comes back.
#[test]
fn a_change_adrift_comes_home_with_its_file() {
    let d = TempDir::new("adrift");
    let mut s = reach(&d, Way::GoneAtReload);
    std::fs::write(themes(&d).join("ember.toml"), EMBER).unwrap();
    s.reload_themes();
    assert!(s.adrift.is_none());
    assert_eq!(s.own_file().as_deref(), Some("ember.toml"));
    assert_eq!(s.theme_name(), "Ember, changed");
}

/// A field that keeps typed text apart from its value until focus leaves.
#[derive(Debug, Clone, Copy)]
enum Typed {
    /// The base colour field, with three digits.
    Hex,
    /// The number box of the Hue slider.
    Hue,
    /// The number box of the Vignette slider, on the Screen tab.
    Vignette,
}

/// How focus leaves the field.
#[derive(Debug, Clone, Copy)]
enum Away {
    /// A click on a control drawn before the field, which takes focus.
    Before,
    /// A click on a control drawn after the field.
    After,
    /// A click on a part of the pane with no control.
    Empty,
    Tab,
    ShiftTab,
    /// Escape, which also closes the pane in the same frame.
    Escape,
    /// The close button, drawn before the field.
    Close,
    /// Save, in the frame focus leaves: the edit applies before it.
    Save,
}

impl Typed {
    /// The label of the field, the text typed, and the label of a control
    /// drawn before it and of one drawn after it.
    fn parts(
        self,
    ) -> (
        &'static str,
        &'static str,
        (Role, &'static str),
        &'static str,
    ) {
        match self {
            Typed::Hex => (BASE, "f80", (Role::Tab, TAB_NAMES[0]), COLOUR_SLIDERS[0]),
            Typed::Hue => (
                COLOUR_SLIDERS[0],
                "200",
                (Role::TextInput, BASE),
                COLOUR_SLIDERS[2],
            ),
            Typed::Vignette => (
                Strengths::NAMES[1],
                "0.3",
                (Role::TextInput, Strengths::NAMES[0]),
                Strengths::NAMES[2],
            ),
        }
    }

    /// The value typed is the value of the theme.
    fn holds(self, s: &Studio) -> bool {
        match self {
            Typed::Hex => s.spec().base == palette::parse_hex("ff8800").unwrap().0,
            Typed::Hue => {
                s.theme_editor.sliders[0] == 200.0
                    && s.spec().base != palette::parse_hex("ff7a1a").unwrap().0
            }
            Typed::Vignette => (s.spec().strengths.vignette - 0.3).abs() < 1e-4,
        }
    }
}

/// Each field that keeps typed text apart from its value, against each way
/// focus can leave it. The text typed is the value after the frame,
/// whatever took the focus and wherever that control is drawn.
#[test]
fn a_field_commits_its_text_whatever_takes_the_focus() {
    let ways = [
        Away::Before,
        Away::After,
        Away::Empty,
        Away::Tab,
        Away::ShiftTab,
        Away::Escape,
        Away::Close,
        Away::Save,
    ];
    let mut rows = 0;
    // Every row runs, so a failure names all the rows that fail.
    let mut failed = Vec::new();
    for field in [Typed::Hex, Typed::Hue, Typed::Vignette] {
        for way in ways {
            let row = format!("{field:?} {way:?}");
            let d = TempDir::new(&format!("focus-{rows}"));
            rows += 1;
            let s = on_ember(&d);
            let mut h = egui_kittest::Harness::builder()
                .with_size(egui::vec2(1280.0, 720.0))
                .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
            h.state_mut().theme_editor.open = true;
            if matches!(field, Typed::Vignette) {
                h.state_mut().theme_editor.tab = ThemeTab::Screen;
            }
            h.run_steps(3);
            let (label, text, before, after) = field.parts();
            h.get_by_role_and_label(Role::TextInput, label).focus();
            h.run_steps(2);
            h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            h.run_steps(1);
            h.get_by_role_and_label(Role::TextInput, label)
                .type_text(text);
            h.run_steps(2);
            assert!(
                !field.holds(h.state()) || matches!(field, Typed::Hex),
                "{row}: early"
            );
            match way {
                Away::Before => h.get_by_role_and_label(before.0, before.1).click(),
                Away::After => h.get_by_role_and_label(Role::TextInput, after).click(),
                Away::Empty => {
                    let r = h.state().theme_pane_rect();
                    press_at(&h, egui::pos2(r.left() + 40.0, r.top() + 12.0));
                }
                Away::Tab => h.key_press(egui::Key::Tab),
                Away::ShiftTab => {
                    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Tab);
                }
                Away::Escape => h.key_press(egui::Key::Escape),
                Away::Close => h.get_by_role_and_label(Role::Button, CLOSE).click(),
                Away::Save => h.get_by_role_and_label(Role::Button, BUTTONS[4]).click(),
            }
            h.run_steps(3);
            let s = h.state();
            if !field.holds(s) {
                failed.push(row);
                continue;
            }
            match way {
                Away::Escape | Away::Close => assert!(!s.theme_editor.open, "{row}"),
                Away::Save => {
                    assert!(s.changed.is_none(), "{row}");
                    let saved = store::read_folder(&themes(&d));
                    let (_, spec) = saved
                        .themes
                        .iter()
                        .find(|(f, _)| f == "ember.toml")
                        .unwrap();
                    assert_eq!(spec.base, s.spec().base, "{row}");
                    assert_eq!(spec.strengths, s.spec().strengths, "{row}");
                }
                _ => assert!(s.theme_editor.open, "{row}"),
            }
        }
    }
    assert_eq!(failed, Vec::<String>::new());
    assert_eq!(rows, 24);
}

/// A theme of the person's own on a quick colour, every strength at 0.35,
/// the screen moving.
const MINE: &str = "version = 1\nname = \"Mine\"\nbase = \"#5aff7f\"\n[screen]\nbloom = 0.35\nvignette = 0.35\nscanlines = 0.35\ncurvature = 0.35\npersistence = 0.35\nnoise = 0.35\nflicker = 0.35\nband = 0.35\nmask = 0.35\naberration = 0.35\nmoves = true\nrate = 30\n";

/// A window on Mine, with a known adapter so no strength is held.
fn on_mine(d: &TempDir) -> Studio {
    std::fs::write(themes(d).join("mine.toml"), MINE).unwrap();
    let mut s = window(d);
    let i = s.theme_index("Mine").unwrap();
    s.pick_theme(i);
    s.screen.adapter = crate::screen::Adapter::Hardware;
    s.refresh_theme();
    s
}

/// A control of the two tabs of the editor.
#[derive(Debug, Clone, Copy)]
enum Ctl {
    /// The strength slider at this place.
    Strength(usize),
    /// The colour slider at this place.
    Colour(usize),
    Hex,
    Quick,
    Moves,
    Rate,
}

/// How the person puts the value back.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Back {
    /// By the same control.
    Same,
    /// By typing the old number in the control's field.
    Typed,
    Revert,
}

/// Type `text` in the field named `label`, then leave it with Tab.
fn type_in(h: &mut egui_kittest::Harness<'_, Studio>, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.get_by_role_and_label(Role::TextInput, label)
        .type_text(text);
    h.run_steps(2);
    h.key_press(egui::Key::Tab);
    h.run_steps(3);
}

/// Press `key` on the slider named `name`.
fn key_on(h: &mut egui_kittest::Harness<'_, Studio>, name: &str, key: egui::Key) {
    h.get_by_role_and_label(Role::Slider, name).focus();
    h.run_steps(2);
    h.key_press(key);
    h.run_steps(3);
}

/// Click the radio button `label`.
fn radio(h: &mut egui_kittest::Harness<'_, Studio>, label: &str) {
    h.get_by_role_and_label(Role::RadioButton, label).click();
    h.run_steps(3);
}

/// Each control of the two tabs, changed and put back by the same
/// control, by typing the old number and by Revert. Put back, the theme
/// is not changed: the pane's tag, the status line and the menu say so, a
/// pick asks nothing and `settings.toml` holds no `[unsaved]`.
#[test]
fn a_value_put_back_is_no_change() {
    use Back::*;
    let mut rows: Vec<(Ctl, Back)> = Vec::new();
    for i in 0..Strengths::NAMES.len() {
        for b in [Same, Typed, Revert] {
            rows.push((Ctl::Strength(i), b));
        }
    }
    for i in 0..COLOUR_SLIDERS.len() {
        for b in [Same, Typed, Revert] {
            rows.push((Ctl::Colour(i), b));
        }
    }
    for b in [Same, Typed, Revert] {
        rows.push((Ctl::Quick, b));
    }
    // The field is typed in: by the same control is by typing.
    for b in [Typed, Revert] {
        rows.push((Ctl::Hex, b));
    }
    for c in [Ctl::Moves, Ctl::Rate] {
        for b in [Same, Revert] {
            rows.push((c, b));
        }
    }
    let mut failed = Vec::new();
    for (k, (ctl, back)) in rows.iter().copied().enumerate() {
        let row = format!("{ctl:?} {back:?}");
        let d = TempDir::new(&format!("back-{k}"));
        let s = on_mine(&d);
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1280.0, 1000.0))
            .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
        h.state_mut().theme_editor.open = true;
        if matches!(ctl, Ctl::Strength(_) | Ctl::Moves | Ctl::Rate) {
            h.state_mut().theme_editor.tab = ThemeTab::Screen;
        }
        h.run_steps(3);
        // Change it.
        let mut undo = egui::Key::ArrowLeft;
        let mut old = String::new();
        match ctl {
            Ctl::Strength(i) => {
                old = "0.35".into();
                key_on(&mut h, Strengths::NAMES[i], egui::Key::ArrowRight);
            }
            Ctl::Colour(i) => {
                let v = h.state().theme_editor.sliders[i];
                old = format!("{v:.0}");
                key_on(&mut h, COLOUR_SLIDERS[i], egui::Key::ArrowRight);
                if h.state().theme_editor.sliders[i] == v {
                    key_on(&mut h, COLOUR_SLIDERS[i], egui::Key::ArrowLeft);
                    undo = egui::Key::ArrowRight;
                }
            }
            Ctl::Hex => type_in(&mut h, BASE, "123456"),
            Ctl::Quick => radio(&mut h, QUICK[2].0),
            Ctl::Moves => {
                h.get_by_role_and_label(Role::CheckBox, MOVES).click();
                h.run_steps(3);
            }
            Ctl::Rate => radio(&mut h, "60"),
        }
        if h.state().changed.is_none() {
            failed.push(format!("{row}: no change made"));
            continue;
        }
        // Put it back.
        match (ctl, back) {
            (_, Revert) => {
                h.get_by_role_and_label(Role::Button, BUTTONS[2]).click();
                h.run_steps(3);
            }
            (Ctl::Strength(i), Same) => key_on(&mut h, Strengths::NAMES[i], undo),
            (Ctl::Colour(i), Same) => key_on(&mut h, COLOUR_SLIDERS[i], undo),
            (Ctl::Strength(i), Typed) => type_in(&mut h, Strengths::NAMES[i], &old),
            (Ctl::Colour(i), Typed) => type_in(&mut h, COLOUR_SLIDERS[i], &old),
            (Ctl::Quick, Same) => radio(&mut h, QUICK[0].0),
            (Ctl::Quick | Ctl::Hex, Typed) => type_in(&mut h, BASE, "5aff7f"),
            (Ctl::Moves, _) => {
                h.get_by_role_and_label(Role::CheckBox, MOVES).click();
                h.run_steps(3);
            }
            (Ctl::Rate, _) => radio(&mut h, "30"),
            (Ctl::Hex, Same) => unreachable!(),
        }
        let s = h.state_mut();
        let row_mark = s.checked(Command::Theme(ThemeId::At(s.current)));
        let mut faults = Vec::new();
        if s.changed.is_some() {
            faults.push("still changed".to_owned());
        }
        if s.pane_tag() != TAG_YOURS {
            faults.push(format!("tag {}", s.pane_tag()));
        }
        if s.status.1 != "Theme: Mine." {
            faults.push(format!("status {}", s.status.1));
        }
        if !row_mark || s.checked(Command::Theme(ThemeId::Changed)) {
            faults.push("menu mark".to_owned());
        }
        s.save_settings();
        let file = std::fs::read_to_string(d.0.join(store::SETTINGS)).unwrap();
        if file.contains("[unsaved]") {
            faults.push("settings keep [unsaved]".to_owned());
        }
        s.pick_theme(1);
        if s.theme_editor.question.is_some() || s.current != 1 {
            faults.push("a pick asks".to_owned());
        }
        if !faults.is_empty() {
            failed.push(format!("{row}: {}", faults.join(", ")));
        }
    }
    assert_eq!(failed, Vec::<String>::new());
    assert_eq!(rows.len(), 48);
}

/// A value that differs from the saved one only below what the file
/// holds is no change; one that differs at that precision is.
#[test]
fn a_change_below_the_file_precision_is_no_change() {
    let d = TempDir::new("precision");
    let mut s = on_mine(&d);
    s.edit_theme(|t| t.strengths.vignette = 0.354);
    assert!(s.changed.is_none(), "0.354 is written 0.35");
    assert_eq!(s.pane_tag(), TAG_YOURS);
    s.edit_theme(|t| t.strengths.vignette = 0.356);
    assert!(s.changed.is_some(), "0.356 is written 0.36");
    // Saved and read back, a value equals itself.
    let line = s.save_theme().unwrap();
    assert!(line.contains("saved"), "{line}");
    assert!(s.changed.is_none());
    s.reload_themes();
    s.edit_theme(|t| t.strengths.vignette = 0.36);
    assert!(s.changed.is_none());
}

/// An `[unsaved]` equal to the theme it names is no change at start.
/// The file keeps it until the next write the person causes.
#[test]
fn an_unsaved_equal_to_its_theme_is_dropped_on_the_next_write() {
    let d = TempDir::new("start-equal");
    std::fs::write(themes(&d).join("mine.toml"), MINE).unwrap();
    let screen = MINE.split("[screen]\n").nth(1).unwrap();
    let settings = format!(
        "version = 1\ntheme = \"mine.toml\"\n\n[unsaved]\nfrom = \"mine.toml\"\nname = \"Mine\"\nbase = \"#5aff7f\"\n{screen}"
    );
    std::fs::write(d.0.join(store::SETTINGS), &settings).unwrap();
    let mut s = Studio::new(None).with_settings(d.0.clone());
    assert!(s.changed.is_none());
    assert_eq!(s.theme_name(), "Mine");
    assert!(s.checked(Command::Theme(ThemeId::At(s.current))));
    let file = std::fs::read_to_string(d.0.join(store::SETTINGS)).unwrap();
    assert_eq!(file, settings, "nothing written at start");
    s.compact_header = !s.compact_header;
    s.save_settings();
    let file = std::fs::read_to_string(d.0.join(store::SETTINGS)).unwrap();
    assert!(!file.contains("[unsaved]"), "{file}");
}

/// A reload that reads a file now equal to the change ends the change.
#[test]
fn a_reload_to_the_same_values_ends_the_change() {
    let d = TempDir::new("reload-equal");
    let mut s = on_mine(&d);
    s.edit_theme(|t| t.strengths.vignette = 0.5);
    assert!(s.changed.is_some());
    let text = MINE.replace("vignette = 0.35", "vignette = 0.50");
    std::fs::write(themes(&d).join("mine.toml"), text).unwrap();
    s.reload_themes();
    assert!(s.changed.is_none());
    assert_eq!(s.pane_tag(), TAG_YOURS);
}

/// A change whose own theme is gone stays changed, even with the values
/// of Green, the stand-in under it.
#[test]
fn a_change_adrift_stays_changed() {
    let d = TempDir::new("adrift-stays");
    let mut s = reach(&d, Way::GoneAtReload);
    assert!(s.in_use().adrift());
    let green = crate::theme::SHIPPED[0].clone();
    s.edit_theme(|t| {
        t.base = green.base;
        t.strengths = green.strengths;
        t.moves = green.moves;
        t.rate = green.rate;
    });
    assert!(s.changed.is_some());
    assert_eq!(s.pane_tag(), TAG_CHANGED);
    assert!(s.checked(Command::Theme(ThemeId::Changed)));
}
