// SPDX-License-Identifier: GPL-3.0-or-later

//! The window: its regions, its pages, the panes of the Project page, focus
//! from pane to pane, and the one dialog this release has.

use crate::art::{Art, Icon};
use crate::editor::{Answer, Editor};
use crate::kit::{Button, ButtonKind, Kit, Level};
use crate::menus::{self, Command};
use crate::project::{Manifest, Project};
use crate::theme::{Effects, Theme, Type, size};
use eframe::egui::{
    self, CornerRadius, Id, Key, Modifiers, Rect, Sense, Ui, UiBuilder, accesskit::Role,
};
use std::path::{Path, PathBuf};

/// The pages of the rail.
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
    /// The page's name, as the rail shows it.
    pub fn name(self) -> &'static str {
        match self {
            Page::Project => "Project",
            Page::Card => "Card",
            Page::Device => "Device",
            Page::Sdk => "SDK",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Page::Project => Icon::Bench,
            Page::Card => Icon::Card,
            Page::Device => Icon::Cable,
            Page::Sdk => Icon::Sdk,
        }
    }

    const ALL: [Page; 4] = [Page::Project, Page::Card, Page::Device, Page::Sdk];
}

/// The places focus moves through with F6, in the design's reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The toolbar.
    Toolbar,
    /// The rail.
    Rail,
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
    /// The status bar.
    Status,
}

impl Place {
    /// The name in the accessibility tree.
    pub fn name(self) -> &'static str {
        match self {
            Place::Toolbar => "Toolbar",
            Place::Rail => "Rail",
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
    /// The look.
    pub theme: Theme,
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
    /// The last message, for the status bar.
    pub status: (Level, String),
    /// Draw the menu row in the window. False where the system shows the menus.
    pub menu_row: bool,
    /// Commands chosen in a menu outside the window, run next frame.
    pub pending: Vec<Command>,
    art: Option<(Art, f32, Effects)>,
    styled: Option<Theme>,
    fonts: bool,
    focus_request: Option<Place>,
    places: Vec<(Place, Rect)>,
    page_rect: Rect,
    title: String,
}

const READY: &str = "Ready";

impl Studio {
    /// A window on `folder`, or on no project.
    pub fn new(folder: Option<&Path>) -> Self {
        let mut s = Self {
            theme: Theme::default(),
            page: Page::Project,
            project: None,
            editor: Editor::default(),
            layout: Layout::default(),
            bottom_tab: BottomTab::Build,
            status: (Level::Info, READY.to_owned()),
            menu_row: true,
            pending: Vec::new(),
            art: None,
            styled: None,
            fonts: false,
            focus_request: None,
            places: Vec::new(),
            page_rect: Rect::NOTHING,
            title: String::new(),
        };
        if let Some(f) = folder {
            match Project::open(f) {
                Ok(p) => {
                    s.status = (Level::Info, format!("Opened {}.", p.root.display()));
                    s.project = Some(p);
                }
                Err(e) => s.status = (Level::Error, e),
            }
        }
        s
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
        let mut v = vec![Place::Toolbar, Place::Rail];
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

    /// Run a menu command.
    pub fn run(&mut self, ctx: &egui::Context, c: Command) {
        match c {
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
            Command::NextPane => self.move_focus(ctx, true),
            Command::PreviousPane => self.move_focus(ctx, false),
            Command::Phosphor(s) => self.theme.scheme = s,
            Command::EffectsOff => self.theme.effects = Effects::Off,
            Command::EffectsSoft => self.theme.effects = Effects::Soft,
            Command::HighLegibility => self.theme.high_legibility = !self.theme.high_legibility,
            _ => {}
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
            k(Modifiers::COMMAND, Key::Q, Command::Quit);
        });
        cmds.append(&mut self.pending);
        for c in cmds {
            self.run(ctx, c);
        }
        if self.page == Page::Project {
            self.editor.keys(ctx);
        }
    }

    fn prepare(&mut self, ctx: &egui::Context) {
        if !self.fonts {
            Theme::install_fonts(ctx);
            self.fonts = true;
        }
        if self.styled != Some(self.theme) {
            self.theme.apply(ctx);
            self.styled = Some(self.theme);
        }
        let ppp = ctx.pixels_per_point();
        let fx = self.theme.effects();
        if !matches!(&self.art, Some((_, p, e)) if *p == ppp && *e == fx) {
            self.art = Some((Art::new(ctx, &self.theme), ppp, fx));
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

    /// Draw one frame of the window into `ui`.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.prepare(&ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.editor.request_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        self.keys(&ctx);
        let Some((art, _, _)) = self.art.take() else {
            return;
        };
        let theme = self.theme;
        let kit = Kit {
            theme: &theme,
            art: &art,
        };
        self.places.clear();
        let full = ui.max_rect();
        let p = kit.p();
        ui.painter().rect_filled(full, CornerRadius::ZERO, p.ground);

        let mut y = full.top();
        if self.menu_row {
            let r = Rect::from_min_size(full.min, egui::vec2(full.width(), size::MENU_H));
            self.menu_bar(&kit, ui, r);
            y = r.bottom();
        }
        let tool = Rect::from_min_max(
            egui::pos2(full.left(), y),
            egui::pos2(full.right(), y + size::TOOLBAR_H),
        );
        let status = Rect::from_min_max(
            egui::pos2(full.left(), full.bottom() - size::STATUS_H),
            full.right_bottom(),
        );
        let rail = Rect::from_min_max(
            egui::pos2(full.left(), tool.bottom()),
            egui::pos2(full.left() + size::RAIL_W, status.top()),
        );
        let page = Rect::from_min_max(
            egui::pos2(rail.right(), tool.bottom()),
            egui::pos2(full.right(), status.top()),
        );
        self.page_rect = page;

        self.toolbar(&kit, ui, tool);
        self.rail(&kit, ui, rail);
        let mut page_ui = kit.region(ui, page, "Page", Role::Main, p.ground, false);
        match self.page {
            Page::Project => self.project_page(&kit, &mut page_ui, page),
            other => self.later_page(&kit, &mut page_ui, page, other),
        }
        self.status_bar(&kit, ui, status);
        self.dialog(&kit, &ctx, full);
        if self.theme.effects() == Effects::Soft {
            art.scanlines(
                &ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Debug,
                    Id::unique("scanlines"),
                )),
                full,
            );
        }
        if let Some((level, text)) = self.editor.news.take() {
            self.status = (level, text);
        }
        if self.editor.quit_allowed {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.focus_request = None;
        self.art = Some((art, ctx.pixels_per_point(), self.theme.effects()));
    }

    fn menu_bar(&mut self, kit: &Kit<'_>, ui: &mut Ui, rect: Rect) {
        let p = kit.p();
        let bar = kit.region(ui, rect, "Menu bar", Role::MenuBar, p.ground, false);
        let mac = cfg!(target_os = "macos");
        let mut chosen = None;
        let mut x = rect.left() + size::UNIT * 2.0;
        for m in menus::menus() {
            let g = bar.painter().layout_no_wrap(
                m.title.to_owned(),
                kit.theme.font(Type::Body),
                p.text,
            );
            let r = Rect::from_min_size(
                egui::pos2(x, rect.top()),
                egui::vec2(g.size().x + size::STEP_16, rect.height()),
            );
            x = r.right();
            let resp = bar.interact(r, Id::unique(("menu", m.title)), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(Role::MenuItem, true, m.title));
            if resp.hovered() || resp.has_focus() {
                bar.painter().rect_filled(r, CornerRadius::ZERO, p.raised);
            }
            bar.painter().galley(
                egui::pos2(r.left() + size::UNIT * 2.0, r.center().y - g.size().y / 2.0),
                g,
                p.text,
            );
            if resp.has_focus() {
                kit.focus_ring(bar.painter(), r);
            }
            egui::Popup::menu(&resp)
                .width(size::MENU_POPUP_W)
                .show(|ui| {
                    for it in &m.items {
                        let note = it.shortcut.map(|s| s.shown(mac)).unwrap_or_default();
                        if kit
                            .menu_row(ui, it.text, &note, it.unavailable, size::MENU_POPUP_W)
                            .clicked()
                        {
                            chosen = Some(it.command);
                            ui.close();
                        }
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
        t.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            kit.theme.frame(p.frame_dim),
        );
        let inner = rect.shrink2(egui::vec2(size::STEP_12, 0.0));
        let ws = kit.art.wordmark_size();
        let wm = Rect::from_min_size(egui::pos2(inner.left(), rect.center().y - ws.y / 2.0), ws);
        kit.art.wordmark(t.painter(), wm.min, p.bright);
        t.put(
            wm,
            egui::Label::new(egui::RichText::new("RobCo Studio").color(egui::Color32::TRANSPARENT)),
        );
        let mut row = t.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    egui::pos2(wm.right() + size::STEP_24, rect.top()),
                    inner.right_bottom(),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let (name, target) = match &self.project {
            Some(pr) => (
                pr.name(),
                match &pr.manifest {
                    Manifest::Read { target, .. } => target.clone(),
                    _ => "None".to_owned(),
                },
            ),
            None => ("None".to_owned(), "None".to_owned()),
        };
        let x = row.cursor().left();
        kit.caps(row.painter(), x, rect.center().y, "Project", p.dim);
        row.add_space(kit.caps_width(&row, "Project") + size::UNIT * 2.0);
        kit.dropdown(
            &mut row,
            "Project",
            Icon::Tape,
            &name,
            self.project.is_none(),
            size::PROJECT_DROPDOWN_W,
            &[name.as_str()],
        );
        row.add_space(size::UNIT * 2.0);
        let x = row.cursor().left();
        kit.caps(row.painter(), x, rect.center().y, "For", p.dim);
        row.add_space(kit.caps_width(&row, "For") + size::UNIT * 2.0);
        kit.dropdown(
            &mut row,
            "For",
            Icon::Wrist,
            &target,
            target == "None",
            size::TARGET_DROPDOWN_W,
            &[target.as_str()],
        );
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (text, icon, kind, why) in [
                (
                    "Write card",
                    Icon::WriteCard,
                    ButtonKind::Toolbar,
                    menus::NO_CARD,
                ),
                ("Debug", Icon::Debug, ButtonKind::Toolbar, menus::NO_DEBUG),
                ("Run", Icon::Run, ButtonKind::Primary, menus::NO_RUN),
                ("Build", Icon::Build, ButtonKind::Toolbar, menus::NO_BUILD),
            ] {
                kit.button(
                    ui,
                    Button::quiet(text).kind(kind).icon(icon).unavailable(why),
                );
            }
        });
    }

    fn rail(&mut self, kit: &Kit<'_>, ui: &mut Ui, rect: Rect) {
        let p = kit.p();
        let focus = self.focus_request == Some(Place::Rail);
        let mut r = kit.region(
            ui,
            rect,
            Place::Rail.name(),
            Role::Navigation,
            p.ground,
            focus,
        );
        self.places.push((Place::Rail, rect));
        r.painter().line_segment(
            [rect.right_top(), rect.right_bottom()],
            kit.theme.frame(p.frame_dim),
        );
        let mut y = rect.top() + size::UNIT * 2.0;
        for page in Page::ALL {
            let item = Rect::from_min_size(
                egui::pos2(rect.left(), y),
                egui::vec2(rect.width(), size::RAIL_ITEM_H),
            );
            if kit
                .rail_item(&mut r, item, page.icon(), page.name(), self.page == page)
                .clicked()
            {
                self.page = page;
            }
            y += size::RAIL_ITEM_H;
        }
        let foot = Rect::from_min_max(
            egui::pos2(
                rect.left(),
                rect.bottom() - size::SMALL_H * 2.0 - size::STEP_24,
            ),
            rect.right_bottom(),
        );
        r.scope_builder(
            UiBuilder::new()
                .max_rect(foot)
                .layout(egui::Layout::top_down(egui::Align::Center)),
            |ui| {
                kit.icon_button(ui, Icon::Settings, "Settings", Some(menus::LATER));
                kit.icon_button(ui, Icon::Help, "Help", Some(menus::LATER));
            },
        );
    }

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
        s.painter().line_segment(
            [rect.left_top(), rect.right_top()],
            kit.theme.frame(p.frame_dim),
        );
        let mut right = rect.right();
        let caret = (self.page == Page::Project
            && self.focused_place(ui.ctx()) == Some(Place::Editor))
        .then(|| self.editor.current().and_then(|d| d.caret))
        .flatten();
        if let Some((l, c)) = caret {
            right = kit.status_segment(&mut s, right, rect, None, &format!("Ln {l}, Col {c}"));
        }
        right = kit.status_segment(&mut s, right, rect, Some(Icon::Cable), "Link off");
        right = kit.status_segment(&mut s, right, rect, Some(Icon::Wrist), "Sim off");
        right = kit.status_segment(&mut s, right, rect, None, "SDK not checked");
        if let Some(pr) = &self.project {
            if let Manifest::Read { target, .. } = &pr.manifest {
                right = kit.status_segment(&mut s, right, rect, None, target);
            }
            right = kit.status_segment(&mut s, right, rect, None, &pr.name());
        }
        let (level, text) = &self.status;
        let icon = Rect::from_min_size(
            egui::pos2(rect.left() + size::UNIT * 2.0, rect.top()),
            egui::vec2(size::ICON, rect.height()),
        );
        kit.art
            .icon(s.painter(), icon, level.icon(), level.color(&p));
        let msg = Rect::from_min_max(
            egui::pos2(icon.right() + size::UNIT * 2.0, rect.top()),
            egui::pos2(right - size::UNIT * 2.0, rect.bottom()),
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
    }

    fn project_page(&mut self, kit: &Kit<'_>, ui: &mut Ui, page: Rect) {
        let area = page.shrink2(egui::vec2(size::PAGE_MARGIN_ACROSS, size::PAGE_MARGIN_DOWN));
        let l = &mut self.layout;
        l.files_w = l.files_w.clamp(size::FILES_MIN, size::FILES_MAX);
        let files_w = if l.files { l.files_w } else { 0.0 };
        let gap_files = if l.files { size::GAP_ACROSS } else { 0.0 };
        let max_sim = area.width() - files_w - gap_files - size::GAP_ACROSS - size::FILES_MIN;
        l.sim_w = l.sim_w.clamp(size::SIM_W, max_sim.max(size::SIM_W));
        l.bottom_h = l.bottom_h.clamp(
            size::BOTTOM_MIN,
            (area.height() / 2.0).max(size::BOTTOM_MIN),
        );
        let sim_left = area.right() - l.sim_w;
        let left_right = sim_left - size::GAP_ACROSS;
        let top_bottom = if l.bottom {
            area.bottom() - l.bottom_h - size::GAP_DOWN
        } else {
            area.bottom()
        };
        let files = Rect::from_min_max(
            area.left_top(),
            egui::pos2(area.left() + files_w, top_bottom),
        );
        let editor = Rect::from_min_max(
            egui::pos2(area.left() + files_w + gap_files, area.top()),
            egui::pos2(left_right, top_bottom),
        );
        let sim = Rect::from_min_max(
            egui::pos2(sim_left, area.top()),
            egui::pos2(area.right(), area.bottom() - size::TAPE_H - size::GAP_DOWN),
        );
        let tape = Rect::from_min_max(
            egui::pos2(sim_left, area.bottom() - size::TAPE_H),
            area.right_bottom(),
        );
        let bottom = Rect::from_min_max(
            egui::pos2(area.left(), area.bottom() - l.bottom_h),
            egui::pos2(left_right, area.bottom()),
        );
        let (show_files, show_bottom) = (l.files, l.bottom);

        if show_files {
            self.files_pane(kit, ui, files);
        }
        self.editor_pane(kit, ui, editor);
        let fr = self.focus_request;
        let run = Button::quiet("Run")
            .kind(ButtonKind::Primary)
            .icon(Icon::Run)
            .unavailable(menus::NO_RUN);
        kit.pane(ui, sim, Place::Simulator.name(), true, fr == Some(Place::Simulator), |ui, r| {
            kit.empty_state(ui, r.content, Icon::Wrist, "No machine is running. Run builds the tape, boots it in the emulator and shows the screen here.", &[run]);
        });
        self.places.push((Place::Simulator, sim));
        let build = Button::quiet("Build")
            .kind(ButtonKind::Toolbar)
            .icon(Icon::Build)
            .unavailable(menus::NO_BUILD)
            .small();
        kit.pane(
            ui,
            tape,
            Place::Tape.name(),
            true,
            fr == Some(Place::Tape),
            |ui, r| {
                kit.empty_state(
                    ui,
                    r.content,
                    Icon::Tape,
                    "No tape yet. Build makes one.",
                    &[build],
                );
            },
        );
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
                    let ids: Vec<Id> = pr
                        .rows
                        .iter()
                        .map(|row| Id::unique(("tree", &row.path)))
                        .collect();
                    for (i, row) in pr.rows.iter().enumerate() {
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
                            row.depth,
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
            ui.memory_mut(|m| m.request_focus(id));
        }
        if let Some(pr) = self.project.as_mut() {
            if let Some((path, to)) = toggle {
                match to {
                    Some(o) => pr.set_open(&path, o),
                    None => pr.toggle(&path),
                }
                pr.selected = Some(path);
                if let Some(e) = pr.error.clone() {
                    self.status = (Level::Error, e);
                }
            }
            if let Some(path) = &open_file {
                pr.selected = Some(path.clone());
            }
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
        kit.pane(ui, rect, Place::Editor.name(), !has_docs, fr, |ui, r| {
            if has_docs {
                editor.tabs(kit, ui, r.header);
            }
            let mut body = r.content;
            if let Some((level, text)) = &banner {
                let mut b = ui.new_child(UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
                let other = Button::quiet("Open another folder").unavailable(menus::NO_DIALOG);
                kit.banner(&mut b, *level, text, Some(other));
                body.min.y = b.min_rect().bottom() + size::UNIT * 2.0;
            }
            if no_project {
                let new = Button::quiet("New project").kind(ButtonKind::Primary).icon(Icon::Add).unavailable(menus::LATER);
                let open = Button::quiet("Open folder").icon(Icon::Folder).unavailable(menus::NO_DIALOG);
                kit.empty_state(ui, body, Icon::Tape, "No project is open. Make one from a template, or open a folder that holds a robco.toml.", &[new, open]);
            } else if !has_docs {
                kit.empty_state(ui, body, Icon::File, "No file is open. Choose one in Files.", &[]);
            } else {
                editor.body(kit, ui, body);
            }
        });
        self.places.push((Place::Editor, rect));
    }

    fn bottom_pane(&mut self, kit: &Kit<'_>, ui: &mut Ui, rect: Rect) {
        let fr = self.focus_request == Some(Place::Bottom);
        let mut tab = self.bottom_tab;
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
                x = tr.right() + crate::theme::stroke::FRAME;
                if out.chosen {
                    tab = t;
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
                        .kind(ButtonKind::Toolbar)
                        .icon(Icon::Debug)
                        .unavailable(menus::NO_DEBUG)
                        .small();
                    kit.empty_state(
                        ui,
                        r.content,
                        Icon::Debug,
                        "Debug starts a run with the debugger attached.",
                        &[b],
                    );
                }
            }
        });
        self.bottom_tab = tab;
        self.places.push((Place::Bottom, rect));
    }

    fn later_page(&mut self, kit: &Kit<'_>, ui: &mut Ui, page: Rect, which: Page) {
        let area = page.shrink2(egui::vec2(size::PAGE_MARGIN_ACROSS, size::PAGE_MARGIN_DOWN));
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
        let (Some(q), Some(text)) = (self.editor.question, self.editor.question_text()) else {
            return;
        };
        let p = kit.p();
        egui::Area::new(Id::unique("scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(full.min)
            .show(ctx, |ui| {
                ui.painter()
                    .rect_filled(full, CornerRadius::ZERO, kit.theme.scrim());
                ui.allocate_rect(full, Sense::click());
            });
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
                        if kit
                            .button(ui, Button::quiet("Save").kind(ButtonKind::Primary))
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
                        if kit.button(ui, Button::quiet("Do not save")).clicked() {
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

fn stroke_w() -> f32 {
    crate::theme::stroke::FRAME
}

impl eframe::App for Studio {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        Studio::ui(self, ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
