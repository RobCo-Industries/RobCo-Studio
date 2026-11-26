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
    /// Studio fell back to Green by itself, at start or at a reload: the
    /// stored theme or the theme its changed theme came from could not be
    /// had, for any reason. Studio then writes the stored choice and the
    /// stored changed theme back as they were, until the person picks,
    /// changes, saves, renames or deletes a theme.
    keep_choice: bool,
    /// The files of the last read of the themes folder that Studio could
    /// not read, or None when it could not read the folder itself.
    unread: Option<Vec<String>>,
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
    /// The themes folder holds the most files Studio reads, so a new file
    /// would not be listed.
    Full,
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
            ThemeFault::Full => format!(
                "The themes folder holds {} files, the most Studio reads. Delete one first. The theme is not saved.",
                store::MAX_FILES
            ),
            ThemeFault::NotTheme(t) | ThemeFault::Write(t) => t.clone(),
        }
    }
}

/// The fault for a write into a themes folder Studio could not read.
fn unlisted(file: &str, why: &str) -> ThemeFault {
    ThemeFault::Write(format!(
        "%STUDIO-E-THEMEWRITE, could not write {}: could not read the themes folder: {why}. The theme is not saved.",
        store::shown(file)
    ))
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
            keep_choice: false,
            unread: Some(Vec::new()),
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
                    self.fell_back(key);
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

    /// The file `file` of the themes folder could not be read at the last
    /// read, or the folder itself could not.
    fn unread(&self, file: &str) -> bool {
        self.store.as_ref().is_some_and(|st| match &st.unread {
            None => true,
            Some(u) => u.iter().any(|f| f == file),
        })
    }

    /// Studio could not have the theme `key` names and shows Green. It
    /// keeps the stored choice and says why once: a refused file said so
    /// when the folder was read.
    fn fell_back(&mut self, key: &str) {
        if let Some(st) = &mut self.store {
            st.keep_choice = true;
        }
        if !key.ends_with(".toml") {
            return;
        }
        let text = if self.unread(key) {
            format!(
                "%STUDIO-W-THEMEKEPT, could not read the theme file {}. Theme: Green. settings.toml keeps {} until you pick a theme.",
                store::shown(key),
                store::shown(key)
            )
        } else if self.refused.iter().any(|(f, _)| f == key) {
            return;
        } else {
            format!(
                "%STUDIO-W-THEMEGONE, the theme file {} is gone. Theme: Green.",
                store::shown(key)
            )
        };
        self.note(Note {
            level: Level::Warning,
            text,
        });
    }

    /// The person chose a theme on purpose: from now on the settings
    /// keep what the window shows.
    pub(super) fn chose(&mut self) {
        if let Some(st) = &mut self.store {
            st.keep_choice = false;
        }
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
                        && store::same_name(&self.own_name(*i), name)
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
        // A file's name is matched as it is, not as a theme's name: Studio
        // writes the key from the file's name itself.
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
    /// is still there; otherwise Green is, and the stored choice stays. A folder Studio could not read
    /// leaves the list as it was, and so does the file of the theme in use
    /// when Studio could not read it: a failed read is not news about a
    /// file.
    pub fn reload_themes(&mut self) {
        let Some(dir) = self.themes_folder() else {
            return;
        };
        let key = self.key(self.current);
        let folder = store::read_folder(&dir);
        if let Some(n) = folder.unreadable {
            let more = if self.theme_files.iter().any(Option::is_some) {
                "The list stays as it was."
            } else {
                "Your own themes are not listed."
            };
            self.note(Note {
                level: n.level,
                text: format!("{} {more}", n.text),
            });
            if let Some(st) = &mut self.store {
                st.unread = None;
            }
            return;
        }
        for n in folder.notes {
            self.note(n);
        }
        let mut listed = folder.themes;
        let mut unread = Vec::new();
        self.refused.clear();
        for (f, r) in folder.refused {
            if let store::Refusal::Unreadable(_) = r {
                unread.push(f.clone());
                // The theme in use keeps what Studio read of it before.
                if f == key
                    && let Some(old) = self.theme_files.iter().position(|t| t.as_ref() == Some(&f))
                {
                    listed.push((f.clone(), self.themes[old].clone()));
                    self.note(r.note().clone());
                    continue;
                }
            }
            self.refused.push((f, r.note().text.clone()));
            self.note(r.note().clone());
        }
        listed.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(st) = &mut self.store {
            st.unread = Some(unread);
        }
        self.themes = SHIPPED.to_vec();
        self.theme_files = vec![None; SHIPPED.len()];
        for (f, t) in listed {
            self.themes.push(t);
            self.theme_files.push(Some(f));
        }
        match self.find_key(&key) {
            Some(i) => self.current = i,
            None => {
                self.current = 0;
                self.fell_back(&key);
            }
        }
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
        if st.keep_choice {
            st.settings.set_switches(hl, compact);
        } else {
            st.settings.set(&key, hl, compact, unsaved.as_ref());
        }
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
        let taken = (SHIPPED.len()..self.themes.len())
            .find(|i| Some(*i) != except && store::same_name(&self.own_name(*i), &name));
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

    /// A new file name in `dir` for the name `name`. A folder Studio
    /// cannot read may hold any name, so it gives none.
    fn new_file(dir: &Path, name: &str) -> Result<String, ThemeFault> {
        // File names, not theme names: in lower case, so a new file never
        // lands on an old one where the system ignores case.
        let have: Vec<String> = match std::fs::read_dir(dir) {
            Ok(r) => {
                let mut have = Vec::new();
                for e in r {
                    let e = e.map_err(|e| unlisted(name, &e.to_string()))?;
                    have.push(e.file_name().to_string_lossy().to_lowercase());
                }
                have
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(unlisted(name, &e.to_string())),
        };
        Ok(store::file_name(name, |f| have.iter().any(|h| *h == f)))
    }

    /// Write `spec` as `file`, read the folder again and take that theme.
    /// The result is the line for the status line.
    ///
    /// A new file is refused when the folder holds [`store::MAX_FILES`]
    /// already, since Studio would not list it. Success means the theme is
    /// listed and in use. On failure the changed theme stays changed.
    fn keep(&mut self, dir: &Path, file: &str, spec: &Spec) -> Result<String, ThemeFault> {
        let files = match store::toml_files(dir) {
            store::Listing::Unreadable(why) => return Err(unlisted(file, &why)),
            l => l.files().to_vec(),
        };
        let new = !files.iter().any(|f| f == file);
        if new && files.len() >= store::MAX_FILES {
            return Err(ThemeFault::Full);
        }
        store::write_theme(dir, file, spec).map_err(|n| ThemeFault::Write(n.text))?;
        self.reload_themes();
        let Some(i) = self.find_key(file) else {
            // Another program filled the folder meanwhile. A new file goes
            // again, so the folder is as it was.
            if new {
                let _ = std::fs::remove_file(dir.join(file));
                self.reload_themes();
            }
            self.refresh_theme();
            return Err(ThemeFault::Full);
        };
        self.current = i;
        self.changed = None;
        self.chose();
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
            None => Self::new_file(&dir, &name)?,
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
        self.chose();
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

    /// Read the theme file at `path` as untrusted text: the theme it gives,
    /// with its name cleaned, when it is one.
    pub(super) fn read_theme_file(&mut self, path: &Path) -> Result<Spec, ThemeFault> {
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
        Ok(Spec {
            name: Cow::Owned(name),
            ..loaded.spec
        })
    }

    /// Add the theme file at `path` and take it. A file that is not a theme
    /// is refused first. While a changed theme is not saved, this asks the
    /// question of a pick and gives `None`; the file is added after Save or
    /// Do not save. Otherwise it gives the line for the status line.
    pub fn add_theme_file(&mut self, path: &Path) -> Result<Option<String>, ThemeFault> {
        self.folder_or_fault()?;
        let spec = self.read_theme_file(path)?;
        self.leave_for(super::Leaving::File {
            path: path.to_owned(),
            name: spec.name.into_owned(),
        })
    }

    /// Write the theme file at `path` as a new clean file from the values
    /// understood, under a new file name, and take it. It never writes over
    /// a file. Only [`Studio::leave_for`] calls this, after its check.
    pub(super) fn take_file(&mut self, path: &Path) -> Result<String, ThemeFault> {
        let dir = self.folder_or_fault()?;
        let spec = self.read_theme_file(path)?;
        let file = Self::new_file(&dir, &spec.name)?;
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
