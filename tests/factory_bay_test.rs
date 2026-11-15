//! Factories build in the open: what a factory makes stands in its bay while it is built, then
//! drives out of one of its two open ends, or for aircraft lifts off its pad, and goes on to the
//! rally point.

use sim_engine::math::fp::{fp_int, Vec2Fp};
use sim_engine::sim::blueprints::{BlueprintId, Res};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{LeavingFactory, OrderKind, Position, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::SimWorld;

fn test_world() -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        economy.base_mass_capacity = Res::from_num(100_000);
        economy.base_energy_capacity = Res::from_num(1_000_000);
        economy.mass_capacity = economy.base_mass_capacity;
        economy.energy_capacity = economy.base_energy_capacity;
        economy.mass = economy.mass_capacity;
        economy.energy = economy.energy_capacity;
    }
    world
}

fn id(world: &SimWorld, key: &str) -> BlueprintId {
    world.blueprints.id(key)
}

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn position(world: &SimWorld, unit: SimId) -> Vec2Fp {
    world.ecs.get::<&Position>(world.unit_entity(unit).unwrap()).unwrap().0.xy()
}

/// Runs until the factory's first unit appears, and returns it.
fn first_built(world: &mut SimWorld, factory: SimId) -> SimId {
    for _ in 0..2_000 {
        world.advance_tick(&[]);
        if let Some(&(unit, _)) = world.units.iter().find(|&&(unit, _)| unit != factory && world.unit_entity(unit).is_some_and(|entity| world.ecs.get::<&sim_engine::sim::components::UnitStats>(entity).is_ok_and(|stats| !stats.is_structure))) {
            return unit;
        }
    }
    panic!("the factory never finished a unit");
}

fn leaving(world: &SimWorld, unit: SimId) -> Option<LeavingFactory> {
    world.ecs.get::<&LeavingFactory>(world.unit_entity(unit).unwrap()).ok().map(|leaving| *leaving)
}

#[test]
fn a_new_bot_starts_in_the_bay_drives_out_the_end_facing_its_rally_and_goes_on() {
    let mut world = test_world();
    let factory = world.spawn_finished_structure(id(&world, "land_factory_t1"), Team::Blue, 50, 50).unwrap();
    let center = position(&world, factory);
    // North-west: the west end faces it more than the east, and the ends come before the sides.
    let rally = point(500, 900);
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: id(&world, "t1_light_bot"), count: 1 }, SimCommand::SetRally { factory, point: rally }]);
    let bot = first_built(&mut world, factory);
    assert_eq!(position(&world, bot), center, "it is built in the bay");
    let out = leaving(&world, bot).expect("it drives out first");
    assert!(out.exit.x < center.x && out.exit.y == center.y, "out of the west end, toward the rally: {:?}", out.exit);
    assert_eq!(out.rally, Some(rally));
    assert_eq!(world.unit_order(bot).unwrap().kind, OrderKind::Idle, "no orders while in the factory");
    let mut ticks = 0;
    while leaving(&world, bot).is_some() {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 60, "it should be out within a few seconds");
        let at = position(&world, bot);
        assert_eq!(at.y, center.y, "straight out along the bay");
    }
    assert_eq!(position(&world, bot), out.exit);
    assert_eq!(world.unit_order(bot).unwrap().kind, OrderKind::Move, "then on to the rally point");
}

#[test]
fn an_order_given_on_the_way_out_takes_over_from_the_rally() {
    let mut world = test_world();
    let factory = world.spawn_finished_structure(id(&world, "land_factory_t1"), Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: id(&world, "t1_light_bot"), count: 1 }, SimCommand::SetRally { factory, point: point(500, 900) }]);
    let bot = first_built(&mut world, factory);
    let elsewhere = point(4_000, 1_000);
    world.advance_tick(&[SimCommand::Move { units: vec![bot], goal: elsewhere }]);
    assert!(leaving(&world, bot).is_some(), "it finishes driving out first");
    while leaving(&world, bot).is_some() {
        world.advance_tick(&[]);
    }
    world.advance_tick(&[]);
    for _ in 0..60 {
        world.advance_tick(&[]);
    }
    assert!(position(&world, bot).x > fp_int(1_200), "it heads east for the new order, not west to the rally");
}

#[test]
fn a_new_aircraft_lifts_off_its_pad_and_flies_to_the_rally() {
    let mut world = test_world();
    let factory = world.spawn_finished_structure(id(&world, "air_factory"), Team::Blue, 50, 50).unwrap();
    let center = position(&world, factory);
    let rally = point(3_000, 1_000);
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: id(&world, "t1_air_scout"), count: 1 }, SimCommand::SetRally { factory, point: rally }]);
    let scout = first_built(&mut world, factory);
    assert_eq!(position(&world, scout), center, "it is built on the pad");
    assert!(leaving(&world, scout).is_none(), "aircraft fly straight off");
    assert_eq!(world.unit_order(scout).unwrap().kind, OrderKind::Move);
}

#[test]
fn anyone_who_sees_a_factory_sees_what_it_is_building_and_how_far_along() {
    let mut world = test_world();
    let factory = world.spawn_finished_structure(id(&world, "land_factory_t1"), Team::Blue, 50, 50).unwrap();
    let bot = id(&world, "t1_light_bot");
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: bot, count: 1 }]);
    let assembling = |world: &SimWorld| world.snapshot_for(Some(Team::Red)).units.iter().find(|unit| unit.id == factory).and_then(|unit| unit.assembling);
    let early = assembling(&world).expect("the bot shows in the bay");
    assert_eq!(early.0, bot.0);
    for _ in 0..20 {
        world.advance_tick(&[]);
    }
    let later = assembling(&world).expect("still building");
    assert!(later.1 > early.1, "it shows how far along it is: {early:?} then {later:?}");
    // An upgrade is not built in the bay.
    let mut world = test_world();
    let factory = world.spawn_finished_structure(id(&world, "land_factory_t1"), Team::Blue, 50, 50).unwrap();
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: id(&world, "land_factory_t2"), count: 1 }]);
    assert_eq!(assembling(&world), None);
}

#[test]
fn anyone_who_sees_a_factory_sees_it_upgrading_and_how_far_along() {
    let mut world = test_world();
    let factory = world.spawn_finished_structure(id(&world, "land_factory_t1"), Team::Blue, 50, 50).unwrap();
    let t2 = id(&world, "land_factory_t2");
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: t2, count: 1 }]);
    let upgrade = |world: &SimWorld| world.snapshot_for(Some(Team::Red)).units.iter().find(|unit| unit.id == factory).and_then(|unit| unit.upgrade);
    let early = upgrade(&world).expect("the upgrade shows");
    assert_eq!(early.0, t2.0);
    for _ in 0..100 {
        world.advance_tick(&[]);
    }
    assert!(upgrade(&world).expect("still upgrading").1 > early.1, "and how far along it is");
}
