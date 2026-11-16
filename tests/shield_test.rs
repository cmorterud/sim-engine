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
    // Healing at 50 a second nearly keeps up with one heavy tank, so it is off here, to count hits.
    world.ecs.get::<&mut Shield>(world.unit_entity(generator).unwrap()).unwrap().regen_per_tick = Fp::ZERO;
    run(&mut world, 120);
    assert_eq!(world.unit_health(sheltered), Some(full), "nothing should get through the dome");
    let now = shield(&world, generator);
    assert!(now.is_up());
    // A shell every 12 ticks, each doing all of its 70 damage to the dome.
    let lost = (now.max_health - now.health).to_num::<i32>();
    assert!(lost % 70 == 0 && lost >= 7 * 70, "the dome took {lost}");
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
    // FAF's generator has only 250 health; this test is about its dome, not the generator.
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
    // A shield is FAF's 3 by 3 size, which a T1 generator takes 2.08% off, five times over.
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
