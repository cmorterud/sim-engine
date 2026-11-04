//! GPU instance data. Interpolates between two snapshots and converts to camera-relative `f32`.

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

pub fn build_unit_instances(
    previous: &[UnitView],
    current: &[UnitView],
    alpha: f64,
    eye: [f64; 3],
    selected: &[SimId],
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
        let color = match (is_selected, unit.team) {
            (true, _) => SELECTED,
            (false, Team::Blue) => BLUE,
            (false, Team::Red) => RED,
        };
        let size = (f64::from(unit.radius) * RAW_TO_WORLD * 2.0) as f32;
        out.push(instance(interpolate(from, now, alpha), [size; 3], color, UNIT_ICON_PX, eye));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(id: u32, x: i32) -> UnitView {
        UnitView { id: SimId(id), team: Team::Blue, x, y: 0, z: 0, radius: 4096 }
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
        build_unit_instances(&previous, &current, 0.5, [0.0; 3], &[], &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(translation(&out[0])[0], 0.5);
        assert_eq!(translation(&out[1])[0], 2.0);
        assert_eq!(translation(&out[2])[0], 1.5);
    }

    #[test]
    fn dead_units_are_not_drawn() {
        let mut out = Vec::new();
        build_unit_instances(&[unit(1, 0), unit(2, 0)], &[unit(2, 0)], 0.5, [0.0; 3], &[], &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn selected_units_change_color() {
        let current = [unit(1, 0), unit(2, 0), unit(3, 0)];
        let mut out = Vec::new();
        build_unit_instances(&current, &current, 0.0, [0.0; 3], &[SimId(2)], &mut out);
        assert_eq!(out[0].color[..3], BLUE);
        assert_eq!(out[1].color[..3], SELECTED);
        assert_eq!(out[2].color[..3], BLUE);
    }

    /// At the far edge of the largest map, adjacent raw positions must stay distinct after
    /// the conversion to `f32`, which needs the subtraction to happen in `f64`.
    #[test]
    fn camera_relative_conversion_keeps_precision_far_from_the_origin() {
        let far = 81_000 * 4096;
        let eye = [81_000.0, 0.0, 0.0];
        let mut out = Vec::new();
        build_unit_instances(&[], &[unit(1, far), unit(2, far + 1)], 0.0, eye, &[], &mut out);
        let difference = translation(&out[1])[0] - translation(&out[0])[0];
        assert!((difference - 1.0 / 4096.0).abs() < 1e-6, "difference was {difference}");
    }
}
