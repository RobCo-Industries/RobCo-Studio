// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of [`validate`]. Each package is a list of paths and bytes, so a
//! case is one line.

#![allow(
    clippy::unwrap_used,
    reason = "a test; a failed unwrap is a failed test"
)]

use super::*;
use crate::install::sha256_hex;
use crate::resolve::sdk_json;

/// The `templates.json` of the spec's example.
const SPEC_JSON: &str = r#"{
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
}"#;

/// A package: its files but `sdk.json`, its extra folders, and whether
/// `sdk.json` lists every file.
#[derive(Clone)]
struct Pkg {
    files: Vec<(String, Vec<u8>)>,
    folders: Vec<String>,
    /// Paths `sdk.json` leaves out, or lists without a file.
    unlisted: Vec<String>,
    ghost: Vec<String>,
    /// Text replacements in `sdk.json`.
    sdk_edits: Vec<(String, String)>,
}

impl Pkg {
    /// The spec's example with `json` as `templates.json`.
    fn spec_with(json: &str) -> Self {
        let mut files: Vec<(String, Vec<u8>)> = [
            "icon.pbm",
            "robco.toml",
            "src/main.c",
            "tests/golden/start/ready.png",
            "tests/start.toml",
        ]
        .iter()
        .map(|p| (format!("holotape-c/{p}"), b"x @NAME@\n".to_vec()))
        .collect();
        files.push(("LICENSES/MIT.txt".into(), b"MIT\n".to_vec()));
        files.push((TEMPLATES_LIST.into(), json.as_bytes().to_vec()));
        Self {
            files,
            folders: Vec::new(),
            unlisted: Vec::new(),
            ghost: Vec::new(),
            sdk_edits: Vec::new(),
        }
    }
    fn spec() -> Self {
        Self::spec_with(SPEC_JSON)
    }
    fn json(json: &str) -> Self {
        Self::spec_with(json)
    }
    /// The spec's example with `from` replaced by `to` in `templates.json`.
    fn edit(from: &str, to: &str) -> Self {
        assert!(SPEC_JSON.contains(from), "{from}");
        Self::spec_with(&SPEC_JSON.replace(from, to))
    }
    fn with(mut self, path: &str, bytes: &[u8]) -> Self {
        self.files.push((path.into(), bytes.to_vec()));
        self
    }
    fn without(mut self, path: &str) -> Self {
        self.files.retain(|(p, _)| p != path);
        self
    }
    fn folder(mut self, path: &str) -> Self {
        self.folders.push(path.into());
        self
    }
    fn not_in_sdk(mut self, path: &str) -> Self {
        self.unlisted.push(path.into());
        self
    }
    fn ghost_in_sdk(mut self, path: &str) -> Self {
        self.ghost.push(path.into());
        self
    }
    /// Replaces `from` with `to` in the text of `sdk.json`.
    fn sdk_edit(mut self, from: &str, to: &str) -> Self {
        self.sdk_edits.push((from.into(), to.into()));
        self
    }

    fn sdk(&self) -> SdkJson {
        self.try_sdk().unwrap()
    }

    fn try_sdk(&self) -> Result<SdkJson, Error> {
        let mut listed: Vec<String> = self
            .files
            .iter()
            .filter(|(p, _)| !self.unlisted.contains(p))
            .map(|(p, b)| {
                format!(
                    r#"{{"path": "{p}", "size": {}, "sha256": "{}"}}"#,
                    b.len(),
                    sha256_hex(b)
                )
            })
            .collect();
        for g in &self.ghost {
            listed.push(format!(
                r#"{{"path": "{g}", "size": 1, "sha256": "{}"}}"#,
                sha256_hex(b"g")
            ));
        }
        listed.sort();
        let mut text = format!(
            r#"{{"format": 1, "name": "templates", "version": "0.1.0",
  "built_from": [{{"repo": "RobCo-Reel", "commit": "3333333333333333333333333333333333333333"}}],
  "formats": {{"templates": 1, "project": 1, "scenario": 1}},
  "licences": [{{"paths": [""], "spdx": "MIT"}}],
  "files": [{}]}}"#,
            listed.join(", ")
        );
        for (from, to) in &self.sdk_edits {
            assert!(text.contains(from.as_str()), "{from}");
            text = text.replacen(from.as_str(), to, 1);
        }
        sdk_json::parse(text.as_bytes()).map_err(|e| Error {
            code: e.code(),
            detail: e.to_string(),
        })
    }

    fn entries(&self) -> Vec<Entry> {
        let mut v = vec![Entry::file(SDK_JSON, 100)];
        v.extend(
            self.files
                .iter()
                .map(|(p, b)| Entry::file(p.clone(), b.len() as u64)),
        );
        v.extend(self.folders.iter().map(|f| Entry::folder(f.clone())));
        v
    }

    fn check(&self, limits: &Limits) -> Result<Vec<Template>, Error> {
        let list = self
            .files
            .iter()
            .find(|(p, _)| p == TEMPLATES_LIST)
            .map(|(_, b)| b.as_slice());
        validate(&self.try_sdk()?, list, &self.entries(), limits)
    }
}

#[test]
fn the_spec_example_is_valid_and_typed() {
    let t = Pkg::spec().check(&Limits::DEFAULT).unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].id, "holotape-c");
    assert_eq!(t[0].title, "Holotape in C");
    let paths: Vec<(&str, bool, u64)> = t[0]
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.substitute, f.size))
        .collect();
    assert_eq!(paths[1], ("robco.toml", true, 9));
    assert_eq!(paths.len(), 5);
    // An archive may hold its folders as entries, and a cache does.
    let p = Pkg::spec()
        .folder("holotape-c")
        .folder("holotape-c/src")
        .folder("holotape-c/tests")
        .folder("holotape-c/tests/golden")
        .folder("holotape-c/tests/golden/start")
        .folder("LICENSES");
    assert!(p.check(&Limits::DEFAULT).is_ok());
}

#[test]
fn the_smallest_real_shape_is_valid() {
    let json = SPEC_JSON
        .replace("        {\"path\": \"icon.pbm\", \"substitute\": false},\n", "")
        .replace(",\n        {\"path\": \"src/main.c\", \"substitute\": false},\n        {\"path\": \"tests/golden/start/ready.png\", \"substitute\": false},\n        {\"path\": \"tests/start.toml\", \"substitute\": false}", "");
    let p = Pkg::json(&json)
        .without("holotape-c/icon.pbm")
        .without("holotape-c/src/main.c")
        .without("holotape-c/tests/golden/start/ready.png")
        .without("holotape-c/tests/start.toml");
    let t = p.check(&Limits::DEFAULT).unwrap();
    assert_eq!(t[0].files.len(), 1, "{json}");
}

/// Each way to break a rule, its code and a text the detail names. One
/// row per case.
#[rustfmt::skip]
fn bad_cases() -> Vec<(&'static str, Pkg, &'static str, &'static str)> {
    let m = "templates: manifest";
    let two = SPEC_JSON.replacen(
        "  \"templates\": [\n    {",
        "  \"templates\": [\n    {\"id\": \"holotape-b\", \"title\": \"B\", \"description\": \"B.\", \"kind\": \"holotape\", \"files\": [{\"path\": \"robco.toml\", \"substitute\": true}]},\n    {",
        1,
    );
    vec![
        ("id without folder", Pkg::json(&two), m, "holotape-b/"),
        ("listed file absent", Pkg::spec().without("holotape-c/icon.pbm"), m, "holotape-c/icon.pbm"),
        ("unlisted template file", Pkg::spec().with("holotape-c/x.c", b"x"), m, "holotape-c/x.c"),
        ("unlisted template folder", Pkg::spec().with("holotape-b/robco.toml", b"x"), m, "holotape-b"),
        ("empty folder in a template", Pkg::spec().folder("holotape-c/empty"), m, "holotape-c/empty"),
        ("extra root file", Pkg::spec().with("README.txt", b"x"), m, "README.txt"),
        ("extra root folder", Pkg::spec().folder("docs"), m, "docs"),
        ("extra licence", Pkg::spec().with("LICENSES/OTHER.txt", b"x"), m, "LICENSES/OTHER.txt"),
        ("no licence", Pkg::spec().without("LICENSES/MIT.txt"), m, "LICENSES/MIT.txt"),
        ("no templates.json", Pkg::spec().without(TEMPLATES_LIST), m, TEMPLATES_LIST),
        ("folders differ by case", Pkg::spec().with("holotape-c/Src/x.c", b"x").not_in_sdk("holotape-c/Src/x.c"), m, "only in case"),
        ("files differ by case", Pkg::edit("\"icon.pbm\"", "\"ROBCO.toml\"").without("holotape-c/icon.pbm").with("holotape-c/ROBCO.toml", b"x").not_in_sdk("holotape-c/ROBCO.toml"), m, "only in case"),
        ("reserved path", Pkg::edit("\"icon.pbm\"", "\"build/x\"").without("holotape-c/icon.pbm").with("holotape-c/build/x", b"x"), m, "build"),
        ("reserved lock", Pkg::edit("\"icon.pbm\"", "\"Robco.Lock\"").without("holotape-c/icon.pbm").with("holotape-c/Robco.Lock", b"x"), m, "robco.lock"),
        ("token in a path", Pkg::edit("\"icon.pbm\"", "\"@NAME@.pbm\""), m, "token"),
        ("device name", Pkg::edit("\"icon.pbm\"", "\"con.pbm\""), m, "device"),
        ("dot dot", Pkg::edit("\"icon.pbm\"", "\"../icon.pbm\""), m, "icon.pbm"),
        ("no robco.toml", Pkg::edit("\"robco.toml\"", "\"robco.tom\"").without("holotape-c/robco.toml").with("holotape-c/robco.tom", b"x"), m, "robco.toml"),
        ("files not sorted", Pkg::edit("\"icon.pbm\"", "\"z.pbm\""), m, "sorted"),
        ("unknown field", Pkg::edit("\"kind\": \"holotape\",", "\"kind\": \"holotape\", \"x\": 1,"), m, "x is not a field"),
        ("missing title", Pkg::edit("\"title\": \"Holotape in C\",", ""), m, "title"),
        ("bad id", Pkg::edit("\"id\": \"holotape-c\"", "\"id\": \"Holotape-c\""), m, "id"),
        ("other kind", Pkg::edit("\"kind\": \"holotape\"", "\"kind\": \"app\""), m, "kind"),
        ("empty list", Pkg::json("{\"format\": 1, \"templates\": []}"), m, "empty"),
        ("not JSON", Pkg::json("{"), m, "JSON"),
        ("format 2", Pkg::edit("\"format\": 1", "\"format\": 2"), "templates: format", "newer tool"),
        ("format 0", Pkg::edit("\"format\": 1", "\"format\": 0"), "templates: format", "positive"),
        ("ids not sorted", Pkg::json(&two.replace("holotape-b", "holotape-z")), m, "sorted"),
        ("not in sdk.json", Pkg::spec().not_in_sdk("holotape-c/icon.pbm"), "package: manifest", "holotape-c/icon.pbm"),
        ("ghost in sdk.json", Pkg::spec().ghost_in_sdk("LICENSES/X.txt"), "package: manifest", "LICENSES/X.txt"),
        // The property table of "The templates package", one row each.
        ("version not X.Y.Z", Pkg::spec().sdk_edit("\"0.1.0\"", "\"0.1\""), "package: manifest", "version"),
        ("abi present", Pkg::spec().sdk_edit("\"built_from\"", "\"abi\": \"0.1\", \"built_from\""), "package: manifest", "abi"),
        ("tools present", Pkg::spec().sdk_edit("\"built_from\"", "\"tools\": [], \"built_from\""), "package: manifest", "tools"),
        ("firmware present", Pkg::spec().sdk_edit("\"built_from\"", "\"firmware\": {}, \"built_from\""), "package: manifest", "firmware"),
        ("requires nonempty", Pkg::spec().sdk_edit("\"licences\"", "\"requires\": [{\"name\": \"pipos\", \"abi\": \"1.3\"}], \"licences\""), "package: manifest", "requires names pipos"),
        ("built_from other repo", Pkg::spec().sdk_edit("RobCo-Reel", "Pip-OS"), "package: manifest", "built_from"),
        ("built_from two repos", Pkg::spec().sdk_edit("\"built_from\": [", "\"built_from\": [{\"repo\": \"Pip-OS\", \"commit\": \"1111111111111111111111111111111111111111\"}, "), "package: manifest", "built_from"),
        ("built_from empty", Pkg::spec().sdk_edit("{\"repo\": \"RobCo-Reel\", \"commit\": \"3333333333333333333333333333333333333333\"}", ""), "package: manifest", "built_from"),
        ("formats lacks one", Pkg::spec().sdk_edit(", \"scenario\": 1", ""), "package: manifest", "formats.scenario"),
        ("formats of pipos", Pkg::spec().sdk_edit("\"scenario\": 1", "\"scenario\": 1, \"rimg\": 1"), "package: manifest", "formats.rimg"),
        ("formats unknown value", Pkg::spec().sdk_edit("\"project\": 1", "\"project\": 2"), "package: format", "project"),
        ("licence not MIT", Pkg::spec().sdk_edit("\"spdx\": \"MIT\"", "\"spdx\": \"GPL-3.0-only\""), "package: manifest", "GPL-3.0-only"),
        ("a second licence", Pkg::spec().sdk_edit("\"spdx\": \"MIT\"}", "\"spdx\": \"MIT\"}, {\"paths\": [\"holotape-c/src/\"], \"spdx\": \"Apache-2.0\"}"), "package: manifest", "Apache-2.0"),
        ("a path no licence covers", Pkg::spec().sdk_edit("\"paths\": [\"\"]", "\"paths\": [\"holotape-c\", \"LICENSES/\"]"), "package: manifest", "templates.json"),
        ("a prefix is no folder", Pkg::spec().sdk_edit("\"paths\": [\"\"]", "\"paths\": [\"holotape\", \"LICENSES\", \"templates.json\"]"), "package: manifest", "holotape-c/icon.pbm"),
        ("no licences", Pkg::spec().sdk_edit("{\"paths\": [\"\"], \"spdx\": \"MIT\"}", ""), "package: manifest", "LICENSES/MIT.txt"),
    ]
}

#[test]
fn every_broken_rule_is_refused_with_its_code_and_names_the_path() {
    for (tag, p, code, named) in bad_cases() {
        let e = p.check(&Limits::DEFAULT).unwrap_err();
        assert_eq!(e.code, code, "{tag}: {e}");
        assert!(e.detail.contains(named), "{tag}: {e}");
    }
}

/// The `sdk.json` RobCo-Reel's `tools/templates_package.py` writes: one
/// `RobCo-Reel` entry, the three formats, empty `requires`, one MIT entry
/// for path `""`. Licences that cover each file by its folders pass too.
#[test]
fn the_package_reel_builds_and_licences_by_folder_are_valid() {
    let reel = Pkg::spec().sdk_edit("\"licences\"", "\"requires\": [], \"licences\"");
    assert!(reel.check(&Limits::DEFAULT).is_ok());
    let by_folder = Pkg::spec().sdk_edit(
        "{\"paths\": [\"\"], \"spdx\": \"MIT\"}",
        "{\"paths\": [\"holotape-c/\", \"LICENSES\"], \"spdx\": \"MIT\"}, {\"paths\": [\"templates.json\"], \"spdx\": \"MIT\"}",
    );
    assert!(by_folder.check(&Limits::DEFAULT).is_ok());
}

#[test]
fn a_link_or_other_entry_is_refused() {
    let mut entries = Pkg::spec().entries();
    entries.push(Entry {
        path: "holotape-c/link".into(),
        kind: Kind::Other,
        size: 0,
    });
    let p = Pkg::spec();
    let e = validate(
        &p.sdk(),
        Some(SPEC_JSON.as_bytes()),
        &entries,
        &Limits::DEFAULT,
    )
    .unwrap_err();
    assert!(e.detail.contains("holotape-c/link"), "{e}");
}

#[test]
fn the_sdk_json_must_name_templates_and_keep_its_limit() {
    let p = Pkg::spec();
    let mut sdk = p.sdk();
    sdk.name = "pipos".into();
    let e = validate(
        &sdk,
        Some(SPEC_JSON.as_bytes()),
        &p.entries(),
        &Limits::DEFAULT,
    )
    .unwrap_err();
    assert_eq!(e.code, "package: manifest");
    let l = Limits {
        manifest: 99,
        ..Limits::DEFAULT
    };
    let e = p.check(&l).unwrap_err();
    assert_eq!(e.code, "package: manifest");
    assert!(e.detail.contains("99"), "{e}");
    assert!(p.check(&Limits { manifest: 100, ..l }).is_ok());
}

#[test]
fn each_limit_holds_at_its_value_and_refuses_one_below() {
    let p = Pkg::spec();
    let list = SPEC_JSON.len() as u64;
    let file = 9;
    let set = |which: usize, v: u64| {
        let mut l = Limits::DEFAULT;
        match which {
            0 => l.list = v,
            1 => l.file = v,
            2 => l.files = v,
            _ => l.bytes = v,
        }
        l
    };
    for (which, at) in [(0, list), (1, file), (2, 5), (3, 5 * file)] {
        assert!(p.check(&set(which, at)).is_ok(), "limit {which}");
        let e = p.check(&set(which, at - 1)).unwrap_err();
        assert_eq!(e.code, "templates: manifest", "limit {which}");
        assert!(e.detail.contains(&(at - 1).to_string()), "{e}");
    }
}

#[test]
fn template_paths_stay_one_plain_name_per_component() {
    for ok in [
        "robco.toml",
        "src/main.c",
        "tests/golden/start/ready.png",
        "a-b/c.d",
    ] {
        assert!(check_path("k", ok).is_ok(), "{ok}");
    }
    for bad in [
        "",
        "/etc/x",
        "../x",
        "a/../x",
        "a/./x",
        "a//x",
        "a\\x",
        "C:x",
        "c:/x",
        "a:b",
        "src/ab:c",
        "-rf",
        "src/-x.c",
        "con",
        "src/NUL.c",
        "Com1.txt",
        "lpt9",
        "a./b",
        "a /b",
        "a\u{1}b",
        "a*b",
        "a?b",
        "src/@NAME@.c",
    ] {
        assert_eq!(
            check_path("k", bad).unwrap_err().code,
            "templates: manifest",
            "{bad}"
        );
    }
}

#[test]
fn reserved_paths_are_refused_in_any_case() {
    for bad in [
        "robco.lock",
        "ROBCO.LOCK",
        "Robco.Lock/x",
        "build/x",
        "BUILD/gen/a.c",
        "build",
    ] {
        let e = check_not_reserved("k", bad).unwrap_err();
        assert!(e.detail.contains(bad), "{}", e.detail);
    }
    for ok in ["robco.toml", "builds/x", "src/build/x", "robco.lock.txt"] {
        assert!(check_not_reserved("k", ok).is_ok(), "{ok}");
    }
}

#[test]
fn template_ids_follow_their_rule() {
    for ok in ["holotape-c", "a", "a1-"] {
        assert!(is_template_id(ok), "{ok}");
    }
    for bad in ["", "1a", "-a", "Holotape", "a_b", &"a".repeat(33)] {
        assert!(!is_template_id(bad), "{bad}");
    }
}

#[test]
fn tokens_are_read_from_the_left() {
    let b = b"a @NAME@ x@y.z @@ @a@ @NAME @A_B@";
    let got: Vec<&[u8]> = tokens(b).into_iter().map(|r| &b[r]).collect();
    assert_eq!(got, vec![&b"@NAME@"[..], b"@A_B@"]);
}
