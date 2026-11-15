//! The look of the in-game interface: framed panels with chamfered corners, square buttons, and
//! the vector glyphs drawn on order buttons. `ui` lays the panels out; this only paints.
#![allow(clippy::disallowed_types)]

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::epaint::{Mesh, PathShape};
use egui::{Align2, Color32, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};

/// The dark fill behind every panel.
pub const PANEL_FILL: Color32 = Color32::from_rgba_premultiplied(12, 15, 20, 236);
/// The panel's lighter top, which fades into the fill.
const PANEL_TOP: Color32 = Color32::from_rgba_premultiplied(30, 37, 46, 240);
/// The steel rim around panels.How
pub const RIM: Color32 = Color32::from_rgb(118, 134, 150);
/// The thin glowing line inside the rim, and what hovered or chosen things light up with.
pub const GLOW: Color32 = Color32::from_rgb(96, 206, 255);
/// Ordinary text on panels.
pub const TEXT: Color32 = Color32::from_rgb(214, 224, 234);
/// Text that matters less: labels, capacities.
pub const DIM_TEXT: Color32 = Color32::from_rgb(140, 154, 168);
/// The clock and other figures that read as system status.
pub const CLOCK: Color32 = Color32::from_rgb(110, 210, 255);
pub const MASS: Color32 = Color32::from_rgb(132, 214, 92);
pub const ENERGY: Color32 = Color32::from_rgb(250, 186, 70);
pub const GAIN: Color32 = Color32::from_rgb(196, 234, 120);
pub const LOSS: Color32 = Color32::from_rgb(255, 92, 76);
/// Button faces, top and bottom of their gradient.
const FACE_TOP: Color32 = Color32::from_rgb(40, 49, 60);
const FACE_BOTTOM: Color32 = Color32::from_rgb(17, 21, 27);
const FACE_EDGE: Color32 = Color32::from_rgb(70, 84, 98);

/// How much each corner of a panel is cut off, in points.
const CHAMFER: f32 = 9.0;

/// The outline of a rectangle with its corners cut off.
fn chamfered(rect: Rect, cut: f32) -> Vec<Pos2> {
    let cut = cut.min(rect.width() / 3.0).min(rect.height() / 3.0);
    let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    vec![
        Pos2::new(l + cut, t),
        Pos2::new(r - cut, t),
        Pos2::new(r, t + cut),
        Pos2::new(r, b - cut),
        Pos2::new(r - cut, b),
        Pos2::new(l + cut, b),
        Pos2::new(l, b - cut),
        Pos2::new(l, t + cut),
    ]
}

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

/// A panel's frame: the dark gradient body, the steel rim, a glowing line just inside it, and
/// small lights at the cut corners.
pub fn panel_shapes(rect: Rect) -> Vec<Shape> {
    let outline = chamfered(rect, CHAMFER);
    let inner = chamfered(rect.shrink(3.0), CHAMFER - 1.5);
    let mut shapes = vec![
        // A soft shadow, so that panels lift off busy terrain.
        Shape::convex_polygon(chamfered(rect.expand(2.0).translate(Vec2::new(0.0, 2.0)), CHAMFER + 1.0), Color32::from_black_alpha(90), Stroke::NONE),
        gradient_polygon(&outline, PANEL_TOP, PANEL_FILL),
        Shape::Path(PathShape::closed_line(outline.clone(), Stroke::new(2.0_f32, RIM))),
        Shape::Path(PathShape::closed_line(inner, Stroke::new(1.0_f32, GLOW.gamma_multiply(0.35)))),
    ];
    // A short bright stroke along each cut corner.
    for index in (0..outline.len()).step_by(2) {
        let (from, to) = (outline[(index + outline.len() - 1) % outline.len()], outline[index]);
        shapes.push(Shape::line_segment([from, to], Stroke::new(2.5_f32, GLOW.gamma_multiply(0.85))));
    }
    shapes
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

/// The face of a square button: a gradient, lit along its rim when hovered and filled with the
/// glow when chosen. Disabled buttons are dimmed.
pub fn face_shapes(rect: Rect, hovered: bool, chosen: bool, enabled: bool) -> Vec<Shape> {
    let outline = chamfered(rect, 4.0);
    let (top, bottom) = if chosen {
        (Color32::from_rgb(40, 104, 140), Color32::from_rgb(16, 46, 66))
    } else if hovered && enabled {
        (Color32::from_rgb(52, 66, 82), Color32::from_rgb(22, 28, 36))
    } else {
        (FACE_TOP, FACE_BOTTOM)
    };
    let rim = if chosen {
        GLOW
    } else if hovered && enabled {
        GLOW.gamma_multiply(0.8)
    } else {
        FACE_EDGE
    };
    let mut shapes = vec![gradient_polygon(&outline, top, bottom), Shape::Path(PathShape::closed_line(outline, Stroke::new(if chosen { 1.8_f32 } else { 1.2 }, rim)))];
    if !enabled {
        shapes.push(Shape::convex_polygon(chamfered(rect, 4.0), Color32::from_black_alpha(120), Stroke::NONE));
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

/// A thin bar that fills from the left, with a dark track.
pub fn meter(painter: &egui::Painter, rect: Rect, share: f32, color: Color32) {
    painter.rect_filled(rect, 1.5, Color32::from_rgb(8, 10, 13));
    let filled = Rect::from_min_size(rect.min, Vec2::new(rect.width() * share.clamp(0.0, 1.0), rect.height()));
    if filled.width() > 0.5 {
        painter.add(gradient_polygon(&[filled.left_top(), filled.right_top(), filled.right_bottom(), filled.left_bottom()], lerp_color(color, Color32::WHITE, 0.35), color.gamma_multiply(0.75)));
    }
    painter.rect_stroke(rect, 1.5, Stroke::new(1.0_f32, Color32::from_rgb(52, 62, 74)));
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
    Overcharge,
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
        Glyph::Overcharge => {
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
    visuals.window_fill = Color32::from_rgba_premultiplied(14, 18, 24, 244);
    visuals.panel_fill = Color32::from_rgba_premultiplied(14, 18, 24, 236);
    visuals.window_stroke = Stroke::new(1.5_f32, RIM);
    visuals.window_rounding = egui::Rounding::same(3.0);
    visuals.menu_rounding = egui::Rounding::same(3.0);
    visuals.extreme_bg_color = Color32::from_rgb(8, 10, 13);
    visuals.selection.bg_fill = Color32::from_rgb(32, 92, 128);
    visuals.selection.stroke = Stroke::new(1.0_f32, GLOW);
    visuals.hyperlink_color = CLOCK;
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(50, 60, 72));
    widgets.inactive.weak_bg_fill = Color32::from_rgb(30, 37, 46);
    widgets.inactive.bg_fill = Color32::from_rgb(30, 37, 46);
    widgets.inactive.bg_stroke = Stroke::new(1.0_f32, FACE_EDGE);
    widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.hovered.weak_bg_fill = Color32::from_rgb(42, 58, 74);
    widgets.hovered.bg_fill = Color32::from_rgb(42, 58, 74);
    widgets.hovered.bg_stroke = Stroke::new(1.2_f32, GLOW.gamma_multiply(0.8));
    widgets.active.weak_bg_fill = Color32::from_rgb(32, 92, 128);
    widgets.active.bg_fill = Color32::from_rgb(32, 92, 128);
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
    let decoder = png::Decoder::new(PORTRAIT_SHEET);
    let Ok(mut reader) = decoder.read_info() else {
        return;
    };
    let mut pixels = vec![0; reader.output_buffer_size()];
    let Ok(frame) = reader.next_frame(&mut pixels) else {
        return;
    };
    let size = [frame.width as usize, frame.height as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels[..frame.buffer_size()]);
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
    fn a_panel_is_framed_on_all_sides_with_its_corners_cut() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(200.0, 60.0));
        let outline = chamfered(rect, CHAMFER);
        assert_eq!(outline.len(), 8);
        assert!(outline.iter().all(|point| rect.expand(0.01).contains(*point)));
        assert!(!outline.contains(&rect.left_top()), "the corners are cut");
    }
}
