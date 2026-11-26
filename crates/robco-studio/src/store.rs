// SPDX-License-Identifier: GPL-3.0-or-later

//! The files Studio keeps: a person's own themes and `settings.toml`, in
//! Studio's settings folder.
//!
//! A theme file is untrusted text. Each read checks the size first, then
//! parses with the `toml` reader that reads `robco.toml`, then checks every
//! key for kind, for being finite and for range. A message about a file
//! names a line and a kind of fault, and never quotes the file's text.
//! Every write goes to a new temporary file in the same folder, then a
//! rename, so a file is whole or absent. A rename replaces a link and
//! never writes through it.

use crate::kit::Level;
use crate::theme::{Spec, Strengths, crt};
use eframe::egui::Color32;
use std::borrow::Cow;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use toml::{Table, Value};

/// The most bytes Studio reads of a theme file.
pub const MAX_BYTES: u64 = 16384;
/// The most theme files Studio reads from the folder.
pub const MAX_FILES: usize = 64;
/// The most characters of a theme's name.
pub const NAME_MAX: usize = 32;
/// The most bytes of a theme file's name Studio lists. Studio's own names
/// hold at most 32 characters, `-` and a number, and `.toml`, so a name
/// over this was not made by Studio, and Studio refuses it rather than
/// list a file it may fail to write again.
pub const FILE_MAX: usize = 64;
/// The most characters of a file's name in a message.
pub const FILE_SHOWN_MAX: usize = 40;
/// The theme format this Studio knows.
pub const FORMAT: i64 = 1;
/// The most bytes Studio reads of `settings.toml`.
pub const SETTINGS_MAX: u64 = 1 << 20;
/// The variable that names the settings folder itself.
pub const ENV: &str = "ROBCO_STUDIO_SETTINGS";
/// The settings file, in the settings folder.
pub const SETTINGS: &str = "settings.toml";
/// The themes folder, in the settings folder.
pub const THEMES: &str = "themes";

/// The keys of `[screen]`, the ten strengths in the order of
/// [`Strengths::get`].
pub const STRENGTH_KEYS: [&str; 10] = [
    "bloom",
    "vignette",
    "scanlines",
    "curvature",
    "persistence",
    "noise",
    "flicker",
    "band",
    "mask",
    "aberration",
];

/// The settings folder, from the environment that `var` reads. The error
/// says which variable is missing.
///
/// `ROBCO_STUDIO_SETTINGS`, when set and not empty, is the folder itself
/// and must be absolute. Otherwise the system's settings folder plus
/// `robco/studio`, as the `robco` tool finds its cache folder.
pub fn folder(var: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf, String> {
    let set = |name: &str| var(name).filter(|v| !v.is_empty());
    if let Some(v) = set(ENV) {
        let p = PathBuf::from(&v);
        return if p.is_absolute() {
            Ok(p)
        } else {
            Err(format!("{ENV} is not an absolute path"))
        };
    }
    let absolute = |name: &str| set(name).map(PathBuf::from).filter(|p| p.is_absolute());
    let base = if cfg!(windows) {
        absolute("APPDATA").ok_or("APPDATA is not an absolute path")?
    } else if cfg!(target_os = "macos") {
        absolute("HOME")
            .ok_or("HOME is not an absolute path")?
            .join("Library")
            .join("Application Support")
    } else {
        match absolute("XDG_CONFIG_HOME") {
            Some(p) => p,
            None => absolute("HOME")
                .ok_or("HOME is not an absolute path")?
                .join(".config"),
        }
    };
    Ok(base.join("robco").join("studio"))
}

/// A line for the status line, with its level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// How bad it is.
    pub level: Level,
    /// The whole line, with what the window adds.
    pub text: String,
}

fn warn(text: String) -> Note {
    Note {
        level: Level::Warning,
        text,
    }
}

fn error(text: String) -> Note {
    Note {
        level: Level::Error,
        text,
    }
}

/// A character a theme's name may hold: a letter or a digit of any
/// script, the space, and `-`, `_`, `.` and `'`.
pub fn allowed(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '\'')
}

/// A theme's name folded for comparison: Unicode lower case, as
/// `str::to_lowercase` gives it. Studio applies no normal form, so a
/// letter with a combining accent and its composed letter stay apart. The
/// cleaned names never hold a combining mark on its own, so that case
/// cannot reach a comparison of two listed names.
pub fn fold(name: &str) -> String {
    name.to_lowercase()
}

/// Two theme names are one name without regard to case. Every check of
/// a theme's name uses this: taken names, the names that ship,
/// `--theme`, twins in the folder, rename and replace.
pub fn same_name(a: &str, b: &str) -> bool {
    fold(a) == fold(b)
}

/// What a file's `version` says, by the one rule for theme files and
/// `settings.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// No `version` key.
    Missing,
    /// The format this Studio knows.
    Known,
    /// A newer whole number.
    Newer(i64),
    /// Anything else: 0, a negative number, a string, a float, a table.
    Bad,
}

/// The `version` value of a file, by the one rule.
pub fn version(v: Option<&Value>) -> Version {
    match v {
        None => Version::Missing,
        Some(Value::Integer(n)) if *n == FORMAT => Version::Known,
        Some(Value::Integer(n)) if *n > FORMAT => Version::Newer(*n),
        Some(_) => Version::Bad,
    }
}

/// The text with every character outside the set gone, spaces at the ends
/// gone, runs of spaces made one, cut at [`NAME_MAX`] characters. Empty
/// when no letter or digit is left.
pub fn clean(raw: &str) -> String {
    let kept: String = raw.chars().filter(|c| allowed(*c)).collect();
    let one: Vec<&str> = kept.split(' ').filter(|w| !w.is_empty()).collect();
    let cut: String = one.join(" ").chars().take(NAME_MAX).collect();
    let cut = cut.trim_end().to_owned();
    if cut.chars().any(char::is_alphanumeric) {
        cut
    } else {
        String::new()
    }
}

/// A file's name as a message shows it: every character outside the set
/// as `?`, cut at [`FILE_SHOWN_MAX`] characters.
pub fn shown(file: &str) -> String {
    file.chars()
        .map(|c| if allowed(c) { c } else { '?' })
        .take(FILE_SHOWN_MAX)
        .collect()
}

/// The name a theme file gives, cleaned: the `name` key, else the file's
/// name less `.toml`, else "Theme". A name of a theme that ships gets the
/// file's name after it. The flag says the `name` key had to be cleaned.
pub fn theme_name(raw: Option<&str>, file: &str) -> (String, bool) {
    // No `name` key is a fact of the file: the file's name stands in.
    let mut name = raw.map(clean).unwrap_or_default();
    let cleaned = raw.is_some_and(|r| !r.is_empty() && name != r);
    if name.is_empty() {
        name = clean(file.strip_suffix(".toml").unwrap_or(file));
    }
    if name.is_empty() {
        name = "Theme".to_owned();
    }
    if crate::theme::shipped(&name).is_some() {
        name = format!("{name}, {}", shown(file));
    }
    (name, cleaned)
}

/// The names Windows keeps for devices, which no file may take.
const RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// The file name for a new theme named `name`: the ASCII letters and
/// digits in lower case, every other run as one `-`, at most 32
/// characters, then `.toml`. Dashes at the ends go. An empty result is
/// `theme`, a reserved name gets `theme-` in front, and a name that
/// `taken` says exists, in any case, gets `-2`, `-3` and so on.
pub fn file_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut stem = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            stem.push(c.to_ascii_lowercase());
        } else if !stem.ends_with('-') {
            stem.push('-');
        }
    }
    let mut stem: String = stem.trim_matches('-').chars().take(NAME_MAX).collect();
    stem = stem.trim_end_matches('-').to_owned();
    if stem.is_empty() {
        stem = "theme".to_owned();
    }
    if RESERVED.contains(&stem.as_str()) {
        stem = format!("theme-{stem}");
    }
    let first = format!("{stem}.toml");
    if !taken(&first) {
        return first;
    }
    let mut n = 2u32;
    loop {
        let f = format!("{stem}-{n}.toml");
        if !taken(&f) {
            return f;
        }
        n += 1;
    }
}

/// A theme read from a file, and the warnings its file gave.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The theme.
    pub spec: Spec,
    /// The warnings: a key Studio does not know, a number out of range, a
    /// cleaned name.
    pub notes: Vec<Note>,
}

/// The line for a file that is not a theme, for the reason `why`.
pub fn not_theme(file: &str, why: &str) -> Note {
    error(format!(
        "%STUDIO-E-NOTTHEME, {} is not a theme: {why}. The file is not listed and is untouched.",
        shown(file)
    ))
}

/// The line of byte `at` in `bytes`, from 1.
fn line_of(bytes: &[u8], at: usize) -> usize {
    1 + bytes.iter().take(at).filter(|b| **b == b'\n').count()
}

/// The kind of a TOML fault, in words that quote nothing of the file.
fn fault_kind(message: &str) -> &'static str {
    let m = message.to_ascii_lowercase();
    if m.contains("duplicate") {
        "a key appears twice"
    } else if m.contains("expected") && m.contains("value") {
        "a value was expected"
    } else if m.contains("string") {
        "a string is not closed or not valid"
    } else {
        "the TOML is not valid"
    }
}

/// Parse TOML text with the size checked already. The error is the line
/// and the kind of fault.
fn parse(bytes: &[u8]) -> Result<Table, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| format!("line {}, not UTF-8", line_of(bytes, e.valid_up_to())))?;
    text.parse::<Table>().map_err(|e| {
        let line = e.span().map_or(1, |s| line_of(bytes, s.start));
        format!("line {line}, {}", fault_kind(e.message()))
    })
}

/// A number as a message shows it: a whole number with no point.
fn number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else if v.abs() >= 1e15 {
        // A huge number reads as a short exponent, not 300 digits.
        format!("{v:e}")
    } else {
        format!("{v}")
    }
}

/// `#` and six hex digits, in either case.
pub fn colour(text: &str) -> Option<Color32> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok().map(crate::palette::hex)
}

/// The screen keys of a theme: ten strengths, `moves` and `rate`, from
/// `t`, with the warnings for `file`. A wrong kind is the error.
fn screen_of(
    t: Option<&Table>,
    file: &str,
    notes: &mut Vec<Note>,
) -> Result<(Strengths, bool, u32), String> {
    let mut s = Strengths::NONE;
    let (mut moves, mut rate) = (false, crt::RATE);
    let Some(t) = t else {
        return Ok((s, moves, rate));
    };
    for (i, key) in STRENGTH_KEYS.iter().enumerate() {
        let v = match t.get(*key) {
            None => continue,
            Some(Value::Integer(n)) => *n as f64,
            Some(Value::Float(f)) if f.is_finite() => *f,
            Some(_) => return Err(format!("{key} is not a number")),
        };
        // Adding 0.0 turns -0.0 into 0.0, so a file cannot keep a sign.
        let kept = v.clamp(0.0, 1.0) + 0.0;
        if kept != v {
            let (edge, word) = if v < 0.0 { (0, "least") } else { (1, "most") };
            notes.push(warn(format!(
                "%STUDIO-W-THEMERANGE, {} asks for {key} {}, the {word} is {edge}. Studio uses {edge}. The file is untouched.",
                shown(file),
                number(v)
            )));
        }
        if let Some(slot) = s.get_mut(i) {
            *slot = kept as f32;
        }
    }
    match t.get("moves") {
        None => {}
        Some(Value::Boolean(b)) => moves = *b,
        Some(_) => return Err("moves is not true or false".into()),
    }
    match t.get("rate") {
        None => {}
        Some(Value::Integer(n)) => {
            let near = crt::RATES
                .iter()
                .copied()
                // `abs_diff` cannot overflow, whatever the file holds.
                .min_by_key(|r| n.abs_diff(i64::from(*r)))
                .unwrap_or(crt::RATE);
            if i64::from(near) != *n {
                notes.push(warn(format!(
                    "%STUDIO-W-THEMERANGE, {} asks for rate {n}, Studio has 15, 30 and 60. Studio uses {near}.",
                    shown(file)
                )));
            }
            rate = near;
        }
        Some(_) => return Err("rate is not a whole number".into()),
    }
    Ok((s, moves, rate))
}

fn unknown(file: &str, key: &str) -> Note {
    warn(format!(
        "%STUDIO-W-THEMEKEY, {} has a key Studio does not know: {}. The key is ignored.",
        shown(file),
        shown(key)
    ))
}

/// Read one theme file's bytes. `file` is the file's own name, used when
/// the file has no `name` and in messages. The error is the one line that
/// says why the file is not a theme.
pub fn read_theme(file: &str, bytes: &[u8]) -> Result<Loaded, Note> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(not_theme(file, "it is over 16 KiB"));
    }
    let root = parse(bytes).map_err(|why| not_theme(file, &why))?;
    match version(root.get("version")) {
        Version::Missing => return Err(not_theme(file, "no version")),
        Version::Newer(n) => {
            return Err(error(format!(
                "%STUDIO-E-THEMENEW, {} uses theme format {n}, this Studio knows {FORMAT}. The theme is not listed. Update Studio to use it.",
                shown(file)
            )));
        }
        Version::Known => {}
        Version::Bad => return Err(not_theme(file, "version is not 1")),
    }
    let base = match root.get("base") {
        None => return Err(not_theme(file, "no base colour")),
        Some(Value::String(s)) => {
            colour(s).ok_or_else(|| not_theme(file, "base is not a colour"))?
        }
        Some(_) => return Err(not_theme(file, "base is not a colour")),
    };
    let raw = match root.get("name") {
        None => None,
        Some(Value::String(s)) => Some(s.as_str()),
        Some(_) => return Err(not_theme(file, "name is not text")),
    };
    let screen = match root.get("screen") {
        None => None,
        Some(Value::Table(t)) => Some(t),
        Some(_) => return Err(not_theme(file, "screen is not a table")),
    };
    let mut notes = Vec::new();
    for key in root.keys() {
        if !["version", "name", "base", "screen"].contains(&key.as_str()) {
            notes.push(unknown(file, key));
        }
    }
    if let Some(t) = screen {
        for key in t.keys() {
            if !STRENGTH_KEYS.contains(&key.as_str()) && key != "moves" && key != "rate" {
                notes.push(unknown(file, &format!("screen.{key}")));
            }
        }
    }
    let (strengths, moves, rate) =
        screen_of(screen, file, &mut notes).map_err(|why| not_theme(file, &why))?;
    let (name, cleaned) = theme_name(raw, file);
    if cleaned {
        notes.push(warn(format!(
            "%STUDIO-W-THEMENAME, {}: the name was cleaned to {name}. The file is untouched.",
            shown(file)
        )));
    }
    Ok(Loaded {
        spec: Spec {
            name: Cow::Owned(name),
            base,
            strengths,
            moves,
            rate,
        },
        notes,
    })
}

/// Read at most `max` bytes of the plain file at `path`, and one more so
/// that a longer file shows. A link, a folder or a device is refused.
pub fn read_plain(path: &Path, max: u64) -> std::io::Result<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() {
        return Err(std::io::Error::other("not a plain file"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(max + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// How Studio read a folder. A folder that is missing is a fact Studio
/// may act on. A folder it could not read tells nothing about what it
/// holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Listing {
    /// The folder is not there.
    Missing,
    /// The folder is there and Studio could not read it: the system's
    /// reason.
    Unreadable(String),
    /// The plain `.toml` files of the folder, in file name order, and the
    /// `.toml` entries refused at the listing, with the reason.
    Files(Vec<String>, Vec<(String, Refusal)>),
}

impl Listing {
    /// The files, none when the folder is missing or could not be read.
    pub fn files(&self) -> &[String] {
        match self {
            Listing::Files(f, _) => f,
            _ => &[],
        }
    }
}

/// What one entry of the themes folder is, from what the system said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A plain `.toml` file to read.
    Listed(String),
    /// Not a theme file: another kind, another name, or gone.
    Skipped,
    /// A `.toml` entry Studio does not list, with the reason.
    Refused(String, Refusal),
    /// The entry's name is not known, so the listing tells nothing.
    ListingUnreadable(String),
}

/// The verdict on one entry of a listing. `entry` is the entry, or the
/// error in its place; for an entry, its name and whether it is a plain
/// file. A failed lookup is never "not a file": it makes the entry
/// unreadable. Only "not found" is a fact, the entry is gone.
pub fn entry(entry: std::io::Result<(OsString, std::io::Result<bool>)>) -> Entry {
    let (name, plain) = match entry {
        Ok(e) => e,
        Err(e) => return Entry::ListingUnreadable(e.to_string()),
    };
    let lossy = name.to_string_lossy().into_owned();
    if !lossy.ends_with(".toml") || lossy.starts_with('.') {
        return Entry::Skipped;
    }
    match plain {
        Ok(true) => {}
        Ok(false) => return Entry::Skipped,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Entry::Skipped,
        Err(e) => {
            let n = file_unreadable(&lossy, &e.to_string());
            return Entry::Refused(lossy, Refusal::Unreadable(n));
        }
    }
    let Ok(file) = name.into_string() else {
        let n = not_theme(&lossy, "its name is not UTF-8");
        return Entry::Refused(lossy, Refusal::NotTheme(n));
    };
    if file.len() > FILE_MAX {
        let n = not_theme(&file, &format!("its name is over {FILE_MAX} bytes"));
        return Entry::Refused(file, Refusal::NotTheme(n));
    }
    Entry::Listed(file)
}

/// Why a file of the themes folder is not listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Studio read it and it is not a theme.
    NotTheme(Note),
    /// Studio could not read it, so it knows nothing about it.
    Unreadable(Note),
}

impl Refusal {
    /// The line for the status line.
    pub fn note(&self) -> &Note {
        match self {
            Refusal::NotTheme(n) | Refusal::Unreadable(n) => n,
        }
    }
}

/// The themes folder as Studio read it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Folder {
    /// The folder could not be read: the line that says why. The other
    /// fields are empty then.
    pub unreadable: Option<Note>,
    /// Each theme with its file's name, in file name order.
    pub themes: Vec<(String, Spec)>,
    /// Each file that is not listed, with its file's name and the reason.
    pub refused: Vec<(String, Refusal)>,
    /// The warnings of the folder and of its themes.
    pub notes: Vec<Note>,
}

/// The plain `.toml` files of `dir`, in file name order, or why there are
/// none.
pub fn toml_files(dir: &Path) -> Listing {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Listing::Missing,
        Err(e) => return Listing::Unreadable(e.to_string()),
    };
    let (mut files, mut refused) = (Vec::new(), Vec::new());
    for e in entries {
        // A folder that fails while it is listed is a folder Studio could
        // not read, not a shorter list.
        let e = e.map(|e| (e.file_name(), e.file_type().map(|t| t.is_file())));
        match entry(e) {
            Entry::Listed(f) => files.push(f),
            Entry::Skipped => {}
            Entry::Refused(f, r) => refused.push((f, r)),
            Entry::ListingUnreadable(why) => return Listing::Unreadable(why),
        }
    }
    files.sort();
    refused.sort_by(|a, b| a.0.cmp(&b.0));
    Listing::Files(files, refused)
}

/// The line for a themes folder Studio could not read.
pub fn folder_unreadable(why: &str) -> Note {
    warn(format!(
        "%STUDIO-W-THEMESREAD, could not read the themes folder: {why}. Your own themes are not listed and settings.toml keeps your choice."
    ))
}

/// The line for a theme file Studio could not read.
pub fn file_unreadable(file: &str, why: &str) -> Note {
    warn(format!(
        "%STUDIO-W-THEMEREAD, could not read {}: {why}. The file is not listed and is untouched.",
        shown(file)
    ))
}

/// Read the themes folder: plain `.toml` files, in file name order, at
/// most [`MAX_FILES`]. Two files with one name, in any case, are both
/// listed, each with its file's name after its name. A file that goes
/// between the listing and its read is gone and not listed.
pub fn read_folder(dir: &Path) -> Folder {
    let mut out = Folder::default();
    let files = match toml_files(dir) {
        Listing::Missing => return out,
        Listing::Unreadable(why) => {
            out.unreadable = Some(folder_unreadable(&why));
            return out;
        }
        Listing::Files(f, refused) => {
            out.refused = refused;
            f
        }
    };
    if files.len() > MAX_FILES {
        out.notes.push(warn(format!(
            "%STUDIO-W-THEMEMANY, the themes folder holds {} files, Studio reads the first {MAX_FILES}",
            files.len()
        )));
    }
    for file in files.into_iter().take(MAX_FILES) {
        let bytes = match read_plain(&dir.join(&file), MAX_BYTES) {
            Ok(b) => b,
            // Gone between the listing and the read: a fact.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) if e.kind() == std::io::ErrorKind::Other => {
                // A link, a folder or a device: Studio read what it is.
                let n = not_theme(&file, &e.to_string());
                out.refused.push((file, Refusal::NotTheme(n)));
                continue;
            }
            Err(e) => {
                let n = file_unreadable(&file, &e.to_string());
                out.refused.push((file, Refusal::Unreadable(n)));
                continue;
            }
        };
        match read_theme(&file, &bytes) {
            Ok(l) => {
                out.notes.extend(l.notes);
                out.themes.push((file, l.spec));
            }
            Err(n) => out.refused.push((file, Refusal::NotTheme(n))),
        }
    }
    let names: Vec<String> = out.themes.iter().map(|(_, t)| fold(&t.name)).collect();
    let mut done: Vec<&str> = Vec::new();
    for (i, n) in names.iter().enumerate() {
        if done.contains(&n.as_str()) {
            continue;
        }
        let twins: Vec<usize> = (i..names.len()).filter(|k| names[*k] == *n).collect();
        if twins.len() < 2 {
            continue;
        }
        done.push(n);
        let name = out.themes[i].1.name.to_string();
        let files: Vec<String> = twins.iter().map(|k| shown(&out.themes[*k].0)).collect();
        let count = if twins.len() == 2 {
            "two".to_owned()
        } else {
            twins.len().to_string()
        };
        out.notes.push(warn(format!(
            "%STUDIO-W-THEMETWICE, {count} theme files are named {name}: {}. Both are listed. Rename one.",
            files.join(", ")
        )));
        for k in twins {
            let (file, spec) = &mut out.themes[k];
            spec.name = Cow::Owned(format!("{}, {}", spec.name, shown(file)));
        }
    }
    out
}

/// The text of a theme file for `spec`, with every key. The name is
/// cleaned, so it holds no quote and no backslash.
pub fn theme_text(spec: &Spec) -> String {
    let mut t = format!(
        "# A RobCo Studio theme.\nversion = {FORMAT}\nname = \"{}\"\nbase = \"{}\"\n\n[screen]\n",
        clean(&spec.name),
        hex(spec.base)
    );
    t.push_str(&screen_text(spec));
    t
}

/// The ten strengths, `moves` and `rate`, one key a line.
fn screen_text(spec: &Spec) -> String {
    let mut t = String::new();
    for (i, key) in STRENGTH_KEYS.iter().enumerate() {
        let v = spec.strengths.get(i).unwrap_or(0.0);
        t.push_str(&format!("{key} = {v:.2}\n"));
    }
    t.push_str(&format!("moves = {}\nrate = {}\n", spec.moves, spec.rate));
    t
}

/// A colour as `#` and six hex digits in lower case.
pub fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

pub use crate::safe_write::temp_name;

/// Write `bytes` as `dir/file`, a person's settings. The folder is made
/// when it is missing, private on Unix. A new file is private on Unix. See
/// [`crate::safe_write`] for the rest.
pub fn write_file(dir: &Path, file: &str, bytes: &[u8]) -> std::io::Result<()> {
    crate::safe_write::make_private_dir(dir)?;
    crate::safe_write::write(&dir.join(file), bytes, crate::safe_write::New::Private)
}

/// Write `spec` as the theme file `file` of `dir`. The error is the line
/// for the status line.
pub fn write_theme(dir: &Path, file: &str, spec: &Spec) -> Result<(), Note> {
    write_file(dir, file, theme_text(spec).as_bytes()).map_err(|e| {
        error(format!(
            "%STUDIO-E-THEMEWRITE, could not write {}: {e}. The theme is not saved.",
            shown(file)
        ))
    })
}

/// The line after a theme is saved.
pub fn saved_news(name: &str, file: &str) -> String {
    format!(
        "%STUDIO-S-THEMESAVED, theme {name} saved as {}",
        shown(file)
    )
}

/// A changed theme that is not saved, as `settings.toml` keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct Unsaved {
    /// The theme it was changed from: a theme that ships, by name, or a
    /// file of the themes folder, as "ember.toml".
    pub from: String,
    /// The name it was shown with. Studio uses it only when `from` cannot
    /// be had; otherwise the name is the name of `from`.
    pub name: Option<String>,
    /// Its numbers. The name in it is empty.
    pub spec: Spec,
}

/// `settings.toml`: the whole table as read, so a key this Studio does
/// not know is written back as it was.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Settings {
    table: Table,
}

impl Settings {
    /// Read `settings.toml` of `dir`. A missing file is the defaults. The
    /// error is the line for the status line: Studio then starts with its
    /// defaults and writes no settings in the run.
    pub fn read(dir: &Path) -> Result<Settings, Note> {
        let path = dir.join(SETTINGS);
        let bytes = match read_plain(&path, SETTINGS_MAX) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(no_settings(&e.to_string())),
        };
        if bytes.len() as u64 > SETTINGS_MAX {
            return Err(no_settings("it is over 1 MiB"));
        }
        Self::parse(&bytes).map_err(|why| no_settings(&why))
    }

    /// The settings in `bytes`. The error is the line and kind of fault,
    /// or the version this Studio cannot read. A missing version is 1,
    /// since every key of the file may be missing.
    pub fn parse(bytes: &[u8]) -> Result<Settings, String> {
        let table = parse(bytes)?;
        match version(table.get("version")) {
            Version::Missing | Version::Known => Ok(Settings { table }),
            Version::Newer(n) => Err(format!(
                "it uses settings format {n}, this Studio knows {FORMAT}"
            )),
            Version::Bad => Err(format!("version is not {FORMAT}")),
        }
    }

    /// The file's text, unknown keys and all. The error is why the table
    /// gives no TOML, so a write never puts an empty file in its place.
    pub fn text(&self) -> Result<String, toml::ser::Error> {
        let mut t = self.table.clone();
        t.insert("version".into(), Value::Integer(FORMAT));
        toml::to_string(&t)
    }

    /// Write the file to `dir`, through a temporary file and a rename.
    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        let text = self.text().map_err(std::io::Error::other)?;
        write_file(dir, SETTINGS, text.as_bytes())
    }

    /// A value of the wrong kind is off, as a missing one: a fact of the
    /// file, and Studio writes the key back right.
    fn flag(&self, key: &str) -> bool {
        matches!(self.table.get(key), Some(Value::Boolean(true)))
    }

    /// The theme in use: a theme that ships by name, or a file of the
    /// themes folder.
    /// A value of the wrong kind names no theme: a fact of the file.
    pub fn theme(&self) -> Option<&str> {
        match self.table.get("theme") {
            Some(Value::String(s)) => Some(s),
            _ => None,
        }
    }

    /// High legibility is on.
    pub fn high_legibility(&self) -> bool {
        self.flag("high_legibility")
    }

    /// The header is compact at any size.
    pub fn compact_header(&self) -> bool {
        self.flag("compact_header")
    }

    /// The changed theme, when one was kept and its keys are right. A key
    /// of the wrong kind is a fact of the file: no changed theme.
    pub fn unsaved(&self) -> Option<Unsaved> {
        let Some(Value::Table(t)) = self.table.get("unsaved") else {
            return None;
        };
        let Some(Value::String(from)) = t.get("from") else {
            return None;
        };
        let Some(Value::String(base)) = t.get("base") else {
            return None;
        };
        let base = colour(base)?;
        let (strengths, moves, rate) = screen_of(Some(t), SETTINGS, &mut Vec::new()).ok()?;
        let name = match t.get("name") {
            Some(Value::String(n)) => Some(n.clone()),
            _ => None,
        };
        Some(Unsaved {
            from: from.clone(),
            name,
            spec: Spec {
                name: Cow::Owned(String::new()),
                base,
                strengths,
                moves,
                rate,
            },
        })
    }

    /// Set the two switches alone. The theme in use and the changed theme
    /// stay as they were read.
    pub fn set_switches(&mut self, high_legibility: bool, compact_header: bool) {
        let t = &mut self.table;
        t.insert("version".into(), Value::Integer(FORMAT));
        t.insert("high_legibility".into(), Value::Boolean(high_legibility));
        t.insert("compact_header".into(), Value::Boolean(compact_header));
    }

    /// Set the theme in use, the switches and the changed theme.
    pub fn set(
        &mut self,
        theme: &str,
        high_legibility: bool,
        compact_header: bool,
        unsaved: Option<&Unsaved>,
    ) {
        let t = &mut self.table;
        t.insert("version".into(), Value::Integer(FORMAT));
        t.insert("theme".into(), Value::String(theme.to_owned()));
        t.insert("high_legibility".into(), Value::Boolean(high_legibility));
        t.insert("compact_header".into(), Value::Boolean(compact_header));
        // A key of `[unsaved]` this Studio does not know stays. A value
        // of the wrong kind in its place is not a table and goes.
        let mut s = match t.remove("unsaved") {
            Some(Value::Table(s)) => s,
            _ => Table::new(),
        };
        for key in UNSAVED_KEYS.iter().chain(STRENGTH_KEYS.iter()) {
            s.remove(*key);
        }
        match unsaved {
            None => {
                if !s.is_empty() {
                    t.insert("unsaved".into(), Value::Table(s));
                }
            }
            Some(u) => {
                s.insert("from".into(), Value::String(u.from.clone()));
                if let Some(n) = &u.name {
                    s.insert("name".into(), Value::String(n.clone()));
                }
                s.insert("base".into(), Value::String(hex(u.spec.base)));
                for (i, key) in STRENGTH_KEYS.iter().enumerate() {
                    let v = u.spec.strengths.get(i).unwrap_or(0.0);
                    // Two decimals, the slider's step, so 0.35 stays 0.35.
                    let v = (f64::from(v) / crt::SLIDER_STEP).round() / crt::SLIDER_STEP.recip();
                    s.insert((*key).into(), Value::Float(v));
                }
                s.insert("moves".into(), Value::Boolean(u.spec.moves));
                s.insert("rate".into(), Value::Integer(i64::from(u.spec.rate)));
                t.insert("unsaved".into(), Value::Table(s));
            }
        }
    }
}

/// The keys of `[unsaved]` besides the ten strengths.
const UNSAVED_KEYS: [&str; 5] = ["from", "name", "base", "moves", "rate"];

fn no_settings(why: &str) -> Note {
    warn(format!(
        "%STUDIO-W-NOSETTINGS, could not read {SETTINGS}: {why}. Studio starts with its defaults and writes no settings in this run. Fix the file or delete it."
    ))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test; a failed unwrap is a failed test"
)]
pub(crate) mod tests;
