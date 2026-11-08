//! Tech tiers (GAME_SPECIFICATION.md §15): who can build what, and the commander's engineering suites.

use sim_engine::math::fp::{fp_int, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Blueprinted, BuildTask, Builder, Enhancements, Enhancing, Position, SimId, Team};
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

/// The commander's left-arm enhancement, if it has one.
fn left_arm(world: &SimWorld, id: SimId) -> Option<String> {
    let installed = world.ecs.get::<&Enhancements>(world.unit_entity(id).unwrap()).ok()?.0[1]?;
    Some(world.blueprints.enhancement(installed).key.clone())
}

fn enhance(world: &mut SimWorld, id: SimId, key: &str) {
    let enhancement = world.blueprints.enhancement_id(key);
    world.advance_tick(&[SimCommand::Enhance { units: vec![id], enhancement }]);
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
fn the_commander_takes_an_engineering_suite_standing_still_and_then_builds_the_next_tier() {
    let mut world = rich_world();
    let commander = spawn(&mut world, "acu", point(1_000, 1_000));
    let entity = world.unit_entity(commander).unwrap();
    let here = world.ecs.get::<&Position>(entity).unwrap().0;
    let mass_before = world.economies[0].mass;
    let health_before = world.unit_health(commander).unwrap();
    enhance(&mut world, commander, "advanced_engineering");
    assert!(world.ecs.get::<&Enhancing>(entity).is_ok());
    // An order to build waits until the enhancement is done.
    world.advance_tick(&[SimCommand::QueueTask { units: vec![commander], task: BuildTask::Assist { target: commander } }]);
    // 1,000 build time at 10 build power is 100 seconds.
    let mut done_at = None;
    for tick in 0..1_100 {
        world.advance_tick(&[]);
        assert_eq!(world.ecs.get::<&Position>(entity).unwrap().0, here, "it stands still while it works");
        if left_arm(&world, commander).as_deref() == Some("advanced_engineering") {
            done_at = Some(tick);
            break;
        }
    }
    let done_at = done_at.expect("the enhancement should finish");
    assert!((990..=1_010).contains(&done_at), "it took {done_at} ticks");
    // 800 mass, less the commander's own 1 a second over the 100 seconds.
    let spent = (mass_before - world.economies[0].mass).to_num::<f64>();
    assert!((695.0..=705.0).contains(&spent), "it cost {spent} mass");
    assert_eq!(world.ecs.get::<&Builder>(entity).unwrap().power, fp_int(42), "with more build power");
    assert_eq!(world.unit_health(commander).unwrap(), health_before + fp_int(2_000), "and more health");
    assert_eq!(blueprint_key(&world, commander), "acu", "the commander keeps its own blueprint");
    world.advance_tick(&[SimCommand::Stop { units: vec![commander] }]);
    order_build(&mut world, vec![commander], "tactical_missile_launcher", 70);
    assert!(!tasks(&world, commander).is_empty(), "now it can build T2 structures");
    world.advance_tick(&[SimCommand::Stop { units: vec![commander] }]);
    order_build(&mut world, vec![commander], "strategic_missile_launcher", 80);
    assert!(tasks(&world, commander).is_empty(), "but not T3 ones");
    // And it can go on to T3, which builds on T2.
    enhance(&mut world, commander, "t3_engineering");
    assert_eq!(world.ecs.get::<&Enhancing>(entity).unwrap().enhancement, world.blueprints.enhancement_id("t3_engineering"));
}

#[test]
fn engineers_can_assist_an_enhancement_and_a_new_order_cancels_it() {
    let mut world = rich_world();
    let commander = spawn(&mut world, "acu", point(1_000, 1_000));
    let helper = spawn(&mut world, "t1_engineer", point(1_060, 1_000));
    enhance(&mut world, commander, "advanced_engineering");
    world.advance_tick(&[SimCommand::Assist { units: vec![helper], target: commander }]);
    // 10 and 5 build power together take two thirds of the time.
    let took = (0..1_000).position(|_| {
        world.advance_tick(&[]);
        left_arm(&world, commander).is_some()
    });
    let took = took.expect("the enhancement should finish");
    assert!((640..=700).contains(&took), "it took {took} ticks");

    let entity = world.unit_entity(commander).unwrap();
    enhance(&mut world, commander, "t3_engineering");
    for _ in 0..100 {
        world.advance_tick(&[]);
    }
    assert!(world.ecs.get::<&Enhancing>(entity).unwrap().progress > Res::ZERO);
    world.advance_tick(&[SimCommand::Move { units: vec![commander], goal: point(1_500, 1_000) }]);
    assert!(world.ecs.get::<&Enhancing>(entity).is_err(), "moving cancels the enhancement");
    assert_eq!(left_arm(&world, commander).as_deref(), Some("advanced_engineering"), "and what it built on stays");
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

#[test]
fn a_commander_queues_the_next_tier_as_soon_as_its_suite_is_started_and_loses_it_if_cancelled() {
    let mut world = rich_world();
    let commander = spawn(&mut world, "acu", point(1_000, 1_000));
    let entity = world.unit_entity(commander).unwrap();
    enhance(&mut world, commander, "advanced_engineering");
    // Even without Shift, a T2 structure waits behind the T2 suite, and does not cancel it.
    order_build(&mut world, vec![commander], "tactical_missile_launcher", 60);
    assert!(world.ecs.get::<&Enhancing>(entity).is_ok(), "the suite carries on");
    assert!(matches!(tasks(&world, commander).as_slice(), [BuildTask::Build { .. }]), "the launcher is queued");
    // T3 is still out of reach.
    let mut more = world.blueprints.id("strategic_missile_launcher");
    world.advance_tick(&[SimCommand::Build { units: vec![commander], blueprint: more, cell_x: 70, cell_y: 50, queued: true }]);
    assert_eq!(tasks(&world, commander).len(), 1, "no T3 structure on a T2 suite");
    // Once the suite is done, it builds the launcher.
    let built = (0..2_000).any(|_| {
        world.advance_tick(&[]);
        world.units.iter().any(|unit| unit.0 != commander && blueprint_key(&world, unit.0) == "tactical_missile_launcher")
    });
    assert!(built, "the queued launcher should be started once the suite is done");

    // A suite cancelled takes what was queued for it along.
    let mut world = rich_world();
    let commander = spawn(&mut world, "acu", point(1_000, 1_000));
    let entity = world.unit_entity(commander).unwrap();
    enhance(&mut world, commander, "advanced_engineering");
    order_build(&mut world, vec![commander], "tactical_missile_launcher", 60);
    more = world.blueprints.id("tactical_missile_defense");
    world.advance_tick(&[SimCommand::Build { units: vec![commander], blueprint: more, cell_x: 70, cell_y: 50, queued: true }]);
    assert_eq!(tasks(&world, commander).len(), 2);
    world.advance_tick(&[SimCommand::Stop { units: vec![commander] }]);
    assert!(world.ecs.get::<&Enhancing>(entity).is_err());
    assert!(tasks(&world, commander).is_empty(), "the T2 structures are gone with the suite");
    // And with no suite under way, T2 is out of reach again.
    order_build(&mut world, vec![commander], "tactical_missile_launcher", 60);
    assert!(tasks(&world, commander).is_empty());
}

#[test]
fn an_enemy_upgrade_in_sight_shows_that_it_is_happening_and_nothing_more() {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        economy.mass = Res::from_num(5_000);
        economy.energy = Res::from_num(50_000);
    }
    let blue = world.spawn_mobile(world.blueprints.id("acu"), Team::Blue, point(1_000, 1_000));
    let red = world.spawn_mobile(world.blueprints.id("acu"), Team::Red, point(4_000, 4_000));
    let enhancement = world.blueprints.enhancement_id("advanced_engineering");
    world.advance_tick(&[SimCommand::Enhance { units: vec![red], enhancement }]);
    world.advance_tick(&[]);
    // Out of sight, nothing at all.
    let view = world.snapshot_for(Some(Team::Blue));
    assert!(!view.upgrading.contains(&red));
    // In sight, that it is upgrading, but not what into or how far along.
    world.advance_tick(&[SimCommand::Move { units: vec![blue], goal: point(3_700, 3_700) }]);
    let seen = (0..1_000).any(|_| {
        world.advance_tick(&[]);
        world.snapshot_for(Some(Team::Blue)).upgrading.contains(&red)
    });
    assert!(seen, "the enemy's upgrade should show once it is in sight");
    let view = world.snapshot_for(Some(Team::Blue));
    assert!(view.enhancing.iter().all(|(id, _, _)| *id != red), "its progress stays hidden");
    assert!(world.snapshot_for(Some(Team::Red)).enhancing.iter().any(|(id, _, _)| *id == red), "its owner sees it");
}

fn key_of(world: &SimWorld, id: SimId) -> String {
    blueprint_key(world, id)
}

/// Runs until something of this kind exists, finished, and returns it.
fn built(world: &mut SimWorld, key: &str, limit: u32) -> Option<SimId> {
    for _ in 0..limit {
        world.advance_tick(&[]);
        let found = world.units.iter().map(|unit| unit.0).find(|&id| {
            let entity = world.unit_entity(id).unwrap();
            key_of(world, id) == key && world.ecs.get::<&sim_engine::sim::components::Construction>(entity).is_err()
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

#[test]
fn t2_builders_build_t2_extractors_generators_radar_and_point_defense_directly() {
    for (key, cell) in [("mass_extractor_t2", (50, 50)), ("power_generator_t2", (60, 50)), ("radar_t2", (70, 50)), ("t2_point_defense", (75, 50))] {
        let mut world = rich_world();
        world.deposits.push((50, 50));
        let engineer = spawn(&mut world, "t2_engineer", point(cell.0 as i32 * 20 - 60, 1_000));
        let blueprint = world.blueprints.id(key);
        world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint, cell_x: cell.0, cell_y: cell.1, queued: false }]);
        assert!(built(&mut world, key, 3_000).is_some(), "a T2 engineer should build {key} directly");
    }
    // A T1 engineer cannot.
    let mut world = rich_world();
    world.deposits.push((50, 50));
    let engineer = spawn(&mut world, "t1_engineer", point(940, 1_000));
    let blueprint = world.blueprints.id("mass_extractor_t2");
    world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint, cell_x: 50, cell_y: 50, queued: false }]);
    assert!(tasks(&world, engineer).is_empty());
}

#[test]
fn support_factories_need_a_headquarters_to_be_built_and_to_build_above_t1() {
    let mut world = rich_world();
    let engineer = spawn(&mut world, "t2_engineer", point(1_000, 1_000));
    let support = world.blueprints.id("land_factory_t2_support");
    world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint: support, cell_x: 55, cell_y: 45, queued: false }]);
    assert!(tasks(&world, engineer).is_empty(), "no headquarters, no support factory");

    let headquarters = world.spawn_finished_structure(world.blueprints.id("land_factory_t2"), Team::Blue, 40, 40).unwrap();
    world.advance_tick(&[SimCommand::Build { units: vec![engineer], blueprint: support, cell_x: 55, cell_y: 45, queued: false }]);
    let factory = built(&mut world, "land_factory_t2_support", 2_000).expect("with a T2 headquarters it builds one");
    // It builds T2 units while the headquarters stands.
    let tank = world.blueprints.id("t2_heavy_tank");
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: tank, count: 2 }]);
    assert!(built(&mut world, "t2_heavy_tank", 600).is_some(), "it builds tanks");
    // Without it, the next tank waits.
    let tanks = |world: &SimWorld| world.units.iter().filter(|unit| key_of(world, unit.0) == "t2_heavy_tank").count();
    let before = tanks(&world);
    world.advance_tick(&[SimCommand::SelfDestruct { units: vec![headquarters] }]);
    for _ in 0..600 {
        world.advance_tick(&[]);
    }
    assert!(world.unit_health(headquarters).is_none());
    assert_eq!(tanks(&world), before, "no tank without a headquarters");
    let entity = world.unit_entity(factory).unwrap();
    let progress = |world: &SimWorld| world.ecs.get::<&sim_engine::sim::components::Factory>(entity).unwrap().progress;
    let waiting = progress(&world);
    let mass = world.economies[0].mass;
    for _ in 0..100 {
        world.advance_tick(&[]);
    }
    assert_eq!(progress(&world), waiting, "the tank started before stays where it was");
    assert!(world.economies[0].mass >= mass, "and nothing more is spent on it");
}

#[test]
fn a_support_factory_upgrades_to_t3_only_with_a_t3_headquarters() {
    let mut world = rich_world();
    world.spawn_finished_structure(world.blueprints.id("land_factory_t2"), Team::Blue, 40, 40).unwrap();
    let factory = world.spawn_finished_structure(world.blueprints.id("land_factory_t2_support"), Team::Blue, 55, 45).unwrap();
    let upgrade = world.blueprints.id("land_factory_t3_support");
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: upgrade, count: 1 }]);
    for _ in 0..600 {
        world.advance_tick(&[]);
    }
    assert_eq!(key_of(&world, factory), "land_factory_t2_support", "it waits for a T3 headquarters");
    world.spawn_finished_structure(world.blueprints.id("land_factory_t3"), Team::Blue, 70, 40).unwrap();
    let upgraded = (0..1_000).any(|_| {
        world.advance_tick(&[]);
        world.unit_entity(factory).is_some_and(|_| key_of(&world, factory) == "land_factory_t3_support")
    });
    assert!(upgraded, "then it upgrades");
}

