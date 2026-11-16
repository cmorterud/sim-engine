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
    /// The mirrored duel score against the anchor, both attack-moving, that the strength
    /// multiplier is solved for: the counter matrix's cell. `None` keeps `strength`.
    pub score_against_anchor: Option<f64>,
    pub strength: f64,
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
        score_against_anchor: None,
        strength: 1.0,
    };
    vec![
        base.clone(),
        // Fast and fragile, for engineers and extractors; line units beat it, keeping about half.
        Role { unit: "t1_light_walker", role: "raider", cost: 24.0, time_to_kill: 0.5, range: 0.6, speed: 1.8, sight_over_range: 1.6, flight_seconds: 0.5, reload_seconds: 0.5, radius: 7.0, score_against_anchor: Some(-0.55), ..base.clone() },
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
            score_against_anchor: Some(0.5),
            ..base.clone()
        },
        // Sight, not a fighter: cheap, fast, and sees far.
        Role { unit: "t1_land_scout", role: "scout", cost: 12.0, time_to_kill: 0.4, range: 0.6, speed: 2.0, sight_over_range: 3.3, flight_seconds: 0.4, reload_seconds: 1.0, radius: 6.0, strength: 0.5, ..base },
    ]
}

/// Equal materiel for every T1 cost above: 12 anchors, 20 raiders, 10 artillery, 40 scouts.
const PRICING_MATERIEL: f64 = 480.0;

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

/// Solves a role's strength multiplier for its score against the anchor: the duel score rises
/// with strength, so bisection over multipliers from 1/8 to 8 finds it.
pub fn price(db: &BlueprintDb, anchor: &Anchor, role: &Role) -> f64 {
    let Some(target) = role.score_against_anchor else { return role.strength };
    let (mut low, mut high) = (-3.0f64, 3.0f64);
    let anchor_id = db.id("t1_medium_tank");
    let count = |cost: f64| (PRICING_MATERIEL / cost).round() as u32;
    for _ in 0..10 {
        let middle = (low + high) / 2.0;
        let trial = Arc::new(apply(db, anchor, role, middle.exp2()));
        let unit = Side { unit: trial.id(role.unit), count: count(role.cost), tactics: Tactics::AttackMove };
        let tanks = Side { unit: anchor_id, count: count(anchor.materiel), tactics: Tactics::AttackMove };
        if mirrored(&trial, unit, tanks, 1.0).score > target {
            high = middle;
        } else {
            low = middle;
        }
    }
    ((low + high) / 2.0).exp2()
}

/// The `--balance-derive` report: T1 land's first-pass numbers from the targets, with the duels
/// that priced them and the counter-matrix cells they should meet.
pub fn derive_report() {
    let anchor = Anchor::from_targets();
    let roles = t1_land_roles();
    let mut db = apply(&BlueprintDb::standard(), &anchor, &roles[0], 1.0);
    let mut strengths = Vec::new();
    for role in &roles {
        let strength = price(&db, &anchor, role);
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
            "             weapon = {{ range = {}, reload_ticks = {}, projectile_speed = {}, damage = {}, splash_radius = {}, spread = {}{} }}",
            weapon.range,
            weapon.reload_ticks,
            weapon.projectile_speed,
            weapon.damage,
            weapon.splash_radius,
            weapon.spread,
            if weapon.artillery { format!(", artillery = true, min_range = {}", weapon.min_range) } else { String::new() },
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
    println!("  massed raiders against artillery:   {:+.2} attack-moving (artillery keeps at least 0.3), {:+.2} with micro (raiders keep at least 0.3)", raiders_plain.score, raiders_micro.score);
    let raid = mirrored(
        &db,
        Side { unit: db.id("t1_light_walker"), count: 4, tactics: Tactics::AttackMove },
        Side { unit: db.id("t1_light_artillery"), count: 2, tactics: Tactics::AttackMove },
        1.0,
    );
    println!("  a raid, 4 raiders against 2 artillery: {:+.2} (raiders win)", raid.score);
    let ten = mirrored(&db, Side { count: 10, ..anchor_side }, Side { count: 10, ..anchor_side }, 1.0);
    println!("  ten anchors against ten:            {:.1} s (targets: 25-45 s)", ten.seconds);
}
