//! Field rigs and the Rig Yard (GAME_SPECIFICATION.md §2.2).

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Builder, Combat, Factory, Intel, SimId, Team, Teleporting, UnitStats};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::SimWorld;

mod common;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

fn world_with(fog_of_war: bool) -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    // The Radar Jammer and Orbital Redeploy are out of the roster; their mechanics are tested here.
    common::retire_into(&mut world);
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

fn spawn(world: &mut SimWorld, key: &str, team: Team, at: Vec2Fp) -> SimId {
    let blueprint = world.blueprints.id(key);
    world.spawn_mobile(blueprint, team, at)
}

fn run(world: &mut SimWorld, ticks: u32) {
    for _ in 0..ticks {
        world.advance_tick(&[]);
    }
}

fn key_of(world: &SimWorld, id: SimId) -> String {
    let blueprint = world.ecs.get::<&sim_engine::sim::components::Blueprinted>(world.unit_entity(id).unwrap()).unwrap().0;
    world.blueprints.get(blueprint).key.clone()
}

fn install(world: &mut SimWorld, id: SimId, key: &str) {
    let enhancement = world.blueprints.enhancement_id(key);
    world.install_enhancement(id, enhancement);
}

#[test]
fn only_t3_builders_can_build_a_gateway() {
    let mut world = world_with(false);
    let gateway = world.blueprints.id("rig_yard");
    let (t2, t3) = (spawn(&mut world, "t2_engineer", Team::Blue, point(1_000, 1_000)), spawn(&mut world, "t3_engineer", Team::Blue, point(1_000, 1_040)));
    let commander = spawn(&mut world, "command_rig", Team::Blue, point(1_000, 1_080));
    world.advance_tick(&[SimCommand::Build { units: vec![t2, t3, commander], blueprint: gateway, cell_x: 60, cell_y: 50, queued: false }]);
    // Building it, as opposed to helping one that can (builder_orders_test.rs).
    let has_task = |world: &SimWorld, id: SimId| {
        world.ecs.get::<&Builder>(world.unit_entity(id).unwrap()).unwrap().tasks.first().is_some_and(|task| matches!(task, sim_engine::sim::components::BuildTask::Build { .. }))
    };
    assert!(!has_task(&world, t2) && has_task(&world, t3) && !has_task(&world, commander));
    install(&mut world, commander, "t2_fabrication");
    install(&mut world, commander, "t3_fabrication");
    world.advance_tick(&[SimCommand::Build { units: vec![commander], blueprint: gateway, cell_x: 70, cell_y: 50, queued: false }]);
    assert!(has_task(&world, commander), "the commander can, with its T3 suite");
}

#[test]
fn a_gateway_builds_a_field_rig_with_heavy_armor_and_build_power() {
    let mut world = world_with(false);
    let gateway = world.spawn_finished_structure(world.blueprints.id("rig_yard"), Team::Blue, 50, 50).unwrap();
    let support = world.blueprints.id("field_rig");
    world.advance_tick(&[SimCommand::QueueUnit { factory: gateway, blueprint: support, count: 1 }]);
    // Its build time at the gateway's build power: 11,033 at 120 is about 92 seconds.
    let power = world.blueprints.get(world.blueprints.id("rig_yard")).build_power.to_num::<f64>();
    let ticks = (world.blueprints.get(support).build_time.to_num::<f64>() / power * 10.0).ceil() as u32;
    run(&mut world, ticks - 10);
    let made = |world: &SimWorld| world.units.iter().find(|&&(id, _)| key_of(world, id) == "field_rig").map(|unit| unit.0);
    assert!(made(&world).is_none(), "not before {ticks} ticks");
    run(&mut world, 20);
    let unit = made(&world).expect("a field rig after its build time");
    let entity = world.unit_entity(unit).unwrap();
    assert_eq!(world.unit_health(unit), Some(world.blueprints.get(world.blueprints.id("field_rig")).health), "at its full health");
    assert_eq!(world.ecs.get::<&Builder>(entity).unwrap().power, fp_int(56));
    assert_eq!(world.commanders[0], None, "it is not the commander");
    let _ = world.ecs.get::<&Factory>(world.unit_entity(gateway).unwrap()).unwrap();
}

#[test]
fn a_field_rig_builds_t3_structures_and_earns_its_keep() {
    let mut world = world_with(false);
    run(&mut world, 1);
    let (mass, energy) = (world.economies[0].mass_income, world.economies[0].energy_income);
    let storage = world.economies[0].energy_capacity;
    let support = spawn(&mut world, "field_rig", Team::Blue, point(1_000, 1_000));
    run(&mut world, 1);
    assert_eq!(world.economies[0].mass_income - mass, Res::from_num(1) / 10);
    assert_eq!(world.economies[0].energy_income - energy, Res::from_num(2));
    assert_eq!(world.economies[0].energy_capacity - storage, Res::from_num(500));
    let silo = world.blueprints.id("strategic_missile_launcher");
    world.advance_tick(&[SimCommand::Build { units: vec![support], blueprint: silo, cell_x: 60, cell_y: 50, queued: false }]);
    assert!(!world.ecs.get::<&Builder>(world.unit_entity(support).unwrap()).unwrap().tasks.is_empty());

    // Destroyed, it takes them with it.
    world.ecs.get::<&mut sim_engine::sim::components::Health>(world.unit_entity(support).unwrap()).unwrap().current = Fp::from_bits(1);
    world.ecs.get::<&mut Combat>(world.unit_entity(support).unwrap()).unwrap().range = Fp::ZERO;
    let tank = spawn(&mut world, "t2_heavy_tank", Team::Red, point(1_200, 1_000));
    // Told to, since left to itself it may pick the silo's site, which is right beside it.
    world.advance_tick(&[SimCommand::Attack { units: vec![tank], target: support }]);
    run(&mut world, 100);
    assert!(world.unit_entity(support).is_none());
    assert_eq!(world.economies[0].mass_income, mass);
    let _ = tank;
}

#[test]
fn losing_a_field_rig_does_not_end_the_match() {
    let mut world = world_with(false);
    spawn(&mut world, "command_rig", Team::Blue, point(1_000, 1_000));
    spawn(&mut world, "command_rig", Team::Red, point(4_000, 4_000));
    let support = spawn(&mut world, "field_rig", Team::Blue, point(2_000, 2_000));
    let bot = spawn(&mut world, "t1_light_walker", Team::Blue, point(2_080, 2_000));
    world.ecs.get::<&mut Combat>(world.unit_entity(bot).unwrap()).unwrap().range = Fp::ZERO;
    world.advance_tick(&[SimCommand::SelfDestruct { units: vec![support] }]);
    run(&mut world, 50);
    assert!(world.unit_entity(support).is_none());
    assert_eq!(world.match_result, None, "only the commander's death ends the match");
    // Its blast is small: 1,000 within 100.
    assert!(world.unit_entity(bot).is_none(), "a light bot 80 away does not survive it");
}

#[test]
fn field_rig_enhancements_change_its_gun_building_and_sensors() {
    let mut world = world_with(false);
    let support = spawn(&mut world, "field_rig", Team::Blue, point(1_000, 1_000));
    let entity = world.unit_entity(support).unwrap();
    install(&mut world, support, "long_barrel");
    let combat = *world.ecs.get::<&Combat>(entity).unwrap();
    assert_eq!((combat.range, combat.projectile_splash, combat.reload_ticks), (fp_int(700), fp_int(40), 10));
    // The other weapon upgrade is in the same arm, so it replaces the first.
    install(&mut world, support, "capacitor_bank");
    let combat = *world.ecs.get::<&Combat>(entity).unwrap();
    assert_eq!((combat.range, combat.projectile_splash, combat.reload_ticks), (fp_int(500), Fp::ZERO, 5));

    install(&mut world, support, "rapid_fabricator");
    assert_eq!(world.ecs.get::<&Builder>(entity).unwrap().power, fp_int(98));
    install(&mut world, support, "sensor_mast");
    assert_eq!(world.ecs.get::<&Builder>(entity).unwrap().power, fp_int(98), "the sensor mast goes in the systems bay, beside the fabricator");
    assert_eq!(world.ecs.get::<&UnitStats>(entity).unwrap().vision, fp_int(900));
    run(&mut world, 2);
    let sensors = world.snapshot_for(Some(Team::Blue)).sensor_ranges;
    assert_eq!(sensors.iter().find(|sensor| sensor.center[0] == fp_int(1_000).to_bits()).map(|sensor| sensor.deep_scan), Some(fp_int(1_600).to_bits()));

    // The commander's enhancements are not the field rig's.
    let heavy_cannon = world.blueprints.enhancement_id("heavy_cannon");
    world.advance_tick(&[SimCommand::Enhance { units: vec![support], enhancement: heavy_cannon }]);
    assert!(world.ecs.get::<&sim_engine::sim::components::Enhancing>(entity).is_err());
}

#[test]
fn a_radar_jammer_puts_decoys_on_enemy_radar_while_paid_for() {
    let mut world = world_with(true);
    world.spawn_finished_structure(world.blueprints.id("radar"), Team::Red, 50, 50).unwrap();
    // 1,000 from the radar: on it, out of its sight.
    let support = spawn(&mut world, "field_rig", Team::Blue, point(2_020, 1_010));
    run(&mut world, 10);
    assert_eq!(world.snapshot_for(Some(Team::Red)).blips.len(), 1, "itself, at first");
    install(&mut world, support, "radar_jammer");
    run(&mut world, 10);
    assert!(world.ecs.get::<&Intel>(world.unit_entity(support).unwrap()).unwrap().jamming);
    let blips = world.snapshot_for(Some(Team::Red)).blips.len();
    assert!(blips > 1 && blips <= 6, "itself and up to five decoys, not {blips}");
    world.economies[0].energy = Res::ZERO;
    world.economies[0].energy_income = Res::ZERO;
    run(&mut world, 1);
    assert!(!world.ecs.get::<&Intel>(world.unit_entity(support).unwrap()).unwrap().jamming, "unpaid, it stops");
}

#[test]
fn a_field_rig_with_a_teleporter_can_teleport() {
    let mut world = world_with(false);
    let support = spawn(&mut world, "field_rig", Team::Blue, point(1_000, 1_000));
    install(&mut world, support, "support_teleporter");
    world.advance_tick(&[SimCommand::Teleport { units: vec![support], point: point(2_000, 1_000) }]);
    assert!(world.ecs.get::<&Teleporting>(world.unit_entity(support).unwrap()).is_ok());
    run(&mut world, 260);
    assert_eq!(world.unit_position(support).unwrap().xy(), point(2_000, 1_000));
}

#[test]
fn a_field_rigs_onboard_reactor_adds_income() {
    let mut world = world_with(false);
    let support = spawn(&mut world, "field_rig", Team::Blue, point(1_000, 1_000));
    run(&mut world, 1);
    let (mass, energy) = (world.economies[0].mass_income, world.economies[0].energy_income);
    install(&mut world, support, "support_onboard_reactor");
    run(&mut world, 1);
    assert_eq!(world.economies[0].mass_income - mass, Res::from_num(1));
    assert_eq!(world.economies[0].energy_income - energy, Res::from_num(100));
}
