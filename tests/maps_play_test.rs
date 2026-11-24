//! The named maps play (GAME_SPECIFICATION.md §17.3): AIs finish matches on them, using what
//! each map is for.

use std::sync::Arc;

use sim_engine::ai::{play_match_on, replay_match_on};
use sim_engine::sim::components::{Blueprinted, Position, UnitStats};
use sim_engine::sim::maps::stillwater;
use sim_engine::sim::scenario::MapSource;
use sim_engine::sim::world::{MatchResult, SimWorld};

/// How many of each team's units have a key starting with `prefix`.
fn count(world: &SimWorld, prefix: &str) -> [usize; 2] {
    let mut counts = [0; 2];
    for &(_, entity) in &world.units {
        let (Ok(stats), Ok(unit)) = (world.ecs.get::<&UnitStats>(entity), world.ecs.get::<&Blueprinted>(entity)) else { continue };
        if world.blueprints.get(unit.0).key.starts_with(prefix) {
            counts[stats.team.index()] += 1;
        }
    }
    counts
}

/// Stillwater, a lake ringed by land: a 1v1 of AIs is won, by armies that go round the lake,
/// both sides build a naval yard on it and put ships out, and the match replays from its orders.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn on_stillwater_ais_go_round_the_lake_and_take_to_it() {
    let map = MapSource::Map(Arc::new(stillwater()));
    let (mut yards, mut ships) = ([0; 2], [0; 2]);
    let record = play_match_on(&map, 1, 40_000, |world| {
        if world.current_tick % 600 != 0 {
            return;
        }
        let (now_yards, frigates, destroyers) = (count(world, "naval_factory"), count(world, "t1_frigate"), count(world, "t2_destroyer"));
        for team in 0..2 {
            yards[team] = yards[team].max(now_yards[team]);
            ships[team] = ships[team].max(frigates[team] + destroyers[team]);
        }
    });
    assert!(yards.iter().all(|&built| built >= 1), "both sides build a naval yard on the lake: {yards:?}");
    assert!(ships.iter().all(|&afloat| afloat >= 1), "both sides put ships on it: {ships:?}");
    assert!(matches!(record.result, Some(MatchResult::Winner(_))), "a side wins: {:?} at tick {}", record.result, record.ticks);
    assert_eq!(replay_match_on(&map, 1, &record.log), record.checksums, "it replays from its orders");
}

/// In a 2v2 on Stillwater each team holds the two corners of one shore, with the lake between
/// the teams.
#[test]
fn a_two_on_two_on_stillwater_puts_each_team_on_one_shore() {
    let map = MapSource::Map(Arc::new(stillwater())).teams(2).expect("Stillwater holds four players");
    let world = map.scenario(1).world;
    let middle = sim_engine::math::fp::fp_int(world.config.map_units / 2);
    let commander = world.blueprints.id("command_rig");
    let mut sides = [[0; 2]; 2];
    for &(_, entity) in &world.units {
        let (Ok(stats), Ok(unit), Ok(position)) = (world.ecs.get::<&UnitStats>(entity), world.ecs.get::<&Blueprinted>(entity), world.ecs.get::<&Position>(entity)) else { continue };
        if unit.0 == commander {
            sides[stats.team.index()][usize::from(position.0.x > middle)] += 1;
        }
    }
    assert_eq!(sides, [[2, 0], [0, 2]], "Blue's two commanders are in the west and Red's in the east");
}
