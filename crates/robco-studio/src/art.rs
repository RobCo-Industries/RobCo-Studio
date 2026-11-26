// SPDX-License-Identifier: GPL-3.0-or-later

//! The drawings and the label font as textures: the wordmarks, the atom, the
//! skyline, the device, the tape, the card, the icons and the Spleen glyph
//! atlas with its halo. Each is white on alpha and tinted when drawn.
//!
//! One loader serves every drawing file. It removes the file's ground
//! rectangle, turns `currentColor` white, drops the group with class `halo`
//! unless the screen is Soft, and rasterises the rest at a whole number of
//! device pixels. The textures are made again when the display scale
//! changes or the screen goes to or from Soft. The device is made for a zoom step when it is
//! first drawn at that step.

use crate::theme::{self, effect, size, text};
use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use std::cell::RefCell;
use std::collections::BTreeMap;

const WORDMARK_SVG: &str = include_str!("../assets/art/wordmark.svg");
const INDUSTRIES_SVG: &str = include_str!("../assets/art/wordmark-industries.svg");
const ATOM_SVG: &str = include_str!("../assets/art/atom.svg");
const SKYLINE_SVG: &str = include_str!("../assets/art/skyline.svg");
const DEVICE_SVG: &str = include_str!("../assets/art/device-frame.svg");
const TAPE_SVG: &str = include_str!("../assets/art/holotape.svg");
const CARD_SVG: &str = include_str!("../assets/art/card.svg");
const APP_ICON_SVG: &str = include_str!("../assets/art/app-icon.svg");
const ICONS_SVG: &str = include_str!("../assets/art/icons.svg");

/// A drawing file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Drawing {
    /// "RobCo Studio".
    Wordmark,
    /// "RobCo Industries", for the footer.
    Industries,
    /// The atom mark.
    Atom,
    /// The works skyline at the right of the header.
    Skyline,
    /// The wrist device around the simulator's screen.
    Device,
    /// The holotape.
    Tape,
    /// The memory card.
    Card,
}

impl Drawing {
    /// Every drawing.
    pub const ALL: [Drawing; 7] = [
        Drawing::Wordmark,
        Drawing::Industries,
        Drawing::Atom,
        Drawing::Skyline,
        Drawing::Device,
        Drawing::Tape,
        Drawing::Card,
    ];

    /// The file's text.
    pub fn source(self) -> &'static str {
        match self {
            Drawing::Wordmark => WORDMARK_SVG,
            Drawing::Industries => INDUSTRIES_SVG,
            Drawing::Atom => ATOM_SVG,
            Drawing::Skyline => SKYLINE_SVG,
            Drawing::Device => DEVICE_SVG,
            Drawing::Tape => TAPE_SVG,
            Drawing::Card => CARD_SVG,
        }
    }

    /// The file's view box: left, top, width, height. A file that keeps room
    /// for its glow starts below zero.
    pub fn view_box(self) -> [f32; 4] {
        view_box(self.source()).unwrap_or([0.0, 0.0, size::ICON, size::ICON])
    }

    /// The drawing's box in file units, from the origin. A file that keeps
    /// room for its glow says "fills the box 0 0 W H" in its comment; any
    /// other file fills its view box.
    pub fn box_units(self) -> Vec2 {
        let key = "fills the box 0 0 ";
        let stated = self.source().find(key).and_then(|at| {
            let mut v = self.source()[at + key.len()..]
                .split(|c: char| !c.is_ascii_digit())
                .filter(|w| !w.is_empty())
                .map(str::parse::<f32>);
            Some(egui::vec2(v.next()?.ok()?, v.next()?.ok()?))
        });
        let [_, _, w, h] = self.view_box();
        stated.unwrap_or(egui::vec2(w, h))
    }
}

/// A drawing at the size the window shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    /// The Studio wordmark in the tall header.
    Wordmark,
    /// The Studio wordmark in the compact header.
    WordmarkCompact,
    /// The Industries wordmark in the footer.
    Industries,
    /// The atom in the tall header.
    Atom,
    /// The atom in the compact header.
    AtomCompact,
    /// The skyline.
    Skyline,
    /// The tape in the Tape pane.
    Tape,
    /// The card.
    Card,
}

impl Mark {
    /// Every mark.
    pub const ALL: [Mark; 8] = [
        Mark::Wordmark,
        Mark::WordmarkCompact,
        Mark::Industries,
        Mark::Atom,
        Mark::AtomCompact,
        Mark::Skyline,
        Mark::Tape,
        Mark::Card,
    ];

    /// Its drawing.
    pub fn drawing(self) -> Drawing {
        match self {
            Mark::Wordmark | Mark::WordmarkCompact => Drawing::Wordmark,
            Mark::Industries => Drawing::Industries,
            Mark::Atom | Mark::AtomCompact => Drawing::Atom,
            Mark::Skyline => Drawing::Skyline,
            Mark::Tape => Drawing::Tape,
            Mark::Card => Drawing::Card,
        }
    }

    /// The height of its box, in points.
    pub fn box_h(self) -> f32 {
        match self {
            Mark::Wordmark => size::WORDMARK_H,
            Mark::WordmarkCompact => size::WORDMARK_COMPACT_H,
            Mark::Industries => size::FOOTER_MARK_H,
            Mark::Atom => size::ATOM,
            Mark::AtomCompact => size::ATOM_COMPACT,
            Mark::Skyline => size::SKYLINE[1],
            Mark::Tape => size::TAPE_ART[1],
            Mark::Card => size::CARD_ART[1],
        }
    }
}

/// The zoom steps of the device.
pub const DEVICE_ZOOMS: [u8; 3] = [1, 2, 3];

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
    Stop,
    Reset,
    Fit,
    Remove,
    Keys,
    Centre,
    Side,
    Export,
    Level,
    Wait,
    Pause,
}

impl Icon {
    /// Every icon.
    pub const ALL: [Icon; 41] = [
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
        Icon::Stop,
        Icon::Reset,
        Icon::Fit,
        Icon::Remove,
        Icon::Keys,
        Icon::Centre,
        Icon::Side,
        Icon::Export,
        Icon::Level,
        Icon::Wait,
        Icon::Pause,
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
            Icon::Stop => "stop",
            Icon::Reset => "reset",
            Icon::Fit => "fit",
            Icon::Remove => "remove",
            Icon::Keys => "keys",
            Icon::Centre => "centre",
            Icon::Side => "side",
            Icon::Export => "export",
            Icon::Level => "level",
            Icon::Wait => "wait",
            Icon::Pause => "pause",
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

/// Rasterise `svg` to fill `w` by `h` pixels with its view box, as
/// premultiplied RGBA bytes.
fn rasterise_bytes(svg: &str, w: u32, h: u32) -> Option<Vec<u8>> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let size = tree.size();
    let t = tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(&tree, t, &mut pixmap.as_mut());
    Some(pixmap.take())
}

fn rasterise(svg: &str, w: u32, h: u32) -> Option<ColorImage> {
    let bytes = rasterise_bytes(svg, w, h)?;
    Some(ColorImage::from_rgba_premultiplied(
        [w as usize, h as usize],
        &bytes,
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

/// The `viewBox` of the root element.
fn view_box(svg: &str) -> Option<[f32; 4]> {
    let key = "viewBox=\"";
    let at = svg.find(key)? + key.len();
    let end = svg[at..].find('"')? + at;
    let mut v = svg[at..end].split_whitespace().map(str::parse::<f32>);
    Some([
        v.next()?.ok()?,
        v.next()?.ok()?,
        v.next()?.ok()?,
        v.next()?.ok()?,
    ])
}

/// `svg` without the element that starts at `at`: an empty element up to
/// its `/>`, or a group up to its matching `</g>`.
fn cut_element(svg: &mut String, at: usize) {
    let rest = &svg[at..];
    let Some(tag_end) = rest.find('>') else {
        return;
    };
    if rest[..tag_end].ends_with('/') {
        svg.replace_range(at..at + tag_end + 1, "");
        return;
    }
    let mut depth = 0usize;
    let mut i = at;
    while let Some(off) = svg[i..].find('<') {
        let j = i + off;
        let tail = &svg[j..];
        let end = tail.find('>').map_or(tail.len(), |e| e + 1);
        if tail.starts_with("</g") {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                svg.replace_range(at..j + end, "");
                return;
            }
        } else if tail.starts_with("<g") && !tail[..end].ends_with("/>") {
            depth += 1;
        }
        i = j + end;
    }
}

/// `svg` without every element whose class is `class`.
pub fn remove_class(svg: &str, class: &str) -> String {
    let mut s = svg.to_owned();
    let mark = format!("class=\"{class}\"");
    while let Some(c) = s.find(&mark) {
        let Some(open) = s[..c].rfind('<') else {
            break;
        };
        let before = s.len();
        cut_element(&mut s, open);
        if s.len() == before {
            break;
        }
    }
    s
}

/// A drawing file made ready for the window: white on nothing, and
/// without its halo group unless `halo`.
pub fn prepare(svg: &str, halo: bool) -> String {
    let s = remove_class(svg, "asset-ground").replace("currentColor", "#fff");
    if halo { s } else { remove_class(&s, "halo") }
}

/// A drawing rasterised for the window.
pub struct Raster {
    /// White on alpha, premultiplied.
    pub image: ColorImage,
    /// Where the image's top left sits from the box's top left, in device
    /// pixels. It is at or above and left of the box when the file keeps
    /// room for its glow.
    pub offset: [i32; 2],
}

/// Rasterise a drawing so that its box is `box_h` points high on a display
/// of `pixels_per_point`. The image covers the whole view box, glow room
/// included, at a whole number of device pixels.
pub fn render(d: Drawing, box_h: f32, pixels_per_point: f32, halo: bool) -> Option<Raster> {
    let [x, y, w, h] = d.view_box();
    let s = box_h * pixels_per_point / d.box_units().y;
    let (pw, ph) = ((w * s).round(), (h * s).round());
    let (ox, oy) = ((x * s).round(), (y * s).round());
    let svg = prepare(d.source(), halo);
    let bytes = rasterise_bytes(&svg, pw as u32, ph as u32)?;
    Some(Raster {
        image: ColorImage::from_rgba_premultiplied([pw as usize, ph as usize], &bytes),
        offset: [ox as i32, oy as i32],
    })
}

/// The application icon file, in its small form or its full one.
fn app_icon_svg(small: bool) -> String {
    let mut svg = APP_ICON_SVG.to_owned();
    if small {
        for class in ["halo", "orbits", "letter"] {
            svg = remove_class(&svg, class);
        }
        svg = svg.replace("class=\"small\" display=\"none\"", "class=\"small\"");
    }
    svg
}

/// The application icon, `px` pixels square, with straight alpha. At 32
/// pixels and below it shows the file's "small" group, the letter alone.
pub fn app_icon(px: u32) -> Option<egui::IconData> {
    app_icon_form(px <= size::APP_ICON_SMALL_MAX, px)
}

fn app_icon_form(small: bool, px: u32) -> Option<egui::IconData> {
    let svg = app_icon_svg(small);
    let bytes = rasterise_bytes(&svg, px, px)?;
    let mut rgba = Vec::with_capacity(bytes.len());
    for p in bytes.as_chunks::<4>().0 {
        let a = u32::from(p[3]);
        let straight = |v: u8| {
            (u32::from(v) * 255 + a / 2)
                .checked_div(a)
                .map_or(0, |c| c.min(255) as u8)
        };
        rgba.extend_from_slice(&[straight(p[0]), straight(p[1]), straight(p[2]), p[3]]);
    }
    Some(egui::IconData {
        rgba,
        width: px,
        height: px,
    })
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

/// A Gaussian of `sigma`, `radius` taps to each side, summing to one.
fn gaussian(sigma: f32, radius: usize) -> Vec<f32> {
    let k: Vec<f32> = (0..=2 * radius)
        .map(|i| {
            let x = i as f32 - radius as f32;
            (-(x * x) / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let sum: f32 = k.iter().sum();
    k.into_iter().map(|v| v / sum).collect()
}

/// Blur an alpha grid of `w` by `h` once across and once down.
fn blur(alpha: &[f32], w: usize, h: usize, kernel: &[f32]) -> Vec<f32> {
    let r = kernel.len() / 2;
    let pass = |src: &[f32], across: bool| -> Vec<f32> {
        let mut out = vec![0.0; src.len()];
        for y in 0..h {
            for x in 0..w {
                let mut v = 0.0;
                for (i, k) in kernel.iter().enumerate() {
                    let (sx, sy) = if across {
                        ((x + i).checked_sub(r), Some(y))
                    } else {
                        (Some(x), (y + i).checked_sub(r))
                    };
                    if let (Some(sx), Some(sy)) = (sx, sy)
                        && sx < w
                        && sy < h
                    {
                        v += k * src[sy * w + sx];
                    }
                }
                out[y * w + x] = v;
            }
        }
        out
    };
    pass(&pass(alpha, true), false)
}

/// The Spleen atlas: rows of 8 by 16 cells, one per character it holds,
/// and its halo: the same glyphs in larger cells, blurred.
pub struct LabelFont {
    texture: TextureHandle,
    halo: TextureHandle,
    index: BTreeMap<char, usize>,
    count: usize,
}

/// The halo atlas as alpha, `cells` by `rows` glyph cells with room for the
/// blur: its width, its height and its alpha, out of 1.
pub fn halo_atlas(glyphs: &BTreeMap<char, Glyph>) -> (usize, usize, Vec<f32>) {
    let (cw, ch) = (text::LABEL_CELL_W as usize, text::LABEL_CELL_H as usize);
    let pad = effect::LABEL_HALO_PAD;
    let (hw, hh) = (cw + 2 * pad, ch + 2 * pad);
    let rows = glyphs.len().max(1).div_ceil(ATLAS_COLUMNS);
    let (w, h) = (hw * ATLAS_COLUMNS, hh * rows);
    let mut a = vec![0.0; w * h];
    for (i, g) in glyphs.values().enumerate() {
        let (col, row) = (i % ATLAS_COLUMNS, i / ATLAS_COLUMNS);
        for (y, bits) in g.rows.iter().enumerate() {
            for x in 0..cw {
                if bits & (0x80 >> x) != 0 {
                    a[(row * hh + pad + y) * w + col * hw + pad + x] = 1.0;
                }
            }
        }
    }
    let k = gaussian(effect::LABEL_HALO_SIGMA, pad);
    (w, h, blur(&a, w, h, &k))
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
        let (hw, hh, alpha) = halo_atlas(&glyphs);
        let bytes: Vec<u8> = alpha
            .iter()
            .flat_map(|a| [(a.clamp(0.0, 1.0) * f32::from(u8::MAX)).round() as u8; 4])
            .collect();
        let halo = ctx.load_texture(
            "spleen-8x16-halo",
            ColorImage::from_rgba_premultiplied([hw, hh], &bytes),
            TextureOptions::LINEAR,
        );
        Self {
            texture,
            halo,
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

    /// One mesh of `label` from an atlas whose cells have `pad` font pixels
    /// of room. Returns the mesh and the label's rectangle.
    #[allow(clippy::too_many_arguments)]
    fn mesh(
        &self,
        texture: &TextureHandle,
        pad: usize,
        origin: Pos2,
        label: &str,
        color: Color32,
        k: f32,
        tracking: f32,
        ppp: f32,
    ) -> (egui::Mesh, Rect) {
        let font_px = k / ppp;
        let cell = egui::vec2(text::LABEL_CELL_W, text::LABEL_CELL_H) * font_px;
        let room = pad as f32 * font_px;
        let advance = (text::LABEL_CELL_W + tracking) * font_px;
        let mut mesh = egui::Mesh::with_texture(texture.id());
        let cols = ATLAS_COLUMNS as f32;
        let rows = self.count.div_ceil(ATLAS_COLUMNS) as f32;
        let mut x = origin.x;
        let mut n = 0usize;
        for c in label.chars().flat_map(char::to_uppercase) {
            if let Some(&i) = self.index.get(&c) {
                let (col, row) = ((i % ATLAS_COLUMNS) as f32, (i / ATLAS_COLUMNS) as f32);
                let uv = Rect::from_min_max(
                    egui::pos2(col / cols, row / rows),
                    egui::pos2((col + 1.0) / cols, (row + 1.0) / rows),
                );
                let r = Rect::from_min_size(egui::pos2(x, origin.y), cell).expand(room);
                mesh.add_rect_with_uv(r, uv, color);
            }
            x += advance;
            n += 1;
        }
        let right = if n == 0 {
            origin.x
        } else {
            x - tracking * font_px
        };
        (
            mesh,
            Rect::from_min_max(origin, egui::pos2(right, origin.y + cell.y)),
        )
    }

    fn origin(pos: Pos2, ppp: f32) -> Pos2 {
        egui::pos2((pos.x * ppp).round() / ppp, (pos.y * ppp).round() / ppp)
    }

    /// Paint `label` in caps with its top left at `pos`, snapped to device
    /// pixels, with `tracking` font pixels added to each advance. Returns
    /// its rectangle.
    pub fn paint(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        label: &str,
        color: Color32,
        k: f32,
        tracking: f32,
    ) -> Rect {
        let ppp = painter.ctx().pixels_per_point();
        let origin = Self::origin(pos, ppp);
        let (mesh, rect) = self.mesh(&self.texture, 0, origin, label, color, k, tracking, ppp);
        painter.add(egui::Shape::mesh(mesh));
        rect
    }

    /// Paint `label` as [`LabelFont::paint`] does, over its halo tinted in
    /// the label colour at `halo`, out of 1. With None it paints no halo.
    /// For `bright` labels on `ground` and `pane` only.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_lit(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        label: &str,
        color: Color32,
        k: f32,
        tracking: f32,
        halo: Option<f32>,
    ) -> Rect {
        if let Some(a) = halo {
            let ppp = painter.ctx().pixels_per_point();
            let origin = Self::origin(pos, ppp);
            let pad = effect::LABEL_HALO_PAD;
            let tint = color.gamma_multiply(a);
            let (mesh, _) = self.mesh(&self.halo, pad, origin, label, tint, k, tracking, ppp);
            painter.add(egui::Shape::mesh(mesh));
        }
        self.paint(painter, pos, label, color, k, tracking)
    }

    /// The width of `label` at scale `k` with `tracking` font pixels added
    /// to each advance but the last, in points.
    pub fn width(label: &str, k: f32, pixels_per_point: f32, tracking: f32) -> f32 {
        let n = label.chars().count() as f32;
        let tracks = (n - 1.0).max(0.0);
        (n * text::LABEL_CELL_W + tracks * tracking) * k / pixels_per_point
    }
}

/// A texture of a drawing and where it sits from its box.
struct Placed {
    texture: TextureHandle,
    /// The texture's top left from the box's top left, in device pixels.
    offset: [i32; 2],
}

impl Placed {
    fn load(ctx: &egui::Context, name: &str, r: Raster) -> Self {
        Self {
            texture: ctx.load_texture(name, r.image, TextureOptions::LINEAR),
            offset: r.offset,
        }
    }

    /// Paint it with its box's top left at `pos`, on whole device pixels.
    fn paint(&self, painter: &egui::Painter, pos: Pos2, ppp: f32, color: Color32) {
        let [w, h] = self.texture.size();
        let min = egui::pos2(
            ((pos.x * ppp).round() + self.offset[0] as f32) / ppp,
            ((pos.y * ppp).round() + self.offset[1] as f32) / ppp,
        );
        let r = Rect::from_min_size(min, egui::vec2(w as f32, h as f32) / ppp);
        painter.image(
            self.texture.id(),
            r,
            Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
            color,
        );
    }
}

/// Every texture the window draws, made for one display scale, with or
/// without the still glow.
pub struct Art {
    /// The display scale they were made for.
    pub pixels_per_point: f32,
    /// They were made with the still glow: the baked halo groups and the
    /// scanline texture. Set once, when the art is made.
    halos: bool,
    /// The Spleen atlas.
    pub label: LabelFont,
    marks: BTreeMap<Mark, Placed>,
    devices: RefCell<BTreeMap<u8, Placed>>,
    icons: BTreeMap<Icon, TextureHandle>,
    scanlines: Option<TextureHandle>,
}

impl Art {
    /// Make every texture for the context's display scale and the theme's
    /// screen level.
    pub fn new(ctx: &egui::Context, theme: &theme::Theme) -> Self {
        let ppp = ctx.pixels_per_point();
        let halos = theme.still_glow();
        let px = (size::ICON * ppp).round().max(1.0);
        let mut icons = BTreeMap::new();
        for icon in Icon::ALL {
            let Some(inner) = icon_source(icon.file_name()) else {
                continue;
            };
            if let Some(img) = rasterise(&icon_svg(inner), px as u32, px as u32) {
                let t = ctx.load_texture(icon.file_name(), img, TextureOptions::LINEAR);
                icons.insert(icon, t);
            }
        }
        let mut marks = BTreeMap::new();
        for m in Mark::ALL {
            if let Some(r) = render(m.drawing(), m.box_h(), ppp, halos) {
                marks.insert(m, Placed::load(ctx, &format!("{m:?}"), r));
            }
        }
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
            halos,
            label: LabelFont::new(ctx),
            marks,
            devices: RefCell::new(BTreeMap::new()),
            icons,
            scanlines,
        }
    }

    /// The art was made with the still glow: the drawings hold their baked
    /// halo groups and the scanline texture exists.
    pub fn halos(&self) -> bool {
        self.halos
    }

    /// The scanline texture was made.
    pub fn has_scanlines(&self) -> bool {
        self.scanlines.is_some()
    }

    /// The art was made for this display scale and for the glow state the
    /// theme's level needs now. When false, make it again.
    pub fn fits(&self, pixels_per_point: f32, theme: &theme::Theme) -> bool {
        self.pixels_per_point == pixels_per_point && self.halos == theme.still_glow()
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

    /// The size of a mark's box in points, glow room left out.
    pub fn mark_size(m: Mark) -> Vec2 {
        let u = m.drawing().box_units();
        egui::vec2(u.x * m.box_h() / u.y, m.box_h())
    }

    /// Paint a mark with its box's top left at `pos`. Its glow reaches
    /// out of the box where the file keeps room for it. Returns the box.
    pub fn mark(&self, painter: &egui::Painter, m: Mark, pos: Pos2, color: Color32) -> Rect {
        if let Some(p) = self.marks.get(&m) {
            p.paint(painter, pos, self.pixels_per_point, color);
        }
        Rect::from_min_size(pos, Self::mark_size(m))
    }

    /// The device's box at a zoom step, in points.
    pub fn device_size(zoom: u8) -> Vec2 {
        egui::Vec2::from(size::DEVICE) * f32::from(zoom.max(1))
    }

    /// Paint the device with its box's top left at `pos`, at a zoom step.
    /// The texture for a step is made the first time it is drawn. The
    /// texture reaches 12 left of and 14 above the box per zoom step.
    pub fn device(&self, painter: &egui::Painter, pos: Pos2, zoom: u8, color: Color32) -> Rect {
        let zoom = zoom.max(1);
        let size = Self::device_size(zoom);
        let mut cache = self.devices.borrow_mut();
        if !cache.contains_key(&zoom)
            && let Some(r) = render(Drawing::Device, size.y, self.pixels_per_point, self.halos)
        {
            let p = Placed::load(painter.ctx(), &format!("device-{zoom}"), r);
            cache.insert(zoom, p);
        }
        if let Some(p) = cache.get(&zoom) {
            p.paint(painter, pos, self.pixels_per_point, color);
        }
        Rect::from_min_size(pos, size)
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

    fn alpha_at(svg: &str, x: u32, y: u32) -> u8 {
        use resvg::{tiny_skia, usvg};
        let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).unwrap();
        let mut pm = tiny_skia::Pixmap::new(40, 40).unwrap();
        resvg::render(&tree, tiny_skia::Transform::identity(), &mut pm.as_mut());
        pm.pixel(x, y).unwrap().alpha()
    }

    const PROBE: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"40\">";

    /// The pinned resvg runs the three features the drawings use.
    #[test]
    fn resvg_runs_blur_gradient_and_clip() {
        let rect = "<rect x=\"15\" y=\"15\" width=\"10\" height=\"10\" fill=\"#fff\"/>";
        let blur = format!(
            "{PROBE}<defs><filter id=\"b\" x=\"-100%\" y=\"-100%\" width=\"300%\" height=\"300%\">\
             <feGaussianBlur stdDeviation=\"3\"/></filter></defs><g filter=\"url(#b)\">{rect}</g></svg>"
        );
        let sharp = format!("{PROBE}{rect}</svg>");
        assert_eq!(alpha_at(&sharp, 12, 20), 0);
        assert!(alpha_at(&blur, 12, 20) > 0, "no blur");

        let grad = format!(
            "{PROBE}<defs><linearGradient id=\"g\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"0\">\
             <stop offset=\"0\" stop-color=\"#fff\" stop-opacity=\".1\"/>\
             <stop offset=\"1\" stop-color=\"#fff\" stop-opacity=\"1\"/></linearGradient></defs>\
             <rect width=\"40\" height=\"40\" fill=\"url(#g)\"/></svg>"
        );
        assert!(
            alpha_at(&grad, 35, 20) > alpha_at(&grad, 5, 20) + 100,
            "no gradient"
        );

        let clip = format!(
            "{PROBE}<defs><clipPath id=\"c\"><rect width=\"20\" height=\"40\"/></clipPath></defs>\
             <rect width=\"40\" height=\"40\" fill=\"#fff\" clip-path=\"url(#c)\"/></svg>"
        );
        assert_eq!(alpha_at(&clip, 10, 20), 255);
        assert_eq!(alpha_at(&clip, 30, 20), 0, "no clip");
    }

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
    fn prepare_drops_ground_and_halo() {
        for d in Drawing::ALL {
            let on = prepare(d.source(), true);
            assert!(!on.contains("asset-ground"), "{d:?}");
            assert!(!on.contains("currentColor"), "{d:?}");
            assert!(on.contains("class=\"halo\""), "{d:?} has no halo group");
            let off = prepare(d.source(), false);
            assert!(!off.contains("class=\"halo\""), "{d:?}");
            assert!(usvg_ok(&off), "{d:?} no longer parses without its halo");
        }
    }

    fn usvg_ok(svg: &str) -> bool {
        resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).is_ok()
    }

    #[test]
    fn remove_class_keeps_what_follows_a_nested_group() {
        let s = "<svg><g class=\"halo\"><g><path/></g><use/></g><g class=\"keep\"/></svg>";
        assert_eq!(remove_class(s, "halo"), "<svg><g class=\"keep\"/></svg>");
        let r = "<svg><rect class=\"asset-ground\" width=\"1\"/><path/></svg>";
        assert_eq!(remove_class(r, "asset-ground"), "<svg><path/></svg>");
    }

    /// Every drawing rasterises at its size, is not empty, is white on
    /// alpha, and changes with its halo group.
    #[test]
    fn every_drawing_rasterises_white_on_alpha() {
        let sizes: Vec<(Drawing, f32)> = Mark::ALL
            .iter()
            .map(|m| (m.drawing(), m.box_h()))
            .chain([(Drawing::Device, size::DEVICE[1])])
            .collect();
        for (d, h) in sizes {
            let [_, _, vw, vh] = d.view_box();
            let s = h / d.box_units().y;
            let on = render(d, h, 1.0, true).unwrap();
            let off = render(d, h, 1.0, false).unwrap();
            assert_eq!(
                on.image.size,
                [(vw * s).round() as usize, (vh * s).round() as usize]
            );
            let lit = on.image.pixels.iter().filter(|p| p.a() > 0).count();
            assert!(lit > on.image.pixels.len() / 50, "{d:?} is nearly empty");
            // The device and the tape each hold one black fill on purpose.
            let black_ok = matches!(d, Drawing::Device | Drawing::Tape);
            for p in &on.image.pixels {
                assert!(p.r() == p.g() && p.g() == p.b(), "{d:?} is not grey: {p:?}");
                assert!(black_ok || p.r() == p.a(), "{d:?} is not white: {p:?}");
            }
            assert_ne!(on.image.pixels, off.image.pixels, "{d:?} has no halo");
            let outer = |img: &ColorImage| -> usize {
                let [w, h] = img.size;
                img.pixels
                    .iter()
                    .enumerate()
                    .filter(|(i, p)| {
                        let (x, y) = (i % w, i / w);
                        p.a() > 0 && (x < 2 || y < 2 || x + 2 >= w || y + 2 >= h)
                    })
                    .count()
            };
            assert!(outer(&on.image) >= outer(&off.image), "{d:?}");
        }
    }

    /// The halo adds lit pixels outside the sharp drawing.
    #[test]
    fn the_halo_glows_outside_the_lines() {
        for d in Drawing::ALL {
            let on = render(d, d.box_units().y, 1.0, true).unwrap();
            let off = render(d, d.box_units().y, 1.0, false).unwrap();
            let glow = on
                .image
                .pixels
                .iter()
                .zip(&off.image.pixels)
                .filter(|(a, b)| b.a() == 0 && a.a() > 0)
                .count();
            assert!(glow > 0, "{d:?}");
        }
    }

    #[test]
    fn sizes_and_offsets_follow_the_files() {
        assert_eq!(Art::mark_size(Mark::Wordmark).x.round(), 276.0);
        assert_eq!(Art::mark_size(Mark::WordmarkCompact).x.round(), 157.0);
        assert_eq!(Art::mark_size(Mark::Industries).x.round(), 146.0);
        assert_eq!(
            Art::mark_size(Mark::Atom),
            egui::vec2(size::ATOM, size::ATOM)
        );
        assert_eq!(Drawing::Skyline.view_box(), [0.0, 0.0, 392.0, 60.0]);
        assert_eq!(
            Art::mark_size(Mark::Skyline),
            egui::Vec2::from(size::SKYLINE)
        );
        assert_eq!(Art::mark_size(Mark::Tape).y, size::TAPE_ART[1]);
        assert_eq!(Drawing::Device.box_units(), egui::Vec2::from(size::DEVICE));
        let d = render(Drawing::Device, size::DEVICE[1], 1.0, true).unwrap();
        assert_eq!(d.image.size, [416, 362]);
        let bleed = size::DEVICE_BLEED;
        assert_eq!(d.offset, [-bleed[0] as i32, -bleed[1] as i32]);
        let d2 = render(Drawing::Device, size::DEVICE[1], 2.0, true).unwrap();
        assert_eq!(d2.image.size, [832, 724]);
        assert_eq!(d2.offset, [-24, -28]);
        let t = render(Drawing::Tape, size::TAPE_ART[1], 1.0, true).unwrap();
        assert_eq!(t.image.size[1], 83);
    }

    /// The device's leftmost line, its strap lug at x 0 of its box, lands
    /// 12 device pixels into the texture: the glow room, once.
    #[test]
    fn the_box_lands_where_the_offset_says() {
        let d = render(Drawing::Device, size::DEVICE[1], 1.0, false).unwrap();
        let w = d.image.size[0];
        let left = d
            .image
            .pixels
            .iter()
            .enumerate()
            .filter(|(_, p)| p.a() > 0)
            .map(|(i, _)| i % w)
            .min()
            .unwrap();
        assert!((10..=12).contains(&left), "{left}");
        assert_eq!(d.offset[0], -12);
    }

    #[test]
    fn app_icon_has_a_small_form() {
        let big = app_icon(size::APP_ICON).unwrap();
        assert_eq!(
            big.rgba.len(),
            (size::APP_ICON * size::APP_ICON * 4) as usize
        );
        assert!(big.rgba.iter().skip(3).step_by(4).any(|a| *a > 0));
        let small = app_icon_svg(true);
        for gone in [
            "class=\"halo\"",
            "class=\"orbits\"",
            "class=\"letter\"",
            "display=\"none\"",
        ] {
            assert!(!small.contains(gone), "{gone}");
        }
        assert!(app_icon_svg(false).contains("display=\"none\""));
        let s = app_icon(size::APP_ICON_SMALL_MAX).unwrap();
        assert!(s.rgba.iter().skip(3).step_by(4).any(|a| *a > 0));
        let full = app_icon_form(false, size::APP_ICON_SMALL_MAX).unwrap();
        assert_ne!(s.rgba, full.rgba, "32 px shows the full mark");
        let next = app_icon(size::APP_ICON_SMALL_MAX * 2).unwrap();
        assert_eq!(
            next.rgba,
            app_icon_form(false, size::APP_ICON_SMALL_MAX * 2)
                .unwrap()
                .rgba
        );
    }

    #[test]
    fn tracking_widens_each_advance_but_the_last() {
        assert_eq!(LabelFont::width("ABC", 1.0, 1.0, 0.0), 24.0);
        assert_eq!(
            LabelFont::width("ABC", 1.0, 1.0, text::LABEL_TRACKING),
            28.0
        );
        assert_eq!(
            LabelFont::width("ABC", 2.0, 1.0, text::LABEL_TRACKING),
            56.0
        );
        assert_eq!(LabelFont::width("", 1.0, 1.0, text::LABEL_TRACKING), 0.0);
        let ctx = egui::Context::default();
        let font = LabelFont::new(&ctx);
        let painter = egui::Painter::new(
            ctx,
            egui::LayerId::background(),
            Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 50.0)),
        );
        let r = font.paint(
            &painter,
            Pos2::ZERO,
            "ABC",
            Color32::WHITE,
            1.0,
            text::LABEL_TRACKING,
        );
        assert_eq!(r.width(), 28.0);
        let lit = font.paint_lit(
            &painter,
            Pos2::ZERO,
            "ABC",
            Color32::WHITE,
            1.0,
            0.0,
            Some(0.3),
        );
        assert_eq!(lit.width(), 24.0);
    }

    /// The halo atlas glows around each glyph and stays inside its cell.
    #[test]
    fn halo_atlas_blurs_into_the_room() {
        let mut g = BTreeMap::new();
        g.insert(
            'I',
            Glyph {
                rows: vec![0x10; 16],
            },
        );
        let (w, _, a) = halo_atlas(&g);
        let pad = effect::LABEL_HALO_PAD;
        let at = |x: usize, y: usize| a[y * w + x];
        // The glyph is the column x = 3 of its cell.
        let (gx, gy) = (pad + 3, pad + 8);
        assert!(at(gx, gy) < 1.0, "not blurred");
        assert!(at(gx + 2, gy) > 0.0, "no glow beside the stroke");
        assert!(at(gx, 1) > 0.0, "no glow above the glyph");
        assert_eq!(at(0, gy), 0.0);
        let total: f32 = a.iter().sum();
        assert!(
            (total - 16.0).abs() < 1.0,
            "the blur loses or adds light: {total}"
        );
    }

    /// The dark regions of `img`, alpha under `dark`, that touch no edge:
    /// the counters. Returns each one's pixel count.
    fn counters(img: &ColorImage, dark: u8) -> Vec<usize> {
        let [w, h] = img.size;
        let mut seen = vec![false; w * h];
        let mut out = Vec::new();
        for start in 0..w * h {
            if seen[start] || img.pixels[start].a() >= dark {
                continue;
            }
            let (mut stack, mut n, mut edge) = (vec![start], 0, false);
            seen[start] = true;
            while let Some(i) = stack.pop() {
                n += 1;
                let (x, y) = (i % w, i / w);
                edge |= x == 0 || y == 0 || x + 1 == w || y + 1 == h;
                let mut near = Vec::new();
                if x > 0 {
                    near.push(i - 1);
                }
                if x + 1 < w {
                    near.push(i + 1);
                }
                if y > 0 {
                    near.push(i - w);
                }
                if y + 1 < h {
                    near.push(i + w);
                }
                for j in near {
                    if !seen[j] && img.pixels[j].a() < dark {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
            if !edge {
                out.push(n);
            }
        }
        out
    }

    /// The alpha at a point given in file units, for a mark at a scale.
    fn alpha_at_unit(m: Mark, ppp: f32, u: [f32; 2]) -> u8 {
        let d = m.drawing();
        let r = render(d, m.box_h(), ppp, false).unwrap();
        let s = m.box_h() * ppp / d.box_units().y;
        let [x, y] = [(u[0] * s) as usize, (u[1] * s) as usize];
        r.image.pixels[y * r.image.size[0] + x].a()
    }

    /// The counters stay open at the sizes the window draws, at display
    /// scales 1 and 2: a point inside the R's counter is dark, and each
    /// mark keeps at least its number of closed dark regions.
    #[test]
    fn the_counters_stay_open() {
        // The middle of the R's counter, in file units.
        let r_studio = [35.6, 22.8];
        let r_industries = [30.8, 22.0];
        // Inside the upright orbit, above the nucleus.
        let atom_gap = [32.0, 14.0];
        let cases = [
            (Mark::Wordmark, r_studio, 6),
            (Mark::WordmarkCompact, r_studio, 6),
            (Mark::Industries, r_industries, 6),
            (Mark::Atom, atom_gap, 8),
            (Mark::AtomCompact, atom_gap, 1),
        ];
        for (m, at, holes) in cases {
            for ppp in [1.0, 2.0] {
                let a = alpha_at_unit(m, ppp, at);
                assert!(a < 96, "{m:?} at {ppp}: the counter is shut, alpha {a}");
                let r = render(m.drawing(), m.box_h(), ppp, false).unwrap();
                let c = counters(&r.image, 128);
                assert!(
                    c.len() >= holes,
                    "{m:?} at {ppp}: {} counters {c:?}",
                    c.len()
                );
            }
        }
    }

    /// Nothing sharp reaches out of the view box, so no part is cut: the
    /// file drawn with 8 more units on each side lights nothing outside it.
    #[test]
    fn no_sharp_part_is_cut_by_the_view_box() {
        for d in [
            Drawing::Wordmark,
            Drawing::Industries,
            Drawing::Atom,
            Drawing::Skyline,
        ] {
            let [x, y, w, h] = d.view_box();
            let pad = 8.0;
            let wide = prepare(d.source(), false).replacen(
                &format!("viewBox=\"{x} {y} {w} {h}\" width=\"{w}\" height=\"{h}\""),
                &format!(
                    "viewBox=\"{} {} {} {}\" width=\"{}\" height=\"{}\"",
                    x - pad,
                    y - pad,
                    w + 2.0 * pad,
                    h + 2.0 * pad,
                    w + 2.0 * pad,
                    h + 2.0 * pad
                ),
                1,
            );
            assert_ne!(wide, prepare(d.source(), false), "{d:?}");
            let (pw, ph) = ((w + 2.0 * pad) as u32, (h + 2.0 * pad) as u32);
            let px = rasterise_bytes(&wide, pw, ph).unwrap();
            let inside = |i: usize| {
                let (px_, py) = ((i as u32 % pw) as f32, (i as u32 / pw) as f32);
                px_ >= pad - 1.0 && py >= pad - 1.0 && px_ <= pad + w && py <= pad + h
            };
            let out = px
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(i, p)| p[3] > 0 && !inside(*i))
                .count();
            assert_eq!(out, 0, "{d:?} reaches out of its view box");
        }
    }
}
