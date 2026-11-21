//! Per-stat randomisation ("jitter") of item attributes. Every variant draws
//! from the item's Park–Miller stream ([`Rng`], the same `RandomUniform` the
//! combat code uses). All arithmetic is f32 in the engine's order.
use crate::combat::Rng;
use crate::stats::PCT;

/// The item's attribute stream: `ItemEquipment::InitializeItem` (0x18033c240)
/// advances the saved seed once (`16807*s - 0x7fffffff*(s/127773)`, i.e. one
/// Schrage step) and hands that state to every attribute store.
pub fn item_stream(seed: u32) -> Rng {
    let mut r = Rng(seed);
    r.next();
    r
}

/// `DefenseAttribute::Jitter` (0x18018aa50; shared by every damage,
/// retaliation and defense attribute class) and, with `skip_zero_pct`,
/// `CharAttribute::Jitter`, which additionally returns `v` untouched when the
/// percentage is 0:
/// `n = (int)((v*pct)*0.01)`, at least 1; `r = next`; `v' = (int)(r % (2n+1) - n) + v`
/// (unsigned arithmetic), keeping `v` when `|v'| < 1`. No draw when `v == 0`.
pub fn jitter_int(v: f32, pct: f32, rng: &mut Rng, skip_zero_pct: bool) -> f32 {
    if v == 0.0 || (skip_zero_pct && pct == 0.0) {
        return v;
    }
    let mut n = ((v * pct) * PCT) as i32;
    if n == 0 {
        n = 1;
    }
    let m = n.wrapping_mul(2).wrapping_add(1) as u32;
    let m = if m == 0 { 1 } else { m };
    let r = rng.next();
    let d = (n as u32).wrapping_neg().wrapping_add(r % m) as i32 as f32 + v;
    if d.abs() < 1.0 { v } else { d }
}

/// `SkillAttribute::Jitter`: like [`jitter_int`] but without the
/// "at least 1" rule (a span of 0 still draws) and no percentage check.
pub fn jitter_skill(v: f32, pct: f32, rng: &mut Rng) -> f32 {
    if v == 0.0 {
        return v;
    }
    let n = ((v * pct) * PCT) as i32;
    let r = rng.next();
    let m = if n.wrapping_mul(2) != -1 {
        n.wrapping_mul(2).wrapping_add(1) as u32
    } else {
        1
    };
    let d = (r % m).wrapping_sub(n as u32) as i32 as f32 + v;
    if d.abs() < 1.0 { v } else { d }
}

/// Conversion percentages (`sub_18017C6F0`): continuous,
/// `v * (1 + unit*2p - p)` clamped to [0, 100], one draw per value.
// `max().min()`, not `clamp`: a NaN becomes 0.
#[allow(clippy::manual_clamp)]
pub fn jitter_continuous(v: f32, pct: f32, rng: &mut Rng) -> f32 {
    let p = pct * PCT;
    let lo = -p;
    let span = p - lo;
    let r = rng.next() as i32 as f32;
    ((((r * 4.656_613e-10) * span) + lo + 1.0) * v)
        .max(0.0)
        .min(100.0)
}

/// `MinJitter` / `MaxJitter` (`max`) of one value: the smallest / largest value
/// the roll can give. `DefenseAttribute::MinJitter` / `MaxJitter`
/// @0x18018ab10 / @0x18018ab70 (COMDAT-shared by the damage, retaliation,
/// defense and skill classes) and `CharAttribute::MinJitter` / `MaxJitter`
/// @0x1800cc010 / @0x1800cc070 (`skip_zero_pct`: a 0 % keeps the value):
/// `n = (int)(v * pct * 0.01)` (1 when 0; negative for a negative value, so
/// the "min" of a negative value is the one nearer 0), `v -/+ n` only when its
/// magnitude stays >= 1, else `v`; 0 stays 0.
pub fn jitter_bound(v: f32, pct: f32, max: bool, skip_zero_pct: bool) -> f32 {
    if v == 0.0 || (skip_zero_pct && pct == 0.0) {
        return v;
    }
    let mut n = ((v * pct) * PCT) as i32;
    if n == 0 {
        n = 1;
    }
    let n = n as f32;
    let r = if max { v + n } else { v - n };
    if r.abs() >= 1.0 { r } else { v }
}

/// What a jitter site of the attribute loaders produces: the seeded roll (the
/// `_Equipment` stores) or the `_Min` / `_Max` stores' bound
/// (`ItemEquipment::InitializeItem` @0x18033c240 loads the same records into
/// three store sets; the `_Min` / `_Max` loaders call `MinJitter` /
/// `MaxJitter` where the equipment one draws `Jitter`).
pub enum Roll<'a> {
    Seeded(&'a mut Rng),
    Bound { max: bool },
}

impl Roll<'_> {
    /// [`jitter_int`] or [`jitter_bound`].
    pub fn int(&mut self, v: f32, pct: f32, skip_zero_pct: bool) -> f32 {
        match self {
            Roll::Seeded(rng) => jitter_int(v, pct, rng, skip_zero_pct),
            Roll::Bound { max } => jitter_bound(v, pct, *max, skip_zero_pct),
        }
    }

    /// [`jitter_skill`], or the bound with the "at least 1" rule (
    /// `SkillAttribute::LoadPrefixTableMin` @0x18051cc30: no percentage check).
    pub fn skill(&mut self, v: f32, pct: f32) -> f32 {
        match self {
            Roll::Seeded(rng) => jitter_skill(v, pct, rng),
            Roll::Bound { max } => jitter_bound(v, pct, *max, false),
        }
    }

    /// A skill attribute's base value under the store's `AddJitter` (seeded:
    /// [`jitter_skill`] `SkillAttribute::AddJitter` @0x18051e670) or
    /// `AddMinJitter` / `AddMaxJitter` (the bound @0x18051e6f0:
    /// at least 1, kept when the result drops under 1). The caller clamps `pct`.
    pub fn skill_base(&mut self, v: f32, pct: f32) -> f32 {
        match self {
            Roll::Seeded(rng) => jitter_skill(v, pct, rng),
            Roll::Bound { max } => jitter_bound(v, pct, *max, false),
        }
    }

    /// [`jitter_continuous`], or `v * (1 -/+ pct/100)` clamped to [0, 100]
    /// (`sub_18017C810` / `sub_18017C880`, the conversion `_Min` /
    /// `_Max` stores).
    // `max().min()`, not `clamp`: a NaN becomes 0.
    #[allow(clippy::manual_clamp)]
    pub fn continuous(&mut self, v: f32, pct: f32) -> f32 {
        match self {
            Roll::Seeded(rng) => jitter_continuous(v, pct, rng),
            Roll::Bound { max } => {
                let p = pct * PCT;
                let f = if *max { p + 1.0 } else { 1.0 - p };
                (f * v).max(0.0).min(100.0)
            }
        }
    }

    /// One unused draw (the seeded stream only).
    pub fn skip(&mut self) {
        if let Roll::Seeded(rng) = self {
            rng.next();
        }
    }

    pub fn seeded(&self) -> bool {
        matches!(self, Roll::Seeded(_))
    }
}

/// `CharAttribute/DamageAttribute::Scale`: `(int)((v*pct)*0.01) + v`.
pub fn scale(v: f32, pct: f32) -> f32 {
    (((v * pct) * PCT) as i32) as f32 + v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_and_jitter() {
        // One Schrage step from seed 1 is 16807.
        assert_eq!(item_stream(1).0, 16807);
        // 10% of 10 = 1 -> span 3 values {9,10,11}; r = 16807*16807 mod m.
        let mut r = Rng(16807);
        let v = jitter_int(10.0, 10.0, &mut r, false);
        assert_eq!(r.0, 282_475_249);
        assert_eq!(v, (282_475_249u32 % 3) as f32 - 1.0 + 10.0);
        // Zero values never draw.
        let mut r = Rng(5);
        assert_eq!(jitter_int(0.0, 20.0, &mut r, false), 0.0);
        assert_eq!(r.0, 5);
        // Char variant: pct 0 doesn't draw, defense variant still does (±1).
        let mut r = Rng(5);
        assert_eq!(jitter_int(7.0, 0.0, &mut r, true), 7.0);
        assert_eq!(r.0, 5);
        jitter_int(7.0, 0.0, &mut r, false);
        assert_ne!(r.0, 5);
        // Small values never collapse below 1.
        let mut r = Rng(1);
        for _ in 0..100 {
            let v = jitter_int(1.0, 50.0, &mut r, false);
            assert!(v == 1.0 || v == 2.0, "{v}");
        }
        assert_eq!(scale(17.0, 30.0), 22.0);
        let mut r = Rng(1);
        let c = jitter_continuous(50.0, 20.0, &mut r);
        assert!((40.0..=60.0).contains(&c));
    }
}
