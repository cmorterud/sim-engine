use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;
use std::sync::Arc;
use twox_hash::XxHash64;

use super::blueprints::{BlueprintDb, Res, UnitClass};
use super::air::{bomb_lands_on, BOMB_GRAVITY};
use super::combat::{dome_entry, entry_hit, firing_solution_within, missile_velocity, probe_surface, probe_terrain, shot_is_clear, GRAVITY};
use super::commands::SimCommand;
use super::components::{BuildTask, AntiAir, Artillery, BombBay, Falling, Flight, Homing,
    Blueprinted, Builder, Combat, Construction, Factory, Footprint, Health, MoveClass, Order, OrderKind, OrderQueue, Patrol,
    Intel, Launcher, Missile, Stunning, MissileDefense, Upgrading, Discharged, DischargeOrder, DischargeWeapon, Paused, PlayerId, Position, Projectile, Shield, SimId, Team, Turning, Turret, UnitStats, Velocity, Veterancy,
    Settling, Wreck, WreckKind, rotate_toward, LeavingFactory, Transport,
};
use super::config::{
    SimConfig, TARGET_SEARCH_INTERVAL, TERRAIN_CELL_UNITS, WRECK_AIR_KEEP_SIXTEENTHS, WRECK_HEALTH_PERMILLE, WRECK_MASS_PERMILLE, WRECK_SINK_PULL_RAW, WRECK_SINK_SPEED_RAW,
    WRECK_SUNK_PERMILLE, WRECK_TECH_PERMILLE, WRECK_WATER_KEEP_SIXTEENTHS,
};
use super::flowfield::{nearest_passable_cell_for, FlowField, FlowSample};
use super::formation::{plan_formation, within, Travel};
use super::production::{Economy, WorkKind, WorkLink};
use super::spatial::{GridUnit, Layer, SpatialGrid};
use super::terrain::Terrain;
use super::visibility::{Sense, Visibility, VISION_REFRESH_TICKS};
use crate::math::fp::{fp_int, sqrt_wide, wide_mul, Fp, Vec2Fp, Vec3Fp, FP_FRAC_BITS};
use crate::math::rect::RectFp;
use crate::math::rng::SimRng;

/// How far from a goal, in unit radii, a moving unit can settle against a crowd.
/// How far off a ship's side, toward the torpedo, an anti-torpedo charge drops its decoy, how deep,
/// and how long the decoy is drawn, in ticks (2026-11-25).
const DECOY_OUT_UNITS: i32 = 120;
const DECOY_DEPTH_UNITS: i32 = 6;
const WRECK_SINK_SPEED: Fp = Fp::from_bits(WRECK_SINK_SPEED_RAW);
const WRECK_SINK_PULL: Fp = Fp::from_bits(WRECK_SINK_PULL_RAW);
const DECOY_TICKS: u32 = 40;
/// How far under the water a ship's torpedoes start (2026-11-25).
const TORPEDO_LAUNCH_DEPTH_UNITS: i32 = 8;
/// A torpedo leaves at this share of its top speed, about a ship's speed, and reaches its top speed
/// in this many ticks, two seconds, as the inspiration's do (2026-11-25).
const TORPEDO_LAUNCH_SHARE: i32 = 3;
const TORPEDO_SPEED_UP_TICKS: i32 = 20;

const CROWD_SETTLE_RADII: i32 = 30;
/// The gap between formation spots, in halves of the largest unit's radius.
const FORMATION_SPACING_RADII: i32 = 5;
const SLOT_EXTRA_TICKS: i64 = 40;
/// How long a formation waits for members that have fallen behind, before going on without them.
const FORMATION_PATIENCE_TICKS: u32 = 100;
/// How often a moving unit looks whether the way straight to its goal is clear, in ticks: often
/// while it is not, and now and then while it is, in case something has been built in the way.
const DIRECT_CHECK_TICKS: u32 = 16;
const DIRECT_RECHECK_TICKS: u32 = 64;
/// It looks no further than this, in map units, the width of the largest maps. A goal further off
/// is followed by its route until it is nearer.
const DIRECT_REACH_UNITS: i32 = 20_480;
/// Within this of its goal, or three of its turning circles if that is more, a unit that is not
/// facing it, within about 18 degrees, goes back to its route, which brings it into the goal's
/// cell to arrive.
const DIRECT_HANDOVER_UNITS: i32 = 100;
const DIRECT_FACING_COSINE: f64 = 0.95;

/// Whether a unit of `radius` could walk in a straight line from `from` to `to`: every point along
/// the way, and as far to each side as the unit is wide, can be walked on.
fn straight_way_is_clear(passable: &impl Fn(Vec2Fp) -> bool, from: Vec2Fp, to: Vec2Fp, radius: Fp) -> bool {
    let away = to - from;
    let length = sqrt_wide(away.length_squared());
    if length > fp_int(DIRECT_REACH_UNITS) {
        return false;
    }
    if length == Fp::ZERO {
        return true;
    }
    let along = away.normalized();
    let side = Vec2Fp::new(-along.y, along.x).scaled(radius);
    // Every cell's width: the lines to each side, as wide apart as the unit, catch a corner the
    // middle line steps over.
    let step = fp_int(super::config::TERRAIN_CELL_UNITS);
    let steps = (length / step).to_num::<i32>() + 1;
    (1..=steps).all(|index| {
        let point = from + along.scaled((step * index).min(length));
        passable(point) && passable(point + side) && passable(point - side)
    })
}

/// A ground attack's shell must come down this close to its spot, or the unit walks closer.
const GROUND_SHOT_REACH_UNITS: i32 = 8;
/// How long a unit passes over a target that the ground stops its shots at.
pub(super) const BLOCKED_TARGET_TICKS: u32 = 50;
/// How far beyond its range a unit on an attack-move walks straight at a target it is chasing,
/// before it needs a route there.
const DIRECT_CHASE_UNITS: i32 = 400;
/// How long a unit passes over a target it could not reach when it chased it.
const UNREACHABLE_CHASE_TICKS: u32 = 300;
/// How far below the ground a ground attack aims: a sixteenth of a unit.
const GROUND_AIM_DEPTH: Fp = Fp::from_bits(256);
/// Long enough for a bomb to fall from any aircraft's height.
const BOMB_LIFETIME_TICKS: u32 = 100;
/// How wide flak scatters for the aircraft it is aimed at: on top of its weapon's spread, an
/// eighth of how far the aircraft flies while the shell is in the air. A strategic bomber at 40
/// a tick, 25 ticks off, gets 125 more; a gunship hovering over its target, none.
const FLAK_LEAD_ERROR: i32 = 8;
/// The same for an anti-air gun's shells, which have no blast to make up for it: a fifth of how far
/// the aircraft flies while they are in the air. T1 Mobile Anti-Air hits a T1 bomber nearly every
/// time, a T1 interceptor most times, an air scout more often than not, and a spy plane, the
/// fastest and highest, next to never. Homing missiles, a SAM's, always hit.
const GUN_LEAD_ERROR: i32 = 5;
/// How far off its aim an anti-air gun's burst of rounds still hits an aircraft, beyond the
/// aircraft's own radius: an aircraft that turns on some ticks and not others is only roughly
/// where a gun aiming along its turn expects.
const GUN_BURST_REACH: i32 = 24;
/// Settings for spawning a unit.
#[derive(Copy, Clone, Debug)]
pub struct UnitSpawn {
    pub position: Vec2Fp,
    pub team: Team,
    /// The player it belongs to, on `team`.
    pub owner: PlayerId,
    pub max_speed: Fp,
    pub radius: Fp,
    pub health: Fp,
    pub range: Fp,
    pub reload_ticks: u32,
    pub projectile_speed: Fp,
    pub projectile_damage: Fp,
    pub projectile_splash: Fp,
    pub is_structure: bool,
    pub vision: Fp,
}

/// How a match ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MatchResult {
    Winner(Team),
    Draw,
}

/// A Discharge shell that landed, where, and on whose dome if it struck one.
#[derive(Copy, Clone, Debug)]
struct DischargeImpact {
    def: super::blueprints::DischargeDef,
    source: SimId,
    team: Team,
    point: (i32, i32),
    dome: Option<SimId>,
}

/// Read-only unit state for the renderer. Values are raw `Fp` bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UnitView {
    pub id: SimId,
    pub team: Team,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub radius: i32,
    /// Raw blueprint id, or `u16::MAX` for a unit without one.
    pub blueprint: u16,
    pub health_permille: u16,
    /// 1000 once finished.
    pub built_permille: u16,
    pub is_structure: bool,
    /// Which way the body faces, as a raw unit vector. Zero for units that turn instantly.
    pub facing: [i32; 2],
    /// Which way the weapon points, as a raw unit vector. Zero for weapons that do not swivel.
    pub aim: [i32; 2],
    /// Which way its first extra weapons point, as `aim` does: a battleship's other turrets.
    pub extra_aims: [[i32; 2]; 3],
    /// How far each turning gun's barrels are raised, the main one's and then its extra
    /// weapons', as the raw sine of their elevation. Zero for level, or for a gun that does not turn.
    pub pitches: [i32; 4],
    /// A factory making something, or a builder at work, with the power to do it. For drawing.
    pub working: bool,
    /// What a factory is building, as a raw blueprint id, and how far along it is in
    /// thousandths, so that it shows in the bay. Anyone who can see the factory sees it.
    pub assembling: Option<(u16, u16)>,
    /// What a structure is upgrading into, as a raw blueprint id, and how far along it is in
    /// thousandths, so that it shows changing as it goes. A factory's upgrade is the head of its
    /// queue. Anyone who can see it sees it.
    pub upgrade: Option<(u16, u16)>,
    /// An enemy structure out of sight, as the viewer last saw it (`KnownStructure`): drawn
    /// faded, with the health it had then. It may be gone.
    pub remembered: bool,
    /// Whether it belongs to an ally: a player of its team other than the team's first, the
    /// one a person plays. Theirs to command, not the person's, so it cannot be selected.
    pub ally: bool,
}

/// A factory's production state, for the build menu. Only for the viewer's team.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactoryView {
    pub id: SimId,
    pub queue: Vec<u16>,
    pub repeat: bool,
    pub progress_permille: u16,
    /// Where new units go, as raw `Fp` bits: x, y, and ground height.
    pub rally: Option<[i32; 3]>,
}

/// What a step in a unit's plan is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlanStepKind {
    Move,
    AttackMove,
    Attack,
    /// Firing at a spot on the ground. The player chose it, so fog of war does not hide it.
    AttackGround,
    Build,
    Support,
    Patrol,
}

/// One place a unit intends to go or work, as raw `Fp` bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PlanStep {
    pub kind: PlanStepKind,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// For a build that has not started: the width of its outline, as raw `Fp` bits like the rest.
    pub pending_width: i32,
}

/// A unit's pending orders, in the order it will carry them out. Only for the viewer's team.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitPlanView {
    pub id: SimId,
    pub steps: Vec<PlanStep>,
}

/// A builder's orders, in the order it will carry them out. Only for the viewer's team.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildQueueView {
    pub id: SimId,
    pub tasks: Vec<BuildTask>,
}

/// Where one unit's order that can still be moved will take it. Only for the viewer's team.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OrderStopView {
    pub unit: SimId,
    pub task: BuildTask,
    /// Raw `Fp` bits.
    pub position: [i32; 3],
}

/// What a structure or commander makes and spends, in tenths per second. Only for the viewer's team.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UnitRatesView {
    pub id: SimId,
    pub mass_income: i32,
    pub energy_income: i32,
    /// What a factory is really spending: less than its full rate during a stall, and more with help.
    pub mass_use: i32,
    pub energy_use: i32,
}

/// What one of the viewer's units would spend at full speed, in tenths per second: its share of
/// the work it is putting build power into, and its upkeep. Only for the viewer's team.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UnitSpendingView {
    pub id: SimId,
    pub mass: i32,
    pub energy: i32,
}

/// A builder that is doing work this tick, and where. Only present while work is really
/// happening, so not while walking to a site, waiting, or out of resources.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WorkLinkView {
    pub builder: SimId,
    /// Where the work is being done, as raw `Fp` bits.
    pub target: [i32; 3],
    /// Reclaiming takes mass back, instead of spending it.
    pub reclaim: bool,
}

/// Read-only wreck state. Values are raw `Fp` bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WreckView {
    pub id: SimId,
    pub x: i32,
    pub y: i32,
    /// Its middle, half its radius over the ground at rest.
    pub z: i32,
    pub radius: i32,
    pub mass: i32,
    pub energy: i32,
    pub kind: WreckKind,
    /// On its way down, or at rest.
    pub motion: WreckMotion,
}

/// Whether a wreck is falling through the air, sinking through the water, or at rest
/// (`Settling`).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum WreckMotion {
    #[default]
    Still,
    Falling,
    Sinking,
}

/// A player as the score panel shows them (GAME_SPECIFICATION.md §17.2): what the viewer can see
/// of their units, and, for a player of the viewer's team or a viewer who sees everything, their
/// income and where they started. An enemy's income and base stay hidden behind the fog.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PlayerView {
    pub team: Team,
    /// Out of the match: their commander has fallen.
    pub out: bool,
    /// Their units the viewer can see now, and what the mobile ones among them cost, in mass.
    /// Enemy structures remembered out of sight do not say whose they were, so they are not here.
    pub units: u32,
    pub army: i64,
    /// Mass income a second, as `EconomyView::mass_income`.
    pub mass_income: Option<i64>,
    /// Their start, as raw `Fp` bits.
    pub start: Option<[i32; 2]>,
}

impl EconomyView {
    /// A team's economy: what its players hold, earn, and spend, added up, building at the pace of
    /// the slowest of them, so that one player stalling shows.
    pub fn team(players: &[Self]) -> Self {
        players.iter().fold(Self { satisfaction_permille: 1000, ..Self::default() }, |sum, player| Self {
            mass: sum.mass + player.mass,
            energy: sum.energy + player.energy,
            mass_capacity: sum.mass_capacity + player.mass_capacity,
            energy_capacity: sum.energy_capacity + player.energy_capacity,
            mass_income: sum.mass_income + player.mass_income,
            energy_income: sum.energy_income + player.energy_income,
            mass_demand: sum.mass_demand + player.mass_demand,
            energy_demand: sum.energy_demand + player.energy_demand,
            satisfaction_permille: sum.satisfaction_permille.min(player.satisfaction_permille),
            mass_reclaim: sum.mass_reclaim + player.mass_reclaim,
            energy_reclaim: sum.energy_reclaim + player.energy_reclaim,
            mass_reclaimed: sum.mass_reclaimed + player.mass_reclaimed,
            energy_reclaimed: sum.energy_reclaimed + player.energy_reclaimed,
        })
    }
}

/// The viewer's economy, in whole units and per-second rates.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct EconomyView {
    pub mass: i64,
    pub energy: i64,
    pub mass_capacity: i64,
    pub energy_capacity: i64,
    pub mass_income: i64,
    pub energy_income: i64,
    pub mass_demand: i64,
    pub energy_demand: i64,
    /// How much of demand is being met, in thousandths.
    pub satisfaction_permille: u16,
    /// What reclaim is bringing in now, a second, and what it has brought in all match.
    pub mass_reclaim: i64,
    pub energy_reclaim: i64,
    pub mass_reclaimed: i64,
    pub energy_reclaimed: i64,
}

/// An enemy structure a team has seen, or had on radar (GAME_SPECIFICATION.md §9, Remembered
/// Structures). Structures do not move, so once found one stays on the team's map, as it was
/// when last seen, and can be fired at, until the team sees that it is gone. Values are raw
/// `Fp` bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct KnownStructure {
    pub id: SimId,
    /// What it is, as a raw blueprint id: `None` while it has only been on radar.
    pub blueprint: Option<u16>,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub radius: i32,
    /// Its health, building, and facing when last seen.
    pub health_permille: u16,
    pub built_permille: u16,
    pub facing: [i32; 2],
    pub aim: [i32; 2],
    /// Destroyed or captured since. The team does not know until it sees the spot, so it is
    /// still drawn; but nothing fires at it of its own accord, as there is nothing there.
    pub gone: bool,
}

/// An enemy contact on radar that the viewer cannot see. Values are raw `Fp` bits. A decoy has
/// no unit; the interface shows it like any other, and an attack on it goes there and finds
/// nothing.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ContactView {
    pub unit: Option<SimId>,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub is_air: bool,
    pub is_naval: bool,
    pub is_sub: bool,
    pub is_structure: bool,
}

/// Read-only projectile state for the renderer. Values are raw `Fp` bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProjectileView {
    pub id: SimId,
    pub team: Team,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub kind: ProjectileKind,
    /// The unit that fired it, so that a new shell shows that unit firing.
    pub source: SimId,
    /// Where it was fired from, as raw `Fp` bits, on the tick it was fired only: by then it has
    /// flown a tick, so it is drawn leaving its gun from here.
    pub from: Option<[i32; 3]>,
    /// How far its blast reaches when it lands, as raw `Fp` bits: zero for a direct hit. For
    /// drawing an explosion of the right size.
    pub splash: i32,
}

/// A unit's blast as it is destroyed, where it was and how far it reaches, as raw `Fp` bits: for
/// drawing an explosion as wide as the damage it does.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BlastView {
    pub team: Team,
    pub x: i32,
    pub y: i32,
    pub radius: i32,
}

/// A detonation heard across the whole map, seen or not: a strategic missile striking, or a
/// commander blowing up as it dies. For the sound, which plays it wherever the camera is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Detonation {
    pub kind: DetonationKind,
    /// Where, as raw `Fp` bits.
    pub x: i32,
    pub y: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DetonationKind {
    StrategicMissile,
    Commander,
}

/// What a projectile is, for drawing it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProjectileKind {
    Shell,
    /// Drawn larger and brighter.
    Discharge,
    TacticalMissile,
    StrategicMissile,
    /// A strategic missile defense's interceptor, flying up to meet a nuclear missile: drawn as
    /// a large, bright missile with a thick trail, so that it can be followed.
    Interceptor,
    /// A laser's beam, drawn as a line from `from`, its muzzle, to where it struck.
    Beam,
}

/// A shield generator's dome, for drawing and for the command panel. Values are raw `Fp` bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ShieldView {
    /// The generator.
    pub id: SimId,
    pub team: Team,
    /// The middle of the dome, on the ground, or for a ship's on the water.
    pub center: [i32; 3],
    pub radius: i32,
    /// How high it rises at its middle: its radius, unless flattened.
    pub height: i32,
    pub health_permille: u16,
    /// Whether the dome is standing. Not while it recharges, is paused, or is still being built.
    pub up: bool,
    /// Ticks until a collapsed dome comes back. Zero otherwise.
    pub recharging: u32,
}

/// One of the viewer's working sensors, for drawing what they cover. Values are raw `Fp` bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SensorRangeView {
    pub center: [i32; 2],
    /// Zero for none.
    pub radar: i32,
    pub deep_scan: i32,
}

/// How close, in units, an enemy must come to see a cloaked unit without deep scan.
pub const CLOAK_REVEAL_UNITS: i32 = 100;
/// Below this speed, in raw `Fp` units a tick, a unit counts as stopped for a missile launcher
/// choosing what to fire at: half a unit a tick.
const MISSILE_STILL_SPEED_RAW: i64 = 1 << (FP_FRAC_BITS - 1);
/// How often decoy blips move, in ticks.
const DECOY_SHUFFLE_TICKS: u32 = 50;

/// A dome that can stop shells this tick.
#[derive(Copy, Clone, Debug)]
struct Dome {
    id: SimId,
    team: Team,
    center: [i32; 3],
    radius: i32,
    /// How high it rises at its middle: its radius, unless flattened.
    height: i32,
}

/// A copy of the state the renderer needs from one tick, sorted by `SimId`.
#[derive(Clone, Debug, Default)]
pub struct SimSnapshot {
    pub tick: u32,
    pub units: Vec<UnitView>,
    pub projectiles: Vec<ProjectileView>,
    /// Projectiles gone since the last tick, at the point where they struck, or ran out.
    pub landed: Vec<ProjectileView>,
    /// Blasts of units destroyed this tick that blow up as they go.
    pub blasts: Vec<BlastView>,
    pub wrecks: Vec<WreckView>,
    /// The shields of generators the viewer can see.
    pub shields: Vec<ShieldView>,
    /// Missiles shot down this tick: from the defense that fired, to the missile.
    pub interceptions: Vec<[[i32; 3]; 2]>,
    /// Torpedo decoys in the water the viewer can see, where anti-torpedo charges dropped them, and
    /// the tick each sinks.
    pub decoys: Vec<([i32; 3], u32)>,
    /// Missiles built and waiting in the viewer's silos, by silo, sorted by `SimId`.
    pub stocks: Vec<(SimId, u32)>,
    /// How far open the doors of silos in `units` are, in thousandths, while a launch is under
    /// way, sorted by `SimId`. Drawing only (`SimWorld::silo_doors`).
    pub silo_doors: Vec<(SimId, u16)>,
    /// How many units each of the viewer's transports carries, sorted by `SimId`. Everyone's,
    /// for a viewer who sees everything.
    pub cargo: Vec<(SimId, u32)>,
    /// The viewer's structures sharing adjacency bonuses, as the giver's and the receiver's
    /// centers, raw `Fp` bits with ground height. Everyone's, for a viewer who sees everything.
    pub adjacency: Vec<([i32; 3], [i32; 3])>,
    /// Enemy contacts on the viewer's radar that it cannot see, real and decoy alike, as raw
    /// `Fp` positions. Nothing says which is which.
    pub blips: Vec<[i32; 3]>,
    /// The same contacts, with the unit each is and what it is, for orders at them. A decoy
    /// names no unit.
    pub contacts: Vec<ContactView>,
    /// The viewer's working sensors, for drawing their coverage.
    pub sensor_ranges: Vec<SensorRangeView>,
    /// The viewer's units with a cloak: whether it is switched on, and whether it is hiding them.
    pub cloaks: Vec<(SimId, bool, bool)>,
    /// Submarines in the snapshot that are on the surface, sorted.
    pub surfaced: Vec<SimId>,
    /// The viewer's finished, unpaused intel structures that are not running for want of energy.
    pub unpowered: Vec<SimId>,
    /// How far along the viewer's upgrades in place are, in thousandths, sorted by `SimId`.
    pub upgrades: Vec<(SimId, u16)>,
    /// The viewer's units' field enhancements, one for each slot, `u16::MAX` for none.
    pub enhancements: Vec<(SimId, [u16; 3])>,
    /// The viewer's enhancements under way: which, and how far along, in thousandths.
    pub enhancing: Vec<(SimId, u16, u16)>,
    /// Every unit in `units`, of either team, that is upgrading: a commander or support
    /// commander being fitted with an enhancement, a factory on an unpaused upgrade, or a
    /// structure upgrading in place. It says nothing more, so it gives away nothing else of an
    /// enemy's. Sorted by `SimId`.
    pub upgrading: Vec<SimId>,
    /// Units in `units` that are stunned, sorted by `SimId` (GAME_SPECIFICATION.md §12).
    pub stunned: Vec<SimId>,
    /// Units in `units` held in a tractor beam, and the unit holding each, as raw `Fp`
    /// positions: from the holder to what it holds.
    pub tractor_beams: Vec<[[i32; 3]; 2]>,
    /// Storms the viewer can see: where, as raw `Fp`, and how wide.
    pub storms: Vec<StormView>,
    /// The viewer's teleports charging: to where, and how far along, in thousandths.
    pub teleports: Vec<(SimId, [i32; 2], u16)>,
    /// The viewer's units counting down to blowing up, and the ticks left.
    pub self_destructs: Vec<(SimId, u32)>,
    pub factories: Vec<FactoryView>,
    /// Sorted by `SimId`.
    pub plans: Vec<UnitPlanView>,
    /// Sorted by builder `SimId`.
    pub work_links: Vec<WorkLinkView>,
    /// Sorted by builder `SimId`.
    pub build_queues: Vec<BuildQueueView>,
    /// Sorted by unit `SimId`.
    pub order_stops: Vec<OrderStopView>,
    /// Sorted by `SimId`.
    pub rates: Vec<UnitRatesView>,
    /// Everything of the viewer's that spends, sorted by `SimId`.
    pub spending: Vec<UnitSpendingView>,
    /// The viewer's finished factories with nothing to build and no factory to help, sorted by
    /// `SimId`.
    pub idle_factories: Vec<SimId>,
    /// How many of the viewer's orders have been cancelled because they could not be reached.
    pub orders_cancelled: u32,
    /// The viewer's paused factories and sites, sorted by `SimId`.
    pub paused: Vec<SimId>,
    /// The rank of each veteran the viewer can see, sorted by `SimId`.
    pub veterans: Vec<(SimId, u8)>,
    /// How many enemies each unit the viewer can see has destroyed, for those that have any,
    /// sorted by `SimId`.
    pub kills: Vec<(SimId, u32)>,
    pub economy: EconomyView,
    /// Both teams' economies, Blue's then Red's, for a watcher who sees everything: each the
    /// sum of its players' (`EconomyView::team`). Only in snapshots for no one side.
    pub team_economies: Option<[EconomyView; 2]>,
    /// The team of each strategic missile launched this tick, seen or not, for the launch
    /// warning everyone hears.
    pub strategic_launches: Vec<Team>,
    /// This tick's detonations heard across the whole map, seen or not.
    pub detonations: Vec<Detonation>,
    /// Every player of the match, in order, for the score panel.
    pub players: Vec<PlayerView>,
    /// Present when fog of war is on: one byte per cell, for the renderer.
    pub fog: Option<Arc<Vec<u8>>>,
    pub match_result: Option<MatchResult>,
}

/// A roaming storm, for drawing.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StormView {
    pub x: i32,
    pub y: i32,
    pub radius: i32,
}

/// A blast waiting to be applied.
#[derive(Copy, Clone, Debug)]
struct Explosion {
    x: i32,
    y: i32,
    radius: Fp,
    damage: Fp,
    /// Who gets the credit for what it destroys.
    source: Option<SimId>,
    /// An EMP blast stuns what it reaches this many ticks.
    stun: u32,
}

/// Mixed into the world's seed for the scatter streams, so that they do not repeat its numbers.
const SCATTER_SEED: u64 = 0x5CA7_7E25_0FF5_E7ED;

/// The highest veteran rank.
pub const MAX_RANK: u8 = 5;
/// Each rank adds this share of a unit's base health, as the inspiration's veterancy does: 10% a
/// level.
pub(super) const RANK_HEALTH_PERMILLE: i64 = 100;
/// Reaching a rank heals this share of the unit's base health at once: the inspiration's default of
/// 10%. The commander heals more at each level, as its blueprint sets.
const PROMOTION_HEAL_PERMILLE: i64 = 100;
const COMMANDER_PROMOTION_HEAL_PERMILLE: [i64; 5] = [100, 200, 300, 400, 500];
/// The inspiration's mass a level, as a multiple of the unit's own cost, in thousandths, by tech
/// level: a rank n unit has destroyed n times that. The commander has its own steps instead.
const VET_MASS_PERMILLE_BY_TIER: [i64; 3] = [2_000, 1_500, 1_250];
const VET_MASS_PERMILLE_SUPPORT: i64 = 2_000;
const COMMANDER_VET_MASS: [i64; 5] = [1_000, 2_250, 4_000, 6_500, 10_000];
/// The inspiration's regeneration a second for each rank, by tech row: T1, T2, T3 and commanders,
/// support commanders, experimentals. Ships, below experimental, take the row above their tier.
const VET_REGEN_ROWS: [i64; 5] = [1, 3, 6, 9, 25];

/// The mass a unit must have destroyed to reach a rank, the inspiration's veterancy thresholds.
pub fn vet_threshold(unit: &super::blueprints::Blueprint, rank: u8) -> Res {
    if rank == 0 {
        return Res::ZERO;
    }
    let rank = rank.min(MAX_RANK);
    if unit.is_commander() {
        return Res::from_num(COMMANDER_VET_MASS[usize::from(rank) - 1]);
    }
    let permille = if unit.class == super::blueprints::UnitClass::FieldRig {
        VET_MASS_PERMILLE_SUPPORT
    } else {
        VET_MASS_PERMILLE_BY_TIER[usize::from(unit.tier.clamp(1, 3)) - 1]
    };
    unit.mass * i64::from(rank) * permille / 1_000
}

/// What a veteran of a rank regenerates a second, the inspiration's flat amounts.
pub fn vet_regen_per_second(unit: &super::blueprints::Blueprint, rank: u8) -> i64 {
    let row = if unit.is_commander() {
        2
    } else if unit.class == super::blueprints::UnitClass::FieldRig {
        3
    } else if unit.tier >= 4 {
        4
    } else {
        let naval = matches!(unit.movement, MoveClass::Naval | MoveClass::Submarine);
        usize::from(unit.tier.clamp(1, 3)) - 1 + usize::from(naval)
    };
    VET_REGEN_ROWS[row] * i64::from(rank)
}

#[derive(Copy, Clone)]
struct PendingShot {
    source: SimId,
    team: Team,
    from: Vec3Fp,
    to: Vec3Fp,
    speed: Fp,
    damage: Fp,
    splash: Fp,
    /// For a Discharge shell, its rule, worked out where it lands.
    discharge: Option<super::blueprints::DischargeDef>,
    missile: Option<MissileShot>,
    /// A shot at an aircraft, that steers itself to it.
    homing: Option<SimId>,
    /// A bomb, with the velocity of the aircraft that dropped it.
    bomb: Option<Vec3Fp>,
    /// How far from where it is aimed an artillery shell may land.
    spread: Fp,
    /// A torpedo, steering under the water.
    underwater: bool,
    /// What it hits is stunned this many ticks.
    stun: u32,
    /// A shell aims where this unit will be when it arrives (§7.1, Leading Shots).
    lead: Option<hecs::Entity>,
    /// A shell at an aircraft that does not steer: it flies straight to where the aircraft will
    /// be if it holds its course.
    fused: bool,
    /// Flak, which also scatters the more, the faster the aircraft and the longer the flight.
    flak: bool,
    /// An artillery shell, lobbed in a high arc to come down on where it was aimed
    /// (GAME_SPECIFICATION.md §7, Artillery).
    lobbed: bool,
}

/// How long a strategic defense's interceptor flies to meet its nuclear missile, at most, in
/// ticks: less if the missile would strike sooner.
pub const INTERCEPTOR_FLIGHT_TICKS: u32 = 30;
/// How far above its defense an interceptor leaves from.
const INTERCEPTOR_LAUNCH_HEIGHT_UNITS: i32 = 12;
/// How wide an interceptor's burst is drawn, in world units.
const INTERCEPTOR_BURST_UNITS: i32 = 40;

/// A strategic defense's interceptor, flying to the nuclear missile it was fired at. Where they
/// meet is worked out at launch from the missile's path, which is fixed, so it never misses: on
/// the tick it arrives, the missile is destroyed, even if the defense is gone by then.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Interceptor {
    id: SimId,
    team: Team,
    /// The defense that fired it.
    source: SimId,
    from: Vec3Fp,
    meet: Vec3Fp,
    /// The nuclear missile.
    target: SimId,
    launched: u32,
    arrives: u32,
}

impl Interceptor {
    /// Where it is on this tick: from its defense to the meeting, picking up speed as it goes.
    fn position(&self, tick: u32) -> Vec3Fp {
        let flight = i64::from((self.arrives - self.launched).max(1));
        let flown = i64::from(tick.saturating_sub(self.launched)).min(flight);
        // A share that starts at half speed and ends at one and a half: s = (t + t²) / 2.
        let share = |from: Fp, to: Fp| {
            let gap = i64::from(to.to_bits()) - i64::from(from.to_bits());
            let along = gap * (flown * flight + flown * flown) / (2 * flight * flight);
            Fp::from_bits((i64::from(from.to_bits()) + along) as i32)
        };
        Vec3Fp::new(share(self.from.x, self.meet.x), share(self.from.y, self.meet.y), share(self.from.z, self.meet.z))
    }

    fn view(&self, tick: u32) -> ProjectileView {
        let at = self.position(tick);
        let bits = |point: Vec3Fp| [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()];
        ProjectileView {
            id: self.id,
            team: self.team,
            x: at.x.to_bits(),
            y: at.y.to_bits(),
            z: at.z.to_bits(),
            kind: ProjectileKind::Interceptor,
            source: self.source,
            from: (tick == self.launched + 1).then(|| bits(self.from)),
            splash: fp_int(INTERCEPTOR_BURST_UNITS).to_bits(),
        }
    }
}

/// A nuclear missile on its way out of a silo or a submarine's hatch (2026-11-25): the doors
/// open over `SILO_DOOR_TICKS`, the missile rises out once they are open, and they close behind
/// it. `ticks` counts from the order.
#[derive(Copy, Clone)]
struct SiloLaunch {
    shot: PendingShot,
    ticks: u32,
}

/// How long a silo's doors take to open, and to close.
pub const SILO_DOOR_TICKS: u32 = 30;
/// How long they stay open once the missile is on its way, for it to rise clear: its lift, at its
/// lift speed, and a little more.
pub const SILO_OPEN_TICKS: u32 = 25;
/// A launch from the order until the doors are closed again.
pub const SILO_SEQUENCE_TICKS: u32 = SILO_DOOR_TICKS * 2 + SILO_OPEN_TICKS;

/// How far open a silo's doors are, from 0 to `SILO_DOOR_TICKS`, this many ticks into a launch.
pub const fn silo_doors_open(ticks: u32) -> u32 {
    if ticks <= SILO_DOOR_TICKS {
        ticks
    } else if ticks <= SILO_DOOR_TICKS + SILO_OPEN_TICKS {
        SILO_DOOR_TICKS
    } else {
        SILO_SEQUENCE_TICKS.saturating_sub(ticks)
    }
}

/// What a launcher gives the missile it fires.
#[derive(Copy, Clone)]
struct MissileShot {
    cruise_height: Fp,
    strategic: bool,
    inner_radius: Fp,
    inner_damage: Fp,
}

pub struct SimWorld {
    pub ecs: hecs::World,
    pub terrain: Terrain,
    pub spatial_grid: SpatialGrid,
    pub flow_field: FlowField,
    pub rng: SimRng,
    /// Where scattering shots land, a stream for each team, both seeded alike: on a mirrored map,
    /// the same shot on each side draws the same numbers, and Red's are turned about the map's
    /// middle, so the sides stay mirror images. From the one shared stream, whichever side fired
    /// first drew first, and the sides parted ways.
    pub scatter_rngs: [SimRng; 2],
    /// Submarines on the surface, by id: on the water like ships, seen and fired on as ships are,
    /// and free to fire their deck guns. Every other submarine is submerged.
    pub surfaced: Vec<SimId>,
    pub config: SimConfig,
    pub current_tick: u32,
    pub next_sim_id: u32,
    /// How many units each player has had, which sets each new unit's `UnitStats::phase`: the
    /// turn it takes at work done every so many ticks, such as looking for a target. By player,
    /// so that a unit and its mirror image on the other team take the same turn: counted by
    /// team, in a team match a player's units took turns one off their opposite number's, and
    /// the teams stopped playing as mirror images within a minute.
    pub units_spawned: Vec<u32>,
    /// Sorted by `SimId`: spawns append, despawns retain order. Any loop whose order
    /// can affect results iterates these, never a `hecs` query.
    pub units: Vec<(SimId, hecs::Entity)>,
    pub projectiles: Vec<(SimId, hecs::Entity)>,
    /// Box obstacles added to the terrain, for the renderer.
    pub obstacles: Vec<RectFp>,
    pub blueprints: Arc<BlueprintDb>,
    pub visibility: Visibility,
    /// Each player's team, by player: two players, Blue's and Red's, unless `set_players` says
    /// otherwise.
    pub players: Vec<Team>,
    /// By player.
    pub economies: Vec<Economy>,
    pub deposits: Vec<(u32, u32)>,
    /// Geothermal vents, by cell (GAME_SPECIFICATION.md §3.4). A plant is built with one at the
    /// middle of its footprint.
    pub vents: Vec<(u32, u32)>,
    /// The structure on each occupied terrain cell, by cell index.
    pub occupancy: BTreeMap<u32, SimId>,
    pub wrecks: Vec<(SimId, hecs::Entity)>,
    /// Wrecks still falling or sinking (`Settling`), sorted by `SimId`.
    pub settling_wrecks: Vec<(SimId, hecs::Entity)>,
    pub builders: Vec<(SimId, hecs::Entity)>,
    pub factories: Vec<(SimId, hecs::Entity)>,
    pub structures: Vec<(SimId, hecs::Entity)>,
    pub patrols: Vec<(SimId, hecs::Entity)>,
    /// Units that are not builders with movement orders waiting, sorted by `SimId`.
    pub order_queues: Vec<(SimId, hecs::Entity)>,
    /// Units with a veteran rank, sorted by `SimId`.
    pub veterans: Vec<(SimId, hecs::Entity)>,
    /// Shield generators, sorted by `SimId`.
    pub shields: Vec<(SimId, hecs::Entity)>,
    /// Projectiles that are missiles, sorted by `SimId`.
    pub missiles: Vec<(SimId, hecs::Entity)>,
    /// Missile defenses, sorted by `SimId`.
    pub missile_defenses: Vec<(SimId, hecs::Entity)>,
    /// Commanders and structures upgrading in place, sorted by `SimId`.
    pub upgrading: Vec<(SimId, hecs::Entity)>,
    /// Units with sensors, stealth fields, jamming, or cloaks, sorted by `SimId`.
    pub intel: Vec<(SimId, hecs::Entity)>,
    /// Units with field enhancements, and those with one under way, sorted by `SimId`.
    pub enhanced: Vec<(SimId, hecs::Entity)>,
    pub enhancing: Vec<(SimId, hecs::Entity)>,
    /// Units charging a teleport, and those counting down to blowing up, sorted by `SimId`.
    pub teleporting: Vec<(SimId, hecs::Entity)>,
    pub self_destructing: Vec<(SimId, hecs::Entity)>,
    /// Units an enemy is capturing, sorted by `SimId`, and those worked on this tick.
    pub being_captured: Vec<(SimId, hecs::Entity)>,
    /// Aircraft, and airfields, sorted by `SimId`.
    pub aircraft: Vec<(SimId, hecs::Entity)>,
    /// Units with anti-torpedo charges, sorted by `SimId`.
    pub torpedo_defenses: Vec<(SimId, hecs::Entity)>,
    /// Submarines, and units that walk the seabed, sorted by `SimId`.
    pub submarines: Vec<(SimId, hecs::Entity)>,
    pub seabed_walkers: Vec<(SimId, hecs::Entity)>,
    /// Structures giving neighbors adjacency bonuses they use, as (giver, receiver), sorted.
    /// Worked out with the economy from the structures there, so not hashed.
    pub adjacency_links: Vec<(SimId, SimId)>,
    /// Air transports, sorted by `SimId`.
    pub transports: Vec<(SimId, hecs::Entity)>,
    /// Land units on their way to board a transport, sorted by `SimId`.
    pub boarding: Vec<(SimId, hecs::Entity)>,
    /// Units inside transports, off the map and out of `units`, sorted by `SimId`.
    pub carried: Vec<(SimId, hecs::Entity)>,
    pub airfields: Vec<(SimId, hecs::Entity)>,
    /// Units that are stunned or held in a tractor beam, and units with tractor beams, sorted by
    /// `SimId`. Worked out from what is hashed, so they are not hashed themselves.
    pub disabled: Vec<(SimId, hecs::Entity)>,
    pub tractors: Vec<(SimId, hecs::Entity)>,
    /// Storms roaming where units died (GAME_SPECIFICATION.md §12), oldest first.
    pub storms: Vec<super::exotic::Storm>,
    /// Stuns from this tick's hits and blasts, applied with the damage.
    pub(super) pending_stuns: Vec<(hecs::Entity, u32)>,
    /// Units that tractor beams take hold of this tick: by whom, and whom.
    pub(super) pending_grabs: Vec<(SimId, SimId)>,
    pub(super) captured_this_tick: Vec<SimId>,
    /// For each team, the cloaked enemies it cannot see this tick, sorted by `SimId`. Worked out
    /// again every tick from what is hashed, so it is not hashed itself.
    concealed: [Vec<SimId>; 2],
    /// By team: the enemy units it has on radar, deep scan, or, under the water, sonar, that no
    /// stealth hides, in `SimId` order. They can be fired at as if seen.
    detected: [Vec<SimId>; 2],
    /// By team: the enemy structures it has found, in `SimId` order (`KnownStructure`). Hashed.
    pub known_structures: [Vec<KnownStructure>; 2],
    /// Missiles shot down this tick, from where to where. Only for drawing, so not in the checksum.
    interceptions: Vec<[[i32; 3]; 2]>,
    /// Strategic interceptors in flight, in the order they were fired, which is `SimId` order.
    interceptors: Vec<Interceptor>,
    /// Missiles shot down this tick, sorted by `SimId`: a nuclear one strikes nothing, and so is
    /// not `landed`.
    shot_down: Vec<SimId>,
    /// Nuclear missiles on their way out of their silos, in the order they were fired. Hashed.
    silo_launches: Vec<SiloLaunch>,
    /// Torpedo decoys in the water, and the tick each sinks. Only for drawing: what they do is in
    /// the torpedoes they lure (`Homing::lure`).
    decoys: Vec<([i32; 3], u32)>,
    /// Groups on a formation move, by the route they share.
    pub formations: BTreeMap<u32, Travel>,
    /// By player.
    pub commanders: Vec<Option<SimId>>,
    /// By player: whose commander has died, which puts them out of the match.
    pub eliminated: Vec<bool>,
    pub match_result: Option<MatchResult>,
    /// Where each team's commander started, for the AI's plans and the camera. Fixed for the
    /// match, and set from the map, so it is not hashed: the map's terrain is.
    pub starts: [Vec2Fp; 2],
    /// Where each player's commander started, by player. In a 1v1 the same as `starts`.
    pub player_starts: Vec<Vec2Fp>,
    pub(super) economy_dirty: bool,
    /// Who worked on what this tick. Rebuilt every tick and only used for drawing,
    /// so it is not part of the checksum.
    pub(super) work_links: Vec<WorkLink>,
    /// What each factory spent this tick. Rebuilt every tick and only used for showing,
    /// so it is not part of the checksum.
    pub(super) factory_spending: Vec<(SimId, Res, Res)>,
    /// Who put build power into what this tick: the job, what kind of work, who, and how much.
    /// Rebuilt every tick and only used for showing, so it is not part of the checksum.
    pub(super) work_sources: Vec<(SimId, WorkKind, SimId, Fp)>,
    /// What each job this tick costs a tick at full speed, and all the power put into it, in the
    /// order of job and kind. Rebuilt every tick and only used for showing, so not hashed.
    pub(super) work_demand: Vec<(SimId, WorkKind, Res, Res, Fp)>,
    /// The upkeep each unit paid this tick: shields, intel, cloaks, stealth, jamming, and
    /// teleports. Rebuilt every tick and only used for showing, so not hashed.
    pub(super) upkeep_paid: Vec<(SimId, Res)>,
    /// Units whose order was cancelled this tick because they cannot reach it, sorted by `SimId`.
    pub(super) unreachable: Vec<SimId>,
    /// How many orders each team has had cancelled that way, for telling the player.
    /// Only for showing, so it is not part of the checksum.
    orders_cancelled: [u32; 2],
    max_mobile_radius: i32,
    max_structure_radius: i32,
    pending_shots: Vec<PendingShot>,
    /// Each hit, and who caused it.
    pub(super) pending_hits: Vec<(hecs::Entity, Fp, Option<SimId>)>,
    /// Hits queued from outside the simulation (`strike`), dealt with the next tick's.
    strikes: Vec<(SimId, Fp)>,
    pending_explosions: Vec<Explosion>,
    /// Shells that came down on a shield this tick, and what they do to it.
    pending_shield_hits: Vec<(SimId, Fp)>,
    /// Discharge shells that landed this tick, to be worked out once the shells have all moved.
    pending_discharges: Vec<DischargeImpact>,
    /// Blasts and shells that land on the ground this tick, which also hit debris.
    pending_wreck_blasts: Vec<Explosion>,
    dead_units: Vec<SimId>,
    dead_projectiles: Vec<SimId>,
    /// Projectiles that struck something, or ran out, this tick, where they did: for drawing
    /// them all the way to what they hit. Not part of the game state.
    landed: Vec<ProjectileView>,
    /// Units' blasts as they were destroyed this tick, for drawing. Not part of the game state.
    blasts: Vec<BlastView>,
    /// Commanders' death blasts this tick, where, as raw `Fp` bits: heard wherever the camera is,
    /// so kept whether or not anyone sees them. Not part of the game state.
    commander_blasts: Vec<[i32; 2]>,
    /// Projectiles fired this tick, by id, and where from as raw `Fp` bits: for drawing a new
    /// shell leaving its gun, not where its first tick took it. Not part of the game state.
    launched: Vec<(SimId, [i32; 3])>,
}

/// How a projectile looks to the renderer, at its position.
fn projectile_view(id: SimId, position: &Position, projectile: &Projectile, discharged: Option<&Discharged>, missile: Option<&Missile>, beam: Option<&super::components::Beam>) -> ProjectileView {
    let (x, y, z) = (position.0.x.to_bits(), position.0.y.to_bits(), position.0.z.to_bits());
    let kind = match (discharged, missile) {
        _ if beam.is_some() => ProjectileKind::Beam,
        (Some(_), _) => ProjectileKind::Discharge,
        (None, Some(missile)) if missile.strategic => ProjectileKind::StrategicMissile,
        (None, Some(_)) => ProjectileKind::TacticalMissile,
        (None, None) => ProjectileKind::Shell,
    };
    // A Discharge shell's blast is worked out when it lands, by its own reach.
    let splash = discharged.map_or(projectile.splash_radius, |discharged| discharged.def.splash_radius);
    // A beam is drawn from its muzzle whenever it is seen, not only on the tick it was fired.
    let from = beam.map(|beam| [beam.from.x.to_bits(), beam.from.y.to_bits(), beam.from.z.to_bits()]);
    ProjectileView { id, team: projectile.source_team, x, y, z, kind, source: projectile.source, from, splash: splash.to_bits() }
}

impl SimWorld {
    pub fn new(config: SimConfig, seed: u64) -> Self {
        let mut rng = SimRng::new(seed);
        let terrain = Terrain::generate(config.map_units, config.terrain, &mut rng);
        Self::build(config, rng, terrain)
    }

    /// A world on terrain from a map file rather than generated.
    pub fn with_terrain(config: SimConfig, seed: u64, terrain: Terrain) -> Self {
        assert_eq!(config.map_units, terrain.map_units(), "the terrain must fill the map");
        Self::build(config, SimRng::new(seed), terrain)
    }

    fn build(config: SimConfig, rng: SimRng, terrain: Terrain) -> Self {
        let spatial_grid = SpatialGrid::new(config.map_units);
        let flow_field = FlowField::new(&terrain);
        let visibility = Visibility::new(config.map_units);
        Self {
            ecs: hecs::World::new(),
            terrain,
            spatial_grid,
            flow_field,
            surfaced: Vec::new(),
            scatter_rngs: [SimRng::new(rng.state() ^ SCATTER_SEED), SimRng::new(rng.state() ^ SCATTER_SEED)],
            rng,
            config,
            current_tick: 0,
            next_sim_id: 0,
            units_spawned: vec![0; 2],
            units: Vec::new(),
            projectiles: Vec::new(),
            obstacles: Vec::new(),
            blueprints: Arc::new(BlueprintDb::standard()),
            visibility,
            players: vec![Team::Blue, Team::Red],
            economies: vec![Economy::default(), Economy::default()],
            deposits: Vec::new(),
            vents: Vec::new(),
            occupancy: BTreeMap::new(),
            wrecks: Vec::new(),
            settling_wrecks: Vec::new(),
            builders: Vec::new(),
            factories: Vec::new(),
            structures: Vec::new(),
            patrols: Vec::new(),
            order_queues: Vec::new(),
            veterans: Vec::new(),
            shields: Vec::new(),
            missiles: Vec::new(),
            missile_defenses: Vec::new(),
            upgrading: Vec::new(),
            intel: Vec::new(),
            enhanced: Vec::new(),
            enhancing: Vec::new(),
            teleporting: Vec::new(),
            self_destructing: Vec::new(),
            being_captured: Vec::new(),
            aircraft: Vec::new(),
            submarines: Vec::new(),
            seabed_walkers: Vec::new(),
            adjacency_links: Vec::new(),
            transports: Vec::new(),
            boarding: Vec::new(),
            carried: Vec::new(),
            torpedo_defenses: Vec::new(),
            airfields: Vec::new(),
            disabled: Vec::new(),
            tractors: Vec::new(),
            storms: Vec::new(),
            pending_stuns: Vec::new(),
            pending_grabs: Vec::new(),
            captured_this_tick: Vec::new(),
            concealed: [Vec::new(), Vec::new()],
            detected: [Vec::new(), Vec::new()],
            known_structures: [Vec::new(), Vec::new()],
            interceptions: Vec::new(),
            interceptors: Vec::new(),
            shot_down: Vec::new(),
            silo_launches: Vec::new(),
            decoys: Vec::new(),
            formations: BTreeMap::new(),
            commanders: vec![None, None],
            eliminated: vec![false, false],
            starts: [Vec2Fp::ZERO; 2],
            player_starts: vec![Vec2Fp::ZERO; 2],
            match_result: None,
            economy_dirty: false,
            work_links: Vec::new(),
            factory_spending: Vec::new(),
            work_sources: Vec::new(),
            work_demand: Vec::new(),
            upkeep_paid: Vec::new(),
            unreachable: Vec::new(),
            orders_cancelled: [0; 2],
            max_mobile_radius: 0,
            max_structure_radius: 0,
            pending_shots: Vec::new(),
            pending_hits: Vec::new(),
            strikes: Vec::new(),
            pending_explosions: Vec::new(),
            pending_wreck_blasts: Vec::new(),
            pending_shield_hits: Vec::new(),
            pending_discharges: Vec::new(),
            dead_units: Vec::new(),
            dead_projectiles: Vec::new(),
            landed: Vec::new(),
            blasts: Vec::new(),
            commander_blasts: Vec::new(),
            launched: Vec::new(),
        }
    }

    /// Does the one-time map preparation now, so that it cannot land on the first tick.
    /// Call after adding obstacles. Safe to skip: it would otherwise run on first use.
    pub fn prepare_map(&mut self) {
        self.flow_field.prepare(&self.terrain);
    }

    pub fn add_box_obstacle(&mut self, rect: RectFp) {
        self.terrain.add_box_obstacle(rect);
        self.flow_field.invalidate();
        self.obstacles.push(rect);
    }

    pub(super) fn next_id(&mut self) -> SimId {
        let id = SimId(self.next_sim_id);
        self.next_sim_id += 1;
        id
    }

    /// Sets who plays, each player's team by player, before anything is spawned: the first
    /// player of each team is the one its team's units go to unless a player is named.
    pub fn set_players(&mut self, teams: &[Team]) {
        assert!(self.units.is_empty(), "players are set before anything is spawned");
        assert!(teams.contains(&Team::Blue) && teams.contains(&Team::Red), "both teams have players");
        self.players = teams.to_vec();
        self.units_spawned = vec![0; teams.len()];
        self.economies = vec![Economy::default(); teams.len()];
        self.commanders = vec![None; teams.len()];
        self.eliminated = vec![false; teams.len()];
        self.player_starts = vec![Vec2Fp::ZERO; teams.len()];
    }

    /// Where a player started: their own start in a team match, or their team's in a 1v1.
    pub fn start_of(&self, player: PlayerId) -> Vec2Fp {
        self.player_starts.get(player.index()).copied().filter(|&at| at != Vec2Fp::ZERO).unwrap_or(self.starts[self.players[player.index()].index()])
    }

    /// A team's first player, who owns its units unless a player is named.
    pub fn first_player(&self, team: Team) -> PlayerId {
        PlayerId(self.players.iter().position(|&of| of == team).expect("both teams have players") as u8)
    }

    /// A team's players, in order.
    pub fn players_of(&self, team: Team) -> impl Iterator<Item = PlayerId> + '_ {
        self.players.iter().enumerate().filter(move |&(_, &of)| of == team).map(|(index, _)| PlayerId(index as u8))
    }

    pub fn spawn_unit(&mut self, spawn: UnitSpawn) -> SimId {
        self.spawn_unit_as(spawn, MoveClass::Land)
    }

    /// Aircraft can start anywhere on the map, even over water or cliffs.
    pub(super) fn spawn_unit_as(&mut self, spawn: UnitSpawn, move_class: MoveClass) -> SimId {
        assert!(self.terrain.passable_for(move_class, spawn.position), "unit spawned where it cannot be");
        self.spawn_unit_anywhere(spawn, move_class)
    }

    /// Spawns a unit even where it could not move, such as a factory's bay, which it then
    /// drives out of.
    pub(super) fn spawn_unit_anywhere(&mut self, spawn: UnitSpawn, move_class: MoveClass) -> SimId {
        assert!(self.terrain.in_bounds(spawn.position), "unit spawned outside the map");
        let id = self.next_id();
        let phase = self.units_spawned[spawn.owner.index()];
        self.units_spawned[spawn.owner.index()] += 1;
        // Ships float on the water.
        let ground = match move_class {
            MoveClass::Naval | MoveClass::Hover => self.terrain.height_at(spawn.position).max(self.terrain.water_level()),
            MoveClass::Submarine => self.terrain.water_level() - spawn.radius * 2,
            _ => self.terrain.height_at(spawn.position),
        };
        let entity = self.ecs.spawn((
            id,
            Position(Vec3Fp::new(spawn.position.x, spawn.position.y, ground + spawn.radius)),
            Velocity(Vec3Fp::ZERO),
            UnitStats {
                max_speed: spawn.max_speed,
                radius: spawn.radius,
                team: spawn.team,
                owner: spawn.owner,
                move_class,
                is_structure: spawn.is_structure,
                vision: spawn.vision,
                phase,
            },
            Health { current: spawn.health, max: spawn.health },
            Combat {
                range: spawn.range,
                reload_ticks: spawn.reload_ticks,
                ticks_until_ready: 0,
                projectile_speed: spawn.projectile_speed,
                projectile_damage: spawn.projectile_damage,
                projectile_splash: spawn.projectile_splash,
                target: None,
                forced_target: None,
                line_blocked: false,
                closing: None,
                chasing: false,
                blocked: None,
                ground_target: None,
                enabled: true,
            },
            Order::IDLE,
        ));
        self.units.push((id, entity));
        let largest = if spawn.is_structure { &mut self.max_structure_radius } else { &mut self.max_mobile_radius };
        *largest = (*largest).max(spawn.radius.to_bits());
        id
    }

    /// Spawns a projectile directly. Units use this through firing; tests and benchmarks can call it.
    pub fn spawn_projectile(&mut self, position: Vec3Fp, projectile: Projectile) -> SimId {
        let id = self.next_id();
        let entity = self.ecs.spawn((id, Position(position), projectile));
        self.projectiles.push((id, entity));
        self.launched.push((id, [position.x.to_bits(), position.y.to_bits(), position.z.to_bits()]));
        id
    }

    pub(super) fn max_mobile_radius(&self) -> i32 {
        self.max_mobile_radius
    }

    pub(super) fn max_structure_radius(&self) -> i32 {
        self.max_structure_radius
    }

    pub(super) fn find_unit(&self, id: SimId) -> Option<(usize, hecs::Entity)> {
        let index = self.units.binary_search_by_key(&id, |unit| unit.0).ok()?;
        Some((index, self.units[index].1))
    }

    pub fn unit_entity(&self, id: SimId) -> Option<hecs::Entity> {
        self.find_unit(id).map(|(_, entity)| entity)
    }

    pub fn unit_position(&self, id: SimId) -> Option<Vec3Fp> {
        let (_, entity) = self.find_unit(id)?;
        Some(self.ecs.get::<&Position>(entity).ok()?.0)
    }

    pub fn unit_health(&self, id: SimId) -> Option<Fp> {
        let (_, entity) = self.find_unit(id)?;
        Some(self.ecs.get::<&Health>(entity).ok()?.current)
    }

    /// Queues a hit of `damage` on a unit, from no one, as a shell's that lands on it would be:
    /// dealt at the end of the next tick, and, if it takes all the unit's health, destroying it
    /// there, wreck and all. For tests and scripted scenarios. Does nothing if there is no such unit.
    pub fn strike(&mut self, id: SimId, damage: Fp) {
        self.strikes.push((id, damage));
    }

    pub fn unit_order(&self, id: SimId) -> Option<Order> {
        let (_, entity) = self.find_unit(id)?;
        Some(*self.ecs.get::<&Order>(entity).ok()?)
    }

    /// Advances the world by exactly one 100 ms deterministic tick. Commands are applied
    /// in slice order, so callers must supply the same order on every instance.
    pub fn advance_tick(&mut self, commands: &[SimCommand]) {
        self.step_commands(commands);
        self.rebuild_grid();
        self.step_movement();
        self.step_transports();
        self.rebuild_grid();
        self.step_visibility();
        self.step_combat();
        self.step_projectiles();
        self.resolve_discharges();
        self.step_production();
        self.step_cleanup();
        self.current_tick += 1;
    }

    pub fn step_commands(&mut self, commands: &[SimCommand]) {
        for command in commands {
            match command {
                SimCommand::Move { units, goal } => {
                    self.clear_unit_plans(units);
                    self.assign_route(units, OrderKind::Move, *goal, None);
                }
                SimCommand::AttackMove { units, goal } => {
                    self.clear_unit_plans(units);
                    self.assign_route(units, OrderKind::AttackMove, *goal, None);
                }
                SimCommand::Follow { units, target } => {
                    if let Some(position) = self.unit_position(*target) {
                        let followers: Vec<SimId> = units.iter().copied().filter(|id| id != target).collect();
                        self.clear_unit_plans(&followers);
                        self.assign_route(&followers, OrderKind::Move, position.xy(), Some(*target));
                    }
                }
                SimCommand::Stop { units } => {
                    self.clear_unit_plans(units);
                    for &id in units {
                        self.set_order(id, Order::IDLE);
                    }
                }
                SimCommand::Attack { units, target } => self.command_attack(units, *target),
                SimCommand::AttackGround { units, point } => self.command_attack_ground(units, *point),
                SimCommand::Discharge { unit, target } => self.command_discharge(*unit, *target),
                SimCommand::QueueMove { units, goal, attack } => self.queue_move(units, *goal, *attack, false),
                SimCommand::QueueAttack { units, target } => self.queue_attack(units, *target),
                SimCommand::FormationMove { units, goal, attack, queued: true } => self.queue_move(units, *goal, *attack, true),
                SimCommand::FormationMove { units, goal, attack, queued: false } => {
                    self.clear_unit_plans(units);
                    let kind = if *attack { OrderKind::AttackMove } else { OrderKind::Move };
                    for (id, order) in self.plan_orders(units, kind, *goal, None, false, true) {
                        self.set_order(id, order);
                    }
                }
                SimCommand::MoveOrder { units, task, goal } => self.move_order(units, *task, *goal),
                SimCommand::Enhance { units, enhancement } => {
                    for &id in units {
                        self.start_enhancement(id, *enhancement);
                    }
                }
                SimCommand::Teleport { units, point } => self.command_teleport(units, *point),
                SimCommand::SelfDestruct { units } => self.command_self_destruct(units),
                SimCommand::Refit { units, staging } => self.command_refit(units, *staging),
                SimCommand::Load { units, transport } => self.command_load(units, *transport),
                SimCommand::Unload { transports, point } => self.command_unload(transports, *point),
                other => self.apply_production_command(other),
            }
        }
        self.step_order_queues();
    }

    fn queue_move(&mut self, units: &[SimId], goal: Vec2Fp, attack: bool, formation: bool) {
        let mobile: Vec<SimId> = units
            .iter()
            .copied()
            .filter(|id| self.find_unit(*id).is_some_and(|(_, entity)| !self.ecs.get::<&UnitStats>(entity).expect("units have stats").is_structure))
            .collect();
        let kind = if attack { OrderKind::AttackMove } else { OrderKind::Move };
        for (id, order) in self.plan_orders(&mobile, kind, goal, None, true, formation) {
            self.enqueue_order(id, order);
        }
    }

    /// An attack on one enemy, after what the units are already doing. Its route follows the
    /// target from now on, so that the attack goes wherever the target has got to by its turn.
    fn queue_attack(&mut self, units: &[SimId], target: SimId) {
        let Some(position) = self.unit_position(target) else {
            return;
        };
        let armed = self.armed_against(units, target);
        for (id, order) in self.plan_orders(&armed, OrderKind::Attack, position.xy(), Some(target), true, false) {
            self.enqueue_order(id, order);
        }
    }

    /// Puts an order at the end of a unit's queue: a builder's tasks, or the order queue.
    fn enqueue_order(&mut self, id: SimId, order: Order) {
        let (_, entity) = self.find_unit(id).expect("orders are planned for live units");
        self.end_patrol(id, entity);
        if self.ecs.get::<&Builder>(entity).is_ok() {
            self.adopt_current_order(id);
            self.ecs.get::<&mut Builder>(entity).expect("just checked").tasks.push(BuildTask::Walk { order, started: false });
        } else if self.ecs.get::<&OrderQueue>(entity).is_ok() {
            self.ecs.get::<&mut OrderQueue>(entity).expect("just checked").0.push(order);
        } else {
            self.ecs.insert_one(entity, OrderQueue(vec![order])).expect("unit is live");
            let at = self.order_queues.partition_point(|queue| queue.0 < id);
            self.order_queues.insert(at, (id, entity));
        }
    }

    /// Starts an order that was waiting its turn. A queued attack fires only at its target,
    /// as one given straight away does; if the target is gone, its route is handed back and
    /// nothing starts. Returns whether the order started.
    pub(super) fn start_queued_order(&mut self, id: SimId, order: Order) -> bool {
        if order.kind == OrderKind::Attack {
            let Some(target) = self.flow_field.follow_target(order.route) else {
                self.flow_field.release(order.route);
                return false;
            };
            if let Some(mut combat) = self.find_unit(id).and_then(|(_, entity)| self.ecs.get::<&mut Combat>(entity).ok()) {
                (combat.forced_target, combat.chasing) = (Some(target), false);
            }
        }
        self.set_order(id, order);
        true
    }

    /// Whether a unit is on, or is waiting to start, an order on this route.
    fn holds_route(&self, id: SimId, route: u32) -> bool {
        let Some((_, entity)) = self.find_unit(id) else {
            return false;
        };
        let in_tasks = self
            .ecs
            .get::<&Builder>(entity)
            .is_ok_and(|builder| builder.tasks.iter().any(|task| matches!(task, BuildTask::Walk { order, .. } if order.route == route)));
        let in_queue = self.ecs.get::<&OrderQueue>(entity).is_ok_and(|queue| queue.0.iter().any(|order| order.route == route));
        let order = *self.ecs.get::<&Order>(entity).expect("units have orders");
        in_tasks || in_queue || (order.kind != OrderKind::Idle && order.route == route)
    }

    fn move_order(&mut self, units: &[SimId], task: BuildTask, goal: Vec2Fp) {
        let BuildTask::Walk { order: old, .. } = task else {
            return;
        };
        // Attacks and follow orders have targets of their own to go to.
        if !matches!(old.kind, OrderKind::Move | OrderKind::AttackMove) || self.flow_field.is_following(old.route) {
            return;
        }
        let holders: Vec<SimId> = units.iter().copied().filter(|id| self.holds_route(*id, old.route)).collect();
        // A formation move stays one.
        let formation = self.formations.contains_key(&old.route);
        for (id, new) in self.plan_orders(&holders, old.kind, goal, None, false, formation) {
            let (_, entity) = self.find_unit(id).expect("just checked");
            let mut waiting = false;
            let mut in_tasks = false;
            if let Ok(mut builder) = self.ecs.get::<&mut Builder>(entity) {
                let at = builder.tasks.iter().position(|task| matches!(task, BuildTask::Walk { order, .. } if order.route == old.route));
                if let Some(at) = at {
                    let started = matches!(builder.tasks[at], BuildTask::Walk { started: true, .. });
                    builder.tasks[at] = BuildTask::Walk { order: new, started };
                    in_tasks = true;
                    waiting = !started;
                }
            }
            if !in_tasks {
                if let Ok(mut queue) = self.ecs.get::<&mut OrderQueue>(entity) {
                    if let Some(at) = queue.0.iter().position(|order| order.route == old.route) {
                        queue.0[at] = new;
                        waiting = true;
                    }
                }
            }
            if waiting {
                // Its order has not started, so only the reference it held needs handing back.
                self.flow_field.release(old.route);
            } else {
                self.set_order(id, new);
            }
        }
    }

    /// Starts the next queued order for units that have finished the last one.
    fn step_order_queues(&mut self) {
        let mut index = 0;
        while index < self.order_queues.len() {
            let (id, entity) = self.order_queues[index];
            if self.ecs.get::<&Order>(entity).expect("units have orders").kind == OrderKind::Idle {
                let mut queue = self.ecs.get::<&mut OrderQueue>(entity).expect("queue list only holds queues");
                let next = queue.0.remove(0);
                let finished = queue.0.is_empty();
                drop(queue);
                let started = self.start_queued_order(id, next);
                if finished {
                    self.ecs.remove_one::<OrderQueue>(entity).expect("just checked");
                    self.order_queues.remove(index);
                    continue;
                }
                if !started {
                    // An attack on something already gone: straight on to the next order.
                    continue;
                }
            }
            index += 1;
        }
    }

    pub(super) fn end_patrol(&mut self, id: SimId, entity: hecs::Entity) {
        if self.ecs.remove_one::<Patrol>(entity).is_ok() {
            self.patrols.retain(|patrol| patrol.0 != id);
        }
    }

    /// Hands back the routes held by orders that never started.
    pub(super) fn release_waiting_orders(&mut self, id: SimId, entity: hecs::Entity) {
        if let Ok(builder) = self.ecs.get::<&Builder>(entity) {
            for task in &builder.tasks {
                if let BuildTask::Walk { order, started: false } = task {
                    self.flow_field.release(order.route);
                }
            }
        }
        if let Ok(queue) = self.ecs.remove_one::<OrderQueue>(entity) {
            for order in &queue.0 {
                self.flow_field.release(order.route);
            }
            self.order_queues.retain(|queue| queue.0 != id);
        }
    }

    /// Where a unit will be once it has done everything it has been told to, so that the
    /// next group order can line up from there.
    fn planned_end(&self, entity: hecs::Entity, here: Vec2Fp) -> Vec2Fp {
        let order_end = |order: &Order| order.slot.or_else(|| self.flow_field.goal(order.route));
        if let Ok(builder) = self.ecs.get::<&Builder>(entity) {
            match builder.tasks.last() {
                Some(BuildTask::Walk { order, .. }) => return order_end(order).unwrap_or(here),
                Some(BuildTask::Build { blueprint, cell_x, cell_y, .. }) => {
                    let unit = self.blueprints.get(*blueprint);
                    return super::production::cell_center(*cell_x, *cell_y, u32::from(unit.footprint));
                }
                Some(_) => return here,
                None => {}
            }
        }
        if let Some(order) = self.ecs.get::<&OrderQueue>(entity).ok().and_then(|queue| queue.0.last().copied()) {
            return order_end(&order).unwrap_or(here);
        }
        let order = *self.ecs.get::<&Order>(entity).expect("units have orders");
        if order.kind == OrderKind::Idle { here } else { order_end(&order).unwrap_or(here) }
    }

    /// Forgets a unit's patrol, build tasks, and forced target. A new order replaces them.
    pub(super) fn clear_unit_plans(&mut self, units: &[SimId]) {
        for &id in units {
            let Some((_, entity)) = self.find_unit(id) else {
                continue;
            };
            if let Ok(mut combat) = self.ecs.get::<&mut Combat>(entity) {
                combat.forced_target = None;
                combat.ground_target = None;
            }
            if let Ok(mut factory) = self.ecs.get::<&mut super::components::Factory>(entity) {
                factory.assisting = None;
            }
            let _ = self.ecs.remove_one::<DischargeOrder>(entity);
            // And an aircraft's trip to refuel.
            if let Ok(mut flight) = self.ecs.get::<&mut Flight>(entity) {
                (flight.refit, flight.docked) = (None, false);
            }
            // A new order cancels a commander's upgrade, enhancement, or teleport, and what was
            // spent on it is lost.
            if self.ecs.remove_one::<Upgrading>(entity).is_ok() {
                self.upgrading.retain(|unit| unit.0 != id);
            }
            self.cancel_enhancement(id, entity);
            self.release_waiting_orders(id, entity);
            if let Ok(mut builder) = self.ecs.get::<&mut Builder>(entity) {
                builder.tasks.clear();
            }
            self.end_patrol(id, entity);
        }
    }

    fn command_discharge(&mut self, unit: SimId, target: SimId) {
        let Some((_, entity)) = self.find_unit(unit) else {
            return;
        };
        let Some((_, target_entity)) = self.find_unit(target) else {
            return;
        };
        let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
        let is_enemy = self.ecs.get::<&UnitStats>(target_entity).is_ok_and(|stats| stats.team != team);
        if self.ecs.get::<&DischargeWeapon>(entity).is_err() || !is_enemy {
            return;
        }
        self.clear_unit_plans(&[unit]);
        self.ecs.insert_one(entity, DischargeOrder { target }).expect("unit is live");
        let position = self.unit_position(target).expect("just found").xy();
        self.assign_route(&[unit], OrderKind::Move, position, Some(target));
    }

    /// Those of `units` with a weapon that can hit `target`: anti-air for an aircraft, a
    /// torpedo for a submarine, and for anything else, a ground weapon, or a torpedo for a ship.
    fn armed_against(&self, units: &[SimId], target: SimId) -> Vec<SimId> {
        let (target_flies, target_dives, target_sails) = self.find_unit(target).map_or((false, false, false), |(_, entity)| {
            let flies = self.ecs.get::<&Flight>(entity).is_ok_and(|flight| flight.airborne());
            let class = self.ecs.get::<&UnitStats>(entity).map_or(MoveClass::Land, |stats| stats.move_class);
            let up = self.surfaced.binary_search(&target).is_ok();
            (flies, class == MoveClass::Submarine && !up, class == MoveClass::Naval || (class == MoveClass::Submarine && up))
        });
        units
            .iter()
            .copied()
            .filter(|id| *id != target)
            .filter(|id| {
                self.find_unit(*id).is_some_and(|(_, entity)| {
                    let anti_air = self.ecs.get::<&AntiAir>(entity).ok().map(|anti_air| *anti_air);
                    let torpedo = self.ecs.get::<&super::components::Torpedo>(entity).is_ok();
                    let reaches = |air: bool, ground: bool, torpedo: bool| {
                        if target_dives {
                            torpedo
                        } else if target_flies {
                            air
                        } else {
                            ground || (torpedo && target_sails)
                        }
                    };
                    let main_can = reaches(anti_air.is_some() && !torpedo, anti_air.is_none_or(|anti_air| anti_air.also_ground) && !torpedo, torpedo);
                    // Or any of its extra weapons can.
                    let extra_can = self
                        .ecs
                        .get::<&super::components::ExtraWeapons>(entity)
                        .is_ok_and(|extras| extras.0.iter().any(|weapon| reaches(weapon.hits_air, weapon.hits_ground, weapon.torpedo)));
                    let can_hit = main_can || extra_can;
                    can_hit && self.ecs.get::<&Combat>(entity).is_ok_and(|combat| combat.range > Fp::ZERO)
                })
            })
            .collect()
    }

    fn command_attack(&mut self, units: &[SimId], target: SimId) {
        let Some(position) = self.unit_position(target) else {
            // A structure the team remembers, gone since without its knowing: its units fire at
            // where it was, and find nothing.
            let team = units.iter().find_map(|&id| self.find_unit(id).and_then(|(_, entity)| self.ecs.get::<&UnitStats>(entity).ok().map(|stats| stats.team)));
            let remembered = team.and_then(|team| {
                let known = &self.known_structures[team.index()];
                known.binary_search_by_key(&target, |record| record.id).ok().map(|index| known[index])
            });
            if let Some(record) = remembered {
                self.command_attack_ground(units, Vec2Fp::new(Fp::from_bits(record.x), Fp::from_bits(record.y)));
            }
            return;
        };
        let armed = self.armed_against(units, target);
        self.clear_unit_plans(&armed);
        for &id in &armed {
            let (_, entity) = self.find_unit(id).expect("just checked");
            let mut combat = self.ecs.get::<&mut Combat>(entity).expect("just checked");
            (combat.forced_target, combat.chasing) = (Some(target), false);
        }
        self.assign_route(&armed, OrderKind::Attack, position.xy(), Some(target));
    }

    /// Armed units fire at a spot until told otherwise, and those that can walk go into range.
    /// The spot can be anywhere on the map, even out of sight.
    fn command_attack_ground(&mut self, units: &[SimId], point: Vec2Fp) {
        if !self.terrain.in_bounds(point) {
            return;
        }
        let armed: Vec<SimId> = units
            .iter()
            .copied()
            .filter(|id| self.find_unit(*id).is_some_and(|(_, entity)| self.ecs.get::<&Combat>(entity).is_ok_and(|combat| combat.range > Fp::ZERO)))
            .collect();
        self.clear_unit_plans(&armed);
        let mut walkers = Vec::with_capacity(armed.len());
        for &id in &armed {
            let (_, entity) = self.find_unit(id).expect("just checked");
            let mut combat = self.ecs.get::<&mut Combat>(entity).expect("just checked");
            combat.ground_target = Some(point);
            combat.target = None;
            drop(combat);
            if self.ecs.get::<&UnitStats>(entity).expect("units have stats").is_structure {
                continue;
            }
            walkers.push(id);
        }
        self.assign_route(&walkers, OrderKind::Attack, point, None);
    }

    pub(super) fn set_order(&mut self, id: SimId, order: Order) {
        if let Some((_, entity)) = self.find_unit(id) {
            let mut current = self.ecs.get::<&mut Order>(entity).expect("units have orders");
            if current.kind != OrderKind::Idle {
                self.flow_field.release(current.route);
            }
            *current = order;
        }
    }

    pub(super) fn assign_route(&mut self, units: &[SimId], kind: OrderKind, goal: Vec2Fp, follow: Option<SimId>) {
        for (id, order) in self.plan_orders(units, kind, goal, follow, false, false) {
            self.set_order(id, order);
        }
    }

    /// Works out a group's route and each unit's order, without giving them yet. Each order
    /// holds one reference to the route. With `from_planned_end`, a formation is lined up from
    /// where units will be after what they are already doing, instead of from where they are.
    /// With `travel`, a group of two or more also keeps that formation on the way.
    fn plan_orders(
        &mut self,
        units: &[SimId],
        kind: OrderKind,
        goal: Vec2Fp,
        follow: Option<SimId>,
        from_planned_end: bool,
        travel: bool,
    ) -> Vec<(SimId, Order)> {
        // Ships path over the water, hovercraft and seabed walkers over land and water, and
        // everything else over the land, so a group of several kinds gets a route for each.
        let family = |world: &Self, id: SimId| {
            world.find_unit(id).map_or(MoveClass::Land, |(_, entity)| match world.ecs.get::<&UnitStats>(entity).map_or(MoveClass::Land, |stats| stats.move_class) {
                MoveClass::Naval | MoveClass::Submarine => MoveClass::Naval,
                // Without water, amphibians go the way land units do, together with them.
                MoveClass::Hover | MoveClass::Seabed if world.terrain.has_water() => MoveClass::Hover,
                MoveClass::Hover | MoveClass::Seabed | MoveClass::Land | MoveClass::Air => MoveClass::Land,
            })
        };
        let families: Vec<MoveClass> = units.iter().map(|&id| family(self, id)).collect();
        let class = families.first().copied().unwrap_or(MoveClass::Land);
        if families.iter().any(|&other| other != class) {
            let mut orders = Vec::new();
            for wanted in [MoveClass::Land, MoveClass::Hover, MoveClass::Naval] {
                let group: Vec<SimId> = units.iter().zip(&families).filter(|&(_, &found)| found == wanted).map(|(&id, _)| id).collect();
                if !group.is_empty() {
                    orders.extend(self.plan_orders(&group, kind, goal, follow, from_planned_end, travel));
                }
            }
            return orders;
        }
        // Keeps the order given, because units take their orders in that order.
        let mut seen = BTreeSet::new();
        let mut members = Vec::with_capacity(units.len());
        let mut walkers = Vec::with_capacity(units.len());
        let mut largest_radius = 0;
        // Whose units they are, whose share of the field builds their route draws on.
        let mut owner = 0;
        for &id in units {
            if let Some((_, entity)) = self.find_unit(id) {
                if !seen.insert(id) {
                    continue;
                }
                members.push(id);
                let stats = self.ecs.get::<&UnitStats>(entity).expect("units have stats");
                if members.len() == 1 {
                    owner = stats.owner.index();
                }
                largest_radius = largest_radius.max(stats.radius.to_bits());
                // Aircraft fly straight to the goal itself, and keep no formation.
                if !stats.is_structure && stats.move_class != MoveClass::Air {
                    let here = self.ecs.get::<&Position>(entity).expect("units have positions").0.xy();
                    let start = if from_planned_end { self.planned_end(entity, here) } else { here };
                    walkers.push((id, start, stats.max_speed));
                }
            }
        }
        if members.is_empty() {
            return Vec::new();
        }
        // Large groups stop further out, because they cannot all fit at the goal.
        let spread = (members.len() as u64).isqrt().max(1) as i64;
        let mut arrive_radius = Fp::from_bits((i64::from(largest_radius) * spread * 6 / 5) as i32);

        // Following means staying beside the unit, not pushing against it.
        if let (OrderKind::Move, Some((_, target))) = (kind, follow.and_then(|target| self.find_unit(target))) {
            let target_radius = self.ecs.get::<&UnitStats>(target).expect("units have stats").radius;
            arrive_radius += target_radius + Fp::from_bits(largest_radius.saturating_mul(2));
        }

        // A group ordered to a place ends up in a formation there.
        let mut slots: Vec<(SimId, Vec2Fp, u32)> = Vec::new();
        let mut keep_formation = None;
        if follow.is_none() && matches!(kind, OrderKind::Move | OrderKind::AttackMove) && walkers.len() >= 2 {
            let positions: Vec<(SimId, Vec2Fp)> = walkers.iter().map(|&(id, position, _)| (id, position)).collect();
            let spacing = Fp::from_bits(largest_radius * FORMATION_SPACING_RADII / 2);
            let formation = plan_formation(&positions, goal, spacing);
            arrive_radius = formation.approach_radius;
            if travel {
                keep_formation = Some(Travel::plan(&walkers, &formation, goal, spacing));
            }
            for (&(id, _, speed), &slot) in walkers.iter().zip(&formation.slots) {
                let slot = self.nearest_open_point(slot, class);
                // Time to cross the whole approach twice, plus a margin for crowding.
                let crossing = i64::from(arrive_radius.to_bits()) * 4 / i64::from(speed.to_bits().max(1));
                slots.push((id, slot, (crossing + SLOT_EXTRA_TICKS).min(i64::from(u32::MAX)) as u32));
            }
        }
        let route = self.flow_field.create_route_for(class, &self.terrain, self.current_tick, goal, members.len() as u32, arrive_radius, follow, owner).or_else(|| {
            // Aircraft alone fly straight to the goal, so they go even where no land unit could
            // route to it, over a sea with no land at all (2026-11-24: they once ignored orders
            // there, on the balance harness's open sea).
            walkers.is_empty().then(|| self.flow_field.create_air_route(&self.terrain, self.current_tick, goal, members.len() as u32, arrive_radius, follow, owner))
        });
        let Some(route) = route else {
            return Vec::new();
        };
        if let Some(travel) = keep_formation {
            self.formations.insert(route, travel);
        }
        slots.sort_unstable_by_key(|slot| slot.0);
        let route_goal = self.flow_field.goal(route).unwrap_or(goal);
        members
            .into_iter()
            .map(|id| {
                let (slot, slot_ticks) = slots
                    .binary_search_by_key(&id, |slot| slot.0)
                    .map_or((None, 0), |index| (Some(slots[index].1), slots[index].2));
                // Whether it can walk straight there, from the start, rather than only once it
                // next looks (step_movement).
                let direct = follow.is_none()
                    && self.find_unit(id).is_some_and(|(_, entity)| {
                        let (Ok(position), Ok(stats)) = (self.ecs.get::<&Position>(entity), self.ecs.get::<&UnitStats>(entity)) else {
                            return false;
                        };
                        let passable = |point: Vec2Fp| self.terrain.passable_for(stats.move_class, point);
                        straight_way_is_clear(&passable, position.0.xy(), route_goal, stats.radius)
                    });
                (id, Order { kind, route, slot, slot_ticks, direct })
            })
            .collect()
    }

    /// The point itself if units of this class can be there and it is on the map, or else the
    /// nearest open cell's center.
    fn nearest_open_point(&self, point: Vec2Fp, class: MoveClass) -> Vec2Fp {
        let max = self.terrain.max_coordinate_raw();
        let inside = Vec2Fp::new(Fp::from_bits(point.x.to_bits().clamp(0, max)), Fp::from_bits(point.y.to_bits().clamp(0, max)));
        if self.terrain.passable_for(class, inside) {
            return inside;
        }
        nearest_passable_cell_for(&self.terrain, class, inside).map_or(inside, |(x, y)| {
            let center = |cell: usize| Fp::from_bits((cell as i32 * TERRAIN_CELL_UNITS + TERRAIN_CELL_UNITS / 2) << FP_FRAC_BITS);
            Vec2Fp::new(center(x), center(y))
        })
    }

    /// Moves each formation's leader along its route, unless the group should wait: for members
    /// still finishing earlier orders, for a fight on an attack-move, or for a while, for members
    /// out of place. A formation ends when fewer than two members are left on it.
    fn step_formations(&mut self) {
        if self.formations.is_empty() {
            return;
        }
        let tick = self.current_tick;
        let mut formations = std::mem::take(&mut self.formations);
        formations.retain(|&route, travel| {
            travel.members.retain(|member| self.holds_route(member.0, route));
            let (Some(goal), Some(approach)) = (self.flow_field.goal(route), self.flow_field.arrive_radius(route)) else {
                return false;
            };
            if travel.members.len() < 2 {
                return false;
            }
            let (mut waiting, mut lagging, mut fighting) = (0, 0, false);
            for &(id, _, _) in &travel.members {
                let (_, entity) = self.find_unit(id).expect("members hold the route, so they are alive");
                let order = *self.ecs.get::<&Order>(entity).expect("units have orders");
                let combat = self.ecs.get::<&Combat>(entity).ok().map(|combat| *combat);
                if order.route != route {
                    // A member chasing a target by route is away fighting, not still finishing
                    // an earlier order, and the group goes on without it.
                    if !combat.is_some_and(|combat| combat.chasing) {
                        waiting += 1;
                    }
                    continue;
                }
                let here = self.ecs.get::<&Position>(entity).expect("units have positions").0.xy();
                let spot = travel.spot(id).expect("it is a member");
                // Near the goal, members leave for their own spots, so they are not behind.
                if !within(here, spot, travel.lag_distance) && !within(here, goal, approach) {
                    lagging += 1;
                }
                // Only a member firing at its target holds the group. One chasing a target out of
                // range would hold it for as long as the chase takes, so it catches up after.
                let firing = combat.and_then(|combat| combat.target.map(|target| (combat.range, target))).is_some_and(|(range, target)| {
                    self.find_unit(target).is_some_and(|(_, target)| {
                        let radius = self.ecs.get::<&UnitStats>(target).map_or(Fp::ZERO, |stats| stats.radius);
                        self.ecs.get::<&Position>(target).is_ok_and(|position| within(here, position.0.xy(), range + radius))
                    })
                });
                fighting |= order.kind == OrderKind::AttackMove && firing;
            }
            let behind = lagging * 4 > travel.members.len();
            travel.held_ticks = if behind { travel.held_ticks.saturating_add(1) } else { 0 };
            if waiting > 0 || fighting || (behind && travel.held_ticks <= FORMATION_PATIENCE_TICKS) {
                return true;
            }
            let leader = travel.leader;
            if within(leader, goal, travel.speed) {
                travel.leader = goal;
                return true;
            }
            let direction = if self.flow_field.goal_cell(route) == Some(self.terrain.unit_cell_of(leader)) {
                (goal - leader).normalized()
            } else {
                match self.flow_field.sample(route, tick, leader, &self.terrain) {
                    FlowSample::Direction(direction) => direction,
                    FlowSample::Hold => return true,
                    // Members find out for themselves, and drop the order.
                    FlowSample::Unreachable => return false,
                }
            };
            travel.leader = leader + direction.scaled(travel.speed);
            travel.heading = rotate_toward(travel.heading, direction, travel.max_turn);
            true
        });
        self.formations = formations;
    }

    pub fn rebuild_grid(&mut self) {
        let terrain = &self.terrain;
        let surfaced = &self.surfaced;
        let mut query = self.ecs.query::<(&Position, &UnitStats, &Order, Option<&Flight>, Option<&Velocity>, Option<&Builder>)>();
        let view = query.view();
        self.spatial_grid.rebuild(self.units.iter().map(|&(id, entity)| {
            let (position, stats, order, flight, velocity, builder) = view.get(entity).expect("unit list only holds live units");
            GridUnit {
                id,
                entity,
                team: stats.team,
                x: position.0.x.to_bits(),
                y: position.0.y.to_bits(),
                z: position.0.z.to_bits(),
                radius: stats.radius.to_bits(),
                is_structure: stats.is_structure,
                // A landed aircraft is on the ground.
                is_air: flight.is_some_and(Flight::airborne),
                // A seabed walker under the water is as submerged as a submarine.
                is_sub: (stats.move_class == MoveClass::Submarine && surfaced.binary_search(&id).is_err())
                    || (stats.move_class == MoveClass::Seabed && position.0.z + stats.radius < terrain.water_level()),
                is_naval: (stats.move_class == MoveClass::Naval && !stats.is_structure) || (stats.move_class == MoveClass::Submarine && surfaced.binary_search(&id).is_ok()),
                // A builder standing still at work has somewhere to be: a unit that bumps into it
                // has not reached the crowd round its own goal. Counted as settled, a commander
                // building beside a factory stopped the engineers coming out of it short of the
                // rally point, stuck against it.
                is_idle: order.kind == OrderKind::Idle && builder.is_none_or(|builder| builder.tasks.is_empty()),
                velocity: velocity.map_or([0, 0], |velocity| [velocity.0.x.to_bits(), velocity.0.y.to_bits()]),
            }
        }));
    }

    pub fn step_movement(&mut self) {
        self.step_formations();
        let tick = self.current_tick;
        let formations = &self.formations;
        let units = &self.units;
        let grid = &self.spatial_grid;
        let terrain = &self.terrain;
        let flow = &mut self.flow_field;
        flow.update_moving_goals(terrain, tick, |target| {
            let index = units.binary_search_by_key(&target, |unit| unit.0).ok()?;
            let entry = grid.entry(index);
            Some(Vec2Fp::new(Fp::from_bits(entry.x), Fp::from_bits(entry.y)))
        });
        flow.build_pending(terrain, tick);

        let max_radius = self.max_mobile_radius;
        let max_coordinate = terrain.max_coordinate_raw();
        let disabled = &self.disabled;
        let surfaced = &self.surfaced;
        let mut unreachable = Vec::new();
        let mut chases = Vec::new();
        // Units that have driven out of their factory, with where they were sent.
        let mut left_factory = Vec::new();
        let mut query = self.ecs.query_mut::<(&mut Position, &mut Velocity, &UnitStats, &mut Order, &Combat, Option<&mut Turning>, Option<&Builder>, Option<&LeavingFactory>, Option<&super::components::ExtraWeapons>)>();
        let mut view = query.view();
        for (index, &(id, entity)) in units.iter().enumerate() {
            let (position, velocity, stats, order, combat, turning, builder, leaving, extras) = view.get_mut(entity).expect("unit list only holds live units");
            if stats.is_structure || stats.move_class == MoveClass::Air {
                continue;
            }
            // Stunned, or held in a tractor beam, it cannot move.
            if !disabled.is_empty() && disabled.binary_search_by_key(&id, |unit| unit.0).is_ok() {
                velocity.0 = Vec3Fp::ZERO;
                continue;
            }
            let here = position.0.xy();
            // Ships keep to deep water, and float on it.
            let class = stats.move_class;
            let passable = |point: Vec2Fp| terrain.passable_for(class, point);
            // A submarine runs just under the water, or on it, surfaced, as a ship does.
            let up = class == MoveClass::Submarine && surfaced.binary_search(&id).is_ok();
            let surface = |point: Vec2Fp| match class {
                MoveClass::Naval | MoveClass::Hover => terrain.height_at(point).max(terrain.water_level()),
                MoveClass::Submarine if up => terrain.water_level(),
                MoveClass::Submarine => terrain.water_level() - stats.radius * 2,
                _ => terrain.height_at(point),
            };

            // Just built: drive straight out of the factory's open end, facing the way out, and
            // only then follow orders.
            if let Some(leaving) = leaving {
                velocity.0 = Vec3Fp::ZERO;
                let remaining = sqrt_wide(here.distance_squared(&leaving.exit));
                let speed = stats.max_speed.max(Fp::ONE);
                let next = if remaining <= speed {
                    left_factory.push((id, entity, leaving.rally));
                    leaving.exit
                } else {
                    let direction = (leaving.exit - here).normalized();
                    if let Some(turning) = turning {
                        turning.facing = direction;
                    }
                    here + direction.scaled(speed)
                };
                position.0 = Vec3Fp::new(next.x, next.y, surface(next) + stats.radius);
                continue;
            }

            // A structure was placed on top of this unit: walk straight out, through it if need be.
            if !passable(here) {
                if let Some((cell_x, cell_y)) = nearest_passable_cell_for(terrain, class, here) {
                    let half_cell = TERRAIN_CELL_UNITS << FP_FRAC_BITS >> 1;
                    let cell_size = TERRAIN_CELL_UNITS << FP_FRAC_BITS;
                    let exit = Vec2Fp::new(
                        Fp::from_bits(cell_x as i32 * cell_size + half_cell),
                        Fp::from_bits(cell_y as i32 * cell_size + half_cell),
                    );
                    let direction = (exit - here).normalized();
                    let speed = stats.max_speed.max(Fp::ONE);
                    let next = here + direction.scaled(speed);
                    position.0 = Vec3Fp::new(next.x, next.y, surface(next) + stats.radius);
                }
                continue;
            }

            // On an attack-move, a target that has left range is chased. While it is near and the
            // way to it open, the unit walks straight at it; otherwise it is given an attack order
            // on it, which finds a way.
            let chasing_toward = combat
                .target
                .filter(|_| order.kind == OrderKind::AttackMove && builder.is_none())
                .and_then(|target| {
                    let entry = grid.entry(units.binary_search_by_key(&target, |unit| unit.0).ok()?);
                    Some((target, Vec2Fp::new(Fp::from_bits(entry.x), Fp::from_bits(entry.y)), Fp::from_bits(entry.radius)))
                })
                .filter(|&(_, point, radius)| !within(here, point, combat.range + radius))
                .and_then(|(target, point, radius)| {
                    let direction = (point - here).normalized();
                    let open = within(here, point, combat.range + radius + fp_int(DIRECT_CHASE_UNITS)) && passable(here + direction.scaled(stats.radius + stats.max_speed));
                    if !open {
                        chases.push((target, id));
                    }
                    open.then_some(point)
                });
            // An ordered target or spot that the ground hides from the gun is walked straight at,
            // until the shot is clear.
            let chasing_toward = chasing_toward.or_else(|| {
                if !combat.line_blocked {
                    return None;
                }
                let point = combat.ground_target.or_else(|| {
                    let entry = grid.entry(units.binary_search_by_key(&combat.forced_target?, |unit| unit.0).ok()?);
                    Some(Vec2Fp::new(Fp::from_bits(entry.x), Fp::from_bits(entry.y)))
                })?;
                if within(here, point, stats.radius) {
                    return None;
                }
                let direction = (point - here).normalized();
                passable(here + direction.scaled(stats.radius + stats.max_speed)).then_some(point)
            });

            // Every so often, it looks whether the way straight to its goal is clear. Not when it
            // follows a unit, which it would walk straight into.
            if order.kind != OrderKind::Idle && !flow.is_following(order.route) {
                let every = if order.direct { DIRECT_RECHECK_TICKS } else { DIRECT_CHECK_TICKS };
                if stats.phase.wrapping_add(tick).is_multiple_of(every) {
                    order.direct = flow.goal(order.route).is_some_and(|goal| straight_way_is_clear(&passable, here, goal, stats.radius));
                }
            }
            // Near its goal, it walks straight only while it faces it: one that turns in arcs,
            // aiming at the very point from beside it, circled it for good, where its route brings
            // it into the goal's cell.
            let straight = order.direct && !flow.is_following(order.route) && flow.goal(order.route).is_some_and(|goal| {
                let circle = turning.as_ref().map_or(Fp::ZERO, |turning| {
                    let walking = if turning.max_turn > Fp::ZERO { stats.max_speed.saturating_div(turning.max_turn) } else { Fp::ZERO };
                    walking.max(turning.radius)
                });
                let handover = circle.saturating_mul(fp_int(3)).max(fp_int(DIRECT_HANDOVER_UNITS));
                let facing_it = turning.as_ref().is_none_or(|turning| {
                    let away = goal - here;
                    away == Vec2Fp::ZERO || {
                        let toward = away.normalized();
                        turning.facing.x * toward.x + turning.facing.y * toward.y >= Fp::from_num(DIRECT_FACING_COSINE)
                    }
                });
                facing_it || !within(here, goal, handover)
            });

            // Steering from the unit's order.
            let mut desired = Vec2Fp::ZERO;
            if let Some(point) = chasing_toward {
                desired = (point - here).normalized().scaled(stats.max_speed);
            } else if order.kind != OrderKind::Idle {
                let goal = flow.goal(order.route).unwrap_or(here);
                let arrive_radius = flow.arrive_radius(order.route).unwrap_or(Fp::ZERO);
                // A flow field ends at the goal's cell, so the last steps are a straight walk.
                // If the goal itself cannot be walked to, reaching that cell is as close as it gets.
                let in_goal_cell = flow.goal_cell(order.route) == Some(terrain.unit_cell_of(here));
                let goal_is_here = in_goal_cell && terrain.in_bounds(goal) && terrain.unit_cell_of(goal) == terrain.unit_cell_of(here);
                let arrived = here.distance_squared(&goal) <= wide_mul(arrive_radius, arrive_radius) || (in_goal_cell && !goal_is_here);
                let walk_to_goal = |speed: Fp| {
                    let direction = (goal - here).normalized();
                    let speed = speed.min(sqrt_wide(here.distance_squared(&goal)));
                    direction.scaled(speed)
                };
                // A ground attack stops as soon as its spot is in range.
                let shelling_ground = combat.ground_target.is_some_and(|spot| within(here, spot, combat.range)) && !combat.line_blocked;
                // So does an attack, once any weapon is firing at what it was told to attack: a
                // submarine's deck gun at something on land its torpedoes cannot reach.
                let extra_engaged = combat.forced_target.is_some() && extras.is_some_and(|extras| extras.0.iter().any(|weapon| weapon.target == combat.forced_target));
                let may_walk = (order.kind == OrderKind::Move || (combat.target.is_none() && !extra_engaged) || combat.line_blocked) && !shelling_ground;
                if let Some(slot) = order.slot {
                    // Close enough to the group's goal to leave the route and walk to its own spot.
                    if arrived {
                        let close_enough = stats.radius / 2;
                        if order.slot_ticks == 0 || here.distance_squared(&slot) <= wide_mul(close_enough, close_enough) {
                            flow.release(order.route);
                            *order = Order::IDLE;
                        } else if may_walk {
                            order.slot_ticks -= 1;
                            let direction = (slot - here).normalized();
                            let remaining = sqrt_wide(here.distance_squared(&slot));
                            let speed = stats.max_speed.min(remaining);
                            desired = direction.scaled(speed);
                        }
                    } else if may_walk {
                        // On a formation move, a member keeps to its spot unless it has fallen too far behind.
                        let spot = formations.get(&order.route).and_then(|travel| {
                            travel.spot(id).filter(|spot| within(here, *spot, travel.catch_up_distance) && passable(*spot))
                        });
                        if goal_is_here {
                            desired = walk_to_goal(stats.max_speed);
                        } else if let Some(spot) = spot {
                            let remaining = sqrt_wide(here.distance_squared(&spot));
                            if remaining > Fp::ZERO {
                                let direction = (spot - here).normalized();
                                let speed = stats.max_speed.min(remaining);
                                desired = direction.scaled(speed);
                            }
                        } else if straight {
                            desired = walk_to_goal(stats.max_speed);
                        } else {
                            match flow.sample(order.route, tick, here, terrain) {
                                FlowSample::Direction(direction) => desired = direction.scaled(stats.max_speed),
                                FlowSample::Unreachable => {
                                    flow.release(order.route);
                                    *order = Order::IDLE;
                                    unreachable.push(id);
                                }
                                // Its route's way is still being worked out: it keeps going as
                                // it was, rather than braking for a tick each time it is given a
                                // new order the same way.
                                FlowSample::Hold => desired = Vec2Fp::new(velocity.0.x, velocity.0.y),
                            }
                        }
                    }
                } else if arrived {
                    if !flow.is_following(order.route) {
                        flow.release(order.route);
                        *order = Order::IDLE;
                    }
                } else if may_walk {
                    if goal_is_here || straight {
                        desired = walk_to_goal(stats.max_speed);
                    } else {
                        match flow.sample(order.route, tick, here, terrain) {
                            FlowSample::Direction(direction) => desired = direction.scaled(stats.max_speed),
                            FlowSample::Unreachable => {
                                flow.release(order.route);
                                *order = Order::IDLE;
                                unreachable.push(id);
                            }
                            FlowSample::Hold => desired = Vec2Fp::new(velocity.0.x, velocity.0.y),
                        }
                    }
                }
            }

            // A unit with a turn rate can only go the way it faces.
            if let Some(turning) = turning {
                let heading_for = if let Some(point) = chasing_toward {
                    point
                } else if order.kind == OrderKind::Idle {
                    here
                } else {
                    order.slot.filter(|_| desired != Vec2Fp::ZERO).unwrap_or_else(|| flow.goal(order.route).unwrap_or(here))
                };
                desired = turning.steer(desired, sqrt_wide(here.distance_squared(&heading_for)));
            }

            // Ease velocity towards the steering direction to round off the 8-way flow directions.
            let half = |from: Fp, to: Fp| Fp::from_bits(from.to_bits() + (to.to_bits() - from.to_bits()) / 2);
            velocity.0 = Vec3Fp::new(half(velocity.0.x, desired.x), half(velocity.0.y, desired.y), Fp::ZERO);

            // Local separation. Reads positions from the grid, so results cannot depend on update order.
            let me = grid.entry(index);
            let (mut push_x, mut push_y) = (0i64, 0i64);
            // A friend in the way that is stopped, or going much slower the same way, is gone round:
            // the unit slides sideways past it, away from the side it is on, rather than shoving it
            // ahead of itself, as an engineer did a commander at work for 200 units.
            let (mut slide_x, mut slide_y) = (0i64, 0i64);
            let (vx, vy) = (i64::from(velocity.0.x.to_bits()), i64::from(velocity.0.y.to_bits()));
            let my_speed_squared = vx * vx + vy * vy;
            let mut touches_settled_friend = false;
            grid.for_each_candidate(None, Layer::Mobile, me.x, me.y, me.radius + max_radius, |other| {
                if other.index == me.index {
                    return;
                }
                let (dx, dy) = (i64::from(me.x - other.x), i64::from(me.y - other.y));
                let contact = i64::from(me.radius + other.radius);
                let distance_squared = dx * dx + dy * dy;
                if distance_squared >= contact * contact {
                    return;
                }
                touches_settled_friend |= other.is_idle && other.team == me.team;
                if other.team == me.team && my_speed_squared > 0 {
                    let ahead = -(dx * vx + dy * vy) > 0;
                    let its_way = i64::from(other.velocity[0]) * vx + i64::from(other.velocity[1]) * vy;
                    if ahead && its_way * 2 < my_speed_squared {
                        // Which side of the unit's path it is on; dead ahead, a side from their ids.
                        let cross = dx * vy - dy * vx;
                        let side = if cross != 0 { cross.signum() } else if me.id < other.id { 1 } else { -1 };
                        slide_x += side * vy;
                        slide_y -= side * vx;
                    }
                }
                let distance = (distance_squared as u64).isqrt() as i64;
                let overlap = contact - distance;
                if distance == 0 {
                    // Exactly overlapping units push apart along a direction from their ids.
                    let pair = (me.id.0.min(other.id.0) ^ me.id.0.max(other.id.0)) as usize % 8;
                    let (nx, ny) = [(1, 0), (0, 1), (1, 1), (1, -1), (2, 1), (1, 2), (2, -1), (1, -2)][pair];
                    let sign = if me.id > other.id { 1 } else { -1 };
                    push_x += sign * nx * overlap / 2;
                    push_y += sign * ny * overlap / 2;
                } else {
                    push_x += dx * overlap / distance;
                    push_y += dy * overlap / distance;
                }
            });
            // A crowd can cover a shared goal, such as a rally point, so that later units can
            // never reach it. A unit that bumps into a settled friend nearby has arrived too.
            // Units with a spot of their own have a time limit instead.
            // Not a builder on its way to work, which arrives once its work is in reach: settled
            // by a crowd short of that, it was given the same walk again on the next tick, and
            // again, and stood there for the rest of the match.
            let walking_to_work = builder.is_some_and(|builder| !builder.tasks.is_empty());
            if touches_settled_friend
                && !walking_to_work
                && order.slot.is_none()
                && matches!(order.kind, OrderKind::Move | OrderKind::AttackMove)
                && !flow.is_following(order.route)
            {
                let settle_radius = stats.radius * CROWD_SETTLE_RADII;
                let goal = flow.goal(order.route).unwrap_or(here);
                if here.distance_squared(&goal) <= wide_mul(settle_radius, settle_radius) {
                    flow.release(order.route);
                    *order = Order::IDLE;
                }
            }

            // Soft and capped, so dense crowds spread out instead of jittering. The cap keeps up
            // with the unit's own speed, so that fast units cannot simply overpower separation.
            let cap = (i64::from(me.radius) / 2).max(i64::from(stats.max_speed.to_bits()));
            let push_x = (push_x / 3).clamp(-cap, cap) as i32;
            let push_y = (push_y / 3).clamp(-cap, cap) as i32;
            let speed = i64::from(stats.max_speed.to_bits());
            let slide_x = slide_x.clamp(-speed, speed) as i32;
            let slide_y = slide_y.clamp(-speed, speed) as i32;

            let step_x = velocity.0.x.to_bits() + push_x + slide_x;
            let step_y = velocity.0.y.to_bits() + push_y + slide_y;
            if step_x == 0 && step_y == 0 {
                continue;
            }

            // One axis at a time against the cost grid, so units slide along walls.
            let mut next = here;
            let candidate_x = Vec2Fp::new(Fp::from_bits((here.x.to_bits() + step_x).clamp(0, max_coordinate)), next.y);
            if passable(candidate_x) {
                next = candidate_x;
            }
            let candidate_y = Vec2Fp::new(next.x, Fp::from_bits((here.y.to_bits() + step_y).clamp(0, max_coordinate)));
            if passable(candidate_y) {
                next = candidate_y;
            }
            if next != here {
                position.0 = Vec3Fp::new(next.x, next.y, surface(next) + stats.radius);
            }
        }
        // Out of the factory: to the rally point, unless given an order on the way out.
        for (id, entity, rally) in left_factory {
            let _ = self.ecs.remove_one::<LeavingFactory>(entity);
            let idle = self.ecs.get::<&Order>(entity).is_ok_and(|order| order.kind == OrderKind::Idle);
            if let (Some(point), true) = (rally, idle) {
                self.assign_route(&[id], OrderKind::Move, point, None);
            }
        }
        self.step_aircraft();
        self.start_chases(chases);
        self.cancel_unreachable(unreachable);
    }

    /// Sends units on an attack-move after targets too far, or too hard, to walk straight at: each
    /// is given an attack order on its target, and its move waits at the front of its queue, still holding
    /// its route, until the target is destroyed. Units after the same target share a route.
    /// `chases` lists (target, unit) pairs.
    fn start_chases(&mut self, mut chases: Vec<(SimId, SimId)>) {
        chases.sort_unstable();
        for group in chases.chunk_by(|first, second| first.0 == second.0) {
            let target = group[0].0;
            let Some(position) = self.unit_position(target) else {
                continue;
            };
            let units: Vec<SimId> = group.iter().map(|&(_, unit)| unit).collect();
            for (id, order) in self.plan_orders(&units, OrderKind::Attack, position.xy(), Some(target), false, false) {
                let (_, entity) = self.find_unit(id).expect("it just chose its target");
                let paused = std::mem::replace(&mut *self.ecs.get::<&mut Order>(entity).expect("units have orders"), order);
                let queued = self.ecs.get::<&mut OrderQueue>(entity).map(|mut queue| queue.0.insert(0, paused)).is_ok();
                if !queued {
                    self.ecs.insert_one(entity, OrderQueue(vec![paused])).expect("unit is live");
                    let at = self.order_queues.partition_point(|queue| queue.0 < id);
                    self.order_queues.insert(at, (id, entity));
                }
                let mut combat = self.ecs.get::<&mut Combat>(entity).expect("it has a weapon");
                (combat.forced_target, combat.chasing) = (Some(target), true);
            }
        }
    }

    /// Finishes cancelling orders that cannot be reached. A forced target goes with its order,
    /// and a patrol ends, since it would only try the same thing again. Builders drop the task
    /// that led there later this tick.
    fn cancel_unreachable(&mut self, mut units: Vec<SimId>) {
        // A chase that cannot reach its target gives it up for a while, and the unit goes back
        // to its move, which was not cancelled.
        let tick = self.current_tick;
        units.retain(|&id| {
            let (_, entity) = self.find_unit(id).expect("the unit just moved");
            let Ok(mut combat) = self.ecs.get::<&mut Combat>(entity) else {
                return true;
            };
            // One walking closer to what it was ordered to shoot, for a clear shot, that can get
            // no closer, keeps its target and fires from where it is.
            if combat.line_blocked {
                let here = combat.closing.map_or([0, 0], |(at, _)| at);
                (combat.line_blocked, combat.closing) = (false, Some((here, super::components::NO_CLOSER)));
                return false;
            }
            let Some(target) = combat.forced_target.filter(|_| combat.chasing) else {
                return true;
            };
            (combat.forced_target, combat.chasing, combat.target) = (None, false, None);
            combat.blocked = Some((target, tick + UNREACHABLE_CHASE_TICKS));
            false
        });
        for &id in &units {
            let (_, entity) = self.find_unit(id).expect("the unit just moved");
            if let Ok(mut combat) = self.ecs.get::<&mut Combat>(entity) {
                combat.chasing = false;
                combat.forced_target = None;
                combat.ground_target = None;
            }
            self.end_patrol(id, entity);
            let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
            self.orders_cancelled[team.index()] = self.orders_cancelled[team.index()].saturating_add(1);
        }
        self.unreachable = units;
    }

    /// Gives a unit its share of the mass of an enemy it helped destroy, at most one rank's worth
    /// at a time as in the inspiration, and promotes it at the inspiration's thresholds
    /// (`vet_threshold`). Units without a blueprint have no mass, so they earn nothing, and nobody
    /// earns anything for destroying their own side.
    fn award_experience(&mut self, killer: SimId, victim_team: Team, mass: Res) {
        let Some((_, entity)) = self.find_unit(killer) else {
            return;
        };
        let Ok(blueprint) = self.ecs.get::<&Blueprinted>(entity).map(|unit| unit.0) else {
            return;
        };
        let (team, alive) = (self.ecs.get::<&UnitStats>(entity).expect("units have stats").team, self.ecs.get::<&Health>(entity).expect("units have health").current > Fp::ZERO);
        let unit = self.blueprints.get(blueprint);
        if team == victim_team || !alive || unit.mass <= Res::ZERO {
            return;
        }
        let before = self.ecs.get::<&Veterancy>(entity).map_or(Veterancy { experience: Res::ZERO, rank: 0 }, |veterancy| *veterancy);
        if before.rank >= MAX_RANK {
            return;
        }
        // No more than the width of the rank it is working toward.
        let step = vet_threshold(unit, before.rank + 1) - vet_threshold(unit, before.rank);
        let experience = before.experience + mass.min(step);
        let earned = (1..=MAX_RANK).take_while(|&rank| experience >= vet_threshold(unit, rank)).last().unwrap_or(0);
        let rank = before.rank.max(earned);
        if self.ecs.get::<&Veterancy>(entity).is_err() {
            self.ecs.insert_one(entity, Veterancy { experience, rank }).expect("unit is live");
        } else {
            *self.ecs.get::<&mut Veterancy>(entity).expect("just checked") = Veterancy { experience, rank };
        }
        if rank > before.rank {
            if before.rank == 0 {
                let at = self.veterans.partition_point(|veteran| veteran.0 < killer);
                self.veterans.insert(at, (killer, entity));
            }
            let base = i64::from((unit.health + self.enhancement_effects(entity).health).to_bits());
            let new_max = Fp::from_bits((base * (1_000 + RANK_HEALTH_PERMILLE * i64::from(rank)) / 1_000) as i32);
            let mut health = self.ecs.get::<&mut Health>(entity).expect("units have health");
            let gained = new_max - health.max;
            let heal_permille = if unit.is_commander() { COMMANDER_PROMOTION_HEAL_PERMILLE[usize::from(rank) - 1] } else { PROMOTION_HEAL_PERMILLE };
            let heal = Fp::from_bits((base * heal_permille / 1_000) as i32);
            health.max = new_max;
            health.current = (health.current + gained + heal).min(new_max);
        }
    }

    /// Veterans heal a little every tick, more for each rank, by the inspiration's flat amounts.
    fn regenerate_veterans(&mut self) {
        for &(_, entity) in &self.veterans {
            let rank = self.ecs.get::<&Veterancy>(entity).map_or(0, |veterancy| veterancy.rank);
            let Ok(blueprint) = self.ecs.get::<&Blueprinted>(entity).map(|unit| unit.0) else {
                continue;
            };
            let per_second = vet_regen_per_second(self.blueprints.get(blueprint), rank);
            let mut health = self.ecs.get::<&mut Health>(entity).expect("veterans are live units");
            if health.current <= Fp::ZERO {
                continue;
            }
            let per_tick = Fp::from_bits(((per_second << FP_FRAC_BITS) / super::blueprints::TICKS_PER_SECOND) as i32);
            health.current = (health.current + per_tick).min(health.max);
        }
    }

    /// Updates what each team can see. Units take turns, which spreads out the cost.
    pub fn step_visibility(&mut self) {
        if !self.config.fog_of_war {
            return;
        }
        let tick = self.current_tick;
        self.visibility.set_tick(tick);
        let mut query = self.ecs.query::<&UnitStats>();
        let view = query.view();
        for (index, &(_, entity)) in self.units.iter().enumerate() {
            let stats = view.get(entity).expect("unit list only holds live units");
            // Everyone looks on the first ticks, so that a new match starts with sight.
            if tick >= VISION_REFRESH_TICKS && !stats.phase.wrapping_add(tick).is_multiple_of(VISION_REFRESH_TICKS) {
                continue;
            }
            let entry = self.spatial_grid.entry(index);
            self.visibility.stamp_disc(stats.team, entry.x, entry.y, stats.vision);
        }
        drop(query);
        // Submarines must be hidden even from a team with no sensors at all.
        if !self.intel.is_empty() || !self.submarines.is_empty() || !self.seabed_walkers.is_empty() {
            self.step_sensors(tick);
        } else if self.detected.iter().chain(&self.concealed).any(|list| !list.is_empty()) {
            // The last sensor is gone: what it had on radar is no longer on it. Left as it was, a
            // dead scout's contacts stayed on the map, and could be fired at, all match.
            self.detected = [Vec::new(), Vec::new()];
            self.concealed = [Vec::new(), Vec::new()];
        }
        self.step_structure_memory();
    }

    /// Each team notes the enemy structures it sees, as they are now, and those it first finds on
    /// radar, as unknown structures where they stand; and forgets those it sees are gone.
    fn step_structure_memory(&mut self) {
        let mut query = self.ecs.query::<(&Position, &UnitStats, &Health, Option<&Blueprinted>, Option<&Construction>, Option<&Turning>, Option<&Turret>)>();
        let view = query.view();
        let permille = |done: i64, of: i64| (done * 1000 / of.max(1)).clamp(0, 1000) as u16;
        for viewer in [Team::Blue, Team::Red] {
            let enemy = viewer.enemy();
            let (concealed, detected) = (&self.concealed[viewer.index()], &self.detected[viewer.index()]);
            let known = &mut self.known_structures[viewer.index()];
            let mut standing: Vec<SimId> = Vec::new();
            for &(id, entity) in &self.units {
                let (position, stats, health, blueprint, construction, turning, turret) = view.get(entity).expect("unit list only holds live units");
                if !stats.is_structure || stats.team != enemy {
                    continue;
                }
                standing.push(id);
                let (x, y, z) = (position.0.x.to_bits(), position.0.y.to_bits(), position.0.z.to_bits());
                let hidden = concealed.binary_search(&id).is_ok();
                let seen = !hidden && self.visibility.is_visible(viewer, x, y);
                let on_radar = !hidden && detected.binary_search(&id).is_ok();
                let found = known.binary_search_by_key(&id, |record| record.id);
                if !seen && (found.is_ok() || !on_radar) {
                    continue;
                }
                let record = KnownStructure {
                    id,
                    blueprint: seen.then(|| blueprint.map_or(u16::MAX, |blueprint| blueprint.0 .0)),
                    x,
                    y,
                    z,
                    radius: stats.radius.to_bits(),
                    health_permille: if seen { permille(i64::from(health.current.to_bits()), i64::from(health.max.to_bits())) } else { 1000 },
                    built_permille: if seen { construction.map_or(1000, |construction| permille(construction.progress.to_bits(), Res::ONE.to_bits())) } else { 1000 },
                    facing: turning.filter(|_| seen).map_or([0, 0], |turning| [turning.facing.x.to_bits(), turning.facing.y.to_bits()]),
                    aim: turret.filter(|_| seen).map_or([0, 0], |turret| [turret.facing.x.to_bits(), turret.facing.y.to_bits()]),
                    gone: false,
                };
                match found {
                    Ok(index) => known[index] = record,
                    Err(index) => known.insert(index, record),
                }
            }
            for record in known.iter_mut() {
                record.gone = standing.binary_search(&record.id).is_err();
            }
            let visibility = &self.visibility;
            known.retain(|record| !(record.gone && visibility.is_visible(viewer, record.x, record.y)));
        }
    }

    /// Radar and deep scan arrays take their turns with sight, and cloaks are worked out every tick.
    /// A cloaked unit is hidden from a team unless that team has deep scan on it, or a unit of its
    /// own within `CLOAK_REVEAL_UNITS`.
    fn step_sensors(&mut self, tick: u32) {
        for index in 0..self.intel.len() {
            let (id, entity) = self.intel[index];
            let phase = self.ecs.get::<&UnitStats>(entity).map_or(0, |stats| stats.phase);
            if tick >= VISION_REFRESH_TICKS && !phase.wrapping_add(tick).is_multiple_of(VISION_REFRESH_TICKS) {
                continue;
            }
            let running = self.ecs.get::<&Intel>(entity).is_ok_and(|intel| intel.running) && self.ecs.get::<&Construction>(entity).is_err();
            if !running || self.is_disabled(id) {
                continue;
            }
            let (radar, deep_scan) = self.sensor_ranges(entity);
            let sonar = self.ecs.get::<&Blueprinted>(entity).map_or(Fp::ZERO, |unit| self.blueprints.get(unit.0).sonar);
            let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
            let at = self.ecs.get::<&Position>(entity).expect("units have positions").0;
            for (sense, range) in [(Sense::Radar, radar), (Sense::DeepScan, deep_scan), (Sense::Sonar, sonar)] {
                if range > Fp::ZERO {
                    self.visibility.stamp(sense, team, at.x.to_bits(), at.y.to_bits(), range);
                }
            }
        }
        let mut concealed = [Vec::new(), Vec::new()];
        let reach = i64::from(fp_int(CLOAK_REVEAL_UNITS).to_bits());
        for &(id, entity) in &self.intel {
            if !self.ecs.get::<&Intel>(entity).is_ok_and(|intel| intel.cloaked) {
                continue;
            }
            let enemy = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team.enemy();
            let at = self.ecs.get::<&Position>(entity).expect("units have positions").0;
            let (x, y) = (at.x.to_bits(), at.y.to_bits());
            let mut revealed = self.visibility.senses(Sense::DeepScan, enemy, x, y);
            if !revealed {
                let mut near = |other: &super::spatial::GridEntry| {
                    let (dx, dy) = (i64::from(other.x - x), i64::from(other.y - y));
                    revealed |= dx * dx + dy * dy <= reach * reach;
                };
                self.spatial_grid.for_each_candidate(Some(enemy), Layer::Mobile, x, y, reach as i32, &mut near);
                self.spatial_grid.for_each_candidate(Some(enemy), Layer::Structure, x, y, reach as i32 + self.max_structure_radius, &mut near);
            }
            if !revealed {
                concealed[enemy.index()].push(id);
            }
        }
        // A submerged submarine, or seabed walker under the water, is hidden from sight and
        // radar, and only sonar finds it.
        for &(id, entity) in self.submarines.iter().chain(&self.seabed_walkers) {
            let stats = *self.ecs.get::<&UnitStats>(entity).expect("units have stats");
            let enemy = stats.team.enemy();
            let at = self.ecs.get::<&Position>(entity).expect("units have positions").0;
            let submerged = (stats.move_class == MoveClass::Submarine && self.surfaced.binary_search(&id).is_err()) || (stats.move_class != MoveClass::Submarine && at.z + stats.radius < self.terrain.water_level());
            if submerged && !self.visibility.senses(Sense::Sonar, enemy, at.x.to_bits(), at.y.to_bits()) {
                concealed[enemy.index()].push(id);
            }
        }
        if !self.submarines.is_empty() || !self.seabed_walkers.is_empty() {
            for list in &mut concealed {
                list.sort_unstable();
                list.dedup();
            }
        }
        self.concealed = concealed;
        self.detected = [self.radar_contacts(Team::Blue), self.radar_contacts(Team::Red)];
    }

    pub fn step_combat(&mut self) {
        self.launched.clear();
        let tick = self.current_tick;
        let units = &self.units;
        let grid = &self.spatial_grid;
        let terrain = &self.terrain;
        let visibility = &self.visibility;
        let concealed = &self.concealed;
        let detected = &self.detected;
        let known = &self.known_structures;
        let fog = self.config.fog_of_war;
        let shots = &mut self.pending_shots;
        let economies = &mut self.economies;
        let (max_mobile, max_structure) = (self.max_mobile_radius, self.max_structure_radius);
        shots.clear();
        let mut finished_attacks: Vec<SimId> = Vec::new();
        let mut finished_discharges: Vec<SimId> = Vec::new();
        let mut fired_nukes: Vec<SimId> = Vec::new();
        // How each unit is moving and turning, for the guns that lead it, read before the units'
        // weapons are borrowed: as the shots will read them when they are fired.
        let courses: Vec<(Vec3Fp, Vec2Fp)> = units
            .iter()
            .map(|&(_, entity)| {
                let velocity = self.ecs.get::<&Velocity>(entity).map_or(Vec3Fp::ZERO, |velocity| velocity.0);
                let turn = self.ecs.get::<&Flight>(entity).map_or(Vec2Fp::new(Fp::ONE, Fp::ZERO), |flight| flight.turn);
                (velocity, turn)
            })
            .collect();
        // The velocity a shot leaves its gun with, before any scatter, as it will be fired: a gun
        // on a turret points along it before it fires. A shot that steers itself, or a missile,
        // points straight at its target. `lead` is the unit it aims ahead of.
        let surface = |at: Vec2Fp| terrain.height_at(at).max(terrain.water_level());
        let launch = |from: Vec3Fp, to: Vec3Fp, speed: Fp, lead: Option<&super::spatial::GridEntry>, fused: bool, lobbed: bool| {
            let (velocity, turn) = lead.map_or((None, Vec2Fp::new(Fp::ONE, Fp::ZERO)), |entry| {
                let (velocity, turn) = courses[entry.index as usize];
                (Some(velocity), turn)
            });
            let aim = super::combat::lead_aim(from, to, speed, fused, velocity, turn, surface);
            super::combat::launch_velocity(from, aim.to, speed, aim.flak_ticks, aim.least_ticks, lobbed)
        };
        let mut query = self.ecs.query_mut::<(
            &UnitStats,
            &mut Combat,
            Option<&mut Turret>,
            Option<&Turning>,
            Option<&mut DischargeWeapon>,
            Option<&DischargeOrder>,
            Option<&mut Launcher>,
            Option<&AntiAir>,
            Option<&BombBay>,
            Option<&Flight>,
            Option<&Artillery>,
            Option<&super::components::Adjacency>,
            Option<&mut super::components::ExtraWeapons>,
            Option<&super::components::Torpedo>,
            (Option<&super::components::TractorBeam>, Option<&super::components::StunWeapon>, &Order, Option<&Builder>, Option<&super::components::Scatter>, (Option<&mut super::components::Setup>, &Velocity, Option<&super::components::NoseGun>, Option<&mut super::components::Salvo>, Option<&super::components::HighArc>)),
        )>();
        let mut view = query.view();
        let disabled = &self.disabled;
        let grabs = &mut self.pending_grabs;
        grabs.clear();
        for (index, &(id, entity)) in units.iter().enumerate() {
            let (stats, combat, turret, turning, mut discharge, discharge_order, mut launcher, anti_air, bomb_bay, flight, artillery, adjacency, extras, torpedo, (tractor, stun_weapon, order, builder, scatter, (setup, velocity, nose_gun, mut salvo, high_arc))) =
                view.get_mut(entity).expect("unit list only holds live units");
            combat.ticks_until_ready = combat.ticks_until_ready.saturating_sub(1);
            if let Some(salvo) = salvo.as_mut() {
                salvo.tick();
            }
            // Artillery that sets up counts down only while it stands still. Its velocity is its
            // own steering, eased, so a unit coming to a stop is moving until it is below an
            // eighth of its speed, and a push from a neighbour does not pack it up.
            let set_up = setup.is_none_or(|setup| {
                let speed = i128::from(velocity.0.x.to_bits()).pow(2) + i128::from(velocity.0.y.to_bits()).pow(2);
                setup.tick(speed * 64 > i128::from(stats.max_speed.to_bits()).pow(2))
            });
            if let Some(weapon) = discharge.as_mut() {
                weapon.ticks_until_ready = weapon.ticks_until_ready.saturating_sub(1);
            }
            if !combat.enabled || combat.range == Fp::ZERO {
                continue;
            }
            // Stunned, or held in a tractor beam, it cannot fire.
            if !disabled.is_empty() && disabled.binary_search_by_key(&id, |unit| unit.0).is_ok() {
                continue;
            }
            let stun = stun_weapon.map_or(0, |weapon| weapon.0);
            // Whether the main weapon fires this tick, which sets off its broadside.
            let mut main_fired = false;
            let me = grid.entry(index);
            let range = i64::from(combat.range.to_bits());
            let squared_distance = |x: i32, y: i32| {
                let (dx, dy) = (i64::from(x - me.x), i64::from(y - me.y));
                dx * dx + dy * dy
            };
            // Guns sit on top of a unit, not in its middle, which lets a ship fire over a bank and
            // a tank over a low rise.
            let muzzle = me.z.saturating_add(me.radius);
            // A ship, a surfaced submarine, or a launcher lets its torpedoes go just under the water,
            // as a submarine does, not from its guns' height: from the deck they flew to the water
            // as shells, and were drawn so (2026-11-25). Torpedo bombers drop theirs from the air.
            let water_launch = (terrain.water_level() - fp_int(TORPEDO_LAUNCH_DEPTH_UNITS)).to_bits();
            let launch_height = |muzzle: i32, torpedo: bool| if torpedo && !me.is_air { muzzle.min(water_launch) } else { muzzle };
            // How far off a target is for its weapons' reach: across the ground, and for an
            // aircraft in flight, up to it as well, so that what flies higher is out of reach sooner.
            let reach_distance = |entry: &super::spatial::GridEntry| {
                let up = if entry.is_air { i64::from(entry.z.saturating_sub(muzzle)) } else { 0 };
                squared_distance(entry.x, entry.y) + up * up
            };
            // Range reaches the edge of a target, so that large structures can be hit from their range.
            // A cloaked enemy it cannot see is no target, even in plain sight.
            let hidden = |id: SimId| concealed[stats.team.index()].binary_search(&id).is_ok();
            // Only anti-air weapons hit aircraft, and most of them hit nothing else.
            // A torpedo hits ships and submarines, and nothing else.
            let torpedo = torpedo.is_some();
            // Under the water, only torpedoes and missiles can be fired.
            let can_fire = !me.is_sub || torpedo || launcher.is_some();
            let hits_air = anti_air.is_some() && !torpedo;
            let flak = anti_air.is_some_and(|anti_air| anti_air.flak);
            let guided = anti_air.is_some_and(|anti_air| anti_air.homing);
            let hits_ground = anti_air.is_none_or(|anti_air| anti_air.also_ground) && !torpedo;
            let can_hit = |entry: &super::spatial::GridEntry| {
                // A tractor beam lifts only what moves on land.
                if tractor.is_some() {
                    return !entry.is_structure && !entry.is_air && !entry.is_sub && !entry.is_naval;
                }
                if entry.is_sub {
                    torpedo
                } else if entry.is_air {
                    hits_air
                } else {
                    hits_ground || (torpedo && entry.is_naval)
                }
            };
            // Artillery cannot hit what is closer than its least range.
            let least = artillery.map_or(0, |artillery| i64::from(artillery.min_range.to_bits()));
            let far_enough = |distance: i64| distance >= least * least;
            // What its team sees, or has on radar, deep scan, or sonar: any of it can be fired at,
            // as the team knows where it is and what it is. A bomber lets its bombs go far short of
            // its target, beyond its own sight, so it needs this most.
            let sensed = |entry: &super::spatial::GridEntry| {
                !fog || visibility.is_visible(stats.team, entry.x, entry.y) || detected[stats.team.index()].binary_search(&entry.id).is_ok()
                    // Structures it has found stay where they were, so they can be fired at
                    // unseen. Only standing ones are in the grid to be picked.
                    || (entry.is_structure && known[stats.team.index()].binary_search_by_key(&entry.id, |record| record.id).is_ok())
            };
            let can_engage = |entry: &super::spatial::GridEntry| {
                let reach = range + i64::from(entry.radius);
                reach_distance(entry) <= reach * reach
                    && far_enough(squared_distance(entry.x, entry.y))
                    && can_hit(entry)
                    && sensed(entry)
                    && !hidden(entry.id)
            };
            let locate = |target: SimId| units.binary_search_by_key(&target, |unit| unit.0).ok().map(|i| grid.entry(i));
            // A missile launcher picks no targets itself, and fires only the missiles it has built.
            let picks_targets = launcher.as_ref().is_none_or(|launcher| launcher.automatic);
            let missile = launcher.as_ref().map(|launcher| MissileShot {
                cruise_height: launcher.cruise_height,
                strategic: launcher.strategic,
                inner_radius: launcher.inner_radius,
                inner_damage: launcher.inner_damage,
            });
            // A tractor beam holds one unit at a time.
            let loaded = launcher.as_ref().is_none_or(|launcher| launcher.automatic || launcher.stock > 0) && tractor.is_none_or(|beam| beam.holding.is_none());

            // A ground attack takes over the gun, so the unit looks for nothing else.
            let ground = combat.ground_target;
            if ground.is_none() && combat.forced_target.is_none() {
                (combat.line_blocked, combat.closing) = (false, None);
            }
            if ground.is_some() {
                combat.target = None;
            } else if let Some(forced) = combat.forced_target {
                match locate(forced) {
                    Some(entry) => {
                        combat.target = can_engage(entry).then_some(forced);
                        if combat.target.is_none() {
                            (combat.line_blocked, combat.closing) = (false, None);
                        }
                    }
                    None => {
                        (combat.forced_target, combat.chasing) = (None, false);
                        combat.target = None;
                        finished_attacks.push(id);
                    }
                }
            } else {
                if combat.blocked.is_some_and(|(_, until)| tick >= until) {
                    combat.blocked = None;
                }
                let blocked = combat.blocked.map(|(target, _)| target);
                // Drop a target that has died, moved out of range, or gone out of sight. On an
                // attack-move, a unit keeps its target until it dies, out of range or sight, as if
                // told to attack it, and goes after it. So does a bomber on any order: each pass
                // carries it past, out of range and out of its own sight, and dropping the target
                // then sent the next bomb at whatever was nearest. Only a cloak, or a change that
                // puts the target beyond its weapon, breaks it off.
                // Movement does the chasing. Builders are left out, since their walks are among
                // their tasks.
                let sticky = bomb_bay.is_some() || (order.kind == OrderKind::AttackMove && builder.is_none());
                if let Some(target) = combat.target {
                    // Not one too close for artillery, which could neither fire nor chase it.
                    let keep = locate(target).is_some_and(|entry| {
                        can_engage(entry) || (sticky && can_hit(entry) && !hidden(entry.id) && far_enough(squared_distance(entry.x, entry.y)))
                    });
                    // Nor one it cannot fire at now: a commander walking the seabed on an
                    // attack-move stopped for a target it could never shoot, and stood there
                    // under the water for the rest of the match.
                    if !keep || !can_fire {
                        combat.target = None;
                    }
                }

                // Searches are spread across ticks. Rings grow outward, so units at the
                // front of a battle stop early with the nearest enemy. Mobile units come first.
                if combat.target.is_none() && picks_targets && can_fire && stats.phase.wrapping_add(tick).is_multiple_of(TARGET_SEARCH_INTERVAL) {
                    // A missile flies to where its target was, so a launcher picking its own looks over
                    // its whole reach at once, for structures first, then what has stopped, and only
                    // then what is moving, which it would miss.
                    let mut ring = if missile.is_some() { range } else { (8i64 << FP_FRAC_BITS).max(range / 8) };
                    loop {
                        let radius = ring.min(range);
                        let mut best: Option<(u8, i64, SimId)> = None;
                        let mut consider = |other: &super::spatial::GridEntry| {
                            let distance = reach_distance(other);
                            let reach = radius + i64::from(other.radius);
                            let visible = sensed(other) && !hidden(other.id);
                            if distance <= reach * reach && far_enough(distance) && visible && blocked != Some(other.id) && can_hit(other) {
                                let rank = if missile.is_none() {
                                    u8::from(other.is_structure)
                                } else if other.is_structure {
                                    0
                                } else {
                                    let (vx, vy) = (i64::from(other.velocity[0]), i64::from(other.velocity[1]));
                                    1 + u8::from(vx * vx + vy * vy > MISSILE_STILL_SPEED_RAW * MISSILE_STILL_SPEED_RAW)
                                };
                                let candidate = (rank, distance, other.id);
                                if best.is_none_or(|current| candidate < current) {
                                    best = Some(candidate);
                                }
                            }
                        };
                        let enemy = Some(stats.team.enemy());
                        if hits_air {
                            grid.for_each_candidate(enemy, Layer::Air, me.x, me.y, radius as i32 + max_mobile, &mut consider);
                        }
                        if hits_ground || torpedo {
                            grid.for_each_candidate(enemy, Layer::Mobile, me.x, me.y, radius as i32 + max_mobile, &mut consider);
                        }
                        if hits_ground && max_structure > 0 {
                            grid.for_each_candidate(enemy, Layer::Structure, me.x, me.y, radius as i32 + max_structure, &mut consider);
                        }
                        if torpedo {
                            grid.for_each_candidate(enemy, Layer::Sub, me.x, me.y, radius as i32 + max_mobile, &mut consider);
                        }
                        if let Some((_, _, found)) = best {
                            combat.target = Some(found);
                            break;
                        }
                        if radius >= range {
                            break;
                        }
                        ring *= 2;
                    }
                }
            }

            // A Discharge order takes over the gun, until it fires or its target is gone.
            let discharge_target = discharge_order.map(|order| order.target);
            if let Some(target) = discharge_target {
                if locate(target).is_none() {
                    finished_discharges.push(id);
                }
            }
            let aim_at = discharge_target.filter(|target| locate(*target).is_some()).or(combat.target);
            let aim_point = match ground {
                Some(spot) => Some((spot.x.to_bits(), spot.y.to_bits())),
                None => aim_at.and_then(locate).map(|entry| (entry.x, entry.y)),
            };

            // A gun on a turret aims and fires from where the turret sits on the hull.
            let front = turning.map(|turning| turning.facing);
            let gun = match (turret.as_deref(), front) {
                (Some(turret), Some(front)) => turret.position((me.x, me.y), front),
                _ => (me.x, me.y),
            };
            let turreted = turret.is_some();
            // A turret swivels to its target, or back to where it rests without one, and only fires once it points there.
            let aimed = match turret {
                // A gun fixed to the nose fires only at what is within its arc of straight ahead.
                None => nose_gun.zip(front).is_none_or(|(arc, front)| {
                    aim_point.is_some_and(|(x, y)| {
                        let away = Vec2Fp::new(Fp::from_bits(x - gun.0), Fp::from_bits(y - gun.1));
                        away != Vec2Fp::ZERO && {
                            let away = away.normalized();
                            front.x * away.x + front.y * away.y >= arc.0
                        }
                    })
                }),
                Some(turret) => {
                    // It lays its barrels along the shot it would fire: at where its target will
                    // be, on the arc the shell will fly, as `PendingShot` resolves it.
                    let raw = |x: i32, y: i32, z: i32| Vec3Fp::new(Fp::from_bits(x), Fp::from_bits(y), Fp::from_bits(z));
                    let from = raw(gun.0, gun.1, launch_height(muzzle, torpedo));
                    let lobbed = (artillery.is_some() || high_arc.is_some()) && missile.is_none();
                    let laid = match (ground, aim_at.and_then(locate)) {
                        (Some(spot), _) => Some(launch(from, Vec3Fp::new(spot.x, spot.y, terrain.height_at(spot) - GROUND_AIM_DEPTH), combat.projectile_speed, None, false, lobbed)),
                        (None, Some(entry)) => {
                            let to = raw(entry.x, entry.y, entry.z);
                            let discharging = discharge_target == Some(entry.id);
                            let fused = !discharging && entry.is_air && !guided && !torpedo;
                            let homing = !discharging && ((entry.is_air && !fused) || torpedo);
                            Some(if missile.is_some() || homing {
                                to - from
                            } else {
                                launch(from, to, combat.projectile_speed, Some(entry), fused, lobbed && !discharging)
                            })
                        }
                        (None, None) => None,
                    };
                    match laid.filter(|_| aim_point.is_some()) {
                        Some(velocity) => turret.lay(velocity),
                        None => {
                            if let Some(front) = front {
                                turret.stand_down(front);
                            }
                            false
                        }
                    }
                }
            };
            // A broadside goes off all at once: the main weapon waits until each of its guns that
            // has a target points at it (as they did last tick), so that the first volley is whole.
            let aimed = aimed && extras.as_deref().is_none_or(|extras| extras.0.iter().all(|weapon| !weapon.broadside || weapon.aimed || weapon.target.is_none()));
            // A gun fixed to the nose never lets a shot go by: ready, with its own target outside
            // its arc or none at all, it fires at the nearest it can hit inside the arc, and keeps
            // steering for its own (the design owner, 2026-11-26).
            let snap = nose_gun.zip(front).filter(|_| !aimed && !turreted && ground.is_none() && discharge_target.is_none() && can_fire && combat.ticks_until_ready == 0).and_then(|(arc, front)| {
                let mut best: Option<(i64, SimId)> = None;
                let mut consider = |other: &super::spatial::GridEntry| {
                    let away = Vec2Fp::new(Fp::from_bits(other.x - gun.0), Fp::from_bits(other.y - gun.1));
                    let ahead = away != Vec2Fp::ZERO && {
                        let away = away.normalized();
                        front.x * away.x + front.y * away.y >= arc.0
                    };
                    if ahead && can_engage(other) {
                        let candidate = (reach_distance(other), other.id);
                        if best.is_none_or(|current| candidate < current) {
                            best = Some(candidate);
                        }
                    }
                };
                let enemy = Some(stats.team.enemy());
                let reach = range as i32 + max_mobile;
                if hits_air {
                    grid.for_each_candidate(enemy, Layer::Air, me.x, me.y, reach, &mut consider);
                }
                if hits_ground {
                    grid.for_each_candidate(enemy, Layer::Mobile, me.x, me.y, reach, &mut consider);
                    if max_structure > 0 {
                        grid.for_each_candidate(enemy, Layer::Structure, me.x, me.y, range as i32 + max_structure, &mut consider);
                    }
                }
                best.map(|(_, found)| found)
            });
            let aimed = aimed || snap.is_some();
            // Discharge fires in range, once aimed and reloaded, if the team has the energy stored.
            if let (Some(target), Some(weapon), true) = (discharge_target, discharge.as_mut(), can_fire) {
                let economy = &mut economies[stats.owner.index()];
                if let Some(entry) = locate(target).filter(|entry| can_engage(entry)) {
                    // By the inspiration's rule, nothing is spent until the shell lands.
                    if aimed && weapon.ticks_until_ready == 0 && economy.energy >= weapon.def.energy_required {
                        let raw = |x: i32, y: i32, z: i32| Vec3Fp::new(Fp::from_bits(x), Fp::from_bits(y), Fp::from_bits(z));
                        shots.push(PendingShot {
                            source: id,
                            team: stats.team,
                            from: raw(gun.0, gun.1, muzzle),
                            to: raw(entry.x, entry.y, entry.z),
                            speed: combat.projectile_speed,
                            damage: weapon.def.max_damage,
                            splash: Fp::ZERO,
                            discharge: Some(weapon.def),
                            missile: None,
                            homing: None,
                            bomb: None,
                            spread: Fp::ZERO,
                            underwater: false,
                            stun: 0,
                            lead: Some(entry.entity),
                            fused: false,
                            flak: false,
                            lobbed: false,
                        });
                        weapon.ticks_until_ready = weapon.def.reload_ticks;
                        finished_discharges.push(id);
                    }
                }
            }
            // While aiming a Discharge elsewhere, the gun does not fire at anything else.
            let gun_free = discharge_target.is_none_or(|target| combat.target == Some(target));
            // A bomber's velocity, which its bombs keep as they fall.
            let speed = flight.map_or(stats.max_speed, |flight| flight.speed);
            let bomb = bomb_bay.and(turning).map(|turning| {
                let step = turning.facing.scaled(speed);
                Vec3Fp::new(step.x, step.y, Fp::ZERO)
            });
            // It bombs only from the height it is flying at, half its elevation out of fuel, not
            // while taking off or landing.
            let at_height = flight.is_none_or(|flight| flight.altitude >= flight.cruising_height());
            let raw = |x: i32, y: i32, z: i32| Vec3Fp::new(Fp::from_bits(x), Fp::from_bits(y), Fp::from_bits(z));
            // A bomber lets go only when its bomb would land on the target. It falls to the
            // ground, or over the sea to the water's surface, where it bursts.
            let splash = combat.projectile_splash;
            let surface = |at: Vec2Fp| terrain.height_at(at).max(terrain.water_level());
            let bomb_on = |to: Vec3Fp, radius: i32, velocity: [i32; 2]| {
                let reach = (splash + Fp::from_bits(radius)) / 2;
                at_height && turning.is_some_and(|turning| bomb_lands_on(raw(me.x, me.y, muzzle), turning.facing, speed, to, velocity, reach))
            };
            // Artillery waits for the energy a shot needs, and reloads faster beside generators.
            let weapon_energy = artillery.map_or(Res::ZERO, |artillery| {
                super::components::Adjacency::less(artillery.energy_per_shot, adjacency.map_or(0, |adjacency| adjacency.weapon_energy_discount))
            });
            let charged = economies[stats.owner.index()].energy >= weapon_energy;
            let reload = match (artillery, adjacency) {
                (Some(_), Some(adjacency)) => {
                    let ticks = i64::from(combat.reload_ticks) * (super::blueprints::ADJACENCY_SCALE - i64::from(adjacency.reload_discount)) / super::blueprints::ADJACENCY_SCALE;
                    ticks.max(1) as u32
                }
                _ => combat.reload_ticks,
            };
            let spread = artillery.map_or_else(|| scatter.map_or(Fp::ZERO, |scatter| scatter.0), |artillery| artillery.spread);
            if combat.ticks_until_ready == 0 && aimed && gun_free && loaded && charged && can_fire && set_up {
                if let Some(target) = snap.or(combat.target) {
                    let entry = locate(target).expect("target was just validated or found");
                    let (from, to) = (raw(gun.0, gun.1, launch_height(muzzle, torpedo)), raw(entry.x, entry.y, entry.z));
                    // Missiles at aircraft steer themselves to them; guns' and flak's shells fly
                    // straight to where the aircraft will be.
                    let fused = entry.is_air && !guided && !torpedo;
                    let homing = ((entry.is_air && !fused) || torpedo).then_some(target);
                    // Missiles, bombs and shots at aircraft are never blocked by the ground, and
                    // a high arc, as a battleship's, comes down on its aim from above.
                    let direct = missile.is_none() && homing.is_none() && bomb.is_none() && tractor.is_none() && !fused && high_arc.is_none();
                    let clear = !direct || shot_is_clear(terrain, from, to, combat.projectile_speed);
                    // A target the player ordered is walked closer to until the shot is clear, rather
                    // than shelling the ground in the way. Artillery lobs over most of it, and fires.
                    if combat.forced_target == Some(target) {
                        combat.line_blocked = !clear && artillery.is_none() && combat.still_closing([me.x, me.y], tick);
                        if clear {
                            combat.closing = None;
                        }
                    }
                    if bomb.is_some() && !bomb_on(Vec3Fp::new(to.x, to.y, surface(to.xy())), entry.radius, entry.velocity) {
                        // Not over it yet.
                    } else if !clear && combat.forced_target.is_none() {
                        // A target picked by the unit itself is passed over for a while.
                        combat.blocked = Some((target, tick + BLOCKED_TARGET_TICKS));
                        combat.target = None;
                    } else if combat.line_blocked {
                        // Walking closer: no shot yet.
                    } else if tractor.is_some() {
                        // It takes hold, once the cleanup has checked the unit can be lifted.
                        grabs.push((id, target));
                        combat.ticks_until_ready = reload;
                    } else {
                        shots.push(PendingShot {
                            source: id,
                            team: stats.team,
                            from,
                            to,
                            speed: combat.projectile_speed,
                            damage: combat.projectile_damage,
                            splash: combat.projectile_splash,
                            discharge: None,
                            missile,
                            homing,
                            bomb,
                            spread,
                            underwater: torpedo,
                            stun,
                            // Every shell leads, artillery's too. Bombs, shots that home, and cruise missiles, which fly to
                            // where the target was at launch, do not.
                            lead: (missile.is_none() && homing.is_none() && bomb.is_none()).then_some(entry.entity),
                            fused,
                            flak: fused && flak,
                            lobbed: (artillery.is_some() || high_arc.is_some()) && missile.is_none(),
                        });
                        economies[stats.owner.index()].energy -= weapon_energy;
                        combat.ticks_until_ready = salvo.as_mut().map_or(reload, |salvo| salvo.fire(reload));
                        main_fired = true;
                        if let Some(launcher) = launcher.as_mut() {
                            launcher.stock = launcher.stock.saturating_sub(1);
                            if launcher.strategic {
                                fired_nukes.push(id);
                            }
                        }
                    }
                }
            }
            // A ground attack aims a hair below the ground, so that rounding cannot leave the shell
            // just above it at the spot, to skim on past. Any deeper, and a flat shot lands short.
            if let Some(spot) = ground.filter(|_| hits_ground && tractor.is_none()) {
                let spot_point = Vec3Fp::new(spot.x, spot.y, if bomb.is_some() { surface(spot) } else { terrain.height_at(spot) });
                let distance = squared_distance(spot.x.to_bits(), spot.y.to_bits());
                let in_reach = if bomb.is_some() { bomb_on(spot_point, 0, [0, 0]) } else { distance <= range * range && far_enough(distance) };
                let charged = economies[stats.owner.index()].energy >= weapon_energy;
                if !in_reach {
                    (combat.line_blocked, combat.closing) = (false, None);
                }
                let from = raw(gun.0, gun.1, launch_height(muzzle, torpedo));
                let to = Vec3Fp::new(spot.x, spot.y, terrain.height_at(spot) - GROUND_AIM_DEPTH);
                // A shell that would come down on a rise in between, or short on a slope, is not
                // fired: the unit walks closer. Bombs, missiles and artillery go over.
                if combat.ticks_until_ready == 0 && aimed && loaded && in_reach && bomb.is_none() && missile.is_none() && artillery.is_none() && high_arc.is_none() {
                    let reach = combat.projectile_splash.max(fp_int(GROUND_SHOT_REACH_UNITS));
                    let lands = super::combat::shot_lands_near(terrain, from, to, combat.projectile_speed, reach);
                    combat.line_blocked = !lands && combat.still_closing([me.x, me.y], tick);
                    if lands {
                        combat.closing = None;
                    }
                }
                if combat.ticks_until_ready == 0 && aimed && loaded && in_reach && charged && can_fire && set_up && !combat.line_blocked {
                    shots.push(PendingShot {
                        source: id,
                        team: stats.team,
                        from,
                        to,
                        speed: combat.projectile_speed,
                        damage: combat.projectile_damage,
                        splash: combat.projectile_splash,
                        discharge: None,
                        missile,
                        homing: None,
                        bomb,
                        spread,
                        underwater: false,
                        stun,
                        lead: None,
                        fused: false,
                        flak: false,
                        lobbed: (artillery.is_some() || high_arc.is_some()) && missile.is_none(),
                    });
                    economies[stats.owner.index()].energy -= weapon_energy;
                    combat.ticks_until_ready = salvo.as_mut().map_or(reload, |salvo| salvo.fire(reload));
                    main_fired = true;
                    if let Some(launcher) = launcher.as_mut() {
                        launcher.stock = launcher.stock.saturating_sub(1);
                        if launcher.strategic {
                            fired_nukes.push(id);
                        }
                    }
                }
            }

            // Each extra weapon picks its own target: the ordered one if it can hit it, or else
            // the nearest it can, mobile units first. A ground attack takes over those that hit the ground.
            if let Some(extras) = extras {
                // A broadside's guns fire only with the main weapon: each time it fires, they are
                // due once more, in turn after it, a ripple apart, so that they land as one volley.
                let mut in_turn = 0;
                for weapon in &mut extras.0 {
                    if main_fired && weapon.broadside {
                        in_turn += 1;
                        weapon.due = true;
                        weapon.ticks_until_ready = in_turn * super::blueprints::BROADSIDE_RIPPLE_TICKS + 1;
                    }
                    weapon.ticks_until_ready = weapon.ticks_until_ready.saturating_sub(1);
                    if let Some(salvo) = weapon.salvo.as_mut() {
                        salvo.tick();
                    }
                    let reach = i64::from(weapon.range.to_bits());
                    let usable = |entry: &super::spatial::GridEntry| {
                        let within = reach + i64::from(entry.radius);
                        let hit = if entry.is_sub {
                            weapon.torpedo
                        } else if entry.is_air {
                            weapon.hits_air
                        } else {
                            weapon.hits_ground || (weapon.torpedo && entry.is_naval)
                        };
                        reach_distance(entry) <= within * within
                            && hit
                            && sensed(entry)
                            && !hidden(entry.id)
                    };
                    let spot = ground.filter(|spot| weapon.hits_ground && squared_distance(spot.x.to_bits(), spot.y.to_bits()) <= reach * reach);
                    let forced = combat.forced_target.and_then(locate).filter(|entry| usable(entry)).map(|entry| entry.id);
                    if spot.is_some() {
                        weapon.target = None;
                    } else if forced.is_some() {
                        weapon.target = forced;
                    } else if !weapon.target.and_then(locate).is_some_and(usable) {
                        weapon.target = None;
                        if stats.phase.wrapping_add(tick).is_multiple_of(TARGET_SEARCH_INTERVAL) {
                            let mut best: Option<(bool, i64, SimId)> = None;
                            let mut consider = |other: &super::spatial::GridEntry| {
                                if usable(other) {
                                    let candidate = (other.is_structure, squared_distance(other.x, other.y), other.id);
                                    if best.is_none_or(|current| candidate < current) {
                                        best = Some(candidate);
                                    }
                                }
                            };
                            let enemy = Some(stats.team.enemy());
                            if weapon.hits_air {
                                grid.for_each_candidate(enemy, Layer::Air, me.x, me.y, reach as i32 + max_mobile, &mut consider);
                            }
                            if weapon.hits_ground || weapon.torpedo {
                                grid.for_each_candidate(enemy, Layer::Mobile, me.x, me.y, reach as i32 + max_mobile, &mut consider);
                            }
                            if weapon.hits_ground && max_structure > 0 {
                                grid.for_each_candidate(enemy, Layer::Structure, me.x, me.y, reach as i32 + max_structure, &mut consider);
                            }
                            if weapon.torpedo {
                                grid.for_each_candidate(enemy, Layer::Sub, me.x, me.y, reach as i32 + max_mobile, &mut consider);
                            }
                            weapon.target = best.map(|(_, _, found)| found);
                        }
                    }
                    let target = weapon.target.and_then(locate);
                    let aim_at = match spot {
                        Some(spot) => Some((spot.x.to_bits(), spot.y.to_bits())),
                        None => target.map(|entry| (entry.x, entry.y)),
                    };
                    let gun = match (weapon.turret.as_ref(), front) {
                        (Some(turret), Some(front)) => turret.position((me.x, me.y), front),
                        _ => (me.x, me.y),
                    };
                    let aimed = match weapon.turret.as_mut() {
                        None => true,
                        Some(turret) => match aim_at {
                            Some(_) => {
                                // Its barrels follow the shot it would fire, as the main gun's do.
                                let from = raw(gun.0, gun.1, launch_height(muzzle, weapon.torpedo));
                                let velocity = match (spot, target) {
                                    (Some(spot), _) => launch(from, Vec3Fp::new(spot.x, spot.y, terrain.height_at(spot) - GROUND_AIM_DEPTH), weapon.projectile_speed, None, false, weapon.lobbed),
                                    (None, Some(entry)) => {
                                        let to = raw(entry.x, entry.y, entry.z);
                                        let fused = !weapon.homing && !weapon.torpedo && weapon.missile_height.is_none() && entry.is_air;
                                        let homing = (entry.is_air && weapon.homing) || weapon.torpedo;
                                        if weapon.missile_height.is_some() || homing {
                                            to - from
                                        } else {
                                            launch(from, to, weapon.projectile_speed, Some(entry), fused, weapon.lobbed && !fused)
                                        }
                                    }
                                    (None, None) => Vec3Fp::ZERO,
                                };
                                turret.lay(velocity)
                            }
                            None => {
                                if let Some(front) = front {
                                    turret.stand_down(front);
                                }
                                false
                            }
                        },
                    };
                    weapon.aimed = aimed;
                    if weapon.ticks_until_ready > 0 || !aimed || (weapon.broadside && !weapon.due) || (me.is_sub && !weapon.torpedo && weapon.missile_height.is_none()) {
                        continue;
                    }
                    let from = raw(gun.0, gun.1, launch_height(muzzle, weapon.torpedo));
                    let (to, homing) = match (spot, target) {
                        (Some(spot), _) => (Vec3Fp::new(spot.x, spot.y, terrain.height_at(spot) - GROUND_AIM_DEPTH), None),
                        (None, Some(entry)) => (raw(entry.x, entry.y, entry.z), ((entry.is_air && weapon.homing) || weapon.torpedo).then_some(entry.id)),
                        (None, None) => continue,
                    };
                    // Missiles fly over the ground. A shell the ground would stop is not fired, and
                    // the weapon looks again.
                    let missile = weapon.missile_height.map(|cruise_height| MissileShot { cruise_height, strategic: false, inner_radius: Fp::ZERO, inner_damage: Fp::ZERO });
                    let homing = if missile.is_some() { None } else { homing };
                    let fused = !weapon.homing && !weapon.torpedo && missile.is_none() && spot.is_none() && target.is_some_and(|entry| entry.is_air);
                    if missile.is_none() && homing.is_none() && spot.is_none() && !fused && !weapon.lobbed && !shot_is_clear(terrain, from, to, weapon.projectile_speed) {
                        weapon.target = None;
                        continue;
                    }
                    shots.push(PendingShot {
                        source: id,
                        team: stats.team,
                        from,
                        to,
                        speed: weapon.projectile_speed,
                        damage: weapon.damage,
                        splash: weapon.splash,
                        discharge: None,
                        missile,
                        homing,
                        bomb: None,
                        spread: weapon.spread,
                        underwater: weapon.torpedo,
                        stun: weapon.stun_ticks,
                        lead: match (spot, target) {
                            (None, Some(entry)) if missile.is_none() && homing.is_none() => Some(entry.entity),
                            _ => None,
                        },
                        fused,
                        flak: fused && weapon.flak,
                        lobbed: weapon.lobbed && missile.is_none() && homing.is_none() && !fused,
                    });
                    weapon.ticks_until_ready = weapon.fired();
                    weapon.due = false;
                }
            }
        }

        for id in finished_attacks {
            self.set_order(id, Order::IDLE);
        }
        // Each order to fire a nuclear missile fires one.
        for id in fired_nukes {
            let (_, entity) = self.find_unit(id).expect("it just fired");
            let mut combat = self.ecs.get::<&mut Combat>(entity).expect("units have combat");
            (combat.ground_target, combat.forced_target, combat.target) = (None, None, None);
            drop(combat);
            self.set_order(id, Order::IDLE);
        }
        for id in finished_discharges {
            let (_, entity) = self.find_unit(id).expect("it just fired");
            let _ = self.ecs.remove_one::<DischargeOrder>(entity);
            self.set_order(id, Order::IDLE);
        }
        let friendly_fire = self.config.friendly_fire;
        self.step_silo_launches();
        for shot in std::mem::take(&mut self.pending_shots) {
            // A nuclear missile waits for its silo's doors to open.
            if shot.missile.is_some_and(|missile| missile.strategic) {
                self.silo_launches.push(SiloLaunch { shot, ticks: 0 });
                continue;
            }
            if let Some(MissileShot { cruise_height, strategic, inner_radius, inner_damage }) = shot.missile {
                // Plenty of time for the climb, the cruise, and the dive, and it never hits a friend.
                let distance = sqrt_wide((shot.to - shot.from).xy().length_squared());
                let lifetime = (distance / shot.speed.max(Fp::ONE)).to_num::<u32>() * 2 + 100;
                let projectile = Projectile {
                    velocity: Vec3Fp::ZERO,
                    damage: shot.damage,
                    source: shot.source,
                    source_team: shot.team,
                    lifetime_ticks: lifetime,
                    splash_radius: shot.splash,
                    allied_safe_ticks: lifetime,
                };
                let id = self.spawn_projectile(shot.from, projectile);
                let entity = self.projectiles.last().filter(|last| last.0 == id).expect("just spawned").1;
                let missile = Missile { target: shot.to.xy(), speed: shot.speed, cruise_height, strategic, inner_radius, inner_damage };
                self.ecs.insert_one(entity, missile).expect("just spawned");
                self.missiles.push((id, entity));
                if shot.stun > 0 {
                    self.ecs.insert_one(entity, Stunning(shot.stun)).expect("just spawned");
                }
                continue;
            }
            if let Some(target) = shot.homing {
                let distance = sqrt_wide(shot.from.distance_squared(&shot.to));
                // A torpedo only keeps up with a ship sailing away while it gathers speed, so it is
                // given that time on top.
                let speeding_up = if shot.underwater { TORPEDO_SPEED_UP_TICKS as u32 } else { 0 };
                let lifetime = (distance / shot.speed.max(Fp::ONE)).to_num::<u32>() * 3 + 10 + speeding_up;
                let projectile = Projectile {
                    velocity: Vec3Fp::ZERO,
                    damage: shot.damage,
                    source: shot.source,
                    source_team: shot.team,
                    lifetime_ticks: lifetime,
                    splash_radius: shot.splash,
                    allied_safe_ticks: 0,
                };
                let id = self.spawn_projectile(shot.from, projectile);
                let entity = self.projectiles.last().filter(|last| last.0 == id).expect("just spawned").1;
                let speed = if shot.underwater { shot.speed / fp_int(TORPEDO_LAUNCH_SHARE) } else { shot.speed };
                self.ecs.insert_one(entity, Homing { target, speed, top_speed: shot.speed, underwater: shot.underwater, lure: None }).expect("just spawned");
                if shot.stun > 0 {
                    self.ecs.insert_one(entity, Stunning(shot.stun)).expect("just spawned");
                }
                continue;
            }
            if let Some(velocity) = shot.bomb {
                let projectile = Projectile {
                    velocity,
                    damage: shot.damage,
                    source: shot.source,
                    source_team: shot.team,
                    lifetime_ticks: BOMB_LIFETIME_TICKS,
                    splash_radius: shot.splash,
                    allied_safe_ticks: if friendly_fire { BOMB_LIFETIME_TICKS } else { 0 },
                };
                let id = self.spawn_projectile(shot.from, projectile);
                let entity = self.projectiles.last().filter(|last| last.0 == id).expect("just spawned").1;
                self.ecs.insert_one(entity, Falling).expect("just spawned");
                if shot.stun > 0 {
                    self.ecs.insert_one(entity, Stunning(shot.stun)).expect("just spawned");
                }
                continue;
            }
            // A beam crosses to where it was aimed within this tick, in a straight line, and is
            // spent: the projectile step takes it there, and it strikes the first enemy, dome, or
            // rise in the ground on the way. It never hits a friend.
            if super::combat::is_beam(shot.speed) {
                let projectile = Projectile {
                    velocity: shot.to - shot.from,
                    damage: shot.damage,
                    source: shot.source,
                    source_team: shot.team,
                    lifetime_ticks: 1,
                    splash_radius: Fp::ZERO,
                    allied_safe_ticks: 1,
                };
                let id = self.spawn_projectile(shot.from, projectile);
                let entity = self.projectiles.last().filter(|last| last.0 == id).expect("just spawned").1;
                self.ecs.insert_one(entity, super::components::Beam { from: shot.from }).expect("just spawned");
                continue;
            }
            // A shell aims where its target will be when it arrives.
            let target_velocity = shot.lead.and_then(|target| self.ecs.get::<&Velocity>(target).ok().map(|velocity| velocity.0));
            // Flak flies straight, in three dimensions, to where the aircraft meets it.
            // Guns and flak aim along the aircraft's course and its steady turn.
            let turn = shot.lead.and_then(|target| self.ecs.get::<&super::components::Flight>(target).ok().map(|flight| flight.turn)).unwrap_or(Vec2Fp::new(Fp::ONE, Fp::ZERO));
            let terrain = &self.terrain;
            let super::combat::Aim { to: aim, least_ticks, flak_ticks } =
                super::combat::lead_aim(shot.from, shot.to, shot.speed, shot.fused, target_velocity, turn, |at| terrain.height_at(at).max(terrain.water_level()));
            // Flak and anti-air guns scatter the more, the faster the aircraft and the longer the
            // shell is in the air: the guess at where it will be is only as good as its course is
            // steady.
            let spread = match (flak_ticks, target_velocity) {
                (Some(ticks), Some(velocity)) => {
                    let speed = sqrt_wide(velocity.xy().length_squared());
                    shot.spread + speed * i32::try_from(ticks).unwrap_or(i32::MAX) / if shot.flak { FLAK_LEAD_ERROR } else { GUN_LEAD_ERROR }
                }
                _ => shot.spread,
            };
            // A shell with a spread, artillery's or a scattering gun's, lands somewhere within it of
            // where it was aimed.
            let to = if spread > Fp::ZERO {
                let stream = &mut self.scatter_rngs[shot.team.index()];
                loop {
                    let (x, y) = (stream.range_fp(-spread, spread), stream.range_fp(-spread, spread));
                    if wide_mul(x, x) + wide_mul(y, y) <= wide_mul(spread, spread) {
                        let (x, y) = if shot.team == Team::Red { (-x, -y) } else { (x, y) };
                        break Vec3Fp::new(aim.x + x, aim.y + y, aim.z);
                    }
                }
            } else {
                aim
            };
            // Flak flies straight to where it was aimed, and bursts there.
            if let Some(ticks) = flak_ticks {
                let offset = to - shot.from;
                let ticks = i32::try_from(ticks).unwrap_or(1).max(1);
                let projectile = Projectile {
                    velocity: Vec3Fp::new(offset.x / ticks, offset.y / ticks, offset.z / ticks),
                    damage: shot.damage,
                    source: shot.source,
                    source_team: shot.team,
                    lifetime_ticks: ticks as u32,
                    splash_radius: shot.splash,
                    allied_safe_ticks: 0,
                };
                let id = self.spawn_projectile(shot.from, projectile);
                let entity = self.projectiles.last().filter(|last| last.0 == id).expect("just spawned").1;
                self.ecs.insert_one(entity, super::components::Fused).expect("just spawned");
                continue;
            }
            // An artillery shell takes as long as a flat one would, so it leads the same, but
            // falls under a gravity of its own that sends it up in a high arc and down on its aim.
            let (solution, lobbed) = if shot.lobbed {
                let (solution, gravity) = super::combat::lobbed_solution(shot.from, to, shot.speed, least_ticks);
                (solution, Some(gravity))
            } else {
                (firing_solution_within(shot.from, to, shot.speed, least_ticks), None)
            };
            let projectile = self.spawn_projectile(
                shot.from,
                Projectile {
                    velocity: solution.velocity,
                    damage: shot.damage,
                    source: shot.source,
                    source_team: shot.team,
                    // Long enough to fly past a dodging target and reach the ground.
                    lifetime_ticks: solution.flight_ticks + solution.flight_ticks / 2 + 10,
                    splash_radius: shot.splash,
                    allied_safe_ticks: if friendly_fire { solution.flight_ticks } else { 0 },
                },
            );
            if let Some(def) = shot.discharge {
                let entity = self.projectiles.last().filter(|last| last.0 == projectile).expect("just spawned").1;
                self.ecs.insert_one(entity, Discharged { def }).expect("just spawned");
            }
            if let Some(gravity) = lobbed {
                let entity = self.projectiles.last().filter(|last| last.0 == projectile).expect("just spawned").1;
                self.ecs.insert_one(entity, super::components::Lobbed(gravity)).expect("just spawned");
            }
            if shot.stun > 0 {
                let entity = self.projectiles.last().filter(|last| last.0 == projectile).expect("just spawned").1;
                self.ecs.insert_one(entity, Stunning(shot.stun)).expect("just spawned");
            }
        }
    }

    /// The shields that are standing: finished, switched on, and not recharging.
    fn standing_domes(&self) -> Vec<Dome> {
        self.shields
            .iter()
            .filter_map(|&(id, entity)| {
                let shield = self.ecs.get::<&Shield>(entity).ok()?;
                if !shield.is_up() || self.ecs.get::<&Construction>(entity).is_ok() || self.ecs.get::<&Paused>(entity).is_ok() {
                    return None;
                }
                // A stunned generator's dome is down.
                if self.is_disabled(id) {
                    return None;
                }
                let base = self.ecs.get::<&Position>(entity).ok()?.0.xy();
                let stats = *self.ecs.get::<&UnitStats>(entity).ok()?;
                let team = stats.team;
                // A ship's dome sits on the water.
                let floor = if stats.move_class == MoveClass::Naval { self.terrain.height_at(base).max(self.terrain.water_level()) } else { self.terrain.height_at(base) };
                let center = [base.x.to_bits(), base.y.to_bits(), floor.to_bits()];
                Some(Dome { id, team, center, radius: shield.radius.to_bits(), height: shield.height.to_bits() })
            })
            .collect()
    }

    /// Steps the nuclear launches under way: once a silo's doors are open, its missile rises out,
    /// if the silo still stands; the launch is over once they have closed again.
    fn step_silo_launches(&mut self) {
        if self.silo_launches.is_empty() {
            return;
        }
        let mut leaving = Vec::new();
        self.silo_launches.retain_mut(|launch| {
            launch.ticks += 1;
            if launch.ticks == SILO_DOOR_TICKS {
                leaving.push(launch.shot);
            }
            launch.ticks < SILO_SEQUENCE_TICKS
        });
        for shot in leaving {
            if self.find_unit(shot.source).is_some() {
                self.launch_nuclear_missile(shot);
            }
        }
    }

    /// A nuclear missile leaves its silo, from inside it, on its flight to the ground at its
    /// target (`NuclearFlight`).
    fn launch_nuclear_missile(&mut self, shot: PendingShot) {
        let missile = shot.missile.expect("a nuclear missile");
        let ground = |at: Vec2Fp| self.terrain.height_at(at);
        // From the floor of the silo, or a submarine's hatch under the water.
        let floor = shot.from.z.min(ground(shot.from.xy()).max(self.terrain.water_level()));
        let start = Vec3Fp::new(shot.from.x, shot.from.y, floor);
        let target = Vec3Fp::new(shot.to.x, shot.to.y, ground(shot.to.xy()));
        let flight = super::components::NuclearFlight::new(start, target, missile.cruise_height + target.z.max(start.z), shot.speed);
        let lift = super::components::NUCLEAR_LIFT_UNITS as u32 / super::components::NUCLEAR_LIFT_SPEED.to_num::<u32>();
        let lifetime = (flight.length() / shot.speed.max(Fp::ONE)).to_num::<u32>() + lift + 20;
        let projectile = Projectile {
            velocity: Vec3Fp::ZERO,
            damage: shot.damage,
            source: shot.source,
            source_team: shot.team,
            lifetime_ticks: lifetime,
            splash_radius: shot.splash,
            allied_safe_ticks: lifetime,
        };
        let id = self.spawn_projectile(start, projectile);
        let entity = self.projectiles.last().filter(|last| last.0 == id).expect("just spawned").1;
        let marker = Missile { target: target.xy(), speed: shot.speed, cruise_height: missile.cruise_height, strategic: true, inner_radius: missile.inner_radius, inner_damage: missile.inner_damage };
        self.ecs.insert(entity, (marker, flight)).expect("just spawned");
        self.missiles.push((id, entity));
    }

    /// How far open each silo's doors are, by launcher, in thousandths, while a launch is under way.
    pub fn silo_doors(&self) -> Vec<(SimId, u16)> {
        self.silo_launches.iter().map(|launch| (launch.shot.source, (silo_doors_open(launch.ticks) * 1_000 / SILO_DOOR_TICKS) as u16)).collect()
    }

    /// Missile defenses that are ready shoot down enemy missiles in range, before anything
    /// moves. Each mount of a tactical defense is a defense of its own: one that is ready
    /// destroys the nearest cruise missile, then reloads. A strategic one spends an interceptor
    /// on a nuclear missile that is in its range. Defenses and their mounts go in `SimId` order,
    /// and each takes only a missile no other has taken: no missile is ever aimed at twice, and
    /// every one aimed at is destroyed, at once.
    /// Returns the missiles destroyed, sorted by `SimId`.
    /// Anti-torpedo charges that are ready each drop a decoy into the water for the nearest enemy
    /// torpedo within range, in `SimId` order, and no torpedo twice: it turns to the decoy and
    /// strikes it instead of its target. They once destroyed the torpedo outright, drawn as a shot
    /// over the water (2026-11-25, the user's call). A charge still stops one torpedo each time it
    /// reloads. Returns the torpedoes destroyed outright, which is none now.
    fn intercept_torpedoes(&mut self) -> Vec<SimId> {
        // The missile defenses' step clears the last tick's interceptions, if there are any.
        if self.missile_defenses.is_empty() && self.interceptors.is_empty() {
            self.interceptions.clear();
        }
        let tick = self.current_tick;
        self.decoys.retain(|&(_, sinks)| sinks > tick);
        let mut lured: Vec<(SimId, hecs::Entity, Vec3Fp)> = Vec::new();
        for index in 0..self.torpedo_defenses.len() {
            let entity = self.torpedo_defenses[index].1;
            let mut defense = *self.ecs.get::<&super::components::TorpedoDefense>(entity).expect("defense list only holds defenses");
            defense.ticks_until_ready = defense.ticks_until_ready.saturating_sub(1);
            if defense.ticks_until_ready == 0 {
                let here = self.ecs.get::<&Position>(entity).expect("units have positions").0;
                let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
                let range = i64::from(defense.range.to_bits());
                let mut best: Option<(i64, SimId, hecs::Entity, Vec3Fp)> = None;
                for &(id, torpedo) in &self.projectiles {
                    // A torpedo already turned to a decoy is spent for this.
                    let free = self.ecs.get::<&Homing>(torpedo).is_ok_and(|homing| homing.underwater && homing.lure.is_none());
                    let enemy = self.ecs.get::<&Projectile>(torpedo).is_ok_and(|projectile| projectile.source_team != team);
                    if !free || !enemy || lured.iter().any(|&(other, _, _)| other == id) {
                        continue;
                    }
                    let at = self.ecs.get::<&Position>(torpedo).expect("projectiles have positions").0;
                    let (dx, dy) = (i64::from(at.x.to_bits() - here.x.to_bits()), i64::from(at.y.to_bits() - here.y.to_bits()));
                    let distance = dx * dx + dy * dy;
                    if distance <= range * range && best.is_none_or(|(closest, ..)| distance < closest) {
                        best = Some((distance, id, torpedo, at));
                    }
                }
                if let Some((distance, id, torpedo, at)) = best {
                    // In the water toward the torpedo, a little off the ship's side, or halfway to
                    // it if it is closer than that.
                    let toward = at.xy() - here.xy();
                    let out = fp_int(DECOY_OUT_UNITS).min(Fp::from_bits((distance.isqrt() / 2) as i32));
                    let spot = here.xy() + toward.normalized().scaled(out);
                    let decoy = Vec3Fp::new(spot.x, spot.y, self.terrain.water_level() - fp_int(DECOY_DEPTH_UNITS));
                    lured.push((id, torpedo, decoy));
                    self.decoys.push(([decoy.x.to_bits(), decoy.y.to_bits(), decoy.z.to_bits()], tick + DECOY_TICKS));
                    defense.ticks_until_ready = defense.reload_ticks;
                }
            }
            *self.ecs.get::<&mut super::components::TorpedoDefense>(entity).expect("defense list only holds defenses") = defense;
        }
        for (_, torpedo, decoy) in lured {
            if let Ok(mut homing) = self.ecs.get::<&mut Homing>(torpedo) {
                homing.lure = Some(decoy);
            }
        }
        Vec::new()
    }

    fn intercept_missiles(&mut self) -> Vec<SimId> {
        self.interceptions.clear();
        let mut destroyed: Vec<SimId> = Vec::new();
        // Interceptors that reach their missile this tick destroy it, with a flash where they meet.
        let tick = self.current_tick;
        let mut arrived = Vec::new();
        self.interceptors.retain(|interceptor| {
            let here = interceptor.arrives <= tick;
            if here {
                arrived.push(*interceptor);
            }
            !here
        });
        for interceptor in arrived {
            if self.missiles.iter().any(|&(id, _)| id == interceptor.target) && !destroyed.contains(&interceptor.target) {
                destroyed.push(interceptor.target);
            }
            let meet = [interceptor.meet.x.to_bits(), interceptor.meet.y.to_bits(), interceptor.meet.z.to_bits()];
            self.interceptions.push([meet, meet]);
        }
        for index in 0..self.missile_defenses.len() {
            let entity = self.missile_defenses[index].1;
            let mut defense = *self.ecs.get::<&MissileDefense>(entity).expect("defense list only holds defenses");
            let mounts = usize::from(defense.mounts.max(1));
            for timer in &mut defense.ticks_until_ready[..mounts] {
                *timer = timer.saturating_sub(1);
            }
            let working = self.ecs.get::<&Construction>(entity).is_err();
            // Each mount is a defense of its own: each that is ready takes the nearest missile
            // that no defense has taken yet this tick. A strategic defense has one, which fires
            // while it has interceptors.
            for mount in 0..mounts {
                let ready = if defense.strategic { defense.stock > 0 } else { defense.ticks_until_ready[mount] == 0 };
                if !working || !ready || self.missiles.is_empty() {
                    continue;
                }
                let here = self.ecs.get::<&Position>(entity).expect("units have positions").0;
                let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
                let range = i128::from(defense.range.to_bits());
                let within = |dx: i32, dy: i32, dz: i32| {
                    let (dx, dy, dz) = (i128::from(dx), i128::from(dy), i128::from(dz));
                    dx * dx + dy * dy + dz * dz <= range * range
                };
                let mut best: Option<(i128, SimId, Vec3Fp)> = None;
                for &(id, missile_entity) in &self.missiles {
                    let (Ok(missile), Ok(projectile), Ok(position)) =
                        (self.ecs.get::<&Missile>(missile_entity), self.ecs.get::<&Projectile>(missile_entity), self.ecs.get::<&Position>(missile_entity))
                    else {
                        continue;
                    };
                    let at = position.0;
                    let chased = self.interceptors.iter().any(|interceptor| interceptor.target == id);
                    if missile.strategic != defense.strategic || projectile.source_team == team || destroyed.contains(&id) || chased {
                        continue;
                    }
                    let (dx, dy, dz) = (at.x.to_bits() - here.x.to_bits(), at.y.to_bits() - here.y.to_bits(), at.z.to_bits() - here.z.to_bits());
                    // A nuclear missile flies high, so only how far it is across the ground counts.
                    let in_reach = if defense.strategic { within(dx, dy, 0) } else { within(dx, dy, dz) };
                    let distance = i128::from(dx) * i128::from(dx) + i128::from(dy) * i128::from(dy) + i128::from(dz) * i128::from(dz);
                    if in_reach && best.is_none_or(|(closest, _, _)| distance < closest) {
                        best = Some((distance, id, at));
                    }
                }
                let Some((_, id, at)) = best else {
                    // Nothing in reach for this mount, so nothing for the others either.
                    break;
                };
                if defense.strategic {
                    defense.stock -= 1;
                    // It fires an interceptor, to meet the missile further along its path before
                    // it strikes; one that would strike this very tick is destroyed at once.
                    let flight = self.missiles.iter().find(|missile| missile.0 == id).and_then(|missile| self.ecs.get::<&super::components::NuclearFlight>(missile.1).ok().map(|flight| *flight));
                    let meeting = flight.and_then(|mut flight| {
                        let mut moves_left = 0u32;
                        let mut ahead = flight;
                        while ahead.travelled < ahead.length() {
                            ahead.travelled += ahead.step();
                            moves_left += 1;
                        }
                        let ticks = INTERCEPTOR_FLIGHT_TICKS.min(moves_left.saturating_sub(1));
                        for _ in 0..ticks {
                            flight.travelled += flight.step();
                        }
                        (ticks > 0).then(|| (ticks, flight.position_at(flight.travelled)))
                    });
                    match meeting {
                        Some((ticks, meet)) => {
                            let from = Vec3Fp::new(here.x, here.y, here.z + fp_int(INTERCEPTOR_LAUNCH_HEIGHT_UNITS));
                            let source = self.missile_defenses[index].0;
                            let interceptor = Interceptor { id: self.next_id(), team, source, from, meet, target: id, launched: tick, arrives: tick + ticks };
                            self.interceptors.push(interceptor);
                        }
                        None => {
                            destroyed.push(id);
                            self.interceptions.push([[here.x.to_bits(), here.y.to_bits(), here.z.to_bits()], [at.x.to_bits(), at.y.to_bits(), at.z.to_bits()]]);
                        }
                    }
                    break;
                }
                destroyed.push(id);
                self.interceptions.push([[here.x.to_bits(), here.y.to_bits(), here.z.to_bits()], [at.x.to_bits(), at.y.to_bits(), at.z.to_bits()]]);
                defense.ticks_until_ready[mount] = defense.reload_ticks;
            }
            *self.ecs.get::<&mut MissileDefense>(entity).expect("defense list only holds defenses") = defense;
        }
        destroyed.sort_unstable();
        destroyed
    }

    pub fn step_projectiles(&mut self) {
        self.landed.clear();
        self.pending_shield_hits.clear();
        self.dead_projectiles.clear();
        // Interceptors in flight still arrive when their defense is gone.
        let mut shot_down = if self.missile_defenses.is_empty() && self.interceptors.is_empty() { Vec::new() } else { self.intercept_missiles() };
        if !self.torpedo_defenses.is_empty() {
            shot_down.extend(self.intercept_torpedoes());
            shot_down.sort_unstable();
            shot_down.dedup();
        }
        self.shot_down.clone_from(&shot_down);
        let domes = if self.shields.is_empty() { Vec::new() } else { self.standing_domes() };
        let shield_hits = &mut self.pending_shield_hits;
        let discharges = &mut self.pending_discharges;
        let terrain = &self.terrain;
        let grid = &self.spatial_grid;
        let hits = &mut self.pending_hits;
        let explosions = &mut self.pending_explosions;
        let wreck_blasts = &mut self.pending_wreck_blasts;
        let dead = &mut self.dead_projectiles;
        let friendly_fire = self.config.friendly_fire;
        let (max_mobile, max_structure) = (self.max_mobile_radius, self.max_structure_radius);
        hits.clear();
        self.pending_stuns.clear();
        let units = &self.units;
        let stuns = &mut self.pending_stuns;
        let mut query = self.ecs.query_mut::<(&mut Position, &mut Projectile, Option<&Discharged>, Option<&Missile>, Option<&mut Homing>, Option<&Falling>, Option<&Stunning>, Option<&super::components::Fused>, (Option<&super::components::Lobbed>, Option<&mut super::components::NuclearFlight>))>();
        let mut view = query.view();
        for &(id, entity) in &self.projectiles {
            if shot_down.binary_search(&id).is_ok() {
                dead.push(id);
                continue;
            }
            let (position, projectile, discharged, missile, homing, falling, stunning, fused, (lobbed, nuclear)) = view.get_mut(entity).expect("projectile list only holds live projectiles");
            let stun = stunning.map_or(0, |stunning| stunning.0);
            let start = position.0;
            // A nuclear missile flies its own path, and nothing on the way stops it, shields and
            // units included. It bursts on the ground at its target.
            if let Some(flight) = nuclear {
                flight.travelled += flight.step();
                let end = flight.position_at(flight.travelled);
                projectile.velocity = end - start;
                position.0 = end;
                projectile.lifetime_ticks = projectile.lifetime_ticks.saturating_sub(1);
                if flight.travelled >= flight.length() {
                    position.0 = flight.target;
                    let blast = Explosion { x: flight.target.x.to_bits(), y: flight.target.y.to_bits(), radius: projectile.splash_radius, damage: projectile.damage, source: Some(projectile.source), stun };
                    explosions.push(blast);
                    if let Some(missile) = missile.filter(|missile| missile.inner_radius > Fp::ZERO) {
                        explosions.push(Explosion { radius: missile.inner_radius, damage: missile.inner_damage, stun: 0, ..blast });
                    }
                    dead.push(id);
                }
                continue;
            }
            // A shell at an aircraft that does not steer flies straight on to where it was aimed.
            // Flak bursts there, hitting every enemy aircraft within its blast; a gun's shell hits
            // the nearest aircraft it reaches there, if any.
            if fused.is_some() {
                position.0 = start + projectile.velocity;
                projectile.lifetime_ticks = projectile.lifetime_ticks.saturating_sub(1);
                if projectile.lifetime_ticks == 0 {
                    let at = position.0;
                    let (x, y, z) = (at.x.to_bits(), at.y.to_bits(), at.z.to_bits());
                    let reach = i64::from(projectile.splash_radius.to_bits());
                    // A gun fires a burst of rounds, which hits an aircraft a little off its aim.
                    let gun_reach = if reach > 0 { 0 } else { i64::from(fp_int(GUN_BURST_REACH).to_bits()) };
                    let (damage, source) = (projectile.damage, projectile.source);
                    let mut nearest: Option<(i64, hecs::Entity)> = None;
                    grid.for_each_candidate(Some(projectile.source_team.enemy()), Layer::Air, x, y, projectile.splash_radius.to_bits() + gun_reach as i32 + max_mobile, |other| {
                        if !other.is_air {
                            return;
                        }
                        let (dx, dy, dz) = (i64::from(other.x - x), i64::from(other.y - y), i64::from(other.z - z));
                        let within = reach + gun_reach + i64::from(other.radius);
                        let distance = dx * dx + dy * dy + dz * dz;
                        if distance > within * within {
                            return;
                        }
                        if reach > 0 {
                            hits.push((other.entity, damage, Some(source)));
                        } else if nearest.is_none_or(|(closest, _)| distance < closest) {
                            nearest = Some((distance, other.entity));
                        }
                    });
                    if let Some((_, entity)) = nearest {
                        hits.push((entity, damage, Some(source)));
                    }
                    dead.push(id);
                }
                continue;
            }
            // A shot at an aircraft flies to it and hits only it. With the aircraft gone, it is spent.
            if let Some(homing) = homing {
                projectile.lifetime_ticks = projectile.lifetime_ticks.saturating_sub(1);
                if homing.speed < homing.top_speed {
                    let gain = (homing.top_speed - homing.top_speed / fp_int(TORPEDO_LAUNCH_SHARE)) / fp_int(TORPEDO_SPEED_UP_TICKS);
                    homing.speed = (homing.speed + gain).min(homing.top_speed);
                }
                // A torpedo turned to a decoy runs to it and strikes it, and hits nothing.
                if let Some(decoy) = homing.lure {
                    let distance = sqrt_wide(start.distance_squared(&decoy));
                    if distance <= homing.speed {
                        position.0 = decoy;
                        dead.push(id);
                        continue;
                    }
                    projectile.velocity = (decoy - start).scaled(homing.speed / distance);
                    position.0 = start + projectile.velocity;
                    if projectile.lifetime_ticks == 0 {
                        dead.push(id);
                    }
                    continue;
                }
                let target = units.binary_search_by_key(&homing.target, |unit| unit.0).ok().map(|index| grid.entry(index));
                // Nor can it hit an aircraft that has landed.
                // A torpedo reaches only what is on or under the water.
                let Some(target) = target.filter(|target| if homing.underwater { target.is_sub || target.is_naval } else { target.is_air }) else {
                    dead.push(id);
                    continue;
                };
                let mut aim = Vec3Fp::new(Fp::from_bits(target.x), Fp::from_bits(target.y), Fp::from_bits(target.z));
                // A torpedo stays under the water: at a ship, it runs for the hull below the waterline.
                if homing.underwater {
                    aim.z = aim.z.min(terrain.water_level() - fp_int(TORPEDO_LAUNCH_DEPTH_UNITS));
                }
                let distance = sqrt_wide(start.distance_squared(&aim));
                if distance <= homing.speed + Fp::from_bits(target.radius) {
                    position.0 = aim;
                    if projectile.splash_radius > Fp::ZERO {
                        // Flak bursts, and hits every enemy aircraft around it. A torpedo with a
                        // blast hits every enemy ship and submarine around it, and nothing else.
                        let reach = i64::from(projectile.splash_radius.to_bits());
                        let (x, y) = (target.x, target.y);
                        let damage = projectile.damage;
                        let source = projectile.source;
                        let underwater = homing.underwater;
                        let mut burst = |other: &super::spatial::GridEntry| {
                            let (dx, dy) = (i64::from(other.x - x), i64::from(other.y - y));
                            let within = reach + i64::from(other.radius);
                            let reached = if underwater { other.is_sub || other.is_naval } else { other.is_air };
                            if reached && dx * dx + dy * dy <= within * within {
                                hits.push((other.entity, damage, Some(source)));
                            }
                        };
                        let search = projectile.splash_radius.to_bits() + max_mobile;
                        let enemy = Some(projectile.source_team.enemy());
                        if underwater {
                            grid.for_each_candidate(enemy, Layer::Mobile, x, y, search, &mut burst);
                            grid.for_each_candidate(enemy, Layer::Sub, x, y, search, &mut burst);
                        } else {
                            grid.for_each_candidate(enemy, Layer::Air, x, y, search, &mut burst);
                        }
                    } else {
                        hits.push((target.entity, projectile.damage, Some(projectile.source)));
                        if stun > 0 {
                            stuns.push((target.entity, stun));
                        }
                    }
                    dead.push(id);
                    continue;
                }
                let share = homing.speed / distance;
                let offset = aim - start;
                projectile.velocity = offset.scaled(share);
                position.0 = start + projectile.velocity;
                if projectile.lifetime_ticks == 0 {
                    dead.push(id);
                }
                continue;
            }
            // A missile steers itself, and a shell falls.
            if let Some(missile) = missile {
                projectile.velocity = missile_velocity(terrain, start, missile);
            }
            let step = projectile.velocity;
            let end = start + step;
            position.0 = end;
            if falling.is_some() {
                projectile.velocity.z -= BOMB_GRAVITY;
            } else if let Some(lobbed) = lobbed {
                projectile.velocity.z -= lobbed.0;
            } else if missile.is_none() {
                projectile.velocity.z -= GRAVITY;
            }
            let strategic = missile.is_some_and(|missile| missile.strategic);
            projectile.lifetime_ticks = projectile.lifetime_ticks.saturating_sub(1);
            let allies_are_safe = projectile.allied_safe_ticks > 0;
            projectile.allied_safe_ticks = projectile.allied_safe_ticks.saturating_sub(1);

            // A bomb bursts on the water's surface; a shell still goes in, to the sea floor.
            let probe = if falling.is_some() { probe_surface(terrain, start, step) } else { probe_terrain(terrain, start, step) };

            // Shells far above the ground cannot touch a unit, which spares most grid queries.
            // Over the sea, the ground that counts is the surface ships ride on, not the seabed.
            let lowest = start.z.min(end.z);
            let reach = Fp::from_bits(max_mobile.max(max_structure) * 2);
            let mut best: Option<(i32, SimId, hecs::Entity, bool)> = None;
            if lowest <= probe.highest_ground.max(terrain.water_level()) + reach {
                let half = (step.x.to_bits().abs() + step.y.to_bits().abs()) / 2;
                let center_x = start.x.to_bits() + step.x.to_bits() / 2;
                let center_y = start.y.to_bits() + step.y.to_bits() / 2;
                let team_filter = if friendly_fire && !allies_are_safe { None } else { Some(projectile.source_team.enemy()) };
                let source = projectile.source;
                let mut consider = |other: &super::spatial::GridEntry| {
                    if other.id == source {
                        return;
                    }
                    if let Some(along) = entry_hit(other, start, step) {
                        let candidate = (along, other.id, other.entity, other.is_structure);
                        if best.is_none_or(|current| (candidate.0, candidate.1) < (current.0, current.1)) {
                            best = Some(candidate);
                        }
                    }
                };
                grid.for_each_candidate(team_filter, Layer::Mobile, center_x, center_y, half + max_mobile, &mut consider);
                if max_structure > 0 {
                    grid.for_each_candidate(team_filter, Layer::Structure, center_x, center_y, half + max_structure, &mut consider);
                }
            }

            // An enemy dome stops a shell coming in from outside before it reaches anything
            // beneath. A splash shell also bleeds into every other enemy dome its blast reaches.
            let dome_hit = domes
                .iter()
                .enumerate()
                // A nuclear missile is not stopped by shields.
                .filter(|(_, dome)| dome.team != projectile.source_team && !strategic)
                .filter_map(|(index, dome)| dome_entry(start, step, dome.center, dome.radius, dome.height).map(|along| (along, index)))
                .min();
            let first = |along: i32| best.is_none_or(|unit| along <= unit.0) && probe.hit.is_none_or(|ground| along <= ground);
            // Parts of the step are divided out, rounding toward zero, rather than shifted, so that
            // a shot and its mirror image strike mirror-image points.
            let point_at = |along: i32| {
                let part = |value: Fp| ((i64::from(value.to_bits()) * i64::from(along)) / (1 << FP_FRAC_BITS)) as i32;
                (start.x.to_bits() + part(step.x), start.y.to_bits() + part(step.y))
            };
            // Where along this step it struck, in full, for the blast and for drawing.
            let impact_at = |along: i32| {
                let part = |value: Fp| Fp::from_bits(((i64::from(value.to_bits()) * i64::from(along)) / (1 << FP_FRAC_BITS)) as i32);
                Vec3Fp::new(start.x + part(step.x), start.y + part(step.y), start.z + part(step.z))
            };
            if let Some((along, struck)) = dome_hit.filter(|(along, _)| first(*along)) {
                position.0 = impact_at(along);
                if let Some(discharged) = discharged {
                    discharges.push(DischargeImpact { def: discharged.def, source: projectile.source, team: projectile.source_team, point: point_at(along), dome: Some(domes[struck].id) });
                    dead.push(id);
                    continue;
                }
                let damage = projectile.damage;
                shield_hits.push((domes[struck].id, damage));
                if projectile.splash_radius > Fp::ZERO {
                    let part = |value: Fp| (i128::from(value.to_bits()) * i128::from(along)) / (1 << FP_FRAC_BITS);
                    let point = [i128::from(start.x.to_bits()) + part(step.x), i128::from(start.y.to_bits()) + part(step.y), i128::from(start.z.to_bits()) + part(step.z)];
                    for (index, dome) in domes.iter().enumerate() {
                        let reach = i128::from(dome.radius) + i128::from(projectile.splash_radius.to_bits());
                        // Heights stretched as `dome_entry` stretches them, for a flattened dome.
                        let dz = (point[2] - i128::from(dome.center[2])) * i128::from(dome.radius) / i128::from(dome.height.max(1));
                        let (dx, dy) = (point[0] - i128::from(dome.center[0]), point[1] - i128::from(dome.center[1]));
                        if index != struck && dome.team != projectile.source_team && dx * dx + dy * dy + dz * dz <= reach * reach {
                            shield_hits.push((dome.id, damage));
                        }
                    }
                }
                dead.push(id);
                continue;
            }

            // Splash shells detonate where they strike, and damage everything nearby.
            let splash = projectile.splash_radius > Fp::ZERO;
            let blast_at = |at: Vec3Fp| Explosion { x: at.x.to_bits(), y: at.y.to_bits(), radius: projectile.splash_radius, damage: projectile.damage, source: Some(projectile.source), stun };
            // A nuclear blast also has an inner ring, that does far more.
            let core_at = |at: Vec3Fp| missile.filter(|missile| missile.inner_radius > Fp::ZERO).map(|missile| Explosion { radius: missile.inner_radius, damage: missile.inner_damage, stun: 0, ..blast_at(at) });

            match (best, probe.hit) {
                (Some((along, _, target, is_structure)), terrain_hit) if terrain_hit.is_none_or(|ground| along <= ground) => {
                    let impact = impact_at(along);
                    position.0 = impact;
                    if let Some(discharged) = discharged {
                        discharges.push(DischargeImpact { def: discharged.def, source: projectile.source, team: projectile.source_team, point: point_at(along), dome: None });
                    } else if splash {
                        explosions.extend([Some(blast_at(impact)), core_at(impact)].into_iter().flatten());
                    } else {
                        let _ = is_structure;
                        hits.push((target, projectile.damage, Some(projectile.source)));
                        if stun > 0 {
                            stuns.push((target, stun));
                        }
                    }
                    dead.push(id);
                }
                (_, Some(ground)) => {
                    let impact = impact_at(ground);
                    position.0 = impact;
                    if let Some(discharged) = discharged {
                        discharges.push(DischargeImpact { def: discharged.def, source: projectile.source, team: projectile.source_team, point: point_at(ground), dome: None });
                    } else if splash {
                        explosions.extend([Some(blast_at(impact)), core_at(impact)].into_iter().flatten());
                    } else {
                        // A shell that comes down on bare ground hits any debris lying there.
                        wreck_blasts.push(Explosion { radius: Fp::ZERO, ..blast_at(impact) });
                    }
                    dead.push(id);
                }
                _ => {
                    if projectile.lifetime_ticks == 0 || !terrain.in_bounds(end.xy()) {
                        dead.push(id);
                    }
                }
            }
        }

    }

    /// Works out the Discharge shells that landed this tick, by the inspiration's rule: each may
    /// spend a share of the energy in store, split between the team's shells in flight, at so much
    /// a point of damage, up to its most; of that, it spends only what destroys the toughest enemy
    /// unit where it lands, and never less than its least. Against a lone commander, a structure,
    /// or a structure's dome, it does the least, and against a mobile unit's dome, what brings
    /// the dome down. The energy goes when it lands. Its blast hurts only enemies, commanders and
    /// structures at most their own amounts.
    fn resolve_discharges(&mut self) {
        if self.pending_discharges.is_empty() {
            return;
        }
        for impact in std::mem::take(&mut self.pending_discharges) {
            let def = impact.def;
            let team = impact.team;
            // The firing player pays, or if it is gone by now, its team's first.
            let owner = self.find_unit(impact.source).map_or_else(|| self.first_player(team), |(_, entity)| self.ecs.get::<&UnitStats>(entity).map_or_else(|_| self.first_player(team), |stats| stats.owner));
            let in_flight = 1 + self
                .projectiles
                .iter()
                .filter(|&&(id, entity)| {
                    self.dead_projectiles.binary_search(&id).is_err()
                        && self.ecs.get::<&Discharged>(entity).is_ok()
                        && self.ecs.get::<&Projectile>(entity).is_ok_and(|projectile| projectile.source_team == team)
                })
                .count() as i64;
            let stored = self.economies[owner.index()].energy.max(Res::ZERO);
            let affordable = stored * i64::from(def.energy_share_permille) / 1000 / in_flight / i64::from(def.energy_per_damage);
            let affordable = Fp::from_num(affordable.min(Res::from_num(def.max_damage)));
            let (x, y) = impact.point;
            // How much the toughest enemy there takes to destroy.
            let mut ideal = Fp::ZERO;
            match impact.dome.and_then(|owner| self.find_unit(owner)) {
                Some((_, owner)) => {
                    let structure = self.ecs.get::<&UnitStats>(owner).is_ok_and(|stats| stats.is_structure);
                    if !structure {
                        ideal = self.ecs.get::<&Shield>(owner).map_or(Fp::ZERO, |shield| shield.max_health);
                    }
                }
                None => {
                    let reach = i64::from(def.search_radius.to_bits());
                    let commanders = &self.commanders;
                    let ecs = &self.ecs;
                    let mut consider = |other: &super::spatial::GridEntry| {
                        let (dx, dy) = (i64::from(other.x - x), i64::from(other.y - y));
                        let within = reach + i64::from(other.radius);
                        if dx * dx + dy * dy > within * within || commanders.contains(&Some(other.id)) {
                            return;
                        }
                        let health = ecs.get::<&Health>(other.entity).map_or(Fp::ZERO, |health| health.current);
                        let shield = ecs.get::<&Shield>(other.entity).ok().filter(|shield| shield.is_up()).map_or(Fp::ZERO, |shield| shield.health);
                        ideal = ideal.max(health.saturating_add(shield));
                    };
                    self.spatial_grid.for_each_candidate(Some(team.enemy()), Layer::Mobile, x, y, (def.search_radius + Fp::from_bits(self.max_mobile_radius)).to_bits(), &mut consider);
                }
            }
            if ideal == Fp::ZERO {
                ideal = def.min_damage;
            }
            let damage = if stored < def.energy_required { affordable } else { affordable.min(ideal).max(def.min_damage) };
            let drain = (Res::from_num(damage) * i64::from(def.energy_per_damage)).min(stored);
            self.economies[owner.index()].energy -= drain;
            if let Some(owner) = impact.dome {
                let structure = self.find_unit(owner).is_some_and(|(_, owner)| self.ecs.get::<&UnitStats>(owner).is_ok_and(|stats| stats.is_structure));
                self.pending_shield_hits.push((owner, if structure { damage.min(def.structure_damage) } else { damage }));
                continue;
            }
            let reach = i64::from(def.splash_radius.to_bits());
            let (commanders, hits) = (self.commanders.clone(), &mut self.pending_hits);
            let mut consider = |other: &super::spatial::GridEntry| {
                let (dx, dy) = (i64::from(other.x - x), i64::from(other.y - y));
                let within = reach + i64::from(other.radius);
                if dx * dx + dy * dy > within * within {
                    return;
                }
                let dealt = if commanders.contains(&Some(other.id)) {
                    damage.min(def.commander_damage)
                } else if other.is_structure {
                    damage.min(def.structure_damage)
                } else {
                    damage
                };
                hits.push((other.entity, dealt, Some(impact.source)));
            };
            let search = def.splash_radius + Fp::from_bits(self.max_mobile_radius.max(self.max_structure_radius));
            self.spatial_grid.for_each_candidate(Some(team.enemy()), Layer::Mobile, x, y, search.to_bits(), &mut consider);
            self.spatial_grid.for_each_candidate(Some(team.enemy()), Layer::Structure, x, y, search.to_bits(), &mut consider);
        }
    }

    /// Queues a hit on every unit within a blast, friend or foe, and on the debris there.
    fn apply_explosion(&mut self, blast: Explosion) {
        self.pending_wreck_blasts.push(blast);
        let (grid, hits, stuns) = (&self.spatial_grid, &mut self.pending_hits, &mut self.pending_stuns);
        let radius = i64::from(blast.radius.to_bits());
        let mut consider = |other: &super::spatial::GridEntry| {
            let (dx, dy) = (i64::from(other.x - blast.x), i64::from(other.y - blast.y));
            let reach = radius + i64::from(other.radius);
            if dx * dx + dy * dy <= reach * reach {
                hits.push((other.entity, blast.damage, blast.source));
                if blast.stun > 0 {
                    stuns.push((other.entity, blast.stun));
                }
            }
        };
        let search = blast.radius.to_bits();
        grid.for_each_candidate(None, Layer::Mobile, blast.x, blast.y, search + self.max_mobile_radius, &mut consider);
        grid.for_each_candidate(None, Layer::Structure, blast.x, blast.y, search + self.max_structure_radius, &mut consider);
    }

    /// Resolves this tick's damage, including blasts from units that die here and the
    /// chain reactions they cause, then removes everything that died.
    pub fn step_cleanup(&mut self) {
        self.dead_units.clear();
        self.blasts.clear();
        self.commander_blasts.clear();
        self.step_wrecks();
        for (id, damage) in std::mem::take(&mut self.strikes) {
            if let Some((_, entity)) = self.find_unit(id) {
                self.pending_hits.push((entity, damage, None));
            }
        }
        self.step_exotic();
        let mut kills: Vec<(SimId, Team, Res)> = Vec::new();
        if !self.config.invulnerable_units {
            loop {
                for blast in std::mem::take(&mut self.pending_explosions) {
                    self.apply_explosion(blast);
                }
                if self.pending_hits.is_empty() {
                    break;
                }
                for (target, damage, source) in std::mem::take(&mut self.pending_hits) {
                    // Who dealt it, if an enemy that is still about: only enemies earn experience.
                    let attacker = source.and_then(|source| {
                        let (_, source_entity) = self.find_unit(source)?;
                        let source_team = self.ecs.get::<&UnitStats>(source_entity).ok()?.team;
                        let target_team = self.ecs.get::<&UnitStats>(target).ok()?.team;
                        (source_team != target_team).then_some(source)
                    });
                    let (id, position, health, blueprint, stats, construction) = self
                        .ecs
                        .query_one_mut::<(&SimId, &Position, &mut Health, Option<&Blueprinted>, &UnitStats, Option<&Construction>)>(target)
                        .expect("hit entities are live until the end of this step");
                    let was_alive = health.current > Fp::ZERO;
                    // Each hit counts for no more than the unit's whole health, as in the
                    // inspiration.
                    let counted = damage.min(health.max).max(Fp::ZERO);
                    health.current = health.current.saturating_sub(damage);
                    let dies = was_alive && health.current <= Fp::ZERO;
                    let (id, x, y, blueprint, team, owner) = (*id, position.0.x.to_bits(), position.0.y.to_bits(), blueprint.map(|b| b.0), stats.team, stats.owner);
                    let built = construction.map_or(Res::ONE, |construction| construction.progress);
                    let finished = construction.is_none();
                    if let Some(attacker) = attacker.filter(|_| was_alive && counted > Fp::ZERO) {
                        if self.ecs.get::<&super::components::VetDamage>(target).is_err() {
                            self.ecs.insert_one(target, super::components::VetDamage::default()).expect("hit entities are live until the end of this step");
                        }
                        let mut record = self.ecs.get::<&mut super::components::VetDamage>(target).expect("just made sure");
                        record.total += counted;
                        match record.by.iter_mut().find(|(id, _)| *id == attacker) {
                            Some((_, dealt)) => *dealt += counted,
                            None => record.by.push((attacker, counted)),
                        }
                    }
                    if dies {
                        self.dead_units.push(id);
                        // The enemy that made the final hit has destroyed one more.
                        if let Some((_, killer)) = attacker.and_then(|killer| self.find_unit(killer)) {
                            let before = self.ecs.get::<&super::components::Kills>(killer).map_or(0, |kills| kills.0);
                            self.ecs.insert_one(killer, super::components::Kills(before + 1)).expect("the killer is live");
                        }
                        if let Some(blueprint) = blueprint {
                            // Those who destroyed it share its mass, or as much as had been built,
                            // each by its share of the damage enemies did it.
                            let worth = self.blueprints.get(blueprint).mass * built;
                            if let Ok(record) = self.ecs.get::<&super::components::VetDamage>(target) {
                                if record.total > Fp::ZERO {
                                    let total = i64::from(record.total.to_bits());
                                    for &(attacker, dealt) in &record.by {
                                        kills.push((attacker, team, worth * i64::from(dealt.to_bits()) / total));
                                    }
                                }
                            }
                            if let Some(blast) = self.blueprints.get(blueprint).death_explosion {
                                // A fabricator only blows up if it was running.
                                let running = finished
                                    && self.ecs.get::<&Paused>(target).is_err()
                                    && self.economies[owner.index()].extractor_efficiency > Res::ZERO;
                                if !blast.only_while_running || running {
                                    self.pending_explosions.push(Explosion { x, y, radius: blast.radius, damage: blast.damage, source: None, stun: 0 });
                                    self.blasts.push(BlastView { team, x, y, radius: blast.radius.to_bits() });
                                    if self.blueprints.get(blueprint).is_commander() {
                                        self.commander_blasts.push([x, y]);
                                    }
                                }
                            }
                            if let Some(storm) = self.blueprints.get(blueprint).death_storm {
                                self.storms.push(super::exotic::Storm::new(team, Vec2Fp::new(Fp::from_bits(x), Fp::from_bits(y)), storm));
                            }
                        }
                    }
                }
            }
        }
        self.blast_wrecks();
        self.damage_shields();
        self.pending_explosions.clear();
        self.pending_hits.clear();
        if !self.pending_stuns.is_empty() {
            self.apply_stuns();
        }
        for (killer, victim_team, mass) in kills {
            self.award_experience(killer, victim_team, mass);
        }
        self.regenerate_veterans();
        self.regenerate_enhanced();
        self.resolve_match_end();

        if !self.dead_units.is_empty() {
            self.dead_units.sort_unstable();
            self.dead_units.dedup();
            let dead = &self.dead_units;
            self.surfaced.retain(|id| dead.binary_search(id).is_err());
            for index in 0..self.dead_units.len() {
                let id = self.dead_units[index];
                let (_, entity) = self.find_unit(id).expect("dead units are still listed");
                self.release_dead_units_orders(entity);
                // What a transport carries goes down with it.
                let cargo = self.ecs.get::<&Transport>(entity).map(|transport| transport.cargo.clone()).unwrap_or_default();
                for carried in cargo {
                    self.destroy_carried(carried);
                }
                self.remove_unit(entity);
            }
            let dead = &self.dead_units;
            let alive = |unit: &(SimId, hecs::Entity)| dead.binary_search(&unit.0).is_err();
            self.units.retain(alive);
            self.builders.retain(alive);
            self.factories.retain(alive);
            self.structures.retain(alive);
            self.patrols.retain(alive);
            self.order_queues.retain(alive);
            self.veterans.retain(alive);
            self.shields.retain(alive);
            self.missile_defenses.retain(alive);
            self.upgrading.retain(alive);
            self.intel.retain(alive);
            self.enhanced.retain(alive);
            self.enhancing.retain(alive);
            self.teleporting.retain(alive);
            self.self_destructing.retain(alive);
            self.being_captured.retain(alive);
            self.aircraft.retain(alive);
            self.submarines.retain(alive);
            self.torpedo_defenses.retain(alive);
            self.seabed_walkers.retain(alive);
            self.transports.retain(alive);
            self.boarding.retain(alive);
            self.airfields.retain(alive);
            self.disabled.retain(alive);
            self.tractors.retain(alive);
            self.dead_units.clear();
        }
        if !self.dead_projectiles.is_empty() {
            let dead = &self.dead_projectiles;
            let ecs = &mut self.ecs;
            // Both lists are sorted by SimId, so one forward pass finds every match.
            let mut next_dead = 0;
            let landed = &mut self.landed;
            let shot_down = &self.shot_down;
            self.projectiles.retain(|&(id, entity)| {
                if dead.get(next_dead) == Some(&id) {
                    next_dead += 1;
                    if let Ok((position, projectile, discharged, missile, beam)) = ecs.query_one_mut::<(&Position, &Projectile, Option<&Discharged>, Option<&Missile>, Option<&super::components::Beam>)>(entity) {
                        // A nuclear missile shot down bursts in the air, with no strike to see or hear.
                        let nuclear_shot_down = missile.is_some_and(|missile| missile.strategic) && shot_down.binary_search(&id).is_ok();
                        if !nuclear_shot_down {
                            landed.push(projectile_view(id, position, projectile, discharged, missile, beam));
                        }
                    }
                    ecs.despawn(entity).expect("dead projectiles are live until now");
                    false
                } else {
                    true
                }
            });
            if !self.missiles.is_empty() {
                self.missiles.retain(|missile| dead.binary_search(&missile.0).is_err());
            }
            self.dead_projectiles.clear();
        }
    }

    /// Takes this tick's hits off the shields they landed on. A dome that runs out collapses,
    /// and comes back at full strength after its recharge time.
    fn damage_shields(&mut self) {
        for (id, damage) in std::mem::take(&mut self.pending_shield_hits) {
            let Ok(index) = self.shields.binary_search_by_key(&id, |shield| shield.0) else {
                continue;
            };
            let mut shield = self.ecs.get::<&mut Shield>(self.shields[index].1).expect("shield list only holds shields");
            if !shield.is_up() {
                continue;
            }
            shield.health = shield.health.saturating_sub(damage);
            if shield.health <= Fp::ZERO {
                shield.collapse();
            }
        }
    }

    /// Wears down debris that this tick's blasts and shells landed on. Debris loses its mass in
    /// proportion to the health it loses, and is gone when that runs out (GAME_SPECIFICATION.md §6.2).
    /// Trees and rocks are not harmed. Each wreck adds up everything that hit it, so the order
    /// of the hits makes no difference.
    fn blast_wrecks(&mut self) {
        let blasts = std::mem::take(&mut self.pending_wreck_blasts);
        if blasts.is_empty() {
            return;
        }
        let mut destroyed = Vec::new();
        for &(id, entity) in &self.wrecks {
            let mut wreck = self.ecs.get::<&mut Wreck>(entity).expect("wreck list only holds live wrecks");
            // Nor debris still falling: blasts hit what lies on the ground, or in the water.
            if wreck.kind.is_scenery() || self.ecs.get::<&Settling>(entity).is_ok_and(|settling| !settling.wet) {
                continue;
            }
            let (x, y, radius) = (wreck.position.x.to_bits(), wreck.position.y.to_bits(), i64::from(wreck.radius.to_bits()));
            let mut damage = Fp::ZERO;
            for blast in &blasts {
                let (dx, dy) = (i64::from(blast.x - x), i64::from(blast.y - y));
                let reach = i64::from(blast.radius.to_bits()) + radius;
                if dx * dx + dy * dy <= reach * reach {
                    damage = damage.saturating_add(blast.damage);
                }
            }
            if damage <= Fp::ZERO {
                continue;
            }
            let left = wreck.health.saturating_sub(damage);
            if left <= Fp::ZERO {
                destroyed.push(id);
            } else {
                let share = Res::from_num(left) / Res::from_num(wreck.health);
                wreck.mass *= share;
                wreck.energy *= share;
                wreck.health = left;
            }
        }
        if destroyed.is_empty() {
            return;
        }
        for &id in &destroyed {
            let index = self.wrecks.binary_search_by_key(&id, |wreck| wreck.0).expect("just found");
            self.ecs.despawn(self.wrecks[index].1).expect("wreck list only holds live wrecks");
        }
        self.wrecks.retain(|wreck| destroyed.binary_search(&wreck.0).is_err());
        self.settling_wrecks.retain(|wreck| destroyed.binary_search(&wreck.0).is_err());
    }

    /// A player whose commander died is out, and everything they own goes with it (GAME_SPECIFICATION.md
    /// §16). A team is out once all its players are, and the match ends: a draw if both went out
    /// on the same tick.
    fn resolve_match_end(&mut self) {
        if self.match_result.is_some() || self.dead_units.is_empty() {
            return;
        }
        let newly_out: Vec<usize> = (0..self.players.len())
            .filter(|&player| !self.eliminated[player] && self.commanders[player].is_some_and(|commander| self.dead_units.contains(&commander)))
            .collect();
        if newly_out.is_empty() {
            return;
        }
        for &player in &newly_out {
            self.eliminated[player] = true;
            for index in 0..self.units.len() {
                let (id, entity) = self.units[index];
                let owned = self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.owner.index() == player);
                if owned && !self.dead_units.contains(&id) {
                    self.dead_units.push(id);
                }
            }
        }
        let out = |team: Team| self.players_of(team).all(|player| self.eliminated[player.index()]);
        self.match_result = match (out(Team::Blue), out(Team::Red)) {
            (true, true) => Some(MatchResult::Draw),
            (true, false) => Some(MatchResult::Winner(Team::Red)),
            (false, true) => Some(MatchResult::Winner(Team::Blue)),
            (false, false) => None,
        };
    }

    /// Despawns a dead unit, leaving a wreck if it had any mass in it.
    /// Hands back routes held by orders that will now never start. The list of queues is
    /// tidied up with the other lists afterwards.
    /// Releases the routes held by a unit's queued walks and moves.
    pub(super) fn release_unit_plans(&mut self, entity: hecs::Entity) {
        self.release_dead_units_orders(entity);
    }

    /// Removes a unit that is in no list, such as one inside a transport, leaving its wreckage.
    pub(super) fn remove_carried_unit(&mut self, entity: hecs::Entity) {
        self.remove_unit(entity);
    }

    fn release_dead_units_orders(&mut self, entity: hecs::Entity) {
        if let Ok(builder) = self.ecs.get::<&Builder>(entity) {
            for task in &builder.tasks {
                if let BuildTask::Walk { order, started: false } = task {
                    self.flow_field.release(order.route);
                }
            }
        }
        if let Ok(queue) = self.ecs.get::<&OrderQueue>(entity) {
            for order in &queue.0 {
                self.flow_field.release(order.route);
            }
        }
    }

    fn remove_unit(&mut self, entity: hecs::Entity) {
        let order = *self.ecs.get::<&Order>(entity).expect("units have orders");
        if order.kind != OrderKind::Idle {
            self.flow_field.release(order.route);
        }
        let position = self.ecs.get::<&Position>(entity).expect("units have positions").0;
        let radius = self.ecs.get::<&UnitStats>(entity).expect("units have stats").radius;
        let velocity = self.ecs.get::<&Velocity>(entity).map_or(Vec3Fp::ZERO, |velocity| velocity.0);
        if let Ok(footprint) = self.ecs.get::<&Footprint>(entity).map(|footprint| *footprint) {
            self.clear_footprint(footprint);
            self.economy_dirty = true;
        }
        // A commander or field rig takes its income and storage with it.
        if let Ok(blueprint) = self.ecs.get::<&Blueprinted>(entity) {
            let unit = self.blueprints.get(blueprint.0);
            if unit.mass_income > Res::ZERO || unit.energy_income > Res::ZERO || unit.mass_storage > Res::ZERO || unit.energy_storage > Res::ZERO {
                self.economy_dirty = true;
            }
        }
        // Where its wreck starts: its middle, half its radius over its underside, as a wreck at
        // rest lies half its radius over the ground. A unit on the ground leaves it at rest. One
        // in the air falls, going on the way it was going; one in the water, on it or under it,
        // sinks to the floor (`step_wrecks`).
        let half = radius / 2;
        let start = position.z - half;
        let ground = self.terrain.height_at(position.xy());
        let wet = ground < self.terrain.water_level() && start <= self.terrain.water_level() + half;
        let settling = (start > ground + half).then_some(Settling { z: start, velocity, wet });
        // A unit that blew itself up leaves nothing to reclaim. The inspiration's rules for the
        // rest: 90% of the mass, less by tech level, less again if it sank, for as much as was
        // built; nothing from a unit under half built, or from a structure not finished. What
        // falls into the water loses the share for sinking when it does.
        let wreck = self.ecs.get::<&Blueprinted>(entity).ok().filter(|_| !self.blew_itself_up(entity)).and_then(|blueprint| {
            let built = self.ecs.get::<&Construction>(entity).map_or(Res::ONE, |construction| construction.progress);
            let unit = self.blueprints.get(blueprint.0);
            if built < Res::ONE / 2 || (unit.is_structure() && built < Res::ONE) {
                return None;
            }
            let tech = if unit.is_commander() || unit.class == super::blueprints::UnitClass::FieldRig {
                WRECK_TECH_PERMILLE[0]
            } else {
                WRECK_TECH_PERMILLE[usize::from(unit.tier.clamp(1, 4)) - 1]
            };
            let mut mass = unit.mass * WRECK_MASS_PERMILLE / 1000 * tech / 1000;
            if wet {
                mass = mass * WRECK_SUNK_PERMILLE / 1000;
            }
            let health = Fp::from_num(Res::from_num(unit.health) * WRECK_HEALTH_PERMILLE / 1000 * built).max(Fp::from_bits(1));
            Some((mass * built, health))
        });
        self.ecs.despawn(entity).expect("dead units are live until now");
        if let Some((mass, health)) = wreck.filter(|(mass, _)| *mass > Res::ZERO) {
            let wreck_id = self.next_id();
            let wreck = self.ecs.spawn((wreck_id, Wreck { mass, energy: Res::ZERO, position: position.xy(), radius, kind: WreckKind::Debris, health }));
            self.wrecks.push((wreck_id, wreck));
            if let Some(settling) = settling {
                self.ecs.insert_one(wreck, settling).expect("just spawned");
                self.settling_wrecks.push((wreck_id, wreck));
            }
        }
    }

    /// Moves wrecks on their way down (GAME_SPECIFICATION.md §6.1). In the air, debris falls as
    /// a bomb does, keeping most of the way it was going, so that a downed aircraft comes down
    /// well ahead of where it was hit; it strikes the ground and lies there, or the water, where
    /// it loses the share of its mass for sinking. In the water, it soon slows, and sinks at a
    /// steady pace to the floor, every sea having one (`SEA_DEPTH_UNITS`), and lies there. It can
    /// be reclaimed all the while it is in the water, as on the floor, from above as from beside
    /// it: reclaim reaches as far across, whatever the depth.
    pub(super) fn step_wrecks(&mut self) {
        if self.settling_wrecks.is_empty() {
            return;
        }
        let terrain = &self.terrain;
        let water = terrain.water_level();
        let max = Fp::from_bits(terrain.max_coordinate_raw());
        let mut settled = Vec::new();
        for &(id, entity) in &self.settling_wrecks {
            let (wreck, settling) = self.ecs.query_one_mut::<(&mut Wreck, &mut Settling)>(entity).expect("settling wrecks are live wrecks with a way down");
            let half = wreck.radius / 2;
            let velocity = &mut settling.velocity;
            if settling.wet {
                velocity.x = velocity.x * WRECK_WATER_KEEP_SIXTEENTHS / 16;
                velocity.y = velocity.y * WRECK_WATER_KEEP_SIXTEENTHS / 16;
                // Down to a steady pace: gathering it from still, or losing what it fell in with.
                velocity.z = if velocity.z < -WRECK_SINK_SPEED { (velocity.z - WRECK_SINK_SPEED) / 2 } else { (velocity.z - WRECK_SINK_PULL).max(-WRECK_SINK_SPEED) };
            } else {
                velocity.x = velocity.x * WRECK_AIR_KEEP_SIXTEENTHS / 16;
                velocity.y = velocity.y * WRECK_AIR_KEEP_SIXTEENTHS / 16;
                velocity.z -= BOMB_GRAVITY;
            }
            let at = Vec2Fp::new((wreck.position.x + velocity.x).clamp(Fp::ZERO, max), (wreck.position.y + velocity.y).clamp(Fp::ZERO, max));
            wreck.position = at;
            settling.z += velocity.z;
            let ground = terrain.height_at(at);
            if !settling.wet && ground < water && settling.z <= water + half {
                settling.wet = true;
                wreck.mass = wreck.mass * WRECK_SUNK_PERMILLE / 1000;
            }
            if settling.z <= ground + half {
                settled.push(id);
            }
        }
        if settled.is_empty() {
            return;
        }
        for &id in &settled {
            let index = self.settling_wrecks.binary_search_by_key(&id, |wreck| wreck.0).expect("just found");
            self.ecs.remove_one::<Settling>(self.settling_wrecks[index].1).expect("settling wrecks have a way down");
        }
        self.settling_wrecks.retain(|wreck| settled.binary_search(&wreck.0).is_err());
    }

    /// The height of a wreck's middle: where it is on its way down, or, at rest, half its radius
    /// over the ground, or the sea floor.
    pub fn wreck_height(&self, entity: hecs::Entity, wreck: &Wreck) -> Fp {
        self.ecs.get::<&Settling>(entity).map_or_else(|_| self.terrain.height_at(wreck.position) + wreck.radius / 2, |settling| settling.z)
    }

    /// Whether a wreck is still in the air, falling, where reclaim cannot reach it.
    pub fn wreck_in_the_air(&self, entity: hecs::Entity) -> bool {
        self.ecs.get::<&Settling>(entity).is_ok_and(|settling| !settling.wet)
    }

    /// Computes a 64-bit checksum of the entire simulation state. Every value that can
    /// affect a future tick is hashed. `Hasher::write_i32` and friends use native
    /// endianness, so all values go through `to_le_bytes`.
    pub fn compute_checksum(&self) -> u64 {
        let mut hasher = XxHash64::with_seed(0);
        hasher.write(&self.current_tick.to_le_bytes());
        hasher.write(&self.next_sim_id.to_le_bytes());
        hasher.write(&self.rng.state().to_le_bytes());
        for stream in &self.scatter_rngs {
            hasher.write(&stream.state().to_le_bytes());
        }
        // Only while any is up, so that checksums from before surfacing still hold.
        if !self.surfaced.is_empty() {
            hasher.write(&[7]);
            for id in &self.surfaced {
                hasher.write(&id.0.to_le_bytes());
            }
        }
        hasher.write(&self.terrain.content_hash().to_le_bytes());
        hasher.write(&[
            u8::from(self.config.friendly_fire),
            u8::from(self.config.invulnerable_units),
            u8::from(self.config.fog_of_war),
        ]);
        hasher.write(&self.max_mobile_radius.to_le_bytes());
        hasher.write(&self.max_structure_radius.to_le_bytes());
        hasher.write(&self.blueprints.content_hash().to_le_bytes());
        for economy in &self.economies {
            economy.hash_state(&mut hasher);
        }
        for &(x, y) in &self.deposits {
            hasher.write(&x.to_le_bytes());
            hasher.write(&y.to_le_bytes());
        }
        // Only maps that have vents hash them.
        if !self.vents.is_empty() {
            hasher.write(&[1]);
            for &(x, y) in &self.vents {
                hasher.write(&x.to_le_bytes());
                hasher.write(&y.to_le_bytes());
            }
        }
        for (&cell, id) in &self.occupancy {
            hasher.write(&cell.to_le_bytes());
            hasher.write(&id.0.to_le_bytes());
        }
        for commander in &self.commanders {
            hasher.write(&commander.map_or(u32::MAX, |id| id.0).to_le_bytes());
        }
        // Players, and who is out, only with more than two, so that 1v1 checksums from before
        // there were players hold.
        if self.players.len() > 2 {
            hasher.write(&[9]);
            for (team, out) in self.players.iter().zip(&self.eliminated) {
                hasher.write(&[*team as u8, u8::from(*out)]);
            }
        }
        hasher.write(&[match self.match_result {
            None => 0,
            Some(MatchResult::Draw) => 1,
            Some(MatchResult::Winner(team)) => 2 + team as u8,
        }]);
        if self.config.fog_of_war {
            self.visibility.hash_state(&mut hasher);
        }
        // Only once a team remembers anything, so that checksums from before remembering hold.
        if self.known_structures.iter().any(|known| !known.is_empty()) {
            hasher.write(&[8]);
            for known in &self.known_structures {
                hasher.write(&(known.len() as u64).to_le_bytes());
                for record in known {
                    hasher.write(&record.id.0.to_le_bytes());
                    hasher.write(&record.blueprint.map_or(u32::MAX, u32::from).to_le_bytes());
                    for value in [record.x, record.y, record.z, record.radius, record.facing[0], record.facing[1], record.aim[0], record.aim[1]] {
                        hasher.write(&value.to_le_bytes());
                    }
                    hasher.write(&record.health_permille.to_le_bytes());
                    hasher.write(&record.built_permille.to_le_bytes());
                    hasher.write(&[u8::from(record.gone)]);
                }
            }
        }

        hasher.write(&(self.units.len() as u64).to_le_bytes());
        for &(id, entity) in &self.units {
            hasher.write(&id.0.to_le_bytes());
            self.hash_unit(entity, &mut hasher);
        }
        // Nothing is written without carried units, so checksums from before transports still hold.
        if !self.carried.is_empty() {
            hasher.write(&[4]);
            hasher.write(&(self.carried.len() as u64).to_le_bytes());
            for &(id, entity) in &self.carried {
                hasher.write(&id.0.to_le_bytes());
                self.hash_unit(entity, &mut hasher);
            }
        }
        hasher.write(&(self.projectiles.len() as u64).to_le_bytes());
        for &(id, entity) in &self.projectiles {
            hasher.write(&id.0.to_le_bytes());
            self.hash_projectile(entity, &mut hasher);
        }
        // Nothing is written without a launch under way, so that checksums from before hold.
        if !self.silo_launches.is_empty() {
            hasher.write(&[0x51]);
            for launch in &self.silo_launches {
                hasher.write(&launch.shot.source.0.to_le_bytes());
                hasher.write(&launch.ticks.to_le_bytes());
                write_vec3(&mut hasher, launch.shot.from);
                write_vec3(&mut hasher, launch.shot.to);
            }
        }
        hasher.write(&(self.wrecks.len() as u64).to_le_bytes());
        for &(id, entity) in &self.wrecks {
            hasher.write(&id.0.to_le_bytes());
            let wreck = self.ecs.get::<&Wreck>(entity).expect("wreck list only holds live wrecks");
            hasher.write(&wreck.mass.to_bits().to_le_bytes());
            hasher.write(&wreck.energy.to_bits().to_le_bytes());
            write_fp(&mut hasher, wreck.position.x);
            write_fp(&mut hasher, wreck.position.y);
            write_fp(&mut hasher, wreck.radius);
            hasher.write(&[wreck.kind as u8]);
            write_fp(&mut hasher, wreck.health);
        }
        // Only while any is in flight, so that checksums from before interceptors flew still hold
        // for matches where none does.
        if !self.interceptors.is_empty() {
            hasher.write(&[0x1c]);
            for interceptor in &self.interceptors {
                hasher.write(&interceptor.id.0.to_le_bytes());
                hasher.write(&interceptor.target.0.to_le_bytes());
                write_vec3(&mut hasher, interceptor.meet);
                hasher.write(&interceptor.arrives.to_le_bytes());
            }
        }
        // Only while any is on its way down, so that checksums from before wrecks fell or sank
        // still hold for matches where none does.
        if !self.settling_wrecks.is_empty() {
            hasher.write(&[0x5e]);
            for &(id, entity) in &self.settling_wrecks {
                let settling = self.ecs.get::<&Settling>(entity).expect("settling wrecks have a way down");
                hasher.write(&id.0.to_le_bytes());
                write_fp(&mut hasher, settling.z);
                write_vec3(&mut hasher, settling.velocity);
                hasher.write(&[u8::from(settling.wet)]);
            }
        }
        self.flow_field.hash_state(&mut hasher);
        // Nothing is written without storms, so checksums from before they existed still hold.
        if !self.storms.is_empty() {
            hasher.write(&[3]);
            hasher.write(&(self.storms.len() as u64).to_le_bytes());
            for storm in &self.storms {
                storm.hash_state(&mut hasher);
            }
        }
        // Nothing is written without formations, so checksums from before they existed still hold.
        if !self.formations.is_empty() {
            hasher.write(&(self.formations.len() as u64).to_le_bytes());
            for (route, travel) in &self.formations {
                hasher.write(&route.to_le_bytes());
                travel.hash_state(&mut hasher);
            }
        }
        hasher.finish()
    }

    fn hash_unit(&self, entity: hecs::Entity, hasher: &mut impl Hasher) {
        let mut query = self
            .ecs
            .query_one::<(&Position, &Velocity, &UnitStats, &Health, &Combat, &Order)>(entity)
            .expect("unit list only holds live units");
        let (position, velocity, stats, health, combat, order) = query.get().expect("units have every component");
        write_vec3(hasher, position.0);
        write_vec3(hasher, velocity.0);
        write_fp(hasher, stats.max_speed);
        write_fp(hasher, stats.radius);
        hasher.write(&[stats.team as u8, stats.move_class as u8, u8::from(stats.is_structure)]);
        // Its owner, only when it is not its team's own player, as every unit is in a 1v1, so
        // that checksums from before there were players hold.
        if stats.owner.index() != stats.team.index() {
            hasher.write(&[0xA7, stats.owner.0]);
        }
        write_fp(hasher, stats.vision);
        hasher.write(&stats.phase.to_le_bytes());
        write_fp(hasher, health.current);
        write_fp(hasher, health.max);
        write_fp(hasher, combat.range);
        hasher.write(&combat.reload_ticks.to_le_bytes());
        hasher.write(&combat.ticks_until_ready.to_le_bytes());
        write_fp(hasher, combat.projectile_speed);
        write_fp(hasher, combat.projectile_damage);
        write_fp(hasher, combat.projectile_splash);
        hasher.write(&combat.target.map_or(u32::MAX, |target| target.0).to_le_bytes());
        hasher.write(&combat.forced_target.map_or(u32::MAX, |target| target.0).to_le_bytes());
        // Written only while chasing, so that checksums from before chases still hold.
        if combat.chasing {
            hasher.write(&[3]);
        }
        let (blocked, blocked_until) = combat.blocked.map_or((u32::MAX, 0), |(target, until)| (target.0, until));
        hasher.write(&blocked.to_le_bytes());
        hasher.write(&blocked_until.to_le_bytes());
        // Nothing is written without a ground attack, so checksums from before it existed still hold.
        if let Some(spot) = combat.ground_target {
            hasher.write(&[2]);
            write_fp(hasher, spot.x);
            write_fp(hasher, spot.y);
        }
        hasher.write(&[u8::from(combat.enabled), order.kind as u8, u8::from(combat.line_blocked)]);
        if let Some((closest, since)) = combat.closing {
            hasher.write(&closest[0].to_le_bytes());
            hasher.write(&closest[1].to_le_bytes());
            hasher.write(&since.to_le_bytes());
        }
        hasher.write(&order.route.to_le_bytes());
        if let Some(slot) = order.slot {
            write_fp(hasher, slot.x);
            write_fp(hasher, slot.y);
        }
        hasher.write(&order.slot_ticks.to_le_bytes());
        hasher.write(&[u8::from(order.direct)]);
        drop(query);
        self.hash_unit_extras(entity, hasher);
    }

    fn hash_projectile(&self, entity: hecs::Entity, hasher: &mut impl Hasher) {
        let mut query = self
            .ecs
            .query_one::<(&Position, &Projectile, Option<&Discharged>)>(entity)
            .expect("projectile list only holds live projectiles");
        let (position, projectile, discharged) = query.get().expect("projectiles have every component");
        if let Some(discharged) = discharged {
            hasher.write(&[1]);
            write_fp(hasher, discharged.def.structure_damage);
        }

        write_vec3(hasher, position.0);
        write_vec3(hasher, projectile.velocity);
        write_fp(hasher, projectile.damage);
        hasher.write(&projectile.source.0.to_le_bytes());
        hasher.write(&[projectile.source_team as u8]);
        hasher.write(&projectile.lifetime_ticks.to_le_bytes());
        write_fp(hasher, projectile.splash_radius);
        hasher.write(&projectile.allied_safe_ticks.to_le_bytes());
        drop(query);
        if let Ok(homing) = self.ecs.get::<&Homing>(entity) {
            hasher.write(&[3]);
            hasher.write(&homing.target.0.to_le_bytes());
            write_fp(hasher, homing.speed);
            if homing.underwater {
                hasher.write(&[1]);
                write_fp(hasher, homing.top_speed);
                if let Some(lure) = homing.lure {
                    hasher.write(&[2]);
                    write_fp(hasher, lure.x);
                    write_fp(hasher, lure.y);
                    write_fp(hasher, lure.z);
                }
            }
        }
        if self.ecs.get::<&Falling>(entity).is_ok() {
            hasher.write(&[4]);
        }
        if let Ok(lobbed) = self.ecs.get::<&super::components::Lobbed>(entity) {
            hasher.write(&[6]);
            write_fp(hasher, lobbed.0);
        }
        if let Ok(stunning) = self.ecs.get::<&Stunning>(entity) {
            hasher.write(&[5]);
            hasher.write(&stunning.0.to_le_bytes());
        }
        if let Ok(flight) = self.ecs.get::<&super::components::NuclearFlight>(entity) {
            hasher.write(&[7]);
            write_fp(hasher, flight.travelled);
            write_fp(hasher, flight.apex);
            write_fp(hasher, flight.turn);
            write_vec3(hasher, flight.start);
            write_vec3(hasher, flight.target);
        }
        // Last, so that nothing is written for a shell.
        if let Ok(missile) = self.ecs.get::<&Missile>(entity) {
            hasher.write(&[2, u8::from(missile.strategic)]);
            write_fp(hasher, missile.target.x);
            write_fp(hasher, missile.target.y);
            write_fp(hasher, missile.speed);
            write_fp(hasher, missile.cruise_height);
            if missile.inner_radius > Fp::ZERO {
                write_fp(hasher, missile.inner_radius);
                write_fp(hasher, missile.inner_damage);
            }
        }
    }

    /// Copies everything, with no fog of war. For the stress-test scenarios.
    pub fn snapshot(&self) -> SimSnapshot {
        self.snapshot_for(None)
    }

    /// Copies the state the renderer needs, limited to what `viewer` can see.
    /// Rendering never touches `SimWorld` directly.
    pub fn snapshot_for(&self, viewer: Option<Team>) -> SimSnapshot {
        let seen = |team: Team, x: i32, y: i32| match viewer {
            Some(viewer) if self.config.fog_of_war && team != viewer => self.visibility.is_visible(viewer, x, y),
            _ => true,
        };
        // Builders spending power, which shows on them while it lasts.
        let mut builders: Vec<SimId> = self
            .work_links
            .iter()
            .filter(|link| link.reclaim || self.find_unit(link.builder).and_then(|(_, entity)| self.ecs.get::<&UnitStats>(entity).ok().map(|stats| self.economies[stats.owner.index()].satisfaction > Res::ZERO)).unwrap_or(false))
            .map(|link| link.builder)
            .collect();
        builders.sort_unstable();
        let mut unit_query = self.ecs.query::<(&Position, &UnitStats, &Health, Option<&Blueprinted>, Option<&Construction>, Option<&Turning>, Option<&Turret>, Option<&Factory>, Option<&Paused>, Option<&super::components::ExtraWeapons>)>();
        let unit_view = unit_query.view();
        // Each player's units in the snapshot, and what the mobile ones cost.
        let mut tallies = vec![(0_u32, 0_i64); self.players.len()];
        let units: Vec<UnitView> = self
            .units
            .iter()
            .filter_map(|&(id, entity)| {
                let (position, stats, health, blueprint, construction, turning, turret, factory, paused, extras) = unit_view.get(entity).expect("unit list only holds live units");
                let powered = self.economies[stats.owner.index()].satisfaction > Res::ZERO;
                let producing = factory.is_some_and(|factory| !factory.queue.is_empty()) && paused.is_none() && powered;
                let (x, y, z) = (position.0.x.to_bits(), position.0.y.to_bits(), position.0.z.to_bits());
                let cloaked_away = viewer.is_some_and(|viewer| self.concealed[viewer.index()].binary_search(&id).is_ok());
                let shown = seen(stats.team, x, y) && !cloaked_away;
                if let (true, Some(tally)) = (shown && blueprint.is_some(), tallies.get_mut(stats.owner.index())) {
                    tally.0 += 1;
                    if !stats.is_structure {
                        tally.1 += blueprint.map_or(0, |blueprint| self.blueprints.get(blueprint.0).mass.to_num::<i64>());
                    }
                }
                shown.then(|| UnitView {
                    id,
                    team: stats.team,
                    ally: stats.owner != self.first_player(stats.team),
                    x,
                    y,
                    z,
                    radius: stats.radius.to_bits(),
                    blueprint: blueprint.map_or(u16::MAX, |blueprint| blueprint.0 .0),
                    health_permille: permille(i64::from(health.current.to_bits()), i64::from(health.max.to_bits())),
                    built_permille: construction.map_or(1000, |construction| {
                        (construction.progress.to_bits() * 1000 / Res::ONE.to_bits()).clamp(0, 1000) as u16
                    }),
                    is_structure: stats.is_structure,
                    facing: turning.map_or([0, 0], |turning| [turning.facing.x.to_bits(), turning.facing.y.to_bits()]),
                    aim: turret.map_or([0, 0], |turret| [turret.facing.x.to_bits(), turret.facing.y.to_bits()]),
                    extra_aims: std::array::from_fn(|index| {
                        let turret = extras.and_then(|extras| extras.0.get(index)).and_then(|weapon| weapon.turret);
                        turret.map_or([0, 0], |turret| [turret.facing.x.to_bits(), turret.facing.y.to_bits()])
                    }),
                    pitches: std::array::from_fn(|index| {
                        let turret = if index == 0 { turret.copied() } else { extras.and_then(|extras| extras.0.get(index - 1)).and_then(|weapon| weapon.turret) };
                        turret.map_or(0, |turret| turret.elevation.y.to_bits())
                    }),
                    working: producing || builders.binary_search(&id).is_ok(),
                    assembling: factory.and_then(|factory| {
                        let item = *factory.queue.first()?;
                        // Upgrades and missiles are not built in the bay.
                        let made = self.blueprints.get(item);
                        (!made.is_factory() && made.class != UnitClass::Ammo)
                            .then(|| (item.0, (factory.progress.to_bits() * 1000 / Res::ONE.to_bits()).clamp(0, 1000) as u16))
                    }),
                    upgrade: {
                        let share = |progress: Res| (progress.to_bits() * 1000 / Res::ONE.to_bits()).clamp(0, 1000) as u16;
                        let from_queue = factory.and_then(|factory| {
                            let item = *factory.queue.first()?;
                            self.blueprints.get(item).is_factory().then(|| (item.0, share(factory.progress)))
                        });
                        from_queue.or_else(|| self.ecs.get::<&Upgrading>(entity).ok().map(|upgrading| (upgrading.into.0, share(upgrading.progress))))
                    },
                    remembered: false,
                })
            })
            .collect();
        // The enemy structures the viewer has found and identified, out of its sight now, as it
        // last saw them. Those only on radar are contacts.
        let mut units = units;
        if let Some(viewer) = viewer.filter(|_| self.config.fog_of_war) {
            let shown: Vec<SimId> = units.iter().map(|unit| unit.id).collect();
            for record in &self.known_structures[viewer.index()] {
                let Some(blueprint) = record.blueprint else {
                    continue;
                };
                if shown.binary_search(&record.id).is_ok() {
                    continue;
                }
                units.push(UnitView {
                    id: record.id,
                    team: viewer.enemy(),
                    ally: false,
                    x: record.x,
                    y: record.y,
                    z: record.z,
                    radius: record.radius,
                    blueprint,
                    health_permille: record.health_permille,
                    built_permille: record.built_permille,
                    is_structure: true,
                    facing: record.facing,
                    aim: record.aim,
                    extra_aims: [[0, 0]; 3],
                    pitches: [0; 4],
                    working: false,
                    assembling: None,
                    upgrade: None,
                    remembered: true,
                });
            }
            units.sort_by_key(|unit| unit.id);
        }
        let mut projectile_query = self.ecs.query::<(&Position, &Projectile, Option<&Discharged>, Option<&Missile>, Option<&super::components::Beam>)>();
        let projectile_view = projectile_query.view();
        let mut strategic_launches = Vec::new();
        let projectiles = self
            .projectiles
            .iter()
            .filter_map(|&(id, entity)| {
                let (position, projectile, discharged, missile, beam) = projectile_view.get(entity).expect("projectile list only holds live projectiles");
                let mut view = self::projectile_view(id, position, projectile, discharged, missile, beam);
                // Ids only grow, so this tick's launches are in id order.
                if view.from.is_none() {
                    view.from = self.launched.binary_search_by_key(&id, |launch| launch.0).ok().map(|index| self.launched[index].1);
                }
                // A strategic launch is heard by everyone, seen or not.
                if view.kind == ProjectileKind::StrategicMissile && self.launched.binary_search_by_key(&id, |launch| launch.0).is_ok() {
                    strategic_launches.push(projectile.source_team);
                }
                seen(projectile.source_team, view.x, view.y).then_some(view)
            })
            .collect::<Vec<_>>();
        // Strategic interceptors in flight, drawn as missiles.
        let mut projectiles = projectiles;
        if !self.interceptors.is_empty() {
            projectiles.extend(self.interceptors.iter().map(|interceptor| interceptor.view(self.current_tick)).filter(|view| seen(view.team, view.x, view.y)));
            projectiles.sort_by_key(|view| view.id);
        }
        let detonations = self
            .landed
            .iter()
            .filter(|view| view.kind == ProjectileKind::StrategicMissile)
            .map(|view| Detonation { kind: DetonationKind::StrategicMissile, x: view.x, y: view.y })
            .chain(self.commander_blasts.iter().map(|&[x, y]| Detonation { kind: DetonationKind::Commander, x, y }))
            .collect();
        let landed = self.landed.iter().filter(|view| seen(view.team, view.x, view.y)).copied().collect();
        let blasts = self.blasts.iter().filter(|view| seen(view.team, view.x, view.y)).copied().collect();
        let wrecks = self
            .wrecks
            .iter()
            .filter_map(|&(id, entity)| {
                let wreck = self.ecs.get::<&Wreck>(entity).expect("wreck list only holds live wrecks");
                let (x, y) = (wreck.position.x.to_bits(), wreck.position.y.to_bits());
                // Wrecks belong to no one, so use the viewer's own sight for both cases. Trees and
                // rocks are part of the map, and show through fog like the terrain.
                let visible = wreck.kind.is_scenery() || viewer.is_none_or(|viewer| !self.config.fog_of_war || self.visibility.is_visible(viewer, x, y));
                visible.then(|| WreckView {
                    id,
                    x,
                    y,
                    z: self.wreck_height(entity, &wreck).to_bits(),
                    radius: wreck.radius.to_bits(),
                    mass: wreck.mass.to_num::<i32>(),
                    energy: wreck.energy.to_num::<i32>(),
                    kind: wreck.kind,
                    motion: match self.ecs.get::<&Settling>(entity).map(|settling| settling.wet) {
                        Ok(true) => WreckMotion::Sinking,
                        Ok(false) => WreckMotion::Falling,
                        Err(_) => WreckMotion::Still,
                    },
                })
            })
            .collect();
        let shields = self
            .shields
            .iter()
            .filter_map(|&(id, entity)| {
                let shield = self.ecs.get::<&Shield>(entity).ok()?;
                let base = self.ecs.get::<&Position>(entity).ok()?.0.xy();
                let stats = *self.ecs.get::<&UnitStats>(entity).ok()?;
                let team = stats.team;
                // Drawn where it stands: a ship's on the water, as `standing_domes` has it.
                let floor = if stats.move_class == MoveClass::Naval { self.terrain.height_at(base).max(self.terrain.water_level()) } else { self.terrain.height_at(base) };
                let (x, y) = (base.x.to_bits(), base.y.to_bits());
                if !seen(team, x, y) {
                    return None;
                }
                let switched_on = self.ecs.get::<&Construction>(entity).is_err() && self.ecs.get::<&Paused>(entity).is_err();
                Some(ShieldView {
                    id,
                    team,
                    center: [x, y, floor.to_bits()],
                    radius: shield.radius.to_bits(),
                    height: shield.height.to_bits(),
                    health_permille: permille(i64::from(shield.health.to_bits()), i64::from(shield.max_health.to_bits())),
                    up: switched_on && shield.is_up() && !self.is_disabled(id),
                    recharging: shield.recharging,
                })
            })
            .collect();
        let interceptions = self.interceptions.iter().copied().filter(|[from, to]| viewer.is_none_or(|viewer| seen(viewer.enemy(), from[0], from[1]) || seen(viewer.enemy(), to[0], to[1]))).collect();
        let decoys = self.decoys.iter().copied().filter(|(at, _)| viewer.is_none_or(|viewer| seen(viewer.enemy(), at[0], at[1]))).collect();
        let cargo = self
            .transports
            .iter()
            .filter(|&&(_, entity)| viewer.is_none_or(|viewer| self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == viewer)))
            .map(|&(id, entity)| (id, self.ecs.get::<&Transport>(entity).map_or(0, |transport| transport.cargo.len() as u32)))
            .collect();
        let adjacency = self
            .adjacency_links
            .iter()
            .filter_map(|&(giver, receiver)| {
                let (_, giver_entity) = self.find_unit(giver)?;
                let (_, receiver_entity) = self.find_unit(receiver)?;
                let team = self.ecs.get::<&UnitStats>(giver_entity).ok()?.team;
                if viewer.is_some_and(|viewer| viewer != team) {
                    return None;
                }
                let at = |entity: hecs::Entity| self.ecs.get::<&Position>(entity).ok().map(|position| {
                    let ground = self.terrain.height_at(position.0.xy());
                    [position.0.x.to_bits(), position.0.y.to_bits(), ground.to_bits()]
                });
                Some((at(giver_entity)?, at(receiver_entity)?))
            })
            .collect();
        let stocks = viewer.map_or_else(Vec::new, |viewer| {
            self.factories
                .iter()
                .filter(|&&(_, entity)| self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == viewer))
                .filter_map(|&(id, entity)| {
                    let launcher = self.ecs.get::<&Launcher>(entity).ok().map(|launcher| launcher.stock);
                    let defense = self.ecs.get::<&MissileDefense>(entity).ok().map(|defense| defense.stock);
                    launcher.or(defense).map(|stock| (id, stock))
                })
                .collect()
        });
        let upgrades = viewer.map_or_else(Vec::new, |viewer| {
            self.upgrading
                .iter()
                .filter(|&&(_, entity)| self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == viewer))
                .filter_map(|&(id, entity)| {
                    let progress = self.ecs.get::<&Upgrading>(entity).ok()?.progress;
                    Some((id, (progress.to_bits() * 1000 / Res::ONE.to_bits()).clamp(0, 1000) as u16))
                })
                .collect()
        });
        let own = |list: &[(SimId, hecs::Entity)]| -> Vec<(SimId, hecs::Entity)> {
            viewer.map_or_else(Vec::new, |viewer| {
                list.iter().copied().filter(|&(_, entity)| self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == viewer)).collect()
            })
        };
        let permille = |done: i64, of: i64| (done * 1000 / of.max(1)).clamp(0, 1000) as u16;
        let enhancements = own(&self.enhanced)
            .into_iter()
            .filter_map(|(id, entity)| {
                let installed = *self.ecs.get::<&super::components::Enhancements>(entity).ok()?;
                Some((id, installed.0.map(|slot| slot.map_or(u16::MAX, |enhancement| enhancement.0))))
            })
            .collect();
        let enhancing = own(&self.enhancing)
            .into_iter()
            .filter_map(|(id, entity)| {
                let under_way = *self.ecs.get::<&super::components::Enhancing>(entity).ok()?;
                Some((id, under_way.enhancement.0, permille(under_way.progress.to_bits(), Res::ONE.to_bits())))
            })
            .collect();
        let teleports = own(&self.teleporting)
            .into_iter()
            .filter_map(|(id, entity)| {
                let charge = *self.ecs.get::<&super::components::Teleporting>(entity).ok()?;
                let done = i64::from(charge.total_ticks - charge.ticks_left);
                Some((id, [charge.destination.x.to_bits(), charge.destination.y.to_bits()], permille(done, i64::from(charge.total_ticks))))
            })
            .collect();
        let self_destructs = own(&self.self_destructing)
            .into_iter()
            .filter_map(|(id, entity)| Some((id, self.ecs.get::<&super::components::SelfDestructing>(entity).ok()?.ticks_left)))
            .collect();
        let contacts = match viewer {
            Some(viewer) if self.config.fog_of_war => self.contacts_for(viewer),
            _ => Vec::new(),
        };
        let blips = contacts.iter().map(|contact| [contact.x, contact.y, contact.z]).collect();
        let own_intel = |viewer: Team| self.intel.iter().filter(move |&&(_, entity)| self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == viewer));
        let sensor_ranges = viewer.map_or_else(Vec::new, |viewer| {
            own_intel(viewer)
                .filter(|&&(id, entity)| self.ecs.get::<&Intel>(entity).is_ok_and(|intel| intel.running) && self.ecs.get::<&Construction>(entity).is_err() && !self.is_disabled(id))
                .filter_map(|&(_, entity)| {
                    let (radar, deep_scan) = self.sensor_ranges(entity);
                    let at = self.ecs.get::<&Position>(entity).ok()?.0;
                    (radar > Fp::ZERO || deep_scan > Fp::ZERO).then(|| SensorRangeView { center: [at.x.to_bits(), at.y.to_bits()], radar: radar.to_bits(), deep_scan: deep_scan.to_bits() })
                })
                .collect()
        });
        let cloaks = viewer.map_or_else(Vec::new, |viewer| {
            own_intel(viewer)
                .filter_map(|&(id, entity)| {
                    let unit = self.blueprints.get(self.ecs.get::<&Blueprinted>(entity).ok()?.0);
                    let intel = *self.ecs.get::<&Intel>(entity).ok()?;
                    (unit.cloak_upkeep.is_some() || self.cloak_upkeep(entity).is_some()).then_some((id, intel.cloak_on, intel.cloaked))
                })
                .collect()
        });
        let unpowered = viewer.map_or_else(Vec::new, |viewer| {
            own_intel(viewer)
                .filter(|&&(_, entity)| {
                    let has_upkeep = self.ecs.get::<&Blueprinted>(entity).is_ok_and(|unit| self.blueprints.get(unit.0).upkeep > Res::ZERO);
                    has_upkeep
                        && self.ecs.get::<&Intel>(entity).is_ok_and(|intel| !intel.running)
                        && self.ecs.get::<&Construction>(entity).is_err()
                        && self.ecs.get::<&Paused>(entity).is_err()
                })
                .map(|&(id, _)| id)
                .collect()
        });
        let factories = viewer.map_or_else(Vec::new, |viewer| {
            self.factories
                .iter()
                .filter_map(|&(id, entity)| {
                    let stats = self.ecs.get::<&UnitStats>(entity).expect("factories have stats");
                    if stats.team != viewer {
                        return None;
                    }
                    let factory = self.ecs.get::<&Factory>(entity).expect("factory list only holds factories");
                    Some(FactoryView {
                        id,
                        queue: factory.queue.iter().map(|blueprint| blueprint.0).collect(),
                        repeat: factory.repeat,
                        progress_permille: (factory.progress.to_bits() * 1000 / Res::ONE.to_bits()).clamp(0, 1000) as u16,
                        rally: factory.rally.map(|point| {
                            [point.x.to_bits(), point.y.to_bits(), self.terrain.height_at(point).to_bits()]
                        }),
                    })
                })
                .collect()
        });
        let plans = viewer.map_or_else(Vec::new, |viewer| self.plans_for(viewer));
        let work_links = self.work_link_views(seen);
        let build_queues = viewer.map_or_else(Vec::new, |viewer| self.build_queues_for(viewer));
        let order_stops = viewer.map_or_else(Vec::new, |viewer| self.order_stops_for(viewer));
        let rates = viewer.map_or_else(Vec::new, |viewer| self.rates_for(viewer));
        let spending = viewer.map_or_else(Vec::new, |viewer| self.spending_for(viewer));
        let idle_factories = viewer.map_or_else(Vec::new, |viewer| self.idle_factories(viewer));
        // The viewer's own economy: its team's first player's, the one a person plays.
        let economy = viewer.map_or_else(EconomyView::default, |viewer| self.economies[self.first_player(viewer).index()].view());
        let team_economies = viewer.is_none().then(|| {
            [Team::Blue, Team::Red].map(|team| EconomyView::team(&self.players_of(team).map(|player| self.economies[player.index()].view()).collect::<Vec<_>>()))
        });
        // A player's income and start are known to their own team, and to a watcher seeing all.
        let known = |team: Team| viewer.is_none_or(|viewer| viewer == team);
        let players = self
            .players
            .iter()
            .enumerate()
            .map(|(index, &team)| {
                let player = super::components::PlayerId(index as u8);
                let start = self.start_of(player);
                PlayerView {
                    team,
                    out: self.eliminated.get(index).copied().unwrap_or(false),
                    units: tallies[index].0,
                    army: tallies[index].1,
                    mass_income: known(team).then(|| self.economies[index].view().mass_income),
                    start: known(team).then(|| [start.x.to_bits(), start.y.to_bits()]),
                }
            })
            .collect();
        let fog = match viewer {
            Some(viewer) if self.config.fog_of_war => Some(Arc::new(self.visibility.fog_bytes(viewer))),
            _ => None,
        };
        let shown = |id: &SimId| units.binary_search_by_key(id, |unit| unit.id).is_ok();
        let factory_upgrading = self.factories.iter().filter(|&&(_, entity)| {
            let next = self.ecs.get::<&Factory>(entity).ok().and_then(|factory| factory.queue.first().copied());
            let upgrade = self.ecs.get::<&Blueprinted>(entity).ok().and_then(|unit| self.blueprints.get(unit.0).upgrades_to);
            let working = self.ecs.get::<&Construction>(entity).is_err() && self.ecs.get::<&Paused>(entity).is_err();
            working && next.is_some() && next == upgrade
        });
        let mut upgrading: Vec<SimId> = self.upgrading.iter().chain(&self.enhancing).chain(factory_upgrading).map(|unit| unit.0).filter(shown).collect();
        upgrading.sort_unstable();
        upgrading.dedup();
        let (stunned, tractor_beams, storms) = self.exotic_views(&units, &seen);
        // Only silos the viewer sees now: a remembered one shows as it was last seen.
        let mut silo_doors: Vec<(SimId, u16)> =
            self.silo_doors().into_iter().filter(|(id, _)| units.binary_search_by_key(id, |unit| unit.id).is_ok_and(|index| !units[index].remembered)).collect();
        silo_doors.sort_unstable();
        SimSnapshot {
            tick: self.current_tick,
            units,
            projectiles,
            landed,
            blasts,
            wrecks,
            shields,
            interceptions,
            decoys,
            stocks,
            silo_doors,
            cargo,
            adjacency,
            blips,
            contacts,
            sensor_ranges,
            cloaks,
            surfaced: self.surfaced.clone(),
            unpowered,
            upgrades,
            enhancements,
            enhancing,
            upgrading,
            stunned,
            tractor_beams,
            storms,
            teleports,
            self_destructs,
            factories,
            plans,
            work_links,
            build_queues,
            order_stops,
            rates,
            spending,
            idle_factories,
            orders_cancelled: viewer.map_or(0, |viewer| self.orders_cancelled[viewer.index()]),
            paused: viewer.map_or_else(Vec::new, |viewer| self.paused_for(viewer)),
            veterans: self
                .veterans
                .iter()
                .filter_map(|&(id, entity)| {
                    let position = self.ecs.get::<&Position>(entity).ok()?.0;
                    let team = self.ecs.get::<&UnitStats>(entity).ok()?.team;
                    let rank = self.ecs.get::<&Veterancy>(entity).ok()?.rank;
                    seen(team, position.x.to_bits(), position.y.to_bits()).then_some((id, rank))
                })
                .collect(),
            kills: {
                let mut kills: Vec<(SimId, u32)> = self
                    .ecs
                    .query::<(&SimId, &super::components::Kills, &UnitStats, &Position)>()
                    .iter()
                    .filter(|(_, (_, _, stats, position))| seen(stats.team, position.0.x.to_bits(), position.0.y.to_bits()))
                    .map(|(_, (id, kills, _, _))| (*id, kills.0))
                    .collect();
                kills.sort_unstable();
                kills
            },
            economy,
            team_economies,
            strategic_launches,
            detonations,
            players,
            fog,
            match_result: self.match_result,
        }
    }
}

impl SimWorld {
    /// The enemy units `viewer` has on radar or deep scan, or, under the water, on sonar, and
    /// that no stealth field, suppression, or cloak hides from it unless deep scan strips them,
    /// in `SimId` order: whether or not it can see them too. Its units can fire at them all.
    pub fn radar_contacts(&self, viewer: Team) -> Vec<SimId> {
        let visibility = &self.visibility;
        if !self.config.fog_of_war || (!visibility.has_any(Sense::Radar, viewer) && !visibility.has_any(Sense::DeepScan, viewer) && !visibility.has_any(Sense::Sonar, viewer)) {
            return Vec::new();
        }
        let enemy = viewer.enemy();
        let on_radar = |x: i32, y: i32| visibility.senses(Sense::Radar, viewer, x, y) || visibility.senses(Sense::DeepScan, viewer, x, y);
        // The enemy's working stealth fields.
        let mut fields: Vec<(i32, i32, i64)> = Vec::new();
        for &(_, entity) in &self.intel {
            let running = self.ecs.get::<&Intel>(entity).is_ok_and(|intel| intel.running) && self.ecs.get::<&Construction>(entity).is_err();
            if !running || self.ecs.get::<&UnitStats>(entity).map_or(true, |stats| stats.team != enemy) {
                continue;
            }
            let unit = self.blueprints.get(self.ecs.get::<&Blueprinted>(entity).expect("intel units have blueprints").0);
            if unit.stealth_field > Fp::ZERO {
                let at = self.ecs.get::<&Position>(entity).expect("units have positions").0;
                fields.push((at.x.to_bits(), at.y.to_bits(), i64::from(unit.stealth_field.to_bits())));
            }
        }
        let mut contacts = Vec::new();
        for &(id, entity) in &self.units {
            let Ok(stats) = self.ecs.get::<&UnitStats>(entity) else {
                continue;
            };
            if stats.team != enemy || self.concealed[viewer.index()].binary_search(&id).is_ok() {
                continue;
            }
            let at = self.ecs.get::<&Position>(entity).expect("units have positions").0;
            let (x, y) = (at.x.to_bits(), at.y.to_bits());
            // Radar does not reach under the water: a submarine shows only where sonar finds it.
            let submerged = (stats.move_class == MoveClass::Submarine && self.surfaced.binary_search(&id).is_err())
                || (stats.move_class == MoveClass::Seabed && at.z + stats.radius < self.terrain.water_level());
            let sensed = if submerged { visibility.senses(Sense::Sonar, viewer, x, y) } else { on_radar(x, y) };
            if !sensed {
                continue;
            }
            if !visibility.senses(Sense::DeepScan, viewer, x, y) {
                let stealthed = fields.iter().any(|&(fx, fy, radius)| {
                    let (dx, dy) = (i64::from(x - fx), i64::from(y - fy));
                    dx * dx + dy * dy <= radius * radius
                });
                let cloaked = self.intel.binary_search_by_key(&id, |unit| unit.0).is_ok() && self.ecs.get::<&Intel>(entity).is_ok_and(|intel| intel.cloaked || intel.stealthed);
                if stealthed || cloaked {
                    continue;
                }
            }
            contacts.push(id);
        }
        contacts
    }

    /// The enemy contacts `viewer` has on radar that it cannot see, as raw `Fp` positions with
    /// what each is, real and decoy alike: a real one names its unit; a decoy names none, which
    /// only the interface knows, and shows like any other.
    pub fn contacts_for(&self, viewer: Team) -> Vec<ContactView> {
        let visibility = &self.visibility;
        let mut contacts: Vec<ContactView> = Vec::new();
        for &id in &self.detected[viewer.index()] {
            let Some((index, entity)) = self.units.binary_search_by_key(&id, |unit| unit.0).ok().map(|index| (index, self.units[index].1)) else {
                continue;
            };
            let entry = self.spatial_grid.entry(index);
            if entry.id != id || visibility.is_visible(viewer, entry.x, entry.y) {
                continue;
            }
            // A structure the viewer has identified shows as it was last seen, not as a blip.
            let known = &self.known_structures[viewer.index()];
            if entry.is_structure && known.binary_search_by_key(&id, |record| record.id).is_ok_and(|index| known[index].blueprint.is_some()) {
                continue;
            }
            let _ = entity;
            contacts.push(ContactView { unit: Some(id), x: entry.x, y: entry.y, z: entry.z, is_air: entry.is_air, is_naval: entry.is_naval, is_sub: entry.is_sub, is_structure: entry.is_structure });
        }
        // Structures found only on radar stay where they were found, unidentified, once off it.
        // One gone since names no unit, as a decoy does: an attack on it finds nothing.
        for record in &self.known_structures[viewer.index()] {
            let on_radar_now = contacts.iter().any(|contact| contact.unit == Some(record.id));
            if record.blueprint.is_some() || on_radar_now || visibility.is_visible(viewer, record.x, record.y) {
                continue;
            }
            let unit = (!record.gone).then_some(record.id);
            contacts.push(ContactView { unit, x: record.x, y: record.y, z: record.z, is_air: false, is_naval: false, is_sub: false, is_structure: true });
        }
        // Decoys from the enemy's working jammers.
        let enemy = viewer.enemy();
        let on_radar = |x: i32, y: i32| visibility.senses(Sense::Radar, viewer, x, y) || visibility.senses(Sense::DeepScan, viewer, x, y);
        let mut jammers: Vec<(SimId, i32, i32, i32, u32)> = Vec::new();
        for &(id, entity) in &self.intel {
            let running = self.ecs.get::<&Intel>(entity).is_ok_and(|intel| intel.running) && self.ecs.get::<&Construction>(entity).is_err();
            if !running || self.ecs.get::<&UnitStats>(entity).map_or(true, |stats| stats.team != enemy) {
                continue;
            }
            let unit = self.blueprints.get(self.ecs.get::<&Blueprinted>(entity).expect("intel units have blueprints").0);
            let at = self.ecs.get::<&Position>(entity).expect("units have positions").0;
            if let Some(jammer) = unit.jammer {
                jammers.push((id, at.x.to_bits(), at.y.to_bits(), jammer.radius.to_num::<i32>(), jammer.decoys));
            }
            // A jammer enhancement, while it is paid for.
            if self.ecs.get::<&Intel>(entity).is_ok_and(|intel| intel.jamming) {
                if let Some(jammer) = self.enhancement_effects(entity).jammer {
                    jammers.push((id, at.x.to_bits(), at.y.to_bits(), jammer.radius.to_num::<i32>(), jammer.decoys));
                }
            }
        }
        let period = self.current_tick / DECOY_SHUFFLE_TICKS;
        for (id, jx, jy, radius, decoys) in jammers {
            for decoy in 0..decoys {
                let spot = decoy_spot(id, period, decoy, radius);
                let (x, y) = (jx + (spot.0 << FP_FRAC_BITS), jy + (spot.1 << FP_FRAC_BITS));
                let point = Vec2Fp::new(Fp::from_bits(x), Fp::from_bits(y));
                if !self.terrain.in_bounds(point) || !on_radar(x, y) || visibility.senses(Sense::DeepScan, viewer, x, y) || visibility.is_visible(viewer, x, y) {
                    continue;
                }
                let z = (self.terrain.height_at(point) + fp_int(8)).to_bits();
                contacts.push(ContactView { unit: None, x, y, z, is_air: false, is_naval: false, is_sub: false, is_structure: false });
            }
        }
        contacts
    }

    /// The radar contacts `viewer` cannot see, as raw `Fp` positions, real and decoy alike.
    pub fn blips_for(&self, viewer: Team) -> Vec<[i32; 3]> {
        self.contacts_for(viewer).into_iter().map(|contact| [contact.x, contact.y, contact.z]).collect()
    }
}

/// Where a jammer's decoy is for a while, as whole units from the jammer, within its radius.
/// From a hash, not the simulation's random numbers, which drawing them must never use up.
pub fn decoy_spot(jammer: SimId, period: u32, decoy: u32, radius: i32) -> (i32, i32) {
    let mut value = (u64::from(jammer.0) << 40) ^ (u64::from(period) << 8) ^ u64::from(decoy);
    // SplitMix64.
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 31;
    let span = (2 * radius + 1).max(1) as u64;
    let (mut dx, mut dy) = ((value % span) as i32 - radius, ((value >> 32) % span) as i32 - radius);
    // Pulled in from the corners, so that every decoy is within the radius.
    while i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy) > i64::from(radius) * i64::from(radius) {
        (dx, dy) = (dx / 2, dy / 2);
    }
    (dx, dy)
}

fn permille(value: i64, max: i64) -> u16 {
    if max <= 0 {
        return 0;
    }
    (value * 1000 / max).clamp(0, 1000) as u16
}

fn write_fp(hasher: &mut impl Hasher, value: Fp) {
    hasher.write(&value.to_bits().to_le_bytes());
}

fn write_vec3(hasher: &mut impl Hasher, value: Vec3Fp) {
    write_fp(hasher, value.x);
    write_fp(hasher, value.y);
    write_fp(hasher, value.z);
}
