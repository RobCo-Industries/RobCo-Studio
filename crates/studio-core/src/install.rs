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
use crate::resolve::index::{
    Archive, IndexPackage, is_archive_path, is_portable_name as is_portable, is_root,
};
use crate::resolve::{self, sdk_json};
use crate::resolve::{is_host_name, is_package_name, is_tool_version};

/// The file of "What the cache records", right under an installed folder.
pub const RECORD: &str = ".robco-archive";

/// The folder of `<cache>` that holds the temporary folders of installs.
pub const TEMP: &str = "tmp";

/// The name of the manifest at the root of a package archive.
const MANIFEST: &str = "sdk.json";

// The bounds below are this file's own: the spec does not state them yet.
// Each read of compressed data stops one byte past its bound.

/// The most bytes of `sdk.json`. The spec does not state this limit yet.
const MAX_MANIFEST: u64 = 4 << 20;
/// The most bytes a tool archive may write, all entries together, and the
/// most bytes of the tar stream of a tar.gz once ungzipped. The spec does
/// not state this limit yet. The pinned compiler unpacks to about 1 GiB.
const MAX_TOOL_BYTES: u64 = 8 << 30;
/// The most entries of a tool archive. The spec does not state this limit
/// yet.
const MAX_TOOL_ENTRIES: u64 = 400_000;
/// The most bytes of a zip link target, and of the tar records between
/// the data of two entries: headers, long names, long link targets and pax
/// records. The spec does not state this limit yet.
const MAX_META: u64 = 1 << 20;

/// The bounds of one install, as parameters so that tests can use small
/// ones.
#[derive(Debug, Clone, Copy)]
struct Limits {
    manifest: u64,
    tool_bytes: u64,
    tool_entries: u64,
    meta: u64,
}

const LIMITS: Limits = Limits {
    manifest: MAX_MANIFEST,
    tool_bytes: MAX_TOOL_BYTES,
    tool_entries: MAX_TOOL_ENTRIES,
    meta: MAX_META,
};

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
        let path = join_inside(&self.base, url, || {
            format!("nothing was fetched from {url}: not a path inside the index folder")
        })?;
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

/// One step of an install, by "step" of RobCo-Core
/// `docs/spec/STUDIO_CLI.md`. They come in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Fetching the archive. A transport that fails, or that gives more
    /// bytes than the index says, fails here.
    Download,
    /// The size and SHA-256 of the whole archive, against the index. A
    /// download that ended short fails here.
    Verify,
    /// Everything after: the archive rules, `sdk.json`, the extraction and
    /// the hash of each extracted file, the record and the move into the
    /// cache.
    Install,
}

/// What an install tells its caller as it works: one call per transition.
///
/// A step that fails gets a `start` and no `done`, as "step" allows: the
/// error the call returns belongs to the step that is still open. An item
/// that the cache holds with the expected record gets one `skip` and no
/// other call. Every failure comes inside a step: one that comes before
/// the step would have begun opens it first. A record that differs, a cache
/// folder that cannot be read and a name that is no cache folder name fail
/// in `install`. A temporary folder that cannot be made, and an unfinished
/// folder that cannot be cleared, fail in `download`.
pub trait Progress {
    /// A step begins.
    fn start(&mut self, phase: Phase);
    /// The step that began last ended well.
    fn done(&mut self, phase: Phase);
    /// The cache already held the item: nothing was needed.
    fn skip(&mut self);
}

/// A [`Progress`] that ignores every transition.
pub struct Quiet;

impl Progress for Quiet {
    fn start(&mut self, _phase: Phase) {}
    fn done(&mut self, _phase: Phase) {}
    fn skip(&mut self) {}
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
/// `None` when a name is not one plain component of a path.
pub fn package_dir(cache: &Path, name: &str, version: &str) -> Option<PathBuf> {
    join_parts(cache, &["packages", name, version], || ()).ok()
}

/// The folder of an installed tool: `<cache>/tools/<name>/<version>/<host>/`.
/// `None` when a name is not one plain component of a path.
pub fn tool_dir(cache: &Path, name: &str, version: &str, host: &str) -> Option<PathBuf> {
    join_parts(cache, &["tools", name, version, host], || ()).ok()
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
    package_with(cache, entry, transport, pin, LIMITS, &mut Quiet)
}

/// [`package`] that tells `progress` each step as it begins and ends.
pub fn package_watched(
    cache: &Path,
    entry: &IndexPackage,
    transport: &dyn Transport,
    pin: Pin,
    progress: &mut dyn Progress,
) -> Result<State, Error> {
    package_with(cache, entry, transport, pin, LIMITS, progress)
}

/// [`package`] with the bounds it is given.
fn package_with(
    cache: &Path,
    entry: &IndexPackage,
    transport: &dyn Transport,
    pin: Pin,
    limits: Limits,
    progress: &mut dyn Progress,
) -> Result<State, Error> {
    let version = entry.version.to_string();
    // The cache folder names come from the entry: each must be one plain
    // folder name. A version prints as digits, letters, `-`, `+` and `.`.
    if !is_package_name(&entry.name) || !is_portable(&entry.name) || !is_folder_name(&version) {
        return Err(open_in(
            progress,
            Phase::Install,
            Error::PackageArchive(format!(
                "package {} {version}: not a cache folder name",
                entry.name
            )),
        ));
    }
    let item = Item {
        kind: Kind::Package,
        label: format!("package {} {version}", entry.name),
        dir: match package_dir(cache, &entry.name, &version) {
            Some(dir) => dir,
            None => {
                return Err(open_in(
                    progress,
                    Phase::Install,
                    Error::PackageArchive(format!(
                        "package {} {version}: not a cache folder name",
                        entry.name
                    )),
                ));
            }
        },
        url: &entry.url,
        size: entry.size,
        sha256: &entry.sha256,
    };
    install(cache, &item, transport, pin, progress, &|archive, out| {
        extract_package(archive, out, entry, limits)?;
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
    tool_with(
        cache,
        [name, version, host],
        archive,
        transport,
        pin,
        LIMITS,
        &mut Quiet,
    )
}

/// [`tool`] that tells `progress` each step as it begins and ends. `id` is
/// the name, the version and the host.
pub fn tool_watched(
    cache: &Path,
    id: [&str; 3],
    archive: &Archive,
    transport: &dyn Transport,
    pin: Pin,
    progress: &mut dyn Progress,
) -> Result<State, Error> {
    tool_with(cache, id, archive, transport, pin, LIMITS, progress)
}

/// [`tool`] with the bounds it is given. `id` is the name, the version and
/// the host.
fn tool_with(
    cache: &Path,
    id: [&str; 3],
    archive: &Archive,
    transport: &dyn Transport,
    pin: Pin,
    limits: Limits,
    progress: &mut dyn Progress,
) -> Result<State, Error> {
    let [name, version, host] = id;
    // A tool version may hold `/`, and `.` or `..` is a version too: each
    // name must be one plain folder name before it names a cache folder.
    if !is_host_name(name)
        || !is_host_name(host)
        || !is_portable(name)
        || !is_portable(host)
        || !is_folder_name(version)
    {
        return Err(open_in(
            progress,
            Phase::Install,
            Error::ToolArchive(format!(
                "tool {name} {version} {host}: not a cache folder name"
            )),
        ));
    }
    let item = Item {
        kind: Kind::Tool,
        label: format!("tool {name} {version} {host}"),
        dir: match tool_dir(cache, name, version, host) {
            Some(dir) => dir,
            None => {
                return Err(open_in(
                    progress,
                    Phase::Install,
                    Error::ToolArchive(format!(
                        "tool {name} {version} {host}: not a cache folder name"
                    )),
                ));
            }
        },
        url: &archive.url,
        size: archive.size,
        sha256: &archive.sha256,
    };
    install(cache, &item, transport, pin, progress, &|file, out| {
        extract_tool(file, out, &archive.root, limits)
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
    progress: &mut dyn Progress,
    extract: &dyn Fn(&Path, &Path) -> Result<PathBuf, Error>,
) -> Result<State, Error> {
    // Every failure happens inside a step. The look into the cache and the
    // check of a record belong to `install`, where the item would have been
    // installed. The temporary folder and the clearing of an unfinished
    // folder come before the fetch of an item the cache lacks, so they
    // belong to `download`. A step opens only when it fails here, so an
    // item found whole in the cache gets its `skip` alone.
    let held = |h: &str, progress: &mut dyn Progress| match compare(item, h, pin) {
        Ok(state) => {
            progress.skip();
            Ok(state)
        }
        Err(e) => Err(open_in(progress, Phase::Install, e)),
    };
    let unfinished = match cached(&item.dir) {
        Ok(Cached::Holds(h)) => return held(&h, progress),
        Ok(Cached::Missing) => false,
        Ok(Cached::Unfinished) => true,
        Err(e) => return Err(open_in(progress, Phase::Install, e)),
    };
    let temp = TempDir::new(cache).map_err(|e| open_in(progress, Phase::Download, e))?;
    if unfinished {
        match clear_unfinished(&temp.0, &item.dir) {
            Ok(Some(h)) => return held(&h, progress),
            Ok(None) => {}
            Err(e) => return Err(open_in(progress, Phase::Download, e)),
        }
    }
    let archive = download(&temp.0, item, transport, progress)?;
    progress.start(Phase::Install);
    let state = finish(item, pin, &temp.0, &archive, extract)?;
    progress.done(Phase::Install);
    Ok(state)
}

/// Opens `phase` for a failure that comes before the step would have
/// begun, and returns the error, which then belongs to that open step.
fn open_in(progress: &mut dyn Progress, phase: Phase, e: Error) -> Error {
    progress.start(phase);
    e
}

/// The `install` step: extracts the checked archive, writes the record and
/// moves the folder into the cache.
fn finish(
    item: &Item<'_>,
    pin: Pin,
    temp: &Path,
    archive: &Path,
    extract: &dyn Fn(&Path, &Path) -> Result<PathBuf, Error>,
) -> Result<State, Error> {
    let out = temp.join("x");
    fs::create_dir(&out).map_err(|e| write_err(&out, &e))?;
    let folder = extract(archive, &out)?;

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
            // When another install took the place, the copy held aside goes
            // with `temp`: `dir` holds a finished install with a valid
            // record, all the cache promises, and the caller judges that one.
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
/// more bytes than the index gives. A failure of the file itself, in
/// `write` or `flush`, is kept in `failed`, so that the caller reports
/// `io: write` and not a mismatch of the download.
struct Hashing<W> {
    file: W,
    hasher: Sha256,
    count: u64,
    limit: u64,
    too_long: bool,
    failed: Option<io::Error>,
}

impl<W: Write> Hashing<W> {
    fn keep(&mut self, e: io::Error) -> io::Error {
        let kind = e.kind();
        self.failed = Some(e);
        io::Error::from(kind)
    }
}

impl<W: Write> Write for Hashing<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // The index gives the size: a transport that gives one byte more
        // is refused before that byte is written.
        if self.count + buf.len() as u64 > self.limit {
            self.too_long = true;
            return Err(io::Error::other("longer than the index says"));
        }
        if let Err(e) = self.file.write_all(buf) {
            return Err(self.keep(e));
        }
        self.hasher.update(buf);
        self.count += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        match self.file.flush() {
            Ok(()) => Ok(()),
            Err(e) => Err(self.keep(e)),
        }
    }
}

/// Streams the archive into `<temp>/archive.part`, hashing it on the way,
/// and renames it to `<temp>/archive` only on a match of size and hash.
/// It opens the `download` step, and the `verify` step once the fetch
/// ended, and closes `verify` once the archive is kept.
fn download(
    temp: &Path,
    item: &Item<'_>,
    transport: &dyn Transport,
    progress: &mut dyn Progress,
) -> Result<PathBuf, Error> {
    progress.start(Phase::Download);
    let part = temp.join("archive.part");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)
        .map_err(|e| write_err(&part, &e))?;
    let file = fetch_checked(&part, item, transport, file, progress)?;
    // The sync is the last write that can fail: a close error after it
    // loses nothing that the sync did not already report.
    file.sync_all().map_err(|e| write_err(&part, &e))?;
    drop(file);
    let archive = temp.join("archive");
    fs::rename(&part, &archive).map_err(|e| write_err(&archive, &e))?;
    progress.done(Phase::Verify);
    Ok(archive)
}

/// Fetches the archive into `file`, which writes `part`, and checks its
/// size and hash. Returns `file` on a match.
fn fetch_checked<W: Write>(
    part: &Path,
    item: &Item<'_>,
    transport: &dyn Transport,
    file: W,
    progress: &mut dyn Progress,
) -> Result<W, Error> {
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
        return Err(write_err(part, &e));
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
    // The fetch ended. What follows is the hash check of "Hashes": a
    // download that ended short fails it.
    progress.done(Phase::Download);
    progress.start(Phase::Verify);
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
    Ok(w.file)
}

/// One file entry of a package archive.
struct PackageFile {
    index: usize,
    name: String,
}

/// One plain folder name: a tool version by its form that [`is_portable`]
/// accepts too.
fn is_folder_name(s: &str) -> bool {
    is_tool_version(s) && is_portable(s)
}

/// Checks a package archive by "The archive", reads `sdk.json` and checks
/// it, then extracts every listed file into `out` and checks its size and
/// hash. Nothing is extracted before every entry and `sdk.json` passed.
fn extract_package(
    archive: &Path,
    out: &Path,
    entry: &IndexPackage,
    limits: Limits,
) -> Result<(), Error> {
    let bad = |d: String| Error::PackageArchive(d);
    let file = File::open(archive).map_err(|e| read_err(archive, &e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| bad(format!("not a zip archive: {e}")))?;

    let mut files: Vec<PackageFile> = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    // Every name, with whether it is a file.
    let mut seen: Vec<(String, bool)> = Vec::new();
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
        if resolve::same_but_case(name, RECORD) {
            return Err(bad(format!(
                "{raw}: the archive holds an entry named {RECORD}"
            )));
        }
        // An archive has few entries, so each pair is compared.
        if seen.iter().any(|(n, _)| resolve::same_but_case(n, name)) {
            return Err(bad(format!(
                "{raw}: another entry has this name, or one that differs only in case"
            )));
        }
        seen.push((name.to_owned(), !dir));
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
    for (name, _) in &seen {
        let mut at = 0;
        while let Some(p) = name[at..].find('/') {
            let parent = &name[..at + p];
            if seen
                .iter()
                .any(|(n, file)| *file && resolve::same_but_case(n, parent))
            {
                return Err(bad(format!("{name}: {parent} is a file of the archive")));
            }
            at += p + 1;
        }
    }

    let Some(manifest) = files.iter().find(|f| f.name == MANIFEST) else {
        return Err(bad(format!("no {MANIFEST} at the archive root")));
    };
    let too_big = |d: String| Error::Manifest(resolve::Error::PackageManifest(d));
    let mut mf = zip
        .by_index(manifest.index)
        .map_err(|e| bad(format!("{MANIFEST} cannot be read: {e}")))?;
    // The size the entry states comes first, and the read is bounded too,
    // because the stated size can lie.
    if mf.size() > limits.manifest {
        return Err(too_big(format!(
            "{MANIFEST} states {} bytes, over the limit of {} bytes",
            mf.size(),
            limits.manifest
        )));
    }
    let mut bytes = Vec::new();
    let n = copy_bounded(&mut mf, &mut bytes, limits.manifest)
        .map_err(|e| bad(format!("{MANIFEST} cannot be read: {}", e.inner())))?;
    if n > limits.manifest {
        return Err(too_big(format!(
            "{MANIFEST} holds more than the limit of {} bytes",
            limits.manifest
        )));
    }
    drop(mf);
    let sdk = sdk_json::parse(&bytes).map_err(Error::Manifest)?;
    sdk.check_against(entry).map_err(Error::Manifest)?;

    let listed: BTreeMap<&str, &sdk_json::FileEntry> =
        sdk.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let present: BTreeSet<&str> = files.iter().map(|f| f.name.as_str()).collect();
    // "Layout of pipos" is the narrower rule: a file of the layout that
    // `files` leaves out fails with `package: manifest`, even when the
    // archive holds it, before "The archive" refuses an unlisted entry.
    if entry.name == resolve::PIPOS {
        check_pipos_layout(&present, &listed)?;
    }
    for f in &files {
        if f.name != MANIFEST && !listed.contains_key(f.name.as_str()) {
            return Err(bad(format!("{}: not listed in {MANIFEST} files", f.name)));
        }
    }
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

    let outside = |n: &str| {
        bad(format!(
            "{n}: the entry would land outside the extraction folder"
        ))
    };
    for d in &dirs {
        let p = join_inside(out, d, || outside(d))?;
        fs::create_dir_all(&p).map_err(|e| write_err(&p, &e))?;
    }
    for f in &files {
        let p = join_inside(out, &f.name, || outside(&f.name))?;
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
        let failed = |e: CopyFail| match e {
            CopyFail::Read(e) => bad(format!("{}: the data cannot be read: {e}", f.name)),
            CopyFail::Write(e) => write_err(&p, &e),
        };
        let Some(expected) = listed.get(f.name.as_str()).copied() else {
            // `sdk.json` itself, already read within its limit.
            let n = copy_bounded(&mut zf, &mut target, limits.manifest).map_err(failed)?;
            if n != bytes.len() as u64 {
                return Err(bad(format!("{}: reads differently twice", f.name)));
            }
            continue;
        };
        let (count, got) = copy_listed(&mut zf, &mut target, expected.size).map_err(failed)?;
        if count != expected.size || got != expected.sha256 {
            return Err(Error::PackageHash(format!(
                "{}: does not match its entry in {MANIFEST} files",
                f.name
            )));
        }
    }
    Ok(())
}

/// The files of "Layout of pipos" that a `pipos` package must hold, but
/// `sdk.json`, which "The archive" already requires.
const PIPOS_FILES: &[&str] = &[
    "LICENSES/MIT.txt",
    "boot.json",
    "boot/PIPOS.CFG",
    "boot/PIPOS.IMG",
    "boot/RUNNER.ROM",
    "boot/config.txt",
    "boot/fwstub.bin",
    "boot/pipos.elf",
    "docs/API.md",
    "flags.json",
    "include/pipos/pip.h",
    "include/pipos/abi.h",
    "include/pipos/crc32.h",
    "include/pipos/crt0.h",
    "include/pipos/heap.h",
    "include/pipos/ini.h",
    "include/pipos/printf.h",
    "include/pipos/selftest.h",
    "include/pipos/string.h",
    "include/pipos/syscall.h",
    "lib/crt0.S",
    "lib/holotape.ld",
    "lib/libpip.a",
    "lib/rom.ld",
];

/// "Layout of pipos": a file of the layout that the archive lacks, or that
/// `files` does not list, fails with `package: manifest` and names the
/// path. A file the layout does not name is not refused here.
fn check_pipos_layout(
    present: &BTreeSet<&str>,
    listed: &BTreeMap<&str, &sdk_json::FileEntry>,
) -> Result<(), Error> {
    // This runs before the check of unlisted entries, so a file of the
    // layout that the archive holds and `files` leaves out fails here.
    match PIPOS_FILES
        .iter()
        .find(|p| !present.contains(*p) || !listed.contains_key(*p))
    {
        Some(p) => Err(Error::Manifest(resolve::Error::PackageManifest(format!(
            "{p}: a pipos package must hold it, listed in sdk.json files"
        )))),
        None => Ok(()),
    }
}

/// Why a bounded copy stopped.
#[derive(Debug)]
enum CopyFail {
    /// The source failed: the archive's fault.
    Read(io::Error),
    /// The destination failed: the cache's fault.
    Write(io::Error),
}

impl CopyFail {
    fn inner(&self) -> &io::Error {
        match self {
            Self::Read(e) | Self::Write(e) => e,
        }
    }
}

/// Copies `from` into `to` until the end of `from` or `limit + 1` bytes,
/// whichever comes first, and returns the count. A count over `limit`
/// means `from` holds more than `limit`: nothing past that one byte is read
/// or written.
fn copy_bounded(from: &mut dyn Read, to: &mut dyn Write, limit: u64) -> Result<u64, CopyFail> {
    let mut buf = [0u8; 64 * 1024];
    let mut count = 0u64;
    loop {
        let room = limit.saturating_add(1) - count;
        if room == 0 {
            return Ok(count);
        }
        let want = usize::try_from(room).map_or(buf.len(), |r| r.min(buf.len()));
        let n = match from.read(&mut buf[..want]) {
            Ok(0) => return Ok(count),
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(CopyFail::Read(e)),
        };
        to.write_all(&buf[..n]).map_err(CopyFail::Write)?;
        count += n as u64;
    }
}

/// A writer that hashes what it passes on.
struct HashTo<'a> {
    to: &'a mut dyn Write,
    hasher: Sha256,
}

impl Write for HashTo<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.to.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.to.flush()
    }
}

/// Copies one package file whose `files` entry gives `size`: at most
/// `size + 1` bytes are read and written. Returns the count and the
/// SHA-256 of what was written.
fn copy_listed(
    from: &mut dyn Read,
    to: &mut dyn Write,
    size: u64,
) -> Result<(u64, String), CopyFail> {
    let mut h = HashTo {
        to,
        hasher: Sha256::new(),
    };
    let n = copy_bounded(from, &mut h, size)?;
    Ok((n, hex(&h.hasher.finalize())))
}

/// Joins a checked relative `/` path to `base`, by [`join_parts`]. An
/// empty path is `base` itself.
fn join_inside<E>(base: &Path, rel: &str, bad: impl FnOnce() -> E) -> Result<PathBuf, E> {
    if rel.is_empty() {
        return join_parts(base, &[], bad);
    }
    join_parts(base, &rel.split('/').collect::<Vec<_>>(), bad)
}

/// Joins names that came from outside to `base`, and proves the result
/// before anyone uses it: it must start with `base`, component by
/// component, and hold exactly one more plain component per name. A name
/// that is empty, absolute, `.`, `..`, or that the system reads as a
/// prefix, a root or more than one component is refused, whatever an
/// earlier check let through. On Windows, `C:payload` is such a name: a
/// push of it would drop `base`. A refusal gives `bad()`, and nothing is
/// created.
fn join_parts<E>(base: &Path, names: &[&str], bad: impl FnOnce() -> E) -> Result<PathBuf, E> {
    let mut p = base.to_path_buf();
    for name in names {
        let mut cs = Path::new(name).components();
        match (cs.next(), cs.next()) {
            (Some(Component::Normal(c)), None) if c == *name => p.push(name),
            _ => return Err(bad()),
        }
    }
    let mut got = p.components();
    let starts = base.components().all(|b| got.next() == Some(b));
    let added: Vec<_> = got.collect();
    if !starts
        || added.len() != names.len()
        || added.iter().any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(bad());
    }
    Ok(p)
}

/// One entry of a tool archive, whatever its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Dir,
    File(Option<u32>),
    Link(String),
}

/// Extracts a tool archive into `out` by "Tool archives" and returns the
/// folder that `root` names, once it holds `bin/` and no record.
fn extract_tool(archive: &Path, out: &Path, root: &str, limits: Limits) -> Result<PathBuf, Error> {
    let bad = |d: String| Error::ToolArchive(d);
    let mut file = File::open(archive).map_err(|e| read_err(archive, &e))?;
    let mut magic = [0u8; 4];
    let n = file.read(&mut magic).map_err(|e| read_err(archive, &e))?;
    drop(file);
    if n >= 4 && magic == [0x50, 0x4B, 0x03, 0x04] {
        extract_tool_zip(archive, out, limits)?;
    } else if n >= 2 && magic[..2] == [0x1F, 0x8B] {
        extract_tool_tar(archive, out, limits)?;
    } else {
        return Err(bad(
            "neither a zip archive nor a gzip tar archive".to_owned()
        ));
    }
    // "Tool archives" refuses with `tool: archive` "an entry would land
    // outside the extraction folder: ... a link that leads outside". That
    // holds for every entry, so every link of the extraction is checked,
    // in the folders that `root` leaves out too. Then each link of the
    // folder that is installed must lead inside that folder, since only it
    // is kept.
    let real_out = fs::canonicalize(out).map_err(|e| read_err(out, &e))?;
    check_links(&real_out, "the extraction folder")?;
    let folder = root_folder(out, root)?;
    check_links(&folder, "the installed folder")?;
    if !fs::symlink_metadata(folder.join("bin")).is_ok_and(|m| m.is_dir()) {
        return Err(bad(format!("root {root:?} holds no bin/")));
    }
    let entries = fs::read_dir(&folder).map_err(|e| read_err(&folder, &e))?;
    for e in entries {
        let e = e.map_err(|e| read_err(&folder, &e))?;
        if resolve::same_but_case(&e.file_name().to_string_lossy(), RECORD) {
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
    let named = join_inside(out, root, || {
        bad(format!("root {root:?} leads outside the extraction folder"))
    })?;
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
/// a drive letter, a `..` component and a component that is not a plain
/// name by [`is_portable`]. `.` and empty components are dropped. `None`
/// for the extraction folder itself.
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
    let p = parts(name);
    if p.is_empty() {
        return Ok(None);
    }
    if !p.iter().all(|c| is_portable(c)) {
        return Err(bad());
    }
    join_parts(out, &p, bad).map(Some)
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

/// Writes one tool entry and adds it to `listed`. A link is only checked
/// and listed: [`make_links`] makes the links once every file and folder
/// exists. A file or link whose name is already taken is refused: two
/// entries of one name, or a name that only a case-blind file system folds
/// onto another.
fn put(
    out: &Path,
    name: &str,
    entry: Entry,
    data: &mut dyn Read,
    listed: &mut Vec<(String, Entry)>,
    budget: &mut Budget,
) -> Result<(), Error> {
    let Some(path) = tool_path(out, name)? else {
        return Ok(());
    };
    if let Entry::Link(target) = &entry {
        if target.is_empty()
            || target.starts_with('/')
            || target.starts_with('\\')
            || target.as_bytes().get(1) == Some(&b':')
        {
            return Err(Error::ToolArchive(format!(
                "{name}: the link leads outside the extraction folder"
            )));
        }
        if !parts(target).iter().all(|c| *c == ".." || is_portable(c)) {
            return Err(Error::ToolArchive(format!(
                "{name}: the link target {target:?} holds a name that is not plain"
            )));
        }
        listed.push((name.to_owned(), entry));
        return Ok(());
    }
    listed.push((name.to_owned(), entry.clone()));
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
            copy_entry(name, &path, data, &mut f, budget)?;
            set_mode(&path, mode)
        }
        Entry::Link(_) => Ok(()),
    }
}

/// A link to make, with the kind its target has.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Link {
    name: String,
    target: String,
    dir: bool,
}

/// The components of an entry name or a link target, `.` and empty ones
/// dropped. `\\` counts as a separator, as it does on Windows.
fn parts(s: &str) -> Vec<&str> {
    s.split(['/', '\\'])
        .filter(|c| !c.is_empty() && *c != ".")
        .collect()
}

/// A path inside the extraction folder, as its components.
type Key<'a> = Vec<&'a str>;

/// Decides, from the entries alone, the links to make: in an order where
/// the target of each link exists when it is made, each with the kind its
/// target really has, whatever the order of the entries. A link that leads
/// out of the extraction folder or to nothing, or a cycle of links, is
/// refused with `tool: archive`.
fn plan_links(listed: &[(String, Entry)]) -> Result<Vec<Link>, Error> {
    let mut nodes: BTreeMap<Key<'_>, &Entry> = BTreeMap::new();
    for (name, entry) in listed {
        let p = parts(name);
        for i in 1..p.len() {
            nodes.entry(p[..i].to_vec()).or_insert(&Entry::Dir);
        }
        if !p.is_empty() {
            nodes.insert(p, entry);
        }
    }
    let mut planner = Planner {
        nodes: &nodes,
        done: BTreeMap::new(),
        visiting: BTreeSet::new(),
        order: Vec::new(),
    };
    for (name, entry) in listed {
        if let Entry::Link(_) = entry {
            planner.resolve(&parts(name), name)?;
        }
    }
    Ok(planner.order)
}

/// The state of [`plan_links`]: the links resolved so far, with the path
/// and kind their target ends at, and the links being resolved.
struct Planner<'a, 'n> {
    nodes: &'n BTreeMap<Key<'a>, &'a Entry>,
    done: BTreeMap<Key<'a>, (Key<'a>, bool)>,
    visiting: BTreeSet<Key<'a>>,
    order: Vec<Link>,
}

impl<'a> Planner<'a, '_> {
    /// Resolves the link at `at`, and first each link its target passes
    /// through, then lists it after them. Returns where it ends and
    /// whether that is a folder.
    fn resolve(&mut self, at: &[&'a str], name: &str) -> Result<(Key<'a>, bool), Error> {
        if let Some(r) = self.done.get(at) {
            return Ok(r.clone());
        }
        let bad = |why: &str| Error::ToolArchive(format!("{name}: the link {why}"));
        let Some(Entry::Link(target)) = self.nodes.get(at).copied() else {
            return Err(bad("is not a link"));
        };
        if !self.visiting.insert(at.to_vec()) {
            return Err(bad("is part of a cycle of links"));
        }
        let mut cur: Key<'a> = at[..at.len().saturating_sub(1)].to_vec();
        for c in parts(target) {
            if c == ".." {
                if cur.pop().is_none() {
                    return Err(bad("leads outside the extraction folder"));
                }
                continue;
            }
            cur.push(c);
            if let Some(Entry::Link(_)) = self.nodes.get(&cur).copied() {
                let key = cur.clone();
                cur = self.resolve(&key, name)?.0;
            }
        }
        let dir = match self.nodes.get(&cur).copied() {
            _ if cur.is_empty() => true,
            Some(Entry::Dir) => true,
            Some(Entry::File(_)) => false,
            _ => return Err(bad("leads to nothing")),
        };
        self.visiting.remove(at);
        self.done.insert(at.to_vec(), (cur.clone(), dir));
        self.order.push(Link {
            name: at.join("/"),
            target: target.clone(),
            dir,
        });
        Ok((cur, dir))
    }
}

/// Makes the links of the extraction once every file and folder exists,
/// in the order and with the kinds [`plan_links`] decides.
fn make_links(out: &Path, listed: &[(String, Entry)]) -> Result<(), Error> {
    for link in plan_links(listed)? {
        let Some(path) = tool_path(out, &link.name)? else {
            continue;
        };
        make_parents(out, &path, &link.name)?;
        symlink(&link.target, &path, link.dir).map_err(|e| {
            if e.kind() == ErrorKind::AlreadyExists {
                Error::ToolArchive(format!("{}: another entry has this name", link.name))
            } else {
                write_err(&path, &e)
            }
        })?;
    }
    Ok(())
}

/// The bytes a tool archive may still write.
struct Budget {
    written: u64,
    limit: u64,
}

impl Budget {
    fn over(&self) -> Error {
        Error::ToolArchive(format!(
            "the archive unpacks to more than {} bytes, the limit",
            self.limit
        ))
    }
}

/// Copies the data of one entry, within what `budget` leaves: one byte
/// past it, the copy stops and the archive is refused. A failure to read
/// is the archive's, `tool: archive`; a failure to write is the
/// destination's, `io: write`.
fn copy_entry(
    name: &str,
    path: &Path,
    data: &mut dyn Read,
    to: &mut dyn Write,
    budget: &mut Budget,
) -> Result<(), Error> {
    let room = budget.limit.saturating_sub(budget.written);
    let n = copy_bounded(data, to, room).map_err(|e| match e {
        CopyFail::Read(e) => Error::ToolArchive(format!("{name}: the data cannot be read: {e}")),
        CopyFail::Write(e) => write_err(path, &e),
    })?;
    budget.written += n;
    if budget.written > budget.limit {
        return Err(budget.over());
    }
    Ok(())
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
fn symlink(target: &str, path: &Path, _dir: bool) -> io::Result<()> {
    std::os::unix::fs::symlink(target, path)
}

#[cfg(windows)]
fn symlink(target: &str, path: &Path, dir: bool) -> io::Result<()> {
    let target = target.replace('/', "\\");
    if dir {
        std::os::windows::fs::symlink_dir(&target, path)
    } else {
        std::os::windows::fs::symlink_file(&target, path)
    }
}

#[cfg(not(any(unix, windows)))]
fn symlink(_target: &str, _path: &Path, _dir: bool) -> io::Result<()> {
    Err(io::Error::from(ErrorKind::Unsupported))
}

/// Checks, once everything is extracted, that each link inside `folder`, a
/// real path, leads to something that exists inside it. The system
/// resolves the link, so a chain of links counts. A link that leads
/// nowhere, or out of `folder`, is refused: any doubt is a refusal.
/// `what` names `folder` in the message.
fn check_links(folder: &Path, what: &str) -> Result<(), Error> {
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
                        "{}: the link leads outside {what}, or to nothing",
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

fn too_many(limits: Limits) -> Error {
    Error::ToolArchive(format!(
        "the archive holds more than {} entries, the limit",
        limits.tool_entries
    ))
}

fn extract_tool_zip(archive: &Path, out: &Path, limits: Limits) -> Result<(), Error> {
    let mut listed = Vec::new();
    let bad = |d: String| Error::ToolArchive(d);
    let file = File::open(archive).map_err(|e| read_err(archive, &e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| bad(format!("not a zip archive: {e}")))?;
    if zip.len() as u64 > limits.tool_entries {
        return Err(too_many(limits));
    }
    let mut budget = Budget {
        written: 0,
        limit: limits.tool_bytes,
    };
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
            let mut target = Vec::new();
            let n = copy_bounded(&mut f, &mut target, limits.meta)
                .map_err(|e| bad(format!("{name}: the link cannot be read: {}", e.inner())))?;
            if n > limits.meta {
                return Err(bad(format!(
                    "{name}: the link target is longer than {} bytes, the limit",
                    limits.meta
                )));
            }
            let Ok(target) = String::from_utf8(target) else {
                return Err(bad(format!("{name}: the link target is not UTF-8")));
            };
            Entry::Link(target)
        } else if f.is_dir() {
            Entry::Dir
        } else if kind == 0 || kind == 0o100_000 {
            Entry::File(mode)
        } else {
            return Err(bad(format!("{name}: not a file, a folder or a link")));
        };
        put(out, &name, entry, &mut f, &mut listed, &mut budget)?;
    }
    make_links(out, &listed)
}

/// Where a [`Meter`] is in the ungzipped tar stream, and how far it may go.
#[derive(Debug, Clone, Copy)]
struct Mark {
    read: u64,
    limit: u64,
}

/// Counts the bytes of the ungzipped tar stream as they come and fails a
/// read one byte past the limit that the caller moves along.
struct Meter<R> {
    inner: R,
    mark: std::rc::Rc<std::cell::Cell<Mark>>,
}

impl<R: Read> Read for Meter<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let m = self.mark.get();
        let over = || io::Error::other("the tar stream is longer than its limit");
        if m.read > m.limit {
            return Err(over());
        }
        let room = m.limit - m.read + 1;
        let want = usize::try_from(room).map_or(buf.len(), |r| r.min(buf.len()));
        let n = self.inner.read(&mut buf[..want])?;
        let read = m.read + n as u64;
        self.mark.set(Mark { read, ..m });
        if read > m.limit {
            return Err(over());
        }
        Ok(n)
    }
}

/// Reads a tar.gz through a [`Meter`]. The stream may hold at most
/// `tool_bytes` in all, and between the data of two entries at most
/// `meta`: so the long names and pax records that the tar reader holds in
/// memory are bounded too.
fn extract_tool_tar(archive: &Path, out: &Path, limits: Limits) -> Result<(), Error> {
    let bad = |d: String| Error::ToolArchive(d);
    let file = File::open(archive).map_err(|e| read_err(archive, &e))?;
    let mark = std::rc::Rc::new(std::cell::Cell::new(Mark {
        read: 0,
        limit: limits.meta.min(limits.tool_bytes),
    }));
    let mut gz = Meter {
        inner: flate2::read::GzDecoder::new(io::BufReader::new(file)),
        mark: mark.clone(),
    };
    // A failure past a limit is reported as that limit.
    let why = |e: String| {
        let m = mark.get();
        if m.read > limits.tool_bytes {
            Error::ToolArchive(format!(
                "the archive unpacks to more than {} bytes, the limit",
                limits.tool_bytes
            ))
        } else if m.read > m.limit {
            Error::ToolArchive(format!(
                "the tar records between two entries take more than {} bytes, the limit",
                limits.meta
            ))
        } else {
            bad(e)
        }
    };
    let mut budget = Budget {
        written: 0,
        limit: limits.tool_bytes,
    };
    let mut listed = Vec::new();
    {
        let mut tar = tar::Archive::new(&mut gz);
        let entries = tar
            .entries()
            .map_err(|e| why(format!("the tar data cannot be read: {e}")))?;
        let mut count = 0u64;
        for e in entries {
            let mut e = e.map_err(|e| why(format!("the tar data cannot be read: {e}")))?;
            count += 1;
            if count > limits.tool_entries {
                return Err(too_many(limits));
            }
            // The data of this entry, padded to 512 bytes, then at most
            // `meta` up to the data of the next one.
            let m = mark.get();
            let data = e.size().div_ceil(512).saturating_mul(512);
            let limit = m
                .read
                .saturating_add(data)
                .saturating_add(limits.meta)
                .min(limits.tool_bytes);
            mark.set(Mark { limit, ..m });
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
            match put(out, &name, entry, &mut e, &mut listed, &mut budget) {
                Err(Error::ToolArchive(d)) => return Err(why(d)),
                r => r?,
            }
        }
    }
    io::copy(&mut gz, &mut io::sink())
        .map_err(|e| why(format!("the gzip data cannot be read to the end: {e}")))?;
    make_links(out, &listed)
}

#[cfg(test)]
mod tests;
