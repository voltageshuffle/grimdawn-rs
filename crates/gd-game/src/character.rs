//! Character animation state (players and creatures), following the
//! engine: `gd_sim::anim` sets by hand
//! state, one channel with 200 ms cross-fades, root motion and callback
//! points. The per-frame pipeline is the engine's one-tick action loop
//! (`CharacterHandlerUpdate` @0x1801279f0: finish ended actions → run pending →
//! animate): [`advance_animations`] advances every clip and sends its callback
//! points ([`AnimCallback`]: where hits land, casts fire, evades jump) and the
//! root-motion velocity in `Update` before the gameplay readers (the engine
//! emits and dispatches them inline, `AnimChannel::Update` @0x180038790 →
//! `SkillManager::HandleSkillAnimationCallback` @0x180533700); [`finish_actions`]
//! drains the "End"s so the frame's requesters see the finished actions;
//! [`drive_characters`] then plays the matching animation for every [`CharState`]
//! the requesters set this frame. A request arriving mid-action is remembered in
//! the one pending slot ([`Character::queued`], last wins) and runs at the clip's
//! `AllowInterrupt` point or its "End" (`casting::requests`' busy gate); a move's
//! cancel leaves the remainder as [`Character::attack_delay`] so the cadence is kept.
//! Cast cadence: a spell plays its
//! slot at `GetSpellCastSpeed` (`SkillActivatedSpell::StartAction` @0x18051b120 passes the
//! capped profile cast speed — bio `characterSpellCastSpeed` × the sheet's cast-speed
//! multiplier, not the sheet's percentage — to the slot, `AnimEntry_Play` @0x180020db0 multiplies the entry's record
//! speed, `AnimChannel::Update` advances `dt × speed` ms), and a held skill re-casts only
//! after the clip's "End" (the same skill is refused, never cancelled at `AllowInterrupt`):
//! one cast per (frames − 1) ÷ fps ÷ speed. The body's bone frames in that
//! pose are computed once, with it ([`Character::posed`]): the hit bounds, the
//! game's body parts and weapons, attach points ([`Character::posed_point`]) and
//! effects all read them.
//! Its facing is turned by `crate::turning` ([`Character::turn`]); an attack or cast says
//! where it goes with [`Character::begin_cast`] / [`Character::begin_swing`], which also picks the
//! `AttTurn*` clip a turning weapon attack starts with (`gd_sim::anim::attack_turn`).
//! A weapon-pool proc plays the picked skill's special animation instead of the plain swing
//! ([`Character::swing_special`], `weapon_pool::pool_swing_specials`;
//! `SkillActivatedWeaponPool::StartAction` @0x18051bb90 runs the picked skill's own `StartAction`),
//! and its hit point is that clip's. Channelled skills play their channel pair
//! ([`CharState::Channel`] / [`CharState::Channel2`]: `*Start` once, then the loop, until the
//! requester releases), a whirlwind [`CharState::Spin`] (`SpinStart` → `Spin`), a loot pickup
//! [`CharState::Pickup`] and a flinch [`CharState::TakeHit`] (the slots 40–46 of the engine's
//! 52-slot table, `GetAnimTypeAsText` @0x180025000), a monster's alert before its pursuit
//! [`CharState::Alert`] (slot 33), and a knockdown [`CharState::KnockedDown`] → [`CharState::GettingUp`]
//! (the get-up slots 38 / 39, `crate::ragdoll`).
//! The Run / Walk / Charge slots' own root-motion speed ([`Character::move_motion`], derived per animation set) is
//! what a mover runs at before its clip has produced root motion (`combat::run_speed`). A move clip plays at the
//! caller speed its mover passes ([`Character::move_rate`]: the run speed, 1.0 for unit pace and a charge, a walk's
//! `walkSpeed` × the run speed), so the ground speed is always the clip's own motion and the feet keep pace.
//! A profile speed change reaches the playing clip in place ([`Character::force_speed_update`],
//! `Character::ForceSpeedUpdate` @0x180068ca0 → `AnimationSet::AdjustAnimation` @0x180024f20:
//! caller speed × record speed on the current slot); the clip carries its [`SpeedKind`] so the
//! debuff slow scales it by kind (a weapon special by the attack slow, a kick by the attack
//! slow, a charge by the run slow), not by animation type.
//!
//! What an idle character plays is its [`IdleMode`] (the engine's controller state picks the idle action;
//! Game.dll):
//! - Combat (`IdleAction` @0x180081ac0): `AttackIdle`, re-picked at each "End". Every character idles so by default.
//! - Long (`LongIdleAction` @0x180082010 / `AnimationCallback` @0x180082230): `IdleTransition` once, then `LongIdle`
//!   looped; at a `LongIdle` "End" once the transition timer is up (a random 3000..20000 ms, `ResetTransitionTimer`
//!   @0x1800824b0, re-rolled when the long idle starts) one `Fidget`, then `LongIdle` with a new timer. No transition
//!   clip: `LongIdle` at once. The player's controller asks for it ([`player_long_idle`]):
//!   `ControllerPlayerStateIdle::OnUpdate` @0x180165ba0 moves to "LongIdle" after 2000 ms of idling out of combat
//!   (the timer restarts while `Character::IsUnderAttack`, the flag the constitution reads: [`Constitution::out_of_combat`]);
//!   `ControllerPlayerStateLongIdle::Attacked` @0x180165ed0 goes back to "Idle", and so does any other state.
//! - Chat (`ChatAction::Execute` @0x180086f90 / `AnimationCallback` @0x180087440): an NPC whose conversation is open
//!   (`ControllerNpcStateIdle::OnUpdate` @0x180159830 → "Chat" while the NPC has a partner; `ControllerNpcStateChat::
//!   OnUpdate` @0x180159eb0 → "Idle" when it ends) plays `Chat` once (no `Chat` clip: `Fidget`; neither: `LongIdle`),
//!   then `LongIdle` and `Fidget` in turn (no timer check). It first turns to the player ([`npc_chat`]).
//! - PORT: a long or chat idle without any of its clips plays `AttackIdle` (the engine's action ends and the last
//!   animation stays); `LongIdle` loops in the channel (the engine's End handler replays nothing until a fidget is due,
//!   so the slot must loop).
//! - Menu (`ControllerPlayerStateMenuIdle::OnBegin` @0x180165f80 → `MenuIdleAction` @0x180082610, `Execute`
//!   @0x1800826e0, `AnimationCallback` @0x180082890): the main menu's character, the
//!   `LongIdleAction` twin on `MenuIdle` / `MenuFidget`: `MenuIdle` looped with a new transition timer; at a
//!   `MenuIdle` "End" once the timer is up one `MenuFidget` (none: the timer restarts), at its "End" `MenuIdle` with
//!   a new timer. Every switch happens at an "End", so the clip never restarts mid-loop and no other idle shows in
//!   between.
//! - An NPC's controller (`ControllerNpc2`) alternates Combat and Long out of conversation: `crate::npc_controller`.
use crate::constitution::Constitution;
use crate::data::GameData;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use gd_formats::anm::Animation;
use gd_formats::arz::Record;
use gd_formats::msh::{Coords, Mesh as Msh};
use gd_sim::anim::{self, AnimChannel, AnimSet, AnimType, Pose, RecordIndex, SpeedKind};
use gd_sim::equip::{HandState, WeaponType};
use std::sync::Arc;

/// The way a character or object faces, a unit vector. Characters face +Z
/// (the spawn rotation is `Quat::from_rotation_y(yaw)` with yaw measured from
/// +Z, as the engine does), while Bevy's `Transform::forward()` is -Z and
/// points behind them; every gameplay facing goes through here.
pub fn facing(tf: &Transform) -> Vec3 {
    tf.rotation * Vec3::Z
}

/// [`facing`] on the ground plane (y dropped, renormalised; +Z when it points
/// straight up or down).
pub fn facing_flat(tf: &Transform) -> Vec3 {
    facing(tf).with_y(0.0).normalize_or(Vec3::Z)
}

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
                    .before(crate::player::PlayerSet)
                    .before(crate::movement::crowd_begin)
                    .before(crate::story::fire_npc_casts)
                    .before(crate::story_world::record_hooks),
            )
            .add_systems(
                Update,
                (
                    start_hooks.run_if(|h: Res<crate::hooks::Hooks>| {
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
                    .after(crate::combat::CombatSet),
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

/// The parsed-`.anm` store shared by gd-game's [`AnimCache`] and gd-render's
/// `ModelCache` (one parse per file; `ModelPlugin` links the two resources,
/// `drop_on_mod_change` re-links and clears them when the mounted mod changes). A clip lives while a holder does
/// ([`crate::weak_cache`]: a playing channel, an `Animator`, the clips a [`Character`] played).
pub type SharedAnims = Arc<std::sync::Mutex<crate::weak_cache::WeakCache<Animation>>>;

/// Parsed `.anm` animations by normalized path (the shared store, [`SharedAnims`]). The last [`RECENT_ANIMS`] clips
/// used stay parsed: a fight's attack / hit / death clips come and go with the creatures that play them.
#[derive(Resource)]
pub struct AnimCache {
    anims: SharedAnims,
    /// The clip sets of new characters being parsed in the background ([`preload_anims`]): the character, the clips
    /// already parsed, and the task parsing the rest.
    preloads: Vec<(
        Entity,
        Vec<Arc<Animation>>,
        bevy::tasks::Task<Vec<(String, Option<Animation>)>>,
    )>,
    /// Clips parsed in the background ahead of a spawn ([`AnimCache::ready`], [`collect_anim_loads`]).
    loading: Vec<bevy::tasks::Task<(String, Option<Animation>)>>,
    /// The paths of `loading`, so a clip is parsed once.
    in_flight: std::collections::HashSet<String>,
}

/// How many clips [`AnimCache`] keeps parsed after their last holder went.
const RECENT_ANIMS: usize = 1024;

impl Default for AnimCache {
    fn default() -> Self {
        Self {
            anims: Arc::new(std::sync::Mutex::new(
                crate::weak_cache::WeakCache::with_recent(RECENT_ANIMS),
            )),
            preloads: Vec::new(),
            loading: Vec::new(),
            in_flight: Default::default(),
        }
    }
}

impl AnimCache {
    /// Cached animations still in use (the memory audit).
    pub fn count(&self) -> usize {
        self.anims.lock().unwrap().live()
    }

    /// The store gd-render's `ModelCache` shares ([`SharedAnims`]).
    pub fn shared(&self) -> SharedAnims {
        self.anims.clone()
    }

    pub fn anim(&mut self, data: &GameData, path: &str) -> Option<Arc<Animation>> {
        shared_anim(&self.anims, data, path)
    }

    /// Whether the clip at `path` is parsed (or failed). Otherwise its parse starts in the background
    /// ([`collect_anim_loads`]) and this is false: a spawn that waits for it never parses a clip on the game thread
    /// when its creature first plays it.
    pub fn ready(&mut self, data: &GameData, path: &str) -> bool {
        let key = gd_formats::vfs::normalized(path);
        if self.anims.lock().unwrap().get_kept(&key).is_some() {
            return true;
        }
        if self.in_flight.insert(key.to_string()) {
            let (key, data) = (key.into_owned(), data.clone());
            self.loading
                .push(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                    let a = data
                        .vfs
                        .read(&key)
                        .and_then(|b| Animation::parse(&b))
                        .inspect_err(|e| warn!("animation {key}: {e:#}"))
                        .ok();
                    (key, a)
                }));
        }
        false
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
        let mut store = cache.anims.lock().unwrap();
        if store.get(&key).is_none() {
            store.put(&key, a);
        }
    }
}

/// Every clip a creature of `record` (`variant`) plays: the animation set its record and its weapons give it, as
/// [`character_for`] builds it. A spawn waits for these ([`AnimCache::ready`]).
pub fn creature_clips(data: &GameData, record: &str, variant: u32) -> Vec<String> {
    let Some(rec) = data.db.get(record) else {
        return Vec::new();
    };
    let (right, left) = crate::creature::creature_look(&data.db, record, variant)
        .map(|l| l.weapons(&data.db))
        .unwrap_or_default();
    let set = anim_set_for(data, rec, HandState::from_weapons(right, left));
    let mut paths: Vec<String> = set
        .slots
        .iter()
        .chain(set.specials.values())
        .flat_map(|s| &s.entries)
        .map(|a| a.path.clone())
        .collect();
    paths.sort_unstable();
    paths.dedup();
    paths
}

/// On a character: every clip of its animation set ([`preload_anims`]), held while it lives.
#[derive(Component)]
pub struct HeldAnims {
    _clips: Vec<Arc<Animation>>,
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
            let mut store = cache.anims.lock().unwrap();
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
        let mut store = cache.anims.lock().unwrap();
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

/// [`AnimCache::anim`] on a [`SharedAnims`] store: the one loader both caches
/// use, so a file is parsed once while it is held. The store is not locked while the file is read and parsed: the
/// systems sharing it (gd-render's `ModelCache`, [`preload_anims`], [`collect_anim_loads`]) run on other threads and
/// would otherwise wait out the whole parse; the second look shares a clip another caller parsed meanwhile.
pub fn shared_anim(anims: &SharedAnims, data: &GameData, path: &str) -> Option<Arc<Animation>> {
    let key = gd_formats::vfs::normalized(path);
    if let Some(hit) = anims.lock().unwrap().get_kept(&key) {
        return hit;
    }
    let a = data
        .vfs
        .read(&key)
        .and_then(|b| Animation::parse(&b))
        .inspect_err(|e| warn!("animation {key}: {e:#}"))
        .ok();
    let mut store = anims.lock().unwrap();
    // Another caller may have parsed it meanwhile: share theirs.
    if let Some(hit) = store.get_kept(&key) {
        return hit;
    }
    store.put(&key, a)
}

/// An animation callback point crossed this frame (e.g. "RightHandHit" is
/// where melee damage lands, "End" when the animation finishes or loops).
/// `cast`: the character was casting when the point crossed — the callback's
/// own identity, not the live state (the "End" of the same step may have left
/// [`CharState::Cast`] before a reader runs).
#[derive(Message, Clone, Debug)]
pub struct AnimCallback {
    pub entity: Entity,
    pub anim: AnimType,
    pub name: String,
    pub cast: bool,
}

/// A plain move request queued while busy ([`Character::queued`]): the fields of
/// [`crate::player_orders::PlayerOrder::Move`].
#[derive(Clone, Copy, Debug)]
pub struct MoveOrder {
    pub to: Option<Vec3>,
    pub ground: Option<Vec3>,
    pub camera: Vec3,
}

/// The single pending-action slot (`ControllerBaseCharacter+21`, `SetQueuedAction`):
/// a request while busy is remembered, the last one wins, and runs when the current
/// action allows it — its `AllowInterrupt` point or its "End"
/// (`CharacterHandlerUpdate` @0x1801279f0, `AttackAction::QueryActionPermission`
/// @0x180084380). PORT: the player's requests only (monsters and pets re-request
/// every AI tick, so their drops are never felt).
#[derive(Clone, Debug)]
pub enum QueuedAction {
    Cast(crate::casting::CastRequest),
    Move(MoveOrder),
}

/// High-level state requested by gameplay/AI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharState {
    Idle,
    Run,
    /// A `WalkAction` (patrol legs): the walk animation (`Character::WalkTo`
    /// sets animation state 6).
    Walk,
    /// A charge skill's approach (`ControllerMonsterStatePursue::OnBegin`
    /// passes animation type 7 to `ControllerAI::MoveTo` for a move type 2
    /// skill): the Charge slot, looped.
    Charge,
    Attack,
    /// Casting a skill: `cast_anim` names the special animation
    /// (`skillSpecialAnimationName`), else `cast_type` plays (see
    /// [`Character::start_cast`]).
    Cast,
    /// A channelled skill (`SkillChanneled` subclasses: beams, cones, drains,
    /// grows): the `ChannelStart` slot once, then the `Channel` loop, until the
    /// skill stops ([`Character::begin_channel`]; [`drive_characters`] ends it with its `crate::casting::Channel`).
    Channel,
    /// The second channel pair (`Channel2Start` / `Channel2`), for the skills
    /// whose table names it ([`Character::begin_channel`]'s `second`).
    Channel2,
    /// A whirlwind (`Skill_AttackRadiusSpin`): `SpinStart` once, then the `Spin`
    /// loop at the attack speed, until the requester releases.
    Spin,
    /// Stunned / asleep / knocked down (`afflictions.rs`): the stun loop.
    Stunned,
    /// A flinch (CC type 49, `DurationDamageManager::UpdateFxAndInfluence`
    /// @0x18027b8a0 → `Character::ExecuteTakeHit` @0x18005d650 plays animation
    /// type 46): the `TakeHit` slot once; the state ends with its "End".
    TakeHit,
    /// Picking loot up (the `Pickup` slot, animation type 8), once; the state
    /// ends with its "End".
    Pickup,
    Death,
    /// Evading (`evade.rs`): the Evade slot (animation type 48, set by
    /// `Skill_Evade::StartAction`).
    Evade,
    /// The flee roar (`flee.rs`): the Flee slot (animation type 37), played
    /// once; the state ends with its "End".
    Roar,
    /// The alert before a pursuit (`combat::monster_ai`, `ControllerMonsterStateAlertBeforePursue::OnBegin`
    /// @0x18014b940: `PlayAnimation(33 Alert, speed 1)`): the Alert slot once; the state ends with its "End".
    Alert,
    /// Knocked down (`Character::ExecuteKnockdown` @0x18005d580: `StopAnimations`, action state 9;
    /// `crate::ragdoll`): the body lies in the first frame of its get-up clip, held (speed 0) while down.
    KnockedDown,
    /// Getting up (`ControllerMonsterStateGettingUp::OnUpdate` @0x18014a9b0: `PlayAnimation(38 GetUpFaceDown
    /// when it fell face down, else 39 GetUpFaceUp, 1.0)`), once; the state ends with its "End" (`HandleEvent`
    /// "End" → Return).
    GettingUp,
    /// A quest `ControllerState` action (`ScriptableAction_ControllerState`,
    /// [`Character::play_scripted`]): the [`Character::scripted`] slot plays
    /// once, then the character idles.
    Scripted,
}

/// What an idle character plays (module doc): the controller's idle action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IdleMode {
    /// `IdleAction`: the attack idle.
    #[default]
    Combat,
    /// `LongIdleAction`: the relaxed idle with its transition and fidgets.
    Long,
    /// `ChatAction`: an NPC talking to the player.
    Chat,
    /// `MenuIdleAction`: the main menu's character (module doc).
    Menu,
}

/// `ControllerPlayerStateIdle`: the player idles this long out of combat before the long idle (ms).
pub const LONG_IDLE_AFTER_MS: f32 = 2000.0;
/// `LongIdleAction` / `ChatAction` transition timer range (ms).
const FIDGET_WAIT_MS: (f32, f32) = (3000.0, 20000.0);

#[derive(Component)]
pub struct Character {
    pub record: String,
    pub is_player: bool,
    pub hand: HandState,
    pub anims: Arc<AnimSet>,
    pub channel: AnimChannel,
    /// Animation type currently in the channel.
    pub playing: AnimType,
    /// The move state whose own clip would not play (an unreadable `.anm`) and the other move clip
    /// `enter_state` played for it: that clip is the state's own until the state is entered again.
    move_fallback: Option<(CharState, AnimType)>,
    pub state: CharState,
    /// What the character plays while [`CharState::Idle`]: the controller sets it, any other state resets it to
    /// [`IdleMode::Combat`] (leaving the controller's idle state ends its action).
    pub idle: IdleMode,
    /// The idle mode whose cycle is playing (a change of [`Character::idle`] restarts the idle).
    idle_playing: IdleMode,
    /// The transition timer of a long / chat idle: ms elapsed, ms due.
    idle_timer: (f32, f32),
    /// Ms the character has been in [`CharState::Idle`] (the player's long-idle timer).
    pub idle_ms: f32,
    pub cast_anim: Option<String>,
    /// The slot a [`CharState::Scripted`] character plays.
    pub scripted: AnimType,
    /// Animation of the current cast when there is no special one
    /// (`gd_sim::anim::skill_animation`), its speed and its speed kind.
    pub cast_type: AnimType,
    pub cast_speed: f32,
    cast_kind: SpeedKind,
    /// The cast's class turns into its attack (`gd_sim::anim::attack_turns`).
    cast_turns: bool,
    /// The weapon-pool proc's special animation reference name
    /// (`skillSpecialAnimationName` of the picked skill, set by
    /// `weapon_pool::pool_swing_specials` as the swing starts): the
    /// [`CharState::Attack`] plays it instead of the plain clip, and the hit
    /// point is its `RightHandHit` (`SkillActivatedWeaponPool::
    /// StartAction` @0x18051bb90 runs the picked skill's own `StartAction`).
    /// `None`: the plain swing. Cleared when the attack ends or another state
    /// takes over.
    pub swing_special: Option<String>,
    /// The `AttTurn*` slots the attack starting tries first ([`Character::begin_attack`]).
    turn_types: &'static [AnimType],
    /// What the character turns toward and how fast (`crate::turning`, the one writer of its facing).
    pub turn: crate::turning::Turn,
    /// Run animation rate (`characterRunSpeed`). The three record
    /// fields are the base `CharAttributeVal_RunSpeed` / `_AttackSpeed` /
    /// `_SpellCastSpeed` (`CharAttributeStore_Bio::Load`) that
    /// `Character::GetRunSpeed` / `GetAttackSpeed` / `GetSpellCastSpeed`
    /// build on. They are only the starting values: the animation rate is the
    /// full speed (`Character::MoveTo`, Game.dll 0x18005f870, plays
    /// the run animation at the `GetRunSpeed` its callers pass; attacks and
    /// casts use `SkillManager::GetAttackSpeed` / `GetSpellCastSpeed`), which
    /// `combat::sync_speeds` copies in from the capped profile speeds.
    pub run_speed: f32,
    /// Attack animation rate (`characterAttackSpeed`, see `run_speed`).
    pub attack_speed: f32,
    /// Spell/buff animation rate (`characterSpellCastSpeed`, see `run_speed`).
    pub spell_speed: f32,
    pub scale: f32,
    /// Mesh with the skeleton/attach points (record `mesh`).
    pub body: Arc<Msh>,
    /// World-space root-motion velocity of the last frame (m/s).
    pub root_velocity: Vec3,
    /// Freeze attacks at this fraction of their length (test hook).
    pub hold: Option<f32>,
    /// Debuff slows as time scales: run, attack, cast (1 = normal).
    pub slow: [f32; 3],
    /// Frozen or petrified: the pose stops.
    pub frozen: bool,
    /// The pose computed this frame ([`advance_animations`]); the game
    /// poses the body's joints and weapons from it.
    pub pose: Pose,
    /// [`Self::body`]'s bone frames in [`Self::pose`] (mesh space, `gd_sim::anim::pose_abs`), refreshed whenever
    /// either changes: the one copy every reader (bounds, body parts, weapons, attach points) uses ([`Self::posed`]).
    posed: Vec<Coords>,
    /// A worn replacement look ([`Character::set_form`]); `body` and `anims`
    /// are the form's while it is on.
    form: Option<FormLook>,
    rng: fastrand::Rng,
    /// The playing clip's speed kind (`None`: a clip at no profile speed —
    /// idles, stuns, deaths… — which no debuff slow scales).
    speed_kind: Option<SpeedKind>,
    /// The playing clip's record speed (the picked entry's `speed`): with the
    /// caller speed it re-derives the channel rate in place
    /// ([`Character::force_speed_update`]).
    record_speed: f32,
    /// The clip plays at a fixed caller speed, not its kind's profile speed
    /// (a move at [`Self::move_rate`]): a profile speed change leaves its rate
    /// alone. PORT: the engine's `Character::ForceSpeedUpdate` @0x180068ca0 re-adjusts
    /// any playing move clip to `GetRunSpeed` until the mover's next `MoveTo` plays
    /// its own factor again; the port keeps the mover's factor throughout.
    fixed_speed: bool,
    /// This frame's caller speed of the move clip (Run / Walk / Charge), the speed
    /// factor `Character::MoveTo` @0x18005f870 passes to `PlayAnimationIfAvailable`
    /// (a different factor re-plays the clip): `None` is the run speed
    /// ([`Self::run_speed`], following profile changes); a unit-pace move and a charge
    /// pass 1.0, a walk `walkSpeed` × the run speed, a warmed-up charge the warm-up run
    /// speed (`MoveToAction::Execute` → `ForceSpeedUpdate` at `GetRunSpeed` with the
    /// warm-up cap). A Charge without one plays at 1.0. Reset each frame by
    /// [`finish_actions`]: the mover sets it with its move state.
    pub move_rate: Option<f32>,
    pending: Vec<(AnimType, String)>,
    /// The callback points [`advance_animations`] crossed this frame, until it sends them.
    crossed: Vec<String>,
    /// The entity events [`advance_animations`] crossed this frame
    /// (`gd_sim::anim::ChannelEvents::entities`), until gd-render's
    /// `character_anim_entities` drains them (replaced each advance, so a
    /// headless game without the render reader does not accumulate them).
    pub entity_events: Vec<anim::AnimEntityEvent>,
    /// `.anm` of the last started animation (for logs).
    last_anim: String,
    /// The pending-action slot ([`QueuedAction`]): the last request while busy wins.
    pub queued: Option<QueuedAction>,
    /// The playing clip's `AllowInterrupt` callback crossed (`AttackAction::
    /// AnimationCallback` @0x1800840f0): its recovery may be cancelled by a new or
    /// queued action. Cleared when a new clip starts; an evade's recovery arms it
    /// at `StopJump` (the dash over).
    pub allow_interrupt: bool,
    /// `ControllerPlayer::SetAttackDelay` (+1324): the cancelled remainder (ms) of a swing /
    /// cast a move interrupted, counted down each frame; a new skill use waits for it, so
    /// recovery-cancelling into a move does not raise the attack speed.
    pub attack_delay: f32,
    /// The evade's dash (warm-up) is up: skill / move requests refuse only while it
    /// is (`JumpToUseSkill::RequestSkillAction` @0x180168550); the recovery is
    /// cancelable. Set by `evade::start_evade`, cleared at the clip's `StopJump`.
    pub evade_warmup: bool,
    /// `Actor::GetFellFaceDown` @0x18002d190 (Actor +1248): the way the body fell, picks the get-up clip.
    pub fell_face_down: bool,
    /// The move slots' root-motion speeds ([`Character::move_motion`]), derived per animation set.
    motion: MoveMotion,
    /// The clips this character played from its current set (`Arc<AnimSet>` address, clips): the anim cache holds
    /// them weakly ([`SharedAnims`]), so a creature's attack clip stays parsed between its swings ([`Self::hold_clip`]).
    held_clips: (usize, Vec<Arc<Animation>>),
    /// Clips started so far ([`Self::play`], [`Self::play_special_named`]): each play's number.
    plays: u32,
    /// [`Self::plays`] when the current cast started ([`Self::start_cast`]): its own clip is a later one.
    cast_after: u32,
    /// The number of the clip whose callbacks [`advance_animations`] sent this frame.
    sent_play: u32,
}

/// The ground speed (m/s at caller speed 1, the world scale included) the root motion of the Run, Walk and Charge
/// slots carries a character: the mean over the slot's variants of each clip's root displacement over its length ×
/// its record speed (the `GetAnimSpeed` mean, `gd_sim::anim::AnimSlot::mean_duration`, over the motion).
/// Derived once per animation set ([`finish_actions`], keyed by the set's address) so a mover has the clip's own
/// speed before the clip has played a frame (`combat::run_speed`, `combat::charge_move`).
#[derive(Clone, Copy, Debug, Default)]
struct MoveMotion {
    /// The `Arc<AnimSet>` address and the body mesh address it was derived for (0: not yet).
    set: usize,
    body: usize,
    /// Object-space speeds of Run, Walk and Charge, and the world scale (the root `Transform`'s).
    run: f32,
    walk: f32,
    charge: f32,
    scale: f32,
}

impl Character {
    /// A character of `record` (`rec`) with body mesh `body` (skeleton,
    /// attach points, root-motion bone), animation set `anims` for hand state
    /// `hand`, starting in `state`; `seed` seeds its animation picks.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        record: &str,
        rec: &Record,
        is_player: bool,
        hand: HandState,
        anims: AnimSet,
        scale: f32,
        body: Arc<Msh>,
        state: CharState,
        hold: Option<f32>,
        seed: u64,
    ) -> Self {
        Character {
            record: record.to_owned(),
            is_player,
            hand,
            anims: Arc::new(anims),
            channel: AnimChannel::default(),
            playing: AnimType::None,
            move_fallback: None,
            state,
            idle: IdleMode::Combat,
            idle_playing: IdleMode::Combat,
            idle_timer: (0.0, 0.0),
            idle_ms: 0.0,
            cast_anim: None,
            scripted: AnimType::None,
            cast_type: AnimType::SpellCast,
            cast_speed: 1.0,
            cast_kind: SpeedKind::Cast,
            cast_turns: false,
            swing_special: None,
            turn_types: &[],
            turn: crate::turning::Turn::from_record(rec),
            run_speed: rec.f32("characterRunSpeed").unwrap_or(1.0)
                * (1.0 + rec.f32("characterRunSpeedModifier").unwrap_or(0.0) / 100.0),
            attack_speed: rec.f32("characterAttackSpeed").unwrap_or(1.0)
                * (1.0 + rec.f32("characterAttackSpeedModifier").unwrap_or(0.0) / 100.0),
            spell_speed: rec
                .f32("characterSpellCastSpeed")
                .filter(|s| *s > 0.0)
                .unwrap_or(1.0)
                * (1.0 + rec.f32("characterSpellCastSpeedModifier").unwrap_or(0.0) / 100.0),
            scale,
            posed: anim::pose_abs(&body, &Pose::default()),
            body,
            root_velocity: Vec3::ZERO,
            hold,
            slow: [1.0; 3],
            frozen: false,
            pose: Pose::default(),
            form: None,
            rng: fastrand::Rng::with_seed(seed),
            speed_kind: None,
            record_speed: 1.0,
            fixed_speed: false,
            move_rate: None,
            pending: Vec::new(),
            crossed: Vec::new(),
            entity_events: Vec::new(),
            last_anim: String::new(),
            queued: None,
            allow_interrupt: false,
            attack_delay: 0.0,
            evade_warmup: false,
            fell_face_down: false,
            motion: MoveMotion::default(),
            held_clips: (0, Vec::new()),
            plays: 0,
            cast_after: 0,
            sent_play: 0,
        }
    }
}

/// A replacement look: the engine's override slot 0 (`Character::SetOverrideMesh`
/// @0x18006d730 / `SetOverrideAnimations(path, slot)` @0x18006d400:
/// `GetMeshInstance` @0x18006da30 and `GetAnimationSet` @0x18005ae40 return the
/// first set override slot, else the character's own). The own body and set
/// are kept here to come back to.
struct FormLook {
    mesh: String,
    records: FormRecords,
    own_body: Arc<Msh>,
    own_anims: Arc<AnimSet>,
}

/// The rest of a form's look, set with the mesh
/// (`Skill_Shapeshift::SetShapeshifted` @0x180505a70): `replacementSounds`
/// (`Character::SetOverrideSounds` @0x18006d650: a `CharacterSounds` table
/// loaded over the character's override table, `crate::char_sounds`),
/// `replacementFootsteps` (`SetOverrideFootsteps` @0x18006d6c0: a footstep
/// set, the footprint decals) and `attackTrail` (`SetUnarmedWeaponTrail`
/// @0x18006d850). Empty: none (the override table is cleared, the own
/// entries apply).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormRecords {
    pub sounds: String,
    pub footsteps: String,
    pub trail: String,
}

impl Character {
    /// Wears the mesh `mesh` (`body`) with the animation set `anims`
    /// (`Character::SetOverrideMesh` / `SetOverrideAnimations`): the
    /// skeleton, attach points and animations become the form's; the old
    /// animation's pending callbacks are dropped (`Actor::ClearAnimationCallbacks`)
    /// and the current state's animation restarts from the new set. `None`:
    /// no replacement set (an empty path sets no override), the own one plays.
    pub fn set_form(
        &mut self,
        mesh: &str,
        body: Arc<Msh>,
        anims: Option<AnimSet>,
        records: FormRecords,
    ) {
        self.wear_form(mesh, body, anims.map(Arc::new), records);
    }

    /// [`Character::set_form`] with a shared set (`None`: the own one).
    pub fn wear_form(
        &mut self,
        mesh: &str,
        body: Arc<Msh>,
        anims: Option<Arc<AnimSet>>,
        records: FormRecords,
    ) {
        let (own_body, own_anims) = match self.form.take() {
            Some(f) => (f.own_body, f.own_anims),
            None => (self.body.clone(), self.anims.clone()),
        };
        self.anims = anims.unwrap_or_else(|| own_anims.clone());
        self.form = Some(FormLook {
            mesh: mesh.to_owned(),
            records,
            own_body,
            own_anims,
        });
        self.body = body;
        self.repose();
        self.pending.clear();
        self.playing = AnimType::None;
    }

    /// Takes the form off (the override slot cleared): the own body and set
    /// come back. `SetOverrideAnimations` @0x18006d400: the current
    /// action restarts its run or idle animation, so a cast in progress
    /// (the form's Transform) ends.
    pub fn clear_form(&mut self) {
        let Some(f) = self.form.take() else { return };
        self.body = f.own_body;
        self.repose();
        self.anims = f.own_anims;
        self.pending.clear();
        if self.state == CharState::Cast {
            self.state = CharState::Idle;
            self.cast_anim = None;
        }
        self.playing = AnimType::None;
    }

    /// The body's bone frames in the current pose (mesh space; `gd_sim::anim::pose_abs(&self.body, &self.pose)`).
    pub fn posed(&self) -> &[Coords] {
        &self.posed
    }

    /// The attach point `name` of the body in its pose, in the character's model space
    /// (`Actor::GetAttachPointCoords`: the parent bone's posed frame × the point's coords; no parent: model space).
    pub fn posed_point(&self, name: &str) -> Option<Vec3> {
        let p = self.body.attach_point(name)?;
        Some(Vec3::from(p.coords_in(&self.posed).origin))
    }

    /// Re-poses [`Self::posed`] after the body or the pose changed.
    fn repose(&mut self) {
        anim::pose_abs_into(&self.body, &self.pose, &mut self.posed);
    }

    /// The worn form's mesh, if any.
    pub fn form_mesh(&self) -> Option<&str> {
        self.form.as_ref().map(|f| f.mesh.as_str())
    }

    /// The worn form's sounds, footsteps and trail ([`FormRecords`]).
    pub fn form_records(&self) -> Option<&FormRecords> {
        self.form.as_ref().map(|f| &f.records)
    }

    /// Plays the Transform slot once as a cast (`Skill::StartNormalAnimation`
    /// with type 49 at the spell-cast speed, `Skill_Shapeshift::
    /// SetShapeshifted` @0x180505a70 right after the swap).
    pub fn play_transform(&mut self) {
        self.cast_anim = None;
        self.cast_type = AnimType::Transform;
        self.cast_speed = self.spell_speed;
        self.cast_kind = SpeedKind::Cast;
        self.state = CharState::Cast;
        self.playing = AnimType::None;
    }
}

/// A character record's animation set (`charAnimationTableName`, else the
/// record) for a hand state.
pub fn anim_set_for(data: &GameData, rec: &Record, hand: HandState) -> AnimSet {
    let table = rec
        .str("charAnimationTableName")
        .and_then(|t| data.db.get(t));
    AnimSet::load(&RecordIndex::new(table.unwrap_or(rec)), hand, |p| {
        data.vfs.exists(p)
    })
}

/// The animation state of a character of `record` (`rec`) with body mesh
/// `body`, its animation set (`charAnimationTableName`, else the record)
/// for the hand state of its weapons (`EquipManager::GetHandState`), at the
/// root's `scale` (`creature` module doc).
/// It starts idle; `GD_CHAR_STATE` / `GD_CHAR_HOLD` (hooks.rs) are applied by [`start_hooks`].
#[allow(clippy::too_many_arguments)]
pub fn character_for(
    data: &GameData,
    record: &str,
    rec: &Record,
    body: Arc<Msh>,
    right: Option<WeaponType>,
    left: Option<WeaponType>,
    is_player: bool,
    scale: f32,
    seed: u64,
) -> Character {
    let hand = HandState::from_weapons(right, left);
    let set = anim_set_for(data, rec, hand);
    Character::new(
        record,
        rec,
        is_player,
        hand,
        set,
        scale,
        body,
        CharState::Idle,
        None,
        seed,
    )
}

/// The start state of a new character ([`RenderHooks::char_state`]) and its attack hold (`GD_CHAR_HOLD`):
/// applied to every character the frame it appears, before it is driven.
fn start_hooks(hooks: Res<crate::hooks::Hooks>, mut new: Query<&mut Character, Added<Character>>) {
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
/// dispatches callbacks inside the advance, `AnimChannel::Update` @0x180038790 →
/// `SkillManager::HandleSkillAnimationCallback` @0x180533700). Also arms
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
        // `ControllerPlayer::Update` @0x18015d900 counts the attack-delay down with the frame's ms.
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
                // `AttackAction::AnimationCallback` @0x1800840f0 sets the flag the
                // action matrix's permission query reads.
                "AllowInterrupt" => c.allow_interrupt = true,
                // The evade's dash over: the recovery is cancelable
                // (`JumpToUseSkill::RequestSkillAction` @0x180168550 refuses only the warm-up).
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

impl Character {
    /// Animation time scale from debuffs for what is playing: the slow of the
    /// clip's [`SpeedKind`] (a weapon special the attack slow, a kick the
    /// attack slow, a charge the run slow), not its animation type's.
    pub fn time_scale(&self) -> f32 {
        if self.frozen {
            return 0.0;
        }
        match self.speed_kind {
            Some(SpeedKind::Run) => self.slow[0],
            Some(SpeedKind::Attack) => self.slow[1],
            Some(SpeedKind::Cast) => self.slow[2],
            None => 1.0,
        }
    }

    /// `Character::ForceSpeedUpdate` @0x180068ca0 → `AnimationSet::AdjustAnimation`
    /// @0x180024f20 → `AnimSlotRandom_Adjust` @0x180021420: the playing clip's
    /// rate follows a changed profile speed in place (caller speed × record
    /// speed on the current slot), no state re-entry needed. `combat::sync_speeds`
    /// calls it when the capped speeds change; a buff gained mid-run speeds the
    /// run clip up at once (the engine's `MoveToAction` / `SkillManager` /
    /// `CombatManager::ForceSpeedUpdate` callers).
    pub fn force_speed_update(&mut self) {
        let Some(kind) = self.speed_kind else {
            return;
        };
        if self.fixed_speed {
            return;
        }
        let caller = match kind {
            SpeedKind::Run => self.run_speed,
            SpeedKind::Attack => self.attack_speed,
            SpeedKind::Cast => self.spell_speed,
        };
        if let Some(cur) = &mut self.channel.current {
            cur.speed = caller * self.record_speed;
        }
    }

    /// Whether a new action may start now (the action matrix's permission 0 / 4,
    /// `CheckAction` @0x180127b90): idle or moving, or the playing action's recovery
    /// is cancelable — its `AllowInterrupt` crossed (an evade: its dash over).
    pub fn can_act(&self) -> bool {
        match self.state {
            CharState::Attack | CharState::Cast => self.allow_interrupt,
            CharState::Evade => !self.evade_warmup && self.allow_interrupt,
            CharState::Death
            | CharState::Stunned
            | CharState::TakeHit
            | CharState::KnockedDown
            | CharState::GettingUp => false,
            _ => true,
        }
    }

    /// The playing clip's remaining time (real ms, what the attack-delay counts down): the attack-delay a
    /// move's recovery cancel carries (`AttackAction::QueryActionPermission` @0x180084380 sets
    /// `GetRemainingAnimationTime`: Engine `AnimChannel::GetRemainingAnimationTime` @0x1800384c0 is
    /// (`GraphicsAnim::GetAnimSpeed` × 1000 − the clip time) ÷ the channel's speed, `GetAnimSpeed` being frame
    /// count ÷ fps — one frame past the final frame the "End" fires at). The port's channel speed is the clip's
    /// rate × its debuff slow ([`Self::time_scale`]); a held clip (rate 0) leaves none.
    pub fn remaining_ms(&self) -> f32 {
        self.channel.current.as_ref().map_or(0.0, |c| {
            let fps = c.anim.frame_rate as f32;
            let rate = c.speed * self.time_scale();
            if fps <= 0.0 || rate <= 0.0 {
                return 0.0;
            }
            let len = c.anim.num_frames as f32 * 1000.0 / fps;
            (len - c.time_ms).max(0.0) / rate
        })
    }

    /// Starts casting a skill of record class `class` with the special
    /// animation `special` (`SkillBook::special_animation`, or the record's
    /// `skillSpecialAnimationName` for monsters). False for classes that do
    /// not animate.
    pub fn start_cast(&mut self, class: &str, special: Option<String>) -> bool {
        let Some((ty, weapon)) = gd_sim::anim::skill_animation(class) else {
            return false;
        };
        self.cast_anim = special;
        self.cast_type = ty;
        self.cast_turns = anim::attack_turns(class);
        self.turn_types = &[];
        self.cast_speed = if weapon {
            self.attack_speed
        } else {
            self.spell_speed
        };
        self.cast_kind = if weapon {
            SpeedKind::Attack
        } else {
            SpeedKind::Cast
        };
        self.state = CharState::Cast;
        // The cast's callbacks come from the clips started from now on ([`Self::callbacks_after_cast`]).
        self.cast_after = self.plays;
        // Restart even when the same type is already playing.
        self.playing = AnimType::None;
        true
    }

    /// Whether the callbacks [`advance_animations`] sent this frame come from a clip started after the current
    /// cast began ([`Self::start_cast`]): only those fire its pending `crate::casting::Casting`. The ended
    /// action's "End" belongs to the old clip even when a requester started a new cast on it the same frame
    /// ([`finish_actions`] left the character idle before the requesters ran).
    pub fn callbacks_after_cast(&self) -> bool {
        self.sent_play > self.cast_after
    }

    /// A cast started (after [`Character::start_cast`]) at `point`, on `target` standing at its position (the
    /// character's facing is `tf`'s, before any turn): `AttackAction::Execute` turns the character to it while it
    /// casts (`crate::turning`), and a turning weapon class picks its `AttTurn*` clip from the angle to the target
    /// (`gd_sim::anim::attack_turn`).
    pub fn begin_cast(&mut self, tf: &Transform, target: Option<(Entity, Vec3)>, point: Vec3) {
        let turns = self.cast_turns;
        self.begin_attack(tf, target, point, turns);
    }

    /// [`Character::begin_cast`] for a plain weapon swing ([`CharState::Attack`]): it always may turn.
    pub fn begin_swing(&mut self, tf: &Transform, target: Option<(Entity, Vec3)>, point: Vec3) {
        self.begin_attack(tf, target, point, true);
    }

    /// A channelled skill starts ([`CharState::Channel`], or [`CharState::Channel2`]
    /// for `second`): the pair's `*Start` clip once, then its loop, until
    /// [`Character::end_channel`] (the requester's release). At the spell-cast
    /// speed: the channels are spells (`SkillChanneled` subclasses).
    pub fn begin_channel(&mut self, second: bool) {
        self.state = if second {
            CharState::Channel2
        } else {
            CharState::Channel
        };
        // Restart even when the same pair is already playing.
        self.playing = AnimType::None;
    }

    /// A whirlwind starts ([`CharState::Spin`]): `SpinStart` once, then the
    /// `Spin` loop at the attack speed (`Skill_AttackRadiusSpin::Update`
    /// @0x1804ca140 ticks at it), until [`Character::end_channel`].
    pub fn begin_spin(&mut self) {
        self.state = CharState::Spin;
        self.playing = AnimType::None;
    }

    /// The channel / whirlwind is over: back to the idle the requester's
    /// state machine picks (the loop's "End" only wraps, it does not leave).
    /// [`drive_characters`] calls it once the skill's `crate::casting::Channel` is gone.
    fn end_channel(&mut self) {
        if matches!(
            self.state,
            CharState::Channel | CharState::Channel2 | CharState::Spin
        ) {
            self.state = CharState::Idle;
            self.playing = AnimType::None;
        }
    }

    fn begin_attack(
        &mut self,
        tf: &Transform,
        target: Option<(Entity, Vec3)>,
        point: Vec3,
        turns: bool,
    ) {
        self.turn.attack(target.map(|t| t.0), point);
        let at = target.map_or(point, |t| t.1);
        let yaw = crate::turning::yaw(tf.rotation);
        self.turn_types = match anim::attack_turn_angle(yaw, (at - tf.translation).to_array()) {
            Some((angle, right)) if turns => anim::attack_turn(self.is_player, angle, right),
            _ => &[],
        };
    }

    /// Plays the first of `types` the set has (`StartNormalAnimation` per type), at `speed`, once.
    fn play_first(
        &mut self,
        data: &GameData,
        cache: &mut AnimCache,
        types: &[AnimType],
        speed: f32,
        kind: Option<SpeedKind>,
    ) -> bool {
        types
            .iter()
            .any(|t| self.play(data, cache, *t, speed, kind, false))
    }

    /// Knocked down (`crate::ragdoll`): `face_down` is how the body fell ([`Character::fell_face_down`]).
    pub fn knock_down(&mut self, face_down: bool) {
        self.fell_face_down = face_down;
        self.state = CharState::KnockedDown;
        self.cast_anim = None;
        self.playing = AnimType::None;
    }

    /// The knockdown is over: the get-up clip plays once ([`CharState::GettingUp`]).
    pub fn get_up(&mut self) {
        self.state = CharState::GettingUp;
        self.playing = AnimType::None;
    }

    /// The ground speed (m/s, world) the root motion of the move `state`'s slot (Run, Walk or Charge) carries the
    /// character at caller speed 1 ([`MoveMotion`]); 0 before [`finish_actions`] derived it, for a set without the
    /// slot, or for another state.
    pub fn move_motion(&self, state: CharState) -> f32 {
        let m = &self.motion;
        let v = match state {
            CharState::Run => m.run,
            CharState::Walk => m.walk,
            CharState::Charge => m.charge,
            _ => 0.0,
        };
        v * m.scale
    }

    /// The caller speed the move `state`'s clip plays at this frame ([`Self::move_rate`]) and whether it is fixed.
    fn move_caller(&self, state: CharState) -> (f32, bool) {
        match (self.move_rate, state) {
            (Some(r), _) => (r, true),
            (None, CharState::Charge) => (1.0, true),
            (None, _) => (self.run_speed, false),
        }
    }

    /// The caller speed of the playing move clip (the move state's own clip only, not one still playing from
    /// before the state was set): what its root motion ([`Self::root_velocity`]) was produced at.
    pub fn move_clip_rate(&self) -> Option<f32> {
        if !matches!(
            self.state,
            CharState::Run | CharState::Walk | CharState::Charge
        ) || !self.plays_own_clip()
            || self.record_speed <= 0.0
        {
            return None;
        }
        let cur = self.channel.current.as_ref()?;
        Some(cur.speed / self.record_speed)
    }

    /// The playing clip is the state's own animation ([`wanted_type`], or the move clip `enter_state` fell
    /// back to for it, [`Self::move_fallback`]).
    fn plays_own_clip(&self) -> bool {
        wanted_type(self.state, &self.anims).contains(&self.playing)
            || self.move_fallback == Some((self.state, self.playing))
    }

    /// Derives [`MoveMotion`] for the current animation set and body when they changed ([`finish_actions`]).
    fn derive_motion(&mut self, data: &GameData, cache: &mut AnimCache, scale: f32) {
        let set = Arc::as_ptr(&self.anims) as usize;
        let body = Arc::as_ptr(&self.body) as usize;
        self.motion.scale = scale;
        if self.motion.set == set && self.motion.body == body {
            return;
        }
        let root = anim::root_motion_bone(&self.body).unwrap_or("").to_owned();
        let slot_speed = |ty: AnimType, cache: &mut AnimCache| -> f32 {
            let entries = &self.anims.slot(ty).entries;
            if entries.is_empty() {
                return 0.0;
            }
            let sum: f32 = entries
                .iter()
                .map(|e| {
                    let Some(a) = cache.anim(data, &e.path) else {
                        return 0.0;
                    };
                    let (n, fps) = (a.num_frames as f32, a.frame_rate as f32);
                    let track = a.tracks.iter().find(|t| t.bone.eq_ignore_ascii_case(&root));
                    match track.and_then(|t| Some((t.keys.first()?, t.keys.last()?))) {
                        Some((k0, k1)) if n > 1.0 && fps > 0.0 => {
                            let secs = (n - 1.0) / fps;
                            let v = [0, 1, 2]
                                .map(|i| (k1.translation[i] - k0.translation[i]) / secs * e.speed);
                            let o = anim::root_velocity_object(&self.body, v, 1.0);
                            (o[0] * o[0] + o[2] * o[2]).sqrt()
                        }
                        _ => 0.0,
                    }
                })
                .sum();
            sum / entries.len() as f32
        };
        let run = slot_speed(AnimType::Run, cache);
        let walk = slot_speed(AnimType::Walk, cache);
        let charge = slot_speed(AnimType::Charge, cache);
        self.motion = MoveMotion {
            set,
            body,
            // A set without the slot moves on the other one (as `enter_state` plays it).
            run: if run > 0.0 { run } else { walk },
            walk: if walk > 0.0 { walk } else { run },
            charge,
            scale,
        };
    }

    /// `AnimationSet::PlayAnimationIfAvailable` 0x180024de0 for a quest
    /// action (`ScriptableAction_ControllerState::Execute`): plays
    /// the slot once when the animation set has it, else nothing happens.
    /// PORT: a dead character ignores it (the engine does not check).
    pub fn play_scripted(&mut self, ty: AnimType) -> bool {
        if ty == AnimType::None || self.state == CharState::Death || !self.anims.has(ty) {
            return false;
        }
        self.state = CharState::Scripted;
        self.scripted = ty;
        // Restart even when the same type is already playing.
        self.playing = AnimType::None;
        true
    }

    /// Keeps `anim` loaded while the character lives ([`Self::held_clips`]); a new animation set (a form, another
    /// hand state) starts the list again.
    fn hold_clip(&mut self, anim: &Arc<Animation>) {
        let set = Arc::as_ptr(&self.anims) as usize;
        let (held_set, clips) = &mut self.held_clips;
        if *held_set != set {
            *held_set = set;
            clips.clear();
        }
        if !clips.iter().any(|a| Arc::ptr_eq(a, anim)) {
            clips.push(anim.clone());
        }
    }

    /// Plays a slot: weighted variant pick, channel speed = caller speed ×
    /// record speed, blend per the engine rules. `kind`: the profile speed the
    /// clip runs at (its debuff slow, and what [`Character::force_speed_update`]
    /// re-derives); `None` for a clip at no profile speed.
    fn play(
        &mut self,
        data: &GameData,
        cache: &mut AnimCache,
        ty: AnimType,
        caller_speed: f32,
        kind: Option<SpeedKind>,
        looped: bool,
    ) -> bool {
        let r = self.rng.f32();
        let Some(e) = self.anims.pick(ty, r).cloned() else {
            return false;
        };
        let Some(anim) = cache.anim(data, &e.path) else {
            return false;
        };
        self.last_anim = e.path.clone();
        self.hold_clip(&anim);
        let blend = anim::blend_ms(
            self.playing,
            ty,
            self.anims.slot(ty).blend_ms,
            self.is_player,
        );
        self.channel
            .play(anim, looped, caller_speed * e.speed, blend);
        self.plays = self.plays.wrapping_add(1);
        self.playing = ty;
        self.speed_kind = kind;
        self.record_speed = e.speed;
        self.fixed_speed = false;
        // A new action's clip: the old one's interrupt point no longer applies.
        self.allow_interrupt = false;
        true
    }

    /// The `cast_anim` special (`AnimSet::specials`, by reference name).
    fn play_special(&mut self, data: &GameData, cache: &mut AnimCache, caller_speed: f32) -> bool {
        let Some(name) = self.cast_anim.clone() else {
            return false;
        };
        self.play_special_named(data, cache, &name, caller_speed, self.cast_kind)
    }

    /// A special animation by reference name (`AnimSet::specials`): the cast's
    /// own, or a weapon-pool proc's ([`Character::swing_special`]). Plays the
    /// `Special` slot once at `caller_speed` × the entry's record speed.
    fn play_special_named(
        &mut self,
        data: &GameData,
        cache: &mut AnimCache,
        name: &str,
        caller_speed: f32,
        kind: SpeedKind,
    ) -> bool {
        let r = self.rng.f32();
        let Some(e) = self
            .anims
            .specials
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .and_then(|(_, slot)| slot.pick(r).cloned())
        else {
            return false;
        };
        let Some(anim) = cache.anim(data, &e.path) else {
            return false;
        };
        self.last_anim = e.path.clone();
        self.hold_clip(&anim);
        let blend = anim::blend_ms(self.playing, AnimType::Special, 200.0, self.is_player);
        self.channel
            .play(anim, false, caller_speed * e.speed, blend);
        self.plays = self.plays.wrapping_add(1);
        self.playing = AnimType::Special;
        self.speed_kind = Some(kind);
        self.record_speed = e.speed;
        self.fixed_speed = false;
        self.allow_interrupt = false;
        true
    }

    fn enter_state(&mut self, data: &GameData, cache: &mut AnimCache) {
        self.move_fallback = None;
        let ok = match self.state {
            CharState::Idle => {
                self.idle_playing = self.idle;
                let first = match self.idle {
                    IdleMode::Combat => false,
                    IdleMode::Long => {
                        self.play(data, cache, AnimType::IdleTransition, 1.0, None, false)
                            || self.play_long_idle(data, cache)
                    }
                    IdleMode::Chat => {
                        self.play(data, cache, AnimType::Chat, 1.0, None, false)
                            || self.play(data, cache, AnimType::Fidget, 1.0, None, false)
                            || self.play_long_idle(data, cache)
                    }
                    IdleMode::Menu => self.play_menu_idle(data, cache),
                };
                first
                    || self.play(data, cache, AnimType::AttackIdle, 1.0, None, true)
                    || self.play(data, cache, AnimType::LongIdle, 1.0, None, true)
            }
            // The move clips play at the mover's caller speed ([`Character::move_rate`]).
            CharState::Run | CharState::Walk => {
                let (s, fixed) = self.move_caller(self.state);
                let kind = Some(SpeedKind::Run);
                let (a, b) = if self.state == CharState::Run {
                    (AnimType::Run, AnimType::Walk)
                } else {
                    (AnimType::Walk, AnimType::Run)
                };
                let own = self.play(data, cache, a, s, kind, true);
                let ok = own || self.play(data, cache, b, s, kind, true);
                // The other clip stands in when the own one would not play (an unreadable `.anm` the set
                // still names): it is the state's clip, not re-entered every frame.
                if ok && !own {
                    self.move_fallback = Some((self.state, b));
                }
                self.fixed_speed = ok && fixed;
                ok
            }
            // `Character::MoveTo` @0x18005f870 plays the type with
            // `PlayAnimationIfAvailable` and no fallback; the caller
            // (`monster_ai`) does not move a character without the slot.
            // The charge is a move: the run slow scales it.
            CharState::Charge => {
                let (s, fixed) = self.move_caller(CharState::Charge);
                let ok = self.play(data, cache, AnimType::Charge, s, Some(SpeedKind::Run), true);
                self.fixed_speed = ok && fixed;
                ok
            }
            // A weapon-pool proc plays the picked skill's special animation
            // (`SkillActivatedWeaponPool::StartAction` @0x18051bb90
            // runs that skill's own `StartAction`, which plays its
            // `skillSpecialAnimationName`; the pool classes do not turn);
            // without one, the plain swing: `SkillActivatedWeapon::StartAction`,
            // the turn clip first.
            CharState::Attack => {
                let s = self.attack_speed;
                let kind = Some(SpeedKind::Attack);
                match self.swing_special.clone() {
                    Some(name) => {
                        self.play_special_named(data, cache, &name, s, SpeedKind::Attack)
                            || self.play(data, cache, AnimType::Attack, s, kind, false)
                    }
                    None => {
                        let turns = self.turn_types;
                        self.play_first(data, cache, turns, s, kind)
                            || self.play(data, cache, AnimType::Attack, s, kind, false)
                    }
                }
            }
            // Skill::StartAction: the special animation, else the class's
            // type (a turning weapon class: its turn clip first); SpellCast/Attack only as a fallback for creatures
            // without that slot. PORT: a player's missing turn clip falls back to the type (the engine's attack
            // fails; no stock table lacks one).
            CharState::Cast => {
                let (s, ty, kind) = (self.cast_speed, self.cast_type, Some(self.cast_kind));
                let turns = if self.cast_turns {
                    self.turn_types
                } else {
                    &[]
                };
                self.play_special(data, cache, s)
                    || self.play_first(data, cache, turns, s, kind)
                    || self.play(data, cache, ty, s, kind, false)
                    || self.play(data, cache, AnimType::SpellCast, s, kind, false)
                    || self.play(data, cache, AnimType::Attack, s, kind, false)
            }
            // The channel pairs and the whirlwind (module doc): the `*Start`
            // clip once, its "End" starts the loop ([`finish_actions`]); no
            // start clip: the loop at once. Channels are spells (cast speed),
            // the whirlwind a weapon skill (attack speed).
            CharState::Channel => {
                let s = self.spell_speed;
                let kind = Some(SpeedKind::Cast);
                self.play(data, cache, AnimType::ChannelStart, s, kind, false)
                    || self.play(data, cache, AnimType::Channel, s, kind, true)
            }
            CharState::Channel2 => {
                let s = self.spell_speed;
                let kind = Some(SpeedKind::Cast);
                self.play(data, cache, AnimType::Channel2Start, s, kind, false)
                    || self.play(data, cache, AnimType::Channel2, s, kind, true)
            }
            CharState::Spin => {
                let s = self.attack_speed;
                let kind = Some(SpeedKind::Attack);
                self.play(data, cache, AnimType::SpinStart, s, kind, false)
                    || self.play(data, cache, AnimType::Spin, s, kind, true)
            }
            CharState::Stunned => {
                self.play(data, cache, AnimType::Stun, 1.0, None, true)
                    || self.play(data, cache, AnimType::AttackIdle, 1.0, None, true)
            }
            // `Character::ExecuteTakeHit` @0x18005d650 plays type 46 once.
            CharState::TakeHit => self.play(data, cache, AnimType::TakeHit, 1.0, None, false),
            CharState::Pickup => self.play(data, cache, AnimType::Pickup, 1.0, None, false),
            CharState::Death => self.play(data, cache, AnimType::Die, 1.0, None, false),
            // The slot's own speed: the evade action's playback rate is not
            // traced (the action object's animation call is virtual).
            CharState::Evade => self.play(data, cache, AnimType::Evade, 1.0, None, false),
            // `PlayAnimationAction` (type 37, speed 1.0),
            // `ControllerMonsterStateFlee::OnBegin`.
            CharState::Roar => self.play(data, cache, AnimType::Flee, 1.0, None, false),
            CharState::Alert => self.play(data, cache, AnimType::Alert, 1.0, None, false),
            CharState::KnockedDown | CharState::GettingUp => {
                let ty = if self.fell_face_down {
                    AnimType::GetUpFaceDown
                } else {
                    AnimType::GetUpFaceUp
                };
                let down = self.state == CharState::KnockedDown;
                self.play(data, cache, ty, if down { 0.0 } else { 1.0 }, None, false)
                    || (down
                        && (self.play(data, cache, AnimType::Stun, 1.0, None, true)
                            || self.play(data, cache, AnimType::AttackIdle, 1.0, None, true)))
            }
            CharState::Scripted => {
                let ty = self.scripted;
                self.play(data, cache, ty, 1.0, None, false)
            }
        };
        if ok && self.state == CharState::Attack && self.playing != AnimType::Attack {
            debug!(
                "{}: swing turns: {:?} {}",
                self.record, self.playing, self.last_anim
            );
        }
        if ok && self.state == CharState::Cast {
            debug!(
                "{}: cast {:?} special {:?} -> {:?} {}",
                self.record, self.cast_type, self.cast_anim, self.playing, self.last_anim
            );
        }
        if !ok && self.state == CharState::Evade {
            // No evade animation for this weapon style: nothing to jump with.
            warn!("{}: no evade animation", self.record);
            self.state = CharState::Idle;
            return;
        }
        if !ok && self.state == CharState::Scripted {
            self.state = CharState::Idle;
            return;
        }
        if !ok
            && matches!(
                self.state,
                CharState::Channel
                    | CharState::Channel2
                    | CharState::Spin
                    | CharState::TakeHit
                    | CharState::Pickup
                    | CharState::GettingUp
            )
        {
            // No clip for the action: it is over at once (no "End" would leave it).
            self.state = CharState::Idle;
            return;
        }
        if !ok && matches!(self.state, CharState::Roar | CharState::Alert) {
            // The slot's animation is unusable: no roar / alert, the flee / pursuit goes on.
            self.state = CharState::Idle;
            return;
        }
        if !ok && matches!(self.state, CharState::Attack | CharState::Cast) {
            // No usable clip, so no "End" would ever leave the state (the AIs wait for it): the swing / cast is
            // over at once. PORT: the engine's skill action without an animation was not decompiled.
            warn!("{}: no animation for {:?}", self.record, self.state);
            self.state = CharState::Idle;
            self.cast_anim = None;
            return;
        }
        if !ok {
            // A record with no idle clip at all (the Crucible turrets name only attack / buff / spawn
            // anims, no `unarmedAttackIdleAnim` / `LongIdle`) stands still in its pose: data, not a fault.
            if self.state == CharState::Idle {
                debug!("{}: no idle animation", self.record);
            } else {
                warn!("{}: no animation for {:?}", self.record, self.state);
            }
            // Mark the state as entered so it is not retried every frame.
            self.playing = wanted_type(self.state, &self.anims)[0];
        }
    }
}

impl Character {
    /// `ResetTransitionTimer` and the looped `LongIdle` of a long / chat idle.
    fn play_long_idle(&mut self, data: &GameData, cache: &mut AnimCache) -> bool {
        self.reset_transition_timer();
        self.play(data, cache, AnimType::LongIdle, 1.0, None, true)
    }

    /// `ResetTransitionTimer` and the looped `MenuIdle` (`MenuIdleAction::Execute`).
    fn play_menu_idle(&mut self, data: &GameData, cache: &mut AnimCache) -> bool {
        self.reset_transition_timer();
        self.play(data, cache, AnimType::MenuIdle, 1.0, None, true)
    }

    /// `ResetTransitionTimer` @0x1800824b0.
    fn reset_transition_timer(&mut self) {
        let (lo, hi) = FIDGET_WAIT_MS;
        self.idle_timer = (0.0, lo + self.rng.f32() * (hi - lo));
    }

    /// An idle animation's "End" (module doc): the idle action's `AnimationCallback`.
    fn idle_end(&mut self, data: &GameData, cache: &mut AnimCache, t: AnimType) {
        match (self.idle_playing, t) {
            // IdleAction::AnimationCallback: re-pick the idle each cycle.
            (IdleMode::Combat, AnimType::AttackIdle) => {
                self.play(data, cache, AnimType::AttackIdle, 1.0, None, true);
            }
            (
                IdleMode::Long | IdleMode::Chat,
                AnimType::IdleTransition | AnimType::Fidget | AnimType::Chat,
            ) => {
                if !self.play_long_idle(data, cache) {
                    self.play(data, cache, AnimType::AttackIdle, 1.0, None, true);
                }
            }
            (IdleMode::Long, AnimType::LongIdle) if self.idle_timer.0 >= self.idle_timer.1 => {
                if !self.play(data, cache, AnimType::Fidget, 1.0, None, false) {
                    self.play_long_idle(data, cache);
                }
            }
            (IdleMode::Chat, AnimType::LongIdle) => {
                self.play(data, cache, AnimType::Fidget, 1.0, None, false);
            }
            // `MenuIdleAction::AnimationCallback`: phase 1 (idle) and phase 2 (fidget done).
            (IdleMode::Menu, AnimType::MenuIdle) if self.idle_timer.0 >= self.idle_timer.1 => {
                if !self.play(data, cache, AnimType::MenuFidget, 1.0, None, false) {
                    self.reset_transition_timer();
                }
            }
            (IdleMode::Menu, AnimType::MenuFidget) => {
                self.play_menu_idle(data, cache);
            }
            _ => {}
        }
    }
}

/// `ControllerPlayerStateIdle::OnUpdate` / `ControllerPlayerStateLongIdle::Attacked` (module doc): the player turns
/// to the long idle after [`LONG_IDLE_AFTER_MS`] idling out of combat, and back to the combat idle when it fights.
pub fn player_long_idle(
    time: Res<Time>,
    mut players: Query<(&mut Character, Option<&Constitution>), With<crate::player::Player>>,
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
/// (`ChatAction::FaceTarget` @0x1800871d0: yaw only, when they are at least 1 m apart and the record's
/// `rotateWhenChatting`, default on, allows it).
pub fn npc_chat(
    conv: Res<crate::conversation::Conversation>,
    data: Res<GameData>,
    players: Query<&Transform, With<crate::player::Player>>,
    mut npcs: Query<
        (&crate::story::StoryActor, &mut Character, &Transform),
        Without<crate::player::Player>,
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

/// The clips that are `s`'s own animation. A move state's other move clip counts only when `anims` lacks its
/// own slot (the fallback `enter_state` plays): a Run → Walk switch must start the Walk clip, not keep the Run
/// one at the walk's caller speed (the ground speed is the playing clip's root motion).
fn wanted_type(s: CharState, anims: &AnimSet) -> &'static [AnimType] {
    match s {
        CharState::Idle => &[
            AnimType::AttackIdle,
            AnimType::LongIdle,
            AnimType::IdleTransition,
            AnimType::Fidget,
            AnimType::Chat,
            AnimType::MenuIdle,
            AnimType::MenuFidget,
        ],
        CharState::Run if anims.has(AnimType::Run) => &[AnimType::Run],
        CharState::Run => &[AnimType::Walk],
        CharState::Walk if anims.has(AnimType::Walk) => &[AnimType::Walk],
        CharState::Walk => &[AnimType::Run],
        CharState::Charge => &[AnimType::Charge],
        CharState::Attack => &[
            AnimType::Attack,
            AnimType::AttTurnRight90,
            AnimType::AttTurnLeft90,
            AnimType::AttTurnRight180,
            AnimType::AttTurnLeft180,
        ],
        CharState::Cast => &[
            AnimType::Special,
            AnimType::AttTurnRight90,
            AnimType::AttTurnLeft90,
            AnimType::AttTurnRight180,
            AnimType::AttTurnLeft180,
            AnimType::BuffSelf,
            AnimType::BuffOther,
            AnimType::SpellCast,
            AnimType::Attack,
            // The kick (`Skill_Kick`, `crate::casting::DEFAULT_KICK`).
            AnimType::Kick,
            AnimType::Transform,
        ],
        CharState::Channel => &[AnimType::ChannelStart, AnimType::Channel],
        CharState::Channel2 => &[AnimType::Channel2Start, AnimType::Channel2],
        CharState::Spin => &[AnimType::SpinStart, AnimType::Spin],
        CharState::Stunned => &[AnimType::Stun, AnimType::AttackIdle],
        CharState::TakeHit => &[AnimType::TakeHit],
        CharState::Pickup => &[AnimType::Pickup],
        CharState::Death => &[AnimType::Die],
        // Only the Evade animation: its `StartJump` / `StopJump` / `End`
        // drive the evade (`evade.rs`), so a playing Run must not count.
        CharState::Evade => &[AnimType::Evade],
        CharState::Roar => &[AnimType::Flee],
        CharState::Alert => &[AnimType::Alert],
        CharState::KnockedDown => &[
            AnimType::GetUpFaceDown,
            AnimType::GetUpFaceUp,
            AnimType::Stun,
            AnimType::AttackIdle,
        ],
        CharState::GettingUp => &[AnimType::GetUpFaceDown, AnimType::GetUpFaceUp],
        CharState::Scripted => &[],
    }
}

/// Dispatches the animations' "End" callbacks: finishes the ended actions before
/// the frame's requesters run — the engine's `CharacterHandlerUpdate` @0x1801279f0
/// shape (finish ended actions → run pending → animate), so a swing that ends this
/// frame chains into the next in the same pass, with no rendered idle frame between.
pub fn finish_actions(
    hooks: Res<crate::hooks::Hooks>,
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
/// - A channel / whirlwind state whose skill stopped (its `crate::casting::Channel` gone: released, out of
///   energy, a new action, a stun, an evade) is over ([`Character::end_channel`]): one place for every stop.
/// - A move clip already playing whose mover now passes another caller speed ([`Character::move_rate`]) takes
///   it in place. PORT: `Character::MoveTo` re-plays the clip (a new cycle, blended); the port keeps the cycle.
pub fn drive_characters(
    data: Res<GameData>,
    mut cache: ResMut<AnimCache>,
    mut chars: Query<(&mut Character, Has<crate::casting::Channel>)>,
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

/// Where other modules order against the character systems: `Animate` advances the animations and
/// dispatches their callbacks (`Update`, before the gameplay readers — hit point → damage is one
/// tick, as the engine's inline dispatch); `Drive` runs the state machines after the frame's
/// requesters (a state set this frame animates this frame); `Turn` turns the characters
/// (`crate::turning`, `PostUpdate` before transform propagation, reading the same frame's callbacks).
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CharacterStage {
    Drive,
    Turn,
    Animate,
}
