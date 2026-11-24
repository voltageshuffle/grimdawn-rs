//! Headless check of a gdx3 bounty's kill condition (`gd_story::story::Kill`, `gd_game::story::count_story_kills`):
//! `bq_av07` counts the player's Aetherial / Aetherial Vanguard kills (type 29), and that counter is in the save.
use bevy::prelude::*;
use gd_game::combat::MonsterKilled;
use gd_game::testing::TestGame;
use gd_game_core::story::StoryRuntime;
use gd_story::story::Kill;

/// The (quest, task, objective, condition) indices of the quest's first condition of `kind`.
fn kill_condition(g: &TestGame, quest: &str, kind: u32) -> (usize, usize, usize, usize) {
    let rt = g.app.world().non_send::<StoryRuntime>();
    let story = rt.story.borrow();
    let defs = &story.quests.defs;
    let qi = defs.by_path(quest).expect(quest);
    for (ti, t) in defs.list[qi].quest.tasks.iter().enumerate() {
        for (oi, o) in t.objectives.iter().enumerate() {
            for (ci, c) in o.conditions.iter().flat_map(|c| c.list.iter().enumerate()) {
                if c.kind == kind {
                    return (qi, ti, oi, ci);
                }
            }
        }
    }
    panic!("{quest}: no condition {kind}");
}

/// Begins the task (forced: whatever the save holds of it).
fn begin(g: &TestGame, (qi, ti, ..): (usize, usize, usize, usize)) {
    let rt = g.app.world().non_send::<StoryRuntime>();
    let mut story = rt.story.borrow_mut();
    let uid = story.quests.defs.list[qi].quest.tasks[ti].uid;
    story.begin_task(qi, uid, true);
}

fn counter(g: &TestGame, (qi, ti, oi, ci): (usize, usize, usize, usize)) -> u32 {
    let rt = g.app.world().non_send::<StoryRuntime>();
    rt.story.borrow().quests.rt[qi].tasks[ti].objectives[oi].counters[ci]
}

/// The first creature record (sorted) whose rule fields match.
fn creature(g: &TestGame, want: impl Fn(&Kill) -> bool) -> String {
    let db = &g.data().db;
    let mut names: Vec<&str> = db
        .names()
        .filter(|n| n.starts_with("records/creatures/enemies/"))
        .collect();
    names.sort();
    names
        .into_iter()
        .find(|n| want(&Kill::of_record(db, n)))
        .expect("a matching creature")
        .to_owned()
}

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

/// A `bq_av07` (type 29) counter is written to `quests.gdd` and read back by the next session.
#[test]
fn faction_kill_counter_survives_save_and_reload() {
    let Some(mut g) = TestGame::new_character("Bounty Hunter", false, false) else {
        return;
    };
    let player = g.spawn_player();
    g.steps(2);
    let c = kill_condition(&g, "quests/bountiesgdx3/bq_av07.qst", 29);
    begin(&g, c);
    let aetherial = creature(&g, |k| k.faction == Some(2) && k.give_xp);
    let outlaw = creature(&g, |k| k.faction == Some(4) && k.give_xp);
    for _ in 0..3 {
        kill(&mut g, &aetherial, Some(player));
    }
    kill(&mut g, &outlaw, Some(player));
    assert_eq!(counter(&g, c), 3);
    let written = g.save_now();
    let Some(mut again) = TestGame::with_save(&written) else {
        return;
    };
    again.spawn_player();
    again.steps(2);
    assert_eq!(counter(&again, c), 3, "the counter after reload");
}
