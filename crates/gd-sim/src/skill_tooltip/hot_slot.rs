//! The HUD hot slot's and the skill pick list's rollovers, and the hot slot status.
use super::*;

/// What a hot-bar skill is waiting for (`Skill.availability`, read by `GameEngine::GenerateUISkillInfo`): 1 its
/// cooldown, 2 energy, 4 its weapon configuration; anything else is ready ([`hot_state`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotState {
    Ready,
    Cooldown,
    LowMana,
    Configuration,
}

/// The state `Skill::SetAvailability` stores in `Skill.availability` (every frame from `Skill::Update`):
/// not enough energy ([`energy_cost`] above the current energy, `Skill::IsManaAvailable`) → 2 `LowMana`;
/// else the cooldown running or no charge left (`GetCooldownCharges` < 1) → 1 `Cooldown`; else the worn
/// weapons not suiting it (`QualifyingWeapon` / `QualifyingHandState`) → 4 `Configuration`; else 0 ready
/// (5 for level 0 and 7 outside the skill's shapeshift form also read as ready). `energy_ok`:
/// `Skill.ignoreMana` ([`ignores_energy`]) or the energy check.
pub fn hot_state(energy_ok: bool, cooling: bool, weapons_ok: bool) -> HotState {
    if !energy_ok {
        HotState::LowMana
    } else if cooling {
        HotState::Cooldown
    } else if !weapons_ok {
        HotState::Configuration
    } else {
        HotState::Ready
    }
}

/// The `Skill.ignoreMana` byte: the energy check of [`hot_state`] is skipped. Set for passives
/// (`Skill_Passive` and kin), `Skill_PlayAttackAnimation` and `SkillBuff*`; the toggles copy their on
/// byte into it in `ActivateNow`. `active` = the skill is on.
pub fn ignores_energy(class: &str, active: bool) -> bool {
    match class {
        "Skill_Passive"
        | "Skill_PassiveDualWieldWeapon"
        | "Skill_PassiveShield"
        | "Skill_Mastery"
        | "Skill_PlayAttackAnimation" => true,
        "Skill_BuffSelfToggled"
        | "Skill_BuffAttackRadiusToggled"
        | "Skill_OnHitAttackRadius"
        | "Skill_OnHitBuffSelf"
        | "Skill_Shapeshift" => active,
        c => c.starts_with("SkillBuff"),
    }
}

/// The energy `Skill::IsManaAvailable` asks for: `skillManaCost` of
/// `GetSkillModifierData` at the current level (the profile plus its learned modifiers) × (1 −
/// `GetTotalSkillAttribute(2)` %, `skillManaCostReduction`: the skill's own record and modifiers, value plus
/// |value| × `…Modifier` %, plus the player's), floored at 0; an item skill (`Skill.isItemSkill`, here
/// `item_skill`) or a potion (`Skill.isPotion`, `Skill_ChargePotion`) takes the cost unreduced. A reduction with a
/// `skillManaCostReductionChance` inside (0, 100) never counts (`IsManaAvailable` asks without the
/// rolled chance part).
pub fn energy_cost(ctx: &SkillTipCtx, skill: &str, item_skill: bool) -> f32 {
    let (Some(rec), Some(prof)) = (ctx.db.get(skill), profile_record(ctx.db, skill)) else {
        return 0.0;
    };
    let disp = display_record(ctx.db, skill).unwrap_or_else(|| rec.clone());
    let (_, _, cur, _) = levels(ctx, skill, &disp);
    // Level 0 is all zero (`GetSkillModifierData(0)`); the cost function's
    // index 0 is level 1, so an unlearned skill never reaches it.
    if cur == 0 {
        return 0.0;
    }
    let mods = learned_mods(ctx, skill, rec);
    let mods: Vec<(&Record, usize)> = mods.iter().map(|(m, ml)| (m, *ml)).collect();
    let char_reduction = ctx
        .player
        .map_or(0.0, |(_, p)| p.skill_attribute("skillManaCostReduction"));
    crate::actor::skill_energy_cost(
        &prof,
        cur as usize - 1,
        &mods,
        item_skill || rec.class == "Skill_ChargePotion",
        char_reduction,
        |_| false,
    )
}

/// Whether a skill attribute with chance `chance` (%) is a plain value, not rolled (
/// `SkillAttribute::GetTotalValue`: a chance inside (0, 100) is rolled).
pub fn plain_value(chance: f32) -> bool {
    chance <= 0.0 || chance >= 100.0
}

/// The rollover of the evade and potion hot slots: [`hot_slot_lines`] of their default skill at level 1.
/// `PlayerHotSlotCtrl::CreateHotSlotOption`: slot types 2 / 3 are
/// `HotSlotOptionPotionSkill`, 5 `HotSlotOptionEvade`, and both keep `HotSlotOptionSkill::GetRolloverText`;
/// `SkillManager::LoadDefaultPlayerSkills` loads `defaulthealthpotion` / `defaultmanapotion` /
/// `defaultevade` at level 1. Neither prints a level header ([`current_block`]).
pub fn default_hot_slot_lines(ctx: &SkillTipCtx, skill: &str, state: HotState) -> Vec<TextLine> {
    let mut book = ctx.book.clone();
    if book.level(skill) == 0 {
        book.skills.push((skill.to_string(), 1));
        book.touched();
    }
    let ctx = SkillTipCtx {
        book: &book,
        ..*ctx
    };
    hot_slot_lines(&ctx, skill, state, None)
}

/// The HUD hot slot's rollover. `HotSlotOptionSkill::GetRolloverText`: it sets the skill's hot bar view
/// (`Skill.isHotBarView`) and, without the slot's simple-text byte (`HotSlotOptionSkill.simpleText`,
/// `SetSimpleText`, set only by the skill pick list `SkillPickList::BuildLines`, [`pick_list_lines`]), prints
/// `GameEngine::GenerateUISkillInfo`: the heading (name 46, description 47), `tagExclusiveSkill` (47) for an
/// exclusive skill,
/// a blank, the current level's block when learned ([`current_block`]: no next level) and a blank, then
/// one status line: `tagSkillUnusableNotLearned` (52) for an unlearned non-item skill, else by `state`
/// `tagSkillUnusableCooldown` / `LowMana` / `Configuration` (52) or `tagSkillUsableReady` (51; an
/// unlearned item skill: `Configuration`). `granted`: the level of a skill that comes from gear (`Skill.isItemSkill`,
/// `SkillManager::AddItemSkills` sets it at the item's `itemSkillLevel`) or is granted for a while (a form's,
/// `SetAsGrantedSkillAuto`); both set `Skill.suppressLevelTitles` (`SetAsItemSkill[Auto]`),
/// so their block has no level header ([`current_block`]).
pub fn hot_slot_lines(
    ctx: &SkillTipCtx,
    skill: &str,
    state: HotState,
    granted: Option<u32>,
) -> Vec<TextLine> {
    let Some(rec) = ctx.db.get(skill) else {
        return Vec::new();
    };
    let item_skill = granted.is_some();
    let mut book = ctx.book.clone();
    if let Some(l) = granted {
        book.skills.retain(|(n, _)| !n.eq_ignore_ascii_case(skill));
        book.skills.push((skill.to_string(), l));
        book.touched();
    }
    let ctx = &SkillTipCtx {
        book: &book,
        ..*ctx
    };
    let disp = display_record(ctx.db, skill).unwrap_or_else(|| rec.clone());
    let mut out = heading_lines(ctx, &disp);
    if rec.i32("exclusiveSkill").unwrap_or(0) != 0 || disp.i32("exclusiveSkill").unwrap_or(0) != 0 {
        out.push(line(class::SKILL_DESCRIPTION, ctx.tag("tagExclusiveSkill")));
    }
    out.push(line(0, ""));
    let (_, _, cur, _) = levels(ctx, skill, &disp);
    out.extend(current_block(ctx, skill, rec, &disp, true, item_skill));
    let (cls, tag) = if !item_skill && cur == 0 {
        (
            class::SKILL_REQUIREMENTS_NOT_MET,
            "tagSkillUnusableNotLearned",
        )
    } else {
        match state {
            HotState::Cooldown => (
                class::SKILL_REQUIREMENTS_NOT_MET,
                "tagSkillUnusableCooldown",
            ),
            HotState::LowMana => (class::SKILL_REQUIREMENTS_NOT_MET, "tagSkillUnusableLowMana"),
            HotState::Configuration => (
                class::SKILL_REQUIREMENTS_NOT_MET,
                "tagSkillUnusableConfiguration",
            ),
            HotState::Ready if !item_skill || cur != 0 => {
                (class::SKILL_REQUIREMENTS, "tagSkillUsableReady")
            }
            HotState::Ready => (
                class::SKILL_REQUIREMENTS_NOT_MET,
                "tagSkillUnusableConfiguration",
            ),
        }
    };
    out.push(line(cls, ctx.tag(tag)));
    collapse_blanks(out)
}

/// A skill pick list entry's rollover (the simple-text path of `HotSlotOptionSkill::GetRolloverText`,
/// see [`hot_slot_lines`]): the heading (name 46, description 47), a blank, then
/// `CreateUIParameterText` (49, hot bar view) of a learned skill or `CreateUINextParameterText(-1)` (the
/// next level's block: `tagNextLevel` and its lines) of an unlearned one.
pub fn pick_list_lines(ctx: &SkillTipCtx, skill: &str) -> Vec<TextLine> {
    let Some(rec) = ctx.db.get(skill) else {
        return Vec::new();
    };
    let disp = display_record(ctx.db, skill).unwrap_or_else(|| rec.clone());
    let mut out = heading_lines(ctx, &disp);
    out.push(line(0, ""));
    let (base, _, cur, _) = levels(ctx, skill, &disp);
    if cur > 0 {
        out.extend(current_block(ctx, skill, rec, &disp, true, false));
    } else {
        out.push(line(
            class::SKILL_LEVEL_TITLES,
            ctx.fmt("tagNextLevel", &[Arg::Num((base + 1) as f32)]),
        ));
        out.extend(stat_lines_next(
            ctx,
            skill,
            cur,
            cur + 1,
            class::SKILL_STATS_NEXT,
        ));
    }
    collapse_blanks(out)
}
