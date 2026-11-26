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
        "Rail",
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

#[test]
fn the_four_jobs_cannot_act() {
    let d = sample("jobs").unwrap();
    let mut h = harness(Some(&*d));
    for job in ["Build", "Run", "Debug", "Write card"] {
        let buttons: Vec<_> = h
            .query_all_by_label(job)
            .filter(|n| n.accesskit_node().role() == Role::Button)
            .collect();
        assert!(!buttons.is_empty(), "{job}");
        for b in buttons {
            assert!(b.accesskit_node().is_disabled(), "{job} looks able to act");
            assert!(
                b.accesskit_node()
                    .description()
                    .is_some_and(|s| s.contains("Nothing") || s.contains("No ")),
                "{job} says why"
            );
        }
    }
    let before = h.state().status.clone();
    for b in h
        .query_all_by_label("Run")
        .filter(|n| n.accesskit_node().role() == Role::Button)
    {
        b.click();
    }
    h.run_steps(2);
    assert_eq!(h.state().status, before, "a click changed nothing");
}

#[test]
fn the_rail_switches_pages() {
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
    h.key_press_modifiers(Modifiers::COMMAND, Key::Num1);
    h.run_steps(2);
    assert_eq!(h.state().page, Page::Project);
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
        Toolbar, Rail, Files, Editor, Simulator, Tape, Bottom, Status, Toolbar,
    ];
    assert_eq!(seen, want.map(Some).to_vec());
    h.key_press_modifiers(Modifiers::SHIFT, Key::F6);
    h.run_steps(2);
    assert_eq!(h.state().focused_place(&h.ctx), Some(Status));
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

/// At the smallest window the find bar lies inside the editor pane, between
/// the Files pane and the Simulator.
#[test]
fn the_find_bar_fits_the_editor() {
    let d = sample("findfit").unwrap();
    let mut h = harness(Some(&*d));
    open_main(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    h.state_mut().editor.find.query = "return".into();
    h.state_mut().editor.find.replace = "goto".into();
    h.run_steps(3);
    let editor = h.state().place_rect(Place::Editor).unwrap();
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
        for n in h.query_all_by_label(label) {
            let r = n.rect();
            if r.top() > 200.0 {
                continue; // not the find bar
            }
            assert!(
                r.left() > left && r.right() < right && r.width() > 0.0,
                "{label} {r:?} is not inside {left}..{right}"
            );
        }
    }
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

#[test]
fn the_toolbar_has_four_actions_of_one_width() {
    use robco_studio::theme::size;
    let h = harness(None);
    for job in ["Build", "Run", "Debug", "Write card"] {
        assert!(
            h.query_all_by_label(job).any(|n| {
                n.accesskit_node().role() == Role::Button
                    && n.rect().width() == size::TOOLBAR_ACTION_W
                    && n.rect().height() == size::CONTROL_H
            }),
            "{job}"
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
