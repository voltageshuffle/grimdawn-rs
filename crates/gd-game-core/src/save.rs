//! Save data shared with the world: the open map's layout ([`MapLayout`]).
//! The save writing and loading are `gd_game::save`.
use bevy::prelude::*;

/// The open map, for the region ids and placements `map.dat` needs (the same `Arc` as the
/// renderer's world; set by it when the map opens, replaced by the next session's).
#[derive(Resource, Clone)]
pub struct MapLayout(pub std::sync::Arc<gd_formats::map::MapFile>);

/// `GameEngine::GetGameDifficultyAsPath`.
pub const DIFFICULTY_DIRS: [&str; 3] = ["Normal", "Elite", "Ultimate"];
