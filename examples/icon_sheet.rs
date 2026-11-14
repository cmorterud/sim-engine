//! Draws every icon as the build bar shows it, and as the map shows it, and saves the sheet as a
//! picture, for comparing icons before and after a change without starting the game.
//!
//! `cargo run --release --example icon_sheet -- <output.bmp> [points per pixel]`
//!
//! egui lays the sheet out and tessellates it as it would in the game, and a small software
//! rasterizer here fills the triangles, so no window or GPU is needed.
use egui::epaint::{ClippedPrimitive, Primitive};
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, TextureId, Vec2};
use sim_engine::icons::{self, Icon};

const COLUMNS: usize = 10;
const CELL: f32 = 62.0;
const BUTTON: f32 = 46.0;

fn main() {
    let mut args = std::env::args().skip(1);
    let output = args.next().unwrap_or_else(|| "icons.bmp".to_string());
    let scale: f32 = args.next().and_then(|value| value.parse().ok()).unwrap_or(2.0);
    let rows = Icon::ALL.len().div_ceil(COLUMNS);
    // Bar buttons on top, then the same icons as the map draws them.
    let size = Vec2::new(COLUMNS as f32 * CELL, rows as f32 * CELL * 2.0 + 20.0);
    let ctx = egui::Context::default();
    sim_engine::ui::install_fonts(&ctx);
    ctx.set_pixels_per_point(scale);
    let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), ..egui::RawInput::default() };
    let draw = |ctx: &egui::Context| {
        egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::from_gray(27))).show(ctx, |ui| {
            let painter = ui.painter();
            let visuals = ui.visuals().widgets.inactive;
            for (index, icon) in Icon::ALL.iter().enumerate() {
                let corner = Pos2::new((index % COLUMNS) as f32 * CELL + 8.0, (index / COLUMNS) as f32 * CELL + 8.0);
                let rect = Rect::from_min_size(corner, Vec2::splat(BUTTON));
                painter.rect(rect, visuals.rounding, visuals.weak_bg_fill, visuals.bg_stroke);
                icons::draw_bar(painter, rect, *icon);
                let font = FontId::new(13.0, egui::FontFamily::Name("bold".into()));
                painter.text(rect.left_top() + Vec2::new(3.0, 1.0), Align2::LEFT_TOP, "Q", font, visuals.text_color());
                let map_center = corner + Vec2::new(BUTTON * 0.5, rows as f32 * CELL + 20.0 + BUTTON * 0.5);
                icons::draw(painter, map_center, 8.0, *icon, 2, Color32::from_rgb(80, 150, 255));
            }
            painter.line_segment([Pos2::new(0.0, rows as f32 * CELL + 6.0), Pos2::new(size.x, rows as f32 * CELL + 6.0)], Stroke::new(1.0_f32, Color32::from_gray(60)));
        });
    };
    // The first frame loads the fonts; the second is the one drawn.
    let _ = ctx.run(input(), draw);
    let output_frame = ctx.run(input(), draw);
    // Shapes sample the font atlas too, at its white texel.
    let font = ctx.fonts(egui::epaint::Fonts::image).into_texture();
    let primitives = ctx.tessellate(output_frame.shapes, output_frame.pixels_per_point);
    let (width, height) = ((size.x * scale) as usize, (size.y * scale) as usize);
    let mut pixels = vec![[0.0_f32; 4]; width * height];
    for ClippedPrimitive { clip_rect, primitive } in &primitives {
        let Primitive::Mesh(mesh) = primitive else { continue };
        if mesh.texture_id != TextureId::default() {
            continue;
        }
        let texture = &font;
        let clip = Rect::from_min_max(clip_rect.min * scale, clip_rect.max * scale);
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]].map(|index| &mesh.vertices[index as usize]);
            let (pa, pb, pc) = (a.pos.to_vec2() * scale, b.pos.to_vec2() * scale, c.pos.to_vec2() * scale);
            let area = (pb - pa).x * (pc - pa).y - (pb - pa).y * (pc - pa).x;
            if area.abs() < 1e-6 {
                continue;
            }
            let min_x = pa.x.min(pb.x).min(pc.x).max(clip.min.x).max(0.0).floor() as usize;
            let max_x = (pa.x.max(pb.x).max(pc.x).min(clip.max.x).ceil() as usize).min(width);
            let min_y = pa.y.min(pb.y).min(pc.y).max(clip.min.y).max(0.0).floor() as usize;
            let max_y = (pa.y.max(pb.y).max(pc.y).min(clip.max.y).ceil() as usize).min(height);
            for y in min_y..max_y {
                for x in min_x..max_x {
                    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((pb - p).x * (pc - p).y - (pb - p).y * (pc - p).x) / area;
                    let w1 = ((pc - p).x * (pa - p).y - (pc - p).y * (pa - p).x) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let uv = a.uv.to_vec2() * w0 + b.uv.to_vec2() * w1 + c.uv.to_vec2() * w2;
                    let texel = texture.sample(uv.x, uv.y);
                    let color = [0, 1, 2, 3].map(|channel| {
                        let vertex = f32::from(a.color[channel]) * w0 + f32::from(b.color[channel]) * w1 + f32::from(c.color[channel]) * w2;
                        vertex / 255.0 * texel[channel]
                    });
                    let pixel = &mut pixels[y * width + x];
                    // Premultiplied alpha, over.
                    for channel in 0..4 {
                        pixel[channel] = color[channel] + pixel[channel] * (1.0 - color[3]);
                    }
                }
            }
        }
    }
    std::fs::write(&output, bmp(&pixels, width, height)).expect("write the sheet");
    println!("saved {output} ({width}x{height})");
}

/// A texture the rasterizer samples, as premultiplied colors from 0 to 1.
struct Texture {
    width: usize,
    height: usize,
    texels: Vec<[f32; 4]>,
}

impl Texture {
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let x = ((u * self.width as f32) as usize).min(self.width - 1);
        let y = ((v * self.height as f32) as usize).min(self.height - 1);
        self.texels[y * self.width + x]
    }
}

trait IntoTexture {
    fn into_texture(self) -> Texture;
}

impl IntoTexture for egui::FontImage {
    fn into_texture(self) -> Texture {
        let texels = self.srgba_pixels(None).map(|color| [color.r(), color.g(), color.b(), color.a()].map(|value| f32::from(value) / 255.0)).collect();
        Texture { width: self.width(), height: self.height(), texels }
    }
}

fn bmp(pixels: &[[f32; 4]], width: usize, height: usize) -> Vec<u8> {
    let row = (width * 3).div_ceil(4) * 4;
    let mut out = Vec::with_capacity(54 + row * height);
    let file_size = (54 + row * height) as u32;
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54_u32.to_le_bytes());
    out.extend_from_slice(&40_u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(-(height as i32)).to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&24_u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in 0..height {
        let start = out.len();
        for x in 0..width {
            let [r, g, b, _] = pixels[y * width + x];
            // egui's colors are in gamma space already, so they go out as they are.
            out.extend_from_slice(&[b, g, r].map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
        out.resize(start + row, 0);
    }
    out
}
