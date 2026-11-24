//! The first-entry marker; `gd-game` adds the entry systems.
use bevy::prelude::*;

/// The session's player has entered the game (`gd_game::entry::first_entry` ran; removed by `end_session_state`).
#[derive(Resource)]
pub struct EnteredGame;
