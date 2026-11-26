// SPDX-License-Identifier: GPL-3.0-or-later

//! The tokens: every colour, size, stroke, radius and font of the window.
//!
//! This is the only module that may hold a colour or a size literal. A view
//! asks [`Theme`] or reads a constant here. A test searches the other source
//! files and fails if one of them holds a colour or a float literal.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    Visuals,
};
use std::sync::Arc;

/// What the screen draws this frame. It is no choice: [`crate::screen::shown`]
/// works it out from the theme, the adapter, reduce motion and high
/// legibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Nothing extra: no halo, no scanlines.
    Off,
    /// Still overlays: faint scanlines, a halo on lit things, the baked
    /// glow of the drawings. No screen pass. What a machine without a
    /// graphics adapter shows.
    Soft,
    /// The screen pass with the effects that do not move.
    Still,
    /// The screen pass with motion.
    Live,
}

impl Screen {
    /// Every form, from the plainest.
    pub const ALL: [Screen; 4] = [Screen::Off, Screen::Soft, Screen::Still, Screen::Live];

    /// The form's name, for pictures and test messages.
    pub fn name(self) -> &'static str {
        match self {
            Screen::Off => "Off",
            Screen::Soft => "Soft",
            Screen::Still => "Still",
            Screen::Live => "Live",
        }
    }

    /// The form draws through the screen pass.
    pub fn pass(self) -> bool {
        matches!(self, Screen::Still | Screen::Live)
    }

    /// The form moves: the scanlines drift.
    pub fn moving(self) -> bool {
        self == Screen::Live
    }
}

/// A theme: a name, a base colour and a screen. The palette comes from the
/// base colour by the rule of [`crate::palette`].
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    /// The name in the menu and on the status line.
    pub name: std::borrow::Cow<'static, str>,
    /// The colour the palette starts from.
    pub base: Color32,
    /// The ten strengths, as saved.
    pub strengths: Strengths,
    /// The screen moves. Off, the moving strengths draw at 0 and the
    /// scanlines stand still, but the theme keeps their numbers.
    pub moves: bool,
    /// Frames a second while the screen moves: one of [`crt::RATES`].
    pub rate: u32,
}

impl Spec {
    /// A theme that ships with Studio has this name, in any case.
    pub fn is_shipped(&self) -> bool {
        shipped(&self.name).is_some()
    }
}

const fn ships(name: &'static str, base: Color32, strengths: Strengths, moves: bool) -> Spec {
    Spec {
        name: std::borrow::Cow::Borrowed(name),
        base,
        strengths,
        moves,
        rate: crt::RATE,
    }
}

/// The six themes that ship with Studio, in menu order. Green is the
/// default.
pub static SHIPPED: [Spec; 6] = [
    ships("Green", GREEN.bright, crt::LIVE, true),
    ships("Amber", AMBER.bright, crt::LIVE, true),
    ships("Blue", BLUE.bright, crt::LIVE, true),
    ships("White", WHITE.bright, crt::LIVE, true),
    ships("Arcade", GREEN.bright, crt::ARCADE, true),
    ships("Flat", GREEN.bright, Strengths::NONE, false),
];

/// The place in [`SHIPPED`] of the theme named `name`, in any case.
pub fn shipped(name: &str) -> Option<usize> {
    SHIPPED
        .iter()
        .position(|t| crate::store::same_name(&t.name, name))
}

/// Said when high legibility holds the theme flat.
pub const LEGIBLE: &str = "High legibility is on.";
/// Said when reduce motion holds the theme still.
pub const REDUCED: &str = "The system asks for reduced motion.";
/// Said when the screen pass cannot run on this machine.
pub const NO_ADAPTER: &str = "This machine has no graphics adapter for the screen pass.";
/// The note in the menu while high legibility holds the theme.
pub const HELD_FLAT: &str = "Shown flat. High legibility is on.";
/// The note in the menu while reduce motion holds the theme.
pub const HELD_STILL: &str = "Shown still. Reduce motion is on.";
/// The note in the menu while the machine has no graphics adapter.
pub const HELD_SIMPLER: &str = "Shown simpler. No graphics adapter.";

/// The note for what holds the theme.
pub fn held_note(h: crate::screen::Held) -> &'static str {
    use crate::screen::Held;
    match h {
        Held::Legible => HELD_FLAT,
        Held::Still => HELD_STILL,
        Held::NoAdapter => HELD_SIMPLER,
    }
}

/// The status line for the theme named `name`, held back by `held` or
/// not: "Theme: Green." or "High legibility is on. Theme: Green, shown
/// flat.".
pub fn news(name: &str, held: Option<crate::screen::Held>) -> String {
    use crate::screen::Held;
    match held {
        None => format!("Theme: {name}."),
        Some(Held::Legible) => format!("{LEGIBLE} Theme: {name}, shown flat."),
        Some(Held::Still) => format!("{REDUCED} Theme: {name}, shown still."),
        Some(Held::NoAdapter) => format!("{NO_ADAPTER} Theme: {name}, shown without it."),
    }
}

/// The status line at the first change of the theme named `name`.
pub fn changed_news(name: &str) -> String {
    format!("Theme: {name}, changed. Not saved.")
}

/// The ten strengths of the screen pass, each from 0 to 1. Zero turns an
/// effect off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strengths {
    /// A glow around bright pixels.
    pub bloom: f32,
    /// Darker corners.
    pub vignette: f32,
    /// Dark lines across.
    pub scanlines: f32,
    /// The bulge of the glass.
    pub curvature: f32,
    /// The last frame fades instead of vanishing.
    pub persistence: f32,
    /// Grain that changes every frame.
    pub noise: f32,
    /// Brightness that wavers.
    pub flicker: f32,
    /// A bright band that rolls down the screen.
    pub band: f32,
    /// Red, green and blue columns of a colour tube.
    pub mask: f32,
    /// Red and blue pulled apart at the edges.
    pub aberration: f32,
}

impl Strengths {
    /// Every effect off.
    pub const NONE: Strengths = Strengths {
        bloom: 0.0,
        vignette: 0.0,
        scanlines: 0.0,
        curvature: 0.0,
        persistence: 0.0,
        noise: 0.0,
        flicker: 0.0,
        band: 0.0,
        mask: 0.0,
        aberration: 0.0,
    };

    /// The ten names, in the order of [`Strengths::get`].
    pub const NAMES: [&'static str; 10] = [
        "Bloom",
        "Vignette",
        "Scanlines",
        "Curvature",
        "Persistence",
        "Noise",
        "Flicker",
        "Rolling band",
        "Shadow mask",
        "Chromatic aberration",
    ];

    /// The places of the four strengths that move, in the order of
    /// [`Strengths::NAMES`]: Persistence, Noise, Flicker, Rolling band.
    pub const MOVING: [usize; 4] = [4, 5, 6, 7];
    /// The places of the two strengths that draw with no screen pass:
    /// Bloom by its halos and Scanlines by its quad.
    pub const WITHOUT_PASS: [usize; 2] = [0, 2];

    /// Some strength is past [`crt::STRONGER`].
    pub fn stronger(&self) -> bool {
        (0..Self::NAMES.len()).any(|i| self.get(i) > crt::STRONGER.get(i))
    }

    /// The strength at `i`, in the order of [`Strengths::NAMES`].
    pub fn get_mut(&mut self, i: usize) -> Option<&mut f32> {
        Some(match i {
            0 => &mut self.bloom,
            1 => &mut self.vignette,
            2 => &mut self.scanlines,
            3 => &mut self.curvature,
            4 => &mut self.persistence,
            5 => &mut self.noise,
            6 => &mut self.flicker,
            7 => &mut self.band,
            8 => &mut self.mask,
            9 => &mut self.aberration,
            _ => return None,
        })
    }

    /// The strength at `i`, in the order of [`Strengths::NAMES`].
    pub fn get(&self, i: usize) -> Option<f32> {
        let mut c = *self;
        c.get_mut(i).map(|v| *v)
    }

    /// The same with every effect that moves turned off: what an
    /// unfocused window holds, and what a theme that does not move draws.
    pub fn still(&self) -> Strengths {
        Strengths {
            persistence: 0.0,
            noise: 0.0,
            flicker: 0.0,
            band: 0.0,
            ..*self
        }
    }
}

/// The screen pass's numbers.
pub mod crt {
    use super::Strengths;

    /// The Live numbers: Green, Amber, Blue and White.
    pub const LIVE: Strengths = Strengths {
        bloom: 0.35,
        vignette: 0.50,
        scanlines: 0.30,
        curvature: 0.12,
        persistence: 0.50,
        noise: 0.10,
        flicker: 0.20,
        band: 0.30,
        mask: 0.0,
        aberration: 0.0,
    };
    /// The Arcade numbers: everything strong.
    pub const ARCADE: Strengths = Strengths {
        bloom: 0.60,
        vignette: 0.80,
        scanlines: 0.60,
        curvature: 0.40,
        persistence: 0.80,
        noise: 0.40,
        flicker: 0.50,
        band: 0.60,
        mask: 0.30,
        aberration: 0.20,
    };
    /// The frame rates a theme may have, a second.
    pub const RATES: [u32; 3] = [15, 30, 60];
    /// The longest step one frame adds to the screen's clock, in seconds:
    /// one frame at the slowest rate. A late frame, or the first one after
    /// the window was hidden, then moves the picture one frame on, not by
    /// the whole time away.
    pub const MAX_STEP: f32 = 1.0 / RATES[0] as f32;
    /// The frame rate of the themes that ship.
    pub const RATE: u32 = 30;
    /// Curvature 1 moves a corner of the glass by this part of the half
    /// width. Keep in step with `curve` in `screen.wgsl`.
    pub const CURVE: f32 = 0.12;
    /// What persistence 1 keeps of the last frame after one frame at
    /// [`PERSIST_RATE`].
    pub const PERSIST_KEEP: f32 = 0.82;
    /// The frame rate at which [`PERSIST_KEEP`] holds. At other rates the
    /// fade is scaled by the time between frames.
    pub const PERSIST_RATE: f32 = 30.0;
    /// The afterglow ends below this share of the old picture: less than
    /// one step of an 8-bit channel.
    pub const AFTERGLOW_END: f32 = 1.0 / 256.0;
    /// The time between frames that persistence assumes at most and at
    /// least, in seconds.
    pub const PERSIST_DT: [f32; 2] = [1.0 / 240.0, 0.25];
    /// The scanline period, in points.
    pub const SCAN_PERIOD: f32 = 3.0;
    /// How fast the scanlines drift down, in points a second.
    pub const SCAN_DRIFT: f32 = 6.0;
    /// The steps of the pointer's inverse mapping.
    pub const INVERSE_STEPS: usize = 50;
    /// A slider moves by this step.
    pub const SLIDER_STEP: f64 = 0.01;
    /// Past the themes that ship: a strength over its number here makes
    /// the editor say that text is harder to read. A strength the themes
    /// that ship leave at 0 counts from above 0; the 1s never count.
    pub const STRONGER: Strengths = Strengths {
        bloom: 1.0,
        vignette: 0.5,
        scanlines: 0.3,
        curvature: 0.12,
        persistence: 1.0,
        noise: 0.1,
        flicker: 1.0,
        band: 1.0,
        mask: 0.0,
        aberration: 0.0,
    };
}

/// The colour roles of one theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The window behind the panes.
    pub ground: Color32,
    /// The inside of a pane, a dialog, a field.
    pub pane: Color32,
    /// A hovered row, a banner, a toast.
    pub raised: Color32,
    /// A selected row, a toolbar action button.
    pub select: Color32,
    /// Pane and control strokes.
    pub frame: Color32,
    /// Dividers between rows.
    pub frame_dim: Color32,
    /// Secondary text, hints, line numbers, comments.
    pub dim: Color32,
    /// Body text and code.
    pub text: Color32,
    /// Labels, headings, active things, the fill of the primary button.
    pub bright: Color32,
    /// Text on a `bright` or `alarm` fill.
    pub ink: Color32,
    /// Errors, refusals, destructive actions.
    pub alarm: Color32,
    /// Warnings.
    pub warn: Color32,
    /// Strings and numbers in code.
    pub lit: Color32,
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The green palette, pinned to its base colour.
pub const GREEN: Palette = Palette {
    ground: hex(0x05140a),
    pane: hex(0x091d10),
    raised: hex(0x0f3019),
    select: hex(0x164524),
    frame: hex(0x40ba60),
    frame_dim: hex(0x216134),
    dim: hex(0x82d893),
    text: hex(0xa2f8ae),
    bright: hex(0x5aff7f),
    ink: hex(0x021a08),
    alarm: hex(0xff8a78),
    warn: hex(0xf5d86a),
    lit: hex(0xdcf08f),
};

/// The amber palette, pinned to its base colour.
pub const AMBER: Palette = Palette {
    ground: hex(0x140c03),
    pane: hex(0x1d1306),
    raised: hex(0x30200a),
    select: hex(0x46300f),
    frame: hex(0xcc8d2e),
    frame_dim: hex(0x6b4915),
    dim: hex(0xe0ad66),
    text: hex(0xffdb9e),
    bright: hex(0xffb93f),
    ink: hex(0x1f1100),
    alarm: hex(0xff8f98),
    warn: hex(0xfff2c4),
    lit: hex(0xf8ec70),
};

/// The blue palette, pinned to its base colour.
pub const BLUE: Palette = Palette {
    ground: hex(0x050f18),
    pane: hex(0x091a27),
    raised: hex(0x0f2a3e),
    select: hex(0x163d5a),
    frame: hex(0x4c9fd6),
    frame_dim: hex(0x24526f),
    dim: hex(0x8cc3e3),
    text: hex(0xb9e5fb),
    bright: hex(0x66c9ff),
    ink: hex(0x02131f),
    alarm: hex(0xff8a78),
    warn: hex(0xf5d86a),
    lit: hex(0xdcf08f),
};

/// The white palette, pinned to its base colour.
pub const WHITE: Palette = Palette {
    ground: hex(0x0b0e0e),
    pane: hex(0x131717),
    raised: hex(0x202626),
    select: hex(0x303939),
    frame: hex(0x96a4a4),
    frame_dim: hex(0x495252),
    dim: hex(0xbcc8c8),
    text: hex(0xe8f1f1),
    bright: hex(0xf4ffff),
    ink: hex(0x0b1010),
    alarm: hex(0xff8a78),
    warn: hex(0xf5d86a),
    lit: hex(0xdcf08f),
};

/// High legibility: neutral text and frames. `bright` is the green value of
/// the table; [`Theme::palette`] puts the theme's own `bright` in for every
/// other base colour.
pub const HIGH_LEGIBILITY: Palette = Palette {
    ground: hex(0x0c0e0d),
    pane: hex(0x131615),
    raised: hex(0x1e2321),
    select: hex(0x27382e),
    frame: hex(0x8a978f),
    frame_dim: hex(0x3c4540),
    dim: hex(0xb7c1ba),
    text: hex(0xf0f4f1),
    bright: hex(0x86ffa8),
    ink: hex(0x06140b),
    alarm: hex(0xff9384),
    warn: hex(0xffd56b),
    lit: hex(0xffd9a0),
};

/// Sizes in logical pixels.
pub mod size {
    /// The window at start.
    pub const WINDOW: [f32; 2] = [1280.0, 720.0];
    /// The spacing unit. Steps are 4, 8, 12, 16 and 24.
    pub const UNIT: f32 = 4.0;
    /// The widest a footer segment gets; longer text is cut.
    pub const SEGMENT_MAX: f32 = 180.0;
    /// The narrowest the footer message gets while segments remain.
    pub const MESSAGE_MIN: f32 = 160.0;
    /// Spacing steps.
    pub const STEP_6: f32 = 6.0;
    /// 12.
    pub const STEP_12: f32 = 12.0;
    /// 16.
    pub const STEP_16: f32 = 16.0;
    /// 24.
    pub const STEP_24: f32 = 24.0;
    /// 32.
    pub const STEP_32: f32 = 32.0;
    /// The unsaved-file dialog.
    pub const DIALOG_H: f32 = 108.0;
    /// Gap between panes, across.
    pub const GAP_ACROSS: f32 = 8.0;
    /// Gap between panes, down.
    pub const GAP_DOWN: f32 = 8.0;
    /// Pane padding at the sides.
    pub const PAD_SIDE: f32 = 10.0;
    /// Pane padding at the top.
    pub const PAD_TOP: f32 = 8.0;
    /// Pane padding at the bottom.
    pub const PAD_BOTTOM: f32 = 8.0;
    /// A control.
    pub const CONTROL_H: f32 = 28.0;
    /// A small control and an icon button.
    pub const SMALL_H: f32 = 24.0;
    /// A text field.
    pub const FIELD_H: f32 = 26.0;
    /// Rows.
    pub const ROW_TREE: f32 = 24.0;
    /// A row of a list or a menu.
    pub const ROW_LIST: f32 = 30.0;
    /// The menu row in the window.
    pub const MENU_H: f32 = 24.0;
    /// The toolbar.
    pub const TOOLBAR_H: f32 = 42.0;
    /// The header band, tall.
    pub const HEADER_H: f32 = 60.0;
    /// The header band, compact.
    pub const HEADER_COMPACT_H: f32 = 36.0;
    /// The header is tall from this inner window height up, in points.
    pub const HEADER_TALL_FROM: f32 = 720.0;
    /// The space before the atom and after the skyline in the header.
    pub const HEADER_PAD: f32 = 12.0;
    /// The space at each side of the header's divider.
    pub const HEADER_GAP: f32 = 14.0;
    /// The header's divider, tall.
    pub const HEADER_DIVIDER_H: f32 = 36.0;
    /// The header's divider, compact.
    pub const HEADER_DIVIDER_COMPACT_H: f32 = 20.0;
    /// The Studio wordmark in the tall header.
    pub const WORDMARK_H: f32 = 56.0;
    /// The Studio wordmark in the compact header.
    pub const WORDMARK_COMPACT_H: f32 = 32.0;
    /// The atom mark in the tall header, square.
    pub const ATOM: f32 = 52.0;
    /// The atom mark in the compact header, square.
    pub const ATOM_COMPACT: f32 = 24.0;
    /// The skyline at the right of the header.
    pub const SKYLINE: [f32; 2] = [392.0, 60.0];
    /// The middle of the skyline's own rule, in its file's units from its
    /// top. It lies on the header's rule.
    pub const SKYLINE_RULE_Y: f32 = 59.0;
    /// The one width of the four action buttons of the toolbar.
    pub const TOOLBAR_ACTION_W: f32 = 132.0;
    /// The space at each side of a divider in the toolbar.
    pub const TOOLBAR_DIVIDER_GAP: f32 = 16.0;
    /// A divider in the toolbar.
    pub const TOOLBAR_DIVIDER_H: f32 = 26.0;
    /// The footer.
    pub const FOOTER_H: f32 = 28.0;
    /// The Industries wordmark in the footer.
    pub const FOOTER_MARK_H: f32 = 22.0;
    /// The narrowest status line of the footer before it hides segments.
    pub const FOOTER_MESSAGE_MIN: f32 = 360.0;
    /// A divider in the footer.
    pub const FOOTER_DIVIDER_H: f32 = 16.0;
    /// A pane's title row with its 1 px rule.
    pub const PANE_HEAD_H: f32 = 27.0;
    /// A tab.
    pub const TAB_H: f32 = 24.0;
    /// A fact row of the Tape pane.
    pub const ROW_FACT: f32 = 18.0;
    /// The device drawing's box at zoom 1, without its glow.
    pub const DEVICE: [f32; 2] = [392.0, 336.0];
    /// How far the device texture reaches left of and above its box, at zoom 1.
    pub const DEVICE_BLEED: [f32; 2] = [12.0, 14.0];
    /// The top left of the screen opening in the device box, at zoom 1.
    pub const DEVICE_SCREEN_AT: [f32; 2] = [36.0, 30.0];
    /// The screen opening of the device at zoom 1: the machine's pixels.
    pub const DEVICE_SCREEN: [f32; 2] = [320.0, 240.0];
    /// The row of controls under the device.
    pub const SIM_ROW_W: f32 = 380.0;
    /// The tape drawing's box in the Tape pane, without its glow.
    pub const TAPE_ART: [f32; 2] = [120.0, 76.0];
    /// The label field of the tape drawing, in that box.
    pub const TAPE_LABEL: [f32; 2] = [81.0, 18.0];
    /// The top left of the tape's label field, in that box.
    pub const TAPE_LABEL_AT: [f32; 2] = [10.0, 42.0];
    /// The least room on each side of the longest project name on the
    /// tape's label field.
    pub const TAPE_LABEL_MARGIN: f32 = 4.0;
    /// The card drawing's box, without its glow.
    pub const CARD_ART: [f32; 2] = [120.0, 160.0];
    /// One icon.
    pub const ICON: f32 = 16.0;
    /// The application icon given to the window, in pixels.
    pub const APP_ICON: u32 = 256;
    /// The application icon shows its small form up to this size, in pixels.
    pub const APP_ICON_SMALL_MAX: u32 = 32;
    /// The editor gutter.
    pub const GUTTER_W: f32 = 52.0;
    /// Indent of one tree level.
    pub const TREE_INDENT: f32 = 14.0;
    /// The target dropdown in the toolbar.
    pub const TARGET_DROPDOWN_W: f32 = 192.0;
    /// A menu popup.
    pub const MENU_POPUP_W: f32 = 300.0;
    /// The check mark of a menu row, and the lock in its place.
    pub const MENU_MARK: f32 = 16.0;
    /// From a menu row's left edge to its label. The mark sits in this
    /// indent, which every row keeps, marked or not, so labels line up.
    pub const MENU_MARK_INDENT: f32 = 28.0;
    /// The find field.
    pub const FIND_W: f32 = 190.0;
    /// The narrowest find or replace field: below it, the find bar's
    /// buttons wrap to a further row.
    pub const FIND_MIN: f32 = 100.0;
    /// The log's filter field in the bottom pane's tab row.
    pub const LOG_FILTER_W: f32 = 150.0;
    /// A dialog.
    pub const DIALOG_W: f32 = 440.0;
    /// The width of an empty state's sentence.
    pub const EMPTY_TEXT_W: f32 = 360.0;
    /// Files pane, default and limits.
    pub const FILES_W: f32 = 216.0;
    /// Narrowest Files pane.
    pub const FILES_MIN: f32 = 180.0;
    /// Widest Files pane.
    pub const FILES_MAX: f32 = 360.0;
    /// Simulator pane, default and narrowest: it only grows.
    pub const SIM_W: f32 = 408.0;
    /// Tape pane.
    pub const TAPE_H: f32 = 118.0;
    /// Bottom pane, default.
    pub const BOTTOM_H: f32 = 152.0;
    /// Bottom pane, lowest. The highest is half the page.
    pub const BOTTOM_MIN: f32 = 120.0;
    /// A divider strip, and the step of its arrow keys.
    pub const DIVIDER: f32 = 8.0;
    /// The page margin at the top and the bottom.
    pub const PAGE_MARGIN_DOWN: f32 = 6.0;
    /// The page margin at the left.
    pub const PAGE_MARGIN_LEFT: f32 = 4.0;
    /// The page margin at the right.
    pub const PAGE_MARGIN_RIGHT: f32 = 8.0;
    /// The space between the header's marks, and between the skyline and
    /// the window's right edge.
    pub const HEADER_MARK_GAP: f32 = 8.0;
    /// The space between the two caps lines of the header.
    pub const HEADER_LINE_GAP: f32 = 3.0;
    /// The skyline hides when less than this would stay before it.
    pub const SKYLINE_ROOM: f32 = 24.0;
    /// A menu name's shape in the menu row.
    pub const MENU_ITEM_H: f32 = 22.0;
    /// The padding at each side of a menu name, a tab and a button.
    pub const LABEL_PAD: f32 = 10.0;
    /// The space between an icon and the label after it.
    pub const ICON_GAP: f32 = 7.0;
    /// The toolbar's four actions: their space from the edge and apart.
    pub const TOOLBAR_GAP: f32 = 8.0;
    /// Tabs start this far in from the pane's left edge.
    pub const TAB_INSET: f32 = 6.0;
    /// The space between two tabs.
    pub const TAB_GAP: f32 = 4.0;
    /// The padding of a footer segment.
    pub const FOOTER_SEG_PAD: f32 = 9.0;
    /// The middle part of the zoom group, which names the step.
    pub const ZOOM_NAME_W: f32 = 36.0;
    /// The space between the device and the honest line, and between the
    /// line and the row under it.
    pub const SIM_ROW_GAP: f32 = 4.0;
    /// The meter after the slot fact of the Tape pane.
    pub const METER_W: f32 = 56.0;
    /// The space between the tape drawing and its facts.
    pub const TAPE_FACTS_GAP: f32 = 16.0;
    /// A quick colour of the theme editor, high.
    pub const SWATCH: f32 = 24.0;
    /// The rail of a slider, high.
    pub const RAIL_H: f32 = 12.0;
    /// One slider's row: its name, its rail and its number box.
    pub const SLIDER_ROW: f32 = 28.0;
    /// A slider's number box, wide.
    pub const NUMBER_W: f32 = 52.0;
    /// The name column of a colour slider.
    pub const COLOUR_NAME_W: f32 = 84.0;
    /// The name column of a strength slider: "Chromatic aberration" fits.
    pub const STRENGTH_NAME_W: f32 = 168.0;
    /// The handle of a slider, wide.
    pub const HANDLE_W: f32 = 8.0;
    /// The box of a checkbox.
    pub const CHECK_BOX: f32 = 14.0;
}

/// Radii and strokes.
pub mod stroke {
    /// The radius of a control.
    pub const CONTROL_RADIUS: u8 = 3;
    /// The radius of a pane.
    pub const PANE_RADIUS: u8 = 4;
    /// A frame.
    pub const FRAME: f32 = 1.0;
    /// The focus ring, and its distance from the control.
    pub const FOCUS_RING: f32 = 2.0;
    /// The focus ring's distance from the control.
    pub const FOCUS_GAP: f32 = 2.0;
    /// The bar at the left edge of a selected row.
    pub const SELECT_BAR: f32 = 3.0;
    /// The editor caret.
    pub const CARET: f32 = 2.0;
    /// An icon's stroke on its 16 px grid.
    pub const ICON: f32 = 1.5;
    /// A dash of an unavailable control's frame, and the gap after it.
    pub const DASH: f32 = 4.0;
    /// The gap between dashes.
    pub const DASH_GAP: f32 = 3.0;
}

/// Type sizes.
pub mod text {
    /// One Spleen cell, wide.
    pub const LABEL_CELL_W: f32 = 8.0;
    /// One Spleen cell, high.
    pub const LABEL_CELL_H: f32 = 16.0;
    /// Body text.
    pub const BODY: f32 = 13.0;
    /// Body line height.
    pub const BODY_LINE: f32 = 18.0;
    /// Code.
    pub const CODE: f32 = 13.0;
    /// Code line height.
    pub const CODE_LINE: f32 = 19.0;
    /// Small text: logs, hints, the footer.
    pub const SMALL: f32 = 12.0;
    /// Tiny text: column heads and the line under the device.
    pub const TINY: f32 = 11.0;
    /// The space added after each cell of a tracked label, in font pixels.
    pub const LABEL_TRACKING: f32 = 2.0;
    /// Labels fall back to Plex Mono bold when the nearest whole Spleen
    /// scale is off by more than this part of the display scale.
    pub const LABEL_SCALE_TOLERANCE: f32 = 0.12;
}

/// The effect recipe.
pub mod effect {
    /// Scanline alpha at Soft, out of 1.
    pub const SCANLINE_SOFT: f32 = 0.07;
    /// Scanline period in device pixels.
    pub const SCANLINE_PERIOD: usize = 3;
    /// Halo blur.
    pub const HALO_BLUR: u8 = 12;
    /// Halo spread.
    pub const HALO_SPREAD: u8 = 0;
    /// Halo alpha at Soft, out of 1, of `bright`.
    pub const HALO_ALPHA: f32 = 0.38;
    /// The focused pane's halo, as a part of the halo alpha.
    pub const HALO_PANE: f32 = 0.7;
    /// A pressed filled button moves this far towards `ink`.
    pub const PRESS_DARKEN: f32 = 0.22;
    /// The label halo's tint at Soft, out of 1, of the label colour.
    pub const LABEL_HALO_SOFT: f32 = 0.30;
    /// The label halo's Gaussian, in font pixels.
    pub const LABEL_HALO_SIGMA: f32 = 1.5;
    /// The room around each glyph of the label halo atlas, in font pixels.
    pub const LABEL_HALO_PAD: usize = 4;
    /// The device with no machine running, out of 1, of `bright`.
    pub const DEVICE_IDLE: f32 = 0.62;
    /// A tape that is not built, out of 1, of `bright`.
    pub const TAPE_IDLE: f32 = 0.5;
    /// The scrim under a dialog, out of 1, black.
    pub const SCRIM: f32 = 0.62;
    /// The scrim under the theme editor: none, since the window behind is
    /// the preview. It still takes every click.
    pub const SCRIM_THEME: f32 = 0.0;
    /// A hovered filled button gets this much lighter.
    pub const HOVER_LIGHTEN: f32 = 0.12;
}

/// Font family names.
pub mod font {
    /// IBM Plex Mono 400: body, code, logs, fields.
    pub const REGULAR: &str = "plex-mono";
    /// IBM Plex Mono 700.
    pub const BOLD: &str = "plex-mono-bold";
    /// IBM Plex Mono italic 400.
    pub const ITALIC: &str = "plex-mono-italic";
}

/// The bundled font files.
mod files {
    pub const PLEX_REGULAR: &[u8] = include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf");
    pub const PLEX_BOLD: &[u8] = include_bytes!("../assets/fonts/IBMPlexMono-Bold.ttf");
    pub const PLEX_ITALIC: &[u8] = include_bytes!("../assets/fonts/IBMPlexMono-Italic.ttf");
}

/// The Spleen 8x16 bitmap font, as its BDF file.
pub const SPLEEN_8X16_BDF: &str = include_str!("../assets/fonts/spleen-8x16.bdf");

/// Text roles. A view asks for a role and gets a font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    /// Text in panes and dialogs, the tree, fields.
    Body,
    /// Body text in bold.
    BodyBold,
    /// The editor.
    Code,
    /// Code in bold.
    CodeBold,
    /// Code in italic.
    CodeItalic,
    /// Logs, hints, tags, the footer.
    Small,
    /// Column heads only.
    Tiny,
    /// The fallback for caps labels.
    LabelFallback,
}

/// What draws one frame: the theme's palette, high legibility and the
/// screen as [`crate::screen::shown`] worked it out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// The base colour of the theme.
    base: Color32,
    /// The palette of `base`, made once.
    own: Palette,
    /// Neutral text and frames, the screen Off.
    pub high_legibility: bool,
    /// What the screen draws, unless high legibility forces it Off.
    pub screen: Screen,
    /// The strengths drawn. At Soft, Bloom and Scanlines say whether the
    /// still overlays draw.
    pub strengths: Strengths,
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(GREEN.bright)
    }
}

impl Theme {
    /// A theme of base colour `base`, at Soft with the Live numbers.
    pub fn new(base: Color32) -> Self {
        Self {
            base,
            own: crate::palette::palette(base),
            high_legibility: false,
            screen: Screen::Soft,
            strengths: crt::LIVE,
        }
    }

    /// The base colour.
    pub fn base(&self) -> Color32 {
        self.base
    }

    /// Take the base colour `base`, and its palette.
    pub fn set_base(&mut self, base: Color32) {
        if base != self.base {
            *self = Self {
                base,
                own: crate::palette::palette(base),
                ..*self
            };
        }
    }

    /// The colours of this theme. High legibility takes the neutral
    /// palette with the theme's `bright`. When the rule moved the theme's
    /// `alarm` or paled its `warn`, it takes those too, so a red accent and
    /// a red alarm do not meet. The four pinned palettes keep their own
    /// high legibility: green as the table has it, the other three with
    /// their `bright` alone.
    pub fn palette(&self) -> Palette {
        let own = self.own;
        if !self.high_legibility {
            return own;
        }
        let mut p = HIGH_LEGIBILITY;
        if self.base == GREEN.bright {
            return p;
        }
        p.bright = own.bright;
        if crate::palette::PINNED.iter().any(|(b, _)| *b == self.base) {
            return p;
        }
        if own.alarm != crate::palette::ALARM_HOME {
            p.alarm = own.alarm;
        }
        if own.warn != crate::palette::WARN_HOME {
            p.warn = own.warn;
        }
        p
    }

    /// The screen drawn: high legibility forces Off.
    pub fn screen(&self) -> Screen {
        if self.high_legibility {
            Screen::Off
        } else {
            self.screen
        }
    }

    /// The kit draws its halos: rectangle halos, label halos and the
    /// drawings' baked halo groups. Only at Soft, with Bloom above 0: the
    /// screen pass's bloom does that work when it runs.
    pub fn halo_on(&self) -> bool {
        self.screen() == Screen::Soft && self.strengths.bloom > 0.0
    }

    /// The window draws its scanline quad. Only at Soft, with Scanlines
    /// above 0.
    pub fn scanlines_on(&self) -> bool {
        self.screen() == Screen::Soft && self.strengths.scanlines > 0.0
    }

    /// The font of a text role. High legibility raises body and code to 14.
    pub fn font(&self, t: Type) -> FontId {
        let up = if self.high_legibility { 1.0 } else { 0.0 };
        let (size, family) = match t {
            Type::Body => (text::BODY + up, font::REGULAR),
            Type::BodyBold => (text::BODY + up, font::BOLD),
            Type::Code => (text::CODE + up, font::REGULAR),
            Type::CodeBold => (text::CODE + up, font::BOLD),
            Type::CodeItalic => (text::CODE + up, font::ITALIC),
            Type::Small => (text::SMALL, font::REGULAR),
            Type::Tiny => (text::TINY, font::REGULAR),
            Type::LabelFallback => (text::BODY, font::BOLD),
        };
        FontId::new(size, FontFamily::Name(family.into()))
    }

    /// The line height of the editor.
    pub fn code_line(&self) -> f32 {
        text::CODE_LINE + if self.high_legibility { 1.0 } else { 0.0 }
    }

    /// A frame stroke in a colour.
    pub fn frame(&self, c: Color32) -> Stroke {
        Stroke::new(stroke::FRAME, c)
    }

    /// The radius of a control.
    pub fn control_radius(&self) -> CornerRadius {
        CornerRadius::same(stroke::CONTROL_RADIUS)
    }

    /// The radius of a pane.
    pub fn pane_radius(&self) -> CornerRadius {
        CornerRadius::same(stroke::PANE_RADIUS)
    }

    /// The radius of a tab: its two top corners.
    pub fn tab_radius(&self) -> CornerRadius {
        CornerRadius {
            nw: stroke::CONTROL_RADIUS,
            ne: stroke::CONTROL_RADIUS,
            sw: 0,
            se: 0,
        }
    }

    /// The halo under a lit thing, or None unless halos draw.
    pub fn halo(&self) -> Option<Shadow> {
        self.halo_on().then(|| Shadow {
            offset: [0, 0],
            blur: effect::HALO_BLUR,
            spread: effect::HALO_SPREAD,
            color: self.palette().bright.gamma_multiply(effect::HALO_ALPHA),
        })
    }

    /// The black of the dialog scrim.
    pub fn scrim(&self) -> Color32 {
        Color32::BLACK.gamma_multiply(effect::SCRIM)
    }

    /// The scrim under the theme editor, which draws nothing.
    pub fn scrim_theme(&self) -> Color32 {
        Color32::BLACK.gamma_multiply(effect::SCRIM_THEME)
    }

    /// A filled button's colour when hovered.
    pub fn hovered(&self, c: Color32) -> Color32 {
        lighten(c, effect::HOVER_LIGHTEN)
    }

    /// A filled button's colour when pressed: moved towards `ink`.
    pub fn pressed(&self, c: Color32) -> Color32 {
        mix(c, self.palette().ink, effect::PRESS_DARKEN)
    }

    /// The focused pane's halo: the halo at part of its strength.
    pub fn pane_halo(&self) -> Option<Shadow> {
        self.halo().map(|mut h| {
            h.color = self
                .palette()
                .bright
                .gamma_multiply(effect::HALO_ALPHA * effect::HALO_PANE);
            h
        })
    }

    /// The tint of a bright caps label's halo, out of 1, or None unless
    /// halos draw.
    pub fn label_halo(&self) -> Option<f32> {
        self.halo_on().then_some(effect::LABEL_HALO_SOFT)
    }

    /// The fill under an error row: 12 percent `alarm` over `pane`.
    pub fn error_row(&self) -> Color32 {
        let p = self.palette();
        mix(p.pane, p.alarm, effect::HOVER_LIGHTEN)
    }

    /// The scanline texture: `SCANLINE_PERIOD` rows of one device pixel,
    /// black with alpha in the first row. None unless the quad draws.
    pub fn scanline_image(&self) -> Option<egui::ColorImage> {
        if !self.scanlines_on() {
            return None;
        }
        let mut px = vec![Color32::TRANSPARENT; effect::SCANLINE_PERIOD];
        px[0] = Color32::BLACK.gamma_multiply(effect::SCANLINE_SOFT);
        Some(egui::ColorImage::new([1, effect::SCANLINE_PERIOD], px))
    }

    /// Put the fonts in the context.
    pub fn install_fonts(ctx: &egui::Context) {
        let mut defs = FontDefinitions::empty();
        for (name, bytes) in [
            (font::REGULAR, files::PLEX_REGULAR),
            (font::BOLD, files::PLEX_BOLD),
            (font::ITALIC, files::PLEX_ITALIC),
        ] {
            defs.font_data
                .insert(name.into(), Arc::new(FontData::from_static(bytes)));
            defs.families
                .insert(FontFamily::Name(name.into()), vec![name.into()]);
        }
        defs.families
            .insert(FontFamily::Monospace, vec![font::REGULAR.into()]);
        defs.families
            .insert(FontFamily::Proportional, vec![font::REGULAR.into()]);
        ctx.set_fonts(defs);
    }

    /// Set egui's own style from the tokens, for what egui draws itself:
    /// the text field, scroll bars, popups and tooltips.
    pub fn apply(&self, ctx: &egui::Context) {
        let p = self.palette();
        let me = *self;
        ctx.all_styles_mut(|s| {
            let mut v = Visuals::dark();
            v.override_text_color = Some(p.text);
            v.panel_fill = p.ground;
            v.window_fill = p.pane;
            v.extreme_bg_color = p.ground;
            v.faint_bg_color = p.raised;
            v.code_bg_color = p.pane;
            v.window_stroke = me.frame(p.frame);
            v.window_corner_radius = me.pane_radius();
            v.menu_corner_radius = me.control_radius();
            v.window_shadow = Shadow::NONE;
            v.popup_shadow = Shadow::NONE;
            v.selection.bg_fill = p.select;
            v.selection.stroke = me.frame(p.bright);
            v.hyperlink_color = p.bright;
            v.warn_fg_color = p.warn;
            v.error_fg_color = p.alarm;
            v.text_cursor.stroke = Stroke::new(stroke::CARET, p.bright);
            for (w, fill, line) in [
                (&mut v.widgets.noninteractive, p.pane, p.frame_dim),
                (&mut v.widgets.inactive, p.pane, p.frame),
                (&mut v.widgets.hovered, p.raised, p.frame),
                (&mut v.widgets.active, p.select, p.bright),
                (&mut v.widgets.open, p.select, p.frame),
            ] {
                w.bg_fill = fill;
                w.weak_bg_fill = fill;
                w.bg_stroke = me.frame(line);
                w.fg_stroke = Stroke::new(stroke::FRAME, p.text);
                w.corner_radius = me.control_radius();
                w.expansion = 0.0;
            }
            s.visuals = v;
            s.spacing.item_spacing = egui::vec2(size::UNIT * 2.0, size::UNIT);
            s.spacing.button_padding = egui::vec2(size::UNIT * 2.0, size::UNIT);
            s.spacing.interact_size.y = size::SMALL_H;
            s.text_styles = [
                (egui::TextStyle::Body, me.font(Type::Body)),
                (egui::TextStyle::Button, me.font(Type::Body)),
                (egui::TextStyle::Monospace, me.font(Type::Code)),
                (egui::TextStyle::Small, me.font(Type::Small)),
                (egui::TextStyle::Heading, me.font(Type::BodyBold)),
            ]
            .into();
        });
    }
}

/// `a` moved towards `b` by `t`, out of 1.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// `c` moved towards white by `t`, out of 1.
pub fn lighten(c: Color32, t: f32) -> Color32 {
    mix(c, Color32::WHITE, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color32) -> f64 {
        let f = |v: u8| {
            let s = f64::from(v) / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn ratio(a: Color32, b: Color32) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// Every palette holds the contrast pairs of the palette rule.
    #[test]
    fn contrast_pairs_hold() {
        for p in [GREEN, AMBER, BLUE, WHITE, HIGH_LEGIBILITY] {
            for pair in crate::palette::pairs(&p) {
                assert!(pair.ratio >= pair.limit, "{p:?}: {}", pair.name);
            }
            let pressed = mix(p.bright, p.ink, effect::PRESS_DARKEN);
            assert!(ratio(p.ink, pressed) >= 4.5);
        }
    }

    /// The pressed fill is the colour moved towards `ink`, and darker.
    #[test]
    fn pressed_moves_towards_ink() {
        let t = Theme::default();
        let p = t.palette();
        assert_eq!(
            t.pressed(p.bright),
            mix(p.bright, p.ink, effect::PRESS_DARKEN)
        );
        assert!(luminance(t.pressed(p.bright)) < luminance(p.bright));
    }

    #[test]
    fn halos_only_at_soft() {
        let on = Theme::default();
        assert!(on.pane_halo().is_some() && on.label_halo().is_some());
        assert!(on.scanline_image().is_some());
        for level in [Screen::Off, Screen::Still, Screen::Live] {
            let off = Theme {
                screen: level,
                ..on
            };
            assert!(
                off.pane_halo().is_none() && off.label_halo().is_none(),
                "{level:?}"
            );
            assert!(off.scanline_image().is_none(), "{level:?}");
        }
        let (h, f) = (on.halo().map(|s| s.color), on.pane_halo().map(|s| s.color));
        assert!(f.zip(h).is_some_and(|(f, h)| f.a() < h.a()));
        // Bloom at 0: no halo, the scanlines stay.
        let no_bloom = Theme {
            strengths: Strengths {
                bloom: 0.0,
                ..crt::LIVE
            },
            ..on
        };
        assert!(!no_bloom.halo_on() && no_bloom.scanlines_on());
        assert!(no_bloom.halo().is_none() && no_bloom.label_halo().is_none());
        assert!(no_bloom.scanline_image().is_some());
        // Scanlines at 0: no quad, the halos stay.
        let no_lines = Theme {
            strengths: Strengths {
                scanlines: 0.0,
                ..crt::LIVE
            },
            ..on
        };
        assert!(no_lines.halo_on() && !no_lines.scanlines_on());
        assert!(no_lines.scanline_image().is_none());
        assert!(no_lines.pane_halo().is_some());
    }

    /// The colour tables hold the values of the design, spot checked.
    #[test]
    fn palettes_hold_the_new_values() {
        assert_eq!(GREEN.ground, hex(0x05140a));
        assert_eq!(GREEN.bright, hex(0x5aff7f));
        assert_eq!(AMBER.frame, hex(0xcc8d2e));
        assert_eq!(BLUE.pane, hex(0x091a27));
        assert_eq!(WHITE.select, hex(0x303939));
        assert_eq!(HIGH_LEGIBILITY.frame, hex(0x8a978f));
    }

    #[test]
    fn high_legibility_turns_the_screen_off() {
        let t = Theme {
            high_legibility: true,
            screen: Screen::Live,
            ..Theme::default()
        };
        assert_eq!(t.screen(), Screen::Off);
        assert!(t.halo().is_none());
        assert!(t.scanline_image().is_none());
        assert_eq!(t.palette().text, HIGH_LEGIBILITY.text);
    }

    /// The six themes that ship hold their numbers.
    #[test]
    fn shipped_hold_their_numbers() {
        let names: Vec<_> = SHIPPED.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names, ["Green", "Amber", "Blue", "White", "Arcade", "Flat"]);
        let bases: Vec<_> = SHIPPED.iter().map(|t| t.base).collect();
        assert_eq!(
            bases,
            [
                hex(0x5aff7f),
                hex(0xffb93f),
                hex(0x66c9ff),
                hex(0xf4ffff),
                hex(0x5aff7f),
                hex(0x5aff7f)
            ]
        );
        let live = [0.35, 0.50, 0.30, 0.12, 0.50, 0.10, 0.20, 0.30, 0.0, 0.0];
        let arcade = [0.60, 0.80, 0.60, 0.40, 0.80, 0.40, 0.50, 0.60, 0.30, 0.20];
        for (t, want, moves) in [
            (&SHIPPED[0], live, true),
            (&SHIPPED[1], live, true),
            (&SHIPPED[2], live, true),
            (&SHIPPED[3], live, true),
            (&SHIPPED[4], arcade, true),
            (&SHIPPED[5], [0.0; 10], false),
        ] {
            for (i, w) in want.iter().enumerate() {
                assert_eq!(
                    t.strengths.get(i),
                    Some(*w),
                    "{} {}",
                    t.name,
                    Strengths::NAMES[i]
                );
            }
            assert_eq!(t.moves, moves, "{}", t.name);
            assert_eq!(t.rate, 30, "{}", t.name);
            assert!(t.is_shipped());
        }
        assert_eq!(Strengths::NONE.get(10), None);
        assert_eq!(shipped("arcade"), Some(4));
        assert_eq!(shipped("FLAT"), Some(5));
        assert_eq!(shipped("live"), None);
        assert_eq!(crt::LIVE.still().noise, 0.0);
        assert_eq!(crt::LIVE.still().bloom, crt::LIVE.bloom);
        assert!(Screen::Live.moving() && !Screen::Still.moving());
        assert!(!Screen::Soft.pass() && !Screen::Off.pass() && Screen::Still.pass());
    }

    #[test]
    fn amber_bright_in_high_legibility() {
        let mut t = Theme::new(AMBER.bright);
        t.high_legibility = true;
        assert_eq!(t.palette().bright, AMBER.bright);
        // The pinned four keep the neutral alarm and warning.
        assert_eq!(t.palette().alarm, HIGH_LEGIBILITY.alarm);
        assert_eq!(t.palette().warn, HIGH_LEGIBILITY.warn);
        let g = Theme {
            high_legibility: true,
            ..Theme::default()
        };
        assert_eq!(g.palette(), HIGH_LEGIBILITY);
    }

    /// A red base colour: the rule moved its alarm away from the red, and
    /// high legibility takes that alarm in place of its own red one.
    #[test]
    fn legible_red_theme_moves_the_alarm() {
        let red = hex(0xff3030);
        let own = crate::palette::palette(red);
        assert_ne!(own.alarm, crate::palette::ALARM_HOME, "the rule moved it");
        let mut t = Theme::new(red);
        t.high_legibility = true;
        let p = t.palette();
        assert_eq!(p.bright, own.bright);
        assert_eq!(p.alarm, own.alarm);
        assert_ne!(p.alarm, HIGH_LEGIBILITY.alarm);
        assert_eq!(p.text, HIGH_LEGIBILITY.text);
        // A base colour that leaves the alarm home keeps the neutral one.
        let cyan = Theme {
            high_legibility: true,
            ..Theme::new(hex(0x40e0ff))
        };
        assert_eq!(
            crate::palette::palette(hex(0x40e0ff)).alarm,
            crate::palette::ALARM_HOME
        );
        assert_eq!(cyan.palette().alarm, HIGH_LEGIBILITY.alarm);
    }

    /// The status lines and the notes, word for word.
    #[test]
    fn the_words_of_a_held_theme() {
        use crate::screen::Held;
        assert_eq!(news("Ember", None), "Theme: Ember.");
        assert_eq!(
            news("Ember", Some(Held::Legible)),
            "High legibility is on. Theme: Ember, shown flat."
        );
        assert_eq!(
            news("Ember", Some(Held::Still)),
            "The system asks for reduced motion. Theme: Ember, shown still."
        );
        assert_eq!(
            news("Ember", Some(Held::NoAdapter)),
            "This machine has no graphics adapter for the screen pass. Theme: Ember, shown without it."
        );
        assert_eq!(
            held_note(Held::Legible),
            "Shown flat. High legibility is on."
        );
        assert_eq!(held_note(Held::Still), "Shown still. Reduce motion is on.");
        assert_eq!(
            held_note(Held::NoAdapter),
            "Shown simpler. No graphics adapter."
        );
        assert_eq!(changed_news("Green"), "Theme: Green, changed. Not saved.");
    }
}
