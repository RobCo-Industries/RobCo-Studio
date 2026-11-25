// SPDX-License-Identifier: GPL-3.0-or-later

//! CRC-32, INI, rimg and the weak time hook, through the safe wrappers.

use std::path::PathBuf;
use std::process::Command;

use studio_sys::{
    Expect, IniEntry, crc32, fs_time, ini_get_int, ini_parse, rimg_crc, rimg_validate,
};

#[test]
fn crc32_known_vector() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
}

#[test]
fn ini_parse_and_get() {
    let text = "top = 1\n[boot]\ndefault = UOS ; comment\ntimeout = 3\n\nnot a key\n";
    let err = ini_parse(text).unwrap_err();
    // "not a key" is neither a section nor a key: INI_ERR_SYNTAX on line 6.
    assert_eq!(err.error.code, -2);
    assert_eq!(err.line, 6);
    let got: Vec<_> = err
        .entries
        .iter()
        .map(|e| (e.section.as_str(), e.key.as_str(), e.value.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("", "top", "1"),
            ("boot", "default", "UOS"),
            ("boot", "timeout", "3")
        ]
    );

    let ok = ini_parse("[a]\nk = v\n").unwrap();
    assert_eq!(
        ok,
        [IniEntry {
            section: "a".into(),
            key: "k".into(),
            value: "v".into(),
            line: 2
        }]
    );

    assert_eq!(
        ini_get_int("[boot]\ntimeout = 0x10\n", "boot", "timeout", 0, 100),
        Ok(16)
    );
    // INI_ERR_RANGE and INI_ERR_NOT_FOUND.
    assert_eq!(
        ini_get_int("[boot]\ntimeout = 200\n", "boot", "timeout", 0, 100)
            .unwrap_err()
            .code,
        -8
    );
    assert_eq!(
        ini_get_int("[boot]\n", "boot", "timeout", 0, 100)
            .unwrap_err()
            .code,
        -5
    );
}

fn python() -> &'static str {
    for p in ["python3", "python"] {
        if Command::new(p)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            return p;
        }
    }
    "python3"
}

/// Packs and seals an image with RobCo-Core's own `tools/rimg.py`, then
/// checks it with `rimg_validate`.
#[test]
fn rimg_validates_sealed_image() {
    let core = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../external/robco-core");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let payload = dir.join("payload.bin");
    let img = dir.join("rom1.img");
    std::fs::write(&payload, b"payload bytes for the studio-sys rimg check\n").unwrap();
    let st = Command::new(python())
        .arg(core.join("tools/rimg.py"))
        .arg("pack")
        .arg(&payload)
        .arg("-o")
        .arg(&img)
        .args([
            "--magic",
            "ROM1",
            "--subtype",
            "0",
            "--load-addr",
            "0x400000",
        ])
        .args([
            "--name",
            "SYS",
            "--image-version",
            "1.0.0",
            "--title",
            "sys test",
        ])
        .status()
        .unwrap();
    assert!(st.success(), "rimg.py pack failed");
    let mut bytes = std::fs::read(&img).unwrap();
    let expect = Expect {
        magic: Some(*b"ROM1\0\0\0\0"),
        file_size: bytes.len() as u32,
        ..Expect::default()
    };
    let h = rimg_validate(&bytes, &expect).unwrap();
    let (load, crc) = (h.load_addr, h.crc32);
    assert_eq!(load, 0x40_0000);
    assert_eq!(Some(crc), rimg_crc(&bytes));

    let wrong = Expect {
        magic: Some(*b"BOOT\0\0\0\0"),
        ..expect
    };
    assert_eq!(
        rimg_validate(&bytes, &wrong).unwrap_err().code,
        3,
        "RIMG_E_MAGIC"
    );

    // One flipped payload byte must fail the CRC step.
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    assert_eq!(
        rimg_validate(&bytes, &expect).unwrap_err().code,
        8,
        "RIMG_E_CRC"
    );
    assert_eq!(rimg_crc(&bytes[..0x5F]), None);
}

#[test]
fn time_hook_links() {
    // The weak default in fs_glue.c: with no clock set it returns 2026-01-01 00:00.
    assert_eq!(fs_time(), (46 << 25) | (1 << 21) | (1 << 16));
}
