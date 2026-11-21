//! Headless checks of the conversation window's rules
//! (`gd_game::conversation`): talking opens the NPC's conversation, answers
//! advance it, and walking away or `Close` ends it.
use bevy::prelude::*;
use gd_game::conversation::{Conversation, ConversationRequest};
use gd_game::story::{StoryActor, StoryRuntime};
use gd_game::testing::TestGame;
use gd_sim::story::state::{Coords, ObjClass, WorldObject};

const NPC: u32 = 900_100;

/// A test game with a story NPC 2 m from the player whose conversation has
/// something to say; returns the game and the NPC entity.
fn setup() -> Option<(TestGame, Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    g.steps(2);
    {
        let rt = g.app.world().non_send::<StoryRuntime>();
        let mut story = rt.story.borrow_mut();
        let mut convs: Vec<String> = story.dialogs.defs.keys().cloned().collect();
        convs.sort();
        let conv = convs
            .into_iter()
            .find(|c| story.has_speech(c))
            .expect("some conversation has something to say for a new character");
        story.entities.objects.insert(
            NPC,
            WorldObject {
                id: NPC,
                record: "records/creatures/npcs/merchants/merchanta01.dbr".into(),
                class: ObjClass::Npc,
                coords: Coords {
                    origin: [2.0, 0.0, 0.0],
                    axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                },
                in_world: true,
                alive: true,
                locked: false,
                open: false,
                conversation: Some(conv),
                available_for_conversation: true,
                scripted: false,
            },
        );
    }
    let npc = g
        .app
        .world_mut()
        .spawn((Transform::from_xyz(2.0, 0.0, 0.0), StoryActor(NPC)))
        .id();
    g.step();
    Some((g, npc))
}

fn view(g: &TestGame) -> Option<gd_sim::story::DialogView> {
    g.app.world().resource::<Conversation>().view.clone()
}

#[test]
fn talking_opens_and_walking_away_or_close_ends_it() {
    let Some((mut g, npc)) = setup() else { return };
    g.app
        .world_mut()
        .write_message(ConversationRequest::Talk(NPC));
    g.step();
    let v = view(&g).expect("the conversation opens");
    assert_eq!(v.npc, NPC);
    // The name line's faction icon: the merchant's `faction_survivors.dbr`.
    let icon = |g: &TestGame| {
        g.app
            .world()
            .resource::<Conversation>()
            .faction_icon
            .clone()
    };
    assert_eq!(
        icon(&g).as_deref(),
        Some("ui/character/faction_survivors.tex")
    );
    assert!(
        !v.responses.is_empty() || !v.on_last_page(),
        "something to click"
    );
    // 4 m away: still within the default `uiCloseDistance` (5 m).
    g.app
        .world_mut()
        .get_mut::<Transform>(npc)
        .unwrap()
        .translation
        .x = 4.0;
    g.step();
    assert!(view(&g).is_some(), "open within the close distance");
    g.app
        .world_mut()
        .get_mut::<Transform>(npc)
        .unwrap()
        .translation
        .x = 6.0;
    g.step();
    assert!(view(&g).is_none(), "walking away closes the window");
    assert_eq!(icon(&g), None, "no icon once closed");
    // Back in reach, `Close` ends it.
    g.app
        .world_mut()
        .get_mut::<Transform>(npc)
        .unwrap()
        .translation
        .x = 2.0;
    g.app
        .world_mut()
        .write_message(ConversationRequest::Talk(NPC));
    g.step();
    assert!(view(&g).is_some());
    g.app.world_mut().write_message(ConversationRequest::Close);
    g.step();
    assert!(view(&g).is_none());
    let rt = g.app.world().non_send::<StoryRuntime>();
    assert!(
        rt.story.borrow().dialogs.session.is_none(),
        "the story's session ended"
    );
}
