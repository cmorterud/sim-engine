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
    /// Hills with 180° rotational symmetry, so that opposing starts are equal.
    MirroredHills,
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
    /// Units only see and target what their team can see.
    pub fog_of_war: bool,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            map_units: MAX_MAP_UNITS,
            terrain: TerrainStyle::Hills,
            friendly_fire: false,
            invulnerable_units: false,
            fog_of_war: false,
        }
    }
}

/// Fog of war resolution.
pub const VISIBILITY_CELL_UNITS: i32 = 80;
/// One unit of build power reclaims this much mass per second.
pub const RECLAIM_MASS_PER_POWER: i64 = 2;
/// Wrecks keep this share of a unit's mass cost, in thousandths (GAME_SPECIFICATION.md §6.1).
pub const WRECK_MASS_PERMILLE: i64 = 810;
/// How often idle patrolling engineers look for work, in ticks.
pub const PATROL_SEARCH_INTERVAL: u32 = 10;
/// How far patrolling engineers look for work.
pub const PATROL_SEARCH_RADIUS_UNITS: i32 = 300;
