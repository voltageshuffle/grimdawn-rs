//! Headless checks of faction reputation (save values, kills, story).
use bevy::prelude::*;
use gd_game::combat::MonsterKilled;
use gd_game::factions::{PlayerFactions, index_of_name};
use gd_game::story::StoryEffect;
use gd_game::testing::TestGame;
use gd_sim::story::Effect;

fn value(g: &TestGame, name: &str) -> f32 {
    g.app
        .world()
        .resource::<PlayerFactions>()
        .value(index_of_name(name).unwrap())
}

/// Story effects set and adjust a reputation; killing an outlaw costs
/// Outlaws reputation.
#[test]
fn story_and_kills_change_reputation() {
    let Some(mut g) = TestGame::new_character("Tester", false, false) else {
        return;
    };
    let p = g.spawn_player();
    g.step();
    g.app
        .world_mut()
        .write_message(StoryEffect(Effect::Faction {
            faction: "Survivors".into(),
            delta: 1000.0,
            set: true,
        }));
    g.step();
    assert_eq!(value(&g, "Survivors"), 1000.0);
    g.app
        .world_mut()
        .write_message(StoryEffect(Effect::Faction {
            faction: "Survivors".into(),
            delta: 250.0,
            set: false,
        }));
    g.step();
    assert!(value(&g, "Survivors") > 1000.0);
    let before = value(&g, "Outlaws");
    g.app.world_mut().write_message(MonsterKilled {
        record: "records/creatures/enemies/humanoutlaw_melee_b02.dbr".into(),
        level: 37,
        position: Vec3::ZERO,
        entity: Entity::PLACEHOLDER,
        killer: Some((p, None)),
    });
    g.step();
    assert!(
        value(&g, "Outlaws") < before,
        "{} -> {}",
        before,
        value(&g, "Outlaws")
    );
    // A death without the player's (or a pet's) kill leaves the player's reputation (`HandleFaction`).
    let before = value(&g, "Outlaws");
    g.app.world_mut().write_message(MonsterKilled {
        record: "records/creatures/enemies/humanoutlaw_melee_b02.dbr".into(),
        level: 37,
        position: Vec3::ZERO,
        entity: Entity::PLACEHOLDER,
        killer: None,
    });
    g.step();
    assert_eq!(value(&g, "Outlaws"), before);
}
