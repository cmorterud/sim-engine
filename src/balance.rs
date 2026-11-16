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
/// Starting layouts each measurement is averaged over: the gap and the groups' sideways offset
/// from each other vary, since a single layout's result turns on which unit dies a tick earlier.
const LAYOUTS: [(i32, i32); 4] = [(0, 0), (160, 37), (320, -71), (480, 113)];
/// A duel that has not ended by now is scored where it stands: ten minutes.
const MAX_TICKS: u32 = 6_000;
/// Bisection steps for a break-even, over multipliers from 1/16 to 16.
const BREAK_EVEN_STEPS: u32 = 12;

/// One side of a duel: so many of one unit, and how they are handled.
#[derive(Copy, Clone, Debug)]
pub struct Side {
    pub unit: BlueprintId,
    pub count: u32,
    pub tactics: Tactics,
}

/// How a side is handled in a duel.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Tactics {
    /// One attack-move at the enemy, as a player who does not micro.
    AttackMove,
    /// What a decent player does against massed artillery: start spread out, twice the usual
    /// spacing; close in on the enemy without stopping to fire, firing on the way; and pull back
    /// any unit under 30% health.
    Micro,
}

/// How far off the straight line a micro unit weaves as it closes, as the tangent of the angle.
/// None: weaving every second at 14 to 35 degrees slowed the approach more than it dodged
/// shells two seconds in flight, and cost about a fifth of what charging straight gained against
/// massed light artillery. Dodging that times itself to shells fired at the unit is for later.
const WEAVE: f64 = 0.0;
/// Below this share of its health, a micro unit pulls back.
const PULL_BACK_SHARE: f64 = 0.3;

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
    duel_in(db, blue, red, health, LAYOUTS[0])
}

/// A duel in one starting layout: `(extra gap, Red's sideways offset)`.
fn duel_in(db: &Arc<BlueprintDb>, blue: Side, red: Side, health: [f64; 2], layout: (i32, i32)) -> DuelResult {
    let config = SimConfig { map_units: MAP_UNITS, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    world.blueprints = Arc::clone(db);
    let middle = MAP_UNITS / 2;
    let gap = START_GAP + layout.0;
    let (west, east) = (middle - gap / 2, middle + gap / 2);
    let blue_ids = spawn_group(&mut world, blue, Team::Blue, west, -1, 0);
    let red_ids = spawn_group(&mut world, red, Team::Red, east, 1, layout.1);
    let homes = [west - gap / 2, east + gap / 2];
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
        SimCommand::AttackMove { units: blue_ids.clone(), goal: goal(east + gap / 2) },
        SimCommand::AttackMove { units: red_ids.clone(), goal: goal(west - gap / 2) },
    ]);
    let full = |world: &SimWorld, ids: &[SimId]| ids.iter().map(|&id| worth(world, id)).sum::<f64>();
    let (blue_full, red_full) = (full(&world, &blue_ids), full(&world, &red_ids));
    let mut first_damage = None;
    let mut finished = false;
    while world.current_tick < MAX_TICKS {
        let mut orders = Vec::new();
        if world.current_tick.is_multiple_of(10) {
            for (side, ids, enemies, home) in [(blue, &blue_ids, &red_ids, homes[0]), (red, &red_ids, &blue_ids, homes[1])] {
                if side.tactics == Tactics::Micro {
                    micro(&world, ids, enemies, home, &mut orders);
                }
            }
        }
        world.advance_tick(&orders);
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

/// This second's orders for a micro side (`Tactics::Micro`).
fn micro(world: &SimWorld, ids: &[SimId], enemies: &[SimId], home: i32, orders: &mut Vec<SimCommand>) {
    let positions: Vec<Vec2Fp> = enemies.iter().filter_map(|&id| world.unit_position(id)).map(|at| at.xy()).collect();
    if positions.is_empty() {
        return;
    }
    let count = positions.len() as i32;
    let center = |pick: fn(&Vec2Fp) -> i32| positions.iter().map(pick).sum::<i32>() / count;
    let enemy = Vec2Fp::new(fp_int(center(|at| at.x.to_num())), fp_int(center(|at| at.y.to_num())));
    let second = (world.current_tick / 10) as i32;
    for (index, &id) in ids.iter().enumerate() {
        let Some(entity) = world.unit_entity(id) else { continue };
        let Ok(health) = world.ecs.get::<&Health>(entity) else { continue };
        let Some(at) = world.unit_position(id).map(|at| at.xy()) else { continue };
        let share = health.current.to_num::<f64>() / health.max.to_num::<f64>().max(f64::MIN_POSITIVE);
        let goal = if share < PULL_BACK_SHARE {
            Vec2Fp::new(fp_int(home), at.y)
        } else {
            // Toward the enemy's middle, weaving off to one side, the other side next second.
            let ahead = (enemy.x - at.x).to_num::<f64>().abs().max(200.0);
            let side_step = if (second + index as i32) % 2 == 0 { ahead * WEAVE } else { -ahead * WEAVE };
            Vec2Fp::new(enemy.x, at.y + Fp::from_num(side_step))
        };
        orders.push(SimCommand::Move { units: vec![id], goal });
    }
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
fn spawn_group(world: &mut SimWorld, side: Side, team: Team, x: i32, back: i32, offset: i32) -> Vec<SimId> {
    let radius = world.blueprints.get(side.unit).radius.to_num::<i32>().max(4);
    let spacing = radius * if side.tactics == Tactics::Micro { 6 } else { 3 };
    let columns = side.count.clamp(1, 5) as i32;
    let rows = (side.count as i32 + columns - 1) / columns;
    let middle = MAP_UNITS / 2 + offset;
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
/// counts: `a`'s mean score, with `a`'s health times `h` and `b`'s divided by it. Scaling both
/// sides alike makes a break-even's answer the same whichever unit is `a`, even where splash
/// makes the fight stray from the square law.
pub fn mirrored(db: &Arc<BlueprintDb>, a: Side, b: Side, h: f64) -> DuelResult {
    let (mut score, mut seconds, mut finished) = (0.0, 0.0, true);
    for layout in LAYOUTS {
        let first = duel_in(db, a, b, [h, 1.0 / h], layout);
        let second = duel_in(db, b, a, [1.0 / h, h], layout);
        score += first.score - second.score;
        seconds += first.seconds + second.seconds;
        finished &= first.finished && second.finished;
    }
    let runs = (LAYOUTS.len() * 2) as f64;
    DuelResult { score: score / runs, seconds: seconds / runs, finished }
}

/// Where two units of equal materiel come out even.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BreakEven {
    /// The multiplier `h` at which the duel is even, with `a`'s health times `h` and `b`'s
    /// divided by it.
    pub health_factor: f64,
    /// `a`'s strength per materiel over `b`'s, by Lanchester's square law: above 1, `a` is the
    /// better buy. Strength goes with health, so the two sides' strengths differ by `h²` at the
    /// break-even, and this is the materiel ratio over `h`.
    pub efficiency: f64,
    /// The even duel's length, in seconds.
    pub seconds: f64,
    /// Whether no multiplier from 1/16 to 16 makes it even, so `efficiency` is only a bound.
    pub at_limit: bool,
}

/// The break-even between `a` and `b` with about `materiel` on each side.
/// `a`'s side is handled with `tactics`, `b`'s by attack-move.
pub fn break_even(db: &Arc<BlueprintDb>, a: BlueprintId, b: BlueprintId, materiel: f64, tactics: Tactics) -> BreakEven {
    let count = |unit: BlueprintId| (materiel / db.get(unit).mass.to_num::<f64>()).round().max(1.0) as u32;
    let side_a = Side { unit: a, count: count(a), tactics };
    let side_b = Side { unit: b, count: count(b), tactics: Tactics::AttackMove };
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
    let efficiency = (materiel_of(side_b) / materiel_of(side_a)) / health_factor;
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
    let side = |unit, count| Side { unit, count, tactics: Tactics::AttackMove };
    let one = mirrored(db, side(anchor, 1), side(anchor, 1), 1.0);
    println!("  one against one: {:.1} s from the first hit to the end, score {:+.3}", one.seconds, one.score);
    let ten = mirrored(db, side(anchor, 10), side(anchor, 10), 1.0);
    println!("  ten against ten: {:.1} s, score {:+.3}", ten.seconds, ten.score);
    println!();
    println!("T1 land against the anchor, about 600 materiel a side (efficiency per materiel, above 1 the better buy):");
    let show = |label: &str, even: BreakEven| {
        let bound = match (even.at_limit, even.health_factor < 1.0) {
            (false, _) => " ",
            (true, true) => ">",
            (true, false) => "<",
        };
        println!("  {label:<34} efficiency {bound}{:>5.2}  (even at {:.2}x health, {:.1} s)", even.efficiency, even.health_factor, even.seconds);
    };
    for key in ["t1_light_walker", "t1_land_scout", "t1_light_artillery", "t1_mobile_aa", "t1_medium_tank"] {
        let unit = db.id(key);
        show(&db.get(unit).name, break_even(db, unit, anchor, 600.0, Tactics::AttackMove));
    }
    println!();
    println!("What micro is worth (the anchor with micro against each, attack-moving; BALANCE.md):");
    for key in ["t1_medium_tank", "t1_light_artillery"] {
        let unit = db.id(key);
        let plain = break_even(db, anchor, unit, 600.0, Tactics::AttackMove);
        let micro = break_even(db, anchor, unit, 600.0, Tactics::Micro);
        show(&format!("against {}, attack-moving", db.get(unit).name), plain);
        show(&format!("against {}, with micro", db.get(unit).name), micro);
        println!("  {:<34} micro gain {:+.0}%", "", (micro.efficiency / plain.efficiency - 1.0) * 100.0);
    }
}
