//! Sound: area music and ambience from the sector "ambient" layer under the listener (`sessions`), `SoundPak`
//! one-shots for gameplay (`pak`), placed and skill loops (`emitters`), conversation and NPC voices (`dialog`), all
//! summed by the module's own mixer (`mixer`) from the file cache (`cache`).
//!
//! Records: `MusicSession {tracks[], randomize}`, `AmbientSession {ambientLoop(+Volume),
//! {frequent,moderate,unique}Sound1..N, *MinTime/*MaxTime, *Volume, night* variants}`, `SoundPak {soundName1..N,
//! weight1..N, volume, volumeSet, fallOff, pitchUp/pitchStep, force2d}`.
//!
//! Volumes (`gd_game_core::options`): every sound has a [`SoundClass`]; its gain is `masterVolume` × the class's option
//! (`musicVolume`, `effectsVolume`, `ambientVolume`, `dialogVolume`). A voice plays at its own volume × that gain
//! clamped to 1 (DirectSound never amplifies); a 3D voice's distance gain and pan come on top.
//! A sound's voice starts paused at zero volume; `mixer::mix` runs in `Last`, sets the final volume and starts it
//! the same frame its event fires.
//!
//! Pause: `SoundManager::Pause` pauses every playing stream and every sound not flagged by
//! `SoundPak::EnablePauseHack`, and starts later unflagged ones at volume 0; UI widgets flag their sounds. PORT: a
//! pak played without a position ([`mixer::PlaysWhilePaused`]) and a video's soundtrack play during the pause;
//! every other voice is held and resumes in place. Session timers and fades run on virtual [`Time`]. While the
//! window is not focused every sound is paused.
//!
//! Devices: the options Audio tab lists output / input devices by name (index 0 = system default, i >= 1 = entry
//! i - 1). PORT: the bus plays through a port-owned rodio sink on the selected `soundDevice` (bevy_audio opens only
//! the default sink); Apply reopens it without dropping the voice queue.
#![allow(clippy::type_complexity)]
use bevy::prelude::*;
use gd_formats::collections::{HashMap, HashSet};
use parking_lot::Mutex;
use std::sync::{Arc, mpsc};

mod cache;
pub mod devices;
mod dialog;
mod emitters;
mod listener;
mod mixer;
mod pak;
mod sessions;

pub use emitters::{spawn_emitter, spawn_loop};
pub use listener::{BaseVolume, SoundClass};
pub use mixer::{Fade, LiveAudio, spawn_2d, spawn_live};
pub use pak::{ENGINE_HZ, play_pak};
pub use sessions::Ambience;

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sounds>()
            .init_resource::<Ambience>()
            .init_resource::<devices::AudioDevices>()
            .add_systems(
                First,
                cache::reset_sounds.after(gd_render::model::drop_on_mod_change),
            )
            .add_systems(Startup, (devices::init_devices, mixer::start_bus).chain())
            .add_systems(
                Update,
                // Two chained halves (a tuple holds at most 20 systems; nested tuples are unordered): the listener
                // and landed files before any sound starts, a skill loop moved before its emitter is heard.
                (
                    (
                        mixer::focus_pause,
                        mixer::switch_device,
                        listener::listen,
                        cache::collect_preload,
                        pak::replay_pending,
                        cache::start_awaited,
                        sessions::track_area,
                        sessions::cache_monster_music,
                        mixer::fade_sounds,
                        sessions::boss_and_event_music,
                    )
                        .chain_ignore_deferred(),
                    (
                        sessions::drive_sessions,
                        sessions::ambient_oneshots,
                        pak::periodic_sounds,
                        pak::story_sounds,
                        dialog::dialog_voice,
                        dialog::npc_voices,
                        pak::play_requests
                            .after(gd_game::combat_sounds::CombatSoundsSet)
                            .after(gd_game::casting::CastingSet),
                        pak::positioned_sounds,
                        emitters::skill_loops,
                        emitters::emitters,
                        cache::preload_sounds,
                    )
                        .chain_ignore_deferred(),
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                (
                    pak::character_callbacks
                        .after(gd_game_core::character::CharacterStage::Animate),
                    mixer::reap_voices,
                    mixer::refill_spares,
                ),
            )
            .add_systems(Last, mixer::mix);
    }
}

#[derive(Resource, Default)]
pub struct Sounds {
    /// Held weakly: a playing voice or a creature's preloaded sounds keep a file.
    cache: HashMap<String, Option<cache::WeakLoaded>>,
    /// The last files used, held strongly.
    recent: gd_game_core::weak_cache::Recent<cache::Loaded>,
    /// The size at which the next new file drops the dead entries of `cache`.
    prune_at: usize,
    /// `SoundPak` records by name; cleared with the files on a mod change.
    paks: HashMap<String, Option<Arc<pak::PakInfo>>>,
    pub listener: listener::Listener,
    /// Background file reads in flight.
    preload: Vec<bevy::tasks::Task<Vec<(String, Option<cache::Loaded>)>>>,
    in_flight: HashSet<String>,
    /// Preloads waiting for their files, each handed to its owner once complete.
    holds: Vec<cache::Hold>,
    /// One-shots waiting for their file's background read.
    pending: Vec<pak::PendingPlay>,
    bus: Option<mpsc::SyncSender<mixer::BusVoice>>,
    /// Shared with the bus's rodio source; survives a device switch.
    bus_state: Option<Arc<Mutex<mixer::BusState>>>,
    /// Reopened on a `soundDevice` change.
    bus_out: Option<mixer::BusSink>,
}
