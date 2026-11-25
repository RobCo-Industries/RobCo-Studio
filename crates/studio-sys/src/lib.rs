// SPDX-License-Identifier: GPL-3.0-or-later

//! RobCo-Core's C, linked into Studio, with a thin Rust binding.
//!
//! `build.rs` compiles a fixed list of RobCo-Core's C files from the
//! submodule at `external/robco-core`. [`ffi`] holds the hand-written
//! `extern "C"` declarations. This module holds safe wrappers: CRC-32, INI,
//! image checks with `rimg_validate`, and card images written with the
//! library's FatFs on its RAM block device.
//!
//! The file system's state is global in the C. The card functions take one
//! lock, so one caller at a time uses it.
//!
//! `unsafe` is allowed here because calling C through `extern "C"` needs
//! it. Every `unsafe` block carries a `SAFETY` comment, which clippy checks.
#![allow(unsafe_code, reason = "calls into RobCo-Core's C need unsafe")]

pub mod ffi;

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::fmt;
use std::sync::Mutex;

pub use ffi::RimgHeader;

/// A failed call into the C, with the code it returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// What was being done: a function name or a card path.
    pub what: String,
    /// The C return code.
    pub code: i32,
    /// The C library's text for the code.
    pub text: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} ({})", self.what, self.text, self.code)
    }
}

impl std::error::Error for Error {}

impl Error {
    fn new(what: impl Into<String>, code: i32, text: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            code,
            text: text.into(),
        }
    }
}

/// Copies a static C string. A NULL pointer gives an empty string.
fn static_text(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the C's strerror functions return NUL-terminated static strings.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// The CRC-32 of `data`: IEEE 802.3, reflected, as in the rimg format.
pub fn crc32(data: &[u8]) -> u32 {
    // SAFETY: `data` is a valid buffer of `data.len()` bytes for the call.
    unsafe { ffi::crc32(data.as_ptr().cast(), data.len()) }
}

/// One key of an INI text, as `ini_parse` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniEntry {
    /// The section, empty before the first header.
    pub section: String,
    /// The key.
    pub key: String,
    /// The value, after comments and escapes.
    pub value: String,
    /// The line number, from 1.
    pub line: u32,
}

/// An INI parse that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniError {
    /// The `INI_ERR_*` code and its text.
    pub error: Error,
    /// The line of the error, or 0 when it has none.
    pub line: u32,
    /// The keys read before the error.
    pub entries: Vec<IniEntry>,
}

impl fmt::Display for IniError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} on line {}", self.error, self.line)
    }
}

impl std::error::Error for IniError {}

fn ini_error(what: &str, rc: c_int) -> Error {
    // SAFETY: ini_strerror takes any code and returns a static string.
    Error::new(what, rc, static_text(unsafe { ffi::ini_strerror(rc) }))
}

unsafe extern "C" fn ini_collect(
    section: *const c_char,
    key: *const c_char,
    value: *const c_char,
    line: c_uint,
    ctx: *mut c_void,
) -> c_int {
    // SAFETY: ini_parse passes NUL-terminated strings and the `ctx` that
    // `ini_parse` below gave it, a live `Vec<IniEntry>` borrowed mutably.
    unsafe {
        let out = &mut *ctx.cast::<Vec<IniEntry>>();
        let s = |p: *const c_char| CStr::from_ptr(p).to_string_lossy().into_owned();
        out.push(IniEntry {
            section: s(section),
            key: s(key),
            value: s(value),
            line,
        });
    }
    0
}

/// Parses an INI text with RobCo-Core's parser and returns every key.
pub fn ini_parse(text: &str) -> Result<Vec<IniEntry>, IniError> {
    let mut entries: Vec<IniEntry> = Vec::new();
    let mut line: c_uint = 0;
    // SAFETY: `text`, `entries` and `line` outlive the call, and
    // `ini_collect` matches `ini_handler_t` and reads `ctx` as `entries`.
    let rc = unsafe {
        ffi::ini_parse(
            text.as_ptr().cast(),
            text.len(),
            Some(ini_collect),
            (&raw mut entries).cast(),
            &mut line,
        )
    };
    if rc < 0 {
        return Err(IniError {
            error: ini_error("ini_parse", rc),
            line,
            entries,
        });
    }
    Ok(entries)
}

fn c_string(what: &str, s: &str) -> Result<CString, Error> {
    CString::new(s).map_err(|_| Error::new(what, -1, "the text holds a NUL byte"))
}

/// Reads one integer key with `ini_get_int`, which checks `min..=max`.
pub fn ini_get_int(text: &str, section: &str, key: &str, min: i32, max: i32) -> Result<i32, Error> {
    let section = c_string("ini_get_int", section)?;
    let key = c_string("ini_get_int", key)?;
    let mut v = 0;
    // SAFETY: every buffer outlives the call and the names end with NUL.
    let rc = unsafe {
        ffi::ini_get_int(
            text.as_ptr().cast(),
            text.len(),
            section.as_ptr(),
            key.as_ptr(),
            min,
            max,
            &mut v,
        )
    };
    if rc != 0 {
        return Err(ini_error("ini_get_int", rc));
    }
    Ok(v)
}

/// What `rimg_validate` checks an image against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Expect {
    /// The 8-byte magic, or `None` to accept the three known magics.
    pub magic: Option<[u8; 8]>,
    /// The base of the load slot, with `slot_size`. 0 and 0 skip the check.
    pub slot_base: u32,
    /// The size of the load slot.
    pub slot_size: u32,
    /// The ABI major. 0 uses the major of the magic.
    pub abi_major: u32,
    /// The size of the file the image came from.
    pub file_size: u32,
}

fn rimg_error(rc: c_int) -> Error {
    // SAFETY: rimg_strerror takes any code and returns a static string.
    Error::new(
        "rimg_validate",
        rc,
        static_text(unsafe { ffi::rimg_strerror(rc) }),
    )
}

/// Checks a sealed image with `rimg_validate` and returns a copy of its
/// header. The error's code is the `RIMG_E_*` code.
pub fn rimg_validate(image: &[u8], expect: &Expect) -> Result<RimgHeader, Error> {
    let magic = expect.magic;
    let c = ffi::RimgExpect {
        magic: magic
            .as_ref()
            .map_or(std::ptr::null(), |m| m.as_ptr().cast()),
        slot_base: expect.slot_base,
        slot_size: expect.slot_size,
        abi_major: expect.abi_major,
        file_size: expect.file_size,
    };
    let mut hdr: *const RimgHeader = std::ptr::null();
    // SAFETY: `image`, `magic` and `c` outlive the call; `avail` is the
    // length of `image`.
    let rc = unsafe { ffi::rimg_validate(image.as_ptr().cast(), image.len(), &c, &mut hdr) };
    if rc != ffi::RIMG_OK {
        return Err(rimg_error(rc));
    }
    if hdr.is_null() {
        return Err(Error::new("rimg_validate", rc, "no header"));
    }
    // SAFETY: on RIMG_OK `hdr` points at the 64-byte header inside `image`,
    // which is alive. The header is packed, so read it unaligned.
    Ok(unsafe { std::ptr::read_unaligned(hdr) })
}

/// The CRC `rimg_validate` expects in the header of `image`, over its
/// whole length. `None` when the image is shorter than its header or longer
/// than 4 GiB.
pub fn rimg_crc(image: &[u8]) -> Option<u32> {
    let size = u32::try_from(image.len()).ok()?;
    if image.len() < ffi::RIMG_MIN_FILE {
        return None;
    }
    // SAFETY: `image` holds `size` bytes, at least the header's 0x60.
    Some(unsafe { ffi::rimg_crc(image.as_ptr().cast(), size) })
}

/// The FAT time stamp the file system writes: the weak default of
/// `fs_glue.c`, 2026-01-01 00:00, so card images are the same everywhere.
pub fn fs_time() -> u32 {
    // SAFETY: no arguments, no state.
    unsafe { ffi::fs_time_hook() }
}

/// The C compiler's view of every shared struct: the size, the alignment
/// and each field offset, as `(name, value)` pairs.
pub fn c_layout() -> Vec<(String, usize)> {
    // SAFETY: no arguments, reads a static table.
    let n = unsafe { ffi::studio_sys_layout_count() };
    (0..n)
        .filter_map(|i| {
            // SAFETY: `i` is below the count, so the C returns a pointer into
            // its static table; NULL is checked anyway.
            let e = unsafe { ffi::studio_sys_layout_at(i).as_ref() }?;
            Some((static_text(e.name), e.value))
        })
        .collect()
}

/// One entry of a card: a folder when `data` is `None`, else a file.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    /// The path from the root, with `/` between parts.
    pub path: &'a str,
    /// The file's bytes, or `None` for a folder.
    pub data: Option<&'a [u8]>,
}

/// The first sector of the card's one partition.
pub const PART_LBA: u64 = 2048;
/// The sector size of a card.
pub const SECTOR: usize = 512;

/// Serialises every use of the C file system, whose state is global.
static FS_LOCK: Mutex<()> = Mutex::new(());

fn fs_check(what: &str, rc: c_int) -> Result<(), Error> {
    if rc < 0 {
        // SAFETY: fs_strerror takes any code and returns a static string.
        let text = static_text(unsafe { ffi::fs_strerror(rc) });
        Err(Error::new(what, rc, text))
    } else {
        Ok(())
    }
}

/// Opens the card's one partition on a RAM block device over `img` and
/// mounts it as `vol`. Calls `f`, then unmounts. Holds the file-system lock.
fn with_volume<T>(
    img: &mut [u8],
    vol: &CStr,
    mkfs: Option<&ffi::FsMkfsOpts>,
    f: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let _guard = FS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut disk = ffi::BlkDev::default();
    let mut part = ffi::PartInfo::default();
    let mut pdev = ffi::BlkDev::default();
    // SAFETY: `img`, `disk`, `part` and `pdev` outlive every call below and
    // the mount, since the volume is unmounted before this block ends. The
    // lock keeps any other caller out of the C's global state.
    unsafe {
        fs_check(
            "blk_ram_init",
            ffi::blk_ram_init(&mut disk, img.as_mut_ptr().cast(), img.len(), 0),
        )?;
        let n = ffi::part_scan(&mut disk, &mut part, 1);
        if n != 1 {
            return Err(Error::new("part_scan", n, "expected one partition"));
        }
        fs_check("part_open", ffi::part_open(&mut disk, &part, &mut pdev))?;
        if let Some(opts) = mkfs {
            fs_check("fs_mkfs", ffi::fs_mkfs(&mut pdev, opts))?;
        }
        fs_check("fs_mount", ffi::fs_mount(vol.as_ptr(), &mut pdev))?;
        let res = f();
        let u = ffi::fs_unmount(vol.as_ptr());
        let v = res?;
        fs_check("fs_unmount", u)?;
        Ok(v)
    }
}

/// The smallest card [`build_card`] makes, in MiB.
pub const CARD_MIN_MIB: usize = 4;
/// The largest card [`build_card`] makes, in MiB.
pub const CARD_MAX_MIB: usize = 32768;
/// The smallest FAT32 card, in MiB. Smaller cards are FAT16.
pub const CARD_FAT32_MIN_MIB: usize = 64;

/// How [`build_card`] lays out a card of a given size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardLayout {
    /// Size of the image in MiB.
    pub mib: usize,
    /// MBR partition type: 0x0E for FAT16, 0x0C for FAT32.
    pub part_type: u8,
    /// The FAT type the volume must have: 16 or 32.
    pub fat_type: u8,
    /// Bytes per cluster, 0 to let FatFs choose.
    pub cluster_bytes: u32,
}

/// The layout of a card of `mib` MiB, the rules of RobCo-Core's
/// `tools/mksdimage.sh`. The card size is a power of 2 from 4 to 32768
/// MiB, the sizes that tool writes; the caller rounds up, as it does.
///
/// | Size | Type | Partition | Cluster |
/// |---|---|---|---|
/// | 4 to 32 MiB | FAT16 | 0x0E | 512 bytes |
/// | 64 to 32768 MiB | FAT32 | 0x0C | FatFs default |
///
/// One-sector clusters give 6 to 63 thousand clusters at 4 to 32 MiB,
/// inside FAT16's 4085 to 65524. Every other size is refused.
pub fn card_layout(mib: usize) -> Result<CardLayout, Error> {
    if !(CARD_MIN_MIB..=CARD_MAX_MIB).contains(&mib) || !mib.is_power_of_two() {
        return Err(Error::new(
            "build_card",
            -1,
            "card size must be a power of 2 from 4 to 32768 MiB: FAT16 from 4 to 32, FAT32 from 64",
        ));
    }
    Ok(if mib >= CARD_FAT32_MIN_MIB {
        CardLayout {
            mib,
            part_type: 0x0C,
            fat_type: 32,
            cluster_bytes: 0,
        }
    } else {
        CardLayout {
            mib,
            part_type: 0x0E,
            fat_type: 16,
            cluster_bytes: 512,
        }
    })
}

/// Builds a card image in memory, laid out as RobCo-Core's
/// `tools/mksdimage.sh` does: an MBR written here with one partition at
/// LBA 2048, sized and typed by [`card_layout`], formatted and filled in
/// order by RobCo-Core's FatFs. It refuses a size [`card_layout`] refuses
/// before it allocates, and fails if the volume FatFs made is not the FAT
/// type the partition table declares.
///
/// FAT16 is tested at 4, 16 and 32 MiB. FAT32 is built at 64 MiB only.
pub fn build_card(mib: usize, entries: &[Entry<'_>]) -> Result<Vec<u8>, Error> {
    let layout = card_layout(mib)?;
    let bytes = mib
        .checked_mul(1024 * 1024)
        .ok_or_else(|| Error::new("build_card", -1, "card too large for this system"))?;
    let sectors = (bytes / SECTOR) as u64;
    let part_sectors = u32::try_from(sectors - PART_LBA)
        .map_err(|_| Error::new("build_card", -1, "card size out of range"))?;
    let mut img = vec![0u8; bytes];
    let mbr = &mut img[0x1BE..0x1CE];
    mbr[0] = 0x00;
    mbr[1..4].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    mbr[4] = layout.part_type;
    mbr[5..8].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    mbr[8..12].copy_from_slice(&(PART_LBA as u32).to_le_bytes());
    mbr[12..16].copy_from_slice(&part_sectors.to_le_bytes());
    img[510] = 0x55;
    img[511] = 0xAA;

    let opts = ffi::FsMkfsOpts {
        fmt: if layout.fat_type == 32 {
            ffi::FS_MKFS_FAT32
        } else {
            ffi::FS_MKFS_FAT
        },
        cluster_bytes: layout.cluster_bytes,
    };
    with_volume(&mut img, c"card", Some(&opts), || {
        let made = statvfs(c"card")?.fat_type;
        if made != layout.fat_type {
            return Err(Error::new(
                "build_card",
                i32::from(made),
                "FatFs made a FAT type the partition table does not declare",
            ));
        }
        for e in entries {
            let p = c_string(e.path, &format!("card:/{}", e.path))?;
            match e.data {
                // SAFETY: `p` ends with NUL and outlives the call.
                None => fs_check(e.path, unsafe { ffi::fs_mkdir(p.as_ptr()) })?,
                Some(d) => write_file(e.path, &p, d)?,
            }
        }
        Ok(())
    })?;
    Ok(img)
}

fn write_file(what: &str, path: &CStr, d: &[u8]) -> Result<(), Error> {
    let mut f = ffi::FsFile::default();
    let flags = ffi::FS_WRITE | ffi::FS_CREATE | ffi::FS_TRUNC;
    // SAFETY: `f`, `path` and `d` outlive the calls; the file is closed
    // before `f` goes out of scope.
    let (w, c) = unsafe {
        fs_check(what, ffi::fs_open(&mut f, path.as_ptr(), flags))?;
        let w = ffi::fs_write(&mut f, d.as_ptr().cast(), d.len());
        (w, ffi::fs_close(&mut f))
    };
    fs_check(what, w)?;
    if usize::try_from(w).ok() != Some(d.len()) {
        return Err(Error::new(what, w, "short write"));
    }
    fs_check(what, c)
}

fn statvfs(vol: &CStr) -> Result<ffi::FsStatvfs, Error> {
    let mut st = ffi::FsStatvfs::default();
    // SAFETY: `vol` ends with NUL; `st` outlives the call.
    fs_check("fs_statvfs", unsafe {
        ffi::fs_statvfs(vol.as_ptr(), &mut st)
    })?;
    Ok(st)
}

/// The partition type in a card image's MBR and the FAT type of its
/// volume, 12, 16 or 32, as the file system reports it.
pub fn card_types(img: &mut [u8]) -> Result<(u8, u8), Error> {
    let part_type = *img
        .get(0x1C2)
        .ok_or_else(|| Error::new("card_types", -1, "image too short"))?;
    let fat = with_volume(img, c"probe", None, || statvfs(c"probe"))?.fat_type;
    Ok((part_type, fat))
}

/// Mounts a card image built by [`build_card`] and reads one file back.
pub fn read_back(img: &mut [u8], path: &str) -> Result<Vec<u8>, Error> {
    let p = c_string(path, &format!("back:/{path}"))?;
    with_volume(img, c"back", None, || {
        let mut st = ffi::FsStat::default();
        // SAFETY: `p` ends with NUL; `st` outlives the call.
        fs_check(path, unsafe { ffi::fs_stat(p.as_ptr(), &mut st) })?;
        let mut buf = vec![0u8; st.size as usize];
        let mut n = 0usize;
        // SAFETY: `buf` holds `buf.len()` bytes; `p` and `n` outlive the call.
        let rc =
            unsafe { ffi::fs_read_file(p.as_ptr(), buf.as_mut_ptr().cast(), buf.len(), &mut n) };
        fs_check(path, rc)?;
        buf.truncate(n);
        Ok(buf)
    })
}
