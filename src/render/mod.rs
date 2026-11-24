//! Presentation. Floats are allowed here, because nothing in this module feeds back
//! into the simulation except commands, which are built from fixed-point goals.
#![allow(clippy::disallowed_types)]

pub mod camera;
pub mod effects;
pub mod instances;
pub mod links;
pub mod meshes;
pub mod pipeline;
mod sight;
