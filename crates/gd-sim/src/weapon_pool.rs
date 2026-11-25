//! Weapon pools: the `Skill_WPAttack*` skills that replace a weapon-pool
//! attack (the default attack, Fire Strike `Skill_WeaponPool_BasicAttack`,
//! Cadence `…_ChargedFinale`, Righteous Fervor `…_ChargedScaling`).
//!
//! `SkillActivatedWeaponPool::StartAction` asks
//! `SkillManager::SelectWeaponPoolSkill` for a pool skill and
//! runs that skill's action with the pool skill as its parent
//! (`Skill_WPAttack.parentSkillId`); `Skill_WPAttack` adds the parent's
//! damage and multiplies its weapon % by the parent's.
//! The selection ([`select`]): learned, available pool skills with
//! a chance weight (`Skill_WPAttack::GetChanceWeight`: `skillChanceWeight`
//! at the level; with the third expansion the modifier-summed total) below
//! 100 share one roll out of `max(sum, 100)`; the rest of the 100 goes to
//! the default attack, or, when there are skills weighing ≥ 100, to one of
//! them picked by weight. Availability = `Skill::SetAvailability`'s
//! weapon checks ([`qualifies`]).
use crate::equip::{HandState, WeaponType};
use crate::stats::level_value;
use gd_formats::arz::Record;
use gd_rules::Rng;

/// Dual-wield flags and their hand-state bits (
/// `SkillProfile::LoadProfile`: `dualWieldOnly` 0x1 melee,
/// `dualRangedOnly` 0x40, `dualRangedOrRanged2hOnly` 0x44,
/// `dualRangedOrAllRangedOnly` 0xC4, `shieldNoRanged` 0x22).
pub const DUAL_WIELD_FLAGS: [(&str, u32); 5] = [
    ("dualWieldOnly", 0x1),
    ("dualRangedOnly", 0x40),
    ("dualRangedOrRanged2hOnly", 0x44),
    ("dualRangedOrAllRangedOnly", 0xC4),
    ("shieldNoRanged", 0x22),
];

/// Weapon flags of `SkillProfile::LoadProfile` (field → `Weapon_Type`).
const WEAPON_FLAGS: [(&str, WeaponType); 16] = [
    ("Sword", WeaponType::Sword),
    ("Axe", WeaponType::Axe),
    ("Ranged2h", WeaponType::Ranged2h),
    ("Spear", WeaponType::Spear),
    ("Mace", WeaponType::Mace),
    ("Dagger", WeaponType::Dagger),
    ("Scepter", WeaponType::Scepter),
    ("Staff", WeaponType::Staff),
    ("Magical", WeaponType::Magical),
    ("Shield", WeaponType::Shield),
    ("Offhand", WeaponType::Offhand),
    ("Ranged1h", WeaponType::Ranged1h),
    ("Sword2h", WeaponType::Sword2h),
    ("Axe2h", WeaponType::Axe2h),
    ("Mace2h", WeaponType::Mace2h),
    ("Spear2h", WeaponType::Spear2h),
];

/// The modifier whose weapon list replaces a skill's (`overwriteBaseSkill`,
/// `Skill::GetQualifyingWeapons`): the first learned one (level
/// above 0) that sets it, else the last gear-granted one that does.
pub fn weapon_list_override<'a>(learned: &[&'a Record], gear: &[&'a Record]) -> Option<&'a Record> {
    learned
        .iter()
        .find(|r| r.bool("overwriteBaseSkill"))
        .or_else(|| gear.iter().rev().find(|r| r.bool("overwriteBaseSkill")))
        .copied()
}

/// Whether `rec` can be used with these weapons: `Skill::QualifyingWeapon`
/// (no flags, or either hand's type listed) and `QualifyingHandState`
/// (dual-wield flags against the hand state; `unarmedOnly` needs empty
/// hands). `list_from`: a modifier replacing the weapon list and
/// the dual-wield flags ([`weapon_list_override`], `GetQualifyingWeapons` /
/// `GetQualifyingDualWeapons`); `unarmedOnly` stays the skill's.
pub fn qualifies(
    rec: &Record,
    list_from: Option<&Record>,
    right: Option<WeaponType>,
    left: Option<WeaponType>,
) -> bool {
    let flags = list_from.unwrap_or(rec);
    let (mut listed, mut held) = (false, false);
    for (k, w) in WEAPON_FLAGS {
        if flags.bool(k) {
            listed = true;
            held |= Some(w) == right || Some(w) == left;
        }
    }
    if listed && !held {
        return false;
    }
    let hs = HandState::from_weapons(right, left) as u32;
    let dual = DUAL_WIELD_FLAGS
        .iter()
        .filter(|(k, _)| flags.bool(k))
        .fold(0, |m, (_, bits)| m | bits);
    let mut ok = true;
    if dual != 0 {
        if dual & 1 != 0 {
            ok = hs == 10;
        }
        if dual & 0x40 != 0 && hs != 11 {
            ok = false;
        }
        if dual & 0x22 == 0x22 {
            ok = hs == 12 || hs == 14;
        }
        if dual & 0x44 == 0x44 {
            ok = hs == 9 || hs == 11;
        }
        if dual & 0xC4 == 0xC4 {
            ok = hs <= 16 && (0x12B00 >> hs) & 1 != 0;
        }
        if dual & 0x41 == 0x41 {
            ok = hs == 10 || hs == 11;
        }
    }
    if rec.bool("unarmedOnly") {
        return ok && hs == 0;
    }
    ok
}

/// A pool skill's chance weight at a level index plus its modifiers'
/// (`Skill_WPAttack::GetChanceWeight`: with the third expansion the
/// weight is the skill-attribute total of the skill and its modifiers).
pub fn chance_weight(rec: &Record, li: usize, mods: &[(&Record, usize)]) -> u32 {
    let own = level_value(rec, "skillChanceWeight", li).unwrap_or(0.0);
    let m: f32 = mods
        .iter()
        .map(|(r, l)| level_value(r, "skillChanceWeight", *l).unwrap_or(0.0))
        .sum();
    (own + m).max(0.0) as u32
}

/// `SkillManager::SelectWeaponPoolSkill`: index into `weights` of the pool
/// skill to use, `None` = the default attack. Weights ≥ 100 are the
/// "replacing" skills that take the share left over by the rest.
pub fn select(weights: &[u32], rng: &mut Rng) -> Option<usize> {
    let (mut sum_a, mut sum_b) = (0u32, 0u32);
    for &w in weights {
        if w >= 100 {
            sum_b = sum_b.saturating_add(w);
        } else {
            sum_a = sum_a.saturating_add(w);
        }
    }
    // The normal skills (0 < w < 100) in order, then at `total` the default
    // attack or the replacing skill drawn here.
    let rest = 100i64 - sum_a as i64;
    let mut total = sum_a;
    let mut tail = None;
    if rest > 0 {
        total = sum_a + rest as u32;
        tail = if sum_b == 0 {
            Some(None)
        } else {
            let r = rng.next() % (sum_b + 1);
            first_reaching(weights, |w| w >= 100, r).map(Some)
        };
    }
    let r = rng.next() % (total + 1);
    match first_reaching(weights, |w| w > 0 && w < 100, r) {
        Some(i) => Some(i),
        None => tail.flatten(),
    }
}

/// First index of the weights kept by `keep` whose running sum reaches `r`.
fn first_reaching(weights: &[u32], keep: impl Fn(u32) -> bool, r: u32) -> Option<usize> {
    let mut c = 0u32;
    for (i, &w) in weights.iter().enumerate() {
        if keep(w) {
            c += w;
            if r <= c {
                return Some(i);
            }
        }
    }
    None
}
