//! The small waves' texture: how steeply a patch of choppy water slopes, east and north, made
//! when the renderer starts from a sum of waves that each fit the patch a whole number of times,
//! so that it tiles with no seam. The water shader scrolls two copies of it across each other at
//! different sizes. Its smaller copies (mip levels) average the slopes away, so that far water
//! is smooth instead of sparkling.

/// Texels a side.
pub const WAVE_SIDE: u32 = 256;
/// How many waves are summed.
const WAVE_COUNT: usize = 48;
/// The most times a wave fits across the patch.
const MAX_REPEATS: i32 = 14;

/// One mip level: its side, and its texels, four bytes each: the slope east and north, each from
/// 0 for -1 to 255 for 1, then 128, 255.
pub struct WaveLevel {
    pub side: u32,
    pub texels: Vec<u8>,
}

/// The texture and its mip levels, largest first.
pub fn wave_levels() -> Vec<WaveLevel> {
    let side = WAVE_SIDE as usize;
    // A small generator of its own, so that the texture is the same on every machine and
    // nothing here touches the simulation's random numbers.
    let mut state: u32 = 0x9E37_79B9;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        f64::from(state) / f64::from(u32::MAX)
    };
    let mut waves = Vec::with_capacity(WAVE_COUNT);
    while waves.len() < WAVE_COUNT {
        let kx = (next() * f64::from(2 * MAX_REPEATS + 1)) as i32 - MAX_REPEATS;
        let ky = (next() * f64::from(2 * MAX_REPEATS + 1)) as i32 - MAX_REPEATS;
        let repeats = f64::from(kx * kx + ky * ky).sqrt();
        if !(2.0..=f64::from(MAX_REPEATS)).contains(&repeats) {
            continue;
        }
        // Longer waves are taller, as on the sea; their slope falls off more gently.
        let amplitude = repeats.powf(-1.6);
        waves.push((f64::from(kx), f64::from(ky), amplitude, next() * std::f64::consts::TAU));
    }
    let mut slopes = vec![[0.0_f64; 2]; side * side];
    let mut largest = 0.0_f64;
    for y in 0..side {
        for x in 0..side {
            let mut slope = [0.0, 0.0];
            for &(kx, ky, amplitude, phase) in &waves {
                let angle = std::f64::consts::TAU * (kx * x as f64 + ky * y as f64) / side as f64 + phase;
                let fall = -amplitude * angle.sin();
                slope[0] += fall * kx;
                slope[1] += fall * ky;
            }
            largest = largest.max(slope[0].abs()).max(slope[1].abs());
            slopes[y * side + x] = slope;
        }
    }
    for slope in &mut slopes {
        slope[0] /= largest;
        slope[1] /= largest;
    }
    let mut levels = Vec::new();
    let mut current = slopes;
    let mut current_side = side;
    loop {
        let texels = current
            .iter()
            .flat_map(|slope| [encode(slope[0]), encode(slope[1]), 128, 255])
            .collect();
        levels.push(WaveLevel { side: current_side as u32, texels });
        if current_side == 1 {
            break;
        }
        let half = current_side / 2;
        let mut smaller = vec![[0.0; 2]; half * half];
        for y in 0..half {
            for x in 0..half {
                let mut sum = [0.0, 0.0];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let slope = current[(y * 2 + dy) * current_side + x * 2 + dx];
                    sum[0] += slope[0] * 0.25;
                    sum[1] += slope[1] * 0.25;
                }
                smaller[y * half + x] = sum;
            }
        }
        current = smaller;
        current_side = half;
    }
    levels
}

fn encode(slope: f64) -> u8 {
    ((slope * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_levels_halve_down_to_one_texel() {
        let levels = wave_levels();
        assert_eq!(levels.len(), 9);
        for (index, level) in levels.iter().enumerate() {
            assert_eq!(level.side, WAVE_SIDE >> index);
            assert_eq!(level.texels.len() as u32, level.side * level.side * 4);
        }
    }

    #[test]
    fn slopes_reach_both_ways_and_average_out_far_away() {
        let levels = wave_levels();
        let east: Vec<u8> = levels[0].texels.chunks(4).map(|texel| texel[0]).collect();
        let (low, high) = (*east.iter().min().unwrap(), *east.iter().max().unwrap());
        assert!(low < 64 && high > 191, "the slopes should use most of the range: {low}..{high}");
        let last = &levels[levels.len() - 1].texels;
        assert!((i32::from(last[0]) - 128).abs() <= 3 && (i32::from(last[1]) - 128).abs() <= 3, "the smallest level is flat: {last:?}");
    }

    #[test]
    fn it_tiles_with_no_seam() {
        let level = &wave_levels()[0];
        let side = level.side as usize;
        let at = |x: usize, y: usize| i32::from(level.texels[(y * side + x) * 4]);
        // Across the seam the change is no larger than the largest change inside the patch.
        let inside = (0..side - 1).map(|x| (at(x + 1, 7) - at(x, 7)).abs()).max().unwrap();
        assert!((at(0, 7) - at(side - 1, 7)).abs() <= inside);
    }

    #[test]
    fn it_is_the_same_every_time() {
        assert_eq!(wave_levels()[0].texels, wave_levels()[0].texels);
    }
}
