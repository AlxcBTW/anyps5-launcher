struct Uniforms { deep: vec4<f32>, middle: vec4<f32>, bright: vec4<f32>, size: vec2<f32>, time: f32, theme: u32 };
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs(@builtin(vertex_index) id: u32) -> VsOut {
    // one oversized triangle: corners (0,0) (2,0) (0,2)
    let corner = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(corner.x, 1.0 - corner.y);   // uv origin top-left, y down (uv.y = 1 at the bottom of the screen)
    return o;
}

fn sstep(e0: f32, e1: f32, x: f32) -> f32 {        // Metal-style smoothstep, valid for e0 > e1
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn hash(p_in: vec2<f32>) -> f32 {
    var p = fract(p_in * vec2<f32>(123.34, 456.21));
    p = p + vec2<f32>(dot(p, p + vec2<f32>(45.32)));
    return fract(p.x * p.y);
}
fn hash2(p: vec2<f32>) -> vec2<f32> { return vec2<f32>(hash(p), hash(p + vec2<f32>(17.17))); }

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), w.x),
               mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), w.x), w.y);
}

fn fbm(p_in: vec2<f32>) -> f32 {
    var value = 0.0;
    var amplitude = 0.5;
    var p = p_in;
    for (var octave = 0; octave < 5; octave++) {
        value += amplitude * noise(p);
        p = p * 2.03 + vec2<f32>(11.7);
        amplitude *= 0.5;
    }
    return value;
}

// Drifting soft discs of light; `rate` moves them upwards (negative: downwards).
fn bokeh(p: vec2<f32>, time: f32, cells: f32, rate: f32, seed: f32) -> f32 {
    var grid = p * cells;
    grid.y -= time * rate;
    let cell = floor(grid);
    var light = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let here = cell + vec2<f32>(f32(x), f32(y));
            let random = hash2(here + vec2<f32>(seed));
            let center = here + vec2<f32>(0.5) + 0.38 * sin(random * 6.2831 + time * (vec2<f32>(0.15) + 0.2 * random.yx));
            let radius = mix(0.06, 0.24, random.x * random.x);
            let dist = length(grid - center);
            let twinkle = 0.55 + 0.45 * sin(time * (0.4 + random.y) + random.x * 40.0);
            let present = step(0.55, hash(here + vec2<f32>(seed * 3.1)));
            light += present * twinkle * sstep(radius, radius * 0.25, dist) * (0.35 + 0.65 * random.y);
        }
    }
    return light;
}

fn cosmic(uv: vec2<f32>, aspect: f32, time: f32) -> vec3<f32> {
    var color = mix(u.middle.rgb * 1.15, u.deep.rgb, sstep(0.0, 1.0, uv.y * 0.85 + 0.08));
    color += u.bright.rgb * 0.28 * exp(-2.8 * length(vec2<f32>((uv.x - 0.8) * aspect, uv.y - 0.08)));
    // ribbons of light flowing across the lower half
    for (var band = 0; band < 6; band++) {
        let b = f32(band);
        let center = 0.56 + 0.028 * b
            + 0.075 * sin(uv.x * (1.4 + 0.22 * b) + time * (0.10 + 0.021 * b) + b * 1.9)
            + 0.03 * sin(uv.x * 4.1 - time * 0.12 + b * 3.1);
        let d = abs(uv.y - center);
        let core = 0.0016 / (d + 0.0028);
        let haze = exp(-d * 14.0);
        let fade = sstep(-0.15, 0.4, uv.x) * sstep(1.2, 0.6, uv.x);
        let pulse = 0.75 + 0.25 * sin(time * 0.3 + b * 1.7 + uv.x * 2.0);
        let tint = mix(u.bright.rgb, vec3<f32>(0.85, 0.92, 1.0), 0.25 + 0.1 * b);
        color += tint * (core * 0.16 + haze * 0.1) * fade * pulse;
    }
    let p = vec2<f32>(uv.x * aspect, uv.y);
    color += u.bright.rgb * 0.22 * bokeh(p, time, 5.0, 0.012, 1.0);
    color += vec3<f32>(0.9, 0.95, 1.0) * 0.14 * bokeh(p, time * 1.3, 11.0, 0.02, 7.0);
    return color;
}

fn aurora(uv: vec2<f32>, aspect: f32, time: f32) -> vec3<f32> {
    var color = mix(u.deep.rgb * 1.8, u.deep.rgb * 0.7, uv.y);
    let p = vec2<f32>(uv.x * aspect, uv.y);
    for (var layer = 0; layer < 3; layer++) {
        let l = f32(layer);
        let sway = fbm(vec2<f32>(p.x * 0.7 + l * 3.7 + time * 0.02, time * 0.03 + l));
        let center = 0.30 + 0.09 * l + 0.36 * (sway - 0.5) + 0.05 * sin(p.x * 1.7 + time * 0.08 + l * 2.0);
        let thickness = 6.0 + 2.0 * l + 2.5 * sin(p.x * 1.3 + time * 0.05 + l);
        let below = uv.y - center;
        let curtain = exp(-below * below * thickness * thickness) * sstep(-0.35, 0.0, below + 0.12);
        let rays = sstep(0.15, 0.95, noise(vec2<f32>(p.x * 16.0 + l * 7.0, time * 0.18 + p.x * 0.5)));
        let glow = curtain * (0.45 + 0.55 * rays);
        let tint = mix(u.bright.rgb,
                       mix(vec3<f32>(0.35, 0.55, 1.0), vec3<f32>(0.75, 0.4, 1.0), l * 0.5),
                       sstep(-0.05, 0.12, -below));
        color += tint * glow * (0.62 - 0.14 * l);
    }
    let starCell = floor(p * 150.0);
    let star = step(0.9965, hash(starCell)) * (0.55 + 0.45 * sin(time * 1.6 + hash(starCell + vec2<f32>(3.0)) * 30.0));
    let local = fract(p * 150.0) - vec2<f32>(0.5);
    color += star * sstep(0.35, 0.0, length(local)) * sstep(0.8, 0.1, uv.y) * 0.9;
    color += u.middle.rgb * 0.45 * sstep(0.6, 1.0, uv.y) * (0.6 + 0.4 * fbm(vec2<f32>(p.x * 1.5, time * 0.03)));
    return color;
}

fn sunset(uv: vec2<f32>, aspect: f32, time: f32) -> vec3<f32> {
    let top = u.deep.rgb * 1.4;
    let middle = u.middle.rgb;
    let low = mix(u.bright.rgb, vec3<f32>(1.0, 0.42, 0.38), 0.45) * 0.85;
    var color = mix(top, middle, sstep(0.0, 0.6, uv.y));
    color = mix(color, low, sstep(0.5, 1.1, uv.y));
    let p = vec2<f32>(uv.x * aspect, uv.y);
    for (var blob = 0; blob < 6; blob++) {
        let b = f32(blob);
        let center = vec2<f32>((0.15 + 0.16 * b) * aspect + 0.14 * sin(time * (0.05 + 0.013 * b) + b * 2.0),
                               0.5 + 0.24 * sin(time * (0.04 + 0.01 * b) + b * 1.3));
        let radius = 0.2 + 0.07 * sin(b * 4.1);
        let d = p - center;
        let light = exp(-dot(d, d) / (radius * radius));
        color += mix(u.bright.rgb, vec3<f32>(1.0, 0.35, 0.6), fract(b * 0.37)) * light * 0.2;
    }
    color += mix(u.bright.rgb, vec3<f32>(1.0, 0.8, 0.6), 0.4) * 0.22 * exp(-abs(uv.y - 0.76) * 5.0)
             * (0.85 + 0.15 * sin(uv.x * 2.4 + time * 0.15));
    color += vec3<f32>(1.0, 0.85, 0.7) * 0.1 * bokeh(p, time, 7.0, -0.01, 3.0);
    return color;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let aspect = u.size.x / max(u.size.y, 1.0);
    let uv = in.uv;
    let time = u.time;
    var color: vec3<f32>;
    if (u.theme == 1u) { color = aurora(uv, aspect, time); }
    else if (u.theme == 2u) { color = sunset(uv, aspect, time); }
    else { color = cosmic(uv, aspect, time); }
    let centered = (uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0);
    color = color * (1.0 - 0.45 * sstep(0.35, 1.25, length(centered)));   // vignette
    color = color / (vec3<f32>(1.0) + color * 0.35);                      // soft tone map
    color += vec3<f32>((hash(in.pos.xy + vec2<f32>(fract(time) * 91.0)) - 0.5) / 255.0 * 2.0);  // dither
    return vec4<f32>(color, 1.0);
}
