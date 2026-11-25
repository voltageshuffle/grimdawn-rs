//! Synergy controller data. The team-forming and super-attack systems are
//! gd-game's `synergy`.
use bevy::prelude::*;

use crate::monster_skills::Special;

/// The controller's mode (`ControllerMonsterSynergy.mode`) and team (`team`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Idle,
    Leader(u32),
    Slave(u32),
}

#[derive(Component, Debug)]
pub struct Synergy {
    pub mode: Mode,
    pub radius: f32,
    pub team_size: usize,
    /// `synergyChargeTime` (s).
    pub charge_time: f32,
    pub charge_left: f32,
    /// `synergySearchInterval*` (ms) and the search timer (s).
    pub interval: (i32, i32),
    pub search: f32,
    /// Slot 0.
    pub super_skill: Option<Special>,
    /// `ControllerMonsterSynergy.superAttackOrdered` / seen running.
    pub fired: bool,
    pub started: bool,
    /// The leader's `AddComrade` list.
    pub comrades: Vec<Entity>,
}

impl Synergy {
    /// 0..1.
    pub fn charge(&self) -> f32 {
        if self.charge_time <= 0.0 {
            1.0
        } else {
            1.0 - self.charge_left / self.charge_time
        }
    }

    pub fn rearm(&mut self) {
        self.search = crate::rng::i32(
            self.interval.0.min(self.interval.1)..=self.interval.1.max(self.interval.0),
        ) as f32
            / 1000.0;
    }
}
