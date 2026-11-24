//! Skill-effect data and pure resolvers: a character's weapon enchantments
//! ([`WeaponEnchantments`]), the toggle-effect marker ([`ToggleFx`]) and the
//! modifier visual-override resolvers below. The sync system and the engine
//! (Game.dll) notes are `gd_game::skill_fx`.
//!
//! Modifier visual overrides ([`override_mods`] and the resolvers below):
//! every getter takes the last *learned* modifier (only those `IsSkillEnabled`) that sets
//! the field, and only when none does, the last such *gear* modifier (`GetItemSkillModifiers`), else the
//! skill's own field. Per-level lists are indexed by the skill's level (clamped to the list).
use crate::options::GameOptions;
use crate::visuals::BuffFx;
use crate::weapon_pool::SkillLinks;
use bevy::prelude::*;
use gd_formats::arz::{RawValue, Record};
use gd_formats::vfs::Database;

/// The weapon enchantments of the buffs on a character: (skill or buff
/// record, `WeaponEnchantmentPak` record), oldest first (`gd_game::skill_fx` module docs).
#[derive(Component, Default, Debug)]
pub struct WeaponEnchantments {
    pub list: Vec<(String, String)>,
    pub sig: u64,
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

/// On a [`BuffFx`] that `gd_game::skill_fx` spawned for a toggle's character effects: the paks
/// it was started from.
#[derive(Component)]
pub struct ToggleFx(pub Vec<String>);

/// A per-level string field at level index `li` (clamped to the list;
/// empty = none), as `SkillProfile`'s level tables.
pub fn level_str(rec: &Record, key: &str, li: usize) -> Option<String> {
    let v = rec.get(key)?;
    match v.get(li.min(v.len().checked_sub(1)?))? {
        RawValue::Str(s) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

/// A skill's target effect pak (`Skill::GetTargetFX`): the last learned
/// modifier whose `targetFxPakOverride` (a per-level list, indexed by the skill's level) is set, else the last such
/// gear modifier (`GetItemSkillModifiers`; stock has them, e.g. the Execution / Doom Bolt item modifiers'
/// `wpattack_*_impact_fxpak`), replaces the skill's own `targetFxPakName` (per-level, clamped). `mods`: the enabled
/// gear modifiers, then the learned ones (`Skill_Modifier::IsSkillEnabled`: a gear modifier made exclusive to a
/// learned transmuter is off, `gd_game::weapon_pool::build_links` leaves it out).
pub fn target_fx(rec: &Record, li: usize, mods: &[(Record, usize)]) -> Option<String> {
    mods.iter()
        .rev()
        .find_map(|(m, _)| level_str(m, "targetFxPakOverride", li))
        .or_else(|| level_str(rec, "targetFxPakName", li))
}

/// The modifiers whose visual overrides apply to `skill` (module doc), in the order the resolvers take
/// them: the gear ones, then the learned ones (a caster with a skill book has the learned; one without has
/// no gear). Gear modifiers that are off (`IsSkillEnabled`) are not in the links (`gd_game::weapon_pool::build_links`).
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

/// `Skill_AttackSpellCone::ActivateNow`: the cone effect is `coneName` at `li` (clamped), replaced by
/// the first `fxChanges` entry when a modifier has a list.
pub fn cone_effect(rec: &Record, li: usize, mods: &[(Record, usize)]) -> Option<String> {
    match fx_changes(mods) {
        Some(v) => Some(v[0].clone()).filter(|s| !s.is_empty()),
        None => level_str(rec, "coneName", li),
    }
}

/// `Skill_AttackRadiusSpin::ApplyCastVisualEffects`: the spin's cast aura, the profile's
/// `skillCastAuraName`, replaced by the `fxChanges` entry at `li`.
pub fn spin_aura(rec: &Record, li: usize, mods: &[(Record, usize)]) -> Option<String> {
    fx_change_at(mods, li).or_else(|| own_str(rec, "skillCastAuraName"))
}

/// `Skill_AttackRadiusGrow::ApplyCastVisualEffects`: the grow step of `radius`, the last
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

/// `Skill_WeaponPool_ChargedScaling::SetChargeEffect`: the effect of `charges` charges (none at 0),
/// `SkillProfile::GetChargeAuraName` (`skillChargeAura` at `charges` − 1, clamped) replaced by the `fxChanges`
/// entry there.
pub fn charge_aura(rec: &Record, charges: u32, mods: &[(Record, usize)]) -> Option<String> {
    let i = (charges as usize).checked_sub(1)?;
    fx_change_at(mods, i).or_else(|| level_str(rec, "skillChargeAura", i))
}

/// `Skill_AttackSpellChaos::Update`: each new beam's `ChaosBeam` record, the first `fxChanges` entry
/// when a modifier has a list, else `chaosBeamName`.
pub fn chaos_beam(rec: &Record, mods: &[(Record, usize)]) -> Option<String> {
    match fx_changes(mods) {
        Some(v) => Some(v[0].clone()).filter(|s| !s.is_empty()),
        None => own_str(rec, "chaosBeamName"),
    }
}

/// The looping aura of a timed self buff (the skill record), `None` for any other record (an aura buff's, a toggle's).
pub fn timed_aura(rec: &Record) -> Option<String> {
    (rec.class == "Skill_BuffSelfDuration")
        .then(|| rec.str("skillActivatedAuraName"))
        .flatten()
        .filter(|a| !a.is_empty())
        .map(str::to_owned)
}

/// The projectile a ranged weapon (`base`: its `basicProjectileName`)
/// fires: the shown weapon enchantment's `projectileName` if it has one
/// (`Weapon::GetProjectileName`; the pak reads it only for a `Ranged1h` /
/// `Ranged2h` weapon, its loader `WeaponEnchantmentPak::Load`), else `base`. The player's
/// enchantment counts only while `toggledWeaponEffects` is on.
pub fn weapon_projectile(
    db: &Database,
    base: &str,
    ench: Option<&WeaponEnchantments>,
    player: bool,
    options: &GameOptions,
) -> String {
    ench.filter(|_| enchantment_visible(options, player))
        .and_then(|w| w.shown())
        .and_then(|p| db.get(p))
        .and_then(|r| r.str("projectileName").map(str::to_owned))
        .unwrap_or_else(|| base.to_owned())
}

/// Whether the player's weapon enchantment shows (`toggledWeaponEffects`,
/// only the main player's is cleared, `gd_game::skill_fx` module docs).
pub fn enchantment_visible(options: &GameOptions, player: bool) -> bool {
    !player || options.flag("toggledWeaponEffects")
}

/// Whether a toggled skill's character effect on a character shows
/// (`toggledAurasSelf`, on the player; never hidden on others in
/// single player, `gd_game::skill_fx` module docs).
pub fn toggle_fx_visible(options: &GameOptions, fx: &BuffFx, owner_is_player: bool) -> bool {
    !fx.toggled || !owner_is_player || options.flag("toggledAurasSelf")
}
