//! What the character sheet, the skill tooltips and the attack rollovers show
//! of a profile's attacks: [`collect_damage`] on the display accumulator (no
//! rolls; a chance weighs its attribute).
use super::*;

/// One damage attribute of the sheet's accumulator: (kind, type, min, max,
/// base duration); kind 0 hit, 1 weapon, 2 damage over time.
pub type Shown = (u8, CombatType, f32, f32, f32);

fn shown_damage(rules: &Rules, atk: Attacker<'_>, skill: &Option<SkillUse>) -> Vec<Shown> {
    let mut rng = Rng(0);
    let (damage, _, _) = collect_damage(
        rules,
        atk,
        skill,
        &mut rng,
        &Debuffs::default(),
        true,
        false,
        0.0,
    );
    let mut v = Vec::new();
    for d in &damage {
        match d {
            combat::Damage::Weapon(w) => v.push((
                1,
                CombatType::PhysicalPierce,
                w.physical[1] + w.pierce[1],
                w.physical[2] + w.pierce[2],
                0.0,
            )),
            combat::Damage::Hit(h) if is_dot_damage(h.ty) => v.push((0, h.ty, h.min, h.max, 0.0)),
            combat::Damage::Dot(x) if is_dot_damage(x.ty) => {
                v.push((2, x.ty, x.dps[1], x.dps[2], x.base_duration))
            }
            _ => {}
        }
    }
    v
}

/// The character's own damage attributes as the sheet's page II rows read
/// them (`Player::CalculateDamageValues(true)`: the character's damage
/// without a skill, merged, modified, converted and modified again; then
/// `CollectDamageValues(type, duration)` sums each attribute's
/// `GetMinMaxDamage` with the "include" flag set): the weapon's
/// physical and pierce parts as type 2 / 4 hits (`CombatAttributeDamage_
/// BasePhysical::GetMinMaxDamage`), typed hits, life leech and % current
/// life (counted only with the flag, `CombatAttributeAbsDamage_LifeLeech`),
/// DoTs per second with their base duration.
pub fn sheet_damage<'a>(rules: &Rules, atk: impl Into<Attacker<'a>>) -> Vec<Shown> {
    let mut rng = Rng(0);
    let (damage, _, _) = collect_damage(
        rules,
        atk.into(),
        &None,
        &mut rng,
        &Debuffs::default(),
        true,
        false,
        0.0,
    );
    let mut v = Vec::new();
    for d in &damage {
        match d {
            combat::Damage::Weapon(w) => {
                v.push((1, CombatType::Physical, w.physical[1], w.physical[2], 0.0));
                v.push((1, CombatType::Pierce, w.pierce[1], w.pierce[2], 0.0));
            }
            combat::Damage::Hit(h) => v.push((0, h.ty, h.min, h.max, 0.0)),
            combat::Damage::Dot(x) => v.push((2, x.ty, x.dps[1], x.dps[2], x.base_duration)),
            combat::Damage::LifeLeech { min, max, .. } => {
                v.push((0, CombatType::LifeLeech, *min, *max, 0.0))
            }
            combat::Damage::PercentCurrentLife { min, max, .. } => {
                v.push((0, CombatType::PercentCurrentLife, *min, *max, 0.0))
            }
            combat::Damage::Reflex { .. } => {}
        }
    }
    v
}

/// The damage one attack shows on the character sheet, summed over damage
/// attributes: `(min, max)` after the modifiers, conversions and attribute
/// scaling (`Skill::CollectCombatParameters` as `Player::
/// CalculateSkillDps` runs it, then `CombatAttributeAccumulator::
/// GetMinMaxDamage` over the accumulator). The display accumulator
/// ([`collect_damage`] with `display`) rolls nothing against chances.
/// Hit damage and weapon damage add their min / max, a damage over time its
/// per-second min / max (`CombatAttributeDurDamage::GetMinMaxDamage` @
/// 0x1801144f0, not the duration); mana burn, disruption, life leech, %
/// current life, crowd control and debuffs count nothing (their
/// `GetMinMaxDamage` is the empty base one or needs the flag the display
/// passes as 0: `CombatAttributeAbsDamage_LifeLeech` / `_PercentLife` @
/// 0x1801186a0; the type 21 duration attribute likewise).
pub fn display_damage(rules: &Rules, atk: &Profile, skill: &Option<SkillUse>) -> (f32, f32) {
    shown_damage(rules, atk.into(), skill)
        .iter()
        .fold((0.0, 0.0), |(a, b), s| (a + s.2, b + s.3))
}

/// The damage lines a skill's own attributes show in its tooltip, scaled by
/// the main player (`DamageAttributeStore::ProcessText` @0x1801a6af0,
/// The skill's damage attributes go through a
/// `CombatDisplayAccumulator` with the same `Process` / modifier machinery
/// the character sheet uses, so the player's `offensive*Modifier` totals and
/// attribute scaling apply). The player's own flat damage (gear, weapon) is
/// not part of the skill's store: PORT: it is taken out of the profile here.
/// No conversions of the character apply and only the skill's own full ones
/// (`ProcessText`: the skill's `ConversionAttributeAccumulator`
/// (vtable +992, `Weigh`) turns an attribute only for a conversion of ≥ 100 %
/// (`ConvertCombatAttributeForDisplay`); the player then adds only its
/// modifiers (`+2128` vtable +128) before `ModifyDamage`).
/// Returns (kind: 0 hit / 2 damage over time, type, min, max).
pub fn skill_tooltip_damage(
    rules: &Rules,
    atk: &Profile,
    skill: SkillUse,
) -> Vec<(u8, CombatType, f32, f32)> {
    use crate::attributes::{AttrFamily as F, lookup};
    let mut p = atk.clone();
    p.physical = (0.0, 0.0);
    p.dual = None;
    let flat = |k: &String, _: &mut f32| {
        let field = k.split_once('@').map_or(k.as_str(), |(f, _)| f);
        !lookup(field).is_some_and(|d| {
            matches!(d.family, F::OffenseBase | F::Offense | F::OffenseDuration)
                && [d.min, d.max, d.chance, d.duration_min, d.duration_max]
                    .iter()
                    .flatten()
                    .any(|f| f.eq_ignore_ascii_case(field))
        })
    };
    p.totals.retain(flat);
    for g in &mut p.global_groups {
        g.all.retain(flat);
        g.one_of.iter_mut().for_each(|m| m.1.retain(flat));
    }
    // The clone shares the live profile's memoized cache; this filtered copy
    // needs its own (or it would read, or fill in, the unfiltered one).
    p.combat_cache = Arc::new(OnceLock::new());
    let mut rng = Rng(0);
    let (damage, _, _) = collect_damage(
        rules,
        (&p).into(),
        &Some(skill),
        &mut rng,
        &Debuffs::default(),
        true,
        true,
        0.0,
    );
    damage
        .iter()
        .filter_map(|d| match d {
            combat::Damage::Hit(h) if is_dot_damage(h.ty) => Some((0, h.ty, h.min, h.max)),
            combat::Damage::Dot(x) if is_dot_damage(x.ty) => Some((2, x.ty, x.dps[1], x.dps[2])),
            _ => None,
        })
        .collect()
}

/// The numbers of a skill's "N% Retaliation Damage (a - b)" line
/// (`Skill::CollectRetaliationDamage` @0x180498c00): with the
/// skill's `retaliationDamagePct` > 0 the display accumulator takes the
/// skill's own retaliation attributes and the character's (equipment,
/// skills, devotions) × that %, then the retaliation modifiers, the skill's
/// and the character's conversions (`Weigh`, any %) and `ModifyDamage`;
/// each attribute's `GetMinMaxValues` sums into (min, max), both
/// `floor(x + 0.5)`. Not scaled by attributes (retaliation damage has none).
/// The modifiers as in combat ([`retaliation_damage`]). PORT: damage over
/// time adds its per-second min / max.
pub fn display_retaliation(rules: &Rules, atk: &Profile, skill: &SkillUse) -> (f32, f32) {
    let pct = skill.retaliation_pct();
    if pct <= 0.0 {
        return (0.0, 0.0);
    }
    let get = |k: &str| atk.totals.get(k).copied().unwrap_or(0.0) + skill.f(k);
    let mut rng = Rng(0);
    let own = skill.conversions();
    let mut ret = retaliation_damage(
        atk,
        Some(skill),
        &get,
        pct * PCT,
        &mut rng,
        &[&own, &atk.conversions],
    );
    process_retaliation(
        &mut ret,
        &AttackerScaling {
            formulas: &rules.formulas,
            vars: CombatVars::default(),
        },
        &ReductionInfo::default(),
        DamageScale(0.0, 0.0),
    );
    let (mut lo, mut hi) = (0.0, 0.0);
    for d in &ret {
        match d {
            combat::Damage::Weapon(w) => {
                lo += w.physical[1] + w.pierce[1];
                hi += w.physical[2] + w.pierce[2];
            }
            combat::Damage::Hit(h) if is_dot_damage(h.ty) => {
                lo += h.min;
                hi += h.max;
            }
            combat::Damage::Dot(x) if is_dot_damage(x.ty) => {
                lo += x.dps[1];
                hi += x.dps[2];
            }
            _ => {}
        }
    }
    ((lo + 0.5).floor(), (hi + 0.5).floor())
}

/// [`display_damage`] of a dual wielder's two accumulators: the second
/// hand's has the attributes the first already holds taken out
/// (`CombatAttributeAccumulator::FilterSameSource` @0x18011b080, which
/// drop attributes of the second matching one of the first; matched here by
/// kind, type, values and duration), so only the off-hand weapon's own
/// damage is left.
pub fn display_damage_hands<'o>(
    rules: &Rules,
    main: &Profile,
    off: impl Into<Attacker<'o>>,
    skill: &Option<SkillUse>,
) -> ((f32, f32), (f32, f32)) {
    let a = shown_damage(rules, main.into(), skill);
    let mut pool = a.clone();
    let mut second = (0.0, 0.0);
    for s in shown_damage(rules, off.into(), skill) {
        match pool.iter().position(|p| *p == s) {
            Some(i) => {
                pool.swap_remove(i);
            }
            None => second = (second.0 + s.2, second.1 + s.3),
        }
    }
    (
        a.iter().fold((0.0, 0.0), |(x, y), s| (x + s.2, y + s.3)),
        second,
    )
}

/// The LMB / RMB rollover's damage lines of a skill (Grim
/// Dawn.exe `sub_140153300`): each hand's accumulator (`CalculateSkillDps`)
/// lists its damage types, instant ones (`GetDamageTypes(1)`) then
/// durations (2), with `GetMinMaxDamage(type, 0, kind)`; the right hand's
/// types first, each combined with the left's same (type, kind) by the
/// player (vtable +0x5F8: the lower min and the higher max of two non-zero
/// hands, as the damage rows), then the left's own; lines with a zero max
/// are dropped. `(duration, type, min, max)`; the weapon's damage counts as
/// its physical and pierce parts. `off`: a dual wielder's off-hand swing
/// ([`Profile::off_hand`]; its shared attributes filtered as in [`display_damage_hands`]). PORT:
/// the order within a hand is the attributes' order. The secondary skills'
/// lines are [`display_breakdown_with_secondaries`].
pub fn display_breakdown(
    rules: &Rules,
    main: &Profile,
    off: Option<Attacker<'_>>,
    skill: &Option<SkillUse>,
) -> Vec<(bool, CombatType, f32, f32)> {
    fn lines(shown: &[Shown]) -> Vec<(bool, CombatType, f32, f32)> {
        let mut out: Vec<(bool, CombatType, f32, f32)> = Vec::new();
        let mut add = |dot: bool, ty: CombatType, a: f32, b: f32| match out
            .iter_mut()
            .find(|l| l.0 == dot && l.1 == ty)
        {
            Some(l) => {
                l.2 += a;
                l.3 += b;
            }
            None => out.push((dot, ty, a, b)),
        };
        for s in shown {
            match s.0 {
                1 => {
                    // (physical, pierce) parts packed by `shown_damage_split`.
                    add(false, CombatType::Physical, s.2, s.3);
                }
                2 => add(true, s.1, s.2, s.3),
                _ => add(false, s.1, s.2, s.3),
            }
        }
        out.sort_by_key(|l| l.0);
        out
    }
    let a = shown_damage_split(rules, main.into(), skill);
    let right = lines(&a);
    let left = off.map(|off| {
        let mut pool = a.clone();
        let mut own = Vec::new();
        for s in shown_damage_split(rules, off, skill) {
            match pool.iter().position(|p| *p == s) {
                Some(i) => {
                    pool.swap_remove(i);
                }
                None => own.push(s),
            }
        }
        lines(&own)
    });
    let mut out = Vec::new();
    let mut left = left.unwrap_or_default();
    for (dot, ty, a, b) in right {
        let l = left
            .iter()
            .position(|l| l.0 == dot && l.1 == ty)
            .map(|i| left.remove(i));
        let (min, max) = match l {
            Some(l) if (l.2, l.3) != (0.0, 0.0) && (a, b) != (0.0, 0.0) => (a.min(l.2), b.max(l.3)),
            Some(l) if (a, b) == (0.0, 0.0) => (l.2, l.3),
            _ => (a, b),
        };
        out.push((dot, ty, min, max));
    }
    out.extend(left);
    out.retain(|l| l.3 != 0.0);
    out
}

/// A rollover / sheet damage line: (duration, type, min, max).
pub type DamageLine = (bool, CombatType, f32, f32);

/// [`display_breakdown`] of a skill plus its secondary skills' damage
/// (Grim Dawn.exe `sub_1401527E0`): for each skill of
/// `Skill::GetSecondarySkills` (the learned ones of the base skill's tree,
/// `Skill` +368, not gear-granted secondaries: see `skills`) with
/// `GetCurrentLevel() != 0`, `Player::CalculateSkillDps(secondary, …,
/// parent)` @0x1803e06d0 fills the accumulators and `sub_140153300` makes its
/// lines; each base line takes (adds min and max of) every secondary line
/// of the same (type, kind), the rest are appended after the base lines, in
/// their order. One list, no heading: the merged lines use the base's tags.
/// A secondary's `CalculateSkillDps` collects its own record's attributes
/// at its level with the parent's weapons (`SkillSecondary` branch: the
/// parent's `SkillActivated` vtable +1760 weapons, then vtable +1272 with no
/// hand count), so only the main hand's accumulator gets its damage: the
/// secondaries take no `off` profile. `secondaries`: one use each, its
/// record the secondary itself (not its `buffSkillName`).
pub fn display_breakdown_with_secondaries(
    rules: &Rules,
    main: &Profile,
    off: Option<Attacker<'_>>,
    skill: &Option<SkillUse>,
    secondaries: Vec<SkillUse>,
) -> Vec<DamageLine> {
    let mut out = display_breakdown(rules, main, off, skill);
    for s in secondaries {
        merge_secondary_lines(&mut out, display_breakdown(rules, main, None, &Some(s)));
    }
    out
}

/// The rollover's merge of a secondary skill's lines into the list
/// (`sub_1401527E0`): every list line adds the min / max of each
/// secondary line with its (duration, type) and drops it; what is left is
/// appended.
pub fn merge_secondary_lines(out: &mut Vec<DamageLine>, mut sec: Vec<DamageLine>) {
    for l in out.iter_mut() {
        sec.retain(|s| {
            let same = s.0 == l.0 && s.1 == l.1;
            if same {
                l.2 += s.2;
                l.3 += s.3;
            }
            !same
        });
    }
    out.extend(sec);
}

/// [`shown_damage`] with the weapon's damage as its physical (kind 1) and
/// pierce (kind 0) parts.
fn shown_damage_split(rules: &Rules, atk: Attacker<'_>, skill: &Option<SkillUse>) -> Vec<Shown> {
    let mut rng = Rng(0);
    let (damage, _, _) = collect_damage(
        rules,
        atk,
        skill,
        &mut rng,
        &Debuffs::default(),
        true,
        false,
        0.0,
    );
    let mut v = Vec::new();
    for d in &damage {
        match d {
            combat::Damage::Weapon(w) => {
                v.push((1, CombatType::Physical, w.physical[1], w.physical[2], 0.0));
                v.push((0, CombatType::Pierce, w.pierce[1], w.pierce[2], 0.0));
            }
            combat::Damage::Hit(h) if is_dot_damage(h.ty) => v.push((0, h.ty, h.min, h.max, 0.0)),
            combat::Damage::Dot(x) if is_dot_damage(x.ty) => {
                v.push((2, x.ty, x.dps[1], x.dps[2], x.base_duration))
            }
            _ => {}
        }
    }
    v
}
