//! Character animation state (players and creatures), following the engine: `gd_sim::anim` sets by hand
//! state, one channel with 200 ms cross-fades, root motion and callback points.
//!
//! The per-frame pipeline is the engine's one-tick action loop (`CharacterHandlerUpdate`: finish ended
//! actions → run pending → animate): `advance_animations` advances every clip and sends its callback points
//! ([`AnimCallback`]) and root-motion velocity before the gameplay readers; `finish_actions` drains the
//! "End"s so the frame's requesters see finished actions; `drive_characters` then plays the animation for
//! every [`CharState`] set this frame. A request mid-action waits in the one pending slot
//! ([`Character::queued`], last wins) until the clip's `AllowInterrupt` point or "End"; a move's cancel
//! leaves the remainder as [`Character::attack_delay`] so the cadence is kept.
//!
//! Cast cadence: a spell plays its slot at `GetSpellCastSpeed` (bio `characterSpellCastSpeed` × the sheet's
//! cast-speed multiplier, capped; `AnimEntry::Play` multiplies the record speed), and a held skill re-casts
//! only after the clip's "End" (never cancelled at `AllowInterrupt`): one cast per (frames − 1) ÷ fps ÷ speed.
//! The pose's bone frames are computed once ([`Character::posed`]) for hit bounds, body parts, weapons,
//! attach points and effects.
//!
//! Facing turns via [`Character::turn`]; [`Character::begin_cast`] / [`Character::begin_swing`] also pick the
//! `AttTurn*` clip a turning weapon attack starts with. A weapon-pool proc plays the picked skill's special
//! animation ([`Character::swing_special`]; `SkillActivatedWeaponPool::StartAction`) with that clip's hit
//! point. Other states: channel pairs ([`CharState::Channel`] / [`CharState::Channel2`]: `*Start` once, then
//! the loop until released), [`CharState::Spin`], [`CharState::Pickup`], [`CharState::TakeHit`] (slots
//! 40–46 of the 52-slot table, `GetAnimTypeAsText`), [`CharState::Alert`] (slot 33), and
//! [`CharState::KnockedDown`] → [`CharState::GettingUp`] (slots 38 / 39).
//!
//! The Run / Walk / Charge slots' own root-motion speed ([`Character::move_motion`]) is what a mover runs at
//! before its clip produces root motion. A move clip plays at its mover's caller speed
//! ([`Character::move_rate`]) so the ground speed is the clip's own motion. A profile speed change reaches
//! the playing clip in place ([`Character::force_speed_update`], `Character::ForceSpeedUpdate` →
//! `AnimationSet::AdjustAnimation`); the clip's [`SpeedKind`] picks which debuff slow scales it.
//!
//! An idle character plays its [`IdleMode`] (Game.dll controller states pick the idle action):
//! - Combat (`IdleAction`): `AttackIdle`, re-picked at each "End". The default.
//! - Long (`LongIdleAction`): `IdleTransition` once, then `LongIdle` looped; at a `LongIdle` "End" once the
//!   transition timer (random 3000..20000 ms, `ResetTransitionTimer`) is up, one `Fidget`, then `LongIdle`
//!   with a new timer. No transition clip: `LongIdle` at once. `ControllerPlayerStateIdle::OnUpdate` enters
//!   it after 2000 ms idle out of combat (restarting while `Character::IsUnderAttack`); any other state
//!   leaves it.
//! - Chat (`ChatAction::Execute`): an NPC with an open conversation plays `Chat` once (else `Fidget`, else
//!   `LongIdle`), then `LongIdle` and `Fidget` in turn (no timer check).
//! - PORT: a long or chat idle without any of its clips plays `AttackIdle` (the engine keeps the last
//!   animation); `LongIdle` loops in the channel.
//! - Menu (`MenuIdleAction`): the `LongIdleAction` twin on `MenuIdle` / `MenuFidget` (no fidget clip: the
//!   timer restarts). Every switch happens at an "End", so the clip never restarts mid-loop.
use crate::data::GameData;
use bevy::prelude::*;
use gd_formats::anm::Animation;
use gd_formats::arz::Record;
use gd_formats::msh::{Coords, Mesh as Msh};
use gd_sim::anim::{self, AnimChannel, AnimSet, AnimType, Pose, SpeedKind};
use gd_sim::equip::{HandState, WeaponType};
use std::sync::Arc;

/// The way a character or object faces, a unit vector. Characters face +Z (the engine's yaw), while
/// Bevy's `Transform::forward()` is -Z; every gameplay facing goes through here.
pub fn facing(tf: &Transform) -> Vec3 {
    tf.rotation * Vec3::Z
}

/// [`facing`] on the ground plane (+Z when it points straight up or down).
pub fn facing_flat(tf: &Transform) -> Vec3 {
    facing(tf).with_y(0.0).normalize_or(Vec3::Z)
}

/// The parsed-`.anm` store shared by gd-game's [`AnimCache`] and gd-render's `ModelCache`. A clip lives
/// while a holder does ([`crate::weak_cache`]).
pub type SharedAnims = Arc<parking_lot::Mutex<crate::weak_cache::WeakCache<Animation>>>;

/// Parsed `.anm` animations by normalized path. The last [`RECENT_ANIMS`] clips used stay parsed.
#[derive(Resource)]
pub struct AnimCache {
    pub anims: SharedAnims,
    /// New characters' clip sets being parsed in the background: the character, the clips already
    /// parsed, and the task parsing the rest.
    pub preloads: Vec<(
        Entity,
        Vec<Arc<Animation>>,
        bevy::tasks::Task<Vec<(String, Option<Animation>)>>,
    )>,
    /// Clips parsed in the background ahead of a spawn ([`AnimCache::ready`]).
    pub loading: Vec<bevy::tasks::Task<(String, Option<Animation>)>>,
    /// The paths of `loading`.
    pub in_flight: gd_formats::collections::HashSet<String>,
}

const RECENT_ANIMS: usize = 1024;

impl Default for AnimCache {
    fn default() -> Self {
        Self {
            anims: Arc::new(parking_lot::Mutex::new(
                crate::weak_cache::WeakCache::with_recent(RECENT_ANIMS),
            )),
            preloads: Vec::new(),
            loading: Vec::new(),
            in_flight: Default::default(),
        }
    }
}

impl AnimCache {
    /// Cached animations still in use.
    pub fn count(&self) -> usize {
        self.anims.lock().live()
    }

    pub fn shared(&self) -> SharedAnims {
        self.anims.clone()
    }

    pub fn anim(&mut self, data: &GameData, path: &str) -> Option<Arc<Animation>> {
        shared_anim(&self.anims, data, path)
    }

    /// Whether the clip at `path` is parsed (or failed); otherwise its background parse starts and this
    /// is false, so a waiting spawn never parses a clip on the game thread.
    pub fn ready(&mut self, data: &GameData, path: &str) -> bool {
        let key = gd_formats::vfs::normalized(path);
        if self.anims.lock().get_kept(&key).is_some() {
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

/// Every clip a creature of `record` (`variant`) plays, as [`character_for`] builds its set.
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

/// Every clip of a character's animation set, held while it lives.
#[derive(Component)]
pub struct HeldAnims {
    pub _clips: Vec<Arc<Animation>>,
}

/// [`AnimCache::anim`] on a [`SharedAnims`] store. The store is not locked during the read and parse
/// (other threads share it).
pub fn shared_anim(anims: &SharedAnims, data: &GameData, path: &str) -> Option<Arc<Animation>> {
    let key = gd_formats::vfs::normalized(path);
    if let Some(hit) = anims.lock().get_kept(&key) {
        return hit;
    }
    let a = data
        .vfs
        .read(&key)
        .and_then(|b| Animation::parse(&b))
        .inspect_err(|e| warn!("animation {key}: {e:#}"))
        .ok();
    let mut store = anims.lock();
    if let Some(hit) = store.get_kept(&key) {
        return hit;
    }
    store.put(&key, a)
}

/// An animation callback point crossed this frame (e.g. "RightHandHit", "End").
/// `cast`: the character was casting when the point crossed (the same step's "End" may already have
/// left [`CharState::Cast`]).
#[derive(Message, Clone, Debug)]
pub struct AnimCallback {
    pub entity: Entity,
    pub anim: AnimType,
    pub name: String,
    pub cast: bool,
}

/// A plain move request queued while busy ([`Character::queued`]).
#[derive(Clone, Copy, Debug)]
pub struct MoveOrder {
    pub to: Option<Vec3>,
    pub ground: Option<Vec3>,
    pub camera: Vec3,
}

/// The single pending-action slot (`ControllerBaseCharacter.queuedAction`): the last request while busy
/// runs at the current action's `AllowInterrupt` point or "End" (`AttackAction::QueryActionPermission`).
/// PORT: the player's requests only.
#[derive(Clone, Debug)]
pub enum QueuedAction {
    Cast(crate::casting::CastRequest),
    Move(MoveOrder),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharState {
    Idle,
    Run,
    /// A `WalkAction` (`Character::WalkTo` sets animation state 6).
    Walk,
    /// A charge skill's approach (`ControllerMonsterStatePursue::OnBegin`: animation type 7): the Charge
    /// slot, looped.
    Charge,
    Attack,
    /// `cast_anim` names the special animation (`skillSpecialAnimationName`), else `cast_type` plays.
    Cast,
    /// A channelled skill (`SkillChanneled`): `ChannelStart` once, then the `Channel` loop until the
    /// skill stops.
    Channel,
    /// The second channel pair (`Channel2Start` / `Channel2`).
    Channel2,
    /// A whirlwind (`Skill_AttackRadiusSpin`): `SpinStart` once, then `Spin` at the attack speed until
    /// released.
    Spin,
    /// Stunned / asleep / knocked down: the stun loop.
    Stunned,
    /// A flinch (CC type 49 → `Character::ExecuteTakeHit`, animation type 46): `TakeHit` once; ends with
    /// its "End".
    TakeHit,
    /// The `Pickup` slot (animation type 8) once; ends with its "End".
    Pickup,
    Death,
    /// The Evade slot (animation type 48, `Skill_Evade::StartAction`).
    Evade,
    /// The flee roar: the Flee slot (animation type 37) once; ends with its "End".
    Roar,
    /// `ControllerMonsterStateAlertBeforePursue::OnBegin`: the Alert slot (33) once; ends with its "End".
    Alert,
    /// `Character::ExecuteKnockdown`: the first frame of the get-up clip, held while down.
    KnockedDown,
    /// `ControllerMonsterStateGettingUp::OnUpdate`: `GetUpFaceDown` (38) or `GetUpFaceUp` (39) once;
    /// ends with its "End".
    GettingUp,
    /// A quest `ControllerState` action ([`Character::play_scripted`]): the [`Character::scripted`] slot
    /// once, then idle.
    Scripted,
    /// `Character.PlayAnimation(anim, hold=true, allow)`: the [`Character::scripted`] slot once, its last
    /// frame held until another action takes over; the monster AI stays off unless `allow`.
    ScriptedHold,
}

/// The controller's idle action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IdleMode {
    /// `IdleAction`.
    #[default]
    Combat,
    /// `LongIdleAction`.
    Long,
    /// `ChatAction`.
    Chat,
    /// `MenuIdleAction`.
    Menu,
}

/// `ControllerPlayerStateIdle`: ms idle out of combat before the long idle.
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
    /// Animation type in the channel.
    pub playing: AnimType,
    /// The move state whose own clip would not play (unreadable `.anm`) and the move clip played instead,
    /// until the state is entered again.
    pub move_fallback: Option<(CharState, AnimType)>,
    pub state: CharState,
    /// What the character plays while [`CharState::Idle`]; any other state resets it to
    /// [`IdleMode::Combat`].
    pub idle: IdleMode,
    /// The idle mode whose cycle is playing (a change restarts the idle).
    pub idle_playing: IdleMode,
    /// The transition timer of a long / chat idle: ms elapsed, ms due.
    pub idle_timer: (f32, f32),
    /// Ms in [`CharState::Idle`] (the player's long-idle timer).
    pub idle_ms: f32,
    pub cast_anim: Option<String>,
    pub scripted: AnimType,
    /// The cast's animation without a special one (`gd_sim::anim::skill_animation`), its speed and kind.
    pub cast_type: AnimType,
    pub cast_speed: f32,
    pub cast_kind: SpeedKind,
    /// The cast turns into its attack (`gd_sim::anim::attack_turns`).
    pub cast_turns: bool,
    /// A weapon-pool proc's special animation (`skillSpecialAnimationName` of the picked skill): the
    /// [`CharState::Attack`] plays it instead of the plain clip, with its `RightHandHit` as the hit point.
    /// Cleared when the attack ends.
    pub swing_special: Option<String>,
    /// The `AttTurn*` slots an attack tries first ([`Character::begin_attack`]).
    pub turn_types: &'static [AnimType],
    /// The one writer of its facing.
    pub turn: crate::turning::Turn,
    /// Run animation rate: the record's `characterRunSpeed` (`CharAttributeStore_Bio::Load`) at spawn,
    /// then the capped profile speed (`combat::sync_speeds`; `Character::MoveTo` plays at `GetRunSpeed`).
    pub run_speed: f32,
    /// Attack animation rate (as `run_speed`).
    pub attack_speed: f32,
    /// Spell/buff animation rate (as `run_speed`).
    pub spell_speed: f32,
    pub scale: f32,
    /// Record `mesh`: skeleton and attach points.
    pub body: Arc<Msh>,
    /// World-space root-motion velocity of the last frame (m/s).
    pub root_velocity: Vec3,
    /// Freeze attacks at this fraction of their length (test hook).
    pub hold: Option<f32>,
    /// Debuff slows as time scales: run, attack, cast (1 = normal).
    pub slow: [f32; 3],
    /// Frozen or petrified: the pose stops.
    pub frozen: bool,
    /// The pose computed this frame (`advance_animations`).
    pub pose: Pose,
    /// [`Self::body`]'s bone frames in [`Self::pose`] (mesh space). PORT: refreshed lazily — the
    /// animation advance invalidates it ([`Self::mark_pose_stale`]) and the first reader of the frame
    /// rebuilds it ([`Self::posed`]); a character nothing reads (off-screen in a crowd) never pays the
    /// absolute-pose walk. The lock keeps every reader immutable (parallel queries included).
    pub posed: std::sync::OnceLock<Vec<Coords>>,
    /// A worn replacement look ([`Character::set_form`]); `body` and `anims` are the form's while on.
    pub form: Option<FormLook>,
    pub rng: fastrand::Rng,
    /// The playing clip's speed kind (`None`: no debuff slow scales it).
    pub speed_kind: Option<SpeedKind>,
    /// The playing clip's record speed; with the caller speed it re-derives the channel rate.
    pub record_speed: f32,
    /// The clip plays at a fixed caller speed ([`Self::move_rate`]), unaffected by profile speed changes.
    /// PORT: `Character::ForceSpeedUpdate` re-adjusts a playing move clip to `GetRunSpeed` until the
    /// mover's next `MoveTo`; the port keeps the mover's factor.
    pub fixed_speed: bool,
    /// This frame's caller speed of the move clip (`Character::MoveTo` → `PlayAnimationIfAvailable`; a
    /// different factor re-plays the clip): `None` is the run speed; unit pace and a charge pass 1.0, a walk
    /// `walkSpeed` × the run speed, a warmed-up charge the warm-up run speed. Reset each frame by
    /// `finish_actions`.
    pub move_rate: Option<f32>,
    pub pending: Vec<(AnimType, String)>,
    /// The callback points crossed this frame, until sent.
    pub crossed: Vec<String>,
    /// The entity events crossed this frame, until gd-render drains them (replaced each advance, so a
    /// headless game does not accumulate them).
    pub entity_events: Vec<anim::AnimEntityEvent>,
    /// `.anm` of the last started animation (for logs).
    pub last_anim: String,
    pub queued: Option<QueuedAction>,
    /// The playing clip's `AllowInterrupt` callback crossed (`AttackAction::AnimationCallback`): a new or
    /// queued action may cancel its recovery. Cleared when a clip starts; an evade arms it at `StopJump`.
    pub allow_interrupt: bool,
    /// `ControllerPlayer.attackDelay`: the cancelled remainder (ms) of a swing / cast a move interrupted;
    /// a new skill use waits for it, so recovery-cancelling does not raise the attack speed.
    pub attack_delay: f32,
    /// The evade's dash is up: skill / move requests are refused only while it is
    /// (`ControllerPlayerStateJumpToUseSkill::RequestSkillAction`). Cleared at the clip's `StopJump`.
    pub evade_warmup: bool,
    /// `Actor.fellFaceDown`: picks the get-up clip.
    pub fell_face_down: bool,
    pub motion: MoveMotion,
    /// The clips played from the current set (`Arc<AnimSet>` address, clips), held so the weak anim cache
    /// keeps them parsed between plays.
    pub held_clips: (usize, Vec<Arc<Animation>>),
    /// Clips started so far: each play's number.
    pub plays: u32,
    /// [`Self::plays`] when the current cast started.
    pub cast_after: u32,
    /// The play number whose callbacks were sent this frame.
    pub sent_play: u32,
}

/// The ground speed (m/s at caller speed 1, world scale included) the root motion of the Run, Walk and
/// Charge slots carries: the mean over the slot's variants of root displacement over length × record speed.
/// Lets a mover have the clip's speed before the clip played a frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct MoveMotion {
    /// The `Arc<AnimSet>` and body mesh addresses it was derived for (0: not yet).
    pub set: usize,
    pub body: usize,
    /// Object-space speeds and the world scale.
    pub run: f32,
    pub walk: f32,
    pub charge: f32,
    scale: f32,
}

impl Character {
    /// `seed` seeds its animation picks.
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
            posed: std::sync::OnceLock::from(anim::pose_abs(&body, &Pose::default())),
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

/// A replacement look: the engine's override slot 0 (`Character::SetOverrideMesh` /
/// `SetOverrideAnimations`). The own body and set are kept to come back to.
pub struct FormLook {
    pub mesh: String,
    pub records: FormRecords,
    pub own_body: Arc<Msh>,
    pub own_anims: Arc<AnimSet>,
}

/// The rest of a form's look (`Skill_Shapeshift::SetShapeshifted`): `replacementSounds`
/// (`Character::SetOverrideSounds`), `replacementFootsteps` (`SetOverrideFootsteps`) and `attackTrail`
/// (`SetUnarmedWeaponTrail`). Empty: none (the own entries apply).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormRecords {
    pub sounds: String,
    pub footsteps: String,
    pub trail: String,
}

impl Character {
    /// Wears `mesh` (`body`) with the animation set `anims` (`Character::SetOverrideMesh` /
    /// `SetOverrideAnimations`); the old animation's pending callbacks are dropped
    /// (`Actor::ClearAnimationCallbacks`) and the state's animation restarts. `None`: the own set plays.
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

    /// Takes the form off: the own body and set come back, and a cast in progress ends
    /// (`SetOverrideAnimations` restarts the run or idle animation).
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

    /// The body's bone frames in the current pose (mesh space), rebuilt if stale.
    pub fn posed(&self) -> &[Coords] {
        self.posed
            .get_or_init(|| anim::pose_abs(&self.body, &self.pose))
    }

    /// The attach point `name` in its pose, in model space (`Actor::GetAttachPointCoords`).
    pub fn posed_point(&self, name: &str) -> Option<Vec3> {
        let p = self.body.attach_point(name)?;
        Some(Vec3::from(p.coords_in(self.posed()).origin))
    }

    /// Marks `posed` behind the pose: the next [`Self::posed`] / [`Self::posed_point`] rebuilds it.
    pub fn mark_pose_stale(&mut self) {
        self.posed.take();
    }

    /// Call after the body or the pose changed.
    pub fn repose(&mut self) {
        let mut abs = self.posed.take().unwrap_or_default();
        anim::pose_abs_into(&self.body, &self.pose, &mut abs);
        self.posed = std::sync::OnceLock::from(abs);
    }

    pub fn form_mesh(&self) -> Option<&str> {
        self.form.as_ref().map(|f| f.mesh.as_str())
    }

    pub fn form_records(&self) -> Option<&FormRecords> {
        self.form.as_ref().map(|f| &f.records)
    }

    /// Plays the Transform slot once as a cast (`Skill::StartNormalAnimation`, type 49 at the spell-cast
    /// speed, right after the swap).
    pub fn play_transform(&mut self) {
        self.cast_anim = None;
        self.cast_type = AnimType::Transform;
        self.cast_speed = self.spell_speed;
        self.cast_kind = SpeedKind::Cast;
        self.state = CharState::Cast;
        self.playing = AnimType::None;
    }
}

/// A character record's animation set (`charAnimationTableName`, else the record) for a hand state.
pub fn anim_set_for(data: &GameData, rec: &Record, hand: HandState) -> AnimSet {
    let table = rec
        .str("charAnimationTableName")
        .and_then(|t| data.db.get(t));
    AnimSet::load(table.unwrap_or(rec), hand, |p| data.vfs.exists(p))
}

/// The animation state of a character of `record` (`rec`) with body mesh `body`, for the hand state of
/// its weapons (`EquipManager::GetHandState`). It starts idle.
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

impl Character {
    /// Animation time scale from debuffs: the slow of the playing clip's [`SpeedKind`].
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

    /// `Character::ForceSpeedUpdate` → `AnimationSet::AdjustAnimation`: the playing clip's rate follows a
    /// changed profile speed in place (caller speed × record speed).
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

    /// Whether a new action may start now (`CheckAction` permission 0 / 4): idle or moving, or the
    /// playing action's `AllowInterrupt` crossed (an evade: its dash over).
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

    /// The playing clip's remaining real ms (`AnimChannel::GetRemainingAnimationTime`: (frame count ÷ fps ×
    /// 1000 − clip time) ÷ channel speed): the attack-delay a move's recovery cancel carries
    /// (`AttackAction::QueryActionPermission`). A held clip leaves none.
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

    /// Starts casting a skill of record class `class` with the special animation `special`. False for
    /// classes that do not animate.
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
        self.cast_after = self.plays;
        // Restart even when the same type is already playing.
        self.playing = AnimType::None;
        true
    }

    /// Whether this frame's callbacks come from a clip started after the current cast began; only those
    /// fire its pending cast. The ended action's "End" belongs to the old clip even when a new cast started
    /// on it the same frame.
    pub fn callbacks_after_cast(&self) -> bool {
        self.sent_play > self.cast_after
    }

    /// A cast started at `point` on `target` (`tf`: the facing before any turn): `AttackAction::Execute`
    /// turns the character to it, and a turning weapon class picks its `AttTurn*` clip from the angle.
    pub fn begin_cast(&mut self, tf: &Transform, target: Option<(Entity, Vec3)>, point: Vec3) {
        let turns = self.cast_turns;
        self.begin_attack(tf, target, point, turns);
    }

    /// [`Character::begin_cast`] for a plain weapon swing: it always may turn.
    pub fn begin_swing(&mut self, tf: &Transform, target: Option<(Entity, Vec3)>, point: Vec3) {
        self.begin_attack(tf, target, point, true);
    }

    /// A channelled skill starts ([`CharState::Channel2`] for `second`): the pair's `*Start` clip once, then
    /// its loop until [`Character::end_channel`]. At the spell-cast speed.
    pub fn begin_channel(&mut self, second: bool) {
        self.state = if second {
            CharState::Channel2
        } else {
            CharState::Channel
        };
        // Restart even when the same pair is already playing.
        self.playing = AnimType::None;
    }

    /// A whirlwind starts: `SpinStart` once, then `Spin` at the attack speed until
    /// [`Character::end_channel`].
    pub fn begin_spin(&mut self) {
        self.state = CharState::Spin;
        self.playing = AnimType::None;
    }

    /// The channel / whirlwind is over: back to idle (the loop's "End" only wraps).
    pub fn end_channel(&mut self) {
        if matches!(
            self.state,
            CharState::Channel | CharState::Channel2 | CharState::Spin
        ) {
            self.state = CharState::Idle;
            self.playing = AnimType::None;
        }
    }

    pub fn begin_attack(
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

    /// Plays the first of `types` the set has, once.
    pub fn play_first(
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

    /// `face_down`: how the body fell.
    pub fn knock_down(&mut self, face_down: bool) {
        self.fell_face_down = face_down;
        self.state = CharState::KnockedDown;
        self.cast_anim = None;
        self.playing = AnimType::None;
    }

    /// The get-up clip plays once.
    pub fn get_up(&mut self) {
        self.state = CharState::GettingUp;
        self.playing = AnimType::None;
    }

    /// The world ground speed (m/s) the move `state`'s slot carries at caller speed 1; 0 before it is
    /// derived, without the slot, or for a non-move state.
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

    /// The caller speed the move `state`'s clip plays at this frame and whether it is fixed.
    pub fn move_caller(&self, state: CharState) -> (f32, bool) {
        match (self.move_rate, state) {
            (Some(r), _) => (r, true),
            (None, CharState::Charge) => (1.0, true),
            (None, _) => (self.run_speed, false),
        }
    }

    /// The caller speed of the playing move clip, if it is the state's own clip: what its root motion was
    /// produced at.
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

    /// The playing clip is the state's own animation (or its [`Self::move_fallback`]).
    pub fn plays_own_clip(&self) -> bool {
        // A held scripted clip stays its own past the "End".
        if self.state == CharState::ScriptedHold {
            return self.playing == self.scripted;
        }
        wanted_type(self.state, &self.anims).contains(&self.playing)
            || self.move_fallback == Some((self.state, self.playing))
    }

    /// Derives [`MoveMotion`] when the animation set or body changed.
    pub fn derive_motion(&mut self, data: &GameData, cache: &mut AnimCache, scale: f32) {
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
            run: if run > 0.0 { run } else { walk },
            walk: if walk > 0.0 { walk } else { run },
            charge,
            scale,
        };
    }

    /// `AnimationSet::PlayAnimationIfAvailable` for `ScriptableAction_ControllerState::Execute`: plays the
    /// slot once if the set has it. PORT: a dead character ignores it (the engine does not check).
    pub fn play_scripted(&mut self, ty: AnimType) -> bool {
        self.play_scripted_with(ty, false)
    }

    /// `hold` (`PlayAnimation`'s arg) keeps the pose past the clip's end.
    pub fn play_scripted_with(&mut self, ty: AnimType, hold: bool) -> bool {
        if ty == AnimType::None || self.state == CharState::Death || !self.anims.has(ty) {
            return false;
        }
        self.state = if hold {
            CharState::ScriptedHold
        } else {
            CharState::Scripted
        };
        self.scripted = ty;
        // Restart even when the same type is already playing.
        self.playing = AnimType::None;
        true
    }

    /// Keeps `anim` loaded while the character lives; a new animation set starts the list again.
    pub fn hold_clip(&mut self, anim: &Arc<Animation>) {
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

    /// Plays a slot: weighted variant pick, channel speed = caller speed × record speed, engine blend
    /// rules. `kind`: the profile speed the clip runs at (`None`: none).
    pub fn play(
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
        self.allow_interrupt = false;
        true
    }

    pub fn play_special(
        &mut self,
        data: &GameData,
        cache: &mut AnimCache,
        caller_speed: f32,
    ) -> bool {
        let Some(name) = self.cast_anim.clone() else {
            return false;
        };
        self.play_special_named(data, cache, &name, caller_speed, self.cast_kind)
    }

    /// A special animation by reference name (`AnimSet::specials`), once at `caller_speed` × record speed.
    pub fn play_special_named(
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

    pub fn enter_state(&mut self, data: &GameData, cache: &mut AnimCache) {
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
                // The other clip stands in when the own one would not play (an unreadable `.anm`).
                if ok && !own {
                    self.move_fallback = Some((self.state, b));
                }
                self.fixed_speed = ok && fixed;
                ok
            }
            // `Character::MoveTo`: `PlayAnimationIfAvailable`, no fallback. The run slow scales it.
            CharState::Charge => {
                let (s, fixed) = self.move_caller(CharState::Charge);
                let ok = self.play(data, cache, AnimType::Charge, s, Some(SpeedKind::Run), true);
                self.fixed_speed = ok && fixed;
                ok
            }
            // A weapon-pool proc plays the picked skill's special animation (no turn); else the plain
            // swing (`SkillActivatedWeapon::StartAction`), the turn clip first.
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
            // `Skill::StartAction`: the special animation, else the class's type (turn clip first);
            // SpellCast/Attack only as a creature fallback. PORT: a player's missing turn clip falls back to
            // the type (the engine's attack fails; no stock table lacks one).
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
            // The `*Start` clip once, its "End" starts the loop; no start clip: the loop at once.
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
            CharState::TakeHit => self.play(data, cache, AnimType::TakeHit, 1.0, None, false),
            CharState::Pickup => self.play(data, cache, AnimType::Pickup, 1.0, None, false),
            CharState::Death => self.play(data, cache, AnimType::Die, 1.0, None, false),
            // The evade action's playback rate is not traced.
            CharState::Evade => self.play(data, cache, AnimType::Evade, 1.0, None, false),
            // `ControllerMonsterStateFlee::OnBegin`: `PlayAnimationAction` (type 37, speed 1.0).
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
            CharState::Scripted | CharState::ScriptedHold => {
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
            warn!("{}: no evade animation", self.record);
            self.state = CharState::Idle;
            return;
        }
        if !ok && matches!(self.state, CharState::Scripted | CharState::ScriptedHold) {
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
            // No "End" would leave it: over at once.
            self.state = CharState::Idle;
            return;
        }
        if !ok && matches!(self.state, CharState::Roar | CharState::Alert) {
            self.state = CharState::Idle;
            return;
        }
        if !ok && matches!(self.state, CharState::Attack | CharState::Cast) {
            // No "End" would ever leave the state: over at once. PORT: the engine's skill action without
            // an animation was not decompiled.
            warn!("{}: no animation for {:?}", self.record, self.state);
            self.state = CharState::Idle;
            self.cast_anim = None;
            return;
        }
        if !ok {
            // A record with no idle clip at all (e.g. the Crucible turrets) stands still in its pose.
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
    pub fn play_long_idle(&mut self, data: &GameData, cache: &mut AnimCache) -> bool {
        self.reset_transition_timer();
        self.play(data, cache, AnimType::LongIdle, 1.0, None, true)
    }

    pub fn play_menu_idle(&mut self, data: &GameData, cache: &mut AnimCache) -> bool {
        self.reset_transition_timer();
        self.play(data, cache, AnimType::MenuIdle, 1.0, None, true)
    }

    /// `ResetTransitionTimer`.
    pub fn reset_transition_timer(&mut self) {
        let (lo, hi) = FIDGET_WAIT_MS;
        self.idle_timer = (0.0, lo + self.rng.f32() * (hi - lo));
    }

    /// An idle animation's "End": the idle action's `AnimationCallback`.
    pub fn idle_end(&mut self, data: &GameData, cache: &mut AnimCache, t: AnimType) {
        match (self.idle_playing, t) {
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
            // `MenuIdleAction::AnimationCallback`.
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

/// The clips that are `s`'s own animation. A move state's other move clip counts only when `anims` lacks
/// its own slot: a Run → Walk switch must start the Walk clip.
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
        // Only Evade: its jump callbacks drive the evade, so a playing Run must not count.
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
        CharState::Scripted | CharState::ScriptedHold => &[],
    }
}

/// `Animate` advances the animations and dispatches their callbacks before the gameplay readers (hit point
/// → damage in one tick); `Drive` runs after the frame's requesters.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CharacterStage {
    Drive,
    Animate,
}
