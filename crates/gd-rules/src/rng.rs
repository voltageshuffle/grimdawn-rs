//! The engine's random stream (`GAME::RandomUniform`), shared by every roll
//! site: combat, loot, proxies, the Shattered Realm generator and the story
//! Lua bindings.

/// `GAME::RandomUniform`: Park–Miller minimal standard generator
/// (a = 16807, m = 2^31 - 1, Schrage with q = 127773, r = 2836).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng(pub u32);

impl Rng {
    // The engine's name for the step; `Rng` is no iterator.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u32 {
        let s = self.0;
        let t = 16807u32
            .wrapping_mul(s % 127_773)
            .wrapping_sub(2836u32.wrapping_mul(s / 127_773)) as i32;
        let t = if t < 0 {
            t.wrapping_add(0x7FFF_FFFF)
        } else {
            t
        };
        self.0 = t as u32;
        self.0
    }
    /// `(float)(int)next * 4.6566129e-10` — [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next() as i32) as f32 * 4.656_613e-10
    }
}
