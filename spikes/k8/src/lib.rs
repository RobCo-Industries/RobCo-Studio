//! Spike K8: the whole egui picture through animated CRT passes.
//!
//! `stand_in` draws a stand-in for the Studio window. `Crt` renders egui's
//! tessellated picture into its own texture and runs the passes in
//! `crt.wgsl`. `warp` is the barrel curve, shared with the pointer mapping.
//! `overlay` is plan B: moving quads on egui's top layer, no post-process.

use eframe::egui::{
    self, Color32, ColorImage, FontId, Pos2, Rect, RichText, TextureHandle, TextureId,
    TextureOptions, Ui, UiBuilder, epaint::ClippedPrimitive, epaint::ImageDelta, vec2,
};
use eframe::egui_wgpu::{self, ScreenDescriptor, wgpu};

pub const WGSL: &str = include_str!("crt.wgsl");

pub const GREEN: Color32 = Color32::from_rgb(0x33, 0xff, 0x66);
pub const GROUND: Color32 = Color32::from_rgb(0x05, 0x12, 0x08);
pub const PANE: Color32 = Color32::from_rgb(0x0a, 0x22, 0x10);

/// Effect strengths. Zero turns an effect off; one is the strong end.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Params {
    pub bloom: f32,
    pub vignette: f32,
    pub scanlines: f32,
    pub curvature: f32,
    pub persistence: f32,
    pub noise: f32,
    pub flicker: f32,
    pub mask: f32,
    pub aberration: f32,
    pub band: f32,
}

impl Params {
    pub const OFF: Params = Params {
        bloom: 0.0,
        vignette: 0.0,
        scanlines: 0.0,
        curvature: 0.0,
        persistence: 0.0,
        noise: 0.0,
        flicker: 0.0,
        mask: 0.0,
        aberration: 0.0,
        band: 0.0,
    };
    /// The strength that looked right to the spike, for code read for hours.
    pub const TASTEFUL: Params = Params {
        bloom: 0.35,
        vignette: 0.5,
        scanlines: 0.3,
        curvature: 0.15,
        persistence: 0.5,
        noise: 0.15,
        flicker: 0.3,
        mask: 0.0,
        aberration: 0.0,
        band: 0.4,
    };
    /// An arcade cabinet setting.
    pub const STRONG: Params = Params {
        bloom: 1.0,
        vignette: 1.0,
        scanlines: 1.0,
        curvature: 1.0,
        persistence: 1.0,
        noise: 1.0,
        flicker: 1.0,
        mask: 1.0,
        aberration: 1.0,
        band: 1.0,
    };

    pub fn named() -> Vec<(&'static str, Params)> {
        let one = |f: fn(&mut Params)| {
            let mut p = Params::OFF;
            f(&mut p);
            p
        };
        vec![
            ("bloom", one(|p| p.bloom = 0.6)),
            ("vignette", one(|p| p.vignette = 0.7)),
            ("scanlines", one(|p| p.scanlines = 0.5)),
            ("curvature", one(|p| p.curvature = 0.5)),
            ("persistence", one(|p| p.persistence = 0.8)),
            ("noise", one(|p| p.noise = 0.6)),
            ("flicker", one(|p| p.flicker = 1.0)),
            ("mask", one(|p| p.mask = 0.6)),
            ("aberration", one(|p| p.aberration = 0.6)),
            ("band", one(|p| p.band = 1.0)),
        ]
    }

    pub fn animated(&self) -> bool {
        *self != Params::OFF
    }

    fn bytes(&self, w: u32, h: u32, time: f32, ppp: f32) -> Vec<u8> {
        let f = [
            w as f32,
            h as f32,
            time,
            ppp,
            self.bloom,
            self.vignette,
            self.scanlines,
            self.curvature,
            self.persistence,
            self.noise,
            self.flicker,
            self.mask,
            self.aberration,
            self.band,
            0.0,
            0.0,
        ];
        f.iter().flat_map(|x| x.to_ne_bytes()).collect()
    }
}

/// The barrel curve, the same formula as `curve` in `crt.wgsl`. It maps a
/// position on the glass, in 0..1, to the position in egui's picture that is
/// drawn there. So a pointer at a glass position is sent to egui at `warp`.
pub fn warp(uv: [f32; 2], curvature: f32) -> [f32; 2] {
    let k = curvature * 0.12;
    let (x, y) = (uv[0] * 2.0 - 1.0, uv[1] * 2.0 - 1.0);
    let (x2, y2) = (x * (1.0 + k * y * y), y * (1.0 + k * x * x));
    [x2 * 0.5 + 0.5, y2 * 0.5 + 0.5]
}

/// Map a pointer position on the glass, in points, to egui's picture.
pub fn map_pointer(pos: Pos2, screen: Rect, curvature: f32) -> Pos2 {
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

/// Where on the glass a point of egui's picture is drawn: the inverse of
/// `map_pointer`, found by fixed-point steps.
pub fn drawn_at(pos: Pos2, screen: Rect, curvature: f32) -> Pos2 {
    let mut s = pos;
    for _ in 0..50 {
        let m = map_pointer(s, screen, curvature);
        s += pos - m;
    }
    s
}

/// Map every pointer position in the input, for the curved screen.
pub fn map_input(raw: &mut egui::RawInput, curvature: f32) {
    let Some(screen) = raw.screen_rect else {
        return;
    };
    if curvature == 0.0 {
        return;
    }
    for e in &mut raw.events {
        match e {
            egui::Event::PointerMoved(p) => *p = map_pointer(*p, screen, curvature),
            egui::Event::PointerButton { pos, .. } => *pos = map_pointer(*pos, screen, curvature),
            egui::Event::Touch { pos, .. } => *pos = map_pointer(*pos, screen, curvature),
            _ => {}
        }
    }
}

const CODE: &str = "static int sum(const int *v, int n)\n{\n    int s = 0;\n    for (int i = 0; i < n; i++) {\n        s += v[i];\n    }\n    return s;\n}\n\nint main(void)\n{\n    int v[4] = { 1, 2, 3, 4 };\n    return sum(v, 4) == 10 ? 0 : 1;\n}";

/// What the stand-in reports back: the first button's rect and whether it was clicked.
pub struct Drawn {
    pub button: Rect,
    pub clicked: bool,
}

/// The stand-in window: dark green ground, two panes, caps labels, code text,
/// filled buttons, and a block that moves so that persistence shows.
pub fn stand_in(ui: &mut Ui, t: f64) -> Drawn {
    let full = ui.max_rect();
    {
        let v = &mut ui.style_mut().visuals;
        v.override_text_color = Some(GREEN);
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
        ] {
            w.weak_bg_fill = GREEN;
            w.bg_fill = GREEN;
        }
    }
    let p = ui.painter().clone();
    p.rect_filled(full, 0.0, GROUND);
    let stroke = egui::Stroke::new(1.0, Color32::from_rgb(0x1a, 0x66, 0x30));
    let left = Rect::from_min_size(full.min + vec2(8.0, 8.0), vec2(200.0, full.height() - 16.0));
    let right = Rect::from_min_max(
        Pos2::new(left.max.x + 8.0, full.min.y + 8.0),
        full.max - vec2(8.0, 8.0),
    );
    for r in [left, right] {
        p.rect_filled(r, 2.0, PANE);
        p.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
    }
    ui.scope_builder(UiBuilder::new().max_rect(left.shrink(12.0)), |ui| {
        ui.label(RichText::new("LABEL").strong().size(15.0));
        ui.add_space(6.0);
        for i in 1..=6 {
            ui.label(format!("LABEL {i}"));
        }
    });
    let mut drawn = Drawn {
        button: Rect::NOTHING,
        clicked: false,
    };
    ui.scope_builder(UiBuilder::new().max_rect(right.shrink(12.0)), |ui| {
        ui.label(RichText::new("SAMPLE").size(22.0));
        ui.add_space(6.0);
        ui.label(RichText::new(CODE).font(FontId::monospace(13.0)));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let b = ui.add(egui::Button::new(RichText::new("SAMPLE").color(GROUND)).fill(GREEN));
            drawn.button = b.rect;
            drawn.clicked = b.clicked();
            for _ in 0..2 {
                ui.add(egui::Button::new(RichText::new("LABEL").color(GROUND)).fill(GREEN));
            }
        });
    });
    // A block that moves, like a caret or a progress mark.
    let x = right.min.x + 12.0 + ((t * 90.0) % 300.0) as f32;
    p.rect_filled(
        Rect::from_min_size(Pos2::new(x, right.max.y - 30.0), vec2(10.0, 16.0)),
        0.0,
        GREEN,
    );
    drawn
}

/// Plan B: scanlines, a rolling band and flicker as quads on egui's top layer.
pub struct Overlay {
    tex: TextureHandle,
}

impl Overlay {
    pub fn new(ctx: &egui::Context) -> Self {
        // Three rows: clear, clear, dark. Repeated over the screen.
        let img = ColorImage::new(
            [1, 3],
            vec![
                Color32::TRANSPARENT,
                Color32::TRANSPARENT,
                Color32::from_black_alpha(110),
            ],
        );
        let opts = TextureOptions {
            wrap_mode: egui::TextureWrapMode::Repeat,
            ..TextureOptions::NEAREST
        };
        Overlay {
            tex: ctx.load_texture("k8-scanlines", img, opts),
        }
    }

    pub fn paint(&self, ctx: &egui::Context, t: f64, p: &Params) {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Debug,
            egui::Id::unique("k8-overlay"),
        ));
        let r = ctx.content_rect();
        if p.scanlines > 0.0 {
            let off = (t * 6.0 / 3.0) as f32;
            let uv =
                Rect::from_min_max(Pos2::new(0.0, -off), Pos2::new(1.0, r.height() / 3.0 - off));
            let a = (p.scanlines * 255.0) as u8;
            painter.image(self.tex.id(), r, uv, Color32::from_white_alpha(a));
        }
        if p.band > 0.0 {
            let y = r.min.y + (((t / 6.0).fract() * 1.4 - 0.2) as f32) * r.height();
            let h = 0.06 * r.height();
            let mut mesh = egui::Mesh::default();
            let c0 = Color32::TRANSPARENT;
            let c1 = Color32::from_white_alpha((p.band * 14.0) as u8);
            for (i, (yy, c)) in [(y - 2.0 * h, c0), (y, c1), (y + 2.0 * h, c0)]
                .into_iter()
                .enumerate()
            {
                mesh.colored_vertex(Pos2::new(r.min.x, yy), c);
                mesh.colored_vertex(Pos2::new(r.max.x, yy), c);
                if i > 0 {
                    let b = (i as u32) * 2;
                    mesh.add_triangle(b - 2, b - 1, b);
                    mesh.add_triangle(b - 1, b + 1, b);
                }
            }
            painter.add(mesh);
        }
        if p.flicker > 0.0 {
            let h = ((t * 30.0).floor().sin() * 43758.545).fract().abs() as f32;
            let a = (p.flicker * 0.03 * h * 255.0) as u8;
            painter.rect_filled(r, 0.0, Color32::from_black_alpha(a));
        }
    }
}

struct Targets {
    size: [u32; 2],
    scene: wgpu::TextureView,
    ha: wgpu::TextureView,
    hb: wgpu::TextureView,
    out_tex: [wgpu::Texture; 2],
    out: [wgpu::TextureView; 2],
}

/// The CRT renderer: its own egui renderer into a texture, then the passes.
pub struct Crt {
    pub renderer: egui_wgpu::Renderer,
    layout: wgpu::BindGroupLayout,
    samp: wgpu::Sampler,
    bright: wgpu::RenderPipeline,
    blur_h: wgpu::RenderPipeline,
    blur_v: wgpu::RenderPipeline,
    comp: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    ubuf: wgpu::Buffer,
    targets: Option<Targets>,
    cur: usize,
    blit_bg: Option<wgpu::BindGroup>,
}

pub const FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl Crt {
    pub fn new(device: &wgpu::Device, blit_format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("k8-crt"),
            source: wgpu::ShaderSource::Wgsl(WGSL.into()),
        });
        let tex = |b| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("k8"),
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
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("k8"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipe = |fs: &str, fmt| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: fmt,
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
        let samp = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("k8-params"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Crt {
            renderer: egui_wgpu::Renderer::new(device, FMT, Default::default()),
            bright: pipe("fs_bright", FMT),
            blur_h: pipe("fs_blur_h", FMT),
            blur_v: pipe("fs_blur_v", FMT),
            comp: pipe("fs_comp", FMT),
            blit: pipe("fs_blit", blit_format),
            layout,
            samp,
            ubuf,
            targets: None,
            cur: 0,
            blit_bg: None,
        }
    }

    fn ensure(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let mk = |w: u32, h: u32, extra| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("k8"),
                size: wgpu::Extent3d {
                    width: w.max(1),
                    height: h.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FMT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | extra,
                view_formats: &[],
            })
        };
        let v = |t: &wgpu::Texture| t.create_view(&Default::default());
        let none = wgpu::TextureUsages::empty();
        let copy = wgpu::TextureUsages::COPY_SRC;
        let (w, h) = (size[0], size[1]);
        let out_tex = [mk(w, h, copy), mk(w, h, copy)];
        self.targets = Some(Targets {
            size,
            scene: v(&mk(w, h, none)),
            ha: v(&mk(w / 2, h / 2, none)),
            hb: v(&mk(w / 2, h / 2, none)),
            out: [v(&out_tex[0]), v(&out_tex[1])],
            out_tex,
        });
    }

    fn bg(
        &self,
        device: &wgpu::Device,
        a: &wgpu::TextureView,
        b: &wgpu::TextureView,
        c: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("k8"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.ubuf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.samp),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(a),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(b),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(c),
                },
            ],
        })
    }

    fn pass(
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        pipe: &wgpu::RenderPipeline,
        bg: &wgpu::BindGroup,
    ) {
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("k8"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(pipe);
        rp.set_bind_group(0, bg, &[]);
        rp.draw(0..3, 0..1);
    }

    /// Render egui's picture and run the passes. The result is `output()`.
    /// `with_crt` false renders egui's picture only, to time it alone.
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        prims: &[ClippedPrimitive],
        sd: &ScreenDescriptor,
        deltas: &[(TextureId, ImageDelta)],
        params: &Params,
        time: f32,
        with_crt: bool,
    ) -> Vec<wgpu::CommandBuffer> {
        self.ensure(device, sd.size_in_pixels);
        for (id, d) in deltas {
            self.renderer.update_texture(device, queue, *id, d);
        }
        let cmds = self
            .renderer
            .update_buffers(device, queue, encoder, prims, sd);
        let t = self.targets.as_ref().expect("targets");
        {
            let rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("k8-scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.scene,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer.render(&mut rp.forget_lifetime(), prims, sd);
        }
        let [w, h] = sd.size_in_pixels;
        queue.write_buffer(
            &self.ubuf,
            0,
            &params.bytes(w, h, time, sd.pixels_per_point),
        );
        let next = 1 - self.cur;
        if with_crt {
            let b1 = self.bg(device, &t.scene, &t.scene, &t.scene);
            Self::pass(encoder, &t.ha, &self.bright, &b1);
            let b2 = self.bg(device, &t.ha, &t.ha, &t.ha);
            Self::pass(encoder, &t.hb, &self.blur_h, &b2);
            let b3 = self.bg(device, &t.hb, &t.hb, &t.hb);
            Self::pass(encoder, &t.ha, &self.blur_v, &b3);
            let b4 = self.bg(device, &t.scene, &t.ha, &t.out[self.cur]);
            Self::pass(encoder, &t.out[next], &self.comp, &b4);
            self.blit_bg = Some(self.bg(device, &t.out[next], &t.out[next], &t.out[next]));
        } else {
            self.blit_bg = Some(self.bg(device, &t.scene, &t.scene, &t.scene));
        }
        self.cur = next;
        cmds
    }

    /// The texture last written by `encode` with the passes on.
    pub fn output(&self) -> &wgpu::Texture {
        &self.targets.as_ref().expect("targets").out_tex[self.cur]
    }

    /// Draw the result into a render pass, such as eframe's own.
    pub fn blit(&self, rp: &mut wgpu::RenderPass<'static>) {
        if let Some(bg) = &self.blit_bg {
            rp.set_pipeline(&self.blit);
            rp.set_bind_group(0, bg, &[]);
            rp.draw(0..3, 0..1);
        }
    }
}

/// The hook inside eframe: one paint callback that carries the whole picture.
pub struct CrtCallback {
    pub prims: Vec<ClippedPrimitive>,
    pub atlas: ImageDelta,
    pub params: Params,
    pub time: f32,
}

impl egui_wgpu::CallbackTrait for CrtCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sd: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let crt: &mut Crt = res.get_mut().expect("Crt in callback resources");
        let deltas = [(TextureId::default(), self.atlas.clone())];
        crt.encode(
            device,
            queue,
            encoder,
            &self.prims,
            sd,
            &deltas,
            &self.params,
            self.time,
            true,
        )
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        rp: &mut wgpu::RenderPass<'static>,
        res: &egui_wgpu::CallbackResources,
    ) {
        if let Some(crt) = res.get::<Crt>() {
            crt.blit(rp);
        }
    }
}

/// Take everything egui painted this frame, tessellate it, and replace it by
/// one paint callback that draws it through the CRT passes.
pub fn redirect_to_crt(ctx: &egui::Context, params: Params, time: f32) {
    let to_global = ctx.memory(|m| m.to_global.clone());
    let shapes = ctx.graphics_mut(|g| g.drain(&[], &to_global));
    let prims = ctx.tessellate(shapes, ctx.pixels_per_point());
    let atlas = ImageDelta::full(ctx.fonts(|f| f.image()), TextureOptions::LINEAR);
    let rect = ctx.content_rect();
    ctx.layer_painter(egui::LayerId::background())
        .add(egui_wgpu::Callback::new_paint_callback(
            rect,
            CrtCallback {
                prims,
                atlas,
                params,
                time,
            },
        ));
}
