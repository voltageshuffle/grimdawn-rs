//! Unlocked tutorial pages (the journal's help pages, `player.gdc` block 15:
//! the page numbers in unlock order).
//!
//! `GameEngine::UnlockTutorialPage`: a page not yet
//! unlocked is set in the engine's page bit set and appended to the list;
//! with its `show` flag the engine then sends event 54 (the tutorial tip).
//! Callers: the scripts' `UnlockTutorial` (`Effect::UnlockTutorial`, show),
//! a riftgate binding the player (`StaticTeleporter::BindToCharacter`:
//! page 9, show; [`crate::riftgates::bind_riftgates`]).
//! [`TutorialTip`] is that event 54: sent once per newly unlocked page (the UI shows the quick tip, `ui/quicktip.rs`).
//! PORT: the engine ignores pages past its page count (the bit set's size, not traced).
use bevy::prelude::*;

/// The unlocked pages, in unlock order.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct TutorialPages {
    pub pages: Vec<u32>,
}

/// Unlocks a tutorial page (`GameEngine::UnlockTutorialPage`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct UnlockTutorial(pub u32);

/// The engine's event 54: a page was just unlocked (the quick tip of the page may show).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct TutorialTip(pub u32);

/// The pages from the loaded character once the player exists (no save:
/// none).
pub fn init_tutorials(
    mut commands: Commands,
    // Option: `pages` is the existence check (this system creates it); `loaded` is absent for a new character.
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
        use gd_game_core::session::MessageDrain;
        app.add_message::<UnlockTutorial>()
            .add_message::<TutorialTip>()
            .drain_messages_unless::<UnlockTutorial, _>(resource_exists::<TutorialPages>)
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
