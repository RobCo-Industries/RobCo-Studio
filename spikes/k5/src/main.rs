//! Spike K5 benchmark. Starts one QEMU, boots the animated guest, and runs
//! timed phases against it. Prints one `phase` line per phase.
//!
//!   k5 screendump --board raspi2b --guest k5.img --fwstub fwstub.bin --tmp DIR
//!   k5 vnc        --board raspi2b --guest k5.img --fwstub fwstub.bin
//!
//! QEMU is killed after 20 seconds whatever happens. No path, user or host
//! name is printed.

#![allow(clippy::needless_late_init)]

use k5::{cpu_seconds, decode_png, decode_ppm, json_str, spread, vnc, Frame, Qmp};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const RUN_LIMIT: Duration = Duration::from_secs(20);
const KEY_EVERY: Duration = Duration::from_millis(50);

struct Opts {
    mode: String,
    board: String,
    guest: PathBuf,
    fwstub: PathBuf,
    tmp: PathBuf,
    qemu_dir: Option<PathBuf>,
    phase: Duration,
    w: u32,
    h: u32,
}

fn opts() -> Opts {
    let a: Vec<String> = std::env::args().collect();
    let get = |k: &str| {
        a.iter()
            .position(|x| x == k)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    let mode = a.get(1).cloned().unwrap_or_default();
    if mode != "screendump" && mode != "vnc" {
        eprintln!("usage: k5 screendump|vnc --board B --guest IMG --fwstub BIN [--size WxH] [--tmp DIR] [--qemu-dir DIR] [--phase SECS]");
        std::process::exit(2);
    }
    let size = get("--size").unwrap_or_else(|| "640x480".into());
    let (w, h) = size.split_once('x').expect("--size WxH");
    Opts {
        mode,
        board: get("--board").expect("--board"),
        guest: get("--guest").expect("--guest").into(),
        fwstub: get("--fwstub").expect("--fwstub").into(),
        tmp: get("--tmp")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir),
        qemu_dir: get("--qemu-dir").map(PathBuf::from),
        phase: Duration::from_secs_f64(get("--phase").map(|s| s.parse().unwrap()).unwrap_or(2.0)),
        w: w.parse().unwrap(),
        h: h.parse().unwrap(),
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn free_vnc_port() -> u16 {
    (5950..6100)
        .find(|p| TcpListener::bind(("127.0.0.1", *p)).is_ok())
        .expect("no free VNC port")
}

/// The boot the suite's harness does (tests/harness/boot.py), with TCP.
fn boot_args(o: &Opts) -> (String, Vec<String>) {
    let a64 = o.board == "raspi3b";
    let (mach, mem, flags) = match o.board.as_str() {
        "raspi0" | "raspi1ap" => (0xC42u32, 512u32 << 20, 0u32),
        "raspi2b" => (0xC43, 1024 << 20, 1),
        "raspi3b" => (0xC43, 1024 << 20, 0),
        b => panic!("unknown board {b}"),
    };
    let bin = if a64 {
        "qemu-system-aarch64"
    } else {
        "qemu-system-arm"
    };
    let bin = match &o.qemu_dir {
        Some(d) => d.join(bin).to_string_lossy().into_owned(),
        None => bin.to_string(),
    };
    let mut v = vec![
        "-device".into(),
        format!("loader,file={},addr=0x0,force-raw=on", o.fwstub.display()),
        "-device".into(),
        format!("loader,file={},addr=0x8000,force-raw=on", o.guest.display()),
    ];
    let words = |addr: u32, w: &[u32], v: &mut Vec<String>| {
        for (i, x) in w.iter().enumerate() {
            v.push("-device".into());
            v.push(format!(
                "loader,addr={:#x},data={:#010x},data-len=4",
                addr + 4 * i as u32,
                x
            ));
        }
    };
    words(0x7F00, &[0x5746_4352, mach, mem, flags], &mut v);
    if a64 {
        // tests/fwstub/a64_shim.py, HYP mode.
        let shim = [
            0xD280_2620,
            0xD51E_1100,
            0xD280_3B40,
            0xD51E_4000,
            0xD280_0000,
            0xD51E_4020,
            0xD69F_03E0,
        ];
        words(0x80000, &shim, &mut v);
        v.push("-device".into());
        v.push("loader,addr=0x80000,cpu-num=0".into());
        for cpu in 1..=3 {
            v.push("-device".into());
            v.push(format!("loader,addr=0x80000,cpu-num={cpu}"));
        }
    }
    (bin, v)
}

/// The QEMU child, so that every exit path can stop it.
static QEMU: OnceLock<Arc<Mutex<Child>>> = OnceLock::new();
/// Local paths to hide from anything printed: (path, label).
static HIDE: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();

fn hide(s: &str) -> String {
    let mut s = s.to_string();
    for (p, l) in HIDE.get().map(|v| v.as_slice()).unwrap_or(&[]) {
        if !p.is_empty() {
            s = s.replace(p.as_str(), l);
        }
    }
    s
}

/// Kills QEMU when main returns or panics.
struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(c) = QEMU.get() {
            if let Ok(mut c) = c.lock() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
}

/// If QEMU has exited, print why and stop.
fn check_qemu() {
    let Some(c) = QEMU.get() else { return };
    let mut c = c.lock().unwrap();
    if let Ok(Some(st)) = c.try_wait() {
        let mut e = String::new();
        if let Some(mut s) = c.stderr.take() {
            let _ = s.read_to_string(&mut e);
        }
        if e.contains("-vnc: invalid option") {
            // A QEMU built without its VNC server: a fact about the build.
            println!("NOT-RUN: this QEMU has no VNC server: {}", hide(e.trim()));
            std::process::exit(0);
        }
        println!("FAIL: QEMU exited early ({st}): {}", hide(e.trim()));
        std::process::exit(1);
    }
}

fn connect(port: u16) -> TcpStream {
    let t0 = Instant::now();
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => {
                s.set_nodelay(true).ok();
                s.set_read_timeout(Some(Duration::from_secs(10))).ok();
                return s;
            }
            Err(e) if t0.elapsed() > Duration::from_secs(10) => panic!("connect: {e}"),
            Err(_) => {
                check_qemu();
                thread::sleep(Duration::from_millis(20))
            }
        }
    }
}

/// Guest progress markers: (arrival, frames drawn, guest ms).
type Marks = Arc<Mutex<Vec<(Instant, u64, u64)>>>;

fn serial_reader(s: TcpStream, marks: Marks, ready: Arc<AtomicBool>) {
    s.set_read_timeout(None).ok();
    let mut r = BufReader::new(s);
    let mut line = String::new();
    loop {
        line.clear();
        match r.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.contains("ev=ready") {
            ready.store(true, Ordering::SeqCst);
        }
        if line.contains("@RC stage=k5 ev=error") {
            println!("guest error: {}", line.trim());
        }
        if let Some(i) = line.find("ev=rate ") {
            let mut it = None;
            let mut ms = None;
            for kv in line[i..].split_whitespace() {
                if let Some(v) = kv.strip_prefix("it=") {
                    it = v.parse().ok();
                }
                if let Some(v) = kv.strip_prefix("ms=") {
                    ms = v.parse().ok();
                }
            }
            if let (Some(it), Some(ms)) = (it, ms) {
                marks.lock().unwrap().push((Instant::now(), it, ms));
            }
        }
    }
}

/// Guest frames drawn per host second, and guest ms per host second, in a window.
fn guest_rate(marks: &Marks, t0: Instant, t1: Instant) -> String {
    let m = marks.lock().unwrap();
    let w: Vec<_> = m.iter().filter(|(t, _, _)| *t >= t0 && *t <= t1).collect();
    if w.len() < 2 {
        return "guest=not-enough-markers".into();
    }
    let (a, b) = (w[0], w[w.len() - 1]);
    let dt = (b.0 - a.0).as_secs_f64();
    // The guest counter is 32 bits: wrapping_sub keeps the delta right.
    let dit = (b.1 as u32).wrapping_sub(a.1 as u32) as f64;
    format!(
        "guest_draws_per_s={:.0} guest_clock={:.2}",
        dit / dt,
        (b.2 - a.2) as f64 / 1000.0 / dt
    )
}

struct Cpu {
    pid: u32,
    t: Instant,
    q: Option<f64>,
    me: Option<f64>,
}

impl Cpu {
    fn start(pid: u32) -> Cpu {
        Cpu {
            pid,
            t: Instant::now(),
            q: cpu_seconds(pid),
            me: cpu_seconds(std::process::id()),
        }
    }
    fn end(&self) -> String {
        let dt = self.t.elapsed().as_secs_f64();
        let pct = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => format!("{:.0}%", (b - a) / dt * 100.0),
            _ => "n/a".into(),
        };
        format!(
            "qemu_cpu={} poller_cpu={}",
            pct(self.q, cpu_seconds(self.pid)),
            pct(self.me, cpu_seconds(std::process::id()))
        )
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Sink {
    File,
    Fifo,
}

#[derive(Clone, Copy, PartialEq)]
enum Keys {
    None,
    Same,
    Second,
}

struct Ctx {
    pid: u32,
    marks: Marks,
    q1: Qmp<TcpStream>,
    q2: Arc<Mutex<Qmp<TcpStream>>>,
    tmp: PathBuf,
    w: u32,
    h: u32,
    failed: Vec<String>,
}

const SEND_KEY: &str =
    r#"{"execute":"send-key","arguments":{"keys":[{"type":"qcode","data":"a"}]}}"#;

/// Send a key every 50 ms on the second connection until `stop`.
fn key_thread(
    q2: Arc<Mutex<Qmp<TcpStream>>>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<(Vec<Duration>, usize)> {
    thread::spawn(move || {
        let mut lat = Vec::new();
        let mut errors = 0;
        let mut q = q2.lock().unwrap();
        let mut due = Instant::now();
        while !stop.load(Ordering::SeqCst) {
            let now = Instant::now();
            if now < due {
                thread::sleep(due - now);
                continue;
            }
            let t = Instant::now();
            if q.cmd(SEND_KEY).expect("qmp2").is_err() {
                errors += 1;
            }
            lat.push(t.elapsed());
            due += KEY_EVERY;
        }
        (lat, errors)
    })
}

fn fmt_spread(name: &str, d: &[Duration]) -> String {
    match spread(d) {
        Some(s) => format!("{name}[{s}]"),
        None => format!("{name}[none]"),
    }
}

fn idle(c: &mut Ctx, dur: Duration) {
    let cpu = Cpu::start(c.pid);
    let t0 = Instant::now();
    let stop = Arc::new(AtomicBool::new(false));
    let kt = key_thread(c.q2.clone(), stop.clone());
    thread::sleep(dur);
    stop.store(true, Ordering::SeqCst);
    let (lat, errs) = kt.join().unwrap();
    let t1 = Instant::now();
    println!(
        "phase=idle {} {} key_conn=second {} key_errors={}",
        cpu.end(),
        guest_rate(&c.marks, t0, t1),
        fmt_spread("key_latency", &lat),
        errs
    );
}

fn mkfifo(p: &Path) -> bool {
    let _ = std::fs::remove_file(p);
    Command::new("mkfifo")
        .arg(p)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn poll(
    c: &mut Ctx,
    name: &str,
    fmt: &str,
    target: Option<f64>,
    sink: Sink,
    keys: Keys,
    dur: Duration,
) {
    let ext = if fmt == "png" { "png" } else { "ppm" };
    let path = c
        .tmp
        .join(format!("k5-{}-{name}.{ext}", std::process::id()));
    if sink == Sink::Fifo && !mkfifo(&path) {
        println!("phase={name} not-run: mkfifo failed");
        return;
    }
    let cmd = format!(
        r#"{{"execute":"screendump","arguments":{{"filename":{},"format":"{fmt}"}}}}"#,
        json_str(&path.to_string_lossy())
    );
    let stop = Arc::new(AtomicBool::new(false));
    let kt = (keys == Keys::Second).then(|| key_thread(c.q2.clone(), stop.clone()));
    let cpu = Cpu::start(c.pid);
    let t0 = Instant::now();
    let end = t0 + dur;
    let period = target.map(|f| Duration::from_secs_f64(1.0 / f));
    let mut next = t0;
    let (mut rtt, mut dec, mut total) = (Vec::new(), Vec::new(), Vec::new());
    let (mut frames, mut changes, mut late, mut bytes) = (0usize, 0usize, 0usize, 0usize);
    let mut uniq = HashSet::new();
    let mut last = None;
    let mut key_due = t0;
    let (mut key_lat, mut key_err) = (Vec::new(), 0usize);
    let mut err = None;
    while Instant::now() < end {
        if let Some(p) = period {
            let now = Instant::now();
            if now < next {
                thread::sleep(next - now);
            } else if now > next + p {
                late += 1;
            }
            next += p;
            if next < Instant::now() {
                next = Instant::now();
            }
        }
        let t_req = Instant::now();
        let data;
        let t_rep;
        if sink == Sink::Fifo {
            c.q1.send(&cmd).unwrap();
            let mut f = std::fs::File::open(&path).unwrap();
            let mut d = Vec::new();
            f.read_to_end(&mut d).unwrap();
            if let Err(e) = c.q1.reply().unwrap() {
                err = Some(e);
                break;
            }
            t_rep = Instant::now();
            data = d;
        } else {
            if let Err(e) = c.q1.cmd(&cmd).unwrap() {
                err = Some(e);
                break;
            }
            t_rep = Instant::now();
            data = std::fs::read(&path).unwrap();
        }
        bytes = data.len();
        let f: Frame = match if fmt == "png" {
            decode_png(&data)
        } else {
            decode_ppm(&data)
        } {
            Ok(f) => f,
            Err(e) => {
                err = Some(e);
                break;
            }
        };
        let t_dec = Instant::now();
        if (f.width, f.height) != (c.w, c.h) {
            err = Some(format!(
                "frame is {}x{}, expected {}x{}",
                f.width, f.height, c.w, c.h
            ));
            break;
        }
        let h = f.hash();
        if last.is_some_and(|l| l != h) {
            changes += 1;
        }
        last = Some(h);
        uniq.insert(h);
        frames += 1;
        rtt.push(t_rep - t_req);
        dec.push(t_dec - t_rep);
        total.push(t_dec - t_req);
        if keys == Keys::Same && Instant::now() >= key_due {
            if c.q1.cmd(SEND_KEY).unwrap().is_err() {
                key_err += 1;
            }
            key_lat.push(key_due.elapsed());
            key_due += KEY_EVERY;
        }
    }
    let t1 = Instant::now();
    let wall = (t1 - t0).as_secs_f64();
    stop.store(true, Ordering::SeqCst);
    if let Some(kt) = kt {
        let (l, e) = kt.join().unwrap();
        key_lat = l;
        key_err = e;
    }
    let _ = std::fs::remove_file(&path);
    if let Some(e) = err {
        let e = hide(&e);
        if frames == 0 && e.contains("Enable PNG support") {
            // A QEMU built without libpng: a fact about the build, not a fault.
            println!("phase={name} not-run: this QEMU has no PNG screendump: {e}");
            return;
        }
        println!("phase={name} error: {e}");
        c.failed.push(format!("{name}: {e}"));
        return;
    }
    let tgt = target
        .map(|t| format!("{t}"))
        .unwrap_or_else(|| "max".into());
    let mut line = format!(
        "phase={name} format={fmt} sink={} target={tgt} frames={frames} fps={:.1} changes={changes} unique={} late={late} bytes={bytes} {} {} {} {} {}",
        if sink == Sink::Fifo { "fifo" } else { "file" },
        frames as f64 / wall,
        uniq.len(),
        cpu.end(),
        guest_rate(&c.marks, t0, t1),
        fmt_spread("reply", &rtt),
        fmt_spread("read_decode", &dec),
        fmt_spread("total", &total),
    );
    if keys != Keys::None {
        line += &format!(
            " key_conn={} {} key_errors={key_err}",
            if keys == Keys::Same { "same" } else { "second" },
            fmt_spread("key_latency", &key_lat)
        );
    }
    println!("{line}");
    if frames < 2 || changes == 0 {
        println!("FAIL: phase {name}: every frame was the same ({frames} frames): the benchmark measured a still picture");
        c.failed.push(format!("{name}: still picture"));
    }
}

fn vnc_phase(c: &mut Ctx, port: u16, name: &str, target: Option<f64>, dur: Duration) {
    let s = connect(port);
    let mut v = vnc::Client::connect(s).expect("vnc connect");
    let cpu = Cpu::start(c.pid);
    let t0 = Instant::now();
    let end = t0 + dur;
    let period = target.map(|f| Duration::from_secs_f64(1.0 / f));
    let mut next = t0;
    let (mut lat, mut frames, mut changes, mut rects, mut bytes) =
        (Vec::new(), 0usize, 0usize, 0usize, 0usize);
    let mut uniq = HashSet::new();
    let mut last = None;
    let mut first = true;
    while Instant::now() < end {
        if let Some(p) = period {
            let now = Instant::now();
            if now < next {
                thread::sleep(next - now);
            }
            next += p;
            if next < Instant::now() {
                next = Instant::now();
            }
        }
        let t = Instant::now();
        v.request(!first).unwrap();
        first = false;
        let (r, b) = v.read_update().unwrap();
        lat.push(t.elapsed());
        rects += r;
        bytes += b;
        if r == 0 {
            continue;
        }
        let h = v.frame.hash();
        if last.is_some_and(|l| l != h) {
            changes += 1;
        }
        last = Some(h);
        uniq.insert(h);
        frames += 1;
    }
    let t1 = Instant::now();
    let wall = (t1 - t0).as_secs_f64();
    let tgt = target
        .map(|t| format!("{t}"))
        .unwrap_or_else(|| "max".into());
    println!(
        "phase={name} target={tgt} updates={frames} fps={:.1} changes={changes} unique={} rects={rects} bytes_per_s={:.0} size={}x{} {} {} {}",
        frames as f64 / wall,
        uniq.len(),
        bytes as f64 / wall,
        v.frame.width,
        v.frame.height,
        cpu.end(),
        guest_rate(&c.marks, t0, t1),
        fmt_spread("request_to_update", &lat),
    );
    if frames < 2 || changes == 0 {
        println!("FAIL: phase {name}: every frame was the same ({frames} updates): the benchmark measured a still picture");
        c.failed.push(format!("{name}: still picture"));
    }
}

fn main() {
    let o = opts();
    let (bin, mut args) = boot_args(&o);
    let (p1, p2, p3) = (free_port(), free_port(), free_port());
    let vnc_port = (o.mode == "vnc").then(free_vnc_port);
    let mut cmd = vec![
        "-M".to_string(),
        o.board.clone(),
        "-display".into(),
        "none".into(),
        "-monitor".into(),
        "none".into(),
        "-no-user-config".into(),
        "-qmp".into(),
        format!("tcp:127.0.0.1:{p1},server=on,wait=off"),
        "-qmp".into(),
        format!("tcp:127.0.0.1:{p2},server=on,wait=off"),
        "-chardev".into(),
        format!("socket,id=ser0,host=127.0.0.1,port={p3},server=on,wait=off"),
        "-serial".into(),
        "chardev:ser0".into(),
        "-S".into(),
    ];
    if let Some(p) = vnc_port {
        cmd.push("-vnc".into());
        cmd.push(format!("127.0.0.1:{}", p - 5900));
    }
    cmd.append(&mut args);
    let child: Child = Command::new(&bin)
        .args(&cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start QEMU");
    let pid = child.id();
    let child = Arc::new(Mutex::new(child));
    let _ = QEMU.set(child.clone());
    let _guard = Guard;
    let _ = HIDE.set(vec![
        (o.guest.to_string_lossy().into_owned(), "<guest>"),
        (o.fwstub.to_string_lossy().into_owned(), "<fwstub>"),
        (o.tmp.to_string_lossy().into_owned(), "<tmp>"),
    ]);
    let started = Instant::now();
    {
        // Watchdog: QEMU never lives longer than RUN_LIMIT.
        let child = child.clone();
        thread::spawn(move || {
            thread::sleep(RUN_LIMIT);
            let mut c = child.lock().unwrap();
            if c.try_wait().ok().flatten().is_none() {
                println!("FAIL: watchdog killed QEMU after {} s", RUN_LIMIT.as_secs());
                let _ = c.kill();
                let _ = c.wait();
                std::process::exit(3);
            }
        });
    }
    let ser = connect(p3);
    let q1 = Qmp::new(connect(p1)).expect("qmp1");
    let q2 = Qmp::new(connect(p2)).expect("qmp2");
    let marks: Marks = Arc::new(Mutex::new(Vec::new()));
    let ready = Arc::new(AtomicBool::new(false));
    {
        let (m, r) = (marks.clone(), ready.clone());
        thread::spawn(move || serial_reader(ser, m, r));
    }
    let mut c = Ctx {
        pid,
        marks,
        q1,
        q2: Arc::new(Mutex::new(q2)),
        tmp: o.tmp.clone(),
        w: o.w,
        h: o.h,
        failed: vec![],
    };
    c.q1.cmd(r#"{"execute":"cont"}"#).unwrap().unwrap();
    let ver = Command::new(&bin)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok());
    println!(
        "run mode={} board={} size={}x{} host={}-{} qemu=\"{}\" phase_s={}",
        o.mode,
        o.board,
        o.w,
        o.h,
        std::env::consts::OS,
        std::env::consts::ARCH,
        ver.as_deref().and_then(|v| v.lines().next()).unwrap_or("?"),
        o.phase.as_secs_f64()
    );
    // Wait for the guest's screen and its first rate markers.
    while !(ready.load(Ordering::SeqCst) && c.marks.lock().unwrap().len() >= 2) {
        if started.elapsed() > Duration::from_secs(8) {
            println!("FAIL: the guest did not start (no ready and rate markers in 8 s)");
            let mut ch = child.lock().unwrap();
            let _ = ch.kill();
            let _ = ch.wait();
            std::process::exit(1);
        }
        thread::sleep(Duration::from_millis(20));
    }
    println!("boot_s={:.2}", started.elapsed().as_secs_f64());
    let d = o.phase;
    idle(&mut c, d);
    if o.mode == "screendump" {
        poll(
            &mut c,
            "ppm-15",
            "ppm",
            Some(15.0),
            Sink::File,
            Keys::None,
            d,
        );
        poll(
            &mut c,
            "ppm-30",
            "ppm",
            Some(30.0),
            Sink::File,
            Keys::None,
            d,
        );
        poll(&mut c, "ppm-max", "ppm", None, Sink::File, Keys::None, d);
        poll(&mut c, "png-max", "png", None, Sink::File, Keys::None, d);
        if cfg!(unix) {
            poll(
                &mut c,
                "ppm-fifo-max",
                "ppm",
                None,
                Sink::Fifo,
                Keys::None,
                d,
            );
        } else {
            println!("phase=ppm-fifo-max not-run: no FIFO on this system");
        }
        poll(
            &mut c,
            "ppm-max-keys-same",
            "ppm",
            None,
            Sink::File,
            Keys::Same,
            d,
        );
        poll(
            &mut c,
            "ppm-max-keys-second",
            "ppm",
            None,
            Sink::File,
            Keys::Second,
            d,
        );
    } else {
        let p = vnc_port.unwrap();
        vnc_phase(&mut c, p, "vnc-30", Some(30.0), d * 2);
        vnc_phase(&mut c, p, "vnc-max", None, d * 2);
    }
    let _ = c.q1.cmd(r#"{"execute":"quit"}"#);
    let mut ch = child.lock().unwrap();
    let t = Instant::now();
    while ch.try_wait().ok().flatten().is_none() && t.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(20));
    }
    if ch.try_wait().ok().flatten().is_none() {
        let _ = ch.kill();
    }
    let _ = ch.wait();
    let mut stderr = String::new();
    if let Some(mut e) = ch.stderr.take() {
        let _ = e.read_to_string(&mut stderr);
    }
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        println!("qemu_stderr_lines={}", stderr.lines().count());
    }
    println!(
        "run_s={:.1} qemu_exited=yes",
        started.elapsed().as_secs_f64()
    );
    if !c.failed.is_empty() {
        println!("FAIL: {}", c.failed.join("; "));
        std::process::exit(1);
    }
    println!("PASS");
}
