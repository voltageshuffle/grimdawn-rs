//! The HUD minimap's mode (`UIPlayerHud::ToggleCompass`), kept on the player and in its character file
//! (`CharacterInfo::compass_state`).
//!
//! A compass-button press steps the mode 3 → 4, 2 → 3, anything else → 2. Modes: 2 closed, 3 the floating minimap
//! (160 px × UI scale), 4 the large one (246 px). A new character starts in mode 3.
use bevy::prelude::*;
use gd_game_core::player::Player;

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompassMode(pub u8);

/// A new character's mode.
pub const NEW_CHARACTER_MODE: u8 = 3;

#[derive(Message, Clone, Copy, Debug)]
pub struct ToggleCompass;

pub fn next_mode(mode: u8) -> u8 {
    match mode {
        3 => 4,
        2 => 3,
        _ => 2,
    }
}

pub fn init_compass(
    mut commands: Commands,
    // Option: `have` is the existence check; `loaded` is absent for a new character.
    have: Option<Res<CompassMode>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    players: Query<(), With<Player>>,
) {
    if have.is_some() || players.is_empty() {
        return;
    }
    let mode = loaded.map_or(NEW_CHARACTER_MODE, |c| c.0.info.compass_state);
    commands.insert_resource(CompassMode(mode));
}

pub fn apply_toggles(mut toggles: MessageReader<ToggleCompass>, mut mode: ResMut<CompassMode>) {
    for _ in toggles.read() {
        mode.0 = next_mode(mode.0);
    }
}

pub struct CompassPlugin;

impl Plugin for CompassPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ToggleCompass>().add_systems(
            Update,
            (
                init_compass.run_if(not(resource_exists::<CompassMode>)),
                apply_toggles.run_if(resource_exists::<CompassMode>),
            )
                .chain()
                .after(gd_game_core::inventory::InventorySet)
                .before(gd_game_core::player::PlayerSet),
        );
    }
}
