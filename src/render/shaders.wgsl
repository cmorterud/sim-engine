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
    // the simulation's time in seconds, for waves, the terrain grid's samples a side, how strongly
    // sight is washed pale, then the quality: 0 for low, 1 for high.
    water: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var heights: texture_2d<i32>;
@group(0) @binding(2) var fog_texture: texture_2d<f32>;
@group(0) @binding(3) var fog_sampler: sampler;

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
const TAU: f32 = 6.2831853;
// How far a leg swings either way, in radians, and how far a gun kicks back, as a share of the unit.
const SWING_ANGLE: f32 = 0.45;
const RECOIL_DISTANCE: f32 = 0.09;
// A muzzle flash's color, in linear light, and how bright pulsing lights are while idle.
const FLASH_COLOR: vec3<f32> = vec3<f32>(1.0, 0.75, 0.3);
const IDLE_GLOW: f32 = 0.3;
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
    // How open the surface is to the sky, from the mesh: less where parts crowd one another.
    @location(5) openness: f32,
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
    // Crowded corners lose most of the sky's light and some of the sun's, which sets parts
    // apart where they meet, as a stylized contact shadow.
    let open = input.openness;
    var diffuse = input.albedo * (ambient * open + SUN * sun * mix(1.0, open, SUN_OCCLUSION));
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
    out.clip = globals.view_proj * vec4<f32>(flat, ground_at(flat) + SHADOW_LIFT, 1.0);
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
    out.clip = globals.view_proj * vec4<f32>(flat, ground_at(flat) + SCAR_LIFT, 1.0);
    out.across = position.xy * 2.0;
    out.look = instance.color.rgb;
    let sample = (flat - globals.terrain_origin.xy) / globals.terrain_origin.w + vec2<f32>(globals.terrain_grid.xy);
    out.fog_coordinates = sample * globals.terrain_scale.z;
    return out;
}

@fragment
fn fs_scar(input: ScarOutput) -> @location(0) vec4<f32> {
    let sight = 0.18 + 0.82 * textureSample(fog_texture, fog_sampler, input.fog_coordinates).r;
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
    let ambient = mix(GROUND_BOUNCE, SKY, normal.z * 0.5 + 0.5);
    let lit = input.albedo * (ambient + SUN * 0.8 * max(dot(normal, LIGHT), 0.0));
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

struct DomeOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) @interpolate(flat) sphere_center: vec3<f32>,
    @location(2) relative: vec3<f32>,
    // Across the dome from above, from -0.5 to 0.5.
    @location(3) across: vec2<f32>,
    @location(4) @interpolate(flat) fade: f32,
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
    let alpha = input.color.a * (DOME_FACE + DOME_HEX * hex + DOME_RIM * rim) * input.fade;
    return vec4<f32>(input.color.rgb, clamp(alpha, 0.0, 0.8));
}

// ---- Terrain ----

struct TerrainOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) relative: vec3<f32>,
    @location(1) height: f32,
    @location(2) fog_coordinates: vec2<f32>,
    // Where it is on the map, for patterns that stay put as the camera moves.
    @location(3) world: vec2<f32>,
    // The ground's smooth normal, from the height map around the vertex.
    @location(4) normal: vec3<f32>,
    // How far the ground around rises above it, per unit of distance: above 0 in hollows and
    // creases, below 0 on ridges and crests.
    @location(5) cavity: f32,
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

// How far the ground `reach` samples around rises above `height`, on average, per unit of distance.
fn rise_around(sample: vec2<i32>, reach: i32, height: f32) -> f32 {
    let around = terrain_sample(sample + vec2<i32>(reach, 0)) + terrain_sample(sample - vec2<i32>(reach, 0))
        + terrain_sample(sample + vec2<i32>(0, reach)) + terrain_sample(sample - vec2<i32>(0, reach));
    return (around * 0.25 - height) / (f32(reach) * globals.terrain_origin.w);
}

@vertex
fn vs_terrain(@builtin(vertex_index) index: u32) -> TerrainOutput {
    let point = grid_point(index);
    let height = terrain_sample(point.sample);
    let relative = vec3<f32>(globals.terrain_origin.xy + point.offset, height - globals.terrain_origin.z);
    // The slope across the neighbouring samples, at the grid's own spacing.
    let step = globals.terrain_grid.z;
    let spacing = f32(step) * globals.terrain_origin.w;
    let across = terrain_sample(point.sample + vec2<i32>(step, 0)) - terrain_sample(point.sample - vec2<i32>(step, 0));
    let along = terrain_sample(point.sample + vec2<i32>(0, step)) - terrain_sample(point.sample - vec2<i32>(0, step));

    var out: TerrainOutput;
    out.clip = globals.view_proj * vec4<f32>(relative, 1.0);
    out.relative = relative;
    out.height = height;
    out.fog_coordinates = vec2<f32>(point.sample) * globals.terrain_scale.z;
    out.world = vec2<f32>(point.sample) * globals.terrain_origin.w;
    out.normal = normalize(vec3<f32>(-across / (2.0 * spacing), -along / (2.0 * spacing), 1.0));
    // Fine creases and broad valleys both.
    out.cavity = rise_around(point.sample, step, height) * 0.4 + rise_around(point.sample, step * 3, height) * 0.6;
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

// Ground colors, in linear light.
const GRASS_LOW: vec3<f32> = vec3<f32>(0.10, 0.17, 0.09);
const GRASS_HIGH: vec3<f32> = vec3<f32>(0.24, 0.27, 0.14);
const DIRT: vec3<f32> = vec3<f32>(0.25, 0.21, 0.14);
const ROCK: vec3<f32> = vec3<f32>(0.30, 0.28, 0.26);
const SAND: vec3<f32> = vec3<f32>(0.44, 0.40, 0.29);
const PEBBLE: vec3<f32> = vec3<f32>(0.29, 0.27, 0.24);

@fragment
fn fs_terrain(input: TerrainOutput) -> @location(0) vec4<f32> {
    // Everything here that needs screen-space derivatives or the fog texture comes first,
    // before any of it depends on the ground.
    let fog = textureSample(fog_texture, fog_sampler, input.fog_coordinates).r;
    let sight = 0.18 + 0.82 * fog;
    let footprint = length(fwidth(input.world));
    let contour = abs(fract(input.height / 8.0) - 0.5);
    let contour_width = max(fwidth(input.height / 8.0), 0.0001);
    let strata_coord = input.height / 9.0 + value_noise(input.world / 45.0) * 1.8;
    let strata_width = fwidth(strata_coord);

    let smooth_normal = normalize(input.normal);
    // How much close-up detail shows: all of it when a pixel covers well under a unit of
    // ground, none by the time it covers a unit and a half.
    let near = 1.0 - smoothstep(0.25, 1.5, footprint);
    // Small bumps break up the light up close.
    let bumps = value_noise_slope(input.world / 4.0).yz / 4.0 * 1.4 + value_noise_slope(input.world / 1.3).yz / 1.3 * 0.2;
    let normal = normalize(smooth_normal + vec3<f32>(-bumps * near, 0.0));
    let light = 0.42 + 0.66 * max(dot(normal, LIGHT), 0.0);

    // Grass, darker in the lows and drier on the heights, in soft patches with bare dirt here
    // and there, speckled with lighter and darker grass and small stones up close.
    let t = clamp((input.height + 36.0) / 72.0, 0.0, 1.0);
    let patches = value_noise(input.world / 220.0) * 0.5 + value_noise(input.world / 61.0) * 0.3 + value_noise(input.world / 17.0) * 0.2;
    var base = mix(GRASS_LOW, GRASS_HIGH, t) * (0.91 + 0.18 * patches);
    base = mix(base, DIRT, smoothstep(0.66, 0.82, patches) * 0.3);
    let speckle = value_noise(input.world / 1.7);
    base = base * (1.0 + (speckle - 0.5) * 0.24 * near);
    let stones = smoothstep(0.88, 0.95, value_noise(input.world / 1.5 + vec2<f32>(17.3, 5.1)));
    base = mix(base, PEBBLE, stones * near * 0.55);
    // Rock where it is steep, in layers of lighter and darker stone with dark cracks between.
    let steep = smoothstep(0.18, 0.42, 1.0 - smooth_normal.z);
    let band = fract(strata_coord);
    let layers = mix(0.86, 1.1, smoothstep(0.2, 0.6, band));
    let crack = 1.0 - smoothstep(0.0, max(0.04, strata_width * 1.5), abs(band - 0.9));
    base = mix(base, ROCK * layers * (1.0 - 0.22 * crack) * (0.9 + 0.2 * patches), steep);
    // Sand along the shore, a little above the water.
    let above_water = input.height - globals.terrain_scale.w;
    base = mix(base, SAND, (1.0 - smoothstep(1.0, 5.0, above_water)) * (1.0 - steep));

    // Hollows and the feet of slopes are darker, crests a little lighter, so that the shape of
    // the land reads from far away.
    let crease = clamp(input.cavity * 2.5, -0.25, 0.45);
    let hollows = 1.0 - max(crease, 0.0) * 0.55 + max(-crease, 0.0) * 0.35;
    // Faint height contours give a sense of scale, on gentle ground only: on a cliff they crowd
    // together into stripes.
    let line = (1.0 - clamp(contour / contour_width, 0.0, 1.0)) * (1.0 - steep);
    var color = base * light * hollows * (1.0 - 0.07 * line);
    // Under water the ground darkens with depth. The water's surface is drawn over it.
    let depth = -above_water;
    if (depth > 0.0) {
        color = color * mix(0.85, 0.35, clamp(depth / 60.0, 0.0, 1.0));
    }
    return vec4<f32>(strategic_sight(color * sight, fog, input.fog_coordinates), 1.0);
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

const WATER_SHALLOW: vec3<f32> = vec3<f32>(0.07, 0.24, 0.30);
const WATER_DEEP: vec3<f32> = vec3<f32>(0.02, 0.09, 0.18);
const WATER_SKY: vec3<f32> = vec3<f32>(0.42, 0.52, 0.62);
const FOAM_COLOR: vec3<f32> = vec3<f32>(0.85, 0.90, 0.92);

@vertex
fn vs_water(@builtin(vertex_index) index: u32) -> WaterOutput {
    let point = grid_point(index);
    let ground = f32(textureLoad(heights, point.sample, 0).r) * globals.terrain_scale.x;
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

@fragment
fn fs_water(input: WaterOutput) -> @location(0) vec4<f32> {
    let time = globals.water.x;
    let at = input.world;
    // Swells: three trains of gentle waves crossing, which tilt the surface a little as they pass.
    let k1 = vec2<f32>(0.021, 0.013);
    let k2 = vec2<f32>(-0.017, 0.026);
    let k3 = vec2<f32>(0.05, -0.034);
    let tilt = k1 * 3.0 * cos(dot(at, k1) + time * 1.3) + k2 * 2.5 * cos(dot(at, k2) + time * 1.7) + k3 * 1.0 * cos(dot(at, k3) + time * 2.3);
    let normal = normalize(vec3<f32>(-tilt, 1.0));
    let to_eye = normalize(-input.relative);
    // Darker and less clear the deeper it is.
    let depth = max(input.depth, 0.0);
    let deep = 1.0 - exp(-depth / 30.0);
    var color = mix(WATER_SHALLOW, WATER_DEEP, deep);
    // The sky shows in it, more at a glancing angle.
    let fresnel = pow(1.0 - max(dot(normal, to_eye), 0.0), 4.0);
    color = mix(color, WATER_SKY, fresnel * 0.45);
    // A gentle shimmer drifting across it, with the odd sparkle. The fine layer fades out
    // before it is small enough on screen to shimmer for the wrong reason.
    let fine = 1.0 - smoothstep(0.3, 1.0, length(fwidth(at / 7.0)));
    let shimmer = value_noise(at / 18.0 + vec2<f32>(time * 0.4, time * 0.15)) * 0.6 + value_noise(at / 7.0 - vec2<f32>(time * 0.25, time * 0.5)) * 0.4 * fine + 0.2 * (1.0 - fine);
    color = color * (0.92 + 0.16 * shimmer) + vec3<f32>(0.9, 0.9, 0.85) * smoothstep(0.75, 0.92, shimmer) * 0.08;
    // Foam where it laps against the shore.
    let ripple = value_noise(at / 9.0 + vec2<f32>(time * 0.3, time * 0.2));
    let edge = 1.0 - smoothstep(0.0, 3.5, depth);
    let foam = edge * smoothstep(0.35, 0.6, ripple + edge * 0.4);
    color = mix(color, FOAM_COLOR, foam * 0.8);
    let alpha = clamp(mix(0.35, 0.9, deep) + foam * 0.5 + fresnel * 0.2, 0.0, 0.95);
    let fog = textureSample(fog_texture, fog_sampler, input.fog_coordinates).r;
    let sight = 0.18 + 0.82 * fog;
    return vec4<f32>(strategic_sight(color * sight, fog, input.fog_coordinates), alpha);
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
