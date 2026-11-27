//! The melee combat loop: life, attacking a target, monster aggro/chase, death.
//!
//! Damage is `gd_sim::actor::attack_linked` (`TakeAttack`), applied when the attack animation crosses its
//! `RightHandHit`/`LeftHandHit` callback (`AttackAction`). Monster behaviour reads the creature's `controller`
//! record. Melee reach = both actor radii + [`MELEE_RANGE`] (`Skill::IsTargetInRange`); the hit-callback re-check
//! uses `meleeTargetDistance` with a cone fallback ([`targets_in_cone`]).
//!
//! Aggro is anger (`AngerManager`, see [`MonsterAi`]): sight ([`sight_anger`]), hits and distress calls
//! ([`anger_from_hits`]) fill it, and the monster goes for the target `AngerManager::GetNewTarget` picks.
//! Distress calls: `Character::UnderAttack` → `DoDistressCall` tells the friends within `distressCallRange`.
//! Bodies: `Character::InitialUpdate` picks the death handler by `deleteBehavior` ([`corpse_time`]).
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
            // Speeds reach the animation the frame the profile rebuilds them.
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
            // The combat core is a dependency chain: the global-RNG draws must keep their order (`notify_monsters` →
            // `anger_rolls` → `monster_ai` → `player_attack` → `apply_hits` → `anger_from_hits`); messages and
            // shared `MonsterAi` / `Combatant` / `Transform` / `Character` writes link the rest.
            .add_systems(
                Update,
                (
                    quest_interrupt,
                    quest_walk,
                    crate::dodge::notify_monsters,
                    crate::anger_rolls::update,
                    monster_ai,
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
            // `ambush_dissolve` shares nothing with the core, but stays after `monster_ai`, whose deferred
            // `AmbushDissolve` insert must flush first.
            .add_systems(
                Update,
                ambush_dissolve
                    .after(monster_ai)
                    .in_set(CombatSet)
                    .after(gd_game_core::player::PlayerSet),
            );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            auto_fight
                .after(monster_ai)
                .before(player_attack)
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
    pub blocked: bool,
    /// Taunt the attack exerted: the anger `ControllerMonster::TauntMe` adds for the attacker
    /// (`gd_sim::combat::Influence::taunt`).
    pub taunt: f32,
    /// The attack's instant-damage types (`CombatAttributeAccumulator::GetDamageTypes` of
    /// `ParametersCombat`, hit filter): the weapon's physical / pierce parts and typed hits,
    /// DoTs and crowd control excluded. Collected only for a target with on-hit procs
    /// ([`crate::autocast::Autocasts`]); empty for anyone else. PORT: the resolved hit's applied
    /// types, so a hit an absorb filter took whole carries none.
    pub types: Vec<gd_sim::attributes::CombatType>,
}

/// An attack made on a target, hit or miss (`SkillServices_Character::AttackingEnemy`): the `AttackEnemy(Crit)`
/// autocast triggers.
#[derive(Message, Clone, Debug)]
pub struct AttackMade {
    pub attacker: Entity,
    pub target: Entity,
    /// The skill used (None: a plain weapon swing).
    pub skill: Option<Arc<str>>,
    /// Crit multiplier > 1.
    pub critical: bool,
    /// A weapon attack: it carries the attacker's weapon ids; spells pass none.
    pub weapon: bool,
    /// Combat type 1 (a plain swing or a weapon skill, not a projectile).
    pub melee: bool,
    /// The outcome (`TakeAttack`, the floating combat numbers).
    pub miss: Option<gd_sim::combat::MissReason>,
    pub blocked: bool,
    pub multiplier: f32,
    pub tier: u32,
    pub total: f32,
    /// The attack's OA and the defender's DA, debuffs included (`CombatManager::RecordHit`).
    pub offensive_ability: f32,
    pub defensive_ability: f32,
}

/// A reflect link's passed-on attack (`ReflectLinkDamage` → `Character::AttackTarget`): runs the caster's
/// `AttackEnemy(Crit)` triggers like [`AttackMade`] but is not shown again.
#[derive(Message, Clone, Debug)]
pub struct LinkedAttack {
    pub attacker: Entity,
    pub target: Entity,
    pub skill: Option<Arc<str>>,
    pub critical: bool,
    pub weapon: bool,
}

/// A retaliation (`TakeAttack` style 3) or reflection (style 6) hit back at the attacker (`target`): `TakeAttack`'s
/// outcome and combat numbers (neither can be blocked; reflection's multiplier is
/// 1 − `defensivePercentReflectionResistance` %), but no `Character::AttackTarget`, so no [`AttackMade`].
#[derive(Message, Clone, Debug)]
pub struct CounterHit {
    pub attacker: Entity,
    pub target: Entity,
    pub hit: gd_sim::actor::Hit,
}

/// The largest single life subtraction an attack made on `target` (`PlayStats::TallyDamageReceived` type 1).
#[derive(Message, Clone, Copy, Debug)]
pub struct LifeSubtracted {
    pub target: Entity,
    pub amount: f32,
}

/// The melee hit re-check: the swing lands while the 3-D distance minus both radii is within `mtd`
/// (`meleeTargetDistance`).
pub fn in_melee_reach(from: Vec3, to: Vec3, radii: f32, mtd: f32) -> bool {
    (to - from).length() - radii <= mtd
}

/// `GameEngine::GetTargetsInCone` (`World::GetEntitiesInCone`): the targets within `range` of `at` (edge to edge,
/// 3-D) inside the facing's cone (`half_angle` radians, horizontal), most centred first (`World::CompareConeHits`).
/// `foes`: (entity, position, radius); the caller filters sides / hiding.
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
