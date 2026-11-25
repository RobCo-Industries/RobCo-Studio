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
