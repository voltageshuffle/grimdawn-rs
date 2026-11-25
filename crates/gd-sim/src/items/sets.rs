//! Item sets (`records/items/lootsets/*.dbr`, class `ItemSet`). A set record
//! lists `setMembers`; its bonus fields are per-count arrays indexed like any
//! per-level attribute: `min(count - 1, len - 1)` (the level is
//! `Character::GetNumberOfItemsInSet`, `ItemEquipment::AttachItem`, and
//! attributes read index level − 1 clamped, `CharAttributeMod::
//! AddToAccumulator`; e.g. `offensivePhysicalModifier 0;50;50;50;50` =
//! bonus from 2 pieces).
//! Set bonuses feed the item-set pass of `CharacterBio::UpdateResources`
//! (`ContributeMiscCharAttributes`).
//!
//! A set's skills (Game.dll): the set record holds an
//! `ItemSkillAugment` (`ItemSet.skillAugment`, loaded like an item's, `ItemSkillAugment::Load`):
//! `augmentSkillName/Level{n}`, `augmentMasteryName/Level{n}`,
//! `augmentAllLevel` (each per count, `ItemSkillAugment::ApplyAugments`), the granted
//! skill `itemSkillName` with `itemSkillLevel` (per count) and
//! `itemSkillAutoController`, and `itemSkillLevelEq` (the granted level;
//! evaluated once at load with the owner's variables, a parse failure or an
//! empty one = 1; stock sets: the constants 1, 2 or 3).
//! `ItemEquipment::AttachItem` / `DetachItem` run `ItemSet::
//! SkillAugmentInstall` / `SkillAugmentRemove`
//! with the new piece count: the previous count's skill bonuses come off
//! (`ItemSkillAugment::RemoveAugments`, `RemoveSkillLevelAugment`), the set's item skills go
//! (`SkillManager::RemoveItemSkills` with the set's id: an active one starts
//! its cooldown and stops), then the bonuses of the new count go on
//! (`ItemSkillAugment::ApplyAugments`) and `ItemSkillAugment::GrantItemSkills` grants the skill when
//! `itemSkillLevel[min(count − 1, len − 1)]` ≠ 0 (or the array is empty):
//! `SkillManager::AddItemSkills` with the set's id, so the skill is an item
//! skill like a worn item's (`gd_game::player` docs, item toggles). So
//! Mogdrogen's Tranquility (`itemset_c027`: `itemSkillLevel 0;0;0;1`) is
//! granted with all four pieces worn, at level 1, and goes when one comes
//! off. [`set_skills`], [`set_skill_bonuses`].
//! PORT: an equation with variables takes the set record's `itemLevel` as
//! `itemLevel` (the set's variable table, `ItemSet.variableTable`, is not traced; no stock
//! set uses one).
use super::db::ItemDb;
use super::resolve::{Attr, ResolvedItem, Source, record_attrs_at};
use gd_formats::arz::Record;
use gd_formats::save::item::Item;
use gd_formats::vfs::{Database, normalize};
use std::collections::BTreeMap;

/// The sets of the worn items with their piece counts (distinct members,
/// `Character::GetNumberOfItemsInSet`): (set record, lower-case; count),
/// in record order.
pub fn worn_sets(db: &Database, items: &[Item]) -> Vec<(String, u32)> {
    member_counts(items.iter().filter_map(|it| {
        let set = db.get(&it.base_name)?.str("itemSetName")?;
        Some((set, it.base_name.as_str()))
    }))
}

/// Distinct members per set (`Character::GetNumberOfItemsInSet`) from
/// (set record, member base record) pairs: (set record, lower-case; count),
/// in record order. An empty set name is no set.
fn member_counts<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<(String, u32)> {
    let mut counts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (set, base) in pairs.filter(|(set, _)| !set.is_empty()) {
        let base = normalize(base);
        let e = counts.entry(normalize(set)).or_default();
        if !e.contains(&base) {
            e.push(base);
        }
    }
    counts
        .into_iter()
        .map(|(s, m)| (s, m.len() as u32))
        .collect()
}

/// A skill a worn set grants (module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct SetSkill {
    pub set: String,
    pub skill: String,
    /// `itemSkillAutoController` (none: a hot slot / toggle skill).
    pub controller: Option<String>,
    pub level: u32,
}

/// The skills the worn sets grant at their piece counts (module docs).
pub fn set_skills(db: &Database, items: &[Item]) -> Vec<SetSkill> {
    let mut out = Vec::new();
    for (set, n) in worn_sets(db, items) {
        let Some(rec) = db.get(&set) else {
            continue;
        };
        let Some(skill) = rec.str("itemSkillName").filter(|s| !s.is_empty()) else {
            continue;
        };
        let gate = rec.f32s("itemSkillLevel");
        if !gate.is_empty() && gate[(n as usize - 1).min(gate.len() - 1)] == 0.0 {
            continue;
        }
        let item_level = rec.f32("itemLevel").unwrap_or(1.0);
        let level = rec
            .str("itemSkillLevelEq")
            .and_then(|e| gd_rules::expr::Expr::parse(e).ok())
            .map(|e| e.eval(&|v| v.eq_ignore_ascii_case("itemLevel").then_some(item_level)))
            .unwrap_or(1.0);
        out.push(SetSkill {
            set: set.clone(),
            skill: skill.to_owned(),
            controller: rec
                .str("itemSkillAutoController")
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            level: level.max(1.0) as u32,
        });
    }
    out
}

/// The set skills granted again on a piece-count change (module docs:
/// `SkillAugmentInstall` / `SkillAugmentRemove` remove the set's item skills
/// on every count change from a non-zero one, `ItemSkillAugment::GrantItemSkills` grants them
/// anew): the `granted` skills (now, [`set_skills`]) whose set was
/// worn `before` with another count. A set's skill that is granted at both
/// counts is a new item skill: an active one's cooldown started, then off
/// until its auto toggle (`gd_game::player` docs). Dormant in stock: every
/// stock set grants its skill only at its full count.
pub fn regranted_set_skills(
    before: &[(String, u32)],
    after: &[(String, u32)],
    granted: &[SetSkill],
) -> Vec<String> {
    let count = |v: &[(String, u32)], set: &str| v.iter().find(|x| x.0 == set).map_or(0, |x| x.1);
    granted
        .iter()
        .filter(|g| {
            let was = count(before, &g.set);
            was > 0 && was != count(after, &g.set)
        })
        .map(|g| g.skill.clone())
        .collect()
}

/// The worn sets' skill bonuses at their piece counts (module docs:
/// `augmentSkillLevel{n}` / `augmentMasteryLevel{n}` per count, index
/// `min(count − 1, len − 1)`): (skill bonuses, mastery bonuses), records
/// lower-case. E.g. Mogdrogen's Tranquility: +3 Briarthorn and Primal Bond
/// from 3 pieces, +1 Shaman from 4.
pub fn set_skill_bonuses(db: &Database, items: &[Item]) -> crate::skills::SkillBonuses {
    let (mut skills, mut masteries, mut all) = (Vec::new(), Vec::new(), 0);
    for (set, n) in worn_sets(db, items) {
        let Some(rec) = db.get(&set) else {
            continue;
        };
        let li = n as usize - 1;
        all += crate::stats::level_value(rec, "augmentAllLevel", li)
            .unwrap_or(0.0)
            .max(0.0) as u32;
        for k in 1..=5 {
            for (name, level, out) in [
                ("augmentSkillName", "augmentSkillLevel", &mut skills),
                ("augmentMasteryName", "augmentMasteryLevel", &mut masteries),
            ] {
                if let (Some(s), Some(l)) = (
                    rec.str(&format!("{name}{k}")).filter(|s| !s.is_empty()),
                    crate::stats::level_value(rec, &format!("{level}{k}"), li),
                ) && l > 0.0
                {
                    out.push((s.to_ascii_lowercase(), l as u32));
                }
            }
        }
    }
    (skills, masteries, all)
}

/// Number of bonus tiers a set's tooltip lists: `setSize`, or the
/// `setMembers` count when it is 0 (`ItemSet::LoadFromDatabase` reads it,
/// `ItemSet::GetUIDisplayText` falls back). Not the distinct piece count:
/// `setMembers` may list variants of one piece (Dagallon's Destruction,
/// `itemset_d116`: 6 entries, the two guns each with a `b` copy, `setSize`
/// 4); every entry gets its own member line, as in the game.
pub fn set_size(rec: &Record) -> usize {
    match rec.i32("setSize").unwrap_or(0) {
        n if n > 0 => n as usize,
        _ => rec.strs("setMembers").len(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActiveSet {
    pub record: String,
    pub name_tag: String,
    pub equipped: u32,
    /// Bonus attributes at the equipped count.
    pub bonus: Vec<Attr>,
}

/// Counts distinct equipped members per set and returns the active bonuses.
pub fn active_sets(db: &ItemDb, equipped: &[&ResolvedItem]) -> Vec<ActiveSet> {
    let pairs = equipped
        .iter()
        .filter_map(|it| Some((it.item_set.as_deref()?, it.item.base_name.as_str())));
    let mut out = Vec::new();
    for (set, n) in member_counts(pairs) {
        let Some(rec) = db.get(&set) else { continue };
        out.push(ActiveSet {
            record: set,
            name_tag: rec.str("setName").unwrap_or("").to_string(),
            equipped: n,
            bonus: record_attrs_at(rec, Source::Base, (n as usize).saturating_sub(1))
                .into_iter()
                .filter(|a| a.values.iter().any(|(_, v)| *v != 0.0))
                .collect(),
        });
    }
    out
}
