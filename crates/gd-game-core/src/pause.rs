//! Single-player pause: gameplay time stops while the game menu, the
//! options window or the pause key holds it.
//!
//! Grim Dawn.exe:
//! - `GAME::PauseGameTime` / `UnpauseGameTime` (Engine.dll) set one global flag; every pause path
//!   is skipped when `Engine::IsNetworkEnabled`;
//! - `UIPlayerHud::SetPaused(pause, sound, banner)`: pausing sets the screen dim's target to 0.6,
//!   pauses game time, the `SoundManager` when `sound`, and shows "Game Paused" when `banner`;
//!   unpausing sets the target to 0 and unpauses;
//! - the game menu (`HudExitWindow::Show`) pauses `(1, 1, 0)` and unpauses on hiding (sound only if
//!   it paused it); its options pause `(1, 0, 0)` (`HudExitWindow::OpenOptions`);
//! - the pause key (key action 40, default `P`; `InGameUI::TogglePause`) toggles `(1, 1, 1)` /
//!   `(0, 1, 0)`, only while neither the options nor the game menu is shown;
//! - losing focus with `inactiveUpdateRate` <= 0 (`Game::OnDeactivate`) pauses game time and every
//!   sound; `Game::OnActivate` unpauses unless another pause holds.
//!
//! `gd_game::pause::apply_pause` (in `Last`) pauses Bevy's virtual clock, so the next frame's `Time`
//! delta is zero and `FixedUpdate` does not run; the input layer swallows world input while paused.
//!
//! The pause key's hold ([`GamePause::key`]) is gameplay state: the UI sends [`PauseKey`] requests;
//! the binary schedules `gd_game::pause::apply_pause_key` in `Update` after its UI input set and
//! before `PlayerSet`, so a request holds the game the same frame.
use bevy::prelude::*;

/// Why the game is paused (none: running).
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GamePause {
    /// The game menu or the options window opened from it.
    pub menu: bool,
    /// The pause key.
    pub key: bool,
    /// The window lost the focus.
    pub unfocused: bool,
    /// A video plays (`Game::UpdateVideo` pauses game time and sounds, not the video's own stream).
    pub video: bool,
}

impl GamePause {
    pub fn paused(&self) -> bool {
        self.menu || self.key || self.unfocused || self.video
    }
}

/// A request to change the pause key's hold ([`GamePause::key`]): `Some(v)` sets it, `None` toggles.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PauseKey(pub Option<bool>);

pub fn running(pause: Res<GamePause>) -> bool {
    !pause.paused()
}
