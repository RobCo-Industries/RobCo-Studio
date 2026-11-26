// SPDX-License-Identifier: GPL-3.0-or-later

//! The menus, described once. The row in the window reads this model, and so
//! would a native menu bar. Each item has its command, its shortcut, and,
//! when this release cannot do it, the reason. An item can sit in a submenu
//! of its menu, and can have a separator before it. The rows of View, Theme
//! come from the list of themes, so the menus are made from it.

use crate::screen::Held;
use crate::theme::Spec;
use std::borrow::Cow;

/// A row of View, Theme that takes a theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    /// The theme at this place in the list the menus were made from.
    At(usize),
    /// The changed theme, not saved.
    Changed,
}

/// A command a menu item gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Command {
    NewProject,
    OpenFolder,
    OpenRecent,
    Save,
    SaveAll,
    CloseTab,
    OpenInEditor,
    Settings,
    Quit,
    Undo,
    Redo,
    CutCopyPaste,
    Find,
    FindNext,
    FindPrevious,
    GoToLine,
    PageProject,
    PageCard,
    PageDevice,
    PageSdk,
    ToggleFiles,
    ToggleBottom,
    SimulatorPlace,
    ResetLayout,
    CompactHeader,
    NextPane,
    PreviousPane,
    TextLarger,
    TextSmaller,
    TextReset,
    Theme(ThemeId),
    ThemeEdit,
    ThemeAdd,
    ThemeFolder,
    /// A note of View, Theme: it cannot be picked.
    ThemeHeld(Held),
    HighLegibility,
    ProjectSettings,
    NewFile,
    Export,
    ShowFolder,
    Build,
    Run,
    Debug,
    Stop,
    RunTests,
    NextError,
    PreviousError,
    Breakpoint,
    WriteCard,
    ResetMachine,
    KeyLegend,
    SimZoom(u8),
    InsertTape,
    EjectTape,
    Continue,
    Step,
    Break,
    GiveKeysBack,
    Connect,
    SaveLog,
    PushAndRun,
    Documentation,
    SdkCheck,
    SystemReport,
    About,
}

/// A key of a shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mods {
    /// No modifier.
    None,
    /// Cmd on macOS, Ctrl elsewhere.
    Cmd,
    /// Cmd or Ctrl, and Shift.
    CmdShift,
    /// Cmd or Ctrl, and Alt.
    CmdAlt,
    /// Shift.
    Shift,
    /// Ctrl and Alt on every system.
    CtrlAlt,
    /// Ctrl on every system.
    Ctrl,
}

/// A shortcut: modifiers and a key name as the menu shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    /// The modifiers.
    pub mods: Mods,
    /// The key, as shown.
    pub key: &'static str,
}

impl Shortcut {
    /// The shortcut as text for this system.
    pub fn shown(&self, mac: bool) -> String {
        let cmd = if mac { "Cmd" } else { "Ctrl" };
        match self.mods {
            Mods::None => self.key.to_owned(),
            Mods::Cmd => format!("{cmd}+{}", self.key),
            Mods::CmdShift => format!("{cmd}+Shift+{}", self.key),
            Mods::CmdAlt => format!("{cmd}+Alt+{}", self.key),
            Mods::Shift => format!("Shift+{}", self.key),
            Mods::CtrlAlt => format!("Ctrl+Alt+{}", self.key),
            Mods::Ctrl => format!("Ctrl+{}", self.key),
        }
    }
}

/// One item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Its text.
    pub text: Cow<'static, str>,
    /// What it does.
    pub command: Command,
    /// Its shortcut.
    pub shortcut: Option<Shortcut>,
    /// Why this release cannot do it, or None.
    pub unavailable: Option<&'static str>,
    /// The submenu it sits in, or None for the menu itself.
    pub group: Option<&'static str>,
    /// A separator comes before it.
    pub separator: bool,
    /// It is a changed theme that is not saved: it shows the square and
    /// "not saved" that an unsaved file has.
    pub unsaved: bool,
}

/// The submenu of the themes, in View.
pub const THEME: &str = "Theme";
/// The most themes of a person's own that View, Theme lists.
pub const THEMES_IN_MENU: usize = 8;
/// The row after them when there are more. It opens the editor.
pub const MORE_THEMES: &str = "More themes";
/// The note of the changed theme's row.
pub const NOT_SAVED: &str = "not saved";

/// One menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    /// Its title.
    pub title: &'static str,
    /// Its items.
    pub items: Vec<Item>,
}

/// Said by an item a later release brings.
pub const LATER: &str = "Not in this release of Studio. Nothing was done.";

/// The jobs drawn ready whose commands do not exist yet, with their
/// names. Their buttons and menu items act, and the act only says so on
/// the status line. A real command takes its job out of this list.
pub const NOT_YET: [(Command, &str); 5] = [
    (Command::Build, "Build"),
    (Command::Run, "Run"),
    (Command::Debug, "Debug"),
    (Command::WriteCard, "Write card"),
    (Command::ThemeFolder, "Show the themes folder"),
];

/// The name of `c` when it is a job of [`NOT_YET`].
pub fn not_yet(c: Command) -> Option<&'static str> {
    NOT_YET.iter().find(|(n, _)| *n == c).map(|(_, name)| *name)
}

/// What a job of [`NOT_YET`] says when it is asked to act.
pub fn not_yet_news(name: &str) -> String {
    format!("{name} is not in this version yet.")
}
/// Said by an edit command the editor takes from its keys only.
pub const KEYS_ONLY: &str = "Use the shortcut in the editor. The menu item does not act yet.";

const fn sc(mods: Mods, key: &'static str) -> Option<Shortcut> {
    Some(Shortcut { mods, key })
}

const fn it(
    text: &'static str,
    command: Command,
    shortcut: Option<Shortcut>,
    unavailable: Option<&'static str>,
) -> Item {
    Item {
        text: Cow::Borrowed(text),
        command,
        shortcut,
        unavailable,
        group: None,
        separator: false,
        unsaved: false,
    }
}

/// The rows of View, Theme, in order: the themes that ship; a separator
/// and the person's own, sorted by name without regard to case, the first
/// [`THEMES_IN_MENU`] then [`MORE_THEMES`]; the changed theme, when there
/// is one; a separator, Edit theme, Add a theme file and Show the themes
/// folder; a separator and one note for each thing in `held`.
///
/// `themes` is every theme known, in any order. A row picks a theme by its
/// place in `themes`. `changed` is the changed theme, not saved.
pub fn theme_items(themes: &[Spec], changed: Option<&Spec>, held: &[Held]) -> Vec<Item> {
    let row = |text: Cow<'static, str>, command: Command| Item {
        text,
        group: Some(THEME),
        ..it("", command, None, None)
    };
    let mut out: Vec<Item> = themes
        .iter()
        .enumerate()
        .filter(|(_, t)| t.is_shipped())
        .map(|(i, t)| row(t.name.clone(), Command::Theme(ThemeId::At(i))))
        .collect();
    let mut own: Vec<(usize, &Spec)> = themes
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.is_shipped())
        .collect();
    own.sort_by_cached_key(|(_, t)| t.name.to_lowercase());
    for (k, (i, t)) in own.iter().take(THEMES_IN_MENU).enumerate() {
        out.push(Item {
            separator: k == 0,
            ..row(t.name.clone(), Command::Theme(ThemeId::At(*i)))
        });
    }
    if own.len() > THEMES_IN_MENU {
        out.push(row(Cow::Borrowed(MORE_THEMES), Command::ThemeEdit));
    }
    if let Some(c) = changed {
        out.push(Item {
            unsaved: true,
            ..row(
                Cow::Owned(format!("{}, changed", c.name)),
                Command::Theme(ThemeId::Changed),
            )
        });
    }
    out.push(Item {
        shortcut: sc(Mods::CmdShift, "T"),
        separator: true,
        ..row(Cow::Borrowed("Edit theme"), Command::ThemeEdit)
    });
    out.push(row(Cow::Borrowed("Add a theme file"), Command::ThemeAdd));
    out.push(row(
        Cow::Borrowed("Show the themes folder"),
        Command::ThemeFolder,
    ));
    for (k, h) in held.iter().enumerate() {
        out.push(Item {
            separator: k == 0,
            ..row(
                Cow::Borrowed(crate::theme::held_note(*h)),
                Command::ThemeHeld(*h),
            )
        });
    }
    out
}

/// Every menu of the design's menu table, in order, with the themes that
/// ship in View, Theme, none changed and none held.
pub fn menus() -> Vec<Menu> {
    menus_with(theme_items(&crate::theme::SHIPPED, None, &[]))
}

/// Every menu of the design's menu table, in order, with `theme` as the
/// rows of View, Theme, as [`theme_items`] makes them.
pub fn menus_with(theme: Vec<Item>) -> Vec<Menu> {
    use Command as C;
    use Mods as M;
    let l = Some(LATER);
    let k = Some(KEYS_ONLY);
    let mut looks = theme;
    looks.push(it(
        "High legibility",
        C::HighLegibility,
        sc(M::CmdShift, "L"),
        None,
    ));
    let mut view = vec![
        it("Project", C::PageProject, sc(M::Cmd, "1"), None),
        it("Card", C::PageCard, sc(M::Cmd, "2"), None),
        it("Device", C::PageDevice, sc(M::Cmd, "3"), None),
        it("SDK", C::PageSdk, sc(M::Cmd, "4"), None),
        it(
            "Show or hide files",
            C::ToggleFiles,
            sc(M::CmdShift, "F"),
            None,
        ),
        it(
            "Show or hide the bottom pane",
            C::ToggleBottom,
            sc(M::Cmd, "J"),
            None,
        ),
        it(
            "Simulator at the side or in the centre",
            C::SimulatorPlace,
            sc(M::CmdShift, "M"),
            l,
        ),
        it("Reset the layout", C::ResetLayout, None, None),
        it("Compact header", C::CompactHeader, None, None),
        it("Next pane", C::NextPane, sc(M::None, "F6"), None),
        it("Previous pane", C::PreviousPane, sc(M::Shift, "F6"), None),
        it("Larger text", C::TextLarger, sc(M::Cmd, "="), l),
        it("Smaller text", C::TextSmaller, sc(M::Cmd, "-"), l),
        it("Reset text size", C::TextReset, sc(M::Cmd, "0"), l),
    ];
    view.extend(looks);
    vec![
        Menu {
            title: "File",
            items: vec![
                it("New project", C::NewProject, sc(M::CmdShift, "N"), l),
                it("Open folder", C::OpenFolder, sc(M::Cmd, "O"), None),
                it("Open recent", C::OpenRecent, None, l),
                it("Save", C::Save, sc(M::Cmd, "S"), None),
                it("Save all", C::SaveAll, sc(M::CmdAlt, "S"), None),
                it("Close tab", C::CloseTab, sc(M::Cmd, "W"), None),
                it(
                    "Open in your editor",
                    C::OpenInEditor,
                    sc(M::CmdShift, "E"),
                    l,
                ),
                it("Settings", C::Settings, sc(M::Cmd, ","), l),
                it("Quit", C::Quit, sc(M::Cmd, "Q"), None),
            ],
        },
        Menu {
            title: "Edit",
            items: vec![
                it("Undo", C::Undo, sc(M::Cmd, "Z"), k),
                it("Redo", C::Redo, sc(M::CmdShift, "Z"), k),
                it("Cut, Copy, Paste, Select all", C::CutCopyPaste, None, k),
                it("Find and replace", C::Find, sc(M::Cmd, "F"), None),
                it("Find next", C::FindNext, sc(M::None, "F3"), None),
                it("Find previous", C::FindPrevious, sc(M::Shift, "F3"), None),
                it("Go to line", C::GoToLine, sc(M::Ctrl, "G"), l),
            ],
        },
        Menu {
            title: "View",
            items: view,
        },
        Menu {
            title: "Project",
            items: vec![
                it("Project settings", C::ProjectSettings, None, l),
                it("New file", C::NewFile, sc(M::Cmd, "N"), l),
                it("Export", C::Export, sc(M::Cmd, "E"), l),
                it("Show the folder", C::ShowFolder, None, l),
            ],
        },
        Menu {
            title: "Build",
            items: vec![
                it("Build", C::Build, sc(M::Cmd, "B"), None),
                it("Run", C::Run, sc(M::Cmd, "R"), None),
                it("Debug", C::Debug, sc(M::CmdShift, "D"), None),
                it("Stop", C::Stop, sc(M::Cmd, "."), l),
                it("Run tests", C::RunTests, sc(M::Cmd, "T"), l),
                it("Next error", C::NextError, sc(M::None, "F8"), l),
                it("Previous error", C::PreviousError, sc(M::Shift, "F8"), l),
                it(
                    "Set or clear a breakpoint",
                    C::Breakpoint,
                    sc(M::None, "F9"),
                    l,
                ),
                it("Write card", C::WriteCard, sc(M::CmdShift, "K"), None),
            ],
        },
        Menu {
            title: "Simulator",
            items: vec![
                it(
                    "Reset the machine",
                    C::ResetMachine,
                    sc(M::CmdShift, "R"),
                    l,
                ),
                it("Key legend", C::KeyLegend, None, l),
                it("Scale 1x", C::SimZoom(1), None, None),
                it("Scale 2x", C::SimZoom(2), None, None),
                it("Scale 3x", C::SimZoom(3), None, None),
                it("Insert tape", C::InsertTape, sc(M::CmdShift, "I"), l),
                it("Eject tape", C::EjectTape, sc(M::CmdShift, "J"), l),
                it("Continue", C::Continue, sc(M::None, "F5"), l),
                it("Step", C::Step, sc(M::None, "F10"), l),
                it("Break", C::Break, sc(M::Shift, "F5"), l),
                it(
                    "Give the keys back",
                    C::GiveKeysBack,
                    sc(M::CtrlAlt, "G"),
                    l,
                ),
            ],
        },
        Menu {
            title: "Device",
            items: vec![
                it("Connect", C::Connect, None, l),
                it("Save the log", C::SaveLog, None, l),
                it("Push and run", C::PushAndRun, sc(M::CmdShift, "P"), l),
            ],
        },
        Menu {
            title: "Help",
            items: vec![
                it("Documentation", C::Documentation, sc(M::None, "F1"), l),
                it("Run SDK check", C::SdkCheck, None, l),
                it("Copy system report", C::SystemReport, None, l),
                it("About RobCo Studio", C::About, None, l),
            ],
        },
    ]
}

/// The id of item `i` of menu `m`, as a native menu carries it.
pub fn item_id(m: usize, i: usize) -> String {
    format!("{m}.{i}")
}

/// The command of an item id in `all`, or None if it names no item.
pub fn command_for_id(all: &[Menu], id: &str) -> Option<Command> {
    let (m, i) = id.split_once('.')?;
    Some(
        all.get(m.parse::<usize>().ok()?)?
            .items
            .get(i.parse::<usize>().ok()?)?
            .command,
    )
}

/// What state a menu item shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// No state: the item only acts.
    Plain,
    /// On or off, flipped by the item.
    Toggle,
    /// One of a set, named here; the chosen one is marked.
    Choice(&'static str),
}

/// The state command `c` shows in a menu. The match names every
/// command, as `crate::app::verdict` does.
pub fn check(c: Command) -> Check {
    use Command as C;
    match c {
        C::ToggleFiles | C::ToggleBottom | C::CompactHeader | C::HighLegibility => Check::Toggle,
        C::PageProject | C::PageCard | C::PageDevice | C::PageSdk => Check::Choice("page"),
        C::Theme(_) => Check::Choice("theme"),
        C::SimZoom(_) => Check::Choice("scale"),
        // Every other command by name, with no catch-all: a new command
        // does not build until it says whether it has a state.
        C::NewProject
        | C::OpenFolder
        | C::OpenRecent
        | C::Save
        | C::SaveAll
        | C::CloseTab
        | C::OpenInEditor
        | C::Settings
        | C::Quit
        | C::Undo
        | C::Redo
        | C::CutCopyPaste
        | C::Find
        | C::FindNext
        | C::FindPrevious
        | C::GoToLine
        | C::SimulatorPlace
        | C::ResetLayout
        | C::NextPane
        | C::PreviousPane
        | C::TextLarger
        | C::TextSmaller
        | C::TextReset
        | C::ThemeEdit
        | C::ThemeAdd
        | C::ThemeFolder
        | C::ThemeHeld(_)
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
        | C::About => Check::Plain,
    }
}

/// The kind of a native menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeKind {
    /// An item with no mark.
    Plain,
    /// An item with a check mark.
    Check,
}

/// One item of a native menu bar, as the program's state gives it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeItem {
    /// The id the native item carries, by [`item_id`].
    pub id: String,
    /// The index of its menu in [`menus`].
    pub menu: usize,
    /// What it does.
    pub command: Command,
    /// Plain or check.
    pub kind: NativeKind,
    /// Its text.
    pub title: String,
    /// The submenu it sits in, or None for the menu itself.
    pub group: Option<&'static str>,
    /// A separator comes before it.
    pub separator: bool,
    /// It can act now.
    pub enabled: bool,
    /// Its mark is on. Always false for a plain item.
    pub checked: bool,
    /// The changed theme, not saved.
    pub unsaved: bool,
    /// The key it carries. None for an item this release cannot do, so
    /// the key still reaches the window.
    pub shortcut: Option<Shortcut>,
}

/// Every item of the model, in order, as a native menu bar shows it for
/// `studio` now. The bar builds and syncs from this list alone.
pub fn native_items(studio: &crate::Studio) -> Vec<NativeItem> {
    let mut out = Vec::new();
    for (m, menu) in studio.menus().iter().enumerate() {
        for (i, it) in menu.items.iter().enumerate() {
            let kind = match check(it.command) {
                Check::Plain => NativeKind::Plain,
                Check::Toggle | Check::Choice(_) => NativeKind::Check,
            };
            out.push(NativeItem {
                id: item_id(m, i),
                menu: m,
                command: it.command,
                kind,
                title: it.text.to_string(),
                group: it.group,
                separator: it.separator,
                enabled: studio.unavailable(it).is_none(),
                unsaved: it.unsaved,
                checked: kind == NativeKind::Check && studio.checked(it.command),
                shortcut: it.shortcut.filter(|_| it.unavailable.is_none()),
            });
        }
    }
    out
}

/// The bar must be built again: an item came or went, or one changed its
/// id, text, kind, place or key. A change of enabled state or mark alone
/// is a sync.
pub fn shape_changed(old: &[NativeItem], new: &[NativeItem]) -> bool {
    let shape = |i: &NativeItem| {
        (
            i.id.clone(),
            i.menu,
            i.command,
            i.kind,
            i.title.clone(),
            i.group,
            i.separator,
            i.shortcut,
        )
    };
    old.len() != new.len() || old.iter().zip(new).any(|(a, b)| shape(a) != shape(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_menus_in_order() {
        let t: Vec<_> = menus().iter().map(|m| m.title).collect();
        assert_eq!(
            t,
            [
                "File",
                "Edit",
                "View",
                "Project",
                "Build",
                "Simulator",
                "Device",
                "Help"
            ]
        );
    }

    #[test]
    fn no_shortcut_twice() {
        let mut seen = std::collections::BTreeSet::new();
        for m in menus() {
            for i in m.items {
                if let Some(s) = i.shortcut {
                    assert!(seen.insert(s.shown(true)), "{} twice", s.shown(true));
                }
            }
        }
    }

    #[test]
    fn ids_lead_back_to_their_command() {
        let all = menus();
        for (m, menu) in all.iter().enumerate() {
            for (i, it) in menu.items.iter().enumerate() {
                assert_eq!(command_for_id(&all, &item_id(m, i)), Some(it.command));
            }
        }
        assert_eq!(command_for_id(&all, "99.0"), None);
        assert_eq!(command_for_id(&all, "x"), None);
    }

    /// A theme of a person's own, named `name`.
    fn own(name: &str) -> Spec {
        Spec {
            name: Cow::Owned(name.to_owned()),
            ..crate::theme::SHIPPED[1].clone()
        }
    }

    fn texts(items: &[Item]) -> Vec<String> {
        items.iter().map(|i| i.text.to_string()).collect()
    }

    #[test]
    fn view_has_the_theme_submenu() {
        let view = menus().into_iter().find(|m| m.title == "View").unwrap();
        let theme: Vec<_> = view
            .items
            .iter()
            .filter(|i| i.group == Some(THEME))
            .cloned()
            .collect();
        assert_eq!(
            texts(&theme),
            [
                "Green",
                "Amber",
                "Blue",
                "White",
                "Arcade",
                "Flat",
                "Edit theme",
                "Add a theme file",
                "Show the themes folder"
            ]
        );
        let seps: Vec<_> = theme.iter().map(|i| i.separator).collect();
        assert_eq!(
            seps,
            [false, false, false, false, false, false, true, false, false]
        );
        // High legibility comes right under Theme.
        let at = view
            .items
            .iter()
            .position(|i| i.command == Command::HighLegibility)
            .unwrap();
        assert_eq!(view.items[at - 1].command, Command::ThemeFolder);
        let edit = theme.iter().find(|i| i.text == "Edit theme").unwrap();
        assert_eq!(
            edit.shortcut.map(|s| s.shown(true)),
            Some("Cmd+Shift+T".into())
        );
        assert_eq!(
            edit.shortcut.map(|s| s.shown(false)),
            Some("Ctrl+Shift+T".into())
        );
        // No phosphor item and no Screen submenu are left.
        assert!(view.items.iter().all(|i| i.group != Some("Screen")));
        assert_eq!(view.items.iter().filter(|i| i.text == "Green").count(), 1);
    }

    /// The person's own come after the six, sorted without regard to
    /// case, each picking its own place in the list.
    #[test]
    fn theme_rows_in_order() {
        let mut list = crate::theme::SHIPPED.to_vec();
        list.extend([own("ember"), own("Dusk"), own("aqua")]);
        let items = theme_items(&list, None, &[]);
        assert_eq!(texts(&items[6..9]), ["aqua", "Dusk", "ember"]);
        assert!(items[6].separator && !items[7].separator);
        assert_eq!(items[6].command, Command::Theme(ThemeId::At(8)));
        assert_eq!(items[8].command, Command::Theme(ThemeId::At(6)));
        assert_eq!(items[0].command, Command::Theme(ThemeId::At(0)));
        assert_eq!(items[9].text, "Edit theme");
        assert!(items.iter().all(|i| i.group == Some(THEME) && !i.unsaved));
    }

    /// The changed theme comes after the themes, before Edit theme.
    #[test]
    fn the_changed_row() {
        let list = crate::theme::SHIPPED.to_vec();
        let changed = own("Ember");
        let items = theme_items(&list, Some(&changed), &[]);
        let at = items.iter().position(|i| i.unsaved).unwrap();
        assert_eq!(at, 6);
        assert_eq!(items[at].text, "Ember, changed");
        assert_eq!(items[at].command, Command::Theme(ThemeId::Changed));
        assert!(!items[at].separator);
        assert_eq!(items[at + 1].text, "Edit theme");
        assert_eq!(check(items[at].command), Check::Choice("theme"));
    }

    /// Eight of a person's own are listed; the ninth brings "More themes".
    #[test]
    fn more_themes_at_nine() {
        let names = [
            "kilo", "Juliet", "india", "Hotel", "golf", "Foxtrot", "echo", "Delta", "charlie",
            "Bravo", "alpha",
        ];
        for n in [8, 9, 11] {
            let mut list = crate::theme::SHIPPED.to_vec();
            list.extend(names[..n].iter().map(|x| own(x)));
            let items = theme_items(&list, None, &[]);
            let own_rows: Vec<_> = items[6..]
                .iter()
                .take_while(|i| matches!(i.command, Command::Theme(_)))
                .collect();
            assert_eq!(own_rows.len(), 8, "{n}");
            let more = items.iter().any(|i| i.text == MORE_THEMES);
            assert_eq!(more, n > 8, "{n}");
            if n == 11 {
                assert_eq!(own_rows[0].text, "alpha");
                assert_eq!(own_rows[7].text, "Hotel");
                assert_eq!(items[14].text, MORE_THEMES);
                assert_eq!(items[14].command, Command::ThemeEdit);
            }
        }
    }

    /// One note for each thing that holds the theme, after a separator.
    #[test]
    fn one_note_per_held() {
        let list = crate::theme::SHIPPED.to_vec();
        assert!(
            theme_items(&list, None, &[])
                .iter()
                .all(|i| !matches!(i.command, Command::ThemeHeld(_)))
        );
        let all = [Held::Legible, Held::Still, Held::NoAdapter];
        let items = theme_items(&list, None, &all);
        let notes: Vec<_> = items
            .iter()
            .filter(|i| matches!(i.command, Command::ThemeHeld(_)))
            .collect();
        assert_eq!(notes.len(), 3);
        assert_eq!(
            notes.iter().map(|i| i.text.as_ref()).collect::<Vec<_>>(),
            [
                "Shown flat. High legibility is on.",
                "Shown still. Reduce motion is on.",
                "Shown simpler. No graphics adapter."
            ]
        );
        assert!(notes[0].separator && !notes[1].separator);
        assert_eq!(
            items.last().map(|i| i.command),
            Some(Command::ThemeHeld(Held::NoAdapter))
        );
        for h in all {
            let one = theme_items(&list, None, &[h]);
            let n: Vec<_> = one
                .iter()
                .filter(|i| matches!(i.command, Command::ThemeHeld(_)))
                .collect();
            assert_eq!(n.len(), 1);
            assert_eq!(n[0].command, Command::ThemeHeld(h));
            assert_eq!(n[0].text, crate::theme::held_note(h));
        }
    }

    #[test]
    fn jobs_not_here_yet_act_and_say_so() {
        let all: Vec<_> = menus().into_iter().flat_map(|m| m.items).collect();
        for (c, name) in NOT_YET {
            let i = all.iter().find(|i| i.command == c).unwrap();
            assert!(i.unavailable.is_none(), "{name}");
            assert_eq!(i.text, name);
            assert_eq!(not_yet(c), Some(name));
        }
        assert_eq!(not_yet_news("Build"), "Build is not in this version yet.");
    }
}
