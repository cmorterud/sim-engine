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
    let gun = world.blueprints.get(world.blueprints.id("t3_mobile_artillery")).weapon.expect("artillery has a gun");
    // No scatter, so that only the aim is measured.
    world.ecs.get::<&mut Artillery>(world.unit_entity(artillery).unwrap()).unwrap().spread = Fp::ZERO;
    // Driving past at three quarters of its reach, beyond its least range.
    let across = 1_000 + gun.range.to_num::<i32>() * 3 / 4;
    let tank = passing_tank(&mut world, point(200, across), point(3_000, across));
    let fired = count_shots(&mut world, artillery, 300);
    for _ in 0..60 {
        world.advance_tick(&[]);
    }
    let hits = ((fp_int(100_000) - world.unit_health(tank).unwrap()) / gun.damage).to_num::<usize>();
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
        // Tough enough to take every shell, now that they come down on it, so that the gun fires again.
        let storage = structure(&mut world, "mass_storage", Team::Red, 180, 100);
        {
            let mut health = world.ecs.get::<&mut Health>(world.unit_entity(storage).unwrap()).unwrap();
            health.max = fp_int(100_000);
            health.current = health.max;
        }
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
    let weapon = sim_engine::sim::blueprints::BlueprintDb::standard();
    let weapon = weapon.get(weapon.id("t3_mobile_artillery")).weapon.expect("artillery has a gun");
    // At four fifths of its reach, beyond its least range.
    let distance = weapon.range.to_num::<i64>() * 4 / 5;
    let volleys = || {
        let mut world = world(5_120);
        let gun = world.spawn_mobile(world.blueprints.id("t3_mobile_artillery"), Team::Blue, point(1_000, 2_500));
        world.advance_tick(&[SimCommand::AttackGround { units: vec![gun], point: point(1_000 + distance as i32, 2_500) }]);
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
    // Aimed at one spot, the shells go different ways, but not far: within its spread, over the
    // distance, in thousandths.
    let directions: Vec<i64> = first.iter().map(|velocity| i64::from(velocity.y.to_bits()) * 1_000 / i64::from(velocity.x.to_bits())).collect();
    let most = weapon.spread.to_num::<i64>() * 1_000 / distance + 1;
    assert!(directions.windows(2).any(|pair| pair[0] != pair[1]), "they should scatter: {directions:?}");
    assert!(directions.iter().all(|slope| slope.abs() <= most), "but only a little, within {most}: {directions:?}");
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
    // The inspiration's light artillery has a firing randomness of 0.5, a spread of 10 here; the
    // light bot has none, so every shot it fires at one spot goes the same way.
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

/// Every artillery unit that sets up before it fires (`setup_ticks`), with its blueprint key.
fn setting_up(world: &SimWorld) -> Vec<(String, u32)> {
    let units: Vec<(String, u32)> = world
        .blueprints
        .iter()
        .filter_map(|def| def.weapon.filter(|weapon| weapon.setup_ticks > 0).map(|weapon| (def.key.clone(), weapon.setup_ticks)))
        .collect();
    assert!(units.iter().any(|(key, _)| key == "t3_mobile_artillery"), "the T3 artillery sets up before it fires");
    units
}

/// Spawns `key` for Blue facing a tank that does not shoot back, at three quarters of its range,
/// and returns both. With `setup` false, the unit fires without setting up.
fn artillery_facing_a_tank(key: &str, setup: bool) -> (SimWorld, SimId, SimId) {
    let mut world = world(5_120);
    let unit = world.spawn_mobile(world.blueprints.id(key), Team::Blue, point(1_000, 1_000));
    let range = world.blueprints.get(world.blueprints.id(key)).weapon.expect("artillery has a gun").range.to_num::<i32>();
    let tank = passing_tank(&mut world, point(1_000, 1_000 + range * 3 / 4), point(1_000, 1_000 + range * 3 / 4));
    if !setup {
        world.ecs.remove_one::<sim_engine::sim::components::Setup>(world.unit_entity(unit).unwrap()).unwrap();
    }
    (world, unit, tank)
}

/// From a standing start, setting up costs nothing it would not have spent anyway: it sets up
/// while it finds and aims at its target (GAME_SPECIFICATION.md §7).
#[test]
fn artillery_that_sets_up_loses_nothing_from_a_standing_start() {
    for (key, ticks) in setting_up(&world(5_120)) {
        let (mut free, free_unit, _) = artillery_facing_a_tank(&key, false);
        let unpacked = first_shot(&mut free, free_unit, 200).unwrap_or_else(|| panic!("{key} fires without setting up"));
        let (mut world, unit, _) = artillery_facing_a_tank(&key, true);
        let shot = first_shot(&mut world, unit, 200).unwrap_or_else(|| panic!("{key} fires once set up"));
        assert!(shot + 1 >= ticks, "{key} fired on tick {shot}, before its {ticks}-tick setup");
        assert!(shot <= unpacked.max(ticks), "{key} fired on tick {shot}, later than setup ({ticks}) or aiming ({unpacked}) needed");
    }
}

/// Moving packs it up: on a move past a target in range it never fires, though the same unit
/// without a setup fires on the move. Once it stops, already aimed and loaded, it fires after its
/// setup: not before, and not much after, while it comes to a stop.
#[test]
fn artillery_that_sets_up_never_fires_on_the_move() {
    let db = world(5_120);
    for (key, ticks) in setting_up(&db) {
        for setup in [false, true] {
            let (mut world, unit, _) = artillery_facing_a_tank(&key, setup);
            world.advance_tick(&[SimCommand::Move { units: vec![unit], goal: point(1_600, 1_000) }]);
            let moving = count_shots(&mut world, unit, 100);
            if setup {
                assert_eq!(moving, 0, "{key} fired {moving} shells on the move");
                world.advance_tick(&[SimCommand::Stop { units: vec![unit] }]);
                let shot = first_shot(&mut world, unit, 200).unwrap_or_else(|| panic!("{key} fires once it has stopped and set up"));
                assert!((ticks..=ticks + 6).contains(&(shot + 1)), "{key} fired {} ticks after stopping, for a {ticks}-tick setup", shot + 1);
            } else {
                assert!(moving > 0, "{key} without a setup fires on the move, so the test means something");
            }
        }
    }
}

/// Every artillery weapon that fires shells (not missiles), with its blueprint key.
fn shell_artillery(world: &SimWorld) -> Vec<String> {
    let keys: Vec<String> = world
        .blueprints
        .iter()
        .filter(|def| def.weapon.is_some_and(|weapon| weapon.artillery && weapon.kind == sim_engine::sim::blueprints::WeaponKind::Shell))
        .map(|def| def.key.clone())
        .collect();
    for key in ["t1_light_artillery", "t3_mobile_artillery", "t3_artillery"] {
        assert!(keys.iter().any(|have| have == key), "{key} is shell artillery");
    }
    keys
}

/// A tank that does not shoot back, with health enough to take a shell, standing still.
fn standing_tank(world: &mut SimWorld, at: Vec2Fp) -> SimId {
    let tank = passing_tank(world, at, at);
    world.advance_tick(&[SimCommand::Stop { units: vec![tank] }]);
    tank
}

/// All artillery lobs its shells (GAME_SPECIFICATION.md §7, Artillery): each climbs well above
/// the line to its target, passes over a unit standing halfway, and comes down on the target,
/// at its least reach, halfway out, and at its full reach.
#[test]
fn artillery_lobs_its_shells_over_what_is_between_onto_its_target() {
    for key in shell_artillery(&world(5_120)) {
        let probe = world(5_120);
        let def = probe.blueprints.get(probe.blueprints.id(&key)).clone();
        let weapon = def.weapon.expect("artillery has a gun");
        let (least, reach) = (weapon.min_range.to_num::<i32>(), weapon.range.to_num::<i32>());
        for distance in [least + 60, (least + reach) / 2, reach * 9 / 10] {
            let map = ((distance * 2 + 2_000).max(5_120) + 1_279) / 1_280 * 1_280;
            let mut world = world(map);
            let y = map / 2;
            let gun = if def.is_structure() {
                structure(&mut world, &key, Team::Blue, 25, (y / 20) as u32)
            } else {
                world.spawn_mobile(world.blueprints.id(&key), Team::Blue, point(500, y))
            };
            let from = world.unit_position(gun).unwrap();
            // No spread, so that the shell comes down on the target itself.
            if let Ok(mut artillery) = world.ecs.get::<&mut Artillery>(world.unit_entity(gun).unwrap()) {
                artillery.spread = Fp::ZERO;
            }
            let x = from.x.to_num::<i32>() + distance;
            let target = standing_tank(&mut world, point(x, y));
            let between = standing_tank(&mut world, point(from.x.to_num::<i32>() + distance / 2, y));
            let Some(_) = first_shot(&mut world, gun, 400) else {
                panic!("{key} fires at {distance}");
            };
            let mut peak = Fp::from_num(-1_000);
            for _ in 0..600 {
                let flying: Vec<Vec3Fp> = world.projectiles.iter().filter_map(|&(_, entity)| {
                    let shell = world.ecs.get::<&Projectile>(entity).ok().filter(|shell| shell.source == gun)?;
                    let _ = shell;
                    world.ecs.get::<&sim_engine::sim::components::Position>(entity).ok().map(|at| at.0)
                }).collect();
                for at in &flying {
                    peak = peak.max(at.z - from.z);
                }
                world.advance_tick(&[]);
                if flying.is_empty() && world.unit_health(target).is_some_and(|health| health < fp_int(100_000)) {
                    break;
                }
            }
            let rise = (distance as f64 * 0.4).min(3_000.0);
            assert!(peak.to_num::<f64>() >= rise * 0.8, "{key} at {distance}: the shell rose {peak}, for a high arc of about {rise:.0}");
            assert!(world.unit_health(target).is_some_and(|health| health < fp_int(100_000)), "{key} at {distance}: the shell came down on its target");
            assert_eq!(world.unit_health(between), Some(fp_int(100_000)), "{key} at {distance}: the shell passed over the unit between");
        }
    }
}

/// A shell comes down where it was aimed whether its target is still there or not: with the
/// target gone from under it, it bursts on the ground there, and hits what stands there. A flat
/// shell flew on past.
#[test]
fn a_shell_lands_where_it_was_aimed_with_its_target_gone() {
    for key in shell_artillery(&world(5_120)) {
        let probe = world(5_120);
        let def = probe.blueprints.get(probe.blueprints.id(&key)).clone();
        let weapon = def.weapon.expect("artillery has a gun");
        if weapon.splash_radius <= Fp::ZERO {
            continue;
        }
        let distance = (weapon.min_range.to_num::<i32>() + weapon.range.to_num::<i32>()) / 2;
        let map = ((distance * 2 + 2_000).max(5_120) + 1_279) / 1_280 * 1_280;
        let mut world = world(map);
        let y = map / 2;
        let gun = if def.is_structure() { structure(&mut world, &key, Team::Blue, 25, (y / 20) as u32) } else { world.spawn_mobile(world.blueprints.id(&key), Team::Blue, point(500, y)) };
        if let Ok(mut artillery) = world.ecs.get::<&mut Artillery>(world.unit_entity(gun).unwrap()) {
            artillery.spread = Fp::ZERO;
        }
        let x = world.unit_position(gun).unwrap().x.to_num::<i32>() + distance;
        let target = standing_tank(&mut world, point(x, y));
        first_shot(&mut world, gun, 400).unwrap_or_else(|| panic!("{key} fires"));
        // With the shell in the air, the target is gone from under it, and another stands beside
        // where it was, off the line of flight, within the blast of a shell that bursts there.
        let entity = world.unit_entity(target).unwrap();
        world.ecs.get::<&mut sim_engine::sim::components::Position>(entity).unwrap().0 = Vec3Fp::new(fp_int(x), fp_int(y + 1_000), fp_int(20));
        let bystander = standing_tank(&mut world, point(x, y + weapon.splash_radius.to_num::<i32>()));
        for _ in 0..400 {
            world.advance_tick(&[]);
            if shells(&world, gun).is_empty() {
                break;
            }
        }
        assert!(world.unit_health(bystander).is_some_and(|health| health < fp_int(100_000)), "{key}: the shell burst where it was aimed");
        assert_eq!(world.unit_health(target), Some(fp_int(100_000)), "{key}: and not on the target, gone from there");
    }
}
