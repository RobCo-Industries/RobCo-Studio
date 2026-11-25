use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use k4_egui::{LOG_LINES, State, Tab};

#[test]
fn press_buttons_change_state() {
    let mut harness = Harness::new_ui_state(|ui, s: &mut State| s.ui(ui), State::default());
    harness.get_by_label("Zoom in").click();
    harness.run();
    assert_eq!(harness.state().zoom, 24.0);

    harness.get_by_label("Log").click();
    harness.run();
    assert_eq!(harness.state().tab, Tab::Log);
}

#[test]
fn log_scrolls_and_jumps_to_end() {
    let mut state = State::default();
    state.tab = Tab::Log;
    let mut harness = Harness::new_ui_state(|ui, s: &mut State| s.ui(ui), state);
    harness.run();
    let top = harness.state().visible_rows.clone();
    assert_eq!(top.start, 0);

    // A set offset starts egui's scroll-bar fade, which keeps repainting, so step
    // frame by frame instead of waiting for the UI to settle.
    harness.state_mut().scroll_to_row = Some(50_000);
    harness.step();
    let mid = harness.state().visible_rows.clone();
    assert!(mid.start >= 49_990 && mid.start <= 50_000, "rows {mid:?}");

    harness.get_by_label("Jump to end").click();
    harness.step();
    harness.step();
    let end = harness.state().visible_rows.clone();
    assert_eq!(end.end, LOG_LINES, "rows {end:?}");
    assert!(end.start > mid.start);
}
