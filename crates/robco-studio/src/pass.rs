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
}

/// The window's side of the link, shared with the plugin.
pub type Shared = Arc<Mutex<Link>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Texture changes that the pass's renderer has not taken yet.
#[derive(Default)]
struct Pending(Mutex<TexturesDelta>);

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
}

impl egui::Plugin for Plugin {
    fn debug_name(&self) -> &'static str {
        "screen pass"
    }

    fn input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        let curvature = lock(&self.link).curvature;
        let rect = input.screen_rect.unwrap_or_else(|| ctx.content_rect());
        map_input(input, rect, curvature);
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
        {
            let mut p = lock(&pending.0);
            for (id, deltas) in &output.textures_delta.set {
                for d in deltas {
                    p.push(*id, d.clone());
                }
            }
            for id in &output.textures_delta.free {
                p.free(*id);
            }
        }
        let Some(frame) = frame else {
            return;
        };
        let shapes = std::mem::take(&mut output.shapes);
        let prims = ctx.tessellate(shapes, output.pixels_per_point);
        let rect = ctx.content_rect();
        let callback = Callback {
            prims,
            frame,
            pending: Arc::clone(pending),
        };
        output.shapes = vec![ClippedShape::new(
            rect,
            egui_wgpu::Callback::new_paint_callback(rect, callback),
        )];
    }
}

/// Add the screen's plugin to `ctx`, linked to the window by `link`. With
/// eframe's render state, the pass is made on its device and the plugin
/// draws through it. Add it before the first frame, so the pass's renderer
/// sees every texture from the start.
pub fn plug(ctx: &egui::Context, link: Shared, gpu: Option<&RenderState>) {
    let pending = gpu.map(|rs| {
        let pass = Pass::new(&rs.device, rs.target_format);
        rs.renderer.write().callback_resources.insert(pass);
        Arc::new(Pending::default())
    });
    ctx.add_plugin(Plugin { link, pending });
}

/// The paint callback that carries one frame's picture.
struct Callback {
    prims: Vec<ClippedPrimitive>,
    frame: Frame,
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
        let mut delta = std::mem::take(&mut *lock(&self.pending.0));
        pass.textures(device, queue, &mut delta);
        pass.encode(device, queue, encoder, &self.prims, sd, &self.frame)
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
            blit_group: None,
        }
    }

    /// Apply the texture changes egui gave since the last frame drawn. A
    /// change to part of a texture the renderer never had is skipped: it
    /// can only come before the pass was plugged in.
    fn textures(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, delta: &mut TexturesDelta) {
        for (id, deltas) in delta.set.drain() {
            for d in deltas {
                if d.pos.is_none() || self.renderer.texture(&id).is_some() {
                    self.renderer.update_texture(device, queue, id, &d);
                }
            }
        }
        for id in delta.free.drain() {
            self.renderer.free_texture(&id);
        }
    }

    fn targets(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
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
    fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        prims: &[ClippedPrimitive],
        sd: &ScreenDescriptor,
        frame: &Frame,
    ) -> Vec<wgpu::CommandBuffer> {
        self.targets(device, sd.size_in_pixels);
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
