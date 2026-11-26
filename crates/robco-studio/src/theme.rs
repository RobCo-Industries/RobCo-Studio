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

/// The phosphor hue of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Green phosphor, the default.
    Green,
    /// Amber phosphor.
    Amber,
    /// Blue phosphor.
    Blue,
    /// White phosphor.
    White,
}

impl Scheme {
    /// Every scheme, in menu order.
    pub const ALL: [Scheme; 4] = [Scheme::Green, Scheme::Amber, Scheme::Blue, Scheme::White];

    /// The scheme's name in the menu.
    pub fn name(self) -> &'static str {
        match self {
            Scheme::Green => "Green",
            Scheme::Amber => "Amber",
            Scheme::Blue => "Blue",
            Scheme::White => "White",
        }
    }
}

/// How much of the screen look is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effects {
    /// No scanlines and no halo.
    Off,
    /// Faint scanlines and a halo on lit things. The default.
    Soft,
}

/// The colour roles of one scheme.
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

/// The green scheme.
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

/// The amber scheme.
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

/// The blue scheme.
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

/// The white scheme.
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
/// the table; [`Theme::palette`] puts the scheme's own `bright` in for the
/// other schemes.
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
    /// The centre of a rail item's icon, from its top.
    pub const RAIL_ICON_Y: f32 = 15.0;
    /// The centre of a rail item's label, from its top.
    pub const RAIL_LABEL_Y: f32 = 33.0;
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
    /// The space before the wordmark and after the skyline in the header.
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
    pub const ATOM: f32 = 44.0;
    /// The atom mark in the compact header, square.
    pub const ATOM_COMPACT: f32 = 24.0;
    /// The skyline at the right of the header.
    pub const SKYLINE: [f32; 2] = [420.0, 60.0];
    /// One of the four action buttons of the toolbar.
    pub const TOOLBAR_ACTION_W: f32 = 148.0;
    /// A divider in the toolbar.
    pub const TOOLBAR_DIVIDER_H: f32 = 26.0;
    /// The rail.
    pub const RAIL_W: f32 = 68.0;
    /// A page button of the rail.
    pub const RAIL_ITEM_H: f32 = 46.0;
    /// A page button of the rail, across.
    pub const RAIL_ITEM_W: f32 = 60.0;
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
    /// The row of controls under the device.
    pub const SIM_ROW_W: f32 = 380.0;
    /// The tape drawing's box in the Tape pane, without its glow.
    pub const TAPE_ART: [f32; 2] = [120.0, 76.0];
    /// The label field of the tape drawing, in that box.
    pub const TAPE_LABEL: [f32; 2] = [81.0, 18.0];
    /// The top left of the tape's label field, in that box.
    pub const TAPE_LABEL_AT: [f32; 2] = [10.0, 42.0];
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
    /// The project dropdown in the toolbar.
    pub const PROJECT_DROPDOWN_W: f32 = 190.0;
    /// The target dropdown in the toolbar.
    pub const TARGET_DROPDOWN_W: f32 = 200.0;
    /// A menu popup.
    pub const MENU_POPUP_W: f32 = 300.0;
    /// The find field.
    pub const FIND_W: f32 = 190.0;
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
    /// The page margin, across: half a gap.
    pub const PAGE_MARGIN_ACROSS: f32 = GAP_ACROSS / 2.0;
    /// The page margin at the top and the bottom.
    pub const PAGE_MARGIN_DOWN: f32 = 6.0;
    /// The page margin after the rail.
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
    /// The rail's page buttons, from the rail's sides.
    pub const RAIL_INSET: f32 = 4.0;
    /// The space between two page buttons of the rail.
    pub const RAIL_GAP: f32 = 6.0;
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
    /// Small text: logs, hints, the status bar.
    pub const SMALL: f32 = 12.0;
    /// Tiny text: column heads only.
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
    /// Halo alpha at Full, out of 1, of `bright`.
    pub const HALO_ALPHA_FULL: f32 = 0.55;
    /// The focused pane's halo, as a part of the halo alpha.
    pub const HALO_PANE: f32 = 0.7;
    /// A pressed filled button moves this far towards `ink`.
    pub const PRESS_DARKEN: f32 = 0.22;
    /// The label halo's tint at Soft, out of 1, of the label colour.
    pub const LABEL_HALO_SOFT: f32 = 0.30;
    /// The label halo's tint at Full.
    pub const LABEL_HALO_FULL: f32 = 0.40;
    /// The label halo's Gaussian, in font pixels.
    pub const LABEL_HALO_SIGMA: f32 = 1.5;
    /// The room around each glyph of the label halo atlas, in font pixels.
    pub const LABEL_HALO_PAD: usize = 4;
    /// Corner shading at Full, at the far corner, out of 1, black.
    pub const CORNER_SHADE_MAX: f32 = 0.18;
    /// The device with no machine running, out of 1, of `bright`.
    pub const DEVICE_IDLE: f32 = 0.62;
    /// A tape that is not built, out of 1, of `bright`.
    pub const TAPE_IDLE: f32 = 0.5;
    /// The scrim under a dialog, out of 1, black.
    pub const SCRIM: f32 = 0.62;
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
    /// Logs, hints, tags, the status bar.
    Small,
    /// Column heads only.
    Tiny,
    /// The fallback for caps labels.
    LabelFallback,
}

/// The scheme, the mode and the effects of the running window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// The phosphor.
    pub scheme: Scheme,
    /// Neutral text and frames, effects off.
    pub high_legibility: bool,
    /// Effects, unless high legibility forces them off.
    pub effects: Effects,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            scheme: Scheme::Green,
            high_legibility: false,
            effects: Effects::Soft,
        }
    }
}

impl Theme {
    /// The colours of this theme.
    pub fn palette(&self) -> Palette {
        let own = match self.scheme {
            Scheme::Green => GREEN,
            Scheme::Amber => AMBER,
            Scheme::Blue => BLUE,
            Scheme::White => WHITE,
        };
        if !self.high_legibility {
            return own;
        }
        let mut p = HIGH_LEGIBILITY;
        if self.scheme != Scheme::Green {
            p.bright = own.bright;
        }
        p
    }

    /// The effects actually drawn: high legibility forces Off.
    pub fn effects(&self) -> Effects {
        if self.high_legibility {
            Effects::Off
        } else {
            self.effects
        }
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

    /// The halo under a lit thing, or None with effects off.
    pub fn halo(&self) -> Option<Shadow> {
        (self.effects() == Effects::Soft).then(|| Shadow {
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

    /// The tint of a bright caps label's halo, out of 1, or None with
    /// effects off.
    pub fn label_halo(&self) -> Option<f32> {
        (self.effects() == Effects::Soft).then_some(effect::LABEL_HALO_SOFT)
    }

    /// The fill under an error row: 12 percent `alarm` over `pane`.
    pub fn error_row(&self) -> Color32 {
        let p = self.palette();
        mix(p.pane, p.alarm, effect::HOVER_LIGHTEN)
    }

    /// The scanline texture: `SCANLINE_PERIOD` rows of one device pixel,
    /// black with alpha in the first row. None with effects off.
    pub fn scanline_image(&self) -> Option<egui::ColorImage> {
        if self.effects() == Effects::Off {
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

    #[test]
    fn contrast_pairs_hold() {
        for p in [GREEN, AMBER, BLUE, WHITE, HIGH_LEGIBILITY] {
            assert!(ratio(p.text, p.pane) >= 4.5);
            assert!(ratio(p.dim, p.select) >= 4.5);
            assert!(ratio(p.bright, p.select) >= 4.5);
            assert!(ratio(p.ink, p.bright) >= 4.5);
            assert!(ratio(p.frame, p.pane) >= 3.0);
            assert!(ratio(p.frame, p.select) >= 3.0);
            assert!(ratio(p.text, p.raised) >= 4.5);
            assert!(ratio(p.frame, p.raised) >= 3.0);
            let pressed = mix(p.bright, p.ink, effect::PRESS_DARKEN);
            assert!(ratio(p.ink, pressed) >= 4.5);
            assert!(ratio(p.bright, mix(p.pane, p.bright, 0.29)) >= 4.5);
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
    fn halos_go_with_effects_off() {
        let on = Theme::default();
        let off = Theme {
            effects: Effects::Off,
            ..on
        };
        assert!(on.pane_halo().is_some() && on.label_halo().is_some());
        assert!(off.pane_halo().is_none() && off.label_halo().is_none());
        let (h, f) = (on.halo().map(|s| s.color), on.pane_halo().map(|s| s.color));
        assert!(f.zip(h).is_some_and(|(f, h)| f.a() < h.a()));
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
    fn high_legibility_turns_effects_off() {
        let t = Theme {
            high_legibility: true,
            ..Theme::default()
        };
        assert_eq!(t.effects(), Effects::Off);
        assert!(t.halo().is_none());
        assert!(t.scanline_image().is_none());
        assert_eq!(t.palette().text, HIGH_LEGIBILITY.text);
    }

    #[test]
    fn amber_bright_in_high_legibility() {
        let t = Theme {
            scheme: Scheme::Amber,
            high_legibility: true,
            ..Theme::default()
        };
        assert_eq!(t.palette().bright, AMBER.bright);
    }
}
