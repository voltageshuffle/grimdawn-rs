//! Looping 3D sounds: placed emitters (a `SoundObject`'s, a monster's or a decoration's `ambientSound`, a riftgate's
//! state sound) and gameplay's [`SoundLoop`](gd_game_core::visuals::SoundLoop)s, started and stopped by [`loop_rule`].
use super::Sounds;
use super::listener::{SoundClass, VOICES};
use super::mixer::{EarGains, OnEnd, Voice, Voice3d, VoiceStart, attach_voice};
use super::pak::{audible_gain, could_possibly_hear, volume_set};
use bevy::prelude::*;
use gd_game_core::data::GameData;
use gd_sim::sound3d::distance_gain;

/// A placed looping sound: attenuated by [`distance_gain`], panned, and started / stopped by [`loop_rule`]. Spawned
/// when its level loads, gone with it. A stereo file's loop (no 3D buffer, `pak` module doc) is not cut by the d²
/// limit of [`loop_rule`] (`SoundManager::Update` checks the buffer's 3D flag `SoundBuffer.is3d`); `spawnAtCameraY`
/// is not applied to loops (no stock looping pak has it).
#[derive(Component)]
pub struct Emitter {
    pak: String,
    volume: f32,
    falloff: f32,
    state: LoopState,
    /// The playing mono file's gains (none while silent or a stereo file).
    ears: Option<EarGains>,
    /// Its volume set: the pak's `volumeSet` (`SoundPak::Load`, as `pak::play_pak`).
    class: SoundClass,
    /// Stop for good, fading over these seconds ([`skill_loops`]).
    stop: Option<f32>,
    /// First frame after spawn: skipped, the `GlobalTransform` is not propagated yet (the identity would be heard at
    /// the world origin).
    fresh: bool,
}

/// Where an [`Emitter`]'s sound is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LoopState {
    Off,
    Playing,
    /// `FadeLastPlayedSound`: seconds left of `.2`, the base volume it fades from.
    Fading(f32, f32, f32),
}

pub fn spawn_emitter(
    commands: &mut Commands,
    data: &GameData,
    record: &str,
    at: Vec3,
) -> Option<Entity> {
    let rec = data.db.get(record)?;
    spawn_loop(
        commands,
        data,
        rec.str("ambientSound").filter(|s| !s.is_empty())?,
        at,
    )
}

/// An [`Emitter`] looping the sound pak `pak` at `at` (e.g. a riftgate's state sound, `riftgates::gate_effects`).
pub fn spawn_loop(commands: &mut Commands, data: &GameData, pak: &str, at: Vec3) -> Option<Entity> {
    let p = data.db.get(pak)?;
    Some(
        commands
            .spawn((
                Transform::from_translation(at),
                Emitter {
                    pak: pak.to_owned(),
                    volume: p.f32("volume").unwrap_or(1.0),
                    falloff: p.f32("fallOff").unwrap_or(10.0).max(0.1),
                    state: LoopState::Off,
                    ears: None,
                    class: volume_set(p.str("volumeSet")),
                    stop: None,
                    fresh: true,
                },
                Name::new("sound emitter"),
            ))
            .id(),
    )
}

/// The [`Emitter`] of a gameplay [`SoundLoop`](gd_game_core::visuals::SoundLoop).
#[derive(Component)]
pub(super) struct LoopOf {
    source: Entity,
    on: Option<Entity>,
    fade: f32,
}

/// Gameplay's looping sounds ([`SoundLoop`](gd_game_core::visuals::SoundLoop)): an [`Emitter`] of its own, so the fade
/// outlives the source, kept at the entity it tracks (`SoundPak::Track`: where it was once that is gone). When the
/// source goes it fades over the source's time (`FadeLastPlayedSound`) and is removed; one never started (out of
/// hearing) is removed at once.
pub(super) fn skill_loops(
    mut commands: Commands,
    data: Res<GameData>,
    added: Query<
        (Entity, &gd_game_core::visuals::SoundLoop),
        Added<gd_game_core::visuals::SoundLoop>,
    >,
    sources: Query<(), With<gd_game_core::visuals::SoundLoop>>,
    at: Query<&GlobalTransform, Without<LoopOf>>,
    mut q: Query<(Entity, &mut LoopOf, &mut Transform, &mut Emitter)>,
) {
    for (source, l) in &added {
        let Some(e) = spawn_loop(
            &mut commands,
            &data,
            &l.pak,
            l.on.and_then(|o| at.get(o).ok())
                .map_or(l.at, |g| g.translation()),
        ) else {
            continue;
        };
        commands.entity(e).insert(LoopOf {
            source,
            on: l.on,
            fade: l.fade,
        });
    }
    for (e, mut l, mut tf, mut em) in &mut q {
        if let Some(on) = l.on {
            match at.get(on) {
                Ok(t) => tf.translation = t.translation(),
                Err(_) => l.on = None,
            }
        }
        if sources.contains(l.source) {
            continue;
        }
        em.stop = Some(l.fade.max(0.001));
        if em.state == LoopState::Off {
            commands.entity(e).try_despawn();
        }
    }
}

/// What a looping 3D sound does this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopStep {
    /// Not playing and not started.
    Off,
    Start,
    Play,
    /// Fade out over 500 ms, then stop.
    Fade,
    /// Stopped now.
    Stop,
}

/// The engine's looping 3D sounds `d` metres from the ears. `SoundAmbient::Update` (Game.dll;
/// `SoundObject`, `Monster` and `Decoration` `ambientSound`, every update; Engine.dll's `SoundEntity::UpdateSelf`
/// alike): a pak not playing is played again (`Play3D` with loop, so only while [`could_possibly_hear`]);
/// one playing farther than 8 × `fallOff` fades out over 500 ms (`FadeLastPlayedSound` → `SoundManager::FadeSound`,
/// which also clears the instance's distance check). `SoundManager::Update` (with
/// `EnableDistanceCheck`, on from the constructor, never switched off): every looping 3D instance with its distance
/// check farther than d² = 5000 (70.7 m; fade value (d² − 5000) × 0.0004 ≥ 1e-4) is stopped at once
/// ([`LOOP_STOP_D2`]), so a loop with 8 × `fallOff` beyond 70.7 m is silent there.
///
/// `three_d`: the d² cut is skipped for a stereo file (no 3D buffer: `SoundManager::Update` tests the buffer's
/// `SoundBuffer.is3d` flag first); the hearing check and the 8 × `fallOff` fade are `Play3D`'s / `SoundAmbient`'s
/// and apply to both.
pub fn loop_rule(playing: bool, three_d: bool, falloff: f32, d: f32) -> LoopStep {
    let cut = three_d && d * d > LOOP_STOP_D2 && (d * d - LOOP_STOP_D2) * 0.0004 >= 0.0001;
    match playing {
        false if could_possibly_hear(falloff, d) && !cut => LoopStep::Start,
        false => LoopStep::Off,
        true if cut => LoopStep::Stop,
        true if d * d > (8.0 * falloff) * (8.0 * falloff) => LoopStep::Fade,
        true => LoopStep::Play,
    }
}

/// `SoundManager::Update`'s squared distance limit of looping 3D sounds.
pub const LOOP_STOP_D2: f32 = 5000.0;

/// `FadeLastPlayedSound`'s time (s).
const LOOP_FADE: f32 = 0.5;

/// Starts, fades and stops the [`Emitter`]s by [`loop_rule`] and sets their volume. A start takes a voice like
/// `pak::play_pak` (none free, or a muted volume set: not started, tried again next frame) and picks a variant
/// (`PickSound`) each time; a stop removes the [`Voice`], freeing the slot. A skill loop ([`LoopOf`]) is heard at its
/// `Transform`, which [`skill_loops`] keeps at its source this frame (the `GlobalTransform` lags a propagation);
/// placed loops (some parented, e.g. a riftgate's) at their `GlobalTransform`.
pub(super) fn emitters(
    mut commands: Commands,
    data: Res<GameData>,
    time: Res<Time>,
    mut sounds: ResMut<Sounds>,
    mut q: Query<(
        Entity,
        &GlobalTransform,
        &Transform,
        &mut Emitter,
        Option<&LoopOf>,
        Option<&Voice>,
    )>,
) {
    let Some(listener) = sounds.listener.ears else {
        return;
    };
    let stop = |commands: &mut Commands, e: Entity| {
        commands.entity(e).try_remove::<(Voice, VoiceStart)>();
    };
    for (e, gt, tf, mut em, lof, voice) in &mut q {
        if em.fresh {
            em.fresh = false;
            continue;
        }
        let at = match lof {
            Some(_) => tf.translation,
            None => gt.translation(),
        };
        let class = em.class;
        let d = at.distance(listener);
        let base = em.volume
            * em.ears
                .as_ref()
                .map_or(1.0, |_| distance_gain(em.falloff, d));
        if let Some(ears) = &em.ears {
            ears.set(sounds.listener.gains(at));
        }
        match em.state {
            LoopState::Fading(left, from, total) => {
                let left = left - time.delta_secs().min(0.1);
                if left <= 0.0 {
                    debug!("loop {} faded out", em.pak);
                    stop(&mut commands, e);
                    em.state = LoopState::Off;
                } else {
                    em.state = LoopState::Fading(left, from, total);
                    if let Some(voice) = voice {
                        voice.set_volume(from * left / total * sounds.listener.gain_final(class));
                    }
                }
                continue;
            }
            LoopState::Off => {
                if em.stop.is_some() || loop_rule(false, false, em.falloff, d) != LoopStep::Start {
                    continue;
                }
                let l = &sounds.listener;
                if !audible_gain(l.master, l.gain(class)) || l.voices >= VOICES {
                    continue;
                }
                let Some(info) = sounds.pak(&data, &em.pak) else {
                    continue;
                };
                // A file not loaded yet is read in the background: the loop starts on a later frame.
                let Some(file) = info
                    .pick(fastrand::u32(..))
                    .and_then(|n| sounds.file_or_fetch(&data, n).ok())
                else {
                    continue;
                };
                // A 3D buffer is not started where `SoundManager::Update` would cut it again.
                if file.mono && loop_rule(false, true, em.falloff, d) != LoopStep::Start {
                    continue;
                }
                let speed = file.speed(info.pitch(true, fastrand::u32(..)));
                debug!("loop {} starts {d:.1} m away at speed {speed:.3}", em.pak);
                sounds.listener.voices += 1;
                let ears = file.mono.then(|| {
                    let ears = EarGains::default();
                    ears.set(sounds.listener.gains(at));
                    ears
                });
                em.ears = ears.clone();
                commands.entity(e).try_insert((
                    class,
                    VoiceStart {
                        end: OnEnd::Keep,
                        speed,
                    },
                ));
                attach_voice(&mut commands, e, Voice3d::of(file, true, ears));
                em.state = LoopState::Playing;
            }
            LoopState::Playing if em.stop.is_some() => {
                let fade = em.stop.unwrap_or(LOOP_FADE);
                em.state = LoopState::Fading(fade, base, fade);
            }
            LoopState::Playing => match loop_rule(true, em.ears.is_some(), em.falloff, d) {
                LoopStep::Stop => {
                    debug!("loop {} stopped {d:.1} m away", em.pak);
                    stop(&mut commands, e);
                    em.state = LoopState::Off;
                }
                LoopStep::Fade => em.state = LoopState::Fading(LOOP_FADE, base, LOOP_FADE),
                _ => {
                    if let Some(voice) = voice {
                        voice.set_volume(base * sounds.listener.gain_final(class));
                    }
                }
            },
        }
    }
}
