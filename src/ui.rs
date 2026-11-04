//! The in-game interface: resource bar, build menu, unit overlays, and the end of match screen.
//! It reads snapshots and returns actions. It never touches the simulation.
#![allow(clippy::disallowed_types)]

use egui::{Align2, Color32, FontId, Pos2, Rect, Rounding, Stroke, Vec2};

use crate::input::feedback::{Marker, MarkerKind, OrderFeedback, LABEL_SECONDS};
use crate::input::orders::PendingBuild;
use crate::render::camera::Camera;
use crate::sim::blueprints::{BlueprintDb, BlueprintId, UnitClass};
use crate::sim::components::{SimId, Team};
use crate::sim::world::{MatchResult, PlanStepKind, SimSnapshot, UnitView};

/// Trails are capped so that a large selection stays readable and cheap to draw.
const MAX_TRAIL_UNITS: usize = 64;
const TRAIL_OPACITY: f32 = 0.85;
/// Rally lines are dimmer: they are a standing setting, not something under way.
const RALLY_OPACITY: f32 = 0.5;
const DASH_POINTS: f32 = 8.0;
const GAP_POINTS: f32 = 5.0;
/// Below this on-screen size, a terrain cell is too small for a placement grid to help.
const MIN_GRID_CELL_POINTS: f32 = 5.0;
/// The placement grid reaches this many cells from the cursor in each direction.
const GRID_REACH_CELLS: i32 = 8;

/// Above this altitude, units are drawn with tactical icons.
const ICON_ALTITUDE: f64 = 1_200.0;
const BLUE: Color32 = Color32::from_rgb(90, 160, 255);
const RED: Color32 = Color32::from_rgb(255, 95, 80);
const STALLED: Color32 = Color32::from_rgb(255, 110, 90);
const HEALTHY: Color32 = Color32::from_rgb(120, 230, 120);
const BUILDING: Color32 = Color32::from_rgb(120, 200, 255);

/// What the player asked for through the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiAction {
    BeginPlacement(BlueprintId),
    Queue { factory: SimId, blueprint: BlueprintId, count: u32 },
    SetRepeat { factory: SimId, repeat: bool },
    ClearQueue { factory: SimId },
    Restart,
}

fn find_unit(snapshot: &SimSnapshot, id: SimId) -> Option<&UnitView> {
    snapshot.units.binary_search_by_key(&id, |unit| unit.id).ok().map(|index| &snapshot.units[index])
}

fn cost_label(db: &BlueprintDb, blueprint: BlueprintId) -> String {
    let unit = db.get(blueprint);
    format!("{}\n{} mass  {} energy", unit.name, unit.mass.to_num::<i64>(), unit.energy.to_num::<i64>())
}

/// The bar across the top of the screen.
pub fn resource_bar(ctx: &egui::Context, snapshot: &SimSnapshot) {
    egui::TopBottomPanel::top("resources").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let economy = &snapshot.economy;
            let stalled = economy.satisfaction_permille < 1000;
            let line = |name: &str, stored: i64, capacity: i64, income: i64, demand: i64| {
                format!("{name} {stored}/{capacity}   +{income}/s  -{demand}/s")
            };
            let row = |ui: &mut egui::Ui, text: String, is_low: bool, base: Color32| {
                ui.colored_label(if is_low && stalled { STALLED } else { base }, text);
            };
            row(
                ui,
                line("Mass", economy.mass, economy.mass_capacity, economy.mass_income, economy.mass_demand),
                economy.mass == 0,
                Color32::from_rgb(120, 230, 170),
            );
            ui.separator();
            row(
                ui,
                line("Energy", economy.energy, economy.energy_capacity, economy.energy_income, economy.energy_demand),
                economy.energy == 0,
                Color32::from_rgb(250, 220, 110),
            );
            if stalled {
                ui.separator();
                ui.colored_label(STALLED, format!("STALL: building at {}%", economy.satisfaction_permille / 10));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let seconds = snapshot.tick / 10;
                ui.label(format!("{}:{:02}", seconds / 60, seconds % 60));
            });
        });
    });
}

/// The panel along the bottom: what is selected, and what it can build.
pub fn command_panel(ctx: &egui::Context, snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb, actions: &mut Vec<UiAction>) {
    if selected.is_empty() {
        return;
    }
    egui::TopBottomPanel::bottom("commands").min_height(64.0).show(ctx, |ui| {
        ui.add_space(4.0);
        let units: Vec<&UnitView> = selected.iter().filter_map(|id| find_unit(snapshot, *id)).collect();
        let Some(first) = units.first() else {
            return;
        };
        if first.blueprint == u16::MAX {
            ui.label(format!("{} units selected", units.len()));
            return;
        }
        let summary = if units.len() == 1 {
            let unit = db.get(BlueprintId(first.blueprint));
            format!("{}: {}% health", unit.name, first.health_permille / 10)
        } else {
            format!("{} units selected", units.len())
        };
        ui.strong(summary);

        // What all the selected builders can build, in the order of the first one's list.
        let builders: Vec<&&UnitView> = units
            .iter()
            .filter(|unit| {
                let blueprint = db.get(BlueprintId(unit.blueprint));
                !blueprint.is_factory() && !blueprint.builds.is_empty()
            })
            .collect();
        if let Some(builder) = builders.first() {
            ui.horizontal_wrapped(|ui| {
                for &option in &db.get(BlueprintId(builder.blueprint)).builds {
                    let everyone_can = builders.iter().all(|unit| db.get(BlueprintId(unit.blueprint)).builds.contains(&option));
                    if everyone_can && ui.button(cost_label(db, option)).clicked() {
                        actions.push(UiAction::BeginPlacement(option));
                    }
                }
            });
        }

        for unit in units.iter().filter(|unit| db.get(BlueprintId(unit.blueprint)).is_factory()).take(1) {
            let Some(factory) = snapshot.factories.iter().find(|factory| factory.id == unit.id) else {
                continue;
            };
            let blueprint = db.get(BlueprintId(unit.blueprint));
            if unit.built_permille < 1000 {
                ui.label("Under construction");
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                for &option in &blueprint.builds {
                    if ui.button(cost_label(db, option)).clicked() {
                        actions.push(UiAction::Queue { factory: unit.id, blueprint: option, count: 1 });
                    }
                }
                if let Some(upgrade) = blueprint.upgrades_to {
                    let upgrading = factory.queue.contains(&upgrade.0);
                    let label = format!("Upgrade to T2\n{}", cost_label(db, upgrade).lines().nth(1).unwrap_or(""));
                    if ui.add_enabled(!upgrading, egui::Button::new(label)).clicked() {
                        actions.push(UiAction::Queue { factory: unit.id, blueprint: upgrade, count: 1 });
                    }
                }
            });
            ui.horizontal(|ui| {
                let mut repeat = factory.repeat;
                if ui.checkbox(&mut repeat, "Repeat").changed() {
                    actions.push(UiAction::SetRepeat { factory: unit.id, repeat });
                }
                if ui.button("Clear queue").clicked() {
                    actions.push(UiAction::ClearQueue { factory: unit.id });
                }
                let names: Vec<&str> = factory.queue.iter().take(6).map(|item| db.get(BlueprintId(*item)).name.as_str()).collect();
                if names.is_empty() {
                    ui.label("Queue is empty. Right-click the ground to set a rally point.");
                } else {
                    ui.add(egui::ProgressBar::new(f32::from(factory.progress_permille) / 1000.0).desired_width(100.0));
                    ui.label(names.join(", "));
                }
            });
        }
    });
}

pub fn match_end_window(ctx: &egui::Context, result: MatchResult, player: Team, actions: &mut Vec<UiAction>) {
    let title = match result {
        MatchResult::Winner(team) if team == player => "Victory",
        MatchResult::Winner(_) => "Defeat",
        MatchResult::Draw => "Draw",
    };
    egui::Window::new(title)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(match title {
                "Victory" => "The enemy commander has been destroyed.",
                "Defeat" => "Your commander has been destroyed.",
                _ => "Both commanders were destroyed.",
            });
            if ui.button("Play again").clicked() {
                actions.push(UiAction::Restart);
            }
        });
}

/// A small floating reminder that the next click will attack-move.
pub fn attack_move_hint(ctx: &egui::Context) {
    egui::Area::new(egui::Id::new("attack move hint"))
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 36.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label("Attack-move: left-click a destination. Right-click or Esc to cancel.");
            });
        });
}

fn marker_color(kind: MarkerKind) -> Color32 {
    match kind {
        MarkerKind::Move => Color32::from_rgb(110, 240, 130),
        MarkerKind::AttackMove | MarkerKind::Attack => Color32::from_rgb(255, 80, 70),
        MarkerKind::Support => Color32::from_rgb(100, 180, 255),
        MarkerKind::Rally => Color32::WHITE,
        MarkerKind::Patrol => Color32::from_rgb(255, 220, 90),
        MarkerKind::Build => Color32::from_rgb(120, 240, 220),
        MarkerKind::Rejected => Color32::from_gray(170),
    }
}

/// Draws one marker: a ring that shrinks as it fades. The shapes differ as well as the colors,
/// so that orders can be told apart without relying on color.
fn draw_marker(painter: &egui::Painter, center: Pos2, marker: &Marker) {
    let progress = marker.progress() as f32;
    let radius = 28.0 - 16.0 * progress;
    let color = marker_color(marker.kind).gamma_multiply(1.0 - progress);
    let stroke = Stroke::new(2.5_f32, color);
    match marker.kind {
        MarkerKind::Rejected => {
            painter.circle_stroke(center, radius * 0.6, stroke);
            let reach = radius * 0.42;
            painter.line_segment([center - Vec2::splat(reach), center + Vec2::splat(reach)], stroke);
        }
        MarkerKind::AttackMove => {
            // A ring with ticks pointing in, like a gun sight.
            painter.circle_stroke(center, radius, stroke);
            for direction in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                painter.line_segment([center + direction * radius, center + direction * (radius * 0.55)], stroke);
            }
        }
        MarkerKind::Attack => {
            painter.circle_stroke(center, radius, stroke);
            painter.circle_stroke(center, radius * 0.55, stroke);
        }
        MarkerKind::Rally | MarkerKind::Patrol => {
            let corners = [Vec2::new(0.0, -radius), Vec2::new(radius, 0.0), Vec2::new(0.0, radius), Vec2::new(-radius, 0.0)];
            for index in 0..corners.len() {
                painter.line_segment([center + corners[index], center + corners[(index + 1) % corners.len()]], stroke);
            }
        }
        MarkerKind::Build => {
            painter.rect_stroke(Rect::from_center_size(center, Vec2::splat(radius * 1.6)), Rounding::ZERO, stroke);
        }
        MarkerKind::Move | MarkerKind::Support => {
            painter.circle_stroke(center, radius, stroke);
        }
    }
}

/// Shows that clicks were received: markers on the map, and a line of text below the top bar.
pub fn order_feedback(ctx: &egui::Context, camera: &Camera, snapshot: &SimSnapshot, feedback: &OrderFeedback, pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let raw = |value: i32| f64::from(value) / 4096.0;
    for marker in feedback.markers() {
        // A marker on a unit follows it, as long as the player can still see it.
        let followed = marker.target.and_then(|target| find_unit(snapshot, target));
        let position = followed.map_or(marker.position, |unit| [raw(unit.x), raw(unit.y), raw(unit.z)]);
        if let Some(pixels) = camera.world_to_screen(position) {
            draw_marker(&painter, Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point), marker);
        }
    }
    if let Some(label) = feedback.label() {
        // Stay solid for most of the time, then fade.
        let remaining = (1.0 - label.age / LABEL_SECONDS) as f32;
        let color = marker_color(label.kind).gamma_multiply((remaining * 3.0).min(1.0));
        egui::Area::new(egui::Id::new("order feedback"))
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 80.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).multiply_with_opacity((remaining * 3.0).min(1.0)).show(ui, |ui| {
                    ui.colored_label(color, egui::RichText::new(&label.text).strong());
                });
            });
    }
}

/// Cuts a screen-space line down to the part inside a rectangle (Liang-Barsky). A goal far
/// off-screen can project very far away, and dashing all of that would be wasted work.
pub fn clip_segment(from: Pos2, to: Pos2, bounds: Rect) -> Option<(Pos2, Pos2)> {
    let delta = to - from;
    let (mut start, mut end) = (0.0_f32, 1.0_f32);
    for (direction, offset) in [
        (-delta.x, from.x - bounds.min.x),
        (delta.x, bounds.max.x - from.x),
        (-delta.y, from.y - bounds.min.y),
        (delta.y, bounds.max.y - from.y),
    ] {
        if direction == 0.0 {
            if offset < 0.0 {
                return None;
            }
            continue;
        }
        let share = offset / direction;
        if direction < 0.0 {
            start = start.max(share);
        } else {
            end = end.min(share);
        }
        if start > end {
            return None;
        }
    }
    Some((from + delta * start, from + delta * end))
}

/// One leg of a trail, in world coordinates.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TrailLeg {
    pub from: [f64; 3],
    pub to: [f64; 3],
    pub kind: MarkerKind,
    pub opacity: f32,
    /// The position in the unit's plan, from 1. Zero if it is not worth numbering.
    pub number: usize,
}

fn trail_marker_kind(kind: PlanStepKind) -> MarkerKind {
    match kind {
        PlanStepKind::Move => MarkerKind::Move,
        PlanStepKind::AttackMove => MarkerKind::AttackMove,
        PlanStepKind::Attack => MarkerKind::Attack,
        PlanStepKind::Build => MarkerKind::Build,
        PlanStepKind::Support => MarkerKind::Support,
        PlanStepKind::Patrol => MarkerKind::Patrol,
    }
}

/// Works out the trails for the selected units. Each one starts at the unit, drawn where
/// the unit appears between ticks, and runs through its pending orders in sequence.
pub fn build_trails(previous: &SimSnapshot, current: &SimSnapshot, alpha: f64, selected: &[SimId]) -> Vec<TrailLeg> {
    let raw = |value: i32| f64::from(value) / 4096.0;
    let mut legs = Vec::new();
    let mut units_drawn = 0;
    for &id in selected {
        let Some(unit) = find_unit(current, id) else {
            continue;
        };
        let now = [raw(unit.x), raw(unit.y), raw(unit.z)];
        let before = find_unit(previous, id).map_or(now, |unit| [raw(unit.x), raw(unit.y), raw(unit.z)]);
        let start: [f64; 3] = std::array::from_fn(|axis| before[axis] + (now[axis] - before[axis]) * alpha);

        if let Some(rally) = current.factories.iter().find(|factory| factory.id == id).and_then(|factory| factory.rally) {
            legs.push(TrailLeg {
                from: start,
                to: [raw(rally[0]), raw(rally[1]), raw(rally[2])],
                kind: MarkerKind::Rally,
                opacity: RALLY_OPACITY,
                number: 0,
            });
        }
        let Ok(index) = current.plans.binary_search_by_key(&id, |plan| plan.id) else {
            continue;
        };
        if units_drawn == MAX_TRAIL_UNITS {
            continue;
        }
        units_drawn += 1;
        let steps = &current.plans[index].steps;
        let mut from = start;
        for (position, step) in steps.iter().enumerate() {
            let to = [raw(step.x), raw(step.y), raw(step.z)];
            legs.push(TrailLeg {
                from,
                to,
                kind: trail_marker_kind(step.kind),
                opacity: TRAIL_OPACITY,
                number: if steps.len() > 1 { position + 1 } else { 0 },
            });
            from = to;
        }
    }
    legs
}

/// Draws the selected units' trails: dashed lines through their pending orders, a dot at
/// each stop, and numbers when there are several.
pub fn order_trails(ctx: &egui::Context, camera: &Camera, legs: &[TrailLeg], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let to_points = |world: [f64; 3]| {
        camera.world_to_screen(world).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
    };
    let bounds = ctx.screen_rect().expand(16.0);
    for leg in legs {
        let color = marker_color(leg.kind).gamma_multiply(leg.opacity);
        let stroke = Stroke::new(2.0_f32, color);
        if let (Some(from), Some(to)) = (to_points(leg.from), to_points(leg.to)) {
            if let Some((start, end)) = clip_segment(from, to, bounds) {
                painter.extend(egui::Shape::dashed_line(&[start, end], stroke, DASH_POINTS, GAP_POINTS));
            }
        }
        let Some(end) = to_points(leg.to) else {
            continue;
        };
        if !bounds.contains(end) {
            continue;
        }
        painter.circle_filled(end, 2.5, color);
        if leg.number > 0 {
            painter.text(end + Vec2::new(6.0, -6.0), Align2::LEFT_BOTTOM, leg.number.to_string(), FontId::proportional(14.0), Color32::WHITE.gamma_multiply(leg.opacity));
        }
    }
}

/// Draws a dimmed footprint for every queued build, so that the next one can be lined up.
pub fn pending_build_outlines(ctx: &egui::Context, camera: &Camera, builds: &[PendingBuild], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let color = marker_color(MarkerKind::Build);
    let stroke = Stroke::new(1.5_f32, color.gamma_multiply(0.7));
    let fill = color.gamma_multiply(0.22);
    for build in builds {
        let half = build.width * 0.5;
        let [x, y, z] = build.center;
        let corners = [[x - half, y - half, z], [x + half, y - half, z], [x + half, y + half, z], [x - half, y + half, z]];
        let points: Vec<Pos2> = corners
            .into_iter()
            .filter_map(|corner| camera.world_to_screen(corner))
            .map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
            .collect();
        if points.len() == corners.len() {
            painter.add(egui::Shape::convex_polygon(points, fill, stroke));
        }
    }
}

/// The grid lines around a placement, in world coordinates at the given height. Lines
/// fall on terrain cell edges, which is where structures snap to.
pub fn placement_grid_lines(center_cell: (u32, u32), cell_units: f64, height: f64, cells_per_side: u32) -> Vec<([f64; 3], [f64; 3])> {
    let clamp = |cell: i32| f64::from(cell.clamp(0, cells_per_side as i32)) * cell_units;
    let (center_x, center_y) = (center_cell.0 as i32, center_cell.1 as i32);
    let (min_x, max_x) = (clamp(center_x - GRID_REACH_CELLS), clamp(center_x + GRID_REACH_CELLS));
    let (min_y, max_y) = (clamp(center_y - GRID_REACH_CELLS), clamp(center_y + GRID_REACH_CELLS));
    let mut lines = Vec::new();
    for offset in -GRID_REACH_CELLS..=GRID_REACH_CELLS {
        let (x, y) = (center_x + offset, center_y + offset);
        if (0..=cells_per_side as i32).contains(&x) {
            lines.push(([clamp(x), min_y, height], [clamp(x), max_y, height]));
        }
        if (0..=cells_per_side as i32).contains(&y) {
            lines.push(([min_x, clamp(y), height], [max_x, clamp(y), height]));
        }
    }
    lines
}

/// Draws a faint grid around the structure being placed, unless the camera is too far out for it to help.
pub fn placement_grid(ctx: &egui::Context, camera: &Camera, lines: &[([f64; 3], [f64; 3])], cell_units: f64, pixels_per_point: f32) {
    let to_points = |world: [f64; 3]| {
        camera.world_to_screen(world).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
    };
    let Some(first) = lines.first() else {
        return;
    };
    // Measure one cell along the first line, near the middle of the grid.
    let mid = |a: f64, b: f64| (a + b) * 0.5;
    let middle = [mid(first.0[0], first.1[0]), mid(first.0[1], first.1[1]), first.0[2]];
    let next = [middle[0], middle[1] + cell_units, middle[2]];
    let (Some(a), Some(b)) = (to_points(middle), to_points(next)) else {
        return;
    };
    if a.distance(b) < MIN_GRID_CELL_POINTS {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::background());
    let stroke = Stroke::new(1.0_f32, Color32::from_white_alpha(46));
    let bounds = ctx.screen_rect().expand(16.0);
    for &(from, to) in lines {
        if let (Some(from), Some(to)) = (to_points(from), to_points(to)) {
            if let Some((start, end)) = clip_segment(from, to, bounds) {
                painter.line_segment([start, end], stroke);
            }
        }
    }
}

/// A small floating reminder of the placement in progress.
pub fn placement_hint(ctx: &egui::Context, db: &BlueprintDb, blueprint: BlueprintId) {
    egui::Area::new(egui::Id::new("placement hint"))
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 36.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.label(format!("Placing {}: left-click to build, Shift-click to queue more, Shift-drag for a row, right-click to cancel", db.get(blueprint).name));
            });
        });
}

fn team_color(team: Team) -> Color32 {
    match team {
        Team::Blue => BLUE,
        Team::Red => RED,
    }
}

/// Draws a tactical icon: a frame in the team's color with a symbol for the unit's role.
fn draw_icon(painter: &egui::Painter, center: Pos2, class: UnitClass, color: Color32, selected: bool) {
    let (half_width, half_height) = match class {
        UnitClass::Commander => (11.0, 8.0),
        UnitClass::Factory => (9.0, 7.0),
        _ => (7.0, 5.0),
    };
    let frame = Rect::from_center_size(center, Vec2::new(half_width * 2.0, half_height * 2.0));
    let stroke = Stroke::new(1.5_f32, if selected { Color32::from_rgb(170, 255, 170) } else { color });
    painter.rect_filled(frame, Rounding::ZERO, Color32::from_black_alpha(150));
    painter.rect_stroke(frame, Rounding::ZERO, stroke);
    let corner = |x: f32, y: f32| center + Vec2::new(x * half_width, y * half_height);
    match class {
        // NATO symbols: an X for infantry, an ellipse for armor, a dot for artillery.
        UnitClass::Bot => {
            painter.line_segment([corner(-1.0, -1.0), corner(1.0, 1.0)], stroke);
            painter.line_segment([corner(-1.0, 1.0), corner(1.0, -1.0)], stroke);
        }
        UnitClass::Tank => {
            painter.line_segment([corner(-0.6, -0.5), corner(0.6, -0.5)], stroke);
            painter.line_segment([corner(-0.6, 0.5), corner(0.6, 0.5)], stroke);
            painter.line_segment([corner(-0.6, -0.5), corner(-0.6, 0.5)], stroke);
            painter.line_segment([corner(0.6, -0.5), corner(0.6, 0.5)], stroke);
        }
        UnitClass::Artillery => {
            painter.circle_filled(center, 2.0, stroke.color);
        }
        UnitClass::Engineer => {
            painter.line_segment([corner(-1.0, 0.0), corner(1.0, 0.0)], stroke);
            painter.line_segment([corner(-0.5, -0.6), corner(-0.5, 0.6)], stroke);
            painter.line_segment([corner(0.5, -0.6), corner(0.5, 0.6)], stroke);
        }
        UnitClass::Commander => {
            painter.circle_stroke(center, 4.0, stroke);
            painter.circle_filled(center, 1.5, stroke.color);
        }
        UnitClass::Factory => {
            painter.rect_filled(Rect::from_center_size(center, Vec2::new(half_width, half_height)), Rounding::ZERO, stroke.color);
        }
        UnitClass::Economy => {
            painter.line_segment([corner(-0.7, 0.7), corner(0.0, -0.7)], stroke);
            painter.line_segment([corner(0.0, -0.7), corner(0.7, 0.7)], stroke);
        }
        UnitClass::Defense => {
            painter.line_segment([corner(0.0, -1.0), corner(0.0, 1.0)], stroke);
            painter.line_segment([corner(-1.0, 0.0), corner(1.0, 0.0)], stroke);
        }
    }
}

fn draw_bar(painter: &egui::Painter, center: Pos2, width: f32, share: f32, color: Color32) {
    let back = Rect::from_center_size(center, Vec2::new(width, 3.0));
    painter.rect_filled(back, Rounding::ZERO, Color32::from_black_alpha(180));
    let fill = Rect::from_min_size(back.min, Vec2::new(width * share.clamp(0.0, 1.0), 3.0));
    painter.rect_filled(fill, Rounding::ZERO, color);
}

/// Health bars, construction progress, and tactical icons, drawn behind the panels.
pub fn world_overlay(ctx: &egui::Context, camera: &Camera, snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb, pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let show_icons = camera.altitude() >= ICON_ALTITUDE;
    let to_points = |pixels: [f64; 2]| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point);
    let raw = |value: i32| f64::from(value) / 4096.0;
    let viewport = ctx.screen_rect();
    for unit in &snapshot.units {
        if unit.blueprint == u16::MAX {
            continue;
        }
        let Some(pixels) = camera.world_to_screen([raw(unit.x), raw(unit.y), raw(unit.z)]) else {
            continue;
        };
        let center = to_points(pixels);
        if !viewport.expand(24.0).contains(center) {
            continue;
        }
        let is_selected = selected.binary_search(&unit.id).is_ok();
        let blueprint = db.get(BlueprintId(unit.blueprint));
        if show_icons {
            draw_icon(&painter, center, blueprint.class, team_color(unit.team), is_selected);
        }
        let hurt = unit.health_permille < 1000;
        let building = unit.built_permille < 1000;
        if hurt || building || is_selected {
            let edge = camera.world_to_screen([raw(unit.x) + raw(unit.radius), raw(unit.y), raw(unit.z)]);
            let apparent = edge.map_or(8.0, |edge| ((edge[0] - pixels[0]).hypot(edge[1] - pixels[1]) as f32 / pixels_per_point).max(8.0));
            let above = if show_icons { 12.0 } else { apparent + 4.0 };
            let width = if show_icons { 18.0 } else { (apparent * 2.0).clamp(14.0, 70.0) };
            let share = f32::from(unit.health_permille) / 1000.0;
            let color = if share > 0.5 { HEALTHY } else if share > 0.25 { Color32::YELLOW } else { STALLED };
            draw_bar(&painter, center - Vec2::new(0.0, above), width, share, color);
            if building {
                draw_bar(&painter, center - Vec2::new(0.0, above + 4.0), width, f32::from(unit.built_permille) / 1000.0, BUILDING);
            }
        }
    }
    if show_icons {
        painter.text(
            Pos2::new(8.0, viewport.bottom() - 8.0),
            Align2::LEFT_BOTTOM,
            format!("{} visible units", snapshot.units.len()),
            FontId::proportional(12.0),
            Color32::from_white_alpha(120),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::world::{FactoryView, PlanStep, UnitPlanView};

    fn raw(value: f64) -> i32 {
        (value * 4096.0) as i32
    }

    fn unit(id: u32, x: f64) -> UnitView {
        UnitView {
            id: SimId(id),
            team: Team::Blue,
            x: raw(x),
            y: 0,
            z: 0,
            radius: raw(8.0),
            blueprint: u16::MAX,
            health_permille: 1000,
            built_permille: 1000,
            is_structure: false,
        }
    }

    fn step(kind: PlanStepKind, x: f64, pending_width: f64) -> PlanStep {
        PlanStep { kind, x: raw(x), y: 0, z: 0, pending_width: raw(pending_width) }
    }

    fn snapshot(units: Vec<UnitView>, plans: Vec<UnitPlanView>) -> SimSnapshot {
        SimSnapshot { units, plans, ..SimSnapshot::default() }
    }

    #[test]
    fn placement_grid_falls_on_cell_edges_and_stays_on_the_map() {
        let lines = placement_grid_lines((50, 50), 20.0, 3.0, 512);
        assert_eq!(lines.len(), (GRID_REACH_CELLS as usize * 2 + 1) * 2);
        for (from, to) in &lines {
            for value in [from[0], from[1], to[0], to[1]] {
                assert_eq!(value % 20.0, 0.0, "{value} is not on a cell edge");
            }
            assert_eq!(from[2], 3.0);
        }
        // At the map's corner, lines that would fall outside it are dropped or cut short.
        for (from, to) in placement_grid_lines((1, 1), 20.0, 0.0, 512) {
            assert!(from[0] >= 0.0 && from[1] >= 0.0 && to[0] >= 0.0 && to[1] >= 0.0);
        }
    }

    #[test]
    fn a_builders_trail_runs_through_each_build_in_order() {
        let plan = UnitPlanView {
            id: SimId(1),
            steps: vec![step(PlanStepKind::Build, 100.0, 0.0), step(PlanStepKind::Build, 200.0, 60.0), step(PlanStepKind::Build, 300.0, 20.0)],
        };
        let current = snapshot(vec![unit(1, 0.0)], vec![plan]);
        let legs = build_trails(&current, &current, 1.0, &[SimId(1)]);
        let from: Vec<f64> = legs.iter().map(|leg| leg.from[0]).collect();
        let to: Vec<f64> = legs.iter().map(|leg| leg.to[0]).collect();
        assert_eq!(from, [0.0, 100.0, 200.0]);
        assert_eq!(to, [100.0, 200.0, 300.0]);
        let numbers: Vec<usize> = legs.iter().map(|leg| leg.number).collect();
        assert_eq!(numbers, [1, 2, 3]);
    }

    #[test]
    fn a_single_order_is_not_numbered() {
        let plan = UnitPlanView { id: SimId(1), steps: vec![step(PlanStepKind::Move, 100.0, 0.0)] };
        let current = snapshot(vec![unit(1, 0.0)], vec![plan]);
        let legs = build_trails(&current, &current, 1.0, &[SimId(1)]);
        assert_eq!(legs[0].number, 0);
        assert_eq!(legs[0].kind, MarkerKind::Move);
    }

    #[test]
    fn only_selected_units_have_trails() {
        let plan = UnitPlanView { id: SimId(1), steps: vec![step(PlanStepKind::Move, 100.0, 0.0)] };
        let current = snapshot(vec![unit(1, 0.0)], vec![plan]);
        assert!(build_trails(&current, &current, 1.0, &[]).is_empty());
    }

    #[test]
    fn a_factorys_rally_line_is_dimmer_than_an_order() {
        let factory = FactoryView { id: SimId(2), queue: vec![], repeat: false, progress_permille: 0, rally: Some([raw(500.0), 0, 0]) };
        let plan = UnitPlanView { id: SimId(1), steps: vec![step(PlanStepKind::Move, 100.0, 0.0)] };
        let mut current = snapshot(vec![unit(1, 0.0), unit(2, 50.0)], vec![plan]);
        current.factories = vec![factory];
        let legs = build_trails(&current, &current, 1.0, &[SimId(1), SimId(2)]);
        let rally = legs.iter().find(|leg| leg.kind == MarkerKind::Rally).expect("the factory has a rally line");
        let order = legs.iter().find(|leg| leg.kind == MarkerKind::Move).unwrap();
        assert_eq!(rally.to[0], 500.0);
        assert!(rally.opacity < order.opacity);
    }

    #[test]
    fn a_trail_starts_where_the_unit_appears_between_ticks() {
        let plan = UnitPlanView { id: SimId(1), steps: vec![step(PlanStepKind::Move, 100.0, 0.0)] };
        let previous = snapshot(vec![unit(1, 0.0)], vec![]);
        let current = snapshot(vec![unit(1, 10.0)], vec![plan]);
        let legs = build_trails(&previous, &current, 0.5, &[SimId(1)]);
        assert_eq!(legs[0].from[0], 5.0);
    }

    #[test]
    fn large_selections_are_capped() {
        let units: Vec<UnitView> = (0..200).map(|id| unit(id, 0.0)).collect();
        let plans: Vec<UnitPlanView> =
            (0..200).map(|id| UnitPlanView { id: SimId(id), steps: vec![step(PlanStepKind::Move, 100.0, 0.0)] }).collect();
        let selected: Vec<SimId> = (0..200).map(SimId).collect();
        let current = snapshot(units, plans);
        assert_eq!(build_trails(&current, &current, 1.0, &selected).len(), MAX_TRAIL_UNITS);
    }

    #[test]
    fn segments_clip_to_the_screen() {
        let bounds = Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0));
        let inside = (Pos2::new(10.0, 10.0), Pos2::new(20.0, 20.0));
        assert_eq!(clip_segment(inside.0, inside.1, bounds), Some(inside));
        let across = clip_segment(Pos2::new(50.0, 50.0), Pos2::new(1_000_000.0, 50.0), bounds).unwrap();
        assert_eq!(across, (Pos2::new(50.0, 50.0), Pos2::new(100.0, 50.0)));
        assert_eq!(clip_segment(Pos2::new(200.0, 0.0), Pos2::new(300.0, 50.0), bounds), None);
    }
}
