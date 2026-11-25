//! Skill effects a character wears while buffs are on it: the weapon
//! enchantment of its buffs and toggles (`weaponEnchantment` →
//! `WeaponEnchantmentPak`: an effect on the weapon, a weapon trail, a tint, a
//! ranged weapon's projectile) and the character effect paks of its toggled
//! skills (`charFxPakSelfNames`).
//!
//! Game.dll: a skill's buff effects go on with `Skill::
//! ApplyBuffSelfEffects` (a toggle switching on, through
//! `ApplyActivatedVisualEffects`) and `Skill::ApplyBuffOtherEffects`
//! (every `SkillBuff::Install`, so also an aura's buff on its
//! own caster): `SkillManager::AddCharFxPak` with the skill's
//! `charFxPakSelfNames` (resp. `charFxPakOtherNames`) entry of its level
//! (level − 1, clamped to the list) plus each learned modifier's
//! `GetBuffSelfCharFxPakName`, then `RemoveWeaponEnchantment(id)` and
//! `AddWeaponEnchantment(id, weaponEnchantment[level − 1])` when that
//! entry is set (`SkillProfile.weaponEnchantment`, read by `SkillProfile::LoadProfile`);
//! switching off removes both. `SkillManager::
//! AddWeaponEnchantment` appends (id, pak) to a list and hands
//! the new pak to `EquipManager::SetWeaponEnchantment`;
//! `RemoveWeaponEnchantment` drops the id's entries and hands
//! over the list's last pak, or "" when it is empty. So a character shows
//! one enchantment, the newest of its buffs still on
//! ([`WeaponEnchantments::shown`]); `SetWeaponEnchantment` gives it to every
//! equipped `Weapon` (`Weapon::SetWeaponEnchantment`, drawn by
//! `grimdawn::fx::enchant`). `Weapon::GetProjectileName`
//! returns the pak's `projectileName` for a ranged weapon
//! ([`enchanted_projectile`]).
//!
//! Visibility: `EquipManager::UpdateWeaponFxVisibility`,
//! called only from `Player::UpdateSelf`, clears the main
//! player's weapon enchantment while the `toggledWeaponEffects` option
//! is off and puts it back when it is on; monsters' and pets'
//! always show. `CharFxPak::UpdateVisibility` and `Skill::
//! UpdateFxVisibility` hide the character effects of an
//! auto-toggle skill (`IsAutoToggle`: the `…Toggled` classes set `Skill_BuffSelfToggled.autoToggle` in
//! their constructors, a `SkillBuff` asks its parent skill) on the main
//! player by the `toggledAurasSelf` option and on another `Player` by
//! `toggledAurasOther`; a pet's or a monster's are never hidden,
//! so in single player `toggledAurasOther` does nothing. `AuraManager::Update`
//! does the same for the auto-toggle skills'
//! `skillActivatedAuraName` loops (see below).
//!
//! The aura loop (`Skill::ApplyActivatedVisualEffects`,
//! called from the toggles' / self buffs' `ReceiveActiveUpdate`): unless the
//! skill's owner is a paper-doll copy (`Player::IsCopy`) it removes the old
//! aura (`Character::RemoveAura`, `Skill.activatedAuraId`) and, switching on, adds the
//! profile's `skillActivatedAuraName` with `Character::AddAura`
//! (`AuraManager::AddAura`: the effect entity attached to the
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
//! Modifier visual overrides ([`gd_game_core::skill_fx::override_mods`] and the resolvers there).
//! Game.dll: every getter takes the last *learned* modifier (only those `IsSkillEnabled`) that sets
//! the field, and only when none does, the last such *gear* modifier (`GetItemSkillModifiers`), else the
//! skill's own field. Per-level lists are indexed by the skill's level (clamped to the list).
//! - `targetFxPakOverride` → `targetFxPakName` (`Skill::GetTargetFX`, [`gd_game_core::skill_fx::target_fx`]).
//! - `projectileOverride` (per-level list) → `skillProjectileName` (`Skill::GetProjectileName`);
//!   `projectileFragmentsOverride` → `projectileFragmentsName` (`Skill::GetProjectileFragmentsName`)
//!   ([`gd_game_core::skill_fx::projectile_name`], [`gd_game_core::skill_fx::fragments_name`]).
//! - `particleEffect1Override` → `particleEffectName1` (`Skill::GetParticleEffect1`,
//!   [`gd_game_core::skill_fx::particle_effect1`]); `lightningOverride` → `lightningName` (`Skill::GetLightning`:
//!   chain / fork / radius lightning, [`gd_game_core::skill_fx::lightning`]).
//! - `projectileFXOverride` → `additionalProjectileFX`, an effect added to a weapon skill's projectile
//!   (`SkillActivated::AddProjectileEffects`; a weapon-pool attack adds its pool parent's
//!   first, `Skill_WPAttack::AddProjectileEffects`) ([`gd_game_core::skill_fx::projectile_fx`]).
//! - `fxChanges` (a list, `Skill::AddModifierFXChanges`, [`gd_game_core::skill_fx::fx_changes`]) replaces what each class
//!   names: the projectile of `Skill_AttackProjectile` / `…Chain` / the secondary area-effect and orbiting
//!   projectiles (`CreateProjectile`: entry at the level over [`gd_game_core::skill_fx::projectile_name`]), the radius family's
//!   `radiusEffectName` (`CreateVisualEffect` of `Skill_BuffRadius` / `AttackBuffRadius` / `OnHitAttackRadius` /
//!   `AttackRadiusTeleport`: entry at the level, [`gd_game_core::skill_fx::radius_effect`]), a beam's `beamName` / `startEffect` /
//!   `endEffect` (`Skill_AttackSpellBeam::ActivateNow`: entries 0–2 when the list has three, [`gd_game_core::skill_fx::beam_effects`]),
//!   a drain's beams (four or more entries: gd-render `chaos_beam.rs`), and the cone's
//!   `coneName` (`Skill_AttackSpellCone::ActivateNow`: entry 0, [`gd_game_core::skill_fx::cone_effect`]), the spin's cast aura
//!   `skillCastAuraName` (`Skill_AttackRadiusSpin::ApplyCastVisualEffects`: entry at the level,
//!   [`gd_game_core::skill_fx::spin_aura`]), the grow's `radiusEffectNames` (`Skill_AttackRadiusGrow::ApplyCastVisualEffects`:
//!   entry at the grow step, [`gd_game_core::skill_fx::grow_effect`]), a chaos skill's `chaosBeamName` (`Skill_AttackSpellChaos::Update`:
//!   entry 0, [`gd_game_core::skill_fx::chaos_beam`]) and the charged-scaling charge effect `skillChargeAura`
//!   (`Skill_WeaponPool_ChargedScaling::SetChargeEffect`: entry at the charge, [`gd_game_core::skill_fx::charge_aura`]); the
//!   effects themselves: `crate::casting::class_fx` (the chaos beams: `crate::casting::chaos`).
use crate::monster_skills::MonsterSkills;
use crate::pets::Pet;
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_formats::vfs::Database;
use gd_game_core::combat::Dead;
use gd_game_core::data::GameData;
use gd_game_core::player::Loadout;
use gd_game_core::skill_buffs::ReceivedBuffs;
use gd_game_core::skill_fx::{ToggleFx, WeaponEnchantments, level_str, timed_aura};
use gd_game_core::visuals::BuffFx;
use gd_game_core::weapon_pool::SkillLinks;
use std::hash::{Hash, Hasher};

pub struct SkillFxPlugin;

impl Plugin for SkillFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_skill_fx.after(crate::auras::AurasSet));
    }
}

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
            key: e.buff.to_string(),
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
    for b in auras.into_iter().flat_map(|a| a.list.iter().rev()) {
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

/// What a character's effect sources are read from: whether it is dead, its buff, aura and toggle names
/// with their levels ([`sync_skill_fx`]'s signature, hashed with the port's hasher every frame).
struct SourceNames<'a> {
    dead: bool,
    loadout: Option<&'a Loadout>,
    auras: Option<&'a ReceivedBuffs>,
    pet: Option<&'a Pet>,
    ms: Option<&'a MonsterSkills>,
}

impl Hash for SourceNames<'_> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.dead.hash(h);
        if let Some(l) = self.loadout {
            hash_names(h, l.buffs.list.iter().map(|b| (b.0.as_str(), b.1)));
        }
        if let Some(a) = self.auras {
            hash_names(h, a.list.iter().map(|b| (b.buff.as_str(), b.level_index)));
        }
        if let Some(p) = self.pet {
            hash_names(h, p.toggled().iter().map(|b| (b.0.as_str(), b.1)));
            hash_names(h, p.buffs().iter().map(|b| (b.0.as_str(), b.1)));
        }
        if let Some(m) = self.ms {
            hash_names(h, m.toggled().iter().map(|b| (b.0.as_str(), b.1)));
            hash_names(h, m.buffs.list.iter().map(|b| (b.0.as_str(), b.1)));
        }
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
    // The toggle effects by owner, gathered once on the first rebuild of the frame (the commands apply after the
    // loop, so the query reads the same all through it): a buff landing on a crowd rebuilds many characters at once.
    let mut by_owner: Option<HashMap<Entity, Vec<(Entity, &str, &Vec<String>)>>> = None;
    for (e, loadout, links, pet, ms, auras, ench, dead) in &mut chars {
        if loadout.is_none() && pet.is_none() && ms.is_none() {
            continue;
        }
        // No buffs, toggles or auras (most characters): the signature is just `dead`, without hashing. The hash of the
        // names has its top bit set, so the two kinds never collide.
        let bare = loadout.is_none_or(|l| l.buffs.list.is_empty())
            && auras.is_none_or(|a| a.list.is_empty())
            && pet.is_none_or(|p| p.toggled().is_empty() && p.buffs().is_empty())
            && ms.is_none_or(|m| m.toggled().is_empty() && m.buffs.list.is_empty());
        let sig = if bare {
            dead as u64
        } else {
            gd_formats::collections::hash_one(SourceNames {
                dead,
                loadout: loadout.as_deref(),
                auras,
                pet,
                ms,
            }) | 1 << 63
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
        let by_owner = by_owner.get_or_insert_with(|| {
            let mut m: HashMap<Entity, Vec<_>> = HashMap::default();
            for (f, b, t) in &fx {
                m.entry(b.owner)
                    .or_default()
                    .push((f, b.skill.as_str(), &t.0));
            }
            m
        });
        let have = by_owner.get(&e).map_or(&[][..], Vec::as_slice);
        for (f, key, paks) in have {
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
                .flat_map(|p| gd_game_core::casting::pak_effects(&data, p))
                .collect();
            // The aura loop sits at the character's origin (empty point, `Skill::ApplyActivatedVisualEffects`).
            effects.extend(s.aura.map(|a| (a, Some(String::new()))));
            let sounds = gd_game_core::casting::char_pak_sounds(&data, &s.paks);
            let (key, paks) = (s.key, s.paks);
            commands.queue(move |w: &mut World| {
                let fx = crate::casting::spawn_pak_on(w, &effects, e, &key, true);
                gd_game_core::casting::start_char_pak_sounds(w, &sounds, e, &fx);
                for f in fx {
                    w.entity_mut(f).insert(ToggleFx(paks.clone()));
                }
            });
        }
    }
}
