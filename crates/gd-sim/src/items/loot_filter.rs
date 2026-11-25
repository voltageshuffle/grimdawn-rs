//! The loot filter: which ground items get a label.
//!
//! Game.dll `Item::PassLootFilter(ItemIgnore)` and the options
//! (`Player::GetLootFilter` / `SetLootFilter`: a 42-bit set, saved in the
//! character file `CharacterInfo` `loot_filters`; GrimDawn.exe loot filter
//! window `LootFilterWindow::Initialize` binds each checkbox to an option, [`OPTIONS`]).
//!
//! Classification `c` = `GetItemClassification(true)` (the base record's
//! raised by its affixes', [`super::Classification`]); Broken items never
//! pass. The [`Mode`] (GrimDawn.exe `UIWorldDescManager::ApplyLootFilter`): `ShowAll` while
//! "Show Items (No Filter)" is held, `HideCommon` while "Show Items (Filter
//! Common)" is held (everything but Common), `HideAll` after "Toggle Hide
//! All Items" (only Quest and Lore items), else `Filter`:
//! - Potions, scrolls, quest items, relics (`Artifact`), blueprints, lore
//!   and augments (classifications 6, 7, 9–15) always pass; components (8)
//!   pass with option 17.
//! - Quality: Common 0, Magic 1, Rare 2 (classification), Monster
//!   Infrequent 3 (the base record alone is Rare), Epic 4, Legendary 5,
//!   Ascended 39 (an ascendant affix), Sets 6 (part of a set). Always Show
//!   Uniques 7 (Epic / Legendary) and Always Show Double Rare 38 (Rare
//!   prefix and suffix) pass the item without the further checks.
//! - Type (one must match): 1h Melee 8 (axe, sword, mace, one-handed spear
//!   types 2–4, 7), 2h Melee 9, 1h Ranged 10, 2h Ranged 11, Dagger/Scepter
//!   12 (dagger, scepter, staff: 5, 6, 9), Caster Off-Hand 13, Shield 14,
//!   Armor 15 (head, chest, legs, feet, hands, shoulders), Accessories 16
//!   (amulet, ring, waist, medal). Others (non-equipment) never match.
//! - Damage / character (when any is set, one must match): Physical 18
//!   (`IsDamageTypePresent` physical / all damage), Pierce 19, Fire 20, Cold
//!   21, Lightning 22 (each also elemental), Acid 23, Vitality 24, Aether 25,
//!   Chaos 26, Bleed 27 (each also all damage); Pet Bonuses 28
//!   (`petBonusName`), My Masteries 29 / Other Masteries 30
//!   (`ItemEquipment::HasMastery`, [`Masteries`]), Speed 31 (total, run,
//!   attack, cast speed), Cooldown Reduction 32 (skill attribute 1), Crit
//!   Damage 33 (damage type 59), Offensive Ability 34 (char 21, 22),
//!   Defensive Ability 35 (char 23, 24), Health 40 (char 4), Health
//!   Regeneration 41 (char 6), Resistances 36 (defense types 2, 4–11, 14–16,
//!   42–47, 57, 58), Retaliation 37 (any retaliation).
//!
//! The attribute checks read the item's own stores (base and affixes; a
//! component or augment is its own item). PORT: an attribute counts by its
//! catalog type ([`crate::attributes`]); the mastery of an
//! `augmentSkillName` is its `records/skills/playerclassNN/` folder (the
//! engine looks the skill up in the player's skill manager and reads its
//! parent mastery's level).
use super::Classification;
use super::resolve::{ResolvedItem, Source, StoreKind};

pub const COUNT: usize = 42;

/// The window's checkboxes: (option, label tag), per column in the window's
/// order (`LootFilterWindow::Initialize`; the Ascended box only with the third
/// expansion; `None`: a spacer line).
pub const OPTIONS: [&[Option<(usize, &str)>]; 4] = [
    &[
        Some((0, "tagLootFilter01")),
        Some((1, "tagLootFilter02")),
        Some((2, "tagLootFilter03")),
        Some((3, "tagLootFilter04")),
        Some((39, "tagLootFilter40")),
        Some((4, "tagLootFilter05")),
        Some((5, "tagLootFilter06")),
        Some((6, "tagLootFilter07")),
        None,
        Some((7, "tagLootFilter08")),
        Some((38, "tagLootFilter39")),
    ],
    &[
        Some((8, "tagLootFilter09")),
        Some((9, "tagLootFilter10")),
        Some((10, "tagLootFilter11")),
        Some((11, "tagLootFilter12")),
        Some((12, "tagLootFilter13")),
        Some((13, "tagLootFilter14")),
        Some((14, "tagLootFilter15")),
        None,
        Some((15, "tagLootFilter16")),
        None,
        Some((16, "tagLootFilter17")),
        Some((17, "tagLootFilter38")),
    ],
    &[
        Some((18, "tagLootFilter18")),
        Some((19, "tagLootFilter19")),
        Some((20, "tagLootFilter20")),
        Some((21, "tagLootFilter21")),
        Some((22, "tagLootFilter22")),
        Some((23, "tagLootFilter23")),
        Some((24, "tagLootFilter24")),
        Some((25, "tagLootFilter25")),
        Some((26, "tagLootFilter26")),
        Some((27, "tagLootFilter27")),
        None,
        Some((28, "tagLootFilter28")),
    ],
    &[
        Some((29, "tagLootFilter29")),
        Some((30, "tagLootFilter30")),
        None,
        Some((31, "tagLootFilter31")),
        Some((32, "tagLootFilter32")),
        Some((33, "tagLootFilter33")),
        Some((34, "tagLootFilter34")),
        Some((35, "tagLootFilter35")),
        None,
        Some((40, "tagLootFilter41")),
        Some((41, "tagLootFilter42")),
        Some((36, "tagLootFilter36")),
        None,
        Some((37, "tagLootFilter37")),
    ],
];

/// A new character's options: 0–17 and 39 on (the real game's
/// new character file; the port's template keeps them in `gd-game` `save.rs`).
pub fn defaults() -> [bool; COUNT] {
    std::array::from_fn(|i| i <= 17 || i == 39)
}

/// The pick filter mode (`InGameUI::GetPickFilterMode`, passed to `Item::PassLootFilter`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Filter,
    ShowAll,
    HideCommon,
    HideAll,
}

/// The player's masteries (record path, lower case) and whether each has
/// points.
pub struct Masteries<'a>(pub &'a [(String, bool)]);

impl Masteries<'_> {
    fn level_of(&self, mastery: &str) -> Option<bool> {
        self.0
            .iter()
            .find(|(m, _)| m.eq_ignore_ascii_case(mastery))
            .map(|(_, l)| *l)
    }

    /// `playerclassNN` of a skill or mastery record.
    fn class_dir(rec: &str) -> Option<String> {
        let low = rec.to_ascii_lowercase();
        let i = low.find("playerclass")?;
        Some(low[i..].split('/').next()?.to_owned())
    }

    fn owns_class(&self, rec: &str) -> bool {
        let Some(c) = Self::class_dir(rec) else {
            return false;
        };
        self.0
            .iter()
            .any(|(m, l)| *l && Self::class_dir(m).as_deref() == Some(c.as_str()))
    }
}

/// What the filter needs of an item besides its resolved stats.
pub struct ItemFacts<'a> {
    pub resolved: &'a ResolvedItem,
    /// `petBonusName` on the base or an affix.
    pub pet_bonus: bool,
    /// `augmentSkillName{n}`, `augmentMasteryName{n}`, `augmentAllLevel` of
    /// the item (`gd_sim::skills::item_skill_bonuses`).
    pub skills: &'a crate::skills::SkillBonuses,
}

fn types<'a>(item: &'a ItemFacts, store: StoreKind) -> impl Iterator<Item = u8> + 'a {
    item.resolved
        .attrs
        .iter()
        .filter(move |a| {
            a.store == store
                && !matches!(
                    a.source,
                    Source::Relic | Source::RelicBonus | Source::Augment
                )
        })
        .filter_map(|a| {
            crate::attributes::class_def(a.class)
                .or_else(|| {
                    a.values
                        .first()
                        .and_then(|(f, _)| crate::attributes::lookup(f))
                })
                .map(|d| d.ty)
        })
}

/// The filter's Armor group (option 15): head, chest, legs, feet, hands, shoulders
/// (`gd_sim::items::illusion::item_type` 1, 3, 4, 5, 7, 14).
pub fn is_armor(item_type: u32) -> bool {
    matches!(item_type, 1 | 3 | 4 | 5 | 7 | 14)
}

/// The filter's Accessories group (option 16): amulet, ring, waist, medal (item types 2, 6, 13, 15).
pub fn is_accessory(item_type: u32) -> bool {
    matches!(item_type, 2 | 6 | 13 | 15)
}

/// `Item::PassLootFilter`.
pub fn passes(f: &[bool; COUNT], mode: Mode, item: &ItemFacts, masteries: &Masteries) -> bool {
    let r = item.resolved;
    let c = r.classification as u32;
    if r.classification == Classification::Broken {
        return false;
    }
    match mode {
        Mode::HideCommon => return c != 0,
        Mode::ShowAll => return true,
        Mode::HideAll => {
            return matches!(
                r.classification,
                Classification::Quest | Classification::Lore
            );
        }
        Mode::Filter => {}
    }
    if (6..=7).contains(&c) || (9..=15).contains(&c) {
        return true;
    }
    if c == 8 {
        return f[17];
    }
    let mut quality = (f[0] && c == 0)
        || (f[1] && c == 1)
        || (f[2] && c == 2)
        || (f[4] && c == 3)
        || (f[5] && c == 4);
    quality |= f[3] && r.base_classification == Classification::Rare;
    quality |= f[39] && !r.item.ascendant_name.is_empty();
    quality |= f[6] && r.item_set.is_some();
    let unique = f[7] && (3..=4).contains(&c);
    let double_rare = f[38]
        && r.prefix_classification == Some(Classification::Rare)
        && r.suffix_classification == Some(Classification::Rare);
    if unique || double_rare {
        return true;
    }
    if !quality {
        return false;
    }
    let w = super::illusion::weapon_type(&r.class);
    let ty = super::illusion::item_type(&r.class);
    let two_handed_melee = r.class.starts_with("WeaponMelee") && r.class.ends_with("2h");
    let kind = (f[8] && (matches!(w, 2..=4) || w == 7))
        || (f[9] && two_handed_melee)
        || (f[10] && w == 13)
        || (f[11] && w == 8)
        || (f[12] && matches!(w, 5 | 6 | 9))
        || (f[13] && w == 12)
        || (f[14] && w == 11)
        || (f[15] && is_armor(ty))
        || (f[16] && is_accessory(ty));
    if !kind {
        return false;
    }
    let damage = |t: &[u8]| types(item, StoreKind::Damage).any(|x| t.contains(&x));
    let chars = |t: &[u8]| types(item, StoreKind::Char).any(|x| t.contains(&x));
    let (sk, ms, all) = item.skills;
    let mastery = |mine: bool| {
        *all > 0
            || ms
                .iter()
                .any(|(m, _)| masteries.level_of(m).unwrap_or(false) == mine)
            || (mine && sk.iter().any(|(s, _)| masteries.owns_class(s)))
    };
    let checks: [(usize, &dyn Fn() -> bool); 22] = [
        (18, &|| damage(&[2, 60])),
        (19, &|| damage(&[4, 3, 60])),
        (20, &|| damage(&[6, 58, 60])),
        (21, &|| damage(&[5, 58, 60])),
        (22, &|| damage(&[8, 58, 60])),
        (23, &|| damage(&[7, 60])),
        (24, &|| damage(&[9, 60])),
        (25, &|| damage(&[11, 60])),
        (26, &|| damage(&[10, 60])),
        (27, &|| damage(&[15, 60])),
        (28, &|| item.pet_bonus),
        (29, &|| mastery(true)),
        (30, &|| mastery(false)),
        (40, &|| chars(&[4])),
        (41, &|| chars(&[6])),
        (31, &|| chars(&[13, 14, 15, 16])),
        (32, &|| types(item, StoreKind::Skill).any(|x| x == 1)),
        (33, &|| damage(&[59])),
        (34, &|| chars(&[21, 22])),
        (35, &|| chars(&[23, 24])),
        (36, &|| {
            types(item, StoreKind::Defense).any(|x| {
                [
                    2, 4, 5, 6, 7, 8, 9, 10, 11, 14, 42, 43, 44, 45, 46, 47, 57, 16, 15, 58,
                ]
                .contains(&x)
            })
        }),
        (37, &|| types(item, StoreKind::Retaliation).next().is_some()),
    ];
    let mut asked = false;
    for (opt, test) in &checks {
        if f[*opt] {
            asked = true;
            if test() {
                return true;
            }
        }
    }
    !asked
}
