//! The listener ([`Listener`]: ears, view, voice count and option volumes) and the sound classes.
use super::Sounds;
use super::mixer::{OnEnd, Voice, VoiceStart};
use bevy::prelude::*;
use gd_game_core::options::GameOptions;
use gd_render::camera::CameraFocus;
use gd_sim::sound3d::{distance_gain, ear_gains};

/// `SoundManager` has 128 playing slots (`GetEmptyCurrentPlayingIndex`); a stream takes one too (`StartStream`),
/// so every playing sound entity counts, the music included.
pub const VOICES: usize = 128;

/// Refreshed by [`listen`] before the frame's sounds start; the single source of the option volumes.
#[derive(Default)]
pub struct Listener {
    /// Sounds playing (or started this frame) that take a voice slot.
    pub voices: usize,
    pub master: f32,
    /// Indexed by [`SoundClass`].
    pub class: [f32; 4],
    pub view: Option<bevy::camera::primitives::Frustum>,
    /// Where sounds are heard from ([`ears`]).
    pub ears: Option<Vec3>,
    pub front: [f32; 2],
    /// False after a reset: read the options again.
    volumes_read: bool,
}

impl Listener {
    pub(super) fn gain(&self, c: SoundClass) -> f32 {
        self.class[c as usize]
    }

    /// `SoundManager::Play3D` / `Play2D` / `UpdateVolume`: `volume` × the class option × `masterVolume`, which
    /// `DS8SoundSample::SetVolume` clamps to 0 dB, silent below 0.001. A 3D sound's distance gain and pan come on
    /// top ([`Listener::gains`]).
    pub(super) fn voice_volume(&self, c: SoundClass, volume: f32) -> f32 {
        let v = self.master * self.gain(c) * volume;
        if v < 0.001 { 0.0 } else { v.min(1.0) }
    }

    /// DirectSound's 3D processing: distance gain ([`distance_gain`]) times pan ([`ear_gains`]), for min distance
    /// `falloff`. Silent without ears.
    pub(super) fn gains(&self, at: Vec3, falloff: f32) -> [f32; 2] {
        let Some(ears) = self.ears else {
            return [0.0; 2];
        };
        let to = at - ears;
        let d = distance_gain(falloff, to.length());
        ear_gains(self.front, to.to_array()).map(|g| g * d)
    }

    /// Reads the option volumes when they changed or were never read since a reset; true when read. Also called
    /// by `mixer::mix`, so a voice started after a mid-frame reset still gets them.
    pub(super) fn sync_volumes(&mut self, options: &Res<GameOptions>) -> bool {
        if self.volumes_read && !options.is_changed() {
            return false;
        }
        self.master = options.float("masterVolume");
        for c in SoundClass::ALL {
            self.class[c as usize] = options.float(c.option());
        }
        self.volumes_read = true;
        true
    }
}

/// `GameCamera::GetTarget`: the followed point 1 m higher (the debug and timed camera offsets are not ported).
/// PORT: the followed point is the player's position (no stock PC mesh has a `Camera` attach point).
pub fn camera_target(followed: Vec3) -> Vec3 {
    followed + Vec3::Y
}

/// `WorldCamera::SetListenerPosition`: the ears are the camera's target 1 m lower.
pub fn ears(camera_target: Vec3) -> Vec3 {
    camera_target - Vec3::Y
}

/// The view direction flattened ([x, z]); a degenerate camera faces -z.
pub fn listener_front(eye: Vec3, target: Vec3) -> [f32; 2] {
    let f = Vec2::new(target.x - eye.x, target.z - eye.z)
        .try_normalize()
        .unwrap_or(Vec2::NEG_Y);
    [f.x, f.y]
}

/// Refreshes [`Listener`]: option volumes, frustum, ears and the voice count ([`VOICES`]).
pub(super) fn listen(
    mut sounds: ResMut<Sounds>,
    options: Res<GameOptions>,
    cams: Query<(
        &bevy::camera::primitives::Frustum,
        &gd_render::camera::GameCamera,
    )>,
    focus: Query<&GlobalTransform, With<CameraFocus>>,
    playing: Query<&SoundClass, With<Voice>>,
) {
    let l = &mut sounds.listener;
    l.sync_volumes(&options);
    let cam = cams.iter().next();
    l.view = cam.map(|c| *c.0);
    // The ears use the focus's propagated transform, as this frame's sound positions do: the camera target is
    // a frame older. Without a focus (menu, test scene) the camera target stands in.
    l.ears = focus
        .single()
        .ok()
        .map(|g| ears(camera_target(g.translation())))
        .or_else(|| cam.map(|c| ears(camera_target(c.1.target))));
    l.front = cam.map_or([0.0, -1.0], |c| listener_front(c.1.eye(), c.1.target));
    l.voices = playing.iter().count();
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundClass {
    Music,
    Effects,
    Ambient,
    Dialog,
}

impl SoundClass {
    const ALL: [SoundClass; 4] = [
        SoundClass::Music,
        SoundClass::Effects,
        SoundClass::Ambient,
        SoundClass::Dialog,
    ];

    pub fn option(self) -> &'static str {
        match self {
            SoundClass::Music => "musicVolume",
            SoundClass::Effects => "effectsVolume",
            SoundClass::Ambient => "ambientVolume",
            SoundClass::Dialog => "dialogVolume",
        }
    }
}

/// Before the options' gain.
#[derive(Component, Clone, Copy, PartialEq)]
pub struct BaseVolume(pub f32);

/// The voice starts paused at zero volume; `mixer::mix` (in `Last`) sets its final volume and starts it the same
/// frame.
pub fn sound(class: SoundClass, volume: f32, end: OnEnd, speed: f32) -> impl Bundle {
    (class, BaseVolume(volume), VoiceStart { end, speed })
}
