//! What a character turns toward and how fast.
use bevy::prelude::*;
use gd_sim::turn::RotationSpeed;

/// Part of `gd_game::character::Character`.
#[derive(Debug, Clone)]
pub struct Turn {
    /// `minRotationSpeed` / `maxRotationSpeed`.
    pub speed: RotationSpeed,
    /// `disallowRotation`: never turns (but [`Turn::face_now`]).
    pub disallow: bool,
    /// `SetRotationSpeedMultiplier`.
    pub mult: f32,
    /// `CrowdAgentMoved` velocity, cleared by `gd_game::turning::turn_characters`.
    pub moved: Option<Vec3>,
    /// `RotateTowardsTarget`.
    pub toward_target: bool,
    /// The attack target (`SetCurrentAttackTarget`): the entity, the point.
    pub target: Option<Entity>,
    pub point: Option<Vec3>,
    /// `SetLookAtCoords` with `RotateTowardsPoint` on.
    pub look: Option<Vec3>,
    /// Pending [`Turn::face_now`].
    pub snap: Option<Vec3>,
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

    /// `velocity` in m/s.
    pub fn moved(&mut self, velocity: Vec3) {
        self.moved = Some(velocity);
    }

    /// `AttackAction::Execute`: turns to the target while attacking.
    pub fn attack(&mut self, target: Option<Entity>, point: Vec3) {
        self.target = target;
        self.point = Some(point);
        self.toward_target = !self.disallow;
    }

    /// `RotateAction`: turn until facing `point`.
    pub fn look_at(&mut self, point: Vec3) {
        if !self.disallow {
            self.look = Some(point);
        }
    }

    /// `ChatAction::FaceTarget`.
    pub fn face_now(&mut self, dir: Vec3) {
        self.snap = Some(dir);
    }

    /// `AttackAction::Finish`.
    pub fn finish(&mut self) {
        self.toward_target = false;
        self.target = None;
        self.point = None;
    }
}

/// Radians from +Z toward +X.
pub fn yaw(rot: Quat) -> f32 {
    let f = rot * Vec3::Z;
    f.x.atan2(f.z)
}
