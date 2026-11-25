//! How characters turn: [`turn_characters`] is the one writer of a character's facing after spawn.
//!
//! Gameplay says what a character turns toward through [`Character::turn`]; it turns at its
//! record's rate (`Character::RotateTowards`). The callers of `RotateTowards`:
//! - Moving (`Character::CrowdAgentMoved`): toward its velocity ([`Turn::moved`]). PORT: leaps and
//!   evades report no velocity; a leap flies at its target and a Disengage away with its facing
//!   reversed (`Character.reversedVelocity`), so both keep facing the target.
//! - Attacking (`Character::UpdateSelf`, action state 7): toward the attack target (the entity while
//!   alive and in the world, else the point) while `RotateTowardsTarget` is on ([`Turn::attack`]).
//!   `AttackAction::Execute` turns it on (unless `disallowRotation`), `TurnStart` on again, `TurnEnd`
//!   and `StopRotation` off, `AttackAction::Finish` off with the target cleared (the character
//!   leaves [`CharState::Cast`] / [`CharState::Attack`]). Turning to the target clears the look-at.
//! - A look-at point ([`Turn::look_at`]: `RotateAction`, `EvadeAction`, `JumpAttackAction`): turned
//!   to in the idle, jump and evade states and in an attack whose target turn is off (channels and
//!   spins included), until reached.
//! - `ChatAction::FaceTarget` faces at once ([`Turn::face_now`]).
//!
//! [`Turn::mult`] is the player's channel's `rotationSpeedMultiplier`, 1 with no channel.
//! PORT: the dead do not turn; `ControllerMonsterStateParalyze` / `AlertBeforePursue`'s own turns
//! and action state 16's turn are not ported.
use bevy::ecs::entity::EntityHashMap;
use bevy::prelude::*;
use gd_game_core::character::{AnimCallback, CharState, Character};
use gd_game_core::combat::Dead;
use gd_game_core::turning::{Turn, yaw};
use gd_sim::turn::{Turned, rotate_towards};

/// Runs in `PostUpdate` before transform propagation, after the frame's animation callbacks; targets
/// are root entities, so their `Transform` is this frame's position.
#[allow(clippy::type_complexity)]
pub fn turn_characters(
    time: Res<Time>,
    mut calls: MessageReader<AnimCallback>,
    // Target and turner `Transform`s overlap: a ParamSet, target positions snapshotted first.
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
