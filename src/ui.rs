//! The in-game interface: resource bar, build menu, unit overlays, and the end of match screen.
//! It reads snapshots and returns actions. It never touches the simulation.
#![allow(clippy::disallowed_types)]

use egui::{Align2, Color32, FontId, Pos2, Rect, Rounding, Stroke, Vec2};

use crate::render::camera::Camera;
use crate::sim::blueprints::{BlueprintDb, BlueprintId, UnitClass};
use crate::sim::components::{SimId, Team};
use crate::sim::world::{MatchResult, SimSnapshot, UnitView};

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

/// A small floating reminder of the placement in progress.
pub fn placement_hint(ctx: &egui::Context, db: &BlueprintDb, blueprint: BlueprintId) {
    egui::Area::new(egui::Id::new("placement hint"))
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 36.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(format!("Placing {}: left-click to build, Shift to queue more, right-click to cancel", db.get(blueprint).name));
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
