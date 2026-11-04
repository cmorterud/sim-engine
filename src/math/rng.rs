use super::fp::Fp;

/// Seeded SplitMix64 generator owned by `SimWorld`. All simulation randomness comes from here.
#[derive(Clone, Debug)]
pub struct SimRng {
    state: u64,
}

impl SimRng {
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub const fn state(&self) -> u64 {
        self.state
    }

    pub fn set_state(&mut self, state: u64) {
        self.state = state;
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform value in `0..bound`. `bound` must be non-zero.
    pub fn below(&mut self, bound: u32) -> u32 {
        (((self.next_u64() >> 32) * bound as u64) >> 32) as u32
    }

    /// Uniform value in `min..max`.
    pub fn range_fp(&mut self, min: Fp, max: Fp) -> Fp {
        let span = (max.to_bits() - min.to_bits()) as u32;
        Fp::from_bits(min.to_bits() + self.below(span) as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::fp::fp_int;

    #[test]
    fn same_seed_gives_same_sequence() {
        let (mut a, mut b) = (SimRng::new(42), SimRng::new(42));
        for _ in 0..1_000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    /// Pins the algorithm: changing it would silently change every generated map.
    #[test]
    fn known_first_output() {
        assert_eq!(SimRng::new(0).next_u64(), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn range_stays_in_bounds() {
        let mut rng = SimRng::new(7);
        for _ in 0..10_000 {
            let value = rng.range_fp(fp_int(-5), fp_int(5));
            assert!(value >= fp_int(-5) && value < fp_int(5));
        }
    }
}
