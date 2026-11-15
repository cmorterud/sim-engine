use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;
use std::sync::Arc;
use twox_hash::XxHash64;

use super::blueprints::{BlueprintDb, Res, UnitClass};
use super::air::{bomb_lands_on, BOMB_GRAVITY};
use super::combat::{entry_hit, firing_solution, missile_velocity, probe_terrain, shot_is_clear, sphere_entry, GRAVITY};
use super::commands::SimCommand;
use super::components::{BuildTask, AntiAir, Artillery, BombBay, Falling, Flight, Homing,
    Blueprinted, Builder, Combat, Construction, Factory, Footprint, Health, MoveClass, Order, OrderKind, OrderQueue, Patrol,
    Intel, Launcher, Missile, Stunning, MissileDefense, Upgrading, Overcharged, OverchargeOrder, OverchargeWeapon, Paused, Position, Projectile, Shield, SimId, Team, Turning, Turret, UnitStats, Velocity, Veterancy,
    Wreck, WreckKind, rotate_toward, LeavingFactory, Transport,
};
use super::config::{SimConfig, TARGET_SEARCH_INTERVAL, TERRAIN_CELL_UNITS, WRECK_HEALTH_PERMILLE, WRECK_MASS_PERMILLE};
use super::flowfield::{nearest_passable_cell_for, FlowField, FlowSample};
use super::formation::{plan_formation, within, Travel};
use super::production::{Economy, WorkLink};
use super::spatial::{GridUnit, Layer, SpatialGrid};
use super::terrain::Terrain;
use super::visibility::{Sense, Visibility, VISION_REFRESH_TICKS};
use crate::math::fp::{fp_int, sqrt_wide, wide_mul, Fp, Vec2Fp, Vec3Fp, FP_FRAC_BITS};
use crate::math::rect::RectFp;
use crate::math::rng::SimRng;

/// How far from a goal, in unit radii, a moving unit can settle against a crowd.
const CROWD_SETTLE_RADII: i32 = 30;
/// The gap between formation spots, in halves of the largest unit's radius.
const FORMATION_SPACING_RADII: i32 = 5;
const SLOT_EXTRA_TICKS: i64 = 40;
/// How long a formation waits for members that have fallen behind, before going on without them.
const FORMATION_PATIENCE_TICKS: u32 = 100;
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

/// Settings for spawning a unit.
#[derive(Copy, Clone, Debug)]
pub struct UnitSpawn {
    pub position: Vec2Fp,
    pub team: Team,
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

/// An Overcharge shell that landed, where, and on whose dome if it struck one.
#[derive(Copy, Clone, Debug)]
struct OverchargeImpact {
    def: super::blueprints::OverchargeDef,
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
    /// A factory making something, or a builder at work, with the power to do it. For drawing.
    pub working: bool,
    /// What a factory is building, as a raw blueprint id, and how far along it is in
    /// thousandths, so that it shows in the bay. Anyone who can see the factory sees it.
    pub assembling: Option<(u16, u16)>,
    /// What a structure is upgrading into, as a raw blueprint id, and how far along it is in
    /// thousandths, so that it shows changing as it goes. A factory's upgrade is the head of its
    /// queue. Anyone who can see it sees it.
    pub upgrade: Option<(u16, u16)>,
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
    pub z: i32,
    pub radius: i32,
    pub mass: i32,
    pub energy: i32,
    pub kind: WreckKind,
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
    /// How far its blast reaches when it lands, as raw `Fp` bits: zero for a direct hit. For
    /// drawing an explosion of the right size.
    pub splash: i32,
}

/// What a projectile is, for drawing it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProjectileKind {
    Shell,
    /// Drawn larger and brighter.
    Overcharge,
    TacticalMissile,
    StrategicMissile,
}

/// A shield generator's dome, for drawing and for the command panel. Values are raw `Fp` bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ShieldView {
    /// The generator.
    pub id: SimId,
    pub team: Team,
    /// The middle of the dome, on the ground.
    pub center: [i32; 3],
    pub radius: i32,
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
    pub omni: i32,
}

/// How close, in units, an enemy must come to see a cloaked unit without omni.
pub const CLOAK_REVEAL_UNITS: i32 = 100;
/// How often decoy blips move, in ticks.
const DECOY_SHUFFLE_TICKS: u32 = 50;

/// A dome that can stop shells this tick.
#[derive(Copy, Clone, Debug)]
struct Dome {
    id: SimId,
    team: Team,
    center: [i32; 3],
    radius: i32,
}

/// A copy of the state the renderer needs from one tick, sorted by `SimId`.
#[derive(Clone, Debug, Default)]
pub struct SimSnapshot {
    pub tick: u32,
    pub units: Vec<UnitView>,
    pub projectiles: Vec<ProjectileView>,
    pub wrecks: Vec<WreckView>,
    /// The shields of generators the viewer can see.
    pub shields: Vec<ShieldView>,
    /// Missiles shot down this tick: from the defense that fired, to the missile.
    pub interceptions: Vec<[[i32; 3]; 2]>,
    /// Missiles built and waiting in the viewer's silos, by silo, sorted by `SimId`.
    pub stocks: Vec<(SimId, u32)>,
    /// How many units each of the viewer's transports carries, sorted by `SimId`. Everyone's,
    /// for a viewer who sees everything.
    pub cargo: Vec<(SimId, u32)>,
    /// The viewer's structures sharing adjacency bonuses, as the giver's and the receiver's
    /// centers, raw `Fp` bits with ground height. Everyone's, for a viewer who sees everything.
    pub adjacency: Vec<([i32; 3], [i32; 3])>,
    /// Enemy contacts on the viewer's radar that it cannot see, real and decoy alike, as raw
    /// `Fp` positions. Nothing says which is which.
    pub blips: Vec<[i32; 3]>,
    /// The viewer's working sensors, for drawing their coverage.
    pub sensor_ranges: Vec<SensorRangeView>,
    /// The viewer's units with a cloak: whether it is switched on, and whether it is hiding them.
    pub cloaks: Vec<(SimId, bool, bool)>,
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
    /// How many of the viewer's orders have been cancelled because they could not be reached.
    pub orders_cancelled: u32,
    /// The viewer's paused factories and sites, sorted by `SimId`.
    pub paused: Vec<SimId>,
    /// The rank of each veteran the viewer can see, sorted by `SimId`.
    pub veterans: Vec<(SimId, u8)>,
    pub economy: EconomyView,
    /// Both teams' economies, Blue's then Red's, for a watcher who sees everything: only in
    /// snapshots for no one side.
    pub team_economies: Option<[EconomyView; 2]>,
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

/// The highest veteran rank.
pub const MAX_RANK: u8 = 5;
/// Each rank adds this share of a unit's base health, so rank 5 doubles it.
pub(super) const RANK_HEALTH_PERMILLE: i64 = 200;
/// Reaching a rank heals this share of the new maximum health at once.
const PROMOTION_HEAL_PERMILLE: i64 = 250;
/// Each rank regenerates this share of maximum health per second, in tenths of a permille.
const REGEN_PER_RANK_PER_SECOND: i64 = 20;

struct PendingShot {
    source: SimId,
    team: Team,
    from: Vec3Fp,
    to: Vec3Fp,
    speed: Fp,
    damage: Fp,
    splash: Fp,
    /// For an Overcharge shell, its rule, worked out where it lands.
    overcharge: Option<super::blueprints::OverchargeDef>,
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
    pub config: SimConfig,
    pub current_tick: u32,
    pub next_sim_id: u32,
    /// How many units each team has had, which sets each new unit's `UnitStats::phase`.
    pub units_spawned: [u32; 2],
    /// Sorted by `SimId`: spawns append, despawns retain order. Any loop whose order
    /// can affect results iterates these, never a `hecs` query.
    pub units: Vec<(SimId, hecs::Entity)>,
    pub projectiles: Vec<(SimId, hecs::Entity)>,
    /// Box obstacles added to the terrain, for the renderer.
    pub obstacles: Vec<RectFp>,
    pub blueprints: Arc<BlueprintDb>,
    pub visibility: Visibility,
    pub economies: [Economy; 2],
    pub deposits: Vec<(u32, u32)>,
    /// Hydrocarbon vents, by cell (GAME_SPECIFICATION.md §3.4). A plant is built with one at the
    /// middle of its footprint.
    pub vents: Vec<(u32, u32)>,
    /// The structure on each occupied terrain cell, by cell index.
    pub occupancy: BTreeMap<u32, SimId>,
    pub wrecks: Vec<(SimId, hecs::Entity)>,
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
    /// Aircraft, and air staging facilities, sorted by `SimId`.
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
    pub air_stagings: Vec<(SimId, hecs::Entity)>,
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
    /// Missiles shot down this tick, from where to where. Only for drawing, so not in the checksum.
    interceptions: Vec<[[i32; 3]; 2]>,
    /// Groups on a formation move, by the route they share.
    pub formations: BTreeMap<u32, Travel>,
    pub commanders: [Option<SimId>; 2],
    pub match_result: Option<MatchResult>,
    /// Where each team's commander started, for the AI's plans and the camera. Fixed for the
    /// match, and set from the map, so it is not hashed: the map's terrain is.
    pub starts: [Vec2Fp; 2],
    pub(super) economy_dirty: bool,
    /// Who worked on what this tick. Rebuilt every tick and only used for drawing,
    /// so it is not part of the checksum.
    pub(super) work_links: Vec<WorkLink>,
    /// What each factory spent this tick. Rebuilt every tick and only used for showing,
    /// so it is not part of the checksum.
    pub(super) factory_spending: Vec<(SimId, Res, Res)>,
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
    pending_explosions: Vec<Explosion>,
    /// Shells that came down on a shield this tick, and what they do to it.
    pending_shield_hits: Vec<(SimId, Fp)>,
    /// Overcharge shells that landed this tick, to be worked out once the shells have all moved.
    pending_overcharges: Vec<OverchargeImpact>,
    /// Blasts and shells that land on the ground this tick, which also hit debris.
    pending_wreck_blasts: Vec<Explosion>,
    dead_units: Vec<SimId>,
    dead_projectiles: Vec<SimId>,
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
            rng,
            config,
            current_tick: 0,
            next_sim_id: 0,
            units_spawned: [0; 2],
            units: Vec::new(),
            projectiles: Vec::new(),
            obstacles: Vec::new(),
            blueprints: Arc::new(BlueprintDb::standard()),
            visibility,
            economies: [Economy::default(), Economy::default()],
            deposits: Vec::new(),
            vents: Vec::new(),
            occupancy: BTreeMap::new(),
            wrecks: Vec::new(),
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
            air_stagings: Vec::new(),
            disabled: Vec::new(),
            tractors: Vec::new(),
            storms: Vec::new(),
            pending_stuns: Vec::new(),
            pending_grabs: Vec::new(),
            captured_this_tick: Vec::new(),
            concealed: [Vec::new(), Vec::new()],
            interceptions: Vec::new(),
            formations: BTreeMap::new(),
            commanders: [None, None],
            starts: [Vec2Fp::ZERO; 2],
            match_result: None,
            economy_dirty: false,
            work_links: Vec::new(),
            factory_spending: Vec::new(),
            unreachable: Vec::new(),
            orders_cancelled: [0; 2],
            max_mobile_radius: 0,
            max_structure_radius: 0,
            pending_shots: Vec::new(),
            pending_hits: Vec::new(),
            pending_explosions: Vec::new(),
            pending_wreck_blasts: Vec::new(),
            pending_shield_hits: Vec::new(),
            pending_overcharges: Vec::new(),
            dead_units: Vec::new(),
            dead_projectiles: Vec::new(),
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
        let phase = self.units_spawned[spawn.team.index()];
        self.units_spawned[spawn.team.index()] += 1;
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
        self.resolve_overcharges();
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
                SimCommand::Overcharge { unit, target } => self.command_overcharge(*unit, *target),
                SimCommand::QueueMove { units, goal, attack } => self.queue_move(units, *goal, *attack, false),
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
            let (_, entity) = self.find_unit(id).expect("just checked");
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
                self.set_order(id, next);
                if finished {
                    self.ecs.remove_one::<OrderQueue>(entity).expect("just checked");
                    self.order_queues.remove(index);
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
            let _ = self.ecs.remove_one::<OverchargeOrder>(entity);
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

    fn command_overcharge(&mut self, unit: SimId, target: SimId) {
        let Some((_, entity)) = self.find_unit(unit) else {
            return;
        };
        let Some((_, target_entity)) = self.find_unit(target) else {
            return;
        };
        let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
        let is_enemy = self.ecs.get::<&UnitStats>(target_entity).is_ok_and(|stats| stats.team != team);
        if self.ecs.get::<&OverchargeWeapon>(entity).is_err() || !is_enemy {
            return;
        }
        self.clear_unit_plans(&[unit]);
        self.ecs.insert_one(entity, OverchargeOrder { target }).expect("unit is live");
        let position = self.unit_position(target).expect("just found").xy();
        self.assign_route(&[unit], OrderKind::Move, position, Some(target));
    }

    fn command_attack(&mut self, units: &[SimId], target: SimId) {
        let Some(position) = self.unit_position(target) else {
            return;
        };
        let (target_flies, target_dives, target_sails) = self.find_unit(target).map_or((false, false, false), |(_, entity)| {
            let flies = self.ecs.get::<&Flight>(entity).is_ok_and(|flight| flight.airborne());
            let class = self.ecs.get::<&UnitStats>(entity).map_or(MoveClass::Land, |stats| stats.move_class);
            (flies, class == MoveClass::Submarine, class == MoveClass::Naval)
        });
        // Only units with a weapon that can hit it: anti-air for an aircraft, a torpedo for a
        // submarine, and for anything else, a ground weapon, or a torpedo for a ship.
        let armed: Vec<SimId> = units
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
            .collect();
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
        for &id in units {
            if let Some((_, entity)) = self.find_unit(id) {
                if !seen.insert(id) {
                    continue;
                }
                members.push(id);
                let stats = self.ecs.get::<&UnitStats>(entity).expect("units have stats");
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
        let Some(route) = self.flow_field.create_route_for(
            class,
            &self.terrain,
            self.current_tick,
            goal,
            members.len() as u32,
            arrive_radius,
            follow,
        ) else {
            return Vec::new();
        };
        if let Some(travel) = keep_formation {
            self.formations.insert(route, travel);
        }
        slots.sort_unstable_by_key(|slot| slot.0);
        members
            .into_iter()
            .map(|id| {
                let (slot, slot_ticks) = slots
                    .binary_search_by_key(&id, |slot| slot.0)
                    .map_or((None, 0), |index| (Some(slots[index].1), slots[index].2));
                (id, Order { kind, route, slot, slot_ticks })
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
        let mut query = self.ecs.query::<(&Position, &UnitStats, &Order, Option<&Flight>)>();
        let view = query.view();
        self.spatial_grid.rebuild(self.units.iter().map(|&(id, entity)| {
            let (position, stats, order, flight) = view.get(entity).expect("unit list only holds live units");
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
                is_sub: stats.move_class == MoveClass::Submarine || (stats.move_class == MoveClass::Seabed && position.0.z + stats.radius < terrain.water_level()),
                is_naval: stats.move_class == MoveClass::Naval && !stats.is_structure,
                is_idle: order.kind == OrderKind::Idle,
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
        let mut unreachable = Vec::new();
        let mut chases = Vec::new();
        // Units that have driven out of their factory, with where they were sent.
        let mut left_factory = Vec::new();
        let mut query = self.ecs.query_mut::<(&mut Position, &mut Velocity, &UnitStats, &mut Order, &Combat, Option<&mut Turning>, Option<&Builder>, Option<&LeavingFactory>)>();
        let mut view = query.view();
        for (index, &(id, entity)) in units.iter().enumerate() {
            let (position, velocity, stats, order, combat, turning, builder, leaving) = view.get_mut(entity).expect("unit list only holds live units");
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
            // A submarine runs just under the water.
            let surface = |point: Vec2Fp| match class {
                MoveClass::Naval | MoveClass::Hover => terrain.height_at(point).max(terrain.water_level()),
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
                let shelling_ground = combat.ground_target.is_some_and(|spot| within(here, spot, combat.range));
                let may_walk = (order.kind == OrderKind::Move || combat.target.is_none()) && !shelling_ground;
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
                        } else {
                            match flow.sample(order.route, tick, here, terrain) {
                                FlowSample::Direction(direction) => desired = direction.scaled(stats.max_speed),
                                FlowSample::Unreachable => {
                                    flow.release(order.route);
                                    *order = Order::IDLE;
                                    unreachable.push(id);
                                }
                                FlowSample::Hold => {}
                            }
                        }
                    }
                } else if arrived {
                    if !flow.is_following(order.route) {
                        flow.release(order.route);
                        *order = Order::IDLE;
                    }
                } else if may_walk {
                    if goal_is_here {
                        desired = walk_to_goal(stats.max_speed);
                    } else {
                        match flow.sample(order.route, tick, here, terrain) {
                            FlowSample::Direction(direction) => desired = direction.scaled(stats.max_speed),
                            FlowSample::Unreachable => {
                                flow.release(order.route);
                                *order = Order::IDLE;
                                unreachable.push(id);
                            }
                            FlowSample::Hold => {}
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
            if touches_settled_friend
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

            let step_x = velocity.0.x.to_bits() + push_x;
            let step_y = velocity.0.y.to_bits() + push_y;
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

    /// Gives a unit the mass of an enemy it destroyed, and promotes it when it has earned a
    /// rank: one for each time its own mass cost. Units without a blueprint have no mass, so
    /// they earn nothing, and nobody earns anything for destroying their own side.
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
        let experience = before.experience + mass;
        let earned = (experience / unit.mass).to_num::<i64>().clamp(0, i64::from(MAX_RANK)) as u8;
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
            let heal = Fp::from_bits((i64::from(new_max.to_bits()) * PROMOTION_HEAL_PERMILLE / 1_000) as i32);
            health.max = new_max;
            health.current = (health.current + gained + heal).min(new_max);
        }
    }

    /// Veterans heal a little every tick, more for each rank.
    fn regenerate_veterans(&mut self) {
        for &(_, entity) in &self.veterans {
            let rank = self.ecs.get::<&Veterancy>(entity).map_or(0, |veterancy| veterancy.rank);
            let mut health = self.ecs.get::<&mut Health>(entity).expect("veterans are live units");
            if health.current <= Fp::ZERO {
                continue;
            }
            let per_tick = i64::from(health.max.to_bits()) * REGEN_PER_RANK_PER_SECOND * i64::from(rank) / 10_000 / 10;
            health.current = (health.current + Fp::from_bits(per_tick as i32)).min(health.max);
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
        }
    }

    /// Radar and omni sensors take their turns with sight, and cloaks are worked out every tick.
    /// A cloaked unit is hidden from a team unless that team has omni on it, or a unit of its
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
            let (radar, omni) = self.sensor_ranges(entity);
            let sonar = self.ecs.get::<&Blueprinted>(entity).map_or(Fp::ZERO, |unit| self.blueprints.get(unit.0).sonar);
            let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
            let at = self.ecs.get::<&Position>(entity).expect("units have positions").0;
            for (sense, range) in [(Sense::Radar, radar), (Sense::Omni, omni), (Sense::Sonar, sonar)] {
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
            let mut revealed = self.visibility.senses(Sense::Omni, enemy, x, y);
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
            let submerged = stats.move_class == MoveClass::Submarine || at.z + stats.radius < self.terrain.water_level();
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
    }

    pub fn step_combat(&mut self) {
        let tick = self.current_tick;
        let units = &self.units;
        let grid = &self.spatial_grid;
        let terrain = &self.terrain;
        let visibility = &self.visibility;
        let concealed = &self.concealed;
        let fog = self.config.fog_of_war;
        let shots = &mut self.pending_shots;
        let economies = &mut self.economies;
        let (max_mobile, max_structure) = (self.max_mobile_radius, self.max_structure_radius);
        shots.clear();
        let mut finished_attacks: Vec<SimId> = Vec::new();
        let mut finished_overcharges: Vec<SimId> = Vec::new();
        let mut fired_nukes: Vec<SimId> = Vec::new();
        let mut query = self.ecs.query_mut::<(
            &UnitStats,
            &mut Combat,
            Option<&mut Turret>,
            Option<&Turning>,
            Option<&mut OverchargeWeapon>,
            Option<&OverchargeOrder>,
            Option<&mut Launcher>,
            Option<&AntiAir>,
            Option<&BombBay>,
            Option<&Flight>,
            Option<&Artillery>,
            Option<&super::components::Adjacency>,
            Option<&mut super::components::ExtraWeapons>,
            Option<&super::components::Torpedo>,
            (Option<&super::components::TractorBeam>, Option<&super::components::StunWeapon>, &Order, Option<&Builder>),
        )>();
        let mut view = query.view();
        let disabled = &self.disabled;
        let grabs = &mut self.pending_grabs;
        grabs.clear();
        for (index, &(id, entity)) in units.iter().enumerate() {
            let (stats, combat, turret, turning, mut overcharge, overcharge_order, mut launcher, anti_air, bomb_bay, flight, artillery, adjacency, extras, torpedo, (tractor, stun_weapon, order, builder)) =
                view.get_mut(entity).expect("unit list only holds live units");
            combat.ticks_until_ready = combat.ticks_until_ready.saturating_sub(1);
            if let Some(weapon) = overcharge.as_mut() {
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
            let me = grid.entry(index);
            let range = i64::from(combat.range.to_bits());
            let squared_distance = |x: i32, y: i32| {
                let (dx, dy) = (i64::from(x - me.x), i64::from(y - me.y));
                dx * dx + dy * dy
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
            // A bomber lets its bombs go far short of its target, beyond its own sight from a
            // fast, high one such as the fighter-bomber, so it bombs what its team has on radar
            // or omni as well as what it sees. Seeing only what was in sight, it never let go.
            let sensed = |x: i32, y: i32| {
                !fog
                    || visibility.is_visible(stats.team, x, y)
                    || (bomb_bay.is_some() && (visibility.senses(super::visibility::Sense::Radar, stats.team, x, y) || visibility.senses(super::visibility::Sense::Omni, stats.team, x, y)))
            };
            let can_engage = |entry: &super::spatial::GridEntry| {
                let reach = range + i64::from(entry.radius);
                squared_distance(entry.x, entry.y) <= reach * reach
                    && far_enough(squared_distance(entry.x, entry.y))
                    && can_hit(entry)
                    && sensed(entry.x, entry.y)
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
            if ground.is_some() {
                combat.target = None;
            } else if let Some(forced) = combat.forced_target {
                match locate(forced) {
                    Some(entry) => combat.target = can_engage(entry).then_some(forced),
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
                    if !keep {
                        combat.target = None;
                    }
                }

                // Searches are spread across ticks. Rings grow outward, so units at the
                // front of a battle stop early with the nearest enemy. Mobile units come first.
                if combat.target.is_none() && picks_targets && stats.phase.wrapping_add(tick).is_multiple_of(TARGET_SEARCH_INTERVAL) {
                    let mut ring = (8i64 << FP_FRAC_BITS).max(range / 8);
                    loop {
                        let radius = ring.min(range);
                        let mut best: Option<(bool, i64, SimId)> = None;
                        let mut consider = |other: &super::spatial::GridEntry| {
                            let distance = squared_distance(other.x, other.y);
                            let reach = radius + i64::from(other.radius);
                            let visible = sensed(other.x, other.y) && !hidden(other.id);
                            if distance <= reach * reach && far_enough(distance) && visible && blocked != Some(other.id) && can_hit(other) {
                                let candidate = (other.is_structure, distance, other.id);
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

            // An Overcharge order takes over the gun, until it fires or its target is gone.
            let overcharge_target = overcharge_order.map(|order| order.target);
            if let Some(target) = overcharge_target {
                if locate(target).is_none() {
                    finished_overcharges.push(id);
                }
            }
            let aim_at = overcharge_target.filter(|target| locate(*target).is_some()).or(combat.target);
            let aim_point = match ground {
                Some(spot) => Some((spot.x.to_bits(), spot.y.to_bits())),
                None => aim_at.and_then(locate).map(|entry| (entry.x, entry.y)),
            };

            // A turret swivels to its target, or back to the front without one, and only fires once it points there.
            let aimed = match turret {
                None => true,
                Some(turret) => {
                    let toward = |(x, y): (i32, i32)| {
                        let away = Vec2Fp::new(Fp::from_bits(x - me.x), Fp::from_bits(y - me.y));
                        (away != Vec2Fp::ZERO).then(|| away.normalized())
                    };
                    match aim_point {
                        Some(point) => toward(point).is_none_or(|toward| turret.aim(toward)),
                        None => {
                            if let Some(front) = turning.map(|turning| turning.facing) {
                                turret.aim(front);
                            }
                            false
                        }
                    }
                }
            };
            // Overcharge fires in range, once aimed and reloaded, if the team has the energy stored.
            if let (Some(target), Some(weapon), true) = (overcharge_target, overcharge.as_mut(), can_fire) {
                let economy = &mut economies[stats.team.index()];
                if let Some(entry) = locate(target).filter(|entry| can_engage(entry)) {
                    // By FAF's rule, nothing is spent until the shell lands.
                    if aimed && weapon.ticks_until_ready == 0 && economy.energy >= weapon.def.energy_required {
                        let raw = |x: i32, y: i32, z: i32| Vec3Fp::new(Fp::from_bits(x), Fp::from_bits(y), Fp::from_bits(z));
                        shots.push(PendingShot {
                            source: id,
                            team: stats.team,
                            from: raw(me.x, me.y, me.z),
                            to: raw(entry.x, entry.y, entry.z),
                            speed: combat.projectile_speed,
                            damage: weapon.def.max_damage,
                            splash: Fp::ZERO,
                            overcharge: Some(weapon.def),
                            missile: None,
                            homing: None,
                            bomb: None,
                            spread: Fp::ZERO,
                            underwater: false,
                            stun: 0,
                        });
                        weapon.ticks_until_ready = weapon.def.reload_ticks;
                        finished_overcharges.push(id);
                    }
                }
            }
            // While aiming an Overcharge elsewhere, the gun does not fire at anything else.
            let gun_free = overcharge_target.is_none_or(|target| combat.target == Some(target));
            // A bomber's velocity, which its bombs keep as they fall.
            let speed = flight.map_or(stats.max_speed, |flight| flight.speed);
            let bomb = bomb_bay.and(turning).map(|turning| {
                let step = turning.facing.scaled(speed);
                Vec3Fp::new(step.x, step.y, Fp::ZERO)
            });
            // It bombs only from its flying height, not while taking off or landing.
            let at_height = flight.is_none_or(|flight| flight.altitude >= flight.elevation);
            let raw = |x: i32, y: i32, z: i32| Vec3Fp::new(Fp::from_bits(x), Fp::from_bits(y), Fp::from_bits(z));
            // A bomber lets go only when its bomb would land on the target.
            let bomb_on = |to: Vec3Fp, radius: i32| {
                let reach = (combat.projectile_splash + Fp::from_bits(radius)) / 2;
                at_height && turning.is_some_and(|turning| bomb_lands_on(raw(me.x, me.y, me.z), turning.facing, speed, to, reach))
            };
            // Artillery waits for the energy a shot needs, and reloads faster beside generators.
            let weapon_energy = artillery.map_or(Res::ZERO, |artillery| {
                super::components::Adjacency::less(artillery.energy_per_shot, adjacency.map_or(0, |adjacency| adjacency.weapon_energy_discount))
            });
            let charged = economies[stats.team.index()].energy >= weapon_energy;
            let reload = match (artillery, adjacency) {
                (Some(_), Some(adjacency)) => {
                    let ticks = i64::from(combat.reload_ticks) * (super::blueprints::ADJACENCY_SCALE - i64::from(adjacency.reload_discount)) / super::blueprints::ADJACENCY_SCALE;
                    ticks.max(1) as u32
                }
                _ => combat.reload_ticks,
            };
            let spread = artillery.map_or(Fp::ZERO, |artillery| artillery.spread);
            if combat.ticks_until_ready == 0 && aimed && gun_free && loaded && charged && can_fire {
                if let Some(target) = combat.target {
                    let entry = locate(target).expect("target was just validated or found");
                    let (from, to) = (raw(me.x, me.y, me.z), raw(entry.x, entry.y, entry.z));
                    let homing = (entry.is_air || torpedo).then_some(target);
                    if bomb.is_some() && !bomb_on(Vec3Fp::new(to.x, to.y, terrain.height_at(to.xy())), entry.radius) {
                        // Not over it yet.
                    // A target picked by the unit itself is passed over for a while if the ground
                    // would stop the shot. One the player ordered is fired at regardless.
                    // Missiles, bombs and shots at aircraft are never blocked.
                    } else if combat.forced_target.is_none() && missile.is_none() && homing.is_none() && bomb.is_none() && tractor.is_none() && !shot_is_clear(terrain, from, to, combat.projectile_speed) {
                        combat.blocked = Some((target, tick + BLOCKED_TARGET_TICKS));
                        combat.target = None;
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
                            overcharge: None,
                            missile,
                            homing,
                            bomb,
                            spread,
                            underwater: torpedo,
                            stun,
                        });
                        economies[stats.team.index()].energy -= weapon_energy;
                        combat.ticks_until_ready = reload;
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
                let spot_point = Vec3Fp::new(spot.x, spot.y, terrain.height_at(spot));
                let distance = squared_distance(spot.x.to_bits(), spot.y.to_bits());
                let in_reach = if bomb.is_some() { bomb_on(spot_point, 0) } else { distance <= range * range && far_enough(distance) };
                let charged = economies[stats.team.index()].energy >= weapon_energy;
                if combat.ticks_until_ready == 0 && aimed && loaded && in_reach && charged && can_fire {
                    let from = Vec3Fp::new(Fp::from_bits(me.x), Fp::from_bits(me.y), Fp::from_bits(me.z));
                    shots.push(PendingShot {
                        source: id,
                        team: stats.team,
                        from,
                        to: Vec3Fp::new(spot.x, spot.y, terrain.height_at(spot) - GROUND_AIM_DEPTH),
                        speed: combat.projectile_speed,
                        damage: combat.projectile_damage,
                        splash: combat.projectile_splash,
                        overcharge: None,
                        missile,
                        homing: None,
                        bomb,
                        spread,
                        underwater: false,
                        stun,
                    });
                    economies[stats.team.index()].energy -= weapon_energy;
                    combat.ticks_until_ready = reload;
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
                for weapon in &mut extras.0 {
                    weapon.ticks_until_ready = weapon.ticks_until_ready.saturating_sub(1);
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
                        squared_distance(entry.x, entry.y) <= within * within
                            && hit
                            && (!fog || visibility.is_visible(stats.team, entry.x, entry.y))
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
                    let aimed = match weapon.turret.as_mut() {
                        None => true,
                        Some(turret) => match aim_at {
                            Some((x, y)) => {
                                let away = Vec2Fp::new(Fp::from_bits(x - me.x), Fp::from_bits(y - me.y));
                                away == Vec2Fp::ZERO || turret.aim(away.normalized())
                            }
                            None => {
                                if let Some(front) = turning.map(|turning| turning.facing) {
                                    turret.aim(front);
                                }
                                false
                            }
                        },
                    };
                    if weapon.ticks_until_ready > 0 || !aimed || (me.is_sub && !weapon.torpedo && weapon.missile_height.is_none()) {
                        continue;
                    }
                    let from = raw(me.x, me.y, me.z);
                    let (to, homing) = match (spot, target) {
                        (Some(spot), _) => (Vec3Fp::new(spot.x, spot.y, terrain.height_at(spot) - GROUND_AIM_DEPTH), None),
                        (None, Some(entry)) => (raw(entry.x, entry.y, entry.z), (entry.is_air || weapon.torpedo).then_some(entry.id)),
                        (None, None) => continue,
                    };
                    // Missiles fly over the ground. A shell the ground would stop is not fired, and
                    // the weapon looks again.
                    let missile = weapon.missile_height.map(|cruise_height| MissileShot { cruise_height, strategic: false, inner_radius: Fp::ZERO, inner_damage: Fp::ZERO });
                    let homing = if missile.is_some() { None } else { homing };
                    if missile.is_none() && homing.is_none() && spot.is_none() && !shot_is_clear(terrain, from, to, weapon.projectile_speed) {
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
                        overcharge: None,
                        missile,
                        homing,
                        bomb: None,
                        spread: Fp::ZERO,
                        underwater: weapon.torpedo,
                        stun: weapon.stun_ticks,
                    });
                    weapon.ticks_until_ready = weapon.reload_ticks;
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
        for id in finished_overcharges {
            let (_, entity) = self.find_unit(id).expect("it just fired");
            let _ = self.ecs.remove_one::<OverchargeOrder>(entity);
            self.set_order(id, Order::IDLE);
        }
        let friendly_fire = self.config.friendly_fire;
        for shot in std::mem::take(&mut self.pending_shots) {
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
                let lifetime = (distance / shot.speed.max(Fp::ONE)).to_num::<u32>() * 3 + 10;
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
                self.ecs.insert_one(entity, Homing { target, speed: shot.speed, underwater: shot.underwater }).expect("just spawned");
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
            // An artillery shell lands somewhere within its spread of where it was aimed.
            let to = if shot.spread > Fp::ZERO {
                loop {
                    let (x, y) = (self.rng.range_fp(-shot.spread, shot.spread), self.rng.range_fp(-shot.spread, shot.spread));
                    if wide_mul(x, x) + wide_mul(y, y) <= wide_mul(shot.spread, shot.spread) {
                        break Vec3Fp::new(shot.to.x + x, shot.to.y + y, shot.to.z);
                    }
                }
            } else {
                shot.to
            };
            let solution = firing_solution(shot.from, to, shot.speed);
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
            if let Some(def) = shot.overcharge {
                let entity = self.projectiles.last().filter(|last| last.0 == projectile).expect("just spawned").1;
                self.ecs.insert_one(entity, Overcharged { def }).expect("just spawned");
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
                Some(Dome { id, team, center, radius: shield.radius.to_bits() })
            })
            .collect()
    }

    /// Missile defenses that are ready shoot down enemy missiles in range, before anything
    /// moves. A tactical defense destroys the nearest cruise missile, then reloads. A strategic
    /// one spends an interceptor on a nuclear missile that is in its range and heading for
    /// somewhere in its range. Defenses go in `SimId` order, and no missile is shot down twice.
    /// Returns the missiles destroyed, sorted by `SimId`.
    /// Anti-torpedo charges that are ready each destroy the nearest enemy torpedo within range,
    /// in `SimId` order, and no torpedo twice. Returns the torpedoes destroyed.
    fn intercept_torpedoes(&mut self) -> Vec<SimId> {
        // The missile defenses' step clears the last tick's interceptions, if there are any.
        if self.missile_defenses.is_empty() {
            self.interceptions.clear();
        }
        let mut destroyed: Vec<SimId> = Vec::new();
        for index in 0..self.torpedo_defenses.len() {
            let entity = self.torpedo_defenses[index].1;
            let mut defense = *self.ecs.get::<&super::components::TorpedoDefense>(entity).expect("defense list only holds defenses");
            defense.ticks_until_ready = defense.ticks_until_ready.saturating_sub(1);
            if defense.ticks_until_ready == 0 {
                let here = self.ecs.get::<&Position>(entity).expect("units have positions").0;
                let team = self.ecs.get::<&UnitStats>(entity).expect("units have stats").team;
                let range = i64::from(defense.range.to_bits());
                let mut best: Option<(i64, SimId, Vec3Fp)> = None;
                for &(id, torpedo) in &self.projectiles {
                    let underwater = self.ecs.get::<&Homing>(torpedo).is_ok_and(|homing| homing.underwater);
                    let enemy = self.ecs.get::<&Projectile>(torpedo).is_ok_and(|projectile| projectile.source_team != team);
                    if !underwater || !enemy || destroyed.contains(&id) {
                        continue;
                    }
                    let at = self.ecs.get::<&Position>(torpedo).expect("projectiles have positions").0;
                    let (dx, dy) = (i64::from(at.x.to_bits() - here.x.to_bits()), i64::from(at.y.to_bits() - here.y.to_bits()));
                    let distance = dx * dx + dy * dy;
                    if distance <= range * range && best.is_none_or(|(closest, _, _)| distance < closest) {
                        best = Some((distance, id, at));
                    }
                }
                if let Some((_, id, at)) = best {
                    destroyed.push(id);
                    self.interceptions.push([[here.x.to_bits(), here.y.to_bits(), here.z.to_bits()], [at.x.to_bits(), at.y.to_bits(), at.z.to_bits()]]);
                    defense.ticks_until_ready = defense.reload_ticks;
                }
            }
            *self.ecs.get::<&mut super::components::TorpedoDefense>(entity).expect("defense list only holds defenses") = defense;
        }
        destroyed
    }

    fn intercept_missiles(&mut self) -> Vec<SimId> {
        self.interceptions.clear();
        let mut destroyed: Vec<SimId> = Vec::new();
        for index in 0..self.missile_defenses.len() {
            let entity = self.missile_defenses[index].1;
            let mut defense = *self.ecs.get::<&MissileDefense>(entity).expect("defense list only holds defenses");
            defense.ticks_until_ready = defense.ticks_until_ready.saturating_sub(1);
            let working = self.ecs.get::<&Construction>(entity).is_err();
            let ready = if defense.strategic { defense.stock > 0 } else { defense.ticks_until_ready == 0 };
            if working && ready && !self.missiles.is_empty() {
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
                    if missile.strategic != defense.strategic || projectile.source_team == team || destroyed.contains(&id) {
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
                if let Some((_, id, at)) = best {
                    destroyed.push(id);
                    self.interceptions.push([[here.x.to_bits(), here.y.to_bits(), here.z.to_bits()], [at.x.to_bits(), at.y.to_bits(), at.z.to_bits()]]);
                    if defense.strategic {
                        defense.stock -= 1;
                    } else {
                        defense.ticks_until_ready = defense.reload_ticks;
                    }
                }
            }
            *self.ecs.get::<&mut MissileDefense>(entity).expect("defense list only holds defenses") = defense;
        }
        destroyed.sort_unstable();
        destroyed
    }

    pub fn step_projectiles(&mut self) {
        self.pending_shield_hits.clear();
        self.dead_projectiles.clear();
        let mut shot_down = if self.missile_defenses.is_empty() { Vec::new() } else { self.intercept_missiles() };
        if !self.torpedo_defenses.is_empty() {
            shot_down.extend(self.intercept_torpedoes());
            shot_down.sort_unstable();
            shot_down.dedup();
        }
        let domes = if self.shields.is_empty() { Vec::new() } else { self.standing_domes() };
        let shield_hits = &mut self.pending_shield_hits;
        let overcharges = &mut self.pending_overcharges;
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
        let mut query = self.ecs.query_mut::<(&mut Position, &mut Projectile, Option<&Overcharged>, Option<&Missile>, Option<&Homing>, Option<&Falling>, Option<&Stunning>)>();
        let mut view = query.view();
        for &(id, entity) in &self.projectiles {
            if shot_down.binary_search(&id).is_ok() {
                dead.push(id);
                continue;
            }
            let (position, projectile, overcharged, missile, homing, falling, stunning) = view.get_mut(entity).expect("projectile list only holds live projectiles");
            let stun = stunning.map_or(0, |stunning| stunning.0);
            let start = position.0;
            // A shot at an aircraft flies to it and hits only it. With the aircraft gone, it is spent.
            if let Some(homing) = homing {
                projectile.lifetime_ticks = projectile.lifetime_ticks.saturating_sub(1);
                let target = units.binary_search_by_key(&homing.target, |unit| unit.0).ok().map(|index| grid.entry(index));
                // Nor can it hit an aircraft that has landed.
                // A torpedo reaches only what is on or under the water.
                let Some(target) = target.filter(|target| if homing.underwater { target.is_sub || target.is_naval } else { target.is_air }) else {
                    dead.push(id);
                    continue;
                };
                let aim = Vec3Fp::new(Fp::from_bits(target.x), Fp::from_bits(target.y), Fp::from_bits(target.z));
                let distance = sqrt_wide(start.distance_squared(&aim));
                if distance <= homing.speed + Fp::from_bits(target.radius) {
                    position.0 = aim;
                    if projectile.splash_radius > Fp::ZERO {
                        // Flak bursts, and hits every enemy aircraft around it.
                        let reach = i64::from(projectile.splash_radius.to_bits());
                        let (x, y) = (target.x, target.y);
                        let damage = projectile.damage;
                        let source = projectile.source;
                        grid.for_each_candidate(Some(projectile.source_team.enemy()), Layer::Air, x, y, projectile.splash_radius.to_bits() + max_mobile, |other| {
                            let (dx, dy) = (i64::from(other.x - x), i64::from(other.y - y));
                            let within = reach + i64::from(other.radius);
                            if dx * dx + dy * dy <= within * within {
                                hits.push((other.entity, damage, Some(source)));
                            }
                        });
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
            } else if missile.is_none() {
                projectile.velocity.z -= GRAVITY;
            }
            let strategic = missile.is_some_and(|missile| missile.strategic);
            projectile.lifetime_ticks = projectile.lifetime_ticks.saturating_sub(1);
            let allies_are_safe = projectile.allied_safe_ticks > 0;
            projectile.allied_safe_ticks = projectile.allied_safe_ticks.saturating_sub(1);

            let probe = probe_terrain(terrain, start, step);

            // Shells far above the ground cannot touch a unit, which spares most grid queries.
            let lowest = start.z.min(end.z);
            let reach = Fp::from_bits(max_mobile.max(max_structure) * 2);
            let mut best: Option<(i32, SimId, hecs::Entity, bool)> = None;
            if lowest <= probe.highest_ground + reach {
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
                .filter_map(|(index, dome)| sphere_entry(start, step, dome.center, dome.radius).map(|along| (along, index)))
                .min();
            let first = |along: i32| best.is_none_or(|unit| along <= unit.0) && probe.hit.is_none_or(|ground| along <= ground);
            let point_at = |along: i32| {
                let part = |value: Fp| ((i64::from(value.to_bits()) * i64::from(along)) >> FP_FRAC_BITS) as i32;
                (start.x.to_bits() + part(step.x), start.y.to_bits() + part(step.y))
            };
            if let Some((along, struck)) = dome_hit.filter(|(along, _)| first(*along)) {
                if let Some(overcharged) = overcharged {
                    overcharges.push(OverchargeImpact { def: overcharged.def, source: projectile.source, team: projectile.source_team, point: point_at(along), dome: Some(domes[struck].id) });
                    dead.push(id);
                    continue;
                }
                let damage = projectile.damage;
                shield_hits.push((domes[struck].id, damage));
                if projectile.splash_radius > Fp::ZERO {
                    let part = |value: Fp| (i128::from(value.to_bits()) * i128::from(along)) >> FP_FRAC_BITS;
                    let point = [i128::from(start.x.to_bits()) + part(step.x), i128::from(start.y.to_bits()) + part(step.y), i128::from(start.z.to_bits()) + part(step.z)];
                    for (index, dome) in domes.iter().enumerate() {
                        let reach = i128::from(dome.radius) + i128::from(projectile.splash_radius.to_bits());
                        let (dx, dy, dz) = (point[0] - i128::from(dome.center[0]), point[1] - i128::from(dome.center[1]), point[2] - i128::from(dome.center[2]));
                        if index != struck && dome.team != projectile.source_team && dx * dx + dy * dy + dz * dz <= reach * reach {
                            shield_hits.push((dome.id, damage));
                        }
                    }
                }
                dead.push(id);
                continue;
            }

            // Splash shells detonate where they stop, and damage everything nearby.
            let splash = projectile.splash_radius > Fp::ZERO;
            let blast = Explosion { x: end.x.to_bits(), y: end.y.to_bits(), radius: projectile.splash_radius, damage: projectile.damage, source: Some(projectile.source), stun };
            // A nuclear blast also has an inner ring, that does far more.
            let core = missile.filter(|missile| missile.inner_radius > Fp::ZERO).map(|missile| Explosion { radius: missile.inner_radius, damage: missile.inner_damage, stun: 0, ..blast });

            match (best, probe.hit) {
                (Some((along, _, target, is_structure)), terrain_hit) if terrain_hit.is_none_or(|ground| along <= ground) => {
                    if let Some(overcharged) = overcharged {
                        overcharges.push(OverchargeImpact { def: overcharged.def, source: projectile.source, team: projectile.source_team, point: point_at(along), dome: None });
                    } else if splash {
                        explosions.extend([Some(blast), core].into_iter().flatten());
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
                    if let Some(overcharged) = overcharged {
                        overcharges.push(OverchargeImpact { def: overcharged.def, source: projectile.source, team: projectile.source_team, point: point_at(ground), dome: None });
                    } else if splash {
                        explosions.extend([Some(blast), core].into_iter().flatten());
                    } else {
                        // A shell that comes down on bare ground hits any debris lying there.
                        let along = |value: Fp| ((i64::from(value.to_bits()) * i64::from(ground)) >> FP_FRAC_BITS) as i32;
                        let (x, y) = (start.x.to_bits() + along(step.x), start.y.to_bits() + along(step.y));
                        wreck_blasts.push(Explosion { x, y, radius: Fp::ZERO, ..blast });
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

    /// Works out the Overcharge shells that landed this tick, by FAF's rule: each may spend a
    /// share of the energy in store, split between the team's shells in flight, at so much a
    /// point of damage, up to its most; of that, it spends only what destroys the toughest enemy
    /// unit where it lands, and never less than its least. Against a lone commander, a structure,
    /// or a structure's dome, it does the least, and against a mobile unit's dome, what brings
    /// the dome down. The energy goes when it lands. Its blast hurts only enemies, commanders and
    /// structures at most their own amounts.
    fn resolve_overcharges(&mut self) {
        if self.pending_overcharges.is_empty() {
            return;
        }
        for impact in std::mem::take(&mut self.pending_overcharges) {
            let def = impact.def;
            let team = impact.team;
            let in_flight = 1 + self
                .projectiles
                .iter()
                .filter(|&&(id, entity)| {
                    self.dead_projectiles.binary_search(&id).is_err()
                        && self.ecs.get::<&Overcharged>(entity).is_ok()
                        && self.ecs.get::<&Projectile>(entity).is_ok_and(|projectile| projectile.source_team == team)
                })
                .count() as i64;
            let stored = self.economies[team.index()].energy.max(Res::ZERO);
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
                    let commanders = self.commanders;
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
            self.economies[team.index()].energy -= drain;
            if let Some(owner) = impact.dome {
                let structure = self.find_unit(owner).is_some_and(|(_, owner)| self.ecs.get::<&UnitStats>(owner).is_ok_and(|stats| stats.is_structure));
                self.pending_shield_hits.push((owner, if structure { damage.min(def.structure_damage) } else { damage }));
                continue;
            }
            let reach = i64::from(def.splash_radius.to_bits());
            let (commanders, hits) = (self.commanders, &mut self.pending_hits);
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
                    let (id, position, health, blueprint, stats, construction) = self
                        .ecs
                        .query_one_mut::<(&SimId, &Position, &mut Health, Option<&Blueprinted>, &UnitStats, Option<&Construction>)>(target)
                        .expect("hit entities are live until the end of this step");
                    let was_alive = health.current > Fp::ZERO;
                    health.current = health.current.saturating_sub(damage);
                    if was_alive && health.current <= Fp::ZERO {
                        let (id, x, y, blueprint, team) =
                            (*id, position.0.x.to_bits(), position.0.y.to_bits(), blueprint.map(|b| b.0), stats.team);
                        let built = construction.map_or(Res::ONE, |construction| construction.progress);
                        self.dead_units.push(id);
                        if let Some(blueprint) = blueprint {
                            // The one who destroyed it earns its mass, or as much of it as had been built.
                            if let Some(killer) = source {
                                kills.push((killer, team, self.blueprints.get(blueprint).mass * built));
                            }
                            if let Some(blast) = self.blueprints.get(blueprint).death_explosion {
                                // A fabricator only blows up if it was running.
                                let running = construction.is_none()
                                    && self.ecs.get::<&Paused>(target).is_err()
                                    && self.economies[team.index()].extractor_efficiency > Res::ZERO;
                                if !blast.only_while_running || running {
                                    self.pending_explosions.push(Explosion { x, y, radius: blast.radius, damage: blast.damage, source: None, stun: 0 });
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
            self.air_stagings.retain(alive);
            self.disabled.retain(alive);
            self.tractors.retain(alive);
            self.dead_units.clear();
        }
        if !self.dead_projectiles.is_empty() {
            let dead = &self.dead_projectiles;
            let ecs = &mut self.ecs;
            // Both lists are sorted by SimId, so one forward pass finds every match.
            let mut next_dead = 0;
            self.projectiles.retain(|&(id, entity)| {
                if dead.get(next_dead) == Some(&id) {
                    next_dead += 1;
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
            if wreck.kind.is_scenery() {
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
    }

    /// A team whose commander died is eliminated, and everything it owns goes with it.
    fn resolve_match_end(&mut self) {
        if self.match_result.is_some() || self.dead_units.is_empty() {
            return;
        }
        let lost = |team: Team| self.commanders[team.index()].is_some_and(|commander| self.dead_units.contains(&commander));
        let (blue_lost, red_lost) = (lost(Team::Blue), lost(Team::Red));
        self.match_result = match (blue_lost, red_lost) {
            (true, true) => Some(MatchResult::Draw),
            (true, false) => Some(MatchResult::Winner(Team::Red)),
            (false, true) => Some(MatchResult::Winner(Team::Blue)),
            (false, false) => None,
        };
        for (team, eliminated) in [(Team::Blue, blue_lost), (Team::Red, red_lost)] {
            if !eliminated {
                continue;
            }
            for index in 0..self.units.len() {
                let (id, entity) = self.units[index];
                let on_team = self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == team);
                if on_team && !self.dead_units.contains(&id) {
                    self.dead_units.push(id);
                }
            }
        }
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
        if let Ok(footprint) = self.ecs.get::<&Footprint>(entity).map(|footprint| *footprint) {
            self.clear_footprint(footprint);
            self.economy_dirty = true;
        }
        // A commander or support commander takes its income and storage with it.
        if let Ok(blueprint) = self.ecs.get::<&Blueprinted>(entity) {
            let unit = self.blueprints.get(blueprint.0);
            if unit.mass_income > Res::ZERO || unit.energy_income > Res::ZERO || unit.mass_storage > Res::ZERO || unit.energy_storage > Res::ZERO {
                self.economy_dirty = true;
            }
        }
        // A unit that blew itself up leaves nothing to reclaim.
        let wreck = self.ecs.get::<&Blueprinted>(entity).ok().filter(|_| !self.blew_itself_up(entity)).map(|blueprint| {
            let built = self.ecs.get::<&Construction>(entity).map_or(Res::ONE, |construction| construction.progress);
            let unit = self.blueprints.get(blueprint.0);
            let health = Fp::from_num(Res::from_num(unit.health) * WRECK_HEALTH_PERMILLE / 1000 * built).max(Fp::from_bits(1));
            (unit.mass * WRECK_MASS_PERMILLE / 1000 * built, health)
        });
        self.ecs.despawn(entity).expect("dead units are live until now");
        if let Some((mass, health)) = wreck.filter(|(mass, _)| *mass > Res::ZERO) {
            let wreck_id = self.next_id();
            let wreck = self.ecs.spawn((wreck_id, Wreck { mass, energy: Res::ZERO, position: position.xy(), radius, kind: WreckKind::Debris, health }));
            self.wrecks.push((wreck_id, wreck));
        }
    }

    /// Computes a 64-bit checksum of the entire simulation state. Every value that can
    /// affect a future tick is hashed. `Hasher::write_i32` and friends use native
    /// endianness, so all values go through `to_le_bytes`.
    pub fn compute_checksum(&self) -> u64 {
        let mut hasher = XxHash64::with_seed(0);
        hasher.write(&self.current_tick.to_le_bytes());
        hasher.write(&self.next_sim_id.to_le_bytes());
        hasher.write(&self.rng.state().to_le_bytes());
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
        hasher.write(&[match self.match_result {
            None => 0,
            Some(MatchResult::Draw) => 1,
            Some(MatchResult::Winner(team)) => 2 + team as u8,
        }]);
        if self.config.fog_of_war {
            self.visibility.hash_state(&mut hasher);
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
        hasher.write(&[u8::from(combat.enabled), order.kind as u8]);
        hasher.write(&order.route.to_le_bytes());
        if let Some(slot) = order.slot {
            write_fp(hasher, slot.x);
            write_fp(hasher, slot.y);
        }
        hasher.write(&order.slot_ticks.to_le_bytes());
        drop(query);
        self.hash_unit_extras(entity, hasher);
    }

    fn hash_projectile(&self, entity: hecs::Entity, hasher: &mut impl Hasher) {
        let mut query = self
            .ecs
            .query_one::<(&Position, &Projectile, Option<&Overcharged>)>(entity)
            .expect("projectile list only holds live projectiles");
        let (position, projectile, overcharged) = query.get().expect("projectiles have every component");
        if let Some(overcharged) = overcharged {
            hasher.write(&[1]);
            write_fp(hasher, overcharged.def.structure_damage);
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
            }
        }
        if self.ecs.get::<&Falling>(entity).is_ok() {
            hasher.write(&[4]);
        }
        if let Ok(stunning) = self.ecs.get::<&Stunning>(entity) {
            hasher.write(&[5]);
            hasher.write(&stunning.0.to_le_bytes());
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
            .filter(|link| link.reclaim || self.find_unit(link.builder).and_then(|(_, entity)| self.ecs.get::<&UnitStats>(entity).ok().map(|stats| self.economies[stats.team.index()].satisfaction > Res::ZERO)).unwrap_or(false))
            .map(|link| link.builder)
            .collect();
        builders.sort_unstable();
        let mut unit_query = self.ecs.query::<(&Position, &UnitStats, &Health, Option<&Blueprinted>, Option<&Construction>, Option<&Turning>, Option<&Turret>, Option<&Factory>, Option<&Paused>)>();
        let unit_view = unit_query.view();
        let units: Vec<UnitView> = self
            .units
            .iter()
            .filter_map(|&(id, entity)| {
                let (position, stats, health, blueprint, construction, turning, turret, factory, paused) = unit_view.get(entity).expect("unit list only holds live units");
                let powered = self.economies[stats.team.index()].satisfaction > Res::ZERO;
                let producing = factory.is_some_and(|factory| !factory.queue.is_empty()) && paused.is_none() && powered;
                let (x, y, z) = (position.0.x.to_bits(), position.0.y.to_bits(), position.0.z.to_bits());
                let cloaked_away = viewer.is_some_and(|viewer| self.concealed[viewer.index()].binary_search(&id).is_ok());
                (seen(stats.team, x, y) && !cloaked_away).then(|| UnitView {
                    id,
                    team: stats.team,
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
                })
            })
            .collect();
        let mut projectile_query = self.ecs.query::<(&Position, &Projectile, Option<&Overcharged>, Option<&Missile>)>();
        let projectile_view = projectile_query.view();
        let projectiles = self
            .projectiles
            .iter()
            .filter_map(|&(id, entity)| {
                let (position, projectile, overcharged, missile) = projectile_view.get(entity).expect("projectile list only holds live projectiles");
                let (x, y, z) = (position.0.x.to_bits(), position.0.y.to_bits(), position.0.z.to_bits());
                let kind = match (overcharged, missile) {
                    (Some(_), _) => ProjectileKind::Overcharge,
                    (None, Some(missile)) if missile.strategic => ProjectileKind::StrategicMissile,
                    (None, Some(_)) => ProjectileKind::TacticalMissile,
                    (None, None) => ProjectileKind::Shell,
                };
                seen(projectile.source_team, x, y).then_some(ProjectileView { id, team: projectile.source_team, x, y, z, kind, source: projectile.source, splash: projectile.splash_radius.to_bits() })
            })
            .collect();
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
                    z: (self.terrain.height_at(wreck.position) + wreck.radius / 2).to_bits(),
                    radius: wreck.radius.to_bits(),
                    mass: wreck.mass.to_num::<i32>(),
                    energy: wreck.energy.to_num::<i32>(),
                    kind: wreck.kind,
                })
            })
            .collect();
        let shields = self
            .shields
            .iter()
            .filter_map(|&(id, entity)| {
                let shield = self.ecs.get::<&Shield>(entity).ok()?;
                let base = self.ecs.get::<&Position>(entity).ok()?.0.xy();
                let team = self.ecs.get::<&UnitStats>(entity).ok()?.team;
                let (x, y) = (base.x.to_bits(), base.y.to_bits());
                if !seen(team, x, y) {
                    return None;
                }
                let switched_on = self.ecs.get::<&Construction>(entity).is_err() && self.ecs.get::<&Paused>(entity).is_err();
                Some(ShieldView {
                    id,
                    team,
                    center: [x, y, self.terrain.height_at(base).to_bits()],
                    radius: shield.radius.to_bits(),
                    health_permille: permille(i64::from(shield.health.to_bits()), i64::from(shield.max_health.to_bits())),
                    up: switched_on && shield.is_up() && !self.is_disabled(id),
                    recharging: shield.recharging,
                })
            })
            .collect();
        let interceptions = self.interceptions.iter().copied().filter(|[from, to]| viewer.is_none_or(|viewer| seen(viewer.enemy(), from[0], from[1]) || seen(viewer.enemy(), to[0], to[1]))).collect();
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
        let blips = match viewer {
            Some(viewer) if self.config.fog_of_war => self.blips_for(viewer),
            _ => Vec::new(),
        };
        let own_intel = |viewer: Team| self.intel.iter().filter(move |&&(_, entity)| self.ecs.get::<&UnitStats>(entity).is_ok_and(|stats| stats.team == viewer));
        let sensor_ranges = viewer.map_or_else(Vec::new, |viewer| {
            own_intel(viewer)
                .filter(|&&(id, entity)| self.ecs.get::<&Intel>(entity).is_ok_and(|intel| intel.running) && self.ecs.get::<&Construction>(entity).is_err() && !self.is_disabled(id))
                .filter_map(|&(_, entity)| {
                    let (radar, omni) = self.sensor_ranges(entity);
                    let at = self.ecs.get::<&Position>(entity).ok()?.0;
                    (radar > Fp::ZERO || omni > Fp::ZERO).then(|| SensorRangeView { center: [at.x.to_bits(), at.y.to_bits()], radar: radar.to_bits(), omni: omni.to_bits() })
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
        let economy = viewer.map_or_else(EconomyView::default, |viewer| self.economies[viewer.index()].view());
        let team_economies = viewer.is_none().then(|| [self.economies[0].view(), self.economies[1].view()]);
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
        SimSnapshot {
            tick: self.current_tick,
            units,
            projectiles,
            wrecks,
            shields,
            interceptions,
            stocks,
            cargo,
            adjacency,
            blips,
            sensor_ranges,
            cloaks,
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
            economy,
            team_economies,
            fog,
            match_result: self.match_result,
        }
    }
}

impl SimWorld {
    /// What the viewer's radar shows that it cannot see: enemies on its radar or omni, except
    /// those hidden by a stealth field or a cloak where it has no omni, and decoys from enemy
    /// jammers, except where its omni or its sight shows there is nothing there. Each is its
    /// position's raw bits. The AI reads these too, so it can be fooled by decoys as a player can.
    pub fn blips_for(&self, viewer: Team) -> Vec<[i32; 3]> {
        let visibility = &self.visibility;
        if !visibility.has_any(Sense::Radar, viewer) && !visibility.has_any(Sense::Omni, viewer) && !visibility.has_any(Sense::Sonar, viewer) {
            return Vec::new();
        }
        let enemy = viewer.enemy();
        let on_radar = |x: i32, y: i32| visibility.senses(Sense::Radar, viewer, x, y) || visibility.senses(Sense::Omni, viewer, x, y);
        // The enemy's working stealth fields and jammers.
        let mut fields: Vec<(i32, i32, i64)> = Vec::new();
        let mut jammers: Vec<(SimId, i32, i32, i32, u32)> = Vec::new();
        for &(id, entity) in &self.intel {
            let running = self.ecs.get::<&Intel>(entity).is_ok_and(|intel| intel.running) && self.ecs.get::<&Construction>(entity).is_err();
            if !running || self.ecs.get::<&UnitStats>(entity).map_or(true, |stats| stats.team != enemy) {
                continue;
            }
            let unit = self.blueprints.get(self.ecs.get::<&Blueprinted>(entity).expect("intel units have blueprints").0);
            let at = self.ecs.get::<&Position>(entity).expect("units have positions").0;
            if unit.stealth_field > Fp::ZERO {
                fields.push((at.x.to_bits(), at.y.to_bits(), i64::from(unit.stealth_field.to_bits())));
            }
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
        let mut blips = Vec::new();
        for &(id, entity) in &self.units {
            let Ok(stats) = self.ecs.get::<&UnitStats>(entity) else {
                continue;
            };
            if stats.team != enemy {
                continue;
            }
            let at = self.ecs.get::<&Position>(entity).expect("units have positions").0;
            let (x, y) = (at.x.to_bits(), at.y.to_bits());
            let shown = visibility.is_visible(viewer, x, y) && self.concealed[viewer.index()].binary_search(&id).is_err();
            // Radar does not reach under the water: a submarine shows only where sonar finds it.
            let submerged = stats.move_class == MoveClass::Submarine || (stats.move_class == MoveClass::Seabed && at.z + stats.radius < self.terrain.water_level());
            let sensed = if submerged { visibility.senses(Sense::Sonar, viewer, x, y) } else { on_radar(x, y) };
            if shown || !sensed {
                continue;
            }
            if !visibility.senses(Sense::Omni, viewer, x, y) {
                let stealthed = fields.iter().any(|&(fx, fy, radius)| {
                    let (dx, dy) = (i64::from(x - fx), i64::from(y - fy));
                    dx * dx + dy * dy <= radius * radius
                });
                let cloaked = self.intel.binary_search_by_key(&id, |unit| unit.0).is_ok() && self.ecs.get::<&Intel>(entity).is_ok_and(|intel| intel.cloaked || intel.stealthed);
                if stealthed || cloaked {
                    continue;
                }
            }
            blips.push([x, y, at.z.to_bits()]);
        }
        let period = self.current_tick / DECOY_SHUFFLE_TICKS;
        for (id, jx, jy, radius, decoys) in jammers {
            for decoy in 0..decoys {
                let spot = decoy_spot(id, period, decoy, radius);
                let (x, y) = (jx + (spot.0 << FP_FRAC_BITS), jy + (spot.1 << FP_FRAC_BITS));
                let point = Vec2Fp::new(Fp::from_bits(x), Fp::from_bits(y));
                if !self.terrain.in_bounds(point) || !on_radar(x, y) || visibility.senses(Sense::Omni, viewer, x, y) || visibility.is_visible(viewer, x, y) {
                    continue;
                }
                blips.push([x, y, (self.terrain.height_at(point) + fp_int(8)).to_bits()]);
            }
        }
        blips
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
