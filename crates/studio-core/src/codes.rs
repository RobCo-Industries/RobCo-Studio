// SPDX-License-Identifier: GPL-3.0-or-later

//! Error codes and the exit code of each.
//!
//! Implements RobCo-Core `docs/spec/STUDIO_CLI.md`, "Exit codes" and
//! "Error codes", as read at RobCo-Core commit `e004c73`. Every error of
//! [`crate::manifest`], [`crate::resolve`] and [`crate::boot`] gives its
//! code through [`ErrorCode`], and [`exit_code`] maps a code to the number
//! the command ends with.

/// Each error code of the spec's table, as the spec spells it, with its
/// exit code.
pub const CODES: &[(&str, u8)] = &[
    ("usage", 2),
    ("project: none", 3),
    ("manifest: syntax", 3),
    ("manifest: format", 3),
    ("manifest: unknown key", 3),
    ("manifest: type", 3),
    ("manifest: missing", 3),
    ("manifest: name", 3),
    ("manifest: version", 3),
    ("manifest: title", 3),
    ("manifest: kind", 3),
    ("manifest: subtype", 3),
    ("manifest: platform", 3),
    ("manifest: abi", 3),
    ("manifest: path", 3),
    ("manifest: icon", 3),
    ("manifest: sources", 3),
    ("lock: syntax", 3),
    ("lock: format", 3),
    ("lock: unknown key", 3),
    ("lock: entry", 3),
    ("lock: stale", 3),
    ("lock: hash", 3),
    ("lock: missing", 3),
    ("lock: tool", 3),
    ("new: name", 3),
    ("new: template", 3),
    ("new: exists", 3),
    ("run: board", 3),
    ("test: none", 3),
    ("test: name", 3),
    ("test: syntax", 3),
    ("test: format", 3),
    ("test: unknown key", 3),
    ("test: type", 3),
    ("test: missing", 3),
    ("test: step", 3),
    ("test: value", 3),
    ("test: golden", 3),
    ("package: archive", 4),
    ("package: format", 4),
    ("package: manifest", 4),
    ("package: hash", 4),
    ("index: location", 4),
    ("index: read", 4),
    ("index: syntax", 4),
    ("index: format", 4),
    ("index: entry", 4),
    ("index: duplicate", 4),
    ("index: host", 4),
    ("sdk: no package", 4),
    ("sdk: no tool", 4),
    ("tool: hash", 4),
    ("tool: archive", 4),
    ("tool: program", 4),
    ("firmware: hash", 4),
    ("cache: path", 4),
    ("templates: none", 4),
    ("templates: format", 4),
    ("templates: manifest", 4),
    ("templates: token", 4),
    ("build: compile", 5),
    ("build: link", 5),
    ("build: objcopy", 5),
    ("build: seal", 5),
    ("run: unmatched", 1),
    ("run: tape", 1),
    ("run: timeout", 6),
    ("sim: start", 7),
    ("sim: died", 7),
    ("sim: qmp", 7),
    ("guest: fault", 8),
    ("guest: runner", 8),
    ("test: failed", 1),
    ("io: read", 9),
    ("io: write", 9),
    ("internal", 10),
];

/// The exit code of an error code, or `None` for a code the table lacks.
pub fn exit_code(code: &str) -> Option<u8> {
    CODES.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

/// An error that carries one code of the spec's table.
pub trait ErrorCode {
    /// The code, spelled as in the table, such as `lock: stale`.
    fn error_code(&self) -> &'static str;

    /// The exit code of [`ErrorCode::error_code`]. A code the table lacks
    /// would be a fault of the tool, so it gives 10, `internal`.
    fn exit_code(&self) -> u8 {
        exit_code(self.error_code()).unwrap_or(10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::BootError;
    use crate::manifest::ManifestError;
    use crate::resolve::Error;

    /// The table of "Error codes", copied from the spec.
    const TABLE: &str = include_str!("../tests/fixtures/error-codes.md");

    /// Each code of the copied table with its exit code.
    fn spec_table() -> Vec<(String, u8)> {
        let mut out = Vec::new();
        for row in TABLE.lines().skip(2) {
            let cells: Vec<&str> = row.split('|').collect();
            let exit: u8 = cells[2].trim().parse().unwrap();
            for code in cells[1].split('`').skip(1).step_by(2) {
                out.push((code.to_owned(), exit));
            }
        }
        out
    }

    #[test]
    fn codes_equal_the_spec_table() {
        let spec = spec_table();
        assert_eq!(spec.len(), CODES.len());
        for (code, exit) in &spec {
            assert_eq!(exit_code(code), Some(*exit), "{code}");
        }
        assert_eq!(exit_code("lock: io"), None);
    }

    fn s() -> String {
        "x".to_owned()
    }

    /// One value of every variant. The `match` below has no wildcard, so a
    /// new variant does not compile until it is listed here.
    fn resolve_errors() -> Vec<Error> {
        let all = vec![
            Error::IndexRead(s()),
            Error::IndexSyntax(s()),
            Error::IndexFormat(s()),
            Error::IndexEntry(s()),
            Error::IndexDuplicate {
                name: s(),
                version: s(),
            },
            Error::IndexHost {
                tool: s(),
                version: s(),
                host: s(),
            },
            Error::PackageFormat(s()),
            Error::PackageManifest(s()),
            Error::NoPackage(s()),
            Error::NoTool {
                name: s(),
                version: s(),
            },
            Error::LockSyntax(s()),
            Error::LockFormat(s()),
            Error::LockUnknownKey(s()),
            Error::LockEntry(s()),
            Error::LockStale(s()),
            Error::LockHash(s()),
            Error::LockMissing(s()),
            Error::LockTool {
                name: s(),
                locked: s(),
                named: s(),
            },
            Error::IoRead {
                path: s(),
                reason: s(),
            },
            Error::IoWrite {
                path: s(),
                reason: s(),
            },
        ];
        for e in &all {
            match e {
                Error::IndexRead(_)
                | Error::IndexSyntax(_)
                | Error::IndexFormat(_)
                | Error::IndexEntry(_)
                | Error::IndexDuplicate { .. }
                | Error::IndexHost { .. }
                | Error::PackageFormat(_)
                | Error::PackageManifest(_)
                | Error::NoPackage(_)
                | Error::NoTool { .. }
                | Error::LockSyntax(_)
                | Error::LockFormat(_)
                | Error::LockUnknownKey(_)
                | Error::LockEntry(_)
                | Error::LockStale(_)
                | Error::LockHash(_)
                | Error::LockMissing(_)
                | Error::LockTool { .. }
                | Error::IoRead { .. }
                | Error::IoWrite { .. } => {}
            }
        }
        all
    }

    fn manifest_errors() -> Vec<ManifestError> {
        let all = vec![
            ManifestError::Syntax {
                line: 1,
                message: s(),
            },
            ManifestError::Format(s()),
            ManifestError::UnknownKey(s()),
            ManifestError::Type {
                key: s(),
                expected: "string",
            },
            ManifestError::Missing(s()),
            ManifestError::Name(s()),
            ManifestError::Version(s()),
            ManifestError::Title(s()),
            ManifestError::Kind(s()),
            ManifestError::Subtype(s()),
            ManifestError::Platform(s()),
            ManifestError::Abi(s()),
            ManifestError::Path {
                key: s(),
                reason: s(),
            },
            ManifestError::Icon(s()),
            ManifestError::Sources(s()),
        ];
        for e in &all {
            match e {
                ManifestError::Syntax { .. }
                | ManifestError::Format(_)
                | ManifestError::UnknownKey(_)
                | ManifestError::Type { .. }
                | ManifestError::Missing(_)
                | ManifestError::Name(_)
                | ManifestError::Version(_)
                | ManifestError::Title(_)
                | ManifestError::Kind(_)
                | ManifestError::Subtype(_)
                | ManifestError::Platform(_)
                | ManifestError::Abi(_)
                | ManifestError::Path { .. }
                | ManifestError::Icon(_)
                | ManifestError::Sources(_) => {}
            }
        }
        all
    }

    fn boot_errors() -> Vec<BootError> {
        let all = vec![
            BootError::Json(s()),
            BootError::Field(s()),
            BootError::NewerFormat(2),
            BootError::UnknownBoard(s()),
            BootError::UnknownMode {
                board: s(),
                mode: s(),
            },
            BootError::PathNotUtf8(s()),
            BootError::OlderFormat(0),
            BootError::BadFormat,
            BootError::UnknownDefaultMode { at: s(), mode: s() },
            BootError::UnknownPlaceholder { at: s(), name: s() },
            BootError::PlaceholderInName(s()),
            BootError::MissingCard {
                board: s(),
                placeholder: s(),
            },
        ];
        for e in &all {
            match e {
                BootError::Json(_)
                | BootError::Field(_)
                | BootError::NewerFormat(_)
                | BootError::UnknownBoard(_)
                | BootError::UnknownMode { .. }
                | BootError::PathNotUtf8(_)
                | BootError::OlderFormat(_)
                | BootError::BadFormat
                | BootError::UnknownDefaultMode { .. }
                | BootError::UnknownPlaceholder { .. }
                | BootError::PlaceholderInName(_)
                | BootError::MissingCard { .. } => {}
            }
        }
        all
    }

    fn check(e: &dyn ErrorCode, shown: String, spec: &[(String, u8)]) {
        let code = e.error_code();
        let row = spec.iter().find(|(c, _)| c == code);
        assert!(row.is_some(), "{code} is not in the spec's table");
        assert_eq!(Some(e.exit_code()), row.map(|(_, n)| *n), "{code}");
        assert!(!shown.is_empty());
    }

    #[test]
    fn every_error_has_a_code_of_the_table() {
        let spec = spec_table();
        for e in resolve_errors() {
            check(&e, e.to_string(), &spec);
            // The message of a resolve error starts with its code.
            assert!(e.to_string().starts_with(e.code()), "{e}");
        }
        for e in manifest_errors() {
            check(&e, e.to_string(), &spec);
            assert!(e.to_string().starts_with(e.error_code()), "{e}");
        }
        for e in boot_errors() {
            check(&e, e.to_string(), &spec);
        }
    }
}
