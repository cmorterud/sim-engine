//! How each kind of anti-air fares against each kind of aircraft (GAME_SPECIFICATION.md §7,
//! Anti-Air by Tech Level), measured in the simulation from the real unit data: a group of
//! anti-air worth so much materiel, against one aircraft flying over it, circling over it, or
//! attacking something beside it. T1 anti-air barely scratches T3 aircraft; flak catches slow
//! aircraft and gunships but mostly misses fast T3 aircraft, unless massed; cruisers come close to
//! the SAM, which is the deterrent to T3 aircraft.
//!
//! Run with `--nocapture` to see the whole table.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Combat, Health, MoveClass, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
use sim_engine::sim::world::SimWorld;

const MAP: i32 = 10_240;
/// The lake, deep water: x and y from 1,000 to these. Ships sail there; the rest stand on land.
const LAKE_X_END: i32 = 4_600;
const LAKE_Y_END: i32 = 9_000;
const LAND: (i32, i32) = (7_400, 5_000);
const WATER: (i32, i32) = (2_800, 5_000);
/// Aircraft take this much before they would die, so that what is dealt can be measured.
const DUMMY_HEALTH: i32 = 400_000;
/// How long a circling or an attack lasts.
const TICKS: u32 = 300;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// Flat land 20 up, with a lake 40 deep, and plenty of materiel and energy for both teams.
fn world() -> SimWorld {
    let cells = (MAP / 20) as usize;
    let samples = cells + 1;
    let height = |x: usize, y: usize| {
        let (wx, wy) = (x as i32 * 20, y as i32 * 20);
        if (1_000..=LAKE_X_END).contains(&wx) && (1_000..=LAKE_Y_END).contains(&wy) { -40 } else { 20 }
    };
    let heights: Vec<i16> = (0..samples * samples).map(|index| (height(index % samples, index / samples) << HEIGHT_FRAC_BITS) as i16).collect();
    let blocked: Vec<bool> = (0..cells * cells)
        .map(|cell| {
            let (x, y) = (cell % cells, cell / cells);
            [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].iter().any(|&(cx, cy)| height(cx, cy) < 0)
        })
        .collect();
    let terrain = Terrain::from_heights(MAP, heights, &blocked, Fp::ZERO);
    let config = SimConfig { map_units: MAP, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::with_terrain(config, 1, terrain);
    world.prepare_map();
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(10_000_000);
        }
        economy.mass = Res::from_num(5_000_000);
        economy.energy = Res::from_num(5_000_000);
    }
    world
}

fn place(world: &mut SimWorld, key: &str, team: Team, at: (i32, i32)) -> SimId {
    let blueprint = world.blueprints.id(key);
    let def = world.blueprints.get(blueprint);
    if def.is_structure() {
        let half = u32::from(def.footprint) / 2;
        let (cell_x, cell_y) = (at.0 as u32 / 20 - half, at.1 as u32 / 20 - half);
        world.spawn_finished_structure(blueprint, team, cell_x, cell_y).unwrap_or_else(|| panic!("no room for {key} at {at:?}"))
    } else {
        world.spawn_mobile(blueprint, team, point(at.0, at.1))
    }
}

/// Makes a unit all but indestructible, and, unless it is to attack, harmless.
fn toughen(world: &mut SimWorld, id: SimId, fights: bool) {
    let entity = world.unit_entity(id).unwrap();
    world.ecs.get::<&mut Combat>(entity).unwrap().enabled = fights;
    let mut health = world.ecs.get::<&mut Health>(entity).unwrap();
    health.max = fp_int(DUMMY_HEALTH);
    health.current = health.max;
}

/// How an aircraft meets the anti-air.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Approach {
    /// Flies straight over it, once, from out of range to out of range.
    Pass,
    /// Patrols a square round it, turning at each corner.
    Circle,
    /// Attacks a ground target in the middle of it: a bomber makes passes, a gunship hovers.
    Attack,
}

/// The share of an aircraft's health that anti-air of `shooter`, as many as `materiel` buys, takes
/// off it as it meets them by `approach`.
fn health_lost(shooter: &str, materiel: i64, aircraft: &str, approach: Approach) -> f64 {
    let mut world = world();
    let def = world.blueprints.get(world.blueprints.id(shooter)).clone();
    let plane = world.blueprints.get(world.blueprints.id(aircraft)).clone();
    let sails = matches!(def.movement, MoveClass::Naval) || def.on_water;
    let center = if sails { WATER } else { LAND };
    let count = (materiel + def.mass.to_num::<i64>() - 1) / def.mass.to_num::<i64>();
    let count = count.max(1) as i32;
    // In a tight square, as a player would mass them.
    let side = (1..).find(|side| side * side >= count).unwrap();
    let gap = (i32::from(def.footprint) * 20 + 20).max(def.radius.to_num::<i32>() * 3);
    for index in 0..count {
        let (column, row) = (index % side, index / side);
        let at = (center.0 + (column - side / 2) * gap, center.1 + (row - side / 2) * gap);
        let id = place(&mut world, shooter, Team::Blue, at);
        let entity = world.unit_entity(id).unwrap();
        let mut health = world.ecs.get::<&mut Health>(entity).unwrap();
        health.max = fp_int(DUMMY_HEALTH);
        health.current = health.max;
    }
    let reach = 2_000;
    let start = match approach {
        Approach::Pass => (center.0, center.1 - reach),
        Approach::Circle | Approach::Attack => (center.0 - 600, center.1 - reach),
    };
    let target = place(&mut world, aircraft, Team::Red, start);
    toughen(&mut world, target, approach == Approach::Attack);
    let order = match approach {
        Approach::Pass => SimCommand::Move { units: vec![target], goal: point(center.0, center.1 + reach) },
        Approach::Circle => {
            let corners = [(-600, -600), (600, -600), (600, 600), (-600, 600)].map(|(dx, dy)| point(center.0 + dx, center.1 + dy));
            SimCommand::Patrol { units: vec![target], points: corners.to_vec(), queued: false }
        }
        Approach::Attack => {
            let ground = place(&mut world, "t1_engineer", Team::Blue, (center.0 + 300, center.1));
            toughen(&mut world, ground, false);
            SimCommand::Attack { units: vec![target], target: ground }
        }
    };
    world.advance_tick(&[order]);
    let ticks = match approach {
        // Long enough to fly the whole way at its speed, and a little more.
        Approach::Pass => (reach * 2 / plane.speed.to_num::<i32>().max(1)) as u32 + 20,
        Approach::Circle | Approach::Attack => TICKS,
    };
    for _ in 0..ticks {
        world.advance_tick(&[]);
    }
    let taken = DUMMY_HEALTH - world.unit_health(target).map_or(0, |health| health.to_num::<i32>());
    f64::from(taken) / plane.health.to_num::<f64>()
}

fn mass(key: &str) -> i64 {
    let world = world();
    world.blueprints.get(world.blueprints.id(key)).mass.to_num::<i64>()
}

const FAST_T3: [&str; 3] = ["t3_air_superiority", "t3_spy_plane", "t3_strategic_bomber"];
const GUNSHIPS: [&str; 2] = ["t2_gunship", "t3_heavy_gunship"];

/// How an aircraft usually meets anti-air: bombers and gunships attacking, fighters and scouts
/// circling.
fn usual(aircraft: &str) -> Approach {
    if matches!(aircraft, "t3_air_superiority" | "t3_spy_plane") { Approach::Circle } else { Approach::Attack }
}

/// The whole table, printed, for tuning and playtesting.
#[test]
fn anti_air_against_every_tech_level_of_aircraft() {
    let shooters = ["t1_aa_turret", "t1_mobile_aa", "t2_flak", "t2_mobile_flak", "t2_cruiser", "t3_sam"];
    let aircraft = ["t1_bomber", "t2_gunship", "t2_fighter_bomber", "t3_heavy_gunship", "t3_air_superiority", "t3_spy_plane", "t3_strategic_bomber"];
    println!("Health lost, as a share of the aircraft's, to anti-air worth as much materiel as the aircraft, and three times as much, over a pass or {} s", TICKS / 10);
    println!("{:<18}{:<22}{:>8}{:>8}{:>8}{:>10}", "anti-air", "aircraft", "pass", "circle", "attack", "usual x3");
    for shooter in shooters {
        for plane in aircraft {
            let worth = mass(plane);
            let cells: Vec<String> = [Approach::Pass, Approach::Circle, Approach::Attack].iter().map(|&approach| format!("{:.0}%", 100.0 * health_lost(shooter, worth, plane, approach))).collect();
            let massed = format!("{:.0}%", 100.0 * health_lost(shooter, worth * 3, plane, usual(plane)));
            println!("{shooter:<18}{plane:<22}{:>8}{:>8}{:>8}{massed:>10}", cells[0], cells[1], cells[2]);
        }
    }
}

/// T1 anti-air barely scratches a fast T3 aircraft: as much materiel of it as the aircraft cost
/// cannot bring it down in a pass, nor a strategic bomber over its attack.
#[test]
fn t1_anti_air_cannot_bring_down_t3_aircraft() {
    let mut failures = Vec::new();
    for shooter in ["t1_aa_turret", "t1_mobile_aa"] {
        for plane in FAST_T3 {
            let lost = health_lost(shooter, mass(plane), plane, Approach::Pass);
            if lost >= 0.5 {
                failures.push(format!("{shooter} took {:.0}% off a {plane} in a pass", lost * 100.0));
            }
        }
        let lost = health_lost(shooter, mass("t3_strategic_bomber"), "t3_strategic_bomber", Approach::Attack);
        if lost >= 1.0 {
            failures.push(format!("{shooter} brought down a strategic bomber attacking them ({:.0}%)", lost * 100.0));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Flak mostly misses fast T3 aircraft: as much of it as the aircraft cost takes under half its
/// health, flying over or as it usually goes about its work, though it cannot shoot back here.
/// Massed, it all but brings down a strategic bomber attacking it: seven times its cost since the
/// price list set it at 1,253 (2026-11-26), about the same flak as three times its borrowed 3,381.
#[test]
fn flak_mostly_misses_fast_t3_aircraft_unless_massed() {
    let mut failures = Vec::new();
    for shooter in ["t2_flak", "t2_mobile_flak"] {
        for plane in FAST_T3 {
            for approach in [Approach::Pass, usual(plane)] {
                let lost = health_lost(shooter, mass(plane), plane, approach);
                if lost >= 0.5 {
                    failures.push(format!("{shooter} took {:.0}% off a {plane} ({approach:?})", lost * 100.0));
                }
            }
        }
        let massed = health_lost(shooter, mass("t3_strategic_bomber") * 7, "t3_strategic_bomber", Approach::Attack);
        if massed < 0.8 {
            failures.push(format!("three times a strategic bomber's worth of {shooter} took only {:.0}% off it", massed * 100.0));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Gunships hover over what they attack, too slow to outfly flak: as much flak as a gunship cost
/// brings it down over its attack, and half as much again for the heavy gunship since the price
/// list halved its cost (2026-11-26), less flak than its borrowed cost bought.
#[test]
fn flak_brings_down_gunships() {
    let mut failures = Vec::new();
    for shooter in ["t2_flak", "t2_mobile_flak"] {
        for plane in GUNSHIPS {
            let worth = if plane == "t3_heavy_gunship" { mass(plane) * 3 / 2 } else { mass(plane) };
            let lost = health_lost(shooter, worth, plane, Approach::Attack);
            if lost < 1.0 {
                failures.push(format!("{shooter} took only {:.0}% off a {plane} attacking it", lost * 100.0));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The SAM is the deterrent to T3 aircraft: one badly damages a strategic bomber in a pass, and
/// two bring it down. A cruiser, the fleet's anti-air, has done more than one SAM since
/// 2026-11-25: four of its volleys, about seven seconds, destroy a strategic bomber, where a SAM
/// takes about eleven ([volleys] and [kill_times] in data/balance_targets.toml). It was about
/// three quarters of a SAM until then.
#[test]
fn sams_deter_t3_aircraft_and_cruisers_come_close() {
    let pass = |shooter: &str, count: i64| health_lost(shooter, mass(shooter) * count, "t3_strategic_bomber", Approach::Pass);
    let (one, two, cruiser) = (pass("t3_sam", 1), pass("t3_sam", 2), pass("t2_cruiser", 1));
    assert!(one >= 0.5, "one SAM took only {:.0}% off a strategic bomber in a pass", one * 100.0);
    assert!(two >= 1.0, "two SAMs took only {:.0}% off a strategic bomber in a pass", two * 100.0);
    let share = cruiser / one;
    assert!((1.1..=1.7).contains(&share), "a cruiser did {:.0}% of what a SAM does to a strategic bomber in a pass", share * 100.0);
}
