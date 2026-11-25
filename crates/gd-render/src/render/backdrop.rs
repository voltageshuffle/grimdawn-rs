//! The main-menu backdrop as the renderer reads it: the menu record's view, the menu map's sector data, its
//! levels and its fixed clock. Present only while the menu scene exists.
use bevy::prelude::*;
use gd_formats::map::MapFile;
use gd_game_core::levels::Levels;
use std::sync::Arc;

/// A `gamemenu.tpl` record.
#[derive(Clone, Debug)]
pub struct MenuView {
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
    /// `mmVignetteBitmap` and `mmLogo`.
    pub vignette: Option<String>,
    pub logo: Option<String>,
}

#[derive(Resource)]
pub struct MenuBackdrop {
    pub view: MenuView,
    /// `None` until loaded.
    pub map: Option<Arc<MapFile>>,
    pub levels: Levels,
    /// `mmTime` (`GD_TIME` overrides): the scene's fixed clock.
    pub time: f32,
}

impl MenuBackdrop {
    /// The sector entry of `layer` at `pos` in the menu map.
    pub fn sector_entry(
        &self,
        pos: Vec3,
        layer: usize,
    ) -> Option<&gd_formats::map::sector::SectorEntry> {
        self.levels.sector_entry(self.map.as_ref()?, pos, layer)
    }

    /// The menu camera's distance to its character (`mmCameraPosition` to `mmPlayerPosition`): the depth-of-field
    /// focus (PORT: the engine's menu depth of field is unchecked).
    pub fn cam_distance(&self) -> f32 {
        self.view.eye.distance(self.view.player.translation)
    }
}
