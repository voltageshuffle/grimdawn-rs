//! The main-menu backdrop as the renderer reads it: the menu record's view, the menu map's sector data, its
//! levels and its fixed clock. The app's menu scene flow (`menu_scene.rs`) writes it while the menu scene is
//! open; the lighting environment (`render::env`), the weather driver and the jukebox
//! (`gd_audio::sessions::track_area`) light / weather / track the menu from it instead of the world. Present only
//! while the menu scene exists (`menu_scene.rs` removes it when the menu closes; `exit.rs` clears it at a
//! session's end).
use bevy::prelude::*;
use gd_formats::map::MapFile;
use gd_game_core::levels::Levels;
use std::sync::Arc;

/// A `gamemenu.tpl` record (grimdawn `menu_scene` module doc).
#[derive(Clone, Debug)]
pub struct MenuView {
    /// The record's place in the menu's record list.
    pub index: usize,
    pub map: String,
    pub eye: Vec3,
    pub target: Vec3,
    /// Vertical, radians.
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    /// The character's place: `mmPlayerPosition`, rotation, `mmPlayerScale`.
    pub player: Transform,
    pub music: Option<String>,
    /// `mmVignetteBitmap` and `mmLogo` (drawn by `ui/mainmenu.rs`).
    pub vignette: Option<String>,
    pub logo: Option<String>,
}

#[derive(Resource)]
pub struct MenuBackdrop {
    /// The menu record's camera, character place and art.
    pub view: MenuView,
    /// The menu map, once loaded (its sector data lights the scene).
    pub map: Option<Arc<MapFile>>,
    /// The scene's levels (sector grids for the environment's weights).
    pub levels: Levels,
    /// `mmTime` (`GD_TIME` overrides): the scene's fixed clock.
    pub time: f32,
}

impl MenuBackdrop {
    /// The sector entry of `layer` at `pos` in the menu map (the ambience, `gd_audio::sessions::track_area`: the
    /// map's ambient layer gives `ambpak_mainmenu`).
    pub fn sector_entry(
        &self,
        pos: Vec3,
        layer: usize,
    ) -> Option<&gd_formats::map::sector::SectorEntry> {
        self.levels.sector_entry(self.map.as_ref()?, pos, layer)
    }

    /// The menu camera's distance to its character (`mmCameraPosition` to `mmPlayerPosition`): the
    /// environment's depth-of-field focus (PORT: the engine's menu depth of field is unchecked).
    pub fn cam_distance(&self) -> f32 {
        self.view.eye.distance(self.view.player.translation)
    }
}
