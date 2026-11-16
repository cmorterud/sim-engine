//! The balance harness (BALANCE.md, step 3): headless, deterministic duels between groups of
//! units, and the health multiplier at which two units of equal materiel come out even.
//!
//! The harness lives outside the simulation. It sets up worlds, gives orders, and reads the
//! results, so it may use floating point; the simulation itself stays in fixed point.

use std::sync::Arc;

use crate::math::fp::{fp_int, Fp, Vec2Fp};
use crate::sim::blueprints::{Blueprint, BlueprintDb, BlueprintId};
use crate::sim::commands::SimCommand;
use crate::sim::components::{Blueprinted, Health, SimId, Team};
use crate::sim::config::{SimConfig, TerrainStyle};
use crate::sim::world::SimWorld;

/// The duel ground: flat, 5 km across, so that every group starts in the open.
const MAP_UNITS: i32 = 5_120;
/// How far apart the two groups' centers start, beyond every direct-fire range.
const START_GAP: i32 = 1_400;
/// A duel that has not ended by now is scored where it stands: ten minutes.
const MAX_TICKS: u32 = 6_000;
/// Bisection steps for a break-even, over health multipliers from 1/16 to 16.
const BREAK_EVEN_STEPS: u32 = 12;

/// One side of a duel: so many of one unit.
#[derive(Copy, Clone, Debug)]
pub struct Side {
    pub unit: BlueprintId,
    pub count: u32,
}

/// How a duel ended.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DuelResult {
    /// Blue's share of its materiel left, weighted by health, less Red's: from -1, Red untouched
    /// and Blue gone, to 1.
    pub score: f64,
    /// From the first damage to the end, in seconds.
    pub seconds: f64,
    /// Whether one side was destroyed before the time ran out.
    pub finished: bool,
}

/// Plays one duel: Blue's group in the west and Red's in the east attack-move at each other.
/// Each side's units have `health[team]` times their health, so that a break-even can be found
/// smoothly; whole unit counts would make it jump.
pub fn duel(db: &Arc<BlueprintDb>, blue: Side, red: Side, health: [f64; 2]) -> DuelResult {
    let config = SimConfig { map_units: MAP_UNITS, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    world.blueprints = Arc::clone(db);
    let middle = MAP_UNITS / 2;
    let (west, east) = (middle - START_GAP / 2, middle + START_GAP / 2);
    let blue_ids = spawn_group(&mut world, blue, Team::Blue, west, -1);
    let red_ids = spawn_group(&mut world, red, Team::Red, east, 1);
    for (ids, scale) in [(&blue_ids, health[0]), (&red_ids, health[1])] {
        if scale == 1.0 {
            continue;
        }
        for &id in ids {
            let entity = world.unit_entity(id).expect("just spawned");
            let mut health = world.ecs.get::<&mut Health>(entity).expect("units have health");
            health.max = Fp::from_num(health.max.to_num::<f64>() * scale);
            health.current = health.max;
        }
    }
    let goal = |x: i32| Vec2Fp::new(fp_int(x), fp_int(middle));
    world.advance_tick(&[
        SimCommand::AttackMove { units: blue_ids.clone(), goal: goal(east + START_GAP / 2) },
        SimCommand::AttackMove { units: red_ids.clone(), goal: goal(west - START_GAP / 2) },
    ]);
    let full = |world: &SimWorld, ids: &[SimId]| ids.iter().map(|&id| worth(world, id)).sum::<f64>();
    let (blue_full, red_full) = (full(&world, &blue_ids), full(&world, &red_ids));
    let mut first_damage = None;
    let mut finished = false;
    while world.current_tick < MAX_TICKS {
        world.advance_tick(&[]);
        let (blue_now, red_now) = (full(&world, &blue_ids), full(&world, &red_ids));
        if first_damage.is_none() && (blue_now < blue_full || red_now < red_full) {
            first_damage = Some(world.current_tick);
        }
        if blue_now == 0.0 || red_now == 0.0 {
            finished = true;
            break;
        }
    }
    let score = full(&world, &blue_ids) / blue_full.max(f64::MIN_POSITIVE) - full(&world, &red_ids) / red_full.max(f64::MIN_POSITIVE);
    let seconds = f64::from(world.current_tick - first_damage.unwrap_or(world.current_tick)) / 10.0;
    DuelResult { score, seconds, finished }
}

/// A unit's materiel, times its share of health left; nothing once destroyed.
fn worth(world: &SimWorld, id: SimId) -> f64 {
    let Some(entity) = world.unit_entity(id) else { return 0.0 };
    let (Ok(health), Ok(unit)) = (world.ecs.get::<&Health>(entity), world.ecs.get::<&Blueprinted>(entity)) else { return 0.0 };
    let mass = world.blueprints.get(unit.0).mass.to_num::<f64>();
    mass * (health.current.to_num::<f64>() / health.max.to_num::<f64>().max(f64::MIN_POSITIVE)).clamp(0.0, 1.0)
}

/// Spawns a group in a block of up to five abreast, facing the other group, centered on `x`.
/// `back` is the side away from the enemy, -1 for the west.
fn spawn_group(world: &mut SimWorld, side: Side, team: Team, x: i32, back: i32) -> Vec<SimId> {
    let radius = world.blueprints.get(side.unit).radius.to_num::<i32>().max(4);
    let spacing = radius * 3;
    let columns = side.count.clamp(1, 5) as i32;
    let rows = (side.count as i32 + columns - 1) / columns;
    let middle = MAP_UNITS / 2;
    (0..side.count as i32)
        .map(|index| {
            let (row, column) = (index / columns, index % columns);
            let along = middle + (column * 2 - (columns - 1)) * spacing / 2;
            let depth = x + back * (row * spacing - (rows - 1) * spacing / 2);
            world.spawn_mobile(side.unit, team, Vec2Fp::new(fp_int(depth), fp_int(along)))
        })
        .collect()
}

/// A duel played both ways round, Blue as `a` and then as `b`, so that neither side's start
/// counts: `a`'s mean score, with `a`'s health times `a_health`.
pub fn mirrored(db: &Arc<BlueprintDb>, a: Side, b: Side, a_health: f64) -> DuelResult {
    let first = duel(db, a, b, [a_health, 1.0]);
    let second = duel(db, b, a, [1.0, a_health]);
    DuelResult { score: (first.score - second.score) / 2.0, seconds: (first.seconds + second.seconds) / 2.0, finished: first.finished && second.finished }
}

/// Where two units of equal materiel come out even.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BreakEven {
    /// The health multiplier on `a` at which the duel is even.
    pub health_factor: f64,
    /// `a`'s strength per materiel over `b`'s, by Lanchester's square law: above 1, `a` is the
    /// better buy. Equal groups with `a`'s health times `h` are even when `h` times `a`'s strength
    /// per unit, times its count squared, equals `b`'s, so this is the materiel ratio over √h.
    pub efficiency: f64,
    /// The even duel's length, in seconds.
    pub seconds: f64,
    /// Whether no multiplier from 1/16 to 16 makes it even, so `efficiency` is only a bound: `a`
    /// wins even at a sixteenth of its health, or cannot win at sixteen times.
    pub at_limit: bool,
}

/// The break-even between `a` and `b` with about `materiel` on each side.
pub fn break_even(db: &Arc<BlueprintDb>, a: BlueprintId, b: BlueprintId, materiel: f64) -> BreakEven {
    let count = |unit: BlueprintId| (materiel / db.get(unit).mass.to_num::<f64>()).round().max(1.0) as u32;
    let (side_a, side_b) = (Side { unit: a, count: count(a) }, Side { unit: b, count: count(b) });
    let materiel_of = |side: Side| f64::from(side.count) * db.get(side.unit).mass.to_num::<f64>();
    let (mut low, mut high) = (-4.0f64, 4.0f64);
    let mut seconds = 0.0;
    for _ in 0..BREAK_EVEN_STEPS {
        let middle = (low + high) / 2.0;
        let result = mirrored(db, side_a, side_b, middle.exp2());
        seconds = result.seconds;
        if result.score == 0.0 {
            (low, high) = (middle, middle);
            break;
        }
        if result.score > 0.0 {
            high = middle;
        } else {
            low = middle;
        }
    }
    let exponent = (low + high) / 2.0;
    let health_factor = exponent.exp2();
    let efficiency = (materiel_of(side_b) / materiel_of(side_a)) / health_factor.sqrt();
    BreakEven { health_factor, efficiency, seconds, at_limit: exponent.abs() > 3.99 }
}

/// A unit's own numbers, for reports.
pub fn describe(blueprint: &Blueprint) -> String {
    let weapon = blueprint.weapon.as_ref();
    let dps = weapon.map_or(0.0, |weapon| weapon.damage.to_num::<f64>() * 10.0 / f64::from(weapon.reload_ticks.max(1)));
    let range = weapon.map_or(0.0, |weapon| weapon.range.to_num::<f64>());
    let health = blueprint.health.to_num::<f64>();
    format!(
        "{}: {} materiel, {:.0} health, {:.1} damage a second, range {:.0}, speed {:.1}, sight {:.0}; time to kill a copy {:.1} s",
        blueprint.name,
        blueprint.mass.to_num::<f64>(),
        health,
        dps,
        range,
        blueprint.speed.to_num::<f64>(),
        blueprint.vision.to_num::<f64>(),
        if dps > 0.0 { health / dps } else { f64::INFINITY },
    )
}

/// The `--balance` report: today's data measured against the targets' questions, starting with
/// the anchor and T1 land.
pub fn report(db: &Arc<BlueprintDb>) {
    let anchor = db.id("t1_medium_tank");
    println!("{}", describe(db.get(anchor)));
    let one = mirrored(db, Side { unit: anchor, count: 1 }, Side { unit: anchor, count: 1 }, 1.0);
    println!("  one against one: {:.1} s from the first hit to the end, score {:+.3}", one.seconds, one.score);
    let ten = mirrored(db, Side { unit: anchor, count: 10 }, Side { unit: anchor, count: 10 }, 1.0);
    println!("  ten against ten: {:.1} s, score {:+.3}", ten.seconds, ten.score);
    println!();
    println!("T1 land against the anchor, about 600 materiel a side (efficiency per materiel, above 1 the better buy):");
    for key in ["t1_light_walker", "t1_land_scout", "t1_light_artillery", "t1_mobile_aa", "t1_medium_tank"] {
        let unit = db.id(key);
        let even = break_even(db, unit, anchor, 600.0);
        let bound = match (even.at_limit, even.health_factor < 1.0) {
            (false, _) => " ",
            (true, true) => ">",
            (true, false) => "<",
        };
        println!("  {:<22} efficiency {bound}{:>5.2}  (even at {:.2}x health, {:.1} s)", db.get(unit).name, even.efficiency, even.health_factor, even.seconds);
    }
}
