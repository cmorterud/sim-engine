//! A sandbox for trying units out (README, Playtest): 5 of every unit for Blue, with no commander,
//! so that there is nothing to lose, on a map that is land to the west and sea to the east, with
//! two Red bases on the coast between, which a turtling AI keeps and defends.

use crate::math::fp::{fp_int, Fp, Vec2Fp};

use super::components::{MoveClass, Team};
use super::config::{SimConfig, TerrainStyle, TERRAIN_CELL_UNITS};
use super::scenario::Scenario;
use super::terrain::{Terrain, HEIGHT_FRAC_BITS};
use super::world::SimWorld;

pub const MAP_UNITS: i32 = 10_240;
/// The land ends here, going down to the sea floor by `SEA_FROM`.
const SHORE_FROM: i32 = 5_000;
const SEA_FROM: i32 = 5_400;
const LAND_HEIGHT: i32 = 20;
const SEA_FLOOR: i32 = -50;
/// How many of each unit Blue has.
const EACH: usize = 5;
/// Where Blue's land army, aircraft, and fleet start.
const BLUE_LAND: (i32, i32) = (1_400, 5_000);
const BLUE_AIR: (i32, i32) = (1_400, 8_200);
const BLUE_SEA: (i32, i32) = (8_400, 5_000);
/// Red's two bases, on the coast, in cells: the main one, with its commander, and the outpost.
const RED_MAIN: (u32, u32) = (215, 125);
const RED_OUTPOST: (u32, u32) = (215, 385);
/// Blue starts with a little to build with, and no income.
const BLUE_MASS: i64 = 3_000;
const BLUE_ENERGY: i64 = 30_000;
const RED_MASS: i64 = 2_000;
const RED_ENERGY: i64 = 20_000;

/// The ground's height at a sample, in map units: flat land, a shore, and a flat sea floor.
fn height(x: i32) -> i32 {
    if x <= SHORE_FROM {
        LAND_HEIGHT
    } else if x >= SEA_FROM {
        SEA_FLOOR
    } else {
        LAND_HEIGHT + (SEA_FLOOR - LAND_HEIGHT) * (x - SHORE_FROM) / (SEA_FROM - SHORE_FROM)
    }
}

fn terrain() -> Terrain {
    let cells = (MAP_UNITS / TERRAIN_CELL_UNITS) as usize;
    let samples = cells + 1;
    let at = |index: usize| height(index as i32 * TERRAIN_CELL_UNITS);
    let heights: Vec<i16> = (0..samples * samples).map(|index| (at(index % samples) << HEIGHT_FRAC_BITS) as i16).collect();
    // Land units go nowhere any corner is under the water.
    let blocked: Vec<bool> = (0..cells * cells).map(|cell| at(cell % cells) < 0 || at(cell % cells + 1) < 0).collect();
    Terrain::from_heights(MAP_UNITS, heights, &blocked, Fp::ZERO)
}

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// The sandbox: Blue's units, Red's two bases and commander. Fog of war is on.
pub fn playtest(seed: u64) -> Scenario {
    let config = SimConfig { map_units: MAP_UNITS, terrain: TerrainStyle::Flat, friendly_fire: true, fog_of_war: true, ..SimConfig::default() };
    let mut world = SimWorld::with_terrain(config, seed, terrain());
    world.starts = [point(BLUE_LAND.0, BLUE_LAND.1), point(i32::try_from(RED_MAIN.0).unwrap_or(0) * TERRAIN_CELL_UNITS - 300, i32::try_from(RED_MAIN.1).unwrap_or(0) * TERRAIN_CELL_UNITS)];
    // Deposits round each base for its extractors, and a few on Blue's side.
    for (cx, cy) in [RED_MAIN, RED_OUTPOST] {
        for (dx, dy) in [(-14, -12), (-14, 12), (-20, 0), (-6, -16), (-6, 16)] {
            world.deposits.push(((cx as i32 + dx) as u32, (cy as i32 + dy) as u32));
        }
    }
    for (x, y) in [(60, 240), (60, 270), (80, 255), (100, 230), (100, 280)] {
        world.deposits.push((x, y));
    }
    blue_power(&mut world);
    red_base(&mut world, RED_MAIN, true);
    red_base(&mut world, RED_OUTPOST, false);
    let commander = world.blueprints.id("command_rig");
    world.spawn_mobile(commander, Team::Red, world.starts[1]);
    blue_army(&mut world);
    world.start_economy(0, 0);
    world.economies[Team::Blue.index()].mass = crate::sim::blueprints::Res::from_num(BLUE_MASS);
    world.economies[Team::Blue.index()].energy = crate::sim::blueprints::Res::from_num(BLUE_ENERGY);
    world.economies[Team::Red.index()].mass = crate::sim::blueprints::Res::from_num(RED_MASS);
    world.economies[Team::Red.index()].energy = crate::sim::blueprints::Res::from_num(RED_ENERGY);
    world.prepare_map();
    Scenario { world, opening_commands: Vec::new() }
}

/// Blue's power, behind its army: two T3 generators, far more than its shields and the rest
/// draw, so that none of them goes down, and storage for the energy it starts with. With none,
/// its mobile shields, corvettes, and walkers' and transports' shields wanted ten times what its
/// field rigs make, and most collapsed in the first half minute.
fn blue_power(world: &mut SimWorld) {
    for (key, cell_x, cell_y) in [("power_generator_t3", 12, 228), ("power_generator_t3", 12, 238), ("energy_storage", 20, 230), ("energy_storage", 20, 240)] {
        let blueprint = world.blueprints.id(key);
        let _ = world.spawn_finished_structure(blueprint, Team::Blue, cell_x, cell_y);
    }
}

/// A Red base round `center`, in cells, on the coast: a factory, generators and extractors, point
/// defense and anti-air facing both the land and the sea, a shield, a radar, and torpedo launchers
/// in the water. The main base also has naval and air factories.
fn red_base(world: &mut SimWorld, center: (u32, u32), main: bool) {
    let (cx, cy) = (center.0 as i32, center.1 as i32);
    let place = |world: &mut SimWorld, key: &str, dx: i32, dy: i32| {
        let blueprint = world.blueprints.id(key);
        let _ = world.spawn_finished_structure(blueprint, Team::Red, (cx + dx) as u32, (cy + dy) as u32);
    };
    place(world, "land_factory_t2", -10, -3);
    for (dx, dy) in [(-14, -12), (-14, 12), (-20, 0), (-6, -16), (-6, 16)] {
        place(world, "mass_extractor", dx, dy);
    }
    for (dx, dy) in [(-3, -8), (-3, -5), (0, -8), (0, -5)] {
        place(world, "power_generator", dx, dy);
    }
    place(world, "power_generator_t2", 3, 6);
    place(world, "shield_generator", -2, 1);
    place(world, "radar", -16, -6);
    place(world, "energy_storage", -5, 6);
    // Facing the land, to the west.
    for dy in [-8, -2, 4, 10] {
        place(world, "t1_point_defense", -26, dy);
    }
    place(world, "t2_point_defense", -24, -5);
    place(world, "t2_point_defense", -24, 7);
    // Facing the sea, to the east, and the sky.
    for dy in [-6, 2, 10] {
        place(world, "t1_point_defense", 8, dy);
    }
    place(world, "t1_aa_turret", -8, 10);
    place(world, "t1_aa_turret", 6, -10);
    place(world, "t2_flak", 2, 12);
    place(world, "tactical_missile_defense", -12, 8);
    for dy in [-8, 8] {
        place(world, "t1_torpedo_launcher", (SEA_FROM / TERRAIN_CELL_UNITS) - cx + 6, dy);
    }
    if main {
        place(world, "air_factory", -16, 14);
        place(world, "naval_factory", (SEA_FROM / TERRAIN_CELL_UNITS) - cx + 14, -2);
        place(world, "t2_artillery", -8, -14);
    }
}

/// `EACH` of every unit there is, but the commander and missiles: land units in rows on Blue's
/// side, aircraft parked to the south, and ships and submarines out at sea.
fn blue_army(world: &mut SimWorld) {
    let kinds: Vec<_> = world
        .blueprints
        .iter()
        .filter(|def| !def.is_structure() && !def.is_commander() && def.class != super::blueprints::UnitClass::Ammo)
        .filter(|def| def.speed > Fp::ZERO)
        .map(|def| (world.blueprints.id(&def.key), def.is_air(), matches!(def.movement, MoveClass::Naval | MoveClass::Submarine), def.radius))
        .collect();
    let mut counts = [0usize; 3];
    for (blueprint, air, sea, radius) in kinds {
        let (group, origin) = if sea { (2, BLUE_SEA) } else if air { (1, BLUE_AIR) } else { (0, BLUE_LAND) };
        // Rows of `EACH`, one kind to a row, far enough apart for the largest.
        let row = counts[group];
        counts[group] += 1;
        let spacing = (radius.to_num::<i32>() * 3).max(40);
        let row_gap = if sea { 160 } else { 90 };
        for index in 0..EACH {
            let x = origin.0 + (index as i32 - EACH as i32 / 2) * spacing;
            let y = origin.1 + (row as i32 - 8) * row_gap;
            world.spawn_mobile(blueprint, Team::Blue, point(x, y.clamp(400, MAP_UNITS - 400)));
        }
    }
}
