// SPDX-License-Identifier: GPL-3.0-or-later

//! Downloads, checks and installs SDK packages and tools into the cache.
//!
//! Implements RobCo-Core `docs/spec/SDK_PACKAGE.md`, sections "The archive",
//! "The cache folder", "What the cache records", "Reading sdk.json", "What
//! must equal the index entry", "Tool archives" and "Hashes", as read at
//! RobCo-Core commit `e004c73`.
//!
//! A command calls [`package`] and [`tool`] between resolving and writing
//! the lock: steps 7 and 9 of "The steps of update, build, run and test".
//! The lock is written only once every call returned `Ok`.
//!
//! The rules that keep the cache whole:
//!
//! - A download streams into a temporary file while it is hashed and
//!   counted. Only a match of size and hash renames it to the archive that
//!   is read next. Anything else, a download that could not be fetched
//!   included, is a mismatch.
//! - Everything is extracted and checked in a temporary folder under
//!   `<cache>/tmp/`. `.robco-archive` is written there last, and one rename
//!   moves the folder into the cache. So a cache folder either holds every
//!   checked file and its record, or it is not there.
//! - A cache folder without a valid record is not an install that ended.
//!   The next install renames it into its temporary folder, reads it there
//!   and removes it only when it is still unfinished. A finished install
//!   that another install put in its place meanwhile goes back.
//! - When two installs of one item race, the second rename fails. The
//!   second install then reads the first one's record and compares it.
//! - A failure removes the temporary folder. Nothing is retried from
//!   another place.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::codes::ErrorCode;
use crate::resolve::index::{Archive, IndexPackage, is_archive_path, is_root};
use crate::resolve::sdk_json;
use crate::resolve::{is_host_name, is_package_name, is_tool_version};

/// The file of "What the cache records", right under an installed folder.
pub const RECORD: &str = ".robco-archive";

/// The folder of `<cache>` that holds the temporary folders of installs.
pub const TEMP: &str = "tmp";

/// The name of the manifest at the root of a package archive.
const MANIFEST: &str = "sdk.json";

/// Fetches the bytes of an archive. The caller passes one in, so tests read
/// local files and never touch the network.
pub trait Transport {
    /// Writes every byte at `url` into `out`, in order. `url` is the `url`
    /// of an index entry: `https://` or a path relative to the index folder.
    /// An error means the bytes could not all be fetched; its text says why.
    fn fetch(&self, url: &str, out: &mut dyn Write) -> Result<(), String>;
}

/// The transport of a local index: a relative `url` is a file in the folder
/// of the index file. It fetches nothing from an `https://` URL, so a local
/// index that lists one gives a hash error that says nothing was fetched,
/// as "Without a network" in `STUDIO_CLI.md` asks.
#[derive(Debug, Clone)]
pub struct LocalFiles {
    /// The folder of the index file.
    pub base: PathBuf,
}

impl Transport for LocalFiles {
    fn fetch(&self, url: &str, out: &mut dyn Write) -> Result<(), String> {
        if url.starts_with("https://") || !is_archive_path(url) {
            return Err(format!(
                "nothing was fetched from {url}: no network transport"
            ));
        }
        let path = self.base.join(url);
        let mut file = File::open(&path)
            .map_err(|e| format!("nothing was fetched from {}: {}", path.display(), e.kind()))?;
        io::copy(&mut file, out)
            .map(|_| ())
            .map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// What the hash of a cache record is compared with, and so which code a
/// mismatch of an item already in the cache gives, by "Hashes".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pin {
    /// The command resolved from the index: `package: hash` or `tool: hash`.
    Index,
    /// The command read a lock: `lock: hash`.
    Lock,
}

/// What an install did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// This call put the item into the cache.
    Installed,
    /// The cache already held it, with the expected record. It was neither
    /// downloaded nor changed.
    Present,
}

/// Why an install failed. Each variant is one code of the spec's table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `package: archive`. The archive breaks a rule of "The archive".
    /// Names the entry.
    PackageArchive(String),
    /// `package: hash`. The download or a file does not match, or the cache
    /// holds another archive of the package.
    PackageHash(String),
    /// `package: manifest` or `package: format`, from reading `sdk.json` and
    /// comparing it with the index entry.
    Manifest(crate::resolve::Error),
    /// `tool: archive`. The tool archive cannot be extracted or does not
    /// hold what the index says.
    ToolArchive(String),
    /// `tool: hash`. The download does not match, or the cache holds another
    /// archive of the tool.
    ToolHash(String),
    /// `lock: hash`. The cache holds another archive than the lock pins.
    LockHash(String),
    /// `io: read`. The host refused to read a file of the cache.
    IoRead {
        /// The file.
        path: String,
        /// What the host said.
        reason: String,
    },
    /// `io: write`. The host refused to write, create, rename or remove a
    /// file of the cache.
    IoWrite {
        /// The file.
        path: String,
        /// What the host said.
        reason: String,
    },
}

impl ErrorCode for Error {
    fn error_code(&self) -> &'static str {
        match self {
            Self::PackageArchive(_) => "package: archive",
            Self::PackageHash(_) => "package: hash",
            Self::Manifest(e) => e.code(),
            Self::ToolArchive(_) => "tool: archive",
            Self::ToolHash(_) => "tool: hash",
            Self::LockHash(_) => "lock: hash",
            Self::IoRead { .. } => "io: read",
            Self::IoWrite { .. } => "io: write",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(e) => write!(f, "{e}"),
            Self::PackageArchive(d)
            | Self::PackageHash(d)
            | Self::ToolArchive(d)
            | Self::ToolHash(d)
            | Self::LockHash(d) => write!(f, "{}: {d}", self.error_code()),
            Self::IoRead { path, reason } | Self::IoWrite { path, reason } => {
                write!(f, "{}: {path}: {reason}", self.error_code())
            }
        }
    }
}

impl std::error::Error for Error {}

fn write_err(path: &Path, e: &io::Error) -> Error {
    Error::IoWrite {
        path: path.display().to_string(),
        reason: e.kind().to_string(),
    }
}

/// The folder of an installed package: `<cache>/packages/<name>/<version>/`.
pub fn package_dir(cache: &Path, name: &str, version: &str) -> PathBuf {
    cache.join("packages").join(name).join(version)
}

/// The folder of an installed tool: `<cache>/tools/<name>/<version>/<host>/`.
pub fn tool_dir(cache: &Path, name: &str, version: &str, host: &str) -> PathBuf {
    cache.join("tools").join(name).join(version).join(host)
}

/// The SHA-256 of some bytes, as 64 lower case hex digits.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(digest: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(64);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// What the cache holds in one item folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cached {
    /// No folder.
    Missing,
    /// A folder without a record in the form of "What the cache records":
    /// not an install that ended. The next install removes it first.
    Unfinished,
    /// A finished install, with the archive hash of its record.
    Holds(String),
}

/// Reads the record of an item folder. It changes nothing.
pub fn cached(dir: &Path) -> Result<Cached, Error> {
    match fs::symlink_metadata(dir) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Cached::Missing),
        Err(e) => return Err(read_err(dir, &e)),
        Ok(m) if !m.is_dir() => return Ok(Cached::Unfinished),
        Ok(_) => {}
    }
    let record = dir.join(RECORD);
    match fs::symlink_metadata(&record) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Cached::Unfinished),
        Err(e) => return Err(read_err(&record, &e)),
        Ok(m) if !m.is_file() || m.len() != 65 => return Ok(Cached::Unfinished),
        Ok(_) => {}
    }
    let bytes = fs::read(&record).map_err(|e| read_err(&record, &e))?;
    match bytes.split_last() {
        Some((b'\n', hash)) => match std::str::from_utf8(hash) {
            Ok(h) if is_hash(h) => Ok(Cached::Holds(h.to_owned())),
            _ => Ok(Cached::Unfinished),
        },
        _ => Ok(Cached::Unfinished),
    }
}

fn read_err(path: &Path, e: &io::Error) -> Error {
    Error::IoRead {
        path: path.display().to_string(),
        reason: e.kind().to_string(),
    }
}

/// Which kind of item an install works on: it picks the codes.
#[derive(Clone, Copy)]
enum Kind {
    Package,
    Tool,
}

impl Kind {
    fn hash(self, d: String) -> Error {
        match self {
            Self::Package => Error::PackageHash(d),
            Self::Tool => Error::ToolHash(d),
        }
    }
    fn archive(self, d: String) -> Error {
        match self {
            Self::Package => Error::PackageArchive(d),
            Self::Tool => Error::ToolArchive(d),
        }
    }
}

/// One item to install: what the index says about its archive.
struct Item<'a> {
    kind: Kind,
    /// `pipos 1.4.0` or `gcc 15.2.1-1.1 linux-x64`, for messages.
    label: String,
    dir: PathBuf,
    url: &'a str,
    size: u64,
    sha256: &'a str,
}

/// Installs one package into `<cache>/packages/<name>/<version>/`, or finds
/// it there. `entry` is the index entry the resolution or the lock picked.
///
/// A folder with a record is compared and never changed: a record that
/// differs from `entry.sha256` gives `package: hash`, or `lock: hash` when
/// `pin` is [`Pin::Lock`]. Otherwise the archive is downloaded, checked by
/// "The archive", its `sdk.json` read and compared with `entry`, each file
/// checked against `files`, and the folder moved into the cache.
pub fn package(
    cache: &Path,
    entry: &IndexPackage,
    transport: &dyn Transport,
    pin: Pin,
) -> Result<State, Error> {
    let version = entry.version.to_string();
    // The cache folder names come from the entry: each must be one plain
    // folder name. A version prints as digits, letters, `-`, `+` and `.`.
    if !is_package_name(&entry.name) || !is_folder_name(&version) {
        return Err(Error::PackageArchive(format!(
            "package {} {version}: not a cache folder name",
            entry.name
        )));
    }
    let item = Item {
        kind: Kind::Package,
        label: format!("package {} {version}", entry.name),
        dir: package_dir(cache, &entry.name, &version),
        url: &entry.url,
        size: entry.size,
        sha256: &entry.sha256,
    };
    install(cache, &item, transport, pin, &|archive, out| {
        extract_package(archive, out, entry)?;
        Ok(out.to_path_buf())
    })
}

/// Installs one tool archive for one host into
/// `<cache>/tools/<name>/<version>/<host>/`, or finds it there. `archive` is
/// the archive object of the index for that host.
///
/// Checks as [`package`] does, with `tool:` codes, then extracts by "Tool
/// archives" and moves the folder that `root` names into the cache.
pub fn tool(
    cache: &Path,
    name: &str,
    version: &str,
    host: &str,
    archive: &Archive,
    transport: &dyn Transport,
    pin: Pin,
) -> Result<State, Error> {
    // A tool version may hold `/`, and `.` or `..` is a version too: each
    // name must be one plain folder name before it names a cache folder.
    if !is_host_name(name) || !is_host_name(host) || !is_folder_name(version) {
        return Err(Error::ToolArchive(format!(
            "tool {name} {version} {host}: not a cache folder name"
        )));
    }
    let item = Item {
        kind: Kind::Tool,
        label: format!("tool {name} {version} {host}"),
        dir: tool_dir(cache, name, version, host),
        url: &archive.url,
        size: archive.size,
        sha256: &archive.sha256,
    };
    install(cache, &item, transport, pin, &|file, out| {
        extract_tool(file, out, &archive.root)
    })
}

/// Compares the record of a finished install with the expected hash.
fn compare(item: &Item<'_>, held: &str, pin: Pin) -> Result<State, Error> {
    if held == item.sha256 {
        return Ok(State::Present);
    }
    let d = format!(
        "the cache holds another archive of {} in {}: delete that folder to install this one",
        item.label,
        item.dir.display()
    );
    Err(match pin {
        Pin::Lock => Error::LockHash(d),
        Pin::Index => item.kind.hash(d),
    })
}

/// A temporary folder under `<cache>/tmp/`, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(cache: &Path) -> Result<Self, Error> {
        static N: AtomicU64 = AtomicU64::new(0);
        let base = cache.join(TEMP);
        fs::create_dir_all(&base).map_err(|e| write_err(&base, &e))?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let mut tries = 0;
        loop {
            let n = N.fetch_add(1, Ordering::Relaxed);
            let dir = base.join(format!("{}-{n}-{nanos}", std::process::id()));
            match fs::create_dir(&dir) {
                Ok(()) => return Ok(Self(dir)),
                Err(e) if e.kind() == ErrorKind::AlreadyExists && tries < 16 => tries += 1,
                Err(e) => return Err(write_err(&dir, &e)),
            }
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The steps shared by packages and tools. `extract` reads the checked
/// archive, fills the folder it is given and returns the folder that
/// becomes the item: the folder itself, or the one `root` names.
fn install(
    cache: &Path,
    item: &Item<'_>,
    transport: &dyn Transport,
    pin: Pin,
    extract: &dyn Fn(&Path, &Path) -> Result<PathBuf, Error>,
) -> Result<State, Error> {
    let unfinished = match cached(&item.dir)? {
        Cached::Holds(h) => return compare(item, &h, pin),
        Cached::Missing => false,
        Cached::Unfinished => true,
    };
    let temp = TempDir::new(cache)?;
    if unfinished && let Some(h) = clear_unfinished(&temp.0, &item.dir)? {
        return compare(item, &h, pin);
    }
    let archive = download(&temp.0, item, transport)?;
    let out = temp.0.join("x");
    fs::create_dir(&out).map_err(|e| write_err(&out, &e))?;
    let folder = extract(&archive, &out)?;

    let record = folder.join(RECORD);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&record)
        .map_err(|e| match e.kind() {
            ErrorKind::AlreadyExists => item
                .kind
                .archive(format!("the archive holds an entry named {RECORD}")),
            _ => write_err(&record, &e),
        })?;
    file.write_all(format!("{}\n", item.sha256).as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| write_err(&record, &e))?;
    drop(file);

    if let Some(parent) = item.dir.parent() {
        fs::create_dir_all(parent).map_err(|e| write_err(parent, &e))?;
    }
    match fs::rename(&folder, &item.dir) {
        Ok(()) => Ok(State::Installed),
        // Another install of the same item finished first.
        Err(e) => match cached(&item.dir)? {
            Cached::Holds(h) => compare(item, &h, pin),
            _ => Err(write_err(&item.dir, &e)),
        },
    }
}

/// The points of [`clear_unfinished`] where a test may act, to drive the
/// interleavings of two installs without sleeping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// After the check that found the place unfinished, before the rename
    /// that takes it aside.
    BeforeAside,
    /// After a finished install was taken aside, before it is put back.
    BeforePutBack,
}

#[cfg(test)]
type Hook = Box<dyn FnMut(Step)>;

#[cfg(test)]
thread_local! {
    /// The hook of the running thread. It is taken while it runs, so an
    /// install that it starts on the same thread does not call it again.
    static HOOK: std::cell::RefCell<Option<Hook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn at(step: Step) {
    let hook = HOOK.with(|h| h.borrow_mut().take());
    if let Some(mut f) = hook {
        f(step);
        HOOK.with(|h| {
            let mut h = h.borrow_mut();
            if h.is_none() {
                *h = Some(f);
            }
        });
    }
}

#[cfg(not(test))]
fn at(_step: Step) {}

/// Clears the unfinished folder at `dir`, so that an install can take the
/// place. Returns the hash of the record when the place holds a finished
/// install instead, which the caller then compares.
///
/// Another install may finish at `dir` between any check and any rename.
/// So nothing is removed from `dir` itself. One rename, atomic on the file
/// system, takes whatever is at `dir` into `temp`, and only then is it
/// read. What holds a valid record is a finished install, and it is never
/// removed: it goes back to `dir`, or, when another one took the place
/// meanwhile, the one at `dir` is kept. Only what this install holds and
/// found unfinished is removed. The spec says nothing more about two
/// tools at work on one cache.
fn clear_unfinished(temp: &Path, dir: &Path) -> Result<Option<String>, Error> {
    // A finished install this call took aside and must put back.
    let mut held: Option<(PathBuf, String)> = None;
    for n in 0..8 {
        if let Some((p, h)) = &held {
            at(Step::BeforePutBack);
            // A rename never replaces a folder that holds files, so it
            // cannot replace another finished install.
            if fs::rename(p, dir).is_ok() {
                return Ok(Some(h.clone()));
            }
        }
        match cached(dir)? {
            Cached::Holds(h) => return Ok(Some(h)),
            Cached::Missing if held.is_none() => return Ok(None),
            Cached::Missing => continue,
            Cached::Unfinished => {}
        }
        at(Step::BeforeAside);
        let aside = temp.join(format!("old-{n}"));
        match fs::rename(dir, &aside) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(write_err(dir, &e)),
        }
        match cached(&aside)? {
            Cached::Holds(h) => held = Some((aside, h)),
            _ => {
                let _ = fs::remove_dir_all(&aside);
            }
        }
    }
    Err(Error::IoWrite {
        path: dir.display().to_string(),
        reason: "the folder kept changing while the install cleared it".to_owned(),
    })
}

/// A writer that hashes and counts what it passes to a file, and refuses
/// more bytes than the index gives.
struct Hashing {
    file: File,
    hasher: Sha256,
    count: u64,
    limit: u64,
    too_long: bool,
    failed: Option<io::Error>,
}

impl Write for Hashing {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.count + buf.len() as u64 > self.limit {
            self.too_long = true;
            return Err(io::Error::other("longer than the index says"));
        }
        if let Err(e) = self.file.write_all(buf) {
            let kind = e.kind();
            self.failed = Some(e);
            return Err(io::Error::from(kind));
        }
        self.hasher.update(buf);
        self.count += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Streams the archive into `<temp>/archive.part`, hashing it on the way,
/// and renames it to `<temp>/archive` only on a match of size and hash.
fn download(temp: &Path, item: &Item<'_>, transport: &dyn Transport) -> Result<PathBuf, Error> {
    let part = temp.join("archive.part");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)
        .map_err(|e| write_err(&part, &e))?;
    let mut w = Hashing {
        file,
        hasher: Sha256::new(),
        count: 0,
        limit: item.size,
        too_long: false,
        failed: None,
    };
    let fetched = transport.fetch(item.url, &mut w);
    if let Some(e) = w.failed.take() {
        return Err(write_err(&part, &e));
    }
    let mismatch = |why: String| {
        item.kind
            .hash(format!("the archive of {} {why}", item.label))
    };
    if w.too_long {
        return Err(mismatch(format!("is longer than {} bytes", item.size)));
    }
    if let Err(why) = fetched {
        return Err(mismatch(format!(
            "could not be fetched whole, {} bytes: {why}",
            w.count
        )));
    }
    if w.count != item.size {
        return Err(mismatch(format!(
            "has {} bytes and the index says {}",
            w.count, item.size
        )));
    }
    let got = hex(&w.hasher.finalize_reset());
    if got != item.sha256 {
        return Err(mismatch(format!("has SHA-256 {got}, not {}", item.sha256)));
    }
    w.file.sync_all().map_err(|e| write_err(&part, &e))?;
    drop(w);
    let archive = temp.join("archive");
    fs::rename(&part, &archive).map_err(|e| write_err(&archive, &e))?;
    Ok(archive)
}

/// One file entry of a package archive.
struct PackageFile {
    index: usize,
    name: String,
}

/// One plain folder name: a tool version by its form, without `/`, and not
/// `.` or `..`.
fn is_folder_name(s: &str) -> bool {
    is_tool_version(s) && !s.contains('/') && s != "." && s != ".."
}

/// Checks a package archive by "The archive", reads `sdk.json` and checks
/// it, then extracts every listed file into `out` and checks its size and
/// hash. Nothing is extracted before every entry and `sdk.json` passed.
fn extract_package(archive: &Path, out: &Path, entry: &IndexPackage) -> Result<(), Error> {
    let bad = |d: String| Error::PackageArchive(d);
    let file = File::open(archive).map_err(|e| read_err(archive, &e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| bad(format!("not a zip archive: {e}")))?;

    let mut files: Vec<PackageFile> = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    // Every name in lower case, with whether it is a file.
    let mut folded: BTreeMap<String, bool> = BTreeMap::new();
    for i in 0..zip.len() {
        let f = zip
            .by_index_raw(i)
            .map_err(|e| bad(format!("entry {i} cannot be read: {e}")))?;
        let Ok(raw) = std::str::from_utf8(f.name_raw()) else {
            return Err(bad(format!("entry {i}: the name is not UTF-8")));
        };
        let dir = raw.ends_with('/');
        let name = raw.strip_suffix('/').unwrap_or(raw);
        if !is_archive_path(name) {
            return Err(bad(format!("{raw}: the name breaks the path rules")));
        }
        match f.compression() {
            zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated => {}
            m => {
                return Err(bad(format!(
                    "{raw}: compression {m} is not stored or deflate"
                )));
            }
        }
        // The type, when the entry gives one: a regular file or a folder.
        let kind = f.unix_mode().map_or(0, |m| m & 0o170_000);
        let ok = match kind {
            0 => true,
            0o100_000 => !dir,
            0o040_000 => dir,
            _ => false,
        };
        if !ok || f.is_symlink() {
            return Err(bad(format!("{raw}: not a regular file or a folder")));
        }
        let lower = name.to_lowercase();
        if lower == RECORD {
            return Err(bad(format!(
                "{raw}: the archive holds an entry named {RECORD}"
            )));
        }
        if folded.insert(lower.clone(), !dir).is_some() {
            return Err(bad(format!(
                "{raw}: another entry has this name, or one that differs only in case"
            )));
        }
        if dir {
            dirs.push(name.to_owned());
        } else {
            files.push(PackageFile {
                index: i,
                name: name.to_owned(),
            });
        }
    }
    // A file cannot also be a folder of another entry.
    for name in folded.keys() {
        let mut at = 0;
        while let Some(p) = name[at..].find('/') {
            let parent = &name[..at + p];
            if folded.get(parent) == Some(&true) {
                return Err(bad(format!("{name}: {parent} is a file of the archive")));
            }
            at += p + 1;
        }
    }

    let Some(manifest) = files.iter().find(|f| f.name == MANIFEST) else {
        return Err(bad(format!("no {MANIFEST} at the archive root")));
    };
    let mut bytes = Vec::new();
    zip.by_index(manifest.index)
        .and_then(|mut f| f.read_to_end(&mut bytes).map_err(Into::into))
        .map_err(|e| bad(format!("{MANIFEST} cannot be read: {e}")))?;
    let sdk = sdk_json::parse(&bytes).map_err(Error::Manifest)?;
    sdk.check_against(entry).map_err(Error::Manifest)?;

    let listed: BTreeMap<&str, &sdk_json::FileEntry> =
        sdk.files.iter().map(|f| (f.path.as_str(), f)).collect();
    for f in &files {
        if f.name != MANIFEST && !listed.contains_key(f.name.as_str()) {
            return Err(bad(format!("{}: not listed in {MANIFEST} files", f.name)));
        }
    }
    let present: BTreeSet<&str> = files.iter().map(|f| f.name.as_str()).collect();
    if let Some(f) = sdk
        .files
        .iter()
        .find(|f| !present.contains(f.path.as_str()))
    {
        return Err(Error::PackageHash(format!(
            "{}: listed in {MANIFEST} files and absent from the archive",
            f.path
        )));
    }

    for d in &dirs {
        let p = join(out, d);
        fs::create_dir_all(&p).map_err(|e| write_err(&p, &e))?;
    }
    for f in &files {
        let p = join(out, &f.name);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|e| write_err(parent, &e))?;
        }
        let mut zf = zip
            .by_index(f.index)
            .map_err(|e| bad(format!("{}: {e}", f.name)))?;
        let mut target = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&p)
            .map_err(|e| write_err(&p, &e))?;
        let expected = listed.get(f.name.as_str()).copied();
        let limit = expected.map_or(u64::MAX, |e| e.size.saturating_add(1));
        let mut hasher = Sha256::new();
        let mut count = 0u64;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = zf
                .read(&mut buf)
                .map_err(|e| bad(format!("{}: the data cannot be read: {e}", f.name)))?;
            if n == 0 {
                break;
            }
            target.write_all(&buf[..n]).map_err(|e| write_err(&p, &e))?;
            hasher.update(&buf[..n]);
            count += n as u64;
            if count >= limit {
                break;
            }
        }
        if let Some(e) = expected {
            let got = hex(&hasher.finalize());
            if count != e.size || got != e.sha256 {
                return Err(Error::PackageHash(format!(
                    "{}: does not match its entry in {MANIFEST} files",
                    f.name
                )));
            }
        }
    }
    Ok(())
}

/// Joins a checked `/` path to a folder. An empty path is the folder.
fn join(base: &Path, rel: &str) -> PathBuf {
    let mut p = base.to_path_buf();
    for c in rel.split('/').filter(|c| !c.is_empty()) {
        p.push(c);
    }
    p
}

/// One entry of a tool archive, whatever its kind.
enum Entry {
    Dir,
    File(Option<u32>),
    Link(String),
}

/// Extracts a tool archive into `out` by "Tool archives" and returns the
/// folder that `root` names, once it holds `bin/` and no record.
fn extract_tool(archive: &Path, out: &Path, root: &str) -> Result<PathBuf, Error> {
    let bad = |d: String| Error::ToolArchive(d);
    let mut file = File::open(archive).map_err(|e| read_err(archive, &e))?;
    let mut magic = [0u8; 4];
    let n = file.read(&mut magic).map_err(|e| read_err(archive, &e))?;
    drop(file);
    if n >= 4 && magic == [0x50, 0x4B, 0x03, 0x04] {
        extract_tool_zip(archive, out)?;
    } else if n >= 2 && magic[..2] == [0x1F, 0x8B] {
        extract_tool_tar(archive, out)?;
    } else {
        return Err(bad(
            "neither a zip archive nor a gzip tar archive".to_owned()
        ));
    }
    let folder = root_folder(out, root)?;
    check_links(&folder)?;
    if !fs::symlink_metadata(folder.join("bin")).is_ok_and(|m| m.is_dir()) {
        return Err(bad(format!("root {root:?} holds no bin/")));
    }
    let entries = fs::read_dir(&folder).map_err(|e| read_err(&folder, &e))?;
    for e in entries {
        let e = e.map_err(|e| read_err(&folder, &e))?;
        if e.file_name().to_string_lossy().to_lowercase() == RECORD {
            return Err(bad(format!("root {root:?} holds an entry named {RECORD}")));
        }
    }
    Ok(folder)
}

/// The folder that `root` names inside `out`, resolved by the system.
///
/// The spec does not state the form of `root` yet. Until it does, a `root`
/// is empty or a path by the rules of "The archive", the rule the index
/// reader applies too. It is checked again here, on the value about to be
/// used, and the folder it names must then resolve to a folder inside
/// `out`. Any doubt is a refusal.
fn root_folder(out: &Path, root: &str) -> Result<PathBuf, Error> {
    let bad = |d: String| Error::ToolArchive(d);
    if !is_root(root) {
        return Err(bad(format!("root {root:?} breaks the path rules")));
    }
    let named = join(out, root);
    if !fs::symlink_metadata(&named).is_ok_and(|m| m.is_dir()) {
        return Err(bad(format!("root {root:?} names no folder of the archive")));
    }
    let real_out = fs::canonicalize(out).map_err(|e| read_err(out, &e))?;
    let inside = fs::canonicalize(&named)
        .ok()
        .filter(|r| r.starts_with(&real_out));
    inside.ok_or_else(|| bad(format!("root {root:?} leads outside the extraction folder")))
}

/// The path of a tool archive entry under `out`. Refuses an absolute name,
/// a drive letter and a `..` component. `.` and empty components are
/// dropped. `None` for the extraction folder itself.
fn tool_path(out: &Path, name: &str) -> Result<Option<PathBuf>, Error> {
    let bad = || {
        Error::ToolArchive(format!(
            "{name}: the entry would land outside the extraction folder"
        ))
    };
    let b = name.as_bytes();
    if name.starts_with('/') || name.starts_with('\\') || (b.len() >= 2 && b[1] == b':') {
        return Err(bad());
    }
    let mut p = out.to_path_buf();
    let mut any = false;
    for c in name.split(['/', '\\']) {
        match c {
            "" | "." => {}
            ".." => return Err(bad()),
            c if c.chars().any(|ch| ch < '\u{20}' || ch == '\u{7f}') => return Err(bad()),
            c => {
                p.push(c);
                any = true;
            }
        }
    }
    Ok(any.then_some(p))
}

/// Makes the folders above `path` inside `out`, and refuses a path that
/// passes through a link, so no write ever follows one.
fn make_parents(out: &Path, path: &Path, name: &str) -> Result<(), Error> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let Ok(rel) = parent.strip_prefix(out) else {
        return Err(Error::ToolArchive(format!(
            "{name}: outside the extraction folder"
        )));
    };
    let mut at = out.to_path_buf();
    for c in rel.components() {
        let Component::Normal(c) = c else {
            return Err(Error::ToolArchive(format!("{name}: a bad path")));
        };
        at.push(c);
        match fs::symlink_metadata(&at) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                return Err(Error::ToolArchive(format!(
                    "{name}: {} is a file or a link of the archive",
                    at.display()
                )));
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                fs::create_dir(&at).map_err(|e| write_err(&at, &e))?;
            }
            Err(e) => return Err(read_err(&at, &e)),
        }
    }
    Ok(())
}

/// Writes one tool entry. A file or link whose name is already taken is
/// refused: two entries of one name, or a name that only a case-blind file
/// system folds onto another.
fn put(out: &Path, name: &str, entry: Entry, data: &mut dyn Read) -> Result<(), Error> {
    let Some(path) = tool_path(out, name)? else {
        return Ok(());
    };
    make_parents(out, &path, name)?;
    let taken = |e: &io::Error| {
        if e.kind() == ErrorKind::AlreadyExists {
            Error::ToolArchive(format!("{name}: another entry has this name"))
        } else {
            write_err(&path, e)
        }
    };
    match entry {
        Entry::Dir => match fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => Ok(()),
            Ok(_) => Err(Error::ToolArchive(format!(
                "{name}: another entry has this name"
            ))),
            Err(_) => fs::create_dir(&path).map_err(|e| taken(&e)),
        },
        Entry::File(mode) => {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| taken(&e))?;
            copy_entry(name, &path, data, &mut f)?;
            set_mode(&path, mode)
        }
        Entry::Link(target) => {
            if target.is_empty()
                || target.starts_with('/')
                || target.starts_with('\\')
                || target.as_bytes().get(1) == Some(&b':')
            {
                return Err(Error::ToolArchive(format!(
                    "{name}: the link leads outside the extraction folder"
                )));
            }
            symlink(&target, &path).map_err(|e| taken(&e))
        }
    }
}

/// Copies the data of one entry. A failure to read is the archive's,
/// `tool: archive`; a failure to write is the destination's, `io: write`.
fn copy_entry(
    name: &str,
    path: &Path,
    data: &mut dyn Read,
    to: &mut dyn Write,
) -> Result<(), Error> {
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = match data.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(Error::ToolArchive(format!(
                    "{name}: the data cannot be read: {e}"
                )));
            }
        };
        to.write_all(&buf[..n]).map_err(|e| write_err(path, &e))?;
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    match mode {
        Some(m) => fs::set_permissions(path, fs::Permissions::from_mode(m & 0o755))
            .map_err(|e| write_err(path, &e)),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: Option<u32>) -> Result<(), Error> {
    Ok(())
}

#[cfg(unix)]
fn symlink(target: &str, path: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, path)
}

#[cfg(windows)]
fn symlink(target: &str, path: &Path) -> io::Result<()> {
    let target = target.replace('/', "\\");
    let is_dir = path.parent().is_some_and(|p| p.join(&target).is_dir());
    if is_dir {
        std::os::windows::fs::symlink_dir(&target, path)
    } else {
        std::os::windows::fs::symlink_file(&target, path)
    }
}

#[cfg(not(any(unix, windows)))]
fn symlink(_target: &str, _path: &Path) -> io::Result<()> {
    Err(io::Error::from(ErrorKind::Unsupported))
}

/// Checks, once everything is extracted, that each link inside `folder`,
/// the real path of the folder that is installed, leads to something that
/// exists inside it. The system resolves the link, so a chain of links
/// counts. A link that leads nowhere, or out of `folder`, is refused: only
/// `folder` is kept.
fn check_links(folder: &Path) -> Result<(), Error> {
    let mut stack = vec![folder.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = fs::read_dir(&d).map_err(|e| read_err(&d, &e))?;
        for e in entries {
            let e = e.map_err(|e| read_err(&d, &e))?;
            let p = e.path();
            let m = fs::symlink_metadata(&p).map_err(|e| read_err(&p, &e))?;
            if m.file_type().is_symlink() {
                let inside = fs::canonicalize(&p).is_ok_and(|r| r.starts_with(folder));
                if !inside {
                    let rel = p.strip_prefix(folder).unwrap_or(&p);
                    return Err(Error::ToolArchive(format!(
                        "{}: the link leads outside the installed folder, or to nothing",
                        rel.display()
                    )));
                }
            } else if m.is_dir() {
                stack.push(p);
            }
        }
    }
    Ok(())
}

fn extract_tool_zip(archive: &Path, out: &Path) -> Result<(), Error> {
    let bad = |d: String| Error::ToolArchive(d);
    let file = File::open(archive).map_err(|e| read_err(archive, &e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| bad(format!("not a zip archive: {e}")))?;
    for i in 0..zip.len() {
        let mut f = zip
            .by_index(i)
            .map_err(|e| bad(format!("entry {i} cannot be read: {e}")))?;
        let name = f
            .name()
            .map_err(|e| bad(format!("entry {i}: {e}")))?
            .into_owned();
        let mode = f.unix_mode();
        let kind = mode.map_or(0, |m| m & 0o170_000);
        let entry = if f.is_symlink() {
            let mut target = String::new();
            f.read_to_string(&mut target)
                .map_err(|e| bad(format!("{name}: the link cannot be read: {e}")))?;
            Entry::Link(target)
        } else if f.is_dir() {
            Entry::Dir
        } else if kind == 0 || kind == 0o100_000 {
            Entry::File(mode)
        } else {
            return Err(bad(format!("{name}: not a file, a folder or a link")));
        };
        put(out, &name, entry, &mut f)?;
    }
    Ok(())
}

fn extract_tool_tar(archive: &Path, out: &Path) -> Result<(), Error> {
    let bad = |d: String| Error::ToolArchive(d);
    let file = File::open(archive).map_err(|e| read_err(archive, &e))?;
    let mut gz = flate2::read::GzDecoder::new(io::BufReader::new(file));
    {
        let mut tar = tar::Archive::new(&mut gz);
        let entries = tar
            .entries()
            .map_err(|e| bad(format!("the tar data cannot be read: {e}")))?;
        for e in entries {
            let mut e = e.map_err(|e| bad(format!("the tar data cannot be read: {e}")))?;
            let path = e.path_bytes().into_owned();
            let Ok(name) = String::from_utf8(path) else {
                return Err(bad("an entry name is not UTF-8".to_owned()));
            };
            let header = e.header();
            let kind = header.entry_type();
            let entry = if kind.is_dir() {
                Entry::Dir
            } else if kind.is_file() || kind.is_contiguous() {
                Entry::File(header.mode().ok())
            } else if kind.is_symlink() {
                let target = e
                    .link_name_bytes()
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default();
                Entry::Link(target)
            } else if kind.is_pax_global_extensions() {
                continue;
            } else {
                return Err(bad(format!("{name}: not a file, a folder or a link")));
            };
            put(out, &name, entry, &mut e)?;
        }
    }
    io::copy(&mut gz, &mut io::sink())
        .map_err(|e| bad(format!("the gzip data cannot be read to the end: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests;
