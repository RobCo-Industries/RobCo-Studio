// SPDX-License-Identifier: GPL-3.0-or-later

//! The drawings and the label font as textures: the wordmark, the icons and
//! the Spleen glyph atlas. Each is white on alpha and tinted when drawn.
//!
//! The SVG files are rasterised once at start, and again if the display
//! scale changes, at a whole number of device pixels.

use crate::theme::{self, size, text};
use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, TextureHandle, TextureOptions};
use std::collections::BTreeMap;

const WORDMARK_SVG: &str = include_str!("../assets/art/wordmark.svg");
const ICONS_SVG: &str = include_str!("../assets/art/icons.svg");

/// The icons the window draws, by their name in the icon file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(missing_docs)]
pub enum Icon {
    Bench,
    Card,
    Cable,
    Sdk,
    Settings,
    Help,
    Build,
    Run,
    Debug,
    WriteCard,
    Folder,
    File,
    FileC,
    Manifest,
    ChevRight,
    ChevDown,
    Close,
    Info,
    Ok,
    Warn,
    Error,
    Refused,
    Tape,
    Wrist,
    Terminal,
    Search,
    Add,
    External,
    Lock,
    Save,
}

impl Icon {
    const ALL: [Icon; 30] = [
        Icon::Bench,
        Icon::Card,
        Icon::Cable,
        Icon::Sdk,
        Icon::Settings,
        Icon::Help,
        Icon::Build,
        Icon::Run,
        Icon::Debug,
        Icon::WriteCard,
        Icon::Folder,
        Icon::File,
        Icon::FileC,
        Icon::Manifest,
        Icon::ChevRight,
        Icon::ChevDown,
        Icon::Close,
        Icon::Info,
        Icon::Ok,
        Icon::Warn,
        Icon::Error,
        Icon::Refused,
        Icon::Tape,
        Icon::Wrist,
        Icon::Terminal,
        Icon::Search,
        Icon::Add,
        Icon::External,
        Icon::Lock,
        Icon::Save,
    ];

    /// The symbol id in the icon file, without its `i-` prefix.
    pub fn file_name(self) -> &'static str {
        match self {
            Icon::Bench => "bench",
            Icon::Card => "card",
            Icon::Cable => "cable",
            Icon::Sdk => "sdk",
            Icon::Settings => "settings",
            Icon::Help => "help",
            Icon::Build => "build",
            Icon::Run => "run",
            Icon::Debug => "debug",
            Icon::WriteCard => "write-card",
            Icon::Folder => "folder",
            Icon::File => "file",
            Icon::FileC => "file-c",
            Icon::Manifest => "manifest",
            Icon::ChevRight => "chev-right",
            Icon::ChevDown => "chev-down",
            Icon::Close => "close",
            Icon::Info => "info",
            Icon::Ok => "ok",
            Icon::Warn => "warn",
            Icon::Error => "error",
            Icon::Refused => "refused",
            Icon::Tape => "tape",
            Icon::Wrist => "wrist",
            Icon::Terminal => "terminal",
            Icon::Search => "search",
            Icon::Add => "add",
            Icon::External => "external",
            Icon::Lock => "lock",
            Icon::Save => "save",
        }
    }
}

/// The inside of `<symbol id="i-NAME" ...>` in the icon file.
pub fn icon_source(name: &str) -> Option<&'static str> {
    let start = format!("<symbol id=\"i-{name}\"");
    let at = ICONS_SVG.find(&start)?;
    let rest = &ICONS_SVG[at..];
    let open = rest.find('>')? + 1;
    let close = rest.find("</symbol>")?;
    rest.get(open..close)
}

fn rasterise(svg: &str, w: u32, h: u32, sx: f32, sy: f32) -> Option<ColorImage> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(sx, sy),
        &mut pixmap.as_mut(),
    );
    Some(ColorImage::from_rgba_premultiplied(
        [w as usize, h as usize],
        pixmap.data(),
    ))
}

/// The icon as an SVG of its own, white strokes on nothing.
fn icon_svg(inner: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {g} {g}\" width=\"{g}\" height=\"{g}\" \
         fill=\"none\" stroke=\"#fff\" stroke-width=\"{s}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{inner}</svg>",
        g = size::ICON,
        s = theme::stroke::ICON,
    )
}

/// The wordmark as white on nothing: the ground rectangle goes, and
/// `currentColor` becomes white.
fn wordmark_svg() -> String {
    let mut s = WORDMARK_SVG.replace("currentColor", "#fff");
    if let Some(a) = s.find("<rect class=\"asset-ground\"")
        && let Some(len) = s[a..].find("/>")
    {
        s.replace_range(a..a + len + 2, "");
    }
    s
}

/// The wordmark file's own size.
fn wordmark_view() -> (f32, f32) {
    let parse = |key: &str| -> Option<f32> {
        let at = WORDMARK_SVG.find(key)? + key.len();
        let end = WORDMARK_SVG[at..].find('"')? + at;
        WORDMARK_SVG[at..end].parse().ok()
    };
    (
        parse("width=\"").unwrap_or(size::WORDMARK_H),
        parse("height=\"").unwrap_or(size::WORDMARK_H),
    )
}

/// One glyph of a bitmap font: rows of bits, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    /// One byte per row; bit 7 is the leftmost pixel.
    pub rows: Vec<u8>,
}

/// Read the 8 pixel wide glyphs of a BDF file, by character.
pub fn parse_bdf(bdf: &str) -> BTreeMap<char, Glyph> {
    let cell = text::LABEL_CELL_H as usize;
    let mut out = BTreeMap::new();
    let mut code: Option<u32> = None;
    let mut rows: Option<Vec<u8>> = None;
    for line in bdf.lines() {
        if let Some(v) = line.strip_prefix("ENCODING ") {
            code = v.trim().parse().ok();
        } else if line == "BITMAP" {
            rows = Some(Vec::new());
        } else if line == "ENDCHAR" {
            if let (Some(c), Some(r)) = (code.and_then(char::from_u32), rows.take())
                && r.len() == cell
            {
                out.insert(c, Glyph { rows: r });
            }
            code = None;
        } else if let Some(r) = rows.as_mut()
            && let Ok(b) = u8::from_str_radix(line.trim(), 16)
        {
            r.push(b);
        }
    }
    out
}

/// Glyphs per row of the atlas.
const ATLAS_COLUMNS: usize = 64;

/// The Spleen atlas: rows of 8 by 16 cells, one per character it holds.
pub struct LabelFont {
    texture: TextureHandle,
    index: BTreeMap<char, usize>,
    count: usize,
}

impl LabelFont {
    fn new(ctx: &egui::Context) -> Self {
        let glyphs = parse_bdf(theme::SPLEEN_8X16_BDF);
        let (cw, ch) = (text::LABEL_CELL_W as usize, text::LABEL_CELL_H as usize);
        let count = glyphs.len().max(1);
        let rows = count.div_ceil(ATLAS_COLUMNS);
        let width = cw * ATLAS_COLUMNS;
        let mut px = vec![Color32::TRANSPARENT; width * ch * rows];
        let mut index = BTreeMap::new();
        for (i, (c, g)) in glyphs.iter().enumerate() {
            index.insert(*c, i);
            for (y, bits) in g.rows.iter().enumerate() {
                for x in 0..cw {
                    if bits & (0x80 >> x) != 0 {
                        let (col, row) = (i % ATLAS_COLUMNS, i / ATLAS_COLUMNS);
                        px[(row * ch + y) * width + col * cw + x] = Color32::WHITE;
                    }
                }
            }
        }
        let image = ColorImage::new([width, ch * rows], px);
        let texture = ctx.load_texture("spleen-8x16", image, TextureOptions::NEAREST);
        Self {
            texture,
            index,
            count,
        }
    }

    /// The whole scale of a font pixel in device pixels, or None if the
    /// nearest whole scale is too far off and labels fall back to Plex Mono.
    pub fn device_scale(pixels_per_point: f32) -> Option<f32> {
        let k = pixels_per_point.round().max(1.0);
        ((k - pixels_per_point).abs() / pixels_per_point <= text::LABEL_SCALE_TOLERANCE)
            .then_some(k)
    }

    /// Paint `label` in caps with its top left at `pos`, snapped to device
    /// pixels. Returns its rectangle.
    pub fn paint(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        label: &str,
        color: Color32,
        k: f32,
    ) -> Rect {
        let ppp = painter.ctx().pixels_per_point();
        let cell = egui::vec2(text::LABEL_CELL_W, text::LABEL_CELL_H) * k / ppp;
        let origin = egui::pos2((pos.x * ppp).round() / ppp, (pos.y * ppp).round() / ppp);
        let mut mesh = egui::Mesh::with_texture(self.texture.id());
        let cols = ATLAS_COLUMNS as f32;
        let rows = self.count.div_ceil(ATLAS_COLUMNS) as f32;
        let mut x = origin.x;
        for c in label.chars().flat_map(char::to_uppercase) {
            if let Some(&i) = self.index.get(&c) {
                let (col, row) = ((i % ATLAS_COLUMNS) as f32, (i / ATLAS_COLUMNS) as f32);
                let uv = Rect::from_min_max(
                    egui::pos2(col / cols, row / rows),
                    egui::pos2((col + 1.0) / cols, (row + 1.0) / rows),
                );
                mesh.add_rect_with_uv(
                    Rect::from_min_size(egui::pos2(x, origin.y), cell),
                    uv,
                    color,
                );
            }
            x += cell.x;
        }
        painter.add(egui::Shape::mesh(mesh));
        Rect::from_min_max(origin, egui::pos2(x, origin.y + cell.y))
    }

    /// The width of `label` at scale `k`, in points.
    pub fn width(label: &str, k: f32, pixels_per_point: f32) -> f32 {
        label.chars().count() as f32 * text::LABEL_CELL_W * k / pixels_per_point
    }
}

/// Every texture the window draws, made for one display scale.
pub struct Art {
    /// The display scale they were made for.
    pub pixels_per_point: f32,
    /// The Spleen atlas.
    pub label: LabelFont,
    wordmark: Option<(TextureHandle, egui::Vec2)>,
    icons: BTreeMap<Icon, TextureHandle>,
    scanlines: Option<TextureHandle>,
}

impl Art {
    /// Make every texture for the context's display scale.
    pub fn new(ctx: &egui::Context, theme: &theme::Theme) -> Self {
        let ppp = ctx.pixels_per_point();
        let px = (size::ICON * ppp).round().max(1.0);
        let mut icons = BTreeMap::new();
        for icon in Icon::ALL {
            let Some(inner) = icon_source(icon.file_name()) else {
                continue;
            };
            let s = px / size::ICON;
            if let Some(img) = rasterise(&icon_svg(inner), px as u32, px as u32, s, s) {
                let t = ctx.load_texture(icon.file_name(), img, TextureOptions::LINEAR);
                icons.insert(icon, t);
            }
        }
        let (vw, vh) = wordmark_view();
        let h = (size::WORDMARK_H * ppp).round();
        let w = (vw * h / vh).round();
        let wordmark = rasterise(&wordmark_svg(), w as u32, h as u32, h / vh, h / vh).map(|img| {
            (
                ctx.load_texture("wordmark", img, TextureOptions::LINEAR),
                egui::vec2(w / ppp, h / ppp),
            )
        });
        let scanlines = theme.scanline_image().map(|img| {
            ctx.load_texture(
                "scanlines",
                img,
                TextureOptions {
                    wrap_mode: egui::TextureWrapMode::Repeat,
                    ..TextureOptions::NEAREST
                },
            )
        });
        Self {
            pixels_per_point: ppp,
            label: LabelFont::new(ctx),
            wordmark,
            icons,
            scanlines,
        }
    }

    /// Paint an icon in a 16 px square at `rect`'s centre.
    pub fn icon(&self, painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
        if let Some(t) = self.icons.get(&icon) {
            let r = Rect::from_center_size(rect.center(), egui::vec2(size::ICON, size::ICON));
            painter.image(
                t.id(),
                r,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                color,
            );
        }
    }

    /// The wordmark's size in points.
    pub fn wordmark_size(&self) -> egui::Vec2 {
        self.wordmark.as_ref().map_or(egui::Vec2::ZERO, |w| w.1)
    }

    /// Paint the wordmark with its top left at `pos`.
    pub fn wordmark(&self, painter: &egui::Painter, pos: Pos2, color: Color32) {
        if let Some((t, s)) = &self.wordmark {
            painter.image(
                t.id(),
                Rect::from_min_size(pos, *s),
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                color,
            );
        }
    }

    /// Paint the scanlines over `rect`, in device pixels so they never beat.
    pub fn scanlines(&self, painter: &egui::Painter, rect: Rect) {
        if let Some(t) = &self.scanlines {
            let ppp = self.pixels_per_point;
            let rows = rect.height() * ppp / theme::effect::SCANLINE_PERIOD as f32;
            let uv = Rect::from_min_max(Pos2::ZERO, egui::pos2(rect.width() * ppp, rows));
            painter.image(t.id(), rect, uv, Color32::WHITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spleen_has_ascii() {
        let g = parse_bdf(theme::SPLEEN_8X16_BDF);
        for c in ' '..='~' {
            assert!(g.contains_key(&c), "{c:?}");
        }
        assert_eq!(g[&'A'].rows.len(), 16);
    }

    #[test]
    fn every_icon_is_in_the_file() {
        for i in Icon::ALL {
            assert!(icon_source(i.file_name()).is_some(), "{i:?}");
        }
    }

    #[test]
    fn spleen_falls_back_off_whole_scales() {
        assert_eq!(LabelFont::device_scale(1.0), Some(1.0));
        assert_eq!(LabelFont::device_scale(2.0), Some(2.0));
        assert_eq!(LabelFont::device_scale(1.5), None);
        assert_eq!(LabelFont::device_scale(1.25), None);
    }

    #[test]
    fn wordmark_loses_its_ground() {
        let s = wordmark_svg();
        assert!(!s.contains("asset-ground"));
        assert!(!s.contains("currentColor"));
    }
}
