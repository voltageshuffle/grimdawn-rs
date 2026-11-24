//! Sound file headers: what the engine needs of a sound before it plays it.
//!
//! `SoundManager::Load` (Engine.dll): a `.ogg` file is a stream
//! (`SoundDescriptor::StartStream`: played at its own rate, no frequency
//! set); anything else is a RIFF WAVE loaded whole into a sound buffer made
//! from the file's own `fmt ` chunk (DS8Sound.dll virtual
//! `DS8SoundSample::LoadWave`: `RIFFGetChunk` 'RIFF' / 'WAVE' / 'fmt ' / 'data',
//! `nSamplesPerSec` copied), whose playback frequency is then set absolute
//! (`DS8SoundSample::SetFrequency` → `IDirectSoundBuffer::SetFrequency`).
//!
//! Stock data (2026-11-03 scan of the four `Sound.arc`s): 7 191 WAVs at
//! 44.1 kHz, 1 414 at 48 kHz (quest dialog, monster vocalisations, Ashes /
//! Forgotten Gods ambience), 62 at 96 kHz (Ugdenbog / Darkwood ambience), 2 at
//! 24 kHz, 6 at 22.05 kHz, 1 at 11.025 kHz; 134 OGGs at 44.1 kHz, 32 at 48 kHz.

/// The sample rate of a RIFF WAVE (`fmt ` chunk `nSamplesPerSec`); `None`
/// for anything else.
pub fn wav_rate(bytes: &[u8]) -> Option<u32> {
    fmt_chunk(bytes)?
        .get(4..8)
        .map(|r| u32::from_le_bytes(r.try_into().unwrap()))
}

/// The channel count of a RIFF WAVE (`fmt ` chunk `nChannels`); `None` for
/// anything else. A multi-channel file cannot be a 3D buffer
/// (`gd_audio::cache::Loaded::mono`). Stock pak entries: 5 054 mono, 901 stereo.
pub fn wav_channels(bytes: &[u8]) -> Option<u16> {
    fmt_chunk(bytes)?
        .get(2..4)
        .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
}

/// The body of the `fmt ` chunk.
fn fmt_chunk(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().ok()?) as usize;
        if &bytes[i..i + 4] == b"fmt " {
            return bytes.get(i + 8..);
        }
        i = i.checked_add(size.checked_add(8 + (size & 1))?)?;
    }
    None
}
