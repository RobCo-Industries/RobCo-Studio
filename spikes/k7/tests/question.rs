//! The pending question is modal and carries its target and its document.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use k7_editor::{Answer, Editor, Question, Replace};
use std::path::{Path, PathBuf};

fn temp(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("k7-question-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

fn harness_on(a: &Path) -> Harness<'static, Editor> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(|ui, e: &mut Editor| e.ui(ui), Editor::new(String::new()));
    h.state_mut().open(a);
    h.run();
    // Give the text field focus, so typed events would reach it.
    h.state_mut().doc.pending_sel = Some((0, 0));
    h.run();
    h
}

/// Open `a`, then "Save as" onto the existing `target`: the question is pending.
fn overwrite_pending(a: &Path, target: &Path) -> Harness<'static, Editor> {
    let mut h = harness_on(a);
    h.state_mut().save_as_path = target.display().to_string();
    h.get_by_label("Save as").click();
    h.run();
    assert!(matches!(
        h.state().question,
        Some(Question::Overwrite { .. })
    ));
    h
}

/// Open `a`, type, then ask to open `b`: the unsaved question is pending.
fn unsaved_pending(a: &Path, b: &Path) -> Harness<'static, Editor> {
    let mut h = harness_on(a);
    h.event(egui::Event::Text("typed".into()));
    h.run();
    assert!(h.state().is_dirty());
    h.state_mut().path = b.display().to_string();
    h.get_by_label("Open").click();
    h.run();
    assert!(matches!(h.state().question, Some(Question::Unsaved { .. })));
    h
}

#[test]
fn open_refused_while_overwrite_pending() {
    let a = temp("ow-a.c", b"aaa\n");
    let t = temp("ow-t.c", b"ttt\n");
    let b = temp("ow-b.c", b"bbb\n");
    let mut h = overwrite_pending(&a, &t);
    let q = h.state().question.clone();
    // The view's Open is disabled, and the model refuses it too.
    h.state_mut().path = b.display().to_string();
    h.get_by_label("Open").click();
    h.run();
    h.state_mut().open(&b);
    h.run();
    assert_eq!(h.state().doc.text, "aaa\n", "B not loaded");
    assert_eq!(h.state().question, q);
    assert_eq!(std::fs::read(&t).unwrap(), b"ttt\n");
    // The answer acts on the document it was asked about, never on B.
    h.get_by_label("Keep the file").click();
    h.run();
    assert_eq!(std::fs::read(&t).unwrap(), b"ttt\n");
    assert!(h.state().question.is_none());
}

#[test]
fn open_refused_while_open_question_pending() {
    let a = temp("oo-a.c", b"aaa\n");
    let b = temp("oo-b.c", b"bbb\n");
    let c = temp("oo-c.c", b"ccc\n");
    let mut h = unsaved_pending(&a, &b);
    h.state_mut().open(&c);
    h.run();
    let id = h.state().doc.id;
    assert_eq!(
        h.state().question,
        Some(Question::Unsaved {
            doc: id,
            then: Replace::Open(b.clone())
        })
    );
    assert_eq!(h.state().doc.text, "typedaaa\n");
    h.get_by_label("Discard edits").click();
    h.run();
    assert_eq!(h.state().doc.text, "bbb\n", "the question's own target");
    assert_eq!(std::fs::read(&a).unwrap(), b"aaa\n");
    assert_eq!(std::fs::read(&c).unwrap(), b"ccc\n");
}

#[test]
fn stale_answer_is_dropped() {
    let a = temp("st-a.c", b"aaa\n");
    let t = temp("st-t.c", b"ttt\n");
    let mut h = overwrite_pending(&a, &t);
    let id = h.state().doc.id;
    let q = h.state().question.clone();
    for stale in [id.wrapping_sub(1), id + 1] {
        h.state_mut().answer(stale, Answer::Overwrite);
        h.run();
        assert_eq!(std::fs::read(&t).unwrap(), b"ttt\n");
        assert_eq!(h.state().question, q);
    }
    // An answer of the wrong kind changes nothing either.
    h.state_mut().answer(id, Answer::Discard);
    assert_eq!(h.state().question, q);
    h.state_mut().answer(id, Answer::Overwrite);
    assert_eq!(std::fs::read(&t).unwrap(), b"aaa\n");
}

#[test]
fn save_refused_while_pending() {
    let a = temp("sv-a.c", b"aaa\n");
    let b = temp("sv-b.c", b"bbb\n");
    let mut h = unsaved_pending(&a, &b);
    h.get_by_label("Save").click();
    h.run();
    h.state_mut().save();
    h.run();
    assert_eq!(std::fs::read(&a).unwrap(), b"aaa\n");
    assert!(h.state().question.is_some());
}

#[test]
fn save_as_refused_while_pending() {
    let a = temp("sa-a.c", b"aaa\n");
    let b = temp("sa-b.c", b"bbb\n");
    let n = temp("sa-n.c", b"");
    std::fs::remove_file(&n).unwrap();
    let mut h = unsaved_pending(&a, &b);
    let q = h.state().question.clone();
    h.state_mut().save_as(&n);
    h.run();
    assert!(!n.exists());
    assert_eq!(h.state().question, q);
}

#[test]
fn close_refused_while_overwrite_pending() {
    let a = temp("cl-a.c", b"aaa\n");
    let t = temp("cl-t.c", b"ttt\n");
    let mut h = overwrite_pending(&a, &t);
    let q = h.state().question.clone();
    assert!(
        !h.state_mut().close_requested(),
        "a clean buffer, still asked"
    );
    assert_eq!(h.state().question, q);
}

#[test]
fn typing_locked_while_pending() {
    let a = temp("ty-a.c", b"aaa\n");
    let t = temp("ty-t.c", b"ttt\n");
    let mut h = overwrite_pending(&a, &t);
    // The Save as click took focus; ask for it back. Without the lock the text field takes it.
    let id = egui::Id::unique((k7_editor::TEXT_ID, h.state().doc.id));
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.run();
    h.event(egui::Event::Text("zz".into()));
    h.run();
    h.event(egui::Event::Paste("pp".into()));
    h.run();
    assert_eq!(h.state().doc.text, "aaa\n");
    h.get_by_label("Overwrite").click();
    h.run();
    assert_eq!(std::fs::read(&t).unwrap(), b"aaa\n");
}

#[test]
fn search_and_go_to_line_harmless_while_pending() {
    let a = temp("se-a.c", b"aaa\nbbb\n");
    let t = temp("se-t.c", b"ttt\n");
    let mut h = overwrite_pending(&a, &t);
    let q = h.state().question.clone();
    h.state_mut().query = "bbb".into();
    h.state_mut().find_next();
    h.state_mut().goto_line(2);
    h.run();
    assert_eq!(h.state().doc.text, "aaa\nbbb\n");
    assert_eq!(h.state().question, q);
    assert_eq!(std::fs::read(&t).unwrap(), b"ttt\n");
    // The selection waits for the answer, then applies.
    h.get_by_label("Keep the file").click();
    h.run();
    let c = h.state().doc.cursor.unwrap().as_sorted_char_range().start.0;
    assert_eq!(c, 4);
}
