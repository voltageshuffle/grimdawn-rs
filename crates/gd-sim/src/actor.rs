//! Combat profiles of the player and of monsters, and one attack between two
//! of them ([`attack_linked`]), built from the stats/combat/items rules.
//!
//! The pieces are the ones they call (bio equations, accumulator,
//! OA/DA, `take_attack`). Monster skills: `Monster::Load` →
//! `SkillManager::LoadSkills` (34 slots, stops at the first empty
//! `skillNameN`; `skillLevelN` a number or an equation in `charLevel`,
//! `SkillManager::ResolveEquationVariable`, truncated to an integer). A
//! `Skill_Passive` counts only while enabled (`Skill::IsSkillEnabled`:
//! level > 0) and then adds its character attributes
//! (`CollectPassiveCharAttributes`) and its offensive / defensive ones
//! (`CollectPassiveOffensiveDamageAttributes`, `…DefenseAttributes`) to the
//! character's accumulators beside the equipment's.
//! `offensiveTotalDamageModifier` is no multiplier of its own:
//! `DamageAttributeAbsMod_TotalDamageModifier::AddModifierToAccumulator`
//! adds its % to every hit type's modifier (physical, pierce, cold, fire,
//! acid, lightning, vitality, chaos, aether) and every DoT type's (physical,
//! cold … aether, bleeding), plus a marker entry whose `Execute` does
//! nothing (see `hit_mod`).
//!
//! Damage over time is collected per owning source ([`Profile::dot_sources`]:
//! each item instance, passive / pak record, the attacking skill, each global
//! group), not summed into one gear entry: the defender's `DotManager` keys
//! its slots by the owner object + attribute (
//! `DamageAttributeDur::AddDamageToAccumulator`), so distinct
//! sources stack and only the same source's re-application keeps max. The
//! summed `totals` keep the DoT fields for the sheet, whose display
//! accumulator sums every source.
//!
//! The derived combat parameters an attack reads (the global groups'
//! attribute split, the chance-gated instances, the defender's resistances /
//! armor) are memoized per profile ([`Profile::combat_cache`]); gd-game
//! rebuilds the profile on `ProfileDirty`, so the cache never outlives its
//! inputs.
//!
//! Submodules: `rules` (game-wide rule records, speed caps), `display` (what
//! the sheet, tooltips and rollovers show), `retaliation`.
//!
//! PERF-36: the attack path reuses per-thread scratch buffers (AttackScratch,
//! combat::ConvertScratch) for its small collections instead of allocating per
//! hit. They hold no gameplay value between attacks (cleared on take and
//! return), so totals / CombatCache remain the single source.
use crate::attributes::{CharAttr, CombatType};
use crate::combat::{
    self, AttackKind, AttackerScaling, DamageScale, Defender, Defense, ReductionInfo, WeaponDamage,
};
use crate::stats::*;
use gd_formats::arz::Record;
use gd_formats::collections::HashMap;
use gd_formats::vfs::Database;
use gd_rules::Rng;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, OnceLock};

mod display;
mod retaliation;
mod rules;

pub use crate::combat::is_dot_damage;
pub use display::*;
pub use retaliation::*;
pub use rules::*;

#[derive(Debug, Clone, Default)]
pub struct Profile {
    pub level: i32,
    pub sheet: CharacterSheet,
    /// Base weapon/skill physical damage.
    pub physical: (f32, f32),
    pub pierce_ratio: f32,
    /// Summed `offensive*`/`defensive*` fields of weapon, armor, skills.
    pub totals: BTreeMap<String, f32>,
    /// Armor not tied to a body region (skills, bio, other items).
    pub protection: f32,
    /// Armor pieces' protection per body region (hit only there).
    pub armor_regions: Vec<(Region, f32)>,
    pub absorption: f32,
    pub resistances: Vec<(CombatType, f32)>,
    /// Attacks per second at animation speed 1 (`characterAttackSpeed`).
    pub attack_speed: f32,
    /// Capped run, attack and cast speed ([`speed_caps`] unslowed; 1 = record speed 1.0).
    pub speeds: [f32; 3],
    /// The bio's base run / attack / cast speed (`CharacterBio` base accumulator), the divisor of the sheet's speed percentages.
    pub base_speeds: [f32; 3],
    /// The same before the cap, with its bounds, for slows ([`speed_caps`]).
    pub speed_caps: [SpeedCap; 3],
    pub is_player: bool,
    /// A player's pet of the engine's `Pet` class (record class `Pet`, [`Side::Pet`]): its resistances cap at
    /// `playerPetDefenseCap` (`Pet::GetDefenseAttributeCap`: the pet cap while its faction,
    /// `Character.faction`, is 0 = the player's, which `Monster::JoinMe` copies from the player it joins; another
    /// faction takes the monster cap). Other summons (`Monster` class turrets, totems, ...) are monsters there.
    pub pet: bool,
    /// Global damage conversions (in type, out type, %): items, item sets,
    /// passive skills / devotions; a skill's own are added per attack.
    pub conversions: Vec<(CombatType, CombatType, f32)>,
    /// Dual wield: each weapon's own flat damage fields ([`Profile::off_hand`]).
    pub dual: Option<Box<HandFlats>>,
    /// Global-chance groups of the skill records in the profile (kept apart
    /// from `totals`, see [`GlobalGroup`]).
    pub global_groups: Vec<GlobalGroup>,
    /// The worn shield ([`combat::Shield::from_record`]); `TakeAttack`
    /// blocks with the left hand's shield (`EquipManager::GetLeftHandShield`).
    pub shield: Option<combat::Shield>,
    /// Life and energy per second the active self buffs cost: the sum of
    /// [`active_cost`] over them (the caller's, `gd_game::player::Loadout::profile_with`; 0 from
    /// [`player_from_items`]).
    pub active_costs: [f32; 2],
    /// The record's `characterRacialProfile` races ([`crate::racial`]).
    pub race: Vec<String>,
    /// The `…ReqReduction` attribute values of the worn gear (`items::req_reduction`).
    pub req_reduction: crate::items::req_reduction::Reduction,
    /// Racial bonuses of the worn items and the skill sources
    /// ([`crate::racial`]; the attacking skill's own are added per attack).
    pub racial: Vec<crate::racial::RacialBonus>,
    /// The charged `Skill_WeaponPool_ChargedScaling`s the profile was built with
    /// ([`crate::skills::charged_scaling_passive`]) and the offensive modifier fields each put in
    /// `totals`: the skill's own attack leaves them out ([`Attacker::without_skill`]).
    pub charged: Vec<(String, BTreeMap<String, f32>)>,
    /// The gear's and passives' damage-over-time fields per owning source
    /// ([`DotSource`]): an attack deals each as its own DoT (the summed
    /// `totals` keep them for the sheet only). Dual wield: the two weapons'
    /// own are in [`HandFlats`] instead.
    pub dot_sources: Vec<DotSource>,
    /// Memoized derived combat parameters ([`CombatCache`]), built lazily on
    /// the first attack after the profile is complete.
    pub(crate) combat_cache: Arc<OnceLock<CombatCache>>,
}

impl Profile {
    /// The speed fields from the character's accumulators: [`speed_caps`], those unslowed, and
    /// the bio's base speeds (`base`).
    fn set_speeds(
        &mut self,
        rules: &Rules,
        acc: &CharAttrAccumulator,
        base: &CharAttrAccumulator,
        class: SpeedClass,
        dual_melee: bool,
    ) {
        self.speed_caps = speed_caps(rules, acc, base, class, dual_melee);
        self.speeds = self.speed_caps.map(|c| c.capped(0.0));
        self.base_speeds = [
            CharAttr::RunSpeed,
            CharAttr::AttackSpeed,
            CharAttr::CastSpeed,
        ]
        .map(|a| base.get(a));
    }
}

/// One attack's reading of a [`Profile`], in place (no copy of it): a dual wielder's off-hand
/// swing reads the off-hand weapon's flat damage instead of the main weapon's
/// ([`Profile::off_hand`]), a charged skill's own attack reads `totals` without that skill's
/// passive offensive modifiers ([`Attacker::without_skill`]). [`Attacker::total`] is the attack's
/// one reading of `totals`.
#[derive(Debug, Clone, Copy)]
pub struct Attacker<'a> {
    pub profile: &'a Profile,
    /// An off-hand swing: the two weapons' own flats ([`Profile::dual`]).
    hands: Option<&'a HandFlats>,
    /// The attacking charged skill's passive offensive modifiers ([`Profile::charged`]).
    skill: Option<&'a BTreeMap<String, f32>>,
}

impl<'a> From<&'a Profile> for Attacker<'a> {
    fn from(profile: &'a Profile) -> Self {
        Self {
            profile,
            hands: None,
            skill: None,
        }
    }
}

impl Attacker<'_> {
    /// The attack of `skill` when it is one of [`Profile::charged`]: its passive offensive
    /// modifiers taken out of `totals` (`SkillManager::GetOffensiveModifierAttributes` skips the
    /// attacking skill by name); `None` otherwise.
    pub fn without_skill(self, skill: &str) -> Option<Self> {
        let (_, own) = self
            .profile
            .charged
            .iter()
            .find(|c| c.0.eq_ignore_ascii_case(skill))?;
        Some(Self {
            skill: Some(own),
            ..self
        })
    }

    /// A `totals` field as this attack reads it: on an off-hand swing less the main weapon's own
    /// flat and plus the off-hand's, then less the charged skill's own modifier where the totals
    /// (or a hand) hold the field.
    pub fn total(&self, k: &str) -> f32 {
        let get = |m: &BTreeMap<String, f32>| m.get(k).copied();
        let base = get(&self.profile.totals);
        let mut present = base.is_some();
        let mut v = base.unwrap_or(0.0);
        if let Some(h) = self.hands {
            let (main, off) = (get(&h.main), get(&h.off));
            present |= main.is_some() || off.is_some();
            v = v - main.unwrap_or(0.0) + off.unwrap_or(0.0);
        }
        if present && let Some(own) = self.skill {
            v -= get(own).unwrap_or(0.0);
        }
        v
    }

    /// The attack's base (weapon) physical damage and pierce ratio: an off-hand swing's from its
    /// totals ([`base_physical`]), else the profile's.
    fn base_physical(&self) -> ((f32, f32), f32) {
        match self.hands {
            Some(_) => base_physical(|k| self.total(k)),
            None => (self.profile.physical, self.profile.pierce_ratio),
        }
    }
}

/// One active skill's share of `SkillManager::GetActiveLifeCost` / `GetActiveManaCost` (life, energy per
/// second). `SkillManager::Update`: for every `SkillActivatedBuffSelf` skill
/// ([`crate::skills::ClassInfo::buff_self`]) whose
/// `Skill.active` byte is set, the virtual `Skill_Modifier::GetSkillModifierData` (shared by
/// every skill: `Skill::GetSkillModifierData` at `GetCurrentLevel`, modifiers included) is
/// summed into `SkillManager.activeLifeCost` / `SkillManager.activeManaCost`: the skill's PROFILE
/// (`GetSkillProfile`, a wrapper's `buffSkillName` record:
/// the devotion `tier3_08g_skill` keeps its cost in `tier3_08g_skill_buff`) at its level, plus each learned
/// modifier (`Skill.modifiers`) and gear skill modifier (`Skill.itemModifiers`) at theirs. `prof` is that
/// profile record, `mods` the
/// modifiers (record, level index). No reduction applies (`skillManaCostReduction` is the cast costs').
/// Stock: no `Skill_Modifier` record has a `skillActive*Cost`, so `mods` adds 0 there.
pub fn active_cost(prof: &Record, li: usize, mods: &[(Record, usize)]) -> [f32; 2] {
    let v = |r: &Record, k: &str, li: usize| crate::stats::level_value(r, k, li).unwrap_or(0.0);
    let mut c = [
        v(prof, "skillActiveLifeCost", li),
        v(prof, "skillActiveManaCost", li),
    ];
    for (m, mli) in mods {
        c[0] += v(m, "skillActiveLifeCost", *mli);
        c[1] += v(m, "skillActiveManaCost", *mli);
    }
    c
}

/// One record's global-chance group: the attributes flagged `<stem>Global`
/// (`DamageAttribute::HasGlobalChance`) go to the store's global
/// lists instead of its plain one (`DamageAttributeDur::AddToStore` and
/// siblings: `<stem>XOR` set → `AddGlobalXorAttribute`, else
/// `AddGlobalAndAttribute`). `DamageAttributeStore::
/// AddDamageToAccumulator` / `AddModifierToAccumulator`:
/// with `GetGlobalChance` (`offensiveGlobalChance`) > 0 one roll
/// `chance >= rand × 100` decides the whole group; then every "all"
/// attribute is added under `SetGlobalChance(chance)` and exactly one "one
/// of" attribute, picked by `rand × Σ chances` over their own chances in
/// list order; both skip their own chance roll (the `bool` argument).
///
/// On the character sheet (`CombatDisplayAccumulator`, whose random state
/// (`CombatDisplayAccumulator.randomState`) starts at 0, so every draw is 0): the roll always passes, the
/// "all" attributes count × the global chance (`GetChanceMultiplier`
/// returns `CombatAttributeAccumulator.globalChance` × 0.01 while it is set) and the "one of" pick is always the
/// first in load order ([`crate::attributes::DAMAGE_LOAD_ORDER`]) at its
/// own chance (the global chance is reset to 0 before the pick).
///
/// Skill / devotion / pak records keep their groups per record
/// ([`add_fields`]), items per damage store ([`item_global_groups`]); a
/// store without a global chance adds none of its global attributes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlobalGroup {
    /// `offensiveGlobalChance` (%).
    pub chance: f32,
    /// The "all" attributes' fields.
    pub all: BTreeMap<String, f32>,
    /// The "one of" attributes in load order: (own chance %, fields).
    pub one_of: Vec<(f32, BTreeMap<String, f32>)>,
    /// The store's stable source id ([`dot_source_key`] of the owner and the
    /// store): its DoTs key apart from the owner's ungated ones.
    pub source: u64,
}

/// The two weapons' own flat damage of a dual wielder (base damage, flat
/// typed damage, DoTs): `totals` holds the main weapon's.
#[derive(Debug, Clone, Default)]
pub struct HandFlats {
    pub main: BTreeMap<String, f32>,
    pub off: BTreeMap<String, f32>,
    /// Each weapon's own DoT sources ([`DotSource`]): a hand's swing deals
    /// its weapon's, not the other's (the flat fields' hand swap).
    pub main_dots: Vec<DotSource>,
    pub off_dots: Vec<DotSource>,
}

/// One owner's damage-over-time fields (`offensiveSlowX*` of the damage
/// types), kept apart from `totals` so an attack deals them as their own
/// source: the defender's `DotManager` keys slots by the owning object +
/// attribute (`DamageAttributeDur::AddDamageToAccumulator`:
/// `(GetObjectId(owner), attribute_index)`;
/// `ItemEquipment::GetOffensiveDamageAttributes` passes the
/// item's id, `Skill_Passive::CollectPassiveOffensiveDamageAttributes`
/// the skill's), so distinct owners' same-type DoTs stack and
/// only the same owner's re-application keeps max. PORT: one entry per owner
/// and damage type (the engine keeps one per attribute instance; an item's
/// base and affix DoTs of one type are summed here before the key).
#[derive(Debug, Clone, Default)]
pub struct DotSource {
    /// The owner's stable id ([`item_dot_owner`] for an item instance,
    /// the port's hash of the record path for a skill / passive / pak record).
    pub owner: u64,
    /// Its ungated DoT fields (plain names; chance-gated instances stay in
    /// `totals` and roll on their own).
    pub fields: BTreeMap<String, f32>,
}

/// A DoT's slot key: the owner id hashed with the attribute's field (the
/// engine's `(GetObjectId(owner), attribute_index)`; the port hashes with
/// [`gd_formats::collections::hash_one`], no allocation). The value is
/// process-local (never saved): only equality matters.
pub fn dot_source_key(owner: u64, field: &str) -> u64 {
    gd_formats::collections::hash_one((owner, field))
}

/// An item instance's owner id: its base, affixes and seeds (the save's
/// `Item` identity — two rolls of the same base are distinct owners), hashed
/// with the port's hasher ([`gd_formats::collections::hash_one`]).
pub fn item_dot_owner(item: &gd_formats::save::item::Item) -> u64 {
    gd_formats::collections::hash_one((
        &item.base_name,
        &item.prefix_name,
        &item.suffix_name,
        &item.modifier_name,
        &item.relic_name,
        &item.augment_name,
        &item.ascendant_name,
        item.seed,
        item.relic_seed,
        item.augment_seed,
        item.var1,
    ))
}

/// A field that is a damage-over-time value (not a debuff, not a
/// chance-gated instance).
fn is_dot_field(k: &str) -> bool {
    !k.contains('@') && crate::attributes::lookup(k).is_some_and(is_dot_def)
}

/// [`is_dot_field`] by the field's definition.
fn is_dot_def(d: &crate::attributes::FieldDef) -> bool {
    d.family == crate::attributes::AttrFamily::OffenseDuration
        && d.combat_type().is_some_and(is_dot_damage)
}

/// Adds an owner's DoT fields to `sources` (merging a repeated owner).
fn push_dot_source(sources: &mut Vec<DotSource>, owner: u64, fields: &BTreeMap<String, f32>) {
    let mut dot = BTreeMap::new();
    for (k, v) in fields {
        if is_dot_field(k) {
            *dot.entry(k.clone()).or_insert(0.0) += *v;
        }
    }
    if dot.is_empty() {
        return;
    }
    match sources.iter_mut().find(|s| s.owner == owner) {
        Some(s) => {
            for (k, v) in dot {
                *s.fields.entry(k).or_insert(0.0) += v;
            }
        }
        None => sources.push(DotSource { owner, fields: dot }),
    }
}

/// A weapon's own per-hand fields: flat (not %) hit and DoT damage.
fn is_hand_flat(key: &str) -> bool {
    use crate::attributes::{AttrFamily as F, lookup};
    lookup(key).is_some_and(|d| {
        matches!(d.family, F::Offense | F::OffenseBase | F::OffenseDuration)
            && [d.min, d.max, d.chance, d.duration_min, d.duration_max].contains(&Some(key))
    }) || key == "offensivePierceRatioMin"
}

impl Profile {
    /// An off-hand (left hand) swing: the off-hand weapon's flat damage
    /// instead of the main weapon's; `None` without a second weapon.
    /// Dual-wield attacks alternate hands (`Player::CalculateDamageValues`
    /// keeps one damage accumulator per hand): each hand collects
    /// every skill and gear offensive attribute for its own weapon id
    /// (`SkillManager::CollectAvailableOffensiveDamageAttributes`, scaled by
    /// `dwWeaponDamageFactor` = 1), so % modifiers and the other gear stay
    /// shared and only the weapon's flat damage differs.
    pub fn off_hand(&self) -> Option<Attacker<'_>> {
        Some(Attacker {
            profile: self,
            hands: Some(self.dual.as_deref()?),
            skill: None,
        })
    }
}

/// A profile's memoized derived combat parameters ([`Profile::combat_cache`]):
/// the global groups' attribute split ([`global_parts`]), the chance-gated
/// modifiers and instances of `totals` ([`chance_key`]), the defender's
/// profile-derived defenses and scalar lookups ([`defender`]), and the dense
/// per-source offense arrays ([`SLOTS`], [`SlotVals`]) the attack's
/// `collect_source` reads — gear (main / off hand), each DoT source, each global
/// group's damage store and each gated instance — so an attack rolls over floats
/// in catalog order instead of scanning the attribute families with string
/// lookups per source. `totals` stays the single raw source; these are derived
/// from it once. Built once per profile; gd-game rebuilds the profile on
/// `ProfileDirty`, so the cache never outlives its inputs. A profile must be
/// complete before the first attack reads it (the builders finish mutating it
/// first).
#[derive(Debug, Clone)]
pub struct CombatCache {
    groups: Vec<CachedGroup>,
    /// Chance-gated % modifiers (`OffenseModifier` / `OffenseDurationModifier`
    /// `field@chance` keys) in `totals` order: (field, chance, value).
    gated: Vec<(Box<str>, f32, f32)>,
    /// Chance-gated offense instances ([`chance_instances`]).
    offense_instances: Vec<ChanceInstance>,
    /// Chance-gated retaliation instances ([`chance_instances`]).
    retaliation_instances: Vec<(f32, BTreeMap<String, f32>)>,
    /// Whether any retaliation attribute is present in `totals` (a zero
    /// total still counts) or a gated instance exists: [`retaliate`]'s
    /// early-out, so a retaliation-less defender skips the catalog scan.
    has_retaliation: bool,
    defenses: Vec<Defense>,
    negative_damage_mult: f32,
    /// `defensivePercentReflectionResistance` total, uncapped ([`Profile::reflection_resist`]).
    reflection_resist: f32,
    /// Its max-resist bonus ([`crate::sheet::resistance_cap_bonus`]).
    reflection_resist_cap_bonus: f32,
    reflect: f32,
    block_modifier: f32,
    block_amount_modifier: f32,
    /// The gear source's dense offense values ([`SLOTS`]) from `totals` (the main-hand reading).
    gear: [SlotVals; OFFENSE_SLOTS],
    /// The off-hand reading (`totals` − main hand + off hand), when dual wielding.
    gear_off: Option<[SlotVals; OFFENSE_SLOTS]>,
    /// Dense values of `dot_sources`, parallel to it (the owner stays on [`DotSource`]).
    dots: Vec<[SlotVals; OFFENSE_SLOTS]>,
    /// Dense values of the dual weapons' own DoT sources (`HandFlats::main_dots` / `off_dots`).
    main_dots: Vec<[SlotVals; OFFENSE_SLOTS]>,
    off_dots: Vec<[SlotVals; OFFENSE_SLOTS]>,
}

impl CombatCache {
    /// The chance-gated retaliation instances (chance, fields), one engine attribute instance each.
    pub(crate) fn retaliation_instances(&self) -> &[(f32, BTreeMap<String, f32>)] {
        &self.retaliation_instances
    }

    /// The defender's profile-derived defenses (`build_defenses`).
    pub(crate) fn defenses(&self) -> &[Defense] {
        &self.defenses
    }
}

/// One chance-gated offense instance: its chance, its fields under their
/// plain names and its DoT owner key (attribute class + chance: the engine
/// rolls one instance per attribute per source; the port groups same-class
/// same-chance fields of the totals).
#[derive(Debug, Clone)]
struct ChanceInstance {
    chance: f32,
    fields: BTreeMap<String, f32>,
    owner: u64,
    /// Dense offense values of `fields` ([`collect_source`]).
    dense: [SlotVals; OFFENSE_SLOTS],
}

/// One [`GlobalGroup`]'s deterministic attribute split ([`cache_attrs`]).
#[derive(Debug, Clone)]
struct CachedGroup {
    all: CachedAttrs,
    one_of: Vec<CachedAttrs>,
    /// Dense damage values of `all` / each `one_of` store (play: no own chance; the sheet adds it).
    all_dmg: [SlotVals; OFFENSE_SLOTS],
    one_of_dmg: Vec<[SlotVals; OFFENSE_SLOTS]>,
}

/// One global store's offense attributes, split once per profile so an
/// attack only rolls and accumulates (no per-hit catalog scan or string
/// allocation).
#[derive(Debug, Clone, Default)]
struct CachedAttrs {
    /// Modifier fields (`OffenseModifier` / `OffenseDurationModifier`).
    mods: Vec<(&'static str, f32)>,
    /// Damage / debuff / crowd-control fields (not the chance field).
    dmg: Vec<(&'static str, f32)>,
    /// The attributes' own chance fields: the sheet inserts them at the
    /// group's weight; play reads no own chance for group attributes.
    chance: Vec<&'static str>,
}

/// [`CachedAttrs`] of one global store's fields (the `global_parts` split).
fn cache_attrs(fields: &BTreeMap<String, f32>) -> CachedAttrs {
    use crate::attributes::AttrFamily;
    let mut out = CachedAttrs::default();
    for d in offense_defs() {
        if !d.fields().any(|f| fields.contains_key(f)) {
            continue;
        }
        let modifier = matches!(
            d.family,
            AttrFamily::OffenseModifier | AttrFamily::OffenseDurationModifier
        );
        for f in d.fields() {
            let Some(v) = fields.get(f) else { continue };
            if Some(f) == d.chance {
                if !modifier {
                    out.chance.push(f);
                }
            } else if modifier {
                out.mods.push((f, *v));
            } else {
                out.dmg.push((f, *v));
            }
        }
    }
    out
}

impl Profile {
    /// The profile's memoized combat parameters ([`CombatCache`]).
    pub fn combat_cache(&self) -> &CombatCache {
        self.combat_cache.get_or_init(|| {
            use crate::attributes::AttrFamily as F;
            let t = |k: &str| self.totals.get(k).copied().unwrap_or(0.0);
            let offense_instances: Vec<ChanceInstance> = chance_instances(
                &self.totals,
                &[
                    F::OffenseBase,
                    F::Offense,
                    F::OffenseDuration,
                    F::OffenseCrowdControl,
                ],
            )
            .into_iter()
            .map(|(chance, fields)| {
                let owner = fields
                    .keys()
                    .next()
                    .and_then(|k| crate::attributes::lookup(k))
                    .map_or(0, |d| {
                        gd_formats::collections::hash_one((d.class, chance.to_bits()))
                    });
                let dense = dense_from(|k| fields.get(k).copied().unwrap_or(0.0));
                ChanceInstance {
                    chance,
                    fields,
                    owner,
                    dense,
                }
            })
            .collect();
            let retaliation_instances = chance_instances(
                &self.totals,
                &[
                    F::Retaliation,
                    F::RetaliationDuration,
                    F::RetaliationCrowdControl,
                ],
            );
            let has_retaliation = !retaliation_instances.is_empty()
                || [
                    F::Retaliation,
                    F::RetaliationDuration,
                    F::RetaliationCrowdControl,
                ]
                .into_iter()
                .any(|f| {
                    crate::attributes::family(f)
                        .any(|d| d.fields().any(|k| self.totals.contains_key(k)))
                });
            CombatCache {
                groups: self
                    .global_groups
                    .iter()
                    .map(|g| {
                        let all = cache_attrs(&g.all);
                        let one_of: Vec<CachedAttrs> =
                            g.one_of.iter().map(|(_, f)| cache_attrs(f)).collect();
                        let dmg = |pairs: &[(&'static str, f32)], k: &str| {
                            pairs.iter().find(|(f, _)| *f == k).map_or(0.0, |(_, v)| *v)
                        };
                        let all_dmg = dense_from(|k| dmg(&all.dmg, k));
                        let one_of_dmg = one_of
                            .iter()
                            .map(|a| dense_from(|k| dmg(&a.dmg, k)))
                            .collect();
                        CachedGroup {
                            all,
                            one_of,
                            all_dmg,
                            one_of_dmg,
                        }
                    })
                    .collect(),
                gated: self
                    .totals
                    .iter()
                    .filter_map(|(k, v)| {
                        let (field, chance) = k.split_once('@')?;
                        // Flat damage instances are `offense_instances`.
                        let d = crate::attributes::lookup(field)?;
                        if !matches!(d.family, F::OffenseModifier | F::OffenseDurationModifier) {
                            return None;
                        }
                        Some((Box::from(field), chance.parse::<f32>().ok()?, *v))
                    })
                    .collect(),
                offense_instances,
                retaliation_instances,
                has_retaliation,
                defenses: build_defenses(self),
                negative_damage_mult: t("negativeDamageMult"),
                reflection_resist: t("defensivePercentReflectionResistance"),
                reflection_resist_cap_bonus: crate::sheet::resistance_cap_bonus(
                    self,
                    CombatType::ReflectionResist,
                ),
                // `defensiveReflect` × (1 + `defensiveReflectModifier` %) (
                // `DefenseAttributeMisc_ReflectModifier` adds into the reflect
                // attributes' modifier); `defensiveReflectChance` not rolled.
                reflect: t("defensiveReflect") * (1.0 + t("defensiveReflectModifier") * PCT),
                block_modifier: t("defensiveBlockModifier"),
                block_amount_modifier: t("defensiveBlockAmountModifier"),
                gear: dense_from(t),
                gear_off: self.dual.as_ref().map(|h| {
                    dense_from(|k| {
                        t(k) - h.main.get(k).copied().unwrap_or(0.0)
                            + h.off.get(k).copied().unwrap_or(0.0)
                    })
                }),
                dots: self
                    .dot_sources
                    .iter()
                    .map(|ds| dense_from(|k| ds.fields.get(k).copied().unwrap_or(0.0)))
                    .collect(),
                main_dots: self
                    .dual
                    .as_ref()
                    .map(|h| {
                        h.main_dots
                            .iter()
                            .map(|ds| dense_from(|k| ds.fields.get(k).copied().unwrap_or(0.0)))
                            .collect()
                    })
                    .unwrap_or_default(),
                off_dots: self
                    .dual
                    .as_ref()
                    .map(|h| {
                        h.off_dots
                            .iter()
                            .map(|ds| dense_from(|k| ds.fields.get(k).copied().unwrap_or(0.0)))
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        })
    }

    /// `defensivePercentReflectionResistance` under the defense cap `cap` plus
    /// its max-resist bonus, at most 100: the one value `TakeAttack` scales
    /// reflected damage by ([`reflect`]) and the sheet's Reflect Resist row shows.
    pub fn reflection_resist(&self, cap: f32) -> f32 {
        let c = self.combat_cache();
        c.reflection_resist
            .min(cap + c.reflection_resist_cap_bonus)
            .min(100.0)
    }
}

/// DB damage-type name of `conversionInType` / `conversionOutType`
/// (the name lookup used by the conversion `ConversionAttribute::Load`;
/// `Poison` = acid, `Life` = vitality, `Stun` = stun; any other name, e.g.
/// `Bleeding`, disables the conversion).
pub fn conversion_type(name: &str) -> Option<CombatType> {
    use CombatType as T;
    Some(match name {
        "Physical" => T::Physical,
        "Pierce" => T::Pierce,
        "Cold" => T::Cold,
        "Fire" => T::Fire,
        "Poison" => T::Acid,
        "Lightning" => T::Lightning,
        "Life" => T::Vitality,
        "Chaos" => T::Chaos,
        "Aether" => T::Aether,
        "Elemental" => T::Elemental,
        "Stun" => T::Stun,
        _ => return None,
    })
}

/// A record's conversions at a level (`conversionInType`/`OutType`/
/// `Percentage`, and the `…2` pair).
pub fn conversions_of(rec: &Record, li: usize) -> Vec<(CombatType, CombatType, f32)> {
    let mut v = Vec::new();
    conversions_of_into(rec, li, &mut v);
    v
}

/// [`conversions_of`] appended into a caller-held buffer (PERF-36).
fn conversions_of_into(rec: &Record, li: usize, v: &mut Vec<(CombatType, CombatType, f32)>) {
    const CONVERSION_FIELDS: [[&str; 3]; 2] = [
        [
            "conversionPercentage",
            "conversionInType",
            "conversionOutType",
        ],
        [
            "conversionPercentage2",
            "conversionInType2",
            "conversionOutType2",
        ],
    ];
    for [pct, in_type, out_type] in CONVERSION_FIELDS {
        let pct = level_value(rec, pct, li).unwrap_or(0.0);
        if pct <= 0.0 {
            continue;
        }
        if let (Some(i), Some(o)) = (
            rec.str(in_type).and_then(conversion_type),
            rec.str(out_type).and_then(conversion_type),
        ) {
            v.push((i, o, pct));
        }
    }
}

/// A `%` damage modifier with its own `...ModifierChance` (e.g. a monster's
/// 35% physical at 8% chance) is its own attribute instance, rolled per
/// attack: kept as `field@chance` so summing does not merge it with
/// unconditional ones (see `gated_mods`). The same holds for chance-gated flat
/// damage, DoT, crowd control and retaliation attributes: the engine creates
/// one instance per attribute per source, each rolled on its own chance
/// (`DamageAttributeAbs::AddDamageToAccumulator`), so
/// their value fields (and the chance field itself) are kept apart too.
/// `d` is `field`'s definition ([`crate::attributes::lookup`]).
fn chance_key(
    rec: &Record,
    field: &str,
    d: Option<&crate::attributes::FieldDef>,
    level_index: usize,
) -> String {
    use crate::attributes::AttrFamily as F;
    let Some(d) = d else {
        return field.to_owned();
    };
    let own_chance = match d.family {
        F::OffenseModifier if d.modifier == Some(field) => d.chance,
        F::Offense
        | F::OffenseBase
        | F::OffenseDuration
        | F::OffenseCrowdControl
        | F::Retaliation
        | F::RetaliationDuration
        | F::RetaliationCrowdControl
            if [d.min, d.max, d.duration_min, d.duration_max, d.chance].contains(&Some(field)) =>
        {
            d.chance
        }
        _ => None,
    };
    let chance = own_chance
        .and_then(|c| level_value(rec, c, level_index))
        .filter(|c| *c > 0.0 && *c < 100.0);
    match chance {
        Some(c) => format!("{field}@{c}"),
        None => field.to_owned(),
    }
}

/// The chance-gated instances (`field@chance`, [`chance_key`]) of a totals
/// map, grouped by attribute class and chance, their fields under the plain
/// names: one group per engine attribute instance, to roll on its own.
pub(crate) fn chance_instances(
    totals: &BTreeMap<String, f32>,
    families: &[crate::attributes::AttrFamily],
) -> Vec<(f32, BTreeMap<String, f32>)> {
    let mut keys: Vec<(&'static str, f32)> = Vec::new();
    let mut out: Vec<(f32, BTreeMap<String, f32>)> = Vec::new();
    for (k, v) in totals {
        let Some((field, c)) = k.split_once('@') else {
            continue;
        };
        let Ok(c) = c.parse::<f32>() else { continue };
        let Some(d) = crate::attributes::lookup(field) else {
            continue;
        };
        if !families.contains(&d.family) {
            continue;
        }
        let i = match keys.iter().position(|(cl, ch)| *cl == d.class && *ch == c) {
            Some(i) => i,
            None => {
                keys.push((d.class, c));
                out.push((c, BTreeMap::new()));
                out.len() - 1
            }
        };
        out[i].1.insert(field.to_owned(), *v);
    }
    out
}

/// Fields summed into `Profile::totals`: combat attributes plus the skill
/// attributes (`skillCooldownReduction`, `skillManaCostReduction`, their
/// `…Modifier`s, ...), which `SkillManager::GetTotalSkillAttribute` sums
/// over gear and passives.
fn is_total_field(k: &str) -> bool {
    use crate::attributes::{AttrFamily, lookup};
    k.starts_with("offensive")
        || k.starts_with("defensive")
        || k.starts_with("retaliation")
        || lookup(k)
            .is_some_and(|d| matches!(d.family, AttrFamily::SkillValue | AttrFamily::SkillModifier))
}

/// The offense attribute definitions a store loads (table order).
fn offense_defs() -> impl Iterator<Item = &'static crate::attributes::FieldDef> {
    use crate::attributes::AttrFamily as F;
    static DEFS: LazyLock<Vec<&'static crate::attributes::FieldDef>> = LazyLock::new(|| {
        crate::catalog::FIELDS
            .iter()
            .filter(|d| {
                matches!(
                    d.family,
                    F::Offense
                        | F::OffenseBase
                        | F::OffenseModifier
                        | F::OffenseDuration
                        | F::OffenseDurationModifier
                        | F::OffenseCrowdControl
                )
            })
            .collect()
    });
    DEFS.iter().copied()
}

/// One offense attribute slot of [`SLOTS`]: a catalog definition [`collect_source`] reads, in the
/// exact family/field iteration order the per-attack rolls follow (OffenseBase, Offense,
/// OffenseDuration, OffenseCrowdControl). The dense per-source arrays ([`SlotVals`]) are indexed by
/// these slots, so an attack reads floats instead of scanning the catalog with string lookups.
#[derive(Clone, Copy)]
struct SlotDef {
    ty: CombatType,
    kind: SlotKind,
    /// [`SlotKind::Dot`]'s outcome filter (precomputed from `ty`).
    dot: DotKind,
    /// The min field (also the DoT slot key's field, [`dot_source_key`]).
    fmin: &'static str,
    max: Option<&'static str>,
    chance: Option<&'static str>,
    dmin: Option<&'static str>,
    dmax: Option<&'static str>,
    /// `offensivePhysicalMin`: weapon-style base damage, skipped by the summed sources.
    base_physical: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum SlotKind {
    Hit,
    Dot,
    Cc,
}

/// A DoT slot's filter: damage (scaled, gated by `collect_dots`), an allowed debuff, or skipped.
#[derive(Clone, Copy, PartialEq)]
enum DotKind {
    Damage,
    Debuff,
    Skip,
}

/// One slot's raw field values from a source (0 when absent): min, max, chance, duration min/max.
/// The clamps / chance filter [`collect_source`] applies are the engine's, done per attack on these.
#[derive(Clone, Copy, Default, Debug)]
struct SlotVals([f32; 5]);

const SV_MIN: usize = 0;
const SV_MAX: usize = 1;
const SV_CHANCE: usize = 2;
const SV_DMIN: usize = 3;
const SV_DMAX: usize = 4;

/// Number of [`SLOTS`] (the offense definitions with a min field and a combat type).
const OFFENSE_SLOTS: usize = offense_slot_count();

const fn offense_slot_count() -> usize {
    use crate::attributes::AttrFamily as F;
    let fams = [
        F::OffenseBase,
        F::Offense,
        F::OffenseDuration,
        F::OffenseCrowdControl,
    ];
    let mut n = 0;
    let mut fi = 0;
    while fi < fams.len() {
        let (start, end) = crate::catalog::FAMILY_RANGES[fams[fi] as usize];
        let mut i = start;
        while i < end {
            let d = &crate::catalog::FIELDS[i];
            if d.min.is_some() && d.ty >= 1 && (d.ty as usize) <= CombatType::ALL.len() {
                n += 1;
            }
            i += 1;
        }
        fi += 1;
    }
    n
}

/// The offense slots in [`collect_source`]'s iteration order (family order, then catalog order).
static SLOTS: LazyLock<Vec<SlotDef>> = LazyLock::new(|| {
    use crate::attributes::{AttrFamily as F, family};
    use CombatType as T;
    let mut v = Vec::new();
    for (fam, kind) in [
        (F::OffenseBase, SlotKind::Hit),
        (F::Offense, SlotKind::Hit),
        (F::OffenseDuration, SlotKind::Dot),
        (F::OffenseCrowdControl, SlotKind::Cc),
    ] {
        for d in family(fam) {
            let (Some(fmin), Some(ty)) = (d.min, d.combat_type()) else {
                continue;
            };
            let dot = if kind != SlotKind::Dot {
                DotKind::Skip
            } else if is_dot_damage(ty) {
                DotKind::Damage
            } else if matches!(
                ty,
                T::TotalSpeed
                    | T::AttackSpeed
                    | T::CastSpeed
                    | T::RunSpeed
                    | T::OffensiveAbility
                    | T::DefensiveAbility
                    | T::OffensiveReduction
                    | T::DefensiveReduction
                    | T::Fumble
                    | T::ProjectileFumble
                    | T::LifeLeech
                    | T::ManaLeech
                    | T::DamageMultiplier
            ) || (28..=37).contains(&(ty as u8))
            {
                DotKind::Debuff
            } else {
                DotKind::Skip
            };
            v.push(SlotDef {
                ty,
                kind,
                dot,
                fmin,
                max: d.max,
                chance: d.chance,
                dmin: d.duration_min,
                dmax: d.duration_max,
                base_physical: fmin == "offensivePhysicalMin",
            });
        }
    }
    assert_eq!(v.len(), OFFENSE_SLOTS, "offense slot count mismatch");
    v
});

/// A source's dense offense values: read each slot's fields once (at cache build, or per attack for
/// the skill source) instead of per-field string lookups in [`collect_source`].
fn dense_from(get: impl Fn(&str) -> f32) -> [SlotVals; OFFENSE_SLOTS] {
    let mut out = [SlotVals::default(); OFFENSE_SLOTS];
    for (i, s) in SLOTS.iter().enumerate() {
        out[i] = SlotVals([
            get(s.fmin),
            s.max.map_or(0.0, &get),
            s.chance.map_or(0.0, &get),
            s.dmin.map_or(0.0, &get),
            s.dmax.map_or(0.0, &get),
        ]);
    }
    out
}

/// A record's offense field values at a level index, cached per record ([`Record::derived`] slot 1)
/// so the skill source ([`skill_offense_vals`]) combines array reads, not record scans, per attack.
#[derive(Default)]
struct RecordOffenseFields(parking_lot::Mutex<HashMap<usize, Arc<[SlotVals; OFFENSE_SLOTS]>>>);

fn offense_fields(rec: &Record, li: usize) -> Arc<[SlotVals; OFFENSE_SLOTS]> {
    match rec.derived::<1, _>(RecordOffenseFields::default) {
        Some(m) => {
            m.0.lock()
                .entry(li)
                .or_insert_with(|| Arc::new(dense_from(|k| level_value(rec, k, li).unwrap_or(0.0))))
                .clone()
        }
        None => Arc::new(dense_from(|k| level_value(rec, k, li).unwrap_or(0.0))),
    }
}

/// The skill source's dense offense values: `SkillUse::f` over the cached per-record values, in the
/// same f32 order (own record × scale + the modifiers' sum, then the parent's `f`). A tooltip drops
/// the learned modifiers' values (own record × scale + the parent's `f`, `DamageAttributeStore::
/// ProcessText`).
fn skill_offense_vals(s: &SkillUse, tooltip: bool) -> [SlotVals; OFFENSE_SLOTS] {
    let rec = offense_fields(s.record, s.level_index);
    let parent = s.parent.as_ref().map(|p| skill_offense_vals(p, false));
    let mods: Vec<Arc<[SlotVals; OFFENSE_SLOTS]>> = if tooltip {
        Vec::new()
    } else {
        s.mods
            .iter()
            .map(|(r, li)| offense_fields(r, *li))
            .collect()
    };
    let mut out = [SlotVals::default(); OFFENSE_SLOTS];
    for i in 0..OFFENSE_SLOTS {
        for f in 0..5 {
            let rec_term = rec[i].0[f] * s.scale;
            let pv = parent.as_ref().map_or(0.0, |p| p[i].0[f]);
            out[i].0[f] = if tooltip {
                rec_term + pv
            } else {
                let mut ms = 0.0;
                for m in &mods {
                    ms += m[i].0[f];
                }
                (rec_term + ms) + pv
            };
        }
    }
    out
}

/// An offense attribute with a global-chance stem ([`split_global`]).
struct GlobalDef {
    def: &'static crate::attributes::FieldDef,
    /// `<stem>Global`.
    global: String,
    /// `<stem>XOR`.
    xor: String,
    /// [`crate::attributes::FieldDef::load_index`].
    load_index: usize,
}

/// The [`offense_defs`] with a global stem, in their order.
static GLOBAL_DEFS: LazyLock<Vec<GlobalDef>> = LazyLock::new(|| {
    offense_defs()
        .filter_map(|def| {
            let stem = def.global_stem()?;
            Some(GlobalDef {
                def,
                global: format!("{stem}Global"),
                xor: format!("{stem}XOR"),
                load_index: def.load_index(),
            })
        })
        .collect()
});

/// A min / max pair of one record's attribute: the engine keeps (min,
/// max(max − min, 0)) (`DamageAttributeAbs::LoadFromTable`,
/// `DamageAttributeDur::LoadFromTable` for damage and duration), so a max
/// below the min (absent) is the min. Done per record, before anything is
/// summed: a 25 flat from one source next to 1–2 from another is 26–27.
fn fill_max(fields: &mut BTreeMap<String, f32>) {
    for d in offense_defs() {
        for (lo, hi) in [(d.min, d.max), (d.duration_min, d.duration_max)] {
            let (Some(lo), Some(hi)) = (lo, hi) else {
                continue;
            };
            let min = fields.get(lo).copied().unwrap_or(0.0);
            if min > 0.0 && fields.get(hi).copied().unwrap_or(0.0) < min {
                fields.insert(hi.to_owned(), min);
            }
        }
    }
}

/// Takes a record's global-chance attributes out of its fields
/// ([`GlobalGroup`]); `None` without any.
fn split_global(
    fields: &mut BTreeMap<String, f32>,
    chance: f32,
    owner: u64,
) -> Option<GlobalGroup> {
    if !fields.keys().any(|k| k.ends_with("Global")) {
        return None;
    }
    let mut g = GlobalGroup {
        chance,
        // The record's global store keys apart from its ungated DoTs.
        source: dot_source_key(owner, "global"),
        ..Default::default()
    };
    let mut one_of: Vec<(usize, f32, BTreeMap<String, f32>)> = Vec::new();
    for gd in GLOBAL_DEFS.iter() {
        let d = gd.def;
        if fields.get(gd.global.as_str()).copied().unwrap_or(0.0) == 0.0 {
            continue;
        }
        let own: BTreeMap<String, f32> = d
            .fields()
            .filter_map(|f| fields.get(f).map(|v| (f.to_owned(), *v)))
            .collect();
        // An attribute exists when its main value does (min, or modifier).
        let main = d
            .min
            .or(d.modifier)
            .and_then(|f| own.get(f))
            .copied()
            .unwrap_or(0.0);
        if main == 0.0 {
            continue;
        }
        for f in d.fields() {
            fields.remove(f);
        }
        if fields.get(gd.xor.as_str()).copied().unwrap_or(0.0) != 0.0 {
            let c = d.chance.and_then(|f| own.get(f)).copied().unwrap_or(0.0);
            one_of.push((gd.load_index, c, own));
        } else {
            g.all.extend(own);
        }
    }
    if g.all.is_empty() && one_of.is_empty() {
        return None;
    }
    one_of.sort_by_key(|m| m.0);
    g.one_of = one_of.into_iter().map(|(_, c, f)| (c, f)).collect();
    Some(g)
}

/// The global-chance groups of an item's resolved attributes
/// (`items::resolve::Global`): one per damage store, the item's own tables
/// (base and affixes) being one store and each of its component, relic
/// bonus, augment and ascendant affix another. "One of" attributes keep the
/// store's creation order (the resolver's attribute order).
pub fn item_global_groups(attrs: &[crate::items::resolve::Attr], owner: u64) -> Vec<GlobalGroup> {
    use crate::items::resolve::Source as S;
    let store = |s: S| match s {
        S::Base | S::Prefix | S::Suffix | S::Modifier => S::Base,
        s => s,
    };
    let label = |s: S| match s {
        S::Base => "base",
        S::Relic => "relic",
        S::RelicBonus => "relicbonus",
        S::Augment => "augment",
        S::Ascendant => "ascendant",
        _ => "base",
    };
    let mut out: Vec<(S, GlobalGroup)> = Vec::new();
    for a in attrs {
        let Some(gl) = a.global else { continue };
        let key = store(a.source);
        let i = out.iter().position(|g| g.0 == key).unwrap_or_else(|| {
            out.push((
                key,
                GlobalGroup {
                    chance: gl.chance,
                    source: dot_source_key(owner, label(key)),
                    ..Default::default()
                },
            ));
            out.len() - 1
        });
        let g = &mut out[i].1;
        if gl.xor {
            // Its own chance weighs the pick.
            let own = crate::attributes::class_def(a.class)
                .and_then(|d| d.chance)
                .and_then(|c| a.get(c))
                .unwrap_or(0.0);
            g.one_of.push((own, a.values.iter().cloned().collect()));
        } else {
            for (k, v) in &a.values {
                *g.all.entry(k.clone()).or_default() += v;
            }
        }
    }
    out.into_iter()
        .map(|g| g.1)
        .filter(|g| g.chance > 0.0)
        .collect()
}

/// What [`add_fields`] adds for one record at one level index: computed once per (record, level index, name) and
/// kept on the record ([`Record::derived`]), so a profile rebuild (a monster gaining an aura, a player swapping an
/// item) adds cached sums instead of walking every field of every record again.
struct Contribution {
    /// Summed into `totals` (chance-gated fields under their `field@chance` keys).
    totals: Vec<(String, f32)>,
    group: Option<GlobalGroup>,
    owner: u64,
    /// The record's ungated DoTs (their own source, [`push_dot_source`]).
    ungated: BTreeMap<String, f32>,
}

/// A record's [`Contribution`]s by (level index, name hash).
#[derive(Default)]
struct RecordContributions(parking_lot::Mutex<HashMap<(usize, u64), Arc<Contribution>>>);

fn contribution(rec: &Record, level_index: usize, owner: u64) -> Contribution {
    let mut own: BTreeMap<String, f32> = rec
        .fields
        .iter()
        .filter(|(k, _)| is_total_field(k))
        .filter_map(|(k, _)| level_value(rec, k, level_index).map(|v| (k.to_string(), v)))
        .collect();
    fill_max(&mut own);
    // Without a global chance the global lists add nothing
    // (`DamageAttributeStore::AddDamageToAccumulator`).
    let chance = own.get("offensiveGlobalChance").copied().unwrap_or(0.0);
    let group = split_global(&mut own, chance, owner).filter(|_| chance > 0.0);
    // Each field's chance key, and whether it is one of the record's ungated DoTs, their own source (the
    // chance-gated instances stay in `totals`: `chance_instances` rolls them per attack).
    let mut ungated = BTreeMap::new();
    let mut totals = Vec::with_capacity(own.len());
    for (k, v) in own {
        let d = crate::attributes::lookup(&k);
        let key = chance_key(rec, &k, d, level_index);
        if key == k && d.is_some_and(is_dot_def) {
            ungated.insert(k, v);
        }
        totals.push((key, v));
    }
    Contribution {
        totals,
        group,
        owner,
        ungated,
    }
}

fn add_fields(
    totals: &mut BTreeMap<String, f32>,
    groups: &mut Vec<GlobalGroup>,
    dots: &mut Vec<DotSource>,
    rec: &Record,
    level_index: usize,
) {
    let owner = gd_formats::collections::hash_one(rec.name.as_str());
    let c = match rec.derived::<0, _>(RecordContributions::default) {
        Some(m) => {
            m.0.lock()
                .entry((level_index, owner))
                .or_insert_with(|| Arc::new(contribution(rec, level_index, owner)))
                .clone()
        }
        None => Arc::new(contribution(rec, level_index, owner)),
    };
    for (k, v) in &c.totals {
        match totals.get_mut(k.as_str()) {
            Some(t) => *t += v,
            None => {
                totals.insert(k.clone(), *v);
            }
        }
    }
    if let Some(g) = &c.group {
        groups.push(g.clone());
    }
    push_dot_source(dots, c.owner, &c.ungated);
}

fn finish(
    rules: &Rules,
    acc: &CharAttrAccumulator,
    level: i32,
    totals: BTreeMap<String, f32>,
    attack_speed: f32,
    is_player: bool,
) -> Profile {
    let mut sheet = CharacterSheet::compute(acc, level, &rules.formulas);
    if is_player {
        sheet.apply_player_regen(&rules.regen);
    }
    let t = |k: &str| totals.get(k).copied().unwrap_or(0.0);
    let resistances = expand_resistances(t);
    let (physical, pierce_ratio) = base_physical(t);
    Profile {
        level,
        sheet,
        physical,
        pierce_ratio,
        protection: t("defensiveProtection"),
        armor_regions: Vec::new(),
        absorption: if t("defensiveAbsorption") > 0.0 {
            t("defensiveAbsorption")
        } else {
            rules.caps.armor_absorption
        },
        resistances,
        totals,
        attack_speed: if attack_speed > 0.0 {
            attack_speed
        } else {
            1.0
        },
        speeds: [1.0; 3],
        base_speeds: [1.0; 3],
        speed_caps: [SpeedCap::default(); 3],
        is_player,
        pet: false,
        conversions: Vec::new(),
        dual: None,
        global_groups: Vec::new(),
        shield: None,
        active_costs: [0.0; 2],
        race: Vec::new(),
        racial: Vec::new(),
        req_reduction: crate::items::req_reduction::Reduction::from_acc(acc),
        charged: Vec::new(),
        dot_sources: Vec::new(),
        combat_cache: Arc::new(OnceLock::new()),
    }
}

/// The base (weapon) physical damage and pierce ratio of a totals reading:
/// `offensivePhysicalMin` / `offensivePhysicalMax` (a max below the min is
/// the min) and `offensivePierceRatioMin`.
fn base_physical(t: impl Fn(&str) -> f32) -> ((f32, f32), f32) {
    let min = t("offensivePhysicalMin");
    (
        (min, t("offensivePhysicalMax").max(min)),
        t("offensivePierceRatioMin"),
    )
}

/// Conversions of resolved item attributes (`ConversionAttribute`: the
/// percentage plus `conversionInType:<name>` / `conversionOutType:<name>`
/// markers, see `items::resolve::conversions`).
pub fn attr_conversions(
    attrs: &[crate::items::resolve::Attr],
) -> Vec<(CombatType, CombatType, f32)> {
    attrs
        .iter()
        .filter(|a| a.store == crate::items::resolve::StoreKind::Conversion)
        .filter_map(|a| {
            let pct = a
                .values
                .iter()
                .find(|(k, _)| k.starts_with("conversionPercentage"))
                .map(|v| v.1)?;
            let ty = |p: &str| {
                a.values.iter().find_map(|(k, _)| {
                    k.strip_prefix(p)
                        .and_then(|r| r.split_once(':'))
                        .and_then(|(_, n)| conversion_type(n))
                })
            };
            (pct > 0.0).then_some((ty("conversionInType")?, ty("conversionOutType")?, pct))
        })
        .collect()
}

/// A creature record's skills (`skillName1..N`) with their levels
/// (`skillLevelN`, a number or an equation in `charLevel`), as
/// `SkillManager::LoadSkills` reads them for `Monster::Load`:
/// at most 34 slots, stopping at the first empty name; the
/// level truncated to an integer. A skill at level 0 is loaded but disabled
/// (`Skill::IsSkillEnabled` needs level > 0, checked by
/// `ControllerMonster::ChooseBestSkill` and the passive collectors), so it
/// is left out here: every entry has level ≥ 1.
pub fn monster_skill_levels(rec: &Record, level: i32) -> Vec<(String, u32)> {
    let lvl = level as f32;
    (1..=34)
        .map_while(|i| {
            rec.str(&SKILL_NAME_KEYS[(i - 1) as usize])
                .filter(|n| !n.is_empty())
                .map(|n| (i, n))
        })
        .filter_map(|(i, name)| {
            let sl = crate::skills::skill_level_eq(rec, i as usize, lvl) as i32;
            (sl > 0).then(|| (name.to_owned(), sl as u32))
        })
        .collect()
}

/// `skillName1..=34` keys (a static table, no per-spawn `format!`).
static SKILL_NAME_KEYS: LazyLock<[String; 34]> =
    LazyLock::new(|| std::array::from_fn(|i| format!("skillName{}", i + 1)));

/// A monster record at `level` (bio equations + passive skills).
pub fn monster(
    db: &Database,
    rules: &Rules,
    record: &str,
    level: i32,
    side: Side,
) -> Option<Profile> {
    monster_with(db, rules, record, level, side, &[])
}

/// [`monster`] plus active buff records (a cast buff, a toggled aura's
/// `buffSkillName`) at their level index, added like its passive skills.
pub fn monster_with(
    db: &Database,
    rules: &Rules,
    record: &str,
    level: i32,
    side: Side,
    buffs: &[(Record, usize)],
) -> Option<Profile> {
    monster_full(db, rules, record, level, side, buffs, &[])
}

/// [`monster_with`] plus the items the monster wears (`Character::LoadLoot`
/// equips them, so their attributes count like a player's gear; the main
/// weapon's base damage only, as for players). Armor protects its own body
/// region, as for players (the region is the item's, set by the
/// `ArmorProtective_*` constructors, e.g. `ArmorProtective_Head` region 2;
/// region chances are `Character::GetCombatRegionChance`, shared by every
/// character).
pub fn monster_full(
    db: &Database,
    rules: &Rules,
    record: &str,
    level: i32,
    side: Side,
    buffs: &[(Record, usize)],
    items: &[crate::items::ResolvedItem],
) -> Option<Profile> {
    let rec = db.get(record)?;
    let mut bio = BioStore::from_record(rec);
    if let Some(b) = rec
        .str("characterAttributeEquations")
        .and_then(|p| db.get(p))
    {
        bio.apply_equations(b, level).ok()?;
    }
    let mut totals = BTreeMap::new();
    let mut groups = Vec::new();
    let mut dots = Vec::new();
    add_fields(&mut totals, &mut groups, &mut dots, rec, 0);
    let mut skills: Vec<(Record, usize)> = Vec::new();
    for (name, sl) in monster_skill_levels(rec, level) {
        let Some(sk) = db.get_owned(&name) else {
            continue;
        };
        if sk.class != "Skill_Passive" {
            continue;
        }
        add_fields(&mut totals, &mut groups, &mut dots, &sk, sl as usize - 1);
        skills.push((sk, sl as usize - 1));
    }
    for (b, li) in buffs {
        add_fields(&mut totals, &mut groups, &mut dots, b, *li);
        skills.push((b.clone(), *li));
    }
    let pak = match side {
        Side::Monster => rules.monster_pak.as_ref(),
        Side::Pet => rules.pet_pak.as_ref(),
    };
    if let Some(pak) = pak {
        add_fields(&mut totals, &mut groups, &mut dots, pak, rules.balance);
    }
    let challenge = if side == Side::Monster {
        rules.challenge.as_ref()
    } else {
        None
    };
    if let Some(c) = challenge {
        add_fields(
            &mut totals,
            &mut groups,
            &mut dots,
            c,
            rules.challenge_index,
        );
    }
    let area = if side == Side::Monster {
        rules.area_adjustment.as_ref()
    } else {
        None
    };
    if let Some(a) = area {
        add_fields(&mut totals, &mut groups, &mut dots, a, rules.balance);
    }
    // A pet takes the player mutators (the `Pet` vtable's `ContributeMutator*` are `Player::`'s,
    // folded with `Pet::GetPetMutators` = `Player::GetPlayerMutators`, the `affectsPlayers` ones).
    let mutators: &[Record] = if side == Side::Monster {
        &rules.mutators_monster
    } else {
        &rules.mutators_player
    };
    // `Monster::ContributeMutator*`: column = character level (`list[level - 1]`).
    let mutator_col = (level - 1).max(0) as usize;
    for m in mutators {
        add_fields(&mut totals, &mut groups, &mut dots, m, mutator_col);
    }
    // Ascendant altar monster bonuses (`GameEngine::GetAscendant*Attributes`).
    let ascendant = if side == Side::Monster {
        crate::ascendant::monster_paks(
            db,
            &rules.ascendant_paks,
            rec.str("monsterClassification").unwrap_or("Common"),
        )
    } else {
        Vec::new()
    };
    for (p, col) in &ascendant {
        add_fields(&mut totals, &mut groups, &mut dots, p, *col);
    }
    let main = items
        .iter()
        .position(|i| i.is_weapon && !i.class.contains("Shield"));
    let mut regions: BTreeMap<Region, f32> = BTreeMap::new();
    let owners: Vec<u64> = items.iter().map(|i| item_dot_owner(&i.item)).collect();
    for (n, i) in items.iter().enumerate() {
        let region = armor_region(&i.class);
        let t = i.totals();
        for (k, v) in &t {
            if Some(n) != main
                && matches!(
                    k.as_str(),
                    "offensivePhysicalMin" | "offensivePhysicalMax" | "offensivePierceRatioMin"
                )
            {
                continue;
            }
            if let Some(r) = region
                && *k == "defensiveProtection"
            {
                *regions.entry(r).or_insert(0.0) += *v;
                continue;
            }
            *totals.entry(k.clone()).or_insert(0.0) += *v;
        }
        push_dot_source(&mut dots, owners[n], &t);
    }
    let contributions = Contributions {
        skills: skills
            .iter()
            .map(|(sk, li)| {
                Box::new(move |a: &mut CharAttrAccumulator| a.add_record(sk, *li)) as Source
            })
            .collect(),
        game_balance: pak
            .map(|p| {
                Box::new(move |a: &mut CharAttrAccumulator| a.add_record(p, rules.balance))
                    as Source
            })
            .into_iter()
            .chain(challenge.map(|c| {
                Box::new(move |a: &mut CharAttrAccumulator| a.add_record(c, rules.challenge_index))
                    as Source
            }))
            .chain(area.map(|p| {
                Box::new(move |a: &mut CharAttrAccumulator| a.add_record(p, rules.balance))
                    as Source
            }))
            .chain(mutators.iter().map(|m| {
                Box::new(move |a: &mut CharAttrAccumulator| a.add_record(m, mutator_col)) as Source
            }))
            .chain(ascendant.iter().map(|(p, col)| {
                Box::new(move |a: &mut CharAttrAccumulator| a.add_record(p, *col)) as Source
            }))
            .collect(),
        equipment: items
            .iter()
            .map(|i| {
                Box::new(move |a: &mut CharAttrAccumulator| i.add_char_attributes(a)) as Source
            })
            .collect(),
        ..Default::default()
    };
    groups.extend(
        items
            .iter()
            .zip(&owners)
            .flat_map(|(i, o)| item_global_groups(&i.attrs, *o)),
    );
    let acc = accumulate(&bio, &rules.rates, &contributions);
    let mut p = finish(
        rules,
        &acc,
        level,
        totals,
        rec.f32("characterAttackSpeed").unwrap_or(1.0),
        false,
    );
    p.global_groups = groups;
    p.dot_sources = dots;
    // `CharacterBio::Update` regenerates every character: the
    // bio equation, not the raw attribute (no player gate; the player's
    // `playerresourcebehavior` equations run in `finish`).
    p.sheet.life_regen_per_sec = bio.life_regen_per_sec(level, &acc);
    let class = if matches!(rec.str("monsterClassification"), Some("Boss" | "Quest")) {
        SpeedClass::Boss
    } else {
        SpeedClass::Monster
    };
    p.set_speeds(rules, &acc, &bio.base, class, false);
    p.armor_regions = regions.into_iter().collect();
    for i in items {
        p.conversions.extend(attr_conversions(&i.attrs));
    }
    p.shield = worn_shield(db, items);
    p.race = crate::racial::races_of(rec);
    p.racial = crate::racial::of_skills(skills.iter().map(|(r, li)| (r, *li)));
    p.racial.extend(
        items
            .iter()
            .flat_map(|i| crate::racial::of_item(db, &i.item)),
    );
    p.pet = side == Side::Pet && rec.class == "Pet";
    Some(p)
}

/// Body region an armor piece protects, by its record class.
fn armor_region(class: &str) -> Option<Region> {
    match class {
        "ArmorProtective_Head" => Some(Region::Head),
        "ArmorProtective_Chest" => Some(Region::Torso),
        "ArmorProtective_Hands" => Some(Region::Arms),
        "ArmorProtective_Legs" => Some(Region::Legs),
        "ArmorProtective_Feet" => Some(Region::Feet),
        "ArmorProtective_Shoulders" => Some(Region::Shoulders),
        _ => None,
    }
}

/// The player ([`player_from_items`]): bio base values (from the save, or the
/// pc record for a new character), equipped items' resolved attribute totals
/// and item-set bonuses' character attributes (item-set pass).
/// `bio_values` are the base Physique/Cunning/Spirit/Life/Energy without any
/// spent attribute point (the save's with them split off), `allocated` every
/// point spent, the save's and this session's ([`BioStore::allocate`] adds them);
/// `skills` are learned stat skills (`skills::SkillBook::stat_sources`);
/// `item_owners` the items' DoT owner ids ([`item_dot_owner`]), one per entry
/// of `items`.
#[allow(clippy::too_many_arguments)]
fn player_inner(
    db: &Database,
    rules: &Rules,
    pc_record: &str,
    level: i32,
    bio_values: Option<[f32; 5]>,
    allocated: AllocatedPoints,
    items: &[(Option<Region>, BTreeMap<String, f32>)],
    item_char: &[&dyn Fn(&mut CharAttrAccumulator)],
    set_char: &[&dyn Fn(&mut CharAttrAccumulator)],
    skills: &[(Record, usize)],
    dual_melee: bool,
    item_owners: &[u64],
) -> Option<Profile> {
    let rec = db.get(pc_record)?;
    let mut bio = BioStore::from_record(rec);
    let original =
        [CharAttr::Physique, CharAttr::Cunning, CharAttr::Spirit].map(|a| bio.base.get(a));
    // PORT: the pc record's own `characterLifeRegen` / `characterManaRegen`
    // (1 each) do not reach the player's totals: with them the sheet reads
    // 33.59 / 296.03 where the real sheet of the same save reads 32.59 /
    // 270.22 (life 15 from the amulet alone; energy 20.2 flat at +130 %).
    bio.base.value[CharAttr::LifeRegen as usize] = 0.0;
    bio.base.value[CharAttr::EnergyRegen as usize] = 0.0;
    if let Some([p, c, s, l, e]) = bio_values {
        use crate::attributes::CharAttr::*;
        for (a, v) in [
            (Physique, p),
            (Cunning, c),
            (Spirit, s),
            (Life, l),
            (Energy, e),
        ] {
            bio.base.value[a as usize] = v;
        }
    }
    bio.allocate(&rules.levels, allocated);
    let mut totals = BTreeMap::new();
    let mut groups = Vec::new();
    let mut dots = Vec::new();
    let mut regions: BTreeMap<Region, f32> = BTreeMap::new();
    for (idx, (region, t)) in items.iter().enumerate() {
        for (k, v) in t {
            // Armor pieces protect their body region only (hit regions,
            // `combatRegion*Chance`); the rest of the protection is general.
            if let Some(r) = region
                && k == "defensiveProtection"
            {
                *regions.entry(*r).or_insert(0.0) += v;
                continue;
            }
            if is_total_field(k) {
                *totals.entry(k.clone()).or_insert(0.0) += v;
            }
        }
        let owner = item_owners[idx];
        push_dot_source(&mut dots, owner, t);
    }
    if totals.get("offensivePhysicalMax").copied().unwrap_or(0.0) <= 0.0 {
        // Unarmed: the pc record's hand damage.
        add_fields(&mut totals, &mut groups, &mut dots, rec, 0);
    }
    for (sk, li) in skills {
        add_fields(&mut totals, &mut groups, &mut dots, sk, *li);
    }
    let pak = rules.player_pak.as_ref();
    if let Some(pak) = pak {
        add_fields(&mut totals, &mut groups, &mut dots, pak, rules.balance);
    }
    // `Player::ContributeMutator*`: column = character level (`list[level - 1]`).
    let mutator_col = (level - 1).max(0) as usize;
    for m in &rules.mutators_player {
        add_fields(&mut totals, &mut groups, &mut dots, m, mutator_col);
    }
    let contributions = Contributions {
        skills: skills
            .iter()
            .map(|(sk, li)| {
                Box::new(move |a: &mut CharAttrAccumulator| a.add_record(sk, *li)) as Source
            })
            .collect(),
        item_sets: set_char
            .iter()
            .map(|f| Box::new(move |a: &mut CharAttrAccumulator| f(a)) as Source)
            .collect(),
        game_balance: pak
            .map(|p| {
                Box::new(move |a: &mut CharAttrAccumulator| a.add_record(p, rules.balance))
                    as Source
            })
            .into_iter()
            .chain(rules.mutators_player.iter().map(|m| {
                Box::new(move |a: &mut CharAttrAccumulator| a.add_record(m, mutator_col)) as Source
            }))
            .collect(),
        equipment: item_char
            .iter()
            .map(|f| Box::new(move |a: &mut CharAttrAccumulator| f(a)) as Source)
            .collect(),
    };
    let acc = accumulate(&bio, &rules.rates, &contributions);
    let mut p = finish(
        rules,
        &acc,
        level,
        totals,
        rec.f32("characterAttackSpeed").unwrap_or(1.0),
        true,
    );
    p.global_groups = groups;
    p.dot_sources = dots;
    p.set_speeds(rules, &acc, &bio.base, SpeedClass::Player, dual_melee);
    {
        use CharAttr::*;
        p.sheet.base = crate::stats::BioBase {
            attributes: [Physique, Cunning, Spirit, Life, Energy].map(|a| bio.base.get(a)),
            regen: [bio.base.get(LifeRegen), bio.base.get(EnergyRegen)],
            original,
        };
    }
    p.armor_regions = regions.into_iter().collect();
    p.race = crate::racial::races_of(rec);
    p.racial = crate::racial::of_skills(skills.iter().map(|(r, li)| (r, *li)));
    // Passive skills / devotions (`Skill_Passive::CollectPassiveConversionAttributes`).
    for (sk, li) in skills {
        p.conversions.extend(conversions_of(sk, *li));
    }
    Some(p)
}

/// [`Profile::skill_attribute`] over a totals map.
pub fn skill_attribute(totals: &BTreeMap<String, f32>, field: &str) -> f32 {
    let t = |k: &str| totals.get(k).copied().unwrap_or(0.0);
    let v = t(field);
    v + v.abs() * t(&format!("{field}Modifier")) * PCT
}

impl Profile {
    /// The character's total of a skill attribute (`skillCooldownReduction`,
    /// `skillManaCostReduction`, ...) over gear and passives: value + |value|
    /// × `<field>Modifier` % (`Player::CalculateSkillValues`
    /// and the manager part of `Skill::GetTotalSkillAttribute`).
    pub fn skill_attribute(&self, field: &str) -> f32 {
        skill_attribute(&self.totals, field)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Hit {
    pub missed: bool,
    pub critical: bool,
    /// Why it missed (`missed`).
    pub miss: Option<combat::MissReason>,
    /// The hit roll's damage multiplier (with the crit damage bonus) and
    /// critical tier (`CalculateDamageModifier`).
    pub multiplier: f32,
    pub tier: u32,
    /// The attack's style (`ParametersCombat.attackStyle`): `afflictions::receive`
    /// reads it for the sleep wake (`DurationDamageManager::BeginNewAttack`
    /// runs for every style but retaliation / debuff /
    /// reflection).
    pub kind: AttackKind,
    /// Absolute damage of the hit ([`combat::HitResult::total`]).
    pub total: f32,
    /// Instant damage applied to the defender's life, per type and in order
    /// ([`combat::HitResult::applied`]): each is one `Character::SubtractLife`
    /// (overkill included).
    pub applied: Vec<(CombatType, f32)>,
    pub damage: f32,
    /// DoTs and debuffs for the defender's duration-damage manager.
    pub dots: Vec<combat::DotDamage>,
    /// Crowd control on the defender: (type, seconds).
    pub reflex: Vec<(CombatType, f32)>,
    /// Taunt, convert and disruption on the defender ([`combat::Influence`]).
    pub influence: combat::Influence,
    /// Damage the defender reflects back at the attacker ([`reflect`]).
    pub reflected: Vec<combat::Damage>,
    /// Energy the defender lost to mana burn.
    pub energy_burned: f32,
    /// Life returned to the attacker ([`combat::HitResult::leeched`]): the
    /// leech percent of the life the hit removed, healed as a flat life
    /// bonus (`TakeAttack`'s tail → `Character::TakeBonus`).
    pub leeched: f32,
    /// Energy gained by the defender ([`combat::HitResult::energy_absorbed`]).
    pub energy_absorbed: f32,
    /// The defender's shield blocked it.
    pub blocked: bool,
    /// The defender's reflect links' share ([`combat::HitResult::link_copy`],
    /// [`attack_linked`]).
    pub link_copy: Vec<combat::Damage>,
}

impl Hit {
    /// The hit of a `take_attack` result `r` of style `kind` that removed
    /// `damage` life.
    fn from_result(r: combat::HitResult, kind: AttackKind, damage: f32) -> Self {
        Hit {
            missed: r.miss.is_some(),
            critical: r.roll.multiplier > 1.0,
            miss: r.miss,
            multiplier: r.roll.multiplier,
            tier: r.roll.tier,
            kind,
            total: r.total,
            damage,
            applied: r.applied,
            dots: r.dots,
            reflex: r.reflex,
            influence: r.influence,
            reflected: r.reflected,
            energy_burned: r.energy_burned,
            leeched: r.leeched,
            energy_absorbed: r.energy_absorbed,
            blocked: r.blocked,
            link_copy: r.link_copy,
        }
    }

    /// The damage `CombatManager::ApplyDamage` hands `GameEngine::RegisterDamage`:
    /// every applied amount, overkill included.
    pub fn applied_total(&self) -> f32 {
        self.applied.iter().map(|a| a.1).sum()
    }

    /// The largest single life subtraction (`Character::SubtractLife` hands
    /// each amount to `PlayStats::TallyDamageReceived`).
    pub fn largest_applied(&self) -> f32 {
        self.applied.iter().fold(0.0, |m, a| m.max(a.1))
    }

    /// `Character::SubtractLife`'s overkill: an amount that leaves the
    /// life (starting at `life_before`) at or below 0 and is at least `threshold`
    /// (`maxLife * gibThreshold / 100`).
    pub fn overkills(&self, life_before: f32, threshold: f32) -> bool {
        let mut life = life_before;
        self.applied.iter().any(|&(_, v)| {
            life -= v;
            life <= 0.0 && v >= threshold
        })
    }
}

/// An attack's Offensive Ability: the sheet's less the attacker's
/// `OffensiveAbility` debuff, ≥ 0 (`TakeAttack`'s `ParametersCombat` value;
/// `CombatManager::RecordHit` records it).
pub fn debuffed_offensive_ability(p: &Profile, debuffs: &Debuffs) -> f32 {
    (p.sheet.offensive_ability - debuffs.get(CombatType::OffensiveAbility)).max(0.0)
}

/// A defender's Defensive Ability: the sheet's less its `DefensiveAbility`
/// debuff, ≥ 0 (`DesignerCalculateDefensiveAbility`, as `TakeAttack` and
/// `RecordHit` use it).
pub fn debuffed_defensive_ability(p: &Profile, debuffs: &Debuffs) -> f32 {
    (p.sheet.defensive_ability - debuffs.get(CombatType::DefensiveAbility)).max(0.0)
}

/// Debuffs active on a character (`DurationDamageManager` fixed damage:
/// the largest active value per type, `GetFixedDamage`).
#[derive(Debug, Clone, Default)]
pub struct Debuffs(pub Vec<(CombatType, f32)>);

impl Debuffs {
    pub fn get(&self, ty: CombatType) -> f32 {
        self.0
            .iter()
            .filter(|d| d.0 == ty)
            .map(|d| d.1)
            .fold(0.0, f32::max)
    }
}

/// A skill's damage for [`attack_linked`]: its record at a level index.
pub struct SkillUse<'a> {
    pub record: &'a Record,
    pub level_index: usize,
    /// Learned modifiers of the skill (record, level index): their fields add
    /// to the skill's own, `weaponDamagePct` included
    /// (`Skill::CollectLocalOffensiveDamageAttributes` →
    /// `AddModifierOffensiveDamageAttributes`, `CollectLocalWeaponDamage`).
    pub mods: Vec<(&'a Record, usize)>,
    /// The weapon-pool skill a `Skill_WPAttack` runs under (Fire Strike,
    /// Cadence…, see `weapon_pool`): its damage (with its modifiers) adds,
    /// its weapon % multiplies (`Skill_WPAttack::CollectLocalWeaponDamage`).
    pub parent: Option<Box<SkillUse<'a>>>,
    /// Scale of the record's own fields (not its modifiers'): Righteous
    /// Fervor's charge multiplier (`Skill_WeaponPool_ChargedScaling::
    /// CollectLocalOffensiveDamageAttributes` / `CollectLocalWeaponDamage`);
    /// 1 otherwise.
    pub scale: f32,
    /// Scale of the whole hit's damage: a projectile burst / ring's range
    /// falloff ([`range_scale`]); 1 otherwise.
    pub damage_scale: f32,
    /// `TakeAttack` combat style of the hit (`ParametersCombat.combatStyle`, as the
    /// skill's `TargetResult` passes it to `CollectCombatParameters`:
    /// weapon attacks melee, projectiles ranged, radius / spell skills style
    /// 0 = [`AttackKind::Direct`] for `Skill_AttackRadius`,
    /// `…RadiusLightning`, `…LineFan`, `…PathCharge`, the projectile
    /// classes).
    pub style: AttackKind,
    /// The combo charge bonus of this use, (100 + multiplier) % (the
    /// `DamageScaleInfo.comboFactor`, applied after the % modifiers:
    /// `gd_sim::combo`); 0 = none.
    pub combo: f32,
    /// The attacker's worn weapons (right, left), when known: a modifier's
    /// `weaponDamagePct` counts only if they suit the modifier itself.
    pub hands: Option<(
        Option<crate::equip::WeaponType>,
        Option<crate::equip::WeaponType>,
    )>,
    /// An item skill (`Skill.isItemSkill`: `SkillManager::AddItemSkills` / `Skill::SetAsItemSkill`, the skills worn
    /// gear grants): `Skill::GetManaCost` takes its cost unreduced ([`skill_energy_cost`]).
    pub item_skill: bool,
}

/// `projectileDamageRange<n>Min/Max/Scale` (n = 1..3) of
/// `Skill_AttackProjectileBurst` / `Skill_AttackProjectileRing` /
/// `SkillSecondary_AttackProjectileRing`: a hit whose projectile travelled
/// a distance in (min, max] deals scale % of its damage (every damage
/// attribute's `ScaleDamage`, the multiplier excepted; the first matching
/// band with a non-zero scale) `Load` / `TargetResult`. None when
/// no band applies.
pub fn range_scale(rec: &Record, distance: f32) -> Option<f32> {
    use crate::items::db::Fields;
    if !matches!(
        rec.class.as_str(),
        "Skill_AttackProjectileBurst"
            | "Skill_AttackProjectileRing"
            | "SkillSecondary_AttackProjectileRing"
    ) {
        return None;
    }
    (1..=3).find_map(|n| {
        let (lo, hi) = (
            rec.float(&format!("projectileDamageRange{n}Min")),
            rec.float(&format!("projectileDamageRange{n}Max")),
        );
        let scale = rec.int(&format!("projectileDamageRange{n}Scale"));
        ((lo != 0.0 || hi != 0.0) && distance > lo && distance <= hi && scale != 0)
            .then_some(scale as f32 * 0.01)
    })
}

/// The energy cost of a skill (`Skill::GetManaCost`):
/// `skillManaCost` of the record and its learned modifiers,
/// × (1 − reduction %), floored at 0. The reduction is the skill's own
/// `skillManaCostReduction` (+ |value| × `…Modifier` %) plus the character's
/// total (`GetTotalSkillAttribute`). An item skill (`item_skill`)
/// or a potion (`Skill_ChargePotion`) pays unreduced. `roll(chance)` decides
/// each chance-gated reduction instance (`SkillAttribute::GetTotalValue`
/// rolls a chance inside (0, 100) per call); pass `|_| false`
/// for the plain value the hot slot's usable state shows (it asks without
/// the chance part). The one cost function of the cast request and the
/// energy absorption (`ParametersCombat.manaCost`).
pub fn skill_energy_cost(
    rec: &Record,
    level_index: usize,
    mods: &[(&Record, usize)],
    item_skill: bool,
    char_reduction: f32,
    mut roll: impl FnMut(f32) -> bool,
) -> f32 {
    let cost: f32 = std::iter::once((rec, level_index))
        .chain(mods.iter().copied())
        .map(|(r, li)| level_value(r, "skillManaCost", li).unwrap_or(0.0))
        .sum();
    if item_skill || rec.class == "Skill_ChargePotion" {
        return cost;
    }
    let mut rolled = |k: &str| -> f32 {
        std::iter::once((rec, level_index))
            .chain(mods.iter().copied())
            .map(|(r, li)| {
                let c = level_value(r, &format!("{k}Chance"), li).unwrap_or(0.0);
                if c > 0.0 && c < 100.0 && !roll(c) {
                    0.0
                } else {
                    level_value(r, k, li).unwrap_or(0.0)
                }
            })
            .sum()
    };
    let v = rolled("skillManaCostReduction");
    let cut = v + v.abs() * rolled("skillManaCostReductionModifier") * PCT + char_reduction;
    (cost * (1.0 - cut * PCT)).max(0.0)
}

impl<'a> SkillUse<'a> {
    pub fn new(record: &'a Record, level_index: usize) -> Self {
        Self {
            record,
            level_index,
            mods: Vec::new(),
            parent: None,
            scale: 1.0,
            damage_scale: 1.0,
            style: AttackKind::Melee,
            combo: 0.0,
            hands: None,
            item_skill: false,
        }
    }
    fn f(&self, k: &str) -> f32 {
        self.own(k) + self.parent.as_ref().map_or(0.0, |p| p.f(k))
    }
    fn own(&self, k: &str) -> f32 {
        level_value(self.record, k, self.level_index).unwrap_or(0.0) * self.scale
            + self
                .mods
                .iter()
                .map(|(r, li)| level_value(r, k, *li).unwrap_or(0.0))
                .sum::<f32>()
    }
    /// [`skill_energy_cost`] of this use: its record and learned modifiers
    /// (the weapon-pool parent's cost is not paid again).
    pub fn energy_cost(&self, char_reduction: f32, roll: impl FnMut(f32) -> bool) -> f32 {
        skill_energy_cost(
            self.record,
            self.level_index,
            &self.mods,
            self.item_skill,
            char_reduction,
            roll,
        )
    }
    /// `weaponDamagePct`: share of the attacker's weapon (and gear flat)
    /// damage the skill deals. A record without the field has 0 (
    /// `SkillProfile::GetWeaponDamagePct` / `Skill::CollectLocalWeaponDamage`,
    /// for `Skill_AttackWeapon*` and `Skill_WPAttack*` too); a 0 parent
    /// leaves the child's % unchanged, a non-zero one multiplies it
    /// (`Skill_WPAttack::CollectLocalWeaponDamage`). Port stand-in:
    /// a `Skill_WeaponPool_*` used as the hit's own record counts 100 (the
    /// game's default attack it rides on has its own %).
    pub fn weapon_pct(&self) -> f32 {
        // Modifiers add theirs × `GetPotionScale` (1 outside potions) only
        // when the weapons suit the modifier (`Skill::
        // CollectWeaponDamageFromModifiers`: its `QualifyingWeapon` and
        // `QualifyingHandState`).
        let k = "weaponDamagePct";
        let v = level_value(self.record, k, self.level_index).unwrap_or(0.0) * self.scale
            + self
                .mods
                .iter()
                .filter(|(r, _)| {
                    self.hands
                        .is_none_or(|(rh, lh)| crate::weapon_pool::qualifies(r, None, rh, lh))
                })
                .map(|(r, li)| level_value(r, k, *li).unwrap_or(0.0))
                .sum::<f32>();
        let own = if v > 0.0 {
            v
        } else if self.record.class.starts_with("Skill_WeaponPool") {
            100.0
        } else {
            0.0
        };
        // A parent without a weapon % leaves it unchanged.
        let parent = self
            .parent
            .as_ref()
            .map(|p| p.weapon_pct())
            .filter(|p| *p > 0.0)
            .map_or(1.0, |p| p * PCT);
        own * parent
    }
    /// `retaliationDamagePct` of the skill at its level plus its modifiers'
    /// at theirs (`Skill::CollectLocalRetaliationDamagePercent` →
    /// `CollectRetaliationDamagePctFromModifiers`).
    /// `Skill::CollectCombatParameters` (Ashes of Malmouth on):
    /// above 0 the attack's retaliation accumulator gets the
    /// skill's own retaliation attributes and then the character's
    /// (`SkillManager::CollectAvailableRetaliationAttributes`: equipment,
    /// sets, skills but the attacking one), every value × % / 100, and the
    /// character's retaliation modifiers; that part goes through the attack's
    /// conversions and `Process` (no attribute scaling, the racial / combo
    /// `DamageScaleInfo`) and joins the attack's damage, so it lands with the
    /// attack's hit roll and the defender's defences. The modifiers: see
    /// [`retaliation_damage`].
    pub fn retaliation_pct(&self) -> f32 {
        let k = "retaliationDamagePct";
        level_value(self.record, k, self.level_index).unwrap_or(0.0)
            + self
                .mods
                .iter()
                .map(|(r, li)| level_value(r, k, *li).unwrap_or(0.0))
                .sum::<f32>()
    }
    /// `offensiveDamageMultModifier` of the skill, its modifiers and its
    /// parent: one multiplier per record, max(1 + %, 0) each (see
    /// `collect_damage`).
    fn damage_mult(&self) -> f32 {
        let k = "offensiveDamageMultModifier";
        let f = |v: f32| (1.0 + v * PCT).max(0.0);
        let own = f(level_value(self.record, k, self.level_index).unwrap_or(0.0) * self.scale);
        let mods = self
            .mods
            .iter()
            .map(|(r, li)| f(level_value(r, k, *li).unwrap_or(0.0)))
            .product::<f32>();
        own * mods * self.parent.as_ref().map_or(1.0, |p| p.damage_mult())
    }
    /// Damage conversions of the skill, its modifiers and its parent
    /// (`Skill_WPAttack::CollectLocalConversionAttributes`: own,
    /// `AddModifierConversionAttributes`, then the parent's).
    fn conversions(&self) -> Vec<(CombatType, CombatType, f32)> {
        let mut v = Vec::new();
        self.conversions_into(&mut v);
        v
    }
    /// [`conversions`](Self::conversions) appended into a caller-held buffer
    /// (the attack path's scratch, PERF-36), in the same order.
    fn conversions_into(&self, out: &mut Vec<(CombatType, CombatType, f32)>) {
        conversions_of_into(self.record, self.level_index, out);
        for (r, li) in &self.mods {
            conversions_of_into(r, *li, out);
        }
        if let Some(p) = &self.parent {
            p.conversions_into(out);
        }
    }
}

/// The global groups' modifier fields summed for one attack: (field, value),
/// first-seen order, one entry per field (a small linear list — PERF-36 reuses
/// its buffer; lookups return the same sum the `BTreeMap` held).
type GroupMods = Vec<(&'static str, f32)>;

/// One global group's damage source for [`collect_source`]: the profile's cached
/// dense values in play (indices into [`CombatCache`]'s groups, so the buffer is
/// scratch-reusable), a chance-weighted copy on the sheet (its own chance fields
/// set to the store's weight).
enum GroupDmg {
    Cached { group: usize, pick: Option<usize> },
    Display(Box<[SlotVals; OFFENSE_SLOTS]>),
}

/// Reusable per-attack buffers (PERF-36): the attack path's small collections
/// (the damage list, the global groups' modifier sums and damage sources, the
/// chance-gated modifiers, the skill's conversions, the defender's absorb copy
/// and the conversion buffers) are collected here instead of fresh heap
/// allocations per hit. One per thread, taken empty and returned cleared (the
/// `gd_game_core::rng` pattern), so a nested or parallel attack never shares
/// buffers and no gameplay value survives an attack — only capacity is reused.
/// `totals` / [`CombatCache`] stay the raw source; this caches nothing.
/// PORT: the engine allocates these per `CollectCombatParameters` / `TakeAttack`.
#[derive(Default)]
struct AttackScratch {
    damage: Vec<combat::Damage>,
    group_mods: GroupMods,
    gated: Vec<(usize, f32)>,
    group_damage: Vec<(u64, GroupDmg)>,
    convs: Vec<combat::Conversion>,
    absorb: Vec<combat::Absorb>,
    convert: combat::ConvertScratch,
}

thread_local! {
    static ATTACK_SCRATCH: RefCell<AttackScratch> = RefCell::new(AttackScratch::default());
}

/// Takes this thread's attack scratch, leaving an empty one (a nested call gets
/// fresh buffers and returns them; the outer call restores its own).
fn take_scratch() -> AttackScratch {
    ATTACK_SCRATCH.with(|c| c.replace(AttackScratch::default()))
}

fn put_scratch(s: AttackScratch) {
    ATTACK_SCRATCH.with(|c| {
        c.replace(s);
    });
}

/// Global-chance groups ([`GlobalGroup`]): the attributes that apply to
/// this attack. Modifiers join the totals (× their chance weight on the
/// sheet); damage attributes are sources of their own (their chance field
/// set to the weight the sheet gives them, or removed: no own roll), each
/// with its store's source id. The attribute split is the profile's
/// [`CombatCache`]; only the rolls run per attack.
fn global_parts(
    atk: &Profile,
    rng: &mut Rng,
    display: bool,
    group_mods: &mut GroupMods,
    group_damage: &mut Vec<(u64, GroupDmg)>,
) {
    let chance_weight = |c: f32| if c > 0.0 && c < 100.0 { c * 0.01 } else { 1.0 };
    group_mods.clear();
    group_damage.clear();
    for (gi, (g, cg)) in atk
        .global_groups
        .iter()
        .zip(&atk.combat_cache().groups)
        .enumerate()
    {
        if !display && g.chance < rng.unit() * 100.0 {
            continue;
        }
        let pick = if g.one_of.is_empty() {
            None
        } else if display {
            Some(0)
        } else {
            let target = rng.unit() * g.one_of.iter().map(|m| m.0).sum::<f32>();
            let mut acc = 0.0;
            g.one_of.iter().position(|m| {
                acc += m.0;
                target <= acc
            })
        };
        let picked = pick.map(|i| {
            (
                &cg.one_of[i],
                Some(g.one_of[i].0),
                &cg.one_of_dmg[i],
                Some(i),
            )
        });
        for (attrs, own, play_dense, pick_idx) in
            std::iter::once((&cg.all, None, &cg.all_dmg, None)).chain(picked)
        {
            // Sheet weight: the global chance for "all", the own chance for the pick.
            let weight = own.unwrap_or(g.chance);
            let w = if display { chance_weight(weight) } else { 1.0 };
            for &(f, v) in &attrs.mods {
                let idx = match group_mods.iter().position(|x| x.0 == f) {
                    Some(i) => i,
                    None => {
                        group_mods.push((f, 0.0));
                        group_mods.len() - 1
                    }
                };
                group_mods[idx].1 += v * w;
            }
            if display {
                let mut dense = *play_dense;
                for (i, s) in SLOTS.iter().enumerate() {
                    if s.chance.is_some_and(|cf| attrs.chance.contains(&cf)) {
                        dense[i].0[SV_CHANCE] = weight;
                    }
                }
                if !attrs.dmg.is_empty() || !attrs.chance.is_empty() {
                    group_damage.push((g.source, GroupDmg::Display(Box::new(dense))));
                }
            } else if !attrs.dmg.is_empty() {
                group_damage.push((
                    g.source,
                    GroupDmg::Cached {
                        group: gi,
                        pick: pick_idx,
                    },
                ));
            }
        }
    }
}

/// Chance-gated modifiers (`field@chance`, see `chance_key`) that apply to
/// this attack (on the sheet: all, × chance / 100). The candidates are the
/// profile's [`CombatCache`]; only the rolls run per attack. Flat damage
/// instances roll in `collect_damage` / `retaliation_parts`, not here.
fn gated_mods(atk: &Profile, rng: &mut Rng, display: bool, out: &mut Vec<(usize, f32)>) {
    let chance_weight = |c: f32| if c > 0.0 && c < 100.0 { c * 0.01 } else { 1.0 };
    out.clear();
    for (i, (_, chance, v)) in atk.combat_cache().gated.iter().enumerate() {
        if display {
            out.push((i, *v * chance_weight(*chance)));
        } else if rng.unit() * 100.0 < *chance {
            out.push((i, *v));
        }
    }
}

/// One [`CombatType`]'s modifier fields from the catalog, built once
/// ([`type_fields`]) so the per-damage modifier sums index a table instead of
/// scanning the attribute families.
#[derive(Default)]
struct TypeFields {
    /// [`hit_mod`]: the `OffenseModifier` fields of the type (and, for cold /
    /// fire / lightning, `Elemental`'s), table order.
    hit: Vec<&'static str>,
    /// [`dot_mod`]: the `OffenseDurationModifier` (modifier, duration
    /// modifier) fields of the type.
    dot: Vec<(Option<&'static str>, Option<&'static str>)>,
    /// [`retaliation_modifier_of`]: `RetaliationModifier` modifier fields.
    ret_hit: Vec<&'static str>,
    /// [`retaliation_modifier_of`] (DoT): `RetaliationDurationModifier`
    /// modifier fields.
    ret_dot: Vec<&'static str>,
    /// [`modify_retaliation`]: `RetaliationDurationModifier` duration
    /// modifier fields.
    ret_dot_duration: Vec<&'static str>,
}

/// The [`TypeFields`] of `ty`.
fn type_fields(ty: CombatType) -> &'static TypeFields {
    static TABLE: LazyLock<Vec<TypeFields>> = LazyLock::new(|| {
        use crate::attributes::{AttrFamily as F, family};
        (0..=CombatType::ALL.len())
            .map(|id| {
                let Some(ty) = CombatType::from_id(id as u8) else {
                    return TypeFields::default();
                };
                let of = |d: &&crate::attributes::FieldDef| d.combat_type() == Some(ty);
                TypeFields {
                    hit: family(F::OffenseModifier)
                        .filter(|d| {
                            of(d)
                                || (ty.is_elemental()
                                    && d.combat_type() == Some(CombatType::Elemental))
                        })
                        .filter_map(|d| d.value.or(d.modifier))
                        .collect(),
                    dot: family(F::OffenseDurationModifier)
                        .filter(of)
                        .map(|d| (d.modifier, d.duration_modifier))
                        .collect(),
                    ret_hit: family(F::RetaliationModifier)
                        .filter(of)
                        .filter_map(|d| d.modifier)
                        .collect(),
                    ret_dot: family(F::RetaliationDurationModifier)
                        .filter(of)
                        .filter_map(|d| d.modifier)
                        .collect(),
                    ret_dot_duration: family(F::RetaliationDurationModifier)
                        .filter(of)
                        .filter_map(|d| d.duration_modifier)
                        .collect(),
                }
            })
            .collect()
    });
    &TABLE[ty as usize]
}

/// A hit type's % modifier from an attribute source.
/// `offensiveElementalModifier` adds its % as three modifiers, cold, fire
/// and lightning (`DamageAttributeAbsMod_Elemental::AddModifierToAccumulator`),
/// so it raises every cold / fire / lightning hit
/// (the thirds of an Elemental damage too). `offensiveTotalDamageModifier`
/// is added to each hit type's modifier and each DoT type's
/// (`DamageAttributeAbsMod_TotalDamageModifier::AddModifierToAccumulator`).
fn hit_mod(get: &dyn Fn(&str) -> f32, ty: CombatType) -> f32 {
    use CombatType as T;
    let total = if matches!(
        ty,
        T::Physical
            | T::Pierce
            | T::Cold
            | T::Fire
            | T::Acid
            | T::Lightning
            | T::Vitality
            | T::Chaos
            | T::Aether
    ) {
        get("offensiveTotalDamageModifier")
    } else {
        0.0
    };
    total + type_fields(ty).hit.iter().map(|&f| get(f)).sum::<f32>()
}

/// A DoT type's (value %, duration %) from an attribute source.
fn dot_mod(get: &dyn Fn(&str) -> f32, ty: CombatType) -> (f32, f32) {
    use CombatType as T;
    let mut m = if matches!(
        ty,
        T::Physical
            | T::Cold
            | T::Fire
            | T::Acid
            | T::Lightning
            | T::Vitality
            | T::Chaos
            | T::Aether
            | T::Bleeding
    ) {
        get("offensiveTotalDamageModifier")
    } else {
        0.0
    };
    let mut dm = 0.0;
    for &(modifier, duration) in &type_fields(ty).dot {
        m += modifier.map_or(0.0, get);
        dm += duration.map_or(0.0, get);
    }
    (m, dm)
}

/// The character's own damage modifiers as its attacks get them on the
/// sheet's display accumulator (no skill): a hit type's %, a DoT type's
/// value % and duration % (`Player::CollectDamageModifiers` /
/// `CollectDamageDurationModifiers` → `CombatAttributeAccumulator::
/// GetTotalDamageModifierType` (type, hit / DoT) / `GetTotalDurationModifierType`,
/// over the accumulator `CalculateDamageModifiers` fills).
pub struct DisplayModifiers<'a> {
    profile: &'a Profile,
    group_mods: GroupMods,
    gated: Vec<(usize, f32)>,
}

impl<'a> DisplayModifiers<'a> {
    pub fn new(atk: &'a Profile) -> Self {
        let mut rng = Rng(0);
        let mut group_mods = GroupMods::default();
        let mut group_damage = Vec::new();
        global_parts(atk, &mut rng, true, &mut group_mods, &mut group_damage);
        let mut gated = Vec::new();
        gated_mods(atk, &mut rng, true, &mut gated);
        DisplayModifiers {
            profile: atk,
            group_mods,
            gated,
        }
    }
    fn get(&self, k: &str) -> f32 {
        let gated = &self.profile.combat_cache().gated;
        self.profile.totals.get(k).copied().unwrap_or(0.0)
            + self
                .group_mods
                .iter()
                .find(|x| x.0 == k)
                .map_or(0.0, |x| x.1)
            + self
                .gated
                .iter()
                .filter(|&&(i, _)| &*gated[i].0 == k)
                .map(|&(_, v)| v)
                .sum::<f32>()
    }
    pub fn hit(&self, ty: CombatType) -> f32 {
        hit_mod(&|k| self.get(k), ty)
    }
    /// (value %, duration %).
    pub fn dot(&self, ty: CombatType) -> (f32, f32) {
        dot_mod(&|k| self.get(k), ty)
    }
}

/// The processed damage list of one attack ([`attack_linked`] up to the defender
/// side), its mana-burn ratio and its crit damage bonus (fraction); also
/// what the character sheet's DPS reads
/// (`Skill::CollectCombatParameters`, [`display_damage`]).
///
/// `display` is the character sheet's accumulator (`CombatDisplayAccumulator`
/// with procs included `GetChanceMultiplier`): nothing
/// is rolled against its chance; an attribute or modifier with a chance
/// strictly between 0 and 100 counts × chance / 100, every other one in full.
/// `tooltip`: a skill tooltip's store text (`DamageAttributeStore::
/// ProcessText`): only the skill's full (≥ 100 %) conversions turn its
/// attributes, the character's conversions do not apply.
/// `racial_pct`: the attack's racial bonus against its target
/// ([`racial_damage`]; 0 on the sheet and tooltips).
#[allow(clippy::too_many_arguments)]
fn collect_damage(
    rules: &Rules,
    atk: Attacker<'_>,
    skill: &Option<SkillUse>,
    rng: &mut Rng,
    atk_debuffs: &Debuffs,
    display: bool,
    tooltip: bool,
    racial_pct: f32,
) -> (Vec<combat::Damage>, f32, f32) {
    let mut sc = take_scratch();
    let cache = atk.profile.combat_cache();
    let chance_weight = |c: f32| if c > 0.0 && c < 100.0 { c * 0.01 } else { 1.0 };
    use CombatType as T;
    let ad = |ty| atk_debuffs.get(ty);
    let out_red = outgoing_reduction(atk_debuffs);
    global_parts(
        atk.profile,
        rng,
        display,
        &mut sc.group_mods,
        &mut sc.group_damage,
    );
    let t = |k: &str| atk.total(k) + sc.group_mods.iter().find(|x| x.0 == k).map_or(0.0, |x| x.1);
    // Damage multipliers (`offensiveDamageMultModifier`, e.g. a gear skill
    // modifier's -30 %) scale the damage after the other modifiers
    // (`CombatAttributeAccumulator::ModifyDamage` applies type 61 last;
    // `CombatAttribute_DamageMultiplier::Execute` covers every damage type
    // but pierce ratio, mana burn, disruption and % current life).
    // Every source adds its own multiplier attribute, applied one after the
    // other as max(1 + %, 0) (`DamageAttributeAbsMod_DamageMultiplier::
    // AddModifierToAccumulator`, `ModifyDamage`); the profile's
    // gear total counts as one source here (its sources are summed).
    let mult = (1.0 + t("offensiveDamageMultModifier") * PCT).max(0.0)
        * skill.as_ref().map_or(1.0, |s| s.damage_mult());
    let total_mod = mult * skill.as_ref().map_or(1.0, |s| s.damage_scale.max(0.0));
    let weapon_scale = skill.as_ref().map_or(1.0, |s| s.weapon_pct() * PCT);
    let vars = CombatVars {
        strength: atk.profile.sheet.physique,
        dexterity: atk.profile.sheet.cunning,
        intelligence: atk.profile.sheet.spirit,
        character_level: atk.profile.level as f32,
        max_life: atk.profile.sheet.life,
        ..Default::default()
    };
    let scaling = AttackerScaling {
        formulas: &rules.formulas,
        vars,
    };
    gated_mods(atk.profile, rng, display, &mut sc.gated);
    // The skill's own modifiers (its record, its modifiers, its parent) and
    // the character's: `Skill::CollectCombatParameters` applies the skill's
    // (`ModifyDamage`) before the skill's and the character's conversions,
    // the character's after them, so only the skill's travel with converted
    // damage (`combat::convert_damage`).
    let local = |f: &str| skill.as_ref().map_or(0.0, |s| s.f(f));
    let player = |f: &str| {
        t(f) + sc
            .gated
            .iter()
            .filter(|&&(i, _)| &*cache.gated[i].0 == f)
            .map(|&(_, v)| v)
            .sum::<f32>()
    };
    let modifier = |ty: CombatType| hit_mod(&local, ty) + hit_mod(&player, ty);
    sc.damage.clear();
    let damage = &mut sc.damage;
    let (physical, pierce_ratio) = atk.base_physical();
    if weapon_scale > 0.0 && physical.1 > 0.0 {
        let (min, max) = (
            physical.0 * total_mod * weapon_scale,
            physical.1 * total_mod * weapon_scale,
        );
        damage.push(combat::Damage::Weapon(WeaponDamage::roll(
            min,
            max,
            pierce_ratio,
            1.0,
            rng,
        )));
    }
    // Typed hit damage: weapon-like base (`offensiveBaseXMin/Max`, e.g. an
    // Aether dagger) and flat bonuses (`offensiveXMin/Max`), each with its
    // `offensiveXModifier` % and optional `offensiveXChance`; the attacker's
    // scaled by the weapon %, the skill's at full value.
    let skill_owner = skill.as_ref().map_or(0, |s| {
        gd_formats::collections::hash_one(s.record.name.as_str())
    });
    // One source's offense attributes, read from its dense [`SlotVals`] in [`SLOTS`] order — the
    // catalog family/field order the per-attack rolls follow (OffenseBase + Offense hits, then
    // OffenseDuration DoTs / debuffs, then OffenseCrowdControl), so the interleaved `rng.unit()`
    // draws and the f32 operations are exactly the catalog scan's. `instance_weight`: a chance-gated
    // instance already rolled (or weighted, on the sheet); `None` rolls each attribute's own chance.
    // `owner`: the source's DoT slot key base ([`dot_source_key`]); `collect_dots`: false for the
    // summed gear source in play (its DoTs come per owner from `dot_sources` below; the sheet keeps
    // the sum).
    let collect_source = |vals: &[SlotVals; OFFENSE_SLOTS],
                          scale: f32,
                          skip_base_physical: bool,
                          instance_weight: Option<f32>,
                          rng: &mut Rng,
                          damage: &mut Vec<combat::Damage>,
                          owner: u64,
                          collect_dots: bool| {
        if scale <= 0.0 {
            return;
        }
        for (s, v) in SLOTS.iter().zip(vals.iter()) {
            // Base physical (`offensivePhysicalMin/Max`) is weapon-style damage: the attacker's is
            // dealt above, a chance-gated instance's below; the summed sources skip it (no roll).
            if s.kind == SlotKind::Hit && skip_base_physical && s.base_physical {
                continue;
            }
            let min = v.0[SV_MIN];
            if min <= 0.0 {
                continue;
            }
            let ty = s.ty;
            let max = if v.0[SV_MAX] >= min { v.0[SV_MAX] } else { min };
            let mut weight = 1.0;
            if let Some(w) = instance_weight {
                weight = w;
            } else if v.0[SV_CHANCE] > 0.0 {
                let c = v.0[SV_CHANCE];
                if s.kind == SlotKind::Cc {
                    // Crowd control rolls its chance even on the sheet (no display weight).
                    if rng.unit() * 100.0 > c {
                        continue;
                    }
                } else if display {
                    weight = chance_weight(c);
                } else if rng.unit() * 100.0 > c {
                    continue;
                }
            }
            match s.kind {
                SlotKind::Hit => {
                    let k = total_mod * scale * weight;
                    match ty {
                        // Life leech: % of the life removed, healed on the attacker.
                        // `DamageAttributeAbs_LifeLeech::AddDamageToAccumulator`: the weapon % is
                        // capped at 1; `Process` takes the outgoing reduction, no attribute scaling,
                        // no racial / combo scale; the type-61 multiplier applies.
                        T::LifeLeech => {
                            let k = total_mod * scale.min(1.0) * weight;
                            let red = |x: f32| {
                                (x - (x.abs() * out_red.total_pct * PCT + out_red.total_abs))
                                    .max(0.0)
                            };
                            damage.push(combat::Damage::LifeLeech {
                                value: red(min * k + (max - min) * k * rng.unit()),
                                min: red(min * k),
                                max: red(max * k),
                            });
                        }
                        // % current life: the skill's scale applies uncapped, no type-61 multiplier
                        // and no reduction (`..._PercentLife::Process`).
                        T::PercentCurrentLife => {
                            let k = skill.as_ref().map_or(1.0, |s| s.damage_scale.max(0.0))
                                * scale
                                * weight;
                            damage.push(combat::Damage::PercentCurrentLife {
                                value: min * k + (max - min) * k * rng.unit(),
                                min: min * k,
                                max: max * k,
                            });
                        }
                        // Fear / confusion / taunt / convert: no weapon scale
                        // (`CombatAttributeInfluenceDamage_*`; the roll × `GetChanceMultiplier`, 1 in
                        // play). Fear `Execute` adds a fixed-duration entry at >= 1 s; taunt, convert
                        // and disruption become the hit's `combat::Influence`.
                        T::Fear | T::Confusion | T::Taunt | T::Convert => {
                            let secs = (min + (max - min) * rng.unit()) * weight;
                            damage.push(combat::Damage::Reflex { ty, secs });
                        }
                        // Disruption seconds scale as % current life does.
                        T::Disruption => {
                            let k = skill.as_ref().map_or(1.0, |s| s.damage_scale.max(0.0))
                                * scale
                                * weight;
                            let secs = (min + (max - min) * rng.unit()) * k;
                            damage.push(combat::Damage::Reflex { ty, secs });
                        }
                        _ => {
                            if ty == CombatType::Elemental {
                                for h in
                                    combat::HitDamage::roll_elemental(min * k, max * k, 1.0, rng)
                                {
                                    damage.push(combat::Damage::Hit(h));
                                }
                            } else {
                                damage.push(combat::Damage::Hit(combat::HitDamage::roll(
                                    ty,
                                    min * k,
                                    max * k,
                                    1.0,
                                    rng,
                                )));
                            }
                        }
                    }
                }
                SlotKind::Dot => {
                    // Damage over time and debuffs (`offensiveSlowX…`, per second for
                    // `DurationMin/Max` seconds); damage types scale with the total damage modifier
                    // and the weapon %, debuff values do not.
                    let dmin = v.0[SV_DMIN];
                    let dmax = if v.0[SV_DMAX] >= dmin {
                        v.0[SV_DMAX]
                    } else {
                        dmin
                    };
                    let dur = dmin + (dmax - dmin) * rng.unit();
                    if dur <= 0.0 {
                        continue;
                    }
                    let dmg = s.dot == DotKind::Damage;
                    if s.dot == DotKind::Skip || (dmg && !collect_dots) {
                        continue;
                    }
                    let k = if dmg { total_mod * scale * weight } else { 1.0 };
                    let val = min * k + (max - min) * k * rng.unit();
                    damage.push(combat::Damage::Dot(combat::DotDamage {
                        ty,
                        dps: [val, min * k, max * k],
                        duration: dur,
                        base_duration: dur,
                        modifier: 0.0,
                        duration_modifier: 0.0,
                        scale_by_attributes: dmg,
                        hit_multiplier: 1.0,
                        source: dot_source_key(owner, s.fmin),
                    }));
                }
                SlotKind::Cc => {
                    // Crowd control (`offensiveStun/Freeze/…Min/Max` seconds), `offensiveXModifier`
                    // % longer.
                    let secs =
                        (min + (max - min) * rng.unit()) * weight * (1.0 + modifier(ty) * PCT);
                    damage.push(combat::Damage::Reflex { ty, secs });
                }
            }
        }
    };
    // The gear source: the summed `totals` (the off-hand reading on a dual wielder's off-hand
    // swing). Its DoTs come per owner below in play; the sheet keeps the sum (`collect_dots`).
    let gear_vals = if atk.hands.is_some() {
        cache.gear_off.as_ref().unwrap()
    } else {
        &cache.gear
    };
    collect_source(gear_vals, weapon_scale, true, None, rng, damage, 0, display);
    // The skill's own damage (its record, learned modifiers and parent at their levels). With no
    // skill every field is 0, so the source rolls nothing — skipped.
    if let Some(s) = skill.as_ref() {
        let sv = skill_offense_vals(s, tooltip);
        collect_source(&sv, 1.0, false, None, rng, damage, skill_owner, true);
    }
    // The global-chance groups' damage stores (rolled in `global_parts`).
    for (owner, gd) in &sc.group_damage {
        let vals = match gd {
            GroupDmg::Cached { group, pick } => pick.map_or(&cache.groups[*group].all_dmg, |p| {
                &cache.groups[*group].one_of_dmg[p]
            }),
            GroupDmg::Display(b) => &**b,
        };
        collect_source(vals, weapon_scale, true, None, rng, damage, *owner, true);
    }
    // The gear's and passives' DoTs per owning source (item instance / record): distinct owners
    // stack in the defender's DotManager, the same owner's re-application keeps max (module docs).
    // A dual wielder's swing deals its hand's weapon's DoTs, not the other hand's.
    if !display {
        let (hand_ds, hand_dense): (&[DotSource], &[[SlotVals; OFFENSE_SLOTS]]) =
            match atk.profile.dual.as_ref() {
                Some(h) if atk.hands.is_some() => (&h.off_dots, &cache.off_dots),
                Some(h) => (&h.main_dots, &cache.main_dots),
                None => (&[], &[]),
            };
        for (ds, vals) in atk
            .profile
            .dot_sources
            .iter()
            .zip(cache.dots.iter())
            .chain(hand_ds.iter().zip(hand_dense.iter()))
        {
            collect_source(vals, weapon_scale, true, None, rng, damage, ds.owner, true);
        }
    }
    // The character's chance-gated instances (`field@chance`, `chance_key`, split once per profile
    // in its `CombatCache`): one roll each, as the engine's per-source attribute instances
    // (`DamageAttributeAbs::AddDamageToAccumulator`).
    if weapon_scale > 0.0 {
        for inst in &cache.offense_instances {
            let c = inst.chance;
            let weight = if display {
                chance_weight(c)
            } else if rng.unit() * 100.0 > c {
                continue;
            } else {
                1.0
            };
            let get = |k: &str| inst.fields.get(k).copied().unwrap_or(0.0);
            // A gated `offensivePhysicalMin/Max` (`DamageAttribute_Physical`) is base physical
            // damage of its own: `DamageAttribute_Physical::AddDamageToAccumulator` adds a
            // `CombatAttributeDamage_BasePhysical` of its min / max × the chance multiplier × the
            // scale. PORT: its pierce ratio is its own record's `offensivePierceRatio`
            // (`GetPierceRatio`), which the totals do not keep per instance: 0 (no stock record with
            // a gated physical sets one).
            let ((min, max), _) = base_physical(get);
            if min > 0.0 {
                let k = total_mod * weapon_scale * weight;
                damage.push(combat::Damage::Weapon(WeaponDamage::roll(
                    min * k,
                    max * k,
                    0.0,
                    1.0,
                    rng,
                )));
            }
            collect_source(
                &inst.dense,
                weapon_scale,
                true,
                Some(weight),
                rng,
                damage,
                inst.owner,
                true,
            );
        }
    }
    // Damage conversion on the flat values (`ConversionAttributeAccumulator::
    // Apply` → `CombatAttributeAccumulator::ConvertDamage`), then each
    // damage's % modifiers and processing (in this order: the accumulator
    // converts before `Process`).
    // Mana burn: a plain % of energy (its `…RatioAdder` raises the damage
    // ratio instead); no attribute scaling (it uses
    // `CombatAttributeAbsDamage::Process`, whose attribute scaling only
    // covers type 4).
    for d in damage.iter_mut() {
        match d {
            combat::Damage::Weapon(w) => w.physical_modifier = hit_mod(&local, T::Physical),
            combat::Damage::Hit(h) if h.ty == T::ManaBurn => {}
            combat::Damage::Hit(h) => h.modifier = hit_mod(&local, h.ty),
            combat::Damage::Dot(dot) => {
                let (m, dm) = dot_mod(&local, dot.ty);
                dot.modifier = if dot.scale_by_attributes { m } else { 0.0 };
                dot.duration_modifier = dm;
            }
            _ => {}
        }
    }
    sc.convs.clear();
    if let Some(s) = skill.as_ref() {
        s.conversions_into(&mut sc.convs);
    }
    if tooltip {
        sc.convs.retain(|c| c.2 >= 100.0);
    }
    let player_convs: &[_] = if tooltip {
        &[]
    } else {
        &atk.profile.conversions
    };
    combat::convert_damage_with(damage, &[&sc.convs, player_convs], &mut sc.convert);
    // `DamageScaleInfo`: the racial bonus (`DamageScaleInfo.racialBonus`, (100 + %) % when above 0,
    // `crate::racial`) and the combo charges (`DamageScaleInfo.comboFactor`).
    let combo_scale = DamageScale(
        if racial_pct > 0.0 {
            100.0 + racial_pct
        } else {
            0.0
        },
        skill.as_ref().map_or(0.0, |s| s.combo),
    );
    for d in damage.iter_mut() {
        match d {
            combat::Damage::Weapon(w) => {
                w.physical_modifier += hit_mod(&player, T::Physical);
                // PORT: the weapon's pierce part takes the skill's pierce
                // modifier unscaled by a physical conversion (the engine's
                // `BasePhysical` holds one modifier, scaled).
                w.pierce_modifier = modifier(T::Pierce);
                w.process(
                    &scaling,
                    &out_red,
                    combo_scale,
                    if weapon_scale > 0.0 {
                        ad(T::OffensiveReduction)
                    } else {
                        0.0
                    },
                );
            }
            combat::Damage::Hit(h) if h.ty == T::ManaBurn => h.scale_by_attributes = false,
            combat::Damage::Hit(h) => {
                h.modifier += hit_mod(&player, h.ty);
                h.process(&scaling, &out_red, combo_scale);
            }
            combat::Damage::Dot(dot) => {
                let (m, dm) = dot_mod(&player, dot.ty);
                let dmg = dot.scale_by_attributes;
                if dmg {
                    dot.modifier += m;
                }
                dot.duration_modifier += dm;
                let red = if dmg {
                    out_red
                } else {
                    ReductionInfo::default()
                };
                dot.process(&scaling, &red, combo_scale);
            }
            _ => {}
        }
    }
    // `retaliationDamagePct`: the attack adds that % of the character's
    // retaliation (see [`SkillUse::retaliation_pct`]).
    let ret_pct = if display {
        0.0
    } else {
        skill.as_ref().map_or(0.0, |s| s.retaliation_pct())
    };
    if ret_pct > 0.0 {
        let get = |k: &str| t(k) + skill.as_ref().map_or(0.0, |s| s.f(k));
        let mut ret = retaliation_damage(
            atk.profile,
            skill.as_ref(),
            &get,
            ret_pct * PCT,
            rng,
            &[&sc.convs, player_convs],
        );
        process_retaliation(
            &mut ret,
            &AttackerScaling {
                formulas: &rules.formulas,
                vars: CombatVars::default(),
            },
            &out_red,
            combo_scale,
        );
        damage.extend(ret);
    }
    let mana_burn_ratio = t("offensiveManaBurnDamageRatio")
        + skill
            .as_ref()
            .map_or(0.0, |s| s.f("offensiveManaBurnDamageRatio"))
        + modifier(T::ManaBurn);
    // `Skill::CollectCombatParameters`: the skill's and
    // the character's type-59 modifiers sum into `ParametersCombat.critDamageBonus`,
    // stored as a fraction (`DamageAttributeAbsMod_CritDamageModifier`
    // × 0.01); `TakeAttack` adds it to the crit multiplier.
    let crit_damage_bonus =
        (player("offensiveCritDamageModifier") + local("offensiveCritDamageModifier")) * PCT;
    let damage = std::mem::take(damage);
    put_scratch(sc);
    (damage, mana_burn_ratio, crit_damage_bonus)
}

/// The racial bonus % of an attack on a target of these races
/// (`crate::racial`): the skill's own and its modifiers' at their level,
/// plus the character's unless the skill has `excludeRacialDamage`.
pub fn racial_damage(atk: &Profile, skill: &Option<SkillUse>, target: &[String]) -> f32 {
    let mut pct = 0.0;
    if let Some(s) = skill {
        pct = std::iter::once((s.record, s.level_index))
            .chain(s.mods.iter().copied())
            .map(|(r, li)| crate::racial::record_damage_against(r, li, target))
            .fold(0.0, |p, x| p + x);
    }
    if !skill
        .as_ref()
        .is_some_and(|s| s.record.bool("excludeRacialDamage"))
    {
        pct += crate::racial::damage_against(&atk.racial, target);
    }
    pct
}

/// One hit of `atk` on `def` (current life `life`), the single attack entry
/// point: weapon / gear damage × the skill's weapon %, the skill's own
/// damage, all with the attacker's % modifiers ([`collect_damage`]), through
/// `take_attack`. `skill: None` is a plain weapon attack.
///
/// - `atk_debuffs` / `def_debuffs`: the debuffs active on the attacker (its
///   damage reductions, the `ReductionInfo` of its outgoing damage, OA loss
///   and fumble chance) and on the defender (DA loss, armor and resistance
///   reductions). "Reduced target's Physical Damage" (`OffensiveReduction`)
///   is a flat amount taken off every weapon damage part, only for attacks
///   with a weapon damage % (`Skill::CollectCombatParameters` passes it to
///   `ProcessDamage`, which only `CombatAttributeDamage_BasePhysical::Process`
///   uses).
/// - `def_energy`: the defender's current energy (`None`: full). Mana burn
///   takes at most what is left of it (`CombatAttributeAbsDamage_ManaBurn::
///   Execute`) and its damage follows the energy
///   actually burned.
/// - `absorb`: the defender's damage-absorbing buffs (their pools run down in
///   place).
/// - `link_absorb`: `GetReflectLinkDamageReduction` (the defender's reflect
///   links' summed `damageAbsorptionPercent`), giving [`Hit::link_copy`]. The
///   links' own damage cut (`Skill_AttackSpellReflectLink::ModifyDamage`) is
///   one `absorb` entry per link.
#[allow(clippy::too_many_arguments)]
pub fn attack_linked(
    rules: &Rules,
    atk: Attacker<'_>,
    def: &Profile,
    life: &mut f32,
    def_energy: Option<f32>,
    block_cooldown_ms: &mut i32,
    rng: &mut Rng,
    skill: Option<SkillUse>,
    atk_debuffs: &Debuffs,
    def_debuffs: &Debuffs,
    absorb: &mut [combat::Absorb],
    link_absorb: f32,
) -> Hit {
    use CombatType as T;
    let ad = |ty| atk_debuffs.get(ty);
    let racial_pct = racial_damage(atk.profile, &skill, &def.race);
    let (damage, mana_burn_ratio, crit_damage_bonus) = collect_damage(
        rules,
        atk,
        &skill,
        rng,
        atk_debuffs,
        false,
        false,
        racial_pct,
    );
    let mut sc = take_scratch();
    let mut attack = combat::Attack {
        kind: skill.as_ref().map_or(AttackKind::Melee, |s| s.style),
        offensive_ability: debuffed_offensive_ability(atk.profile, atk_debuffs),
        fumble_chance: ad(T::Fumble),
        projectile_fumbled: false,
        unblockable: false,
        crit_damage_bonus,
        // `ParametersCombat.manaCost` = `Skill::GetManaCost` (
        // `Skill::CollectCombatParameters`): the cost the
        // defender's energy absorption feeds on.
        energy_cost: skill.as_ref().map_or(0.0, |s| {
            s.energy_cost(atk.profile.skill_attribute("skillManaCostReduction"), |c| {
                rng.unit() * 100.0 <= c
            })
        }),
        mana_burn_ratio,
        damage,
    };
    let mut d = defender(rules, def, *life, def_debuffs);
    if let Some(e) = def_energy {
        d.energy = e.clamp(0.0, d.energy_max);
    }
    sc.absorb.clear();
    sc.absorb.extend_from_slice(absorb);
    d.absorb = std::mem::take(&mut sc.absorb);
    d.link_absorb = link_absorb;
    d.block_cooldown_ms = *block_cooldown_ms;
    d.racial_reduction = crate::racial::defense_against(&def.racial, &atk.profile.race);
    let mut r = combat::take_attack(&rules.formulas, &mut attack, &mut d, rng);
    *block_cooldown_ms = d.block_cooldown_ms;
    absorb.clone_from_slice(&d.absorb);
    sc.absorb = std::mem::take(&mut d.absorb);
    sc.absorb.clear();
    sc.damage = std::mem::take(&mut attack.damage);
    sc.damage.clear();
    let damage = (*life - d.life).max(0.0);
    *life = d.life.max(0.0);
    // Reflected debuffs (`CombatAttributeDurFixedDamage`, the only reflected
    // attributes flagged for the cap) scale by at most the reflector's
    // `Player::GetReflectCap` % (`ScaleDamage(pct, cap)`;
    // monsters have none).
    if def.is_player
        && rules.caps.player_reflect_cap > 0.0
        && r.reflect_pct > rules.caps.player_reflect_cap
    {
        let k = rules.caps.player_reflect_cap / r.reflect_pct;
        for dm in &mut r.reflected {
            if let combat::Damage::Dot(dot) = dm
                && !is_dot_damage(dot.ty)
            {
                dot.dps.iter_mut().for_each(|x| *x *= k);
            }
        }
    }
    put_scratch(sc);
    Hit::from_result(r, attack.kind, damage)
}

/// `Skill_AttackSpellReflectLink::ReflectLinkDamage`:
/// a link's part of [`Hit::link_copy`]: every attribute scaled
/// by the skill's `damageAbsorptionReflectPercent`, the ones flagged for the
/// reflect cap (the debuffs, as in [`attack_linked`]) by at most the
/// reflector's `Player::GetReflectCap` (`playerReflectCap`; monsters: none).
/// The caster then attacks the link's target with it as a style-6 attack
/// (`Character::AttackTarget`), i.e. [`reflect`].
pub fn link_reflection(
    rules: &Rules,
    reflector: &Profile,
    copy: &[combat::Damage],
    reflect_pct: f32,
) -> Vec<combat::Damage> {
    let cap = rules.caps.player_reflect_cap;
    copy.iter()
        .cloned()
        .map(|mut d| {
            let pct = match &d {
                combat::Damage::Dot(dot)
                    if !is_dot_damage(dot.ty) && reflector.is_player && cap > 0.0 =>
                {
                    reflect_pct.min(cap)
                }
                _ => reflect_pct,
            };
            d.scale(pct);
            d
        })
        .collect()
}

/// `CombatManager::ReflectCombatDamage`: the
/// reflected damage hits the original attacker as a style-6 attack (always
/// lands; `TakeAttack` scales it by 1 − the attacker's capped
/// `defensivePercentReflectionResistance` % ([`Profile::reflection_resist`],
/// the value the sheet shows); never reflected again).
pub fn reflect(
    rules: &Rules,
    reflector: &Profile,
    attacker: &Profile,
    attacker_life: &mut f32,
    damage: Vec<combat::Damage>,
    rng: &mut Rng,
    attacker_debuffs: &Debuffs,
) -> Option<Hit> {
    if damage.is_empty() {
        return None;
    }
    let mut attack = combat::Attack {
        kind: AttackKind::Reflection,
        offensive_ability: reflector.sheet.offensive_ability,
        fumble_chance: 0.0,
        projectile_fumbled: false,
        unblockable: true,
        crit_damage_bonus: 0.0,
        energy_cost: 0.0,
        mana_burn_ratio: 0.0,
        damage,
    };
    let mut d = defender(rules, attacker, *attacker_life, attacker_debuffs);
    d.racial_reduction = crate::racial::defense_against(&attacker.racial, &reflector.race);
    let r = combat::take_attack(&rules.formulas, &mut attack, &mut d, rng);
    let damage = (*attacker_life - d.life).max(0.0);
    *attacker_life = d.life.max(0.0);
    Some(Hit {
        critical: false,
        reflected: Vec::new(),
        link_copy: Vec::new(),
        ..Hit::from_result(r, AttackKind::Reflection, damage)
    })
}

/// The defender side of a hit: resistances, armor, `defensiveXDuration`,
/// and the debuffs on it (DA, armor and resistance reductions). The
/// profile-derived parts are the profile's [`CombatCache`] (built once,
/// invalidated by the profile rebuild on `ProfileDirty`); only the
/// debuff-dependent scalars run per hit.
fn defender<'a>(rules: &Rules, def: &'a Profile, life: f32, def_debuffs: &Debuffs) -> Defender<'a> {
    use CombatType as T;
    let dd = |ty| def_debuffs.get(ty);
    let res_red = ReductionInfo {
        total_pct: dd(T::TotalResistanceReductionPercent),
        total_abs: dd(T::TotalResistanceReductionAbsolute),
        physical_pct: dd(T::PhysicalResistanceReductionPercent),
        physical_abs: dd(T::PhysicalResistanceReductionAbsolute),
        elemental_pct: dd(T::ElementalResistanceReductionPercent),
        elemental_abs: dd(T::ElementalResistanceReductionAbsolute),
    };
    let diff = (rules.balance / 4).min(2);
    let cap = if def.is_player {
        rules.caps.player_defense_cap[diff]
    } else if def.pet {
        // `Pet::GetDefenseAttributeCap` ([`Profile::pet`]).
        rules.caps.player_pet_defense_cap[diff]
    } else {
        // `Monster::GetDefenseAttributeCap` reads
        // `monsterDefenseCap` at the difficulty.
        rules.caps.monster_defense_cap[diff]
    };
    let cache = def.combat_cache();
    Defender {
        defensive_ability: debuffed_defensive_ability(def, def_debuffs),
        // `CanIDodgeAttack` / `CanIDeflectProjectile`:
        // character attributes 27 / 28, percent.
        dodge: def.sheet.dodge,
        deflect: def.sheet.deflect,
        // `take_attack` processes a copy of each per hit.
        defenses: &cache.defenses,
        resistance_cap: cap,
        resistance_reduction: res_red,
        armor_reduction: dd(T::DefensiveReduction),
        // `CombatManager::TakeAttack`: `(DDM.GetFixedDamage(61) + 100) +
        // accumulator.GetTotalDefenseType(61)` (the defender's `negativeDamageMult`,
        // `DefenseAttributeMisc_NegativeDamageMultiplier::AddToAccumulator`).
        // `negativeDamageMultChance` is not rolled, by design: no stock record (base … gdx3,
        // `survivalmode*`) sets it, so every `negativeDamageMult` (the Amplification potions) always counts.
        damage_taken_pct: 100.0 + dd(T::DamageMultiplier) + cache.negative_damage_mult,
        reflection_resist: def.reflection_resist(cap),
        // The player's `playerReflectCap` applies to reflected debuffs only,
        // see [`attack_linked`].
        reflect: cache.reflect,
        // The shield block (`TakeAttack`: the left hand's shield, defense
        // types 55 `defensiveBlockModifier` / 56 `defensiveBlockAmountModifier`,
        // character attribute 25 shortens the recovery).
        shield: def.shield,
        absorb: Vec::new(),
        link_absorb: 0.0,
        block_cooldown_ms: 0,
        block_recovery_reduction: def.sheet.block_recovery_reduction,
        block_modifier: cache.block_modifier,
        block_amount_modifier: cache.block_amount_modifier,
        // Against the attacker's races, set per attack (`crate::racial`).
        racial_reduction: 0.0,
        // `TakeAttack` tail: character attribute 26, percent of the
        // attack's energy cost.
        energy_absorption: def.sheet.energy_absorption,
        // Full unless the caller passes the current energy ([`attack_linked`]).
        energy: def.sheet.energy,
        energy_max: def.sheet.energy,
        life,
    }
}

/// The profile-derived defenses of [`defender`] ([`CombatCache`]): the
/// resistances (with the `defensiveCrowdControl` split), the armor pieces and
/// the `defensiveXDuration` entries. No debuff reads: `TakeAttack` applies
/// those per hit.
fn build_defenses(def: &Profile) -> Vec<Defense> {
    use crate::attributes::{AttrFamily, family};
    use CombatType as T;
    let mut defenses: Vec<Defense> = Vec::new();
    for &(ty, value) in &def.resistances {
        // `DefenseAttributeAbs_CrowdControl::AddToAccumulator`:
        // `defensiveCrowdControl` (type 54) becomes four
        // restricted defenses — stun, freeze, petrify, trap — merged into
        // ones already held (`AddDefense`).
        let types: &[CombatType] = if ty == T::CrowdControl {
            &[T::Stun, T::Freeze, T::Petrify, T::Trap]
        } else {
            std::slice::from_ref(&ty)
        };
        for &t in types {
            let held = defenses.iter_mut().find_map(|d| match d {
                Defense::Resistance { ty, value, .. } if *ty == t => Some(value),
                _ => None,
            });
            match held {
                Some(v) => *v += value,
                None => defenses.push(crate::sheet::resistance_defense(def, t, value)),
            }
        }
    }
    let cache_mods = |k: &str| def.totals.get(k).copied().unwrap_or(0.0);
    let pmod = cache_mods("defensiveProtectionModifier");
    let amod = cache_mods("defensiveAbsorptionModifier");
    for (r, p) in &def.armor_regions {
        defenses.push(Defense::Armor {
            region: Some(*r),
            protection: *p,
            absorption: def.absorption,
            protection_modifier: pmod,
            absorption_modifier: amod,
        });
    }
    if def.protection > 0.0 {
        defenses.push(Defense::Armor {
            region: None,
            protection: def.protection,
            absorption: def.absorption,
            protection_modifier: pmod,
            absorption_modifier: amod,
        });
    }
    // `defensiveXDuration`: shorter DoTs/debuffs of that type.
    for dd in family(AttrFamily::DefenseDuration) {
        let (Some(f), Some(ty)) = (dd.value, dd.combat_type()) else {
            continue;
        };
        let v = def.totals.get(f).copied().unwrap_or(0.0);
        if v != 0.0 {
            defenses.push(Defense::DurationResistance {
                ty,
                value: v,
                modifier: 0.0,
                cap_bonus: crate::sheet::resistance_cap_bonus(def, ty),
            });
        }
    }
    defenses
}

/// The `ReductionInfo` of a character's outgoing damage: the damage
/// reduction debuffs on it.
fn outgoing_reduction(debuffs: &Debuffs) -> ReductionInfo {
    use CombatType as T;
    let ad = |ty| debuffs.get(ty);
    ReductionInfo {
        total_pct: ad(T::TotalDamageReductionPercent),
        total_abs: ad(T::TotalDamageReductionAbsolute),
        physical_pct: ad(T::PhysicalDamageReductionPercent),
        elemental_pct: ad(T::ElementalDamageReductionPercent),
        ..Default::default()
    }
}

/// Character attributes of resolved attribute instances (value / modifier /
/// multiplier families).
pub fn add_char_attrs(attrs: &[crate::items::resolve::Attr], acc: &mut CharAttrAccumulator) {
    use crate::attributes::AttrFamily;
    for a in attrs {
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

/// The player's profile from resolved equipment (affixes, components and
/// augments are part of each item) plus active item-set bonuses.
#[allow(clippy::too_many_arguments)]
pub fn player_from_items(
    db: &Database,
    idb: &crate::items::ItemDb,
    rules: &Rules,
    pc_record: &str,
    level: i32,
    bio: Option<[f32; 5]>,
    allocated: AllocatedPoints,
    items: &[crate::items::ResolvedItem],
    skills: &[(Record, usize)],
) -> Option<Profile> {
    // Base weapon damage comes from the main weapon only: a shield's
    // `offensivePhysicalMin` is shield-skill damage, not a second weapon.
    let main = items
        .iter()
        .position(|i| i.is_weapon && !i.class.contains("Shield"));
    // Two one-handed melee weapons = dual wield (only then can both hands
    // hold weapons): the second's flat damage is its hand's alone.
    let one_hand = |c: &str| {
        crate::equip::WeaponType::from_class(c)
            .is_some_and(crate::equip::WeaponType::one_handed_melee)
    };
    let dual_melee = items.iter().filter(|i| one_hand(&i.class)).count() >= 2;
    let off = if dual_melee {
        items
            .iter()
            .enumerate()
            .position(|(n, i)| Some(n) != main && one_hand(&i.class))
    } else {
        None
    };
    let mut owners: Vec<u64> = items.iter().map(|i| item_dot_owner(&i.item)).collect();
    let mut hands = HandFlats::default();
    let mut totals: Vec<(Option<Region>, BTreeMap<String, f32>)> = items
        .iter()
        .enumerate()
        .map(|(n, i)| {
            let mut t = i.totals();
            if Some(n) == main && off.is_some() {
                hands.main = t
                    .iter()
                    .filter(|(k, _)| is_hand_flat(k))
                    .map(|(k, v)| (k.clone(), *v))
                    .collect();
            }
            if Some(n) == off {
                hands.off = t
                    .iter()
                    .filter(|(k, _)| is_hand_flat(k))
                    .map(|(k, v)| (k.clone(), *v))
                    .collect();
                // The off-hand's DoTs are its hand's alone (the flat swap).
                let dots: BTreeMap<String, f32> = t
                    .iter()
                    .filter(|(k, _)| is_dot_field(k))
                    .map(|(k, v)| (k.clone(), *v))
                    .collect();
                if !dots.is_empty() {
                    hands.off_dots = vec![DotSource {
                        owner: owners[n],
                        fields: dots,
                    }];
                }
                t.retain(|k, _| !is_hand_flat(k));
            }
            if Some(n) != main {
                for k in [
                    "offensivePhysicalMin",
                    "offensivePhysicalMax",
                    "offensivePierceRatioMin",
                ] {
                    t.remove(k);
                }
            }
            // Body region an armor piece protects.
            (armor_region(&i.class), t)
        })
        .collect();
    let refs: Vec<&crate::items::ResolvedItem> = items.iter().collect();
    let sets = crate::items::sets::active_sets(idb, &refs);
    for set in &sets {
        let mut t = BTreeMap::new();
        for a in set.bonus.iter().filter(|a| a.global.is_none()) {
            for (k, v) in &a.values {
                *t.entry(k.clone()).or_insert(0.0) += v;
            }
        }
        totals.push((None, t));
        owners.push(gd_formats::collections::hash_one(set.record.as_str()));
    }
    type AddAttrs<'a> = Box<dyn Fn(&mut CharAttrAccumulator) + 'a>;
    let adds: Vec<AddAttrs> = items
        .iter()
        .map(|i| {
            Box::new(move |a: &mut CharAttrAccumulator| i.add_char_attributes(a))
                as Box<dyn Fn(&mut _)>
        })
        .collect();
    let refs: Vec<&dyn Fn(&mut CharAttrAccumulator)> = adds.iter().map(|b| b.as_ref()).collect();
    let set_adds: Vec<AddAttrs> = sets
        .iter()
        .map(|s| {
            Box::new(move |a: &mut CharAttrAccumulator| add_char_attrs(&s.bonus, a))
                as Box<dyn Fn(&mut _)>
        })
        .collect();
    let set_refs: Vec<&dyn Fn(&mut CharAttrAccumulator)> =
        set_adds.iter().map(|b| b.as_ref()).collect();
    let mut p = player_inner(
        db, rules, pc_record, level, bio, allocated, &totals, &refs, &set_refs, skills, dual_melee,
        &owners,
    )?;
    // `owners`: the items' ids, then the sets'.
    p.global_groups.extend(
        items
            .iter()
            .map(|i| &i.attrs)
            .chain(sets.iter().map(|s| &s.bonus))
            .zip(&owners)
            .flat_map(|(attrs, o)| item_global_groups(attrs, *o)),
    );
    for i in items {
        p.conversions.extend(attr_conversions(&i.attrs));
    }
    for set in &sets {
        p.conversions.extend(attr_conversions(&set.bonus));
    }
    if let (Some(main), Some(_off)) = (main, off) {
        // The main weapon's DoTs are its hand's alone too: out of the shared
        // sources, into the hand flats.
        if let Some(pos) = p.dot_sources.iter().position(|ds| ds.owner == owners[main]) {
            hands.main_dots = vec![p.dot_sources.remove(pos)];
        }
        p.dual = Some(Box::new(hands));
    }
    p.shield = worn_shield(db, items);
    p.racial.extend(
        items
            .iter()
            .flat_map(|i| crate::racial::of_item(db, &i.item)),
    );
    Some(p)
}

/// The worn shield's block stats (its record's fields,
/// `WeaponArmor_Shield::Load`).
fn worn_shield(db: &Database, items: &[crate::items::ResolvedItem]) -> Option<combat::Shield> {
    let i = items.iter().find(|i| i.class == "WeaponArmor_Shield")?;
    db.get(&i.item.base_name).map(combat::Shield::from_record)
}
