// SPDX-License-Identifier: GPL-3.0-or-later

//! The palette rule: one base colour in, thirteen colours out.
//!
//! The base colour becomes `bright`, raised towards white when it is too
//! dark to read. The other window roles each have a luminance of their own,
//! so every contrast pair holds for any base colour. They take the hue of
//! `bright` and a share of its chroma. `alarm`, `warn` and `lit` have a home
//! colour each and move away from the base hue when it comes too close. The
//! four shipped base colours give their hand-made palettes exactly.
//!
//! All maths is in OKLCH, in 64-bit floats. Contrast is the WCAG 2.1 formula
//! on the 8-bit results. This is the second module that may hold a colour
//! literal, after [`crate::theme`]. It holds no egui type but [`Color32`].

use eframe::egui::Color32;

use crate::theme::{AMBER, BLUE, GREEN, Palette, WHITE};

/// Bisection steps for a lightness.
pub const L_STEPS: usize = 28;
/// Bisection steps for the gamut's edge.
pub const C_STEPS: usize = 24;
/// No sRGB colour has more chroma than this.
pub const C_TOP: f64 = 0.5;
/// Under this chroma an 8-bit colour is a grey.
pub const HUE_LOST: f64 = 1e-4;
/// How far outside 0 to 1 a linear channel may fall and still be in gamut.
const EPS: f64 = 1e-9;

/// The ratio `bright` is raised to, where 4.5 is needed.
pub const ROOM: f64 = 4.6;
/// The least lightness of `bright` for a base colour with no colour.
pub const GREY_L: f64 = 0.96;
/// How far that floor falls for a base colour with plenty of colour.
pub const GREY_DROP: f64 = 0.25;
/// The floor starts to fall at this saturation.
pub const SAT_FROM: f64 = 0.2;
/// It falls over this span of saturation.
pub const SAT_SPAN: f64 = 0.4;
/// Under this lightness a base colour's saturation counts for less.
pub const DARK_RAMP: f64 = 0.25;

/// The signal rules fade in from this chroma of `bright`.
pub const W_FROM: f64 = 0.02;
/// They fade in over this span of chroma.
pub const W_SPAN: f64 = 0.06;
/// The home colour of `alarm`.
pub const ALARM_HOME: Color32 = hex(0xff8a78);
/// Degrees `alarm` keeps from the base hue.
pub const ALARM_KEEP: f64 = 60.0;
/// The furthest `alarm` moves towards magenta, in degrees from home.
pub const ALARM_DOWN: f64 = -60.0;
/// The furthest `alarm` moves towards orange.
pub const ALARM_UP: f64 = 30.0;
/// A base hue under home by more than this sends `alarm` up.
pub const ALARM_FLIP: f64 = -15.0;
/// The home colour of `warn`.
pub const WARN_HOME: Color32 = hex(0xf5d86a);
/// Where `warn` goes when the base hue sits on it.
pub const WARN_PALE: Color32 = hex(0xfff2c4);
/// Degrees inside which `warn` starts to pale.
pub const WARN_NEAR: f64 = 45.0;
/// Degrees over which it gets there.
pub const WARN_FADE: f64 = 20.0;
/// The home colour of `lit`.
pub const LIT_HOME: Color32 = hex(0xdcf08f);
/// Degrees `lit` keeps from the base hue.
pub const LIT_KEEP: f64 = 28.0;

/// A role's relative luminance, chroma factor and gamut share.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    /// Relative luminance.
    pub y: f64,
    /// The share of the chroma of `bright` the role asks for.
    pub k: f64,
    /// The share of what sRGB can show that it may have.
    pub f: f64,
}

/// The nine window roles that follow `bright`, in the order the rule makes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The window behind the panes.
    Ground,
    /// The inside of a pane.
    Pane,
    /// A hovered row.
    Raised,
    /// A selected row.
    Select,
    /// Dividers between rows.
    FrameDim,
    /// Pane and control strokes.
    Frame,
    /// Secondary text.
    Dim,
    /// Body text.
    Text,
    /// Text on a `bright` fill.
    Ink,
}

const fn row(y: f64, k: f64, f: f64) -> Row {
    Row { y, k, f }
}

/// The nine rows of the rule.
pub const TABLE: [(Role, Row); 9] = [
    (Role::Ground, row(0.0047, 0.60, 0.60)),
    (Role::Pane, row(0.0086, 0.60, 0.70)),
    (Role::Raised, row(0.0200, 0.60, 0.70)),
    (Role::Select, row(0.0400, 0.60, 0.80)),
    (Role::FrameDim, row(0.0825, 0.60, 0.80)),
    (Role::Frame, row(0.3370, 0.90, 0.85)),
    (Role::Dim, row(0.5170, 0.70, 0.60)),
    (Role::Text, row(0.7880, 0.60, 1.00)),
    (Role::Ink, row(0.0061, 0.80, 0.95)),
];

const PANE: Row = TABLE[1].1;
const SELECT: Row = TABLE[3].1;
const INK: Row = TABLE[8].1;

/// The halo of a label at Soft, out of 1.
pub const HALO_SOFT: f64 = 0.21;
/// The halo of a label at its strongest, out of 1.
pub const HALO_FULL: f64 = 0.29;
/// A pressed fill, moved towards `ink`.
pub const PRESS: f64 = 0.22;
/// A hovered fill, moved towards white.
pub const HOVER: f64 = 0.12;
/// An error row, `pane` moved towards `alarm`.
pub const ERROR_ROW: f64 = 0.12;
/// A shaded corner of the screen, moved towards black.
pub const SHADE: f64 = 0.18;

/// The four base colours whose palettes are made by hand, and those palettes.
pub const PINNED: [(Color32, Palette); 4] = [
    (GREEN.bright, GREEN),
    (AMBER.bright, AMBER),
    (BLUE.bright, BLUE),
    (WHITE.bright, WHITE),
];

/// The twelve hue words and the degree each starts at. A hue under the
/// first start is the last word.
pub const HUE_WORDS: [(&str, f64); 12] = [
    ("red", 15.0),
    ("orange", 40.0),
    ("amber", 65.0),
    ("yellow", 90.0),
    ("lime", 112.0),
    ("green", 135.0),
    ("teal", 165.0),
    ("cyan", 195.0),
    ("blue", 225.0),
    ("violet", 280.0),
    ("magenta", 320.0),
    ("rose", 345.0),
];

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

// ------------------------------------------------------------ colour maths

/// One sRGB channel, 0 to 1, to linear light.
fn s2l(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// One linear channel, 0 to 1, to sRGB.
fn l2s(v: f64) -> f64 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn cbrt(x: f64) -> f64 {
    x.abs().powf(1.0 / 3.0).copysign(x)
}

fn lin_to_oklab(r: f64, g: f64, b: f64) -> [f64; 3] {
    let l = cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
    let m = cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
    let s = cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

fn oklab_to_lin(l: f64, a: f64, b: f64) -> [f64; 3] {
    let lo = (l + 0.3963377774 * a + 0.2158037573 * b).powf(3.0);
    let m = (l - 0.1055613458 * a - 0.0638541728 * b).powf(3.0);
    let s = (l - 0.0894841775 * a - 1.2914855480 * b).powf(3.0);
    [
        4.0767416621 * lo - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * lo + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * lo - 0.7034186147 * m + 1.7076147010 * s,
    ]
}

fn linear(c: Color32) -> [f64; 3] {
    [c.r(), c.g(), c.b()].map(|v| s2l(f64::from(v) / 255.0))
}

fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

/// Python's float modulo for a positive divisor.
fn modulo(x: f64, m: f64) -> f64 {
    x.rem_euclid(m)
}

/// An angle difference, in -180 to 180.
fn wrap(d: f64) -> f64 {
    modulo(d + 180.0, 360.0) - 180.0
}

/// An 8-bit colour as OKLCH lightness, chroma and hue in degrees. A chroma
/// under [`HUE_LOST`] is zero and has hue zero: a grey.
pub fn oklch(c: Color32) -> [f64; 3] {
    let [r, g, b] = linear(c);
    let [l, a, bb] = lin_to_oklab(r, g, b);
    let ch = a.hypot(bb);
    if ch < HUE_LOST {
        return [l, 0.0, 0.0];
    }
    [
        l,
        ch,
        modulo(bb.atan2(a) / (std::f64::consts::PI / 180.0), 360.0),
    ]
}

fn lch_to_lin(l: f64, c: f64, h: f64) -> [f64; 3] {
    if c <= 0.0 {
        // A grey, with no drift from the matrices.
        return [l.powf(3.0); 3];
    }
    let r = h * (std::f64::consts::PI / 180.0);
    oklab_to_lin(l, c * r.cos(), c * r.sin())
}

fn in_gamut(l: f64, c: f64, h: f64) -> bool {
    lch_to_lin(l, c, h)
        .iter()
        .all(|v| (-EPS..=1.0 + EPS).contains(v))
}

/// The most chroma sRGB can show at this lightness and hue.
pub fn cmax(l: f64, h: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, C_TOP);
    for _ in 0..C_STEPS {
        let mid = (lo + hi) / 2.0;
        if in_gamut(l, mid, h) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// An OKLCH colour as 8 bits, clipped to sRGB.
pub fn lch_to_rgb(l: f64, c: f64, h: f64) -> Color32 {
    let [r, g, b] = lch_to_lin(l, c, h).map(|v| (l2s(clamp01(v)) * 255.0 + 0.5) as u8);
    Color32::from_rgb(r, g, b)
}

fn lum_lch(l: f64, c: f64, h: f64) -> f64 {
    let [r, g, b] = lch_to_lin(l, c, h).map(clamp01);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// The WCAG 2.1 relative luminance of an 8-bit colour.
pub fn luminance(c: Color32) -> f64 {
    let [r, g, b] = linear(c);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// The WCAG 2.1 contrast ratio of two 8-bit colours.
pub fn ratio(a: Color32, b: Color32) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// `a` moved towards `b` by `t`, per 8-bit channel, rounded half up.
pub fn mix(a: Color32, b: Color32, t: f64) -> Color32 {
    let f = |x: u8, y: u8| {
        let (x, y) = (f64::from(x), f64::from(y));
        (x + (y - x) * t + 0.5) as u8
    };
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// The colour of hue `h` and luminance `y` whose chroma is `c_want`, cut to
/// `share` of what sRGB can show.
pub fn at_luminance(y: f64, c_want: f64, share: f64, h: f64) -> Color32 {
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..L_STEPS {
        let mid = (lo + hi) / 2.0;
        if lum_lch(mid, c_want.min(share * cmax(mid, h)), h) < y {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let l = (lo + hi) / 2.0;
    lch_to_rgb(l, c_want.min(share * cmax(l, h)), h)
}

// ------------------------------------------------------------------ the rule

/// Above zero when `rgb` can be `bright`. The least of four margins, each
/// zero on its limit: `bright` on `select`, `ink` on a pressed fill,
/// `bright` on its own halo at its strongest, all at [`ROOM`], and the
/// lightness a base colour of saturation `sat` must have.
pub fn stands(rgb: Color32, h0: f64, sat: f64) -> f64 {
    let [l, c, mut h] = oklch(rgb);
    if c <= 0.0 {
        h = h0;
    }
    let y = luminance(rgb);
    let need = |ground: f64| ROOM * (ground + 0.05) - 0.05;
    let pane = at_luminance(PANE.y, PANE.k * c, PANE.f, h);
    let margins = [
        y - need(SELECT.y),
        luminance(mix(rgb, Color32::BLACK, PRESS)) - need(INK.y),
        (y + 0.05) - ROOM * (luminance(mix(pane, rgb, HALO_FULL)) + 0.05),
        l - (GREY_L - GREY_DROP * clamp01((sat - SAT_FROM) / SAT_SPAN)),
    ];
    margins.into_iter().fold(f64::INFINITY, f64::min)
}

/// The base colour as `bright`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bright {
    /// The colour.
    pub rgb: Color32,
    /// Its OKLCH lightness.
    pub l: f64,
    /// Its OKLCH chroma.
    pub c: f64,
    /// Its hue in degrees, the base colour's hue for a grey.
    pub h: f64,
    /// Whether the rule raised it.
    pub raised: bool,
}

/// The base colour as `bright`: as it is when it stands, raised towards
/// white at the same hue until it does when not.
pub fn bright_of(base: Color32) -> Bright {
    let [l0, c0, h] = oklch(base);
    let top = cmax(l0, h);
    let sat = (if top > 0.0 { clamp01(c0 / top) } else { 0.0 }) * clamp01(l0 / DARK_RAMP);
    if stands(base, h, sat) >= 0.0 {
        return Bright {
            rgb: base,
            l: l0,
            c: c0,
            h,
            raised: false,
        };
    }
    // The base colour at lightness `l`: its own chroma, or its share of the
    // gamut where that is more, and never more than sRGB shows.
    let at = |l: f64| {
        let top = cmax(l, h);
        lch_to_rgb(l, top.min(c0.max(sat * top)), h)
    };
    let (mut lo, mut hi) = (l0, 1.0);
    for _ in 0..L_STEPS {
        let mid = (lo + hi) / 2.0;
        if stands(at(mid), h, sat) >= 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let rgb = at(hi);
    let [l, c, h2] = oklch(rgb);
    Bright {
        rgb,
        l,
        c,
        h: if c > 0.0 { h2 } else { h },
        raised: true,
    }
}

/// The least lightness a `bright` of hue `h` and saturation `sat` can have.
fn least_lightness(h: f64, sat: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..L_STEPS {
        let mid = (lo + hi) / 2.0;
        if stands(lch_to_rgb(mid, sat * cmax(mid, h), h), h, sat) >= 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// The colour control's three sliders to a base colour. `hue` in degrees,
/// `sat` and `light` from 0 to 1. Lightness 0 is the deepest colour that
/// stays readable, 1 is white.
pub fn from_sliders(hue: f64, sat: f64, light: f64) -> Color32 {
    let low = least_lightness(hue, sat);
    let l = low + (1.0 - low) * light;
    lch_to_rgb(l, sat * cmax(l, hue), hue)
}

/// A base colour to the three sliders, after it became `bright`: hue in
/// degrees, saturation and lightness out of 1.
pub fn to_sliders(base: Color32) -> [f64; 3] {
    let b = bright_of(base);
    let top = cmax(b.l, b.h);
    let sat = if top > 0.0 { clamp01(b.c / top) } else { 0.0 };
    let low = least_lightness(b.h, sat);
    let light = if low < 1.0 {
        clamp01((b.l - low) / (1.0 - low))
    } else {
        1.0
    };
    [b.h, sat, light]
}

/// The word for a hue in degrees.
pub fn hue_word(hue: f64) -> &'static str {
    let h = modulo(hue, 360.0);
    HUE_WORDS
        .iter()
        .rev()
        .find(|(_, start)| h >= *start)
        .map_or(HUE_WORDS[HUE_WORDS.len() - 1].0, |(w, _)| *w)
}

/// Where the three signals go for a `bright` of chroma `c` and hue `h`:
/// the hue of `alarm`, how pale `warn` goes from 0 to 1, and the hue of `lit`.
pub fn signal_hues(c: f64, h: f64) -> [f64; 3] {
    let w = clamp01((c - W_FROM) / W_SPAN);
    let ah = oklch(ALARM_HOME)[2];
    let (o, keep) = (wrap(h - ah), ALARM_KEEP * w);
    let off = if o.abs() >= keep {
        0.0
    } else if o >= ALARM_FLIP {
        (o - keep).max(ALARM_DOWN)
    } else {
        (o + keep).min(ALARM_UP)
    };
    let alarm_h = modulo(ah + off, 360.0);

    let wh = oklch(WARN_HOME)[2];
    let pale = w * clamp01((WARN_NEAR - wrap(h - wh).abs()) / WARN_FADE);

    let lh = oklch(LIT_HOME)[2];
    let (o, keep) = (wrap(h - lh), LIT_KEEP * w);
    // Always towards green, away from `warn`.
    let off = if o.abs() < keep { o + keep } else { 0.0 };
    [alarm_h, pale, modulo(lh + off, 360.0)]
}

/// The rule alone, with no pinned palette.
pub fn derive(base: Color32) -> Palette {
    let b = bright_of(base);
    let role = |r: Role| {
        let (_, row) = TABLE
            .iter()
            .find(|(x, _)| *x == r)
            .copied()
            .unwrap_or(TABLE[0]);
        at_luminance(row.y, row.k * b.c, row.f, b.h)
    };
    let [alarm_h, pale, lit_h] = signal_hues(b.c, b.h);
    let moved = |home: Color32, h: f64| {
        let [_, hc, hh] = oklch(home);
        if h == hh {
            home
        } else {
            at_luminance(luminance(home), hc, 1.0, h)
        }
    };
    let warn = if pale <= 0.0 {
        WARN_HOME
    } else {
        let (home, far) = (oklch(WARN_HOME), oklch(WARN_PALE));
        let y = luminance(WARN_HOME) + (luminance(WARN_PALE) - luminance(WARN_HOME)) * pale;
        let c = home[1] + (far[1] - home[1]) * pale;
        at_luminance(y, c, 1.0, home[2])
    };
    Palette {
        ground: role(Role::Ground),
        pane: role(Role::Pane),
        raised: role(Role::Raised),
        select: role(Role::Select),
        frame: role(Role::Frame),
        frame_dim: role(Role::FrameDim),
        dim: role(Role::Dim),
        text: role(Role::Text),
        bright: b.rgb,
        ink: role(Role::Ink),
        alarm: moved(ALARM_HOME, alarm_h),
        warn,
        lit: moved(LIT_HOME, lit_h),
    }
}

/// What the window draws with: the pinned palette for the four shipped base
/// colours, the rule for every other.
pub fn palette(base: Color32) -> Palette {
    PINNED
        .iter()
        .find(|(b, _)| *b == base)
        .map_or_else(|| derive(base), |(_, p)| *p)
}

// ------------------------------------------------------------ contrast pairs

/// One contrast pair: its name, its ratio and the least ratio it needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pair {
    /// What the pair is.
    pub name: &'static str,
    /// Its WCAG 2.1 ratio.
    pub ratio: f64,
    /// The least ratio it needs.
    pub limit: f64,
}

/// The 26 contrast pairs every palette must hold.
pub fn pairs(p: &Palette) -> [Pair; 26] {
    let (black, white) = (Color32::BLACK, Color32::WHITE);
    let g = |fg: Color32, bg: Color32, limit: f64, name: &'static str| Pair {
        name,
        ratio: ratio(fg, bg),
        limit,
    };
    [
        g(p.text, p.pane, 4.5, "text on pane"),
        g(p.text, p.raised, 4.5, "text on raised"),
        g(p.text, p.select, 4.5, "text on select"),
        g(p.dim, p.pane, 4.5, "dim on pane"),
        g(p.dim, p.ground, 4.5, "dim on ground"),
        g(p.dim, p.select, 4.5, "dim on select"),
        g(p.bright, p.pane, 4.5, "bright on pane"),
        g(p.bright, p.ground, 4.5, "bright on ground"),
        g(p.bright, p.select, 4.5, "bright on select"),
        g(
            p.bright,
            mix(p.pane, p.bright, HALO_SOFT),
            4.5,
            "bright on its soft halo",
        ),
        g(
            p.bright,
            mix(p.pane, p.bright, HALO_FULL),
            4.5,
            "bright on its full halo",
        ),
        g(p.ink, p.bright, 4.5, "ink on bright"),
        g(
            p.ink,
            mix(p.bright, p.ink, PRESS),
            4.5,
            "ink on a pressed fill",
        ),
        g(
            p.ink,
            mix(p.bright, white, HOVER),
            4.5,
            "ink on a hovered fill",
        ),
        g(p.alarm, p.pane, 4.5, "alarm on pane"),
        g(
            p.alarm,
            mix(p.pane, p.alarm, ERROR_ROW),
            4.5,
            "alarm on an error row",
        ),
        g(p.ink, p.alarm, 4.5, "ink on alarm"),
        g(p.warn, p.pane, 4.5, "warn on pane"),
        g(p.lit, p.pane, 4.5, "lit on pane"),
        g(p.frame, p.pane, 3.0, "frame on pane"),
        g(p.frame, p.ground, 3.0, "frame on ground"),
        g(p.frame, p.raised, 3.0, "frame on raised"),
        g(p.frame, p.select, 3.0, "frame on select"),
        g(p.bright, p.pane, 3.0, "bright fill on pane"),
        g(
            mix(p.dim, black, SHADE),
            mix(p.select, black, SHADE),
            4.5,
            "dim on select, shaded",
        ),
        g(
            mix(p.frame, black, SHADE),
            mix(p.select, black, SHADE),
            3.0,
            "frame on select, shaded",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::HIGH_LEGIBILITY;

    fn rows(p: &Palette) -> [Color32; 13] {
        [
            p.ground,
            p.pane,
            p.raised,
            p.select,
            p.frame,
            p.frame_dim,
            p.dim,
            p.text,
            p.bright,
            p.ink,
            p.alarm,
            p.warn,
            p.lit,
        ]
    }

    fn parse(text: &str) -> Color32 {
        let v = u32::from_str_radix(text.trim_start_matches('#'), 16).unwrap_or(u32::MAX);
        assert!(v <= 0xff_ffff, "{text}");
        hex(v)
    }

    /// Reference values: a base colour, then its thirteen roles through the
    /// rule alone, in the order ground, pane, raised, select, frame,
    /// frame-dim, dim, text, bright, ink, alarm, warn, lit.
    const VECTORS: [&str; 12] = [
        "5aff7f  061207 081b0b 102c15 14411d 3bb356 205d2d 78d286 a3f9af 5aff7f 011704 ff8a78 f5d86a dcf08f",
        "ffb93f  140e05 1f1507 32240f 4a3512 ca9337 694c1e e4b772 ffe2b7 ffb93f 1b1001 ff8890 fff2c4 dcf08f",
        "66c9ff  061016 081923 102937 143c51 4fa6d5 215672 90c5e5 c9ebff 66c9ff 01141f ff8a78 f5d86a dcf08f",
        "f4ffff  0b1010 131818 232828 343939 969f9f 4d5252 b9c0c0 e0e7e7 f4ffff 0d1313 ff8a78 f5d86a dcf08f",
        "ff0000  1f0805 2f0b07 491510 5e2821 e28375 7a4139 e8b2a9 ffdfd9 ff9383 2e0101 e48ddd f5d86a dcf08f",
        "0000ff  040d28 061538 15254a 25385e 7b9de1 3d517a abbfe6 dae6ff 8bb1ff 010c3e ff8a78 f5d86a dcf08f",
        "ffff00  0f0f05 181807 28280f 3a3a12 a1a236 53541d c1c372 e9ec84 ffff00 131301 ff8a78 fff2c4 baf7a5",
        "808080  0f0f0f 171717 272727 383838 9d9d9d 515151 bebebe e6e6e6 f2f2f2 121212 ff8a78 f5d86a dcf08f",
        "000000  0f0f0f 171717 272727 383838 9d9d9d 515151 bebebe e6e6e6 f2f2f2 121212 ff8a78 f5d86a dcf08f",
        "200000  1f0805 2b0e0a 3d1e19 51302b ca8e85 6c4843 e4b3ab ffdfd9 e4a095 2b0402 e48ddd f5d86a dcf08f",
        "ff7a1a  190c05 261207 3c1f0f 582d12 e08754 794425 e8b398 ffdfcf ff995f 230b01 f488bf f5d86a dcf08f",
        "ff5fa2  1e0710 2e0a19 471429 5d263b e17fa2 793f54 e8afc1 ffdde7 ff90b8 2c0114 f29546 f5d86a dcf08f",
    ];

    /// Reference values: the hand-made palettes, in the same order.
    const PINNED_ROWS: [&str; 4] = [
        "5aff7f  05140a 091d10 0f3019 164524 40ba60 216134 82d893 a2f8ae 5aff7f 021a08 ff8a78 f5d86a dcf08f",
        "ffb93f  140c03 1d1306 30200a 46300f cc8d2e 6b4915 e0ad66 ffdb9e ffb93f 1f1100 ff8f98 fff2c4 f8ec70",
        "66c9ff  050f18 091a27 0f2a3e 163d5a 4c9fd6 24526f 8cc3e3 b9e5fb 66c9ff 02131f ff8a78 f5d86a dcf08f",
        "f4ffff  0b0e0e 131717 202626 303939 96a4a4 495252 bcc8c8 e8f1f1 f4ffff 0b1010 ff8a78 f5d86a dcf08f",
    ];

    fn split(row: &str) -> (Color32, Vec<Color32>) {
        let mut it = row.split_whitespace().map(parse);
        let base = it.next().unwrap_or(Color32::PLACEHOLDER);
        (base, it.collect())
    }

    #[test]
    fn rule_matches_the_vectors() {
        for row in VECTORS {
            let (base, want) = split(row);
            let got = rows(&derive(base));
            assert_eq!(want.len(), 13);
            for (i, (g, w)) in got.iter().zip(&want).enumerate() {
                let off = [
                    g.r().abs_diff(w.r()),
                    g.g().abs_diff(w.g()),
                    g.b().abs_diff(w.b()),
                ];
                assert!(
                    off.iter().all(|d| *d <= 1),
                    "base {base:?}, role {i}: {g:?} where {w:?} is due"
                );
            }
        }
    }

    #[test]
    fn pinned_palettes_are_exact() {
        for (row, (base, p)) in PINNED_ROWS.iter().zip(PINNED) {
            let (b, want) = split(row);
            assert_eq!(b, base);
            assert_eq!(rows(&palette(base)).to_vec(), want);
            assert_eq!(palette(base), p);
        }
        assert_eq!(palette(parse("5aff7e")), derive(parse("5aff7e")));
    }

    #[test]
    fn cmax_lines() {
        for (l, h, want) in [
            (0.50, 30.0, 0.200529),
            (0.80, 148.0, 0.231548),
            (0.90, 264.0, 0.048190),
            (0.20, 110.0, 0.043663),
        ] {
            assert!((cmax(l, h) - want).abs() < 1e-6, "cmax({l}, {h})");
        }
    }

    #[test]
    fn oklch_lines() {
        for (c, want, y) in [
            ("5aff7f", [0.886391, 0.218878, 147.6137], 0.752260),
            ("0000ff", [0.452014, 0.313214, 264.0520], 0.072200),
            ("808080", [0.599871, 0.0, 0.0], 0.215861),
        ] {
            let got = oklch(parse(c));
            assert!((got[0] - want[0]).abs() < 1e-6, "{c} L");
            assert!((got[1] - want[1]).abs() < 1e-6, "{c} C");
            // The reference prints the hue to four places.
            assert!((got[2] - want[2]).abs() < 1e-4, "{c} H");
            assert!((luminance(parse(c)) - y).abs() < 1e-6, "{c} Y");
        }
    }

    #[test]
    fn from_sliders_table() {
        for (h, s, l, want) in [
            (148.0, 1.0, 0.0, "00cc52"),
            (148.0, 1.0, 0.5, "00fd68"),
            (30.0, 1.0, 0.0, "ff9382"),
            (264.0, 1.0, 0.0, "8ab1ff"),
            (264.0, 0.5, 0.25, "b9c8e4"),
            (77.0, 0.3, 0.0, "e8dcca"),
            (0.0, 0.0, 0.0, "f2f2f2"),
            (200.0, 1.0, 1.0, "ffffff"),
        ] {
            assert_eq!(from_sliders(h, s, l), parse(want), "{h} {s} {l}");
        }
    }

    #[test]
    fn to_sliders_table() {
        for (base, want) in [
            ("5aff7f", [147.61, 1.000, 0.562]),
            ("ffb93f", [76.98, 1.000, 0.276]),
            ("66c9ff", [234.93, 1.000, 0.165]),
            ("f4ffff", [196.97, 1.000, 0.968]),
            ("ff0000", [29.37, 1.000, 0.004]),
            ("808080", [0.00, 0.000, 0.037]),
        ] {
            let got = to_sliders(parse(base));
            assert!((got[0] - want[0]).abs() <= 0.005, "{base} hue {}", got[0]);
            assert!((got[1] - want[1]).abs() <= 0.0005, "{base} sat {}", got[1]);
            assert!(
                (got[2] - want[2]).abs() <= 0.0005,
                "{base} light {}",
                got[2]
            );
        }
    }

    #[test]
    fn hue_words_start_where_they_should() {
        for (h, w) in [
            (0.0, "rose"),
            (14.9, "rose"),
            (15.0, "red"),
            (49.0, "orange"),
            (112.0, "lime"),
            (344.9, "magenta"),
            (345.0, "rose"),
            (359.9, "rose"),
            (360.0, "rose"),
        ] {
            assert_eq!(hue_word(h), w, "{h}");
        }
    }

    /// High legibility on top of a theme: the neutral palette with the
    /// theme's `bright`, and its `alarm` and `warn` where the rule moved them.
    fn legible(own: &Palette) -> Palette {
        let mut p = HIGH_LEGIBILITY;
        p.bright = own.bright;
        if own.alarm != ALARM_HOME {
            p.alarm = own.alarm;
        }
        if own.warn != WARN_HOME {
            p.warn = own.warn;
        }
        p
    }

    /// Every base colour of the sweep: 36 hues by three lightness levels by
    /// three chroma shares, the greys, a 9 by 9 by 9 cube, and seven colours
    /// next to black, white and grey.
    fn sweep() -> Vec<Color32> {
        let mut out = Vec::new();
        for h in (0..360).step_by(10) {
            for l in [0.35, 0.65, 0.90] {
                for share in [0.2, 0.5, 1.0] {
                    let h = f64::from(h);
                    out.push(lch_to_rgb(l, share * cmax(l, h), h));
                }
            }
        }
        for v in (0..=255).step_by(15) {
            out.push(Color32::from_gray(v));
        }
        let steps = [0, 32, 64, 96, 128, 160, 192, 224, 255];
        for r in steps {
            for g in steps {
                for b in steps {
                    out.push(Color32::from_rgb(r, g, b));
                }
            }
        }
        for c in [
            "010000", "000001", "000100", "fffffe", "feffff", "817f7f", "7f8181",
        ] {
            out.push(parse(c));
        }
        out
    }

    #[test]
    fn every_theme_passes() {
        let bases = sweep();
        assert_eq!(bases.len(), 1078);
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let chunk = bases.len().div_ceil(threads);
        let failures: Vec<String> = std::thread::scope(|s| {
            let jobs: Vec<_> = bases
                .chunks(chunk)
                .map(|part| {
                    s.spawn(move || {
                        let mut bad = Vec::new();
                        for &base in part {
                            let p = derive(base);
                            for (how, q) in [("plain", p), ("legible", legible(&p))] {
                                for pair in pairs(&q) {
                                    if pair.ratio < pair.limit {
                                        bad.push(format!(
                                            "{base:?} {how}: {} {:.3} under {}",
                                            pair.name, pair.ratio, pair.limit
                                        ));
                                    }
                                }
                            }
                        }
                        bad
                    })
                })
                .collect();
            jobs.into_iter()
                .flat_map(|j| j.join().unwrap_or_else(|_| vec!["a thread failed".into()]))
                .collect()
        });
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
