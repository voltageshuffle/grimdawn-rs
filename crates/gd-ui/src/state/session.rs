//! The app's phase ([`AppState`]): the binary's `session.rs` mirrors the session resources into it;
//! every play UI system runs under `in_state(AppState::InGame)`.
use bevy::prelude::*;

/// The app's phase. PORT: the engine switches modes in `GameEngine::EnableGameEngine`;
/// here the state mirrors the session resources:
/// `Menu` until the session is chosen (`SessionReady`), `Loading` while its player
/// is spawned and the save applied, `InGame` once [`in_game`] holds. Every HUD /
/// window / tooltip system is in `ui::UiPlaySet`, which runs under `in_state(AppState::InGame)`.
#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Menu,
    Loading,
    InGame,
}
