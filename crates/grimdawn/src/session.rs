//! The play session's start point: `map.dat`'s spawn, else (a save without one) the loaded character's
//! respawn point, a uid resolved through the map's instance groups as the engine does. The session
//! state itself (character choice, [`LoadedCharacter`]) is `gd_game::session`.
use crate::world::StreamedMap;
use gd_ui::state::session::AppState;
use bevy::prelude::*;
pub use gd_game::session::*; // incl. `in_game`, which `AppState` mirrors
pub use gd_game_core::session::*;


/// `AppState` follows the session resources (PreUpdate, so the transition applies the same frame).
// The area-ready gate reads StreamedMap/Levels/PlayerStart alongside the state mirrors.
#[allow(clippy::too_many_arguments)]
fn sync_state(
    playing: In<bool>,
    ready: Option<Res<SessionReady>>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    world: Option<Res<StreamedMap>>,
    levels: Option<Res<crate::world::Levels>>,
    start: Option<Res<PlayerStart>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
) {
    let want = if playing.0 {
        // Lift the cover only once the spawn area is fully streamed in, like a
        // teleport (`StreamedMap::area_ready`); once playing, stay playing. The test
        // arena replaces the streamed levels, so it does not wait.
        // (Tested only while still loading: the area test is not free.)
        let area_ready = || {
            hooks.session.testworld.is_some()
                || match (&world, &levels, &start) {
                    (Some(w), Some(l), Some(sp)) => w.area_ready(l, sp.0),
                    _ => true,
                }
        };
        if *state.get() == AppState::InGame || area_ready() {
            AppState::InGame
        } else {
            AppState::Loading
        }
    } else if ready.is_some() {
        AppState::Loading
    } else {
        AppState::Menu
    };
    if *state.get() != want {
        next.set(want);
    }
}

pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .add_systems(PreUpdate, in_game.pipe(sync_state))
            .add_systems(Startup, load_save)
            .add_systems(
                Update,
                (
                    resolve_start,
                    save_input.before(gd_game::save::SaveStage::Save),
                ),
            );
    }
}

fn resolve_start(
    mut commands: Commands,
    world: Option<Res<StreamedMap>>,
    character: Option<Res<LoadedCharacter>>,
    start: Option<Res<PlayerStart>>,
    ready: Option<Res<SessionReady>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    choice: Res<gd_game_core::session::Choice>,
) {
    if start.is_some() || ready.is_none() {
        return;
    }
    let Some(world) = world else { return };
    if let Some(p) = gd_game::save::player_start(
        &world.map,
        world.spawn_point(),
        hooks.session.start.as_deref(),
        character.as_deref().map(|c| &*c.0),
        &choice,
    ) {
        commands.insert_resource(PlayerStart(p));
    }
}

/// F5 saves (PORT: the port's key, no engine binding found for it), and so does closing the window (the game saves
/// on quit).
fn save_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut closing: MessageReader<bevy::window::WindowCloseRequested>,
    mut out: MessageWriter<gd_game::save::SaveRequest>,
) {
    if keys.just_pressed(KeyCode::F5) || closing.read().count() > 0 {
        out.write(gd_game::save::SaveRequest);
    }
}
