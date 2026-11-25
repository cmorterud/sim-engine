//! The terrain grid cut into square tiles, so that only the tiles in the picture are drawn, and
//! of those, only the ones with water under the surface get the water drawn over them. Each
//! tile's triangles lie together in the index buffer, so that a run of tiles is one draw.

use std::ops::Range;

use glam::{Mat4, Vec4};

/// Grid cells a side in a tile.
pub const TILE_CELLS: u32 = 64;

/// The grid's triangle indices, tile by tile, row by row within each tile, and where each
/// tile's begin and end in them. `size` is the grid's vertices a side.
pub fn tiled_indices(size: u32) -> (Vec<u32>, Vec<Range<u32>>) {
    let cells = size - 1;
    let tiles = cells.div_ceil(TILE_CELLS);
    let mut indices = Vec::with_capacity((cells * cells * 6) as usize);
    let mut ranges = Vec::with_capacity((tiles * tiles) as usize);
    for tile_y in 0..tiles {
        for tile_x in 0..tiles {
            let start = indices.len() as u32;
            for y in tile_y * TILE_CELLS..((tile_y + 1) * TILE_CELLS).min(cells) {
                for x in tile_x * TILE_CELLS..((tile_x + 1) * TILE_CELLS).min(cells) {
                    let (a, b) = (y * size + x, y * size + x + 1);
                    let (c, d) = ((y + 1) * size + x, (y + 1) * size + x + 1);
                    indices.extend_from_slice(&[a, b, c, c, b, d]);
                }
            }
            ranges.push(start..indices.len() as u32);
        }
    }
    (indices, ranges)
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

/// Which tiles to draw: those in the picture, and of those, the ones with ground under
/// `water_level`. Each as runs of neighbouring tiles' indices, one draw a run.
pub struct TileRuns {
    pub terrain: Vec<Range<u32>>,
    pub water: Vec<Range<u32>>,
}

/// Picks the tiles to draw. `view_projection` takes points relative to `eye` to clip space.
pub fn visible_tiles(placement: &GridPlacement, ranges: &HeightRanges, tile_indices: &[Range<u32>], view_projection: Mat4, eye: [f64; 3], water_level: f64) -> TileRuns {
    let tiles = (placement.size - 1).div_ceil(TILE_CELLS);
    let mut runs = TileRuns { terrain: Vec::new(), water: Vec::new() };
    let extend = |runs: &mut Vec<Range<u32>>, range: &Range<u32>| match runs.last_mut() {
        Some(last) if last.end == range.start => last.end = range.end,
        _ => runs.push(range.clone()),
    };
    for tile_y in 0..tiles {
        for tile_x in 0..tiles {
            let index = (tile_y * tiles + tile_x) as usize;
            let sample = |grid: u32| placement.step * grid as i32;
            let min = [placement.first[0] + sample(tile_x * TILE_CELLS), placement.first[1] + sample(tile_y * TILE_CELLS)];
            let max = [placement.first[0] + sample(((tile_x + 1) * TILE_CELLS).min(placement.size - 1)), placement.first[1] + sample(((tile_y + 1) * TILE_CELLS).min(placement.size - 1))];
            let (low, high) = ranges.range(min, max);
            let (low, high) = (f64::from(low) * placement.height_scale, f64::from(high) * placement.height_scale);
            let water_top = high.max(water_level);
            let corner = |sample: i32, axis: usize| sample as f64 * placement.cell_units - eye[axis];
            let (x0, x1, y0, y1) = (corner(min[0], 0), corner(max[0], 0), corner(min[1], 1), corner(max[1], 1));
            if !in_picture(view_projection, [x0, y0, low - eye[2]], [x1, y1, water_top - eye[2]]) {
                continue;
            }
            extend(&mut runs.terrain, &tile_indices[index]);
            if low < water_level {
                extend(&mut runs.water, &tile_indices[index]);
            }
        }
    }
    runs
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
    use super::*;

    #[test]
    fn every_cell_is_in_exactly_one_tile() {
        for size in [2, 65, 66, 130, 1025] {
            let (indices, ranges) = tiled_indices(size);
            assert_eq!(indices.len() as u32, (size - 1) * (size - 1) * 6);
            assert_eq!(ranges.first().unwrap().start, 0);
            assert!(ranges.windows(2).all(|pair| pair[0].end == pair[1].start));
            assert_eq!(ranges.last().unwrap().end as usize, indices.len());
            assert!(indices.iter().all(|&index| index < size * size));
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

    #[test]
    fn only_tiles_in_the_picture_and_only_wet_tiles_for_water_are_drawn() {
        // Flat ground at height 0 over 257 samples 20 units apart, with a pit below the water
        // in the first tile only.
        let samples = 257;
        let mut heights = vec![0_i16; samples * samples];
        heights[10 * samples + 10] = -50;
        let ranges = HeightRanges::new(samples, &heights);
        let (_, tile_indices) = tiled_indices(samples as u32);
        let placement = GridPlacement { first: [0, 0], step: 1, size: samples as u32, cell_units: 20.0, height_scale: 1.0 };
        // Looking straight down from 300 units over the middle of the first tile.
        let eye = [640.0, 640.0, 300.0];
        let view = Mat4::look_at_rh(glam::Vec3::ZERO, glam::Vec3::new(0.0, 0.0, -1.0), glam::Vec3::Y);
        let projection = Mat4::perspective_infinite_reverse_rh(45_f32.to_radians(), 1.6, 1.0);
        let runs = visible_tiles(&placement, &ranges, &tile_indices, projection * view, eye, -10.0);
        assert_eq!(runs.terrain, vec![tile_indices[0].clone()], "only the first tile is under the camera");
        assert_eq!(runs.water, vec![tile_indices[0].clone()], "the first tile holds the pit");
        // From high above the whole map, every tile shows, but water only over the pit's.
        let high = [2_560.0, 2_560.0, 20_000.0];
        let runs = visible_tiles(&placement, &ranges, &tile_indices, projection * view, high, -10.0);
        assert_eq!(runs.terrain, vec![0..tile_indices.last().unwrap().end]);
        assert_eq!(runs.water, vec![tile_indices[0].clone()]);
    }
}
