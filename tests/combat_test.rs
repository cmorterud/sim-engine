//! Phase 3 collision tests: swept hits, terrain masking, dodging, and friendly fire.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp, Vec3Fp};
use sim_engine::math::rect::RectFp;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::combat::firing_solution;
use sim_engine::sim::components::{Combat, Position, Projectile, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::scenario::{self, standard_unit};
use sim_engine::sim::world::SimWorld;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn flat_world(friendly_fire: bool) -> SimWorld {
    SimWorld::new(
        SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, friendly_fire, ..SimConfig::default() },
        1,
    )
}

/// Spawns a unit that does not shoot back, so tests only see the projectile under test.
fn passive_unit(world: &mut SimWorld, team: Team, at: Vec2Fp) -> SimId {
    let id = world.spawn_unit(standard_unit(team, at));
    let entity = world.unit_entity(id).unwrap();
    world.ecs.get::<&mut Combat>(entity).unwrap().range = Fp::ZERO;
    id
}

fn shoot(world: &mut SimWorld, from: Vec2Fp, to: Vec2Fp, speed: Fp, source: SimId, team: Team) {
    let z = |w: &SimWorld, p: Vec2Fp| w.terrain.height_at(p) + fp_int(1);
    let start = Vec3Fp::new(from.x, from.y, z(world, from));
    let end = Vec3Fp::new(to.x, to.y, z(world, to));
    let solution = firing_solution(start, end, speed);
    world.spawn_projectile(
        start,
        Projectile {
            velocity: solution.velocity,
            damage: fp_int(10),
            source,
            source_team: team,
            lifetime_ticks: solution.flight_ticks + 20,
        },
    );
}

fn run_until_projectiles_gone(world: &mut SimWorld, limit: u32) {
    for _ in 0..limit {
        world.advance_tick(&[]);
        if world.projectiles.is_empty() {
            return;
        }
    }
    panic!("projectiles still alive after {limit} ticks");
}

#[test]
fn fast_shell_cannot_pass_through_a_unit_between_ticks() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let target = passive_unit(&mut world, Team::Red, point(225, 100));
    // 50 units per tick against a 2-unit-wide target: endpoint checks would land at
    // 150, 200, and 250, all far from the target at 225.
    shoot(&mut world, point(100, 100), point(400, 100), fp_int(50), source, Team::Blue);
    run_until_projectiles_gone(&mut world, 40);
    assert_eq!(world.unit_health(target), Some(fp_int(90)));
}

#[test]
fn wall_blocks_a_low_shot() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let target = passive_unit(&mut world, Team::Red, point(200, 100));
    world.add_box_obstacle(RectFp::new(point(140, 60), point(160, 140)));
    shoot(&mut world, point(100, 100), point(200, 100), fp_int(20), source, Team::Blue);
    run_until_projectiles_gone(&mut world, 40);
    assert_eq!(world.unit_health(target), Some(fp_int(100)));
}

#[test]
fn high_arc_clears_the_same_wall() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let target = passive_unit(&mut world, Team::Red, point(200, 100));
    world.add_box_obstacle(RectFp::new(point(140, 60), point(160, 140)));
    // A slow shell flies for 200 ticks, which puts the apex well above the 12-unit wall.
    shoot(&mut world, point(100, 100), point(200, 100), Fp::lit("0.5"), source, Team::Blue);
    run_until_projectiles_gone(&mut world, 400);
    assert_eq!(world.unit_health(target), Some(fp_int(90)));
}

#[test]
fn moving_unit_dodges_a_slow_shell() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let target = passive_unit(&mut world, Team::Red, point(200, 100));
    shoot(&mut world, point(100, 100), point(200, 100), Fp::lit("1"), source, Team::Blue);
    world.advance_tick(&[SimCommand::Move { units: vec![target], goal: point(200, 400) }]);
    run_until_projectiles_gone(&mut world, 300);
    assert_eq!(world.unit_health(target), Some(fp_int(100)));
}

#[test]
fn stationary_unit_is_hit_by_the_same_slow_shell() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let target = passive_unit(&mut world, Team::Red, point(200, 100));
    shoot(&mut world, point(100, 100), point(200, 100), Fp::lit("1"), source, Team::Blue);
    run_until_projectiles_gone(&mut world, 300);
    assert_eq!(world.unit_health(target), Some(fp_int(90)));
}

#[test]
fn friendly_fire_setting_controls_allied_hits() {
    for (friendly_fire, expected) in [(false, 100), (true, 90)] {
        let mut world = flat_world(friendly_fire);
        let source = passive_unit(&mut world, Team::Blue, point(100, 100));
        let ally = passive_unit(&mut world, Team::Blue, point(150, 100));
        shoot(&mut world, point(100, 100), point(150, 100), fp_int(5), source, Team::Blue);
        run_until_projectiles_gone(&mut world, 60);
        assert_eq!(world.unit_health(ally), Some(fp_int(expected)), "friendly_fire = {friendly_fire}");
        assert_eq!(world.unit_health(source), Some(fp_int(100)), "the firing unit must never hit itself");
    }
}

#[test]
fn units_acquire_the_nearest_enemy_and_kill_it() {
    let mut world = flat_world(false);
    let shooter = world.spawn_unit(standard_unit(Team::Blue, point(100, 100)));
    let near = passive_unit(&mut world, Team::Red, point(115, 100));
    let far = passive_unit(&mut world, Team::Red, point(125, 100));
    for _ in 0..600 {
        world.advance_tick(&[]);
        if world.unit_position(near).is_none() {
            break;
        }
    }
    assert!(world.unit_position(near).is_none(), "the nearest enemy should have died");
    assert!(world.unit_position(far).is_some());
    assert_eq!(world.unit_health(far), Some(fp_int(100)));
    assert!(world.unit_position(shooter).is_some());
}

#[test]
fn projectile_leaving_the_map_is_removed() {
    let mut world = flat_world(false);
    let source = passive_unit(&mut world, Team::Blue, point(100, 100));
    let start = Vec3Fp::new(fp_int(5_100), fp_int(100), fp_int(50));
    let velocity = Vec3Fp::new(fp_int(30), Fp::ZERO, fp_int(5));
    world.spawn_projectile(start, Projectile { velocity, damage: fp_int(1), source, source_team: Team::Blue, lifetime_ticks: 1_000 });
    world.advance_tick(&[]);
    world.advance_tick(&[]);
    assert!(world.projectiles.is_empty());
}

#[test]
fn battle_on_a_maximum_size_map_runs_to_casualties() {
    let scenario = scenario::battle(3, 200, SimConfig::default());
    let opening = scenario.opening_commands;
    let mut world = scenario.world;
    world.advance_tick(&opening);
    for _ in 0..600 {
        world.advance_tick(&[]);
    }
    assert!(world.units.len() < 400);
    for &(_, entity) in world.units.iter().take(50) {
        let z = world.ecs.get::<&Position>(entity).unwrap().0.z;
        assert!(z.abs() < fp_int(200), "unit height {z} looks wrong");
    }
}
