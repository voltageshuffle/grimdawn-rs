//! Presentational story effects the binary draws: the `Character:Say` speech bubble
//! ([`gd_game::story::SayBubble`]) and the `Game.ScreenShake` camera-shake bridge
//! ([`gd_game::story::ScreenShake`]). Presentation only: the story host inserts the bubble / sets the shake.
//!
//! ## Speech bubble (`Character:Say`, LUA-14)
//! Grim Dawn.exe `LuaCharacter::Say`: localises the tag
//! (`LocalizationManager::LocalizeWithoutParams`) and hands it to the UI vcall `GameUIInterface+24`,
//! which spawns a speech window (`SpeechWindow::SpeechWindow`) — a `UITextBox` loaded from
//! `records/sandbox/paul/speechwindow.dbr` with a 5000 ms life. Its update (`SpeechWindow::Update`): the life
//! counts down by the frame ms; under 1000 ms left the alpha is `left / 1000` (a one-second fade out);
//! the speaker's speech coords are projected to the screen and the box is centred on them with its top
//! `2 × height` above the point; a speaker that went clamps the life to ≤ 1000 ms (fades out).
//! PORT: the sandbox record is not in the shipped data, so the box is the black rect +
//! `genericrolloverbox` border of the quick tip with the text in `style_rollover_description` and a
//! `tutorialtipbottom.tex` tail; the life / fade come from the host's `until` (session seconds), the
//! anchor is the speaker's `HeadEffect` attach point (else 1.6 m up, like the floating texts) projected
//! to the screen, and the box top sits `2 × height` above it. The bubble is drawn while `now < until`;
//! the host removes the expired [`SayBubble`] (the UI only reads it and despawns its own nodes).
//!
//! ## Camera shake (`Game.ScreenShake`, LUA-15)
//! The engine's `Game.ScreenShake(vec, secs, amplitude)` is a game event type 7 → the same
//! `GameCamera::Update` shake as a skill's (`gd_render::camera::apply_shake`, the [`CameraShake`]
//! message). The story host sets [`ScreenShake`] (amplitude + seconds left); this bridge forwards a
//! fresh value to the camera as a [`CameraShake`] and decays `secs_left` by the game frame's dt.
//! PORT: the resource carries no position, so the shake is the player's own (`own`, always applied).
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::collections::{HashMap, HashSet};
use gd_game::story::{SayBubble, ScreenShake};
use gd_game_core::data::GameData;
use gd_render::camera::GameCamera;
use gd_render::character::Character;
use gd_ui::fade::{Fader, seal_fade_group};
use gd_ui_kit::{UiAssets, spawn_root};

/// The text style of the bubble's line (the quick tip's body style; the engine's sandbox record is not
/// shipped).
const STYLE: &str = "records/ui/styles/text/style_rollover_description.dbr";
/// The box border (the quick tip's frame).
const BOX: &str = "records/ui/styles/rollover/style_genericrolloverbox.dbr";
/// The tail under the box (the quick tip's pointer arrow).
const TAIL: &str = "ui/tutorialwindow/tutorialtipbottom.tex";
/// The bubble text's max width (UI units) and inner padding.
const MAX_TEXT: f32 = 280.0;
const PAD: f32 = 8.0;
/// The fade-out window (s) as the bubble's `until` approaches (the engine's last 1000 ms).
const FADE: f32 = 1.0;

pub struct StoryViewPlugin;

impl Plugin for StoryViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScreenShake>()
            .add_systems(
                PostUpdate,
                say_bubbles
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::ui::UiSystems::Layout)
                    .in_set(gd_ui_kit::UiPlaySet),
            )
            .add_systems(Update, screen_shake.in_set(gd_ui_kit::UiPlaySet));
    }
}

/// The full-screen root the bubbles hang under.
#[derive(Component)]
struct SayRoot;

/// A bubble's movable container (box, border, text and tail are children in local coordinates).
#[derive(Component)]
struct SayView;

/// A speaker's built bubble.
struct Entry {
    container: Entity,
    /// The box size (physical px), for the anchor.
    w: f32,
    h: f32,
    /// What it was built for (tag, tags generation, screen, UI scale); a change rebuilds it.
    sig: u64,
}

#[derive(Default)]
struct Bubbles {
    root: Option<Entity>,
    entries: HashMap<Entity, Entry>,
}

/// The speaker's speech point: the `HeadEffect` attach point (bind pose), else 1.6 m up (the floating
/// texts' anchor).
fn head(gt: &GlobalTransform, ch: Option<&Character>) -> Vec3 {
    ch.and_then(|c| gd_game_core::casting::attach_point_bind(&c.body, "HeadEffect"))
        .map_or(gt.translation() + Vec3::Y * 1.6, |l| gt.transform_point(l))
}

/// A bubble's build signature.
fn sig(tag: &str, generation: u64, size: UVec2, scale: f32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tag.hash(&mut h);
    generation.hash(&mut h);
    size.x.hash(&mut h);
    size.y.hash(&mut h);
    scale.to_bits().hash(&mut h);
    h.finish()
}

/// Draws each live [`SayBubble`] over its speaker and drops the bubbles whose speaker went or whose
/// `until` passed (the host removes the component; only the UI nodes are despawned here).
#[allow(clippy::too_many_arguments)]
fn say_bubbles(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    window: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform), With<GameCamera>>,
    speakers: Query<(Entity, &SayBubble, &GlobalTransform, Option<&Character>)>,
    mut assets: UiAssets,
    mut fader: Fader,
    mut nodes: Query<(&mut Node, &mut Visibility), With<SayView>>,
    mut state: Local<Bubbles>,
) {
    if fresh.begun() {
        if let Some(root) = state.root.take() {
            commands.entity(root).try_despawn();
        }
        state.entries.clear();
    }
    let now = time.elapsed_secs();
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 {
        return;
    }
    let Ok((cam, cam_gt)) = cams.single() else {
        return;
    };
    let scale = assets.scale(size);
    let inv = 1.0 / win.scale_factor();
    let sf = win.scale_factor();
    let generation = assets.fonts.generation;

    let mut active = HashSet::default();
    for (e, bubble, gt, ch) in &speakers {
        if now >= bubble.until {
            continue;
        }
        active.insert(e);
        let s = sig(&bubble.tag, generation, size, scale);
        let rebuild = state.entries.get(&e).is_none_or(|en| en.sig != s);
        if rebuild {
            if let Some(en) = state.entries.remove(&e) {
                commands.entity(en.container).try_despawn();
            }
            let text = assets.fonts.tag(&bubble.tag).trim().to_owned();
            if text.is_empty() {
                continue;
            }
            let root = *state
                .root
                .get_or_insert_with(|| spawn_root(&mut commands, SayRoot, 1));
            let mut ui = assets.ui(
                &mut commands,
                &data,
                root,
                scale,
                inv,
                Vec2::new(size.x as f32, size.y as f32),
            );
            let max_w = ui.sc(MAX_TEXT);
            let lines = ui.wrap_lines(STYLE, max_w, &text);
            let lh = ui.line_height(STYLE);
            let pad = ui.sc(PAD);
            let mut tw = 0.0f32;
            for l in &lines {
                tw = tw.max(ui.text_width(STYLE, l));
            }
            let w = tw + 2.0 * pad;
            let h = lh * lines.len() as f32 + 2.0 * pad;
            // The anchor this frame, so the box does not flash at the origin before the first move.
            let p = cam
                .world_to_viewport(cam_gt, head(gt, ch))
                .ok()
                .map(|p| p * sf);
            let (left, top, vis) = match p {
                Some(p) => (
                    Val::Px((p.x - w * 0.5) * inv),
                    Val::Px((p.y - h * 2.0) * inv),
                    Visibility::Inherited,
                ),
                None => (Val::Px(0.0), Val::Px(0.0), Visibility::Hidden),
            };
            let container = gd_ui::fade::fade_group(&mut ui, root, 1.0);
            // The box, border, centred lines and tail, in local coordinates (the container moves them).
            let r = gd_formats::ui::Rect::new(0.0, 0.0, w, h);
            ui.solid(r, Color::srgba(0.0, 0.0, 0.0, 0.85));
            if let Some(b) = ui.record(BOX) {
                ui.border(&b, "", r, 1.0);
            }
            for (i, l) in lines.iter().enumerate() {
                let lw = ui.text_width(STYLE, l);
                let at = Vec2::new((w - lw) * 0.5, pad + lh * i as f32);
                ui.text_line(STYLE, at, l, 1.0);
            }
            if let Some((img, sz)) = ui.image(TAIL) {
                let sz = sz.as_vec2();
                let a = gd_formats::ui::Rect::new(w * 0.5 - sz.x * 0.5, h, sz.x, sz.y);
                ui.quad(img, a, None, Color::WHITE);
            }
            seal_fade_group(ui.commands, container);
            // Replace the fade group's full-screen node with the sized box anchored on the speaker; its
            // children (built in local coordinates) move and fade with it.
            ui.commands.entity(container).insert((
                SayView,
                vis,
                Node {
                    position_type: PositionType::Absolute,
                    left,
                    top,
                    width: Val::Px(w * inv),
                    height: Val::Px(h * inv),
                    ..default()
                },
            ));
            state.entries.insert(
                e,
                Entry {
                    container,
                    w,
                    h,
                    sig: s,
                },
            );
        }
        let Some(en) = state.entries.get(&e) else {
            continue;
        };
        if let Ok((mut node, mut vis)) = nodes.get_mut(en.container) {
            match cam.world_to_viewport(cam_gt, head(gt, ch)).ok() {
                Some(p) => {
                    let p = p * sf;
                    node.left = Val::Px((p.x - en.w * 0.5) * inv);
                    node.top = Val::Px((p.y - en.h * 2.0) * inv);
                    *vis = Visibility::Inherited;
                }
                None => *vis = Visibility::Hidden,
            }
        }
        let left = bubble.until - now;
        fader.set(en.container, if left < FADE { left.max(0.0) } else { 1.0 });
    }
    // A speaker that went or expired: drop its bubble (the host removes the SayBubble component).
    state.entries.retain(|&e, en| {
        if active.contains(&e) {
            true
        } else {
            commands.entity(en.container).try_despawn();
            false
        }
    });
}

/// Forwards a fresh [`ScreenShake`] to the render camera and decays its `secs_left` by the game frame's
/// dt; [`gd_render::camera::apply_shake`] runs the offset.
fn screen_shake(
    mut shake: ResMut<ScreenShake>,
    time: Res<Time>,
    mut writer: MessageWriter<gd_game_core::visuals::CameraShake>,
    mut last: Local<f32>,
) {
    // A value the host changed since the last frame's decay is a new shake.
    if shake.secs_left > 0.0 && shake.secs_left != *last {
        writer.write(gd_game_core::visuals::CameraShake {
            ms: shake.secs_left * 1000.0,
            amp: shake.amplitude,
            at: Vec3::ZERO,
            own: true,
        });
    }
    // Settled at 0: nothing to decay (no change mark).
    if shake.secs_left != 0.0 {
        shake.secs_left = (shake.secs_left - time.delta_secs()).max(0.0);
    }
    *last = shake.secs_left;
}
