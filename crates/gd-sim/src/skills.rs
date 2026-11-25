//! Player skills: mastery bars and class skills, the rules for spending skill
//! points, and which learned skills feed the character's stats.
//!
//! Data: a mastery's `_classtree_classNN.dbr` (`SkillTree`, `skillName1..N`,
//! the mastery bar first) lists its skills; a skill's `skillTier` needs mastery
//! level `skillMasteryTierLevel[tier - 1]` (`gameengine.dbr`, read by
//! `SkillProfile::LoadProfile` / `GameEngine::GetSkillMasteryTierLevels`);
//! `skillMaxLevel` caps it.
//! Parents (`SkillManager::CreateAndLoadSkill`):
//! loading a tree in order, a mastery clears the current base skill, any
//! other skill that is neither a `Skill_Modifier` (transmuters and
//! `Skill_ProjectileModifier` derive from it) nor a `SkillSecondary` becomes
//! it; a modifier's base is its `subSkillName` when set, else the current
//! base, and a secondary's the current base (`Skill::AddBaseSkill` list),
//! whose secondary list (`Skill.secondarySkills`) it joins: that list is what
//! `Skill::GetSecondarySkills` returns. Gear skill modifiers
//! that are secondaries go to a second list (`Skill.itemSecondarySkills`,
//! `Skill::AddToItemSecondarySkills` from `SkillManager::
//! AddItemSkillModifiers`); `Skill::ActivateSecondarySkills`
//! runs both, the sheet's skill rollovers read only the first. A
//! skill with base skills can take points only while one of them has a
//! level (`Skill::IsBaseSkillEnabled`, checked by the skill
//! window in GrimDawn.exe); one without (e.g. right after the mastery)
//! always can. Masteries allowed
//! = 1 from player level 2, 2 from level 10 (`masteryIncrementLevel`
//! 1;10, one step per level-up in `SkillManager::UpdateMasteriesAllowed`
//! from `Character::ReceiveExperience`). Stats: [`SkillBook::stat_sources`]
//! (masteries, passives, switched-on toggles with their buff records, and
//! their modifiers); every other skill acts when cast.
//!
//! [`SkillTrees`] holds the class trees and what is derived from them (bases,
//! modifier parents, potion parents, enumerations, tier levels); every rule that needs a
//! tree takes it. A skill class's constructor facts are one table ([`class_info`]).
use gd_formats::arz::Record;
use gd_formats::collections::HashMap;
use gd_formats::vfs::Database;
use std::borrow::Cow;
use std::sync::LazyLock;

/// Class trees of the six base-game masteries and the expansions'.
pub fn class_trees(db: &Database) -> Vec<(u32, String)> {
    (1..=10)
        .filter_map(|n| {
            let path = format!("records/skills/playerclass{n:02}/_classtree_class{n:02}.dbr");
            db.get(&path).map(|_| (n, path))
        })
        .collect()
}

/// `skillName1..=64` keys.
static SKILL_NAME_KEYS: LazyLock<[String; 64]> =
    LazyLock::new(|| std::array::from_fn(|i| format!("skillName{}", i + 1)));

/// Skill records of a class tree, in tree order (mastery bar first).
pub fn tree_skills(db: &Database, tree: &str) -> Vec<String> {
    let Some(r) = db.get(tree) else {
        return Vec::new();
    };
    SKILL_NAME_KEYS
        .iter()
        .filter_map(|k| r.str(k).map(str::to_owned))
        .collect()
}

/// `s` ASCII-lowercased, borrowed when it already is.
fn lower(s: &str) -> Cow<'_, str> {
    if s.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(s.to_ascii_lowercase())
    } else {
        Cow::Borrowed(s)
    }
}

/// The record holding a skill's display data (icon, name, max level, tier):
/// buff and pet skills keep it in `buffSkillName` / `petSkillName`.
pub fn display_record(db: &Database, skill: &str) -> Option<Record> {
    let first = db.get_owned(skill)?;
    let has =
        |r: &Record| r.str("skillDisplayName").is_some() || r.str("skillUpBitmapName").is_some();
    let mut r = first.clone();
    // Pet modifiers → pet skill → its buff: follow up to three links.
    for _ in 0..3 {
        if has(&r) {
            return Some(r);
        }
        // Walks `buffSkillName` / `petSkillName` links to the record holding a field (not the profile lookup).
        let Some(next) = ["buffSkillName", "petSkillName"]
            .iter()
            .find_map(|k| r.str(k).and_then(|p| db.get_owned(p)))
        else {
            break;
        };
        r = next;
    }
    Some(if has(&r) { r } else { first })
}

/// The record a skill's `SkillProfile` (duration, radius, cost, cooldown,
/// values, sounds) is loaded from: a buff wrapper's `buffSkillName`, else the
/// skill's own. Game.dll: the `Load` of `Skill_BuffRadius`
/// (`Skill_BuffRadiusToggled` inherits it), `Skill_BuffOther`
/// (`Skill_ChargedBuffOther` inherits it), `Skill_AttackBuff`
/// (`Skill_AttackBuffRadius::Load` calls it),
/// `Skill_AttackProjectileDebuf`, `SkillSecondary_BuffSelfDuration`,
/// `SkillSecondary_OnHitBuffRadius`, `SkillSecondary_BuffRadius`
/// and `SkillSecondary_PetModifier` read `buffSkillName`, load that table and
/// run `Skill::LoadResources` on it instead of their own, so
/// `GetCooldownTime`, `skillActiveDuration` etc. of the skill object (the
/// wrapper, however it was cast: hot slot, proc, item) are the buff record's.
/// The wrappers' own records carry only `instantCast` / `pointBlank` (stock
/// data). A skill record that is no wrapper is its own profile.
pub fn profile_name(rec: &Record) -> Option<&str> {
    const WRAPPERS: &[&str] = &[
        "Skill_BuffRadius",
        "Skill_BuffRadiusToggled",
        "Skill_BuffOther",
        "Skill_ChargedBuffOther",
        "Skill_AttackBuff",
        "Skill_AttackBuffRadius",
        "Skill_AttackProjectileDebuf",
        "SkillSecondary_BuffSelfDuration",
        "SkillSecondary_OnHitBuffRadius",
        "SkillSecondary_BuffRadius",
        "SkillSecondary_PetModifier",
    ];
    WRAPPERS
        .contains(&rec.class.as_str())
        .then(|| rec.str("buffSkillName"))
        .flatten()
}

/// The record [`profile_name`] names for skill `skill` (its own when it is no
/// wrapper or the buff record is missing). A `SkillSecondary_PetModifier`'s
/// profile is its pet skill's (`SkillSecondary_PetModifier::Load`:
/// `petSkillName`, then that table's `buffSkillName` when set,
/// loaded as the modifier's own).
pub fn profile_record(db: &Database, skill: &str) -> Option<Record> {
    let r = db.get_owned(skill)?;
    if r.class == "SkillSecondary_PetModifier"
        && let Some(pet) = r.str("petSkillName").and_then(|p| db.get_owned(p))
    {
        return Some(
            pet.str("buffSkillName")
                .and_then(|b| db.get_owned(b))
                .unwrap_or(pet),
        );
    }
    match profile_name(&r).and_then(|b| db.get_owned(b)) {
        Some(b) => Some(b),
        None => Some(r),
    }
}

/// `SkillProfile::GetSkillDependancies` of `skill`'s profile record: the
/// `skillDependancy` names and `skillDependancyAll` (all must be learned, else
/// any one). What [`SkillBook::dependency_unmet`] tests.
pub fn skill_dependencies(db: &Database, skill: &str) -> (Vec<String>, bool) {
    let Some(rec) = profile_record(db, skill) else {
        return (Vec::new(), false);
    };
    let deps = rec
        .strs("skillDependancy")
        .into_iter()
        .map(str::to_string)
        .collect();
    (deps, rec.bool("skillDependancyAll"))
}

/// A skill attribute summed over the skill's record and its modifiers
/// (`Skill::GetSkillModifierData`): `key` at the skill's level plus each modifier's
/// at its own. No % part (`GetTotalSkillAttribute` covers only the 5
/// skill-wide stats).
pub fn summed_attribute(rec: &Record, li: usize, mods: &[(Record, usize)], key: &str) -> f32 {
    let lf = |r: &Record, l: usize| crate::stats::level_value(r, key, l).unwrap_or(0.0);
    lf(rec, li) + mods.iter().map(|(r, l)| lf(r, *l)).sum::<f32>()
}

/// `Skill::GetPetLimit`: the modifier-summed
/// `petLimit` (0 = unlimited); `petBurstSpawn` sums the same way
/// (`SkillProfile_Modifiers.petBurstSpawn`).
pub fn pet_limit(rec: &Record, li: usize, mods: &[(Record, usize)]) -> f32 {
    summed_attribute(rec, li, mods, "petLimit")
}

/// Seconds a summon of `skill` at `level_index` lives, None = forever
/// (`Skill::GetSpawnObjectTimeToLive`): the skill's `spawnObjectsTimeToLive` at its level plus that of each
/// of its modifiers at theirs (`mods`: record, level index, learned; the
/// learned list `Skill.modifiers` and the gear's `Skill.itemModifiers`, both summed by
/// `GetSkillModifierData`), in whole ms; then each learned
/// `Skill_SpawnPetTransmuter`'s `modSpawnObjectsTimeToLive` at the skill's
/// level, when not 0, replaces it (the last one wins). Only a time > 0 is
/// set (`Skill_SpawnPet::SpawnPet`: `if (ttl > 0) SetLifetime`).
pub fn spawn_time_to_live(
    db: &Database,
    skill: &Record,
    level_index: usize,
    mods: &[(String, usize, bool)],
) -> Option<f32> {
    let level_f =
        |r: &Record, k: &str, li: usize| crate::stats::level_value(r, k, li).unwrap_or(0.0);
    let mut secs = level_f(skill, "spawnObjectsTimeToLive", level_index);
    let mut replaced = None;
    for (m, li, learned) in mods {
        let Some(r) = db.get(m) else { continue };
        secs += level_f(r, "spawnObjectsTimeToLive", *li);
        if *learned && r.class == "Skill_SpawnPetTransmuter" {
            let t = (level_f(r, "modSpawnObjectsTimeToLive", level_index) * 1000.0) as i32;
            if t != 0 {
                replaced = Some(t);
            }
        }
    }
    let ms = replaced.unwrap_or((secs * 1000.0) as i32);
    (ms > 0).then_some(ms as f32 / 1000.0)
}

/// Whether a summon class asks `Skill::GetWeightedSpawnObject` and
/// `Skill::AddModifierPetChanges` for its pet (their only callers:
/// `Skill_SpawnPet::SpawnPet` (with the classes using it),
/// `Skill_TargetedSpawnPet::SpawnPet` and
/// `SkillSecondary_TargetedSpawnPet::SpawnPet`). The others take
/// `spawnObjects` alone: `Skill_MonsterGenerator::SkillSpawnObject`
/// (its stock `spawnObjectWeights` are dead data), the
/// projectile / chaos / lightning spawners' `SpawnPet`.
pub fn spawn_picks_weighted(class: &str) -> bool {
    !matches!(
        class,
        "Skill_MonsterGenerator"
            | "Skill_AttackProjectileSpawnPet"
            | "Skill_AttackSpellChaosSpawnPet"
            | "Skill_AttackRadiusLightningSpawnPet"
    )
}

/// The record one summon of `skill` at `level_index` spawns:
/// - With [`spawn_picks_weighted`], `Skill::GetWeightedSpawnObject`:
///   the lists `spawnObjects{,2,3,4}` whose
///   `spawnObjectWeights{,2,3,4}` at the level (integers, the last entry past
///   the end, missing 0) is not 0 and whose name at the level is not empty
///   take part with their weights summed in that order; `roll(total)` (the
///   engine's two Park–Miller steps from a fresh seed, `% total`) picks the
///   first whose running sum exceeds it. None take part: `spawnObjects` at
///   the level.
/// - Then `Skill::AddModifierPetChanges`: the `petChanges` of
///   every enabled `SkillSecondary_PetModifier` among the skill's learned
///   secondaries (`Skill.secondarySkills`, `secondaries`, in their order;
///   `SkillSecondary_PetModifier::CollectModifierPetChanges`) joined into one list, and only
///   when that gives none the gear-granted ones' (`Skill.itemSecondarySkills`, `item_secondaries`:
///   a legendary's `modifierSkillName`); not empty, it replaces the pick by
///   its entry at the level (the last past the end): the legendary pet
///   variants (Mortar Trap b, Thermite Mine c, ...).
///
/// PORT: the `GetSpawnObject{,2,3,4}` overrides of learned `Skill_Modifier`s
/// (`modSpawnObjects*`) are not applied here.
pub fn spawn_object(
    db: &Database,
    skill: &Record,
    level_index: usize,
    secondaries: &[String],
    item_secondaries: &[String],
    roll: impl FnOnce(u32) -> u32,
) -> Option<String> {
    let at = |k: &str| {
        skill
            .strs_iter(k)
            .nth(level_index)
            .or_else(|| skill.strs_iter(k).last())
            .map(|s| s.to_string())
    };
    let mut pick = None;
    if spawn_picks_weighted(&skill.class) {
        let mut total = 0u32;
        let mut entries = Vec::new();
        for k in ["", "2", "3", "4"] {
            let w = crate::stats::level_value(skill, &format!("spawnObjectWeights{k}"), level_index)
                .unwrap_or(0.0) as i32;
            if w == 0 {
                continue;
            }
            if let Some(name) = at(&format!("spawnObjects{k}")) {
                total = total.wrapping_add(w as u32);
                entries.push((total, name));
            }
        }
        if !entries.is_empty() && total > 0 {
            let r = roll(total);
            pick = entries.into_iter().find(|(cum, _)| r < *cum).map(|e| e.1);
        }
    }
    let mut pick = pick.or_else(|| at("spawnObjects"));
    if spawn_picks_weighted(&skill.class) {
        let changes_of = |list: &[String]| -> Vec<String> {
            list.iter()
                .filter_map(|m| db.get(m))
                .filter(|r| r.class == "SkillSecondary_PetModifier")
                .flat_map(|r| {
                    r.strs_iter("petChanges")
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let mut changes = changes_of(secondaries);
        if changes.is_empty() {
            changes = changes_of(item_secondaries);
        }
        if let Some(c) = changes.get(level_index.min(changes.len().saturating_sub(1))) {
            pick = Some(c.clone());
        }
    }
    pick
}

/// A pet's skills with their levels: `skillName{n}` of
/// its record at `skillLevel{n}` (a number or an equation of `charLevel`),
/// `mods` (lower-case skill, level) naming the owner's pet modifiers' pet
/// skills overriding it.
pub fn pet_skill_levels(pet: &Record, level: i32, mods: &[(String, u32)]) -> Vec<(String, u32)> {
    let lvl = level as f32;
    SKILL_NAME_KEYS[..40]
        .iter()
        .enumerate()
        .filter_map(|(i, k)| {
            let name = pet.str(k)?;
            Some((
                name.to_string(),
                pet_skill_level(pet, i + 1, name, lvl, mods),
            ))
        })
        .collect()
}

/// One slot's level ([`pet_skill_levels`]).
pub fn pet_skill_level(
    pet: &Record,
    slot: usize,
    name: &str,
    level: f32,
    mods: &[(String, u32)],
) -> u32 {
    if let Some((_, l)) = mods.iter().find(|(s, _)| s.eq_ignore_ascii_case(name)) {
        return *l;
    }
    skill_level_eq(pet, slot, level).max(0.0) as u32
}

/// One slot's `skillLevel{slot}` at `char_level`: a number or an equation in
/// `charLevel` (`SkillManager::LoadSkills`); 0 when absent or unparsable.
/// The caller truncates.
pub fn skill_level_eq(rec: &Record, slot: usize, char_level: f32) -> f32 {
    let read = |key: &str| match rec.get(key).and_then(|v| v.first()) {
        Some(gd_formats::arz::RawValue::Str(s)) => {
            gd_rules::expr::eval_var(s, "charLevel", char_level).unwrap_or(0.0)
        }
        Some(v) => v.as_f32().unwrap_or(0.0),
        None => 0.0,
    };
    let Some(i) = slot.checked_sub(1) else {
        return 0.0;
    };
    if i >= SKILL_LEVEL_KEYS.len() {
        return read(&format!("skillLevel{slot}"));
    }
    match rec.derived::<3, _>(|| skill_levels(rec)) {
        Some(cache) => match &cache[i] {
            SkillLevel::Num(v) => *v,
            SkillLevel::Eq(e) => e.as_ref().map_or(0.0, |e| {
                e.eval(&|n| n.eq_ignore_ascii_case("charLevel").then_some(char_level))
            }),
            SkillLevel::Absent => 0.0,
        },
        None => read(&SKILL_LEVEL_KEYS[i]),
    }
}

/// A record's `skillLevelN` ([`Record::derived`] slot 3): a number, an equation in
/// `charLevel`, or absent.
enum SkillLevel {
    Absent,
    Num(f32),
    /// `None`: a parse error (reads 0, as [`gd_rules::expr::eval_var`]).
    Eq(Option<gd_rules::expr::Expr>),
}

/// `skillLevel1..=64` keys.
static SKILL_LEVEL_KEYS: LazyLock<[String; 64]> =
    LazyLock::new(|| std::array::from_fn(|i| format!("skillLevel{}", i + 1)));

fn skill_levels(rec: &Record) -> [SkillLevel; 64] {
    std::array::from_fn(
        |i| match rec.get(&SKILL_LEVEL_KEYS[i]).and_then(|v| v.first()) {
            Some(gd_formats::arz::RawValue::Str(s)) => {
                SkillLevel::Eq(gd_rules::expr::Expr::parse(s).ok())
            }
            Some(v) => SkillLevel::Num(v.as_f32().unwrap_or(0.0)),
            None => SkillLevel::Absent,
        },
    )
}

/// Whether a pet bonus record reaches a pet of `pet_types` (`petTypeNames` of both).
pub fn pet_bonus_applies(bonus: &Record, pet_types: &[String]) -> bool {
    let mut want = bonus.strs_iter("petTypeNames").peekable();
    want.peek().is_none()
        || pet_types.is_empty()
        || want.any(|w| pet_types.iter().any(|p| p.eq_ignore_ascii_case(w)))
}

/// The `Skill_Modifier` family by class name: `Skill_Modifier`, `Skill_*Modifier` and
/// `Skill_*Transmuter` (RTTI: `Skill_Transmuter`, `Skill_SpawnPetTransmuter`,
/// `Skill_ProjectileModifier`, `Skill_ProjectileTransmuter`); the potion modifiers
/// ([`is_potion_class`]) are among them. No secondaries.
pub fn is_skill_modifier_class(class: &str) -> bool {
    class == "Skill_Modifier"
        || (class.starts_with("Skill_")
            && (class.ends_with("Transmuter") || class.ends_with("Modifier")))
}

/// A skill that attaches to a base skill when a tree loads: the [`is_skill_modifier_class`]
/// family and the secondaries ([`is_secondary_class`]).
pub fn is_modifier_class(class: &str) -> bool {
    is_skill_modifier_class(class) || is_secondary_class(class)
}

/// `SkillSecondary` and its subclasses (`SkillSecondary_*`): a base skill's
/// separate effects (`Skill.secondarySkills`), not damage
/// modifiers.
pub fn is_secondary_class(class: &str) -> bool {
    class.starts_with("SkillSecondary")
}

/// The potion classes (`Skill_PotionContainer`, `Skill_PotionModifier`): the player's potion
/// trees, not the class trees.
pub fn is_potion_class(class: &str) -> bool {
    class.starts_with("Skill_Potion")
}

/// A skill the skill tooltip shows as a modifier of its parent (the `...Mod` tags, no learned
/// modifiers of its own): the [`is_skill_modifier_class`] family but the potion modifiers, and
/// `SkillSecondary_PetModifier`.
pub fn is_tooltip_modifier_class(class: &str) -> bool {
    (is_skill_modifier_class(class) && !is_potion_class(class))
        || class == "SkillSecondary_PetModifier"
}

/// Base skills of every tree entry, in tree order
/// (`SkillManager::CreateAndLoadSkill`). Also the rule
/// for a monster's `skillName1..N` (`SkillManager::LoadSkills`
/// calls `CreateAndLoadSkill` in that order: a `SkillSecondary`
/// joins the last ordinary skill's `Skill.secondarySkills`).
pub fn tree_bases(db: &Database, skills: &[String]) -> Vec<Option<String>> {
    let mut last: Option<String> = None;
    skills
        .iter()
        .map(|sk| {
            let rec = db.get(sk);
            let class = rec.as_ref().map(|r| r.class.as_str()).unwrap_or("");
            if class == "Skill_Mastery" {
                last = None;
                None
            } else if is_modifier_class(class) {
                let sub = (!is_secondary_class(class))
                    .then(|| {
                        rec.as_ref()
                            .and_then(|r| r.str("subSkillName").map(str::to_owned))
                    })
                    .flatten();
                sub.or_else(|| last.clone())
            } else {
                last = Some(sk.clone());
                None
            }
        })
        .collect()
}

/// Mastery level each `skillTier` needs.
pub fn tier_levels(db: &Database) -> Vec<u32> {
    db.get("records/game/gameengine.dbr")
        .map(|r| {
            r.f32s("skillMasteryTierLevel")
                .into_iter()
                .map(|f| f as u32)
                .collect()
        })
        .filter(|v: &Vec<u32>| !v.is_empty())
        .unwrap_or_else(|| vec![1, 5, 10, 15, 20, 25, 32, 40, 50])
}

pub fn masteries_allowed(player_level: i32) -> usize {
    if player_level >= 10 {
        2
    } else if player_level >= 2 {
        1
    } else {
        0
    }
}

/// A skill class's hot slot flags `(IsPrimary, IsSecondary, IsAutoToggle)`:
/// `Skill::IsPrimary` / `IsSecondary` (`Skill.isPrimary` / `Skill.isSecondary`) are set by each
/// class's constructor (a subclass overrides its base), `IsAutoToggle`
/// (`Skill::IsAutoToggle`; `SkillActivatedBuffSelf.autoToggle`, the flag
/// `AutoActivateNow` needs) is on for every `SkillActivatedBuffSelf`
/// unless the class clears it. Game.dll constructors; classes not listed
/// (passives, modifiers, transmuters, buffs, secondary skills, masteries,
/// the `Skill_WPAttack…` pool attacks, `Skill_Kick`) are neither.
pub fn hot_slot_flags(class: &str) -> (bool, bool, bool) {
    let i = class_info(class);
    (i.primary, i.secondary, i.auto_toggle)
}

/// What a skill targets (`SkillTargetType`, `SkillActivated.targetType` read by
/// `Skill::GetTargetType`), set by each class's constructor, a
/// subclass keeping its base's. Game.dll constructors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetType {
    /// 0: `Skill_MonsterGenerator`, and classes that set none (passive
    /// modifiers, `Skill_Kick`, ...).
    None,
    /// 1: `SkillActivatedBuffSelf` and its subclasses (self / radius buffs,
    /// toggles, potions, `Skill_RefreshCooldown`, `Skill_Shapeshift`).
    Caster,
    /// 2: weapon and spell attacks (`SkillActivatedWeapon`,
    /// `SkillActivatedSpell`, the weapon pool), summons, passives.
    Enemy,
    /// 3: `Skill_BuffOther`, `Skill_ChargedBuffOther`, `Skill_GiveBonus`.
    Ally,
    /// 4: leaps, path charges, teleports, `Skill_DispelMagic`, `Skill_Evade`,
    /// `Skill_PetAttack`.
    Location,
}

pub fn target_type(class: &str) -> TargetType {
    class_info(class).target
}

/// Whether disruption puts the skill on cooldown (`Skill.disruptable`, read by `Skill::ApplyDisruptionCooldownTime`).
/// The constructors set it: `Skill::Skill` 0, `SkillActivated` (every active skill, the
/// default attacks too), `SkillBuff_Passive`, `SkillSecondary` and the odd specials (`Skill_BuffOther`,
/// `Skill_GiveBonus`, `Skill_Kick`, `Skill_Move`, `Skill_PetAttack`, ...) 1; these stock classes end at 0 (their own
/// constructor's 0, or a base without the flag: `Skill_Passive`, modifiers, transmuters, buffs and debuffs a skill
/// applies), resolved over the RTTI parents. A record's `ignoreDisruption` clears it (`Skill::LoadResources`).
/// The default attacks never meet it: `SkillManager::LoadDefaultSkills` keeps them out of the four lists
/// `ApplyCooldownDamage` walks.
pub fn disruptable(rec: &Record) -> bool {
    class_info(&rec.class).disruptable && rec.i32("ignoreDisruption").unwrap_or(0) == 0
}

/// How a skill gets to its target (`Skill_MoveType`, `Skill::GetMoveType` = `Skill.moveType`), set by the
/// constructors of these final classes, 1 for every other (Game.dll constructors, names from
/// `Skill::GetMoveTypeStateString`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveType {
    /// 1 `MoveToUseSkill`.
    MoveTo,
    /// 2 `ChargeToUseSkill`: `Skill_AttackPathCharge`.
    Charge,
    /// 3 `JumpToUseSkill`: `Skill_AttackRadiusLeap`.
    Leap,
    /// 4 `UseSkill`: `Skill_AttackRadiusTeleport`.
    Teleport,
    /// 5 `JumpToUseSkill`: `Skill_AttackRadiusDisengage`.
    Disengage,
    /// 6 `MoveAndUseSkill`: `Skill_AttackRadiusSpin`.
    MoveAndUse,
    /// 7 `EvadeTo`: `Skill_Evade`.
    Evade,
}

pub fn move_type(class: &str) -> MoveType {
    let is = |c: &str| class.eq_ignore_ascii_case(c);
    if is("Skill_AttackPathCharge") {
        MoveType::Charge
    } else if is("Skill_AttackRadiusLeap") {
        MoveType::Leap
    } else if is("Skill_AttackRadiusTeleport") {
        MoveType::Teleport
    } else if is("Skill_AttackRadiusDisengage") {
        MoveType::Disengage
    } else if is("Skill_AttackRadiusSpin") {
        MoveType::MoveAndUse
    } else if is("Skill_Evade") {
        MoveType::Evade
    } else {
        MoveType::MoveTo
    }
}

impl MoveType {
    /// `Skill::MoveToPointSkill`: move types 2 to 5 and 7.
    pub fn to_point(self) -> bool {
        !matches!(self, MoveType::MoveTo | MoveType::MoveAndUse)
    }
}

/// The distance profile of `Skill::GetGamepadRange` (and overrides): the skill's
/// own (`GetRange`, None) except `Skill_AttackRadiusSpin` / `Skill_TargetedSpawnPet` (`GameEngine.shortRangeProfile`,
/// the Short profile's range) and `Skill_AttackPathCharge` (`GameEngine.longRangeProfile`, Long).
pub fn gamepad_range_profile(class: &str) -> Option<&'static str> {
    let is = |c: &str| class.eq_ignore_ascii_case(c);
    if is("Skill_AttackRadiusSpin") || is("Skill_TargetedSpawnPet") {
        Some("Short")
    } else if is("Skill_AttackPathCharge") {
        Some("Long")
    } else {
        None
    }
}

/// A skill's `targetingMode` (`SkillProfile.targetingMode`, `SkillProfile::GetTargetingMode`),
/// read from its profile record (a wrapper's `buffSkillName`
/// record, [`profile_record`]).
///
/// Game.dll `SkillProfile::LoadProfile` (`SkillProfile::LoadTargetingMode`) and
/// `ResolveEnum_TargetingMode`: the exact strings `Default` 0, `Point` 1,
/// `Object` 2, `Target` 3; anything else (a missing key, the templates'
/// list `Default;Point;Object;Target`)
/// is `Default`. Readers (every one checks 1 or 3 only, so `Object` acts as
/// `Default`; no Grim Dawn.exe reader):
/// - the player's request ([`request_aim`]) and its out-of-range check
///   ([`refused_out_of_range`]);
/// - `Skill_BuffAttackRadiusDrop` / `SkillSecondary_…` `ActivateNow` /
///   `Update` (`Skill_BuffAttackRadiusDrop::StartDrop` / `Skill_BuffAttackRadiusDrop::Update`): `Point` drops around
///   the cast point (put on the floor, kept in range and on the path mesh), any
///   other mode around the caster's current coordinates, every interval;
/// - `Skill_AttackSpellTeleportSelf::ActivateNow`: `Target`
///   with a target lands short of it (`CharacterMovementManager::
///   GetPointAwayFromGoal`: caster + target radius before it,
///   along the path), else on the point;
/// - `Skill_TargetedSpawnPet::ActivateNow`, its secondary's
///   (`SkillSecondary_TargetedSpawnPet::ActivateNow`) and `Skill_AttackProjectileSpawnPet::TargetInformation`:
///   any mode but `Point` with a target blocks the target's
///   footprint before picking spawn spots;
/// - `Skill_AttackProjectileBurst::ActivateNow` and
///   `Skill_WPAttack_ProjectileBurst::ExecuteRangedAttack`:
///   `Target` aims at the target's `Target` attach point, else its feet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetingMode {
    Default,
    Point,
    Object,
    Target,
}

pub fn targeting_mode(profile: &Record) -> TargetingMode {
    match profile.str("targetingMode").unwrap_or("") {
        "Point" => TargetingMode::Point,
        "Object" => TargetingMode::Object,
        "Target" => TargetingMode::Target,
        _ => TargetingMode::Default,
    }
}

/// How the player's request aims a skill (the foe under the cursor or the
/// ground point).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestAim {
    /// The class has its own `GetValidTarget`: the target as aimed by the
    /// [`TargetType`].
    AsAimed,
    /// The target is dropped: the cast goes to the point.
    Point,
    /// A valid foe (a breakable or a hostile character) must be the target,
    /// else the request is refused.
    Foe,
}

/// Game.dll `SkillActivatedSpell::GetValidTarget`
/// (the `GetValidTarget` of the classes below; only the player's
/// `ControllerPlayerState::DefaultRequestSkillAction` and
/// `DefaultRequestInstantSkillAction` call it, a false return refuses the
/// request): `targetingMode` `Point`, `skillProjectileTargetGroundOnly` or
/// activation mode 1 (`Skill.activationMode`, of these classes only
/// `Skill_DefensiveWall`'s) clear the target and pass; otherwise
/// `Skill::GetValidRangedTarget` with radius 0 passes only for
/// a target `Skill::ValidateEnemy` accepts (a `Destructible`,
/// or a foe by faction). So a `Default` / `Target` spell needs a foe under
/// the cursor; a `Point` one goes to the ground point even over a foe. Monsters and pets
/// never ask.
pub fn request_aim(class: &str, profile: &Record) -> RequestAim {
    if !class_info(class).spell {
        return RequestAim::AsAimed;
    }
    if targeting_mode(profile) == TargetingMode::Point
        || profile.bool("skillProjectileTargetGroundOnly")
        || class.eq_ignore_ascii_case("Skill_DefensiveWall")
    {
        RequestAim::Point
    } else {
        RequestAim::Foe
    }
}

/// Whether a ranged skill requested beyond its range is refused rather than
/// cast at its range along the line.
///
/// Game.dll `ControllerPlayerStateUseSkill::OnBegin`:
/// with a range profile (`Skill::GetRangeProfile`, `Skill.rangeProfile`) and the
/// target beyond the skill's distance, `targetingMode` `Target` sets the
/// `Idle` state (nothing is cast or paid); any other mode drops the target
/// and moves the point to the range along the line.
pub fn refused_out_of_range(profile: &Record) -> bool {
    targeting_mode(profile) == TargetingMode::Target
}

/// Whether a hot slot's skill is used at once when its button is clicked
/// (`true`) or arms the slot for a target click (`false`).
///
/// Game.dll `HotSlotOptionSkill::Activate`: the
/// skill's activation mode (`Skill.activationMode`) 1 or 2 requests the skill at the
/// player's coordinates whatever the press; any other mode, from the
/// button, asks for a target (`PlayerHotSlotCtrl::ActivateHotSlot` keeps the
/// slot armed). The mode, set by the constructors (a subclass keeping its
/// base's): 1 for `SkillActivatedBuffSelf` and its subclasses but
/// `Skill_Teleport` (0), `Skill_AttackRadius`, `Skill_AttackProjectileRing`,
/// `Skill_DefensiveWall` and the `Skill_SpawnPet` family; 3 for
/// `Skill_PetAttack`; 0 else. The loaders change it: `instantCast` → 2 in
/// `Skill_AttackRadius::Load`, `SkillActivatedBuffSelf::LoadResources`
/// (`Skill_BuffRadius::Load` too) and `Skill_SpawnPet::LoadResources`;
/// `Skill_AttackBuffRadius::Load`: `pointBlank` → 1, `instantCast` → 2;
/// `Skill_DefensiveLine::Load` → 0.
pub fn used_at_once(class: &str, instant_cast: bool, point_blank: bool) -> bool {
    let eq = |c: &str| c.eq_ignore_ascii_case(class);
    if eq("Skill_DefensiveLine") {
        return false;
    }
    if eq("Skill_AttackBuffRadius") {
        return point_blank || instant_cast;
    }
    let i = class_info(class);
    i.at_once || (instant_cast && i.instant_loader)
}

/// Whether a held hot slot key repeats the skill (`Skill::IsRepeatable`
/// = `Skill.repeatable`, read by `HotSlotOptionSkill::IsRepeatable`
/// for the skill, potion and evade options; the scroll option's
/// is false).
///
/// Game.dll: `Skill::Skill` sets it to 1, no loader writes it, and
/// these constructors set it to 0 (their subclasses keep it): the
/// `Skill_BuffSelfToggled` / `Skill_BuffRadius` / `Skill_BuffAttackRadiusToggled`
/// families, `Skill_AttackBuff` (`Skill_AttackBuffRadius`), `Skill_DefensiveWall`
/// (`…Ground`, `…Line`), `Skill_SpawnPet` (and its pet / mirage subclasses),
/// `Skill_ChargePotion`, `Skill_GiveBonus`, `Skill_BuffOther`
/// (`Skill_ChargedBuffOther`), `Skill_DispelMagic`, `Skill_RefreshCooldown`,
/// `Skill_OnHitBuffSelf`, `Skill_OnHitAttackRadius`, the passive on-hit / on-life
/// buffs, `Skill_OnDeathSpawnActor` and the buff / radius secondaries.
pub fn repeatable(class: &str) -> bool {
    class_info(class).repeatable
}

/// The skill classes whose `Skill::ChainsAtEnd` answers 1 (Game.dll: the weapon-attack family, the weapon
/// pools and the `Skill_AttackProjectile` family; every other class's slot is `return 0`). The action matrix reads
/// it (`AttackAction::QueryActionPermission`): a skill of this kind requested during another one's
/// cast waits for its "End" even past `AllowInterrupt`, and is dropped while another skill request is queued.
pub fn chains_at_end(class: &str) -> bool {
    class_info(class).chains_at_end
}

/// What the rules read of a skill class, set by its constructor (a subclass keeping its base's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassInfo {
    /// [`hot_slot_flags`]: `IsPrimary`.
    pub primary: bool,
    /// [`hot_slot_flags`]: `IsSecondary`.
    pub secondary: bool,
    /// [`hot_slot_flags`]: `IsAutoToggle`.
    pub auto_toggle: bool,
    /// `SkillActivatedBuffSelf` or a subclass (Game.dll RTTI): the skills whose running cost
    /// `actor::active_cost` counts and whose tooltip says "auto-toggled" (`Skill::IsAutoToggle`).
    pub buff_self: bool,
    /// [`target_type`].
    pub target: TargetType,
    /// [`disruptable`] before the record's `ignoreDisruption`.
    pub disruptable: bool,
    /// [`repeatable`].
    pub repeatable: bool,
    /// [`chains_at_end`].
    pub chains_at_end: bool,
    /// [`request_aim`]: the class has `SkillActivatedSpell::GetValidTarget`.
    pub spell: bool,
    /// [`used_at_once`]: the constructor sets activation mode 1.
    pub at_once: bool,
    /// [`used_at_once`]: the loader turns `instantCast` into activation mode 2.
    pub instant_loader: bool,
}

/// One class's table entry; `None` / `false` where the class is in no list.
#[derive(Clone, Copy, Default)]
struct ClassEntry {
    hot: Option<(bool, bool, bool)>,
    buff_self: bool,
    target: Option<TargetType>,
    never_disrupt: bool,
    once: bool,
    chains: bool,
    spell: bool,
    at_once: bool,
    instant_loader: bool,
}

/// [`ClassInfo`] of `class` (case-insensitive).
pub fn class_info(class: &str) -> ClassInfo {
    static TABLE: LazyLock<Vec<(String, ClassEntry)>> = LazyLock::new(|| {
        let mut m: std::collections::BTreeMap<String, ClassEntry> = Default::default();
        let mut mark = |list: &[&str], f: &dyn Fn(&str, &mut ClassEntry)| {
            for &c in list {
                f(c, m.entry(c.to_ascii_lowercase()).or_default());
            }
        };
        // Earlier lists win (`get_or_insert`), as in the engine's checks.
        mark(HOT_BOTH, &|_, e| {
            e.hot.get_or_insert((true, true, false));
        });
        mark(HOT_SECONDARY, &|_, e| {
            e.hot.get_or_insert((false, true, false));
        });
        mark(HOT_AUTO, &|_, e| {
            e.hot.get_or_insert((false, true, true));
        });
        mark(TARGET_ALLY, &|_, e| {
            e.target.get_or_insert(TargetType::Ally);
        });
        mark(TARGET_LOCATION, &|_, e| {
            e.target.get_or_insert(TargetType::Location);
        });
        mark(BUFF_SELF, &|c, e| {
            e.buff_self = true;
            e.target.get_or_insert(TargetType::Caster);
            // `Skill_Teleport` sets activation mode 0 but keeps the loader.
            e.at_once = c != "Skill_Teleport";
            e.instant_loader = true;
        });
        mark(TARGET_NONE, &|_, e| {
            e.target.get_or_insert(TargetType::None);
        });
        mark(SPAWN_PETS, &|_, e| {
            e.at_once = true;
            e.instant_loader = true;
        });
        mark(&["Skill_AttackRadius"], &|_, e| {
            e.at_once = true;
            e.instant_loader = true;
        });
        mark(
            &["Skill_AttackProjectileRing", "Skill_DefensiveWall"],
            &|_, e| {
                e.at_once = true;
            },
        );
        mark(NEVER_DISRUPTED, &|_, e| e.never_disrupt = true);
        mark(NOT_REPEATABLE, &|_, e| e.once = true);
        mark(CHAINS_AT_END, &|_, e| e.chains = true);
        mark(SPELLS, &|_, e| e.spell = true);
        m.into_iter().collect()
    });
    let lower = class.bytes().map(|b| b.to_ascii_lowercase());
    let e = TABLE
        .binary_search_by(|(k, _)| k.bytes().cmp(lower.clone()))
        .map_or_else(|_| ClassEntry::default(), |i| TABLE[i].1);
    let skill = class.starts_with("Skill");
    let (primary, secondary, auto_toggle) = e.hot.unwrap_or_default();
    ClassInfo {
        primary,
        secondary,
        auto_toggle,
        buff_self: e.buff_self,
        target: e.target.unwrap_or(if skill {
            TargetType::Enemy
        } else {
            TargetType::None
        }),
        disruptable: skill && !e.never_disrupt,
        repeatable: !e.once,
        chains_at_end: e.chains,
        spell: e.spell,
        at_once: e.at_once,
        instant_loader: e.instant_loader,
    }
}

/// [`hot_slot_flags`]: primary and secondary (left mouse button and the other slots).
const HOT_BOTH: &[&str] = &[
    "Skill_AttackChain",
    "Skill_AttackInherent",
    "Skill_AttackLine",
    "Skill_AttackPathCharge",
    "Skill_AttackProjectile",
    "Skill_AttackProjectileBurst",
    "Skill_AttackProjectileChain",
    "Skill_AttackProjectileDebuf",
    "Skill_AttackProjectileMultiHit",
    "Skill_AttackProjectileOrbiting",
    "Skill_AttackRadius",
    "Skill_AttackRadiusLeap",
    "Skill_AttackRadiusLightning",
    "Skill_AttackRadiusLightning2",
    "Skill_AttackRadiusLightningSpawnPet",
    "Skill_AttackRadiusTeleport",
    "Skill_AttackSpell",
    "Skill_AttackSpellChaos",
    "Skill_AttackSpellChaosSpawnPet",
    "Skill_AttackSpellReflectLink",
    "Skill_AttackWave",
    "Skill_AttackWeapon",
    "Skill_AttackWeaponBlink",
    "Skill_AttackWeaponCharge",
    "Skill_AttackWeaponRangedSpread",
    "Skill_E3FauxAttack",
    "Skill_Move",
    "Skill_OrmenosChainLaser",
    "Skill_PetAttack",
    "Skill_TyphonSkillTransfer",
    "Skill_WeaponPool_BasicAttack",
    "Skill_WeaponPool_ChargedFinale",
    "Skill_WeaponPool_ChargedLinear",
    "Skill_WeaponPool_ChargedScaling",
    "Skill_WeaponPool_Default",
];

/// [`hot_slot_flags`]: secondary only, not auto-toggled.
const HOT_SECONDARY: &[&str] = &[
    "Skill_AktaiosLightOfRa",
    "Skill_AktaiosMirage",
    "Skill_AttackBuff",
    "Skill_AttackBuffRadius",
    "Skill_AttackLineFan",
    "Skill_AttackPattern",
    "Skill_AttackProjectileAreaEffect",
    "Skill_AttackProjectileDrop",
    "Skill_AttackProjectileFan",
    "Skill_AttackProjectileRing",
    "Skill_AttackProjectileSpawnPet",
    "Skill_AttackRadiusDisengage",
    "Skill_AttackRadiusGrow",
    "Skill_AttackRadiusSpin",
    "Skill_AttackSpellBeam",
    "Skill_AttackSpellCone",
    "Skill_AttackSpellDrain",
    "Skill_AttackSpellTeleport",
    "Skill_AttackSpellTeleportSelf",
    "Skill_AttackTelekinesis",
    "Skill_BuffAttackRadiusDuration",
    "Skill_BuffAttackRadiusLightning",
    "Skill_BuffOther",
    "Skill_BuffSelfColossus",
    "Skill_BuffSelfDuration",
    "Skill_BuffSelfImmobilize",
    "Skill_BuffSelfInvulnerable",
    "Skill_BuffSelfShield",
    "Skill_BuffSelfSick",
    "Skill_CerberusGeysers",
    "Skill_ChargePotion",
    "Skill_ChargedBuffOther",
    "Skill_CharonGeysers",
    "Skill_DefensiveGround",
    "Skill_DefensiveLine",
    "Skill_DefensiveWall",
    "Skill_DispelMagic",
    "Skill_DropProjectileTelekinesis",
    "Skill_Evade",
    "Skill_GiveBonus",
    "Skill_RefreshCooldown",
    "Skill_Shapeshift",
    "Skill_SpawnMiniPet",
    "Skill_SpawnPet",
    "Skill_SpawnPetMonster",
    "Skill_SpawnQuestPet",
    "Skill_TargetedSpawnPet",
    "Skill_TurretFireControl",
];

/// [`hot_slot_flags`]: secondary, auto-toggled (`SkillActivatedBuffSelf`'s default kept).
const HOT_AUTO: &[&str] = &[
    "Skill_BuffAttackRadiusToggled",
    "Skill_BuffRadius",
    "Skill_BuffRadiusToggled",
    "Skill_BuffSelfToggled",
    "Skill_OnHitAttackRadius",
    "Skill_OnHitBuffSelf",
    "Skill_Teleport",
    "SkillActivatedBuffSelf",
];

/// `SkillActivatedBuffSelf` and its subclasses ([`ClassInfo::buff_self`]; [`target_type`] `Caster`).
const BUFF_SELF: &[&str] = &[
    "SkillActivatedBuffSelf",
    "Skill_BuffAttackRadiusLightning",
    "Skill_BuffAttackRadiusDuration",
    "Skill_BuffAttackRadiusToggled",
    "Skill_BuffRadius",
    "Skill_BuffRadiusToggled",
    "Skill_BuffSelfColossus",
    "Skill_BuffSelfDuration",
    "Skill_BuffSelfImmobilize",
    "Skill_BuffSelfInvulnerable",
    "Skill_BuffSelfShield",
    "Skill_BuffSelfSick",
    "Skill_BuffSelfToggled",
    "Skill_ChargePotion",
    "Skill_OnHitAttackRadius",
    "Skill_OnHitBuffSelf",
    "Skill_RefreshCooldown",
    "Skill_Shapeshift",
    "Skill_Teleport",
];

/// [`target_type`] `Ally`.
const TARGET_ALLY: &[&str] = &[
    "Skill_BuffOther",
    "Skill_ChargedBuffOther",
    "Skill_GiveBonus",
];

/// [`target_type`] `Location`.
const TARGET_LOCATION: &[&str] = &[
    "Skill_AttackPathCharge",
    "Skill_AttackRadiusDisengage",
    "Skill_AttackRadiusLeap",
    "Skill_AttackRadiusTeleport",
    "Skill_DispelMagic",
    "Skill_Evade",
    "Skill_PetAttack",
];

/// [`target_type`] `None`: classes whose constructors leave it to a base without one (the passive /
/// modifier families and the odd specials).
const TARGET_NONE: &[&str] = &[
    "Skill_MonsterGenerator",
    "Skill_Kick",
    "Skill_Move",
    "Skill_MeleeModifier",
    "Skill_MeleeModifierRadius",
    "Skill_Modifier",
    "Skill_Transmuter",
    "Skill_OnDeathSpawnActor",
    "Skill_PassiveOnHitBuffSelf",
    "Skill_PassiveOnCritBuffSelf",
    "Skill_PassiveOnHitBuffShield",
    "Skill_PassiveOnLifeBuffSelf",
    "Skill_PlayAttackAnimation",
    "Skill_PotionModifier",
    "Skill_PotionContainer",
    "Skill_ProjectileModifier",
    "Skill_ProjectileTransmuter",
    "Skill_RefreshCooldownModifier",
    "Skill_SpawnMegalesiosSpirit",
    "Skill_SpawnPetTransmuter",
    "SkillActivated_Suicide",
];

/// [`used_at_once`]: the `Skill_SpawnPet` family (activation mode 1, `instantCast` loader).
const SPAWN_PETS: &[&str] = &[
    "Skill_SpawnPet",
    "Skill_SpawnMiniPet",
    "Skill_SpawnPetMonster",
    "Skill_SpawnQuestPet",
    "Skill_AktaiosMirage",
];

/// [`disruptable`]: the stock classes that end at 0.
const NEVER_DISRUPTED: &[&str] = &[
    "SkillBuff_BuffImmobilize",
    "SkillBuff_Contageous",
    "SkillBuff_Debuf",
    "SkillBuff_DebufFreeze",
    "SkillBuff_DebufProjectile",
    "SkillBuff_DebufRadius",
    "SkillBuff_DebufTrap",
    "SkillBuff_DispelMagic",
    "Skill_BuffSelfImmobilize",
    "Skill_ChargePotion",
    "Skill_Evade",
    "Skill_Mastery",
    "Skill_MeleeModifierRadius",
    "Skill_Modifier",
    "Skill_MonsterGenerator",
    "Skill_OnDeathSpawnActor",
    "Skill_Passive",
    "Skill_PassiveDualWieldWeapon",
    "Skill_PassiveOnCritBuffSelf",
    "Skill_PassiveOnHitBuffSelf",
    "Skill_PassiveOnHitBuffShield",
    "Skill_PassiveOnLifeBuffSelf",
    "Skill_PassiveShield",
    "Skill_PotionContainer",
    "Skill_PotionModifier",
    "Skill_ProjectileModifier",
    "Skill_ProjectileTransmuter",
    "Skill_RefreshCooldownModifier",
    "Skill_SpawnPetTransmuter",
    "Skill_Transmuter",
];

/// [`repeatable`]: the classes that set it to 0.
const NOT_REPEATABLE: &[&str] = &[
    "Skill_BuffSelfToggled",
    "Skill_BuffSelfDuration",
    "Skill_BuffSelfColossus",
    "Skill_BuffSelfImmobilize",
    "Skill_BuffSelfInvulnerable",
    "Skill_BuffSelfShield",
    "Skill_BuffSelfSick",
    "Skill_Shapeshift",
    "Skill_BuffRadius",
    "Skill_BuffRadiusToggled",
    "Skill_BuffAttackRadiusToggled",
    "Skill_BuffAttackRadiusDuration",
    "Skill_BuffAttackRadiusLightning",
    "Skill_AttackBuff",
    "Skill_AttackBuffRadius",
    "Skill_DefensiveWall",
    "Skill_DefensiveGround",
    "Skill_DefensiveLine",
    "Skill_SpawnPet",
    "Skill_SpawnMiniPet",
    "Skill_SpawnPetMonster",
    "Skill_SpawnQuestPet",
    "Skill_AktaiosMirage",
    "Skill_ChargePotion",
    "Skill_GiveBonus",
    "Skill_BuffOther",
    "Skill_ChargedBuffOther",
    "Skill_DispelMagic",
    "Skill_RefreshCooldown",
    "Skill_OnHitBuffSelf",
    "Skill_OnHitAttackRadius",
    "Skill_PassiveOnHitBuffSelf",
    "Skill_PassiveOnCritBuffSelf",
    "Skill_PassiveOnHitBuffShield",
    "Skill_PassiveOnLifeBuffSelf",
    "Skill_OnDeathSpawnActor",
    "SkillSecondary_BuffSelfDuration",
    "SkillSecondary_OnHitBuffRadius",
    "SkillSecondary_BuffRadius",
];

/// [`chains_at_end`]'s classes.
const CHAINS_AT_END: &[&str] = &[
    "SkillActivatedWeapon",
    "SkillActivatedWeaponPool",
    "Skill_AttackWeapon",
    "Skill_AttackWeaponBlink",
    "Skill_AttackWeaponCharge",
    "Skill_AttackWeaponRangedSpread",
    "Skill_AttackInherent",
    "Skill_AttackPathCharge",
    "Skill_AttackProjectile",
    "Skill_AttackProjectileBurst",
    "Skill_AttackProjectileChain",
    "Skill_AttackProjectileDebuf",
    "Skill_AttackProjectileOrbiting",
    "Skill_AttackProjectileSpawnPet",
    "Skill_WPAttack",
    "Skill_WPAttack_AttackWave",
    "Skill_WPAttack_BasicAttack",
    "Skill_WPAttack_ProjectileBurst",
    "Skill_WPAttack_RadialCrit",
    "Skill_WPAttack_Radius",
    "Skill_WeaponPool_BasicAttack",
    "Skill_WeaponPool_ChargedFinale",
    "Skill_WeaponPool_ChargedLinear",
    "Skill_WeaponPool_ChargedScaling",
    "Skill_WeaponPool_Default",
];

/// [`request_aim`]: the classes with `SkillActivatedSpell::GetValidTarget`.
const SPELLS: &[&str] = &[
    "SkillActivatedSpell",
    "Skill_AttackSpell",
    "Skill_AttackPattern",
    "Skill_E3FauxAttack",
    "Skill_AttackProjectileMultiHit",
    "Skill_TyphonSkillTransfer",
    "Skill_TurretFireControl",
    "Skill_TargetedSpawnPet",
    "Skill_OrmenosChainLaser",
    "Skill_DefensiveLine",
    "Skill_DefensiveGround",
    "Skill_DefensiveWall",
    "Skill_CharonGeysers",
    "Skill_AttackTelekinesis",
    "Skill_AttackSpellTeleportSelf",
    "Skill_AttackSpellTeleport",
    "Skill_AttackRadiusLightningSpawnPet",
    "Skill_AttackRadiusLightning2",
    "Skill_AttackRadiusLightning",
    "Skill_AttackProjectileChain",
    "Skill_AttackProjectileSpawnPet",
    "Skill_AttackProjectileOrbiting",
    "Skill_AttackProjectileDrop",
    "Skill_AttackProjectileDebuf",
    "Skill_AttackProjectileBurst",
    "Skill_AttackProjectile",
    "Skill_AttackChain",
    "Skill_AttackBuff",
    "Skill_AktaiosLightOfRa",
    "Skill_CerberusGeysers",
    "Skill_AttackProjectileAreaEffect",
    "Skill_DropProjectileTelekinesis",
];

/// Skills switched on and off from the hotbar: the toggled auras and the
/// on-hit retaliation skills (`Skill_OnHitAttackRadius` derives
/// from `SkillActivatedBuffSelf` like `Skill_BuffSelfToggled`, its
/// `ActivateNow` flips its on flag, its passive attributes —
/// `characterManaLimitReserve` among them — count only while on, sharing
/// `Skill_BuffAttackRadiusToggled::CollectPassiveCharAttributes`).
pub fn is_toggle(class: &str) -> bool {
    class.ends_with("Toggled") || class == "Skill_OnHitAttackRadius"
}

/// The `MasteryEnumeration` of a tree's mastery bar (its first skill).
pub fn tree_enumeration(db: &Database, skills: &[String]) -> Option<u32> {
    let r = db.get(skills.first()?)?;
    let n = r
        .str("MasteryEnumeration")?
        .strip_prefix("SkillClass")?
        .parse::<u32>()
        .ok()?;
    Some(n.saturating_sub(1))
}

/// A class tree with what the raise / refund rules read of it.
#[derive(Debug, Clone)]
pub struct ClassTree {
    pub path: String,
    /// The class number NN of `_classtree_classNN.dbr` ([`class_trees`]).
    pub class: u32,
    /// [`tree_skills`].
    pub skills: Vec<String>,
    /// [`tree_bases`] of `skills`.
    pub bases: Vec<Option<String>>,
    /// [`tree_enumeration`]: the [`SkillTrees::mastery_enumeration`] of the tree's skills.
    pub enumeration: Option<u32>,
}

impl ClassTree {
    /// The tree's mastery bar (its first skill).
    pub fn mastery(&self) -> Option<&str> {
        self.skills.first().map(String::as_str)
    }

    /// Whether `skill` is in the tree.
    pub fn contains(&self, skill: &str) -> bool {
        self.skills.iter().any(|s| s.eq_ignore_ascii_case(skill))
    }

    /// `skill`'s base skills: one per place it appears in the tree that has one.
    pub fn bases_of<'a>(&'a self, skill: &'a str) -> impl Iterator<Item = &'a str> {
        self.skills
            .iter()
            .zip(&self.bases)
            .filter(move |(s, _)| s.eq_ignore_ascii_case(skill))
            .filter_map(|(_, b)| b.as_deref())
    }
}

/// Every class tree ([`class_trees`] order) and what is derived from them.
#[derive(Debug, Clone)]
pub struct SkillTrees {
    pub trees: Vec<ClassTree>,
    /// [`tier_levels`].
    pub tiers: Vec<u32>,
    /// Parent skill of every modifier in the class trees (lower-case modifier path → parent path): its
    /// first base skill in tree order.
    pub parents: HashMap<String, String>,
    /// [`potion_parents`].
    pub potion_parents: HashMap<String, String>,
}

/// The class trees with their bases and enumerations.
fn load_trees(db: &Database) -> Vec<ClassTree> {
    class_trees(db)
        .into_iter()
        .map(|(class, path)| {
            let skills = tree_skills(db, &path);
            ClassTree {
                bases: tree_bases(db, &skills),
                enumeration: tree_enumeration(db, &skills),
                class,
                skills,
                path,
            }
        })
        .collect()
}

/// [`SkillTrees::parents`] of loaded trees.
fn parents_of(trees: &[ClassTree]) -> HashMap<String, String> {
    let mut parent = HashMap::default();
    for t in trees {
        for (sk, base) in t.skills.iter().zip(&t.bases) {
            if let Some(b) = base {
                parent
                    .entry(sk.to_ascii_lowercase())
                    .or_insert_with(|| b.clone());
            }
        }
    }
    parent
}

/// The potion modifiers' and containers' potion (lower-case path → the potion's path): the player
/// record's `customPotionTree1/2`, each entry that is a modifier or potion class belonging to the
/// tree's last other entry, the potion (`Player::Load` → `SkillManager::LoadSkills`; the potion's
/// `Skill.modifiers` like any learned modifier).
fn potion_parents(db: &Database) -> HashMap<String, String> {
    let mut parents = HashMap::default();
    let Some(pc) = db.get("records/creatures/pc/malepc01.dbr") else {
        return parents;
    };
    for key in ["customPotionTree1", "customPotionTree2"] {
        let Some(tree) = pc.str(key) else { continue };
        let mut base: Option<String> = None;
        for sk in tree_skills(db, tree) {
            let class = db.get(&sk).map(|r| r.class.clone()).unwrap_or_default();
            if is_potion_class(&class) || is_modifier_class(&class) {
                if let Some(b) = &base {
                    parents
                        .entry(sk.to_ascii_lowercase())
                        .or_insert_with(|| b.clone());
                }
            } else {
                base = Some(sk);
            }
        }
    }
    parents
}

impl SkillTrees {
    pub fn load(db: &Database) -> Self {
        let trees = load_trees(db);
        Self {
            parents: parents_of(&trees),
            potion_parents: potion_parents(db),
            tiers: tier_levels(db),
            trees,
        }
    }

    /// The tree at record path `path`.
    pub fn tree(&self, path: &str) -> Option<&ClassTree> {
        self.trees
            .iter()
            .find(|t| t.path.eq_ignore_ascii_case(path))
    }

    /// The tree whose mastery bar is `mastery`.
    pub fn mastery_tree(&self, mastery: &str) -> Option<&ClassTree> {
        self.trees
            .iter()
            .find(|t| t.mastery().is_some_and(|m| m.eq_ignore_ascii_case(mastery)))
    }

    /// The parent of modifier `skill` ([`Self::parents`]); with `potions` the potion trees' too
    /// (the class trees' first).
    pub fn parent(&self, skill: &str, potions: bool) -> Option<&str> {
        let low = lower(skill);
        self.parents
            .get(low.as_ref())
            .or_else(|| {
                potions
                    .then(|| self.potion_parents.get(low.as_ref()))
                    .flatten()
            })
            .map(String::as_str)
    }

    /// `Skill_Mastery::Load`'s `MasteryEnumeration` (`SkillClassNN` → NN − 1, `Skill_Mastery.masteryEnumeration`)
    /// of the mastery whose class tree holds `skill` (the tree's first skill is the mastery
    /// bar); None outside every tree (an item or devotion skill).
    pub fn mastery_enumeration(&self, skill: &str) -> Option<u32> {
        self.trees
            .iter()
            .filter(|t| t.contains(skill))
            .find_map(|t| t.enumeration)
    }

    /// The trees whose mastery bar has a point: the character's masteries.
    pub fn learned<'a>(&'a self, book: &'a SkillBook) -> impl Iterator<Item = &'a ClassTree> {
        self.trees
            .iter()
            .filter(|t| t.mastery().is_some_and(|m| book.level(m) > 0))
    }

    /// The number of trees whose mastery bar has a point ([`Self::learned`]).
    pub fn masteries(&self, book: &SkillBook) -> usize {
        self.learned(book).count()
    }

    /// The mastery level `skill`'s tier needs (`Skill::GetMasteryLevelRequirement`:
    /// `skillTier` of the display record, else the skill's, default 1).
    pub fn tier_requirement(&self, db: &Database, skill: &str) -> u32 {
        let tier = display_record(db, skill)
            .and_then(|d| d.i32("skillTier"))
            .or_else(|| db.get(skill).and_then(|r| r.i32("skillTier")))
            .unwrap_or(1)
            .max(1) as usize;
        self.tiers.get(tier - 1).copied().unwrap_or(1)
    }
}

/// What `Skill::ExclusiveSkillCheck` compares between an
/// exclusive skill coming on and the manager's current exclusive skill:
/// the level `Skill.level` (`SetSkillLevel`: the points spent, no item
/// bonus), then the mastery's enumeration (`Skill.masteryId` →
/// `Skill_Mastery.masteryEnumeration`), then the augmented level `Skill.augmentedLevel`
/// (`SetAugmentedSkillLevel`: the item bonus levels; `GetCurrentLevel` is
/// the two summed, capped).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExclusiveRank {
    pub level: u32,
    pub mastery: Option<u32>,
    pub augment: u32,
}

impl ExclusiveRank {
    /// `ExclusiveSkillCheck`: may `self` come on while `current` runs? Only
    /// when its level is higher; on equal levels the higher mastery; on
    /// equal masteries the higher augment. A missing mastery on either side
    /// ends an equal-level check with "no".
    pub fn beats(&self, current: &ExclusiveRank) -> bool {
        if self.level != current.level {
            return self.level > current.level;
        }
        match (self.mastery, current.mastery) {
            (Some(a), Some(b)) if a != b => a > b,
            (Some(_), Some(_)) => self.augment > current.augment,
            _ => false,
        }
    }
}

/// Weapon charges: `Skill_AttackWeaponCharge` and its one subclass
/// `Skill_AttackWeaponBlink` (RTTI: Blink's `classInfo` parent).
/// The player's out-of-range use of one goes to "ChargeToUseSkill" instead
/// of "MoveToUseSkill" (`ControllerPlayerState::DefaultRequestSkillAction`
/// tests `IsKindOf(Skill_AttackWeaponCharge)`).
pub fn is_weapon_charge(class: &str) -> bool {
    matches!(
        class,
        "Skill_AttackWeaponCharge" | "Skill_AttackWeaponBlink"
    )
}

/// The run speed multiplier of a skill's warm-up move: the character's own
/// multiplier (`run`, 1 = 100 %) plus the skill's `characterRunSpeedModifier`
/// % (`Skill_AttackWeapon::CollectPassiveCharAttributes` counts the profile's
/// stats while the warm-up flag is on), held under `absoluteRunSpeedCapMax` %
/// instead of the normal cap (`Character::GetRunSpeed` uses
/// `CapAbsoluteRunSpeed` while `Character.skillWarmUp` is set, which
/// `MoveToAction::Execute` / `Character::SkillWarmUp` set).
pub fn warmup_run_multiplier(run: f32, bonus_pct: f32, cap_pct: f32) -> f32 {
    (run + bonus_pct * 0.01).min(cap_pct * 0.01)
}

/// Learned skills: (record, level).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SkillBook {
    pub skills: Vec<(String, u32)>,
    /// Toggled skills that are off (lower-case; save: `skill_active` 0):
    /// newly learned, stopped by weapons, death or a use; the player's auto
    /// toggle switches them on again.
    pub inactive: gd_formats::collections::HashSet<String>,
    /// Toggled skills a dispel stopped (lower-case; not saved): off like
    /// [`Self::inactive`] until the character's auto toggle turns them on
    /// again.
    pub dispelled: gd_formats::collections::HashSet<String>,
    /// Bonus levels from equipment, per skill record (lower-case).
    pub bonus: gd_formats::collections::HashMap<String, u32>,
    /// Locked skills (lower-case; save `unk_205`): potion modifiers until
    /// unlocked (`potions`).
    pub locked: gd_formats::collections::HashSet<String>,
    /// Potion containers' unlock levels (lower-case; save `sublevel`).
    pub unlock: gd_formats::collections::HashMap<String, u32>,
    /// The worn gear's pet modifiers ([`gear_pet_modifiers`]: lower-case pet skill, level), set by
    /// [`Self::with_item_bonuses`].
    pub gear_pet_mods: Vec<(String, u32)>,
    /// The worn items' and sets' skill grants ([`item_skill_grants`]), set by
    /// [`Self::with_item_bonuses`].
    pub item_grants: Vec<ItemSkillGrant>,
}

/// An item's records that carry skill bonuses, grants and modifiers: base, affixes, modifier,
/// component, its completion bonus, augment, ascendant affix.
fn item_records(it: &gd_formats::save::item::Item) -> [&String; 8] {
    [
        &it.base_name,
        &it.prefix_name,
        &it.suffix_name,
        &it.modifier_name,
        &it.relic_name,
        &it.relic_bonus,
        &it.augment_name,
        &it.ascendant_name,
    ]
}

/// The pet skills the worn gear's `SkillSecondary_PetModifier`s raise: (lower-case `petSkillName`, 1)
/// per `modifierSkillName{n}` of an item's records ([`item_records`]; stock: legendaries and ascendant
/// affixes): `SkillManager::AddItemSkillModifiers` gives an item's modifier skills level 1 and
/// `UpdatePetSkill` then sets the pet skill to that level, like a learned one's.
pub fn gear_pet_modifiers(
    db: &Database,
    items: &[gd_formats::save::item::Item],
) -> Vec<(String, u32)> {
    items
        .iter()
        .flat_map(item_records)
        .filter(|n| !n.is_empty())
        .filter_map(|n| db.get(n))
        .flat_map(|r| {
            (1..=8)
                .filter_map(|k| r.str(&format!("modifierSkillName{k}")).map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .filter_map(|m| {
            let mr = db.get(&m)?;
            (mr.class == "SkillSecondary_PetModifier")
                .then(|| mr.str("petSkillName").map(|p| (p.to_ascii_lowercase(), 1)))?
        })
        .collect()
}

/// Skill bonuses (record, levels), mastery bonuses, the all-skills bonus.
pub type SkillBonuses = (Vec<(String, u32)>, Vec<(String, u32)>, u32);

/// `augmentSkillName{n}`, `augmentSkillLevel{n}`, `augmentMasteryName{n}`, `augmentMasteryLevel{n}`
/// for n = 1..=5.
static AUGMENT_KEYS: LazyLock<[[String; 4]; 5]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        let n = i + 1;
        [
            format!("augmentSkillName{n}"),
            format!("augmentSkillLevel{n}"),
            format!("augmentMasteryName{n}"),
            format!("augmentMasteryLevel{n}"),
        ]
    })
});

/// Skill bonuses of one item ([`item_records`]):
/// `augmentSkillName/Level{n}`, `augmentMasteryName/Level{n}` and
/// `augmentAllLevel` → (skill bonuses, mastery bonuses, all-skills bonus).
pub fn item_skill_bonuses(db: &Database, item: &gd_formats::save::item::Item) -> SkillBonuses {
    let (mut skills, mut masteries, mut all) = (Vec::new(), Vec::new(), 0);
    for rec in item_records(item) {
        if rec.is_empty() {
            continue;
        }
        let Some(r) = db.get(rec) else { continue };
        for [skill_name, skill_level, mastery_name, mastery_level] in AUGMENT_KEYS.iter() {
            if let (Some(s), Some(l)) = (r.str(skill_name), r.i32(skill_level)) {
                skills.push((s.to_ascii_lowercase(), l.max(0) as u32));
            }
            if let (Some(m), Some(l)) = (r.str(mastery_name), r.i32(mastery_level)) {
                masteries.push((m.to_ascii_lowercase(), l.max(0) as u32));
            }
        }
        all += r.i32("augmentAllLevel").unwrap_or(0).max(0) as u32;
    }
    (skills, masteries, all)
}

/// The equipped items' skill bonuses together ([`item_skill_bonuses`] of each)
/// and the worn sets' at their piece counts
/// ([`crate::items::sets::set_skill_bonuses`]).
pub fn item_augments(db: &Database, items: &[gd_formats::save::item::Item]) -> SkillBonuses {
    let (mut skills, mut masteries, mut all) = (Vec::new(), Vec::new(), 0);
    for it in items {
        let (sk, ms, a) = item_skill_bonuses(db, it);
        skills.extend(sk);
        masteries.extend(ms);
        all += a;
    }
    let (sk, ms, a) = crate::items::sets::set_skill_bonuses(db, items);
    skills.extend(sk);
    masteries.extend(ms);
    all += a;
    (skills, masteries, all)
}

/// The character sheet's skill bonus rows: the all-skills bonus and each
/// class's mastery bonus in class order 1..=10 (Soldier ... Berserker), keyed
/// by the class's mastery record (`Character::GetTotalAllLevelAugment` /
/// `GetTotalMasteryLevelAugment(mastery)`: a map of mastery name →
/// summed `augmentMasteryLevel`, the all-skills bonus under its own key).
pub fn sheet_augments(
    db: &Database,
    trees: &SkillTrees,
    items: &[gd_formats::save::item::Item],
) -> (u32, [u32; 10]) {
    let (_, ms, all) = item_augments(db, items);
    let mut out = [0u32; 10];
    for t in &trees.trees {
        let Some(mastery) = t.mastery() else { continue };
        out[(t.class - 1) as usize] = ms
            .iter()
            .filter(|(m, _)| m.eq_ignore_ascii_case(mastery))
            .map(|m| m.1)
            .sum();
    }
    (all, out)
}

/// A skill a worn item or set grants (`itemSkillName`; the engine's item
/// skill list, `SkillManager::AddItemSkills`).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemSkillGrant {
    pub skill: String,
    /// `itemSkillAutoController` (none: a hot slot / toggle skill).
    pub controller: Option<String>,
    pub level: u32,
    /// The granting item (index into the worn items); none for a set's.
    pub item: Option<usize>,
}

/// Every skill the worn items grant: each item's records in order ([`item_records`]; level
/// `itemSkillLevelEq` with `itemLevel` = the item's `itemLevel`, else `itemSkillLevel`, else 1),
/// then the worn sets' ([`crate::items::sets::set_skills`]).
pub fn item_skill_grants(
    db: &Database,
    items: &[gd_formats::save::item::Item],
) -> Vec<ItemSkillGrant> {
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let mut item_level = None;
        for name in item_records(it) {
            if name.is_empty() {
                continue;
            }
            let Some(r) = db.get(name) else { continue };
            let Some(skill) = r.str("itemSkillName") else {
                continue;
            };
            let item_level = *item_level.get_or_insert_with(|| {
                db.get(&it.base_name)
                    .and_then(|b| b.f32("itemLevel"))
                    .unwrap_or(1.0)
            });
            let lv = r
                .str("itemSkillLevelEq")
                .and_then(|e| gd_rules::expr::eval_var(e, "itemLevel", item_level))
                .or(r.f32("itemSkillLevel"))
                .unwrap_or(1.0);
            out.push(ItemSkillGrant {
                skill: skill.to_owned(),
                controller: r.str("itemSkillAutoController").map(str::to_owned),
                level: lv.max(1.0) as u32,
                item: Some(i),
            });
        }
    }
    out.extend(
        crate::items::sets::set_skills(db, items)
            .into_iter()
            .map(|s| ItemSkillGrant {
                skill: s.skill,
                controller: s.controller,
                level: s.level,
                item: None,
            }),
    );
    out
}

/// Level of `skill` among `grants` ([`item_skill_grants`]): the highest; 0 when none grants it.
pub fn granted_level(grants: &[ItemSkillGrant], skill: &str) -> u32 {
    grants
        .iter()
        .filter(|g| g.skill.eq_ignore_ascii_case(skill))
        .map(|g| g.level)
        .max()
        .unwrap_or(0)
}

impl SkillBook {
    /// A toggled skill (lower-case) that is off ([`Self::inactive`]) or
    /// stopped by a dispel ([`Self::dispelled`]).
    pub fn switched_off(&self, low: &str) -> bool {
        self.inactive.contains(low) || self.dispelled.contains(low)
    }

    /// Adds the skill bonuses of equipped items (`SkillManager::SetSkillLevelAugment`,
    /// `SetMasteryAugment`: every skill of the mastery and the mastery bar,
    /// `SetAllSkillAugment`).
    pub fn with_item_bonuses(
        &self,
        db: &Database,
        trees: &SkillTrees,
        items: &[gd_formats::save::item::Item],
    ) -> SkillBook {
        let mut bonus: HashMap<String, u32> = HashMap::default();
        let (sk, ms, all) = item_augments(db, items);
        for (s, l) in sk {
            *bonus.entry(s).or_insert(0) += l;
        }
        for (m, l) in ms {
            let Some(t) = trees.mastery_tree(&m) else {
                continue;
            };
            // `SetMasteryAugment` raises each skill object of the mastery once; a skill listed
            // twice in the tree (`CreateAndLoadSkill` reuses its object) counts once.
            for (i, s) in t.skills.iter().enumerate() {
                if t.skills[..i].iter().any(|p| p.eq_ignore_ascii_case(s)) {
                    continue;
                }
                *bonus.entry(s.to_ascii_lowercase()).or_insert(0) += l;
            }
        }
        if all > 0 {
            for (s, _) in &self.skills {
                *bonus.entry(s.to_ascii_lowercase()).or_insert(0) += all;
            }
        }
        SkillBook {
            skills: self.skills.clone(),
            inactive: self.inactive.clone(),
            dispelled: self.dispelled.clone(),
            bonus,
            locked: self.locked.clone(),
            unlock: self.unlock.clone(),
            gear_pet_mods: gear_pet_modifiers(db, items),
            item_grants: item_skill_grants(db, items),
        }
    }

    /// `Skill::GetCurrentLevel`: learned level + item bonuses, capped at the
    /// ultimate level (`skillUltimateLevel`, else `skillMaxLevel`); 0 while
    /// unlearned.
    pub fn effective_level(&self, db: &Database, skill: &str) -> u32 {
        let base = self.level(skill);
        if base == 0 {
            return 0;
        }
        let bonus = self
            .bonus
            .get(&skill.to_ascii_lowercase())
            .copied()
            .unwrap_or(0);
        if bonus == 0 {
            return base;
        }
        let cap = self.ultimate_level(db, skill);
        (base + bonus).min(cap.max(base))
    }

    /// `Skill::GetUltimateLevel`: `SkillProfile::LoadProfile` stores
    /// `max(skillUltimateLevel, skillMaxLevel)` of the skill's own record;
    /// u32::MAX when neither is set.
    pub fn ultimate_level(&self, db: &Database, skill: &str) -> u32 {
        let own = db.get(skill);
        let both = |r: &Record| {
            r.i32("skillUltimateLevel")
                .unwrap_or(0)
                .max(r.i32("skillMaxLevel").unwrap_or(0))
        };
        let c = own.map_or(0, both);
        let c = if c > 0 {
            c
        } else {
            display_record(db, skill).map_or(0, |r| both(&r))
        };
        if c > 0 { c as u32 } else { u32::MAX }
    }

    pub fn level(&self, skill: &str) -> u32 {
        self.skills
            .iter()
            .find(|(s, _)| s.eq_ignore_ascii_case(skill))
            .map_or(0, |x| x.1)
    }

    fn set(&mut self, skill: &str, level: u32) {
        match self
            .skills
            .iter_mut()
            .find(|(s, _)| s.eq_ignore_ascii_case(skill))
        {
            Some(x) => x.1 = level,
            None => self.skills.push((skill.to_owned(), level)),
        }
    }

    /// The skill window's f5 flag (`UISkillPane::GetRequirementFlags`): the dependency is unmet
    /// (`SkillProfile::GetSkillDependancies`: `skillDependancy` names; with `skillDependancyAll` any listed skill at level
    /// 0 fails it, else it fails only when none of them is learned; an empty list never fails). It also refuses the "+"
    /// click ([`Self::cannot_raise`]).
    pub fn dependency_unmet(&self, db: &Database, skill: &str) -> bool {
        let (deps, all) = skill_dependencies(db, skill);
        if deps.is_empty() {
            return false;
        }
        let learned = |d: &String| self.level(d) > 0;
        if all {
            deps.iter().any(|d| !learned(d))
        } else {
            !deps.iter().any(learned)
        }
    }

    /// `skill`'s [`ExclusiveRank`]: the points spent, its tree's mastery enumeration, the item bonus levels.
    pub fn exclusive_rank(&self, trees: &SkillTrees, skill: &str) -> ExclusiveRank {
        ExclusiveRank {
            level: self.level(skill),
            mastery: trees.mastery_enumeration(skill),
            augment: self
                .bonus
                .get(&skill.to_ascii_lowercase())
                .copied()
                .unwrap_or(0),
        }
    }

    /// `UISkillPane::HasExclusiveConflict` (the skill window's f6 flag and the skill tooltip's reason `[6]`): when
    /// `skill` is `exclusiveSkill` (its profile's `IsExclusiveSkill`), the first other exclusive skill of the skill
    /// manager's
    /// active list (`SkillManager::GetActiveSkillList`; here a learned toggle that is on) that beats it as
    /// `ExclusiveSkillCheck` would ([`ExclusiveRank::beats`]: the higher level, on equal levels the higher mastery
    /// enumeration, both resolved, then the higher augmented level).
    pub fn exclusive_conflict(
        &self,
        db: &Database,
        trees: &SkillTrees,
        skill: &str,
    ) -> Option<&str> {
        let exclusive = |n: &str| profile_record(db, n).is_some_and(|r| r.bool("exclusiveSkill"));
        if !exclusive(skill) {
            return None;
        }
        let mine = self.exclusive_rank(trees, skill);
        self.skills
            .iter()
            .find(|(n, lv)| {
                *lv > 0
                    && !n.eq_ignore_ascii_case(skill)
                    && !self.switched_off(&n.to_ascii_lowercase())
                    && db.get(n).is_some_and(|r| is_toggle(&r.class))
                    && exclusive(n)
                    && self.exclusive_rank(trees, n).beats(&mine)
            })
            .map(|(n, _)| n.as_str())
    }

    /// The skill window's f5 and f6 flags: [`Self::dependency_unmet`] or [`Self::exclusive_conflict`].
    pub fn dependency_or_exclusive_blocked(
        &self,
        db: &Database,
        trees: &SkillTrees,
        skill: &str,
    ) -> bool {
        self.dependency_unmet(db, skill) || self.exclusive_conflict(db, trees, skill).is_some()
    }

    /// Why one more point cannot go into `skill` of `tree`, if it cannot: the skill window's flags f1..f6
    /// (`UISkillPane::GetRequirementFlags`; f0, no points left, is the caller's). Any of them disables the
    /// skill button, so an exclusive conflict refuses the point like an unmet dependency.
    /// `tree` is one of `trees`.
    pub fn cannot_raise(
        &self,
        db: &Database,
        trees: &SkillTrees,
        tree: &ClassTree,
        skill: &str,
        player_level: i32,
    ) -> Option<&'static str> {
        let Some(idx) = tree
            .skills
            .iter()
            .position(|s| s.eq_ignore_ascii_case(skill))
        else {
            return Some("not in this tree");
        };
        let Some(disp) = display_record(db, skill) else {
            return Some("unknown skill");
        };
        let max = disp.i32("skillMaxLevel").unwrap_or(1).max(1) as u32;
        if self.level(skill) >= max {
            return Some("at maximum level");
        }
        let mastery = &tree.skills[0];
        if idx == 0 {
            // A new mastery needs a free mastery slot.
            if self.level(mastery) == 0 && trees.masteries(self) >= masteries_allowed(player_level)
            {
                return Some("no mastery slot");
            }
            return None;
        }
        if self.level(mastery) < trees.tier_requirement(db, skill) {
            return Some("mastery level too low");
        }
        // `IsBaseSkillEnabled`: one of its bases (every place the skill
        // appears in the tree adds one) has a level.
        let mut bases = tree.bases_of(skill).peekable();
        if bases.peek().is_some() && bases.all(|b| self.level(b) == 0) {
            return Some("needs the parent skill");
        }
        if self.dependency_unmet(db, skill) {
            return Some("needs a prerequisite skill");
        }
        if self.exclusive_conflict(db, trees, skill).is_some() {
            return Some("a more powerful exclusive skill is active");
        }
        None
    }

    /// Takes `n` levels off `skill` (the skills window's undo); the entry stays
    /// at level 0 when emptied.
    pub fn lower(&mut self, skill: &str, n: u32) {
        let l = self.level(skill);
        self.set(skill, l.saturating_sub(n));
    }

    /// Gives `n` levels back to `skill` without the raise rules (the undo of a refund,
    /// `Skill::IncrementSkillLevel`).
    pub fn restore(&mut self, skill: &str, n: u32) {
        let l = self.level(skill);
        self.set(skill, l + n);
    }

    /// Spends one point, or says why it cannot ([`Self::cannot_raise`]).
    pub fn try_raise(
        &mut self,
        db: &Database,
        trees: &SkillTrees,
        tree: &ClassTree,
        skill: &str,
        player_level: i32,
    ) -> Result<(), &'static str> {
        if let Some(why) = self.cannot_raise(db, trees, tree, skill, player_level) {
            return Err(why);
        }
        let l = self.level(skill);
        self.set(skill, l + 1);
        Ok(())
    }

    /// Learned skills that feed stats: (record, level index). Masteries,
    /// passives and toggled skills (`Skill_Buff{Self,Radius}Toggled` with
    /// their `buffSkillName` record, `Skill_OnHitAttackRadius`) while
    /// switched on contribute their per-level fields, and so do modifiers
    /// (`Skill_Modifier`) of those skills (parent = [`SkillTrees::parents`]).
    /// The toggles' `CollectPassiveCharAttributes`
    /// (`Skill_BuffSelfToggled`, `Skill_BuffRadius`,
    /// `Skill_BuffAttackRadiusToggled`) add the skill's and its
    /// modifiers' attributes only while its active flag is set; the save
    /// keeps that flag (`SkillActiveState` `skillActive` → `inactive`).
    ///
    /// A radius aura's own cost and the buff it emits are two stores of one
    /// profile: `Skill_BuffRadius::Load` builds the
    /// toggle's `SkillProfile` from its `buffSkillName` table, and its
    /// `CollectPassiveCharAttributes` adds only the profile's
    /// `CharAttributeStore_SkillPenalty` (`SkillProfile.penaltyStore`, [`PENALTY_FIELDS`]) and its
    /// modifiers' penalty stores (`AddModifierCharAttributes(.., 0, 1)`); the
    /// rest (`CharAttributeStore_Skill`, `SkillProfile.skillStore`) reaches characters only as the
    /// `SkillBuff` the aura puts on them (`SkillBuff::
    /// CollectPassiveCharAttributes`: the skill store and the modifiers'
    /// skill stores, never the reserve), the caster included only for a non-debuff
    /// aura. So a debuff aura (`debufSkill` buff record)
    /// costs its caster the reserve and nothing else; a buff aura's caster
    /// gets both halves = the whole record. `Skill_BuffSelfToggled`
    /// adds both stores and its modifiers' both: the whole
    /// record too.
    pub fn stat_sources(&self, db: &Database, trees: &SkillTrees) -> Vec<(Record, usize)> {
        const STAT: [&str; 5] = [
            "Skill_Mastery",
            "Skill_Passive",
            "Skill_BuffSelfToggled",
            "Skill_BuffRadiusToggled",
            "Skill_OnHitAttackRadius",
        ];
        let parent = &trees.parents;
        self.skills
            .iter()
            .filter(|(_, l)| *l > 0)
            .map(|(s, _)| (s, self.effective_level(db, s)))
            .filter_map(|(s, l)| {
                // A toggle switched off gives nothing, nor do its modifiers.
                let low = s.to_ascii_lowercase();
                let p = parent.get(&low);
                if self.switched_off(&low) || p.is_some_and(|p| self.switched_off(&lower(p))) {
                    return None;
                }
                let r = db.get_owned(s)?;
                let modifier = r.class == "Skill_Modifier";
                let pr = p.filter(|_| modifier).and_then(|p| db.get(p));
                let stat = STAT.contains(&r.class.as_str())
                    || pr.as_ref().is_some_and(|pr| {
                        p.is_some_and(|p| self.level(p) > 0) && STAT.contains(&pr.class.as_str())
                    });
                // A debuff aura's modifier: only its penalty store.
                let r = if pr.as_ref().is_some_and(|pr| is_debuff_aura(db, pr)) {
                    penalty_only(r)
                } else {
                    r
                };
                stat.then(|| (r, (l - 1) as usize))
            })
            // Toggled auras keep their effect in a linked buff record.
            .flat_map(|(r, li)| {
                let debuff = is_debuff_aura(db, &r);
                let buff = r
                    .class
                    .ends_with("Toggled")
                    .then(|| profile_name(&r).and_then(|b| db.get_owned(b)))
                    .flatten();
                let buff = buff.map(|b| if debuff { penalty_only(b) } else { b });
                std::iter::once((r, li)).chain(buff.map(|b| (b, li)))
            })
            .collect()
    }
}

/// What a `Skill_WeaponPool_ChargedScaling` (Savagery, Righteous Fervor, item skills) adds to its character
/// while it has charges: `Skill_WeaponPool_ChargedScaling::CollectPassive*` run only while charged
/// (`Skill_WeaponPool_ChargedScaling.charges` > 0; `Skill.unlockLevel` is a potion container's unlock level, 0
/// here) and add the record's character, skill, offensive modifier, retaliation, defense and racial attributes at
/// the skill's
/// level with its modifiers' (`AddModifier*`), not its offensive damage (no `CollectPassiveOffensiveDamage`
/// override); `CollectPassiveOffensiveModifierAttributes` sets the accumulator's
/// `IgnoreDamageMultiplier` around the modifiers' part, so a modifier's `offensiveDamageMultModifier`
/// (Savagery's transmuter) counts only in the skill's own attack. Returns the records filtered so (own record
/// first, at `li`), and the offensive modifier fields' values they add (the skill's own attack leaves those
/// out, [`crate::actor::Attacker::without_skill`]).
pub fn charged_scaling_passive(
    rec: &Record,
    li: usize,
    mods: &[(Record, usize)],
) -> (
    Vec<(Record, usize)>,
    std::collections::BTreeMap<String, f32>,
) {
    use crate::attributes::{AttrFamily as F, lookup};
    // A new record (not `Arc::make_mut`): it copies only the kept fields and gets its own key index.
    let keep = |r: &Record, own: bool| {
        let fields = r
            .fields
            .iter()
            .filter(|(k, _)| {
                let fam = lookup(k).map(|d| d.family);
                !matches!(
                    fam,
                    Some(
                        F::Offense
                            | F::OffenseBase
                            | F::OffenseDuration
                            | F::OffenseCrowdControl
                            | F::WeaponBase
                    )
                ) && k.as_ref() != "weaponDamagePct"
                    && (own || k.as_ref() != "offensiveDamageMultModifier")
            })
            .cloned()
            .collect();
        Record::new(r.name.clone(), r.class.clone(), fields)
    };
    let mut out = vec![(keep(rec, true), li)];
    out.extend(mods.iter().map(|(m, l)| (keep(m, false), *l)));
    let mut offensive = std::collections::BTreeMap::new();
    for (r, l) in &out {
        for (k, _) in r.fields.iter() {
            let fam = lookup(k).map(|d| d.family);
            if matches!(fam, Some(F::OffenseModifier | F::OffenseDurationModifier))
                || k.as_ref() == "offensiveDamageMultModifier"
            {
                let v = crate::stats::level_value(r, k, *l).unwrap_or(0.0);
                *offensive.entry(k.to_string()).or_insert(0.0) += v;
            }
        }
    }
    (out, offensive)
}

impl SkillBook {
    /// Learned modifiers of `skill` with their effective level indices (parent =
    /// [`SkillTrees::parent`] with the potion trees'). Call on a book with item bonuses.
    pub fn modifiers_of(
        &self,
        db: &Database,
        trees: &SkillTrees,
        skill: &str,
    ) -> Vec<(String, usize)> {
        self.skills
            .iter()
            .filter(|(s, l)| {
                *l > 0
                    && trees
                        .parent(s, true)
                        .is_some_and(|p| p.eq_ignore_ascii_case(skill))
            })
            .map(|(s, _)| (s.clone(), self.effective_level(db, s).max(1) as usize - 1))
            .collect()
    }

    /// The special animation a cast of `skill` plays
    /// (`Skill::StartSpecialAnimation`): the first learned
    /// modifier of it that names one (`skillSpecialAnimationName`), else the
    /// skill's own. Modifiers = [`SkillTrees::parents`].
    pub fn special_animation(
        &self,
        db: &Database,
        trees: &SkillTrees,
        skill: &str,
    ) -> Option<String> {
        let name = |r: &Record| r.str("skillSpecialAnimationName").map(str::to_owned);
        self.skills
            .iter()
            .filter(|(s, l)| {
                *l > 0
                    && trees
                        .parent(s, false)
                        .is_some_and(|p| p.eq_ignore_ascii_case(skill))
            })
            .find_map(|(s, _)| db.get(s).and_then(name))
            .or_else(|| db.get(skill).and_then(name))
    }
}

/// `CharAttributeStore_SkillPenalty` (`SkillProfile.penaltyStore`): the cost fields a
/// skill keeps on its caster, the energy reserve (`CharAttributeStore_SkillPenalty::Load`
/// reads exactly these two). `CharAttributeStore_Skill` (`SkillProfile.skillStore`,
/// what a `SkillBuff` gives) reads none of the `characterManaLimitReserve…`
/// fields.
pub const PENALTY_FIELDS: [&str; 2] = [
    "characterManaLimitReserve",
    "characterManaLimitReserveModifier",
];

/// The record with only its [`PENALTY_FIELDS`] (and its class / template).
pub fn penalty_only(r: Record) -> Record {
    let fields = r
        .fields
        .iter()
        .filter(|(k, _)| {
            PENALTY_FIELDS.iter().any(|f| k.eq_ignore_ascii_case(f))
                || k.eq_ignore_ascii_case("Class")
                || k.eq_ignore_ascii_case("templateName")
        })
        .cloned()
        .collect();
    Record::new(r.name, r.class, fields)
}

/// A radius-buff skill whose buff record sets `debufSkill` (the flag
/// `Skill_BuffRadius::Load` reads from the buff's table into `Skill_BuffRadius.debuff`).
pub fn is_debuff_aura(db: &Database, r: &Record) -> bool {
    matches!(
        r.class.as_str(),
        "Skill_BuffRadius" | "Skill_BuffRadiusToggled"
    ) && profile_name(r)
        .and_then(|b| db.get(b))
        .is_some_and(|b| b.bool("debufSkill"))
}

/// Dual-wield weapon mask of the character's skills, `(record, level)` of every skill list (learned,
/// item-granted, form-granted). `SkillManager::Update` ORs `Skill::GetQualifyingDualWeapons` of every
/// skill whose current level is not 0 (flags: [`crate::weapon_pool::DUAL_WIELD_FLAGS`]; a modifier is a learned skill of its
/// own here).
pub fn dual_mask<'a>(db: &Database, skills: impl IntoIterator<Item = (&'a str, u32)>) -> u32 {
    let mut m = 0;
    for (s, l) in skills {
        if l == 0 {
            continue;
        }
        let Some(r) = db.get(s) else { continue };
        for (k, bits) in crate::weapon_pool::DUAL_WIELD_FLAGS {
            if r.bool(k) {
                m |= bits;
            }
        }
    }
    m
}
