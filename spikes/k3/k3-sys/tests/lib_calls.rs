//! CRC, INI, rimg and layout checks against RobCo-Core's C.

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::mem::{align_of, offset_of, size_of};
use std::path::PathBuf;
use std::process::Command;

use k3_sys::*;

#[test]
fn crc32_known_vector() {
    let data = b"123456789";
    // SAFETY: data is a valid buffer of data.len() bytes.
    let got = unsafe { crc32(data.as_ptr().cast(), data.len()) };
    assert_eq!(got, 0xCBF4_3926);
}

unsafe extern "C" fn collect(
    section: *const c_char,
    key: *const c_char,
    value: *const c_char,
    _line: c_uint,
    ctx: *mut c_void,
) -> c_int {
    // SAFETY: ini_parse passes NUL-terminated strings and our ctx.
    unsafe {
        let out = &mut *ctx.cast::<Vec<(String, String, String)>>();
        let s = |p: *const c_char| CStr::from_ptr(p).to_string_lossy().into_owned();
        out.push((s(section), s(key), s(value)));
    }
    0
}

#[test]
fn ini_parse_and_get() {
    let text = "top = 1\n[boot]\ndefault = UOS ; comment\ntimeout = 3\n\nnot a key\n";
    let mut got: Vec<(String, String, String)> = Vec::new();
    let mut err_line = 0;
    // SAFETY: text and got outlive the call; collect matches the handler type.
    let rc = unsafe {
        ini_parse(
            text.as_ptr().cast(),
            text.len(),
            Some(collect),
            (&mut got as *mut Vec<_>).cast(),
            &mut err_line,
        )
    };
    // "not a key" is neither a section nor a key: rejected on line 6.
    assert_eq!(rc, -2, "INI_ERR_SYNTAX");
    assert_eq!(err_line, 6);
    let want = [
        ("", "top", "1"),
        ("boot", "default", "UOS"),
        ("boot", "timeout", "3"),
    ];
    let want: Vec<_> = want
        .iter()
        .map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
        .collect();
    assert_eq!(got, want);

    let ok = "[boot]\ntimeout = 0x10\n";
    let mut v = 0;
    // SAFETY: valid buffers and NUL-terminated names.
    let rc = unsafe {
        ini_get_int(
            ok.as_ptr().cast(),
            ok.len(),
            c"boot".as_ptr(),
            c"timeout".as_ptr(),
            0,
            100,
            &mut v,
        )
    };
    assert_eq!((rc, v), (0, 16));
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

/// Packs and seals an image with RobCo-Core's own tools/rimg.py, then
/// validates it with rimg_validate().
#[test]
fn rimg_validates_sealed_image() {
    let core = PathBuf::from(env!("ROBCO_CORE_DIR_AT_BUILD"));
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let payload = dir.join("payload.bin");
    let img = dir.join("rom1.img");
    std::fs::write(&payload, b"payload bytes for the k3 rimg check\n").unwrap();
    let st = Command::new(python())
        .arg(core.join("tools/rimg.py"))
        .args(["pack"])
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
            "--name",
            "K3",
        ])
        .args(["--image-version", "1.0.0", "--title", "K3 test"])
        .status()
        .unwrap();
    assert!(st.success(), "rimg.py pack failed");
    let mut bytes = std::fs::read(&img).unwrap();
    let expect = RimgExpect {
        magic: b"ROM1\0\0\0\0".as_ptr().cast(),
        slot_base: 0,
        slot_size: 0,
        abi_major: 0,
        file_size: bytes.len() as u32,
    };
    let mut hdr: *const RimgHeader = std::ptr::null();
    // SAFETY: bytes and expect outlive the call.
    let rc = unsafe { rimg_validate(bytes.as_ptr().cast(), bytes.len(), &expect, &mut hdr) };
    assert_eq!(rc, RIMG_OK);
    // SAFETY: on RIMG_OK hdr points into bytes, which is alive.
    let h = unsafe { std::ptr::read_unaligned(hdr) };
    let (load, crc) = (h.load_addr, h.crc32);
    assert_eq!(load, 0x40_0000);
    // SAFETY: bytes holds file_size bytes.
    assert_eq!(crc, unsafe {
        rimg_crc(bytes.as_ptr().cast(), bytes.len() as u32)
    });
    // One flipped payload byte must fail the CRC step.
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    // SAFETY: as above.
    let rc = unsafe { rimg_validate(bytes.as_ptr().cast(), bytes.len(), &expect, &mut hdr) };
    assert_eq!(rc, 8, "RIMG_E_CRC");
}

macro_rules! rust_layout {
    ($v:ident; $t:ident as $c:literal { $($f:ident as $cf:literal),* $(,)? }) => {{
        $v.push(($c.to_string(), size_of::<$t>()));
        $v.push((concat!($c, "@align").to_string(), align_of::<$t>()));
        $( $v.push((concat!($c, ".", $cf).to_string(), offset_of!($t, $f))); )*
    }};
}

#[test]
fn layouts_match_c() {
    let mut r: Vec<(String, usize)> = Vec::new();
    rust_layout!(r; BlkDev as "blk_dev_t" { name as "name", block_size as "block_size",
        block_count as "block_count", flags as "flags", read as "read", write as "write",
        sync as "sync", priv_ as "priv" });
    rust_layout!(r; PartInfo as "part_info_t" { dev as "dev", index as "index", type_ as "type",
        bootable as "bootable", superfloppy as "superfloppy", start_lba as "start_lba",
        block_count as "block_count" });
    rust_layout!(r; FsFile as "fs_file_t" {});
    rust_layout!(r; FsDir as "fs_dir_t" {});
    rust_layout!(r; FsStat as "fs_stat_t" { size as "size", attr as "attr", is_dir as "is_dir",
        date as "date", time as "time", name as "name", short_name as "short_name",
        leftover as "leftover" });
    rust_layout!(r; FsStatvfs as "fs_statvfs_t" { total_bytes as "total_bytes",
        free_bytes as "free_bytes", cluster_bytes as "cluster_bytes", fat_type as "fat_type" });
    rust_layout!(r; FsMkfsOpts as "fs_mkfs_opts_t" { fmt as "fmt", cluster_bytes as "cluster_bytes" });
    rust_layout!(r; RimgHeader as "rimg_header_t" { magic as "magic", hdr_version as "hdr_version",
        hdr_size as "hdr_size", flags as "flags", load_addr as "load_addr",
        file_size as "file_size", mem_size as "mem_size", crc32 as "crc32",
        abi_version as "abi_version", image_version as "image_version", subtype as "subtype",
        name as "name", reserved as "reserved" });
    rust_layout!(r; RimgExpect as "rimg_expect_t" { magic as "magic", slot_base as "slot_base",
        slot_size as "slot_size", abi_major as "abi_major", file_size as "file_size" });

    let c = c_layout();
    let mut c_sorted = c.clone();
    c_sorted.sort();
    r.sort();
    assert_eq!(r, c_sorted, "Rust and C disagree on a struct layout");
    println!("{} layout entries match", c.len());
}

#[test]
fn time_hook_links() {
    // The weak default in fs_glue.c: with no clock set it returns 2026-01-01 00:00.
    // SAFETY: no arguments, no state.
    let t = unsafe { fs_time_hook() };
    assert_eq!(t, (46 << 25) | (1 << 21) | (1 << 16));
}
