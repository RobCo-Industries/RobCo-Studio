// SPDX-License-Identifier: GPL-3.0-or-later
/*
 * Size and field offsets of every struct Rust shares with the C, as the C
 * compiler sees them. tests/layout.rs compares them with Rust's view.
 */
#include <stddef.h>
#include <stdint.h>

#include <robco/blk.h>
#include <robco/fs.h>
#include <robco/part.h>
#include <robco/rimg.h>

typedef struct {
    const char *name;
    size_t value;
} studio_sys_layout_entry_t;

#define SZ(t) { #t, sizeof(t) }
#define AL(t) { #t "@align", _Alignof(t) }
#define OF(t, f) { #t "." #f, offsetof(t, f) }

static const studio_sys_layout_entry_t entries[] = {
    SZ(blk_dev_t), AL(blk_dev_t),
    OF(blk_dev_t, name), OF(blk_dev_t, block_size), OF(blk_dev_t, block_count),
    OF(blk_dev_t, flags), OF(blk_dev_t, read), OF(blk_dev_t, write),
    OF(blk_dev_t, sync), OF(blk_dev_t, priv),

    SZ(part_info_t), AL(part_info_t),
    OF(part_info_t, dev), OF(part_info_t, index), OF(part_info_t, type),
    OF(part_info_t, bootable), OF(part_info_t, superfloppy),
    OF(part_info_t, start_lba), OF(part_info_t, block_count),

    SZ(fs_file_t), AL(fs_file_t),
    SZ(fs_dir_t), AL(fs_dir_t),

    SZ(fs_stat_t), AL(fs_stat_t),
    OF(fs_stat_t, size), OF(fs_stat_t, attr), OF(fs_stat_t, is_dir),
    OF(fs_stat_t, date), OF(fs_stat_t, time), OF(fs_stat_t, name),
    OF(fs_stat_t, short_name), OF(fs_stat_t, leftover),

    SZ(fs_statvfs_t), AL(fs_statvfs_t),
    OF(fs_statvfs_t, total_bytes), OF(fs_statvfs_t, free_bytes),
    OF(fs_statvfs_t, cluster_bytes), OF(fs_statvfs_t, fat_type),

    SZ(fs_mkfs_opts_t), AL(fs_mkfs_opts_t),
    OF(fs_mkfs_opts_t, fmt), OF(fs_mkfs_opts_t, cluster_bytes),

    SZ(rimg_header_t), AL(rimg_header_t),
    OF(rimg_header_t, magic), OF(rimg_header_t, hdr_version), OF(rimg_header_t, hdr_size),
    OF(rimg_header_t, flags), OF(rimg_header_t, load_addr), OF(rimg_header_t, file_size),
    OF(rimg_header_t, mem_size), OF(rimg_header_t, crc32), OF(rimg_header_t, abi_version),
    OF(rimg_header_t, image_version), OF(rimg_header_t, subtype), OF(rimg_header_t, name),
    OF(rimg_header_t, reserved),

    SZ(rimg_expect_t), AL(rimg_expect_t),
    OF(rimg_expect_t, magic), OF(rimg_expect_t, slot_base), OF(rimg_expect_t, slot_size),
    OF(rimg_expect_t, abi_major), OF(rimg_expect_t, file_size),
};

/* The fixed formats do not depend on the host. */
_Static_assert(sizeof(rimg_header_t) == 64, "rimg_header_t is 64 bytes");
_Static_assert(offsetof(rimg_header_t, crc32) == 0x1C, "crc32 at 0x1C of the header");
_Static_assert(sizeof(fs_file_t) == FS_FILE_SIZE, "fs_file_t size");
_Static_assert(sizeof(fs_dir_t) == FS_DIR_SIZE, "fs_dir_t size");

size_t studio_sys_layout_count(void);
const studio_sys_layout_entry_t *studio_sys_layout_at(size_t i);

size_t studio_sys_layout_count(void)
{
    return sizeof(entries) / sizeof(entries[0]);
}

const studio_sys_layout_entry_t *studio_sys_layout_at(size_t i)
{
    return i < studio_sys_layout_count() ? &entries[i] : NULL;
}
