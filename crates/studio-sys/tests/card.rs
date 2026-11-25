// SPDX-License-Identifier: GPL-3.0-or-later

//! The card test: a partition table and a FAT16 volume written from Rust
//! through RobCo-Core's block and file-system API. The image must be the
//! same bytes on every system. `STUDIO_SYS_CARD_OUT` names a file to save
//! it to, with the expected tree beside it, for `scripts/check_card.sh`.

use studio_sys::{Entry, build_card, read_back};

/// The first 16 hex digits of the card's SHA-256, the same on Ubuntu,
/// macOS and Windows.
const CARD_SHA256_PREFIX: &str = "ee100f0f93696cff";

/// Deterministic bytes.
fn pattern(len: usize, seed: u32) -> Vec<u8> {
    (0..len as u32)
        .map(|i| ((seed.wrapping_add(i).wrapping_mul(2_654_435_761)) >> 24) as u8)
        .collect()
}

/// SHA-256, FIPS 180-4, small and slow, for the test only.
fn sha256(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, c) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*c);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[test]
fn sha256_known_vectors() {
    assert_eq!(
        sha256(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn card_written_from_rust() {
    let big = pattern(70_000, 7);
    let hello = b"Hello from Rust, written by RobCo-Core's FatFs.\n".to_vec();
    let cfg = b"[k3]\nmarker = card-from-rust\n".to_vec();
    let entries = [
        Entry {
            path: "K3",
            data: None,
        },
        Entry {
            path: "K3/SUB",
            data: None,
        },
        Entry {
            path: "K3/HELLO.TXT",
            data: Some(&hello),
        },
        Entry {
            path: "K3/SUB/BIG.BIN",
            data: Some(&big),
        },
        Entry {
            path: "K3.CFG",
            data: Some(&cfg),
        },
        Entry {
            path: "Long name from Rust.txt",
            data: Some(b"long file name\n"),
        },
    ];
    let mut img = build_card(16, &entries).unwrap();
    let hash = sha256(&img);
    println!("card SHA-256 {hash}");
    assert!(
        hash.starts_with(CARD_SHA256_PREFIX),
        "card SHA-256 {hash}, want {CARD_SHA256_PREFIX}..."
    );
    assert_eq!(read_back(&mut img, "K3/SUB/BIG.BIN").unwrap(), big);
    assert_eq!(read_back(&mut img, "K3/HELLO.TXT").unwrap(), hello);
    assert_eq!(read_back(&mut img, "K3.CFG").unwrap(), cfg);
    assert_eq!(
        read_back(&mut img, "NOPE.TXT").unwrap_err().code,
        -6,
        "FS_ERR_NOENT"
    );

    let Some(out) = std::env::var_os("STUDIO_SYS_CARD_OUT").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::write(&out, &img).unwrap();
    println!("card written to {}", out.display());
    // The expected tree beside it: <out without .img>.tree/<path>.
    let tree = out.with_extension("tree");
    let _ = std::fs::remove_dir_all(&tree);
    for e in &entries {
        let p = tree.join(e.path);
        match e.data {
            None => std::fs::create_dir_all(&p).unwrap(),
            Some(d) => std::fs::write(&p, d).unwrap(),
        }
    }
}

#[test]
fn bad_card_sizes_are_refused() {
    assert!(build_card(0, &[]).is_err());
    assert!(build_card(1, &[]).is_err());
}
