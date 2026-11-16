//! Air transports carry land units: they board as the transport comes to them, ride off the
//! map, and are set down together where it is sent. What is on board goes down with it.

use sim_engine::math::fp::{fp_int, Vec2Fp};
use sim_engine::sim::blueprints::{BlueprintId, Res};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{SimId, Team, Transport};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::transport::{fits, transport_size};
use sim_engine::sim::world::SimWorld;

fn test_world() -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        economy.base_energy_capacity = Res::from_num(1_000_000);
        economy.energy_capacity = economy.base_energy_capacity;
        economy.energy = economy.energy_capacity;
    }
    world
}

fn id(world: &SimWorld, key: &str) -> BlueprintId {
    world.blueprints.id(key)
}

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn spawn(world: &mut SimWorld, key: &str, x: i32, y: i32) -> SimId {
    world.spawn_mobile(id(world, key), Team::Blue, point(x, y))
}

fn cargo(world: &SimWorld, transport: SimId) -> Vec<SimId> {
    world.ecs.get::<&Transport>(world.unit_entity(transport).unwrap()).unwrap().cargo.clone()
}

fn run(world: &mut SimWorld, ticks: u32) {
    for _ in 0..ticks {
        world.advance_tick(&[]);
    }
}

#[test]
fn units_board_a_transport_that_comes_to_them_and_leave_the_map() {
    let mut world = test_world();
    let transport = spawn(&mut world, "t1_light_transport", 1_500, 1_500);
    let bots: Vec<SimId> = (0..3).map(|index| spawn(&mut world, "t1_light_walker", 2_000 + index * 30, 2_000)).collect();
    world.advance_tick(&[SimCommand::Load { units: bots.clone(), transport }]);
    run(&mut world, 300);
    let mut on_board = cargo(&world, transport);
    on_board.sort_unstable();
    assert_eq!(on_board, bots, "all three are on board");
    for &bot in &bots {
        assert!(world.unit_entity(bot).is_none(), "off the map, where nothing can find it");
        assert!(world.snapshot_for(None).units.iter().all(|unit| unit.id != bot), "and not drawn");
    }
}

#[test]
fn a_transport_takes_only_what_fits() {
    let world = test_world();
    let light = world.blueprints.get(id(&world, "t1_light_transport")).transport.unwrap();
    let size = |key: &str| transport_size(world.blueprints.get(id(&world, key))).unwrap();
    let (bot, tank, assault) = (size("t1_light_walker"), size("t2_heavy_tank"), size("t3_heavy_walker"));
    assert!(fits(light, [5, 0, 0], bot) && !fits(light, [6, 0, 0], bot), "six small");
    assert!(fits(light, [0, 1, 0], tank) && !fits(light, [0, 2, 0], tank), "two medium");
    assert!(fits(light, [0, 0, 0], assault) && !fits(light, [0, 0, 1], assault), "one large");
    assert!(fits(light, [2, 1, 0], bot) && !fits(light, [3, 1, 0], bot), "a medium takes the room of three small");
    assert_eq!(transport_size(world.blueprints.get(id(&world, "command_rig"))), None, "commanders walk");
    assert_eq!(transport_size(world.blueprints.get(id(&world, "t1_interceptor"))), None);
    assert_eq!(transport_size(world.blueprints.get(id(&world, "t1_frigate"))), None);

    let mut world = test_world();
    let transport = spawn(&mut world, "t1_light_transport", 1_500, 1_500);
    let bots: Vec<SimId> = (0..8).map(|index| spawn(&mut world, "t1_light_walker", 2_000 + (index % 4) * 30, 2_000 + (index / 4) * 30)).collect();
    world.advance_tick(&[SimCommand::Load { units: bots.clone(), transport }]);
    run(&mut world, 400);
    assert_eq!(cargo(&world, transport).len(), 6, "six get in");
    let left: Vec<SimId> = bots.iter().copied().filter(|bot| world.unit_entity(*bot).is_some()).collect();
    assert_eq!(left.len(), 2, "and two are left behind");
    for bot in left {
        assert_eq!(world.unit_order(bot).unwrap().kind, sim_engine::sim::components::OrderKind::Idle, "and give up");
    }
}

#[test]
fn a_transport_sets_its_cargo_down_where_it_is_sent_and_it_can_be_ordered_again() {
    let mut world = test_world();
    let transport = spawn(&mut world, "t1_light_transport", 1_500, 1_500);
    let bots: Vec<SimId> = (0..4).map(|index| spawn(&mut world, "t1_light_walker", 1_600 + index * 30, 1_600)).collect();
    world.advance_tick(&[SimCommand::Load { units: bots.clone(), transport }]);
    run(&mut world, 300);
    assert_eq!(cargo(&world, transport).len(), 4);
    let drop = point(4_000, 3_500);
    world.advance_tick(&[SimCommand::Unload { transports: vec![transport], point: drop }]);
    run(&mut world, 400);
    assert!(cargo(&world, transport).is_empty(), "all set down");
    for &bot in &bots {
        let at = world.unit_position(bot).expect("back on the map").xy();
        assert!(at.distance_squared(&drop) < sim_engine::math::fp::wide_mul(fp_int(150), fp_int(150)), "set down near the spot");
    }
    let goal = point(4_500, 3_500);
    world.advance_tick(&[SimCommand::Move { units: bots.clone(), goal }]);
    run(&mut world, 300);
    assert!(world.unit_position(bots[0]).unwrap().x > fp_int(4_300), "and they take orders again");
}

#[test]
fn what_a_transport_carries_goes_down_with_it() {
    let mut world = test_world();
    let transport = spawn(&mut world, "t1_light_transport", 1_500, 1_500);
    let bots: Vec<SimId> = (0..2).map(|index| spawn(&mut world, "t1_light_walker", 1_600 + index * 30, 1_600)).collect();
    world.advance_tick(&[SimCommand::Load { units: bots.clone(), transport }]);
    run(&mut world, 300);
    assert_eq!(cargo(&world, transport).len(), 2);
    // Self-destruct takes five seconds.
    world.advance_tick(&[SimCommand::SelfDestruct { units: vec![transport] }]);
    run(&mut world, 70);
    assert!(world.unit_entity(transport).is_none());
    assert!(world.carried.is_empty(), "nothing is left inside");
    for bot in bots {
        assert!(world.unit_entity(bot).is_none(), "the cargo went down with it");
    }
}

#[test]
fn a_transported_match_replays_exactly() {
    let play = || {
        let mut world = test_world();
        let transport = spawn(&mut world, "t2_transport", 1_500, 1_500);
        let units: Vec<SimId> = ["t1_light_walker", "t2_heavy_tank", "t1_engineer"].iter().enumerate().map(|(index, key)| spawn(&mut world, key, 1_600 + index as i32 * 40, 1_600)).collect();
        world.advance_tick(&[SimCommand::Load { units, transport }]);
        run(&mut world, 250);
        let mid = world.compute_checksum();
        world.advance_tick(&[SimCommand::Unload { transports: vec![transport], point: point(3_000, 3_000) }]);
        run(&mut world, 300);
        (mid, world.compute_checksum())
    };
    assert_eq!(play(), play());
}
