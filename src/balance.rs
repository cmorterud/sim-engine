//! The balance harness (BALANCE.md, step 3): headless, deterministic duels between groups of
//! units, and the health multiplier at which two units of equal materiel come out even.
//!
//! The harness lives outside the simulation. It sets up worlds, gives orders, and reads the
//! results, so it may use floating point; the simulation itself stays in fixed point.

use std::sync::Arc;

use crate::math::fp::{fp_int, Fp, Vec2Fp};
use crate::sim::blueprints::{Blueprint, BlueprintDb, BlueprintId, WeaponDef, TICKS_PER_SECOND};
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

/// Where a duel is fought (2026-11-24, for fights between domains), shared with the playtest
/// scenarios that play these fights by hand.
pub use crate::sim::grounds::Ground;

/// A duel's setting: its ground, and the tier whose prices weigh each side (`cost_at`), both in
/// what a side buys and in what it has left.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub ground: Ground,
    pub tier: u8,
}

impl Field {
    /// Flat land at T1's prices: every duel before 2026-11-24.
    pub const LAND: Self = Self { ground: Ground::Land, tier: 1 };

    pub const fn new(ground: Ground, tier: u8) -> Self {
        Self { ground, tier }
    }
}

/// Where the shore's land begins: west of the middle, so that a position built round the middle,
/// as `siege` builds one, stands ashore, and both groups' starts are on their own sides of it.
pub const SHORE_X: i32 = Ground::shore_x(MAP_UNITS);

/// A world for a duel on a ground. Fog is off, so each side sees the other from the start.
fn duel_world(ground: Ground) -> SimWorld {
    let config = SimConfig { map_units: MAP_UNITS, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = ground.world(config, 1);
    if ground != Ground::Land {
        world.prepare_map();
    }
    world
}

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
    duel_in(db, Field::LAND, &[blue], &[red], health, LAYOUTS[0])
}

/// A duel in one starting layout: `(extra gap, Red's sideways offset)`. Each team may be several
/// groups, an army with its escort, each in its own block behind the one before.
fn duel_in(db: &Arc<BlueprintDb>, field: Field, blue: &[Side], red: &[Side], health: [f64; 2], layout: (i32, i32)) -> DuelResult {
    let mut world = duel_world(field.ground);
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
    let full = |world: &SimWorld, ids: &[SimId]| ids.iter().map(|&id| worth(world, id, field.tier)).sum::<f64>();
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

/// A weapon's damage a second, with every shot of a volley, from start of volley to start of
/// volley.
pub fn damage_per_second(weapon: &WeaponDef) -> f64 {
    weapon.damage.to_num::<f64>() * f64::from(weapon.shots_per_reload()) * TICKS_PER_SECOND as f64 / f64::from(weapon.reload_ticks.max(1))
}

/// The highest tier the model has constants for: T4, the experimentals (2026-11-27). Each
/// per-tier table in data/balance_targets.toml has an entry for each tier up to it.
pub const MAX_TIER: u8 = 4;

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
    let (rate, land_share) = rates[usize::from(tier.clamp(1, MAX_TIER)) - 1];
    (unit.mass.to_num::<f64>() + unit.energy.to_num::<f64>() / rate) / land_share
}

/// A unit's cost at a tier's prices, times its share of health left; nothing once destroyed.
fn worth(world: &SimWorld, id: SimId, tier: u8) -> f64 {
    let Some(entity) = world.unit_entity(id) else { return 0.0 };
    let (Ok(health), Ok(unit)) = (world.ecs.get::<&Health>(entity), world.ecs.get::<&Blueprinted>(entity)) else { return 0.0 };
    let price = cost_at(world.blueprints.get(unit.0), tier);
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
    let columns = group_columns(side.count, spacing);
    let rows = (side.count as i32 + columns - 1) / columns;
    ((rows - 1) * spacing, spacing)
}

/// How many abreast a group stands: five, or as few more as keep it no deeper than half the map,
/// so that a large group of cheap units still starts on it.
fn group_columns(count: u32, spacing: i32) -> i32 {
    let deepest = (MAP_UNITS / 2 / spacing.max(1)).max(1);
    (count.max(1) as i32 + deepest - 1).div_euclid(deepest).max(count.clamp(1, 5) as i32)
}

/// Spawns a group in a block of five abreast (more for a very large group, `group_columns`), facing the other group, centered on `x`.
/// `back` is the side away from the enemy, -1 for the west.
fn spawn_group(world: &mut SimWorld, side: Side, team: Team, x: i32, back: i32, offset: i32) -> Vec<SimId> {
    let (_, spacing) = group_shape(world, side);
    let columns = group_columns(side.count, spacing);
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
/// Point defenses alone, three of a tier where the positions above put theirs, for `[premiums]`.
pub const T1_DEFENSES: Position = &[("t1_point_defense", 124, 128), ("t1_point_defense", 132, 128), ("t1_point_defense", 128, 124)];
pub const T2_DEFENSES: Position = &[("t2_point_defense", 124, 128), ("t2_point_defense", 132, 128), ("t2_point_defense", 128, 124)];
pub const T3_DEFENSES: Position = &[("t3_heavy_point_defense", 124, 128), ("t3_heavy_point_defense", 132, 128), ("t3_heavy_point_defense", 128, 124)];
pub const T3_POSITION: Position = &[("t3_heavy_shield", 127, 127), ("t3_heavy_point_defense", 124, 128), ("t3_heavy_point_defense", 132, 128), ("t3_heavy_point_defense", 128, 124)];

/// The materiel a position costs.
pub fn position_materiel(db: &BlueprintDb, position: Position) -> f64 {
    position.iter().map(|&(key, _, _)| cost(db.get(db.id(key)))).sum()
}

/// A siege: `count` of `unit` for Blue start west of `position`, beyond its reach, and
/// attack-move onto it. Red has the energy to keep its shields up throughout.
pub fn siege(db: &Arc<BlueprintDb>, unit: BlueprintId, count: u32, position: Position) -> SiegeResult {
    siege_on(db, Field::LAND, &[Side { unit, count, tactics: Tactics::AttackMove }], position)
}

/// `siege` by a force of several groups, on a ground: on the shore the attackers start at sea and
/// the position stands ashore, or on the water if it is built there, as torpedo launchers are.
pub fn siege_on(db: &Arc<BlueprintDb>, field: Field, attackers: &[Side], position: Position) -> SiegeResult {
    let mut world = duel_world(field.ground);
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
    let longest = attackers.iter().filter_map(|side| db.get(side.unit).weapon).map(|weapon| weapon.range.to_num::<i32>()).max().unwrap_or(0);
    let start = middle - reach.max(longest) - 400;
    // On the shore, ships start at sea, and well inside the ground however far they reach.
    let start = if field.ground == Ground::Shore { start.clamp(400, SHORE_X - 400) } else { start };
    let groups = spawn_team(&mut world, attackers, Team::Blue, start, -1, 0);
    let attackers: Vec<SimId> = groups.iter().flat_map(|(_, ids)| ids.iter().copied()).collect();
    world.advance_tick(&[SimCommand::AttackMove { units: attackers.clone(), goal: Vec2Fp::new(fp_int(middle), fp_int(middle)) }]);
    let worth_of = |world: &SimWorld, ids: &[SimId]| ids.iter().map(|&id| worth(world, id, field.tier)).sum::<f64>();
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
    mirrored_on(db, Field::LAND, a, b, h)
}

/// `mirrored_armies` on a field. On the shore, which is not the same both ways round, `a` is
/// always at sea and `b` ashore, and only the layouts are averaged.
pub fn mirrored_on(db: &Arc<BlueprintDb>, field: Field, a: &[Side], b: &[Side], h: f64) -> DuelResult {
    let (mut score, mut seconds, mut finished, mut runs) = (0.0, 0.0, true, 0.0);
    for layout in LAYOUTS {
        let first = duel_in(db, field, a, b, [h, 1.0 / h], layout);
        score += first.score;
        seconds += first.seconds;
        finished &= first.finished;
        runs += 1.0;
        if field.ground != Ground::Shore {
            let second = duel_in(db, field, b, a, [1.0 / h, h], layout);
            score -= second.score;
            seconds += second.seconds;
            finished &= second.finished;
            runs += 1.0;
        }
    }
    DuelResult { score: score / runs, seconds: seconds / runs, finished }
}

/// The materiel a force costs at a tier's prices.
pub fn force_materiel(db: &BlueprintDb, force: &[Side], tier: u8) -> f64 {
    force.iter().map(|side| f64::from(side.count) * cost_at(db.get(side.unit), tier)).sum()
}

/// A force of whole squadrons, each with so many of each unit, bought to about `materiel` at a
/// tier's prices: at least one squadron.
pub fn squadrons(db: &BlueprintDb, squadron: &[(BlueprintId, u32)], materiel: f64, tier: u8, tactics: Tactics) -> Vec<Side> {
    let one: f64 = squadron.iter().map(|&(unit, count)| f64::from(count) * cost_at(db.get(unit), tier)).sum();
    let many = (materiel / one).round().max(1.0) as u32;
    squadron.iter().map(|&(unit, count)| Side { unit, count: count * many, tactics }).collect()
}

/// Where two forces come out even, as `break_even` finds for two units: `a`'s health is scaled by
/// `h` and `b`'s by its inverse, and `efficiency` is `a`'s strength per materiel over `b`'s, so it
/// is also how many times `a`'s cost of `b` it takes to break `a`.
pub fn break_even_forces(db: &Arc<BlueprintDb>, field: Field, a: &[Side], b: &[Side]) -> BreakEven {
    let (mut low, mut high) = (-4.0f64, 4.0f64);
    let mut seconds = 0.0;
    for _ in 0..BREAK_EVEN_STEPS {
        let middle = (low + high) / 2.0;
        let result = mirrored_on(db, field, a, b, middle.exp2());
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
    let efficiency = (force_materiel(db, b, field.tier) / force_materiel(db, a, field.tier)) / health_factor;
    BreakEven { health_factor, efficiency, seconds, at_limit: exponent.abs() > 3.99 }
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
    let dps = weapon.map_or(0.0, damage_per_second);
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
pub const TARGETS: &str = include_str!("../data/balance_targets.toml");

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
    let dps = damage_per_second(&weapon);
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
        // Tough and amphibious, it holds the front and crosses water. It pays for crossing water
        // (`[premiums] hovercraft_over_heavy_tank_on_land`, 2026-11-25): about three-quarters as
        // strong per materiel as the line unit on land, by break-even, as the inspiration has it.
        // It was about even, a little under, until then.
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
            target: Target::Efficiency("t2_heavy_tank", 0.79),
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

/// T1 air from its rules (`[air]`, `[kills]` and `[kill_times]`), against T1 land frozen. No
/// strength is solved for: each rule sets one number, in order. The bomb is the biggest that
/// destroys a T1 tank in two; Mobile Anti-Air takes two bombs, at the anchor's health per mass; a
/// bomber lasts the rule's time under one interceptor's gun; Mobile Anti-Air's gun destroys a
/// bomber in its `[kill_times]` time; and the bomber costs what brings an army a fifth anti-air
/// even with an equal materiel of bombers (2026-11-27), which sets the T1 air base
/// (`[price_list]`). Its mass, energy and build time scale together to that cost.
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
    // The Interceptor: its size, √(health × damage a second), is what an equal cost of them
    // clearing an equal cost of bombers in [air] interceptors_clear_bombers_seconds holds (checked
    // by its fight, as every T1 air number scales with it); its health over its damage a second is
    // the [model] time to kill in air-to-air fights at T1.
    let interceptor_id = db.id("t1_interceptor");
    let interceptor = db.get(interceptor_id).weapon.expect("interceptors have a gun");
    let size = (db.get(interceptor_id).health.to_num::<f64>() * damage_per_second(&interceptor)).sqrt();
    let air_to_air = model_time_to_kill(1, "air_to_air");
    let reload_seconds = f64::from(interceptor.reload_ticks.max(1)) / TICKS_PER_SECOND as f64 / f64::from(interceptor.shots_per_reload());
    db = db.with_unit(interceptor_id, |unit| {
        unit.health = Fp::from_num((size * air_to_air.sqrt()).round());
        unit.weapon.as_mut().expect("interceptors have a gun").damage = Fp::from_num((size / air_to_air.sqrt() * reload_seconds * 10.0).round() / 10.0);
    });
    // The bomber: so many seconds of one interceptor's gun.
    let interceptor = db.get(interceptor_id).weapon.expect("interceptors have a gun");
    let interceptor_dps = damage_per_second(&interceptor);
    let bomber_health = (interceptor_dps * target_number("air", "interceptor_seconds_per_bomber_t1")).round();
    db = db.with_unit(bomber, |unit| unit.health = Fp::from_num(bomber_health));
    // Mobile Anti-Air's gun: one destroys a bomber in the middle of its [kill_times] time.
    let (least, most) = target_number_range("kill_times", "mobile_aa_t1_bomber");
    let gun = (bomber_health / ((least + most) / 2.0) * 10.0).round() / 10.0;
    db = db.with_unit(anti_air, |unit| unit.weapon.as_mut().expect("anti-air has a gun").damage = Fp::from_num(gun));
    // The bomber's cost: where an army a fifth anti-air and an equal materiel of bombers break
    // even. So many bombers, each costing the army's materiel over their number, break even where
    // the army's health is scaled by `h`, at a cost of that times `h` (their cost counts in the
    // fight through the veterancy their kills give). Whole bombers and whole bombs make one
    // fight's answer jump by about a tenth from one count to the next, so it is the mean over
    // five counts round the materiel's, recentred until it stays.
    let materiel = target_number("air", "check_materiel");
    let share = target_number("air", "anti_air_escort_share");
    let costing = |db: &BlueprintDb, cost: f64| {
        let scale = cost / cost_at(db.get(bomber), 1);
        db.with_unit(bomber, |unit| {
            unit.mass = Res::from_num((unit.mass.to_num::<f64>() * scale).round());
            unit.energy = Res::from_num((unit.energy.to_num::<f64>() * scale).round());
            unit.build_time = Res::from_num((unit.build_time.to_num::<f64>() * scale).round());
        })
    };
    let army = escorted_army(&db, materiel, share);
    let army_materiel = force_materiel(&db, &army, 1);
    let even_at = |count: u32| {
        let each = army_materiel / f64::from(count);
        let trial = Arc::new(costing(&db, each));
        let bombers = Side { unit: bomber, count, tactics: Tactics::AttackMove };
        each * break_even_forces(&trial, Field::LAND, &army, &[bombers]).health_factor
    };
    let mut cost = cost_at(db.get(bomber), 1);
    for _ in 0..3 {
        let middle = (materiel / cost).round() as u32;
        let next = (middle - 2..=middle + 2).map(even_at).sum::<f64>() / 5.0;
        let settled = (materiel / next).round() as u32 == middle;
        cost = next;
        if settled {
            break;
        }
    }
    let db = costing(&db, cost);
    // The T1 air base is what prices the bomber at that cost (`[price_list] domain_base air_t1`),
    // and the Interceptor costs the list's price at it.
    let base = t1_air_base(&db);
    let listed = crate::price_list::price(&db, "t1_interceptor").expect("the list prices the interceptor").at_base(base);
    scaled_to(&db, interceptor_id, 1, listed)
}

/// The T1 air base that prices the T1 Bomber at its cost, which the escort rule sets.
pub fn t1_air_base(db: &BlueprintDb) -> f64 {
    let bomber = crate::price_list::price(db, "t1_bomber").expect("the list prices the bomber");
    bomber.base_for(bomber.actual)
}

/// The [model] time to kill at a tier, in a domain's fights (`time_to_kill_domain`).
pub fn model_time_to_kill(tier: u8, domain: &str) -> f64 {
    let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    let model = &table["model"];
    let tiers = model["time_to_kill_tier"].as_array().expect("[model] time_to_kill_tier");
    as_number(&model["time_to_kill_base_seconds"]) * as_number(&tiers[usize::from(tier.clamp(1, MAX_TIER)) - 1]) * as_number(&model["time_to_kill_domain"][domain])
}

/// A copy with one unit's mass, energy and build time scaled together to a cost at a tier.
pub fn scaled_to(db: &BlueprintDb, id: BlueprintId, tier: u8, cost: f64) -> BlueprintDb {
    let scale = cost / cost_at(db.get(id), tier);
    db.with_unit(id, |unit| {
        unit.mass = Res::from_num((unit.mass.to_num::<f64>() * scale).round());
        unit.energy = Res::from_num((unit.energy.to_num::<f64>() * scale).round());
        unit.build_time = Res::from_num((unit.build_time.to_num::<f64>() * scale).round());
    })
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
    println!("  the T1 air base that prices the bomber at its cost: {:.3} ([price_list] domain_base air_t1)", t1_air_base(&db));
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
    let clear = target_number_range("air", "interceptors_clear_bombers_seconds");
    println!("  interceptors against bombers:         {:+.2} in {:.1} s (targets: {:.0}-{:.0} s)", caught_all.score, caught_all.seconds, clear.0, clear.1);
    println!("  interceptors against interceptors:    {:+.2} (even)", mirrored(db, bought(db, "t1_interceptor", materiel), bought(db, "t1_interceptor", materiel), 1.0).score);
}

/// A force named in `[domains.forces]` in its proportions by cost, bought to about `materiel` at
/// a tier's prices in whole units of each: so that it can match another force's materiel closely,
/// which whole squadrons cannot.
pub fn force_by_cost(db: &BlueprintDb, name: &str, materiel: f64, tier: u8) -> Vec<Side> {
    let squadron = domain_force(db, name);
    let one: f64 = squadron.iter().map(|&(unit, count)| f64::from(count) * cost_at(db.get(unit), tier)).sum();
    squadron.iter().map(|&(unit, count)| Side { unit, count: (materiel * f64::from(count) / one).round().max(1.0) as u32, tactics: Tactics::AttackMove }).collect()
}

/// The escort rule at a tier (`[air] escort_forces`): the land army and the strike aircraft
/// that are to break even at a tier.
pub fn escort_forces(tier: u8) -> (String, String) {
    let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    let pair = table["air"]["escort_forces"][format!("t{tier}").as_str()].as_array().unwrap_or_else(|| panic!("[air] escort_forces has t{tier}"));
    let name = |index: usize| pair[index].as_str().expect("a force's name").to_string();
    (name(0), name(1))
}

/// Strike aircraft against an escorted army (`[air] escort_forces`): their break-even efficiency
/// against the tier's land army, the mean over armies of `[air] escort_squadrons` squadrons, the
/// aircraft bought by cost to each army's materiel. Above 1, the aircraft are the better buy. One
/// fight's answer moves by about a tenth from one size to the next, as whole units and the
/// aircraft's passes fall, so it is the mean.
pub fn strike_against_army(db: &BlueprintDb, tier: u8) -> f64 {
    let (army, strike) = escort_forces(tier);
    let db = Arc::new(db.clone());
    // Each unit marches in its own block, one behind another, as a player lines an army up: the
    // shortest reach in front, and the escort behind the rest, as at T1 (`escorted_army`). Where
    // each stands decides much of the fight, since aircraft strip the army of what they reach
    // first.
    let mut squadron = domain_force(&db, &army);
    let is_anti_air = |unit: BlueprintId| db.get(unit).weapon.is_some_and(|weapon| weapon.targets == crate::sim::blueprints::Targets::Air);
    let reach = |unit: BlueprintId| db.get(unit).weapon.map_or(0, |weapon| weapon.range.to_num::<i32>());
    squadron.sort_by_key(|&(unit, _)| (is_anti_air(unit), reach(unit)));
    let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    let range = table["air"]["escort_squadrons"][format!("t{tier}").as_str()].as_array().unwrap_or_else(|| panic!("[air] escort_squadrons has t{tier}"));
    let sizes = (as_number(&range[0]) as u32)..=(as_number(&range[1]) as u32);
    let count = sizes.clone().count() as f64;
    sizes
        .map(|size| {
            let army: Vec<Side> = squadron.iter().map(|&(unit, each)| Side { unit, count: each * size, tactics: Tactics::AttackMove }).collect();
            let air = force_by_cost(&db, &strike, force_materiel(&db, &army, tier), tier);
            break_even_forces(&db, Field::new(Ground::Land, tier), &air, &army).efficiency
        })
        .sum::<f64>()
        / count
}

// The battle line (2026-11-27, the design owner's pick): an army or fleet standing in fixed
// ranks, and an equal cost of aircraft coming at it from the front, a flank, or the rear. Where
// an army's units stand decided much of what aircraft did to it (Heavy Gunships 0.94 to 1.66
// times the better buy by march order alone), so the ranks are set by rule, and the approaches
// averaged over, rather than taken from any one march.

/// Where aircraft come at a battle line from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Approach {
    Front,
    Flank,
    Rear,
}

/// Every approach a battle line is measured from, each counted alike.
pub const APPROACHES: [Approach; 3] = [Approach::Front, Approach::Flank, Approach::Rear];

/// How one battle-line fight starts: where the aircraft come from, which team holds the line,
/// and the layout, `(extra gap, offset along the line's edge)`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LineStart {
    pub approach: Approach,
    pub line_team: Team,
    pub layout: (i32, i32),
}

/// A unit's rank in a battle line: 0 in front, 1 in the middle, 2 behind. Anti-air, whatever
/// does more damage a second to aircraft than to the ground, and shields with no gun of their
/// own stand in the middle, covered by the front and covering it; artillery and guns that lob
/// stand behind, where their reach lets them; everything else is in front.
pub fn battle_rank(unit: &Blueprint) -> u8 {
    let (surface, _) = purpose_weapons(unit, Purpose::Surface);
    let (air, _) = purpose_weapons(unit, Purpose::Air);
    let lobs = unit.weapon.iter().chain(&unit.extra_weapons).any(|weapon| weapon.artillery || weapon.lobbed);
    if (air > 0.0 && air >= surface) || (surface == 0.0 && unit.shield.is_some()) {
        1
    } else if lobs {
        2
    } else {
        0
    }
}

/// A force in its battle-line order, front to back: by rank, and in each rank the shortest
/// surface reach first, so that what must close to fire is nearest the enemy.
pub fn battle_order(db: &BlueprintDb, force: &[Side]) -> Vec<Side> {
    let mut ordered = force.to_vec();
    let key = |side: &Side| {
        let unit = db.get(side.unit);
        (battle_rank(unit), purpose_weapons(unit, Purpose::Surface).1.round() as i64, side.unit.0)
    };
    ordered.sort_by_key(key);
    ordered
}

/// The battle line's ground: 10 km across, room for a massed T3 fleet and the aircraft that
/// come at it from any side.
const LINE_MAP_UNITS: i32 = 10_240;

/// A block's columns for `count` units: at least five abreast, as `spawn_group` lays a group
/// out, and square once there are more than 25, so that a massed rank is a line and not a
/// column.
fn block_columns(count: u32) -> i32 {
    (f64::from(count).sqrt().ceil() as i32).max(5).min(count.max(1) as i32)
}

/// Spawns units in a block centered on `(x, y)`, `columns` abreast along y, each row behind
/// the last towards `back` (-1 west, 1 east), `spacing` apart.
fn spawn_block(world: &mut SimWorld, units: &[BlueprintId], team: Team, (x, y): (i32, i32), back: i32, columns: i32, spacing: i32) -> Vec<SimId> {
    let rows = (units.len() as i32 + columns - 1) / columns;
    units
        .iter()
        .enumerate()
        .map(|(index, &unit)| {
            let (row, column) = (index as i32 / columns, index as i32 % columns);
            let along = y + (column * 2 - (columns - 1)) * spacing / 2;
            let depth = x + back * (row * spacing - (rows - 1) * spacing / 2);
            world.spawn_mobile(unit, team, Vec2Fp::new(fp_int(depth), fp_int(along)))
        })
        .collect()
}

/// The spacing a unit keeps in a block: three times its radius, as `group_shape`'s.
fn block_spacing(db: &BlueprintDb, unit: BlueprintId) -> i32 {
    db.get(unit).radius.to_num::<i32>().max(4) * 3
}

/// One battle-line fight: `line` standing in its ranks, facing west, with its health times
/// `health[0]`, and `air` with its health times `health[1]`, in one block `START_GAP` and the
/// layout's extra gap off the line's edge on the side the start's approach names, shifted along
/// that edge by the layout's offset, attack-moving at its middle. The line holds: it fires at what comes,
/// and nothing moves it. Each rank stands one behind another, at least five abreast and square
/// once massed. `line_team` sides the teams, so that the two can be swapped; on a flank
/// approach, the line's Blue comes from the north and its Red from the south. The score is the
/// air's share of its materiel left less the line's.
pub fn battle_line_fight(db: &Arc<BlueprintDb>, field: Field, line: &[Side], air: &[Side], health: [f64; 2], start: LineStart) -> DuelResult {
    let LineStart { approach, line_team, layout } = start;
    let config = SimConfig { map_units: LINE_MAP_UNITS, terrain: TerrainStyle::Flat, fog_of_war: false, ..SimConfig::default() };
    let mut world = field.ground.world(config, 1);
    if field.ground != Ground::Land {
        world.prepare_map();
    }
    world.blueprints = Arc::clone(db);
    let middle = LINE_MAP_UNITS / 2;
    // Each rank's depth and spacing, front to back, then the whole centered on the map.
    let ranks: Vec<(Side, i32, i32, i32)> = battle_order(db, line)
        .into_iter()
        .map(|side| {
            let (columns, spacing) = (block_columns(side.count), block_spacing(db, side.unit));
            let rows = (side.count as i32 + columns - 1) / columns;
            (side, columns, spacing, (rows - 1) * spacing)
        })
        .collect();
    let depth: i32 = ranks.windows(2).map(|pair| (pair[0].3 + pair[1].3) / 2 + pair[1].2).sum();
    let mut center = middle - depth / 2;
    let mut line_ids = Vec::new();
    for (index, &(side, columns, spacing, rank_depth)) in ranks.iter().enumerate() {
        if index > 0 {
            center += (ranks[index - 1].3 + rank_depth) / 2 + spacing;
        }
        line_ids.extend(spawn_block(&mut world, &vec![side.unit; side.count as usize], line_team, (center, middle), 1, columns, spacing));
    }
    let positions: Vec<Vec2Fp> = line_ids.iter().filter_map(|&id| world.unit_position(id)).map(|at| at.xy()).collect();
    let span = |pick: fn(&Vec2Fp) -> i32| (positions.iter().map(pick).min().unwrap_or(middle), positions.iter().map(pick).max().unwrap_or(middle));
    let ((west, east), (south, north)) = (span(|at| at.x.to_num()), span(|at| at.y.to_num()));
    // The aircraft in one block, by kind in turn, square.
    let wing: Vec<BlueprintId> = air.iter().flat_map(|side| std::iter::repeat_n(side.unit, side.count as usize)).collect();
    let spacing = air.iter().map(|side| block_spacing(db, side.unit)).max().unwrap_or(12);
    let columns = (wing.len() as f64).sqrt().ceil().max(1.0) as i32;
    let half = ((wing.len() as i32 + columns - 1) / columns - 1) * spacing / 2;
    let (gap, along) = (START_GAP + layout.0 + half, layout.1);
    let flank_north = line_team == Team::Blue;
    let (x, y) = match approach {
        Approach::Front => (west - gap, (south + north) / 2 + along),
        Approach::Rear => (east + gap, (south + north) / 2 + along),
        Approach::Flank => ((west + east) / 2 + along, if flank_north { north + gap } else { south - gap }),
    };
    let back = if approach == Approach::Rear { 1 } else { -1 };
    let air_ids = spawn_block(&mut world, &wing, line_team.enemy(), (x, y), back, columns, spacing);
    for (ids, scale) in [(&line_ids, health[0]), (&air_ids, health[1])] {
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
    let goal = Vec2Fp::new(fp_int((west + east) / 2), fp_int((south + north) / 2));
    world.advance_tick(&[SimCommand::AttackMove { units: air_ids.clone(), goal }]);
    let full = |world: &SimWorld, ids: &[SimId]| ids.iter().map(|&id| worth(world, id, field.tier)).sum::<f64>();
    let (line_full, air_full) = (full(&world, &line_ids), full(&world, &air_ids));
    let mut first_damage = None;
    let mut finished = false;
    while world.current_tick < MAX_TICKS {
        let mut orders = Vec::new();
        if world.current_tick.is_multiple_of(10) {
            reengage_aircraft(&world, &air_ids, &line_ids, &mut orders);
        }
        world.advance_tick(&orders);
        let (line_now, air_now) = (full(&world, &line_ids), full(&world, &air_ids));
        if first_damage.is_none() && (line_now < line_full || air_now < air_full) {
            first_damage = Some(world.current_tick);
        }
        if line_now == 0.0 || air_now == 0.0 {
            finished = true;
            break;
        }
    }
    let score = full(&world, &air_ids) / air_full.max(f64::MIN_POSITIVE) - full(&world, &line_ids) / line_full.max(f64::MIN_POSITIVE);
    let seconds = f64::from(world.current_tick - first_damage.unwrap_or(world.current_tick)) / 10.0;
    DuelResult { score, seconds, finished }
}

/// The battle line's score at a health multiplier, `air`'s health times `h` and the line's over
/// it: the mean over every approach and starting layout, with the teams each way round.
pub fn battle_line_mean(db: &Arc<BlueprintDb>, field: Field, line: &[Side], air: &[Side], h: f64) -> DuelResult {
    let (mut score, mut seconds, mut finished, mut runs) = (0.0, 0.0, true, 0.0);
    for approach in APPROACHES {
        for (line_team, layout) in [Team::Blue, Team::Red].into_iter().flat_map(|team| LAYOUTS.map(|layout| (team, layout))) {
            let result = battle_line_fight(db, field, line, air, [1.0 / h, h], LineStart { approach, line_team, layout });
            score += result.score;
            seconds += result.seconds;
            finished &= result.finished;
            runs += 1.0;
        }
    }
    DuelResult { score: score / runs, seconds: seconds / runs, finished }
}

/// Where aircraft and a battle line come out even, as `break_even_forces` finds for two forces
/// meeting in the open: `efficiency` is the air's strength per materiel over the line's, so it
/// is also how many times the air's cost of the line it takes to break the air.
pub fn battle_line_break_even(db: &Arc<BlueprintDb>, field: Field, line: &[Side], air: &[Side]) -> BreakEven {
    let (mut low, mut high) = (-4.0f64, 4.0f64);
    let mut seconds = 0.0;
    for _ in 0..BREAK_EVEN_STEPS {
        let middle = (low + high) / 2.0;
        let result = battle_line_mean(db, field, line, air, middle.exp2());
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
    let efficiency = (force_materiel(db, line, field.tier) / force_materiel(db, air, field.tier)) / health_factor;
    BreakEven { health_factor, efficiency, seconds, at_limit: exponent.abs() > 3.99 }
}

/// A named force's battle-line efficiency against named aircraft at a tier, the mean over
/// forces of each of `sizes` squadrons, the aircraft bought by cost to each force's materiel.
/// Above 1, the aircraft are the better buy.
pub fn air_against_battle_line(db: &BlueprintDb, ground: Ground, tier: u8, line: &str, air: &str, sizes: std::ops::RangeInclusive<u32>) -> f64 {
    let db = Arc::new(db.clone());
    let squadron = domain_force(&db, line);
    let count = sizes.clone().count() as f64;
    sizes
        .map(|size| {
            let force: Vec<Side> = squadron.iter().map(|&(unit, each)| Side { unit, count: each * size, tactics: Tactics::Hold }).collect();
            let aircraft = force_by_cost(&db, air, force_materiel(&db, &force, tier), tier);
            battle_line_break_even(&db, Field::new(ground, tier), &force, &aircraft).efficiency
        })
        .sum::<f64>()
        / count
}

fn battle_line_table() -> toml::Value {
    TARGETS.parse::<toml::Table>().expect("data/balance_targets.toml is valid").remove("battle_line").expect("[battle_line]")
}

/// The squadron sizes a force is measured at in the battle line (`[battle_line] squadrons`).
pub fn battle_line_squadrons(force: &str) -> std::ops::RangeInclusive<u32> {
    let table = battle_line_table();
    let range = table["squadrons"].get(force).and_then(toml::Value::as_array).unwrap_or_else(|| panic!("[battle_line] squadrons has {force}"));
    (as_number(&range[0]) as u32)..=(as_number(&range[1]) as u32)
}

/// A pair of force names in `[battle_line]`.
fn battle_line_pair(key: &str) -> (String, String) {
    let table = battle_line_table();
    let pair = table[key].as_array().unwrap_or_else(|| panic!("[battle_line] {key} names two forces"));
    let name = |index: usize| pair[index].as_str().expect("a force's name").to_string();
    (name(0), name(1))
}

/// A named army's battle-line efficiency against named aircraft on land at a tier, over the
/// army's `[battle_line] squadrons`: above 1, the aircraft are the better buy.
pub fn aircraft_against_army(db: &BlueprintDb, tier: u8, pair: &str) -> f64 {
    let (army, aircraft) = battle_line_pair(pair);
    air_against_battle_line(db, Ground::Land, tier, &army, &aircraft, battle_line_squadrons(&army))
}

/// A named fleet's score in its battle line at sea against an equal cost of named aircraft, the
/// mean over the fleet's `[battle_line] squadrons`, every approach and layout: the fleet's share
/// of its materiel left less the aircraft's, as `[domains.cells]` scores a fight.
pub fn fleet_against_aircraft(db: &BlueprintDb, tier: u8, pair: &str) -> f64 {
    let (fleet, aircraft) = battle_line_pair(pair);
    let db = Arc::new(db.clone());
    let squadron = domain_force(&db, &fleet);
    let sizes = battle_line_squadrons(&fleet);
    let count = sizes.clone().count() as f64;
    sizes
        .map(|size| {
            let force: Vec<Side> = squadron.iter().map(|&(unit, each)| Side { unit, count: each * size, tactics: Tactics::Hold }).collect();
            let air = force_by_cost(&db, &aircraft, force_materiel(&db, &force, tier), tier);
            -battle_line_mean(&db, Field::new(Ground::Water, tier), &force, &air, 1.0).score
        })
        .sum::<f64>()
        / count
}

/// The units as they would be at another domain base: every unit the list prices at `base_key`
/// and nothing holds, at its list price at `base`, and each dome whose group that changes priced
/// again by its group, keeping its drift from the list.
pub fn priced_at_base(db: &BlueprintDb, base_key: &str, base: f64) -> BlueprintDb {
    let mut out = db.clone();
    for unit in db.iter() {
        if crate::price_list::held_reason(&unit.key).is_some() {
            continue;
        }
        let Some(pricing) = crate::price_list::price(db, &unit.key) else { continue };
        if crate::price_list::base_key(pricing.domain, pricing.tier) != base_key {
            continue;
        }
        let id = out.id(&unit.key);
        out = scaled_to(&out, id, pricing.tier, pricing.at_base(base));
    }
    for dome in crate::price_list::shield_group_domes() {
        let (Some(before), Some(after)) = (crate::price_list::shield_price(db, &dome), crate::price_list::shield_price(&out, &dome)) else { continue };
        if (after.list / before.list - 1.0).abs() > 1e-4 {
            let id = out.id(&dome);
            let tier = out.get(id).tier;
            out = scaled_to(&out, id, tier, before.actual * after.list / before.list);
        }
    }
    out
}

/// A domain base solved so that `measure`, of the units priced at it (`priced_at_base`), comes
/// to `target`: secant steps on the logarithm of the base from `first`, until within
/// `tolerance` or after five measures, the closest kept. Returns the units, the base, and the
/// measure there.
fn solve_base(db: &BlueprintDb, base_key: &str, start: f64, first: f64, target: f64, tolerance: f64, measure: impl Fn(&BlueprintDb) -> f64) -> (BlueprintDb, f64, f64) {
    let at = |base: f64| {
        let units = priced_at_base(db, base_key, base);
        let value = measure(&units);
        (units, base, value)
    };
    let mut tried = vec![at(start)];
    let mut next = first;
    for _ in 0..4 {
        if (tried.last().expect("a trial").2 - target).abs() <= tolerance || tried.iter().any(|trial| (trial.1 - next).abs() < 1e-4) {
            break;
        }
        tried.push(at(next));
        let [(_, b0, m0), (_, b1, m1)] = [&tried[tried.len() - 2], &tried[tried.len() - 1]].map(|trial| (0, trial.1, trial.2));
        let slope = (m1 - m0) / (b1.ln() - b0.ln());
        next = if slope.abs() > 1e-9 { (b1.ln() + (target - m1) / slope).exp() } else { b1 };
    }
    tried.into_iter().min_by(|a, b| (a.2 - target).abs().total_cmp(&(b.2 - target).abs())).expect("a trial")
}

fn domain_base(key: &str) -> f64 {
    as_number(&TARGETS.parse::<toml::Table>().expect("data/balance_targets.toml is valid")["price_list"]["domain_base"][key])
}

/// The T3 air base from the battle line (`[battle_line] t3_air`, the design owner's pick): the
/// base at which the T3 strike aircraft, priced at it, are as good a buy against the standard T3
/// army as the T2 strike aircraft against the standard T2 army (`t3_air_as`), both armies
/// standing in their ranks. Returns the units, the base, the T3 efficiency and the T2 one.
pub fn derive_t3_air_base(db: &BlueprintDb) -> (BlueprintDb, f64, f64, f64) {
    let target = aircraft_against_army(db, 2, "t3_air_as");
    let start = domain_base("air_t3");
    let now = aircraft_against_army(&priced_at_base(db, "air_t3", start), 3, "t3_air");
    // Strength per cost goes as one over the cost, which goes as one over the base.
    let (units, base, efficiency) = solve_base(db, "air_t3", start, start * target / now, target, target * 0.005, |units| aircraft_against_army(units, 3, "t3_air"));
    (units, base, efficiency, target)
}

/// The navy base from the battle line (`[battle_line] navy`): the base at which the T2 fleet,
/// its ships priced at it, scores `navy_score` against an equal cost of the T2 air force at sea.
/// Returns the units, the base and the score.
pub fn derive_navy_base(db: &BlueprintDb) -> (BlueprintDb, f64, f64) {
    let target = as_number(&battle_line_table()["navy_score"]);
    let start = domain_base("navy");
    let now = fleet_against_aircraft(&priced_at_base(db, "navy", start), 2, "navy");
    let first = if now < target { start * 1.05 } else { start / 1.05 };
    solve_base(db, "navy", start, first, target, 0.005, |units| fleet_against_aircraft(units, 2, "navy"))
}

/// The `--balance-derive battle-line` report: the T3 air base and the navy base from the
/// battle line, and what they price.
pub fn derive_battle_line_report() {
    let db = BlueprintDb::standard();
    let show = |units: &BlueprintDb, keys: &[&str]| {
        for key in keys {
            let unit = units.get(units.id(key));
            println!("  {key}: {:.0} (from {:.0})", cost_at(unit, unit.tier), cost_at(db.get(db.id(key)), unit.tier));
        }
    };
    let (units, base, efficiency, target) = derive_t3_air_base(&db);
    println!("T3 air base {base:.3} (from {:.3}): T3 strike aircraft {efficiency:.3} against the T3 army, T2's {target:.3} against the T2 army", domain_base("air_t3"));
    show(&units, &["t3_heavy_gunship", "t3_air_superiority", "t3_strategic_bomber", "t3_heavy_transport"]);
    let (units, base, score) = derive_navy_base(&db);
    println!("Navy base {base:.3} (from {:.3}): the T2 fleet scores {score:+.3} against the T2 air force", domain_base("navy"));
    show(&units, &["t1_frigate", "t1_attack_sub", "t2_destroyer", "t2_cruiser", "t2_escort_corvette", "t3_battleship", "t3_battlecruiser"]);
    println!("  the T3 fleet scores {:+.3} against the T3 air force", fleet_against_aircraft(&units, 3, "navy_t3"));
}

/// The Mobile Flak's gun from the escort rule at T2: the damage a shell at which the T2 strike
/// aircraft and the standard T2 army break even, with the aircraft as they are. Their efficiency
/// goes about as the damage to a power, so each step solves for 1 on the line through the last
/// two; it stops when a step changes the damage by under one.
pub fn derive_t2_flak(base: &BlueprintDb) -> (BlueprintDb, f64) {
    let flak = base.id("t2_mobile_flak");
    let start = base.get(flak).weapon.expect("flak has a gun").damage.to_num::<f64>();
    let with = |damage: f64| base.with_unit(flak, |unit| unit.weapon.as_mut().expect("flak has a gun").damage = Fp::from_num(damage));
    let mut tried = vec![(start, strike_against_army(base, 2))];
    // A first guess: efficiency as one over the damage to the power 1.3, as measured (2026-11-27).
    let mut damage = (start * tried[0].1.powf(1.0 / 1.3)).round();
    for _ in 0..4 {
        if tried.iter().any(|&(tried_damage, _)| tried_damage == damage) {
            break;
        }
        let efficiency = strike_against_army(&with(damage), 2);
        tried.push((damage, efficiency));
        let [(d0, e0), (d1, e1)] = [tried[tried.len() - 2], tried[tried.len() - 1]];
        let slope = (e1.ln() - e0.ln()) / (d1.ln() - d0.ln());
        damage = if slope < 0.0 { (d1.ln() - e1.ln() / slope).exp().round() } else { d1 };
    }
    let (damage, efficiency) = tried.iter().copied().min_by(|a, b| (a.1 - 1.0).abs().total_cmp(&(b.1 - 1.0).abs())).expect("a trial");
    (with(damage), efficiency)
}

/// The `--balance-derive air-bases` report: the Mobile Flak's gun from the escort rule at T2.
pub fn derive_air_bases_report() {
    let base = BlueprintDb::standard();
    let (army, strike) = escort_forces(2);
    println!("T2 strike aircraft ({strike}) against the standard T2 army ({army}), as they are: efficiency {:.3}", strike_against_army(&base, 2));
    let (db, efficiency) = derive_t2_flak(&base);
    println!("  t2_mobile_flak: damage {} (efficiency at it {efficiency:.3})", db.get(db.id("t2_mobile_flak")).weapon.expect("flak has a gun").damage);
}

// Counters, priced by the duel they win ([counters] counter_squadrons, 2026-11-27).

fn counters_table() -> toml::Value {
    TARGETS.parse::<toml::Table>().expect("data/balance_targets.toml is valid").remove("counters").expect("[counters]")
}

/// `first`, at `squadrons_of_it` squadrons, against an equal cost of `second`, on a field: the
/// score, as `[counters]` scores it.
pub fn score_against(db: &Arc<BlueprintDb>, field: Field, first: &str, squadrons_of_it: f64, second: &str) -> f64 {
    let squadron = domain_force(db, first);
    let one = force_materiel(db, &squadrons(db, &squadron, 0.0, field.tier, Tactics::AttackMove), field.tier);
    let a = squadrons(db, &squadron, one * squadrons_of_it, field.tier, Tactics::AttackMove);
    let b = squadrons(db, &domain_force(db, second), force_materiel(db, &a, field.tier), field.tier, Tactics::AttackMove);
    mirrored_on(db, field, &a, &b, 1.0).score
}

/// Torpedo Boats against an equal cost of submarines on T2 water: the mean score over fleets of
/// `[counters] counter_squadrons` squadrons of them.
pub fn torpedo_boats_against_submarines(db: &Arc<BlueprintDb>) -> f64 {
    let sizes: Vec<f64> = counters_table()["counter_squadrons"].as_array().expect("[counters] counter_squadrons").iter().map(as_number).collect();
    sizes.iter().map(|&n| score_against(db, Field::new(Ground::Water, 2), "t2_torpedo_boats", n, "t1_submarines")).sum::<f64>() / sizes.len() as f64
}

/// The torpedo bombers it takes to break the T2 fleet (two squadrons of it), as a multiple of its
/// cost: the fleet's break-even efficiency against them.
pub fn torpedo_bombers_to_break_t2_fleet(db: &Arc<BlueprintDb>) -> f64 {
    let fleet = domain_force(db, "t2_fleet");
    let one = force_materiel(db, &squadrons(db, &fleet, 0.0, 2, Tactics::AttackMove), 2);
    let fleet = squadrons(db, &fleet, one * 2.0, 2, Tactics::AttackMove);
    let bombers = squadrons(db, &domain_force(db, "t2_torpedo_bombers"), force_materiel(db, &fleet, 2), 2, Tactics::AttackMove);
    break_even_forces(db, Field::new(Ground::Water, 2), &fleet, &bombers).efficiency
}

/// Bisects a unit's cost at T2, between a fifth under and a fifth over what it costs, for the
/// edge where `meets` turns: the last cost that meets it when `meets_below` (it holds at low cost),
/// else the first. Returns the copy at that cost and the measure there.
fn bisect_cost(base: &BlueprintDb, key: &str, meets_below: bool, measure: impl Fn(&Arc<BlueprintDb>) -> f64, meets: impl Fn(f64) -> bool) -> (BlueprintDb, f64) {
    bisect_cost_at(base, key, 2, meets_below, measure, meets)
}

/// `bisect_cost`, with costs at a tier's prices.
fn bisect_cost_at(base: &BlueprintDb, key: &str, tier: u8, meets_below: bool, measure: impl Fn(&Arc<BlueprintDb>) -> f64, meets: impl Fn(f64) -> bool) -> (BlueprintDb, f64) {
    let id = base.id(key);
    let now = cost_at(base.get(id), tier);
    let at = |cost: f64| {
        let db = Arc::new(scaled_to(base, id, tier, cost));
        let value = measure(&db);
        (db, value)
    };
    let (mut low, mut high) = (now * 0.8, now * 1.2);
    let (mut good, bad) = if meets_below { (at(low), at(high).1) } else { (at(high), at(low).1) };
    assert!(meets(good.1) && !meets(bad), "{key}: the edge is not between {low:.0} and {high:.0} ({:.3}, {bad:.3})", good.1);
    // To about a unit of mass: each step halves the range.
    while high - low > now / 400.0 {
        let middle = (low + high) / 2.0;
        let trial = at(middle);
        let met = meets(trial.1);
        // The edge is above the middle when the middle is on the low end's side of it.
        if met == meets_below {
            low = middle;
        } else {
            high = middle;
        }
        if met {
            good = trial;
        }
    }
    (Arc::try_unwrap(good.0).unwrap_or_else(|db| (*db).clone()), good.1)
}

/// The Torpedo Boat's cost from the duel it wins: the most it can cost and still keep the top of
/// `[counters] winner_survivors` against an equal cost of submarines, as the mean over fleet sizes.
pub fn derive_torpedo_boat(base: &BlueprintDb) -> (BlueprintDb, f64) {
    let top = counters_table()["winner_survivors"].as_array().expect("[counters] winner_survivors").iter().map(as_number).fold(f64::MIN, f64::max);
    bisect_cost(base, "t2_torpedo_boat", true, torpedo_boats_against_submarines, |score| score >= top)
}

/// The Torpedo Bomber's cost from the duel it wins: the least it can cost and still need the
/// least of `[domains.cells] torpedo_bombers_to_break_fleet_t2` times the T2 fleet's cost to break it.
pub fn derive_torpedo_bomber(base: &BlueprintDb) -> (BlueprintDb, f64) {
    let least = domain_cell("torpedo_bombers_to_break_fleet_t2").as_array().expect("a band").iter().map(as_number).fold(f64::MAX, f64::min);
    bisect_cost(base, "t2_torpedo_bomber", false, torpedo_bombers_to_break_t2_fleet, |multiple| multiple >= least)
}

/// The `--balance-derive counters` report: the counters' costs from the duels they win.
pub fn derive_counters_report() {
    let base = BlueprintDb::standard();
    let db = Arc::new(base.clone());
    println!("Torpedo Boats against an equal cost of submarines, as they are: mean score {:+.3}", torpedo_boats_against_submarines(&db));
    println!("Torpedo bombers to break the T2 fleet, as they are: {:.3} times its cost", torpedo_bombers_to_break_t2_fleet(&db));
    for (key, (derived, measure)) in [("t2_torpedo_boat", derive_torpedo_boat(&base)), ("t2_torpedo_bomber", derive_torpedo_bomber(&base))] {
        let unit = derived.get(derived.id(key));
        println!("  {key}: cost {:.1}, from {:.1} (mass = {}, energy = {}, build_time = {}; the duel there {measure:.3})", cost_at(unit, 2), cost_at(base.get(base.id(key)), 2), unit.mass, unit.energy, unit.build_time);
    }
}

/// The most a point defense can cost and still hold (`[premiums]`
/// point_defense_holds_against_line_units): three of it against that many times their cost in
/// their tier's line unit attack-moving into them, found within a fifth of `start`.
pub fn point_defense_at_its_most(base: &BlueprintDb, tier: u8, key: &str, start: f64) -> (BlueprintDb, f64) {
    let times = as_number(&premium("point_defense_holds_against_line_units"));
    let from = scaled_to(base, base.id(key), tier, start);
    let (db, _) = bisect_cost_at(&from, key, tier, true, |db| if defenses_against_line(db, tier, times).broken { 0.0 } else { 1.0 }, |held| held > 0.5);
    let cost = cost_at(db.get(db.id(key)), tier);
    (db, cost)
}

/// The T3 Heavy Point Defense at its most, from its cost.
pub fn heavy_point_defense_at_its_most(base: &BlueprintDb) -> (BlueprintDb, f64) {
    let now = cost_at(base.get(base.id("t3_heavy_point_defense")), 3);
    point_defense_at_its_most(base, 3, "t3_heavy_point_defense", now)
}

/// A point defense's cost (2026-11-27): the list's price, or the most it can cost and still hold
/// where that is less (the T1 one, whose damage the list prices above what the hold allows).
pub fn point_defense_cost(base: &BlueprintDb, tier: u8, key: &str) -> f64 {
    let times = as_number(&premium("point_defense_holds_against_line_units"));
    let list = crate::price_list::price(base, key).expect("the list prices point defenses").list;
    let at_list = Arc::new(scaled_to(base, base.id(key), tier, list));
    if defenses_against_line(&at_list, tier, times).broken {
        point_defense_at_its_most(base, tier, key, list).1
    } else {
        list
    }
}

/// `[price_list] structure_closing_share` (2026-11-27): the closing share at which the list
/// prices the T3 Heavy Point Defense at the most it can cost and still hold.
pub fn derive_structure_closing_share(base: &BlueprintDb) -> f64 {
    let (_, most) = heavy_point_defense_at_its_most(base);
    // Past where its reach is more than the line closes in a time to kill, the list cannot price it.
    let list = |closing: f64| crate::price_list::price_at_structure_closing(base, "t3_heavy_point_defense", closing).map_or(f64::INFINITY, |pricing| pricing.list);
    let (mut low, mut high) = (0.2, 0.95);
    assert!(list(low) < most && list(high) > most, "the closing share is not between {low} and {high}");
    while high - low > 1e-4 {
        let middle = (low + high) / 2.0;
        if list(middle) < most {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low + high) / 2.0
}

/// The `--balance-derive structures` report: the closing share, and each point defense's price
/// by it, against its cost, and its cost: the list's, or the most it can cost and still hold.
pub fn derive_structures_report() {
    let base = BlueprintDb::standard();
    let (_, most) = heavy_point_defense_at_its_most(&base);
    println!("The T3 Heavy Point Defense at its most, still holding: {most:.1}");
    let closing = derive_structure_closing_share(&base);
    println!("structure_closing_share = {closing:.4} (the targets have {:.4})", as_number(&price_list_value("structure_closing_share")));
    for (tier, key) in [(1u8, "t1_point_defense"), (2, "t2_point_defense"), (3, "t3_heavy_point_defense")] {
        let pricing = crate::price_list::price_at_structure_closing(&base, key, closing).expect("the list prices point defenses");
        let cost = point_defense_cost(&base, tier, key);
        let db = scaled_to(&base, base.id(key), tier, cost);
        let unit = db.get(db.id(key));
        println!(
            "  {key}: list {:.1}, cost {cost:.1}, from {:.1} ({:+.1}%): mass = {}, energy = {}, build_time = {}",
            pricing.list,
            pricing.actual,
            (cost / pricing.actual - 1.0) * 100.0,
            unit.mass,
            unit.energy,
            unit.build_time
        );
    }
}

fn price_list_value(key: &str) -> toml::Value {
    let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    table["price_list"][key].clone()
}

// Fights between domains (BALANCE.md, 2026-11-24): `[domains]` in the targets.

/// Three T2 torpedo launchers on the water just off the shore ground's land, for ships to close
/// on: the answer to a fleet at a coast.
pub const TORPEDO_POSITION: Position = &[("t2_torpedo_launcher", 106, 124), ("t2_torpedo_launcher", 106, 130), ("t2_torpedo_launcher", 109, 127)];
/// The same at T1: three T1 torpedo launchers off the shore.
pub const T1_TORPEDO_POSITION: Position = &[("t1_torpedo_launcher", 107, 124), ("t1_torpedo_launcher", 107, 131), ("t1_torpedo_launcher", 110, 127)];

/// What a weapon is for, for a role's strength: guns and missiles at things on the ground or the
/// water, guns at aircraft, or torpedoes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Purpose {
    Surface,
    Air,
    Naval,
}

impl Purpose {
    fn of(targets: crate::sim::blueprints::Targets) -> [bool; 3] {
        use crate::sim::blueprints::Targets;
        match targets {
            Targets::Ground => [true, false, false],
            Targets::Air => [false, true, false],
            Targets::Both => [true, true, false],
            Targets::Naval => [false, false, true],
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Surface => 0,
            Self::Air => 1,
            Self::Naval => 2,
        }
    }

    /// The damage a role is measured by: anti-air for what hunts aircraft, torpedoes for what
    /// hunts ships from below, and guns and missiles for the rest.
    pub fn of_role(key: &str) -> Self {
        match key {
            "t2_mobile_flak" | "t1_mobile_aa" | "t1_interceptor" | "t3_air_superiority" => Self::Air,
            "t1_attack_sub" | "t2_torpedo_boat" | "t2_torpedo_bomber" | "t1_torpedo_launcher" | "t2_torpedo_launcher" => Self::Naval,
            _ => Self::Surface,
        }
    }
}

/// A unit's damage a second, and longest reach, with all its weapons for a purpose.
pub fn purpose_weapons(unit: &Blueprint, purpose: Purpose) -> (f64, f64) {
    unit.weapon.iter().chain(&unit.extra_weapons).filter(|weapon| Purpose::of(weapon.targets)[purpose.index()]).fold((0.0, 0.0), |(dps, reach), weapon| {
        let per_second = damage_per_second(weapon);
        (dps + per_second, reach.max(weapon.range.to_num::<f64>()))
    })
}

/// A unit's strength per cost at its tier, by the damage its role is for, as a multiple of the
/// anchor's: the measure `[domains] strength_over_anchor` gives the inspiration's ratios in.
pub fn purpose_strength(db: &BlueprintDb, key: &str) -> f64 {
    let measure = |key: &str, purpose: Purpose| {
        let unit = db.get(db.id(key));
        let (dps, _) = purpose_weapons(unit, purpose);
        (dps * unit.health.to_num::<f64>()).sqrt() / cost_at(unit, unit.tier)
    };
    measure(key, Purpose::of_role(key)) / measure("t1_medium_tank", Purpose::Surface)
}

/// A unit's strength per cost against aircraft at a tier's prices, √(anti-air damage a second ×
/// health) / cost: how `[premiums] static_anti_air_over_mobile` weighs anti-air.
pub fn anti_air_strength(db: &BlueprintDb, key: &str, tier: u8) -> f64 {
    let unit = db.get(db.id(key));
    let (dps, _) = purpose_weapons(unit, Purpose::Air);
    (dps * unit.health.to_num::<f64>()).sqrt() / cost_at(unit, tier)
}

/// Whether three point defenses of a tier hold against `times` their cost in that tier's line
/// unit attack-moving into them, and the siege, for `[premiums] point_defense_holds_against_line_units`.
pub fn defenses_against_line(db: &Arc<BlueprintDb>, tier: u8, times: f64) -> SiegeResult {
    let (line, defenses) = match tier {
        1 => ("t1_medium_tank", T1_DEFENSES),
        2 => ("t2_heavy_tank", T2_DEFENSES),
        _ => ("t3_heavy_walker", T3_DEFENSES),
    };
    let attackers = squadrons(db, &[(db.id(line), 1)], position_materiel_at(db, defenses, tier) * times, tier, Tactics::AttackMove);
    siege_on(db, Field::new(Ground::Land, tier), &attackers, defenses)
}

/// The materiel a position costs at a tier's prices.
pub fn position_materiel_at(db: &BlueprintDb, position: Position, tier: u8) -> f64 {
    position.iter().map(|&(key, _, _)| cost_at(db.get(db.id(key)), tier)).sum()
}

/// A force named in `[domains.forces]`, by squadron.
pub fn domain_force(db: &BlueprintDb, name: &str) -> Vec<(BlueprintId, u32)> {
    let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    let force = table["domains"]["forces"][name].as_table().unwrap_or_else(|| panic!("[domains.forces] has {name}"));
    let mut squadron: Vec<(BlueprintId, u32)> = force.iter().map(|(key, count)| (db.id(key), count.as_integer().expect("a count") as u32)).collect();
    squadron.sort_by_key(|&(unit, _)| unit.0);
    squadron
}

fn premium(key: &str) -> toml::Value {
    let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    table["premiums"][key].clone()
}

fn domain_cell(key: &str) -> toml::Value {
    let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    table["domains"]["cells"][key].clone()
}

fn as_number(value: &toml::Value) -> f64 {
    value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).expect("a number")
}

/// One row of the fights table.
struct Cell {
    name: String,
    target: String,
    measured: String,
    met: Option<bool>,
}

/// Plays and prints every fight between domains, and how each compares with its target: the
/// tables of reports/balance/<date>-domains.md (`--balance-derive domains`).
pub fn derive_domains_report() {
    let db = Arc::new(BlueprintDb::standard());
    let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    let domains = &table["domains"];

    // Strength per cost against the inspiration's ratios.
    let band = domains["strength_band"].as_array().expect("a band").iter().map(as_number).collect::<Vec<f64>>();
    println!("## Strength per cost, against the inspiration's ratios\n");
    println!("Each role's strength per cost at its tier, √(damage a second × health) over cost, by the damage the role is for, as a multiple of the anchor's. Within {:.2}–{:.2} of the inspiration's ratio counts as met.\n", band[0], band[1]);
    println!("| Unit | Measured by | Ours | Inspiration | Ours over it | Met |");
    println!("| :-- | :-- | --: | --: | --: | :-- |");
    let mut ratios: Vec<(&String, &toml::Value)> = domains["strength_over_anchor"].as_table().expect("a table").iter().collect();
    ratios.sort_by(|a, b| a.0.cmp(b.0));
    for (key, inspiration) in ratios {
        let inspiration = as_number(inspiration);
        let ours = purpose_strength(&db, key);
        let over = ours / inspiration;
        let purpose = match Purpose::of_role(key) {
            Purpose::Surface => "guns and missiles",
            Purpose::Air => "anti-air",
            Purpose::Naval => "torpedoes",
        };
        println!("| {} | {purpose} | {ours:.2} | {inspiration:.2} | {over:.2} | {} |", db.get(db.id(key)).name, if (band[0]..=band[1]).contains(&over) { "yes" } else { "**no**" });
    }

    // The fights.
    let mut cells: Vec<Cell> = Vec::new();
    let force = |name: &str, materiel: f64, tier: u8| squadrons(&db, &domain_force(&db, name), materiel, tier, Tactics::AttackMove);
    let squadron_cost = |name: &str, tier: u8| force_materiel(&db, &squadrons(&db, &domain_force(&db, name), 0.0, tier, Tactics::AttackMove), tier);
    let score_cell = |cells: &mut Vec<Cell>, name: &str, field: Field, first: &str, first_squadrons: f64, second: &str, at_least: Option<f64>, at_most: Option<f64>| {
        let a = force(first, squadron_cost(first, field.tier) * first_squadrons, field.tier);
        let materiel = force_materiel(&db, &a, field.tier);
        let b = force(second, materiel, field.tier);
        let result = mirrored_on(&db, field, &a, &b, 1.0);
        let ratio = force_materiel(&db, &b, field.tier) / materiel;
        let met = match (at_least, at_most) {
            (Some(low), _) => Some(result.score >= low),
            (_, Some(high)) => Some(result.score <= high),
            _ => None,
        };
        let target = match (at_least, at_most) {
            (Some(low), _) => format!("at least {low:+.2}"),
            (_, Some(high)) => format!("at most {high:+.2}"),
            _ => "(measured)".to_string(),
        };
        cells.push(Cell { name: name.to_string(), target, measured: format!("{:+.2} in {:.0} s{} ({:.0} materiel against {:.2} times it)", result.score, result.seconds, if result.finished { "" } else { ", unfinished" }, materiel, ratio), met });
    };
    let number = |key: &str| as_number(&domain_cell(key));
    let water = |tier| Field::new(Ground::Water, tier);
    score_cell(&mut cells, "T2 fleet against an equal cost of the T2 air force, over water", water(2), "t2_fleet", 2.0, "t2_air_force", Some(number("fleet_beats_air_force_t2")), None);
    score_cell(&mut cells, "T3 fleet against an equal cost of the T3 air force, over water", water(3), "t3_fleet", 1.0, "t3_air_force", Some(number("fleet_beats_air_force_t3")), None);
    score_cell(&mut cells, "Battleship-heavy T3 fleet against an equal cost of the T3 air force, over water", water(3), "t3_battleship_heavy_fleet", 2.0, "t3_air_force", None, None);
    score_cell(&mut cells, "T2 fleet with no anti-air against torpedo bombers", water(2), "t2_fleet_without_anti_air", 4.0, "t2_torpedo_bombers", None, Some(number("fleet_without_anti_air_loses_to_torpedo_bombers")));
    score_cell(&mut cells, "Submarines against a T2 fleet with no anti-submarine weapon", water(2), "t1_submarines", 24.0, "t2_fleet_without_anti_submarine", Some(number("submarines_beat_fleet_without_anti_submarine")), None);
    score_cell(&mut cells, "Destroyers alone against an equal cost of submarines", water(2), "t2_destroyers", 4.0, "t1_submarines", None, Some(-number("submarines_beat_destroyers")));
    score_cell(&mut cells, "Torpedo Boats against an equal cost of submarines", water(2), "t2_torpedo_boats", 4.0, "t1_submarines", Some(number("torpedo_boats_beat_submarines")), None);
    score_cell(&mut cells, "T2 fleet against an equal cost of hovercraft, on water", water(2), "t2_fleet", 2.0, "t2_hovercraft", Some(-number("hovercraft_loses_to_fleet")), None);
    score_cell(&mut cells, "T1 fleet against an equal cost of the T1 air force, over water", water(1), "t1_fleet", 4.0, "t1_air_force", None, None);
    score_cell(&mut cells, "Frigates against an equal cost of T1 bombers, over water", water(1), "t1_frigates", 12.0, "t1_bombers", None, None);
    // Air against the standard T2 army, even by the escort rule ([air] escort_forces).
    {
        let (low, high) = target_number_range("air", "escort_efficiency");
        let efficiency = strike_against_army(&db, 2);
        cells.push(Cell {
            name: "T2 strike aircraft against the standard T2 land army, about a third anti-air, by break-even".to_string(),
            target: format!("{low:.2}–{high:.2}"),
            measured: format!("{efficiency:.2} (the aircraft's strength per materiel over the army's)"),
            met: Some((low..=high).contains(&efficiency)),
        });
    }
    // The torpedo bombers it takes to break a fleet, at T2 and a massed T3 one.
    for (tier, fleet_name, squadrons_of_it, cell) in [(2, "t2_fleet", 2.0, "torpedo_bombers_to_break_fleet_t2"), (3, "t3_fleet", 2.0, "torpedo_bombers_to_break_fleet_t3"), (3, "t3_battleship_heavy_fleet", 2.0, "battleship_heavy_fleet_breaks_to_torpedo_bombers_at_most")] {
        let fleet = force(fleet_name, squadron_cost(fleet_name, tier) * squadrons_of_it, tier);
        let materiel = force_materiel(&db, &fleet, tier);
        let bombers = force("t2_torpedo_bombers", materiel, tier);
        let even = break_even_forces(&db, water(tier), &fleet, &bombers);
        // A band, or for the battleship-heavy fleet a ceiling.
        let band = match domain_cell(cell).as_array() {
            Some(band) => band.iter().map(as_number).collect::<Vec<f64>>(),
            None => vec![0.0, number(cell)],
        };
        let which = if fleet_name == "t3_battleship_heavy_fleet" { "battleship-heavy T3 fleet".to_string() } else { format!("T{tier} fleet") };
        cells.push(Cell {
            name: format!("Torpedo bombers it takes to break the {which}, as a multiple of its cost"),
            target: if band[0] > 0.0 { format!("{:.1}–{:.1}", band[0], band[1]) } else { format!("at most {:.1}", band[1]) },
            measured: format!("{:.2}{} (the fleet's strength per materiel over theirs; even after {:.0} s)", even.efficiency, if even.at_limit { ", at the search's limit" } else { "" }, even.seconds),
            met: Some((band[0]..=band[1]).contains(&even.efficiency)),
        });
    }
    // The hovercraft on land, against the Heavy Tank.
    {
        let hover = force("t2_hovercraft", 2_400.0, 2);
        let tanks = force("t2_heavy_tanks", force_materiel(&db, &hover, 2), 2);
        let even = break_even_forces(&db, Field::new(Ground::Land, 2), &hover, &tanks);
        let band = premium("hovercraft_over_heavy_tank_on_land").as_array().expect("a band").iter().map(as_number).collect::<Vec<f64>>();
        cells.push(Cell {
            name: "Hovercraft on land, strength per materiel over the Heavy Tank's (amphibious, `[premiums]`)".to_string(),
            target: format!("{:.2}–{:.2}", band[0], band[1]),
            measured: format!("{:.2}", even.efficiency),
            met: Some((band[0]..=band[1]).contains(&even.efficiency)),
        });
    }
    // What cannot move is worth more for its cost (`[premiums]`): point defenses against the line
    // unit of their tier, and static anti-air against mobile.
    {
        let times = as_number(&premium("point_defense_holds_against_line_units"));
        for tier in 1..=3u8 {
            let result = defenses_against_line(&db, tier, times);
            cells.push(Cell {
                name: format!("Three T{tier} point defenses against {times} times their cost in the T{tier} line unit"),
                target: "hold".to_string(),
                measured: format!("{} after {:.0} s, the attackers losing {:.0}%", if result.broken { "broken" } else { "held" }, result.seconds, result.losses * 100.0),
                met: Some(!result.broken),
            });
        }
        let band = premium("static_anti_air_over_mobile").as_array().expect("a band").iter().map(as_number).collect::<Vec<f64>>();
        for (tier, fixed, mobile) in [(1u8, "t1_aa_turret", "t1_mobile_aa"), (2, "t2_flak", "t2_mobile_flak")] {
            let ratio = anti_air_strength(&db, fixed, tier) / anti_air_strength(&db, mobile, tier);
            cells.push(Cell {
                name: format!("{}'s strength per cost against aircraft over the {}'s", db.get(db.id(fixed)).name, db.get(db.id(mobile)).name),
                target: format!("{:.2}–{:.2}", band[0], band[1]),
                measured: format!("{ratio:.2}"),
                met: Some((band[0]..=band[1]).contains(&ratio)),
            });
        }
    }
    // Coastal reach, and breaking a coastal position.
    {
        let edge = target_number("siege", "outrange_defense_min");
        let seconds_max = number("coastal_seconds_max");
        let losses_max = number("coastal_losses_max");
        let ships = domain_cell("coastal_ships");
        let mut pairs: Vec<(&String, &toml::Value)> = ships.as_table().expect("a table").iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        for (ship, defense) in pairs {
            let defense = defense.as_str().expect("a defense's key");
            let (_, reach) = purpose_weapons(db.get(db.id(ship)), Purpose::Surface);
            let (_, guarded) = purpose_weapons(db.get(db.id(defense)), Purpose::Surface);
            let over = reach / guarded - 1.0;
            cells.push(Cell {
                name: format!("{} outranges the {}", db.get(db.id(ship)).name, db.get(db.id(defense)).name),
                target: format!("by at least {:.0}%", edge * 100.0),
                measured: format!("by {:.0}%", over * 100.0),
                met: Some(over >= edge),
            });
            let tier = db.get(db.id(ship)).tier;
            let position = if tier >= 3 { T3_POSITION } else { T2_POSITION };
            // A count of ships, not an equal cost: one never breaks its tier's dome, by design
            // (2026-11-25), and whole ships at an equal cost come to one.
            let count = number("coastal_ship_count") as u32;
            let attackers = [Side { unit: db.id(ship), count, tactics: Tactics::AttackMove }];
            let result = siege_on(&db, Field::new(Ground::Shore, tier), &attackers, position);
            let bought = force_materiel(&db, &attackers, tier) / position_materiel_at(&db, position, tier);
            cells.push(Cell {
                name: format!("{count} of the {} break a coastal position of its tier", db.get(db.id(ship)).name),
                target: format!("within {seconds_max:.0} s, losing at most {:.0}%", losses_max * 100.0),
                measured: format!("{} in {:.0} s, at {bought:.2} times its cost, losing {:.0}%", if result.broken { "broken" } else { "not broken" }, result.seconds, result.losses * 100.0),
                met: Some(result.broken && result.seconds <= seconds_max && result.losses <= losses_max),
            });
        }
        for (tier, ship, position, cell, label) in [
            (2, "t2_destroyer", TORPEDO_POSITION, "destroyers_break_torpedo_launchers", "destroyers closing on T2 torpedo launchers"),
            (1, "t1_frigate", T1_TORPEDO_POSITION, "frigates_break_t1_torpedo_launchers", "frigates closing on T1 torpedo launchers"),
        ] {
            let materiel = position_materiel_at(&db, position, tier);
            let ships = squadrons(&db, &[(db.id(ship), 1)], materiel, tier, Tactics::AttackMove);
            let result = siege_on(&db, Field::new(Ground::Shore, tier), &ships, position);
            let breaks = domain_cell(cell).as_bool().expect("true or false");
            cells.push(Cell {
                name: format!("An equal cost of {label}"),
                target: if breaks { "breaks them" } else { "does not break them" }.to_string(),
                measured: format!("{} in {:.0} s, losing {:.0}%", if result.broken { "broke them" } else { "did not break them" }, result.seconds, result.losses * 100.0),
                met: Some(result.broken == breaks),
            });
        }
    }
    println!("\n## Fights between domains\n");
    println!("Scored as [counters] scores a fight: the first force's share of its materiel left, less the second's, from -1 to 1, played both ways round and over four starting layouts (on the shore, the ships are always at sea).\n");
    println!("| Fight | Target | Measured | Met |");
    println!("| :-- | :-- | :-- | :-- |");
    for cell in &cells {
        let met = match cell.met {
            Some(true) => "yes",
            Some(false) => "**no**",
            None => "-",
        };
        println!("| {} | {} | {} | {met} |", cell.name, cell.target, cell.measured);
    }

    // What fielding each domain costs.
    println!("\n## What fielding each domain costs\n");
    println!("Each factory's cost at its tier's prices and its build power, over the land factory's of the same tier.\n");
    println!("| Tier | Air factory's cost | Its build power | Naval factory's cost | Its build power |");
    println!("| :-- | --: | --: | --: | --: |");
    for (label, tier, land, air, naval) in [
        ("T1", 1, "land_factory_t1", "air_factory", "naval_factory"),
        ("T2 headquarters", 2, "land_factory_t2", "air_factory_t2", "naval_factory_t2"),
        ("T3 headquarters", 3, "land_factory_t3", "air_factory_t3", "naval_factory_t3"),
        ("T2 support", 2, "land_factory_t2_support", "air_factory_t2_support", "naval_factory_t2_support"),
        ("T3 support", 3, "land_factory_t3_support", "air_factory_t3_support", "naval_factory_t3_support"),
    ] {
        let (Some(land), Some(air), Some(naval)) = (db.find(land), db.find(air), db.find(naval)) else { continue };
        let price = |id: BlueprintId| cost_at(db.get(id), tier);
        let power = |id: BlueprintId| db.get(id).build_power.to_num::<f64>();
        println!("| {label} | {:.2} | {:.2} | {:.2} | {:.2} |", price(air) / price(land), power(air) / power(land), price(naval) / price(land), power(naval) / power(land));
    }
    println!("\nTime to build each unit at its factory, build time over build power, as a multiple of the Medium Tank's at a T1 land factory.\n");
    println!("| Unit | Factory | Time |");
    println!("| :-- | :-- | --: |");
    let seconds = |unit: &str, factory: &str| db.get(db.id(unit)).build_time.to_num::<f64>() / db.get(db.id(factory)).build_power.to_num::<f64>();
    let anchor_seconds = seconds("t1_medium_tank", "land_factory_t1");
    for (unit, factory) in [
        ("t1_frigate", "naval_factory"),
        ("t2_destroyer", "naval_factory_t2"),
        ("t2_cruiser", "naval_factory_t2"),
        ("t3_battleship", "naval_factory_t3"),
        ("t1_bomber", "air_factory"),
        ("t2_gunship", "air_factory_t2"),
        ("t2_torpedo_bomber", "air_factory_t2"),
        ("t3_strategic_bomber", "air_factory_t3"),
        ("t2_heavy_tank", "land_factory_t2"),
        ("t3_heavy_walker", "land_factory_t3"),
    ] {
        println!("| {} | {} | {:.1} |", db.get(db.id(unit)).name, db.get(db.id(factory)).name, seconds(unit, factory) / anchor_seconds);
    }
}
