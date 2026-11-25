//! Fixed scenarios shared by the tests, benchmarks, and interactive app.

use super::commands::SimCommand;
use super::components::{SimId, Team, WreckKind};
use super::config::{SimConfig, TerrainStyle, MAX_MAP_UNITS};
use super::world::{SimWorld, UnitSpawn};
use crate::math::fp::{fp_int, Fp, Vec2Fp};
use crate::math::rect::RectFp;
use crate::math::rng::SimRng;

pub const SPAWN_SPACING: Fp = Fp::lit("2.5");
pub const BATTLE_GAP_UNITS: i32 = 40;

/// The standard PoC unit.
pub fn standard_unit(team: Team, position: Vec2Fp) -> UnitSpawn {
    UnitSpawn {
        position,
        team,
        // The team's own player, as in a 1v1.
        owner: super::components::PlayerId(team as u8),
        max_speed: fp_int(1),
        radius: fp_int(1),
        health: fp_int(100),
        range: fp_int(30),
        reload_ticks: 28,
        projectile_speed: fp_int(2),
        projectile_damage: fp_int(8),
        projectile_splash: Fp::ZERO,
        is_structure: false,
        vision: Fp::ZERO,
    }
}

/// A world plus the commands to issue on its first tick.
pub struct Scenario {
    pub world: SimWorld,
    pub opening_commands: Vec<SimCommand>,
}

impl Scenario {
    /// The same scenario with fog of war on or off. Matches have it on unless the players turn
    /// it off: with it off, every team, the AI's included, sees the whole map.
    #[must_use]
    pub fn with_fog_of_war(mut self, on: bool) -> Self {
        self.world.config.fog_of_war = on;
        self
    }
}

/// The map center of a scenario, where the action starts.
pub fn map_center(world: &SimWorld) -> Vec2Fp {
    let center = fp_int(world.terrain.map_units() / 2);
    Vec2Fp::new(center, center)
}

/// The number of units along each side of a square block holding `count` units.
fn block_side(count: usize) -> u64 {
    let root = (count as u64).isqrt();
    root + u64::from(root * root < count as u64)
}

/// Spawns a jittered square block of units and returns their ids.
fn spawn_block(world: &mut SimWorld, team: Team, count: usize, center: Vec2Fp, spacing: Fp, template: impl Fn(Team, Vec2Fp) -> UnitSpawn) -> Vec<SimId> {
    let side = block_side(count);
    let half_width = spacing * Fp::from_num(side as i32) / 2;
    let jitter = spacing / 8;
    (0..count)
        .map(|index| {
            let (column, row) = ((index as u64 % side) as i32, (index as u64 / side) as i32);
            let x = center.x - half_width + spacing * Fp::from_num(column) + world.rng.range_fp(-jitter, jitter);
            let y = center.y - half_width + spacing * Fp::from_num(row) + world.rng.range_fp(-jitter, jitter);
            world.spawn_unit(template(team, Vec2Fp::new(x, y)))
        })
        .collect()
}

/// Two armies facing each other at the center of the map, ordered to advance.
pub fn battle(seed: u64, per_team: usize, config: SimConfig) -> Scenario {
    let mut world = SimWorld::new(config, seed);
    let center = map_center(&world);
    let half_block = SPAWN_SPACING * Fp::from_num(block_side(per_team) as i32) / 2;
    let offset = half_block + fp_int(BATTLE_GAP_UNITS / 2);
    let blue_center = Vec2Fp::new(center.x - offset, center.y);
    let red_center = Vec2Fp::new(center.x + offset, center.y);
    let blue = spawn_block(&mut world, Team::Blue, per_team, blue_center, SPAWN_SPACING, standard_unit);
    let red = spawn_block(&mut world, Team::Red, per_team, red_center, SPAWN_SPACING, standard_unit);
    Scenario {
        world,
        opening_commands: vec![
            SimCommand::AttackMove { units: blue, goal: red_center },
            SimCommand::AttackMove { units: red, goal: blue_center },
        ],
    }
}

/// The interactive scenario: the full 2,500 vs. 2,500 battle on a maximum-size map,
/// with walls to the north and south so that move orders visibly route through gaps.
pub fn default_battle(seed: u64) -> Scenario {
    let mut scenario = battle(seed, 2_500, SimConfig::default());
    let center = scenario.world.terrain.map_units() / 2;
    for (min_x, max_x, min_y, max_y) in [
        (-900, -60, 300, 340),
        (60, 900, 300, 340),
        (-900, -60, -340, -300),
        (60, 900, -340, -300),
    ] {
        scenario.world.add_box_obstacle(RectFp::new(
            Vec2Fp::new(fp_int(center + min_x), fp_int(center + min_y)),
            Vec2Fp::new(fp_int(center + max_x), fp_int(center + max_y)),
        ));
    }
    scenario.world.prepare_map();
    scenario
}

/// How a navigation scenario was laid out.
pub struct NavigationLayout {
    pub goal: Vec2Fp,
    /// Length of the shortest route around the walls, in units.
    pub route_units: i32,
    pub unit_speed: Fp,
    pub obstacles: Vec<RectFp>,
}

/// One army crossing three long walls, each with a narrow gap, on a maximum-size map.
pub fn navigation(seed: u64, count: usize) -> (Scenario, NavigationLayout) {
    const WALL_THICKNESS: i32 = 40;
    const WALL_HALF_LENGTH: i32 = 6_000;
    const GAP_WIDTH: i32 = 100;
    const WALL_SPACING: i32 = 1_100;

    let config = SimConfig { map_units: MAX_MAP_UNITS, terrain: TerrainStyle::Flat, ..SimConfig::default() };
    let mut world = SimWorld::new(config, seed);
    let center = MAX_MAP_UNITS / 2;
    let unit_speed = fp_int(4);

    // Gaps alternate from side to side, so the route has to weave between them.
    let gap_offsets = [300, -300, 300];
    let mut obstacles = Vec::new();
    for (index, gap_offset) in gap_offsets.into_iter().enumerate() {
        let x = center + (index as i32 + 1) * WALL_SPACING;
        let gap_center = center + gap_offset;
        let wall = |min_y: i32, max_y: i32| {
            RectFp::new(Vec2Fp::new(fp_int(x), fp_int(min_y)), Vec2Fp::new(fp_int(x + WALL_THICKNESS), fp_int(max_y)))
        };
        obstacles.push(wall(center - WALL_HALF_LENGTH, gap_center - GAP_WIDTH / 2));
        obstacles.push(wall(gap_center + GAP_WIDTH / 2, center + WALL_HALF_LENGTH));
    }
    for &rect in &obstacles {
        world.add_box_obstacle(rect);
    }
    world.prepare_map();

    let start = Vec2Fp::new(fp_int(center), fp_int(center));
    let goal = Vec2Fp::new(fp_int(center + 4 * WALL_SPACING + 300), fp_int(center));
    let units = spawn_block(&mut world, Team::Blue, count, start, SPAWN_SPACING, |team, position| UnitSpawn {
        max_speed: unit_speed,
        ..standard_unit(team, position)
    });
    let opening_commands = vec![SimCommand::Move { units, goal }];

    // Straight segments through each gap in turn, which is a lower bound on any route.
    let mut route_units = 0;
    let mut previous = (center, center);
    let stops = gap_offsets
        .iter()
        .enumerate()
        .map(|(index, offset)| (center + (index as i32 + 1) * WALL_SPACING + WALL_THICKNESS / 2, center + offset))
        .chain(std::iter::once((center + 4 * WALL_SPACING + 300, center)));
    for stop in stops {
        let (dx, dy) = (i64::from(stop.0 - previous.0), i64::from(stop.1 - previous.1));
        route_units += (dx * dx + dy * dy).unsigned_abs().isqrt() as i32;
        previous = stop;
    }

    (
        Scenario { world, opening_commands },
        NavigationLayout { goal, route_units, unit_speed, obstacles },
    )
}

/// Where each commander starts on the skirmish map. Red's start mirrors Blue's.
pub const SKIRMISH_MAP_UNITS: i32 = 10_240;
/// The center of a terrain cell. A point on a cell edge would belong to the cell above and to
/// the right of it, so its mirror image would not be in the mirror of its cell. No cell center
/// is on an edge of the terrain, spatial or visibility grids.
pub const SKIRMISH_BLUE_START: (i32, i32) = (1_610, 1_610);
const SKIRMISH_START_MASS: i64 = 650;
const SKIRMISH_START_ENERGY: i64 = 4_000;

/// The start position of a team on the skirmish map.
pub fn skirmish_start(team: Team) -> Vec2Fp {
    let (x, y) = SKIRMISH_BLUE_START;
    match team {
        Team::Blue => Vec2Fp::new(fp_int(x), fp_int(y)),
        Team::Red => Vec2Fp::new(fp_int(SKIRMISH_MAP_UNITS - x), fp_int(SKIRMISH_MAP_UNITS - y)),
    }
}

/// A 1v1 map for the vertical slice, with 180° rotational symmetry so that neither
/// start has an advantage: terrain, walls, mass deposits, and commanders all mirror.
pub fn skirmish(seed: u64) -> Scenario {
    let config = SimConfig {
        map_units: SKIRMISH_MAP_UNITS,
        terrain: TerrainStyle::MirroredHills,
        friendly_fire: true,
        fog_of_war: true,
        ..SimConfig::default()
    };
    let mut world = SimWorld::new(config, seed);
    let map = SKIRMISH_MAP_UNITS;
    let last_cell = (map / super::config::TERRAIN_CELL_UNITS) as u32 - 1;

    // Two mirrored walls in the middle make a choke point between the bases.
    let wall = |min_x: i32, min_y: i32, max_x: i32, max_y: i32| {
        RectFp::new(Vec2Fp::new(fp_int(min_x), fp_int(min_y)), Vec2Fp::new(fp_int(max_x), fp_int(max_y)))
    };
    world.add_box_obstacle(wall(3_200, 5_400, 5_000, 5_480));
    world.add_box_obstacle(wall(map - 5_000, map - 5_480, map - 3_200, map - 5_400));

    // Each deposit is added with its mirror image, in Blue's half of the map.
    let start_cell = SKIRMISH_BLUE_START.0 as u32 / super::config::TERRAIN_CELL_UNITS as u32;
    let near_base = [(-9i32, -5i32), (8, -7), (-6, 9), (9, 7)];
    let mut blue_side: Vec<(u32, u32)> =
        near_base.iter().map(|&(dx, dy)| ((start_cell as i32 + dx) as u32, (start_cell as i32 + dy) as u32)).collect();
    while blue_side.len() < 12 {
        let cell_x = 20 + world.rng.below(last_cell - 40);
        let cell_y = 20 + world.rng.below(last_cell / 2 - 40);
        let clear_of_base = cell_x.abs_diff(start_cell) > 16 || cell_y.abs_diff(start_cell) > 16;
        let spaced = blue_side.iter().all(|&(x, y)| x.abs_diff(cell_x) + y.abs_diff(cell_y) > 12);
        // Skip cells on, or right next to, a wall, so that every deposit can be reached and surrounded.
        let open = (cell_x - 2..=cell_x + 2).all(|x| (cell_y - 2..=cell_y + 2).all(|y| {
            world.terrain.cell_is_open(x, y) && world.terrain.cell_is_open(last_cell - x, last_cell - y)
        }));
        if clear_of_base && spaced && open {
            blue_side.push((cell_x, cell_y));
        }
    }
    for (x, y) in blue_side {
        world.deposits.push((x, y));
        world.deposits.push((last_cell - x, last_cell - y));
    }
    scatter_scenery(&mut world, start_cell);

    let commander = world.blueprints.id("command_rig");
    world.starts = [skirmish_start(Team::Blue), skirmish_start(Team::Red)];
    world.spawn_mobile(commander, Team::Blue, skirmish_start(Team::Blue));
    world.spawn_mobile(commander, Team::Red, skirmish_start(Team::Red));
    world.start_economy(SKIRMISH_START_MASS, SKIRMISH_START_ENERGY);
    world.prepare_map();
    Scenario { world, opening_commands: Vec::new() }
}

/// A map's data, in whole numbers, so that every instance builds the same world from it. The
/// bundled maps (`maps.rs`) generate theirs in code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapData {
    pub name: String,
    pub map_units: i32,
    /// One per cell corner, row by row, in sixteenths of a unit.
    pub heights: Vec<i16>,
    /// One per cell: too steep, or under water, for land units.
    pub blocked: Vec<bool>,
    /// Below this, land is under water. Far below the map if it has none.
    pub water_level: Fp,
    pub deposits: Vec<(u32, u32)>,
    /// Geothermal vents, by cell.
    pub vents: Vec<(u32, u32)>,
    /// Blue's start, then Red's: a 1v1's.
    pub starts: [Vec2Fp; 2],
    /// Starts for team matches, one list for each count of players the map holds beyond two:
    /// each lists a start a player, the first half one team's and the second half the other's.
    /// Empty for a map of two players.
    pub layouts: Vec<Vec<Vec2Fp>>,
    pub scenery: Vec<(WreckKind, Vec2Fp)>,
}

/// A map's trees and rocks. Rocks vary in size as the skirmish's do, and a rock holds mass in
/// proportion, so a rock and its images in the map's mirrors are one size, drawn once: each
/// side's rocks hold what the other side's do. Trees are all one size.
fn spawn_map_scenery(world: &mut SimWorld, map: &MapData) {
    let full = fp_int(map.map_units);
    let mut sizes = std::collections::BTreeMap::new();
    for &(kind, position) in &map.scenery {
        let radius = if kind == WreckKind::Rock {
            let images = [position, Vec2Fp::new(full - position.x, position.y), Vec2Fp::new(position.x, full - position.y), Vec2Fp::new(full - position.x, full - position.y)];
            let first = images.iter().map(|image| (image.x.to_bits(), image.y.to_bits())).min().expect("four images");
            *sizes.entry(first).or_insert_with(|| world.rng.range_fp(fp_int(5), fp_int(10)))
        } else {
            Fp::ZERO
        };
        world.spawn_scenery(kind, position, radius);
    }
}

/// A 1v1 on a map from a file: its terrain, deposits, scenery, and two of its starts.
pub fn from_map(map: &MapData, seed: u64) -> Scenario {
    let config = SimConfig { map_units: map.map_units, friendly_fire: true, fog_of_war: true, ..SimConfig::default() };
    let terrain = super::terrain::Terrain::from_heights(map.map_units, map.heights.clone(), &map.blocked, map.water_level);
    let mut world = SimWorld::with_terrain(config, seed, terrain);
    world.deposits.clone_from(&map.deposits);
    world.vents.clone_from(&map.vents);
    spawn_map_scenery(&mut world, map);
    let commander = world.blueprints.id("command_rig");
    world.starts = map.starts;
    world.spawn_mobile(commander, Team::Blue, map.starts[0]);
    world.spawn_mobile(commander, Team::Red, map.starts[1]);
    world.start_economy(SKIRMISH_START_MASS, SKIRMISH_START_ENERGY);
    world.prepare_map();
    Scenario { world, opening_commands: Vec::new() }
}

/// A team match on a map from a file (GAME_SPECIFICATION.md §17.2): `per_team` players a side,
/// at the map's starts for that many, the first half Blue's and the rest Red's, each with a
/// commander and an economy of their own. One a side is `from_map`'s 1v1. None if the map has
/// no starts for that many.
pub fn from_map_teams(map: &MapData, seed: u64, per_team: usize) -> Option<Scenario> {
    if per_team == 1 {
        return Some(from_map(map, seed));
    }
    let layout = map.layouts.iter().find(|layout| layout.len() == per_team * 2)?;
    let config = SimConfig { map_units: map.map_units, friendly_fire: true, fog_of_war: true, ..SimConfig::default() };
    let terrain = super::terrain::Terrain::from_heights(map.map_units, map.heights.clone(), &map.blocked, map.water_level);
    let mut world = SimWorld::with_terrain(config, seed, terrain);
    let teams: Vec<Team> = (0..per_team * 2).map(|index| if index < per_team { Team::Blue } else { Team::Red }).collect();
    world.set_players(&teams);
    world.deposits.clone_from(&map.deposits);
    world.vents.clone_from(&map.vents);
    spawn_map_scenery(&mut world, map);
    let commander = world.blueprints.id("command_rig");
    world.starts = [layout[0], layout[per_team]];
    for (index, &start) in layout.iter().enumerate() {
        world.player_starts[index] = start;
        world.spawn_mobile_for(commander, super::components::PlayerId(index as u8), start);
    }
    world.start_economy(SKIRMISH_START_MASS, SKIRMISH_START_ENERGY);
    world.prepare_map();
    Some(Scenario { world, opening_commands: Vec::new() })
}

/// Where a match is played: the generated skirmish map, one that comes with the game, the
/// playtest sandbox (`playtest.rs`), or a playtest scenario (`playtest_scenarios.rs`).
#[derive(Clone, Debug)]
pub enum MapSource {
    Skirmish,
    Map(std::sync::Arc<MapData>),
    /// A map from a file, with this many players a side, more than one (`MapSource::teams`).
    Teams(std::sync::Arc<MapData>, usize),
    Playtest,
    Scenario(std::sync::Arc<super::playtest_scenarios::PlaytestScenario>),
}

impl MapSource {
    /// A match with `per_team` players a side, if the map has starts for that many: only maps
    /// from files have team starts, and one a side is the ordinary 1v1.
    pub fn scenario_teams(&self, seed: u64, per_team: usize) -> Option<Scenario> {
        match (self, per_team) {
            (_, 1) => Some(self.scenario(seed)),
            (Self::Map(map) | Self::Teams(map, _), _) => from_map_teams(map, seed, per_team),
            _ => None,
        }
    }

    /// This map with `per_team` players a side, if it has starts for that many.
    pub fn teams(self, per_team: usize) -> Option<Self> {
        match (self, per_team) {
            (source, 1) => Some(source),
            (Self::Map(map) | Self::Teams(map, _), _) => map.layouts.iter().any(|layout| layout.len() == per_team * 2).then_some(Self::Teams(map, per_team)),
            _ => None,
        }
    }

    /// Players a side.
    pub const fn per_team(&self) -> usize {
        match self {
            Self::Teams(_, per_team) => *per_team,
            _ => 1,
        }
    }

    /// How far the map is across, in units.
    pub fn map_units(&self) -> i32 {
        match self {
            Self::Skirmish => SKIRMISH_MAP_UNITS,
            Self::Map(map) | Self::Teams(map, _) => map.map_units,
            Self::Playtest => super::playtest::MAP_UNITS,
            Self::Scenario(_) => super::playtest_scenarios::MAP_UNITS,
        }
    }

    pub fn scenario(&self, seed: u64) -> Scenario {
        match self {
            Self::Skirmish => skirmish(seed),
            Self::Map(map) => from_map(map, seed),
            Self::Teams(map, per_team) => from_map_teams(map, seed, *per_team).expect("checked by MapSource::teams"),
            Self::Playtest => super::playtest::playtest(seed),
            Self::Scenario(scenario) => scenario.build(seed, true),
        }
    }
}

const FORESTS_PER_SIDE: usize = 10;
const TREES_PER_FOREST: usize = 30;
const FOREST_RADIUS_UNITS: i32 = 140;
const ROCK_FIELDS_PER_SIDE: usize = 8;
const ROCKS_PER_FIELD: usize = 5;
const ROCK_FIELD_RADIUS_UNITS: i32 = 60;
/// Scenery stays this many cells from a start, so that bases have room, and from deposits.
const SCENERY_BASE_CLEARANCE_CELLS: u32 = 22;
const SCENERY_DEPOSIT_CLEARANCE_CELLS: u32 = 3;

/// Forests and rock fields on Blue's half of the skirmish map, each with its mirror image on
/// Red's, clear of the bases, the deposits, and the walls.
fn scatter_scenery(world: &mut SimWorld, start_cell: u32) {
    use super::components::WreckKind;
    let map = world.terrain.map_units();
    let last_cell = (map / super::config::TERRAIN_CELL_UNITS) as u32 - 1;
    let mirror = |point: Vec2Fp| Vec2Fp::new(fp_int(map) - point.x, fp_int(map) - point.y);
    let allowed = |world: &SimWorld, point: Vec2Fp| {
        // On Blue's half, well short of the middle, so that it and its mirror never meet.
        if point.y >= fp_int(map / 2 - 200) || !world.terrain.in_bounds(point) {
            return false;
        }
        let (x, y) = world.terrain.cell_of(point);
        let (x, y) = (x as u32, y as u32);
        let near = |a: u32, b: u32, cells: u32| a.abs_diff(b) <= cells;
        let clear_of_bases = [start_cell, last_cell - start_cell]
            .iter()
            .all(|&start| !(near(x, start, SCENERY_BASE_CLEARANCE_CELLS) && near(y, start, SCENERY_BASE_CLEARANCE_CELLS)));
        let clear_of_deposits = world
            .deposits
            .iter()
            .all(|&(dx, dy)| !(near(x, dx, SCENERY_DEPOSIT_CLEARANCE_CELLS) && near(y, dy, SCENERY_DEPOSIT_CLEARANCE_CELLS)));
        clear_of_bases && clear_of_deposits && world.terrain.is_passable(point) && world.terrain.is_passable(mirror(point))
    };
    let groups = [
        (WreckKind::Tree, FORESTS_PER_SIDE, TREES_PER_FOREST, FOREST_RADIUS_UNITS),
        (WreckKind::Rock, ROCK_FIELDS_PER_SIDE, ROCKS_PER_FIELD, ROCK_FIELD_RADIUS_UNITS),
    ];
    for (kind, group_count, per_group, spread) in groups {
        let mut placed_groups = 0;
        // Bounded, so that a crowded map cannot loop forever.
        for _ in 0..group_count * 50 {
            if placed_groups == group_count {
                break;
            }
            let center = Vec2Fp::new(world.rng.range_fp(fp_int(400), fp_int(map - 400)), world.rng.range_fp(fp_int(400), fp_int(map / 2)));
            if !allowed(world, center) {
                continue;
            }
            placed_groups += 1;
            let mut placed = 0;
            for _ in 0..per_group * 4 {
                if placed == per_group {
                    break;
                }
                let offset = |rng: &mut crate::math::rng::SimRng| rng.range_fp(fp_int(-spread), fp_int(spread));
                let (dx, dy) = (offset(&mut world.rng), offset(&mut world.rng));
                // Round clumps rather than square ones.
                if i64::from(dx.to_num::<i32>()).pow(2) + i64::from(dy.to_num::<i32>()).pow(2) > i64::from(spread).pow(2) {
                    continue;
                }
                let point = Vec2Fp::new(center.x + dx, center.y + dy);
                if !allowed(world, point) {
                    continue;
                }
                let radius = world.rng.range_fp(fp_int(5), fp_int(10));
                world.spawn_scenery(kind, point, radius);
                world.spawn_scenery(kind, mirror(point), radius);
                placed += 1;
            }
        }
    }
}

/// Advances a world one tick with its opening commands (on tick 0) and scripted orders.
/// The single code path behind headless runs, windowed test runs, and the determinism tests.
pub fn advance_scripted(world: &mut SimWorld, opening: &[SimCommand], seed: u64) {
    let mut commands = if world.current_tick == 0 { opening.to_vec() } else { Vec::new() };
    commands.extend(scripted_commands(world, seed));
    world.advance_tick(&commands);
}

/// A deterministic stream of orders for a world, derived only from the seed, the tick,
/// and the world's unit list. Headless and windowed runs issue identical orders.
pub fn scripted_commands(world: &SimWorld, seed: u64) -> Vec<SimCommand> {
    const ORDER_INTERVAL_TICKS: u32 = 25;
    let tick = world.current_tick;
    if !tick.is_multiple_of(ORDER_INTERVAL_TICKS) || world.units.is_empty() {
        return Vec::new();
    }
    let mut rng = SimRng::new(seed ^ u64::from(tick).wrapping_mul(0x9E37_79B9));
    let group = 1 + rng.below(world.units.len().min(200) as u32) as usize;
    let units: Vec<SimId> = (0..group)
        .map(|_| world.units[rng.below(world.units.len() as u32) as usize].0)
        .collect();
    let center = map_center(world);
    let spread = fp_int(150);
    let goal = Vec2Fp::new(
        center.x + rng.range_fp(-spread, spread),
        center.y + rng.range_fp(-spread, spread),
    );
    match rng.below(4) {
        0 => vec![SimCommand::Stop { units }],
        1 => vec![SimCommand::AttackMove { units, goal }],
        2 => {
            let target = world.units[rng.below(world.units.len() as u32) as usize].0;
            vec![SimCommand::Follow { units, target }]
        }
        _ => vec![SimCommand::Move { units, goal }],
    }
}
