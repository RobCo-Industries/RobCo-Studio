// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the resolver, the index and `sdk.json` readers and the lock.
//! The fixtures are the specs' examples, copied byte for byte.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::index::{self, Index};
use super::lock::{self, Lock};
use super::sdk_json;
use super::*;

const INDEX: &str = include_str!("../../tests/fixtures/index.json");
const SDK: &str = include_str!("../../tests/fixtures/sdk.json");
const LOCK: &str = include_str!("../../tests/fixtures/robco.lock");
const LOCATION: &str = "https://example.invalid/robco/index.json";
const H: &str = "0000000000000000000000000000000000000000000000000000000000000001";

/// A folder of the test's own under the system's temp folder, removed on
/// drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "studio-resolve-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn listing(&self) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn target(abi: &str) -> Target {
    Target {
        platform: PIPOS.to_owned(),
        abi: Abi::parse(abi).unwrap(),
    }
}

fn pkg(name: &str, version: &str, extra: &str) -> String {
    let abi = if name == PIPOS {
        let v = Version::parse(version).unwrap();
        format!(
            r#""abi": "{}", "tools": [{{"name": "gcc", "version": "15.2.1-1.1"}}, {{"name": "qemu", "version": "10.1.2"}}],"#,
            v.abi()
        )
    } else {
        String::new()
    };
    format!(
        r#"{{"name": "{name}", "version": "{version}", {abi} {extra} "url": "u", "size": 1, "sha256": "{H}"}}"#
    )
}

fn archive() -> String {
    format!(r#"{{"url": "u", "size": 1, "sha256": "{H}", "root": ""}}"#)
}

fn tools_json() -> String {
    format!(
        r#"{{"name": "gcc", "version": "15.2.1-1.1", "archives": {{"linux-x64": {a}, "macos-arm64": {a}}}}},
           {{"name": "qemu", "version": "10.1.2", "archives": {{"linux-x64": {a}, "macos-arm64": {a}}}}}"#,
        a = archive()
    )
}

fn index_of(packages: &[String], tools: &str) -> String {
    format!(
        r#"{{"format": 1, "packages": [{}], "tools": [{tools}]}}"#,
        packages.join(",")
    )
}

fn pipos_index(versions: &[&str]) -> Index {
    let p: Vec<String> = versions.iter().map(|v| pkg(PIPOS, v, "")).collect();
    index::parse(&index_of(&p, &tools_json())).unwrap()
}

fn code<T: std::fmt::Debug>(r: Result<T, Error>) -> &'static str {
    r.unwrap_err().code()
}

// The spec examples.

#[test]
fn resolve_spec_examples_parse() {
    let i = index::parse(INDEX).unwrap();
    assert_eq!(i.packages.len(), 1);
    assert_eq!(i.packages[0].tools[0].version, "15.2.1-1.1");
    let s = sdk_json::parse(SDK).unwrap();
    assert_eq!(s.abi, Abi::parse("1.3"));
    s.check_against(&i.packages[0]).unwrap();
    assert!(fits_sdk(&target("1.3"), &s));
    let l = Lock::parse(LOCK).unwrap();
    assert_eq!(l.tools.len(), 2);
}

#[test]
fn resolve_spec_index_gives_spec_lock_bytes() {
    let i = index::parse(INDEX).unwrap();
    let lock = resolve(&target("1.3"), &i, LOCATION, "macos-arm64").unwrap();
    assert_eq!(lock.to_text(), LOCK);
    assert_eq!(Lock::parse(LOCK).unwrap().to_text(), LOCK);
}

#[test]
fn resolve_writes_same_bytes_twice() {
    let dir = TempDir::new("twice");
    let i = index::parse(INDEX).unwrap();
    let lock = resolve(&target("1.3"), &i, LOCATION, "linux-x64").unwrap();
    lock::write(dir.path(), &lock).unwrap();
    let first = fs::read(dir.path().join(lock::FILE)).unwrap();
    let again = resolve(&target("1.3"), &i, LOCATION, "windows-x64").unwrap();
    lock::write(dir.path(), &again).unwrap();
    let second = fs::read(dir.path().join(lock::FILE)).unwrap();
    assert_eq!(first, second);
    assert_eq!(first, LOCK.as_bytes());
    assert_eq!(dir.listing(), vec![lock::FILE.to_owned()]);
}

#[test]
fn resolve_lock_is_sorted_whatever_the_order_given() {
    let mut lock = Lock::parse(LOCK).unwrap();
    lock.tools.reverse();
    assert_eq!(lock.to_text(), LOCK);
    let mut reel = lock.packages[0].clone();
    reel.name = "reel".to_owned();
    lock.packages.insert(0, reel);
    let text = lock.to_text();
    assert!(text.find("name = \"pipos\"").unwrap() < text.find("name = \"reel\"").unwrap());
}

// Fit rules and the resolver.

#[test]
fn resolve_fit_rules_for_abi_1_3() {
    let i = pipos_index(&["1.2.9", "1.3.0", "1.3.4", "1.4.0", "1.4.2", "2.0.0"]);
    let t = target("1.3");
    let fit: Vec<String> = i
        .packages
        .iter()
        .filter(|p| fits(&t, p))
        .map(|p| p.version.to_string())
        .collect();
    assert_eq!(fit, ["1.3.0", "1.3.4", "1.4.0", "1.4.2"]);
    let lock = resolve(&t, &i, "x", "linux-x64").unwrap();
    assert_eq!(lock.packages[0].version.to_string(), "1.4.2");
}

#[test]
fn resolve_newest_does_not_depend_on_index_order() {
    let i = pipos_index(&["1.4.2", "1.10.0", "1.3.0", "1.9.9"]);
    let lock = resolve(&target("1.3"), &i, "x", "linux-x64").unwrap();
    assert_eq!(lock.packages[0].version.to_string(), "1.10.0");
}

#[test]
fn resolve_no_package_fits() {
    let i = pipos_index(&["1.2.0", "2.0.0"]);
    let e = resolve(&target("1.3"), &i, "x", "linux-x64").unwrap_err();
    assert_eq!(e.code(), "sdk: no package");
    assert_eq!(e.to_string(), "sdk: no package: nothing fits pipos ABI 1.3");
    let mut t = target("1.2");
    t.platform = "uos".to_owned();
    assert_eq!(code(resolve(&t, &i, "x", "linux-x64")), "sdk: no package");
}

#[test]
fn resolve_follows_requires() {
    let p = vec![
        pkg(
            PIPOS,
            "1.3.0",
            r#""requires": [{"name": "reel", "abi": "2.1"}],"#,
        ),
        pkg("reel", "2.0.0", ""),
        pkg("reel", "2.1.0", ""),
        pkg("reel", "2.3.1", ""),
        pkg("reel", "3.0.0", ""),
    ];
    let i = index::parse(&index_of(&p, &tools_json())).unwrap();
    let lock = resolve(&target("1.3"), &i, "x", "linux-x64").unwrap();
    let got: Vec<String> = lock
        .packages
        .iter()
        .map(|p| format!("{} {}", p.name, p.version))
        .collect();
    assert_eq!(got, ["pipos 1.3.0", "reel 2.3.1"]);
    let p = vec![pkg(
        PIPOS,
        "1.3.0",
        r#""requires": [{"name": "reel", "abi": "4.0"}],"#,
    )];
    let i = index::parse(&index_of(&p, &tools_json())).unwrap();
    assert_eq!(
        code(resolve(&target("1.3"), &i, "x", "linux-x64")),
        "sdk: no package"
    );
}

#[test]
fn resolve_takes_the_tools_the_package_names() {
    let a = archive();
    let tools = format!(
        r#"{}, {{"name": "gcc", "version": "16.0.0-1.1", "archives": {{"linux-x64": {a}}}}},
           {{"name": "qemu", "version": "11.0.0", "archives": {{"linux-x64": {a}}}}}"#,
        tools_json()
    );
    let i = index::parse(&index_of(&[pkg(PIPOS, "1.3.0", "")], &tools)).unwrap();
    let lock = resolve(&target("1.3"), &i, "x", "linux-x64").unwrap();
    assert_eq!(lock.tools[0].version, "15.2.1-1.1");
    assert_eq!(lock.tools[1].version, "10.1.2");
    let pick = pick_tool(&i, &i.packages[0], "gcc").unwrap();
    assert_eq!(pick.version, "15.2.1-1.1");
}

#[test]
fn resolve_no_tool() {
    let a = archive();
    let tools =
        format!(r#"{{"name": "gcc", "version": "15.2.1", "archives": {{"linux-x64": {a}}}}}"#);
    let i = index::parse(&index_of(&[pkg(PIPOS, "1.3.0", "")], &tools)).unwrap();
    let e = resolve(&target("1.3"), &i, "x", "linux-x64").unwrap_err();
    assert_eq!(e.code(), "sdk: no tool");
    assert_eq!(
        e.to_string(),
        "sdk: no tool: the index lists no gcc 15.2.1-1.1"
    );
}

#[test]
fn resolve_missing_host() {
    let i = index::parse(INDEX).unwrap();
    let e = resolve(&target("1.3"), &i, "x", "linux-arm64").unwrap_err();
    assert_eq!(
        e,
        Error::IndexHost {
            tool: "qemu".to_owned(),
            version: "10.1.2".to_owned(),
            host: "linux-arm64".to_owned(),
        }
    );
    assert_eq!(e.code(), "index: host");
}

// The index.

#[test]
fn resolve_index_duplicates() {
    let p = vec![pkg(PIPOS, "1.3.0", ""), pkg(PIPOS, "1.03.0", "")];
    assert_eq!(
        code(index::parse(&index_of(&p, &tools_json()))),
        "index: duplicate"
    );
    let t = format!("{}, {}", tools_json(), tools_json());
    assert_eq!(
        code(index::parse(&index_of(&[pkg(PIPOS, "1.3.0", "")], &t))),
        "index: duplicate"
    );
}

#[test]
fn resolve_index_format() {
    let larger = INDEX.replacen("\"format\": 1", "\"format\": 2", 1);
    assert_eq!(code(index::parse(&larger)), "index: format");
    assert_eq!(
        code(index::parse(r#"{"packages": [], "tools": []}"#)),
        "index: format"
    );
    assert_eq!(
        code(index::parse(
            r#"{"format": 0, "packages": [], "tools": []}"#
        )),
        "index: format"
    );
    assert_eq!(code(index::parse("not json")), "index: read");
}

#[test]
fn resolve_index_entries() {
    let no_url = INDEX.replacen(
        "\"url\": \"https://example.invalid/robco/pipos-1.3.0.zip\",",
        "",
        1,
    );
    let e = index::parse(&no_url).unwrap_err();
    assert_eq!(
        e,
        Error::IndexEntry("`packages[0].url` is missing".to_owned())
    );
    let bad = [
        INDEX.replacen("\"abi\": \"1.3\"", "\"abi\": \"1.4\"", 1),
        INDEX.replacen("\"version\": \"1.3.0\"", "\"version\": \"1.3\"", 1),
        INDEX.replacen("\"size\": 812345", "\"size\": -1", 1),
        INDEX.replacen(&format!("\"sha256\": \"{H}\""), "\"sha256\": \"01\"", 1),
        INDEX.replacen(
            r#"{"name": "gcc", "version": "15.2.1-1.1"},
        {"name": "qemu", "version": "10.1.2"}"#,
            r#"{"name": "qemu", "version": "10.1.2"},
        {"name": "gcc", "version": "15.2.1-1.1"}"#,
            1,
        ),
    ];
    for text in bad {
        assert_ne!(text, INDEX);
        assert_eq!(code(index::parse(&text)), "index: entry");
    }
    let templates = index_of(&[pkg("templates", "1.0.0", r#""tools": [],"#)], "");
    assert_eq!(code(index::parse(&templates)), "index: entry");
    let unknown = INDEX.replacen("\"format\": 1,", "\"format\": 1, \"later\": true,", 1);
    index::parse(&unknown).unwrap();
}

// sdk.json.

#[test]
fn resolve_sdk_json_errors() {
    let larger = SDK.replacen("\"format\": 1", "\"format\": 2", 1);
    assert_eq!(code(sdk_json::parse(&larger)), "package: format");
    let unknown_format = SDK.replacen("\"runner\": 1", "\"runner\": 1, \"sound\": 1", 1);
    assert_eq!(code(sdk_json::parse(&unknown_format)), "package: format");
    let newer_format = SDK.replacen("\"rimg\": 1", "\"rimg\": 2", 1);
    assert_eq!(code(sdk_json::parse(&newer_format)), "package: format");
    let no_tools = SDK.replacen(
        r#"  "tools": [
    {"name": "gcc", "version": "15.2.1-1.1"},
    {"name": "qemu", "version": "10.1.2"}
  ],
"#,
        "",
        1,
    );
    assert_ne!(no_tools, SDK);
    assert_eq!(code(sdk_json::parse(&no_tools)), "package: manifest");
    let no_name = SDK.replacen("\"name\": \"pipos\",", "", 1);
    assert_eq!(code(sdk_json::parse(&no_name)), "package: manifest");
    let unsorted = SDK.replacen("\"path\": \"boot.json\"", "\"path\": \"zz\"", 1);
    assert_eq!(code(sdk_json::parse(&unsorted)), "package: manifest");
}

#[test]
fn resolve_sdk_json_tools_must_match_index() {
    let i = index::parse(INDEX).unwrap();
    let s = sdk_json::parse(&SDK.replacen("\"10.1.2\"", "\"10.1.3\"", 1)).unwrap();
    assert_eq!(code(s.check_against(&i.packages[0])), "package: manifest");
}

/// Each field that sdk.json and the index entry both carry must agree.
#[test]
fn resolve_sdk_json_must_agree_with_index_field_by_field() {
    let i = index::parse(INDEX).unwrap();
    let e = &i.packages[0];
    let s = sdk_json::parse(SDK).unwrap();
    s.check_against(e).unwrap();
    let req = |name: &str, abi: &str| Requirement {
        name: name.to_owned(),
        abi: Abi::parse(abi).unwrap(),
    };

    let mut x = s.clone();
    x.name = "other".to_owned();
    assert_eq!(code(x.check_against(e)), "package: manifest", "name");

    let mut x = s.clone();
    x.version = Version::parse("1.3.1").unwrap();
    assert_eq!(code(x.check_against(e)), "package: manifest", "version");

    let mut x = s.clone();
    x.abi = None;
    assert_eq!(code(x.check_against(e)), "package: manifest", "abi");

    // An archive that needs a package its index entry does not name.
    let mut x = s.clone();
    x.requires = vec![req("reel", "1.0")];
    assert_eq!(code(x.check_against(e)), "package: manifest", "requires");
    // The other way round, and the same name at another ABI.
    let mut ie = e.clone();
    ie.requires = vec![req("reel", "1.0")];
    assert_eq!(code(s.check_against(&ie)), "package: manifest");
    let mut x = s.clone();
    x.requires = vec![req("reel", "1.1")];
    assert_eq!(code(x.check_against(&ie)), "package: manifest");
    // The same set in another order agrees.
    ie.requires = vec![req("reel", "1.0"), req("templates", "1.0")];
    x.requires = vec![req("templates", "1.0"), req("reel", "1.0")];
    x.check_against(&ie).unwrap();

    let mut x = s.clone();
    x.tools.reverse();
    assert_eq!(code(x.check_against(e)), "package: manifest", "tools");
}

// The lock.

fn no_requires(_: &lock::LockedPackage) -> Vec<Requirement> {
    Vec::new()
}

fn stale_detail(text: &str, t: &Target, host: &str) -> String {
    match Lock::parse(text)
        .unwrap()
        .check_stale(t, host, &no_requires)
    {
        Err(Error::LockStale(d)) => d,
        other => panic!("not stale: {other:?}"),
    }
}

#[test]
fn resolve_stale_lock_fails_and_writes_nothing() {
    let dir = TempDir::new("stale");
    fs::write(dir.path().join(lock::FILE), LOCK).unwrap();
    let before = fs::read(dir.path().join(lock::FILE)).unwrap();
    let e = lock::use_or_resolve(
        dir.path(),
        &target("1.4"),
        "linux-x64",
        &no_requires,
        &|| panic!("a stale lock must not resolve"),
    )
    .unwrap_err();
    assert_eq!(e.code(), "lock: stale");
    assert!(e.to_string().ends_with("run robco update"));
    assert_eq!(fs::read(dir.path().join(lock::FILE)).unwrap(), before);
    assert_eq!(dir.listing(), vec![lock::FILE.to_owned()]);
}

#[test]
fn resolve_fitting_lock_is_used_unchanged() {
    let dir = TempDir::new("fit");
    fs::write(dir.path().join(lock::FILE), LOCK).unwrap();
    let l = lock::use_or_resolve(
        dir.path(),
        &target("1.2"),
        "linux-x64",
        &no_requires,
        &|| panic!("a fitting lock must not resolve"),
    )
    .unwrap();
    assert_eq!(l, Lock::parse(LOCK).unwrap());
    assert_eq!(
        fs::read(dir.path().join(lock::FILE)).unwrap(),
        LOCK.as_bytes()
    );
}

#[test]
fn resolve_no_lock_writes_one() {
    let dir = TempDir::new("none");
    let i = index::parse(INDEX).unwrap();
    let t = target("1.3");
    lock::use_or_resolve(dir.path(), &t, "linux-x64", &no_requires, &|| {
        resolve(&t, &i, LOCATION, "linux-x64")
    })
    .unwrap();
    assert_eq!(
        fs::read(dir.path().join(lock::FILE)).unwrap(),
        LOCK.as_bytes()
    );
}

#[test]
fn resolve_failed_update_keeps_old_lock() {
    let dir = TempDir::new("update");
    fs::write(dir.path().join(lock::FILE), "old").unwrap();
    let i = pipos_index(&["2.0.0"]);
    let t = target("1.3");
    let e = lock::update(dir.path(), &|| resolve(&t, &i, "x", "linux-x64")).unwrap_err();
    assert_eq!(e.code(), "sdk: no package");
    assert_eq!(fs::read(dir.path().join(lock::FILE)).unwrap(), b"old");
    assert_eq!(dir.listing(), vec![lock::FILE.to_owned()]);
}

#[test]
fn resolve_stale_rules() {
    // 1. No longer fits: ABI or platform.
    assert_eq!(
        stale_detail(LOCK, &target("1.4"), "linux-x64"),
        "package pipos 1.3.0 does not fit ABI 1.4"
    );
    let mut uos = target("1.3");
    uos.platform = "uos".to_owned();
    assert_eq!(
        stale_detail(LOCK, &uos, "linux-x64"),
        "package pipos 1.3.0 does not fit platform uos"
    );
    // 2. A needed package is missing.
    let no_pipos = LOCK.replacen(
        "[[package]]\nname = \"pipos\"",
        "[[package]]\nname = \"reel\"",
        1,
    );
    assert_eq!(
        stale_detail(&no_pipos, &target("1.3"), "linux-x64"),
        "package pipos is needed and not listed"
    );
    // 3. A listed package is not needed: the same lock, read with a
    // requirement that names pipos as needed and reel as extra.
    let extra = LOCK.replacen(
        "\n[[tool]]",
        &format!(
            "\n[[package]]\nname = \"reel\"\nversion = \"1.0.0\"\nsha256 = \"{H}\"\n\n[[tool]]"
        ),
        1,
    );
    assert_eq!(
        stale_detail(&extra, &target("1.3"), "linux-x64"),
        "package reel is listed and not needed"
    );
    // 4. A tool is missing, or has no archive for the host.
    assert_eq!(
        stale_detail(LOCK, &target("1.3"), "linux-arm64"),
        "tool qemu 10.1.2 has no archive for linux-arm64"
    );
    let cut = LOCK.find("\n[[tool]]\nname = \"qemu\"").unwrap();
    assert_eq!(
        stale_detail(&LOCK[..cut + 1], &target("1.3"), "linux-x64"),
        "tool qemu is not listed"
    );
}

#[test]
fn resolve_stale_follows_requires() {
    let l = LOCK.replacen(
        "\n[[tool]]",
        &format!(
            "\n[[package]]\nname = \"reel\"\nversion = \"2.0.0\"\nsha256 = \"{H}\"\n\n[[tool]]"
        ),
        1,
    );
    let needs = |abi: &'static str| {
        move |p: &lock::LockedPackage| {
            if p.name == PIPOS {
                vec![Requirement {
                    name: "reel".to_owned(),
                    abi: Abi::parse(abi).unwrap(),
                }]
            } else {
                Vec::new()
            }
        }
    };
    let lock = Lock::parse(&l).unwrap();
    lock.check_stale(&target("1.3"), "linux-x64", &needs("2.0"))
        .unwrap();
    let e = lock
        .check_stale(&target("1.3"), "linux-x64", &needs("2.1"))
        .unwrap_err();
    assert_eq!(
        e,
        Error::LockStale("package reel 2.0.0 does not fit ABI 2.1".to_owned())
    );
    let only_pipos = Lock::parse(LOCK).unwrap();
    let e = only_pipos
        .check_stale(&target("1.3"), "linux-x64", &needs("2.0"))
        .unwrap_err();
    assert_eq!(
        e,
        Error::LockStale("package reel is needed and not listed".to_owned())
    );
}

/// The lock example with extra packages, each at 1.0.0.
fn lock_with(extra: &[&str]) -> Lock {
    let mut add = String::new();
    for n in extra {
        add.push_str(&format!(
            "\n[[package]]\nname = \"{n}\"\nversion = \"1.0.0\"\nsha256 = \"{H}\"\n"
        ));
    }
    Lock::parse(&LOCK.replacen("\n[[tool]]", &format!("{add}\n[[tool]]"), 1)).unwrap()
}

/// Checks a lock where each `(from, to)` edge is a `requires` at ABI 1.0.
fn stale_with_edges(lock: &Lock, edges: &[(&str, &str)]) -> Result<(), Error> {
    let edges: Vec<(String, String)> = edges
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect();
    let requires = move |p: &lock::LockedPackage| {
        edges
            .iter()
            .filter(|(from, _)| *from == p.name)
            .map(|(_, to)| Requirement {
                name: to.clone(),
                abi: Abi::parse("1.0").unwrap(),
            })
            .collect()
    };
    lock.check_stale(&target("1.3"), "linux-x64", &requires)
}

#[test]
fn resolve_stale_needs_only_what_the_root_reaches() {
    let stale = |d: &str| Err(Error::LockStale(d.to_owned()));
    // Two packages that need each other and that pipos never reaches.
    assert_eq!(
        stale_with_edges(&lock_with(&["a", "b"]), &[("a", "b"), ("b", "a")]),
        stale("package a is listed and not needed")
    );
    // A chain that pipos never reaches.
    assert_eq!(
        stale_with_edges(&lock_with(&["a", "b", "c"]), &[("a", "b"), ("b", "c")]),
        stale("package a is listed and not needed")
    );
    // A cycle that pipos reaches, one that runs back to pipos too: fine.
    stale_with_edges(
        &lock_with(&["a", "b"]),
        &[(PIPOS, "a"), ("a", "b"), ("b", "a"), ("b", PIPOS)],
    )
    .unwrap();
    // A requirement on a package the lock does not hold.
    assert_eq!(
        stale_with_edges(&lock_with(&["a"]), &[(PIPOS, "a"), ("a", "ghost")]),
        stale("package ghost is needed and not listed")
    );
}

#[test]
fn resolve_lock_read_errors() {
    assert_eq!(code(Lock::parse("format = \n")), "lock: syntax");
    assert_eq!(
        code(Lock::parse(&LOCK.replacen("format = 1", "format = 2", 1))),
        "lock: format"
    );
    assert_eq!(
        code(Lock::parse(&LOCK.replacen("format = 1\n", "", 1))),
        "lock: format"
    );
    let e = Lock::parse(&LOCK.replacen("sha256 = ", "date = 1\nsha256 = ", 1)).unwrap_err();
    assert_eq!(e, Error::LockUnknownKey("package.date".to_owned()));
    let e = Lock::parse(&LOCK.replacen("linux-x64 = \"", "linux-x64 = 1 #\"", 1));
    assert_eq!(code(e), "lock: syntax");
    let e = Lock::parse(&LOCK.replacen("index =", "when = 1\nindex =", 1)).unwrap_err();
    assert_eq!(e, Error::LockUnknownKey("when".to_owned()));
    // A newer format with a new key reports the format, not the key.
    let newer = LOCK.replacen("format = 1", "format = 2\nwhen = 1", 1);
    assert_eq!(code(Lock::parse(&newer)), "lock: format");
    assert_eq!(
        code(Lock::parse(&LOCK.replacen("\"1.3.0\"", "\"1.3\"", 1))),
        "lock: syntax"
    );
}

#[test]
fn resolve_lock_tool_hash_missing() {
    let l = Lock::parse(LOCK).unwrap();
    let s = sdk_json::parse(SDK).unwrap();
    l.check_tools(&s).unwrap();
    let edited = Lock::parse(&LOCK.replacen("\"10.1.2\"", "\"10.1.3\"", 1)).unwrap();
    let e = edited.check_tools(&s).unwrap_err();
    assert_eq!(e.code(), "lock: tool");
    assert!(e.to_string().ends_with("run robco update"));

    let p = &l.packages[0];
    p.check_hash(&H.to_ascii_uppercase()).unwrap();
    assert_eq!(code(p.check_hash(&H.replace('1', "2"))), "lock: hash");

    let i = index::parse(INDEX).unwrap();
    p.find_in(&i).unwrap();
    l.tools[0].find_in(&i, "linux-x64").unwrap();
    let other = Lock::parse(&LOCK.replacen("\"1.3.0\"", "\"1.3.1\"", 1)).unwrap();
    assert_eq!(code(other.packages[0].find_in(&i)), "lock: missing");
    assert_eq!(code(l.tools[1].find_in(&i, "linux-arm64")), "lock: missing");
}

#[test]
fn resolve_lock_quotes_odd_text() {
    let mut l = Lock::parse(LOCK).unwrap();
    l.index = "C:\\idx \"a\"\n".to_owned();
    l.tools[0]
        .archives
        .insert("odd host".to_owned(), H.to_owned());
    let back = Lock::parse(&l.to_text()).unwrap();
    assert_eq!(back, l);
}

#[test]
fn resolve_error_codes() {
    let s = String::new;
    let all = [
        (Error::IndexRead(s()), "index: read"),
        (Error::IndexFormat(s()), "index: format"),
        (Error::IndexEntry(s()), "index: entry"),
        (
            Error::IndexDuplicate {
                name: s(),
                version: s(),
            },
            "index: duplicate",
        ),
        (
            Error::IndexHost {
                tool: s(),
                version: s(),
                host: s(),
            },
            "index: host",
        ),
        (Error::PackageFormat(s()), "package: format"),
        (Error::PackageManifest(s()), "package: manifest"),
        (
            Error::NoPackage {
                platform: s(),
                abi: Abi { major: 1, minor: 3 },
            },
            "sdk: no package",
        ),
        (
            Error::NoTool {
                name: s(),
                version: s(),
            },
            "sdk: no tool",
        ),
        (
            Error::LockTool {
                name: s(),
                locked: s(),
                named: s(),
            },
            "lock: tool",
        ),
        (Error::LockSyntax(s()), "lock: syntax"),
        (Error::LockFormat(s()), "lock: format"),
        (Error::LockUnknownKey(s()), "lock: unknown key"),
        (Error::LockStale(s()), "lock: stale"),
        (Error::LockHash(s()), "lock: hash"),
        (Error::LockMissing(s()), "lock: missing"),
        (Error::LockIo(s()), "lock: io"),
    ];
    for (e, c) in all {
        assert_eq!(e.code(), c);
        assert!(e.to_string().starts_with(c));
    }
}

#[test]
fn resolve_versions() {
    assert_eq!(Version::parse("1.02.3").unwrap().to_string(), "1.2.3");
    for bad in [
        "1.2",
        "1.2.3.4",
        "1.2.-3",
        "1.2.65536",
        "+1.2.3",
        "1. 2.3",
        "",
    ] {
        assert_eq!(Version::parse(bad), None, "{bad}");
    }
    assert!(Version::parse("1.10.0") > Version::parse("1.9.9"));
    assert_eq!(Abi::parse("1.3.0"), None);
}
