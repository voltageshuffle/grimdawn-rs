//! `ControllerMonsterSynergy` (the cave Groble shamans, the Aetherial
//! phantoms, Fleshshaper's spirits): monsters that join up and let their
//! leader release a charged super skill. Game.dll:
//!
//! - `Load`: `synergyRadius`, `synergyTeamSize` (6),
//!   `synergyChargeTime` (1000 ms), `synergySearchIntervalMin` /
//!   `…Max` (1000 / 2000 ms), `synergyChaosBeam`, `synergyNumBeams` (1).
//! - `InitialUpdate` keeps special slot 0 (`specialAttack`) aside
//!   as the super skill (`GetSuperSkill`) and empties the slot.
//! - `Update`, idle (`ControllerMonsterSynergy.mode` = 0), its search timer (`searchTimer`)
//!   out: the timer is re-armed (`IGenerate(min, max)`); the `Monster`s
//!   within `synergyRadius` (`GetTargetsInRadius`) whose controller is an
//!   idle synergy one are the candidates, and the target most of them are
//!   after (their state's target) is the team's. With at least
//!   `synergyTeamSize` candidates and such a target: the leader is the
//!   candidate nearest the centre of their bounding box; nothing happens when
//!   this monster's super skill is missing or not ready (`Skill.ready`), fails
//!   `rand() % 100 > chance` or is not close enough to the target
//!   (`CloseEnoughToUseSkill`); from twice the team size only the team size
//!   nearest the leader stay (`Game::FindClosestEntities`). Every member gets the target
//!   (`ControllerMonster.target`); the leader takes a new team (`PickTeam`: ++`m_sTeamPool`),
//!   mode 1 and the charge time (`chargeTimer`), the others mode 2 (`SetSlave`), and
//!   each a `synergyChaosBeam` (`SetNumBeams`) from it to the leader that the
//!   leader keeps with the member (`AddComrade`).
//! - The leader (mode 1): the charge is 1 - left / `synergyChargeTime`; at 1
//!   (once, `superAttackOrdered`) it goes "SuperAttack"
//!   (`ControllerMonsterSynergyStateSuperAttack::OnBegin`:
//!   `UseSkill` of the super skill on its target); before, with fewer than
//!   `synergyTeamSize` of its members within `synergyRadius`, it `Disband`s;
//!   the time left counts down; the beams follow the members' and its
//!   "Target" points, filled to the charge. Members (mode 2) fight on as
//!   usual.
//! - The super attack's "End" (`HandleEvent`): `Disband`, then
//!   "RepositionForAttack" with the current state's enemy and skill (not zeroed);
//!   `ControllerMonsterStateRepositionForAttack` (`OnBegin`, `OnUpdate`)
//!   is the fight state of `melee.rs` (`St::Reposition`: the run to `GetMoveToPoint` at both
//!   radii, "Attack" in reach at the path's end or after 2500 ms, else "Pursue"), entered through
//!   `MeleeAi::reposition`.
//! - `Disband`: the flag cleared, the search timer re-armed; each
//!   member (the leader one of them) idle again (mode 0), its beam detached
//!   and freed; the member list emptied. "Dying"
//!   (`ControllerMonsterSynergyStateDying::OnBegin`) disbands
//!   first.
//!
//! PORT: the beams are drawn by `grimdawn::chaos_beam` (not seen in a run); the super attack starts once the leader
//! is not mid-action (the engine's `SetState` cuts in); RepositionForAttack's
//! in-reach test uses the skill the fight states chose, not the super skill
//! of the state data; the reach distance is flat
//! (the bounding-box centre test adds the region height); ties for the
//! target go to the first found; the draws use the port's RNG.
use crate::combat::MonsterAi;
use crate::monster_skills::MonsterSkills;
use bevy::prelude::*;
use gd_game_core::casting::Casting;
use gd_game_core::character::{CharState, Character};
use gd_game_core::combat::{Combatant, ControllerClass, Dead};
use gd_game_core::data::GameData;
use gd_game_core::monster_skills::Special;
use gd_game_core::monsters::Monster;
use gd_game_core::synergy::{Mode, Synergy};

pub struct SynergyPlugin;

impl Plugin for SynergyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TeamPool>().add_systems(
            Update,
            (init, update)
                .chain()
                .after(crate::monster_skills::MonsterSkillStage::Init)
                .before(crate::monster_skills::MonsterSkillStage::Choose),
        );
    }
}

/// `ControllerMonsterSynergy::m_sTeamPool`.
#[derive(Resource, Default)]
pub struct TeamPool(pub u32);

fn init(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<
        (Entity, &Monster, &MonsterAi, &mut MonsterSkills),
        (Without<Synergy>, Added<MonsterSkills>),
    >,
) {
    for (e, m, ai, mut ms) in &mut q {
        if ai.class != ControllerClass::Synergy {
            continue;
        }
        let ctl = data
            .db
            .get(&m.record)
            .and_then(|r| r.str("controller").and_then(|c| data.db.get(c)));
        let f = |k: &str, d: f32| ctl.as_ref().and_then(|c| c.f32(k)).unwrap_or(d);
        let i = |k: &str, d: i32| ctl.as_ref().and_then(|c| c.i32(k)).unwrap_or(d);
        let super_skill = ms.specials.iter().find(|s| s.slot == 0).cloned();
        let specials: Vec<Special> = ms
            .specials
            .iter()
            .filter(|s| s.slot != 0)
            .cloned()
            .collect();
        let attack = ms.attack.clone();
        ms.set_kit(attack, specials);
        debug!(
            "synergy {e}: {} (super {:?})",
            m.record,
            super_skill.as_ref().map(|s| &s.skill)
        );
        commands.entity(e).insert(Synergy {
            mode: Mode::Idle,
            radius: f("synergyRadius", 0.0),
            team_size: i("synergyTeamSize", 6).max(1) as usize,
            charge_time: i("synergyChargeTime", 1000) as f32 / 1000.0,
            charge_left: 0.0,
            interval: (
                i("synergySearchIntervalMin", 1000),
                i("synergySearchIntervalMax", 2000),
            ),
            search: 0.0,
            super_skill,
            fired: false,
            started: false,
            comrades: Vec::new(),
        });
    }
}

type Row<'a> = (
    Entity,
    &'a mut Synergy,
    &'a mut MonsterAi,
    &'a mut MonsterSkills,
    &'a mut Combatant,
    &'a mut Transform,
    &'a mut Character,
    Has<Dead>,
    Has<Casting>,
);

/// `Disband`: the leader's members (itself among them) go idle.
fn disband(q: &mut Query<Row>, leader: Entity) {
    let Ok((_, mut s, ..)) = q.get_mut(leader) else {
        return;
    };
    s.fired = false;
    s.started = false;
    s.rearm();
    let members = std::mem::take(&mut s.comrades);
    for m in members {
        if let Ok((_, mut ms, ..)) = q.get_mut(m) {
            ms.mode = Mode::Idle;
        }
    }
    debug!("synergy {leader}: disbanded");
}

#[allow(clippy::type_complexity)]
fn update(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut pool: ResMut<TeamPool>,
    mut q: Query<Row>,
    newly_dead: Query<Entity, (With<Synergy>, Added<Dead>)>,
    targets: Query<(&Transform, &Combatant), (Without<Synergy>, Without<Dead>)>,
) {
    let dt = time.delta_secs();
    // Dying: disband first.
    for e in &newly_dead {
        if q.get(e).is_ok_and(|r| matches!(r.1.mode, Mode::Leader(_))) {
            disband(&mut q, e);
        }
    }
    let all: Vec<Entity> = q.iter().filter(|r| !r.7).map(|r| r.0).collect();
    for e in all {
        let Ok((_, mut s, ai, ms, cb, tf, ch, _, casting)) = q.get_mut(e) else {
            continue;
        };
        if s.search > 0.0 {
            s.search -= dt;
        }
        match s.mode {
            Mode::Idle if s.search <= 0.0 => {
                s.rearm();
                let (me, radius, team, sup) =
                    (tf.translation, s.radius, s.team_size, s.super_skill.clone());
                let my_radius = cb.radius;
                let ready = sup.as_ref().is_some_and(|k| ms.ready(&k.skill));
                // The idle synergy monsters in radius and their targets.
                let cands: Vec<(Entity, Vec3, Option<Entity>)> = q
                    .iter()
                    .filter(|r| {
                        !r.7 && r.1.mode == Mode::Idle
                            && r.4.in_radius(&r.5.compute_affine(), me, radius)
                    })
                    .map(|r| (r.0, r.5.translation, r.4.target))
                    .collect();
                let mut counts: Vec<(Entity, usize)> = Vec::new();
                for t in cands.iter().filter_map(|c| c.2) {
                    match counts.iter_mut().find(|c| c.0 == t) {
                        Some(c) => c.1 += 1,
                        None => counts.push((t, 1)),
                    }
                }
                let mut best: Option<(Entity, usize)> = None;
                for c in &counts {
                    if best.is_none_or(|b| c.1 > b.1) {
                        best = Some(*c);
                    }
                }
                let Some((target, _)) = best else { continue };
                if cands.len() < team {
                    continue;
                }
                let (lo, hi) = cands.iter().fold(
                    (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
                    |(lo, hi), c| (lo.min(c.1), hi.max(c.1)),
                );
                let centre = (lo + hi) * 0.5;
                let Some(leader) = cands
                    .iter()
                    .min_by(|a, b| {
                        (a.1 - centre)
                            .with_y(0.0)
                            .length()
                            .total_cmp(&(b.1 - centre).with_y(0.0).length())
                    })
                    .map(|c| (c.0, c.1))
                else {
                    continue;
                };
                // This monster's super skill: ready, its chance, in reach of the target.
                let Some(sup) = sup.filter(|_| ready) else {
                    continue;
                };
                if gd_game_core::rng::i32(0..100) as f32 > sup.chance {
                    continue;
                }
                let Ok((ttf, tcb)) = targets.get(target) else {
                    continue;
                };
                if (ttf.translation - me).with_y(0.0).length()
                    > my_radius
                        + tcb.radius
                        + sup.range
                        + crate::monster_skills::SKILL_USE_TOLERANCE
                {
                    continue;
                }
                let mut members: Vec<(Entity, Vec3)> = cands.iter().map(|c| (c.0, c.1)).collect();
                if members.len() >= 2 * team {
                    members.sort_by(|a, b| {
                        (a.1 - leader.1)
                            .length()
                            .total_cmp(&(b.1 - leader.1).length())
                    });
                    members.truncate(team);
                }
                pool.0 += 1;
                let id = pool.0;
                debug!(
                    "synergy {e}: team {id} of {} led by {} on {target}",
                    members.len(),
                    leader.0
                );
                for (m, _) in &members {
                    let Ok((_, mut ms, mut ai, ..)) = q.get_mut(*m) else {
                        continue;
                    };
                    ms.mode = if *m == leader.0 {
                        Mode::Leader(id)
                    } else {
                        Mode::Slave(id)
                    };
                    ai.enemy = Some(target);
                }
                if let Ok((_, mut ls, ..)) = q.get_mut(leader.0) {
                    ls.charge_left = ls.charge_time;
                    ls.comrades = members.iter().map(|m| m.0).collect();
                }
            }
            Mode::Leader(_) => {
                let charge = s.charge();
                if charge >= 1.0 {
                    if !s.fired {
                        if casting || matches!(ch.state, CharState::Attack | CharState::Cast) {
                            continue;
                        }
                        let Some(sup) = s.super_skill.clone() else {
                            continue;
                        };
                        let target = ai
                            .enemy
                            .or(cb.target)
                            .and_then(|t| targets.get(t).ok().map(|x| (t, x.0.translation)));
                        s.fired = true;
                        let (Ok((_, _, _, mut ms, cb, mut tf, mut ch, ..)), Some((t, at))) =
                            (q.get_mut(e), target)
                        else {
                            disband(&mut q, e);
                            continue;
                        };
                        debug!("synergy {e}: super attack {}", sup.skill);
                        ms.used(&data, &cb, &sup.skill, sup.level_index);
                        crate::monster_skills::start(
                            &mut commands,
                            &data,
                            e,
                            &mut tf,
                            &mut ch,
                            &sup.skill,
                            sup.level_index,
                            sup.kind,
                            at,
                            Some(t),
                        );
                    } else if casting || matches!(ch.state, CharState::Attack | CharState::Cast) {
                        s.started = true;
                    } else if s.started {
                        // The super attack's "End": `Disband`, then
                        // "RepositionForAttack" on the state's enemy.
                        let enemy = ai.enemy.or(cb.target);
                        disband(&mut q, e);
                        if let (Some(t), Ok((_, _, mut ai, ..))) = (enemy, q.get_mut(e)) {
                            ai.melee.reposition(t);
                        }
                    }
                    continue;
                }
                let (me, radius, team) = (tf.translation, s.radius, s.team_size);
                let comrades = s.comrades.clone();
                s.charge_left = (s.charge_left - dt).max(0.0);
                let near = comrades
                    .iter()
                    .filter(|c| {
                        q.get(**c)
                            .is_ok_and(|r| !r.7 && r.4.in_radius(&r.5.compute_affine(), me, radius))
                    })
                    .count();
                if near < team {
                    disband(&mut q, e);
                }
            }
            _ => {}
        }
    }
}
