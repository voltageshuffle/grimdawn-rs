use super::*;

/// The class of the "Pet Attack" skill.
pub const PET_ATTACK_CLASS: &str = "Skill_PetAttack";

/// An order to the owner's pets: `pet: None` is the Pet Attack skill (every pet), `Some` one selected pet
/// (`InGameUI::OrderSelectedPets`); a `target` is attacked, else they move to `point`.
#[derive(Message, Clone, Copy, Debug)]
pub struct PetCommand {
    pub owner: Entity,
    pub pet: Option<Entity>,
    pub target: Option<Entity>,
    pub point: Vec3,
}

/// A pet's controller (`Monster::ControllerType`).
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

    /// The portrait's forward stance key (action 15, `PlayerPortrait::HandleKeyEvent`).
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

/// `Skill_SpawnPet::StopSkill`: all the owner's live pets of `skill` die.
#[derive(Message, Clone, Debug)]
pub struct StopSummonSkill {
    pub owner: Entity,
    pub skill: String,
}

/// A quest pet released by `UnJoinLeader(false)`: alive and friendly, no longer a [`Pet`]. PORT: it stands where it
/// was (no ambient AI for a leaderless friendly).
#[derive(Component)]
pub struct ReleasedQuestPet;

/// "Disband Pet" from a portrait's menu: the owner's pet dies.
#[derive(Message, Clone, Copy, Debug)]
pub struct DisbandPet {
    pub pet: Entity,
}

/// `ControllerPlayer::ReleasePet(pet, suppressDyingSkill)`: the pet dies, an overkill when its remaining life is at
/// least max life × gib threshold; `suppress_dying_skill` keeps its dying skill from going off. [`DisbandPet`] is
/// this with the flag false.
#[derive(Message, Clone, Copy, Debug)]
pub struct ReleasePet {
    pub pet: Entity,
    pub suppress_dying_skill: bool,
}

/// `Character.dyingSkillSuppressed`: the pet's dying skill is not cast.
#[derive(Component)]
pub struct SuppressDyingSkill;

/// The owner's pets with a portrait (`showStatusWidgetWhenPet`), in summon order; dead pets have none.
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

/// On a pet owner: the stance per summon skill (`Player::SetPetControllerType`; session only).
#[derive(Component, Default, Debug, Clone)]
pub struct PetStances(pub Vec<(String, Stance)>);

impl PetStances {
    pub fn get(&self, summon: &str) -> Stance {
        self.0
            .iter()
            .find(|(s, _)| s.eq_ignore_ascii_case(summon))
            .map_or(Stance::Normal, |(_, st)| *st)
    }

    /// The stance a new portrait shows (`Player::GetPetControllerType`: Aggressive for a skill with no stored type,
    /// although the pet runs on Normal).
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
    /// `Skill::GetRange`; 0 for a weapon primary (the melee swing's reach).
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
    /// `specialAttack*Timeout` left: counted down only once the pet has found an enemy; the special waits until it
    /// is out.
    pub timeout: f32,
}

#[derive(Component)]
pub struct Pet {
    pub owner: Entity,
    pub summon: String,
    /// Summoned by a `Skill_SpawnMiniPet`: counts towards `miniPetLimit`.
    pub mini: bool,
    /// Summoned by a `Skill_SpawnQuestPet`: in the owner's quest pen, so other summons' pet limits do not count it
    /// and its stop releases it alive.
    pub quest: bool,
    pub spawned_at: f32,
    /// Seconds it has left to live (`Monster::SetLifetime`, [`spawn_time_to_live`]); None lives until killed,
    /// replaced or disbanded.
    pub time_to_live: Option<f32>,
    /// It has found an enemy once (`ControllerMonster::FindEnemy`'s flag):
    /// the specials' timeouts count from then on.
    pub found_enemy: bool,
    pub primary: Option<PetAttack>,
    pub specials: Vec<PetAttack>,
    /// The record's `angerMultiplier` (`Character::Load`, default 1): scales
    /// the anger monsters gain towards it (`AngerManager::AddAnger`).
    pub anger_multiplier: f32,
    /// The record's `causesAnger` (default 1): a converted pet near the player holds the controller's combat flag.
    pub causes_anger: bool,
    /// The record's `targetable` (default 1): whether the controller targeting takes a converted pet.
    pub targetable: bool,
    /// The record's `showStatusWidgetWhenPet`: it gets a HUD portrait.
    pub portrait: bool,
    /// The record's class is `PetPlayerScaling`: its attacks are collected with the owner's attributes and racial
    /// bonuses.
    pub player_scaling: bool,
    /// The record's `buffSelfSkillName`, `buffSelf2…`, `buffSelf3…` that it
    /// has (skill, level index, a toggle), in that order.
    pub(super) buff_self: Vec<(String, usize, bool)>,
    /// Toggled skills its AI switched on (skill, level index).
    pub(super) toggled: Vec<(String, usize)>,
    /// The timed self buffs: its own (under the skill's name) and others' `Skill_BuffOther` (under the buff
    /// record's).
    pub(super) buffs: gd_game_core::skill_buffs::TimedBuffs,
    /// `buffOtherSkillName`: (skill, level index, buff record, seconds), as
    /// `MonsterSkills::buff_other`.
    pub(super) buff_other: Option<(String, usize, String, f32)>,
    /// A buff-other use is due (the 4 s WheneverPossible poll, or an enemy
    /// was found for WhenEnemyIsSeen).
    pub(super) ally_due: bool,
    /// `skillCooldownTime` left of its used self / ally buffs and initial skills.
    pub(super) cooldowns: Vec<(String, f32)>,
    /// Initial skills that are not toggles, still to cast on itself
    /// (`UseInitialSkillIfSet`).
    pub(super) initial_casts: Vec<(String, usize)>,
    /// The record's `overkillOnDismiss` (`Character::GetDismissBehavior`):
    /// its time to live running out kills it as an overkill (it gibs).
    pub overkill_on_dismiss: bool,
    /// The record's `gibThreshold` (percent of max life; absent: never): a release is an overkill when the
    /// remaining life reaches it.
    pub gib_threshold: Option<i32>,
    /// The controller's "Startup" state: it ignores a player's orders until its Spawn animation (type 19) ends, at
    /// once when there is none.
    pub startup: bool,
    /// The Spawn animation was tried (once, `ControllerMonsterStateStartup.spawnTried`, set by `OnBegin`).
    pub startup_played: bool,
    /// The special `ChooseBestSkill` chose that is still out of reach: the
    /// pet pursues for it (index into [`Pet::specials`]).
    pub(super) held: Option<usize>,
    /// `dyingSkillName` (skill, level index, kind), cast where it dies.
    pub(super) dying: Option<(String, usize, Kind)>,
    /// Current energy: full at the summon, regenerating; only mana burn lowers it (no stock pet skill costs energy).
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
    /// The pet record and character level, for profile rebuilds.
    pub record: String,
    pub(super) level: i32,
    pub(super) ctl: PetController,
    pub(super) stance: Stance,
    /// The commanded enemy (`SetMostHatedEnemy`), chosen before any anger.
    pub(super) most_hated: Option<Entity>,
    /// The "Move" state's destination (a move command).
    pub(super) moving: Option<Vec3>,
    /// The dodge timer (s, `ControllerMonster.dodgeTimer`) and the DodgeAttack state: it runs along [`Pet::path`].
    pub dodge_timer: f32,
    pub dodging: bool,
    /// Anger per foe (`AngerManager`), see [`add_anger`].
    pub(super) anger: Vec<(Entity, f32)>,
    /// `UnderAttack`'s 500 ms gate.
    pub(super) anger_timer: f32,
    /// Seconds to the next `AngerUpdate` tick (250 ms period, phase `rand() % 251` ms): sight anger and forgiving.
    pub(super) anger_tick: f32,
    /// Seconds to the next `FindEnemyUpdate` (200 ms period): the target is picked afresh only then.
    pub(super) find_timer: f32,
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
    /// The record's `Class`: the stationary classes never move.
    pub(super) class: gd_game_core::combat::ControllerClass,
    pub(super) view: f32,
    pub(super) view_y: f32,
    pub(super) pursuit: f32,
    pub(super) teleport: f32,
    /// `InnerViewDistance`, `SightAngerRate`, `InnerSightAngerRate`.
    pub(super) sight: (f32, f32, f32),
    pub(super) forgive: f32,
    pub(super) attacked_anger: f32,
    /// `AngerTolerance` (default 10): least anger a target needs (`GetNewTarget`).
    pub(super) tolerance: f32,
    /// `pathingViewDistance` (0: off): sight anger only for foes a path no longer than it reaches.
    pub(super) pathing_view: f32,
    /// `ignoreSleepingEnemies`: a sleeping foe that is not a player gains no anger and is no target.
    pub(super) ignore_sleeping: bool,
    /// `petTargetGreatestHealth`: `GetNewTarget`'s comparator ranks by life percent first.
    pub(super) greatest_health: bool,
    /// `PursuitTime` (ms) and `randomRepositionChance` (`ControllerMonster`).
    pub(super) pursuit_ms: i32,
    pub(super) random_reposition: i32,
    /// `RepositionChance` (`ProjectileCollided`).
    pub(super) reposition_chance: i32,
    /// `minSwingPause` / `maxSwingPause` (s): the swing timer re-arms from them as an attack starts
    /// (`ResetSwingTimer`).
    pub(super) swing_pause: (f32, f32),
    /// `BuffSelfBehavior` (0 NeverUseSkill … 3 WheneverPossible, default WhenEnemyIsSeen).
    pub(super) buff_self: u8,
    /// `BuffAllyBehavior` (default NeverUseSkill) and `BuffAllyTargeting`
    /// (0 closest, 1 strongest, 2 weakest).
    pub(super) buff_ally: u8,
    pub(super) buff_targeting: u8,
    /// `DodgeChance`, `DodgeDelay` (s), `DodgeDistance`, `MinDodgeDistance`, `ProjectileAnger`.
    pub(super) dodge: (i32, f32, f32, f32, f32),
}

impl Pet {
    /// Rebuilds its profile in the next refresh (its challenge area's rules changed).
    pub fn mark_profile_dirty(&mut self) {
        self.dirty = true;
    }

    /// `ControllerMonsterStateAttack::ProjectileCollisionCallback` for a pet; `can_move`: not rooted and a moving
    /// controller class.
    pub(crate) fn projectile_collided(&mut self, can_move: bool) {
        let chance = self.ctl.reposition_chance;
        self.melee
            .projectile_collided(chance, can_move && self.ctl.class.can_move());
    }

    /// `ControllerMonster::ProjectileNotification` for a pet: `at` is the foe's projectile, `from` its owner,
    /// `mult` the owner's anger multiplier.
    pub fn projectile_notified(
        &mut self,
        e: Entity,
        (me, radius, at): (Vec3, f32, Vec3),
        (from, state, mult): (Entity, CharState, f32),
        nav: &Nav,
        scratch: &mut gd_sim::nav::PathScratch,
    ) {
        let (chance, delay, dist, min, anger) = self.ctl.dodge;
        if self.dodge_timer > 0.0 {
            self.dodge_timer = delay;
            return;
        }
        // The Startup state ignores it: a pet still playing its Spawn animation does not dodge.
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
            && gd_game_core::rng::i32(0..100) < chance
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
        add_anger(&mut self.anger, from, anger * mult);
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

    /// Fallbacks for a missing field as `combat::MonsterAi::from_record` (the `ControllerMonster::Load` defaults).
    pub(super) fn from_controller(ctl: Option<&gd_formats::arz::Record>) -> Self {
        let f = |k: &str, d: f32| ctl.and_then(|c| c.f32(k)).unwrap_or(d);
        Self {
            class: gd_game_core::combat::ControllerClass::of(ctl.and_then(|c| c.str("Class"))),
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
            tolerance: f("AngerTolerance", 10.0),
            pathing_view: f("pathingViewDistance", 0.0),
            ignore_sleeping: ctl.is_some_and(|c| c.bool("ignoreSleepingEnemies")),
            greatest_health: ctl.is_some_and(|c| c.bool("petTargetGreatestHealth")),
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
