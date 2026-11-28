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

/// Two launchers placed alike either side of the target, so that their missiles come into a
/// defense's reach on the same tick. Returns the world and the target.
fn twin_volley(defenses: &[(u32, u32)], mounts: u8) -> (SimWorld, SimId) {
    let mut world = open_world();
    let launchers: Vec<SimId> = [17, 25].into_iter().map(|cell_y| loaded(&mut world, "tactical_missile_launcher", Team::Blue, 20, cell_y, 1)).collect();
    let target = structure(&mut world, "power_generator", Team::Red, 100, 21);
    for &(cell_x, cell_y) in defenses {
        let defense = structure(&mut world, "tactical_missile_defense", Team::Red, cell_x, cell_y);
        world.ecs.get::<&mut MissileDefense>(world.unit_entity(defense).unwrap()).unwrap().mounts = mounts;
    }
    world.advance_tick(&[SimCommand::Attack { units: launchers, target }]);
    (world, target)
}

/// Interceptions each tick, Red's, for so many ticks.
fn interceptions_by_tick(world: &mut SimWorld, ticks: u32) -> Vec<Vec<[[i32; 3]; 2]>> {
    (0..ticks)
        .map(|_| {
            world.advance_tick(&[]);
            world.snapshot_for(Some(Team::Red)).interceptions
        })
        .collect()
}

/// Each mount is a defense of its own, with its own reload (2026-11-25): two mounts take two
/// missiles that come into reach together, on the same tick, where one mount takes one.
#[test]
fn each_mount_of_a_missile_defense_takes_a_missile_of_its_own() {
    let (mut world, target) = twin_volley(&[(102, 21)], 2);
    let ticks = interceptions_by_tick(&mut world, 200);
    assert_eq!(ticks.iter().map(Vec::len).sum::<usize>(), 2, "two missiles, each shot down once");
    assert!(ticks.iter().any(|tick| tick.len() == 2), "both mounts fire on the tick the missiles come into reach");
    assert!(alive(&world, target));

    let (mut world, _) = twin_volley(&[(102, 21)], 1);
    let ticks = interceptions_by_tick(&mut world, 200);
    assert!(ticks.iter().all(|tick| tick.len() <= 1), "one mount fires once a reload");
}

/// However many defenses are ready, no missile is aimed at twice, and every missile aimed at is
/// destroyed: three defenses against two missiles shoot down exactly two, once each.
#[test]
fn no_missile_is_aimed_at_by_two_defenses_and_every_one_aimed_at_is_destroyed() {
    let (mut world, target) = twin_volley(&[(102, 18), (102, 21), (102, 24)], 1);
    let ticks = interceptions_by_tick(&mut world, 200);
    let all: Vec<[i32; 3]> = ticks.iter().flatten().map(|interception| interception[1]).collect();
    assert_eq!(all.len(), 2, "three defenses, two missiles: {all:?}");
    assert_ne!(all[0], all[1], "the two shots were at two missiles");
    assert!(world.missiles.is_empty(), "both are gone");
    assert!(alive(&world, target), "neither got through");
}

/// Every tactical missile defense mount, on a structure or a ship, stops a missile every 1.8
/// seconds; the battleship and the cruiser carry two, and the battlecruiser one (2026-11-25).
#[test]
fn every_tactical_missile_defense_mount_reloads_in_one_point_eight_seconds() {
    let db = sim_engine::sim::blueprints::BlueprintDb::standard();
    let mut checked = 0;
    for unit in db.iter() {
        if let Some(defense) = unit.missile_defense.filter(|defense| !defense.strategic) {
            assert_eq!(defense.reload_ticks, 18, "{}'s mounts reload in {} ticks", unit.key, defense.reload_ticks);
            checked += 1;
        }
    }
    assert!(checked >= 4, "only {checked}");
    let mounts = |key: &str| db.get(db.id(key)).missile_defense.map_or(0, |defense| defense.mounts);
    assert_eq!([mounts("tactical_missile_defense"), mounts("t3_battlecruiser"), mounts("t3_battleship"), mounts("t2_cruiser")], [1, 1, 2, 2]);
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
    let full = world.blueprints.get(world.blueprints.id("power_generator")).health;
    assert_eq!(health(&world, edge), full - fp_int(500), "the outer ring does 500");
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

/// A strategic launch is heard by everyone as a warning, and its strike, and a commander blowing
/// up as it dies, anywhere on the map: each player's snapshot carries them, seen or not. With fog
/// of war, Red sees nothing of the Blue silo 3,000 away that fires at it, but hears the launch.
#[test]
fn a_strategic_launch_its_strike_and_a_commander_blowing_up_reach_every_player_seen_or_not() {
    use sim_engine::sim::world::DetonationKind;
    let (mut world, silo, _, _) = nuclear_standoff();
    world.config.fog_of_war = true;
    let rig = world.blueprints.id("command_rig");
    let commander = world.spawn_mobile(rig, Team::Red, point(3_320, 450));
    world.advance_tick(&[SimCommand::AttackGround { units: vec![silo], point: ground_zero() }]);
    let mut launches = Vec::new();
    let mut struck = Vec::new();
    for _ in 0..600 {
        for viewer in [Team::Blue, Team::Red] {
            let snapshot = world.snapshot_for(Some(viewer));
            if viewer == Team::Red && !snapshot.strategic_launches.is_empty() {
                assert!(snapshot.projectiles.iter().all(|shot| shot.kind != sim_engine::sim::world::ProjectileKind::StrategicMissile), "Red should not see the missile leave its silo");
            }
            launches.extend(snapshot.strategic_launches.iter().map(|&team| (viewer, team)));
            struck.extend(snapshot.detonations.iter().map(|detonation| (viewer, detonation.kind)));
        }
        if !alive(&world, commander) && struck.iter().any(|(_, kind)| *kind == DetonationKind::Commander) {
            break;
        }
        world.advance_tick(&[]);
    }
    assert_eq!(launches, [(Team::Blue, Team::Blue), (Team::Red, Team::Blue)], "one launch, heard by both players");
    for viewer in [Team::Blue, Team::Red] {
        let kinds: Vec<DetonationKind> = struck.iter().filter(|(seen_by, _)| *seen_by == viewer).map(|(_, kind)| *kind).collect();
        assert_eq!(kinds.iter().filter(|kind| **kind == DetonationKind::StrategicMissile).count(), 1, "{viewer:?} hears one strike: {kinds:?}");
        assert_eq!(kinds.iter().filter(|kind| **kind == DetonationKind::Commander).count(), 1, "{viewer:?} hears the commander blow up: {kinds:?}");
    }
}

/// Where a projectile is, by its id.
fn projectile_at(world: &SimWorld, id: SimId) -> Option<sim_engine::math::fp::Vec3Fp> {
    world.projectiles.iter().find(|projectile| projectile.0 == id).and_then(|&(_, entity)| world.ecs.get::<&sim_engine::sim::components::Position>(entity).ok().map(|position| position.0))
}

/// A silo opens its doors before its missile leaves, the missile rises slowly out once they are
/// open, and they close behind it (2026-11-25). The missile then climbs straight up to 2,000 over
/// the ground, flies level, comes straight down, all at one speed, and bursts on the ground at
/// its target, with nothing on the way stopping it: here an enemy shield right over its target
/// and an aircraft hanging in its dive.
#[test]
fn a_nuclear_missile_leaves_its_silo_climbs_flies_level_and_comes_straight_down() {
    use sim_engine::sim::world::{SILO_DOOR_TICKS, SILO_SEQUENCE_TICKS};
    let (mut world, silo, base, _) = nuclear_standoff();
    structure(&mut world, "shield_generator", Team::Red, 172, 24);
    let aircraft = world.spawn_mobile(world.blueprints.id("t3_air_superiority"), Team::Red, ground_zero());
    world.ecs.get::<&mut Combat>(world.unit_entity(aircraft).unwrap()).unwrap().enabled = false;
    world.advance_tick(&[SimCommand::AttackGround { units: vec![silo], point: ground_zero() }]);
    let mut doors = Vec::new();
    let mut launched_at = None;
    for tick in 1..=SILO_DOOR_TICKS + 5 {
        doors.push(world.silo_doors().first().map_or(0, |&(_, open)| open));
        if launched_at.is_none() && missiles_from(&world, silo) > 0 {
            launched_at = Some(tick);
        }
        world.advance_tick(&[]);
    }
    let launched_at = launched_at.expect("the missile left");
    assert!((SILO_DOOR_TICKS - 1..=SILO_DOOR_TICKS + 1).contains(&launched_at), "it left {launched_at} ticks after the order, not once the doors were open");
    assert!(doors.windows(2).take(SILO_DOOR_TICKS as usize - 2).all(|pair| pair[1] > pair[0]), "the doors open steadily: {doors:?}");
    assert!(doors[SILO_DOOR_TICKS as usize..].iter().all(|&open| open == 1_000), "and stay open while it rises: {doors:?}");

    let (id, _) = *world.missiles.first().expect("in flight");
    let ground = world.terrain.height_at(world.unit_position(silo).unwrap().xy());
    let mut path = vec![projectile_at(&world, id).unwrap()];
    for _ in 0..1_000 {
        world.advance_tick(&[]);
        match projectile_at(&world, id) {
            Some(at) => path.push(at),
            None => break,
        }
    }
    let steps: Vec<f64> = path.windows(2).map(|pair| (pair[1] - pair[0]).length_squared().to_num::<f64>().sqrt()).collect();
    // It rises out at 3 a tick, then flies at its 25, however it turns.
    assert!(steps[..10].iter().all(|&step| (step - 3.0).abs() < 0.01), "the lift: {:?}", &steps[..10]);
    let flying = &steps[25..steps.len() - 1];
    assert!(flying.iter().all(|&step| (step - 25.0).abs() <= 1.0), "one speed in flight: {:?}", flying.iter().filter(|&&step| (step - 25.0).abs() > 1.0).collect::<Vec<_>>());
    let top = path.iter().map(|at| at.z).max().unwrap();
    assert!((top - ground - fp_int(2_000)).abs() <= fp_int(1), "it climbed to {top}");
    // Straight up from the silo, and straight down onto the target.
    let over_silo = path.iter().take_while(|at| at.z < ground + fp_int(1_500)).all(|at| at.xy() == path[0].xy());
    assert!(over_silo, "it climbs straight up");
    let last = *path.last().unwrap();
    assert!((last.xy() - ground_zero()).length_squared() <= sim_engine::math::fp::FpWide::from_num(1), "it struck {last:?}");
    // The last place it was seen is the tick before it burst, at most one step above the ground.
    let above = last.z - world.terrain.height_at(ground_zero());
    assert!(above > Fp::ZERO && above <= fp_int(25), "one step from the ground: {above}");
    let dive: Vec<_> = path.iter().rev().take_while(|at| at.z < ground + fp_int(1_500)).collect();
    assert!(dive.iter().all(|at| (at.xy() - ground_zero()).length_squared() <= sim_engine::math::fp::FpWide::from_num(4)), "it comes straight down");
    assert!(base.iter().all(|&id| !alive(&world, id)), "the dome and the aircraft over it did not stop it");
    // The doors closed behind it.
    run_until(&mut world, SILO_SEQUENCE_TICKS, |_| false);
    assert!(world.silo_doors().is_empty());
}

/// What a strategic defense's interceptor and the nuclear missile it chases look like, tick by
/// tick, to the team defending: where each is, until both are gone.
struct Chase {
    /// The interceptor's positions, as raw `Fp` bits, from its first snapshot.
    interceptor: Vec<[i32; 3]>,
    /// The missile's, over the same ticks and before, until it is gone.
    missile: Vec<[i32; 3]>,
    /// Where the snapshots flashed an interception.
    flashes: Vec<[[i32; 3]; 2]>,
    /// Whether anyone heard a nuclear strike.
    struck: bool,
}

/// Orders the standoff's silo to fire at the base, with a Red defense at a cell, and follows the
/// chase. `after_launch` runs once, on the tick the interceptor is first seen.
fn chase(defense_cell: (u32, u32), mut after_launch: impl FnMut(&mut SimWorld, SimId)) -> (SimWorld, Vec<SimId>, SimId, Chase) {
    use sim_engine::sim::world::{DetonationKind, ProjectileKind};
    let (mut world, silo, base, _) = nuclear_standoff();
    let defense = structure(&mut world, "strategic_missile_defense", Team::Red, defense_cell.0, defense_cell.1);
    world.ecs.get::<&mut MissileDefense>(world.unit_entity(defense).unwrap()).unwrap().stock = 1;
    world.advance_tick(&[SimCommand::AttackGround { units: vec![silo], point: ground_zero() }]);
    let mut seen = Chase { interceptor: Vec::new(), missile: Vec::new(), flashes: Vec::new(), struck: false };
    for _ in 0..1_000 {
        world.advance_tick(&[]);
        let snapshot = world.snapshot_for(Some(Team::Red));
        let interceptor = snapshot.projectiles.iter().find(|view| view.kind == ProjectileKind::Interceptor && view.source == defense);
        let missile = snapshot.projectiles.iter().find(|view| view.kind == ProjectileKind::StrategicMissile);
        seen.flashes.extend(snapshot.interceptions.iter().copied());
        seen.struck |= snapshot.detonations.iter().any(|detonation| detonation.kind == DetonationKind::StrategicMissile);
        if let Some(view) = interceptor {
            if seen.interceptor.is_empty() {
                after_launch(&mut world, defense);
            }
            seen.interceptor.push([view.x, view.y, view.z]);
        }
        if let Some(view) = missile {
            seen.missile.push([view.x, view.y, view.z]);
        }
        if missile.is_none() && interceptor.is_none() && !seen.missile.is_empty() {
            break;
        }
    }
    (world, base, defense, seen)
}

fn apart(a: [i32; 3], b: [i32; 3]) -> f64 {
    let raw = |value: i32| f64::from(value) / 4096.0;
    (raw(a[0]) - raw(b[0])).hypot(raw(a[1]) - raw(b[1])).hypot(raw(a[2]) - raw(b[2]))
}

/// A strategic defense fires an interceptor of its own, a missile seen flying up from it, and it
/// meets the nuclear missile further along its path: the two are gone on the same tick, where
/// they met, with a flash there, and nothing strikes the ground or is heard striking. It never
/// misses (2026-11-26, the user's call): not when the defense is destroyed while it flies, and
/// not when the missile only comes into range just before it would strike.
#[test]
fn a_strategic_defense_fires_an_interceptor_that_meets_the_missile_and_never_misses() {
    use sim_engine::sim::world::INTERCEPTOR_FLIGHT_TICKS;
    let destroy = |world: &mut SimWorld, defense: SimId| world.strike(defense, fp_int(100_000));
    // Under the missile's path, its target at the edge of its range, and one destroyed as soon
    // as it has fired.
    let cases: [(&str, (u32, u32), bool); 3] = [("under the path", (165, 30), false), ("at the edge of its range", (170, 110), false), ("destroyed", (165, 30), true)];
    for (case, cell, destroyed) in cases {
        let (world, base, defense, seen) = chase(cell, |world, defense| {
            if destroyed {
                destroy(world, defense);
            }
        });
        assert!(!seen.interceptor.is_empty(), "{case}: an interceptor is seen in flight");
        assert!(seen.interceptor.len() <= INTERCEPTOR_FLIGHT_TICKS as usize, "{case}: it flies {} ticks", seen.interceptor.len());
        let (first, last) = (seen.interceptor[0], *seen.interceptor.last().unwrap());
        assert!(last[2] > first[2], "{case}: it climbs to the missile");
        let met = *seen.missile.last().unwrap();
        assert!(apart(last, met) < 1.0, "{case}: the two meet, {:.1} apart when last seen", apart(last, met));
        assert!(seen.flashes.iter().any(|flash| apart(flash[1], met) < 1.0), "{case}: with a flash where they meet");
        assert!(!seen.struck, "{case}: no strike is heard");
        assert!(base.iter().all(|id| alive(&world, *id)), "{case}: the base stands");
        assert!(world.missiles.is_empty(), "{case}: the missile is gone");
        assert_eq!(alive(&world, defense), !destroyed, "{case}: the defense is there unless destroyed");
    }
}
