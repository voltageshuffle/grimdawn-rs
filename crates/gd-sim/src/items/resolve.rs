//! Item instance → attribute values, exactly as `ItemEquipment::InitializeItem`
//! (Game.dll) builds them from the base / prefix / suffix /
//! modifier records and the saved seed.
//!
//! (decompiled): store order (char, char penalty reduction, skill,
//! damage, retaliation, defense, conversion), creator order inside every store
//! ([`super::order::EQUIPMENT_ORDER`], generated), the RNG stream
//! ([`super::jitter::item_stream`]), which values each class jitters and with
//! which percentage ([`JITTER_CAP`]; the skill base values roll last, see
//! `build_attrs`), the (min, max-min) storage of damage ranges, the damage
//! scale by `lootRandomizerScale + attributeScalePercent`, requirement
//! precedence (record field, then equation, level = max with the affixes').
//! The equation variables ([`equation_var`]) follow the
//! cost-info gathering in Game.dll. The same loaders build the `_Min` /
//! `_Max` store sets of the details view ([`attr_bounds`], [`Roll::Bound`]).
use super::db::{Fields, ItemDb};
use super::jitter::{Roll, item_stream, scale};
use super::order::EQUIPMENT_ORDER;
use crate::attributes::{AttrFamily, FieldDef, class_def};
use crate::catalog::FIELDS;
use crate::stats::CharAttrAccumulator;
use gd_formats::arz::{RawValue, Record};
use gd_formats::save::item::Item;
use gd_rules::expr::Expr;
use std::collections::BTreeMap;

/// Attribute store of `ItemEquipment` (creation order = this order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StoreKind {
    Char,
    Skill,
    Damage,
    Retaliation,
    Defense,
    Conversion,
}

/// One creator call of a store's `Load` (see `order.rs`).
#[derive(Debug, Clone, Copy)]
pub enum Creator {
    /// `CharAttributeVal_BaseAttackSpeed`: weapons only, modifier table only, never jittered.
    BaseAttackSpeed,
    /// A `CharAttribute*` (one field). `jitter`: the store passed 20%.
    Char { field: &'static str, jitter: bool },
    /// A `SkillAttribute*`: affixes jitter on load (`SkillAttribute::Jitter`), the base
    /// array through the store's `AddJitter(20)` (see `build_attrs`).
    Skill { field: &'static str },
    /// A damage / retaliation / defense class (fields from [`FIELDS`]).
    Class { class: &'static str, jitter: bool },
    /// `DamageAttribute_BasePhysical::Create_BasePhysical`: base record only, no jitter.
    BasePhysical,
    /// `DefenseAttribute_BaseProtectionAbsorption` (armor).
    Armor,
    /// `ConversionAttributeStore` (two conversions per source).
    Conversion,
    /// Only when the item is a weapon (`ItemEquipment.isArmor == 0`).
    WeaponOnly(&'static Creator),
    /// Only for armor/jewelry (`Armor` ctor sets `isArmor = 1`).
    NonWeaponOnly(&'static Creator),
}

/// Base percentage the stores pass (`0x41A00000`).
pub const STORE_JITTER: f32 = 20.0;

/// The cap every attribute class's `AddJitter` / `MinJitter` / `MaxJitter`
/// puts on its percentage (`fminf(fmaxf(pct, 0), 50)`, `Game::JitterCapPercent`,
/// in each class's `AddJitter`). The loaders that call the plain
/// `Jitter` directly do not clamp: `CharAttribute::LoadPrefixTable`
/// and `SkillAttribute::LoadPrefixTable` (affix
/// percentages of the character and skill classes) and the conversion store.
/// The only stock affix above it is `ao009b_energydrain_01` (55 %,
/// `offensiveManaBurnDrainMin` 6: the span is 3 either way).
pub const JITTER_CAP: f32 = 50.0;

/// Which table an attribute instance came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Base,
    Prefix,
    Suffix,
    Modifier,
    Relic,
    RelicBonus,
    Augment,
    /// The ascendant affix (`ItemEquipment::AddAscendantBonus`).
    Ascendant,
}

/// One attribute object after loading (and merging).
#[derive(Debug, Clone, PartialEq)]
pub struct Attr {
    pub store: StoreKind,
    pub class: &'static str,
    pub source: Source,
    /// Final field values (`min`/`max` for ranges; `max = min + range`).
    pub values: Vec<(String, f32)>,
    /// Set for a damage attribute flagged `<stem>Global`: it joins its
    /// store's global-chance group instead of counting on its own
    /// (`actor::item_global_groups`).
    pub global: Option<Global>,
}

/// A global-chance damage attribute (`DamageAttribute::HasGlobalChance`):
/// the store's global chance and whether `<stem>XOR` puts it in the
/// "one of" list (`DamageAttributeDur::AddToStore` and siblings →
/// `AddGlobalXorAttribute`, else `AddGlobalAndAttribute`). The
/// store's chance is `offensiveGlobalChance` of the base table, replaced by
/// each of prefix, suffix, modifier that has one above 0
/// (`DamageAttributeStore::SetGlobalChance` / `GetGlobalChance`
/// at level 1); a component / augment record has its
/// own store and chance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Global {
    pub chance: f32,
    pub xor: bool,
}

impl Attr {
    pub fn get(&self, field: &str) -> Option<f32> {
        self.values
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(field))
            .map(|(_, v)| *v)
    }
}

/// `GameEngine::ResolveItemClassificationEnum` (names/values for 0..=13; 7 and 12 unused). 14 has no
/// name there: only `ItemEnchantment::Load` sets it (augments), whatever the record says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Classification {
    Common = 0,
    Magical = 1,
    Rare = 2,
    Epic = 3,
    Legendary = 4,
    Broken = 5,
    Potion = 6,
    Relic = 8,
    Quest = 9,
    Artifact = 10,
    ArtifactFormula = 11,
    Lore = 13,
    Enchantment = 14,
}

impl Classification {
    pub fn parse(s: &str) -> Classification {
        match s {
            "Magical" => Self::Magical,
            "Rare" => Self::Rare,
            "Epic" => Self::Epic,
            "Legendary" => Self::Legendary,
            "Broken" => Self::Broken,
            "Potion" => Self::Potion,
            "Relic" => Self::Relic,
            "Quest" => Self::Quest,
            "ArtifactFormula" => Self::ArtifactFormula,
            "Artifact" => Self::Artifact,
            "Lore" => Self::Lore,
            _ => Self::Common,
        }
    }

    /// Display colour class (`GameEngine::GetItemColorText` index): the
    /// item's rarity `self`, or 16/17 for "double rare" (both affixes Rare;
    /// 17 when the base record itself is Rare).
    /// (`Item::GetDropClassification`).
    pub fn color_index(
        self,
        base: Classification,
        prefix: Option<Classification>,
        suffix: Option<Classification>,
    ) -> u32 {
        if prefix == Some(Self::Rare) && suffix == Some(Self::Rare) {
            16 + (base == Self::Rare) as u32
        } else {
            self as u32
        }
    }

    /// Approximate RGB of the rarity (UI styles `ItemName*`); placeholder
    /// colours for the 3D label until the UI styles are ported.
    pub fn rgb(self) -> [f32; 3] {
        match self {
            Self::Common => [1.0, 1.0, 1.0],
            Self::Magical => [1.0, 0.85, 0.2],
            Self::Rare => [0.3, 1.0, 0.3],
            Self::Epic => [0.25, 0.6, 1.0],
            Self::Legendary => [0.7, 0.3, 1.0],
            Self::Broken => [0.6, 0.6, 0.6],
            Self::Relic => [0.0, 0.9, 0.9],
            Self::Quest => [0.9, 0.5, 0.9],
            Self::Potion
            | Self::ArtifactFormula
            | Self::Artifact
            | Self::Lore
            | Self::Enchantment => [0.9, 0.8, 0.6],
        }
    }
}

/// Tags composing the display name (`tagItemNameOrder`:
/// prefix, quality, style, base, suffix); see [`super::naming`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NameTags {
    pub prefix: String,
    pub quality: String,
    pub style: String,
    pub base: String,
    pub suffix: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Requirements {
    pub level: u32,
    pub physique: u32,
    pub cunning: u32,
    pub spirit: u32,
}

/// A fully resolved item instance.
#[derive(Debug, Clone)]
pub struct ResolvedItem {
    pub item: Item,
    /// Record class (`ArmorProtective_Head`, `WeaponMelee_Sword`, `ItemRelic`, ...).
    pub class: String,
    pub is_weapon: bool,
    /// The item's classification: the highest of base and affixes (a
    /// component / relic / blueprint / augment class forces its own).
    pub classification: Classification,
    /// The base record's own `itemClassification` (double-rare 16 vs 17,
    /// the monster-infrequent symbol, the loot filter's Rare check).
    pub base_classification: Classification,
    pub prefix_classification: Option<Classification>,
    pub suffix_classification: Option<Classification>,
    pub item_level: u32,
    pub requirements: Requirements,
    pub name: NameTags,
    pub attrs: Vec<Attr>,
    pub item_set: Option<String>,
    pub bitmap: String,
    /// [`max_stack`]: the record's `maxStackSize` else the engine default; 1 for the non-stacking classes.
    pub max_stack: u32,
    /// RNG state after all stores (for tests / debugging).
    pub rng_end: u32,
}

/// Item classes that use `ItemEquipment::InitializeItem`.
pub fn is_equipment_class(class: &str) -> bool {
    class.starts_with("Armor") || class.starts_with("Weapon")
}

pub fn is_weapon_class(class: &str) -> bool {
    class.starts_with("Weapon")
}

/// The game's item settings record (`itemMaxStackSize`).
pub const GAME_ITEM_INFO: &str = "records/game/gameiteminfo.dbr";

/// `GameEngine::GetItemMaxStackSize`: `itemMaxStackSize` of [`GAME_ITEM_INFO`] (1000 in stock).
pub fn engine_max_stack(info: Option<&Record>) -> u32 {
    info.and_then(|r| r.i32("itemMaxStackSize"))
        .filter(|m| *m > 0)
        .unwrap_or(1000) as u32
}

/// `Item::GetMaxStackSize`: the record's `maxStackSize` (`Item::Load`, `Item.maxStackSize`) when it is not 0,
/// else `engine` ([`engine_max_stack`]). Equipment, relics (`ItemArtifact`) and the formula classes never stack
/// (their `IncrementStack` slot returns false); components and charms (`ItemRelic` / `ItemCharm`) stack only when
/// complete ([`super::components::is_complete`]), else 1. The one source of the rule (`gd_game::stack` delegates).
pub fn max_stack(rec: &Record, item: &Item, engine: u32) -> u32 {
    let stacks = match rec.class.as_str() {
        c if is_equipment_class(c) => false,
        "ItemArtifact"
        | "ItemArtifactFormula"
        | "ItemRerollFormula"
        | "ItemRandomSetFormula"
        | "ItemSetFormula"
        | "ItemAscensionFormula"
        | "ItemTransmuter" => false,
        "ItemRelic" | "ItemCharm" => super::components::is_complete(rec, item.var1),
        _ => true,
    };
    if !stacks {
        return 1;
    }
    match rec.i32("maxStackSize").unwrap_or(0) {
        0 => engine,
        n => n.max(1) as u32,
    }
}

/// Equation name prefix chosen by each item class constructor (from
/// the `*StrengthEquation` string references).
pub fn requirement_prefix(class: &str) -> Option<&'static str> {
    Some(match class {
        "ArmorProtective_Head" => "head",
        "ArmorProtective_Chest" => "chest",
        "ArmorProtective_Legs" => "legs",
        "ArmorProtective_Feet" => "feet",
        "ArmorProtective_Hands" => "hands",
        "ArmorProtective_Waist" => "waist",
        "ArmorProtective_Shoulders" => "shoulders",
        "ArmorJewelry_Ring" => "ring",
        "ArmorJewelry_Bracelet" => "bracelet",
        "ArmorJewelry_Amulet" => "amulet",
        "ArmorJewelry_Medal" => "medal",
        "WeaponArmor_Shield" => "shield",
        "WeaponArmor_Offhand" => "offhand",
        "WeaponHunting_Spear" => "spear",
        "WeaponHunting_Ranged1h" => "ranged1h",
        "WeaponHunting_Ranged2h" => "ranged2h",
        "WeaponMagical_Staff" => "staff",
        "WeaponMelee_Sword" => "sword",
        "WeaponMelee_Mace" => "mace",
        "WeaponMelee_Axe" => "axe",
        "WeaponMelee_Scepter" => "scepter",
        "WeaponMelee_Dagger" => "dagger",
        "WeaponMelee_Sword2h"
        | "WeaponMelee_Mace2h"
        | "WeaponMelee_Axe2h"
        | "WeaponMelee_Spear2h" => "melee2h",
        _ => return None,
    })
}

struct Tables<'a> {
    base: &'a Record,
    affixes: [Option<&'a Record>; 3],
}

const AFFIX_SOURCES: [Source; 3] = [Source::Prefix, Source::Suffix, Source::Modifier];

/// Damage-attribute storage shapes (from each `LoadFromTable`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum DmgKind {
    /// `DamageAttributeAbs*` / `Reflex_*`: (min, max-min) per level.
    Abs,
    /// `DamageAttributeAbsBase_*`: like Abs, `AddJitter`/`ScaleAttribute` are no-ops.
    AbsBase,
    /// `DamageAttributeDur*`: (min, max-min) damage + (min, max-min) duration; only damage jitters.
    Dur,
    /// `DamageAttributeAbsMod*`: modifier per level.
    AbsMod,
    /// `DamageAttributeDurMod*`: modifier + duration modifier.
    DurMod,
    /// `DamageAttribute_Physical` (non-weapons): pairs + `offensivePierceRatio`.
    Physical,
}

fn dmg_kind(class: &str) -> DmgKind {
    if class == "DamageAttribute_Physical" {
        DmgKind::Physical
    } else if class.contains("AbsMod_") {
        DmgKind::AbsMod
    } else if class.contains("DurMod_") {
        DmgKind::DurMod
    } else if class.contains("AbsBase_") {
        DmgKind::AbsBase
    } else if class.contains("Dur_") {
        DmgKind::Dur
    } else {
        DmgKind::Abs
    }
}

#[derive(Debug, Clone)]
struct DmgInst {
    kind: DmgKind,
    pairs: Vec<(f32, f32)>,
    pairs2: Vec<(f32, f32)>,
    mods: Vec<f32>,
    mods2: Vec<f32>,
    chance: f32,
    global: bool,
    /// `<stem>XOR`: the global attribute is a "one of".
    xor: bool,
    source: Source,
}

/// `(min, max)` arrays → `(min, max(max - min, 0))`, reusing the last max
/// when the max array is shorter (loop in `DamageAttributeAbs::LoadFromTable`).
fn ranges(min: &[f32], max: &[f32]) -> Vec<(f32, f32)> {
    let mut cur = min.first().copied().unwrap_or(0.0);
    min.iter()
        .enumerate()
        .map(|(i, &lo)| {
            if i < max.len() {
                cur = max[i];
            }
            (lo, (cur - lo).max(0.0))
        })
        .collect()
}

/// The stem of an attribute's `<stem>Global` / `<stem>XOR` tags, from its chance field.
fn global_stem(chance: &str) -> &str {
    chance
        .strip_suffix("ModifierChance")
        .or_else(|| chance.strip_suffix("Chance"))
        .unwrap_or(chance)
}

fn load_dmg(def: &FieldDef, kind: DmgKind, t: &Record, source: Source) -> Option<DmgInst> {
    let v = |f: Option<&str>| f.map(|f| t.values(f)).unwrap_or_default();
    let chance = def
        .chance
        .map(|c| t.value_at(c, 0).unwrap_or(0.0))
        .unwrap_or(0.0);
    let tag = |sfx: &str| {
        def.chance
            .is_some_and(|c| t.float(&format!("{}{sfx}", global_stem(c))) != 0.0)
    };
    let (global, xor) = (tag("Global"), tag("XOR"));
    let mut inst = DmgInst {
        kind,
        pairs: Vec::new(),
        pairs2: Vec::new(),
        mods: Vec::new(),
        mods2: Vec::new(),
        chance,
        global,
        xor,
        source,
    };
    match kind {
        DmgKind::Abs | DmgKind::AbsBase => {
            inst.pairs = ranges(&v(def.min), &v(def.max));
            (!inst.pairs.is_empty()).then_some(inst)
        }
        DmgKind::Physical => {
            inst.mods = t.raw_values("offensivePierceRatio");
            inst.pairs = ranges(
                &v(Some("offensivePhysicalMin")),
                &v(Some("offensivePhysicalMax")),
            );
            (!inst.pairs.is_empty()).then_some(inst)
        }
        DmgKind::Dur => {
            inst.pairs = ranges(&v(def.min), &v(def.max));
            inst.pairs2 = ranges(&v(def.duration_min), &v(def.duration_max));
            (!inst.pairs.is_empty() && !inst.pairs2.is_empty()).then_some(inst)
        }
        DmgKind::AbsMod => {
            inst.mods = v(def.modifier);
            (!inst.mods.is_empty()).then_some(inst)
        }
        DmgKind::DurMod => {
            inst.mods = v(def.modifier);
            inst.mods2 = v(def.duration_modifier);
            (!inst.mods.is_empty() || !inst.mods2.is_empty()).then_some(inst)
        }
    }
}

impl DmgInst {
    /// `AddJitter(pct, rng)` of each class (`DamageAttributeAbs` /
    /// `…Dur`, `…AbsMod`, `…DurMod`,
    /// `DamageAttribute_Physical`): nothing unless pct > 0, then
    /// the percentage clamped to [0, 50] (the base's store 20 % and the
    /// affixes' `lootRandomizerJitter` alike: the creators pass the affix value
    /// to this virtual, e.g. `DamageAttributeStore::CreateAttribute`). The `MinJitter` / `MaxJitter`
    /// siblings clamp the same way.
    fn add_jitter(&mut self, pct: f32, rng: &mut Roll) {
        if pct <= 0.0 {
            return;
        }
        let pct = pct.clamp(0.0, JITTER_CAP);
        let j = |v: &mut f32, rng: &mut Roll| *v = rng.int(*v, pct, false);
        match self.kind {
            DmgKind::AbsBase => {}
            DmgKind::Abs | DmgKind::Dur => {
                for (a, b) in &mut self.pairs {
                    j(a, rng);
                    j(b, rng);
                }
            }
            DmgKind::Physical => {
                for (a, b) in &mut self.pairs {
                    j(a, rng);
                    j(b, rng);
                }
                for m in &mut self.mods {
                    j(m, rng);
                }
            }
            DmgKind::AbsMod => self.mods.iter_mut().for_each(|m| j(m, rng)),
            DmgKind::DurMod => {
                self.mods.iter_mut().for_each(|m| j(m, rng));
                self.mods2.iter_mut().for_each(|m| j(m, rng));
            }
        }
    }

    /// `MergeDamage` of each class (`DamageAttributeAbs`, `…AbsMod`,
    /// `…Dur`, `…DurMod`, `DamageAttribute_Physical` ::MergeDamage): never for
    /// a global one, and only at an equal chance. Abs / Physical: one
    /// (min, range) entry each, added (Physical also adds the other's pierce
    /// ratio when it has exactly one). AbsMod: one modifier each, added. Dur:
    /// one damage entry each and identical duration arrays (one entry); only
    /// the damage adds. DurMod: per array, one entry each adds, an empty one
    /// takes the other's, both empty is fine, anything else refuses.
    fn merge(&mut self, o: &DmgInst) -> bool {
        if self.global || self.chance != o.chance {
            return false;
        }
        let add_pair = |a: &mut (f32, f32), b: (f32, f32)| {
            a.0 += b.0;
            a.1 += b.1;
        };
        match self.kind {
            DmgKind::Abs | DmgKind::AbsBase | DmgKind::Physical => {
                if self.pairs.len() != 1 || o.pairs.len() != 1 {
                    return false;
                }
                add_pair(&mut self.pairs[0], o.pairs[0]);
                if self.kind == DmgKind::Physical && self.mods.len() == 1 {
                    self.mods[0] += o.mods.first().copied().unwrap_or(0.0);
                }
                true
            }
            DmgKind::AbsMod => {
                if self.mods.len() != 1 || o.mods.len() != 1 {
                    return false;
                }
                self.mods[0] += o.mods[0];
                true
            }
            DmgKind::Dur => {
                if self.pairs.len() != 1
                    || o.pairs.len() != 1
                    || self.pairs2.len() != 1
                    || self.pairs2 != o.pairs2
                {
                    return false;
                }
                add_pair(&mut self.pairs[0], o.pairs[0]);
                true
            }
            DmgKind::DurMod => {
                let ok = |a: &[f32], b: &[f32]| (a.len() == 1 && b.len() == 1) || a.is_empty();
                if !ok(&self.mods, &o.mods) || !ok(&self.mods2, &o.mods2) {
                    return false;
                }
                let add = |a: &mut Vec<f32>, b: &[f32]| {
                    if a.len() == 1 && b.len() == 1 {
                        a[0] += b[0];
                    } else if a.is_empty() {
                        a.extend_from_slice(b);
                    }
                };
                add(&mut self.mods, &o.mods);
                add(&mut self.mods2, &o.mods2);
                true
            }
        }
    }

    fn scale(&mut self, pct: f32) {
        if pct == 0.0 {
            return;
        }
        match self.kind {
            DmgKind::AbsBase => {}
            DmgKind::Abs | DmgKind::Dur | DmgKind::Physical => {
                self.pairs.iter_mut().for_each(|p| p.0 = scale(p.0, pct))
            }
            DmgKind::AbsMod => self.mods.iter_mut().for_each(|m| *m = scale(*m, pct)),
            DmgKind::DurMod => {
                self.mods.iter_mut().for_each(|m| *m = scale(*m, pct));
                self.mods2.iter_mut().for_each(|m| *m = scale(*m, pct));
            }
        }
    }

    fn to_attr(
        &self,
        store: StoreKind,
        class: &'static str,
        def: &FieldDef,
        global_chance: Option<f32>,
    ) -> Attr {
        let mut values = Vec::new();
        let mut push = |f: Option<&str>, v: Option<f32>| {
            if let (Some(f), Some(v)) = (f, v) {
                values.push((f.to_string(), v));
            }
        };
        let (min_f, max_f) = if self.kind == DmgKind::Physical {
            (Some("offensivePhysicalMin"), Some("offensivePhysicalMax"))
        } else {
            (def.min, def.max)
        };
        push(min_f, self.pairs.first().map(|p| p.0));
        push(max_f, self.pairs.first().map(|p| p.0 + p.1));
        push(def.duration_min, self.pairs2.first().map(|p| p.0));
        push(def.duration_max, self.pairs2.first().map(|p| p.0 + p.1));
        match self.kind {
            DmgKind::Physical => push(Some("offensivePierceRatio"), self.mods.first().copied()),
            _ => push(def.modifier, self.mods.first().copied()),
        }
        push(def.duration_modifier, self.mods2.first().copied());
        if self.chance != 0.0 {
            push(def.chance, Some(self.chance));
        }
        Attr {
            store,
            class,
            source: self.source,
            values,
            global: global_chance.filter(|_| self.global).map(|chance| Global {
                chance,
                xor: self.xor,
            }),
        }
    }
}

/// Loads, jitters and merges one damage/retaliation class (the creator
/// template of `DamageAttributeStore::CreateAttribute` and siblings).
fn create_dmg(
    class: &'static str,
    jitter: bool,
    t: &Tables,
    store_pct: f32,
    rng: &mut Roll,
) -> Vec<DmgInst> {
    let Some(def) = class_def(class) else {
        return Vec::new();
    };
    let kind = dmg_kind(class);
    let mut slots: [Option<DmgInst>; 4] = Default::default();
    if let Some(mut b) = load_dmg(def, kind, t.base, Source::Base) {
        b.add_jitter(if jitter { store_pct } else { 0.0 }, rng);
        slots[0] = Some(b);
    }
    for (i, a) in t.affixes.iter().enumerate() {
        let Some(a) = a else { continue };
        if let Some(mut x) = load_dmg(def, kind, a, AFFIX_SOURCES[i]) {
            x.add_jitter(a.float("lootRandomizerJitter"), rng);
            slots[i + 1] = Some(x);
        }
    }
    merge_slots(&mut slots, |a, b| a.merge(b));
    slots.into_iter().flatten().collect()
}

/// Pairwise merges in the engine's order: (0,1) (0,2) (0,3) (1,2) (1,3) (2,3).
fn merge_slots<T>(slots: &mut [Option<T>; 4], mut f: impl FnMut(&mut T, &T) -> bool) {
    for i in 0..4 {
        for j in i + 1..4 {
            let (lo, hi) = slots.split_at_mut(j);
            if let (Some(a), Some(b)) = (lo[i].as_mut(), hi[0].as_ref())
                && f(a, b)
            {
                hi[0] = None;
            }
        }
    }
}

#[derive(Debug, Clone)]
struct DefInst {
    values: Vec<f32>,
    chance: f32,
    source: Source,
}

/// `DefenseAttribute_Typical` creators (value array, base jitter
/// with the store's 20% when passed, affixes with their `lootRandomizerJitter`,
/// both through `DefenseAttribute_Typical::AddJitter`: pct > 0,
/// then clamped to [0, 50]; e.g. creator `DefenseAttributeStore::CreateAttribute`).
fn create_def(
    class: &'static str,
    jitter: bool,
    t: &Tables,
    store_pct: f32,
    rng: &mut Roll,
) -> Vec<(DefInst, &'static FieldDef)> {
    let Some(def) = class_def(class) else {
        return Vec::new();
    };
    let Some(field) = def.value else {
        return Vec::new();
    };
    let load = |r: &Record, source| {
        let values = r.values(field);
        let chance = def
            .chance
            .map(|c| r.value_at(c, 0).unwrap_or(0.0))
            .unwrap_or(0.0);
        (!values.is_empty()).then_some(DefInst {
            values,
            chance,
            source,
        })
    };
    let jit = |d: &mut DefInst, pct: f32, rng: &mut Roll| {
        if pct > 0.0 {
            let pct = pct.clamp(0.0, JITTER_CAP);
            d.values
                .iter_mut()
                .for_each(|v| *v = rng.int(*v, pct, false));
        }
    };
    let mut slots: [Option<DefInst>; 4] = Default::default();
    if let Some(mut b) = load(t.base, Source::Base) {
        jit(&mut b, if jitter { store_pct } else { 0.0 }, rng);
        slots[0] = Some(b);
    }
    for (i, a) in t.affixes.iter().enumerate() {
        let Some(a) = a else { continue };
        if let Some(mut x) = load(a, AFFIX_SOURCES[i]) {
            jit(&mut x, a.float("lootRandomizerJitter"), rng);
            slots[i + 1] = Some(x);
        }
    }
    // `DefenseAttribute_Typical::MergeDefense`: equal chance and
    // exactly one value in the other → added to this one's first value (this
    // one's length is not checked).
    merge_slots(&mut slots, |a, b| {
        if a.chance == b.chance && !a.values.is_empty() && b.values.len() == 1 {
            a.values[0] += b.values[0];
            true
        } else {
            false
        }
    });
    slots.into_iter().flatten().map(|d| (d, def)).collect()
}

/// Resolution entry point.
pub fn resolve(db: &ItemDb, item: &Item) -> Option<ResolvedItem> {
    let base = db.get(&item.base_name)?;
    Some(resolve_records(
        db,
        item,
        base,
        db.get(&item.prefix_name),
        db.get(&item.suffix_name),
        db.get(&item.modifier_name),
    ))
}

/// The `_Min` / `_Max` attribute sets of an equipment item: the same records
/// through the same loaders with every jitter replaced by its bound
/// ([`Roll::Bound`]). Entry `i` pairs with `ResolvedItem::attrs[i]` (the
/// rolled set comes first there and has the same creators, merges and order).
#[derive(Debug, Clone)]
pub struct AttrBounds {
    pub min: Vec<Attr>,
    pub max: Vec<Attr>,
}

/// [`AttrBounds`] of an equipment item (`None` for other classes: only
/// `ItemEquipment` builds the stores `ItemEquipment::InitializeItem`).
pub fn attr_bounds(db: &ItemDb, item: &Item) -> Option<AttrBounds> {
    let base = db.get(&item.base_name)?;
    if !is_equipment_class(&base.class) {
        return None;
    }
    let t = Tables {
        base,
        affixes: [
            db.get(&item.prefix_name),
            db.get(&item.suffix_name),
            db.get(&item.modifier_name),
        ],
    };
    let is_weapon = is_weapon_class(&base.class);
    let set = |max| {
        let mut out = Vec::new();
        build_attrs(&t, is_weapon, false, &mut Roll::Bound { max }, &mut out);
        out
    };
    Some(AttrBounds {
        min: set(false),
        max: set(true),
    })
}

pub fn resolve_records(
    db: &ItemDb,
    item: &Item,
    base: &Record,
    prefix: Option<&Record>,
    suffix: Option<&Record>,
    modifier: Option<&Record>,
) -> ResolvedItem {
    let class = base.class.clone();
    let is_weapon = is_weapon_class(&class);
    // `ItemArtifact::InitializeItem`: a relic loads only
    // its own table (and the completion bonus below); the save's prefix /
    // suffix / modifier names (a saved relic can carry `ac04_energyregen`)
    // never reach its attributes.
    let affixes = if class == "ItemArtifact" {
        [None, None, None]
    } else {
        [prefix, suffix, modifier]
    };
    let t = Tables { base, affixes };
    let mut rng = item_stream(item.seed);
    let mut attrs = Vec::new();
    // Relics (`ItemArtifact`) roll their own attributes like equipment
    // (`InitializeItem` seeds an `AttributePak` from the item
    // seed; with the store jitter a saved relic gives the game's exact Spirit).
    if is_equipment_class(&class) || class == "ItemArtifact" {
        let relic = class == "ItemArtifact";
        build_attrs(
            &t,
            is_weapon,
            relic,
            &mut Roll::Seeded(&mut rng),
            &mut attrs,
        );
    } else {
        // Relics, components, augments, potions...: plain record values
        // (their InitializeItem never touches the seed, as in
        // ItemRelic/ItemEnchantment).
        attrs.extend(record_attrs(base, Source::Base));
    }
    let rng_end = rng.0;
    // Component, relic completion bonus, augment. The completion bonus
    // belongs to relics only and rolls its own `lootRandomizerJitter` on a
    // fresh stream from the relic's seed (`ItemArtifact::
    // InitializeItem` → `AttributePak::LoadFromDatabase(bonus,
    // seed, true)`). Equipment ignores its `relic_bonus`
    // (`ItemEquipment::InitializeItem` only hands it to
    // `AddRelic`, which stores it in the inner `ItemRelic`'s replica; no
    // `ItemRelic`/`ItemEquipment` code builds attributes from it). The
    // component / augment values themselves are fixed record values.
    for (name, src) in [
        (&item.relic_name, Source::Relic),
        (&item.relic_bonus, Source::RelicBonus),
        (&item.augment_name, Source::Augment),
        (&item.ascendant_name, Source::Ascendant),
    ] {
        if src == Source::RelicBonus && class != "ItemArtifact" {
            continue;
        }
        let Some(r) = db.get(name) else { continue };
        // The ascendant affix does not roll: `AddAscendantBonus(name, seed)`
        // loads it with `AttributePak::LoadFromTable` (no RNG; the seed only
        // goes to the display copy), so it keeps its record values.
        if src == Source::RelicBonus && r.float("lootRandomizerJitter") > 0.0 {
            let seed = item.seed;
            let empty = Record::default();
            let t = Tables {
                base: &empty,
                affixes: [Some(r), None, None],
            };
            let mut bonus = Vec::new();
            build_attrs(
                &t,
                false,
                true,
                &mut Roll::Seeded(&mut item_stream(seed)),
                &mut bonus,
            );
            for b in &mut bonus {
                b.source = src;
            }
            attrs.extend(bonus);
            continue;
        }
        attrs.extend(record_attrs(r, src));
    }

    let cls_of =
        |r: Option<&Record>| r.map(|r| Classification::parse(r.string("itemClassification")));
    let base_classification = Classification::parse(base.string("itemClassification"));
    let mut classification = base_classification;
    let (pc, sc) = (cls_of(prefix), cls_of(suffix));
    for c in [pc, sc].into_iter().flatten() {
        classification = classification.max(c);
    }
    // `ItemRelic::Load`: a component's classification
    // is always Relic (8), whatever its record's `itemClassification`;
    // likewise `ItemArtifact::Load` sets 10 (a relic's record says Rare /
    // Epic / Legendary, which would send it through the loot filter's
    // quality and type checks and hide it) and `ItemArtifactFormula::Load`
    // 11, `ItemEnchantment::Load` 14 (an augment: the loot
    // filter always passes it, its colour index and beam are gameiteminfo's
    // `enchantment*` entries, its name draws in `ItemNameEnchantment`; not
    // being equipment, it drops with its record's `dropSound3D`), and
    // `ItemNote::Load` 13 (a lore note's record says Common, which the loot
    // filter's equipment gate rejects: no ground label, no pickup).
    classification = match class.as_str() {
        "ItemRelic" => Classification::Relic,
        "ItemArtifact" => Classification::Artifact,
        "ItemArtifactFormula" => Classification::ArtifactFormula,
        "ItemNote" => Classification::Lore,
        "ItemEnchantment" => Classification::Enchantment,
        _ => classification,
    };
    let name = NameTags {
        prefix: prefix
            .filter(|_| !base.bool("hidePrefixName"))
            .map(|r| r.string("lootRandomizerName").to_string())
            .unwrap_or_default(),
        quality: base.string("itemQualityTag").to_string(),
        style: base.string("itemStyleTag").to_string(),
        // `Item::GetGameDescription`: an item that is not equipment (a note, a component, a potion) has no
        // `itemNameTag`; its name is the record's `description` (`naming::record_name`).
        base: match base.string("itemNameTag") {
            "" if !is_equipment_class(&class) => base.string("description").to_string(),
            tag => tag.to_string(),
        },
        suffix: suffix
            .filter(|_| !base.bool("hideSuffixName"))
            .map(|r| r.string("lootRandomizerName").to_string())
            .unwrap_or_default(),
    };
    let item_level = base.int("itemLevel").max(0) as u32;
    let mut out = ResolvedItem {
        item: item.clone(),
        class,
        is_weapon,
        classification,
        base_classification,
        prefix_classification: pc,
        suffix_classification: sc,
        item_level,
        requirements: Requirements::default(),
        name,
        attrs,
        item_set: base.str("itemSetName").map(str::to_string),
        // Components (`ItemRelic`) have no `bitmap`: `relicBitmap` when
        // complete, `shardBitmap` while partial (`ItemRelic::
        // GetBitmap`: relic level ≥ `completedRelicLevel`; the
        // level is `var1`, 1 when `var1` is 0, see `components::is_complete`).
        bitmap: {
            let b = base.string("bitmap");
            let art = base.string("artifactBitmap");
            let formula = base.string("artifactFormulaBitmapName");
            if base.class == "ItemNote" {
                // `ItemNote::GetBitmap`: the `noteBitmap` texture `ItemNote::Load` loads (no `bitmap`).
                base.string("noteBitmap").to_string()
            } else if !b.is_empty() {
                b.to_string()
            } else if !formula.is_empty() {
                // `ItemArtifactFormula::Load`: the
                // blueprint's texture is `artifactFormulaBitmapName`.
                formula.to_string()
            } else if !art.is_empty() {
                // `ItemArtifact::Load`: `artifactBitmap`.
                art.to_string()
            } else {
                let need = base.int("completedRelicLevel").max(1) as u32;
                let complete = item.var1.max(1) >= need;
                let k = if complete {
                    "relicBitmap"
                } else {
                    "shardBitmap"
                };
                let b = base.string(k);
                if b.is_empty() {
                    base.string(if complete {
                        "shardBitmap"
                    } else {
                        "relicBitmap"
                    })
                    .to_string()
                } else {
                    b.to_string()
                }
            }
        },
        max_stack: max_stack(base, item, engine_max_stack(db.get(GAME_ITEM_INFO))),
        rng_end,
    };
    out.requirements = requirements(db, item, &out, base, prefix, suffix);
    out
}

/// `relic`: the stores of an `AttributePak` (`ItemArtifact` and its completion
/// bonus) rather than of `ItemEquipment`; only the skill base jitter's place
/// differs (see the skill arm).
fn build_attrs(t: &Tables, is_weapon: bool, relic: bool, rng: &mut Roll, attrs: &mut Vec<Attr>) {
    // The equipment stores pass 20 %; the `_Min` / `_Max` stores pass 0, so
    // their `Load` falls back to the base record's `lootRandomizerJitter`,
    // default 20 (`CharAttributeStore_Min::Load`,
    // `DamageAttributeStore_Min::Load` and the retaliation /
    // defense / penalty-reduction siblings: `GetFloat(.., 20.0)`; no stock
    // base record has the field, so it is 20 there too).
    let store_pct = if rng.seeded() {
        STORE_JITTER
    } else {
        t.base
            .value_at("lootRandomizerJitter", 0)
            .unwrap_or(STORE_JITTER)
    };
    let mut damage: Vec<(DmgInst, &'static str)> = Vec::new();
    // Equipment skill attributes whose base array still takes the store's
    // `AddJitter(20)`: (index in `attrs`, base array, affix parts).
    let mut skill_pending: Vec<(usize, Vec<f32>, [f32; 3])> = Vec::new();
    for (store, creator) in EQUIPMENT_ORDER {
        let mut c = creator;
        loop {
            match c {
                Creator::WeaponOnly(inner) if is_weapon => c = inner,
                Creator::NonWeaponOnly(inner) if !is_weapon => c = inner,
                _ => break,
            }
        }
        match (*store, c) {
            (_, Creator::WeaponOnly(_) | Creator::NonWeaponOnly(_)) => {}
            (StoreKind::Char, Creator::BaseAttackSpeed) => {
                // `CharAttributeVal_BaseAttackSpeed`: only the modifier table
                // loads (prefix/suffix are no-ops), no jitter, never deleted.
                let f = "characterBaseAttackSpeed";
                let base = t.base.values(f);
                let m = t.affixes[2].map_or(0.0, |r| r.float(f));
                let v = base.first().copied().unwrap_or(0.0) + m;
                attrs.push(Attr {
                    global: None,
                    store: StoreKind::Char,
                    class: "CharAttributeVal_BaseAttackSpeed",
                    source: Source::Base,
                    values: vec![(f.to_string(), v)],
                });
            }
            (StoreKind::Char, Creator::Char { field, jitter }) => {
                // CharAttribute template (`CharAttributeStore::CreateAttribute`): affixes load (and
                // jitter with their lootRandomizerJitter if the store passed a
                // percentage) BEFORE the base array is jittered.
                let mut base = t.base.values(field);
                let mut parts = [0.0f32; 3];
                for (i, a) in t.affixes.iter().enumerate() {
                    if let Some(a) = a {
                        let v = a.float(field);
                        // `_Min` / `_Max`: gated by the store percentage, the
                        // affix's own one applied (`CharAttribute::
                        // LoadPrefixTableMin`).
                        parts[i] = if *jitter && store_pct > 0.0 {
                            rng.int(v, a.float("lootRandomizerJitter"), true)
                        } else {
                            v
                        };
                    }
                }
                if base.is_empty() && parts == [0.0; 3] {
                    continue;
                }
                if *jitter && store_pct > 0.0 {
                    let pct = store_pct.clamp(0.0, 50.0);
                    base.iter_mut().for_each(|b| *b = rng.int(*b, pct, true));
                }
                let v = base.first().copied().unwrap_or(0.0) + parts[0] + parts[1] + parts[2];
                attrs.push(Attr {
                    global: None,
                    store: StoreKind::Char,
                    class: "CharAttribute",
                    source: Source::Base,
                    values: vec![(field.to_string(), v)],
                });
            }
            (StoreKind::Skill, Creator::Skill { field }) => {
                let mut base = t.base.values(field);
                let mut parts = [0.0f32; 3];
                for (i, a) in t.affixes.iter().enumerate() {
                    if let Some(a) = a {
                        parts[i] = rng.skill(a.float(field), a.float("lootRandomizerJitter"));
                    }
                }
                if base.is_empty() && parts == [0.0; 3] {
                    continue;
                }
                // The base array (`LoadBaseTable`, no RNG) takes the store's
                // `AddJitter(20)`: `SkillAttributeStore::AddJitter`
                // → `SkillAttribute::AddJitter` (pct clamped to
                // [0, 50], `SkillAttribute::Jitter` on every base entry; the affix
                // parts are not touched). Call sites:
                // `ItemEquipment::InitializeItem`, after every store
                // has loaded and the damage scale ran, so its draws come last on
                // the item stream (nothing reads the stream afterwards); the
                // `_Min` / `_Max` stores take `AddMinJitter` / `AddMaxJitter`(20)
                // there (the bound). A relic's
                // `SkillAttributeStore_Skill::Load` calls it at the end
                // of its own load (`AttributePak::LoadFromTable`, pct 20 from
                // `ItemArtifact::InitializeItem`): in place, before
                // the damage store.
                let pct = STORE_JITTER.clamp(0.0, JITTER_CAP);
                if relic || !rng.seeded() {
                    base.iter_mut().for_each(|b| *b = rng.skill_base(*b, pct));
                } else {
                    skill_pending.push((attrs.len(), base.clone(), parts));
                }
                let v = base.first().copied().unwrap_or(0.0) + parts[0] + parts[1] + parts[2];
                attrs.push(Attr {
                    global: None,
                    store: StoreKind::Skill,
                    class: "SkillAttribute",
                    source: Source::Base,
                    values: vec![(field.to_string(), v)],
                });
            }
            (StoreKind::Damage, Creator::BasePhysical) => {
                let min = t.base.float("offensivePhysicalMin");
                let max = t.base.float("offensivePhysicalMax");
                let range = (max - min).max(0.0);
                attrs.push(Attr {
                    global: None,
                    store: StoreKind::Damage,
                    class: "DamageAttribute_BasePhysical",
                    source: Source::Base,
                    values: vec![
                        ("offensivePhysicalMin".into(), min),
                        ("offensivePhysicalMax".into(), min + range),
                        (
                            "offensivePierceRatioMin".into(),
                            t.base.float("offensivePierceRatioMin"),
                        ),
                    ],
                });
            }
            (StoreKind::Damage | StoreKind::Retaliation, Creator::Class { class, jitter }) => {
                let insts = create_dmg(class, *jitter, t, store_pct, rng);
                if *store == StoreKind::Damage {
                    damage.extend(insts.into_iter().map(|d| (d, *class)));
                } else if let Some(def) = class_def(class) {
                    // PORT: a global retaliation attribute counts on its own.
                    attrs.extend(
                        insts
                            .iter()
                            .map(|d| d.to_attr(StoreKind::Retaliation, class, def, None)),
                    );
                }
            }
            (StoreKind::Defense, Creator::Armor) => attrs.push(armor(t, store_pct, rng)),
            (StoreKind::Defense, Creator::Class { class, jitter }) => {
                for (d, def) in create_def(class, *jitter, t, store_pct, rng) {
                    let mut values = vec![(
                        def.value.unwrap().to_string(),
                        d.values.first().copied().unwrap_or(0.0),
                    )];
                    if let (Some(c), true) = (def.chance, d.chance != 0.0) {
                        values.push((c.to_string(), d.chance));
                    }
                    attrs.push(Attr {
                        store: StoreKind::Defense,
                        class,
                        source: d.source,
                        values,
                        global: None,
                    });
                }
            }
            (StoreKind::Conversion, Creator::Conversion) => attrs.extend(conversions(t, rng)),
            (s, c) => debug_assert!(false, "unhandled creator {s:?} {c:?}"),
        }
    }
    // Damage scale (InitializeItem: prefix + suffix lootRandomizerScale +
    // base attributeScalePercent; only the damage store).
    let pct = t.affixes[0].map_or(0.0, |r| r.float("lootRandomizerScale"))
        + t.affixes[1].map_or(0.0, |r| r.float("lootRandomizerScale"));
    let pct = pct + t.base.float("attributeScalePercent");
    let global_chance = t
        .affixes
        .iter()
        .rev()
        .flatten()
        .map(|a| a.float("offensiveGlobalChance"))
        .find(|c| *c > 0.0)
        .unwrap_or_else(|| {
            t.base
                .values("offensiveGlobalChance")
                .first()
                .copied()
                .unwrap_or(0.0)
        });
    for (d, class) in &mut damage {
        if pct != 0.0 {
            d.scale(pct);
        }
        if let Some(def) = class_def(class) {
            attrs.push(d.to_attr(StoreKind::Damage, class, def, Some(global_chance)));
        }
    }
    // The equipment skill store's `AddJitter(20)`, last (see the skill arm).
    let skill_pct = STORE_JITTER.clamp(0.0, JITTER_CAP);
    for (i, mut base, parts) in skill_pending {
        base.iter_mut()
            .for_each(|b| *b = rng.skill_base(*b, skill_pct));
        attrs[i].values[0].1 =
            base.first().copied().unwrap_or(0.0) + parts[0] + parts[1] + parts[2];
    }
}

/// `DefenseAttribute_BaseProtectionAbsorption`: base
/// `defensiveProtection`/`defensiveBonusProtection` arrays are not jittered;
/// the affix values are, at the store percentage clamped to [0, 50], in the
/// order prefix/suffix/modifier protection, then the three bonus values.
fn armor(t: &Tables, store_pct: f32, rng: &mut Roll) -> Attr {
    let base_p = t
        .base
        .values("defensiveProtection")
        .first()
        .copied()
        .unwrap_or(0.0);
    let base_b = t
        .base
        .values("defensiveBonusProtection")
        .first()
        .copied()
        .unwrap_or(0.0);
    let mut ap = [0.0f32; 6];
    for (i, a) in t.affixes.iter().enumerate() {
        if let Some(a) = a {
            ap[i] = a.float("defensiveProtection");
            ap[i + 3] = a.float("defensiveBonusProtection");
        }
    }
    let pct = store_pct.clamp(0.0, 50.0);
    for v in &mut ap {
        if *v > 0.0 {
            *v = rng.int(*v, pct, false);
        }
    }
    Attr {
        global: None,
        store: StoreKind::Defense,
        class: "DefenseAttribute_BaseProtectionAbsorption",
        source: Source::Base,
        values: vec![
            ("defensiveProtection".into(), base_p + ap[0] + ap[1] + ap[2]),
            (
                "defensiveBonusProtection".into(),
                base_b + ap[3] + ap[4] + ap[5],
            ),
        ],
    }
}

/// Conversion store (`ConversionAttributeStore::Load`: base 1, base 2 at 20%,
/// then per affix 1, 2 at its `lootRandomizerJitter`).
fn conversions(t: &Tables, rng: &mut Roll) -> Vec<Attr> {
    let mut out = Vec::new();
    let tables = [Some(t.base), t.affixes[0], t.affixes[1], t.affixes[2]];
    let sources = [
        Source::Base,
        Source::Prefix,
        Source::Suffix,
        Source::Modifier,
    ];
    for (k, tab) in tables.iter().enumerate() {
        let Some(tab) = tab else { continue };
        let pct = if k == 0 {
            STORE_JITTER
        } else {
            tab.float("lootRandomizerJitter")
        };
        for sfx in ["", "2"] {
            let pcts = tab.values(&format!("conversionPercentage{sfx}"));
            let in_t = tab.string(&format!("conversionInType{sfx}"));
            if pcts.is_empty() || in_t.is_empty() {
                continue;
            }
            let mut v = pcts[0];
            if pct > 0.0 {
                v = rng.continuous(v, pct);
                for _ in 1..pcts.len() {
                    rng.skip();
                }
            }
            out.push(Attr {
                global: None,
                store: StoreKind::Conversion,
                class: "ConversionAttribute",
                source: sources[k],
                values: vec![
                    (format!("conversionPercentage{sfx}"), v),
                    (format!("conversionInType{sfx}:{in_t}"), 0.0),
                    (
                        format!(
                            "conversionOutType{sfx}:{}",
                            tab.string(&format!("conversionOutType{sfx}"))
                        ),
                        0.0,
                    ),
                ],
            });
        }
    }
    out
}

/// Every catalogued attribute field of a record at level index 0, unjittered
/// (relics, components, augments, set bonuses at a given index).
pub fn record_attrs(r: &Record, source: Source) -> Vec<Attr> {
    record_attrs_at(r, source, 0)
}

pub fn record_attrs_at(r: &Record, source: Source, level_index: usize) -> Vec<Attr> {
    let mut out = Vec::new();
    for def in FIELDS {
        let mut values = Vec::new();
        for f in def.fields() {
            if let Some(x) = r.value_at(f, level_index)
                && x != 0.0
            {
                values.push((f.to_string(), x));
            }
        }
        // ranges: report max as given
        if !values.is_empty() && !values.iter().all(|(k, _)| Some(k.as_str()) == def.chance) {
            let store = match def.family {
                AttrFamily::CharacterValue
                | AttrFamily::CharacterModifier
                | AttrFamily::CharacterMultiplier => StoreKind::Char,
                AttrFamily::SkillValue | AttrFamily::SkillModifier => StoreKind::Skill,
                AttrFamily::Retaliation
                | AttrFamily::RetaliationModifier
                | AttrFamily::RetaliationDuration
                | AttrFamily::RetaliationDurationModifier
                | AttrFamily::RetaliationCrowdControl => StoreKind::Retaliation,
                AttrFamily::Defense
                | AttrFamily::DefenseModifier
                | AttrFamily::DefenseDuration
                | AttrFamily::DefenseDurationModifier
                | AttrFamily::DefenseMaxResist
                | AttrFamily::DefenseMisc
                | AttrFamily::Armor => StoreKind::Defense,
                _ => StoreKind::Damage,
            };
            // Avoid double counting the shared weapon/skill physical fields.
            if def.class == "DamageAttribute_BasePhysical" {
                continue;
            }
            let at = |f: &str| r.value_at(f, level_index).unwrap_or(0.0);
            let tag = |sfx: &str| {
                def.global_stem()
                    .is_some_and(|stem| at(&format!("{stem}{sfx}")) != 0.0)
            };
            let global = (store == StoreKind::Damage && tag("Global")).then(|| Global {
                chance: at("offensiveGlobalChance"),
                xor: tag("XOR"),
            });
            out.push(Attr {
                store,
                class: def.class,
                source,
                values,
                global,
            });
        }
    }
    out
}

/// Variables for the requirement/cost equations (`ItemEquipment::
/// ResolveEquationVariable`: `itemLevel` from the item, the rest
/// from the cost-info totals that `ItemEquipment::DumpCostAttributes`
/// gathers from every store's `GetCostInfo`):
/// `damageAvgBase` sums `DamageAttribute_BasePhysical` (min + max) / 2
/// and every `DamageAttributeAbsBase*` (`DamageAttributeAbsBase::GetCostInfo`: the
/// mean of min and max when both are positive, else their sum, × chance / 100
/// when it has a chance); `damageAvgPierceRatio` sums the pierce ratio of
/// base and bonus physical damage (`DamageAttribute_BasePhysical` / `DamageAttribute_Physical::GetCostInfo`);
/// `defenseAttrArmor` sums the first value of each
/// `DefenseAttribute_BaseProtectionAbsorption` (`GetCostInfo`);
/// `charAttackSpeed` the base attack speed values (`CharAttributeVal_
/// BaseAttackSpeed::GetCostInfo`); `shieldBlockChance` /
/// `shieldBlockDefense` are the shield record's `defensiveBlockChance` /
/// `defensiveBlock` (`WeaponArmor_Shield::Load`), 0 elsewhere.
/// `ItemEquipment::InitializeItem`: the equation is evaluated (and
/// the cost cached, nothing else writes it) right after the base, prefix,
/// suffix and modifier stores load and before `AddRelic` /
/// `AddEnchantment` / `AddAscendantBonus`, so the component, augment and
/// ascendant attributes count in none of these variables.
fn equation_var(item: &ResolvedItem, base: &Record, name: &str) -> Option<f32> {
    let own = || {
        item.attrs.iter().filter(|a| {
            matches!(
                a.source,
                Source::Base | Source::Prefix | Source::Suffix | Source::Modifier
            )
        })
    };
    let total = |f: &str| own().filter_map(|a| a.get(f)).sum::<f32>();
    let shield = item.class == "WeaponArmor_Shield";
    Some(match name {
        "itemLevel" => item.item_level as f32,
        "damageAvgBase" => own()
            .map(|a| {
                let def = class_def(a.class);
                let v = |f: Option<&str>| f.and_then(|f| a.get(f)).unwrap_or(0.0);
                if a.class == "DamageAttribute_BasePhysical" {
                    (a.get("offensivePhysicalMin").unwrap_or(0.0)
                        + a.get("offensivePhysicalMax").unwrap_or(0.0))
                        * 0.5
                } else if a.class.starts_with("DamageAttributeAbsBase")
                    && let Some(d) = def
                {
                    let (min, max) = (v(d.min), v(d.max));
                    let mut avg = if min > 0.0 && max > 0.0 {
                        (min + max) * 0.5
                    } else {
                        min + max
                    };
                    let chance = v(d.chance);
                    if chance > 0.0 {
                        avg *= chance * 0.01;
                    }
                    avg.max(0.0)
                } else {
                    0.0
                }
            })
            .sum(),
        "damageAvgPierceRatio" => total("offensivePierceRatioMin"),
        // `ItemEquipment::GetTotalAttrCostCount`: every attribute
        // the item's stores count, base ones included (weapons' cost
        // equations subtract 2 for damage and speed, armour's 1).
        "totalAttCount" => own().count() as f32,
        "defenseAttrArmor" => total("defensiveProtection"),
        "shieldBlockChance" => {
            if shield {
                base.float("defensiveBlockChance")
            } else {
                0.0
            }
        }
        "shieldBlockDefense" => {
            if shield {
                base.float("defensiveBlock")
            } else {
                0.0
            }
        }
        "charAttackSpeed" => total("characterBaseAttackSpeed"),
        _ => return None,
    })
}

/// The item's requirements; the level one is raised to its component's and
/// augment's (`ItemEquipment::GetLevelRequirement`).
fn requirements(
    db: &ItemDb,
    saved: &Item,
    item: &ResolvedItem,
    base: &Record,
    prefix: Option<&Record>,
    suffix: Option<&Record>,
) -> Requirements {
    let explicit = [
        base.int("levelRequirement"),
        base.int("strengthRequirement"),
        base.int("dexterityRequirement"),
        base.int("intelligenceRequirement"),
    ];
    let mut out = explicit;
    if let Some(pfx) = requirement_prefix(&item.class) {
        let costs = base
            .str("itemCostName")
            .and_then(|n| db.get(n))
            .or_else(|| db.get("records/game/itemcostformulas.dbr"));
        if let Some(costs) = costs {
            for (i, kind) in ["Level", "Strength", "Dexterity", "Intelligence"]
                .iter()
                .enumerate()
            {
                if out[i] != 0 {
                    continue;
                }
                let Some(src) = costs.str(&format!("{pfx}{kind}Equation")) else {
                    continue;
                };
                let Ok(e) = Expr::parse(src) else { continue };
                let v = e.eval(&|n| equation_var(item, base, n)).max(0.0);
                out[i] = (v + 0.5) as i32;
            }
        }
    }
    let lvl = out[0]
        .max(prefix.map_or(0, |p| p.int("levelRequirement")))
        .max(suffix.map_or(0, |s| s.int("levelRequirement")))
        .max(
            [&saved.relic_name, &saved.augment_name]
                .into_iter()
                .filter(|n| !n.is_empty())
                .filter_map(|n| db.get(n))
                .map(|r| r.int("levelRequirement"))
                .fold(0, i32::max),
        );
    Requirements {
        level: lvl.max(0) as u32,
        physique: out[1].max(0) as u32,
        cunning: out[2].max(0) as u32,
        spirit: out[3].max(0) as u32,
    }
}

impl ResolvedItem {
    /// Sum of every attribute value per field, global-chance attributes
    /// left out (they count through `actor::item_global_groups`).
    pub fn totals(&self) -> BTreeMap<String, f32> {
        let mut m = BTreeMap::new();
        for a in self.attrs.iter().filter(|a| a.global.is_none()) {
            for (k, v) in &a.values {
                *m.entry(k.clone()).or_insert(0.0) += *v;
            }
        }
        m
    }

    /// Adds the item's character attributes (value / modifier / multiplier)
    /// to an accumulator — the equipment pass of `CharacterBio::UpdateResources`.
    pub fn add_char_attributes(&self, acc: &mut CharAttrAccumulator) {
        for a in &self.attrs {
            for (k, v) in &a.values {
                let Some(def) = crate::attributes::lookup(k) else {
                    continue;
                };
                let Some(attr) = def.char_attr() else {
                    continue;
                };
                match def.family {
                    AttrFamily::CharacterValue => acc.add_value(attr, *v),
                    AttrFamily::CharacterModifier => acc.add_modifier(attr, *v),
                    AttrFamily::CharacterMultiplier => acc.add_multiplier(attr, *v),
                    _ => {}
                }
            }
        }
    }

    pub fn display_color_index(&self) -> u32 {
        self.classification.color_index(
            self.base_classification,
            self.prefix_classification,
            self.suffix_classification,
        )
    }
}

/// The item's value (`Item::GetItemCost`, `ItemEquipment::GetItemCost`
/// / `InitializeItem` / `Load`): equipment uses its class's cost
/// equation (named by the class constructor: `Armor` armour,
/// `ArmorJewelry` jewelry (waist is armour), shield, `offhandCostEquation`
/// for off-hands, melee 2h, `weaponRangedCostEquation` for 1h ranged and
/// staves, ranged 2h, other weapons `weaponCostEquation`) from the base's
/// `itemCostName` table (default `itemcostformulas.dbr`), with
/// `itemPrefixCost`/`itemSuffixCost` = the affixes' `lootRandomizerCost`; a
/// non-zero base `itemCost` replaces the equation. The result is scaled by
/// 1 + (the affixes' `marketAdjustmentPercent` + the base's
/// `itemCostScalePercent`) / 100, rounded half up (equation variables from
/// the base / affix / modifier attributes only, see `equation_var`), plus the inserted
/// component's and augment's own costs. Other items: `itemCost` × (1 +
/// `itemCostScalePercent` / 100), rounded, × stack size. The costs are
/// unsigned 32-bit: the stack product (`Item::GetItemCost`) and
/// the sum with the inserted costs wrap.
pub fn item_cost(db: &ItemDb, item: &Item) -> u32 {
    let Some(base) = db.get(&item.base_name) else {
        return 0;
    };
    let first_cost = |r: &Record| r.int("itemCost") as f32;
    let c = base.class.as_str();
    let eq = if c.starts_with("ArmorProtective_") {
        "armorCostEquation"
    } else if c.starts_with("ArmorJewelry_") {
        "jewelryCostEquation"
    } else if c == "WeaponArmor_Shield" {
        "shieldCostEquation"
    } else if c == "WeaponArmor_Offhand" {
        "offhandCostEquation"
    } else if c == "WeaponHunting_Ranged2h" {
        "weaponRanged2hCostEquation"
    } else if c == "WeaponHunting_Ranged1h" || c == "WeaponMagical_Staff" {
        "weaponRangedCostEquation"
    } else if c.starts_with("WeaponMelee_") && c.ends_with("2h") {
        "weaponMelee2hCostEquation"
    } else if c.starts_with("Weapon") {
        "weaponCostEquation"
    } else {
        let fixed = first_cost(base);
        let own = (fixed * base.float("itemCostScalePercent") * 0.01 + fixed + 0.50001) as u32;
        return own.wrapping_mul(item.stack_count.max(1));
    };
    let (prefix, suffix) = (db.get(&item.prefix_name), db.get(&item.suffix_name));
    let market = |a: Option<&Record>| a.map_or(0.0, |a| a.float("marketAdjustmentPercent"));
    let scale = market(prefix) + market(suffix) + base.float("itemCostScalePercent");
    let fixed = first_cost(base);
    // `ItemEquipment::GetItemCost`: a fixed `itemCost`
    // truncates its scale term before adding it (the equation cost, cached
    // by `InitializeItem`, does not).
    let own = if fixed != 0.0 {
        ((scale * fixed * 0.01) as i32 as f32 + fixed + 0.5).max(0.0) as u32
    } else {
        let v = base
            .str("itemCostName")
            .and_then(|n| db.get(n))
            .or_else(|| db.get("records/game/itemcostformulas.dbr"))
            .and_then(|c| c.str(eq).and_then(|s| Expr::parse(s).ok()))
            .map_or(0.0, |e| {
                let r =
                    resolve_records(db, item, base, prefix, suffix, db.get(&item.modifier_name));
                let affix =
                    |a: Option<&Record>| a.map_or(0, |a| a.int("lootRandomizerCost")) as f32;
                e.eval(&|n| match n {
                    "itemPrefixCost" => Some(affix(prefix)),
                    "itemSuffixCost" => Some(affix(suffix)),
                    _ => equation_var(&r, base, n),
                })
            });
        (v + scale * v * 0.01 + 0.5).max(0.0) as u32
    };
    own.wrapping_add(inserted_costs(db, item))
}

/// The inserted component's and augment's costs.
/// `ItemRelic::GetItemCost` / `ItemEnchantment::GetItemCost`:
/// the `itemCost` entry of the component's (augment's)
/// level minus 1, clamped to the last entry (level 0 wraps to it), plus the
/// `petBonusCost` of its `petBonusName` record, scaled by 1 + (that record's
/// `marketAdjustmentPercent` + `itemCostScalePercent`) / 100, truncated. The
/// entry and the sums are unsigned 32-bit and wrap.
fn inserted_costs(db: &ItemDb, item: &Item) -> u32 {
    let inserted = |n: &str, level: u32| {
        let Some(x) = db.get(n) else { return 0 };
        let list = x.get("itemCost").unwrap_or_default();
        let i = (level.wrapping_sub(1) as usize).min(list.len().saturating_sub(1));
        let mut c = match list.get(i) {
            Some(RawValue::Int(i)) => *i as u32,
            _ => 0,
        };
        let mut market = 0.0;
        if let Some(p) = x.str("petBonusName").and_then(|p| db.get(p)) {
            c = c.wrapping_add(p.int("petBonusCost") as u32);
            market = p.float("marketAdjustmentPercent");
        }
        ((market + x.float("itemCostScalePercent")) * c as f32 * 0.01 + c as f32) as u32
    };
    inserted(&item.relic_name, item.var1)
        .wrapping_add(inserted(&item.augment_name, item.unk_augment))
}
