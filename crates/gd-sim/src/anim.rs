//! Character animation sets and the engine's animation channel, Bevy-free.
//!
//! Reverse-engineered from Game.dll / Engine.dll:
//! - `Character::LoadAnimations` reads the record named by
//!   `charAnimationTableName` (falls back to the character record itself) and
//!   fills 16 `AnimationSet`s, one per weapon prefix, with 16 near-identical
//!   loaders (`Character::LoadUnarmedAnimations` ...) that call `AnimationSet::AddAnimation(type, animKey,
//!   speedKey, weight, refKey)`. [`set_keys`] reproduces that key table.
//! - `AnimationSet` holds 52 slots (`AnimationSet_Type`, names from
//!   `GetAnimTypeAsText` table `AnimTypeNames`). Slots with variants pick one by
//!   `rand()/32767 * totalWeight` (`AnimationSet::PickVariant`); `Special` maps a reference
//!   name to its own weighted list.
//! - A played animation's channel speed is `callerSpeed * recordSpeed`; the
//!   default blend (cross-fade) time is 200 ms (`AnimationBase` ctor).
//! - `AnimChannel::Update` (Engine.dll): time advances in ms by
//!   `dt * speed`; the frame is `fps * t / 1000`; the previous animations keep
//!   playing and fade out linearly over their blend time
//!   (`pose = Blend(pose, old, 1 - elapsed/blend)` per bone). Callback points
//!   and entity events (`CreateEntity` / `RemoveEntity`) are crossed in the
//!   same interval ([`ChannelEvents`]) and emitted inside the advance (the
//!   engine dispatches them inline, `SkillManager::HandleSkillAnimationCallback`);
//!   a step spanning several loops reports each one's points
//!   and "End", and a finished one-shot's overshoot carries into the next
//!   clip's start ([`AnimChannel::play`]).
//! - Root motion (`AnimChannel::Advance`): the root-motion bone's key translation is
//!   removed from the pose and its per-tick delta becomes the actor's
//!   velocity, transformed by the bone's bind matrix, the mesh scale and the
//!   entity orientation (`Actor::UpdateSelf`).
//!
//! PORT: the pose is one key per skeleton bone (the engine's `SkeletalPose` is a bone-name
//! map): a clip's tracks are mapped to bones once per (clip, skeleton), and a track for a
//! bone the skeleton lacks poses nothing.
use crate::equip::HandState;
use gd_formats::anm::{self, Animation, IDENTITY_KEY, Key};
use gd_formats::arz::Record;
use gd_formats::collections::HashMap;
use gd_formats::msh::{BoneSet, Coords, Mesh};
use parking_lot::Mutex;
use std::sync::{Arc, Weak};

/// `GAME::AnimationSet_Type`, in engine order (one name per [`AnimType::ALL`]
/// entry; the length is checked by the type).
pub const ANIM_TYPE_NAMES: [&str; AnimType::COUNT] = [
    "None",
    "AttackIdle",
    "IdleTransition",
    "LongIdle",
    "Fidget",
    "Run",
    "Walk",
    "Charge",
    "Pickup",
    "PassItem",
    "Chat",
    "Stun",
    "CriticalHit",
    "Die",
    "BuffSelf",
    "BuffOther",
    "SpellCast",
    "Attack",
    "Special",
    "Spawn",
    "Respawn",
    "BeginMount",
    "Mount",
    "Dismount",
    "EndDismount",
    "AttTurnRight90",
    "AttTurnLeft90",
    "AttTurnRight180",
    "AttTurnLeft180",
    "TurnRight90",
    "TurnLeft90",
    "TurnRight180",
    "TurnLeft180",
    "Alert",
    "Waiting",
    "Rally",
    "Emote",
    "Flee",
    "GetUpFaceDown",
    "GetUpFaceUp",
    "ChannelStart",
    "Channel",
    "Channel2Start",
    "Channel2",
    "SpinStart",
    "Spin",
    "TakeHit",
    "Kick",
    "Evade",
    "Transform",
    "MenuIdle",
    "MenuFidget",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AnimType {
    None = 0,
    AttackIdle,
    IdleTransition,
    LongIdle,
    Fidget,
    Run,
    Walk,
    Charge,
    Pickup,
    PassItem,
    Chat,
    Stun,
    CriticalHit,
    Die,
    BuffSelf,
    BuffOther,
    SpellCast,
    Attack,
    Special,
    Spawn,
    Respawn,
    BeginMount,
    Mount,
    Dismount,
    EndDismount,
    AttTurnRight90,
    AttTurnLeft90,
    AttTurnRight180,
    AttTurnLeft180,
    TurnRight90,
    TurnLeft90,
    TurnRight180,
    TurnLeft180,
    Alert,
    Waiting,
    Rally,
    Emote,
    Flee,
    GetUpFaceDown,
    GetUpFaceUp,
    ChannelStart,
    Channel,
    Channel2Start,
    Channel2,
    SpinStart,
    Spin,
    TakeHit,
    Kick,
    Evade,
    Transform,
    MenuIdle,
    MenuFidget,
}

impl AnimType {
    /// The number of types: the last variant's discriminant + 1. A variant
    /// removed or inserted before `MenuFidget` changes it, so [`Self::ALL`] and
    /// [`ANIM_TYPE_NAMES`] fail to compile until they follow; one appended
    /// after it must also become the anchor here.
    pub const COUNT: usize = AnimType::MenuFidget as usize + 1;

    /// Every type in engine order: `ALL[i] as usize == i` (checked at compile
    /// time below).
    pub const ALL: [AnimType; Self::COUNT] = [
        Self::None,
        Self::AttackIdle,
        Self::IdleTransition,
        Self::LongIdle,
        Self::Fidget,
        Self::Run,
        Self::Walk,
        Self::Charge,
        Self::Pickup,
        Self::PassItem,
        Self::Chat,
        Self::Stun,
        Self::CriticalHit,
        Self::Die,
        Self::BuffSelf,
        Self::BuffOther,
        Self::SpellCast,
        Self::Attack,
        Self::Special,
        Self::Spawn,
        Self::Respawn,
        Self::BeginMount,
        Self::Mount,
        Self::Dismount,
        Self::EndDismount,
        Self::AttTurnRight90,
        Self::AttTurnLeft90,
        Self::AttTurnRight180,
        Self::AttTurnLeft180,
        Self::TurnRight90,
        Self::TurnLeft90,
        Self::TurnRight180,
        Self::TurnLeft180,
        Self::Alert,
        Self::Waiting,
        Self::Rally,
        Self::Emote,
        Self::Flee,
        Self::GetUpFaceDown,
        Self::GetUpFaceUp,
        Self::ChannelStart,
        Self::Channel,
        Self::Channel2Start,
        Self::Channel2,
        Self::SpinStart,
        Self::Spin,
        Self::TakeHit,
        Self::Kick,
        Self::Evade,
        Self::Transform,
        Self::MenuIdle,
        Self::MenuFidget,
    ];

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }
    pub fn name(self) -> &'static str {
        ANIM_TYPE_NAMES[self as usize]
    }
}

const _: () = {
    let mut i = 0;
    while i < AnimType::COUNT {
        assert!(AnimType::ALL[i] as usize == i, "AnimType::ALL out of order");
        i += 1;
    }
};

/// Weight argument of one loader call.
#[derive(Debug, Clone, PartialEq)]
pub enum WeightSrc {
    Const(f32),
    Key(String),
}

/// One `AnimationSet::AddAnimation` call: animation key, speed key, weight, reference key.
#[derive(Debug, Clone, PartialEq)]
pub struct KeySpec {
    pub ty: AnimType,
    pub anim: String,
    pub speed: String,
    pub weight: WeightSrc,
    pub reference: Option<String>,
}

/// The 241 key specs of one animation set, exactly as the engine loaders
/// list them (including their spelling quirks: `IdleTranAnimSpeed`,
/// `BuffSelfSpeed1`, `SpellAttackAnimWeight` without "1", and the
/// `TrabsformAnimSpeed` typo in the sHanded loaders).
pub fn set_keys(hand: HandState) -> Vec<KeySpec> {
    use AnimType as T;
    let p = hand.prefix();
    let mut v = Vec::with_capacity(241);
    let mut add = |ty: AnimType,
                   anim: String,
                   speed: String,
                   weight: WeightSrc,
                   reference: Option<String>| {
        v.push(KeySpec {
            ty,
            anim,
            speed,
            weight,
            reference,
        })
    };
    let c = WeightSrc::Const;
    let k = |s: String| WeightSrc::Key(s);
    add(
        T::AttackIdle,
        format!("{p}AttackIdleAnim"),
        format!("{p}AttackIdleAnimSpeed"),
        c(100.0),
        None,
    );
    for i in 2..=5 {
        add(
            T::AttackIdle,
            format!("{p}AttackIdleAnim{i}"),
            format!("{p}AttackIdleAnimSpeed{i}"),
            k(format!("{p}AttackIdleAnimWeight{i}")),
            None,
        );
    }
    add(
        T::IdleTransition,
        format!("{p}IdleTransAnim"),
        format!("{p}IdleTranAnimSpeed"),
        c(0.0),
        None,
    );
    add(
        T::LongIdle,
        format!("{p}LongIdleAnim"),
        format!("{p}LongIdleAnimSpeed"),
        c(0.0),
        None,
    );
    for i in 1..=3 {
        add(
            T::Fidget,
            format!("{p}FidgetAnim{i}"),
            format!("{p}FidgetAnimSpeed{i}"),
            k(format!("{p}FidgetAnimWeight{i}")),
            None,
        );
    }
    for (t, n) in [
        (T::Run, "Run"),
        (T::Walk, "Walk"),
        (T::Charge, "Charge"),
        (T::Pickup, "Pickup"),
        (T::PassItem, "PassItem"),
        (T::Chat, "Chat"),
        (T::Stun, "Stun"),
        (T::CriticalHit, "CriticalHit"),
    ] {
        add(
            t,
            format!("{p}{n}Anim"),
            format!("{p}{n}AnimSpeed"),
            c(0.0),
            None,
        );
    }
    for i in 1..=3 {
        add(
            T::Die,
            format!("{p}DieAnim{i}"),
            format!("{p}DieAnimSpeed{i}"),
            k(format!("{p}DieAnimWeight{i}")),
            None,
        );
    }
    for (t, n) in [(T::BuffSelf, "BuffSelf"), (T::BuffOther, "BuffOther")] {
        for i in 1..=3 {
            add(
                t,
                format!("{p}{n}Anim{i}"),
                format!("{p}{n}Speed{i}"),
                k(format!("{p}{n}Weight{i}")),
                None,
            );
        }
    }
    add(
        T::SpellCast,
        format!("{p}SpellAttackAnim"),
        format!("{p}SpellAttackAnimSpeed"),
        k(format!("{p}SpellAttackAnimWeight")),
        None,
    );
    for i in 2..=3 {
        add(
            T::SpellCast,
            format!("{p}SpellAttackAnim{i}"),
            format!("{p}SpellAttackAnimSpeed{i}"),
            k(format!("{p}SpellAttackAnimWeight{i}")),
            None,
        );
    }
    for i in 1..=3 {
        add(
            T::Attack,
            format!("{p}AttackAnim{i}"),
            format!("{p}AttackAnimSpeed{i}"),
            k(format!("{p}AttackAnimWeight{i}")),
            None,
        );
    }
    for i in 1..=160 {
        add(
            T::Special,
            format!("{p}SpecialAnim{i}"),
            format!("{p}SpecialAnimSpeed{i}"),
            c(100.0),
            Some(format!("{p}SpecialAnimRef{i}")),
        );
    }
    for i in 1..=3 {
        for s in ["A", "B", "C"] {
            add(
                T::Special,
                format!("{p}SpecialAnimPool{i}{s}"),
                format!("{p}SpecialAnimPoolSpeed{i}{s}"),
                k(format!("{p}SpecialAnimPoolWeight{i}{s}")),
                Some(format!("{p}SpecialAnimPoolRef{i}")),
            );
        }
    }
    add(
        T::Spawn,
        format!("{p}SpawnAnim"),
        format!("{p}SpawnAnimSpeed"),
        c(0.0),
        None,
    );
    add(
        T::Respawn,
        format!("{p}RespawnAnim"),
        format!("{p}RespawnAnimSpeed"),
        c(0.0),
        None,
    );
    let turn_w = if hand == HandState::Unarmed {
        100.0
    } else {
        0.0
    };
    for (t, n) in [
        (T::AttTurnRight90, "AttTurnRight90"),
        (T::AttTurnLeft90, "AttTurnLeft90"),
        (T::AttTurnRight180, "AttTurnRight180"),
        (T::AttTurnLeft180, "AttTurnLeft180"),
        (T::TurnRight90, "TurnRight90"),
        (T::TurnLeft90, "TurnLeft90"),
        (T::TurnRight180, "TurnRight180"),
        (T::TurnLeft180, "TurnLeft180"),
    ] {
        add(
            t,
            format!("{p}{n}Anim"),
            format!("{p}{n}AnimSpeed"),
            c(turn_w),
            None,
        );
    }
    for (t, n) in [
        (T::Alert, "Alert"),
        (T::Waiting, "Waiting"),
        (T::Rally, "Rally"),
        (T::Emote, "Emote"),
        (T::Flee, "Flee"),
    ] {
        for i in 1..=3 {
            add(
                t,
                format!("{p}{n}Anim{i}"),
                format!("{p}{n}AnimSpeed{i}"),
                k(format!("{p}{n}AnimWeight{i}")),
                None,
            );
        }
    }
    for (t, n) in [
        (T::GetUpFaceDown, "GetUpFaceDown"),
        (T::GetUpFaceUp, "GetUpFaceUp"),
        (T::ChannelStart, "ChannelStart"),
        (T::Channel, "Channel"),
        (T::Channel2Start, "Channel2Start"),
        (T::Channel2, "Channel2"),
        (T::SpinStart, "SpinStart"),
        (T::Spin, "Spin"),
        (T::TakeHit, "TakeHit"),
        (T::Kick, "Kick"),
        (T::Evade, "Evade"),
    ] {
        add(
            t,
            format!("{p}{n}Anim"),
            format!("{p}{n}AnimSpeed"),
            c(100.0),
            None,
        );
    }
    let transform_speed = if p == "sHanded" {
        "TrabsformAnimSpeed"
    } else {
        "TransformAnimSpeed"
    };
    add(
        T::Transform,
        format!("{p}TransformAnim"),
        format!("{p}{transform_speed}"),
        c(100.0),
        None,
    );
    let m = hand.menu_infix();
    add(
        T::MenuIdle,
        format!("{p}{m}MenuIdleAnim"),
        format!("{p}{m}MenuIdleAnimSpeed"),
        c(100.0),
        None,
    );
    add(
        T::MenuFidget,
        format!("{p}{m}MenuFidgetAnim"),
        format!("{p}{m}MenuFidgetAnimSpeed"),
        c(100.0),
        None,
    );
    v
}

/// One loaded variant: `.anm` path, record speed, selection weight.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimEntry {
    pub path: String,
    pub speed: f32,
    pub weight: f32,
}

/// One `AnimationBase` slot. Single-animation slots keep only the first
/// successfully added entry; variant slots append and sum weights.
#[derive(Debug, Clone, Default)]
pub struct AnimSlot {
    pub entries: Vec<AnimEntry>,
    pub total_weight: f32,
    /// Cross-fade time in ms (`AnimationBase` default 200).
    pub blend_ms: f32,
}

impl AnimSlot {
    /// `AnimationSet::PickVariant`: `r = rand()/32767 * total`, first entry whose
    /// running weight sum reaches `r`; `None` if it runs off the end.
    pub fn pick(&self, r01: f32) -> Option<&AnimEntry> {
        let r = r01 * self.total_weight;
        let mut acc = 0.0;
        for e in &self.entries {
            acc += e.weight;
            if r <= acc {
                return Some(e);
            }
        }
        None
    }

    /// `AnimationBase::GetAnimSpeed`: mean of `numFrames/fps / recordSpeed` over the
    /// variants, i.e. the effective duration in seconds at caller speed 1
    /// (1.0 when empty). Needs the frame counts, so the caller supplies them.
    pub fn mean_duration(&self, frames_fps: impl Fn(&str) -> Option<(u32, u32)>) -> f32 {
        if self.entries.is_empty() {
            return 1.0;
        }
        let sum: f32 = self
            .entries
            .iter()
            .map(|e| match frames_fps(&e.path) {
                Some((n, fps)) if fps > 0 && e.speed != 0.0 => n as f32 / fps as f32 / e.speed,
                _ => 0.0,
            })
            .sum();
        sum / self.entries.len() as f32
    }
}

/// Animation types whose slot holds weighted variants (`AnimationBase`
/// subclass vtable `AnimationVariants_vftable` in the `AnimationSet` ctor). `Special` uses a
/// per-reference map of variant lists (vtable `AnimationSpecial_vftable`).
pub fn is_variant_slot(t: AnimType) -> bool {
    use AnimType as T;
    matches!(
        t,
        T::AttackIdle
            | T::Fidget
            | T::Die
            | T::BuffSelf
            | T::BuffOther
            | T::SpellCast
            | T::Attack
            | T::Alert
            | T::Waiting
            | T::Rally
            | T::Emote
            | T::Flee
    )
}

#[derive(Debug, Clone)]
pub struct AnimSet {
    pub hand: HandState,
    pub slots: Vec<AnimSlot>,
    /// `Special` by reference name.
    pub specials: HashMap<String, AnimSlot>,
}

impl AnimSet {
    /// Loads the set for `hand` from an animation-table record (or a
    /// character record with inline keys). `exists` filters out missing
    /// `.anm` files (the engine's AddAnimation fails for those).
    pub fn load(rec: &RecordIndex, hand: HandState, exists: impl Fn(&str) -> bool) -> Self {
        let mut slots = vec![
            AnimSlot {
                blend_ms: 200.0,
                ..Default::default()
            };
            52
        ];
        let mut specials: HashMap<String, AnimSlot> = HashMap::default();
        for k in set_keys(hand) {
            let Some(path) = rec.str(&k.anim) else {
                continue;
            };
            if !exists(path) {
                continue;
            }
            // AnimationSet::AddAnimation reads the speed with default 1.0;
            // the loaders read weights with default 0.
            let speed = rec.f32(&k.speed).unwrap_or(1.0);
            let weight = match &k.weight {
                WeightSrc::Const(w) => *w,
                WeightSrc::Key(key) => rec.f32(key).unwrap_or(0.0),
            };
            let entry = AnimEntry {
                path: path.to_owned(),
                speed,
                weight,
            };
            if k.ty == AnimType::Special {
                let Some(r) = k.reference.as_deref().and_then(|r| rec.str(r)) else {
                    continue;
                };
                let slot = specials.entry(r.to_owned()).or_insert_with(|| AnimSlot {
                    blend_ms: 200.0,
                    ..Default::default()
                });
                slot.total_weight += weight;
                slot.entries.push(entry);
            } else if is_variant_slot(k.ty) {
                let s = &mut slots[k.ty as usize];
                s.total_weight += weight;
                s.entries.push(entry);
            } else {
                let s = &mut slots[k.ty as usize];
                if s.entries.is_empty() {
                    s.total_weight = weight;
                    s.entries.push(entry);
                }
            }
        }
        Self {
            hand,
            slots,
            specials,
        }
    }

    pub fn slot(&self, t: AnimType) -> &AnimSlot {
        &self.slots[t as usize]
    }

    pub fn has(&self, t: AnimType) -> bool {
        !self.slots[t as usize].entries.is_empty()
    }

    /// Weighted pick; single-animation slots always return their entry.
    pub fn pick(&self, t: AnimType, r01: f32) -> Option<&AnimEntry> {
        let s = self.slot(t);
        if is_variant_slot(t) {
            s.pick(r01)
        } else {
            s.entries.first()
        }
    }
}

/// Case-insensitive key lookup over a decoded record.
pub struct RecordIndex<'a> {
    map: HashMap<String, &'a [gd_formats::arz::RawValue]>,
}

impl<'a> RecordIndex<'a> {
    pub fn new(rec: &'a Record) -> Self {
        Self {
            map: rec
                .fields
                .iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v.as_slice()))
                .collect(),
        }
    }
    fn first(&self, key: &str) -> Option<&gd_formats::arz::RawValue> {
        self.map.get(&key.to_ascii_lowercase())?.first()
    }
    pub fn str(&self, key: &str) -> Option<&'a str> {
        match self.map.get(&key.to_ascii_lowercase())?.first()? {
            gd_formats::arz::RawValue::Str(s) if !s.is_empty() => Some(s),
            _ => None,
        }
    }
    pub fn f32(&self, key: &str) -> Option<f32> {
        self.first(key)?.as_f32()
    }
}

/// Blend time for a transition, `Player::PlayAnimation` rules
/// applied to the slot default: starting Attack/Special/AttTurn*/Spin/Evade/
/// Transform cuts (0 ms); Attack or AttTurn* → AttackIdle cuts; leaving Spin
/// blends 75 ms. Non-player characters use the slot time unchanged.
pub fn blend_ms(prev: AnimType, next: AnimType, slot_ms: f32, is_player: bool) -> f32 {
    use AnimType as T;
    if !is_player {
        return slot_ms;
    }
    let mut ms = slot_ms;
    if is_attack(prev) && next == T::AttackIdle {
        ms = 0.0;
    } else if prev == T::Spin {
        ms = 75.0;
    }
    if matches!(
        next,
        T::Attack
            | T::Special
            | T::AttTurnRight90
            | T::AttTurnLeft90
            | T::AttTurnRight180
            | T::AttTurnLeft180
            | T::Spin
            | T::Evade
            | T::Transform
    ) {
        ms = 0.0;
    }
    ms
}

/// A key pose (`GAME::SkeletalPose`) reused across frames: one key per bone of the skeleton it
/// was built for, so sampling, blending and posing index bones instead of hashing their names.
/// [`AnimChannel::update`] refills the values in place and rebuilds the buffer only when the
/// skeleton changes (a shapeshift); a buffer belongs to one channel.
#[derive(Default)]
pub struct Pose {
    /// One key per bone of [`Self::mesh`] (the identity key where unposed).
    keys: Vec<Key>,
    /// The skeleton `keys` indexes.
    mesh: Option<Arc<Mesh>>,
    /// Other skeletons posed with these keys (armour on its wearer): their bone → key maps,
    /// built once per skeleton. A mutex so the reader systems sharing a pose cache them in
    /// place (the `Inventory::info` interior-cache pattern).
    maps: Mutex<Vec<ForeignMap>>,
}

/// Another skeleton posed with a [`Pose`]: its bone → key map, and for attached posing
/// ([`pose_abs_attached_into`]) the parent skeleton its root bones were resolved against.
struct ForeignMap {
    mesh: Weak<Mesh>,
    /// Per bone of `mesh`: its index in [`Pose::keys`], `u16::MAX` when the pose's skeleton
    /// has no bone of that name (the identity key).
    bones: Vec<u16>,
    /// (the parent skeleton, per bone of `mesh`: the parent bone a root bone takes its frame
    /// from, `u16::MAX` for none).
    roots: Option<(Weak<Mesh>, Vec<u16>)>,
}

impl Pose {
    pub fn new() -> Self {
        Self::default()
    }

    /// Writes the root-motion bone's key translation back into the pose: the
    /// advance removes it ([`ChannelEvents::root_translation`]); an object
    /// pose pins root motion itself and needs the sampled value.
    pub fn set_root_translation(&mut self, bone: usize, t: [f32; 3]) {
        if let Some(k) = self.keys.get_mut(bone) {
            k.translation = t;
        }
    }

    /// Indexes `mesh`'s bones, rebuilding the buffer when the skeleton changed.
    fn prepare(&mut self, mesh: &Arc<Mesh>) {
        if self.mesh.as_ref().is_some_and(|m| Arc::ptr_eq(m, mesh)) {
            return;
        }
        self.keys.clear();
        self.keys.resize(mesh.bones.len(), IDENTITY_KEY);
        self.mesh = Some(mesh.clone());
        self.maps.get_mut().clear();
    }
}

/// The entry of `mesh` in `maps`, built if absent: its bones matched to the pose's skeleton
/// by name once (case-insensitive, as the engine's pose lookup). Entries of gone skeletons
/// are dropped.
fn foreign_map<'a>(
    maps: &'a mut Vec<ForeignMap>,
    mesh: &Arc<Mesh>,
    home: Option<&Mesh>,
) -> &'a mut ForeignMap {
    maps.retain(|m| m.mesh.strong_count() > 0);
    // By address, no `upgrade` (two atomic ops per entry, per armour piece, per frame): a live `Weak`
    // keeps its allocation, so an equal address is `mesh` itself.
    if let Some(i) = maps
        .iter()
        .position(|m| std::ptr::eq(m.mesh.as_ptr(), Arc::as_ptr(mesh)))
    {
        return &mut maps[i];
    }
    let bones = mesh
        .bones
        .iter()
        .map(|b| {
            home.and_then(|h| h.bone_index(&b.name))
                .and_then(|i| u16::try_from(i).ok())
                .unwrap_or(u16::MAX)
        })
        .collect();
    maps.push(ForeignMap {
        mesh: Arc::downgrade(mesh),
        bones,
        roots: None,
    });
    maps.last_mut().unwrap()
}

/// Which profile speed a playing clip runs at — the debuff slow that scales
/// it: run for moves and charges, attack for weapon swings, kicks and weapon
/// specials, cast for spells and buffs (`Character::GetAttackSpeed`
/// / `GetSpellCastSpeed` fold the slow in before the
/// cap, and `AnimEntry::Play` plays at that value). Carried with
/// the clip instead of inferred from its [`AnimType`]: a `Special` may be a
/// weapon-pool proc (attack slow) or a spell (cast slow).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedKind {
    Run,
    Attack,
    Cast,
}

/// Engine `SkeletalPose::Interpolate` (lerp translation/scale, slerp both
/// rotations).
pub fn interpolate(a: &Key, b: &Key, t: f32) -> Key {
    let l = |x: [f32; 3], y: [f32; 3]| [0, 1, 2].map(|i| x[i] + (y[i] - x[i]) * t);
    Key {
        translation: l(a.translation, b.translation),
        rotation: anm::slerp(a.rotation, b.rotation, t),
        scale: l(a.scale, b.scale),
        scale_rotation: anm::slerp(a.scale_rotation, b.scale_rotation, t),
    }
}

/// `SkeletalPose::Blend(other, t)`: every bone becomes
/// `Interpolate(this[bone], other[bone], t)`. Both poses index the same skeleton
/// ([`AnimChannel::update`] prepares them), so the blend is element-wise.
fn blend_into(pose: &mut Pose, other: &Pose, t: f32) {
    for (cur, k) in pose.keys.iter_mut().zip(&other.keys) {
        *cur = interpolate(cur, k, t);
    }
}

/// `GraphicsAnim::GetFramePose` into a reused buffer: every track sampled at `frame` into its
/// bone's key ([`ChannelAnim::ensure_map`]'s track → bone map). Bones the clip lacks reset to
/// identity so a reused buffer blends exactly like a fresh one.
fn sample_into(anim: &Animation, frame: f32, looped: bool, pose: &mut Pose, track_bones: &[u16]) {
    pose.keys.fill(IDENTITY_KEY);
    for (t, &b) in anim.tracks.iter().zip(track_bones) {
        if let Some(k) = pose.keys.get_mut(b as usize) {
            *k = t.sample(frame, looped);
        }
    }
}

/// One playing animation of an `AnimChannel`.
#[derive(Clone)]
pub struct ChannelAnim {
    pub anim: Arc<Animation>,
    pub looped: bool,
    /// Elapsed time in ms (already multiplied by speed).
    pub time_ms: f32,
    pub speed: f32,
    /// Root-motion key translation at the previous tick.
    prev_root: Option<[f32; 3]>,
    /// The last tick sampled the pose at or past the final frame (a non-looped clip holds it from there)
    /// and its "End" was reported.
    at_end: bool,
    /// A non-looped clip's "End" was reported (since its [`AnimChannel::play`] / [`AnimChannel::seek`]).
    ended: bool,
    /// Not advanced yet: the first advance reports from frame 0, the carried overshoot included
    /// ([`AnimChannel::play`]).
    fresh: bool,
    /// The skeleton [`Self::track_bones`] was built for.
    map_mesh: Option<Arc<Mesh>>,
    /// Each track's bone index in that skeleton (`u16::MAX`: a bone the skeleton lacks),
    /// built once per (clip, skeleton) ([`Self::ensure_map`]).
    track_bones: Vec<u16>,
}

impl ChannelAnim {
    /// The track → bone map for `mesh`, built once per (clip, skeleton).
    fn ensure_map(&mut self, mesh: &Arc<Mesh>) {
        if self.map_mesh.as_ref().is_some_and(|m| Arc::ptr_eq(m, mesh)) {
            return;
        }
        self.track_bones.clear();
        self.track_bones.extend(self.anim.tracks.iter().map(|t| {
            mesh.bone_index(&t.bone)
                .and_then(|i| u16::try_from(i).ok())
                .unwrap_or(u16::MAX)
        }));
        self.map_mesh = Some(mesh.clone());
    }
}

struct Fading {
    a: ChannelAnim,
    elapsed_ms: f32,
    dur_ms: f32,
    /// Reused sample buffer for this fade's fixed animation.
    pose: Pose,
}

/// A `CreateEntity` / `RemoveEntity` trailer event an advance crossed
/// (module doc): the record to create / remove at the attach point.
#[derive(Debug, Clone)]
pub struct AnimEntityEvent {
    /// `CreateEntity` (false: `RemoveEntity`).
    pub create: bool,
    pub entity: String,
    pub attach: String,
}

/// What happened during a channel update.
#[derive(Debug, Default, Clone)]
pub struct ChannelEvents {
    /// Callback point names crossed this tick, in order (incl. "End").
    pub callbacks: Vec<String>,
    /// The entity events crossed this tick, in the same intervals as the
    /// callbacks (one wrap rule; only the current clip fires them).
    pub entities: Vec<AnimEntityEvent>,
    /// Root-motion velocity in the root bone's key space (units/s), before
    /// the bind/scale/orientation transform.
    pub root_velocity: [f32; 3],
    /// The root-motion bone's sampled key translation this tick, before the
    /// advance removed it from the pose ([`Pose::set_root_translation`]).
    pub root_translation: [f32; 3],
}

/// `GAME::AnimChannel` with cross-fading.
#[derive(Default)]
pub struct AnimChannel {
    pub current: Option<ChannelAnim>,
    fading: Vec<Fading>,
}

impl AnimChannel {
    /// `AnimChannel::PlayAnimation(anim, loop, speed, blendMs)`: the current
    /// animation moves to the fade list; blend 0 also drops all fades.
    /// A finished one-shot's overshoot (the time played past its final frame)
    /// carries into the new clip's start: the engine chains the next action in
    /// the same handler pass (`CharacterActionHandler::Execute`),
    /// so the cadence is the clip's own length, not whole frames. The carry is
    /// at most the new clip's length, and its first advance reports what it
    /// crosses from frame 0 (the callbacks in the carried time, its "End" when
    /// that reaches the end).
    pub fn play(&mut self, anim: Arc<Animation>, looped: bool, speed: f32, blend_ms: f32) {
        let len_ms = |a: &Animation| {
            let fps = a.frame_rate as f32;
            if fps <= 0.0 {
                0.0
            } else {
                (a.num_frames as f32 - 1.0).max(0.0) * 1000.0 / fps
            }
        };
        let carry = self.current.as_ref().map_or(0.0, |c| {
            if c.looped || !c.at_end || c.anim.frame_rate == 0 {
                return 0.0;
            }
            (c.time_ms - len_ms(&c.anim)).clamp(0.0, len_ms(&anim))
        });
        if let Some(cur) = self.current.take() {
            self.fading.insert(
                0,
                Fading {
                    a: cur,
                    elapsed_ms: 0.0,
                    dur_ms: blend_ms,
                    pose: Pose::default(),
                },
            );
        }
        if blend_ms <= 0.0 {
            self.fading.clear();
        }
        self.current = Some(ChannelAnim {
            anim,
            looped,
            time_ms: carry,
            speed,
            prev_root: None,
            at_end: false,
            ended: false,
            fresh: true,
            map_mesh: None,
            track_bones: Vec::new(),
        });
    }

    /// Nothing left to play: a non-looped clip already sampled at its end and no fade running, so another
    /// [`Self::update`] would give the same pose, no root motion and no callback. The caller may skip it
    /// (corpses, finished one-shot clips).
    pub fn settled(&self) -> bool {
        self.fading.is_empty() && self.current.as_ref().is_some_and(|c| !c.looped && c.at_end)
    }

    /// Current frame (fractional) of the playing animation.
    pub fn frame(&self) -> f32 {
        self.current
            .as_ref()
            .map_or(0.0, |c| c.anim.frame_rate as f32 * c.time_ms * 0.001)
    }

    /// Advances by `dt_ms`, refilling `pose` with the blended pose (root-motion
    /// bone translation removed) and returning the crossed callbacks, entity
    /// events and root motion. `pose` is reused across frames (it belongs to
    /// this channel); it is rebuilt only when `mesh` changes.
    pub fn update(&mut self, dt_ms: f32, mesh: &Arc<Mesh>, pose: &mut Pose) -> ChannelEvents {
        pose.prepare(mesh);
        let root = mesh.root_bone;
        let mut ev = ChannelEvents::default();
        self.fading.retain_mut(|f| {
            f.elapsed_ms += dt_ms;
            f.elapsed_ms < f.dur_ms
        });
        let Some(cur) = &mut self.current else {
            pose.keys.fill(IDENTITY_KEY);
            return ev;
        };
        ev.root_velocity = advance(cur, dt_ms, root, Some(&mut ev), pose, mesh);
        for f in &mut self.fading {
            f.pose.prepare(mesh);
            let ovel = advance(&mut f.a, dt_ms, root, None, &mut f.pose, mesh);
            let t = if f.dur_ms != 0.0 {
                f.elapsed_ms / f.dur_ms
            } else {
                1.0
            };
            blend_into(pose, &f.pose, 1.0 - t);
            for i in 0..3 {
                ev.root_velocity[i] = t * ev.root_velocity[i] + ovel[i] * (1.0 - t);
            }
        }
        ev
    }

    /// Moves the current clip to `time_ms` without restarting it: no cross-fade (the running fades go
    /// on) and nothing reported for the jump; the next advance reports from there.
    pub fn seek(&mut self, time_ms: f32) {
        if let Some(c) = &mut self.current {
            c.time_ms = time_ms;
            c.at_end = false;
            c.ended = false;
            c.fresh = false;
        }
    }
}

/// `AnimChannel::Advance`: sample at `t + dt*speed` into `pose`, extract root motion,
/// report the callback points and entity events crossed in
/// `[prevFrame, newFrame)` (with loop wrap) and "End". `root` is the skeleton's
/// root-motion bone index.
/// PORT: a non-looped clip without a duration (one frame, or frame rate 0) reports "End" on its first
/// advancing tick, so a state waiting for it is left (the engine's handling of such a clip was not checked).
fn advance(
    c: &mut ChannelAnim,
    dt_ms: f32,
    root: Option<usize>,
    mut events: Option<&mut ChannelEvents>,
    pose: &mut Pose,
    mesh: &Arc<Mesh>,
) -> [f32; 3] {
    let step = dt_ms * c.speed;
    let fps = c.anim.frame_rate as f32;
    let t_new = c.time_ms + step;
    c.ensure_map(mesh);
    sample_into(&c.anim, fps * t_new * 0.001, c.looped, pose, &c.track_bones);
    let mut vel = [0.0; 3];
    if let Some(k) = root.and_then(|i| pose.keys.get_mut(i)) {
        let now = k.translation;
        if dt_ms > 0.0
            && let Some(prev) = c.prev_root
        {
            vel = [0, 1, 2].map(|i| (now[i] - prev[i]) * 1000.0 / dt_ms);
        }
        c.prev_root = Some(now);
        if let Some(ev) = events.as_deref_mut() {
            ev.root_translation = now;
        }
        k.translation = [0.0; 3];
    }
    if let Some(ev) = events {
        report_advance(c, t_new, ev);
    } else {
        // A fading clip reports nothing and is dropped with its fade: only the current one settles.
        c.at_end = false;
        c.fresh = false;
    }
    c.time_ms = t_new;
    vel
}

/// The callbacks and entity events of the current clip's step from `c.time_ms` (frame 0 on its first
/// advance, [`AnimChannel::play`]'s carry) to `t_new` (frames `[prevFrame, newFrame)` with loop wrap, and
/// "End"), and whether it is now settled at its end. A non-looped clip reports its "End" once, on the
/// first advance that reaches its final frame.
fn report_advance(c: &mut ChannelAnim, t_new: f32, ev: &mut ChannelEvents) {
    let fps = c.anim.frame_rate as f32;
    let last = c.anim.num_frames as f32 - 1.0;
    c.at_end = false;
    let f0 = if c.fresh {
        0.0
    } else {
        c.time_ms * 0.001 * fps
    };
    c.fresh = false;
    let f1 = t_new * 0.001 * fps;
    if c.looped && last > 0.0 {
        // Every loop the step spans reports the points crossed in it and an
        // "End" at its wrap (a hitch may span several; the engine samples per tick).
        let (k0, k1) = ((f0 / last).floor(), (f1 / last).floor());
        let mut k = k0;
        while k <= k1 {
            let (lo, hi) = ((f0 - k * last).max(0.0), (f1 - k * last).min(last));
            report_span(ev, &c.anim, lo, hi);
            if k < k1 {
                ev.callbacks.push("End".into());
            }
            k += 1.0;
        }
    } else {
        let reached = if last > 0.0 && fps > 0.0 {
            f1 >= last
        } else {
            t_new > 0.0
        };
        let ended = reached && !c.ended;
        report_span(ev, &c.anim, f0, f1);
        // One at or past this tick's frame is still to come (a point on or after the final frame).
        let mut owed = false;
        for b in c.anim.trailer.iter().filter(|b| b.kind == "CallbackPoint") {
            owed |= b.int("frame").unwrap_or(0) as f32 >= f1;
        }
        c.ended |= ended;
        // A clip without a frame rate never advances its frame: it is over once it has ticked ("End" above).
        c.at_end = c.ended && (fps <= 0.0 || (f1 >= last && !owed));
        if ended {
            ev.callbacks.push("End".into());
        }
    }
}

/// The trailer's `CallbackPoint` / `CreateEntity` / `RemoveEntity` blocks
/// crossed in `[lo, hi)` (frames), in trailer order — the one interval both
/// the callbacks and the entity events fire from (`AnimChannel::Advance`; read in
/// place, `Animation::callback_points` / `create_entities` decode copies).
fn report_span(ev: &mut ChannelEvents, anim: &Animation, lo: f32, hi: f32) {
    for b in &anim.trailer {
        match b.kind.as_str() {
            "CallbackPoint" => {
                let f = b.int("frame").unwrap_or(0) as f32;
                if f >= lo && f < hi {
                    ev.callbacks
                        .push(b.str("name").unwrap_or_default().to_owned());
                }
            }
            "CreateEntity" | "RemoveEntity" => {
                let f = b.int("frame").unwrap_or(-1) as f32;
                if f >= lo && f < hi {
                    ev.entities.push(AnimEntityEvent {
                        create: b.kind == "CreateEntity",
                        entity: b.str("entity").unwrap_or_default().to_owned(),
                        attach: b.str("attach").unwrap_or_default().to_owned(),
                    });
                }
            }
            _ => {}
        }
    }
}

/// Mesh-space bone transforms for a pose (`GraphicsMesh::GetAbsCoords`):
/// `abs[i] = abs[parent] * bone.local * key(bone)`; missing bones use the
/// identity key. Matches `anm::pose_mesh` for a single animation.
pub fn pose_abs(mesh: &Arc<Mesh>, pose: &Pose) -> Vec<Coords> {
    let mut abs = Vec::new();
    pose_abs_into(mesh, pose, &mut abs);
    abs
}

/// [`pose_abs`] written into `abs` (its allocation reused; a character's pose every frame).
/// `mesh` is the pose's own skeleton, or another posed with its keys (an armour piece): the
/// latter's bone → key map is cached on the pose, built once per skeleton.
pub fn pose_abs_into(mesh: &Arc<Mesh>, pose: &Pose, abs: &mut Vec<Coords>) {
    if pose.mesh.as_ref().is_some_and(|m| Arc::ptr_eq(m, mesh)) || pose.keys.is_empty() {
        pose_abs_from(mesh, &pose.keys, None, |_| None, abs);
        return;
    }
    let mut maps = pose.maps.lock();
    let map = foreign_map(&mut maps, mesh, pose.mesh.as_deref());
    pose_abs_from(mesh, &pose.keys, Some(&map.bones), |_| None, abs);
}

/// Mesh-space bone transforms of a mesh attached to an actor, an armour piece
/// on its wearer, written into `abs`. `Actor::UpdateSelf` (Engine.dll): the
/// attached actor copies the parent mesh instance's `SkeletalPose` (keys by
/// bone name, `Actor::Attach` seeds it) and calls
/// `GraphicsMeshInstance::UpdatePose` with the parent, which is
/// `GraphicsMesh::GetAbsCoords`: a root bone found by name in
/// `parent` takes the parent bone's absolute transform (`parent_abs`, no own
/// bind or key), any other root runs from the identity as in [`pose_abs`];
/// the bones below go on with their own bind and the pose's key, the identity
/// key without one. So a mesh on a foreign skeleton (a paper doll's armour on
/// a form) follows only the bones the two share by name and
/// stays in its bind pose elsewhere. The name matches (bone → key and root →
/// parent bone) are cached on the pose, once per skeleton pair.
pub fn pose_abs_attached_into(
    mesh: &Arc<Mesh>,
    pose: &Pose,
    parent: &Arc<Mesh>,
    parent_abs: &[Coords],
    abs: &mut Vec<Coords>,
) {
    let home = pose.mesh.as_ref().is_some_and(|m| Arc::ptr_eq(m, mesh));
    let mut maps = pose.maps.lock();
    let map = foreign_map(&mut maps, mesh, pose.mesh.as_deref());
    let roots_stale = map
        .roots
        .as_ref()
        .is_none_or(|(p, _)| !std::ptr::eq(p.as_ptr(), Arc::as_ptr(parent)));
    if roots_stale {
        let roots = mesh
            .bones
            .iter()
            .map(|b| {
                if b.parent.is_some() {
                    u16::MAX
                } else {
                    parent
                        .bone_index(&b.name)
                        .and_then(|i| u16::try_from(i).ok())
                        .unwrap_or(u16::MAX)
                }
            })
            .collect();
        map.roots = Some((Arc::downgrade(parent), roots));
    }
    let roots = &map.roots.as_ref().unwrap().1;
    let bones = (!home && !pose.keys.is_empty()).then_some(&map.bones[..]);
    pose_abs_from(
        mesh,
        &pose.keys,
        bones,
        |i| {
            roots
                .get(i)
                .and_then(|&r| parent_abs.get(r as usize))
                .copied()
        },
        abs,
    );
}

/// [`pose_abs_into`] with `bone_map` mapping `mesh`'s bones to `keys` indices (`None`: `keys`
/// indexes `mesh` directly) and `root` giving a root bone's transform by its bone index.
fn pose_abs_from(
    mesh: &Mesh,
    keys: &[Key],
    bone_map: Option<&[u16]>,
    root: impl Fn(usize) -> Option<Coords>,
    abs: &mut Vec<Coords>,
) {
    let n = mesh.bones.len();
    abs.clear();
    abs.resize(n, Coords::IDENTITY);
    let mut done = BoneSet::new(n);
    let key_of = |i: usize| {
        match bone_map {
            None => keys.get(i).copied(),
            Some(m) => m.get(i).and_then(|&k| keys.get(k as usize)).copied(),
        }
        .unwrap_or(IDENTITY_KEY)
    };
    fn go(
        i: usize,
        mesh: &Mesh,
        key_of: &impl Fn(usize) -> Key,
        root: &impl Fn(usize) -> Option<Coords>,
        abs: &mut [Coords],
        done: &mut BoneSet,
    ) -> Coords {
        if done.contains(i) {
            return abs[i];
        }
        let b = &mesh.bones[i];
        abs[i] = match b.parent {
            Some(p) => go(p, mesh, key_of, root, abs, done)
                .mul(&b.local)
                .mul(&key_of(i).to_coords()),
            None => root(i).unwrap_or_else(|| b.local.mul(&key_of(i).to_coords())),
        };
        done.insert(i);
        abs[i]
    }
    for (i, b) in mesh.bones.iter().enumerate() {
        match b.parent {
            // Stock skeletons list every parent before its children: its frame is in (every bone
            // before `i` is), so no recursion. The same product as `go`'s.
            Some(p) if p < i => {
                abs[i] = abs[p].mul(&b.local).mul(&key_of(i).to_coords());
                done.insert(i);
            }
            _ => {
                go(i, mesh, &key_of, &root, abs, &mut done);
            }
        }
    }
}

/// Root-motion bone of a mesh: first bone without a parent (`GraphicsMesh::GetRootMotionBone`).
pub fn root_motion_bone(mesh: &Mesh) -> Option<&str> {
    mesh.root_motion_bone().map(|b| b.name.as_str())
}

/// Object-space root velocity: the key-space delta transformed by the root
/// bone's bind matrix and the mesh (actor) scale (`GraphicsMeshInstance::
/// Update`).
pub fn root_velocity_object(mesh: &Mesh, key_vel: [f32; 3], scale: f32) -> [f32; 3] {
    let Some(b) = mesh.bones.iter().find(|b| b.parent.is_none()) else {
        return [0.0; 3];
    };
    b.local.transform_vector(key_vel).map(|v| v * scale)
}

/// How a skill of record class `class` animates when no special animation
/// applies (`StartAction` of its engine class, Game.dll):
/// - `SkillActivatedBuffSelf` family (self/radius buffs, toggles, teleport,
///   charge potion, …) → `BuffSelf`;
/// - `Skill_Shapeshift` → `Transform` (`Skill_Shapeshift::StartAction`:
///   the special animation, else `StartNormalAnimation(49)` at
///   the spell-cast speed, in the set worn then: the human's going in, the
///   form's coming out);
/// - `SkillActivatedBuffOther` (`Skill_BuffOther`, `Skill_ChargedBuffOther`)
///   → `BuffOther`;
/// - `SkillActivatedWeapon` family, weapon pools and
///   `Skill_PlayAttackAnimation` → `Attack` at attack speed; the classes of
///   [`attack_turns`] first try an `AttTurn*` slot ([`attack_turn`]);
/// - `Skill_Kick` → `Kick` (`Skill_Kick::StartAction`:
///   the kick special animation, a player without the clip falls back to
///   `StartNormalAnimation(47)`); PORT: at the attack speed;
/// - every other activated skill (`SkillActivatedSpell`: projectiles, radius
///   attacks, summons, …) → `SpellCast`.
///
/// Buffs and spells run at the spell-cast speed (`Character::GetSpellCastSpeed`),
/// weapon skills at the attack speed (`Character::GetAttackSpeed`). `None` for passive classes.
pub fn skill_animation(class: &str) -> Option<(AnimType, bool)> {
    use AnimType as T;
    let c = class;
    let weapon = c.starts_with("Skill_WPAttack")
        || c.starts_with("Skill_AttackWeapon")
        || c.starts_with("Skill_WeaponPool")
        || matches!(
            c,
            "Skill_AttackInherent"
                | "Skill_AttackPathCharge"
                | "Skill_PlayAttackAnimation"
                | "SkillActivatedWeapon"
        );
    if weapon {
        return Some((T::Attack, true));
    }
    if c == "Skill_Kick" {
        return Some((T::Kick, true));
    }
    if c == "Skill_Shapeshift" {
        return Some((T::Transform, false));
    }
    if matches!(c, "Skill_BuffOther" | "Skill_ChargedBuffOther") {
        return Some((T::BuffOther, false));
    }
    let buff_self = (c.starts_with("Skill_Buff") && !c.starts_with("Skill_BuffOther"))
        || matches!(
            c,
            "Skill_ChargePotion"
                | "Skill_OnHitAttackRadius"
                | "Skill_OnHitBuffSelf"
                | "Skill_RefreshCooldown"
                | "Skill_Teleport"
                | "SkillActivated_Suicide"
        );
    if buff_self {
        return Some((T::BuffSelf, false));
    }
    let passive = c == "Skill_Passive"
        || c == "Skill_Mastery"
        || c.starts_with("Skill_Passive")
        || c.starts_with("SkillBuff")
        || c.starts_with("SkillSecondary")
        || c.ends_with("Modifier")
        || c.ends_with("Transmuter")
        || c.starts_with("Skill_OnDeath");
    if passive {
        return None;
    }
    Some((T::SpellCast, false))
}

/// Whether a skill of record class `class` animates through `SkillActivatedWeapon::StartAction`
/// (the classes whose `StartAction` virtual is it: `SkillActivatedWeapon`, `Skill_AttackWeapon`,
/// `…WeaponBlink`, `…WeaponCharge`, `…WeaponRangedSpread`, `Skill_AttackInherent`, `Skill_AttackPathCharge`,
/// `Skill_WPAttack`, `…BasicAttack`, `…RadialCrit`), the one that may turn into the attack ([`attack_turn`]);
/// `Skill_PlayAttackAnimation`, `Skill_WPAttack_Radius` / `AttackWave` / `ProjectileBurst` and the weapon pools
/// override it without a turn. A plain weapon swing (the default attack, a monster's melee) is one of them.
pub fn attack_turns(class: &str) -> bool {
    matches!(
        class,
        "SkillActivatedWeapon"
            | "Skill_AttackWeapon"
            | "Skill_AttackWeaponBlink"
            | "Skill_AttackWeaponCharge"
            | "Skill_AttackWeaponRangedSpread"
            | "Skill_AttackInherent"
            | "Skill_AttackPathCharge"
            | "Skill_WPAttack"
            | "Skill_WPAttack_BasicAttack"
            | "Skill_WPAttack_RadialCrit"
    )
}

/// The `AttTurn*` slots an [`attack_turns`] attack tries before `Attack`:
/// `SkillActivatedWeapon::StartAction` (after the special animation): `angle` is the one between the
/// caster's facing (local +Z) and the direction to the target (the target entity's position, else the cast point,
/// flattened), taken as the attack starts (before any turn); `right` = the target lies on the local -X side.
/// A player turning ≥ 45° plays `AttTurnRight90` / `AttTurnLeft90` and nothing else (the engine's attack fails
/// without the clip; every stock player and form table has them); a monster turning ≥ 135° tries
/// `AttTurnRight180` / `Left180`, then ≥ 45° `AttTurn*90`, then `Attack`.
pub fn attack_turn(is_player: bool, angle: f32, right: bool) -> &'static [AnimType] {
    use AnimType as T;
    const QUARTER: f32 = std::f32::consts::FRAC_PI_4; // 0.7853982
    const THREE_QUARTERS: f32 = 3.0 * std::f32::consts::FRAC_PI_4; // 2.3561945
    match (
        angle >= THREE_QUARTERS && !is_player,
        angle >= QUARTER,
        right,
    ) {
        (true, _, true) => &[T::AttTurnRight180, T::AttTurnRight90],
        (true, _, false) => &[T::AttTurnLeft180, T::AttTurnLeft90],
        (false, true, true) => &[T::AttTurnRight90],
        (false, true, false) => &[T::AttTurnLeft90],
        (false, false, _) => &[],
    }
}

/// [`attack_turn`]'s angle and side for a caster facing `yaw` (radians from +Z toward +X) at a target `to` away
/// (flattened; `None` for no direction).
pub fn attack_turn_angle(yaw: f32, to: [f32; 3]) -> Option<(f32, bool)> {
    let l = (to[0] * to[0] + to[2] * to[2]).sqrt();
    if l == 0.0 {
        return None;
    }
    let (dx, dz) = (to[0] / l, to[2] / l);
    let c = (dx * yaw.sin() + dz * yaw.cos()).clamp(-1.0, 1.0);
    // Local +X of the facing is (cos yaw, 0, -sin yaw).
    let side = dx * yaw.cos() - dz * yaw.sin();
    Some((c.acos(), side < 0.0))
}

/// An attack animation: `Attack` or one of the `AttTurn*` slots (types 25..=28).
pub fn is_attack(t: AnimType) -> bool {
    t == AnimType::Attack || (25..=28).contains(&(t as u8))
}
