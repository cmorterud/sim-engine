//! Capture (GAME_SPECIFICATION.md §5.3): builders take over enemy units and structures, in half
//! the target's build time at their build power, for its energy cost, as in the original.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{BeingCaptured, Combat, Factory, SimId, Team, UnitStats};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::SimWorld;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn world() -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(1_000_000);
        }
        economy.mass = Res::from_num(500_000);
        economy.energy = Res::from_num(500_000);
    }
    world
}

fn team_of(world: &SimWorld, id: SimId) -> Team {
    world.ecs.get::<&UnitStats>(world.unit_entity(id).unwrap()).unwrap().team
}

fn engineer(world: &mut SimWorld, at: Vec2Fp) -> SimId {
    world.spawn_mobile(world.blueprints.id("t1_engineer"), Team::Blue, at)
}

/// Runs until the unit changes hands, and returns how many ticks that took.
fn ticks_to_capture(world: &mut SimWorld, target: SimId, limit: u32) -> Option<u32> {
    (0..limit).find(|_| {
        world.advance_tick(&[]);
        team_of(world, target) == Team::Blue
    })
}

#[test]
fn an_engineer_captures_an_enemy_structure_in_half_its_build_time_for_its_energy() {
    let mut world = world();
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 60, 50).unwrap();
    let engineer = engineer(&mut world, point(1_150, 1_010));
    let energy = world.economies[0].energy;
    world.advance_tick(&[SimCommand::Capture { units: vec![engineer], target: generator }]);
    // A generator's 125 build time at an engineer's 5 build power is 25 seconds, and half that 12.5.
    let took = ticks_to_capture(&mut world, generator, 200).expect("it should change hands");
    assert!((120..=130).contains(&took), "it took {took} ticks");
    // It cost the generator's 750 energy, less what came in meanwhile.
    let spent = energy - world.economies[0].energy + world.economies[0].energy_income * i64::from(took + 1);
    assert!((spent - Res::from_num(750)).abs() < Res::from_num(10), "it cost {spent}");
    // Now Blue's, its income is Blue's.
    world.advance_tick(&[]);
    assert_eq!(world.economies[0].energy_income, Res::from_num(2));
    assert_eq!(world.economies[1].energy_income, Res::ZERO);
}

#[test]
fn builders_working_together_capture_faster_and_stopping_starts_over() {
    let mut world = world();
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 60, 50).unwrap();
    let (first, second) = (engineer(&mut world, point(1_150, 1_010)), engineer(&mut world, point(1_150, 1_050)));
    world.advance_tick(&[SimCommand::Capture { units: vec![first, second], target: generator }]);
    let took = ticks_to_capture(&mut world, generator, 200).expect("two engineers should capture it");
    assert!((58..=66).contains(&took), "twice the build power, half the time: {took} ticks");

    let mut world = self::world();
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 60, 50).unwrap();
    let engineer = engineer(&mut world, point(1_150, 1_010));
    world.advance_tick(&[SimCommand::Capture { units: vec![engineer], target: generator }]);
    for _ in 0..60 {
        world.advance_tick(&[]);
    }
    let entity = world.unit_entity(generator).unwrap();
    assert!(world.ecs.get::<&BeingCaptured>(entity).is_ok_and(|capture| capture.progress > Res::from_num(0.3)));
    world.advance_tick(&[SimCommand::Stop { units: vec![engineer] }]);
    world.advance_tick(&[]);
    assert!(world.ecs.get::<&BeingCaptured>(entity).is_err(), "a capture that stops starts over");
    assert_eq!(team_of(&world, generator), Team::Red);
}

#[test]
fn a_captured_unit_changes_sides_with_nothing_to_do_and_its_old_enemies_stop_shooting_it() {
    let mut world = world();
    let factory = world.spawn_finished_structure(world.blueprints.id("land_factory_t1"), Team::Red, 60, 50).unwrap();
    let bot = world.blueprints.id("t1_light_bot");
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: bot, count: 5 }]);
    // A Blue tank shooting the factory, and the engineer that takes it.
    let tank = world.spawn_mobile(world.blueprints.id("t2_heavy_tank"), Team::Blue, point(1_400, 1_030));
    world.advance_tick(&[SimCommand::Attack { units: vec![tank], target: factory }]);
    let engineer = engineer(&mut world, point(1_120, 1_120));
    world.ecs.get::<&mut sim_engine::sim::components::Builder>(world.unit_entity(engineer).unwrap()).unwrap().power = fp_int(500);
    world.advance_tick(&[SimCommand::Capture { units: vec![engineer], target: factory }]);
    ticks_to_capture(&mut world, factory, 100).expect("it should change hands");
    let entity = world.unit_entity(factory).unwrap();
    assert!(world.ecs.get::<&Factory>(entity).unwrap().queue.is_empty(), "its old owner's queue is gone");
    let combat = *world.ecs.get::<&Combat>(world.unit_entity(tank).unwrap()).unwrap();
    assert_ne!(combat.target, Some(factory), "the tank no longer shoots its own side's factory");
    assert_ne!(combat.forced_target, Some(factory));
    let health = world.unit_health(factory).unwrap();
    for _ in 0..30 {
        world.advance_tick(&[]);
    }
    assert_eq!(world.unit_health(factory), Some(health), "nothing hits it now");
}

#[test]
fn commanders_and_unfinished_structures_cannot_be_captured() {
    let mut world = world();
    let commander = world.spawn_mobile(world.blueprints.id("acu"), Team::Red, point(1_200, 1_000));
    world.ecs.get::<&mut Combat>(world.unit_entity(commander).unwrap()).unwrap().range = Fp::ZERO;
    let engineer = engineer(&mut world, point(1_100, 1_000));
    world.advance_tick(&[SimCommand::Capture { units: vec![engineer], target: commander }]);
    for _ in 0..600 {
        world.advance_tick(&[]);
    }
    assert_eq!(team_of(&world, commander), Team::Red);
    let tasks = world.ecs.get::<&sim_engine::sim::components::Builder>(world.unit_entity(engineer).unwrap()).unwrap().tasks.clone();
    assert!(tasks.is_empty(), "the engineer gives up on a commander: {tasks:?}");

    // Nor can a structure still being built.
    let site = world.blueprints.id("power_generator");
    let red_engineer = world.spawn_mobile(world.blueprints.id("t1_engineer"), Team::Red, point(1_500, 1_000));
    world.advance_tick(&[SimCommand::Build { units: vec![red_engineer], blueprint: site, cell_x: 76, cell_y: 50, queued: false }]);
    for _ in 0..30 {
        world.advance_tick(&[]);
    }
    let unfinished = world.units.iter().map(|unit| unit.0).find(|&id| world.ecs.get::<&sim_engine::sim::components::Construction>(world.unit_entity(id).unwrap()).is_ok()).expect("a site");
    world.advance_tick(&[SimCommand::Capture { units: vec![engineer], target: unfinished }]);
    world.advance_tick(&[]);
    assert!(world.ecs.get::<&BeingCaptured>(world.unit_entity(unfinished).unwrap()).is_err());
}
