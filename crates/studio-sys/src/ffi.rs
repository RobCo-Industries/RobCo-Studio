// SPDX-License-Identifier: GPL-3.0-or-later

//! Hand-written declarations for RobCo-Core's C, as its headers in
//! `include/robco/` give them. `tests/layout.rs` checks every struct
//! against the C compiler's view.

use std::ffi::{c_char, c_int, c_uint, c_void};

/// `blk_dev_t.read`.
pub type BlkReadFn = Option<unsafe extern "C" fn(*mut BlkDev, u64, u32, *mut c_void) -> c_int>;
/// `blk_dev_t.write`.
pub type BlkWriteFn = Option<unsafe extern "C" fn(*mut BlkDev, u64, u32, *const c_void) -> c_int>;
/// `blk_dev_t.sync`.
pub type BlkSyncFn = Option<unsafe extern "C" fn(*mut BlkDev) -> c_int>;

/// `blk_dev_t`, a block device.
#[repr(C)]
#[derive(Debug)]
pub struct BlkDev {
    /// The device's name.
    pub name: *const c_char,
    /// Bytes per block.
    pub block_size: u32,
    /// Number of blocks.
    pub block_count: u64,
    /// `BLK_F_*` flags.
    pub flags: u32,
    /// Reads blocks.
    pub read: BlkReadFn,
    /// Writes blocks.
    pub write: BlkWriteFn,
    /// Flushes the device.
    pub sync: BlkSyncFn,
    /// The driver's state.
    pub priv_: *mut c_void,
}

impl Default for BlkDev {
    fn default() -> Self {
        Self {
            name: std::ptr::null(),
            block_size: 0,
            block_count: 0,
            flags: 0,
            read: None,
            write: None,
            sync: None,
            priv_: std::ptr::null_mut(),
        }
    }
}

/// `part_info_t`, one partition found by `part_scan`.
#[repr(C)]
#[derive(Debug)]
pub struct PartInfo {
    /// The whole device.
    pub dev: *mut BlkDev,
    /// The MBR slot, from 0.
    pub index: u8,
    /// The MBR partition type.
    pub type_: u8,
    /// Non-zero when marked bootable.
    pub bootable: u8,
    /// Non-zero for a volume with no partition table.
    pub superfloppy: u8,
    /// The first block.
    pub start_lba: u64,
    /// The number of blocks.
    pub block_count: u64,
}

impl Default for PartInfo {
    fn default() -> Self {
        Self {
            dev: std::ptr::null_mut(),
            index: 0,
            type_: 0,
            bootable: 0,
            superfloppy: 0,
            start_lba: 0,
            block_count: 0,
        }
    }
}

/// `FS_FILE_SIZE`, the bytes of an opaque `fs_file_t`.
pub const FS_FILE_SIZE: usize = 640;
/// `FS_DIR_SIZE`, the bytes of an opaque `fs_dir_t`.
pub const FS_DIR_SIZE: usize = 128;

/// `fs_file_t`, an open file, opaque.
#[repr(C, align(8))]
#[derive(Debug)]
pub struct FsFile {
    /// The C's storage.
    pub raw: [u8; FS_FILE_SIZE],
}

impl Default for FsFile {
    fn default() -> Self {
        Self {
            raw: [0; FS_FILE_SIZE],
        }
    }
}

/// `fs_dir_t`, an open folder, opaque.
#[repr(C, align(8))]
#[derive(Debug)]
pub struct FsDir {
    /// The C's storage.
    pub raw: [u8; FS_DIR_SIZE],
}

impl Default for FsDir {
    fn default() -> Self {
        Self {
            raw: [0; FS_DIR_SIZE],
        }
    }
}

/// `fs_stat_t`, what `fs_stat` reports.
#[repr(C)]
#[derive(Debug)]
pub struct FsStat {
    /// Bytes.
    pub size: u32,
    /// FAT attribute bits.
    pub attr: u8,
    /// Non-zero for a folder.
    pub is_dir: u8,
    /// FAT date.
    pub date: u16,
    /// FAT time.
    pub time: u16,
    /// The long name.
    pub name: [c_char; 256],
    /// The 8.3 name.
    pub short_name: [c_char; 13],
    /// Non-zero when a crash left the entry behind.
    pub leftover: u8,
}

impl Default for FsStat {
    fn default() -> Self {
        Self {
            size: 0,
            attr: 0,
            is_dir: 0,
            date: 0,
            time: 0,
            name: [0; 256],
            short_name: [0; 13],
            leftover: 0,
        }
    }
}

/// `fs_statvfs_t`, what `fs_statvfs` reports.
#[repr(C)]
#[derive(Debug, Default)]
pub struct FsStatvfs {
    /// Bytes of the volume.
    pub total_bytes: u64,
    /// Free bytes.
    pub free_bytes: u64,
    /// Bytes per cluster.
    pub cluster_bytes: u32,
    /// 12, 16 or 32.
    pub fat_type: u8,
}

/// `fs_mkfs_opts_t`.
#[repr(C)]
#[derive(Debug, Default)]
pub struct FsMkfsOpts {
    /// `FS_MKFS_FAT` or `FS_MKFS_FAT32`.
    pub fmt: c_uint,
    /// Bytes per cluster, 0 for the default.
    pub cluster_bytes: u32,
}

/// `rimg_header_t`, the 64-byte header at 0x20 of a sealed image.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct RimgHeader {
    /// The magic, such as `ROM1`.
    pub magic: [u8; 8],
    /// The header version.
    pub hdr_version: u16,
    /// The header size, 64.
    pub hdr_size: u16,
    /// `RIMG_FLAG_*` bits.
    pub flags: u32,
    /// The load address.
    pub load_addr: u32,
    /// The bytes of the file.
    pub file_size: u32,
    /// The bytes in memory.
    pub mem_size: u32,
    /// The CRC-32 of the file with this field as zero.
    pub crc32: u32,
    /// The ABI the image needs.
    pub abi_version: u32,
    /// The image's own version.
    pub image_version: u32,
    /// The subtype.
    pub subtype: u32,
    /// The short name.
    pub name: [u8; 16],
    /// Zero.
    pub reserved: u32,
}

/// `rimg_expect_t`, what `rimg_validate` checks against.
#[repr(C)]
#[derive(Debug)]
pub struct RimgExpect {
    /// 8 bytes of magic, or NULL for any known magic.
    pub magic: *const c_char,
    /// The load slot's base.
    pub slot_base: u32,
    /// The load slot's size.
    pub slot_size: u32,
    /// The ABI major, 0 for the magic's.
    pub abi_major: u32,
    /// The size of the file.
    pub file_size: u32,
}

/// `rimg_icon_format_t`, a C enum the size of `int`.
pub type RimgIconFormat = c_int;
/// `RIMG_ICON_AUTO`: 128 bytes is raw, any other length is PBM.
pub const RIMG_ICON_AUTO: RimgIconFormat = 0;
/// `RIMG_ICON_RAW`: exactly 128 bytes in the META layout.
pub const RIMG_ICON_RAW: RimgIconFormat = 1;
/// `RIMG_ICON_PBM`: a 32 by 32 PBM, P1 or P4.
pub const RIMG_ICON_PBM: RimgIconFormat = 2;

/// `rimg_seal_opts_t`, the inputs of `rimg_seal`. NULL is an absent input.
#[repr(C)]
#[derive(Debug)]
pub struct RimgSealOpts {
    /// NUL-terminated name, or NULL.
    pub name: *const c_char,
    /// Non-zero when `image_version` is given.
    pub has_image_version: c_int,
    /// `(X << 16) | (Y << 8) | Z`.
    pub image_version: u32,
    /// NUL-terminated title, or NULL.
    pub title: *const c_char,
    /// The icon file's bytes, or NULL.
    pub icon: *const u8,
    /// The number of icon bytes.
    pub icon_len: usize,
    /// How to decode the icon.
    pub icon_format: RimgIconFormat,
}

/// `rimg_pack_opts_t`, the inputs of `rimg_pack`.
#[repr(C)]
#[derive(Debug)]
pub struct RimgPackOpts {
    /// Name, image version, title and icon.
    pub seal: RimgSealOpts,
    /// NUL-terminated magic: `BOOT`, `ROM1` or `ROBCO78`.
    pub magic: *const c_char,
    /// The subtype.
    pub subtype: i64,
    /// The load address.
    pub load_addr: i64,
    /// Non-zero when `mem_size` is given.
    pub has_mem_size: c_int,
    /// The bytes in memory.
    pub mem_size: i64,
    /// Non-zero when `abi_version` is given.
    pub has_abi_version: c_int,
    /// `(MAJOR << 16) | MINOR`.
    pub abi_version: u32,
    /// -1 follows the magic, 0 no META block, 1 a META block.
    pub meta: c_int,
    /// The entry offset.
    pub entry: i64,
}

/// One entry of the C's layout table in `csrc/layout.c`.
#[repr(C)]
#[derive(Debug)]
pub struct LayoutEntry {
    /// `type`, `type@align` or `type.field`.
    pub name: *const c_char,
    /// The size, alignment or offset.
    pub value: usize,
}

/// `ini_handler_t`.
pub type IniHandler = Option<
    unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, c_uint, *mut c_void) -> c_int,
>;

/// Open for reading.
pub const FS_READ: c_uint = 0x01;
/// Open for writing.
pub const FS_WRITE: c_uint = 0x02;
/// Create the file.
pub const FS_CREATE: c_uint = 0x04;
/// Truncate the file.
pub const FS_TRUNC: c_uint = 0x08;
/// Format as FAT12 or FAT16, by size.
pub const FS_MKFS_FAT: c_uint = 1;
/// Format as FAT32.
pub const FS_MKFS_FAT32: c_uint = 2;
/// `rimg_validate` success.
pub const RIMG_OK: c_int = 0;
/// `RIMG_E_CRC`.
pub const RIMG_E_CRC: c_int = 8;
/// `RIMG_E_IO`.
pub const RIMG_E_IO: c_int = 2;
/// The bytes `rimg_pack` adds to a payload with a META block: header and META.
pub const RIMG_PACK_MAX_EXTRA: usize = 0x120;
/// The shortest file `rimg_crc` and `rimg_validate` read: `RIMG_META_OFFSET`.
pub const RIMG_MIN_FILE: usize = 0x60;

unsafe extern "C" {
    /// CRC-32 of `len` bytes.
    pub fn crc32(data: *const c_void, len: usize) -> u32;
    /// Parses an INI text, calling `f` for each key.
    pub fn ini_parse(
        text: *const c_char,
        len: usize,
        f: IniHandler,
        ctx: *mut c_void,
        err_line: *mut c_uint,
    ) -> c_int;
    /// Reads one integer key within `min..=max`.
    pub fn ini_get_int(
        text: *const c_char,
        len: usize,
        section: *const c_char,
        key: *const c_char,
        min: i32,
        max: i32,
        out: *mut i32,
    ) -> c_int;
    /// Short text for an `INI_*` code.
    pub fn ini_strerror(code: c_int) -> *const c_char;
    /// Checks a sealed image.
    pub fn rimg_validate(
        image: *const c_void,
        avail: usize,
        expect: *const RimgExpect,
        out: *mut *const RimgHeader,
    ) -> c_int;
    /// The CRC the header of `image` should hold.
    pub fn rimg_crc(image: *const c_void, file_size: u32) -> u32;
    /// Seals `len` bytes of `image` into `out`, which holds `len` bytes.
    pub fn rimg_seal(
        image: *const c_void,
        len: usize,
        opts: *const RimgSealOpts,
        out: *mut c_void,
    ) -> c_int;
    /// Packs a payload into `out`, which holds `out_cap` bytes.
    pub fn rimg_pack(
        payload: *const c_void,
        payload_len: usize,
        opts: *const RimgPackOpts,
        out: *mut c_void,
        out_cap: usize,
        out_len: *mut usize,
    ) -> c_int;
    /// Short text for a `RIMG_*` code.
    pub fn rimg_strerror(code: c_int) -> *const c_char;
    /// A block device over a RAM buffer.
    pub fn blk_ram_init(dev: *mut BlkDev, buf: *mut c_void, size: usize, flags: u32) -> c_int;
    /// Reads the partition table.
    pub fn part_scan(dev: *mut BlkDev, out: *mut PartInfo, max: c_uint) -> c_int;
    /// A block device over one partition.
    pub fn part_open(dev: *mut BlkDev, p: *const PartInfo, out: *mut BlkDev) -> c_int;
    /// Formats a device.
    pub fn fs_mkfs(dev: *mut BlkDev, opts: *const FsMkfsOpts) -> c_int;
    /// Mounts a device as `volume`.
    pub fn fs_mount(volume: *const c_char, dev: *mut BlkDev) -> c_int;
    /// Unmounts `volume`.
    pub fn fs_unmount(volume: *const c_char) -> c_int;
    /// Makes a folder.
    pub fn fs_mkdir(path: *const c_char) -> c_int;
    /// Opens a file.
    pub fn fs_open(f: *mut FsFile, path: *const c_char, flags: c_uint) -> c_int;
    /// Writes to a file; returns the bytes written or an error.
    pub fn fs_write(f: *mut FsFile, buf: *const c_void, len: usize) -> c_int;
    /// Closes a file.
    pub fn fs_close(f: *mut FsFile) -> c_int;
    /// Reads up to `max` bytes of a file.
    pub fn fs_read_file(
        path: *const c_char,
        buf: *mut c_void,
        max: usize,
        out: *mut usize,
    ) -> c_int;
    /// Reports a file or folder.
    pub fn fs_stat(path: *const c_char, out: *mut FsStat) -> c_int;
    /// Reports a volume.
    pub fn fs_statvfs(volume: *const c_char, out: *mut FsStatvfs) -> c_int;
    /// Short text for an `FS_ERR_*` code.
    pub fn fs_strerror(err: c_int) -> *const c_char;
    /// The FAT time stamp for new entries. Weak in `fs_glue.c`.
    pub fn fs_time_hook() -> u32;
    /// The number of entries in the layout table.
    pub fn studio_sys_layout_count() -> usize;
    /// One entry of the layout table, or NULL past the end.
    pub fn studio_sys_layout_at(i: usize) -> *const LayoutEntry;
}
