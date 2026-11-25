//! The day / night clock (`AmbianceManager.time`, Engine.dll), hours in 0..24.
//!
//! `AmbianceManager::Reset` sets 9.0; `SetTime` clamps to 0..24; `GetTime` returns it while the
//! `dayNightCycle` option is on, else the fixed 12.0. It runs at 30x real time
//! (`AmbianceManager::ComputeShadowIntensity`). A character's first entry (`Game::LoadModMap`) calls
//! `SetTime(GameEngine::GetNewCharacterTimeOfDay)`: `newCharacterTime` of `gameengine.dbr` (9.0 when
//! absent, 2.0 in the stock data).
//!
//! PORT: the clock is reset to 9.0 per session (the engine's `Reset` calls not traced).
use bevy::prelude::*;

/// `AmbianceManager::Reset`.
pub const RESET_HOUR: f32 = 9.0;

#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct DayClock(pub f32);

impl Default for DayClock {
    fn default() -> Self {
        Self(RESET_HOUR)
    }
}

impl DayClock {
    /// `AmbianceManager::SetTime`.
    pub fn set(&mut self, hour: f32) {
        self.0 = hour.clamp(0.0, 24.0);
    }
}

/// `GameEngine::GetNewCharacterTimeOfDay`.
pub fn new_character_time(db: &gd_formats::vfs::Database) -> f32 {
    db.get("records/game/gameengine.dbr")
        .and_then(|r| r.f32("newCharacterTime"))
        .unwrap_or(RESET_HOUR)
}
