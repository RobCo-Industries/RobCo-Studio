// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::theme::SHIPPED;

/// A folder under the system's temp folder, removed when dropped, so a
/// failing test cleans up too.
pub(crate) struct TempDir(pub PathBuf);

impl TempDir {
    pub(crate) fn new(tag: &str) -> TempDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "robco-studio-store-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn with(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    move |name| {
        pairs
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| OsString::from(v))
    }
}

#[cfg(not(windows))]
const ABS: &str = "/c";
#[cfg(windows)]
const ABS: &str = "C:\\c";

#[test]
fn the_variable_is_the_folder_itself() {
    assert_eq!(
        folder(with(&[("ROBCO_STUDIO_SETTINGS", ABS)])).unwrap(),
        PathBuf::from(ABS)
    );
}

#[test]
fn a_relative_variable_is_refused() {
    assert!(folder(with(&[("ROBCO_STUDIO_SETTINGS", "rel/s")])).is_err());
}

#[test]
fn an_empty_variable_counts_as_unset() {
    let got = folder(with(&[
        ("ROBCO_STUDIO_SETTINGS", ""),
        ("HOME", ABS),
        ("APPDATA", ABS),
    ]))
    .unwrap();
    assert!(got.starts_with(ABS), "{got:?}");
    assert!(got.ends_with(Path::new("robco").join("studio")), "{got:?}");
}

#[test]
fn no_home_is_refused() {
    assert!(folder(with(&[])).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_takes_application_support() {
    assert_eq!(
        folder(with(&[("HOME", "/h")])).unwrap(),
        Path::new("/h")
            .join("Library")
            .join("Application Support")
            .join("robco")
            .join("studio")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_takes_an_absolute_xdg_config_home() {
    assert_eq!(
        folder(with(&[("XDG_CONFIG_HOME", "/x"), ("HOME", "/h")])).unwrap(),
        PathBuf::from("/x/robco/studio")
    );
    assert_eq!(
        folder(with(&[("XDG_CONFIG_HOME", "x"), ("HOME", "/h")])).unwrap(),
        PathBuf::from("/h/.config/robco/studio")
    );
}

#[cfg(windows)]
#[test]
fn windows_takes_appdata() {
    assert_eq!(
        folder(with(&[("APPDATA", "C:\\a")])).unwrap(),
        Path::new("C:\\a").join("robco").join("studio")
    );
}

/// The TOML escape of every character outside printable ASCII.
fn esc(raw: &str) -> String {
    raw.chars()
        .map(|c| match c {
            ' '..='~' => c.to_string(),
            c if (c as u32) < 0x10000 => format!("\\u{:04X}", c as u32),
            c => format!("\\U{:08X}", c as u32),
        })
        .collect()
}

fn read(text: &str) -> Result<Loaded, Note> {
    read_theme("ember.toml", text.as_bytes())
}

const MIN: &str = "version = 1\nbase = \"#FF7A1A\"\n";

#[test]
fn version_and_base_are_a_whole_theme() {
    let l = read(MIN).unwrap();
    assert!(l.notes.is_empty());
    assert_eq!(l.spec.name, "ember");
    assert_eq!(l.spec.base, Color32::from_rgb(0xff, 0x7a, 0x1a));
    assert_eq!(l.spec.strengths, Strengths::NONE);
    assert!(!l.spec.moves);
    assert_eq!(l.spec.rate, 30);
}

#[test]
fn an_unknown_key_is_ignored() {
    let l = read(&format!("{MIN}sparkle = 3\n[screen]\nglow = 1\n")).unwrap();
    let texts: Vec<&str> = l.notes.iter().map(|n| n.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "%STUDIO-W-THEMEKEY, ember.toml has a key Studio does not know: sparkle. The key is ignored.",
            "%STUDIO-W-THEMEKEY, ember.toml has a key Studio does not know: screen.glow. The key is ignored.",
        ]
    );
}

#[test]
fn a_missing_version_or_base_is_not_a_theme() {
    assert_eq!(
        read("base = \"#ffffff\"").unwrap_err().text,
        "%STUDIO-E-NOTTHEME, ember.toml is not a theme: no version. The file is not listed and is untouched."
    );
    let e = read("version = 1").unwrap_err();
    assert_eq!(e.level, Level::Error);
    assert_eq!(
        e.text,
        "%STUDIO-E-NOTTHEME, ember.toml is not a theme: no base colour. The file is not listed and is untouched."
    );
}

#[test]
fn a_strength_out_of_range_is_clamped() {
    let l = read(&format!("{MIN}[screen]\nbloom = 7\nnoise = -0.5\n")).unwrap();
    assert_eq!(l.spec.strengths.bloom, 1.0);
    assert_eq!(l.spec.strengths.noise, 0.0);
    assert_eq!(
        l.notes[0].text,
        "%STUDIO-W-THEMERANGE, ember.toml asks for bloom 7, the most is 1. Studio uses 1. The file is untouched."
    );
    assert_eq!(
        l.notes[1].text,
        "%STUDIO-W-THEMERANGE, ember.toml asks for noise -0.5, the least is 0. Studio uses 0. The file is untouched."
    );
}

#[test]
fn a_rate_takes_the_nearest() {
    let l = read(&format!("{MIN}[screen]\nrate = 144\n")).unwrap();
    assert_eq!(l.spec.rate, 60);
    assert_eq!(
        l.notes[0].text,
        "%STUDIO-W-THEMERANGE, ember.toml asks for rate 144, Studio has 15, 30 and 60. Studio uses 60."
    );
    assert_eq!(
        read(&format!("{MIN}[screen]\nrate = 20\n"))
            .unwrap()
            .spec
            .rate,
        15
    );
}

#[test]
fn a_wrong_kind_is_not_a_theme() {
    for (text, why) in [
        ("[screen]\nbloom = \"x\"", "bloom is not a number"),
        ("[screen]\nbloom = nan", "bloom is not a number"),
        ("[screen]\nvignette = inf", "vignette is not a number"),
        ("[screen]\nnoise = -inf", "noise is not a number"),
        ("[screen]\nmoves = 1", "moves is not true or false"),
        ("[screen]\nrate = 30.5", "rate is not a whole number"),
        ("name = 3", "name is not text"),
        ("screen = 3", "screen is not a table"),
    ] {
        let e = read(&format!("{MIN}{text}\n")).unwrap_err();
        assert_eq!(
            e.text,
            format!(
                "%STUDIO-E-NOTTHEME, ember.toml is not a theme: {why}. The file is not listed and is untouched."
            )
        );
    }
    for base in [
        "\"ff7a1a\"",
        "\"#ff7a1\"",
        "\"#gg7a1a\"",
        "3",
        "\"#ff7a1a0\"",
    ] {
        let e = read(&format!("version = 1\nbase = {base}\n")).unwrap_err();
        assert!(
            e.text
                .ends_with("base is not a colour. The file is not listed and is untouched."),
            "{base}: {}",
            e.text
        );
    }
}

#[test]
fn a_newer_version_is_not_listed() {
    assert_eq!(
        read("version = 2\nbase = \"#ffffff\"").unwrap_err().text,
        "%STUDIO-E-THEMENEW, ember.toml uses theme format 2, this Studio knows 1. The theme is not listed. Update Studio to use it."
    );
}

#[test]
fn not_toml_names_a_line_and_quotes_nothing() {
    let e = read_theme("notes.toml", b"version = 1\n\nsecret = \n").unwrap_err();
    assert_eq!(
        e.text,
        "%STUDIO-E-NOTTHEME, notes.toml is not a theme: line 3, a value was expected. The file is not listed and is untouched."
    );
    let e = read_theme("notes.toml", b"version = 1\nbase = \"\xff\"\n").unwrap_err();
    assert!(e.text.contains("line 2, not UTF-8"), "{}", e.text);
    let e = read("version = 1\nversion = 1\nhidden = 1\n").unwrap_err();
    assert!(e.text.contains("line 2, a key appears twice"), "{}", e.text);
    assert!(!e.text.contains("hidden") && !e.text.contains("`"));
}

#[test]
fn a_file_of_17_kib_is_not_read() {
    let mut text = MIN.to_owned();
    text.push_str(&"#".repeat(17 * 1024));
    let e = read(&text).unwrap_err();
    assert!(e.text.contains("is over 16 KiB"), "{}", e.text);
}

#[test]
fn a_name_is_cleaned() {
    for (raw, want) in [
        ("Ev\u{202e}il", "Evil"),
        ("Two\nlines", "Twolines"),
        ("a\u{200d}b", "ab"),
        ("  Dusk   at  sea ", "Dusk at sea"),
        ("Été 2", "Été 2"),
    ] {
        let l = read(&format!("{MIN}name = \"{}\"\n", esc(raw))).unwrap();
        assert_eq!(l.spec.name, want, "{raw:?}");
    }
    let long = "x".repeat(10_000);
    let l = read(&format!("{MIN}name = \"{long}\"\n")).unwrap();
    assert_eq!(l.spec.name.chars().count(), NAME_MAX);
    assert_eq!(
        l.notes[0].text,
        format!(
            "%STUDIO-W-THEMENAME, ember.toml: the name was cleaned to {}. The file is untouched.",
            "x".repeat(32)
        )
    );
    // A name of a theme that ships cannot pass for it.
    let l = read(&format!("{MIN}name = \"green\"\n")).unwrap();
    assert_eq!(l.spec.name, "green, ember.toml");
    assert!(!l.spec.is_shipped());
    // An empty name takes the file's name, then "Theme".
    let l = read_theme(
        "\u{202e}.toml",
        format!("{MIN}name = \"\u{1F600}\"\n").as_bytes(),
    )
    .unwrap();
    assert_eq!(l.spec.name, "Theme");
}

#[test]
fn a_file_name_is_shown_safely() {
    assert_eq!(shown("ev\u{202e}l\n.toml"), "ev?l?.toml");
    assert_eq!(shown(&"a".repeat(100)).len(), FILE_SHOWN_MAX);
}

#[test]
fn file_names_from_names() {
    let none = |_: &str| false;
    assert_eq!(file_name("Ember", none), "ember.toml");
    assert_eq!(file_name("CON", none), "theme-con.toml");
    assert_eq!(file_name("Été 2", none), "t-2.toml");
    assert_eq!(file_name("", none), "theme.toml");
    assert_eq!(file_name("ééé", none), "theme.toml");
    assert_eq!(file_name(&"a".repeat(50), none).len(), 32 + 5);
    let taken = |f: &str| ["ember.toml", "ember-2.toml"].contains(&f);
    assert_eq!(file_name("Ember", taken), "ember-3.toml");
}

#[test]
fn shipped_numbers_round_trip() {
    for spec in &SHIPPED {
        let l = read(&theme_text(spec)).unwrap();
        // A file cannot pass for a theme that ships.
        assert_eq!(l.spec.name, format!("{}, ember.toml", spec.name));
        assert_eq!(l.spec.base, spec.base);
        assert_eq!(l.spec.strengths, spec.strengths, "{}", spec.name);
        assert_eq!((l.spec.moves, l.spec.rate), (spec.moves, spec.rate));
        assert!(l.notes.is_empty());
    }
}

#[test]
fn write_then_read_a_folder() {
    let d = TempDir::new("write");
    let themes = d.0.join(THEMES);
    let mut s = SHIPPED[1].clone();
    s.name = Cow::Borrowed("Ember");
    write_theme(&themes, "ember.toml", &s).unwrap();
    let f = read_folder(&themes);
    assert_eq!(f.themes, vec![("ember.toml".to_owned(), s)]);
    // No temporary file is left behind.
    assert_eq!(std::fs::read_dir(&themes).unwrap().count(), 1);
}

#[test]
fn two_files_one_name() {
    let d = TempDir::new("twins");
    for f in ["dusk.toml", "dusk-2.toml"] {
        std::fs::write(d.0.join(f), format!("{MIN}name = \"Dusk\"\n")).unwrap();
    }
    std::fs::write(d.0.join("zed.toml"), format!("{MIN}name = \"dusk\"\n")).unwrap();
    let f = read_folder(&d.0);
    let names: Vec<&str> = f.themes.iter().map(|(_, t)| t.name.as_ref()).collect();
    assert_eq!(
        names,
        ["Dusk, dusk-2.toml", "Dusk, dusk.toml", "dusk, zed.toml"]
    );
    assert_eq!(
        f.notes[0].text,
        "%STUDIO-W-THEMETWICE, 3 theme files are named Dusk: dusk-2.toml, dusk.toml, zed.toml. Both are listed. Rename one."
    );
}

#[test]
fn sixty_five_files() {
    let d = TempDir::new("many");
    for i in 0..65 {
        std::fs::write(d.0.join(format!("t{i:02}.toml")), MIN).unwrap();
    }
    std::fs::write(d.0.join("readme.txt"), "x").unwrap();
    std::fs::create_dir(d.0.join("sub.toml")).unwrap();
    let f = read_folder(&d.0);
    assert_eq!(f.themes.len(), MAX_FILES);
    assert_eq!(f.themes[63].0, "t63.toml");
    assert_eq!(
        f.notes[0].text,
        "%STUDIO-W-THEMEMANY, the themes folder holds 65 files, Studio reads the first 64"
    );
}

#[test]
fn a_refused_file_is_kept_with_its_reason() {
    let d = TempDir::new("refused");
    std::fs::write(d.0.join("bad.toml"), "version = 1").unwrap();
    let f = read_folder(&d.0);
    assert!(f.themes.is_empty());
    assert_eq!(f.refused[0].0, "bad.toml");
    assert!(f.refused[0].1.note().text.contains("no base colour"));
}

#[cfg(unix)]
#[test]
fn a_link_is_not_read_and_not_written_through() {
    let d = TempDir::new("link");
    let outside = d.0.join("outside.txt");
    std::fs::write(&outside, "keep").unwrap();
    let themes = d.0.join(THEMES);
    std::fs::create_dir(&themes).unwrap();
    std::os::unix::fs::symlink(&outside, themes.join("ember.toml")).unwrap();
    assert!(read_folder(&themes).themes.is_empty());
    write_theme(&themes, "ember.toml", &SHIPPED[0]).unwrap();
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
    assert!(
        !std::fs::symlink_metadata(themes.join("ember.toml"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn settings_round_trip() {
    let d = TempDir::new("settings");
    let mut s = Settings::default();
    let mut spec = SHIPPED[0].clone();
    spec.strengths.bloom = 0.6;
    let u = Unsaved {
        from: "ember.toml".into(),
        name: Some("Ember".into()),
        spec: Spec {
            name: Cow::Owned(String::new()),
            ..spec
        },
    };
    s.set("ember.toml", true, true, Some(&u));
    s.write(&d.0).unwrap();
    let back = Settings::read(&d.0).unwrap();
    assert_eq!(back.theme(), Some("ember.toml"));
    assert!(back.high_legibility() && back.compact_header());
    assert_eq!(back.unsaved(), Some(u));
    let mut s = back;
    s.set("Green", false, false, None);
    assert!(s.unsaved().is_none());
}

#[test]
fn an_unknown_key_survives() {
    let d = TempDir::new("unknown");
    std::fs::write(
        d.0.join(SETTINGS),
        "version = 1\ntheme = \"Amber\"\ntext_size = 115\n[later]\npanes = [1, 2]\n",
    )
    .unwrap();
    let mut s = Settings::read(&d.0).unwrap();
    assert_eq!(s.theme(), Some("Amber"));
    s.set("Blue", false, false, None);
    s.write(&d.0).unwrap();
    let text = std::fs::read_to_string(d.0.join(SETTINGS)).unwrap();
    let t: Table = text.parse().unwrap();
    assert_eq!(t.get("text_size"), Some(&Value::Integer(115)));
    assert_eq!(
        t.get("later").and_then(|l| l.get("panes")),
        Some(&Value::Array(vec![Value::Integer(1), Value::Integer(2)]))
    );
    assert_eq!(t.get("theme"), Some(&Value::String("Blue".into())));
}

#[test]
fn a_broken_settings_file_is_a_note() {
    let d = TempDir::new("broken");
    std::fs::write(d.0.join(SETTINGS), "version = 1\ntheme = \n").unwrap();
    assert_eq!(
        Settings::read(&d.0).unwrap_err().text,
        "%STUDIO-W-NOSETTINGS, could not read settings.toml: line 2, a value was expected. Studio starts with its defaults and writes no settings in this run. Fix the file or delete it."
    );
    assert_eq!(Settings::read(&d.0.join("none")), Ok(Settings::default()));
}

#[test]
fn one_rule_for_versions() {
    let rows: &[(&str, Version)] = &[
        ("", Version::Missing),
        ("version = 1\n", Version::Known),
        ("version = 2\n", Version::Newer(2)),
        ("version = 0\n", Version::Bad),
        ("version = -1\n", Version::Bad),
        ("version = \"1\"\n", Version::Bad),
        ("version = 1.0\n", Version::Bad),
    ];
    for (text, want) in rows {
        let t: Table = text.parse().unwrap();
        assert_eq!(version(t.get("version")), *want, "{text}");
    }
}

#[test]
fn theme_file_versions() {
    let rows: &[(&str, Result<(), &str>)] = &[
        (
            "",
            Err("%STUDIO-E-NOTTHEME, t.toml is not a theme: no version."),
        ),
        ("version = 1\n", Ok(())),
        (
            "version = 2\n",
            Err("%STUDIO-E-THEMENEW, t.toml uses theme format 2, this Studio knows 1."),
        ),
        (
            "version = 0\n",
            Err("%STUDIO-E-NOTTHEME, t.toml is not a theme: version is not 1."),
        ),
        (
            "version = -1\n",
            Err("%STUDIO-E-NOTTHEME, t.toml is not a theme: version is not 1."),
        ),
        (
            "version = \"1\"\n",
            Err("%STUDIO-E-NOTTHEME, t.toml is not a theme: version is not 1."),
        ),
        (
            "version = 1.0\n",
            Err("%STUDIO-E-NOTTHEME, t.toml is not a theme: version is not 1."),
        ),
    ];
    for (v, want) in rows {
        let text = format!("{v}base = \"#ff7a1a\"\n");
        let got = read_theme("t.toml", text.as_bytes());
        match want {
            Ok(()) => assert!(got.is_ok(), "{v}"),
            Err(start) => {
                let line = got.unwrap_err().text;
                assert!(line.starts_with(start), "{v}: {line}");
            }
        }
    }
}

#[test]
fn settings_file_versions() {
    let rows: &[(&str, Result<(), &str>)] = &[
        ("", Ok(())),
        ("version = 1\n", Ok(())),
        (
            "version = 2\n",
            Err("it uses settings format 2, this Studio knows 1"),
        ),
        ("version = 0\n", Err("version is not 1")),
        ("version = -1\n", Err("version is not 1")),
        ("version = \"1\"\n", Err("version is not 1")),
        ("version = 1.0\n", Err("version is not 1")),
    ];
    for (v, want) in rows {
        let text = format!("{v}theme = \"Amber\"\n");
        let got = Settings::parse(text.as_bytes()).map(|_| ());
        assert_eq!(got, want.map_err(str::to_owned), "{v}");
    }
}

#[test]
fn one_verdict_per_entry() {
    use std::io::{Error, ErrorKind};
    let err = |k: ErrorKind| Error::new(k, "made up");
    let name = |s: &str| OsString::from(s);
    let long = format!("{}.toml", "a".repeat(FILE_MAX - 5));
    let listed = |f: &str| Entry::Listed(f.to_owned());
    let unreadable = |f: &str| {
        Entry::Refused(
            f.to_owned(),
            Refusal::Unreadable(file_unreadable(f, "made up")),
        )
    };
    type Seen = std::io::Result<(OsString, std::io::Result<bool>)>;
    let rows: Vec<(&str, Seen, Entry)> = vec![
        (
            "a file",
            Ok((name("ember.toml"), Ok(true))),
            listed("ember.toml"),
        ),
        (
            "a folder",
            Ok((name("ember.toml"), Ok(false))),
            Entry::Skipped,
        ),
        (
            "another name",
            Ok((name("notes.txt"), Ok(true))),
            Entry::Skipped,
        ),
        (
            "a hidden file",
            Ok((name(".x.toml"), Ok(true))),
            Entry::Skipped,
        ),
        (
            "a leftover",
            Ok((name(&temp_name(u32::MAX, u64::MAX)), Ok(true))),
            Entry::Skipped,
        ),
        (
            "gone at the lookup",
            Ok((name("ember.toml"), Err(err(ErrorKind::NotFound)))),
            Entry::Skipped,
        ),
        (
            "refused at the lookup",
            Ok((name("ember.toml"), Err(err(ErrorKind::PermissionDenied)))),
            unreadable("ember.toml"),
        ),
        (
            "another fault at the lookup",
            Ok((name("ember.toml"), Err(err(ErrorKind::Interrupted)))),
            unreadable("ember.toml"),
        ),
        (
            "a lookup fault on another name",
            Ok((name("notes.txt"), Err(err(ErrorKind::PermissionDenied)))),
            Entry::Skipped,
        ),
        (
            "the entry is not found",
            Err(err(ErrorKind::NotFound)),
            Entry::ListingUnreadable("made up".into()),
        ),
        (
            "the entry is refused",
            Err(err(ErrorKind::PermissionDenied)),
            Entry::ListingUnreadable("made up".into()),
        ),
        (
            "the entry fails another way",
            Err(err(ErrorKind::Other)),
            Entry::ListingUnreadable("made up".into()),
        ),
        (
            "a name at the limit",
            Ok((name(&long), Ok(true))),
            listed(&long),
        ),
        (
            "a name over the limit",
            Ok((name(&format!("a{long}")), Ok(true))),
            Entry::Refused(
                format!("a{long}"),
                Refusal::NotTheme(not_theme(
                    &format!("a{long}"),
                    &format!("its name is over {FILE_MAX} bytes"),
                )),
            ),
        ),
    ];
    for (row, input, want) in rows {
        assert_eq!(entry(input), want, "{row}");
    }
}

#[test]
fn a_temporary_name_is_short_and_never_a_theme() {
    for (pid, n) in [(0, 0), (1, 2), (u32::MAX, u64::MAX)] {
        let t = temp_name(pid, n);
        assert!(!t.ends_with(".toml"), "{t}");
        assert!(t.starts_with('.') && t.len() <= 64, "{t}");
    }
    // Studio's longest own name, with a clash number, fits the limit.
    let longest = format!("{}-{}.toml", "a".repeat(NAME_MAX), u32::MAX);
    assert!(longest.len() <= FILE_MAX, "{longest}");
}

#[test]
fn a_leftover_is_not_listed_and_does_not_count() {
    let d = TempDir::new("leftover");
    for i in 0..MAX_FILES {
        std::fs::write(d.0.join(format!("t{i:02}.toml")), "x").unwrap();
    }
    std::fs::write(d.0.join(temp_name(7, 9)), "version = 1\n").unwrap();
    let l = toml_files(&d.0);
    assert_eq!(l.files().len(), MAX_FILES);
    assert!(l.files().iter().all(|f| !f.starts_with('.')));
    assert!(read_folder(&d.0).notes.is_empty());
}

#[test]
fn a_long_destination_name_writes() {
    let d = TempDir::new("longname");
    for len in [1usize, 40, 200, 250, 255] {
        let file = "b".repeat(len);
        // Some systems or temp folders take shorter names: skip the row.
        if std::fs::write(d.0.join(&file), "").is_err() {
            eprintln!("skipped: a name of {len} bytes cannot be made here");
            continue;
        }
        write_file(&d.0, &file, b"x").unwrap_or_else(|e| panic!("{len}: {e}"));
        assert_eq!(std::fs::read(d.0.join(&file)).unwrap(), b"x", "{len}");
    }
}

/// What a number read from a file gives: a value with or without a
/// warning, or a refusal with its reason.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Got {
    /// Kept as it is, with no warning.
    Kept(f64),
    /// Brought to this value, with a warning.
    Moved(f64),
    /// Refused, with this reason.
    Refused(&'static str),
}

/// The values every numeric key is tried with, as TOML text.
const EXTREMES: [&str; 11] = [
    "-9223372036854775808",
    "9223372036854775807",
    "-1",
    "0",
    "0.5",
    "1",
    "nan",
    "inf",
    "-inf",
    "1e308",
    "-0.0",
];

#[test]
fn every_number_from_a_file_is_checked_before_it_is_used() {
    use Got::*;
    let not_number = Refused("is not a number");
    // A strength: 0 to 1, floats or integers.
    let strength: &[(&str, Got)] = &[
        (EXTREMES[0], Moved(0.0)),
        (EXTREMES[1], Moved(1.0)),
        (EXTREMES[2], Moved(0.0)),
        (EXTREMES[3], Kept(0.0)),
        (EXTREMES[4], Kept(0.5)),
        (EXTREMES[5], Kept(1.0)),
        (EXTREMES[6], not_number),
        (EXTREMES[7], not_number),
        (EXTREMES[8], not_number),
        (EXTREMES[9], Moved(1.0)),
        (EXTREMES[10], Kept(0.0)),
        ("-0.01", Moved(0.0)),
        ("1.01", Moved(1.0)),
        ("\"0.5\"", not_number),
    ];
    for key in STRENGTH_KEYS {
        for (v, want) in strength {
            let text = format!("version = 1\nbase = \"#ff7a1a\"\n[screen]\n{key} = {v}\n");
            let got = match read_theme("t.toml", text.as_bytes()) {
                Ok(l) => {
                    let i = STRENGTH_KEYS.iter().position(|k| *k == key).unwrap();
                    let s = f64::from(l.spec.strengths.get(i).unwrap());
                    // -0.0 equals 0.0, so the sign is checked on its own.
                    assert!(s.is_sign_positive(), "{key} = {v}: {s}");
                    if l.notes.is_empty() {
                        Kept(s)
                    } else {
                        Moved(s)
                    }
                }
                Err(n) => {
                    assert!(
                        n.text.contains(&format!("{key} is not a number")),
                        "{key} = {v}: {}",
                        n.text
                    );
                    not_number
                }
            };
            assert_eq!(got, *want, "{key} = {v}");
        }
    }

    // `rate`: a whole number, the nearest of 15, 30 and 60.
    let whole = Refused("rate is not a whole number");
    let rate: &[(&str, Got)] = &[
        (EXTREMES[0], Moved(15.0)),
        (EXTREMES[1], Moved(60.0)),
        (EXTREMES[2], Moved(15.0)),
        (EXTREMES[3], Moved(15.0)),
        (EXTREMES[4], whole),
        (EXTREMES[5], Moved(15.0)),
        (EXTREMES[6], whole),
        (EXTREMES[7], whole),
        (EXTREMES[8], whole),
        (EXTREMES[9], whole),
        (EXTREMES[10], whole),
        ("30.0", whole),
        ("14", Moved(15.0)),
        ("15", Kept(15.0)),
        ("16", Moved(15.0)),
        ("29", Moved(30.0)),
        ("31", Moved(30.0)),
        ("59", Moved(60.0)),
        ("60", Kept(60.0)),
        ("61", Moved(60.0)),
        ("-9223372036854775807", Moved(15.0)),
        ("9223372036854775806", Moved(60.0)),
    ];
    for (v, want) in rate {
        let text = format!("version = 1\nbase = \"#ff7a1a\"\n[screen]\nrate = {v}\n");
        let got = match read_theme("t.toml", text.as_bytes()) {
            Ok(l) => {
                let r = f64::from(l.spec.rate);
                match l.notes.first() {
                    None => Kept(r),
                    Some(n) => {
                        assert_eq!(
                            n.text,
                            format!(
                                "%STUDIO-W-THEMERANGE, t.toml asks for rate {v}, Studio has 15, 30 and 60. Studio uses {r}."
                            )
                        );
                        Moved(r)
                    }
                }
            }
            Err(n) => {
                assert!(
                    n.text.contains("rate is not a whole number"),
                    "{v}: {}",
                    n.text
                );
                whole
            }
        };
        assert_eq!(got, *want, "rate = {v}");
        // `[unsaved]` of `settings.toml` reads the same keys by the same
        // rule. A refused value there means no changed theme.
        let s = format!("[unsaved]\nfrom = \"Green\"\nbase = \"#ff7a1a\"\nrate = {v}\n");
        let u = Settings::parse(s.as_bytes()).unwrap().unsaved();
        match want {
            Kept(r) | Moved(r) => assert_eq!(f64::from(u.unwrap().spec.rate), *r, "{v}"),
            Refused(_) => assert_eq!(u, None, "{v}"),
        }
    }

    // `version`: 1 only. A newer whole number says so, the rest is not 1.
    let versions: &[(&str, Result<(), &str>)] = &[
        (EXTREMES[0], Err("version is not 1")),
        (
            EXTREMES[1],
            Err("uses theme format 9223372036854775807, this Studio knows 1"),
        ),
        (EXTREMES[2], Err("version is not 1")),
        (EXTREMES[3], Err("version is not 1")),
        (EXTREMES[4], Err("version is not 1")),
        (EXTREMES[5], Ok(())),
        (EXTREMES[6], Err("version is not 1")),
        (EXTREMES[7], Err("version is not 1")),
        (EXTREMES[8], Err("version is not 1")),
        (EXTREMES[9], Err("version is not 1")),
        (EXTREMES[10], Err("version is not 1")),
        ("1.0", Err("version is not 1")),
        ("2", Err("uses theme format 2, this Studio knows 1")),
    ];
    for (v, want) in versions {
        let text = format!("version = {v}\nbase = \"#ff7a1a\"\n");
        let got = read_theme("t.toml", text.as_bytes()).map(|_| ());
        match want {
            Ok(()) => assert!(got.is_ok(), "version = {v}"),
            Err(why) => {
                let t = got.unwrap_err().text;
                assert!(t.contains(why), "version = {v}: {t}");
            }
        }
        let s = Settings::parse(format!("version = {v}\n").as_bytes()).map(|_| ());
        match want {
            Ok(()) => assert_eq!(s, Ok(()), "settings version = {v}"),
            Err(why) if why.starts_with("uses") => {
                let s = s.unwrap_err();
                assert!(s.starts_with("it uses settings format"), "{v}: {s}");
            }
            Err(_) => assert_eq!(s, Err("version is not 1".to_owned()), "{v}"),
        }
    }

    // `base`: `#` and six hex digits. A number is not a colour.
    for v in EXTREMES {
        let text = format!("version = 1\nbase = {v}\n");
        let t = read_theme("t.toml", text.as_bytes()).unwrap_err().text;
        assert!(t.contains("base is not a colour"), "base = {v}: {t}");
        let s = format!("[unsaved]\nfrom = \"Green\"\nbase = {v}\n");
        assert_eq!(
            Settings::parse(s.as_bytes()).unwrap().unsaved(),
            None,
            "{v}"
        );
    }
}
