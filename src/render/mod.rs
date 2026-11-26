//! Presentation. Floats are allowed here, because nothing in this module feeds back
//! into the simulation except commands, which are built from fixed-point goals.
#![allow(clippy::disallowed_types)]

pub mod camera;
pub mod climate;
pub mod detail;
pub mod effects;
mod gpu_timing;
pub mod instances;
pub mod land;
pub mod landscape;
pub mod links;
pub mod meshes;
pub mod pipeline;
pub mod relief;
pub mod shore;
mod sight;
pub mod tiles;
pub mod waves;
pub mod wrecks;
