//! Phase 2 gate: an army crosses walls through their choke points on a maximum-size map.

use sim_engine::math::fp::{fp_int, sqrt_wide, Fp, Vec2Fp};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::OrderKind;
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::scenario::{self, standard_unit};
use sim_engine::sim::components::Team;
use sim_engine::math::rect::RectFp as RectRange;
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

/// Units sent one at a time to a shared point, like a factory's rally point, must all
/// settle. Otherwise they push at the crowd forever and never become idle.
#[test]
fn units_sent_one_at_a_time_to_a_shared_point_all_settle() {
    let mut world = flat_world();
    let goal = point(1_000, 1_000);
    let mut units = Vec::new();
    for index in 0..60 {
        let unit = world.spawn_unit(UnitSpawn { radius: fp_int(7), ..standard_unit(Team::Blue, point(600 + index * 20, 700)) });
        units.push(unit);
        world.advance_tick(&[SimCommand::Move { units: vec![unit], goal }]);
    }
    for _ in 0..800 {
        world.advance_tick(&[]);
    }
    let still_moving = units.iter().filter(|unit| world.unit_order(**unit).unwrap().kind != OrderKind::Idle).count();
    assert_eq!(still_moving, 0, "{still_moving} of 60 units never settled");
    assert_eq!(world.flow_field.route_count(), 0, "finished routes should be freed");
}

/// Standard units on a flat map, in a loose bunch around a point.
fn bunch(world: &mut SimWorld, count: i32, x: i32, y: i32) -> Vec<sim_engine::sim::components::SimId> {
    (0..count)
        .map(|index| {
            let spawn = UnitSpawn { radius: fp_int(5), max_speed: fp_int(3), ..standard_unit(Team::Blue, point(x + index % 6 * 25, y + index / 6 * 25)) };
            world.spawn_unit(spawn)
        })
        .collect()
}

fn run_until_idle(world: &mut SimWorld, units: &[sim_engine::sim::components::SimId], limit: u32) -> usize {
    for _ in 0..limit {
        world.advance_tick(&[]);
        if units.iter().all(|unit| world.unit_order(*unit).unwrap().kind == OrderKind::Idle) {
            return 0;
        }
    }
    units.iter().filter(|unit| world.unit_order(**unit).unwrap().kind != OrderKind::Idle).count()
}

#[test]
fn a_group_order_ends_in_an_orderly_formation_at_the_goal() {
    let mut world = flat_world();
    let units = bunch(&mut world, 30, 300, 300);
    let goal = point(1_500, 1_500);
    world.advance_tick(&[SimCommand::Move { units: units.clone(), goal }]);
    assert_eq!(run_until_idle(&mut world, &units, 2_000), 0, "every unit should settle");
    assert_eq!(world.flow_field.route_count(), 0, "finished routes should be freed");

    let positions: Vec<Vec2Fp> = units.iter().map(|unit| world.unit_position(*unit).unwrap().xy()).collect();
    // Evenly spread: no blob, and everyone close to a spot 12.5 units from the next.
    for (index, position) in positions.iter().enumerate() {
        for other in &positions[index + 1..] {
            assert!(sqrt_wide(position.distance_squared(other)) >= fp_int(10), "two units ended up {} apart", sqrt_wide(position.distance_squared(other)));
        }
        assert!(sqrt_wide(position.distance_squared(&goal)) <= fp_int(80), "a unit ended up far from the goal at {position:?}");
    }
    let count = positions.len() as i32;
    let mean_x: i32 = positions.iter().map(|position| position.x.to_num::<i32>()).sum::<i32>() / count;
    let mean_y: i32 = positions.iter().map(|position| position.y.to_num::<i32>()).sum::<i32>() / count;
    assert!((mean_x - 1_500).abs() <= 5 && (mean_y - 1_500).abs() <= 5, "the formation should center on the goal: ({mean_x}, {mean_y})");
}

#[test]
fn spots_on_a_wall_move_to_open_ground_and_nobody_gets_stuck() {
    let mut world = flat_world();
    let units = bunch(&mut world, 30, 300, 300);
    let goal = point(1_500, 1_500);
    // A wall through the middle of where the formation would stand.
    world.add_box_obstacle(RectRange::new(point(1_400, 1_480), point(1_600, 1_520)));
    world.advance_tick(&[SimCommand::Move { units: units.clone(), goal }]);
    assert_eq!(run_until_idle(&mut world, &units, 3_000), 0, "every unit should settle");
    for unit in &units {
        assert!(world.terrain.is_passable(world.unit_position(*unit).unwrap().xy()), "a unit is inside the wall");
    }
}

#[test]
fn a_spot_sealed_off_by_walls_cannot_trap_a_group_forever() {
    let mut world = flat_world();
    let units = bunch(&mut world, 12, 300, 300);
    let goal = point(1_500, 1_500);
    // A box around one end of the formation, with its spots still technically open.
    world.add_box_obstacle(RectRange::new(point(1_495, 1_420), point(1_505, 1_580)));
    world.advance_tick(&[SimCommand::AttackMove { units: units.clone(), goal }]);
    assert_eq!(run_until_idle(&mut world, &units, 3_000), 0, "units must give up on spots they cannot reach");
}

#[test]
fn a_single_unit_still_goes_straight_to_the_click() {
    let mut world = flat_world();
    let unit = world.spawn_unit(standard_unit(Team::Blue, point(100, 100)));
    world.advance_tick(&[SimCommand::Move { units: vec![unit], goal: point(300, 100) }]);
    assert_eq!(world.unit_order(unit).unwrap().slot, None);
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
