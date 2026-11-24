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

/// `a * b` rounded toward zero, so that negating either factor negates the product exactly.
/// `Fp`'s own `*` rounds toward negative infinity: a unit heading the negative way along an axis
/// would move a fraction further each tick than its mirror image heading the positive way, and
/// on a mirrored map that hands one side a small, steady edge. Scaling a direction uses this.
pub fn mul_toward_zero(a: Fp, b: Fp) -> Fp {
    Fp::from_bits((i64::from(a.to_bits()) * i64::from(b.to_bits()) / (1 << FP_FRAC_BITS)) as i32)
}

/// The mean of some points, worked out as offsets from `about` and rounded toward it, so that on a
/// mirrored map mirror images have mirror-image means. Map positions are all positive, so their
/// plain mean rounded toward the map's corner on one side and toward the middle on the other, a
/// raw bit apart: a transport headed for the middle of its riders turned a hair one way on one
/// side and not on the other. `about` must be a mirror-image pair itself, as a goal or a unit's
/// own position is. `about` if there are no points.
pub fn mean_about(points: impl Iterator<Item = Vec2Fp>, about: Vec2Fp) -> Vec2Fp {
    let (mut x, mut y, mut count) = (0_i64, 0_i64, 0_i64);
    for point in points {
        x += i64::from(point.x.to_bits()) - i64::from(about.x.to_bits());
        y += i64::from(point.y.to_bits()) - i64::from(about.y.to_bits());
        count += 1;
    }
    if count == 0 {
        return about;
    }
    Vec2Fp::new(Fp::from_bits(about.x.to_bits() + (x / count) as i32), Fp::from_bits(about.y.to_bits() + (y / count) as i32))
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

    /// Each component times `factor`, rounded toward zero, so that mirror images scale alike.
    pub fn scaled(&self, factor: Fp) -> Self {
        Self::new(mul_toward_zero(self.x, factor), mul_toward_zero(self.y, factor))
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

    /// Each component times `factor`, rounded toward zero, so that mirror images scale alike.
    pub fn scaled(&self, factor: Fp) -> Self {
        Self::new(mul_toward_zero(self.x, factor), mul_toward_zero(self.y, factor), mul_toward_zero(self.z, factor))
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

/// The cosine and sine of an angle in tenths of a degree, from 0 to 900, using only
/// fixed-point arithmetic so that every machine gets the same bits.
pub fn cos_sin_tenths_of_degree(tenths: u32) -> (Fp, Fp) {
    assert!(tenths <= 900, "only angles up to a right angle are needed");
    cos_sin_radians(radians_from_tenths_of_degree(tenths))
}

/// An angle in tenths of a degree, in radians.
pub fn radians_from_tenths_of_degree(tenths: u32) -> FpWide {
    // Pi in 24 fractional bits.
    FpWide::from_bits(52_707_179) * i64::from(tenths) / 1_800
}

/// The cosine and sine of an angle in radians, from 0 to a right angle, from fixed-point arithmetic only.
pub fn cos_sin_radians(x: FpWide) -> (Fp, Fp) {
    let x2 = x * x;
    // Taylor series, with enough terms for a right angle to stay well within one Fp step.
    let sin = x * (FpWide::ONE - x2 / 6 * (FpWide::ONE - x2 / 20 * (FpWide::ONE - x2 / 42 * (FpWide::ONE - x2 / 72))));
    let cos = FpWide::ONE - x2 / 2 * (FpWide::ONE - x2 / 12 * (FpWide::ONE - x2 / 30 * (FpWide::ONE - x2 / 56)));
    let narrow = |value: FpWide| Fp::from_bits((value.to_bits() >> (24 - FP_FRAC_BITS)) as i32);
    (narrow(cos), narrow(sin))
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn cosine_and_sine_match_the_true_values_closely() {
        let step = 1.0 / 4096.0;
        for tenths in [0_u32, 50, 90, 180, 360, 450, 600, 900] {
            let (cos, sin) = cos_sin_tenths_of_degree(tenths);
            let angle = f64::from(tenths) / 10.0 * std::f64::consts::PI / 180.0;
            assert!((cos.to_num::<f64>() - angle.cos()).abs() <= step * 2.0, "cos of {tenths} tenths was {cos}");
            assert!((sin.to_num::<f64>() - angle.sin()).abs() <= step * 2.0, "sin of {tenths} tenths was {sin}");
        }
    }

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
    fn scaling_a_mirrored_direction_gives_the_mirrored_step() {
        // Fp's own product rounds toward negative infinity, so -a * b is one bit further from
        // zero than a * b. Rounding toward zero keeps mirror images exactly opposite.
        let (a, b) = (Fp::from_bits(3_001), Fp::from_bits(1_234));
        assert_eq!(mul_toward_zero(-a, b), -mul_toward_zero(a, b));
        assert_eq!(mul_toward_zero(a, -b), -mul_toward_zero(a, b));
        assert_eq!(mul_toward_zero(a, b), a * b);
        let direction = Vec2Fp::new(fp_int(3), fp_int(-4)).normalized();
        let mirror = Vec2Fp::new(-direction.x, -direction.y);
        let speed = Fp::lit("1.37");
        assert_eq!(mirror.scaled(speed), Vec2Fp::new(-direction.scaled(speed).x, -direction.scaled(speed).y));
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
