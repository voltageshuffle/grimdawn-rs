//! Affliction data: the DoT combat-numbers message ([`DotNumbers`]), the icy
//! look ([`Icy`]) with its handler / texture resolvers, the sick-buff state
//! ([`Sickness`], [`sick_profile`]) and the tick ordering set
//! ([`AfflictionStage`]). The `Afflictions` component and the systems that run
//! damage over time, debuffs and crowd control are gd-game's `afflictions`
//! (its module docs carry the full engine trace).
use crate::data::GameData;
use bevy::prelude::*;
use gd_sim::attributes::CombatType as T;

/// One second's DoT damage of the main player on `target` (module docs).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct DotNumbers {
    pub target: Entity,
    pub damage: f32,
    pub multiplier: f32,
}

/// The dressing request of a frozen / petrified character's icy look (module docs, "Icy look"): on the
/// character while its freeze / petrify handler is enabled. grimdawn draws it from [`icy_texture`].
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Icy {
    /// The petrify handler's look (stone), else the freeze handler's (ice).
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

/// The handler record of the freeze / petrify look (`BeginFreeze` / `BeginPetrify` defaults).
pub fn icy_handler(data: &GameData, petrify: bool) -> Option<gd_formats::arz::Record> {
    let engine = data.engine();
    data.db.get_owned(if petrify {
        &engine.petrify_handler
    } else {
        &engine.freeze_handler
    })
}

/// The look's texture, `icyEffectName` (`SpecialCharHandler_IcyCharacter::LoadFromTable`, default
/// `xpack/effects/textures/icy.tex`).
pub fn icy_texture(data: &GameData, petrify: bool) -> String {
    icy_handler(data, petrify)
        .and_then(|r| r.str("icyEffectName").map(str::to_owned))
        .unwrap_or_else(|| "xpack/effects/textures/icy.tex".to_owned())
}

/// The involuntary effect's handler: `Some(petrify)` for freeze / petrify.
pub fn icy_kind(cc: Option<T>) -> Option<bool> {
    match cc {
        Some(T::Freeze) => Some(false),
        Some(T::Petrify) => Some(true),
        _ => None,
    }
}

/// The sick buff class (module docs).
pub const BUFF_SELF_SICK: &str = "Skill_BuffSelfSick";

/// The player's running `Skill_BuffSelfSick` timers (module docs): skill,
/// seconds to the next roll, its time left last frame (a recast restarts
/// it: `ActivateNow`).
#[derive(Component, Default, Debug)]
pub struct Sickness(pub Vec<(String, f32, f32)>);

/// A sick buff's `stunDuration`, `stunPeriod` (s, whole ms) and
/// `stunChance` (0..1).
pub fn sick_profile(rec: &gd_formats::arz::Record) -> (f32, f32, f32) {
    let ms = |k: &str| (rec.f32(k).unwrap_or(0.0) * 1000.0) as i32 as f32 / 1000.0;
    (
        ms("stunDuration"),
        ms("stunPeriod"),
        rec.f32("stunChance").unwrap_or(0.0) as i32 as f32 * 0.01,
    )
}

/// Afflictions tick: what reacts to a status running out orders after `Tick`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AfflictionStage {
    Tick,
}
