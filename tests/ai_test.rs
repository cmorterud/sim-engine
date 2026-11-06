//! The scripted opponent: full matches, replays, and playing by the player's rules.

use sim_engine::ai::{play_match, replay_match, AiBrain};
use sim_engine::sim::commands::SimCommand;
use sim_engine::math::fp::{fp_int, Vec2Fp};
use sim_engine::sim::components::{Blueprinted, Position, Team, UnitStats};
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

/// An order with its unit ids left out and its places turned into Blue's side of the map.
fn mirrored_order(world: &SimWorld, team: Team, command: &SimCommand) -> String {
    let map = scenario::SKIRMISH_MAP_UNITS;
    let last = world.terrain.cells_per_side() as i32 - 1;
    let point = |point: Vec2Fp| {
        let (x, y) = (point.x.to_num::<i32>(), point.y.to_num::<i32>());
        if team == Team::Red { (map - x, map - y) } else { (x, y) }
    };
    match command {
        SimCommand::Build { units, blueprint, cell_x, cell_y, .. } => {
            let size = i32::from(world.blueprints.get(*blueprint).footprint);
            let (x, y) = (*cell_x as i32, *cell_y as i32);
            let cell = if team == Team::Red { (last - x - size + 1, last - y - size + 1) } else { (x, y) };
            format!("build {} x{} at {cell:?}", blueprint.0, units.len())
        }
        SimCommand::AttackMove { units, goal } => format!("attack-move x{} to {:?}", units.len(), point(*goal)),
        SimCommand::SetRally { point: rally, .. } => format!("rally {:?}", point(*rally)),
        SimCommand::QueueUnit { blueprint, count, .. } => format!("queue {} x{count}", blueprint.0),
        SimCommand::Assist { units, .. } => format!("assist x{}", units.len()),
        SimCommand::Reclaim { units, .. } => format!("reclaim x{}", units.len()),
        other => panic!("unexpected command {other:?}"),
    }
}

/// Each unit's blueprint and position in whole units, turned into Blue's side of the map.
fn mirrored_units(world: &SimWorld, team: Team) -> Vec<(u16, i32, i32)> {
    let map = scenario::SKIRMISH_MAP_UNITS;
    let mut units: Vec<_> = world
        .units
        .iter()
        .filter(|(_, entity)| world.ecs.get::<&UnitStats>(*entity).is_ok_and(|stats| stats.team == team))
        .map(|(_, entity)| {
            let mut position = world.ecs.get::<&Position>(*entity).unwrap().0.xy();
            if team == Team::Red {
                position = Vec2Fp::new(fp_int(map) - position.x, fp_int(map) - position.y);
            }
            (world.ecs.get::<&Blueprinted>(*entity).unwrap().0 .0, position.x.to_num::<i32>(), position.y.to_num::<i32>())
        })
        .collect();
    units.sort_unstable();
    units
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn ai_teams_play_mirror_images_until_they_meet() {
    // The map turns into itself about its center, and both AIs think on the same tick. Until
    // the armies meet, Red should play Blue's game turned around. Anything else gives one side
    // a better base, a faster start, or a shorter walk.
    //
    // Fixed-point rounding is not symmetric under negation, so the sides drift apart a little:
    // a unit a fraction of a unit off, or a builder in range a tick sooner.
    const TOLERANCE: i32 = 10;
    let matches = |red: &[(u16, i32, i32)], blue: &[(u16, i32, i32)]| {
        let mut partnered = vec![false; blue.len()];
        red.len() == blue.len()
            && red.iter().all(|unit| {
                let partner = blue.iter().enumerate().position(|(index, other)| {
                    !partnered[index] && other.0 == unit.0 && (other.1 - unit.1).abs() <= TOLERANCE && (other.2 - unit.2).abs() <= TOLERANCE
                });
                partner.inspect(|&index| partnered[index] = true).is_some()
            })
    };
    for seed in [1, 2, 3, 4, 5, 6, 7, 8] {
        let mut world = scenario::skirmish(seed).world;
        let mut brains = [AiBrain::new(Team::Blue, seed), AiBrain::new(Team::Red, seed)];
        let (mut blue_units, mut red_units) = (Vec::new(), Vec::new());
        // Five minutes, well before the first wave sets out.
        for tick in 0..3_000 {
            let blue = brains[0].think(&world);
            let red = brains[1].think(&world);
            let orders = |team: Team, commands: &[SimCommand]| commands.iter().map(|command| mirrored_order(&world, team, command)).collect::<Vec<_>>();
            assert_eq!(orders(Team::Red, &red), orders(Team::Blue, &blue), "seed {seed}: orders differ at tick {tick}");
            let mut commands = blue;
            commands.extend(red);
            world.advance_tick(&commands);
            blue_units.push(mirrored_units(&world, Team::Blue));
            red_units.push(mirrored_units(&world, Team::Red));
            // Equal-cost paths can bend differently on each side, so walks far from home drift.
            if (2..1_200).contains(&tick) {
                let then = tick - 1;
                let near_in_time = (then - 1..=then + 1).any(|blue| matches(&red_units[then], &blue_units[blue]));
                assert!(near_in_time, "seed {seed}: at tick {then}, Red has {:?}\nwhere Blue has {:?}", red_units[then], blue_units[then]);
            }
        }
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
