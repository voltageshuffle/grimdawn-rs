//! Headless checks of the story runtime: it loads the save's quest states
//! and tokens, and its scripts run with the player in the world.
use bevy::prelude::*;
use gd_game::progress::PlayerProgress;
use gd_game::story::StoryRuntime;
use gd_game::testing::TestGame;

const TOKEN: &str = "TEST_STORY_TOKEN";

/// A new character with a story token at level 12, saved by the port; the
/// session started from that save loads the token (`quests.gdd`) and the
/// story's player level follows the game's.
#[test]
fn story_loads_the_save_and_ticks() {
    let Some(mut first) = TestGame::new_character("Tester", false, false) else {
        return;
    };
    first.spawn_player();
    first.step();
    first
        .app
        .world()
        .non_send::<StoryRuntime>()
        .story
        .borrow_mut()
        .player
        .tokens
        .insert(TOKEN.into());
    first
        .app
        .world_mut()
        .resource_mut::<PlayerProgress>()
        .progress
        .level = 12;
    first.step();
    // Valid while `first` lives.
    let save = first.save_now();
    let mut g = TestGame::with_save(&save).unwrap();
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    g.steps(5);
    let rt = g.app.world().non_send::<StoryRuntime>();
    let story = rt.story.borrow();
    assert!(story.player.tokens.contains(TOKEN), "the save's tokens");
    assert_eq!(
        story.player.level, 12,
        "the player's level follows the game"
    );
}
