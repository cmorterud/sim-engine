//! Artillery (GAME_SPECIFICATION.md §7): it cannot hit what is too close, its shells scatter, an
//! installation's shot costs energy, and generators beside one cut that cost and its reload.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp, Vec3Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Artillery, Combat, Health, Projectile, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::SimWorld;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn world(map_units: i32) -> SimWorld {
    let config = SimConfig { map_units, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
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

fn structure(world: &mut SimWorld, key: &str, team: Team, cell_x: u32, cell_y: u32) -> SimId {
    world.spawn_finished_structure(world.blueprints.id(key), team, cell_x, cell_y).unwrap()
}

/// The shells this unit has in the air, with their velocities.
fn shells(world: &SimWorld, source: SimId) -> Vec<Vec3Fp> {
    world.projectiles.iter().filter_map(|&(_, entity)| world.ecs.get::<&Projectile>(entity).ok().filter(|shell| shell.source == source).map(|shell| shell.velocity)).collect()
}

/// Runs until the unit fires, and returns the tick it did.
fn first_shot(world: &mut SimWorld, source: SimId, limit: u32) -> Option<u32> {
    (0..limit).find(|_| {
        world.advance_tick(&[]);
        !shells(world, source).is_empty()
    })
}

/// A tank driving steadily across a weapon's front, with health enough to take everything, that
/// does not shoot back.
fn passing_tank(world: &mut SimWorld, from: Vec2Fp, to: Vec2Fp) -> SimId {
    let tank = world.spawn_mobile(world.blueprints.id("t1_medium_tank"), Team::Red, from);
    let entity = world.unit_entity(tank).unwrap();
    world.ecs.get::<&mut Combat>(entity).unwrap().range = Fp::ZERO;
    {
        let mut health = world.ecs.get::<&mut Health>(entity).unwrap();
        health.max = fp_int(100_000);
        health.current = health.max;
    }
    world.advance_tick(&[SimCommand::Move { units: vec![tank], goal: to }]);
    tank
}

/// Runs, and returns how many shots `source` fired.
fn count_shots(world: &mut SimWorld, source: SimId, ticks: u32) -> usize {
    let mut fired = std::collections::BTreeSet::new();
    for _ in 0..ticks {
        world.advance_tick(&[]);
        for &(id, shell) in &world.projectiles {
            if world.ecs.get::<&Projectile>(shell).is_ok_and(|shell| shell.source == source) {
                fired.insert(id);
            }
        }
    }
    fired.len()
}

/// Artillery leads its target as every shell does (GAME_SPECIFICATION.md §7.1, Leading Shots): a
/// tank driving steadily past is hit, where before the shells fell where it had been.
#[test]
fn artillery_leads_a_target_moving_steadily() {
    let mut world = world(5_120);
    let artillery = world.spawn_mobile(world.blueprints.id("t3_mobile_artillery"), Team::Blue, point(1_000, 1_000));
    // No scatter, so that only the aim is measured.
    world.ecs.get::<&mut Artillery>(world.unit_entity(artillery).unwrap()).unwrap().spread = Fp::ZERO;
    let tank = passing_tank(&mut world, point(200, 2_100), point(3_000, 2_100));
    let fired = count_shots(&mut world, artillery, 300);
    for _ in 0..60 {
        world.advance_tick(&[]);
    }
    let hits = ((fp_int(100_000) - world.unit_health(tank).unwrap()) / fp_int(750)).to_num::<usize>();
    assert!(fired >= 2, "it should have fired a few times, not {fired}");
    assert!(hits * 2 >= fired, "most shells should land on a tank driving steadily: {hits} of {fired}");
}

/// Artillery leads a tank on any steady course: driving across its front, straight at it, or
/// straight away. Until 2026-11-19 the aim was refined only twice from where the tank was, which
/// for slow shells fell well short of a tank coming on and of one going away.
#[test]
fn artillery_leads_a_target_on_any_steady_course() {
    // Each starts out of range, so that it is at full speed by the time it is fired at.
    for (name, start, goal) in [
        ("across", point(0, 1_500), point(2_400, 1_500)),
        ("toward", point(1_000, 2_000), point(1_000, 1_250)),
        ("away", point(1_000, 1_000 - 100), point(1_000, 2_400)),
    ] {
        let mut world = world(5_120);
        // Four of the slowest shells, with no scatter and no splash, so that only the aim counts.
        let mut batteries = Vec::new();
        for x in [880, 960, 1_040, 1_120] {
            let artillery = world.spawn_mobile(world.blueprints.id("t1_light_artillery"), Team::Blue, point(x, 1_000));
            let entity = world.unit_entity(artillery).unwrap();
            world.ecs.get::<&mut Artillery>(entity).unwrap().spread = Fp::ZERO;
            world.ecs.get::<&mut Combat>(entity).unwrap().projectile_splash = Fp::ZERO;
            batteries.push(artillery);
        }
        let damage = world.ecs.get::<&Combat>(world.unit_entity(batteries[0]).unwrap()).unwrap().projectile_damage;
        let tank = passing_tank(&mut world, start, goal);
        let mut fired = std::collections::BTreeSet::new();
        for _ in 0..300 {
            world.advance_tick(&[]);
            for &(id, shell) in &world.projectiles {
                if world.ecs.get::<&Projectile>(shell).is_ok_and(|shell| batteries.contains(&shell.source)) {
                    fired.insert(id);
                }
            }
        }
        let fired = fired.len();
        let hits = ((fp_int(100_000) - world.unit_health(tank).unwrap()) / damage).to_num::<usize>();
        println!("{name}: {hits} of {fired}");
        assert!(fired >= 4, "{name}: they should have fired several times, not {fired}");
        assert!(hits * 10 >= fired * 9, "{name}: nearly every shell should land on a tank driving steadily: {hits} of {fired}");
    }
}

/// Cruise missiles do not lead, and do not follow a target that moves: they fly to where it was
/// when they were launched, so that their long range is no help against what keeps moving.
#[test]
fn cruise_missiles_land_where_a_moving_target_was() {
    let mut world = world(5_120);
    let launcher = world.spawn_mobile(world.blueprints.id("t2_mobile_missile_launcher"), Team::Blue, point(1_000, 1_000));
    let tank = passing_tank(&mut world, point(300, 2_000), point(3_000, 2_000));
    let fired = count_shots(&mut world, launcher, 200);
    for _ in 0..80 {
        world.advance_tick(&[]);
    }
    assert!(fired >= 2, "it should have fired at the tank, not {fired} times");
    assert_eq!(world.unit_health(tank), Some(fp_int(100_000)), "every missile lands behind a tank that keeps driving");
}

#[test]
fn artillery_cannot_hit_what_is_too_close() {
    let mut world = world(5_120);
    let gun = structure(&mut world, "t2_artillery", Team::Blue, 100, 100);
    // A storage 600 away, inside its least range of 1,000.
    structure(&mut world, "mass_storage", Team::Red, 130, 100);
    assert_eq!(first_shot(&mut world, gun, 300), None, "too close to shoot at");
    // One 1,600 away is in reach.
    structure(&mut world, "mass_storage", Team::Red, 180, 100);
    assert!(first_shot(&mut world, gun, 400).is_some());
}

#[test]
fn an_installation_waits_for_the_energy_a_shot_costs() {
    let mut world = world(5_120);
    let gun = structure(&mut world, "t2_artillery", Team::Blue, 100, 100);
    structure(&mut world, "mass_storage", Team::Red, 180, 100);
    world.economies[0].energy = Res::from_num(500);
    world.economies[0].energy_income = Res::ZERO;
    assert_eq!(first_shot(&mut world, gun, 300), None, "500 energy is not enough for a shot of 1,000");
    world.economies[0].energy = Res::from_num(5_000);
    let before = world.economies[0].energy;
    assert!(first_shot(&mut world, gun, 300).is_some());
    let spent = before - world.economies[0].energy;
    assert!((spent - Res::from_num(1_000)).abs() < Res::from_num(5), "a shot costs 1,000: {spent}");
}

#[test]
fn a_t2_generator_beside_an_installation_cheapens_and_speeds_its_shots() {
    let shot_cost_and_reload = |generator: bool| {
        let mut world = world(5_120);
        let gun = structure(&mut world, "t2_artillery", Team::Blue, 100, 100);
        if generator {
            structure(&mut world, "power_generator_t2", Team::Blue, 101, 99);
        }
        structure(&mut world, "mass_storage", Team::Red, 180, 100);
        world.advance_tick(&[]);
        let income = world.economies[0].energy_income;
        let before = world.economies[0].energy;
        let first = first_shot(&mut world, gun, 300).expect("it fires");
        let spent = before - world.economies[0].energy + income * i64::from(first + 1);
        let ids = |world: &SimWorld| -> Vec<SimId> {
            world.projectiles.iter().filter(|&&(_, entity)| world.ecs.get::<&Projectile>(entity).is_ok_and(|shell| shell.source == gun)).map(|projectile| projectile.0).collect()
        };
        let fired = ids(&world);
        let second = (0..400).find(|_| {
            world.advance_tick(&[]);
            ids(&world).iter().any(|id| !fired.contains(id))
        });
        (spent, second.expect("it fires again") + 1)
    };
    let (cost, reload) = shot_cost_and_reload(false);
    let (cheaper, faster) = shot_cost_and_reload(true);
    assert!((cost - Res::from_num(1_000)).abs() < Res::from_num(5), "{cost}");
    assert!((cheaper - Res::from_num(950)).abs() < Res::from_num(5), "5% off each shot: {cheaper}");
    assert_eq!(reload, 200);
    assert_eq!(faster, 187, "6.25% off the 200 ticks between shots");
}

#[test]
fn artillery_shells_scatter_around_their_aim_the_same_way_every_time() {
    let volleys = || {
        let mut world = world(5_120);
        let gun = world.spawn_mobile(world.blueprints.id("t3_mobile_artillery"), Team::Blue, point(1_000, 2_500));
        world.advance_tick(&[SimCommand::AttackGround { units: vec![gun], point: point(2_200, 2_500) }]);
        let mut seen: Vec<Vec3Fp> = Vec::new();
        for _ in 0..500 {
            world.advance_tick(&[]);
            for velocity in shells(&world, gun) {
                if !seen.iter().any(|known| known.x == velocity.x && known.y == velocity.y) {
                    seen.push(velocity);
                }
            }
        }
        seen
    };
    let first = volleys();
    assert!(first.len() >= 4, "it should fire again and again: {}", first.len());
    // Aimed at one spot, the shells go different ways, but not far: within 25 over the 1,200.
    let directions: Vec<i64> = first.iter().map(|velocity| i64::from(velocity.y.to_bits()) * 1_000 / i64::from(velocity.x.to_bits())).collect();
    assert!(directions.windows(2).any(|pair| pair[0] != pair[1]), "they should scatter: {directions:?}");
    assert!(directions.iter().all(|slope| slope.abs() <= 25), "but only a little: {directions:?}");
    let again: Vec<(i32, i32)> = volleys().iter().map(|velocity| (velocity.x.to_bits(), velocity.y.to_bits())).collect();
    assert_eq!(first.iter().map(|velocity| (velocity.x.to_bits(), velocity.y.to_bits())).collect::<Vec<_>>(), again, "and the same in every replay");
}

/// The directions a unit's shells set off in, over a few volleys at a spot on the ground.
fn shell_directions(kind: &str, team: Team, from: Vec2Fp, at: Vec2Fp, ticks: u32) -> Vec<(i32, i32)> {
    let mut world = world(5_120);
    let gun = world.spawn_mobile(world.blueprints.id(kind), team, from);
    world.advance_tick(&[SimCommand::AttackGround { units: vec![gun], point: at }]);
    let mut seen: Vec<(i32, i32)> = Vec::new();
    for _ in 0..ticks {
        world.advance_tick(&[]);
        for velocity in shells(&world, gun) {
            let key = (velocity.x.to_bits(), velocity.y.to_bits());
            if !seen.contains(&key) {
                seen.push(key);
            }
        }
    }
    seen
}

#[test]
fn guns_with_firing_randomness_scatter_and_mirror_images_scatter_mirrored() {
    // FAF's light artillery has a firing randomness of 0.5, a spread of 10 here; the light bot
    // has none, so every shot it fires at one spot goes the same way.
    let scattering = shell_directions("t1_light_artillery", Team::Blue, point(1_000, 2_500), point(1_500, 2_500), 600);
    assert!(scattering.len() >= 3, "it should fire several times: {}", scattering.len());
    assert!(scattering.windows(2).any(|pair| pair[0] != pair[1]), "its shells should go different ways: {scattering:?}");
    let steady = shell_directions("t1_light_walker", Team::Blue, point(1_000, 2_500), point(1_200, 2_500), 100);
    assert_eq!(steady.len(), 1, "a gun without randomness fires the same way every time: {steady:?}");
    // The same gun on the other side, turned about the middle of the map, scatters the same way
    // turned about, so a mirrored match stays a mirror image.
    let mirrored = shell_directions("t1_light_artillery", Team::Red, point(4_120, 2_620), point(3_620, 2_620), 600);
    let turned: Vec<(i32, i32)> = mirrored.iter().map(|&(x, y)| (-x, -y)).collect();
    let close = |a: (i32, i32), b: (i32, i32)| (a.0 - b.0).abs() <= 2 && (a.1 - b.1).abs() <= 2;
    assert_eq!(turned.len(), scattering.len());
    assert!(turned.iter().zip(&scattering).all(|(a, b)| close(*a, *b)), "Red's {turned:?} against Blue's {scattering:?}");
}

#[test]
fn heavy_artillery_reaches_across_the_map() {
    let mut world = world(20_480);
    let gun = structure(&mut world, "t3_artillery", Team::Blue, 100, 500);
    let target = structure(&mut world, "power_generator", Team::Red, 600, 500);
    assert!(first_shot(&mut world, gun, 200).is_some(), "10,000 away is well within its 16,500");
    let hit = (0..400).any(|_| {
        world.advance_tick(&[]);
        world.unit_health(target).is_none()
    });
    assert!(hit, "7,840 damage takes the generator");
}

#[test]
fn a_mobile_missile_launcher_fires_on_its_own_but_not_up_close() {
    let mut world = world(5_120);
    let launcher = world.spawn_mobile(world.blueprints.id("t2_mobile_missile_launcher"), Team::Blue, point(1_000, 2_500));
    // A storage 150 away, inside its least range of 240.
    structure(&mut world, "mass_storage", Team::Red, 57, 125);
    assert_eq!(first_shot(&mut world, launcher, 200), None, "too close");
    // One 1,000 away is in reach.
    let far = structure(&mut world, "mass_storage", Team::Red, 100, 125);
    assert!(first_shot(&mut world, launcher, 200).is_some(), "it fires on its own");
    let gone = (0..600).any(|_| {
        world.advance_tick(&[]);
        world.unit_health(far).is_none()
    });
    assert!(gone);
}
