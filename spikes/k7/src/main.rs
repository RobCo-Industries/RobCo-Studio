//! Runs the spike editor. With no setting it opens a sample C file of 5,000 lines.
//!
//! `K7_BENCH=type-end|type-mid|scroll` and `K7_LINES=<n>` run a benchmark:
//! it types one character per frame at the end or in the middle, or moves the
//! view 40 lines per frame. After 10 s it prints the frame rate, CPU use and the
//! time spent in the editor's `ui` per frame, then exits. It exits with an error
//! if the text or the first visible line never changed.

use eframe::egui;
use k7_editor::{Editor, sample_c};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bench {
    TypeEnd,
    TypeMid,
    Scroll,
}

#[derive(Default)]
struct Counters {
    frames: AtomicU64,
    /// Frames in which the text changed.
    edits: AtomicU64,
    /// Frames in which the first visible line changed.
    line_changes: AtomicU64,
    /// Frames in which the gutter and the text disagreed on the first line.
    gutter_mismatch: AtomicU64,
    /// Frames with the window focused.
    focused: AtomicU64,
    ui_ns: AtomicU64,
    ui_max_ns: AtomicU64,
    highlight_runs: AtomicU64,
    /// For type-mid: 1 if the typed text sits on the middle line.
    typed_mid: AtomicU64,
}

struct App {
    ed: Editor,
    bench: Option<(Bench, usize, Arc<Counters>)>,
    last_len: usize,
    last_line: usize,
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some((b, _, c)) = &self.bench
            && *b != Bench::Scroll
            && c.frames.load(Ordering::Relaxed) >= 2
        {
            raw.events.push(egui::Event::Text("x".into()));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let Some((bench, lines, c)) = &self.bench else {
            egui::CentralPanel::default().show(ui, |ui| self.ed.ui(ui));
            return;
        };
        let (bench, lines, c) = (*bench, *lines, c.clone());
        let n = c.frames.load(Ordering::Relaxed);
        if n == 0 {
            match bench {
                Bench::TypeEnd => {
                    let end = self.ed.doc.text.chars().count();
                    self.ed.doc.pending_sel = Some((end, end));
                }
                Bench::TypeMid => self.ed.goto_line(lines / 2),
                Bench::Scroll => {}
            }
        }
        if bench == Bench::Scroll {
            self.ed.doc.scroll_to_line = Some((n as usize * 40) % lines);
        }
        let t = Instant::now();
        egui::CentralPanel::default().show(ui, |ui| self.ed.ui(ui));
        let ns = t.elapsed().as_nanos() as u64;
        c.ui_ns.fetch_add(ns, Ordering::Relaxed);
        c.ui_max_ns.fetch_max(ns, Ordering::Relaxed);
        c.frames.fetch_add(1, Ordering::Relaxed);
        c.highlight_runs
            .store(self.ed.highlight_runs, Ordering::Relaxed);
        if ui.ctx().input(|i| i.viewport().focused) == Some(true) {
            c.focused.fetch_add(1, Ordering::Relaxed);
        }
        if self.ed.doc.text.len() != self.last_len {
            if n > 0 {
                c.edits.fetch_add(1, Ordering::Relaxed);
            }
            self.last_len = self.ed.doc.text.len();
        }
        if self.ed.first_gutter_line != self.last_line {
            c.line_changes.fetch_add(1, Ordering::Relaxed);
            self.last_line = self.ed.first_gutter_line;
        }
        if self.ed.check_lines && self.ed.first_gutter_line != self.ed.first_text_line {
            c.gutter_mismatch.fetch_add(1, Ordering::Relaxed);
        }
        if bench == Bench::TypeMid {
            let line = self
                .ed
                .doc
                .text
                .split('\n')
                .nth(lines / 2 - 1)
                .unwrap_or("");
            c.typed_mid
                .store(line.starts_with("xx") as u64, Ordering::Relaxed);
        }
        ui.ctx().request_repaint();
    }
}

/// CPU time of this process in seconds, from `ps` (macOS and Linux).
fn cpu_seconds() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "time=", "-p", &std::process::id().to_string()])
        .output();
    let s = out
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    s.split(':')
        .fold(0.0, |acc, p| acc * 60.0 + p.parse::<f64>().unwrap_or(0.0))
}

fn report(bench: Bench, lines: usize, c: &Counters, el: f64, cpu: f64) -> bool {
    let l = |a: &AtomicU64| a.load(Ordering::Relaxed);
    let frames = l(&c.frames).max(1);
    println!(
        "k7 bench={bench:?} lines={lines} frames={} fps={:.1} cpu_pct={:.1} ui_ms_mean={:.2} ui_ms_max={:.2} focused_frames={}",
        l(&c.frames),
        l(&c.frames) as f64 / el,
        100.0 * cpu / el,
        l(&c.ui_ns) as f64 / frames as f64 / 1e6,
        l(&c.ui_max_ns) as f64 / 1e6,
        l(&c.focused),
    );
    println!(
        "k7 proof edits={} highlight_runs={} first_line_changes={} gutter_mismatch_frames={} typed_on_mid_line={}",
        l(&c.edits),
        l(&c.highlight_runs),
        l(&c.line_changes),
        l(&c.gutter_mismatch),
        l(&c.typed_mid),
    );
    match bench {
        Bench::TypeEnd | Bench::TypeMid if l(&c.edits) == 0 => {
            eprintln!("FAIL: no frame changed the text, the benchmark measured no typing");
            false
        }
        Bench::TypeMid if l(&c.typed_mid) == 0 => {
            eprintln!("FAIL: the typed text is not on the middle line");
            false
        }
        Bench::Scroll if l(&c.line_changes) == 0 => {
            eprintln!(
                "FAIL: the first visible line never changed, the benchmark measured a still view"
            );
            false
        }
        Bench::Scroll if l(&c.gutter_mismatch) > 0 => {
            eprintln!("FAIL: the gutter and the text disagreed on the first line");
            false
        }
        _ => true,
    }
}

fn main() -> eframe::Result {
    let lines: usize = std::env::var("K7_LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5000);
    let mut ed = Editor::new(sample_c(lines));
    let bench = std::env::var("K7_BENCH").ok().map(|b| {
        let b = match b.as_str() {
            "type-end" => Bench::TypeEnd,
            "type-mid" => Bench::TypeMid,
            _ => Bench::Scroll,
        };
        let c = Arc::new(Counters::default());
        let cc = c.clone();
        std::thread::spawn(move || {
            let start = Instant::now();
            std::thread::sleep(Duration::from_secs(10));
            let el = start.elapsed().as_secs_f64();
            let ok = report(b, lines, &cc, el, cpu_seconds());
            std::process::exit(if ok { 0 } else { 1 });
        });
        (b, lines, c)
    });
    ed.check_lines = matches!(bench, Some((Bench::Scroll, _, _)));
    let last_len = ed.doc.text.len();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 760.0]),
        ..Default::default()
    };
    eframe::run_native(
        "K7 editor",
        opts,
        Box::new(move |_| {
            Ok(Box::new(App {
                ed,
                bench,
                last_len,
                last_line: 1,
            }))
        }),
    )
}
