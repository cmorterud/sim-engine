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

/// Each side's rocks hold what their images on the other side hold. A rock's mass grows with
/// its size, which a match draws for each rock, and each piece of a map file's scenery once drew
/// its own: a rock and its image differed, so one side had more to reclaim near its base, a
/// different amount every seed. The skirmish map always drew one size for a rock and its image.
#[test]
fn every_rock_on_a_map_from_a_file_holds_what_its_images_hold() {
    use sim_engine::sim::components::Wreck;
    use sim_engine::sim::maps::{bundled, BUNDLED};
    use sim_engine::sim::world::SimWorld;
    // Rocks by position, in raw fixed-point units, with their radius and mass.
    let rocks = |world: &SimWorld| {
        let mut rocks: Vec<((i64, i64), Fp, String)> = world
            .wrecks
            .iter()
            .filter_map(|&(_, entity)| {
                let wreck = *world.ecs.get::<&Wreck>(entity).ok()?;
                (wreck.kind == WreckKind::Rock).then(|| ((i64::from(wreck.position.x.to_bits()), i64::from(wreck.position.y.to_bits())), wreck.radius, wreck.mass.to_string()))
            })
            .collect();
        rocks.sort_unstable_by_key(|rock| rock.0);
        rocks
    };
    for name in BUNDLED {
        let map = Arc::new(bundled(name).expect("a bundled map"));
        let full = i64::from(fp_int(map.map_units).to_bits());
        // The 1v1, and every team layout the map has: a 2v2's opposite numbers are images
        // turned about the center, and its teammates are images across the middle row.
        let mut sources = vec![MapSource::Map(Arc::clone(&map))];
        sources.extend(map.layouts.iter().filter_map(|layout| MapSource::Map(Arc::clone(&map)).teams(layout.len() / 2)));
        for source in sources {
            for seed in 1..=3 {
                let world = source.scenario(seed).world;
                let rocks = rocks(&world);
                assert!(rocks.len() > 100, "{name}: {} rocks", rocks.len());
                let at = |point: (i64, i64)| rocks.binary_search_by_key(&point, |rock| rock.0).ok().map(|index| &rocks[index]);
                for rock in &rocks {
                    let (x, y) = rock.0;
                    let turned = at((full - x, full - y)).unwrap_or_else(|| panic!("{name}: the rock at {x}, {y} has no turned image"));
                    assert_eq!((turned.1, &turned.2), (rock.1, &rock.2), "{name} ({} a side), seed {seed}: the rock at {x}, {y} and its turned image differ", source.per_team());
                    // On a map that is its own image in both mirrors, any image there is matches too.
                    for image in [(full - x, y), (x, full - y)] {
                        if let Some(other) = at(image) {
                            assert_eq!((other.1, &other.2), (rock.1, &rock.2), "{name} ({} a side), seed {seed}: the rock at {x}, {y} and its mirror image differ", source.per_team());
                        }
                    }
                }
            }
        }
    }
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
