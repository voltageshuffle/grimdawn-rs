//! How a character turns toward a direction, Bevy-free.
//!
//! Game.dll `Character::RotateTowards(dtMs, dir, pos)`: a direction shorter than 0.2 (or
//! zero) changes nothing and is not "reached"; a character whose record sets `disallowRotation` keeps its facing and
//! counts as reached; otherwise the direction is flattened (y dropped, renormalised), `c` = the cosine between it
//! and the facing (local +Z, clamped to [-1, 1]) and the turn this tick is
//! `step = (max + (min - max) * (c + 1) / 2) * mult * dtMs / 1000` radians: the full `maxRotationSpeed` when the
//! direction is behind, `minRotationSpeed` when it is ahead. An angle `acos(c)` under the step snaps the facing onto
//! the direction (reached); else the facing turns by the step about the world up, toward the side the direction is
//! on (positive when it lies on the local +X side or straight behind). The speeds are rad/s, read by
//! `Character::Load` from the character's own record (`minRotationSpeed` default 9,
//! `maxRotationSpeed` default 12; the player's `malepc01` / `femalepc01`: 10 and 40); the multiplier is
//! `Character::SetRotationSpeedMultiplier` (1 by default, [`speed_multiplier`]). The game engine record's
//! `min/maxRotationSpeed` and `min/maxPlayerRotationSpeed` are read (`GameEngine::LoadFromDatabase`) but their
//! getters have no caller: dead data.

/// A character's turn rates (rad/s), `minRotationSpeed` / `maxRotationSpeed`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotationSpeed {
    pub min: f32,
    pub max: f32,
}

impl Default for RotationSpeed {
    /// `Character::Load`'s defaults.
    fn default() -> Self {
        Self {
            min: 9.0,
            max: 12.0,
        }
    }
}

impl RotationSpeed {
    /// The record's fields, the engine's defaults for missing ones.
    pub fn from_fields(min: Option<f32>, max: Option<f32>) -> Self {
        let d = Self::default();
        Self {
            min: min.unwrap_or(d.min),
            max: max.unwrap_or(d.max),
        }
    }
}

/// `RotateTowards` does nothing for a direction shorter than this.
pub const MIN_DIRECTION: f32 = 0.2;

/// The outcome of one [`rotate_towards`] tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Turned {
    /// The direction is too short: nothing happened (`false`).
    Ignored,
    /// The facing yaw moved toward the direction and has not reached it (`false`).
    Turning(f32),
    /// The facing yaw is on the direction (`true`); `disallowRotation` keeps the old yaw.
    Reached(f32),
}

impl Turned {
    /// The new yaw, if any.
    pub fn yaw(self) -> Option<f32> {
        match self {
            Turned::Ignored => None,
            Turned::Turning(y) | Turned::Reached(y) => Some(y),
        }
    }
}

/// One tick of `Character::RotateTowards` (module doc) for a character whose facing is `yaw` (radians from +Z
/// toward +X, the facing `(sin yaw, 0, cos yaw)`), turning toward `dir` (any length) over `dt_ms`.
pub fn rotate_towards(
    yaw: f32,
    dir: [f32; 3],
    dt_ms: f32,
    speed: RotationSpeed,
    mult: f32,
    disallow: bool,
) -> Turned {
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if len == 0.0 || len < MIN_DIRECTION {
        return Turned::Ignored;
    }
    let flat = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt();
    if disallow || flat == 0.0 {
        return Turned::Reached(yaw);
    }
    let (dx, dz) = (dir[0] / flat, dir[2] / flat);
    let (fx, fz) = (yaw.sin(), yaw.cos());
    let c = (fx * dx + fz * dz).clamp(-1.0, 1.0);
    let (min, max) = (speed.min * mult, speed.max * mult);
    let step = ((min - max) * ((c + 1.0) * 0.5) + max) * dt_ms * 0.001;
    if c.acos() < step {
        return Turned::Reached(dx.atan2(dz));
    }
    // Local +X of a facing `yaw` is (cos yaw, 0, -sin yaw).
    let side = dx * yaw.cos() - dz * yaw.sin();
    let s = if side >= 0.0 { step } else { -step };
    Turned::Turning(wrap(yaw + s))
}

/// `yaw` in (-pi, pi].
pub fn wrap(yaw: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let y = (yaw + PI).rem_euclid(TAU) - PI;
    if y <= -PI { y + TAU } else { y }
}

/// The record classes whose `ActivateNow` sets the caster's rotation-speed multiplier to the skill's
/// `rotationSpeedMultiplier` (default 1, their `Load`) `Skill_AttackSpellBeam`,
/// `Skill_AttackSpellCone`, `Skill_AttackSpellDrain`, `Skill_AttackRadiusSpin`,
/// `Skill_AttackRadiusGrow`; their `StopSkill` (and the cone's / drain's `OnMoved`) puts
/// it back to 1.
pub fn speed_multiplier(class: &str, field: Option<f32>) -> Option<f32> {
    matches!(
        class,
        "Skill_AttackSpellBeam"
            | "Skill_AttackSpellCone"
            | "Skill_AttackSpellDrain"
            | "Skill_AttackRadiusSpin"
            | "Skill_AttackRadiusGrow"
    )
    .then(|| field.unwrap_or(1.0))
}
