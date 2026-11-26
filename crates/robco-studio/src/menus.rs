// SPDX-License-Identifier: GPL-3.0-or-later

//! The menus, described once. The row in the window reads this model, and so
//! would a native menu bar. Each item has its command, its shortcut, and,
//! when this release cannot do it, the reason.

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
    Phosphor(crate::theme::Scheme),
    EffectsOff,
    EffectsSoft,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    /// Its text.
    pub text: &'static str,
    /// What it does.
    pub command: Command,
    /// Its shortcut.
    pub shortcut: Option<Shortcut>,
    /// Why this release cannot do it, or None.
    pub unavailable: Option<&'static str>,
}

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
/// Said by Build.
pub const NO_BUILD: &str = "Build is not in this release of Studio. Nothing was built.";
/// Said by Run.
pub const NO_RUN: &str = "Run is not in this release of Studio. No tape ran.";
/// Said by Debug.
pub const NO_DEBUG: &str = "Debug is not in this release of Studio. No debugger started.";
/// Said by Write card.
pub const NO_CARD: &str = "Write card is not in this release of Studio. Nothing was written.";
/// Said by Open folder.
pub const NO_DIALOG: &str =
    "Studio has no folder dialog yet. Start it with a folder: robco-studio PATH.";
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
        text,
        command,
        shortcut,
        unavailable,
    }
}

/// Every menu of the design's menu table, in order.
pub fn menus() -> Vec<Menu> {
    use Command as C;
    use Mods as M;
    let l = Some(LATER);
    let k = Some(KEYS_ONLY);
    let mut phosphor: Vec<Item> = crate::theme::Scheme::ALL
        .iter()
        .map(|s| it(s.name(), C::Phosphor(*s), None, None))
        .collect();
    phosphor.extend([
        it("Effects off", C::EffectsOff, None, None),
        it("Effects soft", C::EffectsSoft, None, None),
        it(
            "High legibility",
            C::HighLegibility,
            sc(M::CmdShift, "L"),
            None,
        ),
    ]);
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
    view.extend(phosphor);
    vec![
        Menu {
            title: "File",
            items: vec![
                it("New project", C::NewProject, sc(M::CmdShift, "N"), l),
                it(
                    "Open folder",
                    C::OpenFolder,
                    sc(M::Cmd, "O"),
                    Some(NO_DIALOG),
                ),
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
                it("Build", C::Build, sc(M::Cmd, "B"), Some(NO_BUILD)),
                it("Run", C::Run, sc(M::Cmd, "R"), Some(NO_RUN)),
                it("Debug", C::Debug, sc(M::CmdShift, "D"), Some(NO_DEBUG)),
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
                it(
                    "Write card",
                    C::WriteCard,
                    sc(M::CmdShift, "K"),
                    Some(NO_CARD),
                ),
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

/// The command of an item id, or None if it names no item.
pub fn command_for_id(id: &str) -> Option<Command> {
    let (m, i) = id.split_once('.')?;
    let all = menus();
    Some(
        all.get(m.parse::<usize>().ok()?)?
            .items
            .get(i.parse::<usize>().ok()?)?
            .command,
    )
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
        for (m, menu) in menus().iter().enumerate() {
            for (i, it) in menu.items.iter().enumerate() {
                assert_eq!(command_for_id(&item_id(m, i)), Some(it.command));
            }
        }
        assert_eq!(command_for_id("99.0"), None);
        assert_eq!(command_for_id("x"), None);
    }

    #[test]
    fn jobs_say_why() {
        let all: Vec<_> = menus().into_iter().flat_map(|m| m.items).collect();
        for c in [
            Command::Build,
            Command::Run,
            Command::Debug,
            Command::WriteCard,
        ] {
            let i = all.iter().find(|i| i.command == c).unwrap();
            assert!(i.unavailable.is_some());
        }
    }
}
