//! Affliction data: DoT combat numbers, the icy look, the sick-buff state and
//! the tick ordering set. The systems are gd-game's `afflictions`.
use crate::data::GameData;
use bevy::prelude::*;
use gd_sim::attributes::CombatType as T;

/// One second's DoT damage of the main player on `target`.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct DotNumbers {
    pub target: Entity,
    pub damage: f32,
    pub multiplier: f32,
}

/// A frozen / petrified character's icy look, while its freeze / petrify handler is enabled.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Icy {
    /// Stone (petrify) rather than ice.
    pub petrify: bool,
    /// Milliseconds since the death (`ClearDamageFxs` at [`ICY_DEATH_MS`]).
    pub dead_ms: i32,
}

impl Icy {
    pub fn new(petrify: bool) -> Self {
        Self {
            petrify,
            dead_ms: 0,
        }
    }
}

/// `Character::Character` (`Character.damageFxLinger` = 250): how long a dead character keeps its damage effects.
pub const ICY_DEATH_MS: i32 = 250;

/// `BeginFreeze` / `BeginPetrify` defaults.
pub fn icy_handler(data: &GameData, petrify: bool) -> Option<gd_formats::arz::Record> {
    let engine = data.engine();
    data.db.get_owned(if petrify {
        &engine.petrify_handler
    } else {
        &engine.freeze_handler
    })
}

/// `icyEffectName` (`SpecialCharHandler_IcyCharacter::LoadFromTable`).
pub fn icy_texture(data: &GameData, petrify: bool) -> String {
    icy_handler(data, petrify)
        .and_then(|r| r.str("icyEffectName").map(str::to_owned))
        .unwrap_or_else(|| "xpack/effects/textures/icy.tex".to_owned())
}

/// `Some(petrify)` for freeze / petrify.
pub fn icy_kind(cc: Option<T>) -> Option<bool> {
    match cc {
        Some(T::Freeze) => Some(false),
        Some(T::Petrify) => Some(true),
        _ => None,
    }
}

pub const BUFF_SELF_SICK: &str = "Skill_BuffSelfSick";

/// The player's running `Skill_BuffSelfSick` timers: skill, seconds to the next roll,
/// time left last frame (a recast restarts it: `ActivateNow`).
#[derive(Component, Default, Debug)]
pub struct Sickness(pub Vec<(String, f32, f32)>);

/// `stunDuration`, `stunPeriod` (s, whole ms) and `stunChance` (0..1).
pub fn sick_profile(rec: &gd_formats::arz::Record) -> (f32, f32, f32) {
    let ms = |k: &str| (rec.f32(k).unwrap_or(0.0) * 1000.0) as i32 as f32 / 1000.0;
    (
        ms("stunDuration"),
        ms("stunPeriod"),
        rec.f32("stunChance").unwrap_or(0.0) as i32 as f32 * 0.01,
    )
}

/// What reacts to a status running out orders after `Tick`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AfflictionStage {
    Tick,
}
