//! Material blur (docs/DESIGN.md section 4): a two-pass separable Gaussian of the backdrop.
//!
//! The backdrop texture (half the window's pixels) is first reduced 4x with a box filter, giving
//! 1/8 of the physical resolution, then blurred horizontally and vertically with a kernel built
//! from the linear-sampling trick (about half as many fetches as taps). The result is tiny, so the
//! whole chain costs a few hundred microseconds and runs only when the backdrop changed.
//!
//! The chain is generic: [`BlurChain::run`] takes any texture view as source, so artwork that is
//! composed into a backdrop slot gets blurred the same way.

use bytemuck::{Pod, Zeroable};
use eframe::egui_wgpu::wgpu;

use crate::ui::gpu::{self, BLUR_FORMAT, Target};

/// Blur strength of the "ultra thin material" look, in dp at final scale (estimate, DESIGN.md 12).
pub const DEFAULT_SIGMA_DP: f32 = 20.0;
/// How much smaller than the source the blur chain works.
pub const DOWNSCALE: u32 = 4;
/// Most pairs of taps the shader supports (centre + 7 pairs = radius 14).
pub const MAX_TAPS: usize = 8;

/// Settings of one blur.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurSettings {
    /// Gaussian sigma in dp (final scale).
    pub sigma_dp: f32,
}

impl Default for BlurSettings {
    fn default() -> Self {
        Self {
            sigma_dp: DEFAULT_SIGMA_DP,
        }
    }
}

/// Offsets (in texels) and weights of the linear-sampling Gaussian: `taps[0]` is the centre,
/// every further tap stands for two neighbouring texels and is applied on both sides.
/// `w0 + 2 * sum(w_i)` is 1.
pub fn gaussian_taps(sigma: f32) -> Vec<[f32; 2]> {
    let sigma = sigma.max(0.05);
    let radius = ((sigma * 3.0).ceil() as usize).clamp(1, 2 * (MAX_TAPS - 1));
    let weights: Vec<f64> = (0..=radius)
        .map(|i| (-((i * i) as f64) / (2.0 * f64::from(sigma).powi(2))).exp())
        .collect();
    let total: f64 = weights[0] + 2.0 * weights[1..].iter().sum::<f64>();
    let w = |i: usize| weights.get(i).copied().unwrap_or(0.0) / total;
    let mut taps = vec![[0.0, w(0) as f32]];
    let mut i = 1;
    while i <= radius {
        let (a, b) = (w(i), w(i + 1));
        let combined = a + b;
        let offset = (i as f64 * a + (i + 1) as f64 * b) / combined;
        taps.push([offset as f32, combined as f32]);
        i += 2;
    }
    taps
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlurUniforms {
    step: [f32; 2],
    count: u32,
    _pad: u32,
    taps: [[f32; 4]; MAX_TAPS],
}

impl BlurUniforms {
    fn new(step: [f32; 2], taps: &[[f32; 2]]) -> Self {
        let mut out = Self::zeroed();
        out.step = step;
        out.count = taps.len().min(MAX_TAPS) as u32;
        for (slot, tap) in out.taps.iter_mut().zip(taps) {
            *slot = [tap[0], tap[1], 0.0, 0.0];
        }
        out
    }
}

/// Pipelines of the chain. Create once.
pub struct BlurPipelines {
    layout: wgpu::BindGroupLayout,
    down: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
}

impl BlurPipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/blur.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur layout"),
            entries: &[
                gpu::uniform_entry(0, false),
                gpu::texture_entry(1),
                gpu::sampler_entry(2),
            ],
        });
        let down = gpu::triangle_pipeline(
            device,
            "blur down",
            &module,
            "fs_down",
            &[&layout],
            BLUR_FORMAT,
            None,
        );
        let blur = gpu::triangle_pipeline(
            device,
            "blur gauss",
            &module,
            "fs_blur",
            &[&layout],
            BLUR_FORMAT,
            None,
        );
        Self { layout, down, blur }
    }
}

/// The blur targets for one source size.
pub struct BlurChain {
    source_size: (u32, u32),
    a: Target,
    b: Target,
    down_uniform: wgpu::Buffer,
    h_uniform: wgpu::Buffer,
    v_uniform: wgpu::Buffer,
    /// Bind groups are rebuilt when the source view changes (same size, new texture).
    down_bind: Option<wgpu::BindGroup>,
    h_bind: wgpu::BindGroup,
    v_bind: wgpu::BindGroup,
    last_sigma_bits: Option<u32>,
}

impl BlurChain {
    /// Targets for a source of `source_size` texels.
    pub fn new(
        device: &wgpu::Device,
        pipelines: &BlurPipelines,
        sampler: &wgpu::Sampler,
        source_size: (u32, u32),
    ) -> Self {
        let size = (
            source_size.0.div_ceil(DOWNSCALE).max(1),
            source_size.1.div_ceil(DOWNSCALE).max(1),
        );
        let a = Target::new(device, "blur a", size, BLUR_FORMAT);
        let b = Target::new(device, "blur b", size, BLUR_FORMAT);
        let uniform = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<BlurUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let down_uniform = uniform("blur down uniform");
        let h_uniform = uniform("blur h uniform");
        let v_uniform = uniform("blur v uniform");
        let bind = |uniform: &wgpu::Buffer, view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blur bind"),
                layout: &pipelines.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };
        let h_bind = bind(&h_uniform, &a.view);
        let v_bind = bind(&v_uniform, &b.view);
        Self {
            source_size,
            a,
            b,
            down_uniform,
            h_uniform,
            v_uniform,
            down_bind: None,
            h_bind,
            v_bind,
            last_sigma_bits: None,
        }
    }

    pub fn source_size(&self) -> (u32, u32) {
        self.source_size
    }

    /// The blurred result (valid after [`run`](Self::run)).
    pub fn output(&self) -> &wgpu::TextureView {
        &self.a.view
    }

    /// Reduces `source` and blurs it. `sigma_texels` is the Gaussian sigma in texels of the
    /// reduced image (`sigma_dp * pixels_per_point / 8` for a half-resolution backdrop).
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: &BlurPipelines,
        sampler: &wgpu::Sampler,
        source: &wgpu::TextureView,
        rebind_source: bool,
        sigma_texels: f32,
    ) {
        if self.down_bind.is_none() || rebind_source {
            // One source texel in uv units, for the box filter; fixed for the chain's size.
            let step = [
                1.0 / self.source_size.0 as f32,
                1.0 / self.source_size.1 as f32,
            ];
            queue.write_buffer(
                &self.down_uniform,
                0,
                bytemuck::bytes_of(&BlurUniforms::new(step, &[])),
            );
            self.down_bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blur down bind"),
                layout: &pipelines.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.down_uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            }));
        }
        let sigma_bits = sigma_texels.to_bits();
        if self.last_sigma_bits != Some(sigma_bits) {
            self.last_sigma_bits = Some(sigma_bits);
            let taps = gaussian_taps(sigma_texels);
            let (w, h) = (self.a.size.0 as f32, self.a.size.1 as f32);
            queue.write_buffer(
                &self.h_uniform,
                0,
                bytemuck::bytes_of(&BlurUniforms::new([1.0 / w, 0.0], &taps)),
            );
            queue.write_buffer(
                &self.v_uniform,
                0,
                bytemuck::bytes_of(&BlurUniforms::new([0.0, 1.0 / h], &taps)),
            );
        }
        let Some(down_bind) = self.down_bind.as_ref() else {
            return;
        };
        for (label, pipeline, bind, target) in [
            ("blur down", &pipelines.down, down_bind, &self.a.view),
            (
                "blur horizontal",
                &pipelines.blur,
                &self.h_bind,
                &self.b.view,
            ),
            ("blur vertical", &pipelines.blur, &self.v_bind, &self.a.view),
        ] {
            let mut pass = gpu::begin_pass(encoder, label, target);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mass(taps: &[[f32; 2]]) -> f32 {
        taps[0][1] + 2.0 * taps[1..].iter().map(|t| t[1]).sum::<f32>()
    }

    #[test]
    fn kernel_is_normalised_for_every_sigma() {
        for sigma in [0.5, 1.0, 2.5, 4.0, 5.0, 8.0] {
            let taps = gaussian_taps(sigma);
            assert!(
                (mass(&taps) - 1.0).abs() < 1e-4,
                "sigma {sigma}: {}",
                mass(&taps)
            );
            assert!(taps.len() <= MAX_TAPS, "sigma {sigma}: {} taps", taps.len());
        }
    }

    #[test]
    fn linear_sampling_matches_the_discrete_gaussian() {
        // Reconstruct the discrete weights from a pair: offset o between i and i+1 with weight w
        // means w_i : w_{i+1} = (i+1-o) : (o-i).
        let sigma = 2.5f32;
        let taps = gaussian_taps(sigma);
        let g = |i: f32| (-(i * i) / (2.0 * sigma * sigma)).exp();
        let total = 1.0 + 2.0 * (1..=8).map(|i| g(i as f32)).sum::<f32>();
        assert!((taps[0][1] - 1.0 / total).abs() < 1e-4);
        // First pair stands for texels 1 and 2.
        let [o, w] = taps[1];
        assert!((w - (g(1.0) + g(2.0)) / total).abs() < 1e-4);
        assert!(o > 1.0 && o < 2.0);
        assert!((o - (g(1.0) + 2.0 * g(2.0)) / (g(1.0) + g(2.0))).abs() < 1e-3);
    }

    #[test]
    fn uniform_block_is_the_size_the_shader_expects() {
        // step (8) + count (4) + pad (4) + 8 * vec4 (128)
        assert_eq!(std::mem::size_of::<BlurUniforms>(), 144);
    }

    #[test]
    fn huge_sigma_is_clamped_to_the_shader_limit() {
        let taps = gaussian_taps(100.0);
        assert!(taps.len() <= MAX_TAPS);
    }
}
