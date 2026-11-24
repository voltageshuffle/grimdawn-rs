//! The port's settings-file helpers the UI kit shares: the file's path ([`path`]), the write-back
//! ([`store`]) and the UI-scale option ([`UiScaleOption`]). The settings systems (load / apply_video /
//! apply_ui_scale / apply_language) stay in the binary; only the pieces the kit and the key bindings
//! read live here.
use bevy::prelude::*;
use gd_game_core::options::GameOptions;
use std::path::PathBuf;

/// The port's settings file.
pub fn path() -> PathBuf {
    gd_game_core::session::data_dir().join("Settings/options.txt")
}

/// Writes the options to the port's file (never into the game's folders).
pub fn store(options: &GameOptions) {
    gd_game_core::session::write_user_file("options", &path(), options.write());
}

/// `uiScale` (float 11): the slider in [0, 1]; it counts in game, the menus use 0.5
/// (the default). PORT: it takes effect on Apply, not the frame the slider moves; the UI reads it when built, and the
/// built HUD, windows and panes carry the scale factor in their rebuild signatures, so an Apply rebuilds what is shown.
#[derive(Resource, Clone, Copy, PartialEq)]
pub struct UiScaleOption(pub f32);

impl Default for UiScaleOption {
    fn default() -> Self {
        Self(0.5)
    }
}

impl UiScaleOption {
    /// The UI scale factor at a `width` × `height` physical-pixel screen.
    pub fn factor(self, width: u32, height: u32) -> f32 {
        gd_formats::ui::ui_scale_factor(width, height, self.0)
    }
}
