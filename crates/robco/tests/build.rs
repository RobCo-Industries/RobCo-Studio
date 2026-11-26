// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs `robco build` as a program, against a local index and a cache in a
//! temporary folder. No test touches the network.
//!
//! The tests of the steps before the compile need no compiler. The full
//! build needs a real `pipos` package and a real compiler archive, which
//! this repository does not hold. It reads them from the environment:
//!
//! - `ROBCO_TEST_PIPOS`: the path of a `pipos` package zip.
//! - `ROBCO_TEST_GCC`: the path of an `arm-none-eabi-gcc` archive for the
//!   running host, of the version the package names.
//! - `ROBCO_TEST_GCC_ROOT`: the folder inside that archive that holds
//!   `bin/`, empty when `bin/` is at the top.
//! - `ROBCO_TEST_TAPE_OUT`: when set, the test copies the tape there, so a
//!   CI job can check it with `rimg.py verify` and compare its SHA-256
//!   across systems.
//! - `ROBCO_TEST_REQUIRE_BUILD`: when set to 1, a missing variable fails
//!   the full build test in place of skipping it. CI sets it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test file; a failed unwrap is a failed test"
)]

use std::fs;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use studio_core::install::sha256_hex;
use zip::write::SimpleFileOptions;

fn host() -> &'static str {
    studio_core::resolve::running_host().expect("a host the index names")
}

const MANIFEST: &str = r#"format = 1

[project]
name = "TINY"
title = "Tiny tape"
version = "1.2.3"
kind = "holotape"
subtype = "utility"

[target]
platform = "pipos"
abi = "1.4"

[build]
sources = ["src/*.c"]
"#;

const MAIN_C: &str = r#"#include <pipos/pip.h>

int pip_main(const pipos_startup_t *startup)
{
    (void)startup;
    pip_printf("tiny: ready\n");
    return 0;
}
"#;

/// A tool zip with `bin/` and nothing real in it.
fn empty_tool_zip() -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.add_directory("bin/", SimpleFileOptions::default())
        .unwrap();
    z.finish().unwrap().into_inner()
}

/// A test folder: `index.json` and its archives, `cache/` and the project
/// `proj/`. Removed on drop.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "robco-build-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("archives")).unwrap();
        fs::create_dir_all(dir.join("proj/src")).unwrap();
        let f = Self { dir };
        f.put("proj/robco.toml", MANIFEST.as_bytes());
        f.put("proj/src/main.c", MAIN_C.as_bytes());
        f
    }

    fn put(&self, rel: &str, bytes: &[u8]) {
        let p = self.dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    fn proj(&self) -> PathBuf {
        self.dir.join("proj")
    }

    fn cache(&self) -> PathBuf {
        self.dir.join("cache")
    }

    /// Writes an index that lists the package zip `pkg`, the gcc archive
    /// `gcc` with its `root`, and an empty `qemu` archive: `robco build`
    /// never installs `qemu`, but a resolution needs its entry.
    fn index(&self, pkg: &[u8], gcc: &[u8], gcc_root: &str) {
        let sdk = sdk_json_of(pkg);
        let tool = |n: usize| sdk["tools"][n]["version"].as_str().unwrap().to_owned();
        let qemu = empty_tool_zip();
        self.put("archives/pipos.zip", pkg);
        self.put("archives/gcc.archive", gcc);
        self.put("archives/qemu.zip", &qemu);
        let archive = |url: &str, b: &[u8], root: &str| json!({host(): {"url": url, "size": b.len(), "sha256": sha256_hex(b), "root": root}});
        let index = json!({
            "format": 1,
            "packages": [{
                "name": sdk["name"],
                "version": sdk["version"],
                "abi": sdk["abi"],
                "requires": sdk["requires"],
                "tools": sdk["tools"],
                "formats": sdk["formats"],
                "url": "archives/pipos.zip",
                "size": pkg.len(),
                "sha256": sha256_hex(pkg)
            }],
            "tools": [
                {"name": "gcc", "version": tool(0), "archives": archive("archives/gcc.archive", gcc, gcc_root)},
                {"name": "qemu", "version": tool(1), "archives": archive("archives/qemu.zip", &qemu, "")}
            ]
        });
        self.put("index.json", index.to_string().as_bytes());
    }

    /// Runs `robco` in the project with only the variables it needs.
    fn robco(&self, args: &[&str]) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_robco"));
        c.args(args)
            .current_dir(self.proj())
            .env_clear()
            .env("ROBCO_CACHE", self.cache());
        // Windows needs its system root, and gcc there its temporary folder.
        for key in ["SYSTEMROOT", "TEMP", "TMP"] {
            if let Some(v) = std::env::var_os(key) {
                c.env(key, v);
            }
        }
        c.output().expect("robco runs")
    }

    fn tape(&self) -> PathBuf {
        self.proj().join("build/TINY.HTP")
    }

    /// Leaves an older tape behind, as an earlier build did.
    fn older_tape(&self) {
        self.put("proj/build/TINY.HTP", b"an older tape");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn sdk_json_of(pkg: &[u8]) -> Value {
    let mut z = zip::ZipArchive::new(Cursor::new(pkg)).unwrap();
    let mut s = String::new();
    z.by_name("sdk.json")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    serde_json::from_str(&s).unwrap()
}

fn lines(o: &Output) -> Vec<Value> {
    String::from_utf8(o.stdout.clone())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// The `error` line's code and the exit code.
fn failure(o: &Output) -> (String, i32) {
    let l = lines(o);
    let err = l
        .iter()
        .find(|v| v["kind"] == "error")
        .unwrap_or_else(|| panic!("no error line: {l:?}"));
    (
        err["code"].as_str().unwrap().to_owned(),
        o.status.code().unwrap(),
    )
}

#[test]
fn build_without_a_manifest_is_project_none() {
    let f = Fixture::new("none");
    fs::remove_file(f.proj().join("robco.toml")).unwrap();
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("project: none".to_owned(), 3));
}

#[test]
fn build_with_a_bad_manifest_deletes_nothing() {
    let f = Fixture::new("bad-manifest");
    f.older_tape();
    f.put(
        "proj/robco.toml",
        MANIFEST.replace("1.2.3", "1.2").as_bytes(),
    );
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("manifest: version".to_owned(), 3));
    // Step 2 failed: the manifest gave no NAME, so nothing is deleted.
    assert!(f.tape().is_file());
}

#[test]
fn build_that_fails_deletes_an_older_tape() {
    let f = Fixture::new("fails");
    f.older_tape();
    // No lock and no --index: the resolution at step 6 fails.
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("index: location".to_owned(), 4));
    assert!(!f.tape().exists());
    assert!(!f.proj().join("robco.lock").exists());
}

/// A lock of `pipos` 1.3.0 for a manifest that asks for ABI 1.4.
const STALE_LOCK: &str = "format = 1
index = \"index.json\"

[[package]]
name = \"pipos\"
version = \"1.3.0\"
sha256 = \"0000000000000000000000000000000000000000000000000000000000000001\"

[[tool]]
name = \"gcc\"
version = \"15.2.1-1.1\"

[tool.archives]
linux-x64 = \"0000000000000000000000000000000000000000000000000000000000000002\"
macos-arm64 = \"0000000000000000000000000000000000000000000000000000000000000002\"
macos-x64 = \"0000000000000000000000000000000000000000000000000000000000000002\"
windows-x64 = \"0000000000000000000000000000000000000000000000000000000000000002\"

[[tool]]
name = \"qemu\"
version = \"10.1.2\"

[tool.archives]
linux-x64 = \"0000000000000000000000000000000000000000000000000000000000000003\"
macos-arm64 = \"0000000000000000000000000000000000000000000000000000000000000003\"
macos-x64 = \"0000000000000000000000000000000000000000000000000000000000000003\"
windows-x64 = \"0000000000000000000000000000000000000000000000000000000000000003\"
";

#[test]
fn a_stale_lock_fails_and_changes_nothing() {
    let f = Fixture::new("stale");
    f.older_tape();
    f.put("proj/robco.lock", STALE_LOCK.as_bytes());
    let o = f.robco(&["build", "--index", "../index.json", "--json"]);
    assert_eq!(failure(&o), ("lock: stale".to_owned(), 3));
    // The lock is as it was, the cache is untouched, and nothing but the
    // deleted tape changed under build/.
    assert_eq!(
        fs::read_to_string(f.proj().join("robco.lock")).unwrap(),
        STALE_LOCK
    );
    assert!(!f.cache().exists());
    assert!(!f.tape().exists());
    assert_eq!(fs::read_dir(f.proj().join("build")).unwrap().count(), 0);
    // No step line: nothing was resolved, downloaded or compiled.
    assert!(lines(&o).iter().all(|v| v["kind"] != "step"));
}

#[test]
fn a_missing_compiler_is_tool_program() {
    // A gcc archive with an empty bin/: the build reads flags.json, then
    // fails to find the programs. Needs a real package for flags.json.
    let Some(pkg) = std::env::var_os("ROBCO_TEST_PIPOS") else {
        return skip("a_missing_compiler_is_tool_program");
    };
    let f = Fixture::new("no-program");
    f.older_tape();
    f.index(&fs::read(pkg).unwrap(), &empty_tool_zip(), "");
    let o = f.robco(&["build", "--index", "../index.json", "--json"]);
    assert_eq!(failure(&o), ("tool: program".to_owned(), 4));
    assert!(!f.tape().exists());
}

fn skip(name: &str) {
    assert!(
        std::env::var("ROBCO_TEST_REQUIRE_BUILD").as_deref() != Ok("1"),
        "{name}: ROBCO_TEST_REQUIRE_BUILD is 1 and ROBCO_TEST_PIPOS or ROBCO_TEST_GCC is not set"
    );
    eprintln!("{name}: skipped, no ROBCO_TEST_PIPOS or ROBCO_TEST_GCC");
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// Checks the header against the table of "What a holotape build
/// produces", reading the bytes itself.
fn check_header(tape: &[u8]) {
    // The header starts after the 32-byte branch stub.
    const H: usize = 0x20;
    assert_eq!(&tape[H..H + 8], b"ROBCO78\0", "magic");
    assert_eq!(u32_at(tape, H + 12), 1, "flags: the META block");
    assert_eq!(u32_at(tape, H + 16), 0x0080_0000, "load_addr");
    assert_eq!(u32_at(tape, H + 20) as usize, tape.len(), "file_size");
    let mem = u32_at(tape, H + 24) as usize;
    assert!(mem >= tape.len(), "mem_size {mem} holds the file");
    assert_eq!(
        u32_at(tape, H + 32),
        0x0001_0004,
        "abi_version of target.abi"
    );
    assert_eq!(
        u32_at(tape, H + 36),
        (1 << 16) | (2 << 8) | 3,
        "image_version"
    );
    assert_eq!(u32_at(tape, H + 40), 1, "subtype of utility");
    let mut name = [0u8; 16];
    name[..4].copy_from_slice(b"TINY");
    assert_eq!(&tape[H + 44..H + 60], &name, "name");
    let mut title = [0u8; 64];
    title[..9].copy_from_slice(b"Tiny tape");
    assert_eq!(&tape[0x60..0xA0], &title, "META title");
    let mut zeroed = tape.to_vec();
    zeroed[H + 28..H + 32].fill(0);
    assert_eq!(
        u32_at(tape, H + 28),
        studio_sys::crc32(&zeroed),
        "crc32 over the file with the field as zero"
    );
}

fn steps(o: &Output) -> Vec<String> {
    lines(o)
        .iter()
        .filter(|v| v["kind"] == "step")
        .map(|v| format!("{} {} {}", v["step"], v["item"], v["state"]).replace('"', ""))
        .collect()
}

#[test]
fn build_a_tape_from_a_fixture_index() {
    let (Some(pkg), Some(gcc)) = (
        std::env::var_os("ROBCO_TEST_PIPOS"),
        std::env::var_os("ROBCO_TEST_GCC"),
    ) else {
        return skip("build_a_tape_from_a_fixture_index");
    };
    let root = std::env::var("ROBCO_TEST_GCC_ROOT").unwrap_or_default();
    let pkg = fs::read(pkg).unwrap();
    let sdk = sdk_json_of(&pkg);
    let gcc_version = sdk["tools"][0]["version"].as_str().unwrap().to_owned();
    let f = Fixture::new("full");
    f.index(&pkg, &fs::read(gcc).unwrap(), &root);
    f.older_tape();

    // The first build: resolve, install, write the lock, build.
    let o = f.robco(&["build", "--index", "../index.json", "--json"]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    assert!(o.stderr.is_empty());
    let pipos = format!("pipos@{}", sdk["version"].as_str().unwrap());
    let gcc_item = format!("gcc@{gcc_version}/{}", host());
    let mut want = vec!["resolve  start".to_owned(), "resolve  done".to_owned()];
    for item in [&pipos, &gcc_item] {
        for s in ["download", "verify", "install"] {
            want.push(format!("{s} {item} start"));
            want.push(format!("{s} {item} done"));
        }
    }
    let built = [
        "compile crt0 start",
        "compile crt0 done",
        "compile src/main.c start",
        "compile src/main.c done",
        "link  start",
        "link  done",
        "objcopy  start",
        "objcopy  done",
        "seal  start",
        "seal  done",
    ];
    want.extend(built.iter().map(|s| (*s).to_owned()));
    assert_eq!(steps(&o), want);

    let tape = fs::read(f.tape()).unwrap();
    check_header(&tape);
    let result = lines(&o).pop().unwrap();
    assert_eq!(
        result,
        json!({
            "v": 1, "kind": "result", "command": "build", "ok": true, "exit": 0,
            "tape": "build/TINY.HTP", "elf": "build/TINY.elf",
            "size": tape.len(), "sha256": sha256_hex(&tape),
            "abi": "1.4", "gcc": gcc_version, "sources": ["src/main.c"]
        })
    );
    assert!(f.proj().join("build/TINY.elf").is_file());
    let lock = fs::read(f.proj().join("robco.lock")).unwrap();
    if let Some(out) = std::env::var_os("ROBCO_TEST_TAPE_OUT") {
        fs::write(out, &tape).unwrap();
    }

    // The second build: the lock and the cache hold everything, so it
    // reads no index, skips the installs, keeps the lock and gives the
    // same tape.
    let o = f.robco(&["build", "--json"]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let mut want = vec![
        format!("install {pipos} skip"),
        format!("install {gcc_item} skip"),
    ];
    want.extend(built.iter().map(|s| (*s).to_owned()));
    assert_eq!(steps(&o), want);
    assert_eq!(fs::read(f.tape()).unwrap(), tape);
    assert_eq!(fs::read(f.proj().join("robco.lock")).unwrap(), lock);

    // A lock edited to pin another gcc archive: the cached one no longer
    // matches it, so the build fails with lock: hash and changes nothing.
    let edited = String::from_utf8(lock.clone()).unwrap();
    let cached_hash = sha256_hex(&fs::read(f.dir.join("archives/gcc.archive")).unwrap());
    let other = "f".repeat(64);
    f.put(
        "proj/robco.lock",
        edited.replace(&cached_hash, &other).as_bytes(),
    );
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("lock: hash".to_owned(), 3));
    assert!(!f.tape().exists());
    f.put("proj/robco.lock", &lock);

    // A source with a mistake: build: compile with the source, a gcc
    // diagnostic, and no tape.
    f.put("proj/src/main.c", b"int pip_main(void) { return x; }\n");
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("build: compile".to_owned(), 5));
    let l = lines(&o);
    let err = l.iter().find(|v| v["kind"] == "error").unwrap();
    assert_eq!(err["detail"], "src/main.c");
    assert!(
        l.iter()
            .any(|v| v["kind"] == "diagnostic" && v["program"] == "gcc"),
        "{l:?}"
    );
    assert!(!f.tape().exists());

    // Without --json the diagnostics and the one error line go to
    // standard error, the error line last.
    let o = f.robco(&["build"]);
    assert_eq!(o.status.code(), Some(5));
    let err = String::from_utf8(o.stderr).unwrap();
    assert!(err.ends_with("build: compile: src/main.c\n"), "{err}");
    assert!(err.lines().count() > 1, "{err}");

    // A source that is a link inside the project compiles under its
    // manifest path, so the tape is the same as with the file itself.
    #[cfg(unix)]
    {
        f.put("proj/main.real", MAIN_C.as_bytes());
        fs::remove_file(f.proj().join("src/main.c")).unwrap();
        link(&f, "../main.real", "proj/src/main.c");
        let o = f.robco(&["build", "--json"]);
        assert_eq!(
            o.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&o.stdout)
        );
        assert!(steps(&o).contains(&"compile src/main.c done".to_owned()));
        assert_eq!(lines(&o).pop().unwrap()["sources"], json!(["src/main.c"]));
        assert_eq!(fs::read(f.tape()).unwrap(), tape);
    }
}

/// Makes a link at `at` under the fixture's folder that points to `target`.
#[cfg(unix)]
fn link(f: &Fixture, target: &str, at: &str) {
    std::os::unix::fs::symlink(target, f.dir.join(at)).unwrap();
}

/// A manifest with an icon at `icon`.
#[cfg(unix)]
fn with_icon(f: &Fixture, icon: &str) {
    let m = MANIFEST.replace(
        "subtype = \"utility\"",
        &format!("subtype = \"utility\"\nicon = \"{icon}\""),
    );
    f.put("proj/robco.toml", m.as_bytes());
}

/// Step 2 passed: with no lock and no index, step 6 is the first failure.
#[cfg(unix)]
fn passes_step_2(f: &Fixture) {
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("index: location".to_owned(), 4));
}

#[cfg(unix)]
fn fails_with(f: &Fixture, code: &str) {
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), (code.to_owned(), 3));
}

#[cfg(unix)]
#[test]
fn an_icon_link_out_of_the_root_is_manifest_path() {
    let f = Fixture::new("icon-out");
    f.put("icon.bin", &[0u8; 128]);
    link(&f, "../icon.bin", "proj/icon.bin");
    with_icon(&f, "icon.bin");
    fails_with(&f, "manifest: path");
}

#[cfg(unix)]
#[test]
fn an_icon_link_inside_the_root_is_read() {
    let f = Fixture::new("icon-in");
    f.put("proj/art/icon.bin", &[0u8; 128]);
    link(&f, "art/icon.bin", "proj/icon.bin");
    with_icon(&f, "icon.bin");
    passes_step_2(&f);
    // The same link to a bad icon fails: the file was read.
    f.put("proj/art/icon.bin", b"not an icon");
    fails_with(&f, "manifest: icon");
}

#[cfg(unix)]
#[test]
fn a_dangling_icon_link_is_manifest_icon() {
    let f = Fixture::new("icon-dangling");
    link(&f, "gone.bin", "proj/icon.bin");
    with_icon(&f, "icon.bin");
    fails_with(&f, "manifest: icon");
}

#[cfg(unix)]
#[test]
fn a_source_link_inside_the_root_builds() {
    let f = Fixture::new("src-in");
    fs::rename(f.proj().join("src/main.c"), f.proj().join("main.real")).unwrap();
    link(&f, "../main.real", "proj/src/main.c");
    passes_step_2(&f);
}

#[cfg(unix)]
#[test]
fn a_source_link_out_of_the_root_is_manifest_path() {
    let f = Fixture::new("src-out");
    f.put("away.c", MAIN_C.as_bytes());
    link(&f, "../../away.c", "proj/src/away.c");
    fails_with(&f, "manifest: path");
}

#[cfg(unix)]
#[test]
fn a_dangling_source_link_is_manifest_sources() {
    let f = Fixture::new("src-dangling");
    fs::remove_file(f.proj().join("src/main.c")).unwrap();
    link(&f, "gone.c", "proj/src/main.c");
    fails_with(&f, "manifest: sources");
}

#[cfg(unix)]
#[test]
fn a_source_folder_link_inside_the_root_builds() {
    let f = Fixture::new("dir-in");
    fs::rename(f.proj().join("src"), f.proj().join("code")).unwrap();
    link(&f, "code", "proj/src");
    passes_step_2(&f);
}

#[cfg(unix)]
#[test]
fn a_source_folder_link_out_of_the_root_is_manifest_path() {
    let f = Fixture::new("dir-out");
    fs::rename(f.proj().join("src"), f.dir.join("code")).unwrap();
    link(&f, "../code", "proj/src");
    fails_with(&f, "manifest: path");
}

/// A manifest with these source patterns.
#[cfg(unix)]
fn with_sources(f: &Fixture, sources: &str) {
    let m = MANIFEST.replace(r#"sources = ["src/*.c"]"#, &format!("sources = {sources}"));
    f.put("proj/robco.toml", m.as_bytes());
}

#[cfg(unix)]
#[test]
fn an_earlier_pattern_link_out_comes_before_a_later_empty_pattern() {
    // Step 6 checks each pattern whole, `path` then `sources`, in order.
    let f = Fixture::new("order-file");
    f.put("away.c", MAIN_C.as_bytes());
    link(&f, "../../away.c", "proj/src/away.c");
    with_sources(&f, r#"["src/*.c", "none/*.c"]"#);
    fails_with(&f, "manifest: path");
}

#[cfg(unix)]
#[test]
fn an_earlier_empty_pattern_comes_before_a_later_folder_link_out() {
    let f = Fixture::new("order-dir");
    f.put("code/lib.c", MAIN_C.as_bytes());
    link(&f, "../code", "proj/lib");
    with_sources(&f, r#"["none.c", "lib/*.c"]"#);
    fails_with(&f, "manifest: sources");
}

#[cfg(unix)]
#[test]
fn a_pattern_link_out_comes_before_its_own_non_c_match() {
    // Within one pattern, `path` comes before `sources`.
    let f = Fixture::new("order-within");
    f.put("away.c", MAIN_C.as_bytes());
    link(&f, "../../away.c", "proj/src/away.c");
    f.put("proj/src/notes.txt", b"notes");
    with_sources(&f, r#"["src/*"]"#);
    fails_with(&f, "manifest: path");
}

#[cfg(unix)]
#[test]
fn an_entry_no_pattern_matches_is_not_looked_at() {
    // A link loop: its type cannot be read. No pattern matches its name.
    let f = Fixture::new("loop");
    link(&f, "loop.txt", "proj/src/loop.txt");
    passes_step_2(&f);
}

/// The hashes a warm cache records and its lock pins.
#[cfg(unix)]
const PKG_HASH: &str = "00000000000000000000000000000000000000000000000000000000000000a1";
#[cfg(unix)]
const GCC_HASH: &str = "00000000000000000000000000000000000000000000000000000000000000a2";

/// A cache that holds a `pipos` 1.4.0 package and a `gcc` tool, each with
/// its `.robco-archive`, and a lock that pins both, so a build reads no
/// index. The two programs are shell scripts: `gcc` appends its arguments,
/// one per line, to `gcc.args` beside the project and makes an empty file
/// at each `-o`, and `objcopy` writes a raw image as the startup code would,
/// then runs `hook`, a shell line, in the project root.
#[cfg(unix)]
fn warm_cache(f: &Fixture, hook: &str) {
    use std::os::unix::fs::PermissionsExt;
    let mut sdk: Value =
        serde_json::from_str(include_str!("../../studio-core/tests/fixtures/sdk.json")).unwrap();
    sdk["version"] = json!("1.4.0");
    sdk["abi"] = json!("1.4");
    let pkg = "cache/packages/pipos/1.4.0";
    f.put(
        &format!("{pkg}/.robco-archive"),
        format!("{PKG_HASH}\n").as_bytes(),
    );
    f.put(&format!("{pkg}/sdk.json"), sdk.to_string().as_bytes());
    f.put(
        &format!("{pkg}/flags.json"),
        include_bytes!("../../studio-core/tests/plan/flags.json"),
    );
    let gcc = format!("cache/tools/gcc/15.2.1-1.1/{}", host());
    f.put(
        &format!("{gcc}/.robco-archive"),
        format!("{GCC_HASH}\n").as_bytes(),
    );
    let raw = studio_sys::rimg_pack(
        &[0u8; 64],
        &studio_sys::PackOpts {
            magic: "ROBCO78",
            subtype: 1,
            load_addr: 0x0080_0000,
            abi_version: Some(0x0001_0004),
            ..Default::default()
        },
    )
    .unwrap();
    f.put("raw.bin", &raw);
    let raw = f.dir.join("raw.bin");
    let gcc_sh = "#!/bin/sh\nprintf '%s\\n' \"$@\" >> ../gcc.args\nprev=\nfor a; do [ \"$prev\" = -o ] && : > \"$a\"; prev=$a; done\n";
    let objcopy_sh = format!(
        "#!/bin/sh\nfor a; do last=$a; done\ncp '{}' \"$last\"\n{hook}\n",
        raw.display()
    );
    for (name, body) in [
        ("arm-none-eabi-gcc", gcc_sh.to_owned()),
        ("arm-none-eabi-objcopy", objcopy_sh),
    ] {
        let p = format!("{gcc}/bin/{name}");
        f.put(&p, body.as_bytes());
        fs::set_permissions(f.dir.join(p), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let lock = STALE_LOCK
        .replace("1.3.0", "1.4.0")
        .replace(
            "0000000000000000000000000000000000000000000000000000000000000001",
            PKG_HASH,
        )
        .replace(
            "0000000000000000000000000000000000000000000000000000000000000002",
            GCC_HASH,
        );
    let lock = lock.replace("linux-x64 = \"", &format!("{} = \"", host()));
    let lock = dedup_hosts(&lock);
    f.put("proj/robco.lock", lock.as_bytes());
}

/// Drops a host line that a replacement made twice within one table.
#[cfg(unix)]
fn dedup_hosts(lock: &str) -> String {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = String::new();
    for l in lock.lines() {
        if l.starts_with('[') {
            seen.clear();
        }
        if let Some((k, _)) = l.split_once(" = ")
            && !l.starts_with("name")
            && !l.starts_with("version")
            && !l.starts_with("sha256")
            && !l.starts_with("index")
            && !l.starts_with("format")
        {
            if seen.contains(&k) {
                continue;
            }
            seen.push(k);
        }
        out.push_str(l);
        out.push('\n');
    }
    out
}

#[cfg(unix)]
#[test]
fn a_warm_cache_builds_a_tape_with_no_index() {
    let f = Fixture::new("warm");
    warm_cache(&f, "");
    let o = f.robco(&["build", "--json"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    assert!(f.tape().is_file());
}

#[cfg(unix)]
#[test]
fn a_source_named_with_a_leading_at_builds_as_a_file() {
    // gcc reads `@main.c` as a response file, so the plan passes `./@main.c`.
    let f = Fixture::new("at-source");
    warm_cache(&f, "");
    fs::rename(f.proj().join("src/main.c"), f.proj().join("@main.c")).unwrap();
    with_sources(&f, r#"["@main.c"]"#);
    let o = f.robco(&["build", "--json"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    assert!(f.tape().is_file());
    let args = fs::read_to_string(f.dir.join("gcc.args")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert!(args.contains(&"./@main.c"), "{args:?}");
    assert!(!args.contains(&"@main.c"), "{args:?}");
}

#[cfg(unix)]
#[test]
fn a_cached_package_without_sdk_json_is_package_manifest() {
    let f = Fixture::new("no-sdk");
    warm_cache(&f, "");
    f.older_tape();
    fs::remove_file(f.dir.join("cache/packages/pipos/1.4.0/sdk.json")).unwrap();
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("package: manifest".to_owned(), 4));
    assert!(!f.tape().exists());
}

#[cfg(unix)]
#[test]
fn a_source_removed_during_the_build_leaves_no_failure_after_the_tape() {
    // The source goes away before the tape is written. The build already
    // holds its source list, so it reports it and does not fail after the
    // tape is in place.
    let f = Fixture::new("gone");
    warm_cache(&f, "rm src/main.c");
    let o = f.robco(&["build", "--json"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    assert!(f.tape().is_file());
    assert_eq!(lines(&o).pop().unwrap()["sources"], json!(["src/main.c"]));
}

#[cfg(unix)]
#[test]
fn a_compiler_the_host_cannot_start_is_tool_program() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new("no-exec");
    warm_cache(&f, "");
    f.older_tape();
    let gcc = f.dir.join(format!(
        "cache/tools/gcc/15.2.1-1.1/{}/bin/arm-none-eabi-gcc",
        host()
    ));
    fs::set_permissions(&gcc, fs::Permissions::from_mode(0o644)).unwrap();
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("tool: program".to_owned(), 4));
    assert!(!f.tape().exists());
}

#[cfg(unix)]
#[test]
fn a_failed_objcopy_leaves_no_tape_and_keeps_lock_and_cache() {
    let f = Fixture::new("objcopy-fails");
    warm_cache(&f, "exit 1");
    f.older_tape();
    let lock = fs::read(f.proj().join("robco.lock")).unwrap();
    let sdk = f.dir.join("cache/packages/pipos/1.4.0/sdk.json");
    let before = fs::read(&sdk).unwrap();
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("build: objcopy".to_owned(), 5));
    assert!(!f.tape().exists());
    assert!(!f.proj().join("build/TINY.HTP.tmp").exists());
    assert_eq!(fs::read(f.proj().join("robco.lock")).unwrap(), lock);
    assert_eq!(fs::read(&sdk).unwrap(), before);
}

/// Adds `src/zz.c` after `src/main.c` and makes it unreadable.
#[cfg(unix)]
fn unreadable_second_source(f: &Fixture) {
    use std::os::unix::fs::PermissionsExt;
    f.put("proj/src/zz.c", b"int zz;\n");
    let p = f.proj().join("src/zz.c");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
}

#[cfg(unix)]
#[test]
fn a_missing_compiler_comes_before_an_unreadable_source() {
    let f = Fixture::new("order-tool-read");
    warm_cache(&f, "");
    unreadable_second_source(&f);
    fs::remove_file(f.dir.join(format!(
        "cache/tools/gcc/15.2.1-1.1/{}/bin/arm-none-eabi-gcc",
        host()
    )))
    .unwrap();
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("tool: program".to_owned(), 4));
}

#[cfg(unix)]
#[test]
fn a_failed_compile_comes_before_a_later_unreadable_source() {
    let f = Fixture::new("order-compile-read");
    warm_cache(&f, "");
    unreadable_second_source(&f);
    gcc_stand_in(
        &f,
        "for a; do [ \"$a\" = src/main.c ] && exit 1; done\n\
         prev=\nfor a; do [ \"$prev\" = -o ] && : > \"$a\"; prev=$a; done",
    );
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("build: compile".to_owned(), 5));
    assert!(steps(&o).iter().any(|s| s.contains("src/main.c")));
}

/// Replaces the `gcc` stand-in of a warm cache with this shell body.
#[cfg(unix)]
fn gcc_stand_in(f: &Fixture, body: &str) {
    let p = f.dir.join(format!(
        "cache/tools/gcc/15.2.1-1.1/{}/bin/arm-none-eabi-gcc",
        host()
    ));
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
}

#[cfg(unix)]
#[test]
fn a_compiler_that_writes_nothing_fails_and_leaves_no_tape() {
    // A first build leaves its objects. The second compiler ends well and
    // writes nothing, as `-fsyntax-only` does: the old objects must not
    // reach the link.
    let f = Fixture::new("no-output");
    warm_cache(&f, "");
    let o = f.robco(&["build", "--json"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    assert!(f.proj().join("build/obj/crt0.o").is_file());
    gcc_stand_in(&f, "exit 0");
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("build: compile".to_owned(), 5));
    assert!(!f.tape().exists());
    assert!(!f.proj().join("build/obj/crt0.o").exists());
}

#[cfg(unix)]
#[test]
fn a_stale_object_is_gone_before_the_compile_runs() {
    let f = Fixture::new("stale-object");
    warm_cache(&f, "");
    f.put("proj/build/obj/src/main.c.o", b"old");
    // The first command, the crt0 compile, notes a stale object it sees.
    let seen = f.dir.join("seen");
    gcc_stand_in(
        &f,
        &format!(
            "prev=\nfor a; do [ \"$prev\" = -o ] && out=$a; prev=$a; done\n\
             [ \"$out\" = build/obj/crt0.o ] && [ -e build/obj/src/main.c.o ] && : > '{}'\n\
             prev=\nfor a; do [ \"$prev\" = -o ] && : > \"$a\"; prev=$a; done",
            seen.display()
        ),
    );
    let o = f.robco(&["build", "--json"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    assert!(!seen.exists(), "a compile saw the stale object");
    assert_eq!(
        fs::read(f.proj().join("build/obj/src/main.c.o")).unwrap(),
        b""
    );
}

#[cfg(unix)]
#[test]
fn a_matched_source_that_is_a_link_loop_is_manifest_path() {
    let f = Fixture::new("src-loop");
    link(&f, "loop.c", "proj/src/loop.c");
    fails_with(&f, "manifest: path");
}

#[cfg(unix)]
#[test]
fn a_project_folder_that_is_not_utf8_fails_before_anything_changes() {
    use std::os::unix::ffi::OsStrExt;
    let f = Fixture::new("non-utf8");
    warm_cache(&f, "");
    f.older_tape();
    let odd = f.dir.join(std::ffi::OsStr::from_bytes(b"p\xffq"));
    if fs::create_dir(&odd).is_err() {
        println!("skipped: this file system refuses a name that is not UTF-8");
        return;
    }
    fs::remove_dir(&odd).unwrap();
    fs::rename(f.proj(), &odd).unwrap();
    // The command runs in `proj`, a link, so its current folder is `odd`.
    std::os::unix::fs::symlink(&odd, f.proj()).unwrap();
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("manifest: path".to_owned(), 3));
    let detail = lines(&o)
        .into_iter()
        .find(|l| l["kind"] == "error")
        .unwrap()["detail"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(detail.contains("must be valid UTF-8"), "{detail}");
    assert!(steps(&o).is_empty(), "{:?}", steps(&o));
    assert!(odd.join("build/TINY.HTP").is_file());
}

#[cfg(unix)]
#[test]
fn an_icon_that_is_a_folder_is_manifest_icon() {
    let f = Fixture::new("icon-dir");
    fs::create_dir_all(f.proj().join("art")).unwrap();
    link(&f, "art", "proj/icon.lnk");
    for icon in ["art", "icon.lnk"] {
        with_icon(&f, icon);
        fails_with(&f, "manifest: icon");
    }
}

#[cfg(unix)]
#[test]
fn a_source_that_is_a_folder_is_manifest_sources() {
    let f = Fixture::new("src-dir");
    fs::create_dir_all(f.proj().join("src/dir.c")).unwrap();
    with_sources(&f, r#"["src/main.c", "src/dir.c"]"#);
    let o = f.robco(&["build", "--json"]);
    assert_eq!(failure(&o), ("manifest: sources".to_owned(), 3));
    let s = String::from_utf8_lossy(&o.stdout);
    assert!(s.contains("src/dir.c: not a regular file"), "{s}");
    // A wildcard keeps only regular files: the folder is not a match.
    with_sources(&f, r#"["src/*.c"]"#);
    passes_step_2(&f);
}
