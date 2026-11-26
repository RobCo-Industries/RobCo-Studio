// SPDX-License-Identifier: GPL-3.0-or-later

//! The cache folder, by "The cache folder" of RobCo-Core
//! `docs/spec/SDK_PACKAGE.md`.

use std::ffi::OsString;
use std::path::PathBuf;

/// The cache folder, from the environment that `var` reads. The error is
/// the detail of `cache: path`.
///
/// `ROBCO_CACHE`, when set and not empty, is the folder itself and must be
/// absolute. Otherwise the system's folder plus `robco`. The spec does not
/// say what happens when that system folder has no value at all, such as
/// `HOME` unset on Linux: this tool fails with `cache: path` too.
pub fn folder(var: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf, String> {
    let set = |name: &str| var(name).filter(|v| !v.is_empty());
    if let Some(v) = set("ROBCO_CACHE") {
        let p = PathBuf::from(&v);
        return if p.is_absolute() {
            Ok(p)
        } else {
            Err(format!(
                "ROBCO_CACHE is {}, not an absolute path",
                p.display()
            ))
        };
    }
    let absolute = |name: &str| set(name).map(PathBuf::from).filter(|p| p.is_absolute());
    let base = if cfg!(windows) {
        absolute("LOCALAPPDATA").ok_or("LOCALAPPDATA is not an absolute path")?
    } else if cfg!(target_os = "macos") {
        absolute("HOME")
            .ok_or("HOME is not an absolute path")?
            .join("Library")
            .join("Caches")
    } else {
        match absolute("XDG_CACHE_HOME") {
            Some(p) => p,
            None => absolute("HOME")
                .ok_or("HOME is not an absolute path")?
                .join(".cache"),
        }
    };
    Ok(base.join("robco"))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test; a failed unwrap is a failed test"
)]
mod tests {
    use super::*;

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
    fn robco_cache_is_the_folder_itself() {
        assert_eq!(
            folder(with(&[("ROBCO_CACHE", ABS)])).unwrap(),
            PathBuf::from(ABS)
        );
    }

    #[test]
    fn a_relative_robco_cache_is_refused() {
        assert!(folder(with(&[("ROBCO_CACHE", "rel/cache")])).is_err());
    }

    #[test]
    fn an_empty_robco_cache_counts_as_unset() {
        let got = folder(with(&[
            ("ROBCO_CACHE", ""),
            ("HOME", ABS),
            ("LOCALAPPDATA", ABS),
        ]))
        .unwrap();
        assert!(got.starts_with(ABS) && got.ends_with("robco"), "{got:?}");
    }

    #[test]
    fn no_home_is_refused() {
        assert!(folder(with(&[])).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_takes_an_absolute_xdg_cache_home() {
        assert_eq!(
            folder(with(&[("XDG_CACHE_HOME", "/x"), ("HOME", "/h")])).unwrap(),
            PathBuf::from("/x/robco")
        );
        assert_eq!(
            folder(with(&[("XDG_CACHE_HOME", "x"), ("HOME", "/h")])).unwrap(),
            PathBuf::from("/h/.cache/robco")
        );
    }
}
