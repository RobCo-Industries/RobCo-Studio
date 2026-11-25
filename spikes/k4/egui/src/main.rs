//! Runs the spike window. `K4_BENCH=canvas|screen|log` shows one tab, keeps it busy,
//! and a timer thread prints frames per second and process CPU use after 10 s, then exits.

use eframe::egui;
use k4_egui::{State, Tab};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct App {
    state: State,
    frames: Option<Arc<AtomicU64>>,
    sim: Arc<AtomicU64>,
    /// Log benchmark: how often the first visible row changed, and whether the last line showed.
    row_changes: Arc<AtomicU64>,
    saw_end: Arc<AtomicU64>,
    last_first_row: usize,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let (Some(frames), Tab::Log) = (&self.frames, self.state.tab) {
            // Set the scroll position before the log is drawn. A cycle of 200 frames
            // steps down through the log 500 rows at a time, then jumps to the end,
            // then back to the top.
            let n = frames.load(Ordering::Relaxed) as usize % 200;
            match n {
                198 => self.state.jump_to_end = true,
                199 => self.state.scroll_to_row = Some(0),
                _ => self.state.scroll_to_row = Some(n * 500),
            }
        }
        egui::CentralPanel::default().show(ui, |ui| self.state.ui(ui));
        if let Some(frames) = &self.frames {
            frames.fetch_add(1, Ordering::Relaxed);
            self.sim.store(self.state.frame, Ordering::Relaxed);
            match self.state.tab {
                Tab::Canvas => {
                    // Paint one pixel per frame so the texture is uploaded every frame.
                    let n = frames.load(Ordering::Relaxed) as usize;
                    self.state.set_pixel(n % 32, (n / 32) % 32);
                    ui.ctx().request_repaint();
                }
                Tab::Log => {
                    let rows = self.state.visible_rows.clone();
                    if rows.start != self.last_first_row {
                        self.row_changes.fetch_add(1, Ordering::Relaxed);
                        self.last_first_row = rows.start;
                    }
                    if rows.end == k4_egui::LOG_LINES {
                        self.saw_end.fetch_add(1, Ordering::Relaxed);
                    }
                    ui.ctx().request_repaint();
                }
                Tab::Screen => {}
            }
        }
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

fn main() -> eframe::Result {
    let mut state = State::default();
    let sim = Arc::new(AtomicU64::new(0));
    let row_changes = Arc::new(AtomicU64::new(0));
    let saw_end = Arc::new(AtomicU64::new(0));
    let frames = std::env::var("K4_BENCH").ok().map(|t| {
        state.tab = match t.as_str() {
            "screen" => Tab::Screen,
            "log" => Tab::Log,
            _ => Tab::Canvas,
        };
        let frames = Arc::new(AtomicU64::new(0));
        let (f, s, tab) = (frames.clone(), sim.clone(), state.tab);
        let (rc, se) = (row_changes.clone(), saw_end.clone());
        std::thread::spawn(move || {
            let start = Instant::now();
            std::thread::sleep(Duration::from_secs(10));
            let el = start.elapsed().as_secs_f64();
            let cpu = cpu_seconds();
            println!(
                "egui tab={tab:?} frames={} fps={:.1} sim_fps={:.1} cpu_pct={:.1}",
                f.load(Ordering::Relaxed),
                f.load(Ordering::Relaxed) as f64 / el,
                s.load(Ordering::Relaxed) as f64 / el,
                100.0 * cpu / el
            );
            if tab == Tab::Log {
                let (rc, se) = (rc.load(Ordering::Relaxed), se.load(Ordering::Relaxed));
                println!("egui log first_row_changes={rc} frames_at_end={se}");
                if rc == 0 || se == 0 {
                    eprintln!("FAIL: the log did not scroll, the benchmark measured a still log");
                    std::process::exit(1);
                }
            }
            std::process::exit(0);
        });
        frames
    });
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 760.0]),
        ..Default::default()
    };
    eframe::run_native(
        "K4 egui",
        opts,
        Box::new(|_| {
            Ok(Box::new(App {
                state,
                frames,
                sim,
                row_changes,
                saw_end,
                last_first_row: 0,
            }))
        }),
    )
}
