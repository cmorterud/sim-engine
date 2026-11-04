//! GPU instance data. Interpolates between two snapshots and converts to camera-relative `f32`.

use crate::sim::blueprints::{BlueprintDb, UnitClass};
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
/// Structures are flatter than they are wide.
const STRUCTURE_HEIGHT_SHARE: f32 = 0.6;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct UnitInstanceRaw {
    /// Interpolated, camera-relative 3D transform.
    pub model_matrix: [[f32; 4]; 4],
    /// RGB, with the tactical dot's half-size in pixels in the last component.
    pub color: [f32; 4],
}

fn instance(center: [f64; 3], size: [f32; 3], color: [f32; 3], icon_px: f32, eye: [f64; 3]) -> UnitInstanceRaw {
    UnitInstanceRaw {
        model_matrix: [
            [size[0], 0.0, 0.0, 0.0],
            [0.0, size[1], 0.0, 0.0],
            [0.0, 0.0, size[2], 0.0],
            [(center[0] - eye[0]) as f32, (center[1] - eye[1]) as f32, (center[2] - eye[2]) as f32, 1.0],
        ],
        color: [color[0], color[1], color[2], icon_px],
    }
}

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

/// Blends a role's color into a team's, so that buildings can be told apart at a glance
/// while their owner stays obvious. Unit classes without a tint use the plain team color.
pub fn role_tint(class: UnitClass, key: &str) -> Option<[f32; 3]> {
    match (class, key) {
        (UnitClass::Factory, _) => Some([0.35, 0.35, 0.45]),
        (UnitClass::Defense, _) => Some([0.75, 0.25, 0.85]),
        (UnitClass::Economy, "mass_extractor") => Some([0.10, 0.95, 0.75]),
        (UnitClass::Economy, "power_generator") => Some([1.00, 0.85, 0.20]),
        (UnitClass::Economy, _) => Some([0.60, 0.90, 0.60]),
        (UnitClass::Commander, _) => Some([1.00, 1.00, 1.00]),
        _ => None,
    }
}

/// Per-blueprint tints, indexed by raw blueprint id.
pub fn role_tints(blueprints: &BlueprintDb) -> Vec<Option<[f32; 3]>> {
    blueprints.iter().map(|unit| role_tint(unit.class, &unit.key)).collect()
}

#[allow(clippy::too_many_arguments)]
pub fn build_unit_instances(
    previous: &[UnitView],
    current: &[UnitView],
    alpha: f64,
    eye: [f64; 3],
    selected: &[SimId],
    tints: &[Option<[f32; 3]>],
    out: &mut Vec<UnitInstanceRaw>,
) {
    out.clear();
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
        if !is_selected {
            if let Some(Some(tint)) = tints.get(usize::from(unit.blueprint)) {
                color = std::array::from_fn(|channel| color[channel] * 0.45 + tint[channel] * 0.55);
            }
        }
        let size = (f64::from(unit.radius) * RAW_TO_WORLD * 2.0) as f32;
        let mut center = interpolate(from, now, alpha);
        if unit.is_structure {
            // A structure rises from the ground as it is built, and looks unfinished until then.
            let built = f32::from(unit.built_permille) / 1000.0;
            let height = size * STRUCTURE_HEIGHT_SHARE * (0.25 + 0.75 * built);
            center[2] += f64::from(height - size) * 0.5;
            let shade = 0.55 + 0.45 * built;
            let tinted = [color[0] * shade, color[1] * shade, color[2] * shade];
            out.push(instance(center, [size, size, height], tinted, UNIT_ICON_PX * 1.6, eye));
        } else {
            out.push(instance(center, [size; 3], color, UNIT_ICON_PX, eye));
        }
    });
}

pub fn build_projectile_instances(
    previous: &[ProjectileView],
    current: &[ProjectileView],
    alpha: f64,
    eye: [f64; 3],
    out: &mut Vec<UnitInstanceRaw>,
) {
    merge_by_id(previous, current, |projectile| projectile.id, |before, projectile| {
        let now = [projectile.x, projectile.y, projectile.z];
        let from = before.map_or(now, |before| [before.x, before.y, before.z]);
        out.push(instance(interpolate(from, now, alpha), [PROJECTILE_SIZE; 3], SHELL, PROJECTILE_ICON_PX, eye));
    });
}

/// A box obstacle as an instance. Obstacles keep their true size at every zoom.
pub fn obstacle_instance(min: [f64; 2], max: [f64; 2], base: f64, height: f64, eye: [f64; 3]) -> UnitInstanceRaw {
    let center = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5, base + height * 0.5];
    let size = [(max[0] - min[0]) as f32, (max[1] - min[1]) as f32, height as f32];
    instance(center, size, OBSTACLE, NO_ICON, eye)
}

/// A wreck: low, dark debris on the ground.
pub fn wreck_instance(center: [f64; 3], radius: f64, eye: [f64; 3]) -> UnitInstanceRaw {
    let width = (radius * 1.6) as f32;
    instance(center, [width, width, width * 0.4], WRECK, PROJECTILE_ICON_PX, eye)
}

/// A mass deposit: a flat marker that stays visible at every zoom.
pub fn deposit_instance(center: [f64; 3], width: f64, eye: [f64; 3]) -> UnitInstanceRaw {
    instance(center, [width as f32, width as f32, 1.0], DEPOSIT, UNIT_ICON_PX, eye)
}

/// A preview of a structure about to be placed.
pub fn ghost_instance(center: [f64; 3], width: f64, valid: bool, eye: [f64; 3]) -> UnitInstanceRaw {
    let color = if valid { GHOST_VALID } else { GHOST_INVALID };
    instance(center, [width as f32, width as f32, (width * 0.25) as f32], color, NO_ICON, eye)
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

    #[test]
    fn interpolates_matching_units_and_keeps_new_ones_in_place() {
        let previous = [unit(1, 0), unit(3, 4096)];
        let current = [unit(1, 4096), unit(2, 8192), unit(3, 8192)];
        let mut out = Vec::new();
        build_unit_instances(&previous, &current, 0.5, [0.0; 3], &[], &[], &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(translation(&out[0])[0], 0.5);
        assert_eq!(translation(&out[1])[0], 2.0);
        assert_eq!(translation(&out[2])[0], 1.5);
    }

    #[test]
    fn dead_units_are_not_drawn() {
        let mut out = Vec::new();
        build_unit_instances(&[unit(1, 0), unit(2, 0)], &[unit(2, 0)], 0.5, [0.0; 3], &[], &[], &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn selected_units_change_color() {
        let current = [unit(1, 0), unit(2, 0), unit(3, 0)];
        let mut out = Vec::new();
        build_unit_instances(&current, &current, 0.0, [0.0; 3], &[SimId(2)], &[], &mut out);
        assert_eq!(out[0].color[..3], BLUE);
        assert_eq!(out[1].color[..3], SELECTED);
        assert_eq!(out[2].color[..3], BLUE);
    }

    #[test]
    fn roles_tint_buildings_differently_but_keep_the_team_visible() {
        let db = BlueprintDb::standard();
        let tints = role_tints(&db);
        let building = |key: &str| {
            let mut view = unit(1, 0);
            view.blueprint = db.id(key).0;
            let mut out = Vec::new();
            build_unit_instances(&[], &[view], 0.0, [0.0; 3], &[], &tints, &mut out);
            out[0].color
        };
        let (factory, generator, extractor) = (building("land_factory_t1"), building("power_generator"), building("mass_extractor"));
        assert_ne!(factory, generator);
        assert_ne!(generator, extractor);
        assert_eq!(building("t1_light_bot")[..3], BLUE, "combat units keep the plain team color");
    }

    /// At the far edge of the largest map, adjacent raw positions must stay distinct after
    /// the conversion to `f32`, which needs the subtraction to happen in `f64`.
    #[test]
    fn camera_relative_conversion_keeps_precision_far_from_the_origin() {
        let far = 81_000 * 4096;
        let eye = [81_000.0, 0.0, 0.0];
        let mut out = Vec::new();
        build_unit_instances(&[], &[unit(1, far), unit(2, far + 1)], 0.0, eye, &[], &[], &mut out);
        let difference = translation(&out[1])[0] - translation(&out[0])[0];
        assert!((difference - 1.0 / 4096.0).abs() < 1e-6, "difference was {difference}");
    }
}
