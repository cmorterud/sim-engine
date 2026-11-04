//! The scripted opponent: full matches, replays, and playing by the player's rules.

use sim_engine::ai::{play_match, replay_match, AiBrain};
use sim_engine::sim::commands::SimCommand;
use sim_engine::sim::components::{Blueprinted, Team, UnitStats};
use sim_engine::sim::scenario;
use sim_engine::sim::world::SimWorld;

const MATCH_LIMIT: u32 = 40_000;

fn count_owned(world: &SimWorld, team: Team, key: &str) -> usize {
    let wanted = world.blueprints.id(key);
    world
        .units
        .iter()
        .filter(|(_, entity)| {
            world.ecs.get::<&UnitStats>(*entity).is_ok_and(|stats| stats.team == team)
                && world.ecs.get::<&Blueprinted>(*entity).is_ok_and(|unit| unit.0 == wanted)
        })
        .count()
}

#[test]
fn ai_builds_an_economy_and_an_army() {
    let mut world = scenario::skirmish(11).world;
    let mut brain = AiBrain::new(Team::Blue, 11);
    // Five minutes, with nobody playing Red.
    for _ in 0..3_000 {
        let commands = brain.think(&world);
        world.advance_tick(&commands);
    }
    let count = |key: &str| count_owned(&world, Team::Blue, key);
    assert!(count("land_factory_t1") + count("land_factory_t2") >= 1, "no factory");
    assert!(count("mass_extractor") >= 4, "only {} extractors", count("mass_extractor"));
    assert!(count("power_generator") >= 2, "only {} generators", count("power_generator"));
    assert!(count("t1_engineer") >= 2, "only {} engineers", count("t1_engineer"));
    assert!(count("t1_light_bot") + count("t1_light_artillery") + count("t2_heavy_tank") >= 3, "no army");
    let income = (world.economies[0].mass_income * 10).to_num::<i64>();
    assert!(income >= 8, "mass income is only {income}/s");
}

#[test]
fn ai_commands_only_its_own_units() {
    let mut world = scenario::skirmish(12).world;
    let mut brains = [AiBrain::new(Team::Blue, 12), AiBrain::new(Team::Red, 12)];
    for _ in 0..4_000 {
        for brain in &mut brains {
            let commands = brain.think(&world);
            for command in &commands {
                let units: Vec<_> = match command {
                    SimCommand::Build { units, .. }
                    | SimCommand::Assist { units, .. }
                    | SimCommand::Reclaim { units, .. }
                    | SimCommand::AttackMove { units, .. } => units.clone(),
                    SimCommand::QueueUnit { factory, .. } | SimCommand::SetRally { factory, .. } => vec![*factory],
                    other => panic!("unexpected command {other:?}"),
                };
                for unit in units {
                    let entity = world.unit_entity(unit).expect("AI commands a living unit");
                    assert_eq!(world.ecs.get::<&UnitStats>(entity).unwrap().team, brain.team());
                }
            }
            world.advance_tick(&commands);
        }
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn ai_matches_finish_and_replay_exactly() {
    for seed in [1, 2, 3] {
        let record = play_match(seed, MATCH_LIMIT, |_| {});
        assert!(record.result.is_some(), "seed {seed} was unfinished after {} ticks", record.ticks);
        assert_eq!(replay_match(seed, &record.log), record.checksums, "seed {seed} did not replay exactly");
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn ai_cannot_see_through_fog() {
    // Red's start is known, but nothing that happens there may change Blue's orders
    // until Blue can see it. Two worlds differ only in a hidden Red structure, far from any Blue sight.
    let run = |extra_structure: bool| {
        let mut world = scenario::skirmish(21).world;
        if extra_structure {
            let generator = world.blueprints.id("power_generator");
            let cells = world.terrain.cells_per_side() as u32;
            world.spawn_finished_structure(generator, Team::Red, cells - 30, cells - 30).unwrap();
        }
        let mut brain = AiBrain::new(Team::Blue, 21);
        let mut orders = Vec::new();
        for _ in 0..1_200 {
            let commands = brain.think(&world);
            // Ids shift when anything extra spawns, so compare what was ordered, not which id got it.
            for command in &commands {
                match command {
                    SimCommand::Build { blueprint, cell_x, cell_y, .. } => orders.push((blueprint.0, *cell_x, *cell_y)),
                    SimCommand::QueueUnit { blueprint, .. } => orders.push((blueprint.0, 0, 0)),
                    _ => {}
                }
            }
            world.advance_tick(&commands);
        }
        orders
    };
    assert_eq!(run(false), run(true), "Blue's orders changed because of something it could not see");
}
