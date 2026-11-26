// SPDX-License-Identifier: GPL-3.0-or-later

//! The controls of the theme editor: a slider with a plain or a mesh rail,
//! the swatch group and a checkbox.

use super::Kit;
use crate::art::Icon;
use crate::theme::{Type, size, stroke};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, EventFilter, Key, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, WidgetInfo,
    accesskit::{Action, ActionData, Role},
};

/// What a slider's rail shows.
#[derive(Debug, Clone, Copy)]
pub enum Rail<'r> {
    /// A plain rail, filled in `bright` up to the handle.
    Fill,
    /// A rail of colours, spread evenly from its left end to its right.
    Mesh(&'r [Color32]),
}

/// A slider to draw: a name, a rail and a number box on one row.
#[derive(Debug, Clone)]
pub struct Slider<'r> {
    /// Its name, at the row's left and for a screen reader.
    pub name: &'r str,
    /// The low and the high end.
    pub range: (f64, f64),
    /// One step, for an arrow key.
    pub step: f64,
    /// The decimals of the number box.
    pub decimals: usize,
    /// Left of the low end is the high end, as on a hue.
    pub wrap: bool,
    /// What the rail shows.
    pub rail: Rail<'r>,
    /// The width of the name's column.
    pub name_w: f32,
    /// The value as a screen reader says it, or None for the number.
    pub value_text: Option<String>,
    /// Why the slider is held, with its saved number, or None when it moves.
    pub held: Option<String>,
}

/// The text a field holds apart from its value, kept while the field has
/// focus. None when the field does not hold any.
pub fn held_text(ui: &Ui, id: egui::Id) -> Option<String> {
    ui.data(|d| d.get_temp::<String>(id.with(HELD)))
}

/// After a field that keeps typed text apart from its value is drawn: keep
/// `text` while the field has focus, and give it back once, in the first
/// frame the field is drawn without focus, for the caller to commit. This
/// does not depend on what took the focus or when: a control drawn before
/// or after the field, a click on empty space, Tab, Shift+Tab or Escape.
/// Call it only while the field takes input, so a field held by a
/// disabled pane keeps its text for later.
pub fn hold_text(ui: &Ui, id: egui::Id, focused: bool, text: &str) -> Option<String> {
    let key = id.with(HELD);
    if focused {
        ui.data_mut(|d| d.insert_temp(key, text.to_owned()));
        None
    } else {
        ui.data_mut(|d| d.remove_temp::<String>(key))
    }
}

const HELD: &str = "held text";

/// What moved a slider in a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovedBy {
    /// It did not move.
    Nothing,
    /// Its number box committed typed text, and nothing came after.
    Text,
    /// The rail, a key or an action moved it, after any typed text.
    Act,
}

/// One act on a slider's rail, applied after the number box.
#[derive(Debug, Clone, Copy)]
enum Move {
    /// To this value.
    To(f64),
    /// This many steps.
    By(f64),
}

/// Steps for Shift with an arrow, and for Page Up and Page Down.
const BIG: f64 = 10.0;

impl Slider<'_> {
    fn number(&self, v: f64) -> String {
        format!("{v:.*}", self.decimals)
    }

    /// `v` on a step, inside the range or wrapped into it.
    fn settle(&self, v: f64) -> f64 {
        let (lo, hi) = self.range;
        let v = lo + ((v - lo) / self.step).round() * self.step;
        if self.wrap {
            let span = hi - lo + self.step;
            lo + (v - lo).rem_euclid(span)
        } else {
            v.clamp(lo, hi)
        }
    }
}

impl Kit<'_> {
    /// A slider row: its name, a rail with a handle and a number box. The
    /// arrows move it a step, with Shift ten, Page Up and Page Down ten,
    /// Home and End to its ends. Enter goes to the number box, which takes
    /// a typed number, and back. A held slider has a dashed rail, no
    /// handle and its saved number in `dim`; it does not move, and its
    /// tooltip and description say why. Returns the rail's response,
    /// changed when the value moved.
    pub fn slider(&self, ui: &mut Ui, s: &Slider<'_>, value: &mut f64) -> Response {
        self.slider_by(ui, s, value).0
    }

    /// [`Kit::slider`], and what moved it this frame. In a frame where
    /// the number box loses focus and the rail is used, the typed text
    /// commits first and the rail's act comes last, so the act wins.
    pub fn slider_by(&self, ui: &mut Ui, s: &Slider<'_>, value: &mut f64) -> (Response, MovedBy) {
        let p = self.p();
        let held = s.held.is_some();
        let w = ui.available_width();
        let (row, _) = ui.allocate_exact_size(egui::vec2(w, size::SLIDER_ROW), Sense::hover());
        let id = ui.scope_id().with(("slider", s.name));
        let nid = id.with("number");
        let num = Rect::from_center_size(
            egui::pos2(row.right() - size::NUMBER_W / 2.0, row.center().y),
            egui::vec2(size::NUMBER_W, size::SMALL_H),
        );
        let rail_area = Rect::from_min_max(
            egui::pos2(row.left() + s.name_w, row.top()),
            egui::pos2(
                (num.left() - size::GAP_ACROSS).max(row.left() + s.name_w),
                row.bottom(),
            ),
        );
        let sense = if held {
            Sense::focusable_noninteractive()
        } else {
            Sense::drag()
        };
        let resp = ui.interact(rail_area, id, sense);
        let before = *value;
        let able = ui.is_enabled() && !held;
        let travel = rail_area.x_range().shrink(size::HANDLE_W / 2.0);
        let (lo, hi) = s.range;
        // What the person did on the rail this frame. It applies after the
        // number box commits its text, so the last act wins.
        let mut acts: Vec<Move> = Vec::new();
        if able {
            if let Some(at) = resp.interact_pointer_pos() {
                let t = f64::from(((at.x - travel.min) / travel.span().max(1.0)).clamp(0.0, 1.0));
                acts.push(Move::To(lo + t * (hi - lo)));
            }
            if resp.has_focus() {
                ui.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        id,
                        EventFilter {
                            horizontal_arrows: true,
                            ..Default::default()
                        },
                    );
                });
                let (left, right, up, down, home, end, enter, shift) = ui.input(|i| {
                    (
                        i.num_presses(Key::ArrowLeft) as f64,
                        i.num_presses(Key::ArrowRight) as f64,
                        i.num_presses(Key::PageUp) as f64,
                        i.num_presses(Key::PageDown) as f64,
                        i.key_pressed(Key::Home),
                        i.key_pressed(Key::End),
                        i.key_pressed(Key::Enter),
                        i.modifiers.shift,
                    )
                });
                let by = if shift { BIG } else { 1.0 };
                let steps = (right - left) * by + (up - down) * BIG;
                if steps != 0.0 {
                    acts.push(Move::By(steps));
                }
                if home {
                    acts.push(Move::To(lo));
                }
                if end {
                    acts.push(Move::To(hi));
                }
                if enter {
                    ui.memory_mut(|m| m.request_focus(nid));
                }
            }
            let (inc, dec, set) = ui.input(|i| {
                (
                    i.num_accesskit_action_requests(id, Action::Increment) as f64,
                    i.num_accesskit_action_requests(id, Action::Decrement) as f64,
                    i.accesskit_action_requests(id, Action::SetValue)
                        .filter_map(|r| match r.data {
                            Some(ActionData::NumericValue(v)) => Some(v),
                            _ => None,
                        })
                        .last(),
                )
            });
            if inc != dec {
                acts.push(Move::By(inc - dec));
            }
            if let Some(v) = set {
                acts.push(Move::To(v));
            }
        }

        // The number box, before the rail is drawn: its text commits
        // first, then the acts, whatever the order of focus.
        let cr = self.theme.control_radius();
        let painter = ui.painter().clone();
        painter.rect_filled(num, cr, p.ground);
        let mut typed_moved = false;
        if held || !ui.is_enabled() {
            painter.text(
                egui::pos2(num.right() - size::UNIT * 2.0, num.center().y),
                Align2::RIGHT_CENTER,
                s.number(*value),
                self.theme.font(Type::Body),
                if held { p.dim } else { p.text },
            );
            if held {
                self.dashed_rect(&painter, num, p.frame);
            } else {
                painter.rect_stroke(num, cr, self.theme.frame(p.frame), StrokeKind::Inside);
            }
        } else {
            // The text typed stays until the frame focus leaves, which
            // commits it, whatever took the focus.
            let mut text = held_text(ui, nid).unwrap_or_else(|| s.number(*value));
            let inner = num.shrink2(egui::vec2(size::UNIT, 0.0));
            let r = ui.place(
                inner,
                egui::TextEdit::singleline(&mut text)
                    .id(nid)
                    .frame(egui::Frame::NONE)
                    .font(self.theme.font(Type::Body))
                    .text_color(p.text)
                    .align(Align2::RIGHT_CENTER)
                    .desired_width(inner.width()),
            );
            ui.ctx()
                .accesskit_node_builder(nid, |n| n.set_label(s.name.to_owned()));
            let focused = r.has_focus() && acts.is_empty();
            if r.has_focus() && !focused {
                // An act on the rail while the box has focus ends the
                // typing: the text commits now, before the act.
                ui.memory_mut(|m| m.surrender_focus(nid));
            }
            if let Some(typed) = hold_text(ui, nid, focused, &text) {
                if let Ok(n) = typed.trim().parse::<f64>()
                    && n.is_finite()
                {
                    let was = *value;
                    *value = s.settle(n.clamp(lo, hi));
                    typed_moved = *value != was;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    ui.memory_mut(|m| m.request_focus(id));
                }
            }
            let (line, sw) = if focused {
                (p.bright, stroke::FRAME * 2.0)
            } else {
                (p.frame, stroke::FRAME)
            };
            painter.rect_stroke(num, cr, Stroke::new(sw, line), StrokeKind::Inside);
        }
        let after_text = *value;
        for m in &acts {
            *value = match *m {
                Move::To(v) => s.settle(v.clamp(lo, hi)),
                Move::By(n) => s.settle(*value + n * s.step),
            };
        }
        let acted = !acts.is_empty() && *value != after_text;
        let v = *value;
        resp.widget_info(|| WidgetInfo::slider(!held, v, s.name));
        let text = s.value_text.clone().unwrap_or_else(|| s.number(v));
        ui.ctx().accesskit_node_builder(id, |n| {
            n.set_min_numeric_value(lo);
            n.set_max_numeric_value(hi);
            n.set_numeric_value_step(s.step);
            n.set_value(text);
            if held {
                n.set_disabled();
            } else {
                n.add_action(Action::SetValue);
                n.add_action(Action::Increment);
                n.add_action(Action::Decrement);
            }
            if let Some(why) = &s.held {
                n.set_description(why.clone());
            }
        });

        let fg = if held { p.dim } else { p.text };
        painter.text(
            egui::pos2(row.left(), row.center().y),
            Align2::LEFT_CENTER,
            s.name,
            self.theme.font(Type::Body),
            fg,
        );
        let rail = Rect::from_center_size(
            rail_area.center(),
            egui::vec2(rail_area.width(), size::RAIL_H),
        );
        let t = if hi > lo {
            ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let hx = travel.min + travel.span() * t;
        if held {
            self.dashed_rect(&painter, rail, p.frame);
        } else {
            match s.rail {
                Rail::Fill => {
                    painter.rect_filled(rail, cr, p.ground);
                    let filled = Rect::from_min_max(rail.min, egui::pos2(hx, rail.max.y));
                    painter.rect_filled(filled, cr, p.bright);
                }
                Rail::Mesh(colours) => mesh_rail(&painter, rail, colours),
            }
            painter.rect_stroke(rail, cr, self.theme.frame(p.frame), StrokeKind::Inside);
            let handle = Rect::from_center_size(
                egui::pos2(hx, rail.center().y),
                egui::vec2(size::HANDLE_W, size::RAIL_H + size::UNIT * 2.0),
            );
            let line = if resp.hovered() || resp.dragged() || resp.has_focus() {
                p.bright
            } else {
                p.text
            };
            painter.rect(
                handle,
                cr,
                p.raised,
                Stroke::new(stroke::FRAME, line),
                StrokeKind::Inside,
            );
        }
        if resp.has_focus() {
            self.focus_ring(&painter, rail);
        }

        let mut resp = resp;
        if *value != before {
            resp.mark_changed();
        }
        let by = if acted {
            MovedBy::Act
        } else if typed_moved && *value != before {
            MovedBy::Text
        } else {
            MovedBy::Nothing
        };
        let resp = match &s.held {
            Some(why) => resp.on_hover_text(why.as_str()),
            None => resp,
        };
        (resp, by)
    }

    /// A row of colour swatches, one radio group named `name` and one stop
    /// in the Tab order. The chosen one has a 2 px frame in `bright` and a
    /// tick. Left and Right move to the next swatch and pick it, Home and
    /// End to the first and the last. Each radio is its name, its value
    /// the colour. Returns the swatch picked this frame.
    pub fn swatches(
        &self,
        ui: &mut Ui,
        name: &str,
        items: &[(&str, Color32)],
        chosen: Option<usize>,
    ) -> Option<usize> {
        let p = self.p();
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, size::SWATCH), Sense::hover());
        let list = ui.new_child(
            UiBuilder::new()
                .id_salt(("swatches", name))
                .max_rect(rect)
                .sense(Sense::hover())
                .accessibility_label(name)
                .accessibility_role(Role::RadioGroup),
        );
        list.ctx().accesskit_node_builder(list.unique_id(), |n| {
            n.set_bounds(egui::accesskit::Rect {
                x0: rect.min.x.into(),
                y0: rect.min.y.into(),
                x1: rect.max.x.into(),
                y1: rect.max.y.into(),
            });
        });
        let n = items.len();
        if n == 0 {
            return None;
        }
        let gap = size::STEP_6;
        let sw = ((w - gap * (n - 1) as f32) / n as f32).max(size::SWATCH);
        let stop = chosen.unwrap_or(0);
        let painter = list.painter().clone();
        let cr = self.theme.control_radius();
        let mut out = None;
        for (i, (label, colour)) in items.iter().enumerate() {
            let r = Rect::from_min_size(
                egui::pos2(rect.left() + (sw + gap) * i as f32, rect.top()),
                egui::vec2(sw, size::SWATCH),
            );
            let sid = list.unique_id().with(i);
            let sense = if i == stop {
                Sense::click()
            } else {
                Sense::CLICK
            };
            let resp = list.interact(r, sid, sense);
            if i == stop
                && list.data_mut(|d| d.remove_temp::<bool>(sid.with("focus"))) == Some(true)
            {
                list.memory_mut(|m| m.request_focus(sid));
            }
            let on = chosen == Some(i);
            resp.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, on, *label));
            let hex = crate::palette::hex_text(*colour);
            list.ctx().accesskit_node_builder(sid, |b| b.set_value(hex));
            painter.rect_filled(r, cr, *colour);
            if on {
                painter.rect_stroke(
                    r,
                    cr,
                    Stroke::new(stroke::FRAME * 2.0, p.bright),
                    StrokeKind::Outside,
                );
                self.art.icon(&painter, r, Icon::Ok, p.ink);
            } else {
                let line = if resp.hovered() { p.bright } else { p.frame };
                painter.rect_stroke(r, cr, self.theme.frame(line), StrokeKind::Inside);
            }
            if resp.has_focus() {
                self.focus_ring(&painter, r);
            }
            if resp.clicked() {
                out = Some(i);
            }
            if resp.has_focus() {
                list.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        sid,
                        EventFilter {
                            horizontal_arrows: true,
                            ..Default::default()
                        },
                    );
                });
                let (left, right, home, end) = list.input(|k| {
                    (
                        k.key_pressed(Key::ArrowLeft),
                        k.key_pressed(Key::ArrowRight),
                        k.key_pressed(Key::Home),
                        k.key_pressed(Key::End),
                    )
                });
                let to = if home {
                    Some(0)
                } else if end {
                    Some(n - 1)
                } else if left {
                    Some(if i == 0 { n - 1 } else { i - 1 })
                } else if right {
                    Some(if i + 1 == n { 0 } else { i + 1 })
                } else {
                    None
                };
                if let Some(to) = to {
                    let next = list.unique_id().with(to).with("focus");
                    list.data_mut(|d| d.insert_temp(next, true));
                    out = Some(to);
                }
            }
        }
        out
    }

    /// A checkbox row: a box with a tick when on, and its text. A held
    /// checkbox is `dim`, takes no click and says why. Returns its
    /// response: clicked when it was toggled.
    pub fn checkbox(&self, ui: &mut Ui, text: &str, on: bool, held: Option<&str>) -> Response {
        let p = self.p();
        let w = ui.available_width();
        let sense = if held.is_some() {
            Sense::focusable_noninteractive()
        } else {
            Sense::click()
        };
        let (row, resp) = ui.allocate_exact_size(egui::vec2(w, size::SLIDER_ROW), sense);
        resp.widget_info(|| WidgetInfo::selected(Role::CheckBox, held.is_none(), on, text));
        if let Some(why) = held {
            ui.ctx().accesskit_node_builder(resp.id, |n| {
                n.set_disabled();
                n.set_description(why.to_owned());
            });
        }
        let painter = ui.painter();
        let b = Rect::from_center_size(
            egui::pos2(row.left() + size::CHECK_BOX / 2.0, row.center().y),
            egui::vec2(size::CHECK_BOX, size::CHECK_BOX),
        );
        let cr = self.theme.control_radius();
        let fg = if held.is_some() { p.dim } else { p.text };
        if held.is_some() {
            self.dashed_rect(painter, b, p.frame);
        } else {
            let fill = if on { p.bright } else { p.ground };
            let line = if resp.hovered() { p.bright } else { p.frame };
            painter.rect(b, cr, fill, self.theme.frame(line), StrokeKind::Inside);
        }
        if on {
            let tick = if held.is_some() { p.dim } else { p.ink };
            self.art.icon(painter, b, Icon::Ok, tick);
        }
        painter.text(
            egui::pos2(b.right() + size::GAP_ACROSS, row.center().y),
            Align2::LEFT_CENTER,
            text,
            self.theme.font(Type::Body),
            fg,
        );
        if resp.has_focus() {
            self.focus_ring(painter, b);
        }
        match held {
            Some(why) => resp.on_hover_text(why),
            None => resp,
        }
    }
}

/// A rail of `colours`, spread evenly, as one mesh with a colour at each
/// vertex: two triangles a stop.
fn mesh_rail(painter: &egui::Painter, rect: Rect, colours: &[Color32]) {
    let n = colours.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        painter.rect_filled(rect, CornerRadius::ZERO, colours[0]);
        return;
    }
    let mut mesh = egui::Mesh::default();
    for (i, c) in colours.iter().enumerate() {
        let x = rect.left() + rect.width() * i as f32 / (n - 1) as f32;
        mesh.colored_vertex(egui::pos2(x, rect.top()), *c);
        mesh.colored_vertex(egui::pos2(x, rect.bottom()), *c);
        if i > 0 {
            let k = (i * 2) as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k, k + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}
