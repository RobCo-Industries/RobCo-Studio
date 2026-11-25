use gpui::{Modifiers, TestAppContext};
use k4_gpui::{Spike, Tab};

#[gpui::test]
fn press_buttons_change_state(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Spike::default());

    let b = cx.debug_bounds("zoom-in").expect("zoom-in button");
    cx.simulate_click(b.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |s, _| s.zoom), 24.0);

    let b = cx.debug_bounds("tab-log").expect("log tab");
    cx.simulate_click(b.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |s, _| s.tab), Tab::Log);
}
