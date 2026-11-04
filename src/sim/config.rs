//! World-scale constants (SPECIFICATION.md §1.4) and per-world settings.

/// Terrain and cost grid resolution. Maps must be a multiple of `SECTOR_UNITS`.
pub const TERRAIN_CELL_UNITS: i32 = 20;
/// Flow field sector width in terrain cells.
pub const SECTOR_CELLS: usize = 64;
pub const SECTOR_UNITS: i32 = SECTOR_CELLS as i32 * TERRAIN_CELL_UNITS;
pub const MAX_MAP_UNITS: i32 = 81_920;

/// Spatial grid resolution. Tuned from the Phase 3 benchmark.
pub const SPATIAL_CELL_UNITS: i32 = 8;
/// Queries above this radius scan all entries instead of covering many rows.
pub const SPATIAL_SCAN_RADIUS_UNITS: i32 = 512;

/// About 9.8 m/s² expressed per tick², as raw `Fp` bits.
pub const GRAVITY_RAW: i32 = 401;

/// Constant work budget. Never derived from elapsed time.
pub const MAX_FIELD_BUILDS_PER_TICK: usize = 8;
pub const MAX_CACHED_FIELDS: usize = 4_096;
/// A moving goal re-routes at most this often, per group.
pub const REROUTE_INTERVAL_TICKS: u32 = 10;
/// Units without a target search when `(SimId + tick) % TARGET_SEARCH_INTERVAL == 0`.
pub const TARGET_SEARCH_INTERVAL: u32 = 5;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TerrainStyle {
    Flat,
    Hills,
}

#[derive(Copy, Clone, Debug)]
pub struct SimConfig {
    /// Map width and height in units.
    pub map_units: i32,
    pub terrain: TerrainStyle,
    /// Whether projectiles can hit units on the firing team.
    pub friendly_fire: bool,
    /// Benchmark setting: keeps unit counts constant so the workload does not get easier.
    pub invulnerable_units: bool,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            map_units: MAX_MAP_UNITS,
            terrain: TerrainStyle::Hills,
            friendly_fire: false,
            invulnerable_units: false,
        }
    }
}
