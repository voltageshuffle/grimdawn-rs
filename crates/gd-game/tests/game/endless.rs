//! Shattered Realm host (`gd_game::endless`): exit gates, buff shrines, floor progress, the boss and reloads.
use bevy::prelude::*;
use gd_game::endless::{EndlessGate, EndlessShrine};
use gd_game::player_orders::{InteractTarget, Interactable};
use gd_game::riftgates::GateTravel;
use gd_game::testing::TestGame;
use gd_game_core::story::StoryRuntime;
use gd_story::endless::{Dungeon, Entity as MapEntity, Floor, FloorState};

fn floor(region: usize, entrance: Option<[f32; 3]>) -> FloorState {
    FloorState {
        floor: Floor {
            record: String::new(),
            regions: Vec::new(),
            boss: false,
        },
        regions: vec![region],
        entrance: entrance.map(|pos| MapEntity {
            uid: [0; 4],
            region,
            pos,
        }),
        proxies: Vec::new(),
        progress: 0.0,
        total: 10.0,
    }
}

fn game() -> Option<TestGame> {
    TestGame::new_character("Tester", false, false)
}

/// A two-floor dungeon with the player standing in floor 0.
fn setup(g: &mut TestGame, id: u32, active: usize) {
    let rt = g.app.world().non_send::<StoryRuntime>();
    let mut story = rt.story.borrow_mut();
    story.endless.dungeon = Some(Dungeon {
        ruleset: String::new(),
        tier: 1,
        floors: vec![floor(10, None), floor(11, Some([500.0, 3.0, 40.0]))],
        active,
        id,
    });
    story.player.region = Some(10);
}

fn gates(g: &mut TestGame) -> Vec<(Entity, u32, usize)> {
    let w = g.app.world_mut();
    w.query::<(Entity, &EndlessGate)>()
        .iter(w)
        .map(|(e, g)| (e, g.stamp, g.floor))
        .collect()
}

#[test]
fn using_an_exit_gate_travels_to_the_active_entrance() {
    let Some(mut g) = game() else {
        return;
    };
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    setup(&mut g, 1, 1);
    g.steps(3);
    let (gate, ..) = gates(&mut g)[0];
    g.interact(gate);
    g.steps(1);
    let to: Vec<Vec3> = g
        .app
        .world()
        .resource::<Messages<GateTravel>>()
        .iter_current_update_messages()
        .map(|t| t.0)
        .collect();
    assert_eq!(to, vec![Vec3::new(500.0, 3.0, 40.0)]);
    let p = g.player();
    assert!(g.get::<InteractTarget>(p).0.is_none());
}

#[test]
fn a_shrine_is_used_once_and_gives_a_timed_buff() {
    use gd_game_core::skill_buffs::ReceivedBuffs;
    let Some(mut g) = game() else {
        return;
    };
    g.flat_ground(Vec3::ZERO, 24.0);
    let player = g.spawn_player();
    setup(&mut g, 1, 0);
    let shrine = use_buff_shrine(&mut g, player, 2);
    let buffs = g.get::<ReceivedBuffs>(player);
    assert_eq!(buffs.list.len(), 1);
    assert!(buffs.list[0].buff.contains("oleronbloodlust_buff"));
    assert!((buffs.list[0].left - 30.0).abs() < 1.0, "30 s");
    // A second click does nothing.
    g.interact(shrine);
    g.steps(2);
    assert_eq!(g.get::<ReceivedBuffs>(player).list.len(), 1);
}

/// Uses the buff shrine `n` (1..5) standing on the player.
fn use_buff_shrine(g: &mut TestGame, player: Entity, n: u32) -> Entity {
    let at = g.get::<Transform>(player).translation;
    let shrine = g
        .app
        .world_mut()
        .spawn((
            Transform::from_translation(at),
            GlobalTransform::from(Transform::from_translation(at)),
            EndlessShrine {
                record: format!("records/endlessdungeon/shrines/buffshrine_0{n}.dbr"),
                radius: 1.25,
                used: false,
                key: (0, n),
            },
            Interactable::fixed(1.25),
        ))
        .id();
    g.steps(1);
    g.interact(shrine);
    g.steps(3);
    assert!(g.get::<EndlessShrine>(shrine).used);
    shrine
}

/// `endless::progress`: a kill of an active-floor monster adds 1 to the floor's progress, counts in the kill chain
/// and a hero pays its bonus time; a monster outside the floor's proxies counts for nothing.
#[test]
fn kills_of_the_active_floor_add_progress_chain_and_bonus_time() {
    use gd_game::combat::MonsterKilled;
    const HERO: &str = "records/creatures/enemies/hero/zombie_h01.dbr";
    let Some(mut g) = game() else {
        return;
    };
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    let hero = g.spawn_monster(HERO, 60, Vec3::new(10.0, 0.0, 0.0));
    let zombie = g.spawn_monster(crate::common::ZOMBIE, 60, Vec3::new(-10.0, 0.0, 0.0));
    setup(&mut g, 1, 0);
    {
        let rt = g.app.world().non_send::<StoryRuntime>();
        let mut story = rt.story.borrow_mut();
        let d = story.endless.dungeon.as_mut().unwrap();
        d.ruleset = "records/endlessdungeon/rulesets/dungeonset01.dbr".into();
        d.floors[0].total = 1000.0;
        // Only the hero is a monster of the active floor (spawn_monster's slot is (MAX, 0, 0)).
        story.endless.activated.push((0, (usize::MAX, 0)));
    }
    let kill = |g: &mut TestGame, e, record: &str| {
        g.app.world_mut().write_message(MonsterKilled {
            record: record.into(),
            level: 60,
            position: Vec3::ZERO,
            entity: e,
            killer: None,
        });
    };
    // The generator starts the section's timer once the player stands in the floor.
    g.steps(2);
    let before = g
        .app
        .world()
        .non_send::<StoryRuntime>()
        .story
        .borrow()
        .game
        .survival
        .timer_ms;
    assert!(before > 0, "timer running");
    kill(&mut g, hero, HERO);
    g.steps(2);
    let (progress, chain, timer, souls) = {
        let rt = g.app.world().non_send::<StoryRuntime>();
        let story = rt.story.borrow();
        let e = &story.endless;
        (
            e.dungeon.as_ref().unwrap().floors[0].progress,
            e.chain.count,
            story.game.survival.timer_ms,
            e.soul_charges,
        )
    };
    // A hero always rolls an essence (heroChance 100): its progress comes on top of the kill's 1.
    assert!(
        progress >= 1.0 && souls > 0,
        "progress {progress}, souls {souls}"
    );
    // The essence buff's `achievementTracking`: one soul, the essence at the charge reached.
    let counts = g.app.world().resource::<gd_game::play_stats::PlayCounts>();
    assert_eq!(
        (counts.endless_souls, counts.endless_essence),
        (1, souls),
        "souls / essence counted"
    );
    assert_eq!(chain, 1, "one kill in the chain");
    assert!(
        (before + 4000 - 200..=before + 4000).contains(&timer),
        "hero bonus time 4 s (less the elapsed steps): {timer}"
    );
    // Kill outside the floor's proxies: nothing changes.
    {
        let rt = g.app.world().non_send::<StoryRuntime>();
        rt.story.borrow_mut().endless.activated.clear();
    }
    kill(&mut g, zombie, crate::common::ZOMBIE);
    g.steps(2);
    let rt = g.app.world().non_send::<StoryRuntime>();
    let story = rt.story.borrow();
    assert_eq!(story.endless.chain.count, 1, "not the floor's monster");
    assert_eq!(
        story.endless.dungeon.as_ref().unwrap().floors[0].progress,
        progress
    );
}

/// Save, quit and reload in the middle of a run: the save holds nothing of the dungeon, and the next session
/// starts with no generator, the tier counter at 0, no bonus timer and fresh proxy / shrine state.
#[test]
fn a_run_is_not_saved_and_a_reload_starts_without_a_dungeon() {
    use gd_game::monsters::ProxyStates;
    use gd_game::session;
    use gd_game_core::session::Choice;
    let Some(mut g) = game() else {
        return;
    };
    g.flat_ground(Vec3::ZERO, 24.0);
    let player = g.spawn_player();
    g.steps(2);
    setup(&mut g, 7, 0);
    {
        let rt = g.app.world().non_send::<StoryRuntime>();
        let mut story = rt.story.borrow_mut();
        story.endless.generated = 5;
        story.endless.next_id = 8;
        story.endless.soul_charges = 3;
        story.endless.timer_started = true;
        story.endless.boss_dead = true;
        story.game.survival.set_timer(120_000, false);
    }
    let shrine = use_buff_shrine(&mut g, player, 2);

    let written = g.save_now();
    let bytes = std::fs::read(&written).expect("player.gdc written");
    let saved = gd_formats::save::gdc::Gdc::parse(&bytes).unwrap();
    // The endless counters are the new character's: no essence buff landed (shrine buffs do not count).
    assert_eq!(
        (saved.stats.endless_souls, saved.stats.endless_essence),
        (0, 0)
    );

    let world = g.app.world_mut();
    world.entity_mut(shrine).despawn();
    world.entity_mut(player).despawn();
    session::end_session_state(world);
    world.remove_non_send::<StoryRuntime>();
    world
        .resource_mut::<gd_game_core::session::SessionEpoch>()
        .0 += 1;
    g.steps(3);
    let difficulty = saved.info.difficulty();
    let veteran = gd_game_core::session::is_veteran(&saved);
    let choice = Choice {
        save: Some(written),
        name: saved.header.name.clone(),
        female: saved.header.sex == 0,
        hardcore: false,
        survival: None,
        custom: None,
    };
    session::begin(
        &mut g.app.world_mut().commands(),
        choice,
        Some(saved),
        difficulty,
        veteran,
    );
    g.app.world_mut().flush();
    g.step();
    g.spawn_player();
    g.steps(5);

    assert!(g.app.world().resource::<ProxyStates>().0.is_empty());
    let rt = g.app.world().non_send::<StoryRuntime>();
    let story = rt.story.borrow();
    let e = &story.endless;
    assert!(e.dungeon.is_none(), "no generator after a reload");
    assert_eq!(e.generated, 0, "EnableGameEngine zeroes dungeonsGenerated");
    assert_eq!(e.soul_charges, 0);
    assert!(!e.timer_started && !e.boss_dead && e.activated.is_empty());
    assert_eq!(story.game.survival.timer_ms, 0, "no bonus timer");
}

/// The boss's death (`EndlessDungeon_Generator::Update`, HUD event 67 type 1): the bonus timer stops
/// (a reset of a running timer is no failure), and with the player in the dungeon tutorial page 0x42 unlocks (once).
/// A boss killed after the timer ran out: the reset counts a failure (the HUD's `timerFailedSound`).
#[test]
fn the_boss_death_stops_the_timer_and_unlocks_the_end_tutorial() {
    use gd_game::endless::BOSS_TUTORIAL_PAGE;
    use gd_game::tutorials::TutorialPages;
    use gd_story::endless::{ProxyInfo, ProxyKind};
    let Some(mut g) = game() else {
        return;
    };
    g.spawn_player();
    g.steps(1);
    let boss_floor = |g: &mut TestGame, timer: i32| {
        setup(g, 1, 0);
        let rt = g.app.world().non_send::<StoryRuntime>();
        let mut story = rt.story.borrow_mut();
        story.endless.boss_dead = false;
        story.endless.timer_started = true;
        let d = story.endless.dungeon.as_mut().unwrap();
        d.ruleset = "records/endlessdungeon/rulesets/dungeonset01.dbr".into();
        d.floors[0].floor.boss = true;
        d.floors[0].proxies = vec![ProxyInfo {
            kind: ProxyKind::Boss,
            entity: MapEntity {
                uid: [9; 4],
                region: 10,
                pos: [0.0; 3],
            },
            activated: true,
        }];
        story.game.survival.timer_ms = timer;
    };
    let state = |g: &TestGame| {
        let rt = g.app.world().non_send::<StoryRuntime>();
        let story = rt.story.borrow();
        let s = &story.game.survival;
        (story.endless.boss_dead, s.timer_ms, s.timer_failures)
    };
    let pages = |g: &TestGame| {
        let p = &g.app.world().resource::<TutorialPages>().pages;
        p.iter().filter(|&&p| p == BOSS_TUTORIAL_PAGE).count()
    };
    g.app
        .world_mut()
        .resource_mut::<TutorialPages>()
        .pages
        .clear();
    boss_floor(&mut g, 60_000);
    g.steps(2);
    assert_eq!(
        state(&g),
        (true, 0, 0),
        "boss dead in time: timer stopped, no failure"
    );
    assert_eq!(pages(&g), 1, "the end-of-Shard page unlocks");
    boss_floor(&mut g, 0);
    g.steps(2);
    assert_eq!(
        state(&g),
        (true, 0, 1),
        "boss dead after the timer ran out: a failure"
    );
    assert_eq!(pages(&g), 1, "unlocked once");
}
