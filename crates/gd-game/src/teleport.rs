//! The teleport activity every travel runs: fade out, load the destination, place the player, fade in.
use bevy::prelude::*;

/// A travel in progress: its destination; also the player's `Character.isTeleporting`. Cleared when the
/// activity ends. A second teleport while one runs is dropped (`ActivityManager::IsAlreadyTeleporting`):
/// requests only set it when empty.
#[derive(Resource, Default)]
pub struct PendingTravel(pub Option<Vec3>);

/// `TeleportLocalClientActivity` (Game.dll), for every [`PendingTravel`]. `Update` (ms; each phase change
/// restarts the clock):
/// - start: the player is frozen and teleporting;
/// - [`TeleportPhase::FadeOut`]: the loading screen fades in over [`TELEPORT_FADE_MS`];
/// - [`TeleportPhase::Loading`]: the destination's `RegionLoader` loads; once done the clock is taken modulo
///   [`TELEPORT_HOURGLASS_MS`] and the phase ends when it reaches it again; then `PlacePlayer`;
/// - [`TeleportPhase::FadeIn`]: the screen fades back over [`TELEPORT_FADE_MS`]; then the player is unfrozen
///   and (a frame later, PORT: the same frame) no longer teleporting.
///
/// `Render`: alpha = clock / fade in FadeOut, 1 while loading, 1 − clock / fade in FadeIn.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct TeleportActivity {
    pub phase: TeleportPhase,
    /// `Activity.elapsedMs`.
    pub ms: u32,
    /// `TeleportLocalClientActivity.loaded`.
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

/// `Activity::Activity`: `Activity.fadeMs`.
pub const TELEPORT_FADE_MS: u32 = 1000;
/// `Activity::Activity`: `Activity.hourglassMs`.
pub const TELEPORT_HOURGLASS_MS: u32 = 1000;

/// What a [`TeleportActivity::step`] asks the caller to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeleportStep {
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

    /// `ready`: the destination is loaded (read only while loading).
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
