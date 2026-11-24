//! The skirmish map is a mirror image of itself, turned 180° about its center, and both sides
//! play the same AI, so the two sides should play mirror-image games. Anything in the simulation
//! that favors one side, such as breaking ties by row and column or by unit id, shows up here
//! as the two sides drifting apart. Such biases made the side with the lower ids win 58% of
//! AI-vs-AI matches.

use sim_engine::ai::{advance_ai_vs_ai, AiBrain};
use sim_engine::sim::components::{Blueprinted, Position, Team, UnitStats};
use sim_engine::sim::scenario::skirmish;
use sim_engine::sim::world::SimWorld;

/// A team's units, as blueprint and position in raw fixed-point units, Red's turned about the
/// center, sorted so that the two teams' lists can be compared.
fn units(world: &SimWorld, team: Team) -> Vec<(u16, i64, i64)> {
    let map = i64::from(world.config.map_units) << 12;
    let mut units: Vec<(u16, i64, i64)> = world
        .units
        .iter()
        .filter_map(|&(_, entity)| {
            let stats = world.ecs.get::<&UnitStats>(entity).ok()?;
            if stats.team != team {
                return None;
            }
            let blueprint = world.ecs.get::<&Blueprinted>(entity).ok()?.0 .0;
            let position = world.ecs.get::<&Position>(entity).ok()?.0;
            let (mut x, mut y) = (i64::from(position.x.to_bits()), i64::from(position.y.to_bits()));
            if team == Team::Red {
                (x, y) = (map - x, map - y);
            }
            Some((blueprint, x, y))
        })
        .collect();
    units.sort_unstable();
    units
}

/// Whether every unit of one team has a partner of the same kind on the other within
/// `tolerance` raw units. Fixed-point rounding is not symmetric under negation, so the two
/// sides end up a raw bit or so apart.
fn mirrored(blue: &[(u16, i64, i64)], red: &[(u16, i64, i64)], tolerance: i64) -> bool {
    let mut taken = vec![false; red.len()];
    blue.len() == red.len()
        && blue.iter().all(|unit| {
            let partner = red.iter().enumerate().position(|(index, other)| {
                !taken[index] && other.0 == unit.0 && (other.1 - unit.1).abs() <= tolerance && (other.2 - unit.2).abs() <= tolerance
            });
            partner.inspect(|&index| taken[index] = true).is_some()
        })
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn both_sides_play_mirror_images_through_the_opening() {
    // Before tie-breaks and staggered work stopped favoring a side, these drifted apart
    // between ticks 3,800 and 4,500.
    const TICKS: u32 = 5_000;
    for seed in 1..=3 {
        let mut world = skirmish(seed).world;
        let mut brains = [AiBrain::new(Team::Blue, seed, world.starts), AiBrain::new(Team::Red, seed, world.starts)];
        while world.current_tick < TICKS {
            advance_ai_vs_ai(&mut world, &mut brains);
            // A twentieth of a unit.
            assert!(
                mirrored(&units(&world, Team::Blue), &units(&world, Team::Red), 4096 / 20),
                "seed {seed}: the sides stopped mirroring each other at tick {}",
                world.current_tick
            );
        }
    }
}

/// Saddleback, a mirror image across both axes with its 1v1 starts facing each other over the
/// middle row, plays as mirror images through the opening too. Its starts once stood on the line
/// through the map's middle, where a unit's two nearest cells are equally near the center, and
/// the sides parted on the first tick; and the AI broke ties between vents and between raid
/// targets the same way for both sides, where a map listed them in fours rather than pairs.
/// Red won three matches in five. Stillwater and Scattered Isles, the same kind of image with
/// their 1v1 starts in opposite corners, are held to the same, and so is a generated map of
/// each kind.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn the_named_maps_sides_play_mirror_images_through_the_opening() {
    use std::sync::Arc;
    use sim_engine::sim::maps::{generate, saddleback, scattered_isles, stillwater, Choices, MapSettings};
    use sim_engine::sim::scenario::MapSource;
    const TICKS: u32 = 4_000;
    // And a generated map of each kind, which is as much its own image as the named ones.
    let generated = |kind: &str| {
        let choices = Choices { kind: kind.into(), seed: Some(5), ..Choices::default() };
        generate(&MapSettings::chosen(&choices).expect("settings the game offers"))
    };
    let maps = [
        ("saddleback", saddleback()),
        ("stillwater", stillwater()),
        ("scattered_isles", scattered_isles()),
        ("generated passes", generated("passes")),
        ("generated central lake", generated("central_lake")),
        ("generated islands", generated("islands")),
    ];
    for (name, map) in maps {
        let map = MapSource::Map(Arc::new(map));
        for seed in 1..=2 {
            let mut world = map.scenario(seed).world;
            let mut brains = [AiBrain::new(Team::Blue, seed, world.starts), AiBrain::new(Team::Red, seed, world.starts)];
            while world.current_tick < TICKS {
                advance_ai_vs_ai(&mut world, &mut brains);
                assert!(
                    mirrored(&units(&world, Team::Blue), &units(&world, Team::Red), 4096 / 20),
                    "{name}, seed {seed}: the sides stopped mirroring each other at tick {}",
                    world.current_tick
                );
            }
        }
    }
}

/// Well past the opening, into the first fights, matches on the maps from files still play as
/// mirror images. Each of these once parted, each for a fault of its own that favored a side:
/// Highland's on the pad an airfield's first aircraft took, the south-west for both sides;
/// Twin Passes' where a unit stood exactly on the map's middle row and both took the cell north
/// of it; a 2v2 on Stillwater's where each team sent its waves at the other's oldest structure,
/// one enemy player's for one team and the other's for the other; and a 3v3 on Saddleback's
/// where a shot at an aircraft, rounded toward negative infinity as it led its turn and as it
/// passed its target, hit on one side and missed on the other, and where each player's flow
/// fields waited behind the others' for a share of one budget.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn matches_on_the_maps_from_files_play_mirror_images_into_the_first_fights() {
    use std::sync::Arc;
    use sim_engine::ai::Difficulty;
    use sim_engine::ai_strategy::Strategy;
    use sim_engine::sim::components::PlayerId;
    use sim_engine::sim::maps::bundled;
    use sim_engine::sim::scenario::MapSource;
    const TICKS: u32 = 9_000;
    for (name, per_team, seed) in [("highland", 1, 2), ("twin_passes", 1, 4), ("stillwater", 2, 1), ("saddleback", 3, 1)] {
        let map = MapSource::Map(Arc::new(bundled(name).expect("a bundled map"))).teams(per_team).expect("starts for the match");
        let mut world = map.scenario(seed).world;
        let mut brains: Vec<AiBrain> = (0..world.players.len()).map(|player| AiBrain::for_player(&world, PlayerId(player as u8), seed, Difficulty::Normal, Strategy::balanced())).collect();
        while world.current_tick < TICKS && world.match_result.is_none() {
            advance_ai_vs_ai(&mut world, &mut brains);
            assert!(
                mirrored(&units(&world, Team::Blue), &units(&world, Team::Red), 4096 / 20),
                "{name} {per_team}v{per_team}, seed {seed}: the sides stopped mirroring each other at tick {}",
                world.current_tick
            );
        }
    }
}

/// In a 2v2 each player's opposite number is their image turned about the map's center, and the
/// two teams play mirror images through the opening as the two sides of a 1v1 do. They did not:
/// each unit's turn at work done every so many ticks, such as checking whether the way straight
/// to its goal is clear, was counted by team, so a player's units took turns one off their
/// opposite number's, and the teams parted within 700 ticks. The soak had Blue winning 49 of 60
/// 2v2s on Stillwater, and 38 of 60 on Saddleback. A 3v3 on Saddleback and a 4v4 on Scattered
/// Isles are held to the same.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn the_teams_of_a_team_match_play_mirror_images_through_the_opening() {
    use std::sync::Arc;
    use sim_engine::ai::Difficulty;
    use sim_engine::ai_strategy::Strategy;
    use sim_engine::sim::components::PlayerId;
    use sim_engine::sim::maps::{saddleback, scattered_isles, stillwater};
    use sim_engine::sim::scenario::MapSource;
    const TICKS: u32 = 4_000;
    for (name, map, per_team) in [
        ("saddleback", saddleback(), 2),
        ("stillwater", stillwater(), 2),
        ("scattered_isles", scattered_isles(), 2),
        ("saddleback", saddleback(), 3),
        ("scattered_isles", scattered_isles(), 4),
    ] {
        let map = MapSource::Map(Arc::new(map)).teams(per_team).expect("starts for the match");
        let mut world = map.scenario(1).world;
        assert_eq!(world.players.len(), per_team * 2);
        let mut brains: Vec<AiBrain> = (0..world.players.len()).map(|player| AiBrain::for_player(&world, PlayerId(player as u8), 1, Difficulty::Normal, Strategy::balanced())).collect();
        while world.current_tick < TICKS {
            advance_ai_vs_ai(&mut world, &mut brains);
            assert!(
                mirrored(&units(&world, Team::Blue), &units(&world, Team::Red), 4096 / 20),
                "{name} {per_team}v{per_team}: the teams stopped mirroring each other at tick {}",
                world.current_tick
            );
        }
    }
}
