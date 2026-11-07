//! Tech tiers (GAME_SPECIFICATION.md §15): who can build what, and the commander's upgrades.

use sim_engine::math::fp::{fp_int, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Blueprinted, BuildTask, Builder, Position, SimId, Team, Upgrading};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::SimWorld;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn rich_world() -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.mass] {
            *value = Res::from_num(100_000);
        }
        for value in [&mut economy.base_energy_capacity, &mut economy.energy_capacity, &mut economy.energy] {
            *value = Res::from_num(1_000_000);
        }
    }
    world
}

fn spawn(world: &mut SimWorld, key: &str, at: Vec2Fp) -> SimId {
    let blueprint = world.blueprints.id(key);
    world.spawn_mobile(blueprint, Team::Blue, at)
}

fn blueprint_key(world: &SimWorld, id: SimId) -> String {
    let blueprint = world.ecs.get::<&Blueprinted>(world.unit_entity(id).unwrap()).unwrap().0;
    world.blueprints.get(blueprint).key.clone()
}

fn tasks(world: &SimWorld, id: SimId) -> Vec<BuildTask> {
    world.ecs.get::<&Builder>(world.unit_entity(id).unwrap()).unwrap().tasks.clone()
}

fn order_build(world: &mut SimWorld, units: Vec<SimId>, key: &str, cell_x: u32) {
    let blueprint = world.blueprints.id(key);
    world.advance_tick(&[SimCommand::Build { units, blueprint, cell_x, cell_y: 50, queued: false }]);
}

#[test]
fn each_tier_of_builder_builds_only_up_to_its_tier() {
    let mut world = rich_world();
    let (t1, t2, t3) = (spawn(&mut world, "t1_engineer", point(1_100, 1_000)), spawn(&mut world, "t2_engineer", point(1_100, 1_040)), spawn(&mut world, "t3_engineer", point(1_100, 1_080)));
    let commander = spawn(&mut world, "acu", point(1_100, 1_120));
    for (key, able, cell_x) in [("power_generator", vec![t1, t2, t3, commander], 60), ("tactical_missile_launcher", vec![t2, t3], 70), ("strategic_missile_launcher", vec![t3], 80)] {
        for unit in [t1, t2, t3, commander] {
            world.advance_tick(&[SimCommand::Stop { units: vec![unit] }]);
        }
        order_build(&mut world, vec![t1, t2, t3, commander], key, cell_x);
        for unit in [t1, t2, t3, commander] {
            assert_eq!(!tasks(&world, unit).is_empty(), able.contains(&unit), "{key} for {}", blueprint_key(&world, unit));
        }
    }
}

#[test]
fn the_commander_upgrades_in_place_standing_still_and_then_builds_the_next_tier() {
    let mut world = rich_world();
    let commander = spawn(&mut world, "acu", point(1_000, 1_000));
    let entity = world.unit_entity(commander).unwrap();
    let here = world.ecs.get::<&Position>(entity).unwrap().0;
    let mass_before = world.economies[0].mass;
    world.advance_tick(&[SimCommand::Upgrade { units: vec![commander] }]);
    assert!(world.ecs.get::<&Upgrading>(entity).is_ok());
    // An order to build waits until the upgrade is done.
    world.advance_tick(&[SimCommand::QueueTask { units: vec![commander], task: BuildTask::Assist { target: commander } }]);
    // 1,000 build time at 10 build power is 100 seconds.
    let mut done_at = None;
    for tick in 0..1_100 {
        world.advance_tick(&[]);
        assert_eq!(world.ecs.get::<&Position>(entity).unwrap().0, here, "it stands still while it upgrades");
        if blueprint_key(&world, commander) == "acu_t2" {
            done_at = Some(tick);
            break;
        }
    }
    let done_at = done_at.expect("the upgrade should finish");
    assert!((990..=1_010).contains(&done_at), "it took {done_at} ticks");
    // 800 mass for the upgrade, less the commander's own 1 a second over the 100 seconds.
    let spent = (mass_before - world.economies[0].mass).to_num::<f64>();
    assert!((695.0..=705.0).contains(&spent), "it cost {spent} mass");
    assert_eq!(world.ecs.get::<&Builder>(entity).unwrap().power, fp_int(20), "with more build power");
    assert!(world.ecs.get::<&Upgrading>(entity).is_err());
    world.advance_tick(&[SimCommand::Stop { units: vec![commander] }]);
    order_build(&mut world, vec![commander], "tactical_missile_launcher", 70);
    assert!(!tasks(&world, commander).is_empty(), "now it can build T2 structures");
    // And it can go on to T3.
    world.advance_tick(&[SimCommand::Upgrade { units: vec![commander] }]);
    assert_eq!(world.ecs.get::<&Upgrading>(entity).unwrap().into, world.blueprints.id("acu_t3"));
}

#[test]
fn engineers_can_assist_an_upgrade_and_a_new_order_cancels_it() {
    let mut world = rich_world();
    let commander = spawn(&mut world, "acu", point(1_000, 1_000));
    let helper = spawn(&mut world, "t1_engineer", point(1_060, 1_000));
    world.advance_tick(&[SimCommand::Upgrade { units: vec![commander] }]);
    world.advance_tick(&[SimCommand::Assist { units: vec![helper], target: commander }]);
    // 10 and 5 build power together take two thirds of the time.
    let took = (0..1_000).position(|_| {
        world.advance_tick(&[]);
        blueprint_key(&world, commander) == "acu_t2"
    });
    let took = took.expect("the upgrade should finish");
    assert!((640..=700).contains(&took), "it took {took} ticks");

    let entity = world.unit_entity(commander).unwrap();
    world.advance_tick(&[SimCommand::Upgrade { units: vec![commander] }]);
    for _ in 0..100 {
        world.advance_tick(&[]);
    }
    assert!(world.ecs.get::<&Upgrading>(entity).unwrap().progress > Res::ZERO);
    world.advance_tick(&[SimCommand::Move { units: vec![commander], goal: point(1_500, 1_000) }]);
    assert!(world.ecs.get::<&Upgrading>(entity).is_err(), "moving cancels the upgrade");
    assert_eq!(blueprint_key(&world, commander), "acu_t2");
}

#[test]
fn factories_go_up_to_t3_and_make_engineers_of_their_tier() {
    let mut world = rich_world();
    let factory = world.spawn_finished_structure(world.blueprints.id("land_factory_t2"), Team::Blue, 50, 50).unwrap();
    let entity = world.unit_entity(factory).unwrap();
    world.ecs.get::<&mut sim_engine::sim::components::Factory>(entity).unwrap().power = fp_int(2_000);
    world.advance_tick(&[SimCommand::Upgrade { units: vec![factory] }]);
    for _ in 0..40 {
        world.advance_tick(&[]);
    }
    assert_eq!(blueprint_key(&world, factory), "land_factory_t3");
    let engineer = world.blueprints.id("t3_engineer");
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: engineer, count: 1 }]);
    // 520 build time at the T3 factory's 90 build power is under 6 seconds.
    for _ in 0..80 {
        world.advance_tick(&[]);
    }
    let made = world.units.iter().any(|&(id, _)| id != factory && blueprint_key(&world, id) == "t3_engineer");
    assert!(made, "the T3 factory should have made a T3 engineer");
}
