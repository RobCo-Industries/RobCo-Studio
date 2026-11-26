// SPDX-License-Identifier: GPL-3.0-or-later

//! The open project: a folder, its manifest if it has one, and the tree of
//! its files as the Files pane shows it. Folders are read when opened.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What the folder's `robco.toml` said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Manifest {
    /// The folder has no `robco.toml`.
    Missing,
    /// It has one and it reads: the project name and its target.
    Read {
        /// `project.name`.
        name: String,
        /// The target, as the toolbar shows it.
        target: String,
        /// What it builds, as the Tape pane shows it: "Holotape, game".
        kind: String,
    },
    /// It has one and the core refused it, with the core's reason.
    Refused(String),
    /// It is there but could not be read, with the system's reason.
    Unreadable(String),
}

/// One row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The file or folder.
    pub path: PathBuf,
    /// Its name.
    pub name: String,
    /// Folders count from 0 at the project's top.
    pub depth: usize,
    /// A folder.
    pub is_dir: bool,
    /// A folder shown open.
    pub open: bool,
}

/// An open folder.
#[derive(Debug)]
pub struct Project {
    /// The folder.
    pub root: PathBuf,
    /// Its manifest.
    pub manifest: Manifest,
    open: BTreeSet<PathBuf>,
    /// The rows as last read.
    pub rows: Vec<Row>,
    /// The row last chosen.
    pub selected: Option<PathBuf>,
    /// The last error from reading a folder.
    pub error: Option<String>,
}

impl Project {
    /// Open `root` and read its top level.
    pub fn open(root: &Path) -> Result<Project, String> {
        if !root.is_dir() {
            return Err(format!(
                "{} is not a folder. No project is open.",
                root.display()
            ));
        }
        let path = root.join("robco.toml");
        let manifest = match crate::document::read_regular(&path, crate::document::MAX_FILE) {
            Err(_)
                if std::fs::metadata(&path)
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                Manifest::Missing
            }
            Err(crate::document::LoadError::Read(e)) => Manifest::Unreadable(e),
            Err(crate::document::LoadError::TooLarge) => Manifest::Unreadable(format!(
                "it is larger than {} MiB, the most Studio reads.",
                crate::document::MAX_FILE / (1024 * 1024)
            )),
            Err(_) => Manifest::Unreadable(
                "it is not a regular file. Studio opens regular files only.".to_owned(),
            ),
            Ok(bytes) => match studio_core::manifest::parse(&bytes) {
                Ok(m) => Manifest::Read {
                    name: m.project.name.clone(),
                    target: format!("Pip-OS ABI {}.{}", m.target.abi.major, m.target.abi.minor),
                    kind: format!(
                        "Holotape, {}",
                        match m.project.subtype {
                            studio_core::manifest::Subtype::Game => "game",
                            studio_core::manifest::Subtype::Utility => "utility",
                        }
                    ),
                },
                Err(e) => Manifest::Refused(e.to_string()),
            },
        };
        let mut p = Project {
            root: root.to_owned(),
            manifest,
            open: BTreeSet::new(),
            rows: Vec::new(),
            selected: None,
            error: None,
        };
        p.refresh();
        Ok(p)
    }

    /// The project's name: the manifest's, or the folder's.
    pub fn name(&self) -> String {
        match &self.manifest {
            Manifest::Read { name, .. } => name.clone(),
            _ => self.root.file_name().map_or_else(
                || self.root.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
        }
    }

    /// `path` as shown to a person: relative to the project, with `/`.
    pub fn shown(&self, path: &Path) -> String {
        let rel = path.strip_prefix(&self.root).unwrap_or(path);
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Open or close a folder row.
    pub fn toggle(&mut self, path: &Path) {
        if !self.open.remove(path) {
            self.open.insert(path.to_owned());
        }
        self.refresh();
    }

    /// Open (`true`) or close a folder row.
    pub fn set_open(&mut self, path: &Path, open: bool) {
        if open != self.open.contains(path) {
            self.toggle(path);
        }
    }

    /// Read the open folders again.
    pub fn refresh(&mut self) {
        let mut rows = Vec::new();
        self.error = None;
        let root = self.root.clone();
        self.read_into(&root, 0, &mut rows);
        self.rows = rows;
    }

    fn read_into(&mut self, dir: &Path, depth: usize, rows: &mut Vec<Row>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                self.error = Some(format!("Studio could not read {}: {e}.", self.shown(dir)));
                return;
            }
        };
        let mut list: Vec<(bool, String, PathBuf)> = Vec::new();
        for entry in entries {
            match entry {
                Ok(e) => {
                    let path = e.path();
                    list.push((
                        !path.is_dir(),
                        e.file_name().to_string_lossy().into_owned(),
                        path,
                    ));
                }
                // An entry that cannot be read is reported, not dropped
                // as if it were not there.
                Err(e) => {
                    self.error = Some(format!("Studio could not read {}: {e}.", self.shown(dir)));
                }
            }
        }
        // Folders first, then by name, ignoring case.
        list.sort_by_key(|a| (a.0, a.1.to_lowercase()));
        for (is_file, name, path) in list {
            let open = !is_file && self.open.contains(&path);
            rows.push(Row {
                path: path.clone(),
                name,
                depth,
                is_dir: !is_file,
                open,
            });
            if open {
                self.read_into(&path, depth + 1, rows);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_first_and_open_on_demand() {
        let d = std::env::temp_dir().join(format!("robco-studio-project-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("a.txt"), "a").unwrap();
        std::fs::write(d.join("src/main.c"), "int x;\n").unwrap();
        let mut p = Project::open(&d).unwrap();
        let names: Vec<_> = p.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["src", "a.txt"]);
        assert_eq!(p.manifest, Manifest::Missing);
        p.toggle(&d.join("src"));
        let names: Vec<_> = p.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["src", "main.c", "a.txt"]);
        assert_eq!(p.shown(&d.join("src/main.c")), "src/main.c");
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// Removes its folder when dropped, so a failing test cleans up too.
    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    self.0.join("robco.toml"),
                    std::fs::Permissions::from_mode(0o644),
                );
            }
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A manifest that is there but cannot be read is not reported as
    /// missing. Root reads a mode 000 file anyway, so the test checks that
    /// the file really cannot be read, which detects root without `unsafe`
    /// or a new crate, and skips if it can.
    #[cfg(unix)]
    #[test]
    fn unreadable_manifest_is_not_missing() {
        use std::os::unix::fs::PermissionsExt;
        let d = TempDir(
            std::env::temp_dir().join(format!("robco-studio-unreadable-{}", std::process::id())),
        );
        let _ = std::fs::remove_dir_all(&d.0);
        std::fs::create_dir_all(&d.0).unwrap();
        let m = d.0.join("robco.toml");
        std::fs::write(&m, "[project]\n").unwrap();
        std::fs::set_permissions(&m, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&m).is_ok() {
            eprintln!("skipped: this user can read a mode 000 file, likely root");
            return;
        }
        let p = Project::open(&d.0).unwrap();
        assert!(
            matches!(p.manifest, Manifest::Unreadable(_)),
            "{:?}",
            p.manifest
        );
    }

    /// A robco.toml that is a named pipe does not hang the open.
    #[cfg(unix)]
    #[test]
    fn manifest_pipe_does_not_hang() {
        use crate::document::tests::{mkfifo, within_time};
        let d = TempDir(
            std::env::temp_dir().join(format!("robco-studio-manifest-fifo-{}", std::process::id())),
        );
        let _ = std::fs::remove_dir_all(&d.0);
        std::fs::create_dir_all(&d.0).unwrap();
        if !mkfifo(&d.0.join("robco.toml")) {
            return;
        }
        let root = d.0.clone();
        let p = within_time(move || Project::open(&root).map(|p| p.manifest))
            .expect("opening a project whose robco.toml is a pipe hung")
            .unwrap();
        assert!(
            matches!(&p, Manifest::Unreadable(e) if e.contains("regular files only")),
            "{p:?}"
        );
    }
}
