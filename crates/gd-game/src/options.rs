//! The game options (`options.txt`): read once at start-up into [`GameOptions`]
//! and changed by the options window (`grimdawn::ui::options`). Every
//! system that honours an option reads it here (one source per value).
//! The file format, the option table and the engine's defaults are `gd_formats::options`.
//!
//! The port reads and writes its own copy of the file (`grimdawn::settings`), never the game's.
//!
//! `errorSpeech` (bool 36) gates the player's spoken error lines ([`crate::error_vox`]). `rockOn` (bool 13) is read
//! only by Grim Dawn.exe sub_1400C33F0 (`rockOn`, or `GetPlayerCompletedLevel` and a `strdate` check), which picks one of two descriptors
//! of the credits music (sub_1400BE320 case 9, sub_1400C39B0), both loaded from `Sound/Music/Music_Credits.ogg`
//! (sub_1400C35B0): not ported on purpose, it changes nothing audible. `speakerType` (int 9) is read only by
//! Engine.dll `SoundManager::Update` @0x1801b5b10, which hands it to DS8Sound.dll `DS8SoundSystem::SetSpeakerType`
//! @0x1800043b0 = `IDirectSound8::SetSpeakerConfig` (0 headphones, 1 stereo, 2 quad, 3 surround, 4 5.1, 5 7.1): by
//! design not ported (Bevy / rodio mix to stereo; the window does not show it). Not ported on purpose: `toggledAurasOther`,
//! `critFeedback`, `extraRollovers` and `reportStats` are no-ops in the engine, and the one-hit low-health flash is
//! a stock quirk that never fires in normal play. `inactiveUpdateRate` <= 0 (the default) pauses game time and every
//! sound while the window is unfocused (`grimdawn/src/audio/mixer.rs` `unfocused_mute`, `GamePause::unfocused`).
use bevy::prelude::*;
use gd_formats::options::{Options, Value};

/// The session's options (one copy; the options window edits a draft and
/// replaces this on Apply). Reads go through [`Options`] (`Deref`); changes
/// through [`Self::set`], which keeps [`Self::version`].
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct GameOptions {
    options: Options,
    /// Bumped on every change (readers that cache drawn state compare it).
    pub version: u64,
}

impl std::ops::Deref for GameOptions {
    type Target = Options;
    fn deref(&self) -> &Options {
        &self.options
    }
}

impl GameOptions {
    /// The options of an `options.txt` text ([`Options::parse`]).
    pub fn parse(text: &str) -> Self {
        Self {
            options: Options::parse(text),
            version: 0,
        }
    }

    /// Sets a known option (same kind as its default); false otherwise.
    pub fn set(&mut self, name: &str, v: Value) -> bool {
        let changed = self.options.get(name) != Some(&v);
        let ok = self.options.set(name, v);
        if ok && changed {
            self.version += 1;
        }
        ok
    }

    /// The same values (ignoring [`Self::version`]).
    pub fn same_values(&self, other: &Self) -> bool {
        self.options == other.options
    }
}
