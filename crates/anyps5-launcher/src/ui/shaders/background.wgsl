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

fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// Random 0..1 for an integer cell. Integer arithmetic, so neighbouring pixels that reach the same
// cell from different sides always agree (a float `fract(p * big)` hash does not once the compiler
// fuses the multiply, and every disagreement shows up as a hard seam).
fn hash(cell: vec2<f32>, seed: u32) -> f32 {
    let c = bitcast<vec2<u32>>(vec2<i32>(floor(cell)));
    return f32(pcg(c.x ^ pcg(c.y ^ seed)) >> 8u) / 16777216.0;
}

// Gradient noise in 0..1 with a quintic fade: no flat spots or creases at the lattice.
fn lattice_gradient(cell: vec2<f32>) -> vec2<f32> {
    let angle = hash(cell, 0u) * 6.2831853;
    return vec2<f32>(cos(angle), sin(angle));
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = dot(lattice_gradient(i), f);
    let b = dot(lattice_gradient(i + vec2<f32>(1.0, 0.0)), f - vec2<f32>(1.0, 0.0));
    let c = dot(lattice_gradient(i + vec2<f32>(0.0, 1.0)), f - vec2<f32>(0.0, 1.0));
    let d = dot(lattice_gradient(i + vec2<f32>(1.0, 1.0)), f - vec2<f32>(1.0, 1.0));
    return clamp(0.5 + 0.75 * mix(mix(a, b, w.x), mix(c, d, w.x), w.y), 0.0, 1.0);
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

// Colour blends in Oklab, so a gradient between two hues does not dip into grey or brown on the way.
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let k = max(c, vec3<f32>(0.0));
    return select(pow((k + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), k / 12.92, k <= vec3<f32>(0.04045));
}

fn to_display(c: vec3<f32>) -> vec3<f32> {
    let k = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(k, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), k * 12.92, k <= vec3<f32>(0.0031308));
}

fn oklab(display: vec3<f32>) -> vec3<f32> {
    let c = to_linear(display);
    let l = pow(0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b + 1e-7, 1.0 / 3.0);
    let m = pow(0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b + 1e-7, 1.0 / 3.0);
    let s = pow(0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b + 1e-7, 1.0 / 3.0);
    return vec3<f32>(0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
                     1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
                     0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s);
}

fn from_oklab(lab: vec3<f32>) -> vec3<f32> {
    let l = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
    let m = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
    let s = lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z;
    let l3 = l * l * l;
    let m3 = m * m * m;
    let s3 = s * s * s;
    return to_display(vec3<f32>(4.0767416621 * l3 - 3.3077115913 * m3 + 0.2309699292 * s3,
                                -1.2684380046 * l3 + 2.6097574011 * m3 - 0.3413193965 * s3,
                                -0.0041960863 * l3 - 0.7034186147 * m3 + 1.7076147010 * s3));
}

fn blend(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> {
    return from_oklab(mix(oklab(a), oklab(b), t));
}

// Drifting soft discs of light; `rate` moves them upwards (negative: downwards), `density` is the
// share of empty cells (higher = fewer lights).
fn bokeh(p: vec2<f32>, time: f32, cells: f32, rate: f32, seed: u32, density: f32) -> f32 {
    var grid = p * cells;
    grid.y -= time * rate;
    let cell = floor(grid);
    var light = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let here = cell + vec2<f32>(f32(x), f32(y));
            let random = vec2<f32>(hash(here, seed), hash(here, seed + 1u));
            let center = here + vec2<f32>(0.5) + 0.36 * sin(random * 6.2831 + time * (vec2<f32>(0.08) + 0.1 * random.yx));
            let radius = mix(0.08, 0.26, random.x * random.x);
            let dist = length(grid - center);
            let twinkle = 0.8 + 0.2 * sin(time * (0.2 + 0.4 * random.y) + random.x * 40.0);
            let present = step(density, hash(here, seed + 2u));
            light += present * twinkle * sstep(radius, radius * 0.3, dist) * (0.4 + 0.6 * random.y);
        }
    }
    return light;
}

fn cosmic(uv: vec2<f32>, aspect: f32, time: f32) -> vec3<f32> {
    var color = blend(u.middle.rgb * 1.12, u.deep.rgb, sstep(0.0, 1.0, uv.y * 0.9 + 0.05));
    let pool = vec2<f32>((uv.x - 0.82) * aspect, (uv.y - 0.04) * 1.3);
    color += u.bright.rgb * 0.22 * exp(-2.2 * dot(pool, pool));
    // one soft ribbon of light across the lower half: four strands riding the same slow wave,
    // drifting apart and together along the way
    let flow = 0.08 * sin(uv.x * 2.6 + time * 0.07 + 0.8) + 0.03 * sin(uv.x * 5.3 - time * 0.05 + 1.3);
    let fade = sstep(-0.1, 0.4, uv.x) * sstep(1.15, 0.6, uv.x);
    for (var strand = 0; strand < 4; strand++) {
        let s = f32(strand);
        let spread = 0.6 + 0.4 * sin(uv.x * 2.4 + time * 0.05 + s * 1.7);
        let center = 0.6 + flow + (0.016 + 0.014 * s) * spread * (s - 1.5);
        let d = uv.y - center;
        let core = 0.0012 / (abs(d) + 0.003);
        let haze = exp(-d * d * 260.0);
        let tint = mix(u.bright.rgb, vec3<f32>(0.85, 0.92, 1.0), 0.3 + 0.1 * s);
        color += tint * (core * 0.2 + haze * 0.07) * fade;
    }
    let p = vec2<f32>(uv.x * aspect, uv.y);
    color += u.bright.rgb * 0.16 * bokeh(p, time, 4.0, 0.01, 10u, 0.72);
    color += vec3<f32>(0.9, 0.95, 1.0) * 0.08 * bokeh(p, time * 1.2, 9.0, 0.016, 70u, 0.86);
    return color;
}

fn aurora(uv: vec2<f32>, aspect: f32, time: f32) -> vec3<f32> {
    var color = blend(u.deep.rgb * 1.8, u.deep.rgb * 0.7, uv.y);
    let p = vec2<f32>(uv.x * aspect, uv.y);
    for (var layer = 0; layer < 3; layer++) {
        let l = f32(layer);
        let sway = fbm(vec2<f32>(p.x * 0.6 + l * 3.7 + time * 0.015, time * 0.02 + l));
        let center = 0.30 + 0.09 * l + 0.32 * (sway - 0.5) + 0.04 * sin(p.x * 1.5 + time * 0.06 + l * 2.0);
        let thickness = 6.0 + 2.0 * l + 2.0 * sin(p.x * 1.1 + time * 0.04 + l);
        let below = uv.y - center;
        // a crisp lower edge and a long glow upwards, like a real curtain
        let reach = select(thickness * 0.5, thickness * 1.7, below > 0.0);
        let curtain = exp(-below * below * reach * reach);
        // wide, soft folds instead of hard columns
        let fold = 0.65 * noise(vec2<f32>(p.x * 3.0 + l * 7.0, time * 0.05 + l))
                 + 0.35 * noise(vec2<f32>(p.x * 7.3 + l * 3.0, time * 0.08 - l));
        let rays = 0.45 + 0.55 * sstep(0.3, 0.75, fold);
        let tint = mix(u.bright.rgb,
                       mix(vec3<f32>(0.35, 0.55, 1.0), vec3<f32>(0.7, 0.45, 1.0), l * 0.5),
                       sstep(-0.05, 0.14, -below));
        let rim = exp(-below * below * thickness * thickness * 9.0);
        color += tint * curtain * rays * (0.55 - 0.12 * l) + u.bright.rgb * rim * rays * (0.22 - 0.05 * l);
    }
    let starCell = floor(p * 150.0);
    let star = step(0.9975, hash(starCell, 11u)) * (0.75 + 0.25 * sin(time * 0.8 + hash(starCell, 12u) * 30.0));
    let local = fract(p * 150.0) - vec2<f32>(0.5);
    color += star * sstep(0.35, 0.0, length(local)) * sstep(0.75, 0.1, uv.y) * 0.8;
    color += u.middle.rgb * 0.4 * sstep(0.62, 1.0, uv.y) * (0.7 + 0.3 * fbm(vec2<f32>(p.x * 1.2, time * 0.02)));
    return color;
}

fn sunset(uv: vec2<f32>, aspect: f32, time: f32) -> vec3<f32> {
    let top = u.deep.rgb * 1.3;
    let low = mix(u.bright.rgb, vec3<f32>(1.0, 0.5, 0.42), 0.35) * 0.92;
    var color = blend(top, u.middle.rgb, sstep(0.0, 0.62, uv.y));
    color = blend(color, low, sstep(0.48, 1.05, uv.y));
    let p = vec2<f32>(uv.x * aspect, uv.y);
    for (var glow = 0; glow < 5; glow++) {
        let b = f32(glow);
        let center = vec2<f32>((0.18 + 0.19 * b) * aspect + 0.12 * sin(time * (0.035 + 0.01 * b) + b * 2.0),
                               0.5 + 0.2 * sin(time * (0.03 + 0.008 * b) + b * 1.3));
        let radius = 0.24 + 0.06 * sin(b * 4.1);
        let d = p - center;
        color += mix(u.bright.rgb, vec3<f32>(1.0, 0.4, 0.62), fract(b * 0.37)) * exp(-dot(d, d) / (radius * radius)) * 0.16;
    }
    let band = uv.y - 0.78;
    color += mix(u.bright.rgb, vec3<f32>(1.0, 0.82, 0.62), 0.4) * 0.2 * exp(-band * band * 40.0)
             * (0.9 + 0.1 * sin(uv.x * 2.0 + time * 0.1));
    color += vec3<f32>(1.0, 0.88, 0.75) * 0.07 * bokeh(p, time, 6.0, -0.008, 30u, 0.8);
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
    color = color * (1.0 - 0.4 * sstep(0.35, 1.25, length(centered)));   // vignette
    color = color / (vec3<f32>(1.0) + color * 0.35);                      // soft tone map
    return vec4<f32>(color, 1.0);
}
