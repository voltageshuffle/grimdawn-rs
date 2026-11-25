//! Save data shared with the world: the open map's layout ([`MapLayout`]).
use bevy::prelude::*;

/// The open map, for the region ids and placements `map.dat` needs (the renderer's `Arc`).
#[derive(Resource, Clone)]
pub struct MapLayout(pub std::sync::Arc<gd_formats::map::MapFile>);

/// `GameEngine::GetGameDifficultyAsPath`.
pub const DIFFICULTY_DIRS: [&str; 3] = ["Normal", "Elite", "Ultimate"];
