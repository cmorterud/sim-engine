//! Deterministic simulation. Floats, randomized containers, and wall-clock time are banned here.
#![deny(clippy::float_arithmetic)]

pub mod air;
pub mod blueprints;
pub mod combat;
pub mod commands;
pub mod components;
pub mod config;
pub mod enhancements;
pub mod exotic;
pub mod flowfield;
pub mod formation;
pub mod maps;
pub mod production;
pub mod scenario;
pub mod spatial;
pub mod terrain;
pub mod visibility;
pub mod world;
