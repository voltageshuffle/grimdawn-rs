//! Combat sounds: weapon swipes, melee strikes, what the struck character hears, ranged shots,
//! skill activations and projectiles. Chosen here (pure functions over the records, the
//! character's sound tables and the worn items: [`Holdings`]) and sent as [`PlaySound`].
//! Every engine sound below is `SoundPak::Play3D(coords, 0, true)` at the coordinates named.
//!
//! Records: `Weapon` `hitSound` / `swipeSound`; shield and `Armor` `blockSound`, `Armor`
//! `armorImpactSound`; `Character` `impactSoundChance`; the `CharacterSounds` entries
//! `attackSound`, `swipeSound`, `criticalHitSound`, `impactSound`; the skill profile's
//! `skillWarmUpSound`, `skillSwipeSound`, `skillHitSound`, `skillActivatedSound`,
//! `skillDeactivatedSound`, `skillCastSound`, `skillUnCastSound`, `playSpecialAttackSound1..4`;
//! the projectile's `projectileHitSound`, `projectileExplodingHitSound`, `projectileSwipeSound`,
//! `projectileBounceSound`, `projectileDestructSound`. Every shield and off-hand is a `Weapon`,
//! so they swipe and hit with their own sounds.
//!
//! - Swipes ([`swipe`]; `SkillActivatedWeapon::SwipeAction`): `SwipeRight` / `SwipeLeft` play
//!   that hand's weapon's `swipeSound` (the illusion's when its transmute is a `Weapon`); with
//!   none, or while shifted, the arm's `swipeSound` entry. Then `skillSwipeSound` on every swipe
//!   callback, `SwipeBoth` too.
//! - A melee strike ([`strike`]; `SkillActivated::ApplyMeleeDamage`): `skillHitSound` at the
//!   attacker; then, for the first target hit that is not evading (checked after the damage, so
//!   a killing blow plays too), both hands' weapon `hitSound` at the attacker, or `attackSound`
//!   when it holds neither. Once per strike.
//! - The struck character ([`struck`]; `CombatManager::TakeAttack`, at the defender): a shield
//!   block plays the shield's `blockSound`; a melee attack on a character not evading plays
//!   `impactSound` (when `rand() × 0.0030519` < `impactSoundChance`) and the body armour's
//!   `armorImpactSound`; a critical (multiplier > 1) of at least 1 damage plays
//!   `criticalHitSound`. No miss or dodge sound.
//! - A ranged weapon shot ([`shot`]; `SkillActivated::CreateProjectile`): the right-hand weapon
//!   (else the left) plays its `hitSound` at the shooter, and the new projectile its
//!   `projectileSwipeSound`, tracked.
//! - Projectile impacts ([`projectile_hit`]): each target hit plays `projectileExplodingHitSound`
//!   when the projectile explodes there, else `projectileHitSound`; an `explodeOnMiss` end the
//!   exploding sound; full distance without a hit `projectileDestructSound`.
//! - Activation ([`activate`]; `Skill::PlayActivateSound`): `skillActivatedSound` at the caster,
//!   then each `playSpecialAttackSoundN` set plays the caster's `specialAttackSoundN`.
//! - A spell's swipe ([`swipe_action`], [`spell_swipe`]): the spell / buff classes'
//!   `SwipeAction` (`Skill_Kick::SwipeAction` in IDA) plays only the profile's `skillSwipeSound`
//!   at the caster: the cast sound of most spells. Only `HandleSkillAnimationCallback` calls
//!   `SwipeAction`, so an action-less proc swipes nothing. No stock animation sends
//!   `SkillSound1` / `SkillSound2`, so `skillSound1` / `2` stay unread.
//! - FX paks ([`gd_game_core::combat_sounds::fx_pak_sound`]): `FxPak::OnAddToLevel` plays its
//!   `activateSound` at its coordinates. `CharFxPak::Start` plays `startSound` and loops the
//!   tracked `activeSound`; `Stop` fades that over 10 ms and plays `stopSound`.
//! - Spells' `skillHitSound` ([`spell_hit`]): at the launch point per projectile
//!   ([`HitAt::Launch`]), at each target of a per-target `TargetResult` ([`HitAt::Target`]), once
//!   at the caster for the radius / burst / summon / bonus classes ([`HitAt::Caster`]), at the
//!   skill's point for the targeted summons and buff wrappers ([`HitAt::Point`]). The class is
//!   the skill's own, the sound its profile's.
//! - Sounds that last while a skill does ([`SoundLoop`](gd_game_core::visuals::SoundLoop);
//!   [`held_loops`] for those without an entity): a wave's `waveLoopSound` (250 ms fade), a line
//!   fan's `loopSound` (250 ms from the first line's end); `startSound`, tracked `loopSound`
//!   (500 ms) and `endSound` of the cone, drain, beam (with `groundSound`), spin, grow, reflect
//!   link and tether classes.
//! - Lasting areas: `projectileSwipeSound` once, tracked, at the area (not looped);
//!   `projectileHitSound` where it ends.
//! - Switching off ([`deactivate`]): the buff / toggle classes play the activation sounds going
//!   active and `skillDeactivatedSound` going inactive, at the caster (not while loading).
//! - A buff on its recipient ([`buff_cast`]; `SkillBuff::ReceiveActiveUpdate`): `skillCastSound`
//!   going active, `skillUnCastSound` going inactive.
//! - Projectile flight: `projectileSwipeSound` tracked with the projectile; `PlayBounceSound`
//!   only from `ProjectileGrenade::CollisionCallback`'s first non-water contact.
//! - The stun sound ([`StunSound`]; `Character::PlayStunSound`): on entering the stun / sleep
//!   state, and each time a trap / freeze buff is created (`SkillBuff_DebufTrap::Install`), not
//!   on a refresh.
//!
//! PORT: a strike is the frame's attacks of one attacker; a strike that reaches no target plays
//! no `skillHitSound`; the not-evading gate is no miss; `ParametersCombat.critSoundGate` (not
//! identified) is taken as clear; a breakable's `impactSound` is not played; world contact plays
//! the destruct sound; every non-weapon cast plays the activation sounds when it fires; a path
//! charge's `skillHitSound` plays as it starts; tether and link loops play on their target, a
//! tether's end sound at the caster; a beam's `groundSound` is [`beam_ground_sounds`]'; a
//! monster's channels make no loop; `skillDeactivatedSound` plays for the player's toggles,
//! timed self buffs and forms, for monsters' and pets' timed buffs running out and for dispels;
//! `skillCastSound` / `skillUnCastSound` for every buff another character puts on one, when
//! created, not on a refresh; bounce and weapon enchantment sounds are not played.
use crate::char_sounds::sound;
use crate::combat::AttackMade;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::character::{AnimCallback, Character};
use gd_game_core::combat::Combatant;
use gd_game_core::data::GameData;
use gd_game_core::player::Player;
use gd_game_core::visuals::{PlaySound, SoundLoop};

/// `FadeLastPlayedSound`'s time for most skill loops (s).
pub const LOOP_FADE: f32 = 0.5;
/// `FadeLastPlayedSound`'s time for a wave's and a line fan's loop (s).
pub const SHORT_LOOP_FADE: f32 = 0.25;

pub struct CombatSoundsPlugin;

/// The combat sound writers that run after the frame's combat and casting; `play_requests`
/// orders after it. `swipe_sounds` stays out: it must read the swipe callbacks before
/// `CastingSet` advances them.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CombatSoundsSet;

impl Plugin for CombatSoundsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<StunSound>()
            .configure_sets(
                Update,
                CombatSoundsSet
                    .after(gd_game_core::combat::CombatSet)
                    .after(crate::casting::CastingSet)
                    .after(gd_game_core::afflictions::AfflictionStage::Tick),
            )
            .add_systems(
                Update,
                (
                    swipe_sounds.before(crate::casting::CastingSet),
                    (
                        hit_sounds,
                        spell_hit_sounds,
                        held_loops,
                        beam_ground_sounds,
                        stun_sounds,
                        critical_fx,
                        crate::char_sounds::low_health_vox,
                    )
                        .in_set(CombatSoundsSet),
                ),
            );
    }
}

/// A creature's hand items and body armour, from its look (the player's come from the
/// `Inventory`).
#[derive(Component, Clone, Debug, Default)]
pub struct Gear {
    pub right: Option<String>,
    pub left: Option<String>,
    pub chest: Option<String>,
}

/// A worn item: its record and its illusion's (the transmute), if any.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeldItem {
    pub record: String,
    pub transmute: Option<String>,
}

impl HeldItem {
    pub fn new(record: &str) -> Self {
        HeldItem {
            record: record.to_owned(),
            transmute: None,
        }
    }

    /// The record whose swipe / hit sounds the weapon plays: the transmute's when it is a
    /// `Weapon`.
    fn weapon_record(&self, data: &GameData) -> Option<Record> {
        self.transmute
            .as_deref()
            .and_then(|t| data.db.get_owned(t))
            .filter(|r| is_weapon(&r.class))
            .or_else(|| data.db.get_owned(&self.record))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub right: Option<HeldItem>,
    pub left: Option<HeldItem>,
    pub chest: Option<HeldItem>,
}

/// Weapon classes (`Weapon` and its RTTI children, shields and off-hands included).
fn is_weapon(class: &str) -> bool {
    class.starts_with("Weapon")
}

fn field(r: &Record, k: &str) -> Option<String> {
    r.str(k).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// The worn items of characters: the player's inventory, creatures' [`Gear`].
#[derive(SystemParam)]
pub struct Holdings<'w, 's> {
    /// Option: the player's worn items are empty without it.
    inv: Option<Res<'w, gd_game_core::inventory::Inventory>>,
    players: Query<'w, 's, (), With<Player>>,
    gear: Query<'w, 's, &'static Gear>,
}

impl Holdings<'_, '_> {
    pub fn of(&self, e: Entity) -> Held {
        if self.players.contains(e) {
            return self.inv.as_deref().map(player_held).unwrap_or_default();
        }
        let Ok(g) = self.gear.get(e) else {
            return Held::default();
        };
        let item = |r: &Option<String>| r.as_deref().map(HeldItem::new);
        Held {
            right: item(&g.right),
            left: item(&g.left),
            chest: item(&g.chest),
        }
    }
}

/// The player's attached main hand, off hand and chest items, with their illusions.
pub fn player_held(inv: &gd_game_core::inventory::Inventory) -> Held {
    use gd_formats::inventory::EquipSlot as S;
    let slot = |s: S| {
        let i = gd_game_core::inventory::slot_index(s);
        inv.equipped[i]
            .as_ref()
            .filter(|_| inv.is_attached(i))
            .filter(|it| !it.base_name.is_empty())
            .map(|it| HeldItem {
                record: it.base_name.clone(),
                transmute: Some(it.transmute_name.clone()).filter(|t| !t.is_empty()),
            })
    };
    Held {
        right: slot(S::MainHand),
        left: slot(S::OffHand),
        chest: slot(S::Chest),
    }
}

/// The sound paks a creature's weapons play, for the renderer's sound preload.
pub fn weapon_sound_paks(data: &GameData, held: &Held) -> Vec<String> {
    let mut out = Vec::new();
    for h in [&held.right, &held.left] {
        let Some(w) = h
            .as_ref()
            .and_then(|h| h.weapon_record(data))
            .filter(|r| is_weapon(&r.class))
        else {
            continue;
        };
        out.extend(field(&w, "swipeSound"));
        out.extend(field(&w, "hitSound"));
    }
    out
}

/// The sound paks a skill's profile plays, for the renderer's sound preload.
pub fn skill_sound_paks(data: &GameData, skill: &str) -> Vec<String> {
    let Some(r) = profile_record(data, skill) else {
        return Vec::new();
    };
    [
        "skillSwipeSound",
        "skillHitSound",
        "skillActivatedSound",
        "skillCastSound",
        "skillUnCastSound",
        "skillDeactivatedSound",
        "skillWarmUpSound",
    ]
    .into_iter()
    .filter_map(|k| field(&r, k))
    .collect()
}

/// The sounds of a weapon skill's swipe callback `name` (module doc).
pub fn swipe(
    data: &GameData,
    c: &Character,
    held: &Held,
    name: &str,
    skill: Option<&str>,
) -> Vec<String> {
    let mut out = Vec::new();
    let hand = match name {
        "SwipeRight" => Some(&held.right),
        "SwipeLeft" => Some(&held.left),
        "SwipeBoth" | "Swipe" => None,
        _ => return out,
    };
    if let Some(hand) = hand {
        let weapon = hand
            .as_ref()
            .and_then(|h| h.weapon_record(data))
            .filter(|r| is_weapon(&r.class));
        match weapon.filter(|_| c.form_mesh().is_none()) {
            Some(w) => out.extend(field(&w, "swipeSound")),
            None => out.extend(sound(data, c, "swipeSound")),
        }
    }
    out.extend(
        skill
            .and_then(|s| profile_record(data, s))
            .and_then(|r| field(&r, "skillSwipeSound")),
    );
    out
}

/// The attacker's sounds of a melee strike (module doc); `landed`: hit, the target not evading.
pub fn strike(
    data: &GameData,
    c: &Character,
    held: &Held,
    skill: Option<&str>,
    landed: bool,
) -> Vec<String> {
    let mut out: Vec<String> = skill
        .and_then(|s| profile_record(data, s))
        .and_then(|r| field(&r, "skillHitSound"))
        .into_iter()
        .collect();
    if !landed {
        return out;
    }
    let mut armed = false;
    for hand in [&held.right, &held.left] {
        if let Some(w) = hand
            .as_ref()
            .and_then(|h| h.weapon_record(data))
            .filter(|r| is_weapon(&r.class))
        {
            armed = true;
            out.extend(field(&w, "hitSound"));
        }
    }
    if !armed {
        out.extend(sound(data, c, "attackSound"));
    }
    out
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Struck {
    /// Not missed.
    pub hit: bool,
    pub blocked: bool,
    /// Combat type 1 (melee).
    pub melee: bool,
    pub critical: bool,
    /// The absolute damage.
    pub total: f32,
}

/// The struck character's sounds (module doc); `roll` is 0 .. 100 against `impactSoundChance`.
pub fn struck(
    data: &GameData,
    c: &Character,
    held: &Held,
    s: Struck,
    roll: impl FnOnce() -> f32,
) -> Vec<String> {
    let mut out = Vec::new();
    if !s.hit {
        return out;
    }
    if s.blocked
        && let Some(r) = held
            .left
            .as_ref()
            .and_then(|h| data.db.get(&h.record))
            .filter(|r| r.class == "WeaponArmor_Shield")
    {
        out.extend(field(r, "blockSound"));
    }
    if s.melee {
        let chance = data
            .db
            .get(&c.record)
            .and_then(|r| r.f32("impactSoundChance"))
            .unwrap_or(0.0);
        if roll() < chance {
            out.extend(sound(data, c, "impactSound"));
        }
        out.extend(
            held.chest
                .as_ref()
                .and_then(|h| data.db.get(&h.record))
                .and_then(|r| field(r, "armorImpactSound")),
        );
    }
    if s.critical && s.total >= 1.0 {
        out.extend(sound(data, c, "criticalHitSound"));
    }
    out
}

/// A ranged weapon shot's sound at the shooter: the right-hand weapon's (else the left's)
/// `hitSound`.
pub fn shot(data: &GameData, held: &Held) -> Option<String> {
    let w = [&held.right, &held.left].into_iter().find_map(|h| {
        h.as_ref()
            .and_then(|h| h.weapon_record(data))
            .filter(|r| is_weapon(&r.class))
    })?;
    field(&w, "hitSound")
}

/// A skill's activation sounds at the caster (module doc).
pub fn activate(data: &GameData, c: &Character, skill: &str) -> Vec<String> {
    let Some(r) = profile_record(data, skill) else {
        return Vec::new();
    };
    let mut out: Vec<String> = field(&r, "skillActivatedSound").into_iter().collect();
    for n in 1..=4 {
        if r.bool(&format!("playSpecialAttackSound{n}")) {
            out.extend(sound(data, c, &format!("specialAttackSound{n}")));
        }
    }
    out
}

pub fn spell_swipe(data: &GameData, skill: &str) -> Option<String> {
    profile_record(data, skill).and_then(|r| field(&r, "skillSwipeSound"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwipeBy {
    /// `SkillActivatedWeapon::SwipeAction`, the default attack without a cast.
    Weapon(String),
    /// The spells' and buffs' `SwipeAction`: the skill's `skillSwipeSound` only.
    Spell(String),
}

/// The action a swipe callback of `anim` belongs to: the cast the character is doing (or an
/// ally cast), the default attack (the Attack slot), else the cast whose hit already fired in
/// this animation.
pub fn swipe_action(
    anim: gd_sim::anim::AnimType,
    cast: Option<&gd_game_core::casting::Casting>,
    ally: Option<&str>,
    last: Option<&gd_game_core::casting::LastCast>,
) -> Option<SwipeBy> {
    use gd_game_core::casting::{DEFAULT_ATTACK, Kind};
    match (cast, ally) {
        (Some(c), _) if c.kind == Kind::Weapon => Some(SwipeBy::Weapon(c.skill.clone())),
        (Some(c), _) => Some(SwipeBy::Spell(c.skill.clone())),
        (None, Some(s)) => Some(SwipeBy::Spell(s.to_owned())),
        (None, None) if gd_sim::anim::is_attack(anim) => {
            Some(SwipeBy::Weapon(DEFAULT_ATTACK.to_owned()))
        }
        (None, None) => last.filter(|l| l.anim == anim).map(|l| {
            if l.weapon {
                SwipeBy::Weapon(l.skill.clone())
            } else {
                SwipeBy::Spell(l.skill.clone())
            }
        }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectileEvent {
    /// Hit a target (exploding there or not).
    Hit { exploding: bool },
    /// Ended without a hit and exploded (`explodeOnMiss`).
    MissExplode,
    /// Ended without a hit (full distance, world contact).
    Destruct,
}

pub fn projectile_hit(data: &GameData, projectile: &str, ev: ProjectileEvent) -> Option<String> {
    let r = data.db.get(projectile)?;
    let key = match ev {
        ProjectileEvent::Hit { exploding: false } => "projectileHitSound",
        ProjectileEvent::Hit { exploding: true } | ProjectileEvent::MissExplode => {
            "projectileExplodingHitSound"
        }
        ProjectileEvent::Destruct => "projectileDestructSound",
    };
    field(r, key)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitAt {
    /// Each projectile it creates, at the launch point.
    Launch,
    /// Each target of its `TargetResult`, at the target.
    Target,
    /// Once per activation, at the caster.
    Caster,
    /// Once per activation, at the skill's point.
    Point,
}

/// The `skillHitSound` rule of skill class `class` other than [`HitAt::Launch`], if any.
pub fn hit_at(class: &str) -> Option<HitAt> {
    match class {
        "Skill_AttackSpell"
        | "Skill_AttackWave"
        | "Skill_AttackSpellCone"
        | "Skill_AttackSpellBeam"
        | "Skill_AttackSpellDrain"
        | "Skill_AttackRadiusGrow"
        | "Skill_AttackRadiusSpin"
        | "Skill_AttackLineFan"
        | "SkillSecondary_AttackRadiusLightning"
        | "Skill_AttackSpellChaos"
        | "Skill_AttackSpellChaosSpawnPet"
        | "Skill_AttackChain"
        | "SkillSecondary_ChainLightning"
        | "SkillSecondary_ChainBonus" => Some(HitAt::Target),
        "Skill_AttackRadius"
        | "Skill_AttackRadiusLightning"
        | "Skill_BuffAttackRadiusLightning"
        | "Skill_AttackRadiusLeap"
        | "Skill_AttackRadiusDisengage"
        | "Skill_AttackPathCharge"
        | "Skill_AttackProjectileBurst"
        | "Skill_SpawnPet"
        | "Skill_RefreshCooldown"
        | "Skill_GiveBonus"
        | "Skill_ChargePotion"
        | "Skill_OnHitAttackRadius"
        | "Skill_OnHitBuffSelf"
        | "Skill_CharonGeysers" => Some(HitAt::Caster),
        "Skill_TargetedSpawnPet"
        | "SkillSecondary_TargetedSpawnPet"
        | "Skill_AttackRadiusLightningSpawnPet"
        | "Skill_AttackBuff"
        | "Skill_AttackBuffRadius"
        | "Skill_BuffRadius" => Some(HitAt::Point),
        _ => None,
    }
}

/// Skill `skill`'s `skillHitSound` for the event `at`, when its class plays it there.
pub fn spell_hit(data: &GameData, skill: &str, at: HitAt) -> Option<String> {
    if at != HitAt::Launch && hit_at(&data.db.get(skill)?.class) != Some(at) {
        return None;
    }
    profile_record(data, skill).and_then(|r| field(&r, "skillHitSound"))
}

/// The record a skill's sounds come from: its profile (a buff wrapper's `buffSkillName` record).
pub fn profile_record(data: &GameData, skill: &str) -> Option<gd_formats::arz::Record> {
    gd_sim::skills::profile_record(&data.db, skill)
}

/// The weapon-pool parent's own sound after a pool attack's (`Skill_WPAttack::PlaySwipeSound` /
/// `PlayHitSound`), unless the parent lacks `Skill.isParent` (`Skill_WeaponPool_ChargedFinale`
/// clears it). `key` is `skillSwipeSound` or `skillHitSound`.
pub fn parent_sound(data: &GameData, parent: &str, key: &str) -> Option<String> {
    if parent.is_empty()
        || data
            .db
            .get(parent)
            .is_some_and(|r| r.class == "Skill_WeaponPool_ChargedFinale")
    {
        return None;
    }
    profile_record(data, parent).and_then(|r| field(&r, key))
}

pub fn deactivate(data: &GameData, skill: &str) -> Option<String> {
    profile_record(data, skill).and_then(|r| field(&r, "skillDeactivatedSound"))
}

/// A `SkillBuff` record going active (`skillCastSound`) or inactive (`skillUnCastSound`).
pub fn buff_cast(data: &GameData, buff: &str, active: bool) -> Option<String> {
    data.db.get(buff).and_then(|r| {
        field(
            r,
            if active {
                "skillCastSound"
            } else {
                "skillUnCastSound"
            },
        )
    })
}

/// The projectile's launch sound (`projectileSwipeSound`, tracked with it).
pub fn projectile_launch(data: &GameData, projectile: &str) -> Option<String> {
    data.db
        .get(projectile)
        .and_then(|r| field(r, "projectileSwipeSound"))
}

/// `Character::PlayStunSound` on `holder` (module doc).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StunSound {
    pub holder: Entity,
}

/// A character's `stunSound` entry (a form's first).
pub fn stun(data: &GameData, c: &Character) -> Option<String> {
    sound(data, c, "stunSound")
}

fn stun_sounds(
    data: Res<GameData>,
    mut stuns: MessageReader<StunSound>,
    chars: Query<(&Character, &Transform)>,
    mut out: MessageWriter<PlaySound>,
) {
    for s in stuns.read() {
        let Ok((c, tf)) = chars.get(s.holder) else {
            continue;
        };
        let Some(pak) = stun(&data, c) else { continue };
        debug!("{}: stun -> {pak}", s.holder);
        out.write(PlaySound {
            pak,
            at: Some(tf.translation),
        });
    }
}

/// The per-target `skillHitSound`s ([`HitAt::Target`]) of the frame's non-projectile skill hits.
fn spell_hit_sounds(
    data: Res<GameData>,
    mut hits: MessageReader<gd_game_core::casting::SkillHit>,
    at: Query<&Transform>,
    mut out: MessageWriter<PlaySound>,
) {
    for h in hits.read() {
        if h.projectile {
            continue;
        }
        let Some(pak) = spell_hit(&data, &h.skill, HitAt::Target) else {
            continue;
        };
        let Ok(tf) = at.get(h.target) else { continue };
        debug!("{}: skill hit -> {pak}", h.target);
        out.write(PlaySound {
            pak,
            at: Some(tf.translation),
        });
    }
}

/// Swipe callbacks at the character ([`swipe_action`]). Characters without combat (the paper
/// doll) make none.
fn swipe_sounds(
    data: Res<GameData>,
    mut callbacks: MessageReader<AnimCallback>,
    chars: Query<
        (
            &Character,
            &Transform,
            Option<&gd_game_core::casting::Casting>,
            Option<&crate::monster_skills::AllyCast>,
            Option<&gd_game_core::casting::LastCast>,
        ),
        With<Combatant>,
    >,
    picks: Query<&crate::weapon_pool::PoolPick>,
    holdings: Holdings,
    mut out: MessageWriter<PlaySound>,
) {
    for cb in callbacks.read() {
        if !matches!(
            cb.name.as_str(),
            "Swipe" | "SwipeRight" | "SwipeLeft" | "SwipeBoth"
        ) {
            continue;
        }
        let Ok((c, tf, cast, ally, last)) = chars.get(cb.entity) else {
            continue;
        };
        let paks = match swipe_action(cb.anim, cast, ally.map(|a| a.skill.as_str()), last) {
            Some(SwipeBy::Weapon(skill)) => {
                // `SkillActivatedWeaponPool::SwipeAction` forwards to the picked
                // `Skill_WPAttack`, which plays its own sound and then the parent's.
                // PORT: the picked attack's own animation is not played.
                let pick = picks
                    .get(cb.entity)
                    .ok()
                    .filter(|p| p.is_for(&skill))
                    .and_then(|p| p.pick.as_ref());
                match pick {
                    Some((wp, _)) => {
                        let mut v = swipe(&data, c, &holdings.of(cb.entity), &cb.name, Some(wp));
                        v.extend(parent_sound(&data, &skill, "skillSwipeSound"));
                        v
                    }
                    None => swipe(&data, c, &holdings.of(cb.entity), &cb.name, Some(&skill)),
                }
            }
            Some(SwipeBy::Spell(skill)) => spell_swipe(&data, &skill).into_iter().collect(),
            None => continue,
        };
        for pak in paks {
            debug!("{}: {} -> {pak}", cb.entity, cb.name);
            out.write(PlaySound {
                pak,
                at: Some(tf.translation),
            });
        }
    }
}

struct HeldLoop {
    owner: Entity,
    skill: String,
    target: Option<Entity>,
    loops: Vec<Entity>,
}

/// The channels', tethers' and reflect links' sounds (module doc): `startSound` and the loops when
/// one begins, `endSound` and the loops' fade when it is gone. PORT: every class by the same
/// fields; class-specific `startSound` moments (a drain's first target) are its start here.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn held_loops(
    mut commands: Commands,
    data: Res<GameData>,
    channels: Query<(Entity, &gd_game_core::casting::Channel)>,
    tethers: Query<(Entity, &gd_game_core::secondary::Tethers)>,
    links: Query<(Entity, &gd_game_core::reflect_link::ReflectLinks)>,
    changed: Query<
        (),
        Or<(
            Changed<gd_game_core::casting::Channel>,
            Changed<gd_game_core::secondary::Tethers>,
            Changed<gd_game_core::reflect_link::ReflectLinks>,
        )>,
    >,
    mut gone_channels: RemovedComponents<gd_game_core::casting::Channel>,
    mut gone_tethers: RemovedComponents<gd_game_core::secondary::Tethers>,
    mut gone_links: RemovedComponents<gd_game_core::reflect_link::ReflectLinks>,
    at: Query<&Transform>,
    mut held: Local<Vec<HeldLoop>>,
    mut out: MessageWriter<PlaySound>,
) {
    let gone =
        gone_channels.read().count() + gone_tethers.read().count() + gone_links.read().count();
    if gone == 0 && changed.is_empty() {
        return;
    }
    let mut now: Vec<(Entity, &str, Option<Entity>)> = channels
        .iter()
        .map(|(e, c)| (e, c.skill.as_str(), None))
        .collect();
    now.extend(tethers.iter().flat_map(|(e, t)| {
        t.0.iter()
            .map(move |t| (e, t.skill.as_str(), Some(t.target)))
    }));
    now.extend(links.iter().flat_map(|(e, l)| {
        l.0.iter()
            .map(move |l| (e, l.skill.as_str(), Some(l.target)))
    }));
    held.retain(|h| {
        if now
            .iter()
            .any(|n| (n.0, n.1, n.2) == (h.owner, h.skill.as_str(), h.target))
        {
            return true;
        }
        for e in &h.loops {
            commands.entity(*e).try_despawn();
        }
        if let (Some(pak), Ok(tf)) = (
            data.db.get(&h.skill).and_then(|r| field(r, "endSound")),
            at.get(h.owner),
        ) {
            out.write(PlaySound {
                pak,
                at: Some(tf.translation),
            });
        }
        false
    });
    for (owner, skill, target) in now {
        if held
            .iter()
            .any(|h| (h.owner, h.skill.as_str(), h.target) == (owner, skill, target))
        {
            continue;
        }
        let (Some(r), Ok(tf)) = (data.db.get(skill), at.get(owner)) else {
            continue;
        };
        let spot = tf.translation;
        out.write_batch(field(r, "startSound").map(|pak| PlaySound {
            pak,
            at: Some(spot),
        }));
        let mut loops = Vec::new();
        if let Some(pak) = field(r, "loopSound") {
            loops.push(
                commands
                    .spawn(SoundLoop {
                        pak,
                        on: Some(target.unwrap_or(owner)),
                        at: spot,
                        fade: LOOP_FADE,
                    })
                    .id(),
            );
        }
        held.push(HeldLoop {
            owner,
            skill: skill.to_owned(),
            target,
            loops,
        });
    }
}

/// A beam's `groundSound` (`Skill_AttackSpellBeam::StartEndEffects` / `DestroyBeams`): a ray that
/// meets a surface starts it at the emit point (looping, not tracked) when it is not playing, a
/// ray that meets none stops it, and the node going fades it over 500 ms.
/// PORT: the node's rays share one pak, so the last ray decides; a stop fades over 500 ms too.
fn beam_ground_sounds(
    mut commands: Commands,
    data: Res<GameData>,
    casters: Query<(Entity, &gd_game_core::casting::SpellBeams)>,
    mut playing: Local<Vec<(Entity, u64, Entity)>>,
) {
    if playing.is_empty() && casters.is_empty() {
        return;
    }
    playing.retain(|(owner, id, e)| {
        let live = casters.get(*owner).is_ok_and(|(_, b)| {
            b.0.iter()
                .any(|n| n.id == *id && n.surface.last() == Some(&true))
        });
        if !live {
            commands.entity(*e).try_despawn();
        }
        live
    });
    for (owner, beams) in &casters {
        for n in beams.0.iter().filter(|n| n.surface.last() == Some(&true)) {
            if playing.iter().any(|p| (p.0, p.1) == (owner, n.id)) {
                continue;
            }
            let Some(pak) = data.db.get(&n.skill).and_then(|r| field(r, "groundSound")) else {
                continue;
            };
            let e = commands
                .spawn(SoundLoop {
                    pak,
                    on: None,
                    at: n.emit,
                    fade: LOOP_FADE,
                })
                .id();
            playing.push((owner, n.id, e));
        }
    }
}

/// The critical hit effect (`CombatManager::TakeAttack`): a crit (multiplier > 1) of the local
/// player of at least 1 damage spawns at the defender `combatformulas.dbr`'s `criticalhitFxPak`
/// (melee, style 1) or `projectileCritFxPak` (ranged weapon, style 2); other styles and
/// non-players spawn nothing.
/// PORT: `weapon && !melee` stands for style 2 (which projectile paths use it is not traced).
fn critical_fx(
    mut commands: Commands,
    data: Res<GameData>,
    mut made: MessageReader<AttackMade>,
    players: Query<(), With<gd_game_core::player::Player>>,
    chars: Query<&Transform>,
) {
    for m in made.read() {
        if m.miss.is_some() || m.multiplier <= 1.0 || m.total < 1.0 || !players.contains(m.attacker)
        {
            continue;
        }
        let key = if m.melee {
            "criticalhitFxPak"
        } else if m.weapon {
            "projectileCritFxPak"
        } else {
            continue;
        };
        let (Some(pak), Ok(tf)) = (
            data.db
                .get("records/game/combatformulas.dbr")
                .and_then(|r| r.str(key).map(str::to_owned))
                .filter(|p| !p.is_empty()),
            chars.get(m.target),
        ) else {
            continue;
        };
        gd_game_core::visuals::spawn_fx(
            &mut commands,
            &data,
            &pak,
            Transform::from_translation(tf.translation),
        );
    }
}

/// The attacker's [`strike`] and the defender's [`struck`] sounds of the frame's attacks.
fn hit_sounds(
    data: Res<GameData>,
    mut made: MessageReader<AttackMade>,
    mut skill_hits: MessageReader<gd_game_core::casting::SkillHit>,
    chars: Query<(&Character, &Transform)>,
    holdings: Holdings,
    mut out: MessageWriter<PlaySound>,
) {
    let parents: Vec<(Entity, &str, &str)> = skill_hits
        .read()
        .filter_map(|h| {
            let p = h.parent.as_ref().filter(|p| !p.0.is_empty())?;
            Some((h.attacker, h.skill.as_str(), p.0.as_str()))
        })
        .collect();
    let mut strikes: Vec<(Entity, Option<std::sync::Arc<str>>, bool)> = Vec::new();
    for m in made.read() {
        if m.melee {
            let landed = m.miss.is_none();
            match strikes.iter_mut().find(|s| s.0 == m.attacker) {
                Some(s) => s.2 |= landed,
                None => strikes.push((m.attacker, m.skill.clone(), landed)),
            }
        }
        let Ok((c, tf)) = chars.get(m.target) else {
            continue;
        };
        let s = Struck {
            hit: m.miss.is_none(),
            blocked: m.blocked,
            melee: m.melee,
            critical: m.multiplier > 1.0,
            total: m.total,
        };
        for pak in struck(&data, c, &holdings.of(m.target), s, || {
            gd_game_core::rng::f32() * 100.0
        }) {
            debug!("{}: struck -> {pak}", m.target);
            out.write(PlaySound {
                pak,
                at: Some(tf.translation),
            });
        }
    }
    for (attacker, skill, landed) in strikes {
        let Ok((c, tf)) = chars.get(attacker) else {
            continue;
        };
        let skill = skill
            .as_deref()
            .unwrap_or(gd_game_core::casting::DEFAULT_ATTACK);
        let mut paks = strike(&data, c, &holdings.of(attacker), Some(skill), landed);
        // The pool attack's parent's `skillHitSound` follows its own.
        if let Some(p) = parents
            .iter()
            .find(|p| p.0 == attacker && p.1.eq_ignore_ascii_case(skill))
            && let Some(pak) = parent_sound(&data, p.2, "skillHitSound")
        {
            let own =
                profile_record(&data, skill).is_some_and(|r| field(&r, "skillHitSound").is_some());
            paks.insert(usize::from(own), pak);
        }
        for pak in paks {
            debug!("{attacker}: strike -> {pak}");
            out.write(PlaySound {
                pak,
                at: Some(tf.translation),
            });
        }
    }
}
