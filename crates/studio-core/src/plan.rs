// SPDX-License-Identifier: GPL-3.0-or-later

//! The build plan: the commands that turn a project into `build/NAME.elf`
//! and its raw image.
//!
//! The contract is the "flags.json" section of `docs/spec/SDK_PACKAGE.md` in
//! RobCo-Core, with its parts "Build order", "Startup code", "Path maps" and
//! "Reading flags.json", and the sections "A project folder", "Source order"
//! and "What a holotape build produces" of `docs/spec/STUDIO_PROJECT.md`. The text was read at RobCo-Core
//! commit `e004c73`. [`manifest::expand_sources`] holds the source order.
//!
//! One rule is this module's own: each path field of `flags.json` must be
//! a plain relative path with `/`, checked after the spec's checks.
//!
//! This module is pure. It reads `flags.json` from its bytes, and [`plan`]
//! gives the ordered list of commands of a build. It runs nothing and reads
//! no file: the caller gathers the bytes of `flags.json` and the list of the
//! project's regular files. The host is a parameter, so a test on one
//! system checks the commands of every system.
//!
//! The commands, in order:
//!
//! 1. The compile of the startup code, `crt0.S`.
//! 2. One compile per source, in the order of the source list.
//! 3. The link, to `build/NAME.elf`.
//! 4. `objcopy`, to the raw image that the seal reads.
//!
//! Before the first command the caller creates [`BuildPlan::dirs`], the
//! folders the commands write into. Every output lies under `build/`.
//!
//! Each compile carries the three path maps of the spec, and each map twice
//! on Windows. The seal, step 5 of the spec, is not a command: the caller
//! seals [`BuildPlan::raw`] itself.

use std::fmt;

use serde_json::{Map, Value};

use crate::manifest::{self, Kind, Manifest, ManifestError, Subtype};

/// The highest `flags.json` `format` this module reads.
pub const FORMAT: u64 = 1;

/// The folder, relative to the project root, that holds the objects.
pub const OBJECT_DIR: &str = "build/obj";

/// A checked `flags.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flags {
    /// The `format` of the file.
    pub format: u64,
    /// Flags for each C compile, in order.
    pub cflags: Vec<String>,
    /// Folders for `-I`, relative to the package root.
    pub include_dirs: Vec<String>,
    /// Flags for the link, in order.
    pub ldflags: Vec<String>,
    /// Kind to linker script path, relative to the package root.
    pub linker_scripts: Vec<(String, String)>,
    /// The startup code source, relative to the package root.
    pub crt0: String,
    /// Kind and subtype to the macro definitions of the startup code.
    pub crt0_defines: Vec<(String, Vec<String>)>,
    /// Archive paths relative to the package root, in link order.
    pub libs: Vec<String>,
    /// The compiler's own libraries, last on the link line.
    pub system_libs: Vec<String>,
    /// The macro that carries the project's ABI.
    pub abi_define: String,
}

/// Why `flags.json` was refused. Each variant is one code of the spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlagsError {
    /// Not UTF-8 JSON with an object at the top, or a required field is
    /// missing or has the wrong type. The detail names the field.
    Manifest(String),
    /// `format` is missing, not a positive integer, or too large.
    Format(String),
}

impl FlagsError {
    /// The code, as the spec spells it, such as `manifest`.
    pub fn code(&self) -> &'static str {
        match self {
            FlagsError::Manifest(_) => "manifest",
            FlagsError::Format(_) => "format",
        }
    }

    /// The detail that follows the code.
    pub fn detail(&self) -> &str {
        match self {
            FlagsError::Manifest(d) | FlagsError::Format(d) => d,
        }
    }
}

impl fmt::Display for FlagsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "package: {}: flags.json: {}", self.code(), self.detail())
    }
}

impl std::error::Error for FlagsError {}

/// Reads and checks the bytes of a `flags.json`.
///
/// The checks run in the spec's order: UTF-8 JSON with an object at the
/// top, then `format`, then each field in the order of the spec's table.
/// Each field is checked whole, its type then its value, before the next
/// field is read. So the first bad field of the table is the one reported.
/// A field the spec does not define is ignored, as "Reading flags.json" of
/// SDK_PACKAGE.md says, so no unknown key reaches a command.
///
/// # The boundary
///
/// A package comes from an index. Nothing it holds may make a program of
/// the build read, write or run a file outside the package and the
/// project. Each text that ends up in an argument of `gcc` or `objcopy`:
///
/// | Source | Lands in | Spec allows | Checked | An attacker could put |
/// |---|---|---|---|---|
/// | `cflags` | Steps 1, 2, first | Flags, no path | [`FLAG_SHAPES`], scope `Both` | `-fplugin=`, `-B`, `-specs=` |
/// | `include_dirs` | Steps 1, 2, `-I<PKG/dir>` | Path in the package | [`package_path`], then inside `PKG` | `../..`, `/etc` |
/// | `ldflags` | Step 3, first | Flags, no path | [`FLAG_SHAPES`], `Both` or `Ldflags` | `-Wl,-T,/x`, `-specs=` |
/// | `linker_scripts.<kind>` | Step 3, after `-T` | Path in the package | [`package_path`], then inside `PKG` | `/tmp/evil.ld` |
/// | `crt0` | Step 1, the source | Path in the package | [`package_path`], then inside `PKG` | `-specs=x`, `../x` |
/// | `crt0_defines.<key>` | Step 1, `-D<string>` | `NAME=VALUE` | C name, `=`, value of [`define_value`] | a newline and `#include "/etc/x"` |
/// | `libs` | Step 3, `<PKG/lib>` | Path in the package | [`package_path`], then inside `PKG` | `../../x.a` |
/// | `system_libs` | Step 3, last | Library flags, such as `-lgcc` | [`FLAG_SHAPES`], scope `SystemLibs`: `-l` and `[a-z0-9_+-]` | `-specs=/tmp/x` |
/// | `abi_define` | Steps 1, 2, `-D<name>=<abi>` | A macro name | A C name | `X=1\n#include ...` |
/// | Keys of `linker_scripts` and `crt0_defines` | Nowhere: a lookup key | Kind names | Looked up only | Nothing reaches a command |
/// | Unknown keys of `flags.json` | Nowhere | Ignored | Ignored | Nothing reaches a command |
/// | `robco.toml` `project.name` | Steps 3, 4, `build/NAME.elf` | Name rules | Manifest name rules: `[A-Z0-9_-]`, after `build/` | Nothing outside `build/` |
/// | `robco.toml` `build.sources` | Step 2, the source | A manifest path | `manifest: path`, `manifest: sources`; a name with a leading `-` is passed as `./NAME` | `-specs=x.c` as a file name |
/// | `robco.toml` `target.abi` | Steps 1, 2, `0x` and 8 hex digits | `MAJOR.MINOR` | Parsed to numbers | Nothing: only digits reach the command |
/// | `sdk.json` `name`, `version`; the tool manifest's name, version, host | The absolute `PKG` and toolchain folders, program paths, path maps | Names and versions | The cache folder name checks of `install` | Nothing: the folder is absolute, under the cache |
/// | The project root | `cwd`, a path map | The person's own folder | Not checked: not package text | Nothing: the person chose it |
pub fn parse_flags(bytes: &[u8]) -> Result<Flags, FlagsError> {
    let root: Value = std::str::from_utf8(bytes)
        .ok()
        .and_then(|t| serde_json::from_str(t).ok())
        .ok_or_else(|| FlagsError::Manifest("not UTF-8 JSON".into()))?;
    let Value::Object(root) = root else {
        return Err(FlagsError::Manifest("the top is not an object".into()));
    };
    let format = match root.get("format") {
        None => return Err(FlagsError::Format("missing".into())),
        Some(v) => match v.as_u64() {
            Some(0) => {
                return Err(FlagsError::Format("0 is not a positive integer".into()));
            }
            Some(n) if n > FORMAT => {
                return Err(FlagsError::Format(format!(
                    "{n} is newer than this tool reads, {FORMAT}: install a newer tool"
                )));
            }
            Some(n) => n,
            None => return Err(FlagsError::Format("not a positive integer".into())),
        },
    };
    let cflags = strings(&root, "cflags")?;
    check_flags("cflags", &cflags)?;
    let include_dirs = strings(&root, "include_dirs")?;
    for d in &include_dirs {
        package_path("include_dirs", d)?;
    }
    let ldflags = strings(&root, "ldflags")?;
    check_flags("ldflags", &ldflags)?;
    let linker_scripts = object(&root, "linker_scripts", |key, v| {
        let p = v.as_str().ok_or_else(|| wrong(key, "a string"))?;
        package_path(key, p)?;
        Ok(p.to_string())
    })?;
    let crt0 = string(&root, "crt0")?;
    package_path("crt0", &crt0)?;
    let crt0_defines = object(&root, "crt0_defines", |key, v| {
        let list = array_of_strings(key, v)?;
        for d in &list {
            check_define(key, d)?;
        }
        Ok(list)
    })?;
    let libs = strings(&root, "libs")?;
    for l in &libs {
        package_path("libs", l)?;
    }
    let system_libs = strings(&root, "system_libs")?;
    check_flags("system_libs", &system_libs)?;
    let abi_define = string(&root, "abi_define")?;
    if !c_name(&abi_define) {
        return Err(FlagsError::Manifest(format!(
            "abi_define: {abi_define:?}: not a C name, [A-Za-z_][A-Za-z0-9_]*"
        )));
    }
    Ok(Flags {
        format,
        cflags,
        include_dirs,
        ldflags,
        linker_scripts,
        crt0,
        crt0_defines,
        libs,
        system_libs,
        abi_define,
    })
}

/// Checks a path inside the package: the manifest path rules of
/// [`manifest::check_path`], then no part that starts with `-`, so no
/// file name reads as an option, and no `:`, a Windows stream or drive.
/// [`plan`] checks again, by components, that the joined path stays in
/// the package folder.
fn package_path(key: &str, path: &str) -> Result<(), FlagsError> {
    manifest::check_path(key, path, false).map_err(|e| FlagsError::Manifest(e.detail()))?;
    let fail = |why: &str| Err(FlagsError::Manifest(format!("{key}: {path:?}: {why}")));
    if path.split('/').any(|p| p.starts_with('-')) {
        return fail("a part starts with -");
    }
    if path.chars().any(|c| c == ':' || c.is_control()) {
        return fail("holds a : or a control character");
    }
    Ok(())
}

/// A value of a `crt0_defines` string: `[A-Za-z0-9_.+-]*`, or the same
/// characters and spaces between two `"`.
fn define_value(v: &str) -> bool {
    let plain = |c: char| c.is_ascii_alphanumeric() || "_.+-".contains(c);
    match v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(inner) => inner.chars().all(|c| plain(c) || c == ' '),
        None => v.chars().all(plain),
    }
}

/// Checks one `crt0_defines` string: a C name, `=`, then a [`define_value`].
fn check_define(key: &str, d: &str) -> Result<(), FlagsError> {
    let ok = d
        .split_once('=')
        .is_some_and(|(n, v)| c_name(n) && define_value(v));
    if ok {
        Ok(())
    } else {
        Err(FlagsError::Manifest(format!(
            "{key}: {d:?}: not NAME=VALUE with a C name and a plain or quoted value"
        )))
    }
}

/// Tells whether a whole flag has one shape.
type Shape = fn(&str) -> bool;

/// Where a flag shape is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// In `cflags` and in `ldflags`.
    Both,
    /// In `ldflags` only.
    Ldflags,
    /// In `system_libs` only, and nothing else is allowed there.
    SystemLibs,
}

/// The shapes a flag of `cflags` or `ldflags` may take, each matched
/// against the whole flag. Any other flag is refused.
///
/// SDK_PACKAGE.md says only "No flag in `cflags` or `ldflags` holds a
/// path". This list is stricter on purpose: a package comes from an
/// index, and a flag such as `-fplugin=` or `-B` runs code on the
/// person's machine, so a list of refused flags would always miss one.
/// The spec takes this table at its next update.
const FLAG_SHAPES: &[(&str, Scope, Shape)] = &[
    ("machine", Scope::Both, shape_machine),
    ("feature", Scope::Both, shape_feature),
    ("std", Scope::Both, shape_std),
    ("optimize", Scope::Both, shape_optimize),
    ("debug", Scope::Both, shape_debug),
    ("warning", Scope::Both, shape_warning),
    ("word", Scope::Both, shape_word),
    ("define", Scope::Both, shape_define),
    ("undefine", Scope::Both, shape_undefine),
    ("linker", Scope::Ldflags, shape_linker),
    ("library", Scope::SystemLibs, shape_library),
];

/// The `-f` features a package may ask for: what follows `-f` must be
/// exactly one entry. The project chose this closed list, stricter than
/// SDK_PACKAGE.md: GCC has hundreds of `-f` options and several read or
/// write files, such as `-fopt-info=<file>`, so a list of refused names
/// would always miss one. A new name is added by a change to this table
/// and to the spec.
const FEATURES: &[&str] = &[
    "freestanding",
    "function-sections",
    "data-sections",
    "builtin",
    "no-builtin",
    "common",
    "no-common",
    "omit-frame-pointer",
    "no-omit-frame-pointer",
    "short-enums",
    "no-short-enums",
    "strict-aliasing",
    "no-strict-aliasing",
    "wrapv",
    "signed-char",
    "unsigned-char",
    "exceptions",
    "no-exceptions",
    "unwind-tables",
    "no-unwind-tables",
    "asynchronous-unwind-tables",
    "no-asynchronous-unwind-tables",
    "stack-protector",
    "stack-protector-strong",
    "stack-protector-all",
    "no-stack-protector",
    "pic",
    "PIC",
    "no-pic",
    "pie",
    "PIE",
    "no-pie",
    "merge-constants",
    "no-merge-constants",
    "inline",
    "no-inline",
    "inline-functions",
    "no-inline-functions",
    "toplevel-reorder",
    "no-toplevel-reorder",
    "delete-null-pointer-checks",
    "no-delete-null-pointer-checks",
    "zero-initialized-in-bss",
    "no-zero-initialized-in-bss",
    "ident",
    "no-ident",
    "visibility=hidden",
    "visibility=default",
    // Pip-OS ships it in PIP_CODEFLAGS, sdk/pipos-app.mk.
    "no-tree-loop-distribute-patterns",
];

/// The words a package may pass to the linker with `-Wl,`. A closed list:
/// GNU ld takes the value of an option such as `--output` or `--script`
/// as the next argument, so an open `--word` shape lets `-Wl,--output,--x`
/// write a file named `--x`. GNU ld manual, "Command-line Options",
/// <https://sourceware.org/binutils/docs/ld/Options.html>. Seeded with
/// what Pip-OS ships; a new word is added here and in the spec.
const LINKER_WORDS: &[&str] = &["--gc-sections", "--no-warn-rwx-segments"];

fn lower_digit(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// A first char that passes `first`, then chars that pass `rest`.
fn word(s: &str, first: fn(char) -> bool, rest: fn(char) -> bool) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(first) && c.all(rest)
}

/// `-m`, a lower-case letter or digit, then `[a-z0-9=+._-]*`.
fn shape_machine(f: &str) -> bool {
    f.strip_prefix("-m")
        .is_some_and(|s| word(s, lower_digit, |c| lower_digit(c) || "=+._-".contains(c)))
}

/// `-f` then exactly one entry of [`FEATURES`].
fn shape_feature(f: &str) -> bool {
    f.strip_prefix("-f").is_some_and(|s| FEATURES.contains(&s))
}

/// `-std=` then `[a-z0-9+:]+`.
fn shape_std(f: &str) -> bool {
    f.strip_prefix("-std=").is_some_and(|s| {
        word(
            s,
            |c| lower_digit(c) || "+:".contains(c),
            |c| lower_digit(c) || "+:".contains(c),
        )
    })
}

/// `-O`, alone or with `0`, `1`, `2`, `3`, `s`, `g`, `z` or `fast`.
fn shape_optimize(f: &str) -> bool {
    matches!(
        f,
        "-O" | "-O0" | "-O1" | "-O2" | "-O3" | "-Os" | "-Og" | "-Oz" | "-Ofast"
    )
}

/// `-g` or `-ggdb`, alone or with a digit 0 to 3; `-gdwarf-` with 2 to 5.
fn shape_debug(f: &str) -> bool {
    let level = |s: &str, lo: char, hi: char| {
        s.is_empty() || (s.len() == 1 && s.chars().all(|c| (lo..=hi).contains(&c)))
    };
    if let Some(s) = f.strip_prefix("-gdwarf-") {
        return s.len() == 1 && s.chars().all(|c| ('2'..='5').contains(&c));
    }
    if let Some(s) = f.strip_prefix("-ggdb") {
        return level(s, '0', '3');
    }
    f.strip_prefix("-g").is_some_and(|s| level(s, '0', '3'))
}

/// `-W`, a lower-case letter or digit, then `[a-z0-9=+-]*`; or `-w`,
/// `-pedantic`, `-pedantic-errors`. No comma, so no `-Wl,` here.
fn shape_warning(f: &str) -> bool {
    matches!(f, "-w" | "-pedantic" | "-pedantic-errors")
        || f.strip_prefix("-W")
            .is_some_and(|s| word(s, lower_digit, |c| lower_digit(c) || "=+-".contains(c)))
}

/// Exact words.
fn shape_word(f: &str) -> bool {
    matches!(
        f,
        "-nostdlib" | "-nostartfiles" | "-nodefaultlibs" | "-nostdinc" | "-nolibc" | "-static"
    )
}

fn c_name(s: &str) -> bool {
    word(
        s,
        |c| c.is_ascii_alphabetic() || c == '_',
        |c| c.is_ascii_alphanumeric() || c == '_',
    )
}

/// `-D` then a C name, optionally `=` and `[A-Za-z0-9_.+-]*`.
fn shape_define(f: &str) -> bool {
    f.strip_prefix("-D")
        .is_some_and(|s| match s.split_once('=') {
            None => c_name(s),
            Some((n, v)) => {
                c_name(n)
                    && v.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_.+-".contains(c))
            }
        })
}

/// `-U` then a C name.
fn shape_undefine(f: &str) -> bool {
    f.strip_prefix("-U").is_some_and(c_name)
}

/// `-Wl,` then parts joined by commas, each an entry of [`LINKER_WORDS`].
fn shape_linker(f: &str) -> bool {
    f.strip_prefix("-Wl,")
        .is_some_and(|s| s.split(',').all(|p| LINKER_WORDS.contains(&p)))
}

/// `-l` then a library name, `[a-z0-9][a-z0-9_+-]*`, such as `-lgcc`.
fn shape_library(f: &str) -> bool {
    f.strip_prefix("-l")
        .is_some_and(|s| word(s, lower_digit, |c| lower_digit(c) || "_+-".contains(c)))
}

/// Refuses a flag that matches no shape of [`FLAG_SHAPES`] allowed in
/// `key`: `cflags`, `ldflags` or `system_libs`.
fn check_flags(key: &str, list: &[String]) -> Result<(), FlagsError> {
    let allowed = |scope: Scope| match key {
        "cflags" => scope == Scope::Both,
        "ldflags" => matches!(scope, Scope::Both | Scope::Ldflags),
        _ => scope == Scope::SystemLibs,
    };
    for f in list {
        let ok = FLAG_SHAPES
            .iter()
            .any(|(_, scope, m)| allowed(*scope) && m(f));
        if !ok {
            return Err(FlagsError::Manifest(format!(
                "{key}: {f:?}: not in the allowed flag shapes"
            )));
        }
    }
    Ok(())
}

fn wrong(key: &str, expected: &str) -> FlagsError {
    FlagsError::Manifest(format!("{key}: expected {expected}"))
}

fn field<'a>(root: &'a Map<String, Value>, key: &str) -> Result<&'a Value, FlagsError> {
    root.get(key)
        .ok_or_else(|| FlagsError::Manifest(format!("{key}: missing")))
}

fn string(root: &Map<String, Value>, key: &str) -> Result<String, FlagsError> {
    field(root, key)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| wrong(key, "a string"))
}

fn array_of_strings(key: &str, v: &Value) -> Result<Vec<String>, FlagsError> {
    let bad = || wrong(key, "an array of strings");
    v.as_array()
        .ok_or_else(bad)?
        .iter()
        .map(|s| s.as_str().map(str::to_string).ok_or_else(bad))
        .collect()
}

fn strings(root: &Map<String, Value>, key: &str) -> Result<Vec<String>, FlagsError> {
    array_of_strings(key, field(root, key)?)
}

/// Reads an object field. Its keys come sorted; the plan looks them up.
fn object<T>(
    root: &Map<String, Value>,
    key: &str,
    item: impl Fn(&str, &Value) -> Result<T, FlagsError>,
) -> Result<Vec<(String, T)>, FlagsError> {
    let map = field(root, key)?
        .as_object()
        .ok_or_else(|| wrong(key, "an object"))?;
    map.iter()
        .map(|(k, v)| Ok((k.clone(), item(&format!("{key}.{k}"), v)?)))
        .collect()
}

/// The system the commands run on. It decides the path separator, the
/// program suffix and how many path maps a compile carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    /// Linux or macOS: `/`, no suffix, one spelling of each map.
    Unix,
    /// Windows: `\`, `.exe`, each map with `\` then with `/`.
    Windows,
}

impl Host {
    /// The system this program runs on.
    pub fn current() -> Host {
        if cfg!(windows) {
            Host::Windows
        } else {
            Host::Unix
        }
    }

    fn sep(self) -> char {
        match self {
            Host::Unix => '/',
            Host::Windows => '\\',
        }
    }

    /// Joins a `/`-separated relative path to an absolute folder, with the
    /// host's separator.
    fn join(self, root: &str, rel: &str) -> String {
        let sep = self.sep();
        let rel: String = rel
            .chars()
            .map(|c| if c == '/' { sep } else { c })
            .collect();
        if root.ends_with(sep) || (self == Host::Windows && root.ends_with('/')) {
            format!("{root}{rel}")
        } else {
            format!("{root}{sep}{rel}")
        }
    }

    /// A program of the toolchain's `bin/`.
    fn program(self, toolchain: &str, name: &str) -> String {
        match self {
            Host::Unix => self.join(toolchain, &format!("bin/{name}")),
            Host::Windows => self.join(toolchain, &format!("bin/{name}.exe")),
        }
    }

    /// The `-ffile-prefix-map` flags of one root.
    fn maps(self, root: &str, to: &str, out: &mut Vec<String>) {
        match self {
            Host::Unix => out.push(format!("-ffile-prefix-map={root}={to}")),
            Host::Windows => {
                let back = root.replace('/', "\\");
                let fwd = root.replace('\\', "/");
                out.push(format!("-ffile-prefix-map={back}={to}"));
                out.push(format!("-ffile-prefix-map={fwd}={to}"));
            }
        }
    }
}

/// What a command does, for the `step` lines and the error of a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Compiles the startup code. Its step item is `crt0`.
    CompileCrt0,
    /// Compiles one source. Holds its path, relative to the project root.
    Compile(String),
    /// Links `build/NAME.elf`.
    Link,
    /// Writes the raw image.
    Objcopy,
}

/// One program to start, with no shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// What the command does.
    pub step: Step,
    /// The absolute path of the program.
    pub program: String,
    /// The arguments, each passed as it is.
    pub args: Vec<String>,
    /// The folder the program runs in: the project root.
    pub cwd: String,
}

/// The commands of a build and the files they write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    /// The source list, in link order, relative to the project root.
    pub sources: Vec<String>,
    /// The folders the commands write into, relative to the project root,
    /// with `/`. Each parent comes before its children and each folder
    /// comes once. The caller creates them before the first command.
    pub dirs: Vec<String>,
    /// The commands, in the order they run.
    pub commands: Vec<Command>,
    /// The ELF the link writes, relative to the project root.
    pub elf: String,
    /// The raw image `objcopy` writes, relative to the project root.
    pub raw: String,
    /// Every file the commands write, relative to the project root, with
    /// `/`, in the order the commands run: the crt0 object, each source's
    /// object, the ELF and the raw image. A caller that guards the outputs
    /// reads them here, never from the arguments.
    pub outputs: Vec<String>,
}

/// What a plan needs.
#[derive(Debug, Clone, Copy)]
pub struct PlanInput<'a> {
    /// The checked `robco.toml`.
    pub manifest: &'a Manifest,
    /// The bytes of the installed package's `flags.json`.
    pub flags_json: &'a [u8],
    /// The absolute path of the installed package folder.
    pub package: &'a str,
    /// The absolute path of the folder that holds the gcc archive's `bin/`.
    pub toolchain: &'a str,
    /// The absolute path of the folder that holds `robco.toml`.
    pub project: &'a str,
    /// Every regular file of the project, relative, with `/`.
    pub files: &'a [&'a str],
    /// The system the commands run on.
    pub host: Host,
}

/// Why no plan could be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// `flags.json` was refused.
    Flags(FlagsError),
    /// The source patterns were refused.
    Manifest(ManifestError),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::Flags(e) => e.fmt(f),
            PlanError::Manifest(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for PlanError {}

impl From<FlagsError> for PlanError {
    fn from(e: FlagsError) -> Self {
        PlanError::Flags(e)
    }
}

impl From<ManifestError> for PlanError {
    fn from(e: ManifestError) -> Self {
        PlanError::Manifest(e)
    }
}

/// The key of `crt0_defines` for a project, such as `holotape-game`.
fn defines_key(kind: Kind, subtype: Subtype) -> &'static str {
    match (kind, subtype) {
        (Kind::Holotape, Subtype::Game) => "holotape-game",
        (Kind::Holotape, Subtype::Utility) => "holotape-utility",
    }
}

/// The key of `linker_scripts` for a kind.
fn script_key(kind: Kind) -> &'static str {
    match kind {
        Kind::Holotape => "holotape",
    }
}

fn lookup<'a, T>(map: &'a [(String, T)], field: &str, key: &str) -> Result<&'a T, FlagsError> {
    map.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
        .ok_or_else(|| FlagsError::Manifest(format!("{field}.{key}: missing")))
}

/// Builds the ordered commands of a build. Runs nothing, reads no file.
pub fn plan(input: &PlanInput<'_>) -> Result<BuildPlan, PlanError> {
    let flags = parse_flags(input.flags_json)?;
    let m = input.manifest;
    let host = input.host;
    let sources = manifest::expand_sources(&m.build.sources, input.files)?;

    let defines = lookup(
        &flags.crt0_defines,
        "crt0_defines",
        defines_key(m.project.kind, m.project.subtype),
    )?;
    let script = lookup(
        &flags.linker_scripts,
        "linker_scripts",
        script_key(m.project.kind),
    )?;

    let gcc = host.program(input.toolchain, "arm-none-eabi-gcc");
    let objcopy = host.program(input.toolchain, "arm-none-eabi-objcopy");
    let abi = format!(
        "-D{}=0x{:08x}",
        flags.abi_define,
        m.target.abi.abi_version()
    );
    // Every package path, joined, must stay in the package folder.
    let pkg = |key: &str, rel: &str| -> Result<String, FlagsError> {
        let joined = host.join(input.package, rel);
        if inside(host, input.package, &joined) {
            Ok(joined)
        } else {
            Err(FlagsError::Manifest(format!(
                "{key}: {rel:?}: leads outside the package folder"
            )))
        }
    };
    let includes = flags
        .include_dirs
        .iter()
        .map(|d| pkg("include_dirs", d).map(|p| format!("-I{p}")))
        .collect::<Result<Vec<String>, _>>()?;
    let script_path = pkg(
        &format!("linker_scripts.{}", script_key(m.project.kind)),
        script,
    )?;
    let crt0_path = pkg("crt0", &flags.crt0)?;
    let lib_paths = flags
        .libs
        .iter()
        .map(|l| pkg("libs", l))
        .collect::<Result<Vec<String>, _>>()?;
    let mut maps = Vec::new();
    host.maps(input.project, ".", &mut maps);
    host.maps(input.toolchain, "toolchain", &mut maps);
    host.maps(input.package, "sdk", &mut maps);

    let compile = |step: Step, extra: &[String], source: String, object: &str| {
        let mut args = flags.cflags.clone();
        args.push(abi.clone());
        args.extend_from_slice(extra);
        args.extend(includes.iter().cloned());
        args.extend(maps.iter().cloned());
        args.extend(["-c".into(), source, "-o".into(), object.to_string()]);
        Command {
            step,
            program: gcc.clone(),
            args,
            cwd: input.project.to_string(),
        }
    };

    let crt0_object = format!("{OBJECT_DIR}/crt0.o");
    let crt0_defines: Vec<String> = defines.iter().map(|d| format!("-D{d}")).collect();
    let mut commands = vec![compile(
        Step::CompileCrt0,
        &crt0_defines,
        crt0_path,
        &crt0_object,
    )];
    let mut objects = Vec::with_capacity(sources.len());
    for source in &sources {
        let object = format!("{OBJECT_DIR}/{source}.o");
        // A file name that starts with `-` would read as an option.
        let arg = if source.starts_with('-') {
            format!("./{source}")
        } else {
            source.clone()
        };
        commands.push(compile(Step::Compile(source.clone()), &[], arg, &object));
        objects.push(object);
    }

    let name = &m.project.name;
    let elf = format!("build/{name}.elf");
    let raw = format!("build/{name}.bin");
    let mut args = flags.ldflags.clone();
    args.extend(["-T".into(), script_path]);
    args.push(crt0_object.clone());
    args.extend(objects.iter().cloned());
    args.extend(lib_paths);
    args.extend(flags.system_libs.iter().cloned());
    args.extend(["-o".into(), elf.clone()]);
    commands.push(Command {
        step: Step::Link,
        program: gcc,
        args,
        cwd: input.project.to_string(),
    });
    commands.push(Command {
        step: Step::Objcopy,
        program: objcopy,
        args: vec!["-O".into(), "binary".into(), elf.clone(), raw.clone()],
        cwd: input.project.to_string(),
    });

    let dirs = dirs_of(
        std::iter::once(crt0_object.as_str())
            .chain(objects.iter().map(String::as_str))
            .chain([elf.as_str(), raw.as_str()]),
    );

    let outputs = std::iter::once(crt0_object)
        .chain(objects)
        .chain([elf.clone(), raw.clone()])
        .collect();

    Ok(BuildPlan {
        sources,
        dirs,
        commands,
        elf,
        raw,
        outputs,
    })
}

/// True when `joined` lies strictly inside `root`, by components, with
/// `.` dropped and `..` taking one component off. Both use the host's
/// separators.
fn inside(host: Host, root: &str, joined: &str) -> bool {
    let split = |s: &str| -> Option<Vec<String>> {
        let mut out: Vec<String> = Vec::new();
        let seps: &[char] = match host {
            Host::Unix => &['/'],
            Host::Windows => &['/', '\\'],
        };
        for part in s.split(seps) {
            match part {
                "" | "." => {}
                ".." => {
                    out.pop()?;
                }
                p => out.push(p.to_string()),
            }
        }
        Some(out)
    };
    match (split(root), split(joined)) {
        (Some(r), Some(j)) => j.len() > r.len() && j.starts_with(&r),
        _ => false,
    }
}

/// The folders that hold the given files and all their parents, sorted.
/// A sorted list puts each parent before its children, since a parent is a
/// prefix of its child.
fn dirs_of<'a>(files: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut dirs = std::collections::BTreeSet::new();
    for file in files {
        let mut at = 0;
        while let Some(i) = file[at..].find('/') {
            at += i;
            dirs.insert(file[..at].to_string());
            at += 1;
        }
    }
    dirs.into_iter().collect()
}

#[cfg(test)]
mod tests;
