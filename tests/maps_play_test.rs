//! The named maps play (GAME_SPECIFICATION.md §17.3): AIs finish matches on them, using what
//! each map is for.

use std::sync::Arc;

use sim_engine::ai::{play_match_on, replay_match_on};
use sim_engine::sim::components::{Blueprinted, Position, UnitStats};
use sim_engine::sim::maps::{scattered_isles, stillwater};
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

/// Each team's land army: its combat units that go by land, and how many of them cannot cross
/// water.
fn land_army(world: &SimWorld) -> [(usize, usize); 2] {
    use sim_engine::sim::blueprints::UnitClass;
    use sim_engine::sim::components::MoveClass;
    let mut armies = [(0, 0); 2];
    for &(_, entity) in &world.units {
        let (Ok(stats), Ok(unit)) = (world.ecs.get::<&UnitStats>(entity), world.ecs.get::<&Blueprinted>(entity)) else { continue };
        if matches!(world.blueprints.get(unit.0).class, UnitClass::Bot | UnitClass::Tank | UnitClass::Artillery) {
            armies[stats.team.index()].0 += 1;
            armies[stats.team.index()].1 += usize::from(stats.move_class == MoveClass::Land);
        }
    }
    armies
}

/// Scattered Isles has no land route between its home islands, and the AIs play it so: each
/// keeps only a guard of land units that cannot cross, where it once massed two hundred tanks
/// that stood at home for the whole match, and goes at the other with ships, aircraft, and
/// hovercraft. The match is won, and replays from its orders.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn on_scattered_isles_ais_keep_a_guard_at_home_and_cross_by_sea_and_air() {
    let map = MapSource::Map(Arc::new(scattered_isles()));
    let (mut yards, mut ships, mut aircraft, mut crossing, mut stranded) = ([0; 2], [0; 2], [0; 2], [0; 2], [0; 2]);
    let record = play_match_on(&map, 1, 40_000, |world| {
        if world.current_tick % 600 != 0 {
            return;
        }
        let afloat = ["t1_frigate", "t1_attack_sub", "t2_destroyer", "t2_cruiser", "t3_battle"].map(|key| count(world, key));
        let flying = ["t1_bomber", "t1_interceptor", "t2_gunship", "t2_fighter_bomber", "t3_"].map(|key| count(world, key));
        let (now_yards, army) = (count(world, "naval_factory"), land_army(world));
        for team in 0..2 {
            yards[team] = yards[team].max(now_yards[team]);
            ships[team] = ships[team].max(afloat.iter().map(|kind| kind[team]).sum());
            aircraft[team] = aircraft[team].max(flying[0][team] + flying[1][team] + flying[2][team] + flying[3][team]);
            crossing[team] = crossing[team].max(army[team].0 - army[team].1);
            stranded[team] = stranded[team].max(army[team].1);
        }
    });
    assert!(yards.iter().all(|&built| built >= 1), "both sides build a naval yard: {yards:?}");
    assert!(ships.iter().all(|&afloat| afloat >= 4), "both sides put a fleet out: {ships:?}");
    assert!(aircraft.iter().all(|&flying| flying >= 4), "both sides fly: {aircraft:?}");
    assert!(crossing.iter().all(|&hovering| hovering >= 10), "both sides send hovercraft: {crossing:?}");
    // The guard is twelve; a squad for a transport and anti-air come on top of it.
    assert!(stranded.iter().all(|&at_home| at_home <= 24), "neither masses an army that cannot cross: {stranded:?}");
    assert!(matches!(record.result, Some(MatchResult::Winner(_))), "a side wins: {:?} at tick {}", record.result, record.ticks);
    assert_eq!(replay_match_on(&map, 1, &record.log), record.checksums, "it replays from its orders");
}

/// On Scattered Isles the AIs spend what they make on ships and aircraft, which are how the war
/// there is fought. Their naval yards and air factories once started nothing while stored energy
/// was under a quarter, and a half, of what it could hold, and two generators at a time never got
/// it there while T3 extractors raised mass income past 400 a second: by tick 12,000 each side
/// had 8 ships and 2 aircraft, with its mass store full and running over.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn on_scattered_isles_ais_spend_on_a_fleet_and_an_air_force() {
    use sim_engine::sim::components::MoveClass;
    let map = MapSource::Map(Arc::new(scattered_isles()));
    let (mut ships, mut aircraft) = ([0; 2], [0; 2]);
    play_match_on(&map, 1, 12_000, |world| {
        if world.current_tick != 12_000 {
            return;
        }
        for &(_, entity) in &world.units {
            let (Ok(stats), Ok(unit)) = (world.ecs.get::<&UnitStats>(entity), world.ecs.get::<&Blueprinted>(entity)) else { continue };
            let unit = world.blueprints.get(unit.0);
            if unit.is_structure() {
                continue;
            }
            match unit.move_class() {
                MoveClass::Naval | MoveClass::Submarine => ships[stats.team.index()] += 1,
                MoveClass::Air => aircraft[stats.team.index()] += 1,
                _ => {}
            }
        }
    });
    assert!(ships.iter().all(|&afloat| afloat >= 12), "both sides keep their yards busy: {ships:?} ships at tick 12,000");
    assert!(aircraft.iter().all(|&flying| flying >= 20), "both sides keep their air factories busy: {aircraft:?} aircraft at tick 12,000");
}

/// On Scattered Isles each team has an island: a 2v2 and a 4v4 put all of Blue's commanders on
/// the west one and all of Red's on the east.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn team_matches_on_scattered_isles_put_each_team_on_its_own_island() {
    for per_team in [2, 4] {
        let map = MapSource::Map(Arc::new(scattered_isles())).teams(per_team).expect("Scattered Isles holds eight players");
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
        assert_eq!(sides, [[per_team, 0], [0, per_team]], "{per_team} a side: Blue's commanders are in the west and Red's in the east");
    }
}
