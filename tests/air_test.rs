//! Aircraft and anti-air (GAME_SPECIFICATION.md §10): aircraft fly over anything, land when
//! they have nothing to do, and only anti-air weapons can hit them in flight.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
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

fn flight(world: &SimWorld, id: SimId) -> sim_engine::sim::components::Flight {
    *world.ecs.get::<&sim_engine::sim::components::Flight>(world.unit_entity(id).unwrap()).unwrap()
}

fn staging_center() -> Vec2Fp {
    point(2_030, 2_030)
}

fn set_fuel(world: &mut SimWorld, id: SimId, fuel: u32) {
    world.ecs.get::<&mut sim_engine::sim::components::Flight>(world.unit_entity(id).unwrap()).unwrap().fuel = fuel;
}

#[test]
fn aircraft_burn_fuel_in_the_air_and_fly_low_and_slow_without_it() {
    let mut world = world();
    let scout = spawn(&mut world, "t1_air_scout", Team::Blue, point(500, 2_500));
    assert_eq!(flight(&world, scout).fuel, 5_000, "500 seconds of fuel");
    world.advance_tick(&[SimCommand::Move { units: vec![scout], goal: point(4_500, 2_500) }]);
    run(&mut world, 10);
    assert_eq!(flight(&world, scout).fuel, 4_989);
    set_fuel(&mut world, scout, 1);
    run(&mut world, 40);
    let at = world.unit_position(scout).unwrap();
    assert!(flight(&world, scout).out_of_fuel());
    assert_eq!(at.z, world.terrain.height_at(at.xy()) + fp_int(160), "half its height");
    assert_eq!(flight(&world, scout).speed, fp_int(19), "half its speed");
    // Landed, the tank slowly fills again.
    world.advance_tick(&[SimCommand::Stop { units: vec![scout] }]);
    run(&mut world, 60);
    assert!(flight(&world, scout).fuel > 100, "it refuels on the ground: {}", flight(&world, scout).fuel);
}

#[test]
fn an_air_staging_facility_refuels_and_repairs_aircraft_for_energy() {
    let mut world = world();
    let staging = world.spawn_finished_structure(world.blueprints.id("air_staging"), Team::Blue, 100, 100).unwrap();
    let interceptor = spawn(&mut world, "t1_interceptor", Team::Blue, point(1_000, 1_000));
    set_fuel(&mut world, interceptor, 100);
    world.ecs.get::<&mut sim_engine::sim::components::Health>(world.unit_entity(interceptor).unwrap()).unwrap().current = fp_int(95);
    world.advance_tick(&[SimCommand::Refit { units: vec![interceptor], staging }]);
    let docked = (0..300).any(|_| {
        world.advance_tick(&[]);
        flight(&world, interceptor).docked
    });
    assert!(docked, "it should land on the pad");
    let pad = world.unit_position(interceptor).unwrap().xy();
    assert!(distance(&world, interceptor, staging_center()) < 40, "on the facility, at {pad:?}");
    let energy = world.economies[0].energy;
    // 3,000 ticks of fuel at 250 a tick, and 200 health at 50 a tick.
    let left = (0..20).find(|_| {
        world.advance_tick(&[]);
        flight(&world, interceptor).refit.is_none()
    });
    assert!(left.is_some_and(|ticks| ticks <= 13), "done in about 12 ticks: {left:?}");
    assert_eq!(world.unit_health(interceptor), Some(fp_int(295)));
    assert!(energy - world.economies[0].energy >= Res::from_num(12), "repairs cost energy");
    // With nothing to do, it lifts off the pad, flies clear of the facility, and lands beside it.
    run(&mut world, 80);
    let after = flight(&world, interceptor);
    assert!(!after.airborne(), "it lands");
    assert!(after.fuel >= after.max_fuel - 10, "full, less the hop: {}", after.fuel);
    let at = world.unit_position(interceptor).unwrap().xy();
    assert!(world.terrain.is_passable(at), "off the facility");
    assert!(distance(&world, interceptor, staging_center()) < 800, "and close by");
}

#[test]
fn a_facility_holds_four_aircraft_at_once_and_the_rest_wait_overhead() {
    let mut world = world();
    let staging = world.spawn_finished_structure(world.blueprints.id("air_staging"), Team::Blue, 100, 100).unwrap();
    // All five arrive at once.
    let scouts: Vec<SimId> = (0..5).map(|index| spawn(&mut world, "t1_air_scout", Team::Blue, point(2_020 + index * 5, 2_030))).collect();
    for &scout in &scouts {
        set_fuel(&mut world, scout, 0);
    }
    world.advance_tick(&[SimCommand::Refit { units: scouts.clone(), staging }]);
    let mut most = 0;
    let mut all_served = [false; 5];
    for _ in 0..600 {
        world.advance_tick(&[]);
        let docked: Vec<bool> = scouts.iter().map(|&scout| flight(&world, scout).docked).collect();
        most = most.max(docked.iter().filter(|&&docked| docked).count());
        for (served, docked) in all_served.iter_mut().zip(&docked) {
            *served |= *docked;
        }
    }
    assert_eq!(most, 4, "four pads");
    assert!(all_served.iter().all(|&served| served), "the fifth gets its turn");
    assert!(scouts.iter().all(|&scout| flight(&world, scout).fuel == flight(&world, scout).max_fuel));
}

#[test]
fn patrolling_aircraft_stay_up_refuel_when_low_and_go_back_to_their_patrol() {
    let mut world = world();
    world.spawn_finished_structure(world.blueprints.id("air_staging"), Team::Blue, 100, 100).unwrap();
    let interceptor = spawn(&mut world, "t1_interceptor", Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Patrol { units: vec![interceptor], points: vec![point(1_000, 1_000), point(4_000, 1_000)], queued: false }]);
    for _ in 0..300 {
        world.advance_tick(&[]);
        assert!(flight(&world, interceptor).airborne(), "a patrol never lands");
    }
    // Below a fifth of a tank, it goes to refuel.
    set_fuel(&mut world, interceptor, 500);
    let docked = (0..300).any(|_| {
        world.advance_tick(&[]);
        flight(&world, interceptor).docked
    });
    assert!(docked, "it should divert to the facility");
    run(&mut world, 60);
    let after = flight(&world, interceptor);
    assert!(after.refit.is_none() && after.airborne(), "refuelled, it is back in the air");
    assert_ne!(world.unit_order(interceptor).unwrap().kind, OrderKind::Idle, "on its patrol");
    assert!(after.fuel > after.max_fuel * 9 / 10);
}

#[test]
fn gunships_hover_over_what_they_attack() {
    let mut world = world();
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 150, 125).unwrap();
    let gunship = spawn(&mut world, "t2_gunship", Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Attack { units: vec![gunship], target: generator }]);
    let target = world.unit_position(generator).unwrap().xy();
    let mut hovered = false;
    for _ in 0..600 {
        world.advance_tick(&[]);
        if world.unit_health(generator).is_none() {
            break;
        }
        let near = distance(&world, gunship, target) < 440;
        hovered |= near && flight(&world, gunship).speed == Fp::ZERO;
    }
    assert!(hovered, "it should stop in the air within range");
    assert!(world.unit_health(generator).is_none(), "and shoot the generator to pieces");
    assert!(flight(&world, gunship).airborne());
}

#[test]
fn flak_bursts_hit_every_enemy_aircraft_around_it() {
    let mut world = world();
    let flak = world.spawn_finished_structure(world.blueprints.id("t2_flak"), Team::Blue, 125, 125).unwrap();
    // Three scouts flying close together over the cannon.
    let scouts: Vec<SimId> = (0..3).map(|index| spawn(&mut world, "t1_air_scout", Team::Red, point(3_000 + index * 15, 2_510))).collect();
    world.advance_tick(&[SimCommand::Follow { units: scouts.clone(), target: flak }]);
    let first = (0..300).find(|_| {
        world.advance_tick(&[]);
        scouts.iter().any(|&scout| world.unit_health(scout).is_none())
    });
    assert!(first.is_some(), "the cannon should hit them");
    assert!(scouts.iter().all(|&scout| world.unit_health(scout).is_none()), "one burst takes all three");
}

#[test]
fn an_air_factory_upgrades_to_t2_and_builds_gunships() {
    let mut world = world();
    let factory = world.spawn_finished_structure(world.blueprints.id("air_factory"), Team::Blue, 50, 50).unwrap();
    let (upgrade, gunship) = (world.blueprints.id("air_factory_t2"), world.blueprints.id("t2_gunship"));
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: upgrade, count: 1 }]);
    world.advance_tick(&[SimCommand::QueueUnit { factory, blueprint: gunship, count: 1 }]);
    let built = (0..2_000).any(|_| {
        world.advance_tick(&[]);
        world.units.iter().any(|unit| world.ecs.get::<&sim_engine::sim::components::Blueprinted>(unit.1).is_ok_and(|bp| bp.0 == gunship))
    });
    assert!(built, "the upgraded factory should build a gunship");
}

#[test]
fn a_strategic_bomber_flattens_a_whole_cluster_with_one_bomb() {
    let mut world = world();
    let key = world.blueprints.id("power_generator");
    let generators: Vec<SimId> = [(150, 125), (152, 125), (150, 127)].iter().map(|&(x, y)| world.spawn_finished_structure(key, Team::Red, x, y).unwrap()).collect();
    let bomber = spawn(&mut world, "t3_strategic_bomber", Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Attack { units: vec![bomber], target: generators[0] }]);
    let first = (0..600).find(|_| {
        world.advance_tick(&[]);
        generators.iter().any(|&id| world.unit_health(id).is_none())
    });
    assert!(first.is_some(), "the bomb should land");
    assert!(generators.iter().all(|&id| world.unit_health(id).is_none()), "and take all three");
}

#[test]
fn sam_launchers_and_air_superiority_fighters_down_heavy_aircraft() {
    let mut world = world();
    let sam = world.spawn_finished_structure(world.blueprints.id("t3_sam"), Team::Blue, 125, 125).unwrap();
    let fighter = spawn(&mut world, "t3_air_superiority", Team::Red, point(3_000, 2_500));
    world.advance_tick(&[SimCommand::Follow { units: vec![fighter], target: sam }]);
    run(&mut world, 200);
    assert!(world.unit_health(fighter).is_none(), "2,300 health, two volleys of 1,200");

    let mut world = self::world();
    let bomber = spawn(&mut world, "t1_bomber", Team::Red, point(2_500, 2_500));
    let fighter = spawn(&mut world, "t3_air_superiority", Team::Blue, point(2_200, 2_400));
    world.advance_tick(&[SimCommand::Follow { units: vec![bomber], target: fighter }]);
    run(&mut world, 40);
    assert!(world.unit_health(bomber).is_none(), "one shot of 500 against 215");
}

#[test]
fn a_strategic_bomber_bombs_with_its_main_weapon_and_shoots_aircraft_with_its_guns() {
    let mut world = world();
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 150, 125).unwrap();
    let bomber = spawn(&mut world, "t3_strategic_bomber", Team::Blue, point(1_000, 1_000));
    let scout = spawn(&mut world, "t1_air_scout", Team::Red, point(1_300, 1_000));
    world.advance_tick(&[SimCommand::Follow { units: vec![scout], target: bomber }]);
    world.advance_tick(&[SimCommand::Attack { units: vec![bomber], target: generator }]);
    run(&mut world, 600);
    assert!(world.unit_health(scout).is_none(), "its guns should down the scout on its tail");
    assert!(world.unit_health(generator).is_none(), "while its bomb takes the generator");
}

#[test]
fn a_unit_can_be_told_to_attack_what_only_its_extra_weapons_can_hit() {
    let mut world = world();
    let gunship = spawn(&mut world, "t3_heavy_gunship", Team::Blue, point(1_000, 1_000));
    let scout = spawn(&mut world, "t1_air_scout", Team::Red, point(1_500, 1_000));
    world.advance_tick(&[SimCommand::Follow { units: vec![scout], target: gunship }]);
    world.advance_tick(&[SimCommand::Attack { units: vec![gunship], target: scout }]);
    assert_eq!(world.unit_order(gunship).unwrap().kind, OrderKind::Attack, "its anti-air gun can hit the scout");
    run(&mut world, 200);
    assert!(world.unit_health(scout).is_none());
}

#[test]
fn a_fighter_bomber_carpets_the_ground_and_fights_aircraft() {
    let mut world = world();
    let key = world.blueprints.id("power_generator");
    let generators: Vec<SimId> = [(150, 125), (152, 125)].iter().map(|&(x, y)| world.spawn_finished_structure(key, Team::Red, x, y).unwrap()).collect();
    let janus = spawn(&mut world, "t2_fighter_bomber", Team::Blue, point(1_000, 1_000));
    let interceptor = spawn(&mut world, "t1_interceptor", Team::Red, point(1_200, 1_000));
    world.advance_tick(&[SimCommand::Follow { units: vec![interceptor], target: janus }]);
    world.advance_tick(&[SimCommand::Attack { units: vec![janus], target: generators[0] }]);
    run(&mut world, 700);
    assert!(world.unit_health(interceptor).is_none(), "150 a second against 295");
    assert!(generators.iter().all(|&id| world.unit_health(id).is_none()), "1,600 within 60 takes both");
}
