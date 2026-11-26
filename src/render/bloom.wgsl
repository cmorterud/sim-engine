// Bloom (bloom.rs): light brighter than the screen can show spreads into a soft glow around it.
//
// The scene is drawn untoned into a bright target. Its brightest light is taken out at half
// size, shrunk step by step into smaller and smaller copies, then grown back, each copy added
// to the one above it, so that a glint spreads a little and an explosion a long way. Last,
// the scene and the glow are toned together into the window.

struct Params {
    // Brightness, before toning, from which light glows, and how soft that start is.
    threshold: f32,
    knee: f32,
    // How much of the glow is added to the scene.
    intensity: f32,
    // How far each step's glow spreads, in its texels.
    radius: f32,
};

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var smooth_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;
// In the last step, the glow; in the others, the source again.
@group(0) @binding(3) var glow: texture_2d<f32>;

struct Fullscreen {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle covering the screen.
@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> Fullscreen {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Fullscreen;
    out.clip = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn tap(uv: vec2<f32>, texel: vec2<f32>, x: f32, y: f32) -> vec3<f32> {
    return textureSampleLevel(source, smooth_sampler, uv + texel * vec2<f32>(x, y), 0.0).rgb;
}

fn luma(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// A group of four taps, weighted down by how bright it is (Karis), so that a single bright
// pixel, such as a glint, does not flicker as a large blob when it moves by a pixel.
fn karis(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>, d: vec3<f32>) -> vec4<f32> {
    let mean = (a + b + c + d) * 0.25;
    let weight = 1.0 / (1.0 + luma(mean));
    return vec4<f32>(mean * weight, weight);
}

// Thirteen taps over the source, to halve it smoothly (Jimenez, 2014): a box of four in the
// middle, and four overlapping boxes at the corners.
fn shrunk(uv: vec2<f32>, steady: bool) -> vec3<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    let a = tap(uv, texel, -2.0, -2.0);
    let b = tap(uv, texel, 0.0, -2.0);
    let c = tap(uv, texel, 2.0, -2.0);
    let d = tap(uv, texel, -1.0, -1.0);
    let e = tap(uv, texel, 1.0, -1.0);
    let f = tap(uv, texel, -2.0, 0.0);
    let g = tap(uv, texel, 0.0, 0.0);
    let h = tap(uv, texel, 2.0, 0.0);
    let i = tap(uv, texel, -1.0, 1.0);
    let j = tap(uv, texel, 1.0, 1.0);
    let k = tap(uv, texel, -2.0, 2.0);
    let l = tap(uv, texel, 0.0, 2.0);
    let m = tap(uv, texel, 2.0, 2.0);
    if (steady) {
        let groups = karis(d, e, i, j) * 0.5 + karis(a, b, f, g) * 0.125 + karis(b, c, g, h) * 0.125 + karis(f, g, k, l) * 0.125 + karis(g, h, l, m) * 0.125;
        return groups.rgb / groups.a;
    }
    return (d + e + i + j) * 0.125 + (a + b + f + g + b + c + g + h + f + g + k + l + g + h + l + m) * 0.03125;
}

// Only the light above the threshold, with a soft start (a quadratic knee).
fn bright_part(color: vec3<f32>) -> vec3<f32> {
    let brightness = max(color.r, max(color.g, color.b));
    let soft = clamp(brightness - params.threshold + params.knee, 0.0, 2.0 * params.knee);
    let curve = soft * soft / (4.0 * params.knee + 0.00001);
    let share = max(curve, brightness - params.threshold) / max(brightness, 0.00001);
    return color * share;
}

// The first step: the scene's bright light, at half size.
@fragment
fn fs_prefilter(input: Fullscreen) -> @location(0) vec4<f32> {
    // Half floats reach 65504, and a stray pixel must not light the screen.
    let color = min(shrunk(input.uv, true), vec3<f32>(256.0));
    return vec4<f32>(bright_part(color), 1.0);
}

// Each smaller copy.
@fragment
fn fs_downsample(input: Fullscreen) -> @location(0) vec4<f32> {
    return vec4<f32>(shrunk(input.uv, false), 1.0);
}

// Each copy grown back up, through a 3x3 tent, and added to the one above it.
@fragment
fn fs_upsample(input: Fullscreen) -> @location(0) vec4<f32> {
    let texel = params.radius / vec2<f32>(textureDimensions(source));
    let uv = input.uv;
    var sum = tap(uv, texel, 0.0, 0.0) * 4.0;
    sum += (tap(uv, texel, -1.0, 0.0) + tap(uv, texel, 1.0, 0.0) + tap(uv, texel, 0.0, -1.0) + tap(uv, texel, 0.0, 1.0)) * 2.0;
    sum += tap(uv, texel, -1.0, -1.0) + tap(uv, texel, 1.0, -1.0) + tap(uv, texel, -1.0, 1.0) + tap(uv, texel, 1.0, 1.0);
    return vec4<f32>(sum / 16.0, 1.0);
}

// The film curve, as the scene's shaders tone with it when bloom is off (shaders.wgsl, `film_tone`).
const TONE_KNEE: f32 = 0.6;

fn film_tone(color: vec3<f32>) -> vec3<f32> {
    let over = max(color - vec3<f32>(TONE_KNEE), vec3<f32>(0.0));
    let rolled = vec3<f32>(TONE_KNEE) + (1.0 - TONE_KNEE) * (vec3<f32>(1.0) - exp(-over / (1.0 - TONE_KNEE)));
    return select(color, rolled, color > vec3<f32>(TONE_KNEE));
}

// The scene and its glow, toned together into the window.
@fragment
fn fs_composite(input: Fullscreen) -> @location(0) vec4<f32> {
    let scene = textureSampleLevel(source, smooth_sampler, input.uv, 0.0).rgb;
    let light = textureSampleLevel(glow, smooth_sampler, input.uv, 0.0).rgb;
    return vec4<f32>(film_tone(scene + light * params.intensity), 1.0);
}
