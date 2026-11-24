//! Conversations with NPCs: opening, answering and closing the conversation
//! window. The story runtime ([`gd_game_core::story::StoryRuntime`], `gd_story::story`
//! dialogs) decides what is said; this module owns the open conversation
//! ([`Conversation`]) and applies the player's [`ConversationRequest`]s.
//! The game draws the window from [`Conversation::view`] and sends requests.
//!
//! * Talk range: `ControllerPlayerState::DefaultRequestNpcAction`
//!   (Game.dll): clicking an NPC talks at once when
//!   `Character::NearTarget(npc, 3.0)` holds (3D distance of the origins
//!   ≤ player radius + NPC radius + 3 m, [`in_talk_range`]); otherwise the
//!   player walks to it (`MoveToNpc`) and talks when the path ends
//!   (`ControllerPlayerStateMoveToNpc::EndOfPathReached` → `TalkToNpc`,
//!   whatever the distance then). `Npc::Converse` opens the window only when
//!   the conversation has an available speech.
//! * Closing: the window's update `Grim Dawn.exe` `ConversationWindow::Update`: it
//!   closes when the NPC or the player is gone, when the 3D distance between
//!   them exceeds the NPC's `GetUICloseDistance` (`uiCloseDistance` of its
//!   record, 5 m when missing or ≤ 0, Engine.dll `Actor::GetUICloseDistance`;
//!   [`close_distance`]), or after 150 s without input
//!   (`Engine::IsUserIdle(150000)`; PORT: not checked). The window's close
//!   slot (`ConversationWindow::OnCloseSlot`, used for Esc / the close button) and the "End
//!   Conversation" button close it without running actions
//!   ([`ConversationRequest::Close`]).
//! * Voice-overs: the window's step update `Grim Dawn.exe`
//!   `ConversationWindow::UpdateStep`: each speech shown (opening, a Continue / link reply) stops
//!   the previous speech's VO (`ConversationStep::StopVO`, the step kept at
//!   `ConversationWindow.currentStep`) and plays its own (`PlayVO`, [`DialogView::voice`]); the
//!   window's close (`ConversationWindow::Close`: Esc / close button / End, Accept /
//!   Decline / End replies and no speech left through `UpdateStep`, walking
//!   away / the NPC gone through the update `ConversationWindow::Update`) stops it. Nothing
//!   waits for the voice: the text and replies show at once, a page
//!   "Continue" neither restarts nor stops it, and the voice runs to its end
//!   while the window stays open. [`Conversation::line`] numbers the speeches
//!   shown; the game's audio plays [`DialogView::voice`] of each new line.
//! * PORT: the window's 0.5 s fade in / out (`ConversationWindow.fade`, `dt × 0.002`) is not
//!   modelled; it opens and closes at once.
use bevy::prelude::*;
use gd_game_core::data::GameData;
use gd_game_core::player::Player;
use gd_game_core::story::{StoryActor, StoryRuntime};
use gd_story::story::DialogView;

pub struct ConversationPlugin;

impl Plugin for ConversationPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ConversationRequest>()
            .init_resource::<Conversation>()
            .add_systems(
                Update,
                run_conversations
                    .in_set(ConversationStage::Run)
                    .after(gd_game_core::story::StoryStage::Tick),
            );
    }
}

/// The player's requests to the conversation window.
#[derive(Message, Clone, Debug, PartialEq)]
pub enum ConversationRequest {
    /// Talk to a story NPC (object id); opens its conversation when it has
    /// an available speech. Ignored while a conversation is open.
    Talk(u32),
    /// Pick a response by its step index ([`DialogView::responses`]);
    /// [`DialogView::END`] is the window's own "End Conversation".
    Choose(u32),
    /// The page "Continue" button (a speech longer than one page).
    NextPage,
    /// Close the window (Esc, its close button).
    Close,
}

/// The open conversation: what the window shows. `version` changes with
/// every change of `view` (the window rebuilds on it).
#[derive(Resource, Default)]
pub struct Conversation {
    pub view: Option<DialogView>,
    pub version: u64,
    /// The NPC's faction icon while the window is open
    /// ([`crate::factions::faction_icon`], taken when it opens:
    /// `ConversationWindow::Layout` reads the NPC's
    /// `GetVisibleFaction`).
    pub faction_icon: Option<String>,
    /// The NPC's conversation has a quest step the player's standing with
    /// the NPC's faction locks (`Story::locked_quest_faction`, taken when it
    /// opens: `ConversationWindow::Open`, PORT: the engine also re-runs it from two
    /// window virtuals, not decompiled): the window shows the locked-quests
    /// icon instead of the faction icon.
    pub locked_quests: bool,
    /// The faction icon's hover tooltip (taken when the window opens, PORT: the engine builds it on hover).
    pub faction_tip: Option<crate::factions::FactionTip>,
    /// Number of the speech shown: changes with every speech the window
    /// shows (`PlayVO`), not with its pages; the voice-over of `view` belongs
    /// to this line.
    pub line: u64,
    /// The NPC's [`close_distance`], taken when the window opens.
    close_distance: f32,
}

impl Conversation {
    fn set(&mut self, view: Option<DialogView>) {
        if view.is_none() {
            self.faction_icon = None;
            self.locked_quests = false;
            self.faction_tip = None;
        }
        self.line += 1;
        self.view = view;
        self.version += 1;
    }
}

/// `NearTarget(npc, 3.0)`: the NPC is close enough to talk without walking.
pub fn in_talk_range(distance: f32, player_radius: f32, npc_radius: f32) -> bool {
    distance <= player_radius + npc_radius + 3.0
}

/// `Actor::GetUICloseDistance` of an NPC record.
pub fn close_distance(rec: Option<&gd_formats::arz::Record>) -> f32 {
    rec.and_then(|r| r.f32("uiCloseDistance"))
        .filter(|d| *d > 0.0)
        .unwrap_or(5.0)
}

/// Applies the requests, then closes the window when the player walked away
/// from the NPC or the NPC left the world.
#[allow(clippy::too_many_arguments)]
pub fn run_conversations(
    rt: Option<NonSend<StoryRuntime>>,
    mut requests: MessageReader<ConversationRequest>,
    mut conv: ResMut<Conversation>,
    players: Query<&Transform, With<Player>>,
    actors: Query<(&StoryActor, &Transform)>,
    data: Res<GameData>,
    factions: Option<Res<crate::factions::PlayerFactions>>,
    mut says: MessageWriter<crate::trade::NpcSay>,
    ids: Query<(Entity, &StoryActor)>,
) {
    let Some(rt) = rt else {
        requests.clear();
        return;
    };
    for r in requests.read() {
        match (r, conv.view.clone()) {
            (ConversationRequest::Talk(npc), None) => {
                let v = rt.host.talk(*npc);
                // `Npc::Converse` with no speech to give: the NPC's own greeting (`crate::trade::NpcSay::Greet`).
                if v.is_none()
                    && let Some((e, _)) = ids.iter().find(|(_, a)| a.0 == *npc)
                {
                    says.write(crate::trade::NpcSay::Greet(e));
                }
                if let Some(v) = &v {
                    info!("conversation: {}: {}", v.speaker, v.text);
                }
                let record = rt
                    .story
                    .borrow()
                    .entities
                    .get(*npc)
                    .map(|o| o.record.clone())
                    .unwrap_or_default();
                conv.set(v);
                conv.close_distance = close_distance(data.db.get(&record));
                conv.faction_icon = conv
                    .view
                    .as_ref()
                    .and_then(|_| crate::factions::faction_icon(&data.db, &record));
                let lock = crate::factions::my_faction(&data.db, &record)
                    .and_then(|f| rt.story.borrow().locked_quest_faction(*npc, &f));
                conv.locked_quests = conv.view.is_some() && lock.is_some();
                conv.faction_tip = conv.view.as_ref().and_then(|_| {
                    let f = factions.as_ref()?;
                    let i = crate::factions::visible_faction(&data.db, &record)?;
                    let gf = data.db.get(gd_rules::faction::GAME_FACTIONS)?;
                    crate::factions::faction_tip(gf, i, f.value(i), lock.map(|(_, d)| d))
                });
            }
            (ConversationRequest::NextPage, Some(mut v)) => {
                if v.next_page() {
                    conv.view = Some(v);
                    conv.version += 1;
                }
            }
            (ConversationRequest::Choose(step), Some(v))
                if v.on_last_page() && v.responses.iter().any(|r| r.0 == *step) =>
            {
                let next = rt.host.choose(*step);
                if let Some(n) = &next {
                    info!("conversation: {}: {}", n.speaker, n.text);
                }
                conv.set(next);
            }
            (ConversationRequest::Close, Some(_)) => {
                rt.host.choose(DialogView::END);
                conv.set(None);
            }
            _ => {}
        }
    }
    let Some(npc) = conv.view.as_ref().map(|v| v.npc) else {
        return;
    };
    let in_world = rt
        .story
        .borrow()
        .entities
        .get(npc)
        .is_some_and(|o| o.in_world && o.alive);
    let near = players
        .single()
        .ok()
        .zip(actors.iter().find(|(a, _)| a.0 == npc))
        .is_some_and(|(p, (_, a))| p.translation.distance(a.translation) <= conv.close_distance);
    if !in_world || !near {
        debug!("conversation: closed (npc in world {in_world}, near {near})");
        rt.host.choose(DialogView::END);
        conv.set(None);
    }
}

/// Conversations run: what shows or answers them orders around `Run`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversationStage {
    Run,
}
