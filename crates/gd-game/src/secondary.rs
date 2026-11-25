//! Secondary skills (`SkillSecondary_*` modifiers: Explosive Strike, Savage Strike's chain
//! lightning, Judgment's flames …): extra effects of their parent skill.
//!
//! `Skill::ActivateSecondarySkills` runs every enabled secondary without
//! `specialSecondaryActivation` of both lists (learned and gear) through its `ActivateNow` with
//! (target, target list, coordinates); `SpecialActivateSecondarySkills` runs the special ones
//! (Leap's landing, Vire's Might's arrival); Cadence's finale runs all of them whatever it hits
//! (`AlternateActivateSecondarySkills`). No chance roll, only `Skill::IsSkillEnabled`.
//! `PrimaryActivateSecondarySkills` runs at the parents' cast, but only
//! `SkillSecondary_BuffRadius::PrimaryActivateNow` acts there.
//!
//! When: the parents' `TargetResult`, after the damage, only with a non-empty list:
//! - weapon attacks: the melee hit point's targets (an out-of-reach target or interrupted swing
//!   runs nothing); a pool skill runs the pool parent's; coordinates are the attacker's;
//! - projectiles: each impact on what it hit, at the projectile;
//! - radius attacks (`Skill_AttackRadius`, `…Leap`, `…Spin`, `…Grow` pulses, `Skill_AttackSpell`,
//!   `Skill_AttackWave` bands, path charge hits, `ProjectileAreaEffect` ticks): the foes hit;
//!   `Skill_AttackRadius` / `…Leap` then run the special ones with an empty list whatever they hit;
//! - `Skill_AttackBuff::TargetResult`: once per target, passed explicitly (Lightning Net's tethers);
//! - self buffs / toggles / potions: on use.
//!
//! The hit / miss roll is later (`Character::AttackTarget`): a dodged swing still runs them. Where
//! each class acts (`ActivateNow`): `AttackRadius` around the explicit target, else the list's
//! first, else the coordinates; `AttackRadiusLightning`, `AttackProjectileRing`, `…AreaEffect`,
//! `…Drop` at the explicit target else the coordinates; the chains from the list; `Bonus` on each
//! listed character; `Tether` on the explicit target; `OnKillAttackRadius` / `OnHitBuffRadius` /
//! `BuffRadius` have their own triggers. [`Activate`] carries those moments; each secondary is one
//! free [`ProcCast`] at its own learned level, shaped by [`kind`]. [`Mode`] is the
//! `specialSecondaryActivation` routing. `ProjectileHoming` / `ProjectileChain` run the special
//! pair when they end, hit or not; other projectile classes never do. The Primary pair runs at the
//! cast of [`primary_activates`] parents; [`primary_only`] ones run nothing else.
//! `SkillSecondary_BuffRadius` buffs the caster only and keeps its own duration (no stock record
//! uses the class). `AttackInfo.replica` skips a `Skill_AttackWeapon`'s ordinary activation; the
//! port's weapon splashes are plain [`SkillHit`](gd_game_core::casting::SkillHit)s outside the
//! cast's target list, so a splash starts no secondaries and no second splash. PORT: the splash's
//! target effect is not told apart. Cadence's ordinary attacks run none.
//! On-hit / on-kill ones roll their chance: `OnKillAttackRadius` on kills
//! (`CombatManager::ApplyDamage`), `OnHitBuffRadius` when the owner is hit
//! (`SkillManager::UnderAttack`).
//! `SkillSecondary_Tether` (Lightning Net): one tether per target hit (re-tethering restores the
//! duration; at `tetherLimit` the oldest ends). After `tetherDelay` s it hits every
//! `timeBetweenAttacks` ms / spell cast speed for `tetherDuration` s from the activation while the
//! target stays within `maxRange`; each hit lands on the foes within `tetherThickness` of the beam,
//! the tethered one only with `hurtsTarget` ([`beam_targets`]); the beam shows after the delay.
//! `PetModifier` is `pets.rs`'s.
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
            // After the frame's combat / casting writers: a hit's procs resolve after the hit that
            // caused them (`SkillAutoCastController::AppraiseAndExecute` fires inside the hit).
            (activate, on_kill, on_hit_taken, tethers)
                .chain()
                .in_set(SecondarySet)
                .after(gd_game_core::combat::CombatSet),
        );
    }
}

/// After the frame's combat / casting writers; `casting::fire_late_procs` fires their procs the
/// same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecondarySet;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// Those without `specialSecondaryActivation`.
    Normal,
    /// The special moment (landing, arrival): only the special ones.
    Special,
    /// Cadence's finale: all of them.
    All,
    /// `PrimaryActivateSecondarySkills` at the parent's cast: the non-special ones.
    Primary,
}

/// `targets`: what the attack resolved on (first: the primary target); `point`: its coordinates
/// (the attacker for a swing, the impact for a projectile, the centre for a radius).
#[derive(Message, Clone, Debug)]
pub struct Activate {
    pub caster: Entity,
    pub skill: String,
    pub targets: Vec<Entity>,
    pub point: Vec3,
    pub mode: Mode,
    pub level_index: usize,
    /// `AttackInfo.replica`: `Skill_AttackWeapon::TargetResult` skips the ordinary secondaries.
    /// Nothing writes it.
    pub replica: bool,
}

/// Parent classes whose `ActivateNow` calls `PrimaryActivateSecondarySkills`. PORT:
/// `Skill_DefensiveWall` and `Skill_Shapeshift` call it too but do not cast through this flow.
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

/// Parent classes that run only the Primary pair on the cast (`Skill_BuffSelfDuration::ActivateNow`).
pub fn primary_only(class: &str) -> bool {
    matches!(class, "Skill_BuffSelfDuration" | "Skill_BuffSelfSick")
}

impl Activate {
    /// The parent's ordinary secondaries, if it hit anything. A pool skill runs the pool parent's;
    /// an empty parent (Cadence's ordinary attacks) none.
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

    /// The special pair of an attack that runs it hit or miss, at `point`; the pool parent's for a
    /// pool skill.
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

/// How a secondary class acts (`None`: not activated by its parent). `Bonus` buffs its validated
/// target (PORT: the caster); `BuffRadius` everyone friendly in radius (PORT: the caster only);
/// `AttackProjectileDrop` drops from above as `Skill_AttackProjectileDrop`. `BuffAttackRadiusDrop`
/// (the Fangs undead nemesis' rock rain) keeps the activation point on the floor and rains around
/// it like `Skill_BuffAttackRadiusDrop`. PORT: that point is not pulled into line of sight / onto
/// the path mesh.
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

fn cast_record(data: &GameData, sec: &str) -> String {
    data.db
        .get(sec)
        .and_then(|r| gd_sim::skills::profile_name(r).map(str::to_owned))
        .unwrap_or_else(|| sec.to_owned())
}

/// `SkillSecondary_Tether::GetTargets`: a foe counts when the ray from `from` to `to` enters its
/// world box grown by `tetherThickness`. PORT: the beam runs between the two characters' positions,
/// not their `emitPointName` points.
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
            // `SkillSecondary_BuffRadius` acts only at the parent's cast (`PrimaryActivateNow`).
            if (m.mode == Mode::Primary) != (rec.class == "SkillSecondary_BuffRadius") {
                continue;
            }
            // `specialSecondaryActivation` routes to the special pair; Primary skips them.
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
            // `AttackRadius` and the chains at the first target, the rest at the coordinates.
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

/// `SkillSecondary_OnHitBuffRadius::OnHitSecondaryActivation`: the owner being hit may cast the
/// buff (Spectral Armor's debuff): a `debuff` one around the attacker, else around the owner.
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
