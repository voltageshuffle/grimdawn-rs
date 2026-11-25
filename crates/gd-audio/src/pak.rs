//! `SoundPak` one-shots: gameplay requests, story sounds, periodic decoration sounds and the characters' animation
//! callbacks, all started by [`play_pak`]. A pak played without a position (or entity) is an interface sound and
//! plays through the game's pause ([`PlaysWhilePaused`]); a placed one is held by it.
//!
//! 3D sounds: [`Positioned`] holds every mono sound the engine would make a 3D buffer; its voice is mixed to the two
//! ears with the gains of [`gd_sim::sound3d`], kept current every frame ([`positioned_sounds`], `emitters::emitters`).
//! DS8Sound.dll `DS8SoundSample::LoadWave`: a sample's buffer is made with `DSBCAPS_CTRL3D`, which DirectSound
//! refuses for a stereo WAV; it is then made again as a plain buffer and marked not 3D (`SoundBuffer.is3d`), so the
//! position and min / max distance calls do nothing: a stereo file plays centred at its full volume wherever it is
//! (`Loaded::mono`; 901 of the 5 955 stock pak entries, many monster vocalisations). The checks of `SoundPak::Play3D`
//! still apply to it.
use super::Sounds;
use super::cache::Loaded;
use super::listener::{Listener, SoundClass, VOICES, sound};
use super::mixer::{EarGains, OnEnd, PlaysWhilePaused, Voice3d, attach_voice};
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::data::GameData;
use std::sync::Arc;

/// The frequency every engine sound buffer is set to unless pitched: `SoundDescriptor::SoundDescriptor`
/// (`SoundDescriptor.frequency` = 44100).
pub const ENGINE_HZ: f32 = 44100.0;

/// Playback speed of a sound file at the engine frequency `freq` (Hz). `SoundManager::Play3D` /
/// `Play2D` set a sound buffer's frequency to the descriptor's (`ENGINE_HZ`, or a [`pitch_hz`]) through
/// `IDirectSoundBuffer::SetFrequency`, absolute Hz, on a buffer made in the WAV's own format (`gd_formats::wav`): a
/// 48 kHz WAV plays at 44100 / 48000 = 0.919 speed, a 96 kHz one at half speed. `.ogg` streams (`rate` None,
/// `StartStream`) get no frequency: their own speed.
pub fn playback_speed(freq: f32, rate: Option<u32>) -> f32 {
    rate.filter(|r| *r > 0)
        .map_or(1.0, |r| (freq / r as f32).max(0.05))
}

/// A `SoundPak` record as `SoundPak::Load` reads it, kept in [`Sounds`] by name until the mod changes, so
/// a play does no record lookups.
pub struct PakInfo {
    /// `volumeSet` ([`volume_set`]).
    pub(super) class: SoundClass,
    /// `soundName1..N` (normalized file keys) and their `weightN` (default 25).
    pub names: Vec<String>,
    weights: Vec<u32>,
    pub volume: f32,
    /// `fallOff` as written (a one-shot defaults to 0, a loop to 10).
    pub(super) falloff: Option<f32>,
    force2d: bool,
    at_camera_y: bool,
    pitch_up: i32,
    pitch_step: i32,
}

impl PakInfo {
    fn read(rec: &Record) -> Self {
        let names: Vec<String> = numbered(rec, "soundName")
            .into_iter()
            .map(|n| gd_formats::vfs::normalize(&n))
            .collect();
        let weights = (1..=names.len())
            .map(|i| rec.i32(&format!("weight{i}")).unwrap_or(25).max(0) as u32)
            .collect();
        PakInfo {
            class: volume_set(rec.str("volumeSet")),
            names,
            weights,
            volume: rec.f32("volume").unwrap_or(1.0),
            falloff: rec.f32("fallOff"),
            force2d: rec.bool("force2d"),
            at_camera_y: rec.bool("spawnAtCameraY"),
            pitch_up: rec.i32("pitchUp").unwrap_or(0),
            pitch_step: rec.i32("pitchStep").unwrap_or(0),
        }
    }

    /// The variant `PickSound` takes for `roll` (None without variants).
    pub(super) fn pick(&self, roll: u32) -> Option<&str> {
        self.names
            .get(pick_sound(&self.weights, roll))
            .map(String::as_str)
    }

    /// [`pitch_hz`] of the pak.
    pub(super) fn pitch(&self, positional: bool, roll: u32) -> f32 {
        pitch_hz(self.pitch_up, self.pitch_step, positional, roll)
    }
}

impl Sounds {
    /// The `SoundPak` record `name`, read on first use.
    pub fn pak(&mut self, data: &GameData, name: &str) -> Option<Arc<PakInfo>> {
        if let Some(p) = self.paks.get(name) {
            return p.clone();
        }
        let info = data.db.get(name).map(|r| Arc::new(PakInfo::read(r)));
        self.paks.insert(name.to_owned(), info.clone());
        info
    }
}

/// Plays one variant of a `SoundPak` record (`SoundPak::Play3D` / `Play2D`, Engine.dll). `at`: the sound's world
/// position, heard from the listener (`listener::ears`); `None` (or no listener yet) plays it 2D. The single place
/// gameplay one-shots start, so every rule that limits what is heard is here:
///
/// - `SoundManager::Play3D` / `Play2D`: not started while its volume set's option
///   gain or the master volume is below 1e-4 ([`audible_gain`]); not started while all [`VOICES`] slots play
///   (`GetEmptyCurrentPlayingIndex` returns -1: no stealing, no priority); slots free when a sound ends.
/// - `SoundPak::Play3D`: a `force2d` pak positioned in the world plays 2D only when the spot is
///   inside the view frustum widened by 20 m (`IsValidPositionInFrustum`); others only if
///   [`could_possibly_hear`] (`CouldPossiblyHear`, always on).
/// - `SoundPak::Load`: `volumeSet` picks the volume option ([`volume_set`]); `weightN` default
///   25, `PickSound` ([`pick_sound`]).
/// - Pitch: [`pitch_hz`], played at the file's [`playback_speed`].
///
/// Footstep / vox / combat requests that fail these are dropped here, at the same point the engine drops them (the
/// callbacks themselves are not gated: `Actor::UpdateSelf` runs `HandleAnimationCallbacks` for every updated actor).
///
/// `SoundPak::GetPlayLocation`: after the hearing check a `spawnAtCameraY` pak (level up, low
/// health warnings) plays at the ears' height. A mono file heard in 3D stays where it started ([`Positioned`]).
pub fn play_pak(
    commands: &mut Commands,
    data: &GameData,
    sounds: &mut Sounds,
    pak: &str,
    at: Option<Vec3>,
) -> Option<Entity> {
    start_pak(commands, data, sounds, pak, at, None)
}

/// [`play_pak`], the sound moving with the entity `on` ([`Positioned`]).
fn start_pak(
    commands: &mut Commands,
    data: &GameData,
    sounds: &mut Sounds,
    pak: &str,
    at: Option<Vec3>,
    on: Option<Entity>,
) -> Option<Entity> {
    let info = sounds.pak(data, pak)?;
    let name = info.pick(fastrand::u32(..))?;
    start_named(commands, data, sounds, pak, &info, name, at, on, 0)
}

/// How many frames a one-shot waits for its file's background read ([`replay_pending`], `dialog::npc_voices`) before
/// it is dropped.
pub(super) const PENDING_FRAMES: u8 = 20;

/// A one-shot whose picked file was still being read ([`start_named`]).
pub(super) struct PendingPlay {
    pak: String,
    name: String,
    at: Option<Vec3>,
    on: Option<Entity>,
    frames: u8,
}

/// [`start_pak`] once the pak's file `name` is picked: the hearing rules, then the voice; a file not loaded yet is
/// read in the background and the play waits for it ([`replay_pending`]).
#[allow(clippy::too_many_arguments)]
fn start_named(
    commands: &mut Commands,
    data: &GameData,
    sounds: &mut Sounds,
    pak: &str,
    info: &PakInfo,
    name: &str,
    at: Option<Vec3>,
    on: Option<Entity>,
    frames: u8,
) -> Option<Entity> {
    if !audible_gain(sounds.listener.master, sounds.listener.gain(info.class))
        || sounds.listener.voices >= VOICES
    {
        return None;
    }
    let heard = at.zip(sounds.listener.ears);
    let positional = heard.is_some() && !info.force2d;
    // Culled before the file is fetched: a sound out of view / earshot is never read.
    let mut heard_at = None;
    if let Some((pos, listener)) = heard {
        if info.force2d {
            if sounds
                .listener
                .view
                .as_ref()
                .is_some_and(|f| !in_widened_view(f, pos))
            {
                return None;
            }
        } else {
            let falloff = info.falloff.unwrap_or(0.0);
            if !could_possibly_hear(falloff, pos.distance(listener)) {
                return None;
            }
            let pos = if info.at_camera_y {
                Vec3::new(pos.x, listener.y, pos.z)
            } else {
                pos
            };
            heard_at = Some((pos, falloff));
        }
    }
    let file = match sounds.file_or_fetch(data, name) {
        Ok(file) => file,
        Err(on_its_way) => {
            if on_its_way && frames < PENDING_FRAMES {
                sounds.pending.push(PendingPlay {
                    pak: pak.to_owned(),
                    name: name.to_owned(),
                    at,
                    on,
                    frames: frames + 1,
                });
            }
            return None;
        }
    };
    let place = heard_at
        .filter(|_| file.mono)
        .map(|(pos, falloff)| Positioned {
            target: on,
            at: pos,
            falloff: falloff.max(0.1),
            ears: EarGains::default(),
        });
    let speed = file.speed(info.pitch(positional, fastrand::u32(..)));
    debug!("start {pak} at speed {speed:.3}");
    sounds.listener.voices += 1;
    let e = start_voice(
        commands,
        &sounds.listener,
        file,
        info.class,
        info.volume,
        OnEnd::Despawn,
        speed,
        place,
    );
    // A sound with no place in the world is the interface's (a button, an item moved in a window, a spoken refusal:
    // the engine's `EnablePauseHack` ones): it plays while the game is paused, every other pak sound is held.
    if at.is_none() && on.is_none() {
        commands.entity(e).insert(PlaysWhilePaused);
    }
    Some(e)
}

/// Starts the one-shots that waited for their file ([`start_named`]) once `cache::collect_preload` has it: a few
/// frames late (PORT: the engine streams a sound it did not preload rather than stall the frame on the read).
pub(super) fn replay_pending(
    mut commands: Commands,
    data: Res<GameData>,
    mut sounds: ResMut<Sounds>,
) {
    if sounds.pending.is_empty() {
        return;
    }
    for p in std::mem::take(&mut sounds.pending) {
        if let Some(info) = sounds.pak(&data, &p.pak) {
            start_named(
                &mut commands,
                &data,
                &mut sounds,
                &p.pak,
                &info,
                &p.name,
                p.at,
                p.on,
                p.frames,
            );
        }
    }
}

/// Starts a file, as a 3D voice at `place` or else plain. `volume` is the sound's own
/// ([`BaseVolume`](super::listener::BaseVolume)); a 3D voice's distance gain and pan are its [`EarGains`]
/// ([`Listener::gains`]).
#[allow(clippy::too_many_arguments)]
pub(super) fn start_voice(
    commands: &mut Commands,
    listener: &Listener,
    file: Loaded,
    class: SoundClass,
    volume: f32,
    end: OnEnd,
    speed: f32,
    place: Option<Positioned>,
) -> Entity {
    let Some(place) = place else {
        let e = commands.spawn(sound(class, volume, end, speed)).id();
        attach_voice(commands, e, Voice3d::of(file, false, None));
        return e;
    };
    place.ears.set(listener.gains(place.at, place.falloff));
    let voice = Voice3d::of(file, false, Some(place.ears.clone()));
    let e = commands
        .spawn((place, sound(class, volume, end, speed)))
        .id();
    attach_voice(commands, e, voice);
    e
}

/// `SoundPak::CouldPossiblyHear`: a 3D sound starts only when the listener is closer than 8 × `fallOff`
/// (squared distances compared).
pub fn could_possibly_hear(falloff: f32, d: f32) -> bool {
    d * d < (8.0 * falloff) * (8.0 * falloff)
}

/// `SoundManager::Play3D` / `Play2D`: the sound's volume set and the master volume both at least 1e-4.
pub fn audible_gain(master: f32, class: f32) -> bool {
    master >= 0.0001 && class >= 0.0001
}

/// `SoundPak::Load`: the `volumeSet` string's class (exact names, anything else Effects).
pub fn volume_set(s: Option<&str>) -> SoundClass {
    match s {
        Some("Ambient") => SoundClass::Ambient,
        Some("Music") => SoundClass::Music,
        Some("Dialog") => SoundClass::Dialog,
        _ => SoundClass::Effects,
    }
}

/// `SoundPak::PickSound`: `roll % total`, the first variant whose cumulative weight reaches it; 0 when
/// the weights sum to 0.
pub fn pick_sound(weights: &[u32], roll: u32) -> usize {
    let total: u32 = weights.iter().sum();
    if total == 0 {
        return 0;
    }
    let r = roll % total;
    let mut acc = 0;
    for (i, w) in weights.iter().enumerate() {
        acc += w;
        if r <= acc {
            return i;
        }
    }
    0
}

/// The frequency (Hz) a pak plays a variant at. `SoundPak::Load` reads `pitchUp` into both pitch
/// fields (`pitchDown` is never read) and `pitchStep` (0 → 2000); `SoundPak::Play3D` plays at 44100 +
/// (`rand() % (2 up + 1)`) × step Hz (only upwards), `Play2D` at 44100 + (`rand() % (2 up + 1)` − up) ×
/// step; the file's speed then follows [`playback_speed`].
pub fn pitch_hz(up: i32, step: i32, positional: bool, roll: u32) -> f32 {
    let up = up.max(0);
    let step = if step == 0 { 2000 } else { step };
    let k = (roll % (2 * up as u32 + 1)) as i32 - if positional { 0 } else { up };
    (44100 + k * step) as f32
}

/// `IsValidPositionInFrustum`: inside every plane of the view frustum moved out by 20 m.
fn in_widened_view(f: &bevy::camera::primitives::Frustum, p: Vec3) -> bool {
    f.half_spaces
        .iter()
        .all(|h| h.normal_d().dot(p.extend(1.0)) + 20.0 >= 0.0)
}

/// `key1..keyN` string values in order (stops at the first gap after 1).
pub(super) fn numbered(rec: &Record, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    for i in 1..=64 {
        match rec.str(&format!("{key}{i}")) {
            Some(s) => out.push(s.to_owned()),
            None if i > 1 && out.len() < i - 1 => break,
            None => {}
        }
    }
    out
}

/// A mono sound heard in 3D. It has a fixed position, its gain and pan following the listener
/// (`gd_sim::sound3d` module doc), unless tracked (`SoundPak::Track` → `SoundManager::RegisterMovingObject`):
/// `SoundManager::Update` moves it to its object every frame and drops the object from the
/// list once it is gone (the sound plays on where it was).
#[derive(Component)]
pub struct Positioned {
    pub(super) target: Option<Entity>,
    pub(super) at: Vec3,
    pub(super) falloff: f32,
    pub(super) ears: EarGains,
}

/// [`Positioned`] sounds' 3D gains (distance and pan, [`Listener::gains`]) for this frame's listener; their own volume
/// ([`BaseVolume`](super::listener::BaseVolume)) stays as it started.
pub(super) fn positioned_sounds(
    mut q: Query<&mut Positioned>,
    at: Query<&GlobalTransform>,
    sounds: Res<Sounds>,
) {
    if sounds.listener.ears.is_none() {
        return;
    }
    for mut p in &mut q {
        match p.target.map(|t| at.get(t)) {
            Some(Ok(g)) => p.at = g.translation(),
            Some(Err(_)) => p.target = None,
            None => {}
        }
        p.ears.set(sounds.listener.gains(p.at, p.falloff));
    }
}

/// Gameplay's sound requests (`gd_game_core::visuals::PlaySound`), heard from the ears; tracked ones (`PlaySoundOn`)
/// follow their entity ([`Positioned`]).
pub(super) fn play_requests(
    mut commands: Commands,
    mut events: MessageReader<gd_game_core::visuals::PlaySound>,
    mut tracked: MessageReader<gd_game_core::visuals::PlaySoundOn>,
    data: Res<GameData>,
    mut sounds: ResMut<Sounds>,
) {
    for ev in events.read() {
        debug!("play {}", ev.pak);
        start_pak(&mut commands, &data, &mut sounds, &ev.pak, ev.at, None);
    }
    for ev in tracked.read() {
        debug!("play {} on {}", ev.pak, ev.on);
        start_pak(
            &mut commands,
            &data,
            &mut sounds,
            &ev.pak,
            Some(ev.at),
            Some(ev.on),
        );
    }
}

/// Story `PlaySound` (a sound pak, at a script object or on the player).
pub(super) fn story_sounds(
    mut commands: Commands,
    mut events: MessageReader<gd_game_core::story::StoryEffect>,
    data: Res<GameData>,
    mut sounds: ResMut<Sounds>,
    actors: Query<(&gd_game_core::story::StoryActor, &GlobalTransform)>,
) {
    for ev in events.read() {
        if let gd_story::story::Effect::PlaySound { record, at } = &ev.0 {
            let pos = at.and_then(|id| {
                actors
                    .iter()
                    .find(|(a, _)| a.0 == id)
                    .map(|(_, t)| t.translation())
            });
            start_pak(&mut commands, &data, &mut sounds, record, pos, None);
        }
    }
}

pub(super) fn periodic_sounds(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut sounds: ResMut<Sounds>,
    mut q: Query<(&GlobalTransform, &mut gd_game_core::world::PeriodicSound)>,
    alive: Query<()>,
) {
    for (gt, mut p) in &mut q {
        p.left -= time.delta_secs();
        if p.left > 0.0 {
            continue;
        }
        p.arm();
        if p.last.is_some_and(|e| alive.contains(e)) {
            continue;
        }
        let last = start_pak(
            &mut commands,
            &data,
            &mut sounds,
            &p.pak,
            Some(gt.translation()),
            None,
        );
        p.last = last;
    }
}

/// Footprints are placed this close to the player at most (PORT: the engine places every character's, into the
/// region's decal set).
const FOOTPRINT_RANGE: f32 = 40.0;

/// The sounds and footprints of characters' animation callbacks, picked by `gd_game::char_sounds` (the form's
/// overrides read from the `Character`): played at the character, footprints placed like a particle's decal
/// (`fx::DropDecal`: `World::PlaceDecal`, the 256-decal ring, only with `detailObjects`). The paper doll makes none.
/// Swipe and hit sounds come from gameplay (`gd_game::combat_sounds`, as `PlaySound`).
#[allow(clippy::too_many_arguments)]
pub(super) fn character_callbacks(
    mut commands: Commands,
    data: Res<GameData>,
    mut callbacks: MessageReader<gd_game_core::character::AnimCallback>,
    chars: Query<
        (&gd_game_core::character::Character, &GlobalTransform),
        Without<gd_render::character::DollBody>,
    >,
    players: Query<&GlobalTransform, With<gd_game_core::player::Player>>,
    coll: Res<gd_game::collision::WorldCollision>,
    mut bodies: ResMut<gd_game_core::creature::BodyCache>,
    mut out: MessageWriter<gd_game_core::visuals::PlaySound>,
    mut tracked: MessageWriter<gd_game_core::visuals::PlaySoundOn>,
) {
    use gd_game::char_sounds::{self, Surface};
    let player = players.iter().next().map(|g| g.translation());
    for cb in callbacks.read() {
        let Ok((c, g)) = chars.get(cb.entity) else {
            continue;
        };
        let at = g.translation();
        let surface = char_sounds::is_footstep(&cb.name)
            .then(|| {
                let code = coll.surface_below(at);
                let s = Surface::footstep(code);
                if s.is_none() {
                    debug!(
                        "{}: {} on surface {code}: silent at {at}",
                        cb.entity, cb.name
                    );
                }
                s
            })
            .flatten();
        let heard = char_sounds::callback(&data, c, &cb.name, surface, char_sounds::roll);
        if let Some(pak) = heard.sound {
            debug!("{}: {} on {surface:?} at {at} -> {pak}", cb.entity, cb.name);
            if heard.tracked {
                tracked.write(gd_game_core::visuals::PlaySoundOn {
                    pak,
                    on: cb.entity,
                    at,
                });
            } else {
                out.write(gd_game_core::visuals::PlaySound { pak, at: Some(at) });
            }
        }
        let Some((decal, left)) = heard.footprint else {
            continue;
        };
        if player.is_none_or(|p| p.distance(at) > FOOTPRINT_RANGE) {
            continue;
        }
        // `GetFootCoords`: the toe bone, else the character's own coords.
        let foot = bodies.toe(&c.body, left).and_then(|i| {
            c.posed()
                .get(i)
                .map(|m| g.transform_point(Vec3::from(m.origin)))
        });
        let (_, rot, _) = g.to_scale_rotation_translation();
        debug!("{}: footprint {decal}", cb.entity);
        commands.spawn((
            gd_render::fx::DropDecal(decal.into()),
            Transform::from_translation(foot.unwrap_or(at)).with_rotation(rot),
        ));
    }
}
