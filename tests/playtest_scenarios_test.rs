//! Playtest scenarios (README, Playtest scenarios), from data/scenarios.toml: every one names
//! units that exist, is laid out as the file describes, and plays out as a fight.

use std::collections::BTreeSet;

use sim_engine::sim::blueprints::BlueprintDb;
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::Team;
use sim_engine::sim::playtest_scenarios::{self, MAP_UNITS};
use sim_engine::sim::scenario::MapSource;

#[test]
fn every_scenario_names_real_units_and_is_laid_out_right() {
    let db = BlueprintDb::standard();
    let scenarios = playtest_scenarios::all();
    assert!(scenarios.len() >= 5, "only {} scenarios", scenarios.len());
    let ids: BTreeSet<&str> = scenarios.iter().map(|scenario| scenario.id.as_str()).collect();
    assert_eq!(ids.len(), scenarios.len(), "scenario ids repeat");
    let problems: Vec<String> = scenarios.iter().flat_map(|scenario| scenario.problems(&db)).collect();
    assert!(problems.is_empty(), "{problems:#?}");
    for scenario in &scenarios {
        assert!(scenario.id.chars().all(|letter| letter.is_ascii_lowercase() || letter.is_ascii_digit() || letter == '_'), "{} is not a plain id for a page address", scenario.id);
        assert!(!scenario.title.is_empty() && !scenario.shows.is_empty() && !scenario.to_try.is_empty(), "{} needs its text", scenario.id);
    }
}

/// Built, each has what the file lists, all on the ground, Blue west of Red, with fog of war as
/// asked, and Red's attackers ordered at Blue from the first tick.
#[test]
fn every_scenario_builds_what_the_file_lists() {
    let db = BlueprintDb::standard();
    for scenario in playtest_scenarios::all() {
        for fog in [true, false] {
            let built = MapSource::Scenario(std::sync::Arc::new(scenario.clone())).scenario(1).with_fog_of_war(fog);
            let world = &built.world;
            assert_eq!(world.config.fog_of_war, fog, "{}", scenario.id);
            let snapshot = world.snapshot();
            for (team, groups) in [(Team::Blue, &scenario.blue), (Team::Red, &scenario.red)] {
                for group in groups {
                    let id = db.id(&group.unit);
                    let wanted = groups.iter().filter(|other| other.unit == group.unit).map(|other| other.count).sum::<u32>() as usize;
                    let have = snapshot.units.iter().filter(|unit| unit.team == team && unit.blueprint == id.0).count();
                    assert_eq!(have, wanted, "{}: {team:?} has {have} {} of {wanted}", scenario.id, group.unit);
                }
            }
            for unit in &snapshot.units {
                assert!((0..MAP_UNITS << 12).contains(&unit.x) && (0..MAP_UNITS << 12).contains(&unit.y), "{}: a unit is off the map", scenario.id);
            }
            let attackers: usize = scenario.red.iter().filter(|group| group.attacks()).map(|group| group.count as usize).sum();
            let ordered: usize = built.opening_commands.iter().map(|command| if let SimCommand::AttackMove { units, .. } = command { units.len() } else { 0 }).sum();
            assert_eq!(ordered, attackers, "{}: Red's attackers are ordered in", scenario.id);
        }
    }
}

/// Played with Blue attack-moving at Red, as a player who does nothing clever would, every
/// scenario comes to a fight within three minutes: both sides lose materiel, or one is wiped out.
#[test]
fn every_scenario_comes_to_a_fight() {
    for scenario in playtest_scenarios::all() {
        let built = MapSource::Scenario(std::sync::Arc::new(scenario.clone())).scenario(1).with_fog_of_war(false);
        let mut world = built.world;
        let blue: Vec<_> = world.snapshot().units.iter().filter(|unit| unit.team == Team::Blue).map(|unit| unit.id).collect();
        let goal = world.starts[Team::Red.index()];
        let mut opening = built.opening_commands;
        opening.push(SimCommand::AttackMove { units: blue, goal });
        let worth = |world: &sim_engine::sim::world::SimWorld, team: Team| {
            world.snapshot().units.iter().filter(|unit| unit.team == team).map(|unit| world.blueprints.get(sim_engine::sim::blueprints::BlueprintId(unit.blueprint)).mass.to_num::<f64>() * f64::from(unit.health_permille)).sum::<f64>()
        };
        let start = [worth(&world, Team::Blue), worth(&world, Team::Red)];
        world.advance_tick(&opening);
        for _ in 0..1_800 {
            world.advance_tick(&[]);
        }
        let end = [worth(&world, Team::Blue), worth(&world, Team::Red)];
        let lost = [end[0] < start[0], end[1] < start[1]];
        let wiped = end[0] == 0.0 || end[1] == 0.0;
        assert!((lost[0] && lost[1]) || wiped, "{}: no fight in three minutes (Blue {:.0} of {:.0}, Red {:.0} of {:.0})", scenario.id, end[0], start[0], end[1], start[1]);
    }
}
