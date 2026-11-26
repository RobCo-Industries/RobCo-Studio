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
