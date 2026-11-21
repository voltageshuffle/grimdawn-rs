//! Single-player pause: gameplay time stops while the game menu, the
//! options window or the pause key holds it.
//!
//! Grim Dawn.exe:
//! - `GAME::PauseGameTime` / `UnpauseGameTime` (Engine.dll) set one global
//!   flag the engine's game clock reads; every pause path is skipped when
//!   `Engine::IsNetworkEnabled` (multiplayer), so this is single player only;
//! - the game window's pause (sub_14020A480, vtable +232): `(pause, sound,
//!   banner)`; pausing sets the screen dim's target to 0.6, pauses game time,
//!   pauses the `SoundManager` when `sound`, and shows the "Game Paused"
//!   banner when `banner`; unpausing sets the target to 0 and unpauses;
//! - the game menu (`hudExitWindow`, show sub_14026E820) pauses with
//!   `(1, sound 1, banner 0)` and on hiding unpauses (sound only if it paused
//!   it); opening the options from it pauses `(1, 0, 0)` (sub_14021BD50);
//! - the pause key (key action 40, `tagKeybind37` "Pause Game (Single Player
//!   Only)", default `P`; handler sub_140026910 → sub_140026860) toggles
//!   `(1, 1, 1)` / `(0, 1, 0)`, only while neither the options nor the game
//!   menu window is shown;
//! - losing the window focus with `inactiveUpdateRate` <= 0 (sub_140011AC0)
//!   pauses game time and every sound; regaining it (sub_140011B20)
//!   unpauses unless another pause holds (`grimdawn/src/audio/mixer.rs`
//!   `unfocused_mute`).
//!
//! Here the pause is the [`GamePause`] resource; [`apply_pause`] (in `Last`)
//! pauses Bevy's virtual clock, so the next frame's `Time` delta is zero for
//! every gameplay system and `FixedUpdate` does not run. Systems that act
//! without time (requests from input) are fed nothing while paused: the
//! game's input layer swallows world input (`grimdawn::ui::gamemenu`).
use bevy::prelude::*;

pub struct PausePlugin;

impl Plugin for PausePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GamePause>()
            .add_systems(Last, apply_pause);
    }
}

/// Why the game is paused (none: running).
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GamePause {
    /// The game menu or the options window opened from it.
    pub menu: bool,
    /// The pause key ("Game Paused" banner).
    pub key: bool,
    /// The window lost the focus (set on focus changes only).
    pub unfocused: bool,
}

impl GamePause {
    pub fn paused(&self) -> bool {
        self.menu || self.key || self.unfocused
    }
}

/// Run condition: gameplay is not paused.
pub fn running(pause: Res<GamePause>) -> bool {
    !pause.paused()
}

/// Pauses / unpauses the virtual clock (`Time` in the gameplay schedules)
/// to match [`GamePause`]; takes effect from the next frame.
pub fn apply_pause(pause: Res<GamePause>, mut time: ResMut<Time<Virtual>>) {
    match (pause.paused(), time.is_paused()) {
        (true, false) => time.pause(),
        (false, true) => time.unpause(),
        _ => {}
    }
}
