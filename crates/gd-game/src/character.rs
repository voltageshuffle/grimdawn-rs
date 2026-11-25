//! The character animation systems: the single-tick action pipeline
//! (advance → finish → drive), the idle controllers and the background clip loads.
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
            // Callbacks dispatch before the gameplay readers: a hit point crossed this frame lands its damage
            // this frame (the engine's inline dispatch).
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
                crate::turning::turn_characters.before(TransformSystems::Propagate),
            )
            // `Animate`'s PostUpdate readers (audio's `character_callbacks`) must read propagated transforms.
            .configure_sets(
                PostUpdate,
                CharacterStage::Animate.after(TransformSystems::Propagate),
            );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            start_hooks
                .run_if(|h: Res<gd_game_core::hooks::Hooks>| {
                    h.render.char_state.is_some() || h.render.char_hold.is_some()
                })
                .before(player_long_idle)
                .in_set(CharacterStage::Drive)
                .after(gd_game_core::combat::CombatSet),
        );
    }
}

/// Files the clips parsed in the background, kept warm until their creatures hold them.
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

/// Parses a new character's whole clip set in the background and hands it the clips ([`HeldAnims`]) (PORT: the
/// analogue of the engine's background `ResourceLoader`). A clip played before its set lands is parsed on the spot.
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

/// The `GD_CHAR_STATE` / `GD_CHAR_HOLD` start state, applied the frame a character appears, before it is driven.
#[cfg(feature = "dev")]
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

/// Advances every character's animation by the frame time (scaled by slows): stores the pose, the world-space
/// root-motion velocity, and sends the callback points crossed before the gameplay readers (the engine
/// dispatches inside the advance, `AnimChannel::Update` → `SkillManager::HandleSkillAnimationCallback`). Also
/// arms [`Character::allow_interrupt`] at `AllowInterrupt`, ends an evade's dash at `StopJump`, counts the
/// attack-delay down, and stores the crossed entity events. A settled clip keeps its pose and is not sampled.
/// Characters advance in parallel; the callbacks then go out in query order.
pub fn advance_animations(
    time: Res<Time>,
    mut chars: Query<(Entity, &Transform, &mut Character)>,
    mut out: MessageWriter<AnimCallback>,
) {
    let dt_ms = time.delta_secs() * 1000.0;
    chars.par_iter_mut().for_each(|(_, root_tf, mut c)| {
        // `ControllerPlayer::Update`.
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
        // Characters are root entities: the local transform is the current one.
        let root = root_tf.compute_affine();
        let v = anim::root_velocity_object(&c.body, ev.root_velocity, 1.0);
        c.root_velocity = root.transform_vector3(Vec3::from(v)) * k;
        for name in &ev.callbacks {
            match name.as_str() {
                "RightHandHit" | "LeftHandHit" => {
                    debug!("{}: {name} at frame {:.1}", c.record, c.channel.frame());
                }
                // `AttackAction::AnimationCallback`.
                "AllowInterrupt" => c.allow_interrupt = true,
                // The recovery is cancelable (`ControllerPlayerStateJumpToUseSkill::RequestSkillAction` refuses
                // only the warm-up).
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

/// `ControllerPlayerStateIdle::OnUpdate` / `ControllerPlayerStateLongIdle::Attacked`: the long idle after
/// [`LONG_IDLE_AFTER_MS`] idling out of combat, back to the combat idle when it fights.
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

/// The NPC whose conversation is open plays the chat idle; at its start it turns to the player
/// (`ChatAction::FaceTarget`: yaw only, at least 1 m apart, `rotateWhenChatting` default on).
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

/// Finishes the actions whose "End" callback came, before the frame's requesters run (the engine's
/// `CharacterHandlerUpdate` order), so a swing that ends this frame chains into the next with no idle frame.
pub fn finish_actions(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    time: Res<Time>,
    data: Res<GameData>,
    mut cache: ResMut<AnimCache>,
    mut chars: Query<(&mut Character, Option<&Transform>)>,
) {
    let dt = time.delta_secs() * 1000.0;
    for (mut c, tf) in &mut chars {
        let c = &mut *c;
        c.derive_motion(&data, &mut cache, tf.map_or(1.0, |t| t.scale.x));
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
                    // The settled clip must not satisfy `drive_characters`' wanted check: a requester may
                    // re-set the same state this frame (the swing chain).
                    c.playing = AnimType::None;
                }
                // The proc's special ended: the swing is over.
                (CharState::Attack, AnimType::Special) => {
                    c.state = CharState::Idle;
                    c.playing = AnimType::None;
                    c.swing_special = None;
                }
                // The start clip ended: its loop plays until the requester releases (the loop's "End" only
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
                // A looped scripted clip wraps with an "End" every cycle: clearing `playing` would restart it.
                (CharState::Scripted, t) if t == c.scripted => c.state = CharState::Idle,
                (CharState::Attack, t) if anim::is_attack(t) => {
                    #[cfg(feature = "dev")]
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
                        continue;
                    }
                    c.state = CharState::Idle;
                    c.playing = AnimType::None;
                }
                _ => {}
            }
        }
    }
}

/// Starts animations for state changes, after the frame's requesters (a state set this frame animates this
/// frame).
/// - A channel / whirlwind state whose `gd_game_core::casting::Channel` is gone is over
///   ([`Character::end_channel`]): one place for every stop.
/// - A playing move clip whose mover passes another caller speed takes it in place. PORT: `Character::MoveTo`
///   re-plays the clip (a new cycle, blended); the port keeps the cycle.
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
        // The proc's special belongs to its swing (a plain swing's pool roll sets it again before this runs).
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
            // A corpse that died lying keeps its held frame.
            true
        } else if c.state == CharState::Attack && c.playing == AnimType::Special {
            // A cast's Special left over from an interrupted cast is not the swing.
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
