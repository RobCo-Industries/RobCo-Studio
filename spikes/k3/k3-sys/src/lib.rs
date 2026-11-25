//! Spike K3: hand-written declarations for RobCo-Core's host C, and a card
//! builder on top of them. Throwaway code.

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};

pub type BlkFn = Option<unsafe extern "C" fn(*mut BlkDev, u64, u32, *mut c_void) -> c_int>;
pub type BlkWriteFn = Option<unsafe extern "C" fn(*mut BlkDev, u64, u32, *const c_void) -> c_int>;
pub type BlkSyncFn = Option<unsafe extern "C" fn(*mut BlkDev) -> c_int>;

#[repr(C)]
pub struct BlkDev {
    pub name: *const c_char,
    pub block_size: u32,
    pub block_count: u64,
    pub flags: u32,
    pub read: BlkFn,
    pub write: BlkWriteFn,
    pub sync: BlkSyncFn,
    pub priv_: *mut c_void,
}

#[repr(C)]
pub struct PartInfo {
    pub dev: *mut BlkDev,
    pub index: u8,
    pub type_: u8,
    pub bootable: u8,
    pub superfloppy: u8,
    pub start_lba: u64,
    pub block_count: u64,
}

pub const FS_FILE_SIZE: usize = 640;
pub const FS_DIR_SIZE: usize = 128;

#[repr(C, align(8))]
pub struct FsFile {
    pub raw: [u8; FS_FILE_SIZE],
}

#[repr(C, align(8))]
pub struct FsDir {
    pub raw: [u8; FS_DIR_SIZE],
}

#[repr(C)]
pub struct FsStat {
    pub size: u32,
    pub attr: u8,
    pub is_dir: u8,
    pub date: u16,
    pub time: u16,
    pub name: [c_char; 256],
    pub short_name: [c_char; 13],
    pub leftover: u8,
}

#[repr(C)]
pub struct FsStatvfs {
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub cluster_bytes: u32,
    pub fat_type: u8,
}

#[repr(C)]
pub struct FsMkfsOpts {
    pub fmt: c_uint,
    pub cluster_bytes: u32,
}

#[repr(C, packed)]
pub struct RimgHeader {
    pub magic: [u8; 8],
    pub hdr_version: u16,
    pub hdr_size: u16,
    pub flags: u32,
    pub load_addr: u32,
    pub file_size: u32,
    pub mem_size: u32,
    pub crc32: u32,
    pub abi_version: u32,
    pub image_version: u32,
    pub subtype: u32,
    pub name: [u8; 16],
    pub reserved: u32,
}

#[repr(C)]
pub struct RimgExpect {
    pub magic: *const c_char,
    pub slot_base: u32,
    pub slot_size: u32,
    pub abi_major: u32,
    pub file_size: u32,
}

#[repr(C)]
pub struct LayoutEntry {
    pub name: *const c_char,
    pub value: usize,
}

pub type IniHandler = Option<
    unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, c_uint, *mut c_void) -> c_int,
>;

pub const FS_READ: c_uint = 0x01;
pub const FS_WRITE: c_uint = 0x02;
pub const FS_CREATE: c_uint = 0x04;
pub const FS_TRUNC: c_uint = 0x08;
pub const FS_MKFS_FAT: c_uint = 1;
pub const FS_MKFS_FAT32: c_uint = 2;
pub const RIMG_OK: c_int = 0;

unsafe extern "C" {
    pub fn crc32(data: *const c_void, len: usize) -> u32;
    pub fn ini_parse(
        text: *const c_char,
        len: usize,
        f: IniHandler,
        ctx: *mut c_void,
        err_line: *mut c_uint,
    ) -> c_int;
    pub fn ini_get_int(
        text: *const c_char,
        len: usize,
        section: *const c_char,
        key: *const c_char,
        min: i32,
        max: i32,
        out: *mut i32,
    ) -> c_int;
    pub fn rimg_validate(
        image: *const c_void,
        avail: usize,
        expect: *const RimgExpect,
        out: *mut *const RimgHeader,
    ) -> c_int;
    pub fn rimg_crc(image: *const c_void, file_size: u32) -> u32;
    pub fn blk_ram_init(dev: *mut BlkDev, buf: *mut c_void, size: usize, flags: u32) -> c_int;
    pub fn part_scan(dev: *mut BlkDev, out: *mut PartInfo, max: c_uint) -> c_int;
    pub fn part_open(dev: *mut BlkDev, p: *const PartInfo, out: *mut BlkDev) -> c_int;
    pub fn fs_mkfs(dev: *mut BlkDev, opts: *const FsMkfsOpts) -> c_int;
    pub fn fs_mount(volume: *const c_char, dev: *mut BlkDev) -> c_int;
    pub fn fs_unmount(volume: *const c_char) -> c_int;
    pub fn fs_mkdir(path: *const c_char) -> c_int;
    pub fn fs_open(f: *mut FsFile, path: *const c_char, flags: c_uint) -> c_int;
    pub fn fs_write(f: *mut FsFile, buf: *const c_void, len: usize) -> c_int;
    pub fn fs_close(f: *mut FsFile) -> c_int;
    pub fn fs_read_file(
        path: *const c_char,
        buf: *mut c_void,
        max: usize,
        out: *mut usize,
    ) -> c_int;
    pub fn fs_stat(path: *const c_char, out: *mut FsStat) -> c_int;
    pub fn fs_statvfs(volume: *const c_char, out: *mut FsStatvfs) -> c_int;
    pub fn fs_strerror(err: c_int) -> *const c_char;
    pub fn fs_time_hook() -> u32;
    pub fn k3_layout_count() -> usize;
    pub fn k3_layout_at(i: usize) -> *const LayoutEntry;
}

/// The C side's view of every shared struct: (name, size or offset).
pub fn c_layout() -> Vec<(String, usize)> {
    // SAFETY: k3_layout_at returns a pointer into a static table, or NULL past the end.
    unsafe {
        (0..k3_layout_count())
            .map(|i| {
                let e = &*k3_layout_at(i);
                (
                    CStr::from_ptr(e.name).to_string_lossy().into_owned(),
                    e.value,
                )
            })
            .collect()
    }
}

/// An fs error code as text.
pub fn fs_err(rc: c_int) -> String {
    // SAFETY: fs_strerror returns a static string for every code.
    unsafe {
        CStr::from_ptr(fs_strerror(rc))
            .to_string_lossy()
            .into_owned()
    }
}

fn check(what: &str, rc: c_int) -> Result<(), String> {
    if rc < 0 {
        Err(format!("{what}: {} ({rc})", fs_err(rc)))
    } else {
        Ok(())
    }
}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

/// One entry of a card: a folder (None) or a file with its bytes.
pub struct Entry<'a> {
    pub path: &'a str,
    pub data: Option<&'a [u8]>,
}

pub const PART_LBA: u64 = 2048;
pub const SECTOR: usize = 512;

/// Builds a card image the way tools/mksdimage.sh lays it out: an MBR with
/// one partition at LBA 2048, FAT16 (type 0x0E) below 64 MiB and FAT32
/// (type 0x0C) from 64 MiB, then the entries in order. The MBR is written
/// here; the partition is formatted and filled by RobCo-Core's FatFs.
pub fn build_card(mib: usize, entries: &[Entry<'_>]) -> Result<Vec<u8>, String> {
    let mut img = vec![0u8; mib * 1024 * 1024];
    let sectors = (img.len() / SECTOR) as u64 - PART_LBA;
    let fat32 = mib >= 64;
    let mbr = &mut img[0x1BE..0x1CE];
    mbr[0] = 0x00;
    mbr[1..4].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    mbr[4] = if fat32 { 0x0C } else { 0x0E };
    mbr[5..8].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    mbr[8..12].copy_from_slice(&(PART_LBA as u32).to_le_bytes());
    mbr[12..16].copy_from_slice(&(sectors as u32).to_le_bytes());
    img[510] = 0x55;
    img[511] = 0xAA;

    let vol = cstr("card");
    // SAFETY: every pointer passed below points into `img`, `disk`, `part`,
    // `pdev` or a local, all of which outlive the calls; the volume is
    // unmounted before they go out of scope. The fs API is not thread-safe:
    // callers serialise their use of it.
    unsafe {
        let mut disk: BlkDev = std::mem::zeroed();
        check(
            "blk_ram_init",
            blk_ram_init(&mut disk, img.as_mut_ptr().cast(), img.len(), 0),
        )?;
        let mut part: PartInfo = std::mem::zeroed();
        let n = part_scan(&mut disk, &mut part, 1);
        if n != 1 {
            return Err(format!("part_scan found {n} entries"));
        }
        let mut pdev: BlkDev = std::mem::zeroed();
        check("part_open", part_open(&mut disk, &part, &mut pdev))?;
        let opts = FsMkfsOpts {
            fmt: if fat32 { FS_MKFS_FAT32 } else { FS_MKFS_FAT },
            cluster_bytes: if fat32 { 0 } else { 512 },
        };
        check("fs_mkfs", fs_mkfs(&mut pdev, &opts))?;
        check("fs_mount", fs_mount(vol.as_ptr(), &mut pdev))?;
        let res = (|| {
            for e in entries {
                let p = cstr(&format!("card:/{}", e.path));
                match e.data {
                    None => check(e.path, fs_mkdir(p.as_ptr()))?,
                    Some(d) => {
                        let mut f: FsFile = std::mem::zeroed();
                        check(
                            e.path,
                            fs_open(&mut f, p.as_ptr(), FS_WRITE | FS_CREATE | FS_TRUNC),
                        )?;
                        let w = fs_write(&mut f, d.as_ptr().cast(), d.len());
                        let c = fs_close(&mut f);
                        check(e.path, w)?;
                        if w as usize != d.len() {
                            return Err(format!("{}: short write {w}", e.path));
                        }
                        check(e.path, c)?;
                    }
                }
            }
            Ok(())
        })();
        let u = fs_unmount(vol.as_ptr());
        res?;
        check("fs_unmount", u)?;
    }
    Ok(img)
}

/// Mounts `img` again and reads one file back through the library.
pub fn read_back(img: &mut [u8], path: &str) -> Result<Vec<u8>, String> {
    let vol = cstr("back");
    // SAFETY: as in build_card.
    unsafe {
        let mut disk: BlkDev = std::mem::zeroed();
        check(
            "blk_ram_init",
            blk_ram_init(&mut disk, img.as_mut_ptr().cast(), img.len(), 0),
        )?;
        let mut part: PartInfo = std::mem::zeroed();
        if part_scan(&mut disk, &mut part, 1) != 1 {
            return Err("part_scan".into());
        }
        let mut pdev: BlkDev = std::mem::zeroed();
        check("part_open", part_open(&mut disk, &part, &mut pdev))?;
        check("fs_mount", fs_mount(vol.as_ptr(), &mut pdev))?;
        let mut buf = vec![0u8; 1 << 20];
        let mut n = 0usize;
        let p = cstr(&format!("back:/{path}"));
        let rc = fs_read_file(p.as_ptr(), buf.as_mut_ptr().cast(), buf.len(), &mut n);
        let mut vfs: FsStatvfs = std::mem::zeroed();
        let s = fs_statvfs(vol.as_ptr(), &mut vfs);
        let u = fs_unmount(vol.as_ptr());
        check(path, rc)?;
        check("fs_statvfs", s)?;
        check("fs_unmount", u)?;
        buf.truncate(n);
        Ok(buf)
    }
}
