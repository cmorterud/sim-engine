//! Builders' other work against the unit data (GAME_SPECIFICATION.md §6, §5): every builder that
//! walks captures an enemy in half the enemy's build time at its build power, for the enemy's
//! energy, and reclaims at 2 materiel and 10 energy a second for each point of build power.

use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::blueprints::{Blueprint, Res};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Builder, Team, UnitStats, Wreck, WreckKind};
use sim_engine::sim::config::{SimConfig, TerrainStyle};
use sim_engine::sim::world::SimWorld;

fn world() -> SimWorld {
    let config = SimConfig { map_units: 10_240, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    for economy in &mut world.economies {
        for value in [&mut economy.base_mass_capacity, &mut economy.mass_capacity, &mut economy.base_energy_capacity, &mut economy.energy_capacity] {
            *value = Res::from_num(10_000_000);
        }
        economy.mass = Res::from_num(1_000_000);
        economy.energy = Res::from_num(1_000_000);
    }
    world
}

/// The builders that walk, other than ships, which cannot reach a unit on land.
fn walking_builders() -> Vec<Blueprint> {
    world()
        .blueprints
        .iter()
        .filter(|def| !def.is_structure() && !def.is_air() && def.build_power > Fp::ZERO && def.speed > Fp::ZERO)
        .filter(|def| !matches!(def.movement, sim_engine::sim::components::MoveClass::Naval | sim_engine::sim::components::MoveClass::Submarine))
        .cloned()
        .collect()
}

#[test]
fn every_builder_captures_in_half_the_build_time_for_the_energy() {
    let mut failures = Vec::new();
    let builders = walking_builders();
    assert!(builders.len() >= 5, "only {} builders", builders.len());
    for def in builders {
        // The same, with and without the capture, so that the builder's own income and upkeep
        // are left out of what the capture cost.
        let setup = || {
            let mut world = world();
            let builder = world.spawn_mobile(world.blueprints.id(&def.key), Team::Blue, Vec2Fp::new(fp_int(3_000), fp_int(3_000)));
            // Close beside it, unable to shoot or move.
            let target = world.spawn_mobile(world.blueprints.id("t1_medium_tank"), Team::Red, Vec2Fp::new(fp_int(3_040), fp_int(3_000)));
            world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(target).unwrap()).unwrap().enabled = false;
            world.ecs.get::<&mut UnitStats>(world.unit_entity(target).unwrap()).unwrap().max_speed = Fp::ZERO;
            world.ecs.get::<&mut sim_engine::sim::components::Combat>(world.unit_entity(builder).unwrap()).unwrap().enabled = false;
            (world, builder, target)
        };
        let (mut world, builder, target) = setup();
        let (mut control, _, _) = setup();
        let target_def = world.blueprints.get(world.blueprints.id("t1_medium_tank")).clone();
        let power = world.ecs.get::<&Builder>(world.unit_entity(builder).unwrap()).unwrap().power;
        world.advance_tick(&[SimCommand::Capture { units: vec![builder], target }]);
        control.advance_tick(&[]);
        let expected = target_def.build_time.to_num::<f64>() * 10.0 / 2.0 / power.to_num::<f64>();
        let taken = (0..(expected as u32 * 2 + 100)).find(|_| {
            world.advance_tick(&[]);
            control.advance_tick(&[]);
            world.ecs.get::<&UnitStats>(world.unit_entity(target).unwrap()).unwrap().team == Team::Blue
        });
        let spent = control.economies[0].energy - world.economies[0].energy;
        match taken {
            None => failures.push(format!("{}: never captured it", def.key)),
            Some(ticks) if (f64::from(ticks + 1) - expected).abs() > expected * 0.02 + 3.0 => {
                failures.push(format!("{}: captured in {} ticks, expected {expected:.0}", def.key, ticks + 1));
            }
            _ if (spent - target_def.energy).abs() > target_def.energy / 100 + Res::from_num(1) => {
                failures.push(format!("{}: spent {spent} energy, expected {}", def.key, target_def.energy));
            }
            _ => {}
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn every_builder_reclaims_at_its_build_powers_rate() {
    let mut failures = Vec::new();
    for def in walking_builders() {
        let mut world = world();
        let builder = world.spawn_mobile(world.blueprints.id(&def.key), Team::Blue, Vec2Fp::new(fp_int(3_000), fp_int(3_000)));
        let power = world.ecs.get::<&Builder>(world.unit_entity(builder).unwrap()).unwrap().power;
        // A tree beside it, for energy, and a big rock, for materiel.
        for (kind, radius) in [(WreckKind::Tree, Fp::ZERO), (WreckKind::Rock, fp_int(10))] {
            let wreck = world.spawn_scenery(kind, Vec2Fp::new(fp_int(3_030), fp_int(3_000)), radius);
            // A great deal in it, so that it lasts.
            let index = world.wrecks.binary_search_by_key(&wreck, |wreck| wreck.0).unwrap();
            let entity = world.wrecks[index].1;
            {
                let mut held = world.ecs.get::<&mut Wreck>(entity).unwrap();
                held.mass = Res::from_num(100_000);
                held.energy = Res::from_num(100_000);
            }
            world.advance_tick(&[SimCommand::Reclaim { units: vec![builder], target: wreck }]);
            for _ in 0..30 {
                world.advance_tick(&[]);
            }
            let before = *world.ecs.get::<&Wreck>(entity).unwrap();
            let ticks = 50;
            for _ in 0..ticks {
                world.advance_tick(&[]);
            }
            let after = *world.ecs.get::<&Wreck>(entity).unwrap();
            let per_second = |gone: Res| (gone / i64::from(ticks) * 10).to_num::<f64>();
            let (mass, energy) = (per_second(before.mass - after.mass), per_second(before.energy - after.energy));
            let (want_mass, want_energy) = (power.to_num::<f64>() * 2.0, power.to_num::<f64>() * 10.0);
            if (mass - want_mass).abs() > want_mass * 0.01 + 0.01 || (energy - want_energy).abs() > want_energy * 0.01 + 0.01 {
                failures.push(format!("{} reclaiming a {kind:?}: {mass:.2} materiel and {energy:.2} energy a second, expected {want_mass:.2} and {want_energy:.2}", def.key));
            }
            world.advance_tick(&[SimCommand::Stop { units: vec![builder] }]);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
