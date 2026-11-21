//! Fear and confusion: the controller states the duration-damage manager's
//! involuntary influences enter, below the hard crowd control
//! (`DurationDamageManager::UpdateFxAndInfluence` @0x18027b8a0:
//! while none of immobilize / petrify / freeze / trap / sleep / stun /
//! knockdown runs, confusion (53) before fear (52)).
//!
//! - Confusion: `Monster::CombatExertInfluenceConfusion` @0x18037b2e0 →
//!   `ControllerMonster::Confuse` @0x180135970 → `ControllerMonsterStateConfused`
//!   (`OnBegin` @0x180149df0, `OnUpdate` @0x180149f70, `WalkRandomly`
//!   @0x18014a1c0, `PathFailed` = `EndOfPathReached` → `WalkRandomly`):
//!   walks to a random point 2 r..7 m from its home (sub_18013E7A0: up to 5
//!   tries, any direction, on the navmesh, more than 1 m away), and every
//!   `rand() % 2000 + 1000` ms attacks at random (`UseSkillOnPoint` with the
//!   best skill at a point 2 r..5 m from home — anyone in reach is hit,
//!   friend or foe).
//! - Fear: `Character::CombatExertInfluenceFear` @0x180068ee0 →
//!   `ControllerMonster::ScareMe` @0x180135cb0 → `ControllerMonsterStateScared`
//!   (`OnBegin` @0x18014ae10, `OnUpdate` @0x18014aee0, `PickRunToPoint`
//!   @0x18014b090, `PathFailed` = `EndOfPathReached` → re-pick): runs to
//!   `max(fleeDistance, 5)` m directly away from the last attacker
//!   (Character +1692, [`Attackers::last`]) with ±1 m of horizontal jitter,
//!   re-picking every 1–2 s and on the path's end / failure; an invalid
//!   enemy stops the flee. No roar, no flee count (the Flee state's
//!   bookkeeping is its own, `crate::flee`).
//!
//! Both run only for monsters (`ControllerMonster`); the player's controller
//! ignores them (its `ScareMe` / `Confuse` vtable slots are no-ops).
//!
//! PORT: [`crate::afflictions::tick`] marks a feared / confused monster
//! `Stunned` (the normal AI's exclusion flag) but not `Rooted`, and this
//! system drives it after `CombatSet` (after the afflictions tick, before
//! the crowd): the move goes through `MonsterAi::path` + [`advance_steered`] like
//! `monster_ai`'s own, so the crowd's separation, depenetration and stuck
//! checks apply; a failed path re-picks the point (both states'
//! `PathFailed`). A confused monster's random attack is a plain swing at a
//! random nearby character of any side within its view distance (the engine
//! uses its best skill at a random point; the skill choice machinery is the
//! normal AI's, which the takeover suspends), approached on foot — a ranged
//! monster swings melee too. Pet-class pets (no `MonsterAi`, `pets::ai`
//! drives them) and the player keep acting: their entries show on the HUD
//! only. The scripted / patrolling / spirit states `monster_ai` leaves alone
//! are left alone here too.

use crate::afflictions::{AfflictionStage, Afflictions};
use crate::character::{CharState, Character};
use crate::combat::{
    Attackers, CombatSet, Combatant, HiddenFromCombat, MELEE_RANGE, MonsterAi, QuestMove,
    advance_steered, run_speed,
};
use crate::movement::{CrowdAgent, Rooted, crowd_update};
use crate::nav::Nav;
use bevy::prelude::*;
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

/// The driven monster's state timers while its fear / confusion runs.
#[derive(Component, Default)]
struct CcControl {
    /// Confusion: seconds to the next random attack (`rand() % 2000 + 1000` ms).
    attack: f32,
    /// Fear: seconds to the next flee point re-pick (1–2 s).
    repick: f32,
}

/// `ControllerMonsterStateConfused::WalkRandomly`'s point (sub_18013E7A0):
/// up to 5 tries at a random point 2 r..7 m from home, any direction, on the
/// navmesh and more than 1 m from the monster; the path to it (empty: none
/// found, the monster stands).
fn wander_path(
    nav: &Nav,
    scratch: &mut gd_sim::nav::PathScratch,
    me: Vec3,
    home: Vec3,
    radius: f32,
) -> Vec<Vec3> {
    for _ in 0..5 {
        let dist = 2.0 * radius + crate::rng::f32() * (7.0 - 2.0 * radius);
        let angle = crate::rng::f32() * std::f32::consts::TAU;
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

/// A path to `to` (the AI's own: the first point is where the monster stands).
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
    // The driven monsters mutably, every attackable character read-only: a
    // snapshot first (the `ParamSet` keeps the two queries apart, as
    // `monster_ai`'s does).
    mut set: ParamSet<(
        Query<
            (Entity, &Transform, &Combatant),
            (
                Without<crate::combat::Dead>,
                Without<HiddenFromCombat>,
                // A physics prop has a `Combatant` but is no character to attack.
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
                Without<crate::combat::Dead>,
                Without<crate::player::Player>,
                Without<crate::world::Dummy>,
                Without<crate::casting::PathCharge>,
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
    // Nobody feared / confused nor handing back: no snapshot (the common frame).
    if !set.p1().iter().any(|(.., aff, _, ctl, _)| {
        ctl.is_some() || matches!(aff.cc_state(), Some(T::Fear | T::Confusion))
    }) {
        return;
    }
    // Every living attackable character: a confused monster's random target
    // is any of them, friend or foe.
    let all: Vec<(Entity, Vec3, f32)> = set
        .p0()
        .iter()
        .map(|(e, tf, cb)| (e, tf.translation, cb.radius))
        .collect();
    for (e, mut tf, mut ai, mut cb, mut ch, aff, attackers, ctl, rooted) in &mut set.p1() {
        let cc = aff.cc_state();
        if !matches!(cc, Some(T::Fear | T::Confusion)) {
            // The effect ended (or a hard crowd control took over): hand the
            // monster back to its normal AI.
            if ctl.is_some() {
                commands.entity(e).try_remove::<CcControl>();
                cb.target = None;
                ai.path.clear();
            }
            continue;
        }
        cb.cooldown -= dt;
        // A swing in progress finishes first (the animation's "End" leaves
        // the state); the confused state's attack timer waits like the
        // engine's suspended Confused state.
        if matches!(ch.state, CharState::Attack | CharState::Cast) {
            continue;
        }
        // The crowd failed the move: both states' `PathFailed` re-picks.
        let mut agent = crowd.get_mut(e).ok();
        let failed = agent.as_mut().is_some_and(|a| a.take_path_failed());
        let me = tf.translation;
        let can_move = ai.class.can_move() && !rooted;
        let mut attack = ctl
            .as_ref()
            .map_or_else(|| 1.0 + crate::rng::f32() * 2.0, |c| c.attack);
        let mut repick = ctl.as_ref().map_or(0.0, |c| c.repick);
        let mut moving = false;
        let mut swinging = false;
        match cc {
            Some(T::Fear) => {
                // Scared: away from the last attacker; an invalid enemy
                // stops the flee ("scared of an invalid enemy" → Hide).
                cb.target = None;
                repick -= dt;
                let enemy = attackers
                    .last
                    .and_then(|en| all.iter().find(|(oe, ..)| *oe == en));
                if let Some((_, ep, _)) = enemy {
                    if can_move && (repick <= 0.0 || ai.path.is_empty() || failed) {
                        ai.path = path_to(&nav, &mut scratch, me, ai.flee.scare_point(me, *ep));
                        repick = 1.0 + crate::rng::f32();
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
                // Confused: every 1–3 s a random nearby character of any
                // side becomes the target; approach it and swing.
                attack -= dt;
                if attack <= 0.0 {
                    attack = 1.0 + crate::rng::f32() * 2.0;
                    let candidates: Vec<Entity> = all
                        .iter()
                        .filter(|(oe, op, or)| {
                            *oe != e && (*op - me).with_y(0.0).length() <= ai.view + cb.radius + or
                        })
                        .map(|(oe, ..)| *oe)
                        .collect();
                    cb.target = (!candidates.is_empty())
                        .then(|| candidates[crate::rng::usize(..candidates.len())]);
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
                                    + crate::rng::f32()
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
                // No target: wander (WalkRandomly on the begin and on every
                // path's end / failure).
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
