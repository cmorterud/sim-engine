//! Maps that come with the game. They are this game's own (GAME_SPECIFICATION.md §20), so they
//! can be shipped: each is generated from a fixed seed with whole-number math, so it is the same
//! map on every machine and in every match. Both turn into themselves about the map's center, as
//! the AI's base plan does, so neither side starts better off.
//!
//! * **Highland:** a 10 km plateau of rolling hills, broken by steep rocky outcrops and ridges,
//!   strewn with rock fields to reclaim, with starts in opposite corners.
//! * **Twin Passes:** 20 km, with a sea down the middle crossed by two land bridges, which are
//!   the ways across, and forests on both shores. The starts face each other across the water.

use super::components::WreckKind;
use super::config::TERRAIN_CELL_UNITS;
use super::scenario::MapData;
use super::terrain::{add_noise_octave, HEIGHT_FRAC_BITS};
use crate::math::fp::{fp_int, Fp, Vec2Fp};
use crate::math::rng::SimRng;

/// The maps that come with the game, by the name `--map` takes.
pub const BUNDLED: [&str; 2] = ["highland", "twin_passes"];

/// A bundled map by name.
pub fn bundled(name: &str) -> Option<MapData> {
    match name {
        "highland" => Some(highland()),
        "twin_passes" => Some(twin_passes()),
        _ => None,
    }
}

/// A cell whose corners differ by more than this, in sixteenths of a unit, is too steep for land
/// units: 0.75 of height for each unit of ground, the limit maps from files use.
const STEEP: i32 = (TERRAIN_CELL_UNITS * 3 / 4) << HEIGHT_FRAC_BITS;

/// Heights in progress, and the map's own random numbers.
struct Draft {
    cells: usize,
    samples: usize,
    heights: Vec<i16>,
    rng: SimRng,
}

impl Draft {
    fn new(cells: usize, seed: u64, base_units: i32) -> Self {
        let samples = cells + 1;
        let base = (base_units << HEIGHT_FRAC_BITS) as i16;
        Self { cells, samples, heights: vec![base; samples * samples], rng: SimRng::new(seed) }
    }

    fn map_units(&self) -> i32 {
        self.cells as i32 * TERRAIN_CELL_UNITS
    }

    /// Smooth hills: lattice points `spacing` cells apart, each up to `amplitude` units up or down.
    fn hills(&mut self, spacing: usize, amplitude: i32) {
        add_noise_octave(&mut self.heights, self.samples, spacing, amplitude << HEIGHT_FRAC_BITS, &mut self.rng);
    }

    /// Changes every sample within `radius + edge` cells of a cell, by `amount` units inside
    /// `radius`, and less across the `edge` beyond it. A narrow edge makes a cliff.
    fn disc(&mut self, center: (i32, i32), radius: i32, edge: i32, amount: i32) {
        let reach = radius + edge;
        let raw = amount << HEIGHT_FRAC_BITS;
        for y in (center.1 - reach).max(0)..=(center.1 + reach).min(self.cells as i32) {
            for x in (center.0 - reach).max(0)..=(center.0 + reach).min(self.cells as i32) {
                let distance = isqrt((x - center.0).pow(2) + (y - center.1).pow(2));
                let share = if distance <= radius { 256 } else if distance < reach { 256 * (reach - distance) / edge.max(1) } else { 0 };
                let height = &mut self.heights[y as usize * self.samples + x as usize];
                *height = (i32::from(*height) + raw * share / 256).clamp(0, i32::from(i16::MAX)) as i16;
            }
        }
    }

    /// Blends the ground within `radius` cells of a cell toward the height there, flat at the
    /// middle and untouched at the rim, so that a base has room to build.
    fn flatten(&mut self, center: (i32, i32), radius: i32) {
        let level = i32::from(self.heights[center.1 as usize * self.samples + center.0 as usize]);
        for y in (center.1 - radius).max(0)..=(center.1 + radius).min(self.cells as i32) {
            for x in (center.0 - radius).max(0)..=(center.0 + radius).min(self.cells as i32) {
                let distance = isqrt((x - center.0).pow(2) + (y - center.1).pow(2));
                if distance >= radius {
                    continue;
                }
                // Fully flat out to two thirds of the way, then easing back to the land.
                let keep = (256 * (distance - radius * 2 / 3) / (radius / 3).max(1)).clamp(0, 256);
                let height = &mut self.heights[y as usize * self.samples + x as usize];
                *height = ((level * (256 - keep) + i32::from(*height) * keep) / 256) as i16;
            }
        }
    }

    /// Makes the hills turn into themselves about the map's center, by averaging each height
    /// with its partner's. Copying one half onto the other would leave a seam of false cliffs
    /// down the middle row, where each sample meets one from the far side of the map.
    fn symmetrize(&mut self) {
        let last = self.heights.len() - 1;
        for index in 0..self.heights.len() / 2 {
            let average = ((i32::from(self.heights[index]) + i32::from(self.heights[last - index])) / 2) as i16;
            self.heights[index] = average;
            self.heights[last - index] = average;
        }
    }

    /// A sample's partner, turned about the map's center.
    fn turned(&self, sample: (i32, i32)) -> (i32, i32) {
        (self.cells as i32 - sample.0, self.cells as i32 - sample.1)
    }

    /// A disc and its turned partner.
    fn disc_pair(&mut self, center: (i32, i32), radius: i32, edge: i32, amount: i32) {
        self.disc(center, radius, edge, amount);
        self.disc(self.turned(center), radius, edge, amount);
    }

    fn corner(&self, x: usize, y: usize) -> i32 {
        i32::from(self.heights[y * self.samples + x])
    }

    /// Cells too steep for land units, or wholly under the water.
    fn blocked(&self, water_raw: i32) -> Vec<bool> {
        let mut blocked = vec![false; self.cells * self.cells];
        for y in 0..self.cells {
            for x in 0..self.cells {
                let corners = [self.corner(x, y), self.corner(x + 1, y), self.corner(x, y + 1), self.corner(x + 1, y + 1)];
                let (low, high) = (corners.iter().min().copied().unwrap_or(0), corners.iter().max().copied().unwrap_or(0));
                blocked[y * self.cells + x] = high - low > STEEP || high < water_raw;
            }
        }
        blocked
    }
}

/// A smooth wavy line: `length` values, from points `spacing` apart, each up to `amplitude`
/// either way, eased between with smoothstep.
fn smooth_line(rng: &mut SimRng, length: i32, spacing: i32, amplitude: i32) -> Vec<i32> {
    let points: Vec<i32> = (0..length / spacing + 2).map(|_| rng.below(amplitude as u32 * 2 + 1) as i32 - amplitude).collect();
    (0..length)
        .map(|at| {
            let (index, offset) = ((at / spacing) as usize, i64::from(at % spacing));
            let t = (offset << 16) / i64::from(spacing);
            let weight = (t * t * ((3 << 16) - 2 * t)) >> 32;
            let (from, to) = (i64::from(points[index]), i64::from(points[index + 1]));
            (from + (((to - from) * weight) >> 16)) as i32
        })
        .collect()
}

fn isqrt(value: i32) -> i32 {
    crate::math::fp::sqrt_wide(crate::math::fp::FpWide::from_num(value.max(0))).to_num::<i32>()
}

/// Deposits, starts, and scenery placed on the high-row half and turned onto the other.
struct Layout<'a> {
    cells: i32,
    blocked: &'a [bool],
    deposits: Vec<(u32, u32)>,
    scenery: Vec<(WreckKind, Vec2Fp)>,
}

impl Layout<'_> {
    fn turned(&self, cell: (i32, i32)) -> (i32, i32) {
        (self.cells - 1 - cell.0, self.cells - 1 - cell.1)
    }

    /// Whether every cell within `margin` of a cell is open ground on the map.
    fn open(&self, cell: (i32, i32), margin: i32) -> bool {
        (cell.1 - margin..=cell.1 + margin).all(|y| {
            (cell.0 - margin..=cell.0 + margin).all(|x| (0..self.cells).contains(&x) && (0..self.cells).contains(&y) && !self.blocked[(y * self.cells + x) as usize])
        })
    }

    /// A deposit and its turned partner, if both are on open, level ground.
    fn deposit(&mut self, cell: (i32, i32)) -> bool {
        let partner = self.turned(cell);
        if !self.open(cell, 1) || !self.open(partner, 1) || self.deposits.contains(&(cell.0 as u32, cell.1 as u32)) {
            return false;
        }
        self.deposits.push((cell.0 as u32, cell.1 as u32));
        self.deposits.push((partner.0 as u32, partner.1 as u32));
        true
    }

    fn near_deposit(&self, cell: (i32, i32), cells: i32) -> bool {
        self.deposits.iter().any(|&(x, y)| (x as i32 - cell.0).abs() <= cells && (y as i32 - cell.1).abs() <= cells)
    }

    /// A clump of scenery around a point, each piece with its turned partner, kept off blocked
    /// ground, away from `avoid`, and clear of deposits.
    fn clump(&mut self, rng: &mut SimRng, kind: WreckKind, center: Vec2Fp, count: usize, spread: i32, avoid: &dyn Fn((i32, i32)) -> bool) {
        let map = fp_int(self.cells * TERRAIN_CELL_UNITS);
        let mut placed = 0;
        for _ in 0..count * 4 {
            if placed == count {
                break;
            }
            let offset = |rng: &mut SimRng| rng.range_fp(fp_int(-spread), fp_int(spread));
            let (dx, dy) = (offset(rng), offset(rng));
            if i64::from(dx.to_num::<i32>()).pow(2) + i64::from(dy.to_num::<i32>()).pow(2) > i64::from(spread).pow(2) {
                continue;
            }
            let point = Vec2Fp::new(center.x + dx, center.y + dy);
            if point.x <= Fp::ZERO || point.y <= Fp::ZERO || point.x >= map || point.y >= map {
                continue;
            }
            let cell = ((point.x.to_num::<i32>()) / TERRAIN_CELL_UNITS, (point.y.to_num::<i32>()) / TERRAIN_CELL_UNITS);
            if !self.open(cell, 0) || avoid(cell) || self.near_deposit(cell, 2) {
                continue;
            }
            self.scenery.push((kind, point));
            self.scenery.push((kind, Vec2Fp::new(map - point.x, map - point.y)));
            placed += 1;
        }
    }
}

fn cell_point(cell: (i32, i32)) -> Vec2Fp {
    Vec2Fp::new(fp_int(cell.0 * TERRAIN_CELL_UNITS + TERRAIN_CELL_UNITS / 2), fp_int(cell.1 * TERRAIN_CELL_UNITS + TERRAIN_CELL_UNITS / 2))
}

fn chebyshev(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

/// Highland, 10 km: rolling hills on a plateau, steep outcrops and ridges, rock fields.
pub fn highland() -> MapData {
    const CELLS: usize = 512;
    let mut draft = Draft::new(CELLS, 0x4849_4748, 300);
    draft.hills(80, 110);
    draft.hills(24, 28);
    draft.hills(8, 6);
    draft.symmetrize();
    let start = (143, 404);
    let last = CELLS as i32 - 1;
    let clear_of_start = |cell: (i32, i32), cells: i32| chebyshev(cell, start) > cells && chebyshev(cell, (last - start.0, last - start.1)) > cells;

    // Outcrops and ridges on the high half, steep sided, kept well away from the starts.
    let half = CELLS as i32 / 2;
    let mut outcrops = 0;
    for _ in 0..400 {
        if outcrops == 16 {
            break;
        }
        let cell = (24 + draft.rng.below(CELLS as u32 - 48) as i32, half + 12 + draft.rng.below(half as u32 - 36) as i32);
        if !clear_of_start(cell, 48) {
            continue;
        }
        let (radius, height) = (4 + draft.rng.below(9) as i32, 30 + draft.rng.below(31) as i32);
        // Some are ridges: a few outcrops in a row.
        let links = if draft.rng.below(3) == 0 { 3 + draft.rng.below(4) as i32 } else { 1 };
        let step = (draft.rng.below(9) as i32 - 4, draft.rng.below(9) as i32 - 4);
        for link in 0..links {
            let at = (cell.0 + step.0 * radius * link / 2, cell.1 + step.1 * radius * link / 2);
            if (8..CELLS as i32 - 8).contains(&at.0) && (half + 4..CELLS as i32 - 8).contains(&at.1) && clear_of_start(at, 44) {
                draft.disc_pair(at, radius, 2, height);
            }
        }
        outcrops += 1;
    }
    draft.flatten(start, 30);
    draft.flatten(draft.turned(start), 30);

    let blocked = draft.blocked(i32::MIN);
    let mut layout = Layout { cells: CELLS as i32, blocked: &blocked, deposits: Vec::new(), scenery: Vec::new() };
    for offset in [(-6, -4), (5, -6), (-4, 7), (7, 5)] {
        layout.deposit((start.0 + offset.0, start.1 + offset.1));
    }
    let mut expansions = 0;
    for _ in 0..2_000 {
        if expansions == 12 {
            break;
        }
        let cell = (16 + draft.rng.below(CELLS as u32 - 32) as i32, half + 6 + draft.rng.below(half as u32 - 22) as i32);
        if !clear_of_start(cell, 22) || layout.near_deposit(cell, 14) {
            continue;
        }
        if layout.deposit(cell) {
            expansions += 1;
            // Half of them come in pairs.
            if draft.rng.below(2) == 0 {
                layout.deposit((cell.0 + 2, cell.1 + 1));
            }
        }
    }
    let map = draft.map_units();
    let away = |cell: (i32, i32)| !clear_of_start(cell, 26);
    for (kind, groups, per_group, spread) in [(WreckKind::Rock, 32, 24, 140), (WreckKind::Tree, 8, 14, 90)] {
        for _ in 0..groups {
            let center = Vec2Fp::new(draft.rng.range_fp(fp_int(300), fp_int(map - 300)), draft.rng.range_fp(fp_int(map / 2 + 200), fp_int(map - 300)));
            layout.clump(&mut draft.rng, kind, center, per_group, spread, &away);
        }
    }
    let starts = [cell_point(start), cell_point((last - start.0, last - start.1))];
    let (deposits, scenery) = (layout.deposits, layout.scenery);
    MapData { name: "highland".into(), map_units: map, heights: draft.heights, blocked, water_level: fp_int(-1000), deposits, starts, scenery }
}

/// Twin Passes, 20 km: a sea down the middle, crossed by two land bridges, forests on the shores.
pub fn twin_passes() -> MapData {
    const CELLS: usize = 1024;
    const WATER_UNITS: i32 = 140;
    let mut draft = Draft::new(CELLS, 0x5457_494E, 300);
    draft.hills(128, 80);
    draft.hills(32, 20);
    draft.hills(10, 5);
    draft.symmetrize();
    let (center, half) = (CELLS as i32 / 2, CELLS as i32 / 2);
    // The sea: a band down the middle, with gentle shores, except where a bridge crosses, at 68%
    // of the way and at its turned partner, 32%. Both are symmetric about the center already.
    let (sea_half_width, shore, bridge_row, bridge_half_width, bridge_shore) = (84, 26, CELLS as i32 * 68 / 100, 22, 14);
    let floor = (WATER_UNITS - 50) << HEIGHT_FRAC_BITS;
    // The west shore wanders in and out along the map, smoothly, and the east shore is the west
    // one turned about the center, so the sea stays fair while its coasts look natural.
    let wander = smooth_line(&mut draft.rng, CELLS as i32 + 1, 48, 24);
    for y in 0..=CELLS as i32 {
        let (west, east) = (wander[y as usize], wander[(CELLS as i32 - y) as usize]);
        for x in 0..=CELLS as i32 {
            let across = if x <= center { center - x - west } else { x - center - east };
            let sea = if across <= sea_half_width { 256 } else if across < sea_half_width + shore { 256 * (sea_half_width + shore - across) / shore } else { 0 };
            let along = (y - bridge_row).abs().min((y - (CELLS as i32 - bridge_row)).abs());
            let bridge = if along <= bridge_half_width { 256 } else if along < bridge_half_width + bridge_shore { 256 * (bridge_half_width + bridge_shore - along) / bridge_shore } else { 0 };
            let depth = sea * (256 - bridge) / 256;
            let height = &mut draft.heights[y as usize * draft.samples + x as usize];
            *height = ((i32::from(*height) * (256 - depth) + floor * depth) / 256) as i16;
        }
    }
    let start = (110, 600);
    let last = CELLS as i32 - 1;
    let clear_of_start = |cell: (i32, i32), cells: i32| chebyshev(cell, start) > cells && chebyshev(cell, (last - start.0, last - start.1)) > cells;
    // Rocky outcrops on the land, away from the sea and the starts.
    let mut outcrops = 0;
    for _ in 0..600 {
        if outcrops == 22 {
            break;
        }
        let cell = (24 + draft.rng.below(CELLS as u32 - 48) as i32, half + 16 + draft.rng.below(half as u32 - 40) as i32);
        if (cell.0 - center).abs() < sea_half_width + shore + 20 || !clear_of_start(cell, 60) {
            continue;
        }
        let (radius, height) = (5 + draft.rng.below(10) as i32, 30 + draft.rng.below(30) as i32);
        draft.disc_pair(cell, radius, 2, height);
        outcrops += 1;
    }
    draft.flatten(start, 32);
    draft.flatten(draft.turned(start), 32);

    let water_raw = WATER_UNITS << HEIGHT_FRAC_BITS;
    let blocked = draft.blocked(water_raw);
    let mut layout = Layout { cells: CELLS as i32, blocked: &blocked, deposits: Vec::new(), scenery: Vec::new() };
    for offset in [(-6, -4), (5, -6), (-4, 7), (7, 5)] {
        layout.deposit((start.0 + offset.0, start.1 + offset.1));
    }
    // Two on each bridge, worth fighting over.
    layout.deposit((center - 40, bridge_row - 4));
    layout.deposit((center + 40, bridge_row + 4));
    let mut expansions = 0;
    for _ in 0..4_000 {
        if expansions == 20 {
            break;
        }
        let cell = (16 + draft.rng.below(CELLS as u32 - 32) as i32, half + 6 + draft.rng.below(half as u32 - 22) as i32);
        if !clear_of_start(cell, 24) || layout.near_deposit(cell, 18) {
            continue;
        }
        if layout.deposit(cell) {
            expansions += 1;
            if draft.rng.below(2) == 0 {
                layout.deposit((cell.0 + 2, cell.1 + 1));
            }
        }
    }
    let map = draft.map_units();
    let away = |cell: (i32, i32)| !clear_of_start(cell, 28);
    for (kind, groups, per_group, spread) in [(WreckKind::Tree, 40, 25, 200), (WreckKind::Rock, 14, 16, 120)] {
        for _ in 0..groups {
            let center = Vec2Fp::new(draft.rng.range_fp(fp_int(300), fp_int(map - 300)), draft.rng.range_fp(fp_int(map / 2 + 200), fp_int(map - 300)));
            layout.clump(&mut draft.rng, kind, center, per_group, spread, &away);
        }
    }
    let starts = [cell_point(start), cell_point((last - start.0, last - start.1))];
    let (deposits, scenery) = (layout.deposits, layout.scenery);
    MapData { name: "twin_passes".into(), map_units: map, heights: draft.heights, blocked, water_level: fp_int(WATER_UNITS), deposits, starts, scenery }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Whether land units can walk from one cell to another, around blocked ground.
    fn connected(map: &MapData, from: (usize, usize), to: (usize, usize)) -> bool {
        let cells = (map.map_units / TERRAIN_CELL_UNITS) as usize;
        let mut seen = vec![false; cells * cells];
        let mut queue = VecDeque::from([from]);
        seen[from.1 * cells + from.0] = true;
        while let Some((x, y)) = queue.pop_front() {
            if (x, y) == to {
                return true;
            }
            for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
                if nx < cells && ny < cells && !seen[ny * cells + nx] && !map.blocked[ny * cells + nx] {
                    seen[ny * cells + nx] = true;
                    queue.push_back((nx, ny));
                }
            }
        }
        false
    }

    fn cell_of(map: &MapData, point: Vec2Fp) -> (usize, usize) {
        let last = (map.map_units / TERRAIN_CELL_UNITS - 1) as usize;
        let cell = |value: Fp| ((value.to_num::<i32>() / TERRAIN_CELL_UNITS) as usize).min(last);
        (cell(point.x), cell(point.y))
    }

    fn check(map: &MapData) {
        let cells = (map.map_units / TERRAIN_CELL_UNITS) as usize;
        assert_eq!(map.heights.len(), (cells + 1) * (cells + 1));
        // It turns into itself about the center: heights, blocked cells, deposits, and starts.
        let last = map.heights.len() - 1;
        assert!((0..map.heights.len()).all(|index| map.heights[index] == map.heights[last - index]), "heights mirror");
        let last_cell = map.blocked.len() - 1;
        assert!((0..map.blocked.len()).all(|index| map.blocked[index] == map.blocked[last_cell - index]), "blocked cells mirror");
        for &(x, y) in &map.deposits {
            let partner = (cells as u32 - 1 - x, cells as u32 - 1 - y);
            assert!(map.deposits.contains(&partner), "deposit {x}, {y} has no partner");
            assert!(!map.blocked[y as usize * cells + x as usize], "deposit {x}, {y} is on blocked ground");
        }
        let full = fp_int(map.map_units);
        assert_eq!(map.starts[1], Vec2Fp::new(full - map.starts[0].x, full - map.starts[0].y), "the starts mirror");
        // Each start is open, and the two are joined by land.
        let (blue, red) = (cell_of(map, map.starts[0]), cell_of(map, map.starts[1]));
        assert!(!map.blocked[blue.1 * cells + blue.0] && !map.blocked[red.1 * cells + red.0]);
        assert!(connected(map, blue, red), "the starts must be joined by land");
        assert!(map.deposits.len() >= 30, "{} deposits", map.deposits.len());
    }

    #[test]
    fn highland_is_fair_open_where_it_matters_and_the_same_every_time() {
        let map = highland();
        check(&map);
        assert_eq!(map, highland(), "the same every time");
        let steep = map.blocked.iter().filter(|&&blocked| blocked).count();
        assert!((1_000..40_000).contains(&steep), "outcrops and ridges block some ground: {steep} cells");
        let rocks = map.scenery.iter().filter(|(kind, _)| *kind == WreckKind::Rock).count();
        assert!(rocks > 800, "{rocks} rocks");
    }

    #[test]
    fn twin_passes_has_a_sea_crossed_by_two_bridges() {
        let map = twin_passes();
        check(&map);
        let cells = (map.map_units / TERRAIN_CELL_UNITS) as usize;
        // The middle column is sea, except on the two bridges, at about 32% and 68% of the way.
        let middle = cells / 2;
        let water: Vec<usize> = (0..cells).filter(|&y| map.blocked[y * cells + middle]).collect();
        assert!(water.len() > cells / 2, "most of the middle is water: {}", water.len());
        for row in [cells * 32 / 100, cells * 68 / 100] {
            assert!(!map.blocked[row * cells + middle], "a bridge crosses at row {row}");
        }
        let trees = map.scenery.iter().filter(|(kind, _)| *kind == WreckKind::Tree).count();
        assert!(trees > 1_000, "{trees} trees");
        assert_eq!(bundled("twin_passes").as_ref(), Some(&map));
        assert!(bundled("nowhere").is_none());
    }
}
