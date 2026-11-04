//! Drag-box selection (screen to world).

use crate::math::fp::{Fp, Vec2Fp};
use crate::render::camera::Camera;
use crate::sim::commands::SimCommand;
use crate::sim::components::{SimId, Team};
use crate::sim::world::UnitView;

/// Drags shorter than this, in pixels, count as a click.
pub const CLICK_THRESHOLD_PX: f64 = 4.0;
/// How far from a unit's center a click can land and still select it.
const CLICK_RADIUS_PX: f64 = 12.0;

/// Tracks the left-button drag and the current selection (sorted by `SimId`).
#[derive(Default)]
pub struct Selection {
    drag_start: Option<[f64; 2]>,
    cursor: [f64; 2],
    selected: Vec<SimId>,
}

impl Selection {
    pub fn selected(&self) -> &[SimId] {
        &self.selected
    }

    pub fn clear(&mut self) {
        self.selected.clear();
    }

    pub fn move_cursor(&mut self, cursor: [f64; 2]) {
        self.cursor = cursor;
    }

    pub const fn cursor(&self) -> [f64; 2] {
        self.cursor
    }

    pub fn begin_drag(&mut self) {
        self.drag_start = Some(self.cursor);
    }

    /// The current box as `(min, max)` in pixels, once the drag is long enough to be a box.
    pub fn drag_box(&self) -> Option<([f64; 2], [f64; 2])> {
        let start = self.drag_start?;
        let min = [start[0].min(self.cursor[0]), start[1].min(self.cursor[1])];
        let max = [start[0].max(self.cursor[0]), start[1].max(self.cursor[1])];
        (max[0] - min[0] >= CLICK_THRESHOLD_PX || max[1] - min[1] >= CLICK_THRESHOLD_PX).then_some((min, max))
    }

    /// Ends the drag, replacing the selection with the player's units in the box,
    /// or the nearest one if it was a click.
    pub fn end_drag(&mut self, units: &[UnitView], camera: &Camera, team: Team) {
        let dragged = self.drag_box();
        let Some(start) = self.drag_start.take() else {
            return;
        };
        self.selected = match dragged {
            Some((min, max)) => {
                // A box over a base and an army means the army.
                let found = units_in_box(units, camera, team, min, max);
                let is_mobile = |id: &SimId| {
                    units.binary_search_by_key(id, |unit| unit.id).is_ok_and(|index| !units[index].is_structure)
                };
                if found.iter().any(is_mobile) {
                    found.into_iter().filter(is_mobile).collect()
                } else {
                    found
                }
            }
            None => nearest_unit(units, camera, team, start).into_iter().collect(),
        };
    }

    /// Replaces the selection directly, for hotkeys and scripted runs. Ids must be sorted.
    pub fn select(&mut self, ids: Vec<SimId>) {
        self.selected = ids;
    }

    /// Removes units that have died, given the current units sorted by `SimId`.
    pub fn retain_alive(&mut self, units: &[UnitView]) {
        self.selected.retain(|id| units.binary_search_by_key(id, |unit| unit.id).is_ok());
    }
}

fn screen_position(unit: &UnitView, camera: &Camera) -> Option<[f64; 2]> {
    camera.world_to_screen([raw_to_world(unit.x), raw_to_world(unit.y), raw_to_world(unit.z)])
}

fn raw_to_world(raw: i32) -> f64 {
    f64::from(raw) / 4096.0
}

/// The team's units whose centers are inside a screen box, sorted by `SimId`.
pub fn units_in_box(units: &[UnitView], camera: &Camera, team: Team, min: [f64; 2], max: [f64; 2]) -> Vec<SimId> {
    units
        .iter()
        .filter(|unit| unit.team == team)
        .filter(|unit| {
            screen_position(unit, camera)
                .is_some_and(|p| p[0] >= min[0] && p[0] <= max[0] && p[1] >= min[1] && p[1] <= max[1])
        })
        .map(|unit| unit.id)
        .collect()
}

/// The team's unit closest to a screen point, if any is close enough.
pub fn nearest_unit(units: &[UnitView], camera: &Camera, team: Team, point: [f64; 2]) -> Option<SimId> {
    let mut best: Option<(f64, SimId)> = None;
    for unit in units.iter().filter(|unit| unit.team == team) {
        if let Some(p) = screen_position(unit, camera) {
            let distance = (p[0] - point[0]).powi(2) + (p[1] - point[1]).powi(2);
            // Large units can be clicked anywhere on them, as they appear on screen.
            let radius = raw_to_world(unit.radius);
            let edge = camera.world_to_screen([raw_to_world(unit.x) + radius, raw_to_world(unit.y), raw_to_world(unit.z)]);
            let apparent = edge.map_or(0.0, |edge| (edge[0] - p[0]).hypot(edge[1] - p[1]));
            let reach = CLICK_RADIUS_PX.max(apparent);
            if distance <= reach * reach && best.is_none_or(|(closest, _)| distance < closest) {
                best = Some((distance, unit.id));
            }
        }
    }
    best.map(|(_, id)| id)
}

/// Builds the order for a right-click. This is the one place a float becomes simulation
/// input. Every instance receives the resulting fixed-point command, so it stays deterministic.
pub fn order_command(units: &[SimId], ground: [f64; 2], attack_move: bool) -> SimCommand {
    let goal = Vec2Fp::new(Fp::from_num(ground[0]), Fp::from_num(ground[1]));
    let units = units.to_vec();
    if attack_move {
        SimCommand::AttackMove { units, goal }
    } else {
        SimCommand::Move { units, goal }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Camera {
        let mut camera = Camera::new(81_920.0, [1000.0, 800.0]);
        camera.look_at(40_000.0, 40_000.0, 0.0);
        camera
    }

    fn unit(id: u32, team: Team, x: f64, y: f64) -> UnitView {
        let raw = |value: f64| (value * 4096.0) as i32;
        UnitView {
            id: SimId(id),
            team,
            x: raw(x),
            y: raw(y),
            z: raw(1.0),
            radius: raw(1.0),
            blueprint: u16::MAX,
            health_permille: 1000,
            built_permille: 1000,
            is_structure: false,
        }
    }

    #[test]
    fn box_selects_only_visible_units_on_the_team() {
        let camera = camera();
        let center = camera.world_to_screen([40_000.0, 40_000.0, 1.0]).unwrap();
        let units = [
            unit(1, Team::Blue, 40_000.0, 40_000.0),
            unit(2, Team::Red, 40_000.0, 40_000.0),
            unit(3, Team::Blue, 79_000.0, 79_000.0),
        ];
        let min = [center[0] - 20.0, center[1] - 20.0];
        let max = [center[0] + 20.0, center[1] + 20.0];
        assert_eq!(units_in_box(&units, &camera, Team::Blue, min, max), [SimId(1)]);
    }

    #[test]
    fn short_drag_is_a_click_on_the_nearest_unit() {
        let camera = camera();
        let units = [unit(1, Team::Blue, 40_000.0, 40_000.0), unit(2, Team::Blue, 40_050.0, 40_000.0)];
        let at = camera.world_to_screen([40_000.0, 40_000.0, 1.0]).unwrap();
        let mut selection = Selection::default();
        selection.move_cursor(at);
        selection.begin_drag();
        selection.move_cursor([at[0] + 1.0, at[1]]);
        assert!(selection.drag_box().is_none());
        selection.end_drag(&units, &camera, Team::Blue);
        assert_eq!(selection.selected(), [SimId(1)]);
    }

    #[test]
    fn clicking_empty_ground_clears_the_selection() {
        let camera = camera();
        let units = [unit(1, Team::Blue, 40_000.0, 40_000.0)];
        let at = camera.world_to_screen([40_000.0, 40_000.0, 1.0]).unwrap();
        let mut selection = Selection::default();
        selection.move_cursor(at);
        selection.begin_drag();
        selection.end_drag(&units, &camera, Team::Blue);
        assert_eq!(selection.selected().len(), 1);
        selection.move_cursor([at[0] + 300.0, at[1]]);
        selection.begin_drag();
        selection.end_drag(&units, &camera, Team::Blue);
        assert!(selection.selected().is_empty());
    }

    #[test]
    fn dead_units_leave_the_selection() {
        let camera = camera();
        let units = [unit(1, Team::Blue, 40_000.0, 40_000.0)];
        let at = camera.world_to_screen([40_000.0, 40_000.0, 1.0]).unwrap();
        let mut selection = Selection::default();
        selection.move_cursor(at);
        selection.begin_drag();
        selection.end_drag(&units, &camera, Team::Blue);
        selection.retain_alive(&[]);
        assert!(selection.selected().is_empty());
    }
}
