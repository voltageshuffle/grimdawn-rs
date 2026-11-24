//! The melee combat loop: life, attacking a target, monster aggro/chase, death.
//!
//! Damage is `gd_sim::actor::attack_linked` (the engine's TakeAttack path), applied
//! when the attack animation crosses its `RightHandHit`/`LeftHandHit`
//! callback, as `AttackAction` does. Monster behaviour reads the creature's
//! `controller` record (ViewDistance, MaxYViewDistance, MaxPursuitDistance,
//! min/maxSwingPause). Melee reach = both actor radii + [`MELEE_RANGE`]
//! (`Skill::IsTargetInRange`: distance minus both radii ≤ the
//! skill's `Skill::GetRange`, `gameengine.dbr` `meleeRange` for the melee
//! profile); the hit-callback re-check uses `meleeTargetDistance`
//! ([`gd_game_core::rules::GameEngineRules::melee_target_distance`]) with a cone fallback ([`targets_in_cone`]).
//!
//! Aggro is anger (`AngerManager`, see [`MonsterAi`]): sight anger
//! ([`sight_anger`]), hits and distress calls ([`anger_from_hits`]) fill it
//! and the monster goes for the target `AngerManager::GetNewTarget` picks
//! (see `monster_ai`). Distress calls: `Character::UnderAttack`
//! → `DoDistressCall` tells the friends within
//! `distressCallRange` (`ControllerMonster::AllyAttacked`).
//! Bodies: `Character::InitialUpdate` picks the death handler by
//! `deleteBehavior` (see [`corpse_time`]).
//!
//! Submodules: `sight` (anger from sight / hits, speed sync), `ai` (controller classes, `MonsterAi`, flee,
//! movement helpers), `monster_ai` (the monster / auto-fight / player attack systems), `hits` (`apply_hits`,
//! block recovery), `influence` (convert and disruption), `bodies` (suicide, kill, corpses, regen, picking),
//! `cc_states` (fear / confusion states), `combatant` (`Combatant`, `Dead`, ...), `grid` (spatial indices).
use crate::monsters::ProxyStates;
use crate::movement::MovePath;
use crate::nav::Nav;
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_game_core::character::{AnimCallback, CharState, Character};
use gd_game_core::data::GameData;
use gd_game_core::monsters::Monster;
use gd_game_core::player::Player;
use gd_game_core::session::PlayerStart;
use gd_rules::Rng;
use std::sync::Arc;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<MonsterKilled>()
            .add_message::<HitLanded>()
            .add_message::<AttackMade>()
            .add_message::<CounterHit>()
            .add_message::<LifeSubtracted>()
            .add_message::<LinkedAttack>()
            .add_message::<Influenced>()
            .add_observer(body_bounds_on_combatant)
            .add_observer(body_bounds_on_character)
            // Speeds reach the animation the frame the profile rebuilds them
            // (the player's and the monsters' gear rebuild; pets rebuild at spawn).
            .add_systems(
                Update,
                sync_speeds
                    .after(crate::player::rebuild_profile)
                    .after(crate::monsters::equip_monsters)
                    .before(gd_game_core::character::CharacterStage::Drive),
            )
            .add_systems(Update, block_recovery)
            // Body boxes are this frame's pose: right after the advance in Update.
            .add_systems(
                Update,
                posed_bounds
                    .after(crate::character::advance_animations)
                    .in_set(gd_game_core::character::CharacterStage::Animate),
            )
            // ECS-1: the combat core is a genuine dependency chain, not a blanket
            // serialization. Every link below carries data: the global-RNG draws
            // (`gd_game_core::rng`, a thread-local stream) must keep their exact
            // mutual order — `notify_monsters` (dodge roll) → `anger_rolls` →
            // `monster_ai` (melee/flee/patrol/alert rolls) → `player_attack`
            // (`try_cast`/`los_spot`) → `apply_hits` (its session RNG seed) →
            // `anger_from_hits` (distress roll); the message flows
            // (`AttackMade`→`hit_aggro`, `HitLanded`/`MonsterKilled`→`anger_from_hits`,
            // `Influenced`→`join_converted`/`disrupt_skills`) and the shared
            // `MonsterAi`/`Combatant`/`Transform`/`Character` writes fix the rest.
            // None of these is component-disjoint from the next, so they stay ordered.
            .add_systems(
                Update,
                (
                    quest_interrupt,
                    quest_walk,
                    crate::dodge::notify_monsters,
                    crate::anger_rolls::update,
                    monster_ai,
                    auto_fight,
                    player_attack,
                    apply_hits.run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                    hit_aggro,
                    (join_converted, unjoin_converted)
                        .run_if(resource_exists::<crate::factions::PlayerFactions>),
                    disrupt_skills,
                    suicides,
                    anger_from_hits,
                    sight_anger,
                    dead_bodies,
                    player_regen,
                )
                    .chain()
                    .in_set(CombatSet)
                    .after(gd_game_core::player::PlayerSet),
            )
            // `ambush_dissolve` is the one combat system component-disjoint from the
            // core: it only advances `AmbushDissolve` (no other core system reads it),
            // draws no RNG and passes no message, so it overlaps the core freely. It
            // stays after `monster_ai`, which inserts `AmbushDissolve` deferred: the
            // flush must precede the `is_added` skip so a fresh dissolve starts ticking
            // the frame after its insert, exactly as the chain had it.
            .add_systems(
                Update,
                ambush_dissolve
                    .after(monster_ai)
                    .in_set(CombatSet)
                    .after(gd_game_core::player::PlayerSet),
            );
    }
}

/// A landed attack (autocast triggers): who hit whom, with what.
#[derive(Message, Clone, Debug)]
pub struct HitLanded {
    pub attacker: Entity,
    pub target: Entity,
    /// The skill used (None: a plain weapon swing).
    pub skill: Option<Arc<str>>,
    pub critical: bool,
    /// Melee (plain swings and weapon skills, not ranged weapons).
    pub melee: bool,
    /// The target's shield blocked it.
    pub blocked: bool,
    /// Taunt the attack exerted: the anger `ControllerMonster::TauntMe` adds
    /// for the attacker (`gd_sim::combat::Influence::taunt`: the skill's,
    /// the gear's and passives' `offensiveTaunt*`, after the defender's
    /// `defensiveTaunt`).
    pub taunt: f32,
}

/// An attack made on a target, hit or miss (`Character::AttackTarget`
/// calls `SkillServices_Character::AttackingEnemy` whatever the
/// outcome): the `AttackEnemy(Crit)` autocast triggers.
#[derive(Message, Clone, Debug)]
pub struct AttackMade {
    pub attacker: Entity,
    pub target: Entity,
    /// The skill used (None: a plain weapon swing).
    pub skill: Option<Arc<str>>,
    /// Critical (`ParametersCombat` crit multiplier > 1).
    pub critical: bool,
    /// A weapon attack (plain swing or weapon skill): it carries the
    /// attacker's weapon ids (`ParametersCombat.weaponIds`); spells
    /// pass none (`Skill_AttackSpell::TargetResult` / `Skill_AttackRadius::
    /// TargetResult` hand `AttackTarget` zero ids).
    pub weapon: bool,
    /// Combat type 1 (melee: a plain swing or a weapon skill, not a
    /// projectile; `TakeAttack`'s impact sound, `crate::combat_sounds`).
    pub melee: bool,
    /// The outcome (`TakeAttack`, the floating combat numbers): why it
    /// missed, the shield block, the hit roll's multiplier and critical tier,
    /// the absolute damage.
    pub miss: Option<gd_sim::combat::MissReason>,
    pub blocked: bool,
    pub multiplier: f32,
    pub tier: u32,
    pub total: f32,
    /// The attack's Offensive Ability and the defender's Defensive Ability,
    /// debuffs included (`gd_sim::actor::debuffed_offensive_ability` /
    /// `debuffed_defensive_ability`; `CombatManager::RecordHit`, `death.rs`).
    pub offensive_ability: f32,
    pub defensive_ability: f32,
}

/// A reflect link's passed-on attack (`ReflectLinkDamage` →
/// `Character::AttackTarget`): runs the caster's `AttackEnemy(Crit)` autocast
/// triggers like [`AttackMade`] but is not shown again (its numbers are the
/// [`CounterHit`]'s).
#[derive(Message, Clone, Debug)]
pub struct LinkedAttack {
    pub attacker: Entity,
    pub target: Entity,
    pub skill: Option<Arc<str>>,
    pub critical: bool,
    pub weapon: bool,
}

/// A retaliation (`TakeAttack` style 3) or reflection (style 6) hit back
/// at the original attacker (`target`). It runs `TakeAttack`'s outcome and
/// combat-number code like any attack (`CombatManager::TakeAttack`
///: neither style can be blocked; reflection's multiplier is
/// 1 − `defensivePercentReflectionResistance` %), but not
/// `Character::AttackTarget`, so no [`AttackMade`] (no autocasts).
#[derive(Message, Clone, Debug)]
pub struct CounterHit {
    pub attacker: Entity,
    pub target: Entity,
    pub hit: gd_sim::actor::Hit,
}

/// The largest single life subtraction an attack (hit, reflection, retaliation) made on `target`
/// (`Character::SubtractLife` → `PlayStats::TallyDamageReceived` type 1;
/// [`gd_sim::actor::Hit::largest_applied`]; the greatest hit received, `death.rs`).
#[derive(Message, Clone, Copy, Debug)]
pub struct LifeSubtracted {
    pub target: Entity,
    pub amount: f32,
}

/// The melee hit re-check (`mtd`: [`gd_game_core::rules::GameEngineRules::melee_target_distance`]): the swing
/// lands while the 3-D distance minus both radii (`radii`) is within `mtd`.
pub fn in_melee_reach(from: Vec3, to: Vec3, radii: f32, mtd: f32) -> bool {
    (to - from).length() - radii <= mtd
}

/// `GameEngine::GetTargetsInCone` (Engine `World::GetEntitiesInCone`):
/// the targets within `range` of `at` (edge to edge, 3-D) inside the facing's cone
/// (`half_angle` radians, horizontal), most centred first — the engine sorts by
/// descending `forward · direction` (comparator `World::CompareConeHits`). `foes`: the valid
/// targets as (entity, position, radius); the caller filters sides / hiding.
pub fn targets_in_cone(
    at: Vec3,
    fwd: Vec3,
    radius: f32,
    range: f32,
    half_angle: f32,
    foes: impl Iterator<Item = (Entity, Vec3, f32)>,
) -> Vec<Entity> {
    let mut hits: Vec<(Entity, f32)> = foes
        .filter_map(|(e, pos, r)| {
            let full = pos - at;
            let d = full.with_y(0.0);
            let key = d.dot(fwd) / full.length().max(1e-6);
            (full.length() - radius - r <= range
                && d.normalize_or_zero().dot(fwd).clamp(-1.0, 1.0).acos() <= half_angle)
                .then_some((e, key))
        })
        .collect();
    hits.sort_by(|a, b| b.1.total_cmp(&a.1));
    hits.into_iter().map(|(e, _)| e).collect()
}

mod sight;
use self::sight::*;
mod ai;
pub use self::ai::*;
mod monster_ai;
use self::monster_ai::*;
mod hits;
use self::hits::*;
mod influence;
pub use self::influence::*;
mod bodies;
pub mod cc_states;
pub use self::bodies::*;
mod combatant;
pub use self::combatant::*;
use gd_game_core::combat::{Attackers, CombatSet, Combatant, ControllerClass, Dead};
use gd_game_core::rules::CombatRules;
pub mod grid;
