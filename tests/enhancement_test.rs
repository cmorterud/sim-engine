//! The commander's field enhancements, teleporting, and self-destruct (GAME_SPECIFICATION.md §2.1).

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Builder, Combat, Enhancements, Enhancing, Intel, Position, SimId, Team, Teleporting};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::enhancements::teleport_cost;
use sim_engine::sim::world::{MatchResult, SimWorld};

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn world_with(fog_of_war: bool) -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.mass] {
            *value = Res::from_num(100_000);
        }
        for value in [&mut economy.base_energy_capacity, &mut economy.energy_capacity, &mut economy.energy] {
            *value = Res::from_num(5_000_000);
        }
    }
    world
}

fn commander(world: &mut SimWorld, team: Team, at: Vec2Fp) -> SimId {
    let rig = world.blueprints.id("command_rig");
    world.spawn_mobile(rig, team, at)
}

fn install(world: &mut SimWorld, id: SimId, key: &str) {
    let enhancement = world.blueprints.enhancement_id(key);
    world.install_enhancement(id, enhancement);
}

fn enhance(world: &mut SimWorld, id: SimId, key: &str) {
    let enhancement = world.blueprints.enhancement_id(key);
    world.advance_tick(&[SimCommand::Enhance { units: vec![id], enhancement }]);
}

fn slots(world: &SimWorld, id: SimId) -> [Option<String>; 3] {
    let installed = world.ecs.get::<&Enhancements>(world.unit_entity(id).unwrap()).map_or([None; 3], |installed| installed.0);
    installed.map(|slot| slot.map(|enhancement| world.blueprints.enhancement(enhancement).key.clone()))
}

fn run(world: &mut SimWorld, ticks: u32) {
    for _ in 0..ticks {
        world.advance_tick(&[]);
    }
}

fn position(world: &SimWorld, id: SimId) -> Vec2Fp {
    world.unit_position(id).unwrap().xy()
}

#[test]
fn the_heavy_cannon_gives_the_gun_more_range_and_damage() {
    let mut world = world_with(false);
    let rig = commander(&mut world, Team::Blue, point(1_000, 1_000));
    install(&mut world, rig, "heavy_cannon");
    let combat = *world.ecs.get::<&Combat>(world.unit_entity(rig).unwrap()).unwrap();
    assert_eq!((combat.range, combat.projectile_damage), (fp_int(600), fp_int(140)));
    assert_eq!(slots(&world, rig)[0].as_deref(), Some("heavy_cannon"));
}

#[test]
fn a_new_enhancement_in_a_full_slot_removes_the_old_one_at_once() {
    let mut world = world_with(false);
    let rig = commander(&mut world, Team::Blue, point(1_000, 1_000));
    install(&mut world, rig, "signature_suppression");
    let entity = world.unit_entity(rig).unwrap();
    assert_eq!(world.unit_health(rig), Some(fp_int(14_000)));

    // Damage control is a systems enhancement too: signature suppression comes off when it starts.
    enhance(&mut world, rig, "damage_control");
    assert_eq!(slots(&world, rig)[2], None, "the old one is gone");
    assert_eq!(world.unit_health(rig), Some(fp_int(12_000)), "and what it gave");
    // 800 build time at 10 build power.
    run(&mut world, 805);
    assert_eq!(slots(&world, rig)[2].as_deref(), Some("damage_control"));
    assert_eq!(world.unit_health(rig), Some(fp_int(13_500)), "its 1,500 more health comes with it");

    // It heals 40 a second.
    world.ecs.get::<&mut sim_engine::sim::components::Health>(entity).unwrap().current = fp_int(5_000);
    run(&mut world, 10);
    assert_eq!(world.unit_health(rig), Some(fp_int(5_040)), "40 over the second");
}

#[test]
fn an_enhancement_that_builds_on_another_needs_it_first() {
    let mut world = world_with(false);
    let rig = commander(&mut world, Team::Blue, point(1_000, 1_000));
    let entity = world.unit_entity(rig).unwrap();
    enhance(&mut world, rig, "t3_fabrication");
    assert!(world.ecs.get::<&Enhancing>(entity).is_err(), "it needs the T2 suite");
    enhance(&mut world, rig, "thermal_camouflage");
    assert!(world.ecs.get::<&Enhancing>(entity).is_err(), "and the cloak needs stealth");

    // The suite it builds on stays while the next one is built, so it builds with 42.
    install(&mut world, rig, "t2_fabrication");
    enhance(&mut world, rig, "t3_fabrication");
    assert_eq!(slots(&world, rig)[1].as_deref(), Some("t2_fabrication"));
    // 8,400 build time at 42 build power is 200 seconds.
    run(&mut world, 2_005);
    assert_eq!(slots(&world, rig)[1].as_deref(), Some("t3_fabrication"));
    assert_eq!(world.ecs.get::<&Builder>(entity).unwrap().power, fp_int(100));
    assert_eq!(world.unit_health(rig), Some(fp_int(15_000)), "3,000 over its own, not on top of the T2 suite's");

    // The T3 suite includes the T2 one, so the T2 one cannot be taken again, which would be a step back.
    enhance(&mut world, rig, "t2_fabrication");
    assert!(world.ecs.get::<&Enhancing>(entity).is_err(), "it has T2 already, as part of T3");
    assert_eq!(slots(&world, rig)[1].as_deref(), Some("t3_fabrication"));
    // Likewise stealth, once it has the cloak built on it.
    install(&mut world, rig, "thermal_camouflage");
    enhance(&mut world, rig, "signature_suppression");
    assert!(world.ecs.get::<&Enhancing>(entity).is_err());
    assert_eq!(slots(&world, rig)[2].as_deref(), Some("thermal_camouflage"));
}

#[test]
fn a_field_shield_goes_where_the_commander_goes() {
    let mut world = world_with(false);
    let rig = commander(&mut world, Team::Blue, point(1_000, 1_000));
    install(&mut world, rig, "field_shield");
    run(&mut world, 2);
    let dome = |world: &SimWorld| world.snapshot_for(Some(Team::Blue)).shields.iter().find(|shield| shield.id == rig).map(|shield| (shield.center, shield.radius));
    let (center, radius) = dome(&world).expect("a dome over the commander");
    assert_eq!(radius, fp_int(30).to_bits());
    assert_eq!([center[0], center[1]], [fp_int(1_000).to_bits(), fp_int(1_000).to_bits()]);
    world.advance_tick(&[SimCommand::Move { units: vec![rig], goal: point(1_400, 1_000) }]);
    run(&mut world, 60);
    let (center, _) = dome(&world).unwrap();
    assert_eq!(center[0], position(&world, rig).x.to_bits(), "the dome moves with it");

    // Its shell is stopped by the dome, not by the commander.
    let tank = world.blueprints.id("t2_heavy_tank");
    let tank = world.spawn_mobile(tank, Team::Red, position(&world, rig) + Vec2Fp::new(fp_int(300), Fp::ZERO));
    let full = world.unit_health(rig).unwrap();
    run(&mut world, 100);
    assert_eq!(world.unit_health(rig), Some(full), "the shield should take the shells");
    let shield = *world.ecs.get::<&sim_engine::sim::components::Shield>(world.unit_entity(rig).unwrap()).unwrap();
    assert!(shield.health < shield.max_health);
    let _ = tank;

    // Swapping to another back enhancement takes the shield away.
    enhance(&mut world, rig, "signature_suppression");
    assert!(dome(&world).is_none());
}

#[test]
fn the_onboard_reactor_system_adds_income() {
    let mut world = world_with(false);
    let rig = commander(&mut world, Team::Blue, point(1_000, 1_000));
    run(&mut world, 1);
    let (mass, energy) = (world.economies[0].mass_income, world.economies[0].energy_income);
    install(&mut world, rig, "onboard_reactor");
    run(&mut world, 1);
    assert_eq!(world.economies[0].mass_income - mass, Res::from_num(14) / 10);
    assert_eq!(world.economies[0].energy_income - energy, Res::from_num(250));
}

#[test]
fn signature_suppression_keeps_the_commander_off_enemy_radar() {
    let mut world = world_with(true);
    let radar = world.blueprints.id("radar");
    world.spawn_finished_structure(radar, Team::Red, 50, 50).unwrap();
    // 1,000 from the radar: beyond its sight, within its radar.
    let rig = commander(&mut world, Team::Blue, point(2_020, 1_010));
    run(&mut world, 10);
    assert_eq!(world.snapshot_for(Some(Team::Red)).blips.len(), 1, "on radar at first");
    install(&mut world, rig, "signature_suppression");
    run(&mut world, 10);
    assert!(world.snapshot_for(Some(Team::Red)).blips.is_empty(), "hidden by its stealth");
    assert!(world.ecs.get::<&Intel>(world.unit_entity(rig).unwrap()).unwrap().stealthed);
    // Without the energy for it, it is back on radar.
    world.economies[0].energy = Res::ZERO;
    world.economies[0].energy_income = Res::ZERO;
    run(&mut world, 1);
    assert!(!world.ecs.get::<&Intel>(world.unit_entity(rig).unwrap()).unwrap().stealthed);
}

#[test]
fn a_teleport_charges_standing_still_then_lands_and_destroys_an_enemy_there() {
    let mut world = world_with(false);
    let rig = commander(&mut world, Team::Blue, point(1_000, 1_000));
    let entity = world.unit_entity(rig).unwrap();
    // Without a teleporter, nothing happens.
    world.advance_tick(&[SimCommand::Teleport { units: vec![rig], point: point(3_000, 1_000) }]);
    assert!(world.ecs.get::<&Teleporting>(entity).is_err());

    install(&mut world, rig, "teleporter");
    let bot = world.blueprints.id("t1_light_walker");
    let victim = world.spawn_mobile(bot, Team::Red, point(3_000, 1_000));
    world.ecs.get::<&mut Combat>(world.unit_entity(victim).unwrap()).unwrap().range = Fp::ZERO;
    let teleporter = world.blueprints.enhancement(world.blueprints.enhancement_id("teleporter")).effects.teleporter.unwrap();
    let (energy, ticks) = teleport_cost(&teleporter, fp_int(2_000));
    assert!((200..220).contains(&ticks), "{ticks} ticks");
    let stored = world.economies[0].energy;
    world.advance_tick(&[SimCommand::Teleport { units: vec![rig], point: point(3_000, 1_000) }]);
    let here = position(&world, rig);
    run(&mut world, ticks - 2);
    assert_eq!(position(&world, rig), here, "it stands still while it charges");
    assert!(world.ecs.get::<&Teleporting>(entity).is_ok());
    run(&mut world, 2);
    assert_eq!(position(&world, rig), point(3_000, 1_000), "then it is there");
    assert!(world.unit_entity(victim).is_none(), "and what stood there is destroyed");
    // It paid the charge, less what came in meanwhile.
    let income = world.economies[0].energy_income * i64::from(ticks);
    let paid = stored + income - world.economies[0].energy;
    assert!((paid - energy).abs() < Res::from_num(ticks), "paid {paid}, not {energy}");
}

#[test]
fn a_teleport_aborts_when_the_energy_runs_out_and_needs_a_spot_it_can_sense() {
    let mut world = world_with(true);
    let rig = commander(&mut world, Team::Blue, point(1_000, 1_000));
    let entity = world.unit_entity(rig).unwrap();
    install(&mut world, rig, "teleporter");
    run(&mut world, 2);
    // Far out of sight, with no radar there.
    world.advance_tick(&[SimCommand::Teleport { units: vec![rig], point: point(4_000, 4_000) }]);
    assert!(world.ecs.get::<&Teleporting>(entity).is_err(), "it cannot jump somewhere it cannot sense");

    // In sight, 400 away within its 520, but with too little energy to finish.
    world.advance_tick(&[SimCommand::Teleport { units: vec![rig], point: point(1_400, 1_000) }]);
    assert!(world.ecs.get::<&Teleporting>(entity).is_ok());
    world.economies[0].energy = Res::from_num(10_000);
    run(&mut world, 50);
    assert!(world.ecs.get::<&Teleporting>(entity).is_err(), "it aborts when it cannot pay");
    assert_eq!(position(&world, rig), point(1_000, 1_000), "and stays where it was");
}

#[test]
fn a_new_order_cancels_a_teleport() {
    let mut world = world_with(false);
    let rig = commander(&mut world, Team::Blue, point(1_000, 1_000));
    install(&mut world, rig, "teleporter");
    world.advance_tick(&[SimCommand::Teleport { units: vec![rig], point: point(3_000, 1_000) }]);
    world.advance_tick(&[SimCommand::Move { units: vec![rig], goal: point(1_200, 1_000) }]);
    assert!(world.ecs.get::<&Teleporting>(world.unit_entity(rig).unwrap()).is_err());
    assert!(world.snapshot_for(Some(Team::Blue)).teleports.is_empty());
}

#[test]
fn self_destruct_counts_down_five_seconds_and_leaves_no_debris() {
    let mut world = world_with(false);
    let bot = world.blueprints.id("t2_heavy_tank");
    let tank = world.spawn_mobile(bot, Team::Blue, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::SelfDestruct { units: vec![tank] }]);
    assert_eq!(world.snapshot_for(Some(Team::Blue)).self_destructs, [(tank, 49)]);
    // Pressing again stops it.
    run(&mut world, 20);
    world.advance_tick(&[SimCommand::SelfDestruct { units: vec![tank] }]);
    run(&mut world, 60);
    assert!(world.unit_entity(tank).is_some(), "cancelled, it lives");

    world.advance_tick(&[SimCommand::SelfDestruct { units: vec![tank] }]);
    run(&mut world, 48);
    assert!(world.unit_entity(tank).is_some(), "still counting");
    run(&mut world, 1);
    assert!(world.unit_entity(tank).is_none(), "five seconds after the order, it is gone");
    assert!(world.wrecks.is_empty(), "with nothing to reclaim");
}

#[test]
fn a_commander_that_blows_itself_up_loses_the_match_and_takes_its_surroundings_with_it() {
    let mut world = world_with(false);
    let blue = commander(&mut world, Team::Blue, point(1_000, 1_000));
    let red = commander(&mut world, Team::Red, point(3_000, 3_000));
    let bot = world.blueprints.id("t1_light_walker");
    let neighbor = world.spawn_mobile(bot, Team::Red, point(1_150, 1_000));
    world.ecs.get::<&mut Combat>(world.unit_entity(neighbor).unwrap()).unwrap().range = Fp::ZERO;
    world.advance_tick(&[SimCommand::SelfDestruct { units: vec![blue] }]);
    run(&mut world, 50);
    assert_eq!(world.match_result, Some(MatchResult::Winner(Team::Red)));
    assert!(world.unit_entity(neighbor).is_none(), "the reactor's blast reaches 300");
    let _ = (red, world.ecs.get::<&Position>(world.unit_entity(red).unwrap()).is_ok());
}
