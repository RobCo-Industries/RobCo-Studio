// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs `robco new` as a program, against a cache in a temporary folder.
//! No test touches the network.
//!
//! The program carries no template. Each test makes its own `templates`
//! package from the files below, laid out as "The templates package" of
//! RobCo-Core `docs/spec/SDK_PACKAGE.md` says, with the one template
//! `holotape-c`. `robco.toml` and `tests/start.toml` are the spec's text;
//! `src/main.c`, the icon and the image are small stand-ins written here.
//!
//! Two tests need files this repository does not hold, and skip without
//! them:
//!
//! - `ROBCO_TEST_TEMPLATES`: the path of a real `templates` package zip.
//! - `ROBCO_TEST_PIPOS`, `ROBCO_TEST_GCC` and `ROBCO_TEST_GCC_ROOT`, as in
//!   `tests/build.rs`: the project `robco new` wrote is then built.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test file; a failed unwrap is a failed test"
)]

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use studio_core::install::sha256_hex;
use zip::write::SimpleFileOptions;

const ROBCO_TOML: &str = r#"format = 1

[project]
name = "@NAME@"
title = "@NAME@"
version = "0.1.0"
kind = "holotape"
subtype = "utility"
icon = "icon.pbm"

[target]
platform = "pipos"
abi = "1.4"

[build]
backend = "robco"
sources = ["src/*.c"]
"#;

const START_TOML: &str = r#"format = 1

[[step]]
expect = "tape: ready"

[[step]]
screen = "ready"

[[step]]
tap = "select"

[[step]]
exit = 0
"#;

/// A stand-in tape: it builds, and logs the line. It does not draw.
const MAIN_C: &str = "/* SPDX-License-Identifier: MIT */\r\n#include <pipos/pip.h>\r\n\r\n\
int pip_main(const pipos_startup_t *startup)\r\n{\r\n    (void)startup;\r\n    \
pip_printf(\"tape: ready\\n\");\r\n    return 0;\r\n}\r\n";

/// A stand-in image: bytes a copy must keep, CR LF included.
const READY_PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot an image, @TITLE@ kept\r\n";

/// A 32x32 `P1` icon: a frame with a cross.
fn icon() -> Vec<u8> {
    let mut s = String::from("P1\n32 32\n");
    for y in 0..32 {
        let row: Vec<&str> = (0..32)
            .map(|x| {
                let on = x == 0 || y == 0 || x == 31 || y == 31 || x == y || x + y == 31;
                if on { "1" } else { "0" }
            })
            .collect();
        s.push_str(&row.join(" "));
        s.push('\n');
    }
    s.into_bytes()
}

const TEMPLATES_JSON: &str = r#"{
  "format": 1,
  "templates": [
    {
      "id": "holotape-c",
      "title": "Holotape in C",
      "description": "A holotape in C that draws one screen and waits for SELECT.",
      "kind": "holotape",
      "files": [
        {"path": "icon.pbm", "substitute": false},
        {"path": "robco.toml", "substitute": true},
        {"path": "src/main.c", "substitute": false},
        {"path": "tests/golden/start/ready.png", "substitute": false},
        {"path": "tests/start.toml", "substitute": false}
      ]
    }
  ]
}
"#;

/// The five files of `holotape-c`, by their path in the template.
fn template_files() -> Vec<(String, Vec<u8>)> {
    vec![
        ("icon.pbm".into(), icon()),
        ("robco.toml".into(), ROBCO_TOML.into()),
        ("src/main.c".into(), MAIN_C.into()),
        ("tests/golden/start/ready.png".into(), READY_PNG.into()),
        ("tests/start.toml".into(), START_TOML.into()),
    ]
}

/// Every file of the package but `sdk.json`, sorted by path.
fn package_files(templates_json: &str) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = template_files()
        .into_iter()
        .map(|(p, b)| (format!("holotape-c/{p}"), b))
        .collect();
    v.push(("LICENSES/MIT.txt".into(), b"MIT licence text\n".to_vec()));
    v.push(("templates.json".into(), templates_json.into()));
    v.sort();
    v
}

fn sdk_json(files: &[(String, Vec<u8>)]) -> Value {
    let files: Vec<Value> = files
        .iter()
        .map(|(p, b)| json!({"path": p, "size": b.len(), "sha256": sha256_hex(b)}))
        .collect();
    json!({
        "format": 1,
        "name": "templates",
        "version": "0.1.0",
        "built_from": [{"repo": "RobCo-Reel", "commit": "3333333333333333333333333333333333333333"}],
        "formats": {"templates": 1, "project": 1, "scenario": 1},
        "requires": [],
        "licences": [{"paths": [""], "spdx": "MIT"}],
        "files": files
    })
}

fn package_zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = SimpleFileOptions::default();
    z.start_file("sdk.json", o).unwrap();
    z.write_all(sdk_json(files).to_string().as_bytes()).unwrap();
    for (p, b) in files {
        z.start_file(p.as_str(), o).unwrap();
        z.write_all(b).unwrap();
    }
    z.finish().unwrap().into_inner()
}

/// A test folder: `cache/`, `work/` where `robco new` runs, and an index
/// with its archives. Removed on drop.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "robco-new-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("work")).unwrap();
        Self { dir }
    }

    fn put(&self, rel: &str, bytes: &[u8]) {
        let p = self.dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    fn work(&self) -> PathBuf {
        self.dir.join("work")
    }

    fn cache(&self) -> PathBuf {
        self.dir.join("cache")
    }

    fn pkg(&self) -> PathBuf {
        self.cache().join("packages/templates/0.1.0")
    }

    /// Puts an installed package into the cache as an install leaves it,
    /// with `templates.json` as given and the record of `.robco-archive`.
    fn installed(&self, templates_json: &str) {
        let files = package_files(templates_json);
        let pkg = self.pkg();
        for (p, b) in &files {
            let at = pkg.join(p);
            fs::create_dir_all(at.parent().unwrap()).unwrap();
            fs::write(at, b).unwrap();
        }
        fs::write(pkg.join("sdk.json"), sdk_json(&files).to_string()).unwrap();
        fs::write(pkg.join(".robco-archive"), format!("{}\n", "a".repeat(64))).unwrap();
    }

    /// The installed package of the spec.
    fn good(&self) {
        self.installed(TEMPLATES_JSON);
    }

    /// An index that lists the package zip `zip`.
    fn index(&self, zip: &[u8]) {
        self.put("archives/templates.zip", zip);
        let sdk = sdk_json_of(zip);
        let index = json!({
            "format": 1,
            "packages": [{
                "name": "templates",
                "version": sdk["version"],
                "requires": [],
                "formats": sdk["formats"],
                "url": "archives/templates.zip",
                "size": zip.len(),
                "sha256": sha256_hex(zip)
            }],
            "tools": []
        });
        self.put("index.json", index.to_string().as_bytes());
    }

    /// Runs `robco` in `work/` with only the variables it needs.
    fn robco_in(&self, cwd: &Path, args: &[&str]) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_robco"));
        c.args(args)
            .current_dir(cwd)
            .env_clear()
            .env("ROBCO_CACHE", self.cache());
        for key in ["SYSTEMROOT", "TEMP", "TMP"] {
            if let Some(v) = std::env::var_os(key) {
                c.env(key, v);
            }
        }
        c.output().expect("robco runs")
    }

    fn robco(&self, args: &[&str]) -> Output {
        self.robco_in(&self.work(), args)
    }

    /// The names in `work/`, sorted.
    fn in_work(&self) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(self.work())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        v.sort();
        v
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn sdk_json_of(zip: &[u8]) -> Value {
    let mut z = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
    let mut s = String::new();
    z.by_name("sdk.json")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    serde_json::from_str(&s).unwrap()
}

fn lines(o: &Output) -> Vec<Value> {
    String::from_utf8(o.stdout.clone())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// The `error` line's code and detail, and the exit code.
fn failure(o: &Output) -> (String, String, i32) {
    let l = lines(o);
    let err = l
        .iter()
        .find(|v| v["kind"] == "error")
        .unwrap_or_else(|| panic!("no error line: {l:?}"));
    (
        err["code"].as_str().unwrap().to_owned(),
        err["detail"].as_str().unwrap().to_owned(),
        o.status.code().unwrap(),
    )
}

/// Runs `new holotape-c NAME --json` and checks it failed with `code` and
/// `exit` and wrote nothing in `work/`.
fn refused(f: &Fixture, template: &str, name: &str, code: &str, exit: i32) -> String {
    let before = f.in_work();
    let o = f.robco(&["new", template, name, "--json"]);
    let (c, detail, e) = failure(&o);
    assert_eq!((c.as_str(), e), (code, exit), "{detail}");
    assert_eq!(f.in_work(), before, "the command wrote something");
    assert!(o.stderr.is_empty());
    detail
}

fn check_project(dir: &Path, name: &str) {
    for (p, b) in template_files() {
        let got = fs::read(dir.join(&p)).unwrap();
        let want = if p == "robco.toml" {
            ROBCO_TOML.replace("@NAME@", name).into_bytes()
        } else {
            b
        };
        assert_eq!(got, want, "{p}");
    }
    let m = studio_core::manifest::parse(&fs::read(dir.join("robco.toml")).unwrap()).unwrap();
    assert_eq!(m.project.name, name);
    assert_eq!(m.project.title, name);
}

#[test]
fn new_holotape_c_writes_a_project_whose_manifest_parses() {
    let f = Fixture::new("ok");
    f.index(&package_zip(&package_files(TEMPLATES_JSON)));
    let o = f.robco(&["sdk", "install", "templates", "--index", "../index.json"]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );

    let o = f.robco(&["new", "holotape-c", "PIPES", "--json"]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    assert!(o.stderr.is_empty());
    assert_eq!(
        lines(&o),
        vec![json!({
            "v": 1, "kind": "result", "command": "new", "ok": true, "exit": 0,
            "template": "holotape-c", "name": "PIPES", "path": "PIPES",
            "files": ["icon.pbm", "robco.toml", "src/main.c",
                      "tests/golden/start/ready.png", "tests/start.toml"]
        })]
    );
    // Only the project: no temporary folder, no lock, no build/.
    assert_eq!(f.in_work(), vec!["PIPES".to_owned()]);
    let p = f.work().join("PIPES");
    check_project(&p, "PIPES");
    assert!(!p.join("robco.lock").exists() && !p.join("build").exists());

    // A second run finds the folder full.
    refused(&f, "holotape-c", "PIPES", "new: exists", 3);
}

#[test]
fn new_an_empty_folder_is_filled() {
    let f = Fixture::new("empty");
    f.good();
    fs::create_dir(f.work().join("P_1-2")).unwrap();
    let o = f.robco(&["new", "holotape-c", "P_1-2"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stderr.is_empty());
    assert!(!o.stdout.is_empty(), "a line for a person");
    check_project(&f.work().join("P_1-2"), "P_1-2");
    assert_eq!(f.in_work(), vec!["P_1-2".to_owned()]);
}

#[test]
fn new_a_bad_name_fails_with_one_line_and_writes_nothing() {
    let f = Fixture::new("name");
    f.good();
    for name in [
        "pipes",
        "Pipes",
        "TOOLONGNM",
        "PI.PES",
        "PI~1",
        "PI PES",
        "É",
    ] {
        let d = refused(&f, "holotape-c", name, "new: name", 3);
        assert!(!d.is_empty());
    }
    // Without --json: one line on standard error, nothing on standard
    // output.
    let o = f.robco(&["new", "holotape-c", "pipes"]);
    assert_eq!(o.status.code(), Some(3));
    assert!(o.stdout.is_empty());
    let e = String::from_utf8(o.stderr).unwrap();
    assert_eq!(e.lines().count(), 1, "{e}");
    assert!(e.starts_with("new: name: "), "{e}");
}

#[test]
fn new_a_folder_that_is_not_empty_fails_and_is_kept() {
    let f = Fixture::new("exists");
    f.good();
    f.put("work/PIPES/mine.c", b"mine");
    refused(&f, "holotape-c", "PIPES", "new: exists", 3);
    assert_eq!(fs::read(f.work().join("PIPES/mine.c")).unwrap(), b"mine");
    // A file of that name is no folder.
    f.put("work/FILE", b"x");
    refused(&f, "holotape-c", "FILE", "new: exists", 3);
}

#[test]
fn new_an_unknown_template_fails_and_lists_the_ids() {
    let f = Fixture::new("template");
    f.good();
    let d = refused(&f, "holotape-rust", "PIPES", "new: template", 3);
    assert!(d.contains("holotape-c"), "{d}");
}

#[test]
fn new_no_package_is_templates_none() {
    let f = Fixture::new("none");
    let d = refused(&f, "holotape-c", "PIPES", "templates: none", 4);
    assert!(d.ends_with("run robco sdk install templates"), "{d}");
    // A folder whose install did not end holds no package.
    f.good();
    fs::remove_file(f.pkg().join(".robco-archive")).unwrap();
    refused(&f, "holotape-c", "PIPES", "templates: none", 4);
}

#[test]
fn new_the_checks_come_in_the_order_of_the_spec() {
    let f = Fixture::new("order");
    // 1 before 2: a bad name and no package.
    refused(&f, "holotape-c", "pipes", "new: name", 3);
    f.installed(&TEMPLATES_JSON.replace("\"format\": 1", "\"format\": 2"));
    f.put("work/PIPES/x", b"x");
    // 4 before 5 and 7.
    refused(&f, "nope", "PIPES", "templates: format", 4);
    // 5 before 7.
    let f = Fixture::new("order2");
    f.good();
    f.put("work/PIPES/x", b"x");
    refused(&f, "nope", "PIPES", "new: template", 3);
    // 6 before 7.
    fs::write(
        f.pkg().join("holotape-c/robco.toml"),
        ROBCO_TOML.replace("title = \"@NAME@\"", "title = \"@TITLE@\""),
    )
    .unwrap();
    let d = refused(&f, "holotape-c", "PIPES", "templates: token", 4);
    assert!(d.contains("robco.toml") && d.contains("@TITLE@"), "{d}");
}

#[test]
fn new_a_bad_sdk_json_is_a_package_error() {
    let f = Fixture::new("sdk");
    f.good();
    let sdk = f.pkg().join("sdk.json");
    let good = fs::read_to_string(&sdk).unwrap();
    fs::write(&sdk, "not json").unwrap();
    refused(&f, "holotape-c", "PIPES", "package: manifest", 4);
    fs::write(&sdk, good.replace("\"scenario\":1", "\"scenario\":2")).unwrap();
    refused(&f, "holotape-c", "PIPES", "package: format", 4);
    fs::write(
        &sdk,
        good.replace("\"name\":\"templates\"", "\"name\":\"pipos\""),
    )
    .unwrap();
    refused(&f, "holotape-c", "PIPES", "package: manifest", 4);
    fs::remove_file(&sdk).unwrap();
    refused(&f, "holotape-c", "PIPES", "package: manifest", 4);
}

#[test]
fn new_each_bad_templates_json_is_refused() {
    let bad_paths = [
        "../../../escape",
        "/etc/passwd",
        "src/../../x",
        "C:/x",
        "a\\\\b",
        "-rf",
        "src/x:y.c",
        "NUL",
        "src/con.c",
        "",
        "src//x",
        "src/@NAME@.c",
    ];
    let mut cases: Vec<(String, &str)> = vec![
        ("[]".to_owned(), "templates: manifest"),
        ("{".to_owned(), "templates: manifest"),
        (TEMPLATES_JSON.replace("\"format\": 1", "\"format\": 0"), "templates: format"),
        (TEMPLATES_JSON.replace("\"format\": 1,", ""), "templates: format"),
        (TEMPLATES_JSON.replace("\"format\": 1", "\"format\": 2"), "templates: format"),
        (TEMPLATES_JSON.replace("\"format\": 1,", "\"format\": 1, \"x\": 1,"), "templates: manifest"),
        (TEMPLATES_JSON.replace("\"kind\": \"holotape\"", "\"kind\": \"rom\""), "templates: manifest"),
        (TEMPLATES_JSON.replace("\"kind\": \"holotape\",", "\"kind\": \"holotape\", \"extra\": 1,"), "templates: manifest"),
        (TEMPLATES_JSON.replace("\"Holotape in C\"", "\"\""), "templates: manifest"),
        (TEMPLATES_JSON.replace("\"Holotape in C\"", "1"), "templates: manifest"),
        (TEMPLATES_JSON.replace("\"id\": \"holotape-c\"", "\"id\": \"Holotape-C\""), "templates: manifest"),
        (TEMPLATES_JSON.replace("\"substitute\": true", "\"substitute\": \"yes\""), "templates: manifest"),
        // Not sorted.
        (
            TEMPLATES_JSON.replace(
                "{\"path\": \"icon.pbm\", \"substitute\": false},\n        {\"path\": \"robco.toml\", \"substitute\": true},",
                "{\"path\": \"robco.toml\", \"substitute\": true},\n        {\"path\": \"icon.pbm\", \"substitute\": false},",
            ),
            "templates: manifest",
        ),
        // Listed and not held.
        (
            TEMPLATES_JSON.replace(
                "{\"path\": \"tests/start.toml\", \"substitute\": false}",
                "{\"path\": \"tests/start.toml\", \"substitute\": false},\n        {\"path\": \"zz.c\", \"substitute\": false}",
            ),
            "templates: manifest",
        ),
        // Held and not listed.
        (
            TEMPLATES_JSON.replace(
                "{\"path\": \"icon.pbm\", \"substitute\": false},\n",
                "",
            ),
            "templates: manifest",
        ),
        // Two paths that differ only in case.
        (
            TEMPLATES_JSON.replace(
                "{\"path\": \"icon.pbm\", \"substitute\": false},",
                "{\"path\": \"ICON.PBM\", \"substitute\": false},\n        {\"path\": \"icon.pbm\", \"substitute\": false},",
            ),
            "templates: manifest",
        ),
    ];
    for p in bad_paths {
        cases.push((
            TEMPLATES_JSON.replace(
                "{\"path\": \"icon.pbm\", \"substitute\": false},",
                &format!("{{\"path\": \"{p}\", \"substitute\": false}},\n        {{\"path\": \"icon.pbm\", \"substitute\": false}},"),
            ),
            "templates: manifest",
        ));
    }
    for (i, (text, code)) in cases.iter().enumerate() {
        let f = Fixture::new(&format!("tj{i}"));
        f.installed(text);
        let before = fs::read_dir(f.dir.clone()).unwrap().count();
        let d = refused(&f, "holotape-c", "PIPES", code, 4);
        assert!(!d.is_empty(), "case {i}");
        assert_eq!(fs::read_dir(f.dir.clone()).unwrap().count(), before);
        assert!(!f.dir.join("escape").exists() && !f.dir.join("x").exists());
    }
    // No templates.json.
    let f = Fixture::new("tj-missing");
    f.good();
    fs::remove_file(f.pkg().join("templates.json")).unwrap();
    refused(&f, "holotape-c", "PIPES", "templates: manifest", 4);
}

#[test]
fn new_a_substitute_file_keeps_its_line_ends() {
    let f = Fixture::new("crlf");
    f.good();
    let crlf = ROBCO_TOML.replace('\n', "\r\n");
    fs::write(f.pkg().join("holotape-c/robco.toml"), &crlf).unwrap();
    let o = f.robco(&["new", "holotape-c", "PIPES"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(f.work().join("PIPES/robco.toml")).unwrap(),
        crlf.replace("@NAME@", "PIPES")
    );
    // A non-substitute file keeps even a token as it is.
    assert_eq!(
        fs::read(f.work().join("PIPES/tests/golden/start/ready.png")).unwrap(),
        READY_PNG
    );
}

#[cfg(unix)]
#[test]
fn new_a_link_in_the_package_is_refused() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new("link");
    f.good();
    f.put("outside/secret", b"secret");
    let file = f.pkg().join("holotape-c/src/main.c");
    fs::remove_file(&file).unwrap();
    symlink(f.dir.join("outside/secret"), &file).unwrap();
    refused(&f, "holotape-c", "PIPES", "templates: manifest", 4);
    // A folder that is a link.
    let f = Fixture::new("link-dir");
    f.good();
    f.put("outside/main.c", MAIN_C.as_bytes());
    let src = f.pkg().join("holotape-c/src");
    fs::remove_dir_all(&src).unwrap();
    symlink(f.dir.join("outside"), &src).unwrap();
    refused(&f, "holotape-c", "PIPES", "templates: manifest", 4);
    // templates.json as a link.
    let f = Fixture::new("link-list");
    f.good();
    let list = f.pkg().join("templates.json");
    f.put("outside/templates.json", TEMPLATES_JSON.as_bytes());
    fs::remove_file(&list).unwrap();
    symlink(f.dir.join("outside/templates.json"), &list).unwrap();
    refused(&f, "holotape-c", "PIPES", "templates: manifest", 4);
}

#[cfg(unix)]
#[test]
fn new_a_target_that_is_a_link_is_not_written_through() {
    let f = Fixture::new("target-link");
    f.good();
    fs::create_dir_all(f.dir.join("outside")).unwrap();
    std::os::unix::fs::symlink(f.dir.join("outside"), f.work().join("PIPES")).unwrap();
    refused(&f, "holotape-c", "PIPES", "new: exists", 3);
    assert_eq!(fs::read_dir(f.dir.join("outside")).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn new_a_folder_the_host_will_not_write_is_io_write_and_leaves_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new("readonly");
    f.good();
    let w = f.work();
    fs::set_permissions(&w, fs::Permissions::from_mode(0o555)).unwrap();
    let writable = fs::create_dir(w.join("probe")).is_ok();
    let o = f.robco(&["new", "holotape-c", "PIPES", "--json"]);
    let _ = fs::remove_dir(w.join("probe"));
    fs::set_permissions(&w, fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        println!("skipped: this process writes in a read-only folder");
        return;
    }
    let (code, _, exit) = failure(&o);
    assert_eq!((code.as_str(), exit), ("io: write", 9));
    assert!(f.in_work().is_empty());
}

#[cfg(unix)]
#[test]
fn new_a_package_file_the_host_will_not_read_is_io_read() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new("partway");
    f.good();
    let png = f.pkg().join("holotape-c/tests/golden/start/ready.png");
    fs::set_permissions(&png, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::File::open(&png).is_ok();
    let o = f.robco(&["new", "holotape-c", "PIPES", "--json"]);
    fs::set_permissions(&png, fs::Permissions::from_mode(0o644)).unwrap();
    if readable {
        println!("skipped: this process reads a file without read permission");
        return;
    }
    // A file the host will not read is io: read, before anything is
    // written.
    let (code, detail, exit) = failure(&o);
    assert_eq!((code.as_str(), exit), ("io: read", 9), "{detail}");
    assert!(f.in_work().is_empty());
}

#[test]
fn new_a_real_templates_package_writes_a_project() {
    let Some(zip) = std::env::var_os("ROBCO_TEST_TEMPLATES") else {
        eprintln!("skipped: no ROBCO_TEST_TEMPLATES");
        return;
    };
    let f = Fixture::new("real");
    f.index(&fs::read(zip).unwrap());
    let o = f.robco(&["sdk", "install", "templates", "--index", "../index.json"]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let o = f.robco(&["new", "holotape-c", "PIPES", "--json"]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let toml = fs::read(f.work().join("PIPES/robco.toml")).unwrap();
    assert_eq!(toml, ROBCO_TOML.replace("@NAME@", "PIPES").into_bytes());
    studio_core::manifest::parse(&toml).unwrap();
}

/// Writes an index of the `pipos` package, the gcc archive and an empty
/// `qemu` archive, as `tests/build.rs` does.
fn pipos_index(f: &Fixture, pkg: &[u8], gcc: &[u8], gcc_root: &str) {
    let host = studio_core::resolve::running_host().expect("a host the index names");
    let sdk = sdk_json_of(pkg);
    let tool = |n: usize| sdk["tools"][n]["version"].as_str().unwrap().to_owned();
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.add_directory("bin/", SimpleFileOptions::default())
        .unwrap();
    let qemu = z.finish().unwrap().into_inner();
    f.put("archives/pipos.zip", pkg);
    f.put("archives/gcc.archive", gcc);
    f.put("archives/qemu.zip", &qemu);
    let archive = |url: &str, b: &[u8], root: &str| json!({host: {"url": url, "size": b.len(), "sha256": sha256_hex(b), "root": root}});
    let index = json!({
        "format": 1,
        "packages": [{
            "name": sdk["name"], "version": sdk["version"], "abi": sdk["abi"],
            "requires": sdk["requires"], "tools": sdk["tools"], "formats": sdk["formats"],
            "url": "archives/pipos.zip", "size": pkg.len(), "sha256": sha256_hex(pkg)
        }],
        "tools": [
            {"name": "gcc", "version": tool(0), "archives": archive("archives/gcc.archive", gcc, gcc_root)},
            {"name": "qemu", "version": tool(1), "archives": archive("archives/qemu.zip", &qemu, "")}
        ]
    });
    f.put("index.json", index.to_string().as_bytes());
}

#[test]
fn new_the_new_project_builds() {
    let (Some(pkg), Some(gcc)) = (
        std::env::var_os("ROBCO_TEST_PIPOS"),
        std::env::var_os("ROBCO_TEST_GCC"),
    ) else {
        assert!(
            std::env::var("ROBCO_TEST_REQUIRE_BUILD").as_deref() != Ok("1"),
            "ROBCO_TEST_REQUIRE_BUILD is 1 and ROBCO_TEST_PIPOS or ROBCO_TEST_GCC is not set"
        );
        eprintln!("skipped: no ROBCO_TEST_PIPOS or ROBCO_TEST_GCC");
        return;
    };
    let root = std::env::var("ROBCO_TEST_GCC_ROOT").unwrap_or_default();
    let f = Fixture::new("builds");
    // The real templates package when there is one, else the test's.
    match std::env::var_os("ROBCO_TEST_TEMPLATES") {
        Some(t) => {
            let zip = fs::read(t).unwrap();
            let files = sdk_json_of(&zip);
            assert_eq!(files["name"], "templates");
            let mut z = zip::ZipArchive::new(Cursor::new(&zip)).unwrap();
            z.extract(f.pkg()).unwrap();
            fs::write(
                f.pkg().join(".robco-archive"),
                format!("{}\n", sha256_hex(&zip)),
            )
            .unwrap();
        }
        None => f.good(),
    }
    let o = f.robco(&["new", "holotape-c", "PIPES"]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    pipos_index(&f, &fs::read(pkg).unwrap(), &fs::read(gcc).unwrap(), &root);
    let o = f.robco_in(
        &f.work().join("PIPES"),
        &["build", "--index", "../../index.json", "--json"],
    );
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let tape = fs::read(f.work().join("PIPES/build/PIPES.HTP")).unwrap();
    assert_eq!(&tape[0x20..0x28], b"ROBCO78\0");
}
