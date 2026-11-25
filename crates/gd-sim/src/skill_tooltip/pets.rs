//! The summon block of a skill tooltip: limit, time to live, the pet's summary and skills
//! (`Skill::CreateUIPetText`).
use super::*;

/// `spawnObjects` record of a summon at `level`, a learned
/// `Skill_SpawnPetTransmuter`'s `modSpawnObjects` replacing it
/// (`Skill_SpawnPetTransmuter::PreLoadPet`; `spawnObjects`
/// holds its last entry for later levels).
fn spawn_record(prof: &Record, mods: &[(Record, usize)], level: u32) -> Option<String> {
    let pick = |r: &Record, f: &str| {
        let v = r.strs(f);
        v.get((level.max(1) as usize - 1).min(v.len().saturating_sub(1)))
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };
    mods.iter()
        .filter(|(m, _)| m.class == "Skill_SpawnPetTransmuter")
        .find_map(|(m, _)| pick(m, "modSpawnObjects"))
        .or_else(|| pick(prof, "spawnObjects"))
}

/// `Skill::CreateUI[Next]PetLimitText` and
/// `Skill_Modifier::CreateUIPetLimitText` (`...Mod` tags, any value > 0):
/// burst, limit.
fn pet_limit_lines(
    ctx: &SkillTipCtx,
    rec: &Record,
    prof: &Record,
    mods: &[(Record, usize)],
    level: u32,
    prev: Option<u32>,
    class: u8,
) -> Vec<TextLine> {
    let mut out = Vec::new();
    let is_mod = is_tooltip_modifier_class(&rec.class);
    let chaos = rec.class == "Skill_AttackSpellChaosSpawnPet";
    let sum = |f: &str, l: u32| total_f(prof, mods, f, l) as i32;
    let (burst, limit) = (sum("petBurstSpawn", level), sum("petLimit", level));
    let moved = |f: &str| prev.is_none_or(|p| sum(f, p) != sum(f, level));
    if moved("petBurstSpawn") {
        if is_mod && burst >= 1 {
            out.push(line(
                class,
                ctx.fmt(
                    if burst == 1 {
                        "SkillPetBurstSpawnMod"
                    } else {
                        "SkillPetBurstSpawnsMod"
                    },
                    &[Arg::Num(burst as f32)],
                ),
            ));
        } else if !is_mod && burst > 1 {
            out.push(line(
                class,
                ctx.fmt("SkillPetBurstSpawn", &[Arg::Num(burst as f32)]),
            ));
        }
    }
    if moved("petLimit") {
        if is_mod && limit != 0 {
            out.push(line(
                class,
                ctx.fmt("SkillPetLimitMod", &[Arg::Num(limit as f32)]),
            ));
        } else if !is_mod && (limit > 1 || chaos && limit != 0) {
            out.push(line(
                class,
                ctx.fmt("SkillPetLimit", &[Arg::Num(limit as f32)]),
            ));
        }
    }
    out
}

/// `tagSkillPetTimeToLive[Mod]`: [`crate::skills::spawn_time_to_live`] at the level.
fn pet_ttl_lines(
    ctx: &SkillTipCtx,
    rec: &Record,
    prof: &Record,
    mods: &[(Record, usize)],
    level: u32,
    prev: Option<u32>,
    class: u8,
) -> Vec<TextLine> {
    let listed: Vec<(String, usize, bool)> = mods
        .iter()
        .map(|(m, li)| (m.name.clone(), *li, true))
        .collect();
    // Level 0 has no data (`GetSkillModifierData(0)` is all zero, as `level_f`).
    let ttl = |l: u32| {
        (l > 0)
            .then(|| crate::skills::spawn_time_to_live(ctx.db, prof, l as usize - 1, &listed))
            .flatten()
    };
    // A transmuter shows (and compares) its own `modSpawnObjectsTimeToLive`.
    let at = |l: u32| {
        if rec.class == "Skill_SpawnPetTransmuter" {
            let t = level_f(prof, "modSpawnObjectsTimeToLive", l);
            (t > 0.0).then_some(t)
        } else {
            ttl(l)
        }
    };
    let Some(t) = at(level) else {
        return Vec::new();
    };
    if prev.is_some_and(|p| at(p) == Some(t)) {
        return Vec::new();
    }
    let tag = if is_tooltip_modifier_class(&rec.class) && rec.class != "Skill_SpawnPetTransmuter" {
        "tagSkillPetTimeToLiveMod"
    } else {
        "tagSkillPetTimeToLive"
    };
    vec![line(class, ctx.fmt(tag, &[Arg::Num(t)]))]
}

#[allow(clippy::too_many_arguments)]
pub(super) fn pet_lines(
    ctx: &SkillTipCtx,
    rec: &Record,
    prof: &Record,
    mods: &[(Record, usize)],
    level: u32,
    prev: Option<u32>,
    class: u8,
    depth: u8,
) -> Vec<TextLine> {
    let mut out = pet_limit_lines(ctx, rec, prof, mods, level, prev, class);
    let pet = spawn_record(prof, mods, level);
    // The next level's pet: only when its record differs.
    let pet = pet.filter(|p| {
        prev.is_none_or(|old| old == 0 || spawn_record(prof, mods, old).as_deref() != Some(p))
    });
    let Some(pet) = pet.and_then(|p| ctx.db.get(&p).map(|r| (p, r))) else {
        out.extend(pet_ttl_lines(ctx, rec, prof, mods, level, prev, class));
        return out;
    };
    let next = prev.is_some();
    out.push(line(0, ""));
    let title = if next {
        class::PET_BONUS_NEXT_TITLE
    } else {
        class::PET_BONUS_TITLE
    };
    let name = ctx.text(pet.1.str("description"));
    out.push(line(
        if next { class } else { title },
        ctx.fmt("SkillPetDescriptionHeading", &[Arg::Str(&name)]),
    ));
    out.extend(pet_ttl_lines(ctx, rec, prof, mods, level, prev, class));
    out.extend(pet_summary(ctx, &pet.0, pet.1, next, depth));
    out
}

/// The levels of a pet's skills as the summon gets them: `skillName{n}` at
/// `skillLevel{n}` (equations of `charLevel`), a learned pet modifier's level
/// instead, or a worn item's (level 1, [`crate::skills::gear_pet_modifiers`];
/// [`crate::skills::pet_skill_levels`]).
fn pet_skills(ctx: &SkillTipCtx, pet: &Record, level: i32) -> Vec<(String, u32)> {
    let mods: Vec<(String, u32)> = ctx
        .book
        .skills
        .iter()
        .filter_map(|(s, _)| {
            let r = ctx.db.get(s)?;
            if r.class != "SkillSecondary_PetModifier" {
                return None;
            }
            let lvl = ctx.book.effective_level(ctx.db, s);
            (lvl > 0).then(|| r.str("petSkillName").map(|p| (p.to_ascii_lowercase(), lvl)))?
        })
        .chain(ctx.book.gear_pet_mods.iter().cloned())
        .collect();
    crate::skills::pet_skill_levels(pet, level, &mods)
}

/// `Monster::CreateUI[Next]SummaryText`. Nothing without the
/// player: a summon's level is the player's.
fn pet_summary(
    ctx: &SkillTipCtx,
    record: &str,
    pet: &Record,
    next: bool,
    depth: u8,
) -> Vec<TextLine> {
    let Some((rules, player)) = ctx.player else {
        return Vec::new();
    };
    let types: Vec<String> = pet
        .strs("petTypeNames")
        .iter()
        .map(|s| s.to_string())
        .collect();
    let bonuses: Vec<(Record, usize)> = ctx
        .pet_bonuses
        .iter()
        .filter(|(b, _)| crate::skills::pet_bonus_applies(b, &types))
        .cloned()
        .collect();
    let Some(profile) = crate::actor::monster_with(
        ctx.db,
        rules,
        record,
        player.level,
        crate::actor::Side::Pet,
        &bonuses,
    ) else {
        return Vec::new();
    };
    let (stats, name_class, title) = if next {
        (
            class::PET_SKILL_STATS_NEXT,
            class::PET_SKILL_NAME_NEXT,
            class::PET_BONUS_NEXT_TITLE,
        )
    } else {
        (
            class::PET_SKILL_STATS_CURRENT,
            class::PET_SKILL_NAME_CURRENT,
            class::PET_BONUS_TITLE,
        )
    };
    let tidy = |s: String| if next { strip_colors(&s) } else { s };
    let mut out = Vec::new();
    // `PetPlayerScaling` summons hide both.
    let scaling = pet.class == "PetPlayerScaling";
    if !scaling {
        out.push(line(
            stats,
            tidy(ctx.fmt(
                "SkillPetDescriptionHealth",
                &[Arg::Num(profile.sheet.life.trunc())],
            )),
        ));
        if profile.sheet.energy > 0.0 {
            out.push(line(
                stats,
                tidy(ctx.fmt("SkillPetDescriptionMana", &[Arg::Num(profile.sheet.energy)])),
            ));
        }
    }
    out.push(line(0, ""));
    let name = ctx.text(pet.str("description"));
    out.push(line(
        title,
        ctx.fmt("tagSkillPetAbilities", &[Arg::Str(&name)]),
    ));
    let mut shown = false;
    for (skill, level) in pet_skills(ctx, pet, player.level) {
        let Some(rec) = ctx.db.get(&skill) else {
            continue;
        };
        let Some(prof) = profile_record(ctx.db, &skill) else {
            continue;
        };
        if level == 0 || !prof.bool("isPetDisplayable") {
            continue;
        }
        shown = true;
        // `CreateUIShortSummaryText`: `SkillManager` of the pet, no learned
        // modifiers in the player's book; damage only through the pet bonus.
        let name = ctx.text(prof.str("skillDisplayName"));
        if !name.is_empty() {
            out.push(line(name_class, name));
        }
        out.extend(section_of(
            ctx,
            &skill,
            rec,
            &prof,
            &[],
            level,
            None,
            stats,
            false,
            depth + 1,
        ));
        out.push(line(0, ""));
    }
    if !shown {
        let (lo, hi) = (
            pet.f32("handHitDamageMin").unwrap_or(0.0),
            pet.f32("handHitDamageMax").unwrap_or(0.0),
        );
        if lo > 0.0 || hi > 0.0 {
            let text = if lo == hi {
                ctx.fmt("SkillPetDescriptionDamageMinOnly", &[Arg::Num(lo)])
            } else {
                ctx.fmt(
                    "SkillPetDescriptionDamageMinMax",
                    &[Arg::Num(lo), Arg::Num(hi)],
                )
            };
            out.push(line(stats, tidy(text)));
        }
    }
    out
}
