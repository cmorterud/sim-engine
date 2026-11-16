//! The minimap: the whole map in a framed panel under the economy, with the land shaded by
//! height, what is out of sight darkened, every unit and blip as a dot, and the camera's view
//! outlined. Clicking or dragging on it moves the camera there, and right-clicking sends the
//! selection there.
#![allow(clippy::disallowed_types)]

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, Vec2};

use crate::hud;
use crate::render::camera::{Camera, HeightField};
use crate::render::pipeline::FOG_CELL_UNITS;
use crate::sim::components::Team;
use crate::sim::world::SimSnapshot;
use crate::ui::UiAction;

/// The minimap's side, in points.
pub const MINIMAP_SIZE: f32 = 200.0;
/// The most pixels a side the land's picture has.
const MOST_PIXELS: usize = 256;
/// Where the panel below the economy starts, remembered from the last frame.
const BELOW_ECONOMY: &str = "below economy";
/// Where the panel below the minimap starts, remembered from the last frame.
const BELOW_MINIMAP: &str = "below minimap";

const DEEP: Color32 = Color32::from_rgb(22, 52, 78);
const SHALLOW: Color32 = Color32::from_rgb(52, 108, 128);
const SAND: Color32 = Color32::from_rgb(170, 158, 116);
const LOW: Color32 = Color32::from_rgb(78, 104, 52);
const HIGH: Color32 = Color32::from_rgb(132, 132, 82);
const ROCK: Color32 = Color32::from_rgb(118, 112, 104);

/// The land seen from above, `side` pixels a side, row by row from the map's top edge: water
/// darker with depth, grass from low to high, sand along the shore, rock where it is steep, and
/// lit from the top left so that hills stand out.
pub fn land_colors(heights: &HeightField, water_level: f64, side: usize) -> Vec<Color32> {
    let size = (heights.samples() - 1) as f64 * heights.cell_units();
    let step = size / side as f64;
    let at = |x: f64, y: f64| heights.height_at(x.clamp(0.0, size), y.clamp(0.0, size));
    let (lowest, highest) = (0..side * side).fold((f64::MAX, f64::MIN), |(lo, hi), index| {
        let height = at((index % side) as f64 * step, (index / side) as f64 * step);
        (lo.min(height), hi.max(height))
    });
    let span = (highest - water_level.max(lowest)).max(1.0);
    let mut colors = Vec::with_capacity(side * side);
    // North, the way the camera looks, is up, as on the screen: the top row is the far edge.
    for row in 0..side {
        for column in 0..side {
            let (x, y) = ((column as f64 + 0.5) * step, size - (row as f64 + 0.5) * step);
            let height = at(x, y);
            let (east, south) = (at(x + step, y) - at(x - step, y), at(x, y - step) - at(x, y + step));
            let slope = (east.hypot(south) / (2.0 * step)) as f32;
            // Light from the top left: slopes facing it are lighter.
            let light = (1.0 - ((east + south) / (2.0 * step)) as f32 * 1.6).clamp(0.55, 1.35);
            let color = if height < water_level {
                hud::lerp_color(SHALLOW, DEEP, ((water_level - height) / 60.0) as f32)
            } else {
                let rise = ((height - water_level.max(lowest)) / span) as f32;
                let grass = hud::lerp_color(LOW, HIGH, rise);
                let shore = if height - water_level < 4.0 { 1.0 - ((height - water_level) / 4.0) as f32 } else { 0.0 };
                let ground = hud::lerp_color(grass, SAND, shore * 0.8);
                hud::lerp_color(ground, ROCK, ((slope - 0.35) / 0.4).clamp(0.0, 1.0))
            };
            let lit = |channel: u8| (f32::from(channel) * light).clamp(0.0, 255.0) as u8;
            colors.push(Color32::from_rgb(lit(color.r()), lit(color.g()), lit(color.b())));
        }
    }
    colors
}

/// Where a world point falls on the minimap.
fn to_minimap(rect: Rect, map_size: f64, point: [f64; 2]) -> Pos2 {
    // North up, as the camera looks: the map's far edge, +y, is the minimap's top.
    Pos2::new(rect.left() + (point[0] / map_size) as f32 * rect.width(), rect.bottom() - (point[1] / map_size) as f32 * rect.height())
}

/// The world point under a spot on the minimap, kept on the map.
pub fn from_minimap(rect: Rect, map_size: f64, spot: Pos2) -> [f64; 2] {
    let share = |offset: f32, length: f32| f64::from((offset / length).clamp(0.0, 1.0)) * map_size;
    [share(spot.x - rect.left(), rect.width()), share(rect.bottom() - spot.y, rect.height())]
}

/// The ground at the corners of the screen: bottom left, bottom right, top right, top left. A
/// top corner that looks over the horizon is moved down the screen until it meets the ground.
pub fn view_corners(camera: &Camera, heights: &HeightField) -> Vec<[f64; 2]> {
    let [width, height] = camera.viewport();
    let mut corners = Vec::with_capacity(4);
    for (x, bottom) in [(0.0, true), (width, true), (width, false), (0.0, false)] {
        let ground = if bottom {
            camera.screen_to_ground([x, height], heights)
        } else {
            (0..20).find_map(|step| camera.screen_to_ground([x, height * f64::from(step) / 20.0], heights))
        };
        if let Some(ground) = ground {
            corners.push(ground);
        }
    }
    corners
}

/// The minimap's pictures, made once for the land and again as the fog changes.
#[derive(Default)]
pub struct Minimap {
    land: Option<egui::TextureHandle>,
    fog: Option<egui::TextureHandle>,
    fog_tick: Option<u32>,
}

impl Minimap {
    /// Draws the minimap under the economy panel. `fog_side` is the size of the fog grid, if the
    /// match has fog. Returns nothing; a click or drag on it asks to move the camera.
    #[allow(clippy::too_many_arguments)]
    pub fn show(&mut self, ctx: &egui::Context, camera: &Camera, heights: &HeightField, water_level: f64, snapshot: &SimSnapshot, fog_side: Option<usize>, actions: &mut Vec<UiAction>) {
        let map_size = (heights.samples() - 1) as f64 * heights.cell_units();
        let land = self.land.get_or_insert_with(|| {
            let side = (heights.samples() - 1).clamp(16, MOST_PIXELS);
            let image = egui::ColorImage { size: [side, side], pixels: land_colors(heights, water_level, side) };
            ctx.load_texture("minimap land", image, egui::TextureOptions::LINEAR)
        });
        let land_id = land.id();
        // The fog, as dark where out of sight, updated when a new snapshot brings it.
        if let (Some(fog), Some(side)) = (&snapshot.fog, fog_side) {
            if self.fog_tick != Some(snapshot.tick) {
                self.fog_tick = Some(snapshot.tick);
                let pixels = fog.iter().map(|&seen| if seen == u8::MAX { Color32::TRANSPARENT } else { Color32::from_black_alpha(130) }).collect();
                let image = egui::ColorImage { size: [side, side], pixels };
                match &mut self.fog {
                    Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                    None => self.fog = Some(ctx.load_texture("minimap fog", image, egui::TextureOptions::LINEAR)),
                }
            }
        } else {
            self.fog = None;
        }
        let fog = self.fog.as_ref().zip(fog_side).map(|(texture, side)| (texture.id(), side));
        let top = ctx.data(|data| data.get_temp::<f32>(egui::Id::new(BELOW_ECONOMY))).unwrap_or(96.0);
        let area = egui::Area::new(egui::Id::new("minimap")).anchor(Align2::LEFT_TOP, Vec2::new(4.0, top)).show(ctx, |ui| {
            hud::framed(ui, Vec2::new(8.0, 8.0), |ui| {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(MINIMAP_SIZE), Sense::click_and_drag());
                let painter = ui.painter_at(rect);
                painter.image(land_id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                if let Some((fog, side)) = fog {
                    // The fog grid can reach past the map's edge.
                    let reach = (map_size / (side as f64 * FOG_CELL_UNITS)) as f32;
                    // Its first row is the map's near edge, at the bottom.
                    painter.image(fog, rect, Rect::from_min_max(Pos2::new(0.0, reach), Pos2::new(reach, 0.0)), Color32::WHITE);
                }
                let raw = |value: i32| f64::from(value) / 4096.0;
                // Every dot in one mesh: a battle has thousands, and shapes one by one cost
                // more to lay out than the rest of the interface.
                let mut dots = egui::epaint::Mesh::default();
                for blip in &snapshot.blips {
                    dots.add_colored_rect(Rect::from_center_size(to_minimap(rect, map_size, [raw(blip[0]), raw(blip[1])]), Vec2::splat(2.5)), Color32::from_gray(170));
                }
                for unit in &snapshot.units {
                    let at = to_minimap(rect, map_size, [raw(unit.x), raw(unit.y)]);
                    let color = match unit.team {
                        Team::Blue => Color32::from_rgb(90, 160, 255),
                        Team::Red => Color32::from_rgb(255, 95, 80),
                    };
                    if unit.is_structure {
                        // A dark rim, so that structures stand apart from units.
                        dots.add_colored_rect(Rect::from_center_size(at, Vec2::splat(4.6)), Color32::from_black_alpha(200));
                        dots.add_colored_rect(Rect::from_center_size(at, Vec2::splat(3.4)), color);
                    } else {
                        dots.add_colored_rect(Rect::from_center_size(at, Vec2::splat(2.2)), color);
                    }
                }
                painter.add(egui::Shape::mesh(dots));
                // The camera's view.
                let corners: Vec<Pos2> = view_corners(camera, heights).into_iter().map(|point| to_minimap(rect, map_size, point)).collect();
                if corners.len() >= 3 {
                    painter.add(egui::Shape::closed_line(corners, Stroke::new(1.4_f32, Color32::WHITE)));
                }
                painter.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, hud::RIM.gamma_multiply(0.7)));
                // Only the left button moves the view: the right gives orders.
                if response.is_pointer_button_down_on() && ui.input(|input| input.pointer.primary_down()) {
                    if let Some(spot) = response.interact_pointer_pos() {
                        let [x, y] = from_minimap(rect, map_size, spot);
                        actions.push(UiAction::CenterCamera([x.round() as i32, y.round() as i32]));
                    }
                }
                if response.secondary_clicked() {
                    if let Some(spot) = response.interact_pointer_pos() {
                        let [x, y] = from_minimap(rect, map_size, spot);
                        actions.push(UiAction::MinimapOrder([x.round() as i32, y.round() as i32]));
                    }
                }
                response.on_hover_text("Click or drag to move the view. Right-click to send the selection there: Shift queues, Alt attack-moves, Ctrl keeps formation.");
            });
        });
        ctx.data_mut(|data| data.insert_temp(egui::Id::new(BELOW_MINIMAP), area.response.rect.bottom() + 6.0));
    }
}

/// Remembers where the economy panel ends, so that the minimap sits under it.
pub fn note_economy_bottom(ctx: &egui::Context, bottom: f32) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(BELOW_ECONOMY), bottom + 6.0));
}

/// Where the panels under the minimap start, or under the economy if there is no minimap.
pub fn below_minimap(ctx: &egui::Context) -> Option<f32> {
    ctx.data(|data| data.get_temp::<f32>(egui::Id::new(BELOW_MINIMAP)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bowl() -> HeightField {
        // A 33-sample map, 20 units a cell, low in the middle and high at the edges, in steps of
        // a quarter of a unit.
        let samples = 33;
        let heights = (0..samples * samples)
            .map(|index| {
                let (x, y) = ((index % samples) as f64 - 16.0, (index / samples) as f64 - 16.0);
                ((x * x + y * y) * 0.6 - 40.0) as i16 * 4
            })
            .collect();
        HeightField::new(samples, heights, 20.0, 0.25)
    }

    #[test]
    fn the_land_picture_shows_water_in_the_lows_and_grass_above() {
        let heights = bowl();
        let side = 32;
        let colors = land_colors(&heights, 0.0, side);
        assert_eq!(colors.len(), side * side);
        let middle = colors[side / 2 * side + side / 2];
        let edge = colors[side / 2 * side + 1];
        assert!(middle.b() > middle.g() && middle.b() > middle.r(), "the bowl's bottom is under water: {middle:?}");
        assert!(edge.g() >= edge.b(), "its rim is land: {edge:?}");
    }

    #[test]
    fn minimap_spots_and_world_points_match_and_stay_on_the_map() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::splat(200.0));
        let size = 5_120.0;
        let point = [1_280.0, 3_840.0];
        let spot = to_minimap(rect, size, point);
        assert_eq!(spot, Pos2::new(60.0, 70.0), "north, the far edge, is up");
        let back = from_minimap(rect, size, spot);
        assert!((back[0] - point[0]).abs() < 1e-6 && (back[1] - point[1]).abs() < 1e-6);
        assert_eq!(from_minimap(rect, size, Pos2::new(-50.0, 500.0)), [0.0, 0.0], "a drag past the edge stops at it");
    }

    /// Clicks the middle of the minimap through real egui input, and returns what came out.
    fn click_middle(button: egui::PointerButton) -> Vec<UiAction> {
        let heights = bowl();
        let mut camera = Camera::new(640.0, [800.0, 600.0]);
        camera.look_at(320.0, 320.0, 300.0);
        camera.update(0.0, &heights);
        let snapshot = SimSnapshot::default();
        let ctx = egui::Context::default();
        let mut minimap = Minimap::default();
        // The panel sits at (4, 96), and the map starts inside its 8 point margin.
        let at = Pos2::new(12.0 + MINIMAP_SIZE / 2.0, 104.0 + MINIMAP_SIZE / 2.0);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0));
        let press = |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE };
        let mut actions = Vec::new();
        for events in [vec![], vec![egui::Event::PointerMoved(at)], vec![press(true)], vec![press(false)], vec![]] {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..egui::RawInput::default() };
            let _ = ctx.run(input, |ctx| minimap.show(ctx, &camera, &heights, 0.0, &snapshot, None, &mut actions));
        }
        actions
    }

    #[test]
    fn clicking_the_minimap_moves_the_view_and_right_clicking_sends_the_selection() {
        let left = click_middle(egui::PointerButton::Primary);
        assert!(!left.is_empty() && left.iter().all(|action| *action == UiAction::CenterCamera([320, 320])), "{left:?}");
        assert_eq!(click_middle(egui::PointerButton::Secondary), [UiAction::MinimapOrder([320, 320])]);
    }

    #[test]
    fn the_view_outline_has_a_corner_for_each_corner_of_the_screen() {
        let heights = bowl();
        let mut camera = Camera::new(640.0, [800.0, 600.0]);
        camera.look_at(320.0, 320.0, 300.0);
        camera.update(0.0, &heights);
        let corners = view_corners(&camera, &heights);
        assert_eq!(corners.len(), 4);
        // Looking along +Y, the top of the screen is farther up the map than the bottom.
        assert!(corners[3][1] > corners[0][1] && corners[1][0] > corners[0][0]);
    }
}
