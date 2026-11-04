//! GPU instance data. Interpolates between two snapshots and converts to camera-relative `f32`.

use std::collections::BTreeMap;
use std::f32::consts::FRAC_1_SQRT_2;

use super::meshes::Shape;
use crate::sim::blueprints::BlueprintDb;
use crate::sim::components::{SimId, Team};
use crate::sim::world::{ProjectileView, UnitView};

const RAW_TO_WORLD: f64 = 1.0 / 4096.0;
const PROJECTILE_SIZE: f32 = 0.6;

/// On-screen half-size of a unit's tactical dot, in pixels.
const UNIT_ICON_PX: f32 = 2.5;
const PROJECTILE_ICON_PX: f32 = 1.0;
/// Geometry with this icon size never collapses to a dot.
const NO_ICON: f32 = 0.0;

const BLUE: [f32; 3] = [0.20, 0.50, 1.00];
const RED: [f32; 3] = [1.00, 0.27, 0.22];
const SELECTED: [f32; 3] = [0.55, 1.00, 0.55];
const SHELL: [f32; 3] = [1.00, 0.85, 0.35];
const OBSTACLE: [f32; 3] = [0.42, 0.40, 0.38];
const WRECK: [f32; 3] = [0.24, 0.23, 0.22];
const DEPOSIT: [f32; 3] = [0.10, 0.75, 0.62];
const GHOST_VALID: [f32; 3] = [0.35, 0.95, 0.45];
const GHOST_INVALID: [f32; 3] = [0.95, 0.25, 0.20];
/// How far a unit must have moved in a tick, in raw squared units, to count as heading somewhere.
const HEADING_MIN_MOVE_SQUARED: f64 = 400.0 * 400.0;
/// How quickly a unit turns towards its new heading, per frame.
const HEADING_BLEND: f32 = 0.2;

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
#[derive(Default)]
pub struct InstanceBatches {
    by_shape: [Vec<UnitInstanceRaw>; Shape::COUNT],
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
}

/// Maps a blueprint to its look. Unknown keys fall back to a plain cube.
pub fn unit_look(key: &str) -> UnitLook {
    let look = |shape, tint, height_share| UnitLook { shape, tint, height_share };
    match key {
        "acu" => look(Shape::Commander, Some([1.00, 1.00, 1.00]), 1.0),
        "t1_engineer" => look(Shape::Engineer, Some([0.95, 0.80, 0.30]), 1.0),
        "t1_light_bot" => look(Shape::Bot, None, 1.0),
        "t1_light_artillery" => look(Shape::Artillery, None, 1.0),
        "t2_heavy_tank" => look(Shape::Tank, None, 1.0),
        "land_factory_t1" => look(Shape::Factory, Some([0.35, 0.35, 0.45]), 0.55),
        "land_factory_t2" => look(Shape::Factory, Some([0.20, 0.20, 0.30]), 0.7),
        "mass_extractor" => look(Shape::Extractor, Some([0.10, 0.95, 0.75]), 0.8),
        "power_generator" => look(Shape::Generator, Some([1.00, 0.85, 0.20]), 1.2),
        "mass_storage" => look(Shape::Storage, Some([0.60, 0.90, 0.60]), 0.65),
        "t1_point_defense" => look(Shape::PointDefense, Some([0.75, 0.25, 0.85]), 0.9),
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

/// Turns a unit towards the way it moved this tick. A unit that stops keeps its facing.
fn update_heading(headings: &mut Headings, unit: &UnitView, before: Option<&UnitView>) -> [f32; 2] {
    let heading = headings.entry(unit.id).or_insert_with(|| default_heading(unit.team));
    if let Some(before) = before {
        let (dx, dy) = (f64::from(unit.x - before.x), f64::from(unit.y - before.y));
        let moved_squared = dx * dx + dy * dy;
        if moved_squared > HEADING_MIN_MOVE_SQUARED {
            let length = moved_squared.sqrt();
            let target = [(dx / length) as f32, (dy / length) as f32];
            let blended = [
                heading[0] + (target[0] - heading[0]) * HEADING_BLEND,
                heading[1] + (target[1] - heading[1]) * HEADING_BLEND,
            ];
            let size = blended[0].hypot(blended[1]);
            // Turning exactly around cancels out, so jump straight there.
            *heading = if size > 0.01 { [blended[0] / size, blended[1] / size] } else { target };
        }
    }
    *heading
}

#[allow(clippy::too_many_arguments)]
pub fn build_unit_instances(
    previous: &[UnitView],
    current: &[UnitView],
    alpha: f64,
    eye: [f64; 3],
    selected: &[SimId],
    looks: &[UnitLook],
    headings: &mut Headings,
    out: &mut InstanceBatches,
) {
    let mut next_selected = 0;
    merge_by_id(previous, current, |unit| unit.id, |before, unit| {
        let now = [unit.x, unit.y, unit.z];
        let from = before.map_or(now, |before| [before.x, before.y, before.z]);
        while next_selected < selected.len() && selected[next_selected] < unit.id {
            next_selected += 1;
        }
        let is_selected = selected.get(next_selected) == Some(&unit.id);
        let mut color = match (is_selected, unit.team) {
            (true, _) => SELECTED,
            (false, Team::Blue) => BLUE,
            (false, Team::Red) => RED,
        };
        let look = looks.get(usize::from(unit.blueprint));
        if !is_selected {
            if let Some(tint) = look.and_then(|look| look.tint) {
                color = std::array::from_fn(|channel| color[channel] * 0.7 + tint[channel] * 0.3);
            }
        }
        let shape = look.map_or(Shape::Cube, |look| look.shape);
        let size = (f64::from(unit.radius) * RAW_TO_WORLD * 2.0) as f32;
        let mut center = interpolate(from, now, alpha);
        if unit.is_structure {
            // A structure rises from the ground as it is built, and looks unfinished until then.
            let built = f32::from(unit.built_permille) / 1000.0;
            let height_share = look.map_or(0.6, |look| look.height_share);
            let height = size * height_share * (0.25 + 0.75 * built);
            center[2] += f64::from(height - size) * 0.5;
            let shade = 0.55 + 0.45 * built;
            let tinted = [color[0] * shade, color[1] * shade, color[2] * shade];
            out.push(shape, instance(center, [size, size, height], NORTH, tinted, UNIT_ICON_PX * 1.6, eye));
        } else {
            // The stress test's plain cubes have no front, and skip this to save time.
            let heading = if look.is_some() { update_heading(headings, unit, before) } else { NORTH };
            out.push(shape, instance(center, [size; 3], heading, color, UNIT_ICON_PX, eye));
        }
    });
}

pub fn build_projectile_instances(previous: &[ProjectileView], current: &[ProjectileView], alpha: f64, eye: [f64; 3], out: &mut InstanceBatches) {
    merge_by_id(previous, current, |projectile| projectile.id, |before, projectile| {
        let now = [projectile.x, projectile.y, projectile.z];
        let from = before.map_or(now, |before| [before.x, before.y, before.z]);
        out.push(Shape::Shell, instance(interpolate(from, now, alpha), [PROJECTILE_SIZE; 3], NORTH, SHELL, PROJECTILE_ICON_PX, eye));
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

/// A mass deposit: a flat marker that stays visible at every zoom.
pub fn deposit_instance(center: [f64; 3], width: f64, eye: [f64; 3]) -> UnitInstanceRaw {
    instance(center, [width as f32, width as f32, 1.0], NORTH, DEPOSIT, UNIT_ICON_PX, eye)
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
        }
    }

    fn translation(instance: &UnitInstanceRaw) -> [f32; 3] {
        let column = instance.model_matrix[3];
        [column[0], column[1], column[2]]
    }

    /// Builds instances with no looks, so that everything lands in the cube batch.
    fn cubes(previous: &[UnitView], current: &[UnitView], alpha: f64, eye: [f64; 3], selected: &[SimId]) -> Vec<UnitInstanceRaw> {
        let mut out = InstanceBatches::default();
        build_unit_instances(previous, current, alpha, eye, selected, &[], &mut Headings::new(), &mut out);
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
    fn selected_units_change_color() {
        let current = [unit(1, 0), unit(2, 0), unit(3, 0)];
        let out = cubes(&current, &current, 0.0, [0.0; 3], &[SimId(2)]);
        assert_eq!(out[0].color[..3], BLUE);
        assert_eq!(out[1].color[..3], SELECTED);
        assert_eq!(out[2].color[..3], BLUE);
    }

    #[test]
    fn every_blueprint_has_its_own_look() {
        let db = BlueprintDb::standard();
        for unit in db.iter() {
            assert_ne!(unit_look(&unit.key).shape, Shape::Cube, "{} has no shape of its own", unit.key);
        }
        // The T1 and T2 factory share a shape, and everything else is unique.
        let shapes: Vec<Shape> = db.iter().filter(|unit| unit.key != "land_factory_t2").map(|unit| unit_look(&unit.key).shape).collect();
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
        build_unit_instances(&[], &[tank, generator], 0.0, [0.0; 3], &[], &looks, &mut Headings::new(), &mut out);
        assert_eq!(out.shape(Shape::Tank).len(), 1);
        assert_eq!(out.shape(Shape::Generator).len(), 1);
        assert_eq!(out.shape(Shape::Cube).len(), 0);
        assert_eq!(out.total(), 2);
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
            build_unit_instances(&[before], &[after], 1.0, [0.0; 3], &[], &looks, &mut headings, &mut out);
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
