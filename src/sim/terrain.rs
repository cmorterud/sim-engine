use std::hash::Hasher;
use twox_hash::XxHash64;

use super::components::MoveClass;
use super::config::{TerrainStyle, SECTOR_UNITS, TERRAIN_CELL_UNITS};
use crate::math::fp::{Fp, Vec2Fp, FP_FRAC_BITS};
use crate::math::rect::RectFp;
use crate::math::rng::SimRng;

pub const COST_OPEN: u8 = 1;
pub const COST_IMPASSABLE: u8 = 255;

/// Height samples are stored in steps of 1/16 unit.
pub const HEIGHT_FRAC_BITS: u32 = 4;
/// Height added to a box obstacle's samples, in units.
const OBSTACLE_HEIGHT_UNITS: i32 = 12;

const CELL_RAW: i64 = (TERRAIN_CELL_UNITS as i64) << FP_FRAC_BITS;

pub struct Terrain {
    map_units: i32,
    cells: usize,
    samples: usize,
    heights: Vec<i16>,
    water_level: Fp,
    land_cost: Vec<u8>,
    content_hash: u64,
}

impl Terrain {
    pub fn generate(map_units: i32, style: TerrainStyle, rng: &mut SimRng) -> Self {
        assert!(
            map_units > 0 && map_units % SECTOR_UNITS == 0,
            "map size must be a multiple of {SECTOR_UNITS} units"
        );
        let cells = (map_units / TERRAIN_CELL_UNITS) as usize;
        let samples = cells + 1;
        let mut heights = vec![0i16; samples * samples];
        if style == TerrainStyle::Hills {
            // Two octaves of smooth value noise: broad hills and smaller bumps.
            add_noise_octave(&mut heights, samples, 64, 30 << HEIGHT_FRAC_BITS, rng);
            add_noise_octave(&mut heights, samples, 16, 6 << HEIGHT_FRAC_BITS, rng);
        }
        let mut terrain = Self {
            map_units,
            cells,
            samples,
            heights,
            water_level: Fp::from_bits(-(1000 << FP_FRAC_BITS)),
            land_cost: vec![COST_OPEN; cells * cells],
            content_hash: 0,
        };
        terrain.update_content_hash();
        terrain
    }

    fn update_content_hash(&mut self) {
        let mut hasher = XxHash64::with_seed(0);
        for height in &self.heights {
            hasher.write(&height.to_le_bytes());
        }
        hasher.write(&self.land_cost);
        hasher.write(&self.water_level.to_bits().to_le_bytes());
        self.content_hash = hasher.finish();
    }

    /// Marks a box as impassable and raises it, so it blocks both movement and low shots.
    pub fn add_box_obstacle(&mut self, rect: RectFp) {
        let (min_x, min_y) = self.cell_of(rect.min);
        // `max` is exclusive, so step back one raw unit before finding its cell.
        let (max_x, max_y) = self.cell_of(Vec2Fp::new(
            Fp::from_bits(rect.max.x.to_bits() - 1),
            Fp::from_bits(rect.max.y.to_bits() - 1),
        ));
        let raise = (OBSTACLE_HEIGHT_UNITS << HEIGHT_FRAC_BITS) as i16;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                self.land_cost[y * self.cells + x] = COST_IMPASSABLE;
            }
        }
        for y in min_y..=(max_y + 1) {
            for x in min_x..=(max_x + 1) {
                let height = &mut self.heights[y * self.samples + x];
                *height = height.saturating_add(raise);
            }
        }
        self.update_content_hash();
    }

    pub const fn map_units(&self) -> i32 {
        self.map_units
    }

    pub const fn cells_per_side(&self) -> usize {
        self.cells
    }

    pub const fn samples_per_side(&self) -> usize {
        self.samples
    }

    pub fn heights(&self) -> &[i16] {
        &self.heights
    }

    pub const fn water_level(&self) -> Fp {
        self.water_level
    }

    pub const fn content_hash(&self) -> u64 {
        self.content_hash
    }

    /// The cost grid for a movement class. Only `Land` is allocated in the PoC.
    pub fn cost_grid(&self, class: MoveClass) -> Option<&[u8]> {
        match class {
            MoveClass::Land => Some(&self.land_cost),
            _ => None,
        }
    }

    /// The largest raw position value inside the map on each axis.
    pub const fn max_coordinate_raw(&self) -> i32 {
        (self.map_units << FP_FRAC_BITS) - 1
    }

    pub fn in_bounds(&self, point: Vec2Fp) -> bool {
        let (x, y) = (point.x.to_bits(), point.y.to_bits());
        let max = self.max_coordinate_raw();
        (0..=max).contains(&x) && (0..=max).contains(&y)
    }

    pub fn cell_of(&self, point: Vec2Fp) -> (usize, usize) {
        let max = self.cells as i64 - 1;
        let x = (point.x.to_bits() as i64 / CELL_RAW).clamp(0, max);
        let y = (point.y.to_bits() as i64 / CELL_RAW).clamp(0, max);
        (x as usize, y as usize)
    }

    pub fn is_passable(&self, point: Vec2Fp) -> bool {
        let (x, y) = self.cell_of(point);
        self.land_cost[y * self.cells + x] != COST_IMPASSABLE
    }

    /// Bilinear ground height. Positions outside the map use the nearest edge.
    pub fn height_at(&self, point: Vec2Fp) -> Fp {
        let max = self.max_coordinate_raw() as i64;
        let x = (point.x.to_bits() as i64).clamp(0, max);
        let y = (point.y.to_bits() as i64).clamp(0, max);
        let (cell_x, frac_x) = ((x / CELL_RAW) as usize, x % CELL_RAW);
        let (cell_y, frac_y) = ((y / CELL_RAW) as usize, y % CELL_RAW);
        let index = cell_y * self.samples + cell_x;
        let sample = |offset: usize| (self.heights[index + offset] as i64) << (FP_FRAC_BITS - HEIGHT_FRAC_BITS);
        let (h00, h10) = (sample(0), sample(1));
        let (h01, h11) = (sample(self.samples), sample(self.samples + 1));
        let near = h00 + (h10 - h00) * frac_x / CELL_RAW;
        let far = h01 + (h11 - h01) * frac_x / CELL_RAW;
        Fp::from_bits((near + (far - near) * frac_y / CELL_RAW) as i32)
    }
}

/// Adds smooth value noise with lattice points every `spacing` samples.
/// `amplitude` is in height steps. Integer math only.
fn add_noise_octave(heights: &mut [i16], samples: usize, spacing: usize, amplitude: i32, rng: &mut SimRng) {
    let lattice_side = samples / spacing + 2;
    let lattice: Vec<i32> = (0..lattice_side * lattice_side)
        .map(|_| rng.below(amplitude as u32 * 2 + 1) as i32 - amplitude)
        .collect();
    // Precompute the smoothstep weight for each offset between lattice points (16 fractional bits).
    let weights: Vec<i64> = (0..spacing as i64)
        .map(|offset| {
            let t = (offset << 16) / spacing as i64;
            (t * t * ((3 << 16) - 2 * t)) >> 32
        })
        .collect();
    for y in 0..samples {
        let (lattice_y, offset_y) = (y / spacing, y % spacing);
        let weight_y = weights[offset_y];
        let row = lattice_y * lattice_side;
        for x in 0..samples {
            let (lattice_x, offset_x) = (x / spacing, x % spacing);
            let weight_x = weights[offset_x];
            let at = |dx: usize, dy: usize| lattice[row + dy * lattice_side + lattice_x + dx] as i64;
            let near = at(0, 0) + (((at(1, 0) - at(0, 0)) * weight_x) >> 16);
            let far = at(0, 1) + (((at(1, 1) - at(0, 1)) * weight_x) >> 16);
            let value = near + (((far - near) * weight_y) >> 16);
            let height = &mut heights[y * samples + x];
            *height = height.saturating_add(value as i16);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::fp::fp_int;

    fn point(x: i32, y: i32) -> Vec2Fp {
        Vec2Fp::new(fp_int(x), fp_int(y))
    }

    #[test]
    fn same_seed_generates_identical_terrain() {
        let a = Terrain::generate(2_560, TerrainStyle::Hills, &mut SimRng::new(5));
        let b = Terrain::generate(2_560, TerrainStyle::Hills, &mut SimRng::new(5));
        let c = Terrain::generate(2_560, TerrainStyle::Hills, &mut SimRng::new(6));
        assert_eq!(a.content_hash(), b.content_hash());
        assert_ne!(a.content_hash(), c.content_hash());
    }

    #[test]
    fn height_interpolates_between_samples() {
        let mut terrain = Terrain::generate(1_280, TerrainStyle::Flat, &mut SimRng::new(0));
        terrain.heights[1] = 16 << HEIGHT_FRAC_BITS;
        assert_eq!(terrain.height_at(point(0, 0)), fp_int(0));
        assert_eq!(terrain.height_at(point(5, 0)), fp_int(4));
        assert_eq!(terrain.height_at(point(10, 0)), fp_int(8));
        assert_eq!(terrain.height_at(point(20, 0)), fp_int(16));
    }

    #[test]
    fn box_obstacle_blocks_cells_and_raises_ground() {
        let mut terrain = Terrain::generate(1_280, TerrainStyle::Flat, &mut SimRng::new(0));
        terrain.add_box_obstacle(RectFp::new(point(100, 100), point(140, 140)));
        assert!(!terrain.is_passable(point(110, 110)));
        assert!(terrain.is_passable(point(140, 110)));
        assert_eq!(terrain.height_at(point(120, 120)), fp_int(OBSTACLE_HEIGHT_UNITS));
    }

    #[test]
    fn maximum_size_map_samples_its_far_corner() {
        let terrain = Terrain::generate(81_920, TerrainStyle::Flat, &mut SimRng::new(0));
        assert_eq!(terrain.cells_per_side(), 4_096);
        assert_eq!(terrain.height_at(point(90_000, 90_000)), Fp::ZERO);
        assert!(!terrain.in_bounds(point(81_920, 0)));
    }
}
