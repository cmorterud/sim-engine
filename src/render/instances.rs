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

const BLUE: [f32; 3] = [0.20, 0.50, 1.00];
const RED: [f32; 3] = [1.00, 0.27, 0.22];
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
/// Bare armor, before it takes on a little of the team's and the blueprint's color.
const ARMOR: [f32; 3] = [0.50, 0.52, 0.55];
/// How much of the blueprint's tint, then the team's color, the armor takes on.
const ARMOR_TINT: f32 = 0.4;
const ARMOR_TEAM: f32 = 0.15;
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
}

impl Motion {
    pub fn new() -> Self {
        Self::default()
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
}

/// Maps a blueprint to its look. Unknown keys fall back to a plain cube.
pub fn unit_look(key: &str) -> UnitLook {
    let look = |shape, tint, height_share| UnitLook { shape, tint, height_share, base: None, size: 0.0 };
    match key {
        "command_rig" => UnitLook { base: Some(Shape::CommanderLegs), ..look(Shape::Commander, Some([1.00, 1.00, 1.00]), 1.0) },
        "field_rig" => UnitLook { base: Some(Shape::CommanderLegs), ..look(Shape::Commander, Some([0.70, 0.75, 0.85]), 1.0) },
        "rig_yard" => look(Shape::RigYard, Some([0.55, 0.60, 1.00]), 0.6),
        "geothermal_plant" => look(Shape::GeothermalPlant, Some([0.85, 0.60, 0.25]), 0.7),
        "mass_fabricator" => look(Shape::Fabricator, Some([0.40, 0.95, 0.85]), 1.0),
        "mass_fabricator_t3" => look(Shape::Fabricator, Some([0.70, 0.45, 1.00]), 0.8),
        "t2_engineer" => UnitLook { base: Some(Shape::EngineerHull), ..look(Shape::Engineer, Some([0.95, 0.65, 0.25]), 1.0) },
        "t3_engineer" => UnitLook { base: Some(Shape::EngineerHull), ..look(Shape::Engineer, Some([0.95, 0.50, 0.20]), 1.0) },
        "land_factory_t3" => look(Shape::FactoryT3, Some([0.10, 0.10, 0.20]), 0.85),
        "t1_engineer" => UnitLook { base: Some(Shape::EngineerHull), ..look(Shape::Engineer, Some([0.95, 0.80, 0.30]), 1.0) },
        "t1_light_walker" => UnitLook { base: Some(Shape::BotLegs), ..look(Shape::Bot, None, 1.0) },
        "t1_land_scout" => look(Shape::LandScout, Some([0.75, 0.95, 1.00]), 1.0),
        "t1_medium_tank" => UnitLook { base: Some(Shape::TankHull), ..look(Shape::Tank, Some([0.80, 0.85, 0.70]), 1.0) },
        "t2_grenadier_walker" => UnitLook { base: Some(Shape::BotLegs), ..look(Shape::GrenadierWalker, None, 1.0) },
        "t2_mobile_shield" => UnitLook { base: Some(Shape::TankHull), ..look(Shape::ShieldEmitter, Some([0.55, 0.85, 1.00]), 1.0) },
        "t2_combat_engineer" => UnitLook { base: Some(Shape::EngineerHull), ..look(Shape::Engineer, Some([0.95, 0.40, 0.25]), 1.0) },
        "t3_heavy_walker" => UnitLook { base: Some(Shape::BotLegs), ..look(Shape::HeavyWalker, None, 1.0) },
        "t3_siege_walker" => UnitLook { base: Some(Shape::BotLegs), ..look(Shape::SiegeWalker, Some([0.70, 0.70, 0.75]), 1.0) },
        "t2_torpedo_boat" => look(Shape::TorpedoBoat, Some([0.40, 0.90, 0.80]), 0.5),
        "t3_heavy_point_defense" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::HeavyPointDefense, Some([0.95, 0.35, 0.30]), 1.0) },
        "t3_heavy_shield" => look(Shape::ShieldGenerator, Some([0.45, 0.70, 1.00]), 0.9),
        "wall" => look(Shape::Wall, Some([0.70, 0.70, 0.70]), 0.9),
        "field_workshop" => look(Shape::FieldWorkshop, Some([0.95, 0.75, 0.30]), 0.8),
        "t1_light_transport" => look(Shape::Transport, Some([0.85, 0.80, 0.60]), 1.0),
        "t2_transport" => look(Shape::Transport, Some([0.70, 0.75, 0.80]), 1.0),
        "t3_heavy_transport" => look(Shape::Transport, Some([0.55, 0.60, 0.75]), 1.0),
        "field_workshop_t3" => look(Shape::FieldWorkshop, Some([0.95, 0.55, 0.20]), 0.95),
        "t1_light_artillery" => UnitLook { base: Some(Shape::ArtilleryHull), ..look(Shape::Artillery, None, 1.0) },
        "t2_heavy_tank" => UnitLook { base: Some(Shape::TankHull), ..look(Shape::Tank, None, 1.0) },
        "t2_assault_hovercraft" => UnitLook { base: Some(Shape::TankHull), ..look(Shape::Tank, Some([0.45, 0.65, 0.75]), 1.0) },
        "land_factory_t1" => look(Shape::Factory, Some([0.35, 0.35, 0.45]), 0.55),
        "land_factory_t2" => look(Shape::FactoryT2, Some([0.20, 0.20, 0.30]), 0.7),
        "mass_extractor" => look(Shape::Extractor, Some([0.10, 0.95, 0.75]), 0.8),
        "mass_extractor_t2" => look(Shape::ExtractorT2, Some([0.10, 0.75, 0.95]), 1.0),
        "mass_extractor_t3" => look(Shape::ExtractorT3, Some([0.55, 0.40, 1.00]), 1.2),
        "power_generator" => look(Shape::Generator, Some([1.00, 0.85, 0.20]), 1.2),
        "mass_storage" => look(Shape::Storage, Some([0.60, 0.90, 0.60]), 0.65),
        "energy_storage" => look(Shape::Storage, Some([1.00, 0.90, 0.40]), 0.65),
        "t2_artillery" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::ArtilleryGun, Some([0.80, 0.55, 0.30]), 1.0) },
        "t3_artillery" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::ArtilleryGun, Some([0.50, 0.30, 0.20]), 0.6) },
        "t3_mobile_artillery" => UnitLook { base: Some(Shape::ArtilleryHull), ..look(Shape::Artillery, Some([0.55, 0.45, 0.35]), 1.0) },
        "radar" => look(Shape::Radar, Some([0.40, 0.90, 0.55]), 1.3),
        "deep_scan_array" => look(Shape::Radar, Some([0.75, 0.55, 1.00]), 1.6),
        "stealth_field_generator" => look(Shape::StealthGenerator, Some([0.30, 0.30, 0.35]), 1.2),
        "shield_generator" => look(Shape::ShieldGenerator, Some([0.55, 0.85, 1.00]), 1.0),
        "tactical_missile_launcher" => look(Shape::MissileLauncher, Some([0.85, 0.85, 0.80]), 0.9),
        "tactical_missile_defense" => look(Shape::MissileDefense, Some([1.00, 0.55, 0.45]), 1.2),
        "strategic_missile_launcher" => look(Shape::NukeSilo, Some([1.00, 0.90, 0.30]), 0.5),
        "strategic_missile_defense" => look(Shape::StrategicInterceptorSilo, Some([0.95, 0.95, 0.95]), 0.7),
        "air_factory" => look(Shape::AirFactory, Some([0.45, 0.60, 0.80]), 0.5),
        "airfield" => look(Shape::Airfield, Some([0.70, 0.80, 0.90]), 0.5),
        "naval_factory" => look(Shape::NavalFactory, Some([0.35, 0.45, 0.55]), 0.5),
        "t1_frigate" => look(Shape::Frigate, None, 0.5),
        "t1_attack_sub" => look(Shape::Submarine, Some([0.10, 0.20, 0.30]), 0.3),
        "t2_escort_corvette" => look(Shape::EscortCorvette, Some([0.55, 0.85, 1.00]), 0.5),
        "t3_missile_sub" => look(Shape::MissileSubmarine, Some([0.05, 0.10, 0.20]), 0.35),
        "t2_mobile_missile_launcher" => UnitLook { base: Some(Shape::ArtilleryHull), ..look(Shape::MissileLauncher, Some([0.80, 0.80, 0.70]), 1.0) },
        "t2_torpedo_bomber" => look(Shape::Bomber, Some([0.40, 0.55, 0.70]), 0.55),
        "t1_sonar" => look(Shape::Radar, Some([0.30, 0.60, 0.90]), 1.0),
        "t2_sonar" => look(Shape::Radar, Some([0.20, 0.45, 0.85]), 1.2),
        "t1_torpedo_launcher" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::SamLauncher, Some([0.20, 0.40, 0.60]), 0.7) },
        "t2_torpedo_launcher" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::SamLauncher, Some([0.15, 0.30, 0.50]), 0.8) },
        "t2_destroyer" => look(Shape::Destroyer, Some([0.45, 0.45, 0.50]), 0.5),
        "t2_cruiser" => look(Shape::Cruiser, Some([0.70, 0.75, 0.80]), 0.5),
        "t3_battleship" => look(Shape::Battleship, Some([0.25, 0.25, 0.30]), 0.6),
        "t3_battlecruiser" => look(Shape::Battlecruiser, Some([0.40, 0.35, 0.30]), 0.55),
        "naval_factory_t2" => look(Shape::NavalFactoryT2, Some([0.25, 0.35, 0.45]), 0.6),
        "naval_factory_t3" => look(Shape::NavalFactoryT3, Some([0.15, 0.22, 0.32]), 0.7),
        "naval_factory_t2_support" => look(Shape::NavalFactoryT2, Some([0.30, 0.40, 0.50]), 0.55),
        "naval_factory_t3_support" => look(Shape::NavalFactoryT3, Some([0.20, 0.28, 0.38]), 0.65),
        "land_factory_t2_support" => look(Shape::FactoryT2, Some([0.28, 0.28, 0.36]), 0.62),
        "land_factory_t3_support" => look(Shape::FactoryT3, Some([0.16, 0.16, 0.26]), 0.78),
        "air_factory_t2_support" => look(Shape::AirFactoryT2, Some([0.36, 0.52, 0.72]), 0.58),
        "air_factory_t3_support" => look(Shape::AirFactoryT3, Some([0.22, 0.34, 0.54]), 0.72),
        "power_generator_t2" => look(Shape::GeneratorT2, Some([1.00, 0.70, 0.15]), 1.0),
        "power_generator_t3" => look(Shape::GeneratorT3, Some([1.00, 0.55, 0.10]), 0.9),
        "radar_t2" => look(Shape::Radar, Some([0.50, 0.95, 0.75]), 1.45),
        "t2_point_defense" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::PointDefense, Some([0.85, 0.30, 0.65]), 1.0) },
        "air_factory_t2" => look(Shape::AirFactoryT2, Some([0.30, 0.45, 0.65]), 0.65),
        "air_factory_t3" => look(Shape::AirFactoryT3, Some([0.15, 0.25, 0.45]), 0.8),
        "t2_gunship" => look(Shape::Gunship, None, 0.6),
        "t3_heavy_gunship" => look(Shape::HeavyGunship, Some([0.20, 0.20, 0.25]), 0.6),
        "t3_spy_plane" => look(Shape::Aircraft, Some([0.30, 0.30, 0.35]), 0.5),
        "t3_air_superiority" => look(Shape::AirSuperiority, Some([0.95, 0.95, 1.00]), 0.5),
        "t3_strategic_bomber" => look(Shape::StrategicBomber, None, 0.5),
        "t2_fighter_bomber" => look(Shape::Bomber, Some([0.85, 0.70, 0.55]), 0.55),
        "t2_flak" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::FlakGun, Some([0.35, 0.75, 0.95]), 1.0) },
        "t2_mobile_flak" => UnitLook { base: Some(Shape::TankHull), ..look(Shape::FlakGun, None, 1.0) },
        "t3_sam" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::SamLauncher, Some([0.55, 0.85, 1.00]), 1.1) },
        "t1_air_scout" => look(Shape::Aircraft, Some([0.85, 0.90, 0.95]), 0.5),
        "t1_interceptor" => look(Shape::Aircraft, None, 0.5),
        "t1_bomber" => look(Shape::Bomber, None, 0.6),
        "t1_aa_turret" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::AntiAirGun, Some([0.35, 0.75, 0.95]), 0.9) },
        "t1_mobile_aa" => UnitLook { base: Some(Shape::TankHull), ..look(Shape::AntiAirGun, None, 1.0) },
        "t1_point_defense" => UnitLook { base: Some(Shape::PointDefenseBase), ..look(Shape::PointDefense, Some([0.75, 0.25, 0.85]), 0.9) },
        _ => look(Shape::Cube, None, 0.6),
    }
}

/// Per-blueprint looks, indexed by raw blueprint id.
pub fn unit_looks(blueprints: &BlueprintDb) -> Vec<UnitLook> {
    blueprints.iter().map(|unit| UnitLook { size: unit.radius.to_num::<f32>() * 2.0, ..unit_look(&unit.key) }).collect()
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
    if unit.facing == [0, 0] {
        return None;
    }
    let now = [f64::from(unit.facing[0]), f64::from(unit.facing[1])];
    let from = before.filter(|before| before.facing != [0, 0]).map_or(now, |before| [f64::from(before.facing[0]), f64::from(before.facing[1])]);
    let blended = [from[0] + (now[0] - from[0]) * alpha, from[1] + (now[1] - from[1]) * alpha];
    let length = blended[0].hypot(blended[1]);
    // Turning right round in one tick passes through nothing, so use where it ends up.
    let [x, y] = if length > 1.0 { blended } else { now };
    let length = x.hypot(y);
    Some([(x / length) as f32, (y / length) as f32])
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
        let color = match unit.team {
            Team::Blue => BLUE,
            Team::Red => RED,
        };
        let look = looks.get(usize::from(unit.blueprint));
        let tint = look.and_then(|look| look.tint);
        let hull = armor(color, tint);
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
            // A structure rises from the ground as it is built, and looks unfinished until then.
            let built = f32::from(unit.built_permille) / 1000.0;
            let height_share = look.map_or(0.6, |look| look.height_share);
            let height = size * height_share * (0.25 + 0.75 * built);
            center[2] += f64::from(height - size) * 0.5;
            let shade = 0.55 + 0.45 * built;
            let (trim, hull) = (color.map(|channel| channel * shade), hull.map(|channel| channel * shade));
            // Nothing glows until it is finished.
            let glow = if unit.built_permille >= 1000 { glow } else { hull };
            // A structure's base stays put, and its gun, if any, turns to aim.
            if let Some(base) = look.and_then(|look| look.base) {
                out.push(base, animated(painted(center, [size, size, height], NORTH, trim, hull, glow, UNIT_ICON_PX * 1.6, eye), 0));
            }
            let aim = unit_vector(unit.aim).unwrap_or(NORTH);
            out.push(shape, animated(painted(center, [size, size, height], aim, trim, hull, glow, UNIT_ICON_PX * 1.6, eye), spin));
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
                    let next_hull = armor(color, next.tint).map(|channel| channel * (0.6 + 0.4 * progress));
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
                    let made_hull = armor(color, made.tint).map(|channel| channel * dim);
                    let extent = [made.size, made.size, rise];
                    if let Some(base) = made.base {
                        out.push(base, painted(at, extent, EAST, made_trim, made_hull, made_hull, NO_ICON, eye));
                    }
                    out.push(made.shape, painted(at, extent, EAST, made_trim, made_hull, made_hull, NO_ICON, eye));
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
            if let Some(base) = look.and_then(|look| look.base) {
                let stride = if base.walks() { byte(motion.stride(unit.id, center, size)) } else { 0 };
                out.push(base, animated(painted(center, [size; 3], body, color, hull, glow, UNIT_ICON_PX, eye), stride));
            }
            out.push(shape, animated(painted(center, [size; 3], upper, color, hull, glow, UNIT_ICON_PX, eye), spin));
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
        let from = before.map_or(now, |before| [before.x, before.y, before.z]);
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
            facing: [0, 0], aim: [0, 0],
            working: false,
            assembling: None,
            upgrade: None,
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
        // commander share the shape they come from, the air factory is a factory, the scout a small
        // fighter, and the anti-air turret has the mobile anti-air's guns. Everything else is unique.
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
            "t1_air_scout",
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
            "t2_fighter_bomber",
            "t2_destroyer",
            "t1_attack_sub",
            "t2_assault_hovercraft",
            "t2_escort_corvette",
            "t3_missile_sub",
            "t2_mobile_missile_launcher",
            "t2_torpedo_bomber",
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

    fn shell(id: u32, source: u32) -> ProjectileView {
        ProjectileView { id: SimId(id), team: Team::Blue, x: 0, y: 0, z: 0, kind: ProjectileKind::Shell, source: SimId(source), splash: 0 }
    }

    /// A unit kicks back as a new shell of its own appears, and settles again after.
    #[test]
    fn a_new_shell_kicks_its_gun_back_for_a_moment() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut tank = unit(1, 0);
        tank.blueprint = db.id("t2_heavy_tank").0;
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

    #[test]
    fn each_unit_is_drawn_with_its_blueprints_shape() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut tank = unit(1, 0);
        tank.blueprint = db.id("t2_heavy_tank").0;
        let mut generator = unit(2, 0);
        generator.blueprint = db.id("power_generator").0;
        generator.is_structure = true;
        let mut out = InstanceBatches::default();
        build_unit_instances(&[], &[tank, generator], 0.0, [0.0; 3], &[], &looks, &[], &mut Motion::new(), &mut out);
        assert_eq!(out.shape(Shape::Tank).len(), 1);
        assert_eq!(out.shape(Shape::Generator).len(), 1);
        assert_eq!(out.shape(Shape::Cube).len(), 0);
        assert_eq!(out.shape(Shape::TankHull).len(), 1, "a tank is drawn as a hull and a turret");
        assert_eq!(out.total(), 3);
    }

    fn facing(instance: &UnitInstanceRaw) -> [f32; 2] {
        let [x, y, ..] = instance.model_matrix[1];
        let size = x.hypot(y);
        [x / size, y / size]
    }

    #[test]
    fn units_turn_towards_where_they_are_going_and_keep_facing_when_stopped() {
        let db = BlueprintDb::standard();
        let looks = unit_looks(&db);
        let mut tank = unit(1, 0);
        tank.blueprint = db.id("t2_heavy_tank").0;
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
        tank.blueprint = db.id("t2_heavy_tank").0;
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
        before.blueprint = db.id("t2_heavy_tank").0;
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
