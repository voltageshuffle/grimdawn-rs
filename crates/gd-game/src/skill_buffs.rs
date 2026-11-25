//! Buffs and debuffs other characters put on a character: the `SkillBuff` records an aura pass, a curse or a debuff
//! skill lands on it, each kept as one entry ([`ReceivedBuff`]: record, level, modifiers, caster, time left) in
//! [`ReceivedBuffs`]. Its stats count in the holder's profile rebuild ([`ReceivedBuffs::stat_records`]), the buff bar
//! lists it, a debuff's attack runs from it ([`run_buffs`]).
//!
//! How a buff gets there (Game.dll):
//! - `SkillActivated::ApplyBuffOnTarget` fills a `SkillBuffTransfer`: buff name, level, the skill's devotion level
//!   ([`ReceivedBuff::devotion`]), its modifiers (each with its level), duration in ms, caster, applying skill and
//!   item; then `Character::BuffTarget` or, for a debuff, `Character::DebufTarget`.
//! - `Character::DebufTarget`: nothing on an invincible or dead target; else `CreateUpdateSkillBuff`; then, unless
//!   the caster debuffs itself, `Character::UnderAttack(caster)` on the target, an empty `DurationDamageManager`
//!   attack and the caster's `CastingDebuf` (the `CastDebuf` autocast trigger).
//! - Curses: `Skill_AttackBuff::TargetResult` (also `Skill_AttackBuffRadius`): per target `CreateTargetFx`, the
//!   duration = `skillActiveDuration` with its modifiers' × 1000 as int, `ApplyBuffOnTarget(…, GetCurrentLevel,
//!   debuff)`, then `ActivateSecondarySkills` with that target. No attack of its own (the wrapper's profile is the
//!   `buffSkillName` record's, so radius and duration are the buff record's).
//! - Projectile debuffs (webs, nets, Blade Trap): `Skill_AttackProjectileDebuf::TargetResult` the same per target
//!   its projectile hits, without an attack or secondaries.
//! - Aura passes: `Skill_BuffRadius::TargetResult` the same way with 1500 ms; a celestial power's aura carries its
//!   devotion level (pets' and monsters' skills: 0; PORT: not traced further).
//!
//! The rule ([`ReceivedBuffs::land`]; `CreateUpdateSkillBuff`, transfers per character searched by buff name):
//! - none of that name: appended, the buff created ([`Landed::New`]);
//! - one equal in every field (`SkillBuffTransfer::Equals`): `SetTimeToLive` on the running buff
//!   ([`Landed::Refreshed`]);
//! - same caster, other values: the transfer copied over and the buff made anew ([`Landed::Replaced`]);
//! - another caster, equal values but the caster: ignored while the first caster `IsAlive`, else replaced;
//! - another caster, other values: replaced when level + its modifiers' levels + its devotion level exceed the
//!   running one's, else ignored ([`Landed::Ignored`]).
//!
//! A new `SkillBuff` (also a replacing one) goes active: `skillCastSound` at the holder, the record's
//! `charFxPakOtherNames` on another character. It is pushed to the front of the holder's buff list:
//! [`ReceivedBuffs::list`] is newest first. `SkillBuff::Update` runs its time to live down (0 total = endless) and
//! at the end removes it: `skillUnCastSound`, its effects go.
//!
//! Debuffs (`SkillBuff_Debuf` and the `…Trap` / `…Freeze` subclasses): `SkillBuff_Debuf::Update` attacks the holder
//! from the caster every 1000 ms (the timer starts at 0, so a new or replaced debuff attacks on its first update, a
//! refreshed one keeps its cadence): the buff record's combat parameters as the caster's attack, style 4, the
//! multiplier `max(CalculateDamageModifier(ProbabilityToHit(attack OA, holder DA), rand), 1)` (it never misses),
//! `Character::AttackTarget` with its autocast triggers — [`DebuffTick`]. The caster is looked up with no life
//! check, so a dead caster's debuffs keep attacking (its kills included) until its body is removed. A debuff gives
//! its holder only its character / defense attributes: its `offensive*` fields are its attack's.
//!
//! `SkillBuff_DebufRadius::Update` (its own; only with the first expansion loaded, PORT: always): on the same
//! 1000 ms timer, the caster's foes within the buff's `skillTargetRadius` (with its modifiers') of the holder, the
//! holder included, each `Character` among them in line of sight of the holder (`Skill::IsTargetInLOS`; others
//! untested), are attacked from the caster like the holder (the multiplier rolled against the holder's defensive
//! ability: [`DebuffTick::holder`]); the profile's `radiusEffectName` plays at the holder each time, targets or not
//! (PORT: no stock buff has one). It neither traps nor freezes.
//!
//! Projectile debuffs (`SkillBuff_DebufProjectile`: Mark of Darkness, Mark of Doom; only with Fangs of Asterkarn
//! loaded, PORT: always): no debuff attack on the holder; while the holder is not dead a timer (from 0) runs down
//! and at 0 is re-armed to `debufAttackTime` (s → whole ms; ≤ 0: 1000 ms); with the caster and the applying skill
//! still there, the caster launches that skill's projectile from the holder's coordinates (to 1 m above them), one
//! per tick unless `skillProjectileMaximumNumber` live ones (no stock record sets it). The projectile lasts the buff
//! record's `projectileDuration` as whole seconds; stock: a `ProjectileAreaEffect` hitting the caster's foes from
//! the buff record's attack.
//!
//! Contagious debuffs (`SkillBuff_Contageous`: Bloody Pox, the plague item skills, Fetid Pool's devotion): a debuff
//! in stats, buff bar and attack ([`is_debuff`], [`gd_game_core::skill_buffs::attacks`]).
//! - `Install`: the spread timer = `contagionInterval` (whole ms, default 1000), the budget = `contagionMaxSpread`
//!   (default 1); `Character::SetImmuneToPlagueTime(buff, total ms × 1.5 + 4000)` on the holder
//!   ([`plague_immunity_secs`], [`ReceivedBuffs::immune`]; cleared on death).
//! - `Update`: the debuff's attack first; then, while spreading, when the timer runs out or the holder has just died:
//!   timer re-armed; the caster's foes (the holder excluded) within `contagionRadius` (default 2; none at ≤ 0) of the
//!   holder that it sees; nothing if the caster is gone. Up to `contagionLimit` (default 1) of them that neither
//!   hold that buff nor are immune to it get `DebufTarget(caster, …)` with the buff's level, devotion level,
//!   modifiers and applying skill / item, for its `skillActiveDuration` with the modifiers' × 1000 as int; each
//!   infection takes one off the budget, which at ≤ 0 stops spreading for good (the current spread still runs to its
//!   limit). A holder that died then has the buff removed. Infected characters' buffs spread the same way: only the
//!   budget per buff and the immunity bound it.
//!
//! PORT: `SkillBuff.spreading` is taken as set for every new buff; `Skill::ResolveValidateTarget` is not modelled;
//! breakables are no characters; the death spread runs the frame the holder becomes [`Dead`].
//!
//! Dispels ([`DispelOn`]): `SkillManager::DispelSkillBuffs` runs `DispelBuff` on each received `SkillBuff` (only
//! `SkillBuff_Passive*` / `…BuffImmobilize` have one) and `DispellSelfBuff` on the skills of its active, item and
//! sub skill lists. `SkillActivatedBuffSelf::DispellSelfBuff`: unless the profile is `notDispelable`, `StopSkill`,
//! which for `Skill_BuffSelfToggled` (and subclasses: `…Duration`, `Skill_Shapeshift`, `…Colossus`, `…Shield`, …),
//! `Skill_BuffRadius{,Toggled}`, `Skill_BuffAttackRadius{Toggled,Duration,Lightning}`, `Skill_OnHitBuffSelf` /
//! `Skill_OnHitAttackRadius` clears the active flag (`skillDeactivatedSound`): timed self buffs, toggles, damage
//! auras and forms all stop. Every other class has an empty `DispellSelfBuff`, so a dispel leaves it running: the
//! passive procs, the secondaries' buffs, `Skill_BuffOther`, `Skill_GiveBonus`, pets' and passives' skills
//! ([`stopped_by_dispel`]). An item-granted skill stops like a learned one; the sub skills (`Skill_Shapeshift`'s
//! `grantedSkills`) are all attacks, so a dispel stops none of them. A toggle comes back only by a new use, except
//! the main player's: `SkillManager::Update` runs `AutoActivateNow` while no `SkillBuff_DispelMagic` is received,
//! which switches an auto toggle on again ([`auto_toggles`]). `SkillManager::DispelSkillDeBuffs` runs
//! `SkillBuff_Debuf::DispelDeBuff` on each received debuff (contagious ones too); each skips a profile with
//! `notDispelable` (a wrapper's: its `buffSkillName` record's) and removes the buff like running out;
//! `DispelDamageOverTime` clears the damage over time. Callers of `DispelSkillBuffs`: `SkillBuff_DispelMagic::Install`
//! and `Skill_DispelMagic::TargetResult` (on foes).
//! `SkillBuff_DispelMagic` (Nullification, an elixir; `dispelDamageOverTime`, `dispelFriendly`): a plain
//! `SkillBuff` landed like a debuff ([`gd_game_core::skill_buffs::lands_as_debuff`]); `Install` with a `Character`
//! caster: on a foe of the caster `DispelSkillBuffs`, then the caster's `AttackTarget` with the buff's combat
//! parameters (style 4); on a friend (the caster too) `DispelSkillBuffs` with `dispelFriendly`, else
//! `DispelSkillDeBuffs`, and with `dispelDamageOverTime` `DispelDamageOverTime`. It stays for its duration with its
//! stats like any buff.
//! The port's own skills, each by its cast record's class: the player's timed buffs (learned or item-granted, procs
//! included, [`gd_game_core::skill_buffs::TimedBuffs`]), learned and worn-item toggles (into `SkillBook::dispelled`)
//! and form; monsters' and pets' timed buffs and toggles, and anyone's damage auras. A stopped buff-self skill's
//! absorb ends with it ([`stop_shields`]) and a stopped `Skill_BuffSelfImmobilize`'s hold with its last refresh.
//! Death stops every character's timed self buffs the same way (`SkillManager::ImDead`, [`stop_buffs_on_death`]).
//! PORT: the attack's multiplier is the debuff tick's roll.
//!
//! Traps and freezes (`SkillBuff_DebufTrap` / `…Freeze`): `SetTimeToLive(ms)` (creation and refresh) sets the time
//! left and total, then `ModifyDuration` takes the holder's resistance (`defensiveTrap` / `defensiveFreeze`) off the
//! time left when > 0 ([`resisted_ms`]; the total stays); `Install` (new or replaced) plays the holder's stun sound
//! ([`on_created`]). `Update` = the debuff's attack, then every 100 ms while more than 1 ms is left
//! `AddFixedDamage(44 / 45, 1.0, min(0.25 s, time left))` on the holder, so it is trapped (moves not, acts) /
//! frozen for as long as the buff lasts.
//!
//! The transfer's modifiers are the buff's own (`SkillManager::CreateSkillBuff`), so they add to its attack like any
//! skill's: [`DebuffTick::mods`].
//!
//! PORT: the per-debuff random stream is the combat `Rng`; the empty `DurationDamageManager` attack is not made; the
//! player's own buffs on itself stay in `Loadout::buffs` / `SkillBook::stat_sources`.
use crate::afflictions::Afflictions;
use crate::collision::WorldCollision;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::vfs::Database;
use gd_game_core::casting::level_f;
use gd_game_core::character::Character;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::player::ProfileDirty;
use gd_game_core::skill_buffs::{
    DEBUF_PROJECTILE, DEBUFF_TICK_SECS, DISPEL_MAGIC, HOLD_EVERY_SECS, HOLD_SECS, Landed,
    ReceivedBuff, ReceivedBuffs, contagion_profile, is_debuff, plague_immunity_secs,
};
use gd_game_core::visuals::BuffFx;
use gd_sim::attributes::CombatType;
use std::sync::Arc;

pub struct SkillBuffsPlugin;

impl Plugin for SkillBuffsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DebufTarget>()
            .add_message::<DebuffTick>()
            .add_message::<DispelOn>()
            .add_message::<DispelInstall>()
            .add_systems(
                Update,
                (
                    run_buffs,
                    install_dispels,
                    dispel_on,
                    stop_buffs_on_death,
                    auto_toggles,
                )
                    .chain()
                    .after(crate::casting::CastingSet)
                    .before(crate::auras::AurasSet)
                    .before(gd_game_core::combat::CombatSet),
            );
    }
}

/// A `SkillBuff_DebufProjectile`'s launch due.
struct Launch {
    caster: Entity,
    at: Vec3,
    buff: String,
    level_index: usize,
    hostile: bool,
}

/// A received buff `Character::DispelSkillBuffs` removes: the classes with `SkillBuff_Passive::DispelBuff`.
fn dispellable_buff(rec: &Record) -> bool {
    matches!(
        rec.class.as_str(),
        "SkillBuff_Passive"
            | "SkillBuff_PassiveEndless"
            | "SkillBuff_PassiveCharged"
            | "SkillBuff_PassiveShield"
            | "SkillBuff_BuffImmobilize"
    )
}

/// `Character::DebufTarget`: put `buff` on `target`.
#[derive(Message, Clone, Debug)]
pub struct DebufTarget {
    pub caster: Entity,
    pub target: Entity,
    pub buff: String,
    /// The skill applying it.
    pub skill: String,
    pub level_index: usize,
    pub mods: Vec<(String, usize)>,
    pub devotion: u32,
    /// Seconds (the transfer's whole milliseconds).
    pub secs: f32,
}

/// A debuff's attack on its holder, made by `crate::combat` as the caster's `TakeAttack` style 4.
#[derive(Message, Clone, Debug)]
pub struct DebuffTick {
    pub caster: Entity,
    pub target: Entity,
    /// Its defensive ability rolls the multiplier (the target but for a `SkillBuff_DebufRadius`'s other targets).
    pub holder: Entity,
    pub buff: String,
    pub level_index: usize,
    /// The buff's modifiers (the transfer's).
    pub mods: Arc<[(String, usize)]>,
}

/// What a created buff does on landing: `skillCastSound` at the holder, a trap / freeze's stun sound; on another
/// character than the caster, when new, the buff record's (else the skill's) `charFxPakOtherNames`.
pub(crate) fn on_created(
    commands: &mut Commands,
    data: &GameData,
    b: &ReceivedBuff,
    target: Entity,
    at: Vec3,
    put: Landed,
) {
    if let Some(pak) = crate::combat_sounds::buff_cast(data, &b.buff, true) {
        debug!(
            "buff {} (level {}) on {target} -> {pak}",
            b.buff.rsplit('/').next().unwrap_or(""),
            b.level_index + 1
        );
        commands.write_message(gd_game_core::visuals::PlaySound { pak, at: Some(at) });
    }
    // `SkillBuff_DebufTrap::Install` (freezes too): the holder's stun sound.
    if b.hold.is_some() {
        commands.write_message(crate::combat_sounds::StunSound { holder: target });
    }
    // `SkillBuff_DispelMagic::Install`.
    if data
        .db
        .get(&b.buff)
        .is_some_and(|r| r.class == DISPEL_MAGIC)
    {
        commands.write_message(DispelInstall {
            caster: b.from,
            holder: target,
            buff: b.buff.clone(),
            level_index: b.level_index,
            mods: b.mods.clone(),
        });
    }
    if put != Landed::New || target == b.from {
        return;
    }
    let brec = data.db.get(&b.buff);
    let pak = brec
        .as_ref()
        .and_then(|r| {
            r.strs_iter("charFxPakOtherNames")
                .next()
                .map(|p| p.to_string())
        })
        .or_else(|| {
            data.db.get(&b.skill).and_then(|r| {
                r.strs_iter("charFxPakOtherNames")
                    .next()
                    .map(|p| p.to_string())
            })
        });
    if let Some(pak) = pak {
        let toggled = data
            .db
            .get(&b.skill)
            .is_some_and(|r| gd_sim::skills::is_toggle(&r.class));
        crate::casting::attach_buff_fx(commands, data, &pak, target, &b.skill, toggled);
    }
}

type Targets<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        &'static Combatant,
        Option<&'static mut ReceivedBuffs>,
        Option<&'static mut Afflictions>,
    ),
    Without<Dead>,
>;
type Others<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        &'static Combatant,
        crate::combat::VictimSide,
    ),
    Without<Dead>,
>;
/// A debuff caster's side, looked up as any character.
type Bodies<'w, 's> = Query<'w, 's, crate::combat::SideData, With<Combatant>>;

struct Pulse {
    holder: Entity,
    at: Transform,
    buff: ReceivedBuff,
    /// The holder died: its entry is gone, nothing to write back.
    dying: bool,
}

/// Each frame, before the aura passes: buffs run out, debuffs land ([`DebufTarget`]), attack ([`DebuffTick`]) and
/// hold (traps, freezes), contagious ones spread.
#[allow(clippy::too_many_arguments)]
pub fn run_buffs(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut debufs: MessageReader<DebufTarget>,
    mut targets: Targets,
    others: Others,
    bodies: Bodies,
    fx: Query<(Entity, &BuffFx)>,
    mut ticks: MessageWriter<DebuffTick>,
    (chars, coll): (Query<&'static Character>, Res<WorldCollision>),
    (mut dying, breakables, invincible): (
        Query<(Entity, &Transform, &mut ReceivedBuffs), (With<Dead>, Added<Dead>)>,
        Query<(), With<gd_game_core::world::Breakable>>,
        Query<(), With<crate::combat::Invincible>>,
    ),
) {
    let sight = Sight {
        data: &data,
        chars: &chars,
        coll: &coll,
    };
    let dt = time.delta_secs();
    for (e, tf, _, buffs, _) in &mut targets {
        let Some(mut buffs) = buffs else { continue };
        // The timers tick every frame; nothing reads `Changed<ReceivedBuffs>` (the `changed` flag marks real changes).
        let buffs = buffs.bypass_change_detection();
        // `Character::UpdateSelf`: plague immunities run down.
        if !buffs.immune.is_empty() {
            buffs.immune.iter_mut().for_each(|i| i.1 -= dt);
            buffs.immune.retain(|i| i.1 > 0.0);
        }
        buffs
            .list
            .iter_mut()
            .filter(|b| b.total > 0.0)
            .for_each(|b| b.left -= dt);
        // `SkillBuff::Update`: the buff's bonus every second, the first at once.
        for b in buffs.list.iter_mut() {
            b.heal -= dt;
            if b.heal > 0.0 {
                continue;
            }
            b.heal = 1.0;
            if let Some(r) = data.db.get(&b.buff) {
                let mods: Vec<_> = b
                    .mods
                    .iter()
                    .filter_map(|(m, l)| data.db.get(m).map(|r| (r, *l)))
                    .collect();
                let li = b.level_index;
                crate::casting::Bonus::buff_duration(|k| {
                    level_f(r, k, li) + mods.iter().map(|(m, l)| level_f(m, k, *l)).sum::<f32>()
                })
                .queue(&mut commands, e);
            }
        }
        let gone: Vec<ReceivedBuff> = buffs
            .list
            .iter()
            .filter(|b| b.total > 0.0 && b.left <= 0.0)
            .cloned()
            .collect();
        if gone.is_empty() {
            continue;
        }
        buffs.list.retain(|b| b.total <= 0.0 || b.left > 0.0);
        buffs.changed = true;
        // `SkillBuff::ReceiveActiveUpdate` going inactive: `skillUnCastSound`.
        removed(&mut commands, &data, &fx, e, tf.translation, &gone);
    }
    // BTreeMap: the landing order feeds `run_debuffs`' immediate ticks below, so it must be deterministic. Each new
    // holder's list is inserted once, at the end, with everything that landed on it.
    let mut fresh: std::collections::BTreeMap<Entity, ReceivedBuffs> =
        std::collections::BTreeMap::new();
    for d in debufs.read() {
        land(
            &mut commands,
            &data,
            &mut targets,
            &others,
            &invincible,
            &mut fresh,
            d,
        );
    }
    // A debuff landing on a character without buffs ticks at once too.
    let mut pulses = Vec::new();
    let mut launches = Vec::new();
    let mut radius_fx = Vec::new();
    for (&t, list) in fresh.iter_mut() {
        if let Ok((_, tf, _, _, aff)) = targets.get_mut(t) {
            run_debuffs(
                &data.db,
                list,
                t,
                tf,
                0.0,
                aff.map(Mut::into_inner),
                &others,
                &bodies,
                &sight,
                &mut ticks,
                &mut pulses,
                &mut launches,
                &mut radius_fx,
            );
        }
    }
    for (t, tf, _, buffs, aff) in &mut targets {
        if let Some(mut b) = buffs {
            run_debuffs(
                &data.db,
                b.bypass_change_detection(),
                t,
                tf,
                dt,
                aff.map(Mut::into_inner),
                &others,
                &bodies,
                &sight,
                &mut ticks,
                &mut pulses,
                &mut launches,
                &mut radius_fx,
            );
        }
    }
    for (fx, at) in radius_fx {
        gd_game_core::visuals::spawn_fx(&mut commands, &data, &fx, Transform::from_translation(at));
    }
    // The debuffs' projectiles: an area of the buff record's projectile at the holder for its `projectileDuration`
    // (whole s), radius `projectileExplosionRadius`.
    for l in launches {
        let Some(rec) = data.db.get(&l.buff) else {
            continue;
        };
        let dur = level_f(rec, "projectileDuration", l.level_index) as i32 as f32;
        let radius = level_f(rec, "projectileExplosionRadius", l.level_index);
        let cast = gd_game_core::casting::Casting {
            skill: l.buff.clone(),
            level_index: l.level_index,
            kind: gd_game_core::casting::Kind::Area,
            point: l.at,
            target: None,
            hostile: l.hostile,
            parent: None,
        };
        crate::casting::spawn_area(&mut commands, &data, rec, cast, l.caster, dur, radius);
    }
    // `SkillBuff_Contageous::Update` on a holder that died: a spread at once, then `RemoveSkillBuff`; its
    // immunities are cleared.
    for (e, tf, mut buffs) in &mut dying {
        for b in buffs
            .list
            .iter()
            .filter(|b| b.contagion.is_some_and(|c| c.spreading))
        {
            pulses.push(Pulse {
                holder: e,
                at: *tf,
                buff: b.clone(),
                dying: true,
            });
        }
        if buffs.list.iter().any(|b| b.contagion.is_some()) {
            buffs.list.retain(|b| b.contagion.is_none());
            buffs.changed = true;
        }
        buffs.immune.clear();
    }
    let spreads = spread(
        &data.db,
        &mut targets,
        &others,
        &bodies,
        &breakables,
        &sight,
        &fresh,
        pulses,
    );
    for d in &spreads {
        land(
            &mut commands,
            &data,
            &mut targets,
            &others,
            &invincible,
            &mut fresh,
            d,
        );
    }
    for (t, list) in fresh {
        commands.entity(t).try_insert(list);
    }
}

/// `Character::DebufTarget` → `CreateUpdateSkillBuff`: lands `d` on its target (in `fresh` when it has no buffs
/// yet).
fn land(
    commands: &mut Commands,
    data: &GameData,
    targets: &mut Targets,
    others: &Others,
    invincible: &Query<(), With<crate::combat::Invincible>>,
    fresh: &mut std::collections::BTreeMap<Entity, ReceivedBuffs>,
    d: &DebufTarget,
) {
    // `Character::DebufTarget`: nothing on an invincible target.
    if invincible.contains(d.target) {
        return;
    }
    let Ok((t, tf, cb, buffs, _)) = targets.get_mut(d.target) else {
        return;
    };
    let b = ReceivedBuff {
        devotion: d.devotion,
        ..ReceivedBuff::new(
            &data.db,
            &d.buff,
            &d.skill,
            d.level_index,
            d.caster,
            d.mods.clone(),
            d.secs,
        )
    };
    let mut buffs = buffs;
    let list = match buffs.as_deref_mut() {
        Some(list) => list,
        None => fresh.entry(t).or_default(),
    };
    let put = list.land(b.clone(), |c| others.contains(c));
    // A trap / freeze's `SetTimeToLive` (new or refreshed) shortens its time by the holder's resistance.
    if put != Landed::Ignored
        && let Some((ty, _)) = b.hold
        && let Some(entry) = list
            .list
            .iter_mut()
            .find(|x| x.buff.eq_ignore_ascii_case(&b.buff))
    {
        entry.left = resisted_ms(entry.total, resistance(cb, ty)) / 1000.0;
    }
    // `SkillBuff_Contageous::Install`: the holder's immunity to its spread.
    if put.created() && b.contagion.is_some() {
        list.set_immune(&b.buff, plague_immunity_secs(b.total));
    }
    debug!(
        "debuff {} L{} from {} on {t}: {put:?}",
        d.buff.rsplit('/').next().unwrap_or(""),
        d.level_index + 1,
        d.caster
    );
    if put.created() {
        on_created(commands, data, &b, t, tf.translation, put);
    }
}

/// `SkillBuff_Contageous::Update`'s spreads: the infections as [`DebufTarget`]s; the holders' budgets are written
/// back.
#[allow(clippy::too_many_arguments)]
fn spread(
    db: &Database,
    targets: &mut Targets,
    others: &Others,
    bodies: &Bodies,
    breakables: &Query<(), With<gd_game_core::world::Breakable>>,
    sight: &Sight,
    fresh: &std::collections::BTreeMap<Entity, ReceivedBuffs>,
    pulses: Vec<Pulse>,
) -> Vec<DebufTarget> {
    let mut out: Vec<DebufTarget> = Vec::new();
    for p in pulses {
        let b = &p.buff;
        // The caster is looked up as any object: gone, no spread.
        let Ok(csd) = bodies.get(b.from) else {
            continue;
        };
        let caster_side = csd.side();
        let Some(rec) = db.get(&b.buff) else { continue };
        let prof = contagion_profile(rec, b.level_index);
        let mut limit = prof.limit;
        let mut left = b.contagion.map_or(0, |c| c.left);
        // The transfer's duration: `skillActiveDuration` with the modifiers' (whole ms).
        let secs = ((level_f(rec, "skillActiveDuration", b.level_index)
            + mod_sum(db, &b.mods, "skillActiveDuration"))
            * 1000.0) as i32 as f32
            / 1000.0;
        if prof.radius > 0.0 {
            for (t, tt, tcb, v) in others.iter() {
                if limit == 0 {
                    break;
                }
                // Foes, not hiding, no breakable.
                if t == p.holder
                    || !caster_side.hits(&v)
                    || breakables.contains(t)
                    || !tcb.in_radius(&tt.compute_affine(), p.at.translation, prof.radius)
                    || !sight.sees(p.holder, &p.at, t, tt)
                {
                    continue;
                }
                // `Character::IsSkillBuffActive` / `IsImmuneToPlague`.
                let held = targets
                    .get(t)
                    .ok()
                    .and_then(|x| x.3)
                    .or_else(|| fresh.get(&t))
                    .is_some_and(|r| r.has(&b.buff) || r.is_immune(&b.buff))
                    || out
                        .iter()
                        .any(|d| d.target == t && d.buff.eq_ignore_ascii_case(&b.buff));
                if held {
                    continue;
                }
                out.push(DebufTarget {
                    caster: b.from,
                    target: t,
                    buff: b.buff.clone(),
                    skill: b.skill.clone(),
                    level_index: b.level_index,
                    mods: b.mods.clone(),
                    devotion: b.devotion,
                    secs,
                });
                left -= 1;
                limit -= 1;
            }
        }
        if p.dying {
            continue;
        }
        if let Ok((_, _, _, Some(mut list), _)) = targets.get_mut(p.holder)
            && let Some(c) = list
                .list
                .iter_mut()
                .find(|x| x.buff.eq_ignore_ascii_case(&b.buff))
                .and_then(|x| x.contagion.as_mut())
        {
            c.left = left;
            if left <= 0 {
                c.spreading = false;
            }
        }
    }
    out
}

/// Whether `DispelSkillBuffs` stops a character's own skill of `class`: its `DispellSelfBuff` is
/// `SkillActivatedBuffSelf::DispellSelfBuff` with a `StopSkill` that ends something.
pub fn stopped_by_dispel(class: &str) -> bool {
    matches!(
        class,
        "Skill_BuffSelfToggled"
            | "Skill_BuffSelfDuration"
            | "Skill_BuffSelfSick"
            | "Skill_BuffSelfShield"
            | "Skill_BuffSelfInvulnerable"
            | "Skill_BuffSelfImmobilize"
            | "Skill_BuffSelfColossus"
            | "Skill_Shapeshift"
            | "Skill_BuffRadius"
            | "Skill_BuffRadiusToggled"
            | "Skill_BuffAttackRadiusToggled"
            | "Skill_BuffAttackRadiusDuration"
            | "Skill_BuffAttackRadiusLightning"
            | "Skill_BuffAttackRadiusDrop"
            | "Skill_OnHitAttackRadius"
            | "Skill_OnHitBuffSelf"
    )
}

/// What a dispel strips from one character: its buffs (received `SkillBuff_Passive*` and its own timed buffs), its
/// debuffs and its damage over time; none flagged `notDispelable`.
#[derive(Message, Clone, Debug)]
pub struct DispelOn {
    pub target: Entity,
    pub buffs: bool,
    pub debuffs: bool,
    pub dots: bool,
}

/// A `SkillBuff_DispelMagic` was created on `holder`.
#[derive(Message, Clone, Debug)]
pub struct DispelInstall {
    pub caster: Entity,
    pub holder: Entity,
    pub buff: String,
    pub level_index: usize,
    pub mods: Vec<(String, usize)>,
}

/// `SkillBuff_DispelMagic::Install`: on a foe of the caster its buffs go and the caster attacks it with the buff
/// (style 4); on a friend (the caster too) its debuffs go, or with `dispelFriendly` its buffs, and with
/// `dispelDamageOverTime` its damage over time.
fn install_dispels(
    data: Res<GameData>,
    mut installs: MessageReader<DispelInstall>,
    sides: Bodies,
    mut out: MessageWriter<DispelOn>,
    mut ticks: MessageWriter<DebuffTick>,
) {
    for i in installs.read() {
        let (Ok(csd), Ok(hsd)) = (sides.get(i.caster), sides.get(i.holder)) else {
            continue;
        };
        let Some(rec) = data.db.get(&i.buff) else {
            continue;
        };
        if i.caster != i.holder && csd.side().foe(hsd.side_ref()) {
            out.write(DispelOn {
                target: i.holder,
                buffs: true,
                debuffs: false,
                dots: false,
            });
            ticks.write(DebuffTick {
                caster: i.caster,
                target: i.holder,
                holder: i.holder,
                buff: i.buff.clone(),
                level_index: i.level_index,
                mods: Arc::from(i.mods.as_slice()),
            });
        } else {
            let friendly = rec.bool("dispelFriendly");
            out.write(DispelOn {
                target: i.holder,
                buffs: friendly,
                debuffs: !friendly,
                dots: rec.bool("dispelDamageOverTime"),
            });
        }
    }
}

/// Applies [`DispelOn`]: removed received buffs end like run-out ones; the character's own buff-self skills stop
/// (`skillDeactivatedSound`): timed buffs, toggles (the player's into `SkillBook::dispelled`, see
/// [`auto_toggles`]), damage auras and the player's form.
#[allow(clippy::type_complexity)]
fn dispel_on(
    mut commands: Commands,
    data: Res<GameData>,
    mut events: MessageReader<DispelOn>,
    mut q: Query<(
        &Transform,
        Option<&mut ReceivedBuffs>,
        Option<&mut crate::monster_skills::MonsterSkills>,
        Option<&mut Afflictions>,
        Option<&mut gd_game_core::player::Loadout>,
        Option<&mut crate::pets::Pet>,
        Option<&mut gd_game_core::casting::DamageAuras>,
        Option<&crate::shapeshift::Shapeshifted>,
    )>,
    fx: Query<(Entity, &BuffFx)>,
    mut forms: MessageWriter<crate::shapeshift::Toggle>,
) {
    // A skill of a class whose `DispellSelfBuff` does nothing, or with a `notDispelable` profile, keeps running.
    let keep = |name: &str| {
        data.db.get(name).is_some_and(|r| {
            !stopped_by_dispel(&r.class)
                || gd_sim::skills::profile_name(r)
                    .and_then(|p| data.db.get(p))
                    .unwrap_or(r)
                    .bool("notDispelable")
        })
    };
    for d in events.read() {
        let Ok((tf, received, ms, aff, loadout, pet, damage, form)) = q.get_mut(d.target) else {
            continue;
        };
        if let Some(mut list) = received {
            let strip = |b: &ReceivedBuff| {
                data.db.get(&b.buff).is_some_and(|r| {
                    !r.bool("notDispelable")
                        && ((d.buffs && dispellable_buff(r)) || (d.debuffs && is_debuff(r)))
                })
            };
            let gone: Vec<ReceivedBuff> = list.list.iter().filter(|b| strip(b)).cloned().collect();
            if !gone.is_empty() {
                list.list.retain(|b| !strip(b));
                list.changed = true;
                removed(&mut commands, &data, &fx, d.target, tf.translation, &gone);
            }
        }
        if d.buffs {
            let mut stopped: Vec<String> = Vec::new();
            if let Some(mut ms) = ms {
                stopped.extend(ms.dispel(&data.db, keep));
            }
            if let Some(mut p) = pet {
                stopped.extend(p.dispel(keep));
            }
            if let Some(mut a) = damage {
                for s in a.dispel(keep) {
                    if !stopped.contains(&s) {
                        stopped.push(s);
                    }
                }
            }
            let mut timed: Vec<String> = stopped.clone();
            if let Some(mut l) = loadout {
                // A form entry stays: it leaves with the form, not by dispel.
                for b in l
                    .buffs
                    .list
                    .iter_mut()
                    .filter(|b| b.2.is_finite() && !keep(&b.0) && b.2 > 0.0)
                {
                    timed.push(b.0.clone());
                    b.2 = 0.0;
                }
                let on: Vec<String> = l
                    .skills_and_item_toggles(&data.db)
                    .iter()
                    .chain(l.granted.iter())
                    .filter(|(s, lv)| {
                        *lv > 0
                            && !keep(s)
                            && l.skill_active(&data.db, s)
                            && data
                                .db
                                .get(s)
                                .is_some_and(|r| gd_sim::skills::is_toggle(&r.class))
                    })
                    .map(|(s, _)| s.clone())
                    .collect();
                if !on.is_empty() {
                    debug!("dispel: {} toggles off", on.len());
                    l.skills
                        .dispelled
                        .extend(on.iter().map(|s| s.to_ascii_lowercase()));
                    commands.entity(d.target).try_insert(ProfileDirty);
                    for s in on {
                        if !stopped.contains(&s) {
                            stopped.push(s);
                        }
                    }
                }
            }
            stop_shields(&mut commands, &data, d.target, &timed);
            if let Some(f) = form.filter(|f| !keep(&f.skill)) {
                forms.write(crate::shapeshift::Toggle {
                    caster: d.target,
                    skill: f.skill.clone(),
                    level_index: 0,
                });
                stopped.push(f.skill.clone());
            }
            for pak in stopped
                .iter()
                .filter_map(|b| crate::combat_sounds::deactivate(&data, b))
            {
                commands.write_message(gd_game_core::visuals::PlaySound {
                    pak,
                    at: Some(tf.translation),
                });
            }
        }
        if d.dots
            && let Some(mut a) = aff
        {
            a.clear_dots();
        }
    }
}

/// A stopped `SkillActivatedBuffSelf` skill (`Skill_BuffSelfToggled::StopSkill`) clears its combat filter's on byte
/// and its active flag, so a stopped skill with an absorb drops its own buff filter from the caster's
/// [`gd_game_core::casting::Shields`] (not a potion's: `Skill_ChargePotion`'s `StopSkill` is empty). A stopped
/// `Skill_BuffSelfImmobilize` refreshes its hold no more: the last refresh runs out.
fn stop_shields(commands: &mut Commands, data: &GameData, target: Entity, stopped: &[String]) {
    let absorbs = |r: &Record| {
        ["damageAbsorptionPercent", "damageAbsorption"]
            .iter()
            .any(|k| r.f32s_iter(k).any(|v| v > 0.0))
    };
    let shields: Vec<String> = stopped
        .iter()
        .filter(|s| {
            data.db.get(s).is_some_and(|r| {
                absorbs(r)
                    || gd_sim::skills::profile_name(r)
                        .and_then(|p| data.db.get(p))
                        .is_some_and(absorbs)
            })
        })
        .cloned()
        .collect();
    if !shields.is_empty() {
        commands.queue(move |w: &mut World| {
            if let Some(mut sh) = w.get_mut::<gd_game_core::casting::Shields>(target) {
                sh.0.retain(|f| {
                    f.kind != gd_game_core::casting::ShieldKind::Buff
                        || !shields.iter().any(|s| s.eq_ignore_ascii_case(&f.skill))
                });
            }
        });
    }
}

/// A character's own timed buffs ended (run out, dispelled, stopped by death): `skillDeactivatedSound` at the caster
/// and the effects they put on it go. The caller marks the profile dirty.
pub(crate) fn timed_buffs_ended(
    commands: &mut Commands,
    data: &GameData,
    fx: &Query<(Entity, &BuffFx)>,
    e: Entity,
    at: Vec3,
    gone: &[String],
) {
    for pak in gone
        .iter()
        .filter_map(|g| crate::combat_sounds::deactivate(data, g))
    {
        commands.write_message(gd_game_core::visuals::PlaySound { pak, at: Some(at) });
    }
    for (f, b) in fx {
        if b.owner == e && gone.contains(&b.skill) {
            commands.entity(f).try_despawn();
        }
    }
}

/// Death stops the character's own timed buffs: `SkillManager::ImDead` `StopSkill`s every active-list and item-list
/// skill, which ends a timed self buff as a dispel does: `skillDeactivatedSound`, its effects and absorb go, the
/// profile rebuilds. The same for monsters' and pets' lists. A form entry stays: it leaves with the form.
#[allow(clippy::type_complexity)]
fn stop_buffs_on_death(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            &Transform,
            Option<&mut gd_game_core::player::Loadout>,
            Option<&mut crate::monster_skills::MonsterSkills>,
            Option<&mut crate::pets::Pet>,
        ),
        Added<Dead>,
    >,
    fx: Query<(Entity, &BuffFx)>,
) {
    for (e, tf, loadout, ms, pet) in &mut q {
        let gone = if let Some(mut l) = loadout {
            let gone = l.bypass_change_detection().buffs.stop_timed();
            if !gone.is_empty() {
                l.set_changed();
                commands.entity(e).try_insert(ProfileDirty);
            }
            gone
        } else if let Some(mut m) = ms {
            let gone = m.bypass_change_detection().buffs.stop_timed();
            if !gone.is_empty() {
                m.mark_profile_dirty();
            }
            gone
        } else if let Some(mut p) = pet {
            let gone = p.bypass_change_detection().buffs_mut().stop_timed();
            if !gone.is_empty() {
                p.mark_profile_dirty();
            }
            gone
        } else {
            continue;
        };
        if gone.is_empty() {
            continue;
        }
        debug!("{e}: death stops {} timed buffs", gone.len());
        stop_shields(&mut commands, &data, e, &gone);
        timed_buffs_ended(&mut commands, &data, &fx, e, tf.translation, &gone);
    }
}

/// The main player's toggles, learned and item ones (Game.dll): `SkillManager::Update` runs, unless a
/// `SkillBuff_DispelMagic` is received, each active-list and item-list skill's `AutoActivateNow`, which switches a
/// toggle on when it is off, enabled, the character alive, an auto toggle (every toggle is), its weapons and hand
/// state qualify and `ExclusiveSkillCheck` passes. So a toggle is on whenever it can be: there is no user
/// switch-off, a newly learned or loaded one comes on at the next update (the save's active flag does not matter),
/// one stopped by weapons, death or a dispel comes back once that is over, with its activate effect.
/// - death: `SkillManager::ImDead` `StopSkill`s every active-list and item-list skill; the alive test keeps them
///   off until the respawn.
/// - exclusive skills (`exclusiveSkill`): one going active becomes the manager's exclusive skill, stopping the one
///   before; `Skill::ExclusiveSkillCheck` lets another exclusive come on only with a higher
///   [`gd_sim::skills::ExclusiveRank`] (points spent, then mastery enumeration, then item bonus). So the highest one
///   runs. An item toggle's level is its granted level with no augment. PORT: several on (a load) keep the first of
///   the book that no later one beats.
#[allow(clippy::type_complexity)]
fn auto_toggles(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            Option<&Transform>,
            &mut gd_game_core::player::Loadout,
            Option<Ref<ReceivedBuffs>>,
            Option<Ref<gd_game_core::weapon_pool::SkillLinks>>,
            Has<Dead>,
        ),
        With<gd_game_core::player::Player>,
    >,
    mut was_dead: Local<bool>,
) {
    let db = &data.db;
    for (e, tf, mut l, received, links, dead) in &mut q {
        // Only the book and worn items, the weapons, a dispel and being dead decide a toggle, so an idle frame skips
        // the pass. The dispel test is the `changed` flag, not Bevy's change detection: `run_buffs` ticks the buff
        // timers every frame.
        let was = std::mem::replace(&mut *was_dead, dead);
        if !dead
            && !was
            && !l.is_changed()
            && !links.as_ref().is_some_and(|k| k.is_changed())
            && !received.as_ref().is_some_and(|r| r.changed)
        {
            continue;
        }
        // (record, profile record, learned, item level) of each toggle the player has.
        let toggles: Vec<(String, Record, bool, u32)> = l
            .skills
            .skills
            .iter()
            .filter(|(_, lv)| *lv > 0)
            .map(|(s, _)| (s.clone(), true, 0))
            .chain(l.item_toggles(db).into_iter().map(|(s, lv)| (s, false, lv)))
            .filter_map(|(s, learned, lv)| {
                db.get_owned(&s)
                    .filter(|r| gd_sim::skills::is_toggle(&r.class))
                    .map(|r| (s, r, learned, lv))
            })
            .collect();
        let is_on = |l: &gd_game_core::player::Loadout, s: &str| {
            !l.skills.switched_off(&s.to_ascii_lowercase())
        };
        let mut stopped: Vec<String> = Vec::new();
        let mut changed = false;
        if dead {
            stopped.extend(
                toggles
                    .iter()
                    .filter(|t| is_on(&l, &t.0))
                    .map(|t| t.0.clone()),
            );
        } else if !received.as_ref().is_some_and(|r| {
            r.list
                .iter()
                .any(|b| db.get(&b.buff).is_some_and(|r| r.class == DISPEL_MAGIC))
        }) {
            // The flag sits on the buff record (`auraconviction1_buff`).
            let exclusive = |s: &str| {
                gd_sim::skills::profile_record(db, s).is_some_and(|r| r.bool("exclusiveSkill"))
            };
            let trees = data.skill_trees();
            let book = l.effective_skills(db, &trees);
            let ranks: Vec<gd_sim::skills::ExclusiveRank> = toggles
                .iter()
                .map(|(s, _, learned, item)| gd_sim::skills::ExclusiveRank {
                    level: if *learned { l.skills.level(s) } else { *item },
                    mastery: trees.mastery_enumeration(s),
                    augment: if *learned {
                        book.bonus
                            .get(&s.to_ascii_lowercase())
                            .copied()
                            .unwrap_or(0)
                    } else {
                        0
                    },
                })
                .collect();
            let rank = |s: &str| {
                toggles
                    .iter()
                    .position(|t| t.0 == s)
                    .map(|i| ranks[i])
                    .expect("a listed toggle")
            };
            // The exclusive skill on (several only from a load: the best stays).
            let mut current: Option<(String, gd_sim::skills::ExclusiveRank)> = None;
            for (s, _, _, _) in toggles
                .iter()
                .filter(|t| exclusive(&t.0) && is_on(&l, &t.0))
            {
                let lv = rank(s);
                match &current {
                    Some((c, clv)) if lv.beats(clv) => {
                        stopped.push(c.clone());
                        current = Some((s.clone(), lv));
                    }
                    Some(_) => stopped.push(s.clone()),
                    None => current = Some((s.clone(), lv)),
                }
            }
            for (s, r, learned, _) in &toggles {
                let low = s.to_ascii_lowercase();
                if is_on(&l, s) || stopped.iter().any(|x| x.eq_ignore_ascii_case(s)) {
                    continue;
                }
                // PORT: item toggles' weapons are not tested.
                if *learned && links.as_ref().is_some_and(|k| !k.weapons_allow(db, r, s)) {
                    continue;
                }
                if exclusive(s) {
                    let lv = rank(s);
                    match &current {
                        Some((_, clv)) if !lv.beats(clv) => continue,
                        Some((c, _)) => stopped.push(c.clone()),
                        None => {}
                    }
                    current = Some((s.clone(), lv));
                }
                l.skills.inactive.remove(&low);
                l.skills.dispelled.remove(&low);
                debug!("auto toggle: {s} on");
                changed = true;
            }
        }
        for s in &stopped {
            l.skills.inactive.insert(s.to_ascii_lowercase());
            if let Some(pak) = crate::combat_sounds::deactivate(&data, s) {
                commands.write_message(gd_game_core::visuals::PlaySound {
                    pak,
                    at: tf.map(|t| t.translation),
                });
            }
            debug!("toggle {s} stopped (death / exclusive)");
        }
        if changed || !stopped.is_empty() {
            commands.entity(e).try_insert(ProfileDirty);
        }
    }
}

/// Received buffs that ended (ran out, dispelled): `skillUnCastSound` at the holder, the effects they put on it go.
fn removed(
    commands: &mut Commands,
    data: &GameData,
    fx: &Query<(Entity, &BuffFx)>,
    e: Entity,
    at: Vec3,
    gone: &[ReceivedBuff],
) {
    for pak in gone
        .iter()
        .filter_map(|g| crate::combat_sounds::buff_cast(data, &g.buff, false))
    {
        commands.write_message(gd_game_core::visuals::PlaySound { pak, at: Some(at) });
    }
    for (f, b) in fx {
        if b.owner == e && gone.iter().any(|g| g.from != e && g.skill == b.skill) {
            commands.entity(f).try_despawn();
        }
    }
}

/// The modifiers' values of `field` (`GetSkillModifierData`).
fn mod_sum(db: &Database, mods: &[(String, usize)], field: &str) -> f32 {
    mods.iter()
        .filter_map(|(m, li)| db.get(m).map(|m| level_f(m, field, *li)))
        .sum()
}
/// `SkillBuff_DebufTrap::GetResistance`: the sum of the holder's defense attributes of that type.
fn resistance(cb: &Combatant, ty: CombatType) -> f32 {
    cb.profile
        .resistances
        .iter()
        .filter(|r| r.0 == ty)
        .map(|r| r.1)
        .sum()
}

/// `SkillBuff_DebufTrap::ModifyDuration`: `secs` (whole ms) less `res` % when `res` > 0, as whole ms.
pub fn resisted_ms(secs: f32, res: f32) -> f32 {
    let ms = (secs * 1000.0).round();
    if res > 0.0 {
        (ms - ms * res * 0.01) as i32 as f32
    } else {
        ms
    }
}

/// Runs the debuffs' attack and hold timers on `holder` at `at`.
#[allow(clippy::too_many_arguments)]
fn run_debuffs(
    db: &Database,
    buffs: &mut ReceivedBuffs,
    holder: Entity,
    tf: &Transform,
    dt: f32,
    mut aff: Option<&mut Afflictions>,
    others: &Others,
    bodies: &Bodies,
    sight: &Sight,
    out: &mut MessageWriter<DebuffTick>,
    pulses: &mut Vec<Pulse>,
    launches: &mut Vec<Launch>,
    radius_fx: &mut Vec<(String, Vec3)>,
) {
    let at = tf.translation;
    let holder_at = others.get(holder).ok().map(|o| *o.1);
    for b in buffs.list.iter_mut() {
        if let Some((ty, t)) = b.hold.as_mut() {
            *t -= dt;
            if *t <= 0.0 && b.left > 0.001 {
                *t = HOLD_EVERY_SECS;
                if let Some(a) = aff.as_deref_mut() {
                    a.hold(*ty, HOLD_SECS.min(b.left));
                }
            }
        }
        // `SkillBuff_Contageous::Update` after the debuff's: every `contagionInterval` while it spreads.
        if let Some(c) = b.contagion.as_mut() {
            c.next -= dt;
            if c.spreading && c.next <= 0.0 {
                c.next = db
                    .get(&b.buff)
                    .map_or(1.0, |r| contagion_profile(r, b.level_index).interval);
                pulses.push(Pulse {
                    holder,
                    at: *tf,
                    buff: b.clone(),
                    dying: false,
                });
            }
        }
        let Some(t) = b.tick.as_mut() else { continue };
        *t -= dt;
        if *t > 0.0 {
            continue;
        }
        *t = DEBUFF_TICK_SECS;
        let Ok(csd) = bodies.get(b.from) else {
            continue;
        };
        let caster_side = csd.side();
        let Some(rec) = db.get(&b.buff) else { continue };
        if rec.class == DEBUF_PROJECTILE {
            // `SkillBuff_DebufProjectile::Update`: every `debufAttackTime`.
            let every = rec.f32("debufAttackTime").unwrap_or(0.0);
            *t = if every > 0.0 {
                (every * 1000.0) as i32 as f32 / 1000.0
            } else {
                DEBUFF_TICK_SECS
            };
            launches.push(Launch {
                caster: b.from,
                at,
                buff: b.buff.clone(),
                level_index: b.level_index,
                hostile: caster_side.hostile(),
            });
            continue;
        }
        if rec.class != "SkillBuff_DebufRadius" {
            out.write(DebuffTick {
                caster: b.from,
                target: holder,
                holder,
                buff: b.buff.clone(),
                level_index: b.level_index,
                mods: Arc::from(b.mods.as_slice()),
            });
            continue;
        }
        // `SkillBuff_DebufRadius::Update`: the profile's `radiusEffectName` at the holder, then the caster's foes
        // within the buff's `skillTargetRadius` (with its modifiers') of the holder, the holder included, that the
        // holder sees.
        if let Some(fx) = radius_effect(rec, b.level_index) {
            radius_fx.push((fx, at));
        }
        let r = level_f(rec, "skillTargetRadius", b.level_index)
            + mod_sum(db, &b.mods, "skillTargetRadius");
        let mods: Arc<[(String, usize)]> = Arc::from(b.mods.as_slice());
        for (t, tt, tcb, v) in others.iter() {
            if caster_side.hits(&v)
                && t != b.from
                && tcb.in_radius(&tt.compute_affine(), at, r)
                && holder_at.is_none_or(|h| sight.sees(holder, &h, t, tt))
            {
                out.write(DebuffTick {
                    caster: b.from,
                    target: t,
                    holder,
                    buff: b.buff.clone(),
                    level_index: b.level_index,
                    mods: mods.clone(),
                });
            }
        }
    }
}

/// A `SkillBuff_DebufRadius` profile's `radiusEffectName` for a level (clamped to the list's last entry; an empty
/// name plays nothing).
fn radius_effect(rec: &gd_formats::arz::Record, level_index: usize) -> Option<String> {
    rec.strs_iter("radiusEffectName")
        .nth(level_index)
        .or_else(|| rec.strs_iter("radiusEffectName").last())
        .filter(|f| !f.is_empty())
        .map(|f| f.to_string())
}

/// What a holder's `IsTargetInLOS` needs.
struct Sight<'a, 'w, 's> {
    data: &'a GameData,
    chars: &'a Query<'w, 's, &'static Character>,
    coll: &'a WorldCollision,
}

impl Sight<'_, '_, '_> {
    /// `Skill::IsTargetInLOS(holder, target)`; a target that is no character is not tested.
    fn sees(&self, holder: Entity, at: &Transform, target: Entity, tat: &Transform) -> bool {
        let (Ok(ch), Ok(tch)) = (self.chars.get(holder), self.chars.get(target)) else {
            return true;
        };
        let (head, aim) = crate::monster_skills::sight_ray(self.data, ch, at, tat, Some(tch));
        self.coll.los_clear(head, aim)
    }
}

/// Marks the player [`ProfileDirty`] when its received buffs changed.
pub(crate) fn mark_player(
    commands: &mut Commands,
    e: Entity,
    buffs: Option<Mut<ReceivedBuffs>>,
    player: bool,
) {
    if player
        && let Some(mut b) = buffs
        && b.take_changed()
    {
        commands.entity(e).try_insert(ProfileDirty);
    }
}
