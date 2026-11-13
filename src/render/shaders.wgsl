// All world positions arrive relative to the camera, which sits at the origin.

struct Globals {
    view_proj: mat4x4<f32>,
    // viewport width and height, then the size-to-distance ratios where units are fully dots and fully cubes.
    viewport_lod: vec4<f32>,
    // camera-relative position of the terrain grid's first vertex, then camera height, cell size.
    terrain_origin: vec4<f32>,
    // first sample, sample step, and samples per side.
    terrain_grid: vec4<i32>,
    // selection box in clip space: min x, min y, max x, max y.
    selection_box: vec4<f32>,
    // terrain height scale, display scale factor, fog texture coordinates per height sample, water level.
    terrain_scale: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var heights: texture_2d<i32>;
@group(0) @binding(2) var fog_texture: texture_2d<f32>;
@group(0) @binding(3) var fog_sampler: sampler;

const LIGHT: vec3<f32> = vec3<f32>(0.38, 0.27, 0.88);
const TERRAIN_GRID_SIZE: u32 = 512u;

// ---- Units, projectiles, and obstacles ----

// Materials, as the meshes tag their vertices.
const TRIM: u32 = 0u;
const HULL: u32 = 1u;
const METAL: u32 = 2u;
const GLOW: u32 = 3u;
// Dark gunmetal, the same for every unit.
const GUNMETAL: vec3<f32> = vec3<f32>(0.20, 0.21, 0.23);
// Light from the open sky above and from the ground below, and from the sun.
const SKY: vec3<f32> = vec3<f32>(0.42, 0.45, 0.52);
const GROUND_BOUNCE: vec3<f32> = vec3<f32>(0.20, 0.19, 0.16);
const SUN: vec3<f32> = vec3<f32>(0.80, 0.77, 0.70);

struct InstanceInput {
    @location(2) m0: vec4<f32>,
    @location(3) m1: vec4<f32>,
    @location(4) m2: vec4<f32>,
    @location(5) m3: vec4<f32>,
    @location(6) color: vec4<f32>,
    @location(7) hull: vec4<f32>,
    @location(8) glow: vec4<f32>,
};

struct UnitOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) albedo: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) relative: vec3<f32>,
    @location(3) @interpolate(flat) material: u32,
    // 0 for a lit shape, up to 1 for a flat tactical dot.
    @location(4) morph: f32,
};

// How far a shape has collapsed towards its tactical dot: 0 for a lit shape, 1 for a dot.
fn dot_morph(instance: InstanceInput, center: vec3<f32>, center_clip: vec4<f32>) -> f32 {
    let icon_px = instance.color.a * globals.terrain_scale.y;
    if (icon_px <= 0.0 || center_clip.w <= 0.0) {
        return 0.0;
    }
    let apparent_size = length(instance.m0.xyz) / max(length(center), 0.001);
    return 1.0 - smoothstep(globals.viewport_lod.z, globals.viewport_lod.w, apparent_size);
}

// Colors are chosen as they should look on screen, and lit in linear light.
fn to_linear(color: vec3<f32>) -> vec3<f32> {
    let low = color / 12.92;
    let high = pow((color + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, color <= vec3<f32>(0.04045));
}

@vertex
fn vs_unit(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(9) material: u32,
    instance: InstanceInput,
) -> UnitOutput {
    let model = mat4x4<f32>(instance.m0, instance.m1, instance.m2, instance.m3);
    let center = instance.m3.xyz;
    let icon_px = instance.color.a * globals.terrain_scale.y;
    let relative = (model * vec4<f32>(position, 1.0)).xyz;
    let cube_clip = globals.view_proj * vec4<f32>(relative, 1.0);
    let center_clip = globals.view_proj * vec4<f32>(center, 1.0);

    // Far units collapse to a screen-aligned dot with a minimum size. Rotating faces
    // flatten: the top and bottom become the dot's square, and the sides become lines.
    let morph = dot_morph(instance, center, center_clip);
    let corner = sign(position.xy);
    let offset = corner * icon_px * 2.0 / globals.viewport_lod.xy * center_clip.w;
    let dot_clip = vec4<f32>(center_clip.xy + offset, center_clip.z, center_clip.w);

    var albedo = instance.color.rgb;
    if (material == HULL) {
        albedo = instance.hull.rgb;
    } else if (material == GLOW) {
        albedo = instance.glow.rgb;
    }
    var linear = to_linear(albedo);
    if (material == METAL) {
        linear = GUNMETAL * GUNMETAL;
    }
    var out: UnitOutput;
    out.clip = mix(cube_clip, dot_clip, morph);
    // A dot is all team color.
    out.albedo = mix(linear, to_linear(instance.color.rgb), morph);
    out.normal = (model * vec4<f32>(normal, 0.0)).xyz;
    out.relative = relative;
    out.material = material;
    out.morph = morph;
    return out;
}

@fragment
fn fs_unit(input: UnitOutput) -> @location(0) vec4<f32> {
    if (input.material == GLOW || input.morph >= 1.0) {
        return vec4<f32>(input.albedo, 1.0);
    }
    let normal = normalize(input.normal);
    let to_eye = normalize(-input.relative);
    let sun = max(dot(normal, LIGHT), 0.0);
    let ambient = mix(GROUND_BOUNCE, SKY, normal.z * 0.5 + 0.5);
    var diffuse = input.albedo * (ambient + SUN * sun);
    // Metal shines sharply, paint softly.
    var shininess = 20.0;
    var strength = 0.18;
    if (input.material == METAL) {
        shininess = 48.0;
        strength = 0.35;
    }
    let halfway = normalize(LIGHT + to_eye);
    let shine = pow(max(dot(normal, halfway), 0.0), shininess) * strength * step(0.0, sun);
    // Edges facing away from the eye catch a little light, so that outlines show against the ground.
    let rim = pow(1.0 - max(dot(normal, to_eye), 0.0), 3.0) * 0.18;
    let lit = diffuse + vec3<f32>(shine) + rim * mix(input.albedo, vec3<f32>(1.0), 0.5);
    return vec4<f32>(mix(lit, input.albedo, input.morph), 1.0);
}

// ---- Shadows ----

// How much of the light a shadow takes away, and how far above the ground it is drawn, so
// that it stays in front of the ground it lies on.
const SHADOW_DARKNESS: f32 = 0.42;
const SHADOW_LIFT: f32 = 0.4;
// An aircraft's shadow fades as it climbs, down to this share, at this height.
const HIGH_SHADOW: f32 = 0.55;
const HIGH_SHADOW_HEIGHT: f32 = 600.0;

fn terrain_sample(sample: vec2<i32>) -> f32 {
    let last = globals.terrain_grid.w - 1;
    let clamped = clamp(sample, vec2<i32>(0, 0), vec2<i32>(last, last));
    return f32(textureLoad(heights, clamped, 0).r) * globals.terrain_scale.x;
}

// The camera-relative height of the ground under a camera-relative point, on the same
// triangles the terrain grid draws, or of the water where that is higher.
fn ground_at(relative: vec2<f32>) -> f32 {
    let step = globals.terrain_grid.z;
    let grid = (relative - globals.terrain_origin.xy) / (globals.terrain_origin.w * f32(step));
    let cell = floor(grid);
    let along = grid - cell;
    let first = globals.terrain_grid.xy + vec2<i32>(cell) * step;
    let a = terrain_sample(first);
    let b = terrain_sample(first + vec2<i32>(step, 0));
    let c = terrain_sample(first + vec2<i32>(0, step));
    let d = terrain_sample(first + vec2<i32>(step, step));
    // Each grid square is two triangles, split from its (1, 0) corner to its (0, 1) corner.
    var height = d + (c - d) * (1.0 - along.x) + (b - d) * (1.0 - along.y);
    if (along.x + along.y <= 1.0) {
        height = a + (b - a) * along.x + (c - a) * along.y;
    }
    return max(height, globals.terrain_scale.w) - globals.terrain_origin.z;
}

struct ShadowOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) darkness: f32,
};

// A shape flattened onto the ground along the sunlight, so that its shadow has its outline.
@vertex
fn vs_shadow(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    instance: InstanceInput,
) -> ShadowOutput {
    let model = mat4x4<f32>(instance.m0, instance.m1, instance.m2, instance.m3);
    let center = instance.m3.xyz;
    var out: ShadowOutput;
    // Shadows fade with the shapes as they become dots, and nothing under water casts one.
    let morph = dot_morph(instance, center, globals.view_proj * vec4<f32>(center, 1.0));
    let water = globals.terrain_scale.w - globals.terrain_origin.z;
    if (morph >= 1.0 || center.z < water) {
        out.clip = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        out.darkness = 0.0;
        return out;
    }
    let point = (model * vec4<f32>(position, 1.0)).xyz;
    let ground = ground_at(center.xy);
    let flat = point.xy - LIGHT.xy / LIGHT.z * max(point.z - ground, 0.0);
    out.clip = globals.view_proj * vec4<f32>(flat, ground_at(flat) + SHADOW_LIFT, 1.0);
    let height = clamp((center.z - ground) / HIGH_SHADOW_HEIGHT, 0.0, 1.0);
    out.darkness = SHADOW_DARKNESS * (1.0 - morph) * mix(1.0, HIGH_SHADOW, height);
    return out;
}

@fragment
fn fs_shadow(input: ShadowOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(vec3<f32>(1.0 - input.darkness), 1.0);
}

// ---- Shield domes ----

struct DomeOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) relative: vec3<f32>,
};

// Domes never shrink to dots, and their color's last component is how opaque they are.
@vertex
fn vs_dome(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    instance: InstanceInput,
) -> DomeOutput {
    let model = mat4x4<f32>(instance.m0, instance.m1, instance.m2, instance.m3);
    let relative = (model * vec4<f32>(position, 1.0)).xyz;
    var out: DomeOutput;
    out.clip = globals.view_proj * vec4<f32>(relative, 1.0);
    out.color = instance.color;
    out.normal = normalize((model * vec4<f32>(normal, 0.0)).xyz);
    out.relative = relative;
    return out;
}

@fragment
fn fs_dome(input: DomeOutput) -> @location(0) vec4<f32> {
    // Faint where it is seen face on, and bright at the rim, as a bubble looks.
    let facing = abs(dot(normalize(input.normal), normalize(-input.relative)));
    let rim = 1.0 - facing;
    let alpha = input.color.a * (0.2 + 1.6 * rim * rim);
    return vec4<f32>(input.color.rgb, clamp(alpha, 0.0, 0.85));
}

// ---- Terrain ----

struct TerrainOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) relative: vec3<f32>,
    @location(1) height: f32,
    @location(2) fog_coordinates: vec2<f32>,
};

@vertex
fn vs_terrain(@builtin(vertex_index) index: u32) -> TerrainOutput {
    let grid = vec2<i32>(i32(index % TERRAIN_GRID_SIZE), i32(index / TERRAIN_GRID_SIZE));
    let first = globals.terrain_grid.xy;
    let step = globals.terrain_grid.z;
    let last = globals.terrain_grid.w - 1;
    // Vertices beyond the map edge collapse onto it, so that the grid never leaves the map.
    let sample = clamp(first + grid * step, vec2<i32>(0, 0), vec2<i32>(last, last));
    let height = f32(textureLoad(heights, sample, 0).r) * globals.terrain_scale.x;
    let offset = vec2<f32>(sample - first) * globals.terrain_origin.w;
    let relative = vec3<f32>(globals.terrain_origin.xy + offset, height - globals.terrain_origin.z);

    var out: TerrainOutput;
    out.clip = globals.view_proj * vec4<f32>(relative, 1.0);
    out.relative = relative;
    out.height = height;
    out.fog_coordinates = vec2<f32>(sample) * globals.terrain_scale.z;
    return out;
}

@fragment
fn fs_terrain(input: TerrainOutput) -> @location(0) vec4<f32> {
    // Flat shading from screen-space derivatives.
    var normal = normalize(cross(dpdx(input.relative), dpdy(input.relative)));
    if (normal.z < 0.0) {
        normal = -normal;
    }
    let light = 0.55 + 0.45 * max(dot(normal, LIGHT), 0.0);
    let t = clamp((input.height + 36.0) / 72.0, 0.0, 1.0);
    let base = mix(vec3<f32>(0.10, 0.16, 0.11), vec3<f32>(0.30, 0.33, 0.20), t);
    // Height contours give a sense of shape and scale without any textures.
    let contour = abs(fract(input.height / 8.0) - 0.5);
    let width = max(fwidth(input.height / 8.0), 0.0001);
    let line = 1.0 - clamp(contour / width, 0.0, 1.0);
    // Ground that has never been seen is dark, and ground seen before is dim.
    let sight = 0.18 + 0.82 * textureSample(fog_texture, fog_sampler, input.fog_coordinates).r;
    var color = base * light * (1.0 - 0.18 * line);
    // Under the water level, ground shows through water, less the deeper it lies.
    let depth = globals.terrain_scale.w - input.height;
    if (depth > 0.0) {
        let murk = clamp(0.6 + depth / 80.0, 0.6, 0.92);
        color = mix(base * light, vec3<f32>(0.08, 0.22, 0.36), murk);
    }
    return vec4<f32>(color * sight, 1.0);
}

// ---- Selection box ----

@vertex
fn vs_box(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let corner = corners[index % 6u];
    let edge = index / 6u;
    let lo = globals.selection_box.xy;
    let hi = globals.selection_box.zw;
    let thickness = 3.0 / globals.viewport_lod.xy;
    var min_corner = lo;
    var max_corner = hi;
    if (edge == 0u) {
        max_corner.y = lo.y + thickness.y;
    } else if (edge == 1u) {
        min_corner.y = hi.y - thickness.y;
    } else if (edge == 2u) {
        max_corner.x = lo.x + thickness.x;
    } else {
        min_corner.x = hi.x - thickness.x;
    }
    return vec4<f32>(mix(min_corner, max_corner, corner), 1.0, 1.0);
}

@fragment
fn fs_box() -> @location(0) vec4<f32> {
    return vec4<f32>(0.55, 1.0, 0.55, 1.0);
}
