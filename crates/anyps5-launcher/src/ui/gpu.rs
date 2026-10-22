//! Small wgpu helpers shared by the backdrop passes (background, blur, glass).

use eframe::egui_wgpu::wgpu;

/// Offscreen format of the backdrop: display-referred sRGB values, no gamma conversion.
pub const BACKDROP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Format of the blur chain (half floats avoid banding in dark gradients).
pub const BLUR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// A texture with its default view.
pub struct Target {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub size: (u32, u32),
}

impl Target {
    pub fn new(
        device: &wgpu::Device,
        label: &str,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            size,
        }
    }
}

/// Linear, clamped sampler.
pub fn linear_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("backdrop sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    })
}

/// Bind group layout entry for a uniform buffer visible to the fragment stage.
pub fn uniform_entry(binding: u32, dynamic: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Bind group layout entry for a filterable 2D float texture.
pub fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// Bind group layout entry for a filtering sampler.
pub fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// A pipeline that draws one oversized triangle with the shader's `vs` / `fs` entry points
/// (or `fs_entry`) into a single colour target.
pub fn triangle_pipeline(
    device: &wgpu::Device,
    label: &str,
    module: &wgpu::ShaderModule,
    fs_entry: &str,
    layouts: &[&wgpu::BindGroupLayout],
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let layouts: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().map(|l| Some(*l)).collect();
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &layouts,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// Starts a render pass that fully overwrites `view`.
pub fn begin_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    label: &str,
    view: &'a wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// Whether a swap-chain format needs sRGB-to-linear conversion for display-referred content.
pub fn needs_linearize(format: wgpu::TextureFormat) -> bool {
    format.is_srgb()
}

#[cfg(test)]
mod tests {
    use naga::valid::{Capabilities, ValidationFlags, Validator};

    use crate::ui::background::{BACKGROUND_WGSL, BLIT_WGSL};
    use crate::ui::glass::GLASS_WGSL;

    const BLUR_WGSL: &str = include_str!("shaders/blur.wgsl");

    /// Parses and validates a WGSL module with naga and returns its entry point names.
    fn validate(name: &str, source: &str) -> Vec<String> {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|err| panic!("{name} does not parse:\n{}", err.emit_to_string(source)));
        Validator::new(ValidationFlags::all(), Capabilities::all())
            .validate(&module)
            .unwrap_or_else(|err| {
                panic!("{name} does not validate:\n{}", err.emit_to_string(source))
            });
        module.entry_points.iter().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn theme_background_shader_is_valid_wgsl() {
        let entries = validate("background.wgsl", BACKGROUND_WGSL);
        assert!(entries.contains(&"vs".to_owned()) && entries.contains(&"fs".to_owned()));
    }

    #[test]
    fn blit_shader_is_valid_wgsl() {
        let entries = validate("blit.wgsl", BLIT_WGSL);
        assert!(entries.contains(&"vs".to_owned()) && entries.contains(&"fs".to_owned()));
    }

    #[test]
    fn blur_shader_is_valid_wgsl() {
        let entries = validate("blur.wgsl", BLUR_WGSL);
        for entry in ["vs", "fs_down", "fs_blur"] {
            assert!(entries.contains(&entry.to_owned()), "missing {entry}");
        }
    }

    #[test]
    fn glass_shader_is_valid_wgsl() {
        let entries = validate("glass.wgsl", GLASS_WGSL);
        assert!(entries.contains(&"vs".to_owned()) && entries.contains(&"fs".to_owned()));
    }
}
