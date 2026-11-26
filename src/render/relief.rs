//! The relief field: the ground's height, slope and creases at each height sample, worked out
//! once when a map loads. The terrain's vertices light the ground from it at full detail, and
//! where the grid is drawn coarser (tiles.rs) the terrain shader reads it at each pixel instead,
//! so that the light and the colors that follow the height do not change with the grid. One
//! texel a height sample, up to 1,025 a side, with smaller copies for the far distance:
//!
//! - red, green: how fast the ground rises east and north, per unit of distance;
//! - blue: how far the ground around rises above it, per unit of distance: above 0 in hollows
//!   and creases, below 0 on ridges and crests;
//! - alpha: the height.
//!
//! Half floats, so that gentle slopes light smoothly. Only the renderer reads it.

use super::camera::HeightField;

/// The most texels a side: every sample of the largest map the terrain grid draws in full.
const MAX_SIDE: usize = 1_025;
/// How many sizes are made, the full one first, each half the last.
const LEVELS: usize = 6;

/// One size of the field: `side` texels a side, four half floats each.
pub struct ReliefLevel {
    pub side: u32,
    pub texels: Vec<u16>,
}

/// The field for this ground, the full size first and then each smaller copy, each texel of a
/// copy the average of the four under it.
pub fn relief_levels(heights: &HeightField) -> Vec<ReliefLevel> {
    let side = heights.samples().clamp(2, MAX_SIDE);
    let cells = heights.samples() - 1;
    let spacing = cells as f64 * heights.cell_units() / (side - 1) as f64;
    let mut ground = vec![0.0_f32; side * side];
    for y in 0..side {
        for x in 0..side {
            ground[y * side + x] = heights.height_at(x as f64 * spacing, y as f64 * spacing) as f32;
        }
    }
    let spacing = spacing as f32;
    let last = side as isize - 1;
    // Clamped to the map's edge, as the shaders clamp their height lookups.
    let at = |x: isize, y: isize| ground[y.clamp(0, last) as usize * side + x.clamp(0, last) as usize];
    let mut full = vec![[0.0_f32; 4]; side * side];
    for y in 0..side as isize {
        for x in 0..side as isize {
            let here = at(x, y);
            // As the terrain's vertices worked it out until 2026-11-25: the slope across the
            // neighbouring samples, and creases at the grid's spacing and three times it.
            let rise_around = |reach: isize| (at(x + reach, y) + at(x - reach, y) + at(x, y + reach) + at(x, y - reach)) * 0.25 - here;
            let cavity = rise_around(1) / spacing * 0.4 + rise_around(3) / (3.0 * spacing) * 0.6;
            full[y as usize * side + x as usize] = [(at(x + 1, y) - at(x - 1, y)) / (2.0 * spacing), (at(x, y + 1) - at(x, y - 1)) / (2.0 * spacing), cavity, here];
        }
    }
    let mut levels = vec![(side, full)];
    while levels.len() < LEVELS {
        let (side, texels) = levels.last().expect("there is a level");
        if *side < 2 {
            break;
        }
        let half = side / 2;
        let mut smaller = vec![[0.0_f32; 4]; half * half];
        for y in 0..half {
            for x in 0..half {
                let mut sum = [0.0_f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let texel = texels[(y * 2 + dy) * side + x * 2 + dx];
                    for (total, value) in sum.iter_mut().zip(texel) {
                        *total += value * 0.25;
                    }
                }
                smaller[y * half + x] = sum;
            }
        }
        levels.push((half, smaller));
    }
    levels
        .into_iter()
        .map(|(side, texels)| ReliefLevel { side: side as u32, texels: texels.iter().flatten().map(|&value| half_float(value)).collect() })
        .collect()
}

/// The nearest half float to `value`, as its bits. Values too small for a half float's normal
/// range become 0, and values too large its largest.
pub(super) fn half_float(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    if exponent <= 0 || value.is_nan() {
        return sign;
    }
    if exponent >= 31 {
        return sign | 0x7bff;
    }
    // Rounded to nearest: a carry out of the mantissa raises the exponent, as it should.
    let rounded = ((exponent as u32) << 10) + (((bits & 0x007f_ffff) + 0x1000) >> 13);
    sign | rounded.min(0x7bff) as u16
}

/// The value of a half float's bits, as `half_float` makes them: no subnormals or infinities.
pub(super) fn from_half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    if exponent == 0 {
        return 0.0;
    }
    sign * (1.0 + f32::from(bits & 0x3ff) / 1024.0) * 2_f32.powi(exponent - 15)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_round_to_nearest() {
        for value in [0.0, 1.0, -1.0, 0.5, 0.0123, -0.37, 3.75, 1000.0, 65_504.0] {
            let back = from_half(half_float(value));
            assert!((back - value).abs() <= value.abs() / 1024.0, "{value} came back as {back}");
        }
        assert_eq!(from_half(half_float(1.0e9)), 65_504.0);
        assert_eq!(from_half(half_float(1.0e-9)), 0.0);
    }

    #[test]
    fn a_slope_reads_as_its_rise_and_a_hollow_as_creased() {
        // Ground rising half a unit per unit east, with a pit in the middle.
        let samples = 33;
        let mut heights: Vec<i16> = (0..samples * samples).map(|index| ((index % samples) * 10) as i16).collect();
        heights[16 * samples + 16] -= 40;
        let field = relief_levels(&HeightField::new(samples, heights, 20.0, 1.0));
        assert_eq!(field[0].side, 33);
        assert_eq!(field[1].side, 16);
        let texel = |x: usize, y: usize| -> [f32; 4] { std::array::from_fn(|channel| from_half(field[0].texels[(y * 33 + x) * 4 + channel])) };
        let open = texel(8, 8);
        assert!((open[0] - 0.5).abs() < 1e-3 && open[1].abs() < 1e-3, "{open:?}");
        assert!(open[2].abs() < 1e-3, "an even slope has no creases: {open:?}");
        assert!(texel(16, 16)[2] > 0.5, "the pit is a hollow: {:?}", texel(16, 16));
        assert!(texel(17, 16)[2] < 0.0, "and its rim is a crest: {:?}", texel(17, 16));
        assert_eq!(texel(8, 3)[3], 80.0, "the height is kept");
    }
}
