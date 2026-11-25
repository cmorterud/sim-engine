//! The land map: how the light falls on each point of the ground, worked out once when a map
//! loads, since the sun never moves. One texel a height sample, up to 1,024 a side, since hill
//! shadows are soft and this keeps the bake short on big maps:
//!
//! - red: sunlight, 0 where a hill casts its shadow and 1 in the open, soft at the edges;
//! - green: how open the ground is to the sky, low in valleys and at the feet of cliffs;
//! - blue: how damp the ground is, high in hollows that gather water and near the shore;
//! - alpha: unused, 255.
//!
//! It is baked from a copy of the heights, in floats, and only the renderer reads it.

use super::camera::HeightField;

/// Where the sun shines from, as the shaders have it, for anything else lit the same way.
pub const SUN_DIRECTION: [f32; 3] = super::sight::LIGHT;

/// The most texels a side.
const MAX_SIDE: usize = 1_024;
/// How far a shadow is looked for along the sunlight, in texels.
const SHADOW_REACH: f32 = 200.0;
/// The shadow ray's steps lengthen by this share of how far it has come, since far hills cast
/// broad shadows and need not be found to the texel.
const SHADOW_STRIDE: f32 = 0.06;
/// The eight directions looked along, a texel at a time: straight and diagonal.
const DIRECTIONS: [(isize, isize); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
/// How many of those steps away each direction is looked at for the horizon.
const HORIZON_STEPS: [isize; 7] = [1, 2, 3, 5, 8, 13, 21];
/// How many steps around a hollow is measured for dampness.
const DAMP_REACH: isize = 5;

pub struct LandField {
    side: usize,
    texels: Vec<u8>,
}

impl LandField {
    /// The map for this ground, lit from `light` (pointing towards the sun), with water up to
    /// `water_level`.
    pub fn bake(heights: &HeightField, light: [f32; 3], water_level: f64) -> Self {
        Self::bake_sized(heights, light, water_level, heights.samples().min(MAX_SIDE))
    }

    /// The same, `side` texels a side, at least 2, for a smaller picture such as the minimap's.
    pub fn bake_sized(heights: &HeightField, light: [f32; 3], water_level: f64, side: usize) -> Self {
        let cells = heights.samples() - 1;
        let side = side.max(2);
        let map = cells as f64 * heights.cell_units();
        let spacing = map / (side - 1) as f64;
        let mut ground = vec![0.0_f32; side * side];
        for y in 0..side {
            for x in 0..side {
                ground[y * side + x] = heights.height_at(x as f64 * spacing, y as f64 * spacing) as f32;
            }
        }
        let spacing = spacing as f32;
        let highest = ground.iter().copied().fold(f32::MIN, f32::max);
        let last = (side - 1) as f32;
        let last_index = side as isize - 1;
        // Height at a point between texels, clamped to the map.
        let at = |x: f32, y: f32| -> f32 {
            let (x, y) = (x.clamp(0.0, last), y.clamp(0.0, last));
            let (cx, cy) = ((x as usize).min(side - 2), (y as usize).min(side - 2));
            let (tx, ty) = (x - cx as f32, y - cy as f32);
            let h = |ix: usize, iy: usize| ground[iy * side + ix];
            let near = h(cx, cy) + (h(cx + 1, cy) - h(cx, cy)) * tx;
            let far = h(cx, cy + 1) + (h(cx + 1, cy + 1) - h(cx, cy + 1)) * tx;
            near + (far - near) * ty
        };
        let flat = (light[0] * light[0] + light[1] * light[1]).sqrt().max(1e-4);
        let sun_step = [light[0] / flat, light[1] / flat];
        // How much the sunlight climbs for each texel it travels across the ground.
        let climb = light[2] / flat * spacing;
        let mut texels = vec![255_u8; side * side * 4];
        for y in 0..side {
            for x in 0..side {
                let here = ground[y * side + x];
                let (fx, fy) = (x as f32, y as f32);

                // Sunlight: step towards the sun until the ray is above every hill on the map.
                // How far the ground rises above the ray, against how far along, softens the edge.
                let mut lit: f32 = 1.0;
                let mut along = 0.0;
                while along < SHADOW_REACH {
                    along += 1.0_f32.max(along * SHADOW_STRIDE);
                    let ray = here + 0.5 + climb * along;
                    if ray > highest {
                        break;
                    }
                    let above = at(fx + sun_step[0] * along, fy + sun_step[1] * along) - ray;
                    lit = lit.min(1.0 - (above / (along * spacing * 0.08)).clamp(0.0, 1.0));
                    if lit <= 0.0 {
                        break;
                    }
                }

                // Openness: in eight directions, the highest the ground rises above here, as
                // the sine of its angle, averaged. The lookups land on texels, so need no
                // blending.
                let texel_at = |dx: isize, dy: isize| {
                    let (ix, iy) = ((x as isize + dx).clamp(0, last_index), (y as isize + dy).clamp(0, last_index));
                    ground[iy as usize * side + ix as usize]
                };
                let mut hidden = 0.0;
                for &(dx, dy) in &DIRECTIONS {
                    let length = if dx != 0 && dy != 0 { std::f32::consts::SQRT_2 } else { 1.0 } * spacing;
                    let mut steepest: f32 = 0.0;
                    for &steps in &HORIZON_STEPS {
                        let rise = texel_at(dx * steps, dy * steps) - here;
                        steepest = steepest.max(rise / (steps as f32 * length));
                    }
                    hidden += steepest / (1.0 + steepest * steepest).sqrt();
                }
                let open = 1.0 - hidden / 8.0;

                // Dampness: how far the ground around stands above here, and nearness to water.
                let around: f32 = DIRECTIONS.iter().map(|&(dx, dy)| texel_at(dx * DAMP_REACH, dy * DAMP_REACH)).sum();
                let hollow = ((around / 8.0 - here) / (DAMP_REACH as f32 * 1.2 * spacing) * 4.0).clamp(0.0, 1.0);
                let above_water = here - water_level as f32;
                let shore = (1.0 - above_water / 12.0).clamp(0.0, 1.0);
                let damp = hollow.max(shore * 0.7);

                let texel = &mut texels[(y * side + x) * 4..(y * side + x) * 4 + 4];
                texel[0] = (lit * 255.0).round() as u8;
                texel[1] = (open.clamp(0.0, 1.0) * 255.0).round() as u8;
                texel[2] = (damp * 255.0).round() as u8;
            }
        }
        Self { side, texels }
    }

    pub const fn side(&self) -> usize {
        self.side
    }

    /// Four bytes a texel, row by row from the map's south-west corner.
    pub fn texels(&self) -> &[u8] {
        &self.texels
    }

    /// One of a texel's four bytes: 0 sunlight, 1 openness, 2 dampness.
    pub fn channel(&self, x: usize, y: usize, channel: usize) -> u8 {
        self.texels[(y * self.side + x) * 4 + channel]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flat ground at height 0 with a wall 100 high along x = 32 to 34, on 65 samples 20
    /// units apart.
    fn wall() -> HeightField {
        let samples = 65;
        let heights = (0..samples * samples).map(|index| if (32..=34).contains(&(index % samples)) { 100 } else { 0 }).collect();
        HeightField::new(samples, heights, 20.0, 1.0)
    }

    /// The sun low in the east: its light comes from +x.
    const LOW_EAST_SUN: [f32; 3] = [0.9, 0.0, 0.436];

    #[test]
    fn a_wall_shades_the_ground_on_its_far_side_from_the_sun() {
        let field = LandField::bake(&wall(), LOW_EAST_SUN, -1_000.0);
        let (west, east) = (field.channel(28, 32, 0), field.channel(40, 32, 0));
        assert!(west < 60, "west of the wall is in its shadow: {west}");
        assert_eq!(east, 255, "east of the wall faces the sun");
        assert_eq!(field.channel(5, 32, 0), 255, "far beyond the shadow is lit");
    }

    #[test]
    fn the_foot_of_a_wall_is_less_open_to_the_sky_than_open_ground() {
        let field = LandField::bake(&wall(), LOW_EAST_SUN, -1_000.0);
        let (foot, open) = (field.channel(31, 32, 1), field.channel(5, 32, 1));
        assert!(foot + 30 < open, "foot {foot}, open ground {open}");
        assert_eq!(open, 255);
    }

    #[test]
    fn ground_near_the_water_is_damp_and_high_dry_ground_is_not() {
        let field = LandField::bake(&wall(), LOW_EAST_SUN, -2.0);
        assert!(field.channel(5, 32, 2) > 100, "flat ground just above the water is damp");
        assert!(field.channel(33, 32, 2) < 20, "the top of the wall is dry");
    }

    #[test]
    fn a_field_is_one_texel_a_sample_and_four_bytes_a_texel() {
        let field = LandField::bake(&wall(), LOW_EAST_SUN, 0.0);
        assert_eq!(field.side(), 65);
        assert_eq!(field.texels().len(), 65 * 65 * 4);
    }
}
