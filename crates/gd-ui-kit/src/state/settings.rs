//! The port's settings-file helpers the UI kit shares.
use bevy::prelude::*;
use gd_game_core::options::GameOptions;
use std::path::PathBuf;

pub fn path() -> PathBuf {
    gd_game_core::session::data_dir().join("Settings/options.txt")
}

/// Writes the options to the port's file (never into the game's folders).
pub fn store(options: &GameOptions) {
    gd_game_core::session::write_user_file("options", &path(), options.write());
}

/// `uiScale` (float 11): the slider in [0, 1]; the menus use 0.5.
/// PORT: takes effect on Apply, not as the slider moves.
#[derive(Resource, Clone, Copy, PartialEq)]
pub struct UiScaleOption(pub f32);

impl Default for UiScaleOption {
    fn default() -> Self {
        Self(0.5)
    }
}

impl UiScaleOption {
    pub fn factor(self, width: u32, height: u32) -> f32 {
        gd_formats::ui::ui_scale_factor(width, height, self.0)
    }
}
