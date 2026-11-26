// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs `robco sdk install` and `robco sdk list` as a program, against a
//! local index and a cache in a temporary folder. Each test makes its own
//! small archives. No test touches the network.
//!
//! The two examples of "Examples" in RobCo-Core `docs/spec/STUDIO_CLI.md`
//! are copied below and must equal the program's output, line for line,
//! with `linux-x64` read as the running host.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test file; a failed unwrap is a failed test"
)]

use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use studio_core::codes;
use studio_core::install::{self, LocalFiles, Pin, sha256_hex};
use studio_core::resolve::index::{self, IndexSource};
use zip::write::SimpleFileOptions;

/// `robco sdk install pipos@1.4 --index index.json --json`, on Linux, with
/// `gcc` already in the cache.
const EXAMPLE_INSTALL: &str = r#"{"v":1,"kind":"step","step":"resolve","item":"","state":"start"}
{"v":1,"kind":"step","step":"resolve","item":"","state":"done"}
{"v":1,"kind":"step","step":"download","item":"pipos@1.4.0","state":"start"}
{"v":1,"kind":"step","step":"download","item":"pipos@1.4.0","state":"done"}
{"v":1,"kind":"step","step":"verify","item":"pipos@1.4.0","state":"start"}
{"v":1,"kind":"step","step":"verify","item":"pipos@1.4.0","state":"done"}
{"v":1,"kind":"step","step":"install","item":"pipos@1.4.0","state":"start"}
{"v":1,"kind":"step","step":"install","item":"pipos@1.4.0","state":"done"}
{"v":1,"kind":"step","step":"install","item":"gcc@15.2.1-1.1/linux-x64","state":"skip"}
{"v":1,"kind":"step","step":"download","item":"qemu@10.1.2/linux-x64","state":"start"}
{"v":1,"kind":"step","step":"download","item":"qemu@10.1.2/linux-x64","state":"done"}
{"v":1,"kind":"step","step":"verify","item":"qemu@10.1.2/linux-x64","state":"start"}
{"v":1,"kind":"step","step":"verify","item":"qemu@10.1.2/linux-x64","state":"done"}
{"v":1,"kind":"step","step":"install","item":"qemu@10.1.2/linux-x64","state":"start"}
{"v":1,"kind":"step","step":"install","item":"qemu@10.1.2/linux-x64","state":"done"}
{"v":1,"kind":"result","command":"sdk install","ok":true,"exit":0,"packages":[{"name":"pipos","version":"1.4.0","state":"installed"}],"tools":[{"name":"gcc","version":"15.2.1-1.1","host":"linux-x64","state":"present"},{"name":"qemu","version":"10.1.2","host":"linux-x64","state":"installed"}]}
"#;

/// `robco sdk list --json`, after that.
const EXAMPLE_LIST: &str = r#"{"v":1,"kind":"item","type":"package","name":"pipos","version":"1.4.0","host":"","installed":true,"offered":false}
{"v":1,"kind":"item","type":"tool","name":"gcc","version":"15.2.1-1.1","host":"linux-x64","installed":true,"offered":false}
{"v":1,"kind":"item","type":"tool","name":"qemu","version":"10.1.2","host":"linux-x64","installed":true,"offered":false}
{"v":1,"kind":"result","command":"sdk list","ok":true,"exit":0,"count":3}
"#;

fn host() -> &'static str {
    studio_core::resolve::running_host().expect("a host the index names")
}

/// An example with `linux-x64` read as the running host.
fn on_host(example: &str) -> String {
    example.replace("linux-x64", host())
}

fn opts() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

/// The files of "Layout of pipos", `sdk.json` aside.
const FILES: &[(&str, &[u8])] = &[
    ("LICENSES/MIT.txt", b"MIT licence text\n"),
    ("boot.json", b"{\"format\": 1}\n"),
    ("boot/PIPOS.CFG", b"cfg\n"),
    ("boot/PIPOS.IMG", b"img\n"),
    ("boot/RUNNER.ROM", b"rom\n"),
    ("boot/config.txt", b"config\n"),
    ("boot/fwstub.bin", b"stub\n"),
    ("boot/pipos.elf", b"elf\n"),
    ("docs/API.md", b"api\n"),
    ("flags.json", b"{\"format\": 1}\n"),
    ("include/pipos/abi.h", b"h\n"),
    ("include/pipos/crc32.h", b"h\n"),
    ("include/pipos/crt0.h", b"h\n"),
    ("include/pipos/heap.h", b"h\n"),
    ("include/pipos/ini.h", b"h\n"),
    ("include/pipos/pip.h", b"h\n"),
    ("include/pipos/printf.h", b"h\n"),
    ("include/pipos/selftest.h", b"h\n"),
    ("include/pipos/string.h", b"h\n"),
    ("include/pipos/syscall.h", b"h\n"),
    ("lib/crt0.S", b"s\n"),
    ("lib/holotape.ld", b"ld\n"),
    ("lib/libpip.a", b"a\n"),
    ("lib/rom.ld", b"ld\n"),
];

/// The `sdk.json` of `pipos` 1.4.0. `qemu` is the version it names.
fn sdk_json(qemu: &str) -> Value {
    let files: Vec<Value> = FILES
        .iter()
        .map(|(p, b)| json!({"path": p, "size": b.len(), "sha256": sha256_hex(b)}))
        .collect();
    json!({
        "format": 1,
        "name": "pipos",
        "version": "1.4.0",
        "abi": "1.4",
        "built_from": [
            {"repo": "Pip-OS", "commit": "1111111111111111111111111111111111111111"},
            {"repo": "RobCo-Core", "commit": "2222222222222222222222222222222222222222"}
        ],
        "formats": {"rimg": 1, "flags": 1, "boot": 1, "runner": 1},
        "requires": [],
        "tools": [
            {"name": "gcc", "version": "15.2.1-1.1"},
            {"name": "qemu", "version": qemu}
        ],
        "firmware": {
            "repo": "raspberrypi/firmware",
            "tag": "1.20260915",
            "commit": "6f0881cba8bea8ec24956a5718714730e9936ec5",
            "base_url": "https://raw.githubusercontent.com/raspberrypi/firmware/6f0881cba8bea8ec24956a5718714730e9936ec5/boot",
            "files": [
                {"name": "bootcode.bin", "sha256": "4245ec23b58158dc3e55f3e065eb4ecc0c0705d76c8db506e9f824941feee32e"},
                {"name": "start.elf", "sha256": "32f990801e7765bae6fe5cb940ba5eeff02dd0dac8dc5474db5b3fd345281790"},
                {"name": "fixup.dat", "sha256": "7853f2b51e4489f8b3ce6f9c52fd0afdda38c7383961524e0883d34afcb4827b"}
            ]
        },
        "licences": [{"paths": [""], "spdx": "MIT"}],
        "files": files
    })
}

/// A package zip: `sdk.json`, the files, then each entry of `extra`.
fn package_zip(sdk: &Value, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("sdk.json", opts()).unwrap();
    z.write_all(sdk.to_string().as_bytes()).unwrap();
    for (name, bytes) in FILES.iter().chain(extra) {
        z.start_file(*name, opts()).unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap().into_inner()
}

/// A tool zip with the folder `<root>/bin/` and nothing real in it, or
/// with no `bin/` when `bin` is false.
fn tool_zip(root: &str, bin: bool) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let dir = if bin { "bin/" } else { "lib/" };
    let path = if root.is_empty() {
        dir.to_owned()
    } else {
        format!("{root}/{dir}")
    };
    z.add_directory(path, opts()).unwrap();
    z.finish().unwrap().into_inner()
}

/// A test folder: `index.json` and its archives, and `cache/`. Removed on
/// drop.
struct Fixture {
    dir: PathBuf,
    index: Value,
}

impl Fixture {
    /// The archives of the examples, and an index that lists them for the
    /// running host.
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "robco-sdk-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("archives")).unwrap();
        let mut f = Self {
            dir,
            index: Value::Null,
        };
        let pkg = package_zip(&sdk_json("10.1.2"), &[]);
        let gcc = tool_zip("", true);
        let qemu = tool_zip("qemu-10.1.2", true);
        f.put("archives/pipos-1.4.0.zip", &pkg);
        f.put("archives/gcc.zip", &gcc);
        f.put("archives/qemu.zip", &qemu);
        let archive = |url: &str, b: &[u8], root: &str| json!({"url": url, "size": b.len(), "sha256": sha256_hex(b), "root": root});
        let mut gcc_archives = serde_json::Map::new();
        gcc_archives.insert(host().to_owned(), archive("archives/gcc.zip", &gcc, ""));
        let mut qemu_archives = serde_json::Map::new();
        qemu_archives.insert(
            host().to_owned(),
            archive("archives/qemu.zip", &qemu, "qemu-10.1.2"),
        );
        f.index = json!({
            "format": 1,
            "packages": [{
                "name": "pipos",
                "version": "1.4.0",
                "abi": "1.4",
                "requires": [],
                "tools": [
                    {"name": "gcc", "version": "15.2.1-1.1"},
                    {"name": "qemu", "version": "10.1.2"}
                ],
                "formats": {"rimg": 1, "flags": 1, "boot": 1, "runner": 1},
                "url": "archives/pipos-1.4.0.zip",
                "size": pkg.len(),
                "sha256": sha256_hex(&pkg)
            }],
            "tools": [
                {"name": "gcc", "version": "15.2.1-1.1", "archives": gcc_archives},
                {"name": "qemu", "version": "10.1.2", "archives": qemu_archives}
            ]
        });
        f.write_index();
        f
    }

    fn put(&self, rel: &str, bytes: &[u8]) {
        fs::write(self.dir.join(rel), bytes).unwrap();
    }

    fn write_index(&self) {
        self.put("index.json", self.index.to_string().as_bytes());
    }

    /// Replaces an archive and the size and hash its index entry gives.
    fn replace_package(&mut self, bytes: &[u8]) {
        self.put("archives/pipos-1.4.0.zip", bytes);
        let p = &mut self.index["packages"][0];
        p["size"] = json!(bytes.len());
        p["sha256"] = json!(sha256_hex(bytes));
        self.write_index();
    }

    fn qemu_archive(&mut self) -> &mut Value {
        &mut self.index["tools"][1]["archives"][host()]
    }

    fn cache(&self) -> PathBuf {
        self.dir.join("cache")
    }

    /// Runs `robco` in the fixture folder with the cache of the fixture.
    fn robco(&self, args: &[&str]) -> Output {
        self.robco_with(args, Some(self.cache().as_os_str()))
    }

    /// Runs `robco` with only the variables it needs: `ROBCO_CACHE` when
    /// given, and on Windows the system root.
    fn robco_with(&self, args: &[&str], cache: Option<&std::ffi::OsStr>) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_robco"));
        c.args(args).current_dir(&self.dir).env_clear();
        if let Some(cache) = cache {
            c.env("ROBCO_CACHE", cache);
        }
        if let Some(root) = std::env::var_os("SYSTEMROOT") {
            c.env("SYSTEMROOT", root);
        }
        c.output().expect("robco runs")
    }

    /// Puts the `gcc` archive into the cache, as an earlier install did.
    fn gcc_in_cache(&self) {
        let idx = index::parse(self.index.to_string().as_bytes(), IndexSource::Local).unwrap();
        let gcc = &idx.tools[0];
        install::tool(
            &self.cache(),
            "gcc",
            &gcc.version,
            host(),
            &gcc.archives[host()],
            &LocalFiles {
                base: self.dir.clone(),
            },
            Pin::Index,
        )
        .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap()
}

fn install_json(f: &Fixture) -> Output {
    f.robco(&[
        "sdk",
        "install",
        "pipos@1.4",
        "--index",
        "index.json",
        "--json",
    ])
}

#[test]
fn sdk_install_example_output() {
    let f = Fixture::new("install-example");
    f.gcc_in_cache();
    let out = install_json(&f);
    assert_eq!(text(&out.stdout), on_host(EXAMPLE_INSTALL));
    assert!(out.stderr.is_empty());
    assert_eq!(out.status.code(), Some(0));

    // The package and both tools are in the cache, `bin/` right under each
    // tool folder.
    let cache = f.cache();
    assert!(cache.join("packages/pipos/1.4.0/flags.json").is_file());
    assert!(
        cache
            .join(format!("tools/gcc/15.2.1-1.1/{}/bin", host()))
            .is_dir()
    );
    assert!(
        cache
            .join(format!("tools/qemu/10.1.2/{}/bin", host()))
            .is_dir()
    );
}

#[test]
fn sdk_list_example_output() {
    let f = Fixture::new("list-example");
    f.gcc_in_cache();
    assert_eq!(install_json(&f).status.code(), Some(0));
    let out = f.robco(&["sdk", "list", "--json"]);
    assert_eq!(text(&out.stdout), on_host(EXAMPLE_LIST));
    assert!(out.stderr.is_empty());
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn sdk_install_twice_skips_everything() {
    let f = Fixture::new("twice");
    assert_eq!(install_json(&f).status.code(), Some(0));
    let out = install_json(&f);
    assert_eq!(out.status.code(), Some(0));
    let h = host();
    let want = format!(
        r#"{{"v":1,"kind":"step","step":"resolve","item":"","state":"start"}}
{{"v":1,"kind":"step","step":"resolve","item":"","state":"done"}}
{{"v":1,"kind":"step","step":"install","item":"pipos@1.4.0","state":"skip"}}
{{"v":1,"kind":"step","step":"install","item":"gcc@15.2.1-1.1/{h}","state":"skip"}}
{{"v":1,"kind":"step","step":"install","item":"qemu@10.1.2/{h}","state":"skip"}}
{{"v":1,"kind":"result","command":"sdk install","ok":true,"exit":0,"packages":[{{"name":"pipos","version":"1.4.0","state":"present"}}],"tools":[{{"name":"gcc","version":"15.2.1-1.1","host":"{h}","state":"present"}},{{"name":"qemu","version":"10.1.2","host":"{h}","state":"present"}}]}}
"#
    );
    assert_eq!(text(&out.stdout), want);

    // Without --json: one plain line per item, nothing on standard error.
    let out = f.robco(&["sdk", "install", "pipos@1.4", "--index=index.json"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        text(&out.stdout).replace("\r\n", "\n"),
        format!(
            "package pipos 1.4.0 present\ntool gcc 15.2.1-1.1 {h} present\ntool qemu 10.1.2 {h} present\n"
        )
    );
    assert!(out.stderr.is_empty());
}

#[test]
fn sdk_list_with_index_marks_what_is_offered() {
    let f = Fixture::new("list-index");
    let h = host();
    let out = f.robco(&["sdk", "list", "--index", "index.json", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let want = format!(
        r#"{{"v":1,"kind":"item","type":"package","name":"pipos","version":"1.4.0","host":"","installed":false,"offered":true}}
{{"v":1,"kind":"item","type":"tool","name":"gcc","version":"15.2.1-1.1","host":"{h}","installed":false,"offered":true}}
{{"v":1,"kind":"item","type":"tool","name":"qemu","version":"10.1.2","host":"{h}","installed":false,"offered":true}}
{{"v":1,"kind":"result","command":"sdk list","ok":true,"exit":0,"count":3}}
"#
    );
    assert_eq!(text(&out.stdout), want);

    // An empty cache without --index lists nothing.
    let out = f.robco(&["sdk", "list"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());

    let out = f.robco(&["sdk", "list", "--index", "index.json"]);
    assert_eq!(
        text(&out.stdout).replace("\r\n", "\n"),
        format!(
            "package pipos 1.4.0 offered\ntool gcc 15.2.1-1.1 {h} offered\ntool qemu 10.1.2 {h} offered\n"
        )
    );
}

/// Runs a refused command line twice, without and with `--json`, and checks
/// the exit code of `code`, the one error line and the streams.
fn refused(f: &Fixture, args: &[&str], code: &str, cache: Option<&std::ffi::OsStr>) {
    let exit = i32::from(codes::exit_code(code).unwrap());
    let cache = cache.or(Some(f.cache().as_os_str())).map(|c| c.to_owned());

    let out = f.robco_with(args, cache.as_deref());
    assert_eq!(
        out.status.code(),
        Some(exit),
        "{args:?}: {}",
        text(&out.stderr)
    );
    assert!(out.stdout.is_empty(), "{args:?}: {}", text(&out.stdout));
    let err = text(&out.stderr);
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 1, "{args:?}: {err}");
    assert!(
        lines[0].starts_with(&format!("{code}: ")),
        "{args:?}: {err}"
    );

    let mut json_args = args.to_vec();
    json_args.push("--json");
    let out = f.robco_with(&json_args, cache.as_deref());
    assert_eq!(out.status.code(), Some(exit), "{json_args:?}");
    assert!(out.stderr.is_empty(), "{json_args:?}");
    let stdout = text(&out.stdout);
    let lines: Vec<Value> = stdout
        .split_terminator('\n')
        .map(|l| serde_json::from_str(l).expect("one object per line"))
        .collect();
    let errors: Vec<&Value> = lines.iter().filter(|l| l["kind"] == "error").collect();
    assert_eq!(errors.len(), 1, "{stdout}");
    let n = lines.len();
    assert_eq!(lines[n - 2]["kind"], "error", "{stdout}");
    assert_eq!(lines[n - 2]["code"], code, "{stdout}");
    assert_eq!(lines[n - 2]["exit"], exit, "{stdout}");
    assert_eq!(lines[n - 1]["kind"], "result", "{stdout}");
    assert_eq!(lines[n - 1]["ok"], false, "{stdout}");
    assert_eq!(lines[n - 1]["exit"], exit, "{stdout}");
    assert!(
        !lines.iter().any(|l| l["kind"] == "item"),
        "a failed list writes no item line: {stdout}"
    );
}

const INSTALL: &[&str] = &["sdk", "install", "pipos@1.4", "--index", "index.json"];

#[test]
fn sdk_refuses_wrong_command_lines() {
    let f = Fixture::new("usage");
    for args in [
        &["sdk"][..],
        &["sdk", "remove", "pipos"],
        &["sdk", "doctor"],
        &["sdk", "install"],
        &["sdk", "install", "pipos@1"],
        &["sdk", "install", "pipos", "extra", "--index", "index.json"],
        &["sdk", "list", "--index"],
        &["sdk", "list", "--index", "a", "--index=b"],
        &["sdk", "list", "--bogus"],
    ] {
        refused(&f, args, "usage", None);
    }
}

#[test]
fn sdk_refuses_a_relative_cache() {
    let f = Fixture::new("cache");
    refused(&f, INSTALL, "cache: path", Some("rel/cache".as_ref()));
    refused(
        &f,
        &["sdk", "list"],
        "cache: path",
        Some("rel/cache".as_ref()),
    );
}

#[test]
fn sdk_refuses_bad_index_locations() {
    let f = Fixture::new("location");
    refused(&f, &["sdk", "install", "pipos"], "index: location", None);
    refused(
        &f,
        &["sdk", "install", "pipos", "--index", "http://x/i.json"],
        "index: location",
        None,
    );
    refused(
        &f,
        &["sdk", "list", "--index", "ftp://x/i.json"],
        "index: location",
        None,
    );
    refused(
        &f,
        &["sdk", "install", "pipos", "--index", "none.json"],
        "index: read",
        None,
    );
    refused(
        &f,
        &["sdk", "list", "--index", "none.json"],
        "index: read",
        None,
    );
    // No https transport exists yet: the index cannot be fetched.
    refused(
        &f,
        &[
            "sdk",
            "install",
            "pipos",
            "--index",
            "https://x.invalid/i.json",
        ],
        "index: read",
        None,
    );
}

#[test]
fn sdk_refuses_a_bad_index() {
    type Change = fn(&mut Value);
    let cases: &[(&str, Change)] = &[
        ("index: format", |i| i["format"] = json!(2)),
        ("index: entry", |i| {
            i["packages"][0]["sha256"] = json!("A".repeat(64));
        }),
        ("index: duplicate", |i| {
            let p = i["packages"][0].clone();
            i["packages"].as_array_mut().unwrap().push(p);
        }),
        ("sdk: no tool", |i| {
            i["tools"][1]["version"] = json!("10.1.3")
        }),
        ("index: host", |i| {
            let a = i["tools"][1]["archives"].as_object_mut().unwrap();
            let v = a.remove(host()).unwrap();
            a.insert("other-host".to_owned(), v);
        }),
        ("package: format", |i| {
            i["packages"][0]["formats"]["rimg"] = json!(99);
        }),
    ];
    for (code, change) in cases {
        let mut f = Fixture::new("index");
        change(&mut f.index);
        f.write_index();
        let args: &[&str] = if *code == "package: format" {
            &["sdk", "install", "pipos@1.4.0", "--index", "index.json"]
        } else {
            INSTALL
        };
        refused(&f, args, code, None);
    }

    let f = Fixture::new("syntax");
    f.put("index.json", b"not json");
    refused(&f, INSTALL, "index: syntax", None);
    refused(
        &f,
        &["sdk", "list", "--index", "index.json"],
        "index: syntax",
        None,
    );
}

#[test]
fn sdk_refuses_what_the_index_does_not_list() {
    let f = Fixture::new("no-package");
    for argument in ["gcc", "pipos@2.0", "pipos@1.4.1", "nothing"] {
        refused(
            &f,
            &["sdk", "install", argument, "--index", "index.json"],
            "sdk: no package",
            None,
        );
    }
}

#[test]
fn sdk_refuses_bad_archives() {
    // A package archive that does not match its hash.
    let mut f = Fixture::new("package-hash");
    f.index["packages"][0]["sha256"] = json!("0".repeat(64));
    f.write_index();
    refused(&f, INSTALL, "package: hash", None);

    // A package archive with a record entry.
    let mut f = Fixture::new("package-archive");
    f.replace_package(&package_zip(
        &sdk_json("10.1.2"),
        &[(".robco-archive", b"x")],
    ));
    refused(&f, INSTALL, "package: archive", None);

    // An sdk.json whose tools differ from its index entry.
    let mut f = Fixture::new("package-manifest");
    f.replace_package(&package_zip(&sdk_json("10.1.3"), &[]));
    refused(&f, INSTALL, "package: manifest", None);

    // A tool archive of another size.
    let mut f = Fixture::new("tool-hash");
    f.qemu_archive()["size"] = json!(1);
    f.write_index();
    refused(&f, INSTALL, "tool: hash", None);

    // A tool archive without bin/.
    let mut f = Fixture::new("tool-archive");
    let bad = tool_zip("qemu-10.1.2", false);
    f.put("archives/qemu.zip", &bad);
    let a = f.qemu_archive();
    a["size"] = json!(bad.len());
    a["sha256"] = json!(sha256_hex(&bad));
    f.write_index();
    refused(&f, INSTALL, "tool: archive", None);
}

#[test]
fn sdk_refuses_a_cache_it_cannot_use() {
    // The cache is a file. Unix reports a read under a file, Windows a
    // path that is not there, then a refused create: `io: read` or
    // `io: write`, both exit 9.
    let f = Fixture::new("io");
    let file = f.dir.join("cache-file");
    fs::write(&file, b"").unwrap();
    let out = f.robco_with(INSTALL, Some(file.as_os_str()));
    let err = text(&out.stderr);
    let code = if err.starts_with("io: read: ") {
        "io: read"
    } else {
        "io: write"
    };
    refused(&f, INSTALL, code, Some(file.as_os_str()));
}

#[test]
fn sdk_install_fails_in_the_install_step() {
    // The archive passes its hash and breaks a rule of "The archive".
    let mut f = Fixture::new("install-step");
    f.replace_package(&package_zip(
        &sdk_json("10.1.2"),
        &[(".robco-archive", b"x")],
    ));
    let out = install_json(&f);
    assert_eq!(out.status.code(), Some(4));
    let stdout = text(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    let want = [
        r#"{"v":1,"kind":"step","step":"resolve","item":"","state":"start"}"#,
        r#"{"v":1,"kind":"step","step":"resolve","item":"","state":"done"}"#,
        r#"{"v":1,"kind":"step","step":"download","item":"pipos@1.4.0","state":"start"}"#,
        r#"{"v":1,"kind":"step","step":"download","item":"pipos@1.4.0","state":"done"}"#,
        r#"{"v":1,"kind":"step","step":"verify","item":"pipos@1.4.0","state":"start"}"#,
        r#"{"v":1,"kind":"step","step":"verify","item":"pipos@1.4.0","state":"done"}"#,
        r#"{"v":1,"kind":"step","step":"install","item":"pipos@1.4.0","state":"start"}"#,
    ];
    assert_eq!(lines[..lines.len() - 2], want, "{stdout}");
    let error: Value = serde_json::from_str(lines[lines.len() - 2]).unwrap();
    assert_eq!(error["code"], "package: archive", "{stdout}");
}

#[test]
fn sdk_refuses_a_pipos_package_without_flags_json() {
    let mut f = Fixture::new("no-flags");
    let mut sdk = sdk_json("10.1.2");
    sdk["files"]
        .as_array_mut()
        .unwrap()
        .retain(|e| e["path"] != "flags.json");
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("sdk.json", opts()).unwrap();
    z.write_all(sdk.to_string().as_bytes()).unwrap();
    for (name, bytes) in FILES.iter().filter(|(p, _)| *p != "flags.json") {
        z.start_file(*name, opts()).unwrap();
        z.write_all(bytes).unwrap();
    }
    f.replace_package(&z.finish().unwrap().into_inner());
    refused(&f, INSTALL, "package: manifest", None);
}

/// A cache where no temporary folder can be made fails inside `download`,
/// the first step of the item: its `start` line has no `done`.
#[cfg(unix)]
#[test]
fn sdk_install_fails_in_the_download_step_on_a_read_only_cache() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new("read-only");
    fs::create_dir_all(f.cache()).unwrap();
    fs::set_permissions(f.cache(), fs::Permissions::from_mode(0o555)).unwrap();
    let out = install_json(&f);
    fs::set_permissions(f.cache(), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(out.status.code(), Some(9));
    let stdout = text(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    let want = [
        r#"{"v":1,"kind":"step","step":"resolve","item":"","state":"start"}"#,
        r#"{"v":1,"kind":"step","step":"resolve","item":"","state":"done"}"#,
        r#"{"v":1,"kind":"step","step":"download","item":"pipos@1.4.0","state":"start"}"#,
    ];
    assert_eq!(lines[..lines.len() - 2], want, "{stdout}");
    let error: Value = serde_json::from_str(lines[lines.len() - 2]).unwrap();
    assert_eq!(error["code"], "io: write", "{stdout}");
    assert_eq!(error["exit"], 9, "{stdout}");
    let result: Value = serde_json::from_str(lines[lines.len() - 1]).unwrap();
    assert_eq!(result["ok"], false, "{stdout}");
}
