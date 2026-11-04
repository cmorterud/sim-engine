use fixed::types::{I20F12, I40F24};
use std::ops::{Add, Sub};

/// Positions, velocities, and per-unit values.
pub type Fp = I20F12;
/// Exact product of two `Fp` values. Every squared length and dot product uses this.
pub type FpWide = I40F24;

/// Number of fractional bits in `Fp`.
pub const FP_FRAC_BITS: u32 = 12;

/// Builds an `Fp` from a whole number.
pub const fn fp_int(value: i32) -> Fp {
    Fp::from_bits(value << FP_FRAC_BITS)
}

pub fn wide_mul(a: Fp, b: Fp) -> FpWide {
    FpWide::from_bits(a.to_bits() as i64 * b.to_bits() as i64)
}

/// The raw bits of an `FpWide` have 24 fractional bits, so their integer square
/// root has 12 fractional bits: exactly the raw bits of an `Fp`.
pub fn sqrt_wide(value: FpWide) -> Fp {
    Fp::from_bits((value.to_bits() as u64).isqrt() as i32)
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Vec2Fp {
    pub x: Fp,
    pub y: Fp,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Vec3Fp {
    pub x: Fp,
    pub y: Fp,
    pub z: Fp,
}

impl Vec2Fp {
    pub const ZERO: Self = Self { x: Fp::ZERO, y: Fp::ZERO };

    pub const fn new(x: Fp, y: Fp) -> Self {
        Self { x, y }
    }

    pub fn length_squared(&self) -> FpWide {
        wide_mul(self.x, self.x) + wide_mul(self.y, self.y)
    }

    pub fn distance_squared(&self, other: &Self) -> FpWide {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        wide_mul(dx, dx) + wide_mul(dy, dy)
    }

    /// Returns `ZERO` for a zero-length vector; callers handle that case explicitly.
    pub fn normalized(&self) -> Self {
        let length = sqrt_wide(self.length_squared()).to_bits() as i64;
        if length == 0 {
            return Self::ZERO;
        }
        Self {
            x: Fp::from_bits((((self.x.to_bits() as i64) << FP_FRAC_BITS) / length) as i32),
            y: Fp::from_bits((((self.y.to_bits() as i64) << FP_FRAC_BITS) / length) as i32),
        }
    }
}

impl Vec3Fp {
    pub const ZERO: Self = Self { x: Fp::ZERO, y: Fp::ZERO, z: Fp::ZERO };

    pub const fn new(x: Fp, y: Fp, z: Fp) -> Self {
        Self { x, y, z }
    }

    pub fn xy(&self) -> Vec2Fp {
        Vec2Fp::new(self.x, self.y)
    }

    pub fn length_squared(&self) -> FpWide {
        wide_mul(self.x, self.x) + wide_mul(self.y, self.y) + wide_mul(self.z, self.z)
    }

    pub fn distance_squared(&self, other: &Self) -> FpWide {
        (*self - *other).length_squared()
    }
}

impl Add for Vec2Fp {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Vec2Fp {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Add for Vec3Fp {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Sub for Vec3Fp {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximum_map_distance_does_not_overflow() {
        let origin = Vec3Fp::ZERO;
        let corner = Vec3Fp::new(fp_int(81_920), fp_int(81_920), fp_int(81_920));
        let distance_squared = corner.distance_squared(&origin);
        assert_eq!(distance_squared, FpWide::from_num(3i64 * 81_920 * 81_920));
        // sqrt(3) * 81,920 is about 141,889.6.
        let distance = sqrt_wide(distance_squared);
        assert_eq!(distance.to_num::<i32>(), 141_889);
    }

    #[test]
    fn sqrt_wide_is_exact_on_perfect_squares() {
        for value in [0, 1, 2, 3, 7, 100, 724, 725, 4_096, 81_920, 141_889] {
            let fp = fp_int(value);
            assert_eq!(sqrt_wide(wide_mul(fp, fp)), fp, "value {value}");
        }
        let half = Fp::lit("0.5");
        assert_eq!(sqrt_wide(wide_mul(half, half)), half);
    }

    #[test]
    fn normalized_has_unit_length_within_rounding() {
        let cases = [(3, 4), (-3, 4), (81_920, 1), (1, 81_920), (-500, -500)];
        for (x, y) in cases {
            let unit = Vec2Fp::new(fp_int(x), fp_int(y)).normalized();
            let error = (unit.length_squared() - FpWide::ONE).abs();
            assert!(error < FpWide::lit("0.002"), "({x}, {y}) error {error}");
        }
    }

    #[test]
    fn normalized_zero_vector_is_zero() {
        assert_eq!(Vec2Fp::ZERO.normalized(), Vec2Fp::ZERO);
    }

    /// The unoptimized `cargo test` run relies on overflow failing loudly.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic]
    fn plain_fp_multiplication_overflow_panics_in_debug_builds() {
        let large = std::hint::black_box(fp_int(725));
        let _ = large * large;
    }
}
