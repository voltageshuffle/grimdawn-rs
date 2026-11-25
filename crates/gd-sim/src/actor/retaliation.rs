//! Retaliation: a character's `retaliation*` attributes as damage, their
//! modifiers, and the counter-attack after a landed melee hit
//! ([`retaliate`]).
use super::*;

/// A retaliation type's total % modifier: its own `retaliation<X>Modifier` (hits) or `retaliationSlow<X>Modifier`
/// (DoTs, `dot`) plus `retaliationTotalDamageModifier` for hit types 2, 4–11, 20 and DoT types 2, 5–11, 15
/// (`RetaliationAttributeAbsMod_TotalDamageModifier::AddModifierToAccumulator`). The sheet's retaliation modifier
/// rows read it per type (`Player::CollectRetaliationModifiers`, never type 58).
///
/// `retaliationElementalModifier` never counts: `RetaliationAttributeAbsMod_Elemental` adds one type-58 modifier
/// (unlike the offensive one) and no damage of type 58 exists, elemental damage being split into thirds when
/// created. The crowd-control modifiers (`retaliationStun/Knockdown/SleepModifier`) work the same way on their reflex.
pub fn retaliation_modifier(holder: &Profile, ty: CombatType, dot: bool) -> f32 {
    let t = |k: &str| holder.totals.get(k).copied().unwrap_or(0.0);
    retaliation_modifier_of(&t, ty, dot)
}

/// [`retaliation_modifier`] of one source's fields (`get`).
fn retaliation_modifier_of(get: &dyn Fn(&str) -> f32, ty: CombatType, dot: bool) -> f32 {
    use CombatType as T;
    let fields = type_fields(ty);
    let list = if dot {
        &fields.ret_dot
    } else {
        &fields.ret_hit
    };
    let own: f32 = list.iter().map(|&f| get(f)).sum();
    let listed = if dot {
        matches!(
            ty,
            T::Physical
                | T::Cold
                | T::Fire
                | T::Acid
                | T::Lightning
                | T::Vitality
                | T::Chaos
                | T::Aether
                | T::Bleeding
        )
    } else {
        matches!(
            ty,
            T::Physical
                | T::Pierce
                | T::Cold
                | T::Fire
                | T::Acid
                | T::Lightning
                | T::Vitality
                | T::Chaos
                | T::Aether
                | T::LifeLeech
        )
    };
    own + if listed {
        get("retaliationTotalDamageModifier")
    } else {
        0.0
    }
}

/// `CombatAttributeAccumulator::ModifyDamage` of retaliation damage with one source's retaliation modifiers (`get`):
/// each part's type gets its [`retaliation_modifier_of`] added (a DoT also its `retaliationSlow<X>DurationModifier`;
/// crowd control: its seconds lengthened), then `retaliationDamageMultModifier` scales the damage of types 1, 2,
/// 4–11 and 15 by max(1 + %, 0) (`CombatAttribute_DamageMultiplier::Execute`, type mask 0x8FF6). The accumulator's
/// `ignoreDamageMultiplier` byte is never set for retaliation. PORT: one source = one call (the engine adds one
/// multiplier per attribute record); a crowd control's modifier multiplies its seconds per call instead of adding
/// to one modifier.
pub fn modify_retaliation(damage: &mut [combat::Damage], get: &dyn Fn(&str) -> f32) {
    use CombatType as T;
    let m = (1.0 + get("retaliationDamageMultModifier") * PCT).max(0.0);
    let multiplied = |ty: CombatType| {
        matches!(
            ty,
            T::PhysicalPierce
                | T::Physical
                | T::Pierce
                | T::Cold
                | T::Fire
                | T::Acid
                | T::Lightning
                | T::Vitality
                | T::Chaos
                | T::Aether
                | T::Bleeding
        )
    };
    for d in damage {
        let ty = match d {
            combat::Damage::Weapon(w) => {
                w.physical_modifier += retaliation_modifier_of(get, T::Physical, false);
                T::Physical
            }
            combat::Damage::Hit(h) => {
                h.modifier += retaliation_modifier_of(get, h.ty, false);
                h.ty
            }
            combat::Damage::Dot(x) => {
                if is_dot_damage(x.ty) {
                    x.modifier += retaliation_modifier_of(get, x.ty, true);
                }
                x.duration_modifier += type_fields(x.ty)
                    .ret_dot_duration
                    .iter()
                    .map(|&f| get(f))
                    .sum::<f32>();
                x.ty
            }
            combat::Damage::Reflex { ty, secs } => {
                *secs *= 1.0 + retaliation_modifier_of(get, *ty, false) * PCT;
                continue;
            }
            _ => continue,
        };
        if m != 1.0 && multiplied(ty) {
            d.scale(m * 100.0);
        }
    }
}

/// A character's (or a skill's, `skill`) retaliation as unprocessed damage:
/// the attributes × `scale` ([`retaliation_parts`]), then
/// - with `skill` (`Skill::CollectCombatParameters`):
///   the skill's own retaliation modifiers
///   (`CollectLocalRetaliationModifierAttributes`: its record
///   and its modifiers') and `ModifyDamage` before any conversion,
/// - the conversion `stages` ([`combat::convert_damage`]),
/// - `holder`'s retaliation modifiers on the converted types
///   (`SkillManager::CollectAvailableRetaliationModifierAttributes`, then
///   `ModifyDamage`; `CombatManager::Retaliation` and
///   `Player::CalculateRetaliationValues` the same without a
///   skill).
///
/// PORT: the engine leaves the attacking skill out of the character's
/// attributes and modifiers; `holder.totals` is taken to hold none of the
/// attacking skill's fields (as for its retaliation attributes).
pub(super) fn retaliation_damage(
    holder: &Profile,
    skill: Option<&SkillUse>,
    get: &dyn Fn(&str) -> f32,
    scale: f32,
    rng: &mut Rng,
    stages: &[&[combat::Conversion]],
) -> Vec<combat::Damage> {
    let mut damage = retaliation_parts(
        get,
        &holder.combat_cache().retaliation_instances,
        scale,
        rng,
    );
    if let Some(s) = skill {
        modify_retaliation(&mut damage, &|k: &str| s.own(k));
    }
    combat::convert_damage(&mut damage, stages);
    let t = |k: &str| holder.totals.get(k).copied().unwrap_or(0.0);
    modify_retaliation(&mut damage, &t);
    damage
}

/// A character's retaliation attributes (`retaliation*` fields of `get`) as
/// unprocessed damage × `scale` (`GetRetaliationAttributes(acc, scale)`),
/// rolled in load order: physical as base physical (armour applies), hits,
/// DoTs and debuffs, crowd control; none scaled by attributes
/// (`RetaliationAttributeAbs` creates its damage with the flag off), no
/// modifiers yet ([`modify_retaliation`]). The chance-gated `instances`
/// (`field@chance` of `totals`, [`CombatCache`]) roll on their own, as the
/// engine's per-source attribute instances do; % current life, fear,
/// confusion and convert map as in the offense loop.
fn retaliation_parts(
    get: &dyn Fn(&str) -> f32,
    instances: &[(f32, BTreeMap<String, f32>)],
    scale: f32,
    rng: &mut Rng,
) -> Vec<combat::Damage> {
    use crate::attributes::{AttrFamily, family};
    use CombatType as T;
    let mut damage = Vec::new();
    let collect = |get: &dyn Fn(&str) -> f32,
                   weight: Option<f32>,
                   rng: &mut Rng,
                   damage: &mut Vec<combat::Damage>| {
        let roll_range = |d: &crate::attributes::FieldDef, rng: &mut Rng| -> Option<(f32, f32)> {
            let min = get(d.min?);
            if min <= 0.0 {
                return None;
            }
            let max = d.max.map(get).filter(|m| *m >= min).unwrap_or(min);
            if weight.is_none()
                && let Some(c) = d.chance.map(get).filter(|c| *c > 0.0)
                && rng.unit() * 100.0 > c
            {
                return None;
            }
            let w = weight.unwrap_or(1.0);
            Some((min * scale * w, max * scale * w))
        };
        for d in family(AttrFamily::Retaliation) {
            let Some(ty) = d.combat_type() else { continue };
            let Some((min, max)) = roll_range(d, rng) else {
                continue;
            };
            match ty {
                T::Physical => {
                    let mut w = WeaponDamage::roll(min, max, 0.0, 1.0, rng);
                    w.scale_by_attributes = false;
                    damage.push(combat::Damage::Weapon(w));
                }
                T::PercentCurrentLife => damage.push(combat::Damage::PercentCurrentLife {
                    value: min + (max - min) * rng.unit(),
                    min,
                    max,
                }),
                T::Fear | T::Confusion | T::Convert => damage.push(combat::Damage::Reflex {
                    ty,
                    secs: min + (max - min) * rng.unit(),
                }),
                _ => {
                    let mut push = |mut h: combat::HitDamage| {
                        h.scale_by_attributes = false;
                        damage.push(combat::Damage::Hit(h));
                    };
                    if ty == T::Elemental {
                        combat::HitDamage::roll_elemental(min, max, 1.0, rng)
                            .into_iter()
                            .for_each(push);
                    } else {
                        push(combat::HitDamage::roll(ty, min, max, 1.0, rng));
                    }
                }
            }
        }
        for d in family(AttrFamily::RetaliationDuration) {
            let Some(ty) = d.combat_type() else { continue };
            let Some((min, max)) = roll_range(d, rng) else {
                continue;
            };
            let dmin = d.duration_min.map(get).unwrap_or(0.0);
            let dmax = d
                .duration_max
                .map(get)
                .filter(|m| *m >= dmin)
                .unwrap_or(dmin);
            let dur = dmin + (dmax - dmin) * rng.unit();
            if dur <= 0.0 {
                continue;
            }
            damage.push(combat::Damage::Dot(combat::DotDamage {
                ty,
                dps: [min + (max - min) * rng.unit(), min, max],
                duration: dur,
                base_duration: dur,
                modifier: 0.0,
                duration_modifier: 0.0,
                scale_by_attributes: false,
                hit_multiplier: 1.0,
                // PORT: the summed retaliation totals are one source (the
                // engine keys retaliation per owning item too).
                source: dot_source_key(
                    gd_formats::collections::hash_one("retaliation"),
                    d.min.unwrap_or("retaliation"),
                ),
            }));
        }
        for d in family(AttrFamily::RetaliationCrowdControl) {
            let Some(ty) = d.combat_type() else { continue };
            let Some((min, max)) = roll_range(d, rng) else {
                continue;
            };
            let secs = min + (max - min) * rng.unit();
            damage.push(combat::Damage::Reflex { ty, secs });
        }
    };
    collect(get, None, rng, &mut damage);
    for (c, fields) in instances {
        let c = *c;
        if rng.unit() * 100.0 > c {
            continue;
        }
        let g = |k: &str| fields.get(k).copied().unwrap_or(0.0);
        collect(&g, Some(1.0), rng, &mut damage);
    }
    damage
}

/// `Process` of [`retaliation_parts`]' damage (debuffs take no outgoing
/// reduction).
pub(super) fn process_retaliation(
    damage: &mut [combat::Damage],
    scaling: &AttackerScaling,
    out_red: &ReductionInfo,
    scale: DamageScale,
) {
    for d in damage {
        match d {
            combat::Damage::Weapon(w) => w.process(scaling, out_red, scale, 0.0),
            combat::Damage::Hit(h) => h.process(scaling, out_red, scale),
            combat::Damage::Dot(dot) => {
                let red = if is_dot_damage(dot.ty) {
                    *out_red
                } else {
                    ReductionInfo::default()
                };
                dot.process(scaling, &red, scale);
            }
            _ => {}
        }
    }
}

/// `CombatManager::Retaliation`: after a landed melee hit (`TakeAttack`: style 1, multiplier > 0) the defender
/// (`holder`) hits back with its `retaliation*` attributes as a style-3 attack (always lands, no retaliation of its
/// own), physical as base physical (armor applies), not scaled by attributes; the character's conversions, then its
/// retaliation modifiers ([`retaliation_damage`]). `None` when the holder has no retaliation.
#[allow(clippy::too_many_arguments)]
pub fn retaliate(
    rules: &Rules,
    holder: &Profile,
    attacker: &Profile,
    attacker_life: &mut f32,
    rng: &mut Rng,
    holder_debuffs: &Debuffs,
    attacker_debuffs: &Debuffs,
) -> Option<Hit> {
    if !holder.combat_cache().has_retaliation {
        return None;
    }
    let t = |k: &str| holder.totals.get(k).copied().unwrap_or(0.0);
    let out_red = outgoing_reduction(holder_debuffs);
    let scaling = AttackerScaling {
        formulas: &rules.formulas,
        vars: CombatVars::default(),
    };
    let mut damage = retaliation_damage(holder, None, &t, 1.0, rng, &[&holder.conversions]);
    process_retaliation(&mut damage, &scaling, &out_red, DamageScale::default());
    if damage.is_empty() {
        return None;
    }
    let mut attack = combat::Attack {
        kind: AttackKind::Retaliation,
        offensive_ability: holder.sheet.offensive_ability,
        fumble_chance: 0.0,
        projectile_fumbled: false,
        unblockable: false,
        crit_damage_bonus: 0.0,
        energy_cost: 0.0,
        mana_burn_ratio: 0.0,
        damage,
    };
    let mut d = defender(rules, attacker, *attacker_life, attacker_debuffs);
    d.racial_reduction = crate::racial::defense_against(&attacker.racial, &holder.race);
    let r = combat::take_attack(&rules.formulas, &mut attack, &mut d, rng);
    let damage = (*attacker_life - d.life).max(0.0);
    *attacker_life = d.life.max(0.0);
    Some(Hit {
        missed: false,
        critical: false,
        miss: None,
        blocked: false,
        reflected: Vec::new(),
        link_copy: Vec::new(),
        ..Hit::from_result(r, AttackKind::Retaliation, damage)
    })
}
