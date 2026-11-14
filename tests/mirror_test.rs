//! The skirmish map is a mirror image of itself, turned 180° about its center, and both sides
//! play the same AI, so the two sides should play mirror-image games. Anything in the simulation
//! that favors one side, such as breaking ties by row and column or by unit id, shows up here
//! as the two sides drifting apart. Such biases made the side with the lower ids win 58% of
//! AI-vs-AI matches.

use sim_engine::ai::{advance_ai_vs_ai, AiBrain};
use sim_engine::sim::components::{Blueprinted, Position, Team, UnitStats};
use sim_engine::sim::scenario::{skirmish, SKIRMISH_MAP_UNITS};
use sim_engine::sim::world::SimWorld;

/// A team's units, as blueprint and position in raw fixed-point units, Red's turned about the
/// center, sorted so that the two teams' lists can be compared.
fn units(world: &SimWorld, team: Team) -> Vec<(u16, i64, i64)> {
    let map = i64::from(SKIRMISH_MAP_UNITS) << 12;
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
