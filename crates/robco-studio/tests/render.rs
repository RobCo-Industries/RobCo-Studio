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
    harness_scaled(folder, height, 1.0)
}

fn harness_scaled(folder: Option<&Path>, height: f32, ppp: f32) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, height))
        .with_pixels_per_point(ppp)
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

/// The Project page with the sample project in each colour theme, the Card
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
    let mut h2 = harness_scaled(Some(&d), 720.0, 2.0);
    open_main(&mut h2);
    shot(&mut h2, &dir, "window-green-2x.png").unwrap();
    drop(h2);
    // The longest project name the manifest allows, on the tape's label.
    let ln = sample("longname").unwrap();
    std::fs::write(
        ln.join("robco.toml"),
        MANIFEST.replace("name = \"PIPES\"", "name = \"LONGNAME\""),
    )
    .unwrap();
    let mut h3 = harness(Some(&ln));
    open_main(&mut h3);
    shot(&mut h3, &dir, "window-longname.png").unwrap();
    drop(h3);
    h.state_mut().page = Page::Card;
    shot(&mut h, &dir, "window-card.png").unwrap();
    h.state_mut().page = Page::Project;
    h.state_mut().pick_theme(1);
    shot(&mut h, &dir, "window-amber.png").unwrap();
    h.state_mut().pick_theme(2);
    shot(&mut h, &dir, "window-blue.png").unwrap();
    h.state_mut().pick_theme(3);
    shot(&mut h, &dir, "window-white.png").unwrap();
    h.state_mut().pick_theme(0);
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
/// tinted `bright`. The left column is at Soft, the right one at Off.
fn paint_sheet(ui: &mut egui::Ui, arts: &mut Option<[Art; 2]>) {
    use robco_studio::theme::{Screen, Theme, effect, text};
    let ctx = ui.ctx().clone();
    let soft = Theme::default();
    let mut off = soft;
    off.screen = Screen::Off;
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
        let halo = art.halos().then_some(effect::LABEL_HALO_SOFT);
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

/// How the screen shows in a picture, and how a test gets it: the shown
/// form follows the theme, reduce motion and the adapter.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Look {
    /// Flat: ten zeros.
    Off,
    /// Green with no adapter known: the still overlays.
    Soft,
    /// Green with reduce motion on.
    Still,
    /// Green.
    Live,
    /// Arcade.
    Arcade,
}

impl Look {
    const ALL: [Look; 5] = [Look::Off, Look::Soft, Look::Still, Look::Live, Look::Arcade];

    fn name(self) -> &'static str {
        match self {
            Look::Off => "off",
            Look::Soft => "soft",
            Look::Still => "still",
            Look::Live => "live",
            Look::Arcade => "arcade",
        }
    }

    /// The shipped theme, reduce motion, and whether the adapter is
    /// hidden from the rules.
    fn how(self) -> (usize, bool, bool) {
        match self {
            Look::Off => (5, false, false),
            Look::Soft => (0, false, true),
            Look::Still => (0, true, false),
            Look::Live => (0, false, false),
            Look::Arcade => (4, false, false),
        }
    }

    /// The form the screen draws in this look on a real adapter.
    fn screen(self) -> robco_studio::theme::Screen {
        use robco_studio::theme::Screen;
        match self {
            Look::Off => Screen::Off,
            Look::Soft => Screen::Soft,
            Look::Still => Screen::Still,
            Look::Live | Look::Arcade => Screen::Live,
        }
    }
}

/// A window in `look`, its pass plugged in before the first frame on the
/// same device as the harness's renderer, as eframe would give it.
fn screen_harness(folder: Option<&Path>, ppp: f32, look: Look) -> Harness<'static, Studio> {
    screen_harness_gpu(folder, ppp, look, None).0
}

/// [`screen_harness`] and the render state its pass was made on.
fn screen_harness_gpu(
    folder: Option<&Path>,
    ppp: f32,
    look: Look,
    settings: Option<&Path>,
) -> (Harness<'static, Studio>, eframe::egui_wgpu::RenderState) {
    use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};
    use robco_studio::screen::Adapter;
    let rs = create_render_state(default_wgpu_setup(), Default::default());
    let (theme, reduce, hidden) = look.how();
    let adapter = if hidden {
        Adapter::Unknown
    } else {
        Adapter::of(rs.adapter.get_info().device_type)
    };
    let mut studio = Studio::new(folder);
    if let Some(dir) = settings {
        studio = studio.with_settings(dir.to_path_buf());
    }
    let link = studio.screen.link();
    let gpu = rs.clone();
    studio.choose_screen(adapter, Some(reduce), Some(theme));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .with_pixels_per_point(ppp)
        .with_context_setup(move |ctx| robco_studio::pass::plug(ctx, link, Some(&gpu)))
        .renderer(WgpuTestRenderer::from_render_state(rs.clone()))
        .build_ui_state(full_window, studio);
    h.run_steps(3);
    (h, rs)
}

/// Make the theme in use run the pass at zero: persistence alone, so
/// little that its afterglow ends at once, and the pass then draws every
/// strength at 0.
fn pass_at_zero(h: &mut Harness<'static, Studio>) {
    use robco_studio::theme::Strengths;
    h.state_mut().edit_theme(|t| {
        t.moves = true;
        t.strengths = Strengths {
            persistence: 0.01,
            ..Strengths::NONE
        };
    });
}

/// The Project page in each look, three Live frames half a second apart,
/// an open menu, an open dropdown, a tooltip and a dialog at Live, and Live
/// at display scale 2, as `screen-*.png`, and View with high legibility on,
/// as `menu-view-marks.png`. Into `STUDIO_SHOTS`.
#[test]
#[ignore = "writes PNG files; needs a wgpu adapter"]
fn render_the_screen_levels() {
    use egui_kittest::kittest::NodeT;
    let Some(dir) = std::env::var_os("STUDIO_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let d = sample("screen").unwrap();
    for look in Look::ALL {
        let mut h = screen_harness(Some(&d), 1.0, look);
        assert_eq!(
            h.state().theme.screen,
            look.screen(),
            "no hardware adapter here"
        );
        open_main(&mut h);
        let name = format!("screen-{}.png", look.name());
        shot(&mut h, &dir, &name).unwrap();
    }
    let mut h = screen_harness(Some(&d), 1.0, Look::Live);
    open_main(&mut h);
    for i in 0..3 {
        // Two steps of a quarter second each.
        shot(&mut h, &dir, &format!("screen-live-{i}.png")).unwrap();
        h.run_steps(2);
    }
    // An open menu, with its Theme submenu.
    h.query_all_by_label("View")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::MenuItem)
        .unwrap()
        .click();
    h.run_steps(2);
    let sub = h
        .query_all_by_label("Theme")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::MenuItem)
        .unwrap()
        .rect();
    h.hover_at(sub.center());
    h.run_steps(3);
    shot(&mut h, &dir, "screen-live-menu.png").unwrap();
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    // An open dropdown list.
    h.query_all_by_label_contains("Target:")
        .next()
        .unwrap()
        .click();
    h.run_steps(3);
    shot(&mut h, &dir, "screen-live-dropdown.png").unwrap();
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    // A tooltip, over a button that is not in this release.
    let settings = h.get_by_label("Settings").rect();
    h.hover_at(settings.center());
    h.run_steps(6);
    shot(&mut h, &dir, "screen-live-tooltip.png").unwrap();
    h.hover_at(egui::pos2(640.0, 400.0));
    h.run_steps(2);
    // A dialog over the scrim.
    h.state_mut().theme_editor.open = true;
    shot(&mut h, &dir, "screen-live-dialog.png").unwrap();
    h.state_mut().theme_editor.open = false;
    h.run_steps(2);
    // View with high legibility on: its mark, Green's in Theme, and the
    // note that high legibility holds the theme.
    h.state_mut().theme.high_legibility = true;
    h.run_steps(2);
    h.query_all_by_label("View")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::MenuItem)
        .unwrap()
        .click();
    h.run_steps(2);
    let sub = h
        .query_all_by_label("Theme")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::MenuItem)
        .unwrap()
        .rect();
    h.hover_at(sub.center());
    h.run_steps(3);
    shot(&mut h, &dir, "menu-view-marks.png").unwrap();
    drop(h);
    let mut h = screen_harness(Some(&d), 2.0, Look::Live);
    open_main(&mut h);
    shot(&mut h, &dir, "screen-live-2x.png").unwrap();
}

/// A theme entered from another draws as the same theme entered at start,
/// with no adapter known: Green, at Soft, entered from Flat gains the
/// drawings' halos and the scanline quad, and Flat, at Off, entered from
/// Green drops them. Neither runs the pass, so pixel for pixel.
#[test]
#[ignore = "needs a wgpu adapter"]
fn a_theme_entered_later_draws_as_at_start() {
    use robco_studio::menus::{Command, ThemeId};
    use robco_studio::theme::Screen;
    let d = sample("entered").unwrap();
    let dir = std::env::var_os("STUDIO_SHOTS").map(PathBuf::from);
    let shown = |t: usize| if t == 0 { Screen::Soft } else { Screen::Off };
    for (from, to) in [(5, 0), (0, 5)] {
        let picture = |first: usize| {
            let mut h = screen_harness(Some(&d), 1.0, Look::Soft);
            if first != 0 {
                h.state_mut().pick_theme(first);
                h.run_steps(2);
            }
            assert_eq!(h.state().theme.screen, shown(first));
            open_main(&mut h);
            if first != to {
                h.state_mut().pending.push(Command::Theme(ThemeId::At(to)));
            }
            h.run_steps(4);
            assert_eq!(h.state().theme.screen, shown(to));
            h.state_mut().status.1 = "Ready.".to_owned();
            h.hover_at(egui::pos2(900.0, 500.0));
            h.run_steps(4);
            h.render().unwrap()
        };
        let start = picture(to);
        let later = picture(from);
        if let Some(dir) = &dir {
            std::fs::create_dir_all(dir).unwrap();
            let name = format!(
                "screen-{}-from-{}.png",
                shown(to).name(),
                shown(from).name()
            )
            .to_lowercase();
            later.save(dir.join(name)).unwrap();
        }
        assert_eq!(start.dimensions(), later.dimensions());
        let differ = start
            .pixels()
            .zip(later.pixels())
            .filter(|(a, b)| a != b)
            .count();
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for (x, y, a) in start.enumerate_pixels() {
            if a != later.get_pixel(x, y) {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
        assert_eq!(
            differ, 0,
            "{to:?} from {from:?}: {differ} pixels differ in {x0},{y0} to {x1},{y1}"
        );
    }
}

/// With every strength at zero the pass draws what egui draws: the code
/// text, the Spleen labels, the icons, the drawings and the device all
/// reach the pass's renderer. With the curve, the corners outside it are
/// black.
#[test]
#[ignore = "needs a wgpu adapter"]
fn the_pass_at_zero_draws_what_egui_draws() {
    use robco_studio::theme::Screen;
    let d = sample("zero").unwrap();
    let mut off = screen_harness(Some(&d), 1.0, Look::Off);
    open_main(&mut off);
    off.run_steps(4);
    let flat = off.render().unwrap();
    let mut pass = screen_harness(Some(&d), 1.0, Look::Live);
    if pass.state().theme.screen != Screen::Live {
        return; // A software adapter: no pass to compare.
    }
    pass_at_zero(&mut pass);
    pass.state_mut().status.1 = off.state().status.1.clone();
    open_main(&mut pass);
    pass.run_steps(4);
    let through = pass.render().unwrap();
    let mut worst = 0;
    let mut differ = 0;
    for (a, b) in flat.pixels().zip(through.pixels()) {
        let d = (0..3).map(|i| a.0[i].abs_diff(b.0[i])).max().unwrap_or(0);
        worst = worst.max(d);
        differ += usize::from(d > 2);
    }
    assert!(differ < 50, "{differ} pixels differ, by up to {worst}");
    // Live: the corner is outside the curve, black. A pick would ask
    // first, so put the saved values back to end the change.
    let saved = pass.state().themes()[0].clone();
    pass.state_mut().edit_theme(|t| *t = saved);
    pass.run_steps(2);
    let live = pass.render().unwrap();
    for (x, y) in [(0, 0), (1279, 0), (0, 719), (1279, 719)] {
        let p = live.get_pixel(x, y).0;
        assert!(p[..3].iter().all(|c| *c < 4), "corner {x},{y}: {p:?}");
    }
}

/// A texture made, painted and dropped in one frame is in that frame's
/// picture through the pass: egui frees it after the frame is painted, and
/// the pass's renderer must too.
#[test]
#[ignore = "needs a wgpu adapter"]
fn a_texture_dropped_in_its_frame_is_drawn_through_the_pass() {
    use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};
    use robco_studio::screen::{Adapter, Settings};
    use robco_studio::theme::{Screen, Strengths, crt};
    let rs = create_render_state(default_wgpu_setup(), Default::default());
    if Adapter::of(rs.adapter.get_info().device_type) != Adapter::Hardware {
        return; // A software adapter: no pass.
    }
    let settings = Settings::default();
    let link = settings.link();
    let gpu = rs.clone();
    let spot = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(64.0, 64.0));
    let mut h = Harness::builder()
        .with_size(egui::vec2(320.0, 240.0))
        .with_pixels_per_point(1.0)
        .with_context_setup(move |ctx| robco_studio::pass::plug(ctx, link, Some(&gpu)))
        .renderer(WgpuTestRenderer::from_render_state(rs))
        .build_ui_state(
            move |ui, s: &mut Settings| {
                let ctx = ui.ctx().clone();
                let red = egui::ColorImage::filled([4, 4], egui::Color32::RED);
                let tex = ctx.load_texture("dropped", red, Default::default());
                ui.painter().image(
                    tex.id(),
                    spot,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                drop(tex);
                let mut theme = robco_studio::theme::Theme::default();
                theme.screen = Screen::Still;
                theme.strengths = Strengths::NONE;
                let drawn = s.drawn(theme, crt::RATE);
                s.end_frame(&ctx, &drawn);
            },
            settings,
        );
    h.run_steps(3);
    let img = h.render().unwrap();
    let c = spot.center();
    let p = img.get_pixel(c.x as u32, c.y as u32).0;
    assert!(p[0] > 200 && p[1] < 40 && p[2] < 40, "{p:?}");
}

/// A second plug, after the pass has drawn, changes nothing: the window's
/// own plug and the harness's, the same context twice, keep one pass and
/// its textures, so the picture stays.
#[test]
#[ignore = "needs a wgpu adapter"]
fn a_second_plug_keeps_the_pass() {
    use robco_studio::theme::Screen;
    let d = sample("replug").unwrap();
    let (mut h, rs) = screen_harness_gpu(Some(&d), 1.0, Look::Live, None);
    if h.state().theme.screen != Screen::Live {
        return; // A software adapter: no pass.
    }
    pass_at_zero(&mut h);
    open_main(&mut h);
    h.run_steps(4);
    let before = h.render().unwrap();
    let ctx = h.ctx.clone();
    h.state().screen.plug(&ctx, Some(&rs));
    robco_studio::pass::plug(&ctx, h.state().screen.link(), Some(&rs));
    h.run_steps(4);
    let after = h.render().unwrap();
    let differ = before
        .pixels()
        .zip(after.pixels())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(differ, 0, "{differ} pixels differ after a second plug");
}

/// Back at a moving level the pass keeps nothing of the picture it drew
/// before it stopped: Live on a white page, Soft on a dark one, then Live
/// on a third. Its first frame is the frame drawn with no history. Every
/// strength is zero but persistence, so the pictures compare exactly.
#[test]
#[ignore = "needs a wgpu adapter"]
fn the_afterglow_starts_empty_when_the_pass_resumes() {
    use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};
    use robco_studio::pass::Shared;
    use robco_studio::screen::{Adapter, Frame};
    use robco_studio::theme::Strengths;
    /// What one frame shows: a page colour, through the pass or not.
    #[derive(Clone, Copy)]
    struct Shown {
        page: egui::Color32,
        pass: bool,
        /// The frame's first pass is discarded and run again.
        discard: bool,
    }
    let picture = |frames: &[Shown]| {
        let rs = create_render_state(default_wgpu_setup(), Default::default());
        if Adapter::of(rs.adapter.get_info().device_type) != Adapter::Hardware {
            return None; // A software adapter: no pass.
        }
        let link = Shared::default();
        let plugged = link.clone();
        let gpu = rs.clone();
        let mut h = Harness::builder()
            .with_size(egui::vec2(160.0, 120.0))
            .with_pixels_per_point(1.0)
            .with_context_setup(move |ctx| robco_studio::pass::plug(ctx, plugged, Some(&gpu)))
            .renderer(WgpuTestRenderer::from_render_state(rs))
            .build_ui_state(
                move |ui, shown: &mut Shown| {
                    let r = ui.ctx().content_rect();
                    ui.painter().rect_filled(r, 0.0, shown.page);
                    if shown.discard && !ui.ctx().will_discard() {
                        ui.ctx().request_discard("test");
                    }
                    let mut l = link.lock().unwrap();
                    l.frame = shown.pass.then_some(Frame {
                        strengths: Strengths {
                            persistence: 1.0,
                            ..Strengths::NONE
                        },
                        time: 0.0,
                        scan_offset: 0.0,
                        keep: 0.9,
                    });
                },
                frames[0],
            );
        let mut last = None;
        for f in frames {
            *h.state_mut() = *f;
            h.step();
            last = Some(h.render().unwrap());
        }
        last
    };
    let live = |page| Shown {
        page,
        pass: true,
        discard: false,
    };
    let soft = |page| Shown {
        page,
        pass: false,
        discard: false,
    };
    let dark = egui::Color32::from_rgb(10, 20, 30);
    let resumed = picture(&[
        live(egui::Color32::WHITE),
        live(egui::Color32::WHITE),
        soft(egui::Color32::BLACK),
        soft(egui::Color32::BLACK),
        live(dark),
    ]);
    let fresh = picture(&[live(dark)]);
    let (Some(resumed), Some(fresh)) = (resumed, fresh) else {
        return;
    };
    let differ = resumed
        .pixels()
        .zip(fresh.pixels())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(differ, 0, "{differ} pixels keep the old picture");
    // A frame run twice, its first pass discarded, still keeps the
    // afterglow of the frame before.
    let rerun = Shown {
        discard: true,
        ..live(dark)
    };
    let Some(kept) = picture(&[live(egui::Color32::WHITE), rerun]) else {
        return;
    };
    assert_ne!(
        kept, fresh,
        "the afterglow is kept through a discarded pass"
    );
}

/// A frame is shaded by the theme it was drawn for. A command that
/// switches Flat, at Off, to a theme at Live in the middle of a frame
/// leaves that frame a plain Off frame, with no pass; the next frame is a
/// plain Live frame. And the reverse. The Live theme's moving strengths
/// and persistence are zero, so the pictures compare exactly.
#[test]
#[ignore = "needs a wgpu adapter"]
fn a_frame_is_shaded_by_the_theme_that_drew_it() {
    use robco_studio::menus::{Command, ThemeId};
    use robco_studio::theme::{SHIPPED, Screen, Spec, Strengths, crt};
    const FLAT: usize = 5;
    // The theme at Live, added after the six.
    const STILL_LIVE: usize = 6;
    let still_live = Spec {
        name: "Calm".into(),
        strengths: Strengths {
            scanlines: 0.0,
            ..crt::LIVE.still()
        },
        ..SHIPPED[0].clone()
    };
    let harness_at = |theme: usize| {
        let mut h = screen_harness(None, 1.0, Look::Live);
        h.state_mut().list_theme(still_live.clone(), "calm.toml");
        h.state_mut().pick_theme(theme);
        h.run_steps(3);
        h
    };
    let level = |t: usize| if t == FLAT { Screen::Off } else { Screen::Live };
    // A plain frame in `theme`, with `status` in the footer.
    let plain = |theme: usize, status: &(robco_studio::kit::Level, String)| {
        let mut h = harness_at(theme);
        h.state_mut().status = status.clone();
        h.step();
        h.render().unwrap()
    };
    let differ = |a: &[u8], b: &[u8]| {
        assert_eq!(a.len(), b.len());
        a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x != y).count()
    };
    for (from, to) in [(FLAT, STILL_LIVE), (STILL_LIVE, FLAT)] {
        let mut h = harness_at(from);
        if h.state().theme.screen != level(from) {
            return; // A software adapter: no pass.
        }
        h.state_mut().pending.push(Command::Theme(ThemeId::At(to)));
        h.step();
        let mid = h.render().unwrap();
        let status = h.state().status.clone();
        assert_eq!(h.state().theme.screen, level(to));
        let n = differ(mid.as_raw(), plain(from, &status).as_raw());
        assert_eq!(
            n, 0,
            "{from:?} to {to:?}: the switch frame is not {from:?}'s"
        );
        h.step();
        let next = h.render().unwrap();
        let n = differ(next.as_raw(), plain(to, &status).as_raw());
        assert_eq!(n, 0, "{from:?} to {to:?}: the next frame is not {to:?}'s");
    }
}

/// The theme editor on a window at Live with a base colour the rule
/// raises: the Colour tab, the Screen tab, and the Screen tab with reduce
/// motion holding four sliders and the switch, as `theme-*.png`. Into
/// `STUDIO_SHOTS`.
#[test]
#[ignore = "writes PNG files; needs a wgpu adapter"]
fn render_the_theme_editor() {
    use robco_studio::app::ThemeTab;
    let Some(dir) = std::env::var_os("STUDIO_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let d = sample("theme-editor").unwrap();
    for (look, tab, name) in [
        (Look::Live, ThemeTab::Colour, "theme-colour.png"),
        (Look::Live, ThemeTab::Screen, "theme-screen.png"),
        (Look::Still, ThemeTab::Screen, "theme-screen-held.png"),
    ] {
        let mut h = screen_harness(Some(&d), 1.0, look);
        open_main(&mut h);
        h.state_mut()
            .edit_theme(|t| t.base = robco_studio::palette::hex(0xff7a1a));
        h.state_mut().theme_editor.open = true;
        h.state_mut().theme_editor.tab = tab;
        h.event(egui::Event::PointerGone);
        shot(&mut h, &dir, name).unwrap();
    }
}

/// The window behind the theme editor is its preview: with the editor
/// open, every pixel outside the pane and its halo is the same as with
/// the editor closed. Flat, so no pass moves a pixel.
#[test]
#[ignore = "needs a wgpu adapter"]
fn the_theme_editor_leaves_the_window_behind_as_it_was() {
    use robco_studio::theme::size;
    let d = sample("theme-behind").unwrap();
    let mut h = screen_harness(Some(&d), 1.0, Look::Off);
    h.state_mut().status.1 = "Ready.".to_owned();
    h.event(egui::Event::PointerGone);
    h.run_steps(4);
    let closed = h.render().unwrap();
    h.state_mut().theme_editor.open = true;
    h.run_steps(4);
    let open = h.render().unwrap();
    let pane = h.state().theme_pane_rect().expand(size::STEP_16);
    assert!(pane.width() > size::SIM_W);
    let mut differ = 0;
    for (x, y, a) in closed.enumerate_pixels() {
        if pane.contains(egui::pos2(x as f32, y as f32)) {
            continue;
        }
        if open.get_pixel(x, y) != a {
            differ += 1;
        }
    }
    assert_eq!(differ, 0, "pixels outside the pane changed");
}

/// Under Arcade's curvature a drag on a slider at the pane's right edge
/// tracks the pointer: the pass maps the pointer and the bounds both.
#[test]
#[ignore = "needs a wgpu adapter"]
fn a_slider_at_the_pane_edge_tracks_under_arcade() {
    use egui::accesskit::Role;
    use robco_studio::app::ThemeTab;
    let d = sample("theme-arcade").unwrap();
    let mut h = screen_harness(Some(&d), 1.0, Look::Arcade);
    h.state_mut().theme_editor.open = true;
    h.state_mut().theme_editor.tab = ThemeTab::Screen;
    h.run_steps(3);
    let r = h
        .query_all_by_label("Bloom")
        .find(|n| n.accesskit_node().role() == Role::Slider)
        .unwrap()
        .rect();
    let press = |h: &mut Harness<'static, Studio>, at: egui::Pos2, down: bool| {
        h.event(egui::Event::PointerMoved(at));
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: down,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    };
    let bloom = |h: &Harness<'static, Studio>| h.state().spec().strengths.bloom;
    press(&mut h, r.right_center() - egui::vec2(1.0, 0.0), true);
    assert!(
        bloom(&h) > 0.95,
        "the press at the right end: {}",
        bloom(&h)
    );
    h.event(egui::Event::PointerMoved(r.center()));
    h.run_steps(1);
    assert!((bloom(&h) - 0.5).abs() < 0.05, "the drag: {}", bloom(&h));
    press(&mut h, r.left_center(), false);
    assert!(
        bloom(&h) < 0.05,
        "the release at the left end: {}",
        bloom(&h)
    );
}

/// The states of a theme, into `STUDIO_SHOTS`: the editor with the Save
/// as name row open, the Delete question, View, Theme with a person's
/// theme and a changed one, and the window in each of the eight quick
/// colours, as `theme-*.png` and `quick-*.png`. The settings folder is a
/// temporary one.
#[test]
#[ignore = "writes PNG files; needs a wgpu adapter"]
fn render_the_theme_states() {
    use egui::accesskit::Role;
    let Some(dir) = std::env::var_os("STUDIO_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let d = sample("theme-states").unwrap();
    let settings = Sample(std::env::temp_dir().join(format!(
        "robco-studio-shots-settings-{}",
        std::process::id()
    )));
    let themes = settings.join("themes");
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(
        themes.join("ember.toml"),
        "version = 1\nname = \"Ember\"\nbase = \"#ff7a1a\"\n[screen]\nbloom = 0.45\nscanlines = 0.3\n",
    )
    .unwrap();
    std::fs::write(themes.join("notes.toml"), "version = 1\n").unwrap();
    let (mut h, _) = screen_harness_gpu(Some(&d), 1.0, Look::Live, Some(&settings));
    open_main(&mut h);
    h.state_mut().edit_theme(|t| t.strengths.noise = 0.2);
    h.state_mut().theme_editor.open = true;
    h.run_steps(2);
    h.state_mut().start_naming(false);
    h.event(egui::Event::PointerGone);
    shot(&mut h, &dir, "theme-save-as.png").unwrap();
    h.state_mut().theme_editor.naming = None;
    let ember = h.state().theme_index("Ember").unwrap();
    h.state_mut().pick_theme(ember);
    h.state_mut().theme_editor.question = Some(robco_studio::app::ThemeQuestion::Delete);
    shot(&mut h, &dir, "theme-delete.png").unwrap();
    h.state_mut().theme_editor.question = None;
    h.state_mut().theme_editor.open = false;
    h.state_mut().edit_theme(|t| t.strengths.noise = 0.2);
    h.run_steps(2);
    h.query_all_by_label("View")
        .find(|n| n.accesskit_node().role() == Role::MenuItem)
        .unwrap()
        .click();
    h.run_steps(2);
    let sub = h
        .query_all_by_label("Theme")
        .find(|n| n.accesskit_node().role() == Role::MenuItem)
        .unwrap()
        .rect();
    h.hover_at(sub.center());
    h.run_steps(3);
    shot(&mut h, &dir, "theme-menu-own.png").unwrap();
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.event(egui::Event::PointerGone);
    for (name, c) in robco_studio::palette::QUICK {
        h.state_mut().edit_theme(|t| t.base = c);
        let file = format!("quick-{}.png", name.to_lowercase());
        shot(&mut h, &dir, &file).unwrap();
    }
}

/// How bright the glow is just outside a `bright` glyph at Bloom 1 on
/// Green, as a share of `bright`. It prints the number and checks nothing
/// against a limit. It reads the Files pane's title, FILES. Glyph pixels
/// are those within 8 of `bright` on every channel in a frame with the
/// pass at zero. The halo is each pixel 1 to 3 pixels from a glyph pixel
/// that is `pane` in that frame. The share is
/// the halo pixel's largest channel at Bloom 1 over `bright`'s, and the
/// same less the frame at zero.
#[test]
#[ignore = "needs a wgpu adapter; prints a measurement"]
fn measure_the_bloom_halo() {
    use robco_studio::theme::Strengths;
    let d = sample("bloom").unwrap();
    let mut h = screen_harness(Some(&d), 1.0, Look::Still);
    h.event(egui::Event::PointerGone);
    let bright = h.state().theme.palette().bright;
    let frame = |h: &mut Harness<'static, Studio>, s: Strengths| {
        h.state_mut().edit_theme(|t| {
            t.moves = false;
            t.strengths = s;
        });
        h.run_steps(6);
        h.render().unwrap()
    };
    let zero = frame(
        &mut h,
        Strengths {
            persistence: 0.01,
            ..Strengths::NONE
        },
    );
    let one = frame(
        &mut h,
        Strengths {
            bloom: 1.0,
            ..Strengths::NONE
        },
    );
    let near = |a: &image::Rgba<u8>, c: egui::Color32, by: i32| {
        (0..3).all(|k| (i32::from(a[k]) - i32::from([c.r(), c.g(), c.b()][k])).abs() <= by)
    };
    // The Files pane's title row: FILES, a caps label in `bright` on
    // `pane`, away from any filled control.
    let files = h
        .state()
        .place_rect(robco_studio::app::Place::Files)
        .unwrap();
    let ground = h.state().theme.palette().pane;
    let (x0, y0) = (files.left() as u32, files.top() as u32);
    let (w, ht) = (files.right() as u32, (files.top() + 26.0) as u32);
    let top = |c: &[u8]| f32::from(c[0].max(c[1]).max(c[2]));
    let b = f32::from(bright.r().max(bright.g()).max(bright.b()));
    let (mut most, mut most_less, mut glyphs) = (0.0f32, 0.0f32, 0);
    for y in y0..ht {
        for x in x0..w {
            if !near(zero.get_pixel(x, y), bright, 8) {
                continue;
            }
            glyphs += 1;
            for dy in -3i32..=3 {
                for dx in -3i32..=3 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= ht as i32 {
                        continue;
                    }
                    let (nx, ny) = (nx as u32, ny as u32);
                    if !near(zero.get_pixel(nx, ny), ground, 4) {
                        continue;
                    }
                    let lit = top(&one.get_pixel(nx, ny).0);
                    let base = top(&zero.get_pixel(nx, ny).0);
                    most = most.max(lit / b);
                    most_less = most_less.max((lit - base) / b);
                }
            }
        }
    }
    assert!(glyphs > 0, "no bright glyph found");
    eprintln!(
        "bloom halo: {glyphs} glyph pixels, brightest halo pixel {most:.3} of bright, {most_less:.3} above the ground"
    );
}

/// A settings folder with 64 themes, "Theme 00" on, removed when the test
/// ends.
fn many_themes(tag: &str) -> std::io::Result<Sample> {
    let settings = Sample(
        std::env::temp_dir().join(format!("robco-studio-shots-{tag}-{}", std::process::id())),
    );
    let themes = settings.join("themes");
    let _ = std::fs::remove_dir_all(&*settings);
    std::fs::create_dir_all(&themes)?;
    for i in 0..64 {
        std::fs::write(
            themes.join(format!("t{i:02}.toml")),
            format!("version = 1\nname = \"Theme {i:02}\"\nbase = \"#5aff7f\"\n"),
        )?;
    }
    Ok(settings)
}

/// Open the theme editor's Theme list with a click where it is drawn.
fn open_theme_list(h: &mut Harness<'static, Studio>) -> Option<egui::Rect> {
    use egui::accesskit::Role;
    h.state_mut().theme_editor.open = true;
    h.run_steps(3);
    let field = h
        .query_all_by_role(Role::ComboBox)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("Theme:"))
        })?
        .rect();
    click_at(h, field.center());
    h.run_steps(4);
    h.query_by_role(Role::ListBox).map(|n| n.rect())
}

fn click_at(h: &mut Harness<'static, Studio>, at: egui::Pos2) {
    h.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(1);
}

/// The theme editor's Theme list open over 64 themes, into `STUDIO_SHOTS`
/// as `theme-list-64.png`.
#[test]
#[ignore = "writes PNG files; needs a wgpu adapter"]
fn render_the_long_theme_list() {
    let Some(dir) = std::env::var_os("STUDIO_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let d = sample("theme-list").unwrap();
    let settings = many_themes("theme-list-settings").unwrap();
    let (mut h, _) = screen_harness_gpu(Some(&d), 1.0, Look::Live, Some(&settings));
    open_main(&mut h);
    open_theme_list(&mut h).unwrap();
    h.key_press(egui::Key::End);
    h.event(egui::Event::PointerGone);
    shot(&mut h, &dir, "theme-list-64.png").unwrap();
}

/// Under Arcade's curvature, a click on the last row the open list shows
/// picks that row: the pass maps the pointer back through the curve.
#[test]
#[ignore = "needs a wgpu adapter"]
fn a_click_on_the_last_shown_row_picks_it_under_arcade() {
    use egui::accesskit::Role;
    let d = sample("theme-list-arcade").unwrap();
    let settings = many_themes("theme-list-arcade-settings").unwrap();
    let (mut h, _) = screen_harness_gpu(Some(&d), 1.0, Look::Arcade, Some(&settings));
    let list = open_theme_list(&mut h).unwrap();
    let (label, r) = h
        .query_all_by_role(Role::ListBoxOption)
        .map(|n| (n.accesskit_node().label().unwrap_or_default(), n.rect()))
        .filter(|(_, r)| list.contains_rect(*r))
        .max_by(|a, b| a.1.bottom().total_cmp(&b.1.bottom()))
        .unwrap();
    assert!(label.starts_with("Theme "), "{label}");
    click_at(&mut h, r.center());
    h.run_steps(3);
    assert_eq!(h.state().theme_name(), label);
}
