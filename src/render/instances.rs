//! GPU instance data. Interpolates between two snapshots and converts to camera-relative `f32`.

use std::collections::BTreeMap;
use std::f32::consts::FRAC_1_SQRT_2;

use super::meshes::Shape;
use crate::sim::blueprints::BlueprintDb;
use crate::sim::components::{SimId, Team, WreckKind};
use crate::sim::world::{ProjectileKind, ProjectileView, ShieldView, UnitView, WorkLinkView};

const RAW_TO_WORLD: f64 = 1.0 / 4096.0;
const PROJECTILE_SIZE: f32 = 0.6;

/// On-screen half-size of a unit's tactical dot, in pixels.
const UNIT_ICON_PX: f32 = 2.5;
const PROJECTILE_ICON_PX: f32 = 1.0;
/// Geometry with this icon size never collapses to a dot.
const NO_ICON: f32 = 0.0;

const BLUE: [f32; 3] = [0.20, 0.50, 1.00];
const RED: [f32; 3] = [1.00, 0.27, 0.22];
const SELECTED: [f32; 3] = [0.25, 1.00, 0.35];
const SHELL: [f32; 3] = [1.00, 0.85, 0.35];
const OVERCHARGE_SHELL: [f32; 3] = [0.75, 0.95, 1.00];
const OVERCHARGE_SIZE: f32 = 2.4;
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
    /// RGB, with the tactical dot's half-size in pixels in the last component.
    pub color: [f32; 4],
}

/// A unit's facing as a unit vector, which is where a shape's +Y axis points.
const NORTH: [f32; 2] = [0.0, 1.0];

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
    }
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

fn interpolate(previous: [i32; 3], current: [i32; 3], alpha: f64) -> [f64; 3] {
    let lerp = |from: i32, to: i32| (f64::from(from) + (f64::from(to) - f64::from(from)) * alpha) * RAW_TO_WORLD;
    [lerp(previous[0], current[0]), lerp(previous[1], current[1]), lerp(previous[2], current[2])]
}

/// Walks two lists sorted by id, calling `visit` with each current item and its previous
/// position if it had one. New entities are drawn where they are, without interpolation.
fn merge_by_id<T>(previous: &[T], current: &[T], id: impl Fn(&T) -> SimId, mut visit: impl FnMut(Option<&T>, &T)) {
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
}

/// Maps a blueprint to its look. Unknown keys fall back to a plain cube.
pub fn unit_look(key: &str) -> UnitLook {
    let look = |shape, tint, height_share| UnitLook { shape, tint, height_share, base: None };
    match key {
        "acu" => UnitLook { base: Some(Shape::CommanderLegs), ..look(Shape::Commander, Some([1.00, 1.00, 1.00]), 1.0) },
        "support_commander" => UnitLook { base: Some(Shape::CommanderLegs), ..look(Shape::Commander, Some([0.70, 0.75, 0.85]), 1.0) },
        "quantum_gateway" => look(Shape::Gateway, Some([0.55, 0.60, 1.00]), 0.6),
        "hydrocarbon_plant" => look(Shape::HydrocarbonPlant, Some([0.85, 0.60, 0.25]), 0.7),
        "mass_fabricator" => look(Shape::Fabricator, Some([0.40, 0.95, 0.85]), 1.0),
        "mass_fabricator_t3" => look(Shape::Fabricator, Some([0.70, 0.45, 1.00]), 0.8),
        "t2_engineer" => UnitLook { base: Some(Shape::EngineerHull), ..look(Shape::Engineer, Some([0.95, 0.65, 0.25]), 1.0) },
        "t3_engineer" => UnitLook { base: Some(Shape::EngineerHull), ..look(Shape::Engineer, Some([0.95, 0.50, 0.20]), 1.0) },
        "land_factory_t3" => look(Shape::Factory, Some([0.10, 0.10, 0.20]), 0.85),
        "t1_engineer" => UnitLook { base: Some(Shape::EngineerHull), ..look(Shape::Engineer, Some([0.95, 0.80, 0.30]), 1.0) },
        "t1_light_bot" => UnitLook { base: Some(Shape::BotLegs), ..look(Shape::Bot, None, 1.0) },
        "t1_light_artillery" => UnitLook { base: Some(Shape::ArtilleryHull), ..look(Shape::Artillery, None, 1.0) },
        "t2_heavy_tank" => UnitLook { base: Some(Shape::TankHull), ..look(Shape::Tank, None, 1.0) },
        "land_factory_t1" => look(Shape::Factory, Some([0.35, 0.35, 0.45]), 0.55),
        "land_factory_t2" => look(Shape::Factory, Some([0.20, 0.20, 0.30]), 0.7),
        "mass_extractor" => look(Shape::Extractor, Some([0.10, 0.95, 0.75]), 0.8),
        "mass_extractor_t2" => look(Shape::Extractor, Some([0.10, 0.75, 0.95]), 1.0),
        "mass_extractor_t3" => look(Shape::Extractor, Some([0.55, 0.40, 1.00]), 1.2),
        "power_generator" => look(Shape::Generator, Some([1.00, 0.85, 0.20]), 1.2),
        "mass_storage" => look(Shape::Storage, Some([0.60, 0.90, 0.60]), 0.65),
        "radar" => look(Shape::Radar, Some([0.40, 0.90, 0.55]), 1.3),
        "omni_sensor" => look(Shape::Radar, Some([0.75, 0.55, 1.00]), 1.6),
        "stealth_field_generator" => look(Shape::StealthGenerator, Some([0.30, 0.30, 0.35]), 1.2),
        "shield_generator" => look(Shape::ShieldGenerator, Some([0.55, 0.85, 1.00]), 1.0),
        "tactical_missile_launcher" => look(Shape::MissileLauncher, Some([0.85, 0.85, 0.80]), 0.9),
        "tactical_missile_defense" => look(Shape::MissileDefense, Some([1.00, 0.55, 0.45]), 1.2),
        "strategic_missile_launcher" => look(Shape::NukeSilo, Some([1.00, 0.90, 0.30]), 0.5),
        "strategic_missile_defense" => look(Shape::AntiNukeSilo, Some([0.95, 0.95, 0.95]), 0.7),
        "air_factory" => look(Shape::Factory, Some([0.45, 0.60, 0.80]), 0.5),
        "air_staging" => look(Shape::AirStaging, Some([0.70, 0.80, 0.90]), 0.5),
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
    blueprints.iter().map(|unit| unit_look(&unit.key)).collect()
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
    headings: &mut Headings,
    out: &mut InstanceBatches,
) {
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
        let mut color = match unit.team {
            Team::Blue => BLUE,
            Team::Red => RED,
        };
        let look = looks.get(usize::from(unit.blueprint));
        if let Some(tint) = look.and_then(|look| look.tint) {
            color = std::array::from_fn(|channel| color[channel] * 0.7 + tint[channel] * 0.3);
        }
        let shape = look.map_or(Shape::Cube, |look| look.shape);
        let size = (f64::from(unit.radius) * RAW_TO_WORLD * 2.0) as f32;
        let mut center = interpolate(from, now, alpha);
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
            let tinted = [color[0] * shade, color[1] * shade, color[2] * shade];
            // A structure's base stays put, and its gun, if any, turns to aim.
            if let Some(base) = look.and_then(|look| look.base) {
                out.push(base, instance(center, [size, size, height], NORTH, tinted, UNIT_ICON_PX * 1.6, eye));
            }
            let aim = unit_vector(unit.aim).unwrap_or(NORTH);
            out.push(shape, instance(center, [size, size, height], aim, tinted, UNIT_ICON_PX * 1.6, eye));
        } else {
            // The body faces the way the simulation says. Units that turn instantly, such as the
            // stress test's cubes, have no facing, so they are turned here as before.
            let (body, upper) = match body_facing(unit, before, alpha) {
                Some(body) => {
                    // Work comes first, then the weapon's aim, and otherwise straight ahead.
                    let rest = unit_vector(unit.aim).unwrap_or(body);
                    let upper = if look.is_some_and(|look| look.base.is_some()) { update_heading(headings, unit, before, working_on, Some(rest)) } else { body };
                    (body, upper)
                }
                None => {
                    // The stress test's plain cubes have no front, and skip this to save time.
                    let heading = if look.is_some() { update_heading(headings, unit, before, working_on, None) } else { NORTH };
                    (heading, heading)
                }
            };
            if let Some(base) = look.and_then(|look| look.base) {
                out.push(base, instance(center, [size; 3], body, color, UNIT_ICON_PX, eye));
            }
            out.push(shape, instance(center, [size; 3], upper, color, UNIT_ICON_PX, eye));
        }
    });
}

pub fn build_projectile_instances(previous: &[ProjectileView], current: &[ProjectileView], alpha: f64, eye: [f64; 3], out: &mut InstanceBatches) {
    merge_by_id(previous, current, |projectile| projectile.id, |before, projectile| {
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
            ProjectileKind::Shell => out.push(Shape::Shell, instance(center, [PROJECTILE_SIZE; 3], NORTH, SHELL, PROJECTILE_ICON_PX, eye)),
            ProjectileKind::Overcharge => out.push(Shape::Shell, instance(center, [OVERCHARGE_SIZE; 3], NORTH, OVERCHARGE_SHELL, UNIT_ICON_PX, eye)),
            ProjectileKind::TacticalMissile => out.push(Shape::Missile, instance(center, [3.0, 9.0, 3.0], heading, MISSILE, UNIT_ICON_PX, eye)),
            ProjectileKind::StrategicMissile => out.push(Shape::Missile, instance(center, [7.0, 20.0, 7.0], heading, NUKE, UNIT_ICON_PX * 1.6, eye)),
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

/// A hydrocarbon vent: a dark, wide patch on the ground.
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
        }
    }

    fn translation(instance: &UnitInstanceRaw) -> [f32; 3] {
        let column = instance.model_matrix[3];
        [column[0], column[1], column[2]]
    }

    /// Builds instances with no looks, so that everything lands in the cube batch.
    fn cubes(previous: &[UnitView], current: &[UnitView], alpha: f64, eye: [f64; 3], selected: &[SimId]) -> Vec<UnitInstanceRaw> {
        let mut out = InstanceBatches::default();
        build_unit_instances(previous, current, alpha, eye, selected, &[], &[], &mut Headings::new(), &mut out);
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
        build_unit_instances(&current, &current, 0.0, [0.0; 3], &[SimId(2)], &[], &[], &mut Headings::new(), &mut out);
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
        build_unit_instances(&[], &[generator], 0.0, [0.0; 3], &[SimId(1)], &looks, &[], &mut Headings::new(), &mut out);
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
        // Higher tiers of the factory and the engineer, the radar's omni upgrade, and the support
        // commander share the shape they come from, the air factory is a factory, the scout a small
        // fighter, and the anti-air turret has the mobile anti-air's guns. Everything else is unique.
        let shared = [
            "land_factory_t2",
            "land_factory_t3",
            "t2_engineer",
            "t3_engineer",
            "omni_sensor",
            "support_commander",
            "mass_extractor_t2",
            "mass_extractor_t3",
            "mass_fabricator_t3",
            "air_factory",
            "t1_air_scout",
            "t1_aa_turret",
        ];
        let shapes: Vec<Shape> = drawn().filter(|unit| !shared.contains(&unit.key.as_str())).map(|unit| unit_look(&unit.key).shape).collect();
        for (index, shape) in shapes.iter().enumerate() {
            assert!(!shapes[index + 1..].contains(shape), "{shape:?} is used twice");
        }
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
        build_unit_instances(&[], &[tank, generator], 0.0, [0.0; 3], &[], &looks, &[], &mut Headings::new(), &mut out);
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
        let mut headings = Headings::new();
        let mut draw = |before: UnitView, after: UnitView| {
            let mut out = InstanceBatches::default();
            build_unit_instances(&[before], &[after], 1.0, [0.0; 3], &[], &looks, &[], &mut headings, &mut out);
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
        let mut acu = unit(1, 0);
        acu.blueprint = db.id("acu").0;
        // Directly north-west of the unit, whichever way it was facing.
        let work = [WorkLinkView { builder: SimId(1), target: [-40_960, 40_960, 0], reclaim: false }];
        let mut headings = Headings::new();
        let mut draw = |before: UnitView, links: &[WorkLinkView]| {
            let mut out = InstanceBatches::default();
            build_unit_instances(&[before], &[acu], 1.0, [0.0; 3], &[], &looks, links, &mut headings, &mut out);
            facing(&out.shape(Shape::Commander)[0])
        };
        let start = draw(acu, &[]);
        assert!(start[0] > 0.0, "Blue starts out facing the middle of the map: {start:?}");
        let mut now = start;
        for _ in 0..60 {
            now = draw(acu, &work);
        }
        assert!((now[0] + FRAC_1_SQRT_2).abs() < 1e-2 && (now[1] - FRAC_1_SQRT_2).abs() < 1e-2, "should face its work: {now:?}");

        // Being nudged while working must not turn it away.
        let mut nudged = acu;
        nudged.x = -4096 * 4;
        let settled = draw(nudged, &work);
        assert!((settled[0] + FRAC_1_SQRT_2).abs() < 1e-2);
        assert_eq!(draw(acu, &[]), settled, "it keeps facing the same way when the work is done");
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
        let mut acu = unit(1, 0);
        acu.blueprint = db.id("acu").0;
        // Walking east, and building something to the north.
        acu.facing = [4096, 0];
        let work = [WorkLinkView { builder: SimId(1), target: [0, 40_960, 0], reclaim: false }];
        let mut headings = Headings::new();
        let mut out = InstanceBatches::default();
        for _ in 0..60 {
            out.clear();
            build_unit_instances(&[acu], &[acu], 1.0, [0.0; 3], &[], &looks, &work, &mut headings, &mut out);
        }
        let legs = facing(&out.shape(Shape::CommanderLegs)[0]);
        let body = facing(&out.shape(Shape::Commander)[0]);
        assert!((legs[0] - 1.0).abs() < 1e-3, "the legs should face east: {legs:?}");
        assert!((body[1] - 1.0).abs() < 1e-2, "the body should face its work to the north: {body:?}");

        // With the work done, the body settles back over its legs.
        for _ in 0..60 {
            out.clear();
            build_unit_instances(&[acu], &[acu], 1.0, [0.0; 3], &[], &looks, &[], &mut headings, &mut out);
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
        let mut headings = Headings::new();
        let mut out = InstanceBatches::default();
        for _ in 0..60 {
            out.clear();
            build_unit_instances(&[tank], &[tank], 1.0, [0.0; 3], &[], &looks, &[], &mut headings, &mut out);
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
        build_unit_instances(&[tower], &[tower], 1.0, [0.0; 3], &[], &looks, &[], &mut Headings::new(), &mut out);
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
        build_unit_instances(&[before], &[after], 0.5, [0.0; 3], &[], &looks, &[], &mut Headings::new(), &mut out);
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
        let mut headings = Headings::new();
        let units = [first, second];
        let mut out = InstanceBatches::default();
        for _ in 0..60 {
            out.clear();
            build_unit_instances(&units, &units, 1.0, [0.0; 3], &[], &looks, &work, &mut headings, &mut out);
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
