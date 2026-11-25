// SPDX-License-Identifier: GPL-3.0-or-later

//! Error codes and the exit code of each.
//!
//! Implements RobCo-Core `docs/spec/STUDIO_CLI.md`, "Exit codes" and
//! "Error codes", as read at RobCo-Core commit `223e8cd`. Every error of
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

