//! The in-game interface: resource bar, build menu, unit overlays, and the end of match screen.
//! It reads snapshots and returns actions. It never touches the simulation.
#![allow(clippy::disallowed_types)]

use egui::{Align2, Color32, FontId, Pos2, Rect, Rounding, Stroke, Vec2};

use crate::input::feedback::{Marker, MarkerKind, OrderFeedback, LABEL_SECONDS};
use crate::input::hotkeys::{self, blueprint_hotkey};
use crate::input::orders::PendingBuild;
use crate::render::camera::Camera;
use crate::sim::blueprints::{BlueprintDb, BlueprintId, UnitClass};
use crate::sim::components::{BuildTask, SimId, Team};
use crate::sim::world::{MatchResult, PlanStepKind, SimSnapshot, UnitView};

/// Trails are capped so that a large selection stays readable and cheap to draw.
const MAX_TRAIL_UNITS: usize = 64;
const ALL_ORDERS_UNIT_LIMIT: usize = 256;
const TRAIL_OPACITY: f32 = 0.85;
/// Rally lines are dimmer: they are a standing setting, not something under way.
const RALLY_OPACITY: f32 = 0.5;
const DASH_POINTS: f32 = 8.0;
const GAP_POINTS: f32 = 5.0;
/// Below this on-screen size, a terrain cell is too small for a placement grid to help.
const MIN_GRID_CELL_POINTS: f32 = 5.0;
/// Below this on-screen radius, a selected unit's ground ring is too small to see.
const MIN_SELECTION_RING_POINTS: f32 = 4.0;
/// The placement grid reaches this many cells from the cursor in each direction.
const GRID_REACH_CELLS: i32 = 8;

/// Above this altitude, units are drawn with tactical icons.
const ICON_ALTITUDE: f64 = 1_200.0;
const BLUE: Color32 = Color32::from_rgb(90, 160, 255);
const RED: Color32 = Color32::from_rgb(255, 95, 80);
const STALLED: Color32 = Color32::from_rgb(255, 110, 90);
const HEALTHY: Color32 = Color32::from_rgb(120, 230, 120);
const BUILDING: Color32 = Color32::from_rgb(120, 200, 255);
/// Warm, so that beams stand out against both blue structures and the terrain.
const BEAM_BUILD: Color32 = Color32::from_rgb(255, 215, 110);
const BEAM_RECLAIM: Color32 = Color32::from_rgb(150, 255, 170);
/// How tall the queue can grow before it scrolls, in points.
const QUEUE_MAX_HEIGHT: f32 = 60.0;
/// How many pulses are on a work beam at once.
const BEAM_PULSES: usize = 5;
/// How long a pulse takes to travel along a beam, in seconds.
const BEAM_PULSE_SECONDS: f64 = 1.1;
/// A beam shorter than this on screen is only clutter.
const MIN_BEAM_POINTS: f32 = 6.0;

/// What the player asked for through the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiAction {
    BeginPlacement(BlueprintId),
    /// Queue one item in each of these factories, out of `selected_count` selected.
    Queue { factories: Vec<SimId>, blueprint: BlueprintId, selected_count: usize },
    SetRepeat { factories: Vec<SimId>, repeat: bool },
    ClearQueue { factories: Vec<SimId> },
    /// Remove one order from these builders' queues.
    CancelTask { units: Vec<SimId>, task: BuildTask },
    /// Remove one of an item from each of these factories' queues.
    RemoveQueued { factories: Vec<SimId>, blueprint: BlueprintId },
    Restart,
}

fn find_unit(snapshot: &SimSnapshot, id: SimId) -> Option<&UnitView> {
    snapshot.units.binary_search_by_key(&id, |unit| unit.id).ok().map(|index| &snapshot.units[index])
}

/// A button's name and cost, with its key in front.
fn cost_label(db: &BlueprintDb, blueprint: BlueprintId) -> String {
    let unit = db.get(blueprint);
    let key = blueprint_hotkey(&unit.key).map_or(String::new(), |letter| format!("[{letter}] "));
    format!("{key}{}\n{} mass  {} energy", unit.name, unit.mass.to_num::<i64>(), unit.energy.to_num::<i64>())
}

/// What all the selected builders can build, in the order of the first one's list.
fn builder_options(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Vec<BlueprintId> {
    let builders: Vec<&UnitView> = selected
        .iter()
        .filter_map(|id| find_unit(snapshot, *id))
        .filter(|unit| {
            unit.blueprint != u16::MAX && {
                let blueprint = db.get(BlueprintId(unit.blueprint));
                !blueprint.is_factory() && !blueprint.builds.is_empty()
            }
        })
        .collect();
    let Some(first) = builders.first() else {
        return Vec::new();
    };
    db.get(BlueprintId(first.blueprint))
        .builds
        .iter()
        .copied()
        .filter(|option| builders.iter().all(|unit| db.get(BlueprintId(unit.blueprint)).builds.contains(option)))
        .collect()
}

fn queue_action(menu: &FactoryMenu, able: &[SimId], blueprint: BlueprintId) -> UiAction {
    UiAction::Queue { factories: able.to_vec(), blueprint, selected_count: menu.factories.len() }
}

/// A mixed selection turns on for all, as the next press turns off for all.
fn repeat_action(menu: &FactoryMenu) -> UiAction {
    UiAction::SetRepeat { factories: menu.factories.clone(), repeat: menu.repeat != RepeatState::On }
}

fn clear_action(menu: &FactoryMenu) -> UiAction {
    UiAction::ClearQueue { factories: menu.factories.clone() }
}

/// What a command panel key does for this selection: the same as its button, or `None`
/// if the panel has no such option right now.
pub fn hotkey_action(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb, letter: char) -> Option<UiAction> {
    let is_key = |blueprint: BlueprintId| blueprint_hotkey(&db.get(blueprint).key) == Some(letter);
    if let Some(option) = builder_options(snapshot, selected, db).into_iter().find(|option| is_key(*option)) {
        return Some(UiAction::BeginPlacement(option));
    }
    let menu = factory_menu(snapshot, selected, db)?;
    if let Some((option, able)) = menu.options.iter().find(|(option, _)| is_key(*option)) {
        return Some(queue_action(&menu, able, *option));
    }
    match letter {
        hotkeys::UPGRADE => menu.upgrade.as_ref().map(|(upgrade, able)| queue_action(&menu, able, *upgrade)),
        hotkeys::REPEAT => Some(repeat_action(&menu)),
        hotkeys::CLEAR_QUEUE => Some(clear_action(&menu)),
        _ => None,
    }
}

/// One entry in the queue shown for the selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueChip {
    pub label: String,
    /// What right-clicking it does.
    pub remove: UiAction,
}

/// What a builder's order is called on screen.
pub fn task_name(task: &BuildTask, db: &BlueprintDb) -> String {
    match *task {
        BuildTask::Build { blueprint, .. } => db.get(blueprint).name.clone(),
        BuildTask::Assist { .. } => "Assist".to_string(),
        BuildTask::Reclaim { .. } => "Reclaim".to_string(),
        BuildTask::Repair { .. } => "Repair".to_string(),
    }
}

/// The queue for the selection: builders' orders in order, then factories' items. An order
/// that several selected units share appears once, and removing it removes it from them all.
pub fn queue_chips(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Vec<QueueChip> {
    let mut orders: Vec<(BuildTask, Vec<SimId>)> = Vec::new();
    for &id in selected {
        let Ok(index) = snapshot.build_queues.binary_search_by_key(&id, |queue| queue.id) else {
            continue;
        };
        for task in &snapshot.build_queues[index].tasks {
            match orders.iter_mut().find(|(existing, _)| existing.is_same_order(task)) {
                Some((_, units)) => units.push(id),
                None => orders.push((*task, vec![id])),
            }
        }
    }
    let mut chips: Vec<QueueChip> = orders
        .into_iter()
        .enumerate()
        .map(|(position, (task, units))| {
            let built = match task {
                BuildTask::Build { site: Some(site), .. } => find_unit(snapshot, site).map_or(String::new(), |site| format!(" {}%", site.built_permille / 10)),
                _ => String::new(),
            };
            QueueChip { label: format!("{}. {}{built}", position + 1, task_name(&task, db)), remove: UiAction::CancelTask { units, task } }
        })
        .collect();

    if let Some(menu) = factory_menu(snapshot, selected, db) {
        for &(item, count) in &menu.queue {
            let holding = |id: &&SimId| snapshot.factories.iter().any(|factory| factory.id == **id && factory.queue.contains(&item.0));
            let factories: Vec<SimId> = menu.factories.iter().filter(holding).copied().collect();
            let name = &db.get(item).name;
            let label = if count > 1 { format!("{name} x{count}") } else { name.clone() };
            chips.push(QueueChip { label, remove: UiAction::RemoveQueued { factories, blueprint: item } });
        }
    }
    chips
}

fn queue_chip_row(ui: &mut egui::Ui, chips: &[QueueChip], actions: &mut Vec<UiAction>) {
    // Long queues scroll, so that the panel cannot cover the map.
    egui::ScrollArea::vertical().max_height(QUEUE_MAX_HEIGHT).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for chip in chips {
                if ui.button(&chip.label).on_hover_text("Right-click to remove").secondary_clicked() {
                    actions.push(chip.remove.clone());
                }
            }
        });
    });
}

/// Nothing in the game takes typing. Without this, Tab would give a button keyboard focus,
/// and it would then swallow every key, so that hotkeys seem to stop working.
pub fn release_keyboard_focus(ctx: &egui::Context) {
    ctx.memory_mut(|memory| {
        if let Some(focused) = memory.focused() {
            memory.surrender_focus(focused);
        }
    });
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

        let options = builder_options(snapshot, selected, db);
        if !options.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for &option in &options {
                    if ui.button(cost_label(db, option)).clicked() {
                        actions.push(UiAction::BeginPlacement(option));
                    }
                }
            });
        }

        if let Some(menu) = factory_menu(snapshot, selected, db) {
            factory_controls(ui, &menu, db, actions);
        }

        let chips = queue_chips(snapshot, selected, db);
        if !chips.is_empty() {
            ui.label("Queue (right-click to remove):");
            queue_chip_row(ui, &chips, actions);
        }
    });
}

/// Whether selected factories are set to repeat.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RepeatState {
    Off,
    On,
    Mixed,
}

/// What the selected factories can do together. Unfinished factories take orders too,
/// and start on them once they are built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactoryMenu {
    pub factories: Vec<SimId>,
    /// Everything at least one factory can build, with the factories able to build it.
    pub options: Vec<(BlueprintId, Vec<SimId>)>,
    /// The upgrade, with the factories able to start it.
    pub upgrade: Option<(BlueprintId, Vec<SimId>)>,
    pub repeat: RepeatState,
    /// The combined queue, in order of first appearance, with how many of each.
    pub queue: Vec<(BlueprintId, u32)>,
    /// Average progress on items under way, in thousandths, if any are.
    pub progress_permille: Option<u16>,
    /// How many of the factories are still under construction.
    pub unfinished: usize,
}

/// Works out the shared menu for the selected factories. `None` if no factory is selected.
pub fn factory_menu(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Option<FactoryMenu> {
    let mut menu = FactoryMenu {
        factories: Vec::new(),
        options: Vec::new(),
        upgrade: None,
        repeat: RepeatState::Off,
        queue: Vec::new(),
        progress_permille: None,
        unfinished: 0,
    };
    let (mut repeating, mut progress_total, mut under_way) = (0usize, 0u32, 0u32);
    for &id in selected {
        let Some(unit) = find_unit(snapshot, id).filter(|unit| unit.blueprint != u16::MAX) else {
            continue;
        };
        let blueprint = db.get(BlueprintId(unit.blueprint));
        let Some(factory) = snapshot.factories.iter().find(|factory| factory.id == id).filter(|_| blueprint.is_factory()) else {
            continue;
        };
        let finished = unit.built_permille == 1000;
        menu.factories.push(id);
        menu.unfinished += usize::from(!finished);
        repeating += usize::from(factory.repeat);
        // With an upgrade queued, it can also queue what the upgrade will let it build.
        let queued_upgrade = blueprint.upgrades_to.filter(|upgrade| factory.queue.contains(&upgrade.0));
        let after_upgrade = queued_upgrade.iter().flat_map(|upgrade| &db.get(*upgrade).builds);
        for &option in blueprint.builds.iter().chain(after_upgrade) {
            match menu.options.iter_mut().find(|(existing, _)| *existing == option) {
                Some((_, able)) if !able.contains(&id) => able.push(id),
                Some(_) => {}
                None => menu.options.push((option, vec![id])),
            }
        }
        if let Some(upgrade) = blueprint.upgrades_to.filter(|upgrade| !factory.queue.contains(&upgrade.0)) {
            menu.upgrade.get_or_insert_with(|| (upgrade, Vec::new())).1.push(id);
        }
        for &item in &factory.queue {
            let item = BlueprintId(item);
            match menu.queue.iter_mut().find(|(existing, _)| *existing == item) {
                Some((_, count)) => *count += 1,
                None => menu.queue.push((item, 1)),
            }
        }
        if finished && !factory.queue.is_empty() {
            progress_total += u32::from(factory.progress_permille);
            under_way += 1;
        }
    }
    if menu.factories.is_empty() {
        return None;
    }
    menu.repeat = match repeating {
        0 => RepeatState::Off,
        count if count == menu.factories.len() => RepeatState::On,
        _ => RepeatState::Mixed,
    };
    menu.progress_permille = (under_way > 0).then(|| (progress_total / under_way) as u16);
    Some(menu)
}

/// Says when queued units will start, if any selected factory is still being built.
pub fn construction_note(unfinished: usize, total: usize) -> Option<String> {
    match unfinished {
        0 => None,
        _ if unfinished == total => Some("Under construction: the queue starts once built.".to_string()),
        _ => Some(format!("{unfinished} of {total} under construction: their queues start once built.")),
    }
}

/// A count of factories for confirmation text.
pub fn factory_count(count: usize) -> String {
    match count {
        1 => "1 factory".to_string(),
        count => format!("{count} factories"),
    }
}

/// Text confirming a queue order, saying how many factories took it.
pub fn queue_label(name: &str, given: usize, selected: usize) -> String {
    match (given, selected) {
        (_, 1) => format!("{name} queued"),
        (given, selected) if given == selected => format!("{name}: all {selected} factories"),
        (given, selected) => format!("{name}: {given} of {selected} factories"),
    }
}

fn factory_controls(ui: &mut egui::Ui, menu: &FactoryMenu, db: &BlueprintDb, actions: &mut Vec<UiAction>) {
    let total = menu.factories.len();
    let partial = |able: usize| if able < total { format!("\n{able} of {total} factories") } else { String::new() };
    ui.horizontal_wrapped(|ui| {
        for (option, able) in &menu.options {
            if ui.button(format!("{}{}", cost_label(db, *option), partial(able.len()))).clicked() {
                actions.push(queue_action(menu, able, *option));
            }
        }
        if let Some((upgrade, able)) = &menu.upgrade {
            let cost = cost_label(db, *upgrade);
            let label = format!("[{}] Upgrade to T2\n{}{}", hotkeys::UPGRADE, cost.lines().nth(1).unwrap_or(""), partial(able.len()));
            if ui.button(label).clicked() {
                actions.push(queue_action(menu, able, *upgrade));
            }
        }
    });
    ui.horizontal(|ui| {
        let mut repeat = menu.repeat == RepeatState::On;
        let checkbox = egui::Checkbox::new(&mut repeat, format!("[{}] Repeat", hotkeys::REPEAT)).indeterminate(menu.repeat == RepeatState::Mixed);
        if ui.add(checkbox).clicked() {
            actions.push(repeat_action(menu));
        }
        if ui.button(format!("[{}] Clear queue", hotkeys::CLEAR_QUEUE)).clicked() {
            actions.push(clear_action(menu));
        }
        if total > 1 {
            ui.label(format!("{total} factories:"));
        }
        if menu.queue.is_empty() {
            ui.label("Queue is empty. Right-click the ground to set a rally point.");
        } else {
            if let Some(progress) = menu.progress_permille {
                ui.add(egui::ProgressBar::new(f32::from(progress) / 1000.0).desired_width(100.0));
            }
        }
    });
    if let Some(note) = construction_note(menu.unfinished, total) {
        ui.label(note);
    }
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
    trails_for(previous, current, alpha, selected, MAX_TRAIL_UNITS)
}

/// Trails for every other unit with pending orders, for while Shift is held.
/// `strength` runs from 1 down to 0 as they fade out afterwards.
pub fn other_units_trails(previous: &SimSnapshot, current: &SimSnapshot, alpha: f64, selected: &[SimId], strength: f32) -> Vec<TrailLeg> {
    let mut ids: Vec<SimId> = current.plans.iter().map(|plan| plan.id).collect();
    ids.extend(current.factories.iter().filter(|factory| factory.rally.is_some()).map(|factory| factory.id));
    ids.sort_unstable();
    ids.dedup();
    ids.retain(|id| !selected.contains(id));
    let mut legs = trails_for(previous, current, alpha, &ids, ALL_ORDERS_UNIT_LIMIT);
    for leg in &mut legs {
        leg.opacity *= strength;
    }
    legs
}

fn trails_for(previous: &SimSnapshot, current: &SimSnapshot, alpha: f64, selected: &[SimId], limit: usize) -> Vec<TrailLeg> {
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
        if units_drawn == limit {
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

/// A beam between a builder and what it is working on, in world coordinates.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct WorkBeam {
    pub builder: SimId,
    pub from: [f64; 3],
    pub to: [f64; 3],
    pub reclaim: bool,
}

/// Works out the beams for all work being done. Each starts at the front of the builder,
/// drawn where the builder appears between ticks.
pub fn work_beams(previous: &SimSnapshot, current: &SimSnapshot, alpha: f64) -> Vec<WorkBeam> {
    let raw = |value: i32| f64::from(value) / 4096.0;
    current
        .work_links
        .iter()
        .filter_map(|link| {
            let unit = find_unit(current, link.builder)?;
            let now = [raw(unit.x), raw(unit.y), raw(unit.z)];
            let before = find_unit(previous, link.builder).map_or(now, |unit| [raw(unit.x), raw(unit.y), raw(unit.z)]);
            let center: [f64; 3] = std::array::from_fn(|axis| before[axis] + (now[axis] - before[axis]) * alpha);
            let to = link.target.map(raw);
            let (dx, dy) = (to[0] - center[0], to[1] - center[1]);
            let distance = dx.hypot(dy);
            let reach = raw(unit.radius).min(distance);
            let (forward_x, forward_y) = if distance > 0.0 { (dx / distance * reach, dy / distance * reach) } else { (0.0, 0.0) };
            Some(WorkBeam { builder: link.builder, from: [center[0] + forward_x, center[1] + forward_y, center[2]], to, reclaim: link.reclaim })
        })
        .collect()
}

/// How far along its beam each pulse is, from 0 at the builder to 1 at the work. Pulses
/// flow towards the work, or back to the builder when reclaiming. Builders are out of
/// step with each other, so that a group reads as separate beams.
pub fn beam_pulse_shares(time: f64, builder: SimId, reclaim: bool) -> [f32; BEAM_PULSES] {
    let offset = (f64::from(builder.0) * 0.37).fract();
    std::array::from_fn(|index| {
        let share = (time / BEAM_PULSE_SECONDS + offset + index as f64 / BEAM_PULSES as f64).fract();
        (if reclaim { 1.0 - share } else { share }) as f32
    })
}

/// Draws work beams: a faint line, pulses travelling along it, and a ring at the work.
pub fn draw_work_beams(ctx: &egui::Context, camera: &Camera, beams: &[WorkBeam], time: f64, pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let to_points = |world: [f64; 3]| {
        camera.world_to_screen(world).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
    };
    let bounds = ctx.screen_rect().expand(16.0);
    for beam in beams {
        let (Some(from), Some(to)) = (to_points(beam.from), to_points(beam.to)) else {
            continue;
        };
        if from.distance(to) < MIN_BEAM_POINTS || clip_segment(from, to, bounds).is_none() {
            continue;
        }
        let color = if beam.reclaim { BEAM_RECLAIM } else { BEAM_BUILD };
        // A soft glow under a bright core.
        painter.line_segment([from, to], Stroke::new(5.0_f32, color.gamma_multiply(0.18)));
        painter.line_segment([from, to], Stroke::new(1.5_f32, color.gamma_multiply(0.75)));
        for share in beam_pulse_shares(time, beam.builder, beam.reclaim) {
            // Pulses fade in and out at the ends, so that they do not pop.
            let brightness = (share * std::f32::consts::PI).sin();
            painter.circle_filled(from + (to - from) * share, 1.5 + 2.5 * brightness, color.gamma_multiply(0.3 + 0.7 * brightness));
        }
        if bounds.contains(to) {
            let breath = ((time * 5.0 + f64::from(beam.builder.0)).sin() * 0.5 + 0.5) as f32;
            painter.circle_stroke(to, 5.0 + 6.0 * breath, Stroke::new(2.0_f32, color.gamma_multiply(1.0 - 0.6 * breath)));
            painter.circle_filled(to, 2.5, color);
        }
    }
}

/// Marks where an extractor can go while placing one, since deposits are small from far away.
pub fn deposit_highlights(ctx: &egui::Context, camera: &Camera, free: &[[f64; 3]], target: Option<[f64; 3]>, pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let color = marker_color(MarkerKind::Build);
    let to_points = |world: [f64; 3]| {
        camera.world_to_screen(world).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
    };
    for &deposit in free {
        if let Some(center) = to_points(deposit) {
            painter.circle_stroke(center, 9.0, Stroke::new(1.5_f32, color.gamma_multiply(0.7)));
        }
    }
    if let Some(center) = target.and_then(to_points) {
        painter.circle_stroke(center, 13.0, Stroke::new(2.5_f32, color));
    }
}

/// Keeps selection visible when units are too small on screen for their ground rings to show.
/// Units already showing a tactical icon are skipped, since the icon turns green instead.
pub fn distant_selection_markers(ctx: &egui::Context, camera: &Camera, snapshot: &SimSnapshot, selected: &[SimId], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let stroke = Stroke::new(1.5_f32, Color32::from_rgb(70, 255, 100));
    let raw = |value: i32| f64::from(value) / 4096.0;
    let viewport = ctx.screen_rect().expand(8.0);
    let icons_showing = camera.altitude() >= ICON_ALTITUDE;
    for &id in selected {
        let Some(unit) = find_unit(snapshot, id) else {
            continue;
        };
        if icons_showing && unit.blueprint != u16::MAX {
            continue;
        }
        let world = [raw(unit.x), raw(unit.y), raw(unit.z)];
        let (Some(center), Some(edge)) = (camera.world_to_screen(world), camera.world_to_screen([world[0] + raw(unit.radius), world[1], world[2]])) else {
            continue;
        };
        let apparent = (edge[0] - center[0]).hypot(edge[1] - center[1]) as f32 / pixels_per_point;
        let center = Pos2::new(center[0] as f32 / pixels_per_point, center[1] as f32 / pixels_per_point);
        if apparent < MIN_SELECTION_RING_POINTS && viewport.contains(center) {
            painter.rect_stroke(Rect::from_center_size(center, Vec2::splat(9.0)), Rounding::ZERO, stroke);
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
    use crate::sim::world::{BuildQueueView, FactoryView, PlanStep, UnitPlanView, WorkLinkView};

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

    fn factory_view(id: u32, queue: Vec<u16>, repeat: bool, progress_permille: u16) -> FactoryView {
        FactoryView { id: SimId(id), queue, repeat, progress_permille, rally: None }
    }

    /// One T1 factory, one T2 factory, and a T1 factory still under construction.
    fn mixed_factories(db: &BlueprintDb) -> SimSnapshot {
        let factory_unit = |id: u32, key: &str, built: u16| {
            let mut view = unit(id, 0.0);
            view.blueprint = db.id(key).0;
            view.is_structure = true;
            view.built_permille = built;
            view
        };
        let mut current = snapshot(
            vec![factory_unit(1, "land_factory_t1", 1000), factory_unit(2, "land_factory_t2", 1000), factory_unit(3, "land_factory_t1", 400)],
            vec![],
        );
        current.factories = vec![factory_view(1, vec![], false, 0), factory_view(2, vec![], false, 0), factory_view(3, vec![], false, 0)];
        current
    }

    #[test]
    fn the_menu_lists_everything_any_factory_can_build_and_who_can() {
        let db = BlueprintDb::standard();
        let current = mixed_factories(&db);
        let menu = factory_menu(&current, &[SimId(1), SimId(2), SimId(3)], &db).unwrap();
        assert_eq!(menu.factories, [SimId(1), SimId(2), SimId(3)], "unfinished factories take orders too");
        assert_eq!(menu.unfinished, 1);
        let able = |key: &str| menu.options.iter().find(|(option, _)| *option == db.id(key)).map(|(_, able)| able.clone());
        assert_eq!(able("t1_light_bot").unwrap(), [SimId(1), SimId(2), SimId(3)]);
        assert_eq!(able("t2_heavy_tank").unwrap(), [SimId(2)], "only T2 factories can build tanks");
        assert_eq!(menu.upgrade, Some((db.id("land_factory_t2"), vec![SimId(1), SimId(3)])), "only T1 factories can upgrade");
    }

    #[test]
    fn repeat_can_be_off_on_or_mixed() {
        let db = BlueprintDb::standard();
        let mut current = mixed_factories(&db);
        let state = |current: &SimSnapshot| factory_menu(current, &[SimId(1), SimId(2)], &db).unwrap().repeat;
        assert_eq!(state(&current), RepeatState::Off);
        current.factories[0].repeat = true;
        assert_eq!(state(&current), RepeatState::Mixed);
        current.factories[1].repeat = true;
        assert_eq!(state(&current), RepeatState::On);
    }

    #[test]
    fn the_queue_is_combined_and_an_upgrade_under_way_is_not_offered_again() {
        let db = BlueprintDb::standard();
        let (bot, upgrade) = (db.id("t1_light_bot"), db.id("land_factory_t2"));
        let mut current = mixed_factories(&db);
        current.factories[0] = factory_view(1, vec![upgrade.0, bot.0], false, 200);
        current.factories[1] = factory_view(2, vec![bot.0, bot.0], false, 400);
        let menu = factory_menu(&current, &[SimId(1), SimId(2)], &db).unwrap();
        assert_eq!(menu.queue, [(upgrade, 1), (bot, 3)]);
        assert_eq!(menu.progress_permille, Some(300));
        assert_eq!(menu.upgrade, None);
    }

    #[test]
    fn a_factory_under_construction_has_a_menu_and_shows_its_queue() {
        let db = BlueprintDb::standard();
        let bot = db.id("t1_light_bot");
        let mut current = mixed_factories(&db);
        current.factories[2] = factory_view(3, vec![bot.0], false, 0);
        let menu = factory_menu(&current, &[SimId(3)], &db).unwrap();
        assert_eq!(menu.factories, [SimId(3)]);
        assert_eq!(menu.unfinished, 1);
        assert_eq!(menu.queue, [(bot, 1)]);
        assert_eq!(menu.progress_permille, None, "nothing is being made until it is built");
    }

    #[test]
    fn an_unfinished_factory_does_not_drag_down_the_progress_bar() {
        let db = BlueprintDb::standard();
        let bot = db.id("t1_light_bot");
        let mut current = mixed_factories(&db);
        current.factories[0] = factory_view(1, vec![bot.0], false, 600);
        current.factories[2] = factory_view(3, vec![bot.0], false, 0);
        let menu = factory_menu(&current, &[SimId(1), SimId(3)], &db).unwrap();
        assert_eq!(menu.progress_permille, Some(600));
        assert_eq!(menu.queue, [(bot, 2)]);
    }

    #[test]
    fn a_factory_with_an_upgrade_queued_offers_what_the_upgrade_will_build() {
        let db = BlueprintDb::standard();
        let (upgrade, tank, bot) = (db.id("land_factory_t2"), db.id("t2_heavy_tank"), db.id("t1_light_bot"));
        let mut current = mixed_factories(&db);
        let able = |current: &SimSnapshot, selected: &[SimId], option| {
            factory_menu(current, selected, &db).unwrap().options.iter().find(|(existing, _)| *existing == option).map(|(_, able)| able.clone())
        };
        assert_eq!(able(&current, &[SimId(1)], tank), None, "without an upgrade, no tanks");

        current.factories[0] = factory_view(1, vec![upgrade.0], false, 200);
        assert_eq!(able(&current, &[SimId(1)], tank).unwrap(), [SimId(1)]);
        assert_eq!(able(&current, &[SimId(1)], bot).unwrap(), [SimId(1)], "listed once, though both tiers can build it");
        let expected = UiAction::Queue { factories: vec![SimId(1)], blueprint: tank, selected_count: 1 };
        assert_eq!(hotkey_action(&current, &[SimId(1)], &db, 'H'), Some(expected));

        // The other T1 factory has no upgrade queued, so it is left out.
        assert_eq!(able(&current, &[SimId(1), SimId(3)], tank).unwrap(), [SimId(1)]);
    }

    #[test]
    fn no_factory_means_no_menu() {
        let db = BlueprintDb::standard();
        assert_eq!(factory_menu(&mixed_factories(&db), &[SimId(99)], &db), None);
        assert_eq!(factory_menu(&mixed_factories(&db), &[], &db), None);
    }

    #[test]
    fn the_note_says_when_queues_will_start() {
        assert_eq!(construction_note(0, 2), None);
        assert_eq!(construction_note(1, 1).unwrap(), "Under construction: the queue starts once built.");
        assert_eq!(construction_note(2, 2).unwrap(), "Under construction: the queue starts once built.");
        assert_eq!(construction_note(1, 3).unwrap(), "1 of 3 under construction: their queues start once built.");
    }

    #[test]
    fn a_work_beam_runs_from_the_front_of_the_builder_to_the_work() {
        let raw = |value: f64| (value * 4096.0) as i32;
        let mut builder = unit(1, 100.0);
        builder.radius = raw(5.0);
        let mut current = snapshot(vec![builder], vec![]);
        current.work_links = vec![
            WorkLinkView { builder: SimId(1), target: [raw(150.0), 0, raw(7.0)], reclaim: false },
            // A builder that cannot be seen has no beam.
            WorkLinkView { builder: SimId(2), target: [raw(150.0), 0, 0], reclaim: false },
        ];
        let beams = work_beams(&current, &current, 1.0);
        assert_eq!(beams.len(), 1);
        assert_eq!(beams[0].from[0], 105.0, "the beam starts at the front of the builder");
        assert_eq!(beams[0].to, [150.0, 0.0, 7.0]);

        // Between ticks, the beam stays attached to where the builder is drawn.
        let mut earlier = builder;
        earlier.x = raw(90.0);
        let before = snapshot(vec![earlier], vec![]);
        assert_eq!(work_beams(&before, &current, 0.5)[0].from[0], 100.0);
    }

    #[test]
    fn a_builder_standing_on_its_work_still_gets_a_valid_beam() {
        let mut current = snapshot(vec![unit(1, 100.0)], vec![]);
        current.work_links = vec![WorkLinkView { builder: SimId(1), target: [100 * 4096, 0, 0], reclaim: false }];
        let beam = work_beams(&current, &current, 1.0)[0];
        assert!(beam.from.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn beam_pulses_flow_to_the_work_and_back_when_reclaiming() {
        let later = 0.05;
        let (build_before, build_after) = (beam_pulse_shares(1.0, SimId(1), false), beam_pulse_shares(1.0 + later, SimId(1), false));
        let (reclaim_before, reclaim_after) = (beam_pulse_shares(1.0, SimId(1), true), beam_pulse_shares(1.0 + later, SimId(1), true));
        for index in 0..BEAM_PULSES {
            for share in [build_before[index], reclaim_before[index]] {
                assert!((0.0..=1.0).contains(&share));
            }
            let moved = build_after[index] - build_before[index];
            // A pulse that just wrapped has jumped back to the start.
            assert!(moved > 0.0 || moved < -0.5, "building should flow outwards: {moved}");
            let moved = reclaim_after[index] - reclaim_before[index];
            assert!(moved < 0.0 || moved > 0.5, "reclaiming should flow back: {moved}");
        }
        assert_ne!(beam_pulse_shares(1.0, SimId(1), false), beam_pulse_shares(1.0, SimId(2), false), "builders should be out of step");
    }

    fn commander_and_engineer(db: &BlueprintDb) -> SimSnapshot {
        let mut commander = unit(10, 0.0);
        commander.blueprint = db.id("acu").0;
        let mut engineer = unit(11, 0.0);
        engineer.blueprint = db.id("t1_engineer").0;
        let mut bot = unit(12, 0.0);
        bot.blueprint = db.id("t1_light_bot").0;
        snapshot(vec![commander, engineer, bot], vec![])
    }

    #[test]
    fn builder_keys_begin_placing_the_same_structures_as_the_buttons() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        for (letter, key) in [('Q', "land_factory_t1"), ('E', "mass_extractor"), ('G', "power_generator"), ('T', "mass_storage"), ('C', "t1_point_defense")] {
            for selected in [vec![SimId(10)], vec![SimId(11)], vec![SimId(10), SimId(11)]] {
                assert_eq!(hotkey_action(&current, &selected, &db, letter), Some(UiAction::BeginPlacement(db.id(key))), "{letter} for {selected:?}");
            }
        }
    }

    #[test]
    fn keys_do_nothing_when_the_selection_has_no_such_option() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        assert_eq!(hotkey_action(&current, &[], &db, 'E'), None, "nothing is selected");
        assert_eq!(hotkey_action(&current, &[SimId(12)], &db, 'E'), None, "a bot cannot build");
        assert_eq!(hotkey_action(&current, &[SimId(10)], &db, 'B'), None, "a commander is not a factory");
        assert_eq!(hotkey_action(&current, &[SimId(10)], &db, 'R'), None, "a commander has no repeat");
        let factories = mixed_factories(&db);
        assert_eq!(hotkey_action(&factories, &[SimId(1)], &db, 'H'), None, "a T1 factory cannot build tanks");
        assert_eq!(hotkey_action(&factories, &[SimId(1)], &db, 'E'), None, "a factory does not place structures");
        assert_eq!(hotkey_action(&factories, &[SimId(2)], &db, 'U'), None, "a T2 factory has no upgrade");
    }

    #[test]
    fn factory_keys_reach_every_factory_able_to_follow_them() {
        let db = BlueprintDb::standard();
        let current = mixed_factories(&db);
        let selected = [SimId(1), SimId(2), SimId(3)];
        let queue = |key: &str, factories: Vec<SimId>| UiAction::Queue { factories, blueprint: db.id(key), selected_count: 3 };
        assert_eq!(hotkey_action(&current, &selected, &db, 'Z'), Some(queue("t1_engineer", selected.to_vec())));
        assert_eq!(hotkey_action(&current, &selected, &db, 'B'), Some(queue("t1_light_bot", selected.to_vec())));
        assert_eq!(hotkey_action(&current, &selected, &db, 'V'), Some(queue("t1_light_artillery", selected.to_vec())));
        assert_eq!(hotkey_action(&current, &selected, &db, 'H'), Some(queue("t2_heavy_tank", vec![SimId(2)])));
        assert_eq!(hotkey_action(&current, &selected, &db, 'U'), Some(queue("land_factory_t2", vec![SimId(1), SimId(3)])));
        assert_eq!(hotkey_action(&current, &selected, &db, 'K'), Some(UiAction::ClearQueue { factories: selected.to_vec() }));
    }

    #[test]
    fn the_repeat_key_toggles_like_the_checkbox() {
        let db = BlueprintDb::standard();
        let mut current = mixed_factories(&db);
        let selected = [SimId(1), SimId(2)];
        let press = |current: &SimSnapshot| hotkey_action(current, &selected, &db, 'R');
        let set = |repeat: bool| Some(UiAction::SetRepeat { factories: selected.to_vec(), repeat });
        assert_eq!(press(&current), set(true));
        current.factories[0].repeat = true;
        assert_eq!(press(&current), set(true), "a mixed selection turns on for all");
        current.factories[1].repeat = true;
        assert_eq!(press(&current), set(false));
    }

    #[test]
    fn a_builder_and_a_factory_selected_together_both_answer_their_own_keys() {
        let db = BlueprintDb::standard();
        let mut current = mixed_factories(&db);
        let mut engineer = unit(4, 0.0);
        engineer.blueprint = db.id("t1_engineer").0;
        current.units.push(engineer);
        let selected = [SimId(1), SimId(4)];
        assert_eq!(hotkey_action(&current, &selected, &db, 'E'), Some(UiAction::BeginPlacement(db.id("mass_extractor"))));
        let expected = UiAction::Queue { factories: vec![SimId(1)], blueprint: db.id("t1_engineer"), selected_count: 1 };
        assert_eq!(hotkey_action(&current, &selected, &db, 'Z'), Some(expected));
    }

    #[test]
    fn every_button_label_shows_its_key() {
        let db = BlueprintDb::standard();
        assert!(cost_label(&db, db.id("mass_extractor")).starts_with("[E] Mass Extractor"), "{}", cost_label(&db, db.id("mass_extractor")));
        assert!(cost_label(&db, db.id("t1_light_bot")).starts_with("[B] "));
    }

    fn build(db: &BlueprintDb, key: &str, cell_x: u32, site: Option<SimId>) -> BuildTask {
        BuildTask::Build { blueprint: db.id(key), cell_x, cell_y: 5, site }
    }

    #[test]
    fn a_builders_orders_are_listed_in_order_with_how_far_a_started_site_has_got() {
        let db = BlueprintDb::standard();
        let mut site = unit(20, 0.0);
        site.built_permille = 450;
        let mut current = commander_and_engineer(&db);
        current.units.push(site);
        let tasks = vec![build(&db, "power_generator", 1, Some(SimId(20))), build(&db, "mass_extractor", 2, None), BuildTask::Reclaim { target: SimId(30) }];
        current.build_queues = vec![BuildQueueView { id: SimId(10), tasks: tasks.clone() }];
        let chips = queue_chips(&current, &[SimId(10)], &db);
        let labels: Vec<&str> = chips.iter().map(|chip| chip.label.as_str()).collect();
        assert_eq!(labels, ["1. Power Generator 45%", "2. Mass Extractor", "3. Reclaim"]);
        assert_eq!(chips[1].remove, UiAction::CancelTask { units: vec![SimId(10)], task: tasks[1] });
        assert!(queue_chips(&current, &[SimId(11)], &db).is_empty(), "the engineer has no orders");
    }

    #[test]
    fn an_order_shared_by_several_builders_appears_once_and_is_removed_from_them_all() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        // One has started the site, and the other has not reached it yet.
        current.build_queues = vec![
            BuildQueueView { id: SimId(10), tasks: vec![build(&db, "power_generator", 1, Some(SimId(20)))] },
            BuildQueueView { id: SimId(11), tasks: vec![build(&db, "mass_extractor", 2, None), build(&db, "power_generator", 1, None)] },
        ];
        let chips = queue_chips(&current, &[SimId(10), SimId(11)], &db);
        assert_eq!(chips.len(), 2);
        let UiAction::CancelTask { units, .. } = &chips[0].remove else {
            panic!("a builder's order should cancel a task");
        };
        assert_eq!(units, &[SimId(10), SimId(11)]);
        let UiAction::CancelTask { units, .. } = &chips[1].remove else {
            panic!("a builder's order should cancel a task");
        };
        assert_eq!(units, &[SimId(11)]);
    }

    #[test]
    fn a_factorys_items_are_grouped_and_only_reach_factories_holding_them() {
        let db = BlueprintDb::standard();
        let (bot, tank) = (db.id("t1_light_bot"), db.id("t2_heavy_tank"));
        let mut current = mixed_factories(&db);
        current.factories[0] = factory_view(1, vec![bot.0, bot.0], false, 100);
        current.factories[1] = factory_view(2, vec![tank.0, bot.0], false, 100);
        let chips = queue_chips(&current, &[SimId(1), SimId(2)], &db);
        assert_eq!(chips.len(), 2);
        assert_eq!(chips[0].label, "Light Assault Bot x3");
        assert_eq!(chips[0].remove, UiAction::RemoveQueued { factories: vec![SimId(1), SimId(2)], blueprint: bot });
        assert_eq!(chips[1].label, "Heavy Tank");
        assert_eq!(chips[1].remove, UiAction::RemoveQueued { factories: vec![SimId(2)], blueprint: tank });
    }

    /// Runs real egui input through the queue row, and returns what came out.
    fn click_first_chip(button: egui::PointerButton) -> (Vec<UiAction>, bool) {
        let ctx = egui::Context::default();
        let chips = [QueueChip { label: "1. Mass Extractor".to_string(), remove: UiAction::Restart }];
        let at = egui::pos2(20.0, 16.0);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let input = |events: Vec<egui::Event>| egui::RawInput { screen_rect: Some(screen), events, ..egui::RawInput::default() };
        let press = |pressed: bool| egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE };
        let mut actions = Vec::new();
        let mut run = |events: Vec<egui::Event>| {
            let _ = ctx.run(input(events), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| queue_chip_row(ui, &chips, &mut actions));
            });
        };
        run(vec![]);
        run(vec![egui::Event::PointerMoved(at)]);
        run(vec![]);
        let over_panel = ctx.wants_pointer_input();
        run(vec![press(true)]);
        run(vec![press(false)]);
        run(vec![]);
        (actions, over_panel)
    }

    #[test]
    fn right_clicking_a_queue_item_removes_it_and_left_clicking_does_not() {
        let (actions, over_panel) = click_first_chip(egui::PointerButton::Secondary);
        assert_eq!(actions, [UiAction::Restart], "a right-click should remove the order");
        assert!(over_panel, "the interface must claim the click, or it would also order units to move");
        assert!(click_first_chip(egui::PointerButton::Primary).0.is_empty());
    }

    #[test]
    fn keyboard_focus_never_stays_on_a_button() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("button");
        ctx.memory_mut(|memory| memory.request_focus(id));
        assert!(ctx.wants_keyboard_input(), "the test needs a focused button to begin with");
        release_keyboard_focus(&ctx);
        assert!(!ctx.wants_keyboard_input(), "focus would swallow hotkeys");
    }

    #[test]
    fn queue_text_says_how_many_factories_took_the_order() {
        assert_eq!(queue_label("Bot", 1, 1), "Bot queued");
        assert_eq!(queue_label("Bot", 3, 3), "Bot: all 3 factories");
        assert_eq!(queue_label("Tank", 2, 3), "Tank: 2 of 3 factories");
        assert_eq!(factory_count(1), "1 factory");
        assert_eq!(factory_count(4), "4 factories");
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
    fn shift_view_covers_every_other_unit_and_fades() {
        let plan = |id: u32| UnitPlanView { id: SimId(id), steps: vec![step(PlanStepKind::Move, 100.0, 0.0)] };
        let factory = FactoryView { id: SimId(3), queue: vec![], repeat: false, progress_permille: 0, rally: Some([raw(500.0), 0, 0]) };
        let mut current = snapshot(vec![unit(1, 0.0), unit(2, 0.0), unit(3, 0.0), unit(4, 0.0)], vec![plan(1), plan(2)]);
        current.factories = vec![factory];
        let legs = other_units_trails(&current, &current, 1.0, &[SimId(1)], 0.5);
        assert_eq!(legs.len(), 2, "the unselected unit and the factory, but not the selected unit or the idle one");
        let walker = legs.iter().find(|leg| leg.kind == MarkerKind::Move).unwrap();
        assert!((walker.opacity - TRAIL_OPACITY * 0.5).abs() < 1e-6);
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
