//! Game camera input: zoom, rotation and the followed target of the world camera (`camera`: the camera
//! itself, its defaults, pitch curve, projection and shake). Game.dll `GameCamera`:
//! - Zoom (`GameCamera::Zoom` @0x1802bf700 / `UpdateFromInputImpl` @0x1802be540): a wheel step moves the wanted
//!   zoom by 0.1 of the min..max range; each frame the distance moves `clamp(0.004 dt_ms, 0, 1) * 0.5` of the way
//!   there (never past it) and snaps once within 0.001 of the range ([`zoom_step`]).
//! - Yaw (`GameCamera::Update` @0x1802bf870): the yaw eases to the wanted yaw, `(wanted - yaw) x dt_s x 4` per
//!   frame (never past it, still within 0.0001); a held rotate key turns the wanted yaw at 1 rad/s
//!   (`YawLeft` / `YawRight(true)`, `StopRotation` on release), a wheel-bound one steps it by 0.1 rad
//!   (`YawLeft` / `YawRight(false)`); `SetCameraYaw` sets both ([`yaw_step`]).
//! - Keys (`Grim Dawn.exe` sub_140026910), key actions: 21 zoom in
//!   (`Zoom(-1)`, wheel up by default), 22 zoom out (`Zoom(+1)`, wheel down), 23 / 24 all the way in / out
//!   (`Zoom(-/+10000)`, numpad 1 / 3), 25 back to the defaults (`LerpToDefaults` @0x1802bfc40: the default
//!   distance, yaw 50°; numpad 2), 26 rotate with the mouse while held (middle mouse: `AdjustYaw((last x - x) x
//!   0.0025)` per cursor pixel, sub_14001F370), 27 / 28 rotate left / right (',' / '.'); bound to the wheel, 26-28
//!   step instead. The wheel is key codes 145 (up, DirectInput `DIMOFS_Z` > 0) / 146 (down) (DirectInput.dll
//!   `DirectInputDevice::Update` @0x1800031a0), so it does what the actions bound to them do.
//!   PORT: the zoom keys are not refused while the window the engine checks (+270944, unidentified) is open, nor the
//!   keys while a text field takes them.
//!
//! The followed target is read from its `Transform` (the player is a root entity): the same frame's
//! position the gameplay just wrote, not the pre-propagation `GlobalTransform` of the frame before
//! (`GameCamera::GetTarget` @0x1802be6e0 reads the attach point in the same tick after movement).
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use gd_render::camera::{CameraFocus, CameraSet, GameCamera};

/// Registers the camera input; the camera itself is `camera::CameraPlugin`.
pub struct CameraInputPlugin;

impl Plugin for CameraInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            camera_input
                .before(CameraSet)
                .before(TransformSystems::Propagate),
        );
    }
}

/// `UpdateFromInputImpl`: one frame of the zoom ease from `cur` to `want` (distances) over `range` = max - min.
pub fn zoom_step(cur: f32, want: f32, range: f32, dt_ms: f32) -> f32 {
    let diff = want - cur;
    if diff.abs() <= 0.001 * range {
        return want;
    }
    let step = (dt_ms * 0.004).clamp(0.0, 1.0) * 0.5 * diff;
    if diff > 0.0 {
        cur + step.min(diff)
    } else {
        cur + step.max(diff)
    }
}

/// `GameCamera::Update`: one frame of the yaw ease from `yaw` to `wanted` over `dt` seconds.
pub fn yaw_step(yaw: f32, wanted: f32, dt: f32) -> f32 {
    let diff = wanted - yaw;
    if diff.abs() <= 0.0001 {
        return yaw;
    }
    let step = diff * dt * 4.0;
    yaw + if diff > 0.0 {
        step.min(diff)
    } else {
        step.max(diff)
    }
}

/// The wheel zooms only over the plain world: a window under the cursor
/// (`UiCapture`, the same flag that keeps clicks off the world) or the modal
/// options window consumes it (PORT: the engine routes input to the UI first).
fn wheel_zooms(dy: f32, ui_captured: bool, modal_open: bool) -> bool {
    dy != 0.0 && !ui_captured && !modal_open
}

/// Key codes of the wheel the camera actions may be bound to (module docs); the other mouse codes (middle, Mouse 4
/// to 8) come through the bindings (`ui::keys::mouse_codes`).
const WHEEL_UP: u32 = 145;
const WHEEL_DOWN: u32 = 146;

/// One key action's press on the camera (`sub_140026910`); `wheel`: the press is a wheel notch, up or down (a bound
/// rotation then steps 0.1 rad: 26 left on wheel up and right on wheel down, 27 left, 28 right).
fn press(cam: &mut GameCamera, action: u32, wheel: Option<bool>) {
    let range = cam.distance_max - cam.distance_min;
    let zoom = |cam: &mut GameCamera, steps: f32| {
        let from = cam.zoom.unwrap_or(cam.distance);
        cam.zoom = Some((from + steps * 0.1 * range).clamp(cam.distance_min, cam.distance_max));
    };
    match (action, wheel) {
        (21, _) => zoom(cam, -1.0),
        (22, _) => zoom(cam, 1.0),
        (23, _) => zoom(cam, -10000.0),
        (24, _) => zoom(cam, 10000.0),
        (25, _) => cam.lerp_to_defaults(),
        (26, Some(up)) => cam.wanted_yaw += if up { 0.1 } else { -0.1 },
        (27, Some(_)) => cam.wanted_yaw += 0.1,
        (28, Some(_)) => cam.wanted_yaw -= 0.1,
        (27, None) => cam.rotation = 1,
        (28, None) => cam.rotation = -1,
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn camera_input(
    mut cams: Query<&mut GameCamera>,
    scroll: Res<AccumulatedMouseScroll>,
    capture: Res<crate::inventory::UiCapture>,
    options: Res<crate::ui::options::OptionsWindow>,
    pause: Res<gd_game::pause::GamePause>,
    // The main menu's screens (present while it shows) take the wheel too.
    main_menu: Option<Res<crate::menu::MainMenu>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<crate::ui::keys::KeyBindings>,
    time: Res<Time>,
    // The focus (the player) is a root entity: its `Transform` is this frame's
    // position — `GlobalTransform` would still hold last frame's (propagation runs
    // after this), and the camera would trail the player by a frame.
    focus: Query<&Transform, With<CameraFocus>>,
    mut last_x: Local<Option<f32>>,
) {
    // Cursor x in physical pixels (the engine's mouse events).
    let x = window
        .single()
        .ok()
        .and_then(|w| w.cursor_position().map(|c| c.x * w.scale_factor()));
    let dt = time.delta_secs();
    for mut cam in &mut cams {
        let range = cam.distance_max - cam.distance_min;
        // The wheel: what its codes are bound to.
        // The game menu, its confirm and Options (`GamePause::menu`) and the main menu take the wheel.
        if wheel_zooms(
            scroll.delta.y,
            capture.0,
            options.open || pause.menu || main_menu.is_some(),
        ) {
            let code = if scroll.delta.y > 0.0 {
                WHEEL_UP
            } else {
                WHEEL_DOWN
            };
            // A notch per line; a pixel-unit (touchpad) scroll counts as one.
            let notches = match scroll.unit {
                bevy::input::mouse::MouseScrollUnit::Line => {
                    (scroll.delta.y.abs().round() as i32).max(1)
                }
                bevy::input::mouse::MouseScrollUnit::Pixel => 1,
            };
            for _ in 0..notches {
                for action in 21..=28 {
                    if bindings.get(action).contains(&code) {
                        press(&mut cam, action, Some(code == WHEEL_UP));
                    }
                }
            }
        }
        // Key actions (Mouse 4-8 / middle bound ones too) do nothing under the game menu / Options.
        for action in [21, 22, 23, 24, 25, 27, 28] {
            if !pause.menu && bindings.just_pressed_no_wheel(action, &keys) {
                press(&mut cam, action, None);
            }
        }
        if bindings.just_released(27, &keys) || bindings.just_released(28, &keys) {
            cam.rotation = 0;
        }
        if let Some(want) = cam.zoom {
            cam.distance = zoom_step(cam.distance, want, range, dt * 1000.0);
            if cam.distance == want {
                cam.zoom = None;
            }
        }
        // Action 26 held (default: the middle mouse button): the mouse turns the wanted yaw.
        let rotate_held = !pause.menu && bindings.pressed(26, &keys);
        if rotate_held
            && let (Some(x), Some(last)) = (x, *last_x)
            && x != last
        {
            cam.wanted_yaw += (last - x) * 0.0025;
        }
        // `GameCamera::Update`: the held rotate key, then the ease.
        let turn = cam.rotation as f32 * dt;
        cam.wanted_yaw += turn;
        cam.yaw = yaw_step(cam.yaw, cam.wanted_yaw, dt);
        if let Ok(t) = focus.single() {
            cam.target = t.translation;
        } else {
            // Free-fly for debugging when nothing is focused.
            let mut d = Vec3::ZERO;
            let fwd = Vec3::new(-cam.yaw.sin(), 0.0, -cam.yaw.cos());
            let right = Vec3::new(-fwd.z, 0.0, fwd.x);
            if keys.pressed(KeyCode::KeyW) {
                d += fwd;
            }
            if keys.pressed(KeyCode::KeyS) {
                d -= fwd;
            }
            if keys.pressed(KeyCode::KeyD) {
                d += right;
            }
            if keys.pressed(KeyCode::KeyA) {
                d -= right;
            }
            let speed = if keys.pressed(KeyCode::ShiftLeft) {
                120.0
            } else {
                30.0
            };
            cam.target += d.normalize_or_zero() * speed * dt;
        }
    }
    if x.is_some() {
        *last_x = x;
    }
}
