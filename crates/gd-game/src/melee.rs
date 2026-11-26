//! The monster fight states that bring a monster to its enemy and the `SlotManager` plumbing they, the
//! followers (patrol.rs) and the pets share.
//!
//! # Slots
//!
//! Every living character carries a [`MeleeSlots`] component: its attack and defence `SlotManager`
//! (`gd_sim::slots`, `Character::RequestAttackSlot` / `RequestDefenseSlot`), sized by `numAttackSlots` /
//! `numDefenseSlots` (default 4, `Character::Load`), plus its body (position, radius), synced once per
//! frame by [`sync_slots`]. A character that dies loses the component, so its slots elsewhere are free
//! (`CleanupRing`'s `IsAlive`). Systems reach the components through [`Slots`].
//!
//! # The states
//!
//! `ControllerMonsterState{Pursue, WaitToAttack, Attack, RepositionForAttack}`, each with the enemy and
//! the chosen skill as state data; [`MeleeAi`] is their state machine. A transition runs the new state's
//! `OnBegin` at once (`ControllerAI::SetState`), which also releases the monster's slots of the old enemy
//! when the enemy changes.
//!
//! * `GetMoveToPoint` (`Character::GetMoveToPoint`, a character enemy): with a Melee
//!   `Skill::GetRangeProfile`, a slot from the enemy's attack manager (spacing = `Skill::GetRange` + both
//!   radii); none free: no point. Any other skill: the point on the path `Skill::GetRange` + the enemy's
//!   radius short of it (`CharacterMovementManager::GetPointAwayFromGoal`, [`point_away`]); without a skill
//!   (RepositionForAttack): the same at both radii.
//! * Pursue `OnBegin`: no `CanMove`: Idle (the stationary classes' own Pursue: in reach "Attack", else
//!   "Idle"). No move point: "WaitToAttack". `CloseEnoughToUseSkill` already, or a skill of move type 3 / 5
//!   (`Skill_AttackRadiusLeap`, `...Disengage`): "Attack". Else a point more than 0.025 m away and
//!   reachable (`CanMoveTo`): go there, `PursuitTime` ms to live and the skill re-chosen every 200 ms;
//!   unreachable: "NavigateObstacle". The pace: with a `walkDistance` > 0 and the point within it the
//!   monster walks (`walkSpeed` × its run speed, the Walk animation unless `walkUsesRun`); else it runs, at
//!   unit speed for a move type 2 skill (`Skill_AttackPathCharge`) and at its run speed otherwise; a walker
//!   later farther than `walkDistance` runs on. The move carries the skill (`MoveToAction::Execute` →
//!   `WarmUpStart`): a weapon charge warms up on it (`crate::casting::begin_pursuit_charge`).
//!   `OnUpdate`: `PursuitTime` out: "Return"; a different re-chosen skill (only while
//!   `Skill::CanRechooseInPursuit`, [`SkillMove::rechooses`]): "Pursue" again; close enough, or past the
//!   skill's `Skill::GetMaxSkillDistance` (only `Skill_AttackWeaponCharge`, `...Blink`,
//!   `Skill_AttackPathCharge`: farther from the last charge start than the goal plus a non-zero
//!   `maxDistanceBuffer`): "Attack". `EndOfPathReached`: close enough, or `Skill::AttacksOnArrival`
//!   (`Skill_Evade`, `Skill_AttackRadiusDisengage`, `...Leap`, `Skill_AttackPathCharge`,
//!   `Skill_AttackWeaponCharge`, `...Blink`): "Attack", else the slot is released and "WaitToAttack".
//!   `LostSlot` asks for the move point again at the next update. `PathFailed`
//!   ([`MeleeAi::note_path_failed`]): `AttacksOnArrival`: "Attack"; else the slot is released and,
//!   counting the failures since the last `OnUpdate`, "RepositionForAttack" below 5, else "Idle".
//! * NavigateObstacle `OnBegin` / `OnUpdate`: no move point: Idle. A reachable point more than 0.025 m away
//!   is run to at unit speed; else the point shifted by `radius + 1` m along +x (its seven tries compute
//!   `i / 7` in integer arithmetic, so all are the same): reachable, run there; none: Idle. `OnUpdate`:
//!   close enough "Attack", else "Pursue" again at once.
//! * WaitToAttack `OnBegin` / `OnUpdate`: every 333 ms asks for the move point again: found "Pursue", else
//!   50 % of the time "RepositionForAttack". Meanwhile it `EmoteOrRoam`s: every 1000 ms it runs to a
//!   random spot at half the distance (4..10 m, halved) to the enemy on the circle around the point that
//!   far from the enemy toward itself (`StartRoaming`); when it cannot roam it stands.
//! * Attack `OnBegin` / `OnUpdate`: stands; once the swing timer has run out it attacks
//!   (`AttackEnemyOrReturn`: wherever the enemy now is, `UseSkill` has no range test); when the attack has
//!   ended it chooses again: close enough: with `randomRepositionChance` % (needs `CanMove`; or the flag a
//!   projectile's world contact set, [`ProjectileCollided`]) and the same skill "RepositionForAttack", else
//!   "Attack"; not close enough: "Pursue". `LostSlot`: "Pursue". `OnBegin` also zeroes the swing timer for
//!   a move type 2 skill or a `Skill_AttackWeaponCharge` (`...Blink`).
//! * RepositionForAttack `OnBegin`: runs to the move point without a skill. `OnUpdate`: a monster that is
//!   not running ends the path (close enough "Attack", else "Pursue"); while it runs the 2500 ms timer
//!   counts down, then close enough: "Attack". `PathFailed`: a random spot 3..6 m away to run to, none:
//!   "Return"; from the 5th failure since the last `OnUpdate` the slot is released ("WaitToAttack").
//! * WaitToAttack `PathFailed`: `EmoteOrRoam`, which cannot: "Idle". The other fight states' `PathFailed`
//!   is empty.
//!
//! `LostSlot`: only Pursue, Attack and FollowLeader (patrol.rs) react; elsewhere a lost slot is dropped
//! ([`Slots::take_lost`] is read every update).
//!
//! Pets (`ControllerPet`) run these states too.
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

/// A projectile stopped by the world (`ProjectileBase::CollisionCallback` →
/// `Character::ProjectileCollisionCallback` → the owner's controller state).
#[derive(Message, Clone, Copy, Debug)]
pub struct ProjectileCollided {
    pub attacker: Entity,
}

/// `ControllerMonsterStateAttack::ProjectileCollisionCallback`: in the Attack state the reposition flag
/// becomes `rand() % 100 < RepositionChance && CanMove`; other states ignore it.
/// `ProjectileResultCallback` clears the flag for a result on the entity it names. Pets report too
/// ([`crate::pets::Pet::projectile_collided`]).
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

const DEFAULT_SLOTS: usize = 4;

/// A living character's slot managers, its body this frame, and whether it was handed `LostSlot` since
/// it last looked.
#[derive(Component)]
pub struct MeleeSlots {
    attack: SlotManager,
    defense: SlotManager,
    pos: Vec3,
    radius: f32,
    lost: bool,
}

pub type SlotQuery<'w, 's> = Query<'w, 's, &'static mut MeleeSlots>;

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

/// The characters' slot managers for one system run: `Slots::new(&mut slot_query, &scratch)` with the
/// system's `Local<RefCell<PathScratch>>`.
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

    /// `Character::RequestAttackSlot`: a spot around `owner` for `req`, `range` + both radii from it.
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

    /// `ControllerAI::SetState` releasing the slots of the old enemy.
    pub fn release_all(&self, owner: Entity, req: Entity) {
        if let Some(s) = self.store.borrow_mut().slots_mut(owner) {
            s.attack.release(req.to_bits());
            s.defense.release(req.to_bits());
        }
    }

    pub fn take_lost(&self, e: Entity) -> bool {
        self.store
            .borrow_mut()
            .slots_mut(e)
            .is_some_and(|s| std::mem::take(&mut s.lost))
    }
}

/// Gives new living characters their managers (`Character::Load`), syncs the bodies, and drops the
/// managers of the dead.
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

/// `CharacterMovementManager::GetPointAwayFromGoal`: the path to `goal` walked back from its end until
/// `dist` from the goal. None: no path.
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum St {
    /// Idle: the next update finds the enemy and pursues.
    #[default]
    Idle,
    Pursue,
    Navigate,
    Wait,
    Attack,
    Reposition,
}

/// `ControllerAI::MoveTo`'s animation and speed arguments (`ControllerMonsterStatePursue::OnBegin`).
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

/// What the states ask of a skill's class: `Skill::GetMoveType` (1; `Skill_AttackPathCharge` 2,
/// `Skill_AttackRadiusLeap` 3, `...Disengage` 5), `Skill::AttacksOnArrival` / `CanRechooseInPursuit`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SkillMove {
    pub charge: bool,
    pub jump: bool,
    /// `Skill::AttacksOnArrival`: the end of the Pursue path starts the skill.
    pub shortcut: bool,
    /// `Attack::OnBegin` zeroes the swing timer.
    pub resets_swing: bool,
    /// `Skill::CanRechooseInPursuit`: Pursue chooses again every 200 ms.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Run,
    Stand,
    /// The plain attack (`AttackEnemyOrReturn`; a chosen skill is started by `monster_skills`).
    Swing,
    /// The "Return" state.
    Return,
}

/// The chosen skill as the approach needs it.
#[derive(Clone, Debug, Default)]
pub struct Approach {
    /// The state data's skill id ("" without one). An `Arc` so the per-frame approach rebuild and the
    /// state's copy share one allocation; compared by value (different `Arc`s can name the same skill).
    pub key: std::sync::Arc<str>,
    /// `CloseEnoughToUseSkill`: centre distance.
    pub reach: f32,
    /// `Skill::GetRange`.
    pub range: f32,
    /// `Skill::GetRangeProfile` is Melee: the enemy's attack slots are used.
    pub melee: bool,
    pub plain: bool,
    pub mv: SkillMove,
    /// `Skill::GetMaxSkillDistance` (weapon charge, blink, path charge): where the last charge started
    /// and `maxDistanceBuffer`; none otherwise, for a zero buffer or before a charge.
    pub charge_limit: Option<(Vec3, f32)>,
}

pub struct Frame<'a, 's> {
    pub me: Entity,
    pub pos: Vec3,
    pub radius: f32,
    pub dt: f32,
    pub foe: (Entity, Vec3, f32),
    pub app: &'a Approach,
    pub can_move: bool,
    /// `ControllerStationaryMonster` / `ControllerTotem`: its own Pursue state.
    pub stationary: bool,
    pub swing_ready: bool,
    pub view_y: f32,
    pub leader: Option<Entity>,
    pub walk: Walk,
    pub nav: &'a Nav,
    pub slots: &'a Slots<'s>,
    pub scratch: &'a RefCell<gd_sim::nav::PathScratch>,
}

/// The states' data.
#[derive(Clone, Debug, Default)]
pub struct MeleeAi {
    pub st: St,
    enemy: Option<Entity>,
    key: std::sync::Arc<str>,
    pursuit_ms: f32,
    pub random_reposition: i32,
    /// Pursue / Reposition / Wait timer (s) and the 200 ms re-choose timer.
    timer: f32,
    repick: f32,
    /// Pursue: `LostSlot` was sent.
    relook: bool,
    /// A `MoveTo` is under way: its path ending is `EndOfPathReached`.
    pathed: bool,
    /// WaitToAttack: `EmoteOrRoam`'s timer and flag, the roam circle.
    roam_t: f32,
    roaming: bool,
    center: Option<Vec3>,
    radius: f32,
    /// Attack: made / over, and the reposition roll.
    attacked: bool,
    ended: bool,
    swinging: bool,
    roll: bool,
    /// `PathFailed` count since the last `OnUpdate` (`ControllerMonsterStatePursue.pathFailures`).
    fails: u32,
    /// `PathFailed` is due at the next update.
    failed: bool,
    ret: bool,
    pace: Pace,
    walking: bool,
    dest: Option<Vec3>,
    swing_reset: bool,
    /// Another controller's `SetState("RepositionForAttack")` (synergy.rs), run at the next update.
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

    /// Whether the monster is swinging or casting; the end of the Attack state's attack is its "End".
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

    pub fn engaged(&self) -> bool {
        self.enemy.is_some()
    }

    pub fn enemy(&self) -> Option<Entity> {
        self.enemy
    }

    pub fn in_attack(&self) -> bool {
        self.st == St::Attack
    }

    /// `ControllerMonsterStateAttack.attackPending`: a skill user's attack is due. After the attack's "End"
    /// [`Self::update`] re-checks `CloseEnoughToUseSkill`, so a skill never fires off an ended attack at a
    /// target that walked away.
    pub fn may_attack(&self) -> bool {
        self.st == St::Attack && !self.attacked
    }

    /// The Attack state's `ProjectileCollisionCallback`.
    pub fn projectile_collided(&mut self, chance: i32, can_move: bool) {
        if self.st == St::Attack {
            self.roll = gd_game_core::rng::i32(0..100) < chance && can_move;
        }
    }

    /// `SetState("RepositionForAttack")` from outside the fight states
    /// (`ControllerMonsterSynergyStateSuperAttack`'s "End"); its `OnBegin` runs at the next
    /// [`Self::update`]. PORT: an enemy other than the frame's then is dropped (the engine walks to the old
    /// one until `EnemyFound` re-targets).
    pub fn reposition(&mut self, enemy: Entity) {
        self.reposition_on = Some(enemy);
        self.st = St::Reposition;
        self.pathed = false;
    }

    /// `Character::CrowdAgentError`: `PathFailed` runs at the next [`Self::update`] if this state's move
    /// was under way.
    pub fn note_path_failed(&mut self) {
        self.failed = self.pathed;
    }

    pub fn pace(&self) -> Pace {
        self.pace
    }

    /// The skill the state's move carries.
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn take_swing_reset(&mut self) -> bool {
        std::mem::take(&mut self.swing_reset)
    }

    /// The fight is over: the enemy's slots are released and the states start afresh.
    pub fn leave(&mut self, slots: &Slots, me: Entity) {
        if let Some(e) = self.enemy.take() {
            slots.release_all(e, me);
        }
        self.st = St::Idle;
        self.pathed = false;
        self.failed = false;
        self.walking = false;
        self.pace = Pace::Run;
        self.key = "".into();
    }

    /// `Character::GetMoveToPoint`; None is a point without a region.
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

    /// `ControllerAIStateT::MaxSkillDistance`: the monster has run farther (ground distance) from the
    /// charge start than the Pursue goal plus `maxDistanceBuffer`.
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

    /// `ControllerAI::MoveTo` / `Character::CanMoveTo`.
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

    /// `ControllerAI::SetState` onto `st`, and the state's `OnBegin`.
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
                    // `IdleAction`.
                    None => path.clear(),
                }
            }
        }
    }

    fn begin_pursue(&mut self, f: &Frame, path: &mut Vec<Vec3>) {
        // The stationary Pursue `OnBegin`: in reach "Attack", else "Idle".
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
        // Seven tries at `radius + 1` m; `i / 7` in integer arithmetic makes each the same +x shift.
        if !self.move_to(f, point + Vec3::new(f.radius + 1.0, 0.0, 0.0), path) {
            path.clear();
            self.set(St::Idle, f, path);
        }
    }

    fn path_failed(&mut self, f: &Frame, path: &mut Vec<Vec3>) {
        match self.st {
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

    /// One `OnUpdate` of the state the monster is in (path events and `LostSlot` delivered first); `path`
    /// is what is left of its move.
    pub fn update(&mut self, f: &Frame, path: &mut Vec<Vec3>) -> Act {
        self.ret = false;
        self.fails = 0;
        // Only Pursue and Attack react to `LostSlot`.
        let lost = f.slots.take_lost(f.me);
        if std::mem::take(&mut self.reposition_on) == Some(f.foe.0) {
            self.set(St::Reposition, f, path);
            return self.act(path);
        }
        // `DefaultEnemyFoundResponse`: a monster not in these states (or with another enemy) pursues.
        if self.st == St::Idle || self.enemy != Some(f.foe.0) {
            // `SetState` leaving DefendLeader / FollowLeader releases the defence slot.
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
                    if self.close(f) || f.app.mv.shortcut {
                        self.set(St::Attack, f, path);
                    } else {
                        self.release_enemy_slot(f);
                        self.set(St::Wait, f, path);
                    }
                } else if std::mem::take(&mut self.relook) {
                    // After `LostSlot` the monster only checks that a move point is still to be had.
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
                            // A walker farther from its goal than `walkDistance` runs on.
                            self.walking = false;
                            self.pace = Pace::Unit;
                        }
                    }
                }
            }
            St::Navigate => {
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
