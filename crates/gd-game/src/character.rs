//! The character animation systems: the single-tick action pipeline
//! (advance → finish → drive), the idle controllers and the background clip
//! loads (the character data is [`gd_game_core::character`]).
use crate::constitution::Constitution;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use gd_formats::anm::Animation;
use gd_game_core::character::{
    AnimCache, AnimCallback, CharState, Character, CharacterStage, HeldAnims, IdleMode,
    LONG_IDLE_AFTER_MS,
};
use gd_game_core::data::GameData;
use gd_sim::anim::{self, AnimType, SpeedKind};

pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AnimCallback>()
            .init_resource::<AnimCache>()
            .add_systems(First, collect_anim_loads)
            // The single-tick action pipeline (module doc): the animations advance and
            // dispatch their callbacks before the frame's gameplay readers, so a hit point
            // crossed this frame lands its damage this frame (the engine's inline dispatch).
            .add_systems(
                Update,
                (advance_animations, finish_actions)
                    .chain()
                    .in_set(CharacterStage::Animate)
                    .before(gd_game_core::player::PlayerSet)
                    .before(crate::movement::crowd_begin)
                    .before(crate::story::fire_npc_casts)
                    .before(crate::story_world::record_hooks),
            )
            .add_systems(
                Update,
                (
                    start_hooks.run_if(|h: Res<gd_game_core::hooks::Hooks>| {
                        h.render.char_state.is_some() || h.render.char_hold.is_some()
                    }),
                    player_long_idle,
                    npc_chat.run_if(resource_exists::<crate::conversation::Conversation>),
                    crate::npc_controller::attach_npc_controllers,
                    crate::npc_controller::npc_idle,
                    drive_characters,
                    preload_anims,
                )
                    .chain()
                    .in_set(CharacterStage::Drive)
                    .after(gd_game_core::combat::CombatSet),
            )
            .add_systems(
                PostUpdate,
                crate::turning::turn_characters
                    .in_set(CharacterStage::Turn)
                    .before(TransformSystems::Propagate),
            )
            // `Animate` dispatches in Update; its PostUpdate readers (audio's
            // `character_callbacks`) order after the set, which crosses schedules and
            // would otherwise drop them before propagation: keep the set after it, so
            // they read propagated transforms as before.
            .configure_sets(
                PostUpdate,
                CharacterStage::Animate.after(TransformSystems::Propagate),
            );
    }
}

/// Files the clips [`AnimCache::ready`] parsed in the background, kept warm until their creatures hold them.
fn collect_anim_loads(mut cache: ResMut<AnimCache>) {
    use bevy::tasks::{block_on, futures_lite::future};
    if cache.loading.is_empty() {
        return;
    }
    let cache = &mut *cache;
    let mut i = 0;
    while i < cache.loading.len() {
        let Some((key, a)) = block_on(future::poll_once(&mut cache.loading[i])) else {
            i += 1;
            continue;
        };
        drop(cache.loading.swap_remove(i));
        cache.in_flight.remove(&key);
        let mut store = cache.anims.lock();
        if store.get(&key).is_none() {
            store.put(&key, a);
        }
    }
}

/// Parses a new character's whole clip set in the background and hands it the clips ([`HeldAnims`]), so its
/// attack / hit / death clips are parsed before it first plays them instead of on the game thread mid-fight (PORT:
/// the port's analogue of the engine's background `ResourceLoader`, as `SoundPak::PreLoad` is for sounds). A clip
/// played before its set lands is parsed on the spot, as before.
fn preload_anims(
    mut commands: Commands,
    data: Res<GameData>,
    mut cache: ResMut<AnimCache>,
    new: Query<(Entity, &Character), Added<Character>>,
) {
    use bevy::tasks::{AsyncComputeTaskPool, block_on, futures_lite::future};
    let cache = &mut *cache;
    for (e, c) in &new {
        let mut keys: Vec<String> = c
            .anims
            .slots
            .iter()
            .chain(c.anims.specials.values())
            .flat_map(|s| &s.entries)
            .map(|a| gd_formats::vfs::normalized(&a.path).into_owned())
            .collect();
        keys.sort_unstable();
        keys.dedup();
        let mut held = Vec::with_capacity(keys.len());
        {
            let mut store = cache.anims.lock();
            keys.retain(|k| match store.get_kept(k) {
                Some(Some(a)) => {
                    held.push(a);
                    false
                }
                Some(None) => false,
                None => true,
            });
        }
        if keys.is_empty() {
            commands.entity(e).try_insert(HeldAnims { _clips: held });
            continue;
        }
        let data = data.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            keys.into_iter()
                .map(|k| {
                    let a = data
                        .vfs
                        .read(&k)
                        .and_then(|b| Animation::parse(&b))
                        .inspect_err(|err| warn!("animation {k}: {err:#}"))
                        .ok();
                    (k, a)
                })
                .collect()
        });
        cache.preloads.push((e, held, task));
    }
    let mut i = 0;
    while i < cache.preloads.len() {
        let Some(parsed) = block_on(future::poll_once(&mut cache.preloads[i].2)) else {
            i += 1;
            continue;
        };
        let (e, mut held, _) = cache.preloads.swap_remove(i);
        let mut store = cache.anims.lock();
        for (k, a) in parsed {
            // A clip the character played meanwhile was parsed on the spot: keep that one.
            match store.get_kept(&k) {
                Some(Some(a)) => held.push(a),
                Some(None) => {}
                None => held.extend(store.put(&k, a)),
            }
        }
        drop(store);
        commands.entity(e).try_insert(HeldAnims { _clips: held });
    }
}

/// The start state of a new character ([`RenderHooks::char_state`]) and its attack hold (`GD_CHAR_HOLD`):
/// applied to every character the frame it appears, before it is driven.
fn start_hooks(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut new: Query<&mut Character, Added<Character>>,
) {
    for mut c in &mut new {
        c.state = match hooks.render.char_state.as_deref() {
            Some("run") => CharState::Run,
            Some("attack") => CharState::Attack,
            Some("death") => CharState::Death,
            _ => CharState::Idle,
        };
        c.hold = hooks.render.char_hold;
    }
}

/// Advances every character's animation by the frame time (scaled by
/// slows, held by `GD_CHAR_HOLD`): stores the pose and its bone frames
/// ([`Character::posed`]), the world-space root-motion velocity, and sends the
/// callback points crossed — in `Update`, before the gameplay readers, so a hit
/// point crossed this frame lands its damage this frame (the engine emits and
/// dispatches callbacks inside the advance, `AnimChannel::Update` →
/// `SkillManager::HandleSkillAnimationCallback`). Also arms
/// [`Character::allow_interrupt`] at the clip's `AllowInterrupt` point, ends an
/// evade's dash at `StopJump`, counts the attack-delay down, and stores the
/// crossed entity events ([`Character::entity_events`]) for gd-render.
/// A clip that has played out (`AnimChannel::settled`: corpses, finished
/// one-shots) keeps its pose and is not sampled again.
/// Characters advance in parallel (each touches only its own state and RNG); the
/// callbacks then go out in query order, as one loop over the characters sends them.
pub fn advance_animations(
    time: Res<Time>,
    mut chars: Query<(Entity, &Transform, &mut Character)>,
    mut out: MessageWriter<AnimCallback>,
) {
    let dt_ms = time.delta_secs() * 1000.0;
    chars.par_iter_mut().for_each(|(_, root_tf, mut c)| {
        // `ControllerPlayer::Update` counts the attack-delay down with the frame's ms.
        c.attack_delay = (c.attack_delay - dt_ms).max(0.0);
        if c.channel.settled() {
            if c.root_velocity != Vec3::ZERO {
                c.root_velocity = Vec3::ZERO;
            }
            return;
        }
        let c = &mut *c;
        let mut dt = dt_ms;
        if let (Some(h), CharState::Attack, Some(cur)) = (c.hold, c.state, &c.channel.current) {
            let len_ms = cur.anim.num_frames.saturating_sub(1) as f32 * 1000.0
                / cur.anim.frame_rate.max(1) as f32;
            if cur.time_ms >= h * len_ms {
                dt = 0.0;
            }
        }
        let k = c.time_scale();
        let ev = c.channel.update(dt * k, &c.body, &mut c.pose);
        c.repose();
        // Characters are root entities: the local transform is the current one
        // (propagation runs after the gameplay).
        let root = root_tf.compute_affine();
        let v = anim::root_velocity_object(&c.body, ev.root_velocity, 1.0);
        c.root_velocity = root.transform_vector3(Vec3::from(v)) * k;
        for name in &ev.callbacks {
            match name.as_str() {
                "RightHandHit" | "LeftHandHit" => {
                    debug!("{}: {name} at frame {:.1}", c.record, c.channel.frame());
                }
                // `AttackAction::AnimationCallback` sets the flag the
                // action matrix's permission query reads.
                "AllowInterrupt" => c.allow_interrupt = true,
                // The evade's dash over: the recovery is cancelable
                // (`ControllerPlayerStateJumpToUseSkill::RequestSkillAction` refuses only the warm-up).
                "StopJump" if c.playing == AnimType::Evade => {
                    c.evade_warmup = false;
                    c.allow_interrupt = true;
                }
                _ => {}
            }
            c.pending.push((c.playing, name.clone()));
        }
        c.crossed = ev.callbacks;
        c.entity_events = ev.entities;
    });
    for (entity, _, mut c) in &mut chars {
        if c.crossed.is_empty() {
            continue;
        }
        let c = &mut *c;
        c.sent_play = c.plays;
        let cast = c.state == CharState::Cast;
        for name in c.crossed.drain(..) {
            out.write(AnimCallback {
                entity,
                anim: c.playing,
                name,
                cast,
            });
        }
    }
}

/// `ControllerPlayerStateIdle::OnUpdate` / `ControllerPlayerStateLongIdle::Attacked` (module doc): the player turns
/// to the long idle after [`LONG_IDLE_AFTER_MS`] idling out of combat, and back to the combat idle when it fights.
pub fn player_long_idle(
    time: Res<Time>,
    mut players: Query<(&mut Character, Option<&Constitution>), With<gd_game_core::player::Player>>,
) {
    let dt = time.delta_secs() * 1000.0;
    for (mut c, con) in &mut players {
        let calm = con.is_none_or(Constitution::out_of_combat);
        if c.state != CharState::Idle || !calm {
            c.idle_ms = 0.0;
            if c.idle == IdleMode::Long {
                c.idle = IdleMode::Combat;
            }
            continue;
        }
        c.idle_ms += dt;
        if c.idle == IdleMode::Combat && c.idle_ms >= LONG_IDLE_AFTER_MS {
            c.idle = IdleMode::Long;
        }
    }
}

/// The NPC whose conversation is open plays the chat idle (module doc); at its start it turns to the player
/// (`ChatAction::FaceTarget`: yaw only, when they are at least 1 m apart and the record's
/// `rotateWhenChatting`, default on, allows it).
pub fn npc_chat(
    conv: Res<crate::conversation::Conversation>,
    data: Res<GameData>,
    players: Query<&Transform, With<gd_game_core::player::Player>>,
    mut npcs: Query<
        (&gd_game_core::story::StoryActor, &mut Character, &Transform),
        Without<gd_game_core::player::Player>,
    >,
) {
    let talking = conv.view.as_ref().map(|v| v.npc);
    for (actor, mut c, tf) in &mut npcs {
        let chat = talking == Some(actor.0);
        if !chat {
            if c.idle == IdleMode::Chat {
                c.idle = IdleMode::Combat;
            }
            continue;
        }
        if c.idle == IdleMode::Chat || c.state != CharState::Idle {
            continue;
        }
        c.idle = IdleMode::Chat;
        debug!("{}: chat idle", c.record);
        let turns = data
            .db
            .get(&c.record)
            .and_then(|r| r.f32("rotateWhenChatting"))
            .is_none_or(|v| v != 0.0);
        if let (true, Ok(p)) = (turns, players.single()) {
            let to = (p.translation - tf.translation).with_y(0.0);
            if to.length() >= 1.0 {
                c.turn.face_now(to);
            }
        }
    }
}

/// Dispatches the animations' "End" callbacks: finishes the ended actions before
/// the frame's requesters run — the engine's `CharacterHandlerUpdate`
/// shape (finish ended actions → run pending → animate), so a swing that ends this
/// frame chains into the next in the same pass, with no rendered idle frame between.
pub fn finish_actions(
    hooks: Res<gd_game_core::hooks::Hooks>,
    time: Res<Time>,
    data: Res<GameData>,
    mut cache: ResMut<AnimCache>,
    mut chars: Query<(&mut Character, Option<&Transform>)>,
) {
    let dt = time.delta_secs() * 1000.0;
    for (mut c, tf) in &mut chars {
        let c = &mut *c;
        c.derive_motion(&data, &mut cache, tf.map_or(1.0, |t| t.scale.x));
        // The movers pass this frame's move clip speed again with their move state.
        c.move_rate = None;
        if c.state != CharState::Idle {
            c.idle = IdleMode::Combat;
        }
        c.idle_timer.0 += dt;
        if c.state == CharState::Death {
            c.queued = None;
            c.attack_delay = 0.0;
        }
        let ends: Vec<AnimType> = c
            .pending
            .drain(..)
            .filter(|(_, n)| n == "End")
            .map(|(t, _)| t)
            .collect();
        for t in ends {
            match (c.state, t) {
                (CharState::Idle, t) if c.idle == c.idle_playing => {
                    c.idle_end(&data, &mut cache, t)
                }
                (
                    CharState::Cast,
                    AnimType::Special
                    | AnimType::BuffSelf
                    | AnimType::BuffOther
                    | AnimType::SpellCast
                    | AnimType::Attack
                    | AnimType::Kick
                    | AnimType::AttTurnRight90
                    | AnimType::AttTurnLeft90
                    | AnimType::AttTurnRight180
                    | AnimType::AttTurnLeft180
                    | AnimType::Transform,
                ) => {
                    c.state = CharState::Idle;
                    c.cast_anim = None;
                    // The ended clip must not satisfy `drive_characters`' wanted check:
                    // a requester may re-set the same state this frame (the swing chain),
                    // and the settled clip would never advance again.
                    c.playing = AnimType::None;
                }
                // The proc's special ended: the swing is over (its hit point
                // already fired from this clip, `combat::apply_hits`).
                (CharState::Attack, AnimType::Special) => {
                    c.state = CharState::Idle;
                    c.playing = AnimType::None;
                    c.swing_special = None;
                }
                // The channel / whirlwind start clip ended: its loop plays
                // until the requester releases (the loop's own "End" only
                // wraps). No loop clip: the action is over.
                (CharState::Channel, AnimType::ChannelStart) => {
                    if !c.play(
                        &data,
                        &mut cache,
                        AnimType::Channel,
                        c.spell_speed,
                        Some(SpeedKind::Cast),
                        true,
                    ) {
                        c.state = CharState::Idle;
                        c.playing = AnimType::None;
                    }
                }
                (CharState::Channel2, AnimType::Channel2Start) => {
                    if !c.play(
                        &data,
                        &mut cache,
                        AnimType::Channel2,
                        c.spell_speed,
                        Some(SpeedKind::Cast),
                        true,
                    ) {
                        c.state = CharState::Idle;
                        c.playing = AnimType::None;
                    }
                }
                (CharState::Spin, AnimType::SpinStart) => {
                    if !c.play(
                        &data,
                        &mut cache,
                        AnimType::Spin,
                        c.attack_speed,
                        Some(SpeedKind::Attack),
                        true,
                    ) {
                        c.state = CharState::Idle;
                        c.playing = AnimType::None;
                    }
                }
                (CharState::TakeHit, AnimType::TakeHit) | (CharState::Pickup, AnimType::Pickup) => {
                    c.state = CharState::Idle;
                    c.playing = AnimType::None;
                }
                (CharState::Roar, AnimType::Flee)
                | (CharState::Alert, AnimType::Alert)
                | (CharState::GettingUp, AnimType::GetUpFaceDown | AnimType::GetUpFaceUp) => {
                    c.state = CharState::Idle;
                    c.playing = AnimType::None;
                }
                // The scripted clip keeps playing (as before the action-queue work): a looped one (the doll's and
                // the player's long idle) wraps with an "End" every cycle, and clearing `playing` here made Idle
                // restart the combat idle and the scripted clip every wrap (the doll's twitch).
                (CharState::Scripted, t) if t == c.scripted => c.state = CharState::Idle,
                (CharState::Attack, t) if anim::is_attack(t) => {
                    if hooks.render.char_state.as_deref() == Some("attack") {
                        c.swing_special = None;
                        c.play(
                            &data,
                            &mut cache,
                            AnimType::Attack,
                            c.attack_speed,
                            Some(SpeedKind::Attack),
                            false,
                        );
                    } else {
                        c.state = CharState::Idle;
                        c.playing = AnimType::None;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Starts animations for state changes. Runs after the frame's requesters
/// (`CharacterStage::Drive` orders it after `CombatSet`): a state set this frame
/// animates this frame, and the ended actions were already finished by
/// [`finish_actions`] before the requesters ran.
/// - A channel / whirlwind state whose skill stopped (its `gd_game_core::casting::Channel` gone: released, out of
///   energy, a new action, a stun, an evade) is over ([`Character::end_channel`]): one place for every stop.
/// - A move clip already playing whose mover now passes another caller speed ([`Character::move_rate`]) takes
///   it in place. PORT: `Character::MoveTo` re-plays the clip (a new cycle, blended); the port keeps the cycle.
pub fn drive_characters(
    data: Res<GameData>,
    mut cache: ResMut<AnimCache>,
    mut chars: Query<(&mut Character, Has<gd_game_core::casting::Channel>)>,
) {
    for (mut c, channel) in &mut chars {
        let c = &mut *c;
        if !channel {
            c.end_channel();
        }
        // The proc's special belongs to its swing: another state drops it
        // (a plain swing's pool roll sets it again before this runs).
        if c.state != CharState::Attack {
            c.swing_special = None;
        }
        let wanted = if c.state == CharState::Scripted {
            c.playing == c.scripted
        } else if c.state == CharState::Idle && c.idle != c.idle_playing {
            false
        } else if c.state == CharState::Death
            && matches!(c.playing, AnimType::GetUpFaceDown | AnimType::GetUpFaceUp)
            && c.channel.current.as_ref().is_some_and(|a| a.speed == 0.0)
        {
            // A corpse that died lying keeps its held frame (the engine's ragdoll just goes on).
            true
        } else if c.state == CharState::Attack && c.playing == AnimType::Special {
            // Only the proc's own special is the swing: a cast's Special left
            // over from an interrupted cast must not count.
            c.swing_special.is_some()
        } else {
            c.plays_own_clip()
        };
        if !wanted {
            c.enter_state(&data, &mut cache);
        } else if let Some(playing) = c.move_clip_rate() {
            let (rate, fixed) = c.move_caller(c.state);
            if (playing - rate).abs() > 1e-4 {
                let speed = rate * c.record_speed;
                if let Some(cur) = &mut c.channel.current {
                    cur.speed = speed;
                }
                c.fixed_speed = fixed;
            }
        }
    }
}
