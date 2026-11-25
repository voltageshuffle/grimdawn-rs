//! The spirit guide (`NpcSkillReallocator`, rules and engine facts in `gd_game::reclamation`): clicking one is the
//! shared interact order (`gd_game::player_orders`, the hover pick); gd-game walks beside it and in reach (the NPC
//! service rule, `gd_game::reclamation`) opens the mode; this module opens what the answer says, after closing the open
//! windows (GrimDawn.exe `GameUI::DisplaySkillReallocationWindow` → `UIPlayerHud::CloseWindows`): the skills window in
//! reclamation mode (drawn by `ui::skills`, its clicks become [`ReclaimRequest::Refund`]) or the devotion window in its
//! reclaim mode (drawn by `ui::devotion`: a star click refunds, the sidebar shows the money, aether and price). The two
//! windows' tabs to each other move the mode ([`ReclaimRequest::Switch`]). Hiding the window that has the mode sends
//! [`ReclaimRequest::Close`]; the mode ending in gd-game (walking away) hides that window.
//!
//! The "nothing to reclaim" message box is the dialog manager's ok box (`confirm.rs`): built
//! once in its base look, its hover / press look moves in place (`box_live`).
//!
//! `GD_USE_SPIRIT_GUIDE` (hooks.rs): once, walks to the nearest spirit guide within 40 m as if it were clicked
//! (one stands at `GD_START=-3455,6972`, Region0J001).
use crate::state::inventory::UiCapture;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_game::reclamation::{ReclaimAnswer, ReclaimRequest, SkillReclamation, is_spirit_guide};

pub struct ReclamationUiPlugin;

impl Plugin for ReclamationUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NoPointsBox>().add_systems(
            Update,
            (click_spirit_guide, answers)
                .chain()
                .in_set(super::UiInputSet)
                .after(crate::skills::clicks),
        );
        super::window::add_window(app, build_box, box_clicks, |b: Res<NoPointsBox>| {
            b.open || b.rects.is_some()
        });
        // The box's hover / press look moves in place (no rebuild on a hover).
        app.add_systems(
            Update,
            box_live
                .run_if(|b: Res<NoPointsBox>| b.open || b.rects.is_some())
                .after(build_box)
                .in_set(super::UiPlaySet),
        );
    }
}

/// What the skills window draws in reclamation mode (`HudValues::reclaim`, from `ui::view::collect`; `None` while
/// the devotion window has the mode).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReclaimView {
    /// Refunds made (`SkillReclamation::used`; prices the next one).
    pub used: u32,
    /// Skills with a celestial power bound (the f7 flag).
    pub bound: Vec<String>,
}

impl ReclaimView {
    /// Writes the view of an active mode (`None` without one) into `slot`, allocating only when it differs;
    /// whether it changed.
    pub fn put(
        slot: &mut Option<Self>,
        r: Option<&SkillReclamation>,
        devotion: Option<&gd_game::devotion::DevotionState>,
    ) -> bool {
        let Some(r) = r.filter(|r| r.skills_mode()) else {
            return slot.take().is_some();
        };
        let bindings = devotion.map_or(&[][..], |d| &d.bindings[..]);
        if let Some(v) = slot
            && v.used == r.used
            && v.bound.len() == bindings.len()
            && v.bound.iter().zip(bindings).all(|(a, b)| *a == b.skill)
        {
            return false;
        }
        *slot = Some(Self {
            used: r.used,
            bound: bindings.iter().map(|b| b.skill.clone()).collect(),
        });
        true
    }

    pub fn is_bound(&self, skill: &str) -> bool {
        self.bound.iter().any(|b| b.eq_ignore_ascii_case(skill))
    }
}

/// What the devotion window draws in its reclaim mode (`HudValues::devotion_reclaim`, from `ui::view::collect`):
/// the aether crystals held (`gd_game::devotion::aether_held`); the money and the price come from `HudValues::gold`
/// and the devotion state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DevotionReclaimView {
    pub aether: u32,
}

impl DevotionReclaimView {
    /// The view while the devotion window has the mode (`aether` counts the crystals).
    pub fn of(r: Option<&SkillReclamation>, aether: impl FnOnce() -> u32) -> Option<Self> {
        r.filter(|r| r.devotion_mode())
            .map(|_| Self { aether: aether() })
    }
}

/// Clicking a spirit guide is the shared interact order ([`gd_game::player_orders::PlayerOrder::Interact`],
/// from the hover pick [`crate::state::hover::Hovered`]): gd-game walks beside it and, in reach, opens the mode
/// (`NpcSkillReallocator::OnPlayerInteract`). The Interact key's `UseObject` routes through the same
/// order (`apply_orders`).
#[allow(clippy::too_many_arguments)]
fn click_spirit_guide(
    hooks: Res<gd_game_core::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: crate::state::casting::ClickKeys,
    capture: Res<UiCapture>,
    data: Res<gd_game_core::data::GameData>,
    hovered: Res<crate::state::hover::Hovered>,
    npcs: Query<
        (Entity, &GlobalTransform, &gd_render::character::Character),
        Without<gd_game_core::player::Player>,
    >,
    players: Query<&Transform, With<gd_game_core::player::Player>>,
    mut orders: MessageWriter<gd_game::player_orders::PlayerOrder>,
    mut tested: Local<bool>,
) {
    let Ok(tf) = players.single() else {
        return;
    };
    let guide =
        |c: &gd_render::character::Character| data.db.get(&c.record).is_some_and(is_spirit_guide);
    // `GD_USE_SPIRIT_GUIDE`: the nearest within 40 m, as if clicked.
    if !*tested && hooks.gameplay.use_spirit_guide {
        let near = npcs
            .iter()
            .filter(|(_, gt, c)| gt.translation().distance(tf.translation) < 40.0 && guide(c))
            .min_by(|a, b| {
                a.1.translation()
                    .distance(tf.translation)
                    .total_cmp(&b.1.translation().distance(tf.translation))
            });
        if let Some((e, gt, _)) = near {
            *tested = true;
            info!(
                "GD_USE_SPIRIT_GUIDE: walking to the spirit guide at {}",
                gt.translation()
            );
            orders.write(gd_game::player_orders::PlayerOrder::Interact(e));
            return;
        }
    }
    if capture.0 || keys.skips_objects() || !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    // The hover pick (one pick feeds the hover and the world clicks): a spirit guide under the cursor.
    let Some(e) = hovered.entity else { return };
    if let Ok((_, _, c)) = npcs.get(e)
        && guide(c)
    {
        orders.write(gd_game::player_orders::PlayerOrder::Interact(e));
    }
}

/// Opens what an [`ReclaimAnswer`] says; keeps the window and the mode together.
fn answers(
    mut answers: MessageReader<ReclaimAnswer>,
    mut windows: super::window::Windows,
    // Option: created with the player (`gd_game::reclamation::init_reclamation`).
    state: Option<Res<SkillReclamation>>,
    mut out: MessageWriter<ReclaimRequest>,
    mut no_points: ResMut<NoPointsBox>,
) {
    use super::window::Window;
    for a in answers.read() {
        match a {
            // Through the window policy, so Factions / Achievements / Loot Filter close too; the other
            // of the three character windows always closes (the spirit guide switches between them).
            ReclaimAnswer::Skills => {
                windows.close(Window::Character);
                windows.close(Window::Devotion);
                windows.open(Window::Skills);
                if windows.panels.skill_tab == 0 {
                    windows.panels.skill_tab = 1;
                }
            }
            ReclaimAnswer::Devotion => {
                windows.close(Window::Character);
                windows.close(Window::Skills);
                windows.open(Window::Devotion);
            }
            ReclaimAnswer::NoPoints => no_points.open = true,
            ReclaimAnswer::Closed { devotion: false } => windows.close(Window::Skills),
            ReclaimAnswer::Closed { devotion: true } => windows.close(Window::Devotion),
        }
    }
    // The window with the mode hid (Esc, its close button, the key; not its tab to the other window, which
    // switches): the mode ends.
    let panels = &windows.panels;
    if state
        .is_some_and(|s| s.skills_mode() && !panels.skills || s.devotion_mode() && !panels.devotion)
    {
        out.write(ReclaimRequest::Close);
    }
}

/// The "nothing to reclaim" message box (module doc).
#[derive(Resource, Default)]
pub struct NoPointsBox {
    open: bool,
    /// The Ok and close rects as drawn (physical px).
    rects: Option<(gd_formats::ui::Rect, Option<gd_formats::ui::Rect>)>,
}

impl NoPointsBox {
    /// The shared Esc dispatcher's close (`ui::esc_dispatcher`): the box is a notice, Esc dismisses it.
    pub fn escape(&mut self) -> bool {
        std::mem::replace(&mut self.open, false)
    }
}

#[allow(clippy::too_many_arguments)]
fn build_box(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<gd_game_core::data::GameData>,
    mut state: ResMut<NoPointsBox>,
    roots: Query<Entity, With<BoxRoot>>,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<super::window::Rebuild<(bool, UVec3)>>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        state.open = false;
        built.reset();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || !built.changed((state.open, size.extend(assets.scale(size).to_bits()))) {
        return;
    }
    super::window::despawn_all(&mut commands, &roots);
    state.rects = None;
    if !state.open {
        return;
    }
    let s = assets.scale(size);
    let root = gd_ui_kit::spawn_root(
        &mut commands,
        (BoxRoot, super::hud::KeepsShown),
        super::window::Z_MODAL,
    );
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    state.rects = super::confirm::ok_box(&mut ui, "tagReclaimNoPoints", (None, false));
    // The box takes every click while up: no HUD button under it plays its sound.
    ui.sound_blocker(gd_formats::ui::Rect::new(
        0.0,
        0.0,
        size.x as f32,
        size.y as f32,
    ));
}

/// The box's hover / press look in place ([`super::confirm::apply_look`]): the box is built in
/// its base look, so a hover never rebuilds it.
fn box_live(
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    pause: Res<gd_game_core::pause::GamePause>,
    state: Res<NoPointsBox>,
    mouse: Res<ButtonInput<MouseButton>>,
    roots: Query<Entity, With<BoxRoot>>,
    mut looks: Query<(&super::confirm::ConfirmLook, &mut ImageNode)>,
) {
    let (Ok(win), Some((ok, close)), Ok(root)) = (window.single(), state.rects, roots.single())
    else {
        return;
    };
    // The game menu / Options over the box takes the pointer.
    let hit = super::cursor_px(&hooks, win)
        .filter(|_| !pause.menu)
        .and_then(|p| {
            if super::contains(&ok, p) {
                Some(super::confirm::Hit::Yes)
            } else if close.is_some_and(|r| super::contains(&r, p)) {
                Some(super::confirm::Hit::Close)
            } else {
                None
            }
        });
    let look = (hit, hit.is_some() && mouse.pressed(MouseButton::Left));
    super::confirm::apply_look(root, look, &mut looks);
}

#[derive(Component)]
struct BoxRoot;

fn box_clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut state: ResMut<NoPointsBox>,
    mut capture: ResMut<UiCapture>,
    data: Res<gd_game_core::data::GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
) {
    if !state.open {
        return;
    }
    capture.0 = true;
    // Esc dismisses the box through the shared dispatcher (`ui::esc_dispatcher`).
    let (Ok(win), Some((ok, close))) = (window.single(), state.rects) else {
        return;
    };
    let Some(p) = super::cursor_px(&hooks, win) else {
        return;
    };
    if !super::widgets::button_pressed(&buttons) {
        return;
    }
    if super::contains(&ok, p) {
        state.open = false;
    } else if close.is_some_and(|r| super::contains(&r, p)) {
        state.open = false;
        if let Some(pak) = super::confirm::close_sound(&data) {
            sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
        }
    }
}
