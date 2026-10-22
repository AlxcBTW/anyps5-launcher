// Draws an offscreen backdrop texture into egui's render pass (the callback rect is the viewport).
// The texture holds display-referred sRGB values; `linearize` converts them when the swap chain
// format is an sRGB format.

struct Params {
    uv_min: vec2<f32>,
    uv_max: vec2<f32>,
    // Rectangle in framebuffer pixels: min.xy, size.xy (for the rounded corners).
    rect: vec4<f32>,
    alpha: f32,
    linearize: u32,
    radius: f32,
    _pad: f32,
};
@group(0) @binding(0) var<uniform> p: Params;
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

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// Uniform in 0..1, fixed per framebuffer pixel so the dither never flickers.
fn pixel_noise(px: vec2<u32>, seed: u32) -> f32 {
    return f32(pcg(px.x ^ pcg(px.y ^ seed)) >> 8u) / 16777216.0;
}

fn rounded_box_sdf(q: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let d = abs(q) - half_size + vec2<f32>(r);
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - r;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let uv = mix(p.uv_min, p.uv_max, in.uv);
    var c = textureSample(tex, samp, uv).rgb;
    // triangular dither of about one 8-bit step at full resolution against banding in the dark gradients
    let px = vec2<u32>(in.pos.xy);
    c = max(c + vec3<f32>((pixel_noise(px, 0x9e37u) + pixel_noise(px, 0x85ebu) - 1.0) / 255.0), vec3<f32>(0.0));
    if (p.linearize != 0u) {
        c = srgb_to_linear(c);
    }
    var coverage = 1.0;
    if (p.radius > 0.0) {
        let half_size = p.rect.zw * 0.5;
        let d = rounded_box_sdf(in.pos.xy - (p.rect.xy + half_size), half_size, min(p.radius, min(half_size.x, half_size.y)));
        coverage = clamp(0.5 - d, 0.0, 1.0);
    }
    let a = p.alpha * coverage;
    return vec4<f32>(c * a, a);
}
