//! Monster skills: which active skill a monster uses and when, after `ControllerMonster` (Game.dll).
//!
//! `Monster::InitSkillsInController` reads from the monster record, each resolved among the monster's own
//! skills (`skillName1..N` at `skillLevelN`, `SkillManager::FindSkillId`):
//! - `attackSkillName`: replaces the default attack;
//! - `specialAttack[2..8]SkillName` with `…Timeout`, `…Delay` (s), `…Chance` (%) and `…Range` (`ShortRange` 1 /
//!   `MediumRange` 2 / `LongRange` 3, anything else 0 = any distance);
//! - `buffSelf[2,3]SkillName`, `buffOther…`, `heal…`, `berserk…`, `dying…`, `initial…`;
//!   `short/medium/longRangeMin/Max`.
//!
//! `ControllerMonster::ChooseBestSkill` takes the first special slot whose delay and timeout countdowns are both
//! ≤ 0, that passes `rand() % 100 < chance`, and whose target is in range (`IsSkillInProperRange`: centre
//! distance − both radii, at least 0.5, strictly inside the band; band 0 always passes); else the attack skill
//! (in a shapeshift form every pick also passes the form's set gate, [`MonsterSkills::set_active_set`]).
//! `Update` counts the delays down always and the timeouts (initially the timeout) only once the monster found
//! an enemy; `SkillUsed` re-arms the used slot's delay.
//!
//! When (`ControllerMonsterStateAttack::OnUpdate`, `ControllerMonsterStatePursue::OnUpdate`): the next skill is
//! chosen when an attack ends and on finding an enemy; while the chosen skill is out of reach the monster
//! pursues and chooses again every 200 ms, unless `Skill::CanRechooseInPursuit` is false (weapon charges,
//! blinks, path charges, leaps, disengage, evade). A skill is used once the centre distance is within both
//! radii + its `Skill::GetRange` + 0.5 (`ControllerAIStateT::CloseEnoughToUseSkill`) and the swing timer
//! (`ResetSwingTimer`: `min/maxSwingPause`) has run out; the timer is re-armed as the attack starts.
//! `attackSkillName` of any kind, weapon attacks included, is the attack used. Line of sight: only
//! `Skill::IsTargetInLOS` reaches `World::CheckLOS`; `ControllerAIStateT<ControllerMonster>::UseSkill` checks it
//! only when `Skill::GetRequiresLOS` says so, and on failure, when the monster `CanMove`, moves to a nearby spot
//! that sees the target. The rays aim at the target's [`center_of_mass`]. `GetRequiresLOS` is true for the chain
//! skills (`Skill_AttackChain`, `Skill_AttackProjectileChain`, `Skill_AttackRadiusLightning*`, the chain
//! secondaries), conditional for `Skill_AttackProjectileBurst` / `Skill_GiveBonus` ([`requires_los`] /
//! [`los_spot`]), false otherwise.
//!
//! Self buffs (`ChooseBestBuffSelfSkill`, `BuffSelfBehavior`: NeverUseSkill / WhenIdle / WhenEnemyIsSeen
//! (default) / WheneverPossible): unless NeverUseSkill, the first of `buffSelf`, `buffSelf2`, `buffSelf3` that
//! is enabled and not already running is used when an enemy is found and when an attack ends; WheneverPossible
//! also every 4 s, in or out of combat. A monster's toggled skills turn on only when its AI uses them
//! (`AutoActivateNow` runs only for the main player), so a damaging aura that is neither a self buff nor an
//! initial skill stays off. A switched-on radius aura or running timed `Skill_BuffRadius` buffs the monster and
//! its friends in radius (a debuff one its foes) ([`MonsterSkills::emissions`]).
//!
//! Dying skill: `ControllerMonsterStateDying::OnBegin` → `StartDyingSkill` casts `dyingSkillName` at once, with
//! no cast animation (`ActivateSkillConfigCmd`), unless `Character::ShouldSuppressDyingSkill` (set by
//! `SuppressDyingSkill`, a flagged `ReleasePetConfigCmd`, and `Monster::UpdateSelf` for a non-pet monster outside
//! its endless-dungeon region): a summon whose time to live runs out ([`expire_minions`]) fires it.
//!
//! Heal: `Update` counts a timer down from `healSkillDelay` (when above 0) and calls `TryToHealAlly` at 0, then
//! re-arms it. `TryToHealAlly`: the leader (`Monster::GetLeader`) when its life is below
//! `healLeaderHealthPercentage` (default 0), else the friend with the lowest life fraction below
//! `healAllyHealthPercentage` (default 70) in its `ViewDistance` sphere (`MaxYViewDistance` in height when set)
//! other than itself, via `UseSkillOnAlly`. PORT: the candidates are the monsters. Each healer picks on its own.
//! The heal skills are `Skill_GiveBonus` ([`crate::casting::Bonus`]).
//!
//! Initial skills (`initialSkillName`, `initial2SkillName`): `UseInitialSkillIfSet` uses them on the monster
//! itself at startup; here when its skills are first set up. Chains: when the attack state ends after
//! `chainInitialSkill`, `chainNextSkill` follows at once by `chainBehavior`: UseOnCurrentEnemy (0), UseOnLeader
//! (1), UseOnSelf (2), UseOnAllies (3); the data uses only 0 and 2.
//!
//! Berserk: `Character::PlayLowHealthSoundIfNeeded` raises `LowHealth` once life drops to
//! `lowHealthTriggerLevel` % and re-arms at `lowHealthResetLevel` %; `DefaultLowHealthResponse` then flees when
//! `FleeBehavior` is FleeOnLowHealth and `CanFlee`, else pursues its target with `berserkSkillName`.
//!
//! Buff other: only `buffOtherSkillName` is used (`buffOther2/3` are never read), by `BuffAllyBehavior`
//! (default NeverUseSkill): on finding an enemy (WhenEnemyIsSeen) or every 4 s (WheneverPossible),
//! `ChooseBestAllyToBuff` picks among the live friends within `ViewDistance` other than itself and without a
//! leader ([`buff_ally_candidate`]) that lack the buff (`!Character::IsSkillBuffActive(buffSkillName)`), by
//! `BuffAllyTargeting`: BuffClosest (skipping `Npc`s), BuffStrongest (highest life) or BuffWeakest (lowest life
//! fraction). The caster needs the skill enabled and `IsValidTarget` before `UseSkillOnAlly`. The buff lands when
//! the cast goes off (`Skill_BuffOther::TargetResult`). A `Skill_BuffRadius` used there goes off on the caster:
//! one pass at once, heal included, then timed passes when its buff record has a `skillActiveDuration`; without
//! one it is instant. Its cooldown is the buff record's.
//!
//! Channelled skills ([`Kind::Beam`]) are left out of every monster slot: no stock monster has one. In the
//! engine only the player controllers re-use a running channel, so a monster's channel would last one
//! `duration` per attack.
use crate::combat::MonsterAi;
use crate::nav::Nav;
use bevy::prelude::*;
use gd_game_core::casting::level_f;
use gd_game_core::casting::{Casting, Kind, SpawnPet, kind_of};
use gd_game_core::character::{AnimCallback, CharState, Character};
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::monster_skills::{MIRAGE, Special};
use gd_game_core::monsters::Monster;
use gd_game_core::rules::CombatRules;
use gd_game_core::visuals::spawn_fx;
use std::cell::RefCell;
use std::sync::Arc;

pub struct MonsterSkillsPlugin;

impl Plugin for MonsterSkillsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AllyLanded>()
            .add_message::<crate::auras::Activated>()
            .add_systems(
                Update,
                (
                    (
                        init_skills.in_set(MonsterSkillStage::Init),
                        prefetch_summons,
                        quest_use_skill,
                        skill_timers,
                        ally_cast_callbacks,
                        choose_skills.in_set(MonsterSkillStage::Choose),
                        buff_allies,
                        heal_allies,
                        ally_casts_land,
                        dying_skills.in_set(MonsterSkillStage::Dying),
                        generators.in_set(MonsterSkillStage::Generators),
                    )
                        .chain()
                        .before(crate::auras::AurasSet),
                    (
                        rebuild_profiles
                            .in_set(MonsterSkillStage::Rebuild)
                            .run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                        spawn_minions.run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                        expire_minions,
                        mirage_availability,
                    )
                        .chain()
                        .after(crate::auras::AurasSet),
                )
                    .chain()
                    .after(crate::casting::CastingSet)
                    .before(gd_game_core::combat::CombatSet),
            );
    }
}

/// Starts parsing, in the background, the bodies and animation clips of every creature a new monster's skills
/// can summon, so its first summon does not parse them on the game thread mid-fight.
fn prefetch_summons(
    new: Query<&MonsterSkills, Added<MonsterSkills>>,
    mut assets: crate::creature::Spawner,
    mut anims: ResMut<gd_game_core::character::AnimCache>,
    mut done: Local<gd_formats::collections::HashSet<String>>,
    mut session: gd_game_core::session::NewSession,
) {
    if session.begun() {
        done.clear();
    }
    for ms in &new {
        let skills = ms
            .attack
            .iter()
            .map(|a| &a.0)
            .chain(ms.specials.iter().map(|s| &s.skill))
            .chain(ms.dying.iter().map(|d| &d.0))
            .chain(ms.initial.iter().map(|i| &i.0));
        for skill in skills {
            let Some(rec) = assets.data.db.get(skill) else {
                continue;
            };
            for object in rec.strs_iter("spawnObjects") {
                if !done.insert(object.to_owned()) {
                    continue;
                }
                if let Some(mesh) =
                    gd_game_core::creature::creature_mesh(&assets.data.db, object, 0)
                {
                    assets.bodies.ready(&assets.data, &mesh);
                }
                for p in gd_game_core::character::creature_clips(&assets.data, object, 0) {
                    anims.ready(&assets.data, &p);
                }
            }
        }
    }
}

/// `ControllerAIStateT::GetSkillUseTolerance`.
pub(crate) const SKILL_USE_TOLERANCE: f32 = 0.5;

/// `ControllerMonster::Update`: the WheneverPossible buff checks run every 4 s (first after 4 s).
const BUFF_POLL: f32 = 4.0;

/// `ControllerMonsterStatePursue::OnUpdate`: re-choose every 200 ms while pursuing.
const PURSUE_REPICK: f32 = 0.2;

/// `BuffSelfBehavior` / `BuffAllyBehavior` (`ControllerMonster::Load`).
pub(crate) fn behaviour(s: Option<&str>, default: u8) -> u8 {
    match s {
        Some("NeverUseSkill") => 0,
        Some("WhenIdle") => 1,
        Some("WhenEnemyIsSeen") => 2,
        Some("WheneverPossible") => 3,
        _ => default,
    }
}

/// The skill the monster is going for (`ChooseBestSkill`'s result, or the berserk skill).
#[derive(Clone, Debug)]
enum Pick {
    /// `attackSkillName`, else the plain swing.
    Attack,
    /// The plain swing though an attack skill is set: the form's set gate bars the attack skill.
    Plain,
    /// Nothing passes the form's set gate (`ChooseBestSkill` returns 0; PORT: the monster keeps its approach
    /// and chooses again at its next attack turn).
    Nothing,
    Special(usize),
    Skill(String, usize, Kind),
}

/// `SkillManager::LoadDefaultSkills`'s default attack (`ControllerMonster.defaultAttackSkill`).
const DEFAULT_ATTACK: &str = "records/skills/default/defaultwpbasicattack.dbr";

#[derive(Component, Clone, Debug)]
pub struct MonsterSkills {
    /// `attackSkillName`: (skill, level index, kind, `Skill::GetRange`).
    pub attack: Option<(String, usize, Kind, f32)>,
    pub specials: Vec<Special>,
    /// `(min, max)` of the short, medium and long range bands.
    pub bands: [(f32, f32); 3],
    /// Timed `buffSelf` skills: (skill, level index).
    pub buff_self: Vec<(String, usize)>,
    /// Toggled `buffSelf` skills, switched on by their first use.
    toggles: Vec<(String, usize)>,
    /// Toggled skills that are on, other than radius auras (those reach it as aura buffs): (stat record,
    /// level index).
    pub auras: Vec<(String, usize)>,
    /// `Skill_BuffAttackRadiusToggled`s that are on.
    damage_auras: Vec<(String, usize)>,
    toggled: Vec<(String, usize)>,
    pub buffs: gd_game_core::skill_buffs::TimedBuffs,
    /// `skillCooldownTime` left per used skill.
    cooldowns: Vec<(String, f32)>,
    /// `healSkillName`: (skill, level index, `healSkillDelay`, timer left).
    pub heal: Option<(String, usize, f32, f32)>,
    pub heal_pct: f32,
    /// `healLeaderHealthPercentage` (default 0: never).
    pub heal_leader_pct: f32,
    pub dying: Option<(String, usize, Kind)>,
    /// `initialSkillName` / `initial2SkillName`, until used.
    pub initial: Vec<(String, usize, Kind)>,
    /// (`chainInitialSkill`, `chainNextSkill`, on self).
    pub chain: Option<(String, (String, usize, Kind), bool)>,
    chain_pending: bool,
    /// `buffOtherSkillName`: (wrapper skill, level index, buff record, seconds).
    pub buff_other: Option<(String, usize, String, f32)>,
    /// `BuffAllyTargeting`: 0 closest, 1 strongest, 2 weakest.
    pub buff_targeting: u8,
    buff_self_behaviour: u8,
    buff_ally_behaviour: u8,
    poll: f32,
    /// A WheneverPossible ally buff is due.
    ally_poll: bool,
    ally_on_found: bool,
    /// `berserkSkillName`.
    pub berserk: Option<(String, usize, Kind)>,
    /// `Skill_MonsterGenerator`s: (skill, level index, seconds to the next spawn).
    pub generators: Vec<(String, usize, f32)>,
    /// `Skill_OnDeathSpawnActor`s.
    pub on_death: Vec<(String, usize)>,
    low_health: (f32, f32, bool),
    in_combat: bool,
    was_aggro: bool,
    /// An attack or cast was seen running: its end is next.
    attacking: bool,
    pick: Option<Pick>,
    repick: f32,
    dirty: bool,
    /// Skills `SetAvailability` turns off: a summon skill [`pen_gated`] while one of its summons lives.
    unavailable: Vec<String>,
    /// Its [`pen_gated`] skills among the specials and the attack.
    mirages: Vec<String>,
    /// `Character.activeSkillSet` (0 = default) and the skills `Skill::SetAvailability` turns off for it.
    active_set: i32,
    off_set: Vec<String>,
    /// The form's `allowedSkillTypes` (empty out of a form): `ChooseBestSkill`'s set gate.
    allowed: Vec<String>,
    form: Option<(String, usize)>,
}

impl Default for MonsterSkills {
    fn default() -> Self {
        Self {
            attack: None,
            specials: Vec::new(),
            bands: [(0.0, 4.0), (4.0, 8.0), (8.0, 18.0)],
            buff_self: Vec::new(),
            toggles: Vec::new(),
            auras: Vec::new(),
            damage_auras: Vec::new(),
            toggled: Vec::new(),
            buffs: gd_game_core::skill_buffs::TimedBuffs::default(),
            cooldowns: Vec::new(),
            heal: None,
            heal_pct: 70.0,
            heal_leader_pct: 0.0,
            dying: None,
            initial: Vec::new(),
            chain: None,
            chain_pending: false,
            buff_other: None,
            buff_targeting: 0,
            buff_self_behaviour: 2,
            buff_ally_behaviour: 0,
            poll: BUFF_POLL,
            ally_poll: false,
            ally_on_found: false,
            berserk: None,
            generators: Vec::new(),
            on_death: Vec::new(),
            low_health: (0.0, 0.0, false),
            in_combat: false,
            was_aggro: false,
            attacking: false,
            pick: None,
            repick: 0.0,
            dirty: false,
            unavailable: Vec::new(),
            mirages: Vec::new(),
            active_set: 0,
            off_set: Vec::new(),
            allowed: Vec::new(),
            form: None,
        }
    }
}

impl MonsterSkills {
    /// Dispelled: its timed buffs end and its switched-on toggles switch off (`DispellSelfBuff` → `StopSkill`),
    /// but those `keep` names (`notDispelable`); a toggle comes back only when its AI uses it again. The ended
    /// ones.
    pub fn dispel(
        &mut self,
        db: &gd_formats::vfs::Database,
        keep: impl Fn(&str) -> bool,
    ) -> Vec<String> {
        let mut gone: Vec<String> = self
            .buffs
            .list
            .iter()
            .filter(|b| !keep(&b.0))
            .map(|b| b.0.clone())
            .collect();
        self.buffs.list.retain(|b| keep(&b.0));
        let off: Vec<(String, usize)> = self
            .toggled
            .iter()
            .filter(|t| !keep(&t.0))
            .cloned()
            .collect();
        for (s, li) in &off {
            let stats = db
                .get(s)
                .and_then(|r| gd_sim::skills::profile_name(r).map(str::to_owned))
                .unwrap_or_else(|| s.clone());
            if let Some(i) = self.auras.iter().position(|a| a.0 == stats && a.1 == *li) {
                self.auras.remove(i);
            }
            self.damage_auras.retain(|a| &a.0 != s);
            gone.push(s.clone());
        }
        self.toggled.retain(|t| keep(&t.0));
        if !gone.is_empty() {
            self.dirty = true;
        }
        gone
    }

    /// `ControllerGraeae` / `ControllerMonsterSynergy` rewrite `ControllerMonster.attackSkill` and the special
    /// slots: the pending choice is dropped.
    pub(crate) fn set_kit(
        &mut self,
        attack: Option<(String, usize, Kind, f32)>,
        specials: Vec<Special>,
    ) {
        self.attack = attack;
        self.specials = specials;
        self.pick = None;
    }

    pub(crate) fn toggle_is_on(&self, skill: &str) -> bool {
        self.toggled.iter().any(|t| t.0 == skill)
    }

    /// A toggled skill switches on or off by its use (`ActivateNow`).
    pub(crate) fn set_toggle(
        &mut self,
        db: &gd_formats::vfs::Database,
        skill: &str,
        li: usize,
        on: bool,
    ) {
        if on {
            if let Some(r) = db.get(skill).filter(|_| !self.toggle_is_on(skill)) {
                self.toggle_on(r, skill, li);
            }
            return;
        }
        let Some(i) = self.toggled.iter().position(|t| t.0 == skill) else {
            return;
        };
        let (s, li) = self.toggled.remove(i);
        let stats = db
            .get(&s)
            .and_then(|r| gd_sim::skills::profile_name(r).map(str::to_owned))
            .unwrap_or_else(|| s.clone());
        if let Some(i) = self.auras.iter().position(|a| a.0 == stats && a.1 == li) {
            self.auras.remove(i);
        }
        self.damage_auras.retain(|a| a.0 != s);
        self.dirty = true;
    }

    /// The rules changed (the Crucible's ramp / mutators).
    pub fn mark_profile_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn add_buff(&mut self, skill: &str, li: usize, secs: f32) {
        self.buffs.add(skill, li, secs);
        self.dirty = true;
    }

    pub fn toggled(&self) -> &[(String, usize)] {
        &self.toggled
    }

    /// `Skill.active`: a toggle switched on or a timed buff running.
    pub(crate) fn skill_running(&self, skill: &str) -> bool {
        self.toggled
            .iter()
            .map(|t| &t.0)
            .chain(self.buffs.list.iter().map(|b| &b.0))
            .any(|s| s.eq_ignore_ascii_case(skill))
    }

    /// A toggled skill switches on: its stats (a radius aura's come back as aura buffs), and its damage when
    /// it is a `Skill_BuffAttackRadiusToggled`.
    fn toggle_on(&mut self, rec: &gd_formats::arz::Record, skill: &str, li: usize) {
        if rec.class != "Skill_BuffRadiusToggled" {
            self.auras.push((
                gd_sim::skills::profile_name(rec)
                    .map(str::to_owned)
                    .unwrap_or_else(|| skill.to_owned()),
                li,
            ));
        }
        if rec.class == "Skill_BuffAttackRadiusToggled" {
            self.damage_auras.push((skill.to_owned(), li));
        }
        self.toggled.push((skill.to_owned(), li));
        self.dirty = true;
    }

    /// Its auras on (a non-debuff one buffs the monster too): switched-on radius toggles and running
    /// `Skill_BuffRadius` buffs. Reads only the toggles' and buffs' skills and levels (`auras::CachedEmissions`
    /// relies on it).
    pub fn emissions(&self, db: &gd_formats::vfs::Database) -> Vec<crate::auras::Emission> {
        let toggles = self
            .toggled
            .iter()
            .filter(|(s, _)| {
                db.get(s)
                    .is_some_and(|r| r.class == "Skill_BuffRadiusToggled")
            })
            .map(|(s, li)| (s, *li));
        let timed = self
            .buffs
            .list
            .iter()
            .filter(|b| db.get(&b.0).is_some_and(|r| r.class == "Skill_BuffRadius"))
            .map(|b| (&b.0, b.1));
        toggles
            .chain(timed)
            .filter_map(|(s, li)| crate::auras::emission(db, s, li, Vec::new(), true))
            .collect()
    }

    fn from_record(data: &GameData, record: &str, level: i32) -> Option<Self> {
        let rec = data.db.get(record)?;
        let levels = gd_sim::actor::monster_skill_levels(rec, level);
        // `FindSkillId`: only the monster's own skills.
        let own = |key: &str| -> Option<(String, usize, gd_formats::arz::Record)> {
            let name = rec.str(key)?;
            let (n, l) = levels.iter().find(|(n, _)| n.eq_ignore_ascii_case(name))?;
            Some((n.clone(), *l as usize - 1, data.db.get_owned(n)?))
        };
        let band = |key: &str| match rec.str(key).unwrap_or("") {
            "ShortRange" => 1,
            "MediumRange" => 2,
            "LongRange" => 3,
            _ => 0,
        };
        let mut specials = Vec::new();
        let mut mirages = Vec::new();
        for (slot, i) in ["", "2", "3", "4", "5", "6", "7", "8"]
            .into_iter()
            .enumerate()
        {
            let Some((skill, li, r)) = own(&format!("specialAttack{i}SkillName")) else {
                continue;
            };
            let kind = kind_of(&r);
            if matches!(kind, Kind::Unsupported | Kind::Beam | Kind::Toggle) {
                debug!("{record}: special {skill} ({}) not supported", r.class);
                continue;
            }
            let timeout = rec.f32(&format!("specialAttack{i}Timeout")).unwrap_or(0.0);
            if pen_gated(&r) {
                mirages.push(skill.clone());
            }
            specials.push(Special {
                slot,
                skill,
                level_index: li,
                kind,
                chance: rec.f32(&format!("specialAttack{i}Chance")).unwrap_or(0.0),
                delay: rec.f32(&format!("specialAttack{i}Delay")).unwrap_or(0.0),
                band: band(&format!("specialAttack{i}Range")),
                range: crate::casting::skill_range(data, &r),
                timeout,
                delay_left: 0.0,
                timeout_left: timeout,
            });
        }
        let attack = own("attackSkillName").and_then(|(s, li, r)| {
            let kind = kind_of(&r);
            (!matches!(kind, Kind::Unsupported | Kind::Beam | Kind::Toggle)).then(|| {
                if pen_gated(&r) {
                    mirages.push(s.clone());
                }
                (s, li, kind, crate::casting::skill_range(data, &r))
            })
        });
        let f = |k: &str| rec.f32(k).unwrap_or(0.0);
        let mut buff_self = Vec::new();
        let mut toggles = Vec::new();
        for k in [
            "buffSelfSkillName",
            "buffSelf2SkillName",
            "buffSelf3SkillName",
        ] {
            let Some((s, li, r)) = own(k) else { continue };
            if gd_sim::skills::is_toggle(&r.class) {
                toggles.push((s, li));
            } else if kind_of(&r) == Kind::Buff {
                buff_self.push((s, li));
            }
        }
        let castable = |k: &str| {
            own(k).and_then(|(s, li, r)| {
                let kind = kind_of(&r);
                (!matches!(kind, Kind::Unsupported | Kind::Beam | Kind::Toggle))
                    .then_some((s, li, kind))
            })
        };
        let ctl = rec.str("controller").and_then(|c| data.db.get(c));
        let delay = rec.f32("healSkillDelay").unwrap_or(0.0);
        let mut s = Self {
            heal: own("healSkillName").map(|(s, li, _)| (s, li, delay, delay)),
            heal_pct: ctl
                .as_ref()
                .and_then(|c| c.f32("healAllyHealthPercentage"))
                .unwrap_or(70.0),
            heal_leader_pct: ctl
                .as_ref()
                .and_then(|c| c.f32("healLeaderHealthPercentage"))
                .unwrap_or(0.0),
            dying: castable("dyingSkillName"),
            initial: ["initialSkillName", "initial2SkillName"]
                .iter()
                .filter_map(|k| castable(k))
                .collect(),
            chain: rec
                .str("chainInitialSkill")
                .filter(|c| !c.is_empty())
                .and_then(|first| {
                    let next = rec.str("chainNextSkill").filter(|c| !c.is_empty())?;
                    let li = levels
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case(next))
                        .map_or(0, |(_, l)| *l as usize - 1);
                    let kind = kind_of(data.db.get(next)?);
                    (kind != Kind::Unsupported).then(|| {
                        (
                            first.to_ascii_lowercase(),
                            (next.to_owned(), li, kind),
                            rec.str("chainBehavior") == Some("UseOnSelf"),
                        )
                    })
                }),
            berserk: castable("berserkSkillName"),
            generators: levels
                .iter()
                .filter(|(n, _)| {
                    data.db
                        .get(n)
                        .is_some_and(|r| r.class == "Skill_MonsterGenerator")
                })
                .map(|(n, l)| (n.clone(), (*l).max(1) as usize - 1, 0.0))
                .collect(),
            on_death: levels
                .iter()
                .filter(|(n, _)| {
                    data.db
                        .get(n)
                        .is_some_and(|r| r.class == "Skill_OnDeathSpawnActor")
                })
                .map(|(n, l)| (n.clone(), (*l).max(1) as usize - 1))
                .collect(),
            // A `Skill_BuffRadius` without a duration is instant.
            buff_other: own("buffOtherSkillName").and_then(|(s, li, r)| {
                let inner = gd_sim::skills::profile_name(&r)
                    .map(str::to_owned)
                    .unwrap_or_else(|| s.clone());
                let secs = data
                    .db
                    .get(&inner)
                    .map_or(0.0, |b| level_f(b, "skillActiveDuration", li));
                (secs > 0.0 || r.class == "Skill_BuffRadius").then_some((s, li, inner, secs))
            }),
            buff_targeting: match ctl.as_ref().and_then(|c| c.str("BuffAllyTargeting")) {
                Some("BuffStrongest") => 1,
                Some("BuffWeakest") => 2,
                _ => 0,
            },
            buff_self_behaviour: behaviour(ctl.as_ref().and_then(|c| c.str("BuffSelfBehavior")), 2),
            buff_ally_behaviour: behaviour(ctl.as_ref().and_then(|c| c.str("BuffAllyBehavior")), 0),
            low_health: (f("lowHealthTriggerLevel"), f("lowHealthResetLevel"), false),
            attack,
            specials,
            mirages,
            bands: [
                (f("shortRangeMin"), f("shortRangeMax")),
                (f("mediumRangeMin"), f("mediumRangeMax")),
                (f("longRangeMin"), f("longRangeMax")),
            ],
            buff_self,
            toggles,
            ..Self::default()
        };
        // Initial skills that are toggles go on at startup (`UseInitialSkillIfSet`).
        for k in ["initialSkillName", "initial2SkillName"] {
            if let Some((n, li, r)) = own(k)
                && gd_sim::skills::is_toggle(&r.class)
            {
                s.toggle_on(&r, &n, li);
            }
        }
        Some(s)
    }

    /// Not turned off by `SetAvailability` (a mirage with live copies, a skill of another set).
    pub fn available(&self, skill: &str) -> bool {
        !self.unavailable.iter().any(|u| u == skill) && !self.off_set.iter().any(|u| u == skill)
    }

    pub(crate) fn ready(&self, skill: &str) -> bool {
        !self.cooldowns.iter().any(|c| c.0 == skill) && self.available(skill)
    }

    /// The active skill set changes (a timed shapeshift form starts or ends) with the form's
    /// `allowedSkillTypes` (empty coming out). `Skill::SetAvailability`: a skill whose `skillSet` is not 0 is
    /// usable only while it equals the active set. `ChooseBestSkill`: every pick (special slots, the attack
    /// skill, the default attack) also needs `skillSet == set`, or a set other than 0 and
    /// `SkillManager::IsGlobalSkillTypeAndAllowed` (no allowed types = all, else the class name among them,
    /// case-insensitive): in a form, set 0 skills are chosen only when their class is allowed
    /// ([`Self::barred`]); buffs, heal and berserk are not gated.
    pub fn set_active_set(&mut self, data: &GameData, set: i32, allowed: &[String]) {
        self.active_set = set;
        self.allowed = allowed.to_vec();
        let mut names: Vec<&String> = self.specials.iter().map(|s| &s.skill).collect();
        names.extend(self.attack.iter().map(|a| &a.0));
        names.extend(self.buff_self.iter().map(|b| &b.0));
        names.extend(self.toggles.iter().map(|b| &b.0));
        names.extend(self.berserk.iter().map(|b| &b.0));
        names.extend(self.initial.iter().map(|b| &b.0));
        names.extend(self.heal.iter().map(|b| &b.0));
        self.off_set = names
            .into_iter()
            .filter(|n| {
                data.db
                    .get(n)
                    .and_then(|r| r.i32("skillSet"))
                    .is_some_and(|k| k != 0 && k != set)
            })
            .cloned()
            .collect();
    }

    /// A timed form's stats count while it lasts (it is a `BuffSelfToggled`).
    pub fn set_form_stats(&mut self, form: Option<(String, usize)>) {
        self.form = form;
        self.dirty = true;
    }

    /// A skill is used: its `skillCooldownTime` starts; a `Skill_BuffRadius`'s is its `buffSkillName`
    /// record's.
    pub(crate) fn used(&mut self, data: &GameData, cb: &Combatant, skill: &str, li: usize) {
        let cd = data.db.get(skill).map_or(0.0, |r| {
            let profile = (r.class == "Skill_BuffRadius")
                .then(|| gd_sim::skills::profile_name(r).and_then(|b| data.db.get(b)))
                .flatten()
                .unwrap_or(r);
            crate::casting::cooldown_for(
                profile,
                li,
                &[],
                cb.profile.skill_attribute("skillCooldownReduction"),
            )
        });
        if cd > 0.0 {
            self.cooldowns.push((skill.to_owned(), cd));
        }
    }

    /// Disruption (`SkillManager::ApplyCooldownDamage`): every skill disruption reaches cools down for `secs`
    /// unless it already does for longer.
    pub(crate) fn disrupt(&mut self, db: &gd_formats::vfs::Database, secs: f32) {
        let mut names: Vec<String> = self.specials.iter().map(|s| s.skill.clone()).collect();
        names.extend(self.attack.iter().map(|a| a.0.clone()));
        names.extend(self.buff_self.iter().map(|b| b.0.clone()));
        names.extend(self.toggles.iter().map(|b| b.0.clone()));
        names.extend(self.heal.iter().map(|b| b.0.clone()));
        names.extend(self.berserk.iter().map(|b| b.0.clone()));
        names.extend(self.initial.iter().map(|b| b.0.clone()));
        names.extend(self.buff_other.iter().map(|b| b.0.clone()));
        if let Some((first, (next, ..), _)) = &self.chain {
            names.extend([first.clone(), next.clone()]);
        }
        names.sort();
        names.dedup();
        for n in names {
            if db.get(&n).is_some_and(gd_sim::skills::disruptable) {
                gd_game_core::casting::replace_cooldown(&mut self.cooldowns, &n, secs);
            }
        }
    }

    /// `ControllerMonster::Update`'s special-slot countdowns: the delays always, the timeouts once an enemy was
    /// found; in any state but Dying, casting included.
    fn count_specials(&mut self, aggro: bool, dt: f32) {
        if aggro {
            self.in_combat = true;
        }
        let in_combat = self.in_combat;
        for s in &mut self.specials {
            if s.delay_left > 0.0 {
                s.delay_left -= dt;
            }
            if in_combat && s.timeout_left > 0.0 {
                s.timeout_left -= dt;
            }
        }
    }

    /// `IsSkillInProperRange` for a gap (centre distance − radii).
    fn in_band(&self, band: usize, gap: f32) -> bool {
        if band == 0 {
            return true;
        }
        let (lo, hi) = self.bands[band - 1];
        let g = gap.max(0.5);
        g > lo && g < hi
    }

    /// `ChooseBestSkill`. In a form the set gate bars specials and the attack skill: then the plain swing when
    /// allowed, else nothing.
    fn choose(&self, data: &GameData, gap: f32) -> Pick {
        let barred = |s: &str| self.barred(data, s);
        (0..self.specials.len())
            .find(|&i| {
                let s = &self.specials[i];
                s.delay_left <= 0.0
                    && s.timeout_left <= 0.0
                    && (gd_game_core::rng::u32(..100) as f32) < s.chance
                    && self.ready(&s.skill)
                    && self.in_band(s.band, gap)
                    && !barred(&s.skill)
            })
            .map_or_else(
                || match &self.attack {
                    Some(a) if !barred(&a.0) => Pick::Attack,
                    _ if !self.barred(data, DEFAULT_ATTACK) => Pick::Plain,
                    _ => Pick::Nothing,
                },
                Pick::Special,
            )
    }

    /// `ChooseBestSkill`'s set gate fails for `skill`: its `skillSet` is not the active set and, in a form,
    /// its class is not allowed.
    pub fn barred(&self, data: &GameData, skill: &str) -> bool {
        let Some(r) = data.db.get(skill) else {
            return false;
        };
        let allowed = || {
            self.allowed.is_empty()
                || self
                    .allowed
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(&r.class))
        };
        r.i32("skillSet").unwrap_or(0) != self.active_set && (self.active_set == 0 || !allowed())
    }

    /// The controller's state has a skill (the state data's skill id, read by `Skill_Shapeshift::Update`).
    pub(crate) fn has_pick(&self) -> bool {
        self.pick
            .as_ref()
            .is_some_and(|p| !matches!(p, Pick::Nothing))
    }

    /// `ChooseBestBuffSelfSkill`: the first self buff that is ready and not running.
    fn best_self_buff(&self) -> Option<(String, usize, bool)> {
        let toggles = self
            .toggles
            .iter()
            .filter(|(s, _)| !self.toggled.iter().any(|t| &t.0 == s))
            .map(|(s, li)| (s.clone(), *li, true));
        let timed = self
            .buff_self
            .iter()
            .filter(|(s, _)| !self.buffs.list.iter().any(|b| &b.0 == s))
            .map(|(s, li)| (s.clone(), *li, false));
        toggles.chain(timed).find(|(s, ..)| self.ready(s))
    }
}

/// The monster skill pipeline's stages: `Init` creates a monster's skills, `Choose` picks what it casts.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonsterSkillStage {
    Init,
    Choose,
    /// Skills cast by dying monsters.
    Dying,
    Generators,
    /// After the aura buffs.
    Rebuild,
}

pub(crate) fn init_skills(
    mut commands: Commands,
    data: Res<GameData>,
    q: Query<(Entity, &Monster), (With<MonsterAi>, Without<MonsterSkills>)>,
) {
    for (e, m) in &q {
        let mut s = MonsterSkills::from_record(&data, &m.record, m.level).unwrap_or_default();
        s.set_active_set(&data, 0, &[]);
        if !s.specials.is_empty()
            || s.attack.is_some()
            || !s.buff_self.is_empty()
            || !s.auras.is_empty()
        {
            debug!(
                "{}: attack {:?}, specials {:?}, buffs {:?}, auras {:?}",
                m.record,
                s.attack.as_ref().map(|a| &a.0),
                s.specials
                    .iter()
                    .map(|x| (&x.skill, x.band, x.chance, x.delay))
                    .collect::<Vec<_>>(),
                s.buff_self,
                s.auras
            );
        }
        sync_damage_auras(&mut commands, &data, e, &s);
        commands.entity(e).try_insert(s);
        if let Some(rec) = data.db.get(&m.record) {
            let list = gd_sim::actor::monster_skill_levels(rec, m.level);
            let mut links = gd_game_core::weapon_pool::SkillLinks::of_list(&data.db, &list);
            if let Some(a) = crate::autocast::SkillAutocasts::of(&data.db, &list) {
                commands.entity(e).try_insert(a);
            }
            // Weapon-pool skills (`SkillManager::SelectWeaponPoolSkill`): the list's `Skill_WPAttack*` with a
            // chance weight make the pool (PORT: their weapon requirements are not checked), and a
            // `Skill_WeaponPool_Charged*` skill keeps charges.
            let mut charged = false;
            for (s, level) in &list {
                let Some(sr) = data.db.get(s) else { continue };
                let li = (*level).max(1) as usize - 1;
                charged |= gd_game_core::weapon_pool::is_pool_parent(sr);
                if sr.class.starts_with("Skill_WPAttack") {
                    let mods: Vec<(gd_formats::arz::Record, usize)> = links
                        .mods_of(s)
                        .iter()
                        .filter_map(|(m, l)| data.db.get_owned(m).map(|r| (r, *l)))
                        .collect();
                    let w = gd_sim::weapon_pool::chance_weight(
                        sr,
                        li,
                        &mods.iter().map(|(r, l)| (r, *l)).collect::<Vec<_>>(),
                    );
                    if w > 0 {
                        links.pool.push((s.clone(), li, w));
                    }
                }
            }
            if charged {
                commands
                    .entity(e)
                    .try_insert(gd_game_core::weapon_pool::PoolCharges::default());
            }
            if charged
                || !links.pool.is_empty()
                || !links.secondaries.is_empty()
                || !links.mods.is_empty()
            {
                commands.entity(e).try_insert(links);
            }
        }
    }
}

/// The damaging toggles that are on, as [`gd_game_core::casting::DamageAuras`].
pub(crate) fn sync_damage_auras(
    commands: &mut Commands,
    data: &GameData,
    e: Entity,
    ms: &MonsterSkills,
) {
    let auras: Vec<_> = ms
        .damage_auras
        .iter()
        .filter_map(|(n, li)| {
            data.db
                .get(n)
                .map(|r| gd_game_core::casting::DamageAura::new(r, n, *li, true))
        })
        .collect();
    if !auras.is_empty() {
        commands
            .entity(e)
            .try_insert(gd_game_core::casting::DamageAuras(auras));
    }
}

/// A script's `UseSkillAction` (`Character::QuestCommandUseSkill`): the monster uses one of its own skills on a
/// target in the "QuestUseSkill" state, which ignores enemy events; a target that is not a Character makes it
/// use the skill on itself (`ControllerMonsterStateQuestUseSkill::OnBegin`).
#[derive(Component)]
pub struct QuestUseSkill {
    pub skill: String,
    pub target: Option<Entity>,
    pub point: Vec3,
}

/// `Character:ToggleSkills(false)` (`Monster::EnableSkills`, gated in `ChooseBestSkill`): the monster chooses no
/// skill, it only swings.
#[derive(Component)]
pub struct SkillsDisabled;

/// Starts the commanded casts once the monster is free.
#[allow(clippy::type_complexity)]
fn quest_use_skill(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &mut Character,
            &Monster,
            &QuestUseSkill,
        ),
        (
            Without<Dead>,
            Without<Casting>,
            Without<crate::movement::Stunned>,
        ),
    >,
) {
    for (e, mut tf, mut ch, m, cmd) in &mut q {
        commands.entity(e).try_remove::<QuestUseSkill>();
        let Some(rec) = data.db.get(&m.record) else {
            continue;
        };
        // `FindSkillId`: only the monster's own skills.
        let levels = gd_sim::actor::monster_skill_levels(rec, m.level);
        let Some((name, level)) = levels
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(&cmd.skill))
        else {
            debug!("quest skill {}: {} does not have it", cmd.skill, m.record);
            continue;
        };
        let Some(sk) = data.db.get(name) else {
            continue;
        };
        let kind = kind_of(sk);
        if matches!(kind, Kind::Unsupported | Kind::Toggle) {
            debug!("quest skill {name} ({}) not supported", sk.class);
            continue;
        }
        start(
            &mut commands,
            &data,
            e,
            &mut tf,
            &mut ch,
            name,
            (*level).max(1) as usize - 1,
            kind,
            cmd.point,
            cmd.target,
        );
    }
}

/// Starts a monster cast of `skill` at `target`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn start(
    commands: &mut Commands,
    data: &GameData,
    e: Entity,
    tf: &mut Transform,
    ch: &mut Character,
    skill: &str,
    li: usize,
    kind: Kind,
    point: Vec3,
    target: Option<Entity>,
) {
    let Some(rec) = data.db.get(skill) else {
        return;
    };
    let special = rec.str("skillSpecialAnimationName").map(str::to_owned);
    if !ch.start_cast(&rec.class, special) {
        ch.start_cast("Skill_AttackSpell", None);
    }
    // `AttackAction::Execute`: the caster turns to its target as the skill starts.
    if kind != Kind::Buff {
        ch.begin_cast(tf, target.map(|t| (t, point)), point);
    }
    commands.entity(e).try_insert(Casting {
        skill: skill.to_owned(),
        level_index: li,
        kind,
        point,
        target,
        hostile: true,
        parent: None,
    });
    if gd_game_core::skill_fx::timed_aura(rec).is_none()
        && let Some(fx) = rec.str("skillActivatedAuraName")
    {
        spawn_fx(
            commands,
            data,
            fx,
            Transform::from_translation(tf.translation + Vec3::Y),
        );
    }
    debug!(
        "monster {e}: {} ({kind:?})",
        skill.rsplit('/').next().unwrap_or(skill)
    );
}

/// A toggle switches on with the cast animation, a timed buff is cast.
#[allow(clippy::too_many_arguments)]
fn use_self_buff(
    commands: &mut Commands,
    data: &GameData,
    e: Entity,
    tf: &mut Transform,
    ch: &mut Character,
    ms: &mut MonsterSkills,
    cb: &Combatant,
    (s, li, toggle): (String, usize, bool),
) {
    ms.used(data, cb, &s, li);
    if toggle {
        let Some(r) = data.db.get(&s) else { return };
        ch.start_cast(
            &r.class,
            r.str("skillSpecialAnimationName").map(str::to_owned),
        );
        ms.toggle_on(r, &s, li);
        sync_damage_auras(commands, data, e, ms);
        debug!(
            "monster {e}: toggles {}",
            s.rsplit('/').next().unwrap_or("")
        );
    } else {
        let here = tf.translation;
        start(commands, data, e, tf, ch, &s, li, Kind::Buff, here, None);
    }
}

/// The chosen skill with its `Skill::GetRange`; `None` for the plain swing.
fn skill_of<'m>(
    data: &GameData,
    ms: &'m MonsterSkills,
    pick: &'m Pick,
) -> Option<(&'m str, usize, Kind, f32)> {
    match pick {
        Pick::Attack => ms.attack.as_ref().map(|a| (a.0.as_str(), a.1, a.2, a.3)),
        Pick::Plain | Pick::Nothing => None,
        Pick::Special(i) => ms
            .specials
            .get(*i)
            .map(|x| (x.skill.as_str(), x.level_index, x.kind, x.range)),
        Pick::Skill(s, li, kind) => Some((
            s.as_str(),
            *li,
            *kind,
            data.db
                .get(s)
                .map_or(gd_game_core::combat::MELEE_RANGE, |r| {
                    crate::casting::skill_range(data, r)
                }),
        )),
    }
}

/// The centre distance to use the chosen skill: both radii + its range + the tolerance; a weapon attack or the
/// plain swing with a ranged weapon reaches as far as the weapon.
fn reach_of(
    data: &GameData,
    ms: &MonsterSkills,
    pick: &Pick,
    radii: f32,
    ranged: Option<&gd_game_core::casting::RangedWeapon>,
) -> f32 {
    match (skill_of(data, ms, pick), ranged) {
        (Some((_, _, Kind::Weapon, _)) | None, Some(w)) => radii + w.reach,
        (Some((.., range)), _) => radii + range + SKILL_USE_TOLERANCE,
        (None, None) => radii + gd_game_core::combat::MELEE_RANGE + SKILL_USE_TOLERANCE,
    }
}

/// What the approach states need of the chosen skill: the key (the state data's skill),
/// `CloseEnoughToUseSkill`'s centre distance, `Skill::GetRange` and whether the range profile is Melee
/// (`Character::GetMoveToPoint`: only those use the enemy's attack slots).
pub fn approach(
    data: &GameData,
    ms: Option<&MonsterSkills>,
    radii: f32,
    ranged: Option<&gd_game_core::casting::RangedWeapon>,
    starts: Option<&gd_game_core::casting::ChargeStarts>,
) -> crate::melee::Approach {
    let attack = Pick::Attack;
    let pick = ms.and_then(|m| m.pick.as_ref()).unwrap_or(&attack);
    let picked = ms.and_then(|m| skill_of(data, m, pick));
    let kind = picked.map(|p| p.2);
    let range = picked.map_or(gd_game_core::combat::MELEE_RANGE, |p| p.3);
    let key: std::sync::Arc<str> = picked.map_or_else(|| "".into(), |p| p.0.into());
    let rec = picked.and_then(|p| data.db.get(p.0));
    let melee_profile = !matches!(
        rec.and_then(|r| r.str("distanceProfile")),
        Some("Short" | "Moderate" | "Long" | "Maximum" | "Boss")
    );
    let mv = rec.map_or_else(Default::default, |r| {
        crate::melee::SkillMove::of_class(&r.class)
    });
    let charge_limit = rec
        .filter(|r| {
            matches!(
                r.class.as_str(),
                "Skill_AttackWeaponCharge" | "Skill_AttackWeaponBlink" | "Skill_AttackPathCharge"
            )
        })
        .and_then(|r| {
            let buffer = r.f32("maxDistanceBuffer").unwrap_or(0.0);
            (buffer != 0.0)
                .then(|| {
                    starts
                        .and_then(|s| s.0.get(&r.name).copied())
                        .map(|at| (at, buffer))
                })
                .flatten()
        });
    match ranged.filter(|_| matches!(kind, Some(Kind::Weapon) | None)) {
        Some(w) => crate::melee::Approach {
            key,
            reach: radii + w.reach,
            range: w.reach,
            melee: false,
            plain: picked.is_none() && !matches!(pick, Pick::Nothing),
            mv,
            charge_limit,
        },
        None => crate::melee::Approach {
            key,
            reach: radii + range + SKILL_USE_TOLERANCE,
            range,
            melee: melee_profile,
            plain: picked.is_none() && !matches!(pick, Pick::Nothing),
            mv,
            charge_limit,
        },
    }
}

/// The skill timers of every living monster, whatever it is doing: `SkillManager::Update` runs every frame
/// regardless of the controller state, so a stun or a cast does not pause them; [`choose_skills`] skips
/// casting / stunned / quest-moving monsters, so they tick here.
fn skill_timers(time: Res<Time>, mut q: Query<(&MonsterAi, &mut MonsterSkills), Without<Dead>>) {
    let dt = time.delta_secs();
    for (ai, mut ms) in &mut q {
        ms.cooldowns.iter_mut().for_each(|c| c.1 -= dt);
        ms.cooldowns.retain(|c| c.1 > 0.0);
        ms.count_specials(ai.aggro, dt);
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn choose_skills(
    mut commands: Commands,
    scratch: Local<RefCell<gd_sim::nav::PathScratch>>,
    time: Res<Time>,
    data: Res<GameData>,
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &mut MonsterAi,
            &mut Combatant,
            &mut Character,
            &mut MonsterSkills,
            Option<&gd_game_core::casting::RangedWeapon>,
            Has<crate::graeae::ToggleEye>,
            Has<SkillsDisabled>,
        ),
        (
            Without<Dead>,
            Without<Casting>,
            Without<crate::movement::Stunned>,
            Without<crate::combat::QuestMove>,
            Without<crate::spirit::Spirit>,
            Without<crate::spirit::Dormant>,
        ),
    >,
    foes: Query<(&Transform, &Combatant, Option<&Character>), (Without<MonsterAi>, Without<Dead>)>,
    (coll, nav): (Res<crate::collision::WorldCollision>, Res<Nav>),
    rooted: Query<(), With<crate::movement::Rooted>>,
    (mode, paths, pets, mut anim_cache): (
        Res<gd_game_core::session::GameMode>,
        Query<&crate::movement::MovePath>,
        Query<&crate::pets::Pet>,
        ResMut<gd_game_core::character::AnimCache>,
    ),
) {
    let dt = time.delta_secs();
    // The monsters a monster of a foe faction may target (not in `foes`), only when some monster's target is
    // one of them.
    let monster_targets: bevy::ecs::entity::EntityHashMap<(Transform, f32)> = if q
        .iter()
        .any(|(_, _, _, cb, ..)| cb.target.is_some_and(|t| !foes.contains(t)))
    {
        q.iter()
            .map(|(e, tf, _, cb, ..)| (e, (*tf, cb.radius)))
            .collect()
    } else {
        Default::default()
    };
    for (e, mut tf, mut ai, mut cb, mut ch, mut ms, ranged, toggling_eye, skills_off) in &mut q {
        ms.repick -= dt;
        // `ControllerMonster::Update`'s WheneverPossible poll.
        ms.poll -= dt;
        let poll = ms.poll <= 0.0;
        if poll {
            ms.poll = BUFF_POLL;
            if ms.buff_ally_behaviour == 3 {
                ms.ally_poll = true;
            }
        }
        // The Graeae's "ToggleEye" state chooses nothing, nor does "AlertBeforePursue" (its handlers are empty),
        // nor a monster down or getting up.
        if toggling_eye
            || skills_off
            || matches!(
                ch.state,
                CharState::Alert | CharState::KnockedDown | CharState::GettingUp
            )
        {
            continue;
        }
        if matches!(
            ch.state,
            CharState::Attack | CharState::Cast | CharState::Death
        ) {
            // The chosen skill is being used: the next is chosen at its end.
            ms.attacking = true;
            ms.pick = None;
            continue;
        }
        let ended = std::mem::take(&mut ms.attacking);
        // `DefaultEnemyFoundResponse`.
        let found = ai.aggro && !ms.was_aggro;
        ms.was_aggro = ai.aggro;
        if found && ms.buff_ally_behaviour == 2 {
            ms.ally_on_found = true;
        }
        if !ms.initial.is_empty() {
            let (s, li, kind) = ms.initial.remove(0);
            ms.used(&data, &cb, &s, li);
            let here = tf.translation;
            start(
                &mut commands,
                &data,
                e,
                &mut tf,
                &mut ch,
                &s,
                li,
                kind,
                here,
                Some(e),
            );
            continue;
        }
        let buff_now = match ms.buff_self_behaviour {
            0 => false,
            b => found || (ended && ai.aggro) || (b == 3 && poll),
        };
        if buff_now && let Some(b) = ms.best_self_buff() {
            use_self_buff(&mut commands, &data, e, &mut tf, &mut ch, &mut ms, &cb, b);
            continue;
        }
        if std::mem::take(&mut ms.chain_pending)
            && let Some((_, (s, li, kind), on_self)) = ms.chain.clone()
        {
            let (point, t) = match cb.target.and_then(|t| {
                foes.get(t)
                    .ok()
                    .map(|(f, ..)| f.translation)
                    .or_else(|| monster_targets.get(&t).map(|m| m.0.translation))
                    .map(|p| (p, t))
            }) {
                Some((p, t)) if !on_self => (p, Some(t)),
                _ => (tf.translation, Some(e)),
            };
            debug!("monster {e}: chain -> {s}");
            ms.used(&data, &cb, &s, li);
            start(
                &mut commands,
                &data,
                e,
                &mut tf,
                &mut ch,
                &s,
                li,
                kind,
                point,
                t,
            );
            continue;
        }
        if !ai.aggro || ai.flee.run.is_some() {
            ms.pick = None;
            continue;
        }
        if ai.reposition.is_some() {
            continue;
        }
        let (ttf, tradius, tch): (&Transform, f32, Option<&Character>) =
            match cb.target.and_then(|t| foes.get(t).ok()) {
                Some((t, c, ch)) => (t, c.radius, ch),
                None => match cb.target.and_then(|t| monster_targets.get(&t)) {
                    Some((t, r)) => (t, *r, None),
                    None => continue,
                },
            };
        let target = cb.target;
        let to = ttf.translation;
        let dist = (to - tf.translation).with_y(0.0).length();
        let radii = cb.radius + tradius;
        let gap = dist - radii;
        // `PlayLowHealthSoundIfNeeded`.
        let pct = 100.0 * cb.life / cb.profile.sheet.life.max(1.0);
        let (trig, reset, fired) = ms.low_health;
        if fired && pct >= reset {
            ms.low_health.2 = false;
        } else if !fired && pct <= trig {
            ms.low_health.2 = true;
            // `DefaultLowHealthResponse`: FleeOnLowHealth flees when `CanFlee`, else berserk.
            let fled = target.is_some_and(|t| ai.flee.trigger(crate::flee::Behavior::LowHealth, t));
            if !fled
                && let Some((s, li, kind)) = ms.berserk.clone()
                && ms.ready(&s)
            {
                debug!("monster {e}: berserk at {pct:.0}%");
                ms.pick = Some(Pick::Skill(s, li, kind));
                ms.repick = PURSUE_REPICK;
            }
        }
        // Choose after an attack, or again every 200 ms while pursuing, unless Pursue keeps the chosen skill.
        let pursuing = |ms: &MonsterSkills| {
            ms.pick.as_ref().is_some_and(|p| {
                dist > reach_of(&data, ms, p, radii, ranged)
                    && skill_of(&data, ms, p)
                        .and_then(|s| data.db.get(s.0))
                        .is_none_or(|r| crate::melee::SkillMove::of_class(&r.class).rechooses)
            })
        };
        if ms.pick.is_none() || (ms.repick <= 0.0 && pursuing(&ms)) {
            let p = ms.choose(&data, gap);
            ms.pick = Some(p);
            ms.repick = PURSUE_REPICK;
        } else if ms.repick <= 0.0 {
            ms.repick = PURSUE_REPICK;
        }
        let Some(pick) = ms.pick.clone() else {
            continue;
        };
        // Only the Attack state uses the skill, wherever the enemy now is (`AttackEnemyOrReturn` → `UseSkill`
        // has no range test), and only before it has attacked: an ended attack re-chooses and re-checks the
        // reach first (`ControllerMonsterStateAttack::OnUpdate`).
        if !ai.melee.may_attack() || cb.cooldown > 0.0 {
            continue;
        }
        if matches!(pick, Pick::Nothing) {
            ms.pick = None;
            continue;
        }
        let Some((skill, li, kind, range)) =
            skill_of(&data, &ms, &pick).map(|s| (s.0.to_owned(), s.1, s.2, s.3))
        else {
            // The plain swing: `combat.rs` swings.
            continue;
        };
        if !ms.ready(&skill) {
            ms.pick = None;
            continue;
        }
        // `ControllerAIStateT<ControllerMonster>::UseSkill`: a skill that needs line of sight is used only when
        // the target is in it, else the monster moves to a spot that sees it.
        if requires_los(&data, &skill) {
            let (head, aim) = sight_ray(&data, &ch, &tf, ttf, tch);
            if !target_in_los(&coll, head, to, aim) {
                // `ControllerMonster::CanMove` gates the search (PORT: "not rooted" and the controller class).
                let spot = if rooted.contains(e) || !ai.class.can_move() {
                    None
                } else {
                    los_spot(
                        &coll,
                        &nav,
                        &mut scratch.borrow_mut(),
                        tf.translation,
                        to,
                        range,
                        character_center(&data, &ch).y,
                    )
                };
                match spot {
                    Some(spot) => {
                        debug!("monster {e}: {skill} needs sight of its target, moving");
                        ai.reposition = Some(spot);
                    }
                    // No such spot: a monster with a leader enters "Return", else "RepositionForAttack" with
                    // zeroed state data, which idles and ends in "Return": the monster gives up the fight and
                    // finds its enemy again at a later `FindEnemyUpdate`.
                    None => {
                        debug!("monster {e}: {skill} has no spot that sees its target, returning");
                        ai.give_up = true;
                    }
                }
                continue;
            }
        }
        if let Pick::Special(i) = pick {
            let d = ms.specials[i].delay;
            ms.specials[i].delay_left = d;
        }
        ms.used(&data, &cb, &skill, li);
        ms.chain_pending = ms
            .chain
            .as_ref()
            .is_some_and(|c| c.0.eq_ignore_ascii_case(&skill));
        ms.pick = None;
        // `ResetSwingTimer` as the attack starts.
        cb.cooldown = ai.swing_pause.0
            + gd_game_core::rng::f32() * (ai.swing_pause.1 - ai.swing_pause.0).max(0.0);
        // `LeadTarget` (a ranged weapon's projectile leads for every skill).
        let mut point = to;
        if crate::lead::roll(&ai.lead, mode.0.difficulty)
            && let Some(rec) = data.db.get(&skill)
        {
            let path_target = target.and_then(|t| {
                paths
                    .get(t)
                    .ok()
                    .and_then(|p| p.points.last().copied())
                    .or_else(|| pets.get(t).ok().and_then(|p| p.path_target()))
            });
            let vel = crate::lead::velocity(to, path_target, tch);
            let led = match ranged {
                Some(rw) => crate::lead::aim_projectile(
                    &data,
                    &rw.projectile,
                    tf.translation,
                    (to, vel),
                    crate::lead::delay(&data, &mut anim_cache, &ch),
                    range.max(rw.reach),
                ),
                None => crate::lead::aim(
                    &data,
                    rec,
                    li,
                    tf.translation,
                    (to, vel),
                    crate::lead::delay(&data, &mut anim_cache, &ch),
                    range,
                ),
            };
            if let Some(p) = led {
                debug!("monster {e}: {skill} leads its target to {p}");
                point = p;
                if ranged.is_some() {
                    commands.entity(e).try_insert(crate::casting::AimPoint(p));
                }
            }
        }
        start(
            &mut commands,
            &data,
            e,
            &mut tf,
            &mut ch,
            &skill,
            li,
            kind,
            point,
            target,
        );
    }
}

/// `Skill::GetRequiresLOS` (per class): the chain classes, and `Skill_AttackProjectileBurst` / `Skill_GiveBonus`
/// for a non-player owner.
pub(crate) fn requires_los(data: &GameData, skill: &str) -> bool {
    data.db.get(skill).is_some_and(|r| {
        matches!(
            r.class.as_str(),
            "Skill_AttackChain"
                | "Skill_AttackProjectileChain"
                | "Skill_AttackRadiusLightning"
                | "Skill_AttackRadiusLightningSpawnPet"
                | "Skill_AttackProjectileBurst"
                | "Skill_GiveBonus"
        )
    })
}

/// `GetCenterOfMass`, in the entity's frame: a player's is (0, `actorHeight` × scale × ½, 0); any other actor's
/// is the centre of its mesh's bounding box, unscaled (`Actor::GetCenterOfMass`). No bounding box: the origin.
pub fn center_of_mass(
    is_player: bool,
    actor_height: f32,
    scale: f32,
    body: &gd_formats::msh::Mesh,
) -> Vec3 {
    if is_player {
        return Vec3::Y * (actor_height * scale * 0.5);
    }
    body.bounding_box.map_or(Vec3::ZERO, |(min, max)| {
        (Vec3::from(min) + Vec3::from(max)) * 0.5
    })
}

pub(crate) fn character_center(data: &GameData, ch: &Character) -> Vec3 {
    let height = data
        .db
        .get(&ch.record)
        .and_then(|r| r.f32("actorHeight"))
        .unwrap_or(0.0);
    center_of_mass(ch.is_player, height, ch.scale, &ch.body)
}

/// The ray of `Skill::IsTargetInLOS(character, id)`: from `ch`'s `HeadEffect` attach point (PORT: 1.6 m above
/// its feet without one) to the target's centre of mass in the world.
pub(crate) fn sight_ray(
    data: &GameData,
    ch: &Character,
    tf: &Transform,
    ttf: &Transform,
    tch: Option<&Character>,
) -> (Vec3, Vec3) {
    let head = gd_game_core::casting::attach_point_bind(&ch.body, "HeadEffect")
        .map_or(tf.translation + Vec3::Y * 1.6, |l| tf.transform_point(l));
    (
        head,
        ttf.translation + ttf.rotation * tch.map_or(Vec3::ZERO, |c| character_center(data, c)),
    )
}

/// `UseSkill`'s test: `IsTargetInLOS(character, id)` to the target's centre of mass (`aim`) or
/// `IsTargetInLOS(character, position)` to its coordinates (`target`) is clear.
pub(crate) fn target_in_los(
    coll: &crate::collision::WorldCollision,
    head: Vec3,
    target: Vec3,
    aim: Vec3,
) -> bool {
    coll.los_clear(head, aim) || coll.los_clear(head, target)
}

/// The search of `UseSkill`: starting at a random one of the angles 0, 36 … 180 degrees, up to 5 tries (each 36
/// degrees further, wrapping to 0 past 180): the spot at distance min(range, current distance) from the target
/// along its direction to the monster turned by that angle (x' = x·sin − z·cos, z' = x·cos + z·sin), on the
/// floor and raised by the monster's centre of mass, qualifies when the `IsTargetInLOS(position, position)` ray
/// from it to the target is clear and the monster can reach it.
pub(crate) fn los_spot(
    coll: &crate::collision::WorldCollision,
    nav: &Nav,
    scratch: &mut gd_sim::nav::PathScratch,
    me: Vec3,
    target: Vec3,
    range: f32,
    center_y: f32,
) -> Option<Vec3> {
    let to = (me - target).with_y(0.0);
    let dist = to.length();
    let dir = to.normalize_or_zero();
    let r = dist.min(range);
    let mut a = gd_game_core::rng::u32(0..=5) as f32 * 0.628_318_55;
    for _ in 0..5 {
        let (s, c) = a.sin_cos();
        let mut spot = target + Vec3::new(dir.x * s - dir.z * c, 0.0, dir.x * c + dir.z * s) * r;
        if let Some(g) = nav.0.ground([spot.x, target.y + 0.5, spot.z]) {
            spot.y = g;
        }
        if coll.los_clear(spot + Vec3::Y * center_y, target)
            && nav
                .0
                .find_path(
                    scratch,
                    me.to_array(),
                    spot.to_array(),
                    crate::nav::PATH_BUDGET,
                )
                .is_some()
        {
            return Some(spot);
        }
        a += 0.628_318_55;
        if a > std::f32::consts::PI {
            a = 0.0;
        }
    }
    None
}

/// A skill used on an ally (`UseSkillOnAlly`): it lands when the caster's cast goes off.
#[derive(Component, Clone)]
pub(crate) struct AllyCast {
    pub(crate) caster: Entity,
    pub(crate) target: Entity,
    pub(crate) skill: String,
    pub(crate) level_index: usize,
    /// A buff-other's (buff record, seconds); `None` for a heal.
    pub(crate) buff: Option<(String, f32)>,
}

/// Starts an ally cast; without an animation it lands at once (returned).
pub(crate) fn cast_on_ally(
    commands: &mut Commands,
    data: &GameData,
    caster: Entity,
    ch: &mut Character,
    cast: AllyCast,
) -> Option<AllyCast> {
    let rec = data.db.get(&cast.skill)?;
    if ch.start_cast(
        &rec.class,
        rec.str("skillSpecialAnimationName").map(str::to_owned),
    ) {
        commands.entity(caster).try_insert(cast);
        None
    } else {
        Some(cast)
    }
}

/// `TryToHealAlly` every `healSkillDelay` seconds.
#[allow(clippy::type_complexity)]
fn heal_allies(
    time: Res<Time>,
    data: Res<GameData>,
    mut commands: Commands,
    mut q: Query<
        (
            Entity,
            &Transform,
            &Combatant,
            Option<&mut MonsterSkills>,
            &MonsterAi,
            &mut Character,
        ),
        (Without<Dead>, Without<Casting>, Without<AllyCast>),
    >,
    // A leader of any kind (a monster, the player or a pet that converted it).
    leaders: Query<&Combatant, Without<Dead>>,
    mut land: MessageWriter<AllyLanded>,
) {
    let dt = time.delta_secs();
    // The snapshot of the hurt is only built when a heal is due this frame.
    let mut due = false;
    for (_, _, _, ms, _, ch) in q.iter_mut() {
        let Some(mut ms) = ms else { continue };
        let Some(h) = ms.heal.as_mut() else { continue };
        if h.2 <= 0.0 {
            continue;
        }
        if h.3 > 0.0 {
            h.3 -= dt;
        }
        // Busy: the heal follows the current action.
        if h.3 <= 0.0
            && !matches!(
                ch.state,
                CharState::Attack | CharState::Cast | CharState::Death
            )
        {
            due = true;
        }
    }
    if !due {
        return;
    }
    let pct_of = |c: &Combatant| c.life.max(0.0) / c.profile.sheet.life.max(1.0) * 100.0;
    let snapshot: Vec<(
        Entity,
        Vec3,
        f32,
        Option<Arc<gd_game_core::monsters::FactionPack>>,
    )> = q
        .iter()
        .map(|(e, t, c, _, ai, _)| (e, t.translation, pct_of(c), ai.faction.clone()))
        .collect();
    for (e, tf, _, ms, ai, mut ch) in &mut q {
        let Some(mut ms) = ms else { continue };
        let (pct, leader_pct) = (ms.heal_pct, ms.heal_leader_pct);
        let Some(h) = ms.heal.as_mut() else { continue };
        if h.2 <= 0.0
            || h.3 > 0.0
            || matches!(
                ch.state,
                CharState::Attack | CharState::Cast | CharState::Death
            )
        {
            continue;
        }
        h.3 = h.2;
        let leader = ai
            .patrol
            .leader
            .filter(|l| leaders.get(*l).is_ok_and(|c| pct_of(c) < leader_pct));
        let target = leader.or_else(|| {
            let me = tf.translation;
            let reach = if ai.view > 0.0 { ai.view } else { 15.0 };
            snapshot
                .iter()
                .filter(|(o, p, l, pack)| {
                    *o != e
                        && *l < pct
                        && p.distance(me) <= reach
                        && (ai.view_y() <= 0.0 || (p.y - me.y).abs() <= ai.view_y())
                        && match (&ai.faction, pack) {
                            (Some(a), Some(b)) => a.is_friend(b),
                            _ => true,
                        }
                })
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .map(|a| a.0)
        });
        if let Some(t) = target {
            let cast = AllyCast {
                caster: e,
                target: t,
                skill: h.0.clone(),
                level_index: h.1,
                buff: None,
            };
            if let Some(c) = cast_on_ally(&mut commands, &data, e, &mut ch, cast) {
                land.write(AllyLanded(c));
            }
        }
    }
}

/// The caster a [`buff_ally_candidate`] test is for.
pub(crate) struct BuffSphere<'a> {
    pub side: &'a crate::combat::Side,
    pub at: Vec3,
    pub view: f32,
    pub view_y: f32,
}

/// `ControllerMonster::GetCharactersInSphere` with both flags false, for `ChooseBestAllyToBuff`: in the
/// caster's `ViewDistance` sphere, within `MaxYViewDistance` in height when set, a friend of the caster and
/// [`buff_candidate_by_leader`].
pub(crate) fn buff_ally_candidate(
    sphere: &BuffSphere,
    at: Vec3,
    other: crate::combat::SideRef,
    by_leader: bool,
) -> bool {
    by_leader
        && !sphere.side.foe(other)
        && at.distance(sphere.at) <= sphere.view
        && (sphere.view_y == 0.0 || (at.y - sphere.at.y).abs() <= sphere.view_y)
}

/// `GetCharactersInSphere`'s leader test: a character without a leader (`GetLeader` 0) or a quest pet
/// (`IsQuestPet`). A pet is always led.
pub(crate) fn buff_candidate_by_leader(
    ai: Option<&MonsterAi>,
    pet: Option<&crate::pets::Pet>,
) -> bool {
    match pet {
        Some(p) => p.quest,
        None => ai.is_none_or(|ai| ai.patrol.leader.is_none()),
    }
}

/// The characters a buff-other may pick.
type AllyQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        &'static Combatant,
        crate::combat::SideData,
        Option<&'static MonsterSkills>,
        Option<&'static gd_game_core::skill_buffs::ReceivedBuffs>,
        Option<&'static gd_game_core::player::Loadout>,
        Option<&'static crate::pets::Pet>,
    ),
    (Without<Dead>, Without<gd_game_core::world::Breakable>),
>;

/// Buff-other skills on allies (`BuffAllyBehavior`): the player and pets are candidates too, busy monsters as
/// well.
fn buff_allies(
    data: Res<GameData>,
    mut commands: Commands,
    mut set: ParamSet<(
        AllyQuery,
        Query<
            (
                Entity,
                &Transform,
                &MonsterAi,
                Has<crate::combat::Allied>,
                &Combatant,
                &mut Character,
                &mut MonsterSkills,
            ),
            (Without<Dead>, Without<Casting>, Without<AllyCast>),
        >,
    )>,
    mut land: MessageWriter<AllyLanded>,
) {
    struct Ally {
        e: Entity,
        at: Vec3,
        life: f32,
        frac: f32,
        on: Vec<String>,
        side: crate::combat::Side,
        by_leader: bool,
    }
    // The snapshot of every character's buffs is only built when a buff-other use is due this frame.
    let due = set.p1().iter().any(|(.., ch, ms)| {
        (ms.ally_poll || ms.ally_on_found)
            && !matches!(
                ch.state,
                CharState::Attack | CharState::Cast | CharState::Death
            )
    });
    if !due {
        return;
    }
    let snapshot: Vec<Ally> = set
        .p0()
        .iter()
        .map(|(e, t, c, sd, ms, received, loadout, pet)| {
            // `Character::IsSkillBuffActive`.
            let mut on: Vec<String> = ms
                .map(|m| m.buffs.list.iter().map(|b| b.0.clone()).collect())
                .unwrap_or_default();
            on.extend(
                loadout
                    .iter()
                    .flat_map(|l| l.buffs.list.iter().map(|b| b.0.clone())),
            );
            on.extend(
                pet.iter()
                    .flat_map(|p| p.buffs().iter().map(|b| b.0.clone())),
            );
            on.extend(
                received
                    .iter()
                    .flat_map(|a| a.list.iter().map(|b| b.buff.clone())),
            );
            Ally {
                e,
                at: t.translation,
                life: c.life,
                frac: c.life / c.profile.sheet.life.max(1.0),
                on,
                side: sd.side(),
                by_leader: buff_candidate_by_leader(sd.ai, pet),
            }
        })
        .collect();
    for (e, tf, ai, allied, cb, mut ch, mut ms) in &mut set.p1() {
        if !(ms.ally_poll || ms.ally_on_found)
            || matches!(
                ch.state,
                CharState::Attack | CharState::Cast | CharState::Death
            )
        {
            continue;
        }
        ms.ally_poll = false;
        ms.ally_on_found = false;
        let Some((skill, li, inner, secs)) = ms.buff_other.clone().filter(|b| ms.ready(&b.0))
        else {
            continue;
        };
        // `FactionManager::IsFriend`: two monsters by their packs (one without a pack is everyone's friend);
        // PORT: the player's side by the `Allied` mark.
        let side = crate::combat::Side::Monster {
            pack: ai.faction.clone(),
            allied,
        };
        let sphere = BuffSphere {
            side: &side,
            at: tf.translation,
            view: if ai.view > 0.0 { ai.view } else { 15.0 },
            view_y: ai.view_y,
        };
        // `ChooseBestAllyToBuff`.
        let cands = snapshot.iter().filter(|a| {
            a.e != e
                && buff_ally_candidate(&sphere, a.at, a.side.as_ref(), a.by_leader)
                && !a.on.iter().any(|b| b.eq_ignore_ascii_case(&inner))
        });
        let pick = match ms.buff_targeting {
            0 => cands.min_by(|a, b| {
                a.at.distance(tf.translation)
                    .total_cmp(&b.at.distance(tf.translation))
            }),
            2 => cands.min_by(|a, b| a.frac.total_cmp(&b.frac)),
            _ => cands.max_by(|a, b| a.life.total_cmp(&b.life)),
        };
        let Some(t) = pick.map(|a| a.e) else { continue };
        ms.used(&data, cb, &skill, li);
        debug!(
            "monster buff {} on {t}",
            inner.rsplit('/').next().unwrap_or("")
        );
        let cast = AllyCast {
            caster: e,
            target: t,
            skill,
            level_index: li,
            buff: Some((inner, secs)),
        };
        if let Some(c) = cast_on_ally(&mut commands, &data, e, &mut ch, cast) {
            land.write(AllyLanded(c));
        }
    }
}

/// An ally cast that lands this frame.
#[derive(Message)]
pub(crate) struct AllyLanded(pub(crate) AllyCast);

/// Ally casts land, monsters' and pets' alike. A buff-other lands on its target for its duration; a
/// `Skill_BuffRadius` goes off on the caster instead ([`crate::auras::Activated`]), reaching the target with its
/// other allies.
#[allow(clippy::too_many_arguments)]
fn ally_casts_land(
    mut commands: Commands,
    data: Res<GameData>,
    mut now: MessageReader<AllyLanded>,
    mut targets: Query<
        (
            &Transform,
            &mut Combatant,
            Option<&mut MonsterSkills>,
            Option<&mut crate::pets::Pet>,
            Option<&mut gd_game_core::player::Loadout>,
        ),
        Without<Dead>,
    >,
    mut activated: MessageWriter<crate::auras::Activated>,
) {
    // The same landing twice this frame (a hit and the `End` together) lands once.
    let mut landed: Vec<AllyCast> = Vec::new();
    for l in now.read() {
        if !landed
            .iter()
            .any(|c| c.target == l.0.target && c.skill == l.0.skill)
        {
            landed.push(l.0.clone());
        }
    }
    for c in landed {
        let radius = data
            .db
            .get(&c.skill)
            .is_some_and(|r| r.class == "Skill_BuffRadius");
        if radius {
            // `Skill_BuffRadius::ActivateNow` on the caster: the first pass now, and a lasting one runs as the
            // caster's timed buff.
            activated.write(crate::auras::Activated {
                caster: c.caster,
                skill: c.skill.clone(),
                level_index: c.level_index,
                mods: Vec::new(),
            });
            let secs = c.buff.as_ref().map_or(0.0, |b| b.1);
            if secs > 0.0
                && let Ok((_, _, Some(mut ms), _, _)) = targets.get_mut(c.caster)
            {
                debug!(
                    "ally buff {} on {}",
                    c.skill.rsplit('/').next().unwrap_or(""),
                    c.caster
                );
                ms.add_buff(&c.skill, c.level_index, secs);
            }
            continue;
        }
        let on = c.target;
        let Ok((ttf, _, ms, pet, loadout)) = targets.get_mut(on) else {
            continue;
        };
        match c.buff {
            Some((name, secs)) => {
                debug!(
                    "ally buff {} on {on}",
                    name.rsplit('/').next().unwrap_or("")
                );
                // `Skill_BuffOther::TargetResult`: the buff, then the profile's bonus.
                if let Some(brec) = data.db.get(&name) {
                    crate::casting::Bonus::of_record(&data.db, brec, c.level_index, &[])
                        .queue(&mut commands, on);
                }
                if let Some(mut ms) = ms {
                    ms.add_buff(&name, c.level_index, secs);
                } else if let Some(mut pet) = pet {
                    pet.add_buff(&name, c.level_index, secs);
                } else if let Some(mut loadout) = loadout {
                    loadout.buffs.add(&name, c.level_index, secs);
                    commands
                        .entity(on)
                        .try_insert(gd_game_core::player::ProfileDirty);
                }
            }
            None => {
                let Some(rec) = data.db.get(&c.skill) else {
                    continue;
                };
                // `Skill_GiveBonus` → `CharacterBio::TakeBonus`.
                crate::casting::Bonus::of_record(&data.db, rec, c.level_index, &[])
                    .queue(&mut commands, on);
                debug!(
                    "monster heal {} on {}",
                    c.skill.rsplit('/').next().unwrap_or(""),
                    c.target
                );
                if let Some(fx) = rec.str("skillBonusEffectName") {
                    spawn_fx(
                        &mut commands,
                        &data,
                        fx,
                        Transform::from_translation(ttf.translation + Vec3::Y),
                    );
                }
            }
        }
    }
}

/// The ally casts whose clip reached its hit or `End` this frame land. Runs before [`choose_skills`]: a cast
/// started later this frame would make the previous clip's callbacks look stale.
fn ally_cast_callbacks(
    mut commands: Commands,
    mut events: MessageReader<AnimCallback>,
    casters: Query<(&AllyCast, &gd_game_core::character::Character)>,
    mut land: MessageWriter<AllyLanded>,
) {
    for ev in events.read() {
        if !matches!(ev.name.as_str(), "RightHandHit" | "LeftHandHit" | "End") {
            continue;
        }
        let Ok((c, ch)) = casters.get(ev.entity) else {
            continue;
        };
        if !ch.callbacks_after_cast() {
            continue;
        }
        // Every consumed callback takes the cast off.
        commands.entity(ev.entity).try_remove::<AllyCast>();
        land.write(AllyLanded(c.clone()));
    }
}

/// `StartDyingSkill`: the dying skill goes off where the monster died.
pub fn dying_skills(
    mut procs: MessageWriter<gd_game_core::casting::ProcCast>,
    q: Query<(Entity, &Transform, &MonsterSkills), Added<Dead>>,
) {
    for (e, tf, ms) in &q {
        if let Some((skill, li, kind)) = &ms.dying {
            debug!("monster {e} dying skill {skill}");
            procs.write(gd_game_core::casting::ProcCast {
                caster: e,
                cast: Casting {
                    skill: skill.clone(),
                    level_index: *li,
                    kind: *kind,
                    point: tf.translation,
                    target: None,
                    hostile: true,
                    parent: None,
                },
            });
        }
    }
}

/// `Skill_MonsterGenerator::Update`: while the owner is alive and no spawn is pending, a timer (0 at first)
/// counts down; at 0 it tops the owner's live spawns of this skill up by `petBurstSpawn`, at most to `petLimit`
/// (both per level), then re-arms with `petPeriod`. Not gated on combat.
pub fn generators(
    time: Res<Time>,
    mut q: Query<(Entity, &Transform, &mut MonsterSkills), Without<Dead>>,
    data: Res<GameData>,
    mut pets: MessageWriter<SpawnPet>,
) {
    let dt = time.delta_secs();
    for (e, tf, mut ms) in &mut q {
        for (skill, li, timer) in ms.generators.iter_mut() {
            *timer -= dt;
            if *timer > 0.0 {
                continue;
            }
            *timer = data
                .db
                .get(skill)
                .map_or(10.0, |r| level_f(r, "petPeriod", *li));
            pets.write(SpawnPet {
                owner: e,
                skill: skill.clone(),
                level_index: *li,
                point: tf.translation,
                target: None,
            });
        }
    }
}

/// Buffs and auras change a monster's profile: rebuilt from its record with them added, keeping the life
/// fraction.
pub(crate) fn rebuild_profiles(
    data: Res<GameData>,
    rules: Res<CombatRules>,
    mut q: Query<(
        &Monster,
        &mut MonsterSkills,
        &mut Combatant,
        Option<&crate::monsters::Worn>,
        Option<&mut gd_game_core::skill_buffs::ReceivedBuffs>,
        Option<&crate::challenge_areas::AreaRules>,
    )>,
) {
    // Each monster's rebuild reads only shared records / rules and writes its own components.
    let base = &rules.0;
    q.par_iter_mut()
        .for_each(|(m, mut ms, mut cb, worn, mut auras, area)| {
            let rules = area.map_or(base, |a| &a.0);
            // Read before taking: taking through `DerefMut` would flag `ReceivedBuffs` / `MonsterSkills` changed
            // for every monster every frame.
            let received = auras.as_ref().is_some_and(|a| a.changed)
                && auras.as_mut().is_some_and(|a| a.take_changed());
            if !ms.dirty && !received {
                return;
            }
            ms.dirty = false;
            let mut extra = Vec::new();
            for (s, li) in ms
                .auras
                .iter()
                .map(|(s, li)| (s.clone(), *li))
                .chain(ms.buffs.list.iter().map(|b| (b.0.clone(), b.1)))
                .chain(ms.form.clone())
            {
                let Some(r) = data.db.get_owned(&s) else {
                    continue;
                };
                if r.class == "Skill_BuffRadius" {
                    continue;
                }
                // Timed buffs keep their stats in the skill or its buff record.
                let r = gd_sim::skills::profile_name(&r)
                    .and_then(|b| data.db.get_owned(b))
                    .unwrap_or(r);
                extra.push((r, li));
            }
            if let Some(a) = &auras {
                extra.extend(a.stat_records(&data.db));
            }
            let worn = worn.map_or(&[][..], |w| &w.resolved[..]);
            let Some(p) = gd_sim::actor::monster_full(
                &data.db,
                rules,
                &m.record,
                m.level,
                gd_sim::actor::Side::Monster,
                &extra,
                worn,
            ) else {
                return;
            };
            let frac = if cb.profile.sheet.life > 0.0 {
                cb.life / cb.profile.sheet.life
            } else {
                1.0
            };
            cb.life = p.sheet.life * frac;
            cb.profile = Arc::new(p);
        });
}

#[derive(Component)]
pub struct Minion {
    pub owner: Entity,
    pub summon: String,
    spawned_at: f32,
    /// Seconds left to live (`Monster::SetLifetime`, `spawnObjectsTimeToLive`); None = forever.
    pub time_to_live: Option<f32>,
    /// `overkillOnDismiss` (`Character::GetDismissBehavior`): its `KillMe` on expiry and on the pet limit gibs.
    pub overkill_on_dismiss: bool,
}

/// Monster summons: each one's record from `gd_sim::skills::spawn_object` at the owner's level, on the owner's
/// side and sharing its anger; `petLimit` per owner and skill, oldest first.
#[allow(clippy::too_many_arguments)]
fn spawn_minions(
    mut commands: Commands,
    mut events: MessageReader<SpawnPet>,
    rules: Res<CombatRules>,
    mut owners: Query<(
        &mut Transform,
        &Monster,
        &MonsterAi,
        Option<&mut crate::movement::MovePath>,
        Option<&mut Character>,
    )>,
    minions: Query<(Entity, &Minion), Without<Dead>>,
    others: Query<(&Transform, Option<&crate::pets::Pet>), (Without<Monster>, Without<Dead>)>,
    combatants: Query<&Combatant>,
    nav: Res<Nav>,
    time: Res<Time>,
    mut assets: crate::creature::Spawner,
) {
    // This run's spawns and dismissals: the queries see neither until the commands apply, so a second event for
    // the same skill this frame counts them here against `petLimit`.
    let mut fresh: Vec<(Entity, Entity, String, f32)> = Vec::new();
    let mut culled: Vec<Entity> = Vec::new();
    for ev in events.read() {
        let Ok((otf, owner, oai, ..)) = owners.get(ev.owner) else {
            continue;
        };
        let (otf, level, slot) = (*otf, owner.level, owner.slot.0);
        let owner_record = owner.record.clone();
        let owner_radius = combatants.get(ev.owner).map_or(0.5, |c| c.radius);
        let owner_aggro = oai.aggro;
        // Its own handle: the record stays borrowed while `assets` spawns.
        let db = assets.data.db.clone();
        let Some(rec) = db.get(&ev.skill) else {
            continue;
        };
        // The level's `spawnObjects`: the pet object the placement measures; each summon picks its own record
        // below.
        let Some(base_name) = rec
            .strs_iter("spawnObjects")
            .nth(ev.level_index)
            .or_else(|| rec.strs_iter("spawnObjects").last())
            .map(|s| s.to_string())
        else {
            continue;
        };
        // Casts spawn `petBurstSpawn` up to the limit but at least one (`Skill_SpawnPet::ActivateNow`), then the
        // oldest beyond `petLimit` die (`Skill_TargetedSpawnPet::SpawnPet`); generators only top up.
        let limit = level_f(rec, "petLimit", ev.level_index).max(0.0) as usize;
        let burst = level_f(rec, "petBurstSpawn", ev.level_index).max(0.0) as usize;
        let generator = rec.class == "Skill_MonsterGenerator";
        let ttl = crate::pets::spawn_time_to_live(&assets.data.db, rec, ev.level_index, &[]);
        let mut mine: Vec<(Entity, f32)> = minions
            .iter()
            .filter(|(e, m)| m.owner == ev.owner && m.summon == ev.skill && !culled.contains(e))
            .map(|(e, m)| (e, m.spawned_at))
            .chain(
                fresh
                    .iter()
                    .filter(|f| f.0 == ev.owner && f.2 == ev.skill && !culled.contains(&f.1))
                    .map(|f| (f.1, f.3)),
            )
            .collect();
        mine.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mirage = rec.class == MIRAGE;
        let db = &assets.data.db;
        let pet = db.get(&base_name);
        // The spots: a `Skill_TargetedSpawnPet` packs all `petBurstSpawn` around its target / point, the hit
        // spawners around what their attack hit, a generator tops up around the caster clear of its footprint,
        // the `Skill_SpawnPet` classes fill the set around the caster (its own spot first).
        let target = ev
            .target
            .and_then(|t| match owners.get(t) {
                Ok((tf, m, ..)) => Some((tf.translation, db.get(&m.record))),
                Err(_) => others
                    .get(t)
                    .ok()
                    .map(|(tf, p)| (tf.translation, p.and_then(|p| db.get(&p.record)))),
            })
            .map(|(p, r)| (p, gd_game_core::pets::placement::extents(r)));
        let mut spots: Vec<Vec3> = if rec.class == "Skill_TargetedSpawnPet" {
            crate::pets::placement::targeted_spots(
                db,
                &nav,
                rec,
                level_f(rec, "petBurstSpawn", ev.level_index),
                pet,
                &crate::pets::placement::Aim {
                    caster: otf.translation,
                    point: ev.point,
                    target,
                },
            )
        } else if crate::pets::placement::spawns_on_hit(&rec.class) {
            crate::pets::placement::hit_spots(
                db,
                &nav,
                rec,
                level_f(rec, "petBurstSpawn", ev.level_index),
                pet,
                ev.point,
                target,
            )
        } else if generator {
            let n = burst.min(limit.saturating_sub(mine.len())) as u32;
            crate::pets::placement::generator_spots(
                &nav,
                otf.translation,
                owner_radius,
                gd_game_core::pets::placement::pet_extents(pet),
                n,
            )
        } else {
            let n = gd_game_core::pets::placement::spawn_pet_count(
                &rec.class,
                limit as u32,
                mine.len() as u32,
                burst as u32,
            );
            let caster = gd_game_core::pets::placement::extents(db.get(&owner_record));
            let radii = crate::pets::placement::spawn_pet_radii(&rec.class, caster, pet);
            let mut set = if n == 0 {
                Vec::new()
            } else {
                crate::pets::placement::spawn_pet_set(&nav, otf.translation, radii, n)
            };
            // Base `GetNewCasterCoords`: the first point (the caster's) is dropped.
            if !mirage && !set.is_empty() {
                set.remove(0);
            }
            set
        };
        // `Skill_AktaiosMirage::GetNewCasterCoords`: the caster takes a random spot of the set (its own
        // included), the copies the rest; the draw past the end keeps it in place.
        if mirage {
            let m = spots.len() as i32;
            let i = gd_game_core::rng::i32(0..=m);
            if m > 1 && i < m {
                let to = spots.remove(i as usize);
                if let Ok((mut tf, _, _, path, ch)) = owners.get_mut(ev.owner) {
                    tf.translation = to;
                    if let Some(mut p) = path {
                        p.points.clear();
                    }
                    // `SetCasterCoords`: animation type 19 on the caster.
                    if let Some(mut ch) = ch {
                        ch.play_scripted(gd_sim::anim::AnimType::Spawn);
                    }
                }
                debug!("mirage {}: the caster goes to spot {i} of {m}", ev.owner);
            }
        }
        for at in spots {
            // `SpawnPet` per point (a monster's skills have no modifiers, so no `petChanges`).
            let rec_name =
                gd_sim::skills::spawn_object(&assets.data.db, rec, ev.level_index, &[], &[], |n| {
                    gd_game_core::rng::u32(0..n)
                })
                .unwrap_or_else(|| base_name.clone());
            // `KillExtraPets`: the oldest beyond the limit die through `KillMe(overkillOnDismiss)`.
            while !generator && limit > 0 && mine.len() >= limit {
                let (old, _) = mine.remove(0);
                culled.push(old);
                let overkill = minions.get(old).is_ok_and(|(_, m)| m.overkill_on_dismiss);
                dismiss(&mut commands, old, overkill);
            }
            let Some(profile) = gd_sim::actor::monster(
                &assets.data.db,
                &rules.0,
                &rec_name,
                level,
                gd_sim::actor::Side::Monster,
            ) else {
                break;
            };
            // A generator's pet faces a random yaw.
            let rot = if generator {
                Quat::from_rotation_y(gd_game_core::rng::f32() * std::f32::consts::TAU)
            } else {
                otf.rotation
            };
            let tf = Transform::from_translation(at).with_rotation(rot);
            let Some(e) = crate::creature::spawn(
                &mut commands,
                &mut assets,
                tf,
                &rec_name,
                gd_game_core::rng::u32(..),
            ) else {
                break;
            };
            let radius = gd_game_core::combat::get_radius(assets.data.db.get(&rec_name), 0.5);
            let mut ai = MonsterAi::from_record(&assets.data, &rec_name, tf.translation);
            // `Monster::TransferAnger`.
            ai.aggro = owner_aggro;
            // `Monster::JoinMe`: the owner leads.
            ai.patrol.join(ev.owner);
            // `Skill_SpawnPet::SpawnPet` / `Skill_MonsterGenerator::SkillSpawnObject`: every spawn plays its
            // Spawn animation.
            ai.enable_spawn_animation();
            commands.entity(e).insert((
                // Culled with the owner's level; no proxy slot to mark dead.
                Monster {
                    record: rec_name.clone(),
                    level,
                    slot: (slot, u32::MAX, usize::MAX),
                },
                Combatant::new(Arc::new(profile), radius),
                ai,
                Minion {
                    owner: ev.owner,
                    summon: ev.skill.clone(),
                    spawned_at: time.elapsed_secs(),
                    time_to_live: ttl,
                    overkill_on_dismiss: assets
                        .data
                        .db
                        .get(&rec_name)
                        .is_some_and(|r| r.bool("overkillOnDismiss")),
                },
            ));
            mine.push((e, time.elapsed_secs()));
            fresh.push((ev.owner, e, ev.skill.clone(), time.elapsed_secs()));
            debug!("monster {} summons {rec_name}", ev.owner);
        }
    }
}

/// Summon skills a monster cannot cast again while a summon of theirs lives: `Skill_AktaiosMirage::SetAvailability`,
/// and `Skill_TargetedSpawnPet::Update` for a `Monster` owner unless the record sets `ignorePetLimit` (which does
/// not touch the `petLimit` cull).
fn pen_gated(r: &gd_formats::arz::Record) -> bool {
    r.class == MIRAGE || (r.class == "Skill_TargetedSpawnPet" && !r.bool("ignorePetLimit"))
}

/// [`pen_gated`]: the skill is off while any summon of it lives.
fn mirage_availability(
    mut q: Query<(Entity, &mut MonsterSkills)>,
    minions: Query<&Minion, Without<Dead>>,
) {
    for (e, mut ms) in &mut q {
        if ms.mirages.is_empty() {
            if !ms.unavailable.is_empty() {
                ms.unavailable.clear();
            }
            continue;
        }
        let off: Vec<String> = ms
            .mirages
            .iter()
            .filter(|s| minions.iter().any(|m| m.owner == e && &m.summon == *s))
            .cloned()
            .collect();
        if off != ms.unavailable {
            ms.unavailable = off;
        }
    }
}

/// Summons with a time to live die when it runs out (`ControllerMonster::Update` → `ControllerCombat::KillMe`
/// with `overkillOnDismiss`); the dying skill still goes off.
fn expire_minions(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Minion), Without<Dead>>,
) {
    let dt = time.delta_secs();
    for (e, mut m) in &mut q {
        let overkill = m.overkill_on_dismiss;
        let Some(left) = m.time_to_live.as_mut() else {
            continue;
        };
        *left -= dt;
        if *left <= 0.0 {
            dismiss(&mut commands, e, overkill);
        }
    }
}

/// A summon's `ControllerCombat::KillMe(overkillOnDismiss)`: an ordinary death, or an overkill (the gib death)
/// when the flag is set (inserted first: the death systems see it).
fn dismiss(commands: &mut Commands, e: Entity, overkill: bool) {
    if overkill {
        commands.entity(e).try_insert(crate::combat::Overkill);
    }
    crate::combat::kill_me(commands, e);
}
