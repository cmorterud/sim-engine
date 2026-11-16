//! Builders' orders on structures being built: assisting a factory site carries on into its
//! production, and a new order takes a builder off what it was building.

use sim_engine::math::fp::{fp_int, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Builder, BuildTask, Construction, SimId, Team};
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
            *value = Res::from_num(5_000_000);
        }
    }
    world
}

fn tasks(world: &SimWorld, id: SimId) -> Vec<BuildTask> {
    world.ecs.get::<&Builder>(world.unit_entity(id).unwrap()).unwrap().tasks.clone()
}

/// The site a builder's first task is building, once it has started one.
fn site_of(world: &SimWorld, id: SimId) -> Option<SimId> {
    match tasks(world, id).first() {
        Some(BuildTask::Build { site, .. }) => *site,
        _ => None,
    }
}

fn progress(world: &SimWorld, site: SimId) -> Option<Res> {
    world.unit_entity(site).and_then(|entity| world.ecs.get::<&Construction>(entity).ok().map(|construction| construction.progress))
}

fn run_until(world: &mut SimWorld, ticks: u32, mut done: impl FnMut(&SimWorld) -> bool) -> bool {
    for _ in 0..ticks {
        if done(world) {
            return true;
        }
        world.advance_tick(&[]);
    }
    done(world)
}

#[test]
fn builders_assisting_a_factory_site_go_on_to_help_it_build_units() {
    let mut world = rich_world();
    let commander = world.spawn_mobile(world.blueprints.id("command_rig"), Team::Blue, point(1_000, 1_000));
    let engineer = world.spawn_mobile(world.blueprints.id("t1_engineer"), Team::Blue, point(1_000, 1_060));
    let factory_kind = world.blueprints.id("air_factory");
    world.advance_tick(&[SimCommand::Build { units: vec![commander], blueprint: factory_kind, cell_x: 60, cell_y: 50, queued: false }]);
    assert!(run_until(&mut world, 600, |world| site_of(world, commander).is_some()), "the commander should start the factory");
    let site = site_of(&world, commander).unwrap();
    // A right-click on the site, by the engineer and by the commander building it.
    world.advance_tick(&[SimCommand::Assist { units: vec![engineer, commander], target: site }]);
    assert!(run_until(&mut world, 5_000, |world| progress(world, site).is_none()), "the factory should be finished");
    // Nothing is queued for a while, and they wait by it.
    for _ in 0..200 {
        world.advance_tick(&[]);
    }
    let bomber = world.blueprints.id("t1_bomber");
    world.advance_tick(&[SimCommand::QueueUnit { factory: site, blueprint: bomber, count: 1 }]);
    world.advance_tick(&[]);
    world.advance_tick(&[]);
    let links = world.snapshot().work_links;
    for builder in [engineer, commander] {
        assert_eq!(tasks(&world, builder).first(), Some(&BuildTask::Assist { target: site }), "{builder:?} should stay on the factory");
        assert!(links.iter().any(|link| link.builder == builder), "{builder:?} should be helping build the factory's bomber");
    }
}

#[test]
fn a_builder_given_a_new_structure_stops_building_the_last_one() {
    let mut world = rich_world();
    let builder = world.spawn_mobile(world.blueprints.id("command_rig"), Team::Blue, point(1_000, 1_000));
    world.install_enhancement(builder, world.blueprints.enhancement_id("t2_fabrication"));
    let (factory_kind, generator_kind) = (world.blueprints.id("air_factory"), world.blueprints.id("power_generator_t2"));
    world.advance_tick(&[SimCommand::Build { units: vec![builder], blueprint: factory_kind, cell_x: 60, cell_y: 50, queued: false }]);
    assert!(run_until(&mut world, 600, |world| site_of(world, builder).is_some_and(|site| progress(world, site).is_some_and(|done| done > Res::from_num(0.1)))));
    let factory = site_of(&world, builder).unwrap();
    // A new order, not queued, for a T2 generator a little way off.
    world.advance_tick(&[SimCommand::Build { units: vec![builder], blueprint: generator_kind, cell_x: 40, cell_y: 50, queued: false }]);
    let stopped_at = progress(&world, factory).unwrap();
    let mut generator = None;
    for _ in 0..300 {
        world.advance_tick(&[]);
        generator = generator.or_else(|| site_of(&world, builder).filter(|&site| site != factory));
    }
    let generator = generator.expect("the builder should start the generator");
    assert!(progress(&world, generator).is_some_and(|done| done > Res::ZERO) || world.unit_entity(generator).is_some(), "the generator is under way");
    assert_eq!(progress(&world, factory), Some(stopped_at), "the factory should wait, with nobody building it");
}

/// A selection of a commander that can build T2 and a T1 engineer that cannot, both building a
/// factory, told to build a T2 generator: the engineer used to go on building the factory, so
/// that both went up at once. It now helps the commander with the generator.
#[test]
fn builders_that_cannot_build_a_new_structure_help_those_that_can() {
    let mut world = rich_world();
    let commander = world.spawn_mobile(world.blueprints.id("command_rig"), Team::Blue, point(1_000, 1_000));
    world.install_enhancement(commander, world.blueprints.enhancement_id("t2_fabrication"));
    let engineer = world.spawn_mobile(world.blueprints.id("t1_engineer"), Team::Blue, point(1_000, 1_060));
    let (factory_kind, generator_kind) = (world.blueprints.id("air_factory"), world.blueprints.id("power_generator_t2"));
    let both = vec![commander, engineer];
    world.advance_tick(&[SimCommand::Build { units: both.clone(), blueprint: factory_kind, cell_x: 60, cell_y: 50, queued: false }]);
    assert!(run_until(&mut world, 600, |world| site_of(world, commander).is_some_and(|site| progress(world, site).is_some_and(|done| done > Res::from_num(0.1)))));
    let factory = site_of(&world, commander).unwrap();
    world.advance_tick(&[SimCommand::Build { units: both, blueprint: generator_kind, cell_x: 40, cell_y: 50, queued: false }]);
    assert_eq!(tasks(&world, engineer), [BuildTask::Assist { target: commander }], "the engineer should help the commander");
    let stopped_at = progress(&world, factory).unwrap();
    for _ in 0..300 {
        world.advance_tick(&[]);
    }
    assert_eq!(progress(&world, factory), Some(stopped_at), "nobody should go on building the factory");
    let generator = site_of(&world, commander).filter(|&site| site != factory).expect("the commander should be building the generator");
    let links = world.snapshot().work_links;
    assert!(links.iter().any(|link| link.builder == engineer), "the engineer should be working on the generator {generator:?}");
}
