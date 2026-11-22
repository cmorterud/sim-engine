//! Matches on map data (GAME_SPECIFICATION.md §20): a small map built here in code, played and
//! replayed.

use std::sync::Arc;

use sim_engine::ai::{play_match_on, replay_match_on};
use sim_engine::math::fp::{fp_int, Fp, Vec2Fp};
use sim_engine::sim::components::{Team, WreckKind};
use sim_engine::sim::scenario::{from_map, MapData, MapSource};
use sim_engine::sim::terrain::HEIGHT_FRAC_BITS;

fn point(x: i32, y: i32) -> Vec2Fp {
    Vec2Fp::new(fp_int(x), fp_int(y))
}

/// A 10,240-unit map with a gentle slope, a steep ridge with a gap in it, deposits by each
/// start, and a little scenery.
fn test_map() -> MapData {
    let cells = 512usize;
    let samples = cells + 1;
    let heights = (0..samples * samples).map(|index| (((index % samples) / 8) as i16) << HEIGHT_FRAC_BITS).collect();
    let mut blocked = vec![false; cells * cells];
    // A ridge across the middle, open at both ends.
    for x in 60..452 {
        blocked[256 * cells + x] = true;
    }
    let deposits = vec![(140, 120), (150, 130), (370, 390), (360, 380)];
    let scenery = vec![(WreckKind::Tree, point(4_000, 3_000)), (WreckKind::Rock, point(6_000, 7_000))];
    MapData {
        name: "test".into(),
        map_units: 10_240,
        heights,
        blocked,
        water_level: Fp::from_num(-1000),
        deposits,
        vents: vec![(200, 300), (311, 211)],
        starts: [point(2_800, 2_400), point(7_440, 7_840)],
        layouts: Vec::new(),
        scenery,
    }
}

#[test]
fn a_map_becomes_a_world_with_its_terrain_starts_deposits_and_scenery() {
    let map = test_map();
    let world = from_map(&map, 3).world;
    assert_eq!(world.terrain.map_units(), 10_240);
    assert_eq!(world.starts, map.starts);
    assert_eq!(world.deposits, map.deposits);
    assert_eq!(world.wrecks.len(), 2);
    assert!(!world.terrain.is_passable(point(3_000, 256 * 20 + 10)), "the ridge is blocked");
    assert!(world.terrain.is_passable(point(500, 256 * 20 + 10)), "but not its gap");
    // The slope is there: the far side of the map is higher.
    assert!(world.terrain.height_at(point(10_000, 500)) > world.terrain.height_at(point(500, 500)));
    let commanders: Vec<Vec2Fp> = world.commanders.iter().map(|id| world.unit_position(id.unwrap()).unwrap().xy()).collect();
    assert_eq!(commanders, map.starts);
    assert_eq!(world.commanders.len(), 2);
    let _ = Team::Blue;
}

#[test]
fn the_same_map_and_seed_play_the_same_match() {
    let source = MapSource::Map(Arc::new(test_map()));
    let first = play_match_on(&source, 5, 3_000, |_| {});
    let again = play_match_on(&source, 5, 3_000, |_| {});
    assert_eq!(first.checksums, again.checksums);
    assert_eq!(replay_match_on(&source, 5, &first.log), first.checksums, "and a replay of its orders matches");
    assert!(first.units_built_peak > 10, "both sides built something: {}", first.units_built_peak);
}
