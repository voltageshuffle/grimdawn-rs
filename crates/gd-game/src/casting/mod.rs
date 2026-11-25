//! Skill casting: the hotbar (from the save), energy, cooldowns, cast
//! animations and the skill effects.
//!
//! A cast starts the caster's `Cast` state (the skill's
//! `skillSpecialAnimationName`, else the spell-cast animation); the effect
//! fires on its own clip's `RightHandHit`/`LeftHandHit` callback
//! (`Character::callbacks_after_cast`: never the "End" of the clip before it), like
//! attacks — the same frame the point crosses (`CharacterStage::Animate`
//! dispatches before the readers). Energy (`skillManaCost`) and cooldown
//! (`skillCooldownTime`, with its charges) are paid when the cast fires
//! (`fire_casts`: the engine's `SkillTargetResult` / `CreateProjectile` /
//! radius-activation moment), not at its start — an interrupted
//! wind-up pays nothing. A weapon charge's warm-up is the exception (it
//! pays at `WarmUpStart`, [`WarmPaid`]). A request arriving
//! mid-swing / mid-cast is remembered in the character's one pending-action slot
//! ([`gd_game_core::character::QueuedAction`], last wins) and runs at the clip's
//! `AllowInterrupt` point or its "End"; `instantCast` skills (activation mode 2)
//! bypass the queue and the animation entirely (`ActivateSkillConfigCmd`, fired
//! through [`ProcCast`]). Kinds by record class:
//! - weapon attacks (`Skill_WeaponPool_Default`, `Skill_WPAttack*`,
//!   `Skill_AttackWeapon*`): a hit on the target with `weaponDamagePct`;
//! - `Skill_AttackRadius`: every enemy within `skillTargetRadius` of the caster;
//! - targeted areas (`Skill_AttackRadiusLightning`, `…ProjectileAreaEffect`,
//!   `Skill_AttackBuffRadius`): within `skillTargetRadius` of the target point;
//! - projectiles (`Skill_AttackProjectile*`): `skillProjectileNumber` copies of
//!   `skillProjectileName` (velocity, distance, flight/impact FX);
//! - drops (`Skill_AttackProjectileDrop`, `Skill_BuffAttackRadiusDrop`):
//!   `numProjectiles` falling from `dropHeight` around the target, exploding
//!   with `projectileExplosionRadius`;
//! - self buffs (`Skill_BuffSelfDuration`, `Skill_BuffRadius`): the skill's
//!   stats for `skillActiveDuration` seconds; a `Skill_BuffRadius` reaches
//!   the caster's allies through its aura passes while it runs (`auras.rs`).
//!
//! A buff wrapper's stats, duration, radius, cost, cooldown and sounds are
//! its `buffSkillName` record's (`gd_sim::skills::profile_name`:
//! the wrappers' `Load` runs `LoadResources` on that table), however it is
//! cast: a hot slot cast runs as that record (`try_cast`; its cooldown runs
//! under the wrapper's name, the slot's, [`CastCooldownKey`]); procs (celestial
//! powers, items), consumables, monsters and pets run the wrapper, which
//! `fire_casts` resolves (the running buff keeps the wrapper's name; its
//! stats derive from the one `TimedBuffs` store at the profile rebuild).
//!
//! Ranges come from the skill's `distanceProfile` (`skill_range`); a
//! projectile touches what its `actorRadius` ray passes (`Projectile::touch`);
//! drops fly from above the landing spot at their own velocity; several
//! projectiles spread per `SkillActivated::ExecuteRangedAttack` (see each).
//!
//! Submodules: `potions`, `areas`, `ranged`, `leaps`, `auras`, `charges`, `caster`, `requests`,
//! `spell_beams`, `warmup`, `fire`, `projectiles`, `projectile_classes`, `attack_line`, `effects`,
//! `class_fx`, `chaos`.
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
        // `GD_FX_TEST` (hooks.rs): where it ran in the chain above.
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
        // The procs the frame's combat / trigger writers sent after
        // `fire_casts` ran (autocast triggers, secondaries, monster casts,
        // weapon-pool special clips): fired the same frame, through the
        // shared [`ProcCursor`] (`fire_casts` consumed the earlier ones).
        app.add_systems(
            Update,
            fire::fire_late_procs
                .after(crate::autocast::AutocastStage::Trigger)
                .after(crate::secondary::SecondarySet),
        );
    }
}

/// Skill casting systems.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CastingSet;

/// The kick (`SkillManager::GetDefaultSkillId(2)`, `Skill_Kick`): the primary
/// action on a `Destructible` whose `attackAnimation` is "Kick" uses it
/// whatever the left slot holds (`crate::hotbar::kicks_breakable`).
pub const DEFAULT_KICK: &str = "records/skills/default/defaultkickattack.dbr";
/// `Skill_GiveBonus::ActivateNow` gives its bonus to a target
/// within this many metres, edge to edge (`IsTargetInRange(…, 20.0)`).
const GIVE_BONUS_REACH: f32 = 20.0;
/// Projectile distance for a record that lacks `projectileDistance` (a data
/// fallback, not a range rule).
const MAX_RANGE: f32 = 20.0;

/// `Skill::GetCooldownTime`: the profile's
/// `skillCooldownTime` at the level × (1 − total `skillCooldownReduction`
/// %): the skill's own part (its record + modifiers, chance-gated,
/// [`own_skill_attribute`]) plus the character's (gear, passives:
/// `char_cdr`, already summed). Item skills and potions take no character
/// part (pass 0.0; `Skill.isItemSkill` / `Skill.isPotion`). The one cooldown rule every store
/// starts through (`Cooldowns::start` skips a result of 0.1 s or less).
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

/// The shared [`ProcCast`] read cursor: `fire_casts` consumes the procs
/// written before it, `fire_late_procs` (after the combat / trigger
/// writers) the rest — each proc fires once, the frame it was sent.
#[derive(Resource, Default)]
pub(crate) struct ProcCursor(pub(crate) bevy::ecs::message::MessageCursor<ProcCast>);

/// A skill's range (m): its `distanceProfile` → gameengine.dbr
/// `meleeRange` / `shortRange` / `moderateRange` / `longRange` /
/// `maximumRange` / `bossRange`, Melee when missing (`Skill::LoadResources`,
/// `Skill::GetRange`, `GameEngine::LoadFromDatabase`).
/// The usable distance adds both radii
/// (`Character::GetTargetDistance`) and 0.5.
pub fn skill_range(data: &GameData, rec: &Record) -> f32 {
    data.engine()
        .profile_range(rec.str("distanceProfile").unwrap_or("Melee"))
}

/// `Skill::GetGamepadRange` (the keyboard modes' aim distance, [`keyboard_aim`]): [`skill_range`], or the profile
/// of [`gd_sim::skills::gamepad_range_profile`].
pub fn gamepad_range(data: &GameData, rec: &Record) -> f32 {
    match gd_sim::skills::gamepad_range_profile(&rec.class) {
        Some(p) => data.engine().profile_range(p),
        None => skill_range(data, rec),
    }
}

/// `CloseEnoughToUseSkill`: usable distance = range + both radii
/// (`Character::GetTargetDistance`) + the state's
/// `GetSkillUseTolerance` (0.5 at rest, `ControllerAIStateT::
/// GetSkillUseTolerance`).
pub const MOVE_TOLERANCE: f32 = 0.5;

/// A skill's usable distance: [`skill_range`] + both radii + [`MOVE_TOLERANCE`] —
/// the one formula the walk-to-range and the fire-from-where-you-stand clamp share
/// (`try_cast`, `move_to_use_skill`).
pub fn usable_range(data: &GameData, rec: &Record, radii: f32) -> f32 {
    skill_range(data, rec) + radii + MOVE_TOLERANCE
}

/// Where a Shift-held (stationary) attack aims.
#[derive(Component, Clone, Copy, Debug)]
pub struct AimPoint(pub Vec3);

/// What [`keyboard_aim`] needs about the request and the player.
#[derive(Clone, Copy, Debug)]
pub struct KeyboardAim {
    /// The `movementType` option (0 Classic, 1 Keyboard, 2 Keyboard Only).
    pub movement_type: i32,
    pub move_type: gd_sim::skills::MoveType,
    pub controller: gd_game_core::player::ControllerDirection,
    /// The player's position and facing (+Z).
    pub pos: Vec3,
    pub facing: Vec3,
    /// [`gamepad_range`].
    pub range: f32,
    /// The `gamepadTargetLock` option.
    pub gamepad_lock: bool,
}

/// The keyboard movement modes' skill aim, `ControllerPlayerState::DefaultRequestSkillAction`
/// (InputMode 0): from the request's `target` (the combat
/// enemy / ally) and `point`, the target and point the skill is used at, and whether the point is then cut to the
/// straight walkable line from the player (`NavManager::FindStraightMovePointOnSlopes`). With `movementType` above 0:
/// - the direction is the controller direction while a movement key is held (its length above 0), else the facing;
/// - a `MoveAndUseSkill` skill (`Skill_AttackRadiusSpin`) ignores the target for the point, and standing (no key
///   held) aims at the player himself;
/// - a move-to-point skill (`MoveToPointSkill`: leaps, charges, teleports, disengage, evade) drops the target in
///   Keyboard Only without `gamepadTargetLock`, and always for an evade;
/// - with no target left, in Keyboard Only (any skill) or for a `MoveAndUseSkill` skill, the point is
///   [`KeyboardAim::range`] along the direction (a move-to-point skill but a disengage then takes the straight
///   walkable point);
/// - the target that stays is still used at its own position (the request's `point`). Classic leaves both as they are.
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
    // A target that stays wins over the computed point (its coordinates replace it).
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
