// SPDX-License-Identifier: GPL-3.0-or-later

//! `robco build`, by "robco build" and "The steps of update, build, run and
//! test" of RobCo-Core `docs/spec/STUDIO_CLI.md`, and "What a holotape build
//! produces" and "When the lock changes" of `docs/spec/STUDIO_PROJECT.md`.
//!
//! It takes steps 1 to 11 of the shared list, in order, and stops at the
//! first failure. Step 3 deletes an older `build/NAME.HTP` right after the
//! manifest, so a build that fails later never leaves an older tape. The
//! lock is written at step 10, only after a resolution and once every
//! install passed. A lock that is there is never written.
//!
//! Inputs and outputs follow two rules. Every path the manifest names, the
//! icon, each source and each source folder, may be a link that stays
//! inside the project root, and one that leads outside fails with
//! `manifest: path`, through [`manifest::inside_root`]; the build's own
//! outputs, the `build` folder, the tape, the temp tape and the objects,
//! are never written or deleted through a link.
//!
//! The build has succeeded once `write_tape` has renamed the tape into
//! place, the last thing step 11 does. Nothing that can fail runs after
//! that point: [`run`] only writes the `result` line, from what it already
//! holds. So a failed build never leaves a new tape.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use studio_core::install::{self, Cached, LocalFiles, Phase, Pin, Progress, sha256_hex};
use studio_core::manifest::{self, Manifest, ManifestError, OnDisk};
use studio_core::plan::{self, BuildPlan, Host, PlanError, PlanInput, Step};
use studio_core::resolve::index::{Index, IndexPackage};
use studio_core::resolve::lock::{self, Installed, LockedPackage, LockedTool, Resolution};
use studio_core::resolve::sdk_json::{self, SdkJson};
use studio_core::resolve::{self, Abi, PIPOS, Target};

use crate::index;
use crate::lines::{Out, Val};
use crate::net::Archives;
use crate::{Failure, Net};

/// The manifest's file name in the project root.
const MANIFEST: &str = "robco.toml";

/// The header magic of a holotape.
const MAGIC: [u8; 8] = *b"ROBCO78\0";

/// The `flags` value of a holotape: the META block is present.
const FLAGS_META: u32 = 1;

/// What `robco build` was asked for.
pub struct Request<'a> {
    /// The `--index` location, when given.
    pub index: Option<&'a str>,
    /// The project root: the current folder.
    pub project: &'a Path,
    /// The running host, or `None` on a system the index has no name for.
    pub host: Option<&'a str>,
}

/// Writes the `step` lines of one install.
struct Lines<'a, 'o> {
    out: &'a mut Out<'o>,
    item: &'a str,
}

impl Progress for Lines<'_, '_> {
    fn start(&mut self, phase: Phase) {
        self.out.step(word(phase), self.item, "start");
    }
    fn done(&mut self, phase: Phase) {
        self.out.step(word(phase), self.item, "done");
    }
    fn skip(&mut self) {
        self.out.step("install", self.item, "skip");
    }
}

fn word(phase: Phase) -> &'static str {
    match phase {
        Phase::Download => "download",
        Phase::Verify => "verify",
        Phase::Install => "install",
    }
}

fn read_failure(path: &Path, e: &std::io::Error) -> Failure {
    Failure::new("io: read", format!("{}: {}", path.display(), e.kind()))
}

fn write_failure(path: &Path, e: &std::io::Error) -> Failure {
    Failure::new("io: write", format!("{}: {}", path.display(), e.kind()))
}

/// The index, read once when a step first needs it.
struct LazyIndex<'a> {
    given: Option<&'a str>,
    net: &'a Net,
    read: Option<(Index, Option<PathBuf>)>,
}

impl LazyIndex<'_> {
    fn get(&mut self) -> Result<&(Index, Option<PathBuf>), Failure> {
        if self.read.is_none() {
            let location = index::locate(self.given)?;
            self.read = Some(index::read(&location, self.net)?);
        }
        // Filled just above.
        self.read
            .as_ref()
            .ok_or_else(|| Failure::new("internal", "the index was not kept"))
    }
}

/// The project after step 2: the manifest and what its files gave.
struct Project {
    manifest: Manifest,
    icon: Option<[u8; manifest::ICON_SIZE]>,
    files: Vec<String>,
    /// The source list, in link order, as step 2 checked it.
    sources: Vec<String>,
}

/// Runs the command, writing every line but the error and the failed
/// `result` line, which the caller writes from the returned failure.
pub fn run(out: &mut Out<'_>, cache: &Path, net: &Net, req: &Request<'_>) -> Result<(), Failure> {
    let root = req.project;
    let host = req.host.unwrap_or("unknown");

    // Before step 2: every folder a command names must be text.
    check_folders(root, cache)?;

    // Step 2.
    let project = read_project(root)?;
    let m = &project.manifest;

    // Step 3: delete the older tape.
    let tape_rel = format!("build/{}.HTP", m.project.name);
    delete_older_tape(root, &tape_rel)?;

    // Step 5, and step 6 without a lock.
    let target = Target {
        platform: PIPOS.to_owned(),
        abi: Abi {
            major: m.target.abi.major,
            minor: m.target.abi.minor,
        },
    };
    let mut idx = LazyIndex {
        given: req.index,
        net,
        read: None,
    };
    let resolution = match lock::read(root).map_err(|e| Failure::of(&e))? {
        Some(l) => {
            l.check_stale(&target, host, None)
                .map_err(|e| Failure::of(&e))?;
            Resolution::Locked(l)
        }
        None => {
            index::locate(req.index)?;
            out.step("resolve", "", "start");
            let (i, _) = idx.get()?;
            let given = req.index.unwrap_or_default();
            let new = resolve::resolve(&target, i, given, host).map_err(|e| Failure::of(&e))?;
            out.step("resolve", "", "done");
            Resolution::New(new)
        }
    };
    let locked = matches!(resolution, Resolution::Locked(_));
    let lk = resolution.lock();

    // Step 7: the packages, sorted by name in byte order.
    let mut packages: Vec<&LockedPackage> = lk.packages.iter().collect();
    packages.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
    for p in &packages {
        install_package(out, cache, &mut idx, p, locked)?;
    }

    // Step 8.
    let mut sdks: Vec<(&LockedPackage, SdkJson)> = Vec::new();
    for p in &packages {
        sdks.push((p, read_sdk(cache, p)?));
    }
    let pipos = sdks
        .iter()
        .find(|(p, _)| p.name == PIPOS)
        .ok_or_else(|| Failure::new("internal", "the lock lists no pipos package"))?;
    if locked {
        let installed_of = |p: &LockedPackage| -> Installed {
            sdks.iter()
                .find(|(q, _)| q.name == p.name)
                .map(|(_, s)| Installed {
                    abi: s.abi,
                    requires: s.requires.clone(),
                })
                .unwrap_or_default()
        };
        lk.check_stale(&target, host, Some(&installed_of))
            .map_err(|e| Failure::of(&e))?;
        lk.check_tools(&pipos.1).map_err(|e| Failure::of(&e))?;
    }

    // Step 9: `gcc` only. `robco build` never installs `qemu`.
    let gcc =
        lk.tools.iter().find(|t| t.name == "gcc").ok_or_else(|| {
            Failure::new("lock: stale", "tool gcc is not listed: run robco update")
        })?;
    let toolchain = install_gcc(out, cache, &mut idx, gcc, host, locked)?;

    // Step 10.
    if let Resolution::New(new) = &resolution {
        lock::write(root, new).map_err(|e| Failure::of(&e))?;
    }

    // Step 11.
    let pkg_dir = package_dir(cache, pipos.0)?;
    let built = build(out, root, &project, &pkg_dir, &toolchain)?;

    let size = built.len();
    let sha = sha256_hex(&built);
    let elf = format!("build/{}.elf", m.project.name);
    out.text(&format!("built {tape_rel}, {size} bytes, sha256 {sha}"));
    // Nothing below can fail: the build has succeeded once the tape is
    // written, and only the result line is left.
    let sources = project.sources.clone();
    out.ok(
        "build",
        vec![
            ("tape", Val::s(tape_rel)),
            ("elf", Val::s(elf)),
            ("size", Val::Int(size as u64)),
            ("sha256", Val::s(sha)),
            (
                "abi",
                Val::s(format!("{}.{}", m.target.abi.major, m.target.abi.minor)),
            ),
            ("gcc", Val::s(gcc.version.clone())),
            (
                "sources",
                Val::Arr(sources.into_iter().map(Val::s).collect()),
            ),
        ],
    );
    Ok(())
}

/// Step 2: reads `robco.toml` in the six steps of "Order of checks": the
/// file, its keys, then the icon file and the source patterns.
fn read_project(root: &Path) -> Result<Project, Failure> {
    let path = root.join(MANIFEST);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(Failure::new(
                "project: none",
                format!("the current folder holds no {MANIFEST}"),
            ));
        }
        Err(e) => return Err(read_failure(&path, &e)),
    };
    let manifest = manifest::parse(&bytes).map_err(|e| manifest_failure(&e))?;
    let icon = match &manifest.project.icon {
        None => None,
        Some(rel) => {
            let data = match manifest_file(root, "project.icon", rel)? {
                Found::Missing => None,
                Found::NotFile => {
                    return Err(manifest_failure(&ManifestError::Icon(format!(
                        "{rel}: not a regular file"
                    ))));
                }
                Found::File(p) => match fs::read(&p) {
                    Ok(d) => Some(d),
                    Err(e) if e.kind() == ErrorKind::NotFound => None,
                    Err(e) => return Err(read_failure(&p, &e)),
                },
            };
            Some(manifest::read_icon(rel, data.as_deref()).map_err(|e| manifest_failure(&e))?)
        }
    };
    let (files, sources) = plan_sources(root, &manifest.build.sources)?;
    Ok(Project {
        manifest,
        icon,
        files,
        sources,
    })
}

/// [`manifest::inside_root`] with its errors as failures: a link out of
/// the root or a link loop is `manifest: path`, a host refusal `io: read`
/// with the path, and nothing there `None`.
fn resolve(root: &Path, key: &str, rel: &str) -> Result<Option<PathBuf>, Failure> {
    match manifest::inside_root(root, key, rel).map_err(|e| manifest_failure(&e))? {
        OnDisk::Inside(p) => Ok(Some(p)),
        OnDisk::Missing => Ok(None),
        OnDisk::Unreadable { path, kind } => Err(Failure::new(
            "io: read",
            format!("{}: {kind}", path.display()),
        )),
    }
}

/// What a manifest path resolves to, after the path rule.
enum Found {
    /// A regular file, a link to one included: the path to read.
    File(PathBuf),
    /// Something that is not a regular file, such as a folder.
    NotFile,
    /// Nothing, a dangling link included.
    Missing,
}

/// A manifest path must resolve to a regular file. Runs [`resolve`], then
/// looks at the type, so a folder is never read: reading one gives
/// `IsADirectory` on Unix and `PermissionDenied` on Windows, and neither
/// is the host refusing a file. The caller gives its key's error for
/// [`Found::NotFile`].
fn manifest_file(root: &Path, key: &str, rel: &str) -> Result<Found, Failure> {
    let Some(p) = resolve(root, key, rel)? else {
        return Ok(Found::Missing);
    };
    match fs::metadata(&p) {
        Ok(md) if md.is_file() => Ok(Found::File(p)),
        Ok(_) => Ok(Found::NotFile),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Found::Missing),
        Err(e) => Err(read_failure(&p, &e)),
    }
}

fn not_a_file(src: &str) -> Failure {
    Failure::new("manifest: sources", format!("{src}: not a regular file"))
}

fn manifest_failure(e: &ManifestError) -> Failure {
    Failure::of(e)
}

/// The matched files, sorted, and the source list in link order, each
/// source by its manifest path.
///
/// Step 6 of "Order of checks" takes "each pattern of `sources` in order,
/// `manifest: path` and `manifest: sources`". So one pattern is checked
/// whole before the next: its folder and its matches against the path
/// rule, then its own `sources` rules. A later pattern that matches
/// nothing never hides an earlier pattern's link out of the root.
///
/// A pattern's `*` stands only in its last component, so a pattern reaches
/// one folder and no other folder is read. A folder that is missing or not
/// a folder gives no files, and the pattern then matches nothing. Links
/// are followed: a folder or a file that leads outside the project root
/// fails with `manifest: path`.
fn plan_sources(root: &Path, patterns: &[String]) -> Result<(Vec<String>, Vec<String>), Failure> {
    let mut files = Vec::new();
    for pattern in patterns {
        let rel = pattern.rsplit_once('/').map_or("", |(d, _)| d);
        let dir = if rel.is_empty() {
            Some(root.to_path_buf())
        } else {
            resolve(root, "build.sources", rel)?
        };
        let mut matched = Vec::new();
        if let Some(dir) = dir.filter(|d| d.is_dir()) {
            list_dir(root, &dir, rel, std::slice::from_ref(pattern), &mut matched)?;
        }
        matched.sort();
        for src in &matched {
            match manifest_file(root, "build.sources", src)? {
                Found::File(_) => {}
                Found::NotFile => return Err(not_a_file(src)),
                Found::Missing => {
                    return Err(Failure::new(
                        "manifest: sources",
                        format!("{src}: gone while the build read it"),
                    ));
                }
            }
        }
        let refs: Vec<&str> = matched.iter().map(String::as_str).collect();
        manifest::expand_sources(std::slice::from_ref(pattern), &refs)
            .map_err(|e| manifest_failure(&e))?;
        files.extend(matched);
    }
    files.sort();
    files.dedup();
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let list = manifest::expand_sources(patterns, &refs).map_err(|e| manifest_failure(&e))?;
    Ok((files, list))
}

/// The regular files of one folder, not of its subfolders, a link to a
/// regular file included, that a source pattern matches. A dangling link
/// is no file, and a pattern without `*` that names a folder or another
/// kind fails with `manifest: sources`. Names are matched first, by the manifest's own matcher, and
/// only a matched entry is looked at, so an entry no pattern matches never
/// fails the build. A matched entry goes through [`resolve`] before its
/// type is read, through [`manifest_file`], so one rule decides for links out, loops, dangling links
/// and host refusals.
fn list_dir(
    root: &Path,
    dir: &Path,
    rel: &str,
    patterns: &[String],
    out: &mut Vec<String>,
) -> Result<(), Failure> {
    let entries = fs::read_dir(dir).map_err(|e| read_failure(dir, &e))?;
    for entry in entries {
        let entry = entry.map_err(|e| read_failure(dir, &e))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            // A name that is not UTF-8 cannot match a pattern.
            continue;
        };
        let project_path = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if !patterns
            .iter()
            .any(|p| manifest::pattern_matches(p, &project_path))
        {
            continue;
        }
        match manifest_file(root, "build.sources", &project_path)? {
            Found::File(_) => out.push(project_path),
            // A pattern without `*` names this one entry, so it must be a
            // file; a wildcard keeps only regular files, as the spec says.
            Found::NotFile if !patterns.iter().any(|p| p.contains('*')) => {
                return Err(not_a_file(&project_path));
            }
            Found::NotFile | Found::Missing => {}
        }
    }
    Ok(())
}

fn package_dir(cache: &Path, p: &LockedPackage) -> Result<PathBuf, Failure> {
    install::package_dir(cache, &p.name, &p.version.to_string()).ok_or_else(|| {
        Failure::new(
            "package: archive",
            format!("package {} {}: not a cache folder name", p.name, p.version),
        )
    })
}

/// Step 7 for one package. A package the cache holds is compared with the
/// lock and never downloaded, so a build with a full cache reads no index.
fn install_package(
    out: &mut Out<'_>,
    cache: &Path,
    idx: &mut LazyIndex<'_>,
    p: &LockedPackage,
    locked: bool,
) -> Result<(), Failure> {
    let item = format!("{}@{}", p.name, p.version);
    let dir = package_dir(cache, p)?;
    if locked && let Cached::Holds(hash) = install::cached(&dir).map_err(|e| Failure::of(&e))? {
        p.check_hash(&hash).map_err(|e| Failure::of(&e))?;
        out.step("install", &item, "skip");
        return Ok(());
    }
    let net = idx.net;
    let (i, base) = idx.get()?;
    let entry: &IndexPackage = if locked {
        p.find_in(i).map_err(|e| Failure::of(&e))?
    } else {
        i.packages
            .iter()
            .find(|e| e.name == p.name && e.version == p.version)
            .ok_or_else(|| Failure::new("internal", "a picked package left the index"))?
    };
    let transport = Archives {
        local: base.clone().map(|base| LocalFiles { base }),
        net,
    };
    let pin = if locked { Pin::Lock } else { Pin::Index };
    let mut lines = Lines { out, item: &item };
    install::package_watched(cache, entry, &transport, pin, &mut lines)
        .map_err(|e| Failure::of(&e))?;
    Ok(())
}

/// Step 8 for one package: its installed `sdk.json`. A cached package whose
/// `sdk.json` is gone, or is a folder, fails with `package: manifest`, the
/// code step 8 gives, and the cache is left as it is: the spec names no
/// repair of a cache entry, only reports. A host that refuses to read the
/// file is `io: read`.
fn read_sdk(cache: &Path, p: &LockedPackage) -> Result<SdkJson, Failure> {
    let path = package_dir(cache, p)?.join("sdk.json");
    let bytes = read_package_file(&path, "sdk.json")?;
    sdk_json::parse(&bytes).map_err(|e| Failure::of(&e))
}

/// Reads a file of an installed package. One that is missing or not a
/// regular file is `package: manifest`, naming the file; a regular file
/// the host refuses to read is `io: read`. The path's type is looked at
/// first, so the answer does not hang on which error a system gives for
/// reading a folder: Unix says `IsADirectory`, Windows `PermissionDenied`.
fn read_package_file(path: &Path, name: &str) -> Result<Vec<u8>, Failure> {
    let missing = || {
        Failure::new(
            "package: manifest",
            format!("{name} is missing from the installed package"),
        )
    };
    match fs::metadata(path) {
        Ok(md) if md.is_file() => {}
        Ok(_) => return Err(missing()),
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(missing()),
        Err(e) => return Err(read_failure(path, &e)),
    }
    fs::read(path).map_err(|e| read_failure(path, &e))
}

/// Step 9: installs the locked `gcc` for the running host, or finds it.
/// Returns the folder that holds its `bin/`.
fn install_gcc(
    out: &mut Out<'_>,
    cache: &Path,
    idx: &mut LazyIndex<'_>,
    gcc: &LockedTool,
    host: &str,
    locked: bool,
) -> Result<PathBuf, Failure> {
    let item = format!("{}@{}/{host}", gcc.name, gcc.version);
    let dir = install::tool_dir(cache, &gcc.name, &gcc.version, host).ok_or_else(|| {
        Failure::new(
            "tool: archive",
            format!(
                "tool {} {} {host}: not a cache folder name",
                gcc.name, gcc.version
            ),
        )
    })?;
    if locked && let Cached::Holds(hash) = install::cached(&dir).map_err(|e| Failure::of(&e))? {
        if gcc.archives.get(host) != Some(&hash) {
            return Err(Failure::new(
                "lock: hash",
                format!(
                    "tool {} {} {host} does not match its lock hash",
                    gcc.name, gcc.version
                ),
            ));
        }
        out.step("install", &item, "skip");
        return Ok(dir);
    }
    let net = idx.net;
    let (i, base) = idx.get()?;
    let archive = if locked {
        gcc.find_in(i, host).map_err(|e| Failure::of(&e))?
    } else {
        i.tools
            .iter()
            .filter(|t| t.name == gcc.name && t.version == gcc.version)
            .find_map(|t| t.archives.get(host))
            .ok_or_else(|| Failure::new("internal", "a picked tool left the index"))?
    };
    let transport = Archives {
        local: base.clone().map(|base| LocalFiles { base }),
        net,
    };
    let pin = if locked { Pin::Lock } else { Pin::Index };
    let mut lines = Lines { out, item: &item };
    install::tool_watched(
        cache,
        [&gcc.name, &gcc.version, host],
        archive,
        &transport,
        pin,
        &mut lines,
    )
    .map_err(|e| Failure::of(&e))?;
    Ok(dir)
}

/// The first component of `p` that is not valid UTF-8, shown lossily.
fn non_utf8_part(p: &Path) -> Option<String> {
    p.components()
        .map(Component::as_os_str)
        .find(|c| c.to_str().is_none())
        .map(|c| c.to_string_lossy().into_owned())
}

/// Refuses a cache or project folder whose path is not valid UTF-8, before
/// anything is deleted or installed. The plan, its path maps and the error
/// details carry paths as text, and the maps must spell the folder exactly
/// as the compiler sees it, so such a folder cannot be built in. The cache
/// is checked first, as STUDIO_CLI.md checks it before any file is read,
/// and with it every package and tool folder, whose names under it are
/// checked text. The spec has no code for a project folder the tool cannot
/// use: `manifest: path` is the closest, since every manifest path lies
/// under that folder.
fn check_folders(root: &Path, cache: &Path) -> Result<(), Failure> {
    let refuse = |code, what: &str, p: &Path, part: String| {
        Err(Failure::new(
            code,
            format!(
                "{}: the {what} folder's path must be valid UTF-8, and {part} is not",
                p.display()
            ),
        ))
    };
    if let Some(part) = non_utf8_part(cache) {
        return refuse("cache: path", "cache", cache, part);
    }
    if let Some(part) = non_utf8_part(root) {
        return refuse("manifest: path", "project", root, part);
    }
    Ok(())
}

/// A folder as text. [`check_folders`] ran first, so a failure here is a
/// fault of the tool.
fn path_text(p: &Path) -> Result<String, Failure> {
    p.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Failure::new("internal", format!("{}: not a UTF-8 path", p.display())))
}

/// Step 11: compiles, links, seals, validates and writes the tape. Returns
/// its bytes.
fn build(
    out: &mut Out<'_>,
    root: &Path,
    project: &Project,
    pkg_dir: &Path,
    toolchain: &Path,
) -> Result<Vec<u8>, Failure> {
    let m = &project.manifest;
    // 1. flags.json.
    let flags_path = pkg_dir.join("flags.json");
    let flags_json = read_package_file(&flags_path, "flags.json")?;
    let files: Vec<&str> = project.files.iter().map(String::as_str).collect();
    let (package, toolchain_s, project_s) =
        (path_text(pkg_dir)?, path_text(toolchain)?, path_text(root)?);
    let bp: BuildPlan = plan::plan(&PlanInput {
        manifest: m,
        flags_json: &flags_json,
        package: &package,
        toolchain: &toolchain_s,
        project: &project_s,
        files: &files,
        host: Host::current(),
    })
    .map_err(|e| match e {
        PlanError::Flags(f) => Failure::new(
            if f.code() == "format" {
                "package: format"
            } else {
                "package: manifest"
            },
            format!("flags.json: {}", f.detail()),
        ),
        PlanError::Manifest(e) => manifest_failure(&e),
    })?;

    // 2. The two programs.
    for c in &bp.commands {
        if !Path::new(&c.program).is_file() {
            return Err(Failure::new(
                "tool: program",
                format!("{} is not in the gcc tool", program_name(&c.program)),
            ));
        }
    }

    for d in &bp.dirs {
        out_dir(root, d)?;
    }
    // The plan's own record of the outputs, never a scan of the arguments.
    for o in &bp.outputs {
        if !in_build(o) {
            return Err(Failure::new(
                "internal",
                format!("{o}: a planned output is not inside build/"),
            ));
        }
        remove_output(&root.join(o))?;
    }
    // One output per command, in the same order.
    if bp.outputs.len() != bp.commands.len() {
        return Err(Failure::new(
            "internal",
            "the plan does not name one output per command",
        ));
    }

    // 3 to 5.
    for (c, o) in bp.commands.iter().zip(&bp.outputs) {
        let (step, item, code, program) = match &c.step {
            Step::CompileCrt0 => ("compile", "crt0".to_owned(), "build: compile", "gcc"),
            Step::Compile(s) => ("compile", s.clone(), "build: compile", "gcc"),
            Step::Link => ("link", String::new(), "build: link", "gcc"),
            Step::Objcopy => ("objcopy", String::new(), "build: objcopy", "objcopy"),
        };
        out.step(step, &item, "start");
        // A source the host does not let the tool read is `io: read`, not a
        // compile error, checked at its own compile step as the spec's
        // order has it. Headers are the compiler's business.
        if let Step::Compile(src) = &c.step {
            open_source(root, src)?;
        }
        let output = Command::new(&c.program)
            .args(&c.args)
            .current_dir(&c.cwd)
            .output()
            // A program the tool folder holds but the host cannot start is
            // not a program the command can start: `tool: program`.
            .map_err(|e| {
                Failure::new(
                    "tool: program",
                    format!("{} could not start: {}", program_name(&c.program), e.kind()),
                )
            })?;
        for bytes in [&output.stdout, &output.stderr] {
            for line in String::from_utf8_lossy(bytes).lines() {
                out.diagnostic(program, line);
            }
        }
        if !output.status.success() {
            let detail = if item.is_empty() {
                format!("{program} ended with {}", output.status)
            } else {
                item.clone()
            };
            return Err(Failure::new(code, detail));
        }
        // Each output was removed above, so a file there now is one this
        // run wrote. A program that ends well and writes nothing, as with
        // `-fsyntax-only`, fails its own step.
        if !wrote_file(&root.join(o)) {
            let what = if item.is_empty() {
                program.to_owned()
            } else {
                format!("{item}: {program}")
            };
            return Err(Failure::new(code, format!("{what} wrote no output {o}")));
        }
        out.step(step, &item, "done");
    }

    // 6. Seal and validate.
    out.step("seal", "", "start");
    let raw_path = root.join(&bp.raw);
    let raw = fs::read(&raw_path).map_err(|e| read_failure(&raw_path, &e))?;
    let sealed = seal(m, project.icon.as_ref(), &raw)?;
    out.step("seal", "", "done");

    // 7. Write the tape. The ELF is where the link wrote it.
    write_tape(root, &m.project.name, &sealed)?;
    Ok(sealed)
}

/// Opens a source for reading, and gives `io: read` with its path when
/// the host refuses.
fn open_source(root: &Path, src: &str) -> Result<(), Failure> {
    let path = root.join(src);
    fs::File::open(&path).map_err(|e| read_failure(&path, &e))?;
    Ok(())
}

fn link_failure(path: &Path) -> Failure {
    Failure::new(
        "io: write",
        format!(
            "{}: a symbolic link or not a folder, not followed",
            path.display()
        ),
    )
}

/// Step 3: deletes `build/NAME.HTP`. A `build` that is a link is refused,
/// so no file outside the project is deleted. A tape that is a link is
/// removed as a link.
fn delete_older_tape(root: &Path, tape_rel: &str) -> Result<(), Failure> {
    let build = root.join("build");
    match fs::symlink_metadata(&build) {
        Ok(md) if md.is_dir() => {}
        Ok(_) => return Err(link_failure(&build)),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(write_failure(&build, &e)),
    }
    let tape = root.join(tape_rel);
    match fs::remove_file(&tape) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(write_failure(&tape, &e)),
    }
}

/// Creates the folder `rel` under the root one component at a time. A
/// component that is a link or not a folder is refused, so the build never
/// writes through a link.
fn out_dir(root: &Path, rel: &str) -> Result<(), Failure> {
    let mut path = root.to_path_buf();
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        path.push(part);
        if let Err(e) = fs::create_dir(&path)
            && e.kind() != ErrorKind::AlreadyExists
        {
            return Err(write_failure(&path, &e));
        }
        match fs::symlink_metadata(&path) {
            Ok(md) if md.is_dir() => {}
            Ok(_) => return Err(link_failure(&path)),
            Err(e) => return Err(write_failure(&path, &e)),
        }
    }
    Ok(())
}

/// True when `rel` is a relative path, inside `build/` by its components,
/// split on `/`, with no empty, `.`, `..`, root or prefix component.
fn in_build(rel: &str) -> bool {
    let mut parts = rel.split('/');
    parts.next() == Some("build")
        && parts.clone().next().is_some()
        && parts.all(|c| {
            !c.is_empty()
                && !c.contains('\\')
                && c != "."
                && c != ".."
                && matches!(Path::new(c).components().next(), Some(Component::Normal(_)))
        })
}

/// Removes whatever file is at a path a compiler or `objcopy` writes, a
/// link or a regular file from an earlier build, before any command runs.
/// A link is removed as a link, so its target is kept. So the program
/// writes a new file there, and a file there after the command is one
/// this run wrote. A folder there is refused.
fn remove_output(path: &Path) -> Result<(), Failure> {
    match fs::symlink_metadata(path) {
        Ok(md) if md.is_dir() => Err(link_failure(path)),
        Ok(_) => fs::remove_file(path).map_err(|e| write_failure(path, &e)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(write_failure(path, &e)),
    }
}

/// True when a regular file, not a link, is at `path`.
fn wrote_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|md| md.file_type().is_file())
}

/// Writes `build/NAME.HTP` through `build/NAME.HTP.tmp`, renamed over it.
/// A stale regular temporary file is removed first; any other entry there
/// is refused. The temporary file is created new, so nothing is followed.
fn write_tape(root: &Path, name: &str, bytes: &[u8]) -> Result<(), Failure> {
    let tape = root.join(format!("build/{name}.HTP"));
    let tmp = root.join(format!("build/{name}.HTP.tmp"));
    match fs::symlink_metadata(&tmp) {
        Ok(md) if md.file_type().is_file() => {
            fs::remove_file(&tmp).map_err(|e| write_failure(&tmp, &e))?;
        }
        Ok(_) => return Err(link_failure(&tmp)),
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(write_failure(&tmp, &e)),
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| write_failure(&tmp, &e))?;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| write_failure(&tmp, &e))
        .and_then(|()| {
            drop(file);
            fs::rename(&tmp, &tape).map_err(|e| write_failure(&tape, &e))
        });
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

fn program_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Seals the raw image with the manifest's fields, then validates it and
/// checks the header fields that the startup code wrote against the
/// manifest. Any failure is `build: seal`.
fn seal(
    m: &Manifest,
    icon: Option<&[u8; manifest::ICON_SIZE]>,
    raw: &[u8],
) -> Result<Vec<u8>, Failure> {
    let fail = |d: String| Failure::new("build: seal", d);
    let opts = studio_sys::SealOpts {
        name: Some(&m.project.name),
        image_version: Some(m.project.version.image_version()),
        title: Some(&m.project.title),
        icon: icon.map(|b| studio_sys::Icon {
            bytes: b,
            format: studio_sys::IconFormat::Raw,
        }),
    };
    let sealed = studio_sys::rimg_seal(raw, &opts).map_err(|e| fail(e.text))?;
    let file_size = u32::try_from(sealed.len()).map_err(|_| fail("size".to_owned()))?;
    let hdr = studio_sys::rimg_validate(
        &sealed,
        &studio_sys::Expect {
            magic: Some(MAGIC),
            abi_major: u32::from(m.target.abi.major),
            file_size,
            ..Default::default()
        },
    )
    .map_err(|e| fail(e.text))?;
    let (abi, subtype, flags) = (hdr.abi_version, hdr.subtype, hdr.flags);
    if flags != FLAGS_META {
        return Err(fail(format!("version: flags is {flags}, not 1")));
    }
    if abi != m.target.abi.abi_version() {
        return Err(fail(format!(
            "abi: the startup code wrote abi_version 0x{abi:08x}, robco.toml asks 0x{:08x}",
            m.target.abi.abi_version()
        )));
    }
    if subtype != u32::from(m.project.subtype.value()) {
        return Err(fail(format!(
            "bounds: the startup code wrote subtype {subtype}, robco.toml asks {}",
            m.project.subtype.value()
        )));
    }
    Ok(sealed)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test; a failed unwrap is a failed test"
)]
mod tests {
    use super::*;

    #[test]
    fn a_package_file_that_is_a_folder_is_a_manifest_error() {
        let p = std::env::temp_dir().join(format!("robco-unit-pkgdir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        for name in ["sdk.json", "flags.json"] {
            fs::create_dir_all(p.join(name)).unwrap();
            let e = read_package_file(&p.join(name), name).unwrap_err();
            assert_eq!(e.code, "package: manifest", "{name}");
            assert_eq!(
                e.detail,
                format!("{name} is missing from the installed package")
            );
            let e = read_package_file(&p.join("gone").join(name), name).unwrap_err();
            assert_eq!(e.code, "package: manifest", "{name}");
        }
        let _ = fs::remove_dir_all(&p);
    }

    #[test]
    fn in_build_takes_only_paths_under_build() {
        for ok in ["build/x.elf", "build/obj/src/a.c.o"] {
            assert!(in_build(ok), "{ok}");
        }
        for bad in [
            "/build/x",
            "build",
            "x/build/y",
            "build/../x",
            "build/./x",
            "build//x",
            "build\\x",
            "/etc/passwd",
            "out.o",
            "",
        ] {
            assert!(!in_build(bad), "{bad}");
        }
    }

    const MANIFEST: &str = "format = 1\n[project]\nname = \"TINY\"\nversion = \"1.0.0\"\n\
        kind = \"holotape\"\nsubtype = \"utility\"\n[target]\nplatform = \"pipos\"\nabi = \"1.4\"\n";

    /// A raw holotape image as the startup code would leave it, with the
    /// ABI and subtype given.
    fn raw(abi: u32, subtype: i64) -> Vec<u8> {
        studio_sys::rimg_pack(
            &[0u8; 64],
            &studio_sys::PackOpts {
                magic: "ROBCO78",
                subtype,
                load_addr: 0x0080_0000,
                abi_version: Some(abi),
                ..Default::default()
            },
        )
        .unwrap()
    }

    /// A project folder removed when the test ends.
    #[cfg(unix)]
    struct Dir(PathBuf);

    #[cfg(unix)]
    impl Dir {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("robco-unit-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(p.join("src")).unwrap();
            fs::write(p.join("robco.toml"), MANIFEST).unwrap();
            fs::write(p.join("src/main.c"), "int main(void) { return 0; }\n").unwrap();
            Self(p)
        }
    }

    #[cfg(unix)]
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_source_that_cannot_be_read_is_io_read() {
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new("unreadable-source");
        let main = d.0.join("src/main.c");
        fs::set_permissions(&main, fs::Permissions::from_mode(0o000)).unwrap();
        // Root reads any file: the test then has nothing to check.
        let readable = fs::File::open(&main).is_ok();
        let got = open_source(&d.0, "src/main.c");
        fs::set_permissions(&main, fs::Permissions::from_mode(0o644)).unwrap();
        if readable {
            return;
        }
        let e = got.err().unwrap();
        assert_eq!(e.code, "io: read");
        assert!(e.detail.contains("src/main.c"), "{}", e.detail);
        assert!(open_source(&d.0, "src/main.c").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_folder_no_pattern_reaches_is_not_read() {
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new("unreadable");
        let nm = d.0.join("node_modules");
        fs::create_dir(&nm).unwrap();
        fs::set_permissions(&nm, fs::Permissions::from_mode(0o000)).unwrap();
        let got = read_project(&d.0);
        fs::set_permissions(&nm, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            got.map(|p| p.files).ok(),
            Some(vec!["src/main.c".to_owned()])
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_folder_a_pattern_reaches_fails_and_is_named() {
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new("unreadable-src");
        let src = d.0.join("src");
        fs::set_permissions(&src, fs::Permissions::from_mode(0o000)).unwrap();
        let got = read_project(&d.0);
        fs::set_permissions(&src, fs::Permissions::from_mode(0o755)).unwrap();
        let e = got.err().unwrap();
        assert_eq!(e.code, "io: read");
        assert!(e.detail.contains("src"), "{}", e.detail);
    }

    #[cfg(unix)]
    #[test]
    fn a_pattern_through_a_folder_without_search_is_io_read() {
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new("nosearch");
        fs::create_dir(d.0.join("src/sub")).unwrap();
        fs::write(
            d.0.join("robco.toml"),
            format!("{MANIFEST}[build]\nsources = [\"src/sub/*.c\"]\n"),
        )
        .unwrap();
        let src = d.0.join("src");
        fs::set_permissions(&src, fs::Permissions::from_mode(0o600)).unwrap();
        let readable = fs::metadata(src.join("sub")).is_ok();
        let got = read_project(&d.0);
        fs::set_permissions(&src, fs::Permissions::from_mode(0o755)).unwrap();
        if readable {
            println!("skipped: this process can search a folder without search permission");
            return;
        }
        let e = got.err().unwrap();
        assert_eq!(
            (e.code, e.detail.ends_with("src/sub: permission denied")),
            ("io: read", true),
            "{}",
            e.detail
        );
    }

    /// `d/build` and a file outside the project, `outside`, removed with it.
    #[cfg(unix)]
    fn outside(d: &Dir) -> PathBuf {
        let o = d.0.with_extension("outside");
        let _ = fs::remove_dir_all(&o);
        fs::create_dir_all(&o).unwrap();
        fs::write(o.join("keep"), b"keep").unwrap();
        o
    }

    #[cfg(unix)]
    #[test]
    fn a_temporary_tape_that_is_a_link_is_not_written_through() {
        let d = Dir::new("tmp-link");
        let o = outside(&d);
        fs::create_dir(d.0.join("build")).unwrap();
        std::os::unix::fs::symlink(o.join("keep"), d.0.join("build/TINY.HTP.tmp")).unwrap();
        let got = write_tape(&d.0, "TINY", b"tape");
        let kept = fs::read(o.join("keep")).unwrap();
        let tape = fs::symlink_metadata(d.0.join("build/TINY.HTP")).ok();
        fs::remove_dir_all(&o).unwrap();
        assert_eq!(kept, b"keep");
        assert!(got.is_err());
        assert!(tape.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_stale_temporary_tape_is_replaced() {
        let d = Dir::new("tmp-stale");
        fs::create_dir(d.0.join("build")).unwrap();
        fs::write(d.0.join("build/TINY.HTP.tmp"), b"old").unwrap();
        write_tape(&d.0, "TINY", b"tape").unwrap();
        assert_eq!(fs::read(d.0.join("build/TINY.HTP")).unwrap(), b"tape");
        assert!(!d.0.join("build/TINY.HTP.tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_build_folder_that_is_a_link_is_refused() {
        let d = Dir::new("build-link");
        let o = outside(&d);
        fs::write(o.join("TINY.HTP"), b"theirs").unwrap();
        std::os::unix::fs::symlink(&o, d.0.join("build")).unwrap();
        let deleted = delete_older_tape(&d.0, "build/TINY.HTP");
        let made = out_dir(&d.0, "build/obj/src");
        let (tape, obj) = (o.join("TINY.HTP").exists(), o.join("obj").exists());
        fs::remove_dir_all(&o).unwrap();
        assert!(deleted.is_err() && made.is_err());
        assert!(tape && !obj);
    }

    #[cfg(unix)]
    #[test]
    fn an_output_that_is_a_link_is_dropped_and_its_target_kept() {
        let d = Dir::new("elf-link");
        let o = outside(&d);
        fs::create_dir(d.0.join("build")).unwrap();
        let elf = d.0.join("build/TINY.elf");
        std::os::unix::fs::symlink(o.join("keep"), &elf).unwrap();
        remove_output(&elf).unwrap();
        let (gone, kept) = (fs::symlink_metadata(&elf).is_err(), o.join("keep").exists());
        fs::remove_dir_all(&o).unwrap();
        assert!(gone && kept);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_path_that_is_not_utf8_is_refused_and_named() {
        use std::os::unix::ffi::OsStrExt;
        let bad = Path::new("/x").join(std::ffi::OsStr::from_bytes(b"p\xffq"));
        let good = Path::new("/x/cache");
        assert!(check_folders(Path::new("/x/proj"), good).is_ok());
        let e = check_folders(&bad.join("proj"), good).unwrap_err();
        assert_eq!(e.code, "manifest: path");
        assert!(
            e.detail
                .contains("project folder's path must be valid UTF-8, and p\u{fffd}q is not"),
            "{}",
            e.detail
        );
        let e = check_folders(Path::new("/x/proj"), &bad).unwrap_err();
        assert_eq!(e.code, "cache: path");
        assert!(e.detail.contains("cache folder"), "{}", e.detail);
    }

    #[test]
    fn a_tape_as_the_manifest_asks_is_sealed() {
        let m = manifest::parse(MANIFEST.as_bytes()).unwrap();
        assert!(seal(&m, None, &raw(0x0001_0004, 1)).is_ok());
    }

    #[test]
    fn a_startup_code_of_another_abi_fails_the_seal() {
        let m = manifest::parse(MANIFEST.as_bytes()).unwrap();
        let e = seal(&m, None, &raw(0x0001_0003, 1)).unwrap_err();
        assert_eq!(e.code, "build: seal");
        assert!(e.detail.starts_with("abi"), "{}", e.detail);
    }

    #[test]
    fn a_startup_code_of_another_subtype_fails_the_seal() {
        let m = manifest::parse(MANIFEST.as_bytes()).unwrap();
        let e = seal(&m, None, &raw(0x0001_0004, 0)).unwrap_err();
        assert_eq!(e.code, "build: seal");
    }

    #[test]
    fn a_seal_error_names_the_rc_code() {
        let m = manifest::parse(MANIFEST.as_bytes()).unwrap();
        let e = seal(&m, None, &[0u8; 16]).unwrap_err();
        assert_eq!((e.code, e.detail.as_str()), ("build: seal", "io"));
    }
}
