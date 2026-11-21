//! The marks floating over NPCs: green-ish exclamation (a quest to take),
//! gold / grey question marks (turn in / in progress), conversation gems
//! (something to say, new or heard), and the service icons of merchants,
//! blacksmiths, ... (the `punctuation` field of the NPC's record).
//!
//! * `Character::UpdatePunctuation` (Game.dll 0x180067b10): a
//!   character with a conversation asks `Conversation::GetPunctuation`
//!   ([`gd_sim::story::Story::conversation_punctuation`]) at most every 500 ms
//!   (its own timer, `Character::UpdateSelf` adds the frame time; it resets
//!   only when a conversation exists) and attaches the FX record
//!   [`Punctuation::record`]: 1 `npcexclaimationpoint01`, 2
//!   `npcquestionmark02` (grey texture), 4 `npcquestionmark01` (gold), 6
//!   `npcconversationmark02`. For 0 / 5 (nothing / a heard speech) the record's `punctuation`
//!   string (`Character::Load` field) wins when it is not empty; else a heard
//!   speech gets `npcconversationmark01` and nothing detaches the mark. A
//!   character without a conversation shows its `punctuation` record, or no
//!   mark. `NpcTransmuter::Load` always attaches `npcillusionistmark01`, and
//!   `Npc::SetBoatMasterPunctuation` `npctravelicon01` (no stock boat master
//!   is in the port's data path): PORT: the Illusionist's is just its record's
//!   `punctuation`, which the stock data has too.
//! * [`NpcMark`] is the derived value (the attached FX record, set only when it
//!   changes); the presentation (`grimdawn::npc_marks`) reads it.
//! * PORT: one shared 500 ms poll for all NPCs instead of each one's own
//!   timer; story NPCs ([`StoryActor`]) and placed level NPCs ([`LevelNpc`],
//!   no story hooks: only the `punctuation` record) are covered, not
//!   monsters.
use crate::character::Character;
use crate::data::GameData;
use crate::monsters::LevelNpc;
use crate::story::{StoryActor, StoryRuntime};
use bevy::prelude::*;
use gd_sim::story::Punctuation;

/// `Character::UpdateSelf`'s poll period for the mark.
pub const POLL_SECS: f32 = 0.5;

pub struct NpcMarksPlugin;

impl Plugin for NpcMarksPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            update_npc_marks.after(crate::story::StoryStage::Tick),
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

/// The mark record `UpdatePunctuation` picks: `p` is the conversation's
/// punctuation (`None` when the character has no conversation), `own` the
/// NPC record's `punctuation` field.
pub fn mark_record(p: Option<Punctuation>, own: &str) -> Option<String> {
    if let Some(r) = p.and_then(punctuation_record) {
        return Some(r.to_owned());
    }
    if !own.is_empty() {
        return Some(own.to_owned());
    }
    (p == Some(Punctuation::Speech)).then(|| "records/fx/ui/npcconversationmark01.dbr".to_owned())
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
            .and_then(|r| r.str("punctuation").map(str::to_owned))
            .unwrap_or_default();
        let p = object
            .and_then(|o| o.conversation.as_deref())
            .map(|c| story.conversation_punctuation(c));
        let want = mark_record(p, &own).map(NpcMark);
        if want.as_ref() != current {
            match want {
                Some(m) => commands.entity(e).try_insert(m),
                None => commands.entity(e).try_remove::<NpcMark>(),
            };
        }
    }
}
