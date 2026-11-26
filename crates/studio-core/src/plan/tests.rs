// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// The spec's example `flags.json`, copied byte for byte.
const FLAGS: &[u8] = include_bytes!("../../tests/plan/flags.json");

const MANIFEST: &str = "format = 1

[project]
name = \"PIPES\"
version = \"1.0.0\"
kind = \"holotape\"
subtype = \"game\"

[target]
platform = \"pipos\"
abi = \"1.3\"

[build]
sources = [\"src/main.c\", \"src/*.c\"]
";

const FILES: &[&str] = &[
    "robco.toml",
    "src/game.c",
    "src/main.c",
    "src/draw.c",
    "src/notes.h.txt",
];

const CFLAGS: &[&str] = &[
    "-mcpu=arm1176jzf-s",
    "-marm",
    "-mfloat-abi=soft",
    "-mgeneral-regs-only",
    "-mno-unaligned-access",
    "-std=gnu11",
    "-ffreestanding",
    "-nostdlib",
    "-ffunction-sections",
    "-fdata-sections",
    "-fno-tree-loop-distribute-patterns",
    "-O2",
    "-g",
    "-Wall",
    "-Wextra",
    "-DPIPOS_ABI_TARGET=0x00010003",
];

const LDFLAGS: &[&str] = &[
    "-mcpu=arm1176jzf-s",
    "-marm",
    "-mfloat-abi=soft",
    "-nostdlib",
    "-Wl,--gc-sections",
];

fn manifest(text: &str) -> Manifest {
    match manifest::parse(text.as_bytes()) {
        Ok(m) => m,
        Err(e) => panic!("{e}"),
    }
}

fn make(m: &Manifest, flags: &[u8], host: Host) -> Result<BuildPlan, PlanError> {
    make_with(m, flags, host, FILES)
}

fn make_with(
    m: &Manifest,
    flags: &[u8],
    host: Host,
    files: &[&str],
) -> Result<BuildPlan, PlanError> {
    let (package, toolchain, project) = match host {
        Host::Unix => ("/c/pkg/pipos/1.4.0", "/c/tools/gcc", "/w/my proj"),
        Host::Windows => (
            "C:\\c\\pkg\\pipos\\1.4.0",
            "C:\\c\\tools\\gcc",
            "D:\\w\\my proj",
        ),
    };
    plan(&PlanInput {
        manifest: m,
        flags_json: flags,
        package,
        toolchain,
        project,
        files,
        host,
    })
}

fn ok(host: Host) -> BuildPlan {
    match make(&manifest(MANIFEST), FLAGS, host) {
        Ok(p) => p,
        Err(e) => panic!("{e}"),
    }
}

fn v(parts: &[&[&str]]) -> Vec<String> {
    parts
        .iter()
        .flat_map(|p| p.iter())
        .map(|s| s.to_string())
        .collect()
}

fn compile(step: Step, gcc: &str, cwd: &str, mid: &[&str], tail: &[&str]) -> Command {
    Command {
        step,
        program: gcc.into(),
        args: v(&[CFLAGS, mid, tail]),
        cwd: cwd.into(),
    }
}

#[test]
fn plan_unix_matches_fixture() {
    let gcc = "/c/tools/gcc/bin/arm-none-eabi-gcc";
    let cwd = "/w/my proj";
    let inc: &[&str] = &[
        "-I/c/pkg/pipos/1.4.0/include",
        "-ffile-prefix-map=/w/my proj=.",
        "-ffile-prefix-map=/c/tools/gcc=toolchain",
        "-ffile-prefix-map=/c/pkg/pipos/1.4.0=sdk",
    ];
    let crt0_mid = v(&[&["-DPIP_IMG_MAGIC=\"ROBCO78\"", "-DPIP_IMG_SUBTYPE=0"], inc]);
    let crt0_mid: Vec<&str> = crt0_mid.iter().map(String::as_str).collect();
    let want = vec![
        compile(
            Step::CompileCrt0,
            gcc,
            cwd,
            &crt0_mid,
            &[
                "-c",
                "/c/pkg/pipos/1.4.0/lib/crt0.S",
                "-o",
                "build/obj/crt0.o",
            ],
        ),
        compile(
            Step::Compile("src/main.c".into()),
            gcc,
            cwd,
            inc,
            &["-c", "src/main.c", "-o", "build/obj/src/main.c.o"],
        ),
        compile(
            Step::Compile("src/draw.c".into()),
            gcc,
            cwd,
            inc,
            &["-c", "src/draw.c", "-o", "build/obj/src/draw.c.o"],
        ),
        compile(
            Step::Compile("src/game.c".into()),
            gcc,
            cwd,
            inc,
            &["-c", "src/game.c", "-o", "build/obj/src/game.c.o"],
        ),
        Command {
            step: Step::Link,
            program: gcc.into(),
            args: v(&[
                LDFLAGS,
                &[
                    "-T",
                    "/c/pkg/pipos/1.4.0/lib/holotape.ld",
                    "build/obj/crt0.o",
                    "build/obj/src/main.c.o",
                    "build/obj/src/draw.c.o",
                    "build/obj/src/game.c.o",
                    "/c/pkg/pipos/1.4.0/lib/libpip.a",
                    "-lgcc",
                    "-o",
                    "build/PIPES.elf",
                ],
            ]),
            cwd: cwd.into(),
        },
        Command {
            step: Step::Objcopy,
            program: "/c/tools/gcc/bin/arm-none-eabi-objcopy".into(),
            args: v(&[&["-O", "binary", "build/PIPES.elf", "build/PIPES.bin"]]),
            cwd: cwd.into(),
        },
    ];
    let p = ok(Host::Unix);
    assert_eq!(p.sources, ["src/main.c", "src/draw.c", "src/game.c"]);
    assert_eq!(p.elf, "build/PIPES.elf");
    assert_eq!(p.raw, "build/PIPES.bin");
    assert_eq!(p.dirs, ["build", "build/obj", "build/obj/src"]);
    assert_eq!(
        p.outputs,
        [
            "build/obj/crt0.o",
            "build/obj/src/main.c.o",
            "build/obj/src/draw.c.o",
            "build/obj/src/game.c.o",
            "build/PIPES.elf",
            "build/PIPES.bin",
        ]
    );
    assert_eq!(p.commands, want);
}

#[test]
fn plan_windows_matches_fixture() {
    let gcc = "C:\\c\\tools\\gcc\\bin\\arm-none-eabi-gcc.exe";
    let cwd = "D:\\w\\my proj";
    let inc: &[&str] = &[
        "-IC:\\c\\pkg\\pipos\\1.4.0\\include",
        "-ffile-prefix-map=D:\\w\\my proj=.",
        "-ffile-prefix-map=D:/w/my proj=.",
        "-ffile-prefix-map=C:\\c\\tools\\gcc=toolchain",
        "-ffile-prefix-map=C:/c/tools/gcc=toolchain",
        "-ffile-prefix-map=C:\\c\\pkg\\pipos\\1.4.0=sdk",
        "-ffile-prefix-map=C:/c/pkg/pipos/1.4.0=sdk",
    ];
    let crt0_mid = v(&[&["-DPIP_IMG_MAGIC=\"ROBCO78\"", "-DPIP_IMG_SUBTYPE=0"], inc]);
    let crt0_mid: Vec<&str> = crt0_mid.iter().map(String::as_str).collect();
    let want = vec![
        compile(
            Step::CompileCrt0,
            gcc,
            cwd,
            &crt0_mid,
            &[
                "-c",
                "C:\\c\\pkg\\pipos\\1.4.0\\lib\\crt0.S",
                "-o",
                "build/obj/crt0.o",
            ],
        ),
        compile(
            Step::Compile("src/main.c".into()),
            gcc,
            cwd,
            inc,
            &["-c", "src/main.c", "-o", "build/obj/src/main.c.o"],
        ),
        compile(
            Step::Compile("src/draw.c".into()),
            gcc,
            cwd,
            inc,
            &["-c", "src/draw.c", "-o", "build/obj/src/draw.c.o"],
        ),
        compile(
            Step::Compile("src/game.c".into()),
            gcc,
            cwd,
            inc,
            &["-c", "src/game.c", "-o", "build/obj/src/game.c.o"],
        ),
        Command {
            step: Step::Link,
            program: gcc.into(),
            args: v(&[
                LDFLAGS,
                &[
                    "-T",
                    "C:\\c\\pkg\\pipos\\1.4.0\\lib\\holotape.ld",
                    "build/obj/crt0.o",
                    "build/obj/src/main.c.o",
                    "build/obj/src/draw.c.o",
                    "build/obj/src/game.c.o",
                    "C:\\c\\pkg\\pipos\\1.4.0\\lib\\libpip.a",
                    "-lgcc",
                    "-o",
                    "build/PIPES.elf",
                ],
            ]),
            cwd: cwd.into(),
        },
        Command {
            step: Step::Objcopy,
            program: "C:\\c\\tools\\gcc\\bin\\arm-none-eabi-objcopy.exe".into(),
            args: v(&[&["-O", "binary", "build/PIPES.elf", "build/PIPES.bin"]]),
            cwd: cwd.into(),
        },
    ];
    let p = ok(Host::Windows);
    assert_eq!(p.dirs, ["build", "build/obj", "build/obj/src"]);
    assert_eq!(p.commands, want);
}

#[test]
fn plan_maps_only_on_compiles() {
    for host in [Host::Unix, Host::Windows] {
        let p = ok(host);
        let maps = if host == Host::Windows { 6 } else { 3 };
        for c in &p.commands {
            let n = c
                .args
                .iter()
                .filter(|a| a.starts_with("-ffile-prefix-map="))
                .count();
            match c.step {
                Step::CompileCrt0 | Step::Compile(_) => assert_eq!(n, maps, "{c:?}"),
                Step::Link | Step::Objcopy => assert_eq!(n, 0, "{c:?}"),
            }
            assert!(!c.args.iter().any(|a| a == "-fno-ident"));
        }
    }
}

#[test]
fn plan_utility_takes_its_defines() {
    let m = manifest(&MANIFEST.replace("\"game\"", "\"utility\""));
    let p = match make(&m, FLAGS, Host::Unix) {
        Ok(p) => p,
        Err(e) => panic!("{e}"),
    };
    let args = &p.commands[0].args;
    assert!(args.iter().any(|a| a == "-DPIP_IMG_SUBTYPE=1"));
    assert!(!args.iter().any(|a| a == "-DPIP_IMG_SUBTYPE=0"));
}

#[test]
fn plan_abi_is_eight_hex_digits() {
    let m = manifest(&MANIFEST.replace("\"1.3\"", "\"1.10\""));
    let p = match make(&m, FLAGS, Host::Unix) {
        Ok(p) => p,
        Err(e) => panic!("{e}"),
    };
    for c in &p.commands[..4] {
        assert!(c.args.iter().any(|a| a == "-DPIPOS_ABI_TARGET=0x0001000a"));
    }
}

#[test]
fn plan_written_order_wins() {
    let m = manifest(&MANIFEST.replace(
        "[\"src/main.c\", \"src/*.c\"]",
        "[\"src/main.c\", \"src/game.c\", \"src/draw.c\"]",
    ));
    let p = match make(&m, FLAGS, Host::Unix) {
        Ok(p) => p,
        Err(e) => panic!("{e}"),
    };
    assert_eq!(p.sources, ["src/main.c", "src/game.c", "src/draw.c"]);
}

#[test]
fn plan_source_errors_pass_through() {
    let m = manifest(&MANIFEST.replace("\"src/*.c\"", "\"lib/*.c\""));
    match make(&m, FLAGS, Host::Unix) {
        Err(PlanError::Manifest(e)) => assert_eq!(e.code(), "sources"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn plan_missing_kind_entry_is_package_manifest() {
    let text = String::from_utf8_lossy(FLAGS).replace("\"holotape-game\"", "\"other\"");
    match make(&manifest(MANIFEST), text.as_bytes(), Host::Unix) {
        Err(PlanError::Flags(e)) => {
            assert_eq!(
                e.to_string(),
                "package: manifest: flags.json: crt0_defines.holotape-game: missing"
            );
        }
        other => panic!("{other:?}"),
    }
}

fn flags_err(text: &str) -> FlagsError {
    match parse_flags(text.as_bytes()) {
        Ok(f) => panic!("parsed: {f:?}"),
        Err(e) => e,
    }
}

#[test]
fn plan_flags_example_parses() {
    let f = match parse_flags(FLAGS) {
        Ok(f) => f,
        Err(e) => panic!("{e}"),
    };
    assert_eq!(f.format, 1);
    assert_eq!(f.cflags.len(), 15);
    assert_eq!(f.include_dirs, ["include"]);
    assert_eq!(f.crt0, "lib/crt0.S");
    assert_eq!(f.libs, ["lib/libpip.a"]);
    assert_eq!(f.system_libs, ["-lgcc"]);
    assert_eq!(f.abi_define, "PIPOS_ABI_TARGET");
    assert_eq!(f.crt0_defines.len(), 3);
    assert_eq!(f.linker_scripts.len(), 2);
}

#[test]
fn plan_flags_syntax_and_top() {
    assert_eq!(flags_err("{").code(), "manifest");
    assert_eq!(flags_err("[1]").code(), "manifest");
    let e = match parse_flags(&[0xff, b'{', b'}']) {
        Err(e) => e,
        Ok(f) => panic!("{f:?}"),
    };
    assert_eq!(e.code(), "manifest");
}

#[test]
fn plan_flags_format() {
    for text in [
        "{}",
        "{\"format\": 0}",
        "{\"format\": -1}",
        "{\"format\": 1.5}",
        "{\"format\": \"1\"}",
        "{\"format\": 2}",
    ] {
        assert_eq!(flags_err(text).code(), "format", "{text}");
    }
}

#[test]
fn plan_flags_format_detail() {
    // Only a larger format asks for a newer tool: it is the one case a
    // newer tool can read.
    assert_eq!(
        flags_err("{\"format\": 2}").detail(),
        "2 is newer than this tool reads, 1: install a newer tool"
    );
    for text in [
        "{}",
        "{\"format\": 0}",
        "{\"format\": -1}",
        "{\"format\": 1.5}",
        "{\"format\": \"1\"}",
        "{\"format\": null}",
        "{\"format\": {}}",
    ] {
        assert!(!flags_err(text).detail().contains("newer"), "{text}");
    }
}

#[test]
fn plan_flags_format_before_fields() {
    // A later format with fields this tool does not know reports format.
    assert_eq!(flags_err("{\"format\": 2, \"cflags\": 3}").code(), "format");
}

#[test]
fn plan_flags_field_order_and_types() {
    let base: Value = match serde_json::from_slice(FLAGS) {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    };
    let fields = [
        "cflags",
        "include_dirs",
        "ldflags",
        "linker_scripts",
        "crt0",
        "crt0_defines",
        "libs",
        "system_libs",
        "abi_define",
    ];
    // Removing every field reports the first of the table.
    let mut all = base.clone();
    if let Value::Object(m) = &mut all {
        for f in fields {
            m.remove(f);
        }
    }
    assert_eq!(flags_err(&all.to_string()).detail(), "cflags: missing");
    for f in fields {
        let mut gone = base.clone();
        let mut bad = base.clone();
        if let (Value::Object(g), Value::Object(b)) = (&mut gone, &mut bad) {
            g.remove(f);
            b.insert(f.into(), Value::Bool(true));
        }
        let e = flags_err(&gone.to_string());
        assert_eq!(e.code(), "manifest");
        assert_eq!(e.detail(), format!("{f}: missing"));
        let e = flags_err(&bad.to_string());
        assert_eq!(e.code(), "manifest");
        assert!(e.detail().starts_with(&format!("{f}: expected")), "{e}");
    }
}

#[test]
fn plan_flags_inner_types() {
    let text = String::from_utf8_lossy(FLAGS).replace("\"-lgcc\"", "7");
    assert_eq!(
        flags_err(&text).detail(),
        "system_libs: expected an array of strings"
    );
    let text = String::from_utf8_lossy(FLAGS).replace("\"lib/rom.ld\"", "[]");
    assert_eq!(
        flags_err(&text).detail(),
        "linker_scripts.rom: expected a string"
    );
    let text = String::from_utf8_lossy(FLAGS).replace("\"PIP_IMG_SUBTYPE=1\"", "1");
    assert_eq!(
        flags_err(&text).detail(),
        "crt0_defines.holotape-utility: expected an array of strings"
    );
}

#[test]
fn plan_flags_unknown_field_ignored() {
    let text = String::from_utf8_lossy(FLAGS).replace("\"format\": 1,", "\"format\": 1, \"x\": 1,");
    assert!(parse_flags(text.as_bytes()).is_ok());
}

fn dirs_for(sources: &str, files: &[&str]) -> Vec<String> {
    let m = manifest(&MANIFEST.replace("[\"src/main.c\", \"src/*.c\"]", sources));
    match make_with(&m, FLAGS, Host::Unix, files) {
        Ok(p) => p.dirs,
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn plan_dirs_nested_source() {
    let dirs = dirs_for("[\"src/game/logic/main.c\"]", &["src/game/logic/main.c"]);
    assert_eq!(
        dirs,
        [
            "build",
            "build/obj",
            "build/obj/src",
            "build/obj/src/game",
            "build/obj/src/game/logic",
        ]
    );
}

#[test]
fn plan_dirs_two_sources_one_folder() {
    let dirs = dirs_for("[\"src/a.c\", \"src/b.c\"]", &["src/a.c", "src/b.c"]);
    assert_eq!(dirs, ["build", "build/obj", "build/obj/src"]);
}

#[test]
fn plan_dirs_source_at_root() {
    let dirs = dirs_for("[\"main.c\"]", &["main.c"]);
    assert_eq!(dirs, ["build", "build/obj"]);
}

#[test]
fn plan_dirs_hold_every_output() {
    for host in [Host::Unix, Host::Windows] {
        let p = ok(host);
        for out in &p.outputs {
            let parent = &out[..out.rfind('/').unwrap_or(0)];
            assert!(p.dirs.iter().any(|d| d == parent), "{out}");
        }
        for (i, d) in p.dirs.iter().enumerate() {
            if let Some(at) = d.rfind('/') {
                assert!(p.dirs[..i].iter().any(|x| x == &d[..at]), "{d}");
            }
        }
    }
}

fn path_err(from: &str, to: &str) -> FlagsError {
    let text = String::from_utf8_lossy(FLAGS).replacen(from, to, 1);
    flags_err(&text)
}

#[test]
fn plan_flags_paths_checked() {
    let fields = [
        ("\"include\"", "include_dirs"),
        ("\"lib/holotape.ld\"", "linker_scripts.holotape"),
        ("\"lib/crt0.S\"", "crt0"),
        ("\"lib/libpip.a\"", "libs"),
    ];
    let bad = [
        ("/abs", "starts with /"),
        ("C:lib", "starts with a drive letter"),
        ("lib\\\\x", "uses \\, the separator is /"),
        ("lib//x", "has an empty component"),
        ("", "has an empty component"),
        ("lib/", "has an empty component"),
        ("./lib", "has a . component"),
        ("lib/../x", "has a .. component"),
        ("lib\\u0000x", "holds a NUL"),
    ];
    for (from, field) in fields {
        for (value, reason) in bad {
            let e = path_err(from, &format!("\"{value}\""));
            assert_eq!(e.code(), "manifest", "{field} {value}");
            let shown = serde_json::from_str::<String>(&format!("\"{value}\"")).unwrap_or_default();
            assert_eq!(e.detail(), format!("{field}: {shown:?}: {reason}"));
        }
    }
}

#[test]
fn plan_flags_paths_in_field_order_after_types() {
    // A bad path and a later type error: the type error wins.
    let text = String::from_utf8_lossy(FLAGS)
        .replace("\"include\"", "\"../x\"")
        .replace("\"PIPOS_ABI_TARGET\"", "1");
    assert_eq!(flags_err(&text).detail(), "abi_define: expected a string");
    // Two bad paths: the first field of the table wins.
    let text = String::from_utf8_lossy(FLAGS)
        .replace("\"lib/libpip.a\"", "\"/x\"")
        .replace("\"include\"", "\"../x\"");
    assert!(flags_err(&text).detail().starts_with("include_dirs:"));
}

#[test]
fn plan_flags_refuse_a_flag_that_holds_a_path() {
    for (from, key) in [("\"-O2\"", "cflags"), ("\"-Wl,--gc-sections\"", "ldflags")] {
        for flag in [
            "-o",
            "-oout.o",
            "-T",
            "-Tx.ld",
            "-Iinc",
            "-L/lib",
            "-include",
            "-MFdeps",
            "-specs=nano.specs",
            "--sysroot=/x",
            "@args",
        ] {
            let e = path_err(from, &format!("\"{flag}\""));
            assert_eq!(e.code(), "manifest", "{key} {flag}");
            assert_eq!(e.detail(), format!("{key}: {flag:?}: holds a path"));
        }
    }
}
