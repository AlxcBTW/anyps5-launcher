//! Animated background drawn by a wgpu paint callback inside egui's own render pass.
//!
//! This is a rendering proof: a fullscreen triangle with a tiny WGSL shader driven by one
//! time uniform. The real home-screen background replaces the shader later.

use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use wgpu::util::DeviceExt;

const SHADER: &str = r#"
struct Params { time: f32, aspect: f32, _pad: vec2<f32> };
@group(0) @binding(0) var<uniform> params: Params;

struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> Out {
    // One oversized triangle that covers the whole viewport.
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    let t = params.time * 0.15;
    var p = (in.uv - 0.5) * vec2<f32>(params.aspect, 1.0);
    // Slow, soft wave field in blue-violet tones on near-black.
    var v = 0.0;
    for (var k = 1; k <= 4; k = k + 1) {
        let f = f32(k);
        v = v + sin(p.x * 2.2 * f + t * (0.7 + f * 0.3) + sin(p.y * 1.7 * f - t)) / f;
    }
    let glow = smoothstep(-0.2, 1.6, v) * 0.5;
    let base = vec3<f32>(0.020, 0.027, 0.055);
    let tint = vec3<f32>(0.06, 0.14, 0.36) * glow + vec3<f32>(0.10, 0.04, 0.22) * glow * glow;
    let vignette = 1.0 - 0.55 * dot(in.uv - 0.5, in.uv - 0.5) * 2.0;
    return vec4<f32>((base + tint) * vignette, 1.0);
}
"#;

/// Uniform block: time, aspect, two floats of padding (16 bytes).
fn params_bytes(time: f32, aspect: f32) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&time.to_le_bytes());
    out[4..8].copy_from_slice(&aspect.to_le_bytes());
    out
}

/// GPU objects kept in egui's callback resources, created once at start-up.
struct Gpu {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// Builds the pipeline. Call once from the app creator.
pub fn init(render_state: &egui_wgpu::RenderState) {
    let device = &render_state.device;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("background"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("background params"),
        contents: &params_bytes(0.0, 1.0),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("background layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("background bind group"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("background pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("background pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(render_state.target_format.into())],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    render_state
        .renderer
        .write()
        .callback_resources
        .insert(Gpu {
            pipeline,
            buffer,
            bind_group,
        });
}

struct Callback {
    time: f32,
    aspect: f32,
}

impl egui_wgpu::CallbackTrait for Callback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(gpu) = resources.get::<Gpu>() {
            queue.write_buffer(&gpu.buffer, 0, &params_bytes(self.time, self.aspect));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(gpu) = resources.get::<Gpu>() {
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

/// Paints the animated background into `rect` and asks for the next frame.
pub fn paint(ui: &egui::Ui, rect: egui::Rect) {
    let time = ui.input(|i| i.time) as f32;
    let aspect = rect.width() / rect.height().max(1.0);
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
        rect,
        Callback { time, aspect },
    ));
    ui.request_repaint();
}
