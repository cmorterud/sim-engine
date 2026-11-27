//! Shared by the tests that `mod common;` it.

use std::sync::Arc;

use sim_engine::sim::blueprints::BlueprintDb;
use sim_engine::sim::world::SimWorld;

/// The standard data with what has left the roster since 2026-11-27 put back: the enhancements
/// (Orbital Redeploy, Thermal Camouflage, the field rig's Radar Jammer and Orbital Redeploy) and
/// the Field Workshops, so that the engine's teleporting, cloaking, personal jamming and
/// assisting stations stay tested for data that uses them.
#[allow(dead_code)]
pub fn with_retired() -> BlueprintDb {
    let text = format!(
        "{}\n{}\n{}",
        include_str!("../../data/units.toml"),
        include_str!("../fixtures/retired_enhancements.toml"),
        include_str!("../fixtures/retired_units.toml")
    );
    BlueprintDb::parse(&text).expect("the data with what was retired is valid")
}

/// Gives a world, before anything is spawned in it, what was retired too.
#[allow(dead_code)]
pub fn retire_into(world: &mut SimWorld) {
    world.blueprints = Arc::new(with_retired());
}
