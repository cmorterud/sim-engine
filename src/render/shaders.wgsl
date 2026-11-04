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
    // terrain height scale, display scale factor, fog texture coordinates per height sample, unused.
    terrain_scale: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var heights: texture_2d<i32>;
@group(0) @binding(2) var fog_texture: texture_2d<f32>;
@group(0) @binding(3) var fog_sampler: sampler;

const LIGHT: vec3<f32> = vec3<f32>(0.38, 0.27, 0.88);
const TERRAIN_GRID_SIZE: u32 = 512u;

// ---- Units, projectiles, and obstacles ----

struct InstanceInput {
    @location(2) m0: vec4<f32>,
    @location(3) m1: vec4<f32>,
    @location(4) m2: vec4<f32>,
    @location(5) m3: vec4<f32>,
    @location(6) color: vec4<f32>,
};

struct UnitOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_unit(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    instance: InstanceInput,
) -> UnitOutput {
    let model = mat4x4<f32>(instance.m0, instance.m1, instance.m2, instance.m3);
    let center = instance.m3.xyz;
    let icon_px = instance.color.a * globals.terrain_scale.y;
    let cube_clip = globals.view_proj * model * vec4<f32>(position, 1.0);
    let center_clip = globals.view_proj * vec4<f32>(center, 1.0);

    // Far units collapse to a screen-aligned dot with a minimum size. Rotating faces
    // flatten: the top and bottom become the dot's square, and the sides become lines.
    var morph = 0.0;
    if (icon_px > 0.0 && center_clip.w > 0.0) {
        let apparent_size = length(instance.m0.xyz) / max(length(center), 0.001);
        morph = 1.0 - smoothstep(globals.viewport_lod.z, globals.viewport_lod.w, apparent_size);
    }
    let corner = sign(position.xy);
    let offset = corner * icon_px * 2.0 / globals.viewport_lod.xy * center_clip.w;
    let dot_clip = vec4<f32>(center_clip.xy + offset, center_clip.z, center_clip.w);

    let shade = 0.5 + 0.5 * max(dot(normal, LIGHT), 0.0);
    var out: UnitOutput;
    out.clip = mix(cube_clip, dot_clip, morph);
    out.color = mix(instance.color.rgb * shade, instance.color.rgb, morph);
    return out;
}

@fragment
fn fs_unit(input: UnitOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(input.color, 1.0);
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
    return vec4<f32>(base * light * (1.0 - 0.18 * line) * sight, 1.0);
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
