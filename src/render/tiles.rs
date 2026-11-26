//! The terrain grid cut into square blocks, each drawn only if it is in the picture, and at the
//! detail it needs: every height sample, or every second, fourth, eighth or sixteenth, as coarse
//! as it can be drawn without the ground straying from its true heights by more than
//! `LOD_ERROR_PIXELS` on screen. Flat ground is drawn coarse even up close, and hills keep their
//! detail until they are far away, so the graphics card does not work through triangles that
//! show nothing. Of the blocks drawn, only those with water over them get the water drawn too.
//!
//! The shaders keep the coarser grids from popping in or opening cracks:
//!
//! - Each vertex's height blends smoothly from the full-detail ground to the next coarser
//!   level's as the camera draws away, by a level of detail worked out from the vertex's own
//!   place (`shaped_level`), so a block switches to the coarser grid only once its vertices
//!   already lie on it, and every block draws a vertex it shares with another at one height.
//! - Where a block meets a coarser one, the vertices along their shared edge that the coarse
//!   block lacks are laid on its straight edge (`drawn_height` in shaders.wgsl), from each
//!   block's level, which the shaders read from a small texture.
//! - Ground near the water's edge stays at full detail at every distance, so the coast never
//!   changes shape as the camera comes down, and the foam stays on it.
//!
//! Blocks drawn coarser are lit and colored from the relief field (relief.rs) at each pixel, as
//! the full-detail grid would light and color them, so neither changes with the grid, and a cliff
//! a cell wide keeps its rock. The colors that follow the height, such as the sand along the
//! shore, follow the true heights everywhere, not the drawn ones. Each
//! level's triangles lie together in the index buffer, block by block along a Z-order curve, so
//! that a run of neighbouring blocks at one level is one draw.

use std::ops::Range;

use glam::{Mat4, Vec4};

use super::shore::{ShoreField, SHORE_RANGE};

/// Grid cells a side in a block. The shaders' `BLOCK_CELLS` is the same.
pub const BLOCK_CELLS: u32 = 16;
/// The coarsest level: one vertex every 2^4 = 16 samples, one cell a block. The shaders'
/// `MAX_LEVEL` is the same.
pub const MAX_LEVEL: usize = 4;
const LEVELS: usize = MAX_LEVEL + 1;
/// How far the ground drawn coarser may stray from its true heights, in pixels on screen. Units
/// stand on the true heights, so this is also about as far as one may seem to sink into the
/// ground or float above it.
pub const LOD_ERROR_PIXELS: f64 = 0.5;
/// Ground nearer the water's edge than this many cells is drawn in full detail at every
/// distance: every corner of a cell the edge crosses is within a cell's diagonal of it, so the
/// coast, where the flat water meets the ground, keeps its shape. Further from the edge, the
/// coarsest level the shore allows rises steadily, to `MAX_LEVEL` at the shore field's reach.
pub const COAST_CELLS: f64 = 1.5;

/// Where each block's triangles begin and end in the terrain's indices, at each level.
pub struct BlockLayout {
    /// Blocks a side.
    blocks: u32,
    /// Block numbers, row by row, in the order their triangles lie in the indices.
    order: Vec<u32>,
    /// For each block, row by row, its triangles at each level: empty at levels too coarse for it.
    ranges: Vec<[Range<u32>; LEVELS]>,
    /// The coarsest level each block can be drawn at: a block at the far edges of a map whose
    /// cells do not divide into whole blocks may be too narrow for the coarser grids.
    max_level: Vec<u8>,
}

impl BlockLayout {
    pub const fn blocks(&self) -> u32 {
        self.blocks
    }
}

/// The grid's triangle indices and where each block's lie in them. `size` is the grid's
/// vertices a side. Every level's triangles are split as the full grid's are, from each cell's
/// (1, 0) corner to its (0, 1) corner, so that each coarser cell's two triangles hold exactly
/// eight of the finer ones, and the finer grid laid on the coarser ground matches it.
pub fn block_indices(size: u32) -> (Vec<u32>, BlockLayout) {
    let cells = size - 1;
    let blocks = cells.div_ceil(BLOCK_CELLS);
    let count = (blocks * blocks) as usize;
    let mut order: Vec<u32> = (0..blocks * blocks).collect();
    order.sort_by_key(|&block| morton(block % blocks, block / blocks));
    let span = |block: u32| {
        let start = block * BLOCK_CELLS;
        start..(start + BLOCK_CELLS).min(cells)
    };
    let max_level: Vec<u8> = (0..blocks * blocks)
        .map(|block| {
            let (across, down) = (span(block % blocks).len() as u32, span(block / blocks).len() as u32);
            across.trailing_zeros().min(down.trailing_zeros()).min(MAX_LEVEL as u32) as u8
        })
        .collect();
    let mut indices = Vec::with_capacity((cells * cells * 8) as usize);
    let mut ranges: Vec<[Range<u32>; LEVELS]> = vec![std::array::from_fn(|_| 0..0); count];
    for (level, spacing) in (0..LEVELS).map(|level| (level, 1_u32 << level)) {
        for &block in &order {
            let start = indices.len() as u32;
            if level <= usize::from(max_level[block as usize]) {
                for y in span(block / blocks).step_by(spacing as usize) {
                    for x in span(block % blocks).step_by(spacing as usize) {
                        let (a, b) = (y * size + x, y * size + x + spacing);
                        let (c, d) = ((y + spacing) * size + x, (y + spacing) * size + x + spacing);
                        indices.extend_from_slice(&[a, b, c, c, b, d]);
                    }
                }
            }
            ranges[block as usize][level] = start..indices.len() as u32;
        }
    }
    (indices, BlockLayout { blocks, order, ranges, max_level })
}

/// A block's place along the Z-order curve: its column's and row's bits interleaved.
fn morton(x: u32, y: u32) -> u64 {
    let spread = |value: u32| (0..16).fold(0_u64, |bits, bit| bits | (u64::from(value >> bit & 1) << (bit * 2)));
    spread(x) | spread(y) << 1
}

/// A level of detail with its steps held: from a raw level, which rises by one each time the
/// distance doubles, the shaped level holds at each whole level for the first half of the step
/// and blends to the next over the second. The shaders' `shaped_level` is the same.
pub fn shaped_level(raw: f64) -> f64 {
    if raw <= 0.0 {
        return 0.0;
    }
    let whole = raw.floor();
    (whole + ((raw - whole) * 2.0 - 1.0).clamp(0.0, 1.0)).min(MAX_LEVEL as f64)
}

/// How the shore limits the level of detail, in world units: ground nearer the water's edge than
/// the first number is at full detail, and the raw level rises by one for each further step of
/// the second. `None` where the cells are too large for the shore field to reach past them, when
/// the grid is drawn in full everywhere instead.
pub fn coast_reach(cell_units: f64) -> Option<(f64, f64)> {
    let margin = COAST_CELLS * cell_units;
    (margin < f64::from(SHORE_RANGE) * 0.5).then(|| (margin, (f64::from(SHORE_RANGE) - margin) / MAX_LEVEL as f64))
}

/// For each block of a grid of every sample of the map, the lowest raw level the shore allows
/// anywhere in it, from the shore field the shaders read. A little low, to allow for the
/// shaders blending between the field's texels.
pub fn coast_levels(shore: &ShoreField, samples: usize, cell_units: f64) -> Option<Vec<f64>> {
    let (margin, step) = coast_reach(cell_units)?;
    let cells = samples - 1;
    let blocks = cells.div_ceil(BLOCK_CELLS as usize);
    let side = shore.side();
    let per_sample = (side - 1) as f64 / cells as f64;
    let texel_units = cell_units / per_sample;
    let texel = |sample: usize, round: fn(f64) -> f64| round(sample as f64 * per_sample) as isize;
    let mut levels = Vec::with_capacity(blocks * blocks);
    for block_y in 0..blocks {
        for block_x in 0..blocks {
            let corner = |block: usize, round: fn(f64) -> f64, extra: isize| (texel((block * BLOCK_CELLS as usize).min(cells), round) + extra).clamp(0, side as isize - 1) as usize;
            let mut nearest = f32::INFINITY;
            for y in corner(block_y, f64::floor, -1)..=corner(block_y + 1, f64::ceil, 1) {
                for x in corner(block_x, f64::floor, -1)..=corner(block_x + 1, f64::ceil, 1) {
                    nearest = nearest.min(shore.distance(x, y).abs());
                }
            }
            // Out of the field's reach everywhere in the block, as most blocks are, the shaders
            // find the level the field's reach gives, the coarsest, at every vertex.
            let level = if nearest >= SHORE_RANGE - 0.5 { f64::INFINITY } else { (f64::from(nearest) - texel_units - margin) / step };
            levels.push(level);
        }
    }
    Some(levels)
}

/// The lowest and highest height sample in square blocks of the map, at every power-of-two
/// block size, for telling quickly how high and low any stretch of ground goes.
pub struct HeightRanges {
    samples: usize,
    /// Level `k` holds blocks `2^k` samples a side (the last row and column may be short).
    levels: Vec<(usize, Vec<(i16, i16)>)>,
}

impl HeightRanges {
    pub fn new(samples: usize, heights: &[i16]) -> Self {
        let mut levels = vec![(samples, heights.iter().map(|&height| (height, height)).collect::<Vec<_>>())];
        while levels.last().is_some_and(|(side, _)| *side > 1) {
            let (side, below) = levels.last().expect("checked");
            let half = side.div_ceil(2);
            let mut level = Vec::with_capacity(half * half);
            for y in 0..half {
                for x in 0..half {
                    let mut range = (i16::MAX, i16::MIN);
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (bx, by) = (x * 2 + dx, y * 2 + dy);
                        if bx < *side && by < *side {
                            let (low, high) = below[by * side + bx];
                            range = (range.0.min(low), range.1.max(high));
                        }
                    }
                    level.push(range);
                }
            }
            levels.push((half, level));
        }
        Self { samples, levels }
    }

    /// The lowest and highest sample from `min` to `max` inclusive, each way, clamped to the map.
    pub fn range(&self, min: [i32; 2], max: [i32; 2]) -> (i16, i16) {
        let last = self.samples as i32 - 1;
        let (x0, y0) = (min[0].clamp(0, last) as usize, min[1].clamp(0, last) as usize);
        let (x1, y1) = (max[0].clamp(0, last) as usize, max[1].clamp(0, last) as usize);
        // The level whose blocks are as large as the stretch or a little smaller, so that it
        // covers at most three blocks each way and reaches little past the stretch.
        let span = (x1 - x0).max(y1 - y0).max(1);
        let level = (usize::BITS - 1 - span.leading_zeros()) as usize;
        let level = level.min(self.levels.len() - 1);
        let (side, blocks) = &self.levels[level];
        let mut range = (i16::MAX, i16::MIN);
        for by in (y0 >> level)..=(y1 >> level).min(side - 1) {
            for bx in (x0 >> level)..=(x1 >> level).min(side - 1) {
                let (low, high) = blocks[by * side + bx];
                range = (range.0.min(low), range.1.max(high));
            }
        }
        range
    }
}

/// Where the terrain grid lies this frame, as the shader places it (`grid_point`).
pub struct GridPlacement {
    /// The height sample of the grid's first vertex.
    pub first: [i32; 2],
    /// Height samples between neighbouring grid vertices.
    pub step: i32,
    /// The grid's vertices a side.
    pub size: u32,
    pub cell_units: f64,
    pub height_scale: f64,
}

/// How far each block's ground strays from its true heights when drawn coarser, for a grid of
/// every sample of the map: for the shaders, a field over the blocks, one value a block, read
/// smoothly between their middles; and for choosing blocks' levels, the most that field reaches
/// over each block. Each is in world units per step of detail: a level as coarse as `k` strays
/// at most `2^k` times it.
pub struct Strays {
    /// One a block, row by row, as the shaders read them: half floats.
    pub field: Vec<u16>,
    /// The most the shaders' field reaches anywhere in each block.
    pub bound: Vec<f64>,
}

/// How far the ground strays from `heights` (`samples` a side, `height_scale` units a step) at
/// each level of each block, worked out once for the map.
pub fn strays(heights: &[i16], samples: usize, height_scale: f64) -> Strays {
    use super::relief::{from_half, half_float};
    let cells = samples - 1;
    let blocks = cells.div_ceil(BLOCK_CELLS as usize);
    let last = cells as i64;
    let height = |x: i64, y: i64| f64::from(heights[y.clamp(0, last) as usize * samples + x.clamp(0, last) as usize]) * height_scale;
    // The ground drawn at a level, a vertex every `spacing` samples, at a sample: on the coarse
    // grid's triangles, as the shaders' `level_height` has it.
    let level_height = |x: i64, y: i64, spacing: i64| {
        let (cx, cy) = (x / spacing * spacing, y / spacing * spacing);
        let (along_x, along_y) = ((x - cx) as f64 / spacing as f64, (y - cy) as f64 / spacing as f64);
        let (b, c) = (height(cx + spacing, cy), height(cx, cy + spacing));
        if along_x + along_y <= 1.0 {
            let a = height(cx, cy);
            a + (b - a) * along_x + (c - a) * along_y
        } else {
            let d = height(cx + spacing, cy + spacing);
            d + (c - d) * (1.0 - along_x) + (b - d) * (1.0 - along_y)
        }
    };
    // The least distance at which each level of the block would stray by the allowed pixels
    // is in proportion to how far it strays, over the level's spacing, at its worst.
    let mut own = vec![0.0_f32; blocks * blocks];
    for block_y in 0..blocks {
        for block_x in 0..blocks {
            let span = |block: usize| (block * BLOCK_CELLS as usize) as i64..=((block + 1) * BLOCK_CELLS as usize).min(cells) as i64;
            let mut worst = 0.0_f64;
            for level in 1..=MAX_LEVEL {
                let spacing = 1_i64 << level;
                let mut strays = 0.0_f64;
                for y in span(block_y) {
                    for x in span(block_x) {
                        strays = strays.max((level_height(x, y, spacing) - height(x, y)).abs());
                    }
                }
                worst = worst.max(strays / spacing as f64);
            }
            own[block_y * blocks + block_x] = worst as f32;
        }
    }
    // Each block's value is the most of its own and its neighbours', so that, read smoothly
    // between the blocks' middles, the field anywhere in a block is at least the block's own.
    let most_around = |values: &[f32], x: usize, y: usize| {
        let mut most = f32::NEG_INFINITY;
        for ny in y.saturating_sub(1)..=(y + 1).min(blocks - 1) {
            for nx in x.saturating_sub(1)..=(x + 1).min(blocks - 1) {
                most = most.max(values[ny * blocks + nx]);
            }
        }
        most
    };
    // Rounded to half floats up, never down, so that the shaders never find the ground flatter
    // than it is.
    let round_up = |value: f32| {
        let bits = half_float(value);
        if from_half(bits) < value { bits + 1 } else { bits }
    };
    let field: Vec<u16> = (0..blocks * blocks).map(|index| round_up(most_around(&own, index % blocks, index / blocks))).collect();
    let shown: Vec<f32> = field.iter().map(|&bits| from_half(bits)).collect();
    let bound = (0..blocks * blocks).map(|index| f64::from(most_around(&shown, index % blocks, index / blocks))).collect();
    Strays { field, bound }
}

/// How detailed to draw the grid this frame.
pub struct Lod<'a> {
    /// The camera's `screen_scale` over `LOD_ERROR_PIXELS`: ground straying by `s` units is
    /// allowed from `s` times this away.
    pub screen_per_error: f64,
    /// How far each block's ground strays when drawn coarser, or `None` to draw every block at
    /// full detail.
    pub strays: Option<&'a Strays>,
    /// For each block, the lowest raw level the shore allows in it (`coast_levels`), or `None`
    /// where there is no water.
    pub coast: Option<&'a [f64]>,
}

/// What to draw this frame: runs of neighbouring blocks' indices, one draw a run, for the
/// terrain at full detail, for the terrain drawn coarser, which is lit another way, and for the
/// water; and each block's level, row by row, for the shaders.
#[derive(Default)]
pub struct BlockPlan {
    pub terrain: Vec<Range<u32>>,
    pub far_terrain: Vec<Range<u32>>,
    pub water: Vec<Range<u32>>,
    pub levels: Vec<u8>,
}

/// Picks each block's level, and which to draw: those in the picture, and of those, the ones
/// with ground under `water_level` for the water. `view_projection` takes points relative to
/// `eye` to clip space.
pub fn plan_blocks(placement: &GridPlacement, ranges: &HeightRanges, layout: &BlockLayout, view_projection: Mat4, eye: [f64; 3], water_level: f64, lod: &Lod<'_>) -> BlockPlan {
    let blocks = layout.blocks;
    let mut plan = BlockPlan { levels: vec![0; (blocks * blocks) as usize], ..BlockPlan::default() };
    // Whether each block is in the picture and has water over it.
    let mut shown = vec![(false, false); (blocks * blocks) as usize];
    for block_y in 0..blocks {
        for block_x in 0..blocks {
            let index = (block_y * blocks + block_x) as usize;
            let sample = |grid: u32| placement.step * grid.min(placement.size - 1) as i32;
            let min = [placement.first[0] + sample(block_x * BLOCK_CELLS), placement.first[1] + sample(block_y * BLOCK_CELLS)];
            let max = [placement.first[0] + sample((block_x + 1) * BLOCK_CELLS), placement.first[1] + sample((block_y + 1) * BLOCK_CELLS)];
            let (low, high) = ranges.range(min, max);
            let (low, high) = (f64::from(low) * placement.height_scale, f64::from(high) * placement.height_scale);
            let corner = |sample: i32, axis: usize| f64::from(sample) * placement.cell_units - eye[axis];
            let (x0, x1, y0, y1) = (corner(min[0], 0), corner(max[0], 0), corner(min[1], 1), corner(max[1], 1));
            // The nearest any of the block's ground can be to the eye, which no vertex in it is nearer than.
            let nearest = |low: f64, high: f64| if low > 0.0 { low } else if high < 0.0 { -high } else { 0.0 };
            let distance = nearest(x0, x1).hypot(nearest(y0, y1)).hypot(nearest(low - eye[2], high - eye[2]));
            // As the shaders work out each vertex's, but from the nearest the block comes and
            // the most it strays, so never above any vertex's.
            let mut raw = lod.strays.map_or(0.0, |strays| (distance.max(0.001) / (lod.screen_per_error * strays.bound[index]).max(0.001)).log2());
            if let Some(coast) = lod.coast {
                raw = raw.min(coast[index]);
            }
            plan.levels[index] = (shaped_level(raw).floor() as u8).min(layout.max_level[index]);
            let water_top = high.max(water_level);
            if in_picture(view_projection, [x0, y0, low - eye[2]], [x1, y1, water_top - eye[2]]) {
                shown[index] = (true, low < water_level);
            }
        }
    }
    let extend = |runs: &mut Vec<Range<u32>>, range: &Range<u32>| match runs.last_mut() {
        Some(last) if last.end == range.start => last.end = range.end,
        _ => runs.push(range.clone()),
    };
    for &block in &layout.order {
        let block = block as usize;
        let (visible, wet) = shown[block];
        if !visible {
            continue;
        }
        let level = usize::from(plan.levels[block]);
        let range = &layout.ranges[block][level];
        extend(if level == 0 { &mut plan.terrain } else { &mut plan.far_terrain }, range);
        if wet {
            extend(&mut plan.water, range);
        }
    }
    plan
}

/// Whether any of a box, given by its lowest and highest corners relative to the eye, can be in
/// the picture: false only when all its corners lie beyond one edge of it, or behind the eye.
fn in_picture(view_projection: Mat4, low: [f64; 3], high: [f64; 3]) -> bool {
    let corners = (0..8).map(|bits: u32| {
        let pick = |axis: usize| if bits >> axis & 1 == 0 { low[axis] } else { high[axis] } as f32;
        view_projection * Vec4::new(pick(0), pick(1), pick(2), 1.0)
    });
    let mut outside = [true; 5];
    for clip in corners {
        let tests = [clip.x < -clip.w, clip.x > clip.w, clip.y < -clip.w, clip.y > clip.w, clip.w <= 0.0];
        for (all, test) in outside.iter_mut().zip(tests) {
            *all &= test;
        }
    }
    !outside.iter().any(|&all| all)
}

#[cfg(test)]
mod tests {
    use super::super::relief::{from_half, half_float};
    use super::*;

    #[test]
    fn every_cell_is_in_exactly_one_block_at_full_detail() {
        for size in [2, 17, 65, 66, 130, 1025] {
            let (indices, layout) = block_indices(size);
            let full: Vec<&Range<u32>> = layout.order.iter().map(|&block| &layout.ranges[block as usize][0]).collect();
            assert_eq!(full.first().unwrap().start, 0);
            assert!(full.windows(2).all(|pair| pair[0].end == pair[1].start));
            assert_eq!(full.last().unwrap().end, (size - 1) * (size - 1) * 6);
            assert!(indices.iter().all(|&index| index < size * size));
        }
    }

    #[test]
    fn each_level_covers_a_block_with_a_quarter_of_the_triangles_of_the_last() {
        let size = 65;
        let (indices, layout) = block_indices(size);
        for level in 0..=MAX_LEVEL {
            let cells = (16 >> level) * (16 >> level);
            for ranges in &layout.ranges {
                assert_eq!(ranges[level].len(), cells * 6, "level {level}");
            }
        }
        // The coarsest level's cell is the whole block, split the same way as the finest.
        let block = &layout.ranges[0][MAX_LEVEL];
        assert_eq!(&indices[block.start as usize..block.end as usize], &[0, 16, 16 * 65, 16 * 65, 16, 16 * 65 + 16]);
    }

    #[test]
    fn blocks_too_narrow_for_a_level_are_never_drawn_at_it() {
        // 70 cells: four whole blocks and one of six cells, which halves once at most.
        let (_, layout) = block_indices(71);
        assert_eq!(layout.blocks, 5);
        assert_eq!(layout.max_level[0], 4);
        assert_eq!(layout.max_level[4], 1, "six cells across");
        assert_eq!(layout.max_level[24], 1, "six by six");
        assert!(layout.ranges[4][2].is_empty());
    }

    #[test]
    fn neighbouring_blocks_lie_together_along_the_curve() {
        let (_, layout) = block_indices(65);
        assert_eq!(layout.order, vec![0, 1, 4, 5, 2, 3, 6, 7, 8, 9, 12, 13, 10, 11, 14, 15]);
    }

    #[test]
    fn levels_hold_then_blend() {
        for (raw, shaped) in [(-1.0, 0.0), (0.0, 0.0), (0.4, 0.0), (0.75, 0.5), (1.0, 1.0), (1.3, 1.0), (1.9, 1.8), (9.0, 4.0)] {
            assert!((shaped_level(raw) - shaped).abs() < 1e-9, "{raw} gave {}", shaped_level(raw));
        }
    }

    #[test]
    fn height_ranges_match_a_direct_search() {
        let samples = 37;
        let heights: Vec<i16> = (0..samples * samples).map(|index| ((index * 7919) % 2003) as i16 - 1000).collect();
        let ranges = HeightRanges::new(samples, &heights);
        for (min, max) in [([0, 0], [36, 36]), ([3, 5], [9, 6]), ([10, 10], [10, 10]), ([20, 1], [35, 30]), ([-5, -5], [50, 2])] {
            let mut expected = (i16::MAX, i16::MIN);
            for y in min[1].max(0)..=max[1].min(36) {
                for x in min[0].max(0)..=max[0].min(36) {
                    let height = heights[y as usize * samples + x as usize];
                    expected = (expected.0.min(height), expected.1.max(height));
                }
            }
            let (low, high) = ranges.range(min, max);
            // Blocks may reach past the stretch, so the range found holds the true one.
            assert!(low <= expected.0 && high >= expected.1, "{min:?}..{max:?}: {low}..{high} should hold {expected:?}");
        }
        assert_eq!(ranges.range([0, 0], [36, 36]), (*heights.iter().min().unwrap(), *heights.iter().max().unwrap()));
    }

    /// Flat ground at height 0 over 257 samples 20 units apart, with a pit below the water in
    /// the first block only, seen straight down from `eye`.
    fn plan_over_flat_ground(eye: [f64; 3], lod: &Lod<'_>) -> (BlockLayout, BlockPlan) {
        let samples = 257;
        let mut heights = vec![0_i16; samples * samples];
        heights[5 * samples + 5] = -50;
        let ranges = HeightRanges::new(samples, &heights);
        let (_, layout) = block_indices(samples as u32);
        let placement = GridPlacement { first: [0, 0], step: 1, size: samples as u32, cell_units: 20.0, height_scale: 1.0 };
        let view = Mat4::look_at_rh(glam::Vec3::ZERO, glam::Vec3::new(0.0, 0.0, -1.0), glam::Vec3::Y);
        let projection = Mat4::perspective_infinite_reverse_rh(45_f32.to_radians(), 1.6, 1.0);
        let plan = plan_blocks(&placement, &ranges, &layout, projection * view, eye, -10.0, lod);
        (layout, plan)
    }

    const FULL: Lod<'static> = Lod { screen_per_error: 0.0, strays: None, coast: None };

    /// Ground that strays by `per_step` units per step of detail in every one of 16 by 16 blocks.
    fn even_strays(per_step: f64) -> Strays {
        Strays { field: vec![half_float(per_step as f32); 256], bound: vec![per_step; 256] }
    }

    #[test]
    fn only_blocks_in_the_picture_and_only_wet_blocks_for_water_are_drawn() {
        // Looking straight down from 100 units over the middle of the first block.
        let (layout, plan) = plan_over_flat_ground([160.0, 160.0, 100.0], &FULL);
        assert_eq!(plan.terrain, vec![layout.ranges[0][0].clone()], "only the first block is under the camera");
        assert_eq!(plan.water, vec![layout.ranges[0][0].clone()], "the first block holds the pit");
        // From high above the whole map, every block shows, but water only over the pit's.
        let (layout, plan) = plan_over_flat_ground([2_560.0, 2_560.0, 20_000.0], &FULL);
        let full = layout.ranges.iter().map(|ranges| ranges[0].len()).sum::<usize>() as u32;
        assert_eq!(plan.terrain, vec![0..full], "at full detail, the whole map is one run");
        assert_eq!(plan.water, vec![layout.ranges[0][0].clone()]);
        assert!(plan.levels.iter().all(|&level| level == 0));
    }

    #[test]
    fn far_blocks_are_coarser_and_the_shore_keeps_its_own_detail() {
        // Full detail out to 1,000 units: from 20,000 up, everything is far beyond.
        let strays = even_strays(1.0);
        let lod = Lod { screen_per_error: 1_000.0, strays: Some(&strays), coast: None };
        let (layout, plan) = plan_over_flat_ground([2_560.0, 2_560.0, 20_000.0], &lod);
        assert!(plan.levels.iter().all(|&level| level == 4), "{:?}", plan.levels);
        let coarsest = layout.ranges.iter().map(|ranges| ranges[4].len()).sum::<usize>() as u32;
        assert!(plan.terrain.is_empty(), "nothing is at full detail");
        assert_eq!(plan.far_terrain, vec![layout.ranges[0][4].start..layout.ranges[0][4].start + coarsest], "the coarsest level is one run");
        // Closer, the level rises with distance: 1 at twice the full-detail distance.
        let (_, plan) = plan_over_flat_ground([160.0, 160.0, 2_000.0], &lod);
        assert_eq!(plan.levels[0], 1);
        assert!(plan.levels.windows(2).take(15).all(|pair| pair[0] <= pair[1]), "rising away from the camera: {:?}", &plan.levels[..16]);
        // Ground that strays less is coarser at the same distance: a quarter as much, two levels.
        let gentle = even_strays(0.25);
        let (_, plan) = plan_over_flat_ground([160.0, 160.0, 2_000.0], &Lod { screen_per_error: 1_000.0, strays: Some(&gentle), coast: None });
        assert_eq!(plan.levels[0], 3);
        // The shore holds a block at full detail however far it is.
        let mut coast = vec![9.0; 256];
        coast[17] = 0.2;
        let lod = Lod { screen_per_error: 1_000.0, strays: Some(&strays), coast: Some(&coast) };
        let (_, plan) = plan_over_flat_ground([2_560.0, 2_560.0, 20_000.0], &lod);
        assert_eq!(plan.levels[17], 0);
        assert_eq!(plan.levels[18], 4);
    }

    #[test]
    fn ground_strays_where_coarser_cells_would_cut_across_its_bumps() {
        // Flat ground and a plane draw the same at every level.
        let samples = 65;
        for heights in [vec![7_i16; samples * samples], (0..samples * samples).map(|index| (index % samples + 2 * (index / samples)) as i16).collect()] {
            let found = strays(&heights, samples, 1.0);
            assert_eq!(found.field.len(), 16);
            assert!(found.bound.iter().all(|&value| value == 0.0), "{:?}", found.bound);
        }
        // A spike 32 high on a sample of the second block every eighth sample has: at the
        // finest coarser level, its neighbours stray by 16 over a spacing of 2.
        let mut heights = vec![0_i16; samples * samples];
        heights[24 * samples + 24] = 32;
        let found = strays(&heights, samples, 1.0);
        let field: Vec<f32> = found.field.iter().map(|&bits| from_half(bits)).collect();
        assert_eq!(field[5], 8.0, "the spike's own block");
        assert!([0, 1, 2, 4, 6, 8, 9, 10].iter().all(|&block| field[block] == 8.0), "its neighbours take it too: {field:?}");
        assert!([3, 7, 11, 12, 13, 14, 15].iter().all(|&block| field[block] == 0.0), "the rest are flat: {field:?}");
        // Read smoothly, the field over a block reaches at most what its neighbours hold.
        assert!(found.bound.iter().all(|&value| value == 8.0));
    }

    #[test]
    fn the_coast_holds_full_detail_near_the_water_and_frees_it_further_away() {
        // Ground rising from west to east, crossing the water a third of the way across.
        let samples = 97;
        let heights: Vec<i16> = (0..samples * samples).map(|index| ((index % samples) as i32 - 32) as i16).collect();
        let field = super::super::camera::HeightField::new(samples, heights, 20.0, 1.0);
        let shore = ShoreField::bake(&field, 0.0).expect("there is water");
        let levels = coast_levels(&shore, samples, 20.0).expect("cells are small enough");
        assert_eq!(levels.len(), 36);
        // The edge runs between the second and third columns of blocks, at sample 32.
        let row: Vec<f64> = levels[..6].to_vec();
        assert!(shaped_level(row[1]) < 1.0 && shaped_level(row[2]) < 1.0, "the edge's blocks stay at full detail: {row:?}");
        assert!([0, 3, 4, 5].iter().all(|&column| shaped_level(row[column]) >= 4.0), "blocks far from it are free: {row:?}");
    }
}
