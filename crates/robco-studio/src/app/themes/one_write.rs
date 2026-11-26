// SPDX-License-Identifier: GPL-3.0-or-later

//! One act of the person gives at most one write of `settings.toml`, and
//! that write holds the final state. A draft is never written away before
//! it is in a theme file.

use super::*;
use crate::app::{Leaving, ThemeQuestion};
use crate::editor::Answer;
use crate::menus::Command;
use crate::store::tests::TempDir;
use crate::store::writes;
use eframe::egui;

const EMBER: &str = "version = 1\nname = \"Ember\"\nbase = \"#ff7a1a\"\n[screen]\nbloom = 0.45\n";
/// The draft's mark: a value no theme file holds before a save.
const DRAFT: f32 = 0.77;

/// A window on a folder with the theme Ember in use, with a draft when
/// `draft`, and the writes of the setup taken away.
fn setup(d: &TempDir, draft: bool) -> Studio {
    let t = d.0.join(store::THEMES);
    std::fs::create_dir_all(&t).unwrap();
    std::fs::write(t.join("ember.toml"), EMBER).unwrap();
    std::fs::write(d.0.join("outside.toml"), EMBER.replace("Ember", "Dawn")).unwrap();
    let mut s = Studio::new(None).with_settings(d.0.clone());
    let i = s.find_key("ember.toml").unwrap();
    s.pick_theme(i);
    if draft {
        s.edit_theme(|t| t.strengths.bloom = DRAFT);
    }
    s.frame_settings(false);
    writes::take();
    s
}

fn ask(s: &mut Studio, to: Leaving) {
    assert_eq!(s.leave_for(to), Ok(None));
}

fn file(d: &TempDir) -> Leaving {
    Leaving::File {
        path: d.0.join("outside.toml"),
        name: "Dawn".into(),
    }
}

fn green() -> Leaving {
    Leaving::Theme(SHIPPED[0].name.to_string())
}

fn ember_holds_draft(d: &TempDir) -> bool {
    std::fs::read_dir(d.0.join(store::THEMES))
        .unwrap()
        .any(|e| {
            std::fs::read_to_string(e.unwrap().path())
                .unwrap()
                .contains("bloom = 0.77")
        })
}

type Act = fn(&mut Studio, &TempDir);

/// Each row: the act, whether it starts with a draft, the writes it makes,
/// and whether every write must keep the draft in `[unsaved]` unless a
/// theme file holds it.
fn rows() -> Vec<(&'static str, bool, usize, bool, Act)> {
    vec![
        ("pick", false, 1, false, |s, _| s.pick_theme(1)),
        ("edit", false, 1, false, |s, _| {
            s.edit_theme(|t| t.strengths.bloom = 0.6)
        }),
        ("revert", true, 1, false, |s, _| s.take_theme(s.current)),
        ("save", true, 1, false, |s, _| {
            assert!(s.save_theme().is_ok())
        }),
        ("save as", true, 1, false, |s, _| {
            assert!(s.save_theme_as("Dusk", false).is_ok())
        }),
        ("replace", true, 1, false, |s, _| {
            assert!(s.save_theme_as("Ember", true).is_ok())
        }),
        ("rename", false, 1, false, |s, _| {
            assert!(s.rename_theme("Dusk").is_ok())
        }),
        ("rename changed", true, 1, true, |s, _| {
            assert!(s.rename_theme("Dusk").is_ok())
        }),
        ("rename by the row", true, 1, true, |s, _| {
            s.start_naming(true);
            s.theme_editor.naming.as_mut().unwrap().text = "Dusk".into();
            s.finish_naming();
        }),
        ("delete", false, 1, false, |s, _| {
            s.theme_editor.question = Some(ThemeQuestion::Delete);
            assert!(s.answer_theme(Answer::Save).unwrap().is_ok());
        }),
        ("delete, cancel", false, 0, false, |s, _| {
            s.theme_editor.question = Some(ThemeQuestion::Delete);
            assert!(s.answer_theme(Answer::Cancel).is_none());
        }),
        ("add a file", false, 1, false, |s, d| {
            assert!(
                s.add_theme_file(&d.0.join("outside.toml"))
                    .unwrap()
                    .is_some()
            );
        }),
        ("add a file, asked", true, 0, true, |s, d| {
            assert_eq!(s.add_theme_file(&d.0.join("outside.toml")), Ok(None));
        }),
        ("add a file, keep", true, 1, true, |s, d| {
            ask(s, file(d));
            assert!(s.answer_theme(Answer::Save).unwrap().is_ok());
        }),
        ("add a file, do not save", true, 1, false, |s, d| {
            ask(s, file(d));
            assert!(s.answer_theme(Answer::DoNotSave).unwrap().is_ok());
        }),
        ("add a file, cancel", true, 0, true, |s, d| {
            ask(s, file(d));
            assert!(s.answer_theme(Answer::Cancel).is_none());
        }),
        ("pick, asked", true, 0, true, |s, _| s.pick_theme(0)),
        ("pick, keep", true, 1, true, |s, _| {
            ask(s, green());
            assert!(s.answer_theme(Answer::Save).unwrap().is_ok());
        }),
        ("pick, do not save", true, 1, false, |s, _| {
            ask(s, green());
            assert!(s.answer_theme(Answer::DoNotSave).unwrap().is_ok());
        }),
        ("pick, cancel", true, 0, true, |s, _| {
            ask(s, green());
            assert!(s.answer_theme(Answer::Cancel).is_none());
        }),
        ("high legibility", true, 1, true, |s, _| {
            s.run(&egui::Context::default(), Command::HighLegibility);
        }),
        ("compact header", true, 1, true, |s, _| {
            s.run(&egui::Context::default(), Command::CompactHeader);
        }),
        ("reload", true, 0, true, |s, _| s.reload_themes()),
        ("close", true, 1, true, |s, _| s.save_settings()),
    ]
}

#[test]
fn one_act_writes_the_settings_once_with_its_final_state() {
    for (what, draft, want, keeps, act) in rows() {
        let d = TempDir::new("one-write");
        let mut s = setup(&d, draft);
        act(&mut s, &d);
        // The next frame, with no button held.
        s.frame_settings(false);
        let got = writes::take();
        assert_eq!(got.len(), want, "{what}: {got:#?}");
        // The file on disk is the final state: a write now gives it again.
        let on_disk = std::fs::read_to_string(d.0.join(store::SETTINGS)).unwrap();
        s.save_settings();
        assert_eq!(writes::take(), vec![on_disk.clone()], "{what}");
        if keeps {
            for w in &got {
                assert!(
                    w.contains("[unsaved]") || ember_holds_draft(&d),
                    "{what} wrote the draft away: {w}"
                );
            }
            assert!(
                on_disk.contains("[unsaved]") || ember_holds_draft(&d),
                "{what} lost the draft"
            );
        }
    }
}

#[test]
fn a_failed_settings_write_on_rename_keeps_the_draft() {
    let d = TempDir::new("rename-fail");
    let mut s = setup(&d, true);
    writes::fail(true);
    let r = s.rename_theme("Dusk");
    writes::fail(false);
    assert!(r.is_ok());
    let back = Studio::new(None).with_settings(d.0.clone());
    assert_eq!(back.key(back.current), "ember.toml");
    assert_eq!(back.spec().strengths.bloom, DRAFT);
    assert_eq!(back.theme_name(), "Dusk, changed");
}

#[test]
fn a_failed_theme_write_on_rename_keeps_the_draft() {
    let d = TempDir::new("rename-fail-file");
    let mut s = setup(&d, true);
    let ember = d.0.join(store::THEMES).join("ember.toml");
    let mut p = std::fs::metadata(&ember).unwrap().permissions();
    p.set_readonly(true);
    std::fs::set_permissions(&ember, p.clone()).unwrap();
    let r = s.rename_theme("Dusk");
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "undo the test's own lock"
    )]
    p.set_readonly(false);
    std::fs::set_permissions(&ember, p).unwrap();
    assert!(r.is_err());
    assert!(writes::take().is_empty());
    let back = Studio::new(None).with_settings(d.0.clone());
    assert_eq!(back.key(back.current), "ember.toml");
    assert_eq!(back.spec().strengths.bloom, DRAFT);
    assert_eq!(back.theme_name(), "Ember, changed");
}
