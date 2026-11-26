// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::store::tests::TempDir;

const EMBER: &str = "version = 1\nname = \"Ember\"\nbase = \"#ff7a1a\"\n[screen]\nbloom = 0.45\n";

fn themes(d: &TempDir) -> PathBuf {
    let t = d.0.join(store::THEMES);
    std::fs::create_dir_all(&t).unwrap();
    t
}

fn settings_text(d: &TempDir) -> String {
    std::fs::read_to_string(d.0.join(store::SETTINGS)).unwrap()
}

#[test]
fn without_a_folder_nothing_is_written() {
    let mut s = Studio::new(None);
    assert!(s.themes_folder().is_none());
    s.pick_theme(1);
    s.save_settings();
    assert_eq!(s.save_theme_as("Mine", false), Err(ThemeFault::NoFolder));
}

#[test]
fn own_themes_are_listed_and_the_last_theme_comes_back() {
    let d = TempDir::new("start");
    std::fs::write(themes(&d).join("ember.toml"), EMBER).unwrap();
    let mut s = Studio::new(None).with_settings(d.0.clone());
    assert_eq!(s.themes.len(), SHIPPED.len() + 1);
    assert_eq!(s.themes[6].name, "Ember");
    // A start alone writes nothing.
    assert!(!d.0.join(store::SETTINGS).exists());
    s.pick_theme(6);
    assert!(settings_text(&d).contains("theme = \"ember.toml\""));
    let s = Studio::new(None).with_settings(d.0.clone());
    assert_eq!(s.current, 6);
    assert_eq!(s.theme_name(), "Ember");
}

#[test]
fn a_saved_changed_theme_comes_back_changed() {
    let d = TempDir::new("changed");
    let mut s = Studio::new(None).with_settings(d.0.clone());
    s.pick_theme(1);
    s.edit_theme(|t| t.strengths.bloom = 0.8);
    s.save_settings();
    let back = Studio::new(None).with_settings(d.0.clone());
    assert_eq!(back.current, 1);
    assert_eq!(back.theme_name(), "Amber, changed");
    assert_eq!(back.spec().strengths.bloom, 0.8);
}

#[test]
fn a_gone_file_takes_green() {
    let d = TempDir::new("gone");
    std::fs::write(
        d.0.join(store::SETTINGS),
        "version = 1\ntheme = \"ember.toml\"\n",
    )
    .unwrap();
    let s = Studio::new(None).with_settings(d.0.clone());
    assert_eq!(s.current, 0);
    assert_eq!(
        s.status.1,
        "%STUDIO-W-THEMEGONE, the theme file ember.toml is gone. Theme: Green."
    );
}

#[test]
fn a_broken_settings_file_gives_the_defaults_and_writes_nothing() {
    let d = TempDir::new("broken");
    let text = "version = 1\nhigh_legibility = \n";
    std::fs::write(d.0.join(store::SETTINGS), text).unwrap();
    let mut s = Studio::new(None).with_settings(d.0.clone());
    assert!(!s.theme.high_legibility);
    assert!(
        s.status
            .1
            .starts_with("%STUDIO-W-NOSETTINGS, could not read settings.toml: line 2")
    );
    s.pick_theme(2);
    s.save_settings();
    assert_eq!(settings_text(&d), text);
}

#[test]
fn a_bad_file_says_why_and_the_window_starts() {
    let d = TempDir::new("bad");
    std::fs::write(themes(&d).join("bad.toml"), "version = 1\n").unwrap();
    let s = Studio::new(None).with_settings(d.0.clone());
    assert_eq!(s.themes.len(), SHIPPED.len());
    assert_eq!(s.refused.len(), 1);
    assert!(s.status.1.contains("no base colour"), "{}", s.status.1);
}

#[test]
fn save_as_rename_and_delete() {
    let d = TempDir::new("states");
    let mut s = Studio::new(None).with_settings(d.0.clone());
    s.edit_theme(|t| t.strengths.noise = 0.2);
    assert_eq!(
        s.save_theme_as("GREEN", false),
        Err(ThemeFault::Shipped("Green".into()))
    );
    assert_eq!(s.save_theme_as("a/b", false), Err(ThemeFault::BadName));
    assert_eq!(s.save_theme_as("  ", false), Err(ThemeFault::NoName));
    assert_eq!(
        s.save_theme_as("Ember", false),
        Ok("%STUDIO-S-THEMESAVED, theme Ember saved as ember.toml".into())
    );
    assert_eq!(s.theme_name(), "Ember");
    assert!(s.changed.is_none());
    assert_eq!(s.spec().strengths.noise, 0.2);
    let e = s.save_theme_as("ember", false).unwrap_err();
    assert_eq!(
        e.message(),
        "A theme named Ember exists. Replace writes over it."
    );
    assert!(s.save_theme_as("ember", true).is_ok());
    assert_eq!(s.themes.len(), SHIPPED.len() + 1);

    s.edit_theme(|t| t.strengths.noise = 0.3);
    assert!(s.save_theme().is_ok());
    let file = std::fs::read_to_string(themes(&d).join("ember.toml")).unwrap();
    assert!(file.contains("noise = 0.30"), "{file}");

    assert_eq!(s.rename_theme("Dusk"), Ok("Theme: Dusk.".into()));
    assert_eq!(s.theme_files[s.current].as_deref(), Some("ember.toml"));
    assert_eq!(s.theme_name(), "Dusk");

    assert_eq!(
        s.delete_theme(),
        Ok("Theme Dusk deleted. Theme: Green.".into())
    );
    assert_eq!(s.current, 0);
    assert!(!themes(&d).join("ember.toml").exists());
    assert_eq!(s.delete_theme(), Err(ThemeFault::ShippedTheme));
    assert_eq!(s.save_theme(), Err(ThemeFault::ShippedTheme));
}

#[test]
fn an_added_file_comes_out_clean() {
    let d = TempDir::new("add");
    let src = d.0.join("from.toml");
    std::fs::write(
        &src,
        "version = 1\nname = \"Ev\\u202Eil\"\nbase = \"#ff7a1a\"\nsparkle = 1\n",
    )
    .unwrap();
    let mut s = Studio::new(None).with_settings(d.0.clone());
    assert!(s.add_theme_file(&src).is_ok());
    assert_eq!(s.theme_name(), "Evil");
    let file = std::fs::read_to_string(themes(&d).join("evil.toml")).unwrap();
    assert!(
        !file.contains("sparkle") && !file.contains('\u{202e}'),
        "{file}"
    );
    std::fs::write(&src, "version = 1\n").unwrap();
    assert!(matches!(
        s.add_theme_file(&src),
        Err(ThemeFault::NotTheme(_))
    ));
}

#[test]
fn theme_index_finds_own_themes() {
    let d = TempDir::new("index");
    std::fs::write(themes(&d).join("ember.toml"), EMBER).unwrap();
    let s = Studio::new(None).with_settings(d.0.clone());
    assert_eq!(s.theme_index("amber"), Some(1));
    assert_eq!(s.theme_index("EMBER"), Some(6));
    assert_eq!(s.theme_index("ember.toml"), Some(6));
    assert_eq!(s.theme_index("dusk"), None);
}

/// The folder before an action, in the table of writes.
#[derive(Debug, Clone, Copy)]
enum Before {
    Empty,
    One,
    /// 64 files, and the new name sorts before them.
    FullFirst,
    /// 64 files, and the new name sorts after them.
    FullLast,
    /// Two files both named Ember.
    Twin,
}

/// An action that writes or removes a theme file, then reads the folder.
#[derive(Debug, Clone, Copy)]
enum Act {
    Save,
    SaveAs,
    Replace,
    Rename,
    Delete,
    Add,
}

/// What must hold after the action.
#[derive(Debug, Clone, PartialEq)]
enum After {
    /// Listed and in use, from this file.
    Kept(&'static str),
    /// The file is gone and Green is in use.
    Deleted,
    /// Refused: nothing on disk changed and the change stays.
    Refused(ThemeFault),
}

fn folder_for(d: &TempDir, before: Before) -> PathBuf {
    let t = themes(d);
    let ember = |f: &str| std::fs::write(t.join(f), EMBER).unwrap();
    match before {
        Before::Empty => {}
        Before::One => ember("ember.toml"),
        Before::FullFirst | Before::FullLast => {
            ember("ember.toml");
            for i in 0..63 {
                let text = format!("version = 1\nname = \"M{i:02}\"\nbase = \"#ff7a1a\"\n");
                std::fs::write(t.join(format!("m{i:02}.toml")), text).unwrap();
            }
        }
        Before::Twin => {
            ember("ember.toml");
            ember("ember-2.toml");
        }
    }
    t
}

fn new_name(before: Before) -> &'static str {
    match before {
        Before::FullLast => "Zed",
        _ => "Aaa",
    }
}

#[test]
fn every_write_lists_the_theme_or_keeps_the_change() {
    use After::{Deleted, Kept, Refused};
    use Before::*;
    let rows: &[(Act, Before, After)] = &[
        (Act::Save, Empty, Refused(ThemeFault::ShippedTheme)),
        (Act::Save, One, Kept("ember.toml")),
        (Act::Save, FullFirst, Kept("ember.toml")),
        (Act::Save, FullLast, Kept("ember.toml")),
        (Act::Save, Twin, Kept("ember.toml")),
        (Act::SaveAs, Empty, Kept("aaa.toml")),
        (Act::SaveAs, One, Kept("aaa.toml")),
        (Act::SaveAs, FullFirst, Refused(ThemeFault::Full)),
        (Act::SaveAs, FullLast, Refused(ThemeFault::Full)),
        (Act::SaveAs, Twin, Kept("aaa.toml")),
        (Act::Replace, Empty, Kept("ember.toml")),
        (Act::Replace, One, Kept("ember.toml")),
        (Act::Replace, FullFirst, Kept("ember.toml")),
        (Act::Replace, FullLast, Kept("ember.toml")),
        // The first twin by file name is the one replaced.
        (Act::Replace, Twin, Kept("ember-2.toml")),
        (Act::Rename, Empty, Refused(ThemeFault::ShippedTheme)),
        (Act::Rename, One, Kept("ember.toml")),
        (Act::Rename, FullFirst, Kept("ember.toml")),
        (Act::Rename, FullLast, Kept("ember.toml")),
        (Act::Rename, Twin, Kept("ember.toml")),
        (Act::Delete, Empty, Refused(ThemeFault::ShippedTheme)),
        (Act::Delete, One, Deleted),
        (Act::Delete, FullFirst, Deleted),
        (Act::Delete, FullLast, Deleted),
        (Act::Delete, Twin, Deleted),
        (Act::Add, Empty, Kept("aaa.toml")),
        (Act::Add, One, Kept("aaa.toml")),
        (Act::Add, FullFirst, Refused(ThemeFault::Full)),
        (Act::Add, FullLast, Refused(ThemeFault::Full)),
        (Act::Add, Twin, Kept("aaa.toml")),
    ];
    for (act, before, after) in rows {
        let row = format!("{act:?} {before:?}");
        let d = TempDir::new("writes");
        let t = folder_for(&d, *before);
        let mut s = Studio::new(None).with_settings(d.0.clone());
        if let Some(i) = s.find_key("ember.toml") {
            s.pick_theme(i);
        }
        s.edit_theme(|t| t.strengths.noise = 0.2);
        let files = store::toml_files(&t);
        let name = new_name(*before);
        let src = d.0.join("from.toml");
        let got = match act {
            Act::Save => s.save_theme(),
            Act::SaveAs => s.save_theme_as(name, false),
            Act::Replace => s.save_theme_as("Ember", true),
            Act::Rename => s.rename_theme("Dusk"),
            Act::Delete => s.delete_theme(),
            Act::Add => {
                let text = format!("version = 1\nname = \"{name}\"\nbase = \"#ff7a1a\"\n");
                std::fs::write(&src, text).unwrap();
                s.add_theme_file(&src)
            }
        };
        match after {
            Kept(file) => {
                assert!(got.is_ok(), "{row}: {got:?}");
                assert_eq!(s.theme_files[s.current].as_deref(), Some(*file), "{row}");
                // Add takes the file's numbers, the others the edit.
                let noise = if matches!(act, Act::Add) { 0.0 } else { 0.2 };
                assert_eq!(s.spec().strengths.noise, noise, "{row}");
                // Rename keeps a change not saved; the others save it.
                assert_eq!(s.changed.is_some(), matches!(act, Act::Rename), "{row}");
            }
            Deleted => {
                assert!(got.is_ok(), "{row}: {got:?}");
                assert_eq!(s.current, 0, "{row}");
                assert!(!t.join("ember.toml").exists(), "{row}");
            }
            Refused(fault) => {
                assert_eq!(got.as_ref().err(), Some(fault), "{row}");
                assert!(!fault.message().is_empty(), "{row}");
                assert_eq!(store::toml_files(&t), files, "{row}");
                assert!(s.changed.is_some(), "{row}");
                assert_eq!(s.spec().strengths.noise, 0.2, "{row}");
            }
        }
    }
}

#[test]
fn a_full_folder_says_why() {
    assert_eq!(
        ThemeFault::Full.message(),
        "The themes folder holds 64 files, the most Studio reads. Delete one first. The theme is not saved."
    );
}

#[test]
fn one_rule_for_a_theme_name() {
    let d = TempDir::new("names");
    let t = themes(&d);
    let mine = |f: &str, n: &str| {
        std::fs::write(
            t.join(f),
            format!("version = 1\nname = \"{n}\"\nbase = \"#ff7a1a\"\n"),
        )
        .unwrap();
    };
    mine("ete.toml", "Été");
    let s = Studio::new(None).with_settings(d.0.clone());
    let at = s.find_key("ete.toml");
    assert!(at.is_some());
    // --theme.
    assert_eq!(s.theme_index("été"), at);
    assert_eq!(s.theme_index("ÉTÉ"), at);
    assert_eq!(s.theme_index("GREEN"), Some(0));
    assert_eq!(s.theme_index("green"), Some(0));
    // The settings' theme key names a shipped theme the same way.
    assert_eq!(s.find_key("GREEN"), Some(0));
    // A taken name, as save as and rename check it.
    assert_eq!(
        s.check_name("été", None),
        Err(ThemeFault::Exists("Été".into()))
    );
    assert_eq!(s.check_name("été", at), Ok("été".into()));
    assert_eq!(
        s.check_name("GREEN", None),
        Err(ThemeFault::Shipped("Green".into()))
    );
    assert!(store::same_name("Été", "été"));
    assert!(store::same_name("GREEN", "green"));
    // No normal form: a combining accent stays apart from the composed
    // letter, and a typed one is refused as outside the set.
    assert!(!store::same_name("e\u{301}te\u{301}", "été"));
    assert_eq!(s.check_name("e\u{301}te", None), Err(ThemeFault::BadName));
}

#[test]
fn twins_by_any_case_are_both_listed() {
    let d = TempDir::new("twincase");
    let t = themes(&d);
    for (f, n) in [("a.toml", "Été"), ("b.toml", "ÉTÉ")] {
        std::fs::write(
            t.join(f),
            format!("version = 1\nname = \"{n}\"\nbase = \"#ff7a1a\"\n"),
        )
        .unwrap();
    }
    let s = Studio::new(None).with_settings(d.0.clone());
    assert_eq!(s.themes[6].name, "Été, a.toml");
    assert_eq!(s.themes[7].name, "ÉTÉ, b.toml");
}

#[test]
fn a_newer_or_bad_settings_file_gives_defaults_and_writes_nothing() {
    for v in ["2", "0", "-1", "\"1\"", "1.0"] {
        let d = TempDir::new("newer");
        let text = format!("version = {v}\ntheme = \"Amber\"\nhigh_legibility = true\n");
        std::fs::write(d.0.join(store::SETTINGS), &text).unwrap();
        let mut s = Studio::new(None).with_settings(d.0.clone());
        assert_eq!(s.current, 0, "{v}");
        assert!(!s.theme.high_legibility, "{v}");
        assert!(s.status.1.starts_with("%STUDIO-W-NOSETTINGS"), "{v}");
        let said = s.said.iter().filter(|l| l.contains("NOSETTINGS")).count();
        assert_eq!(said, 1, "{v}");
        s.pick_theme(2);
        s.edit_theme(|t| t.strengths.noise = 0.4);
        s.save_settings();
        assert_eq!(settings_text(&d), text, "{v}");
    }
}

#[test]
fn unknown_settings_keys_survive_a_pick_an_edit_and_a_close() {
    let rows: &[(&str, &[&str])] = &[
        ("version = 1\nsparkle = 7\n", &["sparkle"]),
        (
            "version = 1\ntheme = \"Amber\"\n[unsaved]\nfrom = \"Amber\"\nbase = \"#ffb93f\"\nglint = \"x\"\n",
            &["unsaved.glint"],
        ),
        (
            "version = 1\n[unsaved]\nfrom = \"Green\"\nbase = \"#5aff7f\"\n[unsaved.extra]\nk = 1\n",
            &["unsaved.extra.k"],
        ),
        ("version = 1\n[later]\npanes = [1, 2]\n", &["later.panes"]),
        (
            "high_legibility = \"yes\"\ntext_size = 115\n",
            &["text_size"],
        ),
    ];
    for (text, keys) in rows {
        let d = TempDir::new("keep");
        std::fs::write(d.0.join(store::SETTINGS), text).unwrap();
        let before: toml::Table = text.parse().unwrap();
        let mut s = Studio::new(None).with_settings(d.0.clone());
        s.pick_theme(2);
        s.edit_theme(|t| t.strengths.noise = 0.4);
        s.save_settings();
        let after: toml::Table = settings_text(&d).parse().unwrap();
        let at = |t: &toml::Table, path: &str| -> Option<toml::Value> {
            let mut parts = path.split('.');
            let mut v = t.get(parts.next()?)?.clone();
            for p in parts {
                v = v.get(p)?.clone();
            }
            Some(v)
        };
        for k in *keys {
            assert!(at(&before, k).is_some(), "{k}");
            assert_eq!(at(&after, k), at(&before, k), "{text}: {k}");
        }
        // A known key of the wrong kind becomes the value in use.
        assert_eq!(
            after.get("high_legibility"),
            Some(&toml::Value::Boolean(false)),
            "{text}"
        );
        assert_eq!(
            after.get("version"),
            Some(&toml::Value::Integer(1)),
            "{text}"
        );
    }
}
