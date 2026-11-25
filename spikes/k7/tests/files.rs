use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use k7_editor::Editor;
use std::path::PathBuf;

fn temp(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("k7-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

/// Open through the Open button, type at the end, save through the Save button.
fn open_edit_save(name: &str, bytes: &[u8], typed: &str) -> (Vec<u8>, Editor) {
    let p = temp(name, bytes);
    let mut ed = Editor::new(String::new());
    ed.path = p.display().to_string();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(|ui, e: &mut Editor| e.ui(ui), ed);
    h.get_by_label("Open").click();
    h.run();
    assert!(!h.state().is_dirty());
    assert!(h.query_by_label("saved").is_some());
    if !typed.is_empty() {
        let end = h.state().text.chars().count();
        h.state_mut().pending_sel = Some((end, end));
        h.run();
        h.event(egui::Event::Text(typed.into()));
        h.run();
        assert!(h.state().is_dirty());
        assert!(h.query_by_label("modified").is_some());
    }
    h.get_by_label("Save").click();
    h.run();
    assert!(!h.state().is_dirty(), "{}", h.state().message);
    let out = std::fs::read(&p).unwrap();
    std::fs::remove_file(&p).unwrap();
    let mut ed = Editor::new(String::new());
    std::mem::swap(&mut ed, h.state_mut());
    (out, ed)
}

#[test]
fn lf_round_trip_exact() {
    let src = b"int main(void)\n{\n\treturn 0;\n}\n";
    let (out, ed) = open_edit_save("lf.c", src, "");
    assert_eq!(out, src);
    assert!(!ed.crlf);
}

#[test]
fn crlf_round_trip_exact() {
    let src = b"int main(void)\r\n{\r\n\treturn 0;\r\n}\r\n";
    let (out, ed) = open_edit_save("crlf.c", src, "");
    assert_eq!(out, src);
    assert!(ed.crlf);
    assert!(!ed.text.contains('\r'));
}

#[test]
fn crlf_edit_keeps_crlf() {
    let src = b"a;\r\nb;\r\n";
    let (out, _) = open_edit_save("crlf2.c", src, "\tc;");
    assert_eq!(out, b"a;\r\nb;\r\n\tc;");
}

#[test]
fn tabs_kept() {
    let src = b"\t\tx = 1;\t// tab\n";
    let (out, _) = open_edit_save("tabs.c", src, "\t");
    assert_eq!(out, b"\t\tx = 1;\t// tab\n\t");
}

#[test]
fn invalid_utf8_refused() {
    let p = temp("bad.c", b"int a; /* \xff\xfe */\n");
    let mut ed = Editor::new("kept\n".into());
    ed.open(&p);
    assert_eq!(ed.text, "kept\n", "text not replaced");
    assert!(ed.message.contains("not valid UTF-8"), "{}", ed.message);
    assert!(ed.message.contains("offset 10"), "{}", ed.message);
    // Nothing was written back.
    assert_eq!(std::fs::read(&p).unwrap(), b"int a; /* \xff\xfe */\n");
    std::fs::remove_file(&p).unwrap();
}

#[test]
fn mixed_line_endings_refused() {
    let p = temp("mixed.c", b"a\r\nb\n");
    let mut ed = Editor::new("kept\n".into());
    ed.open(&p);
    assert_eq!(ed.text, "kept\n");
    assert!(ed.message.contains("mixes line endings"), "{}", ed.message);
    std::fs::remove_file(&p).unwrap();
}

fn ui_harness(ed: Editor) -> Harness<'static, Editor> {
    Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(|ui, e: &mut Editor| e.ui(ui), ed)
}

/// Open `a`, type at the end of it, then ask to open `b`.
fn dirty_then_open(a: &std::path::Path, b: &std::path::Path) -> Harness<'static, Editor> {
    let mut ed = Editor::new(String::new());
    ed.path = a.display().to_string();
    let mut h = ui_harness(ed);
    h.get_by_label("Open").click();
    h.run();
    let end = h.state().text.chars().count();
    h.state_mut().pending_sel = Some((end, end));
    h.run();
    h.event(egui::Event::Text("typed".into()));
    h.run();
    assert!(h.state().is_dirty());
    h.state_mut().path = b.display().to_string();
    h.get_by_label("Open").click();
    h.run();
    h
}

#[test]
fn open_over_edits_asks_and_cancel_keeps_them() {
    let a = temp("ask-a.c", b"aaa\n");
    let b = temp("ask-b.c", b"bbb\n");
    let mut h = dirty_then_open(&a, &b);
    assert_eq!(
        h.state().text,
        "aaa\ntyped",
        "not replaced before an answer"
    );
    h.get_by_label("Cancel").click();
    h.run();
    assert_eq!(h.state().text, "aaa\ntyped");
    assert!(h.state().confirm.is_none());
    // Save still goes to A, not to the path typed in the field.
    h.get_by_label("Save").click();
    h.run();
    assert_eq!(std::fs::read(&a).unwrap(), b"aaa\ntyped");
    assert_eq!(std::fs::read(&b).unwrap(), b"bbb\n");
}

#[test]
fn open_over_edits_save_first() {
    let a = temp("save-a.c", b"aaa\n");
    let b = temp("save-b.c", b"bbb\n");
    let mut h = dirty_then_open(&a, &b);
    h.get_by_label("Save first").click();
    h.run();
    assert_eq!(std::fs::read(&a).unwrap(), b"aaa\ntyped");
    assert_eq!(h.state().text, "bbb\n");
    assert!(!h.state().is_dirty());
}

#[test]
fn undo_after_discard_and_open_keeps_new_file() {
    let a = temp("undo-a.c", b"aaa\n");
    let b = temp("undo-b.c", b"bbb\n");
    let mut h = dirty_then_open(&a, &b);
    h.get_by_label("Discard edits").click();
    h.run();
    assert_eq!(h.state().text, "bbb\n");
    assert_eq!(std::fs::read(&a).unwrap(), b"aaa\n", "A untouched");
    h.state_mut().pending_sel = Some((0, 0));
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    assert_eq!(h.state().text, "bbb\n", "undo did not bring back A's text");
    assert!(!h.state().is_dirty());
}

#[test]
fn close_with_edits_is_cancelled_until_discard() {
    use egui::{ViewportCommand, ViewportEvent, ViewportId};
    let mut h = ui_harness(Editor::new("x\n".into()));
    h.state_mut().pending_sel = Some((0, 0));
    h.run();
    h.event(egui::Event::Text("y".into()));
    h.run();
    h.input_mut()
        .viewports
        .entry(ViewportId::ROOT)
        .or_default()
        .events
        .push(ViewportEvent::Close);
    h.step();
    let cmds = |h: &Harness<'static, Editor>| {
        h.output()
            .viewport_output
            .get(&ViewportId::ROOT)
            .map(|v| v.commands.clone())
            .unwrap_or_default()
    };
    assert!(
        cmds(&h).contains(&ViewportCommand::CancelClose),
        "{:?}",
        cmds(&h)
    );
    assert_eq!(h.state().confirm, Some(k7_editor::Replace::Close));
    h.get_by_label("Discard edits").click();
    h.step();
    h.step();
    assert!(cmds(&h).contains(&ViewportCommand::Close), "{:?}", cmds(&h));
}
