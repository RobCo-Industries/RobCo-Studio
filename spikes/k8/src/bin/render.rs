//! Offscreen: the pictures, the pass timings and the curvature pixel check.
//! `cargo run --release --bin render -- <out dir>`. With no GPU adapter it
//! prints "not run: no GPU adapter" and exits 0; any other failure exits 1.

use std::path::{Path, PathBuf};
use std::time::Instant;

use eframe::egui::{self, Pos2, Rect, vec2};
use eframe::egui_wgpu::{ScreenDescriptor, wgpu};
use image::{Rgba, RgbaImage, imageops};
use k8_crt::{Crt, GREEN, GROUND, Overlay, Params, drawn_at, stand_in};

fn flat(d: &egui::TexturesDelta) -> Vec<(egui::TextureId, egui::epaint::ImageDelta)> {
    d.set
        .iter()
        .flat_map(|(id, v)| v.iter().map(move |x| (*id, x.clone())))
        .collect()
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

struct Scene {
    ctx: egui::Context,
    crt: Crt,
    overlay: Option<Overlay>,
}

/// One frame of the stand-in at time `t`: returns the image and the button rect.
#[allow(clippy::too_many_arguments)]
fn frame(
    g: &Gpu,
    s: &mut Scene,
    size: [u32; 2],
    ppp: f32,
    p: &Params,
    t: f64,
    plan_b: bool,
    label: Option<&str>,
    read: bool,
) -> (Option<RgbaImage>, Rect) {
    s.ctx.set_pixels_per_point(ppp);
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            vec2(size[0] as f32 / ppp, size[1] as f32 / ppp),
        )),
        time: Some(t),
        ..Default::default()
    };
    let mut button = Rect::NOTHING;
    let overlay = &mut s.overlay;
    let out = s.ctx.run_ui(raw, |ui| {
        if let Some(text) = label {
            ui.painter().rect_filled(ui.max_rect(), 0.0, GROUND);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(text).color(GREEN).size(14.0).strong())
            });
            return;
        }
        button = stand_in(ui, t).button;
        if plan_b {
            let ctx = ui.ctx().clone();
            overlay
                .get_or_insert_with(|| Overlay::new(&ctx))
                .paint(&ctx, t, &Params::TASTEFUL);
        }
    });
    let prims = s.ctx.tessellate(out.shapes, out.pixels_per_point);
    let sd = ScreenDescriptor {
        size_in_pixels: size,
        pixels_per_point: ppp,
    };
    let mut enc = g.device.create_command_encoder(&Default::default());
    let cmds = s.crt.encode(
        &g.device,
        &g.queue,
        &mut enc,
        &prims,
        &sd,
        &flat(&out.textures_delta),
        p,
        t as f32,
        true,
    );
    let img = read.then(|| {
        let bpr = (size[0] * 4).div_ceil(256) * 256;
        let buf = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (bpr * size[1]) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            s.crt.output().as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bpr),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        buf
    });
    g.queue.submit(cmds.into_iter().chain([enc.finish()]));
    let img = img.map(|buf| {
        buf.slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.expect("map"));
        g.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        let data = buf.slice(..).get_mapped_range().expect("mapped");
        let bpr = (size[0] * 4).div_ceil(256) as usize * 256;
        let mut im = RgbaImage::new(size[0], size[1]);
        for y in 0..size[1] as usize {
            let row = &data[y * bpr..y * bpr + size[0] as usize * 4];
            for x in 0..size[0] as usize {
                let c = &row[x * 4..x * 4 + 4];
                im.put_pixel(x as u32, y as u32, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        im
    });
    for id in &out.textures_delta.free {
        s.crt.renderer.free_texture(id);
    }
    (img, button)
}

/// Twenty frames at 60 a second before `t`, so that persistence has a past.
fn shot(
    g: &Gpu,
    s: &mut Scene,
    size: [u32; 2],
    ppp: f32,
    p: &Params,
    t: f64,
    b: bool,
) -> (RgbaImage, Rect) {
    for k in (1..=20).rev() {
        frame(g, s, size, ppp, p, t - k as f64 / 60.0, b, None, false);
    }
    let (im, r) = frame(g, s, size, ppp, p, t, b, None, true);
    (im.expect("image"), r)
}

fn label(g: &Gpu, s: &mut Scene, w: u32, text: &str) -> RgbaImage {
    frame(
        g,
        s,
        [w, 28],
        1.0,
        &Params::OFF,
        0.0,
        false,
        Some(text),
        true,
    )
    .0
    .expect("label")
}

fn save(im: &RgbaImage, dir: &Path, name: &str) {
    let p = dir.join(name);
    im.save(&p).expect("save png");
    println!("wrote {}", p.display());
}

/// Two images side by side with a gap, a label under each.
fn pair(a: &RgbaImage, b: &RgbaImage, la: &RgbaImage, lb: &RgbaImage) -> RgbaImage {
    let gap = 12;
    let w = a.width() * 2 + gap;
    let h = a.height() + la.height();
    let mut im = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255]));
    imageops::replace(&mut im, a, 0, 0);
    imageops::replace(&mut im, b, (a.width() + gap) as i64, 0);
    imageops::replace(&mut im, la, 0, a.height() as i64);
    imageops::replace(&mut im, lb, (a.width() + gap) as i64, a.height() as i64);
    im
}

/// Mean milliseconds per frame over `n` frames, GPU work waited for each frame.
fn time_frames(
    g: &Gpu,
    s: &mut Scene,
    size: [u32; 2],
    ppp: f32,
    p: &Params,
    crt: bool,
    n: u32,
) -> f64 {
    s.ctx.set_pixels_per_point(ppp);
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            vec2(size[0] as f32 / ppp, size[1] as f32 / ppp),
        )),
        ..Default::default()
    };
    let out = s.ctx.run_ui(raw, |ui| {
        stand_in(ui, 0.0);
    });
    let prims = s.ctx.tessellate(out.shapes, out.pixels_per_point);
    let sd = ScreenDescriptor {
        size_in_pixels: size,
        pixels_per_point: ppp,
    };
    let deltas = flat(&out.textures_delta);
    let mut total = 0.0;
    for i in 0..n + 10 {
        let t0 = Instant::now();
        let mut enc = g.device.create_command_encoder(&Default::default());
        let d = if i == 0 { &deltas[..] } else { &[] };
        let cmds = s.crt.encode(
            &g.device,
            &g.queue,
            &mut enc,
            &prims,
            &sd,
            d,
            p,
            i as f32 / 60.0,
            crt,
        );
        g.queue.submit(cmds.into_iter().chain([enc.finish()]));
        g.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        if i >= 10 {
            total += t0.elapsed().as_secs_f64();
        }
    }
    total / n as f64 * 1000.0
}

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "k8-out".into()));
    std::fs::create_dir_all(&dir).expect("out dir");
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = match pollster::block_on(instance.request_adapter(&Default::default())) {
        Ok(a) => a,
        Err(e) => {
            println!("not run: no GPU adapter ({e})");
            return;
        }
    };
    let info = adapter.get_info();
    println!(
        "adapter: {} {:?} {:?}",
        info.name, info.backend, info.device_type
    );
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default())).expect("device");
    let g = Gpu { device, queue };
    let mut s = Scene {
        ctx: egui::Context::default(),
        crt: Crt::new(&g.device, k8_crt::FMT),
        overlay: None,
    };
    let only_time = std::env::var("K8_ONLY_TIME").is_ok();
    let hd = [1280, 720];

    // Curvature: a pixel drawn where the pointer mapping says the button is.
    let mut curved = Params::OFF;
    curved.curvature = 1.0;
    let (im, button) = shot(&g, &mut s, hd, 1.0, &curved, 2.0, false);
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0));
    let mut ok = true;
    for target in [
        Pos2::new(button.center().x, button.min.y + 1.5),
        Pos2::new(button.min.x + 1.5, button.center().y),
        Pos2::new(button.center().x, button.max.y - 1.5),
    ] {
        let d = drawn_at(target, screen, 1.0);
        let px = im.get_pixel(d.x.round() as u32, d.y.round() as u32);
        let near = (px[0] as i32 - GREEN.r() as i32).abs() < 40
            && (px[1] as i32 - GREEN.g() as i32).abs() < 40
            && (px[2] as i32 - GREEN.b() as i32).abs() < 40;
        println!(
            "curve check: button point {:?} drawn at {:?}, pixel {:?}: {}",
            target,
            d,
            px.0,
            if near {
                "button fill"
            } else {
                "NOT the button"
            }
        );
        ok &= near;
        // Off the curve, the same point is not where the button is drawn.
        let moved = (d - target).length();
        println!("  moved by the curve: {moved:.1} points");
    }
    if !ok {
        eprintln!("curve check failed");
        std::process::exit(1);
    }

    if !only_time {
        save(
            &shot(&g, &mut s, hd, 1.0, &Params::OFF, 2.0, false).0,
            &dir,
            "none.png",
        );
        for (name, p) in Params::named() {
            save(
                &shot(&g, &mut s, hd, 1.0, &p, 2.0, false).0,
                &dir,
                &format!("only-{name}.png"),
            );
        }
        save(
            &shot(&g, &mut s, hd, 1.0, &Params::TASTEFUL, 2.0, false).0,
            &dir,
            "all-on.png",
        );
        save(
            &shot(&g, &mut s, hd, 1.0, &Params::STRONG, 2.0, false).0,
            &dir,
            "all-strong.png",
        );
        save(
            &shot(&g, &mut s, [2560, 1440], 2.0, &Params::TASTEFUL, 2.0, false).0,
            &dir,
            "all-on-2x.png",
        );
        for (i, t) in [2.0, 2.5, 3.0].into_iter().enumerate() {
            save(
                &shot(&g, &mut s, hd, 1.0, &Params::TASTEFUL, t, false).0,
                &dir,
                &format!("motion-{i}.png"),
            );
        }
        save(
            &shot(&g, &mut s, hd, 1.0, &Params::OFF, 2.0, true).0,
            &dir,
            "plan-b.png",
        );
        let mut mask = Params::TASTEFUL;
        mask.mask = 0.6;
        mask.aberration = 0.6;
        let (a, button) = shot(&g, &mut s, hd, 1.0, &Params::TASTEFUL, 2.0, false);
        let (b, _) = shot(&g, &mut s, hd, 1.0, &mask, 2.0, false);
        let la = label(&g, &mut s, hd[0], "MONO");
        let lb = label(&g, &mut s, hd[0], "MASK");
        save(&pair(&a, &b, &la, &lb), &dir, "compare-mono-vs-mask.png");
        // Zoom: the code block and the buttons, four times.
        let d = drawn_at(button.left_top(), screen, Params::TASTEFUL.curvature);
        let (x, y, w, h) = (d.x as u32 - 6, d.y as u32 - 120, 260, 150);
        let z = |im: &RgbaImage| {
            imageops::resize(
                &imageops::crop_imm(im, x, y, w, h).to_image(),
                w * 4,
                h * 4,
                imageops::FilterType::Nearest,
            )
        };
        let lz = label(&g, &mut s, w * 4, "MONO");
        let lm = label(&g, &mut s, w * 4, "MASK");
        save(
            &pair(&z(&a), &z(&b), &lz, &lm),
            &dir,
            "compare-mono-vs-mask-zoom.png",
        );
    }

    println!("timing, ms per frame, GPU waited, mean of 120 frames");
    for (size, ppp) in [([1280u32, 720u32], 1.0f32), ([2560, 1440], 2.0)] {
        let scene = time_frames(&g, &mut s, size, ppp, &Params::OFF, false, 120);
        let all = time_frames(&g, &mut s, size, ppp, &Params::STRONG, true, 120);
        let off = time_frames(&g, &mut s, size, ppp, &Params::OFF, true, 120);
        println!(
            "{}x{}: egui only {scene:.3}, passes all off {off:.3}, passes all strong {all:.3}, passes cost {:.3}",
            size[0],
            size[1],
            all - scene
        );
        for (name, p) in Params::named() {
            let ms = time_frames(&g, &mut s, size, ppp, &p, true, 60);
            println!("  {}x{} only {name}: {ms:.3}", size[0], size[1]);
        }
    }
}
