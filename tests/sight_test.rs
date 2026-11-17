//! Sight and radar against the unit data (GAME_SPECIFICATION.md §9): every unit and structure sees
//! an enemy just inside its vision and not one just outside it, and every radar shows a blip for
//! an enemy just inside its range, beyond sight, and none just outside. Sight is kept on a grid,
//! so "just" is a cell and a half.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::{Blueprint, Res};
use sim_engine::sim::components::{MoveClass, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
use sim_engine::sim::world::SimWorld;

const MAP: i32 = 20_480;
/// A lake from here to `LAKE_END`, for ships, with open land beyond it to the east.
const LAKE_START: i32 = 1_000;
const LAKE_END: i32 = 5_000;
/// How far inside or outside a range is clearly in or out of it, sight being kept in cells of 80.
const MARGIN: i32 = 120;

fn world() -> SimWorld {
    let cells = (MAP / 20) as usize;
    let samples = cells + 1;
    let height = |x: usize, _y: usize| if (LAKE_START..=LAKE_END).contains(&(x as i32 * 20)) { -40 } else { 20 };
    let heights: Vec<i16> = (0..samples * samples).map(|index| (height(index % samples, index / samples) << HEIGHT_FRAC_BITS) as i16).collect();
    let blocked: Vec<bool> = (0..cells * cells)
        .map(|cell| {
            let (x, y) = (cell % cells, cell / cells);
            [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].iter().any(|&(cx, cy)| height(cx, cy) < 0)
        })
        .collect();
    let terrain = Terrain::from_heights(MAP, heights, &blocked, Fp::ZERO);
    let config = SimConfig { map_units: MAP, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() };
    let mut world = SimWorld::with_terrain(config, 1, terrain);
    world.prepare_map();
    for economy in &mut world.economies {
        for value in [&mut economy.base_energy_capacity, &mut economy.energy_capacity, &mut economy.energy] {
            *value = Res::from_num(10_000_000);
        }
    }
    world
}

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn sails(def: &Blueprint) -> bool {
    matches!(def.movement, MoveClass::Naval | MoveClass::Submarine) || def.on_water
}

/// Places the unit or structure under test, on the water near the lake's east shore if it sails,
/// or else on land, and the enemy `away` to the east of it on land. The enemy cannot see or shoot.
fn setting(def: &Blueprint, away: i32) -> (SimWorld, SimId, SimId, Vec2Fp) {
    let mut world = world();
    let from = if sails(def) { (LAKE_END - 300, 10_000) } else { (LAKE_END + 600, 10_000) };
    let blueprint = world.blueprints.id(&def.key);
    let watcher = if def.is_structure() {
        let half = u32::from(def.footprint) / 2;
        let cell = (from.0 as u32 / 20 - half, from.1 as u32 / 20 - half);
        if def.requires_deposit {
            world.deposits.push(cell);
        }
        if def.requires_vent {
            world.vents.push((cell.0 + half, cell.1 + half));
        }
        world.spawn_finished_structure(blueprint, Team::Blue, cell.0, cell.1).unwrap_or_else(|| panic!("no room for {}", def.key))
    } else {
        world.spawn_mobile(blueprint, Team::Blue, point(from.0, from.1))
    };
    // It does not shoot what it is looking at.
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(watcher).unwrap()).unwrap().enabled = false;
    // An aircraft is held where it is, rather than gliding on as it lands.
    if def.is_air() {
        let entity = world.unit_entity(watcher).unwrap();
        world.ecs.get::<&mut sim_engine::sim::components::UnitStats>(entity).unwrap().max_speed = Fp::ZERO;
        world.ecs.get::<&mut sim_engine::sim::components::Flight>(entity).unwrap().speed = Fp::ZERO;
    }
    let watcher_at = world.unit_position(watcher).unwrap().xy();
    let enemy_at = Vec2Fp::new(watcher_at.x + fp_int(away), watcher_at.y);
    let enemy = world.spawn_mobile(world.blueprints.id("t1_engineer"), Team::Red, enemy_at);
    // Two refreshes of sight, and an aircraft's climb to its height.
    for _ in 0..40 {
        world.advance_tick(&[]);
    }
    (world, watcher, enemy, enemy_at)
}

fn seen(world: &SimWorld, enemy: SimId) -> bool {
    world.snapshot_for(Some(Team::Blue)).units.iter().any(|unit| unit.id == enemy)
}

fn blip_near(world: &SimWorld, at: Vec2Fp) -> bool {
    let (x, y) = (at.x.to_bits(), at.y.to_bits());
    world.snapshot_for(Some(Team::Blue)).blips.iter().any(|blip| (i64::from(blip[0] - x).pow(2) + i64::from(blip[1] - y).pow(2)) < i64::from(fp_int(200).to_bits()).pow(2))
}

#[test]
fn every_unit_sees_to_its_vision_and_no_further() {
    let db = world().blueprints;
    let mut failures = Vec::new();
    let mut checked = 0;
    for def in db.iter().filter(|def| def.vision > Fp::from_num(MARGIN * 2)) {
        checked += 1;
        let vision = def.vision.to_num::<i32>();
        let (world, _, enemy, _) = setting(def, vision - MARGIN);
        if !seen(&world, enemy) {
            failures.push(format!("{}: did not see an enemy {} away, inside its {vision} vision", def.key, vision - MARGIN));
        }
        // Radar would show it as a blip, but not as a unit: sight alone shows units.
        let (world, _, enemy, _) = setting(def, vision + MARGIN);
        if seen(&world, enemy) {
            failures.push(format!("{}: saw an enemy {} away, beyond its {vision} vision", def.key, vision + MARGIN));
        }
    }
    assert!(checked >= 60, "only {checked} were checked");
    assert!(failures.is_empty(), "{} failures:\n{failures:#?}", failures.len());
}

#[test]
fn every_radar_shows_a_blip_to_its_range_and_no_further() {
    let db = world().blueprints;
    let mut failures = Vec::new();
    let mut checked = 0;
    for def in db.iter().filter(|def| def.radar > def.vision + Fp::from_num(MARGIN * 3)) {
        checked += 1;
        let radar = def.radar.to_num::<i32>();
        let (world, _, _, at) = setting(def, radar - MARGIN);
        if !blip_near(&world, at) {
            failures.push(format!("{}: no blip for an enemy {} away, inside its {radar} radar", def.key, radar - MARGIN));
        }
        let (world, _, _, at) = setting(def, radar + MARGIN);
        if blip_near(&world, at) {
            failures.push(format!("{}: a blip for an enemy {} away, beyond its {radar} radar", def.key, radar + MARGIN));
        }
    }
    assert!(checked >= 5, "only {checked} radars were checked");
    assert!(failures.is_empty(), "{} failures:\n{failures:#?}", failures.len());
}
