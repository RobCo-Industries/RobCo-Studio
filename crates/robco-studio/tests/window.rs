// SPDX-License-Identifier: GPL-3.0-or-later

//! The window, headless, through the accessibility tree.

use eframe::egui::{self, Event, Key, Modifiers, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use robco_studio::Studio;
use robco_studio::app::{Page, Place};
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
    // falls back, and Larger says why.
    h.state_mut()
        .run(&egui::Context::default(), Command::SimZoom(2));
    h.run_steps(2);
    assert_eq!(h.state().sim_zoom, 1);
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
    use robco_studio::menus::{Command, NO_DIALOG, menus};
    let file = menus().into_iter().find(|m| m.title == "File").unwrap();
    let open = file
        .items
        .iter()
        .find(|i| i.command == Command::OpenFolder)
        .unwrap();
    assert_eq!(open.text, "Open folder");
    assert_eq!(open.unavailable, Some(NO_DIALOG));
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
