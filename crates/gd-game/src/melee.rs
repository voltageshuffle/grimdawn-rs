//! The monster fight states that bring a monster to its enemy and the
//! `SlotManager` plumbing they, the followers (patrol.rs) and the pets
//! (pets.rs) share.
//!
//! # Slots
//!
//! Every living character carries a [`MeleeSlots`] component: its attack and defence `SlotManager`
//! (`gd_sim::slots`, `Character::RequestAttackSlot` / `RequestDefenseSlot`), created with `numAttackSlots` /
//! `numDefenseSlots` (4 unless the record says, `Character::Load`), plus the body (position, radius) the managers
//! ask the object manager for, synced once per frame by [`sync_slots`]. A character that dies loses the
//! component, so its slots elsewhere are free (`CleanupRing`'s `IsAlive`). Systems reach the components through
//! [`Slots`], a borrow of their [`SlotQuery`].
//!
//! # The states
//!
//! `ControllerMonsterState{Pursue, WaitToAttack, Attack, RepositionForAttack}`
//! (Game.dll), each with the enemy and the chosen skill as state
//! data; [`MeleeAi`] is their state machine for a monster that has an enemy.
//! A transition runs the new state's `OnBegin` at once
//! (`ControllerAI::SetState`), which also releases the monster's slots of the
//! old enemy when the state data's enemy changes.
//!
//! * `GetMoveToPoint` (`Character::GetMoveToPoint`, enemy that is
//!   a character): with a skill whose `Skill::GetRangeProfile` is Melee, a
//!   monster asks the enemy's attack manager for a slot
//!   (`RequestSlot(enemy attack slots, me, GetTargetDistance, false)`:
//!   spacing = the skill's `Skill::GetRange` + both radii); none free: no
//!   point. Any other skill: the point on the path to the enemy that is
//!   `Skill::GetRange` + the enemy's radius short of it
//!   (`CharacterMovementManager::GetPointAwayFromGoal`, [`point_away`]);
//!   without a skill (RepositionForAttack): the same at both radii.
//! * Pursue `OnBegin`: no `CanMove` (rooted): Idle. (The
//!   stationary classes' own Pursue: in reach "Attack", else "Idle",
//!   `combat::ControllerClass`.) No move
//!   point: "WaitToAttack". `CloseEnoughToUseSkill` already, or a skill of
//!   move type 3 / 5 (`Skill_AttackRadiusLeap`, `...Disengage`: jumped, not
//!   walked, to use): "Attack". Else the point more than 0.025 m away and
//!   reachable (`CanMoveTo`): go there, `PursuitTime` ms to live and the
//!   skill re-chosen every 200 ms; one that cannot be reached:
//!   "NavigateObstacle". The pace: with a `walkDistance` > 0 and the point
//!   within it the monster walks (`walkSpeed` x its run speed, the Walk
//!   animation unless `walkUsesRun`); else it runs, at unit speed for a move
//!   type 2 skill (`Skill_AttackPathCharge`, charge animation) and at its
//!   run speed otherwise. A walker that is later farther from its goal than
//!   `walkDistance` runs on (`OnUpdate`). The move carries the skill
//!   (`MoveToAction::Execute` → its `WarmUpStart`): a weapon
//!   charge warms up on it, its clip at the warmed-up run speed until the
//!   move ends (`crate::casting::begin_pursuit_charge`,
//!   `crate::combat::charge_move`, run by `combat::monster_ai`).
//!   `OnUpdate`: `PursuitTime` out: "Return"; a re-chosen skill (only while the skill's
//!   `Skill::CanRechooseInPursuit` holds, [`SkillMove::rechooses`])
//!   that differs: "Pursue" again; close enough, or the skill's `MaxSkillDistance`
//!   (`Skill::GetMaxSkillDistance`, the goal passed with the monster's position; only `Skill_AttackWeaponCharge`,
//!   `...Blink`,
//!   `Skill_AttackPathCharge`: ran farther from the last charge start than the goal plus
//!   `maxDistanceBuffer`, never with a 0 buffer): "Attack". Reaching the point
//!   (`EndOfPathReached`): close enough, or a skill whose
//!   `Skill::AttacksOnArrival` is true (`Skill_Evade`, `Skill_AttackRadiusDisengage`,
//!   `...Leap`, `Skill_AttackPathCharge`, `Skill_AttackWeaponCharge`,
//!   `...Blink`): "Attack", else the slot is released and "WaitToAttack".
//!   `LostSlot` asks for the
//!   move point again at the next update. `PathFailed` (the
//!   crowd failed the move, [`MeleeAi::note_path_failed`]): a skill whose
//!   `Skill::AttacksOnArrival` is true: "Attack"; else the slot is released and,
//!   counting the failures since the last `OnUpdate` (which zeroes the
//!   count), "RepositionForAttack" below 5, else "Idle".
//! * NavigateObstacle `OnBegin` / `OnUpdate`: no
//!   move point: Idle. A point more than 0.025 m away that can be reached is
//!   run to at unit speed; else the point shifted by `radius + 1` m (the
//!   code turns `i / 7` of its seven tries in integer arithmetic, so every
//!   try is the same, the +x shift): reachable, run there; none: Idle.
//!   `OnUpdate`: close enough "Attack", else "Pursue" again at once.
//! * WaitToAttack `OnBegin` / `OnUpdate`: every
//!   333 ms it asks for the move point again: found "Pursue", else 50 % of
//!   the time "RepositionForAttack". Meanwhile it `EmoteOrRoam`s: every
//!   1000 ms it runs to a random spot at half the distance (4..10 m, halved)
//!   to the enemy on the circle around the point that far from the enemy
//!   toward itself (`StartRoaming`); when it cannot roam it
//!   stands.
//! * Attack `OnBegin` / `OnUpdate`: stands; once
//!   the swing timer has run out it attacks (`ResetSwingTimer`,
//!   `AttackEnemyOrReturn`: the skill is used wherever the enemy now is,
//!   `UseSkill` has no range test); when the attack has ended it chooses
//!   again: close enough: with `randomRepositionChance` % (rolled until it
//!   succeeds, needs `CanMove`; or the flag a projectile's world contact set,
//!   [`ProjectileCollided`]) and the same skill "RepositionForAttack",
//!   else "Attack"; not close enough: "Pursue". `LostSlot`:
//!   "Pursue". `OnBegin` also zeroes the swing timer for a skill of move
//!   type 2 (`Skill_AttackPathCharge`) or a `Skill_AttackWeaponCharge`
//!   (`...Blink`).
//! * RepositionForAttack `OnBegin`: runs to the move point
//!   without a skill (both radii from the enemy). `OnUpdate`: a
//!   monster that is not running ends the path (`EndOfPathReached`:
//!   close enough "Attack", else "Pursue"); while it runs the
//!   2500 ms timer counts down, then close enough: "Attack". `PathFailed`:
//!   a random spot 3..6 m away (more than 3 m) to run to, none:
//!   "Return"; from the 5th failure since the last `OnUpdate` the slot is
//!   released and it waits ("WaitToAttack").
//! * WaitToAttack `PathFailed` (its roaming walk failed):
//!   `EmoteOrRoam`, which cannot: "Idle". The other fight states' `PathFailed`
//!   is the empty base handler.
//!
//! `LostSlot` (`Character::LostSlot` -> the controller's current state): only
//! Pursue, Attack and FollowLeader (patrol.rs) react; WaitToAttack and
//! RepositionForAttack have the empty base handler, so a slot lost there is
//! dropped ([`Slots::take_lost`] is read every update).
//!
//! Pets (`ControllerPet` registers the monster's states) run these states
//! too (pets.rs).
use crate::nav::Nav;
use bevy::prelude::*;
use gd_game_core::combat::Combatant;
use gd_sim::slots::{SlotEnv, SlotManager, SlotMode};
use std::cell::RefCell;

pub struct MeleePlugin;

impl Plugin for MeleePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ProjectileCollided>().add_systems(
            Update,
            projectile_collisions.before(gd_game_core::combat::CombatSet),
        );
    }
}

/// A projectile stopped by the world (terrain, scenery, a door): sent by
/// `casting.rs`' flight (`ProjectileBase::CollisionCallback` →
/// `Character::ProjectileCollisionCallback` → the owner's
/// controller → its current state).
#[derive(Message, Clone, Copy, Debug)]
pub struct ProjectileCollided {
    pub attacker: Entity,
}

/// `ControllerMonsterStateAttack::ProjectileCollisionCallback`:
/// in the Attack state the reposition flag (the one
/// `randomRepositionChance` rolls, `ControllerMonsterStateAttack.reposition`) becomes `rand() % 100 <
/// RepositionChance && CanMove`; the attack's end then runs
/// "RepositionForAttack" when the same skill is chosen again (see the
/// Attack state above). The other states have the empty base handler.
/// `ProjectileResultCallback` clears the flag for a result on
/// the entity it names. A pet's controller is the same monster state
/// machine, so its projectiles report too ([`crate::pets::Pet::projectile_collided`]);
/// a projectile that hits a target never collided.
fn projectile_collisions(
    mut seen: MessageReader<ProjectileCollided>,
    mut monsters: Query<&mut crate::combat::MonsterAi>,
    mut pets: Query<&mut crate::pets::Pet>,
    rooted: Query<(), With<crate::movement::Rooted>>,
) {
    for c in seen.read() {
        if let Ok(mut pet) = pets.get_mut(c.attacker) {
            pet.projectile_collided(!rooted.contains(c.attacker));
            continue;
        }
        let Ok(mut ai) = monsters.get_mut(c.attacker) else {
            continue;
        };
        let can_move = ai.class.can_move() && !rooted.contains(c.attacker);
        let chance = ai.reposition_chance;
        ai.melee.projectile_collided(chance, can_move);
    }
}

/// `numAttackSlots` / `numDefenseSlots` defaults (`Character::Load`).
const DEFAULT_SLOTS: usize = 4;

/// A living character's slot managers, its body as the managers see it this frame, and whether it was handed
/// `LostSlot` since it last looked.
#[derive(Component)]
pub struct MeleeSlots {
    attack: SlotManager,
    defense: SlotManager,
    pos: Vec3,
    radius: f32,
    lost: bool,
}

/// The query a system borrows into [`Slots`].
pub type SlotQuery<'w, 's> = Query<'w, 's, &'static mut MeleeSlots>;

/// The slot store behind [`Slots`] (a [`SlotQuery`]).
pub trait SlotStore {
    fn slots_mut(&mut self, e: Entity) -> Option<&mut MeleeSlots>;
    fn body(&self, e: Entity) -> Option<(Vec3, f32)>;
}

impl SlotStore for SlotQuery<'_, '_> {
    fn slots_mut(&mut self, e: Entity) -> Option<&mut MeleeSlots> {
        self.get_mut(e).ok().map(Mut::into_inner)
    }
    fn body(&self, e: Entity) -> Option<(Vec3, f32)> {
        self.get(e).ok().map(|s| (s.pos, s.radius))
    }
}

/// The characters' slot managers for one system run (see the module docs): `Slots::new(&mut slot_query, &scratch)`
/// with the system's `Local<RefCell<PathScratch>>` (the slot search's path scratch).
pub struct Slots<'a> {
    store: RefCell<&'a mut (dyn SlotStore + 'a)>,
    scratch: &'a RefCell<gd_sim::nav::PathScratch>,
}

struct Env<'a> {
    store: &'a dyn SlotStore,
    nav: &'a Nav,
    from: Vec3,
    scratch: &'a RefCell<gd_sim::nav::PathScratch>,
}

impl SlotEnv for Env<'_> {
    fn body(&self, id: u64) -> Option<([f32; 3], f32)> {
        self.store
            .body(Entity::from_bits(id))
            .map(|b| (b.0.to_array(), b.1))
    }
    fn on_path_mesh(&self, p: [f32; 3]) -> bool {
        self.nav.0.locate(p).is_some()
    }
    fn can_move_to(&self, p: [f32; 3]) -> bool {
        self.nav
            .0
            .find_path(
                &mut self.scratch.borrow_mut(),
                self.from.to_array(),
                p,
                crate::nav::PATH_BUDGET,
            )
            .is_some()
    }
}

impl<'a> Slots<'a> {
    pub fn new(
        store: &'a mut (dyn SlotStore + 'a),
        scratch: &'a RefCell<gd_sim::nav::PathScratch>,
    ) -> Self {
        Self {
            store: RefCell::new(store),
            scratch,
        }
    }

    fn request(
        &self,
        defense: bool,
        owner: Entity,
        req: Entity,
        spacing: impl Fn(f32, f32) -> f32,
        nav: &Nav,
    ) -> Option<Vec3> {
        let mut store = self.store.borrow_mut();
        let (rpos, rrad) = store.body(req)?;
        // The manager is taken out while the other bodies are read, then put back.
        let (opos, orad, mut m) = {
            let s = store.slots_mut(owner)?;
            let mode = if defense {
                SlotMode::Defense
            } else {
                SlotMode::Attack
            };
            let slot = if defense {
                &mut s.defense
            } else {
                &mut s.attack
            };
            let m = std::mem::replace(slot, SlotManager::new(mode));
            (s.pos, s.radius, m)
        };
        let got = m.request(
            opos.to_array(),
            req.to_bits(),
            rpos.to_array(),
            rrad,
            spacing(orad, rrad),
            false,
            &Env {
                store: &**store,
                nav,
                from: rpos,
                scratch: self.scratch,
            },
        );
        if let Some(s) = store.slots_mut(owner) {
            *(if defense {
                &mut s.defense
            } else {
                &mut s.attack
            }) = m;
        }
        let got = got?;
        if let Some(l) = got.lost
            && let Some(s) = store.slots_mut(Entity::from_bits(l))
        {
            s.lost = true;
        }
        Some(Vec3::from(got.pos))
    }

    /// `Character::RequestAttackSlot` / `GetMoveToPoint`'s melee branch: a
    /// spot around `owner` for `req`, `range` (`Skill::GetRange`) + both
    /// radii from it (`GetTargetDistance`).
    pub fn attack(&self, owner: Entity, req: Entity, range: f32, nav: &Nav) -> Option<Vec3> {
        self.request(false, owner, req, |o, r| range + o + r, nav)
    }

    /// `Character::RequestDefenseSlot`: both radii + 0.5 from `owner`.
    pub fn defense(&self, owner: Entity, req: Entity, nav: &Nav) -> Option<Vec3> {
        self.request(true, owner, req, |o, r| o + r + 0.5, nav)
    }

    pub fn release_attack(&self, owner: Entity, req: Entity) {
        if let Some(s) = self.store.borrow_mut().slots_mut(owner) {
            s.attack.release(req.to_bits());
        }
    }

    pub fn release_defense(&self, owner: Entity, req: Entity) {
        if let Some(s) = self.store.borrow_mut().slots_mut(owner) {
            s.defense.release(req.to_bits());
        }
    }

    /// `ControllerAI::SetState` releasing the slots a monster held of the
    /// state data's old enemy.
    pub fn release_all(&self, owner: Entity, req: Entity) {
        if let Some(s) = self.store.borrow_mut().slots_mut(owner) {
            s.attack.release(req.to_bits());
            s.defense.release(req.to_bits());
        }
    }

    /// `Character::LostSlot` was sent to `e` since the last call.
    pub fn take_lost(&self, e: Entity) -> bool {
        self.store
            .borrow_mut()
            .slots_mut(e)
            .is_some_and(|s| std::mem::take(&mut s.lost))
    }
}

/// Gives new living characters their managers (`Character::Load`'s `numAttackSlots` / `numDefenseSlots`), syncs
/// the bodies the managers read, and drops the managers of the dead.
pub fn sync_slots(
    mut commands: Commands,
    data: Res<gd_game_core::data::GameData>,
    mut q: Query<
        (
            Entity,
            &Transform,
            &Combatant,
            Option<&gd_game_core::monsters::Monster>,
            Option<&mut MeleeSlots>,
        ),
        Without<gd_game_core::combat::Dead>,
    >,
    dead: Query<Entity, (With<gd_game_core::combat::Dead>, With<MeleeSlots>)>,
) {
    for e in &dead {
        commands.entity(e).try_remove::<MeleeSlots>();
    }
    for (e, tf, cb, monster, slots) in &mut q {
        if let Some(mut s) = slots {
            let s = s.bypass_change_detection();
            s.pos = tf.translation;
            s.radius = cb.radius;
            continue;
        }
        let rec = monster.and_then(|m| data.db.get(&m.record));
        let count = |key: &str| {
            rec.as_ref()
                .and_then(|r| r.i32(key))
                .map_or(DEFAULT_SLOTS, |n| n.max(0) as usize)
        };
        let mut attack = SlotManager::new(SlotMode::Attack);
        attack.set_num_slots(count("numAttackSlots"));
        let mut defense = SlotManager::new(SlotMode::Defense);
        defense.set_num_slots(count("numDefenseSlots"));
        commands.entity(e).try_insert(MeleeSlots {
            attack,
            defense,
            pos: tf.translation,
            radius: cb.radius,
            lost: false,
        });
    }
}

/// `CharacterMovementManager::GetPointAwayFromGoal`:
/// the path to `goal` is walked back from its end until `dist` from the goal
/// is reached (the path's own end may already be short of it). None: no path.
pub fn point_away(
    nav: &Nav,
    scratch: &mut gd_sim::nav::PathScratch,
    from: Vec3,
    goal: Vec3,
    dist: f32,
) -> Option<Vec3> {
    if dist < 0.000_1 {
        return Some(goal);
    }
    let path: Vec<Vec3> = nav
        .0
        .find_path(
            scratch,
            from.to_array(),
            goal.to_array(),
            crate::nav::PATH_BUDGET,
        )?
        .into_iter()
        .map(Vec3::from)
        .collect();
    let end = *path.last()?;
    let mut back = dist - end.distance(goal);
    if back <= 0.0 {
        return Some(end);
    }
    let mut at = path.len() - 1;
    while at > 0 {
        let seg = path[at].distance(path[at - 1]);
        if back <= seg {
            return Some(path[at] + (path[at - 1] - path[at]) / seg.max(1e-6) * back);
        }
        back -= seg;
        at -= 1;
    }
    Some(path[0])
}

/// The fight state a monster with an enemy is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum St {
    /// None of them (Idle: the next update finds the enemy and pursues).
    #[default]
    Idle,
    Pursue,
    Navigate,
    Wait,
    Attack,
    Reposition,
}

/// How a monster moves to its goal (`ControllerAI::MoveTo`'s animation and
/// speed arguments, `ControllerMonsterStatePursue::OnBegin`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pace {
    /// The Run animation at `Character::GetRunSpeed`.
    #[default]
    Run,
    /// Unit speed (`NavigateObstacle`, a walker that runs on).
    Unit,
    /// Unit speed with the Charge animation (type 7): a move type 2 skill.
    Charge,
    /// `walkDistance`: `walkSpeed` x the run speed.
    Walk,
}

/// `Character::Load`: `walkDistance`, `walkSpeed`, `walkUsesRun`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Walk {
    pub distance: f32,
    pub speed: f32,
    pub uses_run: bool,
}

impl Walk {
    pub fn of(rec: Option<&gd_formats::arz::Record>) -> Self {
        let Some(r) = rec else { return Self::default() };
        Self {
            distance: r.f32("walkDistance").unwrap_or(0.0),
            speed: r.f32("walkSpeed").unwrap_or(0.0),
            uses_run: r.bool("walkUsesRun"),
        }
    }
}

/// What the states ask of a skill's class (Game.dll vtables:
/// `Skill::GetMoveType` is 1 and set by the constructors of
/// `Skill_AttackPathCharge` 2, `Skill_AttackRadiusLeap` 3 and
/// `...Disengage` 5; `Skill::AttacksOnArrival` is true and `Skill::CanRechooseInPursuit` false for the six
/// classes below, `CanRechooseInPursuit` true for the others: `Skill`, `SkillActivated`,
/// `Skill_AttackWeapon`, `...Radius`, `...Spell`, `...Projectile`, `...Chain`,
/// `SkillActivatedWeapon`, `Skill_WeaponPool_Default`).
#[derive(Clone, Copy, Debug, Default)]
pub struct SkillMove {
    /// Move type 2: charges to use.
    pub charge: bool,
    /// Move type 3 or 5: jumps to use.
    pub jump: bool,
    /// `Skill::AttacksOnArrival`: the end of the Pursue path starts the skill.
    pub shortcut: bool,
    /// `Attack::OnBegin` zeroes the swing timer.
    pub resets_swing: bool,
    /// `Skill::CanRechooseInPursuit`: Pursue chooses again every 200 ms while it runs to use the skill
    /// (`ControllerMonsterStatePursue::OnUpdate`); a charge, leap, disengage or evade is kept.
    pub rechooses: bool,
}

impl SkillMove {
    pub fn of_class(class: &str) -> Self {
        let leap = matches!(
            class,
            "Skill_AttackRadiusLeap" | "Skill_AttackRadiusDisengage"
        );
        let charge = class == "Skill_AttackPathCharge";
        let weapon_charge = matches!(
            class,
            "Skill_AttackWeaponCharge" | "Skill_AttackWeaponBlink"
        );
        let shortcut = leap || charge || weapon_charge || class == "Skill_Evade";
        Self {
            charge,
            jump: leap,
            shortcut,
            resets_swing: charge || weapon_charge,
            rechooses: !shortcut,
        }
    }
}

/// What the monster does this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Follow `path`.
    Run,
    Stand,
    /// Start the plain attack (`AttackEnemyOrReturn`; a chosen skill is
    /// started by `monster_skills`).
    Swing,
    /// The "Return" state: the fight is over for now.
    Return,
}

/// The chosen skill as the approach needs it (`monster_skills::approach`).
#[derive(Clone, Debug, Default)]
pub struct Approach {
    /// Which skill: the state data's skill id.
    pub key: String,
    /// `CloseEnoughToUseSkill`: the centre distance within which it is used.
    pub reach: f32,
    /// `Skill::GetRange`.
    pub range: f32,
    /// `Skill::GetRangeProfile` is Melee: the enemy's attack slots are used.
    pub melee: bool,
    /// The plain swing (no skill): `combat.rs` starts it.
    pub plain: bool,
    pub mv: SkillMove,
    /// `Skill_AttackWeaponCharge::MaxSkillDistance` (also the blink and path
    /// charge): where the skill's last charge started and its
    /// `maxDistanceBuffer`; none for other skills, a zero buffer or no
    /// charge yet.
    pub charge_limit: Option<(Vec3, f32)>,
}

/// Everything one update of the states reads.
pub struct Frame<'a, 's> {
    pub me: Entity,
    pub pos: Vec3,
    pub radius: f32,
    pub dt: f32,
    pub foe: (Entity, Vec3, f32),
    pub app: &'a Approach,
    /// `ControllerMonster::CanMove`.
    pub can_move: bool,
    /// A `ControllerStationaryMonster` / `ControllerTotem` (`combat::ControllerClass`):
    /// its own Pursue state.
    pub stationary: bool,
    /// The swing timer has run out.
    pub swing_ready: bool,
    pub view_y: f32,
    /// `Monster::GetLeader`.
    pub leader: Option<Entity>,
    pub walk: Walk,
    pub nav: &'a Nav,
    pub slots: &'a Slots<'s>,
    /// The system's path scratch (shared with [`Slots`]).
    pub scratch: &'a RefCell<gd_sim::nav::PathScratch>,
}

/// The states' data (see the module docs). `PursuitTime`,
/// `randomRepositionChance` come from the controller record.
#[derive(Clone, Debug, Default)]
pub struct MeleeAi {
    pub st: St,
    /// The state data's enemy and skill.
    enemy: Option<Entity>,
    key: String,
    pursuit_ms: f32,
    /// `randomRepositionChance` (%).
    pub random_reposition: i32,
    /// Pursue / Reposition / Wait timer (s) and the 200 ms re-choose timer.
    timer: f32,
    repick: f32,
    /// Pursue: `LostSlot` was sent, ask again at the next update.
    relook: bool,
    /// A `MoveTo` is under way: its path ending is `EndOfPathReached`.
    pathed: bool,
    /// WaitToAttack: `EmoteOrRoam`'s timer and flag, the roam circle.
    roam_t: f32,
    roaming: bool,
    center: Option<Vec3>,
    radius: f32,
    /// Attack: the attack was made / is over, and the reposition roll.
    attacked: bool,
    ended: bool,
    swinging: bool,
    roll: bool,
    /// The Pursue / RepositionForAttack `PathFailed` counts (`ControllerMonsterStatePursue.pathFailures` /
    /// `ControllerMonsterStateRepositionForAttack.pathFailures`, reset
    /// by their `OnUpdate`, i.e. every update).
    fails: u32,
    /// The crowd failed the move under way: `PathFailed` is due at the next
    /// update.
    failed: bool,
    /// A state asked for "Return".
    ret: bool,
    /// Pursue: the pace of the move under way, whether it walks, its goal.
    pace: Pace,
    walking: bool,
    dest: Option<Vec3>,
    /// Attack `OnBegin` zeroed the swing timer.
    swing_reset: bool,
    /// Another controller's `SetState("RepositionForAttack")` with this enemy
    /// (the synergy super attack's end, synergy.rs), run at the next update.
    reposition_on: Option<Entity>,
}

impl MeleeAi {
    pub fn new(pursuit_ms: i32, random_reposition: i32) -> Self {
        // `ControllerMonster::Load`: `PursuitTime` 10000, below 1000: 8000.
        let ms = if pursuit_ms < 1000 { 8000 } else { pursuit_ms };
        Self {
            pursuit_ms: ms as f32 / 1000.0,
            random_reposition,
            ..Default::default()
        }
    }

    /// Whether the monster is swinging or casting right now; the end of
    /// the attack the Attack state made is its `HandleEvent` "End".
    pub fn note_swing(&mut self, active: bool) {
        if active {
            self.swinging = true;
            if self.st == St::Attack {
                self.attacked = true;
            }
        } else if std::mem::take(&mut self.swinging) && self.st == St::Attack && self.attacked {
            self.ended = true;
        }
    }

    /// The fight states have an enemy.
    pub fn engaged(&self) -> bool {
        self.enemy.is_some()
    }

    /// The state data's enemy.
    pub fn enemy(&self) -> Option<Entity> {
        self.enemy
    }

    pub fn in_attack(&self) -> bool {
        self.st == St::Attack
    }

    /// The Attack state has not made its attack yet (`ControllerMonsterStateAttack.attackPending`): the attack a skill
    /// user starts (`monster_skills::choose_skills`) is due. Once an attack was made the state waits for its
    /// "End", and the end re-chooses and re-checks `CloseEnoughToUseSkill` ([`Self::update`]: in reach "Attack"
    /// again, else "Pursue"): the one source of "the attack ended and the target is still in reach", so a skill
    /// never fires again off an ended attack at a target that walked away.
    pub fn may_attack(&self) -> bool {
        self.st == St::Attack && !self.attacked
    }

    /// The Attack state's `ProjectileCollisionCallback` ([`ProjectileCollided`]).
    pub fn projectile_collided(&mut self, chance: i32, can_move: bool) {
        if self.st == St::Attack {
            self.roll = gd_game_core::rng::i32(0..100) < chance && can_move;
        }
    }

    /// `SetState("RepositionForAttack")` with `enemy` as the state data's enemy
    /// from outside the fight states (`ControllerMonsterSynergyStateSuperAttack`'s
    /// "End", synergy.rs): its `OnBegin` runs at the next [`Self::update`].
    /// The state is left at once (no Attack state to use a skill before then).
    /// PORT: an enemy other than the frame's then is dropped (the engine walks
    /// to the old one until `EnemyFound` re-targets: "Attack" / "Pursue").
    pub fn reposition(&mut self, enemy: Entity) {
        self.reposition_on = Some(enemy);
        self.st = St::Reposition;
        self.pathed = false;
    }

    /// `Character::CrowdAgentError`: the crowd failed the move (the path is
    /// cleared by the caller); the state's `PathFailed` runs at the next
    /// [`Self::update`] when a move of this state was under way.
    pub fn note_path_failed(&mut self) {
        self.failed = self.pathed;
    }

    /// The pace of the move under way.
    pub fn pace(&self) -> Pace {
        self.pace
    }

    /// The skill the state's move carries (its state data's skill; the Pursue's `MoveTo` passes it).
    pub fn key(&self) -> &str {
        &self.key
    }

    /// `Attack::OnBegin` zeroed the swing timer since the last call.
    pub fn take_swing_reset(&mut self) -> bool {
        std::mem::take(&mut self.swing_reset)
    }

    /// The fight is over (enemy gone or invalid, "Return"): the slots of the
    /// enemy are released (`SetState` with another enemy) and the states
    /// start afresh.
    pub fn leave(&mut self, slots: &Slots, me: Entity) {
        if let Some(e) = self.enemy.take() {
            slots.release_all(e, me);
        }
        self.st = St::Idle;
        self.pathed = false;
        self.failed = false;
        self.walking = false;
        self.pace = Pace::Run;
        self.key.clear();
    }

    /// `Character::GetMoveToPoint` for an enemy that is a character; None is
    /// a point without a region.
    fn move_point(&self, f: &Frame, skill: bool) -> Option<Vec3> {
        let (foe, fpos, frad) = f.foe;
        if skill {
            if f.app.melee {
                return f.slots.attack(foe, f.me, f.app.range, f.nav);
            }
            return point_away(
                f.nav,
                &mut f.scratch.borrow_mut(),
                f.pos,
                fpos,
                f.app.range + frad,
            );
        }
        point_away(
            f.nav,
            &mut f.scratch.borrow_mut(),
            f.pos,
            fpos,
            f.radius + frad,
        )
    }

    /// `ControllerMonsterStatePursue::OnUpdate`'s second test, `ControllerAIStateT::MaxSkillDistance`
    /// (`Skill::GetMaxSkillDistance`) with the Pursue goal: the monster has run
    /// farther from where the charge started than the goal is plus
    /// `maxDistanceBuffer`. (Ground distances, as the charge itself.)
    fn charge_far_enough(&self, f: &Frame) -> bool {
        let (Some((start, buffer)), Some(goal)) = (f.app.charge_limit, self.dest) else {
            return false;
        };
        let ground = |a: Vec3| Vec2::new(a.x - start.x, a.z - start.z).length();
        ground(f.pos) > ground(goal) + buffer
    }

    /// `CloseEnoughToUseSkill`.
    fn close(&self, f: &Frame) -> bool {
        Vec2::new(f.foe.1.x - f.pos.x, f.foe.1.z - f.pos.z).length() <= f.app.reach
    }

    /// `ControllerAI::MoveTo` / `Character::CanMoveTo`: the path, or none.
    fn move_to(&mut self, f: &Frame, to: Vec3, path: &mut Vec<Vec3>) -> bool {
        match f.nav.0.find_path(
            &mut f.scratch.borrow_mut(),
            f.pos.to_array(),
            to.to_array(),
            crate::nav::PATH_BUDGET,
        ) {
            Some(p) => {
                *path = p.into_iter().skip(1).map(Vec3::from).collect();
                self.pathed = true;
                true
            }
            None => {
                path.clear();
                self.pathed = false;
                false
            }
        }
    }

    fn release_enemy_slot(&self, f: &Frame) {
        f.slots.release_attack(f.foe.0, f.me);
    }

    /// `ControllerAI::SetState` onto `st` with the enemy and skill as state
    /// data, and the state's `OnBegin`.
    fn set(&mut self, st: St, f: &Frame, path: &mut Vec<Vec3>) {
        if let Some(old) = self.enemy
            && old != f.foe.0
        {
            f.slots.release_all(old, f.me);
        }
        self.enemy = Some(f.foe.0);
        self.key = f.app.key.clone();
        self.st = st;
        self.pathed = false;
        self.failed = false;
        self.walking = false;
        self.pace = Pace::Run;
        match st {
            St::Idle => path.clear(),
            St::Pursue => self.begin_pursue(f, path),
            St::Navigate => self.begin_navigate(f, path),
            St::Wait => {
                self.timer = 0.333;
                self.roaming = false;
                self.roam_t = 0.0;
                self.center = None;
                path.clear();
                if !self.emote_or_roam(f, path) {
                    self.set(St::Idle, f, path);
                }
            }
            St::Attack => {
                path.clear();
                self.attacked = false;
                self.ended = false;
                self.roll = false;
                self.timer = 10.0;
                // `Attack::OnBegin`.
                if f.app.mv.resets_swing {
                    self.swing_reset = true;
                }
            }
            St::Reposition => {
                self.timer = 2.5;
                self.repick = 0.2;
                match self.move_point(f, false) {
                    Some(p) => {
                        if !self.move_to(f, p, path) {
                            self.reposition_failed(f, path);
                        }
                    }
                    // No enemy to run to: `IdleAction`.
                    None => path.clear(),
                }
            }
        }
    }

    fn begin_pursue(&mut self, f: &Frame, path: &mut Vec<Vec3>) {
        // The stationary Pursue `OnBegin`: in reach
        // "Attack", else "Idle"; it never moves.
        if f.stationary {
            path.clear();
            let next = if self.close(f) { St::Attack } else { St::Idle };
            return self.set(next, f, path);
        }
        if !f.can_move {
            return self.set(St::Idle, f, path);
        }
        let Some(point) = self.move_point(f, true) else {
            return self.set(St::Wait, f, path);
        };
        if self.close(f) || f.app.mv.jump {
            return self.set(St::Attack, f, path);
        }
        if point.distance_squared(f.pos) > 0.000_625 {
            if self.move_to(f, point, path) {
                // The pace (`MoveTo`'s animation and speed).
                if f.walk.distance > 0.0 && f.pos.distance(point) <= f.walk.distance {
                    self.pace = Pace::Walk;
                    self.walking = true;
                } else if f.app.mv.charge {
                    self.pace = Pace::Charge;
                }
                self.dest = Some(point);
                debug!(
                    "monster {:?} pursues {:?} to {:.1},{:.1} (melee slot: {}, {:?})",
                    f.me, f.foe.0, point.x, point.z, f.app.melee, self.pace
                );
                self.timer = self.pursuit_ms;
                self.repick = 0.2;
            } else {
                self.set(St::Navigate, f, path);
            }
        } else {
            // "At attack point but not close enough to use skill!": Idle.
            self.set(St::Idle, f, path);
        }
    }

    /// `ControllerMonsterStateNavigateObstacle::OnBegin`.
    fn begin_navigate(&mut self, f: &Frame, path: &mut Vec<Vec3>) {
        let Some(point) = self.move_point(f, true) else {
            return self.set(St::Idle, f, path);
        };
        self.pace = Pace::Unit;
        if point.distance_squared(f.pos) > 0.000_625 && self.move_to(f, point, path) {
            return;
        }
        // Seven tries at `radius + 1` m around the point; `i / 7` is an
        // integer division, so each is the same +x shift.
        if !self.move_to(f, point + Vec3::new(f.radius + 1.0, 0.0, 0.0), path) {
            path.clear();
            self.set(St::Idle, f, path);
        }
    }

    /// The state's `PathFailed` (module docs).
    fn path_failed(&mut self, f: &Frame, path: &mut Vec<Vec3>) {
        match self.st {
            // `ControllerMonsterStatePursue::PathFailed`.
            St::Pursue => {
                if f.app.mv.shortcut {
                    return self.set(St::Attack, f, path);
                }
                self.release_enemy_slot(f);
                self.fails += 1;
                let next = if self.fails < 5 {
                    St::Reposition
                } else {
                    St::Idle
                };
                self.set(next, f, path);
            }
            St::Reposition => self.reposition_failed(f, path),
            // `ControllerMonsterStateWaitToAttack::PathFailed`.
            St::Wait => {
                if !self.emote_or_roam(f, path) {
                    self.set(St::Idle, f, path);
                }
            }
            St::Idle | St::Navigate | St::Attack => {}
        }
    }

    /// `ControllerMonsterStateRepositionForAttack::PathFailed`.
    fn reposition_failed(&mut self, f: &Frame, path: &mut Vec<Vec3>) {
        self.fails += 1;
        if self.fails < 5 {
            // `ControllerMonster::FindRandomPoint`: 3 to 6 m away, more than 3 m from here.
            match crate::patrol::pick_spot(
                f.pos,
                f.pos,
                3.0,
                6.0,
                3.0,
                f.view_y,
                f.nav,
                &mut f.scratch.borrow_mut(),
            ) {
                Some(p) => {
                    *path = p;
                    self.pathed = true;
                }
                None => {
                    path.clear();
                    self.ret = true;
                }
            }
        } else {
            self.release_enemy_slot(f);
            self.set(St::Wait, f, path);
        }
    }

    /// `ControllerMonsterStateWaitToAttack::StartRoaming`.
    fn start_roaming(&mut self, f: &Frame, path: &mut Vec<Vec3>) -> bool {
        let fpos = f.foe.1;
        let center = match self.center {
            Some(c) => c,
            None => {
                let away = f.pos - fpos;
                self.radius = away.length().clamp(4.0, 10.0) * 0.5;
                let c = fpos + away.normalize_or_zero() * self.radius;
                self.center = Some(c);
                c
            }
        };
        for _ in 0..3 {
            if let Some(p) = crate::patrol::pick_spot(
                f.pos,
                center,
                self.radius,
                self.radius,
                0.0,
                f.view_y,
                f.nav,
                &mut f.scratch.borrow_mut(),
            ) {
                *path = p;
                self.pathed = true;
                return true;
            }
        }
        false
    }

    /// `ControllerMonsterStateWaitToAttack::EmoteOrRoam`.
    fn emote_or_roam(&mut self, f: &Frame, path: &mut Vec<Vec3>) -> bool {
        if self.roam_t > 0.0 {
            if !self.roaming {
                // `IdleAction`.
                path.clear();
            }
            return true;
        }
        self.roam_t = 1.0;
        self.roaming = self.start_roaming(f, path);
        self.roaming
    }

    /// One `OnUpdate` of the state the monster is in (with the path events
    /// and `LostSlot` delivered first); `path` is what is left of its move.
    pub fn update(&mut self, f: &Frame, path: &mut Vec<Vec3>) -> Act {
        self.ret = false;
        self.fails = 0;
        // `LostSlot` reaches the state the monster is in; only Pursue and
        // Attack react (the others have the empty base handler).
        let lost = f.slots.take_lost(f.me);
        if std::mem::take(&mut self.reposition_on) == Some(f.foe.0) {
            self.set(St::Reposition, f, path);
            return self.act(path);
        }
        // `DefaultEnemyFoundResponse`: a monster not in these states (or
        // with another enemy) pursues.
        if self.st == St::Idle || self.enemy != Some(f.foe.0) {
            // `SetState` leaves DefendLeader / FollowLeader (the leader is
            // the state data's second id): the defence slot is released.
            if self.st == St::Idle
                && let Some(l) = f.leader
            {
                f.slots.release_defense(l, f.me);
            }
            self.set(St::Pursue, f, path);
            return self.act(path);
        }
        if std::mem::take(&mut self.failed) {
            self.pathed = false;
            path.clear();
            self.path_failed(f, path);
            return self.act(path);
        }
        let ended_path = self.pathed && path.is_empty();
        if ended_path {
            self.pathed = false;
        }
        match self.st {
            St::Idle => {}
            St::Pursue => {
                if lost {
                    self.relook = true;
                }
                if ended_path {
                    // `EndOfPathReached`.
                    if self.close(f) || f.app.mv.shortcut {
                        self.set(St::Attack, f, path);
                    } else {
                        self.release_enemy_slot(f);
                        self.set(St::Wait, f, path);
                    }
                } else if std::mem::take(&mut self.relook) {
                    // `OnUpdate` after `LostSlot`: the monster only checks
                    // that a move point is still to be had.
                    if self.move_point(f, true).is_none() {
                        self.set(St::Wait, f, path);
                    }
                } else {
                    self.timer -= f.dt;
                    self.repick -= f.dt;
                    if self.timer < 0.0 {
                        self.ret = true;
                    } else if self.repick < 0.0 && f.app.key != self.key {
                        self.set(St::Pursue, f, path);
                    } else {
                        if self.repick < 0.0 {
                            self.repick = 0.2;
                        }
                        if self.close(f) || self.charge_far_enough(f) {
                            self.set(St::Attack, f, path);
                        } else if f.walk.distance > 0.0
                            && self.walking
                            && let Some(d) = self.dest
                            && f.pos.distance(d) > f.walk.distance
                        {
                            // A walker farther from its goal than
                            // `walkDistance` runs on (unit speed).
                            self.walking = false;
                            self.pace = Pace::Unit;
                        }
                    }
                }
            }
            St::Navigate => {
                // `ControllerMonsterStateNavigateObstacle::OnUpdate`.
                if self.close(f) {
                    self.set(St::Attack, f, path);
                } else {
                    self.set(St::Pursue, f, path);
                }
            }
            St::Wait => {
                self.timer -= f.dt;
                if self.timer < 0.0 {
                    if self.move_point(f, true).is_some() {
                        self.set(St::Pursue, f, path);
                        return self.act(path);
                    } else if gd_game_core::rng::u32(..100) < 50 {
                        self.set(St::Reposition, f, path);
                        return self.act(path);
                    }
                    self.timer = 0.333;
                }
                if self.roam_t > 0.0 {
                    self.roam_t -= f.dt;
                }
                if !self.emote_or_roam(f, path) {
                    self.set(St::Idle, f, path);
                }
            }
            St::Attack => {
                if lost {
                    self.set(St::Pursue, f, path);
                } else if !self.attacked {
                    if f.swing_ready && f.app.plain {
                        self.attacked = true;
                        return Act::Swing;
                    }
                } else if self.ended {
                    self.ended = false;
                    if self.close(f) {
                        let again = self.roll || {
                            self.roll = gd_game_core::rng::i32(0..100) < self.random_reposition
                                && f.can_move;
                            self.roll
                        };
                        if again && f.app.key == self.key {
                            self.set(St::Reposition, f, path);
                        } else {
                            self.set(St::Attack, f, path);
                        }
                    } else {
                        self.set(St::Pursue, f, path);
                    }
                } else {
                    self.timer -= f.dt;
                    if self.timer < 0.0 {
                        self.ret = true;
                    }
                }
            }
            St::Reposition => {
                if path.is_empty() {
                    // Not running: `EndOfPathReached`.
                    if self.close(f) {
                        self.set(St::Attack, f, path);
                    } else {
                        self.set(St::Pursue, f, path);
                    }
                } else if self.timer > 0.0 {
                    self.timer -= f.dt;
                } else if self.close(f) {
                    self.set(St::Attack, f, path);
                }
            }
        }
        self.act(path)
    }

    fn act(&self, path: &[Vec3]) -> Act {
        if self.ret {
            Act::Return
        } else if path.is_empty() || matches!(self.st, St::Attack | St::Idle) {
            Act::Stand
        } else {
            Act::Run
        }
    }
}
