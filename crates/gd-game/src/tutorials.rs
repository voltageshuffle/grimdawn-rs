//! Unlocked tutorial pages (`player.gdc` block 15: page numbers in unlock order).
//!
//! `GameEngine::UnlockTutorialPage`: a page not yet unlocked is appended; with its `show` flag the
//! engine sends event 54 ([`TutorialTip`]). Callers: the scripts' `UnlockTutorial` (show) and
//! `StaticTeleporter::BindToCharacter` (page 9, show).
//! PORT: the engine ignores pages past its page count (not traced).
use bevy::prelude::*;

#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct TutorialPages {
    pub pages: Vec<u32>,
}

/// Unlocks a tutorial page (`GameEngine::UnlockTutorialPage`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct UnlockTutorial(pub u32);

/// The engine's event 54: a page was just unlocked.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct TutorialTip(pub u32);

/// The pages from the loaded character once the player exists.
pub fn init_tutorials(
    mut commands: Commands,
    // Option: `pages` is the existence check; `loaded` is absent for a new character.
    pages: Option<Res<TutorialPages>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    players: Query<(), With<gd_game_core::player::Player>>,
) {
    if pages.is_some() || players.is_empty() {
        return;
    }
    commands.insert_resource(TutorialPages {
        pages: loaded
            .map(|c| c.0.tutorials.pages.clone())
            .unwrap_or_default(),
    });
}

/// Applies [`UnlockTutorial`] and the scripts' `UnlockTutorial`.
pub fn unlock_tutorials(
    mut pages: ResMut<TutorialPages>,
    mut unlocks: MessageReader<UnlockTutorial>,
    mut story: MessageReader<gd_game_core::story::StoryEffect>,
    mut tips: MessageWriter<TutorialTip>,
) {
    let scripted = story.read().filter_map(|e| match e.0 {
        gd_story::story::Effect::UnlockTutorial(p) => Some(p),
        _ => None,
    });
    for p in unlocks.read().map(|u| u.0).chain(scripted) {
        if !pages.pages.contains(&p) {
            pages.pages.push(p);
            tips.write(TutorialTip(p));
        }
    }
}

pub struct TutorialsPlugin;

impl Plugin for TutorialsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<UnlockTutorial>()
            .add_message::<TutorialTip>()
            .add_systems(
                Update,
                (
                    init_tutorials
                        .after(gd_game_core::player::PlayerSet)
                        .run_if(not(resource_exists::<TutorialPages>)),
                    unlock_tutorials
                        .run_if(resource_exists::<TutorialPages>)
                        .after(crate::riftgates::RiftgateStage::Bind),
                )
                    .chain(),
            );
    }
}
