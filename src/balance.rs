//! The balance harness (BALANCE.md, step 3): headless, deterministic duels between groups of
//! units, and the health multiplier at which two units of equal materiel come out even.
//!
//! The harness lives outside the simulation. It sets up worlds, gives orders, and reads the
//! results, so it may use floating point; the simulation itself stays in fixed point.

use std::sync::Arc;

use crate::math::fp::{fp_int, Fp, Vec2Fp};
use crate::sim::blueprints::{Blueprint, BlueprintDb, BlueprintId, TICKS_PER_SECOND};
use crate::sim::commands::SimCommand;
use crate::sim::blueprints::Res;
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
    /// No orders: it stays where it is and fights what comes, as a commander caught in its base.
    Hold,
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
    duel_in(db, &[blue], &[red], health, LAYOUTS[0])
}

/// A duel in one starting layout: `(extra gap, Red's sideways offset)`. Each team may be several
/// groups, an army with its escort, each in its own block behind the one before.
fn duel_in(db: &Arc<BlueprintDb>, blue: &[Side], red: &[Side], health: [f64; 2], layout: (i32, i32)) -> DuelResult {
    let config = SimConfig { map_units: MAP_UNITS, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    world.blueprints = Arc::clone(db);
    let middle = MAP_UNITS / 2;
    let gap = START_GAP + layout.0;
    let (west, east) = (middle - gap / 2, middle + gap / 2);
    let blue_groups = spawn_team(&mut world, blue, Team::Blue, west, -1, 0);
    let red_groups = spawn_team(&mut world, red, Team::Red, east, 1, layout.1);
    let all = |groups: &[(Side, Vec<SimId>)]| groups.iter().flat_map(|(_, ids)| ids.iter().copied()).collect::<Vec<SimId>>();
    let (blue_ids, red_ids) = (all(&blue_groups), all(&red_groups));
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
    let moving = |groups: &[(Side, Vec<SimId>)]| groups.iter().filter(|(side, _)| side.tactics != Tactics::Hold).flat_map(|(_, ids)| ids.iter().copied()).collect::<Vec<SimId>>();
    world.advance_tick(&[
        SimCommand::AttackMove { units: moving(&blue_groups), goal: goal(east + gap / 2) },
        SimCommand::AttackMove { units: moving(&red_groups), goal: goal(west - gap / 2) },
    ]);
    let full = |world: &SimWorld, ids: &[SimId]| ids.iter().map(|&id| worth(world, id)).sum::<f64>();
    let (blue_full, red_full) = (full(&world, &blue_ids), full(&world, &red_ids));
    let mut first_damage = None;
    let mut finished = false;
    while world.current_tick < MAX_TICKS {
        let mut orders = Vec::new();
        if world.current_tick.is_multiple_of(10) {
            for (groups, enemies, home) in [(&blue_groups, &red_ids, homes[0]), (&red_groups, &blue_ids, homes[1])] {
                for (side, ids) in groups {
                    if side.tactics == Tactics::Micro {
                        micro(&world, ids, enemies, home, &mut orders);
                    }
                    reengage_aircraft(&world, ids, enemies, &mut orders);
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

/// Aircraft with nothing to do are sent at what is left of the enemy, as a player would: an
/// attack-move ends at its goal, and aircraft that flew past each other there sat out of range,
/// so that air fights never finished. Ground units, which meet on the way, are left alone.
fn reengage_aircraft(world: &SimWorld, ids: &[SimId], enemies: &[SimId], orders: &mut Vec<SimCommand>) {
    let positions: Vec<Vec2Fp> = enemies.iter().filter_map(|&id| world.unit_position(id)).map(|at| at.xy()).collect();
    if positions.is_empty() {
        return;
    }
    let idle: Vec<SimId> = ids
        .iter()
        .copied()
        .filter(|&id| {
            world.unit_entity(id).is_some_and(|entity| {
                world.ecs.get::<&crate::sim::components::Flight>(entity).is_ok()
                    && world.ecs.get::<&crate::sim::components::Order>(entity).is_ok_and(|order| order.kind == crate::sim::components::OrderKind::Idle)
            })
        })
        .collect();
    if idle.is_empty() {
        return;
    }
    let count = positions.len() as i32;
    let x = positions.iter().map(|at| at.x.to_num::<i32>()).sum::<i32>() / count;
    let y = positions.iter().map(|at| at.y.to_num::<i32>()).sum::<i32>() / count;
    orders.push(SimCommand::AttackMove { units: idle, goal: Vec2Fp::new(fp_int(x), fp_int(y)) });
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

/// What a unit costs in a fight at T1, in the materiel of a land unit of the same price
/// (`cost_at`).
pub fn cost(unit: &Blueprint) -> f64 {
    cost_at(unit, 1)
}

/// What a unit costs in a fight at this tier, in the materiel of a land unit of the same price:
/// its mass and energy together, energy at the tier's rate (`[economy] energy_per_materiel`),
/// measured so that a land unit, which spends the anchor's energy for each materiel, costs
/// exactly its mass at every tier. Aircraft and many structures spend far more energy for their
/// mass, which counts, for much more at T1 than later, when energy is cheap.
pub fn cost_at(unit: &Blueprint, tier: u8) -> f64 {
    // Read once: duels price every unit on every tick.
    static RATES: std::sync::OnceLock<Vec<(f64, f64)>> = std::sync::OnceLock::new();
    let rates = RATES.get_or_init(|| {
        let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
        let anchor = Anchor::from_targets();
        let by_tier = table["economy"]["energy_per_materiel"].as_array().expect("[economy] energy_per_materiel is a rate for each tier");
        by_tier
            .iter()
            .map(|rate| {
                let rate = rate.as_float().or_else(|| rate.as_integer().map(|rate| rate as f64)).expect("a rate");
                (rate, 1.0 + anchor.energy / anchor.materiel / rate)
            })
            .collect()
    });
    let (rate, land_share) = rates[usize::from(tier.clamp(1, 3)) - 1];
    (unit.mass.to_num::<f64>() + unit.energy.to_num::<f64>() / rate) / land_share
}

/// A unit's cost, times its share of health left; nothing once destroyed.
fn worth(world: &SimWorld, id: SimId) -> f64 {
    let Some(entity) = world.unit_entity(id) else { return 0.0 };
    let (Ok(health), Ok(unit)) = (world.ecs.get::<&Health>(entity), world.ecs.get::<&Blueprinted>(entity)) else { return 0.0 };
    let price = cost(world.blueprints.get(unit.0));
    price * (health.current.to_num::<f64>() / health.max.to_num::<f64>().max(f64::MIN_POSITIVE)).clamp(0.0, 1.0)
}

/// Spawns a team's groups, the first centered on `x` and each later one in a block behind it.
fn spawn_team(world: &mut SimWorld, sides: &[Side], team: Team, x: i32, back: i32, offset: i32) -> Vec<(Side, Vec<SimId>)> {
    let mut groups = Vec::new();
    let mut center = x;
    for (index, &side) in sides.iter().enumerate() {
        if index > 0 {
            let (before, _) = group_shape(world, sides[index - 1]);
            let (now, spacing) = group_shape(world, side);
            center += back * ((before + now) / 2 + spacing);
        }
        groups.push((side, spawn_group(world, side, team, center, back, offset)));
    }
    groups
}

/// A group's depth from its first row to its last, and the spacing between its units.
fn group_shape(world: &SimWorld, side: Side) -> (i32, i32) {
    let radius = world.blueprints.get(side.unit).radius.to_num::<i32>().max(4);
    let spacing = radius * if side.tactics == Tactics::Micro { 6 } else { 3 };
    let columns = side.count.clamp(1, 5) as i32;
    let rows = (side.count as i32 + columns - 1) / columns;
    ((rows - 1) * spacing, spacing)
}

/// Spawns a group in a block of up to five abreast, facing the other group, centered on `x`.
/// `back` is the side away from the enemy, -1 for the west.
fn spawn_group(world: &mut SimWorld, side: Side, team: Team, x: i32, back: i32, offset: i32) -> Vec<SimId> {
    let (_, spacing) = group_shape(world, side);
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

/// How a siege ended.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SiegeResult {
    /// From the first damage to the last structure's fall, or to the time limit, in seconds.
    pub seconds: f64,
    /// Whether every structure fell.
    pub broken: bool,
    /// The share of the attackers' materiel lost, weighted by health.
    pub losses: f64,
}

/// A defended position: Red's structures, finished and powered, by blueprint key and the cell of
/// their south-west corner. The T3 one is a heavy shield over three heavy point defenses.
pub type Position = &'static [(&'static str, u32, u32)];
/// The T2 one: a shield generator over three T2 point defenses.
pub const T2_POSITION: Position = &[("shield_generator", 127, 127), ("t2_point_defense", 124, 128), ("t2_point_defense", 132, 128), ("t2_point_defense", 128, 124)];
pub const T3_POSITION: Position = &[("t3_heavy_shield", 127, 127), ("t3_heavy_point_defense", 124, 128), ("t3_heavy_point_defense", 132, 128), ("t3_heavy_point_defense", 128, 124)];

/// The materiel a position costs.
pub fn position_materiel(db: &BlueprintDb, position: Position) -> f64 {
    position.iter().map(|&(key, _, _)| cost(db.get(db.id(key)))).sum()
}

/// A siege: `count` of `unit` for Blue start west of `position`, beyond its reach, and
/// attack-move onto it. Red has the energy to keep its shields up throughout.
pub fn siege(db: &Arc<BlueprintDb>, unit: BlueprintId, count: u32, position: Position) -> SiegeResult {
    let config = SimConfig { map_units: MAP_UNITS, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = SimWorld::new(config, 1);
    world.blueprints = Arc::clone(db);
    let red = &mut world.economies[Team::Red.index()];
    for value in [&mut red.base_energy_capacity, &mut red.energy_capacity, &mut red.energy] {
        *value = Res::from_num(100_000_000);
    }
    let structures: Vec<SimId> = position
        .iter()
        .map(|&(key, x, y)| world.spawn_finished_structure(db.id(key), Team::Red, x, y).expect("the position fits on the duel ground"))
        .collect();
    let middle = MAP_UNITS / 2;
    let reach = position.iter().filter_map(|&(key, _, _)| db.get(db.id(key)).weapon).map(|weapon| weapon.range.to_num::<i32>()).max().unwrap_or(0);
    let start = middle - reach.max(db.get(unit).weapon.map_or(0, |weapon| weapon.range.to_num::<i32>())) - 400;
    let attackers = spawn_group(&mut world, Side { unit, count, tactics: Tactics::AttackMove }, Team::Blue, start, -1, 0);
    world.advance_tick(&[SimCommand::AttackMove { units: attackers.clone(), goal: Vec2Fp::new(fp_int(middle), fp_int(middle)) }]);
    let worth_of = |world: &SimWorld, ids: &[SimId]| ids.iter().map(|&id| worth(world, id)).sum::<f64>();
    let full = worth_of(&world, &attackers);
    let standing = |world: &SimWorld| structures.iter().filter(|&&id| world.unit_entity(id).is_some()).count();
    let shield_health = |world: &SimWorld| world.snapshot().shields.iter().map(|shield| i64::from(shield.health_permille)).sum::<i64>();
    let (mut first_damage, mut broken) = (None, false);
    let (count_before, shields_before) = (standing(&world), shield_health(&world));
    while world.current_tick < MAX_TICKS {
        world.advance_tick(&[]);
        if first_damage.is_none() && world.current_tick.is_multiple_of(10) && (standing(&world) < count_before || shield_health(&world) < shields_before) {
            first_damage = Some(world.current_tick);
        }
        if standing(&world) == 0 {
            broken = true;
            break;
        }
    }
    let seconds = f64::from(world.current_tick - first_damage.unwrap_or(world.current_tick)) / 10.0;
    SiegeResult { seconds, broken, losses: 1.0 - worth_of(&world, &attackers) / full.max(f64::MIN_POSITIVE) }
}

/// A duel played both ways round, Blue as `a` and then as `b`, so that neither side's start
/// counts: `a`'s mean score, with `a`'s health times `h` and `b`'s divided by it. Scaling both
/// sides alike makes a break-even's answer the same whichever unit is `a`, even where splash
/// makes the fight stray from the square law.
pub fn mirrored(db: &Arc<BlueprintDb>, a: Side, b: Side, h: f64) -> DuelResult {
    mirrored_armies(db, &[a], &[b], h)
}

/// `mirrored`, between armies of several groups each.
pub fn mirrored_armies(db: &Arc<BlueprintDb>, a: &[Side], b: &[Side], h: f64) -> DuelResult {
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
    let count = |unit: BlueprintId| (materiel / cost(db.get(unit))).round().max(1.0) as u32;
    let side_a = Side { unit: a, count: count(a), tactics };
    let side_b = Side { unit: b, count: count(b), tactics: Tactics::AttackMove };
    let materiel_of = |side: Side| f64::from(side.count) * cost(db.get(side.unit));
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
        cost(blueprint).round(),
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

/// The balance targets (BALANCE.md, step 1).
const TARGETS: &str = include_str!("../data/balance_targets.toml");

/// The anchor's numbers, from the targets.
#[derive(Copy, Clone, Debug)]
pub struct Anchor {
    pub materiel: f64,
    pub energy: f64,
    pub build_seconds: f64,
    pub health: f64,
    pub dps: f64,
    pub range: f64,
    pub speed: f64,
    pub vision: f64,
    pub radius: f64,
    pub projectile_speed: f64,
}

impl Anchor {
    pub fn from_targets() -> Self {
        let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
        let anchor = table["anchor"].as_table().expect("the targets have an anchor");
        let number = |key: &str| anchor[key].as_float().or_else(|| anchor[key].as_integer().map(|value| value as f64)).unwrap_or_else(|| panic!("anchor.{key} is a number"));
        Self {
            materiel: number("materiel"),
            energy: number("energy"),
            build_seconds: number("build_seconds"),
            health: number("health"),
            dps: number("dps"),
            range: number("range"),
            speed: number("speed"),
            vision: number("vision"),
            radius: number("radius"),
            projectile_speed: number("projectile_speed"),
        }
    }

    pub fn time_to_kill(&self) -> f64 {
        self.health / self.dps
    }

    /// Strength per materiel, √(health × damage a second) / cost, by Lanchester's square law.
    pub fn efficiency(&self) -> f64 {
        (self.health * self.dps).sqrt() / self.materiel
    }
}

/// A role's shape, as multiples of the anchor's (BALANCE.md, step 2). With a cost and a strength
/// multiplier, it gives every number a unit needs: strength is the anchor's per materiel, times
/// the multiplier, times the cost, split into health and damage by the role's time to kill.
#[derive(Clone, Debug)]
pub struct Role {
    pub unit: &'static str,
    pub role: &'static str,
    pub cost: f64,
    pub time_to_kill: f64,
    pub range: f64,
    pub speed: f64,
    pub sight_over_range: f64,
    /// Seconds for a shell to reach full range.
    pub flight_seconds: f64,
    pub reload_seconds: f64,
    pub splash: f64,
    pub radius: f64,
    /// Artillery's least range, as a share of its range.
    pub min_range: Option<f64>,
    pub spread: f64,
    /// What the strength multiplier is solved for.
    pub target: Target,
    pub strength: f64,
}

/// What a role's strength multiplier is solved for, against a unit already priced.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Target {
    /// Keeps `strength`.
    Fixed,
    /// The mirrored duel score against this unit at equal materiel, both attack-moving: a cell of
    /// the counter matrix.
    Score(&'static str, f64),
    /// Strength per materiel against this unit, by break-even: a tier's efficiency.
    Efficiency(&'static str, f64),
    /// Seconds for an equal materiel of it to break this position (`[siege] seconds`).
    Siege(Position, f64),
    /// This unit's strength per materiel, as the unit data has it, times this: one unit priced
    /// as another with a premium or a discount, with no duel in it (`[missiles]`).
    Relative(&'static str, f64),
}

/// A number in data/balance_targets.toml.
pub fn target_number(section: &str, key: &str) -> f64 {
    let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    let value = &table[section][key];
    value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).unwrap_or_else(|| panic!("{section}.{key} is a number"))
}

/// A unit's strength per materiel, √(health × damage a second) / cost, as a multiple of the
/// anchor's: the multiplier `apply` gives a role.
pub fn strength_of(db: &BlueprintDb, anchor: &Anchor, key: &str) -> f64 {
    let unit = db.get(db.id(key));
    let weapon = unit.weapon.unwrap_or_else(|| panic!("{key} has a weapon"));
    let dps = weapon.damage.to_num::<f64>() * TICKS_PER_SECOND as f64 / f64::from(weapon.reload_ticks.max(1));
    (unit.health.to_num::<f64>() * dps).sqrt() / cost(unit) / anchor.efficiency()
}

/// T1 land, first pass. Costs set how many a materiel buys; strengths are solved by duels.
pub fn t1_land_roles() -> Vec<Role> {
    let base = Role {
        unit: "t1_medium_tank",
        role: "line",
        cost: 40.0,
        time_to_kill: 1.0,
        range: 1.0,
        speed: 1.0,
        sight_over_range: 1.1,
        flight_seconds: 0.55,
        reload_seconds: 1.0,
        splash: 0.0,
        radius: 9.0,
        min_range: None,
        spread: 0.0,
        target: Target::Fixed,
        strength: 1.0,
    };
    vec![
        base.clone(),
        // Fast and fragile, for engineers and extractors; line units beat it, keeping about half.
        Role { unit: "t1_light_walker", role: "raider", cost: 24.0, time_to_kill: 0.5, range: 0.6, speed: 1.8, sight_over_range: 1.6, flight_seconds: 0.5, reload_seconds: 0.5, radius: 7.0, target: Target::Score("t1_medium_tank", -0.55), ..base.clone() },
        // Outranges everything at T1 and punishes clumps; it beats line units that attack-move
        // into it, keeping about half, and loses to micro and to raiders.
        Role {
            unit: "t1_light_artillery",
            role: "artillery",
            cost: 48.0,
            time_to_kill: 1.15,
            range: 1.7,
            speed: 0.8,
            sight_over_range: 0.65,
            flight_seconds: 2.1,
            reload_seconds: 5.0,
            splash: 16.0,
            min_range: Some(0.5),
            spread: 20.0,
            target: Target::Score("t1_medium_tank", 0.5),
            ..base.clone()
        },
        // Sight, not a fighter: cheap, fast, and sees far.
        Role { unit: "t1_land_scout", role: "scout", cost: 12.0, time_to_kill: 0.4, range: 0.6, speed: 2.0, sight_over_range: 3.3, flight_seconds: 0.4, reload_seconds: 1.0, radius: 6.0, strength: 0.5, ..base },
    ]
}

/// Equal materiel for every T1 cost above: 12 anchors, 20 raiders, 10 artillery, 40 scouts.
const PRICING_MATERIEL: f64 = 480.0;

/// T2 land, against T1 land frozen (BALANCE.md, step 3). The tier's line unit costs three
/// anchors (`[tiers] cost_multiplier`), and wins by range and toughness, not by being a better
/// buy: it is priced to the tier efficiency, a touch above the anchor's per materiel. The rest
/// are priced against it, or against what they counter.
pub fn t2_land_roles() -> Vec<Role> {
    let line = Role {
        unit: "t2_heavy_tank",
        role: "line",
        cost: 120.0,
        // Fights last a little longer a tier up ([pace] fight_seconds_t2).
        time_to_kill: 1.15,
        range: 1.25,
        speed: 0.86,
        sight_over_range: 1.1,
        flight_seconds: 0.55,
        reload_seconds: 1.2,
        splash: 0.0,
        radius: 11.0,
        min_range: None,
        spread: 0.0,
        target: Target::Efficiency("t1_medium_tank", 1.07),
        strength: 1.07,
    };
    // Splash weapons reach about one formation spacing ([splash]): splash plus a T2 target's
    // radius, 11, is 1.1 times 2.5 radii.
    let splash = 1.75 * 11.0;
    vec![
        line.clone(),
        // A skirmisher with grenades: outranges the line unit by a quarter, keeps up with it, and
        // lobs shells that burst among a clump. It beats the line unit, keeping about half, and
        // loses to what closes on it.
        Role {
            unit: "t2_grenadier_walker",
            role: "skirmisher",
            cost: 144.0,
            time_to_kill: 1.0,
            // A fifth past the line unit's, after both are rounded to whole units.
            range: 1.25 * 1.21,
            sight_over_range: 1.05,
            flight_seconds: 1.2,
            reload_seconds: 2.0,
            splash,
            radius: 10.0,
            spread: 10.0,
            target: Target::Score("t2_heavy_tank", 0.45),
            ..line.clone()
        },
        // Tough and amphibious, it holds the front and crosses water. About even with the line
        // unit for its materiel, a little under, since crossing water is worth something.
        Role {
            unit: "t2_assault_hovercraft",
            role: "assault",
            cost: 240.0,
            time_to_kill: 2.0,
            speed: 1.0,
            sight_over_range: 0.95,
            reload_seconds: 1.6,
            splash: 10.0,
            radius: 13.0,
            spread: 4.0,
            target: Target::Score("t2_heavy_tank", -0.08),
            ..line.clone()
        },
        // Siege: cruise missiles from far beyond every gun, at whatever stands still. They neither
        // lead nor follow, so what keeps moving beats it, and missile defense shoots them down.
        // Priced as T1 artillery is, with a premium for both (`[missiles]`), rather than by a duel
        // against it, which had every change to T1 artillery move its price.
        Role {
            unit: "t2_mobile_missile_launcher",
            role: "artillery",
            cost: 180.0,
            time_to_kill: 1.15,
            range: 3.7,
            speed: 0.8,
            sight_over_range: 0.5,
            flight_seconds: 3.2,
            reload_seconds: 5.0,
            splash: 10.0,
            radius: 11.0,
            min_range: Some(0.2),
            spread: 0.0,
            target: Target::Relative("t1_light_artillery", target_number("missiles", "strength_over_artillery")),
            ..line
        },
    ]
}

/// Equal materiel for every T2 cost above, and T1's: 36 anchors, 30 light artillery, 12 heavy
/// tanks, 10 grenadiers, 6 hovercraft, 8 missile launchers.
const T2_PRICING_MATERIEL: f64 = 1_440.0;

/// T3 land, against T1 and T2 land frozen (BALANCE.md, step 3). As at T2, the tier's line unit
/// costs three of the last tier's and is priced to the tier efficiency against it; the heavy unit
/// and the artillery are priced against the line unit, as at T1.
pub fn t3_land_roles() -> Vec<Role> {
    let line = Role {
        unit: "t3_heavy_walker",
        role: "line",
        cost: 360.0,
        // Fights longer again a tier up ([pace] fight_seconds_t3).
        time_to_kill: 1.3,
        range: 1.5,
        speed: 0.85,
        sight_over_range: 1.1,
        flight_seconds: 0.55,
        // A gatling: many small shots, so that no one shot decides a kill.
        reload_seconds: 0.3,
        splash: 0.0,
        radius: 13.0,
        min_range: None,
        spread: 0.0,
        target: Target::Efficiency("t2_heavy_tank", 1.07),
        strength: 1.07,
    };
    vec![
        line.clone(),
        // Heavy: slow and very tough, with one big cannon that reaches a little past the line
        // unit's gun. About even with the line unit for its materiel.
        Role {
            unit: "t3_siege_walker",
            role: "heavy",
            cost: 720.0,
            time_to_kill: 2.2,
            range: 1.5 * 1.1,
            speed: 0.6,
            sight_over_range: 0.9,
            flight_seconds: 0.6,
            reload_seconds: 4.0,
            splash: 12.0,
            radius: 15.0,
            target: Target::Score("t3_heavy_walker", 0.0),
            ..line.clone()
        },
        // No artillery role: since its blast went wide (2026-11-21), Mobile Heavy Artillery is set
        // by the hits it takes and deals ([kills] in data/balance_targets.toml), a glass cannon,
        // as the strategic bomber is. Priced by the walk-in duel, with a wide blast, it wiped out
        // Heavy Walkers at the lowest strength the search allows, so the duel could not price it.
    ]
}

/// Equal materiel for every T3 cost above: 10 heavy walkers, 5 siege walkers, 6 artillery, and
/// 30 heavy tanks or 90 anchors.
const T3_PRICING_MATERIEL: f64 = 3_600.0;

/// The unit data with a role's numbers at this strength.
pub fn apply(db: &BlueprintDb, anchor: &Anchor, role: &Role, strength: f64) -> BlueprintDb {
    let id = db.id(role.unit);
    let factory_power = db.get(db.id("land_factory_t1")).build_power.to_num::<f64>();
    let seconds_per_materiel = anchor.build_seconds / anchor.materiel;
    let total = anchor.efficiency() * strength * role.cost;
    let time_to_kill = anchor.time_to_kill() * role.time_to_kill;
    let (health, dps) = (total * time_to_kill.sqrt(), total / time_to_kill.sqrt());
    let range = anchor.range * role.range;
    db.with_unit(id, |unit| {
        unit.mass = crate::sim::blueprints::Res::from_num(role.cost);
        unit.energy = crate::sim::blueprints::Res::from_num(role.cost * anchor.energy / anchor.materiel);
        unit.build_time = crate::sim::blueprints::Res::from_num((role.cost * seconds_per_materiel * factory_power).round());
        unit.health = Fp::from_num(health.round());
        unit.speed = Fp::from_num((anchor.speed * role.speed * 10.0).round() / 10.0);
        unit.radius = Fp::from_num(role.radius);
        unit.vision = Fp::from_num((range * role.sight_over_range / 5.0).round() * 5.0);
        if let Some(weapon) = unit.weapon.as_mut() {
            weapon.reload_ticks = (role.reload_seconds * 10.0).round() as u32;
            weapon.damage = Fp::from_num((dps * role.reload_seconds * 10.0).round() / 10.0);
            weapon.range = Fp::from_num(range.round());
            weapon.projectile_speed = Fp::from_num((range / (role.flight_seconds * 10.0)).round());
            weapon.splash_radius = Fp::from_num(role.splash);
            weapon.artillery = role.min_range.is_some();
            weapon.min_range = Fp::from_num((range * role.min_range.unwrap_or(0.0)).round());
            weapon.spread = Fp::from_num(role.spread);
        }
    })
}

/// The strength multiplier from 1/8 to 8 whose measurement came nearest `target`, by bisection:
/// `measure` rises with strength if `rising`, and falls otherwise. It rises or falls in steps,
/// where a shot just becomes enough to kill in one fewer hit, so this keeps the nearest measured,
/// not the middle of the last step.
fn nearest_strength(target: f64, rising: bool, mut measure: impl FnMut(f64) -> f64) -> f64 {
    let (mut low, mut high) = (-3.0f64, 3.0f64);
    let mut nearest = (f64::INFINITY, 1.0);
    for _ in 0..12 {
        let middle = (low + high) / 2.0;
        let measured = measure(middle.exp2());
        if (measured - target).abs() < nearest.0 {
            nearest = ((measured - target).abs(), middle.exp2());
        }
        if (measured > target) == rising {
            high = middle;
        } else {
            low = middle;
        }
    }
    nearest.1
}

/// Solves a role's strength multiplier for its target, at about `materiel` a side. A duel score
/// rises with strength, so bisection over multipliers from 1/8 to 8 finds it; but it rises in
/// steps, where a shot just becomes enough to kill in one fewer hit, so it keeps the strength
/// whose measured score came nearest the target, not the middle of the last step. Strength per
/// materiel goes with the multiplier, nearly in proportion, so a few corrections settle it.
pub fn price(db: &BlueprintDb, anchor: &Anchor, role: &Role, materiel: f64) -> f64 {
    let count = |unit: &Blueprint| (materiel / cost(unit)).round().max(1.0) as u32;
    match role.target {
        Target::Fixed => role.strength,
        Target::Relative(of, factor) => strength_of(db, anchor, of) * factor,
        Target::Score(against, target) => nearest_strength(target, true, |strength| {
            let trial = Arc::new(apply(db, anchor, role, strength));
            let side = |id: BlueprintId| Side { unit: id, count: count(trial.get(id)), tactics: Tactics::AttackMove };
            mirrored(&trial, side(trial.id(role.unit)), side(trial.id(against)), 1.0).score
        }),
        // A siege shortens as the attackers' damage grows.
        Target::Siege(position, target) => nearest_strength(target, false, |strength| {
            let trial = Arc::new(apply(db, anchor, role, strength));
            let count = (position_materiel(&trial, position) / role.cost).round().max(1.0) as u32;
            siege(&trial, trial.id(role.unit), count, position).seconds
        }),
        Target::Efficiency(against, target) => {
            let mut strength = role.strength;
            for _ in 0..4 {
                let trial = Arc::new(apply(db, anchor, role, strength));
                let measured = break_even(&trial, trial.id(role.unit), trial.id(against), materiel, Tactics::AttackMove).efficiency;
                strength *= target / measured;
            }
            strength
        }
    }
}

/// The `--balance-derive t1` report: T1 land's first-pass numbers from the targets, with the
/// duels that priced them and the counter-matrix cells they should meet.
pub fn derive_t1_report() {
    let anchor = Anchor::from_targets();
    let roles = t1_land_roles();
    let mut db = apply(&BlueprintDb::standard(), &anchor, &roles[0], 1.0);
    let mut strengths = Vec::new();
    for role in &roles {
        let strength = price(&db, &anchor, role, PRICING_MATERIEL);
        db = apply(&db, &anchor, role, strength);
        strengths.push(strength);
    }
    let db = Arc::new(db);
    println!("T1 land, first pass from data/balance_targets.toml (anchor efficiency {:.3} a materiel):", anchor.efficiency());
    for (role, strength) in roles.iter().zip(&strengths) {
        let unit = db.get(db.id(role.unit));
        let weapon = unit.weapon.expect("T1 land units have a weapon");
        println!("  {:<10} strength {:.2}  {}", role.role, strength, describe(unit));
        println!(
            "             mass = {}, energy = {}, build_time = {}, health = {}, speed = \"{}\", radius = {}, vision = {}",
            unit.mass, unit.energy, unit.build_time, unit.health, unit.speed, unit.radius, unit.vision
        );
        println!(
            "             weapon = {{ range = {}, reload_ticks = {}, projectile_speed = {}, damage = {}, splash_radius = {}, spread = {}{}{} }}",
            weapon.range,
            weapon.reload_ticks,
            weapon.projectile_speed,
            weapon.damage,
            weapon.splash_radius,
            weapon.spread,
            if weapon.artillery { format!(", artillery = true, min_range = {}", weapon.min_range) } else { String::new() },
            if weapon.setup_ticks > 0 { format!(", setup_ticks = {}", weapon.setup_ticks) } else { String::new() },
        );
    }
    let side = |key: &str, tactics| {
        let role = roles.iter().find(|role| role.unit == key).expect("a T1 land role");
        Side { unit: db.id(key), count: (PRICING_MATERIEL / role.cost).round() as u32, tactics }
    };
    println!();
    println!("Counter-matrix cells, equal materiel ({PRICING_MATERIEL}), mirrored, score for the first:");
    let anchor_side = side("t1_medium_tank", Tactics::AttackMove);
    let line_vs = |enemy: &str| mirrored(&db, anchor_side, side(enemy, Tactics::AttackMove), 1.0);
    println!("  line against raider:                {:+.2} (line keeps 0.4-0.7)", line_vs("t1_light_walker").score);
    println!("  line against artillery, attack-move: {:+.2} (artillery keeps 0.4-0.7)", line_vs("t1_light_artillery").score);
    let micro = mirrored(&db, side("t1_medium_tank", Tactics::Micro), side("t1_light_artillery", Tactics::AttackMove), 1.0);
    println!("  line against artillery, with micro:  {:+.2} (line keeps at least 0.3)", micro.score);
    let raiders_micro = mirrored(&db, side("t1_light_walker", Tactics::Micro), side("t1_light_artillery", Tactics::AttackMove), 1.0);
    let raiders_plain = mirrored(&db, side("t1_light_walker", Tactics::AttackMove), side("t1_light_artillery", Tactics::AttackMove), 1.0);
    println!("  massed raiders against artillery:   {:+.2} attack-moving, {:+.2} with micro (artillery beats raiders; micro helps)", raiders_plain.score, raiders_micro.score);
    let raid = mirrored(
        &db,
        Side { unit: db.id("t1_light_walker"), count: 4, tactics: Tactics::AttackMove },
        Side { unit: db.id("t1_light_artillery"), count: 2, tactics: Tactics::AttackMove },
        1.0,
    );
    println!("  4 raiders charging 2 artillery:     {:+.2} (artillery may win: raiders are no counter of it)", raid.score);
    let ten = mirrored(&db, Side { count: 10, ..anchor_side }, Side { count: 10, ..anchor_side }, 1.0);
    println!("  ten anchors against ten:            {:.1} s (targets: 25-45 s)", ten.seconds);
}

/// Prints a role's unit as derived: its strength, its numbers, and the lines for the unit data.
fn print_derived(db: &BlueprintDb, role: &Role, strength: f64) {
    let unit = db.get(db.id(role.unit));
    println!("  {:<10} strength {:.2}  {}", role.role, strength, describe(unit));
    println!(
        "             mass = {}, energy = {}, build_time = {}, health = {}, speed = \"{}\", radius = {}, vision = {}",
        unit.mass, unit.energy, unit.build_time, unit.health, unit.speed, unit.radius, unit.vision
    );
    if let Some(weapon) = unit.weapon {
        println!(
            "             weapon = {{ range = {}, reload_ticks = {}, projectile_speed = {}, damage = {}, splash_radius = {}, spread = {}{}{} }}",
            weapon.range,
            weapon.reload_ticks,
            weapon.projectile_speed,
            weapon.damage,
            weapon.splash_radius,
            weapon.spread,
            if weapon.artillery { format!(", artillery = true, min_range = {}", weapon.min_range) } else { String::new() },
            if weapon.setup_ticks > 0 { format!(", setup_ticks = {}", weapon.setup_ticks) } else { String::new() },
        );
    }
}

/// A tier's numbers from the targets, its roles priced in order at about `materiel` a side
/// against the tiers below as the unit data has them (frozen), and the unit data with them.
pub fn derive(base: &BlueprintDb, roles: Vec<Role>, materiel: f64) -> (BlueprintDb, Vec<(Role, f64)>) {
    let anchor = Anchor::from_targets();
    let mut db = base.clone();
    let mut priced = Vec::new();
    for role in roles {
        let strength = price(&db, &anchor, &role, materiel);
        db = apply(&db, &anchor, &role, strength);
        priced.push((role, strength));
    }
    (db, priced)
}

/// Each priced unit's strength per materiel against each of `enemies`: above 1, the unit is the
/// better buy.
fn efficiency_table(db: &Arc<BlueprintDb>, priced: &[(Role, f64)], enemies: &[&str], materiel: f64) {
    for (role, _) in priced {
        let cells: Vec<String> = enemies
            .iter()
            .map(|&enemy| {
                let even = break_even(db, db.id(role.unit), db.id(enemy), materiel, Tactics::AttackMove);
                format!("{} {}{:.2}", db.get(db.id(enemy)).name, if even.at_limit { "~" } else { "" }, even.efficiency)
            })
            .collect();
        println!("  {:<24} {}", db.get(db.id(role.unit)).name, cells.join(", "));
    }
}

/// The `--balance-derive t2` report: T2 land's numbers from the targets, priced against T1 land
/// frozen, with the cells of the counter matrix they should meet, and every T2 unit's strength
/// per materiel against every T1 fighter.
pub fn derive_t2_report() {
    let (db, priced) = derive(&BlueprintDb::standard(), t2_land_roles(), T2_PRICING_MATERIEL);
    let db = Arc::new(db);
    println!("T2 land from data/balance_targets.toml, against T1 land frozen:");
    for (role, strength) in &priced {
        print_derived(&db, role, *strength);
    }
    let side = |key: &str, tactics| {
        let id = db.id(key);
        Side { unit: id, count: (T2_PRICING_MATERIEL / cost(db.get(id))).round().max(1.0) as u32, tactics }
    };
    let plain = |a: &str, b: &str| mirrored(&db, side(a, Tactics::AttackMove), side(b, Tactics::AttackMove), 1.0);
    println!();
    println!("Counter-matrix cells, equal materiel ({T2_PRICING_MATERIEL}), mirrored, score for the first:");
    let tier = break_even(&db, db.id("t2_heavy_tank"), db.id("t1_medium_tank"), T2_PRICING_MATERIEL, Tactics::AttackMove);
    println!("  T2 line against T1 line, efficiency: {:.2} (targets: 1.0-1.15)", tier.efficiency);
    println!("  T2 line against T1 line, score:      {:+.2}", plain("t2_heavy_tank", "t1_medium_tank").score);
    println!("  skirmisher against line:            {:+.2} (skirmisher keeps 0.4-0.7)", plain("t2_grenadier_walker", "t2_heavy_tank").score);
    println!("  assault against line:               {:+.2} (even, within 0.15)", plain("t2_assault_hovercraft", "t2_heavy_tank").score);
    println!("  assault against skirmisher:         {:+.2} (about even, assault ahead)", plain("t2_assault_hovercraft", "t2_grenadier_walker").score);
    let launcher = db.get(db.id("t2_mobile_missile_launcher"));
    let premium = strength_of(&db, &Anchor::from_targets(), "t2_mobile_missile_launcher") / strength_of(&db, &Anchor::from_targets(), "t1_light_artillery");
    println!("  siege's strength over T1 artillery's: {premium:.2} ([missiles] strength_over_artillery)");
    let count = (position_materiel(&db, T2_POSITION) / cost(launcher)).round() as u32;
    let broke = siege(&db, launcher.id, count, T2_POSITION);
    println!(
        "  {count} siege against a shield over three T2 point defenses ({:.0} materiel): {} in {:.0} s, losing {:.0}% (targets: within {:.0} s, nothing lost)",
        position_materiel(&db, T2_POSITION),
        if broke.broken { "broken" } else { "not broken" },
        broke.seconds,
        broke.losses * 100.0,
        target_number("missiles", "siege_seconds_max"),
    );
    println!("  T1 artillery against siege:         {:+.2} (artillery is the better unit)", plain("t1_light_artillery", "t2_mobile_missile_launcher").score);
    println!("  line against siege, attack-move:    {:+.2} (what keeps moving wins)", plain("t2_heavy_tank", "t2_mobile_missile_launcher").score);
    let raid = mirrored(
        &db,
        Side { unit: db.id("t1_light_walker"), count: 6, tactics: Tactics::AttackMove },
        Side { unit: db.id("t2_mobile_missile_launcher"), count: 1, tactics: Tactics::AttackMove },
        1.0,
    );
    println!("  a raid, 6 T1 raiders against 1 siege: {:+.2} (raiders win)", raid.score);
    let grenadier_raid = mirrored(
        &db,
        Side { unit: db.id("t1_light_walker"), count: 6, tactics: Tactics::AttackMove },
        Side { unit: db.id("t2_grenadier_walker"), count: 1, tactics: Tactics::AttackMove },
        1.0,
    );
    println!("  a raid, 6 T1 raiders against 1 skirmisher: {:+.2} (its splash shreds a clump of raiders)", grenadier_raid.score);
    let spread_raiders = mirrored(&db, side("t1_light_walker", Tactics::Micro), side("t2_grenadier_walker", Tactics::AttackMove), 1.0);
    println!("  massed T1 raiders against skirmishers, with micro: {:+.2}", spread_raiders.score);
    let spread_line = mirrored(&db, side("t2_heavy_tank", Tactics::Micro), side("t2_grenadier_walker", Tactics::AttackMove), 1.0);
    println!("  line against skirmisher, with micro: {:+.2} (line keeps at least 0.3: a micro cell)", spread_line.score);
    println!("  assault against siege, attack-move: {:+.2}", plain("t2_assault_hovercraft", "t2_mobile_missile_launcher").score);
    println!("  skirmisher against siege:           {:+.2}", plain("t2_grenadier_walker", "t2_mobile_missile_launcher").score);
    let line = side("t2_heavy_tank", Tactics::AttackMove);
    let ten = mirrored(&db, Side { count: 10, ..line }, Side { count: 10, ..line }, 1.0);
    println!("  ten T2 line against ten:            {:.1} s (targets: 30-50 s)", ten.seconds);
    println!();
    println!("Strength per materiel against T1 land (above 1, the T2 unit is the better buy):");
    efficiency_table(&db, &priced, &["t1_medium_tank", "t1_light_walker", "t1_light_artillery"], T2_PRICING_MATERIEL);
}

/// The `--balance-derive t3` report: T3 land's numbers from the targets, priced against T1 and
/// T2 land frozen, with the cells of the counter matrix they should meet, and every T3 unit's
/// strength per materiel against the tiers below.
pub fn derive_t3_report() {
    let (db, priced) = derive(&BlueprintDb::standard(), t3_land_roles(), T3_PRICING_MATERIEL);
    let db = Arc::new(db);
    println!("T3 land from data/balance_targets.toml, against T1 and T2 land frozen:");
    for (role, strength) in &priced {
        print_derived(&db, role, *strength);
    }
    let side = |key: &str, tactics| {
        let id = db.id(key);
        Side { unit: id, count: (T3_PRICING_MATERIEL / cost(db.get(id))).round().max(1.0) as u32, tactics }
    };
    let cell = |a: &str, a_tactics, b: &str| mirrored(&db, side(a, a_tactics), side(b, Tactics::AttackMove), 1.0).score;
    println!();
    println!("Counter-matrix cells, equal materiel ({T3_PRICING_MATERIEL}), mirrored, score for the first:");
    let tier = break_even(&db, db.id("t3_heavy_walker"), db.id("t2_heavy_tank"), T3_PRICING_MATERIEL, Tactics::AttackMove);
    println!("  T3 line against T2 line, efficiency: {:.2} (targets: 1.0-1.15)", tier.efficiency);
    println!("  T3 line against T2 line, score:      {:+.2}", cell("t3_heavy_walker", Tactics::AttackMove, "t2_heavy_tank"));
    println!("  heavy against line:                 {:+.2} (even, within 0.15)", cell("t3_siege_walker", Tactics::AttackMove, "t3_heavy_walker"));
    let artillery = db.get(db.id("t3_mobile_artillery"));
    let defense = db.get(db.id("t3_heavy_point_defense")).weapon.expect("point defense has a gun").range.to_num::<f64>();
    println!("  artillery's reach over the heavy point defense's: {:.2} (at least 1.15)", artillery.weapon.expect("artillery has a gun").range.to_num::<f64>() / defense);
    let count = (position_materiel(&db, T3_POSITION) / cost(artillery)).round() as u32;
    let broke = siege(&db, db.id("t3_mobile_artillery"), count, T3_POSITION);
    println!(
        "  {count} artillery against a heavy shield over three heavy point defenses ({} materiel): {} in {:.0} s, losing {:.0}% (targets: within 60 s, nothing lost)",
        position_materiel(&db, T3_POSITION),
        if broke.broken { "broken" } else { "holding" },
        broke.seconds,
        broke.losses * 100.0
    );
    let walkers = mirrored(&db, side("t3_heavy_walker", Tactics::AttackMove), side("t3_mobile_artillery", Tactics::AttackMove), 1.0);
    println!("  line against artillery, attack-move: {:+.2} (artillery wins)", walkers.score);
    println!("  line against artillery, with micro:  {:+.2} (line wins)", cell("t3_heavy_walker", Tactics::Micro, "t3_mobile_artillery"));
    println!("  heavy against artillery, attack-move: {:+.2}", cell("t3_siege_walker", Tactics::AttackMove, "t3_mobile_artillery"));
    println!("  T2 assault against T3 artillery:     {:+.2}", cell("t2_assault_hovercraft", Tactics::AttackMove, "t3_mobile_artillery"));
    let raid = mirrored(
        &db,
        Side { unit: db.id("t1_light_walker"), count: 25, tactics: Tactics::AttackMove },
        Side { unit: db.id("t3_mobile_artillery"), count: 1, tactics: Tactics::AttackMove },
        1.0,
    );
    println!("  a raid, 25 T1 raiders against 1 artillery: {:+.2} (raiders win)", raid.score);
    let line = side("t3_heavy_walker", Tactics::AttackMove);
    let ten = mirrored(&db, Side { count: 10, ..line }, Side { count: 10, ..line }, 1.0);
    println!("  ten T3 line against ten:            {:.1} s (targets: 35-60 s)", ten.seconds);
    println!();
    println!("Strength per materiel against T2 and T1 land (above 1, the T3 unit is the better buy):");
    efficiency_table(
        &db,
        &priced,
        &["t2_heavy_tank", "t2_grenadier_walker", "t2_assault_hovercraft", "t2_mobile_missile_launcher", "t1_medium_tank", "t1_light_artillery"],
        T3_PRICING_MATERIEL,
    );
}


/// Seconds for `blue` to destroy `red`, which cannot shoot back, or the time limit.
pub fn strike_seconds(db: &Arc<BlueprintDb>, blue: Side, red: Side) -> f64 {
    mirrored(db, blue, red, 1.0).seconds
}

/// A `[low, high]` range in data/balance_targets.toml.
pub fn target_number_range(section: &str, key: &str) -> (f64, f64) {
    let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    let range = table[section][key].as_array().unwrap_or_else(|| panic!("{section}.{key} is a range"));
    let number = |value: &toml::Value| value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).expect("a number");
    (number(&range[0]), number(&range[1]))
}

/// A side of so many of `key` as `materiel` buys in a fight at T1, attack-moving.
pub fn bought(db: &BlueprintDb, key: &str, materiel: f64) -> Side {
    bought_at(db, key, materiel, 1)
}

/// A side of so many of `key` as `materiel` buys in a fight at this tier, attack-moving.
pub fn bought_at(db: &BlueprintDb, key: &str, materiel: f64, tier: u8) -> Side {
    let id = db.id(key);
    Side { unit: id, count: (materiel / cost_at(db.get(id), tier)).round().max(1.0) as u32, tactics: Tactics::AttackMove }
}

/// The materiel of a tier fight at T1, ten anchors ([pace]): a bombing strike's size.
pub fn strike_fight_materiel() -> f64 {
    10.0 * Anchor::from_targets().materiel
}

/// One of `key`, attack-moving.
pub fn one(db: &BlueprintDb, key: &str) -> Side {
    Side { unit: db.id(key), count: 1, tactics: Tactics::AttackMove }
}

/// An army with `share` of `materiel` in Mobile Anti-Air and the rest in anchors.
pub fn escorted_army(db: &BlueprintDb, materiel: f64, share: f64) -> [Side; 2] {
    [bought(db, "t1_medium_tank", materiel * (1.0 - share)), bought(db, "t1_mobile_aa", materiel * share)]
}

/// T1 air from its rules (`[air]` and `[kills]`), against T1 land frozen. No strength is solved
/// for: each rule sets one number, in order. The bomb is the biggest that destroys a T1 tank in two; Mobile
/// Anti-Air takes two bombs, at the anchor's health per mass; a
/// bomber lasts the rule's time under one interceptor's gun; and Mobile Anti-Air's gun is what
/// brings an army a fifth anti-air even with an equal materiel of bombers.
pub fn derive_t1_air(base: &BlueprintDb) -> BlueprintDb {
    let anchor = Anchor::from_targets();
    let bomber = base.id("t1_bomber");
    let anti_air = base.id("t1_mobile_aa");
    let bomb_with = |db: &BlueprintDb, damage: f64| db.with_unit(bomber, |unit| unit.weapon.as_mut().expect("bombers carry bombs").damage = Fp::from_num(damage.round()));
    // The bomb: as big as lets a T1 tank take two.
    let bomb = (anchor.health / target_number("air", "line_health_over_bomb")).round();
    let mut db = bomb_with(base, bomb);
    // Mobile Anti-Air: two bombs' worth of health, the anchor's health per mass, the anchor's
    // energy per mass, and its build time per mass as it is.
    let health = (bomb * target_number("air", "anti_air_health_over_bomb")).round();
    let mass = (health / (anchor.health / anchor.materiel)).round();
    db = db.with_unit(anti_air, |unit| {
        let build_per_mass = unit.build_time.to_num::<f64>() / unit.mass.to_num::<f64>();
        unit.health = Fp::from_num(health);
        unit.mass = Res::from_num(mass);
        unit.energy = Res::from_num(mass * anchor.energy / anchor.materiel);
        unit.build_time = Res::from_num((mass * build_per_mass).round());
    });
    // The bomber: so many seconds of one interceptor's gun.
    let interceptor = db.get(db.id("t1_interceptor")).weapon.expect("interceptors have a gun");
    let interceptor_dps = interceptor.damage.to_num::<f64>() * TICKS_PER_SECOND as f64 / f64::from(interceptor.reload_ticks.max(1));
    let bomber_health = (interceptor_dps * target_number("air", "interceptor_seconds_per_bomber_t1")).round();
    db = db.with_unit(bomber, |unit| unit.health = Fp::from_num(bomber_health));
    // Mobile Anti-Air's gun: an escorted army's score against bombers rises with it.
    let gun = db.get(anti_air).weapon.expect("anti-air has a gun").damage.to_num::<f64>();
    let materiel = target_number("air", "check_materiel");
    let share = target_number("air", "anti_air_escort_share");
    let gun_with = |db: &BlueprintDb, damage: f64| db.with_unit(anti_air, |unit| unit.weapon.as_mut().expect("anti-air has a gun").damage = Fp::from_num((damage * 10.0).round() / 10.0));
    let multiple = nearest_strength(0.0, true, |multiple| {
        let trial = Arc::new(gun_with(&db, gun * multiple));
        mirrored_armies(&trial, &escorted_army(&trial, materiel, share), &[bought(&trial, "t1_bomber", materiel)], 1.0).score
    });
    gun_with(&db, gun * multiple)
}

/// The `--balance-derive t1-air` report: T1 air's numbers from its rules, against T1 land frozen,
/// with the checks they should meet.
pub fn derive_t1_air_report() {
    let db = Arc::new(derive_t1_air(&BlueprintDb::standard()));
    println!("T1 air from data/balance_targets.toml ([air], [kills]), against T1 land frozen:");
    for key in ["t1_bomber", "t1_interceptor", "t1_mobile_aa"] {
        let unit = db.get(db.id(key));
        let weapon = unit.weapon.expect("T1 air units have a weapon");
        println!("  {}", describe(unit));
        println!("             mass = {}, energy = {}, build_time = {}, health = {}, damage = {}", unit.mass, unit.energy, unit.build_time, unit.health, weapon.damage);
    }
    println!();
    print_t1_air_checks(&db);
}

/// The T1 air rules' checks, measured on `db`.
pub fn print_t1_air_checks(db: &Arc<BlueprintDb>) {
    let materiel = target_number("air", "check_materiel");
    let share = target_number("air", "anti_air_escort_share");
    println!("Checks, mirrored, score for the first:");
    let raid = mirrored(db, Side { count: 5, ..one(db, "t1_bomber") }, Side { tactics: Tactics::Hold, ..one(db, "command_rig") }, 1.0);
    let wanted = target_number_range("air", "commander_seconds_five_bombers_t1");
    println!("  five bombers against a commander alone: {:.0} s{} (targets: {:.0}-{:.0} s)", raid.seconds, if raid.finished { "" } else { ", not destroyed" }, wanted.0, wanted.1);
    let walking = mirrored(db, Side { count: 5, ..one(db, "t1_bomber") }, one(db, "command_rig"), 1.0);
    println!("  the same, the commander walking at them: {:.0} s", walking.seconds);
    let caught = strike_seconds(db, one(db, "t1_interceptor"), one(db, "t1_bomber"));
    println!("  one interceptor against one bomber:   {caught:.1} s (target: about {:.0} s)", target_number("air", "interceptor_seconds_per_bomber_t1"));
    let fight = strike_fight_materiel();
    let strike = mirrored(db, bought(db, "t1_bomber", fight), bought(db, "t1_medium_tank", fight), 1.0);
    println!("  bombers against ten anchors, no anti-air: {:+.2} in {:.1} s (targets: 25-45 s)", strike.score, strike.seconds);
    let big = mirrored(db, bought(db, "t1_bomber", materiel), bought(db, "t1_medium_tank", materiel), 1.0);
    println!("  the same at {materiel:.0} materiel a side:   {:+.2} in {:.1} s (more bombers bomb the same targets)", big.score, big.seconds);
    let escorted = mirrored_armies(db, &escorted_army(db, materiel, share), &[bought(db, "t1_bomber", materiel)], 1.0);
    println!("  an army {:.0}% anti-air against bombers: {:+.2} in {:.1} s (about even, within 0.15)", share * 100.0, escorted.score, escorted.seconds);
    let pure = mirrored(db, bought(db, "t1_mobile_aa", materiel), bought(db, "t1_bomber", materiel), 1.0);
    println!("  Mobile Anti-Air against bombers:      {:+.2} (anti-air keeps at least {:.1})", pure.score, target_number("air", "anti_air_keeps_min"));
    let caught_all = mirrored(db, bought(db, "t1_interceptor", materiel), bought(db, "t1_bomber", materiel), 1.0);
    println!("  interceptors against bombers:         {:+.2} in {:.1} s", caught_all.score, caught_all.seconds);
    println!("  interceptors against interceptors:    {:+.2} (even)", mirrored(db, bought(db, "t1_interceptor", materiel), bought(db, "t1_interceptor", materiel), 1.0).score);
}
