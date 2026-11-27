//! Factories build in the open: what a factory makes stands in its bay while it is built, then
//! drives out of one of its two open ends, or for aircraft lifts off its pad, and goes on to the
//! rally point.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::{BlueprintId, Res};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{LeavingFactory, OrderKind, Position, SimId, Team, Turning};
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
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: id(&world, "t1_light_walker"), count: 1 }, SimCommand::SetRally { factory, point: rally }]);
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
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: id(&world, "t1_light_walker"), count: 1 }, SimCommand::SetRally { factory, point: point(500, 900) }]);
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
    let bot = id(&world, "t1_light_walker");
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

fn facing(world: &SimWorld, unit: SimId) -> Vec2Fp {
    world.ecs.get::<&Turning>(world.unit_entity(unit).unwrap()).unwrap().facing
}

/// Which way the factory's current item faces in the bay, as the renderer is told: 0 east,
/// 1 north, 2 west, 3 south.
fn bay_heading(world: &SimWorld, factory: SimId) -> u8 {
    world.snapshot_for(Some(Team::Blue)).units.iter().find(|unit| unit.id == factory).and_then(|unit| unit.assembling).expect("it is building something").2
}

#[test]
fn a_bot_in_the_bay_faces_the_open_end_nearer_its_rally_and_drives_out_that_way() {
    for (rally, heading, east) in [(point(1_400, 2_000), 0, true), (point(300, 300), 2, false)] {
        let mut world = test_world();
        let factory = world.spawn_finished_structure(id(&world, "land_factory_t1"), Team::Blue, 50, 50).unwrap();
        let center = position(&world, factory);
        world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: id(&world, "t1_light_walker"), count: 1 }, SimCommand::SetRally { factory, point: rally }]);
        // Even with the rally well to the north or south, a land bay opens only east and west.
        assert_eq!(bay_heading(&world, factory), heading, "rally {rally:?}");
        let bot = first_built(&mut world, factory);
        let out = leaving(&world, bot).expect("it drives out first");
        assert_eq!(out.exit.y, center.y);
        assert_eq!(out.exit.x > center.x, east, "out of the end it faces");
        let way = facing(&world, bot);
        assert_eq!(way.x > Fp::ZERO, east, "facing the way out: {way:?}");
        assert_eq!(way.y, Fp::ZERO);
    }
}

#[test]
fn an_aircraft_on_its_pad_faces_whichever_of_the_four_ways_is_nearest_its_rally() {
    for (rally, heading) in [(point(3_000, 1_000), 0), (point(1_000, 3_000), 1), (point(100, 900), 2), (point(1_100, 100), 3)] {
        let mut world = test_world();
        let factory = world.spawn_finished_structure(id(&world, "air_factory"), Team::Blue, 50, 50).unwrap();
        world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: id(&world, "t1_air_scout"), count: 1 }, SimCommand::SetRally { factory, point: rally }]);
        assert_eq!(bay_heading(&world, factory), heading, "rally {rally:?}");
        let scout = first_built(&mut world, factory);
        let way = facing(&world, scout);
        let expected = [(1, 0), (0, 1), (-1, 0), (0, -1)][usize::from(heading)];
        assert_eq!((way.x, way.y), (fp_int(expected.0), fp_int(expected.1)), "it lifts off facing the way it stood");
    }
}

#[test]
fn units_from_a_factory_gather_in_a_neat_grid_at_the_rally_point() {
    let mut world = test_world();
    let factory = world.spawn_finished_structure(id(&world, "land_factory_t1"), Team::Blue, 50, 50).unwrap();
    // Due east of the factory, so that the grid lines up with the map.
    let rally = Vec2Fp::new(fp_int(1_400), position(&world, factory).y);
    let bot = id(&world, "t1_light_walker");
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: bot, count: 12 }, SimCommand::SetRally { factory, point: rally }]);
    let mut built: Vec<SimId> = Vec::new();
    for _ in 0..20_000 {
        world.advance_tick(&[]);
        for &(unit, _) in &world.units {
            if unit != factory && !built.contains(&unit) {
                built.push(unit);
            }
        }
        let settled = built.len() == 12 && built.iter().all(|&unit| leaving(&world, unit).is_none() && world.unit_order(unit).unwrap().kind == OrderKind::Idle);
        if settled {
            break;
        }
    }
    assert_eq!(built.len(), 12, "the factory should finish all twelve");
    assert!(built.iter().all(|&unit| world.unit_order(unit).unwrap().kind == OrderKind::Idle), "every bot should settle");
    let radius = world.blueprints.get(bot).radius;
    let spacing = radius * 3;
    let at: Vec<Vec2Fp> = built.iter().map(|&unit| position(&world, unit)).collect();
    for (index, here) in at.iter().enumerate() {
        // Each on a spot of the grid: rows a spacing apart back toward the factory, in the west,
        // and columns a spacing apart across.
        let (back, across) = (rally.x - here.x, here.y - rally.y);
        let near_line = |offset: Fp| {
            let steps = (offset / spacing).round();
            (offset - steps * spacing).abs() <= radius / 2
        };
        assert!(near_line(back) && near_line(across), "bot {index} at {here:?} is off the grid: {at:?}, spacing {spacing}");
        assert!(back >= -(radius / 2) && across.abs() <= spacing * 2 + radius / 2, "bot {index} at {here:?} is outside the five columns");
        for other in &at[index + 1..] {
            assert!(sim_engine::math::fp::sqrt_wide(here.distance_squared(other)) >= spacing - radius, "two bots ended up crowded: {here:?} and {other:?} in {at:?}");
        }
    }
    // Twelve fill the front two rows and two of the third.
    let rows = at.iter().map(|here| ((rally.x - here.x) / spacing).round().to_num::<i32>()).max().unwrap();
    assert_eq!(rows, 2, "three rows of five: {at:?}");
}
