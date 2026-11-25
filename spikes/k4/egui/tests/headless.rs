use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use k4_egui::{State, Tab};

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
