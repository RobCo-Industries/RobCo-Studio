// SPDX-License-Identifier: GPL-3.0-or-later

//! The window: its strips, its pages, the panes of the Project page, focus
//! from pane to pane, the save question and the pane that adjusts the
//! theme.

use crate::art::{Art, Icon, Mark};
use crate::chooser::{self, Ask, Choice, FolderChooser, SystemChooser};
use crate::editor::{Answer, Editor};
use crate::kit::{Button, ButtonKind, Kit, Level, MenuMark};
use crate::menus::{self, Command, ThemeId};
use crate::project::{Manifest, Project, Row};
use crate::screen::{self, Adapter};
use crate::theme::{Spec, Theme, Type, crt, effect, size};
use eframe::egui::{
    self, CornerRadius, Id, Key, Modifiers, Rect, Sense, Ui, UiBuilder, accesskit::Role,
};
use egui::text_edit::TextEditState;
use std::path::{Path, PathBuf};

mod theme_pane;
mod themes;
pub use theme_pane::{Leaving, Naming, ThemeEditor, ThemeQuestion, ThemeTab};
pub use themes::ThemeFault;

/// The window's pages, chosen in the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// Edit, build and run.
    Project,
    /// The card writer.
    Card,
    /// The device link.
    Device,
    /// The SDK manager.
    Sdk,
}

impl Page {
    /// The page's name, as the pages control shows it.
    pub fn name(self) -> &'static str {
        match self {
            Page::Project => "Project",
            Page::Card => "Card",
            Page::Device => "Device",
            Page::Sdk => "SDK",
        }
    }

    const ALL: [Page; 4] = [Page::Project, Page::Card, Page::Device, Page::Sdk];
}

/// The places focus moves through with F6, in the design's reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The toolbar.
    Toolbar,
    /// The Files pane.
    Files,
    /// The editor pane.
    Editor,
    /// The simulator pane.
    Simulator,
    /// The tape pane.
    Tape,
    /// The bottom pane.
    Bottom,
    /// A page other than Project: its one pane.
    Page,
    /// The footer, named "Status bar" for a screen reader.
    Status,
}

impl Place {
    /// The name in the accessibility tree.
    pub fn name(self) -> &'static str {
        match self {
            Place::Toolbar => "Toolbar",
            Place::Files => "Files",
            Place::Editor => "Editor",
            Place::Simulator => "Simulator",
            Place::Tape => "Tape",
            Place::Bottom => "Bottom pane",
            Place::Page => "Page",
            Place::Status => "Status bar",
        }
    }
}

/// The tabs of the bottom pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    /// Build output.
    Build,
    /// The machine's serial lines.
    Log,
    /// The debugger.
    Debug,
}

/// The log's filters, in the bottom pane's tab row while the Log tab is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogView {
    /// The part of [`LOG_SHOW`] chosen.
    pub show: usize,
    /// The filter field's text.
    pub query: String,
    /// Keep the last line in view.
    pub follow: bool,
}

impl Default for LogView {
    fn default() -> Self {
        Self {
            show: 0,
            query: String::new(),
            follow: true,
        }
    }
}

/// The parts of the log's segmented control.
pub const LOG_SHOW: [&str; 3] = ["All", "Markers", "Text"];

/// The sizes of the Project page's panes, which dividers change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The Files pane's width.
    pub files_w: f32,
    /// The simulator pane's width.
    pub sim_w: f32,
    /// The bottom pane's height.
    pub bottom_h: f32,
    /// The Files pane is shown.
    pub files: bool,
    /// The bottom pane is shown.
    pub bottom: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            files_w: size::FILES_W,
            sim_w: size::SIM_W,
            bottom_h: size::BOTTOM_H,
            files: true,
            bottom: true,
        }
    }
}

/// The window's state.
pub struct Studio {
    /// What draws, worked out from the theme each frame. Its high
    /// legibility is the person's switch.
    pub theme: Theme,
    /// Every theme known, the six that ship first.
    pub themes: Vec<Spec>,
    /// The place of the theme in use in `themes`.
    pub current: usize,
    /// The theme in use, changed and not saved, or None.
    pub changed: Option<Spec>,
    /// For each theme of `themes`, its file in the themes folder, or None
    /// for a theme that ships.
    pub theme_files: Vec<Option<String>>,
    /// Each file of the themes folder that is not a theme, with the line
    /// that says why.
    pub refused: Vec<(String, String)>,
    /// The settings folder and the settings read there. None: Studio
    /// reads and writes no file of its own, as in the tests.
    store: Option<themes::Store>,
    /// The lines about files said this session, each said once.
    said: Vec<String>,
    /// A setting changed: write the settings when no button is held.
    settings_due: bool,
    /// The page shown.
    pub page: Page,
    /// The open folder.
    pub project: Option<Project>,
    /// The editor.
    pub editor: Editor,
    /// Pane sizes.
    pub layout: Layout,
    /// The bottom pane's tab.
    pub bottom_tab: BottomTab,
    /// The log's filters.
    pub log: LogView,
    /// The last message, for the footer. Set it through [`Studio::say`].
    pub status: (Level, String),
    /// The person has had a chance to read the message: an input came in
    /// a frame after the footer showed it.
    status_read: bool,
    /// The footer has drawn the message at least once.
    status_shown: bool,
    /// The last lesser message that [`Studio::say`] kept off the line,
    /// shown as the tooltip of the line's level icon.
    held: Option<(Level, String)>,
    /// Draw the menu row in the window. False where the system shows the menus.
    pub menu_row: bool,
    /// The header is compact at any window size, by the View menu.
    pub compact_header: bool,
    /// The simulator's zoom step, 1 to 3.
    pub sim_zoom: u8,
    /// The largest zoom step the simulator pane has room for, from the
    /// layout of this frame's page, whichever page shows.
    sim_room: u8,
    /// Commands chosen in a menu outside the window, run next frame.
    pub pending: Vec<Command>,
    /// The screen pass, the adapter and reduce motion.
    pub screen: screen::Settings,
    /// The theme editor: open or not, its tab and its fields.
    pub theme_editor: ThemeEditor,
    art: Option<Art>,
    /// What draws this frame, taken in `prepare`.
    drawn: screen::Drawn,
    styled: Option<Theme>,
    fonts: bool,
    focus_request: Option<Place>,
    /// The widget that had keyboard focus when a dialog came to the
    /// front, and a text field's caret and selection, kept to give them
    /// back when the window is in front again. None while nothing is in
    /// front.
    behind_focus: Option<Option<(Id, Option<TextEditState>)>>,
    places: Vec<(Place, Rect)>,
    /// The tape's label field and the name drawn on it, last frame.
    tape_label: Option<(Rect, Rect)>,
    page_rect: Rect,
    title: String,
    chooser: Box<dyn FolderChooser>,
    choosing: Choosing,
    /// What the chooser asks for.
    asking: Ask,
    next_project: Option<Project>,
}

/// Where File, Open folder stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choosing {
    /// No chooser is open.
    Idle,
    /// Open folder was asked: show the chooser at the top of the next frame.
    Due,
    /// The chooser is open.
    Open,
}

const READY: &str = "Ready";

/// The header's caps line, in two lines when the header is tall.
const HEADER_LINES: [&str; 2] = ["Application & system", "Development environment"];

/// The header is tall from 720 points of inner height up, unless a person
/// chose the compact header.
pub fn header_tall(inner_h: f32, compact: bool) -> bool {
    !compact && inner_h >= size::HEADER_TALL_FROM
}

/// Where the header's parts sit, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderLayout {
    /// The wordmark's box.
    pub wordmark: Rect,
    /// The atom's box.
    pub atom: Rect,
    /// The divider's x.
    pub divider_x: f32,
    /// The caps lines, one box around both.
    pub text: Rect,
    /// The skyline's box, None when it gives way.
    pub skyline: Option<Rect>,
}

/// Lay out the header in `rect`, its rule included, for caps lines
/// `text_w` points wide. The skyline stands with its own rule on the
/// header's rule, and gives way when less than the skyline room would stay
/// between it and the caps lines.
pub fn header_layout(rect: Rect, tall: bool, text_w: f32) -> HeaderLayout {
    let rule = rect.bottom() - stroke_w() / 2.0;
    let cy = (rect.top() + rect.bottom() - stroke_w()) / 2.0;
    let (wm, atom) = if tall {
        (Mark::Wordmark, Mark::Atom)
    } else {
        (Mark::WordmarkCompact, Mark::AtomCompact)
    };
    let a = Art::mark_size(atom);
    let atom = Rect::from_min_size(
        egui::pos2(rect.left() + size::HEADER_PAD, cy - a.y / 2.0),
        a,
    );
    let ws = Art::mark_size(wm);
    let wordmark = Rect::from_min_size(
        egui::pos2(atom.right() + size::HEADER_MARK_GAP, cy - ws.y / 2.0),
        ws,
    );
    let divider_x = wordmark.right() + size::HEADER_GAP;
    let tx = divider_x + stroke_w() + size::HEADER_GAP;
    let text_h = if tall {
        crate::theme::text::LABEL_CELL_H * 2.0 + size::HEADER_LINE_GAP
    } else {
        crate::theme::text::LABEL_CELL_H
    };
    let text = Rect::from_min_size(
        egui::pos2(tx, cy - text_h / 2.0),
        egui::vec2(text_w, text_h),
    );
    let skyline = tall
        .then(|| {
            let sky = Art::mark_size(Mark::Skyline);
            let top = rule - size::SKYLINE_RULE_Y * sky.y / size::SKYLINE[1];
            Rect::from_min_size(
                egui::pos2(rect.right() - size::HEADER_MARK_GAP - sky.x, top),
                sky,
            )
        })
        .filter(|r| r.left() - text.right() >= size::SKYLINE_ROOM);
    HeaderLayout {
        wordmark,
        atom,
        divider_x,
        text,
        skyline,
    }
}

/// The window's strips, top to bottom.
#[derive(Debug, Clone, Copy)]
pub struct Strips {
    /// The header band.
    pub header: Rect,
    /// The menu row, where the system has no menu bar.
    pub menu: Option<Rect>,
    /// The toolbar.
    pub toolbar: Rect,
    /// The page.
    pub page: Rect,
    /// The footer.
    pub footer: Rect,
}

/// Cut `full` into the strips: header, menu row, toolbar, page, then the
/// footer. No strip gets a negative side, down to nothing.
pub fn strips(full: Rect, menu_row: bool, tall: bool) -> Strips {
    let full = Rect::from_min_size(full.min, full.size().max(egui::Vec2::ZERO));
    let (top, bottom) = (full.top(), full.bottom());
    let down = |y: f32| y.clamp(top, bottom);
    let band = |a: f32, b: f32| {
        Rect::from_min_max(
            egui::pos2(full.left(), down(a)),
            egui::pos2(full.right(), down(b)),
        )
    };
    let header_h = if tall {
        size::HEADER_H
    } else {
        size::HEADER_COMPACT_H
    };
    let header = band(top, top + header_h);
    let menu = menu_row.then(|| band(header.bottom(), header.bottom() + size::MENU_H));
    let tool_top = menu.map_or(header.bottom(), |m| m.bottom());
    let toolbar = band(tool_top, tool_top + size::TOOLBAR_H);
    let footer = band((bottom - size::FOOTER_H).max(toolbar.bottom()), bottom);
    let page = Rect::from_min_max(
        egui::pos2(full.left(), toolbar.bottom()),
        egui::pos2(full.right(), footer.top()),
    );
    Strips {
        header,
        menu,
        toolbar,
        page,
        footer,
    }
}

impl Studio {
    /// A window on `folder`, or on no project.
    pub fn new(folder: Option<&Path>) -> Self {
        let mut s = Self {
            theme: Theme::default(),
            themes: crate::theme::SHIPPED.to_vec(),
            current: 0,
            changed: None,
            theme_files: vec![None; crate::theme::SHIPPED.len()],
            refused: Vec::new(),
            store: None,
            said: Vec::new(),
            settings_due: false,
            page: Page::Project,
            project: None,
            editor: Editor::default(),
            layout: Layout::default(),
            bottom_tab: BottomTab::Build,
            log: LogView::default(),
            status: (Level::Info, READY.to_owned()),
            status_read: true,
            status_shown: false,
            held: None,
            menu_row: true,
            compact_header: false,
            sim_zoom: 1,
            sim_room: 1,
            pending: Vec::new(),
            screen: screen::Settings::default(),
            theme_editor: ThemeEditor::default(),
            art: None,
            drawn: screen::Drawn {
                theme: Theme::default(),
                strengths: crt::LIVE,
                rate: crt::RATE,
            },
            styled: None,
            fonts: false,
            focus_request: None,
            behind_focus: None,
            places: Vec::new(),
            tape_label: None,
            page_rect: Rect::NOTHING,
            title: String::new(),
            chooser: Box::new(SystemChooser::default()),
            choosing: Choosing::Idle,
            asking: Ask::Folder,
            next_project: None,
        };
        if let Some(f) = folder {
            match Project::open(f) {
                Ok(p) => s.use_project(p),
                Err(e) => s.say(Level::Error, e),
            }
        }
        s
    }

    /// The same window, asking `chooser` for a folder on File, Open folder,
    /// and for a file on Add a theme file.
    pub fn with_chooser(mut self, chooser: Box<dyn FolderChooser>) -> Self {
        self.chooser = chooser;
        self
    }

    /// Put a message on the status line. The only way to set it.
    ///
    /// A message replaces the one shown when its level is the same or
    /// higher, or when the person has had a chance to read the one shown:
    /// an input came in a frame after the footer drew it. Otherwise the
    /// new message is held, and the line's level icon shows it as a
    /// tooltip until the next message takes the line. So at start, or
    /// within one command, an error is never covered by a lesser note.
    pub fn say(&mut self, level: Level, text: String) {
        if rank(level) >= rank(self.status.0) || self.status_read {
            self.status = (level, text);
            self.status_read = false;
            self.status_shown = false;
            self.held = None;
        } else {
            self.held = Some((level, text));
        }
    }

    /// The lesser message held off the status line, if any.
    pub fn held(&self) -> Option<&(Level, String)> {
        self.held.as_ref()
    }

    /// Make `p` the project, with a fresh editor.
    fn use_project(&mut self, p: Project) {
        // A folder that is there but cannot be listed is told the way the
        // tree tells it, not reported as opened.
        match &p.error {
            Some(e) => self.say(Level::Error, e.clone()),
            None => self.say(Level::Info, format!("Opened {}.", p.root.display())),
        }
        self.project = Some(p);
        self.editor = Editor::default();
    }

    /// Show the chooser when it is due, and act on its answer.
    fn choose_folder(&mut self, ctx: &egui::Context) {
        if self.choosing == Choosing::Due {
            self.choosing = Choosing::Open;
            // A menu or a dropdown list open now would stay open, inert,
            // over the scrim.
            egui::Popup::close_all(ctx);
            let start = match self.asking {
                Ask::Folder => {
                    chooser::start_folder(self.project.as_ref().map(|p| p.root.as_path()))
                }
                Ask::ThemeFile => chooser::start_folder(None),
            };
            self.chooser.start(ctx, start.as_deref(), self.asking);
        }
        if self.choosing != Choosing::Open {
            return;
        }
        let Some(answer) = self.chooser.poll() else {
            return;
        };
        self.choosing = Choosing::Idle;
        match answer {
            Choice::Cancelled => {}
            Choice::Failed(e) => self.say(Level::Error, e),
            Choice::Chosen(f) if self.asking == Ask::ThemeFile => {
                if let Err(e) = self.add_theme_file(&f) {
                    self.say(Level::Error, e.message());
                }
            }
            Choice::Chosen(f) => match Project::open(&f) {
                Err(e) => self.say(Level::Error, e),
                Ok(p) => {
                    self.editor.switch_allowed = false;
                    self.next_project = Some(p);
                    self.editor.request_switch();
                    self.finish_switch();
                }
            },
        }
    }

    /// Open the project that waits once every question is answered, or
    /// drop it when one was cancelled.
    fn finish_switch(&mut self) {
        let Some(p) = self.next_project.take() else {
            return;
        };
        if self.editor.switch_allowed {
            self.use_project(p);
        } else if self.editor.switching {
            self.next_project = Some(p);
        }
    }

    /// Why the menu item `it` cannot act now, or None. The scale items
    /// follow the simulator pane's room, as its Larger button does. A note
    /// of View, Theme cannot be picked, and says itself.
    pub fn unavailable(&self, it: &menus::Item) -> Option<String> {
        if let Verdict::Refused(why) = verdict(it.command, self.front()) {
            return Some(why.to_owned());
        }
        if let Command::ThemeHeld(h) = it.command {
            return Some(crate::theme::held_note(h).to_owned());
        }
        match it.command {
            Command::SimZoom(z) => zoom_unavailable(z, self.sim_room),
            _ => it.unavailable.map(str::to_owned),
        }
    }

    /// The mark the in-window menu draws for command `c`: its kind by
    /// [`menus::check`], its state by [`Studio::checked`], as the macOS
    /// menu bar shows it.
    pub fn menu_mark(&self, c: Command) -> MenuMark {
        match menus::check(c) {
            menus::Check::Plain => MenuMark::None,
            menus::Check::Toggle => MenuMark::Check(self.checked(c)),
            menus::Check::Choice(_) => MenuMark::Radio(self.checked(c)),
        }
    }

    /// Whether the menu item of command `c` shows its mark now, by
    /// [`menus::check`]. False for an item with no state. The theme in use
    /// is marked; while it is changed, its changed row carries the mark.
    pub fn checked(&self, c: Command) -> bool {
        match c {
            Command::PageProject => self.page == Page::Project,
            Command::PageCard => self.page == Page::Card,
            Command::PageDevice => self.page == Page::Device,
            Command::PageSdk => self.page == Page::Sdk,
            Command::ToggleFiles => self.layout.files,
            Command::ToggleBottom => self.layout.bottom,
            Command::CompactHeader => self.compact_header,
            Command::HighLegibility => self.theme.high_legibility,
            Command::SimZoom(z) => self.sim_zoom == z,
            Command::Theme(ThemeId::At(i)) => self.changed.is_none() && self.current == i,
            Command::Theme(ThemeId::Changed) => self.changed.is_some(),
            _ => false,
        }
    }

    /// The theme in use: the changed one, else the saved one.
    pub fn spec(&self) -> &Spec {
        match &self.changed {
            Some(c) => c,
            None => self
                .themes
                .get(self.current)
                .unwrap_or(&crate::theme::SHIPPED[0]),
        }
    }

    /// The name of the theme in use, as the menu and the status line give
    /// it: "Green", or "Green, changed".
    pub fn theme_name(&self) -> String {
        match &self.changed {
            Some(c) => format!("{}, changed", c.name),
            None => self.spec().name.to_string(),
        }
    }

    /// What the theme in use draws on this machine now, and what holds it.
    pub fn shown(&self) -> screen::Shown {
        screen::shown(
            self.spec(),
            self.screen.adapter,
            self.screen.reduce_motion,
            self.theme.high_legibility,
        )
    }

    /// The menus, with the rows of View, Theme made from the themes now.
    pub fn menus(&self) -> Vec<menus::Menu> {
        let held = self.shown().held;
        menus::menus_with(menus::theme_items(
            &self.themes,
            self.changed.as_ref(),
            &held,
        ))
    }

    /// Change the theme in use by `edit`. The first change makes it a
    /// changed theme, not saved, and the status line says so once. Studio
    /// writes nothing.
    pub fn edit_theme(&mut self, edit: impl FnOnce(&mut Spec)) {
        let mut spec = self.spec().clone();
        edit(&mut spec);
        if spec == *self.spec() {
            return;
        }
        if self.changed.is_none() {
            self.say(Level::Info, crate::theme::changed_news(&spec.name));
        }
        self.changed = Some(spec);
        self.settings_due = true;
        self.refresh_theme();
    }

    /// Pick the theme at place `i` of the list, from the menu or the
    /// editor's dropdown. While a changed theme is not saved, this asks
    /// first, through [`Studio::leave_for`].
    pub fn pick_theme(&mut self, i: usize) {
        if i >= self.themes.len() {
            return;
        }
        let to = Leaving::Theme(self.key(i));
        // A pick of a listed theme cannot fail.
        let _ = self.leave_for(to);
    }

    /// Take the theme at place `i` of the list. A changed theme goes. Only
    /// [`Studio::leave_for`] and Revert call this.
    fn take_theme(&mut self, i: usize) {
        if i >= self.themes.len() {
            return;
        }
        self.current = i;
        self.changed = None;
        self.refresh_theme();
        self.save_settings();
        let name = self.spec().name.to_string();
        self.say(Level::Info, crate::theme::news(&name, None));
    }

    /// Work out what draws from the theme in use, the machine and high
    /// legibility. The frame takes it at its start.
    pub(crate) fn refresh_theme(&mut self) {
        let shown = self.shown();
        self.theme.set_base(self.spec().base);
        self.theme.screen = shown.screen;
        self.theme.strengths = shown.strengths;
    }

    /// Start the screen, by the rules: a theme asked for on the command
    /// line, by its place among the themes that ship, the adapter, the
    /// system's reduce-motion setting. With eframe's render state the pass
    /// is plugged into `ctx` and draws: call this before the first frame.
    pub fn start_screen(
        &mut self,
        ctx: &egui::Context,
        adapter: Adapter,
        gpu: Option<&eframe::egui_wgpu::RenderState>,
        reduce_motion: Option<bool>,
        asked: Option<usize>,
    ) {
        self.screen.plug(ctx, gpu);
        self.choose_screen(adapter, reduce_motion, asked);
    }

    /// Start the screen by the rules of [`Studio::start_screen`], for a
    /// window whose screen is plugged in already. When something holds
    /// the theme, the status line says so.
    pub fn choose_screen(
        &mut self,
        adapter: Adapter,
        reduce_motion: Option<bool>,
        asked: Option<usize>,
    ) {
        self.screen.adapter = adapter;
        self.screen.reduce_motion = reduce_motion == Some(true);
        if let Some(i) = asked.filter(|i| *i < self.themes.len()) {
            self.current = i;
            self.changed = None;
        }
        self.refresh_theme();
        // A window with no known adapter is a test harness or has no
        // wgpu: it says nothing about an adapter it does not know.
        let first = self
            .shown()
            .first()
            .filter(|h| *h != screen::Held::NoAdapter || adapter == Adapter::Software);
        if let Some(h) = first {
            let name = self.theme_name();
            self.say(Level::Info, crate::theme::news(&name, Some(h)));
        }
    }

    /// What is in front now. The one place that decides it: the dialogs,
    /// the scrim, the keys and the menus' availability all ask here. The
    /// window shows at most one dialog of its own, and a question beats
    /// the theme editor: the editor waits, unchanged, behind it and comes back after the
    /// last answer.
    pub fn front(&self) -> Front {
        if self.choosing() {
            Front::Chooser
        } else {
            self.dialog_shown()
        }
    }

    /// Take keyboard focus from the window when something comes to the
    /// front, and give it back, caret and selection as they were, when
    /// the window is in front again.
    fn hold_focus(&mut self, ctx: &egui::Context, front: Front) {
        if front == Front::Nothing {
            if let Some(Some((id, field))) = self.behind_focus.take() {
                ctx.memory_mut(|m| m.request_focus(id));
                if let Some(st) = field {
                    st.store(ctx, id);
                }
            }
        } else if self.behind_focus.is_none() {
            // A text field without focus drops its selection, and its undo
            // history records that: keep the field's state as it was.
            let had = ctx.memory(|m| m.focused());
            if let Some(id) = had {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
            self.behind_focus = Some(had.map(|id| (id, TextEditState::load(ctx, id))));
        }
    }

    /// The window's own dialog that is drawn: the question, else the theme
    /// editor.
    /// Under the chooser it is drawn too, held with the rest.
    fn dialog_shown(&self) -> Front {
        if self.editor.question.is_some() || self.theme_editor.question.is_some() {
            Front::Question
        } else if self.theme_editor.open {
            Front::Theme
        } else {
            Front::Nothing
        }
    }

    /// The folder chooser is open or about to open.
    pub fn choosing(&self) -> bool {
        self.choosing != Choosing::Idle
    }

    /// The Tape pane's label field and the rectangle of the name drawn on
    /// it, as last drawn. None when no tape is shown.
    pub fn tape_label(&self) -> Option<(Rect, Rect)> {
        self.tape_label
    }

    /// Where `place` was drawn in the last frame, if it was.
    pub fn place_rect(&self, place: Place) -> Option<Rect> {
        self.places
            .iter()
            .find(|(p, _)| *p == place)
            .map(|(_, r)| *r)
    }

    /// Open a file of the project in the editor.
    pub fn open_file(&mut self, path: &Path) {
        let shown = self
            .project
            .as_ref()
            .map_or_else(|| path.display().to_string(), |p| p.shown(path));
        self.editor.open(path, shown);
    }

    fn visible_places(&self) -> Vec<Place> {
        let mut v = vec![Place::Toolbar];
        if self.page == Page::Project {
            if self.layout.files {
                v.push(Place::Files);
            }
            v.extend([Place::Editor, Place::Simulator, Place::Tape]);
            if self.layout.bottom {
                v.push(Place::Bottom);
            }
        } else {
            v.push(Place::Page);
        }
        v.push(Place::Status);
        v
    }

    /// The place that holds the focused widget, as of the last frame.
    pub fn focused_place(&self, ctx: &egui::Context) -> Option<Place> {
        let r = ctx
            .memory(|m| m.focused())
            .and_then(|id| ctx.read_response(id))?;
        self.places
            .iter()
            .find(|(_, pr)| pr.contains(r.rect.center()))
            .map(|(p, _)| *p)
    }

    fn move_focus(&mut self, ctx: &egui::Context, forward: bool) {
        let order = self.visible_places();
        let n = order.len();
        let next = match self
            .focused_place(ctx)
            .and_then(|p| order.iter().position(|q| *q == p))
        {
            Some(i) if forward => order[(i + 1) % n],
            Some(i) => order[(i + n - 1) % n],
            None if forward => order[0],
            None => order[n - 1],
        };
        self.focus_request = Some(next);
    }

    /// Run a menu command, from the window's menus, a shortcut or the
    /// native menu bar. A command runs after this frame's `prepare` and may
    /// change what it checked: the theme, high legibility, the page. So every command asks for the next frame, which
    /// draws its effect even when nothing moves and no input comes.
    pub fn run(&mut self, ctx: &egui::Context, c: Command) {
        ctx.request_repaint();
        // One table decides which command acts over what is in front.
        if let Verdict::Refused(why) = verdict(c, self.front()) {
            self.say(Level::Info, why.to_owned());
            return;
        }
        // The editor keeps one sentence: pass on the last command's before
        // this one can overwrite it, so the line's rule weighs both.
        if let Some((level, text)) = self.editor.news.take() {
            self.say(level, text);
        }
        match c {
            Command::OpenFolder => {
                self.choosing = Choosing::Due;
                self.asking = Ask::Folder;
            }
            Command::ThemeAdd => match self.themes_folder() {
                Some(_) => {
                    self.choosing = Choosing::Due;
                    self.asking = Ask::ThemeFile;
                }
                None => self.say(Level::Error, themes::ThemeFault::NoFolder.message()),
            },
            Command::Save => self.editor.save(),
            Command::SaveAll => self.editor.save_all(),
            Command::CloseTab => {
                if let Some(d) = self.editor.current() {
                    let id = d.id;
                    self.editor.close(id);
                }
            }
            Command::Quit => {
                if self.editor.request_quit() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Command::Find => {
                self.page = Page::Project;
                self.editor.find.open = true;
                self.editor.find.focus = true;
            }
            Command::FindNext => self.editor.find_step(ctx, true),
            Command::FindPrevious => self.editor.find_step(ctx, false),
            Command::PageProject => self.page = Page::Project,
            Command::PageCard => self.page = Page::Card,
            Command::PageDevice => self.page = Page::Device,
            Command::PageSdk => self.page = Page::Sdk,
            Command::ToggleFiles => self.layout.files = !self.layout.files,
            Command::ToggleBottom => self.layout.bottom = !self.layout.bottom,
            Command::ResetLayout => self.layout = Layout::default(),
            Command::CompactHeader => {
                self.compact_header = !self.compact_header;
                self.settings_due = true;
            }
            Command::SimZoom(z) => match zoom_unavailable(z, self.sim_room) {
                Some(why) => self.say(Level::Info, why),
                None => self.sim_zoom = z.clamp(1, 3),
            },
            Command::NextPane => self.move_focus(ctx, true),
            Command::PreviousPane => self.move_focus(ctx, false),
            Command::Theme(ThemeId::At(i)) => self.pick_theme(i),
            Command::ThemeEdit => self.theme_editor.open = true,
            Command::HighLegibility => {
                self.theme.high_legibility = !self.theme.high_legibility;
                self.settings_due = true;
                self.refresh_theme();
            }
            c => {
                if let Some(name) = menus::not_yet(c) {
                    self.say(Level::Info, menus::not_yet_news(name));
                }
            }
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let mut cmds = Vec::new();
        ctx.input_mut(|i| {
            let mut k = |m: Modifiers, key: Key, c: Command| {
                if i.consume_key(m, key) {
                    cmds.push(c);
                }
            };
            // Shift first: a key without modifiers also matches one with Shift.
            k(Modifiers::SHIFT, Key::F6, Command::PreviousPane);
            k(Modifiers::NONE, Key::F6, Command::NextPane);
            k(Modifiers::COMMAND, Key::Num1, Command::PageProject);
            k(Modifiers::COMMAND, Key::Num2, Command::PageCard);
            k(Modifiers::COMMAND, Key::Num3, Command::PageDevice);
            k(Modifiers::COMMAND, Key::Num4, Command::PageSdk);
            k(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::F,
                Command::ToggleFiles,
            );
            // After Cmd+Shift+F, which Cmd+F would also match.
            k(Modifiers::COMMAND, Key::F, Command::Find);
            k(Modifiers::SHIFT, Key::F3, Command::FindPrevious);
            k(Modifiers::NONE, Key::F3, Command::FindNext);
            // Cmd+Alt+S first: Cmd+S would also match it.
            k(
                Modifiers::COMMAND | Modifiers::ALT,
                Key::S,
                Command::SaveAll,
            );
            k(Modifiers::COMMAND, Key::S, Command::Save);
            k(Modifiers::COMMAND, Key::W, Command::CloseTab);
            k(Modifiers::COMMAND, Key::J, Command::ToggleBottom);
            k(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::L,
                Command::HighLegibility,
            );
            k(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::T,
                Command::ThemeEdit,
            );
            k(Modifiers::COMMAND, Key::O, Command::OpenFolder);
            k(Modifiers::COMMAND, Key::Q, Command::Quit);
        });
        cmds.append(&mut self.pending);
        for c in cmds {
            self.run(ctx, c);
        }
        if self.page == Page::Project && matches!(self.front(), Front::Nothing | Front::Question) {
            self.editor.keys(ctx);
        }
    }

    fn prepare(&mut self, ctx: &egui::Context) {
        // The one value that draws this frame. A command or a slider later
        // in the frame changes the theme for the next one.
        self.refresh_theme();
        self.drawn = self.screen.drawn(self.theme, self.spec().rate);
        let theme = self.drawn.theme;
        if !self.fonts {
            Theme::install_fonts(ctx);
            self.fonts = true;
        }
        if self.styled != Some(theme) {
            theme.apply(ctx);
            self.styled = Some(theme);
        }
        let ppp = ctx.pixels_per_point();
        // The art holds the display scale and glow state it was made
        // with, so a level changed in the middle of a frame is seen here,
        // at the start of the next one, whatever changed it.
        if !self.art.as_ref().is_some_and(|a| a.fits(ppp, &theme)) {
            self.art = Some(Art::new(ctx, &theme));
        }
        let title = match &self.project {
            Some(p) => format!("{} - RobCo Studio", p.name()),
            None => "RobCo Studio".to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// What draws the current frame: the theme, its screen and the
    /// strengths, as taken at the frame's start.
    pub fn drawn(&self) -> screen::Drawn {
        self.drawn
    }

    /// The window's title as last sent: the project's name, then the
    /// application's.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Draw one frame of the window into `ui`.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // A message the footer has shown counts as read once the person
        // acts: a key, a click or text.
        if self.status_shown && !self.status_read {
            self.status_read = ctx.input(|i| {
                i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Key { pressed: true, .. }
                            | egui::Event::PointerButton { pressed: true, .. }
                            | egui::Event::Text(_)
                    )
                })
            });
        }
        self.choose_folder(&ctx);
        self.prepare(&ctx);
        // While the folder chooser is open the window stays: the chooser
        // answers first, as with every other command.
        if ctx.input(|i| i.viewport().close_requested())
            && (self.choosing() || !self.editor.request_quit())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        self.keys(&ctx);
        // A setting changed: write it once no button is held, so a slider
        // being dragged does not write.
        if self.settings_due && !ctx.input(|i| i.pointer.any_down()) {
            self.save_settings();
        }
        // Whatever is in front, the window behind is inert, to the pointer
        // and to the keyboard alike: it is drawn disabled, so none of its
        // widgets takes a click or joins the Tab order, and the widget
        // that had keyboard focus gives it up until the window is in
        // front again. Under the chooser the input is held too, see the
        // end of this function.
        let front = self.front();
        self.hold_focus(&ctx, front);
        if front != Front::Nothing {
            let opacity = ui.opacity();
            ui.disable();
            // Under the window's own dialogs the window keeps its look:
            // the scrim alone shades it. Under the chooser it is dimmed.
            if front != Front::Chooser {
                ui.set_opacity(opacity);
            }
        }
        let Some(art) = self.art.take() else {
            return;
        };
        let theme = self.drawn.theme;
        let kit = Kit {
            theme: &theme,
            art: &art,
        };
        self.places.clear();
        let full = ui.max_rect();
        let p = kit.p();
        ui.painter().rect_filled(full, CornerRadius::ZERO, p.ground);

        let tall = header_tall(ctx.content_rect().height(), self.compact_header);
        let st = strips(full, self.menu_row, tall);
        let (tool, page, status) = (st.toolbar, st.page, st.footer);
        self.page_rect = page;
        self.fit_sim(page);
        self.header(&kit, ui, st.header, tall);
        if let Some(r) = st.menu {
            self.menu_bar(&kit, ui, r);
        }
        self.toolbar(&kit, ui, tool);
        let mut page_ui = kit.region(ui, page, "Page", Role::Main, p.ground, false);
        match self.page {
            Page::Project => self.project_page(&kit, &mut page_ui, page),
            other => self.later_page(&kit, &mut page_ui, page, other),
        }
        self.status_bar(&kit, ui, status);
        // One dialog at most, the one in front, under one scrim. It is
        // taken once, so an Escape that answers a question does not also
        // close the theme editor waiting behind it.
        let front = self.front();
        match front {
            // No shade under the theme editor: the window behind is its
            // preview. A click there says why it does nothing.
            Front::Theme => {
                if scrim(&ctx, full, kit.theme.scrim_theme(), true) {
                    self.say(Level::Info, ADJUSTING.to_owned());
                }
            }
            Front::Question | Front::Chooser => {
                scrim(&ctx, full, kit.theme.scrim(), false);
            }
            Front::Nothing => {}
        }
        match self.dialog_shown() {
            Front::Question => self.dialog(&kit, &ctx, full),
            Front::Theme => self.theme_pane(&kit, &ctx, front == Front::Theme),
            Front::Chooser | Front::Nothing => {}
        }
        if self.front() != front {
            // The theme editor comes back, or the focus goes back to the window, on
            // the next frame: ask for it.
            ctx.request_repaint();
        }
        self.finish_switch();
        if art.has_scanlines() {
            art.scanlines(
                &ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Debug,
                    Id::unique("scanlines"),
                )),
                full,
            );
        }
        if let Some((level, text)) = self.editor.news.take() {
            self.say(level, text);
            // The footer is drawn already: a command from the native
            // menu brings no input, so ask for the frame that shows it.
            ctx.request_repaint();
        }
        if self.editor.quit_allowed {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.focus_request = None;
        self.art = Some(art);
        // The one rule for every area while the chooser is open or due:
        // the next frame's input is held before egui sees it.
        self.screen.hold_input(self.choosing());
        self.screen.end_frame(&ctx, &self.drawn);
    }

    /// The header band: atom, wordmark, divider, the caps line and the
    /// skyline over a rule. One image for a screen reader, and no focus.
    fn header(&mut self, kit: &Kit<'_>, ui: &mut Ui, rect: Rect, tall: bool) {
        let p = kit.p();
        let painter = ui.painter().with_clip_rect(rect);
        let rule = rect.bottom() - stroke_w() / 2.0;
        painter.hline(rect.x_range(), rule, kit.theme.frame(p.frame));
        let band = Rect::from_min_max(
            rect.min,
            egui::pos2(rect.right(), rect.bottom() - stroke_w()),
        );
        let cy = band.center().y;
        let (wm, atom, div_h) = if tall {
            (Mark::Wordmark, Mark::Atom, size::HEADER_DIVIDER_H)
        } else {
            (
                Mark::WordmarkCompact,
                Mark::AtomCompact,
                size::HEADER_DIVIDER_COMPACT_H,
            )
        };
        let track = crate::theme::text::LABEL_TRACKING;
        let line = format!("{} {}", HEADER_LINES[0], HEADER_LINES[1]);
        let text_w = if tall {
            HEADER_LINES
                .iter()
                .map(|l| kit.caps_width_tracked(ui, l, track))
                .fold(0.0, f32::max)
        } else {
            kit.caps_width_tracked(ui, &line, track)
        };
        let l = header_layout(rect, tall, text_w);
        kit.art.mark(&painter, wm, l.wordmark.min, p.bright);
        kit.art.mark(&painter, atom, l.atom.min, p.bright);
        painter.vline(
            l.divider_x,
            egui::Rangef::new(cy - div_h / 2.0, cy + div_h / 2.0),
            kit.theme.frame(p.frame),
        );
        let tx = l.text.left();
        if tall {
            let step = (crate::theme::text::LABEL_CELL_H + size::HEADER_LINE_GAP) / 2.0;
            for (line, y) in [(HEADER_LINES[0], cy - step), (HEADER_LINES[1], cy + step)] {
                kit.caps_styled(&painter, egui::pos2(tx, y), line, p.bright, track, true);
            }
        } else {
            kit.caps_styled(&painter, egui::pos2(tx, cy), &line, p.bright, track, true);
        }
        if let Some(sky) = l.skyline {
            kit.art.mark(&painter, Mark::Skyline, sky.min, p.bright);
        }
        let resp = ui.interact(band, Id::unique("header"), Sense::hover());
        resp.widget_info(|| egui::WidgetInfo::labeled(Role::Image, true, "RobCo Studio"));
    }

    fn menu_bar(&mut self, kit: &Kit<'_>, ui: &mut Ui, rect: Rect) {
        let p = kit.p();
        let bar = kit.region(ui, rect, "Menu bar", Role::MenuBar, p.ground, false);
        let mac = cfg!(target_os = "macos");
        let mut chosen = None;
        let mut x = rect.left() + size::UNIT;
        for m in self.menus() {
            let w = kit.caps_width(&bar, m.title) + size::LABEL_PAD * 2.0;
            let r = Rect::from_min_size(
                egui::pos2(x, rect.center().y - size::MENU_ITEM_H / 2.0),
                egui::vec2(w, size::MENU_ITEM_H),
            );
            x = r.right();
            let resp = bar.interact(r, Id::unique(("menu", m.title)), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(Role::MenuItem, true, m.title));
            let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
            let cr = kit.theme.control_radius();
            let fg = if open {
                bar.painter().rect_filled(r, cr, p.bright);
                p.ink
            } else {
                if resp.hovered() {
                    bar.painter().rect_filled(r, cr, p.raised);
                }
                p.bright
            };
            kit.caps_styled(
                bar.painter(),
                egui::pos2(r.left() + size::LABEL_PAD, r.center().y),
                m.title,
                fg,
                0.0,
                !open && !resp.hovered(),
            );
            if resp.has_focus() {
                kit.focus_ring(bar.painter(), r);
            }
            egui::Popup::menu(&resp)
                .width(size::MENU_POPUP_W)
                .show(|ui| {
                    let mut i = 0;
                    while i < m.items.len() {
                        let Some(group) = m.items[i].group else {
                            self.menu_item(kit, ui, &m.items[i], mac, &mut chosen);
                            i += 1;
                            continue;
                        };
                        let start = i;
                        while i < m.items.len() && m.items[i].group == Some(group) {
                            i += 1;
                        }
                        let row = kit.menu_row(ui, group, SUBMENU, None, size::MENU_POPUP_W);
                        egui::containers::menu::SubMenu::new().show(ui, &row, |ui| {
                            for it in &m.items[start..i] {
                                self.menu_item(kit, ui, it, mac, &mut chosen);
                            }
                        });
                    }
                });
        }
        bar.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            kit.theme.frame(p.frame_dim),
        );
        if let Some(c) = chosen {
            self.run(ui.ctx(), c);
        }
    }

    /// One item of a menu row's popup, with its separator before it.
    fn menu_item(
        &self,
        kit: &Kit<'_>,
        ui: &mut Ui,
        it: &menus::Item,
        mac: bool,
        chosen: &mut Option<Command>,
    ) {
        if it.separator {
            ui.separator();
        }
        if let Command::ThemeHeld(_) = it.command {
            kit.menu_note(ui, &it.text, size::MENU_POPUP_W);
            return;
        }
        let note = match it.shortcut {
            Some(s) => s.shown(mac),
            None if it.unsaved => menus::NOT_SAVED.to_owned(),
            None => String::new(),
        };
        let why = self.unavailable(it);
        let row = kit.menu_choice_unsaved(
            ui,
            &it.text,
            &note,
            self.menu_mark(it.command),
            why.as_deref(),
            size::MENU_POPUP_W,
            it.unsaved,
        );
        if row.clicked() {
            *chosen = Some(it.command);
            ui.close();
        }
    }

    fn toolbar(&mut self, kit: &Kit<'_>, ui: &mut Ui, rect: Rect) {
        let p = kit.p();
        let focus = self.focus_request == Some(Place::Toolbar);
        let mut t = kit.region(
            ui,
            rect,
            Place::Toolbar.name(),
            Role::Toolbar,
            p.ground,
            focus,
        );
        self.places.push((Place::Toolbar, rect));
        let inner = rect.shrink2(egui::vec2(size::STEP_12, 0.0));
        let mut row = t.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 0.0;
        let target = match self.project.as_ref().map(|pr| &pr.manifest) {
            Some(Manifest::Read { target, .. }) => target.name(),
            _ => "None".to_owned(),
        };
        let track = crate::theme::text::LABEL_TRACKING;
        let label = |row: &mut Ui, text: &str| {
            let x = row.cursor().left();
            let r = kit.caps_styled(
                row.painter(),
                egui::pos2(x, rect.center().y),
                text,
                p.bright,
                track,
                true,
            );
            row.add_space(r.width() + size::TOOLBAR_GAP);
        };
        let divider = |row: &mut Ui| {
            row.add_space(size::TOOLBAR_DIVIDER_GAP);
            let x = row.cursor().left();
            row.painter().vline(
                x,
                egui::Rangef::new(
                    rect.center().y - size::TOOLBAR_DIVIDER_H / 2.0,
                    rect.center().y + size::TOOLBAR_DIVIDER_H / 2.0,
                ),
                kit.theme.frame(p.frame),
            );
            row.add_space(stroke_w() + size::TOOLBAR_DIVIDER_GAP);
        };
        let names = Page::ALL.map(Page::name);
        let now = Page::ALL
            .iter()
            .position(|&pg| pg == self.page)
            .unwrap_or(0);
        if let Some(i) = kit.segmented_tabs(&mut row, "Pages", &names, now) {
            self.page = Page::ALL[i];
        }
        divider(&mut row);
        label(&mut row, "Target:");
        kit.dropdown(
            &mut row,
            "Target",
            Icon::Wrist,
            &target,
            target == "None",
            size::TARGET_DROPDOWN_W,
            &[target.as_str()],
        );
        let mut acts = t.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    rect.min,
                    egui::pos2(rect.right() - size::TOOLBAR_GAP, rect.bottom()),
                ))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        acts.spacing_mut().item_spacing.x = size::TOOLBAR_GAP;
        let mut first = rect.right();
        for (text, icon, c) in [
            ("Write card", Icon::WriteCard, Command::WriteCard),
            ("Debug", Icon::Debug, Command::Debug),
            ("Run", Icon::Run, Command::Run),
            ("Build", Icon::Build, Command::Build),
        ] {
            let resp = kit.button(
                &mut acts,
                Button::quiet(text)
                    .kind(ButtonKind::Filled)
                    .icon(icon)
                    .width(size::TOOLBAR_ACTION_W),
            );
            first = first.min(resp.rect.left());
            if resp.clicked() {
                self.pending.push(c);
            }
        }
        // A divider sets the action group apart from the target, as one
        // stands between the pages and the target.
        let x = first - size::TOOLBAR_DIVIDER_GAP - stroke_w();
        t.painter().vline(
            x,
            egui::Rangef::new(
                rect.center().y - size::TOOLBAR_DIVIDER_H / 2.0,
                rect.center().y + size::TOOLBAR_DIVIDER_H / 2.0,
            ),
            kit.theme.frame(p.frame),
        );
    }

    /// The footer: the Industries mark, the division, the status line,
    /// then the SDK, Sim, Link and Position segments, then Settings and
    /// Help at the right end.
    fn status_bar(&mut self, kit: &Kit<'_>, ui: &mut Ui, rect: Rect) {
        let p = kit.p();
        let focus = self.focus_request == Some(Place::Status);
        let mut s = kit.region(
            ui,
            rect,
            Place::Status.name(),
            Role::Status,
            p.ground,
            focus,
        );
        self.places.push((Place::Status, rect));
        s.painter().hline(
            rect.x_range(),
            rect.top() + stroke_w() / 2.0,
            kit.theme.frame(p.frame),
        );
        let cy = rect.center().y + stroke_w() / 2.0;
        let ms = Art::mark_size(Mark::Industries);
        let mark = kit.art.mark(
            s.painter(),
            Mark::Industries,
            egui::pos2(rect.left() + size::HEADER_MARK_GAP, cy - ms.y / 2.0),
            p.bright,
        );
        let m = s.interact(mark, Id::unique("footer-mark"), Sense::hover());
        m.widget_info(|| egui::WidgetInfo::labeled(Role::Image, true, "RobCo Industries"));
        let caret = (self.page == Page::Project
            && self.focused_place(ui.ctx()) == Some(Place::Editor))
        .then(|| self.editor.current().and_then(|d| d.caret))
        .flatten();
        // Right to left, from the corner.
        let mut segs: Vec<(Option<Icon>, String)> = Vec::new();
        if let Some((l, c)) = caret {
            segs.push((None, format!("Ln {l}, Col {c}")));
        }
        segs.push((Some(Icon::Cable), "Link off".to_owned()));
        segs.push((Some(Icon::Wrist), "Sim off".to_owned()));
        segs.push((None, "SDK not checked".to_owned()));
        let wants: Vec<f32> = segs
            .iter()
            .map(|(i, t)| kit.status_segment_width(&s, *i, t))
            .collect();
        // The division goes first, then Position, then the rest from
        // the left.
        let give: Vec<usize> = (0..segs.len()).rev().collect();
        let (give, soft) = if caret.is_some() {
            let mut g = vec![0];
            g.extend(give.iter().copied().filter(|&i| i != 0));
            (g, 1)
        } else {
            (give, 0)
        };
        let division = "Engineering division";
        let div_w = kit.caps_width(&s, division) + size::FOOTER_SEG_PAD * 2.0 + stroke_w() * 2.0;
        let choosing = (Level::Info, CHOOSING.to_owned());
        let (level, text) = if self.choosing() {
            &choosing
        } else {
            &self.status
        };
        let msg_need = size::FOOTER_SEG_PAD
            + size::ICON
            + size::UNIT * 2.0
            + s.painter()
                .layout_no_wrap(text.clone(), kit.theme.font(Type::Small), p.text)
                .size()
                .x;
        // Settings and Help hold the right end: the segments give way
        // before them.
        let icons_w = size::FOOTER_SEG_PAD * 2.0 + size::SMALL_H * 2.0 + size::UNIT;
        let icons_left = (rect.right() - icons_w).max(rect.left());
        let lay = status_layout(
            mark.right() + size::FOOTER_SEG_PAD,
            icons_left,
            Some(div_w),
            &wants,
            &give,
            soft,
            msg_need,
        );
        for ((i, t), x) in segs.iter().zip(&lay.segs) {
            if let Some((l, r)) = *x {
                let r = Rect::from_min_max(egui::pos2(l, rect.top()), egui::pos2(r, rect.bottom()));
                kit.status_segment(&mut s, r, *i, t);
            }
        }
        if let Some((x, _)) = lay.division {
            kit.footer_divider(s.painter(), x, cy);
            let x = x + stroke_w() + size::FOOTER_SEG_PAD;
            let r = kit.caps(s.painter(), x, cy, division, p.dim);
            s.put(
                r,
                egui::Label::new(egui::RichText::new(division).color(egui::Color32::TRANSPARENT)),
            );
            kit.footer_divider(s.painter(), r.right() + size::FOOTER_SEG_PAD, cy);
        }
        kit.footer_divider(s.painter(), icons_left, cy);
        s.scope_builder(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    egui::pos2(icons_left + size::FOOTER_SEG_PAD, rect.top()),
                    rect.right_bottom(),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = size::UNIT;
                kit.icon_button(ui, Icon::Settings, "Settings", Some(menus::LATER));
                kit.icon_button(ui, Icon::Help, "Help", Some(menus::LATER));
            },
        );
        let icon = Rect::from_min_size(
            egui::pos2(lay.msg.0 + size::FOOTER_SEG_PAD, rect.top()),
            egui::vec2(size::ICON, rect.height()),
        );
        kit.art
            .icon(s.painter(), icon, level.icon(), level.color(&p));
        let msg = Rect::from_min_max(
            egui::pos2((icon.right() + size::UNIT * 2.0).min(lay.msg.1), rect.top()),
            egui::pos2(lay.msg.1, rect.bottom()),
        );
        s.scope_builder(
            UiBuilder::new()
                .max_rect(msg)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(text)
                            .font(kit.theme.font(Type::Small))
                            .color(level.color(&p)),
                    )
                    .truncate(),
                );
            },
        );
        if !self.choosing() {
            self.status_shown = true;
            if let Some((_, held)) = &self.held {
                s.interact(icon, Id::unique("status-held"), Sense::hover())
                    .on_hover_text(held);
            }
        }
    }

    fn project_page(&mut self, kit: &Kit<'_>, ui: &mut Ui, page: Rect) {
        let Panes {
            area,
            files,
            editor,
            sim,
            tape,
            bottom,
            show_files,
            show_bottom,
        } = panes(&mut self.layout, page);
        let left_right = editor.right();

        if show_files {
            self.files_pane(kit, ui, files);
        }
        self.editor_pane(kit, ui, editor);
        let fr = self.focus_request;
        self.sim_pane(kit, ui, sim, fr == Some(Place::Simulator));
        self.places.push((Place::Simulator, sim));
        self.tape_pane(kit, ui, tape, fr == Some(Place::Tape));
        self.places.push((Place::Tape, tape));
        if show_bottom {
            self.bottom_pane(kit, ui, bottom);
        }

        // Dividers, in the gaps between panes.
        let l = &mut self.layout;
        if show_files {
            let r = Rect::from_min_max(
                egui::pos2(files.right(), files.top()),
                egui::pos2(editor.left(), files.bottom()),
            );
            let d = kit.divider(
                ui,
                Id::unique("divider-files"),
                r,
                true,
                "Divider between Files and Editor",
            );
            l.files_w += d.delta;
            if d.reset {
                l.files_w = size::FILES_W;
            }
        }
        let r = Rect::from_min_max(
            egui::pos2(editor.right(), area.top()),
            egui::pos2(sim.left(), area.bottom()),
        );
        let d = kit.divider(
            ui,
            Id::unique("divider-sim"),
            r,
            true,
            "Divider between Editor and Simulator",
        );
        l.sim_w -= d.delta;
        if d.reset {
            l.sim_w = size::SIM_W;
        }
        if show_bottom {
            let mid = (editor.bottom() + bottom.top()) / 2.0;
            let r = Rect::from_center_size(
                egui::pos2((area.left() + left_right) / 2.0, mid),
                egui::vec2(left_right - area.left(), size::DIVIDER),
            );
            let d = kit.divider(
                ui,
                Id::unique("divider-bottom"),
                r,
                false,
                "Divider above the bottom pane",
            );
            l.bottom_h -= d.delta;
            if d.reset {
                l.bottom_h = size::BOTTOM_H;
            }
        }
    }

    /// Work out the simulator's room from the layout of `page`, on every
    /// page, so the scale items follow a resize made on any page. A chosen
    /// step that no longer fits becomes the largest that does, at once,
    /// and the status line says so once.
    fn fit_sim(&mut self, page: Rect) {
        let mut l = self.layout;
        self.sim_room = sim_fit(panes(&mut l, page).sim);
        if self.sim_zoom > self.sim_room {
            let was = self.sim_zoom;
            self.sim_zoom = self.sim_room;
            self.say(
                Level::Info,
                format!(
                    "{was}x no longer fits. The simulator is at {}x.",
                    self.sim_room
                ),
            );
        }
    }

    /// The simulator pane: the device, idle at 62 percent with the empty
    /// state in its screen opening, the honest line, and Stop, Reset, Fit
    /// and the zoom group.
    fn sim_pane(&mut self, kit: &Kit<'_>, ui: &mut Ui, rect: Rect, focus: bool) {
        let p = kit.p();
        let fit = self.sim_room;
        let zoom = self.sim_zoom;
        let mut new_zoom = zoom;
        kit.pane(ui, rect, Place::Simulator.name(), true, focus, |ui, r| {
            let below = Rect::from_min_max(
                egui::pos2(r.outer.left(), r.row.bottom() + stroke_w()),
                r.outer.max,
            );
            let d = Art::device_size(zoom);
            let at = egui::pos2(rect.center().x - d.x / 2.0, below.top() + size::SIM_ROW_GAP);
            let painter = ui.painter().with_clip_rect(below.intersect(ui.clip_rect()));
            let device = kit.art.device(&painter, at, zoom, p.bright.gamma_multiply(effect::DEVICE_IDLE));
            let z = f32::from(zoom);
            let screen = Rect::from_min_size(
                device.min + egui::Vec2::from(size::DEVICE_SCREEN_AT) * z,
                egui::Vec2::from(size::DEVICE_SCREEN) * z,
            );
            painter.rect_filled(screen, CornerRadius::ZERO, p.ground);
            let run = Button::quiet("Run").kind(ButtonKind::Filled).icon(Icon::Run);
            if kit
                .empty_state(
                    ui,
                    screen,
                    Icon::Wrist,
                    "No machine is running. Run builds the tape, boots it in the emulator and shows the screen here.",
                    &[run],
                )
                .iter()
                .any(|r| r.clicked())
            {
                self.pending.push(Command::Run);
            }
            let line = Rect::from_min_size(
                egui::pos2(
                    rect.left() + size::PAD_SIDE,
                    device.bottom() + size::SIM_ROW_GAP,
                ),
                egui::vec2(
                    (rect.width() - size::PAD_SIDE * 2.0).max(0.0),
                    crate::theme::text::BODY_LINE,
                ),
            );
            let mut l = ui.new_child(
                UiBuilder::new()
                    .max_rect(line)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            l.spacing_mut().item_spacing.x = size::UNIT;
            let (ir, _) = l.allocate_exact_size(egui::vec2(size::ICON, size::ICON), Sense::hover());
            kit.art.icon(l.painter(), ir, Icon::Info, p.dim);
            l.add(
                egui::Label::new(
                    egui::RichText::new(HONEST)
                        .font(kit.theme.font(Type::Tiny))
                        .color(p.dim),
                )
                .truncate(),
            );
            let row = Rect::from_min_size(
                egui::pos2(
                    rect.center().x - size::SIM_ROW_W / 2.0,
                    line.bottom() + size::SIM_ROW_GAP,
                ),
                egui::vec2(size::SIM_ROW_W, size::SMALL_H),
            );
            let stop = Button::quiet("Stop").icon(Icon::Stop).small().unavailable(menus::LATER);
            let reset = Button::quiet("Reset").icon(Icon::Reset).small().unavailable(menus::LATER);
            let fit_b = Button::quiet("Fit").icon(Icon::Fit).small();
            let widths = [
                kit.button_width(ui, &stop),
                kit.button_width(ui, &reset),
                kit.button_width(ui, &fit_b),
                size::SMALL_H * 2.0 + size::ZOOM_NAME_W,
            ];
            let gaps = (widths.len() - 1) as f32;
            let gap = ((row.width() - widths.iter().sum::<f32>()) / gaps).max(0.0);
            let mut b = ui.new_child(
                UiBuilder::new()
                    .max_rect(row)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            b.spacing_mut().item_spacing.x = gap;
            kit.button(&mut b, stop);
            kit.button(&mut b, reset);
            if kit.button(&mut b, fit_b).clicked() {
                new_zoom = fit;
            }
            let smaller = (zoom <= 1).then_some(menus::LATER);
            let next = zoom + 1;
            let no_room = zoom_unavailable(next, fit);
            let larger = if zoom >= 3 {
                Some(menus::LATER)
            } else {
                no_room.as_deref()
            };
            let (less, more) = kit.zoom_group(&mut b, &format!("{zoom}x"), smaller, larger);
            if less {
                new_zoom = zoom - 1;
            }
            if more {
                new_zoom = next;
            }
        });
        self.sim_zoom = new_zoom;
    }

    /// The Tape pane: the tape drawing with its name on the label field,
    /// and four facts. Export sits in the title row.
    fn tape_pane(&mut self, kit: &Kit<'_>, ui: &mut Ui, rect: Rect, focus: bool) {
        let p = kit.p();
        let read = self.project.as_ref().and_then(|pr| match &pr.manifest {
            Manifest::Read { name, target, kind } => {
                Some((name.clone(), target.needs(), kind.clone()))
            }
            _ => None,
        });
        let pending = &mut self.pending;
        let tape_label = &mut self.tape_label;
        *tape_label = None;
        kit.pane(ui, rect, Place::Tape.name(), true, focus, |ui, r| {
            let mut t = ui.new_child(
                UiBuilder::new()
                    .max_rect(Rect::from_min_max(
                        r.row.min,
                        egui::pos2(r.row.right() - size::UNIT, r.row.bottom()),
                    ))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            kit.button(
                &mut t,
                Button::quiet("Export")
                    .icon(Icon::Export)
                    .small()
                    .unavailable(menus::LATER),
            );
            let Some((name, target, kind)) = read else {
                let build = Button::quiet("Build")
                    .kind(ButtonKind::Filled)
                    .icon(Icon::Build)
                    .small();
                let out = kit.empty_state(
                    ui,
                    r.content,
                    Icon::Tape,
                    "No tape yet. Build makes one.",
                    &[build],
                );
                if out.iter().any(egui::Response::clicked) {
                    pending.push(Command::Build);
                }
                return;
            };
            let art = egui::Vec2::from(size::TAPE_ART);
            let at = egui::pos2(r.content.left(), r.content.center().y - art.y / 2.0);
            let painter = ui.painter().with_clip_rect(r.outer);
            let tape = kit.art.mark(
                &painter,
                Mark::Tape,
                at,
                p.bright.gamma_multiply(effect::TAPE_IDLE),
            );
            let label = Rect::from_min_size(
                tape.min + egui::Vec2::from(size::TAPE_LABEL_AT),
                egui::Vec2::from(size::TAPE_LABEL),
            );
            // The label holds the name alone, as a written label would:
            // the longest name fits, a test says so. The file name is in the
            // tooltip and the screen reader name: the Kind row has no room.
            let file = format!("{name}.HTP");
            let w = kit.caps_width(ui, &name);
            let drawn = kit.caps(
                &painter.with_clip_rect(label.intersect(r.outer)),
                label.center().x - w / 2.0,
                label.center().y,
                &name,
                p.dim,
            );
            *tape_label = Some((label, drawn));
            ui.interact(label, Id::unique("tape-label"), Sense::hover())
                .on_hover_text(&file)
                .widget_info(|| egui::WidgetInfo::labeled(Role::Image, true, &file));
            let facts = Rect::from_min_max(
                egui::pos2(tape.right() + size::TAPE_FACTS_GAP, tape.top()),
                egui::pos2(r.content.right(), tape.bottom()),
            );
            let mut f = ui.new_child(
                UiBuilder::new()
                    .max_rect(facts)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            f.spacing_mut().item_spacing.y = 0.0;
            f.spacing_mut().interact_size.y = size::ROW_FACT;
            let rows = [
                (Icon::Tape, "Kind:", kind.as_str(), p.text),
                (Icon::Wrist, "Needs:", target.as_str(), p.text),
                (Icon::Level, "Slot:", "No tape yet", p.text),
                (Icon::Warn, "Seal:", "Not built", p.warn),
            ];
            for (icon, key, value, colour) in rows {
                f.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = size::UNIT * 2.0;
                    let (rr, _) = ui.allocate_exact_size(
                        egui::vec2(size::ICON, size::ROW_FACT),
                        Sense::hover(),
                    );
                    kit.art.icon(
                        ui.painter(),
                        rr,
                        icon,
                        if colour == p.warn { p.warn } else { p.bright },
                    );
                    let kw = kit.caps_width(ui, key);
                    let (kr, _) =
                        ui.allocate_exact_size(egui::vec2(kw, size::ROW_FACT), Sense::hover());
                    kit.caps_styled(
                        ui.painter(),
                        egui::pos2(kr.left(), kr.center().y),
                        key,
                        p.bright,
                        0.0,
                        true,
                    );
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(value)
                                .font(kit.theme.font(Type::Small))
                                .color(colour),
                        )
                        .truncate(),
                    );
                });
            }
        });
    }

    fn files_pane(&mut self, kit: &Kit<'_>, ui: &mut Ui, rect: Rect) {
        let fr = self.focus_request == Some(Place::Files);
        let mut open_file: Option<PathBuf> = None;
        let mut toggle: Option<(PathBuf, Option<bool>)> = None;
        let mut focus_row: Option<Id> = None;
        let project = self.project.as_ref();
        kit.pane(ui, rect, Place::Files.name(), true, fr, |ui, r| {
            let Some(pr) = project else {
                kit.empty_state(ui, r.content, Icon::Folder, "No files yet.", &[]);
                return;
            };
            let area = Rect::from_min_max(
                egui::pos2(r.outer.left() + stroke_w(), r.content.top()),
                egui::pos2(r.outer.right() - stroke_w(), r.content.bottom()),
            );
            let mut child = ui.new_child(UiBuilder::new().max_rect(area).id_salt("files-rows"));
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(&mut child, |ui| {
                    // The project itself is the root row; the folder's
                    // rows hang under it while it is open.
                    let root = Row {
                        path: pr.root.clone(),
                        name: pr.name(),
                        depth: 0,
                        is_dir: true,
                        open: pr.root_open,
                    };
                    let shown: Vec<&Row> = std::iter::once(&root)
                        .chain(pr.rows.iter().filter(|_| pr.root_open))
                        .collect();
                    let ids: Vec<Id> = shown
                        .iter()
                        .map(|row| Id::unique(("tree", &row.path)))
                        .collect();
                    for (i, row) in shown.iter().enumerate() {
                        let depth = if i == 0 { 0 } else { row.depth + 1 };
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), size::ROW_TREE),
                            Sense::hover(),
                        );
                        let icon = if row.name == "robco.toml" {
                            Icon::Manifest
                        } else if row.name.ends_with(".c") || row.name.ends_with(".h") {
                            Icon::FileC
                        } else {
                            Icon::File
                        };
                        let selected = pr.selected.as_ref() == Some(&row.path);
                        let resp = kit.tree_row(
                            ui,
                            ids[i],
                            rect,
                            depth,
                            &row.name,
                            icon,
                            row.is_dir.then_some(row.open),
                            selected,
                        );
                        let act = resp.clicked()
                            || (resp.has_focus() && ui.input(|k| k.key_pressed(Key::Enter)));
                        if act {
                            if row.is_dir {
                                toggle = Some((row.path.clone(), None));
                            } else {
                                open_file = Some(row.path.clone());
                            }
                        }
                        if resp.has_focus() {
                            ui.input(|k| {
                                if k.key_pressed(Key::ArrowDown) && i + 1 < ids.len() {
                                    focus_row = Some(ids[i + 1]);
                                }
                                if k.key_pressed(Key::ArrowUp) && i > 0 {
                                    focus_row = Some(ids[i - 1]);
                                }
                                if row.is_dir && k.key_pressed(Key::ArrowRight) {
                                    toggle = Some((row.path.clone(), Some(true)));
                                }
                                if row.is_dir && k.key_pressed(Key::ArrowLeft) {
                                    toggle = Some((row.path.clone(), Some(false)));
                                }
                            });
                        }
                    }
                });
        });
        self.places.push((Place::Files, rect));
        if let Some(id) = focus_row {
            // The tree moves the focus itself: egui's own arrow move
            // would step a second row.
            ui.memory_mut(|m| {
                m.request_focus(id);
                m.move_focus(egui::FocusDirection::None);
            });
        }
        let mut error = None;
        if let Some(pr) = self.project.as_mut() {
            if let Some((path, to)) = toggle {
                match to {
                    Some(o) => pr.set_open(&path, o),
                    None => pr.toggle(&path),
                }
                pr.selected = Some(path);
                error = pr.error.clone();
            }
            if let Some(path) = &open_file {
                pr.selected = Some(path.clone());
            }
        }
        if let Some(e) = error {
            self.say(Level::Error, e);
        }
        if let Some(path) = open_file {
            self.open_file(&path);
        }
    }

    fn editor_pane(&mut self, kit: &Kit<'_>, ui: &mut Ui, rect: Rect) {
        let fr = self.focus_request == Some(Place::Editor);
        let has_docs = !self.editor.docs.is_empty();
        let banner = match self.project.as_ref().map(|p| &p.manifest) {
            Some(Manifest::Missing) => Some((
                Level::Warning,
                "This folder has no robco.toml, so Studio cannot build it.".to_owned(),
            )),
            Some(Manifest::Refused(e) | Manifest::Unreadable(e)) => Some((
                Level::Error,
                format!("Studio cannot read this folder's robco.toml: {e}"),
            )),
            _ => None,
        };
        let no_project = self.project.is_none();
        let editor = &mut self.editor;
        let mut open_folder = false;
        kit.pane(ui, rect, Place::Editor.name(), !has_docs, fr, |ui, r| {
            if has_docs {
                editor.tabs(kit, ui, r.header);
            }
            let mut body = r.content;
            if let Some((level, text)) = &banner {
                let mut b = ui.new_child(UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
                let other = Button::quiet("Open another folder");
                open_folder |= kit.banner(&mut b, *level, text, Some(other)).is_some_and(|r| r.clicked());
                body.min.y = b.min_rect().bottom() + size::UNIT * 2.0;
            }
            if no_project {
                let new = Button::quiet("New project").kind(ButtonKind::Filled).icon(Icon::Add).unavailable(menus::LATER);
                let open = Button::quiet("Open folder").icon(Icon::Folder);
                let r = kit.empty_state(ui, body, Icon::Tape, "No project is open. Make one from a template, or open a folder that holds a robco.toml.", &[new, open]);
                open_folder |= r.get(1).is_some_and(|r| r.clicked());
            } else if !has_docs {
                kit.empty_state(ui, body, Icon::File, "No file is open. Choose one in Files.", &[]);
            } else {
                editor.body(kit, ui, body);
            }
        });
        // The same command as File, Open folder, run next frame.
        if open_folder {
            self.pending.push(Command::OpenFolder);
            ui.ctx().request_repaint();
        }
        self.places.push((Place::Editor, rect));
    }

    fn bottom_pane(&mut self, kit: &Kit<'_>, ui: &mut Ui, rect: Rect) {
        let fr = self.focus_request == Some(Place::Bottom);
        let mut tab = self.bottom_tab;
        let log = &mut self.log;
        let pending = &mut self.pending;
        kit.pane(ui, rect, Place::Bottom.name(), false, fr, |ui, r| {
            let mut x = r.header.left();
            for (t, name, icon) in [
                (BottomTab::Build, "Build", Icon::Build),
                (BottomTab::Log, "Log", Icon::Terminal),
                (BottomTab::Debug, "Debug", Icon::Debug),
            ] {
                let (tr, out) = kit.tab(
                    ui,
                    egui::pos2(x, r.header.center().y),
                    ("bottom", name),
                    name,
                    Some(icon),
                    tab == t,
                    true,
                    false,
                );
                x = tr.right() + size::TAB_GAP;
                if out.chosen {
                    tab = t;
                }
            }
            if tab == BottomTab::Log {
                // The log's filters stand in the tab row, at the right.
                let at = Rect::from_min_max(
                    egui::pos2(x + size::STEP_12, r.row.top()),
                    egui::pos2(r.row.right() - size::TAB_INSET, r.row.bottom()),
                );
                let mut row = ui.new_child(
                    UiBuilder::new()
                        .max_rect(at)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                );
                row.spacing_mut().item_spacing.x = size::UNIT * 2.0;
                let follow = Button::quiet("Follow").small();
                let follow = if log.follow {
                    follow.kind(ButtonKind::Filled).icon(Icon::Ok)
                } else {
                    follow
                };
                if kit.button(&mut row, follow).clicked() {
                    log.follow = !log.follow;
                }
                kit.text_field(
                    &mut row,
                    Id::unique("log-filter"),
                    &mut log.query,
                    "Filter the log",
                    "Filter",
                    size::LOG_FILTER_W,
                    false,
                );
                if let Some(i) = kit.segmented(&mut row, "log-show", &LOG_SHOW, log.show) {
                    log.show = i;
                }
            }
            match tab {
                BottomTab::Build => {
                    kit.empty_state(
                        ui,
                        r.content,
                        Icon::Terminal,
                        "Build output appears here.",
                        &[],
                    );
                }
                BottomTab::Log => {
                    kit.empty_state(
                        ui,
                        r.content,
                        Icon::Terminal,
                        "The machine's serial lines appear here.",
                        &[],
                    );
                }
                BottomTab::Debug => {
                    let b = Button::quiet("Debug")
                        .kind(ButtonKind::Filled)
                        .icon(Icon::Debug)
                        .small();
                    let out = kit.empty_state(
                        ui,
                        r.content,
                        Icon::Debug,
                        "Debug starts a run with the debugger attached.",
                        &[b],
                    );
                    if out.iter().any(egui::Response::clicked) {
                        pending.push(Command::Debug);
                    }
                }
            }
        });
        self.bottom_tab = tab;
        self.places.push((Place::Bottom, rect));
    }

    fn later_page(&mut self, kit: &Kit<'_>, ui: &mut Ui, page: Rect, which: Page) {
        let area = page_area(page);
        let fr = self.focus_request == Some(Place::Page);
        kit.pane(ui, area, which.name(), true, fr, |ui, r| match which {
            Page::Card => {
                kit.empty_state(ui, r.content, Icon::Card, "The card writer comes in a later release of Studio. Nothing here reads or writes a disk.", &[]);
            }
            Page::Device => {
                kit.empty_state(ui, r.content, Icon::Cable, "The device link comes in a later release of Studio. Nothing here opens a port.", &[]);
            }
            _ => {
                let b = Button::quiet("Fetch index").icon(Icon::Sdk).unavailable(menus::LATER);
                kit.empty_state(ui, r.content, Icon::Sdk, "Studio has not fetched the package index yet.", &[b]);
            }
        });
        self.places.push((Place::Page, area));
    }

    fn dialog(&mut self, kit: &Kit<'_>, ctx: &egui::Context, full: Rect) {
        if self.theme_editor.question.is_some() {
            self.theme_dialog(kit, ctx, full);
            return;
        }
        let (Some(q), Some(text)) = (self.editor.question, self.editor.question_text()) else {
            return;
        };
        let p = kit.p();
        let w = size::DIALOG_W;
        let h = size::DIALOG_H;
        let rect = Rect::from_center_size(full.center(), egui::vec2(w, h));
        let mut answer = None;
        egui::Area::new(Id::unique("question"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .accessible_name(text.clone())
            .show(ctx, |ui| {
                ui.set_min_size(rect.size());
                if let Some(h) = kit.theme.halo() {
                    ui.painter().add(h.as_shape(rect, CornerRadius::ZERO));
                }
                ui.painter().rect_filled(rect, CornerRadius::ZERO, p.pane);
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::ZERO,
                    kit.theme.frame(p.bright),
                    egui::StrokeKind::Inside,
                );
                let inner = rect.shrink(size::PAD_SIDE + size::UNIT);
                let title =
                    Rect::from_min_size(inner.min, egui::vec2(inner.width(), size::CONTROL_H));
                kit.art.icon(
                    ui.painter(),
                    Rect::from_min_size(title.min, egui::vec2(size::ICON, title.height())),
                    Icon::Save,
                    p.bright,
                );
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(Rect::from_min_max(
                            egui::pos2(title.left() + size::ICON + size::UNIT * 2.0, title.top()),
                            title.right_bottom(),
                        ))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        kit.text(ui, &text, Type::BodyBold, p.text);
                    },
                );
                let row = Rect::from_min_max(
                    egui::pos2(inner.left(), inner.bottom() - size::CONTROL_H),
                    inner.right_bottom(),
                );
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(row)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    |ui| {
                        let overwrite = matches!(q, crate::editor::Question::Overwrite { .. });
                        let yes = if overwrite { "Overwrite" } else { "Save" };
                        if kit
                            .button(ui, Button::quiet(yes).kind(ButtonKind::Filled))
                            .clicked()
                        {
                            answer = Some(Answer::Save);
                        }
                        let cancel = kit.button(ui, Button::quiet("Cancel"));
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
                        if !overwrite && kit.button(ui, Button::quiet("Do not save")).clicked() {
                            answer = Some(Answer::DoNotSave);
                        }
                    },
                );
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            answer = Some(Answer::Cancel);
        }
        if let Some(a) = answer {
            self.editor.answer(q.doc(), a);
            if self.editor.quit_allowed {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

/// Why zoom `step` cannot be chosen when the simulator pane has room for
/// `fit`, or None when it fits. The pane's Larger button and the
/// Simulator menu's scale items both ask this.
pub fn zoom_unavailable(step: u8, fit: u8) -> Option<String> {
    (step > fit).then(|| format!("{step}x does not fit this pane. Give the simulator the centre."))
}

/// The scrim over the whole window in `fill`, which takes every click
/// under it. With `tells` it reports a click, which it never takes as
/// focus; otherwise it is inert. Returns whether it was clicked.
fn scrim(ctx: &egui::Context, full: Rect, fill: egui::Color32, tells: bool) -> bool {
    egui::Area::new(Id::unique("scrim"))
        .order(egui::Order::Middle)
        .fixed_pos(full.min)
        .show(ctx, |ui| {
            ui.painter().rect_filled(full, CornerRadius::ZERO, fill);
            if tells {
                ui.allocate_rect(full, Sense::CLICK).clicked()
            } else {
                // Disabled, after its fill: it still covers every layer
                // behind from the pointer, and it takes no keyboard focus.
                ui.disable();
                ui.allocate_rect(full, Sense::click());
                false
            }
        })
        .inner
}

/// The title of the theme editor.
pub const THEME_TITLE: &str = "Theme";
/// The note of a row that opens a submenu.
const SUBMENU: &str = ">";

/// The weight of a level on the status line: plain news, a warning, an
/// error.
fn rank(l: Level) -> u8 {
    match l {
        Level::Success | Level::Info => 0,
        Level::Warning => 1,
        Level::Error => 2,
    }
}

/// What is in front of the window: see [`Studio::front`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Front {
    /// The folder chooser is open or due. It beats everything.
    Chooser,
    /// A question about unsaved edits. It beats the theme editor.
    Question,
    /// The theme editor, a pane at the right of the page. The window
    /// behind it is its preview and takes no input.
    Theme,
    /// No dialog: the window acts.
    Nothing,
}

/// Whether a command acts over what is in front: see [`verdict`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The command acts, by its own availability rules.
    Acts,
    /// The command does nothing, for this reason.
    Refused(&'static str),
}

/// What a command is to the dialogs in front.
#[derive(Clone, Copy)]
enum Part {
    /// Quit: it may raise its question in front of the theme editor.
    Quit,
    /// Open folder: it brings the chooser, which holds the editor as it is.
    OpenFolder,
    /// A row of View, Theme, and View, High legibility: what the theme
    /// editor is about.
    Theme,
    /// Any other command, which acts on the window behind a dialog.
    Window,
}

/// The one table of which command acts over what is in front. `run`
/// asks it before anything else, for every source of a command, and
/// `unavailable` asks it, so the menus, the macOS menu bar and the
/// tooltips show what `run` does. A dialog's own controls are not
/// commands: they belong to the dialog and act while it is in front.
///
/// - The chooser: nothing acts.
/// - A question: nothing acts, a second Quit included; only the
///   question's own answers do, and Escape cancels it.
/// - The theme editor: Quit, Open folder, High legibility and the rows
///   of View, Theme act.
/// - Nothing: every command acts by its own rules.
pub fn verdict(c: Command, front: Front) -> Verdict {
    use Command as C;
    let role = match c {
        C::Quit => Part::Quit,
        C::OpenFolder => Part::OpenFolder,
        C::Theme(_)
        | C::ThemeEdit
        | C::ThemeAdd
        | C::ThemeFolder
        | C::ThemeHeld(_)
        | C::HighLegibility => Part::Theme,
        C::NewProject
        | C::OpenRecent
        | C::Save
        | C::SaveAll
        | C::CloseTab
        | C::OpenInEditor
        | C::Settings
        | C::Undo
        | C::Redo
        | C::CutCopyPaste
        | C::Find
        | C::FindNext
        | C::FindPrevious
        | C::GoToLine
        | C::PageProject
        | C::PageCard
        | C::PageDevice
        | C::PageSdk
        | C::ToggleFiles
        | C::ToggleBottom
        | C::SimulatorPlace
        | C::ResetLayout
        | C::CompactHeader
        | C::NextPane
        | C::PreviousPane
        | C::TextLarger
        | C::TextSmaller
        | C::TextReset
        | C::ProjectSettings
        | C::NewFile
        | C::Export
        | C::ShowFolder
        | C::Build
        | C::Run
        | C::Debug
        | C::Stop
        | C::RunTests
        | C::NextError
        | C::PreviousError
        | C::Breakpoint
        | C::WriteCard
        | C::ResetMachine
        | C::KeyLegend
        | C::SimZoom(_)
        | C::InsertTape
        | C::EjectTape
        | C::Continue
        | C::Step
        | C::Break
        | C::GiveKeysBack
        | C::Connect
        | C::SaveLog
        | C::PushAndRun
        | C::Documentation
        | C::SdkCheck
        | C::SystemReport
        | C::About => Part::Window,
    };
    match (front, role) {
        (Front::Chooser, _) => Verdict::Refused(CHOOSING),
        (Front::Question, _) => Verdict::Refused(ASKING),
        (Front::Theme, Part::Window) => Verdict::Refused(ADJUSTING),
        (Front::Theme, Part::Quit | Part::OpenFolder | Part::Theme) => Verdict::Acts,
        (Front::Nothing, _) => Verdict::Acts,
    }
}

/// Why a command waits while a question is open.
pub const ASKING: &str = "Answer the open question first.";
/// Why a command waits while the theme editor is open.
pub const ADJUSTING: &str = "Close the theme editor first.";

/// The status line while the folder chooser is open.
pub const CHOOSING: &str = "A folder dialog is open.";

fn stroke_w() -> f32 {
    crate::theme::stroke::FRAME
}

impl eframe::App for Studio {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        Studio::ui(self, ui);
    }
}

/// The largest zoom step of the device that the simulator pane `sim`
/// holds with its title row, the honest line and the row of controls.
/// 1 when even that does not fit.
pub fn sim_fit(sim: Rect) -> u8 {
    let below = size::PANE_HEAD_H
        + size::SIM_ROW_GAP * 2.0
        + size::SIM_ROW_GAP
        + crate::theme::text::BODY_LINE
        + size::SMALL_H
        + size::PAD_BOTTOM;
    let mut best = 1;
    for z in crate::art::DEVICE_ZOOMS {
        let d = Art::device_size(z);
        if d.x <= sim.width() && d.y + below <= sim.height() {
            best = z;
        }
    }
    best
}

/// The honest line under the device, at the side.
const HONEST: &str = "Rebuilt sound. No panel timing. This computer's speed.";

/// Where the project page's panes go.
#[derive(Debug, Clone, Copy)]
pub struct Panes {
    /// The page less its margins.
    pub area: Rect,
    /// The Files pane.
    pub files: Rect,
    /// The editor.
    pub editor: Rect,
    /// The simulator.
    pub sim: Rect,
    /// The tape, under the simulator.
    pub tape: Rect,
    /// The bottom pane.
    pub bottom: Rect,
    /// Files is drawn: it is on and there is room for it.
    pub show_files: bool,
    /// The bottom pane is drawn.
    pub show_bottom: bool,
}

/// The page less its margins: 6 at the top and the bottom, 4 at the left,
/// 8 at the right. It can come out inside out; [`panes`] mends that.
fn page_area(page: Rect) -> Rect {
    Rect::from_min_max(
        egui::pos2(
            page.left() + size::PAGE_MARGIN_LEFT,
            page.top() + size::PAGE_MARGIN_DOWN,
        ),
        egui::pos2(
            page.right() - size::PAGE_MARGIN_RIGHT,
            page.bottom() - size::PAGE_MARGIN_DOWN,
        ),
    )
}

/// Where the footer's parts go, as left and right edges.
#[derive(Debug)]
pub struct StatusLayout {
    /// One per segment, in the order given; `None` for one left out.
    pub segs: Vec<Option<(f32, f32)>>,
    /// The division, at the left; `None` when it gave way.
    pub division: Option<(f32, f32)>,
    /// The message, with its icon.
    pub msg: (f32, f32),
}

/// Lay out the footer between `left` and `right`. The division, if any,
/// starts at `left` and the message follows it. `wants` are the segments'
/// widths, from the right corner leftwards; each is capped at
/// [`size::SEGMENT_MAX`]. Parts give way in order: the division first,
/// then the segments `give` lists, in that order. The division and the
/// first `soft` of `give` go while the message's `need`, at least
/// [`size::FOOTER_MESSAGE_MIN`], does not fit. Any part goes while the
/// message would get less than [`size::MESSAGE_MIN`]. No part ever gets a
/// negative width, and none leaves `left..right`.
pub fn status_layout(
    left: f32,
    right: f32,
    division: Option<f32>,
    wants: &[f32],
    give: &[usize],
    soft: usize,
    need: f32,
) -> StatusLayout {
    let left = left.min(right);
    let gap = size::UNIT * 2.0;
    let widths: Vec<f32> = wants
        .iter()
        .map(|w| w.clamp(0.0, size::SEGMENT_MAX))
        .collect();
    let mut div = division.map(|w| w.max(0.0));
    let mut shown = vec![true; widths.len()];
    let reserve = need.max(size::FOOTER_MESSAGE_MIN);
    let order = std::iter::once(None).chain(give.iter().map(|&i| Some(i)));
    for (k, part) in order.enumerate() {
        let segs: f32 = widths
            .iter()
            .zip(&shown)
            .filter(|(_, s)| **s)
            .map(|(w, _)| w)
            .sum();
        let room = right - segs - gap - left - div.unwrap_or(0.0);
        if !((k <= soft && room < reserve) || room < size::MESSAGE_MIN) {
            break;
        }
        match part {
            None => div = None,
            Some(i) => {
                if let Some(s) = shown.get_mut(i) {
                    *s = false;
                }
            }
        }
    }
    let mut x = right;
    let segs = widths
        .iter()
        .zip(&shown)
        .map(|(w, s)| {
            s.then(|| {
                let l = x - w;
                let seg = (l, x);
                x = l;
                seg
            })
        })
        .collect();
    let division = div.map(|w| (left, (left + w).min(x)));
    let start = division.map_or(left, |d| d.1);
    StatusLayout {
        segs,
        division,
        msg: (start, (x - gap).max(start)),
    }
}

/// Lay out the project page in `page`. The window's minimum size fits
/// every pane; given less anyway, Files is left out first, the simulator
/// shrinks, and no rectangle gets a negative width or height.
pub fn panes(l: &mut Layout, page: Rect) -> Panes {
    let page = Rect::from_min_size(page.min, page.size().max(egui::Vec2::ZERO));
    let shrunk = page_area(page);
    let area = Rect::from_min_size(
        shrunk.min.min(page.max),
        shrunk.size().max(egui::Vec2::ZERO),
    );
    // Keep a rectangle inside the area, with no negative side.
    let fit = |r: Rect| {
        let min = r.min.clamp(area.min, area.max);
        let max = r.max.clamp(min, area.max.max(min));
        Rect::from_min_max(min, max)
    };
    l.files_w = l.files_w.clamp(size::FILES_MIN, size::FILES_MAX);
    let show_files = l.files
        && area.width() >= l.files_w + size::GAP_ACROSS * 2.0 + size::FILES_MIN + size::SIM_W;
    let files_w = if show_files { l.files_w } else { 0.0 };
    let gap_files = if show_files { size::GAP_ACROSS } else { 0.0 };
    let max_sim = area.width() - files_w - gap_files - size::GAP_ACROSS - size::FILES_MIN;
    l.sim_w = l.sim_w.clamp(size::SIM_W, max_sim.max(size::SIM_W));
    let sim_w = l.sim_w.min(area.width()).max(0.0);
    l.bottom_h = l.bottom_h.clamp(
        size::BOTTOM_MIN,
        (area.height() / 2.0).max(size::BOTTOM_MIN),
    );
    let bottom_h = l.bottom_h.min(area.height() / 2.0).max(0.0);
    let sim_left = area.right() - sim_w;
    let left_right = (sim_left - size::GAP_ACROSS).max(area.left());
    let top_bottom = if l.bottom {
        area.bottom() - bottom_h - size::GAP_DOWN
    } else {
        area.bottom()
    };
    let files = fit(Rect::from_min_max(
        area.left_top(),
        egui::pos2(area.left() + files_w, top_bottom),
    ));
    let editor = fit(Rect::from_min_max(
        egui::pos2(area.left() + files_w + gap_files, area.top()),
        egui::pos2(left_right, top_bottom),
    ));
    let tape_top = (area.bottom() - size::TAPE_H).max(area.top());
    let sim = fit(Rect::from_min_max(
        egui::pos2(sim_left, area.top()),
        egui::pos2(area.right(), tape_top - size::GAP_DOWN),
    ));
    let tape = fit(Rect::from_min_max(
        egui::pos2(sim_left, tape_top),
        area.right_bottom(),
    ));
    let bottom = fit(Rect::from_min_max(
        egui::pos2(area.left(), area.bottom() - bottom_h),
        egui::pos2(left_right, area.bottom()),
    ));
    Panes {
        area,
        files,
        editor,
        sim,
        tape,
        bottom,
        show_files,
        show_bottom: l.bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Screen;

    /// The longest name the manifest allows, in its widest form, fits the
    /// tape's label field with the stated margin on each side. Every Spleen
    /// cell has one width, so any name of that length is as wide.
    #[test]
    fn the_longest_name_fits_the_tape_label() {
        let longest = "W".repeat(studio_core::manifest::MAX_NAME);
        let w = crate::art::LabelFont::width(&longest, 1.0, 1.0, 0.0);
        let room = size::TAPE_LABEL[0] - 2.0 * size::TAPE_LABEL_MARGIN;
        assert!(w <= room, "{longest} is {w} points, the label has {room}");
    }

    /// The place of Flat, whose ten zeros draw Off.
    const FLAT: usize = 5;

    /// A theme changed by a menu command in the middle of a frame, after
    /// the art was checked, still ends with art made for the new theme:
    /// Flat to Green gains the baked halos and the scanlines, with no
    /// adapter here, and Green to Flat loses them.
    #[test]
    fn the_art_follows_a_theme_changed_mid_frame() {
        let mut h = egui_kittest::Harness::builder()
            .build_ui_state(|ui, s: &mut Studio| s.ui(ui), Studio::new(None));
        for (from, to) in [(FLAT, 0), (0, FLAT)] {
            h.state_mut().pick_theme(from);
            h.run_steps(2);
            let made = |h: &egui_kittest::Harness<'_, Studio>| {
                h.state()
                    .art
                    .as_ref()
                    .map(|a| (a.halos(), a.has_scanlines()))
            };
            let soft = from == 0;
            assert_eq!(made(&h), Some((soft, soft)));
            h.state_mut().pending.push(Command::Theme(ThemeId::At(to)));
            h.run_steps(3);
            let want = if to == 0 { Screen::Soft } else { Screen::Off };
            assert_eq!(h.state().theme.screen(), want);
            let soft = to == 0;
            assert_eq!(made(&h), Some((soft, soft)), "{from:?} to {to:?}");
        }
    }

    /// A command in the middle of a frame changes the theme for the next
    /// frame only: the frame it runs in keeps the screen, the strengths
    /// and the curve the pointer is mapped through that drew it. Flat to
    /// Green and back, on a real adapter.
    #[test]
    fn the_drawn_value_holds_for_the_whole_frame() {
        let mut h = egui_kittest::Harness::builder()
            .build_ui_state(|ui, s: &mut Studio| s.ui(ui), Studio::new(None));
        h.state_mut().screen.adapter = screen::Adapter::Hardware;
        let curve = |h: &egui_kittest::Harness<'_, Studio>| {
            h.state()
                .screen
                .link()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .curvature
        };
        let screen_of = |i: usize| if i == 0 { Screen::Live } else { Screen::Off };
        for (from, to) in [(FLAT, 0), (0, FLAT)] {
            h.state_mut().pick_theme(from);
            h.run_steps(2);
            let before = h.state().drawn();
            assert_eq!(before.level(), screen_of(from));
            let c = curve(&h);
            h.state_mut().pending.push(Command::Theme(ThemeId::At(to)));
            h.step();
            assert_eq!(h.state().theme.screen(), screen_of(to), "the command ran");
            assert_eq!(h.state().drawn(), before, "{from:?} to {to:?}");
            assert_eq!(curve(&h), c, "the curve changed mid frame");
            h.step();
            assert_eq!(h.state().drawn().level(), screen_of(to));
            assert_eq!(
                h.state().drawn().strengths,
                crate::theme::SHIPPED[to].strengths
            );
            assert_eq!(curve(&h) > 0.0, screen_of(to).pass());
        }
    }

    /// A native menu command that switches Green to Flat, Soft to Off with
    /// no adapter here, with no input and nothing moving, asks for the
    /// next frame, and that frame is Off's.
    #[test]
    fn a_native_theme_change_asks_for_its_frame() {
        let mut h = egui_kittest::Harness::builder()
            .build_ui_state(|ui, s: &mut Studio| s.ui(ui), Studio::new(None));
        h.run_steps(4);
        h.step();
        assert!(
            !h.ctx.has_requested_repaint(),
            "Soft at rest asks for nothing"
        );
        h.state_mut()
            .pending
            .push(Command::Theme(ThemeId::At(FLAT)));
        h.step();
        assert!(h.ctx.has_requested_repaint(), "the change asks for a frame");
        h.step();
        let art = h
            .state()
            .art
            .as_ref()
            .map(|a| (a.halos(), a.has_scanlines()));
        assert_eq!(art, Some((false, false)));
    }

    /// What a command can change, cheap to compare: the theme, the page,
    /// the project, the documents and the dialogs.
    fn digest(s: &Studio) -> String {
        let docs: Vec<(u64, &str)> = s
            .editor
            .docs
            .iter()
            .map(|d| (d.id, d.text.as_str()))
            .collect();
        format!(
            "{:?}",
            (
                (s.theme, s.current, &s.changed, s.theme_editor.open),
                (s.page, s.layout, s.compact_header, s.sim_zoom),
                s.project.as_ref().map(|p| p.root.clone()),
                docs,
                (s.editor.question, s.editor.find.open, s.choosing),
            )
        )
    }

    /// A window on one edited file in Green at Live, with `front` in front. A
    /// question comes over an open theme editor, the case that once leaked.
    fn fronted(f: &Path, front: Front) -> Studio {
        let mut s = Studio::new(None);
        s.choose_screen(screen::Adapter::Hardware, None, Some(0));
        s.open_file(f);
        if let Some(d) = s.editor.current_mut() {
            d.text = "new\n".into();
        }
        match front {
            Front::Nothing => {}
            Front::Theme => s.theme_editor.open = true,
            Front::Question => {
                s.theme_editor.open = true;
                assert!(!s.editor.request_quit());
            }
            Front::Chooser => s.choosing = Choosing::Due,
        }
        assert_eq!(s.front(), front);
        s
    }

    /// Every command of the menu model against every front state: the
    /// table's verdict, what `run` did and what `unavailable` says agree.
    /// A command the table lets act changes what it changes with nothing
    /// in front; a refused one changes nothing.
    #[test]
    fn the_front_table_is_what_run_and_the_menus_do() {
        let d = std::env::temp_dir().join(format!("robco-front-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("main.c");
        let ctx = egui::Context::default();
        let mut items: Vec<menus::Item> =
            menus::menus().into_iter().flat_map(|m| m.items).collect();
        // The rows a long list, a changed theme and the notes bring.
        let mut many = crate::theme::SHIPPED.to_vec();
        many.extend((0..9).map(|i| Spec {
            name: format!("Mine {i}").into(),
            ..crate::theme::SHIPPED[1].clone()
        }));
        items.extend(menus::theme_items(
            &many,
            Some(&crate::theme::SHIPPED[0]),
            &[
                screen::Held::Legible,
                screen::Held::Still,
                screen::Held::NoAdapter,
            ],
        ));
        let run = |front: Front, it: &menus::Item| {
            std::fs::write(&f, b"old\n").unwrap();
            let mut s = fronted(&f, front);
            let why = s.unavailable(it);
            let before = digest(&s);
            ctx.begin_pass(egui::RawInput::default());
            s.run(&ctx, it.command);
            ctx.end_pass().textures_delta.clear();
            (why, before != digest(&s))
        };
        for it in &items {
            let (_, acts_alone) = run(Front::Nothing, it);
            for front in [
                Front::Chooser,
                Front::Question,
                Front::Theme,
                Front::Nothing,
            ] {
                let v = verdict(it.command, front);
                let (why, changed) = run(front, it);
                match v {
                    Verdict::Refused(r) => {
                        assert!(
                            !changed,
                            "{:?} over {front:?} changed the window",
                            it.command
                        );
                        assert_eq!(why.as_deref(), Some(r), "{:?} over {front:?}", it.command);
                    }
                    Verdict::Acts => {
                        // The editor over the editor finds it open already.
                        let open = front == Front::Theme && it.command == Command::ThemeEdit;
                        assert_eq!(
                            changed,
                            acts_alone && !open,
                            "{:?} over {front:?}",
                            it.command
                        );
                        assert!(
                            ![ASKING, ADJUSTING, CHOOSING].contains(&why.as_deref().unwrap_or("")),
                            "{:?} over {front:?} said {why:?}",
                            it.command
                        );
                    }
                }
            }
        }
        // The rows decided by hand, spelled out.
        assert_eq!(
            verdict(Command::Quit, Front::Question),
            Verdict::Refused(ASKING)
        );
        assert_eq!(
            verdict(Command::Theme(ThemeId::At(FLAT)), Front::Question),
            Verdict::Refused(ASKING)
        );
        assert_eq!(
            verdict(Command::Theme(ThemeId::At(FLAT)), Front::Theme),
            Verdict::Acts
        );
        assert_eq!(verdict(Command::ThemeAdd, Front::Theme), Verdict::Acts);
        assert_eq!(
            verdict(Command::ThemeAdd, Front::Question),
            Verdict::Refused(ASKING)
        );
        assert_eq!(
            verdict(Command::ThemeAdd, Front::Chooser),
            Verdict::Refused(CHOOSING)
        );
        assert_eq!(verdict(Command::OpenFolder, Front::Theme), Verdict::Acts);
        assert_eq!(
            verdict(Command::Save, Front::Theme),
            Verdict::Refused(ADJUSTING)
        );
        assert_eq!(
            verdict(Command::HighLegibility, Front::Theme),
            Verdict::Acts
        );
        assert_eq!(verdict(Command::Quit, Front::Theme), Verdict::Acts);
        assert_eq!(
            verdict(Command::HighLegibility, Front::Question),
            Verdict::Refused(ASKING)
        );
        assert_eq!(ADJUSTING, "Close the theme editor first.");
        std::fs::remove_dir_all(&d).unwrap();
    }

    fn press(s: &mut Studio, ctx: &egui::Context, key: Key, modifiers: Modifiers) {
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        };
        ctx.begin_pass(input);
        s.keys(ctx);
        ctx.end_pass().textures_delta.clear();
    }

    #[test]
    fn editor_shortcuts_work_on_every_page() {
        let d = std::env::temp_dir().join(format!("robco-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("main.c");
        std::fs::write(&f, b"old\n").unwrap();
        let ctx = egui::Context::default();
        let mut s = Studio::new(None);
        s.open_file(&f);
        s.page = Page::Card;
        if let Some(doc) = s.editor.current_mut() {
            doc.text = "new\n".into();
        }
        press(&mut s, &ctx, Key::S, Modifiers::COMMAND);
        assert_eq!(std::fs::read(&f).unwrap(), b"new\n");

        s.page = Page::Device;
        press(&mut s, &ctx, Key::F, Modifiers::COMMAND);
        assert_eq!(s.page, Page::Project);
        assert!(s.editor.find.open);

        s.page = Page::Sdk;
        press(&mut s, &ctx, Key::W, Modifiers::COMMAND);
        assert!(s.editor.current().is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// A long status line takes the division's place first, then Position,
    /// and only then ends in an ellipsis.
    #[test]
    fn the_footer_gives_way_in_order() {
        let (div, pos, seg) = (200.0, 100.0, 50.0);
        let gap = size::UNIT * 2.0;
        let room = size::FOOTER_MESSAGE_MIN + 10.0;
        // Position at the corner, then Link, Sim and SDK.
        let wants = [pos, seg, seg, seg];
        let right = room + gap + div + pos + seg * 3.0;
        let lay = |need: f32| {
            let l = status_layout(0.0, right, Some(div), &wants, &[0, 3, 2, 1], 1, need);
            (l.division.is_some(), l.segs[0].is_some())
        };
        assert_eq!(lay(100.0), (true, true));
        assert_eq!(lay(room + 50.0), (false, true));
        assert_eq!(lay(room + div + 50.0), (false, false));
        // Without Position, nothing more goes: the line ends in an ellipsis.
        let l = status_layout(
            0.0,
            right - pos,
            Some(div),
            &wants[1..],
            &[2, 1, 0],
            0,
            room + div + 50.0,
        );
        assert!(l.division.is_none() && l.segs.iter().all(Option::is_some));
        // The line claims its minimum even when it is short.
        let l = status_layout(0.0, right - 11.0, Some(div), &wants, &[0, 3, 2, 1], 1, 10.0);
        assert!(l.division.is_none() && l.segs[0].is_some(), "{l:?}");
    }

    /// The simulator pane of the default layout holds 1x only; a larger
    /// one holds more, and a tiny one still says 1.
    #[test]
    fn sim_fit_picks_the_largest_step_that_fits() {
        let at = |w: f32, h: f32| Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
        assert_eq!(sim_fit(at(size::SIM_W, 428.0)), 1);
        assert_eq!(sim_fit(at(900.0, 800.0)), 2);
        assert_eq!(sim_fit(at(0.0, 0.0)), 1);
    }

    /// Given less than the smallest window, down to nothing, the layout
    /// keeps every pane inside the page with no negative side.
    #[test]
    fn panes_never_turn_inside_out() {
        let sizes = [
            (1280.0, 720.0),
            (716.0, 720.0),
            (700.0, 400.0),
            (500.0, 300.0),
            (200.0, 100.0),
            (20.0, 10.0),
            (1.0, 1.0),
            (0.0, 0.0),
        ];
        for files in [true, false] {
            for (w, h) in sizes {
                let mut l = Layout {
                    files,
                    ..Layout::default()
                };
                let page = Rect::from_min_size(egui::pos2(76.0, 70.0), egui::vec2(w, h));
                let p = panes(&mut l, page);
                for (name, r) in [
                    ("files", p.files),
                    ("editor", p.editor),
                    ("sim", p.sim),
                    ("tape", p.tape),
                    ("bottom", p.bottom),
                ] {
                    assert!(
                        r.width() >= 0.0 && r.height() >= 0.0,
                        "{name} at {w}x{h}: {r:?}"
                    );
                    assert!(
                        page.contains_rect(r),
                        "{name} at {w}x{h}: {r:?} outside {page:?}"
                    );
                }
                if p.show_files {
                    assert!(p.files.right() <= p.editor.left(), "{w}x{h}");
                }
                assert!(p.editor.right() <= p.sim.left(), "{w}x{h}");
            }
        }
        // The strips stay in the window and in order, down to nothing.
        for (w, h) in sizes {
            for (menu, tall) in [(true, true), (true, false), (false, true), (false, false)] {
                let full = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, h));
                let s = strips(full, menu, tall);
                let all = [
                    Some(s.header),
                    s.menu,
                    Some(s.toolbar),
                    Some(s.page),
                    Some(s.footer),
                ];
                for r in all.into_iter().flatten() {
                    assert!(r.width() >= 0.0 && r.height() >= 0.0, "{w}x{h}: {r:?}");
                    assert!(full.contains_rect(r), "{w}x{h}: {r:?}");
                }
                assert_eq!(s.menu.is_some(), menu);
                assert!(s.header.bottom() <= s.toolbar.top());
                assert!(s.toolbar.bottom() <= s.page.top() && s.page.bottom() <= s.footer.top());
                assert_eq!(
                    s.page.left(),
                    full.left(),
                    "{w}x{h}: the page spans the window"
                );
                assert_eq!(s.page.right(), full.right(), "{w}x{h}");
                let p = panes(&mut Layout::default(), s.page);
                for r in [p.files, p.editor, p.sim, p.tape, p.bottom] {
                    assert!(r.width() >= 0.0 && r.height() >= 0.0, "{w}x{h}: {r:?}");
                }
            }
        }
        // At 1280 by 720 the header is tall, and under 720 points compact.
        assert!(header_tall(size::HEADER_TALL_FROM, false));
        assert!(!header_tall(size::HEADER_TALL_FROM, true));
        assert!(!header_tall(size::HEADER_TALL_FROM - 1.0, false));
        let s = strips(
            Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0)),
            true,
            true,
        );
        assert_eq!(s.header.height(), size::HEADER_H);
        assert_eq!(s.footer.height(), size::FOOTER_H);
        // At the smallest window every pane is drawn, side by side.
        let mut l = Layout::default();
        let p = panes(
            &mut l,
            Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0 - 100.0)),
        );
        assert!(p.show_files && p.editor.width() > size::FILES_MIN);
    }

    /// The atom comes first and the wordmark after it, in both forms.
    /// At the smallest window the header's parts do not overlap, the caps
    /// lines keep clear space on both sides, the atom stays off the rule,
    /// and the skyline gives way exactly where the skyline room runs out.
    #[test]
    fn the_header_parts_keep_apart() {
        use crate::art::LabelFont;
        let track = crate::theme::text::LABEL_TRACKING;
        let two = HEADER_LINES
            .iter()
            .map(|l| LabelFont::width(l, 1.0, 1.0, track))
            .fold(0.0, f32::max);
        let one = LabelFont::width(&HEADER_LINES.join(" "), 1.0, 1.0, track);
        let w = size::WINDOW[0];
        let tall = header_layout(
            Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, size::HEADER_H)),
            true,
            two,
        );
        let rule = size::HEADER_H - stroke_w() / 2.0;
        assert_eq!(tall.atom.left(), size::HEADER_PAD);
        assert_eq!(
            tall.wordmark.left() - tall.atom.right(),
            size::HEADER_MARK_GAP
        );
        assert!(!tall.atom.intersects(tall.wordmark));
        assert!(
            tall.atom.top() > 0.0 && tall.atom.bottom() < rule - 1.0,
            "{:?}",
            tall.atom
        );
        assert!(tall.text.left() - tall.divider_x >= size::HEADER_GAP);
        assert_eq!(tall.divider_x - tall.wordmark.right(), size::HEADER_GAP);
        let sky = tall
            .skyline
            .expect("the skyline shows at the smallest window");
        assert!(sky.left() - tall.text.right() >= size::SKYLINE_ROOM);
        assert!(sky.right() <= w - size::HEADER_MARK_GAP);
        assert!((sky.top() + size::SKYLINE_RULE_Y - rule).abs() < 0.01);
        for r in [tall.wordmark, tall.atom, tall.text] {
            assert!(!r.intersects(sky), "{r:?}");
        }
        // The skyline gives way below the width that leaves its room.
        let edge = tall.text.right()
            + size::SKYLINE_ROOM
            + Art::mark_size(Mark::Skyline).x
            + size::HEADER_MARK_GAP;
        let at = |w: f32| {
            header_layout(
                Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, size::HEADER_H)),
                true,
                two,
            )
        };
        assert!(at(edge).skyline.is_some());
        assert!(at(edge - 1.0).skyline.is_none());
        assert!(edge < w);
        // The compact header has no skyline, and its one caps line fits.
        let c = header_layout(
            Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, size::HEADER_COMPACT_H)),
            false,
            one,
        );
        assert!(c.skyline.is_none());
        assert_eq!(c.atom.left(), size::HEADER_PAD);
        assert_eq!(c.wordmark.left() - c.atom.right(), size::HEADER_MARK_GAP);
        assert!(!c.atom.intersects(c.wordmark));
        assert_eq!(c.divider_x - c.wordmark.right(), size::HEADER_GAP);
        assert!(c.text.right() + size::HEADER_PAD <= w);
        assert!(c.atom.top() > 0.0 && c.atom.bottom() < size::HEADER_COMPACT_H - 1.0);
    }

    /// The page area starts at the page margin, and the editor takes all
    /// the width the other panes and the gaps leave.
    #[test]
    fn the_page_starts_at_the_margin_and_the_editor_takes_the_rest() {
        let full = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
        let s = strips(full, true, true);
        let mut l = Layout::default();
        let p = panes(&mut l, s.page);
        assert_eq!(p.area.left(), size::PAGE_MARGIN_LEFT);
        assert_eq!(p.files.left(), size::PAGE_MARGIN_LEFT);
        let rest = 1280.0
            - size::PAGE_MARGIN_LEFT
            - size::PAGE_MARGIN_RIGHT
            - size::FILES_W
            - size::SIM_W
            - size::GAP_ACROSS * 2.0;
        assert_eq!(p.editor.width(), rest);
    }

    /// Long segment text and narrow bars never give a negative width or
    /// push the message out.
    #[test]
    fn status_bar_never_turns_inside_out() {
        let long = [10_000.0, 2_000.0, 60.0, 70.0, 80.0];
        for width in [1280.0, 800.0, 500.0, 300.0, 200.0, 100.0, 20.0, 0.0] {
            for wants in [&long[..], &[50.0; 6][..], &[][..]] {
                let left = 40.0;
                let give: Vec<usize> = (0..wants.len()).rev().collect();
                let l = status_layout(left, width, None, wants, &give, 0, 0.0);
                assert!(l.msg.0 <= l.msg.1, "{width}: {l:?}");
                assert!(
                    l.msg.0 >= left.min(width) && l.msg.1 <= width.max(0.0),
                    "{width}: {l:?}"
                );
                let mut edge = width;
                for s in l.segs.iter().flatten() {
                    assert!(
                        s.0 <= s.1 && s.1 - s.0 <= size::SEGMENT_MAX,
                        "{width}: {l:?}"
                    );
                    assert!(s.1 <= edge && s.0 >= l.msg.1, "{width}: {l:?}");
                    edge = s.0;
                }
                if l.segs.iter().any(Option::is_some) {
                    assert!(l.msg.1 - l.msg.0 >= size::MESSAGE_MIN, "{width}: {l:?}");
                }
            }
        }
        // Wide enough: every segment shows, the long ones capped.
        let give = [4, 3, 2, 1, 0];
        let l = status_layout(40.0, 1280.0, None, &long, &give, 0, 0.0);
        assert!(l.segs.iter().all(Option::is_some), "{l:?}");
        // The leftmost segment gives way first.
        let l = status_layout(40.0, 500.0, None, &long, &give, 0, 0.0);
        assert_eq!(l.segs.last(), Some(&None), "{l:?}");
        assert!(l.segs[0].is_some(), "{l:?}");
    }

    fn native(s: &Studio, c: Command) -> menus::NativeItem {
        menus::native_items(s)
            .into_iter()
            .find(|i| i.command == c)
            .unwrap()
    }

    /// Each choice set has exactly one item marked.
    fn one_per_choice(s: &Studio) {
        let list = menus::native_items(s);
        for set in ["page", "theme", "scale"] {
            let on = list
                .iter()
                .filter(|i| menus::check(i.command) == menus::Check::Choice(set) && i.checked)
                .count();
            assert_eq!(on, 1, "{set}");
        }
    }

    #[test]
    fn every_toggle_and_choice_is_a_check_item() {
        let s = Studio::new(None);
        let want = [
            Command::PageProject,
            Command::PageCard,
            Command::PageDevice,
            Command::PageSdk,
            Command::ToggleFiles,
            Command::ToggleBottom,
            Command::CompactHeader,
            Command::HighLegibility,
            Command::SimZoom(1),
            Command::SimZoom(2),
            Command::SimZoom(3),
        ];
        let list = menus::native_items(&s);
        for i in &list {
            let is_state = want.contains(&i.command) || matches!(i.command, Command::Theme(_));
            let kind = if is_state {
                menus::NativeKind::Check
            } else {
                menus::NativeKind::Plain
            };
            assert_eq!(i.kind, kind, "{}", i.title);
            if kind == menus::NativeKind::Plain {
                assert!(!i.checked, "{}", i.title);
            }
        }
        for i in 0..crate::theme::SHIPPED.len() {
            assert_eq!(
                native(&s, Command::Theme(ThemeId::At(i))).kind,
                menus::NativeKind::Check
            );
        }
        assert_eq!(
            list.len(),
            s.menus().iter().map(|m| m.items.len()).sum::<usize>()
        );
        one_per_choice(&s);
    }

    #[test]
    fn marks_follow_the_state_by_menu_key_and_rule() {
        let ctx = egui::Context::default();
        let mut s = Studio::new(None);
        // By the menu.
        assert!(!native(&s, Command::HighLegibility).checked);
        s.run(&ctx, Command::HighLegibility);
        assert!(native(&s, Command::HighLegibility).checked);
        // High legibility holds the theme flat, and it stays marked.
        assert!(native(&s, Command::Theme(ThemeId::At(0))).checked);
        one_per_choice(&s);
        // By the shortcut.
        press(&mut s, &ctx, Key::L, Modifiers::COMMAND | Modifiers::SHIFT);
        assert!(!native(&s, Command::HighLegibility).checked);
        assert!(native(&s, Command::Theme(ThemeId::At(0))).checked);
        press(&mut s, &ctx, Key::Num3, Modifiers::COMMAND);
        assert!(native(&s, Command::PageDevice).checked);
        assert!(!native(&s, Command::PageProject).checked);
        press(&mut s, &ctx, Key::J, Modifiers::COMMAND);
        assert_eq!(native(&s, Command::ToggleBottom).checked, s.layout.bottom);
        let files = s.layout.files;
        s.run(&ctx, Command::ToggleFiles);
        assert_eq!(native(&s, Command::ToggleFiles).checked, !files);
        s.run(&ctx, Command::CompactHeader);
        assert!(native(&s, Command::CompactHeader).checked);
        s.run(&ctx, Command::Theme(ThemeId::At(1)));
        assert!(native(&s, Command::Theme(ThemeId::At(1))).checked);
        assert!(!native(&s, Command::Theme(ThemeId::At(0))).checked);
        one_per_choice(&s);
    }

    #[test]
    fn a_refused_scale_keeps_its_mark_and_stays_greyed() {
        let ctx = egui::Context::default();
        let mut s = Studio::new(None);
        let z3 = native(&s, Command::SimZoom(3));
        assert!(!z3.enabled);
        s.run(&ctx, Command::SimZoom(3));
        assert!(!native(&s, Command::SimZoom(3)).checked);
        assert!(native(&s, Command::SimZoom(1)).checked);
        s.sim_room = 3;
        s.run(&ctx, Command::SimZoom(3));
        assert!(native(&s, Command::SimZoom(3)).checked);
        // The room shrinks: the item greys and keeps its mark.
        s.sim_room = 1;
        let z3 = native(&s, Command::SimZoom(3));
        assert!(!z3.enabled && z3.checked);
        one_per_choice(&s);
    }

    /// Every command of the menus is either plain, with no mark anywhere,
    /// or a check item whose state is defined: running it, when it can
    /// act, leaves its mark as its rule says.
    #[test]
    fn every_command_is_plain_or_a_check_item_with_a_state() {
        let ctx = egui::Context::default();
        let fresh = || {
            let mut s = Studio::new(None);
            s.sim_room = 3;
            s
        };
        for it in menus::menus().into_iter().flat_map(|m| m.items) {
            let c = it.command;
            let mut s = fresh();
            let n = native(&s, c);
            match menus::check(c) {
                menus::Check::Plain => {
                    assert_eq!(n.kind, menus::NativeKind::Plain, "{}", it.text);
                    assert!(!n.checked && !s.checked(c), "{}", it.text);
                    assert_eq!(s.menu_mark(c), MenuMark::None, "{}", it.text);
                }
                check => {
                    assert_eq!(n.kind, menus::NativeKind::Check, "{}", it.text);
                    if !n.enabled {
                        continue;
                    }
                    let before = s.checked(c);
                    s.run(&ctx, c);
                    let want = match check {
                        menus::Check::Toggle => !before,
                        _ => true,
                    };
                    assert_eq!(s.checked(c), want, "{}", it.text);
                    assert_eq!(native(&s, c).checked, want, "{}", it.text);
                }
            }
        }
    }

    /// The in-window menu marks exactly the items the macOS bar marks,
    /// as check boxes for toggles and radio items for a choice.
    fn window_and_bar_agree(s: &Studio) {
        for n in menus::native_items(s) {
            let m = s.menu_mark(n.command);
            match n.kind {
                menus::NativeKind::Plain => assert_eq!(m, MenuMark::None, "{}", n.title),
                menus::NativeKind::Check => {
                    assert_ne!(m, MenuMark::None, "{}", n.title);
                    assert_eq!(m.on(), n.checked, "{}", n.title);
                }
            }
        }
    }

    #[test]
    fn the_window_menu_marks_what_the_bar_marks() {
        let ctx = egui::Context::default();
        let mut s = Studio::new(None);
        window_and_bar_agree(&s);
        for c in [
            Command::HighLegibility,
            Command::CompactHeader,
            Command::PageCard,
            Command::Theme(ThemeId::At(1)),
            Command::HighLegibility,
            Command::Theme(ThemeId::At(4)),
            Command::ToggleBottom,
        ] {
            s.run(&ctx, c);
            window_and_bar_agree(&s);
        }
        s.edit_theme(|t| t.strengths.bloom += 0.25);
        window_and_bar_agree(&s);
    }

    /// The theme in use is marked, alone; a changed theme marks its own
    /// row; Edit theme, Add a theme file and Show the themes folder are
    /// plain.
    #[test]
    fn the_theme_rows_mark_the_theme_in_use() {
        let marked = |s: &Studio| -> Vec<Command> {
            menus::native_items(s)
                .into_iter()
                .filter(|i| matches!(i.command, Command::Theme(_)) && i.checked)
                .map(|i| i.command)
                .collect()
        };
        let ctx = egui::Context::default();
        let mut s = Studio::new(None);
        for c in [Command::ThemeEdit, Command::ThemeAdd, Command::ThemeFolder] {
            assert_eq!(native(&s, c).kind, menus::NativeKind::Plain);
        }
        assert_eq!(marked(&s), [Command::Theme(ThemeId::At(0))]);
        s.choose_screen(Adapter::Hardware, None, Some(4));
        assert_eq!(marked(&s), [Command::Theme(ThemeId::At(4))]);
        assert_eq!(s.status.1, "Ready");
        // A moved slider: the changed row carries the mark, and the
        // status line says so once.
        s.edit_theme(|t| t.strengths.bloom = 0.9);
        assert_eq!(marked(&s), [Command::Theme(ThemeId::Changed)]);
        assert_eq!(
            native(&s, Command::Theme(ThemeId::Changed)).title,
            "Arcade, changed"
        );
        assert_eq!(s.status.1, "Theme: Arcade, changed. Not saved.");
        s.status.1 = "x".into();
        s.edit_theme(|t| t.strengths.bloom = 0.8);
        assert_eq!(s.status.1, "x", "said once");
        assert_eq!(s.theme_name(), "Arcade, changed");
        // Another theme asks first; Do not save lets it take the place,
        // and the change goes.
        s.run(&ctx, Command::Theme(ThemeId::At(1)));
        assert_eq!(s.front(), Front::Question);
        s.answer_theme(crate::editor::Answer::DoNotSave);
        assert_eq!(marked(&s), [Command::Theme(ThemeId::At(1))]);
        assert!(s.changed.is_none());
        assert_eq!(s.status.1, "Theme: Amber.");
        // High legibility holds the theme flat; it stays marked.
        s.run(&ctx, Command::HighLegibility);
        assert_eq!(marked(&s), [Command::Theme(ThemeId::At(1))]);
        assert_eq!(s.theme.screen(), Screen::Off);
    }

    /// What holds the theme says so on the status line at start, and as
    /// one note each in View, Theme, which cannot be picked.
    #[test]
    fn what_holds_the_theme_says_so() {
        let notes = |s: &Studio| -> Vec<String> {
            menus::native_items(s)
                .into_iter()
                .filter(|i| matches!(i.command, Command::ThemeHeld(_)))
                .inspect(|i| assert!(!i.enabled, "{}", i.title))
                .map(|i| i.title)
                .collect()
        };
        let mut s = Studio::new(None);
        s.choose_screen(Adapter::Software, None, None);
        assert_eq!(
            s.status.1,
            "This machine has no graphics adapter for the screen pass. Theme: Green, shown without it."
        );
        assert_eq!(s.theme.screen(), Screen::Soft);
        assert_eq!(notes(&s), [crate::theme::HELD_SIMPLER]);
        let mut s = Studio::new(None);
        s.choose_screen(Adapter::Hardware, Some(true), None);
        assert_eq!(
            s.status.1,
            "The system asks for reduced motion. Theme: Green, shown still."
        );
        assert_eq!(s.theme.screen(), Screen::Still);
        assert_eq!(notes(&s), [crate::theme::HELD_STILL]);
        // Flat has nothing to hold.
        let mut s = Studio::new(None);
        s.choose_screen(Adapter::Hardware, Some(true), Some(FLAT));
        assert_eq!(s.status.1, "Ready");
        assert!(notes(&s).is_empty());
        // No known adapter: the note, and nothing on the status line.
        let mut s = Studio::new(None);
        s.choose_screen(Adapter::Unknown, None, None);
        assert_eq!(s.status.1, "Ready");
        assert_eq!(notes(&s), [crate::theme::HELD_SIMPLER]);
        s.theme.high_legibility = true;
        assert_eq!(
            notes(&s),
            [crate::theme::HELD_FLAT, crate::theme::HELD_SIMPLER]
        );
    }

    /// Edit theme opens the pane by its shortcut; Add a theme file needs a
    /// settings folder; Show the themes folder is not built yet and says
    /// so.
    #[test]
    fn edit_theme_opens_the_pane_and_the_rest_say_not_yet() {
        let ctx = egui::Context::default();
        let mut s = Studio::new(None);
        press(&mut s, &ctx, Key::T, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(s.front(), Front::Theme);
        s.theme_editor.open = false;
        s.run(&ctx, Command::ThemeAdd);
        assert_eq!(s.status.1, ThemeFault::NoFolder.message());
        assert_eq!(s.front(), Front::Nothing);
        s.status_read = true;
        s.run(&ctx, Command::ThemeFolder);
        assert_eq!(
            s.status.1,
            "Show the themes folder is not in this version yet."
        );
    }
}
