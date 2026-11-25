//! The raw write and the read-back. It takes an [`Approved`] disk and nothing else.

use crate::rules::Approved;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Instant;

/// Chunks are 1 MiB, a multiple of every sector size in use (512 and 4096).
const CHUNK: usize = 1024 * 1024;
/// The image is padded with zeros to this multiple. Raw devices refuse partial sectors.
const ALIGN: usize = 4096;

#[derive(Debug)]
pub struct Outcome {
    pub raw_path: String,
    pub bytes: u64,
    pub write_ms: u128,
    pub read_ms: u128,
    /// The first byte offset that differed, if any.
    pub mismatch: Option<u64>,
}

/// The path that gives unbuffered access: `/dev/rdiskN` on macOS.
pub fn raw_path(path: &str) -> String {
    if cfg!(target_os = "macos")
        && let Some(rest) = path.strip_prefix("/dev/disk")
    {
        return format!("/dev/rdisk{rest}");
    }
    path.to_string()
}

fn read_full(f: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

fn padded(n: usize) -> usize {
    n.div_ceil(ALIGN) * ALIGN
}

#[cfg(target_os = "linux")]
fn open_write(p: &str) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    // O_EXCL on a block device fails with EBUSY while anything has it mounted.
    OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_EXCL)
        .open(p)
}

#[cfg(not(target_os = "linux"))]
fn open_write(p: &str) -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open(p)
}

#[cfg(target_os = "linux")]
fn open_read(p: &str) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    // O_DIRECT, so the read-back comes from the device and not from the page cache.
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECT)
        .open(p)
}

#[cfg(not(target_os = "linux"))]
fn open_read(p: &str) -> io::Result<File> {
    File::open(p)
}

/// A buffer whose start is aligned to [`ALIGN`], as O_DIRECT wants, without unsafe code.
struct Aligned {
    store: Vec<u8>,
    off: usize,
}

impl Aligned {
    fn new(len: usize) -> Self {
        let store = vec![0u8; len + ALIGN];
        let off = store.as_ptr().align_offset(ALIGN);
        Aligned { store, off }
    }
    fn get(&mut self, len: usize) -> &mut [u8] {
        &mut self.store[self.off..self.off + len]
    }
}

/// Writes `image` to the approved disk, flushes, reads it back and compares every byte.
pub fn write_and_verify(target: Approved, image: &Path) -> io::Result<Outcome> {
    let raw = raw_path(target.path());
    let mut img = File::open(image)?;
    let len = img.metadata()?.len();
    if padded(len as usize) as u64 > target.size() {
        return Err(io::Error::other("image does not fit on the disk"));
    }
    eprintln!("opening {raw} for writing");
    let t0 = Instant::now();
    let mut dev = open_write(&raw)?;
    let mut buf = vec![0u8; CHUNK];
    let mut bytes = 0u64;
    loop {
        let n = read_full(&mut img, &mut buf)?;
        if n == 0 {
            break;
        }
        let m = padded(n);
        buf[n..m].fill(0);
        dev.write_all(&buf[..m])?;
        bytes += m as u64;
        if n < CHUNK {
            break;
        }
    }
    flush(&dev)?;
    drop(dev);
    let write_ms = t0.elapsed().as_millis();
    let t1 = Instant::now();
    let mismatch = compare(&raw, image, bytes)?;
    Ok(Outcome {
        raw_path: raw,
        bytes,
        write_ms,
        read_ms: t1.elapsed().as_millis(),
        mismatch,
    })
}

/// Reads `bytes` from a device and compares them with the image padded with zeros.
/// Read only. Returns the first differing offset.
pub fn compare(device: &str, image: &Path, bytes: u64) -> io::Result<Option<u64>> {
    let mut dev = open_read(device)?;
    let mut img = File::open(image)?;
    let mut want = vec![0u8; CHUNK];
    let mut got = Aligned::new(CHUNK);
    let mut off = 0u64;
    while off < bytes {
        let len = ((bytes - off) as usize).min(CHUNK);
        let n = read_full(&mut img, &mut want[..len])?;
        want[n..len].fill(0);
        let g = got.get(len);
        let k = read_full(&mut dev, g)?;
        if k < len {
            return Ok(Some(off + k as u64));
        }
        if let Some(i) = g.iter().zip(&want[..len]).position(|(a, b)| a != b) {
            return Ok(Some(off + i as u64));
        }
        off += len as u64;
    }
    Ok(None)
}

/// A pseudo-random test image, the same for the same seed.
pub fn make_image(out: &Path, size: usize, seed: u64) -> io::Result<()> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut data = Vec::with_capacity(size);
    while data.len() < size {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        data.extend_from_slice(&x.to_le_bytes());
    }
    data.truncate(size);
    std::fs::write(out, data)
}

/// Flushes the device. On macOS, `sync_all` is `F_FULLFSYNC`, which a raw disk node
/// refuses with ENOTTY: rdisk writes are unbuffered by the system, and the drive's own
/// cache would need the DKIOCSYNCHRONIZECACHE ioctl, which this spike does not call.
fn flush(dev: &File) -> io::Result<()> {
    match dev.sync_all() {
        Err(e) if cfg!(target_os = "macos") && e.raw_os_error() == Some(25) => {
            eprintln!("flush: F_FULLFSYNC not supported on a raw disk, continuing");
            Ok(())
        }
        r => r,
    }
}
