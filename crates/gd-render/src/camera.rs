//! The game camera's view: `GAME::WorldCamera` (vertical FOV 30°, near 1, far 250) orbiting a target, with the
//! distance/pitch limits from `records/game/gameengine.dbr`, its placement and the shake (Game.dll `GameCamera`):
//! - Defaults (`GameEngine::LoadFromDatabase`, `GameCamera::ResetToDefaults`, also run by
//!   `GameEngine::ExitPlayingMode`): distance `CameraDistanceDefault`, yaw 50° (0.8726646 rad), the pitch of that distance.
//! - The pitch follows the distance (`GameCamera::UpdatePitch`, [`pitch_for`]): linear from
//!   `CameraPitchMin` at `CameraDistanceMin` to `CameraPitchDefault` at `CameraDistanceDefault`, then to
//!   `CameraPitchMax` at `CameraDistanceMax` (stock 20 m 38°, 36 m 46°, 48 m 52°).
//! - Target (`GameCamera::GetTarget`): the player's "Camera" attach point (the hero mesh has none, so
//!   its origin, Engine.dll `GraphicsMesh::GetAttachedCoords`) raised 1 m; the eye is `distance` from it
//!   along yaw / pitch (Engine.dll `WorldCamera::CalculateViewPosition`, [`GameCamera::eye`]).
use bevy::prelude::*;
use gd_game_core::data::GameData;

#[derive(Component)]
pub struct GameCamera {
    /// The followed point (the player's origin); the camera looks [`TARGET_RAISE`] above it.
    pub target: Vec3,
    pub yaw: f32,
    pub distance: f32,
    pub distance_min: f32,
    pub distance_max: f32,
    /// `CameraDistanceDefault`: the start distance and the knee of the pitch curve.
    pub distance_default: f32,
    /// `CameraPitchMin` / `CameraPitchDefault` / `CameraPitchMax`, radians.
    pub pitch_limits: [f32; 3],
    /// The distance a wheel zoom eases to (`None` once reached).
    pub zoom: Option<f32>,
    /// The yaw [`GameCamera::yaw`] eases to (`GameCamera.wantedYaw`).
    pub wanted_yaw: f32,
    /// A held rotate key (`GameCamera.rotation`): 1 left (the wanted yaw grows), -1 right, 0 none.
    pub rotation: i8,
}

/// `GameCamera::GetTarget`: the look-at point is the target raised 1 m.
pub const TARGET_RAISE: f32 = 1.0;
/// `GameCamera::ResetToDefaults`: 50°.
pub const DEFAULT_YAW: f32 = 0.872_664_6;

/// `GameCamera::UpdatePitch`: the pitch (radians) at distance `d` (module doc).
pub fn pitch_for(d: f32, dist: [f32; 3], pitch: [f32; 3]) -> f32 {
    let ([d_min, d_def, d_max], [p_min, p_def, p_max]) = (dist, pitch);
    let p = if d >= d_def {
        p_def + (d - d_def) / (d_max - d_def) * (p_max - p_def)
    } else {
        p_min + (d - d_min) / (d_def - d_min) * (p_def - p_min)
    };
    // `UpdateFromInputImpl` keeps it within 0..89°; a degenerate range keeps the default.
    if p.is_finite() {
        p.clamp(0.0, 89f32.to_radians())
    } else {
        p_def
    }
}

/// The entity the camera follows, if any.
#[derive(Component)]
pub struct CameraFocus;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CameraShake>()
            .init_resource::<ShakeState>()
            .add_systems(Startup, spawn_camera);
        #[cfg(feature = "dev")]
        app.add_systems(Startup, camera_hooks.after(spawn_camera));
        app.add_systems(
            PostUpdate,
            (
                apply_camera.in_set(CameraSet),
                apply_shake.in_set(CameraSet),
            )
                .chain()
                .before(TransformSystems::Propagate),
        );
    }
}

/// `WorldCamera::WorldCamera`: fov 30°, near 1, far 250 (Game.dll never changes it).
pub fn world_projection() -> PerspectiveProjection {
    PerspectiveProjection {
        fov: std::f32::consts::FRAC_PI_6,
        near: 1.0,
        far: 250.0,
        ..default()
    }
}

fn spawn_camera(mut commands: Commands, data: Res<GameData>) {
    let eng = data.db.get("records/game/gameengine.dbr");
    let get = |k: &str, d: f32| eng.as_ref().and_then(|r| r.f32(k)).unwrap_or(d);
    let mut cam = GameCamera {
        target: Vec3::ZERO,
        yaw: DEFAULT_YAW,
        distance: 0.0,
        distance_min: get("CameraDistanceMin", 20.0),
        distance_max: get("CameraDistanceMax", 48.0),
        distance_default: get("CameraDistanceDefault", 36.0),
        pitch_limits: [
            get("CameraPitchMin", 38.0).to_radians(),
            get("CameraPitchDefault", 46.0).to_radians(),
            get("CameraPitchMax", 52.0).to_radians(),
        ],
        zoom: None,
        wanted_yaw: DEFAULT_YAW,
        rotation: 0,
    };
    cam.reset_to_defaults();
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(world_projection()),
        cam,
    ));
}

/// GD_CAM_YAW=<degrees> overrides the starting yaw, GD_CAM_DIST the starting distance (unclamped).
#[cfg(feature = "dev")]
fn camera_hooks(hooks: Res<gd_game_core::hooks::Hooks>, mut cams: Query<&mut GameCamera>) {
    for mut cam in &mut cams {
        if let Some(y) = hooks.render.cam_yaw {
            cam.yaw = y.to_radians();
            cam.wanted_yaw = cam.yaw;
        }
        if let Some(d) = hooks.render.cam_dist {
            cam.distance = d;
        }
    }
}

/// `GameEngine::ExitPlayingMode` → `GameCamera::ResetToDefaults`.
pub fn reset_cameras(world: &mut World) {
    let mut q = world.query::<&mut GameCamera>();
    for mut c in q.iter_mut(world) {
        c.reset_to_defaults();
    }
}

impl GameCamera {
    pub fn reset_to_defaults(&mut self) {
        self.distance = self.distance_default;
        self.yaw = DEFAULT_YAW;
        self.wanted_yaw = DEFAULT_YAW;
        self.rotation = 0;
        self.zoom = None;
    }

    /// `LerpToDefaults` (key action 25): eases to the default distance and to yaw 50° (the yaw first taken modulo a
    /// full turn, `fmodf`, so it turns the short way round from within one turn).
    pub fn lerp_to_defaults(&mut self) {
        self.zoom = Some(self.distance_default);
        self.yaw %= std::f32::consts::TAU;
        self.wanted_yaw = DEFAULT_YAW;
    }

    pub fn pitch(&self) -> f32 {
        pitch_for(
            self.distance,
            [self.distance_min, self.distance_default, self.distance_max],
            self.pitch_limits,
        )
    }

    pub fn look_at(&self) -> Vec3 {
        self.target + Vec3::Y * TARGET_RAISE
    }

    pub fn eye(&self) -> Vec3 {
        let pitch = self.pitch();
        self.look_at()
            + Vec3::new(
                self.yaw.sin() * pitch.cos(),
                pitch.sin(),
                self.yaw.cos() * pitch.cos(),
            ) * self.distance
    }
}

pub fn apply_camera(mut cams: Query<(&GameCamera, &mut Transform)>) {
    for (cam, mut t) in &mut cams {
        t.set_if_neq(Transform::from_translation(cam.eye()).looking_at(cam.look_at(), Vec3::Y));
    }
}

pub use gd_game_core::visuals::CameraShake;

/// The running shake: milliseconds left, the total (1.25 x the duration) and the amplitude.
#[derive(Resource, Default)]
pub struct ShakeState {
    remaining: f32,
    total: f32,
    amp: f32,
}

/// `GameCamera::Update`: with `r` ms left of `total`, `q = total / 4`: the envelope is
/// `r / q` below `q`, 1 above, and `1 - (r - (total - q)) / q` above `total - q` (a fade in from 0.8); the camera
/// is shifted by `amp * env * (B cos(0.02 r) + 0.5 A sin(0.02 r))` metres, A and B the camera's two axes (PORT:
/// A = right, B = up; the engine's pair is not resolved), a pure translation that ends with the timer.
/// The timer counts down with the game frame's ms (virtual time): a pause freezes the shake with the game.
fn apply_shake(
    mut events: MessageReader<CameraShake>,
    mut state: ResMut<ShakeState>,
    options: Res<gd_game_core::options::GameOptions>,
    time: Res<Time>,
    mut cams: Query<(&Camera, &GlobalTransform, &mut Transform), With<GameCamera>>,
) {
    let Ok((camera, gt, mut tf)) = cams.single_mut() else {
        events.clear();
        return;
    };
    let on = options.flag("cameraShake");
    for ev in events.read() {
        let inside = ev.own
            || camera.world_to_ndc(gt, ev.at).is_some_and(|n| {
                n.x.abs() <= 1.0 && n.y.abs() <= 1.0 && (0.0..=1.0).contains(&n.z)
            });
        if on && inside && ev.ms > 0.0 {
            debug!("camera: shake {} ms, amplitude {}", ev.ms, ev.amp);
            *state = ShakeState {
                remaining: ev.ms,
                total: ev.ms * 1.25,
                amp: ev.amp,
            };
        }
    }
    if state.remaining <= 0.0 {
        return;
    }
    let (r, q) = (state.remaining, state.total * 0.25);
    let mut env = if r < q { r / q } else { 1.0 };
    if r > state.total - q {
        env = 1.0 - (r - (state.total - q)) / q;
    }
    let t = r * 0.02;
    let offset = state.amp * env * (tf.up() * t.cos() + 0.5 * tf.right() * t.sin());
    tf.translation += offset;
    state.remaining -= time.delta_secs() * 1000.0;
}

/// The camera is moved and its projection applied.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CameraSet;
