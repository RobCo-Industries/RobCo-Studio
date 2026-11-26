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
        /// The target, as the status bar shows it.
        target: String,
    },
    /// It has one and the core refused it, with the core's reason.
    Refused(String),
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
        let manifest = match std::fs::read(root.join("robco.toml")) {
            Err(_) => Manifest::Missing,
            Ok(bytes) => match studio_core::manifest::parse(&bytes) {
                Ok(m) => Manifest::Read {
                    name: m.project.name.clone(),
                    target: format!("Pip-OS ABI {}.{}", m.target.abi.major, m.target.abi.minor),
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
        let mut list: Vec<(bool, String, PathBuf)> = entries
            .filter_map(Result::ok)
            .map(|e| {
                let path = e.path();
                (
                    !path.is_dir(),
                    e.file_name().to_string_lossy().into_owned(),
                    path,
                )
            })
            .collect();
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
}
