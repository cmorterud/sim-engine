use std::hash::Hasher;
use twox_hash::XxHash64;

use super::components::MoveClass;
use super::config::{TerrainStyle, SECTOR_UNITS, TERRAIN_CELL_UNITS};
use crate::math::fp::{Fp, Vec2Fp, FP_FRAC_BITS};
use crate::math::rect::RectFp;
use crate::math::rng::SimRng;

pub const COST_OPEN: u8 = 1;
pub const COST_IMPASSABLE: u8 = 255;
/// How far below the water a cell's highest corner must be for ships to sail it.
pub const NAVAL_DEPTH_UNITS: i32 = 10;
/// How far below its water every sea's floor lies, where the sea is open, in units: the ground
/// that seabed walkers walk on and sunken wrecks come to rest on (GAME_SPECIFICATION.md §10).
/// Shores and islands rise from it, and nowhere is deeper. Every map with water lays its sea
/// floor here (`maps.rs`, `grounds.rs`, `playtest.rs`), and `tests/map_test.rs` holds them to it.
pub const SEA_DEPTH_UNITS: i32 = 50;

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
    /// Open where the water is deep enough for ships. Worked out from the heights, the water,
    /// and structures, so it is not hashed.
    naval_cost: Vec<u8>,
    /// Open on land and on any water, for hovercraft and units that walk the seabed. Worked out
    /// the same way, and not hashed either.
    amphibious_cost: Vec<u8>,
    /// Whether any cell is under the water. Without water, amphibians move as land units do.
    has_water: bool,
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
        if style != TerrainStyle::Flat {
            // Two octaves of smooth value noise: broad hills and smaller bumps.
            add_noise_octave(&mut heights, samples, 64, 30 << HEIGHT_FRAC_BITS, rng);
            add_noise_octave(&mut heights, samples, 16, 6 << HEIGHT_FRAC_BITS, rng);
        }
        if style == TerrainStyle::MirroredHills {
            // Copy each sample from its partner across the map center.
            for index in 0..heights.len() / 2 {
                heights[index] = heights[samples * samples - 1 - index];
            }
        }
        let mut terrain = Self {
            map_units,
            cells,
            samples,
            heights,
            water_level: Fp::from_bits(-(1000 << FP_FRAC_BITS)),
            land_cost: vec![COST_OPEN; cells * cells],
            naval_cost: Vec::new(),
            amphibious_cost: Vec::new(),
            has_water: false,
            content_hash: 0,
        };
        terrain.derive_costs();
        terrain.update_content_hash();
        terrain
    }

    /// Terrain from a map file: its heights, in sixteenths of a unit, one per cell corner, and
    /// the cells too steep or too deep for land units. Panics if the sizes do not fit the map.
    pub fn from_heights(map_units: i32, heights: Vec<i16>, blocked: &[bool], water_level: Fp) -> Self {
        assert!(
            map_units > 0 && map_units % SECTOR_UNITS == 0,
            "map size must be a multiple of {SECTOR_UNITS} units"
        );
        let cells = (map_units / TERRAIN_CELL_UNITS) as usize;
        let samples = cells + 1;
        assert_eq!(heights.len(), samples * samples, "one height per cell corner");
        assert_eq!(blocked.len(), cells * cells, "one flag per cell");
        let land_cost = blocked.iter().map(|&blocked| if blocked { COST_IMPASSABLE } else { COST_OPEN }).collect();
        let mut terrain = Self { map_units, cells, samples, heights, water_level, land_cost, naval_cost: Vec::new(), amphibious_cost: Vec::new(), has_water: false, content_hash: 0 };
        terrain.derive_costs();
        terrain.update_content_hash();
        terrain
    }

    fn derive_costs(&mut self) {
        let cells = self.cells;
        self.naval_cost = (0..cells * cells).map(|cell| self.naval_cost_of(cell % cells, cell / cells)).collect();
        self.amphibious_cost = (0..cells * cells).map(|cell| self.amphibious_cost_of(cell % cells, cell / cells)).collect();
        self.has_water = (0..cells * cells).any(|cell| self.wet(cell % cells, cell / cells));
    }

    /// Whether the map has any water at all.
    pub const fn has_water(&self) -> bool {
        self.has_water
    }

    /// Whether any of a cell is under the water.
    fn wet(&self, x: usize, y: usize) -> bool {
        let corners = [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)];
        let lowest = corners.iter().map(|&(cx, cy)| i32::from(self.heights[cy * self.samples + cx])).min().unwrap_or(0);
        lowest < self.water_level.to_bits() >> (FP_FRAC_BITS - HEIGHT_FRAC_BITS)
    }

    /// Open to hovercraft and seabed walkers: land that land units can cross, or water.
    fn amphibious_cost_of(&self, x: usize, y: usize) -> u8 {
        if self.land_cost[y * self.cells + x] != COST_IMPASSABLE || self.wet(x, y) { COST_OPEN } else { COST_IMPASSABLE }
    }

    /// Open to ships if the water over the whole cell is deep enough.
    fn naval_cost_of(&self, x: usize, y: usize) -> u8 {
        let corners = [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)];
        let highest = corners.iter().map(|&(cx, cy)| i32::from(self.heights[cy * self.samples + cx])).max().unwrap_or(0);
        // Heights are in sixteenths of a unit, and the water in raw fixed point.
        let water = self.water_level.to_bits() >> (FP_FRAC_BITS - HEIGHT_FRAC_BITS);
        if highest + (NAVAL_DEPTH_UNITS << HEIGHT_FRAC_BITS) <= water { COST_OPEN } else { COST_IMPASSABLE }
    }

    /// Whether a unit of this class can be in a cell: on land, or on deep enough water.
    pub fn passable_for(&self, class: MoveClass, point: Vec2Fp) -> bool {
        match class {
            MoveClass::Naval | MoveClass::Submarine => {
                let (x, y) = self.unit_cell_of(point);
                self.naval_cost[y * self.cells + x] != COST_IMPASSABLE
            }
            MoveClass::Hover | MoveClass::Seabed => {
                let (x, y) = self.unit_cell_of(point);
                self.amphibious_cost[y * self.cells + x] != COST_IMPASSABLE
            }
            MoveClass::Air => true,
            MoveClass::Land => self.is_passable(point),
        }
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
                self.naval_cost[y * self.cells + x] = COST_IMPASSABLE;
                self.amphibious_cost[y * self.cells + x] = COST_IMPASSABLE;
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

    /// Blocks or clears the land cost of a structure's cells. Structures come and go
    /// constantly, so this skips rehashing; the checksum covers them as units instead.
    pub fn set_structure_cells(&mut self, cell_x: u32, cell_y: u32, size: u32, blocked: bool) {
        let cost = if blocked { COST_IMPASSABLE } else { COST_OPEN };
        for y in cell_y..cell_y + size {
            for x in cell_x..cell_x + size {
                let (x, y) = (x as usize, y as usize);
                self.land_cost[y * self.cells + x] = cost;
                // A structure on the water blocks ships too, and once gone leaves the water as it was.
                self.naval_cost[y * self.cells + x] = if blocked { COST_IMPASSABLE } else { self.naval_cost_of(x, y) };
                self.amphibious_cost[y * self.cells + x] = if blocked { COST_IMPASSABLE } else { self.amphibious_cost_of(x, y) };
            }
        }
    }

    /// Whether ships can be in a cell: deep water, with no structure on it.
    pub fn cell_is_navigable(&self, cell_x: u32, cell_y: u32) -> bool {
        let (x, y) = (cell_x as usize, cell_y as usize);
        x < self.cells && y < self.cells && self.naval_cost[y * self.cells + x] != COST_IMPASSABLE
    }

    pub fn cell_is_open(&self, cell_x: u32, cell_y: u32) -> bool {
        let (x, y) = (cell_x as usize, cell_y as usize);
        x < self.cells && y < self.cells && self.land_cost[y * self.cells + x] != COST_IMPASSABLE
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

    /// Whether the ground at a point is under the water: whether it is sea floor.
    pub fn under_water(&self, point: Vec2Fp) -> bool {
        self.height_at(point) < self.water_level
    }

    pub const fn content_hash(&self) -> u64 {
        self.content_hash
    }

    /// The cost grid for a movement class. Only `Land` is allocated in the PoC.
    pub fn cost_grid(&self, class: MoveClass) -> Option<&[u8]> {
        match class {
            MoveClass::Land => Some(&self.land_cost),
            MoveClass::Naval | MoveClass::Submarine => Some(&self.naval_cost),
            MoveClass::Hover | MoveClass::Seabed => Some(&self.amphibious_cost),
            _ => None,
        }
    }

    /// The largest raw position value inside the map on each axis.
    pub const fn max_coordinate_raw(&self) -> i32 {
        (self.map_units << FP_FRAC_BITS) - 1
    }

    /// How far past the map's edge a unit may go, in raw units: half its width. Aircraft fly on
    /// past the edge after a pass, a Strategic Bomber some 2,000 units, and turn back; this is
    /// only a limit on a runaway.
    pub const fn outer_margin_raw(&self) -> i32 {
        (self.map_units / 2) << FP_FRAC_BITS
    }

    /// Whether a point is on the map or within the outer margin around it, where the simulation
    /// still runs: units may be there, though no order can send them there.
    pub fn in_play(&self, point: Vec2Fp) -> bool {
        let (x, y) = (point.x.to_bits(), point.y.to_bits());
        let (low, high) = (1 - self.outer_margin_raw(), self.max_coordinate_raw() + self.outer_margin_raw());
        (low..=high).contains(&x) && (low..=high).contains(&y)
    }

    /// The point, moved onto the map if it is off it: where an order meant for it can go. A raw
    /// bit in from the low edges, so that the two sides' limits are mirror images.
    pub fn onto_map(&self, point: Vec2Fp) -> Vec2Fp {
        let max = self.max_coordinate_raw();
        Vec2Fp::new(Fp::from_bits(point.x.to_bits().clamp(1, max)), Fp::from_bits(point.y.to_bits().clamp(1, max)))
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

    /// The cell a unit is in, for moving it: whether it may stand there, and which way its flow
    /// field sends it. A point on, or within a few raw bits of, the line between two cells
    /// belongs to the one nearer the map's center, so that a unit and its mirror image through
    /// the center are in mirror-image cells.
    /// `cell_of` rounds down, which put Blue's unit exactly on a line in the cell east of it and
    /// Red's mirror image in the cell east of it too, one apart from the mirror, and the two went
    /// different ways. "Within a few raw bits", because fixed-point rounding is not symmetric
    /// under negation, and one side's unit can stop a bit short of the line its mirror is on.
    /// On the line through the center itself both cells are as near it, and the one taken is
    /// the one on the side of the center that the point is, along the other axis: a unit
    /// exactly on the middle row of Twin Passes, and its mirror image, once took the same cell,
    /// which is not its mirror, and the two went different ways.
    pub fn unit_cell_of(&self, point: Vec2Fp) -> (usize, usize) {
        const ON_LINE_RAW: i64 = 8;
        let max = self.cells as i64 - 1;
        let half = self.cells as i64 * CELL_RAW / 2;
        let axis = |raw: i64, other: i64| {
            let line = (raw + CELL_RAW / 2).div_euclid(CELL_RAW);
            let cell = if (raw - line * CELL_RAW).abs() <= ON_LINE_RAW {
                // The cell on the center's side of the line, and on the middle line, the cell on
                // the side the point is of the center, along the other axis.
                let beyond = if raw == half { other > half } else { raw > half };
                if beyond { line - 1 } else { line }
            } else {
                raw.div_euclid(CELL_RAW)
            };
            cell.clamp(0, max)
        };
        let (x, y) = (i64::from(point.x.to_bits()), i64::from(point.y.to_bits()));
        (axis(x, y) as usize, axis(y, x) as usize)
    }

    pub fn is_passable(&self, point: Vec2Fp) -> bool {
        let (x, y) = self.unit_cell_of(point);
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
        // Weighted all at once, with one rounding, so that a point and its image turned half
        // about the map's middle, which weigh the same four samples the other way round, stand
        // at exactly the same height: interpolating along x and then y rounded twice, from
        // opposite corners, and mirrored units a raw bit apart in height could disagree over
        // whether a target was in range.
        let (rest_x, rest_y) = (CELL_RAW - frac_x, CELL_RAW - frac_y);
        let sum = h00 * rest_x * rest_y + h10 * frac_x * rest_y + h01 * rest_x * frac_y + h11 * frac_x * frac_y;
        Fp::from_bits((sum / (CELL_RAW * CELL_RAW)) as i32)
    }
}

/// Adds smooth value noise with lattice points every `spacing` samples.
/// `amplitude` is in height steps. Integer math only.
pub(super) fn add_noise_octave(heights: &mut [i16], samples: usize, spacing: usize, amplitude: i32, rng: &mut SimRng) {
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

    /// A unit and its mirror image through the center are in mirror-image cells, wherever they
    /// stand: in a cell, on or a raw bit off the line between two, and on the lines through
    /// the center itself. Only the very center, its own image, cannot be.
    #[test]
    fn a_unit_and_its_mirror_image_are_in_mirror_image_cells() {
        let terrain = Terrain::generate(2_560, TerrainStyle::Hills, &mut SimRng::new(5));
        let cells = terrain.cells_per_side();
        let full = fp_int(2_560).to_bits();
        let cell = i32::try_from(CELL_RAW).expect("a cell fits");
        let mut values = Vec::new();
        for line in [0, 1, 7, cells as i32 / 2 - 1, cells as i32 / 2, cells as i32 / 2 + 1, cells as i32 - 1] {
            for nudge in [-9, -8, -1, 0, 1, 8, 9, cell / 2] {
                values.push((line * cell + nudge).clamp(0, full));
            }
        }
        for &x in &values {
            for &y in &values {
                if (x, y) == (full / 2, full / 2) {
                    continue;
                }
                let (cx, cy) = terrain.unit_cell_of(Vec2Fp::new(Fp::from_bits(x), Fp::from_bits(y)));
                let image = terrain.unit_cell_of(Vec2Fp::new(Fp::from_bits(full - x), Fp::from_bits(full - y)));
                assert_eq!(image, (cells - 1 - cx, cells - 1 - cy), "raw ({x}, {y}) is in cell ({cx}, {cy})");
            }
        }
    }

    #[test]
    fn mirrored_hills_are_symmetric_across_the_center() {
        let terrain = Terrain::generate(2_560, TerrainStyle::MirroredHills, &mut SimRng::new(3));
        for (x, y) in [(100, 700), (333, 2_100), (1_280, 1_280), (40, 2_500)] {
            assert_eq!(terrain.height_at(point(x, y)), terrain.height_at(point(2_560 - x, 2_560 - y)), "({x}, {y})");
        }
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
