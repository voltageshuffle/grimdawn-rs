//! Story glue: the app side of the story runtime (`gd_game::story`, its world objects
//! `gd_game::story_world`). Talking to NPCs (clicks / the Interact key, requests to
//! `gd_game::conversation`) and the models of plain story objects
//! (`gd_game::visuals::DecorationRequest`, [`dress_story_objects`]). The conversation window is
//! `ui::dialog`, the quest tracker and journal `ui::quests`, the notifications `ui::notify`.
//!
//! Clicking a story NPC (or the Interact key) is the player's interact order (`gd_game::player_orders`), which
//! walks to it and talks ([`talk_input`]). Dev builds: `GD_STORY_AUTOTALK` plays conversations automatically
//! (first Accept, else first response), `GD_STORY_TALK` walks to talkable NPCs and opens them (`test_talk`).
use crate::data::GameData;
use bevy::prelude::*;

use gd_sim::story::Story;

pub struct StoryPlugin;

impl Plugin for StoryPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<DecorationRequest, Visibility>()
            .add_systems(
                Update,
                talk_input
                    .after(crate::player::PlayerSet)
                    .before(gd_game::conversation::ConversationStage::Run),
            )
            .add_systems(
                Update,
                dress_story_objects.after(gd_game::story_world::sync_actors),
            );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            test_talk.after(crate::player::PlayerSet).before(talk_input),
        );
    }
}

pub use gd_game::story::StoryEffect;
pub use gd_game::story::{StoryActor, StoryRuntime};
pub use gd_game::story_world::is_story_record;
use gd_game::visuals::DecorationRequest;

/// Plain story objects get their record's model, the mesh's `CreateEntity` effects and lights (Engine.dll
/// `Actor::CreateAttachmentsFromMesh`, every actor: the Devil's Crossing forge's flames and coal light, the cook's
/// campfire and lanterns are quest assets a token swap creates) and the record's animations (the repaired Devil's
/// Crossing windmill's spin). The model loads in the background (`CharAssets::model_ready`): an object whose model
/// is still loading waits ([`DecorationPending`]) and is dressed on a later frame.
fn dress_story_objects(
    mut commands: Commands,
    new: Query<
        (Entity, &DecorationRequest),
        Or<(Added<DecorationRequest>, With<DecorationPending>)>,
    >,
    mut assets: gd_render::character::CharAssets,
) {
    for (e, req) in &new {
        let model = match assets.model_ready(&req.mesh) {
            gd_render::model::Ready::Loading => {
                commands.entity(e).try_insert(DecorationPending);
                continue;
            }
            gd_render::model::Ready::Done(model) => model,
            gd_render::model::Ready::Failed => {
                commands.entity(e).try_remove::<DecorationPending>();
                continue;
            }
        };
        commands.entity(e).try_remove::<DecorationPending>();
        gd_render::model::spawn_model(&mut commands, e, &model);
        gd_render::fx::spawn_mesh_attachments(&mut commands, &assets.data, &model, e);
        if let Some(a) = assets
            .data
            .db
            .get(&req.record)
            .and_then(crate::decoration_anim::DecorationAnims::from_record)
        {
            commands.entity(e).insert(a);
        }
    }
}

/// A story object whose model is still loading ([`dress_story_objects`]).
#[derive(Component)]
struct DecorationPending;

/// Story NPCs the player can click to talk to, by id: Grim Dawn.exe mouse handler
/// takes any `Npc` under the cursor that `Npc::IsAvailableForConversations` (the story object's flag), and
/// `ControllerPlayerState::DefaultRequestNpcAction` → `TalkToNpc` → `Npc::OnPlayerInteract` → `Npc::Converse` needs
/// no speech: without one the NPC greets (`gd_game::trade::NpcSay::Greet`). A plain `Npc` record is clickable so;
/// the service classes (`NpcMerchant`, ...) open their service through `crate::trade` instead, and talk here only
/// with a speech (`NpcMerchant::Converse`), as do non-`Npc` story actors. PORT: `Npc::Converse`'s 1 s talk cooldown
/// (`+4608`, checked by `IsAvailableForConversations`) is not kept.
fn can_talk(db: &gd_formats::vfs::Database, story: &Story, id: u32) -> bool {
    story.entities.get(id).is_some_and(|o| {
        o.in_world
            && o.available_for_conversation
            && (has_speech(story, id) || db.get(&o.record).is_some_and(|r| r.class == "Npc"))
    })
}

/// The story NPC's conversation has an available speech (`Npc::Converse` opens the window).
fn has_speech(story: &Story, id: u32) -> bool {
    story
        .entities
        .get(id)
        .and_then(|o| o.conversation.as_ref())
        .is_some_and(|c| story.has_speech(c))
}

/// A story NPC's `Actor::GetRadius` (`gd_game::combat::get_radius`: `actorRadius` x the record's `scale` x
/// sqrt(3), the engine's default `actorRadius` when absent), the radius `NearTarget` and the walk use.
fn npc_radius(data: &GameData, story: &Story, id: u32) -> f32 {
    let rec = story.entities.get(id).and_then(|o| data.db.get(&o.record));
    gd_game::combat::get_radius(rec, gd_game::combat::ACTOR_RADIUS)
}

/// Talking: a click on a story NPC (or the Interact key) is the player's interact order
/// (`gd_game::player_orders::InteractTarget`), which walks to it; this talks when it is in talk range
/// ([`gd_game::conversation::in_talk_range`]) or the walk has ended (`DefaultRequestNpcAction` /
/// `MoveToNpc::EndOfPathReached`). Answers: the window (`ui::dialog`), keys `1`–`9`; Esc closes it via the UI
/// dispatcher.
fn talk_input(
    rt: Option<NonSend<StoryRuntime>>,
    keys: Res<ButtonInput<KeyCode>>,
    actors: Query<(Entity, &StoryActor, &GlobalTransform)>,
    conv: Res<gd_game::conversation::Conversation>,
    mut players: Query<
        (
            &Transform,
            &mut crate::player::MovePath,
            &crate::combat::Combatant,
            &mut gd_game::player_orders::InteractTarget,
        ),
        With<crate::player::Player>,
    >,
    data: Res<GameData>,
    mut out: MessageWriter<gd_game::conversation::ConversationRequest>,
) {
    use gd_game::conversation::{ConversationRequest as Req, in_talk_range};
    let Some(rt) = rt else { return };
    if let Some(view) = &conv.view {
        // A page before the last offers only the window's "Continue". PORT: the digit keys (1 continues, 1 to 9 pick
        // a response) are the port's; the conversation window's key handling is not decompiled.
        if !view.on_last_page() {
            if keys.just_pressed(KeyCode::Digit1) {
                out.write(Req::NextPage);
            }
            return;
        }
        const DIGITS: [KeyCode; 9] = [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
        ];
        if let Some(i) = DIGITS
            .iter()
            .position(|k| keys.just_pressed(*k))
            .filter(|&i| i < view.responses.len())
        {
            info!("story: > {}", view.responses[i].1);
            out.write(Req::Choose(view.responses[i].0));
        }
        return;
    }
    let Ok((ptf, mut path, cb, mut interact)) = players.single_mut() else {
        return;
    };
    // The interact order's NPC target: talk when in range or the walk ended; else the walk goes on
    // (`apply_orders` set the path). A target that is not a talkable NPC is another use system's (leave it).
    let Some((_, a, gt)) = interact
        .0
        .and_then(|e| actors.iter().find(|(ee, _, _)| *ee == e))
    else {
        return;
    };
    let story = rt.story.borrow();
    if !can_talk(&data.db, &story, a.0) {
        return;
    }
    let d = gt.translation().distance(ptf.translation);
    if in_talk_range(d, cb.radius, npc_radius(&data, &story, a.0)) || path.points.is_empty() {
        interact.0 = None;
        path.points.clear();
        out.write(Req::Talk(a.0));
    }
}

/// `GD_STORY_*` (hooks.rs), before [`talk_input`]: with `GD_STORY_AUTOTALK` the open window is answered (first
/// Accept, else the first response) and the nearest talkable NPC in talk range is talked to once; with
/// `GD_STORY_TALK` the player walks to the nearest talkable NPC within 40 m and talks (once to each NPC id whose
/// window opened; a swapped NPC is a new one); with a list of record texts, an NPC matching an earlier text first,
/// within 80 m (the Shattered Realm's post-boss NPC can stand ~50 m from the boss room's entrance).
#[cfg(feature = "dev")]
#[allow(clippy::too_many_arguments)]
fn test_talk(
    hooks: Res<gd_game::hooks::Hooks>,
    rt: Option<NonSend<StoryRuntime>>,
    actors: Query<(Entity, &StoryActor, &GlobalTransform)>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    (conv, mut autotalked, mut test_talk, time, mut fresh): (
        Res<gd_game::conversation::Conversation>,
        Local<std::collections::HashSet<u32>>,
        Local<(std::collections::HashSet<u32>, Option<(u32, f32)>)>,
        Res<Time>,
        gd_game::session::NewSession,
    ),
    mut players: Query<
        (
            &Transform,
            &mut crate::player::MovePath,
            &crate::combat::Combatant,
            &mut gd_game::player_orders::InteractTarget,
        ),
        With<crate::player::Player>,
    >,
    (data, nav): (Res<GameData>, Res<crate::world::Nav>),
    mut out: MessageWriter<gd_game::conversation::ConversationRequest>,
) {
    use gd_game::conversation::{ConversationRequest as Req, in_talk_range};
    if fresh.begun() {
        autotalked.clear();
        *test_talk = Default::default();
    }
    let auto = hooks.gameplay.story_autotalk;
    if !auto && !hooks.gameplay.story_talk {
        return;
    }
    let Some(rt) = rt else { return };
    if let Some(view) = &conv.view {
        // The test talk is done with an NPC once its window opened.
        test_talk.0.insert(view.npc);
        if !auto {
            return;
        }
        if !view.on_last_page() {
            out.write(Req::NextPage);
            return;
        }
        let pick = view
            .responses
            .iter()
            .position(|r| r.2 == "tagAccept")
            .or((!view.responses.is_empty()).then_some(0));
        if let Some(i) = pick {
            info!("story: > {}", view.responses[i].1);
            out.write(Req::Choose(view.responses[i].0));
        }
        return;
    }
    let Ok((ptf, mut path, cb, mut interact)) = players.single_mut() else {
        return;
    };
    // A pending interact order is `talk_input`'s.
    if interact.0.is_some() {
        return;
    }
    let story = rt.story.borrow();
    let in_range = |id: u32, gt: &GlobalTransform| {
        in_talk_range(
            gt.translation().distance(ptf.translation),
            cb.radius,
            npc_radius(&data, &story, id),
        )
    };
    let talk_range = if hooks.gameplay.story_talk_filter.is_some() {
        80.0
    } else {
        40.0
    };
    let talk_rank = |id: u32| match &hooks.gameplay.story_talk_filter {
        None => Some(0),
        Some(f) => story
            .entities
            .get(id)
            .and_then(|o| f.iter().position(|t| o.record.contains(t.as_str()))),
    };
    if hooks.gameplay.story_talk
        && let Some((e, a, gt)) = actors
            .iter()
            .filter(|(_, a, gt)| {
                !test_talk.0.contains(&a.0)
                    // A talk that opened nothing (the NPC was made untalkable that frame) is retried after 2 s.
                    && test_talk
                        .1
                        .is_none_or(|(id, t)| id != a.0 || time.elapsed_secs() - t > 2.0)
                    && can_talk(&data.db, &story, a.0)
                    && has_speech(&story, a.0)
                    && gt.translation().distance(ptf.translation) < talk_range
                    && talk_rank(a.0).is_some()
            })
            .min_by(|a, b| {
                talk_rank(a.1.0).cmp(&talk_rank(b.1.0)).then(
                    a.2.translation()
                        .distance(ptf.translation)
                        .total_cmp(&b.2.translation().distance(ptf.translation)),
                )
            })
    {
        test_talk.1 = Some((a.0, time.elapsed_secs()));
        info!(
            "story: test talk to {} at {:.1} m",
            a.0,
            gt.translation().distance(ptf.translation)
        );
        if in_range(a.0, gt) {
            out.write(Req::Talk(a.0));
        } else {
            interact.0 = Some(e);
            // No nav path: a straight move, not a talk from afar that `run_conversations` closes at once (PORT).
            path.points = gd_game::chests::path_near(
                &nav.0,
                &mut scratch,
                ptf.translation,
                gt.translation(),
                npc_radius(&data, &story, a.0) + cb.radius,
            )
            .unwrap_or_else(|| vec![gt.translation()]);
        }
        return;
    }
    if !auto {
        return;
    }
    let nearest = actors
        .iter()
        .filter(|(_, a, gt)| {
            can_talk(&data.db, &story, a.0) && has_speech(&story, a.0) && in_range(a.0, gt)
        })
        .min_by(|a, b| {
            a.2.translation()
                .distance(ptf.translation)
                .total_cmp(&b.2.translation().distance(ptf.translation))
        })
        .map(|(_, a, _)| a.0);
    if let Some(npc) = nearest
        && autotalked.insert(npc)
    {
        out.write(Req::Talk(npc));
    }
}
