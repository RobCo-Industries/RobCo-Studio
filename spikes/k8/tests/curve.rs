//! A click on the curved screen, at the place where a button is drawn,
//! lands on that button once the pointer is mapped. No GPU.

use eframe::egui::{self, Event, PointerButton, Pos2, Rect, vec2};
use k8_crt::{drawn_at, map_input, stand_in, warp};

fn run(ctx: &egui::Context, events: Vec<Event>, curve: f32) -> k8_crt::Drawn {
    let mut raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0))),
        events,
        ..Default::default()
    };
    map_input(&mut raw, curve);
    let mut d = None;
    let _ = ctx.run_ui(raw, |ui| d = Some(stand_in(ui, 0.0)));
    d.expect("drawn")
}

fn click(ctx: &egui::Context, at: Pos2, curve: f32) -> bool {
    run(ctx, vec![Event::PointerMoved(at)], curve);
    let b = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    run(ctx, vec![b(true)], curve);
    run(ctx, vec![b(false)], curve).clicked
}

#[test]
fn warp_is_identity_when_flat() {
    for uv in [[0.0, 0.0], [0.3, 0.7], [1.0, 1.0]] {
        assert_eq!(warp(uv, 0.0), uv);
    }
}

#[test]
fn click_on_curved_button_lands() {
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0));
    for curve in [0.15, 1.0] {
        let ctx = egui::Context::default();
        run(&ctx, vec![], curve);
        let button = run(&ctx, vec![], curve).button;
        // The bottom edge of the button, which the curve moves toward the centre.
        let target = Pos2::new(button.center().x, button.max.y - 1.5);
        let drawn = drawn_at(target, screen, curve);
        assert!((drawn - target).length() > 0.1, "the curve moved nothing");
        assert!(
            click(&ctx, drawn, curve),
            "mapped click missed at {curve}: button {button:?}, drawn {drawn:?}"
        );
        // Without the mapping, the same glass position misses at strong curve.
        if curve == 1.0 {
            // egui grows a hit area by `interact_radius`, 5 points by default,
            // which hides a small miss. Zero it to see the raw miss.
            let ctx = egui::Context::default();
            ctx.all_styles_mut(|s| s.interaction.interact_radius = 0.0);
            run(&ctx, vec![], 0.0);
            run(&ctx, vec![], 0.0);
            assert!(!click(&ctx, drawn, 0.0), "unmapped click should miss");
        }
    }
}
