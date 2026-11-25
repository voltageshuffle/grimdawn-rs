//! Skill casting: the hotbar, energy, cooldowns, cast animations and the skill effects.
//!
//! A cast starts the caster's `Cast` state (`skillSpecialAnimationName`, else the spell-cast animation); the
//! effect fires on its own clip's `RightHandHit`/`LeftHandHit` callback, never the "End" of the clip before it.
//! Energy (`skillManaCost`) and cooldown (`skillCooldownTime`, with its charges) are paid when the cast fires
//! (`fire_casts`: `SkillTargetResult` / `CreateProjectile`), so an interrupted wind-up pays nothing; a weapon
//! charge's warm-up pays at `WarmUpStart` ([`WarmPaid`]). A request mid-swing / mid-cast waits in the
//! one pending-action slot ([`gd_game_core::character::QueuedAction`], last wins) until `AllowInterrupt` or
//! "End"; `instantCast` skills (activation mode 2) bypass the queue and the animation (`ActivateSkillConfigCmd`,
//! via [`ProcCast`]). Kinds by record class:
//! - weapon attacks (`Skill_WeaponPool_Default`, `Skill_WPAttack*`, `Skill_AttackWeapon*`): a hit on the target
//!   with `weaponDamagePct`;
//! - `Skill_AttackRadius`: every enemy within `skillTargetRadius` of the caster;
//! - targeted areas (`Skill_AttackRadiusLightning`, `…ProjectileAreaEffect`, `Skill_AttackBuffRadius`): within
//!   `skillTargetRadius` of the target point;
//! - projectiles (`Skill_AttackProjectile*`): `skillProjectileNumber` copies of `skillProjectileName`;
//! - drops (`Skill_AttackProjectileDrop`, `Skill_BuffAttackRadiusDrop`): `numProjectiles` falling from
//!   `dropHeight` around the target, exploding with `projectileExplosionRadius`;
//! - self buffs (`Skill_BuffSelfDuration`, `Skill_BuffRadius`): the skill's stats for `skillActiveDuration`
//!   seconds; a `Skill_BuffRadius` also reaches the caster's allies.
//!
//! A buff wrapper's stats, duration, radius, cost, cooldown and sounds are its `buffSkillName` record's
//! (the wrappers' `Load` runs `LoadResources` on that table). A hot slot cast runs as that record, its
//! cooldown under the wrapper's name ([`CastCooldownKey`]); procs, consumables, monsters and pets run the
//! wrapper, which `fire_casts` resolves (the running buff keeps the wrapper's name).
use crate::combat::MonsterAi;
use crate::movement::{MovePath, MoveToUseSkill};
use crate::nav::Nav;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::character::{AnimCallback, CharState, Character};
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::player::{Loadout, Player, ProfileDirty};

pub struct CastingPlugin;

impl Plugin for CastingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SkillHit>()
            .add_message::<CastRequest>()
            .add_message::<CastOnAlly>()
            .add_message::<SkillNotReady>()
            .add_message::<UsePotion>()
            .init_resource::<ProcCursor>()
            .add_message::<SpawnPet>()
            .add_message::<ProcCast>()
            .add_message::<Dispel>()
            .add_systems(
                Update,
                (class_fx::channel_fx, class_fx::charge_fx).after(CastingSet),
            )
            .add_systems(
                Update,
                (
                    dispels.after(CastingSet),
                    (shields, potion_over_time, self_buff_heals)
                        .before(gd_game_core::afflictions::AfflictionStage::Tick),
                ),
            )
            .add_systems(
                Update,
                (player_damage_toggles, damage_auras)
                    .chain()
                    .in_set(CastingSet)
                    .after(gd_game_core::player::PlayerSet)
                    .before(gd_game_core::combat::CombatSet),
            )
            .add_systems(
                Update,
                (
                    stop_skills,
                    init_caster,
                    sync_spell_attacker,
                    sync_ranged.run_if(resource_exists::<gd_game_core::inventory::Inventory>),
                    regen_and_cooldowns,
                    potions,
                    (cast_requests, move_to_use_skill, end_warm_ups).chain(),
                    spell_attack_target,
                    ranged_swings,
                    (fire_casts, chaos::chaos_beams).chain(),
                    charges,
                    (path_charges, leaps, note_charge_starts).chain(),
                    channels,
                    spell_beams,
                    (storms, expanding_radii),
                    areas,
                    fly_projectiles,
                    (drop_ground_fx, attack_lines),
                    (expire_buffs, immobilize_holds),
                )
                    .chain()
                    .in_set(CastingSet)
                    .after(gd_game_core::player::PlayerSet)
                    .before(gd_game_core::combat::CombatSet),
            );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            fx_test
                .after(potions)
                .before(cast_requests)
                .in_set(CastingSet)
                .after(gd_game_core::player::PlayerSet)
                .before(gd_game_core::combat::CombatSet),
        );
        // The procs sent after `fire_casts` ran (autocast triggers, secondaries, monster casts): fired the same
        // frame through the shared [`ProcCursor`].
        app.add_systems(
            Update,
            fire::fire_late_procs
                .after(crate::autocast::AutocastStage::Trigger)
                .after(crate::secondary::SecondarySet),
        );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CastingSet;

/// The kick (`SkillManager::GetDefaultSkillId(2)`): the primary action on a `Destructible` whose
/// `attackAnimation` is "Kick" uses it whatever the left slot holds.
pub const DEFAULT_KICK: &str = "records/skills/default/defaultkickattack.dbr";
/// `Skill_GiveBonus::ActivateNow`'s reach, edge to edge (`IsTargetInRange(…, 20.0)`).
const GIVE_BONUS_REACH: f32 = 20.0;
/// Projectile distance for a record that lacks `projectileDistance` (a data fallback).
const MAX_RANGE: f32 = 20.0;

/// `Skill::GetCooldownTime`: `skillCooldownTime` at the level × (1 − total `skillCooldownReduction` %): the
/// skill's own part ([`own_skill_attribute`]) plus the character's `char_cdr` (0.0 for item skills and potions).
/// `Cooldowns::start` skips a result of 0.1 s or less.
pub(crate) fn cooldown_for(
    rec: &Record,
    li: usize,
    mods: &[(&Record, usize)],
    char_cdr: f32,
) -> f32 {
    let cdr = own_skill_attribute(
        std::iter::once((rec, li)).chain(mods.iter().copied()),
        "skillCooldownReduction",
    ) + char_cdr;
    level_f(rec, "skillCooldownTime", li) * (1.0 - cdr * 0.01)
}

/// The shared [`ProcCast`] read cursor of `fire_casts` and `fire_late_procs`: each proc fires once, the frame
/// it was sent.
#[derive(Resource, Default)]
pub(crate) struct ProcCursor(pub(crate) bevy::ecs::message::MessageCursor<ProcCast>);

/// A skill's range (m): its `distanceProfile` → gameengine.dbr `meleeRange` … `bossRange`, Melee when missing
/// (`Skill::GetRange`).
pub fn skill_range(data: &GameData, rec: &Record) -> f32 {
    data.engine()
        .profile_range(rec.str("distanceProfile").unwrap_or("Melee"))
}

/// `Skill::GetGamepadRange` (the keyboard modes' aim distance): [`skill_range`], or the profile of
/// [`gd_sim::skills::gamepad_range_profile`].
pub fn gamepad_range(data: &GameData, rec: &Record) -> f32 {
    match gd_sim::skills::gamepad_range_profile(&rec.class) {
        Some(p) => data.engine().profile_range(p),
        None => skill_range(data, rec),
    }
}

/// `ControllerAIStateT::GetSkillUseTolerance` at rest (`CloseEnoughToUseSkill`).
pub const MOVE_TOLERANCE: f32 = 0.5;

/// A skill's usable distance: [`skill_range`] + both radii (`Character::GetTargetDistance`) +
/// [`MOVE_TOLERANCE`]; walk-to-range and the fire-in-place clamp share it.
pub fn usable_range(data: &GameData, rec: &Record, radii: f32) -> f32 {
    skill_range(data, rec) + radii + MOVE_TOLERANCE
}

/// Where a Shift-held (stationary) attack aims.
#[derive(Component, Clone, Copy, Debug)]
pub struct AimPoint(pub Vec3);

#[derive(Clone, Copy, Debug)]
pub struct KeyboardAim {
    /// The `movementType` option (0 Classic, 1 Keyboard, 2 Keyboard Only).
    pub movement_type: i32,
    pub move_type: gd_sim::skills::MoveType,
    pub controller: gd_game_core::player::ControllerDirection,
    pub pos: Vec3,
    pub facing: Vec3,
    pub range: f32,
    pub gamepad_lock: bool,
}

/// The keyboard movement modes' skill aim (`ControllerPlayerState::DefaultRequestSkillAction`, InputMode 0):
/// the target and point the skill is used at, and whether the point is cut to the straight walkable line
/// (`NavManager::FindStraightMovePointOnSlopes`). Classic leaves both as they are; with `movementType` above 0:
/// - the direction is the controller direction while a movement key is held, else the facing;
/// - a `MoveAndUseSkill` skill ignores the target for the point, and standing aims at the player himself;
/// - a move-to-point skill drops the target in Keyboard Only without `gamepadTargetLock`, and always for an
///   evade;
/// - with no target left, in Keyboard Only or for a `MoveAndUseSkill` skill, the point is
///   [`KeyboardAim::range`] along the direction (straight walkable for a move-to-point skill but disengage).
pub fn keyboard_aim(
    a: &KeyboardAim,
    target: Option<Entity>,
    point: Vec3,
) -> (Option<Entity>, Vec3, bool) {
    use gd_sim::skills::MoveType;
    if a.movement_type <= 0 {
        return (target, point, false);
    }
    let held = a.controller.length > 0.0;
    let mut dir = if held {
        a.controller.dir
    } else {
        a.facing.with_y(0.0).normalize_or_zero()
    };
    let drops = a.move_type.to_point()
        && ((a.movement_type == 2 && !a.gamepad_lock) || a.move_type == MoveType::Evade);
    // A target that stays is used at its own position.
    let target = target.filter(|_| !drops);
    if target.is_some() {
        return (target, point, false);
    }
    if a.move_type == MoveType::MoveAndUse && !held {
        dir = Vec3::ZERO;
    }
    if a.movement_type == 2 || a.move_type == MoveType::MoveAndUse {
        let straight = a.move_type.to_point() && a.move_type != MoveType::Disengage;
        return (None, a.pos + dir * a.range, straight);
    }
    (None, point, false)
}

mod potions;
pub use self::potions::*;
mod areas;
pub use self::areas::*;
mod ranged;
pub use self::ranged::*;
mod leaps;
use self::leaps::*;
mod auras;
pub use self::auras::*;
mod charges;
pub use self::charges::*;
mod caster;
pub use self::caster::*;
mod requests;
pub use self::requests::*;
mod warmup;
pub use self::warmup::*;
mod fire;
use self::fire::*;
mod projectiles;
pub(crate) use self::projectiles::*;
mod projectile_classes;
pub(crate) use self::projectile_classes::*;
mod attack_line;
pub use self::attack_line::*;
mod effects;
pub(crate) use self::effects::*;
use gd_game_core::casting::attach_point_bind;
use gd_game_core::casting::{
    BeamNode, CastCooldownKey, CastOnAlly, CastRequest, CastWrapper, Caster, Casting, Channel,
    ChannelFx, ChaosBeam, Charge, ChargeFx, ChargeStarts, ConeFx, Cooldowns, DEFAULT_ATTACK,
    DamageAura, DamageAuras, Dispel, ENERGY_POTION, ExpandingRadius, HEALTH_POTION,
    IMMOBILIZE_HOLD_SECS, IMMOBILIZE_PERIOD_SECS, ImmobilizeRefresh, Kind, LastCast, Leap,
    MESH_PREFIX, PathCharge, PotionOverTime, ProcCast, ProjMods, RangedWeapon, SLOT_LEFT, SLOTS,
    SelfBuffHeals, SelfBuffToggles, Shield, ShieldKind, Shields, SkillHit, SkillNotReady, SpawnPet,
    SpellAttacker, SpellBeams, Storm, Use, UsePotion, Walk, WarmPaid, WarmUp, bonus_life,
    char_pak_sounds, charges_to_range, cone_rotation, cone_slope, fan, grow_radius, is_leap,
    is_self_buff_class, kind_of, level_f, own_skill_attribute, pak_effects, qualifying_types,
    ranged_weapon, resource_behavior, spawn_on, spell_left, start_char_pak_sounds, storm_landing,
    walks_into_range, warms_up,
};
use gd_game_core::visuals::{BuffFx, ProjectileMesh, ProjectileModelOf, spawn_fx};
mod spell_beams;
pub use self::spell_beams::*;
mod chaos;
mod class_fx;
