//! The in-game interface: resource bar, build menu, unit overlays, and the end of match screen.
//! It reads snapshots and returns actions. It never touches the simulation.
#![allow(clippy::disallowed_types)]

use std::collections::{BTreeMap, BTreeSet};

use egui::{Align2, Color32, FontId, Pos2, Rect, Rounding, Stroke, Vec2};

use crate::input::feedback::{Marker, MarkerKind, OrderFeedback, LABEL_SECONDS};
use crate::input::hotkeys::{self, blueprint_hotkey};
use crate::input::orders::PendingBuild;
use crate::render::camera::Camera;
use crate::sim::blueprints::{BlueprintDb, BlueprintId, UnitClass};
use crate::sim::components::{BuildTask, OrderKind, SimId, Team};
use crate::sim::world::{MatchResult, PlanStepKind, SimSnapshot, UnitRatesView, UnitView};

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
const SPENDING: Color32 = Color32::from_rgb(250, 190, 120);
const VETERAN: Color32 = Color32::from_rgb(255, 205, 60);
/// How close to an order's icon a Shift-press grabs it, in points.
const HANDLE_GRAB_POINTS: f64 = 18.0;
const HANDLE_RADIUS_POINTS: f32 = 10.0;
/// Warm, so that beams stand out against both blue structures and the terrain.
const BEAM_BUILD: Color32 = Color32::from_rgb(255, 215, 110);
const BEAM_RECLAIM: Color32 = Color32::from_rgb(150, 255, 170);
/// Matches how double-clicking a unit works.
const QUICK_DOUBLE_CLICK_SECONDS: f64 = 0.4;
/// How many units a factory queues at once with Shift held.
const SHIFT_QUEUE_COUNT: u32 = 5;
const QUICK_ROW_WIDTH: f32 = 170.0;
const CONTROL_GROUP_ROW_WIDTH: f32 = 220.0;
/// How tall the queue can grow before it scrolls, in points.
const QUEUE_MAX_HEIGHT: f32 = 60.0;
/// How many pulses are on a work beam at once.
const BEAM_PULSES: usize = 5;
/// How long a pulse takes to travel along a beam, in seconds.
const BEAM_PULSE_SECONDS: f64 = 1.1;
/// A beam shorter than this on screen is only clutter.
const MIN_BEAM_POINTS: f32 = 6.0;

/// An order waiting for the player to click where it should go.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArmedOrder {
    Move,
    AttackMove,
    Formation,
    AttackGround,
    Patrol,
    Assist,
    Overcharge,
}

impl ArmedOrder {
    const ALL: [Self; 7] = [Self::Move, Self::AttackMove, Self::Formation, Self::AttackGround, Self::Patrol, Self::Assist, Self::Overcharge];

    const fn letter(self) -> char {
        match self {
            Self::Move => hotkeys::MOVE,
            Self::AttackMove => hotkeys::ATTACK_MOVE,
            Self::Formation => hotkeys::FORMATION,
            Self::AttackGround => hotkeys::ATTACK_GROUND,
            Self::Patrol => hotkeys::PATROL,
            Self::Assist => hotkeys::ASSIST,
            Self::Overcharge => hotkeys::OVERCHARGE,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Move => "Move",
            Self::AttackMove => "Attack-move",
            Self::Formation => "Formation move",
            Self::AttackGround => "Attack ground",
            Self::Patrol => "Patrol",
            Self::Assist => "Assist",
            Self::Overcharge => "Overcharge",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Move => "Walk there, ignoring what is under the cursor. Units still fire at what comes into range.",
            Self::AttackMove => "Walk there, stopping to fire when enemies are in range.",
            Self::Formation => "Travel together at the pace of the slowest unit, keeping the formation all the way. Hold Alt to attack-move, which stops the whole group to fight. Also Ctrl+right-click.",
            Self::AttackGround => "Keep firing at a spot, even out of sight, until given another order. Shells that land on debris destroy it, so the enemy cannot reclaim it.",
            Self::Patrol => "Repeat a route. Combat units fight along it, and engineers also reclaim and repair.",
            Self::Assist => "Stay with a friendly unit and protect it. Builders also help it build.",
            Self::Overcharge => "Spend stored energy on one shot that destroys almost any unit. Weak against structures.",
        }
    }

    /// What to do next, shown while the order is armed.
    pub const fn hint(self) -> &'static str {
        match self {
            Self::Move => "Move: left-click a destination. Right-click or Esc to cancel.",
            Self::AttackMove => "Attack-move: left-click a destination. Right-click or Esc to cancel.",
            Self::Formation => "Formation move: left-click a destination, holding Alt to attack-move. Right-click or Esc to cancel.",
            Self::AttackGround => "Attack ground: left-click a spot or some debris. Right-click or Esc to cancel.",
            Self::Patrol => "Patrol: left-click each point, then right-click or press P to start. Esc to cancel.",
            Self::Assist => "Assist: left-click a friendly unit. Right-click or Esc to cancel.",
            Self::Overcharge => "Overcharge: left-click an enemy. Right-click or Esc to cancel.",
        }
    }
}

/// What the player asked for through the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiAction {
    BeginPlacement(BlueprintId),
    /// Queue `count` of an item in each of these factories, out of `selected_count` selected.
    Queue { factories: Vec<SimId>, blueprint: BlueprintId, selected_count: usize, count: u32 },
    SetRepeat { factories: Vec<SimId>, repeat: bool },
    ClearQueue { factories: Vec<SimId> },
    /// The next click on the map gives this order.
    Arm(ArmedOrder),
    Stop,
    /// Pause or resume these factories and construction sites.
    SetPaused { units: Vec<SimId>, paused: bool },
    /// From the quick select bar: select these, or add them to the selection.
    QuickSelect { units: Vec<SimId>, add: bool },
    /// Remove one order from these builders' queues.
    CancelTask { units: Vec<SimId>, task: BuildTask },
    /// Remove one of an item from each of these factories' queues.
    RemoveQueued { factories: Vec<SimId>, blueprint: BlueprintId },
    /// Upgrade these commanders or structures in place.
    Upgrade { units: Vec<SimId> },
    /// Switch these units' cloaks on or off.
    SetCloak { units: Vec<SimId>, on: bool },
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

/// Whether anything selected can walk, and so take the unit orders.
pub fn has_mobile_units(snapshot: &SimSnapshot, selected: &[SimId]) -> bool {
    selected.iter().filter_map(|id| find_unit(snapshot, *id)).any(|unit| !unit.is_structure)
}

/// Whether anything selected has Overcharge.
pub fn has_overcharge(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> bool {
    selected
        .iter()
        .filter_map(|id| find_unit(snapshot, *id))
        .any(|unit| unit.blueprint != u16::MAX && db.get(BlueprintId(unit.blueprint)).overcharge.is_some())
}

/// A shield generator's dome, in words: how strong it is, or how long until it is back.
pub fn shield_status(snapshot: &SimSnapshot, id: SimId) -> Option<String> {
    let shield = snapshot.shields.iter().find(|shield| shield.id == id)?;
    Some(if shield.up {
        format!("shield {}%", shield.health_permille / 10)
    } else if shield.recharging > 0 {
        format!("shield down, back in {} s", shield.recharging.div_ceil(10))
    } else {
        "shield off".to_string()
    })
}

/// Whether an intel structure is short of energy, or how a cloak is doing.
pub fn intel_status(snapshot: &SimSnapshot, id: SimId) -> Option<String> {
    if snapshot.unpowered.contains(&id) {
        return Some("off: no energy".to_string());
    }
    snapshot.cloaks.iter().find(|(unit, _, _)| *unit == id).and_then(|&(_, on, cloaked)| match (on, cloaked) {
        (true, true) => Some("cloaked".to_string()),
        (true, false) => Some("cloak waiting for energy".to_string()),
        _ => None,
    })
}

/// How many missiles a silo has built and is keeping.
pub fn stock_status(snapshot: &SimSnapshot, id: SimId, db: &BlueprintDb) -> Option<String> {
    let &(_, stock) = snapshot.stocks.iter().find(|(silo, _)| *silo == id)?;
    let unit = find_unit(snapshot, id)?;
    let blueprint = db.get(BlueprintId(unit.blueprint));
    let word = if blueprint.missile_defense.is_some() { "interceptor" } else { "missile" };
    Some(format!("{stock} {word}{} ready", if stock == 1 { "" } else { "s" }))
}

/// Whether anything selected has a gun, and so can attack the ground.
pub fn has_weapons(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> bool {
    selected
        .iter()
        .filter_map(|id| find_unit(snapshot, *id))
        .any(|unit| unit.blueprint == u16::MAX || db.get(BlueprintId(unit.blueprint)).weapon.is_some())
}

fn unit_order_row(ui: &mut egui::Ui, armed: Option<ArmedOrder>, overcharge: bool, weapons: bool, actions: &mut Vec<UiAction>) {
    ui.horizontal_wrapped(|ui| {
        let offered = |order: &ArmedOrder| match order {
            ArmedOrder::Overcharge => overcharge,
            ArmedOrder::AttackGround => weapons,
            _ => true,
        };
        for order in ArmedOrder::ALL.into_iter().filter(offered) {
            let button = egui::Button::new(format!("[{}] {}", order.letter(), order.name())).selected(armed == Some(order));
            if ui.add(button).on_hover_text(order.description()).clicked() {
                actions.push(UiAction::Arm(order));
            }
        }
        if ui.button(format!("[{}] Stop", hotkeys::STOP)).on_hover_text("Drop all orders.").clicked() {
            actions.push(UiAction::Stop);
        }
    });
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

/// Holding Shift queues several units at once. An upgrade only ever happens once.
pub fn with_shift(action: UiAction, shift: bool, db: &BlueprintDb) -> UiAction {
    match action {
        UiAction::Queue { factories, blueprint, selected_count, .. } if shift && !db.get(blueprint).is_factory() => {
            UiAction::Queue { factories, blueprint, selected_count, count: SHIFT_QUEUE_COUNT }
        }
        other => other,
    }
}

fn queue_action(menu: &FactoryMenu, able: &[SimId], blueprint: BlueprintId) -> UiAction {
    UiAction::Queue { factories: able.to_vec(), blueprint, selected_count: menu.factories.len(), count: 1 }
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
    // O switches things off and on: it pauses what can be paused, or else toggles a cloak.
    if letter == hotkeys::PAUSE {
        return pause_action(snapshot, selected, db).or_else(|| cloak_action(snapshot, selected));
    }
    if has_mobile_units(snapshot, selected) {
        match letter {
            hotkeys::MOVE => return Some(UiAction::Arm(ArmedOrder::Move)),
            hotkeys::ATTACK_MOVE => return Some(UiAction::Arm(ArmedOrder::AttackMove)),
            hotkeys::FORMATION => return Some(UiAction::Arm(ArmedOrder::Formation)),
            hotkeys::ATTACK_GROUND if has_weapons(snapshot, selected, db) => return Some(UiAction::Arm(ArmedOrder::AttackGround)),
            hotkeys::ASSIST => return Some(UiAction::Arm(ArmedOrder::Assist)),
            hotkeys::OVERCHARGE if has_overcharge(snapshot, selected, db) => return Some(UiAction::Arm(ArmedOrder::Overcharge)),
            hotkeys::STOP => return Some(UiAction::Stop),
            _ => {}
        }
    }
    let is_key = |blueprint: BlueprintId| blueprint_hotkey(&db.get(blueprint).key) == Some(letter);
    if let Some(option) = builder_options(snapshot, selected, db).into_iter().find(|option| is_key(*option)) {
        return Some(UiAction::BeginPlacement(option));
    }
    if letter == hotkeys::UPGRADE {
        let units = upgradable_units(snapshot, selected, db);
        if !units.is_empty() {
            return Some(UiAction::Upgrade { units });
        }
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

/// A unit's veteran rank, or 0.
pub fn rank_of(snapshot: &SimSnapshot, id: SimId) -> u8 {
    snapshot.veterans.binary_search_by_key(&id, |veteran| veteran.0).map_or(0, |index| snapshot.veterans[index].1)
}

/// The selected units that can be paused: factories, and the player's sites still being built.
pub fn pausable_units(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Vec<SimId> {
    selected
        .iter()
        .copied()
        .filter(|id| {
            find_unit(snapshot, *id).is_some_and(|unit| {
                unit.blueprint != u16::MAX && {
                    let blueprint = db.get(BlueprintId(unit.blueprint));
                    blueprint.is_factory() || blueprint.shield.is_some() || unit.built_permille < 1000
                }
            })
        })
        .collect()
}

/// The selected commanders and structures that can upgrade in place now: finished, not
/// upgrading already, and with somewhere to go. Factories upgrade through their queue instead.
pub fn upgradable_units(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Vec<SimId> {
    selected
        .iter()
        .copied()
        .filter(|id| {
            find_unit(snapshot, *id).is_some_and(|unit| {
                unit.blueprint != u16::MAX && unit.built_permille == 1000 && {
                    let blueprint = db.get(BlueprintId(unit.blueprint));
                    blueprint.upgrades_to.is_some() && !blueprint.is_factory()
                }
            }) && snapshot.upgrades.iter().all(|(upgrading, _)| upgrading != id)
        })
        .collect()
}

/// Cloaks the selected units that can cloak, unless they all are already, in which case it
/// uncloaks them.
pub fn cloak_action(snapshot: &SimSnapshot, selected: &[SimId]) -> Option<UiAction> {
    let able: Vec<&(SimId, bool, bool)> = snapshot.cloaks.iter().filter(|(id, _, _)| selected.contains(id)).collect();
    if able.is_empty() {
        return None;
    }
    let all_on = able.iter().all(|(_, on, _)| *on);
    Some(UiAction::SetCloak { units: able.iter().map(|(id, _, _)| *id).collect(), on: !all_on })
}

/// Pauses them all, unless they are all paused already, in which case it resumes them.
pub fn pause_action(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> Option<UiAction> {
    let units = pausable_units(snapshot, selected, db);
    if units.is_empty() {
        return None;
    }
    let all_paused = units.iter().all(|id| snapshot.paused.binary_search(id).is_ok());
    Some(UiAction::SetPaused { units, paused: !all_paused })
}

/// What the selection makes and spends, as text. Empty if it has nothing to show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionRates {
    pub output: String,
    pub spending: String,
}

fn per_second(tenths: i32) -> String {
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// Adds up what the selected structures make, and what the selected factories are spending.
pub fn selection_rates(snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb) -> SelectionRates {
    let mut total = UnitRatesView { id: SimId(0), mass_income: 0, energy_income: 0, mass_use: 0, energy_use: 0 };
    let mut factories = 0;
    for &id in selected {
        let Ok(index) = snapshot.rates.binary_search_by_key(&id, |rates| rates.id) else {
            continue;
        };
        let rates = snapshot.rates[index];
        total.mass_income += rates.mass_income;
        total.energy_income += rates.energy_income;
        total.mass_use += rates.mass_use;
        total.energy_use += rates.energy_use;
        // Factories and shields spend, so they say so even when they are spending nothing.
        factories += usize::from(find_unit(snapshot, id).is_some_and(|unit| unit.blueprint != u16::MAX && {
            let blueprint = db.get(BlueprintId(unit.blueprint));
            blueprint.is_factory() || blueprint.shield.is_some()
        }));
    }
    let mut output = Vec::new();
    if total.mass_income > 0 {
        output.push(format!("+{} mass/s", per_second(total.mass_income)));
    }
    if total.energy_income > 0 {
        output.push(format!("+{} energy/s", per_second(total.energy_income)));
    }
    let spending = match (factories, total.mass_use + total.energy_use) {
        (0, _) => String::new(),
        (_, 0) => "Using nothing".to_string(),
        _ => format!("Using {} mass/s, {} energy/s", per_second(total.mass_use), per_second(total.energy_use)),
    };
    SelectionRates { output: output.join(", "), spending }
}

/// An order that can be picked up and put somewhere else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DraggableOrder {
    Walk { units: Vec<SimId>, task: BuildTask },
    Build { units: Vec<SimId>, task: BuildTask },
    Rally { factories: Vec<SimId> },
}

/// An order's icon on the map. A group's order has one, in the middle of the group's spots.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderHandle {
    pub position: [f64; 3],
    pub kind: MarkerKind,
    pub order: DraggableOrder,
}

/// One icon for each order that can be moved. Orders shared by several units count once.
pub fn order_handles(snapshot: &SimSnapshot) -> Vec<OrderHandle> {
    let raw = |value: i32| f64::from(value) / 4096.0;
    // The task, its units, and the sum of their positions.
    let mut orders: Vec<(BuildTask, Vec<SimId>, [f64; 3])> = Vec::new();
    for stop in &snapshot.order_stops {
        let position = stop.position.map(raw);
        match orders.iter_mut().find(|(task, _, _)| task.is_same_order(&stop.task)) {
            Some((_, units, sum)) => {
                units.push(stop.unit);
                *sum = std::array::from_fn(|axis| sum[axis] + position[axis]);
            }
            None => orders.push((stop.task, vec![stop.unit], position)),
        }
    }
    let mut handles: Vec<OrderHandle> = orders
        .into_iter()
        .map(|(task, units, sum)| {
            let position = sum.map(|value| value / units.len() as f64);
            match task {
                BuildTask::Walk { order, .. } => {
                    let kind = if order.kind == OrderKind::AttackMove { MarkerKind::AttackMove } else { MarkerKind::Move };
                    OrderHandle { position, kind, order: DraggableOrder::Walk { units, task } }
                }
                _ => OrderHandle { position, kind: MarkerKind::Build, order: DraggableOrder::Build { units, task } },
            }
        })
        .collect();
    // Factories given a rally point together keep sharing it.
    let mut rallies: Vec<([i32; 3], Vec<SimId>)> = Vec::new();
    for factory in &snapshot.factories {
        let Some(rally) = factory.rally else {
            continue;
        };
        match rallies.iter_mut().find(|(point, _)| *point == rally) {
            Some((_, factories)) => factories.push(factory.id),
            None => rallies.push((rally, vec![factory.id])),
        }
    }
    handles.extend(rallies.into_iter().map(|(point, factories)| OrderHandle { position: point.map(raw), kind: MarkerKind::Rally, order: DraggableOrder::Rally { factories } }));
    handles
}

/// The index of the handle under the cursor, if any. Both are in pixels.
pub fn handle_under_cursor(handles: &[OrderHandle], camera: &Camera, cursor: [f64; 2], pixels_per_point: f64) -> Option<usize> {
    let reach = HANDLE_GRAB_POINTS * pixels_per_point;
    let mut best: Option<(f64, usize)> = None;
    for (index, handle) in handles.iter().enumerate() {
        if let Some(at) = camera.world_to_screen(handle.position) {
            let away = (at[0] - cursor[0]).hypot(at[1] - cursor[1]);
            if away <= reach && best.is_none_or(|(closest, _)| away < closest) {
                best = Some((away, index));
            }
        }
    }
    best.map(|(_, index)| index)
}

fn draw_handle(painter: &egui::Painter, center: Pos2, kind: MarkerKind, radius: f32, opacity: f32) {
    let color = marker_color(kind).gamma_multiply(opacity);
    painter.circle(center, radius, Color32::from_black_alpha((210.0 * opacity) as u8), Stroke::new(2.0_f32, color));
    let letter = match kind {
        MarkerKind::AttackMove => "A",
        MarkerKind::Build => "B",
        MarkerKind::Rally => "R",
        _ => "M",
    };
    painter.text(center, Align2::CENTER_CENTER, letter, FontId::proportional(radius * 1.2), color);
}

/// Draws the icons that can be grabbed, for while Shift is held.
pub fn draw_order_handles(ctx: &egui::Context, camera: &Camera, handles: &[OrderHandle], hovered: Option<usize>, strength: f32, pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    for (index, handle) in handles.iter().enumerate() {
        if let Some(at) = camera.world_to_screen(handle.position) {
            let radius = if hovered == Some(index) { HANDLE_RADIUS_POINTS * 1.35 } else { HANDLE_RADIUS_POINTS };
            draw_handle(&painter, Pos2::new(at[0] as f32 / pixels_per_point, at[1] as f32 / pixels_per_point), handle.kind, radius, strength);
        }
    }
}

/// Shows where an order being dragged was, and where it would go.
pub fn draw_dragged_order(ctx: &egui::Context, camera: &Camera, handle: &OrderHandle, cursor: [f64; 2], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let to = Pos2::new(cursor[0] as f32 / pixels_per_point, cursor[1] as f32 / pixels_per_point);
    if let Some(at) = camera.world_to_screen(handle.position) {
        let from = Pos2::new(at[0] as f32 / pixels_per_point, at[1] as f32 / pixels_per_point);
        painter.extend(egui::Shape::dashed_line(&[from, to], Stroke::new(1.5_f32, marker_color(handle.kind).gamma_multiply(0.7)), DASH_POINTS, GAP_POINTS));
        draw_handle(&painter, from, handle.kind, HANDLE_RADIUS_POINTS * 0.7, 0.5);
    }
    draw_handle(&painter, to, handle.kind, HANDLE_RADIUS_POINTS * 1.35, 1.0);
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
        BuildTask::Walk { order, .. } => if order.kind == OrderKind::AttackMove { "Attack-move" } else { "Move" }.to_string(),
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

/// One tier of one kind of factory in the quick select bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickTier {
    pub label: String,
    pub units: Vec<SimId>,
}

/// One kind of factory in the quick select bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickFamily {
    pub name: String,
    pub tiers: Vec<QuickTier>,
}

/// What the quick select bar offers right now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuickSelect {
    pub commander: Option<SimId>,
    pub families: Vec<QuickFamily>,
}

/// "land_factory_t2" is the kind "Land Factory". Later kinds of factory get their own rows.
fn factory_family_name(key: &str) -> String {
    let base = key.rsplit_once("_t").filter(|(_, tier)| tier.parse::<u8>().is_ok()).map_or(key, |(base, _)| base);
    base.split('_')
        .map(|word| {
            let mut letters = word.chars();
            letters.next().map_or(String::new(), |first| first.to_uppercase().chain(letters).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Sorts a team's commander and factories into the bar's rows. Tiers and kinds with
/// nothing in them do not appear. Factories still being built count, as they take orders.
pub fn quick_select_groups(snapshot: &SimSnapshot, team: Team, db: &BlueprintDb) -> QuickSelect {
    let mut groups = QuickSelect::default();
    let mut families: BTreeMap<String, BTreeMap<u8, Vec<SimId>>> = BTreeMap::new();
    for unit in snapshot.units.iter().filter(|unit| unit.team == team && unit.blueprint != u16::MAX) {
        let blueprint = db.get(BlueprintId(unit.blueprint));
        if blueprint.is_commander() {
            groups.commander.get_or_insert(unit.id);
        } else if blueprint.is_factory() {
            families.entry(factory_family_name(&blueprint.key)).or_default().entry(blueprint.tier).or_default().push(unit.id);
        }
    }
    groups.families = families
        .into_iter()
        .map(|(name, tiers)| QuickFamily {
            name,
            tiers: tiers.into_iter().map(|(tier, units)| QuickTier { label: format!("T{tier}  x{}", units.len()), units }).collect(),
        })
        .collect();
    groups
}

/// The one of these units closest to a point on the map, with the lowest id winning a tie.
pub fn closest_unit(snapshot: &SimSnapshot, units: &[SimId], point: [f64; 2]) -> Option<SimId> {
    let distance = |id: SimId| {
        find_unit(snapshot, id).map(|unit| (f64::from(unit.x) / 4096.0 - point[0]).hypot(f64::from(unit.y) / 4096.0 - point[1]))
    };
    let mut best: Option<(f64, SimId)> = None;
    for &id in units {
        if let Some(away) = distance(id) {
            if best.is_none_or(|(closest, _)| away < closest) {
                best = Some((away, id));
            }
        }
    }
    best.map(|(_, id)| id)
}

/// What the quick select bar remembers between frames.
#[derive(Clone, Debug, Default)]
pub struct QuickSelectState {
    expanded: BTreeSet<String>,
    last_click: Option<(String, f64)>,
}

impl QuickSelectState {
    /// Opens a kind of factory, as a click on it would.
    pub fn expand(&mut self, family: &str) {
        self.expanded.insert(family.to_string());
    }

    /// Returns whether this click makes a double-click on the same row.
    fn register_click(&mut self, row: &str, now: f64) -> bool {
        let double = self.last_click.as_ref().is_some_and(|(last, at)| last == row && now - at <= QUICK_DOUBLE_CLICK_SECONDS);
        // A third click starts over, instead of counting as another double-click.
        self.last_click = if double { None } else { Some((row.to_string(), now)) };
        double
    }
}

fn quick_row(ui: &mut egui::Ui, text: &str, class: UnitClass, color: Color32, selected: bool) -> egui::Response {
    bar_row(ui, QUICK_ROW_WIDTH, text, class, color, selected)
}

fn bar_row(ui: &mut egui::Ui, width: f32, text: &str, class: UnitClass, color: Color32, selected: bool) -> egui::Response {
    // Drawn by hand, because a wide button would center its text over the icon.
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 24.0), egui::Sense::click());
    let visuals = ui.style().interact_selectable(&response, selected);
    let painter = ui.painter();
    painter.rect(rect, visuals.rounding, visuals.weak_bg_fill, visuals.bg_stroke);
    draw_icon(painter, Pos2::new(rect.left() + 17.0, rect.center().y), class, color, selected);
    painter.text(Pos2::new(rect.left() + 34.0, rect.center().y), Align2::LEFT_CENTER, text, FontId::proportional(14.0), visuals.text_color());
    response
}

#[allow(clippy::too_many_arguments)]
fn quick_rows(
    ui: &mut egui::Ui,
    snapshot: &SimSnapshot,
    groups: &QuickSelect,
    color: Color32,
    selected: &[SimId],
    view_center: [f64; 2],
    state: &mut QuickSelectState,
    actions: &mut Vec<UiAction>,
) {
    let (shift, now) = ui.input(|input| (input.modifiers.shift, input.time));
    if let Some(commander) = groups.commander {
        if quick_row(ui, "ACU", UnitClass::Commander, color, selected.contains(&commander)).on_hover_text("Click to select. The view stays where it is.").clicked() {
            actions.push(UiAction::QuickSelect { units: vec![commander], add: shift });
        }
    }
    for family in &groups.families {
        let open = state.expanded.contains(&family.name);
        let total: usize = family.tiers.iter().map(|tier| tier.units.len()).sum();
        let text = format!("{} {}  x{total}", if open { "-" } else { "+" }, family.name);
        if quick_row(ui, &text, UnitClass::Factory, color, false).clicked() {
            if open {
                state.expanded.remove(&family.name);
            } else {
                state.expanded.insert(family.name.clone());
            }
        }
        if !open {
            continue;
        }
        for tier in &family.tiers {
            let any_selected = tier.units.iter().any(|id| selected.contains(id));
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                let hint = "Click for the closest to your view. Double-click for all.";
                if quick_row(ui, &tier.label, UnitClass::Factory, color, any_selected).on_hover_text(hint).clicked() {
                    let row = format!("{} {}", family.name, tier.label.split(' ').next().unwrap_or(""));
                    let units = if state.register_click(&row, now) {
                        tier.units.clone()
                    } else {
                        closest_unit(snapshot, &tier.units, view_center).into_iter().collect()
                    };
                    actions.push(UiAction::QuickSelect { units, add: shift });
                }
            });
        }
    }
}

/// The bar on the right: select the commander or factories from anywhere, without moving the view.
#[allow(clippy::too_many_arguments)]
pub fn quick_select_bar(
    ctx: &egui::Context,
    snapshot: &SimSnapshot,
    team: Team,
    db: &BlueprintDb,
    selected: &[SimId],
    view_center: [f64; 2],
    state: &mut QuickSelectState,
    actions: &mut Vec<UiAction>,
) {
    let groups = quick_select_groups(snapshot, team, db);
    if groups.commander.is_none() && groups.families.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("quick select")).anchor(Align2::RIGHT_TOP, Vec2::new(-8.0, 36.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            quick_rows(ui, snapshot, &groups, team_color(team), selected, view_center, state, actions);
        });
    });
}

/// A label that never wraps. A popup near the middle of the screen has little room by default,
/// which would otherwise squeeze its text into a tall, narrow column.
fn one_line_label(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) {
    ui.add(egui::Label::new(text).extend());
}

/// What a control group's row says: its number, and what is in it.
pub fn control_group_label(group: usize, members: &[SimId], snapshot: &SimSnapshot, db: &BlueprintDb) -> String {
    let short_name = |unit: &UnitView| {
        let blueprint = db.get(BlueprintId(unit.blueprint));
        if blueprint.is_commander() { "ACU".to_string() } else { blueprint.name.clone() }
    };
    let mut names = members.iter().filter_map(|id| find_unit(snapshot, *id)).filter(|unit| unit.blueprint != u16::MAX).map(short_name);
    let first = names.next();
    let what = match first {
        Some(name) if names.all(|other| other == name) => {
            if members.len() == 1 { name } else { format!("{name} x{}", members.len()) }
        }
        _ => format!("{} units", members.len()),
    };
    format!("{group}: {what}")
}

fn control_group_rows(ui: &mut egui::Ui, groups: &[(usize, Vec<SimId>)], snapshot: &SimSnapshot, db: &BlueprintDb, color: Color32, selected: &[SimId], actions: &mut Vec<UiAction>) {
    let shift = ui.input(|input| input.modifiers.shift);
    for (group, members) in groups {
        let class = members
            .first()
            .and_then(|id| find_unit(snapshot, *id))
            .filter(|unit| unit.blueprint != u16::MAX)
            .map_or(UnitClass::Bot, |unit| db.get(BlueprintId(unit.blueprint)).class);
        let any_selected = members.iter().any(|id| selected.contains(id));
        let label = control_group_label(*group, members, snapshot, db);
        let hint = format!("Click or press {group} to select. Hold Shift to add. Ctrl + {group} with nothing selected clears it.");
        if bar_row(ui, CONTROL_GROUP_ROW_WIDTH, &label, class, color, any_selected).on_hover_text(hint).clicked() {
            actions.push(UiAction::QuickSelect { units: members.clone(), add: shift });
        }
    }
}

/// The bar on the left: the saved control groups, and what is in them.
pub fn control_group_bar(ctx: &egui::Context, groups: &[(usize, Vec<SimId>)], snapshot: &SimSnapshot, team: Team, db: &BlueprintDb, selected: &[SimId], actions: &mut Vec<UiAction>) {
    if groups.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("control groups")).anchor(Align2::LEFT_TOP, Vec2::new(8.0, 36.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| control_group_rows(ui, groups, snapshot, db, team_color(team), selected, actions));
    });
}

/// The text alert that goes with the "ACU under attack" sound.
pub fn commander_alert(ctx: &egui::Context, strength: f32) {
    if strength <= 0.0 {
        return;
    }
    egui::Area::new(egui::Id::new("commander alert"))
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 150.0))
        .interactable(false)
        .show(ctx, |ui| {
            let text = egui::RichText::new("ACU under attack").size(22.0).strong().color(STALLED.gamma_multiply(strength));
            egui::Frame::popup(ui.style()).multiply_with_opacity(strength).show(ui, |ui| {
                one_line_label(ui, text);
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
pub fn resource_bar(ctx: &egui::Context, snapshot: &SimSnapshot, sound: Option<&str>) {
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
                if let Some(sound) = sound {
                    ui.separator();
                    ui.label(sound);
                }
            });
        });
    });
}

/// The panel along the bottom: what is selected, and what it can build.
pub fn command_panel(ctx: &egui::Context, snapshot: &SimSnapshot, selected: &[SimId], db: &BlueprintDb, armed: Option<ArmedOrder>, actions: &mut Vec<UiAction>) {
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
            let rank = rank_of(snapshot, first.id);
            let veteran = if rank > 0 { format!(", veteran rank {rank}") } else { String::new() };
            let shield = shield_status(snapshot, first.id).map_or(String::new(), |status| format!(", {status}"));
            let stock = stock_status(snapshot, first.id, db).map_or(String::new(), |status| format!(", {status}"));
            let intel = intel_status(snapshot, first.id).map_or(String::new(), |status| format!(", {status}"));
            format!("{}: {}% health{veteran}{shield}{stock}{intel}", unit.name, first.health_permille / 10)
        } else {
            format!("{} units selected", units.len())
        };
        ui.horizontal(|ui| {
            ui.strong(summary);
            let rates = selection_rates(snapshot, selected, db);
            if !rates.output.is_empty() {
                ui.separator();
                ui.colored_label(HEALTHY, rates.output);
            }
            if !rates.spending.is_empty() {
                ui.separator();
                ui.colored_label(SPENDING, rates.spending);
            }
        });

        if has_mobile_units(snapshot, selected) {
            unit_order_row(ui, armed, has_overcharge(snapshot, selected, db), has_weapons(snapshot, selected, db), actions);
        }
        if let Some(action) = cloak_action(snapshot, selected) {
            let UiAction::SetCloak { on, .. } = &action else {
                unreachable!("cloak_action only returns a cloak");
            };
            // O pauses first, when there is something to pause.
            let key = if pause_action(snapshot, selected, db).is_none() { format!("[{}] ", hotkeys::PAUSE) } else { String::new() };
            let label = if *on { format!("{key}Cloak") } else { format!("{key}Uncloak") };
            let hint = "Hidden from sight and radar, except from enemies within 100 or with omni on it. Costs 50 energy a second.";
            if ui.button(label).on_hover_text(hint).clicked() {
                actions.push(action);
            }
        }
        if let Some(action) = pause_action(snapshot, selected, db) {
            let UiAction::SetPaused { paused, .. } = &action else {
                unreachable!("pause_action only returns a pause");
            };
            let label = if *paused { format!("[{}] Pause", hotkeys::PAUSE) } else { format!("[{}] Resume", hotkeys::PAUSE) };
            let hint = "Stops spending and making progress, keeping the queue and what is done so far.";
            ui.horizontal(|ui| {
                if ui.button(label).on_hover_text(hint).clicked() {
                    actions.push(action.clone());
                }
                if !*paused {
                    ui.colored_label(Color32::YELLOW, "Paused");
                }
            });
        }

        // An upgrade in place: what it costs, or how far along it is.
        let upgradable = upgradable_units(snapshot, selected, db);
        if let Some(first) = upgradable.first().and_then(|id| find_unit(snapshot, *id)) {
            let into = db.get(db.get(BlueprintId(first.blueprint)).upgrades_to.expect("only upgradable units are listed"));
            let label = format!("[{}] Upgrade: {}\n{} mass  {} energy", hotkeys::UPGRADE, into.name, into.mass.to_num::<i64>(), into.energy.to_num::<i64>());
            let hint = "A commander stands still while it upgrades, and a new order cancels it.";
            if ui.button(label).on_hover_text(hint).clicked() {
                actions.push(UiAction::Upgrade { units: upgradable });
            }
        }
        for &(id, permille) in snapshot.upgrades.iter().filter(|(id, _)| selected.contains(id)) {
            if let Some(unit) = find_unit(snapshot, id) {
                let into = db.get(BlueprintId(unit.blueprint)).upgrades_to.map_or("", |into| db.get(into).name.as_str());
                ui.colored_label(SPENDING, format!("Upgrading to {into}: {}%", permille / 10));
            }
        }

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
        // Factories of different tiers upgrade to different things, so the button is for the
        // lowest tier selected, and only the factories that upgrade to the same thing take it.
        if let Some(upgrade) = blueprint.upgrades_to.filter(|upgrade| !factory.queue.contains(&upgrade.0)) {
            match &mut menu.upgrade {
                Some((offered, able)) if *offered == upgrade => able.push(id),
                Some((offered, _)) if db.get(*offered).tier <= db.get(upgrade).tier => {}
                _ => menu.upgrade = Some((upgrade, vec![id])),
            }
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
    let shift = ui.input(|input| input.modifiers.shift);
    ui.horizontal_wrapped(|ui| {
        for (option, able) in &menu.options {
            let hint = format!("Hold Shift to queue {SHIFT_QUEUE_COUNT}.");
            if ui.button(format!("{}{}", cost_label(db, *option), partial(able.len()))).on_hover_text(hint).clicked() {
                actions.push(with_shift(queue_action(menu, able, *option), shift, db));
            }
        }
        if let Some((upgrade, able)) = &menu.upgrade {
            let cost = cost_label(db, *upgrade);
            let label = format!("[{}] Upgrade to T{}\n{}{}", hotkeys::UPGRADE, db.get(*upgrade).tier, cost.lines().nth(1).unwrap_or(""), partial(able.len()));
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
pub fn armed_order_hint(ctx: &egui::Context, order: ArmedOrder) {
    egui::Area::new(egui::Id::new("armed order hint"))
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 36.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                one_line_label(ui, order.hint());
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
                    one_line_label(ui, egui::RichText::new(&label.text).strong().color(color));
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
        PlanStepKind::Attack | PlanStepKind::AttackGround => MarkerKind::Attack,
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

/// The outer edge of where any of these circles reach, as short straight pieces: each circle's
/// rim, less what lies inside another circle. Circles are `(center, radius)` in world units.
pub fn coverage_outline(circles: &[([f64; 2], f64)]) -> Vec<[[f64; 2]; 2]> {
    let mut pieces = Vec::new();
    for (index, &(center, radius)) in circles.iter().enumerate() {
        if radius <= 0.0 {
            continue;
        }
        // About 40 units a piece, which looks round from anywhere it is drawn.
        let count = (std::f64::consts::TAU * radius / 40.0).clamp(32.0, 512.0) as usize;
        let at = |angle: f64| [center[0] + radius * angle.cos(), center[1] + radius * angle.sin()];
        let inside_another = |point: [f64; 2]| {
            circles.iter().enumerate().any(|(other, &(middle, reach))| {
                if other == index {
                    false
                } else if middle == center && reach == radius {
                    // Of two identical circles, only the first draws the rim they share.
                    other < index
                } else {
                    (point[0] - middle[0]).hypot(point[1] - middle[1]) < reach - 1e-6
                }
            })
        };
        for piece in 0..count {
            let (from, to) = (piece as f64 / count as f64 * std::f64::consts::TAU, (piece + 1) as f64 / count as f64 * std::f64::consts::TAU);
            if !inside_another(at((from + to) * 0.5)) {
                pieces.push([at(from), at(to)]);
            }
        }
    }
    pieces
}

/// When zoomed out, outlines what the viewer's radar covers, and within that, its omni.
pub fn draw_sensor_coverage(ctx: &egui::Context, camera: &Camera, heights: &crate::render::camera::HeightField, ranges: &[crate::sim::world::SensorRangeView], pixels_per_point: f32) {
    // Fades in from about halfway out, so that it never clutters play up close.
    let strength = ((camera.zoom_from_closest() - 0.45) / 0.15).clamp(0.0, 1.0) as f32;
    if strength <= 0.0 || ranges.is_empty() {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::background());
    let to_points = |point: [f64; 2]| {
        let world = [point[0], point[1], heights.height_at(point[0], point[1]) + 2.0];
        camera.world_to_screen(world).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
    };
    let world = |raw: i32| f64::from(raw) / 4096.0;
    for (pick, color) in [(0usize, Color32::from_rgb(120, 230, 140)), (1, Color32::from_rgb(200, 140, 255))] {
        let circles: Vec<([f64; 2], f64)> =
            ranges.iter().map(|range| ([world(range.center[0]), world(range.center[1])], world(if pick == 0 { range.radar } else { range.omni }))).filter(|circle| circle.1 > 0.0).collect();
        for [from, to] in coverage_outline(&circles) {
            if let (Some(from), Some(to)) = (to_points(from), to_points(to)) {
                painter.line_segment([from, to], Stroke::new(1.5_f32, color.gamma_multiply(0.8 * strength)));
            }
        }
    }
}

/// Radar contacts the viewer cannot see, as small triangles that say nothing of what they are.
pub fn draw_blips(ctx: &egui::Context, camera: &Camera, blips: &[[i32; 3]], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let color = Color32::from_rgb(255, 190, 80);
    for blip in blips {
        let world = [f64::from(blip[0]) / 4096.0, f64::from(blip[1]) / 4096.0, f64::from(blip[2]) / 4096.0];
        let Some(pixels) = camera.world_to_screen(world) else {
            continue;
        };
        let center = Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point);
        let corners = vec![center + Vec2::new(0.0, -6.0), center + Vec2::new(5.5, 4.0), center + Vec2::new(-5.5, 4.0)];
        painter.add(egui::Shape::convex_polygon(corners, color.gamma_multiply(0.35), Stroke::new(1.5_f32, color)));
    }
}

/// Flashes a line from each missile defense to the missile it shot down this tick.
pub fn draw_interceptions(ctx: &egui::Context, camera: &Camera, interceptions: &[[[i32; 3]; 2]], pixels_per_point: f32) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let to_points = |raw: [i32; 3]| {
        let world = [f64::from(raw[0]) / 4096.0, f64::from(raw[1]) / 4096.0, f64::from(raw[2]) / 4096.0];
        camera.world_to_screen(world).map(|pixels| Pos2::new(pixels[0] as f32 / pixels_per_point, pixels[1] as f32 / pixels_per_point))
    };
    let color = Color32::from_rgb(255, 120, 90);
    for &[from, to] in interceptions {
        let (Some(from), Some(to)) = (to_points(from), to_points(to)) else {
            continue;
        };
        painter.line_segment([from, to], Stroke::new(6.0_f32, color.gamma_multiply(0.25)));
        painter.line_segment([from, to], Stroke::new(2.0_f32, Color32::WHITE));
        painter.circle_filled(to, 6.0, Color32::from_rgb(255, 230, 160));
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
    let stroke = Stroke::new(2.0_f32, color.gamma_multiply(0.9));
    let fill = color.gamma_multiply(0.25);
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
        // Never on the map.
        UnitClass::Ammo => {}
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
        if snapshot.paused.binary_search(&unit.id).is_ok() {
            painter.text(center, Align2::CENTER_CENTER, "PAUSED", FontId::proportional(13.0), Color32::YELLOW);
        }
        let rank = rank_of(snapshot, unit.id);
        if rank > 0 {
            // One gold pip per rank, under the unit.
            let spacing = 5.0;
            let left = center.x - spacing * f32::from(rank - 1) / 2.0;
            for pip in 0..rank {
                painter.circle_filled(Pos2::new(left + spacing * f32::from(pip), center.y + 10.0), 2.0, VETERAN);
            }
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
    use crate::sim::world::{BuildQueueView, FactoryView, OrderStopView, PlanStep, UnitPlanView, WorkLinkView};

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
            facing: [0, 0], aim: [0, 0],
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
        assert_eq!(menu.upgrade, Some((db.id("land_factory_t3"), vec![SimId(2)])), "only the T2 factory, to T3");
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
        let expected = UiAction::Queue { factories: vec![SimId(1)], blueprint: tank, selected_count: 1, count: 1 };
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
    }

    #[test]
    fn factory_keys_reach_every_factory_able_to_follow_them() {
        let db = BlueprintDb::standard();
        let current = mixed_factories(&db);
        let selected = [SimId(1), SimId(2), SimId(3)];
        let queue = |key: &str, factories: Vec<SimId>| UiAction::Queue { factories, blueprint: db.id(key), selected_count: 3, count: 1 };
        assert_eq!(hotkey_action(&current, &selected, &db, 'Z'), Some(queue("t1_engineer", selected.to_vec())));
        assert_eq!(hotkey_action(&current, &selected, &db, 'B'), Some(queue("t1_light_bot", selected.to_vec())));
        assert_eq!(hotkey_action(&current, &selected, &db, 'V'), Some(queue("t1_light_artillery", selected.to_vec())));
        assert_eq!(hotkey_action(&current, &selected, &db, 'H'), Some(queue("t2_heavy_tank", vec![SimId(2)])));
        assert_eq!(hotkey_action(&current, &selected, &db, 'U'), Some(queue("land_factory_t2", vec![SimId(1), SimId(3)])), "the lowest tier goes first");
        let to_t3 = UiAction::Queue { factories: vec![SimId(2)], blueprint: db.id("land_factory_t3"), selected_count: 1, count: 1 };
        assert_eq!(hotkey_action(&current, &[SimId(2)], &db, 'U'), Some(to_t3));
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
        let expected = UiAction::Queue { factories: vec![SimId(1)], blueprint: db.id("t1_engineer"), selected_count: 1, count: 1 };
        assert_eq!(hotkey_action(&current, &selected, &db, 'Z'), Some(expected));
    }

    #[test]
    fn unit_order_keys_arm_their_orders_for_anything_that_can_walk() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        for selected in [SimId(10), SimId(11), SimId(12)] {
            for (letter, order) in [('M', ArmedOrder::Move), ('F', ArmedOrder::AttackMove), ('J', ArmedOrder::Formation), ('Y', ArmedOrder::Assist)] {
                assert_eq!(hotkey_action(&current, &[selected], &db, letter), Some(UiAction::Arm(order)), "{letter} for {selected:?}");
            }
            assert_eq!(hotkey_action(&current, &[selected], &db, 'X'), Some(UiAction::Stop));
        }
        assert_eq!(hotkey_action(&current, &[], &db, 'M'), None, "nothing is selected");
    }

    #[test]
    fn coverage_outline_follows_the_outer_edge_of_overlapping_circles() {
        let length = |pieces: &[[[f64; 2]; 2]]| pieces.iter().map(|[a, b]| (a[0] - b[0]).hypot(a[1] - b[1])).sum::<f64>();
        let tau = std::f64::consts::TAU;
        // One circle: its whole rim.
        let one = coverage_outline(&[([0.0, 0.0], 1_000.0)]);
        assert!((length(&one) - tau * 1_000.0).abs() < 20.0);
        // A circle inside another adds nothing.
        let nested = coverage_outline(&[([0.0, 0.0], 1_000.0), ([200.0, 0.0], 300.0)]);
        assert!((length(&nested) - length(&one)).abs() < 1.0, "the inner circle should not show");
        // Two the same draw one rim, not two.
        let twins = coverage_outline(&[([0.0, 0.0], 1_000.0), ([0.0, 0.0], 1_000.0)]);
        assert!((length(&twins) - length(&one)).abs() < 1.0);
        // Two overlapping by half a radius lose the arcs inside each other: a third of each rim.
        let pair = coverage_outline(&[([0.0, 0.0], 1_000.0), ([1_000.0, 0.0], 1_000.0)]);
        let expected = 2.0 * tau * 1_000.0 * (2.0 / 3.0);
        assert!((length(&pair) - expected).abs() < 60.0, "{} against {expected}", length(&pair));
        for [a, b] in &pair {
            let middle = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            assert!(middle[0].hypot(middle[1]) > 990.0 && (middle[0] - 1_000.0).hypot(middle[1]) > 990.0, "a piece inside the other circle");
        }
    }

    #[test]
    fn o_toggles_a_cloak_when_there_is_nothing_to_pause() {
        let db = BlueprintDb::standard();
        let mut current = commander_and_engineer(&db);
        current.cloaks = vec![(SimId(10), false, false)];
        assert_eq!(hotkey_action(&current, &[SimId(10)], &db, 'O'), Some(UiAction::SetCloak { units: vec![SimId(10)], on: true }));
        current.cloaks = vec![(SimId(10), true, true)];
        assert_eq!(hotkey_action(&current, &[SimId(10)], &db, 'O'), Some(UiAction::SetCloak { units: vec![SimId(10)], on: false }));
        assert_eq!(hotkey_action(&current, &[SimId(12)], &db, 'O'), None, "a bot has no cloak");
    }

    #[test]
    fn a_shield_says_how_strong_it_is_or_when_it_will_be_back() {
        use crate::sim::components::Team;
        use crate::sim::world::ShieldView;
        let view = |up: bool, recharging: u32| ShieldView { id: SimId(5), team: Team::Blue, center: [0; 3], radius: 0, health_permille: 640, up, recharging };
        let with = |shield: ShieldView| SimSnapshot { shields: vec![shield], ..SimSnapshot::default() };
        assert_eq!(shield_status(&with(view(true, 0)), SimId(5)).as_deref(), Some("shield 64%"));
        assert_eq!(shield_status(&with(view(false, 291)), SimId(5)).as_deref(), Some("shield down, back in 30 s"));
        assert_eq!(shield_status(&with(view(false, 0)), SimId(5)).as_deref(), Some("shield off"));
        assert_eq!(shield_status(&with(view(true, 0)), SimId(6)), None, "only a shield generator has one");
    }

    #[test]
    fn attack_ground_is_only_offered_to_a_selection_with_a_gun() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        let armed: Vec<SimId> = current.units.iter().filter(|unit| db.get(BlueprintId(unit.blueprint)).weapon.is_some()).map(|unit| unit.id).collect();
        let unarmed: Vec<SimId> = current.units.iter().filter(|unit| db.get(BlueprintId(unit.blueprint)).weapon.is_none()).map(|unit| unit.id).collect();
        assert!(!armed.is_empty() && !unarmed.is_empty());
        assert_eq!(hotkey_action(&current, &armed[..1], &db, 'I'), Some(UiAction::Arm(ArmedOrder::AttackGround)));
        assert_eq!(hotkey_action(&current, &unarmed[..1], &db, 'I'), None);
    }

    #[test]
    fn structures_do_not_take_unit_orders_but_a_mixed_selection_does() {
        let db = BlueprintDb::standard();
        let mut current = mixed_factories(&db);
        for letter in ['M', 'F', 'J', 'Y', 'X'] {
            assert_eq!(hotkey_action(&current, &[SimId(1)], &db, letter), None, "{letter} for a factory");
        }
        let mut engineer = unit(4, 0.0);
        engineer.blueprint = db.id("t1_engineer").0;
        current.units.push(engineer);
        assert!(has_mobile_units(&current, &[SimId(1), SimId(4)]));
        assert!(!has_mobile_units(&current, &[SimId(1), SimId(2)]));
    }

    #[test]
    fn every_unit_order_says_what_to_do_next() {
        for order in ArmedOrder::ALL {
            assert!(order.hint().starts_with(order.name()), "{}", order.hint());
        }
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

    fn base_with_factories(db: &BlueprintDb) -> SimSnapshot {
        let raw = |value: f64| (value * 4096.0) as i32;
        let make = |id: u32, key: &str, x: f64, built: u16, team: Team| {
            let mut view = unit(id, x);
            view.x = raw(x);
            view.blueprint = db.id(key).0;
            view.is_structure = key != "acu";
            view.built_permille = built;
            view.team = team;
            view
        };
        snapshot(
            vec![
                make(1, "acu", 0.0, 1000, Team::Blue),
                make(2, "land_factory_t1", 100.0, 1000, Team::Blue),
                make(3, "land_factory_t1", 900.0, 400, Team::Blue),
                make(4, "land_factory_t2", 500.0, 1000, Team::Blue),
                make(5, "land_factory_t2", 700.0, 1000, Team::Blue),
                make(6, "power_generator", 50.0, 1000, Team::Blue),
                make(7, "land_factory_t1", 300.0, 1000, Team::Red),
                make(8, "acu", 300.0, 1000, Team::Red),
            ],
            vec![],
        )
    }

    #[test]
    fn the_bar_lists_your_commander_and_factories_by_kind_and_tier() {
        let db = BlueprintDb::standard();
        let groups = quick_select_groups(&base_with_factories(&db), Team::Blue, &db);
        assert_eq!(groups.commander, Some(SimId(1)));
        assert_eq!(groups.families.len(), 1);
        assert_eq!(groups.families[0].name, "Land Factory");
        let tiers = &groups.families[0].tiers;
        assert_eq!(tiers.len(), 2);
        assert_eq!((tiers[0].label.as_str(), tiers[0].units.as_slice()), ("T1  x2", [SimId(2), SimId(3)].as_slice()), "an unfinished one counts");
        assert_eq!((tiers[1].label.as_str(), tiers[1].units.as_slice()), ("T2  x2", [SimId(4), SimId(5)].as_slice()));
    }

    #[test]
    fn empty_tiers_and_kinds_are_left_out() {
        let db = BlueprintDb::standard();
        let mut current = base_with_factories(&db);
        current.units.retain(|unit| unit.id != SimId(4) && unit.id != SimId(5));
        let groups = quick_select_groups(&current, Team::Blue, &db);
        assert_eq!(groups.families[0].tiers.len(), 1, "no T2 factories");
        current.units.retain(|unit| unit.team == Team::Blue && !db.get(BlueprintId(unit.blueprint)).is_factory());
        assert!(quick_select_groups(&current, Team::Blue, &db).families.is_empty());
        assert_eq!(quick_select_groups(&snapshot(vec![], vec![]), Team::Blue, &db), QuickSelect::default());
    }

    #[test]
    fn kinds_of_factory_get_readable_names() {
        assert_eq!(factory_family_name("land_factory_t1"), "Land Factory");
        assert_eq!(factory_family_name("land_factory_t2"), "Land Factory");
        assert_eq!(factory_family_name("air_factory_t3"), "Air Factory");
        assert_eq!(factory_family_name("shipyard"), "Shipyard");
    }

    #[test]
    fn closest_means_closest_to_the_view_and_a_tie_goes_to_the_lowest_id() {
        let db = BlueprintDb::standard();
        let current = base_with_factories(&db);
        let t2 = [SimId(4), SimId(5)];
        assert_eq!(closest_unit(&current, &t2, [520.0, 0.0]), Some(SimId(4)));
        assert_eq!(closest_unit(&current, &t2, [680.0, 0.0]), Some(SimId(5)));
        assert_eq!(closest_unit(&current, &t2, [600.0, 0.0]), Some(SimId(4)));
        assert_eq!(closest_unit(&current, &[SimId(99)], [0.0, 0.0]), None);
    }

    /// Clicks on the bar's rows through real egui input.
    struct BarTest {
        ctx: egui::Context,
        current: SimSnapshot,
        groups: QuickSelect,
        state: QuickSelectState,
        time: f64,
    }

    impl BarTest {
        fn new() -> Self {
            let db = BlueprintDb::standard();
            let current = base_with_factories(&db);
            let groups = quick_select_groups(&current, Team::Blue, &db);
            let mut test = Self { ctx: egui::Context::default(), current, groups, state: QuickSelectState::default(), time: 10.0 };
            test.frame(vec![], egui::Modifiers::NONE);
            test
        }

        fn frame(&mut self, events: Vec<egui::Event>, modifiers: egui::Modifiers) -> Vec<UiAction> {
            self.time += 0.02;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                time: Some(self.time),
                modifiers,
                events,
                ..egui::RawInput::default()
            };
            let mut actions = Vec::new();
            let (current, groups, state) = (&self.current, &self.groups, &mut self.state);
            let _ = self.ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    quick_rows(ui, current, groups, BLUE, &[], [680.0, 0.0], state, &mut actions);
                });
            });
            actions
        }

        /// Row 0 is the ACU, row 1 the factory kind, and rows 2 and 3 its tiers.
        fn click(&mut self, row: usize, modifiers: egui::Modifiers) -> Vec<UiAction> {
            let at = egui::pos2(70.0, 20.0 + row as f32 * 27.0);
            let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers };
            let mut actions = self.frame(vec![egui::Event::PointerMoved(at)], modifiers);
            actions.extend(self.frame(vec![press(true)], modifiers));
            actions.extend(self.frame(vec![press(false)], modifiers));
            actions.extend(self.frame(vec![], modifiers));
            actions
        }
    }

    #[test]
    fn clicking_the_commander_selects_it_and_shift_adds_it() {
        let mut bar = BarTest::new();
        assert_eq!(bar.click(0, egui::Modifiers::NONE), [UiAction::QuickSelect { units: vec![SimId(1)], add: false }]);
        bar.time += 1.0;
        assert_eq!(bar.click(0, egui::Modifiers::SHIFT), [UiAction::QuickSelect { units: vec![SimId(1)], add: true }]);
    }

    #[test]
    fn clicking_a_kind_only_opens_and_closes_it() {
        let mut bar = BarTest::new();
        assert!(bar.click(2, egui::Modifiers::NONE).is_empty(), "closed, so there is no tier row there");
        assert!(bar.click(1, egui::Modifiers::NONE).is_empty(), "opening must not change the selection");
        assert!(bar.state.expanded.contains("Land Factory"));
        bar.time += 1.0;
        assert!(bar.click(1, egui::Modifiers::NONE).is_empty());
        assert!(!bar.state.expanded.contains("Land Factory"));
    }

    #[test]
    fn clicking_a_tier_selects_the_closest_and_double_clicking_selects_all() {
        let mut bar = BarTest::new();
        bar.click(1, egui::Modifiers::NONE);
        bar.time += 1.0;
        let closest = || vec![UiAction::QuickSelect { units: vec![SimId(5)], add: false }];
        assert_eq!(bar.click(3, egui::Modifiers::NONE), closest(), "the T2 factory nearest the view");
        assert_eq!(bar.click(3, egui::Modifiers::NONE), [UiAction::QuickSelect { units: vec![SimId(4), SimId(5)], add: false }], "a second click soon after");

        // Too slow to be a double-click.
        bar.time += 1.0;
        assert_eq!(bar.click(3, egui::Modifiers::NONE), closest());
        bar.time += 1.0;
        assert_eq!(bar.click(3, egui::Modifiers::NONE), closest());
    }

    #[test]
    fn shift_adds_from_a_tier_and_clicks_on_different_tiers_are_not_a_double_click() {
        let mut bar = BarTest::new();
        bar.click(1, egui::Modifiers::NONE);
        bar.time += 1.0;
        assert_eq!(bar.click(3, egui::Modifiers::SHIFT), [UiAction::QuickSelect { units: vec![SimId(5)], add: true }]);
        assert_eq!(bar.click(3, egui::Modifiers::SHIFT), [UiAction::QuickSelect { units: vec![SimId(4), SimId(5)], add: true }]);
        bar.time += 1.0;
        assert_eq!(bar.click(2, egui::Modifiers::NONE), [UiAction::QuickSelect { units: vec![SimId(3)], add: false }]);
        assert_eq!(bar.click(3, egui::Modifiers::NONE), [UiAction::QuickSelect { units: vec![SimId(5)], add: false }]);
    }

    /// Reproduces the squeezed popup: an area with little room, as near the middle of the screen.
    #[test]
    fn popup_text_stays_on_one_line_however_little_room_it_has() {
        let ctx = egui::Context::default();
        let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0))), ..egui::RawInput::default() };
        let id = egui::Id::new("test popup");
        for _ in 0..3 {
            let _ = ctx.run(input(), |ctx| {
                egui::Area::new(id).anchor(Align2::CENTER_TOP, Vec2::new(0.0, 80.0)).show(ctx, |ui| {
                    ui.set_max_width(40.0);
                    egui::Frame::popup(ui.style()).show(ui, |ui| one_line_label(ui, "Light Artillery queued"));
                });
            });
        }
        let rect = ctx.memory(|memory| memory.area_rect(id)).expect("the popup was shown");
        assert!(rect.width() > 100.0 && rect.height() < 50.0, "the popup is {} by {}", rect.width(), rect.height());
    }

    fn walk(route: u32) -> BuildTask {
        let order = crate::sim::components::Order { kind: OrderKind::Move, route, slot: None, slot_ticks: 0 };
        BuildTask::Walk { order, started: false }
    }

    fn stop(unit: u32, task: BuildTask, x: f64) -> OrderStopView {
        OrderStopView { unit: SimId(unit), task, position: [(x * 4096.0) as i32, 0, 0] }
    }

    #[test]
    fn a_group_order_gets_one_handle_in_the_middle_of_its_spots() {
        let db = BlueprintDb::standard();
        let build = BuildTask::Build { blueprint: db.id("power_generator"), cell_x: 5, cell_y: 5, site: None };
        let mut current = snapshot(vec![], vec![]);
        current.order_stops = vec![stop(1, walk(7), 100.0), stop(2, walk(7), 200.0), stop(3, walk(8), 500.0), stop(4, build, 900.0), stop(5, build, 900.0)];
        let handles = order_handles(&current);
        assert_eq!(handles.len(), 3);
        assert_eq!(handles[0].position[0], 150.0);
        assert_eq!(handles[0].kind, MarkerKind::Move);
        assert_eq!(handles[0].order, DraggableOrder::Walk { units: vec![SimId(1), SimId(2)], task: walk(7) });
        assert_eq!(handles[1].order, DraggableOrder::Walk { units: vec![SimId(3)], task: walk(8) });
        assert_eq!(handles[2].kind, MarkerKind::Build);
        assert_eq!(handles[2].order, DraggableOrder::Build { units: vec![SimId(4), SimId(5)], task: build });
    }

    #[test]
    fn factories_sharing_a_rally_point_share_a_handle() {
        let mut current = snapshot(vec![], vec![]);
        let rally = |id: u32, x: i32| FactoryView { id: SimId(id), queue: vec![], repeat: false, progress_permille: 0, rally: Some([x, 0, 0]) };
        current.factories = vec![rally(1, 4096), rally(2, 4096), rally(3, 8192), factory_view(4, vec![], false, 0)];
        let handles = order_handles(&current);
        assert_eq!(handles.len(), 2, "the one with no rally point has no handle");
        assert_eq!(handles[0].order, DraggableOrder::Rally { factories: vec![SimId(1), SimId(2)] });
        assert_eq!(handles[1].order, DraggableOrder::Rally { factories: vec![SimId(3)] });
    }

    #[test]
    fn handles_are_easy_to_grab_but_only_near_the_icon() {
        let mut camera = Camera::new(10_240.0, [1600.0, 1000.0]);
        camera.look_at(2_000.0, 2_000.0, 300.0);
        let near = OrderHandle { position: [2_000.0, 2_000.0, 0.0], kind: MarkerKind::Move, order: DraggableOrder::Rally { factories: vec![] } };
        let beside = OrderHandle { position: [2_100.0, 2_000.0, 0.0], ..near.clone() };
        let handles = [near.clone(), beside];
        let at = camera.world_to_screen(near.position).unwrap();
        assert_eq!(handle_under_cursor(&handles, &camera, at, 1.0), Some(0));
        assert_eq!(handle_under_cursor(&handles, &camera, [at[0] + 14.0, at[1] + 8.0], 1.0), Some(0), "a near miss still grabs it");
        assert_eq!(handle_under_cursor(&handles, &camera, [at[0], at[1] + 40.0], 1.0), None);
        assert_eq!(handle_under_cursor(&handles, &camera, [at[0], at[1] + 30.0], 2.0), Some(0), "the reach follows the screen's scale");
    }

    fn rates(id: u32, mass_income: i32, energy_income: i32, mass_use: i32, energy_use: i32) -> UnitRatesView {
        UnitRatesView { id: SimId(id), mass_income, energy_income, mass_use, energy_use }
    }

    #[test]
    fn the_panel_shows_what_structures_make_and_factories_spend() {
        let db = BlueprintDb::standard();
        let mut current = mixed_factories(&db);
        let mut generator = unit(4, 0.0);
        generator.blueprint = db.id("power_generator").0;
        let mut extractor = unit(5, 0.0);
        extractor.blueprint = db.id("mass_extractor").0;
        current.units.extend([generator, extractor]);
        current.rates = vec![rates(1, 0, 0, 36, 180), rates(2, 0, 0, 0, 0), rates(4, 0, 200, 0, 0), rates(5, 25, 0, 0, 0)];

        let of = |selected: &[SimId]| selection_rates(&current, selected, &db);
        assert_eq!(of(&[SimId(5)]), SelectionRates { output: "+2.5 mass/s".to_string(), spending: String::new() });
        assert_eq!(of(&[SimId(4)]), SelectionRates { output: "+20.0 energy/s".to_string(), spending: String::new() });
        assert_eq!(of(&[SimId(4), SimId(5)]).output, "+2.5 mass/s, +20.0 energy/s", "several add up");
        assert_eq!(of(&[SimId(1)]), SelectionRates { output: String::new(), spending: "Using 3.6 mass/s, 18.0 energy/s".to_string() });
        assert_eq!(of(&[SimId(2)]).spending, "Using nothing");
        assert_eq!(of(&[SimId(99)]), SelectionRates::default());
    }

    #[test]
    fn a_control_group_says_what_is_in_it() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        assert_eq!(control_group_label(1, &[SimId(11)], &current, &db), "1: Engineer");
        assert_eq!(control_group_label(2, &[SimId(11), SimId(11)], &current, &db), "2: Engineer x2");
        assert_eq!(control_group_label(0, &[SimId(10), SimId(12)], &current, &db), "0: 2 units");
        assert_eq!(control_group_label(3, &[SimId(10)], &current, &db), "3: ACU");
    }

    #[test]
    fn clicking_a_control_group_selects_it_and_shift_adds_it() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        let groups = vec![(1, vec![SimId(10), SimId(11)]), (0, vec![SimId(12)])];
        let click = |row: usize, modifiers: egui::Modifiers| {
            let ctx = egui::Context::default();
            let at = egui::pos2(70.0, 20.0 + row as f32 * 27.0);
            let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers };
            let mut actions = Vec::new();
            for events in [vec![], vec![egui::Event::PointerMoved(at)], vec![press(true)], vec![press(false)], vec![]] {
                let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), modifiers, events, ..egui::RawInput::default() };
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| control_group_rows(ui, &groups, &current, &db, BLUE, &[], &mut actions));
                });
            }
            actions
        };
        assert_eq!(click(0, egui::Modifiers::NONE), [UiAction::QuickSelect { units: vec![SimId(10), SimId(11)], add: false }]);
        assert_eq!(click(1, egui::Modifiers::SHIFT), [UiAction::QuickSelect { units: vec![SimId(12)], add: true }]);
    }

    #[test]
    fn shift_queues_five_units_but_only_one_upgrade() {
        let db = BlueprintDb::standard();
        let current = mixed_factories(&db);
        let press = |letter: char, shift: bool| with_shift(hotkey_action(&current, &[SimId(1)], &db, letter).unwrap(), shift, &db);
        let count_of = |action: UiAction| match action {
            UiAction::Queue { count, .. } => count,
            other => panic!("expected a queue order, not {other:?}"),
        };
        assert_eq!(count_of(press('B', false)), 1);
        assert_eq!(count_of(press('B', true)), 5);
        assert_eq!(count_of(press('Z', true)), 5);
        assert_eq!(count_of(press('U', true)), 1, "a factory only upgrades once");
        let other = with_shift(UiAction::Stop, true, &db);
        assert_eq!(other, UiAction::Stop, "other orders are not affected");
    }

    #[test]
    fn shift_clicking_a_factory_button_queues_five() {
        let db = BlueprintDb::standard();
        let current = mixed_factories(&db);
        let menu = factory_menu(&current, &[SimId(1)], &db).unwrap();
        let click = |modifiers: egui::Modifiers| {
            let ctx = egui::Context::default();
            let at = egui::pos2(40.0, 25.0);
            let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers };
            let mut actions = Vec::new();
            for events in [vec![], vec![egui::Event::PointerMoved(at)], vec![press(true)], vec![press(false)], vec![]] {
                let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 600.0))), modifiers, events, ..egui::RawInput::default() };
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| factory_controls(ui, &menu, &db, &mut actions));
                });
            }
            actions
        };
        let engineer = db.id("t1_engineer");
        let queue = |count| vec![UiAction::Queue { factories: vec![SimId(1)], blueprint: engineer, selected_count: 1, count }];
        assert_eq!(click(egui::Modifiers::NONE), queue(1));
        assert_eq!(click(egui::Modifiers::SHIFT), queue(5));
    }

    #[test]
    fn pause_covers_factories_and_sites_and_resumes_when_all_are_paused() {
        let db = BlueprintDb::standard();
        let mut current = mixed_factories(&db);
        let mut engineer = unit(4, 0.0);
        engineer.blueprint = db.id("t1_engineer").0;
        current.units.push(engineer);
        // 1 and 2 are finished factories, 3 is a factory still being built, 4 is an engineer.
        let everything = [SimId(1), SimId(2), SimId(3), SimId(4)];
        assert_eq!(pausable_units(&current, &everything, &db), [SimId(1), SimId(2), SimId(3)], "an engineer cannot be paused");
        let press = |current: &SimSnapshot| hotkey_action(current, &everything, &db, 'O');
        assert_eq!(press(&current), Some(UiAction::SetPaused { units: vec![SimId(1), SimId(2), SimId(3)], paused: true }));
        current.paused = vec![SimId(1)];
        assert_eq!(press(&current), Some(UiAction::SetPaused { units: vec![SimId(1), SimId(2), SimId(3)], paused: true }), "some still run, so pause them all");
        current.paused = vec![SimId(1), SimId(2), SimId(3)];
        assert_eq!(press(&current), Some(UiAction::SetPaused { units: vec![SimId(1), SimId(2), SimId(3)], paused: false }));
        assert_eq!(hotkey_action(&current, &[SimId(4)], &db, 'O'), None);
    }

    #[test]
    fn only_a_selection_with_a_commander_can_overcharge() {
        let db = BlueprintDb::standard();
        let current = commander_and_engineer(&db);
        assert_eq!(hotkey_action(&current, &[SimId(10)], &db, 'L'), Some(UiAction::Arm(ArmedOrder::Overcharge)));
        assert_eq!(hotkey_action(&current, &[SimId(10), SimId(12)], &db, 'L'), Some(UiAction::Arm(ArmedOrder::Overcharge)));
        assert_eq!(hotkey_action(&current, &[SimId(11), SimId(12)], &db, 'L'), None, "engineers and bots have none");
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
