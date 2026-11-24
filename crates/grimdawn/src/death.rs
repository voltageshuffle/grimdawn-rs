//! Death, presentation side: the gravestone's model and idle animation,
//! clicking it, and the hardcore Game Over dialog. The rules (penalty,
//! redemption, play stats) are `gd_game::death`.
//!
//! The Game Over dialog is the dialog manager's ok box (`ui/confirm.rs`, `tagGameOver`), as the engine opens
//! it (`ControllerPlayer::CharacterIsDying`: `DialogManager::AddDialog` with the
//! OK-only layout and the localised, genderised `tagGameOver`; the exe draws the Ok button of
//! `prompt_manager.dbr` for it, `PromptManager::LoadOkButton` / `PromptManager::DrawOkButton`). Answering it — Ok,
//! close or Esc — leaves the
//! session ([`gd_ui::state::exit::ExitToMenu`]): the engine's `DeathUpdate` hands the response to the UI interface
//! and the dead hardcore character cannot continue; the character list shows it as dead (`session::is_dead`).
use gd_ui::state::inventory::UiCapture;
use gd_ui::state::death::GameOverBox;
use gd_ui::window::{Rebuild, add_window, despawn_all};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
pub use gd_game::death::*;
use gd_game_core::combat::Dead;
use gd_game_core::player::Player;
pub use gd_game_core::session::is_hardcore;
use gd_render::character::CharAssets;
use gd_ui_kit::{UiAssets, contains, cursor_px, spawn_root};

pub struct DeathPlugin;

impl Plugin for DeathPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<Gravestone, Visibility>()
            .init_resource::<GameOverBox>()
            .add_systems(
                Update,
                (dress_graves, game_over.after(on_death)).after(gd_game_core::player::PlayerSet),
            );
        add_window(
            app,
            build_game_over,
            game_over_clicks,
            |b: Res<GameOverBox>| b.is_open(),
        );
        // The box's hover / press look moves in place (no rebuild on a hover).
        app.add_systems(
            Update,
            game_over_live
                .run_if(|b: Res<GameOverBox>| b.is_open())
                .in_set(gd_ui_kit::UiPlaySet),
        );
    }
}

/// New gravestones get their model and idle animation, once the model is built (it loads in the background,
/// `CharAssets::model_ready`); [`GraveDressed`] marks a dressed one.
fn dress_graves(
    mut commands: Commands,
    new: Query<Entity, (With<Gravestone>, Without<GraveDressed>)>,
    mut assets: CharAssets,
) {
    use gd_render::model::Ready;
    for e in &new {
        let db = assets.data.db.clone();
        let Some(rec) = db.get(GRAVE) else {
            commands.entity(e).try_insert(GraveDressed);
            continue;
        };
        let model = match rec.str("mesh").map(|m| assets.model_ready(m)) {
            Some(Ready::Loading) => continue,
            Some(Ready::Done(model)) => Some(model),
            Some(Ready::Failed) | None => None,
        };
        commands.entity(e).try_insert(GraveDressed);
        let Some(model) = model else {
            continue;
        };
        gd_render::model::spawn_model(&mut commands, e, &model);
        if let Some(an) = rec
            .str("closeAnimation")
            .and_then(|p| assets.cache.anim(&assets.data, p))
        {
            commands.entity(e).insert(gd_render::model::Animator {
                anim: an,
                time: 0.0,
                speed: 1.0,
                looped: true,
            });
        }
    }
}

/// A gravestone [`dress_graves`] is done with.
#[derive(Component)]
struct GraveDressed;

/// A hardcore character died: open the Game Over dialog (module doc).
fn game_over(
    died: Query<(), (With<Player>, Added<Dead>)>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    choice: Res<gd_game_core::session::Choice>,
    mut over: ResMut<GameOverBox>,
) {
    if !died.is_empty() && is_hardcore(loaded.as_deref(), &choice) {
        over.open = true;
    }
}



#[derive(Component)]
struct GameOverRoot;

#[allow(clippy::too_many_arguments)]
fn build_game_over(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<gd_game_core::data::GameData>,
    mut state: ResMut<GameOverBox>,
    roots: Query<Entity, With<GameOverRoot>>,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<Rebuild<(bool, UVec2)>>,
    mut assets: UiAssets,
) {
    if fresh.begun() {
        state.open = false;
        built.reset();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || !built.changed((state.open, size)) {
        return;
    }
    despawn_all(&mut commands, &roots);
    state.rects = None;
    if !state.open {
        return;
    }
    let s = assets.scale(size);
    let root = spawn_root(&mut commands, (GameOverRoot, gd_ui::hud::KeepsShown), 6);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    state.rects = gd_ui_kit::confirm::ok_box(&mut ui, "tagGameOver", (None, false));
}

/// The box's hover / press look in place ([`crate::ui::confirm::apply_look`]).
fn game_over_live(
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    state: Res<GameOverBox>,
    mouse: Res<ButtonInput<MouseButton>>,
    roots: Query<Entity, With<GameOverRoot>>,
    mut looks: Query<(&gd_ui_kit::confirm::ConfirmLook, &mut ImageNode)>,
) {
    let (Ok(win), Some((ok, close)), Ok(root)) = (window.single(), state.rects, roots.single())
    else {
        return;
    };
    let hit = cursor_px(&hooks, win).and_then(|p| {
        if contains(&ok, p) {
            Some(gd_ui_kit::confirm::Hit::Yes)
        } else if close.is_some_and(|r| contains(&r, p)) {
            Some(gd_ui_kit::confirm::Hit::Close)
        } else {
            None
        }
    });
    let look = (hit, hit.is_some() && mouse.pressed(MouseButton::Left));
    gd_ui_kit::confirm::apply_look(root, look, &mut looks);
}

/// Ok or close answers the dialog: the session ends (module doc; Esc: `ui::esc_dispatcher`).
#[allow(clippy::too_many_arguments)]
fn game_over_clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut state: ResMut<GameOverBox>,
    mut capture: ResMut<UiCapture>,
    data: Res<gd_game_core::data::GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
    mut exit: MessageWriter<gd_ui::state::exit::ExitToMenu>,
) {
    if !state.open {
        return;
    }
    capture.0 = true;
    let mut answer = false;
    if let (Ok(win), Some((ok, close))) = (window.single(), state.rects)
        && let Some(p) = cursor_px(&hooks, win)
        && buttons.just_pressed(MouseButton::Left)
    {
        if contains(&ok, p) {
            answer = true;
        } else if close.is_some_and(|r| contains(&r, p)) {
            if let Some(pak) = gd_ui_kit::confirm::close_sound(&data) {
                sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
            }
            answer = true;
        }
    }
    if answer {
        state.open = false;
        exit.write(gd_ui::state::exit::ExitToMenu);
    }
}
