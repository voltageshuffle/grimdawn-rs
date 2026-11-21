use super::*;

/// The class of the "Pet Attack" skill (module docs).
pub const PET_ATTACK_CLASS: &str = "Skill_PetAttack";

/// An order to the owner's pets (module docs): `pet: None` is the Pet
/// Attack skill (every pet; a `target` is attacked, else they move to
/// `point`), `Some` one selected pet (sub_1402056D0 / `HandlePetAction`: a foe
/// `target` is attacked, else it moves to `point`).
#[derive(Message, Clone, Copy, Debug)]
pub struct PetCommand {
    pub owner: Entity,
    pub pet: Option<Entity>,
    pub target: Option<Entity>,
    pub point: Vec3,
}

/// A pet's controller (`Monster::ControllerType`, module docs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stance {
    #[default]
    Normal,
    Aggressive,
    Defensive,
}

impl Stance {
    /// The pet record field naming the controller (`Monster::Load`).
    pub fn field(self) -> &'static str {
        match self {
            Stance::Normal => "controller",
            Stance::Aggressive => "controllerAggressive",
            Stance::Defensive => "controllerDefensive",
        }
    }

    /// The portrait buttons' cycle (GrimDawn.exe sub_140254B40).
    pub fn next(self) -> Self {
        match self {
            Stance::Normal => Stance::Aggressive,
            Stance::Aggressive => Stance::Defensive,
            Stance::Defensive => Stance::Normal,
        }
    }
}

/// Sets a pet's stance (the portrait menu): the pet switches controller and
/// its summon skill remembers it for later summons.
#[derive(Message, Clone, Copy, Debug)]
pub struct SetPetStance {
    pub pet: Entity,
    pub stance: Stance,
}

/// `Skill::StopSkill` of a summon skill (`Skill_SpawnPet::StopSkill`
/// @0x180509a00, module docs): all the owner's live pets of `skill` die.
#[derive(Message, Clone, Debug)]
pub struct StopSummonSkill {
    pub owner: Entity,
    pub skill: String,
}

/// A quest pet released by `UnJoinLeader(false)` (module docs): alive and
/// friendly, no longer a [`Pet`] (no leader, banner, anger or pen entry).
/// PORT: it stands where it was (no ambient AI for a leaderless friendly).
#[derive(Component)]
pub struct ReleasedQuestPet;

/// "Disband Pet" from a portrait's menu: the owner's pet dies (module docs).
#[derive(Message, Clone, Copy, Debug)]
pub struct DisbandPet {
    pub pet: Entity,
}

/// `ControllerPlayer::ReleasePet(pet, suppressDyingSkill)` -> `ReleasePetConfigCmd`
/// (module docs): the pet dies, an overkill when its remaining life is at
/// least max life x gib threshold; `suppress_dying_skill` (the flag, set by
/// the PetPen release paths) keeps its dying skill from going off. The
/// portrait's [`DisbandPet`] is this with the flag false.
#[derive(Message, Clone, Copy, Debug)]
pub struct ReleasePet {
    pub pet: Entity,
    pub suppress_dying_skill: bool,
}

/// `Character::SuppressDyingSkill` (+0x2834): the pet's dying skill is not
/// cast.
#[derive(Component)]
pub struct SuppressDyingSkill;

/// The owner's pets with a portrait, in portrait order (module docs:
/// `showStatusWidgetWhenPet`, summon order; dead pets have none).
pub fn portrait_pets<'a>(
    owner: Entity,
    pets: impl Iterator<Item = (Entity, &'a Pet)>,
) -> Vec<Entity> {
    let mut v: Vec<(Entity, f32)> = pets
        .filter(|(_, p)| p.owner == owner && p.portrait)
        .map(|(e, p)| (e, p.spawned_at))
        .collect();
    v.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    v.into_iter().map(|(e, _)| e).collect()
}

/// On a pet owner: the stance per summon skill (`Player::SetPetControllerType`;
/// session only, module docs).
#[derive(Component, Default, Debug, Clone)]
pub struct PetStances(pub Vec<(String, Stance)>);

impl PetStances {
    pub fn get(&self, summon: &str) -> Stance {
        self.0
            .iter()
            .find(|(s, _)| s.eq_ignore_ascii_case(summon))
            .map_or(Stance::Normal, |(_, st)| *st)
    }

    /// The stance a new portrait shows (GrimDawn.exe sub_140255BE0:
    /// `Player::GetPetControllerType`, which answers Aggressive for a skill
    /// with no stored type although the pet runs on Normal; module docs).
    pub fn portrait(stances: Option<&Self>, summon: &str) -> Stance {
        stances
            .and_then(|s| s.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(summon)))
            .map_or(Stance::Aggressive, |(_, st)| *st)
    }

    pub(super) fn set(&mut self, summon: &str, stance: Stance) {
        match self
            .0
            .iter_mut()
            .find(|(s, _)| s.eq_ignore_ascii_case(summon))
        {
            Some(e) => e.1 = stance,
            None => self.0.push((summon.to_owned(), stance)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PetAttack {
    pub skill: String,
    pub level_index: usize,
    pub kind: Kind,
    /// `Skill::GetRange` ([`crate::casting::skill_range`]); 0 for a weapon primary (the
    /// melee swing's reach).
    pub range: f32,
    pub chance: f32,
    /// `specialAttack*Range`'s band as `(min, max)` of the record's
    /// `short/medium/longRangeMin/Max` (`IsSkillInProperRange`: the gap
    /// between the bodies, at least 0.5, strictly inside); None: any distance.
    pub band: Option<(f32, f32)>,
    pub delay: f32,
    /// Seconds to its next use: `specialAttack*Delay`, re-armed when used
    /// (0 at the summon).
    pub timer: f32,
    /// `specialAttack*Timeout` left: counted down only once the pet has found
    /// an enemy ([`Pet::found_enemy`]); the special waits until it is out
    /// (module docs).
    pub timeout: f32,
}

#[derive(Component)]
pub struct Pet {
    pub owner: Entity,
    pub summon: String,
    /// Summoned by a `Skill_SpawnMiniPet` (`PetPen` entry +36): counts
    /// towards `miniPetLimit` (module docs).
    pub mini: bool,
    /// Summoned by a `Skill_SpawnQuestPet`: it is in the owner's quest pen
    /// (`Character` +8296, apart from the normal pen), so the pet limits of
    /// other summons do not count it and its stop releases it alive
    /// ([`release_quest_pet`](super::commands::release_quest_pet)).
    pub quest: bool,
    pub spawned_at: f32,
    /// Seconds it has left to live (`ControllerMonster` +0x624 set by
    /// `Monster::SetLifetime`; module docs, [`spawn_time_to_live`]); None
    /// lives until killed, replaced or disbanded. The one source of a pet's
    /// remaining time.
    pub time_to_live: Option<f32>,
    /// It has found an enemy once (`ControllerMonster::FindEnemy`'s flag):
    /// the specials' timeouts count from then on.
    pub found_enemy: bool,
    pub primary: Option<PetAttack>,
    pub specials: Vec<PetAttack>,
    /// The record's `angerMultiplier` (`Character::Load`, default 1): scales
    /// the anger monsters gain towards it (`AngerManager::AddAnger`).
    pub anger_multiplier: f32,
    /// The record's `causesAnger` (`Character::Load` +8376, default 1; `Character::CausesAnger`, the
    /// same function for a `Pet` as for a `Monster`): a converted pet near the player holds the controller's combat
    /// flag (grimdawn `controller_target.rs`).
    pub causes_anger: bool,
    /// The record's `targetable` (`Character::Load` +8026, default 1; a `Pet`'s `IsTargetable` is
    /// `Monster::IsTargetable` @0x18037ec40): whether the controller targeting takes a converted pet.
    pub targetable: bool,
    /// The record's `showStatusWidgetWhenPet`: it gets a HUD portrait
    /// ([`portrait_pets`]).
    pub portrait: bool,
    /// The record's class is `PetPlayerScaling`: its attacks are collected
    /// with the owner's attributes and racial bonuses (module docs).
    pub player_scaling: bool,
    /// The record's `buffSelfSkillName`, `buffSelf2…`, `buffSelf3…` that it
    /// has (skill, level index, a toggle), in that order.
    pub(super) buff_self: Vec<(String, usize, bool)>,
    /// Toggled skills its AI switched on (skill, level index).
    pub(super) toggled: Vec<(String, usize)>,
    /// The timed self buffs (the shared [`crate::skill_buffs::TimedBuffs`]
    /// store, ticked by `casting::expire_buffs`): its own (a self buff, a
    /// `Skill_BuffRadius`, under the skill's name) and others'
    /// `Skill_BuffOther` (under the buff record's).
    pub(super) buffs: crate::skill_buffs::TimedBuffs,
    /// `buffOtherSkillName`: (skill, level index, buff record, seconds), as
    /// `MonsterSkills::buff_other`.
    pub(super) buff_other: Option<(String, usize, String, f32)>,
    /// A buff-other use is due (the 4 s WheneverPossible poll, or an enemy
    /// was found for WhenEnemyIsSeen).
    pub(super) ally_due: bool,
    /// `skillCooldownTime` left of its used self / ally buffs and initial
    /// skills (the `Skill` ready check, as `MonsterSkills::cooldowns`).
    pub(super) cooldowns: Vec<(String, f32)>,
    /// Initial skills that are not toggles, still to cast on itself
    /// (`UseInitialSkillIfSet`).
    pub(super) initial_casts: Vec<(String, usize)>,
    /// The record's `overkillOnDismiss` (`Character::GetDismissBehavior`):
    /// its time to live running out kills it as an overkill (it gibs).
    pub overkill_on_dismiss: bool,
    /// The record's `gibThreshold` (percent of the max life; `Character::
    /// GetGibThreshold`; absent: never): a release is an overkill when the
    /// pet's remaining life reaches it (`ReleasePetConfigCmd::Execute`).
    pub gib_threshold: Option<i32>,
    /// The controller's "Startup" state (module docs): it ignores a player's
    /// orders until its Spawn animation (type 19) ends, at once when there is none.
    pub startup: bool,
    /// The Spawn animation was tried (once, `OnBegin`'s state flag +32).
    pub startup_played: bool,
    /// The special `ChooseBestSkill` chose that is still out of reach: the
    /// pet pursues for it (index into [`Pet::specials`]).
    pub(super) held: Option<usize>,
    /// `dyingSkillName` (skill, level index, kind), cast where it dies
    /// ([`pet_dying_skills`]).
    pub(super) dying: Option<(String, usize, Kind)>,
    /// Current energy (`CharacterBio::GetCurrentMana`): full at the summon,
    /// regenerating by the profile's energy regeneration; no stock pet skill
    /// costs energy, so only mana burn lowers it (`combat.rs`).
    pub energy: f32,
    /// The 4 s `WheneverPossible` timer (`ControllerMonster::Update`).
    pub(super) buff_poll: f32,
    /// Seconds to the next `ChooseBestSkill` poll while pursuing (200 ms).
    pub(super) choose_poll: f32,
    /// It had a target last frame (`DefaultEnemyFoundResponse` on a new one).
    pub(super) had_target: bool,
    /// An attack or cast was running last frame (its end is the Attack
    /// state's "End").
    pub(super) was_busy: bool,
    /// Its profile must be rebuilt (a toggle went on).
    pub(super) dirty: bool,
    /// The owner's buffs with a pet bonus its profile was built with.
    pub(super) owner_buff_bonus: Vec<(String, usize)>,
    /// The pet record and character level (to rebuild the profile when the
    /// owner's pet bonuses change).
    pub record: String,
    pub(super) level: i32,
    pub(super) ctl: PetController,
    /// The controller in use.
    pub(super) stance: Stance,
    /// The commanded enemy (`SetMostHatedEnemy`), chosen before any anger.
    pub(super) most_hated: Option<Entity>,
    /// The "Move" state's destination (a move command).
    pub(super) moving: Option<Vec3>,
    /// The dodge timer (s, `ControllerMonster` +0x644) and the DodgeAttack
    /// state (`dodge.rs`): it runs along [`Pet::path`].
    pub dodge_timer: f32,
    pub dodging: bool,
    /// Anger per foe (`AngerManager`), see [`add_anger`].
    pub(super) anger: Vec<(Entity, f32)>,
    /// `UnderAttack`'s 500 ms gate.
    pub(super) anger_timer: f32,
    /// Seconds to the next look at its spot by the owner.
    pub(super) follow_check: f32,
    pub(super) following: bool,
    /// A monster's convert held it last update ([`crate::combat::JoinedPack`]): a change clears its anger, as
    /// `Monster::JoinMe` / `UnJoinLeader` clear the controller's anger list and target.
    pub(super) joined: bool,
    /// The defence slot it runs to.
    pub(super) slot: Option<Vec3>,
    pub(super) path: Vec<Vec3>,
    pub(super) repath: f32,
    /// The monster fight states (`ControllerPet` registers the monster's).
    pub(super) melee: crate::melee::MeleeAi,
    /// `walkDistance`, `walkSpeed`, `walkUsesRun` (`Character::Load`).
    pub(super) walk: crate::melee::Walk,
}

/// The pet's `controller` record settings the AI uses.
#[derive(Clone, Copy, Debug)]
pub(super) struct PetController {
    /// The record's `Class` (`combat::ControllerClass`): the stationary
    /// classes never move.
    pub(super) class: crate::combat::ControllerClass,
    pub(super) view: f32,
    pub(super) view_y: f32,
    pub(super) pursuit: f32,
    pub(super) teleport: f32,
    /// `InnerViewDistance`, `SightAngerRate`, `InnerSightAngerRate`.
    pub(super) sight: (f32, f32, f32),
    pub(super) forgive: f32,
    pub(super) attacked_anger: f32,
    /// `PursuitTime` (ms) and `randomRepositionChance` (`ControllerMonster`).
    pub(super) pursuit_ms: i32,
    pub(super) random_reposition: i32,
    /// `RepositionChance` (`ProjectileCollided`).
    pub(super) reposition_chance: i32,
    /// `minSwingPause` / `maxSwingPause` (s): the swing timer re-arms from
    /// them as an attack starts (`ResetSwingTimer` @0x18013d030,
    /// `combat::monster_ai`; 47 of the 48 pet controllers carry the field).
    pub(super) swing_pause: (f32, f32),
    /// `BuffSelfBehavior` (0 NeverUseSkill … 3 WheneverPossible, default
    /// WhenEnemyIsSeen; `monster_skills` module docs).
    pub(super) buff_self: u8,
    /// `BuffAllyBehavior` (default NeverUseSkill) and `BuffAllyTargeting`
    /// (0 closest, 1 strongest, 2 weakest).
    pub(super) buff_ally: u8,
    pub(super) buff_targeting: u8,
    /// `DodgeChance`, `DodgeDelay` (s), `DodgeDistance`, `MinDodgeDistance`,
    /// `ProjectileAnger` (`dodge::Dodge`).
    pub(super) dodge: (i32, f32, f32, f32, f32),
}

impl Pet {
    /// Rebuilds its profile in the next refresh (`refresh_profiles`; its challenge area's rules changed).
    pub fn mark_profile_dirty(&mut self) {
        self.dirty = true;
    }

    /// Whether it is angry at `foe`.
    pub fn has_anger_at(&self, foe: Entity) -> bool {
        self.anger.iter().any(|a| a.0 == foe && a.1 > 0.0)
    }

    /// `ControllerMonsterStateAttack::ProjectileCollisionCallback` for a pet
    /// ([`crate::melee::ProjectileCollided`]); `can_move`: not rooted and a
    /// moving controller class.
    pub(crate) fn projectile_collided(&mut self, can_move: bool) {
        let chance = self.ctl.reposition_chance;
        self.melee
            .projectile_collided(chance, can_move && self.ctl.class.can_move());
    }

    /// Tests: the Attack state's reposition flag and whether it is in that state.
    #[doc(hidden)]
    pub fn attack_reposition_flag(&self) -> (bool, bool) {
        (self.melee.in_attack(), self.melee.reposition_flag())
    }

    /// Tests: sets the controller's `RepositionChance`.
    #[doc(hidden)]
    pub fn set_reposition_chance(&mut self, chance: i32) {
        self.ctl.reposition_chance = chance;
    }

    /// Tests: sets the controller's `DodgeChance` and `MinDodgeDistance`.
    #[doc(hidden)]
    pub fn set_dodge(&mut self, chance: i32, min: f32) {
        self.ctl.dodge.0 = chance;
        self.ctl.dodge.3 = min;
    }

    /// `ControllerMonster::ProjectileNotification` for a pet (`dodge.rs`
    /// module docs): `at` is the foe's projectile, `from` its owner.
    pub fn projectile_notified(
        &mut self,
        e: Entity,
        (me, radius, at): (Vec3, f32, Vec3),
        (from, state): (Entity, CharState),
        nav: &Nav,
        scratch: &mut gd_sim::nav::PathScratch,
    ) {
        let (chance, delay, dist, min, anger) = self.ctl.dodge;
        if self.dodge_timer > 0.0 {
            self.dodge_timer = delay;
            return;
        }
        // The Startup state's handler is empty (the state slots of
        // `ControllerMonster::ProjectileNotification`, `dodge.rs` module docs):
        // a pet still playing its Spawn animation does not dodge.
        let responds = self.ctl.class.can_move()
            && !self.startup
            && self.moving.is_none()
            && !self.dodging
            && self.melee.st != crate::melee::St::Navigate
            && !matches!(
                state,
                CharState::Attack | CharState::Cast | CharState::Death
            );
        if responds
            && crate::rng::i32(0..100) < chance
            && me.distance(at) >= radius + min
            && let Some(path) = crate::patrol::pick_spot(
                me,
                me,
                dist,
                dist,
                dist - 0.1,
                self.ctl.view_y,
                nav,
                scratch,
            )
        {
            debug!("pet {e} dodges a projectile");
            self.path = path;
            self.dodging = true;
            self.following = false;
        }
        add_anger(&mut self.anger, from, anger);
        self.dodge_timer = delay;
    }
}

impl PetController {
    /// The controller of `stance` named by the pet record; None when the
    /// record names none (`Monster::UseController` then keeps the current
    /// one; Normal falls back to the defaults).
    pub(super) fn of_stance(
        data: &GameData,
        pet_rec: &gd_formats::arz::Record,
        stance: Stance,
    ) -> Option<Self> {
        let ctl = pet_rec
            .str(stance.field())
            .filter(|c| !c.is_empty())
            .and_then(|c| data.db.get(c));
        if ctl.is_none() && stance != Stance::Normal {
            return None;
        }
        Some(Self::from_controller(ctl))
    }

    /// `InPursuitRange` for a pet: a foe at `p` counts within
    /// `MaxPursuitDistance` of the leader at `leader` and `MaxYViewDistance`
    /// in height of the pet at `me`.
    pub(super) fn in_pursuit(&self, leader: Vec3, p: Vec3, me: Vec3) -> bool {
        (self.view_y <= 0.0 || (p.y - me.y).abs() <= self.view_y)
            && (self.pursuit <= 0.0 || leader.distance(p) < self.pursuit)
    }

    /// Fallbacks for a missing field as `combat::MonsterAi::from_record`
    /// (the `ControllerMonster::Load` defaults; the
    /// `controllers/pets` records set them).
    pub(super) fn from_controller(ctl: Option<&gd_formats::arz::Record>) -> Self {
        let f = |k: &str, d: f32| ctl.and_then(|c| c.f32(k)).unwrap_or(d);
        Self {
            class: crate::combat::ControllerClass::of(ctl.and_then(|c| c.str("Class"))),
            view: f("ViewDistance", 15.0),
            view_y: f("MaxYViewDistance", 0.0),
            pursuit: f("MaxPursuitDistance", 0.0),
            teleport: f("TeleportToLeaderDistance", 20.0),
            sight: (
                f("InnerViewDistance", 5.0),
                f("SightAngerRate", 20.0),
                f("InnerSightAngerRate", 20.0),
            ),
            forgive: f("ForgiveRate", 50.0),
            attacked_anger: f("AttackedAnger", 100.0),
            pursuit_ms: ctl.and_then(|c| c.i32("PursuitTime")).unwrap_or(10000),
            random_reposition: ctl
                .and_then(|c| c.i32("randomRepositionChance"))
                .unwrap_or(0),
            reposition_chance: ctl.and_then(|c| c.i32("RepositionChance")).unwrap_or(0),
            swing_pause: (f("minSwingPause", 0.0), f("maxSwingPause", 0.0)),
            buff_self: crate::monster_skills::behaviour(
                ctl.and_then(|c| c.str("BuffSelfBehavior")),
                2,
            ),
            buff_ally: crate::monster_skills::behaviour(
                ctl.and_then(|c| c.str("BuffAllyBehavior")),
                0,
            ),
            buff_targeting: match ctl.and_then(|c| c.str("BuffAllyTargeting")) {
                Some("BuffStrongest") => 1,
                Some("BuffWeakest") => 2,
                _ => 0,
            },
            dodge: {
                let d = crate::dodge::Dodge::from_controller(ctl);
                (d.chance, d.delay, d.distance, d.min, d.anger)
            },
        }
    }
}

pub(super) use crate::combat::add_anger;
