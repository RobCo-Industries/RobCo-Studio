// SPDX-License-Identifier: GPL-3.0-or-later

//! Compiles a fixed list of RobCo-Core's C files with the `cc` crate.
//!
//! RobCo-Core is the submodule at `external/robco-core`. The flags are the
//! ones of the library's own host tests: `-std=gnu11 -DROBCO_HOST=1`,
//! `-Wall -Wextra`, warnings as errors.
//!
//! On Windows with the MSVC target the C needs the GNU-style `clang`
//! driver. MSVC rejects `__attribute__`, and `clang-cl` drops `-std=gnu11`
//! without an error. The `cc` crate would pick MSVC, so this script finds
//! `clang.exe` itself.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The C files, relative to the RobCo-Core root.
const FILES: &[&str] = &[
    "src/lib/rimg.c",
    "src/lib/crc32.c",
    "src/lib/ini.c",
    "src/lib/part.c",
    "src/lib/blk_ram.c",
    "src/lib/k_string.c",
    "src/lib/datetime.c",
    "src/fs/fs.c",
    "src/fs/fs_glue.c",
    "third_party/fatfs/ff.c",
    "third_party/fatfs/ffunicode.c",
];

/// The include paths, relative to the RobCo-Core root.
const INCLUDES: &[&str] = &["include", "src/fs", "third_party/fatfs"];

/// The variables through which the `cc` crate takes a C compiler.
const CC_VARS: &[&str] = &["CC", "TARGET_CC", "HOST_CC"];

const NO_CLANG: &str = "studio-sys: no GNU-style clang found. On Windows the C needs \
the `clang` driver, not MSVC and not clang-cl. Install the Visual Studio component \
\"C++ Clang Compiler for Windows\" (Microsoft.VisualStudio.Component.VC.Llvm.Clang), \
or LLVM from https://github.com/llvm/llvm-project/releases, then build again.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let manifest = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or("CARGO_MANIFEST_DIR is not set")?;
    let core = manifest.join("../../external/robco-core");
    if !core.join("include/robco/rimg.h").is_file() {
        return Err(format!(
            "studio-sys: RobCo-Core is missing at {}. Run `git submodule update --init` \
             in the repository root.",
            core.display()
        ));
    }

    println!("cargo:rerun-if-changed=csrc/layout.c");
    for dir in INCLUDES {
        println!("cargo:rerun-if-changed={}", core.join(dir).display());
    }
    let target = env::var("TARGET").unwrap_or_default();
    for v in CC_VARS {
        println!("cargo:rerun-if-env-changed={v}");
    }
    println!("cargo:rerun-if-env-changed=CC_{target}");
    println!("cargo:rerun-if-env-changed=CC_{}", target.replace('-', "_"));

    let mut b = cc::Build::new();
    b.std("gnu11")
        .define("ROBCO_HOST", "1")
        .warnings(true)
        .extra_warnings(true)
        .warnings_into_errors(true);
    for dir in INCLUDES {
        b.include(core.join(dir));
    }
    for f in FILES {
        let p = core.join(f);
        println!("cargo:rerun-if-changed={}", p.display());
        b.file(p);
    }
    b.file("csrc/layout.c");

    let msvc_target = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|e| e == "msvc");
    if msvc_target && !compiler_set_by_user(&target) {
        let clang = find_windows_clang().ok_or(NO_CLANG)?;
        eprintln!("studio-sys: C compiler {}", clang.display());
        b.compiler(clang);
    }

    let tool = b.try_get_compiler().map_err(|e| e.to_string())?;
    if tool.is_like_msvc() {
        return Err(format!(
            "studio-sys: {} is MSVC-style. MSVC rejects __attribute__ and clang-cl \
             ignores -std=gnu11. Use the GNU-style clang driver.\n{NO_CLANG}",
            tool.path().display()
        ));
    }
    b.try_compile("robco_core").map_err(|e| e.to_string())
}

/// True when a variable the `cc` crate reads names a compiler.
fn compiler_set_by_user(target: &str) -> bool {
    let named = [
        format!("CC_{target}"),
        format!("CC_{}", target.replace('-', "_")),
    ];
    CC_VARS
        .iter()
        .map(|s| s.to_string())
        .chain(named)
        .any(|v| env::var_os(v).is_some_and(|s| !s.is_empty()))
}

/// Finds the GNU-style `clang.exe`: first the Visual Studio clang
/// component, then `clang.exe` on `PATH`, then LLVM's default folder.
fn find_windows_clang() -> Option<PathBuf> {
    vs_clang().or_else(path_clang).or_else(|| {
        let pf = env::var_os("ProgramFiles")?;
        existing(Path::new(&pf).join("LLVM/bin/clang.exe"))
    })
}

fn vs_clang() -> Option<PathBuf> {
    let pf86 = env::var_os("ProgramFiles(x86)")?;
    let vswhere = Path::new(&pf86).join("Microsoft Visual Studio/Installer/vswhere.exe");
    let out = Command::new(vswhere)
        .args(["-latest", "-products", "*", "-requires"])
        .arg("Microsoft.VisualStudio.Component.VC.Llvm.Clang")
        .args(["-property", "installationPath", "-utf8"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let root = PathBuf::from(text.lines().next()?.trim());
    let arch = if env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "aarch64") {
        "ARM64"
    } else {
        "x64"
    };
    existing(root.join("VC/Tools/Llvm").join(arch).join("bin/clang.exe"))
}

fn path_clang() -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path).find_map(|d| existing(d.join("clang.exe")))
}

fn existing(p: PathBuf) -> Option<PathBuf> {
    p.is_file().then_some(p)
}
