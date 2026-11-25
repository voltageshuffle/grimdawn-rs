//! The character window's numbers (stats tab and the page II / III lists),
//! as the strings the exe stores per field.
//!
//! Values of the page II / III lists (`tab2ListBox`, `tab3ListBox`), from
//! `CharacterWindow::UpdatePage2` / `CharacterWindow::UpdatePage3`.
//! Every row comes from the source the combat uses:
//! - damage rows: `Player::CalculateDamageValues(true)` +
//!   `CollectDamageValues(type, duration)` = [`actor::sheet_damage`] (the
//!   character's own damage through the combat's accumulator: modifiers,
//!   conversions, attribute scaling), summed per type; the DoT rows only
//!   count DoTs whose base duration is the row's (3 s; bleed's 3 s, internal
//!   trauma's and poison's 5 s). Both ends `floor(x + 0.5)`, "a - b"
//!   (`tagFormatDash`) or one number when equal; the life steal / % health
//!   rows the same in % (`tagFormatDashPercent` / `tagFormatPercent`).
//! - modifier rows: `CollectDamageModifiers(type, hit / DoT)` /
//!   `CollectDamageDurationModifiers` = [`actor::DisplayModifiers`]
//!   (`tagFormatPlusPercent`, "+0%" when none).
//! - cooldown reduction / energy cost: `Player::CalculateSkillValues(1 / 2)`
//!   = [`Profile::skill_attribute`], clamped to 0..100 (`+N%` / `-N%`).
//! - Constitution `GetTotalCharModifier(10)`, Experience `GetTotalCharAttribute(55)`,
//!   Light Radius `(57)` (`-N%` when negative): [`crate::stats::CharacterSheet`].
//! - skill bonuses: `GetTotalAllLevelAugment` / `GetTotalMasteryLevelAugment`
//!   ([`crate::skills::sheet_augments`]): "0" when ≤ 0, else `tagFormatPlus`.
//! - pet bonuses ([`pet_rows`]), retaliation (`list_values`), stats
//!   ([`stat_rows`]); the Armor row and its breakdown window
//!   ([`armor_breakdown`]).
//!
//! Value strings are canonical ("+15%", "-22%", "15%", "+2", "105 - 111");
//! the window formats them with the matching tag.
//!
//! A dual wielder's damage rows combine both hands ([`both_hands`]).
use crate::actor::{self, Profile, Rules};
use crate::attributes::CombatType as T;
use crate::combat::{DamageScale, Defense, ReductionInfo};
use gd_formats::arz::Record;
use std::collections::BTreeMap;

fn pct(v: f32) -> String {
    format!("{}%", v.trunc() as i32)
}

fn plus_pct(v: f32) -> String {
    format!("+{}%", v.trunc() as i32)
}

/// `%.0f` of a value (rounded), without a "-0".
fn round0(v: f32) -> String {
    let r = v.round_ties_even();
    format!("{}", if r == 0.0 { 0 } else { r as i64 })
}

/// `tagFormatPlusPercent` (`+ {%.0f}%`).
fn plus_pct_f(v: f32) -> String {
    format!("+{}%", round0(v))
}

/// `tagFormatMinusPercent` (`- {%.0f}%`).
fn minus_pct_f(v: f32) -> String {
    format!("-{}%", round0(v))
}

/// A (min, max) damage pair: both `floor(x + 0.5)`; one number when equal.
fn dash(min: f32, max: f32, unit: &str) -> String {
    let (a, b) = ((min + 0.5).floor() as i64, (max + 0.5).floor() as i64);
    if a == b {
        format!("{a}{unit}")
    } else {
        format!("{a}{unit} - {b}{unit}")
    }
}

/// `CharacterWindow::CombineHandDamage`: a damage row from the left hand's and the
/// right hand's (min, max) (`Player::CollectDamageValues`: the two
/// accumulators); `dual` = `SkillManager::IsLeftHandWeapon` (a weapon, not a
/// shield or off-hand, in the left hand). Without it the right hand's;
/// with it the one that has damage, or the lower min and the higher max.
fn both_hands(left: (f32, f32), right: (f32, f32), dual: bool) -> (f32, f32) {
    if left == (0.0, 0.0) || !dual {
        right
    } else if right == (0.0, 0.0) {
        left
    } else {
        (left.0.min(right.0), left.1.max(right.1))
    }
}

/// The hit / DoT types of the damage rows: (row stem, type, DoT base duration).
const DAMAGE_ROWS: &[(&str, T, Option<f32>)] = &[
    ("Physical", T::Physical, None),
    ("Pierce", T::Pierce, None),
    ("Bleed", T::Bleeding, Some(3.0)),
    ("InternalTrauma", T::Physical, Some(5.0)),
    ("Fire", T::Fire, None),
    ("Cold", T::Cold, None),
    ("Lightning", T::Lightning, None),
    ("Acid", T::Acid, None),
    ("Vitality", T::Vitality, None),
    ("Aether", T::Aether, None),
    ("Chaos", T::Chaos, None),
    ("Burn", T::Fire, Some(3.0)),
    ("Coldburn", T::Cold, Some(3.0)),
    ("Electrocute", T::Lightning, Some(3.0)),
    ("Poison", T::Acid, Some(5.0)),
    ("VitalityDecay", T::Vitality, Some(3.0)),
];

/// The sheet's retaliation accumulator `Player::
/// CalculateRetaliationValues`: the character's retaliation
/// attributes (scale 1) in a `CombatDisplayAccumulator` (each min / max ×
/// the attribute's chance / 100 when strictly between 0 and 100, else in
/// full, nothing rolled: `RetaliationAttributeAbs{,_Physical,
/// _ElementalDamage}::AddDamageToAccumulator` / `RetaliationAttributeDurBonus` with
/// `CombatDisplayAccumulator::GetChanceMultiplier`; elemental in thirds, a DoT's duration
/// its minimum), physical merged into base physical, then the character's
/// conversions (`Weigh` + `ConvertDamage`, [`crate::combat::convert_damage`]),
/// then `ModifyDamage`: the retaliation modifiers of each part's type after
/// the conversion and `retaliationDamageMultModifier`
/// ([`actor::modify_retaliation`], the same as in combat), then `Process`
/// with no reduction and no attribute scaling.
pub fn retaliation_damage(p: &Profile, rules: &Rules) -> Vec<crate::combat::Damage> {
    use crate::attributes::{AttrFamily, family};
    use crate::combat::{AttackerScaling, Damage, DotDamage, HitDamage};
    let tot = |k: &str| p.totals.get(k).copied().unwrap_or(0.0);
    // The profile's chance-gated retaliation instances, the `Retaliation` family's (one class each).
    let instances: Vec<&(f32, std::collections::BTreeMap<String, f32>)> = p
        .combat_cache()
        .retaliation_instances()
        .iter()
        .filter(|(_, f)| {
            f.keys()
                .next()
                .and_then(|k| crate::attributes::lookup(k))
                .is_some_and(|d| d.family == AttrFamily::Retaliation)
        })
        .collect();
    let range = |d: &crate::attributes::FieldDef| -> Option<(f32, f32)> {
        // The plain sum plus each chance-gated instance (`field@chance`)
        // weighted by its chance, as the display accumulator does.
        let mut parts: Vec<(f32, f32, f32)> = Vec::new();
        let min = tot(d.min?);
        if min > 0.0 {
            let max = d.max.map(tot).filter(|m| *m >= min).unwrap_or(min);
            // A raw totals map (no `chance_key` pass) keeps the chance plain.
            let c = d.chance.map(tot).unwrap_or(0.0);
            let w = if c > 0.0 && c < 100.0 { c * 0.01 } else { 1.0 };
            parts.push((min, max, w));
        }
        for (c, fields) in instances.iter().copied() {
            let Some(min) = fields.get(d.min?).copied().filter(|m| *m > 0.0) else {
                continue;
            };
            let max = d
                .max
                .and_then(|f| fields.get(f))
                .copied()
                .filter(|m| *m >= min)
                .unwrap_or(min);
            parts.push((min, max, c * 0.01));
        }
        if parts.is_empty() {
            return None;
        }
        Some((
            parts.iter().map(|(l, _, w)| l * w).sum(),
            parts.iter().map(|(_, h, w)| h * w).sum(),
        ))
    };
    let hit = |ty, min, max| {
        Damage::Hit(HitDamage {
            ty,
            value: min,
            min,
            max,
            modifier: 0.0,
            scale_by_attributes: false,
        })
    };
    let mut v = Vec::new();
    for d in family(AttrFamily::Retaliation) {
        let (Some(ty), Some((min, max))) = (d.combat_type(), range(d)) else {
            continue;
        };
        match ty {
            T::Elemental => {
                for e in T::ELEMENTAL {
                    v.push(hit(e, min * 0.333_333_34, max * 0.333_333_34));
                }
            }
            T::PercentCurrentLife => v.push(Damage::PercentCurrentLife {
                value: min,
                min,
                max,
            }),
            T::Fear | T::Confusion => v.push(Damage::Reflex { ty, secs: min }),
            // Convert is an influence (`combat::Influence`), not a sheet damage row.
            T::Convert => {}
            _ => v.push(hit(ty, min, max)),
        }
    }
    for d in family(AttrFamily::RetaliationDuration) {
        let (Some(ty), Some((min, max))) = (d.combat_type(), range(d)) else {
            continue;
        };
        let dur = d.duration_min.map(tot).unwrap_or(0.0);
        if !actor::is_dot_damage(ty) || dur <= 0.0 {
            continue;
        }
        v.push(Damage::Dot(DotDamage {
            ty,
            dps: [min, min, max],
            duration: dur,
            base_duration: dur,
            modifier: 0.0,
            duration_modifier: 0.0,
            scale_by_attributes: false,
            hit_multiplier: 1.0,
            source: 0,
        }));
    }
    crate::combat::convert_damage(&mut v, &[&p.conversions]);
    actor::modify_retaliation(&mut v, &tot);
    let scaling = AttackerScaling {
        formulas: &rules.formulas,
        vars: Default::default(),
    };
    for d in &mut v {
        match d {
            Damage::Hit(h) => {
                h.process(&scaling, &ReductionInfo::default(), DamageScale::default())
            }
            Damage::Dot(x) => {
                x.process(&scaling, &ReductionInfo::default(), DamageScale::default())
            }
            _ => {}
        }
    }
    v
}

/// `Player::CollectRetaliationValues(type, duration)`:
/// `GetMinMaxDamage` summed over the accumulator (hits with
/// duration 0, DoTs whose duration is the row's), both `floor(x + 0.5)` by
/// [`dash`].
fn retaliation_values(v: &[crate::combat::Damage], ty: T, dot: Option<f32>) -> (f32, f32) {
    use crate::combat::Damage;
    v.iter().fold((0.0, 0.0), |(lo, hi), d| match (d, dot) {
        (Damage::Hit(h), None) if h.ty == ty => (lo + h.min, hi + h.max),
        (Damage::Dot(x), Some(want)) if x.ty == ty && x.base_duration == want => {
            (lo + x.dps[1], hi + x.dps[2])
        }
        _ => (lo, hi),
    })
}

/// The page II / III list values, the hands' damage already collected.
fn list_values(
    p: &Profile,
    rules: &Rules,
    attacks_per_second: f32,
    hd: &HandDamage,
) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let s = &p.sheet;
    let tot = |k: &str| p.totals.get(k).copied().unwrap_or(0.0);
    let mut put = |k: &str, v: String| {
        m.insert(format!("list.{k}"), v);
    };
    // `CharacterWindow::UpdatePage2`: the speed rows are the capped speed over the
    // bio's base speed (`CalculateAttackSpeed(0)`, `GetRunSpeed(1)`/GetValue(14),
    // `GetSpellCastSpeed`/GetValue(16)), rounded `floor(x + 0.5)`.
    let rel = |n: usize| ((p.speeds[n] / p.base_speeds[n].max(f32::EPSILON)) * 100.0 + 0.5).floor();
    put("AttacksPerSecond", format!("{:.2}", attacks_per_second));
    put("AttackSpeed", pct(rel(1)));
    put("CastSpeed", pct(rel(2)));
    put("RunSpeed", pct(rel(0)));
    put("CritDmg", plus_pct(tot("offensiveCritDamageModifier")));
    put("HealIncrease", plus_pct(s.heal_increase));
    // `CharacterWindow::UpdatePage2`: the regen less the active skills' cost,
    // `Float2Format` or `NegativeFloat2Format` ("- 1.25") below 0.
    let net = |v: f32| {
        if v >= 0.0 {
            format!("{v:.2}")
        } else {
            format!("- {:.2}", v.abs())
        }
    };
    put("LifeRegen", net(s.life_regen_per_sec - p.active_costs[0]));
    put("ManaRegen", net(s.energy_regen_per_sec - p.active_costs[1]));
    put("EnergyAbsorp", pct(s.energy_absorption));
    put(
        "AvgDmg",
        format!("{:.0} - {:.0}", p.physical.0, p.physical.1),
    );
    put("Constitution", plus_pct_f(s.constitution_modifier));
    put("BonusExpGained", plus_pct_f(s.experience_bonus));
    put(
        "BonusLightRadius",
        if s.light_radius < 0.0 {
            minus_pct_f(s.light_radius.abs())
        } else {
            plus_pct_f(s.light_radius)
        },
    );
    put(
        "CooldownReduction",
        plus_pct_f(
            p.skill_attribute("skillCooldownReduction")
                .clamp(0.0, 100.0),
        ),
    );
    put(
        "ManaCostReduction",
        minus_pct_f(
            p.skill_attribute("skillManaCostReduction")
                .clamp(0.0, 100.0),
        ),
    );
    // A dual wielder's left hand has its own accumulator (`Player::CalculateDamageValues`);
    // `CharacterWindow::CombineHandDamage` combines the two hands.
    let sum = |ty: T, dot: Option<f32>| hd.row(ty, dot);
    let mods = actor::DisplayModifiers::new(p);
    for &(stem, ty, dot) in DAMAGE_ROWS {
        let (lo, hi) = sum(ty, dot);
        put(&format!("{stem}AbsDmg"), dash(lo, hi, ""));
        match dot {
            None => put(&format!("{stem}PercentDmg"), plus_pct_f(mods.hit(ty))),
            Some(_) => {
                let (m, dm) = mods.dot(ty);
                put(&format!("{stem}PercentDmg"), plus_pct_f(m));
                put(&format!("{stem}Duration"), plus_pct_f(dm));
            }
        }
    }
    let (lo, hi) = sum(T::LifeLeech, None);
    put("DamageToHealth", dash(lo, hi, "%"));
    let (lo, hi) = sum(T::PercentCurrentLife, None);
    put("PercentHealthDamage", dash(lo, hi, "%"));
    // Page III, "Retaliation": `CollectRetaliationValues(type, duration)`
    // over [`retaliation_damage`] and `CollectRetaliationModifiers`
    // ([`actor::retaliation_modifier`]) `CharacterWindow::UpdatePage3`.
    let ret = retaliation_damage(p, rules);
    for &(stem, ty, dot) in DAMAGE_ROWS {
        let (lo, hi) = retaliation_values(&ret, ty, dot);
        put(&format!("{stem}Ret"), dash(lo, hi, ""));
        let m = actor::retaliation_modifier(p, ty, dot.is_some());
        put(&format!("{stem}PercentRet"), plus_pct_f(m));
    }
    // Page III, "Defense" (`CharacterWindow::UpdatePage3`): with a shield in the
    // left hand, Chance to Block = `DesignerCalculateMeleeBlockChance(shield
    // chance, defense type 55, pth 100)` (the `meleeBlockEquation`, capped at
    // 100, shown ≥ 0, `tagFormatPercent`), Damage Blocked = the shield's
    // `defensiveBlock` + floor(type 56 % × it + 0.5) (`FloatFormat`); 0
    // without one. Block Recovery is the character attribute 25 capped at 100.
    let (chance, blocked) = match p.shield {
        Some(sh) => {
            let v = crate::stats::CombatVars {
                block_chance: sh.chance,
                block_chance_modifier: tot("defensiveBlockModifier"),
                probability_to_hit: 100.0,
                ..Default::default()
            };
            let c = rules
                .formulas
                .melee_block
                .as_ref()
                .map_or(0.0, |e| v.eval(e))
                .clamp(0.0, 100.0);
            (
                c,
                sh.amount
                    + (tot("defensiveBlockAmountModifier") * crate::stats::PCT * sh.amount + 0.5)
                        .floor(),
            )
        }
        None => (0.0, 0.0),
    };
    put("ChanceToBlock", format!("{}%", round0(chance)));
    put("DamageBlocked", round0(blocked));
    put("BlockRecovery", pct(s.block_recovery_reduction.min(100.0)));
    // dodge and deflect are character attributes 27 / 28.
    put("DodgeChance", pct(s.dodge));
    put("ProjDeflectChance", pct(s.deflect));
    // Crowd-control resists: the capped total of the defence type (`CharacterWindow::GetCappedDefense`:
    // min(total, 100)). `DefenseAttributeAbs_CrowdControl::AddToAccumulator`:
    // `defensiveCrowdControl` expands to stun, freeze, petrify and
    // trap only (not sleep, not slow).
    let cc = tot("defensiveCrowdControl");
    let cap = rules.caps.player_defense_cap[(rules.balance / 4).min(2)];
    for (k, t, with_cc) in [
        ("StunResist", T::Stun, true),
        ("DisruptionResist", T::Disruption, false),
        ("LifeLeechResist", T::LifeLeech, false),
        ("EnergyLeechResist", T::ManaLeech, false),
        ("TrapResist", T::Trap, true),
        ("PetrifyResist", T::Petrify, true),
        ("FreezeResist", T::Freeze, true),
        ("SlowResist", T::TotalSpeed, false),
        ("ReflectResist", T::ReflectionResist, false),
    ] {
        let v = resistance_total(p, t) + if with_cc { cc } else { 0.0 };
        // `Defense::process`: the type's `defensive<X>Modifier` % of |v| on top.
        let v = v + v.abs() * resistance_modifier(p, t) * crate::stats::PCT;
        let max = cap + resistance_cap_bonus(p, t);
        // Reflect resist is the value `TakeAttack` takes ([`Profile::reflection_resist`]).
        let shown = if t == T::ReflectionResist {
            p.reflection_resist(cap)
        } else {
            v.min(max).min(100.0)
        };
        put(k, pct(shown));
        // The rollover's over-maximum amount.
        put(
            &format!("over.{k}"),
            format!("{}", if v >= max { (v - max).max(0.0) } else { 0.0 }),
        );
    }
    m
}

/// The stats tab's resistance rows: (field, combat type).
const RESISTANCE_ROWS: [(&str, T); 10] = [
    ("FireResistanceNumber", T::Fire),
    ("IceResistanceNumber", T::Cold),
    ("LightningResistanceNumber", T::Lightning),
    ("PoisonResistanceNumber", T::Acid),
    ("PiercingResistanceNumber", T::Pierce),
    ("BleedingResistanceNumber", T::Bleeding),
    ("LifeResistanceNumber", T::Vitality),
    ("SpiritResistanceNumber", T::Aether),
    ("StunResistanceNumber", T::Physical),
    ("DisruptionResistanceNumber", T::Chaos),
];

/// A profile's summed `defensive<X>` value of one type ([`Profile::resistances`]).
fn resistance_total(p: &Profile, t: T) -> f32 {
    p.resistances
        .iter()
        .filter(|(ty, _)| *ty == t)
        .map(|x| x.1)
        .sum()
}

/// The resistance reduction on a character (`Character::Contribute{Total,
/// Physical,Elemental}ResistanceReduction{Absolute,Percent}`: its debuffs of
/// types 33, 32, 35, 34, 37, 36), as `TakeAttack` and the sheet read it.
pub fn resistance_reduction(debuffs: &actor::Debuffs) -> ReductionInfo {
    ReductionInfo {
        total_abs: debuffs.get(T::TotalResistanceReductionAbsolute),
        total_pct: debuffs.get(T::TotalResistanceReductionPercent),
        physical_abs: debuffs.get(T::PhysicalResistanceReductionAbsolute),
        physical_pct: debuffs.get(T::PhysicalResistanceReductionPercent),
        elemental_abs: debuffs.get(T::ElementalResistanceReductionAbsolute),
        elemental_pct: debuffs.get(T::ElementalResistanceReductionPercent),
    }
}

/// The maximum-resistance bonus of a resistance type: its own
/// `defensive<X>MaxResist` plus the expansions of `defensiveAllMaxResist`
/// (types 2, 4, 5, 6, 7, 8, 9, 10, 11, 15 = `ALL_RESISTANCES`,
/// `DefenseAttributeDefenseCap_All::AddToAccumulator`) and
/// `defensiveCrowdControlMaxResist` (stun, freeze, petrify, trap,
/// `DefenseAttributeDefenseCap_CrowdControl::AddToAccumulator`).
/// `CombatAttributeAccumulator::ProcessDefense` applies every cap entry to the matching
/// defenses (`ModifyDefenseCap` adds it to the cap bonus) before they process.
pub fn resistance_cap_bonus(p: &Profile, ty: T) -> f32 {
    use crate::attributes::{AttrFamily, family};
    let tot = |k: &str| p.totals.get(k).copied().unwrap_or(0.0);
    let mut v = 0.0;
    for d in family(AttrFamily::DefenseMaxResist) {
        let (Some(f), Some(dt)) = (d.value, d.combat_type()) else {
            continue;
        };
        let val = tot(f);
        if val == 0.0 {
            continue;
        }
        match dt {
            T::AllMaxResist if T::ALL_RESISTANCES.contains(&ty) => v += val,
            T::CrowdControlMaxResist
                if matches!(ty, T::Stun | T::Freeze | T::Petrify | T::Trap) =>
            {
                v += val
            }
            _ if dt == ty => v += val,
            _ => {}
        }
    }
    v
}

/// The % modifier of a resistance type: its own `defensive<X>Modifier`
/// total, plus `defensiveElementalModifier` for fire, cold and lightning.
/// `Defense::process` adds |value| × it %.
pub fn resistance_modifier(p: &Profile, ty: T) -> f32 {
    use crate::attributes::{AttrFamily, family};
    let tot = |k: &str| p.totals.get(k).copied().unwrap_or(0.0);
    family(AttrFamily::DefenseModifier)
        .filter_map(|d| Some((d.value?, d.combat_type()?)))
        .filter(|&(_, dt)| dt == ty || (dt == T::Elemental && ty.is_elemental()))
        .map(|(f, _)| tot(f))
        .sum()
}

/// One resistance defense as `GetAllDefenseAttributes` builds it: the %
/// modifier ([`resistance_modifier`]), the cap bonus
/// ([`resistance_cap_bonus`]) and the restricted flag
/// (`crate::combat::restricted_resistance`); shared by the combat defender and the sheet.
pub fn resistance_defense(p: &Profile, ty: T, value: f32) -> Defense {
    Defense::Resistance {
        ty,
        value,
        modifier: resistance_modifier(p, ty),
        cap_bonus: resistance_cap_bonus(p, ty),
        restricted: crate::combat::restricted_resistance(ty),
    }
}

/// One resistance row of the stats tab.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResistanceRow {
    /// The value shown (after the reduction and the cap, at most 100).
    pub shown: f32,
    /// The rollover's `tagOverCapResist` amount over the cap.
    pub over: f32,
    /// Shown in red (`^r`): a resistance reduction on the character.
    pub reduced: bool,
}

/// A resistance row `CharacterWindow::UpdatePage1` + `CharacterWindow::GetCappedDefense`: three
/// copies of `GetAllDefenseAttributes`: A raw, B `ProcessDefense` without
/// reduction, C `ProcessDefense` with the character's resistance reduction
/// ([`resistance_reduction`]; `CombatAttributeAbsDefense::Process`
/// = [`Defense::process`], which keeps what it took off,
/// `CombatAttributeAccumulator::GetTotalReduction`). Shown = C at most 100; over the cap =
/// max(A − C's reduction − B, 0) while B ≤ C (no reduction pulled the value
/// under the cap), else 0. The value is red when the reduction's total abs +
/// % is > 0, plus the elemental ones for fire / cold / lightning and the
/// physical ones for Stun (physical).
pub fn resistance(p: &Profile, rules: &Rules, reduction: &ReductionInfo, t: T) -> ResistanceRow {
    let raw = resistance_total(p, t);
    // `Character::GetDefenseAttributeCap`, as `TakeAttack` takes it.
    let cap = rules.caps.player_defense_cap[(rules.balance / 4).min(2)];
    // One attribute per type: `CombatAttributeAccumulator::AddDefense`
    // merges a defense into one already held (`CombatAttribute::MergeDefense`).
    let process = |red: &ReductionInfo| {
        let mut d = resistance_defense(p, t, raw);
        d.process(cap, red, DamageScale::default(), 0.0);
        let Defense::Resistance { value, .. } = d else {
            unreachable!()
        };
        // What `Process` took off (`CombatAttributeAbsDefense.reduction`): |v| × % + abs, before the cap.
        let (mut abs, mut pct) = (red.total_abs, red.total_pct);
        if t == T::Physical {
            abs = red.physical_abs.max(abs);
            pct = red.physical_pct.max(pct);
        } else if t.is_elemental() {
            abs = red.elemental_abs.max(abs);
            pct = red.elemental_pct.max(pct);
        }
        (value, raw.abs() * (pct * crate::stats::PCT) + abs)
    };
    let (capped, _) = process(&ReductionInfo::default());
    let (reduced, taken) = process(reduction);
    let over = if capped <= reduced {
        (raw - taken - capped).max(0.0)
    } else {
        0.0
    };
    let r = reduction;
    let mut flag = r.total_abs + r.total_pct;
    if t.is_elemental() {
        flag += r.elemental_abs + r.elemental_pct;
    } else if t == T::Physical {
        flag += r.physical_abs + r.physical_pct;
    }
    ResistanceRow {
        shown: reduced.min(100.0),
        over,
        reduced: flag > 0.0,
    }
}

/// The character window's numbers and rollovers together.
pub fn numbers_and_rollovers(
    p: &Profile,
    rules: &Rules,
    dps: String,
    aps: f32,
    reduction: &ReductionInfo,
    foes: &LastFoes,
    weapon_hands: [(f32, f32); 2],
) -> (BTreeMap<String, String>, BTreeMap<String, RollValues>) {
    let hd = HandDamage::new(rules, p);
    (
        numbers(p, rules, dps, aps, reduction, &hd),
        rollovers(p, rules, foes, weapon_hands, &hd),
    )
}

/// Character-window numbers from a profile (field names of the stats tab).
/// Resistance rows → combat types as Grim Dawn.exe passes them (
/// Ice 5, Poison 7, Piercing 4, Life 9 = Vitality, Spirit 11 = Aether,
/// Stun 2 = Physical, Disruption 10 = Chaos); shown after the player cap
/// (80 + `defensive<X>MaxResist`) and the character's resistance reduction
/// ([`resistance`]), at most 100.
/// Stats shown as `(int)` casts (truncated): the character window
/// builder in Grim Dawn.exe (`CharacterWindow::UpdatePage1`, `CharacterWindow::UpdatePage2`).
fn numbers(
    p: &Profile,
    rules: &Rules,
    dps: String,
    aps: f32,
    reduction: &ReductionInfo,
    hd: &HandDamage,
) -> std::collections::BTreeMap<String, String> {
    let s = &p.sheet;
    let mut m = std::collections::BTreeMap::new();
    let mut put = |k: &str, v: String| {
        m.insert(k.to_string(), v);
    };
    put("StrengthNumber", format!("{}", s.physique.trunc() as i32));
    put("DexterityNumber", format!("{}", s.cunning.trunc() as i32));
    put("IntelligenceNumber", format!("{}", s.spirit.trunc() as i32));
    put("LifeNumber", format!("{}", s.life.trunc() as i32));
    put("ManaNumber", format!("{}", s.energy.trunc() as i32));
    put("LifeRegenNumber", format!("{:.2}", s.life_regen_per_sec));
    put("ManaRegenNumber", format!("{:.2}", s.energy_regen_per_sec));
    put(
        "OffensiveAbilityNumber",
        format!("{}", s.offensive_ability.trunc() as i32),
    );
    put(
        "DefensiveAbilityNumber",
        format!("{}", s.defensive_ability.trunc() as i32),
    );
    put(
        "CombinedArmorNumber",
        armor_breakdown(p, rules).total.to_string(),
    );
    put(
        "TotalDamageNumber",
        format!("{:.0}-{:.0}", p.physical.0, p.physical.1),
    );
    put("AttackSpeedNumber", format!("{:.2}", p.attack_speed));
    put("DPSNumber", dps);
    for (k, t) in RESISTANCE_ROWS {
        let r = resistance(p, rules, reduction, t);
        // Floored: `(int)v - (v < 0)` in `CharacterWindow::UpdatePage1` (79.6 shows 79).
        let red = if r.reduced { "^r" } else { "" };
        put(k, format!("{red}{}%", r.shown.floor() as i32));
        put(&format!("over.{k}"), format!("{}", r.over));
    }
    m.extend(list_values(p, rules, aps, hd));
    m
}

/// The skill bonus rows (`BonusAllSkills`, then the ten classes' rows):
/// "0" when ≤ 0, else `tagFormatPlus` (`CharacterWindow::UpdatePage2`).
pub fn augment_rows(all: u32, masteries: [u32; 10]) -> BTreeMap<String, String> {
    const ROWS: [&str; 10] = [
        "Soldier",
        "Demo",
        "Occu",
        "Nightblade",
        "Arcanist",
        "Shaman",
        "Inquisitor",
        "Necro",
        "Oathkeeper",
        "Berserker",
    ];
    let f = |v: u32| {
        if v == 0 {
            "0".to_string()
        } else {
            format!("+{v}")
        }
    };
    let mut m = BTreeMap::new();
    m.insert("list.BonusAllSkills".to_string(), f(all));
    for (row, v) in ROWS.iter().zip(masteries) {
        m.insert(format!("list.Bonus{row}Skills"), f(v));
    }
    m
}

/// The Pet Bonuses rows (`CharacterWindow::UpdatePage2`): the owner's pet bonus
/// records (`Player::CalculatePetCharAttributeBonus` /
/// `CalculatePetDamageBonus` over the `GameEngine` pet bonus list: each
/// record at its level) — life, attack / cast / run speed: the modifier
/// of the attribute; offensive / defensive ability: the value of the
/// `characterOffensiveAbilityModifier` / `characterDefensiveAbilityModifier`
/// attribute; damage: `offensiveTotalDamageModifier`; crit damage:
/// `offensiveCritDamageModifier` — plus, for the first living pet, its
/// resistances (`GetTotalDefenseType` of its defence accumulator, `pet`).
/// The first living pet's buff skills (`SkillManager::GetBuffSkill` list:
/// toggles, timed buffs, aura buffs on it; per buff the `Skill::CollectPassiveCharAttributes`
/// char attributes and `Skill::CollectPassiveDamageAttributes` damage attributes) add their character attribute
/// modifiers / values and damage modifiers on top of the owner's bonuses
/// (`pet_buffs`: each buff record and its level index).
/// PORT: the pet character's own attribute / damage accumulators (`Character::GetCharAttributeAccumulator` /
/// `Character::GetDamageAttributeAccumulator`) are not added; the pet bonus potion scale is 1.
pub fn pet_rows(
    bonuses: &[(Record, usize)],
    pet_buffs: &[(Record, usize)],
    pet: Option<&Profile>,
) -> BTreeMap<String, String> {
    use crate::attributes::CharAttr as C;
    let mut acc = crate::stats::CharAttrAccumulator::default();
    let (mut dmg, mut crit) = (0.0, 0.0);
    for (r, li) in bonuses.iter().chain(pet_buffs) {
        acc.add_record(r, *li);
        dmg += crate::stats::level_value(r, "offensiveTotalDamageModifier", *li).unwrap_or(0.0);
        crit += crate::stats::level_value(r, "offensiveCritDamageModifier", *li).unwrap_or(0.0);
    }
    let mut m = BTreeMap::new();
    let mut put = |k: &str, v: f32| {
        m.insert(format!("list.{k}"), plus_pct_f(v));
    };
    put("PetHealth", acc.modifier[C::Life as usize]);
    put("PetDamage", dmg);
    put("PetCritDamage", crit);
    put("PetAttackSpeed", acc.modifier[C::AttackSpeed as usize]);
    put("PetCastSpeed", acc.modifier[C::CastSpeed as usize]);
    put("PetRunSpeed", acc.modifier[C::RunSpeed as usize]);
    put("PetOA", acc.get(C::OffensiveAbilityModifier));
    put("PetDA", acc.get(C::DefensiveAbilityModifier));
    let res = |t: T| pet.map_or(0.0, |p| resistance_total(p, t));
    for (k, t) in [
        ("PetPhysicalResist", T::Physical),
        ("PetFireResist", T::Fire),
        ("PetColdResist", T::Cold),
        ("PetLightningResist", T::Lightning),
        ("PetPoisonResist", T::Acid),
        ("PetPierceResist", T::Pierce),
        ("PetBleedResist", T::Bleeding),
        ("PetVitalityResist", T::Vitality),
        ("PetAetherResist", T::Aether),
        ("PetChaosResist", T::Chaos),
        ("PetStunResist", T::Stun),
        ("PetTrapResist", T::Trap),
        ("PetPetrifyResist", T::Petrify),
        ("PetFreezeResist", T::Freeze),
        ("PetSleepResist", T::Sleep),
        ("PetSlowResist", T::TotalSpeed),
    ] {
        put(k, res(t));
    }
    m
}

/// What the page III Stats rows read: the character's play stats (the
/// save's block 16 plus this session's counts) and the equipped items' levels.
#[derive(Debug, Clone, Default)]
pub struct SheetStats {
    pub play_seconds: u32,
    pub kills: u32,
    /// `numberOfChampionKills` (champion-or-better kills).
    pub champion_kills: u32,
    pub greatest_damage_dealt: f32,
    pub greatest_damage_taken: f32,
    pub deaths: u32,
    /// `GetGreatestMonsterKilledName` of the current difficulty (a tag).
    pub greatest_monster: String,
    /// The equipped items' `itemLevel`s.
    pub item_levels: Vec<f32>,
}

/// The Stats rows (`CharacterWindow::UpdatePage3`): `Player::GetItemLevel`
/// (mean of the equipped items' levels, `floor(x + 0.5)`),
/// `tagTimePlayedNumber` of days : hours : minutes
/// (`GetPlayTimeInMinutes` = seconds / 60), `numberOfKills`,
/// `numberOfChampionKills`, `greatestDamageInflicted` / `Received`
/// (`floor(max(x, 0) + 0.5)`), `numberOfDeaths`; the greatest monster's
/// tag goes out as is (the window localises it, `tagNone` when unknown,
/// in the `GetMonsterColorText(4)` colour).
pub fn stat_rows(st: &SheetStats) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        m.insert(format!("list.{k}"), v);
    };
    let n = st.item_levels.len().max(1) as f32;
    let level = if st.item_levels.is_empty() {
        0.0
    } else {
        (st.item_levels.iter().sum::<f32>() / n).max(0.0) + 0.5
    };
    put("ItemLevel", format!("{}", level.floor() as i64));
    let minutes = st.play_seconds / 60;
    put(
        "TotalPlaytime",
        format!(
            "{}:{}:{}",
            minutes / 60 / 24,
            minutes / 60 % 24,
            minutes % 60
        ),
    );
    put("MonstersKilled", st.kills.to_string());
    put("ChampionsKilled", st.champion_kills.to_string());
    put(
        "GreatestDamageDealt",
        format!(
            "{}",
            (st.greatest_damage_dealt.max(0.0) + 0.5).floor() as i64
        ),
    );
    put(
        "GreatestDamageTaken",
        format!(
            "{}",
            (st.greatest_damage_taken.max(0.0) + 0.5).floor() as i64
        ),
    );
    put("TotalDeaths", st.deaths.to_string());
    put("GreatestMonsterName", st.greatest_monster.clone());
    m
}

/// The values of one stats row's rollover, as the page updates store them
/// in the rollover objects (Grim Dawn.exe): `lines[0]` = line 1's
/// two values (`StatRollover.line1Values`), `lines[k]` = line k + 1's (`StatRollover.lineValues[k − 1]`).
/// The text classes pick a line's `LineNFormatTag`
/// when its second value is non-zero, else `LineNFormatTag2` with the first
/// only. `name`: line 2's text instead of its values (the Offensive /
/// Defensive Ability rollovers' last foe, `CharacterWindow::SetLastFoeRollover` sets the flag
/// `StatRollover.hasName` and the string `StatRollover.name`); a tag, or a name when it is not one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RollValues {
    pub lines: [(f32, f32); 8],
    pub name: Option<String>,
    /// A line's label tag instead of the record's `Line<n>Tag` (the DPS window's lines 3 / 4 are the mouse skills'
    /// names, which no record holds).
    pub labels: [Option<String>; 8],
    /// Line 1's format tag instead of the record's `Line1FormatTag`
    /// (`CharacterWindow::SetRolloverFormat`).
    pub format: Option<String>,
}

/// The last monster the player hit and the last one that hit the player
/// (`PlayStats::GetLastMonsterHit` / `GetLastMonsterHitBy` of the current
/// difficulty): the monster's description tag (empty: none) and, recorded
/// with it, its Defensive Ability resp. the attack's Offensive Ability
/// (`CombatManager::RecordHit`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LastFoes {
    pub hit: (String, f32),
    pub hit_by: (String, f32),
}

/// The damage of both hands' accumulators per row type (`Player::
/// CalculateDamageValues(true)` + `CollectDamageValues(left min, left max,
/// right min, right max, type, duration)`): the right hand's, and a dual
/// wielder's left hand's (none without one).
struct HandDamage {
    right: Vec<actor::Shown>,
    left: Option<Vec<actor::Shown>>,
}

impl HandDamage {
    fn new(rules: &Rules, p: &Profile) -> Self {
        Self {
            right: actor::sheet_damage(rules, p),
            left: p.off_hand().map(|l| actor::sheet_damage(rules, l)),
        }
    }

    fn sum_of(shown: &[actor::Shown], ty: T, dot: Option<f32>) -> (f32, f32) {
        shown
            .iter()
            .filter(|x| {
                x.1 == ty
                    && if let Some(d) = dot {
                        x.0 == 2 && x.4 == d
                    } else {
                        x.0 != 2
                    }
            })
            .fold((0.0, 0.0), |(a, b), x| (a + x.2, b + x.3))
    }

    /// (right, left) (min, max); the left is zero without a dual wield.
    fn hands(&self, ty: T, dot: Option<f32>) -> ((f32, f32), (f32, f32)) {
        (
            Self::sum_of(&self.right, ty, dot),
            self.left
                .as_ref()
                .map_or((0.0, 0.0), |l| Self::sum_of(l, ty, dot)),
        )
    }

    /// The row's (min, max) ([`both_hands`]).
    fn row(&self, ty: T, dot: Option<f32>) -> (f32, f32) {
        let (r, l) = self.hands(ty, dot);
        both_hands(l, r, self.left.is_some())
    }
}

/// A damage row's rollover lines 2 / 3 (main hand, off-hand): a hand's
/// max is dropped (0) when it equals its min (`CharacterWindow::UpdatePage2`).
fn hand_lines(right: (f32, f32), left: (f32, f32)) -> RollValues {
    let one = |(a, b): (f32, f32)| (a, if a == b { 0.0 } else { b });
    let mut r = RollValues::default();
    r.lines[1] = one(right);
    r.lines[2] = one(left);
    r
}

/// The stats rows' rollover values (keys as [`numbers`]); the rows whose
/// rollover only shows the row's own number are not here. `weapon_hands`: the weapon attack's
/// main / off-hand damage per hit. Grim Dawn.exe
/// `CharacterWindow::UpdatePage1` (page I) and `CharacterWindow::UpdatePage2` (page II):
/// - Life / Energy: (base, `trunc(total)` − base) resp. (base, total − base);
/// - Physique / Cunning / Spirit: line 1 (base, total − base); line 2
///   `GetPointsSpent` = `(int)((base − original) / increment)`; line 3 the life
///   the attribute above the pc record's gives (`GetLifePer*` × (total −
///   original)); Physique line 4 `GetLifeRegenValue(0, 0, 1)` (the life
///   equation without the regen attribute); Cunning lines 4..6
///   (`DesignerCalculatePhysicalDamagePercentage(1)` − 1) × 100 with
///   `DesignerCalculatePhysicalDamageBonus`, the pierce and physical
///   duration equations at 1 the same way; Spirit lines 4 / 5 the magical and
///   magical duration ones, line 6 the energy above the original
///   (`GetManaPerIntel` × (total − original)), line 7
///   ((`GetManaRegenValue(1, 0, 1, 0)` − `GetManaRegenValue(0, 0, 1, 0)`) − 1)
///   × 100 and `GetManaRegenValue(0, 0, 1, 0)` − `GetManaRegenValue(0, 0, 1,
///   original Spirit)`;
/// - Offensive / Defensive Ability: line 1 the value, line 2 the last foe,
///   line 3 `DesignerCalculateProbabilityToHit(OA, foe's DA)` resp.
///   `(foe's OA, DA)` clamped to 0..100, line 4 its critical chance × 100
///   (≤ 100);
/// - Health / Energy Regeneration (page II): (the regen equation of the bio's
///   base attribute, |total − that − the active skills' cost|
///   ([`Profile::active_costs`])); with a negative bonus the line uses
///   `tagRollFormat16` (`CharacterWindow::SetRolloverFormat`);
/// - Attack / Cast Speed: line 1 the row's percentage, line 2 the cap
///   (`player*SpeedCapMax` + the bio's max modifier; the attack one
///   rounded); Run Speed line 1 (min(cap, uncapped %), max(uncapped % − cap,
///   0)), line 2 the cap;
/// - the damage rows and Weapon Damage: lines 2 / 3 per hand ([`hand_lines`]).
fn rollovers(
    p: &Profile,
    rules: &Rules,
    foes: &LastFoes,
    weapon_hands: [(f32, f32); 2],
    hd: &HandDamage,
) -> BTreeMap<String, RollValues> {
    use crate::stats::CombatVars;
    let mut m: BTreeMap<String, RollValues> = BTreeMap::new();
    let s = &p.sheet;
    let b = &s.base;
    let f = &rules.formulas;
    let total = [s.physique, s.cunning, s.spirit];
    let inc = [
        rules.levels.strength_increment,
        rules.levels.dexterity_increment,
        rules.levels.intelligence_increment,
    ];
    let life_per = [
        rules.rates.life_per_physique,
        rules.rates.life_per_cunning,
        rules.rates.life_per_spirit,
    ];
    let mut attr = |key: &str, i: usize| {
        let mut r = RollValues::default();
        r.lines[0] = (b.attributes[i], total[i] - b.attributes[i]);
        r.lines[1].0 = if inc[i] > 0 {
            ((b.attributes[i] - b.original[i]) / inc[i] as f32).trunc()
        } else {
            0.0
        };
        r.lines[2].0 = life_per[i] * (total[i] - b.original[i]);
        m.insert(key.to_owned(), r);
    };
    attr("StrengthNumber", 0);
    attr("DexterityNumber", 1);
    attr("IntelligenceNumber", 2);
    let eq =
        |e: &Option<gd_rules::expr::Expr>, v: CombatVars| e.as_ref().map_or(1.0, |e| v.eval(e));
    let vars = CombatVars {
        strength: s.physique,
        dexterity: s.cunning,
        intelligence: s.spirit,
        character_level: s.level as f32,
        max_life: s.life,
        ..Default::default()
    };
    if let Some(r) = m.get_mut("StrengthNumber") {
        r.lines[3].0 = rules.regen.life_value(0.0, s.physique, s.spirit);
    }
    if let Some(r) = m.get_mut("DexterityNumber") {
        r.lines[3] = (
            (eq(
                &f.physical_damage_percentage,
                CombatVars {
                    physical_damage: 1.0,
                    ..vars
                },
            ) - 1.0)
                * 100.0,
            f.physical_damage_bonus
                .as_ref()
                .map_or(0.0, |e| vars.eval(e)),
        );
        r.lines[4].0 = (eq(
            &f.pierce_damage,
            CombatVars {
                pierce_damage: 1.0,
                ..vars
            },
        ) - 1.0)
            * 100.0;
        r.lines[5].0 = (eq(
            &f.physical_duration_damage,
            CombatVars {
                physical_damage: 1.0,
                ..vars
            },
        ) - 1.0)
            * 100.0;
    }
    if let Some(r) = m.get_mut("IntelligenceNumber") {
        r.lines[3].0 = (eq(
            &f.magical_damage,
            CombatVars {
                magical_damage: 1.0,
                ..vars
            },
        ) - 1.0)
            * 100.0;
        r.lines[4].0 = (eq(
            &f.magical_duration_damage,
            CombatVars {
                magical_damage: 1.0,
                ..vars
            },
        ) - 1.0)
            * 100.0;
        r.lines[5].0 = rules.rates.energy_per_spirit * (s.spirit - b.original[2]);
        let mana = |regen: f32, int: f32| {
            rules
                .regen
                .mana_value(regen, s.physique, if int != 0.0 { int } else { s.spirit })
        };
        let now = mana(0.0, 0.0);
        // `fminf(fmaxf(d, 0), d)`: d.
        r.lines[6] = (
            (mana(1.0, 0.0) - now - 1.0) * 100.0,
            now - mana(0.0, b.original[2]),
        );
    }
    let put = |m: &mut BTreeMap<String, RollValues>, k: &str, lines: &[(f32, f32)]| {
        let mut r = RollValues::default();
        r.lines[..lines.len()].copy_from_slice(lines);
        m.insert(k.to_owned(), r);
    };
    put(
        &mut m,
        "LifeNumber",
        &[(b.attributes[3], s.life.trunc() - b.attributes[3])],
    );
    put(
        &mut m,
        "ManaNumber",
        &[(b.attributes[4], s.energy - b.attributes[4])],
    );
    let pth_line = |pth: f32| (pth.clamp(0.0, 100.0), 0.0);
    // `Character::DesignerCalculateCriticalChance`.
    let crit_line = |pth: f32| {
        (
            (crate::combat::critical_chance(f, pth) * 100.0).min(100.0),
            0.0,
        )
    };
    let oa_pth = crate::combat::probability_to_hit(f, s.offensive_ability, foes.hit.1);
    let da_pth = crate::combat::probability_to_hit(f, foes.hit_by.1, s.defensive_ability);
    put(
        &mut m,
        "OffensiveAbilityNumber",
        &[
            (s.offensive_ability, 0.0),
            (0.0, 0.0),
            pth_line(oa_pth),
            crit_line(oa_pth),
        ],
    );
    put(
        &mut m,
        "DefensiveAbilityNumber",
        &[
            (s.defensive_ability, 0.0),
            (0.0, 0.0),
            pth_line(da_pth),
            crit_line(da_pth),
        ],
    );
    let life_base = rules.regen.life_value(b.regen[0], s.physique, s.spirit);
    let mana_base = rules.regen.mana_value(b.regen[1], s.physique, s.spirit);
    for (k, base, now, cost) in [
        (
            "list.LifeRegen",
            life_base,
            s.life_regen_per_sec,
            p.active_costs[0],
        ),
        (
            "list.ManaRegen",
            mana_base,
            s.energy_regen_per_sec,
            p.active_costs[1],
        ),
    ] {
        put(&mut m, k, &[(base, (now - base - cost).abs())]);
        if now - base - cost < 0.0 {
            m.get_mut(k).unwrap().format = Some("tagRollFormat16".into());
        }
    }
    // Percent of the bio's base speed.
    let pct_of = |n: usize, v: f32| v / p.base_speeds[n].max(f32::EPSILON) * 100.0;
    let rnd = |v: f32| (v + 0.5).floor();
    let cap = |n: usize| pct_of(n, p.speed_caps[n].max);
    put(
        &mut m,
        "list.AttackSpeed",
        &[(rnd(pct_of(1, p.speeds[1])), 0.0), (rnd(cap(1)), 0.0)],
    );
    put(
        &mut m,
        "list.CastSpeed",
        &[(rnd(pct_of(2, p.speeds[2])), 0.0), (cap(2), 0.0)],
    );
    let run = rnd(pct_of(0, p.speed_caps[0].raw));
    let run_cap = if p.speed_caps[0].max > 0.0 {
        cap(0)
    } else {
        run
    };
    put(
        &mut m,
        "list.RunSpeed",
        &[(run_cap.min(run), (run - run_cap).max(0.0)), (run_cap, 0.0)],
    );
    for &(stem, ty, dot) in DAMAGE_ROWS {
        let (r, l) = hd.hands(ty, dot);
        m.insert(format!("list.{stem}AbsDmg"), hand_lines(r, l));
    }
    for (k, ty) in [
        ("DamageToHealth", T::LifeLeech),
        ("PercentHealthDamage", T::PercentCurrentLife),
    ] {
        let (r, l) = hd.hands(ty, None);
        m.insert(format!("list.{k}"), hand_lines(r, l));
    }
    m.insert(
        "list.AvgDmg".into(),
        hand_lines(weapon_hands[0], weapon_hands[1]),
    );
    if let Some(r) = m.get_mut("OffensiveAbilityNumber") {
        r.name = Some(if foes.hit.0.is_empty() {
            "tagNone".into()
        } else {
            foes.hit.0.clone()
        });
    }
    if let Some(r) = m.get_mut("DefensiveAbilityNumber") {
        r.name = Some(if foes.hit_by.0.is_empty() {
            "tagNone".into()
        } else {
            foes.hit_by.0.clone()
        });
    }
    m
}

/// One body region's row of the armour breakdown window
/// (`CombinedArmorRolloverWindow`): its armour, chance to be hit there and
/// absorption, all `(int)` as the window shows them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ArmorRow {
    pub armor: i32,
    pub chance: i32,
    pub absorption: i32,
}

/// The sheet's Armor row and its breakdown window Grim Dawn.exe
/// `CharacterWindow::UpdatePage1` (page I update, every 500 ms):
/// - per worn slot the item's defence accumulator (`GetTotalDefenseType`
///   38 armour, 39 absorption): head (slot 7), chest (8), hands (10), legs
///   (11), feet (12) and shoulders (9) give their region's armour and
///   absorption; the weapons (slots 0 / 1, the same item once), the belt
///   (6), the jewellery / medal / relic (2..5, 13) and the skills, bio and
///   the character's other sources go to every region (general armour);
/// - armour `x + |x| × defensiveProtectionModifier %` per part, absorption
///   `min(a + |a| × defensiveAbsorptionModifier %, 100)`;
/// - a region's armour = `floor(general + region + 0.5)`, its chance
///   `(int)GetCombatRegionChance(region)`, its absorption `floor(abs + 0.5)`
///   (`ArmorWindow::SetRegionArmor` / `ArmorWindow::SetRegionAbsorption` write them, 0 in the window's
///   `unprotectedColor`);
/// - the row's total = `floor(general + Σ chance % × region + 0.5)`.
///
/// An armour piece's absorption is its `BaseProtectionAbsorption` attribute's
/// (`DefenseAttribute_BaseProtectionAbsorption::AddToAccumulator`: only when its protection > 0):
/// `DefenseAttribute_BaseProtectionAbsorption::LoadFromTable`
/// gives every piece the game balance `armorDefensiveAbsorption`
/// (`GameEngine.armorDefensiveAbsorption`, [`crate::stats::EngineConstants::armor_absorption`]);
/// no record field changes it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArmorBreakdown {
    /// Head, chest, hands, legs, feet, shoulders (the record's field order;
    /// the window lists shoulders second).
    pub rows: [ArmorRow; 6],
    pub total: i32,
}

/// The breakdown's regions in [`ArmorBreakdown::rows`] order.
pub const ARMOR_REGIONS: [crate::stats::Region; 6] = {
    use crate::stats::Region as R;
    [R::Head, R::Torso, R::Arms, R::Legs, R::Feet, R::Shoulders]
};

/// [`ArmorBreakdown`] of a profile: its defender's armour entries, each processed as
/// `Defense::process` does before the reduction.
pub fn armor_breakdown(p: &Profile, rules: &Rules) -> ArmorBreakdown {
    let floor = |v: f32| (v + 0.5).floor() as i32;
    // (region, armour, absorption) per entry; region None = the general armour.
    let entries: Vec<(Option<crate::stats::Region>, f32, f32)> = p
        .combat_cache()
        .defenses()
        .iter()
        .filter_map(|d| match d {
            Defense::Armor {
                region,
                protection,
                absorption,
                protection_modifier,
                absorption_modifier,
            } => Some((
                *region,
                protection + protection.abs() * protection_modifier * 0.01,
                (absorption + absorption.abs() * absorption_modifier * 0.01).min(100.0),
            )),
            _ => None,
        })
        .collect();
    let general: f32 = entries.iter().filter(|e| e.0.is_none()).map(|e| e.1).sum();
    let mut out = ArmorBreakdown::default();
    let mut total = general;
    for (row, region) in out.rows.iter_mut().zip(ARMOR_REGIONS) {
        let pieces = || entries.iter().filter(move |e| e.0 == Some(region));
        let armor: f32 = pieces().map(|e| e.1).sum();
        let chance = rules
            .formulas
            .regions
            .iter()
            .find(|r| r.0 == region)
            .map_or(0.0, |r| r.1 as f32);
        let abs = if armor > 0.0 {
            pieces().map(|e| e.2).fold(0.0, f32::max)
        } else {
            0.0
        };
        total += chance * 0.01 * armor;
        *row = ArmorRow {
            armor: floor(general + armor),
            chance: chance as i32,
            absorption: floor(abs),
        };
    }
    out.total = floor(total);
    out
}
