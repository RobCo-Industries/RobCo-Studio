//! The raw write and the read-back. It takes an [`Approved`] disk and nothing else.

use crate::rules::{Approved, padded_len};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Instant;

/// Chunks are 1 MiB, a multiple of every sector size in use (512 and 4096).
const CHUNK: usize = 1024 * 1024;
/// The read buffer starts at this alignment, as O_DIRECT wants for both sector sizes.
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

fn read_full<R: Read>(f: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// `n` padded with zeros to whole sectors. Raw devices refuse partial sectors.
fn padded(n: usize, sector: u32) -> usize {
    padded_len(n as u64, sector) as usize
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

/// Copies exactly `len` bytes from `src` to `dst`, the last chunk padded with zeros to
/// whole sectors. Fails if `src` ends before `len` or still has bytes after it. The check for
/// extra bytes runs before the last chunk is written. Returns the bytes written.
pub fn copy_exact<R: Read, W: Write>(
    src: &mut R,
    dst: &mut W,
    len: u64,
    sector: u32,
) -> io::Result<u64> {
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    let mut written = 0u64;
    while done < len {
        let want = ((len - done) as usize).min(CHUNK);
        let n = read_full(src, &mut buf[..want])?;
        if n < want {
            return Err(io::Error::other(format!(
                "image ended at byte {}, before the approved length {len}",
                done + n as u64
            )));
        }
        done += n as u64;
        if done == len {
            ensure_ended(src, len)?;
        }
        let m = padded(n, sector);
        buf[n..m].fill(0);
        dst.write_all(&buf[..m])?;
        written += m as u64;
    }
    if len == 0 {
        ensure_ended(src, len)?;
    }
    Ok(written)
}

fn ensure_ended<R: Read>(src: &mut R, len: u64) -> io::Result<()> {
    let mut one = [0u8; 1];
    if read_full(src, &mut one)? != 0 {
        return Err(io::Error::other(format!(
            "image is longer than the approved length {len}"
        )));
    }
    Ok(())
}

/// Compares `padded(len)` bytes of `dev` with exactly `len` bytes of `img` followed by
/// zeros. Fails if the image is shorter or longer than `len`. Returns the first offset
/// that differs, or the offset where the device ended early.
pub fn compare_streams<D: Read, I: Read>(
    dev: &mut D,
    img: &mut I,
    len: u64,
    sector: u32,
) -> io::Result<Option<u64>> {
    let total = padded_len(len, sector);
    let mut want = vec![0u8; CHUNK];
    let mut got = Aligned::new(CHUNK);
    let mut off = 0u64;
    while off < total {
        let chunk = ((total - off) as usize).min(CHUNK);
        let from_img = (len.saturating_sub(off) as usize).min(chunk);
        let n = read_full(img, &mut want[..from_img])?;
        if n < from_img {
            return Err(io::Error::other(format!(
                "image ended at byte {}, before the approved length {len}",
                off + n as u64
            )));
        }
        want[from_img..chunk].fill(0);
        let g = got.get(chunk);
        let k = read_full(dev, g)?;
        if k < chunk {
            return Ok(Some(off + k as u64));
        }
        if let Some(i) = g.iter().zip(&want[..chunk]).position(|(a, b)| a != b) {
            return Ok(Some(off + i as u64));
        }
        off += chunk as u64;
    }
    ensure_ended(img, len)?;
    Ok(None)
}

/// Writes the image to the approved disk, flushes, reads it back and compares.
///
/// `image` is the one open handle the length was taken from: the image path is not
/// opened again, so a file swapped at that path is never read. Exactly
/// `target.image_len()` bytes are written. `recheck` runs after the disk is open and
/// before the first byte: it lists the disks again and must find the same disk with the
/// same size.
pub fn write_and_verify(
    target: Approved,
    image: &mut File,
    recheck: impl FnOnce(&Approved) -> Result<(), String>,
) -> io::Result<Outcome> {
    let raw = raw_path(target.path());
    let len = target.image_len();
    let sector = target.block_size();
    if padded_len(len, sector) > target.size() {
        return Err(io::Error::other("image does not fit on the disk"));
    }
    image.seek(SeekFrom::Start(0))?;
    eprintln!("opening {raw} for writing");
    let t0 = Instant::now();
    let mut dev = open_write(&raw)?;
    recheck(&target).map_err(|e| io::Error::other(format!("disk changed after the check: {e}")))?;
    device_size_matches(&mut dev, target.size())?;
    let bytes = copy_exact(image, &mut dev, len, sector)?;
    flush(&dev)?;
    drop(dev);
    let write_ms = t0.elapsed().as_millis();
    let t1 = Instant::now();
    image.seek(SeekFrom::Start(0))?;
    let mut d = open_read(&raw)?;
    let mismatch = compare_streams(&mut d, image, len, sector)?;
    Ok(Outcome {
        raw_path: raw,
        bytes,
        write_ms,
        read_ms: t1.elapsed().as_millis(),
        mismatch,
    })
}

/// On Linux a block device reports its size through a seek to the end. Other systems
/// need an ioctl for that, which the spike does not call: there the recheck is the guard.
#[cfg(target_os = "linux")]
fn device_size_matches(dev: &mut File, want: u64) -> io::Result<()> {
    let got = dev.seek(SeekFrom::End(0))?;
    dev.seek(SeekFrom::Start(0))?;
    if got != want {
        return Err(io::Error::other(format!(
            "disk size is {got} bytes, the check saw {want}"
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn device_size_matches(_dev: &mut File, _want: u64) -> io::Result<()> {
    Ok(())
}

/// Reads a device and compares it with exactly `len` bytes of the image, padded with
/// zeros. Read only. Returns the first differing offset.
pub fn compare(device: &str, image: &mut File, len: u64, sector: u32) -> io::Result<Option<u64>> {
    let mut dev = open_read(device)?;
    compare_streams(&mut dev, image, len, sector)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn image(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8 + 1).collect()
    }

    #[test]
    fn writes_exactly_the_approved_length() {
        let len = CHUNK + 1000;
        let mut out = Vec::new();
        let w = copy_exact(&mut Cursor::new(image(len)), &mut out, len as u64, 4096).unwrap();
        assert_eq!(w as usize, padded(len, 4096));
        assert_eq!(&out[..len], &image(len)[..]);
        assert!(out[len..].iter().all(|&b| b == 0));
    }

    #[test]
    fn an_image_that_grew_after_the_check_is_refused() {
        // Checked at 1000 bytes, the file now holds 3 MiB.
        let mut out = Vec::new();
        let e = copy_exact(&mut Cursor::new(image(3 * CHUNK)), &mut out, 1000, 4096).unwrap_err();
        assert!(e.to_string().contains("longer"), "{e}");
        assert!(out.is_empty(), "nothing past the check may reach the disk");
        // Grown by one byte past a whole chunk: the last chunk is not written.
        let mut out = Vec::new();
        assert!(
            copy_exact(
                &mut Cursor::new(image(CHUNK * 2 + 1)),
                &mut out,
                CHUNK as u64 * 2,
                4096
            )
            .is_err()
        );
        assert!(out.len() <= CHUNK);
    }

    #[test]
    fn an_image_that_shrank_after_the_check_is_refused() {
        let mut out = Vec::new();
        let e = copy_exact(
            &mut Cursor::new(image(500)),
            &mut out,
            2 * CHUNK as u64,
            4096,
        )
        .unwrap_err();
        assert!(e.to_string().contains("ended"), "{e}");
        assert!(out.is_empty());
    }

    #[test]
    fn read_back_compares_exactly_the_approved_length() {
        let len = 5000usize;
        let mut disk = image(len);
        disk.resize(padded(len, 4096), 0);
        assert_eq!(
            compare_streams(
                &mut Cursor::new(&disk),
                &mut Cursor::new(image(len)),
                len as u64,
                4096
            )
            .unwrap(),
            None
        );
        // The image grew after the write: an error, not a silent pass on the prefix.
        assert!(
            compare_streams(
                &mut Cursor::new(&disk),
                &mut Cursor::new(image(len + 1)),
                len as u64,
                4096
            )
            .is_err()
        );
        // The image shrank after the write.
        assert!(
            compare_streams(
                &mut Cursor::new(&disk),
                &mut Cursor::new(image(len - 1)),
                len as u64,
                4096
            )
            .is_err()
        );
        // A changed byte, and a device that ends early.
        disk[42] ^= 1;
        assert_eq!(
            compare_streams(
                &mut Cursor::new(&disk),
                &mut Cursor::new(image(len)),
                len as u64,
                4096
            )
            .unwrap(),
            Some(42)
        );
        assert_eq!(
            compare_streams(
                &mut Cursor::new(&disk[..100]),
                &mut Cursor::new(image(len)),
                len as u64,
                4096
            )
            .unwrap(),
            Some(100)
        );
    }
}
