use super::fp::{Fp, Vec2Fp};

/// Fixed-point 2D axis-aligned box. `min` is inclusive and `max` is exclusive.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RectFp {
    pub min: Vec2Fp,
    pub max: Vec2Fp,
}

impl RectFp {
    pub const fn new(min: Vec2Fp, max: Vec2Fp) -> Self {
        Self { min, max }
    }

    pub fn contains(&self, point: Vec2Fp) -> bool {
        point.x >= self.min.x && point.x < self.max.x && point.y >= self.min.y && point.y < self.max.y
    }

    pub fn width(&self) -> Fp {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> Fp {
        self.max.y - self.min.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::fp::fp_int;

    #[test]
    fn contains_includes_min_and_excludes_max() {
        let rect = RectFp::new(Vec2Fp::new(fp_int(1), fp_int(1)), Vec2Fp::new(fp_int(3), fp_int(3)));
        assert!(rect.contains(Vec2Fp::new(fp_int(1), fp_int(1))));
        assert!(!rect.contains(Vec2Fp::new(fp_int(3), fp_int(2))));
        assert_eq!(rect.width(), fp_int(2));
    }
}
