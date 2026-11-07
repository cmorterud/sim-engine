//! Missiles and missile defense (GAME_SPECIFICATION.md §8): tactical missiles and the defenses
//! that shoot them down, and nuclear missiles and the silos that stop them.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::math::rect::RectFp;
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Combat, Factory, Launcher, MissileDefense, Shield, SimId, Team};
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
        for capacity in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.mass] {
            *capacity = Res::from_num(100_000);
        }
        for capacity in [&mut economy.base_energy_capacity, &mut economy.energy_capacity, &mut economy.energy] {
            *capacity = Res::from_num(1_000_000);
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

fn missiles_from(world: &SimWorld, source: SimId) -> usize {
    world
        .missiles
        .iter()
        .filter(|&&(_, entity)| world.ecs.get::<&sim_engine::sim::components::Projectile>(entity).is_ok_and(|projectile| projectile.source == source))
        .count()
}

#[test]
fn a_tactical_launcher_destroys_an_enemy_structure_it_can_see_but_ignores_units() {
    let mut world = open_world();
    let launcher = structure(&mut world, "tactical_missile_launcher", Team::Blue, 20, 20);
    // A bot in range is no target for it.
    let bot = world.blueprints.id("t1_light_bot");
    let bot = world.spawn_mobile(bot, Team::Red, point(800, 410));
    world.ecs.get::<&mut Combat>(world.unit_entity(bot).unwrap()).unwrap().range = Fp::ZERO;
    assert!(run_until(&mut world, 60, |world| missiles_from(world, launcher) > 0).is_none(), "it fired at a unit");

    // A power generator 1,600 away is, and one 1,500-damage missile is enough.
    let target = structure(&mut world, "power_generator", Team::Red, 100, 20);
    let took = run_until(&mut world, 300, |world| !alive(world, target)).expect("the missile should destroy it");
    // About 90 ticks of flight at 18 a tick, plus the climb, and the search.
    assert!(took > 60, "it arrived after only {took} ticks");
    assert!(alive(&world, bot));
}

#[test]
fn a_missile_flies_over_a_wall_that_would_stop_a_shell() {
    let mut world = open_world();
    let launcher = structure(&mut world, "tactical_missile_launcher", Team::Blue, 20, 20);
    world.add_box_obstacle(RectFp::new(point(900, 0), point(940, 1_200)));
    let target = structure(&mut world, "power_generator", Team::Red, 60, 20);
    world.advance_tick(&[SimCommand::Attack { units: vec![launcher], target }]);
    assert!(run_until(&mut world, 300, |world| !alive(world, target)).is_some(), "the wall should not stop it");
}

#[test]
fn a_missile_defense_shoots_down_one_missile_at_a_time() {
    let mut world = open_world();
    let first = structure(&mut world, "tactical_missile_launcher", Team::Blue, 20, 20);
    let target = structure(&mut world, "power_generator", Team::Red, 100, 20);
    structure(&mut world, "tactical_missile_defense", Team::Red, 102, 20);
    let mut intercepted = 0;
    for _ in 0..200 {
        world.advance_tick(&[]);
        intercepted += world.snapshot_for(Some(Team::Red)).interceptions.len();
    }
    assert_eq!(intercepted, 1, "one missile, shot down once");
    assert!(alive(&world, target), "the defense should have stopped it");
    // The launcher has reloaded by now, and its second missile is on its way.
    assert!(missiles_from(&world, first) <= 1);

    // Two launchers fire together, and the defense, reloading after the first, misses the second.
    let mut world = open_world();
    for cell_y in [20, 22] {
        structure(&mut world, "tactical_missile_launcher", Team::Blue, 20, cell_y);
    }
    let target = structure(&mut world, "power_generator", Team::Red, 100, 21);
    structure(&mut world, "tactical_missile_defense", Team::Red, 102, 21);
    assert!(run_until(&mut world, 300, |world| !alive(world, target)).is_some(), "a volley should get one through");
}

#[test]
fn a_shield_stops_a_tactical_missile() {
    let mut world = open_world();
    structure(&mut world, "tactical_missile_launcher", Team::Blue, 20, 20);
    let target = structure(&mut world, "power_generator", Team::Red, 100, 20);
    let generator = structure(&mut world, "shield_generator", Team::Red, 101, 22);
    // No healing, so that the dome's loss is exactly what hit it.
    world.ecs.get::<&mut Shield>(world.unit_entity(generator).unwrap()).unwrap().regen_per_tick = Fp::ZERO;
    run_until(&mut world, 140, |_| false);
    assert!(alive(&world, target));
    let shield = *world.ecs.get::<&Shield>(world.unit_entity(generator).unwrap()).unwrap();
    assert_eq!(shield.max_health - shield.health, fp_int(1_500), "the dome took the whole missile");
}

#[test]
fn a_silo_builds_missiles_and_keeps_them() {
    let mut world = open_world();
    let silo = structure(&mut world, "strategic_missile_launcher", Team::Blue, 20, 20);
    let entity = world.unit_entity(silo).unwrap();
    // Far more build power than it has, so that the test does not wait 200 seconds.
    world.ecs.get::<&mut Factory>(entity).unwrap().power = fp_int(4_000);
    let missile = world.blueprints.id("strategic_missile");
    let units_before = world.units.len();
    world.advance_tick(&[SimCommand::QueueUnit { factory: silo, blueprint: missile, count: 2 }]);
    run_until(&mut world, 40, |_| false);
    assert_eq!(world.ecs.get::<&Launcher>(entity).unwrap().stock, 2);
    assert_eq!(world.units.len(), units_before, "a missile is not a unit");
    assert_eq!(world.snapshot_for(Some(Team::Blue)).stocks, [(silo, 2)]);
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
    let far = structure(&mut world, "power_generator", Team::Red, 210, 20);
    (world, silo, base, far)
}

#[test]
fn a_nuclear_missile_flattens_everything_in_its_blast_once_per_order() {
    let (mut world, silo, base, far) = nuclear_standoff();
    // A shield over the base does not stop it.
    structure(&mut world, "shield_generator", Team::Red, 172, 24);
    // Nothing is fired until it is ordered.
    assert!(run_until(&mut world, 30, |world| missiles_from(world, silo) > 0).is_none());
    let spot = point(3_440, 450);
    world.advance_tick(&[SimCommand::AttackGround { units: vec![silo], point: spot }]);
    assert!(run_until(&mut world, 500, |world| base.iter().all(|id| !alive(world, *id))).is_some(), "the blast should flatten the base");
    assert!(alive(&world, far), "800 away is outside the blast");
    let entity = world.unit_entity(silo).unwrap();
    assert_eq!(world.ecs.get::<&Launcher>(entity).unwrap().stock, 1, "one order, one missile");
    assert_eq!(world.ecs.get::<&Combat>(entity).unwrap().ground_target, None);
    assert!(run_until(&mut world, 200, |world| missiles_from(world, silo) > 0).is_none(), "it waits for the next order");
}

#[test]
fn a_strategic_defense_spends_an_interceptor_on_a_missile_aimed_inside_its_range() {
    let (mut world, silo, base, _) = nuclear_standoff();
    let defense = structure(&mut world, "strategic_missile_defense", Team::Red, 165, 30);
    let defense_entity = world.unit_entity(defense).unwrap();
    world.ecs.get::<&mut MissileDefense>(defense_entity).unwrap().stock = 1;
    let spot = point(3_440, 450);
    world.advance_tick(&[SimCommand::AttackGround { units: vec![silo], point: spot }]);
    run_until(&mut world, 500, |_| false);
    assert!(base.iter().all(|id| alive(&world, *id)), "the interceptor should have stopped it");
    assert_eq!(world.ecs.get::<&MissileDefense>(defense_entity).unwrap().stock, 0);

    // With no interceptor left, the second one lands.
    world.advance_tick(&[SimCommand::AttackGround { units: vec![silo], point: spot }]);
    assert!(run_until(&mut world, 500, |world| base.iter().all(|id| !alive(world, *id))).is_some());
}

#[test]
fn a_strategic_defense_lets_a_missile_aimed_elsewhere_fly_past() {
    let (mut world, silo, _, far) = nuclear_standoff();
    // On the way to the far generator, but more than its range from where that missile lands.
    let defense = structure(&mut world, "strategic_missile_defense", Team::Red, 120, 20);
    world.ecs.get::<&mut MissileDefense>(world.unit_entity(defense).unwrap()).unwrap().stock = 1;
    world.advance_tick(&[SimCommand::Attack { units: vec![silo], target: far }]);
    assert!(run_until(&mut world, 500, |world| !alive(world, far)).is_some(), "it should not be intercepted");
    assert_eq!(world.ecs.get::<&MissileDefense>(world.unit_entity(defense).unwrap()).unwrap().stock, 1);
}
