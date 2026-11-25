//! Spike K5: how fast can a Rust host get QEMU's screen?
//!
//! Throwaway code. `main.rs` drives QEMU; this file holds the parts that
//! unit tests can check without QEMU: decoding, frame hashing, statistics,
//! QMP reply parsing, CPU time reading and the VNC message parser.

#![allow(clippy::chunks_exact_to_as_chunks)]
#![allow(unknown_lints)]

use std::io::{self, BufRead, Read, Write};
use std::time::Duration;

/// A decoded frame: RGBA8, row-major, as a window would upload it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32) -> Self {
        Frame {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
        }
    }

    /// A fast 64-bit hash of the pixels, to count distinct frames.
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ ((self.width as u64) << 32 | self.height as u64);
        for c in self.rgba.chunks(8) {
            let mut w = [0u8; 8];
            w[..c.len()].copy_from_slice(c);
            h = (h ^ u64::from_le_bytes(w))
                .wrapping_mul(0x0000_0100_0000_01b3)
                .rotate_left(29);
        }
        h
    }
}

/// Decode a binary PPM (P6, maxval 255) into RGBA.
pub fn decode_ppm(data: &[u8]) -> Result<Frame, String> {
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while i < data.len() && data[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < data.len() && data[i] == b'#' {
            while i < data.len() && data[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let s = i;
        while i < data.len() && !data[i].is_ascii_whitespace() {
            i += 1;
        }
        if s == i {
            return Err("ppm: short header".into());
        }
        fields.push(
            std::str::from_utf8(&data[s..i])
                .map_err(|_| "ppm: bad header")?
                .to_string(),
        );
    }
    i += 1; // one whitespace byte after maxval
    if fields[0] != "P6" || fields[3] != "255" {
        return Err(format!(
            "ppm: unsupported {} maxval {}",
            fields[0], fields[3]
        ));
    }
    let w: u32 = fields[1].parse().map_err(|_| "ppm: bad width")?;
    let h: u32 = fields[2].parse().map_err(|_| "ppm: bad height")?;
    let n = (w * h) as usize;
    let px = data.get(i..i + n * 3).ok_or("ppm: short pixel data")?;
    let mut rgba = Vec::with_capacity(n * 4);
    for p in px.chunks_exact(3) {
        rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
    }
    Ok(Frame {
        width: w,
        height: h,
        rgba,
    })
}

/// Decode a PNG into RGBA with the `png` crate.
pub fn decode_png(data: &[u8]) -> Result<Frame, String> {
    let mut dec = png::Decoder::new(data);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().map_err(|e| format!("png: {e}"))?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).map_err(|e| format!("png: {e}"))?;
    let (w, h) = (info.width, info.height);
    let n = (w * h) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => {
            buf.truncate(n * 4);
            buf
        }
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(n * 4);
            for p in buf[..n * 3].chunks_exact(3) {
                out.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
            out
        }
        t => return Err(format!("png: colour type {t:?} not handled")),
    };
    Ok(Frame {
        width: w,
        height: h,
        rgba,
    })
}

/// Median, 95th centile and worst of a set of durations, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spread {
    pub n: usize,
    pub median: f64,
    pub p95: f64,
    pub max: f64,
}

pub fn spread(samples: &[Duration]) -> Option<Spread> {
    if samples.is_empty() {
        return None;
    }
    let mut v: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    // Nearest rank.
    let rank = |p: f64| v[((p * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1];
    Some(Spread {
        n: v.len(),
        median: rank(0.5),
        p95: rank(0.95),
        max: v[v.len() - 1],
    })
}

impl std::fmt::Display for Spread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "n={} median={:.2}ms p95={:.2}ms max={:.2}ms",
            self.n, self.median, self.p95, self.max
        )
    }
}

/// What one QMP line is.
#[derive(Debug, PartialEq, Eq)]
pub enum QmpLine {
    Return,
    Error(String),
    Event,
    Greeting,
    Other,
}

/// Classify one QMP line without a JSON library. Good enough for the
/// replies this spike reads: `{"return": ...}`, `{"error": ...}`,
/// `{"event": ...}` and the greeting.
pub fn classify(line: &str) -> QmpLine {
    let t = line.trim_start();
    if t.starts_with("{\"return\"") {
        QmpLine::Return
    } else if t.starts_with("{\"error\"") {
        QmpLine::Error(t.to_string())
    } else if t.starts_with("{\"QMP\"") {
        QmpLine::Greeting
    } else if t.contains("\"event\"") {
        QmpLine::Event
    } else {
        QmpLine::Other
    }
}

/// One QMP connection. Commands are sent one at a time and the reply is
/// read before the next one: events in between are skipped.
pub struct Qmp<S: Read + Write> {
    reader: io::BufReader<S>,
    pub events: usize,
}

impl<S: Read + Write> Qmp<S> {
    pub fn new(stream: S) -> io::Result<Self> {
        let mut q = Qmp {
            reader: io::BufReader::new(stream),
            events: 0,
        };
        let mut line = String::new();
        q.reader.read_line(&mut line)?;
        if classify(&line) != QmpLine::Greeting {
            return Err(io::Error::other(format!("qmp: no greeting: {line}")));
        }
        q.cmd("{\"execute\":\"qmp_capabilities\"}")?
            .map_err(io::Error::other)?;
        Ok(q)
    }

    /// Send one command, as raw JSON, and wait for its reply.
    pub fn cmd(&mut self, json: &str) -> io::Result<Result<(), String>> {
        self.send(json)?;
        self.reply()
    }

    pub fn send(&mut self, json: &str) -> io::Result<()> {
        let s = self.reader.get_mut();
        s.write_all(json.as_bytes())?;
        s.write_all(b"\n")?;
        s.flush()
    }

    pub fn reply(&mut self) -> io::Result<Result<(), String>> {
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "qmp closed"));
            }
            match classify(&line) {
                QmpLine::Return => return Ok(Ok(())),
                QmpLine::Error(e) => return Ok(Err(e)),
                QmpLine::Event => self.events += 1,
                _ => {}
            }
        }
    }
}

/// JSON string escaping for a file name in a command.
pub fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Parse `ps -o time=` output from macOS: `[[hh:]mm:]ss.cc`.
pub fn parse_ps_time(s: &str) -> Option<f64> {
    let mut total = 0.0;
    for part in s.trim().split(':') {
        let part = part.split('-').next_back()?; // "dd-hh" is not expected; keep the hour part
        total = total * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(total)
}

/// Parse utime + stime, fields 14 and 15, of a Linux `/proc/<pid>/stat`, in ticks.
pub fn parse_proc_stat(s: &str) -> Option<u64> {
    let rest = &s[s.rfind(')')? + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    // rest starts at field 3 (state), so field 14 is index 11.
    Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
}

/// CPU seconds used so far by a process, user plus system.
pub fn cpu_seconds(pid: u32) -> Option<f64> {
    if cfg!(target_os = "linux") {
        let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let ticks = parse_proc_stat(&s)?;
        Some(ticks as f64 / clk_tck())
    } else if cfg!(unix) {
        let out = std::process::Command::new("ps")
            .args(["-o", "time=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        parse_ps_time(std::str::from_utf8(&out.stdout).ok()?)
    } else {
        None
    }
}

fn clk_tck() -> f64 {
    std::process::Command::new("getconf")
        .arg("CLK_TCK")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(100.0)
}

/// A minimal RFB 3.8 client: no security, 32-bit true colour, Raw encoding
/// only. Enough to count framebuffer updates on loopback.
pub mod vnc {
    use super::Frame;
    use std::io::{self, Read, Write};

    pub struct Client<S: Read + Write> {
        s: S,
        pub frame: Frame,
    }

    fn rd<const N: usize>(s: &mut impl Read) -> io::Result<[u8; N]> {
        let mut b = [0u8; N];
        s.read_exact(&mut b)?;
        Ok(b)
    }

    impl<S: Read + Write> Client<S> {
        pub fn connect(mut s: S) -> io::Result<Self> {
            let ver: [u8; 12] = rd(&mut s)?;
            if &ver[..4] != b"RFB " {
                return Err(io::Error::other("vnc: not RFB"));
            }
            s.write_all(b"RFB 003.008\n")?;
            let n = rd::<1>(&mut s)?[0];
            if n == 0 {
                return Err(io::Error::other("vnc: server refused"));
            }
            let mut types = vec![0u8; n as usize];
            s.read_exact(&mut types)?;
            if !types.contains(&1) {
                return Err(io::Error::other(format!(
                    "vnc: no 'None' security in {types:?}"
                )));
            }
            s.write_all(&[1])?;
            if u32::from_be_bytes(rd(&mut s)?) != 0 {
                return Err(io::Error::other("vnc: security failed"));
            }
            s.write_all(&[1])?; // shared
            let init: [u8; 20] = rd(&mut s)?;
            let w = u16::from_be_bytes([init[0], init[1]]) as u32;
            let h = u16::from_be_bytes([init[2], init[3]]) as u32;
            let name_len = u32::from_be_bytes(rd(&mut s)?) as usize;
            let mut name = vec![0u8; name_len];
            s.read_exact(&mut name)?;
            // SetPixelFormat: 32 bpp, depth 24, little endian, true colour,
            // red at shift 0, green 8, blue 16: bytes come out as R G B x.
            let mut pf = vec![0u8, 0, 0, 0];
            pf.extend_from_slice(&[32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 0, 8, 16, 0, 0, 0]);
            s.write_all(&pf)?;
            // SetEncodings: Raw (0) and DesktopSize (-223).
            let mut enc = vec![2u8, 0, 0, 2];
            enc.extend_from_slice(&0i32.to_be_bytes());
            enc.extend_from_slice(&(-223i32).to_be_bytes());
            s.write_all(&enc)?;
            Ok(Client {
                s,
                frame: Frame::new(w, h),
            })
        }

        pub fn request(&mut self, incremental: bool) -> io::Result<()> {
            let (w, h) = (self.frame.width as u16, self.frame.height as u16);
            let mut m = vec![3u8, incremental as u8, 0, 0, 0, 0];
            m.extend_from_slice(&w.to_be_bytes());
            m.extend_from_slice(&h.to_be_bytes());
            self.s.write_all(&m)
        }

        /// Read server messages until one framebuffer update is complete.
        /// Returns the number of rectangles and the pixel bytes read.
        pub fn read_update(&mut self) -> io::Result<(usize, usize)> {
            loop {
                let t = rd::<1>(&mut self.s)?[0];
                match t {
                    0 => return self.update_body(),
                    1 => {
                        let h: [u8; 5] = rd(&mut self.s)?;
                        let n = u16::from_be_bytes([h[3], h[4]]) as usize;
                        let mut b = vec![0u8; n * 6];
                        self.s.read_exact(&mut b)?;
                    }
                    2 => {}
                    3 => {
                        let h: [u8; 7] = rd(&mut self.s)?;
                        let n = u32::from_be_bytes([h[3], h[4], h[5], h[6]]) as usize;
                        let mut b = vec![0u8; n];
                        self.s.read_exact(&mut b)?;
                    }
                    t => return Err(io::Error::other(format!("vnc: message type {t}"))),
                }
            }
        }

        fn update_body(&mut self) -> io::Result<(usize, usize)> {
            let h: [u8; 3] = rd(&mut self.s)?;
            let n = u16::from_be_bytes([h[1], h[2]]) as usize;
            let mut bytes = 0;
            for _ in 0..n {
                let r: [u8; 12] = rd(&mut self.s)?;
                let x = u16::from_be_bytes([r[0], r[1]]) as u32;
                let y = u16::from_be_bytes([r[2], r[3]]) as u32;
                let w = u16::from_be_bytes([r[4], r[5]]) as u32;
                let hh = u16::from_be_bytes([r[6], r[7]]) as u32;
                let enc = i32::from_be_bytes([r[8], r[9], r[10], r[11]]);
                match enc {
                    0 => {
                        let mut px = vec![0u8; (w * hh * 4) as usize];
                        self.s.read_exact(&mut px)?;
                        bytes += px.len();
                        apply_raw(&mut self.frame, x, y, w, hh, &px);
                    }
                    -223 => self.frame = Frame::new(w, hh),
                    e => return Err(io::Error::other(format!("vnc: encoding {e}"))),
                }
            }
            Ok((n, bytes))
        }
    }

    /// Copy a Raw rectangle of R G B x pixels into the frame as RGBA.
    pub fn apply_raw(f: &mut Frame, x: u32, y: u32, w: u32, h: u32, px: &[u8]) {
        for row in 0..h {
            let fy = y + row;
            if fy >= f.height {
                break;
            }
            for col in 0..w {
                let fx = x + col;
                if fx >= f.width {
                    break;
                }
                let s = ((row * w + col) * 4) as usize;
                let d = ((fy * f.width + fx) * 4) as usize;
                f.rgba[d..d + 3].copy_from_slice(&px[s..s + 3]);
                f.rgba[d + 3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ppm(w: u32, h: u32, px: &[u8]) -> Vec<u8> {
        let mut v = format!("P6\n# c\n{w} {h}\n255\n").into_bytes();
        v.extend_from_slice(px);
        v
    }

    #[test]
    fn ppm_decodes_to_rgba() {
        let f = decode_ppm(&ppm(2, 1, &[1, 2, 3, 4, 5, 6])).unwrap();
        assert_eq!((f.width, f.height), (2, 1));
        assert_eq!(f.rgba, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn ppm_refuses_short_and_wrong() {
        assert!(decode_ppm(&ppm(2, 2, &[0; 5])).is_err());
        assert!(decode_ppm(b"P5\n1 1\n255\n\0").is_err());
        assert!(decode_ppm(b"P6\n1").is_err());
    }

    #[test]
    fn png_round_trip() {
        let mut data = Vec::new();
        {
            let mut e = png::Encoder::new(&mut data, 2, 1);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            e.write_header()
                .unwrap()
                .write_image_data(&[9, 8, 7, 6, 5, 4])
                .unwrap();
        }
        let f = decode_png(&data).unwrap();
        assert_eq!(f.rgba, vec![9, 8, 7, 255, 6, 5, 4, 255]);
    }

    #[test]
    fn hash_tells_frames_apart() {
        let a = Frame::new(4, 4);
        let mut b = a.clone();
        assert_eq!(a.hash(), b.hash());
        b.rgba[37] = 1;
        assert_ne!(a.hash(), b.hash());
    }

    #[test]
    fn spread_uses_nearest_rank() {
        let d: Vec<Duration> = (1..=20).map(Duration::from_millis).collect();
        let s = spread(&d).unwrap();
        assert_eq!((s.n, s.median, s.p95, s.max), (20, 10.0, 19.0, 20.0));
        assert!(spread(&[]).is_none());
    }

    #[test]
    fn qmp_lines() {
        assert_eq!(classify("{\"return\": {}}"), QmpLine::Return);
        assert!(matches!(
            classify("{\"error\": {\"class\": \"x\"}}"),
            QmpLine::Error(_)
        ));
        assert_eq!(
            classify("{\"timestamp\": {}, \"event\": \"STOP\"}"),
            QmpLine::Event
        );
        assert_eq!(classify("{\"QMP\": {}}"), QmpLine::Greeting);
    }

    #[test]
    fn qmp_skips_events() {
        let script = "{\"QMP\": {}}\n{\"return\": {}}\n{\"event\": \"X\"}\n{\"return\": {}}\n";
        let s = io::Cursor::new(script.as_bytes().to_vec());
        let mut q = Qmp::new(Duplex(s, Vec::new())).unwrap();
        assert_eq!(q.cmd("{}").unwrap(), Ok(()));
        assert_eq!(q.events, 1);
    }

    struct Duplex(io::Cursor<Vec<u8>>, Vec<u8>);
    impl Read for Duplex {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.0.read(b)
        }
    }
    impl Write for Duplex {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.1.write(b)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn json_escape() {
        assert_eq!(json_str("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn cpu_time_parsers() {
        assert_eq!(parse_ps_time("0:01.50"), Some(1.5));
        assert_eq!(parse_ps_time("1:02:03.00"), Some(3723.0));
        let stat = "123 (qemu sys) S 1 2 3 4 5 6 7 8 9 10 250 50 0 0";
        assert_eq!(parse_proc_stat(stat), Some(300));
    }

    #[test]
    fn vnc_handshake_and_raw_update() {
        let mut srv = Vec::new();
        srv.extend_from_slice(b"RFB 003.008\n");
        srv.extend_from_slice(&[1, 1]); // one security type: None
        srv.extend_from_slice(&0u32.to_be_bytes());
        srv.extend_from_slice(&[0, 2, 0, 1]); // 2 x 1
        srv.extend_from_slice(&[0; 16]);
        srv.extend_from_slice(&1u32.to_be_bytes());
        srv.push(b'q');
        // One update, one Raw rectangle at (1, 0), 1 x 1.
        srv.extend_from_slice(&[0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0]);
        srv.extend_from_slice(&[10, 20, 30, 0]);
        let mut c = vnc::Client::connect(Duplex(io::Cursor::new(srv), Vec::new())).unwrap();
        assert_eq!(c.read_update().unwrap(), (1, 4));
        assert_eq!(c.frame.rgba, vec![0, 0, 0, 0, 10, 20, 30, 255]);
    }
}
