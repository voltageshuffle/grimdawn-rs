//! Fear and confusion: the controller states the duration-damage manager's involuntary influences enter
//! while no hard crowd control runs (`DurationDamageManager::UpdateFxAndInfluence`; confusion before fear).
//!
//! - Confusion: `ControllerMonster::Confuse` → `ControllerMonsterStateConfused`: walks to a random point
//!   2 r..7 m from home (`ControllerMonster::FindRandomPoint`), and every `rand() % 2000 + 1000` ms attacks
//!   at random (`UseSkillOnPoint` with the best skill at a point 2 r..5 m from home, friend or foe).
//! - Fear: `ControllerMonster::ScareMe` → `ControllerMonsterStateScared`: runs `max(fleeDistance, 5)` m
//!   directly away from the last attacker ([`Attackers::last`]) with ±1 m jitter, re-picking every 1–2 s and
//!   on the path's end / failure; an invalid enemy stops the flee. No roar, no flee count.
//!
//! Both run only for monsters; the player's controller ignores them.
//!
//! PORT: [`crate::afflictions::tick`] marks a feared / confused monster `Stunned` but not `Rooted`, and this
//! system drives it through `MonsterAi::path` + [`advance_steered`]; a failed path re-picks the point. A
//! confused monster's random attack is a plain swing at a random nearby character of any side within its
//! view distance, approached on foot (a ranged monster swings melee too). Pet-class pets and the player keep
//! acting: their entries show on the HUD only. Scripted / patrolling / spirit states are left alone.

use crate::afflictions::Afflictions;
use crate::combat::{HiddenFromCombat, MonsterAi, QuestMove, advance_steered, run_speed};
use crate::movement::{CrowdAgent, Rooted, crowd_update};
use crate::nav::Nav;
use bevy::prelude::*;
use gd_game_core::afflictions::AfflictionStage;
use gd_game_core::character::{CharState, Character};
use gd_game_core::combat::MELEE_RANGE;
use gd_game_core::combat::{Attackers, CombatSet, Combatant};
use gd_sim::attributes::CombatType as T;

pub struct CcStatesPlugin;

impl Plugin for CcStatesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            cc_states
                .after(CombatSet)
                .after(AfflictionStage::Tick)
                .before(crowd_update),
        );
    }
}

#[derive(Component, Default)]
struct CcControl {
    /// Confusion: seconds to the next random attack (`rand() % 2000 + 1000` ms).
    attack: f32,
    /// Fear: seconds to the next flee point re-pick (1–2 s).
    repick: f32,
}

/// `ControllerMonster::FindRandomPoint`: up to 5 tries at a random point 2 r..7 m from home, on the
/// navmesh and more than 1 m from the monster; the path to it (empty: none found).
fn wander_path(
    nav: &Nav,
    scratch: &mut gd_sim::nav::PathScratch,
    me: Vec3,
    home: Vec3,
    radius: f32,
) -> Vec<Vec3> {
    for _ in 0..5 {
        let dist = 2.0 * radius + gd_game_core::rng::f32() * (7.0 - 2.0 * radius);
        let angle = gd_game_core::rng::f32() * std::f32::consts::TAU;
        let mut p = home + Vec3::new(angle.sin() * dist, 0.0, angle.cos() * dist);
        let Some(g) = nav.0.ground([p.x, home.y + 0.5, p.z]) else {
            continue;
        };
        p.y = g;
        if (p - me).length() <= 1.0 {
            continue;
        }
        if let Some(path) = nav.0.find_path(
            scratch,
            me.to_array(),
            p.to_array(),
            crate::nav::PATH_BUDGET,
        ) {
            return path.into_iter().skip(1).map(Vec3::from).collect();
        }
    }
    Vec::new()
}

/// A path to `to` without the monster's own point.
fn path_to(nav: &Nav, scratch: &mut gd_sim::nav::PathScratch, me: Vec3, to: Vec3) -> Vec<Vec3> {
    nav.0
        .find_path(
            scratch,
            me.to_array(),
            to.to_array(),
            crate::nav::PATH_BUDGET,
        )
        .map(|p| p.into_iter().skip(1).map(Vec3::from).collect())
        .unwrap_or_default()
}

#[allow(clippy::type_complexity)]
fn cc_states(
    mut commands: Commands,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    time: Res<Time>,
    nav: Res<Nav>,
    // The driven monsters mutably, every attackable character read-only: a snapshot first.
    mut set: ParamSet<(
        Query<
            (Entity, &Transform, &Combatant),
            (
                Without<gd_game_core::combat::Dead>,
                Without<HiddenFromCombat>,
                // A physics prop has a `Combatant` but is no character.
                Without<crate::physics_decoration::PhysicsDecoration>,
            ),
        >,
        Query<
            (
                Entity,
                &mut Transform,
                &mut MonsterAi,
                &mut Combatant,
                &mut Character,
                &Afflictions,
                &Attackers,
                Option<&mut CcControl>,
                Has<Rooted>,
            ),
            (
                Without<gd_game_core::combat::Dead>,
                Without<gd_game_core::player::Player>,
                Without<gd_game_core::world::Dummy>,
                Without<gd_game_core::casting::PathCharge>,
                Without<QuestMove>,
                Without<crate::patrol::PatrolPoint>,
                Without<crate::graeae::ToggleEye>,
                Without<crate::spirit::Spirit>,
                Without<crate::spirit::Dormant>,
            ),
        >,
    )>,
    mut crowd: Query<&mut CrowdAgent>,
) {
    let dt = time.delta_secs();
    // Nobody feared / confused nor handing back: no snapshot.
    if !set.p1().iter().any(|(.., aff, _, ctl, _)| {
        ctl.is_some() || matches!(aff.cc_state(), Some(T::Fear | T::Confusion))
    }) {
        return;
    }
    // A confused monster's random target is any of them, friend or foe.
    let all: Vec<(Entity, Vec3, f32)> = set
        .p0()
        .iter()
        .map(|(e, tf, cb)| (e, tf.translation, cb.radius))
        .collect();
    for (e, mut tf, mut ai, mut cb, mut ch, aff, attackers, ctl, rooted) in &mut set.p1() {
        let cc = aff.cc_state();
        if !matches!(cc, Some(T::Fear | T::Confusion)) {
            // The effect ended (or a hard crowd control took over): back to the normal AI.
            if ctl.is_some() {
                commands.entity(e).try_remove::<CcControl>();
                cb.target = None;
                ai.path.clear();
            }
            continue;
        }
        cb.cooldown -= dt;
        // A swing in progress finishes first; the attack timer waits.
        if matches!(ch.state, CharState::Attack | CharState::Cast) {
            continue;
        }
        // `PathFailed` re-picks.
        let mut agent = crowd.get_mut(e).ok();
        let failed = agent.as_mut().is_some_and(|a| a.take_path_failed());
        let me = tf.translation;
        let can_move = ai.class.can_move() && !rooted;
        let mut attack = ctl
            .as_ref()
            .map_or_else(|| 1.0 + gd_game_core::rng::f32() * 2.0, |c| c.attack);
        let mut repick = ctl.as_ref().map_or(0.0, |c| c.repick);
        let mut moving = false;
        let mut swinging = false;
        match cc {
            Some(T::Fear) => {
                // An invalid enemy stops the flee.
                cb.target = None;
                repick -= dt;
                let enemy = attackers
                    .last
                    .and_then(|en| all.iter().find(|(oe, ..)| *oe == en));
                if let Some((_, ep, _)) = enemy {
                    if can_move && (repick <= 0.0 || ai.path.is_empty() || failed) {
                        ai.path = path_to(&nav, &mut scratch, me, ai.flee.scare_point(me, *ep));
                        repick = 1.0 + gd_game_core::rng::f32();
                    }
                } else {
                    ai.path.clear();
                }
                if can_move {
                    moving = advance_steered(
                        &mut tf,
                        &mut ai.path,
                        run_speed(&ch),
                        dt,
                        &mut ch.turn,
                        agent.as_deref_mut(),
                    );
                }
            }
            Some(T::Confusion) => {
                attack -= dt;
                if attack <= 0.0 {
                    attack = 1.0 + gd_game_core::rng::f32() * 2.0;
                    let candidates: Vec<Entity> = all
                        .iter()
                        .filter(|(oe, op, or)| {
                            *oe != e && (*op - me).with_y(0.0).length() <= ai.view + cb.radius + or
                        })
                        .map(|(oe, ..)| *oe)
                        .collect();
                    cb.target = (!candidates.is_empty())
                        .then(|| candidates[gd_game_core::rng::usize(..candidates.len())]);
                    ai.path.clear();
                }
                if let Some(te) = cb.target {
                    if let Some((_, tp, tr)) = all.iter().find(|(oe, ..)| *oe == te) {
                        let d = (*tp - me).with_y(0.0).length();
                        if d <= cb.radius + tr + MELEE_RANGE {
                            ai.path.clear();
                            if cb.cooldown <= 0.0 {
                                ch.begin_swing(&tf, Some((te, *tp)), *tp);
                                cb.cooldown = ai.swing_pause.0
                                    + gd_game_core::rng::f32()
                                        * (ai.swing_pause.1 - ai.swing_pause.0).max(0.0);
                                ch.state = CharState::Attack;
                                swinging = true;
                            }
                        } else if can_move {
                            if ai.path.is_empty() || failed {
                                ai.path = path_to(&nav, &mut scratch, me, *tp);
                            }
                            moving = advance_steered(
                                &mut tf,
                                &mut ai.path,
                                run_speed(&ch),
                                dt,
                                &mut ch.turn,
                                agent.as_deref_mut(),
                            );
                        }
                    } else {
                        cb.target = None;
                    }
                }
                // No target: wander (`WalkRandomly`).
                if cb.target.is_none() && can_move {
                    if ai.path.is_empty() || failed {
                        ai.path = wander_path(&nav, &mut scratch, me, ai.home, cb.radius);
                    }
                    moving = advance_steered(
                        &mut tf,
                        &mut ai.path,
                        run_speed(&ch),
                        dt,
                        &mut ch.turn,
                        agent.as_deref_mut(),
                    );
                }
            }
            _ => {}
        }
        if let Some(g) = nav
            .0
            .ground([tf.translation.x, tf.translation.y + 0.5, tf.translation.z])
        {
            tf.translation.y = g;
        }
        let want = if swinging {
            CharState::Attack
        } else if moving {
            CharState::Run
        } else {
            CharState::Idle
        };
        if ch.state != want {
            ch.state = want;
        }
        if let Some(mut c) = ctl {
            c.attack = attack;
            c.repick = repick;
        } else {
            commands.entity(e).try_insert(CcControl { attack, repick });
        }
    }
}
