//! A failed event and its restart at a checkpoint (`eventRestart`): `events/survivalevent.lua`
//! (`SurvivalEvent_Update`'s failure branch) and `survival/eventcontrol.lua` (`restartEvent`), run on arena A's
//! script objects.
use super::*;
use gd_sim::story::Effect;
use gd_sim::story::state::Survival;

/// The story effects the host forwarded (`StoryEffect` messages), collected by the test.
#[derive(Resource, Default)]
struct Seen(Vec<Effect>);

fn collect(mut seen: ResMut<Seen>, mut ev: MessageReader<gd_game::story::StoryEffect>) {
    seen.0.extend(ev.read().map(|e| e.0.clone()));
}

fn lua(g: &TestGame, code: &str) {
    let rt = g.app.world().non_send::<StoryRuntime>();
    rt.host.lua.load(code).exec().expect("lua");
}

fn survival(g: &TestGame) -> Survival {
    let rt = g.app.world().non_send::<StoryRuntime>();
    rt.story.borrow().game.survival.clone()
}

/// The event NPC in the world (`npc_event_01` / `02` / `03`): its body (the story's,
/// `gd_game::story_world::sync_actors`) is moved next to the player to talk to.
fn event_npc(g: &mut TestGame) -> (u32, String) {
    let (id, rec) = {
        let rt = g.app.world().non_send::<StoryRuntime>();
        let s = rt.story.borrow();
        let o = s
            .entities
            .objects
            .values()
            .find(|o| o.record.contains("/npc_event_") && o.in_world && o.alive)
            .expect("an event NPC");
        (o.id, o.record.clone())
    };
    // A body that appeared this update spawns on the next; its mesh is read in the background, so under load
    // (the whole suite in parallel) it may take a few more frames: wait for it, giving the reader time.
    for i in 0..60 {
        let world = g.app.world_mut();
        let mut moved = false;
        for (a, mut tf) in world
            .query::<(&StoryActor, &mut Transform)>()
            .iter_mut(world)
        {
            if a.0 == id {
                tf.translation = Vec3::new(3.0, 0.0, 0.0);
                moved = true;
            }
        }
        if moved {
            return (id, rec);
        }
        if i >= 2 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        g.step();
    }
    panic!("the event NPC {rec} has no body");
}

/// Talks to the event NPC and clicks the rows containing each of `rows` in turn.
fn talk(g: &mut TestGame, rows: &[&str]) {
    let (npc, _) = event_npc(g);
    g.app
        .world_mut()
        .write_message(ConversationRequest::Talk(npc));
    g.step();
    for want in rows {
        let v = g.app.world().resource::<Conversation>().view.clone();
        let v = v.expect("the conversation is open");
        let row = v
            .responses
            .iter()
            .find(|r| r.1.contains(want))
            .unwrap_or_else(|| panic!("no row {want:?} in {:?}", v.responses))
            .0;
        g.app
            .world_mut()
            .write_message(ConversationRequest::Choose(row));
        g.steps(3);
    }
}

/// Wave 51 (the Wave 50 start), the player dies: respawn is held while the wave runs, the event's next update
/// fails it (`Game.PlayersDead`): the failure sound, the compensation chests, the end NPC (`npc_event_03`), the
/// score saved, the timer reset, respawn enabled (the player comes back) with `KillAllMonstersOnNextRespawn`.
/// Then "Restart at Checkpoint" (5 tributes, the first of three): `restartEvent` restarts at
/// `floor(wave / 10) * 10 - 20` (wave 30, the ramp too), one restart counted, the start NPC back, the player
/// teleported to the spawn point; the next event start runs tier 4 from wave 31.
#[test]
fn a_failed_event_restarts_at_the_checkpoint() {
    let Some((mut g, p, _)) = setup() else {
        return;
    };
    g.app.init_resource::<Seen>().add_systems(Update, collect);
    let objs = super::arenas::arena_objects(&g, ARENA);
    {
        let rt = g.app.world().non_send::<StoryRuntime>();
        for (id, rec, c) in &objs {
            rt.host.add_object(*id, rec, *c);
        }
    }
    g.steps(2);
    lua(&g, "gd.survival.eventControl.startTier05Event()");
    g.steps(2);
    let s = survival(&g);
    assert_eq!((s.wave_tier, s.difficulty_ramp), (51, 51));
    assert!(
        s.event_in_progress && !s.respawn,
        "the wave runs, respawn disabled"
    );
    assert_eq!(
        event_npc(&mut g).1,
        "records/creatures/npcs/npc_event_02.dbr"
    );
    g.app
        .world_mut()
        .entity_mut(p)
        .insert(gd_game::combat::Dead { timer: 4.0 });
    // Dead past the corpse time: held down (`RespawnHeld`), until the event's next update (once a second).
    for _ in 0..90 {
        g.step();
        let rt = g.app.world().non_send::<StoryRuntime>();
        let s = rt.story.borrow();
        if !s.game.survival.event_in_progress {
            break;
        }
        drop(s);
        assert!(
            g.app.world().get::<gd_game::combat::Dead>(p).is_some(),
            "held dead"
        );
    }
    g.steps(3);
    let s = survival(&g);
    assert!(!s.event_in_progress, "the event failed");
    assert!(
        s.respawn && s.score_locked,
        "respawn back on, the score saved"
    );
    assert_eq!(s.timer_ms, 0);
    assert!(
        g.app.world().get::<gd_game::combat::Dead>(p).is_none(),
        "the player is back"
    );
    assert_eq!(
        event_npc(&mut g).1,
        "records/creatures/npcs/npc_event_03.dbr"
    );
    let seen = std::mem::take(&mut g.app.world_mut().resource_mut::<Seen>().0);
    let sound = |e: &Effect, r: &str| matches!(e, Effect::PlaySound { record, .. } if record == r);
    assert!(
        seen.iter()
            .any(|e| sound(e, "records/sounds/spak_eventfail.dbr")),
        "{seen:?}"
    );
    assert!(seen.iter().any(|e| matches!(e, Effect::SaveSurvivalScore)));
    assert!(
        seen.iter().any(|e| matches!(e, Effect::KillAllMonsters)),
        "killed on the respawn"
    );

    let tribute_before = tribute(&g);
    talk(
        &mut g,
        &[
            "take on the Crucible again",
            "Restart at Checkpoint (Spend 5 Tributes)",
        ],
    );
    g.steps(3);
    let seen = std::mem::take(&mut g.app.world_mut().resource_mut::<Seen>().0);
    assert!(
        seen.iter()
            .any(|e| matches!(e, Effect::TeleportPlayer { .. })),
        "to the spawn point"
    );
    assert!(
        seen.iter().any(|e| matches!(e, Effect::SurvivalAutosave)),
        "`ResetSurvivalEvent` saves"
    );
    let s = survival(&g);
    assert_eq!(s.restarts, 1);
    assert_eq!(
        (s.wave_tier, s.difficulty_ramp),
        (30, 30),
        "restart at wave 30"
    );
    assert_eq!(tribute(&g), tribute_before - 5);
    assert_eq!(
        event_npc(&mut g).1,
        "records/creatures/npcs/npc_event_01.dbr"
    );
    lua(&g, "gd.survival.eventControl.startSurvivalModeEvent()");
    g.steps(2);
    let s = survival(&g);
    assert_eq!(s.wave_tier, 31, "tier 4's first wave");
}
