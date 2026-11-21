//! The day / night clock (`AmbianceManager` +600, Engine.dll): the hour the `dayNightCycle` option shows.
//!
//! Engine.dll: `AmbianceManager::Reset` @0x180031790 sets the clock to 9.0; `SetTime` @0x180032d00
//! clamps its argument to 0..24 and sets it; `GetTime` @0x180032d30 returns it while the `dayNightCycle` option
//! is on, else the fixed 12.0 (+608, set by the constructor @0x1800315d0). The clock runs at 30 times real time
//! (24 game hours in 48 minutes; `sub_1800345F0`).
//! Game.dll / Grim Dawn.exe: a character's first entry into the game (`entry::first_entry`, the
//! client's world load sub_140015530) calls `SetTime(GameEngine::GetNewCharacterTimeOfDay)`: `newCharacterTime`
//! of `records/game/gameengine.dbr` (`GameEngine::LoadFromDatabase` @0x1802c8750; 9.0 when the field is absent,
//! the `GameEngine` constructor's), 2.0 in the stock data. A new character starts at 2:00 at night.
//!
//! PORT: the clock is reset to 9.0 per session (the port's stand-in for the engine's `Reset` calls, not traced).
//! The grimdawn side (`render::env::TimeOfDay`) shows it, or 12.0 with the option off.
use bevy::prelude::*;

/// The clock's start (`AmbianceManager::Reset`).
pub const RESET_HOUR: f32 = 9.0;

/// The running clock, hours in 0..24.
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

pub struct DayClockPlugin;

impl Plugin for DayClockPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DayClock>().add_systems(
            Update,
            advance_day_clock
                .run_if(resource_exists::<crate::entry::EnteredGame>)
                .after(crate::player::PlayerSet),
        );
    }
}

/// The clock runs while the option is on (the game's pause stops it: virtual time).
fn advance_day_clock(
    mut clock: ResMut<DayClock>,
    options: Res<crate::options::GameOptions>,
    time: Res<Time<Virtual>>,
) {
    if options.flag("dayNightCycle") && time.delta_secs() > 0.0 {
        clock.0 = (clock.0 + time.delta_secs() * 30.0 / 3600.0) % 24.0;
    }
}
