// SPDX-License-Identifier: GPL-3.0-or-later

//! The theme editor: a pane at the right of the page with a Colour tab and
//! a Screen tab. Every change goes through [`Studio::edit_theme`], so the
//! window behind, its preview, changes in the next frame.

use super::{Studio, ThemeFault, page_area};
use crate::art::Icon;
use crate::editor::Answer;
use crate::kit::{Button, ButtonKind, Kit, Level, Rail, Slider};
use crate::palette::{self, QUICK};
use crate::screen::Held;
use crate::theme::{Strengths, Type, crt, size, stroke};
use eframe::egui::{self, Color32, Id, Key, Rect, Sense, UiBuilder, WidgetInfo, accesskit::Role};

/// The two tabs of the theme editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeTab {
    /// The base colour.
    #[default]
    Colour,
    /// The ten strengths, the switch and the frame rate.
    Screen,
}

/// The theme editor's state, kept from frame to frame.
#[derive(Debug, Default)]
pub struct ThemeEditor {
    /// The editor is open.
    pub open: bool,
    /// The tab shown.
    pub tab: ThemeTab,
    /// The base colour field's text.
    hex: String,
    /// The base colour the field's text was last set from or gave.
    hex_for: Option<Color32>,
    /// Hue in degrees, saturation and lightness in percent: the sliders
    /// keep their own numbers while they move.
    sliders: [f64; 3],
    /// The base colour the sliders were last set from or gave.
    sliders_for: Option<Color32>,
    /// The saturation and lightness rails, and the sliders they were made
    /// for. Made again only when the colour changes.
    rails: Option<([f64; 3], [[Color32; 2]; 2])>,
    /// The editor drew last frame. On its first frame the Theme dropdown
    /// takes the focus.
    was_open: bool,
    /// The name row, open in place of the buttons.
    pub naming: Option<Naming>,
    /// The question about a theme, in front of the editor.
    pub question: Option<ThemeQuestion>,
}

/// The name row of Save as and Rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Naming {
    /// Rename, else Save as.
    pub rename: bool,
    /// The name as typed.
    pub text: String,
    /// The field takes the focus on its next frame.
    focus: bool,
}

/// A question about a theme. Each comes in front of the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeQuestion {
    /// Keep the changes before the theme `to`, by its settings key, takes
    /// their place? `name` is the Name field, for a theme that ships.
    Leave {
        /// The theme picked.
        to: String,
        /// The name to save a theme that ships as.
        name: String,
    },
    /// Delete the theme in use?
    Delete,
}

impl ThemeEditor {
    /// Follow a base colour that changed elsewhere: from the menu, Revert,
    /// the quick colours or the other control.
    fn follow(&mut self, base: Color32) {
        if self.hex_for != Some(base) {
            self.hex = palette::hex_text(base);
            self.hex_for = Some(base);
        }
        if self.sliders_for != Some(base) {
            let [h, s, l] = palette::to_sliders(base);
            self.sliders = [h.round() % 360.0, (s * 100.0).round(), (l * 100.0).round()];
            self.sliders_for = Some(base);
        }
    }

    /// The saturation and lightness rails for the sliders now.
    fn rails(&mut self) -> [[Color32; 2]; 2] {
        let k = self.sliders;
        match self.rails {
            Some((at, r)) if at == k => r,
            _ => {
                let r = palette::short_rails(k[0], k[1] / 100.0, k[2] / 100.0);
                self.rails = Some((k, r));
                r
            }
        }
    }

    /// The field's text, as typed.
    pub fn hex(&self) -> &str {
        &self.hex
    }
}

/// The tag of a theme that ships, unchanged.
pub const TAG_SHIPS: &str = "Ships with Studio";
/// The tag of a person's own theme, unchanged.
pub const TAG_YOURS: &str = "Yours";
/// The tag of a changed theme.
pub const TAG_CHANGED: &str = "Not saved";
/// The tab list's name.
pub const TABS: &str = "Theme parts";
/// The two tabs.
pub const TAB_NAMES: [&str; 2] = ["Colour", "Screen"];
/// The base colour field.
pub const BASE: &str = "Base colour";
/// The field's message while its text is not a colour.
pub const NOT_A_COLOUR: &str = "A colour is 3 or 6 hex digits, such as 5aff7f.";
/// The quick colours' group.
pub const QUICK_NAME: &str = "Quick colours";
/// The three colour sliders.
pub const COLOUR_SLIDERS: [&str; 3] = ["Hue", "Saturation", "Lightness"];
/// The head of the three samples.
pub const SIGNALS: &str = "Signals";
/// The samples' words.
pub const SAMPLES: [&str; 3] = ["Error", "Warning", "\"string\""];
/// What a screen reader calls the samples' colours.
const SAMPLE_NAMES: [&str; 3] = ["Error colour", "Warning colour", "String colour"];
/// The hint under the samples.
pub const HINT: &str =
    "The window's other colours follow this one. Text stays readable whatever you pick.";
/// The master switch for motion.
pub const MOVES: &str = "The screen moves";
/// The frame rate choice.
pub const RATE_LABEL: &str = "Frames a second";
/// The note of a theme past the themes that ship.
pub const STRONGER: &str = "Stronger than the themes that ship. Text is harder to read.";
/// Why Save waits on a theme that ships.
pub const SHIPPED_SAVE: &str =
    "A theme that ships with Studio cannot be changed. Save as keeps yours.";
/// The buttons, left to right.
pub const BUTTONS: [&str; 5] = ["Delete", "Rename", "Revert", "Save as", "Save"];
/// The close button's name.
pub const CLOSE: &str = "Close";
/// The name row's field.
pub const NAME: &str = "Name";
/// The question's buttons.
pub const CANCEL: &str = "Cancel";
/// Drop the changes and go on.
pub const DO_NOT_SAVE: &str = "Do not save";
/// Save over a theme of the same name.
pub const REPLACE: &str = "Replace";
/// The Delete question's act.
pub const DELETE_THEME: &str = "Delete theme";

/// The title of the question on leaving a changed theme.
pub fn leave_title(name: &str) -> String {
    format!("Keep the changes to {name}?")
}

/// The text of the question on leaving a changed theme.
pub fn leave_text(name: &str, to: &str) -> String {
    format!("{name} was changed and not saved. {to} takes its place now.")
}

/// The title of the Delete question.
pub fn delete_title(name: &str) -> String {
    format!("Delete the theme {name}?")
}

/// The text of the Delete question.
pub fn delete_text(file: &str) -> String {
    format!(
        "Studio deletes {file} from the themes folder. This cannot be undone. The window goes back to Green."
    )
}

/// The line under the field when the rule raised the colour.
pub fn raised_news(c: Color32) -> String {
    format!("Raised to {} to stay readable.", palette::hex_text(c))
}

/// What a screen reader says of the hue: "49 degrees, orange".
pub fn hue_text(h: f64) -> String {
    format!("{h:.0} degrees, {}", palette::hue_word(h))
}

/// What a screen reader says of saturation or lightness. At 0 lightness
/// says it is the deepest colour that stays readable.
pub fn percent_text(v: f64, lightness: bool) -> String {
    if lightness && v <= 0.0 {
        format!("{v:.0} percent, the deepest that stays readable")
    } else {
        format!("{v:.0} percent")
    }
}

/// Why strength `i` is held, and its saved number, or None when it moves.
/// High legibility holds all ten; reduce motion and a screen that does not
/// move hold the four that move; no graphics adapter holds all but Bloom
/// and Scanlines.
pub fn held_reason(
    i: usize,
    saved: f32,
    moves: bool,
    reduce: bool,
    held: &[Held],
) -> Option<String> {
    let moving = Strengths::MOVING.contains(&i);
    let why = if held.contains(&Held::Legible) {
        "high legibility is on"
    } else if reduce && moving {
        "reduce motion is on"
    } else if !moves && moving {
        "the screen does not move"
    } else if held.contains(&Held::NoAdapter) && !Strengths::WITHOUT_PASS.contains(&i) {
        "this machine has no graphics adapter"
    } else {
        return None;
    };
    Some(format!("Held at 0 while {why}. Saved: {saved:.2}."))
}

/// Why the switch "The screen moves" is held, or None.
pub fn moves_held(moves: bool, reduce: bool) -> Option<String> {
    reduce.then(|| {
        let saved = if moves { "on" } else { "off" };
        format!("Held while reduce motion is on. Saved: {saved}.")
    })
}

/// What the pane asks of the theme this frame.
#[derive(Default)]
struct Asked {
    base: Option<Color32>,
    strengths: Option<Strengths>,
    moves: Option<bool>,
    rate: Option<u32>,
    pick: Option<usize>,
    revert: bool,
    close: bool,
    save: bool,
    save_as: bool,
    rename: bool,
    delete: bool,
    /// The name row's Save, Replace or Rename.
    named: bool,
    /// The name row's Cancel, or Escape in it.
    unnamed: bool,
}

impl Studio {
    /// Where the theme editor stands: the right of the page, as wide as the
    /// Simulator pane, from the page's top to its bottom.
    pub fn theme_pane_rect(&self) -> Rect {
        let area = page_area(self.page_rect);
        Rect::from_min_max(
            egui::pos2((area.right() - size::SIM_W).max(area.left()), area.top()),
            area.right_bottom(),
        )
    }

    /// Draw the theme editor. `enabled` is false under the folder chooser,
    /// which holds it as it is.
    pub(super) fn theme_pane(&mut self, kit: &Kit<'_>, ctx: &egui::Context, enabled: bool) {
        let rect = self.theme_pane_rect();
        let base = self.spec().base;
        self.theme_editor.follow(base);
        // Escape leaves the name row first, then the editor.
        let escape = enabled && ctx.input(|i| i.key_pressed(Key::Escape));
        let naming = self.theme_editor.naming.is_some();
        let mut asked = Asked {
            close: escape && !naming,
            unnamed: escape && naming,
            ..Asked::default()
        };
        let own = self.own_file().is_some();
        let folder = self.themes_folder().is_some();
        let no_folder = ThemeFault::NoFolder.message();
        let why_own = if !folder {
            Some(no_folder.as_str())
        } else if !own {
            Some(SHIPPED_SAVE)
        } else {
            None
        };
        let why_as = (!folder).then_some(no_folder.as_str());
        let fault = self.name_fault();
        let refused: Vec<(String, String)> = self
            .refused
            .iter()
            .map(|(f, why)| (crate::store::shown(f), why.clone()))
            .collect();
        let name = self.theme_name();
        let changed = self.changed.is_some();
        let tag = if changed {
            TAG_CHANGED
        } else if self
            .theme_files
            .get(self.current)
            .is_some_and(Option::is_some)
        {
            TAG_YOURS
        } else {
            TAG_SHIPS
        };
        let rows: Vec<String> = self.themes.iter().map(|t| t.name.to_string()).collect();
        let spec = self.spec().clone();
        let shown = self.shown();
        let reduce = self.screen.reduce_motion;
        let first_frame = !self.theme_editor.was_open;
        self.theme_editor.was_open = true;
        let p = kit.p();
        let mut dropdown = None;
        egui::Area::new(Id::unique("theme-editor"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                if !enabled {
                    ui.disable();
                }
                kit.pane_as(
                    ui,
                    rect,
                    crate::app::THEME_TITLE,
                    Role::Dialog,
                    true,
                    false,
                    |ui, pr| {
                        // The title row: the tag, then the close button.
                        let row = pr.row;
                        let close = Rect::from_center_size(
                            egui::pos2(
                                row.right() - size::UNIT - size::SMALL_H / 2.0,
                                row.center().y,
                            ),
                            egui::vec2(size::SMALL_H, size::SMALL_H),
                        );
                        ui.scope_builder(UiBuilder::new().max_rect(close), |ui| {
                            if kit.icon_button(ui, Icon::Close, CLOSE, None).clicked() {
                                asked.close = true;
                            }
                        });
                        let painter = ui.painter().clone();
                        let font = kit.theme.font(Type::Small);
                        let g = painter.layout_no_wrap(tag.to_owned(), font, p.text);
                        let tx = close.left() - size::GAP_ACROSS - g.size().x;
                        painter.galley(
                            egui::pos2(tx, row.center().y - g.size().y / 2.0),
                            g,
                            p.text,
                        );
                        if changed {
                            painter.rect_filled(
                                Rect::from_center_size(
                                    egui::pos2(tx - size::STEP_6 * 2.0, row.center().y),
                                    egui::vec2(size::STEP_6, size::STEP_6),
                                ),
                                egui::CornerRadius::ZERO,
                                p.bright,
                            );
                        }

                        let c = pr.content;
                        let foot = Rect::from_min_max(
                            egui::pos2(c.left(), c.bottom() - size::SMALL_H),
                            c.right_bottom(),
                        );
                        // The name row's fault, in a line over the row.
                        let said = fault.as_ref().map(|f| {
                            let r = Rect::from_min_max(
                                egui::pos2(c.left(), foot.top() - size::GAP_DOWN - size::ROW_FACT),
                                egui::pos2(c.right(), foot.top() - size::GAP_DOWN),
                            );
                            (r, f.message())
                        });
                        let body_end = said.as_ref().map_or(foot.top(), |(r, _)| r.top());
                        let body = Rect::from_min_max(
                            c.min,
                            egui::pos2(c.right(), body_end - size::GAP_DOWN),
                        );
                        let mut ui = ui.new_child(
                            UiBuilder::new()
                                .max_rect(body)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                        );
                        ui.spacing_mut().item_spacing.y = size::UNIT;
                        let row_names: Vec<&str> = rows.iter().map(String::as_str).collect();
                        let held: Vec<(&str, &str)> = refused
                            .iter()
                            .map(|(f, why)| (f.as_str(), why.as_str()))
                            .collect();
                        let dd = kit.dropdown_held(
                            &mut ui,
                            crate::app::THEME_TITLE,
                            Icon::Settings,
                            &name,
                            false,
                            body.width(),
                            &row_names,
                            &held,
                        );
                        dropdown = Some(dd.0.clone());
                        if let Some(i) = dd.1 {
                            asked.pick = Some(i);
                        }
                        if first_frame {
                            dd.0.request_focus();
                        }
                        ui.add_space(size::UNIT);
                        let at = match self.theme_editor.tab {
                            ThemeTab::Colour => 0,
                            ThemeTab::Screen => 1,
                        };
                        if let Some(k) =
                            kit.segmented_tabs_wide(&mut ui, TABS, &TAB_NAMES, at, body.width())
                        {
                            self.theme_editor.tab = if k == 0 {
                                ThemeTab::Colour
                            } else {
                                ThemeTab::Screen
                            };
                        }
                        ui.add_space(size::UNIT);
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(&mut ui, |ui| {
                                ui.spacing_mut().item_spacing.y = size::UNIT;
                                match self.theme_editor.tab {
                                    ThemeTab::Colour => {
                                        self.colour_tab(kit, ui, &spec, &mut asked);
                                    }
                                    ThemeTab::Screen => {
                                        screen_tab(kit, ui, &spec, &shown.held, reduce, &mut asked);
                                    }
                                }
                                notes(kit, ui, &spec, &shown.held);
                            });

                        if let Some((r, text)) = &said {
                            let mut ui = ui.new_child(
                                UiBuilder::new()
                                    .max_rect(*r)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            note(kit, &mut ui, Icon::Refused, text, p.alarm, None);
                        }
                        let mut ui = ui.new_child(
                            UiBuilder::new()
                                .max_rect(foot)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        if let Some(n) = &mut self.theme_editor.naming {
                            // The name row: the field, then Cancel and the act.
                            name_row(kit, &mut ui, n, fault.as_ref(), &mut asked);
                            return;
                        }
                        // The buttons: Delete and Rename at the left,
                        // Revert, Save as and Save at the right.
                        let button = |ui: &mut egui::Ui, text: &str, why: Option<&str>, filled| {
                            let mut b = Button::quiet(text).small();
                            if filled {
                                b = b.kind(ButtonKind::Filled);
                            }
                            if let Some(w) = why {
                                b = b.unavailable(w);
                            }
                            kit.button(ui, b).clicked()
                        };
                        asked.delete = button(&mut ui, BUTTONS[0], why_own, false);
                        asked.rename = button(&mut ui, BUTTONS[1], why_own, false);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            asked.save = button(ui, BUTTONS[4], why_own, true);
                            asked.save_as = button(ui, BUTTONS[3], why_as, false);
                            asked.revert = button(ui, BUTTONS[2], None, false);
                        });
                    },
                );
            });
        // Focus stays in the pane: when it is elsewhere, the Theme dropdown
        // takes it.
        let inside = ctx
            .memory(|m| m.focused())
            .and_then(|f| ctx.read_response(f))
            .is_some_and(|r| rect.contains(r.rect.center()));
        if enabled
            && !inside
            && !first_frame
            && let Some(r) = dropdown
        {
            r.request_focus();
        }
        if !enabled {
            return;
        }
        self.apply(ctx, asked);
    }

    /// The Colour tab: the field, the raise, the quick colours, the three
    /// sliders, the samples and the hint.
    fn colour_tab(
        &mut self,
        kit: &Kit<'_>,
        ui: &mut egui::Ui,
        spec: &crate::theme::Spec,
        asked: &mut Asked,
    ) {
        let p = kit.p();
        let ed = &mut self.theme_editor;
        kit.text(ui, BASE, Type::Body, p.text);
        let w = ui.available_width();
        let refused = palette::parse_hex(&ed.hex).is_none();
        let bright = palette::bright_of(spec.base);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = size::GAP_ACROSS;
            let (r, _) =
                ui.allocate_exact_size(egui::vec2(size::SWATCH, size::FIELD_H), Sense::hover());
            ui.painter()
                .rect_filled(r, kit.theme.control_radius(), spec.base);
            let id = Id::unique("theme-base-colour");
            let resp = kit.text_field(
                ui,
                id,
                &mut ed.hex,
                BASE,
                "",
                w - size::SWATCH - size::GAP_ACROSS,
                refused,
            );
            let about = if refused {
                Some(NOT_A_COLOUR.to_owned())
            } else if bright.raised {
                Some(raised_news(bright.rgb))
            } else {
                None
            };
            if let Some(a) = about {
                ui.ctx()
                    .accesskit_node_builder(id, |n| n.set_description(a));
            }
            let parsed = palette::parse_hex(&ed.hex);
            if resp.changed()
                && let Some((c, 6)) = parsed
            {
                ed.hex_for = Some(c);
                asked.base = Some(c);
            }
            if resp.lost_focus() {
                if let Some((c, 3)) = parsed {
                    ed.hex_for = Some(c);
                    asked.base = Some(c);
                }
                // Enter keeps the caret in the field.
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    resp.request_focus();
                }
            }
        });
        if refused {
            note(kit, ui, Icon::Refused, NOT_A_COLOUR, p.alarm, None);
        } else if bright.raised {
            note(
                kit,
                ui,
                Icon::Info,
                &raised_news(bright.rgb),
                p.text,
                Some(bright.rgb),
            );
        }

        kit.text(ui, QUICK_NAME, Type::Body, p.text);
        let chosen = QUICK.iter().position(|(_, c)| *c == spec.base);
        if let Some(i) = kit.swatches(ui, QUICK_NAME, &QUICK, chosen)
            && let Some((_, c)) = QUICK.get(i)
        {
            asked.base = Some(*c);
        }

        let rails = ed.rails();
        let mut moved = false;
        for (i, name) in COLOUR_SLIDERS.iter().enumerate() {
            let v = ed.sliders[i];
            let (range, wrap, rail, text) = match i {
                0 => (
                    (0.0, 359.0),
                    true,
                    Rail::Mesh(palette::hue_rail()),
                    hue_text(v),
                ),
                1 => (
                    (0.0, 100.0),
                    false,
                    Rail::Mesh(&rails[0]),
                    percent_text(v, false),
                ),
                _ => (
                    (0.0, 100.0),
                    false,
                    Rail::Mesh(&rails[1]),
                    percent_text(v, true),
                ),
            };
            let s = Slider {
                name,
                range,
                step: 1.0,
                decimals: 0,
                wrap,
                rail,
                name_w: size::COLOUR_NAME_W,
                value_text: Some(text),
                held: None,
            };
            if kit.slider(ui, &s, &mut ed.sliders[i]).changed() {
                moved = true;
            }
        }
        if moved {
            let [h, s, l] = ed.sliders;
            let c = palette::from_sliders(h, s / 100.0, l / 100.0);
            ed.sliders_for = Some(c);
            ed.hex = palette::hex_text(c);
            ed.hex_for = Some(c);
            asked.base = Some(c);
        }

        kit.text(ui, SIGNALS, Type::Body, p.text);
        let (r, _) = ui.allocate_exact_size(egui::vec2(w, size::FIELD_H), Sense::hover());
        let painter = ui.painter().clone();
        painter.rect_stroke(
            r,
            kit.theme.control_radius(),
            kit.theme.frame(p.frame),
            egui::StrokeKind::Inside,
        );
        let mut x = r.left() + size::UNIT * 2.0;
        let icons = [Some(Icon::Error), Some(Icon::Warn), None];
        for (i, word) in SAMPLES.iter().enumerate() {
            let colour = [p.alarm, p.warn, p.lit][i];
            let start = x;
            if let Some(icon) = icons[i] {
                let ir =
                    Rect::from_min_size(egui::pos2(x, r.top()), egui::vec2(size::ICON, r.height()));
                kit.art.icon(&painter, ir, icon, colour);
                x += size::ICON + size::UNIT;
            }
            let g = painter.layout_no_wrap((*word).to_owned(), kit.theme.font(Type::Body), colour);
            let gw = g.size().x;
            painter.galley(egui::pos2(x, r.center().y - g.size().y / 2.0), g, colour);
            x += gw;
            let sr = Rect::from_min_max(egui::pos2(start, r.top()), egui::pos2(x, r.bottom()));
            let said = format!("{} {}", SAMPLE_NAMES[i], palette::hex_text(colour));
            let resp = ui.interact(sr, Id::unique("theme-sample").with(i), Sense::hover());
            resp.widget_info(|| WidgetInfo::labeled(Role::Label, true, &said));
            x += size::STEP_16;
        }
        let hint = egui::Label::new(
            egui::RichText::new(HINT)
                .font(kit.theme.font(Type::Body))
                .color(p.dim),
        )
        .wrap();
        ui.add(hint);
    }

    /// Change the theme as the pane asked, through `edit_theme`.
    fn apply(&mut self, ctx: &egui::Context, a: Asked) {
        let before = self.spec().clone();
        if a.unnamed {
            self.theme_editor.naming = None;
        } else if a.named {
            self.finish_naming();
        } else if a.save_as || a.rename {
            self.start_naming(a.rename);
        } else if a.save {
            if let Err(f) = self.save_theme() {
                self.say(Level::Error, f.message());
            }
        } else if a.delete {
            if self.own_file().is_some() {
                self.theme_editor.question = Some(ThemeQuestion::Delete);
            }
        } else if let Some(i) = a.pick {
            self.ask_pick(i);
        } else if a.revert {
            if self.changed.is_some() {
                self.pick_theme(self.current);
            }
        } else {
            self.edit_theme(|t| {
                if let Some(c) = a.base {
                    t.base = c;
                }
                if let Some(s) = a.strengths {
                    t.strengths = s;
                }
                if let Some(m) = a.moves {
                    t.moves = m;
                }
                if let Some(r) = a.rate {
                    t.rate = r;
                }
            });
        }
        // The pass runs at what drew this frame: a change applies from the
        // next frame, so ask for it.
        if *self.spec() != before {
            ctx.request_repaint();
        }
        if a.close {
            self.theme_editor.open = false;
        }
        if !self.theme_editor.open {
            self.theme_editor.was_open = false;
            self.theme_editor.naming = None;
        }
    }

    /// The file of the theme in use, when it is one of the person's own.
    pub fn own_file(&self) -> Option<String> {
        self.theme_files.get(self.current).cloned().flatten()
    }

    /// Take the theme at place `i`, from the menu or the editor's
    /// dropdown. While a changed theme is not saved, ask first.
    pub fn ask_pick(&mut self, i: usize) {
        if i >= self.themes.len() {
            return;
        }
        if self.changed.is_none() {
            self.pick_theme(i);
            return;
        }
        let name = if self.own_file().is_some() {
            String::new()
        } else {
            self.next_name(&self.own_name(self.current))
        };
        self.theme_editor.naming = None;
        self.theme_editor.question = Some(ThemeQuestion::Leave {
            to: self.key(i),
            name,
        });
    }

    /// Open the name row of Save as, with the next free name, or of
    /// Rename, with the current name.
    pub fn start_naming(&mut self, rename: bool) {
        if rename && self.own_file().is_none() {
            return;
        }
        let own = self.own_name(self.current);
        let text = if rename { own } else { self.next_name(&own) };
        self.theme_editor.naming = Some(Naming {
            rename,
            text,
            focus: true,
        });
    }

    /// What is wrong with the name in the name row, or in the question's
    /// Name field. [`ThemeFault::Exists`] offers Replace on Save as.
    pub fn name_fault(&self) -> Option<ThemeFault> {
        if let Some(n) = &self.theme_editor.naming {
            let except = n.rename.then_some(self.current);
            return self.check_name(&n.text, except).err();
        }
        match &self.theme_editor.question {
            Some(ThemeQuestion::Leave { name, .. }) if self.own_file().is_none() => {
                self.check_name(name, None).err()
            }
            _ => None,
        }
    }

    /// Save as or Rename with the name in the name row. The row closes
    /// when it worked. A name that exists is replaced on Save as, since the
    /// button read Replace.
    pub fn finish_naming(&mut self) {
        let Some(n) = self.theme_editor.naming.clone() else {
            return;
        };
        let done = if n.rename {
            self.rename_theme(&n.text)
        } else {
            let replace = matches!(self.check_name(&n.text, None), Err(ThemeFault::Exists(_)));
            self.save_theme_as(&n.text, replace)
        };
        match done {
            Ok(_) => self.theme_editor.naming = None,
            Err(f) => self.say_fault(&f),
        }
    }

    /// Say a fault that the name row does not show.
    fn say_fault(&mut self, f: &ThemeFault) {
        if matches!(
            f,
            ThemeFault::Write(_)
                | ThemeFault::NoFolder
                | ThemeFault::NotTheme(_)
                | ThemeFault::ShippedTheme
        ) {
            self.say(Level::Error, f.message());
        }
    }

    /// Answer the question about a theme. Cancel keeps all as it was.
    /// On leaving a changed theme, Do not save drops the changes and Save
    /// keeps them first, then the theme picked takes their place. On
    /// Delete, Save is Delete theme.
    pub fn answer_theme(&mut self, a: Answer) {
        let Some(q) = self.theme_editor.question.clone() else {
            return;
        };
        if a == Answer::Cancel {
            self.theme_editor.question = None;
            return;
        }
        match q {
            ThemeQuestion::Delete => {
                self.theme_editor.question = None;
                if a == Answer::Save
                    && let Err(f) = self.delete_theme()
                {
                    self.say(Level::Error, f.message());
                }
            }
            ThemeQuestion::Leave { to, name } => {
                if a == Answer::Save {
                    let kept = if self.own_file().is_some() {
                        self.save_theme()
                    } else {
                        let replace =
                            matches!(self.check_name(&name, None), Err(ThemeFault::Exists(_)));
                        self.save_theme_as(&name, replace)
                    };
                    if let Err(f) = kept {
                        self.say_fault(&f);
                        return;
                    }
                }
                self.theme_editor.question = None;
                if let Some(i) = self.find_key(&to) {
                    self.pick_theme(i);
                }
            }
        }
    }

    /// The question about a theme, under the scrim, in front of the
    /// editor. Escape cancels it.
    pub(super) fn theme_dialog(&mut self, kit: &Kit<'_>, ctx: &egui::Context, full: Rect) {
        let Some(q) = self.theme_editor.question.clone() else {
            return;
        };
        let p = kit.p();
        let own = self.own_name(self.current);
        let (title, text, danger) = match &q {
            ThemeQuestion::Delete => {
                let file = self.own_file().unwrap_or_default();
                (
                    delete_title(&own),
                    delete_text(&crate::store::shown(&file)),
                    true,
                )
            }
            ThemeQuestion::Leave { to, .. } => {
                let to = self
                    .find_key(to)
                    .and_then(|i| self.themes.get(i))
                    .map_or_else(String::new, |t| t.name.to_string());
                (leave_title(&own), leave_text(&own, &to), false)
            }
        };
        let field = matches!(q, ThemeQuestion::Leave { .. }) && self.own_file().is_none();
        let fault = if field { self.name_fault() } else { None };
        let extra = if field {
            size::FIELD_H + size::ROW_FACT + size::UNIT * 2.0
        } else {
            0.0
        };
        let rect = Rect::from_center_size(
            full.center(),
            egui::vec2(size::DIALOG_W, size::DIALOG_H + size::ROW_FACT + extra),
        );
        let line = if danger { p.alarm } else { p.bright };
        let mut answer = None;
        egui::Area::new(Id::unique("theme-question"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .accessible_name(title.clone())
            .show(ctx, |ui| {
                ui.set_min_size(rect.size());
                if let Some(h) = kit.theme.halo() {
                    ui.painter().add(h.as_shape(rect, egui::CornerRadius::ZERO));
                }
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::ZERO, p.pane);
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::ZERO,
                    kit.theme.frame(line),
                    egui::StrokeKind::Inside,
                );
                let inner = rect.shrink(size::PAD_SIDE + size::UNIT);
                let mut ui = ui.new_child(
                    UiBuilder::new()
                        .max_rect(inner)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                ui.spacing_mut().item_spacing.y = size::UNIT;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = size::UNIT * 2.0;
                    let (r, _) = ui.allocate_exact_size(
                        egui::vec2(size::ICON, size::CONTROL_H),
                        Sense::hover(),
                    );
                    let icon = if danger { Icon::Warn } else { Icon::Save };
                    kit.art.icon(ui.painter(), r, icon, line);
                    kit.text(ui, &title, Type::BodyBold, p.text);
                });
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&text)
                            .font(kit.theme.font(Type::Body))
                            .color(p.text),
                    )
                    .wrap(),
                );
                if field
                    && let Some(ThemeQuestion::Leave { name, .. }) = &mut self.theme_editor.question
                {
                    kit.text_field(
                        &mut ui,
                        Id::unique("theme-question-name"),
                        name,
                        NAME,
                        "",
                        inner.width(),
                        fault.is_some(),
                    );
                    if let Some(f) = &fault {
                        note(kit, &mut ui, Icon::Refused, &f.message(), p.alarm, None);
                    }
                }
                let row = Rect::from_min_max(
                    egui::pos2(inner.left(), inner.bottom() - size::CONTROL_H),
                    inner.right_bottom(),
                );
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(row)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    |ui| {
                        let (act, why) = match &q {
                            ThemeQuestion::Delete => (DELETE_THEME, None),
                            ThemeQuestion::Leave { .. } if !field => (BUTTONS[4], None),
                            ThemeQuestion::Leave { .. } => match &fault {
                                None => (BUTTONS[3], None),
                                Some(ThemeFault::Exists(_)) => (REPLACE, None),
                                Some(f) => (BUTTONS[3], Some(f.message())),
                            },
                        };
                        let mut b = Button::quiet(act).kind(ButtonKind::Filled);
                        if let Some(w) = &why {
                            b = b.unavailable(w);
                        }
                        if kit.button(ui, b).clicked() {
                            answer = Some(Answer::Save);
                        }
                        let cancel = kit.button(ui, Button::quiet(CANCEL));
                        if cancel.clicked() {
                            answer = Some(Answer::Cancel);
                        }
                        let inside = ui
                            .memory(|m| m.focused())
                            .and_then(|f| ui.ctx().read_response(f))
                            .is_some_and(|r| rect.contains(r.rect.center()));
                        if !inside {
                            cancel.request_focus();
                        }
                        if matches!(q, ThemeQuestion::Leave { .. })
                            && kit.button(ui, Button::quiet(DO_NOT_SAVE)).clicked()
                        {
                            answer = Some(Answer::DoNotSave);
                        }
                    },
                );
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            answer = Some(Answer::Cancel);
        }
        if let Some(a) = answer {
            self.answer_theme(a);
        }
    }
}

/// The name row: the Name field, then Cancel and Save, Replace or Rename.
/// Enter acts as the last button; Escape is taken by the pane.
fn name_row(
    kit: &Kit<'_>,
    ui: &mut egui::Ui,
    n: &mut Naming,
    fault: Option<&ThemeFault>,
    asked: &mut Asked,
) {
    let (act, why) = match fault {
        None if n.rename => (BUTTONS[1], None),
        None => (BUTTONS[4], None),
        Some(ThemeFault::Exists(_)) if !n.rename => (REPLACE, None),
        Some(f) => (
            if n.rename { BUTTONS[1] } else { BUTTONS[4] },
            Some(f.message()),
        ),
    };
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let mut b = Button::quiet(act).small().kind(ButtonKind::Filled);
        if let Some(w) = &why {
            b = b.unavailable(w);
        }
        if kit.button(ui, b).clicked() {
            asked.named = true;
        }
        if kit.button(ui, Button::quiet(CANCEL).small()).clicked() {
            asked.unnamed = true;
        }
        let id = Id::unique("theme-name-row");
        let w = ui.available_width();
        let resp = kit.text_field(ui, id, &mut n.text, NAME, "", w, fault.is_some());
        if n.focus {
            resp.request_focus();
            n.focus = false;
        }
        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && why.is_none() {
            asked.named = true;
        }
    });
}

/// The Screen tab: the ten strengths, the switch and the frame rate.
fn screen_tab(
    kit: &Kit<'_>,
    ui: &mut egui::Ui,
    spec: &crate::theme::Spec,
    held: &[Held],
    reduce: bool,
    asked: &mut Asked,
) {
    let p = kit.p();
    let mut s = spec.strengths;
    for (i, name) in Strengths::NAMES.iter().enumerate() {
        let Some(saved) = s.get(i) else {
            continue;
        };
        let slider = Slider {
            name,
            range: (0.0, 1.0),
            step: crt::SLIDER_STEP,
            decimals: 2,
            wrap: false,
            rail: Rail::Fill,
            name_w: size::STRENGTH_NAME_W,
            value_text: None,
            held: held_reason(i, saved, spec.moves, reduce, held),
        };
        let mut v = f64::from(saved);
        if kit.slider(ui, &slider, &mut v).changed()
            && let Some(x) = s.get_mut(i)
        {
            *x = v as f32;
            asked.strengths = Some(s);
        }
    }
    let why = moves_held(spec.moves, reduce);
    if kit
        .checkbox(ui, MOVES, spec.moves, why.as_deref())
        .clicked()
    {
        asked.moves = Some(!spec.moves);
    }
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(size::STRENGTH_NAME_W, size::SLIDER_ROW),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(size::STRENGTH_NAME_W);
                kit.text(ui, RATE_LABEL, Type::Body, p.text);
            },
        );
        let parts: Vec<String> = crt::RATES.iter().map(u32::to_string).collect();
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        let at = crt::RATES.iter().position(|r| *r == spec.rate).unwrap_or(1);
        if let Some(k) = kit.segmented(ui, RATE_LABEL, &parts, at)
            && let Some(r) = crt::RATES.get(k)
        {
            asked.rate = Some(*r);
        }
    });
}

/// The notes under the controls: one for each thing that holds the theme,
/// and one when the theme is stronger than the themes that ship.
fn notes(kit: &Kit<'_>, ui: &mut egui::Ui, spec: &crate::theme::Spec, held: &[Held]) {
    let p = kit.p();
    for h in held {
        note(
            kit,
            ui,
            Icon::Info,
            crate::theme::held_note(*h),
            p.text,
            None,
        );
    }
    if spec.strengths.stronger() {
        note(kit, ui, Icon::Warn, STRONGER, p.warn, None);
    }
}

/// One line with an icon, and a swatch of `swatch` after the icon. It is
/// a status for a screen reader.
fn note(
    kit: &Kit<'_>,
    ui: &mut egui::Ui,
    icon: Icon,
    text: &str,
    colour: Color32,
    swatch: Option<Color32>,
) {
    let p = kit.p();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = size::UNIT;
        let (r, _) = ui.allocate_exact_size(egui::vec2(size::ICON, size::ROW_FACT), Sense::hover());
        kit.art.icon(ui.painter(), r, icon, colour);
        if let Some(c) = swatch {
            let (r, _) =
                ui.allocate_exact_size(egui::vec2(size::STEP_12, size::ROW_FACT), Sense::hover());
            let b = Rect::from_center_size(r.center(), egui::vec2(size::STEP_12, size::STEP_12));
            ui.painter().rect_filled(b, egui::CornerRadius::ZERO, c);
            ui.painter().rect_stroke(
                b,
                egui::CornerRadius::ZERO,
                egui::Stroke::new(stroke::FRAME, p.frame),
                egui::StrokeKind::Inside,
            );
        }
        let r = kit.text(ui, text, Type::Body, colour);
        ui.ctx()
            .accesskit_node_builder(r.id, |n| n.set_live(egui::accesskit::Live::Polite));
    });
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test; a failed unwrap is a failed test"
)]
mod tests;
