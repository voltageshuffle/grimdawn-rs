//! Conversation window flow (`ConversationWindow::UpdateStep`): the first speech
//! whose conditions hold is shown, later ones with flag bit 1 are merged in
//! (actions run, responses appended, not recorded as used).
use gd_formats::cnv::{Conversation, Step, StepType};
use gd_formats::qst::{Action, Texts};
use gd_story::story::{Coords, Dialogs, ObjClass, Player, QuestDefs, Story, WorldObject};
use std::sync::Arc;

const NPC: u32 = 50;

fn step(
    index: u32,
    kind: StepType,
    raw_type: u32,
    flags: u32,
    money: u32,
    children: Vec<Step>,
) -> Step {
    let actions = if money > 0 {
        vec![Action::GiveMoney { amount: money }]
    } else {
        Vec::new()
    };
    Step {
        index,
        raw_type,
        kind,
        flags,
        link: None,
        voice: String::new(),
        conditions: None,
        actions,
        children,
    }
}

/// 0 root
///   1 speech A (+1)          → 2 Continue → 3 link to 6
///   4 speech B, merge (+10)  → 5 Accept
///   6 speech C (+1000)       → 7 Decline
///   8 speech D, merge (+10000) → 9 End
fn conversation() -> Conversation {
    use StepType::*;
    let mut link = step(3, Link, 7, 0, 0, vec![]);
    link.link = Some(6);
    let root = step(
        0,
        Root,
        0,
        0,
        0,
        vec![
            step(
                1,
                Speech,
                1,
                0,
                1,
                vec![step(2, Continue, 2, 0, 0, vec![link])],
            ),
            step(4, Speech, 1, 2, 10, vec![step(5, Accept, 3, 0, 0, vec![])]),
            step(
                6,
                Speech,
                1,
                0,
                1000,
                vec![step(7, Decline, 4, 0, 0, vec![])],
            ),
            step(8, Speech, 1, 2, 10000, vec![step(9, End, 6, 0, 0, vec![])]),
        ],
    );
    let texts = ["Npc", "A", "go on", "", "B", "yes", "C", "no", "D", "bye"]
        .map(String::from)
        .to_vec();
    Conversation {
        version: 6,
        header_unknown: 0,
        id: 77,
        root,
        texts: Texts {
            languages: vec![("EN".into(), texts)],
        },
        hash: [0; 16],
    }
}

fn story() -> Story {
    let dialogs = Dialogs::new(
        [("conversations/test.cnv".into(), Arc::new(conversation()))]
            .into_iter()
            .collect(),
    );
    let mut s = Story::new(
        Arc::new(QuestDefs::default()),
        dialogs,
        Player::new_character(1, "T"),
    );
    s.entities.insert(WorldObject {
        id: NPC,
        record: "records/test.dbr".into(),
        class: ObjClass::Npc,
        coords: Coords::at([0.0; 3]),
        in_world: true,
        alive: true,
        locked: false,
        open: false,
        broken: false,
        conversation: Some("conversations/test.cnv".into()),
        available_for_conversation: true,
        scripted: false,
    });
    s
}

#[test]
fn merged_speeches_run_actions_and_add_responses() {
    let mut s = story();
    let m0 = s.player.money;
    let v = s.talk(NPC).expect("speech");
    assert_eq!(v.text, "A");
    // A, then B and D (bit 1); C (no bit) is skipped.
    assert_eq!(s.player.money - m0, 1 + 10 + 10000);
    let r: Vec<_> = v.responses.iter().map(|r| (r.0, r.2)).collect();
    assert_eq!(
        r,
        [
            (2, "tagContinue"),
            (5, "tagAccept"),
            (9, "tagEndConversation")
        ]
    );
    // Only the shown speech is recorded.
    assert_eq!(s.dialogs.used[&77], [1]);

    // A link in speech position: its target's text, actions and responses,
    // recorded under the link's own index.
    let v = s.choose(2).expect("linked speech");
    assert_eq!(v.text, "C");
    assert_eq!(s.player.money - m0, 1 + 10 + 10000 + 1000);
    assert_eq!(v.responses.iter().map(|r| r.0).collect::<Vec<_>>(), [7]);
    assert_eq!(s.dialogs.used[&77], [1, 3]);

    // Accept/Decline/End close the window.
    assert!(s.choose(7).is_none());
    s.talk(NPC).unwrap();
    assert!(s.choose(5).is_none());
}
