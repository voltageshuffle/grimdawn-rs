//! Skill tooltip text (the skill window's hover text, reusable for other
//! skill-like things such as the devotion window's celestial powers).
//!
//! The text builder is `GameEngine::GenerateUISkillText` (Game.dll),
//! driven from the skill button's tooltip request
//! (GrimDawn.exe `SkillButton::RequestTooltip`: flags = the button's `SkillReasons`, text
//! class 49). Output is a list of [`TextLine`]s: a `GameTextClass` (
//! values from `GameEngine::LoadGameTextStyles`, [`class`]) plus text with the
//! tags' `^x` colour markup kept. Each class maps to a `gameengine.dbr` text
//! style key ([`style_key`]); the caller wraps and draws.
//!
//! Line order (`GenerateUISkillText`):
//! 1. `Skill::CreateUISkillHeading`: the name (class 46,
//!    `skillDisplayName`) and the description (class 47, `skillBaseDescription`);
//! 2. `tagExclusiveSkill` (class 47) when `exclusiveSkill` is set;
//! 3. `tagAutoToggledSkill` (class 47) when `IsAutoToggle` (
//!    every `SkillActivatedBuffSelf` descendant, [`crate::skills::ClassInfo::buff_self`]);
//! 4. a blank line when anything came before;
//! 5. learned skills: `Skill::CreateUIParameterText`: the
//!    `tagCurrentLevel[Augment|ExcessAugment]` header (class 48) then the
//!    attribute lines at the current level (class 49), then a blank line;
//! 6. unless maxed: `Skill::CreateUINextParameterText`: the
//!    `tagNextLevel[Augment|ExcessAugment]` header (class 48) and the
//!    attribute lines at the next level (class 50);
//! 7. with [`Reasons`] (the button's `SkillReasons`, `UISkillPane::GetRequirementFlags`): a blank
//!    line and the first applying message (mastery level, dependencies, base
//!    skill, exclusive conflict, points) in class 52 / 51. In reclamation
//!    mode ([`Reasons::reclaim`]) 6 is left out and 7 is the reclaim line
//!    (`crate::reclamation`).
//!
//! Levels: the current level is `Skill::GetCurrentLevel` (learned
//! level + item bonus, capped at the ultimate level; 0 unlearned) and the next
//! level one above it; the header shows `learned + 1` and the bonus.
//!
//! Level indices (`Skill::GetCurrentLevel`): an
//! unlearned skill is level 0 whatever its bonus levels, so "Next Level : 1 + 2"
//! reads array index 0 (level 1), never base + bonus + 1. The numbers of the
//! damage and retaliation lines then pass through the main player's profile
//! (`DamageAttributeStore::ProcessText` runs the skill's damage
//! through a `CombatDisplayAccumulator` with `GetMainPlayer`): Reckless Power
//! shows 75-188 Aether, not the record's 6-15 ([`scale_attr`]).
//!
//! Energy cost, cooldown, duration, radius, targets... are the
//! `SkillProfile_Modifiers` lines of `SkillProfile_Modifiers::CreateUIText` (current level, every
//! non-zero value) and `SkillProfile_Modifiers::CreateUINextText` (next level, what changed), reached
//! through `CreateUI[Next]ModifierText` before the attribute stores
//! ([`PARAMS`]). The base `CreateUINextSpecializedText1/2` hold other lines
//! (activation chance, retaliation / weapon damage %, absorption, potion
//! potency...).
//!
//! The data of every line is the skill's `SkillProfile`
//! (`Skill::GetSkillProfile`): a wrapper's `buffSkillName`
//! record (Curse of Frailty, Blood of Dreeg, Bloody Pox...), a
//! `SkillSecondary_PetModifier`'s pet skill ([`crate::skills::profile_record`]);
//! only the class-specific tags (radius label, per-second cost) follow the
//! skill object's own record.
//!
//! Section order (`Skill::CreateUIParameterText`, next
//! level `CreateUINextParameterText`, [`section`]):
//! `CreateUISpecializedText1` ([`first_lines`]);
//! `CreateUIModifierText` ([`param_lines`]; the `tagCharManaLimitReserve` line
//! of the `CharAttributeStore_SkillPenalty` store is handed to it and comes
//! after the costs); `CreateUISpecializedText2`
//! ([`special_lines`]: charges, projectiles, weapon / retaliation %, restores,
//! absorption, the pet block, skill enhancement); the stores (damage,
//! conversion, racial, character, defense, retaliation, skill); the refresh
//! lines ([`refresh_trigger_lines`], `tagSkillDurationRefresh*` /
//! `tagSkillCooldownRefresh*`), the penalty-reduction store; the pet bonus
//! block ([`pet_bonus_lines`], `Skill::CreateUIPetBonusText`); the current level ends with the "Celestial Power"
//! block of an auto-cast child ([`celestial_power_lines`]). The next level's text is the
//! same with `LocalizeStripColorTags`: no `^x` codes ([`strip_colors`]).
//!
//! Summons ([`pet_lines`] `Skill::CreateUIPetText` /
//! `CreateUINextPetText`): `SkillPetBurstSpawn` / `SkillPetLimit`,
//! a blank, `SkillPetDescriptionHeading` (class 70), `tagSkillPetTimeToLive`,
//! then `Monster::CreateUISummaryText` / `CreateUINextSummaryText`
//! (`pets::pet_summary`): the summon's health and energy, a blank,
//! `tagSkillPetAbilities` (class 70, next 72), per skill with a level and
//! `isPetDisplayable` `Skill::CreateUIShortSummaryText` (name in
//! class 53 / 54, the rest in 55 / 56) and a blank, or the pet's plain
//! `handHitDamage` when none is shown. The next level lists the pet only when
//! `spawnObjects` names another record there, else only the limit and time
//! (`CreateUINextSpecializedText2`). Without a pet record the limit and time
//! still show (a `Skill_Modifier`'s with their `...Mod` tags).
//!
//! Attribute lines are formatted with the item attribute tags
//! (`items::tooltip::attribute_line`), learned modifiers' included ([`section_of`]);
//! the potion, item-skill and reclaim variants are the lines above. The alternate pet text of
//! `Skill_AttackSpellChaosSpawnPet` is the plain one (`CreateUIAlternatePetText` is a
//! stub that falls through to `Skill::CreateUIPetText`). PORT: the `Skill.uiVariant` and
//! `GameEngine.uiVariant` flags are taken as clear. The weapon line's
//! "(a - b)" numbers are [`weapon_numbers`] (`Skill::CollectWeaponDamage`,
//! the character sheet's per-skill damage with the weapon %), the
//! retaliation line's `actor::display_retaliation`; both show only with
//! [`SkillTipCtx::player`] (else the plain `SkillWeaponDamageFormat` /
//! `SkillRetaliationDamageFormat`). The retaliation line is engine-gated on
//! Expansion 2 being loaded (taken as loaded). A summon's numbers need
//! [`SkillTipCtx::player`]; its level is the player's (the preloaded pet is
//! `Skill::PreLoadPet`, which sets no level: `Skill_SpawnPet::
//! SpawnPet` copies the caster's), its life takes the flat bonuses too (the
//! engine only the `characterLifeModifier` of the pet bonuses); its skills'
//! levels take the learned and the gear pet modifiers (`SkillBook::gear_pet_mods`,
//! as the summon gets them). Only the mastery window passes the pet
//! bonuses so far.
//! The devotion window's text is [`devotion_lines`]; the HUD hot slot's [`hot_slot_lines`] (its status by
//! [`hot_state`]; the evade and potion slots: [`default_hot_slot_lines`]) and the skill pick list's
//! [`pick_list_lines`] (`HotSlotOptionSkill::GetRolloverText`).
use crate::items::order::EQUIPMENT_ORDER;
use crate::items::resolve::{Attr, Source, StoreKind, record_attrs_at};
use crate::items::tooltip::attribute_line;
use crate::skills::{
    PENALTY_FIELDS, SkillBook, display_record, is_tooltip_modifier_class, profile_record,
};
use gd_formats::arz::Record;
use gd_formats::text::TagTable;
use gd_formats::vfs::Database;
use params::param_lines;
use pets::pet_lines;

mod devotion_tip;
mod hot_slot;
mod params;
mod pets;

pub use crate::items::tooltip::Arg;
pub use devotion_tip::{DevReclaim, DevStatus, DevotionTip, devotion_lines};
pub use hot_slot::{
    HotState, default_hot_slot_lines, energy_cost, hot_slot_lines, hot_state, ignores_energy,
    pick_list_lines, plain_value,
};

/// `GameTextClass` values (`GameEngine::LoadGameTextStyles`:
/// the id each `gameengine.dbr` style key is stored under).
pub mod class {
    pub const SKILL_NAME: u8 = 46;
    pub const SKILL_DESCRIPTION: u8 = 47;
    pub const SKILL_LEVEL_TITLES: u8 = 48;
    pub const SKILL_STATS_CURRENT: u8 = 49;
    pub const SKILL_STATS_NEXT: u8 = 50;
    pub const SKILL_REQUIREMENTS: u8 = 51;
    pub const SKILL_REQUIREMENTS_NOT_MET: u8 = 52;
    pub const PET_SKILL_NAME_CURRENT: u8 = 53;
    pub const PET_SKILL_NAME_NEXT: u8 = 54;
    pub const PET_SKILL_STATS_CURRENT: u8 = 55;
    pub const PET_SKILL_STATS_NEXT: u8 = 56;
    /// `ItemBaseStats` (the affinity number lines).
    pub const BASE_STATS: u8 = 18;
    pub const PET_BONUS_TITLE: u8 = 70;
    pub const PET_BONUS: u8 = 71;
    pub const PET_BONUS_NEXT_TITLE: u8 = 72;
    pub const PET_BONUS_NEXT: u8 = 73;
    pub const EFFECT_HEADING: u8 = 74;
    pub const EFFECT_NAME: u8 = 75;
    pub const EFFECT_DESCRIPTION: u8 = 76;
    pub const EFFECT_STATS_CURRENT: u8 = 77;
    pub const EFFECT_STATS_NEXT: u8 = 78;
}

/// `gameengine.dbr` key holding the text style of a class (order of
/// `LoadGameTextStyles`, shifted by the ids above).
pub fn style_key(class: u8) -> &'static str {
    match class {
        class::SKILL_NAME => "SkillName",
        class::SKILL_DESCRIPTION => "SkillDescription",
        class::SKILL_LEVEL_TITLES => "SkillLevelTitles",
        class::SKILL_STATS_CURRENT => "SkillStatsCurrent",
        class::SKILL_STATS_NEXT => "SkillStatsNext",
        class::SKILL_REQUIREMENTS => "SkillRequirements",
        class::SKILL_REQUIREMENTS_NOT_MET => "SkillRequirementsNotMet",
        class::PET_SKILL_NAME_CURRENT => "PetSkillNameCurrent",
        class::PET_SKILL_NAME_NEXT => "PetSkillNameNext",
        class::PET_SKILL_STATS_CURRENT => "PetSkillStatsCurrent",
        class::PET_SKILL_STATS_NEXT => "PetSkillStatsNext",
        class::BASE_STATS => "ItemBaseStats",
        class::PET_BONUS_TITLE => "PetBonusTitle",
        class::PET_BONUS => "PetBonus",
        class::PET_BONUS_NEXT_TITLE => "PetBonusNextTitle",
        class::PET_BONUS_NEXT => "PetBonusNext",
        class::EFFECT_HEADING => "EffectHeading",
        class::EFFECT_NAME => "EffectName",
        class::EFFECT_DESCRIPTION => "EffectDescription",
        class::EFFECT_STATS_CURRENT => "EffectStatsCurrent",
        class::EFFECT_STATS_NEXT => "EffectStatsNext",
        _ => "Default",
    }
}

/// Style record path of a class (`gameengine.dbr`), if it names one.
pub fn style_path(db: &Database, class: u8) -> Option<String> {
    db.get("records/game/gameengine.dbr")?
        .str(style_key(class))
        .map(str::to_string)
}

/// One text line: its `GameTextClass` and text (markup kept; empty = blank).
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    pub class: u8,
    pub text: String,
    /// Affinity icon drawn before the text (`GameTextLine`'s bitmap,
    /// `GameEngine::GetAffinityBitmap`: 0..4 = `devotion::AFFINITIES`).
    pub icon: Option<usize>,
}

fn line(class: u8, text: impl Into<String>) -> TextLine {
    TextLine {
        class,
        text: text.into(),
        icon: None,
    }
}

/// Why a skill cannot take a point, for the warning lines (`SkillReasons`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reasons {
    /// `[0]` no skill points left.
    pub no_points: bool,
    /// `[1]` mastery level too low: (additional points needed, mastery name tag).
    pub mastery_low: Option<(u32, String)>,
    /// `[2]` no base skill learned: the base skill's name tag.
    pub base_disabled: Option<String>,
    /// `[3]` maxed.
    pub maxed: bool,
    /// `[4]` the mastery slots are full (mastery skills only).
    pub masteries_full: bool,
    /// `[5]` unmet `skillDependancy`: (all required, dependency name tags
    /// still unlearned, count in the list).
    pub dependency: Option<(bool, Vec<String>, usize)>,
    /// `[6]` a stronger exclusive skill is active: its name tag.
    pub exclusive_conflict: Option<String>,
    /// The window is in reclamation mode (`crate::reclamation`): the reclaim line (`None`: nothing, f1) and the
    /// refund's cost; no next-level block.
    pub reclaim: Option<(Option<crate::reclamation::Tip>, u64)>,
}

/// Formats a tag text: `%[+][.p](f|d|t|s|g|z)N` arguments, `{`/`}` grouping
/// dropped, `^x` colour codes kept (the renderer interprets them). `%z` is
/// the time unit word (`tagSecond` / `tagSeconds`); the caller passes it as
/// a string argument.
pub fn format(fmt: &str, args: &[Arg]) -> String {
    let mut out = String::new();
    let mut b = fmt.chars().peekable();
    while let Some(x) = b.next() {
        match x {
            '{' | '}' => {}
            '%' if b
                .peek()
                .is_some_and(|c| matches!(c, '+' | '.' | 'f' | 'd' | 't' | 's' | 'g' | 'z')) =>
            {
                let mut plus = false;
                let mut prec = None;
                if b.peek() == Some(&'+') {
                    plus = true;
                    b.next();
                }
                if b.peek() == Some(&'.') {
                    b.next();
                    let mut p = String::new();
                    while let Some(d) = b.peek().filter(|c| c.is_ascii_digit()) {
                        p.push(*d);
                        b.next();
                    }
                    // (bad data: a huge `%.N` would allocate N digits; stock tags use 0-2.)
                    prec = p.parse::<usize>().ok().map(|p| p.min(9));
                }
                let conv = b.next().unwrap_or('t');
                let mut idx = String::new();
                while let Some(d) = b.peek().filter(|c| c.is_ascii_digit()) {
                    idx.push(*d);
                    b.next();
                }
                let i: usize = idx.parse().unwrap_or(0);
                let s = match (args.get(i), conv) {
                    (Some(Arg::Str(s)), _) => s.to_string(),
                    (Some(Arg::Num(v)), 'd') => format!("{}", *v as i64),
                    (Some(Arg::Num(v)), 't' | 'g') => {
                        if (v - v.round()).abs() < 0.05 {
                            format!("{}", v.round() as i64)
                        } else {
                            format!("{v:.1}")
                        }
                    }
                    // `%.1f` of 4 shows `4`,
                    // of 5.1 shows `5.1` (trailing zeros dropped).
                    (Some(Arg::Num(v)), _) => {
                        let t = format!("{:.*}", prec.unwrap_or(0), v);
                        if t.contains('.') {
                            t.trim_end_matches('0').trim_end_matches('.').to_string()
                        } else {
                            t
                        }
                    }
                    (None, _) => String::new(),
                };
                if plus && !s.starts_with('-') {
                    out.push('+');
                }
                out.push_str(&s);
            }
            other => out.push(other),
        }
    }
    out
}

/// What the builder reads.
pub struct SkillTipCtx<'a> {
    pub db: &'a Database,
    pub tags: &'a TagTable,
    pub book: &'a SkillBook,
    /// The main player's combat rules and profile: damage and retaliation
    /// lines are scaled by it like the engine does (`DamageAttributeStore::
    /// ProcessText`); `None` shows the record's raw values.
    pub player: Option<(&'a crate::actor::Rules, &'a crate::actor::Profile)>,
    /// The class and potion trees (`gd_game::data::GameData::skill_trees`).
    pub trees: &'a crate::skills::SkillTrees,
    /// The player's pet bonuses (`gd_game::pets::pet_bonuses`): a summon's
    /// health and its skills' damage take them, nothing else does; may be
    /// empty for a read-only text.
    pub pet_bonuses: &'a [(Record, usize)],
}

impl SkillTipCtx<'_> {
    pub fn tag(&self, key: &str) -> String {
        self.tags.get(key).unwrap_or(key).to_string()
    }

    /// A localised name / description given its tag.
    fn text(&self, tag: Option<&str>) -> String {
        tag.map(|t| self.tag(t)).unwrap_or_default()
    }

    fn fmt(&self, tag: &str, args: &[Arg]) -> String {
        format(self.tags.get(tag).unwrap_or(tag), args)
    }

    fn name_tag(&self, skill: &str) -> Option<String> {
        display_record(self.db, skill).and_then(|r| r.str("skillDisplayName").map(str::to_string))
    }
}

/// Skill level numbers of the tooltip: (learned level, bonus levels shown,
/// current level, max level). The current level is always
/// [`SkillBook::effective_level`] (what casting uses); only the shown bonus
/// is dropped as below.
fn levels(ctx: &SkillTipCtx, skill: &str, disp: &Record) -> (u32, u32, u32, u32) {
    let base = ctx.book.level(skill);
    let max = ctx
        .db
        .get(skill)
        .and_then(|r| r.i32("skillMaxLevel"))
        .filter(|m| *m > 0)
        .or(disp.i32("skillMaxLevel"))
        .unwrap_or(1)
        .max(0) as u32;
    // PORT: the real window shows no "+ n" for a skill whose ultimate level
    // equals its max level (Consecrated Blade 3/3, Wrath of Agrivix 0/1 stay
    // plain while 16-max skills read "16 + 2"); where the engine drops the
    // augment from the text is not located (`Skill::SetAugmentedSkillLevel`
    // stores it). Only the "+ n" goes: the current level keeps the bonus
    // levels, as `Skill::GetCurrentLevel` does for the cast.
    let augment = ctx
        .book
        .bonus
        .get(&skill.to_ascii_lowercase())
        .copied()
        .unwrap_or(0);
    let bonus = if ctx.book.ultimate_level(ctx.db, skill) > max {
        augment
    } else {
        0
    };
    let cur = ctx.book.effective_level(ctx.db, skill);
    (base, bonus, cur, max)
}

/// `UISkillPane::IsMaxed`: whether the skill can take no more levels.
pub fn is_maxed(ctx: &SkillTipCtx, skill: &str) -> bool {
    let Some(disp) = display_record(ctx.db, skill) else {
        return false;
    };
    let (base, bonus, cur, max) = levels(ctx, skill, &disp);
    if bonus > 0 {
        base >= max || cur >= ctx.book.ultimate_level(ctx.db, skill)
    } else {
        base >= max
    }
}

/// The bonus levels the skill shows as "+ n" (the tooltip's and the skill window's; 0 for none, [`levels`]).
pub fn shown_bonus(ctx: &SkillTipCtx, skill: &str) -> u32 {
    display_record(ctx.db, skill).map_or(0, |disp| levels(ctx, skill, &disp).1)
}

/// Position of an attribute in the stores' creation order
/// (`items::order::EQUIPMENT_ORDER`:
/// Fire, Aether, Burn modifiers come in that order).
fn creation_rank(a: &Attr) -> usize {
    use crate::items::resolve::Creator;
    fn matches(c: &Creator, a: &Attr) -> bool {
        match c {
            Creator::Class { class, .. } => a.class == *class,
            Creator::Char { field, .. } | Creator::Skill { field } => {
                a.values.iter().any(|(k, _)| k == field)
            }
            Creator::BasePhysical => a.class == "DamageAttribute_BasePhysical",
            Creator::WeaponOnly(c) | Creator::NonWeaponOnly(c) => matches(c, a),
            _ => false,
        }
    }
    EQUIPMENT_ORDER
        .iter()
        .position(|(_, c)| matches(c, a))
        .unwrap_or(usize::MAX)
}

/// `15% Physical Damage converted to Aether Damage` lines of a record at a
/// level (`conversionInType/OutType/Percentage` and the `2` set; `tagDamageConversion`).
fn conversion_lines(ctx: &SkillTipCtx, rec: &Record, li: usize) -> Vec<String> {
    let mut out = Vec::new();
    for sfx in ["", "2"] {
        let pcts = rec.f32s(&format!("conversionPercentage{sfx}"));
        let (Some(i), Some(o)) = (
            rec.str(&format!("conversionInType{sfx}")),
            rec.str(&format!("conversionOutType{sfx}")),
        ) else {
            continue;
        };
        let Some(&pct) = pcts.get(li.min(pcts.len().saturating_sub(1))) else {
            continue;
        };
        if pct <= 0.0 || i.is_empty() || o.is_empty() {
            continue;
        }
        let (i, o) = (
            ctx.tag(&format!("tagConversion{i}")),
            ctx.tag(&format!("tagConversion{o}")),
        );
        out.push(ctx.fmt(
            "tagDamageConversion",
            &[Arg::Num(pct), Arg::Str(&i), Arg::Str(&o)],
        ));
    }
    out
}

/// Crowd-control reflexes (`RetaliationAttributeReflex_*`) take their
/// duration as text: a bare number before ` of ` gets its unit
/// (`1 Second of Stun Retaliation`), and a whole chance loses its `.0`.
fn fix_reflex(text: String) -> String {
    let text = match text.find(".0% ") {
        Some(p)
            if text[..p]
                .chars()
                .all(|c| c.is_ascii_digit() || c == '^' || c.is_ascii_alphabetic()) =>
        {
            format!("{}{}", &text[..p], &text[p + 2..])
        }
        _ => text,
    };
    let Some(p) = text.rfind(" of ") else {
        return text;
    };
    let head = &text[..p];
    let start = head.rfind(' ').map_or(0, |x| x + 1);
    match head[start..].parse::<f32>() {
        Ok(n) if text[p + 4..].contains("Retaliation") => {
            let unit = if (n - 1.0).abs() < 0.05 {
                "Second"
            } else {
                "Seconds"
            };
            format!("{} {unit}{}", head, &text[p..])
        }
        _ => text,
    }
}

/// One per-level value of a record, `None` level = 0 (the engine's
/// `SkillProfile::GetSkillModifierData(0)` is all zero); arrays hold their
/// last entry for later levels.
fn level_f(rec: &Record, field: &str, level: u32) -> f32 {
    if level == 0 {
        return 0.0;
    }
    let v = rec.f32s(field);
    v.get((level as usize - 1).min(v.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0)
}

/// A value of the profile plus the learned modifiers' (`SkillProfile_Modifiers::Accumulate`
/// accumulates `GetSkillModifierData` `Skill::GetSkillModifierData`).
fn total_f(prof: &Record, mods: &[(Record, usize)], field: &str, level: u32) -> f32 {
    level_f(prof, field, level)
        + if level > 0 {
            mods.iter()
                .map(|(m, ml)| level_f(m, field, *ml as u32 + 1))
                .sum::<f32>()
        } else {
            0.0
        }
}

/// Drops the `^x` colour codes (`LocalizeStripColorTags`: the next level's
/// lines take their colour from the class alone).
fn strip_colors(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut c = text.chars();
    while let Some(x) = c.next() {
        if x == '^' {
            c.next();
        } else {
            out.push(x);
        }
    }
    out
}

/// The combo charge lines (`SkillProfile_Modifiers::CreateUIText`): `ComboMaxChargeMod` on a
/// modifier, else the level count and each level's bonus
/// (`GameEngine::GetComboChargeMultiplier`,
/// `gameengine.dbr` `skillComboChargeMultipliers`).
fn combo_lines(
    ctx: &SkillTipCtx,
    is_mod: bool,
    levels: f32,
    old: Option<f32>,
    class: u8,
) -> Vec<TextLine> {
    if levels == 0.0 || old == Some(levels) {
        return Vec::new();
    }
    if is_mod {
        return vec![line(
            class,
            ctx.fmt("ComboMaxChargeMod", &[Arg::Num(levels)]),
        )];
    }
    let mut text = ctx.fmt(
        "SkillIntFormat",
        &[Arg::Num(levels), Arg::Str(&ctx.tag("ComboChargeLevels"))],
    );
    let table = ctx
        .db
        .get("records/game/gameengine.dbr")
        .map(|r| r.f32s("skillComboChargeMultipliers"))
        .unwrap_or_default();
    // (bad data: a record-driven count; stock combos have a handful of levels.)
    for n in 0..(levels as usize).min(64) {
        let mult = table
            .get(n.min(table.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0.0);
        text.push_str(&ctx.fmt(
            if n == 0 {
                "SkillChargeLevel"
            } else {
                "SkillChargeLevel2"
            },
            &[Arg::Num(mult.trunc())],
        ));
    }
    vec![line(class, text)]
}

/// `tagCharManaLimitReserve` (`Skill::CreateUIParameterText`): the
/// `CharAttributeStore_SkillPenalty` store's value and its modifiers' at the
/// level, × (1 + its % modifier).
fn reserve_line(
    ctx: &SkillTipCtx,
    prof: &Record,
    mods: &[(Record, usize)],
    level: u32,
    prev: Option<u32>,
    class: u8,
) -> Option<TextLine> {
    let at = |lv: u32| {
        total_f(prof, mods, "characterManaLimitReserve", lv)
            * (1.0 + total_f(prof, mods, "characterManaLimitReserveModifier", lv) * 0.01)
    };
    let v = at(level);
    let shown = match prev {
        None => v != 0.0,
        Some(p) => v != at(p),
    };
    shown.then(|| line(class, ctx.fmt("tagCharManaLimitReserve", &[Arg::Num(v)])))
}

/// `Skill::CreateUISpecializedText1` (expansion 3
/// moved its chance and life monitor lines to [`param_lines`]), next level
/// `CreateUINextSpecializedText1`: the activation chance
/// (`onHitActivationChance`, `SkillSecondary_OnHitBuffRadius`'s own record
/// holds it), `FilterCaster`, and the refresh skills' [`refresh_lines`].
fn first_lines(
    ctx: &SkillTipCtx,
    rec: &Record,
    prof: &Record,
    level: u32,
    prev: Option<u32>,
    class: u8,
) -> Vec<TextLine> {
    let mut out = refresh_lines(ctx, rec, level, prev, class);
    if level == 0 {
        return out;
    }
    let chance = |lv: u32| {
        if rec.class == "SkillSecondary_OnHitBuffRadius" {
            level_f(rec, "onHitActivationChance", lv)
        } else {
            level_f(prof, "onHitActivationChance", lv)
        }
    };
    let c = chance(level);
    if c > 0.0 && prev.is_none_or(|p| chance(p) != c) {
        out.push(line(
            class,
            ctx.fmt(
                "SkillPercentFormat",
                &[Arg::Num(c), Arg::Str(&ctx.tag("SkillActivationChance"))],
            ),
        ));
    }
    if prev.is_none() && prof.bool("filterCaster") {
        out.push(line(class, ctx.fmt("FilterCaster", &[])));
    }
    out
}

/// The `damageAbsorption` qualifiers (`SkillProfile::LoadProfile`
/// order): field, `formatQualifyingDamage` name tag.
const QUALIFIERS: [(&str, &str); 11] = [
    ("physicalDamageQualifier", "tagQualifyingDamagePhysical"),
    ("pierceDamageQualifier", "tagQualifyingDamagePierce"),
    ("lightningDamageQualifier", "tagQualifyingDamageLightning"),
    ("fireDamageQualifier", "tagQualifyingDamageFire"),
    ("coldDamageQualifier", "tagQualifyingDamageCold"),
    ("poisonDamageQualifier", "tagQualifyingDamagePoison"),
    ("lifeDamageQualifier", "tagQualifyingDamageLife"),
    ("bleedingDamageQualifier", "tagQualifyingDamageBleeding"),
    ("elementalDamageQualifier", "tagQualifyingDamageElemental"),
    ("aetherDamageQualifier", "tagQualifyingDamageAether"),
    ("chaosDamageQualifier", "tagQualifyingDamageChaos"),
];

/// The main player's per-hand weapon-skill damage ((min, max) main, off-hand),
/// `Skill::CollectWeaponDamage`: the display
/// accumulator `CollectWeaponParameters` builds per weapon
/// (the skill's local damage, the hand's weapon scaled by the weapon %,
/// modifiers, conversions, `ModifyDamage`), each attribute's `GetMinMaxValues`
/// summed, `floor(x + 0.5)`. The off-hand's attributes the main hand already
/// holds are dropped (`FilterSameSource`). PORT: the off-hand is the
/// profile's dual-wield hand; weapon-pool parents (`Skill_WPAttack`) and the
/// spell / buff branches use the same accumulator. Zeros without a player.
fn weapon_numbers(
    ctx: &SkillTipCtx,
    prof: &Record,
    mods: &[(Record, usize)],
    level: u32,
) -> ((f32, f32), (f32, f32)) {
    let Some((rules, p)) = ctx.player else {
        return ((0.0, 0.0), (0.0, 0.0));
    };
    let mut su = crate::actor::SkillUse::new(prof, level as usize - 1);
    su.mods = mods.iter().map(|(m, ml)| (m, *ml)).collect();
    let round = |(a, b): (f32, f32)| ((a + 0.5).floor(), (b + 0.5).floor());
    match p.off_hand() {
        Some(off) => {
            let (m, o) = crate::actor::display_damage_hands(rules, p, off, &Some(su));
            (round(m), round(o))
        }
        _ => (
            round(crate::actor::display_damage(rules, p, &Some(su))),
            (0.0, 0.0),
        ),
    }
}

/// The potency a modifier's restores are shown with. `Skill::CreateUISpecializedText2`
/// / `CreateUINextSpecializedText2`: when the skill object is a `Skill_Modifier`
/// kin (`Skill_PotionModifier`, `Skill_PotionContainer` included), each parent in its `Skill.parents` list
/// (`SkillManager::CreateAndLoadSkill` pushes the base skill there and the modifier into the
/// base's `Skill.modifiers`) gives `1 + (modifierPotency + potionTransfusion) × 0.01` of its `GetSkillData`
/// (its profile at its level plus every learned modifier at theirs; the last parent wins), and the
/// eight restore values (`skillLifeBonus` .. `skillManaPercentSlow` of `SkillProfile_Modifiers`)
/// are multiplied by it; any other skill: 1. The parent is the class tree's base skill
/// ([`crate::skills::SkillTrees::parents`]) or, for the potion modifiers, the player record's
/// `customPotionTree1/2` potion (the tree's first non-modifier entry, `Player::Load`). Stock: the gdx3
/// vials (`modifierPotency`) and transfusions (`potionTransfusion`) raise the other potion modifiers' text.
fn modifier_potency(ctx: &SkillTipCtx, skill: &str, rec: &Record) -> f32 {
    let kin =
        rec.class == "Skill_PotionContainer" || crate::skills::is_skill_modifier_class(&rec.class);
    if !kin {
        return 1.0;
    }
    let Some(parent) = ctx.trees.parent(skill, true) else {
        return 1.0;
    };
    let Some(prof) = crate::skills::profile_record(ctx.db, parent) else {
        return 1.0;
    };
    // A default potion is no learned skill: level 1 (`LoadDefaultPlayerSkills`).
    let level = ctx.book.effective_level(ctx.db, parent).max(1);
    let mods = modifiers_learned(ctx, parent);
    let v = |f: &str| total_f(&prof, &mods, f, level);
    1.0 + (v("modifierPotency") + v("potionTransfusion")) * 0.01
}

/// `skill`'s learned modifiers (record, level index): the book's learned skills whose parent
/// ([`crate::skills::SkillTrees::parent`] with the potion trees) is `skill`.
fn modifiers_learned(ctx: &SkillTipCtx, skill: &str) -> Vec<(Record, usize)> {
    ctx.book
        .modifiers_of(ctx.db, ctx.trees, skill)
        .into_iter()
        .filter_map(|(m, li)| ctx.db.get_owned(&m).map(|r| (r, li)))
        .collect()
}

/// A `Skill_ChargePotion`'s restore lines (its own `CreateUIParameterText`: it
/// inlines `CreateUISpecializedText2` with these in place of the generic restores). The eight restores
/// are the profile's alone at `level` (`Skill::GetSkillModifierData(level, 1, 1, 1)` skips the learned
/// and gear modifiers); `potionInfusion` / `potionTransfusion` / `modifierPotency` are the profile's
/// plus its learned modifiers' (`GetSkillModifierData`). An infusion (> 0) scales the life
/// bonus, turns the life % into a life % over time, the energy bonus into `tagBonusManaAbsSlow` and the
/// energy % into an energy % over time, all × `1 + infusion %`; a transfusion (> 0) drops every life and
/// energy line but the two buff-duration ones. Then `PotionPotency` (`…Mod` for a modifier kin) of a
/// positive potency.
fn charge_potion_restores(
    ctx: &SkillTipCtx,
    rec: &Record,
    prof: &Record,
    mods: &[(Record, usize)],
    level: u32,
    class: u8,
) -> Vec<TextLine> {
    let mut out = Vec::new();
    let own = |f: &str| level_f(prof, f, level);
    let sum = |f: &str| total_f(prof, mods, f, level);
    let num = |tag: &str, v: f32| line(class, ctx.fmt(tag, &[Arg::Num(v)]));
    let (mut life_abs, mut life_pct, mut life_slow) = (
        own("skillLifeBonus"),
        own("skillLifePercent"),
        own("skillLifePercentSlow"),
    );
    let (mut mana_abs, mut mana_pct, mut mana_slow) = (
        own("skillManaBonus"),
        own("skillManaPercent"),
        own("skillManaPercentSlow"),
    );
    let mut mana_abs_slow = 0.0;
    let infusion = sum("potionInfusion");
    if infusion > 0.0 {
        let f = infusion * 0.01 + 1.0;
        mana_abs_slow = f * mana_abs;
        mana_slow = f * mana_pct;
        life_slow = f * life_pct;
        life_abs *= f;
        (mana_abs, mana_pct, life_pct) = (0.0, 0.0, 0.0);
    }
    let transfusion = sum("potionTransfusion") > 0.0;
    if transfusion {
        (mana_abs, mana_pct, mana_slow, mana_abs_slow) = (0.0, 0.0, 0.0, 0.0);
    } else {
        if life_pct > 0.0 && life_abs > 0.0 {
            out.push(line(
                class,
                ctx.fmt(
                    "tagBonusLifePercentAbs",
                    &[Arg::Num(life_pct), Arg::Num(life_abs)],
                ),
            ));
        } else if life_pct > 0.0 {
            out.push(num("tagBonusLifePercent", life_pct));
        } else if life_abs > 0.0 {
            out.push(num("tagBonusLifeAbs", life_abs));
        }
        if life_slow > 0.0 {
            out.push(num("tagBonusLifePercentSlow", life_slow));
        }
    }
    for (f, tag) in [
        ("skillLifeBonusBuffDuration", "tagBonusLifeAbsBuff"),
        ("skillLifePercentBuffDuration", "tagBonusLifePercentBuff"),
    ] {
        if own(f) > 0.0 {
            out.push(num(tag, own(f)));
        }
    }
    if mana_pct > 0.0 && mana_abs > 0.0 {
        out.push(line(
            class,
            ctx.fmt(
                "tagBonusManaPercentAbs",
                &[Arg::Num(mana_pct), Arg::Num(mana_abs)],
            ),
        ));
    } else if mana_pct > 0.0 {
        out.push(num("tagBonusManaPercent", mana_pct));
    } else if mana_abs > 0.0 {
        out.push(num("tagBonusManaAbs", mana_abs));
    }
    if mana_abs_slow > 0.0 {
        out.push(num("tagBonusManaAbsSlow", mana_abs_slow));
    }
    if mana_slow > 0.0 {
        out.push(num("tagBonusManaPercentSlow", mana_slow));
    }
    let potency = sum("modifierPotency");
    if potency > 0.0 {
        let tag = if is_tooltip_modifier_class(&rec.class) {
            "PotionPotencyMod"
        } else {
            "PotionPotency"
        };
        out.push(num(tag, potency));
    }
    out
}

/// `Skill::CreateUISpecializedText2` / `CreateUINextSpecializedText2` (their
/// order and tags).
#[allow(clippy::too_many_arguments)]
fn special_lines(
    ctx: &SkillTipCtx,
    rec: &Record,
    prof: &Record,
    mods: &[(Record, usize)],
    level: u32,
    prev: Option<u32>,
    class: u8,
    potency: f32,
    depth: u8,
) -> Vec<TextLine> {
    let mut out = Vec::new();
    if level == 0 {
        return out;
    }
    let own = |f: &str, l: u32| level_f(prof, f, l);
    let sum = |f: &str, l: u32| total_f(prof, mods, f, l);
    let num = |tag: &str, v: f32| line(class, ctx.fmt(tag, &[Arg::Num(v)]));
    // `Skill_BuffAttackRadiusLightning::CreateUI[Next]SpecializedText2`
    // (its vtable `Skill_BuffAttackRadiusLightning::vftable`): before the base lines, `SkillSecondFormat`
    // of the profile's `skillTargetInterval` labelled
    // `Interval`; the next level only when it differs. PORT: Devastation's
    // `Skill_BuffAttackRadiusDrop` (no RTTI name; its `Skill_BuffAttackRadiusDrop::CreateUISpecializedText2` /
    // `CreateUINextSpecializedText2` pair has the same body) takes the same line.
    if matches!(
        prof.class.as_str(),
        "Skill_BuffAttackRadiusLightning" | "Skill_BuffAttackRadiusDrop"
    ) {
        let interval = sum("skillTargetInterval", level);
        if prev.is_none_or(|p| sum("skillTargetInterval", p) != interval) {
            out.push(line(
                class,
                ctx.fmt(
                    "SkillSecondFormat",
                    &[Arg::Num(interval), Arg::Str(&ctx.tag("Interval"))],
                ),
            ));
        }
    }
    // Charge levels: the count, then each multiplier on the same line.
    let charges = own("skillChargeLevel", level);
    if charges != 0.0 && prev.is_none_or(|p| own("skillChargeLevel", p) != charges) {
        let suffix = if prev.is_some() { "Mod" } else { "" };
        let mut text = ctx.fmt(
            "SkillIntFormat",
            &[
                Arg::Num(charges),
                Arg::Str(&ctx.tag(&format!("SkillChargeMax{suffix}"))),
            ],
        );
        let mults = prof.f32s("skillChargeMultipliers");
        if mults.is_empty() {
            text.push_str(&ctx.fmt(&format!("SkillChargeLevel{suffix}"), &[Arg::Num(100.0)]));
        }
        for (i, m) in mults.iter().take(charges as usize).enumerate() {
            let tag = if i == 0 {
                format!("SkillChargeLevel{suffix}")
            } else if prev.is_some() {
                "SkillChargeLevelMod2".to_string()
            } else {
                "SkillChargeLevel2".to_string()
            };
            text.push_str(&ctx.fmt(&tag, &[Arg::Num(m.trunc())]));
        }
        out.push(line(class, text));
    }
    let projectiles = |l: u32| {
        if prof.f32s("skillProjectileNumber").is_empty() || l == 0 {
            1.0
        } else {
            own("skillProjectileNumber", l)
        }
    };
    match prev {
        None if projectiles(level) > 1.0 => {
            out.push(num("ProjectileLaunchNumber", projectiles(level)))
        }
        Some(p) if projectiles(level) != projectiles(p) => {
            out.push(num("ProjectileLaunchNumber", projectiles(level)))
        }
        _ => {}
    }
    if !prof.f32s("weaponDamagePct").is_empty() {
        let pct = own("weaponDamagePct", level);
        // `Skill::CreateUISpecializedText2`: a pet
        // skill's / a zero % line has no numbers; else a main-hand line
        // (max > 0), an off-hand line (max > 0), or the plain one when both
        // maxima are 0.
        if pct > 0.0 && prof.bool("isPetBonusScaling") {
            out.push(num("SkillWeaponDamagePetFormat", pct));
        } else if pct <= 0.0 {
            out.push(num("SkillWeaponDamageFormat", pct));
        } else {
            let ((a, b), (c, d)) = weapon_numbers(ctx, prof, mods, level);
            let with = |tag: &str, lo: f32, hi: f32| {
                line(
                    class,
                    ctx.fmt(tag, &[Arg::Num(pct), Arg::Num(lo), Arg::Num(hi)]),
                )
            };
            if b > 0.0 {
                out.push(with("SkillWeaponDamageMainhandFormat", a, b));
            }
            if d > 0.0 {
                out.push(with("SkillWeaponDamageOffhandFormat", c, d));
            }
            if b == 0.0 && d == 0.0 {
                out.push(num("SkillWeaponDamageFormat", pct));
            }
        }
    }
    let retaliation = own("retaliationDamagePct", level);
    if retaliation > 0.0 {
        // `Skill::CollectRetaliationDamage`, the
        // "Numbers" tag when either sum is non-zero.
        let (lo, hi) = ctx.player.map_or((0.0, 0.0), |(rules, p)| {
            let mut su = crate::actor::SkillUse::new(prof, level as usize - 1);
            su.mods = mods.iter().map(|(m, ml)| (m, *ml)).collect();
            crate::actor::display_retaliation(rules, p, &su)
        });
        if lo == 0.0 && hi == 0.0 {
            out.push(num("SkillRetaliationDamageFormat", retaliation));
        } else {
            out.push(line(
                class,
                ctx.fmt(
                    "SkillRetaliationDamageFormatNumbers",
                    &[Arg::Num(retaliation), Arg::Num(lo), Arg::Num(hi)],
                ),
            ));
        }
    }
    // Restores: the level's values × `potency` ([`modifier_potency`]); the next level's "changed" test
    // compares the scaled value with the previous level's unscaled one (
    // `CreateUINextSpecializedText2` scales only the new values).
    // A charge potion's current level has its own rule ([`charge_potion_restores`]).
    let potion = prof.class == "Skill_ChargePotion" && prev.is_none();
    if potion {
        out.extend(charge_potion_restores(ctx, rec, prof, mods, level, class));
    }
    let restore = |f: &str| sum(f, level) * potency;
    let moved =
        |fs: &[&str]| !potion && prev.is_none_or(|p| fs.iter().any(|f| restore(f) != sum(f, p)));
    let (abs, pct) = (restore("skillLifeBonus"), restore("skillLifePercent"));
    if moved(&["skillLifeBonus", "skillLifePercent"]) {
        if pct > 0.0 && abs > 0.0 {
            out.push(line(
                class,
                ctx.fmt("tagBonusLifePercentAbs", &[Arg::Num(pct), Arg::Num(abs)]),
            ));
        } else if pct > 0.0 {
            out.push(num("tagBonusLifePercent", pct));
        } else if abs > 0.0 {
            out.push(num("tagBonusLifeAbs", abs));
        }
    }
    for (f, tag) in [
        ("skillLifePercentSlow", "tagBonusLifePercentSlow"),
        ("skillLifeBonusBuffDuration", "tagBonusLifeAbsBuff"),
        ("skillLifePercentBuffDuration", "tagBonusLifePercentBuff"),
    ] {
        let v = restore(f);
        if v > 0.0 && moved(&[f]) {
            out.push(num(tag, v));
        }
    }
    let (abs, pct) = (restore("skillManaBonus"), restore("skillManaPercent"));
    if moved(&["skillManaBonus", "skillManaPercent"]) {
        if pct > 0.0 && abs > 0.0 {
            out.push(line(
                class,
                ctx.fmt("tagBonusManaPercentAbs", &[Arg::Num(pct), Arg::Num(abs)]),
            ));
        } else if pct > 0.0 {
            out.push(num("tagBonusManaPercent", pct));
        } else if abs > 0.0 {
            out.push(num("tagBonusManaAbs", abs));
        }
    }
    let v = restore("skillManaPercentSlow");
    if v > 0.0 && moved(&["skillManaPercentSlow"]) {
        out.push(num("tagBonusManaPercentSlow", v));
    }
    // Absorption: the next level lists what grew.
    let grew = |v: f32, old: &dyn Fn(u32) -> f32| prev.map_or(v > 0.0, |p| v - old(p) > 0.0);
    let v = own("damageAbsorption", level);
    if grew(v, &|p| own("damageAbsorption", p)) {
        out.push(num("SkillDamageAbsorption", v));
    }
    let v = sum("damageAbsorptionPercent", level);
    if grew(v, &|p| sum("damageAbsorptionPercent", p)) {
        out.push(num("SkillDamageAbsorptionPercent", v));
    }
    let v = sum("absorbShieldPercentHealth", level);
    if grew(v, &|p| sum("absorbShieldPercentHealth", p)) {
        out.push(num("AbsorbShieldPercentHealth", v));
    }
    let quals: Vec<&str> = QUALIFIERS
        .iter()
        .filter(|(f, _)| prof.bool(f))
        .map(|q| q.1)
        .collect();
    if !quals.is_empty() && (prev.is_none() || level == 1) {
        out.push(line(class, ctx.fmt("tagDamageAbsorptionTitle", &[])));
        for q in quals {
            out.push(line(
                class,
                ctx.fmt("formatQualifyingDamage", &[Arg::Str(&ctx.tag(q))]),
            ));
        }
    }
    let v = own("damageAbsorptionReflectPercent", level);
    if grew(v, &|p| own("damageAbsorptionReflectPercent", p)) {
        out.push(num("SkillDamageAbsorptionReflectPercent", v));
    }
    out.extend(pet_lines(ctx, rec, prof, mods, level, prev, class, depth));
    let v = own("allSkillEnhancement", level);
    if v != 0.0 && prev.is_none_or(|p| own("allSkillEnhancement", p) != v) {
        out.push(num("SkillEnhancement", v));
    }
    if rec.class == "Skill_Shapeshift" {
        out.extend(shapeshift_lines(ctx, prof, level, prev, depth));
    }
    out
}

/// `Skill_Shapeshift::CreateUI[Next]SpecializedText2`:
/// `tagSkillShapeshiftAbilities` (class 70, next 72) and each
/// `grantedSkills` entry's short summary (the next level's with blanks around
/// each). `Skill::CreateUIShortSummaryText` at the form skill's `level` (`prev`: the
/// next-level text): a granted skill's level follows the form skill's
/// (`SkillManager::UpdateSubSkillLevels`, gd-game `shapeshift` docs).
fn shapeshift_lines(
    ctx: &SkillTipCtx,
    prof: &Record,
    level: u32,
    prev: Option<u32>,
    depth: u8,
) -> Vec<TextLine> {
    let next = prev.is_some();
    let granted = prof.strs("grantedSkills");
    let granted: Vec<&str> = granted.into_iter().filter(|s| !s.is_empty()).collect();
    if granted.is_empty() {
        return Vec::new();
    }
    let (title, name_class, stats) = if next {
        (
            class::PET_BONUS_NEXT_TITLE,
            class::PET_SKILL_NAME_NEXT,
            class::PET_SKILL_STATS_NEXT,
        )
    } else {
        (
            class::PET_BONUS_TITLE,
            class::PET_SKILL_NAME_CURRENT,
            class::PET_SKILL_STATS_CURRENT,
        )
    };
    let mut out = vec![line(0, "")];
    let own = ctx.text(prof.str("skillDisplayName"));
    out.push(line(
        title,
        ctx.fmt("tagSkillShapeshiftAbilities", &[Arg::Str(&own)]),
    ));
    if next {
        out.push(line(0, ""));
    }
    for skill in granted {
        let (Some(rec), Some(data)) = (ctx.db.get(skill), profile_record(ctx.db, skill)) else {
            continue;
        };
        let name = ctx.text(data.str("skillDisplayName"));
        if !name.is_empty() {
            out.push(line(name_class, name));
        }
        let mods = learned_mods(ctx, skill, rec);
        let lines = section_of(
            ctx,
            skill,
            rec,
            &data,
            &mods,
            level,
            prev,
            stats,
            true,
            depth + 1,
        );
        out.extend(lines);
        if next {
            out.push(line(0, ""));
        }
    }
    out
}

/// `tagSkillDurationRefresh*` / `tagSkillCooldownRefresh*` (
/// `Skill::CreateUIParameterText`, `Skill::LoadResources` reads
/// `refreshDurationSkill` / `...Trigger`, `refreshCooldownSkill` /
/// `...Trigger`; `SkillProfile_Modifiers::Load` the chance, amount, max): the trigger's
/// `tagRefreshSkillCondition<NN>` (`SkillAutoCastController::GetRefreshTag`,
/// the numbers of [`autocast_condition`]) with the chance, the
/// named skill unless it is the modifier's own base (`GetRefreshSkillTag`),
/// the amount and a duration's max. PORT: the next level
/// repeats a line when any value changed.
fn refresh_trigger_lines(
    ctx: &SkillTipCtx,
    skill: &str,
    rec: &Record,
    prof: &Record,
    level: u32,
    prev: Option<u32>,
    class: u8,
) -> Vec<TextLine> {
    let mut out = Vec::new();
    if level == 0 {
        return out;
    }
    let parent = ctx.trees.parent(skill, false);
    let source = |f: &str| {
        rec.str(f)
            .or_else(|| prof.str(f))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let name_of = |target: &str| {
        if parent.is_some_and(|p| p.eq_ignore_ascii_case(target)) {
            return String::new();
        }
        ctx.name_tag(target)
            .map(|t| ctx.tag(&t))
            .unwrap_or_default()
    };
    let moved = |fs: &[&str]| {
        prev.is_none_or(|p| {
            fs.iter()
                .any(|f| level_f(prof, f, level) != level_f(prof, f, p))
        })
    };
    if let (Some(target), Some(trigger)) = (
        source("refreshDurationSkill"),
        source("refreshDurationTrigger"),
    ) && moved(&[
        "refreshDurationChance",
        "refreshDurationAmount",
        "refreshDurationMax",
    ]) {
        let chance = level_f(prof, "refreshDurationChance", level);
        let cond = refresh_condition(&trigger)
            .map(|t| ctx.fmt(&t, &[Arg::Num(chance)]))
            .unwrap_or_default();
        let (name, amount, max) = (
            name_of(&target),
            level_f(prof, "refreshDurationAmount", level),
            level_f(prof, "refreshDurationMax", level),
        );
        let (unit, max_unit) = (seconds_unit(ctx, amount), seconds_unit(ctx, max));
        let text = match (name.is_empty(), max > 0.0) {
            (true, false) => ctx.fmt(
                "tagSkillDurationRefresh",
                &[Arg::Str(&cond), Arg::Num(amount), Arg::Str(&unit)],
            ),
            (false, false) => ctx.fmt(
                "tagSkillDurationRefreshName",
                &[
                    Arg::Str(&cond),
                    Arg::Str(&name),
                    Arg::Num(amount),
                    Arg::Str(&unit),
                ],
            ),
            (true, true) => ctx.fmt(
                "tagSkillDurationRefreshMax",
                &[
                    Arg::Str(&cond),
                    Arg::Num(amount),
                    Arg::Str(&unit),
                    Arg::Num(max),
                    Arg::Str(&max_unit),
                ],
            ),
            (false, true) => ctx.fmt(
                "tagSkillDurationRefreshNameMax",
                &[
                    Arg::Str(&cond),
                    Arg::Str(&name),
                    Arg::Num(amount),
                    Arg::Str(&unit),
                    Arg::Num(max),
                    Arg::Str(&max_unit),
                ],
            ),
        };
        out.push(line(class, text));
    }
    if let (Some(target), Some(trigger)) = (
        source("refreshCooldownSkill"),
        source("refreshCooldownTrigger"),
    ) && moved(&["refreshCooldownChance", "refreshCooldownAmount"])
    {
        let chance = level_f(prof, "refreshCooldownChance", level);
        let cond = refresh_condition(&trigger)
            .map(|t| ctx.fmt(&t, &[Arg::Num(chance)]))
            .unwrap_or_default();
        let (name, amount) = (
            name_of(&target),
            level_f(prof, "refreshCooldownAmount", level),
        );
        let unit = seconds_unit(ctx, amount);
        let text = if name.is_empty() {
            ctx.fmt(
                "tagSkillCooldownRefresh",
                &[Arg::Str(&cond), Arg::Num(amount), Arg::Str(&unit)],
            )
        } else {
            ctx.fmt(
                "tagSkillCooldownRefreshName",
                &[
                    Arg::Str(&cond),
                    Arg::Str(&name),
                    Arg::Num(amount),
                    Arg::Str(&unit),
                ],
            )
        };
        out.push(line(class, text));
    }
    out
}

/// The `%z` time unit word of `secs`: `tagSecond` for exactly 1 s, else `tagSeconds`.
fn seconds_unit(ctx: &SkillTipCtx, secs: f32) -> String {
    ctx.tag(if secs == 1.0 {
        "tagSecond"
    } else {
        "tagSeconds"
    })
}

/// `tagRefreshSkillCondition<NN>` of a refresh trigger type.
fn refresh_condition(trigger: &str) -> Option<String> {
    autocast_condition(trigger)
        .map(|t| t.replace("tagAutoSkillCondition", "tagRefreshSkillCondition"))
}

/// Learned modifiers of `skill` (record, level index): empty for a modifier.
fn learned_mods(ctx: &SkillTipCtx, skill: &str, rec: &Record) -> Vec<(Record, usize)> {
    if is_tooltip_modifier_class(&rec.class) {
        return Vec::new();
    }
    modifiers_learned(ctx, skill)
}

/// `petBonusName` record lines under a title: by the bonus's `petTypeNames` count `tagPetBonusNameAllPets`,
/// `...OnePet`, `...TwoPets`, else `...ManyPets` and a line per name. The current level is
/// `Skill::CreateUIPetBonusText` (from `CreateUIParameterText`): the caller's text class picks the classes
/// (26, 28, 19 / 33 / 41 and 81 / 82 remap them, `items::tooltip_item::pet_bonus_block`; any other, as the
/// skill classes 46-56, gives 70 / 71); the next level (`next`) is its sibling `Skill::CreateUINextPetBonusText`
/// (from `CreateUINextParameterText`, the skill and devotion windows' next level), which
/// always uses 72 / 73 (`PetBonusNextTitle` / `PetBonusNext`) and drops the title again when no line follows.
pub(crate) fn pet_bonus_lines(
    ctx: &SkillTipCtx,
    rec: &Record,
    level: u32,
    next: bool,
) -> Vec<TextLine> {
    let Some(bonus) = rec
        .str("petBonusName")
        .filter(|n| !n.is_empty())
        .and_then(|n| ctx.db.get(n))
    else {
        return Vec::new();
    };
    let (title, body) = if next {
        (class::PET_BONUS_NEXT_TITLE, class::PET_BONUS_NEXT)
    } else {
        (class::PET_BONUS_TITLE, class::PET_BONUS)
    };
    let mut stats = stat_lines_of(
        ctx,
        std::slice::from_ref(bonus),
        &[],
        level.saturating_sub(1) as usize,
        body,
        false,
    );
    if stats.is_empty() {
        return stats;
    }
    let names: Vec<String> = bonus
        .strs("petTypeNames")
        .iter()
        .map(|n| ctx.tag(n))
        .collect();
    let head = match names.as_slice() {
        [] => vec![ctx.fmt("tagPetBonusNameAllPets", &[])],
        [a] => vec![ctx.fmt("tagPetBonusNameOnePet", &[Arg::Str(a)])],
        [a, b] => vec![ctx.fmt("tagPetBonusNameTwoPets", &[Arg::Str(a), Arg::Str(b)])],
        many => std::iter::once(ctx.fmt("tagPetBonusNameManyPets", &[]))
            .chain(many.iter().cloned())
            .collect(),
    };
    for (i, t) in head.into_iter().enumerate() {
        stats.insert(i, line(title, t));
    }
    stats
}

/// Replaces a damage / retaliation attribute's values by the player-scaled
/// ones (PORT of `DamageAttributeStore::ProcessText` / the retaliation store:
/// `scaled` are the skill's processed hit / DoT damages, retaliation takes the
/// holder's retaliation modifiers).
fn scale_attr(
    a: &mut Attr,
    scaled: &[(u8, crate::attributes::CombatType, f32, f32)],
    prof: &crate::actor::Profile,
    rules: &crate::actor::Rules,
) {
    use crate::attributes::{AttrFamily as F, lookup};
    let Some(def) = a.values.first().and_then(|(k, _)| lookup(k)) else {
        return;
    };
    let (Some(fmin), Some(ty)) = (def.min, def.combat_type()) else {
        return;
    };
    let set = |a: &mut Attr, min: f32, max: f32| {
        for (k, v) in a.values.iter_mut() {
            if k.eq_ignore_ascii_case(fmin) {
                *v = (min + 0.5).floor();
            } else if def.max.is_some_and(|m| k.eq_ignore_ascii_case(m)) {
                *v = (max + 0.5).floor();
            }
        }
    };
    match (a.store, def.family) {
        (StoreKind::Damage, F::Offense | F::OffenseBase | F::OffenseDuration) => {
            let kind = if def.family == F::OffenseDuration {
                2
            } else {
                0
            };
            // `DamageAttributeStore::ProcessText`: each
            // damage attribute keeps the combat attributes it added to the
            // display accumulator; after the modifiers / conversions /
            // `Process`, its line reads `GetMinMaxValues` of each in turn into
            // the same pair, so the last one's values show, each
            // `floor(x + 0.5)`. An Elemental attribute adds Cold, Fire, then
            // Lightning thirds (`DamageAttributeAbs_ElementalDamage::
            // AddDamageToAccumulator`): its line shows the processed Lightning
            // third. PORT: the processed damages are not tied to their
            // attribute; other lines sum their type's entries.
            use crate::attributes::CombatType as T;
            let mut picked: Option<(f32, f32)> = None;
            if ty == T::Elemental {
                picked = scaled
                    .iter()
                    .rfind(|s| s.0 == kind && matches!(s.1, T::Cold | T::Fire | T::Lightning))
                    .map(|s| (s.2, s.3));
            } else {
                for s in scaled.iter().filter(|s| s.0 == kind && s.1 == ty) {
                    let (lo, hi) = picked.unwrap_or((0.0, 0.0));
                    picked = Some((lo + s.2, hi + s.3));
                }
            }
            if let Some((lo, hi)) = picked {
                set(a, lo, hi);
            }
        }
        (StoreKind::Retaliation, F::Retaliation) => {
            // PORT: the engine's tooltip is not traced for
            // `retaliationDamageMultModifier`; it is applied as the sheet and
            // combat do, through the one `actor::modify_retaliation`.
            use crate::combat::{AttackerScaling, Damage, DamageScale, HitDamage, ReductionInfo};
            let (lo, hi) = (
                a.get(fmin).unwrap_or(0.0),
                def.max.and_then(|m| a.get(m)).unwrap_or(0.0),
            );
            let mut d = [Damage::Hit(HitDamage {
                ty,
                value: lo,
                min: lo,
                max: hi,
                modifier: 0.0,
                scale_by_attributes: false,
            })];
            crate::actor::modify_retaliation(&mut d, &|k: &str| {
                prof.totals.get(k).copied().unwrap_or(0.0)
            });
            let scaling = AttackerScaling {
                formulas: &rules.formulas,
                vars: Default::default(),
            };
            if let [Damage::Hit(h)] = &mut d {
                h.process(&scaling, &ReductionInfo::default(), DamageScale::default());
                set(a, h.min, h.max);
            }
        }
        _ => {}
    }
}

/// `Skill_RefreshCooldown(Modifier)::CreateUI[Next]SpecializedText1`:
/// at a level > 0, `GetRefreshTime(level)` (the
/// `refreshTime` entry in ms, ≤ 0 → 0) as `tagSkillRefreshTime` (seconds),
/// or `tagSkillRefreshTimeInfinate` when 0. It comes before the modifier
/// lines (`CreateUIParameterText`: SpecializedText1, ModifierText,
/// SpecializedText2, then the stores). PORT: the next-level variant is
/// listed when the time differs from `prev`'s; `%z` reads `tagSecond` for
/// exactly 1 s, else `tagSeconds`.
fn refresh_lines(
    ctx: &SkillTipCtx,
    rec: &Record,
    level: u32,
    prev: Option<u32>,
    class: u8,
) -> Vec<TextLine> {
    if !matches!(
        rec.class.as_str(),
        "Skill_RefreshCooldown" | "Skill_RefreshCooldownModifier"
    ) || level == 0
    {
        return Vec::new();
    }
    let ms = |l: u32| ((level_f(rec, "refreshTime", l) * 1000.0) as i32).max(0);
    if prev.is_some_and(|p| ms(p) == ms(level)) {
        return Vec::new();
    }
    let t = ms(level);
    let text = if t <= 0 {
        ctx.fmt("tagSkillRefreshTimeInfinate", &[])
    } else {
        let secs = t as f32 * 0.001;
        let unit = seconds_unit(ctx, secs);
        ctx.fmt("tagSkillRefreshTime", &[Arg::Num(secs), Arg::Str(&unit)])
    };
    vec![line(class, text)]
}

/// Deepest [`section_of`] nesting (bad data guard, see there).
const MAX_NESTING: u8 = 4;

/// One level's section of the text (`CreateUI[Next]ParameterText` without
/// the header, module docs), `prev` the level the next-level text compares
/// with. `scaled`: the numbers go through the player's (or for an
/// `isPetBonusScaling` skill the pet bonuses') modifiers; a pet's own skills
/// without that flag list the record's. `depth`: how many granted / summoned
/// skills deep this section is (0 for the hovered skill).
#[allow(clippy::too_many_arguments)]
fn section_of(
    ctx: &SkillTipCtx,
    skill: &str,
    rec: &Record,
    prof: &Record,
    mods: &[(Record, usize)],
    level: u32,
    prev: Option<u32>,
    class: u8,
    reduce: bool,
    depth: u8,
) -> Vec<TextLine> {
    // (bad data: a `grantedSkills` / `spawnObjects` cycle would recurse forever;
    // stock nests a summon's or a form's skills one level deep.)
    if depth > MAX_NESTING {
        return Vec::new();
    }
    let mut out = first_lines(ctx, rec, prof, level, prev, class);
    out.extend(param_lines(
        ctx, rec, prof, mods, level, prev, class, reduce,
    ));
    let potency = modifier_potency(ctx, skill, rec);
    out.extend(special_lines(
        ctx, rec, prof, mods, level, prev, class, potency, depth,
    ));
    out.extend(stat_lines_of(
        ctx,
        std::slice::from_ref(prof),
        mods,
        level.saturating_sub(1) as usize,
        class,
        true,
    ));
    out.extend(refresh_trigger_lines(
        ctx, skill, rec, prof, level, prev, class,
    ));
    if prev.is_some() {
        for l in &mut out {
            l.text = strip_colors(&l.text);
        }
    }
    out
}

/// [`section_of`] of a skill by name.
fn section(
    ctx: &SkillTipCtx,
    skill: &str,
    level: u32,
    prev: Option<u32>,
    class: u8,
    reduce: bool,
) -> Vec<TextLine> {
    let (Some(rec), Some(prof)) = (ctx.db.get(skill), profile_record(ctx.db, skill)) else {
        return Vec::new();
    };
    let mods = learned_mods(ctx, skill, rec);
    section_of(ctx, skill, rec, &prof, &mods, level, prev, class, reduce, 0)
}

/// Attribute lines of a skill at `level` (1-based), PORT (module docs).
pub fn stat_lines(ctx: &SkillTipCtx, skill: &str, level: u32, class: u8) -> Vec<TextLine> {
    section(ctx, skill, level, None, class, true)
}

/// [`stat_lines`]; `reduce`: the player's cooldown / energy cost reduction
/// applies (see [`param_lines`]).
fn stat_lines_with(
    ctx: &SkillTipCtx,
    skill: &str,
    level: u32,
    class: u8,
    reduce: bool,
) -> Vec<TextLine> {
    section(ctx, skill, level, None, class, reduce)
}

/// Next-level lines: the params that differ from `cur`, then the attribute
/// lines at `next`.
pub fn stat_lines_next(
    ctx: &SkillTipCtx,
    skill: &str,
    cur: u32,
    next: u32,
    class: u8,
) -> Vec<TextLine> {
    section(ctx, skill, next, Some(cur), class, true)
}

fn stat_lines_next_with(
    ctx: &SkillTipCtx,
    skill: &str,
    cur: u32,
    next: u32,
    class: u8,
    reduce: bool,
) -> Vec<TextLine> {
    section(ctx, skill, next, Some(cur), class, reduce)
}

/// The profile whose modifiers a pet-scaled skill's damage takes: only the
/// pet bonuses (`DamageAttributeStore::ProcessText`:
/// an `isPetBonusScaling` skill calls `GameEngine::
/// ContributePetBonusOffensiveModifierAttributes` where another calls the
/// main player's `SkillManager`; no attribute scaling, no conversions).
fn pet_bonus_profile(ctx: &SkillTipCtx, player: &crate::actor::Profile) -> crate::actor::Profile {
    let mut totals = std::collections::BTreeMap::new();
    for (r, li) in ctx.pet_bonuses {
        for a in record_attrs_at(r, Source::Base, *li)
            .into_iter()
            .filter(|a| a.store == StoreKind::Damage)
        {
            for (k, v) in a.values {
                *totals.entry(k).or_default() += v;
            }
        }
    }
    // The player's own totals, global groups and conversions are never
    // copied (a new field must be decided on here); `..` drops the DoT
    // sources too (the display reads the summed totals) and skips the
    // crate-private combat cache (fresh per profile, rebuilt lazily).
    let crate::actor::Profile {
        level,
        sheet,
        physical,
        pierce_ratio,
        totals: _,
        protection,
        armor_regions,
        absorption,
        resistances,
        attack_speed,
        speeds,
        base_speeds,
        speed_caps,
        is_player,
        pet,
        conversions: _,
        dual,
        global_groups: _,
        shield,
        active_costs,
        race,
        req_reduction,
        racial,
        charged,
        ..
    } = player;
    let mut sheet = sheet.clone();
    sheet.physique = 0.0;
    sheet.cunning = 0.0;
    sheet.spirit = 0.0;
    crate::actor::Profile {
        level: *level,
        sheet,
        physical: *physical,
        pierce_ratio: *pierce_ratio,
        totals,
        protection: *protection,
        armor_regions: armor_regions.clone(),
        absorption: *absorption,
        resistances: resistances.clone(),
        attack_speed: *attack_speed,
        speeds: *speeds,
        base_speeds: *base_speeds,
        speed_caps: *speed_caps,
        is_player: *is_player,
        pet: *pet,
        conversions: Vec::new(),
        dual: dual.clone(),
        global_groups: Vec::new(),
        shield: *shield,
        active_costs: *active_costs,
        race: race.clone(),
        req_reduction: *req_reduction,
        racial: racial.clone(),
        charged: charged.clone(),
        dot_sources: Vec::new(),
        ..Default::default()
    }
}

/// Attribute lines of `records` at level index `li`.
fn stat_lines_of(
    ctx: &SkillTipCtx,
    records: &[Record],
    mods: &[(Record, usize)],
    li: usize,
    class: u8,
    player_scaled: bool,
) -> Vec<TextLine> {
    // (store group, creation rank, lines): offense (conversions after it),
    // character, defense, retaliation, skill.
    let mut groups: Vec<(u8, usize, Vec<String>)> = Vec::new();
    // `DamageAttributeStore::CreateGlobalText`: the
    // attributes of the `offensiveGlobalChance` roll, all of them (`...AllTag`)
    // or one (`...OneTag`, the `XOR` ones), under their title, `tagIndent`ed.
    let mut global_groups: [(f32, Vec<String>); 2] = Default::default();
    for r in records {
        let pet_scaled = ctx
            .player
            .filter(|_| r.bool("isPetBonusScaling"))
            .map(|(_, p)| pet_bonus_profile(ctx, p));
        let dmg = ctx.player.filter(|_| player_scaled).map(|(rules, prof)| {
            // The learned modifiers' % modifiers and conversions join the
            // skill's, their flat damage does not (`ProcessText`, see
            // `actor::skill_tooltip_damage`).
            let mut su = crate::actor::SkillUse::new(r, li);
            su.mods = mods.iter().map(|(m, ml)| (m, *ml)).collect();
            crate::actor::skill_tooltip_damage(rules, pet_scaled.as_ref().unwrap_or(prof), su)
        });
        for mut a in record_attrs_at(r, Source::Base, li) {
            // The penalty store's reservation reads with the costs
            // ([`reserve_line`]).
            if a.values
                .iter()
                .any(|(k, _)| PENALTY_FIELDS.iter().any(|f| k.eq_ignore_ascii_case(f)))
            {
                continue;
            }
            if let Some((rules, prof)) = ctx.player.filter(|_| player_scaled) {
                scale_attr(
                    &mut a,
                    dmg.as_deref().unwrap_or(&[]),
                    pet_scaled.as_ref().unwrap_or(prof),
                    rules,
                );
            }
            let g = match a.store {
                StoreKind::Damage | StoreKind::Conversion => 0,
                StoreKind::Char => 2,
                StoreKind::Defense => 3,
                StoreKind::Retaliation => 4,
                StoreKind::Skill => 5,
            };
            let Some(t) = attribute_line(ctx.tags, &a) else {
                continue;
            };
            let t = if a.class.contains("AttributeReflex_") {
                fix_reflex(t)
            } else {
                t
            };
            if let Some(global) = a.global {
                let (group, indent) = (
                    &mut global_groups[usize::from(global.xor)],
                    ctx.fmt("tagIndent", &[]),
                );
                group.0 = global.chance;
                group
                    .1
                    .extend(t.split('\n').map(|l| format!("{indent}{l}")));
                continue;
            }
            groups.push((
                g,
                creation_rank(&a),
                t.split('\n').map(str::to_string).collect(),
            ));
        }
        for t in conversion_lines(ctx, r, li) {
            groups.push((1, 0, vec![t]));
        }
        // The profile's `SkillProfile.racialBonus` (`RacialBonus::GetUIDisplayText`) between the conversions and
        // the character store.
        for t in crate::items::tooltip_item::racial_text(ctx.tags, r, li) {
            groups.push((1, 1, vec![t]));
        }
    }
    for (i, (chance, lines)) in global_groups.into_iter().enumerate() {
        if lines.is_empty() {
            continue;
        }
        let which = if i == 0 { "All" } else { "One" };
        let title = if chance >= 100.0 {
            ctx.fmt(&format!("GlobalChanceOf{which}Tag"), &[])
        } else {
            ctx.fmt(
                &format!("GlobalPercentChanceOf{which}Tag"),
                &[Arg::Num(chance)],
            )
        };
        groups.push((0, usize::MAX, std::iter::once(title).chain(lines).collect()));
    }
    groups.sort_by_key(|(g, rank, _)| (*g, *rank));
    groups
        .into_iter()
        .flat_map(|(_, _, ls)| ls)
        .map(|t| line(class, t))
        .collect()
}

/// The buff bar tooltip of a charge potion. `Skill_ChargePotion::CreateUITrackerText`:
/// the skill's name (class 46, through `SimpleStringFormat`), then, with a damage absorption
/// above 0, `SkillDamageAbsorption` (plain `%.0f`, class 49: the buff bar view sets `isHotBarView`, so
/// the class is 49 for an item skill too); nothing else (no cost, no restores). `absorption` is
/// `Skill_ChargePotion.shieldLeft`, the shield LEFT (`TargetResult` sets it and `Skill_ChargePotion.shieldMax` to
/// `absorbShieldPercentHealth` % × potency × max life, `ModifyDamage` takes each absorbed hit
/// off `shieldLeft`). With some absorption the name is the first learned modifier's (`mods`: the potion's
/// `Skill.modifiers` list, `Skill::AddModifier` order; record, level index) whose own skill data
/// (`GetSkillData`) has `absorbShieldPercentHealth` > 0 (stock: the gdx3 absorbing potion
/// modifiers), else the potion's.
pub fn potion_tracker_lines(
    ctx: &SkillTipCtx,
    skill: &str,
    mods: &[(String, usize)],
    absorption: f32,
) -> Vec<TextLine> {
    let mut out = Vec::new();
    let named = (absorption > 0.0)
        .then(|| {
            mods.iter().find(|(m, li)| {
                ctx.db
                    .get(m)
                    .is_some_and(|r| level_f(r, "absorbShieldPercentHealth", *li as u32 + 1) > 0.0)
            })
        })
        .flatten()
        .map_or(skill, |(m, _)| m.as_str());
    let name = ctx.text(ctx.name_tag(named).as_deref());
    if !name.is_empty() {
        out.push(line(class::SKILL_NAME, name));
    }
    if absorption > 0.0 {
        out.push(line(
            class::SKILL_STATS_CURRENT,
            ctx.fmt("SkillDamageAbsorption", &[Arg::Num(absorption)]),
        ));
    }
    out
}

/// The "Celestial Power" block that ends `Skill::CreateUIParameterText`: a skill with an auto-cast
/// child (`Skill::LoadResources` makes
/// `Skill.autoCastChild` from `autoCastSkill` when `autoCastController` is set too) shows it when maxed or
/// in the hot bar view (`hotbar`, `Skill.isHotBarView`). Maxed here is the block's own test: with bonus
/// levels `learned >= skillMaxLevel || current >= skillUltimateLevel`, else `current >= skillMaxLevel`.
/// Lines: a blank (when
/// the last is not), `tagSkillGrantEffect` (class 74), the child's `CreateUISkillHeading` asked for class
/// 75 (its name only: the description is skipped for that class), a blank, then the child's
/// `CreateUIParameterText`: the child has `isAutoCastChild` and `suppressLevelTitles`, so
/// its stat lines are class 77 and its header is only `tagDevotionSkillRank` when its profile has more
/// than one `skillExperienceLevels` entry. Stock: only monster / shrine skills and
/// three totem auras have `autoCastSkill` (devotion powers use `templateAutoCast`, [`devotion_lines`]).
/// PORT: the child's level is the parent's current one (`SkillManager::UpdateSubSkillLevels`).
fn celestial_power_lines(
    ctx: &SkillTipCtx,
    rec: &Record,
    disp: &Record,
    (learned, bonus, cur): (u32, u32, u32),
    hotbar: bool,
) -> Vec<TextLine> {
    let mut out = Vec::new();
    let (Some(child), Some(_)) = (
        rec.str("autoCastSkill").filter(|s| !s.is_empty()),
        rec.str("autoCastController").filter(|s| !s.is_empty()),
    ) else {
        return out;
    };
    if ctx.db.get(child).is_none() {
        return out;
    }
    let max = disp.i32("skillMaxLevel").unwrap_or(0).max(0) as u32;
    let ultimate = disp
        .i32("skillUltimateLevel")
        .filter(|u| *u > 0)
        .map_or(max, |u| u as u32);
    let maxed = if bonus != 0 {
        learned >= max || cur >= ultimate
    } else {
        cur >= max
    };
    if !(maxed || hotbar) {
        return out;
    }
    out.push(line(0, ""));
    out.push(line(
        class::EFFECT_HEADING,
        ctx.fmt("tagSkillGrantEffect", &[]),
    ));
    let name = ctx.text(ctx.name_tag(child).as_deref());
    if !name.is_empty() {
        out.push(line(class::EFFECT_NAME, name));
    }
    out.push(line(0, ""));
    let ranks = profile_record(ctx.db, child).map_or(0, |p| p.f32s("skillExperienceLevels").len());
    if ranks > 1 {
        out.push(line(
            class::SKILL_LEVEL_TITLES,
            ctx.fmt(
                "tagDevotionSkillRank",
                &[Arg::Num(cur as f32), Arg::Num(ranks as f32)],
            ),
        ));
    }
    out.extend(stat_lines(ctx, child, cur, class::EFFECT_STATS_CURRENT));
    out.push(line(0, ""));
    out
}

/// The whole tooltip of `skill`. `reasons` is the hovered button's
/// `SkillReasons` (None: no warnings, e.g. a read-only description).
pub fn skill_lines(ctx: &SkillTipCtx, skill: &str, reasons: Option<&Reasons>) -> Vec<TextLine> {
    skill_lines_with(ctx, skill, reasons, false)
}

/// The buff bar's detail text of a skill with `trackerShowDetail` (`PlayerStatusIcons::GetRolloverText` kind 0): the
/// heading (name 46, description 47), then `Skill::CreateUIParameterText` alone, i.e. the current level's block
/// (a skill at level 0 prints nothing past the heading); no next level, no warnings.
pub fn skill_detail_lines(ctx: &SkillTipCtx, skill: &str) -> Vec<TextLine> {
    skill_lines_with(ctx, skill, None, true)
}

/// `Skill::CreateUIParameterText` (class 49) as the skill window prints it: nothing while unlearned, else
/// the `tagCurrentLevel[Augment|ExcessAugment]` header (class 48), the attribute lines at the current
/// level (49), the pet bonus block after a blank, the "Celestial Power" block ([`celestial_power_lines`];
/// `hotbar` = `Skill.isHotBarView`, the hot bar view) and a blank.
fn current_block(
    ctx: &SkillTipCtx,
    skill: &str,
    rec: &Record,
    disp: &Record,
    hotbar: bool,
    headless: bool,
) -> Vec<TextLine> {
    let mut out = Vec::new();
    let prof = profile_record(ctx.db, skill).unwrap_or_else(|| rec.clone());
    let (base, bonus, cur, _max) = levels(ctx, skill, disp);
    let ultimate = ctx.book.ultimate_level(ctx.db, skill);
    // `Skill::CreateUIParameterText`: no level header with `Skill.suppressLevelTitles` set (the
    // `Skill_Evade` constructor sets it, the `Skill_ChargePotion` one `Skill.isPotion` and it, and
    // `Skill_ChargePotion::CreateUIParameterText` prints none either); a potion (`Skill.isPotion`) has
    // no cooldown / cost reduction (`Skill::CreateUIModifierText`). Item and granted skills set it too
    // (`headless`, [`hot_slot_lines`]). PORT: autocast and static skills' flag is not passed (their texts are
    // not hot slot texts here).
    let headless = headless || matches!(rec.class.as_str(), "Skill_Evade" | "Skill_ChargePotion");
    if cur > 0 && headless {
        let reduce = rec.class != "Skill_ChargePotion";
        out.extend(stat_lines_with(
            ctx,
            skill,
            cur,
            class::SKILL_STATS_CURRENT,
            reduce,
        ));
        out.push(line(0, ""));
    } else if cur > 0 {
        // `Skill::CreateUIParameterText`: "excess" is `learned + bonus`
        // above the profile's ultimate level (`GetUltimateLevel`), not the max.
        let header = if base + bonus > ultimate && bonus > 0 {
            ctx.fmt(
                "tagCurrentLevelExcessAugment",
                &[Arg::Num(base as f32), Arg::Num(bonus as f32)],
            )
        } else if bonus > 0 {
            ctx.fmt(
                "tagCurrentLevelAugment",
                &[Arg::Num(base as f32), Arg::Num(bonus as f32)],
            )
        } else {
            ctx.fmt("tagCurrentLevel", &[Arg::Num(base as f32)])
        };
        out.push(line(class::SKILL_LEVEL_TITLES, header));
        out.extend(stat_lines(ctx, skill, cur, class::SKILL_STATS_CURRENT));
        // A blank line sits before the pet block (real window, Consecrated Blade).
        let pets = pet_bonus_lines(ctx, &prof, cur, false);
        if !pets.is_empty() {
            out.push(line(0, ""));
        }
        out.extend(pets);
        out.extend(celestial_power_lines(
            ctx,
            rec,
            disp,
            (base, bonus, cur),
            hotbar,
        ));
        out.push(line(0, ""));
    }
    out
}

/// `Skill::CreateUISkillHeading`: the name (46) and the description (47).
fn heading_lines(ctx: &SkillTipCtx, disp: &Record) -> Vec<TextLine> {
    let mut out = Vec::new();
    let name = ctx.text(disp.str("skillDisplayName"));
    if !name.is_empty() {
        out.push(line(class::SKILL_NAME, name));
    }
    let desc = ctx.text(disp.str("skillBaseDescription"));
    if !desc.is_empty() {
        out.push(line(class::SKILL_DESCRIPTION, desc));
    }
    out
}

fn skill_lines_with(
    ctx: &SkillTipCtx,
    skill: &str,
    reasons: Option<&Reasons>,
    current_only: bool,
) -> Vec<TextLine> {
    let Some(rec) = ctx.db.get(skill) else {
        return Vec::new();
    };
    let disp = display_record(ctx.db, skill).unwrap_or_else(|| rec.clone());
    let mut out = heading_lines(ctx, &disp);
    if rec.i32("exclusiveSkill").unwrap_or(0) != 0 || disp.i32("exclusiveSkill").unwrap_or(0) != 0 {
        out.push(line(
            class::SKILL_DESCRIPTION,
            ctx.fmt("tagExclusiveSkill", &[]),
        ));
    }
    if crate::skills::class_info(&rec.class).buff_self {
        out.push(line(
            class::SKILL_DESCRIPTION,
            ctx.fmt("tagAutoToggledSkill", &[]),
        ));
    }
    if !out.is_empty() {
        out.push(line(0, ""));
    }
    let prof = profile_record(ctx.db, skill).unwrap_or_else(|| rec.clone());
    let (base, bonus, cur, _max) = levels(ctx, skill, &disp);
    let maxed = reasons.map_or_else(|| is_maxed(ctx, skill), |r| r.maxed);
    let ultimate = ctx.book.ultimate_level(ctx.db, skill);
    out.extend(current_block(ctx, skill, rec, &disp, false, false));
    if current_only {
        return collapse_blanks(out);
    }
    // Reclamation mode (`GenerateUISkillText` `a5`): no next level, the reclaim line even when maxed.
    if let Some((tip, cost)) = reasons.and_then(|r| r.reclaim) {
        use crate::reclamation::Tip;
        let money = crate::reclamation::money(cost);
        let (cls, text) = match tip {
            None => return collapse_blanks(out),
            Some(Tip::MasteryNeeded) => (
                class::SKILL_REQUIREMENTS_NOT_MET,
                ctx.fmt("tagReclaimMastery", &[]),
            ),
            Some(Tip::MasteryLast) => (
                class::SKILL_REQUIREMENTS_NOT_MET,
                ctx.fmt("tagDecreaseMasteryError", &[]),
            ),
            Some(Tip::NoPoints) => (
                class::SKILL_REQUIREMENTS_NOT_MET,
                ctx.fmt("tagReclaimNoPoints", &[]),
            ),
            Some(Tip::NoGold) => (
                class::SKILL_REQUIREMENTS_NOT_MET,
                ctx.fmt("tagReclaimNoGold", &[Arg::Str(&money)]),
            ),
            Some(Tip::Base) => (
                class::SKILL_REQUIREMENTS_NOT_MET,
                ctx.fmt("tagReclaimBase", &[]),
            ),
            Some(Tip::Devotion) => (
                class::SKILL_REQUIREMENTS_NOT_MET,
                ctx.fmt("tagReclaimDevotion", &[]),
            ),
            Some(Tip::Point) => (
                class::SKILL_REQUIREMENTS,
                ctx.fmt("tagReclaimPoint", &[Arg::Str(&money)]),
            ),
        };
        out.push(line(0, ""));
        out.push(line(cls, text));
        return collapse_blanks(out);
    }
    // Next level.
    if !maxed {
        let next = base + 1;
        let header = if bonus == 0 {
            ctx.fmt("tagNextLevel", &[Arg::Num(next as f32)])
        } else if next + bonus <= ultimate {
            ctx.fmt(
                "tagNextLevelAugment",
                &[Arg::Num(next as f32), Arg::Num(bonus as f32)],
            )
        } else {
            ctx.fmt(
                "tagNextLevelExcessAugment",
                &[Arg::Num(next as f32), Arg::Num(bonus as f32)],
            )
        };
        out.push(line(class::SKILL_LEVEL_TITLES, header));
        out.extend(stat_lines_next(
            ctx,
            skill,
            cur,
            cur + 1,
            class::SKILL_STATS_NEXT,
        ));
        let pets = pet_bonus_lines(ctx, &prof, cur + 1, true);
        if !pets.is_empty() {
            out.push(line(0, ""));
        }
        out.extend(pets);
    }
    // Warnings.
    if let Some(r) = reasons
        && !maxed
    {
        out.push(line(0, ""));
        let name_of = |tag: &str| ctx.tag(tag);
        if let Some((need, mastery)) = &r.mastery_low {
            let tag = if *need == 1 {
                "tagSkillReq01Singular"
            } else {
                "tagSkillReq01"
            };
            out.push(line(
                class::SKILL_REQUIREMENTS_NOT_MET,
                ctx.fmt(tag, &[Arg::Num(*need as f32), Arg::Str(&name_of(mastery))]),
            ));
        } else if let Some((all, missing, count)) = &r.dependency {
            if *count > 1 {
                out.push(line(
                    class::SKILL_REQUIREMENTS_NOT_MET,
                    ctx.fmt(
                        if *all {
                            "FailedSkillDependancyAll"
                        } else {
                            "FailedSkillDependancyOne"
                        },
                        &[],
                    ),
                ));
            }
            for m in missing {
                let n = name_of(m);
                let tag = if *count == 1 {
                    "FailedSkillDependancy"
                } else {
                    "FailedSkillDependancyName"
                };
                out.push(line(
                    class::SKILL_REQUIREMENTS_NOT_MET,
                    ctx.fmt(tag, &[Arg::Str(&n)]),
                ));
            }
        } else if let Some(base) = &r.base_disabled {
            out.push(line(
                class::SKILL_REQUIREMENTS_NOT_MET,
                ctx.fmt("tagSkillReq03", &[Arg::Num(1.0), Arg::Str(&name_of(base))]),
            ));
        } else if r.masteries_full {
        } else if let Some(other) = &r.exclusive_conflict {
            out.push(line(
                class::SKILL_REQUIREMENTS_NOT_MET,
                ctx.fmt("tagExclusiveSkillConflict", &[Arg::Str(&name_of(other))]),
            ));
        } else if r.no_points {
            out.push(line(
                class::SKILL_REQUIREMENTS_NOT_MET,
                ctx.fmt("NoSkillPointsToAdd", &[]),
            ));
        } else {
            out.push(line(
                class::SKILL_REQUIREMENTS,
                ctx.fmt("PressToAddUnusedSkillPoints", &[]),
            ));
        }
    }
    collapse_blanks(out)
}

/// `tagAutoSkillCondition<NN>` of an auto-cast controller's `triggerType`
/// (`SkillAutoCastController::GetParams`: the exact
/// type names; `LowHealth` / `LowMana` default their parameter to 25).
pub fn autocast_condition(trigger: &str) -> Option<&'static str> {
    Some(match trigger {
        "LowHealth" => "tagAutoSkillCondition01",
        "LowMana" => "tagAutoSkillCondition02",
        "HitByEnemy" => "tagAutoSkillCondition03",
        "HitByMelee" => "tagAutoSkillCondition04",
        "HitByProjectile" => "tagAutoSkillCondition05",
        "CastBuff" => "tagAutoSkillCondition06",
        "AttackEnemy" => "tagAutoSkillCondition07",
        "OnEquip" => "tagAutoSkillCondition08",
        "HitByCrit" => "tagAutoSkillCondition09",
        "AttackEnemyCrit" => "tagAutoSkillCondition10",
        "Block" => "tagAutoSkillCondition11",
        "OnKill" => "tagAutoSkillCondition12",
        _ => return None,
    })
}

/// A celestial power: a devotion skill whose profile record has an auto-cast
/// controller (`templateAutoCast`, read by `Skill::LoadResources`, so a
/// wrapper's comes from its `buffSkillName` record:
/// [`crate::skills::profile_record`]; what `DevotionState::bind` requires).
pub fn is_power(rec: &Record) -> bool {
    rec.str("templateAutoCast").is_some_and(|c| !c.is_empty())
}

/// The ` (25% Chance on Attack)` suffix of a power's name
/// (`Skill::CreateUISkillHeading`: the controller's condition
/// tag localised with `chanceToRun` and `triggerParam`).
pub fn autocast_suffix(ctx: &SkillTipCtx, rec: &Record) -> String {
    let Some(c) = rec
        .str("templateAutoCast")
        .filter(|c| !c.is_empty())
        .and_then(|c| ctx.db.get(c))
    else {
        return String::new();
    };
    let trigger = c.str("triggerType").unwrap_or("");
    let Some(tag) = autocast_condition(trigger) else {
        return String::new();
    };
    let chance = c.f32("chanceToRun").unwrap_or(0.0);
    let mut param = c.f32("triggerParam").unwrap_or(0.0);
    if param <= 0.0 && matches!(trigger, "LowHealth" | "LowMana") {
        param = 25.0;
    }
    ctx.fmt(tag, &[Arg::Num(chance), Arg::Num(param)])
}

/// `GameTextLines::AddBlankLine` adds a blank line only when the last line has a class:
/// no leading blank, no two in a row.
fn collapse_blanks(lines: Vec<TextLine>) -> Vec<TextLine> {
    let mut out: Vec<TextLine> = Vec::with_capacity(lines.len());
    for l in lines {
        if l.text.is_empty()
            && l.icon.is_none()
            && out
                .last()
                .is_none_or(|p| p.text.is_empty() && p.icon.is_none())
        {
            continue;
        }
        out.push(l);
    }
    out
}

/// The `SkillReasons` of a skill of a mastery `tree` (`UISkillPane::GetRequirementFlags`,
/// flag meanings). `masteries_full`: the mastery slots are taken
/// (only matters for the mastery skill).
pub fn reasons(
    ctx: &SkillTipCtx,
    tree: &str,
    skill: &str,
    points: u32,
    masteries_full: bool,
) -> Reasons {
    let mut r = Reasons {
        no_points: points == 0,
        maxed: is_maxed(ctx, skill),
        ..Default::default()
    };
    let Some(rec) = ctx.db.get(skill) else {
        return r;
    };
    if rec.class == "Skill_Mastery" {
        r.masteries_full = masteries_full && ctx.book.level(skill) == 0;
        return r;
    }
    let trees = ctx.trees;
    let class_tree = trees.tree(tree);
    // [1] the mastery's learned level against the tier requirement (as
    // `SkillBook::cannot_raise`).
    let need = trees.tier_requirement(ctx.db, skill);
    let mastery = class_tree.and_then(|t| t.mastery()).unwrap_or_default();
    let have = ctx.book.level(mastery);
    if have < need {
        let tag = display_record(ctx.db, mastery)
            .and_then(|m| m.str("skillDisplayName").map(str::to_string))
            .unwrap_or_default();
        r.mastery_low = Some((need - have, tag));
    }
    // [2] `IsBaseSkillEnabled`: one of the bases has a level.
    let mut bases = class_tree
        .into_iter()
        .flat_map(|t| t.bases_of(skill))
        .peekable();
    if let Some(&first) = bases.peek()
        && bases.all(|b| ctx.book.level(b) == 0)
    {
        r.base_disabled = ctx.name_tag(first);
    }
    // [5] `skillDependancy`: [`SkillBook::dependency_unmet`] decides, the
    // message lists the unlearned ones.
    if ctx.book.dependency_unmet(ctx.db, skill) {
        let (deps, all) = crate::skills::skill_dependencies(ctx.db, skill);
        let missing = deps
            .iter()
            .filter(|d| ctx.book.level(d.as_str()) == 0)
            .filter_map(|d| ctx.name_tag(d))
            .collect();
        r.dependency = Some((all, missing, deps.len()));
    }
    // [6] `UISkillPane::HasExclusiveConflict`: another active exclusive skill beats it.
    if let Some(other) = ctx.book.exclusive_conflict(ctx.db, trees, skill) {
        r.exclusive_conflict = ctx.name_tag(other);
    }
    r
}
