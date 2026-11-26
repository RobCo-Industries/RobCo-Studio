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
    for c in ["\\t", "\\n", "\\u001F", "\\u007F"] {
        check(
            err(&with("[target]", &format!("title = \"a{c}\"\n[target]"))),
            "title",
            "holds a control character",
        );
    }
    assert!(parse(with("[target]", "title = \"a ~\"\n[target]").as_bytes()).is_ok());
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
        "build.sources holds 0 patterns, 1 to 256 allowed",
    );
    let many = vec!["\"a.c\""; 257].join(", ");
    check(
        err(&format!("{BASE}[build]\nsources = [{many}]\n")),
        "sources",
        "build.sources holds 257 patterns, 1 to 256 allowed",
    );
}

fn strings(s: &[&str]) -> Vec<String> {
    s.iter().map(|s| s.to_string()).collect()
}

const CULVERT_FILES: &[&str] = &["draw.c", "game.c", "logic.c", "main.c", "Makefile"];
const SRC_FILES: &[&str] = &[
    "src/main.c",
    "src/game.c",
    "src/draw.c",
    "src/sub/deep.c",
    "main.c",
];

#[test]
fn source_order_keeps_an_explicit_list_as_written() {
    let list = ["main.c", "game.c", "logic.c", "draw.c"];
    assert_eq!(
        expand_sources(&strings(&list), CULVERT_FILES).unwrap(),
        list
    );
}

#[test]
fn source_order_sorts_a_patterns_matches() {
    assert_eq!(
        expand_sources(&strings(&["src/*.c"]), SRC_FILES).unwrap(),
        ["src/draw.c", "src/game.c", "src/main.c"]
    );
}

#[test]
fn source_order_mixes_names_and_patterns() {
    assert_eq!(
        expand_sources(&strings(&["src/main.c", "src/*.c"]), SRC_FILES).unwrap(),
        ["src/main.c", "src/draw.c", "src/game.c"]
    );
}

#[test]
fn source_order_keeps_a_repeat_at_its_first_place() {
    assert_eq!(
        expand_sources(
            &strings(&["game.c", "main.c", "game.c", "*.c"]),
            CULVERT_FILES
        )
        .unwrap(),
        ["game.c", "main.c", "draw.c", "logic.c"]
    );
}

#[test]
fn order_sources_is_the_rule() {
    let groups = vec![strings(&["b.c"]), strings(&["z.c", "a.c", "b.c"])];
    assert_eq!(order_sources(groups), ["b.c", "a.c", "z.c"]);
}

#[test]
fn source_names_compare_by_byte() {
    check(
        expand_sources(&strings(&["MAIN.c"]), CULVERT_FILES).unwrap_err(),
        "sources",
        "MAIN.c: matches nothing",
    );
}

#[test]
fn pattern_star_matches_runs() {
    let files = ["xayz.c", "xyz.c", "xz.c", "sub/xyz.c"];
    assert_eq!(
        expand_sources(&strings(&["x*y*z.c"]), &files).unwrap(),
        ["xayz.c", "xyz.c"]
    );
}

/// The spec's second example, Culvert, copied byte for byte.
#[test]
fn culvert_example_keeps_its_makefile_order() {
    let m = parse(include_bytes!("../../tests/manifest/culvert.toml")).unwrap();
    assert_eq!(m.project.name, "CULVERT");
    assert_eq!(m.target.abi.abi_version(), 0x0001_0004);
    assert_eq!(
        expand_sources(&m.build.sources, CULVERT_FILES).unwrap(),
        ["main.c", "game.c", "logic.c", "draw.c"]
    );
}

/// The manifest is TOML 1.1: an inline table over several lines, which
/// TOML 1.0 refuses, parses. This pins the behaviour of the `toml` crate.
#[test]
fn toml_1_1_inline_table_over_lines_parses() {
    let text = "format = 1\nproject = {\n  name = \"PIPES\",\n  version = \"1.0.0\",\n  kind = \"holotape\",\n  subtype = \"game\",\n}\ntarget = { platform = \"pipos\", abi = \"1.3\" }\n";
    assert_eq!(parse(text.as_bytes()).unwrap().project.name, "PIPES");
}

/// Step 4 reports the first unknown key in the order of the file, not in
/// the order of the names.
#[test]
fn unknown_keys_come_in_file_order() {
    let text = BASE
        .replace("[project]\n", "[project]\nzeta = 1\n")
        .replace("[target]\n", "[target]\nalpha = 1\n");
    check(err(&text), "unknown key", "project.zeta");
    let text = format!("zz = 1\n{BASE}[aa]\n");
    check(err(&text), "unknown key", "zz");
    let text = format!("{BASE}[build]\nbackend = \"make\"\nruntime = \"reel\"\n");
    check(err(&text), "unknown key", "build.backend = \"make\"");
    let text = format!("{BASE}[build]\nruntime = \"reel\"\nbackend = \"make\"\n");
    check(err(&text), "unknown key", "build.runtime");
}

/// A `build.backend` other than `robco` is step 4: it comes before every
/// value of step 5, and its value is written as a TOML basic string.
#[test]
fn backend_fails_before_values() {
    let text = format!(
        "{}[build]\nbackend = \"Ro\\\"bco\"\n",
        with("name = \"PIPES\"", "name = \"pipes\"")
    );
    check(err(&text), "unknown key", "build.backend = \"Ro\\\"bco\"");
    check(
        err(&format!("{BASE}[build]\nbackend = 1\n")),
        "type",
        "build.backend: expected string",
    );
}

/// `format` may stand anywhere above the first header, as the TOML 1.1
/// fixture shows; below one it belongs to that table, and the file has no
/// `format`. A later format is reported before unknown keys and values.
#[test]
fn format_anywhere_at_the_top() {
    let text = BASE.replace("format = 1\n", "");
    check(
        err(&format!("{text}[build]\nformat = 1\n")),
        "format",
        "missing",
    );
    check(
        err(&format!(
            "format = 2\nwhat = 1\n{}",
            text.replace("PIPES", "pipes")
        )),
        "format",
        "2 is newer than this tool reads, 1: install a newer tool",
    );
}

/// The spec's TOML 1.1 manifest means the first manifest of its Examples,
/// and each of the four forms TOML 1.1 adds is read.
#[test]
fn toml_1_1_forms() {
    let first = parse(EXAMPLE).unwrap();
    let new = parse(include_bytes!("../../tests/manifest/toml11.toml")).unwrap();
    assert_eq!(new, first);
    // \e parses, then the title fails its own rule.
    let text = BASE.replace("name = \"PIPES\"", "name = \"PIPES\"\ntitle = \"\\e\"");
    check(err(&text), "title", "holds a control character");
    // A time without seconds parses and then fails as a type.
    let text = BASE.replace("abi = \"1.3\"", "abi = 08:30");
    check(err(&text), "type", "target.abi: expected string");
}

/// The error codes, as the table of `STUDIO_CLI.md` spells them.
#[test]
fn error_codes_are_spelled_as_the_table() {
    use crate::codes::ErrorCode;
    let e = err(&with("format = 1", "format = 2"));
    assert_eq!(e.error_code(), "manifest: format");
    assert_eq!(e.exit_code(), 3);
}

/// A folder of the test's own under the system's temp folder, removed on
/// drop. On macOS the temp folder is reached through a link, so every test
/// here also checks a root reached through a link.
#[cfg(unix)]
struct LinkDir(std::path::PathBuf);

#[cfg(unix)]
impl LinkDir {
    fn new(name: &str) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "studio-manifest-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("root/src")).unwrap();
        std::fs::write(dir.join("root/src/real.c"), "int x;").unwrap();
        std::fs::write(dir.join("away.c"), "int y;").unwrap();
        Self(dir)
    }
    fn root(&self) -> std::path::PathBuf {
        self.0.join("root")
    }
    fn link(&self, target: &str, at: &str) {
        std::os::unix::fs::symlink(target, self.root().join(at)).unwrap();
    }
}

#[cfg(unix)]
impl Drop for LinkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn path_reason(r: Result<OnDisk, ManifestError>) -> String {
    match r {
        Err(e @ ManifestError::Path { .. }) => e.detail(),
        other => panic!("not a path error: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn inside_root_keeps_a_link_inside_the_root() {
    let d = LinkDir::new("inside");
    d.link("real.c", "src/main.c");
    d.link("main.c", "src/twice.c");
    d.link("src", "lib");
    let root = d.root();
    for rel in [
        "src/real.c",
        "src/main.c",
        "src/twice.c",
        "lib",
        "lib/real.c",
    ] {
        let got = inside_root(&root, "build.sources", rel).unwrap();
        assert_eq!(got, OnDisk::Inside(root.join(rel)), "{rel}");
    }
}

#[cfg(unix)]
#[test]
fn inside_root_refuses_a_link_out_of_the_root() {
    let d = LinkDir::new("outside");
    d.link("../../away.c", "src/main.c");
    d.link("main.c", "src/twice.c");
    d.link("..", "up");
    let root = d.root();
    for rel in ["src/main.c", "src/twice.c", "up", "up/away.c"] {
        let why = path_reason(inside_root(&root, "project.icon", rel));
        assert!(why.starts_with("project.icon: "), "{why}");
    }
}

#[cfg(unix)]
#[test]
fn inside_root_gives_none_for_a_dangling_link() {
    let d = LinkDir::new("dangling");
    d.link("gone.c", "src/main.c");
    d.link("../../gone.c", "src/far.c");
    let root = d.root();
    for rel in ["src/main.c", "src/far.c", "src/missing.c", "src/real.c/x"] {
        assert_eq!(
            inside_root(&root, "k", rel).unwrap(),
            OnDisk::Missing,
            "{rel}"
        );
    }
}

#[cfg(unix)]
#[test]
fn inside_root_accepts_a_root_reached_through_a_link() {
    let d = LinkDir::new("rootlink");
    std::os::unix::fs::symlink("root", d.0.join("alias")).unwrap();
    let alias = d.0.join("alias");
    assert_eq!(
        inside_root(&alias, "k", "src/real.c").unwrap(),
        OnDisk::Inside(alias.join("src/real.c"))
    );
}

#[cfg(unix)]
#[test]
fn inside_root_refuses_a_link_loop_as_path() {
    let d = LinkDir::new("loop");
    d.link("b.c", "src/a.c");
    d.link("a.c", "src/b.c");
    let why = path_reason(inside_root(&d.root(), "build.sources", "src/a.c"));
    assert_eq!(why, "build.sources: src/a.c: a symbolic link loop");
}

#[cfg(unix)]
#[test]
fn inside_root_gives_unreadable_for_a_folder_without_search() {
    use std::os::unix::fs::PermissionsExt;
    let d = LinkDir::new("nosearch");
    let root = d.root();
    let src = root.join("src");
    std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o600)).unwrap();
    let readable = std::fs::metadata(src.join("real.c")).is_ok();
    let got = inside_root(&root, "build.sources", "src/real.c");
    std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        println!("skipped: this process can search a folder without search permission");
        return;
    }
    assert_eq!(
        got.unwrap(),
        OnDisk::Unreadable {
            path: root.join("src/real.c"),
            kind: std::io::ErrorKind::PermissionDenied,
        }
    );
}

#[test]
fn a_link_loop_error_is_known_on_each_system() {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let code = 40;
    #[cfg(windows)]
    let code = 1921;
    #[cfg(not(any(target_os = "linux", target_os = "android", windows)))]
    let code = 62;
    assert!(is_loop(&std::io::Error::from_raw_os_error(code)));
    assert!(!is_loop(&std::io::Error::from_raw_os_error(2)));
}
