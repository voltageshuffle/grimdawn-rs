//! The `Kill` achievement trigger unlocks the right achievement on the player's kill (and not the wrong ones).
use bevy::prelude::*;
use gd_game::achievements::{Achievements, groups};
use gd_game::combat::MonsterKilled;
use gd_game::testing::TestGame;

fn kill(g: &mut TestGame, record: &str, killer: Option<Entity>) {
    g.app.world_mut().write_message(MonsterKilled {
        record: record.into(),
        level: 50,
        position: Vec3::ZERO,
        entity: Entity::PLACEHOLDER,
        killer: killer.map(|k| (k, None)),
    });
    g.step();
}

fn unlocked(g: &TestGame) -> Vec<String> {
    g.app.world().resource::<Achievements>().unlocked.clone()
}

fn has(g: &TestGame, id: &str) -> bool {
    unlocked(g).iter().any(|u| u == id)
}

/// The first record of the achievement's `killDbrs`.
fn kill_record(g: &TestGame, id: &str) -> String {
    let db = g.data().db;
    groups(&db)
        .iter()
        .flat_map(|g| &g.achievements)
        .find(|a| a.id == id)
        .and_then(|a| a.kill_dbrs.first().cloned())
        .expect(id)
}

/// A `Kill` achievement: the player's kill of its record unlocks it; a kill without credit does not, and the
/// Ultimate-only and hardcore-only variants of the same kill stay locked on Normal softcore.
#[test]
fn kill_achievement_unlocks_on_the_players_kill() {
    let Some(mut g) = TestGame::new_character("AchKill", false, false) else {
        return;
    };
    let p = g.spawn_player();
    g.steps(2);
    let record = kill_record(&g, "ACH013");
    kill(&mut g, &record, None);
    assert!(!has(&g, "ACH013"), "a kill without the player's credit");
    kill(&mut g, &record, Some(p));
    assert!(has(&g, "ACH013"));
    assert!(!has(&g, "ACH014"), "the Ultimate variant");
    assert!(!has(&g, "ACH017"), "the hardcore variant");
    // A record no Kill achievement lists unlocks nothing.
    let before = unlocked(&g).len();
    kill(&mut g, "records/creatures/enemies/zombie_a01.dbr", Some(p));
    assert_eq!(unlocked(&g).len(), before);
}
