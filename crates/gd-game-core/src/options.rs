//! The game options (`options.txt`): read at start-up into [`GameOptions`], changed by the options window; every
//! system that honours an option reads it here. Format, table and engine defaults: `gd_formats::options`. The port
//! reads and writes its own copy of the file, never the game's.
//!
//! Not ported: `rockOn` (Grim Dawn.exe `MainMenu::PickCreditsMusic`; both choices load `Music_Credits.ogg`),
//! `speakerType` (Engine.dll `SoundManager::Update` → `IDirectSound8::SetSpeakerConfig`; Bevy mixes to stereo),
//! `toggledAurasOther`, `critFeedback`, `extraRollovers`, `reportStats` (engine no-ops) and the one-hit low-health
//! flash. `inactiveUpdateRate` <= 0 (the default) pauses game time and every sound while the window is unfocused.
use bevy::prelude::*;
use gd_formats::options::{Options, Value};

/// The session's options; the options window edits a draft and replaces this on Apply. Change through
/// [`Self::set`], which bumps [`Self::version`].
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct GameOptions {
    options: Options,
    /// Bumped on every change.
    pub version: u64,
}

impl std::ops::Deref for GameOptions {
    type Target = Options;
    fn deref(&self) -> &Options {
        &self.options
    }
}

impl GameOptions {
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
