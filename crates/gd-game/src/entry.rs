//! A character's first entry into the game.
//!
//! Grim Dawn.exe, the client's world load (`Game::LoadModMap`): a character that has not been in the game
//! (`Player::GetHasBeenInGame` false: a new one) is set as having been in it, gets the new-character time of day and
//! tutorial page 7 (shown). [`first_entry`] is that branch ([`crate::tutorials::UnlockTutorial`], the clock's
//! [`gd_game_core::day_clock::DayClock::set`]). The menu's Create Character writes the file with the flag clear
//! (`crate::save::create_character`) and every session save sets it (`crate::save::save_game`), as the engine's
//! saves after this. The time of day is [`crate::day_clock`]'s.
//!
//! The engine also queues its intro video `video/intro.ogg` here (without a mod): `grimdawn::video` plays it for
//! the same first entry (`first_time_in_game`), and the story's `PlayVideo` action.
use bevy::prelude::*;
use gd_game_core::entry::EnteredGame;
use gd_game_core::player::Player;

pub struct EntryPlugin;

impl Plugin for EntryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            first_entry
                .run_if(resource_exists::<gd_game_core::session::SessionReady>)
                .run_if(not(resource_exists::<EnteredGame>))
                .after(gd_game_core::player::PlayerSet)
                .before(crate::tutorials::unlock_tutorials),
        );
    }
}

/// The character has not been in the game (`Player::GetHasBeenInGame` false): a new one, or a save whose flag is
/// clear.
pub fn first_time_in_game(loaded: Option<&gd_game_core::session::LoadedCharacter>) -> bool {
    loaded.is_none_or(|c| c.0.info.has_been_in_game == 0)
}

/// The world load's first-entry branch (module doc): once per session, when the player exists.
fn first_entry(
    mut commands: Commands,
    // Option: `loaded` is absent for a new character.
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    data: Res<gd_game_core::data::GameData>,
    mut clock: ResMut<gd_game_core::day_clock::DayClock>,
    players: Query<(), With<Player>>,
    mut tutorials: MessageWriter<crate::tutorials::UnlockTutorial>,
) {
    if players.is_empty() {
        return;
    }
    commands.insert_resource(EnteredGame);
    if !first_time_in_game(loaded.as_deref()) {
        return;
    }
    clock.set(gd_game_core::day_clock::new_character_time(&data.db));
    tutorials.write(crate::tutorials::UnlockTutorial(7));
}
