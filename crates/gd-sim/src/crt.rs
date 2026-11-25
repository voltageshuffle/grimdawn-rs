//! The C runtime's `rand()` (MSVC: `x = x * 214013 + 2531011`, returns `(x >> 16) & 0x7fff`; `srand` default 1).
//! The engine calls it, not its own generators, for the water ripple set, the fire lights' flicker and pulse and the
//! SSAO rotation texture (`GraphicsCanvas::CreateSsaoRotationTexture`).

#[derive(Clone, Debug)]
pub struct CrtRand(pub u32);

impl Default for CrtRand {
    fn default() -> Self {
        Self(1)
    }
}

impl CrtRand {
    pub fn rand(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        (self.0 >> 16) & 0x7fff
    }

    /// `rand() * 0.00003051851f` (1 / 32767).
    pub fn unit(&mut self) -> f32 {
        self.rand() as f32 * 0.000_030_518_51
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rand_matches_msvc() {
        let mut r = CrtRand::default();
        assert_eq!(
            [(); 10].map(|_| r.rand()),
            [
                41, 18467, 6334, 26500, 19169, 15724, 11478, 29358, 26962, 24464
            ]
        );
        assert_eq!(r.0, 3_750_785_579);
    }
}
