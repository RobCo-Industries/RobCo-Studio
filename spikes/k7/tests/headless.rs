use eframe::egui::{self, Event, Key, Modifiers, OutputCommand};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use k7_editor::{Editor, sample_c};

fn harness(text: String) -> Harness<'static, Editor> {
    Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(|ui, e: &mut Editor| e.ui(ui), Editor::new(text))
}

/// Put the cursor at character `c` of the main text and focus it.
fn cursor_at(h: &mut Harness<'static, Editor>, c: usize) {
    h.state_mut().pending_sel = Some((c, c));
    h.run();
}

#[test]
fn type_and_read_back() {
    let mut h = harness("int a;\n".into());
    cursor_at(&mut h, 6);
    h.event(Event::Text(" int b;".into()));
    h.run();
    assert_eq!(h.state().text, "int a; int b;\n");
    assert!(h.state().is_dirty());
}

#[test]
fn undo_and_redo() {
    let mut h = harness("abc\n".into());
    cursor_at(&mut h, 3);
    h.event(Event::Text("XYZ".into()));
    h.run();
    assert_eq!(h.state().text, "abcXYZ\n");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().text, "abc\n");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run();
    assert_eq!(h.state().text, "abcXYZ\n");
}

#[test]
fn copy_and_paste() {
    let mut h = harness("alpha beta\n".into());
    h.state_mut().pending_sel = Some((0, 5));
    h.run();
    h.event(Event::Copy);
    h.step();
    let copied = h
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, OutputCommand::CopyText(t) if t == "alpha"));
    assert!(copied, "commands {:?}", h.output().platform_output.commands);
    cursor_at(&mut h, 10);
    h.event(Event::Paste(" alpha".into()));
    h.run();
    assert_eq!(h.state().text, "alpha beta alpha\n");
}

#[test]
fn search_finds_and_moves() {
    let mut h = harness("one two\none two\none\n".into());
    h.state_mut().query = "two".into();
    h.get_by_label("Next").click();
    h.run();
    let first = h.state().cursor.expect("a selection");
    assert_eq!(first.as_sorted_char_range().start.0, 4);
    h.get_by_label("Next").click();
    h.run();
    assert_eq!(h.state().cursor.unwrap().as_sorted_char_range().start.0, 12);
    h.get_by_label("Next").click();
    h.run();
    assert_eq!(
        h.state().cursor.unwrap().as_sorted_char_range().start.0,
        4,
        "wraps"
    );
    h.get_by_label("Previous").click();
    h.run();
    assert_eq!(
        h.state().cursor.unwrap().as_sorted_char_range().start.0,
        12,
        "wraps back"
    );
    h.state_mut().query = "nine".into();
    h.get_by_label("Next").click();
    h.run();
    assert_eq!(h.state().message, "Not found.");
}

#[test]
fn search_scrolls_to_a_far_match() {
    let mut text = sample_c(5000);
    text.push_str("needle\n");
    let mut h = harness(text);
    h.state_mut().check_lines = true;
    h.run();
    assert_eq!(h.state().first_gutter_line, 1);
    h.state_mut().query = "needle".into();
    h.get_by_label("Next").click();
    h.run();
    let s = h.state();
    assert!(
        s.first_gutter_line > 4900,
        "first line {}",
        s.first_gutter_line
    );
    assert_eq!(s.first_gutter_line, s.first_text_line);
}

#[test]
fn go_to_line() {
    let mut h = harness(sample_c(5000));
    h.state_mut().check_lines = true;
    h.state_mut().goto = "2500".into();
    h.get_by_label("Go").click();
    h.run();
    let s = h.state();
    let c = s.cursor.unwrap().as_sorted_char_range().start.0;
    let line = s.text.chars().take(c).filter(|&ch| ch == '\n').count() + 1;
    assert_eq!(line, 2500);
    assert!(
        s.first_gutter_line < 2500 && s.first_gutter_line > 2450,
        "first line {}",
        s.first_gutter_line
    );
}

#[test]
fn gutter_in_step_after_scroll() {
    let mut h = harness(sample_c(5000));
    h.state_mut().check_lines = true;
    h.run();
    assert_eq!(h.state().first_gutter_line, 1);
    assert_eq!(h.state().first_text_line, 1);
    for line in [1234usize, 4000, 77] {
        h.state_mut().scroll_to_line = Some(line);
        h.step();
        let s = h.state();
        assert_eq!(s.first_gutter_line, line + 1, "after scroll to {line}");
        assert_eq!(s.first_text_line, s.first_gutter_line);
    }
}

#[test]
fn tab_key_inserts_a_tab() {
    let mut h = harness("a\n".into());
    cursor_at(&mut h, 1);
    h.key_press(Key::Tab);
    h.run();
    assert_eq!(h.state().text, "a\t\n");
}
