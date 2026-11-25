//! The devotion window's star tooltip (`GameEngine::GenerateUIDevotionText`).
use super::*;

/// Why a devotion star is not taken (the lines after the stats,
/// `GenerateUIDevotionText`'s `SkillReasons` part).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DevStatus {
    /// Taken: `ActiveDevotion`.
    #[default]
    Taken,
    /// Can be taken: `PressToActivateDevotion`.
    Available,
    /// `NoDevotionPointsToAdd`.
    NoPoints,
    /// `LockedDevotion` (a star earlier in the constellation is needed).
    Locked,
    /// `FailedDevotionDependency`: the constellation's required affinities
    /// (affinity index, required, player's value).
    Affinity(Vec<(usize, u32, u32)>),
}

/// The devotion window's reclaim mode line of a star (`GenerateUIDevotionText` `a6`, flags
/// from GrimDawn.exe `DevotionWindow::GetStarReclaimState` / `DevotionWindow::Update` (the aether flag),
/// first that holds): untaken, the price above
/// the money, the aether cost above the crystals held, the constellation's affinity needed by
/// constellations (itself: self locked), a star that a taken star needs, else the price.
#[derive(Debug, Clone, PartialEq)]
pub enum DevReclaim {
    /// `tagReclaimNoPoints` (the star is not taken).
    NoPoints,
    /// `tagReclaimNoGold` with the gold price.
    NoGold,
    /// `tagReclaimNoAether` with the aether price.
    NoAether,
    /// `FailedConstellationSelfLocked`.
    SelfLocked,
    /// `FailedConstellationDependency` and the depending constellations' name tags.
    Dependency(Vec<String>),
    /// `tagReclaimBase`: a taken star of the constellation needs it.
    Base,
    /// `tagReclaimDevotionPoint` with both prices.
    Point,
}

/// What the devotion tooltip reads from the game state.
#[derive(Debug, Clone, Default)]
pub struct DevotionTip {
    /// The skill the power is bound to (record path).
    pub bound: Option<String>,
    /// The power's devotion level (save `devotion_level`; 0 / 1 = level 1).
    pub level: u32,
    /// Its experience (save `devotion_experience`).
    pub experience: u32,
    pub status: DevStatus,
    /// The constellation's completion bonus: (affinity index, amount).
    pub bonus: Vec<(usize, u32)>,
    /// In the reclaim mode: its line and the prices (gold, aether), `GetCurrentDevotionReclamationCost` /
    /// `GetDevotionReclamationAetherCost`.
    pub reclaim: Option<(DevReclaim, u64, u32)>,
}

/// The tooltip of a devotion star's skill (`GameEngine::GenerateUIDevotionText`:
/// heading, exclusive, "Bound To", current level
/// (`tagDevotionSkillRank`), experience (`tagDevotionExperienceFormat`, the
/// next level's `skillExperienceLevels` entry via `Skill::GetRequiredExperience`),
/// next level, `ActiveDevotion` / the reason, the constellation bonus with
/// `GetAffinityBitmap` icons). In the reclaim mode (`a6`, [`DevotionTip::reclaim`]): no
/// `MaxSkillLevelReached` stop, no next level, and the reclaim line ([`DevReclaim`], class 52, the
/// price line 51) ends the tooltip instead of the status and the bonus. PORT: a celestial power is a skill with an
/// auto-cast controller (`SkillAutoCastController.kind == 3` / `Skill.isAutoCastChild`); the blank lines follow the
/// real tooltip; the chance is not reduced by cooldown reduction
/// (`CalculateNewChanceToRun` only runs for item skills with `Skill.autoCastKind == 3`).
pub fn devotion_lines(ctx: &SkillTipCtx, skill: &str, tip: &DevotionTip) -> Vec<TextLine> {
    // The profile record (a wrapper's `buffSkillName`): levels, controller,
    // pet bonus.
    let Some(rec) = crate::skills::profile_record(ctx.db, skill) else {
        return Vec::new();
    };
    let disp = display_record(ctx.db, skill).unwrap_or_else(|| rec.clone());
    let power = is_power(&rec);
    let levels = rec.f32s("skillExperienceLevels");
    // `SkillProfile::LoadProfile`: the ultimate level is the table's length
    // when it has more than one entry.
    let max = if levels.len() > 1 {
        levels.len() as u32
    } else {
        rec.i32("skillUltimateLevel").unwrap_or(1).max(1) as u32
    };
    let taken = tip.status == DevStatus::Taken;
    // `Skill::GetCurrentLevel`: the devotion level above 1, else the learned one.
    let cur = if !taken {
        0
    } else if tip.level > 1 {
        tip.level.min(max)
    } else {
        1
    };
    let mut out = Vec::new();
    let name = ctx.text(disp.str("skillDisplayName"));
    let desc = ctx.text(disp.str("skillBaseDescription"));
    if power {
        out.push(line(
            class::EFFECT_NAME,
            format!("{name}{}", autocast_suffix(ctx, &rec)),
        ));
        if !desc.is_empty() {
            out.push(line(class::EFFECT_DESCRIPTION, desc));
        }
    } else {
        if !name.is_empty() {
            out.push(line(class::SKILL_NAME, name));
        }
        if !desc.is_empty() {
            out.push(line(class::SKILL_DESCRIPTION, desc));
        }
    }
    if rec.i32("exclusiveSkill").unwrap_or(0) != 0 {
        out.push(line(
            class::SKILL_DESCRIPTION,
            ctx.fmt("tagExclusiveSkill", &[]),
        ));
    }
    if !out.is_empty() {
        out.push(line(0, ""));
    }
    if power && let Some(b) = &tip.bound {
        out.push(line(
            class::EFFECT_HEADING,
            ctx.fmt("tagDevotionAttached", &[]),
        ));
        let bn = display_record(ctx.db, b)
            .and_then(|r| r.str("skillDisplayName").map(|t| ctx.tag(t)))
            .unwrap_or_default();
        out.push(line(class::EFFECT_NAME, bn));
        out.push(line(0, ""));
    }
    let (cur_class, next_class) = if power {
        (class::EFFECT_STATS_CURRENT, class::EFFECT_STATS_NEXT)
    } else {
        (class::SKILL_STATS_CURRENT, class::SKILL_STATS_NEXT)
    };
    let ranked = max > 1;
    if cur > 0 {
        if ranked {
            out.push(line(
                class::SKILL_LEVEL_TITLES,
                ctx.fmt(
                    "tagDevotionSkillRank",
                    &[Arg::Num(cur as f32), Arg::Num(max as f32)],
                ),
            ));
        }
        out.extend(stat_lines_with(ctx, skill, cur, cur_class, taken));
        out.extend(devotion_pet_lines(ctx, &rec, cur, false));
        out.push(line(0, ""));
    }
    if power && ranked {
        if cur >= max && tip.reclaim.is_none() {
            out.push(line(
                class::SKILL_REQUIREMENTS,
                ctx.fmt("MaxSkillLevelReached", &[]),
            ));
            return collapse_blanks(out);
        }
        // `GetRequiredExperience(min(max(level + 1, 2), max))`: entry `level - 1`.
        let want = tip.level.saturating_add(1).max(2).min(max);
        let required = levels.get(want as usize - 1).copied().unwrap_or(0.0) as u32;
        out.push(line(
            class::SKILL_REQUIREMENTS,
            ctx.fmt(
                "tagDevotionExperienceFormat",
                &[Arg::Num(tip.experience as f32), Arg::Num(required as f32)],
            ),
        ));
        out.push(line(0, ""));
    }
    if let Some((r, gold, aether)) = &tip.reclaim {
        let (gold, aether) = (
            crate::reclamation::money(*gold),
            crate::reclamation::money(*aether as u64),
        );
        let not_met = |t: String| line(class::SKILL_REQUIREMENTS_NOT_MET, t);
        out.push(line(0, ""));
        match r {
            DevReclaim::NoPoints => out.push(not_met(ctx.fmt("tagReclaimNoPoints", &[]))),
            DevReclaim::NoGold => {
                out.push(not_met(ctx.fmt("tagReclaimNoGold", &[Arg::Str(&gold)])))
            }
            DevReclaim::NoAether => {
                out.push(not_met(ctx.fmt("tagReclaimNoAether", &[Arg::Str(&aether)])))
            }
            DevReclaim::SelfLocked => {
                out.push(not_met(ctx.fmt("FailedConstellationSelfLocked", &[])))
            }
            DevReclaim::Dependency(names) => {
                out.push(not_met(ctx.fmt("FailedConstellationDependency", &[])));
                out.extend(names.iter().map(|n| not_met(ctx.tag(n))));
            }
            DevReclaim::Base => out.push(not_met(ctx.fmt("tagReclaimBase", &[]))),
            DevReclaim::Point => out.push(line(
                class::SKILL_REQUIREMENTS,
                ctx.fmt(
                    "tagReclaimDevotionPoint",
                    &[Arg::Str(&gold), Arg::Str(&aether)],
                ),
            )),
        }
        return collapse_blanks(out);
    }
    if cur == 0 || power {
        if ranked && power {
            out.push(line(
                class::SKILL_LEVEL_TITLES,
                ctx.fmt("tagNextLevel", &[Arg::Num((cur + 1) as f32)]),
            ));
        }
        out.extend(stat_lines_next_with(
            ctx,
            skill,
            cur,
            cur + 1,
            next_class,
            taken,
        ));
        out.extend(devotion_pet_lines(ctx, &rec, cur + 1, true));
        out.push(line(0, ""));
    }
    match &tip.status {
        DevStatus::Taken => out.push(line(
            class::SKILL_REQUIREMENTS,
            ctx.fmt("ActiveDevotion", &[]),
        )),
        DevStatus::Available => out.push(line(
            class::SKILL_REQUIREMENTS,
            ctx.fmt("PressToActivateDevotion", &[]),
        )),
        DevStatus::NoPoints => out.push(line(
            class::SKILL_REQUIREMENTS_NOT_MET,
            ctx.fmt("NoDevotionPointsToAdd", &[]),
        )),
        DevStatus::Locked => out.push(line(
            class::SKILL_REQUIREMENTS_NOT_MET,
            ctx.fmt("LockedDevotion", &[]),
        )),
        DevStatus::Affinity(req) => {
            out.push(line(
                class::SKILL_REQUIREMENTS_NOT_MET,
                ctx.fmt("FailedDevotionDependency", &[]),
            ));
            for &(a, need, have) in req {
                let mut l = line(
                    if have < need {
                        class::SKILL_REQUIREMENTS_NOT_MET
                    } else {
                        class::BASE_STATS
                    },
                    need.to_string(),
                );
                l.icon = Some(a);
                out.push(l);
            }
        }
    }
    if !tip.bonus.is_empty() {
        out.push(line(0, ""));
        out.push(line(
            class::SKILL_REQUIREMENTS,
            ctx.fmt("tagDevotionAffinityBonus", &[]),
        ));
        for &(a, v) in &tip.bonus {
            let mut l = line(class::BASE_STATS, v.to_string());
            l.icon = Some(a);
            out.push(l);
        }
    }
    collapse_blanks(out)
}

/// The pet bonus block of the devotion tooltip: `Skill::CreateUIPetBonusText` adds a blank
/// line before the title (`a6 == 0`).
fn devotion_pet_lines(ctx: &SkillTipCtx, rec: &Record, level: u32, next: bool) -> Vec<TextLine> {
    let mut v = pet_bonus_lines(ctx, rec, level, next);
    if !v.is_empty() {
        v.insert(0, line(0, ""));
    }
    v
}
