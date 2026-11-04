//! Arrival formations: where each unit in a group order ends up. Integer math only.

use super::components::SimId;
use crate::math::fp::{Fp, Vec2Fp, FP_FRAC_BITS};

/// The grid for a group order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formation {
    /// Each unit's spot, in the order the units were given.
    pub slots: Vec<Vec2Fp>,
    /// Distance from the goal at which units leave the shared route for their spot.
    pub approach_radius: Fp,
}

/// The number of columns for a grid about twice as wide as it is deep.
fn column_count(units: usize) -> usize {
    let doubled = units as u64 * 2;
    let root = doubled.isqrt();
    (root + u64::from(root * root < doubled)).max(1) as usize
}

/// Lays out a grid centered on `goal` and facing the direction of travel. The units furthest
/// forward take the front rows, and each row is filled from side to side in the order the units
/// already stand, so that paths cross as little as possible. Ties go to the lowest id, and the
/// result does not depend on the order the units were given in.
pub fn plan_formation(units: &[(SimId, Vec2Fp)], goal: Vec2Fp, spacing: Fp) -> Formation {
    let count = units.len() as i64;
    let center = units.iter().fold((0i64, 0i64), |sum, (_, position)| {
        (sum.0 + i64::from(position.x.to_bits()), sum.1 + i64::from(position.y.to_bits()))
    });
    let center = Vec2Fp::new(
        Fp::from_bits((center.0 / count.max(1)) as i32),
        Fp::from_bits((center.1 / count.max(1)) as i32),
    );
    let travel = (goal - center).normalized();
    let forward = if travel == Vec2Fp::ZERO { Vec2Fp::new(Fp::ZERO, Fp::ONE) } else { travel };
    let (fx, fy) = (i64::from(forward.x.to_bits()), i64::from(forward.y.to_bits()));
    // To the right of the direction of travel.
    let (rx, ry) = (fy, -fx);
    let along = |position: Vec2Fp, ax: i64, ay: i64| {
        i64::from((position.x - center.x).to_bits()) * ax + i64::from((position.y - center.y).to_bits()) * ay
    };

    let columns = column_count(units.len());
    let rows = units.len().div_ceil(columns);

    // Sort everyone from front to back, then fill rows from left to right.
    let mut order: Vec<usize> = (0..units.len()).collect();
    order.sort_unstable_by_key(|&index| (-along(units[index].1, fx, fy), units[index].0));
    let mut slots = vec![goal; units.len()];
    let spacing_raw = i64::from(spacing.to_bits());
    for (row, row_units) in order.chunks_mut(columns).enumerate() {
        row_units.sort_unstable_by_key(|&index| (along(units[index].1, rx, ry), units[index].0));
        // In half-spacings, so that even counts stay centered without rounding.
        let depth = (rows as i64 - 1) - 2 * row as i64;
        let width = row_units.len() as i64;
        for (column, &index) in row_units.iter().enumerate() {
            let side = 2 * column as i64 - (width - 1);
            let offset = |forward_axis: i64, right_axis: i64| {
                ((forward_axis * depth + right_axis * side) * spacing_raw / 2) >> FP_FRAC_BITS
            };
            slots[index] = Vec2Fp::new(
                Fp::from_bits(goal.x.to_bits().saturating_add(offset(fx, rx) as i32)),
                Fp::from_bits(goal.y.to_bits().saturating_add(offset(fy, ry) as i32)),
            );
        }
    }
    let reach = (columns as i64 + rows as i64) * spacing_raw / 2 + 4 * spacing_raw;
    Formation { slots, approach_radius: Fp::from_bits(reach as i32) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::fp::{fp_int, sqrt_wide};

    fn point(x: i32, y: i32) -> Vec2Fp {
        Vec2Fp::new(fp_int(x), fp_int(y))
    }

    fn line_of_units(count: u32) -> Vec<(SimId, Vec2Fp)> {
        (0..count).map(|id| (SimId(id), point(id as i32 * 10, 0))).collect()
    }

    #[test]
    fn the_grid_is_wider_than_it_is_deep() {
        assert_eq!(column_count(1), 2);
        assert_eq!(column_count(2), 2);
        assert_eq!(column_count(8), 4);
        assert_eq!(column_count(50), 10);
        assert_eq!(column_count(5_000), 100);
    }

    #[test]
    fn slots_are_centered_on_the_goal_and_evenly_spaced() {
        let units = line_of_units(16);
        let goal = point(1_000, 1_000);
        let spacing = fp_int(20);
        let formation = plan_formation(&units, goal, spacing);
        assert_eq!(formation.slots.len(), 16);
        let count = formation.slots.len() as i32;
        let mean = |value: fn(&Vec2Fp) -> Fp| formation.slots.iter().map(|slot| value(slot).to_num::<i32>()).sum::<i32>() / count;
        assert!((mean(|slot| slot.x) - 1_000).abs() <= 1, "off center in x");
        assert!((mean(|slot| slot.y) - 1_000).abs() <= 1, "off center in y");
        for (index, slot) in formation.slots.iter().enumerate() {
            for other in &formation.slots[index + 1..] {
                let distance = sqrt_wide(slot.distance_squared(other));
                assert!(distance >= fp_int(19), "two slots are only {distance} apart");
            }
        }
    }

    #[test]
    fn the_formation_lines_up_across_the_direction_of_travel() {
        // Moving along +Y, from a block far below the goal.
        let units: Vec<_> = (0..8).map(|id| (SimId(id), point(id as i32 * 10, id as i32 % 2 * 10))).collect();
        let goal = point(35, 2_000);
        let formation = plan_formation(&units, goal, fp_int(20));
        let span = |value: fn(&Vec2Fp) -> Fp| {
            let values: Vec<i32> = formation.slots.iter().map(|slot| value(slot).to_num()).collect();
            values.iter().max().unwrap() - values.iter().min().unwrap()
        };
        assert!(span(|slot| slot.x) > span(|slot| slot.y), "the wide side should face the direction of travel");
    }

    #[test]
    fn units_keep_their_side_so_that_paths_do_not_cross() {
        // Few enough to fit one row, heading straight up.
        let units: Vec<_> = (0..3).map(|id| (SimId(id), point(id as i32 * 30, 0))).collect();
        let formation = plan_formation(&units, point(30, 3_000), fp_int(10));
        let xs: Vec<Fp> = formation.slots.iter().map(|slot| slot.x).collect();
        assert!(xs.windows(2).all(|pair| pair[0] < pair[1]), "left to right should stay left to right: {xs:?}");
    }

    #[test]
    fn the_unit_furthest_forward_takes_the_front() {
        let units = [(SimId(1), point(0, 500)), (SimId(2), point(0, 0)), (SimId(3), point(10, 250)), (SimId(4), point(10, 0)), (SimId(5), point(0, 300))];
        let formation = plan_formation(&units, point(0, 2_000), fp_int(10));
        let lead = formation.slots[0];
        let last = formation.slots[1];
        assert!(lead.y > last.y, "the leader should end up ahead: {lead:?} {last:?}");
    }

    #[test]
    fn the_result_does_not_depend_on_input_order() {
        let units = line_of_units(12);
        let mut reversed = units.clone();
        reversed.reverse();
        let goal = point(500, 900);
        let forward = plan_formation(&units, goal, fp_int(15));
        let backward = plan_formation(&reversed, goal, fp_int(15));
        for (index, &(id, _)) in units.iter().enumerate() {
            let other = reversed.iter().position(|(candidate, _)| *candidate == id).unwrap();
            assert_eq!(forward.slots[index], backward.slots[other]);
        }
    }

    #[test]
    fn stacked_units_and_a_goal_at_the_center_are_still_valid() {
        let units = [(SimId(1), point(100, 100)), (SimId(2), point(100, 100))];
        let formation = plan_formation(&units, point(100, 100), fp_int(10));
        assert_ne!(formation.slots[0], formation.slots[1]);
    }
}
