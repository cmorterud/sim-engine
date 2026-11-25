//! The ground's close-up texture: four patterns of light and dark, one a channel, made when the
//! renderer starts from noise that wraps around the patch, so that it tiles with no seam. The
//! terrain shader reads all four with one lookup and uses the one for each ground:
//!
//! - red: grass, fine blades leaning one way, with clumps;
//! - green: soil, small clods and pits;
//! - blue: rock, cracked plates;
//! - alpha: sand, ripples blown by the wind.
//!
//! Each is centred on 128, so that it brightens and darkens the ground's color about as much.
//! Its smaller copies (mip levels) average the pattern away, so that it fades to the plain color
//! with distance instead of shimmering.

/// Texels a side.
pub const DETAIL_SIDE: u32 = 256;

/// One mip level: its side, and four bytes a texel.
pub struct DetailLevel {
    pub side: u32,
    pub texels: Vec<u8>,
}

/// Value noise on a lattice of `across` by `cells` that wraps around, at a point from 0 to 1 each
/// way.
struct Wrapping {
    across: usize,
    cells: usize,
    values: Vec<f32>,
}

impl Wrapping {
    fn new(cells: usize, seed: u32) -> Self {
        Self::stretched(cells, cells, seed)
    }

    /// Fewer cells across than along, for patterns drawn out one way.
    fn stretched(across: usize, cells: usize, seed: u32) -> Self {
        let mut state = seed.max(1);
        let values = (0..across * cells)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as f32 / u32::MAX as f32
            })
            .collect();
        Self { across, cells, values }
    }

    fn at(&self, u: f32, v: f32) -> f32 {
        let (x, y) = (u.rem_euclid(1.0) * self.across as f32, v.rem_euclid(1.0) * self.cells as f32);
        let (cx, cy) = (x.floor() as usize % self.across, y.floor() as usize % self.cells);
        let (nx, ny) = ((cx + 1) % self.across, (cy + 1) % self.cells);
        let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
        let (tx, ty) = (smooth(x.fract()), smooth(y.fract()));
        let value = |ix: usize, iy: usize| self.values[iy * self.across + ix];
        let near = value(cx, cy) + (value(nx, cy) - value(cx, cy)) * tx;
        let far = value(cx, ny) + (value(nx, ny) - value(cx, ny)) * tx;
        near + (far - near) * ty
    }
}

/// The texture and its mip levels, largest first.
pub fn detail_levels() -> Vec<DetailLevel> {
    let side = DETAIL_SIDE as usize;
    let octaves = |seed: u32, cells: &[usize]| cells.iter().enumerate().map(|(index, &count)| Wrapping::new(count, seed + index as u32 * 7919)).collect::<Vec<_>>();
    let clumps = octaves(11, &[8, 16]);
    let blades = [Wrapping::stretched(16, 64, 23), Wrapping::stretched(32, 128, 24)];
    let clods = octaves(37, &[16, 32, 64, 128]);
    let plates = octaves(41, &[8, 16, 32]);
    let grit = octaves(53, &[64, 128]);
    let drift = octaves(61, &[4, 8]);

    let mut channels = vec![[0.0_f32; 4]; side * side];
    for y in 0..side {
        for x in 0..side {
            let (u, v) = (x as f32 / side as f32, y as f32 / side as f32);
            // Grass: blades stretched along one way, darker between clumps.
            let blade = blades[0].at(u, v) * 0.6 + blades[1].at(u + 0.5, v) * 0.4;
            let clump = clumps[0].at(u, v) * 0.6 + clumps[1].at(u, v) * 0.4;
            let grass = (blade - 0.5) * 0.9 + (clump - 0.5) * 0.5;
            // Soil: clods at several sizes, with sharp dark pits.
            let clod: f32 = clods.iter().zip([0.4, 0.3, 0.2, 0.1]).map(|(noise, weight)| noise.at(u, v) * weight).sum();
            let pit = (0.18 - (clods[2].at(u + 0.3, v + 0.7) - 0.5).abs()).max(0.0) * 3.0;
            let soil = (clod - 0.5) * 1.1 - pit * 0.6;
            // Rock: plates split by cracks where the noise crosses its middle, with grit.
            let plate = plates[0].at(u, v) * 0.5 + plates[1].at(u, v) * 0.3 + plates[2].at(u, v) * 0.2;
            let crack = 1.0 - ((plate - 0.5).abs() * 25.0).min(1.0);
            let rock = (plate - 0.5) * 0.8 + (grit[0].at(u, v) * 0.5 + grit[1].at(u, v) * 0.5 - 0.5) * 0.5 - crack * 0.5;
            // Sand: ripples across the wind, bent by slow drift. Whole numbers of them fit the
            // patch, so that they tile.
            let bend = drift[0].at(u, v) * 0.6 + drift[1].at(u, v) * 0.4;
            let ripple = ((v * 24.0 + bend * 3.0) * std::f32::consts::TAU).sin();
            let sand = ripple * 0.35 + (grit[1].at(u, v) - 0.5) * 0.4;
            channels[y * side + x] = [grass, soil, rock, sand];
        }
    }

    let mut levels = Vec::new();
    let mut current = channels;
    let mut current_side = side;
    loop {
        let texels = current.iter().flat_map(|texel| texel.map(|value| ((value * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8)).collect();
        levels.push(DetailLevel { side: current_side as u32, texels });
        if current_side == 1 {
            break;
        }
        let half = current_side / 2;
        let mut smaller = vec![[0.0_f32; 4]; half * half];
        for y in 0..half {
            for x in 0..half {
                let mut sum = [0.0; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let texel = current[(y * 2 + dy) * current_side + x * 2 + dx];
                    for (total, value) in sum.iter_mut().zip(texel) {
                        *total += value * 0.25;
                    }
                }
                smaller[y * half + x] = sum;
            }
        }
        current = smaller;
        current_side = half;
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_levels_halve_down_to_one_texel() {
        let levels = detail_levels();
        assert_eq!(levels.len(), 9);
        for (index, level) in levels.iter().enumerate() {
            assert_eq!(level.side, DETAIL_SIDE >> index);
            assert_eq!(level.texels.len() as u32, level.side * level.side * 4);
        }
    }

    #[test]
    fn each_pattern_is_centred_and_varies() {
        let level = &detail_levels()[0];
        for channel in 0..4 {
            let values: Vec<f32> = level.texels.chunks(4).map(|texel| f32::from(texel[channel])).collect();
            let mean = values.iter().sum::<f32>() / values.len() as f32;
            let spread = (values.iter().map(|value| (value - mean).powi(2)).sum::<f32>() / values.len() as f32).sqrt();
            assert!((mean - 128.0).abs() < 25.0, "channel {channel} is centred near 128: {mean}");
            assert!(spread > 10.0, "channel {channel} has some pattern: {spread}");
        }
    }

    #[test]
    fn it_tiles_with_no_seam() {
        let level = &detail_levels()[0];
        let side = level.side as usize;
        for channel in 0..4 {
            let at = |x: usize, y: usize| i32::from(level.texels[(y * side + x) * 4 + channel]);
            let inside = (0..side - 1).flat_map(|y| (0..side - 1).map(move |x| (x, y))).map(|(x, y)| (at(x + 1, y) - at(x, y)).abs().max((at(x, y + 1) - at(x, y)).abs())).max().unwrap();
            let seam = (0..side).map(|i| (at(0, i) - at(side - 1, i)).abs().max((at(i, 0) - at(i, side - 1)).abs())).max().unwrap();
            assert!(seam <= inside, "channel {channel}: seam {seam}, largest step inside {inside}");
        }
    }
}
