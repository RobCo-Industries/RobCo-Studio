// SPDX-License-Identifier: GPL-3.0-or-later

//! Compares the size, alignment and field offsets of every struct Rust
//! shares with the C against the C compiler's view in `csrc/layout.c`.

use std::mem::{align_of, offset_of, size_of};

use studio_sys::c_layout;
use studio_sys::ffi::*;

/// The number of entries in `csrc/layout.c`.
const ENTRIES: usize = 65;

macro_rules! rust_layout {
    ($v:ident; $t:ident as $c:literal { $($f:ident as $cf:literal),* $(,)? }) => {{
        $v.push(($c.to_string(), size_of::<$t>()));
        $v.push((concat!($c, "@align").to_string(), align_of::<$t>()));
        $( $v.push((concat!($c, ".", $cf).to_string(), offset_of!($t, $f))); )*
    }};
}

fn rust_view() -> Vec<(String, usize)> {
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
    r
}

#[test]
fn layouts_match_c() {
    let mut c = c_layout();
    let mut r = rust_view();
    assert_eq!(c.len(), ENTRIES, "the C table has {} entries", c.len());
    assert_eq!(r.len(), ENTRIES, "the Rust view has {} entries", r.len());
    c.sort();
    r.sort();
    assert_eq!(r, c, "Rust and C disagree on a struct layout");
}
