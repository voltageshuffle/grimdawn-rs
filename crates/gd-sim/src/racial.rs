//! Racial bonuses: % damage against, and % damage reduction from, characters of
//! a race (`racialBonusRace`, `racialBonusPercentDamage` /
//! `racialBonusAbsoluteDamage` / `racialBonusPercentDefense` /
//! `racialBonusAbsoluteDefense`, loaded by `RacialBonus::Load`).
//!
//! (Game.dll):
//! - A character's race is its record's `characterRacialProfile` list
//!   (`Character::Load`, read back by `Character::GetRacialProfile`); the player's pc record has
//!   one too (`Race009`, human). Races compare case-insensitively
//!   (`Game::ShareAnyName` → `Game::CompareNoCase`): a bonus applies when any of its races is
//!   one of the other character's.
//! - Attacker side, `Skill::CollectCombatParameters`: with a
//!   `Character` target, the attacking skill's own bonus at its level plus its
//!   modifiers' (`CollectLocalRacialBonusDamage` → `RacialBonus::GetValueAtLevel` /
//!   `AddModifierRacialBonusDamage`), then, unless the skill has
//!   `excludeRacialDamage` (`SkillProfile::IncludeRacialDamage`),
//!   `Character::ContributeRacialBonusDamage`: every worn
//!   non-weapon item and the attack's weapons (`Item::ContributeRacialBonusDamage`), and the
//!   skill manager's skills (`SkillManager::ContributeRacialBonusDamage`
//!   → `CollectPassiveRacialBonusDamage`: passives while enabled,
//!   toggles while on, buffs on the character, each with its modifiers, at
//!   their level). Everything adds up. The % total scales the attack:
//!   `DamageScaleInfo.scale` = (that scale, or 100 when unset) + %, which every
//!   damage attribute's `Process` multiplies in after the % modifiers
//!   ([`crate::combat::DamageScale`]), for the retaliation part of the attack
//!   too. The absolute total goes to `ParametersCombat.racialBonusAbsoluteDamage`, which
//!   `TakeAttack` only logs ("Racial Bonus Damage") and tallies in the play
//!   stats: it deals no damage.
//! - Defender side, `CombatManager::TakeAttack`:
//!   `CombatManager.racialBonusPercentDefense` is cleared for every attack, then `Character::
//!   ContributeRacialBonusDefense` adds the defender's equipped
//!   items' (every slot) and skills' bonuses against the attacker's races. After
//!   the shield block the % (`racialBonusPercentDefense`) cuts every damage
//!   (`ReduceAbsoluteDamage`, [`crate::combat::Defender::racial_reduction`]);
//!   the absolute (`CombatManager.racialBonusAbsoluteDefense`) is a pool for that attack that
//!   `CombatManager::ApplyDamage` soaks before the damage-absorbing buffs (not ported).
//! - Items (`ItemEquipment::InitializeItem` → `CreateItemRacialBonus`): the
//!   prefix, suffix, modifier and base records, one entry per race
//!   (`RacialBonus::SplitRaces` splits a record's race list, `RacialBonus::Merge` merges an entry
//!   into an existing one of the same race), each at its first value
//!   (`ItemEquipment::ContributeRacialBonusDamage`); a monster
//!   with two races listed by one item record gets the bonus twice. The
//!   inserted component (`ItemRelic`) and augment (`ItemEnchantment`) add theirs
//!   as one entry (their race list unsplit).
//!
//! PORT: the attack's weapons are the worn weapons (the ids `CollectCombatParameters`
//! is handed are not traced); the ascendant affix counts with the item (the
//! third contributor `ItemEquipment.thirdRacialBonus` is not traced); components and
//! augments at their first value. `PetPlayerScaling` pets attack with their owner's
//! bonuses. The absolute damage / defense values are not kept.
use crate::stats::level_value;
use gd_formats::arz::Record;
use gd_formats::save::item::Item;
use gd_formats::vfs::Database;

/// One racial bonus entry (`RacialBonus`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RacialBonus {
    /// `racialBonusRace`.
    pub races: Vec<String>,
    /// `racialBonusPercentDamage` (%).
    pub damage_pct: f32,
    /// `racialBonusPercentDefense` (%).
    pub defense_pct: f32,
}

impl RacialBonus {
    /// A record's bonus at a level index (`RacialBonus::GetValueAtLevel`: the value at
    /// min(level, count) − 1). `None` without races or values.
    pub fn from_record(rec: &Record, level_index: usize) -> Option<Self> {
        let races: Vec<String> = rec
            .strs_iter("racialBonusRace")
            .map(str::to_owned)
            .collect();
        if races.is_empty() {
            return None;
        }
        let v = |k: &str| level_value(rec, k, level_index).unwrap_or(0.0);
        let b = Self {
            races,
            damage_pct: v("racialBonusPercentDamage"),
            defense_pct: v("racialBonusPercentDefense"),
        };
        (b.damage_pct != 0.0 || b.defense_pct != 0.0).then_some(b)
    }

    /// The entry applies to a character of these races.
    pub fn matches(&self, races: &[String]) -> bool {
        self.races
            .iter()
            .any(|r| races.iter().any(|c| c.eq_ignore_ascii_case(r)))
    }

    /// One entry per race (`CreateItemRacialBonus`).
    fn split(self) -> Vec<Self> {
        let Self {
            races,
            damage_pct,
            defense_pct,
        } = self;
        races
            .into_iter()
            .map(|r| Self {
                races: vec![r],
                damage_pct,
                defense_pct,
            })
            .collect()
    }
}

/// A skill record's `racialBonusPercentDamage` at a level index against
/// `target` (0 when none of its races is one of the target's).
pub fn record_damage_against(rec: &Record, level_index: usize, target: &[String]) -> f32 {
    let hit = rec
        .strs_iter("racialBonusRace")
        .any(|r| target.iter().any(|t| t.eq_ignore_ascii_case(r)));
    if hit {
        level_value(rec, "racialBonusPercentDamage", level_index).unwrap_or(0.0)
    } else {
        0.0
    }
}

/// A character record's races (`characterRacialProfile`).
pub fn races_of(rec: &Record) -> Vec<String> {
    rec.strs_iter("characterRacialProfile")
        .map(str::to_owned)
        .collect()
}

/// The bonuses of skill records at their level index (passives, toggles,
/// buffs and their modifiers: the profile's skill sources).
pub fn of_skills<'a>(skills: impl IntoIterator<Item = (&'a Record, usize)>) -> Vec<RacialBonus> {
    skills
        .into_iter()
        .filter_map(|(r, li)| RacialBonus::from_record(r, li))
        .collect()
}

/// A worn item's bonuses (base, affixes, component, augment).
pub fn of_item(db: &Database, item: &Item) -> Vec<RacialBonus> {
    let mut out = Vec::new();
    let get = |n: &str| if n.is_empty() { None } else { db.get(n) };
    for name in [
        &item.prefix_name,
        &item.suffix_name,
        &item.modifier_name,
        &item.base_name,
        &item.ascendant_name,
    ] {
        if let Some(b) = get(name).and_then(|r| RacialBonus::from_record(r, 0)) {
            out.extend(b.split());
        }
    }
    for name in [&item.relic_name, &item.augment_name] {
        out.extend(get(name).and_then(|r| RacialBonus::from_record(r, 0)));
    }
    out
}

/// % damage of the matching entries against `target`.
pub fn damage_against<'a>(
    bonuses: impl IntoIterator<Item = &'a RacialBonus>,
    target: &[String],
) -> f32 {
    bonuses
        .into_iter()
        .filter(|b| b.matches(target))
        .fold(0.0, |p, b| p + b.damage_pct)
}

/// % damage reduction of the matching entries against `attacker`.
pub fn defense_against<'a>(
    bonuses: impl IntoIterator<Item = &'a RacialBonus>,
    attacker: &[String],
) -> f32 {
    bonuses
        .into_iter()
        .filter(|b| b.matches(attacker))
        .map(|b| b.defense_pct)
        .sum()
}
