//! Missiles and missile defense (GAME_SPECIFICATION.md §8): tactical missiles and the defenses
//! that shoot them down, and nuclear missiles and the silos that stop them.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::math::rect::RectFp;
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Combat, Factory, Health, Launcher, MissileDefense, Shield, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::SimWorld;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// A flat 5,120-unit map in plain sight, with plenty in store for both teams.
fn open_world() -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        // The inspiration's nuclear missiles cost 12,000 mass and 1,500,000 energy apiece.
        for capacity in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.mass] {
            *capacity = Res::from_num(1_000_000);
        }
        for capacity in [&mut economy.base_energy_capacity, &mut economy.energy_capacity, &mut economy.energy] {
            *capacity = Res::from_num(100_000_000);
        }
    }
    world
}

fn structure(world: &mut SimWorld, key: &str, team: Team, cell_x: u32, cell_y: u32) -> SimId {
    let blueprint = world.blueprints.id(key);
    world.spawn_finished_structure(blueprint, team, cell_x, cell_y).unwrap_or_else(|| panic!("no room for {key}"))
}

fn alive(world: &SimWorld, id: SimId) -> bool {
    world.unit_entity(id).is_some()
}

/// Runs until `done`, or gives up after `limit` ticks. Returns the ticks taken.
fn run_until(world: &mut SimWorld, limit: u32, mut done: impl FnMut(&SimWorld) -> bool) -> Option<u32> {
    for tick in 0..limit {
        world.advance_tick(&[]);
        if done(world) {
            return Some(tick);
        }
    }
    None
}

/// A finished launcher with missiles already built.
fn loaded(world: &mut SimWorld, key: &str, team: Team, cell_x: u32, cell_y: u32, stock: u32) -> SimId {
    let id = structure(world, key, team, cell_x, cell_y);
    world.ecs.get::<&mut Launcher>(world.unit_entity(id).unwrap()).unwrap().stock = stock;
    id
}

fn health(world: &SimWorld, id: SimId) -> Fp {
    world.unit_health(id).unwrap()
}

fn missiles_from(world: &SimWorld, source: SimId) -> usize {
    world
        .missiles
        .iter()
        .filter(|&&(_, entity)| world.ecs.get::<&sim_engine::sim::components::Projectile>(entity).is_ok_and(|projectile| projectile.source == source))
        .count()
}

#[test]
fn a_tactical_launcher_fires_only_on_order_and_only_missiles_it_has_built() {
    let mut world = open_world();
    let launcher = structure(&mut world, "tactical_missile_launcher", Team::Blue, 20, 20);
    // A power generator 1,600 away, in range and in plain sight, is no target until ordered.
    let target = structure(&mut world, "power_generator", Team::Red, 100, 20);
    world.ecs.get::<&mut Launcher>(world.unit_entity(launcher).unwrap()).unwrap().stock = 1;
    assert!(run_until(&mut world, 60, |world| missiles_from(world, launcher) > 0).is_none(), "it fired without an order");

    // Ordered, it fires, and one 1,500-damage missile is enough.
    world.advance_tick(&[SimCommand::Attack { units: vec![launcher], target }]);
    let took = run_until(&mut world, 300, |world| !alive(world, target)).expect("the missile should destroy it");
    // About 57 ticks of flight at the inspiration's 28 a tick over the 1,600, plus the climb: not
    // at once.
    assert!(took > 45, "it arrived after only {took} ticks");

    // With none left, an order waits for one to be built.
    let next = structure(&mut world, "power_generator", Team::Red, 100, 30);
    world.advance_tick(&[SimCommand::Attack { units: vec![launcher], target: next }]);
    assert!(run_until(&mut world, 200, |world| missiles_from(world, launcher) > 0).is_none(), "it has no missile");
    let missile = world.blueprints.id("tactical_missile");
    world.advance_tick(&[SimCommand::QueueUnit { factory: launcher, blueprint: missile, count: 1 }]);
    // 300 build time at its own 10 build power is 30 seconds, then the flight.
    let took = run_until(&mut world, 600, |world| !alive(world, next)).expect("the built missile should destroy it");
    assert!(took > 300, "it fired after only {took} ticks");
}

#[test]
fn a_launcher_keeps_firing_at_its_target_while_it_has_missiles() {
    let mut world = open_world();
    let launcher = loaded(&mut world, "tactical_missile_launcher", Team::Blue, 20, 20, 3);
    // A land factory made tough enough to take three missiles.
    let target = structure(&mut world, "land_factory_t1", Team::Red, 100, 20);
    let missile = world.blueprints.get(world.blueprints.id("tactical_missile_launcher")).weapon.expect("it fires missiles").damage;
    {
        let mut health = world.ecs.get::<&mut Health>(world.unit_entity(target).unwrap()).unwrap();
        health.max = missile * 3 - Fp::ONE;
        health.current = health.max;
    }
    world.advance_tick(&[SimCommand::Attack { units: vec![launcher], target }]);
    assert!(run_until(&mut world, 600, |world| !alive(world, target)).is_some(), "three missiles should destroy it");
    assert_eq!(world.ecs.get::<&Launcher>(world.unit_entity(launcher).unwrap()).unwrap().stock, 0);
}

#[test]
fn a_missile_flies_over_a_wall_that_would_stop_a_shell() {
    let mut world = open_world();
    let launcher = loaded(&mut world, "tactical_missile_launcher", Team::Blue, 20, 20, 1);
    world.add_box_obstacle(RectFp::new(point(900, 0), point(940, 1_200)));
    let target = structure(&mut world, "power_generator", Team::Red, 60, 20);
    world.advance_tick(&[SimCommand::Attack { units: vec![launcher], target }]);
    assert!(run_until(&mut world, 300, |world| !alive(world, target)).is_some(), "the wall should not stop it");
}

#[test]
fn a_missile_defense_shoots_down_one_missile_at_a_time() {
    let mut world = open_world();
    let first = loaded(&mut world, "tactical_missile_launcher", Team::Blue, 20, 20, 1);
    let target = structure(&mut world, "power_generator", Team::Red, 100, 20);
    structure(&mut world, "tactical_missile_defense", Team::Red, 102, 20);
    world.advance_tick(&[SimCommand::Attack { units: vec![first], target }]);
    let mut intercepted = 0;
    for _ in 0..200 {
        world.advance_tick(&[]);
        intercepted += world.snapshot_for(Some(Team::Red)).interceptions.len();
    }
    assert_eq!(intercepted, 1, "one missile, shot down once");
    assert!(alive(&world, target), "the defense should have stopped it");

    // Three launchers fire together. A missile crosses the defense's 620 in about 34 ticks,
    // time for two shots 3 seconds apart, so the third gets through.
    let mut world = open_world();
    let launchers: Vec<SimId> = [18, 20, 22].into_iter().map(|cell_y| loaded(&mut world, "tactical_missile_launcher", Team::Blue, 20, cell_y, 1)).collect();
    let target = structure(&mut world, "power_generator", Team::Red, 100, 21);
    structure(&mut world, "tactical_missile_defense", Team::Red, 102, 21);
    world.advance_tick(&[SimCommand::Attack { units: launchers, target }]);
    assert!(run_until(&mut world, 300, |world| !alive(world, target)).is_some(), "a volley should get one through");
}

#[test]
fn a_shield_stops_a_tactical_missile() {
    let mut world = open_world();
    let launcher = loaded(&mut world, "tactical_missile_launcher", Team::Blue, 20, 20, 1);
    let target = structure(&mut world, "power_generator", Team::Red, 100, 20);
    let generator = structure(&mut world, "shield_generator", Team::Red, 101, 22);
    // No healing, so that the dome's loss is exactly what hit it, and strong enough to take it.
    let missile = world.blueprints.get(world.blueprints.id("tactical_missile_launcher")).weapon.expect("it fires missiles").damage;
    {
        let mut shield = world.ecs.get::<&mut Shield>(world.unit_entity(generator).unwrap()).unwrap();
        shield.regen_per_tick = Fp::ZERO;
        shield.max_health = missile * 2;
        shield.health = shield.max_health;
    }
    world.advance_tick(&[SimCommand::Attack { units: vec![launcher], target }]);
    run_until(&mut world, 140, |_| false);
    assert!(alive(&world, target));
    let shield = *world.ecs.get::<&Shield>(world.unit_entity(generator).unwrap()).unwrap();
    assert_eq!(shield.max_health - shield.health, missile, "the dome took the whole missile");
}

#[test]
fn a_silo_builds_missiles_and_keeps_them() {
    let mut world = open_world();
    let silo = structure(&mut world, "strategic_missile_launcher", Team::Blue, 20, 20);
    let entity = world.unit_entity(silo).unwrap();
    // Far more build power than it has, so that the test does not wait 300 seconds a missile.
    world.ecs.get::<&mut Factory>(entity).unwrap().power = fp_int(300_000);
    let missile = world.blueprints.id("strategic_missile");
    let units_before = world.units.len();
    world.advance_tick(&[SimCommand::QueueUnit { factory: silo, blueprint: missile, count: 2 }]);
    run_until(&mut world, 40, |_| false);
    assert_eq!(world.ecs.get::<&Launcher>(entity).unwrap().stock, 2);
    assert_eq!(world.units.len(), units_before, "a missile is not a unit");
    assert_eq!(world.snapshot_for(Some(Team::Blue)).stocks, [(silo, 2)]);

    // It keeps five at most, and spends nothing on a sixth until it fires one.
    world.advance_tick(&[SimCommand::QueueUnit { factory: silo, blueprint: missile, count: 4 }]);
    run_until(&mut world, 100, |_| false);
    assert_eq!(world.ecs.get::<&Launcher>(entity).unwrap().stock, 5);
    let mass = world.economies[0].mass;
    run_until(&mut world, 20, |_| false);
    assert!(world.economies[0].mass >= mass, "a full silo should spend nothing");
    world.ecs.get::<&mut Launcher>(entity).unwrap().stock = 4;
    run_until(&mut world, 20, |_| false);
    assert_eq!(world.ecs.get::<&Launcher>(entity).unwrap().stock, 5, "with room again, it builds the last one");
}

/// A Blue silo with two missiles built, and a Red base 3,000 away.
fn nuclear_standoff() -> (SimWorld, SimId, Vec<SimId>, SimId) {
    let mut world = open_world();
    let silo = structure(&mut world, "strategic_missile_launcher", Team::Blue, 20, 20);
    world.ecs.get::<&mut Launcher>(world.unit_entity(silo).unwrap()).unwrap().stock = 2;
    let base = vec![
        structure(&mut world, "land_factory_t1", Team::Red, 170, 20),
        structure(&mut world, "power_generator", Team::Red, 176, 20),
        structure(&mut world, "mass_storage", Team::Red, 170, 26),
    ];
    let far = structure(&mut world, "power_generator", Team::Red, 240, 20);
    (world, silo, base, far)
}

/// Where the standoff's missiles are aimed: the middle of the Red base.
fn ground_zero() -> Vec2Fp {
    point(3_440, 450)
}

#[test]
fn a_nuclear_missile_flattens_everything_in_its_blast_once_per_order() {
    let (mut world, silo, base, far) = nuclear_standoff();
    // A shield over the base does not stop it.
    structure(&mut world, "shield_generator", Team::Red, 172, 24);
    // Nothing is fired until it is ordered.
    assert!(run_until(&mut world, 30, |world| missiles_from(world, silo) > 0).is_none());
    // 710 away: outside the inner ring of 600, inside the outer one of 800.
    let edge = structure(&mut world, "power_generator", Team::Red, 207, 22);
    world.advance_tick(&[SimCommand::AttackGround { units: vec![silo], point: ground_zero() }]);
    assert!(run_until(&mut world, 500, |world| base.iter().all(|id| !alive(world, *id))).is_some(), "the inner ring should flatten the base");
    assert!(alive(&world, far), "1,370 away is outside the blast");
    assert_eq!(health(&world, edge), fp_int(100), "the outer ring does 500");
    let entity = world.unit_entity(silo).unwrap();
    assert_eq!(world.ecs.get::<&Launcher>(entity).unwrap().stock, 1, "one order, one missile");
    assert_eq!(world.ecs.get::<&Combat>(entity).unwrap().ground_target, None);
    assert!(run_until(&mut world, 200, |world| missiles_from(world, silo) > 0).is_none(), "it waits for the next order");
}

#[test]
fn a_strategic_defense_spends_an_interceptor_on_a_missile_in_its_range() {
    let (mut world, silo, base, _) = nuclear_standoff();
    let defense = structure(&mut world, "strategic_missile_defense", Team::Red, 165, 30);
    let defense_entity = world.unit_entity(defense).unwrap();
    world.ecs.get::<&mut MissileDefense>(defense_entity).unwrap().stock = 1;
    world.advance_tick(&[SimCommand::AttackGround { units: vec![silo], point: ground_zero() }]);
    run_until(&mut world, 500, |_| false);
    assert!(base.iter().all(|id| alive(&world, *id)), "the interceptor should have stopped it");
    assert_eq!(world.ecs.get::<&MissileDefense>(defense_entity).unwrap().stock, 0);

    // With no interceptor left, the second one lands.
    world.advance_tick(&[SimCommand::AttackGround { units: vec![silo], point: ground_zero() }]);
    assert!(run_until(&mut world, 500, |world| base.iter().all(|id| !alive(world, *id))).is_some());
}

#[test]
fn a_strategic_defense_stops_a_missile_flying_over_it_to_somewhere_else() {
    let (mut world, silo, _, far) = nuclear_standoff();
    // On the way to the far generator, and well over 1,000 from where that missile lands.
    let defense = structure(&mut world, "strategic_missile_defense", Team::Red, 120, 20);
    world.ecs.get::<&mut MissileDefense>(world.unit_entity(defense).unwrap()).unwrap().stock = 1;
    world.advance_tick(&[SimCommand::Attack { units: vec![silo], target: far }]);
    run_until(&mut world, 500, |_| false);
    assert!(alive(&world, far), "it should have been shot down on the way");
    assert_eq!(world.ecs.get::<&MissileDefense>(world.unit_entity(defense).unwrap()).unwrap().stock, 0);

    // One far off to the side never sees it.
    let (mut world, silo, _, far) = nuclear_standoff();
    let aside = structure(&mut world, "strategic_missile_defense", Team::Red, 120, 200);
    world.ecs.get::<&mut MissileDefense>(world.unit_entity(aside).unwrap()).unwrap().stock = 1;
    world.advance_tick(&[SimCommand::Attack { units: vec![silo], target: far }]);
    assert!(run_until(&mut world, 500, |world| !alive(world, far)).is_some(), "out of its range, it should land");
    assert_eq!(world.ecs.get::<&MissileDefense>(world.unit_entity(aside).unwrap()).unwrap().stock, 1);
}
