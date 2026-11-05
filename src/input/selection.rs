//! Drag-box selection (screen to world).

use crate::math::fp::{Fp, Vec2Fp};
use crate::render::camera::Camera;
use crate::sim::commands::SimCommand;
use crate::sim::components::{SimId, Team};
use crate::sim::world::UnitView;

/// Drags shorter than this, in pixels, count as a click.
pub const CLICK_THRESHOLD_PX: f64 = 4.0;
/// Two clicks on a unit within this many seconds make a double click.
const DOUBLE_CLICK_SECONDS: f64 = 0.4;
/// How far from a unit's center a click can land and still select it.
const CLICK_RADIUS_PX: f64 = 12.0;

/// Tracks the left-button drag and the current selection (sorted by `SimId`).
#[derive(Default)]
pub struct Selection {
    drag_start: Option<[f64; 2]>,
    cursor: [f64; 2],
    selected: Vec<SimId>,
    /// The time, from `set_clock`, so that double clicks can be timed.
    clock: f64,
    last_click: Option<(SimId, f64)>,
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
        self.finish_drag(units, camera, team, false);
    }

    /// Ends the drag. When adding, a box adds to the selection, a click on a unit adds or
    /// removes it, and a click on empty ground leaves the selection alone.
    pub fn finish_drag(&mut self, units: &[UnitView], camera: &Camera, team: Team, adding: bool) {
        let dragged = self.drag_box();
        let Some(start) = self.drag_start.take() else {
            return;
        };
        if dragged.is_none() {
            let clicked = nearest_unit(units, camera, team, start);
            let previous = self.last_click.take();
            if let Some(clicked) = clicked {
                let is_double = previous.is_some_and(|(unit, time)| unit == clicked && self.clock - time <= DOUBLE_CLICK_SECONDS);
                if is_double {
                    self.select_type_on_screen(clicked, units, camera, team, adding);
                    return;
                }
                self.last_click = Some((clicked, self.clock));
            }
        } else {
            self.last_click = None;
        }
        if adding && dragged.is_none() {
            if let Some(clicked) = nearest_unit(units, camera, team, start) {
                match self.selected.binary_search(&clicked) {
                    Ok(index) => {
                        self.selected.remove(index);
                    }
                    Err(index) => self.selected.insert(index, clicked),
                }
            }
            return;
        }
        let previous = if adding { std::mem::take(&mut self.selected) } else { Vec::new() };
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
        if adding {
            self.selected.extend(previous);
            self.selected.sort_unstable();
            self.selected.dedup();
        }
    }

    /// Selects every unit of the same type as `clicked` that the player can see on screen.
    fn select_type_on_screen(&mut self, clicked: SimId, units: &[UnitView], camera: &Camera, team: Team, adding: bool) {
        let Ok(index) = units.binary_search_by_key(&clicked, |unit| unit.id) else {
            return;
        };
        let blueprint = units[index].blueprint;
        let viewport = camera.viewport();
        let same_type = units.iter().filter(|unit| unit.team == team && unit.blueprint == blueprint).filter(|unit| {
            screen_position(unit, camera).is_some_and(|p| p[0] >= 0.0 && p[0] <= viewport[0] && p[1] >= 0.0 && p[1] <= viewport[1])
        });
        if !adding {
            self.selected.clear();
        }
        self.selected.extend(same_type.map(|unit| unit.id));
        self.selected.sort_unstable();
        self.selected.dedup();
    }

    /// Replaces the selection directly, for hotkeys and scripted runs. Ids must be sorted.
    pub fn set_clock(&mut self, seconds: f64) {
        self.clock = seconds;
    }

    pub fn select(&mut self, ids: Vec<SimId>) {
        self.selected = ids;
        self.selected.sort_unstable();
        self.selected.dedup();
    }

    /// Adds to the selection, keeping what is already there.
    pub fn add(&mut self, ids: &[SimId]) {
        self.selected.extend_from_slice(ids);
        self.selected.sort_unstable();
        self.selected.dedup();
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
/// Saved selections, on the number keys. Group 1 is the key 1, and group 0 is the key 0.
#[derive(Clone, Debug, Default)]
pub struct ControlGroups {
    groups: [Vec<SimId>; 10],
}

impl ControlGroups {
    /// Replaces a group with these units. Group numbers run from 0 to 9.
    pub fn store(&mut self, group: usize, units: &[SimId]) {
        self.groups[group] = units.to_vec();
    }

    /// Forgets a group. Returns whether there was anything in it.
    pub fn clear(&mut self, group: usize) -> bool {
        let had_units = !self.groups[group].is_empty();
        self.groups[group].clear();
        had_units
    }

    /// The groups with living units, in keyboard order, with 0 last.
    pub fn in_use(&self, units: &[UnitView]) -> Vec<(usize, Vec<SimId>)> {
        (1..10)
            .chain(std::iter::once(0))
            .filter_map(|group| {
                let alive: Vec<SimId> = self.groups[group].iter().copied().filter(|id| units.binary_search_by_key(id, |unit| unit.id).is_ok()).collect();
                (!alive.is_empty()).then_some((group, alive))
            })
            .collect()
    }

    /// The group's units that are still alive, given the current units sorted by `SimId`.
    pub fn recall(&mut self, group: usize, units: &[UnitView]) -> &[SimId] {
        let members = &mut self.groups[group];
        members.retain(|id| units.binary_search_by_key(id, |unit| unit.id).is_ok());
        members
    }
}

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


    fn plain_unit(id: u32) -> UnitView {
        UnitView { id: SimId(id), team: Team::Blue, x: 0, y: 0, z: 0, radius: 4096, blueprint: u16::MAX, health_permille: 1000, built_permille: 1000, is_structure: false }
    }

    #[test]
    fn control_groups_keep_any_mix_of_units_apart_from_each_other() {
        let units: Vec<UnitView> = (1..=5).map(plain_unit).collect();
        let mut groups = ControlGroups::default();
        groups.store(1, &[SimId(1), SimId(4)]);
        groups.store(0, &[SimId(5)]);
        assert_eq!(groups.recall(1, &units), [SimId(1), SimId(4)]);
        assert_eq!(groups.recall(0, &units), [SimId(5)]);
        assert!(groups.recall(2, &units).is_empty());
        groups.store(1, &[SimId(2)]);
        assert_eq!(groups.recall(1, &units), [SimId(2)], "storing again replaces the group");
    }

    #[test]
    fn clearing_a_group_forgets_its_units_and_only_that_group() {
        let units: Vec<UnitView> = (1..=3).map(plain_unit).collect();
        let mut groups = ControlGroups::default();
        groups.store(1, &[SimId(1)]);
        groups.store(2, &[SimId(2)]);
        assert!(groups.clear(1));
        assert!(groups.recall(1, &units).is_empty());
        assert_eq!(groups.recall(2, &units), [SimId(2)]);
        assert!(!groups.clear(1), "there was nothing left to clear");
    }

    #[test]
    fn the_sidebar_lists_groups_in_keyboard_order_without_empty_or_dead_ones() {
        let units: Vec<UnitView> = (1..=3).map(plain_unit).collect();
        let mut groups = ControlGroups::default();
        assert!(groups.in_use(&units).is_empty());
        groups.store(0, &[SimId(3)]);
        groups.store(4, &[SimId(9)]);
        groups.store(2, &[SimId(1), SimId(9)]);
        assert_eq!(groups.in_use(&units), [(2, vec![SimId(1)]), (0, vec![SimId(3)])], "0 comes last, as on the keyboard");
    }

    #[test]
    fn dead_units_drop_out_of_control_groups() {
        let mut groups = ControlGroups::default();
        groups.store(3, &[SimId(1), SimId(2), SimId(3)]);
        let survivors = [plain_unit(1), plain_unit(3)];
        assert_eq!(groups.recall(3, &survivors), [SimId(1), SimId(3)]);
        assert!(groups.recall(3, &[]).is_empty());
    }

    #[test]
    fn recalling_a_group_can_add_to_a_selection_without_duplicates_and_stays_sorted() {
        let mut selection = Selection::default();
        selection.select(vec![SimId(3), SimId(1), SimId(3)]);
        assert_eq!(selection.selected(), [SimId(1), SimId(3)], "drawing relies on this being sorted");
        selection.add(&[SimId(2), SimId(3)]);
        assert_eq!(selection.selected(), [SimId(1), SimId(2), SimId(3)]);
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

    fn click(selection: &mut Selection, units: &[UnitView], camera: &Camera, at: [f64; 2], adding: bool) {
        selection.move_cursor(at);
        selection.begin_drag();
        selection.finish_drag(units, camera, Team::Blue, adding);
    }

    #[test]
    fn shift_click_adds_and_removes_units_one_at_a_time() {
        let camera = camera();
        let units = [unit(1, Team::Blue, 40_000.0, 40_000.0), unit(2, Team::Blue, 40_050.0, 40_000.0)];
        let first = camera.world_to_screen([40_000.0, 40_000.0, 1.0]).unwrap();
        let second = camera.world_to_screen([40_050.0, 40_000.0, 1.0]).unwrap();
        let mut selection = Selection::default();
        click(&mut selection, &units, &camera, second, true);
        click(&mut selection, &units, &camera, first, true);
        assert_eq!(selection.selected(), [SimId(1), SimId(2)], "the selection must stay sorted");
        click(&mut selection, &units, &camera, second, true);
        assert_eq!(selection.selected(), [SimId(1)], "a second click removes it");
        click(&mut selection, &units, &camera, [first[0] + 300.0, first[1]], true);
        assert_eq!(selection.selected(), [SimId(1)], "missing keeps what is selected");
    }

    #[test]
    fn shift_drag_adds_a_box_to_the_selection() {
        let camera = camera();
        let units = [unit(1, Team::Blue, 40_000.0, 40_000.0), unit(2, Team::Blue, 40_050.0, 40_000.0)];
        let first = camera.world_to_screen([40_000.0, 40_000.0, 1.0]).unwrap();
        let second = camera.world_to_screen([40_050.0, 40_000.0, 1.0]).unwrap();
        let mut selection = Selection::default();
        click(&mut selection, &units, &camera, first, false);
        selection.move_cursor([second[0] - 10.0, second[1] - 10.0]);
        selection.begin_drag();
        selection.move_cursor([second[0] + 10.0, second[1] + 10.0]);
        selection.finish_drag(&units, &camera, Team::Blue, true);
        assert_eq!(selection.selected(), [SimId(1), SimId(2)]);
    }

    /// Blue bots at 1 and 2, a Blue tank at 3, a Red bot at 4, and a Blue bot far off screen at 5.
    fn mixed_units() -> [UnitView; 5] {
        let typed = |id: u32, team: Team, blueprint: u16, x: f64| {
            let mut view = unit(id, team, x, 40_000.0);
            view.blueprint = blueprint;
            view
        };
        [
            typed(1, Team::Blue, 7, 40_000.0),
            typed(2, Team::Blue, 7, 40_050.0),
            typed(3, Team::Blue, 9, 40_100.0),
            typed(4, Team::Red, 7, 40_150.0),
            typed(5, Team::Blue, 7, 79_000.0),
        ]
    }

    #[test]
    fn double_click_selects_that_type_on_screen_only() {
        let camera = camera();
        let units = mixed_units();
        let at = camera.world_to_screen([40_000.0, 40_000.0, 1.0]).unwrap();
        let mut selection = Selection::default();
        selection.set_clock(10.0);
        click(&mut selection, &units, &camera, at, false);
        assert_eq!(selection.selected(), [SimId(1)], "one click selects one unit");
        selection.set_clock(10.2);
        click(&mut selection, &units, &camera, at, false);
        assert_eq!(selection.selected(), [SimId(1), SimId(2)], "not the tank, the enemy, or the unit off screen");
    }

    #[test]
    fn slow_clicks_and_clicks_on_different_units_are_not_a_double_click() {
        let camera = camera();
        let units = mixed_units();
        let first = camera.world_to_screen([40_000.0, 40_000.0, 1.0]).unwrap();
        let second = camera.world_to_screen([40_050.0, 40_000.0, 1.0]).unwrap();
        let mut selection = Selection::default();
        selection.set_clock(10.0);
        click(&mut selection, &units, &camera, first, false);
        selection.set_clock(11.0);
        click(&mut selection, &units, &camera, first, false);
        assert_eq!(selection.selected(), [SimId(1)], "too slow");
        selection.set_clock(11.1);
        click(&mut selection, &units, &camera, second, false);
        assert_eq!(selection.selected(), [SimId(2)], "a different unit");
    }

    #[test]
    fn shift_double_click_adds_the_type_to_the_selection() {
        let camera = camera();
        let units = mixed_units();
        let tank = camera.world_to_screen([40_100.0, 40_000.0, 1.0]).unwrap();
        let bot = camera.world_to_screen([40_000.0, 40_000.0, 1.0]).unwrap();
        let mut selection = Selection::default();
        selection.set_clock(10.0);
        click(&mut selection, &units, &camera, tank, false);
        selection.set_clock(11.0);
        click(&mut selection, &units, &camera, bot, true);
        selection.set_clock(11.2);
        click(&mut selection, &units, &camera, bot, true);
        assert_eq!(selection.selected(), [SimId(1), SimId(2), SimId(3)]);
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
