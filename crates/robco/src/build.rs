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

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use studio_core::install::{self, Cached, LocalFiles, Phase, Pin, Progress, sha256_hex};
use studio_core::manifest::{self, Manifest, ManifestError};
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
}

/// Runs the command, writing every line but the error and the failed
/// `result` line, which the caller writes from the returned failure.
pub fn run(out: &mut Out<'_>, cache: &Path, net: &Net, req: &Request<'_>) -> Result<(), Failure> {
    let root = req.project;
    let host = req.host.unwrap_or("unknown");

    // Step 2.
    let project = read_project(root)?;
    let m = &project.manifest;

    // Step 3: delete the older tape.
    let tape_rel = format!("build/{}.HTP", m.project.name);
    let tape = root.join(&tape_rel);
    match fs::remove_file(&tape) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(write_failure(&tape, &e)),
    }

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
    let sources = plan_sources(&project)?;
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
            let p = root.join(rel);
            let data = match fs::read(&p) {
                Ok(d) => Some(d),
                Err(e) if e.kind() == ErrorKind::NotFound => None,
                Err(e) => return Err(read_failure(&p, &e)),
            };
            Some(manifest::read_icon(rel, data.as_deref()).map_err(|e| manifest_failure(&e))?)
        }
    };
    let mut files = Vec::new();
    list_files(root, "", &mut files)?;
    files.sort();
    let project = Project {
        manifest,
        icon,
        files,
    };
    plan_sources(&project)?;
    Ok(project)
}

fn manifest_failure(e: &ManifestError) -> Failure {
    Failure::of(e)
}

/// The source list, in link order.
fn plan_sources(p: &Project) -> Result<Vec<String>, Failure> {
    let files: Vec<&str> = p.files.iter().map(String::as_str).collect();
    manifest::expand_sources(&p.manifest.build.sources, &files).map_err(|e| manifest_failure(&e))
}

/// Every regular file under `dir`, relative to the project root with `/`.
/// A link is not followed.
fn list_files(root: &Path, rel: &str, out: &mut Vec<String>) -> Result<(), Failure> {
    let dir = if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };
    let entries = fs::read_dir(&dir).map_err(|e| read_failure(&dir, &e))?;
    for entry in entries {
        let entry = entry.map_err(|e| read_failure(&dir, &e))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            // A name that is not UTF-8 cannot match a pattern.
            continue;
        };
        let path = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let kind = entry
            .file_type()
            .map_err(|e| read_failure(&entry.path(), &e))?;
        if kind.is_dir() {
            list_files(root, &path, out)?;
        } else if kind.is_file() {
            out.push(path);
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

/// Step 8 for one package: its installed `sdk.json`.
fn read_sdk(cache: &Path, p: &LockedPackage) -> Result<SdkJson, Failure> {
    let path = package_dir(cache, p)?.join("sdk.json");
    let bytes = fs::read(&path).map_err(|e| read_failure(&path, &e))?;
    sdk_json::parse(&bytes).map_err(|e| Failure::of(&e))
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

fn path_text(p: &Path) -> Result<String, Failure> {
    p.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Failure::new("io: read", format!("{}: not a UTF-8 path", p.display())))
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
    let flags_json = match fs::read(&flags_path) {
        Ok(b) => b,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(Failure::new(
                "package: manifest",
                "flags.json is missing from the installed package",
            ));
        }
        Err(e) => return Err(read_failure(&flags_path, &e)),
    };
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
        let p = root.join(d);
        fs::create_dir_all(&p).map_err(|e| write_failure(&p, &e))?;
    }

    // 3 to 5.
    for c in &bp.commands {
        let (step, item, code, program) = match &c.step {
            Step::CompileCrt0 => ("compile", "crt0".to_owned(), "build: compile", "gcc"),
            Step::Compile(s) => ("compile", s.clone(), "build: compile", "gcc"),
            Step::Link => ("link", String::new(), "build: link", "gcc"),
            Step::Objcopy => ("objcopy", String::new(), "build: objcopy", "objcopy"),
        };
        out.step(step, &item, "start");
        let output = Command::new(&c.program)
            .args(&c.args)
            .current_dir(&c.cwd)
            .output()
            .map_err(|e| Failure::new(code, format!("{item}: could not start: {}", e.kind())))?;
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
        out.step(step, &item, "done");
    }

    // 6. Seal and validate.
    out.step("seal", "", "start");
    let raw_path = root.join(&bp.raw);
    let raw = fs::read(&raw_path).map_err(|e| read_failure(&raw_path, &e))?;
    let sealed = seal(m, project.icon.as_ref(), &raw)?;
    out.step("seal", "", "done");

    // 7. Write the tape. The ELF is where the link wrote it.
    let tape = root.join(format!("build/{}.HTP", m.project.name));
    let tmp = root.join(format!("build/{}.HTP.tmp", m.project.name));
    fs::write(&tmp, &sealed).map_err(|e| write_failure(&tmp, &e))?;
    if let Err(e) = fs::rename(&tmp, &tape) {
        let _ = fs::remove_file(&tmp);
        return Err(write_failure(&tape, &e));
    }
    Ok(sealed)
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
