//! The balance harness's grounds (2026-11-24), shared with the playtest scenarios that play its
//! fights by hand (`data/scenarios.toml`): flat land, as every duel was before; open sea over the
//! whole ground; or a shore, the sea in the west, on Blue's side, and land in the east, on Red's,
//! for ships against what stands ashore.

use serde::Deserialize;

use crate::math::fp::fp_int;

use super::components::MoveClass;
use super::config::{SimConfig, TERRAIN_CELL_UNITS};
use super::terrain::{Terrain, HEIGHT_FRAC_BITS, SEA_DEPTH_UNITS};
use super::world::SimWorld;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ground {
    #[default]
    Land,
    Water,
    Shore,
}

/// The sea grounds, in units: the water's surface, the sea floor below it, as deep as every
/// sea's open floor (`SEA_DEPTH_UNITS`), as the named maps' seas are, and the land of the shore
/// above it.
const WATER_LEVEL: i32 = 100;
const SEA_FLOOR: i32 = WATER_LEVEL - SEA_DEPTH_UNITS;
const SHORE_LAND: i32 = 110;

impl Ground {
    /// Where the shore's land begins on a ground `map_units` across: west of the middle, so that a
    /// position built round the middle stands ashore, and both sides' starts are on their own
    /// sides of it.
    pub const fn shore_x(map_units: i32) -> i32 {
        map_units / 2 - 300
    }

    /// Whether the ground at `x` is sea.
    pub const fn is_sea(self, map_units: i32, x: i32) -> bool {
        match self {
            Ground::Land => false,
            Ground::Water => true,
            Ground::Shore => x < Self::shore_x(map_units),
        }
    }

    /// Whether a unit that moves so can stand on the ground at `x`.
    pub const fn holds(self, map_units: i32, x: i32, moves: MoveClass) -> bool {
        match moves {
            MoveClass::Air | MoveClass::Hover | MoveClass::Seabed => true,
            MoveClass::Naval | MoveClass::Submarine => self.is_sea(map_units, x),
            MoveClass::Land => !self.is_sea(map_units, x),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Ground::Land => "flat open ground",
            Ground::Water => "open sea",
            Ground::Shore => "a shore, the sea in the west and land in the east",
        }
    }

    /// A world on this ground, of the size `config` gives; flat land is the config's own terrain.
    /// The caller prepares the map (`SimWorld::prepare_map`) once its structures stand.
    pub fn world(self, config: SimConfig, seed: u64) -> SimWorld {
        if self == Ground::Land {
            return SimWorld::new(config, seed);
        }
        let map_units = config.map_units;
        let cells = (map_units / TERRAIN_CELL_UNITS) as usize;
        let samples = cells + 1;
        let shore = (Self::shore_x(map_units) / TERRAIN_CELL_UNITS) as usize;
        let ashore = |sample: usize| self == Ground::Shore && sample >= shore;
        let heights = (0..samples * samples).map(|index| ((if ashore(index % samples) { SHORE_LAND } else { SEA_FLOOR }) << HEIGHT_FRAC_BITS) as i16).collect();
        // Land units keep to the dry cells, those with land at every corner.
        let blocked: Vec<bool> = (0..cells * cells).map(|index| !ashore(index % cells)).collect();
        let terrain = Terrain::from_heights(map_units, heights, &blocked, fp_int(WATER_LEVEL));
        SimWorld::with_terrain(config, seed, terrain)
    }
}
