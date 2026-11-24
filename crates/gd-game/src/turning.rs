//! How characters turn: the one writer of a character's facing after it spawns ([`turn_characters`]).
//!
//! Gameplay never sets a character's rotation; it says what the character turns toward through its
//! [`Character::turn`] ([`Turn`]), and [`turn_characters`] turns it at its record's rate (`gd_sim::turn`:
//! `Character::RotateTowards`). Game.dll, the callers of `RotateTowards`:
//! - Moving (`Character::CrowdAgentMoved`): every tick the crowd agent moved, toward its velocity
//!   ([`Turn::moved`]: the path followers, charges and path charges). PORT: leaps and evades (`Leap`) report
//!   no velocity: a leap flies along the line to its target and a Disengage away from it with its facing reversed
//!   (the engine's reversed-velocity flag, `Character.reversedVelocity`), so both keep facing the target the cast
//!   turned to.
//! - Attacking (`Character::UpdateSelf`, action state 7 "Attack", every skill use and swing): toward
//!   the attack target (`SetCurrentAttackTarget`: the target entity while it is alive and in the world, else the
//!   target point) while `RotateTowardsTarget` is on ([`Turn::attack`]). `AttackAction::Execute` turns
//!   it on as the skill starts (unless `disallowRotation`), an animation's `TurnStart` callback on again and
//!   `TurnEnd` off (`AttackAction::AnimationCallback`; 330 stock clips stop the turn mid-swing so,
//!   30 restart it), `StopRotation` off (`Character::AnimationCallback`), `AttackAction::Finish` off
//!   with the target cleared (the port: the character leaves [`CharState::Cast`] / [`CharState::Attack`]).
//!   Turning to the target clears the look-at point.
//! - A look-at point ([`Turn::look_at`]: `SetLookAtCoords` + `RotateTowardsPoint`, by `RotateAction::Execute`
//!   (the player's keyboard turn), `EvadeAction::Execute` (the evade's point) and
//!   `JumpAttackAction::Execute`): turned to in the idle, jump and evade states and in an attack whose target turn
//!   is off (a held channel / whirlwind's aim too: [`CharState::Channel`] / `Channel2` / `Spin` are its attack
//!   action), until reached.
//! - `ChatAction::FaceTarget` faces at once (`LookAtNoLean` + `SetCoords`, [`Turn::face_now`]).
//!
//! The rate multiplier ([`Turn::mult`]) is the channelled skills' `rotationSpeedMultiplier`
//! (`gd_sim::turn::speed_multiplier`): the player's channel sets it, and it is 1 again once no channel runs.
//! PORT: the dead do not turn; `ControllerMonsterStateParalyze` / `AlertBeforePursue`'s own turns and action
//! state 16's turn to the controller's target are not ported.
use bevy::ecs::entity::EntityHashMap;
use bevy::prelude::*;
use gd_game_core::character::{AnimCallback, CharState, Character};
use gd_game_core::combat::Dead;
use gd_game_core::turning::{Turn, yaw};
use gd_sim::turn::{Turned, rotate_towards};

/// Turns every character (module doc). Runs in `PostUpdate` before transform propagation: gameplay set the goals
/// in `Update`, and the callbacks it reads were dispatched the same frame (`CharacterStage::Animate`). Targets are
/// root entities: their `Transform` is this frame's position (propagation has not run yet).
#[allow(clippy::type_complexity)]
pub fn turn_characters(
    time: Res<Time>,
    mut calls: MessageReader<AnimCallback>,
    // The turn targets' shared `Transform` and the turning characters' exclusive
    // one overlap: a ParamSet, the target positions snapshotted owned first.
    mut qs: ParamSet<(
        Query<(
            Entity,
            &mut Transform,
            &mut Character,
            Has<Dead>,
            Has<gd_game_core::casting::Channel>,
        )>,
        Query<(Entity, &Transform), Without<Dead>>,
    )>,
    mut targets: Local<EntityHashMap<Vec3>>,
) {
    {
        let mut chars = qs.p0();
        for c in calls.read() {
            let Ok((_, _, mut ch, _, _)) = chars.get_mut(c.entity) else {
                continue;
            };
            match c.name.as_str() {
                "TurnStart" if matches!(ch.state, CharState::Cast | CharState::Attack) => {
                    ch.turn.toward_target = !ch.turn.disallow;
                }
                "TurnEnd" | "StopRotation" => ch.turn.toward_target = false,
                _ => {}
            }
        }
    }
    // This frame's positions of the targets being turned toward, copied out before
    // the characters are turned (the ParamSet's two queries cannot be borrowed at
    // once): only those, not every live entity.
    targets.clear();
    for (_, _, ch, dead, _) in &qs.p0() {
        if !dead
            && matches!(ch.state, CharState::Cast | CharState::Attack)
            && ch.turn.toward_target
            && let Some(t) = ch.turn.target
        {
            targets.insert(t, Vec3::NAN);
        }
    }
    {
        let alive = qs.p1();
        targets.retain(|e, p| match alive.get(*e) {
            Ok((_, tf)) => {
                *p = tf.translation;
                true
            }
            Err(_) => false,
        });
    }
    let dt_ms = time.delta_secs() * 1000.0;
    let mut chars = qs.p0();
    for (_, mut tf, mut ch, dead, channel) in &mut chars {
        let ch = &mut *ch;
        let state = ch.state;
        let turn = &mut ch.turn;
        let moved = turn.moved.take();
        let snap = turn.snap.take();
        if !channel {
            turn.mult = 1.0;
        }
        if dead || state == CharState::Death {
            turn.finish();
            turn.look = None;
            continue;
        }
        if !matches!(state, CharState::Cast | CharState::Attack) {
            turn.finish();
        }
        let before = yaw(tf.rotation);
        let mut yaw_now = before;
        let step = |yaw: f32, dir: Vec3, turn: &Turn| {
            rotate_towards(
                yaw,
                dir.to_array(),
                dt_ms,
                turn.speed,
                turn.mult,
                turn.disallow,
            )
        };
        if let Some(d) = snap
            && d.with_y(0.0).length() > 1e-4
        {
            yaw_now = d.x.atan2(d.z);
        }
        // `UpdateSelf`: the action state's turn.
        let mut target_turned = false;
        if matches!(state, CharState::Cast | CharState::Attack) && turn.toward_target {
            let at = turn
                .target
                .and_then(|t| targets.get(&t).copied())
                .or(turn.point);
            if let Some(at) = at {
                turn.look = None;
                let t = step(yaw_now, at - tf.translation, turn);
                set(&mut yaw_now, t);
                target_turned = true;
            }
        }
        if !target_turned
            && matches!(
                state,
                CharState::Idle
                    | CharState::Evade
                    | CharState::Cast
                    | CharState::Attack
                    | CharState::Channel
                    | CharState::Channel2
                    | CharState::Spin
            )
            && let Some(p) = turn.look
            && let t = step(yaw_now, p - tf.translation, turn)
            && set(&mut yaw_now, t)
        {
            turn.look = None;
        }
        // `CrowdAgentMoved`.
        if let Some(v) = moved {
            let t = step(yaw_now, v, turn);
            set(&mut yaw_now, t);
        }
        if yaw_now != before {
            tf.rotation = Quat::from_rotation_y(yaw_now);
        }
    }
}

/// Applies a turn to `yaw`; whether it reached its direction.
fn set(yaw: &mut f32, t: Turned) -> bool {
    match t {
        Turned::Ignored => false,
        Turned::Turning(y) => {
            *yaw = y;
            false
        }
        Turned::Reached(y) => {
            *yaw = y;
            true
        }
    }
}
