//! Monsters aiming projectiles ahead of a moving target (`LeadChance`).
//!
//! Game.dll:
//! * `ControllerMonster::Load` @0x180135e60: `LeadChance` is an int list of
//!   percentages, one per difficulty (+0x404..0x40C, ×0.01); one entry
//!   serves all three, two give Elite the second (`ControllerMonster`
//!   constructor: 0 without the field). `ControllerMonster::LeadTarget`
//!   @0x18013b660: a uniform draw in [0, 1] ≤ the session difficulty's
//!   chance.
//! * `ControllerMonsterStateAttack::AttackEnemyOrReturn` @0x180142290 rolls
//!   `LeadTarget` for every skill use (after the JumpAttack branch); on a
//!   hit it takes the projectile of the attack (a ranged weapon's when the
//!   monster wields one, else the skill profile's `skillProjectileName` at
//!   its level). None: the skill is used on the target as usual. Else it
//!   creates the projectile object to read `projectileVelocity` (+0x534,
//!   default 0) × (1 + its +0xCB0 % / 100) and, for a `SkillActivated`
//!   skill, a delay = 0.3 × the Attack animation's `GetAnimSpeed` (its mean
//!   duration in seconds, [`delay`]) / the monster's attack speed. The target's velocity is the direction from it to its
//!   path target (`Character::GetPathTarget`, none: no velocity) × its
//!   current speed (`Character::GetSpeed`). The intercept
//!   (`gd_sim::aim::intercept`, sub_1805873C0) from the monster's position
//!   is used when there is one, the target moves and the projectile is no
//!   `ProjectileHoming` (or its `ProjectileChain` child, the RTTI chain): the
//!   aim point is the intercept, pulled in to `Skill::GetRange` from the
//!   monster when farther, and the skill is used at that point (the target
//!   kept as its target).
//!
//! PORT: a fresh projectile's +0xCB0 is taken as 0 (the speed is the
//! record's `projectileVelocity`); the target's path target is the end of
//! the player's `MovePath` or a pet's path, its speed the run / walk speed
//! while running / walking (0 otherwise).
//!
//! `AttackEnemyOrReturn` @0x180142290 (second read): a monster
//! holding a weapon (the +2128 equipment's main hand, else off hand) takes
//! `Weapon::GetProjectileName` for the lead whatever the skill; so a ranged
//! weapon's `basicProjectileName` is led like a skill's projectile, and the
//! shot flies at the led point ([`crate::casting::AimPoint`], not re-aimed at
//! release). PORT: the plain swing's range is the weapon's reach and its
//! delay is the rule above.
use crate::character::{CharState, Character};
use bevy::prelude::*;
use gd_formats::arz::Record;

/// `LeadChance` per difficulty (0..1).
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

/// `LeadTarget`: the draw for this use of a skill.
/// PORT: no draw for a zero chance (the engine's draw could be exactly 0).
pub fn roll(chances: &[f32; 3], difficulty: usize) -> bool {
    let c = chances[difficulty.min(2)];
    c > 0.0 && crate::rng::f32() <= c
}

/// The target's velocity (`GetPathTarget` direction × `GetSpeed`).
pub fn velocity(pos: Vec3, path_target: Option<Vec3>, ch: Option<&Character>) -> Vec3 {
    let speed = match ch {
        Some(c) if c.state == CharState::Run => crate::combat::run_speed(c),
        Some(c) if c.state == CharState::Walk => crate::combat::walk_speed(c),
        _ => 0.0,
    };
    path_target.map_or(Vec3::ZERO, |p| (p - pos).normalize_or_zero() * speed)
}

/// The launch delay: 0.3 × `GetAnimSpeed` of the Attack animation slot
/// / attack speed. `AttackEnemyOrReturn` @0x180142290 and
/// `AnimationSet::GetAnimSpeed`: the call is the `AnimationSet` slot-17
/// (Attack) object's vtable +80, which `gd_sim::anim::AnimSlot::mean_duration`
/// is (seconds: frames / fps / record speed, mean of the variants; 1.0 for an
/// empty slot), as `CalculateSkillDps` uses it.
pub fn delay(
    data: &crate::data::GameData,
    cache: &mut crate::character::AnimCache,
    ch: &Character,
) -> f32 {
    let slot = ch.anims.slot(gd_sim::anim::AnimType::Attack);
    let cache = std::cell::RefCell::new(cache);
    let dur = slot.mean_duration(|p| {
        cache
            .borrow_mut()
            .anim(data, p)
            .map(|a| (a.num_frames, a.frame_rate))
    });
    dur / ch.attack_speed.max(0.01) * 0.3
}

/// The aim point of `skill` (level index `li`) used from `me` at a target
/// at `target` moving at `vel`; None: aim at the target.
pub fn aim(
    data: &crate::data::GameData,
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

/// [`aim`] for the projectile record `name` (a ranged weapon's
/// `basicProjectileName`, or the skill's).
pub fn aim_projectile(
    data: &crate::data::GameData,
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
