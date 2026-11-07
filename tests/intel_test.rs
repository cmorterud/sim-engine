//! Intel (GAME_SPECIFICATION.md §9): radar blips, stealth fields, jamming, omni, and cloaking.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Blueprinted, Combat, Intel, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::{decoy_spot, SimWorld};

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// A flat 5,120-unit map under fog of war, with energy to spare for both teams.
fn foggy_world() -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: true, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.mass] {
            *value = Res::from_num(100_000);
        }
        for value in [&mut economy.base_energy_capacity, &mut economy.energy_capacity, &mut economy.energy] {
            *value = Res::from_num(100_000);
        }
    }
    world
}

fn structure(world: &mut SimWorld, key: &str, team: Team, cell_x: u32, cell_y: u32) -> SimId {
    let blueprint = world.blueprints.id(key);
    world.spawn_finished_structure(blueprint, team, cell_x, cell_y).unwrap_or_else(|| panic!("no room for {key}"))
}

/// A unit that does not shoot.
fn passive(world: &mut SimWorld, key: &str, team: Team, at: Vec2Fp) -> SimId {
    let blueprint = world.blueprints.id(key);
    let id = world.spawn_mobile(blueprint, team, at);
    world.ecs.get::<&mut Combat>(world.unit_entity(id).unwrap()).unwrap().range = Fp::ZERO;
    id
}

/// A commander with the cloaking enhancement on its back.
fn cloaking_commander(world: &mut SimWorld, at: Vec2Fp) -> SimId {
    let acu = world.blueprints.id("acu");
    let commander = world.spawn_mobile(acu, Team::Blue, at);
    world.install_enhancement(commander, world.blueprints.enhancement_id("personal_cloak"));
    commander
}

fn run(world: &mut SimWorld, ticks: u32) {
    for _ in 0..ticks {
        world.advance_tick(&[]);
    }
}

fn blips(world: &SimWorld, viewer: Team) -> Vec<[i32; 3]> {
    world.snapshot_for(Some(viewer)).blips
}

fn shown(world: &SimWorld, viewer: Team, id: SimId) -> bool {
    world.snapshot_for(Some(viewer)).units.iter().any(|unit| unit.id == id)
}

#[test]
fn radar_shows_an_enemy_out_of_sight_as_a_blip() {
    let mut world = foggy_world();
    let radar = structure(&mut world, "radar", Team::Blue, 50, 50);
    // 1,000 away: beyond the radar's own 300 of sight, within its 1,600 of radar.
    let enemy = passive(&mut world, "t1_light_bot", Team::Red, point(2_020, 1_010));
    run(&mut world, 10);
    assert!(!shown(&world, Team::Blue, enemy), "it is out of sight");
    assert_eq!(blips(&world, Team::Blue).len(), 1, "but on radar");
    let sensors = world.snapshot_for(Some(Team::Blue)).sensor_ranges;
    assert_eq!(sensors.len(), 1);
    assert_eq!(sensors[0].radar, fp_int(1_600).to_bits());
    assert!(world.snapshot_for(Some(Team::Red)).sensor_ranges.is_empty(), "only the viewer's own sensors");

    // In sight it is a unit, not a blip.
    let scout = passive(&mut world, "t1_light_bot", Team::Blue, point(1_900, 1_010));
    run(&mut world, 10);
    assert!(shown(&world, Team::Blue, enemy));
    assert!(blips(&world, Team::Blue).is_empty());
    let _ = (radar, scout);
}

#[test]
fn a_radar_without_energy_or_paused_shows_nothing() {
    let mut world = foggy_world();
    let radar = structure(&mut world, "radar", Team::Blue, 50, 50);
    passive(&mut world, "t1_light_bot", Team::Red, point(2_020, 1_010));
    world.economies[0].energy = Res::ZERO;
    run(&mut world, 10);
    assert!(blips(&world, Team::Blue).is_empty(), "with no energy it is off");
    assert_eq!(world.snapshot_for(Some(Team::Blue)).unpowered, [radar]);

    world.economies[0].energy = Res::from_num(10_000);
    world.advance_tick(&[SimCommand::SetPaused { units: vec![radar], paused: true }]);
    run(&mut world, 10);
    assert!(blips(&world, Team::Blue).is_empty(), "paused, it is off");
    assert!(world.snapshot_for(Some(Team::Blue)).unpowered.is_empty(), "paused is not the same as short of energy");
    world.advance_tick(&[SimCommand::SetPaused { units: vec![radar], paused: false }]);
    run(&mut world, 10);
    assert_eq!(blips(&world, Team::Blue).len(), 1);
}

#[test]
fn a_stealth_field_hides_blips_until_omni_strips_it() {
    let mut world = foggy_world();
    structure(&mut world, "radar", Team::Blue, 50, 50);
    // A Red stealth field and jammer far from everything, with a Red unit inside the field.
    let field = structure(&mut world, "stealth_field_generator", Team::Red, 100, 50);
    passive(&mut world, "t1_light_bot", Team::Red, point(2_100, 1_010));
    run(&mut world, 10);
    // The field itself and the bot are hidden. Only decoys show: never more than eight.
    let decoys = blips(&world, Team::Blue);
    assert!(decoys.len() <= 8, "{} blips", decoys.len());
    let near = |blip: &[i32; 3], x: i32, y: i32| (blip[0] - fp_int(x).to_bits()).abs() < fp_int(20).to_bits() && (blip[1] - fp_int(y).to_bits()).abs() < fp_int(20).to_bits();
    assert!(!decoys.iter().any(|blip| near(blip, 2_100, 1_010)), "the bot in the field makes no blip");

    // A Blue omni sensor 600 away: its 800 of omni covers the field, beyond its 400 of sight.
    let omni = structure(&mut world, "radar", Team::Blue, 70, 50);
    let entity = world.unit_entity(omni).unwrap();
    world.ecs.get::<&mut Blueprinted>(entity).unwrap().0 = world.blueprints.id("omni_sensor");
    run(&mut world, 10);
    let now = blips(&world, Team::Blue);
    assert!(now.iter().any(|blip| near(blip, 2_100, 1_010)), "omni strips the bot's stealth: {now:?}");
    assert!(now.iter().any(|blip| near(blip, 2_020, 1_020)), "and the generator's: {now:?}");
    // Decoys within omni are gone: any left are beyond its reach, at 1,410 + 800.
    for blip in now.iter().filter(|blip| !near(blip, 2_100, 1_010) && !near(blip, 2_020, 1_020)) {
        assert!(blip[0] > fp_int(2_210).to_bits(), "a decoy within omni: {blip:?}");
    }
    let _ = field;
}

#[test]
fn decoys_stay_in_range_and_move_from_time_to_time() {
    for jammer in [3, 40, 999] {
        for period in 0..20 {
            for decoy in 0..8 {
                let (dx, dy) = decoy_spot(SimId(jammer), period, decoy, 450);
                assert!(dx * dx + dy * dy <= 450 * 450, "({dx}, {dy}) is out of range");
            }
        }
    }
    assert_eq!(decoy_spot(SimId(7), 3, 2, 450), decoy_spot(SimId(7), 3, 2, 450), "the same every time");
    assert_ne!(decoy_spot(SimId(7), 3, 2, 450), decoy_spot(SimId(7), 4, 2, 450), "and different later");
}

#[test]
fn a_cloaked_commander_cannot_be_seen_or_shot_until_an_enemy_comes_close() {
    let mut world = foggy_world();
    let commander = cloaking_commander(&mut world, point(1_000, 1_000));
    world.ecs.get::<&mut Combat>(world.unit_entity(commander).unwrap()).unwrap().range = Fp::ZERO;
    // A Red bot 300 away: within its sight and its 340 of range.
    let bot = world.blueprints.id("t1_light_bot");
    let bot = world.spawn_mobile(bot, Team::Red, point(1_300, 1_000));
    world.advance_tick(&[SimCommand::SetCloak { units: vec![commander], on: true }]);
    let full = world.unit_health(commander).unwrap();
    run(&mut world, 60);
    assert_eq!(world.unit_health(commander), Some(full), "nothing should fire at it");
    assert!(!shown(&world, Team::Red, commander), "and Red should not see it");
    assert!(blips(&world, Team::Red).is_empty(), "nor have it on radar");
    // It costs 3,000 energy a second, and its stealth 50 more, on top of nothing else.
    let stored = world.economies[0].energy;
    world.advance_tick(&[]);
    assert_eq!(stored - world.economies[0].energy, Res::from_num(305) - world.economies[0].energy_income);

    // Within 100 of the bot, it is found.
    world.ecs.get::<&mut sim_engine::sim::components::Position>(world.unit_entity(bot).unwrap()).unwrap().0.x = fp_int(1_080);
    run(&mut world, 30);
    assert!(world.unit_health(commander).unwrap() < full, "close up, the bot should find it and fire");
}

#[test]
fn a_cloak_drops_when_it_cannot_be_paid_for() {
    let mut world = foggy_world();
    let commander = cloaking_commander(&mut world, point(1_000, 1_000));
    world.advance_tick(&[SimCommand::SetCloak { units: vec![commander], on: true }]);
    assert!(world.ecs.get::<&Intel>(world.unit_entity(commander).unwrap()).unwrap().cloaked);
    world.economies[0].energy = Res::ZERO;
    world.economies[0].energy_income = Res::ZERO;
    world.advance_tick(&[]);
    let intel = *world.ecs.get::<&Intel>(world.unit_entity(commander).unwrap()).unwrap();
    assert!(!intel.cloaked && !intel.cloak_on, "out of energy, the cloak switches off");
    assert!(!intel.stealthed, "and so does its stealth");
}

#[test]
fn an_enemy_commanders_omni_finds_a_cloaked_one() {
    let mut world = foggy_world();
    let acu = world.blueprints.id("acu");
    let blue = cloaking_commander(&mut world, point(1_000, 1_000));
    // 300 away: inside the Red commander's 400 of omni, but well beyond 100.
    let red = world.spawn_mobile(acu, Team::Red, point(1_300, 1_000));
    world.ecs.get::<&mut Combat>(world.unit_entity(red).unwrap()).unwrap().range = Fp::ZERO;
    world.advance_tick(&[SimCommand::SetCloak { units: vec![blue], on: true }]);
    run(&mut world, 10);
    assert!(shown(&world, Team::Red, blue), "omni sees through a cloak");
}

#[test]
fn a_radar_upgrades_in_place_to_an_omni_sensor() {
    let mut world = foggy_world();
    let radar = structure(&mut world, "radar", Team::Blue, 50, 50);
    world.advance_tick(&[SimCommand::Upgrade { units: vec![radar] }]);
    // 1,500 build time at its own 10 build power is 150 seconds.
    run(&mut world, 1_520);
    let blueprint = world.ecs.get::<&Blueprinted>(world.unit_entity(radar).unwrap()).unwrap().0;
    assert_eq!(world.blueprints.get(blueprint).key, "omni_sensor");
    run(&mut world, 10);
    let sensors = world.snapshot_for(Some(Team::Blue)).sensor_ranges;
    assert_eq!((sensors[0].radar, sensors[0].omni), (fp_int(2_400).to_bits(), fp_int(800).to_bits()));
}
