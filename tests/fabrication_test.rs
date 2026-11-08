//! Mass fabricators and hydrocarbon power plants (GAME_SPECIFICATION.md §3.3-3.4, FAF's numbers).

use std::sync::Arc;

use sim_engine::ai::play_match_on;
use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::Res;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Blueprinted, Combat, SimId, Team};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::scenario::MapSource;
use sim_engine::sim::world::SimWorld;

fn world() -> SimWorld {
    let config = SimConfig { map_units: 5_120, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(10_000_000);
        }
        economy.mass = Res::from_num(500_000);
        economy.energy = Res::from_num(5_000_000);
    }
    world
}

fn structure(world: &mut SimWorld, key: &str, cell_x: u32, cell_y: u32) -> Option<SimId> {
    let blueprint = world.blueprints.id(key);
    world.spawn_finished_structure(blueprint, Team::Blue, cell_x, cell_y)
}

fn one_tick(world: &mut SimWorld) -> (Res, Res) {
    let (mass, energy) = (world.economies[0].mass, world.economies[0].energy);
    world.advance_tick(&[]);
    (world.economies[0].mass - mass, world.economies[0].energy - energy)
}

fn near(value: Res, wanted: Res) -> bool {
    (value - wanted).abs() < Res::from_num(0.001)
}

#[test]
fn fabricators_turn_energy_into_mass() {
    let mut world = world();
    structure(&mut world, "mass_fabricator", 50, 50).unwrap();
    world.advance_tick(&[]);
    let (mass, energy) = one_tick(&mut world);
    assert!(near(mass, Res::from_num(1) / 10) && near(energy, -Res::from_num(10)), "1 mass a second for 100 energy: {mass}, {energy}");

    let mut world = self::world();
    structure(&mut world, "mass_fabricator_t3", 50, 50).unwrap();
    world.advance_tick(&[]);
    let (mass, energy) = one_tick(&mut world);
    assert!(near(mass, Res::from_num(16) / 10) && near(energy, -Res::from_num(150)), "16 mass a second for 1,500 energy: {mass}, {energy}");
}

#[test]
fn a_fabricator_blows_up_only_if_it_was_running() {
    for paused in [false, true] {
        let mut world = world();
        let fabricator = structure(&mut world, "mass_fabricator_t3", 50, 50).unwrap();
        // A light bot beside it, which a 5,000-damage blast within 200 would destroy.
        let bot = world.spawn_mobile(world.blueprints.id("t1_light_bot"), Team::Blue, Vec2Fp::new(fp_int(1_090), fp_int(1_030)));
        world.ecs.get::<&mut Combat>(world.unit_entity(bot).unwrap()).unwrap().range = Fp::ZERO;
        world.advance_tick(&[SimCommand::SetPaused { units: vec![fabricator], paused }]);
        world.advance_tick(&[]);
        // Blown up by its owner, so that nothing else is shooting.
        world.advance_tick(&[SimCommand::SelfDestruct { units: vec![fabricator] }]);
        for _ in 0..60 {
            world.advance_tick(&[]);
        }
        assert!(world.unit_entity(fabricator).is_none());
        assert_eq!(world.unit_entity(bot).is_none(), !paused, "paused {paused}: the blast only comes from a running fabricator");
    }
}

#[test]
fn a_hydrocarbon_plant_goes_only_over_a_vent_and_nothing_else_does() {
    let mut world = world();
    world.vents.push((60, 60));
    assert!(structure(&mut world, "hydrocarbon_plant", 40, 40).is_none(), "no vent there");
    assert!(structure(&mut world, "hydrocarbon_plant", 60, 60).is_none(), "the vent must be under its middle, not its corner");
    assert!(structure(&mut world, "power_generator", 60, 60).is_none(), "vents are saved for plants");
    let plant = structure(&mut world, "hydrocarbon_plant", 59, 59).expect("over the vent");
    world.advance_tick(&[]);
    let (_, energy) = one_tick(&mut world);
    assert!(near(energy, Res::from_num(10)), "100 energy a second: {energy}");
    assert_eq!(world.unit_health(plant), Some(fp_int(1_800)));
}

#[test]
fn the_ai_builds_on_vents_near_its_base() {
    let source = MapSource::Imported(Arc::new(sim_engine::sim::maps::highland()));
    let mut built = [false; 2];
    play_match_on(&source, 3, 4_000, |world| {
        for &(_, entity) in &world.units {
            let Ok(blueprint) = world.ecs.get::<&Blueprinted>(entity) else {
                continue;
            };
            if world.blueprints.get(blueprint.0).key == "hydrocarbon_plant" {
                let team = world.ecs.get::<&sim_engine::sim::components::UnitStats>(entity).unwrap().team;
                built[team.index()] = true;
            }
        }
    });
    assert_eq!(built, [true, true], "both sides should build on the vent by their base");
}
