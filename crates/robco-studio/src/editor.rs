// SPDX-License-Identifier: GPL-3.0-or-later

//! The text editor: open files as tabs, each with its own text widget state,
//! a painted gutter, colours for C, and a find and replace bar.
//!
//! The editor never drops text without asking. One question at most is open
//! at a time, and it names the document it is about.

use crate::art::Icon;
use crate::c_lex::{self, Class};
use crate::document::{self, Doc};
use crate::kit::{Button, ButtonKind, Kit};
use crate::theme::{Theme, Type, size, stroke};
use eframe::egui::{
    self, CornerRadius, Id, Key, Rect, Sense, StrokeKind, text::CCursor, text::CCursorRange,
    text::LayoutJob, text::TextFormat,
};
use std::path::Path;

/// The one question the editor can have open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    /// Closing this document would drop its edits.
    CloseTab {
        /// The document.
        doc: u64,
    },
    /// Quitting would drop this document's edits; more may follow.
    Quit {
        /// The document.
        doc: u64,
    },
    /// The file changed on disk since it was read or saved: overwrite it?
    Overwrite {
        /// The document.
        doc: u64,
    },
}

impl Question {
    /// The document the question is about.
    pub fn doc(self) -> u64 {
        match self {
            Question::CloseTab { doc } | Question::Quit { doc } | Question::Overwrite { doc } => {
                doc
            }
        }
    }
}

/// An answer, in the words of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// "Do not save": drop the edits and go on.
    DoNotSave,
    /// "Cancel": nothing happens.
    Cancel,
    /// "Save": save, then go on. If the save fails, nothing else happens.
    /// For [`Question::Overwrite`] this is "Overwrite".
    Save,
}

/// The find and replace bar.
#[derive(Debug, Default)]
pub struct Find {
    /// The bar is shown.
    pub open: bool,
    /// What to find.
    pub query: String,
    /// What to put in its place.
    pub replace: String,
    /// The bar was just opened: give its field focus.
    pub(crate) focus: bool,
}

/// The editor model.
#[derive(Debug, Default)]
pub struct Editor {
    /// The open documents, in tab order.
    pub docs: Vec<Doc>,
    /// The tab shown.
    pub active: Option<usize>,
    next_id: u64,
    /// The open question.
    pub question: Option<Question>,
    /// Quitting is agreed: the window may close.
    pub quit_allowed: bool,
    /// The find and replace bar.
    pub find: Find,
    /// A sentence for the status line, taken by the shell.
    pub news: Option<(crate::kit::Level, String)>,
    /// The last code layout, keyed by document, language, text and the
    /// whole theme: every colour and font of the layout comes from it.
    cache: Option<(u64, bool, String, Theme, LayoutJob)>,
}

const TEXT_ID: &str = "editor-text";

impl Editor {
    /// The text widget id of a document.
    pub fn text_id(doc: u64) -> Id {
        Id::unique((TEXT_ID, doc))
    }

    /// The document shown.
    pub fn current(&self) -> Option<&Doc> {
        self.active.and_then(|i| self.docs.get(i))
    }

    pub(crate) fn current_mut(&mut self) -> Option<&mut Doc> {
        self.active.and_then(|i| self.docs.get_mut(i))
    }

    fn index_of(&self, doc: u64) -> Option<usize> {
        self.docs.iter().position(|d| d.id == doc)
    }

    /// Open `path` in a tab, or show its tab if it is open. Documents are
    /// told apart by the file's identity, so a link to an open file, or
    /// another spelling of its path, shows the open tab.
    pub fn open(&mut self, path: &Path, shown: String) {
        let key = document::identity(path);
        if let Some(i) = self.docs.iter().position(|d| d.key == key) {
            self.active = Some(i);
            return;
        }
        self.next_id += 1;
        match Doc::open(self.next_id, path, shown.clone()) {
            Ok(d) => {
                self.docs.push(d);
                self.active = Some(self.docs.len() - 1);
                self.news = Some((crate::kit::Level::Info, format!("Opened {shown}.")));
            }
            Err(e) => self.news = Some((crate::kit::Level::Error, format!("{shown}: {e}"))),
        }
    }

    /// Save the document shown. A document without edits is not written,
    /// so a change another program made is kept. A file changed on disk
    /// since it was read or saved asks before it is overwritten.
    pub fn save(&mut self) {
        if self.question.is_some() {
            return;
        }
        let Some(d) = self.current_mut() else { return };
        if !d.is_dirty() {
            let s = format!("Nothing to save in {}.", d.shown);
            self.news = Some((crate::kit::Level::Info, s));
        } else if d.changed_on_disk() {
            self.question = Some(Question::Overwrite { doc: d.id });
        } else {
            let r = d.save().map(|()| format!("Saved {}.", d.shown));
            self.report(r);
        }
    }

    /// Save every document with edits. Stops at the first that fails, or
    /// whose file changed on disk: that one is shown and asks.
    pub fn save_all(&mut self) {
        if self.question.is_some() {
            return;
        }
        let mut last = None;
        for i in 0..self.docs.len() {
            let d = &mut self.docs[i];
            if !d.is_dirty() {
                continue;
            }
            if d.changed_on_disk() {
                self.question = Some(Question::Overwrite { doc: d.id });
                self.active = Some(i);
                last = None;
                break;
            }
            last = Some(d.save().map(|()| format!("Saved {}.", d.shown)));
            if matches!(last, Some(Err(_))) {
                break;
            }
        }
        if let Some(r) = last {
            self.report(r);
        }
    }

    fn report(&mut self, r: Result<String, String>) {
        self.news = Some(match r {
            Ok(s) => (crate::kit::Level::Success, s),
            Err(e) => (crate::kit::Level::Error, e),
        });
    }

    /// Close the tab of `doc`, or ask first if it has edits.
    pub fn close(&mut self, doc: u64) {
        if self.question.is_some() {
            return;
        }
        match self.index_of(doc) {
            Some(i) if self.docs[i].is_dirty() => self.question = Some(Question::CloseTab { doc }),
            Some(i) => self.remove(i),
            None => {}
        }
    }

    fn remove(&mut self, i: usize) {
        self.docs.remove(i);
        self.active = match self.active {
            _ if self.docs.is_empty() => None,
            Some(a) if a > i || a == self.docs.len() => Some(a - 1),
            a => a,
        };
    }

    /// The window was asked to close. True if it may close now; otherwise
    /// a question is open about the first document with edits.
    pub fn request_quit(&mut self) -> bool {
        if self.quit_allowed {
            return true;
        }
        if self.question.is_some() {
            return false;
        }
        match self.docs.iter().find(|d| d.is_dirty()) {
            Some(d) => {
                self.question = Some(Question::Quit { doc: d.id });
                self.active = self.index_of(d.id);
                false
            }
            None => {
                self.quit_allowed = true;
                true
            }
        }
    }

    /// Answer the open question about `doc`. An answer about another
    /// document changes nothing.
    pub fn answer(&mut self, doc: u64, a: Answer) {
        let Some(q) = self.question else { return };
        if q.doc() != doc {
            return;
        }
        let Some(i) = self.index_of(doc) else {
            self.question = None;
            return;
        };
        if a == Answer::Cancel
            || (a == Answer::DoNotSave && matches!(q, Question::Overwrite { .. }))
        {
            self.question = None;
            return;
        }
        if let Question::Overwrite { .. } = q {
            let r = self.docs[i]
                .save()
                .map(|()| format!("Saved {}.", self.docs[i].shown));
            self.report(r);
            self.question = None;
            return;
        }
        if a == Answer::Save && self.docs[i].changed_on_disk() {
            let s = format!(
                "{} changed on disk since Studio read it. Studio did not save it. Cancel, then Save to choose.",
                self.docs[i].shown
            );
            self.news = Some((crate::kit::Level::Error, s));
            return;
        }
        if a == Answer::Save {
            let r = self.docs[i]
                .save()
                .map(|()| format!("Saved {}.", self.docs[i].shown));
            let failed = r.is_err();
            self.report(r);
            if failed {
                return;
            }
        }
        self.question = None;
        self.remove(i);
        if let Question::Quit { .. } = q {
            self.request_quit();
        }
    }

    /// The question's sentence, in the design's words.
    pub fn question_text(&self) -> Option<String> {
        let q = self.question?;
        let d = self.docs.get(self.index_of(q.doc())?)?;
        Some(match q {
            Question::Overwrite { .. } => format!("{} changed on disk. Overwrite it?", d.shown),
            _ => format!("Save {} before closing?", d.shown),
        })
    }

    /// Byte ranges of every match of the query in the document shown.
    pub fn matches(&self) -> Vec<std::ops::Range<usize>> {
        match self.current() {
            Some(d) if !self.find.query.is_empty() => d
                .text
                .match_indices(&self.find.query)
                .map(|(i, m)| i..i + m.len())
                .collect(),
            _ => Vec::new(),
        }
    }

    fn caret_byte(d: &Doc, ctx: &egui::Context, end: bool) -> usize {
        let st = egui::text_edit::TextEditState::load(ctx, Self::text_id(d.id));
        let c = st
            .and_then(|s| s.cursor.char_range())
            .map(|r| {
                let s = r.as_sorted_char_range();
                if end { s.end.0 } else { s.start.0 }
            })
            .unwrap_or(0);
        d.text
            .char_indices()
            .nth(c)
            .map_or(d.text.len(), |(b, _)| b)
    }

    /// Select the next match after the caret, or the one before it.
    /// Wraps round the ends of the text.
    pub fn find_step(&mut self, ctx: &egui::Context, forward: bool) {
        self.step(ctx, forward, true);
    }

    fn step(&mut self, ctx: &egui::Context, forward: bool, wrap: bool) {
        let ms = self.matches();
        let Some(d) = self.current() else { return };
        if ms.is_empty() {
            return;
        }
        let hit = if forward {
            let from = Self::caret_byte(d, ctx, true);
            match ms.iter().find(|m| m.start >= from) {
                Some(m) => m.clone(),
                None if wrap => ms[0].clone(),
                None => return,
            }
        } else {
            let to = Self::caret_byte(d, ctx, false);
            ms.iter()
                .rev()
                .find(|m| m.end <= to)
                .unwrap_or(&ms[ms.len() - 1])
                .clone()
        };
        let a = d.text[..hit.start].chars().count();
        let b = a + d.text[hit.clone()].chars().count();
        if let Some(d) = self.current_mut() {
            d.pending_sel = Some((a, b));
        }
    }

    /// Replace the selected match, then select the next one after the
    /// inserted text. Stops after the last match: Next wraps, Replace does
    /// not, so the inserted text is never searched again.
    pub fn replace_one(&mut self, ctx: &egui::Context) {
        if self.question.is_some() {
            return;
        }
        let rep = self.find.replace.clone();
        let q = self.find.query.clone();
        let Some(d) = self.current() else { return };
        let (s, e) = (
            Self::caret_byte(d, ctx, false),
            Self::caret_byte(d, ctx, true),
        );
        if !q.is_empty() && d.text.get(s..e) == Some(q.as_str()) {
            let id = d.id;
            let c = if let Some(d) = self.current_mut() {
                let c = replace_range(&mut d.text, s..e, &rep);
                d.pending_sel = Some((c, c));
                c
            } else {
                return;
            };
            // Text changed under the widget: keep its caret in step.
            let mut st =
                egui::text_edit::TextEditState::load(ctx, Self::text_id(id)).unwrap_or_default();
            st.cursor
                .set_char_range(Some(CCursorRange::one(CCursor::new(c))));
            st.store(ctx, Self::text_id(id));
        }
        self.step(ctx, true, false);
    }

    /// Replace every match in the document shown.
    pub fn replace_all(&mut self) -> usize {
        if self.question.is_some() || self.find.query.is_empty() {
            return 0;
        }
        let (q, rep) = (
            self.find.query.clone(),
            document::to_lf(&self.find.replace).into_owned(),
        );
        let Some(d) = self.current_mut() else {
            return 0;
        };
        let n = d.text.matches(&q).count();
        d.text = d.text.replace(&q, &rep);
        n
    }

    fn layout_job(&mut self, theme: &Theme, doc: u64, c: bool, text: &str) -> LayoutJob {
        let p = theme.palette();
        if let Some((d, lang, t, th, job)) = &self.cache
            && *d == doc
            && *lang == c
            && t == text
            && th == theme
        {
            return job.clone();
        }
        let mut job = LayoutJob::default();
        let line = theme.code_line();
        for (r, class) in spans(text, c) {
            let (font, color) = match class {
                Class::Plain => (theme.font(Type::Code), p.text),
                Class::Keyword => (theme.font(Type::CodeBold), p.bright),
                Class::Comment => (theme.font(Type::CodeItalic), p.dim),
                Class::Str | Class::Number => (theme.font(Type::Code), p.lit),
                Class::Preproc => (theme.font(Type::CodeBold), p.dim),
            };
            let mut f = TextFormat::simple(font, color);
            f.line_height = Some(line);
            job.append(&text[r], 0.0, f);
        }
        job.keep_trailing_whitespace = true;
        self.cache = Some((doc, c, text.to_owned(), *theme, job.clone()));
        job
    }

    /// Keys the editor takes whatever has focus inside it.
    pub fn keys(&mut self, ctx: &egui::Context) {
        // Find, F3, save and close are app commands, dispatched on every page.
        let esc = ctx.input(|i| self.find.open && i.key_pressed(Key::Escape));
        if esc {
            self.find.open = false;
        }
    }

    /// Draw the tabs in the pane's top line.
    pub fn tabs(&mut self, kit: &Kit<'_>, ui: &mut egui::Ui, header: Rect) {
        let mut x = header.left();
        let mut chosen = None;
        let mut close = None;
        let labels = tab_labels(&self.docs);
        for (i, d) in self.docs.iter().enumerate() {
            let icon = if d.path.extension().is_some_and(|e| e == "c" || e == "h") {
                Icon::FileC
            } else if d.name() == "robco.toml" {
                Icon::Manifest
            } else {
                Icon::File
            };
            let (r, out) = kit.tab(
                ui,
                egui::pos2(x, header.center().y),
                ("doc", d.id),
                &labels[i],
                Some(icon),
                self.active == Some(i),
                false,
                d.is_dirty(),
            );
            x = r.right() + stroke::FRAME;
            if out.chosen {
                chosen = Some(i);
            }
            if out.close {
                close = Some(d.id);
            }
        }
        if let Some(i) = chosen {
            self.active = Some(i);
        }
        if let Some(id) = close {
            self.close(id);
        }
    }

    /// Draw the find bar and the text of the document shown in `rect`.
    pub fn body(&mut self, kit: &Kit<'_>, ui: &mut egui::Ui, rect: Rect) {
        let p = kit.p();
        let Some(idx) = self.active else { return };
        let mut area = rect;
        if self.find.open {
            let bar = Rect::from_min_max(
                rect.left_top(),
                egui::pos2(
                    rect.right(),
                    rect.top() + 2.0 * (size::CONTROL_H + size::UNIT),
                ),
            );
            area.min.y = bar.bottom() + size::UNIT;
            self.find_bar(kit, ui, bar);
        }
        let locked = self.question.is_some();
        let doc = &mut self.docs[idx];
        let id = Self::text_id(doc.id);
        let focused = ui.memory(|m| m.has_focus(id));
        // A selection set on a field without focus is dropped: ask for focus
        // first, and set the selection once it has it.
        let mut scroll_to = None;
        if let Some((a, b)) = doc.pending_sel
            && !locked
        {
            if focused {
                doc.pending_sel = None;
                let mut st = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
                st.cursor
                    .set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
                st.store(ui.ctx(), id);
                scroll_to = Some(b);
            } else {
                ui.memory_mut(|m| m.request_focus(id));
                ui.ctx().request_repaint();
            }
        }
        // Text enters by these events: make it LF before the widget reads it.
        if focused {
            ui.input_mut(|i| {
                for e in &mut i.events {
                    match e {
                        egui::Event::Paste(s)
                        | egui::Event::Text(s)
                        | egui::Event::Ime(egui::ImeEvent::Commit(s))
                            if s.contains('\r') =>
                        {
                            *s = document::to_lf(s).into_owned();
                        }
                        _ => {}
                    }
                }
            });
        }
        let mut text = std::mem::take(&mut doc.text);
        let before = locked.then(|| text.clone());
        let doc_id = doc.id;
        let is_c = is_c(&doc.path);
        let shown = doc.shown.clone();
        let mut caret = None;
        let line_h = kit.theme.code_line();
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(area)
                .id_salt(("editor-area", doc_id)),
        );
        egui::ScrollArea::both()
            .id_salt(("editor-scroll", doc_id))
            .auto_shrink(false)
            .show(&mut child, |ui| {
                ui.horizontal_top(|ui| {
                    let (gutter, _) =
                        ui.allocate_exact_size(egui::vec2(size::GUTTER_W, line_h), Sense::hover());
                    let bg = ui.painter().add(egui::Shape::Noop);
                    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                        let mut job = self.layout_job(kit.theme, doc_id, is_c, buf.as_str());
                        job.wrap.max_width = wrap;
                        ui.fonts_mut(|f| f.layout_job(job))
                    };
                    let out = egui::TextEdit::multiline(&mut text)
                        .id(id)
                        .code_editor()
                        .lock_focus(true)
                        .interactive(!locked)
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::NONE)
                        .layouter(&mut layouter)
                        .show(ui);
                    ui.ctx()
                        .accesskit_node_builder(id, |n| n.set_label(format!("Text of {shown}")));
                    let g = &out.galley;
                    let clip = ui.clip_rect();
                    let painter = ui.painter();
                    let cur = out.state.cursor.char_range();
                    if let Some(c) = cur {
                        let r = g.pos_from_cursor(c.primary);
                        let row = Rect::from_min_max(
                            egui::pos2(gutter.left(), out.galley_pos.y + r.top()),
                            egui::pos2(clip.right(), out.galley_pos.y + r.bottom()),
                        );
                        painter.set(
                            bg,
                            egui::Shape::rect_filled(row, CornerRadius::ZERO, p.raised),
                        );
                        let pre: String = text.chars().take(c.primary.index.0).collect();
                        let line = pre.matches('\n').count() + 1;
                        let col = pre.rsplit('\n').next().map_or(0, |s| s.chars().count()) + 1;
                        caret = Some((line, col));
                    }
                    if let Some(c) = scroll_to {
                        let r = g
                            .pos_from_cursor(CCursor::new(c))
                            .translate(out.galley_pos.to_vec2());
                        ui.scroll_to_rect(r, Some(egui::Align::Center));
                    }
                    // Line numbers for the rows in view. No wrapping: row i is line i + 1.
                    let top = clip.top() - out.galley_pos.y;
                    let first = g
                        .rows
                        .partition_point(|r| r.pos.y + r.rect().height() <= top);
                    let font = kit.theme.font(Type::Code);
                    for (i, r) in g.rows.iter().enumerate().skip(first) {
                        let y = out.galley_pos.y + r.pos.y;
                        if y > clip.bottom() {
                            break;
                        }
                        painter.text(
                            egui::pos2(
                                gutter.right() - size::STEP_12,
                                y + (r.rect().height() - line_h) / 2.0 + line_h / 2.0,
                            ),
                            egui::Align2::RIGHT_CENTER,
                            (i + 1).to_string(),
                            font.clone(),
                            p.dim,
                        );
                    }
                    // Matches of the find bar: a `frame` box, the selected one `bright`.
                    if self.find.open && !self.find.query.is_empty() {
                        let sel = cur.map(|c| c.as_sorted_char_range());
                        let mut chars = 0;
                        let mut last = 0;
                        for (b, m) in text.match_indices(&self.find.query) {
                            chars += text[last..b].chars().count();
                            last = b;
                            let n = m.chars().count();
                            let a = g.pos_from_cursor(CCursor::new(chars));
                            let z = g.pos_from_cursor(CCursor::new(chars + n));
                            let r = Rect::from_min_max(a.min, egui::pos2(z.max.x, a.max.y))
                                .translate(out.galley_pos.to_vec2());
                            if r.intersects(clip) {
                                let on = sel
                                    .as_ref()
                                    .is_some_and(|s| s.start.0 == chars && s.end.0 == chars + n);
                                painter.rect_stroke(
                                    r,
                                    CornerRadius::ZERO,
                                    kit.theme.frame(if on { p.bright } else { p.frame }),
                                    StrokeKind::Outside,
                                );
                            }
                        }
                    }
                });
            });
        if text.contains('\r') {
            text = document::to_lf(&text).into_owned();
        }
        let doc = &mut self.docs[idx];
        doc.text = before.unwrap_or(text);
        doc.caret = caret;
    }

    fn find_bar(&mut self, kit: &Kit<'_>, ui: &mut egui::Ui, bar: Rect) {
        let p = kit.p();
        let ms = self.matches();
        let cur = self.current().map(|d| {
            (
                Self::caret_byte(d, ui.ctx(), false),
                Self::caret_byte(d, ui.ctx(), true),
            )
        });
        let at = cur.and_then(|(s, e)| ms.iter().position(|m| m.start == s && m.end == e));
        let none = !self.find.query.is_empty() && ms.is_empty();
        // Two rows, so the bar fits a narrow editor: find on the first,
        // replace on the second. Each field takes what is left of its row.
        let row_h = bar.height() / 2.0;
        let rows = [
            Rect::from_min_size(bar.min, egui::vec2(bar.width(), row_h)),
            Rect::from_min_size(
                egui::pos2(bar.left(), bar.top() + row_h),
                egui::vec2(bar.width(), row_h),
            ),
        ];
        let row = |ui: &mut egui::Ui, r: Rect| {
            ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(r)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            )
        };
        let mut first = row(ui, rows[0]);
        let mut second = row(ui, rows[1]);
        let ui = &mut first;
        if kit
            .icon_button(ui, Icon::Close, "Close find", None)
            .clicked()
        {
            self.find.open = false;
        }
        if kit
            .button(ui, Button::quiet("Next").small().kind(ButtonKind::Quiet))
            .clicked()
        {
            self.find_step(ui.ctx(), true);
        }
        if kit.button(ui, Button::quiet("Previous").small()).clicked() {
            self.find_step(ui.ctx(), false);
        }
        let tag = if none {
            "No match.".to_owned()
        } else if ms.is_empty() {
            String::new()
        } else {
            match at {
                Some(i) => format!("{} of {}", i + 1, ms.len()),
                None => format!("{} matches", ms.len()),
            }
        };
        kit.text(ui, &tag, Type::Small, if none { p.alarm } else { p.dim });
        let fid = Id::unique("find-query");
        let w = size::FIND_W.min(ui.available_width()).max(0.0);
        let r = kit.text_field(ui, fid, &mut self.find.query, "Find", "Find", w, none);
        let ui2 = &mut second;
        if kit
            .button(ui2, Button::quiet("Replace all").small())
            .clicked()
        {
            let n = self.replace_all();
            self.news = Some((crate::kit::Level::Info, format!("Replaced {n}.")));
        }
        if kit.button(ui2, Button::quiet("Replace").small()).clicked() {
            self.replace_one(ui2.ctx());
        }
        let rid = Id::unique("find-replace");
        let w = (size::FIND_W / 2.0 + size::STEP_32)
            .min(ui2.available_width())
            .max(0.0);
        kit.text_field(
            ui2,
            rid,
            &mut self.find.replace,
            "Replace with",
            "Replace with",
            w,
            false,
        );
        if std::mem::take(&mut self.find.focus) {
            r.request_focus();
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            self.find_step(ui.ctx(), true);
            r.request_focus();
        }
    }
}

/// The name each tab shows: the file name, or, when two open files share
/// it, as many trailing folders as tell them apart.
fn tab_labels(docs: &[Doc]) -> Vec<String> {
    let parts: Vec<Vec<String>> = docs
        .iter()
        .map(|d| {
            let mut p: Vec<String> = d
                .path
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            p.reverse();
            p
        })
        .collect();
    let label = |p: &[String], n: usize| -> String {
        let mut v: Vec<&str> = p.iter().take(n).map(String::as_str).collect();
        v.reverse();
        v.join("/")
    };
    docs.iter()
        .enumerate()
        .map(|(i, d)| {
            let mine = &parts[i];
            let mut n = 1;
            while n < mine.len()
                && parts
                    .iter()
                    .enumerate()
                    .any(|(j, o)| j != i && label(o, n) == label(mine, n))
            {
                n += 1;
            }
            if n == 1 { d.name() } else { label(mine, n) }
        })
        .collect()
}

/// A C source or header, by its ending, in any case.
fn is_c(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("c") || e.eq_ignore_ascii_case("h"))
}

/// The colour classes of `text`: C tokens for a C file, plain otherwise.
fn spans(text: &str, c: bool) -> Vec<(std::ops::Range<usize>, Class)> {
    if c {
        c_lex::spans(text)
    } else {
        vec![(0..text.len(), Class::Plain)]
    }
}

/// Put `rep`, made LF, in place of `range`. Gives the caret after it, in chars.
fn replace_range(text: &mut String, range: std::ops::Range<usize>, rep: &str) -> usize {
    let rep = document::to_lf(rep);
    let start = range.start;
    text.replace_range(range, &rep);
    text[..start].chars().count() + rep.chars().count()
}

#[cfg(test)]
mod replace_tests {
    use super::replace_range;

    #[test]
    fn only_c_files_get_c_colours() {
        use super::{Class, is_c, spans};
        use std::path::Path;
        assert!(is_c(Path::new("src/main.c")));
        assert!(is_c(Path::new("INC/PIP.H")));
        assert!(!is_c(Path::new("notes.txt")));
        assert!(!is_c(Path::new("Makefile")));
        let t = "return 0;";
        assert_eq!(spans(t, false), vec![(0..t.len(), Class::Plain)]);
        assert!(spans(t, true).iter().any(|(_, k)| *k == Class::Keyword));
    }

    #[test]
    fn caret_counts_inserted_text() {
        let mut t = String::from("ab FOO cd");
        let c = replace_range(&mut t, 3..6, "x\r\ny");
        assert_eq!(t, "ab x\ny cd");
        assert_eq!(c, 6);
    }

    /// A scheme change recolours the code even when the text colour and
    /// the font stay the same, as they do with high legibility on.
    #[test]
    fn scheme_change_recolours_cached_code() {
        use super::{Editor, Theme};
        let mut e = Editor::default();
        let mut t = Theme {
            high_legibility: true,
            ..Theme::default()
        };
        let green = e.layout_job(&t, 1, true, "int x;");
        t.scheme = crate::theme::Scheme::Amber;
        assert_eq!(
            green.sections[0].format.color,
            Theme {
                scheme: crate::theme::Scheme::Green,
                ..t
            }
            .palette()
            .bright
        );
        let amber = e.layout_job(&t, 1, true, "int x;");
        assert_eq!(amber.sections[0].format.color, t.palette().bright);
        assert_ne!(t.palette().bright, green.sections[0].format.color);
    }

    /// A test folder, removed when the test ends, pass or fail.
    struct Gone(std::path::PathBuf);
    impl Drop for Gone {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn folder(tag: &str) -> Gone {
        let d = std::env::temp_dir().join(format!("robco-ed-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Gone(d)
    }

    /// A link to an open file, or another spelling of its path, shows the
    /// open tab, so Save all never writes one file twice.
    #[cfg(unix)]
    #[test]
    fn a_file_and_a_link_to_it_are_one_document() {
        use super::Editor;
        let g = folder("link");
        let real = g.0.join("real.c");
        std::fs::write(&real, b"x\n").unwrap();
        std::os::unix::fs::symlink(&real, g.0.join("link.c")).unwrap();
        let mut e = Editor::default();
        e.open(&real, "real.c".into());
        e.open(&g.0.join("link.c"), "link.c".into());
        e.open(&g.0.join(".").join("real.c"), "real.c".into());
        assert_eq!(e.docs.len(), 1);
        // A folded spelling: one file where the file system folds case.
        let folds = std::fs::metadata(g.0.join("REAL.C")).is_ok();
        e.open(&g.0.join("REAL.C"), "REAL.C".into());
        if folds {
            assert_eq!(e.docs.len(), 1, "canonicalize keeps the stored spelling");
            assert_eq!(
                super::document::identity(&g.0.join("REAL.C")),
                e.docs[0].key
            );
        }
    }

    /// The identity of a file that does not exist yet is its folder's
    /// canonical path and its name.
    #[test]
    fn a_missing_file_has_its_folders_identity() {
        let g = folder("missing");
        let id = super::document::identity(&g.0.join(".").join("new.c"));
        assert_eq!(id, std::fs::canonicalize(&g.0).unwrap().join("new.c"));
    }

    /// Save on a clean document does not write, so another program's
    /// change is kept.
    #[test]
    fn save_skips_a_clean_document() {
        use super::Editor;
        let g = folder("clean");
        let f = g.0.join("a.txt");
        std::fs::write(&f, b"one\n").unwrap();
        let mut e = Editor::default();
        e.open(&f, "a.txt".into());
        std::fs::write(&f, b"theirs\n").unwrap();
        e.save();
        assert_eq!(std::fs::read(&f).unwrap(), b"theirs\n");
        assert!(
            e.news
                .as_ref()
                .is_some_and(|(_, s)| s.contains("Nothing to save"))
        );
    }

    /// Save of an edited document whose file changed on disk asks first;
    /// Cancel keeps the other change, Overwrite writes.
    #[test]
    fn save_asks_when_the_file_changed_on_disk() {
        use super::{Answer, Editor, Question};
        let g = folder("changed");
        let f = g.0.join("a.txt");
        std::fs::write(&f, b"one\n").unwrap();
        let mut e = Editor::default();
        e.open(&f, "a.txt".into());
        let id = e.docs[0].id;
        e.docs[0].text = "mine\n".into();
        std::fs::write(&f, b"theirs, longer\n").unwrap();
        e.save();
        assert_eq!(e.question, Some(Question::Overwrite { doc: id }));
        assert_eq!(std::fs::read(&f).unwrap(), b"theirs, longer\n");
        e.answer(id, Answer::Cancel);
        assert_eq!(std::fs::read(&f).unwrap(), b"theirs, longer\n");
        e.save_all();
        assert_eq!(e.question, Some(Question::Overwrite { doc: id }));
        e.answer(id, Answer::Save);
        assert_eq!(std::fs::read(&f).unwrap(), b"mine\n");
        assert_eq!(e.docs.len(), 1);
        // Saved: the next save of new edits writes without asking.
        e.docs[0].text = "again\n".into();
        e.save();
        assert_eq!(e.question, None);
        assert_eq!(std::fs::read(&f).unwrap(), b"again\n");
    }
}
