// SPDX-License-Identifier: GPL-3.0-or-later

//! A person's own themes and `settings.toml`, as the window uses them:
//! read at start, written when a setting changes, and the calls the theme
//! editor makes to save, rename, delete and add a theme.

use super::Studio;
use crate::kit::Level;
use crate::store::{self, Note, Settings, Unsaved};
use crate::theme::{SHIPPED, Spec, shipped};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// The settings folder and what Studio read there.
pub(super) struct Store {
    dir: PathBuf,
    settings: Settings,
    /// False when `settings.toml` could not be read, or a theme was asked
    /// for on the command line: Studio then writes no settings in the run.
    write: bool,
}

/// Why a theme was not saved, renamed, deleted or added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeFault {
    /// The name holds a character outside the set.
    BadName,
    /// The name holds no letter and no digit.
    NoName,
    /// The name is longer than 32 characters.
    LongName,
    /// The name of a theme that ships, as it ships.
    Shipped(String),
    /// A theme of the person's own has this name, as it is listed.
    /// Replace writes over it.
    Exists(String),
    /// The theme in use ships with Studio, so it has no file to change.
    ShippedTheme,
    /// Studio has no settings folder in this run.
    NoFolder,
    /// The file to add is not a theme: the line that says why.
    NotTheme(String),
    /// Studio could not write or delete the file: the line that says why.
    Write(String),
}

impl ThemeFault {
    /// The words for the field, the button's tooltip or the status line.
    pub fn message(&self) -> String {
        match self {
            ThemeFault::BadName => "A name takes letters, digits, spaces and - _ . ' only.".into(),
            ThemeFault::NoName => "A name needs a letter or a digit.".into(),
            ThemeFault::LongName => "A name is 32 characters at most.".into(),
            ThemeFault::Shipped(n) => format!("{n} ships with Studio. Choose another name."),
            ThemeFault::Exists(n) => format!("A theme named {n} exists. Replace writes over it."),
            ThemeFault::ShippedTheme => {
                "A theme that ships with Studio cannot be changed. Save as keeps yours.".into()
            }
            ThemeFault::NoFolder => {
                "Studio has no settings folder in this run. The theme is not saved.".into()
            }
            ThemeFault::NotTheme(t) | ThemeFault::Write(t) => t.clone(),
        }
    }
}

impl Studio {
    /// The same window with its settings folder `dir`: it reads
    /// `settings.toml` and the themes folder now, takes the last theme
    /// again, and writes the settings when they change. A fault in a file
    /// never stops the window: it says why on the status line.
    pub fn with_settings(mut self, dir: PathBuf) -> Self {
        let (settings, write) = match Settings::read(&dir) {
            Ok(s) => (s, true),
            Err(n) => {
                self.note(n);
                (Settings::default(), false)
            }
        };
        self.store = Some(Store {
            dir,
            settings: settings.clone(),
            write,
        });
        self.reload_themes();
        self.theme.high_legibility = settings.high_legibility();
        self.compact_header = settings.compact_header();
        let unsaved = settings.unsaved();
        for key in settings
            .theme()
            .into_iter()
            .chain(unsaved.as_ref().map(|u| u.from.as_str()))
        {
            match self.find_key(key) {
                Some(i) => self.current = i,
                None => {
                    if key.ends_with(".toml") {
                        self.note(Note {
                            level: Level::Warning,
                            text: format!(
                                "%STUDIO-W-THEMEGONE, the theme file {} is gone. Theme: Green.",
                                store::shown(key)
                            ),
                        });
                    }
                    self.current = 0;
                }
            }
        }
        if let Some(u) = unsaved
            && self.find_key(&u.from).is_some()
        {
            self.changed = Some(Spec {
                name: self.spec().name.clone(),
                ..u.spec
            });
        }
        self.refresh_theme();
        self
    }

    /// Write no settings for the rest of the run, as with a theme asked
    /// for on the command line.
    pub fn hold_settings(&mut self) {
        if let Some(s) = &mut self.store {
            s.write = false;
        }
    }

    /// The themes folder, when Studio has a settings folder.
    pub fn themes_folder(&self) -> Option<PathBuf> {
        self.store.as_ref().map(|s| s.dir.join(store::THEMES))
    }

    /// The place in `themes` of the theme named `name`, in any case, or of
    /// the file named `name`: what `--theme NAME` takes.
    pub fn theme_index(&self, name: &str) -> Option<usize> {
        shipped(name)
            .or_else(|| {
                (0..self.themes.len()).find(|i| {
                    self.theme_files.get(*i).is_some_and(|f| f.is_some())
                        && self.own_name(*i).eq_ignore_ascii_case(name)
                })
            })
            .or_else(|| self.find_key(name))
    }

    /// Say a line about a file, once in a session.
    fn note(&mut self, n: Note) {
        if !self.said.contains(&n.text) {
            self.said.push(n.text.clone());
            self.say(n.level, n.text);
        }
    }

    /// How `settings.toml` names the theme at `i`: its file, or the name
    /// of a theme that ships.
    pub(super) fn key(&self, i: usize) -> String {
        match self.theme_files.get(i) {
            Some(Some(f)) => f.clone(),
            _ => self
                .themes
                .get(i)
                .map_or_else(|| SHIPPED[0].name.to_string(), |t| t.name.to_string()),
        }
    }

    /// The place of the theme `key` names.
    pub(super) fn find_key(&self, key: &str) -> Option<usize> {
        if key.ends_with(".toml") {
            self.theme_files
                .iter()
                .position(|f| f.as_deref() == Some(key))
        } else {
            shipped(key)
        }
    }

    /// The name of the theme at `i` as its file gives it, without the
    /// file's name a listed twin carries.
    pub(super) fn own_name(&self, i: usize) -> String {
        let name = self.themes.get(i).map_or("", |t| t.name.as_ref());
        match self.theme_files.get(i) {
            Some(Some(f)) => name
                .strip_suffix(&format!(", {}", store::shown(f)))
                .unwrap_or(name)
                .to_owned(),
            _ => name.to_owned(),
        }
    }

    /// Read the themes folder again. The theme in use stays when its file
    /// is still there; otherwise Green is.
    pub fn reload_themes(&mut self) {
        let key = self.key(self.current);
        let folder = self
            .themes_folder()
            .map(|d| store::read_folder(&d))
            .unwrap_or_default();
        for n in folder.notes {
            self.note(n);
        }
        self.refused = folder
            .refused
            .iter()
            .map(|(f, n)| (f.clone(), n.text.clone()))
            .collect();
        for (_, n) in folder.refused {
            self.note(n);
        }
        self.themes = SHIPPED.to_vec();
        self.theme_files = vec![None; SHIPPED.len()];
        for (f, t) in folder.themes {
            self.themes.push(t);
            self.theme_files.push(Some(f));
        }
        self.current = self.find_key(&key).unwrap_or(0);
    }

    /// Write `settings.toml` now, when Studio has a settings folder and
    /// may write it. A failure says why on the status line.
    pub fn save_settings(&mut self) {
        self.settings_due = false;
        let key = self.key(self.current);
        let unsaved = self.changed.as_ref().map(|c| Unsaved {
            from: key.clone(),
            spec: c.clone(),
        });
        let (hl, compact) = (self.theme.high_legibility, self.compact_header);
        let Some(st) = &mut self.store else {
            return;
        };
        if !st.write {
            return;
        }
        st.settings.set(&key, hl, compact, unsaved.as_ref());
        if let Err(e) = st.settings.write(&st.dir) {
            self.note(Note {
                level: Level::Warning,
                text: format!("Studio could not write {}: {e}.", store::SETTINGS),
            });
        }
    }

    /// Check a name a person typed for a theme of their own. `except` is
    /// the place of the theme being renamed, which may keep its name. The
    /// result is the name with its spaces made right.
    pub fn check_name(&self, raw: &str, except: Option<usize>) -> Result<String, ThemeFault> {
        match self.name_check(raw, except)? {
            (n, None) => Ok(n),
            (_, Some(k)) => Err(ThemeFault::Exists(self.own_name(k))),
        }
    }

    /// The name with its spaces made right, and the place of a theme of
    /// the person's own that has it.
    fn name_check(
        &self,
        raw: &str,
        except: Option<usize>,
    ) -> Result<(String, Option<usize>), ThemeFault> {
        if !raw.chars().all(store::allowed) {
            return Err(ThemeFault::BadName);
        }
        let name = raw
            .split(' ')
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !name.chars().any(char::is_alphanumeric) {
            return Err(ThemeFault::NoName);
        }
        if name.chars().count() > store::NAME_MAX {
            return Err(ThemeFault::LongName);
        }
        if let Some(i) = shipped(&name) {
            return Err(ThemeFault::Shipped(SHIPPED[i].name.to_string()));
        }
        let lower = name.to_lowercase();
        let taken = (SHIPPED.len()..self.themes.len())
            .find(|i| Some(*i) != except && self.own_name(*i).to_lowercase() == lower);
        Ok((name, taken))
    }

    /// The name `base` with the next free number, "Green 2", for the name
    /// row and the question.
    pub fn next_name(&self, base: &str) -> String {
        let base: String = base.chars().take(store::NAME_MAX - 4).collect();
        let base = base.trim_end();
        (2..10_000)
            .map(|n| format!("{base} {n}"))
            .find(|c| self.check_name(c, None).is_ok())
            .unwrap_or_else(|| base.to_owned())
    }

    /// The themes folder, or the fault when there is none.
    fn folder_or_fault(&self) -> Result<PathBuf, ThemeFault> {
        self.themes_folder().ok_or(ThemeFault::NoFolder)
    }

    /// A new file name in `dir` for the name `name`.
    fn new_file(dir: &Path, name: &str) -> String {
        let have: Vec<String> = std::fs::read_dir(dir)
            .map(|r| {
                r.flatten()
                    .map(|e| e.file_name().to_string_lossy().to_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        store::file_name(name, |f| have.iter().any(|h| *h == f))
    }

    /// Write `spec` as `file`, read the folder again and take that theme.
    /// The result is the line for the status line.
    fn keep(&mut self, dir: &Path, file: &str, spec: &Spec) -> Result<String, ThemeFault> {
        store::write_theme(dir, file, spec).map_err(|n| ThemeFault::Write(n.text))?;
        self.reload_themes();
        self.current = self.find_key(file).unwrap_or(0);
        self.changed = None;
        self.refresh_theme();
        self.save_settings();
        Ok(store::saved_news(&spec.name, file))
    }

    /// Save the theme in use, changed or not, under the name `name`, as a
    /// new file. With `replace`, a theme of the person's own with that name
    /// is written over; without it, that name is [`ThemeFault::Exists`].
    /// The window then takes the saved theme.
    pub fn save_theme_as(&mut self, name: &str, replace: bool) -> Result<String, ThemeFault> {
        let (name, taken) = self.name_check(name, None)?;
        let dir = self.folder_or_fault()?;
        let file = match taken {
            Some(k) if replace => self.key(k),
            Some(k) => return Err(ThemeFault::Exists(self.own_name(k))),
            None => Self::new_file(&dir, &name),
        };
        let spec = Spec {
            name: Cow::Owned(name),
            ..self.spec().clone()
        };
        let line = self.keep(&dir, &file, &spec)?;
        self.say(Level::Success, line.clone());
        Ok(line)
    }

    /// Write the changed theme over its own file. A theme that ships has
    /// none: [`ThemeFault::ShippedTheme`].
    pub fn save_theme(&mut self) -> Result<String, ThemeFault> {
        let Some(Some(file)) = self.theme_files.get(self.current).cloned() else {
            return Err(ThemeFault::ShippedTheme);
        };
        let dir = self.folder_or_fault()?;
        let spec = Spec {
            name: Cow::Owned(self.own_name(self.current)),
            ..self.spec().clone()
        };
        let line = self.keep(&dir, &file, &spec)?;
        self.say(Level::Success, line.clone());
        Ok(line)
    }

    /// Give the theme in use, one of the person's own, the name `name`.
    /// The name inside its file changes, the file keeps its file name, and
    /// a change not saved stays not saved.
    pub fn rename_theme(&mut self, name: &str) -> Result<String, ThemeFault> {
        let Some(Some(file)) = self.theme_files.get(self.current).cloned() else {
            return Err(ThemeFault::ShippedTheme);
        };
        let name = self.check_name(name, Some(self.current))?;
        let dir = self.folder_or_fault()?;
        let changed = self.changed.take();
        let spec = Spec {
            name: Cow::Owned(name.clone()),
            ..self.themes[self.current].clone()
        };
        let kept = self.keep(&dir, &file, &spec);
        self.changed = changed.map(|c| Spec {
            name: self.spec().name.clone(),
            ..c
        });
        self.refresh_theme();
        self.save_settings();
        kept?;
        let line = crate::theme::news(&name, None);
        self.say(Level::Info, line.clone());
        Ok(line)
    }

    /// Delete the file of the theme in use, one of the person's own. The
    /// window goes back to Green.
    pub fn delete_theme(&mut self) -> Result<String, ThemeFault> {
        let Some(Some(file)) = self.theme_files.get(self.current).cloned() else {
            return Err(ThemeFault::ShippedTheme);
        };
        let dir = self.folder_or_fault()?;
        let name = self.own_name(self.current);
        let cannot = |why: &str| {
            ThemeFault::Write(format!(
                "Studio could not delete {}: {why}. The theme is kept.",
                store::shown(&file)
            ))
        };
        // One plain file of the folder itself: no part of a path, no link
        // and no folder.
        let plain = Path::new(&file)
            .file_name()
            .is_some_and(|n| n == std::ffi::OsStr::new(&file))
            && file.ends_with(".toml");
        if !plain {
            return Err(cannot("not a theme file of the folder"));
        }
        let meta =
            std::fs::symlink_metadata(dir.join(&file)).map_err(|e| cannot(&e.to_string()))?;
        if !meta.is_file() {
            return Err(cannot("not a plain file"));
        }
        std::fs::remove_file(dir.join(&file)).map_err(|e| {
            ThemeFault::Write(format!(
                "Studio could not delete {}: {e}. The theme is kept.",
                store::shown(&file)
            ))
        })?;
        self.current = 0;
        self.changed = None;
        self.reload_themes();
        self.refresh_theme();
        self.save_settings();
        let line = format!(
            "Theme {name} deleted. {}",
            crate::theme::news(&SHIPPED[0].name, None)
        );
        self.say(Level::Info, line.clone());
        Ok(line)
    }

    /// Add the theme file at `path`: read it as untrusted text and, when it
    /// is a theme, write a new clean file from the values understood, under
    /// a new file name, and take it. It never writes over a file.
    pub fn add_theme_file(&mut self, path: &Path) -> Result<String, ThemeFault> {
        let dir = self.folder_or_fault()?;
        let file = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let bytes = store::read_plain(path, store::MAX_BYTES)
            .map_err(|e| ThemeFault::NotTheme(store::not_theme(&file, &e.to_string()).text))?;
        let loaded = store::read_theme(&file, &bytes).map_err(|n| ThemeFault::NotTheme(n.text))?;
        for n in loaded.notes {
            self.note(n);
        }
        let mut name = store::clean(&loaded.spec.name);
        if name.is_empty() {
            name = "Theme".to_owned();
        }
        let file = Self::new_file(&dir, &name);
        let spec = Spec {
            name: Cow::Owned(name),
            ..loaded.spec
        };
        let line = self.keep(&dir, &file, &spec)?;
        self.say(Level::Success, line.clone());
        Ok(line)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test; a failed unwrap is a failed test"
)]
mod tests;
