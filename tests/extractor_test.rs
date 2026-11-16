//! Mass extractors: their upkeep, their upgrades from T1 to T3, and what an energy stall does
//! to them (GAME_SPECIFICATION.md §3, with FAF's numbers).

use sim_engine::math::fp::fp_int;
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Blueprinted, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::SimWorld;

fn world() -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        // Half full, so that gains are not lost to full storage.
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(1_000_000);
        }
        economy.mass = Res::from_num(500_000);
        economy.energy = Res::from_num(500_000);
    }
    world
}

fn extractor(world: &mut SimWorld, cell_x: u32, cell_y: u32) -> SimId {
    world.deposits.push((cell_x, cell_y));
    let blueprint = world.blueprints.id("mass_extractor");
    world.spawn_finished_structure(blueprint, Team::Blue, cell_x, cell_y).unwrap()
}

fn key_of(world: &SimWorld, id: SimId) -> String {
    let blueprint = world.ecs.get::<&Blueprinted>(world.unit_entity(id).unwrap()).unwrap().0;
    world.blueprints.get(blueprint).key.clone()
}

/// The mass and energy gained over one tick.
fn one_tick(world: &mut SimWorld) -> (Res, Res) {
    let (mass, energy) = (world.economies[0].mass, world.economies[0].energy);
    world.advance_tick(&[]);
    (world.economies[0].mass - mass, world.economies[0].energy - energy)
}

#[test]
fn a_t1_extractor_makes_two_mass_a_second_for_two_energy() {
    let mut world = world();
    let mex = extractor(&mut world, 50, 50);
    world.advance_tick(&[]);
    assert_eq!(one_tick(&mut world), (Res::from_num(2) / 10, -Res::from_num(2) / 10));
    assert_eq!(world.unit_health(mex), Some(fp_int(400)));
}

#[test]
fn an_extractor_upgrades_to_t2_and_then_t3_while_it_keeps_working() {
    let mut world = world();
    let mex = extractor(&mut world, 50, 50);
    world.advance_tick(&[SimCommand::Upgrade { units: vec![mex] }]);
    // While it upgrades it still makes its 2 a second, and the upgrade costs 900 mass over
    // 90 seconds: 10 a second.
    let (mass, _) = one_tick(&mut world);
    assert!((mass - (Res::from_num(2) / 10 - Res::from_num(1))).abs() < Res::from_num(0.001), "{mass}");
    // 900 build time at its own 10 build power is 90 seconds.
    let took = (0..1_000).position(|_| {
        world.advance_tick(&[]);
        key_of(&world, mex) == "mass_extractor_t2"
    });
    assert!(took.is_some_and(|ticks| (890..=905).contains(&ticks)), "{took:?}");
    world.advance_tick(&[]);
    assert_eq!(one_tick(&mut world), (Res::from_num(6) / 10, -Res::from_num(9) / 10), "6 materiel a second, for 9 energy");
    assert_eq!(world.unit_health(mex), Some(fp_int(2_000)), "full health kept at the same share");

    // And on to T3: 2,875 build time at the T2's 15 build power, about 192 seconds.
    world.advance_tick(&[SimCommand::Upgrade { units: vec![mex] }]);
    let took = (0..2_100).position(|_| {
        world.advance_tick(&[]);
        key_of(&world, mex) == "mass_extractor_t3"
    });
    assert!(took.is_some_and(|ticks| (1_900..=1_930).contains(&ticks)), "{took:?}");
    world.advance_tick(&[]);
    assert_eq!(one_tick(&mut world), (Res::from_num(18) / 10, -Res::from_num(54) / 10), "18 materiel a second, for 54 energy");
    world.advance_tick(&[SimCommand::Upgrade { units: vec![mex] }]);
    assert_eq!(key_of(&world, mex), "mass_extractor_t3", "T3 is the last tier");
}

#[test]
fn an_energy_stall_slows_every_extractor_alike() {
    let mut world = world();
    // Twenty extractors want 4 energy a tick. One generator, far from them, makes 2.
    for index in 0..20 {
        extractor(&mut world, 20 + index * 3, 20);
    }
    let generator = world.blueprints.id("power_generator");
    world.spawn_finished_structure(generator, Team::Blue, 200, 200).unwrap();
    world.economies[0].energy = Res::ZERO;
    world.advance_tick(&[]);
    world.economies[0].energy = Res::ZERO;
    let (mass, _) = one_tick(&mut world);
    // A tenth of a second's energy is not exact in fixed point, so these are a hair off.
    let near = |value: Res, wanted: Res| (value - wanted).abs() < Res::from_num(0.0001);
    assert!(near(world.economies[0].extractor_efficiency, Res::from_num(1) / 2), "{}", world.economies[0].extractor_efficiency);
    assert!(near(mass, Res::from_num(2)), "half of their 4 materiel a tick, not {mass}");
    let shown = world.snapshot_for(Some(Team::Blue)).economy.mass_income;
    assert!((19..=20).contains(&shown), "the bar shows what they really make, not {shown}");
}

#[test]
fn a_paused_extractor_makes_and_spends_nothing() {
    let mut world = world();
    let mex = extractor(&mut world, 50, 50);
    world.advance_tick(&[SimCommand::SetPaused { units: vec![mex], paused: true }]);
    world.advance_tick(&[]);
    assert_eq!(one_tick(&mut world), (Res::ZERO, Res::ZERO));
    world.advance_tick(&[SimCommand::SetPaused { units: vec![mex], paused: false }]);
    world.advance_tick(&[]);
    assert_eq!(one_tick(&mut world).0, Res::from_num(2) / 10);
}

#[test]
fn a_generator_beside_an_extractor_cuts_its_upkeep() {
    let mut world = world();
    let mex = extractor(&mut world, 50, 50);
    world.advance_tick(&[]);
    let alone = one_tick(&mut world).1;
    let generator = world.blueprints.id("power_generator");
    world.spawn_finished_structure(generator, Team::Blue, 51, 50).unwrap();
    world.advance_tick(&[]);
    let beside = one_tick(&mut world).1 - Res::from_num(2);
    assert!(beside > alone, "it pays less with a generator beside it: {beside} against {alone}");
    let _ = mex;
}
