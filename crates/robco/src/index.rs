// SPDX-License-Identifier: GPL-3.0-or-later

//! Finds and reads the index, by "Where the index is" and "Reading the
//! index" of RobCo-Core `docs/spec/SDK_PACKAGE.md`.

use std::path::{Path, PathBuf};

use studio_core::resolve::index::{self, Index};

use crate::Failure;

/// What a `--index` location is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// A local file path, absolute or relative to the current folder.
    Local(PathBuf),
    /// An `https://` URL.
    Https(String),
}

/// Sorts a location by its form. `None` when there is no location, and a
/// refused scheme, both give `index: location`.
pub fn locate(given: Option<&str>) -> Result<Location, Failure> {
    let Some(s) = given else {
        return Err(Failure::new(
            "index: location",
            "this command reads the index: give --index LOCATION",
        ));
    };
    if s.starts_with("https://") {
        return Ok(Location::Https(s.to_owned()));
    }
    if let Some((scheme, _)) = s.split_once("://")
        && is_scheme(scheme)
    {
        return Err(Failure::new(
            "index: location",
            format!("{s}: only https:// or a local file"),
        ));
    }
    Ok(Location::Local(PathBuf::from(s)))
}

/// A URL scheme: a letter, then letters, digits, `+`, `-` and `.`.
fn is_scheme(s: &str) -> bool {
    let mut b = s.bytes();
    b.next().is_some_and(|c| c.is_ascii_alphabetic())
        && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
}

/// Reads the index at a location, and returns it with the folder that its
/// relative `url` values start from.
///
/// No `https` transport exists yet: an `https://` index fails with
/// `index: read`, the code of a download that fails, and says why.
pub fn read(location: &Location) -> Result<(Index, PathBuf), Failure> {
    match location {
        Location::Https(url) => Err(Failure::new(
            "index: read",
            format!("{url}: this build of robco cannot download yet, give a local index file"),
        )),
        Location::Local(path) => {
            let index = index::read_file(path).map_err(|e| Failure::of(&e))?;
            let base = match path.parent() {
                Some(p) if p != Path::new("") => p.to_path_buf(),
                _ => PathBuf::from("."),
            };
            Ok((index, base))
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test; a failed unwrap is a failed test"
)]
mod tests {
    use super::*;

    #[test]
    fn forms_of_a_location() {
        assert_eq!(locate(None).unwrap_err().code, "index: location");
        assert_eq!(
            locate(Some("http://x/i.json")).unwrap_err().code,
            "index: location"
        );
        assert_eq!(
            locate(Some("ftp://x/i.json")).unwrap_err().code,
            "index: location"
        );
        assert_eq!(
            locate(Some("https://x/i.json")).unwrap(),
            Location::Https("https://x/i.json".to_owned())
        );
        assert_eq!(
            locate(Some("dir/i.json")).unwrap(),
            Location::Local(PathBuf::from("dir/i.json"))
        );
        assert_eq!(
            locate(Some("C:\\i.json")).unwrap(),
            Location::Local(PathBuf::from("C:\\i.json"))
        );
        // "://" after something that is no scheme is a path.
        assert!(matches!(
            locate(Some("a b://c")).unwrap(),
            Location::Local(_)
        ));
    }

    #[test]
    fn an_https_index_is_not_fetched() {
        let e = read(&Location::Https("https://x/i.json".to_owned())).unwrap_err();
        assert_eq!(e.code, "index: read");
    }

    #[test]
    fn a_missing_file_is_index_read() {
        let e = read(&Location::Local(PathBuf::from("no-such-index.json"))).unwrap_err();
        assert_eq!(e.code, "index: read");
    }
}
