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
    // seconds of wall time, for waves, the terrain grid's samples a side, how strongly sight is
    // washed pale, then the quality: 0 for low, 1 for high.
    water: vec4<f32>,
    // The terrain's level of detail (tiles.rs): how far away a full-detail cell shrinks to a few
    // pixels on screen (0 for full detail everywhere), how near the water's edge the ground
    // stays at full detail and how far each coarser level after that is, and 1 if the shore
    // limits the detail at all.
    lod: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var heights: texture_2d<i32>;
@group(0) @binding(2) var fog_texture: texture_2d<f32>;
@group(0) @binding(3) var fog_sampler: sampler;
// How far each point is from the water's edge (shore.rs), sampled with the fog's sampler.
@group(0) @binding(4) var shore_texture: texture_2d<f32>;
// The small waves' slopes east and north (waves.rs), tiled.
@group(0) @binding(5) var wave_texture: texture_2d<f32>;
@group(0) @binding(6) var wave_sampler: sampler;
// How the light falls on the ground (land.rs): sunlight, openness to the sky, dampness. Sampled
// with the fog's sampler.
@group(0) @binding(7) var land_texture: texture_2d<f32>;
// The ground's close-up patterns (detail.rs): grass, soil, rock, sand. Tiled with the waves'
// sampler.
@group(0) @binding(8) var detail_texture: texture_2d<f32>;
// The ground's slopes east and north, its creases and its height (relief.rs), with smaller
// copies for the far distance. Sampled with the fog's sampler.
@group(0) @binding(10) var relief_texture: texture_2d<f32>;
// Each block of the terrain grid's level of detail this frame (tiles.rs).
@group(0) @binding(11) var block_levels: texture_2d<u32>;
// How far each block's ground strays from its true heights when drawn coarser, per step of
// detail, a texel a block at its middle (tiles.rs). Sampled with the fog's sampler.
@group(0) @binding(12) var stray_texture: texture_2d<f32>;

// The map's climate (climate.rs): the colors of its ground, water and sky, in linear light.
struct Climate {
    grass_low: vec4<f32>,
    grass_high: vec4<f32>,
    grass_dry: vec4<f32>,
    grass_lush: vec4<f32>,
    // Bare earth, and how much more of the ground is bare than in the temperate climate.
    dirt: vec4<f32>,
    dirt_damp: vec4<f32>,
    rock: vec4<f32>,
    rock_warm: vec4<f32>,
    sand: vec4<f32>,
    pebble: vec4<f32>,
    // Light from the open sky above and from the ground below, and from the sun.
    sky: vec4<f32>,
    ground_bounce: vec4<f32>,
    sun: vec4<f32>,
    // How much of each color water takes away per unit of depth.
    absorption: vec4<f32>,
    water_deep: vec4<f32>,
    // The sky the water reflects, at the horizon and overhead.
    sky_horizon: vec4<f32>,
    sky_zenith: vec4<f32>,
    // The haze far off in low views, and how thick it is.
    haze: vec4<f32>,
};
@group(0) @binding(9) var<uniform> climate: Climate;

// The wind, which the clouds drift with and which runs across the grass: from the
// south-west, along the long swells.
const WIND: vec2<f32> = vec2<f32>(0.83, 0.55);
// How far across a cloud's shadow is, in world units, how fast clouds drift, in units a
// second, and how much of the sun's light a cloud takes away.
const CLOUD_SIZE: f32 = 520.0;
const CLOUD_SPEED: f32 = 9.0;
const CLOUD_SHADE: f32 = 0.38;
// Cloud shadows fade out between these camera heights, so that far zoom stays quiet.
const CLOUD_FADE_NEAR: f32 = 2500.0;
const CLOUD_FADE_FAR: f32 = 7000.0;

// How much of the sun's light reaches a point on the map past the clouds: 1 in the clear,
// down to 1 - CLOUD_SHADE under a cloud.
fn cloud_light(world: vec2<f32>) -> f32 {
    let shown = 1.0 - smoothstep(CLOUD_FADE_NEAR, CLOUD_FADE_FAR, globals.terrain_origin.z);
    if (shown <= 0.0) {
        return 1.0;
    }
    let at = (world - WIND * globals.water.x * CLOUD_SPEED) / CLOUD_SIZE;
    var cover = value_noise(at);
    if (high_quality()) {
        cover = cover * 0.65 + value_noise(at * 2.3 + vec2<f32>(17.0, 5.0)) * 0.35;
    }
    return 1.0 - CLOUD_SHADE * smoothstep(0.5, 0.72, cover) * shown;
}

// How far off haze begins, how far beyond that it thickens over, in world units, and how much
// of the color it takes at most.
const HAZE_START: f32 = 1800.0;
const HAZE_DISTANCE: f32 = 7000.0;
const HAZE_MOST: f32 = 0.45;

// A color seen at camera-relative `relative`, through the haze: only looking low across the
// land, never from above, so that it gives orbit views depth and leaves the top-down view clear.
fn hazed(color: vec3<f32>, relative: vec3<f32>) -> vec3<f32> {
    let distance = length(relative);
    let down = -relative.z / max(distance, 0.001);
    let amount = (1.0 - exp(-max(distance - HAZE_START, 0.0) / HAZE_DISTANCE)) * (1.0 - smoothstep(0.2, 0.55, down)) * HAZE_MOST * climate.haze.w;
    return mix(color, climate.haze.rgb, clamp(amount, 0.0, 0.8));
}

// A gentle film curve: colors up to the knee stay as they are, and brighter ones roll off
// towards white instead of clipping, so that sun glints and foam keep their shape.
const TONE_KNEE: f32 = 0.6;

fn tone(color: vec3<f32>) -> vec3<f32> {
    let over = max(color - vec3<f32>(TONE_KNEE), vec3<f32>(0.0));
    let rolled = vec3<f32>(TONE_KNEE) + (1.0 - TONE_KNEE) * (vec3<f32>(1.0) - exp(-over / (1.0 - TONE_KNEE)));
    return select(color, rolled, color > vec3<f32>(TONE_KNEE));
}

fn high_quality() -> bool {
    return globals.water.w > 0.5;
}

// Where the sun shines from: high, from the east-south-east, a little behind the camera,
// which looks north, so that the faces of units and buildings that face it are lit, not only
// their tops. Until 2026-11-20 it shone from the north-east, behind them, and every wall facing
// the camera stood in shade.
const LIGHT: vec3<f32> = vec3<f32>(0.449, -0.249, 0.858);

// ---- Units, projectiles, and obstacles ----

// Materials, as the meshes tag their vertices.
const TRIM: u32 = 0u;
const HULL: u32 = 1u;
const METAL: u32 = 2u;
const GLOW: u32 = 3u;
const PANEL: u32 = 4u;
const GLASS: u32 = 5u;
// How much of the body's color a recessed panel keeps.
const PANEL_SHADE: f32 = 0.55;
// Glass: a dark blue-green tint, in linear light.
const GLASS_COLOR: vec3<f32> = vec3<f32>(0.012, 0.03, 0.045);
// How parts move, as flags above the material (meshes.rs, `motion`).
const MATERIAL_MASK: u32 = 0xffu;
const OPENNESS_SHIFT: u32 = 24u;
// How much of the sun's light crowded corners lose, beside the sky's.
const SUN_OCCLUSION: f32 = 0.6;
// How brightly edges facing away from the eye catch the light, close up and from further out,
// and the distances, in world units, between which it grows.
const RIM_NEAR: f32 = 0.18;
const RIM_FAR: f32 = 0.42;
const RIM_NEAR_DISTANCE: f32 = 300.0;
const RIM_FAR_DISTANCE: f32 = 1100.0;
const SPIN: u32 = 256u;
const SWING: u32 = 512u;
const RECOIL: u32 = 1024u;
const FLASH: u32 = 2048u;
const PULSE: u32 = 4096u;
const DOOR: u32 = 8192u;
const TAU: f32 = 6.2831853;
// How far a leg swings either way, in radians, and how far a gun kicks back, as a share of the unit.
const SWING_ANGLE: f32 = 0.45;
const RECOIL_DISTANCE: f32 = 0.09;
// A muzzle flash's color, in linear light, and how bright pulsing lights are while idle.
const FLASH_COLOR: vec3<f32> = vec3<f32>(1.0, 0.75, 0.3);
const IDLE_GLOW: f32 = 0.3;
// Dark gunmetal, the same for every unit.
const GUNMETAL: vec3<f32> = vec3<f32>(0.20, 0.21, 0.23);

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
    // How open the surface is to the sky, from the mesh: less where parts crowd one another.
    @location(5) openness: f32,
    // How much sun reaches the unit past the hills, and how open it is to the sky, from the
    // land map under it: the same for the whole unit.
    @location(6) land: vec2<f32>,
};

struct Animated {
    position: vec3<f32>,
    normal: vec3<f32>,
};

// Moves a vertex of a moving part, in the shape's own space, by the instance's phase (the
// hull color's last byte) and action (the glow's last byte).
fn animate(position: vec3<f32>, normal: vec3<f32>, flags: u32, pivot: vec3<f32>, instance: InstanceInput) -> Animated {
    let phase = instance.hull.a;
    let action = instance.glow.a;
    var point = position;
    var turned = normal;
    if ((flags & SPIN) != 0u) {
        let angle = phase * TAU;
        let c = cos(angle);
        let s = sin(angle);
        let arm = point.xy - pivot.xy;
        point = vec3<f32>(pivot.xy + vec2<f32>(arm.x * c - arm.y * s, arm.x * s + arm.y * c), point.z);
        turned = vec3<f32>(turned.x * c - turned.y * s, turned.x * s + turned.y * c, turned.z);
    }
    if ((flags & SWING) != 0u) {
        // Each leg swings about its hip, the left forward as the right goes back.
        let side = select(-1.0, 1.0, pivot.x > 0.0);
        let angle = sin(phase * TAU) * SWING_ANGLE * side;
        let c = cos(angle);
        let s = sin(angle);
        let arm = point.yz - pivot.yz;
        let swung = pivot.yz + vec2<f32>(arm.x * c - arm.y * s, arm.x * s + arm.y * c);
        point = vec3<f32>(point.x, swung.x, swung.y);
        turned = vec3<f32>(turned.x, turned.y * c - turned.z * s, turned.y * s + turned.z * c);
    }
    if ((flags & RECOIL) != 0u) {
        point.y -= action * RECOIL_DISTANCE;
    }
    if ((flags & FLASH) != 0u) {
        // Only in the first moments after a shot, and gone to nothing otherwise.
        point = pivot + (point - pivot) * smoothstep(0.55, 0.95, action);
    }
    if ((flags & DOOR) != 0u) {
        // A door's pivot is the way it slides, and how far when fully open.
        point = point + pivot * action;
    }
    return Animated(point, turned);
}

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
    @location(10) pivot: vec3<f32>,
    instance: InstanceInput,
) -> UnitOutput {
    let model = mat4x4<f32>(instance.m0, instance.m1, instance.m2, instance.m3);
    let center = instance.m3.xyz;
    let icon_px = instance.color.a * globals.terrain_scale.y;
    let kind = material & MATERIAL_MASK;
    let moved = animate(position, normal, material, pivot, instance);
    let relative = (model * vec4<f32>(moved.position, 1.0)).xyz;
    let cube_clip = globals.view_proj * vec4<f32>(relative, 1.0);
    let center_clip = globals.view_proj * vec4<f32>(center, 1.0);

    // Far units collapse to a screen-aligned dot with a minimum size. Rotating faces
    // flatten: the top and bottom become the dot's square, and the sides become lines.
    let morph = dot_morph(instance, center, center_clip);
    let corner = sign(position.xy);
    let offset = corner * icon_px * 2.0 / globals.viewport_lod.xy * center_clip.w;
    let dot_clip = vec4<f32>(center_clip.xy + offset, center_clip.z, center_clip.w);

    var albedo = instance.color.rgb;
    if (kind == HULL) {
        albedo = instance.hull.rgb;
    } else if (kind == PANEL) {
        albedo = instance.hull.rgb * PANEL_SHADE;
    } else if (kind == GLOW) {
        albedo = instance.glow.rgb;
    }
    var linear = to_linear(albedo);
    if (kind == METAL) {
        linear = GUNMETAL * GUNMETAL;
    } else if (kind == GLASS) {
        linear = GLASS_COLOR;
    }
    if ((material & FLASH) != 0u) {
        linear = FLASH_COLOR;
    } else if ((material & PULSE) != 0u) {
        linear = linear * mix(IDLE_GLOW, 1.0, instance.glow.a);
    }
    var out: UnitOutput;
    out.clip = mix(cube_clip, dot_clip, morph);
    // A dot is all team color.
    out.albedo = mix(linear, to_linear(instance.color.rgb), morph);
    out.normal = (model * vec4<f32>(moved.normal, 0.0)).xyz;
    out.relative = relative;
    out.material = kind;
    out.morph = morph;
    out.openness = f32(material >> OPENNESS_SHIFT) / 255.0;
    // A unit in a hill's shadow is in it too, as is the ground it stands on, and a unit in a
    // valley sees less of the sky. Aircraft fly above both.
    let bottom = center - instance.m2.xyz * 0.5;
    let land = land_at(world_from_relative(bottom.xy));
    let aloft = smoothstep(UNIT_ALOFT_LOW, UNIT_ALOFT_HIGH, bottom.z - ground_at(bottom.xy));
    out.land = vec2<f32>(mix(land.r, 1.0, aloft) * cloud_light(world_from_relative(center.xy)), mix(mix(1.0, land.g, 0.6), 1.0, aloft));
    return out;
}

@fragment
fn fs_unit(input: UnitOutput) -> @location(0) vec4<f32> {
    if (input.material == GLOW || input.morph >= 1.0) {
        return vec4<f32>(input.albedo, 1.0);
    }
    let normal = normalize(input.normal);
    let to_eye = normalize(-input.relative);
    let sun = max(dot(normal, LIGHT), 0.0) * input.land.x;
    let ambient = mix(climate.ground_bounce.rgb, climate.sky.rgb, normal.z * 0.5 + 0.5) * input.land.y;
    // Crowded corners lose most of the sky's light and some of the sun's, which sets parts
    // apart where they meet, as a stylized contact shadow.
    let open = input.openness;
    var diffuse = input.albedo * (ambient * open + climate.sun.rgb * sun * mix(1.0, open, SUN_OCCLUSION));
    // Metal shines sharply, paint softly.
    var shininess = 20.0;
    var strength = 0.18;
    if (input.material == METAL) {
        shininess = 48.0;
        strength = 0.35;
    } else if (input.material == GLASS) {
        shininess = 70.0;
        strength = 0.9;
    }
    let halfway = normalize(LIGHT + to_eye);
    let shine = pow(max(dot(normal, halfway), 0.0), shininess) * strength * step(0.0, sun);
    // Edges facing away from the eye catch a little light, so that outlines show against the
    // ground: more from further out, where units are small and the ground busy around them.
    let rim_strength = mix(RIM_NEAR, RIM_FAR, smoothstep(RIM_NEAR_DISTANCE, RIM_FAR_DISTANCE, length(input.relative)));
    let rim = pow(1.0 - max(dot(normal, to_eye), 0.0), 3.0) * rim_strength;
    let lit = diffuse + vec3<f32>(shine) + rim * mix(input.albedo, vec3<f32>(1.0), 0.5);
    return vec4<f32>(mix(tone(hazed(lit, input.relative)), input.albedo, input.morph), 1.0);
}

// How high above the ground a unit is out of the hills' shadow: partly from the first height,
// fully from the second.
const UNIT_ALOFT_LOW: f32 = 8.0;
const UNIT_ALOFT_HIGH: f32 = 40.0;

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

// The same, on the ground as the terrain draws it this frame, coarser far away, for what is
// laid on it: shadows and scars.
fn drawn_ground_at(relative: vec2<f32>) -> f32 {
    let size = i32(globals.water.y);
    let grid = (relative - globals.terrain_origin.xy) / (globals.terrain_origin.w * f32(globals.terrain_grid.z));
    let cell = clamp(vec2<i32>(floor(grid)), vec2<i32>(0, 0), vec2<i32>(size - 2, size - 2));
    let along = clamp(grid - vec2<f32>(cell), vec2<f32>(0.0), vec2<f32>(1.0));
    let b = drawn_height(cell + vec2<i32>(1, 0));
    let c = drawn_height(cell + vec2<i32>(0, 1));
    var height = 0.0;
    if (along.x + along.y <= 1.0) {
        let a = drawn_height(cell);
        height = a + (b - a) * along.x + (c - a) * along.y;
    } else {
        let d = drawn_height(cell + vec2<i32>(1, 1));
        height = d + (c - d) * (1.0 - along.x) + (b - d) * (1.0 - along.y);
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
    @location(9) material: u32,
    @location(10) pivot: vec3<f32>,
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
    // Moving parts cast their shadows where they are, and a flash casts none.
    if ((material & FLASH) != 0u) {
        out.clip = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        out.darkness = 0.0;
        return out;
    }
    let point = (model * vec4<f32>(animate(position, normal, material, pivot, instance).position, 1.0)).xyz;
    let ground = ground_at(center.xy);
    let flat = point.xy - LIGHT.xy / LIGHT.z * max(point.z - ground, 0.0);
    out.clip = globals.view_proj * vec4<f32>(flat, drawn_ground_at(flat) + SHADOW_LIFT, 1.0);
    let height = clamp((center.z - ground) / HIGH_SHADOW_HEIGHT, 0.0, 1.0);
    out.darkness = SHADOW_DARKNESS * (1.0 - morph) * mix(1.0, HIGH_SHADOW, height);
    return out;
}

@fragment
fn fs_shadow(input: ShadowOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(vec3<f32>(1.0 - input.darkness), 1.0);
}

// ---- Battle scars ----

struct ScarOutput {
    @builtin(position) clip: vec4<f32>,
    // Across the scar, from -1 to 1 either way.
    @location(0) across: vec2<f32>,
    // A crater (1) or a scorch mark (0), how strongly it shows, and a number that varies its shape.
    @location(1) @interpolate(flat) look: vec3<f32>,
    @location(2) fog_coordinates: vec2<f32>,
    // Where it is on the map.
    @location(3) world: vec2<f32>,
    // How wide and long it is, in world units.
    @location(4) @interpolate(flat) size: vec2<f32>,
};

// How dark fresh track marks are, as the share of the ground they cover.
const TRACK_DARKNESS: f32 = 0.32;
const SCORCH: vec3<f32> = vec3<f32>(0.035, 0.03, 0.025);
const CRATER_BOWL: vec3<f32> = vec3<f32>(0.05, 0.04, 0.03);
const THROWN_EARTH: vec3<f32> = vec3<f32>(0.19, 0.15, 0.10);
const SCAR_LIFT: f32 = 0.25;

// A flat square laid over the ground, bending with it, a little above it.
@vertex
fn vs_scar(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    instance: InstanceInput,
) -> ScarOutput {
    let model = mat4x4<f32>(instance.m0, instance.m1, instance.m2, instance.m3);
    let flat = (model * vec4<f32>(position, 1.0)).xy;
    var out: ScarOutput;
    out.clip = globals.view_proj * vec4<f32>(flat, drawn_ground_at(flat) + SCAR_LIFT, 1.0);
    out.across = position.xy * 2.0;
    out.look = instance.color.rgb;
    let sample = (flat - globals.terrain_origin.xy) / globals.terrain_origin.w + vec2<f32>(globals.terrain_grid.xy);
    out.fog_coordinates = sample * globals.terrain_scale.z;
    out.world = world_from_relative(flat);
    out.size = vec2<f32>(length(instance.m0.xy), length(instance.m1.xy));
    return out;
}

@fragment
fn fs_scar(input: ScarOutput) -> @location(0) vec4<f32> {
    let sight = 0.18 + 0.82 * textureSample(fog_texture, fog_sampler, input.fog_coordinates).r;
    // Wakes and slicks, the fourth and fifth kinds, are drawn over the water instead, by fs_wake.
    if (input.look.x > 2.5) {
        discard;
    }
    let seed = input.look.z * 37.0;
    // A ragged edge, never quite round.
    let ragged = (value_noise(input.across * 2.5 + vec2<f32>(seed, seed * 0.7)) - 0.5) * 0.3;
    let reach = length(input.across) + ragged;
    var color = SCORCH;
    var alpha = 0.0;
    if (input.look.x > 1.5) {
        // Track marks: two dark bands of churned earth along the way the vehicle went, faint
        // and broken a little, fading out at either end of the stretch so that stretches join.
        let side = abs(input.across.x);
        let band = smoothstep(0.32, 0.42, side) * (1.0 - smoothstep(0.72, 0.84, side));
        let along = 1.0 - smoothstep(0.45, 0.8, abs(input.across.y));
        let broken = 0.7 + 0.3 * value_noise(input.across * vec2<f32>(3.0, 9.0));
        return vec4<f32>(CRATER_BOWL * sight, band * along * broken * TRACK_DARKNESS * input.look.y);
    }
    if (input.look.x < 0.5) {
        // Blackened ground, solid in the middle and streaked outwards towards its edge.
        let direction = input.across / max(length(input.across), 0.001);
        let streaks = value_noise(direction * 2.2 + vec2<f32>(seed, -seed));
        let edge = smoothstep(0.2, 0.9, reach);
        alpha = (1.0 - smoothstep(0.45 + 0.35 * streaks, 1.0, reach)) * mix(0.8, 0.45 + 0.4 * streaks, edge);
    } else {
        // A dark bowl ringed with lighter earth thrown up out of it, and dark spatter beyond.
        let bowl = 1.0 - smoothstep(0.3, 0.5, reach);
        let rim = smoothstep(0.4, 0.54, reach) * (1.0 - smoothstep(0.58, 0.74, reach));
        let spatter = (1.0 - smoothstep(0.7, 1.0, reach)) * 0.25;
        color = mix(CRATER_BOWL, THROWN_EARTH, rim / max(bowl + rim, 0.001));
        alpha = max(bowl * 0.72, max(rim * 0.4, spatter));
    }
    return vec4<f32>(color * sight, alpha * input.look.y);
}

// How wide each arm of foam in a ship's wake is when new, in world units, and how much wider
// the wake gets for each unit behind the ship, as the effects set it (`WAKE_SPREAD`).
const WAKE_ARM: f32 = 12.0;
const WAKE_SPREAD: f32 = 0.5;

// A ship's wake, laid flat on the water after it is drawn: two thin arms of foam along its
// edges, which open out into a V as the stretches behind the ship spread, and churned water
// down the middle that settles first. Its look is the fourth kind of scar, how strongly it
// shows, and how far through its life it is.
@fragment
fn fs_wake(input: ScarOutput) -> @location(0) vec4<f32> {
    let fog = textureSample(fog_texture, fog_sampler, input.fog_coordinates).r;
    if (input.look.x < 2.5) {
        discard;
    }
    if (input.look.x > 3.5) {
        return oil_slick(input, fog);
    }
    let age = input.look.z;
    // Each stretch is drawn wider than the wake is where it lies, and narrows towards the ship,
    // as the wake does, so that the edges of one stretch meet those of the next in a straight
    // line instead of a step.
    let ahead = input.across.y * 0.5 * input.size.y;
    let width = input.size.x - WAKE_SPREAD * (0.5 * input.size.y + ahead);
    let side = abs(input.across.x) * input.size.x / max(width, 0.001);
    // The arms are foam a few units wide along either edge, however far the wake has spread,
    // widening a little as they break up.
    let from_edge = (1.0 - side) * width * 0.5;
    let arm = WAKE_ARM * (1.0 + age);
    let arms = smoothstep(0.0, arm * 0.25, from_edge) * (1.0 - smoothstep(arm * 0.35, arm, from_edge));
    let middle = (1.0 - smoothstep(0.1, 0.4, side)) * (1.0 - smoothstep(0.0, 0.12, age));
    // Stretches overlap and fade at either end, so that they join into one trail.
    let along = 1.0 - smoothstep(0.5, 1.0, abs(input.across.y));
    let broken = smoothstep(0.25, 0.75, value_noise(input.world / 7.0 + vec2<f32>(age * 6.0, 0.0)) * 0.7 + value_noise(input.world / 2.5) * 0.3);
    let alpha = (arms * 0.65 * (1.0 - age * 0.6) + middle * 0.55) * mix(0.35, 1.0, broken) * along * input.look.y;
    let sight = 0.18 + 0.82 * fog;
    let lit = FOAM_COLOR * mix(1.0, cloud_light(input.world), 0.5) * sight;
    return vec4<f32>(tone(strategic_sight(lit, fog, input.fog_coordinates)), alpha);
}

// How dark an oil slick is, and the colors of the bands where it thins out at its edge.
const OIL: vec3<f32> = vec3<f32>(0.018, 0.02, 0.024);
const FLOTSAM: vec3<f32> = vec3<f32>(0.2, 0.17, 0.13);

// A slick of oil on the water over a wreck on the sea floor, the fifth kind of scar: a dark,
// glossy film, ragged at its edge and slowly changing shape, with faint bands of color where it
// thins out, and scraps of debris afloat in it. Its look is how strongly it shows and a number
// that varies its shape. It marks where a wreck lies, as the water hides the floor.
fn oil_slick(input: ScarOutput, fog: f32) -> vec4<f32> {
    let time = globals.water.x;
    let seed = input.look.z * 37.0;
    let drift = vec2<f32>(time * 0.013, -time * 0.009);
    let ragged = (value_noise(input.across * 1.7 + vec2<f32>(seed, -seed) + drift) - 0.5) * 0.5
        + (value_noise(input.across * 4.3 - vec2<f32>(seed, seed) - drift * 2.0) - 0.5) * 0.18;
    let reach = length(input.across) + ragged;
    let film = 1.0 - smoothstep(0.5, 0.95, reach);
    // Bands of color where the film thins, towards its edge.
    let thickness = reach * 7.0 + value_noise(input.world / 6.0 + drift * 4.0) * 3.0 + time * 0.05;
    let bands = 0.5 + 0.5 * cos(vec3<f32>(thickness) + vec3<f32>(0.0, 2.1, 4.2));
    let thin = smoothstep(0.3, 0.85, reach);
    var color = mix(OIL, bands * 0.28 + OIL, thin * 0.7);
    // Scraps of debris afloat in the middle of it.
    let scraps = step(0.86, value_noise(input.world / 1.4 + vec2<f32>(seed * 3.0))) * (1.0 - smoothstep(0.25, 0.6, reach));
    color = mix(color, FLOTSAM, scraps);
    let alpha = max(film * mix(0.62, 0.3, thin), scraps * 0.85) * input.look.y;
    let sight = 0.18 + 0.82 * fog;
    let lit = color * mix(1.0, cloud_light(input.world), 0.5) * sight;
    return vec4<f32>(tone(strategic_sight(lit, fog, input.fog_coordinates)), alpha);
}

// ---- Smoke ----

struct SmokeOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) albedo: vec3<f32>,
    @location(2) opacity: f32,
    @location(3) relative: vec3<f32>,
};

// Lit softly like the units, and see-through by how opaque the puff is, which the hull
// color's last byte carries.
@vertex
fn vs_smoke(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    instance: InstanceInput,
) -> SmokeOutput {
    let model = mat4x4<f32>(instance.m0, instance.m1, instance.m2, instance.m3);
    var out: SmokeOutput;
    let relative = (model * vec4<f32>(position, 1.0)).xyz;
    out.clip = globals.view_proj * vec4<f32>(relative, 1.0);
    out.normal = (model * vec4<f32>(normal, 0.0)).xyz;
    out.albedo = to_linear(instance.color.rgb);
    out.opacity = instance.hull.a;
    out.relative = relative;
    return out;
}

@fragment
fn fs_smoke(input: SmokeOutput) -> @location(0) vec4<f32> {
    let normal = normalize(input.normal);
    let ambient = mix(climate.ground_bounce.rgb, climate.sky.rgb, normal.z * 0.5 + 0.5);
    let lit = input.albedo * (ambient + climate.sun.rgb * 0.8 * max(dot(normal, LIGHT), 0.0));
    // Thinner towards its edges, as a cloud is, so that its facets do not show.
    let facing = abs(dot(normal, normalize(-input.relative)));
    return vec4<f32>(lit, input.opacity * smoothstep(0.05, 0.6, facing));
}

// ---- Shield domes ----

// How many hexagons fit across a dome, and the share of its strength each part shows: almost
// clear where it is seen face on, faint hexagon lines, and a thin bright rim.
const HEX_ACROSS: f32 = 16.0;
const DOME_FACE: f32 = 0.04;
const DOME_HEX: f32 = 0.3;
const DOME_RIM: f32 = 1.5;
const DOME_RIM_SHARPNESS: f32 = 6.0;
// A dome fades out as the camera comes close, from this many radii from its center to this many.
const DOME_FADE_FAR: f32 = 1.5;
const DOME_FADE_NEAR: f32 = 1.05;
// Its shimmer (2026-11-25): soft glints of light that fade in and out at spots over it, each on
// a cycle of its own, then come up somewhere else, and a slow drift in the hexagons' brightness,
// so that no dome is ever one even texture. How many glints, on high quality and low; how long
// a glint's cycle may be, in seconds; how tight a spot each is (higher is smaller); how bright
// on the face, and how much more on the hexagon lines; and how much more often they come on a
// dome worn to nothing than on a full one.
const DOME_GLINTS_HIGH: u32 = 6u;
const DOME_GLINTS_LOW: u32 = 3u;
const DOME_GLINT_SHORTEST: f32 = 2.0;
const DOME_GLINT_LONGEST: f32 = 4.0;
const DOME_GLINT_TIGHTNESS: f32 = 40.0;
const DOME_GLINT: f32 = 0.5;
const DOME_GLINT_HEX: f32 = 3.0;
const DOME_GLINT_WORN: f32 = 1.8;
const DOME_DRIFT: f32 = 0.25;

struct DomeOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) @interpolate(flat) sphere_center: vec3<f32>,
    @location(2) relative: vec3<f32>,
    // Across the dome from above, from -0.5 to 0.5.
    @location(3) across: vec2<f32>,
    @location(4) @interpolate(flat) fade: f32,
    // A number of its own, so that domes side by side do not shimmer in step, and how much of
    // its strength is left, from 0 to 1.
    @location(5) @interpolate(flat) seed: u32,
    @location(6) @interpolate(flat) health: f32,
};

// A well-mixed number from another (the PCG hash).
fn hash_u32(value: u32) -> u32 {
    let state = value * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// A number from 0 to 1 from three others.
fn hash01(a: u32, b: u32, c: u32) -> f32 {
    return f32(hash_u32(a ^ hash_u32(b ^ hash_u32(c))) >> 8u) / 16777216.0;
}

// How bright a dome's glints are where its surface faces `normal`, at `time` seconds: each fades
// in and out over its cycle, centered on a spot picked anew each cycle from the dome's seed.
fn dome_glints(normal: vec3<f32>, seed: u32, health: f32, time: f32, count: u32) -> f32 {
    var total = 0.0;
    let hurry = mix(DOME_GLINT_WORN, 1.0, health);
    for (var glint = 0u; glint < count; glint = glint + 1u) {
        let period = mix(DOME_GLINT_SHORTEST, DOME_GLINT_LONGEST, hash01(seed, glint, 0x51u)) / hurry;
        let along = time / period + hash01(seed, glint, 0x52u);
        let cycle = u32(i32(floor(along)) & 0x7fffffff);
        let phase = fract(along);
        // Anywhere on the dome above its lowest tenth.
        let up = mix(0.1, 0.95, hash01(seed, glint * 97u + 1u, cycle));
        let around = TAU * hash01(seed, glint * 97u + 2u, cycle);
        let side = sqrt(1.0 - up * up);
        let spot = vec3<f32>(side * cos(around), side * sin(around), up);
        let swell = sin(phase * 3.1415927);
        total = total + swell * swell * exp((dot(normal, spot) - 1.0) * DOME_GLINT_TIGHTNESS);
    }
    return total;
}

// Domes never shrink to dots, and their color's last component is how opaque they are.
@vertex
fn vs_dome(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    instance: InstanceInput,
) -> DomeOutput {
    let model = mat4x4<f32>(instance.m0, instance.m1, instance.m2, instance.m3);
    let relative = (model * vec4<f32>(position, 1.0)).xyz;
    // The sphere's center is on the ground, at the bottom of the shape's box.
    let radius = length(instance.m0.xyz) * 0.5;
    let sphere_center = instance.m3.xyz - instance.m2.xyz * 0.5;
    var out: DomeOutput;
    out.clip = globals.view_proj * vec4<f32>(relative, 1.0);
    out.color = instance.color;
    out.sphere_center = sphere_center;
    out.relative = relative;
    out.across = position.xy;
    out.fade = smoothstep(DOME_FADE_NEAR, DOME_FADE_FAR, length(sphere_center) / max(radius, 0.001));
    // The hull color carries the dome's seed in its first three bytes and its health in its last.
    let bytes = vec3<u32>(round(instance.hull.rgb * 255.0));
    out.seed = bytes.x | (bytes.y << 8u) | (bytes.z << 16u);
    out.health = instance.hull.a;
    return out;
}

// How far a point is from the nearest edge of a grid of hexagons one unit across: 0 on an
// edge, up to 0.5 in the middle.
fn hex_edge_distance(point: vec2<f32>) -> f32 {
    let repeat = vec2<f32>(1.0, 1.7320508);
    let half_repeat = repeat * 0.5;
    let first = point - repeat * floor(point / repeat) - half_repeat;
    let shifted = point - half_repeat;
    let second = shifted - repeat * floor(shifted / repeat) - half_repeat;
    let nearest = select(second, first, dot(first, first) < dot(second, second));
    let folded = abs(nearest);
    return 0.5 - max(dot(folded, vec2<f32>(0.5, 0.8660254)), folded.x);
}

@fragment
fn fs_dome(input: DomeOutput) -> @location(0) vec4<f32> {
    // Clear where it is seen face on, and bright at the rim, as a bubble looks. The normal is
    // the true sphere's, so that the rim is smooth across the mesh's flat faces.
    let normal = normalize(input.relative - input.sphere_center);
    let facing = abs(dot(normal, normalize(-input.relative)));
    let rim = pow(1.0 - facing, DOME_RIM_SHARPNESS);
    let point = input.across * HEX_ACROSS;
    let blur = length(fwidth(point));
    var hex = 1.0 - smoothstep(0.0, max(blur * 1.2, 0.02), hex_edge_distance(point));
    // Lines closer together than a few pixels would shimmer, so they fade out first.
    hex = hex * (1.0 - smoothstep(0.15, 0.35, blur));
    // The shimmer: glints, and on high quality a slow drift of brightness across the lines.
    let time = globals.water.x;
    let high = globals.water.w > 0.5;
    let glint = min(dome_glints(normal, input.seed, input.health, time, select(DOME_GLINTS_LOW, DOME_GLINTS_HIGH, high)), 1.5);
    var drift = 0.0;
    if (high) {
        let drifting = 0.5 * sin(dot(normal, vec3<f32>(4.1, 2.3, 1.7)) + time * 0.45) + 0.5 * sin(dot(normal, vec3<f32>(-1.9, 3.7, 2.9)) - time * 0.31);
        drift = DOME_DRIFT * drifting;
    }
    hex = hex * (1.0 + drift) * (1.0 + DOME_GLINT_HEX * glint);
    let alpha = input.color.a * (DOME_FACE + DOME_HEX * hex + DOME_RIM * rim + DOME_GLINT * glint) * input.fade;
    // Where light runs over it, it is whiter.
    let color = mix(input.color.rgb, vec3<f32>(1.0), 0.45 * min(glint, 1.0));
    return vec4<f32>(color, clamp(alpha, 0.0, 0.8));
}

// ---- Terrain ----

struct TerrainOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) relative: vec3<f32>,
    // The ground's true height, for the colors that follow it: where the ground is drawn
    // coarser, its drawn height strays from it.
    @location(1) height: f32,
    @location(2) fog_coordinates: vec2<f32>,
    // Where it is on the map, for patterns that stay put as the camera moves.
    @location(3) world: vec2<f32>,
    // The ground's normal, then how far the ground around rises above it: above 0 in hollows
    // and creases, below 0 on ridges and crests (`surface_of`).
    @location(4) surface: vec4<f32>,
};

// A grid vertex of the terrain, for a grid index: which height sample it stands on, and where.
struct GridPoint {
    sample: vec2<i32>,
    offset: vec2<f32>,
};

fn grid_point(index: u32) -> GridPoint {
    // The grid's samples a side: the whole map's, up to its limit.
    let size = u32(globals.water.y);
    let grid = vec2<i32>(i32(index % size), i32(index / size));
    let first = globals.terrain_grid.xy;
    let last = globals.terrain_grid.w - 1;
    // Vertices beyond the map edge collapse onto it, so that the grid never leaves the map.
    let sample = clamp(first + grid * globals.terrain_grid.z, vec2<i32>(0, 0), vec2<i32>(last, last));
    return GridPoint(sample, vec2<f32>(sample - first) * globals.terrain_origin.w);
}

// ---- Level of detail (tiles.rs) ----

// Grid cells a side in a block, and the coarsest level: a vertex every 2^4 samples.
const BLOCK_CELLS: i32 = 16;
const MAX_LEVEL: f32 = 4.0;

// A level of detail with its steps held: whole levels hold for the first half of each step and
// blend to the next over the second. As `shaped_level` in tiles.rs.
fn shaped_level(raw: f32) -> f32 {
    if (raw <= 0.0) {
        return 0.0;
    }
    let whole = floor(raw);
    return min(whole + clamp((raw - whole) * 2.0 - 1.0, 0.0, 1.0), MAX_LEVEL);
}

// The height sample a grid vertex stands on.
fn grid_sample(grid: vec2<i32>) -> vec2<i32> {
    let last = globals.terrain_grid.w - 1;
    return clamp(globals.terrain_grid.xy + grid * globals.terrain_grid.z, vec2<i32>(0, 0), vec2<i32>(last, last));
}

// How coarse the ground is drawn at a grid vertex standing `height` high: 0 for every sample,
// rising by one each time the distance from the eye doubles past where the ground would stray
// from its true heights by `LOD_ERROR_PIXELS` on screen at the next level, and held down near
// the water's edge, so that the coast keeps its shape. As `plan_blocks` in tiles.rs.
fn vertex_level(grid: vec2<i32>, height: f32) -> f32 {
    if (globals.lod.x <= 0.0) {
        return 0.0;
    }
    let sample = grid_sample(grid);
    let relative = vec3<f32>(vec2<f32>(sample - globals.terrain_grid.xy) * globals.terrain_origin.w + globals.terrain_origin.xy, height - globals.terrain_origin.z);
    let blocks = f32(textureDimensions(stray_texture).x);
    let strays = textureSampleLevel(stray_texture, fog_sampler, vec2<f32>(grid) / (f32(BLOCK_CELLS) * blocks), 0.0).r;
    var raw = log2(max(length(relative), 0.001) / max(globals.lod.x * strays, 0.001));
    if (raw <= 0.0) {
        return 0.0;
    }
    if (globals.lod.w > 0.0) {
        let from_edge = abs(shore_distance(vec2<f32>(sample) * globals.terrain_origin.w));
        raw = min(raw, (from_edge - globals.lod.y) / globals.lod.z);
    }
    return shaped_level(raw);
}

// The height at a grid vertex of the ground drawn at `level`, a vertex every 2^level samples:
// on the coarser grid's triangles, split as the full grid's are, so that the finer cells laid
// on it match it.
fn level_height(grid: vec2<i32>, level: i32) -> f32 {
    if (level == 0) {
        return terrain_sample(grid_sample(grid));
    }
    let spacing = 1i << u32(level);
    let corner = (grid / spacing) * spacing;
    let along = vec2<f32>(grid - corner) / f32(spacing);
    let b = terrain_sample(grid_sample(corner + vec2<i32>(spacing, 0)));
    let c = terrain_sample(grid_sample(corner + vec2<i32>(0, spacing)));
    if (along.x + along.y <= 1.0) {
        let a = terrain_sample(grid_sample(corner));
        return a + (b - a) * along.x + (c - a) * along.y;
    }
    let d = terrain_sample(grid_sample(corner + vec2<i32>(spacing, spacing)));
    return d + (c - d) * (1.0 - along.x) + (b - d) * (1.0 - along.y);
}

// The ground at a grid vertex as its level of detail has it: its height, blended smoothly
// towards each coarser level's as the camera draws away, so that nothing pops when a block
// changes level, then the level. It depends only on where the vertex is, so every block that
// draws it agrees.
fn level_of_detail(grid: vec2<i32>) -> vec2<f32> {
    var height = terrain_sample(grid_sample(grid));
    let level = vertex_level(grid, height);
    if (level > 0.0) {
        let whole = i32(floor(level));
        let blend = level - f32(whole);
        height = level_height(grid, whole);
        if (blend > 0.0) {
            height = mix(height, level_height(grid, whole + 1), blend);
        }
    }
    return vec2<f32>(height, level);
}

fn block_level(block: vec2<i32>) -> i32 {
    return i32(textureLoad(block_levels, block, 0).r);
}

// The ground as the terrain draws it at a grid vertex: its height, then its level of detail.
// Along an edge between two blocks, the coarser block has fewer vertices: those the finer one
// has between them are laid on the straight line between its, so that no crack opens between
// the two.
fn drawn_vertex(grid: vec2<i32>) -> vec2<f32> {
    let on_edge = (grid % vec2<i32>(BLOCK_CELLS)) == vec2<i32>(0, 0);
    if (on_edge.x != on_edge.y) {
        // Across the edge, and along it.
        let across = select(1, 0, on_edge.x);
        let along = 1 - across;
        let blocks = textureDimensions(block_levels).x;
        var block = grid / BLOCK_CELLS;
        var level = 0;
        if (block[across] < i32(blocks)) {
            level = block_level(block);
        }
        block[across] = block[across] - 1;
        if (block[across] >= 0) {
            level = max(level, block_level(block));
        }
        let spacing = 1i << u32(level);
        let offset = grid[along] % spacing;
        if (offset != 0) {
            var first = grid;
            first[along] = grid[along] - offset;
            var second = first;
            second[along] = first[along] + spacing;
            return mix(level_of_detail(first), level_of_detail(second), f32(offset) / f32(spacing));
        }
    }
    return level_of_detail(grid);
}

fn drawn_height(grid: vec2<i32>) -> f32 {
    return drawn_vertex(grid).x;
}

// The ground's slopes, creases and height (relief.rs) at a point on the map, from the relief
// field's copy `level` sizes down.
fn relief_at(world: vec2<f32>, level: f32) -> vec4<f32> {
    return textureSampleLevel(relief_texture, fog_sampler, map_coordinates(world, textureDimensions(relief_texture).x), level);
}

// The ground's normal from a texel of the relief field, then its creases, for the light.
// Vertices pass the normal on, as they always have: blending normals across a triangle shades a
// cliff's foot and brow otherwise than blending slopes would.
fn surface_of(relief: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(normalize(vec3<f32>(-relief.xy, 1.0)), relief.z);
}

@vertex
fn vs_terrain(@builtin(vertex_index) index: u32) -> TerrainOutput {
    let point = grid_point(index);
    let size = u32(globals.water.y);
    let ground = drawn_vertex(vec2<i32>(i32(index % size), i32(index / size)));
    let relative = vec3<f32>(globals.terrain_origin.xy + point.offset, ground.x - globals.terrain_origin.z);
    var out: TerrainOutput;
    out.clip = globals.view_proj * vec4<f32>(relative, 1.0);
    out.relative = relative;
    out.height = terrain_sample(point.sample);
    out.fog_coordinates = vec2<f32>(point.sample) * globals.terrain_scale.z;
    out.world = vec2<f32>(point.sample) * globals.terrain_origin.w;
    // The field's texels lie on the samples, so this is the slope and creases at the sample.
    out.surface = surface_of(relief_at(out.world, 0.0));
    return out;
}

// A number from 0 to 1 for a whole-numbered cell, from integer arithmetic only (a PCG-style
// hash), so that it is as good at the far side of an 81 km map as at its origin. Until
// 2026-11-25 it was `fract(sin(dot(cell, (127.1, 311.7))) * 43758.5)`, whose argument to `sin`
// reached about 10^7 on big maps, where floats are a radian or more apart.
fn hash2(cell: vec2<f32>) -> f32 {
    var v = bitcast<vec2<u32>>(vec2<i32>(floor(cell)));
    v = v * 1664525u + vec2<u32>(1013904223u);
    v.x = v.x + v.y * 1664525u;
    v.y = v.y + v.x * 1664525u;
    v = v ^ (v >> vec2<u32>(16u));
    v.x = v.x + v.y * 1664525u;
    v.y = v.y + v.x * 1664525u;
    v = v ^ (v >> vec2<u32>(16u));
    return f32(v.x >> 8u) / 16777216.0;
}

// Smooth noise from 0 to 1, changing over about one unit of `point`, then how fast it changes
// along x and along y.
fn value_noise_slope(point: vec2<f32>) -> vec3<f32> {
    let cell = floor(point);
    let along = fract(point);
    let smooth_along = along * along * (3.0 - 2.0 * along);
    let slope_along = 6.0 * along * (1.0 - along);
    let a = hash2(cell);
    let b = hash2(cell + vec2<f32>(1.0, 0.0));
    let c = hash2(cell + vec2<f32>(0.0, 1.0));
    let d = hash2(cell + vec2<f32>(1.0, 1.0));
    let twist = a - b - c + d;
    let value = a + (b - a) * smooth_along.x + (c - a) * smooth_along.y + twist * smooth_along.x * smooth_along.y;
    let slope = vec2<f32>(slope_along.x * ((b - a) + twist * smooth_along.y), slope_along.y * ((c - a) + twist * smooth_along.x));
    return vec3<f32>(value, slope);
}

fn value_noise(point: vec2<f32>) -> f32 {
    return value_noise_slope(point).x;
}

// How strongly the sky and the sun light the ground, so that flat open ground in the sun is
// about as bright as it was before the sky had a color.
const GROUND_AMBIENT: f32 = 0.85;
const GROUND_SUN: f32 = 0.95;
// How much the wind's bands lighten and darken the grass.
const GRASS_WIND: f32 = 0.16;
// World units one copy of the close-up patterns spans.
const DETAIL_SIZE: f32 = 7.0;

// Where a point on the map is in a texture `side` texels a side whose first and last texels lie
// on the map's edges.
fn map_coordinates(world: vec2<f32>, side: u32) -> vec2<f32> {
    let map = f32(globals.terrain_grid.w - 1) * globals.terrain_origin.w;
    return (world / map * (f32(side) - 1.0) + 0.5) / f32(side);
}

// The land map (land.rs) at a point on the map: sunlight, openness, dampness.
fn land_at(world: vec2<f32>) -> vec3<f32> {
    let side = f32(textureDimensions(land_texture).x);
    let map = f32(globals.terrain_grid.w - 1) * globals.terrain_origin.w;
    let coordinates = (world / map * (side - 1.0) + 0.5) / side;
    return textureSampleLevel(land_texture, fog_sampler, coordinates, 0.0).rgb;
}

// Where a camera-relative point is on the map.
fn world_from_relative(relative: vec2<f32>) -> vec2<f32> {
    return relative - globals.terrain_origin.xy + vec2<f32>(globals.terrain_grid.xy) * globals.terrain_origin.w;
}

// The ground in blocks drawn at full detail, lit and colored from its vertices, which stand on
// every sample, so that it is just as the full grid draws it, even as they morph towards the
// next level's heights.
@fragment
fn fs_terrain(input: TerrainOutput) -> @location(0) vec4<f32> {
    return shade_terrain(input, input.surface, input.height);
}

// The ground in blocks drawn coarser (tiles.rs), whose vertices are too far apart to light and
// color it by: a cliff a cell wide would vanish between them. The light and the height for
// colors come from the relief field at each pixel instead, which does not change with the grid,
// so nothing pops when a block changes level. While its texels are over half a pixel across,
// it is laid on the full-detail grid's triangles, just as that grid's vertices would light it;
// beyond, it comes from the field's copy whose texels are about half a pixel across, so that it
// does not shimmer.
@fragment
fn fs_terrain_far(input: TerrainOutput) -> @location(0) vec4<f32> {
    // Across the screen's narrower footprint, so that ground seen at a slant stays sharp.
    let across = min(length(dpdx(input.world)), length(dpdy(input.world)));
    let level = log2(across / relief_spacing()) - 1.0;
    var ground: Ground;
    if (level <= 0.0) {
        ground = ground_on_grid(input.world);
    } else {
        let relief = relief_at(input.world, level);
        ground = Ground(surface_of(relief), relief.w);
        if (level < 1.0) {
            let grid = ground_on_grid(input.world);
            ground = Ground(mix(grid.surface, ground.surface, level), mix(grid.height, ground.height, level));
        }
    }
    return shade_terrain(input, ground.surface, ground.height);
}

// The ground for the light and colors: its normal and creases (`surface_of`), and its height.
struct Ground {
    surface: vec4<f32>,
    height: f32,
};

// World units between the relief field's texels.
fn relief_spacing() -> f32 {
    return f32(globals.terrain_grid.w - 1) * globals.terrain_origin.w / f32(textureDimensions(relief_texture).x - 1u);
}

// The ground at a point on the map as the full-detail grid draws it: from the relief field's
// texels at its triangle's corners, as the grid's vertices take them, across the triangle.
fn ground_on_grid(world: vec2<f32>) -> Ground {
    let last = i32(textureDimensions(relief_texture).x) - 2;
    let grid = world / relief_spacing();
    let cell = clamp(vec2<i32>(floor(grid)), vec2<i32>(0, 0), vec2<i32>(last, last));
    let along = clamp(grid - vec2<f32>(cell), vec2<f32>(0.0), vec2<f32>(1.0));
    let b = textureLoad(relief_texture, cell + vec2<i32>(1, 0), 0);
    let c = textureLoad(relief_texture, cell + vec2<i32>(0, 1), 0);
    let sb = surface_of(b);
    let sc = surface_of(c);
    if (along.x + along.y <= 1.0) {
        let a = textureLoad(relief_texture, cell, 0);
        let sa = surface_of(a);
        return Ground(sa + (sb - sa) * along.x + (sc - sa) * along.y, a.w + (b.w - a.w) * along.x + (c.w - a.w) * along.y);
    }
    let d = textureLoad(relief_texture, cell + vec2<i32>(1, 1), 0);
    let sd = surface_of(d);
    return Ground(sd + (sc - sd) * (1.0 - along.x) + (sb - sd) * (1.0 - along.y), d.w + (c.w - d.w) * (1.0 - along.x) + (b.w - d.w) * (1.0 - along.y));
}

// The ground's color, lit by its `surface` (`surface_of`), standing `height` high.
fn shade_terrain(input: TerrainOutput, surface: vec4<f32>, height: f32) -> vec4<f32> {
    // Everything here that needs screen-space derivatives or the fog texture comes first,
    // before any of it depends on the ground.
    let fog = textureSample(fog_texture, fog_sampler, input.fog_coordinates).r;
    let sight = 0.18 + 0.82 * fog;
    let footprint = length(fwidth(input.world));
    let contour = abs(fract(height / 8.0) - 0.5);
    let contour_width = max(fwidth(height / 8.0), 0.0001);
    let strata_coord = height / 9.0 + value_noise(input.world / 45.0) * 1.8;
    let strata_width = fwidth(strata_coord);

    let smooth_normal = normalize(surface.xyz);
    // How much close-up detail shows: all of it when a pixel covers well under a unit of
    // ground, none by the time it covers a unit and a half.
    let near = 1.0 - smoothstep(0.25, 1.5, footprint);
    // Small bumps break up the light up close.
    let bumps = value_noise_slope(input.world / 4.0).yz / 4.0 * 1.4 + value_noise_slope(input.world / 1.3).yz / 1.3 * 0.2;
    let normal = normalize(smooth_normal + vec3<f32>(-bumps * near, 0.0));
    // The same sky and sun as units: the sky's light by how open the ground is to it, and the
    // sun's where no hill stands in its way.
    let land = land_at(input.world);
    let ambient = mix(climate.ground_bounce.rgb, climate.sky.rgb, normal.z * 0.5 + 0.5) * mix(0.35, 1.0, land.g) * GROUND_AMBIENT;
    let light = ambient + climate.sun.rgb * max(dot(normal, LIGHT), 0.0) * land.r * cloud_light(input.world) * GROUND_SUN;
    // The close-up patterns, faded by their own smaller copies with distance. Low quality
    // goes without.
    var detail = vec4<f32>(0.0);
    if (high_quality()) {
        detail = textureSample(detail_texture, wave_sampler, input.world / DETAIL_SIZE) - vec4<f32>(0.5);
    }

    // Grass, darker in the lows and drier on the heights, in soft patches with bare dirt here
    // and there, speckled with lighter and darker grass and small stones up close.
    let t = clamp((height + 36.0) / 72.0, 0.0, 1.0);
    let patches = value_noise(input.world / 220.0) * 0.5 + value_noise(input.world / 61.0) * 0.3 + value_noise(input.world / 17.0) * 0.2;
    var base = mix(climate.grass_low.rgb, climate.grass_high.rgb, t) * (0.91 + 0.18 * patches);
    // Damp ground is lusher and darker, and dry sunny slopes paler and yellower.
    let damp = land.b;
    base = mix(base, climate.grass_dry.rgb, smoothstep(0.55, 0.95, land.r * (1.0 - damp)) * smoothstep(0.35, 0.75, patches) * 0.35);
    base = mix(base, climate.grass_lush.rgb, damp * 0.45);
    // More of the ground is bare in some climates (dirt's last component), in broad patches.
    let barren = climate.dirt.w * smoothstep(0.3, 0.6, patches) * (1.0 - damp * 0.6);
    let bare = max(smoothstep(0.66, 0.82, patches) * (0.3 + 0.1 * (1.0 - damp)), barren);
    base = mix(base, mix(climate.dirt.rgb, climate.dirt_damp.rgb, damp), bare);
    let ground_detail = mix(detail.r * 1.3, detail.g * 1.4, min(bare * 2.5, 1.0));
    base = base * (1.0 + ground_detail * 0.8);
    let speckle = value_noise(input.world / 1.7);
    base = base * (1.0 + (speckle - 0.5) * 0.24 * near);
    // Wind running across the grass: long, soft bands of lighter and darker grass, drawn out
    // across the wind and moving with it, from close up to the middle distance.
    if (high_quality()) {
        let along = dot(input.world, WIND);
        let across = dot(input.world, vec2<f32>(-WIND.y, WIND.x));
        let gust = value_noise(vec2<f32>(along / 28.0 - globals.water.x * 0.3, across / 90.0));
        let windswept = 1.0 - smoothstep(0.6, 2.5, footprint);
        base = base * (1.0 + (gust - 0.5) * GRASS_WIND * windswept);
    }
    let stones = smoothstep(0.88, 0.95, value_noise(input.world / 1.5 + vec2<f32>(17.3, 5.1)));
    base = mix(base, climate.pebble.rgb, stones * near * 0.55);
    // Rock where it is steep, in layers of lighter and darker stone with dark cracks between.
    let steep = smoothstep(0.18, 0.42, 1.0 - smooth_normal.z);
    let band = fract(strata_coord);
    let layers = mix(0.86, 1.1, smoothstep(0.2, 0.6, band));
    let crack = 1.0 - smoothstep(0.0, max(0.04, strata_width * 1.5), abs(band - 0.9));
    let rock_detail = 1.0 + detail.b * 1.1;
    let rock_tint = mix(climate.rock.rgb, climate.rock_warm.rgb, smoothstep(0.35, 0.65, patches));
    base = mix(base, rock_tint * layers * (1.0 - 0.22 * crack) * (0.9 + 0.2 * patches) * rock_detail, steep);
    // Sand along the shore, a little above the water, rippled by the wind.
    let above_water = height - globals.terrain_scale.w;
    let sand_detail = 1.0 + detail.a * 0.45;
    base = mix(base, climate.sand.rgb * sand_detail, (1.0 - smoothstep(1.0, 5.0, above_water)) * (1.0 - steep));

    // Hollows and the feet of slopes are darker, crests a little lighter, so that the shape of
    // the land reads from far away.
    let crease = clamp(surface.w * 2.5, -0.25, 0.45);
    // The land map's openness darkens broad valleys; this adds the grid's finer creases.
    let hollows = 1.0 - max(crease, 0.0) * 0.3 + max(-crease, 0.0) * 0.25;
    // Faint height contours give a sense of scale, on gentle ground only: on a cliff they crowd
    // together into stripes.
    let line = (1.0 - clamp(contour / contour_width, 0.0, 1.0)) * (1.0 - steep);
    var color = base * light * hollows * (1.0 - 0.07 * line);
    let time = globals.water.x;
    let depth = -above_water;
    if (depth > 0.0) {
        // Under water, light that comes down to the seabed and back up loses its reds first.
        // The water's surface, drawn over it, takes away the rest by how much is lost on the
        // whole, so here only the hue shifts.
        let through = water_transmittance(depth);
        color = color * through / max(dot(through, vec3<f32>(1.0 / 3.0)), 0.05);
        // Rippling light on the shallow seabed, close up only.
        if (high_quality()) {
            let caustic = caustics(input.world, time);
            color = color + climate.sun.rgb * caustic * 0.5 * exp(-depth / 4.0) * smoothstep(0.0, 0.6, depth) * near;
        }
    } else {
        // Sand the waves wash over is darker and glossier, up to where the last one reached.
        let inland = -shore_distance(input.world);
        let wander = select(0.5, value_noise(input.world / 40.0), high_quality());
        let reach = 3.0 + 2.5 * (0.5 + 0.5 * sin(time * 0.8 + wander * 6.0));
        let wet = (1.0 - smoothstep(reach * 0.6, reach, inland)) * (1.0 - steep);
        color = color * mix(1.0, 0.62, wet);
    }
    return vec4<f32>(tone(hazed(strategic_sight(color * sight, fog, input.fog_coordinates), input.relative)), 1.0);
}

// How far, in world units, a point on the map is from the water's edge: positive out to sea,
// negative inland, up to the shore field's range either way.
const SHORE_RANGE: f32 = 128.0;

fn shore_distance(world: vec2<f32>) -> f32 {
    let side = f32(textureDimensions(shore_texture).x);
    let map = f32(globals.terrain_grid.w - 1) * globals.terrain_origin.w;
    let coordinates = (world / map * (side - 1.0) + 0.5) / side;
    return (textureSampleLevel(shore_texture, fog_sampler, coordinates, 0.0).r * 2.0 - 1.0) * SHORE_RANGE;
}

// How much light of each color gets down through water this deep and back up again.
fn water_transmittance(depth: f32) -> vec3<f32> {
    return exp(-climate.absorption.rgb * 2.0 * max(depth, 0.0));
}

// Light focused by the waves into a moving net on the seabed: bright where two drifting
// layers of noise agree.
fn caustics(world: vec2<f32>, time: f32) -> f32 {
    let a = value_noise(world / 6.0 + vec2<f32>(time * 0.31, time * 0.17));
    let b = value_noise(world / 7.5 - vec2<f32>(time * 0.23, -time * 0.29));
    return pow(1.0 - abs(a - b), 10.0);
}

// What is in sight gets a pale wash, fullest when zoomed out, so that the viewer's vision reads
// as bright bubbles over the darker land it cannot see. Without fog of war there is nothing to show.
fn strategic_sight(color: vec3<f32>, fog: f32, coordinates: vec2<f32>) -> vec3<f32> {
    let has_fog = select(0.0, 1.0, globals.terrain_scale.z > 0.0);
    var wash = 0.0;
    if (globals.water.z > 0.0 && has_fog > 0.0) {
        // The fog grid is coarse, so its cells would show as steps. Averaging a ring of samples
        // and taking a narrow band of the result rounds them into smooth bubbles.
        let texel = 1.0 / vec2<f32>(textureDimensions(fog_texture));
        var total = textureSampleLevel(fog_texture, fog_sampler, coordinates, 0.0).r * 2.0;
        for (var index = 0; index < 10; index = index + 1) {
            let angle = f32(index) * 0.6283185;
            let offset = vec2<f32>(cos(angle), sin(angle)) * texel * 1.6;
            total = total + textureSampleLevel(fog_texture, fog_sampler, coordinates + offset, 0.0).r;
        }
        wash = globals.water.z * smoothstep(0.72, 0.8, total / 12.0);
    }
    let dimmed = color * mix(1.0, 0.82, globals.water.z * has_fog * (1.0 - wash));
    return mix(dimmed, dimmed * 0.74 + vec3<f32>(0.25, 0.24, 0.12), wash * 0.85);
}

// ---- Water ----

struct WaterOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) relative: vec3<f32>,
    @location(1) world: vec2<f32>,
    // How deep the water is here.
    @location(2) depth: f32,
    @location(3) fog_coordinates: vec2<f32>,
};

const FOAM_COLOR: vec3<f32> = vec3<f32>(0.85, 0.90, 0.92);
// Where the water's glints come from: the bright sky ahead of the camera, which looks north,
// low over the horizon, rather than the sun, which is behind it, so that they show in the
// usual view.
const GLINT_LIGHT: vec3<f32> = vec3<f32>(-0.20, 0.80, 0.565);
const GLINT_COLOR: vec3<f32> = vec3<f32>(1.0, 0.96, 0.88);

@vertex
fn vs_water(@builtin(vertex_index) index: u32) -> WaterOutput {
    let point = grid_point(index);
    let size = u32(globals.water.y);
    // How deep the water is over the ground as the terrain draws it, coarser far away.
    let ground = drawn_height(vec2<i32>(i32(index % size), i32(index / size)));
    let level = globals.terrain_scale.w;
    let relative = vec3<f32>(globals.terrain_origin.xy + point.offset, level - globals.terrain_origin.z);
    var out: WaterOutput;
    out.clip = globals.view_proj * vec4<f32>(relative, 1.0);
    out.relative = relative;
    out.world = vec2<f32>(point.sample) * globals.terrain_origin.w;
    out.depth = level - ground;
    out.fog_coordinates = vec2<f32>(point.sample) * globals.terrain_scale.z;
    return out;
}

// The slope of the long swells, east and north: four trains of waves, each as fast as deep
// water carries waves of its length, fading out once they are too small on screen to see.
fn swell_slope(at: vec2<f32>, time: f32, footprint: f32) -> vec2<f32> {
    // Direction (east, north), then length in world units and steepness.
    var trains = array<vec4<f32>, 4>(
        vec4<f32>(0.83, 0.55, 95.0, 0.07),
        vec4<f32>(0.37, 0.93, 61.0, 0.06),
        vec4<f32>(0.98, -0.20, 38.0, 0.05),
        vec4<f32>(-0.45, 0.89, 24.0, 0.04),
    );
    var slope = vec2<f32>(0.0);
    // On low quality, the two longest only.
    let count = select(2, 4, high_quality());
    for (var index = 0; index < count; index = index + 1) {
        let train = trains[index];
        let wavenumber = 6.2831853 / train.z;
        let speed = sqrt(9.8 * wavenumber);
        let phase = dot(train.xy, at) * wavenumber - speed * time;
        let shown = 1.0 - smoothstep(train.z / 12.0, train.z / 4.0, footprint);
        slope = slope + train.xy * train.w * cos(phase) * shown;
    }
    return slope;
}

// The small waves' slope, from two copies of their texture drifting across each other.
fn chop_slope(at: vec2<f32>, time: f32) -> vec2<f32> {
    let first = textureSample(wave_texture, wave_sampler, at / 52.0 + vec2<f32>(0.011, 0.017) * time).rg * 2.0 - 1.0;
    var slope = first * 0.32;
    if (high_quality()) {
        let turned = vec2<f32>(at.x * 0.8 - at.y * 0.6, at.x * 0.6 + at.y * 0.8);
        let second = textureSample(wave_texture, wave_sampler, turned / 19.0 - vec2<f32>(0.02, 0.031) * time).rg * 2.0 - 1.0;
        slope = slope + second * 0.22;
    }
    // Calmer and choppier stretches, so that the texture's repeats do not line up into a grid.
    if (high_quality()) {
        slope = slope * (0.55 + 0.9 * value_noise(at / 170.0 + vec2<f32>(time * 0.01, 0.0)));
    }
    return slope;
}

@fragment
fn fs_water(input: WaterOutput) -> @location(0) vec4<f32> {
    let time = globals.water.x;
    let at = input.world;
    // Everything here that needs screen-space derivatives or textures comes first.
    let footprint = length(fwidth(at));
    let chop = chop_slope(at, time);
    let fog = textureSample(fog_texture, fog_sampler, input.fog_coordinates).r;
    let from_shore = shore_distance(at);
    // Low quality breaks the foam up with one layer of noise instead of two.
    let breakup = value_noise(at / 7.0 + vec2<f32>(time * 0.12, -time * 0.07));
    var drift = breakup;
    if (high_quality()) {
        drift = value_noise(at / 23.0 + vec2<f32>(-time * 0.05, time * 0.04)) * 0.6 + breakup * 0.4;
    }

    let normal = normalize(vec3<f32>(-(swell_slope(at, time, footprint) + chop), 1.0));
    let to_eye = normalize(-input.relative);
    let facing = max(dot(normal, to_eye), 0.0);

    // What comes up from below: deep water's own color, more of it the less light gets
    // through. The seabed, drawn already, shows through the rest.
    let depth = max(input.depth, 0.0);
    let through = dot(water_transmittance(depth), vec3<f32>(1.0 / 3.0));
    let cover = clamp(1.0 - through, 0.0, 0.97);

    // The sky, mirrored more at a glancing angle.
    let mirrored = reflect(-to_eye, normal);
    let sky = mix(climate.sky_horizon.rgb, climate.sky_zenith.rgb, sqrt(clamp(mirrored.z, 0.0, 1.0)));
    let fresnel = 0.03 + 0.97 * pow(1.0 - facing, 5.0);

    // Glints of the bright sky, sharp close up and a soft sheen further out, where one pixel
    // covers many waves.
    let roughness = clamp(0.1 + footprint * 0.025, 0.1, 0.55);
    let halfway = normalize(GLINT_LIGHT + to_eye);
    let rough2 = roughness * roughness;
    let spread = dot(normal, halfway) * dot(normal, halfway) * (rough2 - 1.0) + 1.0;
    let glint = min(rough2 / (3.14159 * spread * spread) * max(dot(normal, GLINT_LIGHT), 0.0) * fresnel * 0.3, 2.0);

    // Foam: a thin line lapping at the edge, and bands that roll in towards the beach and fade
    // further out, both broken up. Fine foam fades out when it is too small on screen to keep.
    let edge_width = max(2.5, footprint * 1.5);
    let lapping = 1.0 - smoothstep(0.0, edge_width * (0.8 + 0.6 * breakup), from_shore);
    // Each band has a sharp front towards the beach and trails off behind it, and wanders a
    // little along the coast.
    let wave = fract(from_shore / 16.0 + time * 0.22 + breakup * 0.5);
    let rolling = smoothstep(0.55, 0.9, wave) * (1.0 - smoothstep(0.9, 0.97, wave));
    let patchy = smoothstep(0.25, 0.75, drift);
    let bands = rolling * patchy * exp(-from_shore / 12.0) * (1.0 - smoothstep(3.0, 8.0, footprint));
    let foam = clamp(max(lapping * 0.85, bands * 0.6), 0.0, 1.0) * step(0.0, from_shore);

    // Colors premultiplied by how much they cover.
    // Under a cloud's shadow the water is duller: less light scattered up from within, and fewer
    // glints.
    let clouded = cloud_light(at);
    var color = climate.water_deep.rgb * cover * (1.0 - fresnel) * clouded + sky * fresnel * mix(1.0, clouded, 0.5) + GLINT_COLOR * glint * clouded;
    var alpha = 1.0 - (1.0 - cover) * (1.0 - fresnel);
    color = mix(color, FOAM_COLOR, foam);
    alpha = mix(alpha, 1.0, foam);

    // Sight and the film curve work on the color as it looks, before it is multiplied again.
    let sight = 0.18 + 0.82 * fog;
    let shown = color / max(alpha, 0.001) * sight;
    return vec4<f32>(tone(hazed(strategic_sight(shown, fog, input.fog_coordinates), input.relative)) * alpha, alpha);
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
