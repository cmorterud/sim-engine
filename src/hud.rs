//! The look of the in-game interface: plain square panels with a steel rim and a brass line along
//! their top, square buttons, and
//! the vector glyphs drawn on order buttons. `ui` lays the panels out; this only paints.
#![allow(clippy::disallowed_types)]

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::epaint::{Mesh, PathShape};
use egui::{Align2, Color32, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};

/// The dark fill behind every panel.
pub const PANEL_FILL: Color32 = Color32::from_rgba_premultiplied(19, 22, 20, 238);
/// The steel rim around panels.
pub const RIM: Color32 = Color32::from_rgb(95, 101, 93);
/// The brass line along the top of every panel, and what hovered or chosen things light up with.
pub const GLOW: Color32 = Color32::from_rgb(209, 165, 78);
/// Ordinary text on panels.
pub const TEXT: Color32 = Color32::from_rgb(220, 223, 214);
/// Text that matters less: labels, capacities.
pub const DIM_TEXT: Color32 = Color32::from_rgb(157, 162, 150);
/// The clock and other figures that read as system status.
pub const CLOCK: Color32 = Color32::from_rgb(220, 223, 214);
pub const MASS: Color32 = Color32::from_rgb(132, 214, 92);
pub const ENERGY: Color32 = Color32::from_rgb(250, 186, 70);
pub const GAIN: Color32 = Color32::from_rgb(196, 234, 120);
pub const LOSS: Color32 = Color32::from_rgb(255, 92, 76);
/// Button faces, plain, and their edge.
const FACE: Color32 = Color32::from_rgb(34, 39, 34);
const FACE_EDGE: Color32 = Color32::from_rgb(60, 66, 59);

/// A convex polygon filled with a vertical gradient, `top` at its top edge and `bottom` at its
/// bottom.
fn gradient_polygon(points: &[Pos2], top: Color32, bottom: Color32) -> Shape {
    let (min_y, max_y) = points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), point| (lo.min(point.y), hi.max(point.y)));
    let span = (max_y - min_y).max(1.0);
    let mut mesh = Mesh::default();
    let center = points.iter().fold(Vec2::ZERO, |sum, point| sum + point.to_vec2()) / points.len() as f32;
    let color_at = |y: f32| lerp_color(top, bottom, (y - min_y) / span);
    mesh.colored_vertex(center.to_pos2(), color_at(center.y));
    for point in points {
        mesh.colored_vertex(*point, color_at(point.y));
    }
    let count = points.len() as u32;
    for index in 0..count {
        mesh.add_triangle(0, 1 + index, 1 + (index + 1) % count);
    }
    Shape::mesh(mesh)
}

pub fn lerp_color(from: Color32, to: Color32, share: f32) -> Color32 {
    let share = share.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * share).round() as u8;
    Color32::from_rgba_premultiplied(mix(from.r(), to.r()), mix(from.g(), to.g()), mix(from.b(), to.b()), mix(from.a(), to.a()))
}

/// How thick the brass line along a panel's top is, in points.
const TOP_LINE: f32 = 2.0;

/// A panel's frame: a soft shadow, the dark body with square corners, a steel rim, and a brass
/// line along the top.
pub fn panel_shapes(rect: Rect) -> Vec<Shape> {
    let top = Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), rect.top() + TOP_LINE));
    vec![
        // A soft shadow, so that panels lift off busy terrain.
        Shape::rect_filled(rect.expand(1.0).translate(Vec2::new(0.0, 2.0)), 3.0, Color32::from_black_alpha(80)),
        Shape::rect_filled(rect, 2.0, PANEL_FILL),
        Shape::rect_stroke(rect, 2.0, Stroke::new(1.0_f32, RIM)),
        Shape::rect_filled(top, egui::Rounding { nw: 2.0, ne: 2.0, sw: 0.0, se: 0.0 }, GLOW),
    ]
}

/// Paints a panel's frame behind whatever `add` lays out, with room around it.
pub fn framed<R>(ui: &mut Ui, margin: Vec2, add: impl FnOnce(&mut Ui) -> R) -> R {
    let slot = ui.painter().add(Shape::Noop);
    let inner = egui::Frame::none().inner_margin(egui::Margin::symmetric(margin.x, margin.y)).show(ui, add);
    ui.painter().set(slot, Shape::Vec(panel_shapes(inner.response.rect)));
    inner.inner
}

/// A framed panel floating at a corner or edge of the screen.
pub fn panel<R>(ctx: &egui::Context, id: &str, anchor: Align2, offset: Vec2, add: impl FnOnce(&mut Ui) -> R) -> R {
    panel_rect(ctx, id, anchor, offset, add).0
}

/// A framed panel, as `panel`, also returning where on the screen it is.
pub fn panel_rect<R>(ctx: &egui::Context, id: &str, anchor: Align2, offset: Vec2, add: impl FnOnce(&mut Ui) -> R) -> (R, Rect) {
    let shown = egui::Area::new(egui::Id::new(id)).anchor(anchor, offset).constrain(true).show(ctx, |ui| framed(ui, Vec2::new(11.0, 8.0), add));
    (shown.inner, shown.response.rect)
}

/// The face of a square button: flat, its edge lit brass when hovered, and tinted brass with a
/// brass edge when chosen. Disabled buttons are dimmed.
pub fn face_shapes(rect: Rect, hovered: bool, chosen: bool, enabled: bool) -> Vec<Shape> {
    let fill = if chosen {
        Color32::from_rgb(64, 54, 32)
    } else if hovered && enabled {
        Color32::from_rgb(44, 49, 43)
    } else {
        FACE
    };
    let edge = if chosen {
        GLOW
    } else if hovered && enabled {
        GLOW.gamma_multiply(0.75)
    } else {
        FACE_EDGE
    };
    let mut shapes = vec![Shape::rect_filled(rect, 2.0, fill), Shape::rect_stroke(rect, 2.0, Stroke::new(if chosen { 1.6_f32 } else { 1.0 }, edge))];
    if !enabled {
        shapes.push(Shape::rect_filled(rect, 2.0, Color32::from_black_alpha(120)));
    }
    shapes
}

/// A square button. `paint` draws what is on it, inside its rim. Clicks with either button
/// count, so callers can tell them apart.
pub fn square_button(ui: &mut Ui, size: Vec2, chosen: bool, enabled: bool, paint: impl FnOnce(&egui::Painter, Rect)) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        for shape in face_shapes(rect, response.hovered(), chosen, enabled) {
            painter.add(shape);
        }
        paint(painter, rect.shrink(2.0));
        if !enabled {
            painter.rect_filled(rect.shrink(1.0), 3.0, Color32::from_black_alpha(110));
        }
    }
    response
}

/// Text with a dark outline, so it reads on any button face or icon.
pub fn outlined_text(painter: &egui::Painter, at: Pos2, align: Align2, text: &str, font: FontId, color: Color32) {
    for offset in [Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0), Vec2::new(1.0, -1.0), Vec2::new(-1.0, -1.0)] {
        painter.text(at + offset, align, text, font.clone(), Color32::from_black_alpha(220));
    }
    painter.text(at, align, text, font, color);
}

/// A rectangle filled with `fill` and crossed by diagonal `stripe`s, leaning right.
pub fn hatched(painter: &egui::Painter, rect: Rect, fill: Color32, stripe: Color32) {
    painter.rect_filled(rect, 0.0, fill);
    let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let spacing = 5.0;
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        clipped.line_segment([Pos2::new(x, rect.bottom()), Pos2::new(x + rect.height(), rect.top())], Stroke::new(1.6_f32, stripe));
        x += spacing;
    }
}

/// A thin bar that fills from the left, with a dark track.
pub fn meter(painter: &egui::Painter, rect: Rect, share: f32, color: Color32) {
    painter.rect_filled(rect, 1.5, Color32::from_rgb(9, 11, 9));
    let filled = Rect::from_min_size(rect.min, Vec2::new(rect.width() * share.clamp(0.0, 1.0), rect.height()));
    if filled.width() > 0.5 {
        painter.add(gradient_polygon(&[filled.left_top(), filled.right_top(), filled.right_bottom(), filled.left_bottom()], lerp_color(color, Color32::WHITE, 0.35), color.gamma_multiply(0.75)));
    }
    painter.rect_stroke(rect, 1.5, Stroke::new(1.0_f32, Color32::from_rgb(52, 58, 51)));
}

/// What an order or toggle button shows.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Glyph {
    Move,
    AttackMove,
    Formation,
    AttackGround,
    Patrol,
    Assist,
    Discharge,
    Stop,
    Capture,
    Teleport,
    Unload,
    Pause,
    Resume,
    Cloak,
    Upgrade,
    Repeat,
    ClearQueue,
    Reclaim,
    Repair,
    Sound,
    /// Take a submarine down (`Dive`) or bring it up (`Surface`).
    Dive,
    Surface,
    /// Which ranges to outline on the map.
    Ranges,
    /// Fire missiles on order.
    Launch,
}

/// Draws a glyph inside `rect`. Each order has its own shape as well as its own color, so that
/// they can be told apart without relying on color.
pub fn draw_glyph(painter: &egui::Painter, rect: Rect, glyph: Glyph) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.34;
    let line = |color: Color32| Stroke::new((r * 0.16).max(1.6), color);
    let poly = |points: Vec<Pos2>, color: Color32| Shape::Path(PathShape::closed_line(points, line(color)));
    let at = |x: f32, y: f32| c + Vec2::new(x * r, y * r);
    let green = Color32::from_rgb(110, 236, 140);
    let red = Color32::from_rgb(255, 86, 72);
    let blue = Color32::from_rgb(100, 180, 255);
    let yellow = Color32::from_rgb(255, 214, 84);
    let violet = Color32::from_rgb(200, 132, 255);
    let teal = Color32::from_rgb(110, 236, 214);
    match glyph {
        Glyph::Move => {
            // Four arrowheads pointing out from the middle.
            for direction in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                let side = direction.rot90();
                let tip = c + direction * r;
                painter.line_segment([c + direction * r * 0.25, tip], line(green));
                painter.add(Shape::convex_polygon(vec![tip + direction * r * 0.12, tip - direction * r * 0.32 + side * r * 0.3, tip - direction * r * 0.32 - side * r * 0.3], green, Stroke::NONE));
            }
        }
        Glyph::AttackMove => {
            painter.circle_stroke(c, r * 0.8, line(red));
            for direction in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                painter.line_segment([c + direction * r * 1.05, c + direction * r * 0.4], line(red));
            }
        }
        Glyph::Formation => {
            for (x, y) in [(0.0, -0.55), (-0.6, 0.35), (0.6, 0.35)] {
                let tip = at(x, y - 0.3);
                painter.add(Shape::line(vec![tip + Vec2::new(-r * 0.32, r * 0.32), tip, tip + Vec2::new(r * 0.32, r * 0.32)], line(blue)));
            }
            painter.circle_stroke(c, r * 1.05, Stroke::new(1.0_f32, blue.gamma_multiply(0.5)));
        }
        Glyph::AttackGround => {
            painter.circle_stroke(at(0.0, -0.15), r * 0.65, line(red));
            painter.circle_filled(at(0.0, -0.15), r * 0.18, red);
            painter.line_segment([at(-1.0, 0.85), at(1.0, 0.85)], line(Color32::from_rgb(200, 150, 110)));
        }
        Glyph::Patrol => {
            painter.add(poly(vec![at(0.0, -0.95), at(0.95, 0.0), at(0.0, 0.95), at(-0.95, 0.0)], yellow));
            painter.circle_filled(at(0.0, -0.95), r * 0.2, yellow);
            painter.circle_filled(at(0.0, 0.95), r * 0.2, yellow);
        }
        Glyph::Assist => {
            painter.add(poly(vec![at(-0.8, -0.8), at(0.8, -0.8), at(0.8, 0.1), at(0.0, 0.95), at(-0.8, 0.1)], blue));
            painter.line_segment([at(0.0, -0.45), at(0.0, 0.35)], line(blue));
            painter.line_segment([at(-0.4, -0.05), at(0.4, -0.05)], line(blue));
        }
        Glyph::Discharge => {
            painter.add(Shape::convex_polygon(vec![at(0.2, -1.0), at(-0.55, 0.15), at(0.0, 0.15), at(-0.2, 1.0)], yellow, Stroke::NONE));
            painter.add(Shape::convex_polygon(vec![at(0.2, -1.0), at(0.55, -0.15), at(0.0, -0.15)], yellow, Stroke::NONE));
            painter.add(Shape::convex_polygon(vec![at(0.0, -0.15), at(0.55, -0.15), at(-0.2, 1.0)], yellow, Stroke::NONE));
        }
        Glyph::Stop => {
            let points: Vec<Pos2> = (0..8).map(|index| {
                let angle = (index as f32 + 0.5) / 8.0 * std::f32::consts::TAU;
                c + Vec2::angled(angle) * r
            }).collect();
            painter.add(Shape::convex_polygon(points, Color32::from_rgb(190, 46, 40), Stroke::new(1.5_f32, Color32::from_rgb(255, 150, 140))));
            painter.line_segment([at(-0.45, 0.0), at(0.45, 0.0)], Stroke::new(r * 0.22, Color32::WHITE));
        }
        Glyph::Capture => {
            painter.line_segment([at(-0.6, 1.0), at(-0.6, -1.0)], line(teal));
            painter.add(Shape::convex_polygon(vec![at(-0.6, -1.0), at(0.8, -0.6), at(-0.6, -0.15)], teal, Stroke::NONE));
        }
        Glyph::Teleport => {
            painter.circle_stroke(c, r * 0.95, line(violet));
            painter.circle_stroke(c, r * 0.45, line(violet));
            painter.circle_filled(c, r * 0.15, violet);
        }
        Glyph::Unload => {
            painter.line_segment([at(0.0, -1.0), at(0.0, 0.3)], line(teal));
            painter.add(Shape::line(vec![at(-0.45, -0.1), at(0.0, 0.35), at(0.45, -0.1)], line(teal)));
            painter.line_segment([at(-0.9, 0.85), at(0.9, 0.85)], line(teal));
        }
        Glyph::Pause => {
            painter.rect_filled(Rect::from_center_size(at(-0.38, 0.0), Vec2::new(r * 0.38, r * 1.6)), 1.0, yellow);
            painter.rect_filled(Rect::from_center_size(at(0.38, 0.0), Vec2::new(r * 0.38, r * 1.6)), 1.0, yellow);
        }
        Glyph::Resume => {
            painter.add(Shape::convex_polygon(vec![at(-0.55, -0.85), at(0.85, 0.0), at(-0.55, 0.85)], green, Stroke::NONE));
        }
        Glyph::Cloak => {
            let gray = Color32::from_rgb(190, 200, 214);
            painter.add(Shape::closed_line(vec![at(-1.0, 0.0), at(-0.5, -0.5), at(0.5, -0.5), at(1.0, 0.0), at(0.5, 0.5), at(-0.5, 0.5)], line(gray)));
            painter.circle_filled(c, r * 0.28, gray);
            painter.line_segment([at(-0.85, 0.85), at(0.85, -0.85)], line(gray));
        }
        Glyph::Upgrade => {
            for lift in [0.45, -0.25] {
                painter.add(Shape::line(vec![at(-0.7, lift + 0.4), at(0.0, lift - 0.3), at(0.7, lift + 0.4)], line(violet)));
            }
        }
        Glyph::Repeat => {
            let points: Vec<Pos2> = (0..=20).map(|index| c + Vec2::angled(-0.4 + index as f32 / 20.0 * 5.0) * r * 0.8).collect();
            let end = *points.last().expect("an arc has points");
            painter.add(Shape::line(points, line(teal)));
            painter.add(Shape::convex_polygon(vec![end + Vec2::new(-r * 0.4, -r * 0.1), end + Vec2::new(r * 0.25, -r * 0.35), end + Vec2::new(r * 0.05, r * 0.35)], teal, Stroke::NONE));
        }
        Glyph::ClearQueue => {
            painter.line_segment([at(-0.7, -0.7), at(0.7, 0.7)], line(red));
            painter.line_segment([at(-0.7, 0.7), at(0.7, -0.7)], line(red));
        }
        Glyph::Reclaim => {
            painter.circle_stroke(c, r * 0.7, line(green));
            painter.add(Shape::line(vec![at(-0.3, -0.15), at(0.0, 0.25), at(0.35, -0.45)], line(green)));
        }
        Glyph::Repair => {
            painter.line_segment([at(-0.7, 0.7), at(0.4, -0.4)], Stroke::new(r * 0.3, yellow));
            painter.circle_stroke(at(0.5, -0.5), r * 0.35, line(yellow));
        }
        Glyph::Dive | Glyph::Surface => {
            // A wave line, with an arrow going down through it or coming up.
            let wave: Vec<Pos2> = (0..=12).map(|index| {
                let x = -1.0 + index as f32 / 6.0;
                at(x, 0.15 + 0.12 * (x * std::f32::consts::PI * 2.0).sin())
            }).collect();
            painter.add(Shape::line(wave, line(teal)));
            let (from, to) = if glyph == Glyph::Dive { (-0.95, 0.85) } else { (0.85, -0.95) };
            painter.line_segment([at(0.0, from), at(0.0, to)], line(blue));
            let tip = at(0.0, to);
            let back = if glyph == Glyph::Dive { -0.35 } else { 0.35 };
            painter.add(Shape::line(vec![at(-0.35, to + back), tip, at(0.35, to + back)], line(blue)));
        }
        Glyph::Launch => {
            // A missile climbing to the upper right, on a plume of flame.
            let orange = Color32::from_rgb(255, 150, 60);
            let along = Vec2::new(1.0, -1.0).normalized();
            let side = along.rot90();
            let (tail, nose) = (c - along * r * 0.75, c + along * r * 0.7);
            let half = r * 0.2;
            painter.add(Shape::convex_polygon(vec![tail + side * half, nose + side * half, nose - side * half, tail - side * half], Color32::from_rgb(220, 226, 234), Stroke::NONE));
            painter.add(Shape::convex_polygon(vec![nose + side * half, nose + along * r * 0.4, nose - side * half], red, Stroke::NONE));
            for flank in [1.0, -1.0] {
                painter.add(Shape::convex_polygon(vec![tail + side * half * flank, tail + along * r * 0.35 + side * half * flank, tail - along * r * 0.1 + side * r * 0.5 * flank], red, Stroke::NONE));
            }
            painter.add(Shape::convex_polygon(vec![tail + side * half * 0.8, tail - side * half * 0.8, tail - along * r * 0.55], orange, Stroke::NONE));
        }
        Glyph::Ranges => {
            // Two overlapping rings round their centers, as range outlines on the ground.
            painter.circle_stroke(at(-0.3, 0.1), r * 0.62, line(yellow));
            painter.circle_stroke(at(0.35, -0.1), r * 0.5, line(green));
            painter.circle_filled(at(-0.3, 0.1), r * 0.1, yellow);
            painter.circle_filled(at(0.35, -0.1), r * 0.1, green);
        }
        Glyph::Sound => {
            let gray = Color32::from_rgb(200, 210, 222);
            painter.add(Shape::convex_polygon(vec![at(-0.9, -0.3), at(-0.4, -0.3), at(0.15, -0.85), at(0.15, 0.85), at(-0.4, 0.3), at(-0.9, 0.3)], gray, Stroke::NONE));
            for reach in [0.5, 0.85] {
                let arc: Vec<Pos2> = (0..=8).map(|index| at(0.15, 0.0) + Vec2::angled(-0.8 + index as f32 / 8.0 * 1.6) * r * reach).collect();
                painter.add(Shape::line(arc, Stroke::new(1.4_f32, gray)));
            }
        }
    }
}

/// The resource's mark: a disc in its color with a symbol, mass a diamond and energy a bolt.
pub fn resource_mark(painter: &egui::Painter, center: Pos2, radius: f32, energy: bool) {
    let color = if energy { ENERGY } else { MASS };
    painter.circle(center, radius, Color32::from_rgb(14, 18, 22), Stroke::new(1.6_f32, color));
    let r = radius * 0.55;
    if energy {
        painter.add(Shape::convex_polygon(vec![center + Vec2::new(0.15 * r, -r), center + Vec2::new(-0.55 * r, 0.15 * r), center + Vec2::new(0.55 * r, -0.15 * r)], color, Stroke::NONE));
        painter.add(Shape::convex_polygon(vec![center + Vec2::new(-0.55 * r, 0.15 * r), center + Vec2::new(0.55 * r, -0.15 * r), center + Vec2::new(-0.15 * r, r)], color, Stroke::NONE));
    } else {
        painter.add(Shape::convex_polygon(vec![center + Vec2::new(0.0, -r), center + Vec2::new(r, 0.0), center + Vec2::new(0.0, r), center + Vec2::new(-r, 0.0)], color, Stroke::NONE));
    }
}

/// Signs a whole number: "+43", "-7", "+0".
pub fn signed(value: i64) -> String {
    if value < 0 { value.to_string() } else { format!("+{value}") }
}

/// Shortens big figures the way score boards do: 950, 9.5k, 95k, 1.2M.
pub fn compact(value: i64) -> String {
    let magnitude = value.unsigned_abs();
    let sign = if value < 0 { "-" } else { "" };
    match magnitude {
        0..=9_999 => value.to_string(),
        10_000..=99_999 => format!("{sign}{:.1}k", magnitude as f64 / 1000.0),
        100_000..=999_999 => format!("{sign}{}k", magnitude / 1000),
        _ => format!("{sign}{:.1}M", magnitude as f64 / 1_000_000.0),
    }
}

/// The interface's colors for egui's own widgets: tooltips, windows, sliders and checkboxes, so
/// that they match the panels.
pub fn apply_visuals(style: &mut egui::Style) {
    let visuals = &mut style.visuals;
    *visuals = egui::Visuals::dark();
    visuals.override_text_color = None;
    visuals.window_fill = Color32::from_rgba_premultiplied(19, 22, 20, 244);
    visuals.panel_fill = Color32::from_rgba_premultiplied(19, 22, 20, 236);
    visuals.window_stroke = Stroke::new(1.5_f32, RIM);
    visuals.window_rounding = egui::Rounding::same(3.0);
    visuals.menu_rounding = egui::Rounding::same(3.0);
    visuals.extreme_bg_color = Color32::from_rgb(9, 11, 9);
    visuals.selection.bg_fill = Color32::from_rgb(86, 70, 36);
    visuals.selection.stroke = Stroke::new(1.0_f32, GLOW);
    visuals.hyperlink_color = CLOCK;
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(46, 52, 45));
    widgets.inactive.weak_bg_fill = Color32::from_rgb(34, 39, 34);
    widgets.inactive.bg_fill = Color32::from_rgb(34, 39, 34);
    widgets.inactive.bg_stroke = Stroke::new(1.0_f32, FACE_EDGE);
    widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.hovered.weak_bg_fill = Color32::from_rgb(44, 49, 43);
    widgets.hovered.bg_fill = Color32::from_rgb(44, 49, 43);
    widgets.hovered.bg_stroke = Stroke::new(1.2_f32, GLOW.gamma_multiply(0.8));
    widgets.active.weak_bg_fill = Color32::from_rgb(64, 54, 32);
    widgets.active.bg_fill = Color32::from_rgb(64, 54, 32);
    widgets.active.bg_stroke = Stroke::new(1.5_f32, GLOW);
    for state in [&mut widgets.noninteractive, &mut widgets.inactive, &mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
        state.rounding = egui::Rounding::same(2.0);
    }
}

/// Every unit's portrait, rendered by `examples/portrait_sheet.rs`, and which tile is whose.
const PORTRAIT_SHEET: &[u8] = include_bytes!("../assets/portraits.png");
const PORTRAIT_INDEX: &str = include_str!("../assets/portraits.txt");
const PORTRAITS: &str = "unit portraits";

/// The portraits, once loaded: the sheet, and where on it each unit's tile is.
#[derive(Clone)]
struct Portraits {
    texture: egui::TextureHandle,
    tiles: BTreeMap<String, Rect>,
}

/// The portrait index: the tile size, the tiles a row, and the units' keys, tile by tile.
pub fn portrait_index() -> (usize, usize, Vec<&'static str>) {
    let mut lines = PORTRAIT_INDEX.lines();
    let mut header = lines.next().unwrap_or("").split_whitespace().map(|value| value.parse::<usize>().unwrap_or(1));
    let (tile, columns) = (header.next().unwrap_or(1), header.next().unwrap_or(1));
    (tile, columns, lines.filter(|line| !line.is_empty()).collect())
}

/// Loads the portraits into the interface. Call once, before the first frame; without it,
/// buttons show the units' symbols instead.
pub fn install_portraits(ctx: &egui::Context) {
    let Some((size, pixels)) = portrait_pixels() else {
        return;
    };
    let image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
    let texture = ctx.load_texture(PORTRAITS, image, egui::TextureOptions::LINEAR);
    let (tile, columns, keys) = portrait_index();
    let (width, height) = (size[0] as f32, size[1] as f32);
    let tiles = keys
        .into_iter()
        .enumerate()
        .map(|(index, key)| {
            let corner = Pos2::new(((index % columns) * tile) as f32 / width, ((index / columns) * tile) as f32 / height);
            (key.to_string(), Rect::from_min_size(corner, Vec2::new(tile as f32 / width, tile as f32 / height)))
        })
        .collect();
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(PORTRAITS), Arc::new(Portraits { texture, tiles })));
}

/// Draws a unit's portrait filling `rect`, if the portraits are loaded and it has one. Returns
/// whether it did.
pub fn draw_portrait(painter: &egui::Painter, rect: Rect, key: &str) -> bool {
    let portraits = painter.ctx().data(|data| data.get_temp::<Arc<Portraits>>(egui::Id::new(PORTRAITS)));
    let Some(portraits) = portraits else {
        return false;
    };
    let Some(uv) = portraits.tiles.get(key) else {
        return false;
    };
    painter.image(portraits.texture.id(), rect, *uv, Color32::WHITE);
    true
}

/// The portrait sheet's size, and its pixels as unmultiplied RGBA bytes, row by row.
fn portrait_pixels() -> Option<([usize; 2], Vec<u8>)> {
    let mut reader = png::Decoder::new(PORTRAIT_SHEET).read_info().ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).ok()?;
    pixels.truncate(frame.buffer_size());
    Some(([frame.width as usize, frame.height as usize], pixels))
}

/// The side, in pixels, of the small picture a portrait is shrunk to for comparing it with the
/// others: about as much of it as can be made out at a glance on a bar button.
const GLANCE: usize = 24;
/// What a portrait is laid over before it is shrunk: a plain grey, so that only the unit counts.
const GLANCE_GROUND: f32 = 0.4;

/// How opaque a portrait's pixel must be to count as the unit in its outline: more than its
/// shadow, which `examples/portrait_sheet.rs` keeps faint, ever is.
const OUTLINE_ALPHA: u8 = 128;

/// A portrait shrunk to `GLANCE` pixels a side, for comparing it with the others.
pub struct Glance {
    /// Whose it is.
    pub key: &'static str,
    /// Its colors over `GLANCE_GROUND`, from 0 to 1.
    pub colors: Vec<[f32; 3]>,
    /// How much of each pixel the unit covers, from 0 to 1, its shadow left out.
    pub outline: Vec<f32>,
}

/// What two portraits are compared by.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Likeness {
    /// The whole picture, colors and all.
    Picture,
    /// The outline alone, whatever the colors: how far apart the units are by shape.
    Outline,
}

impl Glance {
    /// How unlike two portraits are, from 0 for the same to 1 for opposites everywhere: by
    /// `Picture`, the mean distance between their colors, pixel by pixel, where black against
    /// white is 1; by `Outline`, the mean difference in how much of each pixel they cover.
    /// Being means over the whole picture, both make little of thin parts, such as booms and
    /// masts, which the eye picks out more readily. Structures seen from the camera's height
    /// mostly fill their footprint, so for them the outline says little and the picture more.
    pub fn distance(&self, other: &Self, by: Likeness) -> f32 {
        match by {
            Likeness::Picture => {
                let total: f32 = self.colors.iter().zip(&other.colors).map(|(a, b)| (0..3).map(|channel| (a[channel] - b[channel]).powi(2)).sum::<f32>().sqrt()).sum();
                total / (self.colors.len().max(1) as f32 * 3.0_f32.sqrt())
            }
            Likeness::Outline => {
                let total: f32 = self.outline.iter().zip(&other.outline).map(|(a, b)| (a - b).abs()).sum();
                total / self.outline.len().max(1) as f32
            }
        }
    }
}

/// Every portrait shrunk to `GLANCE` pixels a side.
pub fn portrait_glances() -> Vec<Glance> {
    let Some((size, pixels)) = portrait_pixels() else {
        return Vec::new();
    };
    let (tile, columns, keys) = portrait_index();
    let block = tile / GLANCE;
    let area = (block * block) as f32;
    keys.into_iter()
        .enumerate()
        .map(|(index, key)| {
            let (left, top) = ((index % columns) * tile, (index / columns) * tile);
            let (mut colors, mut outline) = (Vec::with_capacity(GLANCE * GLANCE), Vec::with_capacity(GLANCE * GLANCE));
            for cell in 0..GLANCE * GLANCE {
                let (x, y) = (left + (cell % GLANCE) * block, top + (cell / GLANCE) * block);
                let (mut sum, mut covered) = ([0.0_f32; 3], 0.0_f32);
                for dy in 0..block {
                    for dx in 0..block {
                        let at = ((y + dy) * size[0] + x + dx) * 4;
                        let alpha = f32::from(pixels[at + 3]) / 255.0;
                        for channel in 0..3 {
                            sum[channel] += f32::from(pixels[at + channel]) / 255.0 * alpha + GLANCE_GROUND * (1.0 - alpha);
                        }
                        if pixels[at + 3] >= OUTLINE_ALPHA {
                            covered += 1.0;
                        }
                    }
                }
                colors.push(sum.map(|total| total / area));
                outline.push(covered / area);
            }
            Glance { key, colors, outline }
        })
        .collect()
}

/// Every pair of portraits that could be side by side on a build bar, most alike first by `by`,
/// with how unlike they are (`Glance::distance`). Only units on the same backdrop are compared,
/// since the backdrop already tells land from air from water, and structures only with
/// structures, since engineers build those and factories the rest.
pub fn most_alike_portraits(db: &crate::sim::blueprints::BlueprintDb, by: Likeness) -> Vec<(f32, &'static str, &'static str)> {
    let glances: Vec<_> = portrait_glances()
        .into_iter()
        .filter_map(|glance| db.find(glance.key).map(|id| ((crate::icons::Ground::of(db.get(id)), db.get(id).is_structure()), glance)))
        .collect();
    let mut pairs = Vec::new();
    for (index, (bar, glance)) in glances.iter().enumerate() {
        for (other_bar, other) in &glances[index + 1..] {
            if bar == other_bar {
                pairs.push((glance.distance(other, by), glance.key, other.key));
            }
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn figures_are_signed_and_shortened_like_a_score_board() {
        assert_eq!(signed(43), "+43");
        assert_eq!(signed(0), "+0");
        assert_eq!(signed(-7), "-7");
        assert_eq!(compact(950), "950");
        assert_eq!(compact(9_633), "9633");
        assert_eq!(compact(47_569), "47.6k");
        assert_eq!(compact(669_000), "669k");
        assert_eq!(compact(-1_250_000), "-1.2M");
    }

    /// The least `Glance::distance` allowed between two portraits that could share a build bar,
    /// by picture and by outline, a little under the most alike pairs on 2026-11-23: the T2 and
    /// T3 support docks by picture (0.0203), and the T1 and T2 support air factories by outline
    /// (0.0075), once the transports and engineers were given shapes of their own.
    /// Raise them as the most alike pairs, which `cargo run --release --example
    /// portrait_likeness` lists, are given shapes of their own.
    const PICTURE_LIKENESS_FLOOR: f32 = 0.018;
    const OUTLINE_LIKENESS_FLOOR: f32 = 0.0065;

    #[test]
    fn no_two_portraits_that_share_a_build_bar_are_nearly_the_same_picture() {
        let db = crate::sim::blueprints::BlueprintDb::standard();
        for (by, floor) in [(Likeness::Picture, PICTURE_LIKENESS_FLOOR), (Likeness::Outline, OUTLINE_LIKENESS_FLOOR)] {
            let pairs = most_alike_portraits(&db, by);
            assert!(pairs.len() > 500, "most units are compared: only {} pairs", pairs.len());
            let (distance, first, second) = pairs[0];
            assert!(
                distance >= floor,
                "{first} and {second} are too alike by {by:?} to tell apart on a button ({distance:.4}): give one a shape of its own, then see `cargo run --release --example portrait_likeness`"
            );
        }
    }

    #[test]
    fn the_likeness_measure_finds_two_variants_of_one_build_more_alike_than_two_units() {
        let glances: BTreeMap<&str, Glance> = portrait_glances().into_iter().map(|glance| (glance.key, glance)).collect();
        for by in [Likeness::Picture, Likeness::Outline] {
            let distance = |first: &str, second: &str| glances[first].distance(&glances[second], by);
            assert_eq!(distance("t1_medium_tank", "t1_medium_tank"), 0.0);
            // Two built on the same piers, under different booms.
            let same_model = distance("naval_factory_t2_support", "naval_factory_t3_support");
            let different = distance("t1_medium_tank", "t3_heavy_walker");
            assert!(same_model * 2.0 < different, "by {by:?}, two support docks ({same_model:.4}) are far more alike than a tank and a walker ({different:.4})");
            assert!(different < 1.0);
        }
    }

    #[test]
    fn the_portrait_sheet_has_a_tile_for_every_unit_and_structure() {
        let db = crate::sim::blueprints::BlueprintDb::standard();
        let (tile, columns, keys) = portrait_index();
        assert_eq!((tile, columns), (96, 12));
        for unit in db.iter().filter(|unit| unit.class != crate::sim::blueprints::UnitClass::Ammo) {
            assert!(keys.contains(&unit.key.as_str()), "{} has no portrait: run `cargo run --release --example portrait_sheet`", unit.key);
        }
        let decoder = png::Decoder::new(PORTRAIT_SHEET);
        let reader = decoder.read_info().expect("the sheet is a PNG");
        let info = reader.info();
        assert_eq!(info.width as usize, columns * tile);
        assert!(info.height as usize >= keys.len().div_ceil(columns) * tile, "every tile fits on the sheet");
    }

    #[test]
    fn a_panel_is_square_with_a_rim_and_a_brass_line_along_its_top() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(200.0, 60.0));
        let shapes = panel_shapes(rect);
        let rims = shapes.iter().filter(|shape| matches!(shape, Shape::Rect(rect_shape) if rect_shape.rect == rect && rect_shape.stroke.color == RIM)).count();
        assert_eq!(rims, 1, "one steel rim round the whole panel");
        let top = shapes.iter().find_map(|shape| match shape {
            Shape::Rect(rect_shape) if rect_shape.fill == GLOW => Some(rect_shape.rect),
            _ => None,
        });
        let top = top.expect("a brass line");
        assert_eq!((top.left(), top.right(), top.top()), (rect.left(), rect.right(), rect.top()), "it runs along the whole top edge");
        assert!(top.height() <= 3.0, "and is thin");
    }
}
