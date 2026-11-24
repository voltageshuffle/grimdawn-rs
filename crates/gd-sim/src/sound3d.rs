//! What a 3D sound does at the listener's two ears: distance gain and stereo
//! panning (`gd_audio` applies them to every voice, every frame).
//!
//! The engine only hands positions to the sound system; DirectSound does the
//! mixing:
//!
//! - `WorldCamera::SetListenerPosition` (Engine.dll):
//!   the listener's front is the camera's z axis (`Math::LookAtMatrix`: target -
//!   eye) flattened and normalised, its top (0, 1, 0). The ears' position:
//!   `gd_audio::listener::ears`. The camera's x axis (up x z) is the image's
//!   left (`Camera::GetImagePoint`), so screen right = front x up
//!   in the game's right-handed coordinates.
//! - DS8Sound.dll negates z in every buffer position (`DS8SoundBuffer::SetPosition`),
//!   the listener position (`DS8SoundSystem::SetListenerPosition`) and the front / top
//!   (`DS8SoundSystem::SetListenerOrientation`), going to DirectSound's
//!   left-handed space, so left and right are the screen's. All deferred,
//!   committed once a frame (`DS8SoundSystem::Update`): gain and
//!   pan follow the listener for sounds already playing, and the camera's
//!   turning pans them.
//! - `SoundManager::Play3D` sets min distance `fallOff`,
//!   max 100 (`DS8SoundBuffer::SetMinDistance` / `SetMaxDistance`, rolloff factor
//!   and algorithm left default); the sample buffers are made with
//!   `DSBCAPS_CTRL3D | DSBCAPS_MUTE3DATMAXDISTANCE` (`DS8SoundSample::LoadWave`, flags
//!   0x200B2).
//!
//! PORT: the distance and pan laws below are not in the game's binaries.
//! They are `DSOUND_Calc3DBuffer` of Wine's dsound (public source,
//! `dlls/dsound/sound3d.c`; speakers at -90 / +90 deg, `dlls/dsound/dsound.c`),
//! which is what the reference game runs on under Proton (its prefix's
//! dsound.dll is GE-Proton's builtin, with that function's trace strings).
//! Native Windows mixes with its own closed code, not reversed here. Stereo
//! output only (Wine pans over more speakers on a surround device).

/// Distance gain of a 3D sound: full inside `fallOff`, then min / d
/// (`DSOUND_Calc3DBuffer`: rolloff 1, volume in millibels = amplitude
/// min / d), silent past the max distance (`DSBCAPS_MUTE3DATMAXDISTANCE`).
pub fn distance_gain(falloff: f32, d: f32) -> f32 {
    let max = MAX_DISTANCE.max(falloff);
    if d > max {
        return 0.0;
    }
    falloff / d.max(falloff)
}

/// `SoundManager::Play3D`'s max distance (m).
pub const MAX_DISTANCE: f32 = 100.0;

/// [left, right] gains of a sound `to_sound` away from the ears, the listener
/// facing the horizontal unit vector `front` ([x, z]). `DSOUND_Calc3DBuffer`:
/// with a = (angle between the listener's left and the sound) / pi, left =
/// sqrt(1 - a), right = sqrt(a): constant power, only the sideways angle
/// counts (ahead and behind mirror each other, height pulls to the centre),
/// not the distance; on the listener a = 1/2, -3 dB in both ears.
pub fn ear_gains(front: [f32; 2], to_sound: [f32; 3]) -> [f32; 2] {
    let [dx, dy, dz] = to_sound;
    let len = (dx * dx + dy * dy + dz * dz).sqrt();
    let a = if len == 0.0 {
        0.5
    } else {
        // The listener's left (front x top in DirectSound's space) is
        // (front.z, 0, -front.x) in the game's.
        ((front[1] * dx - front[0] * dz) / len)
            .clamp(-1.0, 1.0)
            .acos()
            / std::f32::consts::PI
    };
    [(1.0 - a).sqrt(), a.sqrt()]
}
