//! The character sheet's Damage Per Second: the per-skill DPS formulas of
//! `Player::CalculateSkillDps` and the `CalculateDPS` of the
//! skill classes (`SkillActivatedWeapon::CalculateDPS`, the
//! shared `SkillActivatedSpell` one). The damage
//! min / max come from [`crate::actor::display_damage`].

/// The `SkillAttackInfo` fields the formulas read.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Info {
    /// Damage min / max of the main (right) hand accumulator and the second
    /// (left hand) one (`SkillAttackInfo.minDamage` / `SkillAttackInfo.maxDamage`, two each).
    pub min: [f32; 2],
    pub max: [f32; 2],
    /// `SkillAttackInfo.dualWield`: a second weapon (not a shield or off-hand) is worn.
    pub dual: bool,
    /// `SkillAttackInfo.usesPerSecond`: uses per second ([`weapon_rate`], [`spell_rate`]).
    pub rate: f32,
    /// `SkillAttackInfo.cooldownReduction`: the skill's total cooldown reduction as a fraction
    /// (`GetTotalSkillAttribute` × 0.01).
    pub cooldown_reduction: f32,
    /// `SkillAttackInfo.cooldownTime`: `skillCooldownTime` of the skill with its modifiers.
    pub cooldown: f32,
    /// `skillActiveDuration` of the skill with its modifiers (spell formula).
    pub active_duration: f32,
}

/// `SkillActivatedWeapon::CalculateDPS`: with a cooldown both
/// hands' average over the reduced cooldown; otherwise one hand × the rate,
/// two hands 0.75 × each hand's average (first hand empty: the second's).
pub fn weapon_dps(i: &Info) -> f32 {
    let (min1, max1, min2, max2) = (i.min[0], i.max[0], i.min[1], i.max[1]);
    let (sum_min, sum_max) = (min1 + min2, max1 + max2);
    if i.cooldown > 0.0 && i.cooldown_reduction < 1.0 {
        return (sum_max + sum_min) * 0.5 / ((1.0 - i.cooldown_reduction) * i.cooldown);
    }
    let main = (max1 + min1) * 0.5 * i.rate;
    if !i.dual {
        return main;
    }
    if max1 <= 0.0 {
        return if max2 > 0.0 {
            (max2 + min2) * 0.5 * i.rate
        } else {
            main
        };
    }
    (((max1 + min1) + (max2 + min2)) * 0.125 + (sum_max + sum_min) * 0.25) * i.rate
}

/// The `CalculateDPS` of `SkillActivatedSpell` and the skills sharing its
/// code (projectiles, radius attacks, rings, orbiting projectiles):
/// the main hand's average × the rate, or, with a cooldown, over the reduced
/// cooldown (× the active duration when it is shorter than the cooldown;
/// the plain average when the duration is not).
pub fn spell_dps(i: &Info) -> f32 {
    let avg = (i.max[0] + i.min[0]) * 0.5;
    if i.cooldown <= 0.0 || i.cooldown_reduction >= 1.0 {
        return avg * i.rate;
    }
    let mut v = avg;
    if i.active_duration > 0.0 {
        if i.cooldown <= i.active_duration {
            return avg;
        }
        v *= i.active_duration;
    }
    v / ((1.0 - i.cooldown_reduction) * i.cooldown)
}

/// The end of `Player::CalculateSkillDps`: without a cooldown
/// (or a full cooldown reduction) the result loses 5 %.
pub fn finish(dps: f32, cooldown: f32, cooldown_reduction: f32) -> f32 {
    if cooldown <= 0.0 || cooldown_reduction >= 1.0 {
        dps - dps * 0.05
    } else {
        dps
    }
}

/// Weapon skills' uses per second (`SkillAttackInfo.usesPerSecond`, `CalculateSkillDps` +
/// `Player::CalculateAttackSpeed(1)`): the attack speed over the character's
/// base attack speed, over the current weapon set's attack animation
/// duration (`GetAnimSpeed` of its Attack slot: frames / fps / record speed,
/// mean of the variants). `attack_speed` is `Character::GetAttackSpeed`,
/// `base_attack_speed` the record's `characterAttackSpeed` × (1 + modifier %).
/// 0 when either divisor is not positive.
pub fn weapon_rate(attack_speed: f32, base_attack_speed: f32, attack_duration: f32) -> f32 {
    if base_attack_speed <= 0.0 || attack_duration <= 0.0 {
        return 0.0;
    }
    attack_speed / base_attack_speed / attack_duration
}

/// Spells' uses per second (`SkillAttackInfo.usesPerSecond`, `CalculateSkillDps`): the
/// spell cast speed × the frame rate over the length of the cast animation
/// (`GetHitCallbackPoint` returns `GetLength` and `GetFrameRate`); `None`
/// when the animation is missing.
pub fn spell_rate(cast_speed: f32, frames: u32, fps: u32) -> Option<f32> {
    (frames != 0 && fps != 0).then_some(fps as f32 / frames as f32 * cast_speed)
}
