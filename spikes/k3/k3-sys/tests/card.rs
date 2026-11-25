//! The card test: a partition table and a FAT volume written from Rust
//! through RobCo-Core's block and file-system API, then saved to disk.
//! K3_CARD_OUT names the image file; scripts/check_card.sh reads it with
//! mtools and scripts/boot_card.py boots it.

use k3_sys::{Entry, build_card, read_back};

/// Deterministic bytes, so the shell check can rebuild them.
pub fn pattern(len: usize, seed: u32) -> Vec<u8> {
    (0..len as u32)
        .map(|i| ((seed.wrapping_add(i).wrapping_mul(2_654_435_761)) >> 24) as u8)
        .collect()
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
    assert_eq!(read_back(&mut img, "K3/SUB/BIG.BIN").unwrap(), big);
    assert_eq!(read_back(&mut img, "K3/HELLO.TXT").unwrap(), hello);

    let out = std::env::var_os("K3_CARD_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("k3-card.img")
        });
    std::fs::write(&out, &img).unwrap();
    println!("card written to {}", out.display());

    // The expected tree next to it, for the mtools check: <out>.tree/<path>.
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
