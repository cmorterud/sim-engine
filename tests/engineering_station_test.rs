//! Engineering stations help whatever is being built, made, upgraded, or repaired within their
//! reach, by themselves, and nothing beyond it.

use sim_engine::math::fp::fp_int;
use sim_engine::sim::blueprints::{BlueprintId, Res};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Factory, Health, SimId, Team};
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

/// Ticks until the factory has made its first unit.
fn ticks_to_build(world: &mut SimWorld, factory: SimId, blueprint: BlueprintId) -> u32 {
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint, count: 1 }]);
    let before = world.units.len();
    for tick in 1..5_000 {
        world.advance_tick(&[]);
        if world.units.len() > before {
            return tick;
        }
    }
    panic!("the factory never finished");
}

#[test]
fn a_station_speeds_up_a_factory_in_its_reach_and_not_one_beyond() {
    let tank = |world: &SimWorld| id(world, "t1_medium_tank");
    let mut alone = test_world();
    let factory = alone.spawn_finished_structure(id(&alone, "land_factory_t1"), Team::Blue, 50, 50).unwrap();
    let blueprint = tank(&alone);
    let by_itself = ticks_to_build(&mut alone, factory, blueprint);

    let mut helped = test_world();
    let factory = helped.spawn_finished_structure(id(&helped, "land_factory_t1"), Team::Blue, 50, 50).unwrap();
    helped.spawn_finished_structure(id(&helped, "engineering_station"), Team::Blue, 55, 50).unwrap();
    let blueprint = tank(&helped);
    let with_station = ticks_to_build(&mut helped, factory, blueprint);
    // 20 build power and 25 more: a little under half the time.
    assert!(with_station * 2 < by_itself + 20, "{with_station} ticks with the station, {by_itself} without");

    let mut far = test_world();
    let factory = far.spawn_finished_structure(id(&far, "land_factory_t1"), Team::Blue, 50, 50).unwrap();
    far.spawn_finished_structure(id(&far, "engineering_station"), Team::Blue, 100, 50).unwrap();
    let blueprint = tank(&far);
    assert_eq!(ticks_to_build(&mut far, factory, blueprint), by_itself, "1,000 units off is out of reach");
}

#[test]
fn a_station_repairs_a_damaged_friend_in_reach() {
    let mut world = test_world();
    world.spawn_finished_structure(id(&world, "engineering_station"), Team::Blue, 50, 50).unwrap();
    let defense = world.spawn_finished_structure(id(&world, "t1_point_defense"), Team::Blue, 54, 50).unwrap();
    let entity = world.unit_entity(defense).unwrap();
    let max = world.ecs.get::<&Health>(entity).unwrap().max;
    world.ecs.get::<&mut Health>(entity).unwrap().current = max / 2;
    for _ in 0..300 {
        world.advance_tick(&[]);
    }
    let health = world.ecs.get::<&Health>(entity).unwrap().current;
    assert!(health > max / 2 + fp_int(100), "repaired to {health} of {max}");
}

#[test]
fn a_station_leaves_a_factory_with_nothing_to_make_for_other_work() {
    let mut world = test_world();
    let factory = world.spawn_finished_structure(id(&world, "land_factory_t1"), Team::Blue, 50, 50).unwrap();
    world.spawn_finished_structure(id(&world, "engineering_station"), Team::Blue, 55, 50).unwrap();
    let scout = id(&world, "t1_land_scout");
    ticks_to_build(&mut world, factory, scout);
    assert!(world.ecs.get::<&Factory>(world.unit_entity(factory).unwrap()).unwrap().queue.is_empty());
    let defense = world.spawn_finished_structure(id(&world, "t1_point_defense"), Team::Blue, 58, 53).unwrap();
    let entity = world.unit_entity(defense).unwrap();
    let max = world.ecs.get::<&Health>(entity).unwrap().max;
    world.ecs.get::<&mut Health>(entity).unwrap().current = max / 2;
    for _ in 0..300 {
        world.advance_tick(&[]);
    }
    assert!(world.ecs.get::<&Health>(entity).unwrap().current > max / 2, "it went on to repair the defense");
}

#[test]
fn the_t3_station_is_an_upgrade_with_more_build_power() {
    let db_world = test_world();
    let (t2, t3) = (db_world.blueprints.get(id(&db_world, "engineering_station")), db_world.blueprints.get(id(&db_world, "engineering_station_t3")));
    assert_eq!(t2.upgrades_to, Some(t3.id));
    // FAF's build rates: 20 for the T2 station, 30 for the T3.
    assert_eq!((t2.build_power.to_num::<i64>(), t3.build_power.to_num::<i64>()), (20, 30));
    assert!(t2.assists && t3.assists);
}
