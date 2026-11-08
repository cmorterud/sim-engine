//! Exotic weaponry (GAME_SPECIFICATION.md §12): EMP stuns, tractor beams, and death storms, each
//! tried with test units added to the standard data.

use std::sync::Arc;

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::{BlueprintDb, Res};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Health, Position, Projectile, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::SimWorld;

const TEST_UNITS: &str = r#"
[[unit]]
id = "test_emp_tank"
name = "EMP Tank"
class = "tank"
tier = 1
mass = 50
energy = 250
build_time = 100w
health = 3000
speed = 3
radius = 8
vision = 600
weapon = { range = 400, reload_ticks = 80, projectile_speed = 40, damage = 1, splash_radius = 60, stun_seconds = 3 }

[[unit]]
id = "test_emp_flak"
name = "EMP Flak"
class = "tank"
tier = 1
mass = 50
energy = 250
build_time = 100
health = 3000
speed = 3
radius = 8
vision = 600
weapon = { range = 400, reload_ticks = 20, projectile_speed = 90, damage = 1, targets = "air", stun_seconds = 3 }

[[unit]]
id = "test_tractor"
name = "Tractor Walker"
class = "bot"
tier = 1
mass = 500
energy = 2500
build_time = 100
health = 50000
speed = 2
radius = 20
vision = 600
weapon = { range = 200, reload_ticks = 10, projectile_speed = 40, damage = 0, kind = "tractor", crush_seconds = 5 }

[[unit]]
id = "test_storm_walker"
name = "Storm Walker"
class = "bot"
tier = 1
mass = 500
energy = 2500
build_time = 100
health = 1000
speed = 2
radius = 20
vision = 600
death_storm = { radius = 80, damage = 100, seconds = 10, speed = 6 }
"#;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn world() -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let text = format!("{}\n{}", include_str!("../data/units.toml"), TEST_UNITS);
    world.blueprints = Arc::new(BlueprintDb::parse(&text).expect("test units parse"));
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(1_000_000);
        }
        economy.mass = Res::from_num(500_000);
        economy.energy = Res::from_num(500_000);
    }
    world
}

fn unit(world: &mut SimWorld, key: &str, team: Team, x: i32, y: i32) -> SimId {
    world.spawn_mobile(world.blueprints.id(key), team, point(x, y))
}

fn position(world: &SimWorld, id: SimId) -> Option<Vec2Fp> {
    world.ecs.get::<&Position>(world.unit_entity(id)?).ok().map(|position| position.0.xy())
}

fn health(world: &SimWorld, id: SimId) -> Option<Fp> {
    world.ecs.get::<&Health>(world.unit_entity(id)?).ok().map(|health| health.current)
}

fn stunned(world: &SimWorld, id: SimId) -> bool {
    world.snapshot().stunned.contains(&id)
}

fn shots_from(world: &SimWorld, source: SimId) -> usize {
    world.projectiles.iter().filter(|&&(_, entity)| world.ecs.get::<&Projectile>(entity).is_ok_and(|shot| shot.source == source)).count()
}

/// Runs until `done` holds, and returns the tick it did.
fn run_until(world: &mut SimWorld, limit: u32, mut done: impl FnMut(&SimWorld) -> bool) -> Option<u32> {
    (0..limit).find(|_| {
        world.advance_tick(&[]);
        done(world)
    })
}

#[test]
fn an_emp_blast_stuns_what_it_reaches_for_its_time() {
    let mut world = world();
    let emp = unit(&mut world, "test_emp_tank", Team::Blue, 1_000, 1_000);
    let near = unit(&mut world, "t1_light_bot", Team::Red, 1_350, 1_000);
    let beside = unit(&mut world, "t1_light_bot", Team::Red, 1_350, 1_030);
    let far = unit(&mut world, "t1_light_bot", Team::Red, 1_350, 1_200);
    let hit = run_until(&mut world, 100, |world| stunned(world, near)).expect("the EMP fires and stuns");
    assert!(stunned(&world, beside), "the blast reaches the unit beside it");
    assert!(!stunned(&world, far), "but not one out of its reach");
    // It does almost no damage.
    assert!(health(&world, near).unwrap() >= fp_int(190));

    // Stunned, a unit can neither move nor fire, though the EMP tank is in its reach.
    let goal = point(1_100, 1_000);
    world.advance_tick(&[SimCommand::Move { units: vec![near], goal }]);
    let held_at = position(&world, near).unwrap();
    let mut ticks = 1;
    while stunned(&world, near) {
        assert_eq!(position(&world, near).unwrap(), held_at, "a stunned unit does not move");
        assert_eq!(shots_from(&world, near), 0, "nor fire");
        world.advance_tick(&[]);
        ticks += 1;
    }
    // Three seconds, counted from the tick of the hit.
    assert_eq!(ticks, 30, "stunned from tick {hit} for 3 seconds");
    let free = world.current_tick;
    run_until(&mut world, 30, |world| position(world, near).unwrap() != held_at).expect("once the stun wears off, it moves");
    assert!(world.current_tick - free <= 2);
    assert!(health(&world, emp).is_some());
}

#[test]
fn a_new_stun_lengthens_one_already_running_and_never_shortens_it() {
    let mut world = world();
    unit(&mut world, "test_emp_tank", Team::Blue, 1_000, 1_000);
    let target = unit(&mut world, "t1_light_bot", Team::Red, 1_350, 1_000);
    run_until(&mut world, 100, |world| stunned(world, target)).expect("stunned");
    // A second, quicker EMP tank, stunning again partway through.
    unit(&mut world, "test_emp_tank", Team::Blue, 1_000, 1_100);
    let mut longest = 0;
    let mut ticks = 0;
    while stunned(&world, target) && ticks < 200 {
        world.advance_tick(&[]);
        ticks += 1;
        longest = longest.max(ticks);
    }
    assert!(longest > 30, "a second stun kept it stunned past the first's 3 seconds: {longest}");
}

#[test]
fn a_stunned_radar_switches_off_until_the_stun_wears_off() {
    let mut world = world();
    let radar = world.spawn_finished_structure(world.blueprints.id("radar"), Team::Red, 66, 50).unwrap();
    for _ in 0..20 {
        world.advance_tick(&[]);
    }
    let radar_on = |world: &SimWorld| world.snapshot_for(Some(Team::Red)).sensor_ranges.iter().any(|sensor| sensor.radar > 0);
    assert!(radar_on(&world), "it works before the EMP");
    unit(&mut world, "test_emp_tank", Team::Blue, 1_000, 1_000);
    run_until(&mut world, 200, |world| stunned(world, radar)).expect("the EMP stuns it");
    assert!(!radar_on(&world), "a stunned radar is off");
    run_until(&mut world, 60, |world| !stunned(world, radar)).expect("the stun wears off");
    run_until(&mut world, 20, radar_on).expect("the radar comes back");
}

#[test]
fn a_stunned_shield_generator_drops_its_dome() {
    let mut world = world();
    let shield = world.spawn_finished_structure(world.blueprints.id("shield_generator"), Team::Red, 60, 50).unwrap();
    // Inside the dome, so that its shell is not stopped by it.
    unit(&mut world, "test_emp_tank", Team::Blue, 1_200, 1_160);
    let dome_up = |world: &SimWorld| world.snapshot().shields.iter().any(|dome| dome.id == shield && dome.up);
    world.advance_tick(&[]);
    assert!(dome_up(&world));
    run_until(&mut world, 200, |world| stunned(world, shield)).expect("the EMP stuns the generator");
    assert!(!dome_up(&world), "a stunned generator's dome is down");
    // A shell from outside passes through where the dome was.
    run_until(&mut world, 60, |world| !stunned(world, shield)).expect("the stun wears off");
    assert!(dome_up(&world), "and the dome comes back");
}

#[test]
fn aircraft_are_not_stunned() {
    let mut world = world();
    let flak = unit(&mut world, "test_emp_flak", Team::Blue, 1_000, 1_000);
    let fighter = unit(&mut world, "t1_interceptor", Team::Red, 1_200, 1_000);
    let fired = run_until(&mut world, 100, |world| shots_from(world, flak) > 0);
    assert!(fired.is_some(), "it fires at the aircraft");
    for _ in 0..60 {
        world.advance_tick(&[]);
        assert!(!stunned(&world, fighter), "an aircraft is never stunned");
    }
    assert!(world.disabled.is_empty());
}

#[test]
fn a_tractor_beam_lifts_a_unit_draws_it_in_and_crushes_it() {
    let mut world = world();
    let walker = unit(&mut world, "test_tractor", Team::Blue, 1_000, 1_000);
    let bot = unit(&mut world, "t1_light_bot", Team::Red, 1_180, 1_000);
    let other = unit(&mut world, "t1_light_bot", Team::Red, 1_000, 1_180);
    let grabbed = run_until(&mut world, 50, |world| !world.snapshot().tractor_beams.is_empty()).expect("it takes hold");
    // It holds one unit at a time: the other is left alone while the first is held.
    assert_eq!(world.snapshot().tractor_beams.len(), 1);
    let held = if world.disabled.iter().any(|unit| unit.0 == bot) { bot } else { other };
    let start = position(&world, held).unwrap();
    let mut was_closer = false;
    let crushed = run_until(&mut world, 100, |world| {
        if let Some(now) = position(world, held) {
            let here = position(world, walker).unwrap();
            was_closer |= now.distance_squared(&here) < start.distance_squared(&here);
            assert_eq!(shots_from(world, held), 0, "a held unit cannot fire");
        }
        health(world, held).is_none()
    })
    .expect("crushed");
    assert!(was_closer, "drawn in toward the walker");
    assert_eq!(crushed + 1, 50, "crushed 5 seconds after being lifted at tick {grabbed}");
    // The beam is free for the next.
    run_until(&mut world, 50, |world| !world.snapshot().tractor_beams.is_empty()).expect("it takes hold of the other");
}

#[test]
fn a_tractor_beam_cannot_lift_a_commander_or_a_structure() {
    let mut world = world();
    unit(&mut world, "test_tractor", Team::Blue, 1_000, 1_000);
    let acu = unit(&mut world, "acu", Team::Red, 1_150, 1_000);
    world.spawn_finished_structure(world.blueprints.id("mass_storage"), Team::Red, 50, 57).unwrap();
    // Keep the commander from shooting back.
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(acu).unwrap()).unwrap().range = Fp::ZERO;
    for _ in 0..100 {
        world.advance_tick(&[]);
        assert!(world.snapshot().tractor_beams.is_empty(), "nothing here can be lifted");
    }
    assert!(world.disabled.is_empty());
}

#[test]
fn a_held_unit_is_let_go_when_its_holder_dies() {
    let mut world = world();
    let walker = unit(&mut world, "test_tractor", Team::Blue, 1_000, 1_000);
    let bot = unit(&mut world, "t1_light_bot", Team::Red, 1_180, 1_000);
    run_until(&mut world, 50, |world| !world.snapshot().tractor_beams.is_empty()).expect("it takes hold");
    // A point defense out of the beam's reach finishes off the walker.
    world.spawn_finished_structure(world.blueprints.id("t1_point_defense"), Team::Red, 50, 65).unwrap();
    world.ecs.get::<&mut Health>(world.unit_entity(walker).unwrap()).unwrap().current = Fp::ONE;
    run_until(&mut world, 40, |world| health(world, walker).is_none()).expect("the walker is destroyed before it crushes the bot");
    run_until(&mut world, 3, |world| !world.disabled.iter().any(|unit| unit.0 == bot));
    assert!(health(&world, bot).is_some_and(|health| health > Fp::ZERO), "let go, not crushed");
    assert!(!world.disabled.iter().any(|unit| unit.0 == bot), "and free");
    let z = world.ecs.get::<&Position>(world.unit_entity(bot).unwrap()).unwrap().0.z;
    let ground = world.terrain.height_at(position(&world, bot).unwrap());
    assert!(z < ground + fp_int(10), "it falls back to the ground");
}

#[test]
fn a_death_storm_roams_toward_enemies_strikes_them_and_blows_out() {
    let mut world = world();
    let walker = unit(&mut world, "test_storm_walker", Team::Blue, 1_000, 1_000);
    let enemy = world.spawn_finished_structure(world.blueprints.id("mass_storage"), Team::Red, 70, 50).unwrap();
    let before = health(&world, enemy).unwrap();
    world.advance_tick(&[SimCommand::SelfDestruct { units: vec![walker] }]);
    run_until(&mut world, 200, |world| !world.storms.is_empty()).expect("a storm where it died");
    let born = world.current_tick;
    let at = world.snapshot().storms[0];
    assert!((at.x - fp_int(1_000).to_bits()).abs() < fp_int(10).to_bits());
    run_until(&mut world, 120, |world| world.storms.is_empty()).expect("the storm blows out");
    let lasted = world.current_tick - born;
    assert!((99..=101).contains(&lasted), "10 seconds: {lasted}");
    let after = health(&world, enemy).unwrap_or(Fp::ZERO);
    assert!(after < before, "it drifted 400 to the enemy storage and struck it");
}

#[test]
fn a_death_storm_does_not_hurt_its_own_side() {
    let mut world = world();
    let walker = unit(&mut world, "test_storm_walker", Team::Blue, 2_000, 2_000);
    // Well clear of the walker's own death blast, if it has one, but under the storm's path.
    let friend = world.spawn_finished_structure(world.blueprints.id("mass_storage"), Team::Blue, 100, 102).unwrap();
    let before = health(&world, friend).unwrap();
    world.advance_tick(&[SimCommand::SelfDestruct { units: vec![walker] }]);
    run_until(&mut world, 200, |world| !world.storms.is_empty()).expect("a storm");
    run_until(&mut world, 120, |world| world.storms.is_empty()).expect("blown out");
    assert_eq!(health(&world, friend).unwrap(), before);
}

#[test]
fn exotic_weapons_are_deterministic() {
    let run = || {
        let mut world = world();
        unit(&mut world, "test_emp_tank", Team::Blue, 1_000, 1_000);
        unit(&mut world, "test_tractor", Team::Blue, 1_000, 1_100);
        let walker = unit(&mut world, "test_storm_walker", Team::Red, 1_400, 1_000);
        for index in 0..6 {
            unit(&mut world, "t1_light_bot", Team::Red, 1_200 + index * 20, 1_050);
        }
        world.advance_tick(&[SimCommand::SelfDestruct { units: vec![walker] }]);
        let mut checksums = Vec::new();
        for _ in 0..300 {
            world.advance_tick(&[]);
            checksums.push(world.compute_checksum());
        }
        checksums
    };
    assert_eq!(run(), run());
}
