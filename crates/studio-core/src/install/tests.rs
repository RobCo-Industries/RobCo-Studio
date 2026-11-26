// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the install. Each test makes its archives and a local index in
//! its own folder. No test touches the network.

use std::fs;
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use zip::write::SimpleFileOptions;

use super::*;
use crate::resolve::index::{self, IndexSource};

/// A folder of the test's own, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "studio-install-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("cache")).unwrap();
        fs::create_dir_all(dir.join("server")).unwrap();
        Self(dir)
    }
    fn cache(&self) -> PathBuf {
        self.0.join("cache")
    }
    fn server(&self) -> PathBuf {
        self.0.join("server")
    }
    fn local(&self) -> LocalFiles {
        LocalFiles {
            base: self.server(),
        }
    }
    fn put(&self, name: &str, bytes: &[u8]) {
        fs::write(self.server().join(name), bytes).unwrap();
    }
    /// True when no install left a temporary folder behind.
    fn temp_is_empty(&self) -> bool {
        match fs::read_dir(self.cache().join(TEMP)) {
            Ok(mut it) => it.next().is_none(),
            Err(_) => true,
        }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn opts() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

/// The files of a small `templates` package.
fn files() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("holotape-c/robco.toml", b"[project]\nname = \"x\"\n"),
        ("holotape-c/src/main.c", b"int main(void) { return 0; }\n"),
        ("templates.json", b"{\"format\": 1}\n"),
    ]
}

/// An `sdk.json` that lists `listed`, sorted by path.
fn sdk_json(listed: &[(&str, &[u8])]) -> String {
    let mut listed = listed.to_vec();
    listed.sort_by(|a, b| a.0.cmp(b.0));
    let files: Vec<String> = listed
        .iter()
        .map(|(p, b)| {
            format!(
                r#"{{"path": "{p}", "size": {}, "sha256": "{}"}}"#,
                b.len(),
                sha256_hex(b)
            )
        })
        .collect();
    format!(
        r#"{{
  "format": 1,
  "name": "templates",
  "version": "0.1.0",
  "built_from": [{{"repo": "RobCo-Reel", "commit": "3333333333333333333333333333333333333333"}}],
  "formats": {{"templates": 1, "project": 1, "scenario": 1}},
  "licences": [{{"paths": [""], "spdx": "MIT"}}],
  "files": [{}]
}}"#,
        files.join(", ")
    )
}

/// A package zip: `sdk.json` listing `listed`, then each entry of `put`.
fn package_zip(listed: &[(&str, &[u8])], put: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("sdk.json", opts()).unwrap();
    z.write_all(sdk_json(listed).as_bytes()).unwrap();
    for (name, bytes) in put {
        z.start_file(*name, opts()).unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn good_zip() -> Vec<u8> {
    package_zip(&files(), &files())
}

/// The index entry of an archive, by a local index.
fn entry_for(url: &str, bytes: &[u8]) -> IndexPackage {
    entry_with(url, bytes.len() as u64, &sha256_hex(bytes))
}

fn entry_with(url: &str, size: u64, sha: &str) -> IndexPackage {
    let text = format!(
        r#"{{"format": 1, "tools": [], "packages": [{{
  "name": "templates", "version": "0.1.0",
  "formats": {{"templates": 1, "project": 1, "scenario": 1}},
  "url": "{url}", "size": {size}, "sha256": "{sha}"}}]}}"#
    );
    let mut idx = index::parse(text.as_bytes(), IndexSource::Local).unwrap();
    idx.packages.remove(0)
}

/// Serves a zip, installs it and returns the result.
fn install_zip(d: &Dir, bytes: &[u8]) -> Result<State, Error> {
    d.put("templates-0.1.0.zip", bytes);
    let e = entry_for("templates-0.1.0.zip", bytes);
    package(&d.cache(), &e, &d.local(), Pin::Index)
}

fn pkg_dir(d: &Dir) -> PathBuf {
    package_dir(&d.cache(), "templates", "0.1.0").unwrap()
}

/// The failure left nothing: no package folder and no temporary folder.
fn nothing_left(d: &Dir) {
    assert!(!pkg_dir(d).exists(), "a package folder was left");
    assert!(d.temp_is_empty(), "a temporary folder was left");
}

fn refused(d: &Dir, r: Result<State, Error>, code: &str) -> String {
    let e = r.unwrap_err();
    assert_eq!(e.error_code(), code, "{e}");
    nothing_left(d);
    e.to_string()
}

#[test]
fn install_a_good_package() {
    let d = Dir::new("good");
    let bytes = good_zip();
    assert_eq!(install_zip(&d, &bytes), Ok(State::Installed));
    let dir = pkg_dir(&d);
    for (p, b) in files() {
        assert_eq!(fs::read(join_inside(&dir, p, || ()).unwrap()).unwrap(), b);
    }
    assert_eq!(
        fs::read_to_string(dir.join(RECORD)).unwrap(),
        format!("{}\n", sha256_hex(&bytes))
    );
    assert_eq!(cached(&dir), Ok(Cached::Holds(sha256_hex(&bytes))));
    assert!(d.temp_is_empty());
}

/// Counts fetches, to show that an item in the cache is not downloaded.
struct Counting<'a>(&'a dyn Transport, AtomicUsize);

impl Transport for Counting<'_> {
    fn fetch(&self, url: &str, out: &mut dyn Write) -> Result<(), String> {
        self.1.fetch_add(1, Ordering::Relaxed);
        self.0.fetch(url, out)
    }
}

#[test]
fn install_skips_an_item_in_the_cache() {
    let d = Dir::new("present");
    let bytes = good_zip();
    install_zip(&d, &bytes).unwrap();
    let local = d.local();
    let t = Counting(&local, AtomicUsize::new(0));
    let e = entry_for("templates-0.1.0.zip", &bytes);
    assert_eq!(package(&d.cache(), &e, &t, Pin::Index), Ok(State::Present));
    assert_eq!(t.1.load(Ordering::Relaxed), 0);
}

#[test]
fn install_refuses_another_archive_in_the_cache_and_changes_nothing() {
    let d = Dir::new("other");
    install_zip(&d, &good_zip()).unwrap();
    let before = fs::read(pkg_dir(&d).join(RECORD)).unwrap();
    let other = package_zip(&files(), &files()[..2]);
    let e = entry_for("x.zip", &other);
    let r = package(&d.cache(), &e, &d.local(), Pin::Index).unwrap_err();
    assert_eq!(r.error_code(), "package: hash");
    assert!(r.to_string().contains("delete that folder"), "{r}");
    let r = package(&d.cache(), &e, &d.local(), Pin::Lock).unwrap_err();
    assert_eq!(r.error_code(), "lock: hash");
    assert_eq!(fs::read(pkg_dir(&d).join(RECORD)).unwrap(), before);
}

#[test]
fn install_refuses_a_wrong_archive_hash() {
    let d = Dir::new("hash");
    let bytes = good_zip();
    d.put("p.zip", &bytes);
    let mut sha = sha256_hex(&bytes);
    sha.replace_range(0..1, if sha.starts_with('0') { "1" } else { "0" });
    let e = entry_with("p.zip", bytes.len() as u64, &sha);
    let msg = refused(
        &d,
        package(&d.cache(), &e, &d.local(), Pin::Index),
        "package: hash",
    );
    assert!(msg.contains("SHA-256"), "{msg}");
}

#[test]
fn install_refuses_a_short_download() {
    let d = Dir::new("short");
    let bytes = good_zip();
    d.put("p.zip", &bytes[..bytes.len() - 10]);
    let e = entry_for("p.zip", &bytes);
    refused(
        &d,
        package(&d.cache(), &e, &d.local(), Pin::Index),
        "package: hash",
    );
}

/// Writes part of the bytes, then fails, as a connection that drops.
struct Drops(Vec<u8>);

impl Transport for Drops {
    fn fetch(&self, _url: &str, out: &mut dyn Write) -> Result<(), String> {
        out.write_all(&self.0[..self.0.len() / 2])
            .map_err(|e| e.to_string())?;
        Err("the connection dropped".to_owned())
    }
}

#[test]
fn install_refuses_an_interrupted_download() {
    let d = Dir::new("drops");
    let bytes = good_zip();
    let e = entry_for("p.zip", &bytes);
    let msg = refused(
        &d,
        package(&d.cache(), &e, &Drops(bytes), Pin::Index),
        "package: hash",
    );
    assert!(msg.contains("could not be fetched whole"), "{msg}");
}

#[test]
fn install_refuses_a_long_download() {
    let d = Dir::new("long");
    let bytes = good_zip();
    let mut longer = bytes.clone();
    longer.extend_from_slice(b"more");
    d.put("p.zip", &longer);
    let e = entry_for("p.zip", &bytes);
    refused(
        &d,
        package(&d.cache(), &e, &d.local(), Pin::Index),
        "package: hash",
    );
}

#[test]
fn install_says_nothing_was_fetched() {
    let d = Dir::new("nothing");
    let bytes = good_zip();
    let e = entry_for("https://example.invalid/p.zip", &bytes);
    let msg = refused(
        &d,
        package(&d.cache(), &e, &d.local(), Pin::Index),
        "package: hash",
    );
    assert!(msg.contains("nothing was fetched"), "{msg}");
    let e = entry_for("absent.zip", &bytes);
    let msg = refused(
        &d,
        package(&d.cache(), &e, &d.local(), Pin::Index),
        "package: hash",
    );
    assert!(msg.contains("nothing was fetched"), "{msg}");
}

#[test]
fn install_refuses_a_symbolic_link_entry() {
    let d = Dir::new("link");
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("sdk.json", opts()).unwrap();
    z.write_all(sdk_json(&files()).as_bytes()).unwrap();
    for (n, b) in files() {
        z.start_file(n, opts()).unwrap();
        z.write_all(b).unwrap();
    }
    z.add_symlink("holotape-c/link", "../templates.json", opts())
        .unwrap();
    let bytes = z.finish().unwrap().into_inner();
    let msg = refused(&d, install_zip(&d, &bytes), "package: archive");
    assert!(msg.contains("holotape-c/link"), "{msg}");
}

/// A component that Windows reads as a drive, `C:payload`, is refused on
/// every system before anything is extracted.
#[test]
fn install_refuses_a_drive_in_a_component() {
    let d = Dir::new("drive");
    let mut put = files();
    put.push(("holotape-c/C:payload", b"x"));
    // Listed in `sdk.json` too, so only the name rule refuses it.
    let msg = refused(
        &d,
        install_zip(&d, &package_zip(&put, &put)),
        "package: archive",
    );
    assert!(msg.contains("holotape-c/C:payload"), "{msg}");
}

#[test]
fn install_refuses_a_dot_dot_path() {
    let d = Dir::new("dotdot");
    let mut put = files();
    put.push(("holotape-c/../../evil", b"x"));
    let msg = refused(
        &d,
        install_zip(&d, &package_zip(&files(), &put)),
        "package: archive",
    );
    assert!(msg.contains("holotape-c/../../evil"), "{msg}");
    assert!(!d.0.join("evil").exists());
}

#[test]
fn install_refuses_bad_names() {
    for name in ["/abs", "C:x", "a//b", "./a", "a\\b", "a/./b"] {
        let d = Dir::new("names");
        let mut put = files();
        put.push((name, b"x"));
        refused(
            &d,
            install_zip(&d, &package_zip(&files(), &put)),
            "package: archive",
        );
    }
}

#[test]
fn install_refuses_names_that_differ_in_case() {
    let d = Dir::new("case");
    let mut put = files();
    put.push(("Templates.json", b"x"));
    let msg = refused(
        &d,
        install_zip(&d, &package_zip(&put, &put)),
        "package: archive",
    );
    assert!(msg.contains("Templates.json"), "{msg}");
}

#[test]
fn install_refuses_names_that_differ_only_in_unicode_case() {
    for (i, (a, b)) in [("σ", "ς"), ("Å", "å"), ("straße", "STRASSE")]
        .into_iter()
        .enumerate()
    {
        let d = Dir::new(&format!("ucase{i}"));
        let mut put = files();
        put.push((a, b"x"));
        put.push((b, b"y"));
        let msg = refused(
            &d,
            install_zip(&d, &package_zip(&put, &put)),
            "package: archive",
        );
        assert!(msg.contains("differs only in case"), "{a} {b}: {msg}");
    }
}

#[test]
fn install_refuses_an_unlisted_file() {
    let d = Dir::new("unlisted");
    let mut put = files();
    put.push(("extra.txt", b"x"));
    let msg = refused(
        &d,
        install_zip(&d, &package_zip(&files(), &put)),
        "package: archive",
    );
    assert!(msg.contains("extra.txt"), "{msg}");
}

#[test]
fn install_refuses_a_wrong_file_hash() {
    let d = Dir::new("filehash");
    let mut put = files();
    put[1].1 = b"int main(void) { return 1; }\n";
    let msg = refused(
        &d,
        install_zip(&d, &package_zip(&files(), &put)),
        "package: hash",
    );
    assert!(msg.contains("holotape-c/src/main.c"), "{msg}");
}

#[test]
fn install_refuses_a_listed_file_that_is_absent() {
    let d = Dir::new("absent");
    let msg = refused(
        &d,
        install_zip(&d, &package_zip(&files(), &files()[..2])),
        "package: hash",
    );
    assert!(msg.contains("templates.json"), "{msg}");
}

#[test]
fn install_refuses_an_archive_without_sdk_json() {
    let d = Dir::new("nosdk");
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("templates.json", opts()).unwrap();
    z.write_all(b"{}").unwrap();
    let bytes = z.finish().unwrap().into_inner();
    refused(&d, install_zip(&d, &bytes), "package: archive");
    refused(&d, install_zip(&d, b"not a zip"), "package: archive");
}

#[test]
fn install_refuses_a_record_entry() {
    let d = Dir::new("record");
    let mut put = files();
    put.push((".robco-archive", b"x"));
    refused(
        &d,
        install_zip(&d, &package_zip(&files(), &put)),
        "package: archive",
    );
    let d = Dir::new("record-case");
    let mut put = files();
    put.push((".ROBCO-archive", b"x"));
    refused(
        &d,
        install_zip(&d, &package_zip(&files(), &put)),
        "package: archive",
    );
}

#[test]
fn install_refuses_a_file_that_is_also_a_folder() {
    let d = Dir::new("filedir");
    let mut put = files();
    put.push(("templates.json/x", b"x"));
    refused(
        &d,
        install_zip(&d, &package_zip(&files(), &put)),
        "package: archive",
    );
}

#[test]
fn install_refuses_sdk_json_that_differs_from_the_index() {
    let d = Dir::new("differs");
    let bytes = good_zip();
    d.put("p.zip", &bytes);
    let text = format!(
        r#"{{"format": 1, "tools": [], "packages": [{{
  "name": "templates", "version": "0.1.1",
  "formats": {{"templates": 1, "project": 1, "scenario": 1}},
  "url": "p.zip", "size": {}, "sha256": "{}"}}]}}"#,
        bytes.len(),
        sha256_hex(&bytes)
    );
    let e = index::parse(text.as_bytes(), IndexSource::Local)
        .unwrap()
        .packages
        .remove(0);
    let msg = refused(
        &d,
        package(&d.cache(), &e, &d.local(), Pin::Index),
        "package: manifest",
    );
    assert!(msg.contains("version"), "{msg}");
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("sdk.json", opts()).unwrap();
    z.write_all(b"{\"format\": 2}").unwrap();
    refused(
        &d,
        install_zip(&d, &z.finish().unwrap().into_inner()),
        "package: format",
    );
}

#[test]
fn install_replaces_an_unfinished_folder() {
    let d = Dir::new("unfinished");
    let dir = pkg_dir(&d);
    // As a crash or a hand would leave it: files with no record, or a bad one.
    for record in [None, Some("short\n"), Some(&*"A".repeat(64))] {
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("holotape-c")).unwrap();
        fs::write(dir.join("templates.json"), b"half").unwrap();
        if let Some(r) = record {
            fs::write(dir.join(RECORD), format!("{r}\n")).unwrap();
        }
        assert_eq!(cached(&dir), Ok(Cached::Unfinished));
        assert_eq!(install_zip(&d, &good_zip()), Ok(State::Installed));
        assert_eq!(
            fs::read(dir.join("templates.json")).unwrap(),
            b"{\"format\": 1}\n"
        );
        assert!(d.temp_is_empty());
    }
}

#[test]
fn install_leftover_temporary_folders_are_not_an_install() {
    let d = Dir::new("leftover");
    // A temporary folder of an install that was killed after its record.
    let left = d.cache().join(TEMP).join("1-0-0").join("x");
    fs::create_dir_all(&left).unwrap();
    fs::write(left.join(RECORD), format!("{}\n", sha256_hex(b"x"))).unwrap();
    assert_eq!(cached(&pkg_dir(&d)), Ok(Cached::Missing));
    assert_eq!(install_zip(&d, &good_zip()), Ok(State::Installed));
}

/// Waits at a barrier before it serves, so that two installs download at
/// the same time.
struct Together(LocalFiles, Arc<Barrier>);

impl Transport for Together {
    fn fetch(&self, url: &str, out: &mut dyn Write) -> Result<(), String> {
        self.1.wait();
        self.0.fetch(url, out)
    }
}

#[test]
fn install_two_at_once() {
    let d = Dir::new("race");
    let bytes = good_zip();
    d.put("p.zip", &bytes);
    let e = entry_for("p.zip", &bytes);
    let barrier = Arc::new(Barrier::new(2));
    let states: Vec<State> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..2)
            .map(|_| {
                let t = Together(d.local(), barrier.clone());
                let (cache, e) = (d.cache(), e.clone());
                s.spawn(move || package(&cache, &e, &t, Pin::Index))
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap().unwrap()).collect()
    });
    let mut states = states;
    states.sort_by_key(|s| *s == State::Present);
    assert_eq!(states, vec![State::Installed, State::Present]);
    assert_eq!(cached(&pkg_dir(&d)), Ok(Cached::Holds(sha256_hex(&bytes))));
    assert_eq!(
        fs::read(pkg_dir(&d).join("templates.json")).unwrap(),
        b"{\"format\": 1}\n"
    );
    assert!(d.temp_is_empty());
}

/// A transport whose download always fails, as a lost network would.
struct Offline;

impl Transport for Offline {
    fn fetch(&self, _url: &str, _out: &mut dyn Write) -> Result<(), String> {
        Err("offline".to_owned())
    }
}

/// Sets the hook of this thread: `f` runs once, at `step`.
fn hook_once(step: Step, f: impl FnOnce() + 'static) {
    let mut f = Some(f);
    HOOK.with(|h| {
        *h.borrow_mut() = Some(Box::new(move |s| {
            if s == step
                && let Some(f) = f.take()
            {
                f();
            }
        }));
    });
}

fn clear_hook() {
    HOOK.with(|h| *h.borrow_mut() = None);
}

/// An unfinished folder at the package's place, as a crash leaves it.
fn leave_unfinished(d: &Dir) {
    let dir = pkg_dir(d);
    fs::create_dir_all(dir.join("holotape-c")).unwrap();
    fs::write(dir.join("templates.json"), b"half").unwrap();
}

/// The package folder holds the install of `bytes`, whole.
fn holds_whole(d: &Dir, bytes: &[u8]) {
    assert_eq!(cached(&pkg_dir(d)), Ok(Cached::Holds(sha256_hex(bytes))));
    for (p, b) in files() {
        assert_eq!(fs::read(pkg_dir(d).join(p)).unwrap(), b, "{p}");
    }
    assert!(d.temp_is_empty(), "a temporary folder was left");
}

/// A and B both find the folder unfinished. B takes it aside and finishes
/// before A renames. A's rename takes B's finished install: A must put it
/// back, even though A's own download would fail.
#[test]
fn install_puts_back_a_finished_install_it_took_aside() {
    let d = Dir::new("aside-back");
    let bytes = good_zip();
    d.put("p.zip", &bytes);
    leave_unfinished(&d);
    let (cache, local) = (d.cache(), d.local());
    let e = entry_for("p.zip", &bytes);
    let b = e.clone();
    hook_once(Step::BeforeAside, move || {
        assert_eq!(
            package(&cache, &b, &local, Pin::Index),
            Ok(State::Installed)
        );
    });
    let r = package(&d.cache(), &e, &Offline, Pin::Index);
    clear_hook();
    assert_eq!(r, Ok(State::Present));
    holds_whole(&d, &bytes);
}

/// As above, with B's install of another archive: A puts it back too and
/// reports the mismatch, and the other archive stays.
#[test]
fn install_puts_back_another_finished_install_it_took_aside() {
    let d = Dir::new("aside-other");
    let bytes = good_zip();
    d.put("p.zip", &bytes);
    leave_unfinished(&d);
    let (cache, local) = (d.cache(), d.local());
    let b = entry_for("p.zip", &bytes);
    hook_once(Step::BeforeAside, move || {
        assert_eq!(
            package(&cache, &b, &local, Pin::Index),
            Ok(State::Installed)
        );
    });
    let other = entry_with("p.zip", 1, &sha256_hex(b"other"));
    let r = package(&d.cache(), &other, &Offline, Pin::Index);
    clear_hook();
    assert_eq!(r.unwrap_err().error_code(), "package: hash");
    holds_whole(&d, &bytes);
}

/// A took B's finished install aside, and C installed into the free place
/// before A puts it back. A keeps C's install and reports it.
#[test]
fn install_keeps_the_install_that_took_the_place() {
    let d = Dir::new("aside-taken");
    let bytes = good_zip();
    d.put("p.zip", &bytes);
    leave_unfinished(&d);
    let e = entry_for("p.zip", &bytes);
    let (cache, local, b) = (d.cache(), d.local(), e.clone());
    hook_once(Step::BeforeAside, move || {
        assert_eq!(
            package(&cache, &b, &local, Pin::Index),
            Ok(State::Installed)
        );
        let (cache, local, c) = (cache.clone(), local.clone(), b.clone());
        hook_once(Step::BeforePutBack, move || {
            assert_eq!(
                package(&cache, &c, &local, Pin::Index),
                Ok(State::Installed)
            );
        });
    });
    let r = package(&d.cache(), &e, &Offline, Pin::Index);
    clear_hook();
    assert_eq!(r, Ok(State::Present));
    holds_whole(&d, &bytes);
}

/// A took aside B's install of one archive, and C installed another archive
/// of the same package into the free place. A judges the record in place,
/// C's, against its own pin: a hash error, and C's folder stays.
#[test]
fn install_judges_the_install_in_place_not_the_one_it_held() {
    let d = Dir::new("aside-other");
    let mine = good_zip();
    // The same files in another order: another archive, another hash.
    let mut rev = files();
    rev.reverse();
    let theirs = package_zip(&files(), &rev);
    assert_ne!(sha256_hex(&mine), sha256_hex(&theirs));
    d.put("p.zip", &mine);
    d.put("q.zip", &theirs);
    leave_unfinished(&d);
    let e = entry_for("p.zip", &mine);
    let other = entry_for("q.zip", &theirs);
    let (cache, local, b) = (d.cache(), d.local(), e.clone());
    hook_once(Step::BeforeAside, move || {
        assert_eq!(
            package(&cache, &b, &local, Pin::Index),
            Ok(State::Installed)
        );
        let (cache, local) = (cache.clone(), local.clone());
        hook_once(Step::BeforePutBack, move || {
            assert_eq!(
                package(&cache, &other, &local, Pin::Index),
                Ok(State::Installed)
            );
        });
    });
    let r = package(&d.cache(), &e, &Offline, Pin::Index);
    clear_hook();
    assert_eq!(r.unwrap_err().error_code(), "package: hash");
    holds_whole(&d, &theirs);
}

/// The join refuses, by itself, a name the system reads as more than one
/// plain component, whatever the name rule says.
#[test]
fn install_join_refuses_what_is_not_one_plain_name() {
    let base = Path::new("base").join("x");
    let abs = std::env::temp_dir();
    let abs = abs.to_str().unwrap();
    for bad in [abs, "..", ".", "", "a/b"] {
        assert_eq!(join_parts(&base, &[bad], || "bad"), Err("bad"), "{bad:?}");
        assert_eq!(
            join_parts(&base, &["ok", bad], || "bad"),
            Err("bad"),
            "{bad:?}"
        );
    }
    assert_eq!(join_inside(&base, "a//b", || "bad"), Err("bad"));
    assert_eq!(join_inside(&base, "a/../b", || "bad"), Err("bad"));
    assert_eq!(
        join_inside(&base, "a/b.c", || "bad"),
        Ok(base.join("a").join("b.c"))
    );
    assert_eq!(join_inside(&base, "", || "bad"), Ok(base.clone()));
}

/// On Windows, `C:payload` is a path on drive `C:`: a push of it would
/// drop the base. The join refuses it, and so does a whole entry name.
#[cfg(windows)]
#[test]
fn install_join_refuses_a_drive_relative_name() {
    let base = std::env::temp_dir().join("x");
    for bad in [
        "C:payload",
        "dir/C:payload",
        "C:\\payload",
        "\\\\srv\\share\\x",
    ] {
        assert_eq!(join_inside(&base, bad, || "bad"), Err("bad"), "{bad:?}");
    }
}

/// B takes the unfinished folder aside before A does: A's rename finds
/// nothing, and A installs into the free place.
#[test]
fn install_finds_the_unfinished_folder_gone() {
    let d = Dir::new("aside-gone");
    let bytes = good_zip();
    d.put("p.zip", &bytes);
    leave_unfinished(&d);
    let (dir, gone) = (pkg_dir(&d), d.0.join("gone"));
    hook_once(Step::BeforeAside, move || fs::rename(dir, gone).unwrap());
    let r = package(
        &d.cache(),
        &entry_for("p.zip", &bytes),
        &d.local(),
        Pin::Index,
    );
    clear_hook();
    assert_eq!(r, Ok(State::Installed));
    holds_whole(&d, &bytes);
}

// Tools.

fn archive_of(url: &str, bytes: &[u8], root: &str) -> Archive {
    Archive {
        url: url.to_owned(),
        size: bytes.len() as u64,
        sha256: sha256_hex(bytes),
        root: root.to_owned(),
    }
}

fn tool_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        if name.ends_with('/') {
            z.add_directory(*name, opts()).unwrap();
        } else {
            z.start_file(*name, opts().unix_permissions(0o755)).unwrap();
            z.write_all(bytes).unwrap();
        }
    }
    z.finish().unwrap().into_inner()
}

enum T<'a> {
    File(&'a str, &'a [u8]),
    Dir(&'a str),
    Link(&'a str, &'a str),
}

/// A tar.gz whose names are written raw, so a test can give a `..` name.
fn tool_tgz(entries: &[T<'_>]) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut b = tar::Builder::new(gz);
    for e in entries {
        let mut h = tar::Header::new_gnu();
        let (name, data): (&str, &[u8]) = match e {
            T::File(n, d) => {
                h.set_entry_type(tar::EntryType::Regular);
                h.set_mode(0o755);
                (n, d)
            }
            T::Dir(n) => {
                h.set_entry_type(tar::EntryType::Directory);
                h.set_mode(0o755);
                (n, b"")
            }
            T::Link(n, target) => {
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_mode(0o777);
                let t = target.as_bytes();
                h.as_old_mut().linkname[..t.len()].copy_from_slice(t);
                (n, b"")
            }
        };
        h.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
        h.set_size(data.len() as u64);
        h.set_cksum();
        b.append(&h, data).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap()
}

fn tool_path_of(d: &Dir) -> PathBuf {
    tool_dir(&d.cache(), "gcc", "15.2.1-1.1", "linux-x64").unwrap()
}

fn install_tool(d: &Dir, bytes: &[u8], root: &str) -> Result<State, Error> {
    d.put("gcc.archive", bytes);
    let a = archive_of("gcc.archive", bytes, root);
    tool(
        &d.cache(),
        "gcc",
        "15.2.1-1.1",
        "linux-x64",
        &a,
        &d.local(),
        Pin::Index,
    )
}

fn tool_refused(d: &Dir, r: Result<State, Error>, code: &str) -> String {
    let e = r.unwrap_err();
    assert_eq!(e.error_code(), code, "{e}");
    assert!(!tool_path_of(d).exists(), "a tool folder was left");
    assert!(d.temp_is_empty(), "a temporary folder was left");
    e.to_string()
}

#[test]
fn install_a_zip_tool_at_its_root() {
    let d = Dir::new("tool-zip");
    let bytes = tool_zip(&[
        ("xpack-gcc/", b""),
        ("xpack-gcc/bin/", b""),
        ("xpack-gcc/bin/arm-none-eabi-gcc", b"#!"),
        ("xpack-gcc/README", b"r"),
        ("other/file", b"o"),
    ]);
    assert_eq!(install_tool(&d, &bytes, "xpack-gcc"), Ok(State::Installed));
    let dir = tool_path_of(&d);
    assert_eq!(fs::read(dir.join("bin/arm-none-eabi-gcc")).unwrap(), b"#!");
    assert!(!dir.join("other").exists());
    assert_eq!(cached(&dir), Ok(Cached::Holds(sha256_hex(&bytes))));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = fs::metadata(dir.join("bin/arm-none-eabi-gcc")).unwrap();
        assert_eq!(m.permissions().mode() & 0o111, 0o111);
    }
    assert!(d.temp_is_empty());
}

#[test]
fn install_a_tar_tool_with_an_empty_root_and_a_link_inside() {
    let d = Dir::new("tool-tar");
    let bytes = tool_tgz(&[
        T::Dir("./bin/"),
        T::File("./bin/qemu-system-arm", b"q"),
        T::Link("./bin/qemu", "qemu-system-arm"),
    ]);
    assert_eq!(install_tool(&d, &bytes, ""), Ok(State::Installed));
    let dir = tool_path_of(&d);
    assert_eq!(fs::read(dir.join("bin/qemu")).unwrap(), b"q");
    assert!(
        fs::symlink_metadata(dir.join("bin/qemu"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

/// Every component of a tool entry, and of a link target, must be one
/// plain name on every system.
#[test]
fn install_refuses_tool_names_that_are_not_plain() {
    let cases = [
        tool_tgz(&[T::Dir("bin/"), T::File("bin/C:payload", b"x")]),
        tool_tgz(&[T::Dir("bin/"), T::File("bin/nul", b"x")]),
        tool_tgz(&[T::Dir("bin/"), T::Dir("a /"), T::File("a /x", b"x")]),
        tool_zip(&[("bin/", b""), ("bin/a:b", b"x")]),
    ];
    for bytes in cases {
        let d = Dir::new("tool-plain");
        tool_refused(&d, install_tool(&d, &bytes, ""), "tool: archive");
    }
    // The system would refuse this link only because `nul` is missing
    // here: the rule refuses it by its name, as Windows needs.
    let d = Dir::new("tool-plain-link");
    let bytes = tool_tgz(&[
        T::Dir("bin/"),
        T::File("bin/y", b"x"),
        T::Link("bin/l", "nul/../y"),
    ]);
    let msg = tool_refused(&d, install_tool(&d, &bytes, ""), "tool: archive");
    assert!(msg.contains("not plain"), "{msg}");
}

#[test]
fn install_refuses_tool_entries_outside() {
    let cases = [
        tool_tgz(&[T::Dir("bin/"), T::File("../evil", b"x")]),
        tool_tgz(&[T::Dir("bin/"), T::File("/evil", b"x")]),
        tool_tgz(&[T::Dir("bin/"), T::Link("bin/out", "../../evil")]),
        tool_tgz(&[T::Dir("bin/"), T::Link("bin/abs", "/etc/hosts")]),
        tool_tgz(&[T::Dir("bin/"), T::Link("bin/none", "missing")]),
        // A link to a folder inside, then a file through it.
        tool_tgz(&[
            T::Dir("bin/"),
            T::Dir("lib/"),
            T::Link("l", "lib"),
            T::File("l/x", b"x"),
        ]),
        tool_zip(&[("bin/", b""), ("../evil", b"x")]),
    ];
    for bytes in cases {
        let d = Dir::new("tool-out");
        tool_refused(&d, install_tool(&d, &bytes, ""), "tool: archive");
        assert!(!d.0.join("evil").exists());
    }
}

#[test]
fn install_refuses_a_bad_tool_root() {
    let d = Dir::new("tool-root");
    let bytes = tool_zip(&[("a/bin/x", b"x")]);
    let msg = tool_refused(&d, install_tool(&d, &bytes, "b"), "tool: archive");
    assert!(msg.contains("root"), "{msg}");
    let bytes = tool_zip(&[("a/lib/x", b"x")]);
    let msg = tool_refused(&d, install_tool(&d, &bytes, "a"), "tool: archive");
    assert!(msg.contains("bin/"), "{msg}");
}

#[test]
fn install_refuses_a_tool_record_entry_and_a_non_archive() {
    let d = Dir::new("tool-rec");
    let bytes = tool_zip(&[("bin/x", b"x"), (".robco-archive", b"x")]);
    tool_refused(&d, install_tool(&d, &bytes, ""), "tool: archive");
    tool_refused(&d, install_tool(&d, b"plain bytes", ""), "tool: archive");
    let mut cut = tool_tgz(&[T::Dir("bin/"), T::File("bin/x", &[7u8; 4096])]);
    cut.truncate(cut.len() / 2);
    tool_refused(&d, install_tool(&d, &cut, ""), "tool: archive");
}

#[test]
fn install_refuses_a_wrong_tool_hash() {
    let d = Dir::new("tool-hash");
    let bytes = tool_zip(&[("bin/x", b"x")]);
    d.put("gcc.archive", &bytes);
    let mut a = archive_of("gcc.archive", &bytes, "");
    a.size += 1;
    let r = tool(
        &d.cache(),
        "gcc",
        "15.2.1-1.1",
        "linux-x64",
        &a,
        &d.local(),
        Pin::Index,
    );
    tool_refused(&d, r, "tool: hash");
    install_tool(&d, &bytes, "").unwrap();
    let other = archive_of("gcc.archive", b"other", "");
    let r = tool(
        &d.cache(),
        "gcc",
        "15.2.1-1.1",
        "linux-x64",
        &other,
        &d.local(),
        Pin::Index,
    );
    assert_eq!(r.unwrap_err().error_code(), "tool: hash");
    let r = tool(
        &d.cache(),
        "gcc",
        "15.2.1-1.1",
        "linux-x64",
        &other,
        &d.local(),
        Pin::Lock,
    );
    assert_eq!(r.unwrap_err().error_code(), "lock: hash");
}

/// A folder of the user's beside the cache, with a `bin/` in it.
fn side_folder(d: &Dir) -> PathBuf {
    let side = d.0.join("side");
    fs::create_dir_all(side.join("bin")).unwrap();
    fs::write(side.join("bin/own"), b"mine").unwrap();
    side
}

/// The side folder is where it was, as it was.
fn side_untouched(side: &Path) {
    let mut names: Vec<_> = fs::read_dir(side)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["bin"]);
    let bin: Vec<_> = fs::read_dir(side.join("bin")).unwrap().collect();
    assert_eq!(bin.len(), 1);
    assert_eq!(fs::read(side.join("bin/own")).unwrap(), b"mine");
}

#[test]
fn install_refuses_a_tool_root_outside_the_extraction_folder() {
    let d = Dir::new("tool-root-out");
    let side = side_folder(&d);
    // The extraction folder is `<cache>/tmp/<one>/x`: four `..` reach `d`.
    let bytes = tool_zip(&[("bin/x", b"x")]);
    let msg = tool_refused(
        &d,
        install_tool(&d, &bytes, "../../../../side"),
        "tool: archive",
    );
    assert!(msg.contains("path rules"), "{msg}");
    side_untouched(&side);
}

#[test]
fn install_refuses_a_tool_root_outside_the_path_rules() {
    // Each one stays inside once resolved; the form alone refuses it.
    let bytes = tool_zip(&[("pkg/bin/x", b"x")]);
    for root in ["pkg/../pkg", "./pkg", "pkg/", "pkg\\.", "/pkg"] {
        let d = Dir::new("tool-root-form");
        let msg = tool_refused(&d, install_tool(&d, &bytes, root), "tool: archive");
        assert!(msg.contains("path rules"), "{root}: {msg}");
    }
}

#[cfg(unix)]
#[test]
fn install_refuses_a_tool_root_that_resolves_outside() {
    let d = Dir::new("tool-root-link");
    let side = side_folder(&d);
    // `l` leads from the extraction folder to `d`; `l/side` is the user's.
    let bytes = tool_tgz(&[T::Dir("bin/"), T::Link("l", "../../../..")]);
    let msg = tool_refused(&d, install_tool(&d, &bytes, "l/side"), "tool: archive");
    assert!(msg.contains("outside"), "{msg}");
    side_untouched(&side);
}

#[cfg(unix)]
#[test]
fn install_refuses_a_link_out_of_the_installed_folder() {
    let d = Dir::new("tool-link-sib");
    let bytes = tool_tgz(&[
        T::Dir("pkg/bin/"),
        T::Dir("shared/"),
        T::File("shared/gcc", b"g"),
        T::Link("pkg/bin/gcc", "../../shared/gcc"),
    ]);
    let msg = tool_refused(&d, install_tool(&d, &bytes, "pkg"), "tool: archive");
    assert!(msg.contains("installed folder"), "{msg}");
    // A link that stays inside `pkg` is good.
    let bytes = tool_tgz(&[
        T::Dir("pkg/bin/"),
        T::File("pkg/lib", b"g"),
        T::Link("pkg/bin/gcc", "../lib"),
    ]);
    assert_eq!(install_tool(&d, &bytes, "pkg"), Ok(State::Installed));
    assert_eq!(fs::read(tool_path_of(&d).join("bin/gcc")).unwrap(), b"g");
}

#[cfg(unix)]
#[test]
fn install_refuses_a_link_outside_in_a_folder_root_leaves_out() {
    let d = Dir::new("tool-link-left");
    let side = side_folder(&d);
    // `pkg` is installed; `other/out` leads from the extraction folder,
    // `<cache>/tmp/<one>/x`, to the user's folder beside the cache.
    let bytes = tool_tgz(&[
        T::Dir("pkg/bin/"),
        T::File("pkg/bin/gcc", b"g"),
        T::Dir("other/"),
        T::Link("other/out", "../../../../../side"),
    ]);
    let msg = tool_refused(&d, install_tool(&d, &bytes, "pkg"), "tool: archive");
    assert!(msg.contains("extraction folder"), "{msg}");
    side_untouched(&side);
}

#[cfg(unix)]
#[test]
fn install_writes_nothing_through_a_link_in_a_folder_root_leaves_out() {
    let d = Dir::new("tool-link-write");
    let side = side_folder(&d);
    // Through a link to the user's folder, and through one that stays
    // inside: both are refused before anything is written.
    for (target, name) in [("../../../../../side", "bin/evil"), ("../pkg", "bin/x")] {
        let through = format!("other/l/{name}");
        let bytes = tool_tgz(&[
            T::Dir("pkg/bin/"),
            T::Dir("other/"),
            T::Link("other/l", target),
            T::File(&through, b"x"),
        ]);
        tool_refused(&d, install_tool(&d, &bytes, "pkg"), "tool: archive");
    }
    side_untouched(&side);
}

struct Failing;

impl io::Read for Failing {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("broken stream"))
    }
}

impl io::Write for Failing {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::from(io::ErrorKind::StorageFull))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_tool_entry_tells_a_read_failure_from_a_write_failure() {
    let path = Path::new("bin/x");
    let mut sink = Vec::new();
    let e = copy_entry("bin/x", path, &mut Failing, &mut sink).unwrap_err();
    assert_eq!(e.error_code(), "tool: archive");
    let e = copy_entry("bin/x", path, &mut &b"data"[..], &mut Failing).unwrap_err();
    assert_eq!(e.error_code(), "io: write");
}

#[test]
fn install_refuses_names_that_are_not_cache_folder_names() {
    let d = Dir::new("tool-names");
    let bytes = tool_zip(&[("bin/x", b"x")]);
    d.put("gcc.archive", &bytes);
    let a = archive_of("gcc.archive", &bytes, "");
    let cases = [
        ("gcc", "..", "linux-x64"),
        ("gcc", ".", "linux-x64"),
        ("gcc", "1/../../..", "linux-x64"),
        ("../gcc", "1", "linux-x64"),
        ("gcc", "1", ".."),
    ];
    for (name, version, host) in cases {
        let r = tool(&d.cache(), name, version, host, &a, &d.local(), Pin::Index);
        assert_eq!(r.unwrap_err().error_code(), "tool: archive", "{version}");
        assert!(d.temp_is_empty());
    }
    assert_eq!(fs::read_dir(d.cache()).unwrap().count(), 0);
    let mut e = entry_for("pkg.zip", b"x");
    e.name = "../x".to_owned();
    let r = package(&d.cache(), &e, &d.local(), Pin::Index);
    assert_eq!(r.unwrap_err().error_code(), "package: archive");
}

#[test]
fn cache_folder_names_are_the_same_on_every_system() {
    // The spec's example index and the pinned tool versions.
    for ok in [
        "15.2.1-1.1",
        "10.1.2",
        "1.3.0",
        "0.1.0",
        "1.0.0",
        "1.4.0",
        "1.5.2",
        "2.0.0",
        "console",
        "com10",
        "lpt0",
    ] {
        assert!(is_folder_name(ok), "{ok}");
    }
    for bad in [
        "1:2",
        "1*",
        "1?",
        "1<2",
        "1>2",
        "1|2",
        "1\"2",
        "1/2",
        "1\\2",
        "1\u{1}",
        "1.",
        "1 ",
        ".",
        "..",
        "CON",
        "con",
        "Nul.1",
        "aux.tar.gz",
        "COM1",
        "lpt9.0",
        "prn",
    ] {
        assert!(!is_portable(bad), "{bad}");
    }
}

#[test]
fn install_refuses_a_name_that_is_not_a_folder_on_every_system() {
    let d = Dir::new("tool-portable");
    let bytes = tool_zip(&[("bin/x", b"x")]);
    d.put("gcc.archive", &bytes);
    let a = archive_of("gcc.archive", &bytes, "");
    let cases = [
        ("gcc", "1:2", "linux-x64"),
        ("gcc", "1*", "linux-x64"),
        ("gcc", "1?", "linux-x64"),
        ("gcc", "1<2>", "linux-x64"),
        ("gcc", "1|2", "linux-x64"),
        ("gcc", "1.2.", "linux-x64"),
        ("gcc", "CON", "linux-x64"),
        ("gcc", "nul.1", "linux-x64"),
        ("gcc", "COM3", "linux-x64"),
        ("gcc", "1", "aux"),
        ("con", "1", "linux-x64"),
    ];
    for (name, version, host) in cases {
        let r = tool(&d.cache(), name, version, host, &a, &d.local(), Pin::Index);
        let e = r.unwrap_err();
        assert_eq!(e.error_code(), "tool: archive", "{version}");
        assert!(e.to_string().contains("not a cache folder name"), "{e}");
        assert!(d.temp_is_empty());
    }
    assert_eq!(fs::read_dir(d.cache()).unwrap().count(), 0);
    let mut e = entry_for("pkg.zip", b"x");
    e.name = "nul".to_owned();
    let r = package(&d.cache(), &e, &d.local(), Pin::Index);
    assert_eq!(r.unwrap_err().error_code(), "package: archive");
    assert_eq!(fs::read_dir(d.cache()).unwrap().count(), 0);
}

// Links of tool archives, made after every file and folder.

/// True when `p` is a link the system can follow to a folder.
#[cfg(unix)]
fn is_dir_link(p: &Path) -> bool {
    fs::symlink_metadata(p).unwrap().file_type().is_symlink() && p.is_dir()
}

#[cfg(unix)]
#[test]
fn install_a_tool_link_listed_before_its_folder() {
    let d = Dir::new("link-first");
    let bytes = tool_tgz(&[
        T::Link("lib-link", "lib"),
        T::Dir("bin/"),
        T::Dir("lib/"),
        T::File("lib/a", b"a"),
    ]);
    assert_eq!(install_tool(&d, &bytes, ""), Ok(State::Installed));
    let dir = tool_path_of(&d);
    assert!(is_dir_link(&dir.join("lib-link")));
    assert_eq!(fs::read(dir.join("lib-link/a")).unwrap(), b"a");
}

#[cfg(unix)]
#[test]
fn install_a_tool_link_to_a_link_in_both_orders() {
    let tail = [T::Dir("bin/"), T::Dir("lib/"), T::File("lib/a", b"a")];
    for first in [true, false] {
        let d = Dir::new("link-chain");
        let (outer, inner) = (T::Link("outer", "inner"), T::Link("inner", "lib"));
        let mut e: Vec<T<'_>> = if first {
            vec![outer, inner]
        } else {
            vec![inner, outer]
        };
        e.extend(tail.iter().map(|t| match t {
            T::Dir(n) => T::Dir(n),
            T::File(n, b) => T::File(n, b),
            T::Link(n, t) => T::Link(n, t),
        }));
        assert_eq!(install_tool(&d, &tool_tgz(&e), ""), Ok(State::Installed));
        let dir = tool_path_of(&d);
        assert!(is_dir_link(&dir.join("outer")));
        assert_eq!(fs::read(dir.join("outer/a")).unwrap(), b"a");
    }
}

#[cfg(unix)]
#[test]
fn install_refuses_a_cycle_of_tool_links() {
    let d = Dir::new("link-cycle");
    let bytes = tool_tgz(&[T::Dir("bin/"), T::Link("bin/x", "y"), T::Link("bin/y", "x")]);
    let m = tool_refused(&d, install_tool(&d, &bytes, ""), "tool: archive");
    assert!(m.contains("cycle"), "{m}");
}

#[cfg(unix)]
#[test]
fn install_refuses_a_tool_link_to_an_unlisted_target() {
    let d = Dir::new("link-nowhere");
    let bytes = tool_tgz(&[T::Dir("bin/"), T::Link("bin/x", "missing")]);
    let m = tool_refused(&d, install_tool(&d, &bytes, ""), "tool: archive");
    assert!(m.contains("nothing"), "{m}");
}

/// The kind and order of links follow from the entries alone, on every
/// system: whatever the order of the entries, a folder target gives a
/// folder link, a file target a file link, and a link comes after the
/// links its target passes through.
#[test]
fn links_take_the_kind_of_their_target_in_any_order() {
    let link = |n: &str, t: &str| (n.to_owned(), Entry::Link(t.to_owned()));
    let entries = vec![
        link("to-dir", "lib"),
        link("to-file", "lib/a"),
        link("to-link", "to-dir"),
        link("through", "to-dir/a"),
        ("lib/".to_owned(), Entry::Dir),
        ("lib/a".to_owned(), Entry::File(None)),
    ];
    let want = |n: &str, t: &str, dir: bool| Link {
        name: n.to_owned(),
        target: t.to_owned(),
        dir,
    };
    let mut reversed = entries.clone();
    reversed.reverse();
    let forward = plan_links(&entries).unwrap();
    assert_eq!(
        forward,
        vec![
            want("to-dir", "lib", true),
            want("to-file", "lib/a", false),
            want("to-link", "to-dir", true),
            want("through", "to-dir/a", false),
        ]
    );
    let backward = plan_links(&reversed).unwrap();
    assert_eq!(
        backward,
        vec![
            want("to-dir", "lib", true),
            want("through", "to-dir/a", false),
            want("to-link", "to-dir", true),
            want("to-file", "lib/a", false),
        ]
    );
    let cycle = [link("x", "y"), link("y", "x")];
    assert_eq!(
        plan_links(&cycle).unwrap_err().error_code(),
        "tool: archive"
    );
    let out = [link("x", "../up")];
    assert_eq!(plan_links(&out).unwrap_err().error_code(), "tool: archive");
    let nowhere = [link("x", "missing")];
    assert_eq!(
        plan_links(&nowhere).unwrap_err().error_code(),
        "tool: archive"
    );
}

// Failures of the cache file while a download streams.

/// A file that takes every byte and fails on `flush`.
struct FailsOnFlush;

impl Write for FailsOnFlush {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::StorageFull))
    }
}

/// Writes every byte, then flushes, as a transport that buffers does.
struct Flushes(Vec<u8>);

impl Transport for Flushes {
    fn fetch(&self, _url: &str, out: &mut dyn Write) -> Result<(), String> {
        out.write_all(&self.0).map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())
    }
}

#[test]
fn install_reports_a_failed_flush_as_a_write_error() {
    let bytes = good_zip();
    let sha = sha256_hex(&bytes);
    let item = Item {
        kind: Kind::Package,
        label: "package templates 0.1.0".to_owned(),
        dir: PathBuf::from("unused"),
        url: "p.zip",
        size: bytes.len() as u64,
        sha256: &sha,
    };
    let part = Path::new("archive.part");
    let r = fetch_checked(part, &item, &Flushes(bytes.clone()), FailsOnFlush);
    let e = r.err().unwrap();
    assert_eq!(e.error_code(), "io: write", "{e}");
    // The same transport with a file that flushes is a match.
    assert!(fetch_checked(part, &item, &Flushes(bytes), Vec::new()).is_ok());
}
