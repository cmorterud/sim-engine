//! Kill times (data/balance_targets.toml, [volleys] and [kill_times]): how many volleys the
//! cruiser's anti-air takes for each aircraft, and how long set attackers take to destroy a
//! commander with no enhancements, a submarine, and aircraft, in the sim, timed from the first
//! shot fired.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::{Blueprint, BlueprintDb, Res, Targets, WeaponDef, TICKS_PER_SECOND};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Combat, Flight, Projectile, SimId, Team, UnitStats};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
use sim_engine::sim::world::SimWorld;

const TARGETS: &str = include_str!("../data/balance_targets.toml");

fn targets() -> toml::Table {
    TARGETS.parse().expect("data/balance_targets.toml is valid")
}

fn number(value: &toml::Value) -> f64 {
    value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).expect("a number")
}

/// A [kill_times] range, in seconds.
fn seconds_range(name: &str) -> (f64, f64) {
    let table = targets();
    let range = table["kill_times"][name].as_array().unwrap_or_else(|| panic!("[kill_times] {name} is a range"));
    (number(&range[0]), number(&range[1]))
}

/// The one weapon a unit has at aircraft.
fn anti_air(unit: &Blueprint) -> WeaponDef {
    let mut weapons = unit.weapon.iter().chain(&unit.extra_weapons).filter(|weapon| weapon.targets == Targets::Air);
    let weapon = *weapons.next().unwrap_or_else(|| panic!("{} has a weapon at aircraft", unit.key));
    assert!(weapons.next().is_none(), "{} has one weapon at aircraft", unit.key);
    weapon
}

/// Each [volleys] rule holds exactly: so many volleys of the unit's anti-air destroy each aircraft
/// at its base health, and one fewer does not.
#[test]
fn every_volleys_rule_holds_exactly() {
    let db = BlueprintDb::standard();
    let table = targets();
    let volleys = table["volleys"].as_table().expect("[volleys]");
    let mut checked = 0;
    for (shooter, rule) in volleys {
        let weapon = anti_air(db.get(db.id(shooter)));
        let salvo = weapon.salvo.unwrap_or_else(|| panic!("{shooter}'s anti-air fires volleys"));
        let volley = weapon.damage.to_num::<f64>() * f64::from(salvo.shots);
        for (target, wanted) in rule.as_table().expect("a table of aircraft") {
            let health = db.get(db.id(target)).health.to_num::<f64>();
            let took = (health / volley).ceil() as i64;
            assert_eq!(took, wanted.as_integer().expect("a count"), "{shooter}'s volley of {volley} destroys {target} ({health}) in {took}");
            checked += 1;
        }
    }
    assert!(checked >= 12, "only {checked} rules");
}

/// Shots of a volley leave `i * ticks / shots` ticks after its first, and the next volley starts a
/// reload after the first: the cruiser's missiles a second apart, four, then six seconds' wait; its
/// anti-air's eight over the first second of every two.
#[test]
fn the_cruisers_volleys_are_spaced_as_designed() {
    let db = BlueprintDb::standard();
    let cruiser = db.get(db.id("t2_cruiser"));
    let missiles = cruiser.weapon.expect("cruise missiles");
    let salvo = missiles.salvo.expect("in volleys");
    assert_eq!((salvo.shots, missiles.reload_ticks), (4, 100));
    assert_eq!((0..4).map(|shot| salvo.offset(shot)).collect::<Vec<_>>(), [0, 10, 20, 30]);
    let air = anti_air(cruiser);
    let salvo = air.salvo.expect("in volleys");
    assert_eq!((salvo.shots, air.reload_ticks), (8, 20));
    assert!(salvo.offset(7) < 10, "the eighth leaves within the first second");
}

/// Where the water is: 40 deep, for x from 1,000 to `SHORE_X` and y from 1,000 to 9,000. The land
/// is 20 up.
const SHORE_X: i32 = 5_000;
const MAP: i32 = 10_240;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn world() -> SimWorld {
    let cells = (MAP / 20) as usize;
    let samples = cells + 1;
    let height = |x: usize, y: usize| {
        let (wx, wy) = (x as i32 * 20, y as i32 * 20);
        if (1_000..=SHORE_X).contains(&wx) && (1_000..=9_000).contains(&wy) { -40 } else { 20 }
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

/// Places a unit, or a finished structure centred on the point.
fn place(world: &mut SimWorld, key: &str, team: Team, at: (i32, i32)) -> SimId {
    let blueprint = world.blueprints.id(key);
    let def = world.blueprints.get(blueprint);
    if def.is_structure() {
        let half = u32::from(def.footprint) / 2;
        world.spawn_finished_structure(blueprint, team, at.0 as u32 / 20 - half, at.1 as u32 / 20 - half).unwrap_or_else(|| panic!("no room for {key} at {at:?}"))
    } else {
        world.spawn_mobile(blueprint, team, point(at.0, at.1))
    }
}

/// A target that holds still where it is put and never fires back, aircraft kept in the air.
fn target(world: &mut SimWorld, key: &str, at: (i32, i32)) -> SimId {
    let id = place(world, key, Team::Red, at);
    let entity = world.unit_entity(id).unwrap();
    world.ecs.get::<&mut Combat>(entity).unwrap().enabled = false;
    world.ecs.get::<&mut UnitStats>(entity).unwrap().max_speed = Fp::ZERO;
    let flying = world.ecs.get::<&mut Flight>(entity).map(|mut flight| {
        flight.speed = Fp::ZERO;
        flight.max_fuel = 1_000_000;
        flight.fuel = flight.max_fuel;
    });
    if flying.is_ok() {
        // On patrol, with no speed, it hangs where it is.
        let here = point(at.0, at.1);
        world.advance_tick(&[SimCommand::Patrol { units: vec![id], points: vec![here, here + point(0, 50)], queued: false }]);
    }
    id
}

/// Ticks from the first shot any of the shooters fires to the target's end, or `None` within the
/// limit.
fn ticks_to_kill(world: &mut SimWorld, shooters: &[SimId], target: SimId, limit: u32) -> Option<u32> {
    let mut first = None;
    for tick in 0..limit {
        world.advance_tick(&[]);
        if first.is_none() {
            let fired = world.projectiles.iter().any(|&(_, entity)| world.ecs.get::<&Projectile>(entity).is_ok_and(|projectile| shooters.contains(&projectile.source)));
            if fired {
                first = Some(tick);
            }
        }
        if world.unit_entity(target).is_none() {
            return first.map(|first| tick - first);
        }
    }
    None
}

fn seconds(ticks: u32) -> f64 {
    f64::from(ticks) / TICKS_PER_SECOND as f64
}

fn assert_within(name: &str, ticks: Option<u32>) {
    let (least, most) = seconds_range(name);
    let ticks = ticks.unwrap_or_else(|| panic!("{name}: the target was not destroyed"));
    let took = seconds(ticks);
    println!("{name}: {took} s");
    assert!((least..=most).contains(&took), "{name}: {took} s, not within {least} to {most}");
}

/// A cruiser offshore, out of its gun's reach, destroys a commander on land with its cruise
/// missiles in about 100 seconds.
#[test]
fn a_cruiser_destroys_a_commander_on_land_with_its_missiles() {
    let mut world = world();
    let cruiser = place(&mut world, "t2_cruiser", Team::Blue, (SHORE_X - 600, 5_000));
    let commander = target(&mut world, "command_rig", (SHORE_X + 1_400, 5_000));
    assert_within("cruiser_missiles_commander_on_land", ticks_to_kill(&mut world, &[cruiser], commander, 2_000));
}

/// A destroyer's guns destroy a commander on land in about 80 seconds.
#[test]
fn a_destroyers_guns_destroy_a_commander_on_land() {
    let mut world = world();
    let destroyer = place(&mut world, "t2_destroyer", Team::Blue, (SHORE_X - 400, 5_000));
    let commander = target(&mut world, "command_rig", (SHORE_X + 400, 5_000));
    assert_within("destroyer_guns_commander_on_land", ticks_to_kill(&mut world, &[destroyer], commander, 2_000));
}

/// Five destroyers' torpedoes destroy a commander walking the seabed in about 80 seconds: their
/// guns cannot reach it there.
#[test]
fn five_destroyers_torpedoes_destroy_a_commander_under_the_water() {
    let mut world = world();
    let destroyers: Vec<SimId> = (0..5).map(|index| place(&mut world, "t2_destroyer", Team::Blue, (3_000, 4_600 + index * 200))).collect();
    let commander = target(&mut world, "command_rig", (3_600, 5_000));
    assert_within("five_destroyers_torpedoes_commander_under_water", ticks_to_kill(&mut world, &destroyers, commander, 2_000));
}

/// One destroyer's torpedoes destroy a submerged T1 Attack Submarine in about 25 seconds.
#[test]
fn a_destroyers_torpedoes_destroy_a_submerged_submarine() {
    let mut world = world();
    let destroyer = place(&mut world, "t2_destroyer", Team::Blue, (3_000, 5_000));
    let sub = target(&mut world, "t1_attack_sub", (3_600, 5_000));
    assert_within("destroyer_torpedoes_t1_sub_submerged", ticks_to_kill(&mut world, &[destroyer], sub, 1_000));
}

/// Two torpedo boats destroy a commander walking the seabed in about 60 seconds.
#[test]
fn two_torpedo_boats_destroy_a_commander_under_the_water() {
    let mut world = world();
    let boats = [place(&mut world, "t2_torpedo_boat", Team::Blue, (3_000, 4_700)), place(&mut world, "t2_torpedo_boat", Team::Blue, (3_000, 5_300))];
    let commander = target(&mut world, "command_rig", (3_700, 5_000));
    assert_within("two_torpedo_boats_commander_under_water", ticks_to_kill(&mut world, &boats, commander, 2_000));
}

/// The T3 SAM Launcher destroys a Gunship in about 2 seconds, and a Strategic Bomber in about 11.
#[test]
fn a_sam_launcher_destroys_a_gunship_and_a_strategic_bomber_in_time() {
    for (name, key) in [("sam_t2_gunship", "t2_gunship"), ("sam_t3_strategic_bomber", "t3_strategic_bomber")] {
        let mut world = world();
        let aircraft = target(&mut world, key, (7_600, 5_000));
        let sam = place(&mut world, "t3_sam", Team::Blue, (7_000, 5_000));
        assert_within(name, ticks_to_kill(&mut world, &[sam], aircraft, 400));
    }
}

/// The cruiser's anti-air destroys each aircraft in [volleys] in its number of volleys, two
/// seconds apart: never before that volley has started, and within its last missile's flight
/// after. Two volleys take under four seconds, and six under twelve.
#[test]
fn the_cruisers_anti_air_destroys_each_aircraft_in_its_volleys() {
    let table = targets();
    let rule = table["volleys"]["t2_cruiser"].as_table().expect("[volleys] t2_cruiser").clone();
    let two = number(&table["kill_times"]["cruiser_two_volleys_seconds_max"]);
    let six = number(&table["kill_times"]["cruiser_six_volleys_seconds_max"]);
    let mut failures = Vec::new();
    for (key, volleys) in rule {
        let volleys = u32::try_from(volleys.as_integer().expect("a count")).unwrap();
        let mut world = world();
        let cruiser = place(&mut world, "t2_cruiser", Team::Blue, (3_000, 5_000));
        let aircraft = target(&mut world, &key, (3_600, 5_000));
        let Some(ticks) = ticks_to_kill(&mut world, &[cruiser], aircraft, 400) else {
            failures.push(format!("{key} was not destroyed"));
            continue;
        };
        // The last volley starts (volleys - 1) * 2 seconds after the first; its last missile
        // leaves 0.8 seconds into it, and is under a second in the air.
        let (least, most) = ((volleys - 1) * 20, (volleys - 1) * 20 + 8 + 10);
        if !(least..=most).contains(&ticks) {
            failures.push(format!("{key}: {} s, for {volleys} volleys", seconds(ticks)));
        }
        if (volleys == 2 && seconds(ticks) >= two) || (volleys == 6 && seconds(ticks) >= six) {
            failures.push(format!("{key}: {} s, for {volleys} volleys", seconds(ticks)));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A T2 Shield Generator's dome regenerates faster than one cruiser's missiles wear it down, so
/// it holds against one for as long as it is paid for, and falls to two (2026-11-25). The
/// cruisers stand off beyond their guns' reach, so that only their missiles reach the dome.
#[test]
fn a_t2_shield_holds_against_one_cruiser_and_falls_to_two() {
    use sim_engine::sim::components::Shield;
    for cruisers in [1, 2] {
        let mut world = world();
        let generator = place(&mut world, "shield_generator", Team::Red, (SHORE_X + 1_400, 5_000));
        place(&mut world, "power_generator", Team::Red, (SHORE_X + 1_400, 5_080));
        for index in 0..cruisers {
            place(&mut world, "t2_cruiser", Team::Blue, (SHORE_X - 600, 4_900 + index * 200));
        }
        let entity = world.unit_entity(generator).unwrap();
        let mut fell = None;
        for tick in 0..3_000 {
            world.advance_tick(&[]);
            if !world.ecs.get::<&Shield>(entity).unwrap().is_up() {
                fell = Some(tick);
                break;
            }
        }
        match cruisers {
            1 => assert_eq!(fell, None, "one cruiser brought the dome down at tick {fell:?}"),
            _ => assert!(fell.is_some_and(|tick| tick < 900), "two cruisers did not bring the dome down within 90 s: {fell:?}"),
        }
    }
}
