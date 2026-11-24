//! The `SkillProfile_Modifiers` lines: energy cost, cooldown, duration, radius, targets
//! (`SkillProfile_Modifiers::CreateUIText` / `SkillProfile_Modifiers::CreateUINextText`, the parent module's
//! docs).
use super::*;

/// How a changed value is tested in `SkillProfile_Modifiers::CreateUINextText` (the "next level" text:
/// only what differs from the current level is listed).
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Cmp {
    /// `next != current`
    Ne,
    /// `next - current > 0`
    Gt,
}

/// The `SkillProfile_Modifiers` values the tooltip lists, in the order of
/// `SkillProfile_Modifiers::CreateUINextText` / `CreateUIText` (struct order from
/// `SkillProfile_Modifiers::Load` which fills them from these record fields; tags and order
/// from the two functions' string references): (field, test, tag, label tag).
/// The energy reservation lines come after the costs ([`COSTS`]); the
/// chance / life monitor lines before and the spark lines after them are
/// [`param_lines`]'s own.
const COSTS: usize = 4;
pub(super) const PARAMS: &[(&str, Cmp, &str, &str)] = &[
    ("skillManaCost", Cmp::Ne, "SkillCostFormat", "ManaCost"),
    (
        "skillActiveManaCost",
        Cmp::Gt,
        "SkillCostFormat",
        "ActiveManaCost",
    ),
    (
        "skillActiveLifeCost",
        Cmp::Gt,
        "SkillCostFormat",
        "ActiveLifeCost",
    ),
    (
        "skillActivePercentLifeCost",
        Cmp::Gt,
        "SkillCostPercentFormat",
        "ActiveLifeCost",
    ),
    ("cooldownCharges", Cmp::Ne, "CooldownCharges", ""),
    (
        "skillCooldownTime",
        Cmp::Ne,
        "SkillSecondFormat",
        "CooldownTime",
    ),
    (
        "skillActiveDuration",
        Cmp::Gt,
        "SkillSecondFormat",
        "ActiveDuration",
    ),
    (
        "projectileDuration",
        Cmp::Ne,
        "SkillSecondFormat",
        "ProjectileDuration",
    ),
    ("skillTargetAngle", Cmp::Gt, "TargetAngle", ""),
    ("skillTargetNumber", Cmp::Ne, "TargetNumber", ""),
    (
        "projectileLaunchNumber",
        Cmp::Ne,
        "ProjectileLaunchNumber",
        "",
    ),
    (
        "projectilePiercing",
        Cmp::Gt,
        "ProjectilePiercingChance",
        "",
    ),
    ("beamNumber", Cmp::Ne, "BeamNumber", ""),
    ("beamAngle", Cmp::Gt, "BeamAngle", ""),
    (
        "skillTargetRadius",
        Cmp::Ne,
        "SkillDistanceFormat",
        "@TargetRadius",
    ),
    (
        "explosionRadius",
        Cmp::Ne,
        "SkillDistanceFormat",
        "ExplosionRadius",
    ),
    (
        "skillSpeedAcceleration",
        Cmp::Ne,
        "SkillSecondFormat",
        "RunSpeedAcceleration",
    ),
    (
        "waveDistance",
        Cmp::Ne,
        "SkillDistanceFormat",
        "TargetRange",
    ),
];

/// `GetTargetRadiusTag` (the classes overriding it return
/// `ExplosionRadius`, the base `TargetRadius`).
fn target_radius_tag(class: &str) -> &'static str {
    match class {
        "SkillSecondary_OnHitBuffRadius"
        | "SkillSecondary_OnKillAttackRadius"
        | "Skill_AttackBuffRadius"
        | "Skill_AttackRadius"
        | "Skill_AttackRadiusLeap"
        | "Skill_AttackRadiusTeleport"
        | "Skill_BuffAttackRadiusToggled"
        | "Skill_BuffRadius"
        | "Skill_BuffRadiusToggled"
        | "SkillBuff_Passive"
        | "SkillSecondary_AttackRadius"
        | "SkillSecondary_BuffRadius"
        | "SkillSecondary_PetModifier" => "ExplosionRadius",
        _ => "TargetRadius",
    }
}

/// Whether `SkillProfile_Modifiers::CreateUIText` / `CreateUINextText` use the `...Mod` tags: a
/// `Skill_Modifier`, or a `SkillSecondary_PetModifier` unless its pet skill is
/// no modifier and it adds no charges (the check on the pet's skill).
fn uses_mod_tags(rec: &Record, prof: &Record, level: u32) -> bool {
    if rec.class == "SkillSecondary_PetModifier" {
        return is_tooltip_modifier_class(&prof.class)
            || level_f(rec, "cooldownCharges", level) != 0.0;
    }
    is_tooltip_modifier_class(&rec.class)
}

/// The period of a channelled skill's cost (`Skill_AttackSpellCone` /
/// `Skill_AttackSpellDrain::GetManaCostPeriod`: the skill's period
/// over `Character::GetSpellCastSpeed`, `timeBetweenAttacks` ms, default 100,
/// as `gd_game::casting::channels`); 0 otherwise, the cost then reads per use.
fn cost_period(ctx: &SkillTipCtx, rec: &Record, prof: &Record) -> f32 {
    if !matches!(
        rec.class.as_str(),
        "Skill_AttackSpellBeam" | "Skill_AttackSpellCone" | "Skill_AttackSpellDrain"
    ) {
        return 0.0;
    }
    let speed = ctx.player.map_or(1.0, |(_, p)| p.speeds[2]).max(0.01);
    prof.f32("timeBetweenAttacks")
        .filter(|t| *t > 0.0)
        .unwrap_or(100.0)
        * 0.001
        / speed
}

/// Energy cost, cooldown, duration, radius... lines of a skill: the current
/// level's (`prev == None`, `SkillProfile_Modifiers::CreateUIText`: every non-zero value) or the next
/// level's (`SkillProfile_Modifiers::CreateUINextText`: what differs from `prev`). `rec` is the skill
/// object's own record (classes), `prof` its profile; potion lines ([`PARAMS`]'
/// `PotionCooldownCharges`), combo lines ([`combo_lines`]) and a pet modifier's `...Mod` tags
/// ([`uses_mod_tags`]) included.
#[allow(clippy::too_many_arguments)]
pub(super) fn param_lines(
    ctx: &SkillTipCtx,
    rec: &Record,
    prof: &Record,
    mods: &[(Record, usize)],
    level: u32,
    prev: Option<u32>,
    class: u8,
    reduce: bool,
) -> Vec<TextLine> {
    let mut out = Vec::new();
    // `Skill::CreateUIModifierText`: the profile data is
    // the skill's plus its learned modifiers' (`SkillProfile_Modifiers::Accumulate` accumulates
    // `GetSkillModifierData`); a `Skill_Modifier` itself lists its values with
    // the `...Mod` tags (`+23.4 Energy Cost`).
    let is_mod = uses_mod_tags(rec, prof, level);
    let mod_tag = |tag: &str, v: f32| {
        if is_mod && ctx.tags.get(&format!("{tag}Mod")).is_some() && v > 0.0 {
            format!("{tag}Mod")
        } else {
            tag.to_string()
        }
    };
    // PORT: `explosionRadius` of the profile is the projectile skills'
    // `projectileExplosionRadius` field.
    let own = |field: &str, lv: u32| {
        level_f(prof, field, lv)
            + if field == "explosionRadius" {
                level_f(prof, "projectileExplosionRadius", lv)
            } else {
                0.0
            }
    };
    let sum = |field: &str, lv: u32| {
        own(field, lv)
            + if lv > 0 {
                mods.iter()
                    .map(|(m, ml)| level_f(m, field, *ml as u32 + 1))
                    .sum::<f32>()
            } else {
                0.0
            }
    };
    // `SkillProfile_Modifiers::Load`: the chance a monster / auto-cast picks it, the life
    // monitor (`lifeMonitorCooldown` in seconds, as shown: the Savior potion
    // modifiers hold 20).
    // `now`: `sum(f, level)`, which every caller already holds.
    let changed = |f: &str, now: f32| prev.is_none_or(|p| now != sum(f, p));
    let weight = sum("skillChanceWeight", level);
    if weight != 0.0 && changed("skillChanceWeight", weight) {
        out.push(line(
            class,
            ctx.fmt(&mod_tag("SkillChanceWeight", weight), &[Arg::Num(weight)]),
        ));
    }
    let monitor = sum("lifeMonitorPercent", level);
    let cool = sum("lifeMonitorCooldown", level);
    if monitor > 0.0
        && (changed("lifeMonitorPercent", monitor) || changed("lifeMonitorCooldown", cool))
    {
        let text = if cool > 0.0 {
            ctx.fmt(
                &mod_tag("LifeMonitorPercentCooldown", monitor),
                &[Arg::Num(monitor), Arg::Num(cool)],
            )
        } else {
            ctx.fmt(
                &mod_tag("LifeMonitorPercent", monitor),
                &[Arg::Num(monitor)],
            )
        };
        out.push(line(class, text));
    }
    // `Skill::CreateUIModifierText`: the cooldown ×
    // (1 − `GetTotalSkillAttribute(1)` %) and the energy cost × (1 −
    // `GetTotalSkillAttribute(2)` %), floored at 0, unless the skill is an
    // item skill (`Skill.isItemSkill`) or a potion (`Skill.isPotion`). `Skill::GetTotalSkillAttribute`
    // is the skill's own (record + modifiers) plus its
    // `SkillManager`'s total (gear, passives: [`Profile::skill_attribute`]);
    // a skill outside the player's manager (an unlearned devotion star) gets
    // none of the manager's: the real Time Dilation tooltip reads 13 s with
    // 15 % (`reduce` false). Real window: a 6 s skill reads 5.1 s, Wrath of
    // Agrivix 30 reads 23.4. The energy cost is the cast's own
    // [`crate::actor::skill_energy_cost`] (without the chance part, as the hot
    // slot). PORT: the skill's own `skillCooldownReduction` is not added, and
    // a modifier's cooldown reads unreduced (a 3.8 s modifier, real window).
    let cdr = ctx.player.filter(|_| reduce).map_or(0.0, |(_, p)| {
        p.skill_attribute("skillCooldownReduction") * 0.01
    });
    let char_mana_cut = ctx
        .player
        .filter(|_| reduce)
        .map_or(0.0, |(_, p)| p.skill_attribute("skillManaCostReduction"));
    let mod_refs: Vec<(&Record, usize)> = mods.iter().map(|(m, ml)| (m, *ml)).collect();
    // Level 0 is all zero (`GetSkillModifierData(0)`).
    let energy = |lv: u32| {
        if lv == 0 {
            return 0.0;
        }
        crate::actor::skill_energy_cost(
            prof,
            lv as usize - 1,
            &mod_refs,
            rec.class == "Skill_ChargePotion",
            char_mana_cut,
            |_| false,
        )
    };
    // A listed value at a level, reduced as above.
    let value = |field: &str, lv: u32| match field {
        "skillManaCost" => energy(lv),
        "skillCooldownTime" if !is_mod => sum(field, lv) * (1.0 - cdr).max(0.0),
        _ => sum(field, lv),
    };
    let period = cost_period(ctx, rec, prof);
    for (i, &(field, cmp, tag, label)) in PARAMS.iter().enumerate() {
        if i == COSTS {
            out.extend(reserve_line(ctx, prof, mods, level, prev, class));
        }
        if field == "skillCooldownTime" {
            out.extend(combo_lines(
                ctx,
                is_mod,
                sum("skillComboChargeLevel", level),
                prev.map(|p| sum("skillComboChargeLevel", p)),
                class,
            ));
        }
        let v = value(field, level);
        let (shown, v) = match prev {
            // `SkillProfile_Modifiers::CreateUIText`: the charges only above one (a modifier's: any).
            None if field == "cooldownCharges" => (v > 1.0 || (is_mod && v != 0.0), v),
            None => (v != 0.0, v),
            Some(p) => {
                let old = value(field, p);
                (
                    if cmp == Cmp::Ne {
                        v != old
                    } else {
                        v - old > 0.0
                    },
                    v,
                )
            }
        };
        if !shown {
            continue;
        }
        let (v, label) = if field == "skillManaCost" && period > 0.0 {
            (v / period, "ManaCostPerSecond")
        } else {
            (v, label)
        };
        let label = if label == "@TargetRadius" {
            target_radius_tag(&rec.class)
        } else {
            label
        };
        let label = if label.is_empty() {
            String::new()
        } else {
            ctx.tag(label)
        };
        // `SkillProfile_Modifiers::CreateUIText`: a `Skill_ChargePotion`'s charges read
        // `PotionCooldownCharges[Mod]`.
        let tag = if field == "cooldownCharges" && prof.class == "Skill_ChargePotion" {
            mod_tag("PotionCooldownCharges", v)
        } else {
            mod_tag(tag, v)
        };
        let tag = tag.as_str();
        let text = match tag {
            // A permanent buff holds a duration of -1 s (Blade Barrier's `mi` modifier).
            "SkillSecondFormat" if field == "skillActiveDuration" && v <= -1.0 => {
                ctx.fmt("tagPermanentDuration", &[])
            }
            "TargetNumber"
            | "TargetNumberMod"
            | "ProjectileLaunchNumber"
            | "ProjectileLaunchNumberMod"
            | "BeamNumber"
            | "BeamNumberMod"
            | "CooldownCharges"
            | "CooldownChargesMod"
            | "PotionCooldownCharges"
            | "PotionCooldownChargesMod" => ctx.fmt(tag, &[Arg::Num(v)]),
            _ => ctx.fmt(tag, &[Arg::Num(v), Arg::Str(&label)]),
        };
        out.push(line(class, text));
    }
    // `SkillProfile_Modifiers::CreateUIProjectileText`: the fragment count
    // (`projectileFragmentsLaunchNumberMin/Max`).
    let frag = |f: &str, lv: u32| sum(f, lv).round() as i32;
    let (lo, hi) = (
        frag("projectileFragmentsLaunchNumberMin", level),
        frag("projectileFragmentsLaunchNumberMax", level),
    );
    let old = prev.map(|p| {
        (
            frag("projectileFragmentsLaunchNumberMin", p),
            frag("projectileFragmentsLaunchNumberMax", p),
        )
    });
    if hi > 0 && old != Some((lo, hi)) {
        let (key, args) = if lo == hi {
            ("ProjectileFragmentsLaunchNumber", vec![Arg::Num(lo as f32)])
        } else {
            (
                "ProjectileFragmentsLaunchNumberMinMax",
                vec![Arg::Num(lo as f32), Arg::Num(hi as f32)],
            )
        };
        out.push(line(class, ctx.fmt(key, &args)));
    }
    // `SkillProfile_Modifiers::CreateUIProjectileText` goes on with the projectile modifiers' chance to pass
    // through (`projectilePiercingChance`, loaded by `SkillProfile_Modifiers::LoadProjectileModifiers`).
    let pierce = sum("projectilePiercingChance", level);
    if pierce > 0.0 && changed("projectilePiercingChance", pierce) {
        out.push(line(
            class,
            ctx.fmt("ProjectilePiercingChance", &[Arg::Num(pierce)]),
        ));
    }
    // `SkillProfile_Modifiers::CreateUIText`'s end: the sparks (`sparkMaxNumber`, `sparkChance`).
    let (n, chance) = (sum("sparkMaxNumber", level), sum("sparkChance", level));
    if n != 0.0 && (changed("sparkMaxNumber", n) || changed("sparkChance", chance)) {
        let single = if n <= 1.0 { "Single" } else { "" };
        let full = chance >= 100.0;
        let key = format!(
            "tagSparkMaxNumber{}{single}{}",
            if full { "" } else { "Chance" },
            if is_mod { "Mod" } else { "" }
        );
        let args = if full {
            vec![Arg::Num(n)]
        } else {
            vec![Arg::Num(chance), Arg::Num(n)]
        };
        out.push(line(class, ctx.fmt(&key, &args)));
    }
    out
}
