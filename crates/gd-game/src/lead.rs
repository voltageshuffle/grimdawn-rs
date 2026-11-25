//! Monsters aiming projectiles ahead of a moving target (`LeadChance`).
//!
//! * `ControllerMonster::Load`: `LeadChance` is an int list of percentages, one per difficulty; one entry
//!   serves all three, two give Elite the second; 0 without the field. `ControllerMonster::LeadTarget`: a
//!   uniform draw in [0, 1] ≤ the session difficulty's chance.
//! * `ControllerMonsterStateAttack::AttackEnemyOrReturn` rolls `LeadTarget` for every skill use (after the
//!   JumpAttack branch); on a hit it takes the attack's projectile (a wielded weapon's
//!   `Weapon::GetProjectileName` whatever the skill, else the skill's `skillProjectileName` at its level). None:
//!   the skill is used on the target as usual. Else the speed is `projectileVelocity` × (1 +
//!   `Projectile.speedBonusPct` %) and, for a `SkillActivated` skill, the delay is [`delay`]. The target's
//!   velocity is the direction to its path target (`Character::GetPathTarget`) × `Character::GetSpeed`. The
//!   intercept (`ControllerMonster::ComputeIntercept`) is used when there is one, the target moves and the
//!   projectile is no `ProjectileHoming` / `ProjectileChain`: the skill is used at the intercept, pulled in to
//!   `Skill::GetRange`, the target kept. The shot flies at the led point, not re-aimed at release.
//!
//! PORT: a fresh projectile's `speedBonusPct` is taken as 0; the target's path target is the end of the
//! player's `MovePath` or a pet's path, its speed the run / walk speed while running / walking (0 otherwise);
//! a plain weapon swing's range is the weapon's reach.
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::character::{CharState, Character};

pub fn chances(ctl: Option<&Record>) -> [f32; 3] {
    let mut it = ctl
        .into_iter()
        .flat_map(|c| c.f32s_iter("LeadChance"))
        .map(|v| v as i32 as f32 * 0.01);
    let a = it.next().unwrap_or(0.0);
    let b = it.next().unwrap_or(a);
    let c = it.next().unwrap_or(b);
    [a, b, c]
}

/// `LeadTarget`.
/// PORT: no draw for a zero chance (the engine's draw could be exactly 0).
pub fn roll(chances: &[f32; 3], difficulty: usize) -> bool {
    let c = chances[difficulty.min(2)];
    c > 0.0 && gd_game_core::rng::f32() <= c
}

pub fn velocity(pos: Vec3, path_target: Option<Vec3>, ch: Option<&Character>) -> Vec3 {
    let speed = match ch {
        Some(c) if c.state == CharState::Run => crate::combat::run_speed(c),
        Some(c) if c.state == CharState::Walk => crate::combat::walk_speed(c),
        _ => 0.0,
    };
    path_target.map_or(Vec3::ZERO, |p| (p - pos).normalize_or_zero() * speed)
}

/// The launch delay: 0.3 × the Attack slot's `AnimSlot::GetMeanDuration` (`AnimationSet::GetAnimSpeed`) /
/// attack speed.
pub fn delay(
    data: &gd_game_core::data::GameData,
    cache: &mut gd_game_core::character::AnimCache,
    ch: &Character,
) -> f32 {
    let slot = ch.anims.slot(gd_sim::anim::AnimType::Attack);
    let dur = slot.mean_duration(|p| {
        gd_game_core::character::shared_anim(&cache.anims, data, p)
            .map(|a| (a.num_frames, a.frame_rate))
    });
    dur / ch.attack_speed.max(0.01) * 0.3
}

/// The aim point of `skill` (level index `li`) at a target moving at `vel`; None: aim at the target.
pub fn aim(
    data: &gd_game_core::data::GameData,
    skill: &Record,
    li: usize,
    me: Vec3,
    tv: (Vec3, Vec3),
    delay: f32,
    range: f32,
) -> Option<Vec3> {
    let name = skill
        .strs_iter("skillProjectileName")
        .nth(li)
        .or_else(|| skill.strs_iter("skillProjectileName").last())
        .filter(|n| !n.is_empty())?;
    aim_projectile(data, name, me, tv, delay, range)
}

/// [`aim`] for the projectile record `name`.
pub fn aim_projectile(
    data: &gd_game_core::data::GameData,
    name: &str,
    me: Vec3,
    (target, vel): (Vec3, Vec3),
    delay: f32,
    range: f32,
) -> Option<Vec3> {
    let proj = data.db.get(name)?;
    if vel == Vec3::ZERO || matches!(proj.class.as_str(), "ProjectileHoming" | "ProjectileChain") {
        return None;
    }
    let speed = proj.f32("projectileVelocity").unwrap_or(0.0);
    let p = Vec3::from(gd_sim::aim::intercept(
        target.to_array(),
        vel.to_array(),
        me.to_array(),
        speed,
        delay,
    )?);
    let d = p - me;
    Some(if d.length() > range {
        me + d.normalize() * range
    } else {
        p
    })
}
