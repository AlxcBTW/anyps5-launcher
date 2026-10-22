// Two-pass separable Gaussian blur on a small texture (docs/DESIGN.md section 4).
//
//   fs_down: 4 x 4 box downsample using four bilinear taps (exact for a 4x reduction).
//   fs_blur: one direction of the Gaussian with the linear-sampling trick; taps[0] is the centre
//            (offset 0, weight w0), taps[i] pairs two neighbouring texels (x = offset in texels,
//            y = combined weight). Weights sum to 1.

struct BlurParams {
    // Step of one texel along the blur direction, in uv units.
    step: vec2<f32>,
    count: u32,
    _pad: u32,
    taps: array<vec4<f32>, 8>,
};
@group(0) @binding(0) var<uniform> b: BlurParams;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) id: u32) -> VsOut {
    let corner = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return o;
}

// `b.step` holds one *source* texel here.
@fragment
fn fs_down(in: VsOut) -> @location(0) vec4<f32> {
    let o = b.step;
    var c = textureSample(tex, samp, in.uv + vec2<f32>(-o.x, -o.y));
    c += textureSample(tex, samp, in.uv + vec2<f32>(o.x, -o.y));
    c += textureSample(tex, samp, in.uv + vec2<f32>(-o.x, o.y));
    c += textureSample(tex, samp, in.uv + vec2<f32>(o.x, o.y));
    return c * 0.25;
}

@fragment
fn fs_blur(in: VsOut) -> @location(0) vec4<f32> {
    var c = textureSample(tex, samp, in.uv) * b.taps[0].y;
    for (var i = 1u; i < b.count; i++) {
        let tap = b.taps[i];
        let o = b.step * tap.x;
        c += (textureSample(tex, samp, in.uv + o) + textureSample(tex, samp, in.uv - o)) * tap.y;
    }
    return c;
}
