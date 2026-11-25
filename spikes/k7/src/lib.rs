//! Spike K7: a plain text editor on egui's own `TextEdit`, with a layouter for
//! C colours and a gutter painted for the visible rows only.

pub mod c_lex;
pub mod file;

use eframe::egui;
use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Color32, FontId, TextStyle};
use std::path::PathBuf;

pub const TEXT_ID: &str = "k7-text";

/// What an "unsaved edits" question would do once the user agrees.
#[derive(Clone, Debug, PartialEq)]
pub enum Replace {
    Open(PathBuf),
    Close,
}

/// The one question the editor can have pending. Each variant carries all its
/// answer needs: the target, and the id of the document it was asked about.
#[derive(Clone, Debug, PartialEq)]
pub enum Question {
    /// The buffer has unsaved edits and `then` would drop them.
    Unsaved { doc: u64, then: Replace },
    /// Save as would replace the existing file `path`.
    Overwrite { doc: u64, path: PathBuf },
}

impl Question {
    pub fn doc(&self) -> u64 {
        match self {
            Question::Unsaved { doc, .. } | Question::Overwrite { doc, .. } => *doc,
        }
    }
}

/// An answer. Save, Discard and Cancel answer `Unsaved`; Overwrite and Keep
/// answer `Overwrite`. An answer to the other kind of question is ignored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Answer {
    /// Save the edits, then go on. If the save fails, nothing else happens.
    Save,
    /// Drop the edits and go on.
    Discard,
    /// Do nothing.
    Cancel,
    /// Write over the existing file.
    Overwrite,
    /// Leave the existing file alone.
    Keep,
}

/// Everything that belongs to one document. A load replaces it whole, so no
/// state of the old document can reach the new one.
pub struct Doc {
    /// One more than the document before it. The text widget and scroll area
    /// ids carry it, so egui's undo history, cursor and scroll start fresh, and
    /// a question names the document it was asked about by it.
    pub id: u64,
    pub text: String,
    /// The text as last opened or saved, to compute the dirty mark.
    saved: String,
    pub crlf: bool,
    /// The document's own file, the only place Save writes. None for a new buffer.
    pub path: Option<PathBuf>,
    pub cursor: Option<CCursorRange>,
    /// A selection, in characters, to apply before the next draw, then scroll to.
    pub pending_sel: Option<(usize, usize)>,
    sel_focus_asked: bool,
    /// Benchmark and tests: put this line at the top of the view before drawing.
    pub scroll_to_line: Option<usize>,
    /// Last frame's galley, and its top relative to the scrolled content.
    last_galley: Option<(std::sync::Arc<egui::Galley>, f32)>,
}

impl Doc {
    fn new(id: u64, text: String, crlf: bool, path: Option<PathBuf>) -> Self {
        Self {
            id,
            saved: text.clone(),
            text,
            crlf,
            path,
            cursor: None,
            pending_sel: None,
            sel_focus_asked: false,
            scroll_to_line: None,
            last_galley: None,
        }
    }
}

/// The editor model. Its fields, sorted by what they belong to:
///
/// - The document: `doc`, a `Doc` replaced whole on load. Text, saved text,
///   line ending, path, id, cursor, pending selection and scroll, last galley.
/// - The user's input, kept across loads on purpose: `path` (the "File"
///   field), `save_as_path`, `query`, `goto`.
/// - The pending question: `question`, one value at most. While it is set the
///   model is modal: Open, Save, Save as and a close request are refused, and
///   typing is locked out. Search and go to line are allowed: they only set a
///   selection, which waits until the question is answered. An answer must
///   name the document the question was asked about, or it is dropped.
/// - The view and the session: `message`, `close_allowed`, `check_lines`,
///   `first_gutter_line`, `first_text_line`, `hl_cache` (keyed by its inputs,
///   not by document), and the counters `highlight_runs` and `layouter_calls`.
pub struct Editor {
    pub doc: Doc,
    /// The "File" field. Open reads it and nothing else does.
    pub path: String,
    /// The "Save as" field: a destination given for that purpose only.
    pub save_as_path: String,
    pub query: String,
    pub goto: String,
    pub question: Option<Question>,
    pub message: String,
    /// Set once the user agreed to close with unsaved edits.
    pub close_allowed: bool,
    /// Line numbers, from 1, at the top of the view on the last frame:
    /// the one the gutter drew, and the one the text galley has there.
    pub first_gutter_line: usize,
    pub first_text_line: usize,
    /// Compute `first_text_line`. It scans the text, so it is off by default.
    pub check_lines: bool,
    /// How many times the layouter ran the tokenizer, and how many times it was called.
    pub highlight_runs: u64,
    pub layouter_calls: u64,
    hl_cache: Option<(HlKey, LayoutJob)>,
}

const BUSY: &str = "Answer the question first.";

impl Editor {
    pub fn new(text: String) -> Self {
        let text = file::to_lf(&text).into_owned();
        Self {
            doc: Doc::new(0, text, false, None),
            path: String::new(),
            save_as_path: String::new(),
            query: String::new(),
            goto: String::new(),
            question: None,
            message: String::new(),
            close_allowed: false,
            first_gutter_line: 1,
            first_text_line: 1,
            check_lines: false,
            highlight_runs: 0,
            layouter_calls: 0,
            hl_cache: None,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.doc.text != self.doc.saved
    }

    /// Refuse an action while a question is pending. Returns true if refused.
    fn busy(&mut self) -> bool {
        if self.question.is_some() {
            self.message = BUSY.into();
        }
        self.question.is_some()
    }

    fn ask(&mut self, q: Question) {
        self.message = match &q {
            Question::Unsaved { .. } => "Unsaved edits. Save, discard or cancel?".into(),
            Question::Overwrite { path, .. } => format!("{} exists. Overwrite it?", path.display()),
        };
        self.question = Some(q);
    }

    /// Open `path`, or ask first if that would drop unsaved edits.
    pub fn open(&mut self, path: impl Into<PathBuf>) {
        if self.busy() {
            return;
        }
        let path = path.into();
        if self.is_dirty() {
            let doc = self.doc.id;
            self.ask(Question::Unsaved {
                doc,
                then: Replace::Open(path),
            });
            return;
        }
        self.load(path);
    }

    /// The window was asked to close. Returns true if it may close now.
    pub fn close_requested(&mut self) -> bool {
        if self.close_allowed {
            return true;
        }
        if self.busy() {
            return false;
        }
        if !self.is_dirty() {
            return true;
        }
        let doc = self.doc.id;
        self.ask(Question::Unsaved {
            doc,
            then: Replace::Close,
        });
        false
    }

    /// Answer the pending question about document `doc`. An answer about
    /// another document, or of the wrong kind, is dropped and changes nothing.
    pub fn answer(&mut self, doc: u64, a: Answer) {
        let Some(q) = self.question.take() else {
            return;
        };
        if q.doc() != self.doc.id {
            // Cannot happen: a load clears the question. Write nothing.
            self.message.clear();
            return;
        }
        if doc != q.doc() {
            self.question = Some(q);
            return;
        }
        match (q, a) {
            (Question::Unsaved { .. }, Answer::Cancel)
            | (Question::Overwrite { .. }, Answer::Keep) => self.message.clear(),
            (Question::Overwrite { path, .. }, Answer::Overwrite) => self.write_to(path),
            (q @ Question::Unsaved { .. }, Answer::Save) if self.doc.path.is_none() => {
                // Never offered in the view; keep the edits and the question.
                self.question = Some(q);
            }
            (Question::Unsaved { then, .. }, Answer::Save | Answer::Discard) => {
                if a == Answer::Save {
                    self.save_doc();
                    if self.is_dirty() {
                        return;
                    }
                }
                match then {
                    Replace::Open(p) => self.load(p),
                    Replace::Close => self.close_allowed = true,
                }
            }
            (q, _) => self.question = Some(q),
        }
    }

    fn load(&mut self, path: PathBuf) {
        match std::fs::read(&path) {
            Err(e) => self.message = format!("Not opened: {e}."),
            Ok(bytes) => match file::decode(&bytes) {
                Err(e) => self.message = e.to_string(),
                Ok((text, crlf)) => {
                    self.path = path.display().to_string();
                    // The one place a document's state is reset: all of it, whole.
                    self.doc = Doc::new(self.doc.id + 1, text, crlf, Some(path));
                    // A question is about the old document. Answer paths take it
                    // first, and open refuses while one is set; this is a third lock.
                    self.question = None;
                    self.message = format!("Opened, {}.", if crlf { "CRLF" } else { "LF" });
                }
            },
        }
    }

    /// Write the buffer to the document's own file. A new buffer has none.
    pub fn save(&mut self) {
        if !self.busy() {
            self.save_doc();
        }
    }

    fn save_doc(&mut self) {
        match self.doc.path.clone() {
            Some(p) => self.write_to(p),
            None => self.message = "Not saved: a new buffer needs Save as.".into(),
        }
    }

    /// Write to `path`, which becomes the document's file. Ask first if it exists.
    pub fn save_as(&mut self, path: impl Into<PathBuf>) {
        if self.busy() {
            return;
        }
        let path = path.into();
        if path.as_os_str().is_empty() {
            self.message = "Not saved: give a file for Save as.".into();
        } else if path.exists() && self.doc.path.as_ref() != Some(&path) {
            let doc = self.doc.id;
            self.ask(Question::Overwrite { doc, path });
        } else {
            self.write_to(path);
        }
    }

    fn write_to(&mut self, path: PathBuf) {
        let bytes = file::encode(&self.doc.text, self.doc.crlf);
        match std::fs::write(&path, bytes) {
            Ok(()) => {
                self.doc.saved = self.doc.text.clone();
                self.doc.path = Some(path);
                self.message = "Saved.".into();
            }
            Err(e) => self.message = format!("Not saved: {e}."),
        }
    }

    fn char_of_byte(&self, byte: usize) -> usize {
        self.doc.text[..byte].chars().count()
    }

    fn byte_of_char(&self, ch: usize) -> usize {
        self.doc
            .text
            .char_indices()
            .nth(ch)
            .map_or(self.doc.text.len(), |(b, _)| b)
    }

    /// Select the next match after the cursor, wrapping at the end.
    pub fn find_next(&mut self) {
        if self.query.is_empty() {
            return;
        }
        let from = self.doc.cursor.map_or(0, |c| {
            self.byte_of_char(c.primary.index.max(c.secondary.index).0)
        });
        let hit = self.doc.text[from..]
            .find(&self.query)
            .map(|i| i + from)
            .or_else(|| self.doc.text.find(&self.query));
        self.select_hit(hit);
    }

    /// Select the match before the cursor, wrapping at the start.
    pub fn find_prev(&mut self) {
        if self.query.is_empty() {
            return;
        }
        let to = self.doc.cursor.map_or(self.doc.text.len(), |c| {
            self.byte_of_char(c.primary.index.min(c.secondary.index).0)
        });
        let hit = self.doc.text[..to]
            .rfind(&self.query)
            .or_else(|| self.doc.text.rfind(&self.query));
        self.select_hit(hit);
    }

    fn select_hit(&mut self, hit: Option<usize>) {
        match hit {
            Some(b) => {
                let s = self.char_of_byte(b);
                let n = self.query.chars().count();
                self.doc.pending_sel = Some((s, s + n));
                self.message.clear();
            }
            None => self.message = "Not found.".into(),
        }
    }

    /// Put the cursor at the start of line `line`, counted from 1.
    pub fn goto_line(&mut self, line: usize) {
        let total = self.doc.text.split('\n').count();
        if line == 0 || line > total {
            self.message = format!("No line {line}: the file has {total}.");
            return;
        }
        let byte = if line == 1 {
            0
        } else {
            self.doc
                .text
                .match_indices('\n')
                .nth(line - 2)
                .map_or(0, |(i, _)| i + 1)
        };
        let c = self.char_of_byte(byte);
        self.doc.pending_sel = Some((c, c));
        self.message.clear();
    }

    fn highlight(&mut self, ui: &egui::Ui, text: &str) -> LayoutJob {
        let font = TextStyle::Monospace.resolve(ui.style());
        let dark = ui.visuals().dark_mode;
        if let Some((k, job)) = &self.hl_cache
            && k.text == text
            && k.dark == dark
            && k.font == font
        {
            return job.clone();
        }
        self.highlight_runs += 1;
        let mut job = LayoutJob::default();
        for (r, k) in c_lex::spans(text) {
            job.append(
                &text[r],
                0.0,
                TextFormat::simple(font.clone(), color(k, dark)),
            );
        }
        job.keep_trailing_whitespace = true;
        let key = HlKey {
            text: text.to_owned(),
            dark,
            font,
        };
        self.hl_cache = Some((key, job.clone()));
        job
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if ui.input(|i| i.viewport().close_requested()) && !self.close_requested() {
            ui.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        // While a question is pending, only its answers are enabled.
        let free = self.question.is_none();
        ui.add_enabled_ui(free, |ui| self.toolbar(ui));
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if let Some(q) = &self.question {
            let doc = q.doc();
            let answers: &[(&str, Answer)] = match q {
                Question::Overwrite { .. } => &[
                    ("Overwrite", Answer::Overwrite),
                    ("Keep the file", Answer::Keep),
                ],
                // A new buffer has no file to save to: Cancel, then Save as.
                Question::Unsaved { .. } if self.doc.path.is_none() => &[
                    ("Discard edits", Answer::Discard),
                    ("Cancel", Answer::Cancel),
                ],
                Question::Unsaved { .. } => &[
                    ("Save first", Answer::Save),
                    ("Discard edits", Answer::Discard),
                    ("Cancel", Answer::Cancel),
                ],
            };
            ui.horizontal(|ui| {
                for &(name, a) in answers {
                    if ui.button(name).clicked() {
                        self.answer(doc, a);
                    }
                }
            });
        }
        if self.close_allowed {
            ui.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let l = ui.label("C source").id;
        self.editor(ui, l);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
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
        ui.horizontal(|ui| {
            let l = ui.label("Save as file").id;
            ui.add(egui::TextEdit::singleline(&mut self.save_as_path).desired_width(220.0))
                .labelled_by(l);
            if ui.button("Save as").clicked() {
                let p = self.save_as_path.clone();
                self.save_as(p);
            }
        });
    }

    fn editor(&mut self, ui: &mut egui::Ui, label: egui::Id) {
        let id = egui::Id::unique((TEXT_ID, self.doc.id));
        let font: FontId = TextStyle::Monospace.resolve(ui.style());
        let row_h = ui.fonts_mut(|f| f.row_height(&font));
        let lines = self.doc.text.split('\n').count();
        let gutter_w = ui.fonts_mut(|f| f.glyph_width(&font, '0')) * (digits(lines) as f32 + 1.0);

        // egui 0.37 collapses a selection on a text field that does not have focus.
        // A button press moves focus too, one frame late. So focus is requested
        // first, and the selection is applied on a later frame that has focus.
        let focused = ui.memory(|m| m.has_focus(id));
        // A pending question locks the buffer: no typing, and a selection
        // asked for by search or go to line waits until it is answered.
        let locked = self.question.is_some();
        let scroll_sel = match self.doc.pending_sel {
            _ if locked => None,
            Some(_) if !focused || !self.doc.sel_focus_asked => {
                self.doc.sel_focus_asked = true;
                ui.memory_mut(|m| m.request_focus(id));
                ui.ctx().request_repaint();
                None
            }
            Some((a, b)) => {
                self.doc.pending_sel = None;
                self.doc.sel_focus_asked = false;
                let mut st = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
                st.cursor
                    .set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
                st.store(ui.ctx(), id);
                Some(a)
            }
            None => None,
        };

        let mut area = egui::ScrollArea::both()
            .id_salt(("k7-scroll", self.doc.id))
            .auto_shrink(false);
        if let Some(line) = self.doc.scroll_to_line.take() {
            // The row's own position in last frame's galley, so no rounding drifts.
            let y = match &self.doc.last_galley {
                Some((g, origin)) => origin + g.rows.get(line).map_or(0.0, |r| r.pos.y),
                None => row_h * line as f32,
            };
            area = area.vertical_scroll_offset(y);
        }

        // Text enters the buffer by these events. Make it LF before TextEdit sees it,
        // so the cursor and the undo history agree with the stored text.
        if focused {
            ui.input_mut(|i| {
                for e in &mut i.events {
                    let s = match e {
                        egui::Event::Paste(s)
                        | egui::Event::Text(s)
                        | egui::Event::Ime(egui::ImeEvent::Commit(s)) => s,
                        egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                            data: Some(egui::accesskit::ActionData::Value(s)),
                            ..
                        }) => {
                            if s.contains('\r') {
                                *s = file::to_lf(s).into_owned().into();
                            }
                            continue;
                        }
                        _ => continue,
                    };
                    if s.contains('\r') {
                        *s = file::to_lf(s).into_owned();
                    }
                }
            });
        }

        // Moved out so the layouter closure can borrow `self` mutably.
        let mut text = std::mem::take(&mut self.doc.text);
        let before = locked.then(|| text.clone());
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
                    .interactive(!locked)
                    .desired_width(f32::INFINITY)
                    .desired_rows(1)
                    .frame(egui::Frame::NONE)
                    .layouter(&mut layouter)
                    .show(ui);
                out.response.response.clone().labelled_by(label);
                self.doc.cursor = out.state.cursor.char_range();
                self.doc.last_galley = Some((out.galley.clone(), out.galley_pos.y - gutter.top()));

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
        // Backstop for a door not listed above: no CR may stay in the buffer.
        if text.contains('\r') {
            text = file::to_lf(&text).into_owned();
        }
        // Second lock: whatever got past the widget while a question is pending
        // is undone, so the answer acts on the text it was asked about.
        self.doc.text = before.unwrap_or(text);
    }
}

/// What the cached layout job depends on.
struct HlKey {
    text: String,
    dark: bool,
    font: FontId,
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
