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
    scroll_dir: f32,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
                    self.scroll_dir = -self.scroll_dir;
                    ui.ctx().input_mut(|i| {
                        i.smooth_scroll_delta = egui::vec2(0.0, 4000.0 * self.scroll_dir)
                    });
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
    let frames = std::env::var("K4_BENCH").ok().map(|t| {
        state.tab = match t.as_str() {
            "screen" => Tab::Screen,
            "log" => Tab::Log,
            _ => Tab::Canvas,
        };
        let frames = Arc::new(AtomicU64::new(0));
        let (f, s, tab) = (frames.clone(), sim.clone(), state.tab);
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
                scroll_dir: 1.0,
            }))
        }),
    )
}
