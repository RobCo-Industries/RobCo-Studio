//! Spike K7: a plain text editor on egui's own `TextEdit`, with a layouter for
//! C colours and a gutter painted for the visible rows only.

pub mod c_lex;
pub mod file;

use eframe::egui;
use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Color32, FontId, TextStyle};
use std::path::PathBuf;

pub const TEXT_ID: &str = "k7-text";

pub struct Editor {
    pub text: String,
    /// The text as last opened or saved, to compute the dirty mark.
    saved: String,
    pub crlf: bool,
    pub path: String,
    pub message: String,
    pub query: String,
    pub goto: String,
    /// A selection, in characters, to apply before the next draw, then scroll to.
    pub pending_sel: Option<(usize, usize)>,
    /// Benchmark and tests: put this line at the top of the view before drawing.
    pub scroll_to_line: Option<usize>,
    /// Line numbers, from 1, at the top of the view on the last frame:
    /// the one the gutter drew, and the one the text galley has there.
    pub first_gutter_line: usize,
    pub first_text_line: usize,
    /// Compute `first_text_line`. It scans the text, so it is off by default.
    pub check_lines: bool,
    pub cursor: Option<CCursorRange>,
    /// How many times the layouter ran the tokenizer, and how many times it was called.
    pub highlight_runs: u64,
    pub layouter_calls: u64,
    hl_cache: Option<(String, LayoutJob)>,
    /// Last frame's galley, and its top relative to the scrolled content.
    last_galley: Option<(std::sync::Arc<egui::Galley>, f32)>,
    sel_focus_asked: bool,
}

impl Editor {
    pub fn new(text: String) -> Self {
        Self {
            saved: text.clone(),
            text,
            crlf: false,
            path: String::new(),
            message: String::new(),
            query: String::new(),
            goto: String::new(),
            pending_sel: None,
            scroll_to_line: None,
            first_gutter_line: 1,
            first_text_line: 1,
            check_lines: false,
            cursor: None,
            highlight_runs: 0,
            layouter_calls: 0,
            hl_cache: None,
            last_galley: None,
            sel_focus_asked: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.text != self.saved
    }

    pub fn open(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        match std::fs::read(&path) {
            Err(e) => self.message = format!("Not opened: {e}."),
            Ok(bytes) => match file::decode(&bytes) {
                Err(e) => self.message = e.to_string(),
                Ok((text, crlf)) => {
                    self.text = text.clone();
                    self.saved = text;
                    self.crlf = crlf;
                    self.path = path.display().to_string();
                    self.message = format!("Opened, {}.", if crlf { "CRLF" } else { "LF" });
                }
            },
        }
    }

    pub fn save(&mut self) {
        let bytes = file::encode(&self.text, self.crlf);
        match std::fs::write(&self.path, bytes) {
            Ok(()) => {
                self.saved = self.text.clone();
                self.message = "Saved.".into();
            }
            Err(e) => self.message = format!("Not saved: {e}."),
        }
    }

    fn char_of_byte(&self, byte: usize) -> usize {
        self.text[..byte].chars().count()
    }

    fn byte_of_char(&self, ch: usize) -> usize {
        self.text
            .char_indices()
            .nth(ch)
            .map_or(self.text.len(), |(b, _)| b)
    }

    /// Select the next match after the cursor, wrapping at the end.
    pub fn find_next(&mut self) {
        if self.query.is_empty() {
            return;
        }
        let from = self.cursor.map_or(0, |c| {
            self.byte_of_char(c.primary.index.max(c.secondary.index).0)
        });
        let hit = self.text[from..]
            .find(&self.query)
            .map(|i| i + from)
            .or_else(|| self.text.find(&self.query));
        self.select_hit(hit);
    }

    /// Select the match before the cursor, wrapping at the start.
    pub fn find_prev(&mut self) {
        if self.query.is_empty() {
            return;
        }
        let to = self.cursor.map_or(self.text.len(), |c| {
            self.byte_of_char(c.primary.index.min(c.secondary.index).0)
        });
        let hit = self.text[..to]
            .rfind(&self.query)
            .or_else(|| self.text.rfind(&self.query));
        self.select_hit(hit);
    }

    fn select_hit(&mut self, hit: Option<usize>) {
        match hit {
            Some(b) => {
                let s = self.char_of_byte(b);
                let n = self.query.chars().count();
                self.pending_sel = Some((s, s + n));
                self.message.clear();
            }
            None => self.message = "Not found.".into(),
        }
    }

    /// Put the cursor at the start of line `line`, counted from 1.
    pub fn goto_line(&mut self, line: usize) {
        let total = self.text.split('\n').count();
        if line == 0 || line > total {
            self.message = format!("No line {line}: the file has {total}.");
            return;
        }
        let byte = if line == 1 {
            0
        } else {
            self.text
                .match_indices('\n')
                .nth(line - 2)
                .map_or(0, |(i, _)| i + 1)
        };
        let c = self.char_of_byte(byte);
        self.pending_sel = Some((c, c));
        self.message.clear();
    }

    fn highlight(&mut self, ui: &egui::Ui, text: &str) -> LayoutJob {
        if let Some((t, job)) = &self.hl_cache
            && t == text
        {
            return job.clone();
        }
        self.highlight_runs += 1;
        let font = TextStyle::Monospace.resolve(ui.style());
        let dark = ui.visuals().dark_mode;
        let mut job = LayoutJob::default();
        for (r, k) in c_lex::spans(text) {
            job.append(
                &text[r],
                0.0,
                TextFormat::simple(font.clone(), color(k, dark)),
            );
        }
        job.keep_trailing_whitespace = true;
        self.hl_cache = Some((text.to_owned(), job.clone()));
        job
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let l = ui.label("File").id;
            ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(220.0))
                .labelled_by(l);
            if ui.button("Open").clicked() {
                let p = self.path.clone();
                self.open(p);
            }
            if ui.button("Save").clicked() {
                self.save();
            }
            ui.label(if self.is_dirty() { "modified" } else { "saved" });
            ui.separator();
            let l = ui.label("Find").id;
            ui.add(egui::TextEdit::singleline(&mut self.query).desired_width(120.0))
                .labelled_by(l);
            if ui.button("Previous").clicked() {
                self.find_prev();
            }
            if ui.button("Next").clicked() {
                self.find_next();
            }
            ui.separator();
            let l = ui.label("Line").id;
            ui.add(egui::TextEdit::singleline(&mut self.goto).desired_width(60.0))
                .labelled_by(l);
            if ui.button("Go").clicked() {
                match self.goto.trim().parse() {
                    Ok(n) => self.goto_line(n),
                    Err(_) => self.message = "Not a line number.".into(),
                }
            }
        });
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        let l = ui.label("C source").id;
        self.editor(ui, l);
    }

    fn editor(&mut self, ui: &mut egui::Ui, label: egui::Id) {
        let id = egui::Id::unique(TEXT_ID);
        let font: FontId = TextStyle::Monospace.resolve(ui.style());
        let row_h = ui.fonts_mut(|f| f.row_height(&font));
        let lines = self.text.split('\n').count();
        let gutter_w = ui.fonts_mut(|f| f.glyph_width(&font, '0')) * (digits(lines) as f32 + 1.0);

        // egui 0.37 collapses a selection on a text field that does not have focus.
        // A button press moves focus too, one frame late. So focus is requested
        // first, and the selection is applied on a later frame that has focus.
        let focused = ui.memory(|m| m.has_focus(id));
        let scroll_sel = match self.pending_sel {
            Some(_) if !focused || !self.sel_focus_asked => {
                self.sel_focus_asked = true;
                ui.memory_mut(|m| m.request_focus(id));
                ui.ctx().request_repaint();
                None
            }
            Some((a, b)) => {
                self.pending_sel = None;
                self.sel_focus_asked = false;
                let mut st = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
                st.cursor
                    .set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
                st.store(ui.ctx(), id);
                Some(a)
            }
            None => None,
        };

        let mut area = egui::ScrollArea::both()
            .id_salt("k7-scroll")
            .auto_shrink(false);
        if let Some(line) = self.scroll_to_line.take() {
            // The row's own position in last frame's galley, so no rounding drifts.
            let y = match &self.last_galley {
                Some((g, origin)) => origin + g.rows.get(line).map_or(0.0, |r| r.pos.y),
                None => row_h * line as f32,
            };
            area = area.vertical_scroll_offset(y);
        }

        // Moved out so the layouter closure can borrow `self` mutably.
        let mut text = std::mem::take(&mut self.text);
        area.show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (gutter, _) =
                    ui.allocate_exact_size(egui::vec2(gutter_w, row_h), egui::Sense::hover());
                let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                    self.layouter_calls += 1;
                    let mut job = self.highlight(ui, buf.as_str());
                    job.wrap.max_width = wrap;
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let out = egui::TextEdit::multiline(&mut text)
                    .id(id)
                    .code_editor()
                    .lock_focus(true)
                    .desired_width(f32::INFINITY)
                    .desired_rows(1)
                    .frame(egui::Frame::NONE)
                    .layouter(&mut layouter)
                    .show(ui);
                out.response.response.clone().labelled_by(label);
                self.cursor = out.state.cursor.char_range();
                self.last_galley = Some((out.galley.clone(), out.galley_pos.y - gutter.top()));

                if let Some(c) = scroll_sel {
                    let r = out.galley.pos_from_cursor(CCursor::new(c));
                    ui.scroll_to_rect(
                        r.translate(out.galley_pos.to_vec2()),
                        Some(egui::Align::Center),
                    );
                }

                // Gutter: one number per visible row, at the row's own position.
                // With no wrapping, row i is line i + 1.
                let clip = ui.clip_rect();
                let rows = &out.galley.rows;
                let top = clip.top() - out.galley_pos.y;
                let first = rows.partition_point(|r| r.pos.y + r.rect().height() <= top);
                let painter = ui.painter();
                let col = ui.visuals().weak_text_color();
                self.first_gutter_line = first + 1;
                for (i, r) in rows.iter().enumerate().skip(first) {
                    let y = out.galley_pos.y + r.pos.y;
                    if y > clip.bottom() {
                        break;
                    }
                    painter.text(
                        egui::pos2(gutter.right() - row_h * 0.3, y),
                        egui::Align2::RIGHT_TOP,
                        (i + 1).to_string(),
                        font.clone(),
                        col,
                    );
                }
                if self.check_lines {
                    // Independent of the gutter: the character at the top-left of the
                    // view, mapped back to a line by counting newlines in the text.
                    let c = out
                        .galley
                        .cursor_from_pos(egui::vec2(0.0, top.max(0.0) + 1.0));
                    let b = text
                        .char_indices()
                        .nth(c.index.0)
                        .map_or(text.len(), |(b, _)| b);
                    self.first_text_line = text[..b].matches('\n').count() + 1;
                }
            });
        });
        self.text = text;
    }
}

fn digits(n: usize) -> usize {
    n.max(1).ilog10() as usize + 1
}

fn color(k: c_lex::Kind, dark: bool) -> Color32 {
    use c_lex::Kind::*;
    match (k, dark) {
        (Plain, true) => Color32::from_gray(210),
        (Plain, false) => Color32::from_gray(30),
        (Keyword, _) => Color32::from_rgb(200, 120, 40),
        (Comment, _) => Color32::from_rgb(100, 140, 100),
        (Str, _) => Color32::from_rgb(170, 90, 160),
        (Number, _) => Color32::from_rgb(60, 140, 200),
        (Preproc, _) => Color32::from_rgb(160, 150, 60),
    }
}

/// A C source of `lines` lines, the same on every call. It has every kind of
/// token, tabs, and a long comment block.
pub fn sample_c(lines: usize) -> String {
    let mut s = String::new();
    let mut n = 0;
    let mut f = 0;
    let add = |s: &mut String, l: &str| {
        s.push_str(l);
        s.push('\n');
    };
    while n < lines {
        let block = [
            "#include <stdint.h>".to_string(),
            format!("/* Function {f}: adds and scales."),
            "   A comment that spans lines. */".to_string(),
            format!("static int32_t fn_{f}(int32_t a, const char *s)"),
            "{".to_string(),
            "\tint32_t x = a * 0x1F + 42; // a number and a comment".to_string(),
            "\tif (s != NULL && s[0] == 'k') {".to_string(),
            format!("\t\tx += {f}.5e-3 > 1.0 ? 1 : 0;"),
            "\t\tputs(\"string with \\\"escapes\\\" and tabs\\t\");".to_string(),
            "\t}".to_string(),
            "\treturn x;".to_string(),
            "}".to_string(),
            String::new(),
        ];
        for l in block {
            if n == lines {
                break;
            }
            add(&mut s, &l);
            n += 1;
        }
        f += 1;
    }
    s
}
