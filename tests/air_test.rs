//! Aircraft and anti-air (GAME_SPECIFICATION.md §10): aircraft fly over anything, land when
//! they have nothing to do, and only anti-air weapons can hit them in flight.

use sim_engine::math::fp::{fp_int, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{OrderKind, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::math::rect::RectFp;
use sim_engine::sim::world::SimWorld;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn world() -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(1_000_000);
        }
        economy.mass = Res::from_num(500_000);
        economy.energy = Res::from_num(500_000);
    }
    world
}

fn spawn(world: &mut SimWorld, key: &str, team: Team, at: Vec2Fp) -> SimId {
    world.spawn_mobile(world.blueprints.id(key), team, at)
}

fn run(world: &mut SimWorld, ticks: u32) {
    for _ in 0..ticks {
        world.advance_tick(&[]);
    }
}

fn distance(world: &SimWorld, id: SimId, to: Vec2Fp) -> i64 {
    let at = world.unit_position(id).unwrap().xy();
    let (dx, dy) = (i64::from((at.x - to.x).to_num::<i32>()), i64::from((at.y - to.y).to_num::<i32>()));
    (dx * dx + dy * dy).isqrt()
}

#[test]
fn aircraft_fly_over_walls_at_their_height_and_land_where_they_were_sent() {
    let mut world = world();
    // A wall across the map that no land unit can cross.
    world.add_box_obstacle(RectFp::new(point(2_000, 0), point(2_200, 5_120)));
    world.prepare_map();
    let scout = spawn(&mut world, "t1_air_scout", Team::Blue, point(1_000, 2_500));
    let goal = point(3_500, 2_500);
    world.advance_tick(&[SimCommand::Move { units: vec![scout], goal }]);
    let arrived = (0..400).find(|_| {
        world.advance_tick(&[]);
        let at = world.unit_position(scout).unwrap();
        let idle = world.unit_order(scout).unwrap().kind == OrderKind::Idle;
        assert!(idle || at.z == world.terrain.height_at(at.xy()) + fp_int(320), "it flies at its own height");
        idle
    });
    assert!(arrived.is_some(), "it should get there");
    assert!(distance(&world, scout, goal) < 200);
    // With nothing to do, it slows, comes down, and stays on the ground nearby.
    run(&mut world, 40);
    let landed = world.unit_position(scout).unwrap();
    assert_eq!(landed.z, world.terrain.height_at(landed.xy()) + fp_int(8), "it rests on the ground");
    assert!(distance(&world, scout, goal) < 600);
    run(&mut world, 50);
    assert_eq!(world.unit_position(scout), Some(landed), "landed, it stays put");
    // Given an order, it takes off again.
    world.advance_tick(&[SimCommand::Move { units: vec![scout], goal: point(1_000, 2_500) }]);
    run(&mut world, 40);
    let at = world.unit_position(scout).unwrap();
    assert_eq!(at.z, world.terrain.height_at(at.xy()) + fp_int(320), "back at its height");
}

#[test]
fn aircraft_with_nowhere_to_land_circle() {
    let mut world = world();
    // A wall too wide to land on, under a scout with nothing to do.
    world.add_box_obstacle(RectFp::new(point(1_500, 1_500), point(3_500, 3_500)));
    world.prepare_map();
    let scout = spawn(&mut world, "t1_air_scout", Team::Blue, point(2_500, 2_500));
    let mut moved = 0;
    for _ in 0..200 {
        let before = world.unit_position(scout).unwrap().xy();
        world.advance_tick(&[]);
        moved += distance(&world, scout, before);
        assert!(distance(&world, scout, point(2_500, 2_500)) < 800, "it circles close by");
    }
    assert!(moved > 200 * 30, "it never stops: {moved}");
    let at = world.unit_position(scout).unwrap();
    assert_eq!(at.z, world.terrain.height_at(at.xy()) + fp_int(320));
}

#[test]
fn a_landed_aircraft_is_hit_by_ground_fire_and_not_by_anti_air() {
    let mut world = world();
    let scout = spawn(&mut world, "t1_air_scout", Team::Red, point(2_500, 2_500));
    run(&mut world, 60);
    let landed = world.unit_position(scout).unwrap().xy();
    // Anti-air beside it does nothing to it.
    let mobile = spawn(&mut world, "t1_mobile_aa", Team::Blue, landed + point(150, 0));
    run(&mut world, 50);
    assert_eq!(world.unit_health(scout), Some(fp_int(35)), "anti-air cannot hit a landed aircraft");
    assert!(world.unit_health(mobile).is_some());
    // A tank can.
    spawn(&mut world, "t2_heavy_tank", Team::Blue, landed + point(0, 150));
    run(&mut world, 50);
    assert!(world.unit_health(scout).is_none(), "a tank should destroy the landed scout");
}

#[test]
fn only_anti_air_weapons_hit_aircraft_and_most_hit_nothing_else() {
    let mut world = world();
    // An unarmed scout and an interceptor making passes over a tank.
    let scout = spawn(&mut world, "t1_air_scout", Team::Red, point(2_500, 2_500));
    let tank = spawn(&mut world, "t2_heavy_tank", Team::Blue, point(2_500, 2_520));
    let interceptor = spawn(&mut world, "t1_interceptor", Team::Red, point(2_400, 2_500));
    world.advance_tick(&[SimCommand::Follow { units: vec![scout, interceptor], target: tank }]);
    // Ordered to, the tank still cannot attack an aircraft.
    world.advance_tick(&[SimCommand::Attack { units: vec![tank], target: scout }]);
    run(&mut world, 200);
    assert_eq!(world.unit_order(tank).unwrap().kind, OrderKind::Idle, "a tank cannot be told to attack an aircraft");
    assert_eq!(world.unit_health(scout), Some(fp_int(35)), "ground guns cannot hit aircraft");
    let tank_health = world.unit_health(tank).unwrap();
    let full = fp_int(world.blueprints.get(world.blueprints.id("t2_heavy_tank")).health.to_num::<i32>());
    assert_eq!(tank_health, full, "the interceptor does not shoot at the tank");
    assert!(world.unit_health(interceptor).is_some());
}

#[test]
fn interceptors_shoot_down_aircraft() {
    let mut world = world();
    let bomber = spawn(&mut world, "t1_bomber", Team::Red, point(2_500, 2_500));
    // Aircraft only fight what comes within range of their weapons.
    let interceptor = spawn(&mut world, "t1_interceptor", Team::Blue, point(2_200, 2_300));
    // Kept in the air by an order. A landed aircraft is safe from interceptors.
    world.advance_tick(&[SimCommand::Follow { units: vec![bomber], target: interceptor }]);
    // 215 health at 50 a second: about five seconds once in range.
    run(&mut world, 150);
    assert!(world.unit_health(bomber).is_none(), "the bomber should be shot down");
    assert!(world.unit_health(interceptor).is_some());
}

#[test]
fn anti_air_on_the_ground_shoots_down_aircraft() {
    let mut world = world();
    let turret = world.spawn_finished_structure(world.blueprints.id("t1_aa_turret"), Team::Blue, 100, 125).unwrap();
    let scout = spawn(&mut world, "t1_air_scout", Team::Red, point(2_300, 2_500));
    world.advance_tick(&[SimCommand::Follow { units: vec![scout], target: turret }]);
    run(&mut world, 100);
    assert!(world.unit_health(scout).is_none(), "the turret should shoot the scout down");
    assert!(world.unit_health(turret).is_some());

    let mut world = self::world();
    let mobile = spawn(&mut world, "t1_mobile_aa", Team::Blue, point(2_000, 2_500));
    let interceptor = spawn(&mut world, "t1_interceptor", Team::Red, point(2_300, 2_500));
    world.advance_tick(&[SimCommand::Follow { units: vec![interceptor], target: mobile }]);
    // 295 health at 26 a second.
    run(&mut world, 200);
    assert!(world.unit_health(interceptor).is_none(), "mobile anti-air should shoot the interceptor down");
    assert!(world.unit_health(mobile).is_some());
}

#[test]
fn bombers_make_passes_over_a_structure_until_it_is_destroyed() {
    let mut world = world();
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 150, 125).unwrap();
    let bomber = spawn(&mut world, "t1_bomber", Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Attack { units: vec![bomber], target: generator }]);
    let health = world.blueprints.get(world.blueprints.id("power_generator")).health;
    let mut first_hit = None;
    for tick in 0..1_500 {
        world.advance_tick(&[]);
        match world.unit_health(generator) {
            Some(now) if now < health && first_hit.is_none() => first_hit = Some(tick),
            None => break,
            _ => {}
        }
    }
    assert!(first_hit.is_some_and(|tick| tick < 300), "the first bomb should land soon: {first_hit:?}");
    assert!(world.unit_health(generator).is_none(), "repeated passes should destroy it");
    // With its target gone, it has nothing to do, and lands.
    run(&mut world, 60);
    assert_eq!(world.unit_order(bomber).unwrap().kind, OrderKind::Idle);
    let at = world.unit_position(bomber).unwrap();
    assert_eq!(at.z, world.terrain.height_at(at.xy()) + fp_int(10));
}

#[test]
fn an_air_factory_builds_aircraft_that_fly_to_its_rally_point() {
    let mut world = world();
    let factory = world.spawn_finished_structure(world.blueprints.id("air_factory"), Team::Blue, 50, 50).unwrap();
    let interceptor = world.blueprints.id("t1_interceptor");
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: interceptor, count: 1 }]);
    world.advance_tick(&[SimCommand::SetRally { factory, point: point(3_000, 3_000) }]);
    // 500 build time at the factory's 20 build power is 25 seconds.
    run(&mut world, 400);
    let built: Vec<SimId> = world
        .units
        .iter()
        .map(|unit| unit.0)
        .filter(|&id| world.ecs.get::<&sim_engine::sim::components::Blueprinted>(world.unit_entity(id).unwrap()).is_ok_and(|unit| unit.0 == interceptor))
        .collect();
    assert_eq!(built.len(), 1);
    assert!(distance(&world, built[0], point(3_000, 3_000)) < 800, "it flew to the rally point");
}

#[test]
fn air_battles_are_deterministic() {
    let play = || {
        let mut world = world();
        let mut ids = Vec::new();
        for index in 0..4 {
            ids.push(spawn(&mut world, "t1_interceptor", Team::Blue, point(1_000 + index * 40, 1_000)));
            ids.push(spawn(&mut world, "t1_bomber", Team::Red, point(3_000 + index * 40, 3_000)));
        }
        world.spawn_finished_structure(world.blueprints.id("t1_aa_turret"), Team::Red, 150, 150).unwrap();
        world.advance_tick(&[SimCommand::Move { units: ids.iter().copied().step_by(2).collect(), goal: point(3_000, 3_000) }]);
        let mut checksums = Vec::new();
        for _ in 0..400 {
            world.advance_tick(&[]);
            checksums.push(world.compute_checksum());
        }
        (checksums, world.units.len())
    };
    let first = play();
    assert!(first.1 < 9, "some should have been shot down");
    assert_eq!(first, play());
}


#[test]
fn an_aircraft_told_to_follow_a_unit_keeps_with_it() {
    let mut world = world();
    let tank = spawn(&mut world, "t2_heavy_tank", Team::Blue, point(1_000, 2_500));
    let scout = spawn(&mut world, "t1_air_scout", Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Follow { units: vec![scout], target: tank }]);
    world.advance_tick(&[SimCommand::Move { units: vec![tank], goal: point(4_000, 2_500) }]);
    for _ in 0..600 {
        world.advance_tick(&[]);
    }
    let at = world.unit_position(tank).unwrap().xy();
    assert!(distance(&world, scout, at) < 800, "the scout stays over the tank");
    assert_ne!(world.unit_order(scout).unwrap().kind, OrderKind::Idle, "and is still following it");
}
