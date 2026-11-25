//! Spike K4, egui side: three views on tabs, and the state a headless test drives.

use eframe::egui;
use std::time::{Duration, Instant};

pub const SPRITE: usize = 32;
pub const SCREEN_W: usize = 320;
pub const SCREEN_H: usize = 240;
pub const LOG_LINES: usize = 100_000;

const PALETTE: [[u8; 3]; 4] = [[0, 0, 0], [20, 90, 30], [60, 200, 90], [180, 255, 200]];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Canvas,
    Screen,
    Log,
}

pub struct State {
    pub tab: Tab,
    pub sprite: Vec<u8>,
    pub zoom: f32,
    pub ink: u8,
    pub frame: u64,
    pub log: Vec<String>,
    pub jump_to_end: bool,
    sprite_tex: Option<egui::TextureHandle>,
    sprite_dirty: bool,
    screen_tex: Option<egui::TextureHandle>,
    screen_buf: Vec<u8>,
    last_frame: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            tab: Tab::Canvas,
            sprite: vec![0; SPRITE * SPRITE],
            zoom: 12.0,
            ink: 2,
            frame: 0,
            log: (0..LOG_LINES)
                .map(|i| format!("{i:06} build: compiled unit {i} of {LOG_LINES}"))
                .collect(),
            jump_to_end: false,
            sprite_tex: None,
            sprite_dirty: true,
            screen_tex: None,
            screen_buf: vec![0; SCREEN_W * SCREEN_H * 4],
            last_frame: None,
        }
    }
}

impl State {
    pub fn set_pixel(&mut self, x: usize, y: usize) {
        self.sprite[y * SPRITE + x] = self.ink;
        self.sprite_dirty = true;
    }

    /// Produce the next simulator frame on the CPU: a moving pattern.
    fn next_screen(&mut self) {
        self.frame += 1;
        let t = self.frame as usize;
        for y in 0..SCREEN_H {
            for x in 0..SCREEN_W {
                let c = PALETTE[((x + t) / 8 + (y + t / 2) / 8) % 4];
                let i = (y * SCREEN_W + x) * 4;
                self.screen_buf[i..i + 3].copy_from_slice(&c);
                self.screen_buf[i + 3] = 255;
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Canvas, "Canvas");
            ui.selectable_value(&mut self.tab, Tab::Screen, "Screen");
            ui.selectable_value(&mut self.tab, Tab::Log, "Log");
        });
        ui.separator();
        match self.tab {
            Tab::Canvas => self.canvas(ui),
            Tab::Screen => self.screen(ui),
            Tab::Log => self.log_view(ui),
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Zoom in").clicked() {
                self.zoom = (self.zoom * 2.0).min(32.0);
            }
            if ui.button("Zoom out").clicked() {
                self.zoom = (self.zoom / 2.0).max(1.0);
            }
            ui.label(format!("zoom {}x", self.zoom));
        });
        if self.sprite_dirty || self.sprite_tex.is_none() {
            let rgba: Vec<u8> = self
                .sprite
                .iter()
                .flat_map(|&i| {
                    let c = PALETTE[i as usize];
                    [c[0], c[1], c[2], 255]
                })
                .collect();
            let img = egui::ColorImage::from_rgba_unmultiplied([SPRITE, SPRITE], &rgba);
            match &mut self.sprite_tex {
                Some(t) => t.set(img, egui::TextureOptions::NEAREST),
                None => {
                    self.sprite_tex = Some(ui.ctx().load_texture(
                        "sprite",
                        img,
                        egui::TextureOptions::NEAREST,
                    ))
                }
            }
            self.sprite_dirty = false;
        }
        let size = egui::vec2(SPRITE as f32, SPRITE as f32) * self.zoom;
        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
        let painter = ui.painter_at(rect);
        if let Some(t) = &self.sprite_tex {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(t.id(), rect, uv, egui::Color32::WHITE);
        }
        if self.zoom >= 4.0 {
            let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(70));
            for i in 0..=SPRITE {
                let o = i as f32 * self.zoom;
                painter.vline(rect.min.x + o, rect.y_range(), stroke);
                painter.hline(rect.x_range(), rect.min.y + o, stroke);
            }
        }
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let x = ((p.x - rect.min.x) / self.zoom) as usize;
            let y = ((p.y - rect.min.y) / self.zoom) as usize;
            if x < SPRITE && y < SPRITE {
                self.set_pixel(x, y);
            }
        }
    }

    fn screen(&mut self, ui: &mut egui::Ui) {
        let now = Instant::now();
        let due = self
            .last_frame
            .is_none_or(|t| now.duration_since(t) >= Duration::from_micros(33_333));
        if due {
            // Keep a fixed 30 Hz schedule so display vsync does not stretch the period.
            let step = Duration::from_micros(33_333);
            self.last_frame = Some(match self.last_frame {
                Some(t) if now.duration_since(t) < step * 2 => t + step,
                _ => now,
            });
            self.next_screen();
            let img =
                egui::ColorImage::from_rgba_unmultiplied([SCREEN_W, SCREEN_H], &self.screen_buf);
            match &mut self.screen_tex {
                Some(t) => t.set(img, egui::TextureOptions::NEAREST),
                None => {
                    self.screen_tex = Some(ui.ctx().load_texture(
                        "screen",
                        img,
                        egui::TextureOptions::NEAREST,
                    ))
                }
            }
        }
        let avail = ui.available_size();
        let k = ((avail.x / SCREEN_W as f32).min(avail.y / SCREEN_H as f32))
            .floor()
            .max(1.0);
        if let Some(t) = &self.screen_tex {
            ui.image((t.id(), egui::vec2(SCREEN_W as f32 * k, SCREEN_H as f32 * k)));
        }
        ui.ctx().request_repaint_after(Duration::from_millis(2));
    }

    fn log_view(&mut self, ui: &mut egui::Ui) {
        if ui.button("Jump to end").clicked() {
            self.jump_to_end = true;
        }
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
        let mut area = egui::ScrollArea::vertical().auto_shrink(false);
        if self.jump_to_end {
            area = area.vertical_scroll_offset(row_h * self.log.len() as f32);
            self.jump_to_end = false;
        }
        area.show_rows(ui, row_h, self.log.len(), |ui, range| {
            for i in range {
                ui.monospace(&self.log[i]);
            }
        });
    }
}
