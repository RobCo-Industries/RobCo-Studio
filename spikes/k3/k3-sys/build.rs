//! Compiles RobCo-Core's host-buildable C with the `cc` crate.
//!
//! ROBCO_CORE_DIR names a RobCo-Core checkout.

use std::env;
use std::path::PathBuf;

/// The files the sys crate needs, relative to the RobCo-Core root.
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

fn main() {
    println!("cargo:rerun-if-env-changed=ROBCO_CORE_DIR");
    println!("cargo:rerun-if-changed=csrc/layout.c");

    let Some(core) = env::var_os("ROBCO_CORE_DIR").map(PathBuf::from) else {
        eprintln!("k3-sys: set ROBCO_CORE_DIR to a RobCo-Core checkout");
        std::process::exit(1);
    };

    println!("cargo:rustc-env=ROBCO_CORE_DIR_AT_BUILD={}", core.display());
    let mut b = cc::Build::new();
    b.std("gnu11")
        .define("ROBCO_HOST", "1")
        .include(core.join("include"))
        .include(core.join("src/fs"))
        .include(core.join("third_party/fatfs"))
        .warnings(true)
        .extra_warnings(true);
    for f in FILES {
        let p = core.join(f);
        println!("cargo:rerun-if-changed={}", p.display());
        b.file(p);
    }
    b.file("csrc/layout.c");
    let tool = b.get_compiler();
    println!(
        "cargo:warning=k3: C compiler {} (clang-like: {}, gnu-like: {}, msvc-like: {})",
        tool.path().display(),
        tool.is_like_clang(),
        tool.is_like_gnu(),
        tool.is_like_msvc()
    );
    b.compile("robco_core");
}
