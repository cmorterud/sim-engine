//! Shared by the tests that `mod common;` it.

use std::sync::Arc;

use sim_engine::sim::blueprints::BlueprintDb;
use sim_engine::sim::world::SimWorld;

/// The standard data with the enhancements out of the roster since 2026-11-27 (Orbital Redeploy,
/// Thermal Camouflage, the field rig's Radar Jammer and Orbital Redeploy), so that the engine's
/// teleporting, cloaking and personal jamming stay tested for data that uses them.
#[allow(dead_code)]
pub fn with_retired_enhancements() -> BlueprintDb {
    let text = format!("{}\n{}", include_str!("../../data/units.toml"), include_str!("../fixtures/retired_enhancements.toml"));
    BlueprintDb::parse(&text).expect("the data with the retired enhancements is valid")
}

/// Gives a world, before anything is spawned in it, the retired enhancements too.
#[allow(dead_code)]
pub fn retire_into(world: &mut SimWorld) {
    world.blueprints = Arc::new(with_retired_enhancements());
}
