//! How characters turn: the one writer of a character's facing after it spawns ([`turn_characters`]).
//!
//! Gameplay never sets a character's rotation; it says what the character turns toward through its
//! [`Character::turn`] ([`Turn`]), and [`turn_characters`] turns it at its record's rate (`gd_sim::turn`:
//! `Character::RotateTowards`). Game.dll, the callers of `RotateTowards`:
//! - Moving (`Character::CrowdAgentMoved` @0x180066ee0): every tick the crowd agent moved, toward its velocity
//!   ([`Turn::moved`]: the path followers, charges and path charges). PORT: leaps and evades (`Leap`) report
//!   no velocity: a leap flies along the line to its target and a Disengage away from it with its facing reversed
//!   (the engine's reversed-velocity flag, Character +3536), so both keep facing the target the cast turned to.
//! - Attacking (`Character::UpdateSelf` @0x180062320, action state 7 "Attack", every skill use and swing): toward
//!   the attack target (`SetCurrentAttackTarget`: the target entity while it is alive and in the world, else the
//!   target point) while `RotateTowardsTarget` is on ([`Turn::attack`]). `AttackAction::Execute` @0x180083ec0 turns
//!   it on as the skill starts (unless `disallowRotation`), an animation's `TurnStart` callback on again and
//!   `TurnEnd` off (`AttackAction::AnimationCallback` @0x1800840f0; 330 stock clips stop the turn mid-swing so,
//!   30 restart it), `StopRotation` off (`Character::AnimationCallback`), `AttackAction::Finish` @0x180084630 off
//!   with the target cleared (the port: the character leaves [`CharState::Cast`] / [`CharState::Attack`]).
//!   Turning to the target clears the look-at point.
//! - A look-at point ([`Turn::look_at`]: `SetLookAtCoords` + `RotateTowardsPoint`, by `RotateAction::Execute`
//!   @0x180088930 (the player's keyboard turn), `EvadeAction::Execute` (the evade's point) and
//!   `JumpAttackAction::Execute`): turned to in the idle, jump and evade states and in an attack whose target turn
//!   is off (a held channel / whirlwind's aim too: [`CharState::Channel`] / `Channel2` / `Spin` are its attack
//!   action), until reached.
//! - `ChatAction::FaceTarget` @0x1800871d0 faces at once (`LookAtNoLean` + `SetCoords`, [`Turn::face_now`]).
//!
//! The rate multiplier ([`Turn::mult`]) is the channelled skills' `rotationSpeedMultiplier`
//! (`gd_sim::turn::speed_multiplier`): the player's channel sets it, and it is 1 again once no channel runs.
//! PORT: the dead do not turn; `ControllerMonsterStateParalyze` / `AlertBeforePursue`'s own turns and action
//! state 16's turn to the controller's target are not ported.
use crate::character::{AnimCallback, CharState, Character};
use crate::combat::Dead;
use bevy::ecs::entity::EntityHashMap;
use bevy::prelude::*;
use gd_sim::turn::{RotationSpeed, Turned, rotate_towards};

/// What a character turns toward (module doc) and how fast; part of [`Character`].
#[derive(Debug, Clone)]
pub struct Turn {
    /// `minRotationSpeed` / `maxRotationSpeed`.
    pub speed: RotationSpeed,
    /// `disallowRotation`: never turns (but [`Turn::face_now`]).
    pub disallow: bool,
    /// `SetRotationSpeedMultiplier` (module doc).
    pub mult: f32,
    /// The velocity moved this frame (`CrowdAgentMoved`), cleared by [`turn_characters`].
    moved: Option<Vec3>,
    /// `RotateTowardsTarget`.
    toward_target: bool,
    /// The attack target (`SetCurrentAttackTarget`): the entity, the point.
    target: Option<Entity>,
    point: Option<Vec3>,
    /// `SetLookAtCoords` with `RotateTowardsPoint` on.
    look: Option<Vec3>,
    /// A [`Turn::face_now`] for this frame.
    snap: Option<Vec3>,
}

impl Turn {
    pub fn new(speed: RotationSpeed, disallow: bool) -> Self {
        Self {
            speed,
            disallow,
            mult: 1.0,
            moved: None,
            toward_target: false,
            target: None,
            point: None,
            look: None,
            snap: None,
        }
    }

    /// From a character record (`Character::Load`).
    pub fn from_record(rec: &gd_formats::arz::Record) -> Self {
        Self::new(
            RotationSpeed::from_fields(rec.f32("minRotationSpeed"), rec.f32("maxRotationSpeed")),
            rec.f32("disallowRotation").is_some_and(|v| v != 0.0),
        )
    }

    /// The character moved with `velocity` (m/s) this frame.
    pub fn moved(&mut self, velocity: Vec3) {
        self.moved = Some(velocity);
    }

    /// An attack starts on `target` / at `point` (`AttackAction::Execute`): the character turns to it while it
    /// attacks.
    pub fn attack(&mut self, target: Option<Entity>, point: Vec3) {
        self.target = target;
        self.point = Some(point);
        self.toward_target = !self.disallow;
    }

    /// Turn to `point` until facing it (`RotateAction`).
    pub fn look_at(&mut self, point: Vec3) {
        if !self.disallow {
            self.look = Some(point);
        }
    }

    /// Face `dir` at once (`ChatAction::FaceTarget`).
    pub fn face_now(&mut self, dir: Vec3) {
        self.snap = Some(dir);
    }

    /// Whether the character is turning to its attack target.
    pub fn toward_target(&self) -> bool {
        self.toward_target
    }

    /// `AttackAction::Finish`.
    fn finish(&mut self) {
        self.toward_target = false;
        self.target = None;
        self.point = None;
    }
}

/// Facing yaw of a rotation (radians from +Z toward +X).
pub fn yaw(rot: Quat) -> f32 {
    let f = rot * Vec3::Z;
    f.x.atan2(f.z)
}

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
            Has<crate::casting::Channel>,
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
