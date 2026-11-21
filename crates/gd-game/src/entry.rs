//! A character's first entry into the game.
//!
//! Grim Dawn.exe, the client's world load (sub_140015530): a character that has not been in the game
//! (`Player::GetHasBeenInGame` false: a new one) is set as having been in it, gets the new-character time of day and
//! tutorial page 7 (shown). [`first_entry`] is that branch ([`crate::tutorials::UnlockTutorial`], the clock's
//! [`crate::day_clock::DayClock::set`]); a port save is written with `has_been_in_game` set
//! (`crate::save::new_character`), as the engine's after this. The time of day is [`crate::day_clock`]'s.
//!
//! PORT: the engine also queues its intro video `video/intro.ogg` here (without a mod), and story scripts play
//! videos (`ScriptableAction_PlayVideo`). The port plays no videos.
use crate::player::Player;
use bevy::prelude::*;

pub struct EntryPlugin;

impl Plugin for EntryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            first_entry
                .run_if(not(resource_exists::<EnteredGame>))
                .after(crate::player::PlayerSet)
                .before(crate::tutorials::unlock_tutorials),
        );
    }
}

/// The session's player has entered the game ([`first_entry`] ran; removed by `end_session_state`).
#[derive(Resource)]
pub struct EnteredGame;

/// The character has not been in the game (`Player::GetHasBeenInGame` false): a new one, or a save whose flag is
/// clear.
pub fn first_time_in_game(loaded: Option<&crate::session::LoadedCharacter>) -> bool {
    loaded.is_none_or(|c| c.0.info.has_been_in_game == 0)
}

/// The world load's first-entry branch (module doc): once per session, when the player exists.
fn first_entry(
    mut commands: Commands,
    // Option: `loaded` is absent for a new character.
    loaded: Option<Res<crate::session::LoadedCharacter>>,
    data: Res<crate::data::GameData>,
    mut clock: ResMut<crate::day_clock::DayClock>,
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
    clock.set(crate::day_clock::new_character_time(&data.db));
    tutorials.write(crate::tutorials::UnlockTutorial(7));
}
