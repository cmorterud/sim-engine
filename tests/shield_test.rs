//! Shield domes (GAME_SPECIFICATION.md §8.1): what they stop, what they let through, and what
//! keeps them up.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp, Vec3Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Combat, Projectile, Shield, SimId, Team, Turret};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::{ShieldView, SimWorld};

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// A flat map with a finished Red shield generator whose dome is centered on (1,020, 1,020),
/// and enough stored energy to keep it up.
fn shielded_world() -> (SimWorld, SimId) {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let blueprint = world.blueprints.id("shield_generator");
    let generator = world.spawn_finished_structure(blueprint, Team::Red, 50, 50).expect("room for it");
    for economy in &mut world.economies {
        economy.base_energy_capacity = Res::from_num(20_000);
        economy.energy_capacity = Res::from_num(20_000);
        economy.energy = Res::from_num(20_000);
    }
    (world, generator)
}

fn spawn(world: &mut SimWorld, kind: &str, team: Team, at: Vec2Fp, armed: bool) -> SimId {
    let blueprint = world.blueprints.id(kind);
    let id = world.spawn_mobile(blueprint, team, at);
    let entity = world.unit_entity(id).unwrap();
    if !armed {
        world.ecs.get::<&mut Combat>(entity).unwrap().range = Fp::ZERO;
    }
    // Already aimed at the dome, so that the tests do not wait for turrets.
    let toward = (point(1_020, 1_020) - at).normalized();
    if let Ok(mut turret) = world.ecs.get::<&mut Turret>(entity) {
        turret.facing = toward;
    }
    id
}

fn shield(world: &SimWorld, generator: SimId) -> Shield {
    *world.ecs.get::<&Shield>(world.unit_entity(generator).unwrap()).unwrap()
}

fn shield_view(world: &SimWorld, generator: SimId) -> ShieldView {
    *world.snapshot_for(Some(Team::Red)).shields.iter().find(|view| view.id == generator).unwrap()
}

fn run(world: &mut SimWorld, ticks: u32) {
    for _ in 0..ticks {
        world.advance_tick(&[]);
    }
}

#[test]
fn a_dome_stops_enemy_shells_from_outside_and_takes_their_damage() {
    let (mut world, generator) = shielded_world();
    let sheltered = spawn(&mut world, "t1_light_walker", Team::Red, point(1_020, 1_080), false);
    let full = world.unit_health(sheltered).unwrap();
    // 400 from its target, and well outside the 180-unit dome.
    spawn(&mut world, "t2_heavy_tank", Team::Blue, point(1_020, 1_480), true);
    // Healing would hide hits, so it is off here, to count them.
    world.ecs.get::<&mut Shield>(world.unit_entity(generator).unwrap()).unwrap().regen_per_tick = Fp::ZERO;
    run(&mut world, 120);
    assert_eq!(world.unit_health(sheltered), Some(full), "nothing should get through the dome");
    let now = shield(&world, generator);
    assert!(now.is_up());
    // A shell every reload, each doing all of its damage to the dome.
    let shell = world.blueprints.get(world.blueprints.id("t2_heavy_tank")).weapon.expect("the tank has a gun").damage;
    let lost = now.max_health - now.health;
    let shells = (lost / shell).round();
    assert!(shells >= fp_int(7) && (lost - shells * shell).abs() < Fp::from_num(0.01), "the dome took {lost:?}, not whole shells of {shell:?}");
}

#[test]
fn shells_from_inside_fly_out_and_friendly_shells_fly_in() {
    let (mut world, generator) = shielded_world();
    // A Red tank under its own dome, and a Blue unit outside it.
    spawn(&mut world, "t2_heavy_tank", Team::Red, point(1_020, 1_060), true);
    let outside = spawn(&mut world, "t1_light_walker", Team::Blue, point(1_020, 1_420), false);
    // A Blue unit that has walked in under the Red dome, and a Red tank outside shelling it.
    let intruder = spawn(&mut world, "t1_light_walker", Team::Blue, point(900, 1_020), false);
    spawn(&mut world, "t2_heavy_tank", Team::Red, point(500, 1_020), true);
    let (outside_full, intruder_full) = (world.unit_health(outside).unwrap(), world.unit_health(intruder).unwrap());
    run(&mut world, 80);
    assert!(world.unit_health(outside).is_none_or(|health| health < outside_full), "a shell from inside should get out");
    assert!(world.unit_health(intruder).is_none_or(|health| health < intruder_full), "a Red dome should not stop Red shells");
    assert_eq!(shield(&world, generator).health, fp_int(5_000), "a dome is never hit by its own side");
}

#[test]
fn a_dome_that_runs_out_collapses_and_comes_back_at_full_strength() {
    let (mut world, generator) = shielded_world();
    let sheltered = spawn(&mut world, "t1_light_walker", Team::Red, point(1_020, 1_080), false);
    let full = world.unit_health(sheltered).unwrap();
    world.ecs.get::<&mut Shield>(world.unit_entity(generator).unwrap()).unwrap().health = fp_int(10);
    // The inspiration's generator has only 250 health; this test is about its dome, not the
    // generator.
    {
        let mut health = world.ecs.get::<&mut sim_engine::sim::components::Health>(world.unit_entity(generator).unwrap()).unwrap();
        health.max = fp_int(100_000);
        health.current = health.max;
    }
    let tank = spawn(&mut world, "t2_heavy_tank", Team::Blue, point(1_020, 1_480), true);
    let mut collapsed_at = None;
    for tick in 0..80 {
        world.advance_tick(&[]);
        if !shield(&world, generator).is_up() {
            collapsed_at = Some(tick);
            break;
        }
    }
    assert!(collapsed_at.is_some(), "one shell should knock it down");
    assert!(!shield_view(&world, generator).up);
    run(&mut world, 60);
    assert!(world.unit_health(sheltered).is_none_or(|health| health < full), "with the dome down, shells get through");

    // Nobody shoots it while it recharges, and it is back after 30 seconds.
    world.ecs.get::<&mut Combat>(world.unit_entity(tank).unwrap()).unwrap().range = Fp::ZERO;
    run(&mut world, 300 - 60 - 2);
    assert!(!shield(&world, generator).is_up(), "it should still be recharging");
    run(&mut world, 4);
    let back = shield(&world, generator);
    assert!(back.is_up());
    assert_eq!(back.health, back.max_health);
}

/// A shell dropped straight down onto the overlap of two Red domes, 160 apart.
fn drop_on_overlap(splash: i32) -> (Fp, Fp) {
    let (mut world, first) = shielded_world();
    let blueprint = world.blueprints.id("shield_generator");
    let second = world.spawn_finished_structure(blueprint, Team::Red, 58, 50).expect("room for it");
    let source = spawn(&mut world, "t1_light_walker", Team::Blue, point(3_000, 3_000), false);
    world.spawn_projectile(
        Vec3Fp::new(fp_int(1_100), fp_int(1_020), fp_int(600)),
        Projectile {
            velocity: Vec3Fp::new(Fp::ZERO, Fp::ZERO, fp_int(-30)),
            damage: fp_int(100),
            source,
            source_team: Team::Blue,
            lifetime_ticks: 50,
            splash_radius: fp_int(splash),
            allied_safe_ticks: 0,
        },
    );
    // Both domes are full, so neither heals; stop before either could.
    for _ in 0..30 {
        world.advance_tick(&[]);
        if world.projectiles.is_empty() {
            break;
        }
    }
    assert!(world.projectiles.is_empty(), "the shell should have hit a dome");
    let lost = |id: SimId| shield(&world, id).max_health - shield(&world, id).health;
    (lost(first), lost(second))
}

#[test]
fn splash_bleeds_into_every_overlapping_dome_but_a_plain_shell_hits_one() {
    let (first, second) = drop_on_overlap(40);
    assert_eq!((first, second), (fp_int(100), fp_int(100)), "a blast where two domes overlap hits both");
    let (first, second) = drop_on_overlap(0);
    assert_eq!(first + second, fp_int(100), "a plain shell hits only the dome it strikes");
}

#[test]
fn an_energy_stall_collapses_the_dome_and_pausing_drops_it_without_cost() {
    let (mut world, generator) = shielded_world();
    world.economies[1].energy = Res::ZERO;
    run(&mut world, 2);
    assert!(!shield(&world, generator).is_up(), "with nothing to pay its upkeep, it collapses");

    let (mut world, generator) = shielded_world();
    world.ecs.get::<&mut Shield>(world.unit_entity(generator).unwrap()).unwrap().health = fp_int(3_000);
    world.advance_tick(&[SimCommand::SetPaused { units: vec![generator], paused: true }]);
    let stored = world.economies[1].energy;
    run(&mut world, 50);
    assert!(!shield_view(&world, generator).up, "a paused dome is down");
    assert_eq!(world.economies[1].energy, stored, "and costs nothing");
    let health = shield(&world, generator).health;
    world.advance_tick(&[SimCommand::SetPaused { units: vec![generator], paused: false }]);
    let view = shield_view(&world, generator);
    assert!(view.up, "resuming puts it straight back up");
    assert!(shield(&world, generator).health <= health + fp_int(5), "with the health it had");
    // 30 energy a second is 3 a tick.
    let before = world.economies[1].energy;
    world.advance_tick(&[]);
    assert_eq!(before - world.economies[1].energy, Res::from_num(3));
}

#[test]
fn power_generators_beside_a_shield_cut_its_upkeep() {
    let (mut world, generator) = shielded_world();
    let upkeep = |world: &SimWorld| world.snapshot_for(Some(Team::Red)).rates.iter().find(|rates| rates.id == generator).unwrap().energy_use;
    world.advance_tick(&[]);
    assert_eq!(upkeep(&world), 300, "30.0 energy a second alone");
    let power = world.blueprints.id("power_generator");
    // Around a shield at (50, 50), which covers 50 to 52 either way.
    for (x, y) in [(49, 50), (53, 50), (50, 49), (50, 53), (51, 53)] {
        world.spawn_finished_structure(power, Team::Red, x, y).expect("room for it");
    }
    world.advance_tick(&[]);
    // A shield is the inspiration's 3 by 3 size, which a T1 generator takes 2.08% off, five times
    // over.
    assert!((268..=269).contains(&upkeep(&world)), "five neighbors: {}", upkeep(&world));
    // A T2 generator takes an eighth.
    let (mut world, generator) = shielded_world();
    world.spawn_finished_structure(world.blueprints.id("power_generator_t2"), Team::Red, 47, 50).expect("room for it");
    world.advance_tick(&[]);
    let upkeep = |world: &SimWorld| world.snapshot_for(Some(Team::Red)).rates.iter().find(|rates| rates.id == generator).unwrap().energy_use;
    assert!((262..=263).contains(&upkeep(&world)), "26.25 a second: {}", upkeep(&world));
}

#[test]
fn a_dome_is_not_up_until_its_generator_is_built() {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    let blueprint = world.blueprints.id("shield_generator");
    let site = world.spawn_site(blueprint, Team::Red, 50, 50).unwrap();
    world.advance_tick(&[]);
    let view = *world.snapshot_for(Some(Team::Red)).shields.iter().find(|view| view.id == site).unwrap();
    assert!(!view.up);
    assert_eq!(view.recharging, 0);
}

/// Flat land with a lake in the middle third of the map, for ships, and plenty of everything.
fn open_world() -> SimWorld {
    use sim_engine::sim::terrain::{Terrain, HEIGHT_FRAC_BITS};
    let map = 10_240;
    let cells = (map / 20) as usize;
    let samples = cells + 1;
    let height = |x: usize, _y: usize| if (3_000..=7_000).contains(&(x as i32 * 20)) { -40 } else { 20 };
    let heights: Vec<i16> = (0..samples * samples).map(|index| (height(index % samples, index / samples) << HEIGHT_FRAC_BITS) as i16).collect();
    let blocked: Vec<bool> = (0..cells * cells)
        .map(|cell| {
            let (x, y) = (cell % cells, cell / cells);
            [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].iter().any(|&(cx, cy)| height(cx, cy) < 0)
        })
        .collect();
    let terrain = Terrain::from_heights(map, heights, &blocked, Fp::ZERO);
    let config = SimConfig { map_units: map, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::with_terrain(config, 1, terrain);
    world.prepare_map();
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(10_000_000);
        }
        economy.mass = Res::from_num(5_000_000);
        economy.energy = Res::from_num(5_000_000);
    }
    world
}

/// Every shield in the unit data, on a generator, a unit, or from an enhancement, does what its
/// numbers say: shells from outside hurt only the dome while it is up, and take exactly their
/// damage from it; it regenerates at its rate; it collapses when it runs out, after which what it
/// covered is hit; and it comes back at full strength after its recharge time.
#[test]
fn every_shield_absorbs_regenerates_collapses_and_recharges() {
    use sim_engine::sim::components::{Health, MoveClass, UnitStats};
    let db = open_world().blueprints;
    // The unit carrying it, and the enhancement that gives it, if any.
    let mut cases: Vec<(String, Option<String>)> = db.iter().filter(|def| def.shield.is_some()).map(|def| (def.key.clone(), None)).collect();
    for def in db.iter() {
        for enhancement in db.enhancements_for(db.id(&def.key)).filter(|enhancement| enhancement.effects.shield.is_some()) {
            cases.push((def.key.clone(), Some(enhancement.key.clone())));
        }
    }
    assert!(cases.len() >= 8, "only {} shields found", cases.len());
    let mut failures = Vec::new();
    for (key, enhancement) in cases {
        let name = format!("{key}{}", enhancement.as_ref().map_or(String::new(), |enhancement| format!(" with {enhancement}")));
        let mut world = open_world();
        let blueprint = world.blueprints.id(&key);
        let def = world.blueprints.get(blueprint).clone();
        let at_sea = matches!(def.movement, MoveClass::Naval | MoveClass::Submarine);
        let at = if at_sea { point(5_000, 5_000) } else { point(1_500, 5_000) };
        let carrier = if def.is_structure() {
            world.spawn_finished_structure(blueprint, Team::Blue, 75, 250).unwrap()
        } else {
            world.spawn_mobile(blueprint, Team::Blue, at)
        };
        if let Some(enhancement) = &enhancement {
            let id = world.blueprints.enhancement_id(enhancement);
            world.install_enhancement(carrier, id);
        }
        let entity = world.unit_entity(carrier).unwrap();
        world.ecs.get::<&mut Combat>(entity).unwrap().enabled = false;
        {
            let mut health = world.ecs.get::<&mut Health>(entity).unwrap();
            health.max = fp_int(400_000);
            health.current = health.max;
        }
        let Ok(dome) = world.ecs.get::<&Shield>(entity).map(|shield| *shield) else {
            failures.push(format!("{name}: no shield"));
            continue;
        };
        let here = world.unit_position(carrier).unwrap().xy();
        let carrier_radius = world.ecs.get::<&UnitStats>(entity).unwrap().radius;

        // Regeneration, from half strength, with nothing shooting.
        world.ecs.get::<&mut Shield>(entity).unwrap().health = dome.max_health / 2;
        for _ in 0..20 {
            world.advance_tick(&[]);
        }
        let regained = shield(&world, carrier).health - dome.max_health / 2;
        let expected = (dome.regen_per_tick * 20).min(dome.max_health / 2);
        if (regained - expected).abs() > dome.regen_per_tick * 2 {
            failures.push(format!("{name}: regained {regained} in 2 s, expected {expected}"));
        }
        world.ecs.get::<&mut Shield>(entity).unwrap().health = dome.max_health;

        // A gun outside the dome, firing at what it covers, enough a shot to wear it down.
        let reload = 10;
        let damage = (dome.max_health / 8).max(dome.regen_per_tick * reload * 4);
        // On the shore, for a ship.
        let distance = if at_sea { here.x - fp_int(2_800) } else { dome.radius + carrier_radius + fp_int(150) };
        let mut gun = sim_engine::sim::scenario::standard_unit(Team::Red, Vec2Fp::new(here.x - distance, here.y));
        gun.range = distance + fp_int(60);
        gun.reload_ticks = reload as u32;
        gun.projectile_speed = fp_int(40);
        gun.projectile_damage = damage;
        gun.health = fp_int(400_000);
        let gun = world.spawn_unit(gun);
        // An aircraft's dome covers the ground beneath it: the gun fires at a unit there instead.
        let covered = if def.is_air() {
            // Idle, it glides on as it lands: once down, the unit goes beside it, on the gun's side,
            // so that the transport is not in the way of the shells.
            for _ in 0..200 {
                world.advance_tick(&[]);
            }
            let landed = world.unit_position(carrier).unwrap().xy();
            let under = world.spawn_mobile(world.blueprints.id("t1_engineer"), Team::Blue, landed - Vec2Fp::new(carrier_radius + fp_int(12), Fp::ZERO));
            let gun_entity = world.unit_entity(gun).unwrap();
            world.ecs.get::<&mut sim_engine::sim::components::Position>(gun_entity).unwrap().0.x = landed.x - distance;
            world.ecs.get::<&mut sim_engine::sim::components::Position>(gun_entity).unwrap().0.y = landed.y;
            let mut health = world.ecs.get::<&mut Health>(world.unit_entity(under).unwrap()).unwrap();
            health.max = fp_int(400_000);
            health.current = health.max;
            under
        } else {
            carrier
        };
        world.advance_tick(&[SimCommand::Attack { units: vec![gun], target: covered }]);
        let full = fp_int(400_000);
        let mut collapsed_at = None;
        for tick in 0..2_000 {
            world.advance_tick(&[]);
            let now = shield(&world, carrier);
            if now.is_up() {
                if world.unit_health(covered).unwrap() < full {
                    failures.push(format!("{name}: hurt under a dome still up, with {} left", now.health));
                    break;
                }
            } else {
                collapsed_at = Some(tick);
                break;
            }
        }
        let Some(_) = collapsed_at else {
            failures.push(format!("{name}: never collapsed under {damage} a shot"));
            continue;
        };
        // Down, what it covered is hit.
        for _ in 0..60 {
            world.advance_tick(&[]);
        }
        if world.unit_health(covered).unwrap() >= full {
            failures.push(format!("{name}: not hurt with its dome down"));
        }
        // The gun stops, and the dome comes back, at full strength, after its recharge time.
        world.ecs.get::<&mut Combat>(world.unit_entity(gun).unwrap()).unwrap().enabled = false;
        let back = (0..dome.recharge_ticks + 200).find(|_| {
            world.advance_tick(&[]);
            shield(&world, carrier).is_up()
        });
        match back {
            None => failures.push(format!("{name}: never came back")),
            Some(ticks) => {
                // 60 of its recharge ticks went by while it was being hit.
                let waited = ticks + 1 + 60;
                if waited.abs_diff(dome.recharge_ticks) > 2 {
                    failures.push(format!("{name}: back after {waited} ticks, expected {}", dome.recharge_ticks));
                }
                let strength = shield(&world, carrier).health;
                if strength < dome.max_health - dome.regen_per_tick * 2 {
                    failures.push(format!("{name}: back at {strength}, not full {}", dome.max_health));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
