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

/// Walls in a closed ring of cells, from one corner cell to the other, open inside.
fn seal_off(world: &mut SimWorld, low: (u32, u32), high: (u32, u32)) {
    for x in low.0..=high.0 {
        world.terrain.set_structure_cells(x, low.1, 1, true);
        world.terrain.set_structure_cells(x, high.1, 1, true);
    }
    for y in low.1..=high.1 {
        world.terrain.set_structure_cells(low.0, y, 1, true);
        world.terrain.set_structure_cells(high.0, y, 1, true);
    }
    world.flow_field.invalidate();
}

/// The middle of a terrain cell.
fn cell_point(x: i32, y: i32) -> Vec2Fp {
    point(x * 20 + 10, y * 20 + 10)
}

fn cancelled(world: &SimWorld) -> u32 {
    world.snapshot_for(Some(Team::Blue)).orders_cancelled
}

fn travelled(world: &SimWorld, unit: sim_engine::sim::components::SimId, start: Vec2Fp) -> i64 {
    sqrt_wide(world.unit_position(unit).unwrap().xy().distance_squared(&start)).to_num::<i64>()
}

#[test]
fn a_move_into_a_sealed_area_is_cancelled_straight_away() {
    let mut world = flat_world();
    seal_off(&mut world, (100, 100), (110, 110));
    let start = point(300, 300);
    let unit = world.spawn_unit(standard_unit(Team::Blue, start));
    world.advance_tick(&[SimCommand::Move { units: vec![unit], goal: cell_point(105, 105) }]);
    for _ in 0..20 {
        world.advance_tick(&[]);
    }
    assert_eq!(world.unit_order(unit).unwrap().kind, OrderKind::Idle, "it should give up");
    assert!(travelled(&world, unit, start) < 10, "it should not wander towards the walls");
    assert_eq!(world.flow_field.route_count(), 0, "the route should be handed back");
    assert_eq!(cancelled(&world), 1, "the player should be told");
    assert_eq!(world.snapshot_for(Some(Team::Red)).orders_cancelled, 0, "only the team that lost the order");
}

#[test]
fn a_unit_inside_a_sealed_area_cannot_be_sent_out() {
    let mut world = flat_world();
    seal_off(&mut world, (100, 100), (110, 110));
    let unit = world.spawn_unit(standard_unit(Team::Blue, cell_point(105, 105)));
    world.advance_tick(&[SimCommand::Move { units: vec![unit], goal: point(300, 300) }]);
    for _ in 0..20 {
        world.advance_tick(&[]);
    }
    assert_eq!(world.unit_order(unit).unwrap().kind, OrderKind::Idle);
    assert_eq!(cancelled(&world), 1);
}

#[test]
fn only_the_units_that_cannot_reach_a_group_order_are_cancelled() {
    let mut world = flat_world();
    seal_off(&mut world, (100, 100), (110, 110));
    let trapped = world.spawn_unit(standard_unit(Team::Blue, cell_point(105, 105)));
    let free = bunch(&mut world, 6, 300, 300);
    let mut everyone = free.clone();
    everyone.push(trapped);
    world.advance_tick(&[SimCommand::Move { units: everyone.clone(), goal: point(800, 300) }]);
    assert_eq!(run_until_idle(&mut world, &everyone, 3_000), 0, "nobody may be left waiting forever");
    for unit in &free {
        assert!(sqrt_wide(world.unit_position(*unit).unwrap().xy().distance_squared(&point(800, 300))) < fp_int(80), "{unit:?} should have arrived");
    }
    assert_eq!(cancelled(&world), 1, "only the trapped unit's order");
}

#[test]
fn a_long_way_round_is_still_taken_and_never_cancelled() {
    let mut world = flat_world();
    // A long wall with one gap at the far end.
    for y in 0..120 {
        world.terrain.set_structure_cells(60, y, 1, true);
    }
    world.flow_field.invalidate();
    let unit = world.spawn_unit(standard_unit(Team::Blue, cell_point(50, 20)));
    let goal = cell_point(70, 20);
    world.advance_tick(&[SimCommand::Move { units: vec![unit], goal }]);
    assert_eq!(run_until_idle(&mut world, &[unit], 5_000), 0);
    assert!(sqrt_wide(world.unit_position(unit).unwrap().xy().distance_squared(&goal)) < fp_int(12), "it should have gone round");
    assert_eq!(cancelled(&world), 0);
}

#[test]
fn an_unreachable_queued_stop_is_skipped_and_the_next_one_taken() {
    let mut world = flat_world();
    seal_off(&mut world, (100, 100), (110, 110));
    let unit = world.spawn_unit(standard_unit(Team::Blue, point(300, 300)));
    let reachable = point(800, 300);
    world.advance_tick(&[
        SimCommand::Move { units: vec![unit], goal: cell_point(105, 105) },
        SimCommand::QueueMove { units: vec![unit], goal: reachable, attack: false },
    ]);
    // Between one order and the next, a unit is idle for a tick, so wait for the queue too.
    let mut ticks = 0;
    while world.unit_order(unit).unwrap().kind != OrderKind::Idle || !world.order_queues.is_empty() {
        world.advance_tick(&[]);
        ticks += 1;
        assert!(ticks < 3_000, "it never finished");
    }
    assert!(sqrt_wide(world.unit_position(unit).unwrap().xy().distance_squared(&reachable)) < fp_int(12));
    assert_eq!(cancelled(&world), 1);
    assert_eq!(world.flow_field.route_count(), 0);
}

#[test]
fn a_patrol_with_an_unreachable_point_ends() {
    let mut world = flat_world();
    seal_off(&mut world, (100, 100), (110, 110));
    let unit = world.spawn_unit(standard_unit(Team::Blue, point(300, 300)));
    world.advance_tick(&[SimCommand::Patrol { units: vec![unit], points: vec![cell_point(105, 105), point(800, 300)] }]);
    for _ in 0..40 {
        world.advance_tick(&[]);
    }
    assert!(world.patrols.is_empty(), "trying again forever would only waste time");
    assert_eq!(world.unit_order(unit).unwrap().kind, OrderKind::Idle);
}

#[test]
fn following_a_unit_that_cannot_be_reached_is_cancelled() {
    let mut world = flat_world();
    seal_off(&mut world, (100, 100), (110, 110));
    let leader = world.spawn_unit(standard_unit(Team::Blue, cell_point(105, 105)));
    let follower = world.spawn_unit(standard_unit(Team::Blue, point(300, 300)));
    world.advance_tick(&[SimCommand::Follow { units: vec![follower], target: leader }]);
    for _ in 0..40 {
        world.advance_tick(&[]);
    }
    assert_eq!(world.unit_order(follower).unwrap().kind, OrderKind::Idle);
    assert_eq!(cancelled(&world), 1);
}

#[test]
fn cancelling_unreachable_orders_replays_identically() {
    let build = || {
        let mut world = flat_world();
        seal_off(&mut world, (100, 100), (110, 110));
        let units = bunch(&mut world, 8, 300, 300);
        (world, units)
    };
    let (mut first, units) = build();
    let (mut second, _) = build();
    let orders = [SimCommand::Move { units: units.clone(), goal: cell_point(105, 105) }];
    first.advance_tick(&orders);
    second.advance_tick(&orders);
    for _ in 0..200 {
        first.advance_tick(&[]);
        second.advance_tick(&[]);
        assert_eq!(first.compute_checksum(), second.compute_checksum());
    }
    assert_eq!(cancelled(&first), 8);
}

/// Bots, tanks, and artillery in a block: fast, medium, and slow.
fn mixed_group(world: &mut SimWorld) -> Vec<sim_engine::sim::components::SimId> {
    let kinds = ["t1_light_bot", "t2_heavy_tank", "t1_light_artillery"];
    (0..12)
        .map(|index| {
            let blueprint = world.blueprints.id(kinds[index % 3]);
            let at = point(500 + (index as i32 % 4) * 30, 500 + (index as i32 / 4) * 30);
            world.spawn_mobile(blueprint, Team::Blue, at)
        })
        .collect()
}

/// The furthest any unit is from the group's middle, in whole units.
fn spread(world: &SimWorld, units: &[sim_engine::sim::components::SimId]) -> i32 {
    let positions: Vec<(i64, i64)> = units
        .iter()
        .filter_map(|id| world.unit_position(*id))
        .map(|position| (i64::from(position.x.to_num::<i32>()), i64::from(position.y.to_num::<i32>())))
        .collect();
    let count = positions.len() as i64;
    let middle = positions.iter().fold((0, 0), |sum, p| (sum.0 + p.0, sum.1 + p.1));
    let middle = (middle.0 / count, middle.1 / count);
    positions.iter().map(|p| ((p.0 - middle.0).pow(2) + (p.1 - middle.1).pow(2)).isqrt() as i32).max().unwrap_or(0)
}

/// Gives the order, then returns the widest the group got on the way and when everyone had arrived.
fn travel(world: &mut SimWorld, units: &[sim_engine::sim::components::SimId], order: SimCommand) -> (i32, u32) {
    world.advance_tick(&[order]);
    let mut widest = 0;
    for tick in 1..2_000 {
        world.advance_tick(&[]);
        // Forming up at the start takes a few seconds.
        if tick > 100 {
            widest = widest.max(spread(world, units));
        }
        if units.iter().all(|unit| world.unit_order(*unit).unwrap().kind == OrderKind::Idle) {
            return (widest, tick);
        }
    }
    panic!("the group never arrived");
}

#[test]
fn a_formation_move_keeps_a_mixed_group_together_at_the_slowest_pace() {
    let goal = point(2_000, 1_200);
    let mut plain = flat_world();
    let units = mixed_group(&mut plain);
    let (plain_spread, plain_ticks) = travel(&mut plain, &units, SimCommand::Move { units: units.clone(), goal });

    let mut world = flat_world();
    let units = mixed_group(&mut world);
    let (formation_spread, formation_ticks) =
        travel(&mut world, &units, SimCommand::FormationMove { units: units.clone(), goal, attack: false, queued: false });

    // Bots at 5.5 a tick leave artillery at 3.5 behind on a plain move.
    assert!(plain_spread > 250, "a plain move spread only {plain_spread}");
    assert!(formation_spread < 100, "the formation spread out to {formation_spread}");
    // A little slower overall, since everyone keeps the artillery's pace.
    assert!(formation_ticks < plain_ticks * 3 / 2, "{formation_ticks} ticks against {plain_ticks}");
    assert!(world.formations.is_empty(), "a finished formation is forgotten");
    assert_eq!(world.flow_field.route_count(), 0);
}

#[test]
fn a_formation_goes_round_a_wall_and_everyone_arrives() {
    let mut world = flat_world();
    world.add_box_obstacle(RectRange::new(point(1_000, 200), point(1_040, 1_400)));
    world.prepare_map();
    let units = mixed_group(&mut world);
    let goal = point(1_600, 700);
    world.advance_tick(&[SimCommand::FormationMove { units: units.clone(), goal, attack: false, queued: false }]);
    for tick in 0..2_500 {
        world.advance_tick(&[]);
        for &unit in &units {
            assert!(world.terrain.is_passable(world.unit_position(unit).unwrap().xy()), "a unit went into the wall on tick {tick}");
        }
    }
    let still_moving = units.iter().filter(|unit| world.unit_order(**unit).unwrap().kind != OrderKind::Idle).count();
    assert_eq!(still_moving, 0);
    for &unit in &units {
        let at = world.unit_position(unit).unwrap().xy();
        assert!(sqrt_wide(at.distance_squared(&goal)) < fp_int(150), "{unit:?} ended at {at:?}");
    }
}

#[test]
fn a_formation_attack_move_stops_the_whole_group_to_fight() {
    let mut world = flat_world();
    let units = mixed_group(&mut world);
    // A tough enemy that does not fire back, ahead and to one side of the route.
    let enemy = world.spawn_unit(UnitSpawn { health: fp_int(1_000_000), range: Fp::ZERO, ..standard_unit(Team::Red, point(1_100, 1_200)) });
    world.advance_tick(&[SimCommand::FormationMove { units: units.clone(), goal: point(3_000, 2_000), attack: true, queued: false }]);
    // Two minutes: far longer than a formation waits for stragglers, so only the fight can hold it.
    for _ in 0..1_200 {
        world.advance_tick(&[]);
    }
    let fighting = units.iter().filter(|unit| {
        let entity = world.unit_entity(**unit).unwrap();
        world.ecs.get::<&sim_engine::sim::components::Combat>(entity).unwrap().target == Some(enemy)
    });
    assert!(fighting.count() > 0, "nobody is fighting");
    let leader = world.formations.values().next().expect("still on its way").leader;
    let enemy_at = world.unit_position(enemy).unwrap().xy();
    assert!(sqrt_wide(leader.distance_squared(&enemy_at)) < fp_int(600), "the group went on without the fight: {leader:?}");
}

#[test]
fn a_queued_formation_waits_for_everyone_to_finish_first() {
    let mut world = flat_world();
    let units = mixed_group(&mut world);
    let (early, late) = units.split_at(6);
    // Half the group has a long walk to do first.
    world.advance_tick(&[
        SimCommand::Move { units: early.to_vec(), goal: point(600, 600) },
        SimCommand::Move { units: late.to_vec(), goal: point(600, 1_600) },
    ]);
    world.advance_tick(&[SimCommand::FormationMove { units: units.clone(), goal: point(2_000, 1_000), attack: false, queued: true }]);
    let start = world.formations.values().next().expect("a formation was planned").leader;
    for _ in 0..100 {
        world.advance_tick(&[]);
    }
    assert_eq!(world.formations.values().next().unwrap().leader, start, "the leader set off without half the group");
    assert_eq!(run_until_idle(&mut world, &units, 3_000), 0);
}

#[test]
fn a_formation_move_for_one_unit_is_a_plain_move() {
    let mut world = flat_world();
    let unit = world.spawn_unit(standard_unit(Team::Blue, point(100, 100)));
    world.advance_tick(&[SimCommand::FormationMove { units: vec![unit], goal: point(300, 100), attack: false, queued: false }]);
    assert!(world.formations.is_empty());
    // A standard unit walks one unit a tick.
    assert_eq!(run_until_idle(&mut world, &[unit], 400), 0);
}

#[test]
fn a_new_order_takes_a_unit_out_of_its_formation() {
    let mut world = flat_world();
    let units = mixed_group(&mut world);
    world.advance_tick(&[SimCommand::FormationMove { units: units.clone(), goal: point(2_000, 1_200), attack: false, queued: false }]);
    world.advance_tick(&[SimCommand::Stop { units: units[..11].to_vec() }]);
    world.advance_tick(&[]);
    // One member is not a formation.
    assert!(world.formations.is_empty());
    assert_eq!(run_until_idle(&mut world, &units, 2_000), 0);
}

#[test]
fn formation_moves_replay_identically() {
    let build = || {
        let mut world = flat_world();
        world.add_box_obstacle(RectRange::new(point(1_000, 200), point(1_040, 1_400)));
        world.prepare_map();
        let units = mixed_group(&mut world);
        (world, units)
    };
    let (mut first, units) = build();
    let (mut second, _) = build();
    let orders = [SimCommand::FormationMove { units, goal: point(1_600, 700), attack: true, queued: false }];
    first.advance_tick(&orders);
    second.advance_tick(&orders);
    for _ in 0..600 {
        first.advance_tick(&[]);
        second.advance_tick(&[]);
        assert_eq!(first.compute_checksum(), second.compute_checksum());
    }
}
