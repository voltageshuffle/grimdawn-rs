//! Pets: summoned by `Skill_TargetedSpawnPet` / `Skill_SpawnPet` casts. The pet record is
//! `spawnObjects[skill level − 1]` at the owner's character level (`Skill_SpawnPet::SpawnPet`); at
//! most `petLimit` per summon skill, the oldest goes first.
//!
//! Mini pets (`Skill_SpawnMiniPet`, consumables): ordinary pets flagged mini in the `PetPen`
//! (`PetPenEntry.mini`). `KillExtraPets` (after the new pet joined) kills the skill's pets beyond
//! `petLimit`, then the owner's mini pets of any skill beyond `gameengine.dbr` `miniPetLimit` (3;
//! 0 = no limit), oldest first ([`Pet::mini`]), via `PetPen::ReleaseEntry(mode 0)` =
//! `KillMe(overkillOnDismiss)`.
//!
//! Skills: the record's `skillName{n}` at `skillLevel{n}` (equations of `charLevel`); skills
//! unlocked by the owner's pet modifiers (`SkillSecondary_PetModifier.petSkillName`) take the
//! modifier's level. The primary attack is the first active attack skill with a level, else a melee
//! swing; `specialAttack{,2,3}SkillName` fire with `specialAttack*Chance` % (rolled whenever
//! `ChooseBestSkill` runs; one that passes its roll but is out of reach is held, `Pet::held`) once
//! `specialAttack*Delay` s have passed, after their timeout, while the foe is inside the
//! `specialAttack*Range` band ([`PetAttack::band`]).
//!
//! Pet bonuses (`GameEngine::SetPetBonus`, applied by `ContributePetBonus*Attributes` like a skill
//! record at its level): the `petBonusName` of the owner's learned skills (devotions included) at
//! their level ([`pet_bonuses`]), of the player's running buffs while they run, and of every worn
//! item's records at level 1. A bonus with `petTypeNames` reaches only pets whose `petTypeNames`
//! share a name (either list empty: it applies; `Game::ShareAnyName`). Bonuses are live:
//! [`refresh_profiles`] rebuilds the pets' profiles when the owner's loadout changes.
//! `isPetBonusScaling` is read only by tooltips.
//!
//! `PetPlayerScaling` pets (totems, turrets, ...; `Skill::CollectCombatParameters`): when the
//! attacker's leader is a `Player`, that player's accumulator, racial bonuses and skill manager stand
//! in for the attacker. Port: [`Pet::player_scaling`] makes `combat::apply_hits` attack with the
//! owner's profile; what the pet takes back is defended by its own profile.
//!
//! Pet bonus effects ([`PetBonusFx`]): a bonus record's `charFxPakPetNames` (per level) is a
//! character effect pak on every pet it reaches (`Skill::InstallPetBonus` →
//! `GameEngine::AddPetBonusFxPak`, keyed by skill id; `Player::PostPetSpawn` gives them to later
//! pets). Item bonuses use skill id 0, so never show effects. [`sync_pet_bonus_fx`] spawns the paks
//! as `BuffFx` under [`pet_bonus_fx_key`]. PORT: a dying pet drops them at once.
//!
//! Self buffs (`ControllerMonster::ChooseBestBuffSelfSkill`, from the 4 s `WheneverPossible` check,
//! the enemy-found / attacked / ally responses and the Attack state's "End"): the first of
//! `buffSelfSkillName`, `buffSelf2…`, `buffSelf3…` the pet has and that is not on yet. A toggle stays
//! on (`Pet::toggled`); a timed self buff lasts its `skillActiveDuration` (`Pet::buffs`). Initial
//! skills (`initialSkillName`, `initial2SkillName`): toggles are on from the summon, others are cast
//! on the pet once at the start (`ControllerMonsterStateStartup::UseInitialSkillIfSet`). Buff other:
//! as a monster's, the candidates being live friends within `ViewDistance` that have no leader or are
//! quest pets (`ChooseBestAllyToBuff`) and lack the buff. Pets have a mana pool, full at the summon
//! ([`Pet::energy`]). Buffs and initial skills wait out their `skillCooldownTime`.
//! PORT: the defend / attacked responses are folded into "found a target"; `instantCast` toggles
//! skip the cast animation.
//!
//! AI: pets run the monster AI (`ControllerPet` is a `ControllerMonster`) with the record's
//! `controller` and the owner as leader:
//! - targets: every 200 ms the angriest foe (`AngerManager::GetNewTarget`: × 1.3 for the current
//!   target, at least `AngerTolerance`; `petTargetGreatestHealth` ranks by life percent first;
//!   `ignoreSleepingEnemies`); anger from sight on the 250 ms `AngerUpdate` tick and from hits taken
//!   (`AttackedAnger`, at most every 500 ms), scaled by the foe's `angerMultiplier`, decaying by
//!   `ForgiveRate`; a foe counts while within `MaxPursuitDistance` of the leader and
//!   `MaxYViewDistance` in height;
//! - skills fire once the centre distance ≤ both radii + `Skill::GetRange` + 0.5
//!   (`CloseEnoughToUseSkill`);
//! - the swing timer re-arms from `min/maxSwingPause` as an attack starts (`ResetSwingTimer`);
//! - without a target (`FollowLeader` / `DefendLeader`): every 300–700 ms it walks to its slot on the
//!   owner's defence ring at both radii + 0.5 (`Character::RequestDefenseSlot`); farther than
//!   `TeleportToLeaderDistance` (0: never) it is put on its slot, only in those two states. A
//!   follower that loses its slot stands as `DefendLeader` until its next check. Fighting is the
//!   monster's state machine (melee.rs);
//! - a `ControllerStationaryMonster` / `ControllerTotem` pet never moves: no follow or teleport, a
//!   move order only clears its anger, it attacks what is in reach; a totem picks the nearest foe on
//!   its anger list.
//!
//! Commands ([`PetCommand`]): the "Pet Attack" skill (`Skill_PetAttack`, granted to every player)
//! orders every pet of the caster: a target under the cursor → `RequestAllyAttackConfigCmd`, else a
//! `RequestAllyMoveConfigCmd` to the ground point; greyed out without a pet. Selected pets get one
//! order each on the next world click (`InGameUI::OrderSelectedPets`).
//! - attack (`DefaultRequestAttackResponse`): when `IsEnemyValid` (a live foe within
//!   `MaxPursuitDistance` of the leader), the target becomes the most hated enemy until no longer
//!   valid; `FollowLeader` / `DefendLeader` `OnBegin` clear it.
//! - move (`DefaultRequestMoveResponse`): anger cleared, state "Move": run to the point, "Idle" at
//!   the end or on failure. A moving pet does not fight and ignores attack requests, but gains anger.
//! - Passive states ignore both; Startup ignores requests from a player ([`Pet::startup`]). Startup
//!   plays animation type 19 (Spawn) once and leaves for Idle at its "End" (at once without one).
//!   PORT: "End" is the scripted animation's state ending.
//! - Sounds: `petAttackSound` / `petAcknowledgeSound`; no stock record sets them.
//!
//! Stances ([`Stance`]): `controller` (Normal), `controllerAggressive`, `controllerDefensive`. The
//! portrait menu (`PlayerPortrait::OnMenuPick`; key actions 15 / 13 cycle) calls
//! `Monster::UseController` on that pet (missing record: no change) and stores the type per summon
//! skill (`Player::SetPetControllerType`) for its new pets; not saved. The portrait shows Aggressive
//! for a skill with no stored type (`GetPetControllerType` defaults to 1). Each controller has its own
//! anger list, so a switch starts the AI afresh ([`PetStances`], [`SetPetStance`]).
//!
//! Portraits ([`portrait_pets`]): only pets whose record sets `showStatusWidgetWhenPet`
//! (`GameEngine::RegisterLocalPet`), in summon order. "Disband Pet" ([`DisbandPet`]): the pet dies.
//!
//! Lifetime ([`Pet::time_to_live`]): every summon asks `Skill::GetSpawnObjectTimeToLive`
//! ([`spawn_time_to_live`]: `spawnObjectsTimeToLive` of the skill and its modifiers summed; a learned
//! `Skill_SpawnPetTransmuter`'s `modSpawnObjectsTimeToLive` replaces it); > 0 →
//! `ControllerMonster.lifetime` (ms), counted down by game time whatever the state and kept across a
//! stance switch. At 0: `KillMe(overkillOnDismiss)`, an ordinary death, so the dying skill goes off
//! ([`pet_dying_skills`]); `overkillOnDismiss` set: an overkill ([`crate::combat::Overkill`]). The HUD
//! shows no timer.
//!
//! Release (`ReleasePetConfigCmd::Execute`, [`ReleasePet`] / [`DisbandPet`]): `overkill = max(life, 0) >=
//! maxLife x gibThreshold` ([`Pet::gib_threshold`]; absent: never) and, with the command's flag,
//! `Character::SuppressDyingSkill` ([`SuppressDyingSkill`]); then `KillMe(overkill)`. Disband passes
//! false: the dying skill goes off.
//!
//! Release on a lost skill bonus (`ItemSkillAugment::RemoveAugments`; [`AugmentsRemoved`],
//! [`removed_augments`], [`released_by_augments`]): when a skill-bonus container comes off the main
//! player (item detach, a component / augment removed or replaced, a set-count change: the previous
//! count's values), each non-zero bonus releases pets of the normal pen with the suppress flag:
//! - `augmentSkillName{n}`: `Skill::ReleasePets` on the player's skill of that name: its pets and
//!   those of its base skills (`Skill.baseSkills`; `weapon_pool::SkillLinks`);
//! - `augmentMasteryName{n}`: `PetPen::ReleaseMasteryPets`, every skill of that mastery;
//! - `augmentAllLevel`: `PetPen::ReleaseLearnedSkillPets`, every non-item skill.
//!
//! A weapon swap is such a detach (an attach releases nothing). PORT: several set pieces changing in
//! one equipment change count as one move (every count in between releases); a pet's skill counts as
//! "the player's" when the skill book lists it.
//!
//! `Skill_SpawnPet::StopSkill` ([`StopSummonSkill`]; owner death `SkillManager::ImDead`, item skills
//! removed `RemoveItemSkills`) kills all the skill's live pets, `KillExtraPets` the oldest beyond
//! `petLimit` (0 = unlimited), both with `KillMe(overkillOnDismiss)` ([`dismiss`]).
//! `Skill_SpawnQuestPet` pets ([`Pet::quest`]) live in a separate pen; a stop while the owner lives
//! (`GetReleaseMode` 2) runs `Monster::UnJoinLeader(false)`: the pet stays alive and friendly but
//! leaves ([`release_quest_pet`], [`ReleasedQuestPet`]); a new quest pet releases the old ones the
//! same way. PORT: no stock skill uses the class; `questPetVector` is taken as empty.
//!
//! Special attack timeouts ([`PetAttack::timeout`], `Monster::InitSkillsInController`): each slot's
//! timeout (left starts at it) counts down only once the controller has found an enemy;
//! `ChooseBestSkill` takes a special only when delay and timeout are out and `rand() % 100 < chance`.
use crate::combat::{MonsterAi, advance_steered, run_speed};
use crate::nav::Nav;
use bevy::prelude::*;
use gd_game_core::casting::{Casting, Kind, SpawnPet, kind_of};
use gd_game_core::character::{CharState, Character};
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::player::{Loadout, Player};
use gd_game_core::rules::CombatRules;
use gd_game_core::visuals::spawn_fx;
use std::sync::Arc;

pub struct PetsPlugin;

impl Plugin for PetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PetCommand>()
            .add_message::<SetPetStance>()
            .add_message::<DisbandPet>()
            .add_message::<ReleasePet>()
            .add_message::<AugmentsRemoved>()
            .add_message::<StopSummonSkill>()
            .add_systems(
                Update,
                (
                    crate::melee::sync_slots,
                    spawn_pets.run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                    pet_commands,
                    expire_pets,
                    stop_summon_skills,
                    release_augment_pets,
                    disband_pets,
                    pet_dying_skills,
                    pet_ai,
                    crate::auras::pulse_auras.in_set(crate::auras::AurasSet),
                    refresh_profiles.run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                    sync_pet_bonus_fx,
                )
                    .chain()
                    .after(crate::casting::CastingSet)
                    .before(gd_game_core::combat::CombatSet),
            );
    }
}

mod types;
pub use self::types::*;
pub mod placement;
mod spawn;
use self::spawn::*;
mod commands;
pub use self::commands::*;
mod ai;
use self::ai::*;
mod bonuses;
pub use self::bonuses::*;
