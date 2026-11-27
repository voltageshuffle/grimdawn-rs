//! The sound file cache ([`Sounds`]) and its background preload.
//!
//! The cache holds its files weakly ([`WeakLoaded`]): a file stays while a voice plays it or a creature holds it
//! ([`HeldSounds`]), and goes with the last of them (PORT: the engine unloads a sound 5 s after its last release).
//! The last files used, played or preloaded, stay loaded too, up to 96 MB (`Sounds::recent`).
//!
//! Preload: [`preload_sounds`] warms the cache in background tasks when a creature spawns or the player's equipment /
//! skills change (`SoundPak::PreLoad`). The creature holds what was preloaded for it ([`HeldSounds`]) until it goes
//! or its kit changes. A one-shot whose file is not loaded starts its read in the background and plays when it
//! lands ([`Sounds::file_or_fetch`]); so do event / boss music and conversation voice-overs, whose sound entity
//! exists from the start and gets its voice then ([`start_2d`], [`AwaitedFile`]). Only the main menu and the
//! credits read their music on the spot ([`Sounds::file`]).
use super::Sounds;
use super::listener::{SoundClass, sound};
use super::mixer::{BUFFER_BYTES, OnEnd, Pcm, Voice3d, VoiceStart, attach_voice};
use super::pak::ENGINE_HZ;
use bevy::audio::{Decodable, Source};
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_formats::vfs::normalized;
use gd_game_core::data::GameData;
use std::borrow::Cow;
use std::sync::{Arc, Weak};

const PRUNE_MIN: usize = 64;

/// A sound file and how the engine plays it (`pak::playback_speed`).
#[derive(Clone)]
pub struct Loaded {
    /// The file's bytes (a stream decodes them while it plays).
    pub(crate) file: AudioSource,
    pub(super) pcm: Option<Pcm>,
    /// The WAV's own sample rate; `None` for an `.ogg` (a stream).
    pub rate: Option<u32>,
    /// A one-channel file: only these become 3D buffers.
    pub mono: bool,
}

impl Loaded {
    /// Playback speed at the engine's frequency `freq` Hz.
    pub fn speed(&self, freq: f32) -> f32 {
        super::pak::playback_speed(freq, self.rate)
    }
}

impl gd_game_core::weak_cache::Shared for Loaded {
    /// The recently used files stay loaded up to 96 MB (file bytes + decoded samples).
    const BUDGET: usize = 96 << 20;
    fn id(&self) -> usize {
        Arc::as_ptr(&self.file.bytes).cast::<u8>() as usize
    }
    fn cost(&self) -> usize {
        self.file.bytes.len() + self.pcm.as_ref().map_or(0, |p| p.samples.len() * 4)
    }
}

/// A cached [`Loaded`] that keeps nothing alive: the file's bytes and decoded samples as `Weak`s.
pub(super) struct WeakLoaded {
    bytes: Weak<[u8]>,
    /// The decoded samples, channels and rate.
    pcm: Option<(Weak<[f32]>, u16, u32)>,
    rate: Option<u32>,
    mono: bool,
}

impl WeakLoaded {
    fn of(l: &Loaded) -> Self {
        Self {
            bytes: Arc::downgrade(&l.file.bytes),
            pcm: l
                .pcm
                .as_ref()
                .map(|p| (Arc::downgrade(&p.samples), p.channels, p.rate)),
            rate: l.rate,
            mono: l.mono,
        }
    }

    /// The decoded samples, while a voice still plays them.
    fn pcm(&self) -> Option<Pcm> {
        let (samples, channels, rate) = self.pcm.as_ref()?;
        Some(Pcm {
            samples: samples.upgrade()?,
            channels: *channels,
            rate: *rate,
        })
    }

    /// The file, while something holds its bytes.
    fn upgrade(&self) -> Option<Loaded> {
        Some(Loaded {
            file: AudioSource {
                bytes: self.bytes.upgrade()?,
            },
            pcm: self.pcm(),
            rate: self.rate,
            mono: self.mono,
        })
    }

    fn alive(&self) -> bool {
        self.bytes.strong_count() > 0 || self.pcm.as_ref().is_some_and(|p| p.0.strong_count() > 0)
    }
}

/// On a creature: the sound files preloaded for it ([`preload_sounds`]), kept loaded while it lives.
#[derive(Component)]
pub(crate) struct HeldSounds {
    _files: Vec<Loaded>,
}

/// A preload for `owner`: the files it has, and the keys still read in the background.
pub(super) struct Hold {
    owner: Entity,
    files: Vec<Loaded>,
    waiting: Vec<String>,
}

impl Sounds {
    /// Cached files still loaded and their bytes (file + decoded samples).
    #[cfg(feature = "dev")]
    pub fn footprint(&self) -> (usize, usize) {
        self.cache
            .values()
            .flatten()
            .filter_map(WeakLoaded::upgrade)
            .fold((0, 0), |(n, bytes), l| {
                (
                    n + 1,
                    bytes + l.file.bytes.len() + l.pcm.as_ref().map_or(0, |p| p.samples.len() * 4),
                )
            })
    }

    /// Drops every loaded sound, pak fact, pending preload and waiting one-shot (the mounted mod changed).
    pub fn clear(&mut self) {
        self.cache.clear();
        self.recent.clear();
        self.prune_at = 0;
        self.paks.clear();
        self.in_flight.clear();
        self.holds.clear();
        self.preload.clear();
        self.pending.clear();
    }

    /// `Some(file)` while the file at `key` is loaded, `Some(None)` for a file that failed to load, `None` when it
    /// was never read or has gone.
    fn cached(&self, key: &str) -> Option<Option<Loaded>> {
        match self.cache.get(key)? {
            None => Some(None),
            Some(w) => w.upgrade().map(Some),
        }
    }

    /// Records the file read for `key` (`None`: unreadable), reusing a dead entry's key.
    fn insert(&mut self, key: &str, loaded: Option<&Loaded>) {
        let weak = loaded.map(WeakLoaded::of);
        if let Some(slot) = self.cache.get_mut(key) {
            *slot = weak;
            return;
        }
        if self.cache.len() >= self.prune_at {
            self.cache
                .retain(|_, w| w.as_ref().is_none_or(WeakLoaded::alive));
            self.prune_at = (self.cache.len() * 2).max(PRUNE_MIN);
        }
        self.cache.insert(key.to_owned(), weak);
    }

    /// The sound file at `path`, read on first use (a WAV not preloaded streams).
    pub fn file(&mut self, data: &GameData, path: &str) -> Option<Loaded> {
        let key = normalized(path);
        let loaded = match self.cached(&key) {
            Some(hit) => hit,
            None => {
                let loaded = match data.vfs.read(&key) {
                    Ok(bytes) => {
                        let mut l = parse_loaded(&key, bytes);
                        // A voice may still play the decoded samples of a file whose bytes went.
                        l.pcm = self.cache.get(key.as_ref()).and_then(|w| w.as_ref()?.pcm());
                        Some(l)
                    }
                    Err(e) => {
                        warn!("sound {key}: {e:#}");
                        None
                    }
                };
                self.insert(&key, loaded.as_ref());
                loaded
            }
        };
        if let Some(l) = &loaded {
            self.recent.keep(l);
        }
        loaded
    }

    /// The sound file at `path` when it is loaded. Otherwise its read starts in the background and this returns
    /// `Err(true)` while it is on its way, `Err(false)` for a file that failed to load: a one-shot never reads the
    /// archive on the game thread.
    pub fn file_or_fetch(&mut self, data: &GameData, path: &str) -> Result<Loaded, bool> {
        let key = normalized(path);
        match self.cached(&key) {
            Some(Some(l)) => {
                self.recent.keep(&l);
                Ok(l)
            }
            Some(None) => Err(false),
            None => {
                if !self.in_flight.contains(key.as_ref()) {
                    let (key, data) = (key.into_owned(), data.clone());
                    self.in_flight.insert(key.clone());
                    self.preload
                        .push(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                            let loaded = data
                                .vfs
                                .read(&key)
                                .inspect_err(|e| warn!("sound {key}: {e:#}"))
                                .ok()
                                .map(|b| with_pcm(parse_loaded(&key, b)));
                            vec![(key, loaded)]
                        }));
                }
                Err(true)
            }
        }
    }

    /// Preloads the sound files `paths` for `owner` (`SoundPak::PreLoad`): the loaded ones at once, the rest read in
    /// a background task (one read per file however many owners wait for it); [`collect_preload`] hands `owner` the
    /// whole set ([`HeldSounds`]), replacing its older one.
    fn warm<'a>(
        &mut self,
        data: &GameData,
        owner: Entity,
        paths: impl IntoIterator<Item = &'a str>,
    ) {
        let mut keys: Vec<Cow<str>> = paths.into_iter().map(normalized).collect();
        keys.sort_unstable();
        keys.dedup();
        let mut hold = Hold {
            owner,
            files: Vec::new(),
            waiting: Vec::new(),
        };
        let mut todo = Vec::new();
        for key in keys {
            match self.cached(&key) {
                Some(Some(l)) => {
                    hold.files.push(l);
                    continue;
                }
                Some(None) => continue,
                None => {}
            }
            let key = key.into_owned();
            if self.in_flight.insert(key.clone()) {
                todo.push(key.clone());
            }
            hold.waiting.push(key);
        }
        self.holds.retain(|h| h.owner != owner);
        self.holds.push(hold);
        if todo.is_empty() {
            return;
        }
        let data = data.clone();
        self.preload
            .push(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                todo.into_iter()
                    .map(|key| {
                        let loaded = data
                            .vfs
                            .read(&key)
                            .ok()
                            .map(|b| with_pcm(parse_loaded(&key, b)));
                        (key, loaded)
                    })
                    .collect()
            }));
    }
}

/// A 2D sound entity whose file was still being read when it started ([`start_2d`]): it exists, and counts as
/// playing, from that frame, and [`start_awaited`] gives it its voice when the file lands (PORT: a few frames late).
#[derive(Component)]
pub(super) struct AwaitedFile {
    path: String,
    /// The engine frequency it plays at.
    hz: f32,
}

/// Spawns a 2D sound of `class` playing the file at `path` once (`end` when it ends) at the engine frequency `hz`:
/// at once when the file is loaded, else when its background read lands ([`AwaitedFile`]). `None` for a file that
/// cannot be read.
#[allow(clippy::too_many_arguments)]
pub(super) fn start_2d(
    commands: &mut Commands,
    data: &GameData,
    sounds: &mut Sounds,
    path: &str,
    hz: f32,
    class: SoundClass,
    volume: f32,
    end: OnEnd,
) -> Option<Entity> {
    let file = match sounds.file_or_fetch(data, path) {
        Ok(f) => Some(f),
        Err(true) => None,
        Err(false) => return None,
    };
    // The speed of a file still on its way is set when it lands.
    let speed = file.as_ref().map_or(1.0, |f| f.speed(hz));
    let e = commands.spawn(sound(class, volume, end, speed)).id();
    match file {
        Some(f) => attach_voice(commands, e, Voice3d::of(f, false, None)),
        None => {
            commands.entity(e).insert(AwaitedFile {
                path: path.to_owned(),
                hz,
            });
        }
    }
    Some(e)
}

/// Starts the [`AwaitedFile`] sounds whose file landed ([`collect_preload`]); one whose file failed goes.
pub(super) fn start_awaited(
    mut commands: Commands,
    data: Res<GameData>,
    mut sounds: ResMut<Sounds>,
    mut waiting: Query<(Entity, &AwaitedFile, &mut VoiceStart)>,
) {
    for (e, w, mut start) in &mut waiting {
        match sounds.file_or_fetch(&data, &w.path) {
            Ok(f) => {
                start.speed = f.speed(w.hz);
                commands.entity(e).try_remove::<AwaitedFile>();
                attach_voice(&mut commands, e, Voice3d::of(f, false, None));
            }
            Err(true) => {}
            Err(false) => commands.entity(e).try_despawn(),
        }
    }
}

/// Parses a sound file's header into a [`Loaded`] that streams (`SoundManager::Load`: `.ogg` streams, everything
/// else a RIFF WAVE); [`with_pcm`] decodes a small WAV.
fn parse_loaded(key: &str, bytes: Vec<u8>) -> Loaded {
    let rate = (!key.ends_with(".ogg"))
        .then(|| gd_formats::wav::wav_rate(&bytes).unwrap_or(ENGINE_HZ as u32));
    let mono = gd_formats::wav::wav_channels(&bytes) == Some(1);
    Loaded {
        file: AudioSource {
            bytes: Arc::from(bytes),
        },
        pcm: None,
        rate,
        mono,
    }
}

/// A WAV of at most [`BUFFER_BYTES`]: played from memory once decoded.
fn wants_pcm(l: &Loaded) -> bool {
    l.rate.is_some() && l.pcm.is_none() && l.file.bytes.len() <= BUFFER_BYTES
}

/// `l` with its samples decoded when [`wants_pcm`].
fn with_pcm(mut l: Loaded) -> Loaded {
    if wants_pcm(&l) {
        let d = l.file.decoder();
        l.pcm = Some(Pcm {
            channels: d.channels().get(),
            rate: d.sample_rate().get(),
            samples: d.collect(),
        });
    }
    l
}

/// The sound cache clears on `model::ModChanged`. A new session lets the recently played files go.
pub(super) fn reset_sounds(
    mut mod_changed: MessageReader<gd_render::model::ModChanged>,
    mut sounds: ResMut<Sounds>,
    mut fresh: gd_game_core::session::NewSession,
) {
    if fresh.begun() {
        sounds.recent.clear();
    }
    if mod_changed.read().last().is_some() {
        sounds.clear();
    }
}

/// Drains the background preload tasks into the cache and hands each complete preload to its owner.
pub(super) fn collect_preload(mut commands: Commands, mut sounds: ResMut<Sounds>) {
    use bevy::tasks::block_on;
    use bevy::tasks::futures_lite::future;
    if sounds.holds.is_empty() && sounds.preload.is_empty() {
        return;
    }
    let sounds = &mut *sounds;
    let mut landed: HashMap<String, Option<Loaded>> = HashMap::default();
    let mut i = 0;
    while i < sounds.preload.len() {
        if let Some(result) = block_on(future::poll_once(&mut sounds.preload[i])) {
            sounds.preload.swap_remove(i).detach();
            for (key, loaded) in result {
                sounds.in_flight.remove(&key);
                sounds.insert(&key, loaded.as_ref());
                if let Some(l) = &loaded {
                    sounds.recent.keep(l);
                }
                landed.insert(key, loaded);
            }
        } else {
            i += 1;
        }
    }
    let in_flight = &sounds.in_flight;
    sounds.holds.retain_mut(|h| {
        let files = &mut h.files;
        h.waiting.retain(|key| match landed.get(key) {
            Some(l) => {
                files.extend(l.clone());
                false
            }
            // Still read, or dropped by a mod change.
            None => in_flight.contains(key),
        });
        if !h.waiting.is_empty() {
            return true;
        }
        commands.entity(h.owner).try_insert(HeldSounds {
            _files: std::mem::take(&mut h.files),
        });
        false
    });
}

/// A creature's `CharacterSounds` paks warmed when it spawns.
const CHARACTER_PAKS: [&str; 22] = [
    "voxSound",
    "deathSound1",
    "deathSound2",
    "deathSound3",
    "impactSound",
    "attackSound",
    "swipeSound",
    "stunSound",
    "criticalHitSound",
    "specialAttackSound1",
    "specialAttackSound2",
    "specialAttackSound3",
    "specialAttackSound4",
    "lowHealthSound",
    "footGrass",
    "footDirt",
    "footSand",
    "footSnow",
    "footStone",
    "footWood",
    "footWater",
    "footMud",
];

/// The own record's callback sounds (no form override).
const CALLBACK_PAKS: [&str; 8] = [
    "genericSound1",
    "genericSound2",
    "genericSound3",
    "genericSound4",
    "bodyFallSound",
    "voiceSound1",
    "voiceSound2",
    "voiceSound3",
];

/// Warms the sound cache for new creatures and the player's kit (`SoundPak::PreLoad`): a creature's
/// `CharacterSounds` paks, callback sounds, weapons and effective skills when it spawns or its equipment changes.
pub(super) fn preload_sounds(
    data: Res<GameData>,
    mut sounds: ResMut<Sounds>,
    new_chars: Query<Entity, Added<gd_game_core::character::Character>>,
    chars: Query<&gd_game_core::character::Character>,
    holdings: gd_game::combat_sounds::Holdings,
    mut equip: MessageReader<gd_game_core::inventory::EquipmentChanged>,
    loadouts: Query<&gd_game_core::player::Loadout>,
) {
    let mut owners: Vec<Entity> = new_chars.iter().collect();
    owners.extend(equip.read().map(|ev| ev.player));
    if owners.is_empty() {
        return;
    }
    owners.sort_unstable();
    owners.dedup();
    for e in owners {
        let mut paks: Vec<String> = Vec::new();
        if let Ok(c) = chars.get(e) {
            for slot in CHARACTER_PAKS {
                paks.extend(gd_game::char_sounds::sound(&data, c, slot));
            }
            if let Some(rec) = data.db.get(&c.record) {
                paks.extend(
                    CALLBACK_PAKS
                        .iter()
                        .filter_map(|k| rec.str(k).filter(|s| !s.is_empty()).map(str::to_owned)),
                );
            }
        }
        // `Holdings::of` includes the shown weapon enchantment's swipe / hit paks
        // (`WeaponEnchantmentPak::Load`).
        paks.extend(gd_game::combat_sounds::weapon_sound_paks(
            &data,
            &holdings.of(e),
        ));
        if let Ok(loadout) = loadouts.get(e) {
            for (skill, _) in &loadout
                .effective_skills(&data.db, &data.skill_trees())
                .skills
            {
                paks.extend(gd_game::combat_sounds::skill_sound_paks(&data, skill));
            }
        }
        let infos: Vec<_> = paks.iter().filter_map(|p| sounds.pak(&data, p)).collect();
        sounds.warm(
            &data,
            e,
            infos
                .iter()
                .flat_map(|i| i.names.iter().map(String::as_str)),
        );
    }
}
