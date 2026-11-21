//! The teleport activity: every travel (dungeon entrances, riftgates, portals, the Shattered Realm's doors and
//! gates) runs one, fading the screen out, loading the destination, placing the player and fading back in.
//! [`crate::doors::run_teleport`] drives it; the destination's ground comes from [`crate::nav::teleport_landing`].
use bevy::prelude::*;

/// A travel (door, riftgate, portal) in progress: its destination, and the player's teleport state
/// (`Character::IsTeleporting`, Character +9258, set by the teleport activity from its start to its last
/// phase, sub_180582330 / sub_1805823E0). Set by the travel requests, run by [`TeleportActivity`] and cleared when it
/// ends ([`crate::doors::run_teleport`]). `InstallTeleportActivity` @0x1802d6a30: a second teleport while one runs is
/// dropped (`ActivityManager::IsAlreadyTeleporting`): requests only set it when it is empty.
#[derive(Resource, Default)]
pub struct PendingTravel(pub Option<Vec3>);

/// The local player's teleport activity (`TeleportLocalClientActivity`, Game.dll vtable 0x18079DDB0; every
/// [`PendingTravel`], whatever its distance). Update sub_1805823E0 (ms; each phase change restarts the
/// clock):
/// - start: the player is frozen (Character +8048) and teleporting (+9258);
/// - [`TeleportPhase::FadeOut`]: the loading screen fades in over [`TELEPORT_FADE_MS`] (+52);
/// - [`TeleportPhase::Loading`]: `Engine::SetTeleportMode(true)`; the destination's `RegionLoader` (helper
///   sub_1803B8750: the regions in the player's frustum at the destination) loads; once it is done the clock is
///   taken modulo [`TELEPORT_HOURGLASS_MS`] (+88, the hourglass' turn) and the phase ends when it reaches it again;
///   then the player is put at the destination (vtable +72 = sub_1805820F0, [`crate::nav::teleport_landing`]);
/// - [`TeleportPhase::FadeIn`]: the screen fades back over [`TELEPORT_FADE_MS`]; then the player is unfrozen and
///   (a frame later, PORT: the same frame) no longer teleporting.
///
/// The screen (render sub_180581850): alpha = clock / fade in FadeOut, 1 while loading, 1 − clock / fade in FadeIn.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct TeleportActivity {
    pub phase: TeleportPhase,
    /// Milliseconds in the phase (+20).
    pub ms: u32,
    /// The destination is loaded (+92).
    pub loaded: bool,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeleportPhase {
    #[default]
    Idle,
    FadeOut,
    Loading,
    FadeIn,
}

/// Base activity ctor sub_180580F60: +52 = 1000.
pub const TELEPORT_FADE_MS: u32 = 1000;
/// sub_180580F60: +88 = 1000.
pub const TELEPORT_HOURGLASS_MS: u32 = 1000;

/// What a [`TeleportActivity::step`] asks the caller to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeleportStep {
    /// Nothing this frame.
    Wait,
    /// The fade-out is over: start loading the destination.
    Load,
    /// Put the player at the destination.
    Move,
    /// The activity is over: the player is free again.
    Done,
}

impl TeleportActivity {
    pub fn active(&self) -> bool {
        self.phase != TeleportPhase::Idle
    }

    /// Starts the activity (state 0 → 1).
    pub fn start(&mut self) {
        *self = Self {
            phase: TeleportPhase::FadeOut,
            ..default()
        };
    }

    fn enter(&mut self, phase: TeleportPhase) {
        self.phase = phase;
        self.ms = 0;
    }

    /// Advances by `dt_ms`; `ready` = the destination is loaded (only read while loading).
    pub fn step(&mut self, dt_ms: u32, ready: bool) -> TeleportStep {
        self.ms += dt_ms;
        match self.phase {
            TeleportPhase::Idle => TeleportStep::Wait,
            TeleportPhase::FadeOut if self.ms > TELEPORT_FADE_MS => {
                self.enter(TeleportPhase::Loading);
                TeleportStep::Load
            }
            TeleportPhase::Loading if self.loaded => {
                if self.ms >= TELEPORT_HOURGLASS_MS {
                    self.enter(TeleportPhase::FadeIn);
                    TeleportStep::Move
                } else {
                    TeleportStep::Wait
                }
            }
            TeleportPhase::Loading if ready => {
                self.loaded = true;
                self.ms %= TELEPORT_HOURGLASS_MS;
                TeleportStep::Wait
            }
            TeleportPhase::FadeIn if self.ms > TELEPORT_FADE_MS => {
                *self = default();
                TeleportStep::Done
            }
            _ => TeleportStep::Wait,
        }
    }

    /// The loading screen's opacity (module doc of [`TeleportActivity`]).
    pub fn cover_alpha(&self) -> f32 {
        let t = (self.ms as f32 / TELEPORT_FADE_MS as f32).clamp(0.0, 1.0);
        match self.phase {
            TeleportPhase::Idle => 0.0,
            TeleportPhase::FadeOut => t,
            TeleportPhase::Loading => 1.0,
            TeleportPhase::FadeIn => 1.0 - t,
        }
    }
}
