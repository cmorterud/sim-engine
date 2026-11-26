//! Maps that come with the game. They are this game's own (GAME_SPECIFICATION.md §20), so they
//! can be shipped: each is generated from a fixed seed with whole-number math, so it is the same
//! map on every machine and in every match. Both turn into themselves about the map's center, as
//! the AI's base plan does, so neither side starts better off.
//!
//! * **Highland:** a 10 km plateau of rolling hills, broken by steep rocky outcrops and ridges,
//!   strewn with rock fields to reclaim, with starts in opposite corners.
//! * **Twin Passes:** 20 km, with a sea down the middle crossed by two land bridges, which are
//!   the ways across, and forests on both shores. The starts face each other across the water.
//!
//! Named maps (GAME_SPECIFICATION.md §17.3) are a kind of map and its settings, in a file in
//! data/maps, from which the kind's generator here makes the map: Saddleback is a Passes map,
//! Stillwater a Central Lake, and Scattered Isles an Islands map.

use super::components::WreckKind;
use super::config::TERRAIN_CELL_UNITS;
use super::scenario::MapData;
use super::terrain::{add_noise_octave, HEIGHT_FRAC_BITS, SEA_DEPTH_UNITS};
use crate::math::fp::{fp_int, Fp, Vec2Fp};
use crate::math::rng::SimRng;

/// The maps that come with the game, by the name `--map` takes.
pub const BUNDLED: [&str; 5] = ["highland", "twin_passes", "saddleback", "stillwater", "scattered_isles"];

/// A bundled map by name.
pub fn bundled(name: &str) -> Option<MapData> {
    match name {
        "highland" => Some(highland()),
        "twin_passes" => Some(twin_passes()),
        _ => settings(name).map(|settings| generate(&settings)),
    }
}

/// A map from its settings, by its kind.
pub fn generate(settings: &MapSettings) -> MapData {
    match settings.kind.as_str() {
        "passes" => passes(settings),
        "central_lake" => central_lake(settings),
        "islands" => islands(settings),
        kind => panic!("{}: {kind} is not a kind of map", settings.id),
    }
}

/// A named map's settings, as its file in data/maps gives them (GAME_SPECIFICATION.md §17.3):
/// a kind of map, and what it is generated from.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapSettings {
    pub id: String,
    pub name: String,
    /// "passes", "central_lake", or "islands".
    pub kind: String,
    pub size_km: u32,
    /// The most players it holds.
    pub players: u32,
    pub seed: u64,
    /// "sparse", "normal", or "rich": deposits and vents.
    pub resources: String,
    /// "none", "some", or "lots": rock fields and woods.
    pub reclaim: String,
    /// "flat", "hilly", or "rugged": the land away from the map's features.
    pub terrain: String,
    /// Passes: how many valleys cross the range, an odd number.
    #[serde(default)]
    pub valleys: u32,
    /// "some" or "lots": how much of the map is under water, within what its kind allows. A
    /// Passes map has none.
    #[serde(default)]
    pub water: String,
}

/// What a player chose for a generated map (GAME_SPECIFICATION.md §17.3): its kind, and each
/// setting as given, or nothing for the usual.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choices {
    pub kind: String,
    pub size_km: Option<u32>,
    pub players: Option<u32>,
    pub resources: Option<String>,
    pub reclaim: Option<String>,
    pub terrain: Option<String>,
    pub water: Option<String>,
    pub seed: Option<u64>,
}

impl MapSettings {
    /// The kinds of map the generator makes.
    pub const KINDS: [&'static str; 3] = ["passes", "central_lake", "islands"];
    /// The sizes it makes them, in km: those that come to a whole, even number of cells.
    pub const SIZES_KM: [u32; 4] = [5, 10, 15, 20];

    /// The most players a map of a size holds: 2 at 5 km, 4 up to 15 km, and 8 at 20 km.
    pub const fn most_players(size_km: u32) -> u32 {
        match size_km {
            0..=5 => 2,
            6..=15 => 4,
            _ => 8,
        }
    }

    /// A generated map's settings from what a player chose, or what is wrong with the choice.
    /// Left unsaid: 10 km, as many players as the size holds up to four, normal resources, some
    /// reclaim, hilly, some water where the kind has any, and seed 1.
    pub fn chosen(choices: &Choices) -> Result<Self, String> {
        let one_of = |what: &str, given: &Option<String>, usual: &str, allowed: &[&str]| {
            let word = given.as_deref().unwrap_or(usual);
            if allowed.contains(&word) { Ok(word.to_string()) } else { Err(format!("{what} is {}, not {word}", allowed.join(", "))) }
        };
        if choices.kind.is_empty() {
            return Err(format!("a generated map needs its kind: {}", Self::KINDS.join(", ")));
        }
        let kind = one_of("a map's kind", &Some(choices.kind.clone()), "", &Self::KINDS)?;
        let size_km = choices.size_km.unwrap_or(10);
        if !Self::SIZES_KM.contains(&size_km) {
            return Err(format!("a map is 5, 10, 15, or 20 km, not {size_km}"));
        }
        let most = Self::most_players(size_km);
        let players = choices.players.unwrap_or_else(|| most.min(4));
        if ![2, 4, 8].contains(&players) || players > most {
            return Err(format!("a {size_km} km map holds 2{} players, not {players}", if most >= 8 { ", 4, or 8" } else if most >= 4 { " or 4" } else { "" }));
        }
        let water = if kind == "passes" {
            match choices.water.as_deref() {
                None | Some("none") => String::new(),
                Some(other) => return Err(format!("a Passes map has no water, not {other}")),
            }
        } else {
            one_of("water", &choices.water, "some", &["some", "lots"])?
        };
        Ok(Self {
            id: "generated".into(),
            name: "Generated".into(),
            // One valley on the smallest Passes, where three would each be too narrow to fight in.
            valleys: if kind == "passes" { if size_km <= 5 { 1 } else { 3 } } else { 0 },
            kind,
            size_km,
            players,
            seed: choices.seed.unwrap_or(1),
            resources: one_of("resources", &choices.resources, "normal", &["sparse", "normal", "rich"])?,
            reclaim: one_of("reclaim", &choices.reclaim, "some", &["none", "some", "lots"])?,
            terrain: one_of("terrain", &choices.terrain, "hilly", &["flat", "hilly", "rugged"])?,
            water,
        })
    }
}

/// The named maps' settings files, by id.
const NAMED: [(&str, &str); 3] = [
    ("saddleback", include_str!("../../data/maps/saddleback.toml")),
    ("stillwater", include_str!("../../data/maps/stillwater.toml")),
    ("scattered_isles", include_str!("../../data/maps/scattered_isles.toml")),
];

/// A named map's settings, by id.
pub fn settings(id: &str) -> Option<MapSettings> {
    NAMED.iter().find(|(name, _)| *name == id).map(|(_, text)| toml::from_str(text).unwrap_or_else(|error| panic!("data/maps/{id}.toml: {error}")))
}

/// Saddleback, 10 km: a mountain range across the middle, crossed by three valleys.
pub fn saddleback() -> MapData {
    passes(&settings("saddleback").expect("data/maps/saddleback.toml"))
}

/// Stillwater, 10 km: a lake in the middle, inside a ring of land, with starts in the corners.
pub fn stillwater() -> MapData {
    central_lake(&settings("stillwater").expect("data/maps/stillwater.toml"))
}

/// Scattered Isles, 20 km: a home island at each end, and a sea of small islands between.
pub fn scattered_isles() -> MapData {
    islands(&settings("scattered_isles").expect("data/maps/scattered_isles.toml"))
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
    eased(&points, length, spacing)
}

/// Keeps a wandering shore's points within `most` of their neighbors, leaving the `pinned` ones
/// where they are. A shore that swings in or out faster than about half as far as it runs is,
/// with the fall of the beach, a wedge of ground too steep to cross.
fn gentle(points: &mut [i32], most: i32, pinned: &[usize]) {
    for _ in 0..4 {
        for index in 1..points.len() {
            if !pinned.contains(&index) {
                points[index] = points[index].clamp(points[index - 1] - most, points[index - 1] + most);
            }
        }
        for index in (0..points.len() - 1).rev() {
            if !pinned.contains(&index) {
                points[index] = points[index].clamp(points[index + 1] - most, points[index + 1] + most);
            }
        }
    }
}

/// `length` values through points `spacing` apart, eased between with smoothstep, so that the
/// line is level at each point.
fn eased(points: &[i32], length: i32, spacing: i32) -> Vec<i32> {
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

impl Draft {
    /// A sample and its images in both mirrors: across the map's middle column, its middle row,
    /// and both. A map with them all is fair for four players, two each side, and turns into
    /// itself about its center as well, for two.
    fn images(&self, sample: (i32, i32)) -> [(i32, i32); 4] {
        let last = self.cells as i32;
        [sample, (last - sample.0, sample.1), (sample.0, last - sample.1), (last - sample.0, last - sample.1)]
    }

    /// Makes the heights their own images in both mirrors, by averaging each with its images.
    fn symmetrize_four(&mut self) {
        let half = self.cells as i32 / 2;
        for y in 0..=half {
            for x in 0..=half {
                let images = self.images((x, y));
                let index = |(x, y): (i32, i32)| y as usize * self.samples + x as usize;
                let total: i32 = images.iter().map(|&image| i32::from(self.heights[index(image)])).sum();
                for image in images {
                    let at = index(image);
                    self.heights[at] = (total / 4) as i16;
                }
            }
        }
    }

    /// A disc and its images in both mirrors.
    fn disc_four(&mut self, center: (i32, i32), radius: i32, edge: i32, amount: i32) {
        for image in self.images(center) {
            self.disc(image, radius, edge, amount);
        }
    }
}

/// Deposits, starts, and scenery placed on the high-row half and turned onto the other.
struct Layout<'a> {
    cells: i32,
    blocked: &'a [bool],
    deposits: Vec<(u32, u32)>,
    vents: Vec<(u32, u32)>,
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

    /// A geothermal vent and its turned partner, if a plant would fit on open, level ground
    /// over both, away from deposits and other vents.
    fn vent(&mut self, cell: (i32, i32)) -> bool {
        let partner = self.turned(cell);
        let crowded = |layout: &Self, at: (i32, i32)| {
            layout.near_deposit(at, 3) || layout.vents.iter().any(|&(x, y)| chebyshev((x as i32, y as i32), at) < 12)
        };
        if !self.open(cell, 2) || !self.open(partner, 2) || crowded(self, cell) || crowded(self, partner) {
            return false;
        }
        self.vents.push((cell.0 as u32, cell.1 as u32));
        self.vents.push((partner.0 as u32, partner.1 as u32));
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

/// One vent near the start, as close to `near` from it as fits, and the rest anywhere on the
/// high half that `allowed` accepts, each with its turned partner.
fn place_vents(layout: &mut Layout<'_>, rng: &mut SimRng, start: (i32, i32), near: (i32, i32), count: usize, allowed: &dyn Fn((i32, i32)) -> bool) {
    let home = (0..6).find_map(|step| {
        let at = (start.0 + near.0 + step * 2, start.1 + near.1);
        layout.vent(at).then_some(at)
    });
    let mut placed = usize::from(home.is_some());
    let half = layout.cells / 2;
    for _ in 0..3_000 {
        if placed == count {
            break;
        }
        let cell = (16 + rng.below(layout.cells as u32 - 32) as i32, half + 8 + rng.below(half as u32 - 24) as i32);
        if allowed(cell) && layout.vent(cell) {
            placed += 1;
        }
    }
}

impl Layout<'_> {
    /// A cell and its images in both mirrors.
    fn images(&self, cell: (i32, i32)) -> [(i32, i32); 4] {
        let last = self.cells - 1;
        [cell, (last - cell.0, cell.1), (cell.0, last - cell.1), (last - cell.0, last - cell.1)]
    }

    /// A deposit and its images in both mirrors, if all are on open, level ground.
    fn deposit_four(&mut self, cell: (i32, i32)) -> bool {
        let images = self.images(cell);
        if images.iter().any(|&image| !self.open(image, 1) || self.deposits.contains(&(image.0 as u32, image.1 as u32))) {
            return false;
        }
        self.deposits.extend(images.iter().map(|&(x, y)| (x as u32, y as u32)));
        true
    }

    /// A geothermal vent and its images in both mirrors, if a plant fits over each, away from
    /// deposits and other vents.
    fn vent_four(&mut self, cell: (i32, i32)) -> bool {
        let images = self.images(cell);
        let crowded = |layout: &Self, at: (i32, i32)| layout.near_deposit(at, 3) || layout.vents.iter().any(|&(x, y)| chebyshev((x as i32, y as i32), at) < 12);
        if images.iter().any(|&image| !self.open(image, 2) || crowded(self, image)) {
            return false;
        }
        self.vents.extend(images.iter().map(|&(x, y)| (x as u32, y as u32)));
        true
    }

    /// A clump of scenery around a point, each piece with its images in both mirrors, kept off
    /// blocked ground, away from `avoid`, and clear of deposits.
    fn clump_four(&mut self, rng: &mut SimRng, kind: WreckKind, center: Vec2Fp, count: usize, spread: i32, avoid: &dyn Fn((i32, i32)) -> bool) {
        let map = fp_int(self.cells * TERRAIN_CELL_UNITS);
        let mut placed = 0;
        for _ in 0..count * 4 {
            if placed == count {
                break;
            }
            let offset = |rng: &mut SimRng| rng.range_fp(fp_int(-spread), fp_int(spread));
            let (dx, dy) = (offset(rng), offset(rng));
            let point = Vec2Fp::new(center.x + dx, center.y + dy);
            if point.x <= Fp::ZERO || point.y <= Fp::ZERO || point.x >= map || point.y >= map {
                continue;
            }
            let cell = ((point.x.to_num::<i32>()) / TERRAIN_CELL_UNITS, (point.y.to_num::<i32>()) / TERRAIN_CELL_UNITS);
            if self.images(cell).iter().any(|&image| !self.open(image, 0) || avoid(image) || self.near_deposit(image, 2)) {
                continue;
            }
            for image in [point, Vec2Fp::new(map - point.x, point.y), Vec2Fp::new(point.x, map - point.y), Vec2Fp::new(map - point.x, map - point.y)] {
                self.scenery.push((kind, image));
            }
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
    let mut layout = Layout { cells: CELLS as i32, blocked: &blocked, deposits: Vec::new(), vents: Vec::new(), scenery: Vec::new() };
    for offset in [(-6, -4), (5, -6), (-4, 7), (7, 5)] {
        layout.deposit((start.0 + offset.0, start.1 + offset.1));
    }
    // A vent a short walk from each base, and two more out on the plateau to fight over.
    place_vents(&mut layout, &mut draft.rng, start, (16, -14), 3, &|cell| clear_of_start(cell, 24));
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
    let (deposits, vents, scenery) = (layout.deposits, layout.vents, layout.scenery);
    MapData { name: "highland".into(), map_units: map, heights: draft.heights, blocked, water_level: fp_int(-1000), deposits, vents, starts, layouts: Vec::new(), scenery }
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
    let floor = (WATER_UNITS - SEA_DEPTH_UNITS) << HEIGHT_FRAC_BITS;
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
    let mut layout = Layout { cells: CELLS as i32, blocked: &blocked, deposits: Vec::new(), vents: Vec::new(), scenery: Vec::new() };
    for offset in [(-6, -4), (5, -6), (-4, 7), (7, 5)] {
        layout.deposit((start.0 + offset.0, start.1 + offset.1));
    }
    // A vent near each base, and three more on each shore.
    place_vents(&mut layout, &mut draft.rng, start, (18, 14), 4, &|cell| clear_of_start(cell, 26) && (cell.0 - center).abs() > sea_half_width + shore + 30);
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
    let (deposits, vents, scenery) = (layout.deposits, layout.vents, layout.scenery);
    MapData { name: "twin_passes".into(), map_units: map, heights: draft.heights, blocked, water_level: fp_int(WATER_UNITS), deposits, vents, starts, layouts: Vec::new(), scenery }
}

/// A Passes map (GAME_SPECIFICATION.md §17.3) from its settings: a mountain range across the
/// middle, impassable to land units, crossed by valleys, a wide one in the center and narrower
/// ones toward the flanks, whose floors rise gently to a saddle. Each half behind it is open
/// land. It is its own image in both mirrors, so that its four team starts, two each side, are
/// all alike, and it turns into itself about its center, for its 1v1 starts, which face the
/// center valley. The richest deposits are in the valleys.
pub fn passes(settings: &MapSettings) -> MapData {
    assert_eq!(settings.kind, "passes", "{} is not a Passes map", settings.id);
    assert!(settings.valleys % 2 == 1, "{}: valleys must be odd, one in the center", settings.id);
    let cells = (settings.size_km as i32 * 1024 / TERRAIN_CELL_UNITS) as usize;
    let mut draft = Draft::new(cells, settings.seed, 300);
    let (broad, middling, fine) = relief(&settings.terrain);
    draft.hills(80, broad);
    draft.hills(24, middling);
    draft.hills(8, fine);
    let last = cells as i32;
    let middle = last / 2;
    // Sizes are for 10 km, and scale with the map.
    let scale = |value: i32| value * last / 512;
    // The range: a band across the middle row, rising steeply on both sides. Its valleys: the
    // center one wide, the rest narrower, spread evenly out to the flanks. A valley's floor rises
    // only a little to its saddle, 10 units: at 40 it hid one end of the valley from the other,
    // so that a point defense at one end could not fire at what stood just past the middle.
    let (band, edge, rise, saddle) = (scale(36), 5, 220, 10);
    // Its edges wander in and out along the map, smoothly, the same on both sides of the middle
    // column and of the middle row, so that it stays fair while looking natural.
    let wander = smooth_line(&mut draft.rng, middle + 1, 40, scale(10));
    let reach = |x: i32| band + wander[x.min(last - x) as usize];
    // Each valley is an hourglass, two triangles tip to tip: widest where it opens onto either
    // side, narrowing to its choke at the middle row, so that each mouth is a funnel to fight
    // over and the choke is short. By center, and half its width at the mouths and at the choke.
    let flanks = settings.valleys as i32 / 2;
    let mut valleys = vec![(middle, scale(30), scale(10))];
    for step in 1..=flanks {
        let offset = scale(150) * step / flanks.max(1);
        valleys.push((middle - offset, scale(15), scale(5)));
        valleys.push((middle + offset, scale(15), scale(5)));
    }
    let half_width = |center: i32, mouth: i32, choke: i32, y: i32| {
        let band = reach(center).max(1);
        choke + (mouth - choke) * (y - middle).abs().min(band) / band
    };
    let gap = |x: i32, y: i32| {
        valleys
            .iter()
            .map(|&(center, mouth, choke)| {
                let (off, width) = ((x - center).abs(), half_width(center, mouth, choke, y));
                if off <= width { 256 } else if off < width + edge { 256 * (width + edge - off) / edge } else { 0 }
            })
            .max()
            .unwrap_or(0)
    };
    let across = |x: i32, y: i32| {
        let (off, band) = ((y - middle).abs(), reach(x));
        if off <= band { 256 } else if off < band + edge { 256 * (band + edge - off) / edge } else { 0 }
    };
    let ridge = |x: i32, y: i32| rise * across(x, y) * (256 - gap(x, y)) / 65_536;
    for y in 0..=last {
        for x in 0..=last {
            // A valley's floor rises gently toward the middle row, to a saddle.
            let toward = (band + edge - (y - middle).abs()).max(0);
            let up = (ridge(x, y) << HEIGHT_FRAC_BITS) + ((saddle * gap(x, y) * toward / (256 * (band + edge))) << HEIGHT_FRAC_BITS);
            let height = &mut draft.heights[y as usize * draft.samples + x as usize];
            *height = (i32::from(*height) + up).clamp(0, i32::from(i16::MAX)) as i16;
        }
    }
    // Peaks along the range, clear of the valleys.
    let mut peaks = 0;
    for _ in 0..400 {
        if peaks == scale(24) {
            break;
        }
        let at = (6 + draft.rng.below((middle - 6) as u32) as i32, middle + draft.rng.below(band as u32) as i32);
        if gap(at.0 - 10, at.1).max(gap(at.0 + 10, at.1)) > 0 {
            continue;
        }
        let (radius, height) = (3 + draft.rng.below(6) as i32, 30 + draft.rng.below(60) as i32);
        draft.disc_four(at, radius, 3, height);
        peaks += 1;
    }
    // The starts: in 1v1 one each side on the middle column, facing the center valley; in 2v2
    // two each side, each nearest a flank valley; and in 3v3 all three each side.
    let duel = (middle, middle + scale(150));
    let team = (middle - scale(150), middle + scale(144));
    // On a map for eight players, two more each side, between those and the middle column.
    let inner = (middle - scale(50), middle + scale(150));
    let sites: Vec<(i32, i32)> = match settings.players {
        0..=2 => vec![duel],
        3..=6 => vec![duel, team],
        _ => vec![duel, team, inner],
    };
    for &start in &sites {
        for image in draft.images(start) {
            draft.flatten(image, scale(30).max(22));
        }
    }
    draft.symmetrize_four();

    let mut blocked = draft.blocked(i32::MIN);
    // The range is impassable outright, its top too, which no land unit could reach anyway.
    for y in 0..cells {
        for x in 0..cells {
            let corners = [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)];
            if corners.iter().any(|&(cx, cy)| ridge(cx as i32, cy as i32) > 0) {
                blocked[y * cells + x] = true;
            }
        }
    }
    let mut layout = Layout { cells: last, blocked: &blocked, deposits: Vec::new(), vents: Vec::new(), scenery: Vec::new() };
    // Each 1v1 base has four deposits, and each team base four.
    for offset in [(-6, -4), (-5, 7)] {
        layout.deposit_four((duel.0 + offset.0, duel.1 + offset.1));
    }
    for &start in &sites[1..] {
        for offset in [(-6, -4), (5, -6), (-4, 7), (7, 5)] {
            layout.deposit_four((start.0 + offset.0, start.1 + offset.1));
        }
    }
    // The valleys hold the richest, out in their funnels three quarters of the way to the
    // mouths: eight in the center one, two in each of the others.
    let funnel = |center: i32| middle + reach(center) * 3 / 4;
    for at in [(middle - scale(14), funnel(middle)), (middle - scale(5), funnel(middle) + 3)] {
        layout.deposit_four(at);
    }
    for step in 1..=flanks {
        let center = middle - scale(150) * step / flanks.max(1);
        layout.deposit_four((center - scale(4), funnel(center)));
    }
    // A vent a short walk from every base.
    for &start in &sites {
        let _ = (0..6).find(|step| layout.vent_four((start.0 + 14 + step * 2, start.1 - 14)));
    }
    // Expansions out toward the flanks, more or fewer by the map's resources, and by its size:
    // by its area up to 10 km, and by its length beyond.
    let wanted = match settings.resources.as_str() {
        "sparse" => 6,
        "rich" => 14,
        _ => 9,
    };
    let wanted = if last <= 512 { (wanted * last * last / (512 * 512)).max(1) } else { wanted * last / 512 };
    let start_cells: Vec<(i32, i32)> = sites.iter().flat_map(|&start| layout.images(start)).collect();
    let near_start = |cell: (i32, i32), cells: i32| start_cells.iter().any(|&start| chebyshev(cell, start) <= cells);
    let widest = band + scale(10) + edge;
    let lowland = (middle + widest + 12, last - 16);
    let mut expansions = 0;
    for _ in 0..4_000 {
        if expansions == wanted {
            break;
        }
        let cell = (16 + draft.rng.below((middle - 28) as u32) as i32, lowland.0 + draft.rng.below((lowland.1 - lowland.0) as u32) as i32);
        if near_start(cell, 22) || layout.near_deposit(cell, 14) {
            continue;
        }
        if layout.deposit_four(cell) {
            expansions += 1;
            if draft.rng.below(2) == 0 {
                layout.deposit_four((cell.0 + 2, cell.1 + 1));
            }
        }
    }
    // Rock fields along the foot of the range, and woods out on the land.
    let amount = match settings.reclaim.as_str() {
        "none" => 0,
        "lots" => 2,
        _ => 1,
    };
    let map = draft.map_units();
    let away = |cell: (i32, i32)| near_start(cell, 26);
    let foot = (middle + widest + 4) * TERRAIN_CELL_UNITS;
    for _ in 0..scale(10) * amount {
        let center = Vec2Fp::new(draft.rng.range_fp(fp_int(300), fp_int(map / 2 - 200)), draft.rng.range_fp(fp_int(foot), fp_int(foot + 300)));
        layout.clump_four(&mut draft.rng, WreckKind::Rock, center, 14, 120, &away);
    }
    for _ in 0..scale(8) * amount {
        let center = Vec2Fp::new(draft.rng.range_fp(fp_int(300), fp_int(map / 2 - 200)), draft.rng.range_fp(fp_int(foot + 600), fp_int(map - 300)));
        layout.clump_four(&mut draft.rng, WreckKind::Tree, center, 16, 110, &away);
    }
    // Starts stand in the middle of a cell, never on a line between cells: a unit on the line
    // through the map's middle has two cells equally near the center, and took the same one for
    // both sides, so the sides stopped being mirror images on the first tick, and Red won
    // three matches in five.
    let point = |(x, y): (i32, i32)| Vec2Fp::new(fp_int(x * TERRAIN_CELL_UNITS + TERRAIN_CELL_UNITS / 2), fp_int(y * TERRAIN_CELL_UNITS + TERRAIN_CELL_UNITS / 2));
    let full = fp_int(map);
    let mirror = |at: Vec2Fp| [at, Vec2Fp::new(full - at.x, at.y), Vec2Fp::new(at.x, full - at.y), Vec2Fp::new(full - at.x, full - at.y)];
    let duel_at = point(duel);
    let starts = [duel_at, Vec2Fp::new(full - duel_at.x, full - duel_at.y)];
    // Two each side: the first team's in the north, the second's in the south. On a map for six
    // players, three each side as well: the 1v1's start between those two, each of the second
    // team's the image of the first's, turned about the center. On a map for eight, four each
    // side, from west to east.
    let [north_west, north_east, south_west, south_east] = mirror(point(team));
    let mut layouts = Vec::new();
    if settings.players >= 4 {
        layouts.push(vec![north_west, north_east, south_west, south_east]);
    }
    if settings.players == 6 {
        layouts.push(vec![north_west, starts[0], north_east, south_east, starts[1], south_west]);
    }
    if settings.players > 6 {
        let [inner_north_west, inner_north_east, inner_south_west, inner_south_east] = mirror(point(inner));
        layouts.push(vec![north_west, inner_north_west, inner_north_east, north_east, south_west, inner_south_west, inner_south_east, south_east]);
    }
    let (deposits, vents, scenery) = (layout.deposits, layout.vents, layout.scenery);
    // Made with the range across the map, and turned so that it runs up it: the sides face each
    // other west and east, as the camera sees both alike, rather than one looking down at the
    // other.
    quarter_turn(MapData { name: settings.id.clone(), map_units: map, heights: draft.heights, blocked, water_level: fp_int(-1000), deposits, vents, starts, layouts, scenery })
}

/// How high the hills of a map's open land are, by its terrain setting: broad, middling, and
/// fine, in units.
fn relief(terrain: &str) -> (i32, i32, i32) {
    match terrain {
        "flat" => (20, 6, 2),
        "rugged" => (110, 30, 8),
        _ => (60, 18, 5),
    }
}

/// A Central Lake map (GAME_SPECIFICATION.md §17.3) from its settings: a lake in the middle,
/// inside a ring of land narrowest at the middle of each side and widest in the corners, where
/// the starts are. Land armies go round the lake either way; ships and hovercraft cross it, and
/// only they reach the islands in it, which hold deposits. It is its own image in both mirrors,
/// so that its four starts are all alike: in 1v1 two opposite corners, and in 2v2 each team the
/// two corners of one shore, west or east, with the lake between the teams. The richest
/// deposits on land are on the narrow ground north and south of the lake, between the teams.
pub fn central_lake(settings: &MapSettings) -> MapData {
    const WATER_UNITS: i32 = 140;
    /// How far a shore takes to go down from the land to the lake's floor, in cells, whatever
    /// the map's size: any shorter and it would be too steep to walk down.
    const SHORE: i32 = 26;
    /// The least land left between the lake and the map's edge, in cells.
    const NECK: i32 = 28;
    assert_eq!(settings.kind, "central_lake", "{} is not a Central Lake map", settings.id);
    let cells = (settings.size_km as i32 * 1024 / TERRAIN_CELL_UNITS) as usize;
    let mut draft = Draft::new(cells, settings.seed, 300);
    let (broad, middling, fine) = relief(&settings.terrain);
    draft.hills(80, broad);
    draft.hills(24, middling);
    draft.hills(8, fine);
    let last = cells as i32;
    let middle = last / 2;
    // Sizes are for 10 km, and scale with the map.
    let scale = |value: i32| value * last / 512;
    // The lake: about two fifths of the map with lots of water and a quarter with some, by how
    // far its floor reaches from the middle; the water's edge is a little way up its shore. On a
    // small map it is no wider than leaves a neck of land at the middle of each side.
    let sway = scale(24);
    let wide = last * if settings.water == "lots" { 345 } else { 270 } / 1000;
    let radius = wide.min(middle - SHORE - NECK);
    // Its shore wanders in and out round it, smoothly, the same in every quarter of the map, so
    // that it stays fair while looking natural: by how far round its quarter a sample is, from
    // the middle row to the middle column. It keeps to its plain width where the quarters meet,
    // at the middle of each side, so that the necks of land there are as wide on every map. The
    // shore is worked out in sixteenths of a cell: in whole cells it stepped a cell in or out
    // from one sample to the next, and each step was a line of ground too steep to cross.
    const FINE: i32 = 16;
    // Three points a quarter, each no further in or out than its neighbors than two fifths of
    // the way between them along the shore where they are closest, at the middle of a side.
    let (steps, spacing) = (3, scale(320).max(64));
    let mut points: Vec<i32> = (0..steps + 2).map(|_| draft.rng.below((sway * FINE) as u32 * 2 + 1) as i32 - sway * FINE).collect();
    points[0] = 0;
    points[steps as usize] = 0;
    points[steps as usize + 1] = 0;
    gentle(&mut points, radius * FINE * 2 / (5 * steps), &[0, steps as usize, steps as usize + 1]);
    let wander = eased(&points, steps * spacing + 1, spacing);
    let reach = |dx: i32, dy: i32| {
        let (across, along) = (dx.abs(), dy.abs());
        let round = if across + along == 0 { 0 } else { along * steps * spacing / (across + along) };
        radius * FINE + wander[round as usize]
    };
    let from_middle = |at: (i32, i32)| isqrt((at.0 - middle).pow(2) + (at.1 - middle).pow(2));
    let floor = (WATER_UNITS - SEA_DEPTH_UNITS) << HEIGHT_FRAC_BITS;
    let furthest = radius + sway + SHORE;
    for y in (middle - furthest).max(0)..=(middle + furthest).min(last) {
        for x in (middle - furthest).max(0)..=(middle + furthest).min(last) {
            let (dx, dy) = (x - middle, y - middle);
            let out = isqrt((dx * dx + dy * dy) * FINE * FINE) - reach(dx, dy);
            let depth = if out <= 0 { 256 } else if out < SHORE * FINE { 256 * (SHORE * FINE - out) / (SHORE * FINE) } else { 0 };
            let height = &mut draft.heights[y as usize * draft.samples + x as usize];
            *height = ((i32::from(*height) * (256 - depth) + floor * depth) / 256) as i16;
        }
    }
    // Islands: one in the middle of the lake and one out in each quarter of it, low and flat,
    // with shores gentle enough for hovercraft to climb.
    let (island_rise, island_shore) = (80, 8);
    let islet = radius * 55 / 100 * 181 / 256;
    draft.disc((middle, middle), scale(12).max(8), island_shore, island_rise);
    draft.disc_four((middle - islet, middle - islet), scale(8).max(6), island_shore, island_rise);
    // The starts, in the corners. Around each, and in a lane round the lake's shore, the land is
    // kept clear of outcrops, and the two overlap, so that no outcrops wall a start off from the
    // way round.
    let start = (scale(58), scale(58));
    // On a map for eight players, another on each shore between a corner and the middle of the
    // side, halfway from the map's edge to the lake.
    let between = ((middle - radius - SHORE) / 2, middle - scale(90));
    let sites: Vec<(i32, i32)> = if settings.players > 4 { vec![start, between] } else { vec![start] };
    let clear = scale(44).max(30);
    let lane = radius + sway + SHORE + 24;
    let outcrops = match settings.terrain.as_str() {
        "flat" => 0,
        "rugged" => scale(10),
        _ => scale(5),
    };
    let mut placed = 0;
    for _ in 0..400 {
        if placed == outcrops {
            break;
        }
        let cell = (8 + draft.rng.below((middle - 8) as u32) as i32, 8 + draft.rng.below((middle - 8) as u32) as i32);
        if from_middle(cell) < lane + 12 || sites.iter().any(|&site| chebyshev(cell, site) < clear + 12) {
            continue;
        }
        let (size, height) = (3 + draft.rng.below(6) as i32, 30 + draft.rng.below(40) as i32);
        draft.disc_four(cell, size, 2, height);
        placed += 1;
    }
    for &site in &sites {
        for image in draft.images(site) {
            draft.flatten(image, scale(30).max(22));
        }
    }
    draft.symmetrize_four();

    let blocked = draft.blocked(WATER_UNITS << HEIGHT_FRAC_BITS);
    let mut layout = Layout { cells: last, blocked: &blocked, deposits: Vec::new(), vents: Vec::new(), scenery: Vec::new() };
    // Each base has four deposits.
    for &site in &sites {
        for offset in [(-6, -4), (5, -6), (-4, 7), (7, 5)] {
            layout.deposit_four((site.0 + offset.0, site.1 + offset.1));
        }
    }
    // The necks, the narrow ground at the middle of each side, halfway from the map's edge to
    // the shore: four deposits on each of the two between the teams, north and south of the
    // lake, and two on each of the two between teammates.
    let neck = (middle - radius - SHORE) / 2;
    layout.deposit_four((middle - scale(8).max(5), neck));
    layout.deposit_four((middle - scale(22).max(12), neck + 2));
    layout.deposit_four((neck, middle - scale(8).max(5)));
    // The islands: four on the middle one, and one on each of the others.
    layout.deposit_four((middle - 3, middle - 4));
    layout.deposit_four((middle - islet - 1, middle - islet));
    // A vent a short walk from every base.
    for &site in &sites {
        let _ = (0..6).find(|step| layout.vent_four((site.0 + 14 + step * 2, site.1 - 14)));
    }
    // Expansions and more vents out on the ring, more or fewer by the map's resources and size.
    let (wanted_deposits, wanted_vents) = match settings.resources.as_str() {
        "sparse" => (3, 0),
        "rich" => (10, 2),
        _ => (6, 1),
    };
    let by_area = |count: i32| if count == 0 { 0 } else { (count * last * last / (512 * 512)).max(1) };
    let start_cells: Vec<(i32, i32)> = sites.iter().flat_map(|&site| layout.images(site)).collect();
    let near_start = |cell: (i32, i32), cells: i32| start_cells.iter().any(|&start| chebyshev(cell, start) <= cells);
    let on_ring = |cell: (i32, i32)| from_middle(cell) > radius + sway + SHORE + 6;
    let somewhere = |rng: &mut SimRng| (12 + rng.below((middle - 12) as u32) as i32, 12 + rng.below((middle - 12) as u32) as i32);
    let mut expansions = 0;
    for _ in 0..4_000 {
        if expansions == by_area(wanted_deposits) {
            break;
        }
        let cell = somewhere(&mut draft.rng);
        if !on_ring(cell) || near_start(cell, 22) || layout.near_deposit(cell, 14) {
            continue;
        }
        if layout.deposit_four(cell) {
            expansions += 1;
            if draft.rng.below(2) == 0 {
                layout.deposit_four((cell.0 + 2, cell.1 + 1));
            }
        }
    }
    let mut vents = 0;
    for _ in 0..2_000 {
        if vents == by_area(wanted_vents) {
            break;
        }
        let cell = somewhere(&mut draft.rng);
        if on_ring(cell) && !near_start(cell, 24) && layout.vent_four(cell) {
            vents += 1;
        }
    }
    // Rock fields and woods on the ring.
    let amount = match settings.reclaim.as_str() {
        "none" => 0,
        "lots" => 2,
        _ => 1,
    };
    let map = draft.map_units();
    // Clear of the starts: by less on a small map, where the usual left it no room at all.
    let away = |cell: (i32, i32)| near_start(cell, scale(26).clamp(14, 26));
    for (kind, groups, per_group, spread) in [(WreckKind::Rock, by_area(8) * amount, 14, 120), (WreckKind::Tree, by_area(10) * amount, 16, 110)] {
        let mut made = 0;
        for _ in 0..groups * 20 {
            if made == groups {
                break;
            }
            let cell = somewhere(&mut draft.rng);
            if on_ring(cell) {
                layout.clump_four(&mut draft.rng, kind, cell_point(cell), per_group, spread, &away);
                made += 1;
            }
        }
    }
    // Starts stand in the middle of a cell, as on a Passes map. Two each side: the first team's
    // in the west, the second's in the east. A 1v1 uses two opposite corners of them.
    let full = fp_int(map);
    let low = cell_point(start);
    let (west_a, west_b) = (Vec2Fp::new(low.x, full - low.y), low);
    let (east_a, east_b) = (Vec2Fp::new(full - low.x, full - low.y), Vec2Fp::new(full - low.x, low.y));
    let starts = [west_a, east_b];
    let mut layouts = Vec::new();
    if settings.players >= 4 {
        layouts.push(vec![west_a, west_b, east_a, east_b]);
    }
    if settings.players > 4 {
        let low = cell_point(between);
        let (west_c, west_d) = (Vec2Fp::new(low.x, full - low.y), low);
        let (east_c, east_d) = (Vec2Fp::new(full - low.x, full - low.y), Vec2Fp::new(full - low.x, low.y));
        layouts.push(vec![west_a, west_c, west_d, west_b, east_a, east_c, east_d, east_b]);
    }
    let (deposits, vents, scenery) = (layout.deposits, layout.vents, layout.scenery);
    MapData { name: settings.id.clone(), map_units: map, heights: draft.heights, blocked, water_level: fp_int(WATER_UNITS), deposits, vents, starts, layouts, scenery }
}

/// An Islands map (GAME_SPECIFICATION.md §17.3) from its settings: each team on a large home
/// island at one end of the map, west or east, long from north to south with rounded ends, and
/// a sea between them dotted with small islands, one of them in the middle. There is no land
/// route between the home islands: ships, aircraft, hovercraft, and seabed walkers cross. Ships
/// sail all the way round a home island on any but the smallest maps. Home islands hold most
/// deposits, and the small islands the contested ones. It is its own image in both mirrors. Each
/// home island has two start sites either side of its middle, and on a map for eight players
/// two more toward its ends: a 2v2 or a 4v4 fills each team's island, and a 1v1 uses the two
/// furthest apart, at opposite ends of the two islands.
pub fn islands(settings: &MapSettings) -> MapData {
    const WATER_UNITS: i32 = 140;
    /// How far a home island's shore takes to go down to the sea floor, in cells, whatever the
    /// map's size: any shorter and it would be too steep to walk down.
    const SHORE: i32 = 26;
    /// Shores are worked out in sixteenths of a cell, as a Central Lake's is.
    const FINE: i32 = 16;
    assert_eq!(settings.kind, "islands", "{} is not an Islands map", settings.id);
    let cells = (settings.size_km as i32 * 1024 / TERRAIN_CELL_UNITS) as usize;
    let mut draft = Draft::new(cells, settings.seed, 300);
    let (broad, middling, fine) = relief(&settings.terrain);
    draft.hills(80, broad);
    draft.hills(24, middling);
    draft.hills(8, fine);
    let last = cells as i32;
    let middle = last / 2;
    // Sizes are for 20 km, and scale with the map.
    let scale = |value: i32| value * last / 1024;
    // A home island: its axis runs north to south, 18.5% of the way in from its end of the map;
    // at the water's edge it is a quarter of the map wide and 82% of it long, about a fifth of
    // the map's area. Its level ground stops short of the water's edge by the beach, and on a
    // small map, where that would leave too little, it is as wide as a base needs.
    const BEACH: i32 = 20;
    let axis = last * 185 / 1000;
    let half_width = (last * 125 / 1000 - BEACH).max(24);
    let half_length = (last * 41 / 100 - BEACH).max(half_width + 8);
    // How far from the middle row its sides run before its end rounds off.
    let straight = half_length - half_width;
    // Its two long shores, the one facing the map's edge and the one facing the sea between,
    // wander in and out, smoothly and each its own way, by how far along from the middle row a
    // sample is: the same north and south of it, and on both islands, so that the map stays
    // fair. Both are back at the island's plain width where its end rounds off.
    let sway = scale(16).max(2);
    let (steps, spacing) = (4, (straight / 4).max(4));
    let shore_line = |rng: &mut SimRng| {
        let mut points: Vec<i32> = (0..steps + 2).map(|_| rng.below((sway * FINE) as u32 * 2 + 1) as i32 - sway * FINE).collect();
        points[steps as usize] = 0;
        points[steps as usize + 1] = 0;
        // No further in or out from one point to the next than two fifths of the way between.
        gentle(&mut points, spacing * FINE * 2 / 5, &[steps as usize, steps as usize + 1]);
        eased(&points, steps * spacing + 1, spacing)
    };
    let (outer, inner) = (shore_line(&mut draft.rng), shore_line(&mut draft.rng));
    // How far outside a home island's shore a sample is, in sixteenths of a cell: less than
    // nothing on the island.
    let off_home = |x: i32, y: i32| {
        // From the island's axis toward the sea between the islands, or, below nothing, toward
        // its end of the map.
        let seaward = (middle - axis) - (x - middle).abs();
        let along = (y - middle).abs();
        let past = (along - straight).max(0);
        let line = if seaward >= 0 { &inner } else { &outer };
        isqrt((seaward * seaward + past * past) * FINE * FINE) - half_width * FINE - line[along.min(steps * spacing) as usize]
    };
    let floor = (WATER_UNITS - SEA_DEPTH_UNITS) << HEIGHT_FRAC_BITS;
    for y in 0..=last {
        for x in 0..=last {
            let out = off_home(x, y);
            let land = if out <= 0 { 256 } else if out < SHORE * FINE { 256 * (SHORE * FINE - out) / (SHORE * FINE) } else { 0 };
            let height = &mut draft.heights[y as usize * draft.samples + x as usize];
            *height = ((i32::from(*height) * land + floor * (256 - land)) / 256) as i16;
        }
    }
    // The small islands, low and flat, with shores gentle enough for hovercraft to climb: the
    // largest in the middle of the map, and the rest scattered over the sea between the home
    // islands, more of them with less water, each with its images in both mirrors.
    let (island_rise, island_shore) = (80, 8);
    // East of this, on the west half, the sea floor is flat.
    let sea_from = axis + half_width + sway + SHORE;
    let center_size = scale(34).max(10);
    let center_island = middle - center_size - island_shore > sea_from;
    if center_island {
        draft.disc((middle, middle), center_size, island_shore, island_rise);
    }
    let apart = |a: (i32, i32), b: (i32, i32)| isqrt((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2));
    let wanted_isles = (if settings.water == "lots" { 4 } else { 7 } * last / 1024).max(1) as usize;
    let mut isles: Vec<((i32, i32), i32)> = Vec::new();
    for _ in 0..2_000 {
        if isles.len() == wanted_isles {
            break;
        }
        let size = scale(14 + draft.rng.below(13) as i32).max(6);
        let reach = size + island_shore + 2;
        let (west, east, north, south) = (sea_from + reach, middle - reach - 2, reach + 4, middle - reach - 2);
        if west >= east || north >= south {
            continue;
        }
        let at = (west + draft.rng.below((east - west) as u32) as i32, north + draft.rng.below((south - north) as u32) as i32);
        let room = 2 * island_shore + scale(20);
        if apart(at, (middle, middle)) < size + center_size + room || isles.iter().any(|&(other, other_size)| apart(at, other) < size + other_size + room) {
            continue;
        }
        draft.disc_four(at, size, island_shore, island_rise);
        isles.push((at, size));
    }
    // The start sites, on each island's axis: one either side of its middle, and on a map for
    // eight players another toward each end.
    let inner_site = (axis, middle - half_length * 30 / 100);
    let outer_site = (axis, middle - half_length * 72 / 100);
    let sites: Vec<(i32, i32)> = if settings.players > 4 { vec![inner_site, outer_site] } else { vec![inner_site] };
    // Outcrops on the home islands, well in from their shores, clear of the start sites, and
    // never on the strip down an island's axis, which joins its sites whatever else is there.
    let clear = scale(88).max(30);
    let outcrops = match settings.terrain.as_str() {
        "flat" => 0,
        "rugged" => scale(24),
        _ => scale(12),
    };
    let mut placed = 0;
    for _ in 0..600 {
        if placed == outcrops {
            break;
        }
        let cell = (axis - half_width + draft.rng.below(half_width as u32 * 2) as i32, middle - half_length + draft.rng.below(half_length as u32) as i32);
        if off_home(cell.0, cell.1) > -14 * FINE || (cell.0 - axis).abs() < scale(30).max(12) || sites.iter().any(|&site| chebyshev(cell, site) < clear + 12) {
            continue;
        }
        let (size, height) = (3 + draft.rng.below(6) as i32, 30 + draft.rng.below(40) as i32);
        draft.disc_four(cell, size, 2, height);
        placed += 1;
    }
    for &site in &sites {
        for image in draft.images(site) {
            draft.flatten(image, scale(60).max(22));
        }
    }
    draft.symmetrize_four();

    let blocked = draft.blocked(WATER_UNITS << HEIGHT_FRAC_BITS);
    let mut layout = Layout { cells: last, blocked: &blocked, deposits: Vec::new(), vents: Vec::new(), scenery: Vec::new() };
    // Each start site has four deposits, and a vent a short walk away.
    for &site in &sites {
        for offset in [(-6, -4), (5, -6), (-4, 7), (7, 5)] {
            layout.deposit_four((site.0 + offset.0, site.1 + offset.1));
        }
        let _ = (0..6).find(|step| layout.vent_four((site.0 + 14 + step * 2, site.1 - 14)));
    }
    // The small islands hold the contested deposits: four on the middle one, and one on each of
    // the others, or two on the larger ones of a rich map.
    if center_island {
        layout.deposit_four((middle - 3, middle - 4));
    }
    for &(at, size) in &isles {
        layout.deposit_four((at.0 - 1, at.1 - 1));
        if settings.resources == "rich" && size >= scale(20).max(9) {
            layout.deposit_four((at.0 + 2, at.1 + 1));
        }
    }
    // Expansions and more vents on the home islands, more or fewer by the map's resources and
    // by its length, as the small islands are, so that the home islands always hold more
    // deposits than the small ones do.
    let (wanted_deposits, wanted_vents) = match settings.resources.as_str() {
        "sparse" => (4, 0),
        "rich" => (12, 2),
        _ => (8, 1),
    };
    let by_length = |count: i32| if count == 0 { 0 } else { (count * last / 1024).max(1) };
    let by_area = |count: i32| if count == 0 { 0 } else { (count * last / 1024 * last / 1024).max(1) };
    let site_cells: Vec<(i32, i32)> = sites.iter().flat_map(|&site| layout.images(site)).collect();
    let near_site = |cell: (i32, i32), cells: i32| site_cells.iter().any(|&site| chebyshev(cell, site) <= cells);
    // A cell on the west island's north half, somewhere.
    let somewhere = |rng: &mut SimRng| (axis - half_width + rng.below(half_width as u32 * 2) as i32, middle - half_length + rng.below(half_length as u32) as i32);
    let ashore = |cell: (i32, i32)| off_home(cell.0, cell.1) < -8 * FINE;
    let mut expansions = 0;
    for _ in 0..4_000 {
        if expansions == by_length(wanted_deposits) {
            break;
        }
        let cell = somewhere(&mut draft.rng);
        if !ashore(cell) || near_site(cell, 22) || layout.near_deposit(cell, 14) {
            continue;
        }
        if layout.deposit_four(cell) {
            expansions += 1;
            if draft.rng.below(2) == 0 {
                layout.deposit_four((cell.0 + 2, cell.1 + 1));
            }
        }
    }
    let mut vents = 0;
    for _ in 0..2_000 {
        if vents == by_length(wanted_vents) {
            break;
        }
        let cell = somewhere(&mut draft.rng);
        if ashore(cell) && !near_site(cell, 24) && layout.vent_four(cell) {
            vents += 1;
        }
    }
    // Rock fields and woods on the home islands.
    let amount = match settings.reclaim.as_str() {
        "none" => 0,
        "lots" => 2,
        _ => 1,
    };
    let map = draft.map_units();
    let away = |cell: (i32, i32)| near_site(cell, 26);
    for (kind, groups, per_group, spread) in [(WreckKind::Rock, by_area(12) * amount, 14, 120), (WreckKind::Tree, by_area(20) * amount, 16, 110)] {
        let mut made = 0;
        for _ in 0..groups * 20 {
            if made == groups {
                break;
            }
            let cell = somewhere(&mut draft.rng);
            if ashore(cell) {
                layout.clump_four(&mut draft.rng, kind, cell_point(cell), per_group, spread, &away);
                made += 1;
            }
        }
    }
    // Starts stand in the middle of a cell, as on the other kinds. A site's four images: on the
    // west island or the east, and on its north half or its south.
    let full = fp_int(map);
    let at = |site: (i32, i32), east: bool, south: bool| {
        let low = cell_point(site);
        Vec2Fp::new(if east { full - low.x } else { low.x }, if south { low.y } else { full - low.y })
    };
    // The first team's in the west, the second's in the east, each island's from north to south.
    let mut layouts = Vec::new();
    if settings.players >= 4 {
        layouts.push(vec![at(inner_site, false, false), at(inner_site, false, true), at(inner_site, true, false), at(inner_site, true, true)]);
    }
    if settings.players > 4 {
        let island = |east: bool| [at(outer_site, east, false), at(inner_site, east, false), at(inner_site, east, true), at(outer_site, east, true)];
        layouts.push(island(false).into_iter().chain(island(true)).collect());
    }
    // A 1v1 uses the two sites furthest apart: one at the far end of each island.
    let far = *sites.last().expect("a map has a start site");
    let starts = [at(far, false, false), at(far, true, true)];
    let (deposits, vents, scenery) = (layout.deposits, layout.vents, layout.scenery);
    MapData { name: settings.id.clone(), map_units: map, heights: draft.heights, blocked, water_level: fp_int(WATER_UNITS), deposits, vents, starts, layouts, scenery }
}

/// A map turned a quarter turn, north to west: a point (x, y) goes to (width - y, x).
fn quarter_turn(map: MapData) -> MapData {
    let cells = (map.map_units / TERRAIN_CELL_UNITS) as usize;
    let samples = cells + 1;
    let full = fp_int(map.map_units);
    let point = |at: Vec2Fp| Vec2Fp::new(full - at.y, at.x);
    let cell = |&(x, y): &(u32, u32)| (cells as u32 - 1 - y, x);
    let mut heights = vec![0; map.heights.len()];
    for y in 0..samples {
        for x in 0..samples {
            heights[x * samples + (cells - y)] = map.heights[y * samples + x];
        }
    }
    let mut blocked = vec![false; map.blocked.len()];
    for y in 0..cells {
        for x in 0..cells {
            blocked[x * cells + (cells - 1 - y)] = map.blocked[y * cells + x];
        }
    }
    MapData {
        heights,
        blocked,
        deposits: map.deposits.iter().map(cell).collect(),
        vents: map.vents.iter().map(cell).collect(),
        starts: map.starts.map(point),
        layouts: map.layouts.iter().map(|layout| layout.iter().copied().map(point).collect()).collect(),
        scenery: map.scenery.iter().map(|&(kind, at)| (kind, point(at))).collect(),
        ..map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Every cell that can be reached from one, over cells that are `open`.
    fn reached(cells: usize, open: impl Fn(usize) -> bool, from: (usize, usize)) -> Vec<bool> {
        let mut seen = vec![false; cells * cells];
        let mut queue = VecDeque::from([from]);
        seen[from.1 * cells + from.0] = true;
        while let Some((x, y)) = queue.pop_front() {
            for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
                if nx < cells && ny < cells && !seen[ny * cells + nx] && open(ny * cells + nx) {
                    seen[ny * cells + nx] = true;
                    queue.push_back((nx, ny));
                }
            }
        }
        seen
    }

    /// Every cell land units can walk to from one, around blocked ground.
    fn walked(map: &MapData, from: (usize, usize)) -> Vec<bool> {
        reached((map.map_units / TERRAIN_CELL_UNITS) as usize, |cell| !map.blocked[cell], from)
    }

    /// Whether land units can walk from one cell to another, around blocked ground.
    fn connected(map: &MapData, from: (usize, usize), to: (usize, usize)) -> bool {
        walked(map, from)[to.1 * (map.map_units / TERRAIN_CELL_UNITS) as usize + to.0]
    }

    fn cell_of(map: &MapData, point: Vec2Fp) -> (usize, usize) {
        let last = (map.map_units / TERRAIN_CELL_UNITS - 1) as usize;
        let cell = |value: Fp| ((value.to_num::<i32>() / TERRAIN_CELL_UNITS) as usize).min(last);
        (cell(point.x), cell(point.y))
    }

    /// A map whose 1v1 starts are joined by land, as on every kind but Islands.
    fn check(map: &MapData) {
        check_turned(map);
        assert!(connected(map, cell_of(map, map.starts[0]), cell_of(map, map.starts[1])), "the starts must be joined by land");
    }

    /// What every map that comes with the game is: fair, and with enough to fight over.
    fn check_turned(map: &MapData) {
        check_image(map);
        assert!(map.vents.len() >= 6, "{} vents", map.vents.len());
        assert!(map.deposits.len() >= 30, "{} deposits", map.deposits.len());
    }

    /// What every map is, generated ones too: its own image turned about its center, with its
    /// deposits and vents on open ground and its 1v1 starts each other's image.
    fn check_image(map: &MapData) {
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
        // Vents mirror too, and a plant fits over each, clear of deposits.
        for &(x, y) in &map.vents {
            assert!(map.vents.contains(&(cells as u32 - 1 - x, cells as u32 - 1 - y)), "vent {x}, {y} has no partner");
            for (cx, cy) in (y - 1..=y + 1).flat_map(|cy| (x - 1..=x + 1).map(move |cx| (cx, cy))) {
                assert!(!map.blocked[cy as usize * cells + cx as usize] && !map.deposits.contains(&(cx, cy)), "a plant on vent {x}, {y} would not fit");
            }
        }
        let full = fp_int(map.map_units);
        assert_eq!(map.starts[1], Vec2Fp::new(full - map.starts[0].x, full - map.starts[0].y), "the starts mirror");
        // Each start is open.
        let (blue, red) = (cell_of(map, map.starts[0]), cell_of(map, map.starts[1]));
        assert!(!map.blocked[blue.1 * cells + blue.0] && !map.blocked[red.1 * cells + red.0]);
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

    /// Deposits and vents within reach of a start, by kind.
    fn near(map: &MapData, start: Vec2Fp, reach: i32) -> (usize, usize) {
        let within = |&&(x, y): &&(u32, u32)| {
            let at = cell_point((x as i32, y as i32));
            let (dx, dy) = ((at.x - start.x).to_num::<i64>(), (at.y - start.y).to_num::<i64>());
            dx * dx + dy * dy <= i64::from(reach).pow(2)
        };
        (map.deposits.iter().filter(within).count(), map.vents.iter().filter(within).count())
    }

    /// A map for four players or more is its own image in both mirrors, across the middle column
    /// and the middle row: its ground, what blocks it, and its deposits and vents. It has four
    /// team starts, two a side, the first team's in the west, each the others' image, and each
    /// start in a set has the same deposits and vents within reach. Where it has starts for more
    /// players, each team's have the same shares between them as the other's.
    fn check_four(map: &MapData) {
        let cells = (map.map_units / TERRAIN_CELL_UNITS) as usize;
        let (samples, last) = (cells + 1, cells);
        for y in 0..samples {
            for x in 0..samples {
                let height = map.heights[y * samples + x];
                assert_eq!(height, map.heights[y * samples + (last - x)], "heights mirror across the middle column");
                assert_eq!(height, map.heights[(last - y) * samples + x], "heights mirror across the middle row");
            }
        }
        for y in 0..cells {
            for x in 0..cells {
                assert_eq!(map.blocked[y * cells + x], map.blocked[y * cells + (cells - 1 - x)], "blocked cells mirror");
            }
        }
        for &(x, y) in map.deposits.iter().chain(&map.vents) {
            let mirror = (cells as u32 - 1 - x, y);
            assert!(map.deposits.contains(&mirror) || map.vents.contains(&mirror), "{x}, {y} has no mirror image");
        }
        let team = map.layouts.iter().find(|layout| layout.len() == 4).expect("starts for four players");
        let full = fp_int(map.map_units);
        assert_eq!(team[1], Vec2Fp::new(team[0].x, full - team[0].y));
        assert_eq!(team[2], Vec2Fp::new(full - team[0].x, team[0].y));
        assert_eq!(team[3], Vec2Fp::new(full - team[0].x, full - team[0].y));
        assert!(team[0].x < full / 2 && team[2].x > full / 2, "the first team is in the west, the second in the east");
        assert!(map.starts[0].x < full / 2 && map.starts[1].x > full / 2, "the 1v1 starts face each other west and east");
        for set in [map.starts.to_vec(), team.clone()] {
            let first = near(map, set[0], 1_500);
            assert!(first.0 >= 4, "{} deposits near a start", first.0);
            for &start in &set {
                assert_eq!(near(map, start, 1_500), first, "a start has a different share");
            }
        }
        for layout in &map.layouts {
            let shares = |team: &[Vec2Fp]| {
                let mut shares: Vec<(usize, usize)> = team.iter().map(|&start| near(map, start, 1_500)).collect();
                shares.sort_unstable();
                shares
            };
            let (first, second) = layout.split_at(layout.len() / 2);
            assert!(first.iter().all(|start| start.x < full / 2) && second.iter().all(|start| start.x > full / 2), "the first team is in the west, the second in the east");
            assert_eq!(shares(first), shares(second), "the teams of {} have different shares", layout.len() / 2);
        }
    }

    #[test]
    fn saddleback_is_fair_for_one_on_one_two_on_two_and_three_on_three() {
        let map = saddleback();
        check(&map);
        check_four(&map);
        assert_eq!(map, saddleback(), "the same every time");
        let cells = (map.map_units / TERRAIN_CELL_UNITS) as usize;
        let samples = cells + 1;
        // A 3v3 plays the 1v1's starts and the 2v2's, three a side, each of the second team's
        // the first's image turned about the center.
        assert_eq!(map.layouts.iter().map(Vec::len).collect::<Vec<_>>(), [4, 6]);
        let three = &map.layouts[1];
        let full = fp_int(map.map_units);
        for (index, &start) in three[..3].iter().enumerate() {
            assert_eq!(three[index + 3], Vec2Fp::new(full - start.x, full - start.y), "the second team's start {index} is the first's turned");
            assert!(map.starts.contains(&start) || map.layouts[0].contains(&start), "a 3v3 start {start:?} is a 1v1 or 2v2 start");
        }
        // Holding six changes nothing else: the 1v1 and 2v2 play the map they did for four.
        let four = passes(&MapSettings { players: 4, ..settings("saddleback").expect("data/maps/saddleback.toml") });
        assert_eq!(MapData { layouts: four.layouts.clone(), ..map.clone() }, four, "six players change only the layouts");
        assert_eq!(four.layouts, map.layouts[..1]);
        // Every start reaches every other.
        let every: Vec<Vec2Fp> = map.starts.iter().chain(map.layouts.iter().flatten()).copied().collect();
        for &from in &every {
            for &to in &every {
                assert!(connected(&map, cell_of(&map, from), cell_of(&map, to)), "{from:?} cannot reach {to:?}");
            }
        }
        // The range runs up the middle column, crossed only through the valleys: three gaps in
        // it, the center one widest.
        let middle = cells / 2;
        let gaps_in = |column: usize| {
            let mut gaps = Vec::new();
            let mut run = 0;
            for y in 0..cells {
                if map.blocked[y * cells + column] {
                    if run > 0 {
                        gaps.push(run);
                    }
                    run = 0;
                } else {
                    run += 1;
                }
            }
            if run > 0 {
                gaps.push(run);
            }
            gaps
        };
        let gaps = gaps_in(middle);
        assert_eq!(gaps.len(), 3, "valleys across the middle column: {gaps:?}");
        assert!(gaps[1] > gaps[0] && gaps[1] > gaps[2], "the center valley is the widest: {gaps:?}");
        // A gun at one end of the center valley fires clear over its saddle to the other: the
        // straight line between points 12 units up at either end stays above the floor.
        let height = |x: usize| f64::from(map.heights[middle * samples + x]) / 16.0;
        let (from, to) = (middle + 40, middle - 40);
        let (start, end) = (height(from) + 12.0, height(to) + 12.0);
        for x in to..=from {
            let along = (from - x) as f64 / (from - to) as f64;
            let line = start + (end - start) * along;
            assert!(height(x) < line, "the saddle at column {x}, {:.1} high, hides one end of the valley from the other ({line:.1})", height(x));
        }
        // Each valley is an hourglass: narrower at the middle column than out toward its mouths.
        let toward_mouth = gaps_in(middle - 20);
        assert_eq!(toward_mouth.len(), 3, "the valleys toward their mouths: {toward_mouth:?}");
        for (choke, wider) in gaps.iter().zip(&toward_mouth) {
            assert!(wider > choke, "a valley {choke} cells wide at its choke and {wider} toward its mouth");
        }
        // The valleys hold more deposits than their share of the ground.
        let in_range = |&&(x, _): &&(u32, u32)| (x as usize).abs_diff(middle) < 40;
        assert!(map.deposits.iter().filter(in_range).count() >= 12, "deposits in the valleys");
    }

    /// Cells any of which is under the water.
    fn wet(map: &MapData) -> Vec<bool> {
        let cells = (map.map_units / TERRAIN_CELL_UNITS) as usize;
        let water = map.water_level.to_num::<i32>() << HEIGHT_FRAC_BITS;
        (0..cells * cells)
            .map(|cell| {
                let (x, y) = (cell % cells, cell / cells);
                [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].iter().any(|&(cx, cy)| i32::from(map.heights[cy * (cells + 1) + cx]) < water)
            })
            .collect()
    }

    /// What a Central Lake map is, whatever its settings: a lake in the middle that ships sail,
    /// land all the way round it, by which every start reaches every other, and islands in the
    /// lake with deposits no land route reaches. Returns the share of the map under water, in
    /// hundredths, and the narrowest the land is at the middle of a side, in cells.
    fn check_lake(map: &MapData) -> (usize, usize) {
        use crate::sim::components::MoveClass;
        let cells = (map.map_units / TERRAIN_CELL_UNITS) as usize;
        let middle = cells / 2;
        let every: Vec<Vec2Fp> = map.starts.iter().chain(map.layouts.iter().flatten()).copied().collect();
        let ring = walked(map, cell_of(map, map.starts[0]));
        let on_ring = |(x, y): (usize, usize)| ring[y * cells + x];
        for &start in &every {
            assert!(on_ring(cell_of(map, start)), "{start:?} cannot be reached from the first start by land");
        }
        // The islands: deposits on open ground that no start reaches by land.
        let on_islands: Vec<(u32, u32)> = map.deposits.iter().copied().filter(|&(x, y)| !on_ring((x as usize, y as usize))).collect();
        assert!(on_islands.len() >= 8, "{} deposits on islands in the lake", on_islands.len());
        assert!(on_islands.len() * 4 <= map.deposits.len(), "most deposits are on the ring: {} of {} are not", on_islands.len(), map.deposits.len());
        // Ships sail the lake: from beside the middle island out toward every side.
        let terrain = crate::sim::terrain::Terrain::from_heights(map.map_units, map.heights.clone(), &map.blocked, map.water_level);
        let water = wet(map);
        let sails = |x: usize, y: usize| terrain.passable_for(MoveClass::Naval, cell_point((x as i32, y as i32)));
        let reach = (0..middle).take_while(|&step| water[middle * cells + middle + step] || !map.blocked[middle * cells + middle + step]).filter(|&step| water[middle * cells + middle + step]).count();
        assert!(reach * 5 >= cells, "the lake reaches {reach} cells east of the middle of {cells}");
        for (x, y) in [(middle + reach / 2, middle), (middle - reach / 2, middle), (middle, middle + reach / 2), (middle, middle - reach / 2)] {
            assert!(sails(x, y), "a ship can sail at {x}, {y}");
        }
        // Hovercraft and seabed walkers reach the islands from the shore.
        for &(x, y) in &on_islands {
            assert!(terrain.passable_for(MoveClass::Hover, cell_point((x as i32, y as i32))), "a hovercraft can stand on the island at {x}, {y}");
        }
        // The land is narrowest at the middle of each side: the necks, wide enough to fight over.
        let neck = (0..middle).take_while(|&y| !map.blocked[y * cells + middle]).count();
        let share = water.iter().filter(|&&wet| wet).count() * 100 / water.len();
        (share, neck)
    }

    #[test]
    fn stillwater_is_a_lake_ringed_by_land_fair_for_one_on_one_and_two_on_two() {
        let map = stillwater();
        check(&map);
        check_four(&map);
        assert_eq!(map, stillwater(), "the same every time");
        assert_eq!(bundled("stillwater").as_ref(), Some(&map));
        let (share, neck) = check_lake(&map);
        assert!((30..=45).contains(&share), "the lake is {share}% of the map, about two fifths");
        assert!((40..=90).contains(&neck), "the land north of the lake is {neck} cells wide");
        // A 1v1 uses two opposite corners of the four team starts.
        let team = &map.layouts[0];
        assert_eq!(map.starts, [team[0], team[3]]);
        // The richest ground on land is between the teams, north and south of the lake.
        let cells = (map.map_units / TERRAIN_CELL_UNITS) as usize;
        let between = |&&(x, y): &&(u32, u32)| (x as usize).abs_diff(cells / 2) < 40 && ((y as usize) < neck || (y as usize) >= cells - neck);
        assert!(map.deposits.iter().filter(between).count() >= 8, "deposits on the necks between the teams");
    }

    /// The generator makes a fair Central Lake from any settings the game offers: every size,
    /// amount of water, kind of terrain, and a spread of seeds.
    #[test]
    fn every_central_lake_is_fair_and_joined_round_its_shore() {
        for (size_km, players) in [(5, 2), (10, 4), (20, 4)] {
            for (index, (water, terrain, resources)) in [("some", "flat", "sparse"), ("lots", "hilly", "normal"), ("some", "rugged", "rich"), ("lots", "rugged", "normal")].into_iter().enumerate() {
                // The largest maps take longest to make, so fewer of them are tried.
                if size_km == 20 && index % 2 == 0 {
                    continue;
                }
                let settings = MapSettings {
                    id: "generated".into(),
                    name: "Generated".into(),
                    kind: "central_lake".into(),
                    size_km,
                    players,
                    seed: 7 + index as u64 * 1_001 + u64::from(size_km),
                    resources: resources.into(),
                    reclaim: "some".into(),
                    terrain: terrain.into(),
                    valleys: 0,
                    water: water.into(),
                };
                let map = generate(&settings);
                let label = format!("{size_km} km, {water} water, {terrain}, seed {}", settings.seed);
                assert_eq!(map.map_units, size_km as i32 * 1024, "{label}");
                assert_eq!(map.layouts.len(), usize::from(players >= 4), "{label}");
                let (share, neck) = check_lake(&map);
                assert!((12..=45).contains(&share), "{label}: the lake is {share}% of the map");
                assert!(neck >= 20, "{label}: the land north of the lake is {neck} cells wide");
                let full = fp_int(map.map_units);
                assert_eq!(map.starts[1], Vec2Fp::new(full - map.starts[0].x, full - map.starts[0].y), "{label}: the starts mirror");
                assert_eq!(near(&map, map.starts[0], 1_500), near(&map, map.starts[1], 1_500), "{label}: the starts have the same share");
                assert!(near(&map, map.starts[0], 1_500).0 >= 4 && near(&map, map.starts[0], 1_500).1 >= 1, "{label}: a start has deposits and a vent");
                if players >= 4 {
                    check_four(&map);
                }
            }
        }
    }

    /// What an Islands map is, whatever its settings: a home island at each end, with every
    /// start of a team on its own and no land route to the other; a sea between that ships
    /// cross, and hovercraft and seabed walkers too; and small islands in it with deposits no
    /// home island reaches by land. Returns the share of the map that is land, in hundredths,
    /// how many deposits the small islands hold, and whether ships can sail behind a home island.
    fn check_isles(map: &MapData) -> (usize, usize, bool) {
        use crate::sim::components::MoveClass;
        let cells = (map.map_units / TERRAIN_CELL_UNITS) as usize;
        let full = fp_int(map.map_units);
        let every: Vec<Vec2Fp> = map.starts.iter().chain(map.layouts.iter().flatten()).copied().collect();
        let (west, east) = (walked(map, cell_of(map, map.starts[0])), walked(map, cell_of(map, map.starts[1])));
        let at = |(x, y): (usize, usize)| y * cells + x;
        assert!(map.starts[0].x < full / 2 && map.starts[1].x > full / 2, "Blue starts in the west and Red in the east");
        for &start in &every {
            let (home, away) = if start.x < full / 2 { (&west, &east) } else { (&east, &west) };
            assert!(home[at(cell_of(map, start))], "{start:?} is on its team's home island");
            assert!(!away[at(cell_of(map, start))], "{start:?} has no land route to the other island");
        }
        let terrain = crate::sim::terrain::Terrain::from_heights(map.map_units, map.heights.clone(), &map.blocked, map.water_level);
        let open_to = |class: MoveClass, cell: usize| terrain.passable_for(class, cell_point(((cell % cells) as i32, (cell / cells) as i32)));
        // Ships: from the first water deep enough for them east of Blue's start, to the first
        // west of Red's, and from the same row at the map's west edge, behind Blue's island.
        let sails = |cell: usize| open_to(MoveClass::Naval, cell);
        let (blue, red) = (cell_of(map, map.starts[0]), cell_of(map, map.starts[1]));
        let blue_sea = (blue.0..cells).find(|&x| sails(at((x, blue.1)))).expect("sea east of Blue's island");
        let red_sea = (0..red.0).rev().find(|&x| sails(at((x, red.1)))).expect("sea west of Red's island");
        let sailed = reached(cells, sails, (blue_sea, blue.1));
        assert!(sailed[at((red_sea, red.1))], "ships cross from one home island to the other");
        let behind = (0..blue.0).any(|x| sailed[at((x, blue.1))]);
        // Hovercraft and seabed walkers go from start to start.
        assert!(reached(cells, |cell| open_to(MoveClass::Hover, cell), blue)[at(red)], "hovercraft cross from start to start");
        // The small islands' deposits: on open ground no home island reaches by land.
        let on_isles = map.deposits.iter().filter(|&&(x, y)| !west[at((x as usize, y as usize))] && !east[at((x as usize, y as usize))]).count();
        assert!(on_isles >= 4, "{on_isles} deposits on the small islands");
        assert!(on_isles * 2 <= map.deposits.len(), "the home islands hold most deposits: {on_isles} of {} are not on them", map.deposits.len());
        let land = wet(map).iter().filter(|&&wet| !wet).count() * 100 / (cells * cells);
        (land, on_isles, behind)
    }

    #[test]
    fn scattered_isles_is_two_home_islands_across_a_sea_of_small_ones() {
        let map = scattered_isles();
        check_turned(&map);
        check_four(&map);
        assert_eq!(map, scattered_isles(), "the same every time");
        assert_eq!(bundled("scattered_isles").as_ref(), Some(&map));
        assert_eq!(map.map_units, 20 * 1024);
        let (land, on_isles, behind) = check_isles(&map);
        assert!((35..=50).contains(&land), "{land}% of the map is land, about a fifth of it each home island");
        assert!(on_isles >= 30, "{on_isles} deposits on the small islands");
        assert!(behind, "ships sail all the way round a home island");
        // Starts for four players and for eight, four to an island: a 1v1 uses the two at
        // opposite far ends, and a 2v2 the two either side of each island's middle.
        assert_eq!(map.layouts.iter().map(Vec::len).collect::<Vec<_>>(), [4, 8]);
        let (four, eight) = (&map.layouts[0], &map.layouts[1]);
        assert_eq!(map.starts, [eight[0], eight[7]]);
        assert_eq!(four[..], [eight[1], eight[2], eight[5], eight[6]]);
        let full = fp_int(map.map_units);
        let from_middle = |start: &Vec2Fp| (start.y - full / 2).abs();
        assert!(eight.iter().all(|start| from_middle(start) <= from_middle(&map.starts[0])), "the 1v1 starts are the furthest from the middle row");
    }

    /// The generator makes a fair Islands map from any settings the game offers.
    #[test]
    fn every_islands_map_is_fair_and_has_no_land_route_between_its_home_islands() {
        for (size_km, players) in [(5, 2), (10, 4), (20, 8)] {
            for (index, (water, terrain, resources)) in [("some", "flat", "sparse"), ("lots", "hilly", "normal"), ("some", "rugged", "rich")].into_iter().enumerate() {
                // The largest maps take longest to make, so fewer of them are tried.
                if size_km == 20 && index != 1 {
                    continue;
                }
                let settings = MapSettings {
                    id: "generated".into(),
                    name: "Generated".into(),
                    kind: "islands".into(),
                    size_km,
                    players,
                    seed: 11 + index as u64 * 1_001 + u64::from(size_km),
                    resources: resources.into(),
                    reclaim: "some".into(),
                    terrain: terrain.into(),
                    valleys: 0,
                    water: water.into(),
                };
                let map = generate(&settings);
                let label = format!("{size_km} km, {water} water, {terrain}, seed {}", settings.seed);
                assert_eq!(map.map_units, size_km as i32 * 1024, "{label}");
                assert_eq!(map.layouts.iter().map(Vec::len).collect::<Vec<_>>(), [4, 8][..(players as usize / 4).min(2)], "{label}");
                let (land, ..) = check_isles(&map);
                assert!((25..=55).contains(&land), "{label}: {land}% of the map is land");
                let full = fp_int(map.map_units);
                assert_eq!(map.starts[1], Vec2Fp::new(full - map.starts[0].x, full - map.starts[0].y), "{label}: the starts mirror");
                assert_eq!(near(&map, map.starts[0], 1_500), near(&map, map.starts[1], 1_500), "{label}: the starts have the same share");
                assert!(near(&map, map.starts[0], 1_500).0 >= 4 && near(&map, map.starts[0], 1_500).1 >= 1, "{label}: a start has deposits and a vent");
                if players >= 4 {
                    check_four(&map);
                }
            }
        }
    }

    /// What a player may choose for a generated map comes to its settings, with the usual for
    /// what is left unsaid, and what the generator cannot make is refused with the reason.
    #[test]
    fn a_generated_maps_settings_are_checked() {
        let choose = |kind: &str| Choices { kind: kind.into(), ..Choices::default() };
        let usual = MapSettings::chosen(&choose("central_lake")).expect("the usual lake");
        assert_eq!((usual.size_km, usual.players, usual.seed), (10, 4, 1));
        assert_eq!([usual.resources.as_str(), usual.reclaim.as_str(), usual.terrain.as_str(), usual.water.as_str()], ["normal", "some", "hilly", "some"]);
        // A Passes map has no water, three valleys, and one where it is smallest.
        let passes = MapSettings::chosen(&choose("passes")).expect("the usual passes");
        assert_eq!((passes.water.as_str(), passes.valleys), ("", 3));
        assert_eq!(MapSettings::chosen(&Choices { size_km: Some(5), ..choose("passes") }).expect("small passes").valleys, 1);
        assert!(MapSettings::chosen(&Choices { water: Some("none".into()), ..choose("passes") }).is_ok());
        assert!(MapSettings::chosen(&Choices { water: Some("lots".into()), ..choose("passes") }).is_err_and(|problem| problem.contains("no water")));
        // As many players as the size holds, up to four unless more are asked for.
        assert_eq!(MapSettings::chosen(&Choices { size_km: Some(5), ..choose("islands") }).expect("small islands").players, 2);
        assert_eq!(MapSettings::chosen(&Choices { size_km: Some(20), ..choose("islands") }).expect("large islands").players, 4);
        assert_eq!(MapSettings::chosen(&Choices { size_km: Some(20), players: Some(8), ..choose("islands") }).expect("for eight").players, 8);
        for (wrong, reason) in [
            (choose("archipelago"), "passes, central_lake, islands"),
            (Choices { size_km: Some(12), ..choose("passes") }, "5, 10, 15, or 20 km"),
            (Choices { size_km: Some(5), players: Some(4), ..choose("islands") }, "a 5 km map holds 2 players"),
            (Choices { players: Some(8), ..choose("central_lake") }, "a 10 km map holds 2 or 4 players"),
            (Choices { size_km: Some(20), players: Some(6), ..choose("islands") }, "2, 4, or 8 players"),
            (Choices { resources: Some("plenty".into()), ..choose("islands") }, "sparse, normal, rich"),
            (Choices { reclaim: Some("much".into()), ..choose("islands") }, "none, some, lots"),
            (Choices { terrain: Some("mountains".into()), ..choose("islands") }, "flat, hilly, rugged"),
            (Choices { water: Some("none".into()), ..choose("central_lake") }, "some, lots"),
        ] {
            let problem = MapSettings::chosen(&wrong).expect_err("refused");
            assert!(problem.contains(reason), "{wrong:?} is refused with: {problem}");
        }
    }

    /// The generator makes a fair map of every kind, at every size, for every count of players
    /// the size holds: its own image, every start with the same share as its opposite number,
    /// starts for each count of players up to the map's, and the way between the sides that its
    /// kind promises.
    #[test]
    fn every_kind_of_map_is_fair_at_every_size_for_every_count_of_players() {
        for kind in MapSettings::KINDS {
            for size_km in MapSettings::SIZES_KM {
                for players in [2, 4, 8].into_iter().filter(|&players| players <= MapSettings::most_players(size_km)) {
                    let choices = Choices { kind: kind.into(), size_km: Some(size_km), players: Some(players), seed: Some(u64::from(size_km * 7 + players)), ..Choices::default() };
                    let settings = MapSettings::chosen(&choices).expect("settings the game offers");
                    let map = generate(&settings);
                    let label = format!("{kind}, {size_km} km, {players} players");
                    assert_eq!(map, generate(&settings), "{label}: the same every time");
                    assert_eq!(map.map_units, size_km as i32 * 1024, "{label}");
                    check_image(&map);
                    assert_eq!(map.layouts.iter().map(Vec::len).collect::<Vec<_>>(), [4, 8][..(players as usize / 4).min(2)], "{label}: starts for each count of players");
                    let share = near(&map, map.starts[0], 1_500);
                    assert_eq!(share, near(&map, map.starts[1], 1_500), "{label}: the 1v1 starts have the same share");
                    assert!(share.0 >= 4 && share.1 >= 1, "{label}: a start has {} deposits and {} vents in reach", share.0, share.1);
                    if players >= 4 {
                        check_four(&map);
                    }
                    match kind {
                        "islands" => drop(check_isles(&map)),
                        "central_lake" => drop(check_lake(&map)),
                        _ => {
                            let walked = walked(&map, cell_of(&map, map.starts[0]));
                            let cells = (map.map_units / TERRAIN_CELL_UNITS) as usize;
                            for &start in map.starts.iter().chain(map.layouts.iter().flatten()) {
                                let (x, y) = cell_of(&map, start);
                                assert!(walked[y * cells + x], "{label}: {start:?} cannot be reached from the first start by land");
                            }
                        }
                    }
                }
            }
        }
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
