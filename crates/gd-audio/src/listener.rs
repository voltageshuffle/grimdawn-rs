//! The listener ([`Listener`]: ears, view, voice count and option volumes) and the sound classes.
use super::Sounds;
use super::mixer::{OnEnd, Voice, VoiceStart};
use bevy::prelude::*;
use gd_game_core::options::GameOptions;
use gd_render::camera::CameraFocus;
use gd_sim::sound3d::ear_gains;

/// Concurrent sounds the engine plays: `SoundManager` has 128 playing slots (`GetEmptyCurrentPlayingIndex`); a
/// stream takes one too (`StartStream`), so the port's slots are every playing sound entity, the music included.
pub const VOICES: usize = 128;

/// What `pak::play_pak` knows of the listener this frame (refreshed by [`listen`] before the sounds of the frame
/// start). The single source of the option volumes every sound's gain is made of.
#[derive(Default)]
pub struct Listener {
    /// Sounds playing (or started this frame) that take a voice slot.
    pub voices: usize,
    /// `masterVolume`.
    pub master: f32,
    /// Option volume of each class (index [`SoundClass`] as usize).
    pub class: [f32; 4],
    /// The game camera's view frustum.
    pub view: Option<bevy::camera::primitives::Frustum>,
    /// Where sounds are heard from ([`ears`]): distance gains and hearing ranges of every 3D sound.
    pub ears: Option<Vec3>,
    /// The way the ears face ([`listener_front`]).
    pub front: [f32; 2],
    /// `master` and `class` hold the options (false after a reset: read them again).
    volumes_read: bool,
}

impl Listener {
    /// The class's option volume.
    pub(super) fn gain(&self, c: SoundClass) -> f32 {
        self.class[c as usize]
    }

    /// The final gain of a class: master × class option, clamped to 0..1.
    pub(super) fn gain_final(&self, c: SoundClass) -> f32 {
        (self.master * self.gain(c)).clamp(0.0, 1.0)
    }

    /// The ears' gains of a sound at `at`.
    pub(super) fn gains(&self, at: Vec3) -> [f32; 2] {
        ear_gains(
            self.front,
            self.ears.map_or([0.0; 3], |e| (at - e).to_array()),
        )
    }

    /// Reads `masterVolume` and the class volumes from the options.
    pub(super) fn read_volumes(&mut self, options: &GameOptions) {
        self.master = options.float("masterVolume");
        for c in SoundClass::ALL {
            self.class[c as usize] = options.float(c.option());
        }
        self.volumes_read = true;
    }
}

/// The engine's camera target (`GameCamera::GetTarget`, Game.dll): the followed point 1 m
/// higher (plus the debug offset `GameCamera.debugOffset`, zero from `ResetToDefaults`, and the camera offset while
/// `GameCamera.offsetTimeLeft` > 0, not ported). The followed point is the player's `Camera` attach point; no stock
/// PC mesh has one, PORT: taken as the player's own position, the port camera's
/// [`GameCamera::target`](gd_render::camera::GameCamera).
pub fn camera_target(followed: Vec3) -> Vec3 {
    followed + Vec3::Y
}

/// The listener: `WorldCamera::SetListenerPosition` (Engine.dll) puts
/// `SoundManager::SetEarsPosition` at the camera's target ([`camera_target`]) 1 m lower, every frame.
pub fn ears(camera_target: Vec3) -> Vec3 {
    camera_target - Vec3::Y
}

/// The listener's front ([x, z]): the view direction flattened (`gd_sim::sound3d` module doc). `eye` above `target`
/// is never level with it in the game; a degenerate camera faces -z.
pub fn listener_front(eye: Vec3, target: Vec3) -> [f32; 2] {
    let f = Vec2::new(target.x - eye.x, target.z - eye.z)
        .try_normalize()
        .unwrap_or(Vec2::NEG_Y);
    [f.x, f.y]
}

/// Refreshes [`Listener`]: the options' volumes (when they changed), the camera's frustum, the ears and the sounds
/// taking a voice slot (every sound entity, streams included, [`VOICES`]).
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
    if options.is_changed() || !l.volumes_read {
        l.read_volumes(&options);
    }
    let cam = cams.iter().next();
    l.view = cam.map(|c| *c.0);
    // The ears sit at the followed point, the same propagated snapshot this frame's sound positions read: the
    // camera target itself is a frame older (`camera_input` runs before transform propagation), which panned the
    // player's own sounds sideways on every start / stop. Without a focus (menu, test scene) the camera target
    // stands in.
    l.ears = focus
        .single()
        .ok()
        .map(|g| ears(camera_target(g.translation())))
        .or_else(|| cam.map(|c| ears(camera_target(c.1.target))));
    l.front = cam.map_or([0.0, -1.0], |c| listener_front(c.1.eye(), c.1.target));
    l.voices = playing.iter().count();
}

/// What a sound's volume option is.
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

    /// The class's volume option.
    pub fn option(self) -> &'static str {
        match self {
            SoundClass::Music => "musicVolume",
            SoundClass::Effects => "effectsVolume",
            SoundClass::Ambient => "ambientVolume",
            SoundClass::Dialog => "dialogVolume",
        }
    }
}

/// A sound's own volume (before the options' gain).
#[derive(Component, Clone, Copy, PartialEq)]
pub struct BaseVolume(pub f32);

/// A sound's components: its voice (`mixer::attach_voice`) starts paused at zero volume; `mixer::mix` (in `Last`)
/// sets its final volume and starts it the same frame, so none plays a frame at the wrong volume or before its event.
pub fn sound(class: SoundClass, volume: f32, end: OnEnd, speed: f32) -> impl Bundle {
    (class, BaseVolume(volume), VoiceStart { end, speed })
}
