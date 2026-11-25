//! Runs the spike window. `K4_BENCH=canvas|screen|log` shows one tab, keeps it busy,
//! and a timer prints frames per second and process CPU use after 10 s, then exits.

use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use k4_gpui::{Spike, Tab};
use std::time::{Duration, Instant};

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

fn main() {
    let bench = std::env::var("K4_BENCH").ok();
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1000.), px(760.)), cx);
        let win = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| {
                        let mut s = Spike::default();
                        if let Some(t) = &bench {
                            s.busy = true;
                            s.tab = match t.as_str() {
                                "screen" => Tab::Screen,
                                "log" => Tab::Log,
                                _ => Tab::Canvas,
                            };
                        }
                        s
                    })
                },
            )
            .expect("open window");
        cx.activate(true);
        if bench.is_some() {
            let start = Instant::now();
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(10))
                    .await;
                let el = start.elapsed().as_secs_f64();
                let cpu = cpu_seconds();
                let _ = win.update(cx, |s, _, _| {
                    println!(
                        "gpui tab={:?} frames={} fps={:.1} sim_fps={:.1} cpu_pct={:.1}",
                        s.tab,
                        s.frames_drawn,
                        s.frames_drawn as f64 / el,
                        s.frame as f64 / el,
                        100.0 * cpu / el
                    );
                });
                std::process::exit(0);
            })
            .detach();
        }
    });
}
