// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// The spec's example manifest, copied byte for byte.
const EXAMPLE: &[u8] = include_bytes!("../../tests/manifest/example.toml");

/// A valid manifest with no `[build]`, for fixtures to change one line of.
const BASE: &str = "format = 1

[project]
name = \"PIPES\"
version = \"1.0.0\"
kind = \"holotape\"
subtype = \"game\"

[target]
platform = \"pipos\"
abi = \"1.3\"
";

fn with(from: &str, to: &str) -> String {
    assert!(BASE.contains(from), "{from}");
    BASE.replace(from, to)
}

fn err(text: &str) -> ManifestError {
    match parse(text.as_bytes()) {
        Ok(m) => panic!("parsed: {m:?}"),
        Err(e) => e,
    }
}

fn check(e: ManifestError, code: &str, detail: &str) {
    assert_eq!(e.code(), code, "{e}");
    assert_eq!(e.detail(), detail, "{e}");
    assert_eq!(e.to_string(), format!("manifest: {code}: {detail}"));
}

#[test]
fn example_parses() {
    let m = parse(EXAMPLE).unwrap();
    assert_eq!(m.format, 1);
    assert_eq!(m.project.name, "PIPES");
    assert_eq!(m.project.title, "Pipes");
    assert_eq!(m.project.version.image_version(), 0x0001_0000);
    assert_eq!(m.project.kind, Kind::Holotape);
    assert_eq!(m.project.subtype, Subtype::Game);
    assert_eq!(m.project.icon.as_deref(), Some("icon.pbm"));
    assert_eq!(m.target.platform, Platform::Pipos);
    assert_eq!(m.target.abi.abi_version(), 0x0001_0003);
    assert_eq!(m.build.backend, Backend::Robco);
    assert_eq!(m.build.sources, ["src/*.c"]);
}

#[test]
fn defaults_fill_in() {
    let m = parse(BASE.as_bytes()).unwrap();
    assert_eq!(m.project.title, "");
    assert_eq!(m.project.icon, None);
    assert_eq!(m.build.backend, Backend::Robco);
    assert_eq!(m.build.sources, ["src/*.c"]);
    let m = parse(with("subtype = \"game\"", "subtype = \"utility\"").as_bytes()).unwrap();
    assert_eq!(m.project.subtype.value(), 1);
}

#[test]
fn leading_zeros_are_ignored() {
    let m = parse(with("\"1.0.0\"", "\"1.02.3\"").as_bytes()).unwrap();
    assert_eq!(
        m.project.version,
        Version {
            major: 1,
            minor: 2,
            patch: 3
        }
    );
    assert_eq!(
        parse_version("0065535.0255.000").unwrap().image_version(),
        0xFFFF_FF00
    );
    assert_eq!(parse_abi("01.0003").unwrap().abi_version(), 0x0001_0003);
}

// One fixture per code of the spec's "Errors" table.

#[test]
fn code_syntax() {
    check(
        err(&with("[target]", "[target")),
        "syntax",
        "line 9: unclosed table, expected `]`",
    );
    check(
        parse(b"format = 1\n\xff").unwrap_err(),
        "syntax",
        "line 2: not UTF-8",
    );
}

#[test]
fn code_syntax_duplicate_key() {
    let e = err(&with(
        "kind = \"holotape\"",
        "kind = \"holotape\"\nkind = \"holotape\"",
    ));
    assert_eq!(e.code(), "syntax");
    assert!(e.detail().starts_with("line 7: duplicate key"), "{e}");
}

#[test]
fn code_format() {
    check(err(&with("format = 1\n", "")), "format", "missing");
    check(
        err(&with("format = 1", "format = 0")),
        "format",
        "0 is not a positive integer",
    );
    check(
        err(&with("format = 1", "format = \"1\"")),
        "format",
        "not a positive integer",
    );
    check(
        err(&with("format = 1", "format = 2")),
        "format",
        "2 is newer than this tool reads, 1: install a newer tool",
    );
}

#[test]
fn code_unknown_key() {
    check(
        err(&with(
            "subtype = \"game\"",
            "subtype = \"game\"\ntitel = \"x\"",
        )),
        "unknown key",
        "project.titel",
    );
    check(err(&format!("{BASE}[extra]\n")), "unknown key", "extra");
}

#[test]
fn reserved_keys_are_unknown() {
    check(
        err(&format!("{BASE}[build]\nruntime = \"reel\"\n")),
        "unknown key",
        "build.runtime",
    );
    check(
        err(&format!("{BASE}[[asset]]\nname = \"a\"\n")),
        "unknown key",
        "asset",
    );
    check(
        err(&format!("{BASE}[build]\nbackend = \"make\"\n")),
        "unknown key",
        "build.backend = \"make\"",
    );
}

#[test]
fn code_type() {
    check(
        err(&with("name = \"PIPES\"", "name = 7")),
        "type",
        "project.name: expected string",
    );
    check(
        err(&with("format = 1", "format = 1\nbuild = 1")),
        "type",
        "build: expected table",
    );
    check(
        err(&format!("{BASE}[build]\nsources = [1]\n")),
        "type",
        "build.sources[0]: expected string",
    );
}

#[test]
fn code_missing() {
    check(
        err(&with("version = \"1.0.0\"\n", "")),
        "missing",
        "project.version",
    );
    check(
        err(&with("[target]\nplatform = \"pipos\"\nabi = \"1.3\"\n", "")),
        "missing",
        "target",
    );
}

#[test]
fn code_name() {
    check(
        err(&with("\"PIPES\"", "\"pipes\"")),
        "name",
        "\"pipes\": 'p' is not one of A to Z, 0 to 9, _ and -",
    );
    check(
        err(&with("\"PIPES\"", "\"PIPES~1\"")),
        "name",
        "\"PIPES~1\": '~' is not one of A to Z, 0 to 9, _ and -",
    );
    check(
        err(&with("\"PIPES\"", "\"ABCDEFGHI\"")),
        "name",
        "\"ABCDEFGHI\": must be 1 to 8 characters long",
    );
}

#[test]
fn code_version() {
    for bad in [
        "1.0",
        "1.0.0.0",
        "+1.0.0",
        "1.0.256",
        "65536.0.0",
        "1. 0.0",
        "1..0",
        "-1.0.0",
    ] {
        check(
            err(&with("\"1.0.0\"", &format!("\"{bad}\""))),
            "version",
            &format!("\"{bad}\": not X.Y.Z with X to 65535, Y and Z to 255"),
        );
    }
}

#[test]
fn code_title() {
    check(
        err(&with("[target]", "title = \"Pip\u{e9}\"\n[target]")),
        "title",
        "not ASCII",
    );
    check(
        err(&with("[target]", "title = \"a\\u0000\"\n[target]")),
        "title",
        "holds a NUL",
    );
    let long = "x".repeat(65);
    check(
        err(&with("[target]", &format!("title = \"{long}\"\n[target]"))),
        "title",
        "65 bytes, longer than 64",
    );
    let max = "x".repeat(64);
    assert!(parse(with("[target]", &format!("title = \"{max}\"\n[target]")).as_bytes()).is_ok());
}

#[test]
fn code_kind() {
    check(
        err(&with("\"holotape\"", "\"rom\"")),
        "kind",
        "rom: reserved, not built by format 1",
    );
    check(
        err(&with("\"holotape\"", "\"tape\"")),
        "kind",
        "tape: unknown kind",
    );
}

#[test]
fn code_subtype() {
    check(
        err(&with("subtype = \"game\"\n", "")),
        "subtype",
        "missing for a holotape",
    );
    check(
        err(&with("\"game\"", "\"demo\"")),
        "subtype",
        "demo: not game or utility",
    );
}

#[test]
fn code_platform() {
    check(err(&with("\"pipos\"", "\"uos\"")), "platform", "uos");
}

#[test]
fn code_abi() {
    check(
        err(&with("\"1.3\"", "\"2.0\"")),
        "abi",
        "\"2.0\": major 2, pipos needs 1",
    );
    check(
        err(&with("\"1.3\"", "\"1.65536\"")),
        "abi",
        "\"1.65536\": not MAJOR.MINOR, each 0 to 65535",
    );
    check(
        err(&with("\"1.3\"", "\"1.3.0\"")),
        "abi",
        "\"1.3.0\": not MAJOR.MINOR, each 0 to 65535",
    );
}

#[test]
fn code_path() {
    let cases = [
        ("/icon.pbm", "starts with /"),
        ("C:icon.pbm", "starts with a drive letter"),
        ("art\\\\icon.pbm", "uses \\, the separator is /"),
        ("art//icon.pbm", "has an empty component"),
        ("./icon.pbm", "has a . component"),
        ("../icon.pbm", "has a .. component"),
        ("art/", "has an empty component"),
        (
            "*.pbm",
            "* is allowed only in the last component of a source pattern",
        ),
    ];
    for (path, reason) in cases {
        let e = err(&with("[target]", &format!("icon = \"{path}\"\n[target]")));
        assert_eq!(e.code(), "path", "{e}");
        assert!(e.detail().starts_with("project.icon: "), "{e}");
        assert!(e.detail().ends_with(reason), "{e}");
    }
    check(
        err(&format!(
            "{BASE}[build]\nsources = [\"src/*.c\", \"*/main.c\"]\n"
        )),
        "path",
        "build.sources[1]: \"*/main.c\": * is allowed only in the last component of a source pattern",
    );
}

#[test]
fn code_icon() {
    check(
        read_icon("icon.pbm", None).unwrap_err(),
        "icon",
        "icon.pbm: missing",
    );
    check(
        read_icon("icon.pbm", Some(b"P1\n2 2\n0 1 1 0\n")).unwrap_err(),
        "icon",
        "icon.pbm: 15 bytes, so read as PBM: icon is 2x2, expected 32x32",
    );
    let raw = [0x5au8; ICON_SIZE];
    assert_eq!(read_icon("i", Some(&raw)).unwrap(), raw);
    let mut p1 = b"P1\n# c\n32 32\n".to_vec();
    p1.extend(b"1".repeat(1024));
    assert_eq!(read_icon("i", Some(&p1)).unwrap(), [0xff; ICON_SIZE]);
    let mut p4 = b"P4 32 32\n".to_vec();
    p4.extend([0x81u8; ICON_SIZE]);
    assert_eq!(read_icon("i", Some(&p4)).unwrap(), [0x81; ICON_SIZE]);
}

#[test]
fn code_sources() {
    let patterns = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    check(
        expand_sources(&patterns(&["lib/*.c"]), &["src/main.c"]).unwrap_err(),
        "sources",
        "lib/*.c: matches nothing",
    );
    check(
        expand_sources(&patterns(&["src/*"]), &["src/main.c", "src/notes.txt"]).unwrap_err(),
        "sources",
        "src/*: matches src/notes.txt, which is not a .c file",
    );
    check(
        err(&format!("{BASE}[build]\nsources = []\n")),
        "sources",
        "0 patterns, must be 1 to 256",
    );
    let many = vec!["\"a.c\""; 257].join(", ");
    check(
        err(&format!("{BASE}[build]\nsources = [{many}]\n")),
        "sources",
        "257 patterns, must be 1 to 256",
    );
}

#[test]
fn sources_expand_and_drop_repeats() {
    let patterns: Vec<String> = ["src/*.c", "src/main.c", "x*y*z.c"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let files = [
        "src/main.c",
        "src/game.c",
        "src/sub/deep.c",
        "xayz.c",
        "xyz.c",
        "xz.c",
        "main.c",
    ];
    assert_eq!(
        expand_sources(&patterns, &files).unwrap(),
        ["src/game.c", "src/main.c", "xayz.c", "xyz.c"]
    );
}

#[test]
fn source_order_is_byte_order() {
    let mut s: Vec<String> = ["main.c", "game.c", "logic.c", "draw.c", "Z.c", "main.c"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    order_sources(&mut s);
    assert_eq!(s, ["Z.c", "draw.c", "game.c", "logic.c", "main.c"]);
}
