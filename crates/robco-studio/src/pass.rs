// SPDX-License-Identifier: GPL-3.0-or-later

//! The screen pass: the window's whole picture through a CRT shader.
//!
//! eframe draws egui straight into the window's surface and gives no step
//! between egui's picture and the screen. An egui plugin gives two: its
//! input hook sees the input before egui does, and its output hook sees each
//! pass's output before eframe paints it.
//!
//! - The input hook maps the pointer back through the screen's curve, so a
//!   click lands on what is drawn under it.
//! - The output hook moves each rectangle of the accessibility tree to where
//!   the curve draws it, so a screen reader outlines what is on the glass.
//! - The output hook keeps a copy of each texture change for the pass's own
//!   renderer. At a pass level it then tessellates the shapes, in the order
//!   egui gave them, and puts one paint callback in their place.
//! - In the callback's `prepare`, which egui-wgpu runs before its own render
//!   pass, a second `egui_wgpu::Renderer` draws the shapes into a texture,
//!   and the passes of `screen.wgsl` run on it. The callback's `paint`
//!   draws the result into eframe's pass with one triangle.

use crate::screen::{Frame, map_input, map_tree};
use crate::theme::crt;
use eframe::egui::{
    self, TexturesDelta,
    epaint::{ClippedPrimitive, ClippedShape},
};
use eframe::egui_wgpu::{self, RenderState, ScreenDescriptor, wgpu};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// The shader, for the tests that validate it with no GPU.
pub const WGSL: &str = include_str!("screen.wgsl");

/// The format of the pass's own textures. egui writes sRGB values into it.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// What the window tells the plugin each frame.
#[derive(Debug, Default)]
pub struct Link {
    /// The curvature the pointer is mapped through. Zero but at a pass
    /// level.
    pub curvature: f32,
    /// The frame to draw through the pass, or None to leave egui's output
    /// as it is. The output hook takes it.
    pub frame: Option<Frame>,
    /// The window takes no input: a system dialog is open over it.
    pub inert: bool,
}

/// The window's side of the link, shared with the plugin.
pub type Shared = Arc<Mutex<Link>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The texture changes between egui and the pass's renderer.
///
/// egui gives a frame's changes with its output: the sets are applied
/// before the frame is painted, the frees after, since a texture can be
/// painted in the frame that frees it. eframe frees once the frame's
/// commands are submitted. The pass's renderer gets its turn in `prepare`,
/// before eframe submits, so it frees the textures of a frame at the start
/// of the next frame it draws.
///
/// The newest frame's changes are kept as egui gave them: it may be the
/// frame drawn next. Once a newer frame comes, no frame left to draw
/// paints what the older one freed, so its changes fold into `older`,
/// which stays small however long the pass does not draw:
///
/// - a set and a later free of the same texture cancel out;
/// - a full set replaces every earlier change to its texture, and the
///   partial changes after it stay in order;
/// - a partial change or a free of a texture the renderer never had is
///   dropped.
///
/// The next frame drawn takes all of it at once, frees of the older frames
/// first, then every set, before its picture; its own frees wait for the
/// frame after. Nothing is lost and nothing is applied twice.
#[derive(Default)]
struct Queue {
    /// The folded changes of the frames before the newest.
    older: TexturesDelta,
    /// The newest frame's changes.
    newest: TexturesDelta,
    /// The frees of the last frame drawn, done at the next one.
    freed: Vec<egui::TextureId>,
    /// The textures the renderer has once the steps taken are applied.
    known: HashSet<egui::TextureId>,
}

/// What the renderer does with the textures for one frame, in order.
struct Step {
    /// Free these first: no frame left to draw paints them.
    free: Vec<egui::TextureId>,
    /// Then apply these, before the frame is drawn.
    set: TexturesDelta,
}

impl Queue {
    /// Keep a frame's changes until a frame is drawn.
    fn push(&mut self, delta: &TexturesDelta) {
        let newest = std::mem::take(&mut self.newest);
        self.fold(newest);
        for (id, deltas) in &delta.set {
            for d in deltas {
                self.newest.push(*id, d.clone());
            }
        }
        self.newest.free.extend(delta.free.iter().copied());
    }

    /// Fold the changes of a frame that will not be drawn into `older`.
    fn fold(&mut self, mut d: TexturesDelta) {
        for (id, deltas) in d.set.drain() {
            for x in deltas {
                if x.is_whole() || self.older.set.contains_key(&id) || self.known.contains(&id) {
                    self.older.push(id, x);
                }
            }
        }
        for id in d.free.drain() {
            self.older.set.remove(&id);
            if self.known.contains(&id) {
                self.older.free.insert(id);
            }
        }
    }

    /// The step of the frame drawn now. Its own frees wait for the next.
    fn take(&mut self) -> Step {
        let mut newest = std::mem::take(&mut self.newest);
        let mut set = std::mem::take(&mut self.older);
        let mut free = std::mem::take(&mut self.freed);
        free.extend(set.free.drain());
        for (id, deltas) in newest.set.drain() {
            for d in deltas {
                set.push(id, d);
            }
        }
        self.freed = newest.free.drain().collect();
        for id in &free {
            self.known.remove(id);
        }
        for (id, deltas) in &set.set {
            if deltas.iter().any(egui::epaint::ImageDelta::is_whole) {
                self.known.insert(*id);
            }
        }
        for id in &self.freed {
            self.known.remove(id);
        }
        Step { free, set }
    }

    /// The changes held, counted one per texture set and one per free.
    #[cfg(test)]
    fn len(&self) -> usize {
        let count =
            |d: &TexturesDelta| d.set.values().map(|v| v.len()).sum::<usize>() + d.free.len();
        count(&self.older) + count(&self.newest) + self.freed.len()
    }

    fn clear(&mut self) {
        self.older.clear();
        self.newest.clear();
    }
}

/// Texture changes that the pass's renderer has not taken yet.
#[derive(Default)]
struct Pending(Mutex<Queue>);

impl Drop for Pending {
    fn drop(&mut self) {
        // Changes no frame will draw: egui asks that they are cleared.
        lock(&self.0).clear();
    }
}

/// The egui plugin of the screen. Add it with [`plug`].
struct Plugin {
    link: Shared,
    /// Present when the pass can draw: eframe's device has it.
    pending: Option<Arc<Pending>>,
    /// What egui was last told is down.
    down: Down,
}

/// What egui was told is down: the pointer's buttons, the keys and the
/// touches. When input is held, all of it is released first, so nothing is
/// left pressed while the release itself is held back.
#[derive(Default)]
struct Down {
    buttons: [bool; egui::NUM_POINTER_BUTTONS],
    keys: HashSet<egui::Key>,
    touches: BTreeMap<(egui::TouchDeviceId, egui::TouchId), egui::Pos2>,
    /// Keys released while held that may still be down: their repeats,
    /// the tail of the old press, are dropped. A new press or the real
    /// release ends the mark.
    released: HashSet<egui::Key>,
    /// Touches cancelled while held that may still be on the glass: their
    /// moves and their end are dropped. A new start ends the mark.
    cancelled: BTreeSet<(egui::TouchDeviceId, egui::TouchId)>,
    /// The pointer's last place egui was told.
    pos: Option<egui::Pos2>,
    /// The input was held at the last frame.
    held: bool,
}

impl Down {
    /// Keep what the events of a frame egui sees press and release.
    fn track(&mut self, events: &[egui::Event]) {
        for e in events {
            match e {
                egui::Event::PointerMoved(p) => self.pos = Some(*p),
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } => {
                    self.pos = Some(*pos);
                    self.buttons[*button as usize] = *pressed;
                }
                egui::Event::Key { key, pressed, .. } => {
                    if *pressed {
                        self.keys.insert(*key);
                    } else {
                        self.keys.remove(key);
                    }
                }
                egui::Event::WindowFocused(false) => self.keys.clear(),
                egui::Event::Touch {
                    device_id,
                    id,
                    phase,
                    pos,
                    ..
                } => {
                    if matches!(phase, egui::TouchPhase::Start | egui::TouchPhase::Move) {
                        self.touches.insert((*device_id, *id), *pos);
                    } else {
                        self.touches.remove(&(*device_id, *id));
                    }
                }
                _ => {}
            }
        }
    }

    /// Drop the tail of what was ended while held: the repeats of a key
    /// released, and the moves and end of a touch cancelled. A key press
    /// with no repeat flag and a touch start are new: they pass and end
    /// the mark. A key's real release ends it too and is dropped, since
    /// egui has the key up already. Losing focus ends every mark: the
    /// system owns the keyboard then, and the release may never come.
    fn drop_stale(&mut self, events: &mut Vec<egui::Event>) {
        if self.released.is_empty() && self.cancelled.is_empty() {
            return;
        }
        let (released, cancelled) = (&mut self.released, &mut self.cancelled);
        events.retain(|e| match e {
            egui::Event::Key {
                key,
                pressed,
                repeat,
                ..
            } if released.contains(key) => {
                if *pressed && *repeat {
                    return false;
                }
                released.remove(key);
                *pressed
            }
            egui::Event::Touch {
                device_id,
                id,
                phase,
                ..
            } if cancelled.contains(&(*device_id, *id)) => {
                if *phase == egui::TouchPhase::Move {
                    return false;
                }
                cancelled.remove(&(*device_id, *id));
                *phase == egui::TouchPhase::Start
            }
            egui::Event::WindowFocused(false) => {
                released.clear();
                cancelled.clear();
                true
            }
            _ => true,
        });
    }

    /// The events that end everything down, for the first frame held.
    /// Touches are cancelled and keys released. The pointer moves far off
    /// the window and back, then its buttons are released where it was:
    /// the press counts as moved too far, so egui reports the release with
    /// no click, and a drag under way stops where the pointer last was.
    /// The pointer leaves right after, so the moves make no drag delta.
    fn release(&mut self, screen: egui::Rect) -> Vec<egui::Event> {
        let mut out = Vec::new();
        for ((device_id, id), pos) in std::mem::take(&mut self.touches) {
            self.cancelled.insert((device_id, id));
            out.push(egui::Event::Touch {
                device_id,
                id,
                phase: egui::TouchPhase::Cancel,
                pos,
                force: None,
            });
        }
        for key in std::mem::take(&mut self.keys) {
            self.released.insert(key);
            out.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        if !self.buttons.iter().any(|b| *b) {
            return out;
        }
        let at = self.pos.unwrap_or(screen.min);
        out.push(egui::Event::PointerMoved(
            screen.min - egui::vec2(OFF_WINDOW, OFF_WINDOW),
        ));
        out.push(egui::Event::PointerMoved(at));
        for (i, button) in BUTTONS.into_iter().enumerate() {
            if std::mem::take(&mut self.buttons[i]) {
                out.push(egui::Event::PointerButton {
                    pos: at,
                    button,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
        }
        out
    }
}

/// How far off the window, in points, the pointer moves before a held
/// press is released: far past any click distance.
const OFF_WINDOW: f32 = 10_000.0;

/// Every pointer button, in the order of egui's own table.
const BUTTONS: [egui::PointerButton; egui::NUM_POINTER_BUTTONS] = [
    egui::PointerButton::Primary,
    egui::PointerButton::Secondary,
    egui::PointerButton::Middle,
    egui::PointerButton::Extra1,
    egui::PointerButton::Extra2,
];

impl egui::Plugin for Plugin {
    fn debug_name(&self) -> &'static str {
        "screen pass"
    }

    fn input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        let (curvature, inert) = {
            let link = lock(&self.link);
            (link.curvature, link.inert)
        };
        let rect = input.screen_rect.unwrap_or_else(|| ctx.content_rect());
        map_input(input, rect, curvature);
        if inert {
            let release = if self.down.held {
                Vec::new()
            } else {
                self.down.release(rect)
            };
            // A real release while held ends the wait for it.
            self.down.drop_stale(&mut input.events);
            hold(input, release);
            self.down.held = true;
        } else {
            self.down.drop_stale(&mut input.events);
            self.down.track(&input.events);
            self.down.held = false;
        }
    }

    fn output_hook(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        let (frame, curvature) = {
            let mut link = lock(&self.link);
            (link.frame.take(), link.curvature)
        };
        // The pointer is mapped back through the curve whether or not the
        // pass draws, so the tree's rectangles follow it the same way.
        if let Some(update) = &mut output.platform_output.accesskit_update {
            map_tree(update, ctx.content_rect(), curvature);
        }
        let Some(pending) = &self.pending else {
            return;
        };
        lock(&pending.0).push(&output.textures_delta);
        let Some(frame) = frame else {
            return;
        };
        let shapes = std::mem::take(&mut output.shapes);
        let prims = ctx.tessellate(shapes, output.pixels_per_point);
        let rect = ctx.content_rect();
        let callback = Callback {
            prims,
            frame,
            // Every pass of a frame has the frame's number: a pass that is
            // discarded and run again does not break the afterglow.
            seq: ctx.cumulative_frame_nr(),
            pending: Arc::clone(pending),
        };
        output.shapes = vec![ClippedShape::new(
            rect,
            egui_wgpu::Callback::new_paint_callback(rect, callback),
        )];
    }
}

/// Keep from the window every input a person or a screen reader gives:
/// pointer, keys, text, accessibility actions and files. Every area, the
/// root interface, dialogs, menus, dropdown lists, tooltips, gets the same
/// nothing, and the pointer leaves, so no hover or tooltip stays. Focus still
/// passes, and the modifiers read as none. `release` ends what was down; it
/// comes before the pointer leaves.
fn hold(input: &mut egui::RawInput, release: Vec<egui::Event>) {
    input
        .events
        .retain(|e| matches!(e, egui::Event::WindowFocused(_)));
    input.events.extend(release);
    input.events.push(egui::Event::PointerGone);
    input
        .events
        .push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    input.hovered_files.clear();
    input.dropped_files.clear();
}

/// Add the screen's plugin to `ctx`, linked to the window by `link`. With
/// eframe's render state, the pass is made on its device and the plugin
/// draws through it. Add it before the first frame, so the pass's renderer
/// sees every texture from the start.
///
/// This is the only way in, and only the first call for a context counts:
/// a later one, from the program or a test, changes nothing. egui keeps one
/// plugin of a type, but a second pass would still replace the first in
/// eframe's resources and lose its textures, so the context is asked first.
pub fn plug(ctx: &egui::Context, link: Shared, gpu: Option<&RenderState>) {
    if ctx.plugin_opt::<Plugin>().is_some() {
        return;
    }
    let pending = gpu.map(|rs| {
        let pass = Pass::new(&rs.device, rs.target_format);
        rs.renderer.write().callback_resources.insert(pass);
        Arc::new(Pending::default())
    });
    ctx.add_plugin(Plugin {
        link,
        pending,
        down: Down::default(),
    });
}

/// The paint callback that carries one frame's picture.
struct Callback {
    prims: Vec<ClippedPrimitive>,
    frame: Frame,
    /// The number of the frame, as egui counts them.
    seq: u64,
    pending: Arc<Pending>,
}

impl egui_wgpu::CallbackTrait for Callback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sd: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(pass) = res.get_mut::<Pass>() else {
            return Vec::new();
        };
        let step = lock(&self.pending.0).take();
        pass.textures(device, queue, step);
        pass.encode(
            device,
            queue,
            encoder,
            &self.prims,
            sd,
            &self.frame,
            self.seq,
        )
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        rp: &mut wgpu::RenderPass<'static>,
        res: &egui_wgpu::CallbackResources,
    ) {
        if let Some(pass) = res.get::<Pass>() {
            pass.blit(rp);
        }
    }
}

/// The textures the passes draw into, for one window size.
struct Targets {
    size: [u32; 2],
    scene: wgpu::TextureView,
    half_a: wgpu::TextureView,
    half_b: wgpu::TextureView,
    out: [wgpu::TextureView; 2],
}

/// The pass's GPU side: its own egui renderer, the pipelines and the
/// targets. It lives in eframe's callback resources.
struct Pass {
    renderer: egui_wgpu::Renderer,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bright: wgpu::RenderPipeline,
    blur_h: wgpu::RenderPipeline,
    blur_v: wgpu::RenderPipeline,
    comp: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    targets: Option<Targets>,
    /// Which of the two outputs holds the last frame.
    cur: usize,
    /// The output the pass drew last, and at which display scale.
    drawn: Option<(u64, f32)>,
    blit_group: Option<wgpu::BindGroup>,
}

/// The uniform block: four vectors of four, as `P` in the shader.
const UNIFORM_FLOATS: usize = 16;

impl Pass {
    fn new(device: &wgpu::Device, window_format: wgpu::TextureFormat) -> Pass {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("screen pass"),
            source: wgpu::ShaderSource::Wgsl(WGSL.into()),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen pass"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex(2),
                tex(3),
                tex(4),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("screen pass"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipe = |entry: &str, format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let blit_entry = if window_format.is_srgb() {
            "fs_blit_linear"
        } else {
            "fs_blit"
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("screen pass"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen pass"),
            size: (UNIFORM_FLOATS * size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Pass {
            renderer: egui_wgpu::Renderer::new(device, FORMAT, Default::default()),
            bright: pipe("fs_bright", FORMAT),
            blur_h: pipe("fs_blur_h", FORMAT),
            blur_v: pipe("fs_blur_v", FORMAT),
            comp: pipe("fs_comp", FORMAT),
            blit: pipe(blit_entry, window_format),
            layout,
            sampler,
            uniforms,
            targets: None,
            cur: 0,
            drawn: None,
            blit_group: None,
        }
    }

    /// Apply one frame's step of texture changes, before it is drawn. A
    /// change to part of a texture the renderer never had is skipped: it
    /// can only come before the pass was plugged in.
    fn textures(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, mut step: Step) {
        for id in &step.free {
            self.renderer.free_texture(id);
        }
        for (id, deltas) in step.set.set.drain() {
            for d in deltas {
                if d.pos.is_none() || self.renderer.texture(&id).is_some() {
                    self.renderer.update_texture(device, queue, id, &d);
                }
            }
        }
    }

    /// Make the targets for `size` unless they are made. True when made.
    fn targets(&mut self, device: &wgpu::Device, size: [u32; 2]) -> bool {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return false;
        }
        let make = |w: u32, h: u32| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("screen pass"),
                    size: wgpu::Extent3d {
                        width: w.max(1),
                        height: h.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let [w, h] = size;
        self.targets = Some(Targets {
            size,
            scene: make(w, h),
            half_a: make(w / 2, h / 2),
            half_b: make(w / 2, h / 2),
            out: [make(w, h), make(w, h)],
        });
        true
    }

    fn group(
        &self,
        device: &wgpu::Device,
        a: &wgpu::TextureView,
        b: &wgpu::TextureView,
        c: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let view = |binding, v| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(v),
        };
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("screen pass"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                view(2, a),
                view(3, b),
                view(4, c),
            ],
        })
    }

    fn run(
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        group: &wgpu::BindGroup,
    ) {
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("screen pass"),
            color_attachments: &[Some(attachment(target))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(pipeline);
        rp.set_bind_group(0, group, &[]);
        rp.draw(0..3, 0..1);
    }

    /// Draw egui's picture into the scene texture and run the passes.
    ///
    /// The last output is the afterglow's history only when the pass drew
    /// the frame just before this one, number `seq`, at this size and display
    /// scale. After any frame it did not draw, a level without the pass,
    /// a frame dropped, new targets, a new scale or the first frame, the
    /// history is empty and the frame keeps nothing of it.
    #[allow(clippy::too_many_arguments)]
    fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        prims: &[ClippedPrimitive],
        sd: &ScreenDescriptor,
        frame: &Frame,
        seq: u64,
    ) -> Vec<wgpu::CommandBuffer> {
        let made = self.targets(device, sd.size_in_pixels);
        let follows = self.drawn == Some((seq.wrapping_sub(1), sd.pixels_per_point));
        self.drawn = Some((seq, sd.pixels_per_point));
        let frame = &Frame {
            keep: if follows && !made { frame.keep } else { 0.0 },
            ..*frame
        };
        let cmds = self
            .renderer
            .update_buffers(device, queue, encoder, prims, sd);
        let Some(t) = self.targets.as_ref() else {
            return cmds;
        };
        {
            let rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screen pass: egui"),
                color_attachments: &[Some(attachment(&t.scene))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer.render(&mut rp.forget_lifetime(), prims, sd);
        }
        let bytes: Vec<u8> = uniforms(sd, frame)
            .iter()
            .flat_map(|x| x.to_ne_bytes())
            .collect();
        queue.write_buffer(&self.uniforms, 0, &bytes);
        let next = 1 - self.cur;
        if frame.strengths.bloom > 0.0 {
            let g = self.group(device, &t.scene, &t.scene, &t.scene);
            Self::run(encoder, &t.half_a, &self.bright, &g);
            let g = self.group(device, &t.half_a, &t.half_a, &t.half_a);
            Self::run(encoder, &t.half_b, &self.blur_h, &g);
            let g = self.group(device, &t.half_b, &t.half_b, &t.half_b);
            Self::run(encoder, &t.half_a, &self.blur_v, &g);
        }
        let g = self.group(device, &t.scene, &t.half_a, &t.out[self.cur]);
        Self::run(encoder, &t.out[next], &self.comp, &g);
        self.blit_group = Some(self.group(device, &t.out[next], &t.out[next], &t.out[next]));
        self.cur = next;
        cmds
    }

    fn blit(&self, rp: &mut wgpu::RenderPass<'static>) {
        if let Some(g) = &self.blit_group {
            rp.set_pipeline(&self.blit);
            rp.set_bind_group(0, g, &[]);
            rp.draw(0..3, 0..1);
        }
    }
}

fn attachment(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
        },
        depth_slice: None,
    }
}

/// The uniform block of one frame, as `P` in the shader.
fn uniforms(sd: &ScreenDescriptor, f: &Frame) -> [f32; UNIFORM_FLOATS] {
    let s = &f.strengths;
    let [w, h] = sd.size_in_pixels;
    [
        w as f32,
        h as f32,
        f.time,
        sd.pixels_per_point,
        s.bloom,
        s.vignette,
        s.scanlines,
        s.curvature * crt::CURVE,
        f.keep,
        s.noise,
        s.flicker,
        s.mask,
        s.aberration,
        s.band,
        f.scan_offset,
        0.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What one frame showed of a widget that senses clicks and drags.
    #[derive(Debug, Default)]
    struct Seen {
        clicked: bool,
        any_click: bool,
        down: bool,
        dragged: bool,
        delta: egui::Vec2,
        stopped_at: Option<egui::Pos2>,
        a_pressed: bool,
        a_down: bool,
    }

    /// A window with one widget over its top left quarter, held or not.
    fn frame(ctx: &egui::Context, link: &Shared, held: bool, events: Vec<egui::Event>) -> Seen {
        lock(link).inert = held;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            events,
            ..Default::default()
        };
        let mut seen = Seen::default();
        let mut out = ctx.run_ui(input, |ui| {
            let r = ui.allocate_rect(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 200.0)),
                egui::Sense::click_and_drag(),
            );
            seen.clicked = r.clicked();
            seen.dragged = r.dragged();
            seen.delta = r.drag_delta();
            seen.stopped_at = r.drag_stopped().then(|| r.interact_pointer_pos()).flatten();
            ui.input(|i| {
                seen.any_click = i.pointer.any_click();
                seen.down = i.pointer.any_down() || i.pointer.press_origin().is_some();
                seen.a_pressed = i.key_pressed(egui::Key::A);
                seen.a_down = i.key_down(egui::Key::A);
            });
        });
        out.textures_delta.clear();
        seen
    }

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn key_a(pressed: bool, repeat: bool) -> egui::Event {
        egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed,
            repeat,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// A press and a key down when the input is held end in egui while
    /// it is held: no click, no drag, nothing left down, and the key's
    /// repeats after the hold do nothing until a new press.
    #[test]
    fn a_hold_ends_what_is_down_with_no_click() {
        for drag_first in [false, true] {
            let ctx = egui::Context::default();
            let link = Shared::default();
            plug(&ctx, link.clone(), None);
            let p = egui::pos2(50.0, 50.0);
            frame(&ctx, &link, false, vec![egui::Event::PointerMoved(p)]);
            frame(
                &ctx,
                &link,
                false,
                vec![button(p, true), key_a(true, false)],
            );
            let mut at = p;
            if drag_first {
                at = egui::pos2(120.0, 50.0);
                let f = frame(&ctx, &link, false, vec![egui::Event::PointerMoved(at)]);
                assert!(f.dragged, "the drag is under way");
            }
            let mut seen = Vec::new();
            seen.push(frame(&ctx, &link, true, vec![]));
            seen.push(frame(
                &ctx,
                &link,
                true,
                vec![button(at, false), key_a(true, true)],
            ));
            seen.push(frame(&ctx, &link, true, vec![]));
            let q = egui::pos2(150.0, 150.0);
            seen.push(frame(
                &ctx,
                &link,
                false,
                vec![egui::Event::PointerMoved(q)],
            ));
            seen.push(frame(&ctx, &link, false, vec![key_a(true, true)]));
            for (i, f) in seen.iter().enumerate() {
                let tag = format!("drag {drag_first}, frame {i}: {f:?}");
                assert!(!f.clicked && !f.any_click, "{tag}");
                assert!(!f.dragged || i == 0, "{tag}");
                assert_eq!(f.delta, egui::Vec2::ZERO, "{tag}");
                assert!(f.stopped_at.is_none_or(|s| s == at), "{tag}");
                assert!(!f.down && !f.a_pressed && !f.a_down, "{tag}");
            }
            assert_eq!(
                seen.iter().filter(|f| f.stopped_at.is_some()).count(),
                usize::from(drag_first),
                "a drag under way stops once, where it was"
            );
            frame(&ctx, &link, false, vec![key_a(false, false)]);
            let f = frame(&ctx, &link, false, vec![key_a(true, false)]);
            assert!(f.a_pressed, "a new press acts");
        }
    }

    /// A key held over a hold whose real release never comes: its repeats
    /// after the hold do nothing, and one new press acts once. Losing
    /// focus ends the wait too.
    #[test]
    fn a_new_press_after_a_hold_acts_at_once() {
        for lose_focus in [false, true] {
            let ctx = egui::Context::default();
            let link = Shared::default();
            plug(&ctx, link.clone(), None);
            frame(&ctx, &link, false, vec![key_a(true, false)]);
            frame(&ctx, &link, true, vec![]);
            frame(&ctx, &link, true, vec![]);
            if lose_focus {
                frame(&ctx, &link, false, vec![egui::Event::WindowFocused(false)]);
                frame(&ctx, &link, false, vec![egui::Event::WindowFocused(true)]);
            } else {
                for _ in 0..3 {
                    let f = frame(&ctx, &link, false, vec![key_a(true, true)]);
                    assert!(!f.a_pressed && !f.a_down, "a repeat acted");
                }
            }
            let f = frame(&ctx, &link, false, vec![key_a(true, false)]);
            assert!(
                f.a_pressed,
                "focus lost {lose_focus}: the new press is dropped"
            );
            let f = frame(&ctx, &link, false, vec![key_a(true, true)]);
            assert!(f.a_pressed, "the new press's repeat acts");
            frame(&ctx, &link, false, vec![key_a(false, false)]);
            let f = frame(&ctx, &link, false, vec![key_a(true, false)]);
            assert!(f.a_pressed, "the next press acts");
        }
    }

    /// A touch cancelled by a hold: its later moves and end are dropped,
    /// and a new touch with the same id passes.
    #[test]
    fn a_touch_cancelled_by_a_hold_drops_its_tail() {
        let touch = |phase| egui::Event::Touch {
            device_id: egui::TouchDeviceId(1),
            id: egui::TouchId(7),
            phase,
            pos: egui::pos2(10.0, 10.0),
            force: None,
        };
        let mut d = Down::default();
        d.track(&[touch(egui::TouchPhase::Start)]);
        let out = d.release(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(100.0, 100.0),
        ));
        assert_eq!(out.len(), 1);
        let mut tail = vec![touch(egui::TouchPhase::Move), touch(egui::TouchPhase::End)];
        d.drop_stale(&mut tail);
        assert!(tail.is_empty(), "{tail:?}");
        let mut new = vec![
            touch(egui::TouchPhase::Start),
            touch(egui::TouchPhase::Move),
        ];
        let mut d2 = Down::default();
        d2.track(&[touch(egui::TouchPhase::Start)]);
        d2.release(egui::Rect::NOTHING);
        d2.drop_stale(&mut new);
        assert_eq!(new.len(), 2, "a new touch is dropped");
    }

    /// One frame of a few buttons, with or without the plugin at
    /// `curvature`, and its accessibility tree update.
    fn tree(curvature: Option<f32>) -> Option<egui::accesskit::TreeUpdate> {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        if let Some(c) = curvature {
            let link = Shared::default();
            lock(&link).curvature = c;
            plug(&ctx, link, None);
        }
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            let _ = ui.button("Top left");
            ui.add_space(500.0);
            let _ = ui.button("Lower");
        });
        out.textures_delta.clear();
        out.platform_output.accesskit_update
    }

    /// With no curvature the tree is the tree without the pass, field for
    /// field. With curvature it differs.
    #[test]
    fn the_tree_is_untouched_without_curvature() {
        let plain = tree(None);
        assert!(plain.is_some());
        assert_eq!(tree(Some(0.0)), plain);
        assert_ne!(tree(Some(0.4)), plain);
    }

    /// The shader parses and validates with naga, the front end wgpu uses.
    /// No GPU.
    #[test]
    fn the_shader_validates() {
        let module = naga::front::wgsl::parse_str(WGSL).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap();
        for e in [
            "vs",
            "fs_bright",
            "fs_blur_h",
            "fs_blur_v",
            "fs_comp",
            "fs_blit",
            "fs_blit_linear",
        ] {
            assert!(module.entry_points.iter().any(|p| p.name == e), "{e}");
        }
    }

    fn id(n: u64) -> egui::TextureId {
        egui::TextureId::Managed(n)
    }

    fn pixel() -> egui::ColorImage {
        egui::ColorImage::filled([1, 1], egui::Color32::RED)
    }

    fn full() -> egui::epaint::ImageDelta {
        egui::epaint::ImageDelta::full(pixel(), Default::default())
    }

    fn part(x: usize) -> egui::epaint::ImageDelta {
        egui::epaint::ImageDelta::partial([x, 0], pixel(), Default::default())
    }

    /// One frame's changes: these sets in order, then these frees.
    fn delta(set: Vec<(u64, egui::epaint::ImageDelta)>, free: &[u64]) -> TexturesDelta {
        let mut d = TexturesDelta::default();
        for (n, x) in set {
            d.push(id(n), x);
        }
        d.free.extend(free.iter().map(|n| id(*n)));
        d
    }

    fn push(q: &mut Queue, set: Vec<(u64, egui::epaint::ImageDelta)>, free: &[u64]) {
        let mut d = delta(set, free);
        q.push(&d);
        d.clear();
    }

    fn sorted(mut v: Vec<egui::TextureId>) -> Vec<egui::TextureId> {
        v.sort();
        v
    }

    /// The ids a step sets, and the place of each change: None for a full set.
    fn sets(step: &mut Step) -> Vec<(egui::TextureId, Vec<Option<[usize; 2]>>)> {
        let mut v: Vec<_> = step
            .set
            .set
            .drain()
            .map(|(i, d)| (i, d.iter().map(|x| x.pos).collect()))
            .collect();
        v.sort_by_key(|(i, _)| *i);
        step.set.clear();
        v
    }

    /// The renderer's order: a frame's sets before it, its frees at the
    /// next frame drawn, and the frees of frames before it first.
    #[test]
    fn the_queue_sets_before_and_frees_after() {
        let mut q = Queue::default();
        // Frame one makes A and B, paints A and drops it: it is drawn.
        push(&mut q, vec![(1, full()), (2, full())], &[1]);
        let mut step = q.take();
        assert!(step.free.is_empty(), "nothing freed before the frame");
        assert_eq!(sets(&mut step), [(id(1), vec![None]), (id(2), vec![None])]);
        // Frame two drops B and is not drawn; frame three is.
        push(&mut q, vec![], &[2]);
        push(&mut q, vec![(3, full())], &[]);
        let mut step = q.take();
        assert_eq!(sorted(step.free.clone()), [id(1), id(2)]);
        assert_eq!(sets(&mut step), [(id(3), vec![None])]);
        assert!(q.take().free.is_empty());
    }

    /// A set and a later free of one texture, in frames not drawn, cancel.
    #[test]
    fn the_queue_cancels_a_set_and_its_free() {
        let mut q = Queue::default();
        push(&mut q, vec![(1, full()), (1, part(2))], &[]);
        push(&mut q, vec![(5, part(1))], &[1]);
        push(&mut q, vec![], &[]);
        assert_eq!(q.len(), 0);
        let mut step = q.take();
        assert!(step.free.is_empty());
        assert!(sets(&mut step).is_empty());
    }

    /// A free stays when the texture made it to the renderer, and the
    /// changes to it that wait are dropped.
    #[test]
    fn the_queue_keeps_the_free_of_a_texture_drawn() {
        let mut q = Queue::default();
        push(&mut q, vec![(1, full())], &[]);
        let _ = sets(&mut q.take());
        push(&mut q, vec![(1, part(3))], &[]);
        push(&mut q, vec![], &[1]);
        push(&mut q, vec![], &[]);
        let mut step = q.take();
        assert_eq!(step.free, [id(1)]);
        assert!(sets(&mut step).is_empty());
    }

    /// A later full set replaces the earlier changes, and the partial
    /// changes after it stay, in order.
    #[test]
    fn the_queue_keeps_the_last_full_set_and_what_follows() {
        let mut q = Queue::default();
        push(&mut q, vec![(0, full()), (0, part(1))], &[]);
        push(&mut q, vec![(0, part(2))], &[]);
        push(&mut q, vec![(0, full()), (0, part(3))], &[]);
        push(&mut q, vec![(0, part(4))], &[]);
        push(&mut q, vec![], &[]);
        let mut step = q.take();
        assert_eq!(
            sets(&mut step),
            [(id(0), vec![None, Some([3, 0]), Some([4, 0])])]
        );
    }

    /// Partial changes and frees of a texture the renderer never had are
    /// dropped; partial changes of one it has are kept.
    #[test]
    fn the_queue_drops_what_the_renderer_never_had() {
        let mut q = Queue::default();
        push(&mut q, vec![(0, full())], &[]);
        let _ = sets(&mut q.take());
        push(&mut q, vec![(0, part(1)), (7, part(1))], &[8]);
        push(&mut q, vec![], &[]);
        assert_eq!(q.len(), 1);
        let mut step = q.take();
        assert!(step.free.is_empty());
        assert_eq!(sets(&mut step), [(id(0), vec![Some([1, 0])])]);
    }

    /// An hour of level switches at Off, one a second: each makes the
    /// drawings anew under new ids and drops the old, and the font atlas
    /// grows now and then. The queue holds the live drawings and the
    /// atlas, never what was dropped.
    #[test]
    fn the_queue_stays_small_while_the_pass_is_off() {
        const ART: u64 = 6;
        let mut q = Queue::default();
        // The pass drew at Live once: the atlas and a first set of art.
        let first: Vec<_> = (0..=ART).map(|n| (n, full())).collect();
        push(&mut q, first, &[]);
        let _ = sets(&mut q.take());
        let mut most = 0;
        for switch in 0..3600_u64 {
            let old = 1 + (switch * ART)..1 + ((switch + 1) * ART);
            let new: Vec<_> = old.clone().map(|n| (n + ART, full())).collect();
            let mut set = new;
            if switch % 10 == 0 {
                set.push((0, part(switch as usize)));
            }
            let free: Vec<u64> = old.collect();
            push(&mut q, set, &free);
            push(&mut q, vec![], &[]);
            most = most.max(q.len());
        }
        // The first art's frees, the live art and the atlas's partials:
        // 6 + 6 + 360. Without the rules it would be 3600 * 6 sets.
        assert!(most <= 2 * ART as usize + 360, "{most}");
        let mut step = q.take();
        assert_eq!(step.free.len(), ART as usize);
        let set = sets(&mut step);
        assert_eq!(set.len(), 1 + ART as usize);
    }

    /// Plugged twice, the second time with another link: a click on a
    /// button where the curve draws it still lands, and the button's
    /// rectangle in the tree is mapped once.
    #[test]
    fn a_second_plug_changes_nothing() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
        let run = |twice: bool| {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let link = Shared::default();
            lock(&link).curvature = 0.4;
            plug(&ctx, link.clone(), None);
            if twice {
                plug(&ctx, link.clone(), None);
                plug(&ctx, Shared::default(), None);
            }
            let rect = std::cell::Cell::new(egui::Rect::NOTHING);
            let clicked = std::cell::Cell::new(0);
            let frame = |events: Vec<egui::Event>| {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| {
                    ui.add_space(20.0);
                    let r = ui.button("Corner");
                    rect.set(r.rect);
                    clicked.set(clicked.get() + usize::from(r.clicked()));
                });
                out.textures_delta.clear();
                out.platform_output.accesskit_update
            };
            let tree = frame(Vec::new());
            let at = crate::screen::drawn_at(rect.get().center(), screen, 0.4);
            let press = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(vec![egui::Event::PointerMoved(at)]);
            frame(vec![press(true)]);
            frame(vec![press(false)]);
            (tree, clicked.get())
        };
        let (once, clicks) = run(false);
        assert_eq!(clicks, 1);
        assert_eq!(run(true), (once, 1));
    }

    /// The uniform block puts each strength where the shader reads it.
    #[test]
    fn the_uniforms_follow_the_shader() {
        let f = Frame {
            strengths: crate::theme::Screen::Arcade.strengths(),
            time: 2.0,
            scan_offset: 1.0,
            keep: 0.0,
        };
        let sd = ScreenDescriptor {
            size_in_pixels: [1280, 720],
            pixels_per_point: 2.0,
        };
        let u = uniforms(&sd, &f);
        let s = f.strengths;
        assert_eq!(u[..4], [1280.0, 720.0, 2.0, 2.0]);
        assert_eq!(
            u[4..8],
            [s.bloom, s.vignette, s.scanlines, s.curvature * crt::CURVE]
        );
        assert_eq!(u[8..12], [0.0, s.noise, s.flicker, s.mask]);
        assert_eq!(u[12..], [s.aberration, s.band, 1.0, 0.0]);
    }
}
