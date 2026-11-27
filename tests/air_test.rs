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

/// The air scout's flying height, as the unit data has it.
fn scout_height(world: &SimWorld) -> Fp {
    world.blueprints.get(world.blueprints.id("t1_air_scout")).flight.expect("the scout flies").elevation
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
        assert!(idle || at.z == world.terrain.height_at(at.xy()) + scout_height(&world), "it flies at its own height");
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
    assert_eq!(at.z, world.terrain.height_at(at.xy()) + scout_height(&world), "back at its height");
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
    assert_eq!(at.z, world.terrain.height_at(at.xy()) + scout_height(&world));
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
    // 215 health at 50 a second is about five seconds of fire, once its nose is round on the
    // bomber, which circles after it: its guns are fixed to face forward.
    run(&mut world, 250);
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
    // Its shells miss an interceptor about as often as not, so twice its steady fire's time, and
    // a few seconds to get it in range.
    let gun = world.blueprints.get(world.blueprints.id("t1_mobile_aa")).weapon.expect("anti-air has a gun");
    let health = world.unit_health(interceptor).expect("alive").to_num::<f64>();
    let seconds = health / (gun.damage.to_num::<f64>() * 10.0 / f64::from(gun.reload_ticks));
    run(&mut world, ((seconds * 2.0 + 5.0) * 10.0) as u32);
    assert!(world.unit_health(interceptor).is_none(), "mobile anti-air should shoot the interceptor down");
    assert!(world.unit_health(mobile).is_some());
}

/// Out of fuel, a bomber flies low and slow, and still bombs from there.
#[test]
fn a_bomber_out_of_fuel_still_bombs() {
    let mut world = world();
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 150, 125).unwrap();
    let bomber = spawn(&mut world, "t1_bomber", Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::Attack { units: vec![bomber], target: generator }]);
    set_fuel(&mut world, bomber, 1);
    let health = world.blueprints.get(world.blueprints.id("power_generator")).health;
    let hit = (0..600).find(|_| {
        world.advance_tick(&[]);
        world.unit_health(generator).is_none_or(|now| now < health)
    });
    assert!(flight(&world, bomber).out_of_fuel());
    assert!(hit.is_some(), "a bomb should land within a minute");
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
    assert_eq!(at.z, world.terrain.height_at(at.xy()) + scout_height(&world) / 2, "half its height");
    assert_eq!(flight(&world, scout).speed, fp_int(19), "half its speed");
    // Landed, the tank slowly fills again.
    world.advance_tick(&[SimCommand::Stop { units: vec![scout] }]);
    run(&mut world, 60);
    assert!(flight(&world, scout).fuel > 100, "it refuels on the ground: {}", flight(&world, scout).fuel);
}

#[test]
fn an_airfield_facility_refuels_and_repairs_aircraft_for_energy() {
    let mut world = world();
    let staging = world.spawn_finished_structure(world.blueprints.id("airfield"), Team::Blue, 100, 100).unwrap();
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
    // 3,000 ticks of fuel at 250 a tick, and its missing health at 50 a tick, read from its data.
    let left = (0..20).find(|_| {
        world.advance_tick(&[]);
        flight(&world, interceptor).refit.is_none()
    });
    assert!(left.is_some_and(|ticks| ticks <= 13), "done in about 12 ticks: {left:?}");
    let full = world.blueprints.get(world.blueprints.id("t1_interceptor")).health;
    assert_eq!(world.unit_health(interceptor), Some(full));
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
    let staging = world.spawn_finished_structure(world.blueprints.id("airfield"), Team::Blue, 100, 100).unwrap();
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
    world.spawn_finished_structure(world.blueprints.id("airfield"), Team::Blue, 100, 100).unwrap();
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

/// A gunship circles what it attacks, anticlockwise, at half its top speed and about half its range out, rather than make
/// passes, and its turret keeps on the target all the way round: once it is circling it lands
/// every shot its reload allows (the design owner, 2026-11-27). Both gunships, and the heavy
/// gunship's railgun too, which only fires at aircraft, is left out here.
#[test]
fn gunships_circle_what_they_attack_and_keep_firing() {
    for key in ["t2_gunship", "t3_heavy_gunship"] {
        let mut world = world();
        let at = point(2_560, 2_560);
        let target = spawn(&mut world, "t1_engineer", Team::Red, at);
        let entity = world.unit_entity(target).unwrap();
        let mut health = world.ecs.get::<&mut sim_engine::sim::components::Health>(entity).unwrap();
        (health.max, health.current) = (fp_int(1_000_000), fp_int(1_000_000));
        drop(health);
        world.ecs.get::<&mut sim_engine::sim::components::Combat>(entity).unwrap().enabled = false;
        let gunship = spawn(&mut world, key, Team::Blue, point(1_200, 2_560));
        world.advance_tick(&[SimCommand::Attack { units: vec![gunship], target }]);
        let def = world.blueprints.get(world.blueprints.id(key)).clone();
        let weapon = def.weapon.expect("a gunship has a gun");
        let range = i64::from(weapon.range.to_num::<i32>());
        // Until it is in range and up to speed.
        let settled = (0..600).find(|_| {
            world.advance_tick(&[]);
            distance(&world, gunship, at) < range * 3 / 4 && flight(&world, gunship).speed == def.speed / 2
        });
        assert!(settled.is_some(), "{key} should come round onto its circle");
        run(&mut world, 50);
        let before = world.unit_health(target).unwrap();
        let bearing = |world: &SimWorld| {
            let here = world.unit_position(gunship).unwrap().xy();
            f64::from((here.y - at.y).to_num::<i32>()).atan2(f64::from((here.x - at.x).to_num::<i32>()))
        };
        let (mut turned, mut last) = (0.0_f64, bearing(&world));
        let ticks = 300;
        for _ in 0..ticks {
            world.advance_tick(&[]);
            let now = bearing(&world);
            let mut step = now - last;
            if step > std::f64::consts::PI {
                step -= std::f64::consts::TAU;
            } else if step < -std::f64::consts::PI {
                step += std::f64::consts::TAU;
            }
            (turned, last) = (turned + step, now);
            let off = distance(&world, gunship, at);
            assert!(off > range / 4 && off < range * 3 / 4, "{key} should keep about half its range out, not {off} of {range}");
            assert_eq!(flight(&world, gunship).speed, def.speed / 2, "{key} should keep flying as it circles");
        }
        assert!(turned > std::f64::consts::TAU, "{key} should go right round its target anticlockwise, not {:.0} degrees", turned.to_degrees());
        let dealt = (before - world.unit_health(target).unwrap()).to_num::<f64>();
        let most = f64::from(ticks) / f64::from(weapon.reload_ticks) * weapon.damage.to_num::<f64>();
        assert!(dealt >= most * 0.95, "{key} should land every shot as it circles: {dealt:.0} of {most:.0}");
        assert!(flight(&world, gunship).airborne());
    }
}

#[test]
fn flak_bursts_hit_every_enemy_aircraft_around_it() {
    let mut world = world();
    let flak = world.spawn_finished_structure(world.blueprints.id("t2_flak"), Team::Blue, 125, 125).unwrap();
    // Without its scatter, so that the burst is where it was aimed.
    let entity = world.unit_entity(flak).unwrap();
    world.ecs.get::<&mut sim_engine::sim::components::Scatter>(entity).unwrap().0 = Fp::ZERO;
    // Three gunships hovering together over what they attack, beside the cannon: hovering, they
    // are where flak expects them.
    let target = spawn(&mut world, "t1_engineer", Team::Blue, point(2_700, 2_500));
    // Too sturdy to die while they hover over it, so that they stay where the flak expects them.
    let mut health = world.ecs.get::<&mut sim_engine::sim::components::Health>(world.unit_entity(target).unwrap()).unwrap();
    (health.max, health.current) = (fp_int(1_000_000), fp_int(1_000_000));
    drop(health);
    let gunships: Vec<SimId> = (0..3).map(|index| spawn(&mut world, "t2_gunship", Team::Red, point(2_000 + index * 20, 2_000))).collect();
    world.advance_tick(&[SimCommand::Attack { units: gunships.clone(), target }]);
    // The cannon holds its fire until they hover over the target.
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(entity).unwrap().enabled = false;
    run(&mut world, 150);
    world.ecs.get::<&mut sim_engine::sim::components::Combat>(entity).unwrap().enabled = true;
    let full = |world: &SimWorld, id: SimId| world.unit_health(id) == Some(world.blueprints.get(world.blueprints.id("t2_gunship")).health);
    let first = (0..600).find(|_| {
        world.advance_tick(&[]);
        gunships.iter().any(|&gunship| !full(&world, gunship))
    });
    assert!(first.is_some(), "the cannon should hit them");
    // Hovering, they spread out a little, so the burst takes those it reaches, more than one.
    let hit = gunships.iter().filter(|&&gunship| !full(&world, gunship)).count();
    assert!(hit >= 2, "one burst should hit more than one, not {hit}");
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

/// The Strategic Bomber has no anti-air (2026-11-26, the design owner's rule): a scout on its tail
/// is left alone while its bomb takes the generator.
#[test]
fn a_strategic_bomber_bombs_with_its_main_weapon_and_has_no_guns() {
    let mut world = world();
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Red, 150, 125).unwrap();
    let bomber = spawn(&mut world, "t3_strategic_bomber", Team::Blue, point(1_000, 1_000));
    let scout = spawn(&mut world, "t1_air_scout", Team::Red, point(1_300, 1_000));
    world.advance_tick(&[SimCommand::Follow { units: vec![scout], target: bomber }]);
    world.advance_tick(&[SimCommand::Attack { units: vec![bomber], target: generator }]);
    run(&mut world, 600);
    assert!(world.unit_health(scout).is_some(), "it has no guns to down the scout on its tail");
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
    let fighter_bomber = spawn(&mut world, "t2_fighter_bomber", Team::Blue, point(1_000, 1_000));
    let interceptor = spawn(&mut world, "t1_interceptor", Team::Red, point(1_200, 1_000));
    world.advance_tick(&[SimCommand::Follow { units: vec![interceptor], target: fighter_bomber }]);
    world.advance_tick(&[SimCommand::Attack { units: vec![fighter_bomber], target: generators[0] }]);
    run(&mut world, 700);
    assert!(world.unit_health(interceptor).is_none(), "150 a second against 295");
    assert!(generators.iter().all(|&id| world.unit_health(id).is_none()), "1,600 within 60 takes both");
}

/// A fighter-bomber after an aircraft flies as a fighter, on its tail within its gun's reach,
/// rather than out on bombing passes: its bomb is for the ground, and its gun reaches 600.
#[test]
fn a_fighter_bomber_fights_an_aircraft_from_its_tail() {
    use sim_engine::sim::components::Health;
    let mut world = world();
    let generator = world.spawn_finished_structure(world.blueprints.id("power_generator"), Team::Blue, 150, 125).unwrap();
    let bomber = spawn(&mut world, "t1_bomber", Team::Red, point(4_000, 2_500));
    world.advance_tick(&[SimCommand::Attack { units: vec![bomber], target: generator }]);
    run(&mut world, 60);
    let fighter_bomber = spawn(&mut world, "t2_fighter_bomber", Team::Blue, point(1_500, 2_500));
    world.advance_tick(&[SimCommand::Attack { units: vec![fighter_bomber], target: bomber }]);
    let full = Fp::from_num(30_000);
    let (mut dealt, mut ticks, mut farthest) = (0.0f64, 0u32, 0i64);
    for _ in 0..600 {
        for id in [generator, bomber] {
            world.ecs.get::<&mut Health>(world.unit_entity(id).unwrap()).unwrap().current = full;
        }
        world.advance_tick(&[]);
        let lost = (full - world.ecs.get::<&Health>(world.unit_entity(bomber).unwrap()).unwrap().current).to_num::<f64>();
        if lost > 0.0 || ticks > 0 {
            dealt += lost;
            ticks += 1;
            farthest = farthest.max(distance(&world, fighter_bomber, world.unit_position(bomber).unwrap().xy()));
        }
    }
    let per_second = dealt / (f64::from(ticks) / 10.0);
    assert!(per_second > 0.9 * 174.0, "it lands nearly all of its 174 a second: {per_second:.0}");
    assert!(farthest <= 600, "and keeps within its gun's reach once firing: {farthest}");
}

/// On an attack-move, a bomber keeps to the target it picks, pass after pass, as if told to
/// attack it, instead of bombing whatever is nearest after each pass, and moves on only once it
/// is destroyed. Moving targets, under fog, slip out of range and sight between passes.
#[test]
fn an_attack_moving_bomber_sticks_to_each_target_until_it_is_destroyed() {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let enemies: Vec<SimId> =
        [(3_000, 2_500), (3_200, 2_600), (2_900, 2_700), (3_300, 2_400), (3_100, 2_900)].into_iter().map(|(x, y)| spawn(&mut world, "t2_heavy_tank", Team::Red, point(x, y))).collect();
    let bomber = spawn(&mut world, "t1_bomber", Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::AttackMove { units: vec![bomber], goal: point(4_200, 3_400) }, SimCommand::Move { units: enemies, goal: point(2_000, 4_500) }]);
    let entity = world.units.iter().find(|unit| unit.0 == bomber).expect("the bomber is alive").1;
    let mut current: Option<SimId> = None;
    let mut targets = 0;
    for tick in 0..1_500 {
        world.advance_tick(&[]);
        let target = world.ecs.get::<&sim_engine::sim::components::Combat>(entity).expect("bombers fight").target;
        if target != current {
            if let Some(old) = current {
                assert!(world.unit_health(old).is_none(), "tick {tick}: it left {old:?} for {target:?} with the first still alive");
            }
            if target.is_some() {
                targets += 1;
            }
            current = target;
        }
    }
    assert!(targets >= 2, "it moved on to another target once the first was destroyed: {targets}");
}

/// A fighter-bomber lets its bombs go far short of its target, from high and fast, beyond its
/// own sight, so under fog of war it bombs what its team has on radar. Bombing only what it
/// saw, it never let go, on an attack or an attack-move.
#[test]
fn a_fighter_bomber_bombs_under_fog_of_war_on_attack_and_attack_move() {
    for attack_move in [false, true] {
        let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() };
        let mut world = SimWorld::new(config, 1);
        let tanks: Vec<SimId> = (0..3).map(|index| spawn(&mut world, "t2_heavy_tank", Team::Red, point(3_000 + index * 40, 3_000))).collect();
        let fighter_bomber = spawn(&mut world, "t2_fighter_bomber", Team::Blue, point(1_000, 1_000));
        let order = if attack_move { SimCommand::AttackMove { units: vec![fighter_bomber], goal: point(3_500, 3_500) } } else { SimCommand::Attack { units: vec![fighter_bomber], target: tanks[0] } };
        world.advance_tick(&[order]);
        run(&mut world, 900);
        let alive = tanks.iter().filter(|&&tank| world.unit_health(tank).is_some()).count();
        assert!(alive < 3, "attack-move {attack_move}: it bombed nothing");
    }
}

/// Aircraft in flight keep apart: two sent the same way from the same spot spread out instead
/// of flying as one.
#[test]
fn aircraft_in_flight_keep_apart() {
    let mut world = world();
    let first = spawn(&mut world, "t1_interceptor", Team::Blue, point(1_000, 1_000));
    let second = spawn(&mut world, "t1_interceptor", Team::Blue, point(1_002, 1_000));
    world.advance_tick(&[SimCommand::Move { units: vec![first, second], goal: point(4_000, 3_000) }]);
    let mut closest = i64::MAX;
    for tick in 0..200 {
        world.advance_tick(&[]);
        if tick > 60 {
            let (a, b) = (world.unit_position(first).unwrap().xy(), world.unit_position(second).unwrap().xy());
            closest = closest.min(sim_engine::math::fp::sqrt_wide(a.distance_squared(&b)).to_num::<i64>());
        }
    }
    let radius = world.blueprints.get(world.blueprints.id("t1_interceptor")).radius.to_num::<i64>();
    assert!(closest >= radius * 2, "they stay at least their width apart, not {closest}");
}

/// Aircraft exactly together part side by side, across the way they face, so that a group and
/// its mirror image through the map's center part in mirror image. They parted along x, the
/// lowest id furthest east on both sides (2026-11-25): with fighters repriced, five bombers of
/// a player in a Saddleback 3v3 took off from the one spot they had landed on and spread out in
/// opposite orders on the two sides, and 200 ticks later a bomber and the one at its image
/// looked for targets on different ticks and went for different ones.
#[test]
fn aircraft_exactly_together_part_in_mirror_image() {
    let mut world = world();
    let map = fp_int(world.terrain.map_units());
    let mirror = |at: Vec2Fp| Vec2Fp::new(map - at.x, map - at.y);
    let (start, goal) = (point(1_000, 1_500), point(2_200, 900));
    let blue: Vec<SimId> = (0..5).map(|_| spawn(&mut world, "t1_bomber", Team::Blue, start)).collect();
    let red: Vec<SimId> = (0..5).map(|_| spawn(&mut world, "t1_bomber", Team::Red, mirror(start))).collect();
    world.advance_tick(&[SimCommand::Move { units: blue.clone(), goal }, SimCommand::Move { units: red.clone(), goal: mirror(goal) }]);
    for tick in 0..60 {
        world.advance_tick(&[]);
        for (&blue, &red) in blue.iter().zip(&red) {
            let (at, image) = (world.unit_position(blue).unwrap().xy(), mirror(world.unit_position(red).unwrap().xy()));
            // A few raw bits, as rounding is not symmetric under negation.
            let apart = (at.x - image.x).to_bits().abs().max((at.y - image.y).to_bits().abs());
            assert!(apart <= 16, "tick {tick}: Blue's bomber {} is {apart} raw units from Red's {}'s image", blue.0, red.0);
        }
    }
    let (first, last) = (world.unit_position(blue[0]).unwrap().xy(), world.unit_position(blue[4]).unwrap().xy());
    assert!(first.distance_squared(&last) > fp_int(400), "they part: {first:?} and {last:?}");
}

/// A fighter chasing a slower aircraft eases off to its speed once in range behind it, and stays
/// on its tail, firing, until it is down, rather than flying past it and looping round.
#[test]
fn a_fighter_stays_on_a_slower_aircrafts_tail_until_it_is_down() {
    let mut world = world();
    // Tough enough for a long chase, so that flying past would show, and short enough to end
    // before the bomber reaches the end of its move and lands.
    let id = world.blueprints.id("t1_bomber");
    world.blueprints = std::sync::Arc::new(world.blueprints.with_unit(id, |unit| unit.health = fp_int(800)));
    let bomber = spawn(&mut world, "t1_bomber", Team::Red, point(500, 2_500));
    let interceptor = spawn(&mut world, "t1_interceptor", Team::Blue, point(100, 2_500));
    world.advance_tick(&[SimCommand::Move { units: vec![bomber], goal: point(5_000, 2_500) }]);
    world.advance_tick(&[SimCommand::Attack { units: vec![interceptor], target: bomber }]);
    let reach = world.blueprints.get(world.blueprints.id("t1_interceptor")).weapon.expect("interceptors have a gun").range.to_num::<i64>();
    let gap = |world: &SimWorld| (world.unit_position(bomber).unwrap().x - world.unit_position(interceptor).unwrap().x).to_num::<i64>();
    let down = (0..400).find(|&tick| {
        world.advance_tick(&[]);
        if world.unit_health(bomber).is_none() {
            return true;
        }
        let behind = gap(&world);
        assert!(behind > 0, "the fighter flew past the bomber at tick {tick}");
        assert!(tick < 30 || behind <= reach, "the fighter fell out of range at tick {tick}, {behind} behind");
        false
    });
    assert!(down.is_some(), "the bomber went down");
}

/// Each aircraft flies at its share of the T3 spy plane's height ([air.heights] in
/// data/balance_targets.toml), and the spy plane flies highest of all.
#[test]
fn aircraft_fly_at_their_shares_of_the_spy_planes_height() {
    let targets: toml::Table = include_str!("../data/balance_targets.toml").parse().expect("valid targets");
    let db = sim_engine::sim::blueprints::BlueprintDb::standard();
    let height = |key: &str| db.get(db.id(key)).flight.unwrap_or_else(|| panic!("{key} flies")).elevation.to_num::<f64>();
    let top = height("t3_spy_plane");
    let shares = targets["air"]["heights"].as_table().expect("[air.heights]");
    assert!(shares.len() >= 10, "only {} heights", shares.len());
    for (key, share) in shares {
        let share = share.as_float().expect("a share");
        assert!((height(key) - top * share).abs() <= 1.0, "{key} flies at {}, not {share} of {top}", height(key));
    }
    for unit in db.iter().filter(|unit| unit.flight.is_some() && unit.key != "t3_spy_plane") {
        assert!(height(&unit.key) < top, "{} flies as high as the spy plane", unit.key);
    }
}

/// Aircraft go where they are ordered over a sea with no land anywhere, as on the balance
/// harness's open sea (2026-11-24): their orders took a land route, found none, and were dropped,
/// so they circled where they were.
#[test]
fn aircraft_obey_orders_over_open_sea() {
    let map_units = 5_120;
    let cells = (map_units / sim_engine::sim::config::TERRAIN_CELL_UNITS) as usize;
    let heights = vec![(50 << sim_engine::sim::terrain::HEIGHT_FRAC_BITS) as i16; (cells + 1) * (cells + 1)];
    let terrain = sim_engine::sim::terrain::Terrain::from_heights(map_units, heights, &vec![true; cells * cells], fp_int(100));
    let config = SimConfig { map_units, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::with_terrain(config, 1, terrain);
    world.prepare_map();
    let gunship = spawn(&mut world, "t2_gunship", Team::Blue, point(1_000, 2_560));
    let fighter = spawn(&mut world, "t1_interceptor", Team::Blue, point(1_000, 2_660));
    world.advance_tick(&[SimCommand::AttackMove { units: vec![gunship, fighter], goal: point(4_000, 2_560) }]);
    run(&mut world, 600);
    for id in [gunship, fighter] {
        let x = world.unit_position(id).expect("still flying").x.to_num::<i32>();
        assert!(x > 3_000, "it should fly out over the sea, not stay at {x}");
    }
}
