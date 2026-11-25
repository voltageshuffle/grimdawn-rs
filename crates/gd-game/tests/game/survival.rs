//! Crucible host (`gd_game::survival`): the Merit difficulty unlock and the arena's save folder.
use bevy::prelude::*;
use gd_game::testing::TestGame;

/// A Merit item (`ItemDifficultyUnlock`, game mode Survival) unlocks its Crucible difficulty once.
#[test]
fn a_merit_unlocks_the_crucible_difficulty() {
    use gd_game_core::inventory::Inventory;
    use gd_game_core::session::Choice;
    use gd_game_core::story::StoryRuntime;
    const MERIT: &str = "records/items/misc/dlc_crucible_epicunlock.dbr";
    let Some(mut g) = TestGame::new_character("Tester", false, false) else {
        return;
    };
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    g.steps(2);
    let greatest = |g: &TestGame| {
        let rt = g.app.world().non_send::<StoryRuntime>();
        rt.story.borrow().game.survival.greatest_difficulty
    };
    let set = |g: &mut TestGame, v: u8| {
        let rt = g.app.world().non_send::<StoryRuntime>();
        rt.story.borrow_mut().game.survival.greatest_difficulty = v;
    };
    let item = || gd_formats::save::item::Item {
        base_name: MERIT.into(),
        stack_count: 1,
        ..Default::default()
    };
    set(&mut g, 0);
    // Outside the Crucible the item is refused and kept.
    g.app
        .world_mut()
        .resource_mut::<Inventory>()
        .used
        .push((item(), None));
    g.steps(2);
    assert_eq!(greatest(&g), 0);
    // In it, the difficulty unlocks.
    g.app.world_mut().resource_mut::<Choice>().survival = Some("a".into());
    g.app
        .world_mut()
        .resource_mut::<Inventory>()
        .used
        .push((item(), None));
    g.steps(2);
    assert_eq!(greatest(&g), 1, "Elite unlocked");
}

/// A Crucible session saves its story state into the arena's own folder
/// (`maps_survivalworld_a.map/<Difficulty>/`, `GetDifficultyFolder`), never the campaign's.
#[test]
fn crucible_story_state_saves_into_the_arena_folder() {
    use gd_formats::save::gdd::Quests;
    use gd_game_core::session::Choice;
    use gd_game_core::story::StoryRuntime;
    // A character the port saved, loaded (a new character's creation save would write the campaign folder).
    let Some(mut new) = TestGame::new_character("Tester", false, false) else {
        return;
    };
    new.spawn_player();
    new.steps(2);
    let Some(mut g) = TestGame::with_save(&new.save_now()) else {
        return;
    };
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    g.steps(2);
    // Flipping the choice, not the data overlay (shared by the test binary).
    g.app.world_mut().resource_mut::<Choice>().survival = Some("maps/survivalworld_a.map".into());
    g.app
        .world()
        .non_send::<StoryRuntime>()
        .story
        .borrow_mut()
        .player
        .tokens
        .insert("CRUCIBLE_TEST_TOKEN".into());
    let player = g.save_now();
    let char_dir = player.parent().unwrap();
    let q = std::fs::read(char_dir.join("maps_survivalworld_a.map/Normal/quests.gdd"))
        .expect("quests.gdd in the arena folder");
    let q = Quests::parse(&q).unwrap();
    // The tokens are the character's (`player.gdc` survival vector), not the arena file's.
    assert!(q.tokens.is_empty());
    let gdc = std::fs::read(&player).unwrap();
    let gdc = gd_formats::save::gdc::Gdc::parse(&gdc).unwrap();
    assert!(
        gdc.tokens
            .lists
            .iter()
            .flatten()
            .any(|t| t == "CRUCIBLE_TEST_TOKEN")
    );
    assert!(
        !char_dir
            .join("levels_world001.map/Normal/quests.gdd")
            .exists(),
        "the campaign folder is untouched"
    );
}
