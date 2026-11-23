//! Generates the social preview banner (1200x630) and the thumbnail icon (512x512)
//! for sim-engine's GitHub Pages web deployment and social sharing cards.
//!
//! Run with:
//! `cargo run --release --example generate_social_assets`

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use egui::epaint::{ClippedPrimitive, Primitive};
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, TextureId, Vec2};

const PREVIEW_WIDTH: usize = 1200;
const PREVIEW_HEIGHT: usize = 630;
const THUMB_SIZE: usize = 512;

/// A simple RGBA image buffer with drawing and blending primitives.
#[derive(Clone)]
struct Image {
    width: usize,
    height: usize,
    pixels: Vec<[u8; 4]>,
}

impl Image {
    fn new(width: usize, height: usize, fill: [u8; 4]) -> Self {
        Self {
            width,
            height,
            pixels: vec![fill; width * height],
        }
    }

    fn from_png(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let decoder = png::Decoder::new(file);
        let mut reader = decoder.read_info()?;
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf)?;
        let w = info.width as usize;
        let h = info.height as usize;
        let pixels = match info.color_type {
            png::ColorType::Rgba => buf[..w * h * 4]
                .as_chunks::<4>()
                .0
                .to_vec(),
            png::ColorType::Rgb => buf[..w * h * 3]
                .as_chunks::<3>()
                .0
                .iter()
                .map(|c| [c[0], c[1], c[2], 255])
                .collect(),
            _ => return Err("unsupported PNG color type".into()),
        };
        Ok(Self {
            width: w,
            height: h,
            pixels,
        })
    }

    fn write_png(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(path)?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width as u32, self.height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Best);
        let mut writer = encoder.write_header()?;
        let mut data = Vec::with_capacity(self.width * self.height * 4);
        for p in &self.pixels {
            data.extend_from_slice(p);
        }
        writer.write_image_data(&data)?;
        Ok(())
    }

    #[inline(always)]
    fn get_pixel(&self, x: usize, y: usize) -> [u8; 4] {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x]
        } else {
            [0, 0, 0, 0]
        }
    }

    #[inline(always)]
    fn blend_pixel(&mut self, x: isize, y: isize, src: [u8; 4]) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let sa = f32::from(src[3]) / 255.0;
        if sa <= 0.0 {
            return;
        }
        let idx = y as usize * self.width + x as usize;
        let dst = &mut self.pixels[idx];
        let da = f32::from(dst[3]) / 255.0;
        let out_a = sa + da * (1.0 - sa);
        if out_a <= 0.0 {
            return;
        }
        for c in 0..3 {
            let sc = f32::from(src[c]);
            let dc = f32::from(dst[c]);
            let out_c = (sc * sa + dc * da * (1.0 - sa)) / out_a;
            dst[c] = out_c.clamp(0.0, 255.0) as u8;
        }
        dst[3] = (out_a * 255.0).clamp(0.0, 255.0) as u8;
    }

    #[inline(always)]
    fn blend_additive(&mut self, x: isize, y: isize, src: [u8; 4], intensity: f32) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let a = (f32::from(src[3]) / 255.0) * intensity;
        if a <= 0.0 {
            return;
        }
        let idx = y as usize * self.width + x as usize;
        let dst = &mut self.pixels[idx];
        for c in 0..3 {
            let add = f32::from(src[c]) * a;
            dst[c] = (f32::from(dst[c]) + add).clamp(0.0, 255.0) as u8;
        }
    }

    /// Crop a region of the image.
    fn crop(&self, x0: usize, y0: usize, w: usize, h: usize) -> Self {
        let mut cropped = Self::new(w, h, [0, 0, 0, 0]);
        for y in 0..h {
            for x in 0..w {
                cropped.pixels[y * w + x] = self.get_pixel(x0 + x, y0 + y);
            }
        }
        cropped
    }

    /// Draw another image scaled with bilinear filtering and alpha blending.
    fn draw_sprite(&mut self, sprite: &Image, dest_x: isize, dest_y: isize, dest_w: usize, dest_h: usize, tint: [f32; 4]) {
        let scale_x = sprite.width as f32 / dest_w as f32;
        let scale_y = sprite.height as f32 / dest_h as f32;

        for dy in 0..dest_h {
            let py = dest_y + dy as isize;
            if py < 0 || py as usize >= self.height {
                continue;
            }
            let sy = (dy as f32 + 0.5) * scale_y - 0.5;
            let sy0 = (sy.floor() as isize).clamp(0, sprite.height as isize - 1) as usize;
            let sy1 = (sy0 + 1).min(sprite.height - 1);
            let fy = (sy - sy.floor()).clamp(0.0, 1.0);

            for dx in 0..dest_w {
                let px = dest_x + dx as isize;
                if px < 0 || px as usize >= self.width {
                    continue;
                }
                let sx = (dx as f32 + 0.5) * scale_x - 0.5;
                let sx0 = (sx.floor() as isize).clamp(0, sprite.width as isize - 1) as usize;
                let sx1 = (sx0 + 1).min(sprite.width - 1);
                let fx = (sx - sx.floor()).clamp(0.0, 1.0);

                let p00 = sprite.pixels[sy0 * sprite.width + sx0];
                let p10 = sprite.pixels[sy0 * sprite.width + sx1];
                let p01 = sprite.pixels[sy1 * sprite.width + sx0];
                let p11 = sprite.pixels[sy1 * sprite.width + sx1];

                let mut sample = [0u8; 4];
                for c in 0..4 {
                    let top = f32::from(p00[c]) * (1.0 - fx) + f32::from(p10[c]) * fx;
                    let btm = f32::from(p01[c]) * (1.0 - fx) + f32::from(p11[c]) * fx;
                    let val = (top * (1.0 - fy) + btm * fy) * tint[c];
                    sample[c] = val.clamp(0.0, 255.0) as u8;
                }
                self.blend_pixel(px, py, sample);
            }
        }
    }

    /// Draw a line segment with anti-aliasing.
    fn draw_line(&mut self, p0: (f32, f32), p1: (f32, f32), width: f32, color: [u8; 4]) {
        let (x0, y0) = p0;
        let (x1, y1) = p1;
        let dx = x1 - x0;
        let dy = y1 - y0;
        let dist = dx.hypot(dy);
        if dist < 0.001 {
            return;
        }
        let step = 0.5;
        let steps = (dist / step).ceil() as usize;
        let half_w = width * 0.5;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let cx = x0 + dx * t;
            let cy = y0 + dy * t;
            let rad = half_w.ceil() as isize;
            for oy in -rad..=rad {
                for ox in -rad..=rad {
                    let d = (ox as f32).hypot(oy as f32);
                    if d <= half_w {
                        let alpha = ((half_w + 0.5 - d).clamp(0.0, 1.0) * f32::from(color[3])) as u8;
                        self.blend_pixel(cx as isize + ox, cy as isize + oy, [color[0], color[1], color[2], alpha]);
                    }
                }
            }
        }
    }

    /// Draw a circle outline with glow.
    fn draw_circle_outline(&mut self, cx: f32, cy: f32, radius: f32, thickness: f32, color: [u8; 4]) {
        let r_max = radius + thickness * 0.5;
        let min_x = (cx - r_max - 1.0).max(0.0) as isize;
        let max_x = (cx + r_max + 1.0).min(self.width as f32) as isize;
        let min_y = (cy - r_max - 1.0).max(0.0) as isize;
        let max_y = (cy + r_max + 1.0).min(self.height as f32) as isize;

        for y in min_y..max_y {
            for x in min_x..max_x {
                let d = ((x as f32 + 0.5) - cx).hypot((y as f32 + 0.5) - cy);
                let edge = (d - radius).abs();
                if edge <= thickness * 0.5 + 0.75 {
                    let coverage = (thickness * 0.5 + 0.75 - edge).clamp(0.0, 1.0);
                    let a = (f32::from(color[3]) * coverage) as u8;
                    self.blend_pixel(x, y, [color[0], color[1], color[2], a]);
                }
            }
        }
    }

    /// Draw a glowing translucent shield dome.
    fn draw_shield_dome(&mut self, cx: f32, cy: f32, radius: f32) {
        let min_x = (cx - radius - 2.0).max(0.0) as isize;
        let max_x = (cx + radius + 2.0).min(self.width as f32) as isize;
        let min_y = (cy - radius - 2.0).max(0.0) as isize;
        let max_y = (cy + radius + 2.0).min(self.height as f32) as isize;

        for y in min_y..max_y {
            for x in min_x..max_x {
                let d = ((x as f32 + 0.5) - cx).hypot((y as f32 + 0.5) - cy);
                if d <= radius {
                    let norm = d / radius;
                    let rim = norm.powi(4);
                    let interior = (1.0 - norm).powi(2) * 0.08;
                    let intensity = rim * 0.75 + interior;
                    let a = (intensity * 255.0).clamp(0.0, 220.0) as u8;
                    self.blend_additive(x, y, [60, 160, 255, a], 1.0);
                }
            }
        }
        self.draw_circle_outline(cx, cy, radius, 2.5, [100, 200, 255, 230]);
    }
}

/// A texture for sampling egui font atlas.
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

/// Rasterize egui primitives onto the Image buffer using standard premultiplied alpha over blending.
fn rasterize_egui(image: &mut Image, primitives: Vec<ClippedPrimitive>, font_img: &egui::FontImage, scale: f32) {
    let font_texels: Vec<[f32; 4]> = font_img
        .srgba_pixels(None)
        .map(|c| [c.r(), c.g(), c.b(), c.a()].map(|v| f32::from(v) / 255.0))
        .collect();
    let texture = Texture {
        width: font_img.width(),
        height: font_img.height(),
        texels: font_texels,
    };

    for ClippedPrimitive { clip_rect, primitive } in primitives {
        let Primitive::Mesh(mesh) = primitive else { continue };
        if mesh.texture_id != TextureId::default() {
            continue;
        }
        let clip = Rect::from_min_max(clip_rect.min * scale, clip_rect.max * scale);
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]].map(|idx| &mesh.vertices[idx as usize]);
            let (pa, pb, pc) = (a.pos.to_vec2() * scale, b.pos.to_vec2() * scale, c.pos.to_vec2() * scale);
            let area = (pb - pa).x * (pc - pa).y - (pb - pa).y * (pc - pa).x;
            if area.abs() < 1e-6 {
                continue;
            }
            let min_x = pa.x.min(pb.x).min(pc.x).max(clip.min.x).max(0.0).floor() as usize;
            let max_x = (pa.x.max(pb.x).max(pc.x).min(clip.max.x).ceil() as usize).min(image.width);
            let min_y = pa.y.min(pb.y).min(pc.y).max(clip.min.y).max(0.0).floor() as usize;
            let max_y = (pa.y.max(pb.y).max(pc.y).min(clip.max.y).ceil() as usize).min(image.height);

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
                    let color = [0, 1, 2, 3].map(|ch| {
                        let vertex = f32::from(a.color[ch]) * w0 + f32::from(b.color[ch]) * w1 + f32::from(c.color[ch]) * w2;
                        (vertex / 255.0) * texel[ch]
                    });
                    if color[3] <= 0.0 {
                        continue;
                    }

                    let idx = y * image.width + x;
                    for ch in 0..3 {
                        let cur = f32::from(image.pixels[idx][ch]) / 255.0;
                        let blended = color[ch] + cur * (1.0 - color[3]);
                        image.pixels[idx][ch] = (blended.clamp(0.0, 1.0) * 255.0).round() as u8;
                    }
                    image.pixels[idx][3] = 255;
                }
            }
        }
    }
}

/// Unit atlas helper.
struct UnitAtlas {
    sheet: Image,
    names: BTreeMap<String, usize>,
    tile_size: usize,
    columns: usize,
}

impl UnitAtlas {
    fn load(assets_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let sheet = Image::from_png(&assets_dir.join("portraits.png"))?;
        let text = std::fs::read_to_string(assets_dir.join("portraits.txt"))?;
        let mut lines = text.lines();
        let header = lines.next().ok_or("missing header")?;
        let mut parts = header.split_whitespace();
        let tile_size = parts.next().ok_or("missing tile size")?.parse()?;
        let columns = parts.next().ok_or("missing columns")?.parse()?;
        let mut names = BTreeMap::new();
        for (idx, line) in lines.enumerate() {
            let key = line.trim();
            if !key.is_empty() {
                names.insert(key.to_string(), idx);
            }
        }
        Ok(Self {
            sheet,
            names,
            tile_size,
            columns,
        })
    }

    fn sprite(&self, key: &str) -> Option<Image> {
        let idx = *self.names.get(key)?;
        let col = idx % self.columns;
        let row = idx / self.columns;
        let x0 = col * self.tile_size;
        let y0 = row * self.tile_size;
        Some(self.sheet.crop(x0, y0, self.tile_size, self.tile_size))
    }
}

/// Draw tactical HUD grid lines, coordinate ticks, and corner brackets.
fn draw_tactical_hud(img: &mut Image, w: usize, h: usize) {
    let cx = (w as f32) * 0.70;
    let cy = (h as f32) * 0.48;
    let max_radius = ((w as f32).powi(2) + (h as f32).powi(2)).sqrt() * 0.65;

    for y in 0..h {
        for x in 0..w {
            let dist = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let t = (dist / max_radius).clamp(0.0, 1.0);
            let r = (18.0 * (1.0 - t) + 4.0 * t) as u8;
            let g = (25.0 * (1.0 - t) + 6.0 * t) as u8;
            let b = (44.0 * (1.0 - t) + 11.0 * t) as u8;
            img.pixels[y * w + x] = [r, g, b, 255];
        }
    }

    let grid_spacing = 40.0;
    let grid_color = [35, 48, 74, 90];
    let tick_color = [60, 120, 255, 130];

    let mut x = 0.0;
    while x <= w as f32 {
        img.draw_line((x, 0.0), (x, h as f32), 1.0, grid_color);
        x += grid_spacing;
    }
    let mut y = 0.0;
    while y <= h as f32 {
        img.draw_line((0.0, y), (w as f32, y), 1.0, grid_color);
        y += grid_spacing;
    }

    let mut x = grid_spacing;
    while x < w as f32 {
        let mut y = grid_spacing;
        while y < h as f32 {
            img.draw_line((x - 3.0, y), (x + 3.0, y), 1.0, tick_color);
            img.draw_line((x, y - 3.0), (x, y + 3.0), 1.0, tick_color);
            y += grid_spacing * 2.0;
        }
        x += grid_spacing * 2.0;
    }

    let margin = 24.0;
    let bracket_len = 36.0;
    let border_col = [60, 130, 255, 200];
    let dim_border_col = [40, 56, 85, 140];

    img.draw_line((margin, margin), (w as f32 - margin, margin), 1.0, dim_border_col);
    img.draw_line((w as f32 - margin, margin), (w as f32 - margin, h as f32 - margin), 1.0, dim_border_col);
    img.draw_line((w as f32 - margin, h as f32 - margin), (margin, h as f32 - margin), 1.0, dim_border_col);
    img.draw_line((margin, h as f32 - margin), (margin, margin), 1.0, dim_border_col);

    img.draw_line((margin - 2.0, margin - 2.0), (margin + bracket_len, margin - 2.0), 3.0, border_col);
    img.draw_line((margin - 2.0, margin - 2.0), (margin - 2.0, margin + bracket_len), 3.0, border_col);

    img.draw_line((w as f32 - margin + 2.0, margin - 2.0), (w as f32 - margin - bracket_len, margin - 2.0), 3.0, border_col);
    img.draw_line((w as f32 - margin + 2.0, margin - 2.0), (w as f32 - margin + 2.0, margin + bracket_len), 3.0, border_col);

    img.draw_line((margin - 2.0, h as f32 - margin + 2.0), (margin + bracket_len, h as f32 - margin + 2.0), 3.0, border_col);
    img.draw_line((margin - 2.0, h as f32 - margin + 2.0), (margin - 2.0, h as f32 - margin - bracket_len), 3.0, border_col);

    img.draw_line((w as f32 - margin + 2.0, h as f32 - margin + 2.0), (w as f32 - margin - bracket_len, h as f32 - margin + 2.0), 3.0, border_col);
    img.draw_line((w as f32 - margin + 2.0, h as f32 - margin + 2.0), (w as f32 - margin + 2.0, h as f32 - margin - bracket_len), 3.0, border_col);
}

/// Generate the 1200x630 social preview image.
fn generate_social_preview(assets_dir: &Path, output_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut img = Image::new(PREVIEW_WIDTH, PREVIEW_HEIGHT, [5, 6, 10, 255]);
    draw_tactical_hud(&mut img, PREVIEW_WIDTH, PREVIEW_HEIGHT);

    let atlas = UnitAtlas::load(assets_dir)?;
    let map = Image::from_png(&assets_dir.join("web/maps/skirmish.png"))?;

    // --- Right Section: Holographic Combat Theater ---
    let map_cx = 900.0;
    let map_cy = 315.0;
    let map_w = 460;
    let map_h = 360;

    img.draw_circle_outline(map_cx, map_cy, 240.0, 1.5, [40, 80, 160, 90]);
    img.draw_circle_outline(map_cx, map_cy, 180.0, 1.0, [40, 80, 160, 70]);
    img.draw_circle_outline(map_cx, map_cy, 120.0, 1.0, [40, 80, 160, 60]);

    let map_rect_x = (map_cx - (map_w as f32) * 0.5) as isize;
    let map_rect_y = (map_cy - (map_h as f32) * 0.5 + 20.0) as isize;

    for dy in 0..map_h {
        let py = map_rect_y + dy as isize;
        if py < 0 || py as usize >= img.height {
            continue;
        }
        let v = dy as f32 / map_h as f32;
        let sy = (v * map.height as f32) as usize;

        for dx in 0..map_w {
            let px = map_rect_x + dx as isize;
            if px < 0 || px as usize >= img.width {
                continue;
            }
            let u = dx as f32 / map_w as f32;
            let sx = (u * map.width as f32) as usize;

            let pixel = map.get_pixel(sx, sy);
            let dist_center = ((u - 0.5).powi(2) * 1.4 + (v - 0.5).powi(2) * 1.8).sqrt();
            let vignette = (1.0 - (dist_center * 1.8 - 0.4).clamp(0.0, 1.0)).powi(2);
            let alpha = (vignette * 150.0) as u8;

            let r = (f32::from(pixel[0]) * 0.85 + 20.0) as u8;
            let g = (f32::from(pixel[1]) * 0.95 + 30.0) as u8;
            let b = (f32::from(pixel[2]) * 1.1 + 50.0) as u8;
            img.blend_pixel(px, py, [r, g, b, alpha]);
        }
    }

    for i in 0..=8 {
        let ly = map_rect_y as f32 + (i as f32) * (map_h as f32 / 8.0);
        img.draw_line((map_rect_x as f32, ly), (map_rect_x as f32 + map_w as f32, ly), 1.0, [60, 140, 255, 35]);
    }
    for i in 0..=10 {
        let lx = map_rect_x as f32 + (i as f32) * (map_w as f32 / 10.0);
        img.draw_line((lx, map_rect_y as f32), (lx, map_rect_y as f32 + map_h as f32), 1.0, [60, 140, 255, 35]);
    }

    let angle: f32 = 42.0_f32.to_radians();
    let sweep_x = map_cx + angle.cos() * 240.0;
    let sweep_y = map_cy + angle.sin() * 240.0;
    img.draw_line((map_cx, map_cy), (sweep_x, sweep_y), 2.0, [100, 210, 255, 180]);

    let deposits = [
        (map_cx - 130.0, map_cy - 70.0),
        (map_cx - 70.0, map_cy + 90.0),
        (map_cx + 80.0, map_cy - 50.0),
        (map_cx + 140.0, map_cy + 80.0),
        (map_cx - 20.0, map_cy - 120.0),
        (map_cx + 40.0, map_cy + 130.0),
    ];
    for &(dx, dy) in &deposits {
        img.draw_circle_outline(dx, dy, 7.0, 1.5, [0, 240, 255, 200]);
        img.blend_pixel(dx as isize, dy as isize, [220, 255, 255, 255]);
    }

    // Translucent Shield Dome
    let shield_cx = map_cx - 100.0;
    let shield_cy = map_cy + 30.0;
    img.draw_shield_dome(shield_cx, shield_cy, 110.0);

    // 1. Land factory inside shield perimeter
    if let Some(factory) = atlas.sprite("land_factory_t1") {
        img.draw_sprite(&factory, (shield_cx - 85.0) as isize, (shield_cy - 60.0) as isize, 115, 115, [1.0, 1.0, 1.0, 0.9]);
    }

    // 2. Heavy Siege Walker
    if let Some(walker) = atlas.sprite("t3_siege_walker") {
        img.draw_sprite(&walker, (map_cx - 200.0) as isize, (map_cy + 55.0) as isize, 140, 140, [1.0, 1.0, 1.0, 0.95]);
    }

    // 3. T2 Heavy Tank in front right
    if let Some(tank) = atlas.sprite("t2_heavy_tank") {
        img.draw_sprite(&tank, (map_cx + 80.0) as isize, (map_cy + 105.0) as isize, 130, 130, [1.0, 1.0, 1.0, 0.95]);
    }

    // 4. Heroic Command Rig Mech
    if let Some(commander) = atlas.sprite("command_rig") {
        let cmd_size = 280;
        let cmd_x = (map_cx - 65.0) as isize;
        let cmd_y = (map_cy - 85.0) as isize;

        img.draw_sprite(&commander, cmd_x + 10, cmd_y + 12, cmd_size, cmd_size, [0.0, 0.0, 0.0, 0.55]);
        img.draw_sprite(&commander, cmd_x, cmd_y, cmd_size, cmd_size, [1.05, 1.05, 1.1, 1.0]);

        let ccx = cmd_x as f32 + 140.0;
        let ccy = cmd_y as f32 + 140.0;
        img.draw_circle_outline(ccx, ccy, 144.0, 2.0, [60, 150, 255, 160]);
        img.draw_line((ccx - 152.0, ccy), (ccx - 136.0, ccy), 2.0, [100, 210, 255, 240]);
        img.draw_line((ccx + 136.0, ccy), (ccx + 152.0, ccy), 2.0, [100, 210, 255, 240]);
        img.draw_line((ccx, ccy - 152.0), (ccx, ccy - 136.0), 2.0, [100, 210, 255, 240]);
        img.draw_line((ccx, ccy + 136.0), (ccx, ccy + 152.0), 2.0, [100, 210, 255, 240]);

        // Glowing visor effect
        let vx = (cmd_x as f32 + 76.0) as isize;
        let vy = (cmd_y as f32 + 150.0) as isize;
        for oy in -3..=3 {
            for ox in -3..=3 {
                img.blend_additive(vx + ox, vy + oy, [0, 230, 255, 240], 0.8);
                img.blend_additive(vx + ox + 4, vy + oy + 26, [0, 230, 255, 240], 0.8);
            }
        }
    }

    // 5. Gunship in flight above
    if let Some(gunship) = atlas.sprite("t2_gunship") {
        let gx = (map_cx + 110.0) as isize;
        let gy = (map_cy - 180.0) as isize;
        img.draw_line((gx as f32 + 120.0, gy as f32 - 40.0), (gx as f32 + 40.0, gy as f32 + 30.0), 2.5, [100, 180, 255, 130]);
        img.draw_sprite(&gunship, gx, gy, 115, 115, [1.1, 1.1, 1.15, 0.95]);
    }

    // Projectile trails
    img.draw_line((map_cx - 170.0, map_cy + 90.0), (map_cx + 120.0, map_cy - 50.0), 2.0, [255, 160, 50, 180]);
    img.draw_line((map_cx - 50.0, map_cy + 20.0), (map_cx + 180.0, map_cy - 90.0), 2.5, [70, 210, 255, 220]);

    // --- Left Section: Typography & Badges via egui ---
    let ctx = egui::Context::default();
    sim_engine::ui::install_fonts(&ctx);
    ctx.set_pixels_per_point(1.0);

    let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(PREVIEW_WIDTH as f32, PREVIEW_HEIGHT as f32));
    let raw_input = egui::RawInput {
        screen_rect: Some(screen_rect),
        ..egui::RawInput::default()
    };

    let draw_ui = |ctx: &egui::Context| {
        egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::TRANSPARENT)).show(ctx, |ui| {
            let painter = ui.painter();
            let font_bold = |size: f32| FontId::new(size, egui::FontFamily::Name("bold".into()));

            // Category tag
            painter.text(
                Pos2::new(76.0, 68.0),
                Align2::LEFT_TOP,
                "PLANETARY-SCALE FLOW-ECONOMY RTS  //  HALVARD WAR",
                font_bold(13.5),
                Color32::from_rgb(90, 180, 255),
            );

            // Main Title: "sim-engine"
            painter.text(
                Pos2::new(74.0, 96.0),
                Align2::LEFT_TOP,
                "sim-engine",
                font_bold(82.0),
                Color32::from_rgb(255, 255, 255),
            );

            // Subtitle
            painter.text(
                Pos2::new(76.0, 206.0),
                Align2::LEFT_TOP,
                "A deterministic RTS of factories, engineers, and armies of hundreds.",
                font_bold(19.5),
                Color32::from_rgb(220, 230, 245),
            );

            painter.text(
                Pos2::new(76.0, 238.0),
                Align2::LEFT_TOP,
                "Build streaming economies, field massive battle groups across land, sea, and air,\nand destroy the enemy commander to win.",
                font_bold(15.5),
                Color32::from_rgb(160, 175, 200),
            );

            // Technical Badges in a compact 2x2 layout (avoids extending into the right-hand map)
            let badge_row1 = [
                ("5,000 UNITS @ 10 TPS", Color32::from_rgb(70, 140, 255)),
                ("DETERMINISTIC CORE", Color32::from_rgb(0, 210, 240)),
            ];
            let badge_row2 = [
                ("FLOW-RATE ECONOMY", Color32::from_rgb(60, 220, 140)),
                ("BUILT IN RUST", Color32::from_rgb(245, 140, 50)),
            ];

            let badge_font = font_bold(13.0);
            for (row_idx, row) in [badge_row1, badge_row2].iter().enumerate() {
                let mut bx = 76.0;
                let by = 306.0 + row_idx as f32 * 36.0;
                for (text, accent) in row {
                    let text_size = painter.layout_no_wrap(text.to_string(), badge_font.clone(), Color32::WHITE).size();
                    let pad_x = 12.0;
                    let pad_y = 6.0;
                    let rect = Rect::from_min_size(Pos2::new(bx, by), Vec2::new(text_size.x + pad_x * 2.0, text_size.y + pad_y * 2.0));

                    painter.rect(
                        rect,
                        6.0_f32,
                        Color32::from_rgba_premultiplied(16, 24, 42, 230),
                        Stroke::new(1.4_f32, *accent),
                    );
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        *text,
                        badge_font.clone(),
                        Color32::from_rgb(240, 246, 255),
                    );
                    bx += rect.width() + 12.0;
                }
            }

            // Bullet features
            let feat_font = font_bold(14.0);
            let features = [
                "• True physical 3D ballistics and radar sensors",
                "• Autonomous combat commanders, experimentals, and shields",
                "• Real-time browser play via WebGL2 / WebGPU & WebAssembly",
            ];
            for (idx, feat) in features.iter().enumerate() {
                painter.text(
                    Pos2::new(76.0, 396.0 + idx as f32 * 25.0),
                    Align2::LEFT_TOP,
                    *feat,
                    feat_font.clone(),
                    Color32::from_rgb(180, 195, 218),
                );
            }

            // Footer Callout
            let callout_rect = Rect::from_min_size(Pos2::new(76.0, 490.0), Vec2::new(470.0, 68.0));
            painter.rect(
                callout_rect,
                8.0_f32,
                Color32::from_rgba_premultiplied(14, 22, 38, 240),
                Stroke::new(1.6_f32, Color32::from_rgb(70, 140, 255)),
            );

            painter.text(
                Pos2::new(96.0, 502.0),
                Align2::LEFT_TOP,
                "PLAY IN YOUR BROWSER",
                font_bold(12.0),
                Color32::from_rgb(110, 180, 255),
            );

            painter.text(
                Pos2::new(96.0, 522.0),
                Align2::LEFT_TOP,
                "codymorterud.com/sim-engine",
                font_bold(21.0),
                Color32::from_rgb(255, 255, 255),
            );

            painter.text(
                Pos2::new(76.0, 578.0),
                Align2::LEFT_TOP,
                "Open Source on GitHub: github.com/cmorterud/sim-engine",
                font_bold(12.5),
                Color32::from_rgb(120, 135, 160),
            );

            painter.text(
                Pos2::new(PREVIEW_WIDTH as f32 - 76.0, PREVIEW_HEIGHT as f32 - 38.0),
                Align2::RIGHT_BOTTOM,
                "SYS: SIMULATION CORE ONLINE  //  VER 2026.11",
                font_bold(12.0),
                Color32::from_rgb(80, 150, 240),
            );
        });
    };

    let _ = ctx.run(raw_input.clone(), draw_ui);
    let output = ctx.run(raw_input, draw_ui);
    let font_image = ctx.fonts(egui::epaint::Fonts::image);
    let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
    rasterize_egui(&mut img, primitives, &font_image, output.pixels_per_point);

    img.write_png(output_path)?;
    println!("Generated social preview: {} ({}x{})", output_path.display(), PREVIEW_WIDTH, PREVIEW_HEIGHT);
    Ok(())
}

/// Generate the 512x512 thumbnail app icon.
fn generate_thumbnail(assets_dir: &Path, output_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut img = Image::new(THUMB_SIZE, THUMB_SIZE, [8, 10, 16, 255]);
    let w = THUMB_SIZE;
    let h = THUMB_SIZE;
    let cx = (w as f32) * 0.5;
    let cy = (h as f32) * 0.44;

    // 1. Dark radial tactical background
    let max_radius = ((w as f32).powi(2) + (h as f32).powi(2)).sqrt() * 0.55;
    for y in 0..h {
        for x in 0..w {
            let dist = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let t = (dist / max_radius).clamp(0.0, 1.0);
            let r = (24.0 * (1.0 - t) + 6.0 * t) as u8;
            let g = (32.0 * (1.0 - t) + 8.0 * t) as u8;
            let b = (56.0 * (1.0 - t) + 14.0 * t) as u8;
            img.pixels[y * w + x] = [r, g, b, 255];
        }
    }

    // 2. Tactical grid & radar rings
    for radius in [75.0, 135.0, 195.0, 225.0] {
        img.draw_circle_outline(cx, cy, radius, 1.5, [45, 95, 190, 120]);
    }
    // Crosshairs
    img.draw_line((cx - 230.0, cy), (cx + 230.0, cy), 1.2, [45, 95, 190, 100]);
    img.draw_line((cx, cy - 230.0), (cx, cy + 230.0), 1.2, [45, 95, 190, 100]);

    // Diagonal radar hash marks
    let diag = 210.0 * std::f32::consts::FRAC_1_SQRT_2;
    img.draw_line((cx - diag, cy - diag), (cx - diag + 16.0, cy - diag + 16.0), 2.0, [60, 140, 255, 160]);
    img.draw_line((cx + diag, cy - diag), (cx + diag - 16.0, cy - diag + 16.0), 2.0, [60, 140, 255, 160]);
    img.draw_line((cx - diag, cy + diag), (cx - diag + 16.0, cy + diag - 16.0), 2.0, [60, 140, 255, 160]);
    img.draw_line((cx + diag, cy + diag), (cx + diag - 16.0, cy + diag - 16.0), 2.0, [60, 140, 255, 160]);

    // 3. Prominent Heroic Command Rig Mech
    let atlas = UnitAtlas::load(assets_dir)?;
    if let Some(commander) = atlas.sprite("command_rig") {
        let cmd_size = 320;
        let cmd_x = (cx - (cmd_size as f32) * 0.5) as isize;
        let cmd_y = (cy - (cmd_size as f32) * 0.5 - 10.0) as isize;

        img.draw_sprite(&commander, cmd_x + 8, cmd_y + 12, cmd_size, cmd_size, [0.0, 0.0, 0.0, 0.65]);
        img.draw_sprite(&commander, cmd_x, cmd_y, cmd_size, cmd_size, [1.1, 1.1, 1.15, 1.0]);

        img.draw_circle_outline(cx, cy - 10.0, 168.0, 2.2, [60, 160, 255, 180]);

        // Glowing visor slit on commander
        let vx = (cmd_x as f32 + 88.0) as isize;
        let vy = (cmd_y as f32 + 172.0) as isize;
        for oy in -4..=4 {
            for ox in -4..=4 {
                img.blend_additive(vx + ox, vy + oy, [0, 240, 255, 255], 0.9);
                img.blend_additive(vx + ox + 4, vy + oy + 30, [0, 240, 255, 255], 0.9);
            }
        }
    }

    // 4. Outer border with chamfered/rounded corners
    let border_pad = 16.0;
    let border_col = [60, 140, 255, 230];
    let chamfer = 24.0;
    let b_min = border_pad;
    let b_max = w as f32 - border_pad;

    img.draw_line((b_min + chamfer, b_min), (b_max - chamfer, b_min), 3.0, border_col);
    img.draw_line((b_max, b_min + chamfer), (b_max, b_max - chamfer), 3.0, border_col);
    img.draw_line((b_max - chamfer, b_max), (b_min + chamfer, b_max), 3.0, border_col);
    img.draw_line((b_min, b_max - chamfer), (b_min, b_min + chamfer), 3.0, border_col);
    img.draw_line((b_min, b_min + chamfer), (b_min + chamfer, b_min), 3.0, border_col);
    img.draw_line((b_max - chamfer, b_min), (b_max, b_min + chamfer), 3.0, border_col);
    img.draw_line((b_max, b_max - chamfer), (b_max - chamfer, b_max), 3.0, border_col);
    img.draw_line((b_min + chamfer, b_max), (b_min, b_max - chamfer), 3.0, border_col);

    // 5. Bottom Emblem Title Plate via egui
    let ctx = egui::Context::default();
    sim_engine::ui::install_fonts(&ctx);
    ctx.set_pixels_per_point(1.0);

    let screen_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(THUMB_SIZE as f32, THUMB_SIZE as f32));
    let raw_input = egui::RawInput {
        screen_rect: Some(screen_rect),
        ..egui::RawInput::default()
    };

    let draw_ui = |ctx: &egui::Context| {
        egui::CentralPanel::default().frame(egui::Frame::none().fill(Color32::TRANSPARENT)).show(ctx, |ui| {
            let painter = ui.painter();
            let font_bold = |size: f32| FontId::new(size, egui::FontFamily::Name("bold".into()));

            let plate_rect = Rect::from_min_size(Pos2::new(48.0, 404.0), Vec2::new(w as f32 - 96.0, 72.0));
            painter.rect(
                plate_rect,
                8.0_f32,
                Color32::from_rgba_premultiplied(14, 20, 36, 250),
                Stroke::new(2.0_f32, Color32::from_rgb(70, 150, 255)),
            );

            // Title: "SIM-ENGINE"
            painter.text(
                Pos2::new(cx, 428.0),
                Align2::CENTER_CENTER,
                "SIM-ENGINE",
                font_bold(32.0),
                Color32::from_rgb(255, 255, 255),
            );

            // Sub-tag: "PLANETARY RTS"
            painter.text(
                Pos2::new(cx, 455.0),
                Align2::CENTER_CENTER,
                "PLANETARY FLOW RTS",
                font_bold(12.0),
                Color32::from_rgb(90, 180, 255),
            );
        });
    };

    let _ = ctx.run(raw_input.clone(), draw_ui);
    let output = ctx.run(raw_input, draw_ui);
    let font_image = ctx.fonts(egui::epaint::Fonts::image);
    let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
    rasterize_egui(&mut img, primitives, &font_image, output.pixels_per_point);

    img.write_png(output_path)?;
    println!("Generated thumbnail: {} ({}x{})", output_path.display(), THUMB_SIZE, THUMB_SIZE);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let assets_dir = manifest_dir.join("assets");
    let web_assets_dir = assets_dir.join("web");

    let preview_out = web_assets_dir.join("social-preview.png");
    let thumb_out = web_assets_dir.join("thumbnail.png");

    println!("Generating sim-engine social preview and thumbnail...");
    generate_social_preview(&assets_dir, &preview_out)?;
    generate_thumbnail(&assets_dir, &thumb_out)?;

    println!("All assets successfully generated!");
    Ok(())
}
