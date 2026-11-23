//! GPU instance data. Interpolates between two snapshots and converts to camera-relative `f32`.

use std::collections::BTreeMap;
use std::f32::consts::FRAC_1_SQRT_2;

use super::meshes::Shape;
use crate::sim::blueprints::BlueprintDb;
use crate::sim::components::{SimId, Team, WreckKind};
use crate::sim::world::{ProjectileKind, ProjectileView, ShieldView, UnitView, WorkLinkView};

/// How long a gun's kick and flash last after it fires, in seconds.
const RECOIL_SECONDS: f64 = 0.3;
/// A stride's length, as a share of the unit's size, and how quickly legs settle when it stops.
const STRIDE_SHARE: f64 = 1.6;
const SETTLE_SHARE: f64 = 0.04;
/// How quickly working lights pulse, in pulses a second, and how far they dip.
const WORK_PULSES: f64 = 0.8;
const WORK_DIP: f64 = 0.25;

pub(super) const RAW_TO_WORLD: f64 = 1.0 / 4096.0;
/// A shell's tracer, in map units: as thick as this, and more for a shell with a blast, up to
/// `TRACER_MAX_WIDTH`, and `TRACER_LENGTH_SHARE` times as long, so that a shell can be followed
/// in flight close up. A medium tank is 18 across.
const TRACER_WIDTH: f32 = 1.2;
const TRACER_MAX_WIDTH: f32 = 3.0;
const TRACER_LENGTH_SHARE: f32 = 4.0;
/// A shell far off shows as a dot this many pixels across, as a glint.
const TRACER_ICON_PX: f32 = 1.5;

/// On-screen half-size of a unit's tactical dot, in pixels.
const UNIT_ICON_PX: f32 = 2.5;
const PROJECTILE_ICON_PX: f32 = 1.0;
/// Geometry with this icon size never collapses to a dot.
const NO_ICON: f32 = 0.0;

pub(super) const BLUE: [f32; 3] = [0.20, 0.50, 1.00];
/// How dark a remembered structure, out of sight, is drawn, and how much of its color goes grey.
const REMEMBERED_SHADE: f32 = 0.6;
const REMEMBERED_GREY: f32 = 0.5;

/// A color as a remembered structure shows it (`UnitView::remembered`): darker and greyer.
fn remembered_color(color: [f32; 3]) -> [f32; 3] {
    let grey = color[0] * 0.3 + color[1] * 0.59 + color[2] * 0.11;
    color.map(|channel| (channel + (grey - channel) * REMEMBERED_GREY) * REMEMBERED_SHADE)
}
pub(super) const RED: [f32; 3] = [1.00, 0.27, 0.22];
/// An ally's units, another player's on the same team, in a team match: a lighter shade of the
/// team's color, cyan beside Blue's blue and orange beside Red's red, so that the person's own
/// stand apart from their ally's.
pub(super) const BLUE_ALLY: [f32; 3] = [0.25, 0.85, 0.95];
pub(super) const RED_ALLY: [f32; 3] = [1.00, 0.62, 0.18];
const SELECTED: [f32; 3] = [0.25, 1.00, 0.35];
const SHELL: [f32; 3] = [1.00, 0.85, 0.35];
const DISCHARGE_SHELL: [f32; 3] = [0.75, 0.95, 1.00];
const DISCHARGE_SIZE: f32 = 2.4;
const MISSILE: [f32; 3] = [0.92, 0.92, 0.88];
const NUKE: [f32; 3] = [1.00, 0.95, 0.30];
const OBSTACLE: [f32; 3] = [0.42, 0.40, 0.38];
const WRECK: [f32; 3] = [0.24, 0.23, 0.22];
const TREE: [f32; 3] = [0.13, 0.36, 0.17];
const ROCK: [f32; 3] = [0.47, 0.44, 0.40];
const DEPOSIT: [f32; 3] = [0.10, 0.75, 0.62];
const VENT: [f32; 3] = [0.75, 0.55, 0.20];
const GHOST_VALID: [f32; 3] = [0.35, 0.95, 0.45];
const GHOST_INVALID: [f32; 3] = [0.95, 0.25, 0.20];
/// How much of its footprint a structure covers, either way. Walls cover all of theirs.
pub(super) const STRUCTURE_SHARE: f32 = 0.9;
/// How much wider a structure's foundation is than its footprint, and how thick it is, in
/// world units, half of it below the ground so that it sits flush on a slope.
const APRON_WIDTH: f32 = 1.1;
const APRON_HEIGHT: f32 = 2.4;
/// Bare armor, before it takes on a little of the team's and the blueprint's color.
const ARMOR: [f32; 3] = [0.50, 0.52, 0.55];
/// How much of the blueprint's tint, then the team's color, the armor takes on.
const ARMOR_TINT: f32 = 0.85;
const ARMOR_TEAM: f32 = 0.06;
/// How far glowing parts are lifted towards white.
const GLOW_WHITEN: f32 = 0.25;
/// How far a unit must have moved in a tick, in raw squared units, to count as heading somewhere.
const HEADING_MIN_MOVE_SQUARED: f64 = 400.0 * 400.0;
/// How quickly a unit turns towards its new heading, per frame.
const HEADING_BLEND: f32 = 0.2;
/// How far selection markers sit above the ground, so that gentle slopes do not hide them.
const SELECTION_LIFT: f64 = 1.5;
const SELECTION_THICKNESS: f32 = 1.0;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct UnitInstanceRaw {
    /// Interpolated, camera-relative 3D transform.
    pub model_matrix: [[f32; 4]; 4],
    /// The team's color, as RGB, with the tactical dot's half-size in pixels in the last component.
    pub color: [f32; 4],
    /// The body's color, for parts painted as hull, then the phase of moving parts, from 0 to
    /// 1: how far round spinning parts are, or how far through a stride legs are.
    pub hull: [u8; 4],
    /// The color of parts that glow, then the action, from 0 to 1: how hard guns are kicking
    /// back from a shot, or how brightly working lights shine.
    pub glow: [u8; 4],
}

fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A color from 0 to 1 as bytes, as the GPU reads them back, with nothing moving.
fn bytes(color: [f32; 3]) -> [u8; 4] {
    [byte(color[0]), byte(color[1]), byte(color[2]), 0]
}

fn mix(from: [f32; 3], to: [f32; 3], share: f32) -> [f32; 3] {
    std::array::from_fn(|channel| from[channel] + (to[channel] - from[channel]) * share)
}

/// The armor of a unit: grey, with a little of what it is and whose it is.
fn armor(team: [f32; 3], tint: Option<[f32; 3]>) -> [f32; 3] {
    mix(tint.map_or(ARMOR, |tint| mix(ARMOR, tint, ARMOR_TINT)), team, ARMOR_TEAM)
}

/// How much of its armor's brightness a unit keeps by tech level, from none to T3: higher
/// tiers are heavier and darker, so that a tier reads on the model as the dots read on its icon.
const TIER_SHADE: [f32; 4] = [1.0, 1.0, 0.88, 0.76];

/// The color of a unit's body: its armor, darkened by its tier.
fn hull_of(team: [f32; 3], look: &UnitLook) -> [f32; 3] {
    let shade = TIER_SHADE[usize::from(look.tier.min(3))];
    armor(team, look.tint).map(|channel| channel * shade)
}

/// A color at full brightness, lifted a little towards white, for parts lit from inside.
fn glowing(color: [f32; 3]) -> [f32; 3] {
    let brightest = color.iter().copied().fold(0.01_f32, f32::max);
    mix(color.map(|channel| channel / brightest), [1.0; 3], GLOW_WHITEN)
}

/// A unit's facing as a unit vector, which is where a shape's +Y axis points.
const NORTH: [f32; 2] = [0.0, 1.0];
/// What a factory is building faces one of the bay's open ends.
const EAST: [f32; 2] = [1.0, 0.0];

fn instance(center: [f64; 3], size: [f32; 3], heading: [f32; 2], color: [f32; 3], icon_px: f32, eye: [f64; 3]) -> UnitInstanceRaw {
    let [x, y] = heading;
    UnitInstanceRaw {
        model_matrix: [
            [y * size[0], -x * size[0], 0.0, 0.0],
            [x * size[1], y * size[1], 0.0, 0.0],
            [0.0, 0.0, size[2], 0.0],
            [(center[0] - eye[0]) as f32, (center[1] - eye[1]) as f32, (center[2] - eye[2]) as f32, 1.0],
        ],
        color: [color[0], color[1], color[2], icon_px],
        hull: bytes(color),
        glow: bytes(color),
    }
}

/// A unit's part, with its hull and glowing parts colored apart from its team color.
#[allow(clippy::too_many_arguments)]
pub(super) fn painted(center: [f64; 3], size: [f32; 3], heading: [f32; 2], trim: [f32; 3], hull: [f32; 3], glow: [f32; 3], icon_px: f32, eye: [f64; 3]) -> UnitInstanceRaw {
    UnitInstanceRaw { hull: bytes(hull), glow: bytes(glow), ..instance(center, size, heading, trim, icon_px, eye) }
}

/// Instances grouped by shape, ready for one draw per shape.
pub struct InstanceBatches {
    by_shape: [Vec<UnitInstanceRaw>; Shape::COUNT],
}

// Arrays only derive `Default` up to 32 long.
impl Default for InstanceBatches {
    fn default() -> Self {
        Self { by_shape: std::array::from_fn(|_| Vec::new()) }
    }
}

impl InstanceBatches {
    pub fn clear(&mut self) {
        for batch in &mut self.by_shape {
            batch.clear();
        }
    }

    pub fn push(&mut self, shape: Shape, instance: UnitInstanceRaw) {
        self.by_shape[shape as usize].push(instance);
    }

    pub fn shape(&self, shape: Shape) -> &[UnitInstanceRaw] {
        &self.by_shape[shape as usize]
    }

    pub fn total(&self) -> usize {
        self.by_shape.iter().map(Vec::len).sum()
    }
}

/// Which way units are facing. Kept between frames, so that a unit that stops keeps its facing.
pub type Headings = BTreeMap<SimId, [f32; 2]>;

/// A walker's stride: where it was last frame, and how far it has walked.
#[derive(Copy, Clone, Debug)]
struct Stride {
    last: [f64; 2],
    walked: f64,
}

/// A unit's latest shot: the newest shell it fired, and when it was seen.
#[derive(Copy, Clone, Debug)]
struct Shot {
    shell: SimId,
    at: f64,
}

/// How units move between frames, beyond where they are: facing, strides, and gunfire. Kept
/// between frames, and only ever read from the snapshots, never written back.
#[derive(Default)]
pub struct Motion {
    /// The simulation's time at the frame being drawn, in seconds. Spinning parts, pulsing
    /// lights, and recoil follow it, so that they stop while the game is paused.
    pub time: f64,
    headings: Headings,
    strides: BTreeMap<SimId, Stride>,
    shots: BTreeMap<SimId, Shot>,
    /// The ground, for standing structures level on a slope. Without it, a structure stands at
    /// the height of the ground at its middle.
    ground: Option<crate::render::camera::HeightField>,
}

impl Motion {
    pub fn new() -> Self {
        Self::default()
    }

    /// Gives it the map's ground, for standing structures level on slopes.
    pub fn set_ground(&mut self, ground: crate::render::camera::HeightField) {
        self.ground = Some(ground);
    }

    /// The lowest and highest ground under a square `half` either way of a point, at every
    /// terrain corner in it and at its own corners.
    fn ground_under(&self, x: f64, y: f64, half: f64) -> Option<(f64, f64)> {
        let ground = self.ground.as_ref()?;
        let step = ground.cell_units().max(1.0);
        let count = ((half * 2.0) / step).ceil() as i32;
        let (mut low, mut high) = (f64::MAX, f64::MIN);
        for row in 0..=count {
            for column in 0..=count {
                let at = |index: i32| (-half + f64::from(index) * step).min(half);
                let height = ground.height_at(x + at(column), y + at(row));
                (low, high) = (low.min(height), high.max(height));
            }
        }
        Some((low, high))
    }

    /// Forgets units for which `keep` is false.
    pub fn retain(&mut self, keep: impl Fn(&SimId) -> bool) {
        self.headings.retain(|id, _| keep(id));
        self.strides.retain(|id, _| keep(id));
        self.shots.retain(|id, _| keep(id));
    }

    /// Notes which units fired since the last snapshot: those whose shells are new in it.
    pub fn note_shots(&mut self, previous: &[ProjectileView], current: &[ProjectileView]) {
        let time = self.time;
        merge_by_id(previous, current, |projectile| projectile.id, |before, projectile| {
            if before.is_some() {
                return;
            }
            let shot = self.shots.entry(projectile.source).or_insert(Shot { shell: projectile.id, at: time });
            if projectile.id > shot.shell {
                *shot = Shot { shell: projectile.id, at: time };
            }
        });
    }

    /// How hard a unit's guns are kicking back, from 1 as it fires to 0.
    fn recoil(&self, id: SimId) -> f32 {
        self.shots.get(&id).map_or(0.0, |shot| (1.0 - (self.time - shot.at) / RECOIL_SECONDS).clamp(0.0, 1.0) as f32)
    }

    /// How far through a stride a walker is, from how far it has walked. Legs settle straight
    /// once it stops.
    fn stride(&mut self, id: SimId, at: [f64; 3], size: f32) -> f32 {
        let length = f64::from(size) * STRIDE_SHARE;
        let stride = self.strides.entry(id).or_insert(Stride { last: [at[0], at[1]], walked: 0.0 });
        let moved = (at[0] - stride.last[0]).hypot(at[1] - stride.last[1]);
        stride.last = [at[0], at[1]];
        if moved > length * 0.002 && moved < length * 4.0 {
            stride.walked += moved;
        } else {
            let half = length * 0.5;
            let rest = (stride.walked / half).round() * half;
            stride.walked += (rest - stride.walked).clamp(-length * SETTLE_SHARE, length * SETTLE_SHARE);
        }
        (stride.walked / length).rem_euclid(1.0) as f32
    }
}

fn interpolate(previous: [i32; 3], current: [i32; 3], alpha: f64) -> [f64; 3] {
    let lerp = |from: i32, to: i32| (f64::from(from) + (f64::from(to) - f64::from(from)) * alpha) * RAW_TO_WORLD;
    [lerp(previous[0], current[0]), lerp(previous[1], current[1]), lerp(previous[2], current[2])]
}

/// Walks two lists sorted by id, calling `visit` with each current item and its previous
/// position if it had one. New entities are drawn where they are, without interpolation.
pub(super) fn merge_by_id<T>(previous: &[T], current: &[T], id: impl Fn(&T) -> SimId, mut visit: impl FnMut(Option<&T>, &T)) {
    let mut cursor = 0;
    for item in current {
        while cursor < previous.len() && id(&previous[cursor]) < id(item) {
            cursor += 1;
        }
        let matched = previous.get(cursor).filter(|candidate| id(candidate) == id(item));
        visit(matched, item);
    }
}

/// How a blueprint is drawn.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct UnitLook {
    pub shape: Shape,
    /// Blended lightly into the team color, because the shape already shows what it is.
    pub tint: Option<[f32; 3]>,
    /// A structure's height, as a share of its width.
    pub height_share: f32,
    /// A lower part that faces the way the unit moves, under a `shape` that can face its work.
    pub base: Option<Shape>,
    /// How wide a mobile unit is drawn, in world units, for drawing it in a factory's bay
    /// before it exists. Zero from `unit_look`, which knows only the key; `unit_looks` sets it.
    pub size: f32,
    /// How long a ship, vehicle, or aircraft is drawn, in world units, where that is not its
    /// footprint: zero for its footprint. From above, units are told apart by their outline and
    /// length, so these keep apart some that share a footprint in the simulation.
    pub length: f32,
    /// Its tech level, which darkens its armor. Zero from `unit_look`; `unit_looks` sets it.
    pub tier: u8,
    /// Whether it stands on a concrete foundation: structures on land, but walls. False from
    /// `unit_look`; `unit_looks` sets it.
    pub apron: bool,
    /// Gun turrets drawn on their own, each turning to its weapon's aim.
    pub turrets: [Option<TurretMount>; 3],
}

/// A `Shape::NavalTurret` on a hull, which turns to its weapon's aim.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TurretMount {
    /// Which weapon it is: 0 for the main one, then the extra ones in order.
    pub weapon: u8,
    /// Where its foot stands in the hull's box, as `[right, forward, up]`. Across and along, it
    /// is the weapon's `mount` in the blueprint, as a share of the drawn size.
    pub at: [f32; 3],
    /// It faces the stern when it has nothing to aim at, as the weapon's turret does.
    pub aft: bool,
}

/// Maps a blueprint to its look. Unknown keys fall back to a plain cube.
pub fn unit_look(key: &str) -> UnitLook {
    let look = |shape, tint, height_share| UnitLook { shape, tint, height_share, base: None, size: 0.0, length: 0.0, tier: 0, apron: false, turrets: [None; 3] };
    let turret = |weapon, at, aft| Some(TurretMount { weapon, at, aft });
    match key {
        "command_rig" => UnitLook { base: Some(Shape::CommanderLegs), ..look(Shape::Commander, Some([1.00, 1.00, 1.00]), 1.0) },
        "field_rig" => UnitLook { base: Some(Shape::CommanderLegs), ..look(Shape::Commander, Some([0.70, 0.75, 0.85]), 1.0) },
        "rig_yard" => look(Shape::RigYard, Some([0.55, 0.60, 1.00]), 0.6),
        "geothermal_plant" => look(Shape::GeothermalPlant, Some([0.85, 0.60, 0.25]), 0.7),
        "mass_fabricator" => look(Shape::Fabricator, Some([0.40, 0.95, 0.85]), 1.0),
        "mass_fabricator_t3" => look(Shape::Fabricator, Some([0.70, 0.45, 1.00]), 0.8),
        "t2_engineer" => UnitLook { base: Some(Shape::EngineerHull), ..look(Shape::Engineer, Some([1.00, 0.56, 0.10]), 1.0) },
        "t3_engineer" => UnitLook { base: Some(Shape::EngineerHull), ..look(Shape::Engineer, Some([0.98, 0.44, 0.10]), 1.0) },
        "land_factory_t3" => look(Shape::FactoryT3, Some([0.10, 0.10, 0.20]), 0.85),
        "t1_engineer" => UnitLook { base: Some(Shape::EngineerHull), ..look(Shape::Engineer, Some([1.00, 0.70, 0.12]), 1.0) },
        "t1_light_walker" => UnitLook { base: Some(Shape::BotLegs), ..look(Shape::Bot, Some([0.78, 0.70, 0.50]), 1.0) },
        "t1_land_scout" => UnitLook { length: 14.0, ..look(Shape::LandScout, Some([0.75, 0.95, 1.00]), 1.0) },
        "t1_medium_tank" => UnitLook { base: Some(Shape::TankHull), ..look(Shape::Tank, Some([0.80, 0.85, 0.70]), 1.0) },
        "t2_grenadier_walker" => UnitLook { base: Some(Shape::BotLegs), ..look(Shape::GrenadierWalker, Some([0.72, 0.58, 0.40]), 1.0) },
        "t2_mobile_shield" => UnitLook { base: Some(Shape::ShieldCarrier), length: 15.0, ..look(Shape::ShieldEmitter, Some([0.55, 0.85, 1.00]), 1.0) },
        "t2_combat_engineer" => UnitLook { base: Some(Shape::CombatEngineerHull), length: 19.0, ..look(Shape::Engineer, Some([0.95, 0.32, 0.22]), 1.0) },
        "t3_heavy_walker" => UnitLook { base: Some(Shape::BotLegs), ..look(Shape::HeavyWalker, Some([0.58, 0.46, 0.36]), 1.0) },
        "t3_siege_walker" => UnitLook { base: Some(Shape::BotLegs), ..look(Shape::SiegeWalker, Some([0.70, 0.70, 0.75]), 1.0) },
        "t2_torpedo_boat" => UnitLook { length: 24.0, ..look(Shape::TorpedoBoat, Some([0.90, 0.78, 0.30]), 0.5) },
        "t3_heavy_point_defense" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::HeavyPointDefense, Some([0.95, 0.35, 0.30]), 1.0) },
        "t3_heavy_shield" => look(Shape::ShieldGenerator, Some([0.45, 0.70, 1.00]), 0.9),
        "wall" => look(Shape::Wall, Some([0.70, 0.70, 0.70]), 0.9),
        "field_workshop" => look(Shape::FieldWorkshop, Some([0.95, 0.75, 0.30]), 0.8),
        "t1_light_transport" => look(Shape::Transport, Some([0.85, 0.80, 0.60]), 1.0),
        "t2_transport" => look(Shape::Transport, Some([0.70, 0.75, 0.80]), 1.0),
        "t3_heavy_transport" => look(Shape::Transport, Some([0.55, 0.60, 0.75]), 1.0),
        "field_workshop_t3" => look(Shape::FieldWorkshop, Some([0.95, 0.55, 0.20]), 0.95),
        "t1_light_artillery" => UnitLook { base: Some(Shape::ArtilleryHull), length: 22.0, ..look(Shape::Artillery, Some([0.74, 0.66, 0.52]), 1.0) },
        "t2_heavy_tank" => UnitLook { base: Some(Shape::HeavyTankHull), length: 24.0, ..look(Shape::HeavyTank, Some([0.62, 0.52, 0.48]), 1.0) },
        "t2_assault_hovercraft" => UnitLook { base: Some(Shape::HoverHull), ..look(Shape::Tank, Some([0.45, 0.65, 0.75]), 1.0) },
        "land_factory_t1" => look(Shape::Factory, Some([0.35, 0.35, 0.45]), 0.55),
        "land_factory_t2" => look(Shape::FactoryT2, Some([0.20, 0.20, 0.30]), 0.7),
        "mass_extractor" => look(Shape::Extractor, Some([0.10, 0.95, 0.75]), 0.8),
        "mass_extractor_t2" => look(Shape::ExtractorT2, Some([0.10, 0.75, 0.95]), 1.0),
        "mass_extractor_t3" => look(Shape::ExtractorT3, Some([0.55, 0.40, 1.00]), 1.2),
        "power_generator" => look(Shape::Generator, Some([1.00, 0.95, 0.75]), 1.2),
        "mass_storage" => look(Shape::Storage, Some([0.60, 0.90, 0.60]), 0.65),
        "energy_storage" => look(Shape::EnergyStorage, Some([1.00, 0.90, 0.40]), 0.65),
        "t2_artillery" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::ArtilleryGun, Some([0.70, 0.62, 0.50]), 1.0) },
        "t3_artillery" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::ArtilleryGun, Some([0.46, 0.40, 0.34]), 0.6) },
        "t3_mobile_artillery" => UnitLook { base: Some(Shape::CasemateHull), length: 30.0, ..look(Shape::HeavyArtillery, Some([0.55, 0.45, 0.35]), 1.0) },
        "radar" => look(Shape::Radar, Some([0.40, 0.90, 0.55]), 1.3),
        "deep_scan_array" => look(Shape::DeepScan, Some([0.75, 0.55, 1.00]), 1.2),
        "stealth_field_generator" => look(Shape::StealthGenerator, Some([0.30, 0.30, 0.35]), 1.2),
        "shield_generator" => look(Shape::ShieldGenerator, Some([0.55, 0.85, 1.00]), 1.0),
        "tactical_missile_launcher" => look(Shape::MissileLauncher, Some([0.85, 0.85, 0.80]), 0.9),
        "tactical_missile_defense" => look(Shape::MissileDefense, Some([1.00, 0.55, 0.45]), 1.2),
        "strategic_missile_launcher" => look(Shape::NukeSilo, Some([1.00, 0.90, 0.30]), 0.5),
        "strategic_missile_defense" => look(Shape::StrategicInterceptorSilo, Some([0.95, 0.95, 0.95]), 0.7),
        "air_factory" => look(Shape::AirFactory, Some([0.45, 0.60, 0.80]), 0.5),
        "airfield" => look(Shape::Airfield, Some([0.70, 0.80, 0.90]), 0.5),
        "naval_factory" => look(Shape::NavalFactory, Some([0.35, 0.45, 0.55]), 0.5),
        "t1_frigate" => look(Shape::Frigate, Some([0.45, 0.62, 0.28]), 0.5),
        "t1_attack_sub" => look(Shape::Submarine, Some([0.10, 0.20, 0.30]), 0.3),
        "t2_escort_corvette" => UnitLook { length: 30.0, ..look(Shape::EscortCorvette, Some([0.55, 0.85, 1.00]), 0.5) },
        "t3_missile_sub" => UnitLook { length: 48.0, ..look(Shape::MissileSubmarine, Some([0.05, 0.10, 0.20]), 0.35) },
        "t2_mobile_missile_launcher" => UnitLook { base: Some(Shape::MissileTruck), length: 26.0, ..look(Shape::MissileRack, Some([0.80, 0.80, 0.70]), 1.0) },
        "t2_torpedo_bomber" => UnitLook { length: 26.0, ..look(Shape::TorpedoBomber, Some([0.40, 0.55, 0.70]), 0.55) },
        "t1_sonar" => look(Shape::Sonar, Some([0.30, 0.60, 0.90]), 1.0),
        "t2_sonar" => look(Shape::Sonar, Some([0.20, 0.45, 0.85]), 1.2),
        "t1_torpedo_launcher" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::SamLauncher, Some([0.20, 0.68, 0.56]), 0.7) },
        "t2_torpedo_launcher" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::SamLauncher, Some([0.14, 0.55, 0.44]), 0.8) },
        "t2_destroyer" => UnitLook { length: 46.0, ..look(Shape::Destroyer, Some([0.12, 0.30, 0.62]), 0.5) },
        "t2_cruiser" => look(Shape::Cruiser, Some([1.00, 1.00, 1.00]), 0.5),
        // Two turrets forward, the second firing over the first, and one aft.
        "t3_battleship" => UnitLook {
            turrets: [turret(0, [0.0, 0.304, -0.16], false), turret(1, [0.0, 0.109, -0.11], false), turret(2, [0.0, -0.337, -0.2], true)],
            ..look(Shape::Battleship, Some([0.04, 0.04, 0.07]), 0.6)
        },
        "t3_battlecruiser" => UnitLook { length: 86.0, ..look(Shape::Battlecruiser, Some([0.62, 0.56, 0.48]), 0.55) },
        "naval_factory_t2" => look(Shape::NavalFactoryT2, Some([0.25, 0.35, 0.45]), 0.6),
        "naval_factory_t3" => look(Shape::NavalFactoryT3, Some([0.15, 0.22, 0.32]), 0.7),
        "naval_factory_t2_support" => look(Shape::NavalFactoryT2, Some([0.64, 0.58, 0.44]), 0.55),
        "naval_factory_t3_support" => look(Shape::NavalFactoryT3, Some([0.50, 0.44, 0.32]), 0.65),
        "land_factory_t2_support" => look(Shape::FactoryT2, Some([0.62, 0.55, 0.40]), 0.62),
        "land_factory_t3_support" => look(Shape::FactoryT3, Some([0.48, 0.42, 0.30]), 0.78),
        "air_factory_t2_support" => look(Shape::AirFactoryT2, Some([0.66, 0.60, 0.46]), 0.58),
        "air_factory_t3_support" => look(Shape::AirFactoryT3, Some([0.52, 0.46, 0.34]), 0.72),
        "power_generator_t2" => look(Shape::GeneratorT2, Some([1.00, 0.88, 0.62]), 1.0),
        "power_generator_t3" => look(Shape::GeneratorT3, Some([1.00, 0.80, 0.50]), 0.9),
        "radar_t2" => look(Shape::Radar, Some([0.50, 0.95, 0.75]), 1.45),
        "t2_point_defense" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::PointDefense, Some([0.85, 0.30, 0.65]), 1.0) },
        "air_factory_t2" => look(Shape::AirFactoryT2, Some([0.30, 0.45, 0.65]), 0.65),
        "air_factory_t3" => look(Shape::AirFactoryT3, Some([0.15, 0.25, 0.45]), 0.8),
        "t2_gunship" => look(Shape::Gunship, Some([0.52, 0.66, 0.46]), 0.6),
        "t3_heavy_gunship" => look(Shape::HeavyGunship, Some([0.20, 0.20, 0.25]), 0.6),
        "t3_spy_plane" => look(Shape::Scout, Some([0.30, 0.30, 0.35]), 0.5),
        "t3_air_superiority" => UnitLook { length: 28.0, ..look(Shape::AirSuperiority, Some([0.95, 0.95, 1.00]), 0.5) },
        // The largest aircraft with wings, longer and wider than even the heavy transport.
        "t3_strategic_bomber" => UnitLook { length: 44.0, ..look(Shape::StrategicBomber, Some([0.50, 0.52, 0.38]), 0.5) },
        "t2_fighter_bomber" => look(Shape::FighterBomber, Some([0.85, 0.70, 0.55]), 0.55),
        "t2_flak" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::FlakGun, Some([0.35, 0.75, 0.95]), 1.0) },
        "t2_mobile_flak" => UnitLook { base: Some(Shape::TankHull), length: 21.0, ..look(Shape::FlakTurret, Some([0.22, 0.70, 0.98]), 1.0) },
        "t3_sam" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::SamLauncher, Some([0.15, 0.72, 1.00]), 1.1) },
        "t1_air_scout" => look(Shape::Scout, Some([0.85, 0.90, 0.95]), 0.5),
        "t1_interceptor" => look(Shape::Aircraft, Some([0.86, 0.88, 0.78]), 0.5),
        "t1_bomber" => look(Shape::Bomber, Some([0.82, 0.72, 0.52]), 0.6),
        "t1_aa_turret" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::AntiAirGun, Some([0.35, 0.75, 0.95]), 0.9) },
        "t1_mobile_aa" => UnitLook { base: Some(Shape::WheeledHull), length: 19.0, ..look(Shape::TruckAntiAir, Some([0.30, 0.78, 0.98]), 1.0) },
        "t1_point_defense" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::PointDefense, Some([0.75, 0.25, 0.85]), 0.9) },
        _ => look(Shape::Cube, None, 0.6),
    }
}

/// Per-blueprint looks, indexed by raw blueprint id.
pub fn unit_looks(blueprints: &BlueprintDb) -> Vec<UnitLook> {
    blueprints
        .iter()
        .map(|unit| {
            let look = unit_look(&unit.key);
            UnitLook {
                size: if look.length > 0.0 { look.length } else { unit.radius.to_num::<f32>() * 2.0 },
                tier: unit.tier,
                apron: unit.is_structure() && !unit.on_water && look.shape != Shape::Wall,
                ..look
            }
        })
        .collect()
}

/// Units with no heading yet face the middle of the map.
const fn default_heading(team: Team) -> [f32; 2] {
    match team {
        Team::Blue => [FRAC_1_SQRT_2, FRAC_1_SQRT_2],
        Team::Red => [-FRAC_1_SQRT_2, -FRAC_1_SQRT_2],
    }
}

/// Turns a heading a step towards a direction, given as a vector of any length.
fn turn_towards(heading: &mut [f32; 2], dx: f64, dy: f64) {
    // By angle, because blending two vectors never leaves a heading exactly opposite the target.
    let length = dx.hypot(dy);
    let target = [(dx / length) as f32, (dy / length) as f32];
    let cross = heading[0] * target[1] - heading[1] * target[0];
    let dot = heading[0] * target[0] + heading[1] * target[1];
    let (sin, cos) = (cross.atan2(dot) * HEADING_BLEND).sin_cos();
    let turned = [heading[0] * cos - heading[1] * sin, heading[0] * sin + heading[1] * cos];
    let size = turned[0].hypot(turned[1]);
    *heading = [turned[0] / size, turned[1] / size];
}

/// Turns a unit towards what it is working on, or else towards `rest` if given, or else the
/// way it moved this tick. A unit that stops keeps its facing.
fn update_heading(headings: &mut Headings, unit: &UnitView, before: Option<&UnitView>, work: Option<&WorkLinkView>, rest: Option<[f32; 2]>) -> [f32; 2] {
    let heading = headings.entry(unit.id).or_insert_with(|| default_heading(unit.team));
    if let Some(work) = work {
        let (dx, dy) = (f64::from(work.target[0] - unit.x), f64::from(work.target[1] - unit.y));
        if dx != 0.0 || dy != 0.0 {
            turn_towards(heading, dx, dy);
        }
    } else if let Some(rest) = rest {
        turn_towards(heading, f64::from(rest[0]), f64::from(rest[1]));
    } else if let Some(before) = before {
        let (dx, dy) = (f64::from(unit.x - before.x), f64::from(unit.y - before.y));
        if dx * dx + dy * dy > HEADING_MIN_MOVE_SQUARED {
            turn_towards(heading, dx, dy);
        }
    }
    *heading
}

/// A raw unit vector as one for drawing, or `None` for zero.
fn unit_vector(raw: [i32; 2]) -> Option<[f32; 2]> {
    if raw == [0, 0] {
        return None;
    }
    let (x, y) = (f64::from(raw[0]), f64::from(raw[1]));
    let length = x.hypot(y);
    Some([(x / length) as f32, (y / length) as f32])
}

/// The body's facing from the simulation, turned smoothly between ticks.
fn body_facing(unit: &UnitView, before: Option<&UnitView>, alpha: f64) -> Option<[f32; 2]> {
    turned(unit.facing, before.map(|before| before.facing), alpha)
}

/// A raw unit vector from the simulation, turned smoothly between ticks from `before`. Zero is
/// no direction.
fn turned(now: [i32; 2], before: Option<[i32; 2]>, alpha: f64) -> Option<[f32; 2]> {
    if now == [0, 0] {
        return None;
    }
    let from = before.filter(|before| *before != [0, 0]).unwrap_or(now);
    let (now, from) = ([f64::from(now[0]), f64::from(now[1])], [f64::from(from[0]), f64::from(from[1])]);
    let blended = [from[0] + (now[0] - from[0]) * alpha, from[1] + (now[1] - from[1]) * alpha];
    let length = blended[0].hypot(blended[1]);
    // Turning right round in one tick passes through nothing, so use where it ends up.
    let [x, y] = if length > 1.0 { blended } else { now };
    let length = x.hypot(y);
    Some([(x / length) as f32, (y / length) as f32])
}

/// Where each of a look's turrets is drawn on its hull, drawn at `center` as large as `extent`
/// and facing `body`: its middle, and which way it faces. `aim` gives a weapon's aim, if it has
/// one; a turret without rests facing the bow, or the stern.
fn mounted_turrets(look: &UnitLook, center: [f64; 3], extent: [f32; 3], body: [f32; 2], aim: impl Fn(u8) -> Option<[f32; 2]>) -> impl Iterator<Item = ([f64; 3], [f32; 2])> {
    look.turrets.into_iter().flatten().map(move |mount| {
        let [right, forward, up] = mount.at;
        // To the right of a hull facing (x, y) is (y, -x).
        let (across, along) = (f64::from(right * extent[0]), f64::from(forward * extent[1]));
        let (x, y) = (f64::from(body[0]), f64::from(body[1]));
        let at = [center[0] + across * y + along * x, center[1] - across * x + along * y, center[2] + f64::from(up * extent[2])];
        let rest = if mount.aft { [-body[0], -body[1]] } else { body };
        (at, aim(mount.weapon).unwrap_or(rest))
    })
}

/// Which way a unit's weapon aims, as the simulation has it: 0 for the main weapon, then its
/// extra ones. Zero for one that does not swivel.
fn weapon_aim(unit: &UnitView, weapon: u8) -> [i32; 2] {
    match weapon {
        0 => unit.aim,
        extra => unit.extra_aims.get(usize::from(extra) - 1).copied().unwrap_or([0, 0]),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_unit_instances(
    previous: &[UnitView],
    current: &[UnitView],
    alpha: f64,
    eye: [f64; 3],
    selected: &[SimId],
    looks: &[UnitLook],
    work: &[WorkLinkView],
    motion: &mut Motion,
    out: &mut InstanceBatches,
) {
    let time = motion.time;
    let mut next_selected = 0;
    let mut next_work = 0;
    merge_by_id(previous, current, |unit| unit.id, |before, unit| {
        let now = [unit.x, unit.y, unit.z];
        let from = before.map_or(now, |before| [before.x, before.y, before.z]);
        while next_selected < selected.len() && selected[next_selected] < unit.id {
            next_selected += 1;
        }
        let is_selected = selected.get(next_selected) == Some(&unit.id);
        while next_work < work.len() && work[next_work].builder < unit.id {
            next_work += 1;
        }
        let working_on = work.get(next_work).filter(|link| link.builder == unit.id);
        let color = match (unit.team, unit.ally) {
            (Team::Blue, false) => BLUE,
            (Team::Red, false) => RED,
            (Team::Blue, true) => BLUE_ALLY,
            (Team::Red, true) => RED_ALLY,
        };
        let look = looks.get(usize::from(unit.blueprint));
        let tint = look.and_then(|look| look.tint);
        let hull = look.map_or_else(|| armor(color, None), |look| hull_of(color, look));
        let glow = glowing(tint.unwrap_or(color));
        let shape = look.map_or(Shape::Cube, |look| look.shape);
        let size = (f64::from(unit.radius) * RAW_TO_WORLD * 2.0) as f32;
        let mut center = interpolate(from, now, alpha);
        // Guns kick back as they fire, and working lights pulse.
        let pulse = if unit.working { (1.0 - WORK_DIP * (0.5 + 0.5 * (time * WORK_PULSES * std::f64::consts::TAU).sin())) as f32 } else { 0.0 };
        let action = byte(motion.recoil(unit.id).max(pulse));
        // Spinning parts turn with time, each unit a little apart from the next.
        let spin = look.and_then(|look| look.shape.spin_rate()).map_or(0, |rate| byte(((time * f64::from(rate) + f64::from(unit.id.0) * 0.618) % 1.0) as f32));
        let animated = |mut instance: UnitInstanceRaw, phase: u8| {
            instance.hull[3] = phase;
            instance.glow[3] = action;
            instance
        };
        if is_selected {
            // Selection is shown on the ground, so that the unit keeps its own colors.
            let radius = f64::from(unit.radius) * RAW_TO_WORLD;
            let foot = [center[0], center[1], center[2] - radius + SELECTION_LIFT];
            let (marker, scale) = if unit.is_structure { (Shape::SelectionBox, 1.12) } else { (Shape::SelectionRing, 1.4) };
            let width = size * scale;
            out.push(marker, instance(foot, [width, width, SELECTION_THICKNESS], NORTH, SELECTED, NO_ICON, eye));
        }
        if unit.is_structure {
            // Drawn a little inside its footprint, on a foundation as wide as all of it, so that
            // a strip of floor shows between neighbors, where adjacency wiring runs. Walls fill
            // theirs, to join up.
            let footprint = size;
            // On a slope it stands level, on top of its foundation, which reaches from the highest
            // ground under it down below the lowest, rather than sinking into the hill on one side.
            let half = f64::from(footprint) * 0.5 * f64::from(APRON_WIDTH);
            let slab = motion.ground_under(center[0], center[1], half);
            if let Some((_, high)) = slab {
                let foot = center[2] - f64::from(unit.radius) * RAW_TO_WORLD;
                center[2] += (high - foot).max(0.0);
            }
            let size = if shape == Shape::Wall { footprint } else { footprint * STRUCTURE_SHARE };
            // A structure rises from the ground as it is built, and looks unfinished until then.
            let built = f32::from(unit.built_permille) / 1000.0;
            let height_share = look.map_or(0.6, |look| look.height_share);
            let height = size * height_share * (0.25 + 0.75 * built);
            center[2] += f64::from(height - footprint) * 0.5;
            let shade = 0.55 + 0.45 * built;
            let (trim, hull) = (color.map(|channel| channel * shade), hull.map(|channel| channel * shade));
            // One remembered out of sight is faded: darker, its team color half grey, and unlit.
            let (trim, hull) = if unit.remembered { (remembered_color(trim), remembered_color(hull)) } else { (trim, hull) };
            // Its foundation and scaffolding too.
            let color = if unit.remembered { remembered_color(color) } else { color };
            // Nothing glows until it is finished.
            let glow = if unit.built_permille >= 1000 && !unit.remembered { glow } else { hull };
            // A structure's base stays put, and its gun, if any, turns to aim.
            if let Some(base) = look.and_then(|look| look.base) {
                out.push(base, animated(painted(center, [size, size, height], NORTH, trim, hull, glow, UNIT_ICON_PX * 1.6, eye), 0));
            }
            let aim = unit_vector(unit.aim).unwrap_or(NORTH);
            out.push(shape, animated(painted(center, [size, size, height], aim, trim, hull, glow, UNIT_ICON_PX * 1.6, eye), spin));
            // The foundation it stands on, laid before it rises, a little wider than it: a level
            // slab from the highest ground under it to below the lowest.
            if look.is_some_and(|look| look.apron) {
                let ground = center[2] - f64::from(height) * 0.5;
                let (bottom, top) = slab.map_or((ground - f64::from(APRON_HEIGHT) * 0.5, ground + f64::from(APRON_HEIGHT) * 0.5), |(low, _)| (low - f64::from(APRON_HEIGHT) * 0.5, ground + f64::from(APRON_HEIGHT) * 0.5));
                let thick = (top - bottom) as f32;
                out.push(Shape::Apron, painted([center[0], center[1], (top + bottom) * 0.5], [footprint * APRON_WIDTH, footprint * APRON_WIDTH, thick], NORTH, color, hull, hull, NO_ICON, eye));
            }
            // Scaffolding stands around the whole of what it will be until it is finished.
            if unit.built_permille < 1000 {
                let full = size * height_share;
                let ground = center[2] - f64::from(height) * 0.5;
                let frame = [center[0], center[1], ground + f64::from(full) * 0.5];
                out.push(Shape::Scaffold, painted(frame, [size * 1.04, size * 1.04, full], NORTH, color, hull, glow, NO_ICON, eye));
            }
            // An upgrade shows the next tier going up through the structure as it progresses,
            // unlit, inside scaffolding as tall as it will be.
            if let (Some((into, permille)), true) = (unit.upgrade, unit.built_permille >= 1000) {
                if let Some(next) = looks.get(usize::from(into)) {
                    let progress = f32::from(permille) / 1000.0;
                    let ground = center[2] - f64::from(height) * 0.5;
                    let full = size * next.height_share;
                    let rising = full * (0.1 + 0.9 * progress);
                    let at = [center[0], center[1], ground + f64::from(rising) * 0.5];
                    let next_hull = hull_of(color, next).map(|channel| channel * (0.6 + 0.4 * progress));
                    let next_trim = color.map(|channel| channel * (0.6 + 0.4 * progress));
                    let grown = size * 1.02;
                    if let Some(base) = next.base {
                        out.push(base, painted(at, [grown, grown, rising], NORTH, next_trim, next_hull, next_hull, NO_ICON, eye));
                    }
                    out.push(next.shape, painted(at, [grown, grown, rising], NORTH, next_trim, next_hull, next_hull, NO_ICON, eye));
                    let frame = [center[0], center[1], ground + f64::from(full) * 0.5];
                    out.push(Shape::Scaffold, painted(frame, [size * 1.06, size * 1.06, full], NORTH, color, hull, hull, NO_ICON, eye));
                }
            }
            // What a factory is building stands in its bay, rising from the floor as it is built,
            // in its team's colors but unlit and dim, until it drives or flies out.
            if let (Some((item, permille)), Some(floor), true) = (unit.assembling, shape.bay_floor(), unit.built_permille >= 1000) {
                if let Some(made) = looks.get(usize::from(item)).filter(|made| made.size > 0.0) {
                    let ground = center[2] - f64::from(height) * 0.5;
                    let progress = f32::from(permille) / 1000.0;
                    let rise = made.size * (0.2 + 0.8 * progress);
                    let at = [center[0], center[1], ground + f64::from(height * floor + rise * 0.5)];
                    let dim = 0.55 + 0.45 * progress;
                    let made_trim = color.map(|channel| channel * dim);
                    let made_hull = hull_of(color, made).map(|channel| channel * dim);
                    let extent = [made.size, made.size, rise];
                    if let Some(base) = made.base {
                        out.push(base, painted(at, extent, EAST, made_trim, made_hull, made_hull, NO_ICON, eye));
                    }
                    out.push(made.shape, painted(at, extent, EAST, made_trim, made_hull, made_hull, NO_ICON, eye));
                    for (turret_at, facing) in mounted_turrets(made, at, extent, EAST, |_| None) {
                        out.push(Shape::NavalTurret, painted(turret_at, extent, facing, made_trim, made_hull, made_hull, NO_ICON, eye));
                    }
                }
            }
        } else {
            // The body faces the way the simulation says. Units that turn instantly, such as the
            // stress test's cubes, have no facing, so they are turned here as before.
            let (body, upper) = match body_facing(unit, before, alpha) {
                Some(body) => {
                    // Work comes first, then the weapon's aim, and otherwise straight ahead.
                    let rest = unit_vector(unit.aim).unwrap_or(body);
                    let upper = if look.is_some_and(|look| look.base.is_some()) { update_heading(&mut motion.headings, unit, before, working_on, Some(rest)) } else { body };
                    (body, upper)
                }
                None => {
                    // The stress test's plain cubes have no front, and skip this to save time.
                    let heading = if look.is_some() { update_heading(&mut motion.headings, unit, before, working_on, None) } else { NORTH };
                    (heading, heading)
                }
            };
            // A ship, vehicle, or aircraft may be drawn longer or shorter than its footprint.
            let drawn = look.map(|look| look.length).filter(|&length| length > 0.0).unwrap_or(size);
            if let Some(base) = look.and_then(|look| look.base) {
                let stride = if base.walks() { byte(motion.stride(unit.id, center, drawn)) } else { 0 };
                out.push(base, animated(painted(center, [drawn; 3], body, color, hull, glow, UNIT_ICON_PX, eye), stride));
            }
            out.push(shape, animated(painted(center, [drawn; 3], upper, color, hull, glow, UNIT_ICON_PX, eye), spin));
            if let Some(look) = look {
                let aim = |weapon| turned(weapon_aim(unit, weapon), before.map(|before| weapon_aim(before, weapon)), alpha);
                for (at, facing) in mounted_turrets(look, center, [drawn; 3], body, aim) {
                    out.push(Shape::NavalTurret, animated(painted(at, [drawn; 3], facing, color, hull, glow, UNIT_ICON_PX, eye), 0));
                }
            }
        }
    });
}

/// The way a projectile flies, from one tick's position to the next, as a unit vector. One just
/// fired, with no position before, points north.
fn flight(from: [i32; 3], to: [i32; 3]) -> [f32; 3] {
    let delta = [f64::from(to[0] - from[0]), f64::from(to[1] - from[1]), f64::from(to[2] - from[2])];
    let length = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
    if length > 0.0 { delta.map(|axis| (axis / length) as f32) } else { [0.0, 1.0, 0.0] }
}

/// A shell's tracer, `length` along `direction` and `width` across. The shell's mesh lies along x.
fn tracer_instance(center: [f64; 3], direction: [f32; 3], length: f32, width: f32, color: [f32; 3], icon_px: f32, eye: [f64; 3]) -> UnitInstanceRaw {
    let along = glam::Vec3::from(direction);
    // Across: level, unless it flies straight up or down.
    let across = along.cross(glam::Vec3::Z).try_normalize().unwrap_or(glam::Vec3::X);
    let up = across.cross(along);
    let mut tracer = instance(center, [1.0; 3], NORTH, color, icon_px, eye);
    tracer.model_matrix[0] = [along.x * length, along.y * length, along.z * length, 0.0];
    tracer.model_matrix[1] = [across.x * width, across.y * width, across.z * width, 0.0];
    tracer.model_matrix[2] = [up.x * width, up.y * width, up.z * width, 0.0];
    tracer
}

/// Draws the projectiles in flight, and those that struck since the last tick (`landed`) on the
/// way from where they were to where they struck, so that a shell is seen to reach its target.
pub fn build_projectile_instances(previous: &[ProjectileView], current: &[ProjectileView], landed: &[ProjectileView], alpha: f64, eye: [f64; 3], out: &mut InstanceBatches) {
    let mut draw = |before: Option<&ProjectileView>, projectile: &ProjectileView| {
        let now = [projectile.x, projectile.y, projectile.z];
        // One just fired is drawn leaving its gun.
        let from = before.map_or(projectile.from.unwrap_or(now), |before| [before.x, before.y, before.z]);
        let center = interpolate(from, now, alpha);
        // A missile points the way it is flying along the ground.
        let heading = {
            let (dx, dy) = (f64::from(now[0] - from[0]), f64::from(now[1] - from[1]));
            let length = dx.hypot(dy);
            if length > 0.0 { [(dx / length) as f32, (dy / length) as f32] } else { NORTH }
        };
        match projectile.kind {
            ProjectileKind::Shell => {
                let splash = (f64::from(projectile.splash) * RAW_TO_WORLD) as f32;
                let width = (TRACER_WIDTH + splash * 0.08).min(TRACER_MAX_WIDTH);
                out.push(Shape::Shell, tracer_instance(center, flight(from, now), width * TRACER_LENGTH_SHARE, width, SHELL, TRACER_ICON_PX, eye));
            }
            ProjectileKind::Discharge => out.push(Shape::Shell, tracer_instance(center, flight(from, now), DISCHARGE_SIZE * 3.0, DISCHARGE_SIZE, DISCHARGE_SHELL, UNIT_ICON_PX, eye)),
            ProjectileKind::TacticalMissile => out.push(Shape::Missile, instance(center, [3.0, 9.0, 3.0], heading, MISSILE, UNIT_ICON_PX, eye)),
            ProjectileKind::StrategicMissile => out.push(Shape::Missile, instance(center, [7.0, 20.0, 7.0], heading, NUKE, UNIT_ICON_PX * 1.6, eye)),
        }
    };
    merge_by_id(previous, current, |projectile| projectile.id, &mut draw);
    // One that struck is drawn only on its way there, from where the last tick showed it.
    merge_by_id(previous, landed, |projectile| projectile.id, |before, projectile| {
        if before.is_some() && alpha < 1.0 {
            draw(before, projectile);
        }
    });
}

/// A box obstacle as an instance. Obstacles keep their true size at every zoom.
pub fn obstacle_instance(min: [f64; 2], max: [f64; 2], base: f64, height: f64, eye: [f64; 3]) -> UnitInstanceRaw {
    let center = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5, base + height * 0.5];
    let size = [(max[0] - min[0]) as f32, (max[1] - min[1]) as f32, height as f32];
    instance(center, size, NORTH, OBSTACLE, NO_ICON, eye)
}

/// A wreck: low, dark debris on the ground. Use with `Shape::Wreck`.
pub fn wreck_instance(center: [f64; 3], radius: f64, eye: [f64; 3]) -> UnitInstanceRaw {
    let width = (radius * 1.6) as f32;
    instance(center, [width, width, width * 0.5], NORTH, WRECK, PROJECTILE_ICON_PX, eye)
}

/// A wreck drawn as the burnt-out shell of the unit it was, `look`, facing as it fell: its own
/// models, charred, squashed down, and sunk a little into the ground, with nothing lit.
pub fn wreck_model_instances(center: [f64; 3], radius: f64, look: &UnitLook, facing: [f32; 2], eye: [f64; 3], out: &mut InstanceBatches) {
    let size = if look.size > 0.0 { look.size } else { (radius * 2.0) as f32 };
    let height = size * look.height_share.min(1.0) * WRECK_SQUASH;
    let ground = center[2] - radius * 0.5;
    let at = [center[0], center[1], ground + f64::from(height) * 0.35];
    let (char_dark, char_light) = (WRECK, WRECK.map(|channel| channel * 1.35));
    for shape in look.base.into_iter().chain([look.shape]) {
        out.push(shape, painted(at, [size, size, height], facing, char_dark, char_light, char_dark, PROJECTILE_ICON_PX, eye));
    }
    for (turret_at, turret_facing) in mounted_turrets(look, at, [size, size, height], facing, |_| None) {
        out.push(Shape::NavalTurret, painted(turret_at, [size, size, height], turret_facing, char_dark, char_light, char_dark, PROJECTILE_ICON_PX, eye));
    }
}

/// How far a wreck is squashed down from the height of what it was.
const WRECK_SQUASH: f32 = 0.55;

/// A tree or rock, standing on the ground under `center`, which is where wrecks are reported:
/// half their radius up. They keep their true size at every zoom, like the terrain.
pub fn scenery_instance(kind: WreckKind, center: [f64; 3], radius: f64, eye: [f64; 3]) -> (Shape, UnitInstanceRaw) {
    let ground = center[2] - radius * 0.5;
    let (shape, width, height, color) = match kind {
        WreckKind::Tree => (Shape::Tree, radius * 2.4, radius * 4.5, TREE),
        _ => (Shape::Rock, radius * 2.0, radius * 1.1, ROCK),
    };
    let at = [center[0], center[1], ground + height * 0.5];
    (shape, instance(at, [width as f32, width as f32, height as f32], NORTH, color, NO_ICON, eye))
}

/// A shield's dome over its generator. The last component of the color is how opaque it is,
/// which fades as the shield wears down. Use with `Shape::Dome`, in its own see-through pass.
pub fn dome_instance(shield: &ShieldView, eye: [f64; 3]) -> UnitInstanceRaw {
    let radius = f64::from(shield.radius) * RAW_TO_WORLD;
    let ground = f64::from(shield.center[2]) * RAW_TO_WORLD;
    let center = [f64::from(shield.center[0]) * RAW_TO_WORLD, f64::from(shield.center[1]) * RAW_TO_WORLD, ground + radius * 0.5];
    let team = match shield.team {
        Team::Blue => BLUE,
        Team::Red => RED,
    };
    let color: [f32; 3] = std::array::from_fn(|channel| team[channel] * 0.5 + 0.5);
    let health = f32::from(shield.health_permille) / 1000.0;
    let mut dome = instance(center, [(radius * 2.0) as f32, (radius * 2.0) as f32, radius as f32], NORTH, color, NO_ICON, eye);
    dome.color[3] = 0.12 + 0.28 * health;
    dome
}

/// A mass deposit: a flat marker that stays visible at every zoom.
pub fn deposit_instance(center: [f64; 3], width: f64, eye: [f64; 3]) -> UnitInstanceRaw {
    instance(center, [width as f32, width as f32, 1.0], NORTH, DEPOSIT, UNIT_ICON_PX, eye)
}

/// A geothermal vent: a dark, wide patch on the ground.
pub fn vent_instance(center: [f64; 3], width: f64, eye: [f64; 3]) -> UnitInstanceRaw {
    instance(center, [width as f32, width as f32, 1.0], NORTH, VENT, UNIT_ICON_PX, eye)
}

/// A preview of a structure about to be placed, standing on `ground`. Use with the structure's own shape.
pub fn ghost_instance(ground: [f64; 3], width: f64, height_share: f32, valid: bool, eye: [f64; 3]) -> UnitInstanceRaw {
    let color = if valid { GHOST_VALID } else { GHOST_INVALID };
    // Slightly smaller than the real footprint, so that a row of them reads as separate buildings.
    let shown = (width * 0.86) as f32;
    let height = shown * height_share;
    let center = [ground[0], ground[1], ground[2] + f64::from(height) * 0.5];
    instance(center, [shown, shown, height], NORTH, color, NO_ICON, eye)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(id: u32, x: i32) -> UnitView {
        UnitView {
            id: SimId(id),
            team: Team::Blue,
            x,
            y: 0,
            z: 0,
            radius: 4096,
            blueprint: u16::MAX,
            health_permille: 1000,
            built_permille: 1000,
            is_structure: false,
            facing: [0, 0], aim: [0, 0], extra_aims: [[0; 2]; 3],
            working: false,
            assembling: None,
            upgrade: None,
            remembered: false,
            ally: false,
        }
    }

    fn translation(instance: &UnitInstanceRaw) -> [f32; 3] {
        let column = instance.model_matrix[3];
        [column[0], column[1], column[2]]
    }

    /// Builds instances with no looks, so that everything lands in the cube batch.
    fn cubes(previous: &[UnitView], current: &[UnitView], alpha: f64, eye: [f64; 3], selected: &[SimId]) -> Vec<UnitInstanceRaw> {
        let mut out = InstanceBatches::default();
        build_unit_instances(previous, current, alpha, eye, selected, &[], &[], &mut Motion::new(), &mut out);
        out.shape(Shape::Cube).to_vec()
    }

    #[test]
    fn interpolates_matching_units_and_keeps_new_ones_in_place() {
        let previous = [unit(1, 0), unit(3, 4096)];
        let current = [unit(1, 4096), unit(2, 8192), unit(3, 8192)];
        let out = cubes(&previous, &current, 0.5, [0.0; 3], &[]);
        assert_eq!(out.len(), 3);
        assert_eq!(translation(&out[0])[0], 0.5);
        assert_eq!(translation(&out[1])[0], 2.0);
        assert_eq!(translation(&out[2])[0], 1.5);
    }

    #[test]
    fn dead_units_are_not_drawn() {
        assert_eq!(cubes(&[unit(1, 0), unit(2, 0)], &[unit(2, 0)], 0.5, [0.0; 3], &[]).len(), 1);
    }

    #[test]
    fn selected_units_keep_their_color_and_get_a_ring_at_their_feet() {
        let current = [unit(1, 0), unit(2, 0), unit(3, 0)];
        let mut out = InstanceBatches::default();
        build_unit_instances(&current, &current, 0.0, [0.0; 3], &[SimId(2)], &[], &[], &mut Motion::new(), &mut out);
        assert!(out.shape(Shape::Cube).iter().all(|cube| cube.color[..3] == BLUE), "selection must not recolor units");
        let rings = out.shape(Shape::SelectionRing);
        assert_eq!(rings.len(), 1, "only the selected unit gets a ring");
        assert_eq!(rings[0].color[..3], SELECTED);
        assert_eq!(rings[0].color[3], NO_ICON, "a ring must not collapse into a dot");
        // The unit is centered at height 0 with radius 1, so its feet are at -1.
        assert_eq!(translation(&rings[0])[2], -1.0 + SELECTION_LIFT as f32);
        assert!(rings[0].model_matrix[0][0] > 2.0, "the ring must be wider than the unit");
    }

    #[test]
    fn selected_structures_get_a_box_instead_of_a_ring() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut generator = unit(1, 0);
        generator.blueprint = db.id("power_generator").0;
        generator.is_structure = true;
        let mut out = InstanceBatches::default();
        build_unit_instances(&[], &[generator], 0.0, [0.0; 3], &[SimId(1)], &looks, &[], &mut Motion::new(), &mut out);
        assert_eq!(out.shape(Shape::SelectionBox).len(), 1);
        assert_eq!(out.shape(Shape::SelectionRing).len(), 0);
        assert_ne!(out.shape(Shape::Generator)[0].color[..3], SELECTED);
    }

    #[test]
    fn every_blueprint_has_its_own_look() {
        let db = BlueprintDb::standard();
        // Missiles a silo keeps are never on the map.
        let drawn = || db.iter().filter(|unit| unit.class != crate::sim::blueprints::UnitClass::Ammo);
        for unit in drawn() {
            assert_ne!(unit_look(&unit.key).shape, Shape::Cube, "{} has no shape of its own", unit.key);
        }
        // Higher tiers of the factory and the engineer, the radar's deep scan upgrade, and the support
        // commander share the shape they come from, the air factory is a factory, the spy plane is
        // the air scout's T3, and the anti-air turret has the mobile anti-air's guns. Everything
        // else is unique.
        let shared = [
            "land_factory_t2",
            "land_factory_t3",
            "t2_engineer",
            "t3_engineer",
            "deep_scan_array",
            "field_rig",
            "mass_extractor_t2",
            "mass_extractor_t3",
            "mass_fabricator_t3",
            "air_factory",
            "t1_aa_turret",
            "air_factory_t2",
            "air_factory_t3",
            "land_factory_t2_support",
            "land_factory_t3_support",
            "air_factory_t2_support",
            "air_factory_t3_support",
            "power_generator_t2",
            "power_generator_t3",
            "radar_t2",
            "t2_point_defense",
            "energy_storage",
            "t3_artillery",
            "t3_mobile_artillery",
            "t2_destroyer",
            "t1_attack_sub",
            "t2_assault_hovercraft",
            "t2_escort_corvette",
            "t3_missile_sub",
            "t2_mobile_missile_launcher",
            "t1_sonar",
            "t2_sonar",
            "t1_torpedo_launcher",
            "t2_torpedo_launcher",
            "t2_cruiser",
            "t3_battleship",
            "t3_battlecruiser",
            "naval_factory_t2",
            "naval_factory_t3",
            "naval_factory_t2_support",
            "naval_factory_t3_support",
            "t3_heavy_gunship",
            "t3_spy_plane",
            "t3_air_superiority",
            "t2_flak",
            "t1_medium_tank",
            "t2_combat_engineer",
            "t3_heavy_shield",
            "field_workshop_t3",
            "t2_transport",
            "t3_heavy_transport",
        ];
        let shapes: Vec<Shape> = drawn().filter(|unit| !shared.contains(&unit.key.as_str())).map(|unit| unit_look(&unit.key).shape).collect();
        for (index, shape) in shapes.iter().enumerate() {
            assert!(!shapes[index + 1..].contains(shape), "{shape:?} is used twice");
        }
    }

    /// Surface warships look much alike at a glance, long grey hulls, so each is painted a hull
    /// of its own, for either team, far enough from every other's to tell apart in a button.
    #[test]
    fn every_surface_warship_has_a_hull_color_of_its_own() {
        use crate::sim::components::MoveClass;
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let ships: Vec<_> = db.iter().filter(|unit| unit.movement == MoveClass::Naval && !unit.is_structure()).collect();
        assert!(ships.len() >= 5, "the fleet is there");
        for team in [[0.25, 0.5, 1.0], [1.0, 0.3, 0.25]] {
            let hulls: Vec<(&str, [f32; 3])> = ships.iter().map(|unit| (unit.key.as_str(), hull_of(team, &looks[usize::from(unit.id.0)]))).collect();
            for (index, (first, a)) in hulls.iter().enumerate() {
                for (second, b) in &hulls[index + 1..] {
                    let distance = (0..3).map(|channel| (a[channel] - b[channel]).powi(2)).sum::<f32>().sqrt();
                    assert!(distance > 0.08, "{first} and {second} have hulls too alike: {a:?} and {b:?}");
                }
            }
        }
    }

    /// A unit's length as drawn, and how long it is for its width, from above: its base and
    /// what turns on it, facing the same way.
    fn footprint(set: &super::super::meshes::MeshSet, look: &UnitLook) -> (f32, f32) {
        let (mut half_width, mut ends) = (0.0_f32, [f32::MAX, f32::MIN]);
        for shape in look.base.into_iter().chain([look.shape]) {
            let range = set.ranges[shape as usize].clone();
            for &index in &set.indices[range.start as usize..range.end as usize] {
                let [x, y, _] = set.vertices[index as usize].position;
                half_width = half_width.max(x.abs());
                ends = [ends[0].min(y), ends[1].max(y)];
            }
        }
        let length = (ends[1] - ends[0]) * look.size;
        (length, length / (half_width * 2.0 * look.size))
    }

    /// Whether two footprints differ by a tenth or more in length or in length for width.
    fn footprints_apart((length_a, slender_a): (f32, f32), (length_b, slender_b): (f32, f32)) -> bool {
        let apart = |a: f32, b: f32| (a - b).abs() >= 0.1 * a.max(b);
        apart(length_a, length_b) || apart(slender_a, slender_b)
    }

    /// From above, which is how the camera sees the sea, a ship is its outline and its length.
    /// So no two surface ships are drawn the same length, and each differs from every other by a
    /// tenth or more in its length or in how long it is for its width.
    #[test]
    fn every_surface_ship_has_a_footprint_of_its_own() {
        use crate::sim::components::MoveClass;
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let set = super::super::meshes::MeshSet::build();
        let ships: Vec<(&str, (f32, f32))> =
            db.iter().filter(|unit| unit.movement == MoveClass::Naval && !unit.is_structure()).map(|unit| (unit.key.as_str(), footprint(&set, &looks[usize::from(unit.id.0)]))).collect();
        assert!(ships.len() >= 7, "the fleet is there");
        for (index, (first, a)) in ships.iter().enumerate() {
            for (second, b) in &ships[index + 1..] {
                let alike = format!("{first}, {:.0} long and {:.1} to 1, and {second}, {:.0} long and {:.1} to 1", a.0, a.1, b.0, b.1);
                assert!((a.0 - b.0).abs() >= 1.0, "{alike}, are drawn the same length");
                assert!(footprints_apart(*a, *b), "{alike}, look alike from above");
            }
        }
    }

    /// Land vehicles are told apart from above the same way: no two of different models within a
    /// tenth of each other in both length and length for width (until 2026-11-20 every one was
    /// about as long as it was wide). Upgrades of one model, the engineers, differ by size.
    /// Walkers and the command rigs stand on legs, which set them apart.
    #[test]
    fn every_land_vehicle_has_a_footprint_of_its_own() {
        use crate::sim::components::MoveClass;
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let set = super::super::meshes::MeshSet::build();
        let vehicles: Vec<(&str, &UnitLook)> = db
            .iter()
            .filter(|unit| !unit.is_structure() && !unit.is_air() && unit.class != crate::sim::blueprints::UnitClass::Ammo && !matches!(unit.movement, MoveClass::Naval | MoveClass::Submarine))
            .map(|unit| (unit.key.as_str(), &looks[usize::from(unit.id.0)]))
            .filter(|(_, look)| !look.base.is_some_and(Shape::walks))
            .collect();
        assert!(vehicles.len() >= 12, "the vehicles are there");
        for (index, (first, a)) in vehicles.iter().enumerate() {
            for (second, b) in &vehicles[index + 1..] {
                if (a.shape, a.base) == (b.shape, b.base) {
                    continue;
                }
                let (fa, fb) = (footprint(&set, a), footprint(&set, b));
                assert!(footprints_apart(fa, fb), "{first}, {:.0} long and {:.2} to 1, and {second}, {:.0} long and {:.2} to 1, look alike from above", fa.0, fa.1, fb.0, fb.1);
            }
        }
    }

    /// The two pairs of aircraft that once looked alike from above stay apart: the air
    /// superiority fighter is a long canard delta beside the interceptor's plain one, and the
    /// torpedo bomber has twin booms where the attack bomber has one body.
    #[test]
    fn look_alike_aircraft_have_footprints_apart() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let set = super::super::meshes::MeshSet::build();
        let of = |key: &str| footprint(&set, &looks[usize::from(db.id(key).0)]);
        for (first, second) in [("t1_interceptor", "t3_air_superiority"), ("t1_bomber", "t2_torpedo_bomber"), ("t1_bomber", "t3_strategic_bomber")] {
            assert!(footprints_apart(of(first), of(second)), "{first} and {second} look alike from above");
        }
    }

    /// From above, a fixed-wing aircraft's role shows in its wing: fighters' sweep back 40
    /// degrees or more, bombers' 15 or less, and the fighter-bomber's in between. Every aircraft
    /// with a main wing has its role here, so that a new one is placed too.
    #[test]
    fn fighters_sweep_their_wings_back_and_bombers_keep_them_straight() {
        let db = BlueprintDb::standard();
        let roles = [
            ("t1_interceptor", 40.0..=90.0),
            ("t3_air_superiority", 40.0..=90.0),
            ("t2_fighter_bomber", 20.0..=35.0),
            ("t1_bomber", 0.0..=15.0),
            ("t2_torpedo_bomber", 0.0..=15.0),
            ("t3_strategic_bomber", 0.0..=15.0),
        ];
        for unit in db.iter().filter(|unit| unit.is_air()) {
            let Some(wing) = unit_look(&unit.key).shape.main_wing() else { continue };
            let (_, sweep_range) = roles.iter().find(|(key, _)| *key == unit.key).unwrap_or_else(|| panic!("{} has a main wing but no role here", unit.key));
            let sweep = wing.sweep_degrees();
            assert!(sweep_range.contains(&sweep), "{}'s wing sweeps {sweep:.0} degrees, not {sweep_range:?}", unit.key);
        }
    }

    /// The strategic bomber is the largest aircraft with wings, from tip to tip and nose to
    /// tail, as drawn: larger than the heavy transport and the heavy gunship.
    #[test]
    fn the_strategic_bomber_is_the_largest_winged_aircraft() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let set = super::super::meshes::MeshSet::build();
        let extent = |look: &UnitLook| {
            let (mut span, mut ends) = (0.0_f32, [f32::MAX, f32::MIN]);
            for shape in look.base.into_iter().chain([look.shape]) {
                let range = set.ranges[shape as usize].clone();
                for &index in &set.indices[range.start as usize..range.end as usize] {
                    let [x, y, _] = set.vertices[index as usize].position;
                    span = span.max(x.abs() * 2.0);
                    ends = [ends[0].min(y), ends[1].max(y)];
                }
            }
            (span * look.size, (ends[1] - ends[0]) * look.size)
        };
        let bomber = extent(&looks[usize::from(db.id("t3_strategic_bomber").0)]);
        for unit in db.iter().filter(|unit| unit.is_air() && unit.key != "t3_strategic_bomber") {
            let other = extent(&looks[usize::from(unit.id.0)]);
            assert!(bomber.0 > other.0 && bomber.1 > other.1, "{} is {other:?} across and long, the strategic bomber only {bomber:?}", unit.key);
        }
    }

    /// A land unit's team color is one mark, not a lid: from straight above it covers no more
    /// than two fifths of the unit, so that its shape shows (the mobile anti-air was 61% team
    /// color until 2026-11-20). Aircraft are left out, their color being mostly wingtips and
    /// the transports' cargo bodies.
    #[test]
    fn no_land_unit_is_mostly_team_color_from_above() {
        use super::super::meshes::tests::view;
        use crate::sim::components::MoveClass;
        let db = BlueprintDb::standard();
        let set = super::super::meshes::MeshSet::build();
        for unit in db.iter().filter(|unit| !unit.is_structure() && !unit.is_air() && unit.class != crate::sim::blueprints::UnitClass::Ammo && !matches!(unit.movement, MoveClass::Naval | MoveClass::Submarine)) {
            let seen = view(&set, &look_parts(&unit_look(&unit.key)), 90.0);
            let covered = seen.iter().filter(|material| material.is_some()).count().max(1) as f32;
            let trim = seen.iter().filter(|material| matches!(material, Some(super::super::meshes::Material::Trim))).count() as f32 / covered;
            assert!(trim <= 0.4, "{} is {:.0}% team color from above", unit.key, trim * 100.0);
        }
    }

    /// Units drawn with the same model tell apart by their hulls: a higher tier is darker, as
    /// a heavier version of the same thing, and different units are colored apart.
    #[test]
    fn units_sharing_a_model_differ_by_tier_or_hull_color() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let units: Vec<_> = db.iter().filter(|unit| unit.class != crate::sim::blueprints::UnitClass::Ammo).collect();
        let brightness = |color: [f32; 3]| color[0] * 0.3 + color[1] * 0.59 + color[2] * 0.11;
        for team in [BLUE, RED] {
            for (index, first) in units.iter().enumerate() {
                for second in &units[index + 1..] {
                    let (a, b) = (&looks[usize::from(first.id.0)], &looks[usize::from(second.id.0)]);
                    if (a.shape, a.base) != (b.shape, b.base) {
                        continue;
                    }
                    let (hull_a, hull_b) = (hull_of(team, a), hull_of(team, b));
                    let distance = (0..3).map(|channel| (hull_a[channel] - hull_b[channel]).powi(2)).sum::<f32>().sqrt();
                    let darker_by_tier = first.tier != second.tier && {
                        let (lower, higher) = if first.tier < second.tier { (hull_a, hull_b) } else { (hull_b, hull_a) };
                        brightness(higher) < brightness(lower) - 0.02
                    };
                    assert!(darker_by_tier || distance > 0.05, "{} and {} share a model, and their hulls are too alike: {hull_a:?} and {hull_b:?}", first.key, second.key);
                }
            }
        }
    }

    /// A unit's role shows in its color, whoever's it is: mobile builders are work yellow and
    /// orange, and what only shoots at aircraft, on the ground or the water, is a cool cyan.
    #[test]
    fn builders_are_work_yellow_and_anti_air_is_cyan() {
        use crate::sim::blueprints::Targets;
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        for team in [BLUE, RED] {
            for unit in db.iter() {
                let hull = hull_of(team, &looks[usize::from(unit.id.0)]);
                let builds_structures = unit.builds.iter().any(|&made| db.get(made).is_structure());
                if builds_structures && !unit.is_structure() && !unit.is_commander() && unit.class != crate::sim::blueprints::UnitClass::FieldRig {
                    assert!(hull[0] > hull[2] + 0.05, "{} builds, but its hull is not warm: {hull:?}", unit.key);
                }
                if !unit.is_air() && unit.weapon.is_some_and(|weapon| weapon.targets == Targets::Air) {
                    assert!(hull[2] > hull[0] + 0.05 && hull[1] > hull[0], "{} shoots at aircraft, but its hull is not cyan: {hull:?}", unit.key);
                }
            }
        }
    }

    /// A wreck whose unit is known is drawn with that unit's models, charred, lower than it
    /// stood, and in no team's colors.
    #[test]
    fn a_wreck_is_the_charred_shell_of_what_it_was() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let tank = &looks[usize::from(db.id("t1_medium_tank").0)];
        let mut out = InstanceBatches::default();
        wreck_model_instances([0.0, 0.0, 4.5], 9.0, tank, [1.0, 0.0], [0.0, 0.0, 500.0], &mut out);
        let (hull, turret) = (out.shape(Shape::TankHull), out.shape(Shape::Tank));
        assert_eq!((hull.len(), turret.len()), (1, 1), "the tank's hull and turret");
        for part in [hull[0], turret[0]] {
            assert!(part.color[..3].iter().all(|channel| *channel < 0.4), "charred, not team-colored: {:?}", part.color);
            assert!(part.model_matrix[2][2] < tank.size * tank.height_share * 0.6, "squashed down");
        }
    }

    /// The team must show on every unit, whatever else it is painted with.
    #[test]
    fn every_unit_shows_its_team_color() {
        use super::super::meshes::{tests::materials, Material, MeshSet};
        let db = BlueprintDb::standard();
        let set = MeshSet::build();
        for unit in db.iter().filter(|unit| unit.class != crate::sim::blueprints::UnitClass::Ammo) {
            let look = unit_look(&unit.key);
            let parts: Vec<Material> = look.base.into_iter().chain([look.shape]).flat_map(|shape| materials(&set, shape)).collect();
            assert!(parts.contains(&Material::Trim), "{} has nothing in team color", unit.key);
            assert!(parts.len() > 1, "{} is painted all one color", unit.key);
        }
    }

    /// How steeply the camera looks down on units close up, in degrees, for measuring what
    /// shows of them in play.
    pub(crate) const PLAY_PITCH_DEGREES: f32 = 50.0;

    /// A unit's parts as `meshes::tests::view` draws them: its base, what turns on it facing the
    /// same way, and its turrets where they sit, at rest.
    pub(crate) fn look_parts(look: &UnitLook) -> Vec<(Shape, f32, [f32; 3])> {
        let turrets = look.turrets.into_iter().flatten().map(|mount| (Shape::NavalTurret, if mount.aft { std::f32::consts::PI } else { 0.0 }, mount.at));
        look.base.into_iter().chain([look.shape]).map(|shape| (shape, 0.0, [0.0; 3])).chain(turrets).collect()
    }

    /// What share of a unit, as the camera sees it close up, is each material: its base and
    /// what turns on it, facing the same way.
    pub(crate) fn seen_shares(set: &super::super::meshes::MeshSet, key: &str) -> [f32; 6] {
        use super::super::meshes::tests::view;
        let seen = view(set, &look_parts(&unit_look(key)), PLAY_PITCH_DEGREES);
        let covered = seen.iter().filter(|material| material.is_some()).count().max(1) as f32;
        std::array::from_fn(|index| seen.iter().filter(|material| material.map(|material| material as usize) == Some(index)).count() as f32 / covered)
    }

    /// The team's color shows on every unit as the camera sees it, on at least a sixth of it,
    /// so that whose it is reads at a glance: once only on turrets, many hulls showed too little.
    #[test]
    fn every_units_team_color_shows_from_the_camera() {
        let db = BlueprintDb::standard();
        let set = super::super::meshes::MeshSet::build();
        for unit in db.iter().filter(|unit| unit.class != crate::sim::blueprints::UnitClass::Ammo) {
            let trim = seen_shares(&set, &unit.key)[0];
            assert!(trim >= 0.15, "{} shows {:.0}% team color", unit.key, trim * 100.0);
        }
    }

    fn shell(id: u32, source: u32) -> ProjectileView {
        ProjectileView { id: SimId(id), team: Team::Blue, x: 0, y: 0, z: 0, kind: ProjectileKind::Shell, source: SimId(source), from: None, splash: 0 }
    }

    /// A unit kicks back as a new shell of its own appears, and settles again after.
    #[test]
    fn a_new_shell_kicks_its_gun_back_for_a_moment() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut tank = unit(1, 0);
        tank.blueprint = db.id("t1_medium_tank").0;
        let action = |motion: &mut Motion| {
            let mut out = InstanceBatches::default();
            build_unit_instances(&[tank], &[tank], 1.0, [0.0; 3], &[], &looks, &[], motion, &mut out);
            out.shape(Shape::Tank)[0].glow[3]
        };
        let mut motion = Motion::new();
        motion.note_shots(&[], &[]);
        assert_eq!(action(&mut motion), 0, "nothing fired yet");
        motion.note_shots(&[], &[shell(10, 1), shell(11, 2)]);
        assert_eq!(action(&mut motion), 255, "it has just fired");
        motion.time = RECOIL_SECONDS * 0.5;
        // The same shell, a frame later, is not a new shot.
        motion.note_shots(&[], &[shell(10, 1)]);
        let halfway = action(&mut motion);
        assert!(halfway > 64 && halfway < 192, "half settled: {halfway}");
        motion.time = RECOIL_SECONDS * 2.0;
        assert_eq!(action(&mut motion), 0, "settled");
    }

    /// Legs swing through a stride as a walker moves, and come back straight when it stops.
    #[test]
    fn legs_swing_while_walking_and_settle_when_stopped() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut motion = Motion::new();
        let phase = |motion: &mut Motion, x: i32| {
            let mut bot = unit(1, x);
            bot.blueprint = db.id("t1_light_walker").0;
            let mut out = InstanceBatches::default();
            build_unit_instances(&[bot], &[bot], 1.0, [0.0; 3], &[], &looks, &[], motion, &mut out);
            out.shape(Shape::BotLegs)[0].hull[3]
        };
        assert_eq!(phase(&mut motion, 0), 0);
        // A quarter of a stride, for a unit 2 across.
        let quarter = (2.0 * STRIDE_SHARE * 0.25 * 4096.0) as i32;
        let walking = phase(&mut motion, quarter);
        assert!((60..=70).contains(&walking), "a quarter of the way through a stride: {walking}");
        let settled = (0..100).map(|_| phase(&mut motion, quarter)).last().expect("ran");
        assert!(settled == 0 || settled == 128 || settled == 127, "legs straight again: {settled}");
    }

    /// Only a structure still being built stands in scaffolding, as tall as it will be.
    #[test]
    fn scaffolding_stands_around_unfinished_structures() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut site = unit(1, 0);
        site.blueprint = db.id("power_generator").0;
        site.is_structure = true;
        site.built_permille = 300;
        let mut finished = site;
        finished.id = SimId(2);
        finished.built_permille = 1000;
        let mut out = InstanceBatches::default();
        build_unit_instances(&[], &[site, finished], 0.0, [0.0; 3], &[], &looks, &[], &mut Motion::new(), &mut out);
        let frames = out.shape(Shape::Scaffold);
        assert_eq!(frames.len(), 1);
        let site_height = out.shape(Shape::Generator)[0].model_matrix[2][2];
        assert!(frames[0].model_matrix[2][2] > site_height, "the scaffold is as tall as the finished building");
    }

    /// A structure's upgrade shows the next tier rising through it as the upgrade goes on.
    #[test]
    fn an_upgrade_shows_the_next_tier_rising_through_the_structure() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut factory = unit(1, 0);
        factory.blueprint = db.id("land_factory_t1").0;
        factory.is_structure = true;
        factory.radius = 40 << 12;
        let into = db.id("land_factory_t2").0;
        let heights = |factory: UnitView| {
            let mut out = InstanceBatches::default();
            build_unit_instances(&[], &[factory], 0.0, [0.0; 3], &[], &looks, &[], &mut Motion::new(), &mut out);
            out.shape(Shape::FactoryT2).iter().map(|instance| instance.model_matrix[2][2]).collect::<Vec<f32>>()
        };
        assert!(heights(factory).is_empty(), "no upgrade, no T2 shape");
        let early = heights(UnitView { upgrade: Some((into, 100)), ..factory });
        let late = heights(UnitView { upgrade: Some((into, 900)), ..factory });
        assert_eq!((early.len(), late.len()), (1, 1));
        assert!(late[0] > early[0], "it rises as the upgrade goes on");
    }

    /// What a factory is building stands in its bay, rising as it is built, but not in a
    /// factory that is itself still going up.
    #[test]
    fn a_factory_shows_what_it_is_building_in_its_bay() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut factory = unit(1, 0);
        factory.blueprint = db.id("land_factory_t1").0;
        factory.is_structure = true;
        factory.radius = 30 << 12;
        let bot = db.id("t1_light_walker").0;
        let draw = |factory: UnitView| {
            let mut out = InstanceBatches::default();
            build_unit_instances(&[], &[factory], 0.0, [0.0; 3], &[], &looks, &[], &mut Motion::new(), &mut out);
            out.shape(Shape::Bot).iter().map(|instance| instance.model_matrix[2][2]).collect::<Vec<f32>>()
        };
        assert!(draw(factory).is_empty(), "an idle factory's bay is empty");
        let early = draw(UnitView { assembling: Some((bot, 100)), ..factory });
        let late = draw(UnitView { assembling: Some((bot, 900)), ..factory });
        assert_eq!((early.len(), late.len()), (1, 1), "the bot stands in the bay");
        assert!(late[0] > early[0], "it rises as it is built: {early:?} then {late:?}");
        assert!(draw(UnitView { assembling: Some((bot, 500)), built_permille: 500, ..factory }).is_empty(), "not while the factory is unfinished");
    }

    /// Working lights shine while a unit works, and dim when it stops.
    #[test]
    fn working_lights_shine_only_at_work() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut factory = unit(1, 0);
        factory.blueprint = db.id("land_factory_t1").0;
        factory.is_structure = true;
        let mut busy = factory;
        busy.id = SimId(2);
        busy.working = true;
        let mut out = InstanceBatches::default();
        build_unit_instances(&[], &[factory, busy], 0.0, [0.0; 3], &[], &looks, &[], &mut Motion::new(), &mut out);
        let lights: Vec<u8> = out.shape(Shape::Factory).iter().map(|instance| instance.glow[3]).collect();
        assert_eq!(lights[0], 0);
        assert!(lights[1] >= 180, "busy lights shine: {}", lights[1]);
    }

    #[test]
    fn units_have_grey_armor_and_glow_only_once_built() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut generator = unit(1, 0);
        generator.blueprint = db.id("power_generator").0;
        generator.is_structure = true;
        let mut site = generator;
        site.id = SimId(2);
        site.built_permille = 500;
        let mut out = InstanceBatches::default();
        build_unit_instances(&[], &[generator, site], 0.0, [0.0; 3], &[], &looks, &[], &mut Motion::new(), &mut out);
        let [finished, building] = out.shape(Shape::Generator) else { panic!("two generators") };
        assert_eq!(finished.color[..3], BLUE, "the trim is the team's own color");
        assert_ne!(bytes(BLUE), finished.hull, "the armor is not the team color");
        assert_ne!(finished.glow, finished.hull, "a finished generator glows");
        assert_eq!(building.glow, building.hull, "a generator being built does not glow yet");
        assert!(building.hull[0] < finished.hull[0], "a site looks unfinished");
    }

    /// A structure stands a little inside its footprint, on a foundation wider than all of it,
    /// so that a strip of floor shows between neighbors for the wiring between them. Walls fill
    /// their footprints, to join up.
    #[test]
    fn structures_leave_a_strip_of_floor_between_neighbors_but_walls_join_up() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let footprint = 20.0;
        let mut generator = unit(1, 0);
        (generator.blueprint, generator.is_structure, generator.radius) = (db.id("power_generator").0, true, (footprint * 0.5 * 4096.0) as i32);
        let wall = UnitView { id: SimId(2), blueprint: db.id("wall").0, ..generator };
        let mut out = InstanceBatches::default();
        build_unit_instances(&[], &[generator, wall], 1.0, [0.0; 3], &[], &looks, &[], &mut Motion::new(), &mut out);
        let width = |instance: &UnitInstanceRaw| instance.model_matrix[0][0].hypot(instance.model_matrix[0][1]);
        assert!((width(&out.shape(Shape::Generator)[0]) - footprint * STRUCTURE_SHARE).abs() < 1e-3, "the generator stands inside its footprint");
        assert!(width(&out.shape(Shape::Apron)[0]) > footprint, "on a foundation wider than all of it");
        assert!((width(&out.shape(Shape::Wall)[0]) - footprint).abs() < 1e-3, "a wall fills its footprint");
        let generator = &out.shape(Shape::Generator)[0];
        let bottom = generator.model_matrix[3][2] - generator.model_matrix[2][2] * 0.5;
        // The middle it is given is half its footprint above the ground, as before it shrank.
        assert!((bottom + footprint * 0.5).abs() < 1e-3, "and stands on the ground: {bottom}");
    }

    /// On a slope a structure stands level at the highest ground under it, on a foundation that
    /// reaches below the lowest, rather than sinking into the hill on one side.
    #[test]
    fn a_structure_on_a_slope_stands_level_on_a_slab_down_to_the_ground() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        // Ground rising one unit for each unit east.
        let samples = 101;
        let heights: Vec<i16> = (0..samples * samples).map(|index| ((index % samples) * 16 * 20) as i16).collect();
        let mut motion = Motion::new();
        motion.set_ground(crate::render::camera::HeightField::new(samples, heights, 20.0, 1.0 / 16.0));
        let (footprint, x, y) = (40.0_f64, 1_000.0_f64, 1_000.0_f64);
        let raw = |value: f64| (value * 4096.0) as i32;
        let mut generator = unit(1, 0);
        // Its middle half its footprint above the ground at its middle, as the simulation has it.
        (generator.blueprint, generator.is_structure, generator.radius) = (db.id("power_generator").0, true, raw(footprint * 0.5));
        (generator.x, generator.y, generator.z) = (raw(x), raw(y), raw(x + footprint * 0.5));
        let mut out = InstanceBatches::default();
        build_unit_instances(&[], &[generator], 1.0, [0.0; 3], &[], &looks, &[], &mut motion, &mut out);
        let half = footprint * 0.5 * f64::from(APRON_WIDTH);
        let (low, high) = (x - half, x + half);
        let body = &out.shape(Shape::Generator)[0];
        let foot = f64::from(body.model_matrix[3][2] - body.model_matrix[2][2] * 0.5);
        assert!((foot - high).abs() < 0.05, "it stands at the highest ground under it, {high}, not {foot}");
        let slab = &out.shape(Shape::Apron)[0];
        let (slab_bottom, slab_top) = (f64::from(slab.model_matrix[3][2] - slab.model_matrix[2][2] * 0.5), f64::from(slab.model_matrix[3][2] + slab.model_matrix[2][2] * 0.5));
        assert!(slab_bottom < low, "its slab reaches below the lowest ground, {low}: {slab_bottom}");
        assert!(slab_top >= foot, "and up to where it stands: {slab_top} against {foot}");
    }

    /// An ally's units, another player's on the same team, are drawn in a lighter shade of the
    /// team's color, so that the person's own stand apart from theirs.
    #[test]
    fn an_allys_units_take_a_lighter_shade_of_the_team_color() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut generator = unit(1, 0);
        (generator.blueprint, generator.is_structure) = (db.id("power_generator").0, true);
        let allys = UnitView { id: SimId(2), ally: true, ..generator };
        let mut out = InstanceBatches::default();
        build_unit_instances(&[], &[generator, allys], 0.0, [0.0; 3], &[], &looks, &[], &mut Motion::new(), &mut out);
        let [mine, theirs] = out.shape(Shape::Generator) else { panic!("two generators") };
        assert_eq!(mine.color[..3], BLUE);
        assert_eq!(theirs.color[..3], BLUE_ALLY, "an ally's in its own shade");
    }

    #[test]
    fn each_unit_is_drawn_with_its_blueprints_shape() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut tank = unit(1, 0);
        tank.blueprint = db.id("t1_medium_tank").0;
        let mut generator = unit(2, 0);
        generator.blueprint = db.id("power_generator").0;
        generator.is_structure = true;
        let mut out = InstanceBatches::default();
        build_unit_instances(&[], &[tank, generator], 0.0, [0.0; 3], &[], &looks, &[], &mut Motion::new(), &mut out);
        assert_eq!(out.shape(Shape::Tank).len(), 1);
        assert_eq!(out.shape(Shape::Generator).len(), 1);
        assert_eq!(out.shape(Shape::Cube).len(), 0);
        assert_eq!(out.shape(Shape::TankHull).len(), 1, "a tank is drawn as a hull and a turret");
        assert_eq!(out.shape(Shape::Apron).len(), 1, "and the generator stands on its foundation");
        assert_eq!(out.total(), 4);
    }

    fn facing(instance: &UnitInstanceRaw) -> [f32; 2] {
        let [x, y, ..] = instance.model_matrix[1];
        let size = x.hypot(y);
        [x / size, y / size]
    }

    /// A turret drawn on its own stands where its gun is mounted in the blueprint, and rests
    /// the same way, and every mounted gun is drawn on a turret of its own: so that shots are
    /// seen to leave the turret that fires them.
    #[test]
    fn every_turret_drawn_stands_where_its_gun_is_mounted() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        for unit in db.iter() {
            let look = &looks[usize::from(unit.id.0)];
            let weapons: Vec<_> = unit.weapon.iter().chain(&unit.extra_weapons).collect();
            for mount in look.turrets.iter().flatten() {
                let weapon = weapons.get(usize::from(mount.weapon)).unwrap_or_else(|| panic!("{} has no weapon {}", unit.key, mount.weapon));
                assert!(weapon.turn_rate.is_some() && usize::from(mount.weapon) <= 3, "{}'s weapon {} has a turret the view shows", unit.key, mount.weapon);
                let [right, forward] = weapon.mount.map(|axis| axis.to_num::<f32>());
                let drawn = [mount.at[0] * look.size, mount.at[1] * look.size];
                assert!((drawn[0] - right).hypot(drawn[1] - forward) < 1.0, "{}'s turret {} is drawn at {drawn:?}, mounted at {right}, {forward}", unit.key, mount.weapon);
                assert_eq!(mount.aft, weapon.aft, "{}'s turret {} rests the way its gun does", unit.key, mount.weapon);
            }
            for (index, _) in weapons.iter().enumerate().filter(|(_, weapon)| weapon.mount != [crate::math::fp::Fp::ZERO; 2]) {
                assert!(look.turrets.iter().flatten().any(|mount| usize::from(mount.weapon) == index), "{}'s weapon {index} is mounted but not drawn on a turret", unit.key);
            }
        }
    }

    /// Each of a battleship's turrets turns to its own gun's aim, where it stands on the hull,
    /// and turns with the hull while its gun has no aim.
    #[test]
    fn a_battleships_turrets_each_turn_to_their_own_guns_aim() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let look = &looks[usize::from(db.id("t3_battleship").0)];
        let mut ship = unit(1, 0);
        ship.blueprint = db.id("t3_battleship").0;
        ship.radius = db.get(db.id("t3_battleship")).radius.to_bits();
        ship.facing = [0, 4096];
        let draw = |ship: UnitView| {
            let mut out = InstanceBatches::default();
            build_unit_instances(&[ship], &[ship], 1.0, [0.0; 3], &[], &looks, &[], &mut Motion::new(), &mut out);
            out.shape(Shape::NavalTurret).iter().map(|turret| (facing(turret), [turret.model_matrix[3][0], turret.model_matrix[3][1]])).collect::<Vec<_>>()
        };
        let resting = draw(ship);
        assert_eq!(resting.len(), 3, "three turrets");
        for ((heading, at), mount) in resting.iter().zip(look.turrets.iter().flatten()) {
            assert!((heading[1] - if mount.aft { -1.0 } else { 1.0 }).abs() < 1e-3, "turret {} rests {}: {heading:?}", mount.weapon, if mount.aft { "aft" } else { "forward" });
            assert!((at[1] - mount.at[1] * look.size).abs() < 1e-3 && at[0].abs() < 1e-3, "turret {} stands at its mount, north of the middle: {at:?}", mount.weapon);
        }
        // Each gun aims somewhere else: east, west and north-east.
        ship.aim = [4096, 0];
        ship.extra_aims = [[-4096, 0], [2896, 2896], [0, 0]];
        let aimed = draw(ship);
        for ((heading, _), expected) in aimed.iter().zip([[1.0, 0.0], [-1.0, 0.0], [FRAC_1_SQRT_2, FRAC_1_SQRT_2]]) {
            assert!((heading[0] - expected[0]).abs() < 1e-3 && (heading[1] - expected[1]).abs() < 1e-3, "aimed {heading:?}, not {expected:?}");
        }
        // Turned east, the hull carries its turrets round with it.
        ship.facing = [4096, 0];
        let east = draw(UnitView { aim: [0, 0], extra_aims: [[0; 2]; 3], ..ship });
        let front = look.turrets[0].expect("a front turret").at[1] * look.size;
        assert!((east[0].1[0] - front).abs() < 1e-3 && east[0].1[1].abs() < 1e-3, "the front turret is east of the middle: {:?}", east[0].1);
    }

    /// A shell on the tick it was fired has already flown a tick, so it is drawn from its gun,
    /// not from where that tick took it.
    #[test]
    fn a_new_shell_is_drawn_leaving_its_gun() {
        let fired = ProjectileView { x: 4096 * 60, from: Some([4096 * 10, 0, 0]), ..shell(1, 1) };
        let draw = |alpha: f64| {
            let mut out = InstanceBatches::default();
            build_projectile_instances(&[], &[fired], &[], alpha, [0.0; 3], &mut out);
            out.shape(Shape::Shell)[0].model_matrix[3][0]
        };
        assert!((draw(0.0) - 10.0).abs() < 1e-3, "at the gun: {}", draw(0.0));
        assert!((draw(0.5) - 35.0).abs() < 1e-3, "halfway out: {}", draw(0.5));
        assert!((draw(1.0) - 60.0).abs() < 1e-3, "where the tick took it: {}", draw(1.0));
    }

    #[test]
    fn units_turn_towards_where_they_are_going_and_keep_facing_when_stopped() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut tank = unit(1, 0);
        tank.blueprint = db.id("t1_medium_tank").0;
        let mut moved = tank;
        moved.x = 4096 * 4;
        let mut motion = Motion::new();
        let mut draw = |before: UnitView, after: UnitView| {
            let mut out = InstanceBatches::default();
            build_unit_instances(&[before], &[after], 1.0, [0.0; 3], &[], &looks, &[], &mut motion, &mut out);
            facing(&out.shape(Shape::Tank)[0])
        };
        let start = draw(tank, tank);
        assert!((start[0] - FRAC_1_SQRT_2).abs() < 1e-3, "Blue starts facing the middle of the map: {start:?}");
        let mut now = start;
        for _ in 0..60 {
            now = draw(tank, moved);
        }
        assert!((now[0] - 1.0).abs() < 1e-3 && now[1].abs() < 1e-2, "moving east should face east: {now:?}");
        assert_eq!(draw(moved, moved), now, "a stopped unit keeps its facing");
    }

    #[test]
    fn a_working_builder_turns_to_face_its_work_and_keeps_facing_it() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut rig = unit(1, 0);
        rig.blueprint = db.id("command_rig").0;
        // Directly north-west of the unit, whichever way it was facing.
        let work = [WorkLinkView { builder: SimId(1), target: [-40_960, 40_960, 0], reclaim: false }];
        let mut motion = Motion::new();
        let mut draw = |before: UnitView, links: &[WorkLinkView]| {
            let mut out = InstanceBatches::default();
            build_unit_instances(&[before], &[rig], 1.0, [0.0; 3], &[], &looks, links, &mut motion, &mut out);
            facing(&out.shape(Shape::Commander)[0])
        };
        let start = draw(rig, &[]);
        assert!(start[0] > 0.0, "Blue starts out facing the middle of the map: {start:?}");
        let mut now = start;
        for _ in 0..60 {
            now = draw(rig, &work);
        }
        assert!((now[0] + FRAC_1_SQRT_2).abs() < 1e-2 && (now[1] - FRAC_1_SQRT_2).abs() < 1e-2, "should face its work: {now:?}");

        // Being nudged while working must not turn it away.
        let mut nudged = rig;
        nudged.x = -4096 * 4;
        let settled = draw(nudged, &work);
        assert!((settled[0] + FRAC_1_SQRT_2).abs() < 1e-2);
        assert_eq!(draw(rig, &[]), settled, "it keeps facing the same way when the work is done");
    }

    /// Blue starts out facing north-east, and its first factory can be exactly behind it.
    #[test]
    fn a_unit_can_turn_towards_something_exactly_behind_it() {
        let mut heading = [FRAC_1_SQRT_2, FRAC_1_SQRT_2];
        for _ in 0..60 {
            turn_towards(&mut heading, -50.0, -50.0);
        }
        assert!((heading[0] + FRAC_1_SQRT_2).abs() < 1e-3 && (heading[1] + FRAC_1_SQRT_2).abs() < 1e-3, "still facing {heading:?}");
        assert!((heading[0].hypot(heading[1]) - 1.0).abs() < 1e-3, "a heading must stay a unit vector");
    }

    #[test]
    fn turning_takes_the_short_way_round_in_both_directions() {
        for (dx, dy) in [(1.0_f64, 0.2_f64), (0.2, 1.0), (-1.0, 0.1), (0.1, -1.0)] {
            let mut heading = [FRAC_1_SQRT_2, FRAC_1_SQRT_2];
            let target = dy.atan2(dx) as f32;
            let gap = |heading: [f32; 2]| {
                let delta = (heading[1].atan2(heading[0]) - target).rem_euclid(std::f32::consts::TAU);
                delta.min(std::f32::consts::TAU - delta)
            };
            let before = gap(heading);
            turn_towards(&mut heading, dx, dy);
            assert!(gap(heading) < before, "turning towards ({dx}, {dy}) should close the gap");
        }
    }

    #[test]
    fn a_commanders_legs_face_the_way_it_walks_while_its_body_faces_its_work() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut rig = unit(1, 0);
        rig.blueprint = db.id("command_rig").0;
        // Walking east, and building something to the north.
        rig.facing = [4096, 0];
        let work = [WorkLinkView { builder: SimId(1), target: [0, 40_960, 0], reclaim: false }];
        let mut motion = Motion::new();
        let mut out = InstanceBatches::default();
        for _ in 0..60 {
            out.clear();
            build_unit_instances(&[rig], &[rig], 1.0, [0.0; 3], &[], &looks, &work, &mut motion, &mut out);
        }
        let legs = facing(&out.shape(Shape::CommanderLegs)[0]);
        let body = facing(&out.shape(Shape::Commander)[0]);
        assert!((legs[0] - 1.0).abs() < 1e-3, "the legs should face east: {legs:?}");
        assert!((body[1] - 1.0).abs() < 1e-2, "the body should face its work to the north: {body:?}");

        // With the work done, the body settles back over its legs.
        for _ in 0..60 {
            out.clear();
            build_unit_instances(&[rig], &[rig], 1.0, [0.0; 3], &[], &looks, &[], &mut motion, &mut out);
        }
        let body = facing(&out.shape(Shape::Commander)[0]);
        assert!((body[0] - 1.0).abs() < 1e-2, "the body should face the way the legs do: {body:?}");
    }

    #[test]
    fn a_tank_drives_one_way_while_its_turret_aims_another() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut tank = unit(1, 0);
        tank.blueprint = db.id("t1_medium_tank").0;
        tank.facing = [4096, 0];
        tank.aim = [0, -4096];
        let mut motion = Motion::new();
        let mut out = InstanceBatches::default();
        for _ in 0..60 {
            out.clear();
            build_unit_instances(&[tank], &[tank], 1.0, [0.0; 3], &[], &looks, &[], &mut motion, &mut out);
        }
        assert!((facing(&out.shape(Shape::TankHull)[0])[0] - 1.0).abs() < 1e-3, "the hull should face east");
        assert!((facing(&out.shape(Shape::Tank)[0])[1] + 1.0).abs() < 1e-2, "the turret should aim south");
    }

    #[test]
    fn a_point_defense_turns_only_its_gun() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut tower = unit(1, 0);
        tower.blueprint = db.id("t1_point_defense").0;
        tower.is_structure = true;
        tower.aim = [-4096, 0];
        let mut out = InstanceBatches::default();
        build_unit_instances(&[tower], &[tower], 1.0, [0.0; 3], &[], &looks, &[], &mut Motion::new(), &mut out);
        assert_eq!(facing(&out.shape(Shape::PointDefenseBase)[0]), NORTH, "the base stays put");
        assert!((facing(&out.shape(Shape::PointDefense)[0])[0] + 1.0).abs() < 1e-3, "the gun should aim west");
    }

    #[test]
    fn a_units_body_turns_smoothly_between_ticks() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut before = unit(1, 0);
        before.blueprint = db.id("t1_medium_tank").0;
        before.facing = [4096, 0];
        let mut after = before;
        after.facing = [0, 4096];
        let mut out = InstanceBatches::default();
        build_unit_instances(&[before], &[after], 0.5, [0.0; 3], &[], &looks, &[], &mut Motion::new(), &mut out);
        let halfway = facing(&out.shape(Shape::Tank)[0]);
        assert!((halfway[0] - FRAC_1_SQRT_2).abs() < 1e-3 && (halfway[1] - FRAC_1_SQRT_2).abs() < 1e-3, "{halfway:?}");
    }

    #[test]
    fn only_the_builder_doing_the_work_turns_towards_it() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut first = unit(1, 0);
        first.blueprint = db.id("t1_engineer").0;
        let mut second = unit(2, 0);
        second.blueprint = db.id("t1_engineer").0;
        let work = [WorkLinkView { builder: SimId(2), target: [-40_960, 0, 0], reclaim: false }];
        let mut motion = Motion::new();
        let units = [first, second];
        let mut out = InstanceBatches::default();
        for _ in 0..60 {
            out.clear();
            build_unit_instances(&units, &units, 1.0, [0.0; 3], &[], &looks, &work, &mut motion, &mut out);
        }
        let engineers = out.shape(Shape::Engineer);
        assert!((facing(&engineers[0])[0] - FRAC_1_SQRT_2).abs() < 1e-3, "the idle one stays as it was");
        assert!((facing(&engineers[1])[0] + 1.0).abs() < 1e-2, "the working one faces west");
    }

    #[test]
    fn turning_keeps_the_shape_undistorted() {
        let instance = instance([0.0; 3], [2.0, 2.0, 2.0], [0.6, 0.8], BLUE, 1.0, [0.0; 3]);
        let [right, forward, ..] = instance.model_matrix;
        assert!((right[0] * forward[0] + right[1] * forward[1]).abs() < 1e-6, "the axes must stay square");
        assert_eq!(forward[..2], [1.2, 1.6], "+Y points along the heading");
        assert!(right[0] * forward[1] - right[1] * forward[0] > 0.0, "turning must not mirror the shape");
    }

    /// At the far edge of the largest map, adjacent raw positions must stay distinct after
    /// the conversion to `f32`, which needs the subtraction to happen in `f64`.
    #[test]
    fn camera_relative_conversion_keeps_precision_far_from_the_origin() {
        let far = 81_000 * 4096;
        let eye = [81_000.0, 0.0, 0.0];
        let out = cubes(&[], &[unit(1, far), unit(2, far + 1)], 0.0, eye, &[]);
        let difference = translation(&out[1])[0] - translation(&out[0])[0];
        assert!((difference - 1.0 / 4096.0).abs() < 1e-6, "difference was {difference}");
    }
}
