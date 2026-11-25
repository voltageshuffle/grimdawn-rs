//! Advances the [`DayClock`] while the `dayNightCycle` option is on.
use bevy::prelude::*;
use gd_game_core::day_clock::DayClock;

pub struct DayClockPlugin;

impl Plugin for DayClockPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DayClock>().add_systems(
            Update,
            advance_day_clock
                .run_if(resource_exists::<gd_game_core::entry::EnteredGame>)
                .after(gd_game_core::player::PlayerSet),
        );
    }
}

/// Virtual time: the game's pause stops the clock.
fn advance_day_clock(
    mut clock: ResMut<DayClock>,
    options: Res<gd_game_core::options::GameOptions>,
    time: Res<Time<Virtual>>,
) {
    if options.flag("dayNightCycle") && time.delta_secs() > 0.0 {
        clock.0 = (clock.0 + time.delta_secs() * 30.0 / 3600.0) % 24.0;
    }
}
