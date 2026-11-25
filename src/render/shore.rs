//! The shore field: how far each point of the map is from the water's edge, worked out once
//! when a map loads, for foam that rolls towards the beach and for wet sand. It is baked from a
//! copy of the heights, in floats, and only the renderer reads it.
//!
//! The field is finer than the height samples, two texels a cell up to 2,048 a side, and found
//! from the heights between samples, so that the coast it draws is smooth instead of following
//! the terrain grid's 20-unit steps.

use super::camera::HeightField;

/// How far from the edge the field reaches, either way, in world units: beyond it, it holds this.
pub const SHORE_RANGE: f32 = 128.0;
/// The most texels a side, which every WebGL2 device can hold.
const MAX_SIDE: usize = 2_048;

/// Distances to the water's edge, positive over water and negative over land, one per texel of
/// a square grid whose first and last texels lie on the map's edges.
pub struct ShoreField {
    side: usize,
    /// Signed distance to the edge, in world units, clamped to `SHORE_RANGE` either way.
    distances: Vec<f32>,
}

impl ShoreField {
    /// The field for ground under `water_level`, or `None` if no ground is under it.
    pub fn bake(heights: &HeightField, water_level: f64) -> Option<Self> {
        let lowest = heights.heights().iter().copied().min().map_or(0.0, |low| f64::from(low) * heights.units_per_step());
        if lowest >= water_level {
            return None;
        }
        let cells = heights.samples() - 1;
        let side = (cells * 2 + 1).min(MAX_SIDE);
        let map = cells as f64 * heights.cell_units();
        let spacing = map / (side - 1) as f64;
        // Height above the water at each texel.
        let mut above = vec![0.0_f32; side * side];
        for y in 0..side {
            for x in 0..side {
                above[y * side + x] = (heights.height_at(x as f64 * spacing, y as f64 * spacing) - water_level) as f32;
            }
        }
        let spacing = spacing as f32;
        let at = |x: usize, y: usize| above[y * side + x];
        // Texels beside the edge start at their distance to it, from the slope across them; the
        // rest start out of range and take the nearest distance through their neighbours.
        let mut distances = vec![f32::INFINITY; side * side];
        for y in 0..side {
            for x in 0..side {
                let here = at(x, y);
                let neighbours = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)];
                let crosses = neighbours.iter().any(|&(nx, ny)| nx < side && ny < side && (at(nx, ny) < 0.0) != (here < 0.0));
                if crosses {
                    let left = at(x.saturating_sub(1), y);
                    let right = at((x + 1).min(side - 1), y);
                    let down = at(x, y.saturating_sub(1));
                    let up = at(x, (y + 1).min(side - 1));
                    let slope = ((right - left).powi(2) + (up - down).powi(2)).sqrt() / (2.0 * spacing);
                    distances[y * side + x] = (here.abs() / slope.max(1e-4)).min(spacing);
                }
            }
        }
        chamfer(&mut distances, side, spacing);
        for (distance, height) in distances.iter_mut().zip(&above) {
            let unsigned = distance.min(SHORE_RANGE);
            *distance = if *height < 0.0 { unsigned } else { -unsigned };
        }
        Some(Self { side, distances })
    }

    pub const fn side(&self) -> usize {
        self.side
    }

    /// The signed distance at a texel.
    pub fn distance(&self, x: usize, y: usize) -> f32 {
        self.distances[y * self.side + x]
    }

    /// One byte a texel for the graphics card: 0 at `SHORE_RANGE` inland, 255 at `SHORE_RANGE`
    /// out to sea, and the edge itself halfway.
    pub fn bytes(&self) -> Vec<u8> {
        self.distances.iter().map(|distance| ((distance / SHORE_RANGE * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8).collect()
    }
}

/// Spreads distances from texels that have them to those that do not, in two passes over the
/// grid, each texel taking the least of its own and a neighbour's plus the step to it.
fn chamfer(distances: &mut [f32], side: usize, spacing: f32) {
    let diagonal = spacing * std::f32::consts::SQRT_2;
    let forward = [(-1, 0, spacing), (0, -1, spacing), (-1, -1, diagonal), (1, -1, diagonal)];
    let backward = [(1, 0, spacing), (0, 1, spacing), (1, 1, diagonal), (-1, 1, diagonal)];
    let mut pass = |order: &mut dyn Iterator<Item = (usize, usize)>, steps: &[(isize, isize, f32)]| {
        for (x, y) in order {
            let mut best = distances[y * side + x];
            for &(dx, dy, step) in steps {
                let (nx, ny) = (x as isize + dx, y as isize + dy);
                if nx >= 0 && ny >= 0 && (nx as usize) < side && (ny as usize) < side {
                    best = best.min(distances[ny as usize * side + nx as usize] + step);
                }
            }
            distances[y * side + x] = best;
        }
    };
    pass(&mut (0..side).flat_map(|y| (0..side).map(move |x| (x, y))), &forward);
    pass(&mut (0..side).rev().flat_map(|y| (0..side).rev().map(move |x| (x, y))), &backward);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ground sloping up from west to east, one unit of height per unit east, crossing the
    /// water at x = 300, on 33 samples 20 units apart.
    fn ramp() -> HeightField {
        let samples = 33;
        let heights = (0..samples * samples).map(|index| (((index % samples) * 20) as i32 - 300) as i16).collect();
        HeightField::new(samples, heights, 20.0, 1.0)
    }

    fn distance_at(field: &ShoreField, x: f64) -> f32 {
        let spacing = 640.0 / (field.side() - 1) as f64;
        field.distance((x / spacing).round() as usize, field.side() / 2)
    }

    #[test]
    fn distances_are_positive_out_to_sea_negative_inland_and_grow_away_from_the_edge() {
        let field = ShoreField::bake(&ramp(), 0.0).expect("there is water");
        assert_eq!(field.side(), 65);
        for (x, expected) in [(200.0, 100.0), (280.0, 20.0), (300.0, 0.0), (340.0, -40.0), (400.0, -100.0)] {
            let distance = distance_at(&field, x);
            assert!((distance - expected).abs() <= 2.0, "at x = {x}, {distance} should be about {expected}");
        }
        assert_eq!(distance_at(&field, 0.0), SHORE_RANGE, "far out to sea it holds the range");
    }

    #[test]
    fn a_map_with_no_ground_under_the_water_has_no_field() {
        assert!(ShoreField::bake(&ramp(), -1_000.0).is_none());
    }

    #[test]
    fn the_edge_is_halfway_in_the_bytes() {
        let field = ShoreField::bake(&ramp(), 0.0).unwrap();
        let bytes = field.bytes();
        let side = field.side();
        let row = &bytes[side / 2 * side..side / 2 * side + side];
        assert_eq!(row[0], 255);
        assert_eq!(row[side - 1], 0);
        assert!((i32::from(row[30]) - 128).abs() <= 2, "the edge at x = 300 is texel 30: {}", row[30]);
    }
}
