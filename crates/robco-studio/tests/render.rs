// SPDX-License-Identifier: GPL-3.0-or-later

//! Pictures of the window, rendered with no display.
//!
//! Ignored by default: run with `STUDIO_SHOTS=<folder> cargo test -p
//! robco-studio --test render -- --ignored`. It needs a GPU or a software
//! adapter that wgpu can find. The pictures are not compared with anything:
//! they differ between machines.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use robco_studio::Studio;
use robco_studio::app::Page;
use robco_studio::theme::Scheme;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "format = 1\n\n[project]\nname = \"PIPES\"\ntitle = \"Pipes\"\nversion = \"1.0.0\"\nkind = \"holotape\"\nsubtype = \"game\"\n\n[target]\nplatform = \"pipos\"\nabi = \"1.3\"\n\n[build]\nbackend = \"robco\"\nsources = [\"src/*.c\"]\n";

const MAIN_C: &str = r#"/* Pipes: turn the tiles until the water flows. */
#include <pipos/pip.h>
#include "game.h"

#define LEVELS 12

static game_t game;

int pip_main(const pip_startup_t *startup)
{
    (void)startup;
    game_init(&game, 1);
    pip_printf("pipes: title\n");
    while (!game.quit) {
        pip_event_t ev;
        int32_t rc = pip_wait_event(&ev, PIP_WAIT_FOREVER);
        if (rc < 0)
            break;
        game_on_event(&game, &ev);
        if (game.level > LEVELS)
            game.quit = true;
    }
    return 0;
}
"#;

fn sample() -> std::io::Result<PathBuf> {
    let d = std::env::temp_dir().join(format!("robco-studio-shots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src"))?;
    std::fs::create_dir_all(d.join("assets"))?;
    std::fs::write(d.join("robco.toml"), MANIFEST)?;
    std::fs::write(d.join("src/main.c"), MAIN_C)?;
    std::fs::write(d.join("src/game.c"), "#include \"game.h\"\n")?;
    std::fs::write(d.join("src/game.h"), "#pragma once\n")?;
    std::fs::write(d.join("assets/strings.toml"), "title = \"Pipes\"\n")?;
    Ok(d)
}

fn shot(h: &mut Harness<'static, Studio>, dir: &Path, name: &str) -> Result<(), String> {
    h.run_steps(4);
    let img = h.render()?;
    img.save(dir.join(name)).map_err(|e| e.to_string())
}

fn harness(folder: Option<&Path>) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_ui_state(|ui, s: &mut Studio| s.ui(ui), Studio::new(folder));
    h.run_steps(3);
    h
}

#[test]
#[ignore = "writes PNG files; needs a wgpu adapter"]
fn render_the_window() {
    let Some(dir) = std::env::var_os("STUDIO_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let d = sample().unwrap();
    let mut h = harness(Some(&d));
    h.get_by_label("src").click();
    h.run_steps(2);
    h.get_by_label("main.c").click();
    h.run_steps(2);
    shot(&mut h, &dir, "project-green.png").unwrap();
    h.state_mut().theme.scheme = Scheme::Amber;
    shot(&mut h, &dir, "project-amber.png").unwrap();
    h.state_mut().theme.scheme = Scheme::Blue;
    shot(&mut h, &dir, "project-blue.png").unwrap();
    h.state_mut().theme.scheme = Scheme::White;
    shot(&mut h, &dir, "project-white.png").unwrap();
    h.state_mut().theme.scheme = Scheme::Green;

    // The narrowest editor with the find bar open.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    h.state_mut().editor.find.query = "game".into();
    h.state_mut().layout.sim_w = 10_000.0;
    shot(&mut h, &dir, "find-narrow-green.png").unwrap();
    h.key_press(egui::Key::Escape);
    h.state_mut().editor.find.open = false;
    h.state_mut().layout.sim_w = 0.0;

    // Scrolled right and down: the gutter stays, the numbers follow.
    let mut long = "/* ".to_owned() + &"wide ".repeat(80) + "*/\n";
    long.push_str(MAIN_C);
    long.push_str(MAIN_C);
    std::fs::write(d.join("src/game.c"), long).unwrap();
    h.get_by_label("game.c").click();
    h.run_steps(2);
    let editor = h
        .state()
        .place_rect(robco_studio::app::Place::Editor)
        .unwrap();
    h.hover_at(editor.center());
    for (delta, modifiers) in [
        (egui::vec2(0.0, -300.0), egui::Modifiers::SHIFT),
        (egui::vec2(0.0, -200.0), egui::Modifiers::NONE),
    ] {
        h.event_modifiers(
            egui::Event::MouseWheel {
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
    shot(&mut h, &dir, "gutter-scrolled-green.png").unwrap();

    let mut h = harness(None);
    shot(&mut h, &dir, "no-project-green.png").unwrap();
    h.state_mut().page = Page::Sdk;
    shot(&mut h, &dir, "sdk-empty-green.png").unwrap();
    std::fs::remove_dir_all(&d).unwrap();
}

/// The find fields show their text: inside each field's rectangle some
/// pixels are near the text colour, not only the fill and the border.
#[test]
#[ignore = "needs a wgpu adapter"]
fn find_fields_show_their_text() {
    let d = sample().unwrap();
    struct Gone(PathBuf);
    impl Drop for Gone {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _gone = Gone(d.clone());
    let mut h = harness(Some(&d));
    h.get_by_label("src").click();
    h.run_steps(2);
    h.get_by_label("main.c").click();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    h.run_steps(2);
    h.state_mut().editor.find.query = "return".into();
    h.state_mut().editor.find.replace = "goto".into();
    h.run_steps(4);
    let img = h.render().unwrap();
    let c = h.state().theme.palette().text;
    let text = [c.r(), c.g(), c.b()];
    for label in ["Find", "Replace with"] {
        let r = h
            .query_all_by_label(label)
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::TextInput)
            .unwrap()
            .rect();
        // Inside the border, where the text is.
        let inner = r.shrink(3.0);
        let mut hits = 0;
        for y in inner.top() as u32..inner.bottom() as u32 {
            for x in inner.left() as u32..inner.right() as u32 {
                let px = img.get_pixel(x, y).0;
                let near = (0..3).all(|i| px[i].abs_diff(text[i]) < 24);
                hits += usize::from(near);
            }
        }
        assert!(
            hits > 10,
            "{label}: {hits} text-coloured pixels in {inner:?}"
        );
    }
}
