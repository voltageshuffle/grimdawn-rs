//! Secondary skills (`SkillSecondary_*` modifiers: Explosive Strike, the
//! chain lightning of Savage Strike, Judgment's flames …): extra effects of
//! their parent skill, not damage modifiers (`Skill.secondarySkills` list).
//!
//! Game.dll: `Skill::ActivateSecondarySkills` (virtual)
//! runs every enabled secondary without `specialSecondaryActivation`
//! of both lists (`Skill.secondarySkills` learned, `Skill.gearSecondarySkills` gear) through the secondary's
//! `ActivateNow` (virtual) with (target, target list, coordinates);
//! `SpecialActivateSecondarySkills` (virtual) the special ones (Leap's
//! landing, Vire's Might's arrival: `Skill_AttackPathCharge::Update`,
//! `ProjectileBase::SpecialSecondaryActivation`); Cadence's finale runs all
//! of them from its `HitAction` whatever it hits
//! (`Skill_WeaponPool_ChargedFinale::AlternateActivateSecondarySkills`).
//! The only check is `Skill::IsSkillEnabled`: no chance roll.
//! `PrimaryActivateSecondarySkills` runs from the parents'
//! `ActivateNow` (the cast: `Skill_AttackRadius`, `…Wave`, `Skill_BuffRadius`,
//! `Skill_BuffSelfDuration`, `…DefensiveWall`, `…Shapeshift`), but only
//! `SkillSecondary_BuffRadius::PrimaryActivateNow` acts there (the base
//! `SkillSecondary::PrimaryActivateNow` just recurses).
//!
//! When (the callers of `ActivateSecondarySkills`): the parents call it from
//! their `TargetResult`, i.e. when the attack resolves on its target list,
//! after the damage, and only with a non-empty list:
//! - weapon attacks (`Skill_AttackWeapon::TargetResult`, the
//!   charges / blinks / `Skill_AttackWeaponRadius` share it): the melee hit
//!   point's targets (`SkillActivated::ExecuteMeleeAttack` sends
//!   no result without one: a target out of reach or an interrupted swing
//!   runs nothing); a pool skill's hit runs the pool parent's
//!   (`Skill_WPAttack_BasicAttack::TargetResult`: Fire Strike's Explosive
//!   Strike); the coordinates are the attacker's;
//! - projectiles (`SkillActivatedSpell::TargetResult` via
//!   `TargetInformation` from `ProjectileBase::ProcessPathTargets`):
//!   at each impact on what it hit, at the projectile's coordinates; a
//!   projectile hitting nothing runs none;
//! - radius attacks (`Skill_AttackRadius::TargetResult`, `…Leap`,
//!   `…Spin`, `…Grow` pulses, `Skill_AttackSpell`, `Skill_AttackWave` (each
//!   band its front sweeps, `waves.rs`),
//!   `Skill_AttackPathCharge` path hits, `ProjectileAreaEffect` ticks): the
//!   foes hit; `Skill_AttackRadius` / `…Leap` then run the special ones with
//!   an empty list whatever they hit;
//! - `Skill_AttackBuff::TargetResult`: once per target, that
//!   target passed explicitly (Lightning Net's tethers);
//! - self buffs / toggles / potions (`Skill_ChargePotion::TargetResult`,
//!   `Skill_BuffSelfToggled::ReceiveActiveUpdate`): on use.
//!
//! The hit / miss roll is later, in `Character::AttackTarget` per target:
//! a dodged swing still runs them. Where each class acts
//! (`ActivateNow`): `AttackRadius` around the
//! explicit target, else the list's first, else the coordinates;
//! `AttackRadiusLightning`, `AttackProjectileRing`, `…AreaEffect`, `…Drop`
//! (`SkillSecondary::GetActivationPoint`) at the explicit target else the coordinates (so a melee
//! parent's ring starts at the attacker, a projectile's at the impact);
//! `ChainLightning` / `ForkLightning` / `ChainBonus` from the list (and the
//! explicit target); `Bonus` on each listed character; `Tether` on the
//! explicit target; `OnKillAttackRadius` / `OnHitBuffRadius` /
//! `BuffRadius` do nothing there (their own triggers below). The port sends
//! [`Activate`] from those moments (`casting.rs`: the hit point's struck
//! list, projectile impacts, area ticks, channel pulses, path charges,
//! charge arrivals), one free [`ProcCast`] per secondary at its own learned
//! level with a shape per class ([`kind`]). The routing by
//! `specialSecondaryActivation` is [`Mode`] (`SkillSecondary::Load`:
//! `RequiresSpecialActivation`): the ordinary pair skips the flagged ones, the
//! special pair runs only them. `Skill_AttackRadius::TargetResult` sends the
//! special pair after the ordinary one whatever it hit (casting `fire.rs`), a
//! `ProjectileHoming` / `ProjectileChain` when it ends, on a hit or at the end of
//! its flight alike (`ProjectileBase::SpecialSecondaryActivation`, called only from
//! `ProjectileHoming::ProjectileHit` / `ProjectileStop`, states 2 / 3;
//! `fly_projectiles`); other projectile classes never run it. The Primary pair ([`Mode::Primary`]) runs at
//! the cast of [`primary_activates`] parents; a `Skill_BuffSelfDuration` runs
//! nothing else ([`primary_only`]), the other buff parents their ordinary
//! ones too. `SkillSecondary_BuffRadius` buffs the caster only and keeps its
//! own duration, the engine's `PrimaryStop` at the parent's end not sent: by
//! design, no stock record has the class (base … gdx3, `survivalmode*`: only
//! the `base_template skills` template); the `DefensiveWall` / `Shapeshift` casts do not
//! run the Primary pair. `AttackInfo.replica` ([`Activate::replica`]) skips a
//! `Skill_AttackWeapon`'s ordinary activation. Writers: only the
//! splash `AttackInfo` that `Skill_AttackWeaponRadius::OnHitTarget` (the class's unnamed vtable
//! `Skill_AttackWeaponRadius::vftable`, registered by name in
//! `Game::RegisterSkillClasses`; radius = the profile's
//! `skillTargetRadius`), of `Skill_WPAttack_Radius` and (crit-only) of `Skill_WPAttack_RadialCrit` builds
//! around a first-level hit. The port's splash hits are plain [`SkillHit`](gd_game_core::casting::SkillHit)s
//! outside the cast's target list (`casting` `fire.rs`: `Skill_AttackWeaponRadius`; a `Skill_WPAttack_Radius`
//! hit, `Kind::TargetRadius`, is its primary hit and splash in one), and the ordinary pair is activated once per
//! cast on that list, i.e. only for the `replica` = 0 result, so a splash starts no secondaries and no second
//! splash. PORT: the splash's target effect (played only when `Skill.suppressTargetEffect` is 0) is not told
//! apart. Nothing builds
//! a `replica` message: `RadialCrit` and a radius `BasicAttack` have no stock user (by design). Cadence's ordinary attacks run none
//! (`Skill_WeaponPool_ChargedFinale::TargetResult` skips them).
//! The on-hit / on-kill ones roll their activation chance
//! (`OnHitSecondaryActivation`, `SkillSecondary_OnKillAttackRadius::OnEnemyDeath`).
//! `OnKillAttackRadius` fires from `CombatManager::ApplyDamage`
//! on kills, `OnHitBuffRadius` from `SkillManager::UnderAttack` (the owner
//! being hit).
//! `SkillSecondary_Tether` (Lightning Net): one tether per target the parent hits
//! (`SkillSecondary_Tether::ActivateNow` tethers that one; a target already
//! tethered gets its duration back; at `tetherLimit` the oldest ends).
//! After `tetherDelay` s it hits every `timeBetweenAttacks` ms
//! divided by the caster's spell cast speed, for `tetherDuration` s from the
//! activation while the target stays within `maxRange` of the caster
//! (`ActivateNow` / `Update`); each hit lands on the foes within
//! `tetherThickness` of the beam, the tethered one only with `hurtsTarget`
//! ([`beam_targets`]); the beam shows after the delay too ([`Tether::delay`],
//! drawn by grimdawn `beams.rs`). `PetModifier`
//! is handled by `pets.rs`.
use crate::combat::{HitLanded, MonsterKilled};
use bevy::prelude::*;
use gd_game_core::casting::{Casting, Kind, ProcCast};
use gd_game_core::data::GameData;
use gd_game_core::secondary::{Tether, Tethers};
use gd_game_core::weapon_pool::SkillLinks;

pub struct SecondaryPlugin;

impl Plugin for SecondaryPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Activate>().add_systems(
            Update,
            // After the frame's combat / casting writers (`HitLanded`, `MonsterKilled`,
            // `Activate`): a hit's procs resolve deterministically after the hit that
            // caused them (`SkillAutoCastController::AppraiseAndExecute`
            // fires inside `Character::AttackTarget` / `UnderAttack`).
            (activate, on_kill, on_hit_taken, tethers)
                .chain()
                .in_set(SecondarySet)
                .after(gd_game_core::combat::CombatSet),
        );
    }
}

/// The secondary systems ([`SecondaryPlugin`]): after the frame's combat /
/// casting writers; `casting::fire_late_procs` fires the procs they send
/// the same frame, after them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecondarySet;

/// Which secondaries a parent's action runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// Ordinary activation: those without `specialSecondaryActivation`.
    Normal,
    /// The special moment (landing, arrival): only the special ones.
    Special,
    /// Cadence's finale: all of them.
    All,
    /// `PrimaryActivateSecondarySkills` at the parent's cast: the ones
    /// without `specialSecondaryActivation`, through `PrimaryActivateNow`
    /// (only `SkillSecondary_BuffRadius` acts there).
    Primary,
}

/// The parent skill `skill` acted: `targets` are the ones its attack
/// resolved on (the first is the primary target), `point` the attack's
/// coordinates (`AttackInfo`: the attacker for a swing, the impact for a
/// projectile, the centre for a radius).
#[derive(Message, Clone, Debug)]
pub struct Activate {
    pub caster: Entity,
    pub skill: String,
    pub targets: Vec<Entity>,
    pub point: Vec3,
    pub mode: Mode,
    /// The parent's level index.
    pub level_index: usize,
    /// `AttackInfo.replica` set: the attack is a forwarded replica and
    /// `Skill_AttackWeapon::TargetResult` does not run the ordinary
    /// secondaries again. No writer (module docs: the port's
    /// splashes are outside the activation); this is the honouring side.
    pub replica: bool,
}

/// Parent classes whose `ActivateNow` calls `PrimaryActivateSecondarySkills`
/// (`Skill_BuffSelfDuration`, `Skill_BuffRadius`, `Skill_AttackRadius`, `Skill_AttackWave`,
/// `Skill_AttackLine`). PORT: `Skill_DefensiveWall` and `Skill_Shapeshift` call it
/// too but their casts do not go through the casting flow.
pub fn primary_activates(class: &str) -> bool {
    matches!(
        class,
        "Skill_BuffSelfDuration"
            | "Skill_BuffSelfSick"
            | "Skill_BuffRadius"
            | "Skill_AttackRadius"
            | "Skill_AttackWave"
            | "Skill_AttackLine"
    )
}

/// Parent classes that run only the Primary pair on the cast, no
/// `ActivateSecondarySkills` (`Skill_BuffSelfDuration::ActivateNow`; its
/// `TargetResult` is the base's; `Skill_BuffSelfSick` is a subclass).
pub fn primary_only(class: &str) -> bool {
    matches!(class, "Skill_BuffSelfDuration" | "Skill_BuffSelfSick")
}

impl Activate {
    /// An attack of `skill` (under the weapon-pool `parent`, if any) resolved
    /// on `targets` at `point`: the parent's ordinary secondaries, if it hit
    /// anything (`TargetResult` → `ActivateSecondarySkills`, see the module
    /// docs). A pool skill runs the pool parent's; an empty parent (Cadence's
    /// ordinary attacks) none.
    pub fn on_hits(
        caster: Entity,
        skill: &str,
        level_index: usize,
        parent: &Option<(String, usize)>,
        targets: Vec<Entity>,
        point: Vec3,
    ) -> Option<Self> {
        let (skill, level_index) = match parent {
            Some((p, l)) => (p.as_str(), *l),
            None => (skill, level_index),
        };
        (!targets.is_empty() && !skill.is_empty()).then(|| Activate {
            caster,
            skill: skill.to_owned(),
            targets,
            point,
            mode: Mode::Normal,
            level_index,
            replica: false,
        })
    }

    /// The special pair of an attack that always runs it, hit or miss
    /// (`SpecialActivateSecondarySkills` with an empty list: `Skill_AttackRadius`
    /// / `…Leap::TargetResult`, `ProjectileBase::SpecialSecondaryActivation`),
    /// at `point`; the pool parent's for a pool skill.
    pub fn special(
        caster: Entity,
        skill: &str,
        level_index: usize,
        parent: &Option<(String, usize)>,
        point: Vec3,
    ) -> Option<Self> {
        let (skill, level_index) = match parent {
            Some((p, l)) => (p.as_str(), *l),
            None => (skill, level_index),
        };
        (!skill.is_empty()).then(|| Activate {
            caster,
            skill: skill.to_owned(),
            targets: Vec::new(),
            point,
            mode: Mode::Special,
            level_index,
            replica: false,
        })
    }

    /// The parent's cast: `PrimaryActivateSecondarySkills` at the caster.
    pub fn primary(caster: Entity, skill: &str, level_index: usize, point: Vec3) -> Self {
        Activate {
            caster,
            skill: skill.to_owned(),
            targets: Vec::new(),
            point,
            mode: Mode::Primary,
            level_index,
            replica: false,
        }
    }
}

/// How a secondary class acts (`None`: not activated by its parent).
/// Shapes, from each class's `ActivateNow` / `TargetResult` in
/// Game.dll: `AttackRadius*` hit around the target (`GetRadiusTime`);
/// `BuffSelfDuration` / `BuffAttackRadiusDuration` buff the caster,
/// `Bonus` its validated target (`ResolveValidateTarget`; the port: the
/// caster), `BuffRadius` everyone friendly in its radius
/// (`GameEngine::GetTargetsInRadius`; the port buffs the caster only);
/// `AttackProjectileRing` / `…Orbiting` launch projectiles,
/// `…AreaEffect` an area projectile; `AttackProjectileDrop` has no code of
/// its own beyond `Skill_AttackProjectileDrop`'s (its vtable uses that
/// class's `GetLaunchPoint`: dropped from above); `ChainLightning` /
/// `ForkLightning` / `ChainBonus` chain from target to target
/// (`GetTargetsInRadius` + `PickObject`, `GrowChain`); `TargetedSpawnPet` /
/// `PetSpawn` summon. `BuffAttackRadiusDrop` (the Fangs undead nemesis'
/// leap rock rain; classInfo `SkillSecondary_BuffAttackRadiusDrop::classInfo`, a `BuffAttackRadiusDuration` with
/// its own vtable `SkillSecondary_BuffAttackRadiusDrop::vftable`) keeps the activation point with
/// `targetingMode` `Point` (`SkillSecondary_BuffAttackRadiusDrop::SetTargetPoint`, put on the floor) and rains
/// like `Skill_BuffAttackRadiusDrop` around it (`SkillSecondary_BuffAttackRadiusDrop::ActivateNow`), a storm
/// (`casting::Storm`). PORT: the point is not pulled back into line of sight
/// / onto the path mesh (`GetPointInLOS`, `FindStraightMovePointOnSlopes`).
pub fn kind(class: &str) -> Option<Kind> {
    let k = class.strip_prefix("SkillSecondary_")?;
    Some(match k {
        "AttackRadius" | "AttackRadiusLightning" => Kind::TargetRadius,
        "BuffSelfDuration" | "BuffRadius" | "BuffAttackRadiusDuration" | "Bonus" => Kind::Buff,
        "AttackProjectileRing" | "AttackProjectileOrbiting" => Kind::Projectile,
        "AttackProjectileAreaEffect" => Kind::Area,
        "AttackProjectileDrop" => Kind::Drop,
        "ChainLightning" | "ForkLightning" | "ChainBonus" => Kind::Chain,
        "BuffAttackRadiusDrop" => Kind::Storm,
        "TargetedSpawnPet" | "PetSpawn" => Kind::Pet,
        _ => return None,
    })
}

/// The cast record of a secondary: its `buffSkillName` if it has one.
fn cast_record(data: &GameData, sec: &str) -> String {
    data.db
        .get(sec)
        .and_then(|r| gd_sim::skills::profile_name(r).map(str::to_owned))
        .unwrap_or_else(|| sec.to_owned())
}

/// The foes a tether's beam hits on a tick (`SkillSecondary_Tether::
/// GetTargets`: `GameEngine::GetTargetsAroundRay`
/// along the beam, `tetherThickness` wide, leaving the tethered target out
/// unless `hurtsTarget`): a foe counts when the ray from `from` to `to` enters
/// its world box (`Combatant::body_box`) grown by the thickness
/// (`gd_sim::bounds::Aabb::around_ray`). PORT: the beam runs between the two
/// characters' positions (the engine: their `emitPointName` points, default
/// `FXCentered`).
pub fn beam_targets(
    from: Vec3,
    to: Vec3,
    target: Entity,
    thickness: f32,
    hurts_target: bool,
    foes: impl Iterator<Item = (Entity, gd_sim::bounds::Aabb)>,
) -> Vec<Entity> {
    let d = (to - from).normalize_or_zero();
    let len = from.distance(to);
    foes.filter(|(e, b)| {
        if *e == target {
            return hurts_target;
        }
        b.around_ray(from.to_array(), d.to_array(), len, thickness)
    })
    .map(|(e, _)| e)
    .collect()
}

#[allow(clippy::type_complexity)]
fn tethers(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &Transform, &mut Tethers, crate::combat::SideData)>,
    foes: Query<
        (
            Entity,
            &Transform,
            &gd_game_core::combat::Combatant,
            crate::combat::VictimSide,
        ),
        Without<gd_game_core::combat::Dead>,
    >,
    mut hits: MessageWriter<gd_game_core::casting::SkillHit>,
) {
    let dt = time.delta_secs();
    for (e, tf, mut t, sd) in &mut q {
        let side = sd.side();
        t.0.retain_mut(|t| {
            t.left -= dt;
            let Ok((_, ft, ..)) = foes.get(t.target) else {
                return false;
            };
            if t.left <= 0.0 || ft.translation.distance(tf.translation) > t.max_range {
                return false;
            }
            if t.delay > 0.0 {
                t.delay -= dt;
                return true;
            }
            t.tick -= dt;
            if t.tick <= 0.0 {
                let near = foes
                    .iter()
                    .filter(|f| f.0 != e && side.hits(&f.3))
                    .map(|f| (f.0, f.2.body_box(&f.1.compute_affine())));
                for target in beam_targets(
                    tf.translation,
                    ft.translation,
                    t.target,
                    t.thickness,
                    t.hurts_target,
                    near,
                ) {
                    hits.write(gd_game_core::casting::SkillHit {
                        attacker: e,
                        target,
                        skill: t.skill.clone(),
                        level_index: t.level_index,
                        parent: None,
                        scale: 1.0,
                        projectile: false,
                    });
                }
                t.tick += t.every;
            }
            true
        });
        if t.0.is_empty() {
            commands.entity(e).try_remove::<Tethers>();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn activate(
    mut commands: Commands,
    mut msgs: MessageReader<Activate>,
    data: Res<GameData>,
    links: Query<&SkillLinks>,
    tfs: Query<&Transform>,
    mut procs: MessageWriter<ProcCast>,
    mut tethered: Query<&mut Tethers>,
    chars: Query<&gd_game_core::character::Character>,
    monsters: Query<(), With<crate::combat::MonsterAi>>,
) {
    for m in msgs.read() {
        let Ok(l) = links.get(m.caster) else { continue };
        // A monster's secondaries hit the player's side.
        let hostile = monsters.contains(m.caster);
        // Wrapper skills cast their `buffSkillName` record: find the parent.
        let low = m.skill.to_ascii_lowercase();
        let key = if l.secondary_parents().any(|p| *p == low) {
            Some(low.clone())
        } else {
            l.secondary_parents()
                .find(|p| {
                    data.db
                        .get(p)
                        .and_then(|r| {
                            gd_sim::skills::profile_name(r).map(|b| b.eq_ignore_ascii_case(&low))
                        })
                        .unwrap_or(false)
                })
                .cloned()
        };
        let Some(key) = key else { continue };
        // `AttackInfo.replica`: a replica attack of a `Skill_AttackWeapon` runs no ordinary
        // ones (`Skill_AttackWeapon::TargetResult`).
        if m.replica
            && matches!(m.mode, Mode::Normal | Mode::All)
            && data
                .db
                .get(&key)
                .is_some_and(|r| r.class.starts_with("Skill_AttackWeapon"))
        {
            continue;
        }
        let list: Vec<(String, usize)> = l.all_secondaries_of(&key).cloned().collect();
        let first = m.targets.first().copied();
        let first_at = first.and_then(|t| tfs.get(t).ok()).map(|t| t.translation);
        for (sec, li) in &list {
            let Some(rec) = data.db.get(sec) else {
                continue;
            };
            if rec.class == "SkillSecondary_Tether" && matches!(m.mode, Mode::Normal | Mode::All) {
                let lv = |k: &str, d: f32| {
                    rec.get(k)
                        .map_or(d, |_| gd_game_core::casting::level_f(rec, k, *li))
                };
                let limit = lv("tetherLimit", 1.0).max(1.0) as usize;
                let speed = chars.get(m.caster).map_or(1.0, |c| c.spell_speed.max(0.1));
                let (dur, delay, range) = (
                    lv("tetherDuration", 5.0),
                    lv("tetherDelay", 0.0),
                    lv("maxRange", 15.0),
                );
                let every = lv("timeBetweenAttacks", 100.0) / 1000.0 / speed;
                let (thickness, hurts_target) = (
                    rec.f32("tetherThickness").unwrap_or(0.0),
                    rec.bool("hurtsTarget"),
                );
                let new: Vec<_> = m
                    .targets
                    .iter()
                    .copied()
                    .take(limit)
                    .map(|target| Tether {
                        target,
                        skill: sec.clone(),
                        level_index: *li,
                        left: dur,
                        tick: 0.0,
                        delay,
                        max_range: range,
                        every: every.max(0.05),
                        thickness,
                        hurts_target,
                    })
                    .collect();
                if new.is_empty() {
                    continue;
                }
                debug!(
                    "tether {} to {} foes",
                    sec.rsplit('/').next().unwrap_or(""),
                    new.len()
                );
                match tethered.get_mut(m.caster) {
                    Ok(mut t) => {
                        t.0.retain(|x| !new.iter().any(|n| n.target == x.target));
                        t.0.extend(new);
                        let excess = t.0.len().saturating_sub(limit);
                        t.0.drain(..excess);
                    }
                    Err(_) => {
                        commands.entity(m.caster).try_insert(Tethers(new));
                    }
                }
                continue;
            }
            let Some(kind) = kind(&rec.class) else {
                continue;
            };
            // `SkillSecondary_BuffRadius` acts only at the parent's cast
            // (`PrimaryActivateNow`); its `ActivateNow` does nothing.
            if (m.mode == Mode::Primary) != (rec.class == "SkillSecondary_BuffRadius") {
                continue;
            }
            // `specialSecondaryActivation` (`RequiresSpecialActivation`)
            // routes the secondary to the special pair; Primary skips them.
            let special = rec.bool("specialSecondaryActivation");
            match m.mode {
                Mode::Normal | Mode::Primary if special => continue,
                Mode::Special if !special => continue,
                _ => {}
            }
            debug!(
                "secondary {} of {}",
                sec.rsplit('/').next().unwrap_or(""),
                m.skill.rsplit('/').next().unwrap_or("")
            );
            let skill = cast_record(&data, sec);
            // Where it acts (module docs): `AttackRadius` and the chains at
            // the first target, the rest at the attack's coordinates.
            let (point, target) = match kind {
                Kind::Chain => (first_at.unwrap_or(m.point), first),
                Kind::TargetRadius if rec.class == "SkillSecondary_AttackRadius" => {
                    (first_at.unwrap_or(m.point), first)
                }
                _ => (m.point, None),
            };
            procs.write(ProcCast {
                caster: m.caster,
                cast: Casting {
                    skill,
                    level_index: *li,
                    kind,
                    point,
                    target,
                    hostile,
                    parent: None,
                },
            });
        }
    }
}

fn chance(rec: &gd_formats::arz::Record, li: usize) -> bool {
    let c = rec.get("onHitActivationChance").map_or(100.0, |_| {
        gd_game_core::casting::level_f(rec, "onHitActivationChance", li)
    });
    c >= 100.0 || gd_game_core::rng::f32() * 100.0 < c
}

/// `SkillSecondary_OnKillAttackRadius`: a kill by the parent skill blasts
/// the radius around the corpse.
fn on_kill(
    mut killed: MessageReader<MonsterKilled>,
    data: Res<GameData>,
    links: Query<&SkillLinks>,
    mut procs: MessageWriter<ProcCast>,
) {
    for k in killed.read() {
        let Some((killer, Some(skill))) = &k.killer else {
            continue;
        };
        let Ok(l) = links.get(*killer) else { continue };
        for (sec, li) in l.all_secondaries_of(&skill.to_ascii_lowercase()) {
            let Some(rec) = data
                .db
                .get(sec)
                .filter(|r| r.class == "SkillSecondary_OnKillAttackRadius")
            else {
                continue;
            };
            if chance(rec, *li) {
                procs.write(ProcCast {
                    caster: *killer,
                    cast: Casting {
                        skill: sec.clone(),
                        level_index: *li,
                        kind: Kind::TargetRadius,
                        point: k.position,
                        target: None,
                        hostile: false,
                        parent: None,
                    },
                });
            }
        }
    }
}

/// `SkillSecondary_OnHitBuffRadius`: the owner being hit may cast the
/// secondary's buff (Spectral Armor's debuff): a `debuff` one on the foes
/// around the attacker, else around the owner
/// (`SkillSecondary_OnHitBuffRadius::OnHitSecondaryActivation`).
fn on_hit_taken(
    mut landed: MessageReader<HitLanded>,
    data: Res<GameData>,
    links: Query<(&SkillLinks, &gd_game_core::player::Loadout, &Transform)>,
    tfs: Query<&Transform>,
    mut procs: MessageWriter<ProcCast>,
) {
    for h in landed.read() {
        let Ok((l, loadout, tf)) = links.get(h.target) else {
            continue;
        };
        for parent in l.secondary_parents() {
            // A toggled parent switched off does nothing.
            if loadout.skills.switched_off(parent) {
                continue;
            }
            for (sec, li) in l.all_secondaries_of(parent) {
                let Some(rec) = data
                    .db
                    .get(sec)
                    .filter(|r| r.class == "SkillSecondary_OnHitBuffRadius")
                else {
                    continue;
                };
                if !chance(rec, *li) {
                    continue;
                }
                let skill = cast_record(&data, sec);
                let (kind, point, target) = match rec.bool("debuff") {
                    true => (
                        Kind::TargetRadius,
                        tfs.get(h.attacker)
                            .map_or(tf.translation, |t| t.translation),
                        Some(h.attacker),
                    ),
                    false => (Kind::Buff, tf.translation, None),
                };
                procs.write(ProcCast {
                    caster: h.target,
                    cast: Casting {
                        skill,
                        level_index: *li,
                        kind,
                        point,
                        target,
                        hostile: false,
                        parent: None,
                    },
                });
            }
        }
    }
}
