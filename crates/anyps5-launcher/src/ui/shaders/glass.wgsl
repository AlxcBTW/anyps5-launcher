// Frosted-glass panel (SwiftUI `.ultraThinMaterial.opacity(0.55)` + black overlay + optional tint).
//
// Draws a rounded rectangle whose colour is made from the backdrop:
//   material = saturate(blurred backdrop) mixed with a light tint
//   base     = mix(sharp backdrop, material, material_mix)     // `.opacity(0.55)` shows the sharp image through
//   result   = base darkened by `black` and covered by `overlay` (straight alpha colour)
// The rounded-rect mask is an analytic signed distance with one pixel of anti-aliasing.

struct GlassParams {
    // Panel rectangle in framebuffer pixels: min.xy, size.xy.
    rect: vec4<f32>,
    // Backdrop rectangle in framebuffer pixels: min.xy, size.xy.
    backdrop: vec4<f32>,
    // Overlay colour on top of everything (straight alpha).
    overlay: vec4<f32>,
    radius: f32,
    material_mix: f32,
    tint: f32,
    black: f32,
    saturation: f32,
    linearize: u32,
    _pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> g: GlassParams;
@group(1) @binding(0) var sharp: texture_2d<f32>;
@group(1) @binding(1) var blurred: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) id: u32) -> VsOut {
    let corner = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn rounded_box_sdf(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let half_size = g.rect.zw * 0.5;
    let centre = g.rect.xy + half_size;
    let d = rounded_box_sdf(in.pos.xy - centre, half_size, min(g.radius, min(half_size.x, half_size.y)));
    let coverage = clamp(0.5 - d, 0.0, 1.0);
    let uv = (in.pos.xy - g.backdrop.xy) / g.backdrop.zw;
    let s = textureSample(sharp, samp, uv).rgb;
    var m = textureSample(blurred, samp, uv).rgb;
    let luma = dot(m, vec3<f32>(0.2126, 0.7152, 0.0722));
    m = mix(vec3<f32>(luma), m, g.saturation);
    m = mix(m, vec3<f32>(1.0), g.tint);
    var c = mix(s, m, g.material_mix);
    c = c * (1.0 - g.black);
    c = mix(c, g.overlay.rgb, g.overlay.a);
    if (g.linearize != 0u) {
        c = srgb_to_linear(c);
    }
    return vec4<f32>(c * coverage, coverage);
}
