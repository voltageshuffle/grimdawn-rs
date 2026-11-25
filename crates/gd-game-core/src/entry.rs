//! The first-entry marker.
use bevy::prelude::*;

/// The session's player has entered the game (removed by `end_session_state`).
#[derive(Resource)]
pub struct EnteredGame;
