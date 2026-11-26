// SPDX-License-Identifier: GPL-3.0-or-later

//! The window, headless, through the accessibility tree.

use eframe::egui::{self, Event, Key, Modifiers, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use robco_studio::Studio;
use robco_studio::app::{Front, Page, Place};
use std::path::{Path, PathBuf};

const MAIN_C: &str = "#include <pipos/pip.h>\n\nint pip_main(void)\n{\n\treturn 0;\n}\n";

/// A sample project folder, removed when the test ends, pass or fail.
struct Sample(PathBuf);

impl std::ops::Deref for Sample {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Sample {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A sample project in a fresh folder of its own.
fn sample(tag: &str) -> std::io::Result<Sample> {
    let d = Sample(
        std::env::temp_dir().join(format!("robco-studio-test-{tag}-{}", std::process::id())),
    );
    let _ = std::fs::remove_dir_all(&*d);
    std::fs::create_dir_all(d.join("src"))?;
    std::fs::write(d.join("src/main.c"), MAIN_C)?;
    std::fs::write(d.join("notes.txt"), "plain\r\ntext\r\n")?;
    Ok(d)
}

/// The sample project with a manifest: PIPES, for Pip-OS ABI 1.3.
fn pipes(tag: &str) -> std::io::Result<Sample> {
    let d = sample(tag)?;
    std::fs::write(
        d.join("robco.toml"),
        "format = 1\n\n[project]\nname = \"PIPES\"\ntitle = \"Pipes\"\nversion = \"1.0.0\"\nkind = \"holotape\"\nsubtype = \"game\"\n\n[target]\nplatform = \"pipos\"\nabi = \"1.3\"\n\n[build]\nbackend = \"robco\"\nsources = [\"src/*.c\"]\n",
    )?;
    Ok(d)
}

fn harness(folder: Option<&Path>) -> Harness<'static, Studio> {
    harness_at(folder, 720.0)
}

fn harness_at(folder: Option<&Path>, height: f32) -> Harness<'static, Studio> {
    harness_sized(folder, egui::vec2(1280.0, height))
}

fn harness_sized(folder: Option<&Path>, size: egui::Vec2) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(size)
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), Studio::new(folder));
    h.run_steps(3);
    h
}

fn open_main(h: &mut Harness<'static, Studio>) {
    h.get_by_label("src").click();
    h.run_steps(2);
    h.get_by_label("main.c").click();
    h.run_steps(2);
}

#[test]
fn every_region_has_its_name() {
    let d = sample("regions").unwrap();
    let h = harness(Some(&*d));
    for name in [
        "Menu bar",
        "Toolbar",
        "Page",
        "Status bar",
        "Files",
        "Editor",
        "Simulator",
        "Tape",
        "Bottom pane",
    ] {
        assert!(
            h.query_all_by_label(name)
                .any(|n| n.accesskit_node().role() != Role::MenuItem),
            "{name}"
        );
    }
    for menu in [
        "File",
        "Edit",
        "View",
        "Project",
        "Build",
        "Simulator",
        "Device",
        "Help",
    ] {
        assert!(
            h.query_all_by_label(menu)
                .any(|n| n.accesskit_node().role() == Role::MenuItem),
            "{menu}"
        );
    }
    for page in ["Project", "Card", "Device", "SDK"] {
        assert!(
            h.query_all_by_label(page)
                .any(|n| n.accesskit_node().role() == Role::Tab),
            "{page}"
        );
    }
    h.get_by_label("RobCo Studio");
}

/// The tab of `page` in the pages control.
fn page_tab<'a>(h: &'a Harness<'static, Studio>, page: &'a str) -> Option<egui_kittest::Node<'a>> {
    h.query_all_by_label(page)
        .find(|n| n.accesskit_node().role() == Role::Tab)
}

/// The pages the control shows selected.
fn selected_pages(h: &Harness<'static, Studio>) -> Vec<String> {
    ["Project", "Card", "Device", "SDK"]
        .into_iter()
        .filter(|p| page_tab(h, p).is_some_and(|n| n.accesskit_node().is_selected() == Some(true)))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_pages_control_is_one_named_tab_list_in_the_toolbar() {
    let h = harness(None);
    let list = h
        .query_all_by_label("Pages")
        .find(|n| n.accesskit_node().role() == Role::TabList)
        .expect("a tab list named Pages");
    let tool = h.state().place_rect(Place::Toolbar).unwrap();
    assert!(tool.contains_rect(list.rect()), "{:?}", list.rect());
    for p in ["Project", "Card", "Device", "SDK"] {
        assert!(
            list.rect().contains_rect(page_tab(&h, p).unwrap().rect()),
            "{p}"
        );
    }
    assert_eq!(selected_pages(&h), ["Project"]);
}

#[test]
fn no_widget_is_named_for_the_rail() {
    let h = harness(None);
    assert!(h.query_all_by_label("Rail").next().is_none());
    assert!(
        h.query_all(egui_kittest::kittest::by().role(Role::Navigation))
            .next()
            .is_none()
    );
}

#[test]
fn the_pages_control_switches_pages() {
    let mut h = harness(None);
    h.get_by_label(
        "No project is open. Make one from a template, or open a folder that holds a robco.toml.",
    );
    h.get_by_label("No files yet.");
    let sdk = h
        .query_all_by_label("SDK")
        .find(|n| n.accesskit_node().role() == Role::Tab)
        .unwrap();
    sdk.click();
    h.run_steps(2);
    assert_eq!(h.state().page, Page::Sdk);
    h.get_by_label("Studio has not fetched the package index yet.");
    h.query_all_by_label("Card")
        .find(|n| n.accesskit_node().role() == Role::Tab)
        .unwrap()
        .click();
    h.run_steps(2);
    assert_eq!(h.state().page, Page::Card);
    assert_eq!(selected_pages(&h), ["Card"]);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num1);
    h.run_steps(2);
    assert_eq!(h.state().page, Page::Project);
    assert_eq!(selected_pages(&h), ["Project"]);
}

#[test]
fn the_arrow_keys_move_between_pages_in_one_tab_stop() {
    let mut h = harness(None);
    page_tab(&h, "Project").unwrap().focus();
    h.run_steps(2);
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(h.state().page, Page::Card);
    assert_eq!(selected_pages(&h), ["Card"]);
    assert!(
        page_tab(&h, "Card").unwrap().accesskit_node().is_focused(),
        "focus follows"
    );
    h.key_press(Key::ArrowLeft);
    h.run_steps(2);
    h.key_press(Key::ArrowLeft);
    h.run_steps(2);
    assert_eq!(h.state().page, Page::Sdk, "the left arrow wraps");
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(h.state().page, Page::Project, "the right arrow wraps");
    // One stop: Tab from the first part leaves the control, it does not
    // walk its parts.
    h.key_press(Key::Tab);
    h.run_steps(2);
    assert!(h.ctx.memory(|m| m.focused()).is_some());
    for p in ["Project", "Card", "Device", "SDK"] {
        assert!(
            !page_tab(&h, p).unwrap().accesskit_node().is_focused(),
            "{p}"
        );
    }
    assert_eq!(h.state().page, Page::Project);
}

#[test]
fn the_selection_follows_the_view_menu_and_the_shortcuts() {
    let mut h = harness(None);
    for (key, page, name) in [
        (Key::Num2, Page::Card, "Card"),
        (Key::Num3, Page::Device, "Device"),
        (Key::Num4, Page::Sdk, "SDK"),
        (Key::Num1, Page::Project, "Project"),
    ] {
        h.key_press_modifiers(Modifiers::COMMAND, key);
        h.run_steps(2);
        assert_eq!(h.state().page, page);
        assert_eq!(selected_pages(&h), [name]);
    }
    h.state_mut()
        .pending
        .push(robco_studio::menus::Command::PageDevice);
    h.run_steps(2);
    assert_eq!(h.state().page, Page::Device);
    assert_eq!(selected_pages(&h), ["Device"]);
}

#[test]
fn files_open_edit_and_save() {
    let d = sample("edit").unwrap();
    let mut h = harness(Some(&*d));
    h.get_by_label("notes.txt");
    assert!(h.query_by_label("main.c").is_none(), "folders start closed");
    open_main(&mut h);
    assert_eq!(h.state().editor.current().unwrap().text, MAIN_C);
    let end = MAIN_C.chars().count();
    h.state_mut().editor.docs[0].pending_sel = Some((end, end));
    h.run_steps(3);
    h.event(Event::Text("// end\n".into()));
    h.run_steps(2);
    assert!(h.state().editor.current().unwrap().is_dirty());
    h.get_by_label("main.c, not saved");
    assert_eq!(
        std::fs::read_to_string(d.join("src/main.c")).unwrap(),
        MAIN_C,
        "nothing written yet"
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run_steps(2);
    assert_eq!(
        std::fs::read(d.join("src/main.c")).unwrap(),
        format!("{MAIN_C}// end\n").into_bytes()
    );
    assert!(!h.state().editor.current().unwrap().is_dirty());
}

#[test]
fn crlf_file_keeps_its_bytes() {
    let d = sample("crlf").unwrap();
    let mut h = harness(Some(&*d));
    h.get_by_label("notes.txt").click();
    h.run_steps(2);
    let doc = h.state().editor.current().unwrap();
    assert_eq!(doc.text, "plain\ntext\n");
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run_steps(2);
    assert_eq!(
        std::fs::read(d.join("notes.txt")).unwrap(),
        b"plain\r\ntext\r\n"
    );
}

#[test]
fn an_unsaved_file_asks_before_it_is_closed() {
    let d = sample("ask").unwrap();
    let mut h = harness(Some(&*d));
    open_main(&mut h);
    h.state_mut().editor.docs[0].pending_sel = Some((0, 0));
    h.run_steps(3);
    h.event(Event::Text("x".into()));
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(2);
    assert!(
        h.query_all_by_label("Save src/main.c before closing?")
            .next()
            .is_some()
    );
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert_eq!(h.state().editor.docs.len(), 1, "Cancel keeps the tab");
    assert!(h.state().editor.docs[0].text.starts_with('x'));
    h.get_by_label("Close main.c").click();
    h.run_steps(2);
    h.get_by_label("Do not save").click();
    h.run_steps(2);
    assert!(h.state().editor.docs.is_empty());
    assert_eq!(
        std::fs::read_to_string(d.join("src/main.c")).unwrap(),
        MAIN_C
    );
}

#[test]
fn quitting_with_edits_asks_and_save_writes() {
    let d = sample("quit").unwrap();
    let mut h = harness(Some(&*d));
    open_main(&mut h);
    h.state_mut().editor.docs[0].pending_sel = Some((0, 0));
    h.run_steps(3);
    h.event(Event::Text("y".into()));
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Q);
    h.run_steps(2);
    assert!(!h.state().editor.quit_allowed);
    assert!(
        h.query_all_by_label("Save src/main.c before closing?")
            .next()
            .is_some()
    );
    h.query_all_by_label("Save")
        .find(|n| n.accesskit_node().role() == Role::Button)
        .unwrap()
        .click();
    h.run_steps(2);
    assert!(h.state().editor.quit_allowed);
    assert_eq!(
        std::fs::read_to_string(d.join("src/main.c")).unwrap(),
        format!("y{MAIN_C}")
    );
}

#[test]
fn f6_moves_through_the_panes_in_reading_order() {
    let d = sample("f6").unwrap();
    let mut h = harness(Some(&*d));
    let mut seen = Vec::new();
    for _ in 0..9 {
        h.key_press(Key::F6);
        h.run_steps(2);
        seen.push(h.state().focused_place(&h.ctx));
    }
    use Place::*;
    let want = [
        Toolbar, Files, Editor, Simulator, Tape, Bottom, Status, Toolbar, Files,
    ];
    assert_eq!(seen, want.map(Some).to_vec());
    h.key_press_modifiers(Modifiers::SHIFT, Key::F6);
    h.run_steps(2);
    assert_eq!(h.state().focused_place(&h.ctx), Some(Toolbar));
}

#[test]
fn find_and_replace() {
    let d = sample("find").unwrap();
    let mut h = harness(Some(&*d));
    open_main(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    h.state_mut().editor.find.query = "return".into();
    h.state_mut().editor.find.replace = "goto".into();
    h.run_steps(1);
    h.get_by_label("Next").click();
    h.run_steps(4);
    h.get_by_label("1 of 1");
    h.query_all_by_label("Replace")
        .find(|n| n.accesskit_node().role() == Role::Button)
        .unwrap()
        .click();
    h.run_steps(3);
    assert!(
        h.state()
            .editor
            .current()
            .unwrap()
            .text
            .contains("\tgoto 0;")
    );
}

#[test]
fn a_folder_without_manifest_says_so() {
    let d = sample("nomanifest").unwrap();
    let h = harness(Some(&*d));
    h.get_by_label("This folder has no robco.toml, so Studio cannot build it.");
}

#[test]
fn the_sample_folder_goes_when_the_test_ends() {
    let path = {
        let d = sample("drop").unwrap();
        assert!(d.exists());
        d.to_path_buf()
    };
    assert!(!path.exists());
}

#[test]
fn two_files_with_one_name_get_their_own_tabs() {
    let d = sample("samename").unwrap();
    std::fs::create_dir_all(d.join("tests")).unwrap();
    std::fs::write(d.join("tests/main.c"), "int t;\n").unwrap();
    let mut h = harness(Some(&*d));
    open_main(&mut h);
    h.query_all_by_label("main.c")
        .find(|n| n.accesskit_node().role() == Role::Tab)
        .unwrap()
        .focus();
    h.run_steps(2);
    let tab = h.ctx.memory(|m| m.focused());
    assert!(tab.is_some(), "the tab took focus");
    h.state_mut()
        .editor
        .open(&d.join("tests/main.c"), "tests/main.c".into());
    h.run_steps(3);
    h.get_by_label("src/main.c");
    h.get_by_label("tests/main.c");
    assert_eq!(
        h.ctx.memory(|m| m.focused()),
        tab,
        "focus stays on the tab whose label grew"
    );
    h.get_by_label("Close tests/main.c").click();
    h.run_steps(2);
    let docs = &h.state().editor.docs;
    assert_eq!(docs.len(), 1);
    assert!(docs[0].path.ends_with("src/main.c"));
}

#[test]
fn replace_goes_forward_and_stops_at_the_last_match() {
    let d = sample("replace").unwrap();
    std::fs::write(d.join("a.txt"), "x a b a c a\n").unwrap();
    let mut h = harness(Some(&*d));
    h.get_by_label("a.txt").click();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    h.state_mut().editor.find.query = "a".into();
    h.state_mut().editor.find.replace = "aa".into();
    h.run_steps(1);
    h.get_by_label("Next").click();
    h.run_steps(4);
    for _ in 0..6 {
        h.query_all_by_label("Replace")
            .find(|n| n.accesskit_node().role() == Role::Button)
            .unwrap()
            .click();
        h.run_steps(4);
    }
    assert_eq!(h.state().editor.current().unwrap().text, "x aa b aa c aa\n");
}

/// At every editor width the layout allows, from the widest down to the
/// narrowest, each control of the find bar lies inside the editor pane,
/// and each field keeps at least its smallest width.
#[test]
fn the_find_bar_fits_the_editor() {
    use robco_studio::theme::size;
    let d = sample("findfit").unwrap();
    let mut h = harness(Some(&*d));
    open_main(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    h.state_mut().editor.find.query = "return".into();
    h.state_mut().editor.find.replace = "goto".into();
    h.run_steps(3);
    let mut widths = Vec::new();
    let mut sim = size::SIM_W;
    loop {
        h.state_mut().layout.sim_w = sim;
        h.run_steps(3);
        let editor = h.state().place_rect(Place::Editor).unwrap();
        widths.push(editor.width());
        let (left, right) = (editor.left(), editor.right());
        for label in [
            "Find",
            "Replace with",
            "Previous",
            "Next",
            "Replace",
            "Replace all",
            "Close find",
            "1 matches",
        ] {
            let mut seen = false;
            for n in h.query_all_by_label(label) {
                let r = n.rect();
                if r.top() < editor.top() || r.top() > editor.bottom() {
                    continue; // not the find bar
                }
                seen = true;
                assert!(
                    r.left() > left && r.right() < right && r.width() > 0.0,
                    "{label} {r:?} is not inside {left}..{right}"
                );
                // The node is the text inside the field's padding.
                if n.accesskit_node().role() == Role::TextInput {
                    assert!(
                        r.width() >= size::FIND_MIN - size::UNIT * 4.0,
                        "{label} {r:?} at {right}"
                    );
                }
            }
            assert!(seen, "{label} at editor width {}", editor.width());
        }
        // The layout clamps the simulator: once the editor stops shrinking,
        // this is the narrowest editor.
        if widths.len() > 1 && widths[widths.len() - 1] >= widths[widths.len() - 2] {
            break;
        }
        sim += 20.0;
    }
    let narrowest = widths.iter().copied().fold(f32::INFINITY, f32::min);
    assert!(narrowest <= size::FILES_MIN + 1.0, "{widths:?}");
}

/// A project folder that is there but cannot be listed is reported at
/// open, as the tree reports it, and the status line does not say opened.
#[cfg(unix)]
#[test]
fn an_unlistable_folder_says_so_at_open() {
    use std::os::unix::fs::PermissionsExt;
    struct Locked(PathBuf);
    impl Drop for Locked {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let d = Locked(
        std::env::temp_dir().join(format!("robco-studio-test-locked-{}", std::process::id())),
    );
    let _ = std::fs::remove_dir_all(&d.0);
    std::fs::create_dir_all(&d.0).unwrap();
    std::fs::set_permissions(&d.0, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&d.0).is_ok() {
        eprintln!("skipped: this user can list a mode 000 folder, likely root");
        return;
    }
    let h = harness(Some(&d.0));
    let (level, text) = &h.state().status;
    assert_eq!(*level, robco_studio::kit::Level::Error);
    assert!(text.starts_with("Studio could not read"), "{text}");
    assert!(!text.contains("Opened"), "{text}");
    h.get_by_label(text);
}

/// With more tabs than the header holds, the active tab is drawn inside
/// the editor, and the menu at the end lists every tab.
#[test]
fn tabs_past_the_header_scroll_into_view() {
    let d = sample("manytabs").unwrap();
    let names: Vec<String> = (0..30).map(|i| format!("file_number_{i:02}.txt")).collect();
    for n in &names {
        std::fs::write(d.join(n), "x\n").unwrap();
    }
    let mut h = harness(Some(&*d));
    for n in &names {
        let p = d.join(n);
        h.state_mut().open_file(&p);
        h.run_steps(2);
    }
    let editor = h
        .state()
        .place_rect(Place::Editor)
        .expect("the editor is drawn");
    let last = names.last().unwrap();
    let tab = h
        .query_all_by_label(last)
        .find(|n| n.accesskit_node().role() == Role::Tab)
        .expect("the last tab");
    let r = tab.accesskit_node().bounding_box().expect("a box");
    assert!(
        r.x0 >= f64::from(editor.left()) && r.x1 <= f64::from(editor.right()),
        "{r:?} outside {editor:?}"
    );
    // The wheel over the strip scrolls it back towards the first tab.
    let y = (r.y0 + r.y1) / 2.0;
    h.input_mut()
        .events
        .push(Event::PointerMoved(egui::pos2(editor.center().x, y as f32)));
    h.input_mut().events.push(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 400.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
        source: egui::MouseWheelSource::Wheel,
    });
    h.run_steps(30);
    let moved = h
        .query_all_by_label(last)
        .find(|n| n.accesskit_node().role() == Role::Tab)
        .and_then(|n| n.accesskit_node().bounding_box())
        .expect("the last tab");
    assert!(
        moved.x0 > r.x0 + 100.0,
        "{moved:?} did not scroll from {r:?}"
    );
    h.get_by_label("All tabs").click();
    h.run_steps(2);
    assert!(
        h.query_all_by_label(&names[0])
            .any(|n| n.accesskit_node().role() == Role::MenuItem)
    );
}

/// The header's one image, and its height.
fn header_h(h: &Harness<'static, Studio>) -> f32 {
    h.query_all_by_label("RobCo Studio")
        .find(|n| n.accesskit_node().role() == Role::Image)
        .map_or(f32::NAN, |n| n.rect().height())
}

#[test]
fn the_header_is_compact_under_720_points_and_by_choice() {
    use robco_studio::theme::{size, stroke};
    let band = |h: f32| h - stroke::FRAME;
    let mut h = harness(None);
    assert_eq!(header_h(&h), band(size::HEADER_H));
    h.state_mut().run(
        &egui::Context::default(),
        robco_studio::menus::Command::CompactHeader,
    );
    h.run_steps(2);
    assert_eq!(header_h(&h), band(size::HEADER_COMPACT_H));
    let h = harness_at(None, 700.0);
    assert_eq!(header_h(&h), band(size::HEADER_COMPACT_H));
}

#[test]
fn the_view_menu_has_compact_header() {
    let view = robco_studio::menus::menus()
        .into_iter()
        .find(|m| m.title == "View")
        .unwrap();
    assert!(view.items.iter().any(|i| i.text == "Compact header"
        && i.command == robco_studio::menus::Command::CompactHeader
        && i.unavailable.is_none()));
}

#[test]
fn the_menu_row_is_absent_where_the_system_has_a_menu_bar() {
    let mut h = harness(None);
    assert!(h.query_by_label("Menu bar").is_some());
    h.state_mut().menu_row = false;
    h.run_steps(2);
    assert!(h.query_by_label("Menu bar").is_none());
    assert!(
        h.query_all_by_label("File")
            .all(|n| n.accesskit_node().role() != Role::MenuItem)
    );
}

/// The rectangles of the toolbar's controls, left to right.
fn toolbar_controls(h: &Harness<'static, Studio>) -> Option<Vec<(String, egui::Rect)>> {
    let mut v = vec![(
        "Pages".to_owned(),
        h.query_all_by_label("Pages")
            .find(|n| n.accesskit_node().role() == Role::TabList)?
            .rect(),
    )];
    for (name, role) in [
        ("Target: Pip-OS, ABI 1.3", Role::ComboBox),
        ("Build", Role::Button),
        ("Run", Role::Button),
        ("Debug", Role::Button),
        ("Write card", Role::Button),
    ] {
        let n = h
            .query_all_by_label(name)
            .find(|n| n.accesskit_node().role() == role)?;
        v.push((name.to_owned(), n.rect()));
    }
    Some(v)
}

#[test]
fn the_toolbar_fits_1280_points_with_no_overlap() {
    use robco_studio::theme::size;
    let d = pipes("toolbar").unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(
            |ui, s: &mut Studio| {
                // The whole screen, as eframe gives it, with no margin.
                let r = ui.ctx().content_rect();
                ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| s.ui(ui));
            },
            Studio::new(Some(&*d)),
        );
    h.run_steps(3);
    let tool = h.state().place_rect(Place::Toolbar).unwrap();
    assert_eq!(tool.width(), 1280.0);
    let all = toolbar_controls(&h).unwrap();
    for (name, r) in &all {
        assert!(tool.contains_rect(*r), "{name} {r:?} outside {tool:?}");
    }
    for w in all.windows(2) {
        assert!(
            w[0].1.right() + size::TOOLBAR_GAP <= w[1].1.left(),
            "{} {:?} crowds {} {:?}",
            w[0].0,
            w[0].1,
            w[1].0,
            w[1].1
        );
    }
    assert_eq!(all.len(), 6, "pages, target, four actions");
    for (name, b) in &all[2..] {
        assert_eq!(b.width(), size::TOOLBAR_ACTION_W, "{name}: one width");
        assert_eq!(b.height(), size::CONTROL_H, "{name}");
    }
    // The dropdown's width holds the longest target name whole.
    assert!(
        all[1].1.width() >= size::TARGET_DROPDOWN_W,
        "{:?}",
        all[1].1
    );
}

/// The rectangle of the footer button `name`.
fn footer_button(h: &Harness<'static, Studio>, name: &str) -> Option<egui::Rect> {
    h.query_all_by_label(name)
        .find(|n| n.accesskit_node().role() == Role::Button)
        .map(|n| n.rect())
}

#[test]
fn settings_and_help_stand_at_the_right_end_of_the_footer() {
    let h = harness(None);
    let foot = h.state().place_rect(Place::Status).unwrap();
    let set = footer_button(&h, "Settings").unwrap();
    let help = footer_button(&h, "Help").unwrap();
    assert!(foot.contains_rect(set) && foot.contains_rect(help));
    assert!(set.right() <= help.left());
    let link = h.get_by_label("Link off").rect();
    assert!(link.right() <= set.left(), "after the last segment");
}

#[test]
fn settings_and_help_are_the_last_to_give_way() {
    let h = harness_sized(None, egui::vec2(400.0, 720.0));
    let foot = h.state().place_rect(Place::Status).unwrap();
    assert!(
        h.query_by_label("Link off").is_none(),
        "the window is narrow enough that segments gave way"
    );
    for name in ["Settings", "Help"] {
        assert!(
            foot.contains_rect(footer_button(&h, name).unwrap()),
            "{name}"
        );
    }
}

#[test]
fn the_log_filters_stand_in_the_tab_row_while_the_log_is_on() {
    let mut h = harness(None);
    assert!(h.query_by_label("Filter the log").is_none());
    h.query_all_by_label("Log")
        .find(|n| n.accesskit_node().role() == Role::Tab)
        .unwrap()
        .click();
    h.run_steps(2);
    let tab = h
        .query_all_by_label("Log")
        .find(|n| n.accesskit_node().role() == Role::Tab)
        .unwrap()
        .rect();
    let field = h.get_by_label("Filter the log").rect();
    assert!(field.center().y > tab.top() && field.center().y < tab.bottom());
    for part in ["All", "Markers", "Text"] {
        assert!(
            h.query_all_by_label(part)
                .any(|n| n.accesskit_node().role() == Role::RadioButton),
            "{part}"
        );
    }
    h.get_by_label("Markers").click();
    h.run_steps(2);
    assert_eq!(h.state().log.show, 1);
    assert!(h.query_by_label("Follow").is_some());
}

#[test]
fn the_new_parts_have_their_names() {
    let d = sample("names").unwrap();
    let h = harness(Some(&*d));
    for (name, role) in [
        ("RobCo Studio", Role::Image),
        ("RobCo Industries", Role::Image),
        ("Stop", Role::Button),
        ("Reset", Role::Button),
        ("Fit", Role::Button),
        ("Smaller", Role::Button),
        ("Larger", Role::Button),
    ] {
        assert!(
            h.query_all_by_label(name)
                .any(|n| n.accesskit_node().role() == role),
            "{name}"
        );
    }
    // A short status line leaves room for the division.
    harness(None).get_by_label("Engineering division");
}

#[test]
fn the_zoom_menu_and_the_row_share_one_step() {
    use robco_studio::menus::Command;
    let mut h = harness(None);
    assert_eq!(h.state().sim_zoom, 1);
    // At the side of a 1280 px window only 1x fits: 2x from the menu
    // is refused and says why, and Larger says the same.
    h.state_mut()
        .run(&egui::Context::default(), Command::SimZoom(2));
    h.run_steps(2);
    assert_eq!(h.state().sim_zoom, 1);
    assert!(h.state().status.1.contains("2x does not fit"));
    let larger = h
        .query_all_by_label("Larger")
        .find(|n| n.accesskit_node().role() == Role::Button)
        .unwrap();
    assert!(larger.accesskit_node().is_disabled());
    assert!(
        larger
            .accesskit_node()
            .description()
            .is_some_and(|s| s.contains("2x does not fit"))
    );
    // With room for it, the menu's step is the row's step.
    let mut h = harness_sized(None, egui::vec2(1400.0, 1200.0));
    h.state_mut().layout.sim_w = 1000.0;
    h.state_mut().layout.files = false;
    h.state_mut().layout.bottom = false;
    h.run_steps(2);
    h.state_mut()
        .run(&egui::Context::default(), Command::SimZoom(2));
    h.run_steps(2);
    assert_eq!(h.state().sim_zoom, 2);
    h.get_by_label("2x");
}

/// The folder name a sample without a manifest goes by.
fn folder_name(d: &Path) -> String {
    d.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[test]
fn a_dropdown_shows_its_list_on_the_click_that_opens_it() {
    let d = pipes("dropdown").unwrap();
    let name = "Pip-OS, ABI 1.3";
    let mut h = harness(Some(&*d));
    h.get_by_label(&format!("Target: {name}")).click();
    // The click's frame lays the new list out unseen, as egui does for
    // every new popup, and asks itself for the frame that shows it.
    h.step();
    assert!(h.ctx.has_requested_repaint());
    h.step();
    assert!(
        h.query_all_by_label(name)
            .any(|n| n.accesskit_node().role() == Role::MenuItem),
        "the list shows in the frame after the click, with no new input"
    );
}

#[test]
fn a_long_project_name_is_cut_and_given_in_full_on_hover() {
    let d = sample("a-long-project-name").unwrap();
    let name = folder_name(&d);
    let mut h = harness(Some(&*d));
    // The status bar names the project too: count before the hover.
    let before = h.query_all_by_label(&name).count();
    tree_root(&h, &name).unwrap().hover();
    h.run_steps(90);
    assert_eq!(
        h.query_all_by_label(&name).count(),
        before + 1,
        "a cut name gives its full text in a tooltip"
    );
}

/// The tree's root row named `name`.
fn tree_root<'a>(h: &'a Harness<'static, Studio>, name: &'a str) -> Option<egui_kittest::Node<'a>> {
    h.query_all_by_label(name)
        .find(|n| n.accesskit_node().role() == Role::TreeItem)
}

/// The toolbar holds no project dropdown and no PROJECT label.
#[test]
fn no_widget_is_named_for_the_project_dropdown() {
    let d = pipes("no-project-dropdown").unwrap();
    let h = harness(Some(&*d));
    let tool = h.state().place_rect(Place::Toolbar).unwrap();
    let combos: Vec<String> = h
        .query_all(egui_kittest::kittest::by().role(Role::ComboBox))
        .filter(|n| tool.contains_rect(n.rect()))
        .filter_map(|n| n.accesskit_node().label())
        .collect();
    assert_eq!(combos, ["Target: Pip-OS, ABI 1.3"]);
    assert!(h.query_by_label("Project: PIPES").is_none());
    assert!(h.query_by_label("Project:").is_none());
}

/// The window's title names the project, then the application.
#[test]
fn the_window_title_follows_the_project() {
    let d = pipes("title").unwrap();
    assert_eq!(harness(Some(&*d)).state().title(), "PIPES - RobCo Studio");
    let f = sample("title-folder").unwrap();
    let name = folder_name(&f);
    assert_eq!(
        harness(Some(&*f)).state().title(),
        format!("{name} - RobCo Studio")
    );
    assert_eq!(harness(None).state().title(), "RobCo Studio");
}

/// The project is the tree's root row: open at first, with the folder
/// under it; closing it hides the tree; arrows and keys reach it.
#[test]
fn the_tree_has_the_project_as_its_root() {
    let d = pipes("tree-root").unwrap();
    let mut h = harness(Some(&*d));
    let root = tree_root(&h, "PIPES").unwrap();
    assert_eq!(root.accesskit_node().data().is_expanded(), Some(true));
    let below = |h: &Harness<'static, Studio>, n: &str| {
        h.query_all_by_label(n)
            .any(|x| x.accesskit_node().role() == Role::TreeItem)
    };
    assert!(below(&h, "src") && below(&h, "robco.toml"));
    // The rows hang one step in from the root.
    assert!(h.get_by_label("src").rect().top() > tree_root(&h, "PIPES").unwrap().rect().top());
    tree_root(&h, "PIPES").unwrap().click();
    h.run_steps(2);
    assert_eq!(
        tree_root(&h, "PIPES")
            .unwrap()
            .accesskit_node()
            .data()
            .is_expanded(),
        Some(false)
    );
    assert!(!below(&h, "src") && !below(&h, "robco.toml"));
    // The keys open it again, and Down moves from the root to its first row.
    tree_root(&h, "PIPES").unwrap().focus();
    h.run_steps(1);
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert!(below(&h, "src"));
    tree_root(&h, "PIPES").unwrap().focus();
    h.run_steps(1);
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    assert!(h.get_by_label("src").accesskit_node().is_focused());
    // One row a press, not two.
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    assert!(h.get_by_label("notes.txt").accesskit_node().is_focused());
    // A folder without a manifest goes by its own name.
    let f = sample("tree-folder").unwrap();
    let h = harness(Some(&*f));
    tree_root(&h, &folder_name(&f)).unwrap();
}

/// What the project dropdown offered lives in the File menu.
#[test]
fn the_file_menu_opens_a_project() {
    use robco_studio::menus::{Command, menus};
    let file = menus().into_iter().find(|m| m.title == "File").unwrap();
    let open = file
        .items
        .iter()
        .find(|i| i.command == Command::OpenFolder)
        .unwrap();
    assert_eq!(open.text, "Open folder");
    assert_eq!(open.unavailable, None);
    let mut h = harness(None);
    h.get_by_label("File").click();
    h.run_steps(2);
    assert!(
        h.query_all_by_label("Open folder")
            .any(|n| n.accesskit_node().role() == Role::MenuItem)
    );
}

/// The empty Tape pane's Build and the empty Debug tab's Debug act and
/// say they are not here yet.
#[test]
fn the_empty_states_jobs_act_and_say_so() {
    let d = sample("empty-jobs").unwrap();
    let mut h = harness(Some(&*d));
    let tape = h.state().place_rect(Place::Tape).unwrap();
    h.query_all_by_label("Build")
        .find(|n| n.accesskit_node().role() == Role::Button && tape.intersects(n.rect()))
        .unwrap()
        .click();
    h.run_steps(3);
    h.get_by_label("Build is not in this version yet.");
    h.query_all_by_label("Debug")
        .find(|n| n.accesskit_node().role() == Role::Tab)
        .unwrap()
        .click();
    h.run_steps(3);
    let bottom = h.state().place_rect(Place::Bottom).unwrap();
    let b = h
        .query_all_by_label("Debug")
        .find(|n| n.accesskit_node().role() == Role::Button && bottom.contains_rect(n.rect()))
        .unwrap();
    assert!(!b.accesskit_node().is_disabled());
    h.state_mut().status = (robco_studio::kit::Level::Info, "Ready.".to_owned());
    h.run_steps(2);
    h.query_all_by_label("Debug")
        .find(|n| n.accesskit_node().role() == Role::Button && bottom.contains_rect(n.rect()))
        .unwrap()
        .click();
    h.run_steps(3);
    h.get_by_label("Debug is not in this version yet.");
}

#[test]
fn a_long_file_name_is_cut_in_the_tree_and_given_in_full_on_hover() {
    let d = sample("longfile").unwrap();
    let name = "a_file_name_far_too_long_for_the_files_pane_to_show_in_one_row.c";
    std::fs::write(d.join(name), "").unwrap();
    let mut h = harness(Some(&*d));
    let before = h.query_all_by_label(name).count();
    h.get_by_label(name).hover();
    h.run_steps(90);
    assert_eq!(
        h.query_all_by_label(name).count(),
        before + 1,
        "a cut name gives its full text in a tooltip"
    );
}

#[test]
fn news_from_a_native_menu_command_shows_without_input() {
    let d = sample("news").unwrap();
    let mut h = harness(Some(&*d));
    h.run_steps(30);
    assert!(!h.ctx.has_requested_repaint(), "the window is idle");
    h.state_mut()
        .pending
        .push(robco_studio::menus::Command::SaveAll);
    h.step();
    assert!(h.ctx.has_requested_repaint());
    h.step();
    h.get_by_label("Nothing to save.");
}

/// With a long line, a scroll to the right leaves the gutter in place, and
/// a scroll down keeps the first number on the first text row in view.
#[test]
fn the_gutter_stays_put_across_and_follows_down() {
    let d = sample("gutter").unwrap();
    let mut long = "x".repeat(2000);
    long.push('\n');
    for i in 0..200 {
        long.push_str(&format!("line {i}\n"));
    }
    std::fs::write(d.join("src/long.c"), long).unwrap();
    let mut h = harness(Some(&*d));
    h.get_by_label("src").click();
    h.run_steps(2);
    h.get_by_label("long.c").click();
    h.run_steps(3);
    let before = h.state().editor.gutter.unwrap();
    assert_eq!(before.offset, egui::Vec2::ZERO);
    let editor = h.state().place_rect(Place::Editor).unwrap();
    h.hover_at(editor.center());
    // Shift turns the wheel sideways.
    for (delta, modifiers) in [
        (egui::vec2(0.0, -400.0), Modifiers::SHIFT),
        (egui::vec2(0.0, -537.0), Modifiers::NONE),
    ] {
        h.event_modifiers(
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta,
                modifiers,
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
            },
            modifiers,
        );
        h.run_steps(4);
    }
    let after = h.state().editor.gutter.unwrap();
    assert!(after.offset.x > 100.0, "{after:?}");
    assert!(after.offset.y > 100.0, "{after:?}");
    assert_eq!(after.rect, before.rect);
    // The first text row in view: the one whose bottom is below the top.
    let line_h = h.state().theme.code_line();
    let (n, y) = after.first.unwrap();
    let row_top = after.text_top - after.offset.y + (n - 1) as f32 * line_h;
    assert_eq!(
        n,
        (after.offset.y / line_h).floor() as usize + 1,
        "{after:?}"
    );
    assert!((y - (row_top + line_h / 2.0)).abs() < 1.0, "{after:?}");
}

/// The four jobs are drawn ready. A click on one, in the toolbar or in the
/// simulator's empty state, says on the status line that it is not here yet.
#[test]
fn the_jobs_act_and_say_they_are_not_here_yet() {
    let d = sample("jobs").unwrap();
    let mut h = harness(Some(&*d));
    for job in ["Build", "Run", "Debug", "Write card"] {
        let b = h
            .query_all_by_label(job)
            .find(|n| {
                n.accesskit_node().role() == Role::Button && !n.accesskit_node().is_disabled()
            })
            .unwrap_or_else(|| panic!("{job} is not an able button"));
        b.click();
        h.run_steps(3);
        h.get_by_label(&format!("{job} is not in this version yet."));
    }
    h.state_mut().status = (robco_studio::kit::Level::Info, "Ready.".to_owned());
    h.run_steps(2);
    // The simulator's own Run, below the toolbar.
    let runs: Vec<_> = h
        .query_all_by_label("Run")
        .filter(|n| n.accesskit_node().role() == Role::Button)
        .map(|n| n.rect())
        .collect();
    assert_eq!(runs.len(), 2, "{runs:?}");
    let low = runs.iter().map(|r| r.top()).fold(f32::MIN, f32::max);
    h.query_all_by_label("Run")
        .find(|n| n.accesskit_node().role() == Role::Button && n.rect().top() == low)
        .unwrap()
        .click();
    h.run_steps(3);
    h.get_by_label("Run is not in this version yet.");
}

/// The Target dropdown names the target with a comma; the Tape pane's
/// NEEDS row states the same ABI as a bare fact.
#[test]
fn the_target_is_named_with_a_comma() {
    let d = pipes("target").unwrap();
    let mut h = harness(Some(&*d));
    h.get_by_label("Target: Pip-OS, ABI 1.3");
    h.get_by_label("Pip-OS ABI 1.3");
    // The dropdown shows the name whole: no tooltip gives it in full.
    h.get_by_label("Target: Pip-OS, ABI 1.3").hover();
    h.run_steps(90);
    assert_eq!(h.query_all_by_label("Pip-OS, ABI 1.3").count(), 0);
}

/// The log filter field leaves the row's gap on both sides: to the
/// segmented control at its left and to Follow at its right.
#[test]
fn the_log_filter_keeps_the_rows_gap() {
    use robco_studio::theme::size;
    let mut h = harness(None);
    h.state_mut().bottom_tab = robco_studio::app::BottomTab::Log;
    h.run_steps(2);
    // The node is the text inside the field's padding.
    let field = h
        .get_by_label("Filter the log")
        .rect()
        .expand2(egui::vec2(size::UNIT * 2.0, size::UNIT));
    let text = h
        .query_all_by_label("Text")
        .find(|n| n.accesskit_node().role() == Role::RadioButton)
        .unwrap()
        .rect();
    let follow = h
        .query_all_by_label("Follow")
        .find(|n| n.accesskit_node().role() == Role::Button)
        .unwrap()
        .rect();
    let gap = size::UNIT * 2.0;
    let before = field.left() - text.right();
    let after = follow.left() - field.right();
    assert!(before >= gap - 0.5, "{text:?} {field:?}");
    assert!(after >= gap - 0.5, "{field:?} {follow:?}");
    assert_eq!(field.width(), size::LOG_FILTER_W);
}

/// What the fake folder chooser was asked, and what it answers.
#[derive(Default)]
struct FakeState {
    /// The answer to give; None keeps the chooser open.
    answer: Option<robco_studio::chooser::Choice>,
    /// The start folder of each time it was shown.
    starts: Vec<Option<PathBuf>>,
}

struct FakeChooser(std::sync::Arc<std::sync::Mutex<FakeState>>);

impl robco_studio::chooser::FolderChooser for FakeChooser {
    fn start(&mut self, _ctx: &egui::Context, start: Option<&Path>) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .starts
            .push(start.map(Path::to_path_buf));
    }
    fn poll(&mut self) -> Option<robco_studio::chooser::Choice> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .answer
            .take()
    }
}

type Fake = std::sync::Arc<std::sync::Mutex<FakeState>>;

/// A window on `folder` whose chooser answers `answer`.
fn with_fake(
    folder: Option<&Path>,
    answer: Option<robco_studio::chooser::Choice>,
) -> (Harness<'static, Studio>, Fake) {
    let fake = Fake::default();
    fake.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .answer = answer;
    let studio = Studio::new(folder).with_chooser(Box::new(FakeChooser(fake.clone())));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), studio);
    h.run_steps(3);
    (h, fake)
}

fn press_open(h: &mut Harness<'static, Studio>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::O);
    h.run_steps(3);
}

use robco_studio::chooser::Choice;

#[test]
fn open_folder_switches_the_project() {
    let a = pipes("switch-from").unwrap();
    let b = sample("switch-to").unwrap();
    let (mut h, fake) = with_fake(Some(&*a), Some(Choice::Chosen(b.to_path_buf())));
    assert_eq!(h.state().title(), "PIPES - RobCo Studio");
    press_open(&mut h);
    let name = folder_name(&b);
    assert_eq!(
        fake.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .starts,
        [std::fs::canonicalize(&*a)
            .unwrap()
            .parent()
            .map(Path::to_path_buf)]
    );
    assert_eq!(h.state().title(), format!("{name} - RobCo Studio"));
    assert!(tree_root(&h, &name).is_some());
    assert!(tree_root(&h, "PIPES").is_none());
    assert_eq!(h.state().status, Studio::new(Some(&*b)).status);
}

#[test]
fn open_folder_starts_at_home_without_a_project() {
    let (mut h, fake) = with_fake(None, Some(Choice::Cancelled));
    press_open(&mut h);
    assert_eq!(
        fake.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .starts,
        [std::env::home_dir()]
    );
}

#[test]
fn cancel_in_the_chooser_changes_nothing() {
    let a = pipes("chooser-cancel").unwrap();
    let (mut h, fake) = with_fake(Some(&*a), Some(Choice::Cancelled));
    let status = h.state().status.clone();
    press_open(&mut h);
    assert_eq!(
        fake.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .starts
            .len(),
        1
    );
    assert_eq!(h.state().status, status);
    assert_eq!(h.state().title(), "PIPES - RobCo Studio");
    assert!(tree_root(&h, "PIPES").is_some());
}

#[test]
fn a_chosen_folder_says_what_the_command_line_says() {
    let a = pipes("chooser-msg-from").unwrap();
    // No manifest: the same status line and the same banner.
    let b = sample("chooser-msg-bare").unwrap();
    let (mut h, _) = with_fake(Some(&*a), Some(Choice::Chosen(b.to_path_buf())));
    press_open(&mut h);
    assert_eq!(h.state().status, Studio::new(Some(&*b)).status);
    h.get_by_label("This folder has no robco.toml, so Studio cannot build it.");
    // Not a folder: the project stays, and the line is the same.
    let file = b.join("notes.txt");
    let (mut h, _) = with_fake(Some(&*a), Some(Choice::Chosen(file.clone())));
    press_open(&mut h);
    assert_eq!(h.state().status, Studio::new(Some(&file)).status);
    assert_eq!(h.state().status.0, robco_studio::kit::Level::Error);
    assert_eq!(h.state().title(), "PIPES - RobCo Studio");
}

#[test]
fn edits_are_asked_about_before_the_switch() {
    let a = sample("chooser-edit-from").unwrap();
    let b = pipes("chooser-edit-to").unwrap();
    let (mut h, fake) = with_fake(Some(&*a), Some(Choice::Chosen(b.to_path_buf())));
    open_main(&mut h);
    h.state_mut().editor.docs[0].pending_sel = Some((0, 0));
    h.run_steps(3);
    h.event(Event::Text("x".into()));
    h.run_steps(2);
    press_open(&mut h);
    assert!(
        h.query_all_by_label("Save src/main.c before closing?")
            .next()
            .is_some()
    );
    assert!(tree_root(&h, &folder_name(&a)).is_some(), "no switch yet");
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert!(
        tree_root(&h, &folder_name(&a)).is_some(),
        "Cancel keeps the project"
    );
    assert_eq!(h.state().editor.docs.len(), 1);
    assert!(
        h.state().editor.docs[0].text.starts_with('x'),
        "and the edit"
    );
    // Asked again, Do not save drops the edit and opens the project.
    fake.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .answer = Some(Choice::Chosen(b.to_path_buf()));
    press_open(&mut h);
    h.get_by_label("Do not save").click();
    h.run_steps(3);
    assert!(tree_root(&h, "PIPES").is_some());
    assert!(h.state().editor.docs.is_empty());
    assert_eq!(
        std::fs::read_to_string(a.join("src/main.c")).unwrap(),
        MAIN_C
    );
}

#[test]
fn a_chooser_error_shows_in_the_status_bar() {
    let a = pipes("chooser-error").unwrap();
    let e = "No folder dialog opened.";
    let (mut h, _) = with_fake(Some(&*a), Some(Choice::Failed(e.to_owned())));
    press_open(&mut h);
    assert_eq!(
        h.state().status,
        (robco_studio::kit::Level::Error, e.to_owned())
    );
    assert_eq!(h.state().title(), "PIPES - RobCo Studio");
}

#[test]
fn the_menu_opens_the_chooser_as_the_shortcut_does() {
    let a = pipes("chooser-menu-from").unwrap();
    let b = sample("chooser-menu-to").unwrap();
    let (mut h, fake) = with_fake(Some(&*a), Some(Choice::Chosen(b.to_path_buf())));
    h.get_by_label("File").click();
    h.run_steps(2);
    h.query_all_by_label("Open folder")
        .find(|n| n.accesskit_node().role() == Role::MenuItem)
        .unwrap()
        .click();
    h.run_steps(3);
    assert_eq!(
        fake.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .starts
            .len(),
        1
    );
    assert!(tree_root(&h, &folder_name(&b)).is_some());
}

#[test]
fn an_open_chooser_takes_no_other_command() {
    let a = pipes("chooser-busy").unwrap();
    let (mut h, fake) = with_fake(Some(&*a), None);
    press_open(&mut h);
    assert!(h.state().choosing());
    press_open(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num2);
    h.run_steps(2);
    assert_eq!(
        fake.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .starts
            .len(),
        1,
        "no second chooser"
    );
    assert_eq!(h.state().page, Page::Project, "no other command");
    fake.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .answer = Some(Choice::Cancelled);
    h.run_steps(2);
    assert!(!h.state().choosing());
    press_open(&mut h);
    assert_eq!(
        fake.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .starts
            .len(),
        2
    );
}

/// Opens main.c and notes.txt and edits both.
fn edit_two(h: &mut Harness<'static, Studio>) {
    open_main(h);
    h.get_by_label("notes.txt").click();
    h.run_steps(2);
    for d in &mut h.state_mut().editor.docs {
        d.text.insert(0, 'x');
    }
    h.run_steps(2);
    assert_eq!(h.state().editor.docs.len(), 2);
}

#[test]
fn cancel_on_the_second_question_keeps_both_edits_in_a_switch() {
    let a = sample("chooser-two-from").unwrap();
    let b = pipes("chooser-two-to").unwrap();
    let (mut h, _) = with_fake(Some(&*a), Some(Choice::Chosen(b.to_path_buf())));
    edit_two(&mut h);
    press_open(&mut h);
    h.get_by_label("Do not save").click();
    h.run_steps(3);
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert!(tree_root(&h, &folder_name(&a)).is_some());
    let docs = &h.state().editor.docs;
    assert_eq!(docs.len(), 2, "no document dropped");
    assert!(
        docs.iter().all(|d| d.text.starts_with('x')),
        "no edit dropped"
    );
}

#[test]
fn cancel_on_the_second_question_keeps_both_edits_in_a_quit() {
    let a = sample("quit-two").unwrap();
    let mut h = harness(Some(&*a));
    edit_two(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Q);
    h.run_steps(3);
    h.get_by_label("Do not save").click();
    h.run_steps(3);
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert!(!h.state().editor.quit_allowed);
    let docs = &h.state().editor.docs;
    assert_eq!(docs.len(), 2, "no document dropped");
    assert!(
        docs.iter().all(|d| d.text.starts_with('x')),
        "no edit dropped"
    );
}

#[test]
fn an_open_chooser_takes_no_click() {
    let a = pipes("chooser-click").unwrap();
    let (mut h, _) = with_fake(Some(&*a), None);
    press_open(&mut h);
    assert!(h.state().choosing());
    page_tab(&h, "Card").unwrap().click();
    h.run_steps(2);
    assert_eq!(h.state().page, Page::Project, "the page tab does not act");
    // A screen reader's click reaches the widget past the scrim.
    page_tab(&h, "Card").unwrap().click_accesskit();
    h.run_steps(2);
    assert_eq!(h.state().page, Page::Project, "nor by a screen reader");
    h.get_by_label(robco_studio::app::CHOOSING);
}

#[test]
fn a_project_opened_as_dot_has_an_absolute_root() {
    // The tests run in the crate's folder; "." is that folder, read only.
    let here = std::fs::canonicalize(".").unwrap();
    let (mut h, fake) = with_fake(Some(Path::new(".")), Some(Choice::Cancelled));
    let root = h.state().project.as_ref().unwrap().root.clone();
    assert_eq!(root, here);
    assert_eq!(h.state().title(), "robco-studio - RobCo Studio");
    press_open(&mut h);
    assert_eq!(
        fake.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .starts,
        [here.parent().map(Path::to_path_buf)]
    );
}

/// The Simulator menu's scale items, each with its reason when it is not
/// available. None when the menu or an item is missing.
fn scale_items(h: &mut Harness<'static, Studio>) -> Option<Vec<Option<String>>> {
    h.query_all_by_label("Simulator")
        .find(|n| n.accesskit_node().role() == Role::MenuItem)?
        .click();
    h.run_steps(2);
    let v = ["Scale 1x", "Scale 2x", "Scale 3x"]
        .into_iter()
        .map(|l| {
            let n = h
                .query_all_by_label(l)
                .find(|n| n.accesskit_node().role() == Role::MenuItem)?;
            let a = n.accesskit_node();
            Some(
                a.is_disabled()
                    .then(|| a.description().unwrap_or_default().to_owned()),
            )
        })
        .collect();
    h.key_press(Key::Escape);
    h.run_steps(2);
    v
}

#[test]
fn the_scale_items_follow_the_panes_room() {
    let mut h = harness(None);
    let at_720 = scale_items(&mut h).unwrap();
    assert_eq!(at_720[0], None);
    assert!(
        at_720[1]
            .as_deref()
            .is_some_and(|s| s.contains("2x does not fit"))
    );
    assert!(
        at_720[2]
            .as_deref()
            .is_some_and(|s| s.contains("3x does not fit"))
    );
    let mut h = harness_sized(None, egui::vec2(1400.0, 1200.0));
    h.state_mut().layout.sim_w = 1000.0;
    h.state_mut().layout.files = false;
    h.state_mut().layout.bottom = false;
    h.run_steps(2);
    let roomy = scale_items(&mut h).unwrap();
    assert_eq!(&roomy[..2], [None, None]);
    // A state the window's menu and the macOS bar both read.
    let it = robco_studio::menus::menus()
        .into_iter()
        .flat_map(|m| m.items)
        .find(|i| i.text == "Scale 2x")
        .unwrap();
    assert_eq!(h.state().unavailable(&it), None);
}

/// The room comes from the layout, not from drawing the pane: on the Card
/// page a resize changes the scale items with no Project page drawn, and
/// a chosen 2x that stops fitting becomes 1x with one status line.
#[test]
fn the_scale_items_follow_a_resize_on_another_page() {
    use robco_studio::app::Page;
    use robco_studio::menus::Command;
    let mut h = harness(None);
    h.state_mut().layout.sim_w = 1000.0;
    h.state_mut().layout.files = false;
    h.state_mut().layout.bottom = false;
    h.state_mut().page = Page::Card;
    h.run_steps(2);
    assert!(scale_items(&mut h).unwrap()[1].is_some());
    h.set_size(egui::vec2(1400.0, 1200.0));
    h.run_steps(2);
    assert_eq!(scale_items(&mut h).unwrap()[1], None);
    h.state_mut()
        .run(&egui::Context::default(), Command::SimZoom(2));
    h.run_steps(2);
    assert_eq!(h.state().sim_zoom, 2);
    h.state_mut().status.1.clear();
    h.set_size(egui::vec2(1280.0, 720.0));
    h.run_steps(1);
    assert_eq!(h.state().sim_zoom, 1);
    assert_eq!(
        h.state().status.1,
        "2x no longer fits. The simulator is at 1x."
    );
    // Once: a later frame does not say it again.
    h.state_mut().status.1.clear();
    h.run_steps(2);
    assert!(h.state().status.1.is_empty());
    assert!(scale_items(&mut h).unwrap()[1].is_some());
    assert_eq!(h.state().page, Page::Card);
}

#[test]
fn the_empty_states_open_folder_opens_the_chooser() {
    let b = sample("empty-open-to").unwrap();
    let (mut h, fake) = with_fake(None, Some(Choice::Chosen(b.to_path_buf())));
    h.query_all_by_label("Open folder")
        .find(|n| n.accesskit_node().role() == Role::Button)
        .unwrap()
        .click();
    h.run_steps(3);
    assert_eq!(
        fake.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .starts,
        [std::env::home_dir()]
    );
    assert!(tree_root(&h, &folder_name(&b)).is_some());
    // The banner's button of a folder without robco.toml does the same.
    let c = sample("empty-open-next").unwrap();
    fake.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .answer = Some(Choice::Chosen(c.to_path_buf()));
    h.get_by_label("Open another folder").click();
    h.run_steps(3);
    assert!(tree_root(&h, &folder_name(&c)).is_some());
}

#[test]
fn an_eight_character_name_stays_inside_the_tape_label() {
    let d = pipes("longname").unwrap();
    let manifest = std::fs::read_to_string(d.join("robco.toml")).unwrap();
    std::fs::write(
        d.join("robco.toml"),
        manifest.replace("name = \"PIPES\"", "name = \"LONGNAME\""),
    )
    .unwrap();
    let mut h = harness(Some(&d));
    h.run_steps(2);
    let (field, name) = h.state().tape_label().expect("the tape is shown");
    assert!(name.width() > 0.0);
    assert!(
        field.contains_rect(name),
        "{name:?} is not inside {field:?}"
    );
    let label = h.get_by_label("LONGNAME.HTP");
    assert_eq!(label.accesskit_node().role(), Role::Image);
}

/// The window with a real adapter as far as the rules know, at `level`. No
/// GPU here: the pass does not draw, but every rule around it holds.
fn screen_at(level: robco_studio::theme::Screen) -> Harness<'static, Studio> {
    let mut s = Studio::new(None);
    s.choose_screen(
        robco_studio::screen::Adapter::Hardware,
        Some(false),
        Some(level),
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
    h.run_steps(3);
    h
}

/// Open View, Screen and read each level's item: available, and checked.
/// None when the menu or an item is missing.
fn screen_items(h: &mut Harness<'static, Studio>) -> Option<Vec<(String, bool, bool)>> {
    use egui::accesskit::Toggled;
    h.query_all_by_label("View")
        .find(|n| n.accesskit_node().role() == Role::MenuItem)?
        .click();
    h.run_steps(2);
    let sub = h
        .query_all_by_label("Screen")
        .find(|n| n.accesskit_node().role() == Role::MenuItem)?
        .rect();
    h.hover_at(sub.center());
    h.run_steps(3);
    ["Off", "Soft", "Still", "Live", "Arcade"]
        .iter()
        .map(|l| {
            let n = h
                .query_all_by_label(l)
                .find(|n| n.accesskit_node().role() == Role::MenuItemRadio)?;
            let a = n.accesskit_node();
            Some((
                (*l).to_owned(),
                !a.is_disabled(),
                a.toggled() == Some(Toggled::True),
            ))
        })
        .collect()
}

#[test]
fn the_screen_menu_shows_the_levels_and_the_choice() {
    use robco_studio::theme::Screen;
    // No adapter: the pass levels say why they are not there.
    let mut h = harness(None);
    let items = screen_items(&mut h).unwrap();
    let on: Vec<_> = items.iter().map(|(_, on, _)| *on).collect();
    assert_eq!(on, [true, true, false, false, false]);
    let checked: Vec<_> = items.iter().map(|(_, _, c)| *c).collect();
    assert_eq!(checked, [false, true, false, false, false]);
    let still = h
        .query_all_by_label("Still")
        .find(|n| n.accesskit_node().role() == Role::MenuItemRadio)
        .unwrap();
    assert_eq!(
        still.accesskit_node().description().as_deref(),
        Some(robco_studio::screen::NO_ADAPTER)
    );
    // Off from the menu.
    h.query_all_by_label("Off")
        .find(|n| n.accesskit_node().role() == Role::MenuItemRadio)
        .unwrap()
        .click();
    h.run_steps(2);
    assert_eq!(h.state().theme.screen, Screen::Off);
    assert_eq!(h.state().status.1, "Screen: Off.");

    // With an adapter, Live by default, and every level there.
    let mut h = screen_at(Screen::Live);
    let items = screen_items(&mut h).unwrap();
    assert!(items.iter().all(|(_, on, _)| *on));
    assert!(items[3].2, "{items:?}");
}

#[test]
fn high_legibility_forces_the_screen_off() {
    use robco_studio::theme::Screen;
    let mut h = screen_at(Screen::Arcade);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::L);
    h.run_steps(2);
    assert!(h.state().theme.high_legibility);
    assert_eq!(h.state().theme.screen(), Screen::Off);
    let items = screen_items(&mut h).unwrap();
    let on: Vec<_> = items.iter().map(|(_, on, _)| *on).collect();
    assert_eq!(on, [true, false, false, false, false]);
    assert!(items[0].2, "Off is the choice: {items:?}");
    // No curve while it is off.
    assert!(h.state().theme.halo().is_none());
}

/// A click where the curved screen draws a tab lands on that tab. The
/// same place on the flat screen misses it.
#[test]
fn a_click_on_the_curve_lands_where_it_is_drawn() {
    use robco_studio::screen::drawn_at;
    use robco_studio::theme::Screen;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
    // Where the tab is laid out: the tree follows the curve, so read it
    // with no curve.
    let tab = screen_at(Screen::Soft)
        .query_all_by_label("Card")
        .find(|n| n.accesskit_node().role() == Role::Tab)
        .unwrap()
        .rect();
    let click = |level: Screen| {
        let mut h = screen_at(level);
        // egui grows a hit area by 5 points, which hides a small miss.
        h.ctx
            .all_styles_mut(|s| s.interaction.interact_radius = 0.0);
        h.run_steps(2);
        // The bottom edge of the tab, which the curve draws lower.
        let target = egui::pos2(tab.center().x, tab.max.y - 1.5);
        let glass = drawn_at(target, screen, Screen::Arcade.strengths().curvature);
        assert!((glass - target).length() > 3.0, "the curve moved nothing");
        h.hover_at(glass);
        h.run_steps(1);
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos: glass,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.run_steps(1);
        }
        h.run_steps(1);
        h.state().page
    };
    assert_eq!(click(Screen::Arcade), Page::Card);
    assert_eq!(click(Screen::Soft), Page::Project);
}

#[test]
fn adjust_moves_a_strength_and_reset_brings_it_back() {
    use robco_studio::theme::Screen;
    let mut h = screen_at(Screen::Live);
    h.state_mut()
        .pending
        .push(robco_studio::menus::Command::ScreenAdjust);
    h.run_steps(2);
    assert!(h.state().screen.adjusting);
    let bloom = h
        .query_all_by_label("Bloom")
        .find(|n| n.accesskit_node().role() == Role::Slider)
        .unwrap();
    bloom.set_value_accesskit(0.9);
    h.run_steps(2);
    assert!((h.state().screen.strengths.bloom - 0.9).abs() < 1e-6);
    assert!(h.state().screen_custom());
    assert_eq!(h.state().status.1, "Screen: Custom.");
    // The simulator's Reset is not in this release: the dialog's is.
    h.query_all_by_label("Reset")
        .find(|n| !n.accesskit_node().is_disabled())
        .unwrap()
        .click();
    h.run_steps(2);
    assert_eq!(h.state().screen.strengths, Screen::Live.strengths());
    assert!(!h.state().screen_custom());
    assert_eq!(h.state().status.1, "Screen: Live.");
    // The frame rate choice.
    h.get_by_label("60").click();
    h.run_steps(2);
    assert_eq!(h.state().screen.rate, 60);
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(!h.state().screen.adjusting);
}

/// The delay before the frame the window asks for, or None.
fn repaint_asked(h: &Harness<'static, Studio>) -> Option<std::time::Duration> {
    let d = h
        .output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)?
        .repaint_delay;
    (d < std::time::Duration::from_secs(1000)).then_some(d)
}

#[test]
fn only_a_focused_live_screen_asks_for_frames() {
    use robco_studio::theme::Screen;
    for level in [Screen::Off, Screen::Soft, Screen::Still] {
        let mut h = screen_at(level);
        h.run_steps(4);
        assert_eq!(repaint_asked(&h), None, "{level:?}");
    }
    let mut h = screen_at(Screen::Live);
    h.run_steps(4);
    let d = repaint_asked(&h).unwrap();
    assert!(d <= std::time::Duration::from_secs(1) / 30, "{d:?}");
    // Without persistence: the harness counts some of its own frames as a
    // change, and the afterglow then never ends. The afterglow has its own
    // tests on the clock.
    h.state_mut().screen.strengths.persistence = 0.0;
    h.input_mut().focused = false;
    h.run_steps(4);
    assert_eq!(repaint_asked(&h), None);
}

/// Steps until the window asks for no frame, at most `max`.
fn settle(h: &mut Harness<'static, Studio>, max: u32) -> u32 {
    let mut n = 0;
    loop {
        h.step();
        n += 1;
        if !h.ctx.has_requested_repaint() || n >= max {
            return n;
        }
    }
}

/// Persistence alone, at Still: a change of the picture starts frames that
/// end once the afterglow has faded, and the screen's own frames do not
/// count as changes. Without persistence the window settles at once.
#[test]
fn persistence_alone_asks_for_frames_after_a_change_then_stops() {
    use robco_studio::screen::Adapter;
    use robco_studio::theme::{Screen, Strengths};
    for persistence in [0.0, 1.0] {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 720.0))
            .with_step_dt(1.0 / 30.0)
            .build_ui_state(|ui, s: &mut Studio| s.ui(ui), Studio::new(None));
        let s = h.state_mut();
        s.choose_screen(Adapter::Hardware, Some(false), Some(Screen::Still));
        s.screen.strengths = Strengths {
            persistence,
            ..Screen::Still.strengths()
        };
        assert!(settle(&mut h, 300) < 300, "{persistence}: settles at start");
        h.hover_at(egui::pos2(640.0, 400.0));
        let n = settle(&mut h, 300);
        assert!(n < 300, "{persistence}: the afterglow ends");
        if persistence > 0.0 {
            // At 30 frames a second the full afterglow fades in about 28.
            assert!(n > 20, "{persistence}: the afterglow runs, {n} frames");
        } else {
            assert!(n <= 3, "{persistence}: no afterglow, {n} frames");
        }
    }
}

/// With curvature, each button's reported rectangle holds the point of the
/// glass where a click lands on that button's centre: at the centre, near
/// the toolbar and near a corner, at Live's and Arcade's curvature.
#[test]
fn the_tree_follows_the_curve() {
    use robco_studio::screen::{Adapter, drawn_at, map_pointer};
    use robco_studio::theme::Screen;
    let d = sample("curve").unwrap();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
    for level in [Screen::Live, Screen::Arcade] {
        let at = |curvature: f32| {
            let mut h = harness(Some(&*d));
            let s = h.state_mut();
            s.choose_screen(Adapter::Hardware, Some(false), Some(level));
            s.screen.strengths.curvature = curvature;
            h.run_steps(3);
            h
        };
        let curvature = level.strengths().curvature;
        assert!(curvature > 0.0);
        let flat = at(0.0);
        let curved = at(curvature);
        let mut seen = [false; 3];
        for b in flat.query_all_by_role(Role::Button) {
            let id = b.accesskit_node().id();
            let Some(c) = curved
                .query_all_by_role(Role::Button)
                .find(|n| n.accesskit_node().id() == id)
            else {
                continue;
            };
            let laid = b.rect();
            // The glass point a click goes to so it lands at the centre.
            let glass = drawn_at(laid.center(), screen, curvature);
            assert!((map_pointer(glass, screen, curvature) - laid.center()).length() < 0.05);
            let reported = c.rect();
            assert!(
                reported.expand(0.01).contains(glass),
                "{level:?} {:?}: {reported:?} lacks {glass:?}, laid out at {laid:?}",
                b.accesskit_node().label()
            );
            let p = laid.center();
            let off = (glass - p).length();
            if (p - screen.center()).length() < 300.0 {
                seen[0] = true;
            }
            if p.y < 120.0 && off > 2.0 {
                seen[1] = true;
            }
            let corner = [
                screen.left_top(),
                screen.right_top(),
                screen.left_bottom(),
                screen.right_bottom(),
            ]
            .iter()
            .map(|k| (p - *k).length())
            .fold(f32::MAX, f32::min);
            if corner < 120.0 && off > 5.0 {
                seen[2] = true;
            }
        }
        assert_eq!(seen, [true; 3], "{level:?}: centre, toolbar, corner");
    }
}

/// A window at Live on `folder` whose chooser stays open once asked.
fn live_with_fake(folder: &Path) -> Harness<'static, Studio> {
    let fake = Fake::default();
    let mut s = Studio::new(Some(folder)).with_chooser(Box::new(FakeChooser(fake)));
    s.choose_screen(
        robco_studio::screen::Adapter::Hardware,
        Some(false),
        Some(robco_studio::theme::Screen::Live),
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
    h.run_steps(3);
    h
}

fn menu_item<'a>(
    h: &'a Harness<'static, Studio>,
    label: &'a str,
) -> Option<egui_kittest::Node<'a>> {
    h.query_all_by_label(label)
        .find(|n| n.accesskit_node().role() == Role::MenuItem)
}

/// While the chooser is open the Adjust dialog, drawn above the scrim,
/// takes no drag, no click and no screen reader's action, and a click
/// that would open a menu opens none.
#[test]
fn an_open_chooser_holds_the_adjust_dialog_and_the_menus() {
    let a = pipes("chooser-adjust").unwrap();
    let mut h = live_with_fake(&a);
    h.state_mut()
        .pending
        .push(robco_studio::menus::Command::ScreenAdjust);
    h.run_steps(2);
    let bloom = h
        .query_all_by_label("Bloom")
        .find(|n| n.accesskit_node().role() == Role::Slider)
        .unwrap();
    bloom.set_value_accesskit(0.9);
    h.run_steps(2);
    let before = h.state().screen.strengths;
    assert!((before.bloom - 0.9).abs() < 1e-6);
    press_open(&mut h);
    assert!(h.state().choosing());
    assert!(h.state().screen.adjusting);
    // A drag along the slider, from its left end to its right end.
    let r = h
        .query_all_by_label("Bloom")
        .find(|n| n.accesskit_node().role() == Role::Slider)
        .unwrap()
        .rect();
    let (from, to) = (r.left_center(), r.right_center());
    h.event(Event::PointerMoved(from));
    h.event(Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    h.event(Event::PointerMoved(to));
    h.event(Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    h.query_all_by_label("Bloom")
        .find(|n| n.accesskit_node().role() == Role::Slider)
        .unwrap()
        .set_value_accesskit(0.1);
    h.run_steps(2);
    let resets = h.query_all_by_label("Reset").count();
    assert!(resets > 0);
    for i in 0..resets {
        h.query_all_by_label("Reset").nth(i).unwrap().click();
        h.run_steps(2);
    }
    h.query_all_by_label("Close")
        .next()
        .unwrap()
        .click_accesskit();
    h.run_steps(2);
    assert_eq!(h.state().screen.strengths, before, "the dialog took input");
    assert!(h.state().screen.adjusting, "Close acted");
    // A click on View opens no menu.
    menu_item(&h, "View").unwrap().click();
    h.run_steps(3);
    assert!(menu_item(&h, "Screen").is_none(), "a menu opened");
}

/// The Bloom slider of the Adjust dialog, open on a window at Live whose
/// chooser answers what `fake` holds.
fn adjust_with_fake(folder: &Path) -> (Harness<'static, Studio>, Fake, egui::Rect) {
    let fake = Fake::default();
    let mut s = Studio::new(Some(folder)).with_chooser(Box::new(FakeChooser(fake.clone())));
    s.choose_screen(
        robco_studio::screen::Adapter::Hardware,
        Some(false),
        Some(robco_studio::theme::Screen::Live),
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
    h.run_steps(3);
    h.state_mut()
        .pending
        .push(robco_studio::menus::Command::ScreenAdjust);
    h.run_steps(2);
    let r = bloom(&h).map_or(egui::Rect::NOTHING, |n| n.rect());
    (h, fake, r)
}

fn bloom<'a>(h: &'a Harness<'static, Studio>) -> Option<egui_kittest::Node<'a>> {
    h.query_all_by_label("Bloom")
        .find(|n| n.accesskit_node().role() == Role::Slider)
}

fn close_chooser(h: &mut Harness<'static, Studio>, fake: &Fake) {
    fake.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .answer = Some(Choice::Cancelled);
    h.run_steps(3);
    assert!(!h.state().choosing());
}

/// A press held when the chooser opens and released while it is open
/// ends in egui with no click and no drag once the chooser closes.
#[test]
fn a_press_held_over_the_chooser_ends_with_no_click() {
    let a = pipes("chooser-press").unwrap();
    let (mut h, fake, r) = adjust_with_fake(&a);
    let at = r.right_center() - egui::vec2(4.0, 0.0);
    h.event(Event::PointerMoved(at));
    h.event(Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    // A slider follows a press at once: the value from here on must hold.
    let before = h.state().screen.strengths;
    press_open(&mut h);
    assert!(h.state().choosing());
    h.event(Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    close_chooser(&mut h, &fake);
    assert!(
        h.ctx
            .input(|i| !i.pointer.any_down() && i.pointer.press_origin().is_none()),
        "a press is left down"
    );
    h.event(Event::PointerMoved(r.left_center()));
    h.run_steps(2);
    h.event(Event::PointerMoved(r.center()));
    h.run_steps(2);
    assert_eq!(
        h.state().screen.strengths,
        before,
        "a click or a drag acted"
    );
}

/// A key held when the chooser opens is released for egui, and its
/// repeats after the chooser closes do nothing until a new press.
#[test]
fn a_key_held_over_the_chooser_does_not_repeat() {
    let a = pipes("chooser-key").unwrap();
    let (mut h, fake, _) = adjust_with_fake(&a);
    bloom(&h).unwrap().focus();
    h.run_steps(2);
    let key = |h: &mut Harness<'static, Studio>, pressed, repeat| {
        h.event(Event::Key {
            key: Key::ArrowLeft,
            physical_key: None,
            pressed,
            repeat,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(2);
    };
    let bloom_now = |h: &Harness<'static, Studio>| h.state().screen.strengths.bloom;
    let start = bloom_now(&h);
    key(&mut h, true, false);
    let pressed = bloom_now(&h);
    assert!(pressed < start, "the arrow moves the focused slider");
    press_open(&mut h);
    assert!(h.state().choosing());
    key(&mut h, true, true);
    close_chooser(&mut h, &fake);
    assert!(
        h.ctx.input(|i| !i.key_down(Key::ArrowLeft)),
        "the key is left down"
    );
    bloom(&h).unwrap().focus();
    h.run_steps(2);
    key(&mut h, true, true);
    key(&mut h, true, true);
    assert_eq!(bloom_now(&h), pressed, "a repeat acted");
    key(&mut h, false, false);
    key(&mut h, true, false);
    assert!(bloom_now(&h) < pressed, "a new press acts");
}

/// A key held when the chooser opens whose release goes to the chooser
/// and never reaches the window: after the chooser closes its repeats do
/// nothing, and one new press acts once.
#[test]
fn a_key_released_in_the_chooser_acts_on_its_next_press() {
    let a = pipes("chooser-key-lost").unwrap();
    let (mut h, fake, _) = adjust_with_fake(&a);
    bloom(&h).unwrap().focus();
    h.run_steps(2);
    let key = |h: &mut Harness<'static, Studio>, pressed, repeat| {
        h.event(Event::Key {
            key: Key::ArrowLeft,
            physical_key: None,
            pressed,
            repeat,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(2);
    };
    let bloom_now = |h: &Harness<'static, Studio>| h.state().screen.strengths.bloom;
    key(&mut h, true, false);
    let held = bloom_now(&h);
    press_open(&mut h);
    assert!(h.state().choosing());
    close_chooser(&mut h, &fake);
    bloom(&h).unwrap().focus();
    h.run_steps(2);
    key(&mut h, true, true);
    assert_eq!(bloom_now(&h), held, "a repeat of the old press acted");
    key(&mut h, true, false);
    let once = bloom_now(&h);
    assert!(once < held, "the first new press is dropped");
    h.run_steps(2);
    assert_eq!(bloom_now(&h), once, "the press acted more than once");
}

/// A menu open when the chooser opens is closed.
#[test]
fn the_chooser_closes_an_open_menu() {
    let a = pipes("chooser-menu").unwrap();
    let mut h = live_with_fake(&a);
    menu_item(&h, "View").unwrap().click();
    h.run_steps(3);
    assert!(menu_item(&h, "Screen").is_some(), "View is open");
    h.state_mut()
        .pending
        .push(robco_studio::menus::Command::OpenFolder);
    h.run_steps(3);
    assert!(h.state().choosing());
    assert!(menu_item(&h, "Screen").is_none(), "the menu stayed open");
}

/// A window started on `folder` with the screen chosen for `adapter` and
/// Reduce Motion `reduce`, as `main` starts it, after its first frames.
fn started(
    folder: &Path,
    adapter: robco_studio::screen::Adapter,
    reduce: bool,
) -> Harness<'static, Studio> {
    let mut s = Studio::new(Some(folder));
    s.choose_screen(adapter, Some(reduce), None);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
    h.run_steps(2);
    h
}

/// The held note shows as the tooltip of the status line's level icon.
fn held_tooltip(h: &mut Harness<'static, Studio>, error: &str, note: &str) {
    let line = h.get_by_label(error).rect();
    h.hover_at(egui::pos2(line.left() - 12.0, line.center().y));
    h.run_steps(90);
    h.get_by_label(note);
}

#[test]
fn a_start_error_keeps_the_line_over_the_screen_note() {
    use robco_studio::kit::Level;
    use robco_studio::screen::Adapter;
    let gone = std::env::temp_dir().join(format!("robco-studio-test-gone-{}", std::process::id()));
    let error = Studio::new(Some(&gone)).status;
    assert_eq!(error.0, Level::Error);
    for (adapter, reduce, word) in [
        (Adapter::Software, false, "Screen: Soft."),
        (Adapter::Hardware, true, "Screen: Still."),
    ] {
        let mut h = started(&gone, adapter, reduce);
        assert_eq!(h.state().status, error);
        let note = h.state().held().expect("the screen note is held").clone();
        assert_eq!(note.0, Level::Info);
        assert!(note.1.ends_with(word), "{}", note.1);
        held_tooltip(&mut h, &error.1, &note.1);
    }
}

#[test]
fn a_good_start_shows_the_screen_note() {
    let a = pipes("start-soft").unwrap();
    let h = started(&a, robco_studio::screen::Adapter::Software, false);
    assert!(
        h.state().status.1.ends_with("Screen: Soft."),
        "{:?}",
        h.state().status
    );
    assert!(h.state().held().is_none());
}

#[test]
fn a_plain_message_replaces_an_error_once_it_was_read() {
    use robco_studio::kit::Level;
    let gone = std::env::temp_dir().join(format!("robco-studio-test-gone2-{}", std::process::id()));
    let mut h = harness(Some(&gone));
    h.run_steps(2);
    let error = h.state().status.clone();
    h.state_mut().say(Level::Info, "Plain.".to_owned());
    assert_eq!(h.state().status, error);
    // A higher or equal level always takes the line.
    h.state_mut().say(Level::Error, "Worse.".to_owned());
    assert_eq!(h.state().status.1, "Worse.");
    h.run_steps(1);
    h.key_press(Key::Escape);
    h.run_steps(1);
    h.state_mut().say(Level::Info, "Plain.".to_owned());
    assert_eq!(h.state().status, (Level::Info, "Plain.".to_owned()));
    assert!(h.state().held().is_none());
}

/// A window on a sample project at Live, where Adjust works, with
/// main.c open and edited, and Adjust open.
fn adjusting(tag: &str) -> Option<(Sample, Harness<'static, Studio>)> {
    let d = sample(tag).ok()?;
    let mut s = Studio::new(Some(&*d));
    s.choose_screen(
        robco_studio::screen::Adapter::Hardware,
        Some(false),
        Some(robco_studio::theme::Screen::Live),
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), s);
    h.run_steps(3);
    open_main(&mut h);
    h.state_mut().editor.docs[0].text.insert(0, 'x');
    h.state_mut()
        .pending
        .push(robco_studio::menus::Command::ScreenAdjust);
    h.run_steps(3);
    (h.state().front() == Front::Adjust).then_some((d, h))
}

fn shown(h: &Harness<'static, Studio>, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

const ASK_CLOSE: &str = "Save src/main.c before closing?";
const ASK_QUIT: &str = ASK_CLOSE;

fn adjust_item() -> Option<robco_studio::menus::Item> {
    robco_studio::menus::screen_items()
        .into_iter()
        .find(|i| i.command == robco_studio::menus::Command::ScreenAdjust)
}

/// Adjust is modal: closing a tab behind it does nothing at all.
#[test]
fn adjust_is_modal_and_close_tab_waits() {
    let (_d, mut h) = adjusting("adjust-modal").unwrap();
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(3);
    assert!(!shown(&h, ASK_CLOSE), "no question behind Adjust");
    assert_eq!(h.state().editor.docs.len(), 1, "the tab stays");
    assert_eq!(h.state().front(), Front::Adjust);
    assert!(shown(&h, robco_studio::app::ADJUST_TITLE));
    // The native menu bar says why.
    let close = robco_studio::menus::menus()
        .into_iter()
        .flat_map(|m| m.items)
        .find(|i| i.command == robco_studio::menus::Command::CloseTab)
        .unwrap();
    assert_eq!(
        h.state().unavailable(&close).as_deref(),
        Some(robco_studio::app::ADJUSTING)
    );
}

/// A question raised while Adjust is open, by quitting, comes in front.
/// Adjust is put aside, then comes back as it was. Escape cancels the
/// question only; a click on Cancel does the same.
#[test]
fn a_question_beats_adjust_and_adjust_comes_back() {
    let (_d, mut h) = adjusting("adjust-question").unwrap();
    let strengths = h.state().screen.strengths;
    for answer_by_click in [false, true] {
        h.key_press_modifiers(Modifiers::COMMAND, Key::Q);
        h.run_steps(2);
        assert_eq!(h.state().front(), Front::Question);
        assert!(shown(&h, ASK_QUIT));
        assert!(
            !shown(&h, robco_studio::app::ADJUST_TITLE),
            "Adjust is put aside"
        );
        // No Adjust command while a question is open, in any menu.
        assert_eq!(
            h.state().unavailable(&adjust_item().unwrap()).as_deref(),
            Some(robco_studio::app::ASKING)
        );
        if answer_by_click {
            h.get_by_label("Cancel").click();
        } else {
            h.key_press(Key::Escape);
        }
        h.run_steps(3);
        assert!(!shown(&h, ASK_QUIT));
        assert_eq!(h.state().front(), Front::Adjust, "Escape left Adjust");
        assert!(shown(&h, robco_studio::app::ADJUST_TITLE));
        assert_eq!(h.state().screen.strengths, strengths);
        assert_eq!(h.state().editor.docs.len(), 1);
    }
    // Escape then closes Adjust, the one in front.
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(h.state().front(), Front::Nothing);
}

/// A Screen level from the macOS menu bar, while a quit question covers
/// Adjust with a moved slider, changes nothing; after Cancel, Adjust is
/// back with the slider where it was.
#[test]
fn a_native_screen_level_waits_behind_a_question_over_adjust() {
    let (_d, mut h) = adjusting("adjust-native-level").unwrap();
    bloom(&h).unwrap().set_value_accesskit(0.9);
    h.run_steps(2);
    let moved = h.state().screen.strengths;
    assert!((moved.bloom - 0.9).abs() < 1e-6);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Q);
    h.run_steps(2);
    assert_eq!(h.state().front(), Front::Question);
    for l in robco_studio::theme::Screen::ALL {
        h.state_mut()
            .pending
            .push(robco_studio::menus::Command::Screen(l));
        h.run_steps(2);
        assert_eq!(h.state().front(), Front::Question, "{l:?}");
        assert_eq!(h.state().theme.screen(), robco_studio::theme::Screen::Live);
        assert_eq!(h.state().screen.strengths, moved, "{l:?}");
        assert!(h.state().screen.adjusting, "{l:?}");
    }
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert_eq!(h.state().front(), Front::Adjust);
    assert!(shown(&h, robco_studio::app::ADJUST_TITLE));
    assert_eq!(h.state().screen.strengths, moved);
}

/// While a question is open, the Adjust command does not open Adjust.
#[test]
fn a_question_refuses_the_adjust_command() {
    let (_d, mut h) = adjusting("adjust-refused").unwrap();
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(h.state().front(), Front::Nothing);
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(2);
    assert_eq!(h.state().front(), Front::Question);
    h.state_mut()
        .pending
        .push(robco_studio::menus::Command::ScreenAdjust);
    h.run_steps(3);
    assert!(!h.state().screen.adjusting);
    assert!(!shown(&h, robco_studio::app::ADJUST_TITLE));
    assert!(shown(&h, ASK_CLOSE));
    assert_eq!(h.state().status.1, robco_studio::app::ASKING);
}

/// The folder chooser's hold beats a question and Adjust both: the
/// question is drawn, held, and Adjust waits behind it.
#[test]
fn the_chooser_beats_a_question_and_adjust() {
    let a = pipes("chooser-front").unwrap();
    let (mut h, fake) = with_fake(Some(&*a), None);
    open_main(&mut h);
    h.state_mut().editor.docs[0].text.insert(0, 'x');
    press_open(&mut h);
    assert!(h.state().choosing());
    h.state_mut().screen.adjusting = true;
    assert!(!h.state_mut().editor.request_quit());
    h.run_steps(3);
    assert_eq!(h.state().front(), Front::Chooser);
    assert!(!shown(&h, robco_studio::app::ADJUST_TITLE));
    assert!(shown(&h, ASK_QUIT));
    // Escape goes to the chooser, held: the question stays.
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().editor.question.is_some());
    fake.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .answer = Some(Choice::Cancelled);
    h.run_steps(3);
    assert_eq!(h.state().front(), Front::Question);
}

/// What is in front of the window: how a test brings it and sends it away.
#[derive(Clone, Copy, Debug)]
enum Dialog {
    Adjust,
    Question,
    Chooser,
}

/// The editor's text, caret and selection, and undo history.
fn editor_state(h: &Harness<'static, Studio>) -> Option<(String, String, String)> {
    use robco_studio::editor::Editor;
    let doc = h.state().editor.current()?;
    let st = egui::text_edit::TextEditState::load(&h.ctx, Editor::text_id(doc.id))?;
    Some((
        doc.text.clone(),
        format!("{:?}", st.cursor.char_range()),
        format!("{:?}", st.undoer()),
    ))
}

/// While a dialog is in front, the editor that had focus takes no key,
/// text, paste or IME commit, Tab stays in front, and when the dialog goes
/// the editor has focus again with its caret and selection.
#[test]
fn the_window_behind_a_dialog_takes_no_keys_and_gets_its_focus_back() {
    use egui::text::{CCursor, CCursorRange};
    use robco_studio::editor::{Editor, Question};
    use robco_studio::screen::Adapter;
    use robco_studio::theme::Screen;
    for dialog in [Dialog::Adjust, Dialog::Question, Dialog::Chooser] {
        let d = pipes(&format!("inert-{dialog:?}").to_lowercase()).unwrap();
        let (mut h, fake) = with_fake(Some(&*d), None);
        h.state_mut()
            .choose_screen(Adapter::Hardware, Some(false), Some(Screen::Still));
        open_main(&mut h);
        let doc = h.state().editor.current().unwrap().id;
        let id = Editor::text_id(doc);
        h.ctx.memory_mut(|m| m.request_focus(id));
        h.run_steps(2);
        // An edit, so that closing the tab asks.
        h.event(Event::Text("// ".into()));
        h.run_steps(2);
        let mut st = egui::text_edit::TextEditState::load(&h.ctx, id).unwrap();
        st.cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(5), CCursor::new(9))));
        st.store(&h.ctx, id);
        // Two seconds: egui's undo history takes the new selection as a
        // point of its own after a pause.
        h.run_steps(120);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(id), "{dialog:?}");
        let before = editor_state(&h).unwrap();
        match dialog {
            Dialog::Adjust => h.state_mut().screen.adjusting = true,
            Dialog::Question => h.state_mut().editor.question = Some(Question::CloseTab { doc }),
            Dialog::Chooser => press_open(&mut h),
        }
        h.run_steps(2);
        assert_ne!(h.state().front(), Front::Nothing, "{dialog:?}");
        if matches!(dialog, Dialog::Question) {
            assert!(h.state().editor.question_text().is_some());
        }
        h.event(Event::Text("typed".into()));
        h.event(Event::Paste("pasted".into()));
        h.event(Event::Ime(egui::ImeEvent::Commit("ime".into())));
        h.run_steps(1);
        h.key_press(Key::Backspace);
        // Enter belongs to the question: it presses its focused button.
        if !matches!(dialog, Dialog::Question) {
            h.key_press(Key::Enter);
        }
        h.run_steps(1);
        assert_ne!(h.state().front(), Front::Nothing, "{dialog:?}");
        for _ in 0..4 {
            h.key_press(Key::Tab);
            h.run_steps(1);
            let f = h.ctx.memory(|m| m.focused());
            assert_ne!(f, Some(id), "{dialog:?}: Tab left the dialog");
            if !matches!(dialog, Dialog::Chooser) {
                assert!(f.is_some(), "{dialog:?}: a control of the dialog has focus");
            }
        }
        h.run_steps(2);
        // egui draws a field without focus with no selection; the text
        // and the undo history stay.
        let (text, _, undo) = editor_state(&h).unwrap();
        assert_eq!(
            (text, undo),
            (before.0.clone(), before.2.clone()),
            "{dialog:?}: the document changed"
        );
        match dialog {
            Dialog::Adjust | Dialog::Question => h.key_press(Key::Escape),
            Dialog::Chooser => {
                fake.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .answer = Some(Choice::Cancelled);
            }
        }
        h.run_steps(3);
        assert_eq!(h.state().front(), Front::Nothing, "{dialog:?}");
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(id), "{dialog:?}");
        assert_eq!(editor_state(&h).unwrap(), before, "{dialog:?}");
        h.event(Event::Text("Z".into()));
        h.run_steps(2);
        let text = &h.state().editor.current().unwrap().text;
        assert_ne!(text, &before.0, "{dialog:?}: typing works again");
        assert_eq!(text.chars().nth(5), Some('Z'), "{dialog:?}");
    }
}

/// The arrow keys move the focused slider of Adjust.
#[test]
fn the_arrow_keys_move_a_slider_of_adjust() {
    use robco_studio::screen::Adapter;
    use robco_studio::theme::Screen;
    let mut h = harness(None);
    h.state_mut()
        .choose_screen(Adapter::Hardware, Some(false), Some(Screen::Still));
    h.state_mut().screen.adjusting = true;
    h.run_steps(3);
    let before = h.state().screen.strengths;
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert_ne!(h.state().screen.strengths, before);
}
