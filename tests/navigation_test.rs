//! Phase 2 gate: an army crosses walls through their choke points on a maximum-size map.

use sim_engine::math::fp::{fp_int, sqrt_wide, Fp, Vec2Fp};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::OrderKind;
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::scenario::{self, standard_unit};
use sim_engine::sim::components::Team;
use sim_engine::sim::world::{SimWorld, UnitSpawn};

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// Runs the navigation gate and returns the tick when 95% of units had arrived.
fn run_navigation_gate(unit_count: usize) -> u32 {
    let (mut scenario, layout) = scenario::navigation(7, unit_count);
    let ideal_ticks = layout.route_units / layout.unit_speed.to_num::<i32>();
    let limit = ideal_ticks as u32 * 3;
    let required = unit_count * 95 / 100;
    let close = fp_int(128);
    let commands = std::mem::take(&mut scenario.opening_commands);
    let mut world = scenario.world;
    world.advance_tick(&commands);

    for tick in 1..=limit {
        world.advance_tick(&[]);
        let mut arrived = 0;
        for &(id, _) in &world.units {
            let position = world.unit_position(id).unwrap().xy();
            assert!(world.terrain.is_passable(position), "unit {id:?} is inside a wall on tick {tick}");
            if sqrt_wide(position.distance_squared(&layout.goal)) <= close {
                arrived += 1;
            }
        }
        if arrived >= required {
            println!("{arrived}/{unit_count} arrived on tick {tick} (ideal {ideal_ticks}, limit {limit})");
            return tick;
        }
    }
    let positions: Vec<_> = world.units.iter().take(5).map(|&(id, _)| world.unit_position(id).unwrap()).collect();
    panic!("fewer than 95% arrived within {limit} ticks (ideal {ideal_ticks}); samples: {positions:?}");
}

#[test]
fn small_army_passes_the_navigation_gate() {
    run_navigation_gate(300);
}

/// The Phase 2 gate at full size. Release builds only, for run time.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn full_army_passes_the_navigation_gate() {
    run_navigation_gate(5_000);
}

fn flat_world() -> SimWorld {
    SimWorld::new(SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, ..SimConfig::default() }, 1)
}

#[test]
fn units_arrive_and_become_idle() {
    let mut world = flat_world();
    let unit = world.spawn_unit(standard_unit(Team::Blue, point(100, 100)));
    world.advance_tick(&[SimCommand::Move { units: vec![unit], goal: point(200, 100) }]);
    for _ in 0..200 {
        world.advance_tick(&[]);
    }
    assert_eq!(world.unit_order(unit).unwrap().kind, OrderKind::Idle);
    assert!(sqrt_wide(world.unit_position(unit).unwrap().xy().distance_squared(&point(200, 100))) < fp_int(4));
    assert_eq!(world.flow_field.route_count(), 0, "finished routes should be freed");
}

#[test]
fn overlapping_units_separate() {
    let mut world = flat_world();
    let a = world.spawn_unit(standard_unit(Team::Blue, point(100, 100)));
    let b = world.spawn_unit(standard_unit(Team::Blue, point(100, 100)));
    for _ in 0..30 {
        world.advance_tick(&[]);
    }
    let (pa, pb) = (world.unit_position(a).unwrap().xy(), world.unit_position(b).unwrap().xy());
    assert!(sqrt_wide(pa.distance_squared(&pb)) >= Fp::lit("1.9"), "units still overlap: {pa:?} {pb:?}");
}

#[test]
fn followers_track_a_moving_target() {
    let mut world = flat_world();
    // The leader moves at half speed, so that its follower can catch up.
    let leader = world.spawn_unit(UnitSpawn { max_speed: Fp::lit("0.5"), ..standard_unit(Team::Blue, point(300, 300)) });
    let follower = world.spawn_unit(standard_unit(Team::Blue, point(100, 300)));
    world.advance_tick(&[
        SimCommand::Move { units: vec![leader], goal: point(300, 2_500) },
        SimCommand::Follow { units: vec![follower], target: leader },
    ]);
    let start = world.unit_position(follower).unwrap().xy();
    for _ in 0..1_500 {
        world.advance_tick(&[]);
    }
    let (leader_at, follower_at) = (world.unit_position(leader).unwrap().xy(), world.unit_position(follower).unwrap().xy());
    assert!(sqrt_wide(follower_at.distance_squared(&start)) > fp_int(700), "follower barely moved");
    assert!(sqrt_wide(follower_at.distance_squared(&leader_at)) < fp_int(60), "follower lost its target: {follower_at:?} {leader_at:?}");
}
