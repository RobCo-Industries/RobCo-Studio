//! The window benchmark. `K8_MODE=idle|crt|overlay`, `K8_FPS=60|30|15`,
//! `K8_W`, `K8_H` in points, `K8_CURVE` for the curvature strength.
//! It runs 11 s, measures the process CPU time from `ps` over the last 10 s,
//! prints one line, and closes itself.

use std::time::{Duration, Instant};

use eframe::egui;
use k8_crt::{Crt, Overlay, Params, map_input, redirect_to_crt, stand_in};

fn var(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_owned())
}

/// CPU seconds of this process, from `ps -o time=`.
fn cpu_secs() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "time=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let mut total = 0.0;
    for part in s.split(':') {
        total = total * 60.0 + part.parse::<f64>().unwrap_or(0.0);
    }
    total
}

struct App {
    mode: String,
    fps: f64,
    params: Params,
    overlay: Option<Overlay>,
    start: Instant,
    cpu0: Option<f64>,
    frames: u64,
    focused_frames: u64,
    ui_ms: f64,
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        map_input(raw, self.params.curvature);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let t0 = Instant::now();
        let ctx = ui.ctx().clone();
        let t = self.start.elapsed().as_secs_f64();
        stand_in(ui, if self.mode == "idle" { 0.0 } else { t });
        match self.mode.as_str() {
            "crt" => redirect_to_crt(&ctx, self.params, t as f32),
            "overlay" => {
                let o = self.overlay.get_or_insert_with(|| Overlay::new(&ctx));
                o.paint(&ctx, t, &Params::TASTEFUL);
            }
            _ => {}
        }
        self.ui_ms += t0.elapsed().as_secs_f64() * 1000.0;
        self.frames += 1;
        if self.frames % 100 == 1 && std::env::var("K8_CAUSES").is_ok() {
            eprintln!("repaint causes: {:?}", ctx.repaint_causes());
        }
        if ctx.input(|i| i.viewport().focused.unwrap_or(false)) {
            self.focused_frames += 1;
        }
        if self.cpu0.is_none() && t >= 1.0 {
            self.cpu0 = Some(cpu_secs());
        }
        if t >= 11.0 {
            let cpu = cpu_secs() - self.cpu0.unwrap_or(0.0);
            println!(
                "mode={} fps_req={} frames={} focused={} cpu={:.0}% ui_ms_mean={:.2}",
                self.mode,
                self.fps,
                self.frames,
                self.focused_frames,
                cpu / 10.0 * 100.0,
                self.ui_ms / self.frames as f64
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.mode == "idle" {
            // Today's window draws nothing when idle. Wake only for the timers.
            ctx.request_repaint_after(Duration::from_millis(500));
        } else {
            ctx.request_repaint_after(Duration::from_secs_f64(1.0 / self.fps));
        }
    }
}

fn main() {
    let mode = var("K8_MODE", "crt");
    let fps: f64 = var("K8_FPS", "60").parse().expect("K8_FPS");
    let w: f32 = var("K8_W", "640").parse().expect("K8_W");
    let h: f32 = var("K8_H", "400").parse().expect("K8_H");
    let mut params = Params::TASTEFUL;
    params.curvature = var("K8_CURVE", "0.15").parse().expect("K8_CURVE");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([w, h])
            .with_title("K8"),
        ..Default::default()
    };
    eframe::run_native(
        "K8",
        options,
        Box::new(move |cc| {
            let rs = cc.wgpu_render_state.as_ref().expect("wgpu renderer");
            eprintln!("surface format {:?}", rs.target_format);
            let crt = Crt::new(&rs.device, rs.target_format);
            rs.renderer.write().callback_resources.insert(crt);
            Ok(Box::new(App {
                mode,
                fps,
                params,
                overlay: None,
                start: Instant::now(),
                cpu0: None,
                frames: 0,
                focused_frames: 0,
                ui_ms: 0.0,
            }))
        }),
    )
    .expect("window");
}
