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
use crate::theme::{Theme, Type, size};
use eframe::egui::{
    self, CornerRadius, Id, Key, Rect, StrokeKind, text::CCursor, text::CCursorRange,
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
    /// A project switch asks about each document with edits.
    pub switching: bool,
    /// The switch is agreed: no document with edits is left.
    pub switch_allowed: bool,
    /// The find and replace bar.
    pub find: Find,
    /// A sentence for the status line, taken by the shell.
    pub news: Option<(crate::kit::Level, String)>,
    /// The last code layout, keyed by document, language, text and the
    /// whole theme: every colour and font of the layout comes from it.
    cache: Option<(u64, bool, String, Theme, LayoutJob)>,
    /// How far the tab strip is scrolled to the left, in points.
    tab_scroll: f32,
    /// The document whose tab was last brought into view.
    tab_seen: Option<u64>,
    /// A Save all that waits on an answer, then goes on.
    run: Option<SaveRun>,
    /// A close, quit or switch that collects an answer per edited document.
    leave: Option<LeaveRun>,
    /// The tabs did not fit the header in the last frame.
    tab_overflow: bool,
    /// The gutter as last drawn.
    pub gutter: Option<Gutter>,
}

/// Where the gutter was last drawn, and what it showed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gutter {
    /// The gutter's rectangle. It stays put when the text scrolls sideways.
    pub rect: Rect,
    /// How far the text is scrolled, across and down.
    pub offset: egui::Vec2,
    /// The top of the text before any scroll.
    pub text_top: f32,
    /// The first line number drawn, and the centre height of its text.
    pub first: Option<(usize, f32)>,
}

/// A Save all in progress: the documents still to save, and the count so far.
#[derive(Debug, Default)]
struct SaveRun {
    todo: std::collections::VecDeque<u64>,
    saved: usize,
    kept: usize,
    error: Option<String>,
}

/// What waits on the answers about edited documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leave {
    /// Close one tab.
    Close,
    /// Close the window.
    Quit,
    /// Open another project.
    Switch,
}

/// A close, quit or switch in progress. Answers are only collected here;
/// nothing is saved or dropped until the last one is in.
#[derive(Debug)]
struct LeaveRun {
    why: Leave,
    /// Documents still to ask about.
    todo: std::collections::VecDeque<u64>,
    /// Every document asked about: they go once the run ends.
    asked: Vec<u64>,
    /// Documents whose answer was Save and that are not written yet.
    save: Vec<u64>,
    /// The answers are all in and the saves have begun.
    saving: bool,
    /// How many of `save` were written so far, and the last one's name.
    saved: usize,
    last: Option<String>,
}

/// What the save gate did.
#[derive(Debug)]
enum Gate {
    /// The file was written.
    Saved,
    /// The file changed on disk, was removed or cannot be read: nothing
    /// was written, the Overwrite question decides.
    Changed,
    /// The write failed; the file was left as it was.
    Failed(String),
}

/// "1 document", "3 documents".
fn count(k: usize) -> String {
    if k == 1 {
        "1 document".to_owned()
    } else {
        format!("{k} documents")
    }
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
        } else {
            let (id, shown) = (d.id, d.shown.clone());
            let Some(i) = self.active else { return };
            match self.write(i, false) {
                Gate::Saved => self.report(Ok(format!("Saved {shown}."))),
                Gate::Changed => self.question = Some(Question::Overwrite { doc: id }),
                Gate::Failed(e) => self.report(Err(e)),
            }
        }
    }

    /// The save gate: every write of a document goes through here. Unless
    /// `overwrite`, the file is first checked against what the document
    /// last read or wrote, by bytes, right before the write; a file that
    /// changed, went or cannot be read is not written.
    fn write(&mut self, i: usize, overwrite: bool) -> Gate {
        let d = &mut self.docs[i];
        if !overwrite && d.changed_on_disk() {
            return Gate::Changed;
        }
        match d.save() {
            Ok(()) => Gate::Saved,
            Err(e) => Gate::Failed(e),
        }
    }

    /// Save every document with edits. One whose file changed on disk is
    /// shown and asks; after the answer the others are saved too. The
    /// status line then says how many were saved and how many were not.
    pub fn save_all(&mut self) {
        if self.question.is_some() {
            return;
        }
        let todo: std::collections::VecDeque<u64> = self
            .docs
            .iter()
            .filter(|d| d.is_dirty())
            .map(|d| d.id)
            .collect();
        if todo.is_empty() {
            self.news = Some((crate::kit::Level::Info, "Nothing to save.".to_owned()));
            return;
        }
        self.run = Some(SaveRun {
            todo,
            ..SaveRun::default()
        });
        self.go_on_saving();
    }

    /// Save the rest of a Save all, up to the next question.
    fn go_on_saving(&mut self) {
        let Some(mut run) = self.run.take() else {
            return;
        };
        while let Some(id) = run.todo.pop_front() {
            let Some(i) = self.index_of(id) else { continue };
            if !self.docs[i].is_dirty() {
                continue;
            }
            match self.write(i, false) {
                Gate::Saved => run.saved += 1,
                Gate::Changed => {
                    self.question = Some(Question::Overwrite { doc: id });
                    self.active = Some(i);
                    self.run = Some(run);
                    return;
                }
                Gate::Failed(e) => {
                    run.kept += 1;
                    run.error.get_or_insert(e);
                }
            }
        }
        let n = count;
        self.news = Some(match (run.kept, run.error) {
            (0, _) => (
                crate::kit::Level::Success,
                format!("Saved {}.", n(run.saved)),
            ),
            (k, e) => {
                let mut s = format!("Saved {}. {} not saved.", n(run.saved), n(k));
                if let Some(e) = e {
                    s = format!("{s} {e}");
                }
                (crate::kit::Level::Warning, s)
            }
        });
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
            Some(i) if self.docs[i].is_dirty() => {
                self.begin_leave(Leave::Close, [doc].into());
            }
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

    fn dirty_ids(&self) -> std::collections::VecDeque<u64> {
        self.docs
            .iter()
            .filter(|d| d.is_dirty())
            .map(|d| d.id)
            .collect()
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
        let todo = self.dirty_ids();
        self.begin_leave(Leave::Quit, todo)
    }

    /// Another project is to open. True if it may open now; otherwise a
    /// question is open about the first document with edits, the same
    /// question quitting asks.
    pub fn request_switch(&mut self) -> bool {
        if self.question.is_some() {
            return false;
        }
        self.switch_allowed = false;
        self.switching = true;
        let todo = self.dirty_ids();
        self.begin_leave(Leave::Switch, todo)
    }

    /// Ask about each of `todo` in turn. Nothing is saved or dropped until
    /// the last answer is in. True if there was nothing to ask.
    fn begin_leave(&mut self, why: Leave, todo: std::collections::VecDeque<u64>) -> bool {
        self.leave = Some(LeaveRun {
            why,
            asked: todo.iter().copied().collect(),
            todo,
            save: Vec::new(),
            saving: false,
            saved: 0,
            last: None,
        });
        self.ask_next()
    }

    /// Ask the next question of the run, or end the run when every answer
    /// is in. True if the run ended and went on.
    fn ask_next(&mut self) -> bool {
        let Some(run) = self.leave.as_mut() else {
            return false;
        };
        while let Some(doc) = run.todo.pop_front() {
            if let Some(i) = self.docs.iter().position(|d| d.id == doc) {
                self.question = Some(match run.why {
                    Leave::Close => Question::CloseTab { doc },
                    Leave::Quit | Leave::Switch => Question::Quit { doc },
                });
                self.active = Some(i);
                return false;
            }
        }
        self.end_leave()
    }

    /// Every answer is in: save what was to be saved, each through the
    /// gate, then drop the rest and go on. A file changed on disk stops the
    /// run at that file and asks Overwrite; the answer goes on or keeps
    /// everything. A failed save stops the run and drops nothing. Files
    /// saved before a stop stay saved.
    fn end_leave(&mut self) -> bool {
        let Some(mut run) = self.leave.take() else {
            return false;
        };
        run.saving = true;
        while let Some(&id) = run.save.first() {
            let Some(i) = self.index_of(id) else {
                run.save.remove(0);
                continue;
            };
            match self.write(i, false) {
                Gate::Saved => {
                    run.save.remove(0);
                    run.saved += 1;
                    run.last = Some(self.docs[i].shown.clone());
                }
                Gate::Changed => {
                    let s = format!(
                        "Saved {}. {} changed on disk; nothing else was saved or closed.",
                        count(run.saved),
                        self.docs[i].shown
                    );
                    self.news = Some((crate::kit::Level::Warning, s));
                    self.question = Some(Question::Overwrite { doc: id });
                    self.active = Some(i);
                    self.leave = Some(run);
                    return false;
                }
                Gate::Failed(e) => {
                    self.switching = false;
                    self.active = Some(i);
                    let e = match run.saved {
                        0 => e,
                        k => format!("Saved {}. {e}", count(k)),
                    };
                    self.report(Err(e));
                    return false;
                }
            }
        }
        match (run.saved, run.last.take()) {
            (0, _) => {}
            (1, Some(shown)) => self.report(Ok(format!("Saved {shown}."))),
            (k, _) => self.report(Ok(format!("Saved {}.", count(k)))),
        }
        for id in run.asked {
            if let Some(i) = self.index_of(id) {
                self.remove(i);
            }
        }
        match run.why {
            Leave::Close => {}
            Leave::Quit => self.quit_allowed = true,
            Leave::Switch => {
                self.switching = false;
                self.switch_allowed = true;
            }
        }
        true
    }

    /// A quit, close or switch stopped while saving: Cancel at the
    /// Overwrite question, or a failed write. Nothing more is saved or
    /// dropped; files saved before the stop stay saved.
    fn leave_stopped(&mut self, error: Option<String>) {
        let Some(run) = self.leave.take() else { return };
        self.switching = false;
        let mut s = format!("Saved {}. Nothing was closed.", count(run.saved));
        let level = match error {
            Some(e) => {
                s = format!("{s} {e}");
                crate::kit::Level::Error
            }
            None => crate::kit::Level::Warning,
        };
        self.news = Some((level, s));
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
        if let Question::Overwrite { .. } = q
            && self.leave.as_ref().is_some_and(|r| r.saving)
        {
            self.question = None;
            if a == Answer::Save {
                match self.write(i, true) {
                    Gate::Saved => {
                        if let Some(run) = self.leave.as_mut() {
                            run.save.retain(|d| *d != doc);
                            run.saved += 1;
                            run.last = Some(self.docs[i].shown.clone());
                        }
                        self.end_leave();
                    }
                    Gate::Failed(e) => self.leave_stopped(Some(e)),
                    Gate::Changed => self.leave_stopped(None),
                }
            } else {
                self.leave_stopped(None);
            }
            return;
        }
        if let Question::Overwrite { .. } = q {
            self.question = None;
            let r = (a == Answer::Save).then(|| match self.write(i, true) {
                Gate::Failed(e) => Err(e),
                _ => Ok(format!("Saved {}.", self.docs[i].shown)),
            });
            match (self.run.as_mut(), r) {
                (Some(run), Some(Ok(_))) => run.saved += 1,
                (Some(run), Some(Err(e))) => {
                    run.kept += 1;
                    run.error.get_or_insert(e);
                }
                (Some(run), None) => run.kept += 1,
                (None, Some(r)) => self.report(r),
                (None, None) => {}
            }
            self.go_on_saving();
            return;
        }
        if a == Answer::Cancel {
            self.question = None;
            self.leave = None;
            self.switching = false;
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
        self.question = None;
        if a == Answer::Save
            && let Some(run) = self.leave.as_mut()
        {
            run.save.push(doc);
        }
        self.ask_next();
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

    /// Draw the tabs in the pane's top line. Tabs that do not fit scroll
    /// sideways, by the wheel over the strip, and the active tab is brought
    /// into view when it changes. A menu at the end then lists them all.
    pub fn tabs(&mut self, kit: &Kit<'_>, ui: &mut egui::Ui, header: Rect) {
        let mut strip = header;
        let mut listed = None;
        if self.tab_overflow {
            strip.max.x = (header.right() - size::SMALL_H - size::UNIT).max(header.left());
            let at = Rect::from_min_max(egui::pos2(strip.right(), header.top()), header.max);
            let mut m = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(at)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            let resp = kit.icon_button(&mut m, Icon::ChevDown, "All tabs", None);
            let labels = tab_labels(&self.docs);
            egui::Popup::menu(&resp)
                .width(size::MENU_POPUP_W)
                .show(|ui| {
                    for (i, l) in labels.iter().enumerate() {
                        if kit.menu_row(ui, l, "", None, size::MENU_POPUP_W).clicked() {
                            listed = Some(i);
                            ui.close();
                        }
                    }
                });
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(strip));
        child.set_clip_rect(strip.intersect(ui.clip_rect()));
        let ui = &mut child;
        let origin = strip.left() - self.tab_scroll;
        let mut x = origin;
        let mut chosen = None;
        let mut close = None;
        let mut active_span = None;
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
            if self.active == Some(i) {
                active_span = Some((r.left() - origin, r.right() - origin));
            }
            x = r.right() + size::TAB_GAP;
            if out.chosen {
                chosen = Some(i);
            }
            if out.close {
                close = Some(d.id);
            }
        }
        let total = x - origin;
        if ui.rect_contains_pointer(strip) {
            let d = ui.input(|i| i.smooth_scroll_delta);
            self.tab_scroll -= d.x + d.y;
        }
        let active_id = self.current().map(|d| d.id);
        let bring = if active_id != self.tab_seen {
            self.tab_seen = active_id;
            active_span
        } else {
            None
        };
        let before = self.tab_scroll;
        self.tab_scroll = tab_scroll(self.tab_scroll, strip.width(), total, bring);
        let overflow = total > header.width();
        if before != self.tab_scroll || overflow != self.tab_overflow {
            ui.ctx().request_repaint();
        }
        self.tab_overflow = overflow;
        if let Some(i) = chosen.or(listed) {
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
            area.min.y = self.find_bar(kit, ui, rect) + size::UNIT;
        }
        // Under a dialog the window draws this disabled and the field has
        // given up its focus: a pending selection waits, since asking for
        // focus now would take it from the dialog.
        let behind = !ui.is_enabled();
        let doc = &mut self.docs[idx];
        let id = Self::text_id(doc.id);
        let focused = ui.memory(|m| m.has_focus(id));
        // A selection set on a field without focus is dropped: ask for focus
        // first, and set the selection once it has it.
        let mut scroll_to = None;
        if let Some((a, b)) = doc.pending_sel
            && !behind
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
        let doc_id = doc.id;
        let is_c = is_c(&doc.path);
        let shown = doc.shown.clone();
        let mut caret = None;
        let line_h = kit.theme.code_line();
        // The gutter stays put across and follows the text down: it is
        // drawn beside the scroll area, from the offset that area reports.
        let gutter = Rect::from_min_max(
            area.min,
            egui::pos2(
                (area.left() + size::GUTTER_W).min(area.right()),
                area.bottom(),
            ),
        );
        let text_area = Rect::from_min_max(egui::pos2(gutter.right(), area.top()), area.max);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(text_area)
                .id_salt(("editor-area", doc_id)),
        );
        let mut seen = None;
        let out = egui::ScrollArea::both()
            .id_salt(("editor-scroll", doc_id))
            .auto_shrink(false)
            .show(&mut child, |ui| {
                let bg = ui.painter().add(egui::Shape::Noop);
                // Code never wraps: a long line scrolls sideways, and row i
                // is line i + 1 for the gutter.
                let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                    let mut job = self.layout_job(kit.theme, doc_id, is_c, buf.as_str());
                    job.wrap.max_width = f32::INFINITY;
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let out = egui::TextEdit::multiline(&mut text)
                    .id(id)
                    .code_editor()
                    .lock_focus(true)
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
                let mut row = None;
                if let Some(c) = cur {
                    let r = g.pos_from_cursor(c.primary);
                    let band = (out.galley_pos.y + r.top(), out.galley_pos.y + r.bottom());
                    painter.set(
                        bg,
                        egui::Shape::rect_filled(
                            Rect::from_min_max(
                                egui::pos2(clip.left(), band.0),
                                egui::pos2(clip.right(), band.1),
                            ),
                            CornerRadius::ZERO,
                            p.raised,
                        ),
                    );
                    row = Some(band);
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
                seen = Some((out.galley.clone(), out.galley_pos.y, row));
            });
        // Line numbers for the rows in view, in the screen place of each
        // row. No wrapping: row i is line i + 1.
        let mut first = None;
        if let Some((g, galley_y, row)) = seen {
            let painter = ui.painter_at(gutter);
            if let Some((top, bottom)) = row {
                painter.rect_filled(
                    Rect::from_min_max(
                        egui::pos2(gutter.left(), top),
                        egui::pos2(gutter.right(), bottom),
                    ),
                    CornerRadius::ZERO,
                    p.raised,
                );
            }
            let view = out.inner_rect;
            let top = view.top() - galley_y;
            let skip = g
                .rows
                .partition_point(|r| r.pos.y + r.rect().height() <= top);
            let font = kit.theme.font(Type::Code);
            for (i, r) in g.rows.iter().enumerate().skip(skip) {
                let y = galley_y + r.pos.y;
                if y > view.bottom() {
                    break;
                }
                let centre = y + (r.rect().height() - line_h) / 2.0 + line_h / 2.0;
                first.get_or_insert((i + 1, centre));
                painter.text(
                    egui::pos2(gutter.right() - size::STEP_12, centre),
                    egui::Align2::RIGHT_CENTER,
                    (i + 1).to_string(),
                    font.clone(),
                    p.dim,
                );
            }
        }
        self.gutter = Some(Gutter {
            rect: gutter,
            offset: out.state.clamped_offset(),
            text_top: out.inner_rect.top(),
            first,
        });
        if text.contains('\r') {
            text = document::to_lf(&text).into_owned();
        }
        let doc = &mut self.docs[idx];
        doc.text = text;
        doc.caret = caret;
    }

    /// Draw the find bar at the top of `area` and return its bottom.
    fn find_bar(&mut self, kit: &Kit<'_>, ui: &mut egui::Ui, area: Rect) -> f32 {
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
        // Find on the first row, replace below it. Each field keeps at
        // least `FIND_MIN`; the buttons that do not fit beside it wrap to
        // further rows, so every control stays inside a narrow editor.
        let gap = ui.spacing().item_spacing.x;
        let button = |t: &str| kit.button_width(ui, &Button::quiet(t).small());
        let tag_w = ui
            .painter()
            .layout_no_wrap(tag.clone(), kit.theme.font(Type::Small), p.dim)
            .size()
            .x;
        let width = area.width();
        let find_rows = pack(
            &[
                size::SMALL_H,
                button("Next"),
                button("Previous"),
                if tag.is_empty() { 0.0 } else { tag_w },
            ],
            width,
            gap,
        );
        let replace_rows = pack(&[button("Replace all"), button("Replace")], width, gap);
        let n_find = find_rows.iter().max().map_or(1, |r| r + 1);
        let n_replace = replace_rows.iter().max().map_or(1, |r| r + 1);
        let row_h = size::CONTROL_H + size::UNIT;
        let mut rows: Vec<egui::Ui> = (0..n_find + n_replace)
            .map(|i| {
                let r = Rect::from_min_size(
                    egui::pos2(area.left(), area.top() + row_h * i as f32),
                    egui::vec2(width, row_h),
                );
                ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(r)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                )
            })
            .collect();
        if kit
            .icon_button(&mut rows[find_rows[0]], Icon::Close, "Close find", None)
            .clicked()
        {
            self.find.open = false;
        }
        let ui = &mut rows[find_rows[1]];
        if kit
            .button(ui, Button::quiet("Next").small().kind(ButtonKind::Outlined))
            .clicked()
        {
            self.find_step(ui.ctx(), true);
        }
        let ui = &mut rows[find_rows[2]];
        if kit.button(ui, Button::quiet("Previous").small()).clicked() {
            self.find_step(ui.ctx(), false);
        }
        if !tag.is_empty() {
            kit.text(
                &mut rows[find_rows[3]],
                &tag,
                Type::Small,
                if none { p.alarm } else { p.dim },
            );
        }
        let ui = &mut rows[n_find + replace_rows[0]];
        if kit
            .button(ui, Button::quiet("Replace all").small())
            .clicked()
        {
            let n = self.replace_all();
            self.news = Some((crate::kit::Level::Info, format!("Replaced {n}.")));
        }
        let ui = &mut rows[n_find + replace_rows[1]];
        if kit.button(ui, Button::quiet("Replace").small()).clicked() {
            self.replace_one(ui.ctx());
        }
        let ui = &mut rows[n_find];
        let rid = Id::unique("find-replace");
        let w = (size::FIND_W / 2.0 + size::STEP_32)
            .max(size::FIND_MIN)
            .min(ui.available_width())
            .max(0.0);
        kit.text_field(
            ui,
            rid,
            &mut self.find.replace,
            "Replace with",
            "Replace with",
            w,
            false,
        );
        let ui = &mut rows[0];
        let fid = Id::unique("find-query");
        let w = size::FIND_W.min(ui.available_width()).max(0.0);
        let r = kit.text_field(ui, fid, &mut self.find.query, "Find", "Find", w, none);
        if std::mem::take(&mut self.find.focus) {
            r.request_focus();
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            self.find_step(ui.ctx(), true);
            r.request_focus();
        }
        area.top() + row_h * (n_find + n_replace) as f32
    }
}

/// The row of each control beside a field, packed right to left in
/// `width` with `gap` between them. Row 0 keeps `FIND_MIN` for the field;
/// a control that does not fit goes to the next row. A zero width is an
/// absent control and takes no room.
fn pack(widths: &[f32], width: f32, gap: f32) -> Vec<usize> {
    let mut row = 0;
    let mut used = size::FIND_MIN;
    widths
        .iter()
        .map(|&w| {
            if w > 0.0 {
                if used + gap + w > width {
                    row += 1;
                    used = w;
                } else {
                    used += gap + w;
                }
            }
            row
        })
        .collect()
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

/// The scroll of a tab strip `view` wide holding tabs `total` wide: kept
/// in range, and moved so that `bring`, a tab's left and right from the
/// strip's start, is in view.
fn tab_scroll(scroll: f32, view: f32, total: f32, bring: Option<(f32, f32)>) -> f32 {
    let mut s = scroll;
    if let Some((l, r)) = bring {
        if r - s > view {
            s = r - view;
        }
        if l < s {
            s = l;
        }
    }
    s.clamp(0.0, (total - view).max(0.0))
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
        e.news = None;
        e.save_all();
        assert!(
            e.news
                .as_ref()
                .is_some_and(|(_, s)| s == "Nothing to save.")
        );
        assert_eq!(std::fs::read(&f).unwrap(), b"theirs\n");
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

    /// Save all asks about each document changed on disk in turn and
    /// goes on after each answer, then counts what it saved.
    #[test]
    fn save_all_goes_on_after_each_question() {
        use super::{Answer, Editor, Question};
        let g = folder("all");
        let names = ["a.txt", "b.txt", "c.txt", "d.txt"];
        let mut e = Editor::default();
        for n in names {
            std::fs::write(g.0.join(n), b"one\n").unwrap();
            e.open(&g.0.join(n), n.into());
        }
        for d in &mut e.docs {
            d.text = "mine\n".into();
        }
        let ids: Vec<u64> = e.docs.iter().map(|d| d.id).collect();
        std::fs::write(g.0.join("a.txt"), b"theirs, longer\n").unwrap();
        std::fs::write(g.0.join("c.txt"), b"theirs, longer\n").unwrap();
        e.save_all();
        assert_eq!(e.question, Some(Question::Overwrite { doc: ids[0] }));
        e.answer(ids[0], Answer::Save);
        assert_eq!(e.question, Some(Question::Overwrite { doc: ids[2] }));
        assert_eq!(std::fs::read(g.0.join("b.txt")).unwrap(), b"mine\n");
        e.answer(ids[2], Answer::Cancel);
        assert_eq!(e.question, None);
        for (n, want) in [
            ("a.txt", &b"mine\n"[..]),
            ("b.txt", b"mine\n"),
            ("c.txt", b"theirs, longer\n"),
            ("d.txt", b"mine\n"),
        ] {
            assert_eq!(std::fs::read(g.0.join(n)).unwrap(), want, "{n}");
        }
        assert_eq!(
            e.news.as_ref().map(|(_, s)| s.as_str()),
            Some("Saved 3 documents. 1 document not saved.")
        );
    }

    /// Three edited documents, each with its own text.
    fn three(tag: &str) -> (Gone, super::Editor, Vec<u64>) {
        let g = folder(tag);
        let mut e = super::Editor::default();
        for n in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(g.0.join(n), b"one\n").unwrap();
            e.open(&g.0.join(n), n.into());
        }
        for d in &mut e.docs {
            d.text = "mine\n".into();
        }
        let ids = e.docs.iter().map(|d| d.id).collect();
        (g, e, ids)
    }

    /// Quit with several edited documents asks about each in turn. Cancel
    /// at any question changes nothing: no file written, no edit dropped.
    /// Once the last answer is in, the saves happen and the window may go.
    #[test]
    fn quit_asks_about_each_document_in_turn() {
        use super::{Answer, Question};
        let (g, mut e, ids) = three("quit");
        assert!(!e.request_quit());
        e.answer(ids[0], Answer::Save);
        assert_eq!(e.question, Some(Question::Quit { doc: ids[1] }));
        e.answer(ids[1], Answer::DoNotSave);
        assert_eq!(e.question, Some(Question::Quit { doc: ids[2] }));
        e.answer(ids[2], Answer::Cancel);
        assert_eq!(e.question, None);
        assert!(!e.quit_allowed);
        assert_eq!(e.docs.len(), 3, "every document stays open");
        assert!(e.docs.iter().all(|d| d.text == "mine\n" && d.is_dirty()));
        for n in ["a.txt", "b.txt", "c.txt"] {
            assert_eq!(std::fs::read(g.0.join(n)).unwrap(), b"one\n", "{n}");
        }
        assert!(!e.request_quit());
        e.answer(ids[0], Answer::Save);
        e.answer(ids[1], Answer::DoNotSave);
        e.answer(ids[2], Answer::DoNotSave);
        assert!(e.quit_allowed);
        assert_eq!(std::fs::read(g.0.join("a.txt")).unwrap(), b"mine\n");
        assert_eq!(std::fs::read(g.0.join("b.txt")).unwrap(), b"one\n");
        assert_eq!(std::fs::read(g.0.join("c.txt")).unwrap(), b"one\n");
    }

    /// A switch asks as quit does: Do not save, then Cancel, drops nothing.
    #[test]
    fn cancel_late_in_a_switch_drops_nothing() {
        use super::Answer;
        let (_g, mut e, ids) = three("switch");
        assert!(!e.request_switch());
        e.answer(ids[0], Answer::DoNotSave);
        e.answer(ids[1], Answer::Cancel);
        assert!(!e.switching && !e.switch_allowed);
        assert_eq!(e.docs.len(), 3);
        assert!(e.docs.iter().all(|d| d.is_dirty()));
    }

    /// Two edited documents, a leave, Save for the first, then the first
    /// file changes or goes, then the last answer. Nothing is written over
    /// the change, nothing is closed, and Overwrite is asked for it.
    fn stopped(tag: &str, quit: bool, remove: bool) -> (Gone, super::Editor, Vec<u64>) {
        use super::{Answer, Question};
        let (g, mut e, ids) = three(tag);
        e.docs.pop();
        assert!(!if quit {
            e.request_quit()
        } else {
            e.request_switch()
        });
        e.answer(ids[0], Answer::Save);
        if remove {
            std::fs::remove_file(g.0.join("a.txt")).unwrap();
        } else {
            std::fs::write(g.0.join("a.txt"), b"theirs, longer\n").unwrap();
        }
        e.answer(ids[1], Answer::Save);
        let on_disk = std::fs::read(g.0.join("a.txt")).ok();
        let want: Option<&[u8]> = (!remove).then_some(b"theirs, longer\n");
        assert_eq!(on_disk.as_deref(), want, "no write over the change");
        assert_eq!(std::fs::read(g.0.join("b.txt")).unwrap(), b"one\n");
        assert!(!e.quit_allowed && !e.switch_allowed);
        assert_eq!(e.docs.len(), 2);
        assert!(e.docs.iter().all(|d| d.text == "mine\n" && d.is_dirty()));
        assert_eq!(e.question, Some(Question::Overwrite { doc: ids[0] }));
        assert!(
            e.news
                .as_ref()
                .is_some_and(|(_, s)| s.starts_with("Saved 0 documents. a.txt changed on disk"))
        );
        (g, e, ids)
    }

    #[test]
    fn a_changed_file_stops_the_quit_and_overwrite_finishes_it() {
        let (g, mut e, ids) = stopped("stop-quit", true, false);
        e.answer(ids[0], super::Answer::Save);
        assert!(e.quit_allowed && e.docs.is_empty());
        assert_eq!(std::fs::read(g.0.join("a.txt")).unwrap(), b"mine\n");
        assert_eq!(std::fs::read(g.0.join("b.txt")).unwrap(), b"mine\n");
    }

    #[test]
    fn a_changed_file_stops_the_switch_and_cancel_keeps_everything() {
        let (g, mut e, ids) = stopped("stop-switch", false, false);
        e.answer(ids[0], super::Answer::Cancel);
        assert!(!e.switch_allowed && !e.switching && e.question.is_none());
        assert_eq!(e.docs.len(), 2);
        assert!(e.docs.iter().all(|d| d.is_dirty()));
        assert_eq!(
            std::fs::read(g.0.join("a.txt")).unwrap(),
            b"theirs, longer\n"
        );
        assert_eq!(std::fs::read(g.0.join("b.txt")).unwrap(), b"one\n");
    }

    #[test]
    fn a_removed_file_stops_the_switch_and_overwrite_finishes_it() {
        let (g, mut e, ids) = stopped("stop-gone", false, true);
        e.answer(ids[0], super::Answer::Save);
        assert!(e.switch_allowed && e.docs.is_empty());
        assert_eq!(std::fs::read(g.0.join("a.txt")).unwrap(), b"mine\n");
    }

    /// Files saved before the stop stay saved, and the status line says so.
    #[test]
    fn files_saved_before_the_stop_stay_saved() {
        use super::{Answer, Question};
        let (g, mut e, ids) = three("stop-later");
        assert!(!e.request_quit());
        e.answer(ids[0], Answer::Save);
        e.answer(ids[1], Answer::Save);
        std::fs::write(g.0.join("b.txt"), b"theirs, longer\n").unwrap();
        e.answer(ids[2], Answer::DoNotSave);
        assert_eq!(std::fs::read(g.0.join("a.txt")).unwrap(), b"mine\n");
        assert_eq!(e.question, Some(Question::Overwrite { doc: ids[1] }));
        assert_eq!(e.docs.len(), 3);
        assert!(
            e.news
                .as_ref()
                .is_some_and(|(_, s)| s.starts_with("Saved 1 document. b.txt changed"))
        );
    }

    /// A write that fails after Overwrite stops the run and drops nothing.
    #[test]
    fn a_failed_save_stops_the_switch() {
        use super::Answer;
        let (g, mut e, ids) = three("switch-fail");
        assert!(!e.request_switch());
        e.answer(ids[0], Answer::DoNotSave);
        e.answer(ids[1], Answer::Save);
        // The folder goes before the last answer: the gate asks, the
        // write then fails.
        std::fs::remove_dir_all(&g.0).unwrap();
        e.answer(ids[2], Answer::DoNotSave);
        e.answer(ids[1], Answer::Save);
        assert!(!e.switch_allowed && !e.switching);
        assert_eq!(e.docs.len(), 3);
        assert!(
            e.news
                .as_ref()
                .is_some_and(|(l, _)| *l == crate::kit::Level::Error)
        );
    }

    /// The active tab is brought into view from either side, and the
    /// scroll stays within the strip.
    #[test]
    fn tab_scroll_brings_the_tab_into_view() {
        use super::tab_scroll;
        assert_eq!(tab_scroll(0.0, 100.0, 500.0, Some((400.0, 450.0))), 350.0);
        assert_eq!(tab_scroll(350.0, 100.0, 500.0, Some((10.0, 60.0))), 10.0);
        assert_eq!(tab_scroll(900.0, 100.0, 500.0, None), 400.0);
        assert_eq!(tab_scroll(-5.0, 100.0, 50.0, None), 0.0);
    }
}
