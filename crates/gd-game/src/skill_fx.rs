//! Skill effects a character wears while buffs are on it: the weapon
//! enchantment of its buffs and toggles (`weaponEnchantment` →
//! `WeaponEnchantmentPak`: an effect on the weapon, a weapon trail, a tint, a
//! ranged weapon's projectile) and the character effect paks of its toggled
//! skills (`charFxPakSelfNames`).
//!
//! Game.dll: a skill's buff effects go on with `Skill::
//! ApplyBuffSelfEffects` @0x18049de00 (a toggle switching on, through
//! `ApplyActivatedVisualEffects`) and `Skill::ApplyBuffOtherEffects`
//! @0x18049e550 (every `SkillBuff::Install`, so also an aura's buff on its
//! own caster): `SkillManager::AddCharFxPak` with the skill's
//! `charFxPakSelfNames` (resp. `charFxPakOtherNames`) entry of its level
//! (level − 1, clamped to the list) plus each learned modifier's
//! `GetBuffSelfCharFxPakName`, then `RemoveWeaponEnchantment(id)` and
//! `AddWeaponEnchantment(id, weaponEnchantment[level − 1])` when that
//! entry is set (SkillProfile +2064, read by `SkillProfile::Load`
//! @0x180545880); switching off removes both. `SkillManager::
//! AddWeaponEnchantment` @0x18053d700 appends (id, pak) to a list and hands
//! the new pak to `EquipManager::SetWeaponEnchantment` @0x180286b60;
//! `RemoveWeaponEnchantment` @0x18053d7e0 drops the id's entries and hands
//! over the list's last pak, or "" when it is empty. So a character shows
//! one enchantment, the newest of its buffs still on
//! ([`WeaponEnchantments::shown`]); `SetWeaponEnchantment` gives it to every
//! equipped `Weapon` (`Weapon::SetWeaponEnchantment` @0x18058b330, drawn by
//! `grimdawn::fx::enchant`). `Weapon::GetProjectileName` @0x1805895c0
//! returns the pak's `projectileName` for a ranged weapon
//! ([`enchanted_projectile`]).
//!
//! Visibility: `EquipManager::UpdateWeaponFxVisibility`
//! @0x180286c50, called only from `Player::UpdateSelf`, clears the main
//! player's weapon enchantment while the `toggledWeaponEffects` option
//! (bool 64) is off and puts it back when it is on; monsters' and pets'
//! always show. `CharFxPak::UpdateVisibility` @0x1800ed3c0 and `Skill::
//! UpdateFxVisibility` @0x18049ed30 hide the character effects of an
//! auto-toggle skill (`IsAutoToggle`: the `…Toggled` classes set +1521 in
//! their constructors, a `SkillBuff` asks its parent skill) on the main
//! player by `toggledAurasSelf` (bool 62) and on another `Player` by
//! `toggledAurasOther` (bool 63); a pet's or a monster's are never hidden,
//! so in single player bool 63 does nothing. `AuraManager::Update`
//! @0x180047620 does the same for the auto-toggle skills'
//! `skillActivatedAuraName` loops (see below).
//!
//! The aura loop (`Skill::ApplyActivatedVisualEffects` @0x18049db20,
//! called from the toggles' / self buffs' `ReceiveActiveUpdate`): unless the
//! skill's owner is a paper-doll copy (`Player::IsCopy`) it removes the old
//! aura (`Character::RemoveAura`, skill +181) and, switching on, adds the
//! profile's `skillActivatedAuraName` (+2088) with `Character::AddAura`
//! (`AuraManager::AddAura` @0x1800478e0: the effect entity attached to the
//! character at identity coords, kept until `RemoveAura` clears its keep flag
//! and `AuraManager::Update` stops it emitting, self-deleting) and
//! `IsAutoToggle` as the visibility flag `Update` applies with the options
//! above. The port keeps it as a [`ToggleFx`] effect of the toggle at the
//! character's origin. A timed self buff (`Skill_BuffSelfDuration`) loops it too until the buff ends
//! ([`timed_aura`]; other casts keep the one-shot of `casting.rs`). PORT: a stopped one is despawned at once; the
//! aura-hide options are not applied to a timed buff's (its auto-toggle flag was not checked).
//!
//! Port ([`sync_skill_fx`]): the sources on a character are, in the order
//! the engine adds them on a load: the player's switched-on toggles (its
//! radius toggles' buff on itself right after each), its timed buffs and
//! the aura buffs others keep on it; a pet's or a monster's switched-on
//! toggles, its timed buffs and its aura buffs (its own aura's included,
//! `auras.rs`). The enchantment list keeps the engine's order: a source
//! gone drops out, a new one is appended. The toggles' own effect paks (and
//! the paks of a radius toggle's buff on its caster) are spawned here as
//! [`BuffFx`] with a [`ToggleFx`] marker; timed buffs' and other
//! characters' aura effects stay with `casting.rs` / `auras.rs`. PORT: the
//! sources are re-read only when the character's buff names, toggles or
//! loadout change (a cheap signature), not every frame.
//!
//! Modifier visual overrides ([`override_mods`] and the resolvers below).
//! Game.dll: every getter takes the last *learned* modifier (only those `IsSkillEnabled`) that sets
//! the field, and only when none does, the last such *gear* modifier (`GetItemSkillModifiers`), else the
//! skill's own field. Per-level lists are indexed by the skill's level (clamped to the list).
//! - `targetFxPakOverride` → `targetFxPakName` (`Skill::GetTargetFX` @0x18049c1d0, [`target_fx`]).
//! - `projectileOverride` (per-level list) → `skillProjectileName` (`Skill::GetProjectileName`
//!   @0x18049b680); `projectileFragmentsOverride` → `projectileFragmentsName` (`GetProjectileFragmentsName`
//!   @0x18049ba70) ([`projectile_name`], [`fragments_name`]).
//! - `particleEffect1Override` → `particleEffectName1` (`Skill::GetParticleEffect1` @0x180495ba0,
//!   [`particle_effect1`]); `lightningOverride` → `lightningName` (`Skill::GetLightning` @0x180496a50:
//!   chain / fork / radius lightning, [`lightning`]).
//! - `projectileFXOverride` → `additionalProjectileFX`, an effect added to a weapon skill's projectile
//!   (`SkillActivated::AddProjectileEffects` @0x180519510; a weapon-pool attack adds its pool parent's
//!   first, `Skill_WPAttack::AddProjectileEffects` @0x180516d10) ([`projectile_fx`]).
//! - `fxChanges` (a list, `Skill::AddModifierFXChanges` @0x180494bb0, [`fx_changes`]) replaces what each class
//!   names: the projectile of `Skill_AttackProjectile` / `…Chain` / the secondary area-effect and orbiting
//!   projectiles (`CreateProjectile`: entry at the level over [`projectile_name`]), the radius family's
//!   `radiusEffectName` (`CreateVisualEffect` of `Skill_BuffRadius` / `AttackBuffRadius` / `OnHitAttackRadius` /
//!   `AttackRadiusTeleport`: entry at the level, [`radius_effect`]), a beam's `beamName` / `startEffect` /
//!   `endEffect` (`Skill_AttackSpellBeam::ActivateNow`: entries 0–2 when the list has three, [`beam_effects`]),
//!   a drain's beams (four or more entries: gd-render `chaos_beam.rs`), and the cone's
//!   `coneName` (`Skill_AttackSpellCone::ActivateNow` @0x1804cd370: entry 0, [`cone_effect`]), the spin's cast aura
//!   `skillCastAuraName` (`Skill_AttackRadiusSpin::ApplyCastVisualEffects` @0x1804c9c70: entry at the level,
//!   [`spin_aura`]), the grow's `radiusEffectNames` (`Skill_AttackRadiusGrow::ApplyCastVisualEffects` @0x1804c6a80:
//!   entry at the grow step, [`grow_effect`]), a chaos skill's `chaosBeamName` (`Skill_AttackSpellChaos::Update`
//!   @0x1804dc590: entry 0, [`chaos_beam`]) and the charged-scaling charge effect `skillChargeAura`
//!   (`Skill_WeaponPool_ChargedScaling::SetChargeEffect` @0x180510c00: entry at the charge, [`charge_aura`]); the
//!   effects themselves: `crate::casting::class_fx` (the chaos beams: `crate::casting::chaos`).
use crate::casting::BuffFx;
use crate::combat::Dead;
use crate::data::GameData;
use crate::monster_skills::MonsterSkills;
use crate::pets::Pet;
use crate::player::Loadout;
use crate::skill_buffs::ReceivedBuffs;
use crate::weapon_pool::SkillLinks;
use bevy::prelude::*;
use gd_formats::arz::{RawValue, Record};
use gd_formats::vfs::Database;
use std::hash::{Hash, Hasher};

pub struct SkillFxPlugin;

impl Plugin for SkillFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_skill_fx.after(crate::auras::AurasSet));
    }
}

/// The weapon enchantments of the buffs on a character: (skill or buff
/// record, `WeaponEnchantmentPak` record), oldest first (module docs).
#[derive(Component, Default, Debug)]
pub struct WeaponEnchantments {
    list: Vec<(String, String)>,
    sig: u64,
}

impl WeaponEnchantments {
    /// The pak its weapons wear: the newest (`RemoveWeaponEnchantment`
    /// hands over the list's last).
    pub fn shown(&self) -> Option<&str> {
        self.list.last().map(|(_, p)| p.as_str())
    }

    pub fn list(&self) -> &[(String, String)] {
        &self.list
    }
}

/// On a [`BuffFx`] that a toggle's character effects spawned here: the paks
/// it was started from.
#[derive(Component)]
pub struct ToggleFx(pub Vec<String>);

/// One buff or toggle on a character.
#[derive(Debug, Clone, PartialEq)]
struct Source {
    /// The skill (a toggle) or buff record.
    key: String,
    /// Its character effect paks spawned here (toggles only).
    paks: Vec<String>,
    enchantment: Option<String>,
    /// The toggle's `skillActivatedAuraName` loop (toggles only).
    aura: Option<String>,
}

/// A per-level string field at level index `li` (clamped to the list;
/// empty = none), as `SkillProfile`'s level tables.
pub fn level_str(rec: &Record, key: &str, li: usize) -> Option<String> {
    let v = rec.get(key)?;
    match v.get(li.min(v.len().checked_sub(1)?))? {
        RawValue::Str(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// A skill's target effect pak (`Skill::GetTargetFX` @0x18049c1d0): the last learned
/// modifier whose `targetFxPakOverride` (a per-level list, indexed by the skill's level) is set, else the last such
/// gear modifier (`GetItemSkillModifiers`; stock has them, e.g. the Execution / Doom Bolt item modifiers'
/// `wpattack_*_impact_fxpak`), replaces the skill's own `targetFxPakName` (per-level, clamped). `mods`: the enabled
/// gear modifiers, then the learned ones (`Skill_Modifier::IsSkillEnabled`: a gear modifier made exclusive to a
/// learned transmuter is off, `weapon_pool::build_links` leaves it out).
pub fn target_fx(rec: &Record, li: usize, mods: &[(Record, usize)]) -> Option<String> {
    mods.iter()
        .rev()
        .find_map(|(m, _)| level_str(m, "targetFxPakOverride", li))
        .or_else(|| level_str(rec, "targetFxPakName", li))
}

/// The modifiers whose visual overrides apply to `skill` (module doc), in the order the resolvers take
/// them: the gear ones, then the learned ones (a caster with a skill book has the learned; one without has
/// no gear). Gear modifiers that are off (`IsSkillEnabled`) are not in the links (`weapon_pool::build_links`).
pub fn override_mods(
    db: &Database,
    links: Option<&SkillLinks>,
    book: Option<&gd_sim::skills::SkillBook>,
    skill: &str,
) -> Vec<(Record, usize)> {
    let names = links.map_or(&[][..], |l| l.mods_of(skill));
    let learned = |m: &str| book.is_none_or(|b| b.level(m) > 0);
    let (mine, gear): (Vec<_>, Vec<_>) = names.iter().partition(|(m, _)| learned(m));
    gear.into_iter()
        .chain(mine)
        .filter_map(|(m, ml)| db.get_owned(m).map(|r| (r, *ml)))
        .collect()
}

/// A plain string override `key` of `mods` (the last that sets it, module doc).
fn override_str(mods: &[(Record, usize)], key: &str) -> Option<String> {
    mods.iter()
        .rev()
        .find_map(|(m, _)| m.str(key).filter(|s| !s.is_empty()).map(str::to_owned))
}

fn own_str(rec: &Record, key: &str) -> Option<String> {
    rec.str(key).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// `Skill::AddModifierFXChanges`: the `fxChanges` list of the last modifier that has one (module doc).
pub fn fx_changes(mods: &[(Record, usize)]) -> Option<Vec<String>> {
    mods.iter().rev().find_map(|(m, _)| {
        let v: Vec<String> = m.strs_iter("fxChanges").map(|s| s.to_string()).collect();
        v.iter().any(|s| !s.is_empty()).then_some(v)
    })
}

/// The `fxChanges` entry for level index `li` (clamped to the list; empty = none).
fn fx_change_at(mods: &[(Record, usize)], li: usize) -> Option<String> {
    fx_changes(mods)
        .and_then(|v| v.get(li.min(v.len() - 1)).cloned())
        .filter(|s| !s.is_empty())
}

/// The projectile record a projectile skill fires at `li` (`CreateProjectile`): the `fxChanges` entry, else
/// `Skill::GetProjectileName` (`projectileOverride`, else the skill's `skillProjectileName`).
pub fn projectile_name(rec: &Record, li: usize, mods: &[(Record, usize)]) -> Option<String> {
    fx_change_at(mods, li)
        .or_else(|| {
            mods.iter()
                .rev()
                .find_map(|(m, _)| level_str(m, "projectileOverride", li))
        })
        .or_else(|| level_str(rec, "skillProjectileName", li))
}

/// `Skill::GetProjectileFragmentsName`: `projectileFragmentsOverride`, else the skill's `projectileFragmentsName`.
pub fn fragments_name(rec: &Record, mods: &[(Record, usize)]) -> Option<String> {
    override_str(mods, "projectileFragmentsOverride")
        .or_else(|| own_str(rec, "projectileFragmentsName"))
}

/// `Skill::GetParticleEffect1`: `particleEffect1Override`, else the skill's `particleEffectName1`.
pub fn particle_effect1(rec: &Record, mods: &[(Record, usize)]) -> Option<String> {
    override_str(mods, "particleEffect1Override").or_else(|| own_str(rec, "particleEffectName1"))
}

/// `Skill::GetLightning`: `lightningOverride`, else the skill's `lightningName`.
pub fn lightning(rec: &Record, mods: &[(Record, usize)]) -> Option<String> {
    override_str(mods, "lightningOverride").or_else(|| own_str(rec, "lightningName"))
}

/// `SkillActivated::AddProjectileEffects`: `projectileFXOverride`, else the skill's `additionalProjectileFX`.
pub fn projectile_fx(rec: &Record, mods: &[(Record, usize)]) -> Option<String> {
    override_str(mods, "projectileFXOverride").or_else(|| own_str(rec, "additionalProjectileFX"))
}

/// The effects a cast adds to its projectile (`AddProjectileEffects`): a weapon-pool attack's pool `parent`'s
/// first (with the parent's own modifiers), then the skill's ([`projectile_fx`] each).
pub fn projectile_effects(
    db: &Database,
    links: Option<&SkillLinks>,
    book: Option<&gd_sim::skills::SkillBook>,
    rec: &Record,
    mods: &[(Record, usize)],
    parent: Option<&str>,
) -> Vec<String> {
    let parent_fx = parent.and_then(|p| {
        let r = db.get(p)?;
        projectile_fx(r, &override_mods(db, links, book, p))
    });
    parent_fx
        .into_iter()
        .chain(projectile_fx(rec, mods))
        .collect()
}

/// The radius family's effect at `li` (`CreateVisualEffect`): the `fxChanges` entry, else the skill's
/// `radiusEffectName` (per level, clamped).
pub fn radius_effect(rec: &Record, li: usize, mods: &[(Record, usize)]) -> Option<String> {
    fx_change_at(mods, li).or_else(|| level_str(rec, "radiusEffectName", li))
}

/// A beam skill's `beamName`, `startEffect` and `endEffect` (`Skill_AttackSpellBeam::ActivateNow`): an
/// `fxChanges` list of three or more replaces all three by its first entries.
pub fn beam_effects(rec: &Record, mods: &[(Record, usize)]) -> [Option<String>; 3] {
    match fx_changes(mods).filter(|v| v.len() >= 3) {
        Some(v) => [0, 1, 2].map(|i| Some(v[i].clone()).filter(|s| !s.is_empty())),
        None => ["beamName", "startEffect", "endEffect"].map(|k| own_str(rec, k)),
    }
}

/// `Skill_AttackSpellCone::ActivateNow` @0x1804cd370: the cone effect is `coneName` at `li` (clamped), replaced by
/// the first `fxChanges` entry when a modifier has a list.
pub fn cone_effect(rec: &Record, li: usize, mods: &[(Record, usize)]) -> Option<String> {
    match fx_changes(mods) {
        Some(v) => Some(v[0].clone()).filter(|s| !s.is_empty()),
        None => level_str(rec, "coneName", li),
    }
}

/// `Skill_AttackRadiusSpin::ApplyCastVisualEffects` @0x1804c9c70: the spin's cast aura, the profile's
/// `skillCastAuraName` (+2120), replaced by the `fxChanges` entry at `li`.
pub fn spin_aura(rec: &Record, li: usize, mods: &[(Record, usize)]) -> Option<String> {
    fx_change_at(mods, li).or_else(|| own_str(rec, "skillCastAuraName"))
}

/// `Skill_AttackRadiusGrow::ApplyCastVisualEffects` @0x1804c6a80: the grow step of `radius`, the last
/// `radiusEffectSize` entry it reaches (0 when it reaches none).
pub fn grow_step(rec: &Record, radius: f32) -> usize {
    rec.f32s("radiusEffectSize")
        .iter()
        .rposition(|s| radius >= *s)
        .unwrap_or(0)
}

/// `Skill_AttackRadiusGrow::ApplyCastVisualEffects`: the effect of grow step `step`, `radiusEffectNames` at the
/// step (clamped; none without the list), replaced by the `fxChanges` entry at the step.
pub fn grow_effect(rec: &Record, step: usize, mods: &[(Record, usize)]) -> Option<String> {
    rec.get("radiusEffectNames")?;
    fx_change_at(mods, step).or_else(|| level_str(rec, "radiusEffectNames", step))
}

/// `Skill_WeaponPool_ChargedScaling::SetChargeEffect` @0x180510c00: the effect of `charges` charges (none at 0),
/// `SkillProfile::GetChargeAuraName` (`skillChargeAura` at `charges` − 1, clamped) replaced by the `fxChanges`
/// entry there.
pub fn charge_aura(rec: &Record, charges: u32, mods: &[(Record, usize)]) -> Option<String> {
    let i = (charges as usize).checked_sub(1)?;
    fx_change_at(mods, i).or_else(|| level_str(rec, "skillChargeAura", i))
}

/// `Skill_AttackSpellChaos::Update` @0x1804dc590: each new beam's `ChaosBeam` record, the first `fxChanges` entry
/// when a modifier has a list, else `chaosBeamName`.
pub fn chaos_beam(rec: &Record, mods: &[(Record, usize)]) -> Option<String> {
    match fx_changes(mods) {
        Some(v) => Some(v[0].clone()).filter(|s| !s.is_empty()),
        None => own_str(rec, "chaosBeamName"),
    }
}

/// A toggle at `li`: its own paks (with its learned modifiers'
/// `charFxPakSelfNames`) and enchantment; a radius toggle that buffs its
/// caster adds its buff record's `charFxPakOtherNames` and enchantment
/// after it (the `SkillBuff` it installs on the caster) when `with_buff`
/// (the player's: a pet's or monster's comes back as an aura buff).
fn toggle_sources(
    db: &Database,
    skill: &str,
    li: usize,
    mods: &[(String, usize)],
    with_buff: bool,
    out: &mut Vec<Source>,
) {
    let Some(rec) = db.get(skill) else { return };
    let mut paks: Vec<String> = level_str(rec, "charFxPakSelfNames", li)
        .into_iter()
        .collect();
    paks.extend(mods.iter().filter_map(|(m, mli)| {
        db.get(m)
            .and_then(|r| level_str(r, "charFxPakSelfNames", *mli))
    }));
    out.push(Source {
        key: skill.to_owned(),
        paks,
        enchantment: level_str(rec, "weaponEnchantment", li),
        aura: rec
            .str("skillActivatedAuraName")
            .filter(|a| !a.is_empty())
            .map(str::to_owned),
    });
    if with_buff
        && rec.class == "Skill_BuffRadiusToggled"
        && let Some(e) = crate::auras::emission(db, skill, li, Vec::new(), true)
        && e.on_self
        && let Some(b) = db.get(&e.buff)
    {
        out.push(Source {
            key: e.buff.clone(),
            paks: level_str(b, "charFxPakOtherNames", li)
                .into_iter()
                .collect(),
            enchantment: level_str(b, "weaponEnchantment", li),
            aura: None,
        });
    }
}

/// A timed or aura buff: its enchantment only (its effects are spawned by
/// `casting.rs` / `auras.rs`).
fn buff_source(db: &Database, record: &str, li: usize) -> Option<Source> {
    let rec = db.get(record)?;
    let enchantment = level_str(rec, "weaponEnchantment", li).or_else(|| {
        gd_sim::skills::profile_name(rec)
            .and_then(|b| db.get(b))
            .and_then(|b| level_str(b, "weaponEnchantment", li))
    });
    // A timed self buff's `skillActivatedAuraName` loops until the buff ends (`Skill_BuffSelfDuration` inherits
    // `Skill_BuffSelfToggled::ReceiveActiveUpdate`): see [`timed_aura`].
    let aura = timed_aura(rec);
    if enchantment.is_none() && aura.is_none() {
        return None;
    }
    Some(Source {
        key: record.to_owned(),
        paks: Vec::new(),
        enchantment,
        aura,
    })
}

/// The looping aura of a timed self buff (the skill record), `None` for any other record (an aura buff's, a toggle's).
pub fn timed_aura(rec: &Record) -> Option<String> {
    (rec.class == "Skill_BuffSelfDuration")
        .then(|| rec.str("skillActivatedAuraName"))
        .flatten()
        .filter(|a| !a.is_empty())
        .map(str::to_owned)
}

fn player_sources(
    db: &Database,
    l: &Loadout,
    effective: &gd_sim::skills::SkillBook,
    links: Option<&SkillLinks>,
    auras: Option<&ReceivedBuffs>,
    out: &mut Vec<Source>,
) {
    // Learned and worn items' toggles (`crate::player` docs, item toggles).
    for (s, lv) in &l.skills_and_item_toggles(db) {
        if *lv == 0
            || !l.skill_active(db, s)
            || !db
                .get(s)
                .is_some_and(|r| gd_sim::skills::is_toggle(&r.class))
        {
            continue;
        }
        let li = l.skill_level(db, effective, s).max(1) as usize - 1;
        toggle_sources(
            db,
            s,
            li,
            links.map_or(&[][..], |k| k.mods_of(s)),
            true,
            out,
        );
    }
    for (s, li, _) in l.buffs.list.as_slice() {
        out.extend(buff_source(db, s, *li));
    }
    for b in auras.map(|a| a.list.as_slice()).unwrap_or_default() {
        out.extend(buff_source(db, &b.buff, b.level_index));
    }
}

/// A pet's or monster's: toggles, timed buffs, aura buffs (oldest first).
fn npc_sources(
    db: &Database,
    toggled: &[(String, usize)],
    buffs: &[(String, usize, f32)],
    auras: Option<&ReceivedBuffs>,
    out: &mut Vec<Source>,
) {
    for (s, li) in toggled {
        toggle_sources(db, s, *li, &[], false, out);
    }
    for (s, li, _) in buffs {
        out.extend(buff_source(db, s, *li));
    }
    for b in auras
        .map(|a| a.list.iter().rev().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        let Some(rec) = db.get(&b.buff) else { continue };
        let own = toggled
            .iter()
            .any(|(s, _)| s.eq_ignore_ascii_case(&b.skill));
        // The buff of its own toggled aura: its other-character effects on
        // the caster (`SkillBuff::Install`), spawned with the toggles'.
        let paks = if own {
            level_str(rec, "charFxPakOtherNames", b.level_index)
                .into_iter()
                .collect()
        } else {
            Vec::new()
        };
        let enchantment = level_str(rec, "weaponEnchantment", b.level_index);
        if !paks.is_empty() || enchantment.is_some() {
            out.push(Source {
                key: b.buff.clone(),
                paks,
                enchantment,
                aura: None,
            });
        }
    }
}

fn hash_names<'a>(h: &mut impl Hasher, names: impl Iterator<Item = (&'a str, usize)>) {
    for (n, li) in names {
        n.hash(h);
        li.hash(h);
    }
}

/// Keeps every character's [`WeaponEnchantments`] and its toggles'
/// [`ToggleFx`] effects in step with the buffs on it (module docs).
#[allow(clippy::too_many_arguments)]
pub fn sync_skill_fx(
    mut commands: Commands,
    data: Res<GameData>,
    mut chars: Query<(
        Entity,
        Option<Ref<Loadout>>,
        Option<Ref<SkillLinks>>,
        Option<&Pet>,
        Option<&MonsterSkills>,
        Option<&ReceivedBuffs>,
        Option<&mut WeaponEnchantments>,
        Has<Dead>,
    )>,
    fx: Query<(Entity, &BuffFx, &ToggleFx)>,
) {
    let db = &data.db;
    for (e, loadout, links, pet, ms, auras, ench, dead) in &mut chars {
        if loadout.is_none() && pet.is_none() && ms.is_none() {
            continue;
        }
        // No buffs, toggles or auras (most characters): the signature is just `dead`, without SipHash. The hash of the
        // names has its top bit set, so the two kinds never collide.
        let bare = loadout.is_none_or(|l| l.buffs.list.is_empty())
            && auras.is_none_or(|a| a.list.is_empty())
            && pet.is_none_or(|p| p.toggled().is_empty() && p.buffs().is_empty())
            && ms.is_none_or(|m| m.toggled().is_empty() && m.buffs.list.is_empty());
        let sig = if bare {
            dead as u64
        } else {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            dead.hash(&mut h);
            if let Some(l) = loadout.as_deref() {
                hash_names(&mut h, l.buffs.list.iter().map(|b| (b.0.as_str(), b.1)));
            }
            if let Some(a) = auras {
                hash_names(
                    &mut h,
                    a.list.iter().map(|b| (b.buff.as_str(), b.level_index)),
                );
            }
            if let Some(p) = pet {
                hash_names(&mut h, p.toggled().iter().map(|b| (b.0.as_str(), b.1)));
                hash_names(&mut h, p.buffs().iter().map(|b| (b.0.as_str(), b.1)));
            }
            if let Some(m) = ms {
                hash_names(&mut h, m.toggled().iter().map(|b| (b.0.as_str(), b.1)));
                hash_names(&mut h, m.buffs.list.iter().map(|b| (b.0.as_str(), b.1)));
            }
            h.finish() | 1 << 63
        };
        let loadout_changed = loadout.as_ref().is_some_and(|l| l.is_changed())
            || links.as_ref().is_some_and(|l| l.is_changed());
        if ench.as_ref().is_some_and(|w| w.sig == sig) && !loadout_changed {
            continue;
        }
        let mut sources = Vec::new();
        if !dead {
            match (&loadout, pet, ms) {
                (Some(l), ..) => {
                    let effective = l.effective_skills(db, &data.skill_trees());
                    player_sources(db, l, &effective, links.as_deref(), auras, &mut sources)
                }
                (None, Some(p), _) => npc_sources(db, p.toggled(), p.buffs(), auras, &mut sources),
                (None, None, Some(m)) => {
                    npc_sources(db, m.toggled(), &m.buffs.list, auras, &mut sources)
                }
                _ => {}
            }
        }
        // Enchantments: gone ones drop out, new ones are appended.
        let want: Vec<(String, String)> = sources
            .iter()
            .filter_map(|s| s.enchantment.clone().map(|p| (s.key.clone(), p)))
            .collect();
        let mut list: Vec<(String, String)> =
            ench.as_ref().map(|w| w.list.clone()).unwrap_or_default();
        list.retain(|x| want.contains(x));
        for w in want {
            if !list.contains(&w) {
                list.push(w);
            }
        }
        match ench {
            Some(mut w) => {
                if w.list != list {
                    debug!(
                        "{e}: weapon enchantments {:?}",
                        list.iter()
                            .map(|x| x.1.rsplit('/').next().unwrap_or(""))
                            .collect::<Vec<_>>()
                    );
                    w.list = list;
                }
                w.sig = sig;
            }
            None => {
                commands
                    .entity(e)
                    .try_insert(WeaponEnchantments { list, sig });
            }
        }
        // Toggle effects: restart the ones whose paks changed, stop the
        // ones whose toggle went off.
        let have: Vec<(Entity, &str, &Vec<String>)> = fx
            .iter()
            .filter(|(_, b, _)| b.owner == e)
            .map(|(f, b, t)| (f, b.skill.as_str(), &t.0))
            .collect();
        for (f, key, paks) in &have {
            if !sources.iter().any(|s| {
                s.key == *key && &s.paks == *paks && (!s.paks.is_empty() || s.aura.is_some())
            }) {
                commands.entity(*f).try_despawn();
            }
        }
        for s in sources
            .into_iter()
            .filter(|s| !s.paks.is_empty() || s.aura.is_some())
        {
            if have.iter().any(|(_, k, p)| *k == s.key && **p == s.paks) {
                continue;
            }
            let mut effects: Vec<(String, Option<String>)> = s
                .paks
                .iter()
                .flat_map(|p| crate::casting::pak_effects(&data, p))
                .collect();
            // The aura loop sits at the character's origin (empty point, `Skill::ApplyActivatedVisualEffects`).
            effects.extend(s.aura.map(|a| (a, Some(String::new()))));
            let sounds = crate::casting::char_pak_sounds(&data, &s.paks);
            let (key, paks) = (s.key, s.paks);
            commands.queue(move |w: &mut World| {
                let fx = crate::casting::spawn_pak_on(w, &effects, e, &key, true);
                crate::casting::start_char_pak_sounds(w, &sounds, e, &fx);
                for f in fx {
                    w.entity_mut(f).insert(ToggleFx(paks.clone()));
                }
            });
        }
    }
}

/// The projectile a ranged weapon (`base`: its `basicProjectileName`)
/// fires: the shown weapon enchantment's `projectileName` if it has one
/// (`Weapon::GetProjectileName`; the pak reads it only for a `Ranged1h` /
/// `Ranged2h` weapon, its loader sub_18058D560), else `base`. The player's
/// enchantment counts only while `toggledWeaponEffects` is on.
pub fn weapon_projectile(
    db: &Database,
    base: &str,
    ench: Option<&WeaponEnchantments>,
    player: bool,
    options: &crate::options::GameOptions,
) -> String {
    ench.filter(|_| enchantment_visible(options, player))
        .and_then(|w| w.shown())
        .and_then(|p| db.get(p))
        .and_then(|r| r.str("projectileName").map(str::to_owned))
        .unwrap_or_else(|| base.to_owned())
}

/// Whether the player's weapon enchantment shows (`toggledWeaponEffects`,
/// bool 64; only the main player's is cleared, module docs).
pub fn enchantment_visible(options: &crate::options::GameOptions, player: bool) -> bool {
    !player || options.flag("toggledWeaponEffects")
}

/// Whether a toggled skill's character effect on a character shows
/// (`toggledAurasSelf`, bool 62, on the player; never hidden on others in
/// single player, module docs).
pub fn toggle_fx_visible(
    options: &crate::options::GameOptions,
    fx: &BuffFx,
    owner_is_player: bool,
) -> bool {
    !fx.toggled || !owner_is_player || options.flag("toggledAurasSelf")
}
