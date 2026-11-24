//! Single-player pause: gameplay time stops while the game menu, the
//! options window or the pause key holds it.
//!
//! Grim Dawn.exe:
//! - `GAME::PauseGameTime` / `UnpauseGameTime` (Engine.dll) set one global
//!   flag the engine's game clock reads; every pause path is skipped when
//!   `Engine::IsNetworkEnabled` (multiplayer), so this is single player only;
//! - the HUD's pause (`UIPlayerHud::SetPaused`, a virtual): `(pause, sound,
//!   banner)`; pausing sets the screen dim's target to 0.6, pauses game time,
//!   pauses the `SoundManager` when `sound`, and shows the "Game Paused"
//!   banner when `banner`; unpausing sets the target to 0 and unpauses;
//! - the game menu (`hudExitWindow`, show `HudExitWindow::Show`) pauses with
//!   `(1, sound 1, banner 0)` and on hiding unpauses (sound only if it paused
//!   it); opening the options from it pauses `(1, 0, 0)` (`HudExitWindow::OpenOptions`);
//! - the pause key (key action 40, `tagKeybind37` "Pause Game (Single Player
//!   Only)", default `P`; handler `InGameUI::HandleKeyEvent` → `InGameUI::TogglePause`) toggles
//!   `(1, 1, 1)` / `(0, 1, 0)`, only while neither the options nor the game
//!   menu window is shown;
//! - losing the window focus with `inactiveUpdateRate` <= 0 (`Game::OnDeactivate`)
//!   pauses game time and every sound; regaining it (`Game::OnActivate`)
//!   unpauses unless another pause holds (`gd-audio/src/mixer.rs`
//!   `unfocused_mute`).
//!
//! Here the pause is the [`GamePause`] resource; `gd_game::pause::apply_pause` (in `Last`)
//! pauses Bevy's virtual clock, so the next frame's `Time` delta is zero for
//! every gameplay system and `FixedUpdate` does not run. Systems that act
//! without time (requests from input) are fed nothing while paused: the
//! game's input layer swallows world input (`grimdawn::ui::gamemenu`).
//!
//! The pause key's hold ([`GamePause::key`]) is gameplay state: the UI only
//! reads it and sends [`PauseKey`] requests (the game menu and the Esc
//! dispatcher, `grimdawn::ui`); `gd_game::pause::apply_pause_key` applies them. The binary
//! schedules that system in `Update` after its UI input set and before
//! `PlayerSet` (gd-game cannot name the UI sets), so a request holds the game
//! the same frame; the clock still follows from the next frame.
use bevy::prelude::*;

/// Why the game is paused (none: running).
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GamePause {
    /// The game menu or the options window opened from it.
    pub menu: bool,
    /// The pause key ("Game Paused" banner).
    pub key: bool,
    /// The window lost the focus (set on focus changes only).
    pub unfocused: bool,
    /// A video plays (`Game::LoadModMap` queues the intro: the engine pauses game time and the sounds while one
    /// does, `sub_14000F860` / `SoundManager::Pause`, but not the video's own stream); `grimdawn::video` sets it.
    /// The soundtrack's voice is `gd_audio`'s `PlaysWhilePaused`, so it keeps playing.
    pub video: bool,
}

impl GamePause {
    pub fn paused(&self) -> bool {
        self.menu || self.key || self.unfocused || self.video
    }
}

/// A request to change the pause key's hold ([`GamePause::key`]): the UI sends
/// this instead of writing the gameplay resource (the UI only reads). `Some(v)`
/// sets the hold (the game menu opening replaces the key pause with `false`,
/// the `GD_PAUSE` hook sets `true`); `None` toggles it (the pause key).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PauseKey(pub Option<bool>);

/// Run condition: gameplay is not paused.
pub fn running(pause: Res<GamePause>) -> bool {
    !pause.paused()
}
