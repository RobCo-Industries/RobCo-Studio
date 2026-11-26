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
fn plan_flags_a_bad_value_wins_over_a_later_type_error() {
    // Each field is checked whole before the next: the earlier bad path wins.
    let text = String::from_utf8_lossy(FLAGS)
        .replace("\"include\"", "\"../x\"")
        .replace("\"PIPOS_ABI_TARGET\"", "1");
    assert!(flags_err(&text).detail().starts_with("include_dirs:"));
}

/// One value fault per field of the spec's table, in the table's order.
fn faults() -> Vec<(&'static str, Value)> {
    use serde_json::json;
    vec![
        ("cflags", json!(["-B"])),
        ("include_dirs", json!(["../x"])),
        ("ldflags", json!(["-specs=x"])),
        ("linker_scripts", json!({"holotape": "/x.ld"})),
        ("crt0", json!("-x.S")),
        (
            "crt0_defines",
            json!({"holotape-game": ["A=1\n#include \"/etc/x\""]}),
        ),
        ("libs", json!(["/x.a"])),
        ("system_libs", json!(["-specs=/tmp/inject.specs"])),
        ("abi_define", json!("X=1")),
    ]
}

#[test]
fn plan_flags_two_faults_report_the_first_of_the_table() {
    let base: Value = match serde_json::from_slice(FLAGS) {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    };
    let faults = faults();
    for (one, _) in &faults {
        let mut f = base.clone();
        if let (Value::Object(m), Some((_, v))) = (&mut f, faults.iter().find(|x| x.0 == *one)) {
            m.insert((*one).into(), v.clone());
        }
        assert!(flags_err(&f.to_string()).detail().starts_with(one), "{one}");
    }
    for pair in faults.windows(2) {
        let mut f = base.clone();
        if let Value::Object(m) = &mut f {
            for (k, v) in pair {
                m.insert((*k).into(), v.clone());
            }
        }
        let e = flags_err(&f.to_string());
        assert_eq!(e.code(), "manifest");
        assert!(
            e.detail().starts_with(pair[0].0),
            "{} then {}: {e}",
            pair[0].0,
            pair[1].0
        );
    }
}

#[test]
fn plan_system_libs_take_library_flags_only() {
    assert_passes(
        "system_libs",
        &["-lgcc", "-lc", "-lstdc++", "-lgcc_s", "-lm-2"],
    );
    let bad = [
        "-specs=/tmp/inject.specs",
        "-l",
        "-lGCC",
        "-l../x",
        "-l/tmp/x",
        "-lgcc,x",
        "-L/tmp",
        "-Wl,--gc-sections",
        "-O2",
        "gcc",
    ];
    for f in bad {
        match shape_ok("system_libs", f) {
            Ok(()) => panic!("{f:?} passed"),
            Err(e) => assert_eq!(
                e.detail(),
                format!("system_libs: {f:?}: not in the allowed flag shapes")
            ),
        }
    }
    // A library flag is not a cflag or an ldflag.
    assert!(shape_ok("cflags", "-lgcc").is_err());
    assert!(shape_ok("ldflags", "-lgcc").is_err());
    let e = path_err("\"-lgcc\"", "\"-specs=/tmp/inject.specs\"");
    assert_eq!(e.code(), "manifest");
    assert_eq!(
        e.detail(),
        "system_libs: \"-specs=/tmp/inject.specs\": not in the allowed flag shapes"
    );
}

#[test]
fn plan_package_paths_refuse_options_and_streams() {
    let fields = [
        ("\"include\"", "include_dirs"),
        ("\"lib/holotape.ld\"", "linker_scripts.holotape"),
        ("\"lib/crt0.S\"", "crt0"),
        ("\"lib/libpip.a\"", "libs"),
    ];
    let bad = [
        ("-specs=x", "a part starts with -"),
        ("lib/-x", "a part starts with -"),
        ("lib/a:b", "holds a : or a control character"),
        ("lib/a\\nb", "holds a : or a control character"),
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
fn plan_inside_is_by_components() {
    let u = Host::Unix;
    assert!(inside(u, "/c/pkg", "/c/pkg/lib/x.a"));
    assert!(!inside(u, "/c/pkg", "/c/pkg"));
    assert!(!inside(u, "/c/pkg", "/c/pkg/../x"));
    assert!(!inside(u, "/c/pkg", "/c/pkg/lib/../../x"));
    assert!(!inside(u, "/c/pkg", "/c/pkgx/lib"));
    assert!(!inside(u, "/c/pkg", "/../../c/pkg/x"));
    let w = Host::Windows;
    assert!(inside(w, "C:\\c\\pkg", "C:\\c\\pkg\\lib\\x.a"));
    assert!(!inside(w, "C:\\c\\pkg", "C:\\c\\pkg\\..\\x"));
    assert!(!inside(w, "C:\\c\\pkg", "C:\\c\\pkg/lib\\..\\..\\x"));
}

#[test]
fn plan_defines_and_abi_define_are_names() {
    for (from, to) in [
        (
            "\"PIP_IMG_SUBTYPE=0\"",
            "\"PIP_IMG_SUBTYPE=0\\n#include \\\"/etc/x\\\"\"",
        ),
        ("\"PIP_IMG_SUBTYPE=0\"", "\"PIP_IMG_SUBTYPE\""),
        ("\"PIP_IMG_SUBTYPE=0\"", "\"1X=0\""),
        ("\"PIP_IMG_SUBTYPE=0\"", "\"X=/etc/passwd\""),
        ("\"PIP_IMG_SUBTYPE=0\"", "\"X=\\\"a\\\"b\\\"\""),
    ] {
        let e = path_err(from, to);
        assert_eq!(e.code(), "manifest", "{to}");
        assert!(
            e.detail().starts_with("crt0_defines.holotape-game: "),
            "{e}"
        );
    }
    let e = path_err("\"PIPOS_ABI_TARGET\"", "\"X=1\"");
    assert_eq!(
        e.detail(),
        "abi_define: \"X=1\": not a C name, [A-Za-z_][A-Za-z0-9_]*"
    );
    // The values Pip-OS ships pass.
    assert!(parse_flags(FLAGS).is_ok());
}

#[test]
fn plan_source_with_a_leading_dash_is_not_an_option() {
    let m = manifest(&MANIFEST.replace("[\"src/main.c\", \"src/*.c\"]", "[\"*.c\"]"));
    let p = match make_with(&m, FLAGS, Host::Unix, &["-specs=x.c", "robco.toml"]) {
        Ok(p) => p,
        Err(e) => panic!("{e}"),
    };
    assert_eq!(p.sources, ["-specs=x.c"]);
    let c = &p.commands[1];
    assert_eq!(c.step, Step::Compile("-specs=x.c".into()));
    assert!(c.args.contains(&"./-specs=x.c".to_string()));
    assert!(!c.args.contains(&"-specs=x.c".to_string()));
}

fn shape_ok(key: &str, flag: &str) -> Result<(), FlagsError> {
    check_flags(key, &[flag.to_string()])
}

fn assert_passes(key: &str, flags: &[&str]) {
    for f in flags {
        if let Err(e) = shape_ok(key, f) {
            panic!("{key} {f:?}: {e}");
        }
    }
}

#[test]
fn plan_flag_shape_machine() {
    assert_passes(
        "cflags",
        &["-marm", "-mcpu=arm1176jzf-s", "-mfpu=vfp+x.y_z"],
    );
}

#[test]
fn plan_flag_shape_feature() {
    assert_passes(
        "cflags",
        &["-ffreestanding", "-fno-builtin", "-fvisibility=hidden"],
    );
}

#[test]
fn plan_flag_shape_std() {
    assert_passes("cflags", &["-std=gnu11", "-std=c++17"]);
}

#[test]
fn plan_flag_shape_optimize() {
    assert_passes(
        "cflags",
        &["-O", "-O0", "-O2", "-Os", "-Og", "-Oz", "-Ofast"],
    );
}

#[test]
fn plan_flag_shape_debug() {
    assert_passes("cflags", &["-g", "-g3", "-ggdb", "-ggdb2", "-gdwarf-4"]);
}

#[test]
fn plan_flag_shape_warning() {
    assert_passes(
        "cflags",
        &[
            "-Wall",
            "-Wno-unused-parameter",
            "-Wformat=2",
            "-w",
            "-pedantic",
            "-pedantic-errors",
        ],
    );
}

#[test]
fn plan_flag_shape_word() {
    assert_passes(
        "cflags",
        &[
            "-nostdlib",
            "-nostartfiles",
            "-nodefaultlibs",
            "-nostdinc",
            "-nolibc",
            "-static",
        ],
    );
}

#[test]
fn plan_flag_shape_define() {
    assert_passes("cflags", &["-DNDEBUG", "-D_X=1", "-DV=1.2+rc-1"]);
}

#[test]
fn plan_flag_shape_undefine() {
    assert_passes("cflags", &["-UNDEBUG", "-U_X"]);
}

#[test]
fn plan_flag_shape_linker() {
    assert_passes(
        "ldflags",
        &[
            "-Wl,--gc-sections",
            "-Wl,--gc-sections,--no-warn-rwx-segments",
        ],
    );
}

/// The flags Pip-OS ships, from `sdk/pipos-app.mk`: PIP_CODEFLAGS as
/// `cflags`, PIP_LDFLAGS with PIP_LD_RWX as `ldflags`.
#[test]
fn plan_flags_of_pipos_pass() {
    let arch = [
        "-mcpu=arm1176jzf-s",
        "-marm",
        "-mfloat-abi=soft",
        "-mgeneral-regs-only",
        "-mno-unaligned-access",
    ];
    let mut cflags = arch.to_vec();
    cflags.extend([
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
    ]);
    let mut ldflags = arch.to_vec();
    ldflags.extend([
        "-nostdlib",
        "-Wl,--gc-sections",
        "-Wl,--no-warn-rwx-segments",
    ]);
    assert_passes("cflags", &cflags);
    assert_passes("ldflags", &ldflags);
}

#[test]
fn plan_flags_refused_outside_the_shapes() {
    let bad = [
        "-Wl,-Map=/tmp/x.map",
        "-Wl,-Map,x.map",
        "-Wl,--build-id=none",
        "-Wl,-T,x.ld",
        "-Wa,-ahl=x",
        "-Wp,-MD,x",
        "-Xlinker",
        "-fplugin=x",
        "-fdump-rtl-all",
        "-ffile-prefix-map=a=b",
        "-wrapper",
        "-B",
        "-Bdir",
        "-o",
        "-ox",
        "-c",
        "-S",
        "-E",
        "-x",
        "-include",
        "-I.",
        "-L.",
        "-lfoo",
        "-T",
        "@file",
        "-specs=x",
        "--sysroot=x",
        "-save-temps",
        "-pipe",
        "",
        "-O2 -g",
        "-O2\n",
        "-mcpu=../x",
        "-DNAME=/etc/passwd",
    ];
    for key in ["cflags", "ldflags"] {
        for f in bad {
            match shape_ok(key, f) {
                Ok(()) => panic!("{key} {f:?} passed"),
                Err(e) => {
                    assert_eq!(e.code(), "manifest");
                    assert_eq!(
                        e.detail(),
                        format!("{key}: {f:?}: not in the allowed flag shapes")
                    );
                }
            }
        }
    }
    assert!(shape_ok("cflags", "-Wl,--gc-sections").is_err());
}

#[test]
fn plan_flags_refuse_a_flag_through_parse() {
    for (from, key) in [("\"-O2\"", "cflags"), ("\"-Wl,--gc-sections\"", "ldflags")] {
        let e = path_err(from, "\"-Wl,-Map=/tmp/tape.map\"");
        assert_eq!(e.code(), "manifest");
        assert_eq!(
            e.detail(),
            format!("{key}: \"-Wl,-Map=/tmp/tape.map\": not in the allowed flag shapes")
        );
    }
}
