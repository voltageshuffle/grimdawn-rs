//! Sound: area music and ambience from the sector "ambient" layer under the listener (`sessions`), `SoundPak`
//! one-shots for gameplay (`pak`), placed and skill loops (`emitters`), conversation and NPC voices (`dialog`), all
//! summed by the module's own mixer (`mixer`) from the file cache (`cache`).
//!
//! Records: `MusicSession {tracks[], randomize}`, `AmbientSession {ambientLoop(+Volume),
//! {frequent,moderate,unique}Sound1..N, *MinTime/*MaxTime, *Volume, night* variants}`, `SoundPak {soundName1..N,
//! weight1..N, volume, volumeSet, fallOff, pitchUp/pitchStep, force2d}`.
//!
//! Volumes (`gd_game_core::options`): every sound has a [`SoundClass`]; its gain is `masterVolume` × the class's option
//! (`musicVolume`, `effectsVolume`, `ambientVolume`, `dialogVolume`), read into `listener::Listener` when the options
//! change.
//! A sound's voice starts paused at zero volume; `mixer::mix` runs in `Last` (after every system that starts a
//! sound), sets the final volume and starts it the same frame its event fires.
//!
//! Pause: Engine.dll `SoundManager::Pause` (the game menu, its options window and the pause key
//! pause with sound) pauses every playing stream and every sound not flagged by `SoundPak::EnablePauseHack`, and starts
//! later unflagged ones at volume 0; the UI's widgets flag their sounds, so a click plays during the pause. PORT: a
//! sound started while the menu or the pause key holds the game is taken to be such a UI sound and plays through the
//! pause. The session timers and fades run on virtual [`Time`], so they freeze too. While the window is not focused
//! every sound is paused (`mixer::unfocused_mute`).
//!
//! Devices (`devices`): the options Audio tab lists the host's output / input devices by name (index 0 = system
//! default, i >= 1 = entry i - 1). bevy_audio 0.20 opens only the default sink (`AudioOutput` is pub(crate), no
//! device hook), so the bus plays through a port-owned rodio sink on the selected `soundDevice`
//! (`mixer::start_bus`); Apply reopens it without dropping the voice queue (`mixer::switch_device`).
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

pub(crate) use emitters::{spawn_emitter, spawn_loop};
pub(crate) use listener::{BaseVolume, SoundClass};
pub(crate) use mixer::{Fade, spawn_2d};
pub(crate) use pak::{ENGINE_HZ, PeriodicSound, play_pak};
pub(crate) use sessions::Ambience;

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
                    ),
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
                    ),
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

/// The sound files, the `SoundPak` facts read so far, the listener, and the mixer's input and output.
#[derive(Resource, Default)]
pub struct Sounds {
    /// Loaded files, held weakly (`cache::WeakLoaded`): a playing voice or a creature's preloaded sounds
    /// (`cache::HeldSounds`) keep a file.
    cache: HashMap<String, Option<cache::WeakLoaded>>,
    /// The last files used, played or preloaded, held strongly (`gd_game_core::weak_cache::Recent`;
    /// `cache::reset_sounds` empties it).
    recent: gd_game_core::weak_cache::Recent<cache::Loaded>,
    /// The size at which the next new file drops the dead entries of `cache`.
    prune_at: usize,
    /// `SoundPak` records by name, read once (`pak::PakInfo`); cleared with the files on a mod change.
    paks: HashMap<String, Option<Arc<pak::PakInfo>>>,
    /// The listener's limits for [`play_pak`].
    pub listener: listener::Listener,
    /// Background file reads in flight (`cache::preload_sounds`), drained by `cache::collect_preload`.
    preload: Vec<bevy::tasks::Task<Vec<(String, Option<cache::Loaded>)>>>,
    /// File keys being read in the background, so a file is read once.
    in_flight: HashSet<String>,
    /// The preloads waiting for their files, each handed to its owner once complete (`cache::Hold`).
    holds: Vec<cache::Hold>,
    /// One-shots waiting for their file's background read (`pak::replay_pending`).
    pending: Vec<pak::PendingPlay>,
    /// New voices to the mixer; set at startup (`mixer::start_bus`).
    bus: Option<mpsc::SyncSender<mixer::BusVoice>>,
    /// The mixer's rate (the opened sink's; `mixer::open_bus`).
    bus_hz: u32,
    /// The bus's mixing state, shared with its rodio source (`mixer::BusState`); survives a device switch.
    bus_state: Option<Arc<Mutex<mixer::BusState>>>,
    /// The port-owned rodio sink playing the bus (`mixer::start_bus`), reopened on a `soundDevice` change
    /// (`mixer::switch_device`).
    bus_out: Option<mixer::BusSink>,
}
