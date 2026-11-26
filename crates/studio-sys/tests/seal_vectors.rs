// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs every vector of RobCo-Core's `tests/vectors/rimg/vectors.ini`
//! through the seal and pack wrappers. Each one must give the bytes of its
//! output file, or fail with its code and hand back no bytes. The file is
//! read with the library's own INI parser, as RobCo-Core's
//! `tests/host/test_rimg_seal.c` does.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test file; a failed unwrap is a failed test"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use studio_sys::{
    Error, Icon, IconFormat, PackOpts, SealOpts, ini_parse, rimg_pack, rimg_seal, rimg_validate,
};

fn vec_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../external/robco-core/tests/vectors/rimg")
}

fn read(name: &str) -> Vec<u8> {
    let p = vec_dir().join(name);
    fs::read(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

/// A C literal: an optional '-', then decimal or 0x hex.
fn number(s: &str) -> i64 {
    let (neg, s) = s.strip_prefix('-').map_or((false, s), |r| (true, r));
    let v = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => i64::from_str_radix(h, 16),
        None => s.parse(),
    }
    .unwrap_or_else(|e| panic!("bad number {s}: {e}"));
    if neg { -v } else { v }
}

/// X.Y.Z or MAJOR.MINOR, parsed as the caller of the C functions must.
fn version(s: &str, parts: usize) -> u32 {
    let n: Vec<u32> = s.split('.').map(|p| p.parse().unwrap()).collect();
    assert_eq!(n.len(), parts, "version {s}");
    if parts == 3 {
        (n[0] << 16) | (n[1] << 8) | n[2]
    } else {
        (n[0] << 16) | n[1]
    }
}

type Vector = BTreeMap<String, String>;

/// The vectors in file order, by the library's INI parser.
fn vectors(text: &str) -> Vec<(String, Vector)> {
    let mut out: Vec<(String, Vector)> = Vec::new();
    for e in ini_parse(text).unwrap() {
        let id = e
            .section
            .strip_prefix("vector ")
            .unwrap_or_else(|| panic!("line {}: section {} is not a vector", e.line, e.section));
        if out.last().is_none_or(|(last, _)| last != id) {
            out.push((id.to_string(), Vector::new()));
        }
        let v = &mut out.last_mut().unwrap().1;
        assert!(
            v.insert(e.key.clone(), e.value).is_none(),
            "line {}: repeated key {}",
            e.line,
            e.key
        );
    }
    out
}

fn run(id: &str, v: &Vector) -> Result<Vec<u8>, Error> {
    let get = |k: &str| v.get(k).map(String::as_str);
    let input = read(get("input").unwrap());
    let icon_bytes = get("icon").map(read);
    let icon = icon_bytes.as_deref().map(|bytes| Icon {
        bytes,
        format: match get("icon_format") {
            None | Some("auto") => IconFormat::Auto,
            Some("raw") => IconFormat::Raw,
            Some("pbm") => IconFormat::Pbm,
            Some(f) => panic!("{id}: icon_format {f}"),
        },
    });
    let seal = SealOpts {
        name: get("name"),
        image_version: get("image_version").map(|s| version(s, 3)),
        title: get("title"),
        icon,
    };
    match get("op") {
        Some("seal") => rimg_seal(&input, &seal),
        Some("pack") => rimg_pack(
            &input,
            &PackOpts {
                seal,
                magic: get("magic").unwrap_or(""),
                subtype: get("subtype").map_or(0, number),
                load_addr: get("load_addr").map_or(0, number),
                mem_size: get("mem_size").map(number),
                abi_version: get("abi_version").map(|s| version(s, 2)),
                meta: get("meta").map(|m| m == "yes"),
                entry: get("entry").map_or(0, number),
            },
        ),
        op => panic!("{id}: op {op:?}"),
    }
}

#[test]
fn every_vector() {
    let text = String::from_utf8(read("vectors.ini")).unwrap();
    let in_file = text
        .lines()
        .filter(|l| l.trim_start().starts_with('['))
        .count();
    let vecs = vectors(&text);
    assert!(in_file > 0, "vectors.ini holds no vector");
    assert_eq!(
        vecs.len(),
        in_file,
        "a section was read twice or not at all"
    );

    let (mut ran, mut seal, mut pack, mut failed) = (0, 0, 0, Vec::new());
    for (id, v) in &vecs {
        match v.get("op").map(String::as_str) {
            Some("seal") => seal += 1,
            Some("pack") => pack += 1,
            _ => {}
        }
        let expect = v.get("expect").map(String::as_str).unwrap_or("");
        let got = run(id, v);
        ran += 1;
        match (&got, expect) {
            (Ok(bytes), "ok") => {
                let want = read(v.get("output").unwrap());
                if *bytes != want {
                    failed.push(format!(
                        "{id}: {} bytes differ from the output",
                        bytes.len()
                    ));
                }
            }
            (Ok(_), e) => failed.push(format!("{id}: ok, want {e}")),
            (Err(e), want) if e.text == want => {
                assert!(e.code > 0, "{id}: a code from the C, not the wrapper");
                assert!(!v.contains_key("output"), "{id}: a failure has no output");
            }
            (Err(e), want) => failed.push(format!("{id}: {e}, want {want}")),
        }
    }
    assert!(
        failed.is_empty(),
        "{} vectors failed:\n{}",
        failed.len(),
        failed.join("\n")
    );
    assert_eq!(ran, in_file, "ran fewer vectors than vectors.ini holds");
    assert_eq!(seal + pack, ran);
    assert!(seal > 0 && pack > 0);
}

#[test]
fn pack_output_seals_and_validates() {
    let opts = PackOpts {
        magic: "ROBCO78",
        load_addr: 0x80_0000,
        ..PackOpts::default()
    };
    let img = rimg_pack(&[0u8; 32], &opts).unwrap();
    assert_eq!(img.len(), 32 + 0x120);
    rimg_validate(&img, &studio_sys::Expect::default()).unwrap();
    let sealed = rimg_seal(
        &img,
        &SealOpts {
            title: Some("T"),
            ..SealOpts::default()
        },
    )
    .unwrap();
    assert_eq!(sealed.len(), img.len());
}

#[test]
fn errors_keep_the_c_code() {
    let e = rimg_pack(
        b"abcd",
        &PackOpts {
            magic: "NOPE",
            ..PackOpts::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        (e.what.as_str(), e.code, e.text.as_str()),
        ("rimg_pack", 3, "magic")
    );
    let e = rimg_seal(&[0u8; 8], &SealOpts::default()).unwrap_err();
    assert_eq!((e.what.as_str(), e.code), ("rimg_seal", 2));
    let e = rimg_seal(
        &[0u8; 8],
        &SealOpts {
            name: Some("a\0b"),
            ..SealOpts::default()
        },
    )
    .unwrap_err();
    assert_eq!(e.code, -1);
}
