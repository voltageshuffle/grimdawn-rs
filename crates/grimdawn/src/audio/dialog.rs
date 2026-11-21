//! Conversation voice-overs ([`dialog_voice`]) and NPCs' greeting / goodbye lines ([`npc_voices`]).
use super::Sounds;
use super::listener::{SoundClass, VOICES};
use super::mixer::{EarGains, OnEnd};
use super::pak::{ENGINE_HZ, PENDING_FRAMES, Positioned, audible_gain, start_voice};
use crate::data::GameData;
use bevy::prelude::*;

/// The conversation voice-over's sound entity (at most one).
#[derive(Component)]
pub(super) struct DialogVoice;

/// The volume of a conversation voice-over: the step's `SoundDescriptor` default,
/// `SoundDescriptor::SoundDescriptor` @0x1801b29f0 (+52 = 0x3F4CCCCD; `ConversationStep::Read` @0x18017a100 changes
/// only the volume set).
pub const VOICE_VOLUME: f32 = 0.8;

/// Conversation voice-overs (rules in `gd_game::conversation`): the voice of each new line shown starts, the previous
/// one stops; closing the window stops it. `ConversationStep::Read` (Game.dll 0x18017a100) loads the step's
/// VO into its own `SoundDescriptor` (a `.wav` sample; an `.ogg` a stream) in the Dialog volume set
/// (`dialogVolume`); `ConversationStep::PlayVO` @0x18017a8f0 plays it with `SoundManager::Play2D` (2D, at no
/// position), `StopVO` @0x18017a920 is `SoundManager::StopPlaying`. A WAV voice plays at the descriptor's 44100 Hz
/// (`pak::playback_speed`): all 166 stock voices are 44.1 kHz WAVs, so they play at their own speed. `Play2D` gates it
/// as any sound: Dialog and master volume at least 1e-4 ([`audible_gain`]), a free voice slot ([`VOICES`]). A voice not
/// loaded yet is read in the background and starts when it lands, a few frames into the line (`cache::start_2d`;
/// PORT: the engine streams it rather than stall the frame).
pub(super) fn dialog_voice(
    mut commands: Commands,
    conv: Res<gd_game::conversation::Conversation>,
    data: Res<GameData>,
    mut sounds: ResMut<Sounds>,
    playing: Query<Entity, With<DialogVoice>>,
    mut line: Local<u64>,
) {
    if conv.line == *line {
        return;
    }
    *line = conv.line;
    for e in &playing {
        debug!("voice stopped");
        commands.entity(e).try_despawn();
    }
    let Some(path) = conv
        .view
        .as_ref()
        .map(|v| v.voice.as_str())
        .filter(|p| !p.is_empty())
    else {
        return;
    };
    let l = &sounds.listener;
    if !audible_gain(l.master, l.gain(SoundClass::Dialog)) || l.voices >= VOICES {
        return;
    }
    let Some(e) = super::cache::start_2d(
        &mut commands,
        &data,
        &mut sounds,
        path,
        ENGINE_HZ,
        SoundClass::Dialog,
        VOICE_VOLUME,
        OnEnd::Despawn,
    ) else {
        return;
    };
    debug!("voice {path}");
    sounds.listener.voices += 1;
    commands.entity(e).insert(DialogVoice);
}

/// The sound entity of an NPC's `dialogPak` line ([`npc_voices`]): the NPC that says it.
#[derive(Component)]
pub(super) struct NpcVoicePlaying(Entity);

/// The fall-off of a `dialogPak` line: the `SoundDescriptor` default, `SoundDescriptor::SoundDescriptor`
/// @0x1801b29f0 (+68 = 5.5; the pak's load sets only the volume set).
const NPC_VOICE_FALLOFF: f32 = 5.5;

/// NPCs' greeting / goodbye lines (rules and pick in `gd_game::trade`, "Voices"): `sub_180265E20` plays the
/// file with `SoundManager::Play3D` at the NPC's coords, in the Dialog volume set with the descriptor's volume
/// ([`VOICE_VOLUME`]), fall-off and 44 100 Hz; `Play3D` gates it as any sound (Dialog and master volume ≥ 1e-4, a
/// free voice slot) and checks no hearing range. A stereo file plays 2D (`pak` module doc). `sub_180266650`:
/// no new line while the NPC's previous one still plays. A line whose file is not loaded yet is read in the background
/// and plays when it lands, as a `SoundPak` one-shot does (`pak::replay_pending`): it counts as the NPC's line while
/// it waits, and is dropped after [`PENDING_FRAMES`] or when the NPC goes.
#[allow(clippy::too_many_arguments)]
pub(super) fn npc_voices(
    mut commands: Commands,
    mut lines: MessageReader<gd_game::trade::NpcVoice>,
    data: Res<GameData>,
    mut sounds: ResMut<Sounds>,
    playing: Query<&NpcVoicePlaying>,
    alive: Query<()>,
    mut pending: Local<Vec<(gd_game::trade::NpcVoice, u8)>>,
    mut fresh: gd_game::session::NewSession,
) {
    if fresh.begun() {
        pending.clear();
    }
    let mut started = Vec::new();
    let waited = std::mem::take(&mut *pending);
    for (line, frames) in waited
        .into_iter()
        .chain(lines.read().map(|l| (l.clone(), 0)))
    {
        // A waited line whose NPC went is dropped.
        if frames > 0 && !alive.contains(line.npc) {
            continue;
        }
        if started.contains(&line.npc) || playing.iter().any(|p| p.0 == line.npc) {
            debug!("npc voice {}: the previous line still plays", line.file);
            continue;
        }
        let l = &sounds.listener;
        if !audible_gain(l.master, l.gain(SoundClass::Dialog)) || l.voices >= VOICES {
            continue;
        }
        let file = match sounds.file_or_fetch(&data, &line.file) {
            Ok(f) => f,
            Err(on_its_way) => {
                if on_its_way && frames < PENDING_FRAMES && alive.contains(line.npc) {
                    started.push(line.npc);
                    pending.push((line, frames + 1));
                }
                continue;
            }
        };
        let place = file.mono.then(|| Positioned {
            target: None,
            at: line.at,
            volume: VOICE_VOLUME,
            falloff: NPC_VOICE_FALLOFF,
            ears: EarGains::default(),
        });
        let speed = file.speed(ENGINE_HZ);
        debug!("npc voice {} at speed {speed:.3}", line.file);
        sounds.listener.voices += 1;
        let e = start_voice(
            &mut commands,
            &sounds.listener,
            file,
            SoundClass::Dialog,
            VOICE_VOLUME,
            OnEnd::Despawn,
            speed,
            place,
        );
        commands.entity(e).insert(NpcVoicePlaying(line.npc));
        started.push(line.npc);
    }
}
