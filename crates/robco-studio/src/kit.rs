// SPDX-License-Identifier: GPL-3.0-or-later

//! The thin layer: Studio's own components over the window toolkit.
//!
//! A view builds the window from these and never styles the toolkit itself.
//! Each component takes its colours and sizes from [`Theme`], draws itself,
//! and gives the accessibility tree a role and a name.

mod controls;
pub use controls::{Rail, Slider};

use crate::art::{Art, Icon, LabelFont};
use crate::theme::{Palette, Theme, Type, size, stroke};
use eframe::egui::{
    self, Color32, CornerRadius, Id, Key, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind,
    Ui, UiBuilder, WidgetInfo, accesskit::Role,
};

/// The level of a message: each has an icon, a word and a colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// A job ended well.
    Success,
    /// A step, or plain news.
    Info,
    /// It went on, and something needs a look.
    Warning,
    /// It stopped.
    Error,
}

impl Level {
    /// The level's icon.
    pub fn icon(self) -> Icon {
        match self {
            Level::Success => Icon::Ok,
            Level::Info => Icon::Info,
            Level::Warning => Icon::Warn,
            Level::Error => Icon::Error,
        }
    }

    /// The level's colour.
    pub fn color(self, p: &Palette) -> Color32 {
        match self {
            Level::Success => p.bright,
            Level::Info => p.text,
            Level::Warning => p.warn,
            Level::Error => p.alarm,
        }
    }
}

/// The mark a menu row carries, and whether it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuMark {
    /// No state: the row only acts.
    None,
    /// A check item, on or off.
    Check(bool),
    /// One of a set of radio items, chosen or not.
    Radio(bool),
}

impl MenuMark {
    /// The row shows its tick.
    pub fn on(self) -> bool {
        matches!(self, MenuMark::Check(true) | MenuMark::Radio(true))
    }
}

/// The kinds of button of the design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// The four toolbar actions and the next step of a pane: `bright`
    /// fill, `ink` label, a halo.
    Filled,
    /// Everything else: `frame` stroke, `bright` label, no fill.
    Outlined,
}

/// A button to draw.
#[derive(Debug, Clone, Copy)]
pub struct Button<'a> {
    /// Its kind.
    pub kind: ButtonKind,
    /// Its label, a verb. Drawn in caps, named in normal case.
    pub text: &'a str,
    /// An icon before the label.
    pub icon: Option<Icon>,
    /// 24 high in place of 28.
    pub small: bool,
    /// A fixed width, or None to fit the label.
    pub width: Option<f32>,
    /// The narrowest a button that fits its label gets.
    pub min_width: f32,
    /// The padding at each side of the icon and label.
    pub pad: f32,
    /// A job is running: the label names it and the hourglass shows.
    pub busy: bool,
    /// Why it cannot act, or None when it can.
    pub unavailable: Option<&'a str>,
}

impl<'a> Button<'a> {
    /// An outlined button that acts.
    pub fn quiet(text: &'a str) -> Self {
        Self {
            kind: ButtonKind::Outlined,
            text,
            icon: None,
            small: false,
            width: None,
            min_width: size::SMALL_H,
            pad: size::LABEL_PAD,
            busy: false,
            unavailable: None,
        }
    }

    /// The same button of another kind.
    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    /// The same button with an icon.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The same button, small.
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    /// The same button at a fixed width.
    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    /// The same button fit to its label, with `pad` at each side and no
    /// narrower than `min`.
    pub fn fitted(mut self, pad: f32, min: f32) -> Self {
        self.width = None;
        self.pad = pad;
        self.min_width = min;
        self
    }

    /// The same button while its job runs.
    pub fn busy(mut self) -> Self {
        self.busy = true;
        self
    }

    /// The same button, shown as not available, with the reason.
    pub fn unavailable(mut self, why: &'a str) -> Self {
        self.unavailable = Some(why);
        self
    }
}

/// What a tab reports.
#[derive(Debug, Default, Clone, Copy)]
pub struct TabOut {
    /// The tab was chosen.
    pub chosen: bool,
    /// Its close button was pressed.
    pub close: bool,
}

/// What a divider reports.
#[derive(Debug, Default, Clone, Copy)]
pub struct DividerOut {
    /// Moved by this much this frame.
    pub delta: f32,
    /// Enter: put it back.
    pub reset: bool,
}

/// The rectangles of a pane.
#[derive(Debug, Clone, Copy)]
pub struct PaneRects {
    /// The whole pane, frame included.
    pub outer: Rect,
    /// Where the content goes, inside the padding.
    pub content: Rect,
    /// The strip where tabs stand, on the title row's rule.
    pub header: Rect,
    /// The title row, above its rule. Pane buttons go at its right.
    pub row: Rect,
}

/// The theme and the textures, which every component needs.
#[derive(Clone, Copy)]
pub struct Kit<'a> {
    /// The tokens.
    pub theme: &'a Theme,
    /// The textures.
    pub art: &'a Art,
}

impl<'a> Kit<'a> {
    /// The colours.
    pub fn p(&self) -> Palette {
        self.theme.palette()
    }

    fn label_scale(&self, ui: &Ui) -> Option<f32> {
        LabelFont::device_scale(ui.ctx().pixels_per_point())
    }

    /// The width of a caps label.
    pub fn caps_width(&self, ui: &Ui, text: &str) -> f32 {
        self.caps_width_tracked(ui, text, 0.0)
    }

    /// The width of a caps label with `tracking` font pixels after each
    /// cell but the last.
    pub fn caps_width_tracked(&self, ui: &Ui, text: &str, tracking: f32) -> f32 {
        match self.label_scale(ui) {
            Some(k) => LabelFont::width(text, k, ui.ctx().pixels_per_point(), tracking),
            None => {
                ui.painter()
                    .layout_job(self.fallback_job(text, tracking, Color32::PLACEHOLDER))
                    .size()
                    .x
            }
        }
    }

    fn fallback_job(&self, text: &str, tracking: f32, color: Color32) -> egui::text::LayoutJob {
        egui::text::LayoutJob::single_section(
            text.to_uppercase(),
            egui::TextFormat {
                font_id: self.theme.font(Type::LabelFallback),
                extra_letter_spacing: tracking,
                color,
                ..Default::default()
            },
        )
    }

    /// Paint a caps label in Spleen, or in Plex Mono bold caps when the
    /// display scale is not a whole Spleen scale. Its vertical centre is `y`.
    pub fn caps(
        &self,
        painter: &egui::Painter,
        x: f32,
        y: f32,
        text: &str,
        color: Color32,
    ) -> Rect {
        self.caps_styled(painter, egui::pos2(x, y), text, color, 0.0, false)
    }

    /// Paint a caps label as [`Kit::caps`] does, with `tracking` font
    /// pixels after each cell, and over its halo when `lit` and the screen
    /// is Soft. Light only `bright` labels on `ground` or `pane`. The Plex Mono
    /// fallback has no halo. `at` is the left end of the label's centre line.
    pub fn caps_styled(
        &self,
        painter: &egui::Painter,
        at: egui::Pos2,
        text: &str,
        color: Color32,
        tracking: f32,
        lit: bool,
    ) -> Rect {
        let half = crate::theme::text::LABEL_CELL_H / 2.0;
        let halo = if lit { self.theme.label_halo() } else { None };
        match LabelFont::device_scale(painter.ctx().pixels_per_point()) {
            Some(k) => self.art.label.paint_lit(
                painter,
                egui::pos2(at.x, at.y - half),
                text,
                color,
                k,
                tracking,
                halo,
            ),
            None => {
                let g = painter.layout_job(self.fallback_job(text, tracking, color));
                let r = Rect::from_min_size(egui::pos2(at.x, at.y - g.size().y / 2.0), g.size());
                painter.galley(r.min, g, color);
                r
            }
        }
    }

    /// One line of text cut to `max_w` with an ellipsis. The second value
    /// is true when the text was cut: then the caller gives the full text
    /// in a tooltip and to the screen reader.
    pub fn fit(
        &self,
        painter: &egui::Painter,
        text: &str,
        t: Type,
        color: Color32,
        max_w: f32,
    ) -> (std::sync::Arc<egui::Galley>, bool) {
        let mut job =
            egui::text::LayoutJob::simple_singleline(text.to_owned(), self.theme.font(t), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(0.0));
        let g = painter.layout_job(job);
        let cut = g.elided;
        (g, cut)
    }

    /// Body text as a widget, so that it is in the accessibility tree.
    pub fn text(&self, ui: &mut Ui, text: &str, t: Type, color: Color32) -> Response {
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .font(self.theme.font(t))
                    .color(color),
            )
            .wrap(),
        )
    }

    /// The 2 px focus ring, 2 px away from `rect`.
    pub fn focus_ring(&self, painter: &egui::Painter, rect: Rect) {
        painter.rect_stroke(
            rect.expand(stroke::FOCUS_GAP),
            self.theme.control_radius(),
            Stroke::new(stroke::FOCUS_RING, self.p().bright),
            StrokeKind::Outside,
        );
    }

    fn dashed_rect(&self, painter: &egui::Painter, rect: Rect, color: Color32) {
        let r = rect.shrink(stroke::FRAME / 2.0);
        let pts = [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            r.left_top(),
        ];
        painter.extend(Shape::dashed_line(
            &pts,
            Stroke::new(stroke::FRAME, color),
            stroke::DASH,
            stroke::DASH_GAP,
        ));
    }

    /// A pane: a rounded frame, then a title row closed by a rule. The
    /// title is a lit caps label at the row's left; with `label` false the
    /// row is left to tabs, which stand on the rule. The pane is a node of
    /// its own in the accessibility tree, named `name`, and it can take focus.
    pub fn pane<R>(
        &self,
        ui: &mut Ui,
        rect: Rect,
        name: &str,
        label: bool,
        focus_now: bool,
        add: impl FnOnce(&mut Ui, PaneRects) -> R,
    ) -> (Response, R) {
        self.pane_as(ui, rect, name, Role::Pane, label, focus_now, add)
    }

    /// A pane, as [`Kit::pane`], whose node in the accessibility tree has
    /// `role`: a dialog drawn as a pane.
    #[allow(clippy::too_many_arguments)]
    pub fn pane_as<R>(
        &self,
        ui: &mut Ui,
        rect: Rect,
        name: &str,
        role: Role,
        label: bool,
        focus_now: bool,
        add: impl FnOnce(&mut Ui, PaneRects) -> R,
    ) -> (Response, R) {
        let p = self.p();
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt(("pane", name))
                .max_rect(rect)
                .sense(Sense::focusable_noninteractive())
                .accessibility_label(name)
                .accessibility_role(role),
        );
        if focus_now {
            let id = child.unique_id();
            child.memory_mut(|m| m.request_focus(id));
        }
        let focused = self.contains_focus(&child, rect);
        let painter = child.painter().clone();
        let pr = self.theme.pane_radius();
        if focused && let Some(h) = self.theme.pane_halo() {
            painter.add(h.as_shape(rect, pr));
        }
        let line = if focused { p.bright } else { p.frame };
        painter.rect(rect, pr, p.pane, self.theme.frame(line), StrokeKind::Inside);
        let head = (rect.top() + size::PANE_HEAD_H).min(rect.bottom());
        let row = Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.right(), (head - stroke::FRAME).max(rect.top())),
        );
        painter.hline(
            rect.x_range().shrink(stroke::FRAME),
            head - stroke::FRAME / 2.0,
            self.theme.frame(p.frame),
        );
        if label {
            self.caps_styled(
                &painter,
                egui::pos2(rect.left() + size::PAD_SIDE, row.center().y),
                name,
                p.bright,
                0.0,
                true,
            );
        }
        let header = Rect::from_min_max(
            egui::pos2(
                rect.left() + size::TAB_INSET,
                (row.bottom() - size::TAB_H).max(rect.top()),
            ),
            egui::pos2(
                (rect.right() - size::TAB_INSET).max(rect.left()),
                row.bottom(),
            ),
        );
        let content = Rect::from_min_max(
            egui::pos2(rect.left() + size::PAD_SIDE, head + size::PAD_TOP),
            egui::pos2(
                rect.right() - size::PAD_SIDE,
                rect.bottom() - size::PAD_BOTTOM,
            ),
        );
        let r = add(
            &mut child,
            PaneRects {
                outer: rect,
                content,
                header,
                row,
            },
        );
        (child.response(), r)
    }

    /// Whether the focused widget lies in `rect`.
    pub fn contains_focus(&self, ui: &Ui, rect: Rect) -> bool {
        let ctx = ui.ctx();
        ctx.memory(|m| m.focused())
            .and_then(|id| ctx.read_response(id))
            .is_some_and(|r| rect.contains(r.rect.center()))
    }

    /// The width a button takes.
    pub fn button_width(&self, ui: &Ui, b: &Button<'_>) -> f32 {
        let icon_w = if b.icon.is_some() || b.busy {
            size::ICON + size::ICON_GAP
        } else {
            0.0
        };
        b.width
            .unwrap_or((b.pad * 2.0 + icon_w + self.caps_width(ui, b.text)).max(b.min_width))
    }

    /// The zoom group: Smaller, the step's name, Larger, as one outlined
    /// group 24 high. A part that cannot act is `dim`, disabled and says
    /// why. Returns whether Smaller and Larger were pressed.
    pub fn zoom_group(
        &self,
        ui: &mut Ui,
        step: &str,
        smaller: Option<&str>,
        larger: Option<&str>,
    ) -> (bool, bool) {
        let p = self.p();
        let w = size::SMALL_H * 2.0 + size::ZOOM_NAME_W;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, size::SMALL_H), Sense::hover());
        let cr = self.theme.control_radius();
        let mut out = [false, false];
        let parts = [
            (rect.left(), "Smaller", Icon::Remove, smaller),
            (rect.right() - size::SMALL_H, "Larger", Icon::Add, larger),
        ];
        for (i, (x, name, icon, why)) in parts.into_iter().enumerate() {
            let r = Rect::from_min_size(
                egui::pos2(x, rect.top()),
                egui::vec2(size::SMALL_H, size::SMALL_H),
            );
            let sense = if why.is_some() {
                Sense::focusable_noninteractive()
            } else {
                Sense::click()
            };
            let resp = ui.interact(r, ui.scope_id().with(("zoom", name)), sense);
            resp.widget_info(|| WidgetInfo::labeled(Role::Button, why.is_none(), name));
            if let Some(why) = why {
                ui.ctx().accesskit_node_builder(resp.id, |n| {
                    n.set_disabled();
                    n.set_description(why.to_owned());
                });
            }
            let painter = ui.painter();
            if why.is_none() && resp.is_pointer_button_down_on() {
                painter.rect_filled(r, cr, p.select);
            } else if why.is_none() && resp.hovered() {
                painter.rect_filled(r, cr, p.raised);
            }
            self.art.icon(
                painter,
                r,
                icon,
                if why.is_some() { p.dim } else { p.bright },
            );
            if resp.has_focus() {
                self.focus_ring(painter, r);
            }
            out[i] = resp.clicked();
            let tip = why.unwrap_or(name);
            if resp.has_focus() {
                resp.show_tooltip_text(tip);
            }
            resp.on_hover_text(tip);
        }
        let painter = ui.painter();
        let mid = Rect::from_min_max(
            egui::pos2(rect.left() + size::SMALL_H, rect.top()),
            egui::pos2(rect.right() - size::SMALL_H, rect.bottom()),
        );
        painter.vline(mid.left(), rect.y_range(), self.theme.frame(p.frame));
        painter.vline(mid.right(), rect.y_range(), self.theme.frame(p.frame));
        let tw = self.caps_width(ui, step);
        self.caps(
            painter,
            mid.center().x - tw / 2.0,
            mid.center().y,
            step,
            p.text,
        );
        painter.rect_stroke(rect, cr, self.theme.frame(p.frame), StrokeKind::Inside);
        ui.put(
            mid,
            egui::Label::new(egui::RichText::new(step).color(Color32::TRANSPARENT)),
        );
        (out[0], out[1])
    }

    /// A button. It always takes keyboard focus. An unavailable one never
    /// acts: it is marked so in the accessibility tree, and says why.
    pub fn button(&self, ui: &mut Ui, b: Button<'_>) -> Response {
        let p = self.p();
        let h = if b.small {
            size::SMALL_H
        } else {
            size::CONTROL_H
        };
        let icon_w = if b.icon.is_some() || b.busy {
            size::ICON + size::ICON_GAP
        } else {
            0.0
        };
        let label_w = self.caps_width(ui, b.text);
        let w = self.button_width(ui, &b);
        let sense = if b.unavailable.is_some() || b.busy {
            Sense::focusable_noninteractive()
        } else {
            Sense::click()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), sense);
        let able = b.unavailable.is_none() && !b.busy;
        resp.widget_info(|| WidgetInfo::labeled(Role::Button, able, b.text));
        if let Some(why) = b.unavailable {
            ui.ctx().accesskit_node_builder(resp.id, |n| {
                n.set_disabled();
                n.set_description(why.to_owned());
            });
        }
        let painter = ui.painter();
        let pressed = resp.is_pointer_button_down_on();
        let hovered = resp.hovered();
        let cr = self.theme.control_radius();
        let fg = if b.unavailable.is_some() {
            self.dashed_rect(painter, rect, p.frame);
            p.dim
        } else if b.busy {
            painter.rect(
                rect,
                cr,
                p.select,
                self.theme.frame(p.bright),
                StrokeKind::Inside,
            );
            p.bright
        } else {
            match b.kind {
                ButtonKind::Filled => {
                    if !pressed && let Some(h) = self.theme.halo() {
                        painter.add(h.as_shape(rect, cr));
                    }
                    let fill = if pressed {
                        self.theme.pressed(p.bright)
                    } else if hovered {
                        self.theme.hovered(p.bright)
                    } else {
                        p.bright
                    };
                    painter.rect_filled(rect, cr, fill);
                    p.ink
                }
                ButtonKind::Outlined => {
                    let fill = if pressed {
                        p.select
                    } else if hovered {
                        p.raised
                    } else {
                        Color32::TRANSPARENT
                    };
                    painter.rect(
                        rect,
                        cr,
                        fill,
                        self.theme.frame(p.frame),
                        StrokeKind::Inside,
                    );
                    p.bright
                }
            }
        };
        let mut x = rect.center().x - (icon_w + label_w) / 2.0;
        let icon = if b.busy { Some(Icon::Wait) } else { b.icon };
        if let Some(i) = icon {
            let r = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(size::ICON, h));
            self.art.icon(painter, r, i, fg);
            x += icon_w;
        }
        self.caps(painter, x, rect.center().y, b.text, fg);
        if resp.has_focus() {
            self.focus_ring(painter, rect);
        }
        if let Some(why) = b.unavailable {
            if resp.has_focus() {
                resp.show_tooltip_text(why);
            }
            resp.on_hover_text(why)
        } else {
            resp
        }
    }

    /// A 24 by 24 button with an icon only, a tooltip and a name.
    pub fn icon_button(
        &self,
        ui: &mut Ui,
        icon: Icon,
        name: &str,
        unavailable: Option<&str>,
    ) -> Response {
        let p = self.p();
        let sense = if unavailable.is_some() {
            Sense::focusable_noninteractive()
        } else {
            Sense::click()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(size::SMALL_H, size::SMALL_H), sense);
        resp.widget_info(|| WidgetInfo::labeled(Role::Button, unavailable.is_none(), name));
        if let Some(why) = unavailable {
            ui.ctx().accesskit_node_builder(resp.id, |n| {
                n.set_disabled();
                n.set_description(why.to_owned());
            });
        }
        let painter = ui.painter();
        if unavailable.is_none() && resp.hovered() {
            painter.rect_filled(rect, self.theme.control_radius(), p.raised);
        }
        let fg = if unavailable.is_some() {
            p.dim
        } else {
            p.bright
        };
        self.art.icon(painter, rect, icon, fg);
        if resp.has_focus() {
            self.focus_ring(painter, rect);
            resp.show_tooltip_text(unavailable.unwrap_or(name));
        }
        resp.on_hover_text(unavailable.unwrap_or(name))
    }

    /// A tab standing on a pane's top line. `caps` for the bottom pane's
    /// tabs; otherwise a file tab with an icon, a name, a square when not
    /// saved and a close button. `key` names the tab apart from its text,
    /// which two tabs may share.
    #[allow(clippy::too_many_arguments)]
    pub fn tab(
        &self,
        ui: &mut Ui,
        at: Pos2,
        key: impl std::hash::Hash + std::fmt::Debug,
        text: &str,
        icon: Option<Icon>,
        active: bool,
        caps: bool,
        dirty: bool,
    ) -> (Rect, TabOut) {
        let p = self.p();
        let pad = size::LABEL_PAD;
        let h = size::TAB_H;
        let font = self.theme.font(Type::Body);
        let text_w = if caps {
            self.caps_width(ui, text)
        } else {
            ui.painter()
                .layout_no_wrap(text.to_owned(), font.clone(), p.text)
                .size()
                .x
        };
        let icon_w = if icon.is_some() {
            size::ICON + size::ICON_GAP
        } else {
            0.0
        };
        let mark_w = if dirty { size::STEP_12 } else { 0.0 };
        let close_w = if caps { 0.0 } else { size::SMALL_H };
        let w = pad * 2.0 + icon_w + text_w + mark_w + close_w;
        let rect = Rect::from_min_size(egui::pos2(at.x, at.y - h / 2.0), egui::vec2(w, h));
        let name = if dirty {
            format!("{text}, not saved")
        } else {
            text.to_owned()
        };
        let id = ui.scope_id().with(("tab", key));
        let resp = ui.interact(rect, id, Sense::click());
        resp.widget_info(|| WidgetInfo::selected(Role::Tab, true, active, &name));
        let painter = ui.painter();
        let (fg, icon_fg) = self.tab_shape(painter, rect, active, &resp);
        let mut x = rect.left() + pad;
        if let Some(i) = icon {
            self.art.icon(
                painter,
                Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(size::ICON, h)),
                i,
                icon_fg,
            );
            x += icon_w;
        }
        if caps {
            self.caps(painter, x, rect.center().y, text, fg);
        } else {
            let g = painter.layout_no_wrap(text.to_owned(), font, fg);
            painter.galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, fg);
        }
        x += text_w;
        if dirty {
            let s = size::STEP_6;
            painter.rect_filled(
                Rect::from_center_size(
                    egui::pos2(x + mark_w / 2.0, rect.center().y),
                    egui::vec2(s, s),
                ),
                CornerRadius::ZERO,
                fg,
            );
        }
        let mut out = TabOut {
            chosen: resp.clicked(),
            close: false,
        };
        if !caps {
            let cr = Rect::from_min_size(
                egui::pos2(rect.right() - close_w, rect.top()),
                egui::vec2(close_w, h),
            );
            let cid = id.with("close");
            let c = ui.interact(cr, cid, Sense::click());
            let close_name = format!("Close {text}");
            c.widget_info(|| WidgetInfo::labeled(Role::Button, true, &close_name));
            if c.hovered() {
                painter.rect_filled(
                    cr.shrink(stroke::FRAME),
                    self.theme.control_radius(),
                    if active {
                        self.theme.hovered(p.bright)
                    } else {
                        p.raised
                    },
                );
            }
            self.art.icon(painter, cr, Icon::Close, fg);
            if c.has_focus() {
                self.focus_ring(painter, cr);
            }
            out.close = c.clicked();
            out.chosen &= !out.close;
        }
        if resp.has_focus() {
            self.focus_ring(painter, rect);
            out.chosen |= ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space));
        }
        (rect, out)
    }

    /// The shape of a tab, a page button or a part of a segmented control:
    /// a `bright` fill and a halo when chosen, else a `frame` outline open
    /// at the bottom on `pane`, `raised` on hover and `select` while
    /// pressed. Returns the label colour and the icon colour.
    fn tab_shape(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        active: bool,
        resp: &Response,
    ) -> (Color32, Color32) {
        let p = self.p();
        let tr = self.theme.tab_radius();
        if active {
            if let Some(h) = self.theme.halo() {
                painter.add(h.as_shape(rect, tr));
            }
            painter.rect_filled(rect, tr, p.bright);
            return (p.ink, p.ink);
        }
        let fill = if resp.is_pointer_button_down_on() {
            p.select
        } else if resp.hovered() {
            p.raised
        } else {
            p.pane
        };
        painter.rect_filled(rect, tr, fill);
        // Three sides: the bottom edge lies under the clip.
        painter.with_clip_rect(rect).rect_stroke(
            Rect::from_min_max(
                rect.min,
                egui::pos2(rect.right(), rect.bottom() + size::UNIT),
            ),
            tr,
            self.theme.frame(p.frame),
            StrokeKind::Inside,
        );
        (p.text, p.bright)
    }

    /// A segmented control: one outlined group of caps parts with lines
    /// between them. The chosen part is filled, with a tick. Each part is a
    /// radio button named by its text. Returns the part chosen this frame.
    pub fn segmented(
        &self,
        ui: &mut Ui,
        key: &str,
        parts: &[&str],
        chosen: usize,
    ) -> Option<usize> {
        self.segments(ui, key, parts, chosen, None)
    }

    /// The segmented control as a tab list named `name`, `FIELD_H` high and
    /// with no tick. It is one stop in the Tab order: only the chosen part
    /// takes focus, and the left and right arrows move to the part beside
    /// it. Each part is a tab named by its text. Returns the part chosen
    /// this frame.
    pub fn segmented_tabs(
        &self,
        ui: &mut Ui,
        name: &str,
        parts: &[&str],
        chosen: usize,
    ) -> Option<usize> {
        let w = self.segmented_tabs_width(ui, parts);
        self.tab_list(ui, name, parts, chosen, w)
    }

    /// The tab list of [`Kit::segmented_tabs`], `width` wide: the parts
    /// share the room left over.
    pub fn segmented_tabs_wide(
        &self,
        ui: &mut Ui,
        name: &str,
        parts: &[&str],
        chosen: usize,
        width: f32,
    ) -> Option<usize> {
        let w = width.max(self.segmented_tabs_width(ui, parts));
        self.tab_list(ui, name, parts, chosen, w)
    }

    fn tab_list(
        &self,
        ui: &mut Ui,
        name: &str,
        parts: &[&str],
        chosen: usize,
        w: f32,
    ) -> Option<usize> {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, size::FIELD_H), Sense::hover());
        let mut list = ui.new_child(
            UiBuilder::new()
                .id_salt(("tabs", name))
                .max_rect(rect)
                .sense(Sense::hover())
                .accessibility_label(name)
                .accessibility_role(Role::TabList)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        // A list that only hovers gets no bounds of its own.
        list.ctx().accesskit_node_builder(list.unique_id(), |n| {
            n.set_bounds(egui::accesskit::Rect {
                x0: rect.min.x.into(),
                y0: rect.min.y.into(),
                x1: rect.max.x.into(),
                y1: rect.max.y.into(),
            });
        });
        self.segments(&mut list, name, parts, chosen, Some(w))
    }

    /// The width [`Kit::segmented_tabs`] takes for `parts`.
    pub fn segmented_tabs_width(&self, ui: &Ui, parts: &[&str]) -> f32 {
        parts
            .iter()
            .map(|t| self.caps_width(ui, t) + size::LABEL_PAD * 2.0)
            .sum()
    }

    /// The parts of a segmented control. `tabs` is the width of a tab
    /// list, None for a radio group.
    fn segments(
        &self,
        ui: &mut Ui,
        key: &str,
        parts: &[&str],
        chosen: usize,
        tabs: Option<f32>,
    ) -> Option<usize> {
        let p = self.p();
        let tick = if tabs.is_some() {
            0.0
        } else {
            size::ICON + size::UNIT
        };
        let h = if tabs.is_some() {
            size::FIELD_H
        } else {
            size::SMALL_H
        };
        let mut widths: Vec<f32> = parts
            .iter()
            .enumerate()
            .map(|(i, t)| {
                self.caps_width(ui, t)
                    + size::LABEL_PAD * 2.0
                    + if i == chosen { tick } else { 0.0 }
            })
            .collect();
        let fit: f32 = widths.iter().sum();
        if let Some(w) = tabs
            && w > fit
            && !parts.is_empty()
        {
            let more = (w - fit) / parts.len() as f32;
            widths.iter_mut().for_each(|x| *x += more);
        }
        let total: f32 = widths.iter().sum();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(total, h), Sense::hover());
        let cr = self.theme.control_radius();
        let painter = ui.painter();
        let mut out = None;
        let mut x = rect.left();
        let last = parts.len().saturating_sub(1);
        for (i, (t, w)) in parts.iter().zip(&widths).enumerate() {
            let r = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*w, rect.height()));
            let on = i == chosen;
            let id = ui.scope_id().with((key, i));
            // A tab list is one stop: the parts not chosen only click.
            let sense = if tabs.is_some() && !on {
                Sense::CLICK
            } else {
                Sense::click()
            };
            let resp = ui.interact(r, id, sense);
            if on && ui.data_mut(|d| d.remove_temp::<bool>(id.with("focus"))) == Some(true) {
                ui.memory_mut(|m| m.request_focus(id));
            }
            let role = if tabs.is_some() {
                Role::Tab
            } else {
                Role::RadioButton
            };
            resp.widget_info(|| WidgetInfo::selected(role, true, on, *t));
            if tabs.is_some() {
                ui.ctx()
                    .accesskit_node_builder(resp.id, |n| n.set_selected(on));
            }
            let fg = if on {
                if let Some(h) = self.theme.halo() {
                    painter.add(h.as_shape(r, cr));
                }
                painter.rect_filled(r, cr, p.bright);
                p.ink
            } else {
                if resp.is_pointer_button_down_on() {
                    painter.rect_filled(r, cr, p.select);
                } else if resp.hovered() {
                    painter.rect_filled(r, cr, p.raised);
                }
                p.text
            };
            // A tab list wider than its labels centres each one.
            let mut lx = if tabs.is_some() {
                r.center().x - self.caps_width(ui, t) / 2.0
            } else {
                r.left() + size::LABEL_PAD
            };
            if on && tick > 0.0 {
                self.art.icon(
                    painter,
                    Rect::from_min_size(
                        egui::pos2(lx, r.top()),
                        egui::vec2(size::ICON, r.height()),
                    ),
                    Icon::Ok,
                    fg,
                );
                lx += tick;
            }
            self.caps(painter, lx, r.center().y, t, fg);
            if i > 0 {
                painter.vline(x, rect.y_range(), self.theme.frame(p.frame));
            }
            if resp.has_focus() {
                self.focus_ring(painter, r);
            }
            if resp.clicked()
                || (resp.has_focus()
                    && ui.input(|k| k.key_pressed(Key::Enter) || k.key_pressed(Key::Space)))
            {
                out = Some(i);
            }
            if tabs.is_some() && resp.has_focus() {
                ui.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        id,
                        egui::EventFilter {
                            horizontal_arrows: true,
                            ..Default::default()
                        },
                    );
                });
                let (left, right) = ui.input(|k| {
                    (
                        k.key_pressed(Key::ArrowLeft),
                        k.key_pressed(Key::ArrowRight),
                    )
                });
                let to = if left {
                    Some(if i == 0 { last } else { i - 1 })
                } else if right {
                    Some(if i == last { 0 } else { i + 1 })
                } else {
                    None
                };
                if let Some(to) = to {
                    // That part takes focus once it is the chosen one.
                    let next = ui.scope_id().with((key, to)).with("focus");
                    ui.data_mut(|d| d.insert_temp(next, true));
                    out = Some(to);
                }
            }
            x += w;
        }
        painter.rect_stroke(rect, cr, self.theme.frame(p.frame), StrokeKind::Inside);
        out
    }

    /// One row of the tree: indent, chevron for a folder, icon, name.
    #[allow(clippy::too_many_arguments)]
    pub fn tree_row(
        &self,
        ui: &mut Ui,
        id: Id,
        rect: Rect,
        depth: usize,
        name: &str,
        icon: Icon,
        folder: Option<bool>,
        selected: bool,
    ) -> Response {
        let p = self.p();
        let resp = ui.interact(rect, id, Sense::click());
        resp.widget_info(|| WidgetInfo::selected(Role::TreeItem, true, selected, name));
        if let Some(open) = folder {
            ui.ctx()
                .accesskit_node_builder(id, |n| n.set_expanded(open));
        }
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, CornerRadius::ZERO, p.select);
            painter.rect_filled(
                Rect::from_min_size(rect.min, egui::vec2(stroke::SELECT_BAR, rect.height())),
                CornerRadius::ZERO,
                p.bright,
            );
        } else if resp.hovered() {
            painter.rect_filled(rect, CornerRadius::ZERO, p.raised);
        }
        let mut x = rect.left() + size::UNIT * 2.0 + depth as f32 * size::TREE_INDENT;
        let slot = |x: f32| {
            Rect::from_min_size(
                egui::pos2(x, rect.top()),
                egui::vec2(size::ICON, rect.height()),
            )
        };
        let fg = if selected {
            p.bright
        } else if folder.is_some() {
            p.dim
        } else {
            p.text
        };
        if let Some(open) = folder {
            self.art.icon(
                painter,
                slot(x),
                if open {
                    Icon::ChevDown
                } else {
                    Icon::ChevRight
                },
                p.dim,
            );
        } else {
            self.art.icon(painter, slot(x), icon, fg);
        }
        x += size::ICON + size::UNIT;
        let (g, cut) = self.fit(
            painter,
            name,
            Type::Body,
            fg,
            rect.right() - size::UNIT * 2.0 - x,
        );
        painter.galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, fg);
        if resp.has_focus() {
            self.focus_ring(painter, rect.shrink(stroke::FOCUS_GAP + stroke::FOCUS_RING));
        }
        if cut { resp.on_hover_text(name) } else { resp }
    }

    /// A one-line text field: a 1 px `frame` on `ground`, 26 high, a hint in
    /// `dim` while empty, a doubled `bright` stroke while focused, an `alarm`
    /// one while refused. Named `name` for a screen reader.
    #[allow(clippy::too_many_arguments)]
    pub fn text_field(
        &self,
        ui: &mut Ui,
        id: Id,
        value: &mut String,
        name: &str,
        hint: &str,
        width: f32,
        refused: bool,
    ) -> Response {
        let p = self.p();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, size::FIELD_H), Sense::hover());
        let inner = rect.shrink2(egui::vec2(size::UNIT * 2.0, size::UNIT));
        // The fill goes first: shapes later on the same layer cover earlier
        // ones, so a fill after the text would hide it.
        ui.painter()
            .rect_filled(rect, self.theme.control_radius(), p.ground);
        // Placed, not put: the field's own allocation keeps the layout's
        // gap to the next widget.
        let resp = ui.place(
            inner,
            egui::TextEdit::singleline(value)
                .id(id)
                .frame(egui::Frame::NONE)
                .font(self.theme.font(Type::Body))
                .text_color(p.text)
                .hint_text(egui::RichText::new(hint).color(p.dim))
                .desired_width(inner.width()),
        );
        ui.ctx()
            .accesskit_node_builder(id, |n| n.set_label(name.to_owned()));
        let painter = ui.painter();
        let (line, w) = if refused {
            (p.alarm, stroke::FRAME * 2.0)
        } else if resp.has_focus() {
            (p.bright, stroke::FRAME * 2.0)
        } else {
            (p.frame, stroke::FRAME)
        };
        painter.rect_stroke(
            rect,
            self.theme.control_radius(),
            Stroke::new(w, line),
            StrokeKind::Inside,
        );
        resp
    }

    /// A closed dropdown: an icon, the value, a chevron. A click opens its
    /// list; Esc and a click outside close it. Returns the row chosen.
    #[allow(clippy::too_many_arguments)]
    pub fn dropdown(
        &self,
        ui: &mut Ui,
        name: &str,
        icon: Icon,
        value: &str,
        none: bool,
        width: f32,
        rows: &[&str],
    ) -> Option<usize> {
        self.dropdown_response(ui, name, icon, value, none, width, rows)
            .1
    }

    /// The dropdown of [`Kit::dropdown`], with its response, to give it
    /// the focus.
    #[allow(clippy::too_many_arguments)]
    pub fn dropdown_response(
        &self,
        ui: &mut Ui,
        name: &str,
        icon: Icon,
        value: &str,
        none: bool,
        width: f32,
        rows: &[&str],
    ) -> (Response, Option<usize>) {
        let p = self.p();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, size::FIELD_H), Sense::click());
        let shown = format!("{name}: {value}");
        resp.widget_info(|| WidgetInfo::labeled(Role::ComboBox, true, &shown));
        let painter = ui.painter();
        let cr = self.theme.control_radius();
        painter.rect_filled(rect, cr, if resp.hovered() { p.raised } else { p.ground });
        painter.rect_stroke(rect, cr, self.theme.frame(p.frame), StrokeKind::Inside);
        let slot = |x: f32| {
            Rect::from_min_size(
                egui::pos2(x, rect.top()),
                egui::vec2(size::ICON, rect.height()),
            )
        };
        let x = rect.left() + size::UNIT * 2.0;
        self.art.icon(painter, slot(x), icon, p.dim);
        let fg = if none { p.dim } else { p.text };
        let tx = x + size::ICON + size::UNIT * 2.0;
        let chev = rect.right() - size::UNIT * 2.0 - size::ICON;
        let (g, cut) = self.fit(painter, value, Type::Body, fg, chev - size::UNIT - tx);
        painter.galley(egui::pos2(tx, rect.center().y - g.size().y / 2.0), g, fg);
        self.art.icon(painter, slot(chev), Icon::ChevDown, p.dim);
        if resp.has_focus() {
            self.focus_ring(painter, rect);
        }
        let resp = if cut { resp.on_hover_text(value) } else { resp };
        // Toggle before the popup reads its state, so the click's own
        // frame lays the list out.
        let popup_id = egui::Popup::default_response_id(&resp);
        if resp.clicked() {
            egui::Popup::toggle_id(ui.ctx(), popup_id);
        }
        let mut chosen = None;
        egui::Popup::from_response(&resp)
            .open_memory(None)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(width)
            .show(|ui| {
                for (i, r) in rows.iter().enumerate() {
                    if self.menu_row(ui, r, "", None, width).clicked() {
                        chosen = Some(i);
                    }
                }
            });
        if chosen.is_some() {
            egui::Popup::close_id(ui.ctx(), popup_id);
        }
        (resp, chosen)
    }

    /// A row of a list or a menu: text, a note at the right such as a
    /// shortcut, and, when it cannot act, the lock and the reason.
    pub fn menu_row(
        &self,
        ui: &mut Ui,
        text: &str,
        note: &str,
        unavailable: Option<&str>,
        width: f32,
    ) -> Response {
        self.menu_choice(ui, text, note, MenuMark::None, unavailable, width)
    }

    /// A menu row with a mark: a check item or one of a set of radio
    /// items. A marked row shows a tick at its left in its own text colour;
    /// an unmarked one keeps that place empty, or shows the lock when it
    /// cannot act.
    pub fn menu_choice(
        &self,
        ui: &mut Ui,
        text: &str,
        note: &str,
        mark: MenuMark,
        unavailable: Option<&str>,
        width: f32,
    ) -> Response {
        self.menu_choice_unsaved(ui, text, note, mark, unavailable, width, false)
    }

    /// A menu row with a mark, as [`Kit::menu_choice`]. An unsaved row
    /// shows the square of an unsaved file before its note, and a screen
    /// reader hears "not saved" after its name.
    #[allow(clippy::too_many_arguments)]
    pub fn menu_choice_unsaved(
        &self,
        ui: &mut Ui,
        text: &str,
        note: &str,
        mark: MenuMark,
        unavailable: Option<&str>,
        width: f32,
        unsaved: bool,
    ) -> Response {
        let p = self.p();
        let name = if unsaved {
            format!("{text}, not saved")
        } else {
            text.to_owned()
        };
        let sense = if unavailable.is_some() {
            Sense::focusable_noninteractive()
        } else {
            Sense::click()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, size::ROW_TREE), sense);
        let on = unavailable.is_none();
        resp.widget_info(|| match mark {
            MenuMark::None => WidgetInfo::labeled(Role::MenuItem, on, &name),
            MenuMark::Check(c) => WidgetInfo::selected(Role::MenuItemCheckBox, on, c, &name),
            MenuMark::Radio(c) => WidgetInfo::selected(Role::MenuItemRadio, on, c, &name),
        });
        if let Some(why) = unavailable {
            ui.ctx().accesskit_node_builder(resp.id, |n| {
                n.set_disabled();
                n.set_description(why.to_owned());
            });
        }
        let painter = ui.painter();
        if unavailable.is_none() && (resp.hovered() || resp.has_focus()) {
            painter.rect_filled(rect, CornerRadius::ZERO, p.raised);
        }
        let fg = if unavailable.is_some() { p.dim } else { p.text };
        let icon = if mark.on() {
            Some(Icon::Ok)
        } else if unavailable.is_some() {
            Some(Icon::Lock)
        } else {
            None
        };
        if let Some(icon) = icon {
            self.art.icon(
                painter,
                // The mark ends one unit before the label.
                Rect::from_min_size(
                    egui::pos2(
                        rect.left() + size::MENU_MARK_INDENT - size::UNIT - size::MENU_MARK,
                        rect.top(),
                    ),
                    egui::vec2(size::MENU_MARK, rect.height()),
                ),
                icon,
                fg,
            );
        }
        let x = rect.left() + size::MENU_MARK_INDENT;
        let n = painter.layout_no_wrap(note.to_owned(), self.theme.font(Type::Small), p.dim);
        let note_x = rect.right() - size::UNIT * 2.0 - n.size().x;
        let gap = if note.is_empty() {
            0.0
        } else {
            size::UNIT * 2.0
        };
        let square = if unsaved { size::STEP_12 } else { 0.0 };
        if unsaved {
            let s = size::STEP_6;
            painter.rect_filled(
                Rect::from_center_size(
                    egui::pos2(note_x - gap - square / 2.0, rect.center().y),
                    egui::vec2(s, s),
                ),
                CornerRadius::ZERO,
                fg,
            );
        }
        let (g, cut) = self.fit(painter, text, Type::Body, fg, note_x - gap - square - x);
        painter.galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, fg);
        painter.galley(
            egui::pos2(note_x, rect.center().y - n.size().y / 2.0),
            n,
            p.dim,
        );
        if resp.has_focus() {
            self.focus_ring(painter, rect.shrink(stroke::FOCUS_GAP + stroke::FOCUS_RING));
        }
        match unavailable {
            Some(why) => resp.on_hover_text(why),
            None if cut => resp.on_hover_text(text),
            None => resp,
        }
    }

    /// A row of a menu that cannot be picked: the info icon in the mark's
    /// place and a note in `dim`. A screen reader hears an unavailable
    /// menu item named by the note.
    pub fn menu_note(&self, ui: &mut Ui, text: &str, width: f32) -> Response {
        let p = self.p();
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(width, size::ROW_TREE),
            Sense::focusable_noninteractive(),
        );
        resp.widget_info(|| WidgetInfo::labeled(Role::MenuItem, false, text));
        ui.ctx()
            .accesskit_node_builder(resp.id, |n| n.set_disabled());
        let painter = ui.painter();
        self.art.icon(
            painter,
            Rect::from_min_size(
                egui::pos2(
                    rect.left() + size::MENU_MARK_INDENT - size::UNIT - size::MENU_MARK,
                    rect.top(),
                ),
                egui::vec2(size::MENU_MARK, rect.height()),
            ),
            Icon::Info,
            p.dim,
        );
        let x = rect.left() + size::MENU_MARK_INDENT;
        let (g, cut) = self.fit(
            painter,
            text,
            Type::Body,
            p.dim,
            rect.right() - size::UNIT * 2.0 - x,
        );
        painter.galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, p.dim);
        if cut { resp.on_hover_text(text) } else { resp }
    }

    /// A banner: a full-width line with a level icon, a sentence or two, and
    /// the one button that moves on.
    pub fn banner(
        &self,
        ui: &mut Ui,
        level: Level,
        text: &str,
        button: Option<Button<'_>>,
    ) -> Option<Response> {
        let p = self.p();
        let frame = egui::Frame::NONE
            .fill(p.raised)
            .stroke(self.theme.frame(level.color(&p)))
            .inner_margin(egui::Margin::same(size::UNIT as i8 * 2));
        frame
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(size::ICON, size::ICON), Sense::hover());
                    self.art
                        .icon(ui.painter(), r, level.icon(), level.color(&p));
                    self.text(ui, text, Type::Body, p.text);
                    button.map(|b| self.button(ui, b.small()))
                })
                .inner
            })
            .inner
    }

    /// An empty state: an icon, one sentence, at most two buttons, centred
    /// in `rect`. Returns the buttons' responses.
    pub fn empty_state(
        &self,
        ui: &mut Ui,
        rect: Rect,
        icon: Icon,
        text: &str,
        buttons: &[Button<'_>],
    ) -> Vec<Response> {
        let p = self.p();
        let mut out = Vec::new();
        let w = size::EMPTY_TEXT_W.min(rect.width());
        let inner = Rect::from_center_size(rect.center(), egui::vec2(w, rect.height()));
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Center)),
            |ui| {
                let used = size::ICON
                    + size::UNIT * 2.0
                    + crate::theme::text::BODY_LINE * 2.0
                    + if buttons.is_empty() {
                        0.0
                    } else {
                        size::CONTROL_H + size::STEP_12
                    };
                ui.add_space(((inner.height() - used) / 2.0).max(0.0));
                let (r, _) =
                    ui.allocate_exact_size(egui::vec2(size::ICON, size::ICON), Sense::hover());
                self.art.icon(ui.painter(), r, icon, p.dim);
                ui.add_space(size::UNIT * 2.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(text)
                            .font(self.theme.font(Type::Body))
                            .color(p.dim),
                    )
                    .wrap()
                    .halign(egui::Align::Center),
                );
                if !buttons.is_empty() {
                    ui.add_space(size::STEP_12);
                    ui.horizontal(|ui| {
                        let total: f32 = buttons
                            .iter()
                            .map(|b| self.caps_width(ui, b.text) + size::STEP_24)
                            .sum();
                        ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
                        for b in buttons {
                            out.push(self.button(ui, *b));
                        }
                    });
                }
            },
        );
        out
    }

    /// A divider: an 8 px strip that can be dragged, and that takes focus.
    /// Arrow keys move it 8 px; Enter puts it back.
    pub fn divider(&self, ui: &mut Ui, id: Id, rect: Rect, across: bool, name: &str) -> DividerOut {
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        resp.widget_info(|| WidgetInfo::labeled(Role::Splitter, true, name));
        let mut out = DividerOut::default();
        let d = resp.drag_delta();
        out.delta = if across { d.x } else { d.y };
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(if across {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::ResizeVertical
            });
        }
        if resp.has_focus() {
            let (less, more) = if across {
                (Key::ArrowLeft, Key::ArrowRight)
            } else {
                (Key::ArrowUp, Key::ArrowDown)
            };
            ui.input(|i| {
                if i.key_pressed(less) {
                    out.delta -= size::DIVIDER;
                }
                if i.key_pressed(more) {
                    out.delta += size::DIVIDER;
                }
                out.reset = i.key_pressed(Key::Enter);
            });
            let p = self.p();
            ui.painter().rect_filled(rect, CornerRadius::ZERO, p.select);
            self.focus_ring(ui.painter(), rect);
        }
        out
    }

    /// A region of the window: a node of its own in the accessibility tree,
    /// with a role and a name, that can take focus.
    pub fn region(
        &self,
        ui: &mut Ui,
        rect: Rect,
        name: &str,
        role: Role,
        fill: Color32,
        focus_now: bool,
    ) -> Ui {
        let child = ui.new_child(
            UiBuilder::new()
                .id_salt(("region", name))
                .max_rect(rect)
                .sense(Sense::focusable_noninteractive())
                .accessibility_label(name)
                .accessibility_role(role),
        );
        child.painter().rect_filled(rect, CornerRadius::ZERO, fill);
        if focus_now {
            let id = child.unique_id();
            child.memory_mut(|m| m.request_focus(id));
        }
        child
    }

    /// The width one footer segment wants for `text`, uncut.
    pub fn status_segment_width(&self, ui: &Ui, icon: Option<Icon>, text: &str) -> f32 {
        let g = ui.painter().layout_no_wrap(
            text.to_owned(),
            self.theme.font(Type::Small),
            self.p().text,
        );
        Self::status_segment_chrome(icon) + g.size().x
    }

    /// The padding and icon width of a segment, around its text.
    fn status_segment_chrome(icon: Option<Icon>) -> f32 {
        let icon_w = if icon.is_some() {
            size::ICON + size::UNIT
        } else {
            0.0
        };
        size::FOOTER_SEG_PAD * 2.0 + icon_w
    }

    /// Paint one segment of the footer in `r`: padding and a 16 px line in
    /// `frame` at its left. Text that does not fit is cut with an ellipsis
    /// and shown whole in a tooltip.
    pub fn status_segment(&self, ui: &mut Ui, r: Rect, icon: Option<Icon>, text: &str) {
        let p = self.p();
        let painter = ui.painter().clone();
        let pad = size::FOOTER_SEG_PAD;
        let icon_w = Self::status_segment_chrome(icon) - pad * 2.0;
        let (g, cut) = self.fit(
            &painter,
            text,
            Type::Small,
            p.text,
            r.width() - Self::status_segment_chrome(icon),
        );
        self.footer_divider(&painter, r.left(), r.center().y);
        if let Some(i) = icon {
            self.art.icon(
                &painter,
                Rect::from_min_size(
                    egui::pos2(r.left() + pad, r.top()),
                    egui::vec2(size::ICON, r.height()),
                ),
                i,
                p.bright,
            );
        }
        let y = r.center().y - g.size().y / 2.0;
        painter.galley(egui::pos2(r.left() + pad + icon_w, y), g, p.text);
        // In the accessibility tree too: an invisible label over the segment.
        let resp = ui.place(
            r,
            egui::Label::new(
                egui::RichText::new(text)
                    .color(Color32::TRANSPARENT)
                    .font(self.theme.font(Type::Small)),
            ),
        );
        if cut {
            resp.on_hover_text(text);
        }
    }

    /// A 16 px line in `frame`, centred on `y`.
    pub fn footer_divider(&self, painter: &egui::Painter, x: f32, y: f32) {
        let h = size::FOOTER_DIVIDER_H / 2.0;
        painter.vline(
            x,
            egui::Rangef::new(y - h, y + h),
            self.theme.frame(self.p().frame),
        );
    }
}
