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
    ground: hex(0x040906),
    pane: hex(0x07110b),
    raised: hex(0x0d1f14),
    select: hex(0x143a22),
    frame: hex(0x2f9150),
    frame_dim: hex(0x1b4a2b),
    dim: hex(0x74c08c),
    text: hex(0xabedbd),
    bright: hex(0x5cff8f),
    ink: hex(0x03170a),
    alarm: hex(0xff8170),
    warn: hex(0xf0d264),
    lit: hex(0xd6e98f),
};

/// The amber scheme.
pub const AMBER: Palette = Palette {
    ground: hex(0x0a0703),
    pane: hex(0x120c05),
    raised: hex(0x22160a),
    select: hex(0x40290d),
    frame: hex(0xa87324),
    frame_dim: hex(0x573a12),
    dim: hex(0xcf9f5a),
    text: hex(0xffd9a3),
    bright: hex(0xffb23e),
    ink: hex(0x1c0f00),
    alarm: hex(0xff8691),
    warn: hex(0xfff0c0),
    lit: hex(0xf5e96b),
};

/// The blue scheme.
pub const BLUE: Palette = Palette {
    ground: hex(0x04070a),
    pane: hex(0x071018),
    raised: hex(0x0d1d2b),
    select: hex(0x143650),
    frame: hex(0x3b86b8),
    frame_dim: hex(0x1c435e),
    dim: hex(0x7fb6d6),
    text: hex(0xb5e0f7),
    bright: hex(0x6cc9ff),
    ink: hex(0x03121c),
    alarm: hex(0xff8170),
    warn: hex(0xf0d264),
    lit: hex(0xd6e98f),
};

/// The white scheme.
pub const WHITE: Palette = Palette {
    ground: hex(0x070808),
    pane: hex(0x0e1010),
    raised: hex(0x1a1d1d),
    select: hex(0x2c3333),
    frame: hex(0x7f8b8b),
    frame_dim: hex(0x3b4242),
    dim: hex(0xb0bcbc),
    text: hex(0xe2ecec),
    bright: hex(0xf4ffff),
    ink: hex(0x0b1010),
    alarm: hex(0xff8170),
    warn: hex(0xf0d264),
    lit: hex(0xd6e98f),
};

/// High legibility: neutral text and frames. `bright` is the green value of
/// the table; [`Theme::palette`] puts the scheme's own `bright` in for the
/// other schemes.
pub const HIGH_LEGIBILITY: Palette = Palette {
    ground: hex(0x0c0e0d),
    pane: hex(0x131615),
    raised: hex(0x1e2321),
    select: hex(0x27382e),
    frame: hex(0x828f87),
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
    /// The widest a status bar segment gets; longer text is cut.
    pub const SEGMENT_MAX: f32 = 180.0;
    /// The narrowest the status message gets while segments remain.
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
    /// The centre of a rail item's icon, from its top.
    pub const RAIL_ICON_Y: f32 = 16.0;
    /// The centre of a rail item's label, from its top.
    pub const RAIL_LABEL_Y: f32 = 36.0;
    /// The unsaved-file dialog.
    pub const DIALOG_H: f32 = 108.0;
    /// Gap between panes, across.
    pub const GAP_ACROSS: f32 = 8.0;
    /// Gap between panes, down. It leaves room for the label on the frame line.
    pub const GAP_DOWN: f32 = 18.0;
    /// Pane padding at the sides.
    pub const PAD_SIDE: f32 = 10.0;
    /// Pane padding at the top.
    pub const PAD_TOP: f32 = 14.0;
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
    pub const TOOLBAR_H: f32 = 44.0;
    /// The rail.
    pub const RAIL_W: f32 = 68.0;
    /// A page button of the rail.
    pub const RAIL_ITEM_H: f32 = 56.0;
    /// The status bar.
    pub const STATUS_H: f32 = 24.0;
    /// The wordmark's height in the toolbar.
    pub const WORDMARK_H: f32 = 26.0;
    /// One icon.
    pub const ICON: f32 = 16.0;
    /// The editor gutter.
    pub const GUTTER_W: f32 = 52.0;
    /// Indent of one tree level.
    pub const TREE_INDENT: f32 = 14.0;
    /// The project dropdown in the toolbar.
    pub const PROJECT_DROPDOWN_W: f32 = 170.0;
    /// The target dropdown in the toolbar.
    pub const TARGET_DROPDOWN_W: f32 = 190.0;
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
    pub const TAPE_H: f32 = 122.0;
    /// Bottom pane, default.
    pub const BOTTOM_H: f32 = 168.0;
    /// Bottom pane, lowest. The highest is half the page.
    pub const BOTTOM_MIN: f32 = 120.0;
    /// A divider strip, and the step of its arrow keys.
    pub const DIVIDER: f32 = 8.0;
    /// The page margin, across: half a gap.
    pub const PAGE_MARGIN_ACROSS: f32 = GAP_ACROSS / 2.0;
    /// The page margin, down: half a gap.
    pub const PAGE_MARGIN_DOWN: f32 = GAP_DOWN / 2.0;
}

/// Radii and strokes.
pub mod stroke {
    /// The clipped top right corner of a pane.
    pub const PANE_CLIP: f32 = 9.0;
    /// The radius of a control.
    pub const CONTROL_RADIUS: u8 = 2;
    /// A frame.
    pub const FRAME: f32 = 1.0;
    /// The focus ring, and its distance from the control.
    pub const FOCUS_RING: f32 = 2.0;
    /// The focus ring's distance from the control.
    pub const FOCUS_GAP: f32 = 2.0;
    /// The bar at the left edge of a selected row.
    pub const SELECT_BAR: f32 = 3.0;
    /// The bar on top of the active tab.
    pub const TAB_BAR: f32 = 2.0;
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
    pub const HALO_BLUR: u8 = 10;
    /// Halo spread.
    pub const HALO_SPREAD: u8 = 1;
    /// Halo alpha, out of 1, of `bright`.
    pub const HALO_ALPHA: f32 = 0.30;
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
            v.window_corner_radius = CornerRadius::ZERO;
            v.menu_corner_radius = CornerRadius::ZERO;
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
        }
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
