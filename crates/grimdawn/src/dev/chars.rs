//! Character test scene: `GD_CHARS="player:male;player:female;creature:<record>;..."`
//! (`player:male:<item>,<item>` for explicit gear; `creature:<record>:<variant>`), lined up on a ground
//! plane far from the world's levels. `GD_CHAR_KEYS=1`: keys 1–4 switch all characters to idle / run /
//! attack / death; `GD_CHAR_STATE` sets the initial state and `GD_CHAR_HOLD=<0..1>` freezes attacks at that
//! fraction (for screenshots; both read by `gd_game::character`). `GD_CHAR_CAM` / `GD_CHAR_YAW`: the camera's
//! distance and yaw.
use bevy::prelude::*;
use gd_game::character::drive_characters;
use gd_game_core::hooks::Hooks;
use gd_game_core::player::starting_gear;
use gd_render::camera::{CameraFocus, GameCamera};
use gd_render::character::{CharAssets, CharState, Character, spawn_player};
use gd_sim::anim::AnimType;

/// The scene's origin, far from the world's levels so streaming leaves it alone.
const ORIGIN: Vec3 = Vec3::new(20000.0, 3000.0, 20000.0);

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Startup, scene).add_systems(
        Update,
        (
            keys.before(drive_characters),
            keep_camera.after(drive_characters),
        ),
    );
}

/// Marks the `GD_CHARS` scene: the camera stays on it.
#[derive(Resource)]
pub struct TestScene {
    pub distance: f32,
}

fn scene(
    hooks: Res<Hooks>,
    mut commands: Commands,
    mut a: CharAssets,
    mut cams: Query<&mut GameCamera>,
) {
    let Some(list) = &hooks.session.chars else {
        return;
    };
    let specs: Vec<&str> = list.split(';').filter(|s| !s.is_empty()).collect();
    let n = specs.len() as f32;
    let spacing = 2.6;
    let ground = a.meshes.add(Plane3d::default().mesh().size(80.0, 80.0));
    let gmat = a.materials.add(Color::srgb(0.3, 0.32, 0.25));
    commands.spawn((
        Mesh3d(ground),
        MeshMaterial3d(gmat),
        Transform::from_translation(ORIGIN),
    ));
    for (i, spec) in specs.iter().enumerate() {
        // Line up along the camera's screen-right axis (camera yaw π/4).
        let off = (i as f32 - (n - 1.0) / 2.0) * spacing;
        let pos = ORIGIN
            + Vec3::new(
                off * std::f32::consts::FRAC_1_SQRT_2,
                0.0,
                -off * std::f32::consts::FRAC_1_SQRT_2,
            );
        let tf = Transform::from_translation(pos)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_4));
        let mut it = spec.splitn(3, ':');
        match (it.next(), it.next(), it.next()) {
            (Some("player"), Some(sex), gear) => {
                let female = sex.starts_with('f');
                let gear: Vec<String> = match gear {
                    Some(g) => g.split(',').map(str::to_owned).collect(),
                    None => starting_gear(&a.data.db, female),
                };
                spawn_player(&mut commands, &mut a, tf, female, &gear);
            }
            (Some("creature"), Some(rec), v) => {
                gd_game::creature::spawn_creature(
                    &mut commands,
                    &a.data,
                    &mut a.bodies,
                    tf,
                    rec,
                    v.and_then(|v| v.parse().ok()).unwrap_or(0),
                    None,
                );
            }
            _ => warn!("bad GD_CHARS entry {spec}"),
        }
    }
    let dist = hooks.render.char_cam.unwrap_or(6.0 + n * 2.5);
    for mut c in &mut cams {
        c.distance = dist;
        c.distance_min = 2.0;
        c.target = ORIGIN + Vec3::Y * 1.3;
    }
    commands.insert_resource(TestScene { distance: dist });
    commands.insert_resource(super::SceneCamera);
}

/// `GD_CHAR_KEYS` (hooks.rs): keys 1-4 force every character into idle/run/attack/death. Off by default: 1-0 are
/// the hotbar keys.
fn keys(
    hooks: Res<Hooks>,
    keys: Res<ButtonInput<KeyCode>>,
    mut chars: Query<&mut Character>,
    mut on: Local<Option<bool>>,
) {
    if !*on.get_or_insert_with(|| hooks.render.char_keys) {
        return;
    }
    let s = if keys.just_pressed(KeyCode::Digit1) {
        CharState::Idle
    } else if keys.just_pressed(KeyCode::Digit2) {
        CharState::Run
    } else if keys.just_pressed(KeyCode::Digit3) {
        CharState::Attack
    } else if keys.just_pressed(KeyCode::Digit4) {
        CharState::Death
    } else {
        return;
    };
    for mut c in &mut chars {
        c.state = s;
        c.playing = AnimType::None;
        if s == CharState::Attack {
            // Force a fresh attack even when one is playing.
            c.playing = AnimType::AttackIdle;
        }
    }
}

/// The world's player takes the camera focus when it spawns; in the test scene the camera stays on the
/// characters.
fn keep_camera(
    hooks: Res<Hooks>,
    mut commands: Commands,
    scene: Option<Res<TestScene>>,
    focus: Query<Entity, With<CameraFocus>>,
    mut cams: Query<&mut GameCamera>,
) {
    let Some(scene) = scene else { return };
    for e in &focus {
        commands.entity(e).try_remove::<CameraFocus>();
    }
    for mut c in &mut cams {
        if !focus.is_empty() {
            c.target = ORIGIN + Vec3::Y * 1.3;
        }
        // First frame with a camera (it spawns in Startup too).
        if c.distance_min != 2.0 {
            c.distance_min = 2.0;
            c.distance = scene.distance;
            c.target = ORIGIN + Vec3::Y * 1.3;
            if let Some(y) = hooks.render.char_yaw {
                c.yaw = y.to_radians();
                c.wanted_yaw = c.yaw;
            }
        }
    }
}
