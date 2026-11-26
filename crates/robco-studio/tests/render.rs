// SPDX-License-Identifier: GPL-3.0-or-later

//! Pictures of the window, rendered with no display.
//!
//! Ignored by default: run with `STUDIO_SHOTS=<folder> cargo test -p
//! robco-studio --test render -- --ignored`. Point it at a folder of its
//! own: it writes `window-*.png` and `sheet-green.png` there. It needs a GPU
//! or a software adapter that wgpu can find. The pictures are not compared with anything:
//! they differ between machines.

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use robco_studio::Studio;
use robco_studio::app::Page;
use robco_studio::art::{Art, Icon, Mark};
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

/// A sample project in a folder of its own, removed when the test ends,
/// pass or fail.
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

fn sample(tag: &str) -> std::io::Result<Sample> {
    let d = Sample(
        std::env::temp_dir().join(format!("robco-studio-shots-{tag}-{}", std::process::id())),
    );
    let _ = std::fs::remove_dir_all(&*d);
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
    harness_at(folder, 720.0)
}

fn harness_at(folder: Option<&Path>, height: f32) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, height))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_ui_state(full_window, Studio::new(folder));
    h.run_steps(3);
    h
}

/// Draw the window over the whole screen, as eframe gives it, and not
/// inside the harness's margin.
fn full_window(ui: &mut egui::Ui, s: &mut Studio) {
    let r = ui.ctx().content_rect();
    ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
        ui.set_clip_rect(r);
        s.ui(ui);
    });
}

fn open_main(h: &mut Harness<'static, Studio>) {
    h.get_by_label("src").click();
    h.run_steps(2);
    h.get_by_label("main.c").click();
    h.run_steps(2);
}

/// The Project page with the sample project in each scheme, the Card
/// page, the compact header, the find bar, and two empty pages.
#[test]
#[ignore = "writes PNG files; needs a wgpu adapter"]
fn render_the_window() {
    let Some(dir) = std::env::var_os("STUDIO_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let d = sample("window").unwrap();
    let mut h = harness(Some(&d));
    open_main(&mut h);
    shot(&mut h, &dir, "window-green.png").unwrap();
    h.state_mut().page = Page::Card;
    shot(&mut h, &dir, "window-card.png").unwrap();
    h.state_mut().page = Page::Project;
    h.state_mut().theme.scheme = Scheme::Amber;
    shot(&mut h, &dir, "window-amber.png").unwrap();
    h.state_mut().theme.scheme = Scheme::Blue;
    shot(&mut h, &dir, "window-blue.png").unwrap();
    h.state_mut().theme.scheme = Scheme::White;
    shot(&mut h, &dir, "window-white.png").unwrap();
    h.state_mut().theme.scheme = Scheme::Green;
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    h.run_steps(2);
    h.state_mut().editor.find.query = "game".into();
    h.state_mut().bottom_tab = robco_studio::app::BottomTab::Log;
    shot(&mut h, &dir, "window-find.png").unwrap();

    // The narrowest editor with the find bar open.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    h.state_mut().editor.find.query = "game".into();
    h.state_mut().layout.sim_w = 10_000.0;
    shot(&mut h, &dir, "window-find-narrow.png").unwrap();
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
    shot(&mut h, &dir, "window-gutter.png").unwrap();
    let mut h = harness_at(Some(&d), 700.0);
    open_main(&mut h);
    shot(&mut h, &dir, "window-compact.png").unwrap();

    let mut h = harness(None);
    shot(&mut h, &dir, "window-no-project.png").unwrap();
    h.state_mut().page = Page::Sdk;
    shot(&mut h, &dir, "window-sdk.png").unwrap();
}

/// The find fields show their text: inside each field's rectangle some
/// pixels are near the text colour, not only the fill and the border.
#[test]
#[ignore = "needs a wgpu adapter"]
fn find_fields_show_their_text() {
    let d = sample("find").unwrap();
    let mut h = harness(Some(&d));
    open_main(&mut h);
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

/// Every drawing, every icon and the label styles on the green ground,
/// tinted `bright`. The left column has effects on, the right one off.
fn paint_sheet(ui: &mut egui::Ui, arts: &mut Option<[Art; 2]>) {
    use robco_studio::theme::{Effects, Theme, effect, text};
    let ctx = ui.ctx().clone();
    let soft = Theme::default();
    let off = Theme {
        effects: Effects::Off,
        ..soft
    };
    let [on_art, off_art] =
        arts.get_or_insert_with(|| [Art::new(&ctx, &soft), Art::new(&ctx, &off)]);
    let p = soft.palette();
    let painter = ui.painter().clone();
    painter.rect_filled(ui.max_rect(), 0.0, p.ground);
    for (col, art) in [(0.0, &*on_art), (500.0, &*off_art)] {
        let x = 20.0 + col;
        let mut y = 20.0;
        for m in Mark::ALL {
            let r = art.mark(&painter, m, egui::pos2(x, y), p.bright);
            painter.rect_stroke(r, 0.0, (1.0, p.frame_dim), egui::StrokeKind::Outside);
            y = r.bottom() + 20.0;
        }
        let d = art.device(&painter, egui::pos2(x + 12.0, y + 14.0), 1, p.bright);
        painter.rect_stroke(d, 0.0, (1.0, p.frame_dim), egui::StrokeKind::Outside);
        y = d.bottom() + 24.0;
        for (i, icon) in Icon::ALL.iter().enumerate() {
            let at = egui::pos2(x + (i % 16) as f32 * 26.0, y + (i / 16) as f32 * 26.0);
            art.icon(
                &painter,
                egui::Rect::from_min_size(at, egui::vec2(24.0, 24.0)),
                *icon,
                p.bright,
            );
        }
        y += 3.0 * 26.0 + 10.0;
        let halo = (art.effects == Effects::Soft).then_some(effect::LABEL_HALO_SOFT);
        art.label.paint(
            &painter,
            egui::pos2(x, y),
            "PLAIN LABEL",
            p.bright,
            1.0,
            0.0,
        );
        art.label.paint_lit(
            &painter,
            egui::pos2(x + 120.0, y),
            "LIT LABEL",
            p.bright,
            1.0,
            0.0,
            halo,
        );
        art.label.paint_lit(
            &painter,
            egui::pos2(x, y + 24.0),
            "APPLICATION & SYSTEM",
            p.bright,
            1.0,
            text::LABEL_TRACKING,
            halo,
        );
    }
}

#[test]
#[ignore = "writes a PNG file; needs a wgpu adapter"]
fn render_the_art_sheet() {
    let Some(dir) = std::env::var_os("STUDIO_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 1250.0))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_ui_state(paint_sheet, None);
    h.run_steps(3);
    let img = h.render().unwrap();
    img.save(dir.join("sheet-green.png")).unwrap();
}
