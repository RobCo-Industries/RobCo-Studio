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
    /// Ember is in use, then another program adds 64 files that sort
    /// before ember.toml.
    Pushed,
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
    /// A pick of Amber while the change is not saved, then Save in the
    /// question.
    Keep,
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
        Before::One | Before::Pushed => ember("ember.toml"),
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
    let pushed = ThemeFault::PastLimit("ember.toml".into());
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
        (Act::Keep, Empty, Kept("aaa.toml")),
        (Act::Keep, One, Kept("ember.toml")),
        (Act::Keep, FullFirst, Kept("ember.toml")),
        (Act::Keep, Twin, Kept("ember.toml")),
        // Files added from outside push ember.toml past the limit.
        (Act::Save, Pushed, Refused(pushed.clone())),
        (Act::SaveAs, Pushed, Refused(ThemeFault::Full)),
        (Act::Replace, Pushed, Refused(pushed.clone())),
        (Act::Rename, Pushed, Refused(pushed.clone())),
        (Act::Delete, Pushed, Deleted),
        (Act::Add, Pushed, Refused(ThemeFault::Full)),
        (Act::Keep, Pushed, Refused(pushed.clone())),
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
        if matches!(before, Pushed) {
            for i in 0..64 {
                std::fs::write(t.join(format!("a{i:02}.toml")), "version = 1\n").unwrap();
            }
        }
        let files = store::toml_files(&t).files().to_vec();
        let bytes: Vec<Vec<u8>> = files
            .iter()
            .map(|f| std::fs::read(t.join(f)).unwrap())
            .collect();
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
                // A change not saved: Add asks first, as a pick does.
                assert_eq!(s.add_theme_file(&src), Ok(None), "{row}");
                assert!(s.theme_editor.question.is_some(), "{row}");
                s.answer_theme(crate::editor::Answer::DoNotSave).unwrap()
            }
            Act::Keep => {
                // A theme that ships is kept under the proposed name.
                s.pick_theme(1);
                if let Some(super::super::theme_pane::ThemeQuestion::Leave { name, .. }) =
                    &mut s.theme_editor.question
                {
                    *name = "Aaa".into();
                }
                assert!(s.theme_editor.question.is_some(), "{row}");
                s.answer_theme(crate::editor::Answer::Save).unwrap()
            }
        };
        match after {
            Kept(file) => {
                assert!(got.is_ok(), "{row}: {got:?}");
                if matches!(act, Act::Keep) {
                    // Keep saves the change, then Amber takes its place.
                    let saved = store::read_folder(&t);
                    let (_, spec) = saved.themes.iter().find(|(f, _)| f == file).unwrap();
                    assert_eq!(spec.strengths.noise, 0.2, "{row}");
                    assert_eq!(s.current, 1, "{row}");
                    continue;
                }
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
                assert_eq!(store::toml_files(&t).files().to_vec(), files, "{row}");
                for (f, b) in files.iter().zip(&bytes) {
                    assert_eq!(&std::fs::read(t.join(f)).unwrap(), b, "{row}: {f}");
                }
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
    assert_eq!(
        ThemeFault::PastLimit("ember.toml".into()).message(),
        "Other files now sort before ember.toml in the themes folder, and Studio reads only the first 64. Move or delete one of them first. The theme is not saved."
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

/// What `settings.toml` holds after a run.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Wrote {
    /// The file is byte for byte as it was.
    Nothing,
    /// The stored theme and the stored changed theme are as they were.
    Kept,
    /// The theme this key names, written over the stored one.
    Theme(&'static str),
}

/// One thing that can fail when Studio reads its files.
struct Case {
    row: &'static str,
    // A file gone at start or at a reload takes Green as a stand-in, and
    // a change not saved stays on screen under its own name: the person's
    // numbers are not lost.
    /// Make the folder before Studio starts.
    before: fn(&TempDir),
    /// Make the fault after the start, then read the themes folder again.
    /// None: the fault is there at start.
    reload: Option<fn(&TempDir)>,
    /// The theme the window shows.
    shown: &'static str,
    /// A code the status line says once, or none.
    says: Option<&'static str>,
    /// What closing with no act writes.
    close: Wrote,
    /// Whether a pick and an edit are written.
    acts_written: bool,
}

/// The settings every case starts from: Ember, changed and not saved.
const STORED: &str = "version = 1\ntheme = \"ember.toml\"\nlater = \"kept\"\n\n[unsaved]\nfrom = \"ember.toml\"\nname = \"Ember\"\nbase = \"#123456\"\nbloom = 0.25\n";

fn stored(d: &TempDir) {
    std::fs::write(themes(d).join("ember.toml"), EMBER).unwrap();
    std::fs::write(themes(d).join("dusk.toml"), EMBER.replace("Ember", "Dusk")).unwrap();
    std::fs::write(d.0.join(store::SETTINGS), STORED).unwrap();
}

/// A path whose permission is taken away, and given back when dropped, so
/// the temporary folder can go.
#[cfg(unix)]
struct Locked(PathBuf, u32);

#[cfg(unix)]
impl Locked {
    /// Take every permission from `p`. None when the permission does not
    /// bite for this user, as for root: the row is skipped.
    fn new(p: PathBuf) -> Option<Locked> {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&p).unwrap().permissions().mode();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
        let l = Locked(p, mode);
        let bites = if l.0.is_dir() {
            std::fs::read_dir(&l.0).is_err()
        } else {
            std::fs::File::open(&l.0).is_err()
        };
        bites.then_some(l)
    }
}

#[cfg(unix)]
impl Drop for Locked {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(self.1));
    }
}

#[cfg(unix)]
thread_local! {
    static LOCKS: std::cell::RefCell<Vec<Locked>> = const { std::cell::RefCell::new(Vec::new()) };
    static SKIP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Lock `p` for the rest of the case, or mark the case to be skipped.
#[cfg(unix)]
fn lock(p: PathBuf) {
    match Locked::new(p) {
        Some(l) => LOCKS.with(|v| v.borrow_mut().push(l)),
        None => SKIP.with(|s| s.set(true)),
    }
}

#[cfg(unix)]
fn unlock() {
    LOCKS.with(|v| v.borrow_mut().clear());
}

/// Fill the themes folder with files that come before `ember.toml`, so
/// Studio reads only those.
fn many(d: &TempDir) {
    for n in 0..store::MAX_FILES {
        std::fs::write(themes(d).join(format!("a{n:02}.toml")), EMBER).unwrap();
    }
}

/// Put a folder in the place of `ember.toml`.
fn as_folder(d: &TempDir) {
    let f = themes(d).join("ember.toml");
    std::fs::remove_file(&f).unwrap();
    std::fs::create_dir(f).unwrap();
}

/// Give the theme in `ember.toml` the name `name`.
fn rename_inside(d: &TempDir, name: &str) {
    std::fs::write(
        themes(d).join("ember.toml"),
        EMBER.replace("\"Ember\"", &format!("\"{name}\"")),
    )
    .unwrap();
}

fn cases() -> Vec<Case> {
    let mut v = vec![
        Case {
            row: "the settings folder missing",
            before: |d| std::fs::remove_dir_all(&d.0).unwrap(),
            reload: None,
            shown: "Green",
            says: None,
            close: Wrote::Theme("Green"),
            acts_written: true,
        },
        Case {
            row: "settings.toml missing",
            before: |d| {
                stored(d);
                std::fs::remove_file(d.0.join(store::SETTINGS)).unwrap();
            },
            reload: None,
            shown: "Green",
            says: None,
            close: Wrote::Theme("Green"),
            acts_written: true,
        },
        Case {
            row: "settings.toml too large",
            before: |d| {
                stored(d);
                let mut t = STORED.to_owned();
                t.push_str(&format!("# {}\n", "x".repeat(1 << 20)));
                std::fs::write(d.0.join(store::SETTINGS), t).unwrap();
            },
            reload: None,
            shown: "Green",
            says: Some("%STUDIO-W-NOSETTINGS"),
            close: Wrote::Nothing,
            acts_written: false,
        },
        Case {
            row: "settings.toml not UTF-8",
            before: |d| {
                stored(d);
                std::fs::write(
                    d.0.join(store::SETTINGS),
                    b"version = 1\ntheme = \"\xff\"\n",
                )
                .unwrap();
            },
            reload: None,
            shown: "Green",
            says: Some("%STUDIO-W-NOSETTINGS"),
            close: Wrote::Nothing,
            acts_written: false,
        },
        Case {
            row: "settings.toml not TOML",
            before: |d| {
                stored(d);
                std::fs::write(d.0.join(store::SETTINGS), "version = 1\ntheme = \n").unwrap();
            },
            reload: None,
            shown: "Green",
            says: Some("%STUDIO-W-NOSETTINGS"),
            close: Wrote::Nothing,
            acts_written: false,
        },
        Case {
            row: "settings.toml of a newer version",
            before: |d| {
                stored(d);
                std::fs::write(
                    d.0.join(store::SETTINGS),
                    STORED.replace("version = 1", "version = 2"),
                )
                .unwrap();
            },
            reload: None,
            shown: "Green",
            says: Some("%STUDIO-W-NOSETTINGS"),
            close: Wrote::Nothing,
            acts_written: false,
        },
        Case {
            row: "the themes folder missing",
            before: |d| {
                stored(d);
                std::fs::remove_dir_all(themes(d)).unwrap();
            },
            reload: None,
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMEGONE"),
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "the themes folder missing at a reload",
            before: stored,
            reload: Some(|d| std::fs::remove_dir_all(themes(d)).unwrap()),
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMEGONE"),
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "the selected theme's file gone",
            before: |d| {
                stored(d);
                std::fs::remove_file(themes(d).join("ember.toml")).unwrap();
            },
            reload: None,
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMEGONE"),
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "the selected theme's file gone at a reload",
            before: stored,
            reload: Some(|d| std::fs::remove_file(themes(d).join("ember.toml")).unwrap()),
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMEGONE"),
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "the selected theme's file past the files Studio reads",
            before: |d| {
                stored(d);
                many(d);
            },
            reload: None,
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMEGONE"),
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "the selected theme's file past the files Studio reads at a reload",
            before: stored,
            reload: Some(many),
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMEGONE"),
            close: Wrote::Kept,
            acts_written: true,
        },
    ];
    // Each way the store refuses the selected theme's file, at start and
    // at a reload. Studio keeps the change on screen and the stored choice.
    macro_rules! refused {
        ($what:literal, $code:literal, $bytes:expr) => {
            v.extend([
                Case {
                    row: concat!("the selected theme's file ", $what),
                    before: |d| {
                        stored(d);
                        std::fs::write(themes(d).join("ember.toml"), $bytes).unwrap();
                    },
                    reload: None,
                    shown: "Ember, changed",
                    says: Some($code),
                    close: Wrote::Kept,
                    acts_written: true,
                },
                Case {
                    row: concat!("the selected theme's file ", $what, " at a reload"),
                    before: stored,
                    reload: Some(|d| std::fs::write(themes(d).join("ember.toml"), $bytes).unwrap()),
                    shown: "Ember, changed",
                    says: Some($code),
                    close: Wrote::Kept,
                    acts_written: true,
                },
            ])
        };
    }
    refused!("no longer a theme", "%STUDIO-E-NOTTHEME", "version = 1\n");
    refused!(
        "of a newer version",
        "%STUDIO-E-THEMENEW",
        EMBER.replace("version = 1", "version = 2")
    );
    refused!(
        "of a bad version",
        "%STUDIO-E-NOTTHEME",
        EMBER.replace("version = 1", "version = 0")
    );
    refused!(
        "with no version",
        "%STUDIO-E-NOTTHEME",
        EMBER.replace("version = 1\n", "")
    );
    refused!("not TOML", "%STUDIO-E-NOTTHEME", "version = \n");
    refused!(
        "not UTF-8",
        "%STUDIO-E-NOTTHEME",
        b"version = 1\nname = \"\xff\"\n"
    );
    refused!(
        "too large",
        "%STUDIO-E-NOTTHEME",
        format!("{EMBER}# {}\n", "x".repeat(1 << 15))
    );
    // A folder in the file's place: Studio lists plain files only.
    v.extend([
        Case {
            row: "the selected theme's file a folder",
            before: |d| {
                stored(d);
                as_folder(d);
            },
            reload: None,
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMEGONE"),
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "the selected theme's file a folder at a reload",
            before: stored,
            reload: Some(as_folder),
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMEGONE"),
            close: Wrote::Kept,
            acts_written: true,
        },
    ]);
    // A file still listed under another name: the choice is the file, so
    // it stays.
    v.extend([
        Case {
            row: "the selected theme renamed inside",
            before: |d| {
                stored(d);
                rename_inside(d, "Night");
            },
            reload: None,
            shown: "Night, changed",
            says: None,
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "the selected theme renamed inside at a reload",
            before: stored,
            reload: Some(|d| rename_inside(d, "Night")),
            shown: "Ember, changed",
            says: None,
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "the selected theme a twin",
            before: |d| {
                stored(d);
                rename_inside(d, "Dusk");
            },
            reload: None,
            shown: "Dusk, ember.toml, changed",
            says: Some("%STUDIO-W-THEMETWICE"),
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "the selected theme a twin at a reload",
            before: stored,
            reload: Some(|d| rename_inside(d, "Dusk")),
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMETWICE"),
            close: Wrote::Kept,
            acts_written: true,
        },
    ]);
    #[cfg(unix)]
    v.extend([
        Case {
            row: "the settings folder unreadable",
            before: |d| {
                stored(d);
                lock(d.0.clone());
            },
            reload: None,
            shown: "Green",
            says: Some("%STUDIO-W-NOSETTINGS"),
            close: Wrote::Nothing,
            acts_written: false,
        },
        Case {
            row: "settings.toml unreadable",
            before: |d| {
                stored(d);
                lock(d.0.join(store::SETTINGS));
            },
            reload: None,
            shown: "Green",
            says: Some("%STUDIO-W-NOSETTINGS"),
            close: Wrote::Nothing,
            acts_written: false,
        },
        Case {
            row: "the themes folder unreadable",
            before: |d| {
                stored(d);
                lock(themes(d));
            },
            reload: None,
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMEKEPT"),
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "the themes folder unreadable at a reload",
            before: stored,
            reload: Some(|d| lock(themes(d))),
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMESREAD"),
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "another file of the themes folder unreadable",
            before: |d| {
                stored(d);
                lock(themes(d).join("dusk.toml"));
            },
            reload: None,
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMEREAD"),
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "the selected theme's file unreadable",
            before: |d| {
                stored(d);
                lock(themes(d).join("ember.toml"));
            },
            reload: None,
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMEKEPT"),
            close: Wrote::Kept,
            acts_written: true,
        },
        Case {
            row: "the selected theme's file unreadable at a reload",
            before: stored,
            reload: Some(|d| lock(themes(d).join("ember.toml"))),
            shown: "Ember, changed",
            says: Some("%STUDIO-W-THEMEREAD"),
            close: Wrote::Kept,
            acts_written: true,
        },
    ]);
    v
}

/// What `settings.toml` holds now, read with every lock given back for
/// the read.
fn settings_now(d: &TempDir) -> Option<Vec<u8>> {
    #[cfg(unix)]
    let paths: Vec<PathBuf> =
        LOCKS.with(|v| v.borrow_mut().drain(..).map(|l| l.0.clone()).collect());
    let bytes = std::fs::read(d.0.join(store::SETTINGS)).ok();
    #[cfg(unix)]
    for p in paths {
        lock(p);
    }
    bytes
}

/// What the settings folder holds now, with every lock given back.
fn settings_after(d: &TempDir) -> Option<Vec<u8>> {
    #[cfg(unix)]
    unlock();
    std::fs::read(d.0.join(store::SETTINGS)).ok()
}

/// Start, make the fault, then close, pick or edit, as `act` does.
/// None when the row's permission does not bite.
/// The window after a run, and `settings.toml` before and after it.
type Ran = (Studio, Option<Vec<u8>>, Option<Vec<u8>>);

fn run(c: &Case, act: fn(&mut Studio)) -> Option<Ran> {
    #[cfg(unix)]
    SKIP.with(|s| s.set(false));
    let d = TempDir::new("fault");
    (c.before)(&d);
    #[cfg(unix)]
    if SKIP.with(std::cell::Cell::get) {
        unlock();
        return None;
    }
    let before = settings_now(&d);
    let mut s = Studio::new(None).with_settings(d.0.clone());
    if let Some(r) = c.reload {
        r(&d);
        s.reload_themes();
    }
    #[cfg(unix)]
    if SKIP.with(std::cell::Cell::get) {
        unlock();
        return None;
    }
    act(&mut s);
    s.save_settings();
    let after = settings_after(&d);
    Some((s, before, after))
}

fn parsed(bytes: &Option<Vec<u8>>) -> store::Settings {
    store::Settings::parse(bytes.as_deref().unwrap()).unwrap()
}

/// A read that fails never costs the person a stored choice: one row for
/// each thing that can fail at start and at a reload.
#[test]
fn a_failed_read_keeps_the_stored_choice() {
    let mut ran = 0;
    for c in cases() {
        let row = c.row;
        // Close with no act.
        let Some((s, before, after)) = run(&c, |_| {}) else {
            eprintln!("skipped, the permission does not bite: {row}");
            continue;
        };
        ran += 1;
        assert_eq!(s.theme_name(), c.shown, "{row}");
        let said: Vec<&String> = s.said.iter().collect();
        match c.says {
            Some(code) => assert_eq!(
                said.iter().filter(|l| l.starts_with(code)).count(),
                1,
                "{row}: {said:?}"
            ),
            None => assert!(
                !said.iter().any(|l| l.starts_with("%STUDIO-W-THEMEKEPT")),
                "{row}: {said:?}"
            ),
        }
        match c.close {
            Wrote::Nothing => assert_eq!(after, before, "{row}"),
            Wrote::Kept => {
                let a = parsed(&after);
                assert_eq!(a.theme(), Some("ember.toml"), "{row}");
                let u = a.unsaved().unwrap();
                assert_eq!(u.from, "ember.toml", "{row}");
                assert_eq!(store::hex(u.spec.base), "#123456", "{row}");
                assert!(a.text().unwrap().contains("later = \"kept\""), "{row}");
            }
            Wrote::Theme(t) => assert_eq!(parsed(&after).theme(), Some(t), "{row}"),
        }
        // A pick on purpose, past the question a change not saved asks.
        let (_, before, after) = run(&c, |s| s.take_theme(1)).unwrap();
        if c.acts_written {
            assert_eq!(parsed(&after).theme(), Some("Amber"), "{row}: pick");
        } else {
            assert_eq!(after, before, "{row}: pick");
        }
        // An edit on purpose.
        let (_, before, after) = run(&c, |s| s.edit_theme(|t| t.strengths.bloom = 0.9)).unwrap();
        if c.acts_written {
            let u = parsed(&after).unsaved().unwrap();
            assert_eq!(u.spec.strengths.bloom, 0.9, "{row}: edit");
        } else {
            assert_eq!(after, before, "{row}: edit");
        }
    }
    assert!(ran >= 32, "rows run: {ran}");
}

/// A save into a themes folder Studio cannot read writes nothing: it
/// cannot know which file names are free.
#[cfg(unix)]
#[test]
fn a_save_into_an_unreadable_folder_writes_nothing() {
    let d = TempDir::new("unlisted");
    stored(&d);
    let mut s = Studio::new(None).with_settings(d.0.clone());
    let Some(l) = Locked::new(themes(&d)) else {
        eprintln!("skipped, the permission does not bite");
        return;
    };
    let e = s.save_theme_as("Night", false).unwrap_err();
    drop(l);
    assert!(
        e.message().contains("could not read the themes folder"),
        "{e:?}"
    );
    assert!(!themes(&d).join("night.toml").exists());
}

#[test]
fn a_file_name_of_any_length_saves_renames_and_deletes_or_is_refused() {
    // The stem's bytes: with `.toml`, 6 to 260 bytes, so the row of 250
    // is a name of 255 bytes, the limit of many systems.
    for stem in [1usize, 40, 200, 250, 255] {
        let file = format!("{}.toml", "e".repeat(stem));
        for action in ["save", "rename", "delete"] {
            let row = format!("{} bytes, {action}", file.len());
            let d = TempDir::new("namelen");
            if std::fs::write(themes(&d).join(&file), EMBER).is_err() {
                eprintln!("skipped {row}: the name cannot be made here");
                continue;
            }
            let mut s = Studio::new(None).with_settings(d.0.clone());
            let Some(i) = s.find_key(&file) else {
                assert!(file.len() > store::FILE_MAX, "{row}");
                let why = format!("its name is over {} bytes", store::FILE_MAX);
                assert!(
                    s.refused
                        .iter()
                        .any(|(f, t)| *f == file && t.contains(&why)),
                    "{row}"
                );
                continue;
            };
            assert!(file.len() <= store::FILE_MAX, "{row}");
            s.pick_theme(i);
            match action {
                "save" => {
                    s.edit_theme(|t| t.strengths.noise = 0.3);
                    assert!(s.save_theme().is_ok(), "{row}");
                    let text = std::fs::read_to_string(themes(&d).join(&file)).unwrap();
                    assert!(text.contains("noise = 0.30"), "{row}");
                }
                "rename" => {
                    assert_eq!(s.rename_theme("Dusk"), Ok("Theme: Dusk.".into()), "{row}");
                }
                _ => {
                    assert!(s.delete_theme().is_ok(), "{row}");
                    assert!(!themes(&d).join(&file).exists(), "{row}");
                }
            }
        }
    }
}

#[test]
fn a_listed_theme_is_picked_by_its_file() {
    let mut s = Studio::new(None);
    let calm = Spec {
        name: "Calm".into(),
        ..SHIPPED[0].clone()
    };
    s.list_theme(calm, "calm.toml");
    let i = s.themes().len() - 1;
    s.pick_theme(i);
    assert_eq!(s.current, i);
    assert_eq!(s.in_use().name, "Calm");
}
