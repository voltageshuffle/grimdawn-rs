//! The marks floating over NPCs: quest exclamation / question marks, conversation gems and service icons
//! (the `punctuation` field of the NPC's record).
//!
//! * `Character::UpdatePunctuation`: a character with a conversation asks `Conversation::GetPunctuation` at
//!   most every 500 ms and attaches [`punctuation_record`]. For 0 / 5 (nothing / a heard speech) the record's
//!   `punctuation` wins when not empty; else a heard speech gets `npcconversationmark01` and nothing detaches
//!   the mark. A character without a conversation shows its `punctuation` record, or no mark.
//!   PORT: the Illusionist's mark is its record's `punctuation`, not `NpcTransmuter::Load`'s
//!   `npcillusionistmark01`; `Npc::SetBoatMasterPunctuation` is not ported.
//! * PORT: one shared 500 ms poll for all NPCs instead of each one's own timer; story NPCs ([`StoryActor`])
//!   and placed level NPCs ([`LevelNpc`]) are covered, not monsters.
use crate::monsters::LevelNpc;
use bevy::prelude::*;
use gd_game_core::character::Character;
use gd_game_core::data::GameData;
use gd_game_core::story::{StoryActor, StoryRuntime};
use gd_story::story::Punctuation;

pub const POLL_SECS: f32 = 0.5;

pub struct NpcMarksPlugin;

impl Plugin for NpcMarksPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            update_npc_marks.after(gd_game_core::story::StoryStage::Tick),
        );
    }
}

/// The FX record attached over this NPC (`Entity::AttachPunctuation`).
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct NpcMark(pub String);

/// The FX record each conversation mark attaches (`UpdatePunctuation`).
pub fn punctuation_record(p: Punctuation) -> Option<&'static str> {
    match p {
        Punctuation::None | Punctuation::Speech => None,
        Punctuation::Exclamation => Some("records/fx/ui/npcexclaimationpoint01.dbr"),
        Punctuation::QuestionGray => Some("records/fx/ui/npcquestionmark02.dbr"),
        Punctuation::QuestionGold => Some("records/fx/ui/npcquestionmark01.dbr"),
        Punctuation::NewSpeech => Some("records/fx/ui/npcconversationmark02.dbr"),
    }
}

/// The mark record `UpdatePunctuation` picks: `p` is the conversation's punctuation (`None` without a
/// conversation), `own` the record's `punctuation`.
pub fn mark_record(p: Option<Punctuation>, own: &str) -> Option<&str> {
    if let Some(r) = p.and_then(punctuation_record) {
        return Some(r);
    }
    if !own.is_empty() {
        return Some(own);
    }
    (p == Some(Punctuation::Speech)).then_some("records/fx/ui/npcconversationmark01.dbr")
}

#[allow(clippy::too_many_arguments)]
pub fn update_npc_marks(
    mut commands: Commands,
    time: Res<Time>,
    mut acc: Local<Option<f32>>,
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<GameData>,
    npcs: Query<
        (Entity, &Character, Option<&StoryActor>, Option<&NpcMark>),
        Or<(With<StoryActor>, With<LevelNpc>)>,
    >,
) {
    let Some(rt) = rt else { return };
    let t = acc.get_or_insert(POLL_SECS);
    *t += time.delta_secs();
    if *t < POLL_SECS {
        return;
    }
    *t = 0.0;
    let story = rt.story.borrow();
    for (e, ch, actor, current) in &npcs {
        let object = actor.and_then(|a| story.entities.get(a.0));
        let record = object.map_or(ch.record.as_str(), |o| o.record.as_str());
        let own = data
            .db
            .get(record)
            .and_then(|r| r.str("punctuation"))
            .unwrap_or("");
        let p = object
            .and_then(|o| o.conversation.as_deref())
            .map(|c| story.conversation_punctuation(c));
        let want = mark_record(p, own);
        if want != current.map(|m| m.0.as_str()) {
            match want {
                Some(m) => commands.entity(e).try_insert(NpcMark(m.to_owned())),
                None => commands.entity(e).try_remove::<NpcMark>(),
            };
        }
    }
}
