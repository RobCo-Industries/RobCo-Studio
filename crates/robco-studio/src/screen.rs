// SPDX-License-Identifier: GPL-3.0-or-later

//! The rules of the screen levels, with no graphics: which level the window
//! starts in, which levels a person may pick, when the window asks for a
//! frame, how persistence fades, and the curve that the pointer is mapped
//! back through. [`crate::pass`] draws; this module decides.
//!
//! # When the screen asks for frames
//!
//! Each strength alone above zero, the other nine at zero, at a pass level:
//!
//! | Strength | Frames |
//! | --- | --- |
//! | bloom | never |
//! | vignette | never |
//! | scanlines | always at Live and Arcade, where they drift; never at Still |
//! | curvature | never |
//! | persistence | after a change of the picture, until the afterglow fades |
//! | noise | always |
//! | flicker | always |
//! | band | always |
//! | mask | never |
//! | aberration | never |
//!
//! "Always" means at the frame rate while the window has focus. Persistence
//! is afterglow: it shows only while the picture changes. A frame counts as
//! a change unless the screen's own request was its only cause and it
//! brought no input. After a change the screen asks for frames until what
//! is left of the old picture is below [`AFTERGLOW_END`], then stops, so an
//! idle window still costs nothing. The afterglow runs without focus too:
//! it ends by itself.

use crate::theme::{Screen, Strengths, crt};
use eframe::egui::{self, Pos2, Rect};
use std::time::Duration;

/// What kind of graphics adapter the window draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adapter {
    /// None is known: the headless tests, or a window with no wgpu.
    Unknown,
    /// A software adapter, which runs the GPU's work on the CPU.
    Software,
    /// A real graphics adapter.
    Hardware,
}

impl Adapter {
    /// The kind of a wgpu adapter. Only a CPU adapter is software.
    pub fn of(device_type: eframe::egui_wgpu::wgpu::DeviceType) -> Adapter {
        match device_type {
            eframe::egui_wgpu::wgpu::DeviceType::Cpu => Adapter::Software,
            _ => Adapter::Hardware,
        }
    }
}

/// Said when the screen pass cannot run on this machine.
pub const NO_ADAPTER: &str = "This machine has no graphics adapter for the screen pass.";
/// Said when Studio starts in Still because of the system's setting.
pub const REDUCED: &str = "The system asks for reduced motion.";
/// Said by a level that high legibility does not allow.
pub const LEGIBLE: &str = "High legibility keeps the screen Off.";

/// The status line for a level: "Screen: Live.".
pub fn news(level: Screen, custom: bool) -> String {
    if custom {
        "Screen: Custom.".to_owned()
    } else {
        format!("Screen: {}.", level.name())
    }
}

/// The level at start, and what the status line says about it.
///
/// A level asked for on the command line comes first, unless it needs the
/// pass and there is no real adapter. With no real adapter Studio starts in
/// Soft. When the system asks for reduced motion it starts in Still.
/// Otherwise it starts in Live.
pub fn start(
    adapter: Adapter,
    reduce_motion: Option<bool>,
    asked: Option<Screen>,
) -> (Screen, Option<String>) {
    let no_pass = adapter != Adapter::Hardware;
    let soft = || {
        let why = (adapter == Adapter::Software).then(|| format!("{NO_ADAPTER} Screen: Soft."));
        (Screen::Soft, why)
    };
    match asked {
        Some(l) if l.pass() && no_pass => soft(),
        Some(l) => (l, None),
        None if no_pass => soft(),
        None if reduce_motion == Some(true) => {
            (Screen::Still, Some(format!("{REDUCED} Screen: Still.")))
        }
        None => (Screen::Live, None),
    }
}

/// Why a person cannot pick `level` now, or None.
pub fn unavailable(level: Screen, adapter: Adapter, high_legibility: bool) -> Option<&'static str> {
    if high_legibility && level != Screen::Off {
        Some(LEGIBLE)
    } else if level.pass() && adapter != Adapter::Hardware {
        Some(NO_ADAPTER)
    } else {
        None
    }
}

/// The strengths differ from the level's own: the level reads "Custom".
pub fn custom(level: Screen, s: &Strengths) -> bool {
    level.pass() && *s != level.strengths()
}

/// The pass changes from frame to frame at these strengths.
pub fn animated(level: Screen, s: &Strengths) -> bool {
    level.pass()
        && (s.noise > 0.0
            || s.flicker > 0.0
            || s.band > 0.0
            || (level.moving() && s.scanlines > 0.0))
}

/// The afterglow stops when what is left of the old picture is below this
/// share of it: less than one step of an 8-bit channel.
pub const AFTERGLOW_END: f32 = crt::AFTERGLOW_END;

/// When the strengths make the screen ask for frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// Never: the picture is the same each time it is drawn.
    Never,
    /// After a change of the picture, until the afterglow fades.
    AfterChange,
    /// Always, at the frame rate while the window has focus.
    Always,
}

/// When the screen asks for frames at a level and its strengths. See the
/// table in the module's docs.
pub fn motion(level: Screen, s: &Strengths) -> Motion {
    if animated(level, s) {
        Motion::Always
    } else if level.pass() && s.persistence > 0.0 {
        Motion::AfterChange
    } else {
        Motion::Never
    }
}

/// When the window asks for its next frame: at the frame rate while the
/// pass moves and the window has focus, or while an afterglow fades,
/// otherwise never. An idle window then costs nothing.
pub fn next_frame(animated: bool, glowing: bool, focused: bool, rate: u32) -> Option<Duration> {
    ((animated && focused || glowing) && rate > 0).then(|| Duration::from_secs(1) / rate)
}

/// What persistence keeps of the last frame after `dt` seconds. The fade
/// is the same per second at any frame rate.
pub fn persistence_keep(strength: f32, dt: f32) -> f32 {
    if strength <= 0.0 {
        return 0.0;
    }
    let [lo, hi] = crt::PERSIST_DT;
    let frames = dt.clamp(lo, hi) * crt::PERSIST_RATE;
    (strength * crt::PERSIST_KEEP).powf(frames)
}

/// The barrel curve: from a place on the glass, 0 to 1 across, to the
/// place in the window's picture drawn there. The same formula as `curve`
/// in `screen.wgsl`.
pub fn warp(uv: [f32; 2], curvature: f32) -> [f32; 2] {
    let k = curvature * crt::CURVE;
    let (x, y) = (uv[0] * 2.0 - 1.0, uv[1] * 2.0 - 1.0);
    let (x2, y2) = (x * (1.0 + k * y * y), y * (1.0 + k * x * x));
    [(x2 + 1.0) / 2.0, (y2 + 1.0) / 2.0]
}

/// Map a pointer on the glass, in points, to the window's picture.
pub fn map_pointer(pos: Pos2, screen: Rect, curvature: f32) -> Pos2 {
    if curvature <= 0.0 || screen.width() <= 0.0 || screen.height() <= 0.0 {
        return pos;
    }
    let uv = [
        (pos.x - screen.min.x) / screen.width(),
        (pos.y - screen.min.y) / screen.height(),
    ];
    let w = warp(uv, curvature);
    Pos2::new(
        screen.min.x + w[0] * screen.width(),
        screen.min.y + w[1] * screen.height(),
    )
}

/// Where on the glass a point of the picture is drawn: the inverse of
/// [`map_pointer`], found by fixed-point steps.
pub fn drawn_at(pos: Pos2, screen: Rect, curvature: f32) -> Pos2 {
    let mut s = pos;
    for _ in 0..crt::INVERSE_STEPS {
        let m = map_pointer(s, screen, curvature);
        s += pos - m;
    }
    s
}

/// Map every pointer and touch position of `raw` through the curve, so a
/// click lands on what is drawn under it.
pub fn map_input(raw: &mut egui::RawInput, screen: Rect, curvature: f32) {
    if curvature <= 0.0 {
        return;
    }
    for e in &mut raw.events {
        match e {
            egui::Event::PointerMoved(p) => *p = map_pointer(*p, screen, curvature),
            egui::Event::PointerButton { pos, .. } | egui::Event::Touch { pos, .. } => {
                *pos = map_pointer(*pos, screen, curvature);
            }
            _ => {}
        }
    }
}

/// Move every rectangle of an accessibility tree update to where it is
/// drawn through the curve, so a screen reader outlines what is on the
/// glass. egui gives each node's bounds in points of the window: only the
/// root carries a transform, the scale to physical pixels. A node with a
/// transform of its own is left as it is. Each bounds becomes the box
/// around its four corners sent forward through the curve. With no
/// curvature the update is not touched.
pub fn map_tree(update: &mut egui::accesskit::TreeUpdate, screen: Rect, curvature: f32) {
    if curvature <= 0.0 {
        return;
    }
    for (_, node) in &mut update.nodes {
        if node.transform().is_some() {
            continue;
        }
        let Some(b) = node.bounds() else {
            continue;
        };
        let r = Rect::from_min_max(
            Pos2::new(b.x0 as f32, b.y0 as f32),
            Pos2::new(b.x1 as f32, b.y1 as f32),
        );
        let mut out = Rect::NOTHING;
        for c in [
            r.left_top(),
            r.right_top(),
            r.left_bottom(),
            r.right_bottom(),
        ] {
            out.extend_with(drawn_at(c, screen, curvature));
        }
        node.set_bounds(egui::accesskit::Rect {
            x0: out.min.x.into(),
            y0: out.min.y.into(),
            x1: out.max.x.into(),
            y1: out.max.y.into(),
        });
    }
}

/// What the system says about reduced motion, or None when Studio cannot
/// read it. On macOS this is the Reduce motion switch of the Accessibility
/// settings. Studio does not read it on Windows or Linux yet.
pub fn system_reduce_motion() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        let ws = objc2_app_kit::NSWorkspace::sharedWorkspace();
        Some(ws.accessibilityDisplayShouldReduceMotion())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// The screen's settings in the running window, beyond the level that the
/// theme holds. They last for the session: there is no settings file yet.
pub struct Settings {
    /// The strengths drawn at a pass level. Moved by the Adjust dialog.
    pub strengths: Strengths,
    /// Frames a second while the pass moves: one of [`crt::RATES`].
    pub rate: u32,
    /// The adapter the window draws with.
    pub adapter: Adapter,
    /// The Adjust dialog is open.
    pub adjusting: bool,
    clock: Clock,
    link: crate::pass::Shared,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            strengths: Screen::Soft.strengths(),
            rate: crt::RATE,
            adapter: Adapter::Unknown,
            adjusting: false,
            clock: Clock::default(),
            link: crate::pass::Shared::default(),
        }
    }
}

impl Settings {
    /// Plug the screen into `ctx` through [`crate::pass::plug`]: the
    /// pointer is mapped through the curve from then on, and with eframe's
    /// render state the pass draws. Only the first plug of a context
    /// counts, whoever makes it. With a render state, call it before the
    /// first frame.
    pub fn plug(&self, ctx: &egui::Context, gpu: Option<&eframe::egui_wgpu::RenderState>) {
        crate::pass::plug(ctx, self.link.clone(), gpu);
    }

    /// The window's side of the link to the screen's plugin, for a caller
    /// that plugs the screen in itself with [`crate::pass::plug`], as a
    /// test harness does before its first frame.
    pub fn link(&self) -> crate::pass::Shared {
        self.link.clone()
    }

    /// Hold the window's input from the next frame on, or let it through:
    /// while a system dialog is open over the window, nothing in it acts.
    pub fn hold_input(&self, hold: bool) {
        self.link
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .inert = hold;
    }

    /// End the frame: hand the frame to the pass at a pass level, set the
    /// curve the next input is mapped through, and ask for the next frame
    /// by the rules. Call last in the frame.
    pub fn end_frame(&mut self, ctx: &egui::Context, level: Screen) {
        self.plug(ctx, None);
        let (now, focused, input) = ctx.input(|i| (i.time, i.focused, !i.events.is_empty()));
        // The screen's own request for a frame is no change of the picture.
        let causes = ctx.repaint_causes();
        let own = !input && !causes.is_empty() && causes.iter().all(|c| c.file == file!());
        let frame = self.clock.frame(now, level, &self.strengths, focused, !own);
        {
            let mut link = self
                .link
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            link.frame = level.pass().then_some(frame);
            link.curvature = if level.pass() {
                self.strengths.curvature
            } else {
                0.0
            };
        }
        let moving = animated(level, &self.strengths);
        if let Some(d) = next_frame(moving, self.clock.glowing(), focused, self.rate) {
            ctx.request_repaint_after(d);
        }
    }
}

/// One frame of the pass: the strengths and the clock, after the rules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// The strengths drawn this frame.
    pub strengths: Strengths,
    /// The pass's clock, in seconds. It stops while nothing moves.
    pub time: f32,
    /// How far the scanlines moved down, in points.
    pub scan_offset: f32,
    /// What persistence keeps of the last frame.
    pub keep: f32,
}

/// The pass's clock: it runs while the pass moves and stops otherwise, so a
/// still picture does not change when it is drawn again.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Clock {
    /// The pass's time when it last moved.
    time: f32,
    /// The window's time of the last frame drawn.
    last: Option<f64>,
    /// What is left of the picture before the last change, out of 1.
    glow: f32,
    /// The last frame drawn moved: the next one goes on from it.
    moved: bool,
}

impl Clock {
    /// The frame to draw at the window's time `now`, for a level and its
    /// strengths, with or without focus. `changed` says the picture may
    /// differ from the last frame's.
    pub fn frame(
        &mut self,
        now: f64,
        level: Screen,
        s: &Strengths,
        focused: bool,
        changed: bool,
    ) -> Frame {
        let dt = self
            .last
            .map_or(0.0, |l| (now - l).max(0.0) as f32)
            .min(crt::MAX_STEP);
        self.last = Some(now);
        let moving = animated(level, s) && focused;
        // Animation time is time spent drawing moving frames: the first
        // frame after a pause goes on from the phase that was held.
        let step = if std::mem::replace(&mut self.moved, moving) {
            dt
        } else {
            0.0
        };
        if !moving && level.pass() && s.persistence > 0.0 {
            // Only the afterglow moves: the clock holds, the old picture fades.
            let fade = persistence_keep(s.persistence, dt);
            self.glow = if changed { fade } else { self.glow * fade };
            if self.glow < AFTERGLOW_END {
                self.glow = 0.0;
            }
            // With nothing left to fade, the still form, drawn the same.
            let keep = if changed || self.glow > 0.0 {
                fade
            } else {
                0.0
            };
            let persistence = if keep > 0.0 { s.persistence } else { 0.0 };
            return Frame {
                strengths: Strengths {
                    persistence,
                    ..s.still()
                },
                time: self.time,
                scan_offset: self.scan_offset(level),
                keep,
            };
        }
        self.glow = 0.0;
        if !moving {
            // The still form: what does not move, at the clock where it stopped.
            let still = s.still();
            return Frame {
                strengths: still,
                time: self.time,
                scan_offset: self.scan_offset(level),
                keep: 0.0,
            };
        }
        self.time += step;
        Frame {
            strengths: *s,
            time: self.time,
            scan_offset: self.scan_offset(level),
            keep: persistence_keep(s.persistence, step),
        }
    }

    /// An afterglow is still fading: the screen wants another frame.
    pub fn glowing(&self) -> bool {
        self.glow > 0.0
    }

    fn scan_offset(&self, level: Screen) -> f32 {
        if level.moving() {
            (self.time * crt::SCAN_DRIFT) % crt::SCAN_PERIOD
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0))
    }

    #[test]
    fn the_curve_is_flat_at_zero() {
        for uv in [[0.0, 0.0], [0.3, 0.7], [1.0, 1.0]] {
            assert_eq!(warp(uv, 0.0), uv);
        }
        assert_eq!(warp([0.5, 0.5], 1.0), [0.5, 0.5]);
    }

    #[test]
    fn the_pointer_goes_there_and_back() {
        for curve in [0.12, 0.4, 1.0] {
            for p in [
                Pos2::new(640.0, 360.0),
                Pos2::new(100.0, 50.0),
                Pos2::new(1200.0, 700.0),
                Pos2::new(20.0, 690.0),
            ] {
                let glass = drawn_at(p, screen(), curve);
                let back = map_pointer(glass, screen(), curve);
                assert!(
                    (back - p).length() < 0.01,
                    "{curve}: {p:?} -> {glass:?} -> {back:?}"
                );
            }
        }
        // Near a corner the curve moves a point inwards on the glass.
        let corner = Pos2::new(10.0, 10.0);
        let at = drawn_at(corner, screen(), 0.4);
        assert!(at.x > corner.x && at.y > corner.y, "{at:?}");
        // At Still's curvature the move stays under a point away from the
        // edges, and about ten points at a corner.
        let mid = Pos2::new(400.0, 250.0);
        assert!((drawn_at(mid, screen(), 0.12) - mid).length() < 1.0);
        assert!((drawn_at(corner, screen(), 0.12) - corner).length() < 12.0);
    }

    #[test]
    fn a_frame_is_asked_for_only_with_motion_and_focus() {
        assert_eq!(
            next_frame(true, false, true, 30),
            Some(Duration::from_secs(1) / 30)
        );
        assert_eq!(
            next_frame(true, false, true, 15),
            Some(Duration::from_secs(1) / 15)
        );
        assert_eq!(next_frame(true, false, false, 30), None);
        assert_eq!(
            next_frame(false, true, false, 30),
            Some(Duration::from_secs(1) / 30)
        );
        assert_eq!(next_frame(false, false, true, 60), None);
        for level in Screen::ALL {
            let a = animated(level, &level.strengths());
            assert_eq!(a, level.moving(), "{level:?}");
        }
        // Still with noise raised moves, without drift.
        let s = Strengths {
            noise: 0.2,
            ..Screen::Still.strengths()
        };
        assert!(animated(Screen::Still, &s));
        assert!(!animated(Screen::Soft, &Screen::Arcade.strengths()));
    }

    #[test]
    fn persistence_fades_the_same_at_any_rate() {
        let s = 0.5;
        let one = persistence_keep(s, 1.0 / 15.0);
        let half = persistence_keep(s, 1.0 / 30.0);
        let quarter = persistence_keep(s, 1.0 / 60.0);
        assert!((one - half * half).abs() < 1e-5, "{one} {half}");
        assert!((one - quarter.powi(4)).abs() < 1e-5);
        assert!((half - s * crt::PERSIST_KEEP).abs() < 1e-5);
        assert_eq!(persistence_keep(0.0, 0.1), 0.0);
        // A frame drawn twice at one time keeps less than all of it.
        assert!(persistence_keep(1.0, 0.0) < 1.0);
    }

    #[test]
    fn start_follows_the_adapter_and_the_system() {
        use Adapter::*;
        assert_eq!(start(Hardware, None, None), (Screen::Live, None));
        assert_eq!(start(Hardware, Some(false), None), (Screen::Live, None));
        let (l, why) = start(Hardware, Some(true), None);
        assert_eq!(l, Screen::Still);
        assert_eq!(
            why.as_deref(),
            Some("The system asks for reduced motion. Screen: Still.")
        );
        let (l, why) = start(Software, Some(true), None);
        assert_eq!(l, Screen::Soft);
        assert_eq!(
            why.as_deref(),
            Some("This machine has no graphics adapter for the screen pass. Screen: Soft.")
        );
        assert_eq!(start(Unknown, None, None), (Screen::Soft, None));
        assert_eq!(
            start(Hardware, Some(true), Some(Screen::Arcade)),
            (Screen::Arcade, None)
        );
        assert_eq!(
            start(Software, None, Some(Screen::Off)),
            (Screen::Off, None)
        );
        assert_eq!(start(Software, None, Some(Screen::Live)).0, Screen::Soft);
    }

    #[test]
    fn levels_follow_the_adapter_and_high_legibility() {
        for level in Screen::ALL {
            let pass = level.pass();
            assert_eq!(unavailable(level, Adapter::Hardware, false), None);
            assert_eq!(unavailable(level, Adapter::Software, false).is_some(), pass);
            assert_eq!(unavailable(level, Adapter::Unknown, false).is_some(), pass);
            assert_eq!(
                unavailable(level, Adapter::Hardware, true).is_some(),
                level != Screen::Off
            );
        }
        assert_eq!(
            Adapter::of(eframe::egui_wgpu::wgpu::DeviceType::Cpu),
            Adapter::Software
        );
        assert_eq!(
            Adapter::of(eframe::egui_wgpu::wgpu::DeviceType::IntegratedGpu),
            Adapter::Hardware
        );
    }

    #[test]
    fn the_clock_stops_without_focus() {
        let mut c = Clock::default();
        let s = Screen::Live.strengths();
        c.frame(1.0, Screen::Live, &s, true, true);
        let f = c.frame(1.05, Screen::Live, &s, true, true);
        assert!((f.time - 0.05).abs() < 1e-6);
        assert!(f.keep > 0.0 && f.strengths.noise > 0.0);
        let held = c.frame(9.0, Screen::Live, &s, false, false);
        assert_eq!(held.time, f.time);
        assert_eq!(held.keep, 0.0);
        assert_eq!(held.strengths, s.still());
        assert_eq!(held.scan_offset, f.scan_offset);
        let still = c.frame(10.0, Screen::Still, &Screen::Still.strengths(), true, true);
        assert_eq!(still.scan_offset, 0.0);
    }

    #[test]
    fn the_clock_goes_on_from_the_held_phase() {
        let s = Screen::Live.strengths();
        for rate in [15.0, 30.0, 60.0] {
            let mut c = Clock::default();
            let mut t = 0.0;
            let mut before = c.frame(t, Screen::Live, &s, true, true);
            for i in 1..=(2.0 * rate) as u32 {
                t = f64::from(i) / rate;
                before = c.frame(t, Screen::Live, &s, true, true);
            }
            // The band's speed is the same at any rate.
            assert!((before.time - 2.0).abs() < 0.02, "{rate}: {}", before.time);
            // Ten seconds unfocused, then back.
            c.frame(t + 0.01, Screen::Live, &s, false, false);
            let back = c.frame(t + 10.0, Screen::Live, &s, true, true);
            let step = 1.0 / rate as f32;
            assert!(back.time - before.time <= step + 1e-4, "{rate}");
            assert!(back.keep > 0.0, "the afterglow is not wiped");
            // Ten seconds with no frame and focus kept: one step at most.
            let late = c.frame(t + 20.0, Screen::Live, &s, true, true);
            assert!(late.time - back.time <= crt::MAX_STEP + 1e-4, "{rate}");
        }
    }

    #[test]
    fn custom_reads_when_a_strength_moved() {
        let mut s = Screen::Live.strengths();
        assert!(!custom(Screen::Live, &s));
        s.bloom = 0.9;
        assert!(custom(Screen::Live, &s));
        assert!(!custom(Screen::Soft, &s));
        assert_eq!(news(Screen::Live, false), "Screen: Live.");
        assert_eq!(news(Screen::Live, true), "Screen: Custom.");
    }

    /// Each strength alone, at Still and at Live: the table of the module's
    /// docs, row by row.
    #[test]
    fn each_strength_asks_for_frames_by_the_table() {
        use Motion::*;
        type Set = fn(&mut Strengths);
        let rows: [(&str, Set, Motion, Motion); 10] = [
            ("bloom", |s| s.bloom = 1.0, Never, Never),
            ("vignette", |s| s.vignette = 1.0, Never, Never),
            ("scanlines", |s| s.scanlines = 1.0, Never, Always),
            ("curvature", |s| s.curvature = 1.0, Never, Never),
            (
                "persistence",
                |s| s.persistence = 1.0,
                AfterChange,
                AfterChange,
            ),
            ("noise", |s| s.noise = 1.0, Always, Always),
            ("flicker", |s| s.flicker = 1.0, Always, Always),
            ("band", |s| s.band = 1.0, Always, Always),
            ("mask", |s| s.mask = 1.0, Never, Never),
            ("aberration", |s| s.aberration = 1.0, Never, Never),
        ];
        for (name, set, still, live) in rows {
            let mut s = Strengths::NONE;
            set(&mut s);
            assert_eq!(motion(Screen::Still, &s), still, "{name} at Still");
            assert_eq!(motion(Screen::Live, &s), live, "{name} at Live");
            assert_eq!(motion(Screen::Arcade, &s), live, "{name} at Arcade");
            for level in [Screen::Off, Screen::Soft] {
                assert_eq!(motion(level, &s), Never, "{name} at {level:?}");
            }
            // The clock agrees: an idle window asks for frames only when
            // the row says always.
            let mut c = Clock::default();
            c.frame(0.0, Screen::Still, &s, true, false);
            let wants = next_frame(animated(Screen::Still, &s), c.glowing(), true, 30);
            assert_eq!(wants.is_some(), still == Always, "{name} idle at Still");
        }
    }

    /// Persistence alone: a change starts frames, which stop once the old
    /// picture is below the stated level, and the slider reaches the pass.
    #[test]
    fn persistence_alone_glows_after_a_change_then_stops() {
        let s = Strengths {
            persistence: 1.0,
            ..Screen::Still.strengths()
        };
        let mut c = Clock::default();
        let idle = c.frame(0.0, Screen::Still, &s, true, false);
        assert!(!c.glowing() && idle.keep == 0.0);
        let first = c.frame(1.0, Screen::Still, &s, true, true);
        assert!(c.glowing(), "a change starts the afterglow");
        assert!(first.keep > 0.0 && first.strengths.persistence == 1.0);
        assert_eq!(first.time, idle.time, "the clock holds");
        let mut t = 1.0;
        let mut left = first.keep;
        let mut frames = 1;
        while c.glowing() {
            t += 1.0 / 30.0;
            let f = c.frame(t, Screen::Still, &s, false, false);
            // Each frame fades the old picture; the last one, below the
            // end level, draws without it.
            assert_eq!(f.keep > 0.0, c.glowing());
            left *= persistence_keep(s.persistence, 1.0 / 30.0);
            frames += 1;
            assert!(frames < 100, "the afterglow ends");
        }
        assert!(left < AFTERGLOW_END, "{left}");
        // At 30 frames a second, under a second.
        assert!(frames <= 30, "{frames}");
        let after = c.frame(t + 1.0, Screen::Still, &s, true, false);
        assert_eq!(after.keep, 0.0);
        assert_eq!(next_frame(false, c.glowing(), true, 30), None);
    }
}
