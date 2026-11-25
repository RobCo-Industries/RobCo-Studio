//! Spike K4, GPUI side: three views on tabs, and the state a headless test drives.
//!
//! GPUI 0.2.2 samples every image with a linear filter. To keep pixels sharp,
//! the sprite is drawn as one quad per pixel and the screen is scaled on the CPU
//! to device pixels before upload, so the GPU samples it 1:1.

use gpui::{
    Bounds, Context, Corners, Hsla, MouseButton, Pixels, RenderImage, ScrollStrategy,
    UniformListScrollHandle, Window, canvas, div, fill, point, prelude::*, px, rgb, size,
    uniform_list,
};
use image::{Frame, RgbaImage};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const SPRITE: usize = 32;
pub const SCREEN_W: usize = 320;
pub const SCREEN_H: usize = 240;
pub const LOG_LINES: usize = 100_000;

const PALETTE: [u32; 4] = [0x000000, 0x145a1e, 0x3cc85a, 0xb4ffc8];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Canvas,
    Screen,
    Log,
}

pub struct Spike {
    pub tab: Tab,
    pub sprite: Vec<u8>,
    pub zoom: f32,
    pub ink: u8,
    pub frame: u64,
    pub frames_drawn: u64,
    pub busy: bool,
    log: Vec<String>,
    log_scroll: UniformListScrollHandle,
    screen_img: Option<Arc<RenderImage>>,
    sprite_origin: Rc<Cell<gpui::Point<Pixels>>>,
    last_frame: Option<Instant>,
}

impl Default for Spike {
    fn default() -> Self {
        Self {
            tab: Tab::Canvas,
            sprite: vec![0; SPRITE * SPRITE],
            zoom: 12.0,
            ink: 2,
            frame: 0,
            frames_drawn: 0,
            busy: false,
            log: (0..LOG_LINES)
                .map(|i| format!("{i:06} build: compiled unit {i} of {LOG_LINES}"))
                .collect(),
            log_scroll: UniformListScrollHandle::new(),
            screen_img: None,
            sprite_origin: Rc::default(),
            last_frame: None,
        }
    }
}

fn button(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(move || id.to_string())
        .px_2()
        .py_1()
        .bg(rgb(0x2a3a2a))
        .text_color(rgb(0xb4ffc8))
        .cursor_pointer()
        .child(label)
}

impl Spike {
    pub fn set_pixel(&mut self, x: usize, y: usize) {
        self.sprite[y * SPRITE + x] = self.ink;
    }

    pub fn jump_to_end(&self) {
        self.log_scroll
            .scroll_to_item(self.log.len() - 1, ScrollStrategy::Top);
    }

    /// Next simulator frame, scaled by a whole factor `k` on the CPU, in BGRA.
    fn next_screen(&mut self, k: usize) -> Arc<RenderImage> {
        self.frame += 1;
        let t = self.frame as usize;
        let (w, h) = (SCREEN_W * k, SCREEN_H * k);
        let mut buf = vec![0u8; w * h * 4];
        for y in 0..SCREEN_H {
            for x in 0..SCREEN_W {
                let c = PALETTE[((x + t) / 8 + (y + t / 2) / 8) % 4];
                let px = [
                    (c & 0xff) as u8,
                    (c >> 8 & 0xff) as u8,
                    (c >> 16) as u8,
                    255,
                ];
                for dy in 0..k {
                    let row = ((y * k + dy) * w + x * k) * 4;
                    for dx in 0..k {
                        buf[row + dx * 4..row + dx * 4 + 4].copy_from_slice(&px);
                    }
                }
            }
        }
        let img = RgbaImage::from_raw(w as u32, h as u32, buf).expect("buffer size");
        Arc::new(RenderImage::new([Frame::new(img)]))
    }

    fn canvas_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let sprite = self.sprite.clone();
        let zoom = self.zoom;
        let side = px(SPRITE as f32 * zoom);
        let origin = self.sprite_origin.clone();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button("zoom-in", "Zoom in").on_click(cx.listener(|s, _, _, cx| {
                            s.zoom = (s.zoom * 2.0).min(32.0);
                            cx.notify();
                        })),
                    )
                    .child(
                        button("zoom-out", "Zoom out").on_click(cx.listener(|s, _, _, cx| {
                            s.zoom = (s.zoom / 2.0).max(1.0);
                            cx.notify();
                        })),
                    )
                    .child(format!("zoom {zoom}x")),
            )
            .child(
                div()
                    .id("sprite")
                    .debug_selector(|| "sprite".into())
                    .size(side)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |s, ev: &gpui::MouseDownEvent, _, cx| {
                            s.click_at(ev.position, cx);
                        }),
                    )
                    .child(
                        canvas(
                            move |b: Bounds<Pixels>, _, _| origin.set(b.origin),
                            move |b: Bounds<Pixels>, _, window, _| {
                                let z = px(zoom);
                                for y in 0..SPRITE {
                                    for x in 0..SPRITE {
                                        let o = point(
                                            b.origin.x + z * x as f32,
                                            b.origin.y + z * y as f32,
                                        );
                                        let c = rgb(PALETTE[sprite[y * SPRITE + x] as usize]);
                                        window.paint_quad(fill(Bounds::new(o, size(z, z)), c));
                                    }
                                }
                                if zoom >= 4.0 {
                                    let g: Hsla = rgb(0x464646).into();
                                    for i in 0..=SPRITE {
                                        let o = z * i as f32;
                                        window.paint_quad(fill(
                                            Bounds::new(
                                                point(b.origin.x + o, b.origin.y),
                                                size(px(1.), b.size.height),
                                            ),
                                            g,
                                        ));
                                        window.paint_quad(fill(
                                            Bounds::new(
                                                point(b.origin.x, b.origin.y + o),
                                                size(b.size.width, px(1.)),
                                            ),
                                            g,
                                        ));
                                    }
                                }
                            },
                        )
                        .size_full(),
                    ),
            )
    }

    /// Set the pixel under a window position.
    pub fn click_at(&mut self, pos: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let o = self.sprite_origin.get();
        let x = f32::from(pos.x - o.x) / self.zoom;
        let y = f32::from(pos.y - o.y) / self.zoom;
        if x >= 0.0 && y >= 0.0 && (x as usize) < SPRITE && (y as usize) < SPRITE {
            self.set_pixel(x as usize, y as usize);
            cx.notify();
        }
    }

    fn screen_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = window.scale_factor();
        let avail = window.viewport_size();
        let k = ((f32::from(avail.width) / SCREEN_W as f32)
            .min((f32::from(avail.height) - 60.0) / SCREEN_H as f32))
        .floor()
        .max(1.0);
        let device_k = (k * scale).round() as usize;
        let now = Instant::now();
        let step = Duration::from_micros(33_333);
        let due = self
            .last_frame
            .is_none_or(|t| now.duration_since(t) >= step);
        if due {
            self.last_frame = Some(match self.last_frame {
                Some(t) if now.duration_since(t) < step * 2 => t + step,
                _ => now,
            });
            let img = self.next_screen(device_k);
            if let Some(old) = self.screen_img.replace(img) {
                cx.drop_image(old, Some(window));
            }
        }
        window.request_animation_frame();
        let img = self.screen_img.clone();
        let (w, h) = (px(SCREEN_W as f32 * k), px(SCREEN_H as f32 * k));
        div().w(w).h(h).child(
            canvas(
                |_, _, _| (),
                move |b, _, window, _| {
                    if let Some(img) = img {
                        let _ = window.paint_image(b, Corners::default(), img, 0, false);
                    }
                },
            )
            .size_full(),
        )
    }

    fn log_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                button("jump-end", "Jump to end").on_click(cx.listener(|s, _, _, cx| {
                    s.jump_to_end();
                    cx.notify();
                })),
            )
            .child(
                uniform_list(
                    "log",
                    self.log.len(),
                    cx.processor(|s, range: std::ops::Range<usize>, _, _| {
                        range
                            .map(|i| div().font_family("Menlo").child(s.log[i].clone()))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.log_scroll.clone())
                .flex_grow(),
            )
    }
}

impl Render for Spike {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frames_drawn += 1;
        if self.busy {
            match self.tab {
                Tab::Canvas => {
                    let n = self.frames_drawn as usize;
                    self.set_pixel(n % SPRITE, (n / SPRITE) % SPRITE);
                }
                Tab::Log => {
                    let n = if self.frames_drawn % 2 == 0 {
                        0
                    } else {
                        LOG_LINES - 1
                    };
                    self.log_scroll.scroll_to_item(n, ScrollStrategy::Top);
                }
                Tab::Screen => {}
            }
            window.request_animation_frame();
        }
        let tab = |id: &'static str, label: &'static str, t: Tab, cur: Tab| {
            button(id, label).when(t == cur, |d| d.bg(rgb(0x3cc85a)).text_color(rgb(0x000000)))
        };
        let cur = self.tab;
        let body = match self.tab {
            Tab::Canvas => self.canvas_view(cx).into_any_element(),
            Tab::Screen => self.screen_view(window, cx).into_any_element(),
            Tab::Log => self.log_view(cx).into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(rgb(0x101810))
            .text_color(rgb(0xb4ffc8))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        tab("tab-canvas", "Canvas", Tab::Canvas, cur).on_click(cx.listener(
                            |s, _, _, cx| {
                                s.tab = Tab::Canvas;
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        tab("tab-screen", "Screen", Tab::Screen, cur).on_click(cx.listener(
                            |s, _, _, cx| {
                                s.tab = Tab::Screen;
                                cx.notify();
                            },
                        )),
                    )
                    .child(tab("tab-log", "Log", Tab::Log, cur).on_click(cx.listener(
                        |s, _, _, cx| {
                            s.tab = Tab::Log;
                            cx.notify();
                        },
                    ))),
            )
            .child(div().flex_grow().child(body))
    }
}
