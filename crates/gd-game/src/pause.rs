//! The pause systems: applying [`PauseKey`] requests and pausing Bevy's
//! virtual clock to match [`GamePause`].
//!
//! The binary schedules [`apply_pause_key`] after its UI input and before
//! `PlayerSet`, so a request holds the game the same frame; the clock follows
//! from the next frame.
use bevy::prelude::*;
use gd_game_core::pause::{GamePause, PauseKey};

pub struct PausePlugin;

impl Plugin for PausePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GamePause>()
            .add_message::<PauseKey>()
            .add_systems(Last, apply_pause);
    }
}

/// Applies the [`PauseKey`] requests to [`GamePause::key`].
pub fn apply_pause_key(mut msgs: MessageReader<PauseKey>, mut pause: ResMut<GamePause>) {
    for m in msgs.read() {
        pause.key = m.0.unwrap_or(!pause.key);
    }
}

/// Pauses / unpauses the virtual clock to match [`GamePause`]; takes effect from the next frame.
pub fn apply_pause(pause: Res<GamePause>, mut time: ResMut<Time<Virtual>>) {
    match (pause.paused(), time.is_paused()) {
        (true, false) => time.pause(),
        (false, true) => time.unpause(),
        _ => {}
    }
}
