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

/// The T2 Flak Cannon destroys each air transport hanging in the air in about the design owner's
/// time (2026-11-27): a Light Air Transport in about 4 seconds, an Air Transport in about 11, and a
/// Heavy Air Transport, through its shield, in about 55.
#[test]
fn the_flak_cannon_destroys_each_air_transport_in_time() {
    for (name, key) in [("flak_t1_light_transport", "t1_light_transport"), ("flak_t2_transport", "t2_transport"), ("flak_t3_heavy_transport", "t3_heavy_transport")] {
        let mut world = world();
        let aircraft = target(&mut world, key, (7_600, 5_000));
        let flak = place(&mut world, "t2_flak", Team::Blue, (7_000, 5_000));
        assert_within(name, ticks_to_kill(&mut world, &[flak], aircraft, 1_000));
    }
}

/// Aircraft against aircraft hanging in the air, and the heavy gunship against a commander on
/// land, each alone, in about the design owner's times (2026-11-26).
#[test]
fn aircraft_destroy_their_targets_in_time() {
    for (name, shooter, prey) in [
        ("t3_fighter_strategic_bomber", "t3_air_superiority", "t3_strategic_bomber"),
        ("t3_fighter_t3_fighter", "t3_air_superiority", "t3_air_superiority"),
        ("t3_fighter_heavy_gunship", "t3_air_superiority", "t3_heavy_gunship"),
        ("fighter_bomber_interceptor", "t2_fighter_bomber", "t1_interceptor"),
        ("heavy_gunship_commander_on_land", "t3_heavy_gunship", "command_rig"),
    ] {
        let mut world = world();
        let target = target(&mut world, prey, (7_600, 5_000));
        let plane = place(&mut world, shooter, Team::Blue, (6_400, 5_000));
        world.advance_tick(&[SimCommand::Attack { units: vec![plane], target }]);
        assert_within(name, ticks_to_kill(&mut world, &[plane], target, 2_000));
    }
}

/// A frigate's anti-air destroys one T1 bomber hanging in the air in about 21 seconds
/// (2026-11-26, the design owner's number).
#[test]
fn a_frigates_anti_air_destroys_a_t1_bomber_in_time() {
    let mut world = world();
    let bomber = target(&mut world, "t1_bomber", (3_700, 5_000));
    let frigate = place(&mut world, "t1_frigate", Team::Blue, (3_000, 5_000));
    assert_within("frigate_anti_air_t1_bomber", ticks_to_kill(&mut world, &[frigate], bomber, 600));
}

/// Mobile Anti-Air destroys one T1 bomber hanging in the air in about 24 seconds (2026-11-27, the
/// design owner's number), which sets its gun and, through the escort rule, the bomber's cost.
#[test]
fn mobile_anti_air_destroys_a_t1_bomber_in_time() {
    let mut world = world();
    let bomber = target(&mut world, "t1_bomber", (SHORE_X + 1_400, 5_000));
    let anti_air = place(&mut world, "t1_mobile_aa", Team::Blue, (SHORE_X + 1_000, 5_000));
    assert_within("mobile_aa_t1_bomber", ticks_to_kill(&mut world, &[anti_air], bomber, 600));
}

/// Six T1 bombers sink a frigate that fires back in two passes and lose none (2026-11-26, the
/// design owner's rule): it outlives one pass's six bombs and not two passes' twelve.
#[test]
fn six_t1_bombers_sink_a_frigate_in_two_passes_and_lose_none() {
    let mut world = world();
    let frigate = place(&mut world, "t1_frigate", Team::Blue, (3_000, 5_000));
    let bombers: Vec<SimId> = (0..6).map(|index| place(&mut world, "t1_bomber", Team::Red, (4_600, 4_850 + index * 60))).collect();
    world.advance_tick(&[SimCommand::Attack { units: bombers.clone(), target: frigate }]);
    let (mut seen, mut bombs, mut sunk) = (Vec::<SimId>::new(), 0usize, false);
    for _ in 0..1_200 {
        world.advance_tick(&[]);
        let now: Vec<SimId> = world.projectiles.iter().filter(|&&(_, entity)| world.ecs.get::<&Projectile>(entity).is_ok_and(|projectile| bombers.contains(&projectile.source))).map(|&(id, _)| id).collect();
        bombs += now.iter().filter(|id| !seen.contains(id)).count();
        seen = now;
        if world.unit_entity(frigate).is_none() {
            sunk = true;
            break;
        }
    }
    assert!(sunk, "six T1 bombers did not sink the frigate ({bombs} bombs)");
    let passes = number(&targets()["kill_times"]["six_t1_bombers_sink_a_frigate_passes"]) as usize;
    assert!(bombs > 6 * (passes - 1) && bombs <= 6 * passes, "the frigate sank to {bombs} bombs, not in {passes} passes of six");
    let lost = bombers.iter().filter(|&&bomber| world.unit_entity(bomber).is_none()).count();
    assert_eq!(lost, 0, "the frigate shot down {lost} bombers");
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

/// Six T1 Attack Submarines, about a destroyer's cost, sink one in about 33 seconds, as the
/// design owner has it (2026-11-25): it sinks one of them and damages a second, and the other
/// four come through untouched. Both sides attack-move onto each other.
#[test]
fn six_submarines_sink_a_destroyer_and_lose_one() {
    let mut world = world();
    let destroyer = place(&mut world, "t2_destroyer", Team::Blue, (3_000, 5_000));
    let subs: Vec<SimId> = (0..6).map(|index| place(&mut world, "t1_attack_sub", Team::Red, (4_400 + (index % 2) * 60, 4_850 + (index / 2) * 100))).collect();
    let full = world.unit_health(subs[0]).unwrap();
    world.advance_tick(&[
        SimCommand::AttackMove { units: subs.clone(), goal: point(3_000, 5_000) },
        SimCommand::AttackMove { units: vec![destroyer], goal: point(4_400, 5_000) },
    ]);
    let mut first = None;
    let mut sunk = None;
    for tick in 0..1_000 {
        world.advance_tick(&[]);
        first = first.or((!world.projectiles.is_empty()).then_some(tick));
        if world.unit_entity(destroyer).is_none() {
            sunk = first.map(|first| tick - first);
            break;
        }
    }
    assert_within("six_t1_subs_sink_a_destroyer", sunk);
    let health: Vec<Option<Fp>> = subs.iter().map(|&sub| world.unit_health(sub)).collect();
    let lost = health.iter().filter(|health| health.is_none()).count();
    let damaged = health.iter().filter(|health| health.is_some_and(|health| health < full)).count();
    assert_eq!((lost, damaged), (1, 1), "the submarines' health after the fight: {health:?}");
}

/// A battleship parked between its guns' reach and a T2 Artillery Installation's destroys a T2
/// artillery position, a Shield Generator's dome over four installations, in about 45 seconds
/// from its first damage, and is never fired on; its first volley takes the dome down, and no
/// less does (2026-11-25, `[domains.cells]`).
#[test]
fn a_battleship_outranges_and_destroys_shielded_t2_artillery() {
    use sim_engine::sim::components::Shield;
    let table = targets();
    let cells = &table["domains"]["cells"];
    let range = |key: &str| {
        let range = cells[key].as_array().unwrap_or_else(|| panic!("[domains.cells] {key} is a range"));
        (number(&range[0]), number(&range[1]))
    };
    let db = BlueprintDb::standard();
    let reach = |key: &str| db.get(db.id(key)).weapon.expect("a gun").range.to_num::<i32>();
    let (guns, artillery) = (reach("t3_battleship"), reach("t2_artillery"));
    let (least, most) = range("battleship_over_t2_artillery_reach");
    let over = f64::from(guns) / f64::from(artillery);
    assert!((least..=most).contains(&over), "the battleship outranges the artillery by {over:.2}");

    let mut world = world();
    // The dome's generator's south-west cell; two installations side by side against its seaward
    // side, one off its north side, and one behind it.
    let (x, y) = (280, 255);
    let generator = world.spawn_finished_structure(db.id("shield_generator"), Team::Red, x, y).expect("room for the generator");
    let mut structures = vec![generator];
    structures.extend([(x - 1, y), (x - 1, y + 1), (x + 1, y + 5), (x + 5, y + 1)].map(|(x, y)| world.spawn_finished_structure(db.id("t2_artillery"), Team::Red, x, y).expect("room for the artillery")));
    let middle = (x as i32 * 20 + 30, y as i32 * 20 + 30);
    let battleship = place(&mut world, "t3_battleship", Team::Blue, (middle.0 - (guns + artillery) / 2, middle.1));
    let full = world.unit_health(battleship).unwrap();
    let dome = world.unit_entity(generator).unwrap();

    let mut shells: Vec<SimId> = Vec::new();
    let (mut landed, mut first_damage, mut volley_landed, mut fired_on) = (0, None, None, false);
    let mut cleared = None;
    for tick in 0..2_000 {
        world.advance_tick(&[]);
        let from = |team: Team| -> Vec<SimId> {
            world.projectiles.iter().filter(|&&(_, entity)| world.ecs.get::<&Projectile>(entity).is_ok_and(|projectile| projectile.source_team == team)).map(|&(id, _)| id).collect()
        };
        let flying = from(Team::Blue);
        landed += shells.iter().filter(|shell| !flying.contains(shell)).count();
        shells = flying;
        fired_on |= !from(Team::Red).is_empty();
        let dome_up = world.ecs.get::<&Shield>(dome).is_ok_and(|shield| shield.is_up());
        if first_damage.is_none() && world.ecs.get::<&Shield>(dome).is_ok_and(|shield| shield.health < shield.max_health) {
            first_damage = Some(tick);
        }
        if volley_landed.is_none() && !dome_up {
            volley_landed = Some(landed);
        }
        if structures.iter().all(|&id| world.unit_entity(id).is_none()) {
            cleared = first_damage.map(|first| tick - first);
            break;
        }
    }
    let turrets = db.get(db.id("t3_battleship")).extra_weapons.iter().chain(&db.get(db.id("t3_battleship")).weapon).filter(|weapon| weapon.targets == Targets::Ground).count();
    assert_eq!(volley_landed, Some(turrets), "shells landed when the dome went down");
    assert!(!fired_on, "the artillery fired at the battleship");
    // Not hit: its health is never under full. Destroying the position can earn it a veterancy
    // rank, which adds health.
    assert!(world.unit_health(battleship).is_some_and(|health| health >= full), "the battleship was hit");
    let (least, most) = range("battleship_breaks_shielded_t2_artillery");
    let took = seconds(cleared.expect("the position was not destroyed"));
    println!("battleship_breaks_shielded_t2_artillery: {took} s");
    assert!((least..=most).contains(&took), "the position fell in {took} s, not within {least} to {most}");
}

/// The battleship's shells, in flight and landed: `landed` counts those gone since the last call.
struct Shells {
    from: SimId,
    flying: Vec<SimId>,
    landed: usize,
}

impl Shells {
    fn new(from: SimId) -> Self {
        Self { from, flying: Vec::new(), landed: 0 }
    }

    /// After a tick: notes the shells that have landed since the last.
    fn update(&mut self, world: &SimWorld) {
        let flying: Vec<SimId> = world.projectiles.iter().filter(|&&(_, entity)| world.ecs.get::<&Projectile>(entity).is_ok_and(|projectile| projectile.source == self.from)).map(|&(id, _)| id).collect();
        self.landed += self.flying.iter().filter(|shell| !flying.contains(shell)).count();
        self.flying = flying;
    }
}

/// A battleship's guns that fire together as one broadside: its main gun and the extra weapons in
/// its broadside.
fn broadside_guns(unit: &Blueprint) -> usize {
    1 + unit.extra_weapons.iter().filter(|weapon| weapon.broadside).count()
}

/// Each [broadsides] rule holds in the sim: a battleship offshore destroys each target in its
/// number of broadsides, every shell landing (an extractor in one, a commander in two, a
/// battlecruiser out at sea in three), and a commander within [kill_times]
/// battleship_commander_on_land. Its guns fire as one volley, a ripple apart.
#[test]
fn a_battleships_broadsides_destroy_an_extractor_in_one_and_a_commander_in_two() {
    let table = targets();
    let db = BlueprintDb::standard();
    let battleship_def = db.get(db.id("t3_battleship"));
    let guns = broadside_guns(battleship_def);
    assert_eq!(guns, 3, "the battleship's three turrets fire as one broadside");
    let rules = table["broadsides"]["t3_battleship"].as_table().expect("[broadsides] t3_battleship").clone();
    assert!(rules.len() >= 2);
    for (key, broadsides) in rules {
        let broadsides = usize::try_from(broadsides.as_integer().expect("a count")).unwrap();
        let mut world = world();
        let battleship = place(&mut world, "t3_battleship", Team::Blue, (SHORE_X - 600, 5_000));
        // A ship on the water, out at sea from it; anything else ashore.
        let at = if db.get(db.id(&key)).move_class() == sim_engine::sim::components::MoveClass::Naval { (SHORE_X - 2_000, 5_000) } else { (SHORE_X + 800, 5_000) };
        if db.get(db.id(&key)).requires_deposit {
            world.deposits.push((at.0 as u32 / 20, at.1 as u32 / 20));
        }
        let victim = target(&mut world, &key, at);
        let mut shells = Shells::new(battleship);
        let (mut first, mut took) = (None, None);
        for tick in 0..1_000 {
            world.advance_tick(&[]);
            shells.update(&world);
            first = first.or((!shells.flying.is_empty()).then_some(tick));
            if world.unit_entity(victim).is_none() {
                took = first.map(|first| tick - first);
                break;
            }
        }
        let took = took.unwrap_or_else(|| panic!("{key} was not destroyed"));
        let needed = shells.landed.div_ceil(guns);
        assert_eq!(needed, broadsides, "{key} fell to {} shells, {needed} broadsides, in {} s", shells.landed, seconds(took));
        if key == "command_rig" {
            assert_within("battleship_commander_on_land", Some(took));
        }
    }
}

/// One battleship wears down an unassisted T3 Heavy Shield Generator's dome in about 45 seconds
/// from its first damage, a broadside at a time that the dome cannot heal in time, and the
/// broadside that breaks the dome destroys the generator (2026-11-25).
#[test]
fn a_battleship_breaks_a_t3_shield_and_the_same_broadside_destroys_its_generator() {
    use sim_engine::sim::components::Shield;
    let db = BlueprintDb::standard();
    let reload = db.get(db.id("t3_battleship")).weapon.expect("a main gun").reload_ticks;
    let mut world = world();
    let generator = place(&mut world, "t3_heavy_shield", Team::Red, (SHORE_X + 600, 5_000));
    let battleship = place(&mut world, "t3_battleship", Team::Blue, (SHORE_X - 1_400, 5_000));
    let dome = world.unit_entity(generator).unwrap();
    let mut shells = Shells::new(battleship);
    let (mut first_damage, mut fell, mut destroyed) = (None, None, None);
    for tick in 0..2_000 {
        world.advance_tick(&[]);
        shells.update(&world);
        if world.unit_entity(generator).is_none() {
            destroyed = Some(tick);
            break;
        }
        let shield = *world.ecs.get::<&Shield>(dome).unwrap();
        if first_damage.is_none() && shield.health < shield.max_health {
            first_damage = Some(tick);
        }
        if fell.is_none() && !shield.is_up() {
            fell = Some(tick);
        }
    }
    let (first_damage, fell, destroyed) = (first_damage.expect("the dome was hit"), fell.expect("the dome fell"), destroyed.expect("the generator was destroyed"));
    let (least, most) = seconds_range("battleship_breaks_t3_shield");
    let took = seconds(fell - first_damage);
    println!("battleship_breaks_t3_shield: {took} s");
    assert!((least..=most).contains(&took), "the dome fell in {took} s, not within {least} to {most}");
    assert!(destroyed - fell < reload / 4, "the generator outlived its dome by {} s: the broadside that broke it should carry through", seconds(destroyed - fell));
}

/// A broadside: the battleship's three turrets fire together, a ripple apart, once each for each
/// shot of its main gun, and none on its own; the first volley waits until all three point at
/// their targets (2026-11-25).
#[test]
fn a_battleships_turrets_fire_together_a_ripple_apart() {
    use sim_engine::sim::blueprints::BROADSIDE_RIPPLE_TICKS;
    let db = BlueprintDb::standard();
    let reload = db.get(db.id("t3_battleship")).weapon.expect("a main gun").reload_ticks;
    let mut world = world();
    let battleship = place(&mut world, "t3_battleship", Team::Blue, (SHORE_X - 600, 5_000));
    target(&mut world, "command_rig", (SHORE_X + 800, 5_000));
    let (mut seen, mut fired) = (Vec::<SimId>::new(), Vec::new());
    for tick in 0..600 {
        world.advance_tick(&[]);
        for &(id, entity) in &world.projectiles {
            if !seen.contains(&id) && world.ecs.get::<&Projectile>(entity).is_ok_and(|projectile| projectile.source == battleship) {
                seen.push(id);
                fired.push(tick);
            }
        }
        if fired.len() >= 6 {
            break;
        }
    }
    assert!(fired.len() >= 6, "only {} shells in 60 s", fired.len());
    let first = fired[0];
    let ripple = BROADSIDE_RIPPLE_TICKS;
    assert_eq!(fired[..6], [first, first + ripple, first + 2 * ripple, first + reload, first + reload + ripple, first + reload + 2 * ripple], "the shells left at {fired:?}");
}

/// A battleship's shells go up in a high arc and fly just fast enough that a commander that
/// turns back each time a broadside goes up only just walks out of them from the edge of the
/// battleship's reach, and is destroyed from any nearer (2026-11-26, the design owner's ask).
/// The commander walks up and down across the line of fire, as a player keeps one moving, and
/// turns back 0.3 seconds after each broadside's first shell leaves, as a player would on seeing it.
#[test]
fn a_microed_commander_only_just_walks_out_of_a_battleships_broadsides_at_its_full_reach() {
    let reach = fp_int(2_990);
    let db = BlueprintDb::standard();
    assert!(db.get(db.id("t3_battleship")).weapon.is_some_and(|weapon| weapon.lobbed && weapon.range == reach), "the battleship lobs its shells to 2,990");
    let survives_from = |range: i32| {
        let mut world = world();
        let ship = place(&mut world, "t3_battleship", Team::Blue, (SHORE_X - 600, 5_000));
        let x = SHORE_X - 600 + range;
        let commander = place(&mut world, "command_rig", Team::Red, (x, 4_400));
        world.ecs.get::<&mut Combat>(world.unit_entity(commander).unwrap()).unwrap().enabled = false;
        let walk = |up: bool| SimCommand::Move { units: vec![commander], goal: point(x, if up { 5_600 } else { 4_400 }) };
        let (mut up, mut commands, mut flying, mut turn_at, mut fired) = (true, vec![walk(true)], Vec::<SimId>::new(), None, 0);
        for tick in 0..1_200 {
            world.advance_tick(&commands);
            commands.clear();
            let Some(at) = world.unit_position(commander) else { return (false, fired) };
            let shells: Vec<SimId> = world.projectiles.iter().filter(|&&(_, entity)| world.ecs.get::<&Projectile>(entity).is_ok_and(|shell| shell.source == ship)).map(|&(id, _)| id).collect();
            let new = shells.iter().filter(|shell| !flying.contains(shell)).count();
            if new > 0 && new == shells.len() {
                turn_at = turn_at.or(Some(tick + 3));
            }
            fired += new;
            flying = shells;
            let end = (up && at.y >= fp_int(5_550)) || (!up && at.y <= fp_int(4_450));
            if end || turn_at == Some(tick) {
                up = !up;
                commands.push(walk(up));
                turn_at = turn_at.filter(|&due| due != tick);
            }
        }
        (true, fired)
    };
    let (survived, fired) = survives_from(2_980);
    assert!(survived && fired >= 15, "from the edge of its reach the commander walked out of {fired} shells: {survived}");
    let (survived, fired) = survives_from(2_200);
    assert!(!survived, "from 2,200 the commander walked out of all {fired} shells");
}

/// `[price_list.landed_share]` (2026-11-26): each aircraft lands the share of its card's damage a
/// second that its price assumes. Anti-air is measured against a bomber flying its runs at a tank,
/// everything else against a target holding still; both are kept alive, and the share is counted
/// over two minutes from the first hit.
#[test]
fn aircraft_land_the_share_of_their_damage_their_price_assumes() {
    use sim_engine::sim::components::Health;
    let table = targets();
    let shares = table["price_list"]["landed_share"].as_table().expect("[price_list.landed_share]");
    let full = Fp::from_num(30_000);
    let per_second = |weapon: &WeaponDef| weapon.damage.to_num::<f64>() * f64::from(weapon.salvo.map_or(1, |salvo| salvo.shots)) * TICKS_PER_SECOND as f64 / f64::from(weapon.reload_ticks.max(1));
    // (the share's key, the aircraft, the target, where the target is, whether the target flies a bomber's runs)
    let cases = [
        ("t1_bomber", "t1_bomber", "t1_medium_tank", (7_600, 5_000), false),
        ("t2_fighter_bomber", "t2_fighter_bomber", "t1_medium_tank", (7_600, 5_000), false),
        ("t3_strategic_bomber", "t3_strategic_bomber", "t2_heavy_tank", (7_600, 5_000), false),
        ("t2_torpedo_bomber", "t2_torpedo_bomber", "t2_destroyer", (3_000, 5_000), false),
        ("t2_gunship", "t2_gunship", "t1_medium_tank", (7_600, 5_000), false),
        ("t3_heavy_gunship", "t3_heavy_gunship", "t2_heavy_tank", (7_600, 5_000), false),
        ("t1_interceptor", "t1_interceptor", "t1_bomber", (8_800, 5_000), true),
        ("t2_fighter_bomber_anti_air", "t2_fighter_bomber", "t1_bomber", (8_800, 5_000), true),
        ("t3_air_superiority", "t3_air_superiority", "t3_strategic_bomber", (8_800, 5_000), true),
    ];
    let mut off = Vec::new();
    for (key, shooter, prey, at, flying) in cases {
        let share = number(&shares[key]);
        let mut world = world();
        let (target, kept) = if flying {
            let tank = target(&mut world, "t2_heavy_tank", (7_600, 5_000));
            let bomber = place(&mut world, prey, Team::Red, at);
            world.advance_tick(&[SimCommand::Attack { units: vec![bomber], target: tank }]);
            for _ in 0..60 {
                world.advance_tick(&[]);
            }
            (bomber, vec![tank, bomber])
        } else {
            let target = target(&mut world, prey, at);
            (target, vec![target])
        };
        let plane = place(&mut world, shooter, Team::Blue, (at.0 - 1_200, at.1));
        world.advance_tick(&[SimCommand::Attack { units: vec![plane], target }]);
        let (mut dealt, mut ticks) = (0.0f64, 0u32);
        for _ in 0..2_400 {
            for &id in &kept {
                world.ecs.get::<&mut Health>(world.unit_entity(id).expect("kept alive")).unwrap().current = full;
            }
            world.advance_tick(&[]);
            let lost = (full - world.ecs.get::<&Health>(world.unit_entity(target).unwrap()).unwrap().current).to_num::<f64>();
            if lost > 0.0 || ticks > 0 {
                dealt += lost;
                ticks += 1;
            }
            if ticks == 1_200 {
                break;
            }
        }
        let blueprint = world.blueprints.get(world.blueprints.id(shooter));
        let listed: f64 = blueprint.weapon.iter().chain(&blueprint.extra_weapons).filter(|weapon| (weapon.targets == Targets::Air) == flying).map(per_second).sum();
        let landed = dealt / (f64::from(ticks) / TICKS_PER_SECOND as f64) / listed;
        println!("{key}: lands {landed:.2} of {listed:.1} a second ([price_list] {share:.2})");
        if (landed - share).abs() > 0.05 {
            off.push(format!("{key} lands {landed:.2} of its damage a second, and its price assumes {share:.2}"));
        }
    }
    assert!(off.is_empty(), "{off:#?}");
}
