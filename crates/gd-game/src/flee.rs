//! The monsters' Flee state (`ControllerMonsterStateFlee`:
//! the constructor, `OnBegin`, `DoFlee`,
//! `PickFleePoint`, `OnUpdate`, `OnEnd`,
//! `EndOfPathReached`).
//!
//! The controller record (`ControllerMonster::Load`):
//! `FleeBehavior` (NeverFlee default / FleeOnLowHealth / FleeOnDamage /
//! FleeWhenEnemyClose), `FleeTarget` (AwayFromEnemy default / TowardsAllies /
//! TowardsHome), `maxFleeCount` (3), `FleeTime` ms (3000), `FleeDelay` ms
//! (2000), `FleeChance` % (100), `fleeDistance`, `ClearAngerWhenFleeing`,
//! `ResetOriginAfterFleeing`, `enemyTooClose`. `ControllerMonster::CanFlee`:
//! fewer flees than `maxFleeCount` so far, the delay since the
//! last one over (`ResetFleeTimer`, counted down in `Update`) and
//! `rand() % 100 < FleeChance`. Nothing in the controller code resets the
//! flee count: it is per lifetime.
//!
//! Triggers (each asks `CanFlee` and enters Flee with the enemy as state
//! data): `DefaultLowHealthResponse` (FleeOnLowHealth, instead
//! of berserk), `DefaultAttackedResponse` and
//! `ControllerMonsterStateAttack::Attacked` (FleeOnDamage),
//! `DefaultClosestEnemyFoundResponse` (FleeWhenEnemyClose),
//! raised by `FindEnemyUpdate`: every 200 ms a monster with
//! `enemyTooClose` > 0 and that behavior asks `CanFlee`, then looks for the
//! closest foe nearer than `enemyTooClose` (+0.1).
//!
//! The state: `OnBegin` counts the flee; `DoFlee` picks the point, goes
//! idle at once when it is the monster's own spot or unreachable, else runs
//! there at the run speed and arms the `FleeTime` timer (`OnUpdate` counts
//! it down: expiry, or reaching the point, ends the state in "Idle", where
//! the monster finds its enemy again through its anger). `OnEnd` re-arms the
//! flee delay and, if it ran, `ClearAngerWhenFleeing` empties the anger list
//! and `ResetOriginAfterFleeing` makes its current spot its origin.
//! `PickFleePoint`: AwayFromEnemy runs `fleeDistance` directly away from the
//! enemy with ±1 m of jitter on each horizontal axis; TowardsAllies runs to
//! the first friend (within `RoamDistance` of the monster's home, other than
//! itself) that lies on the far side of the monster from the enemy and more
//! than 3 m off, and on past it by a random distance between the sum of the
//! two radii and twice that, else flees as AwayFromEnemy; TowardsHome runs
//! to its home.
//!
//! The roar (`OnBegin`): after the flee is counted, `rand() % 100 <
//! fleeAnimChance` (Monster record, default 0) asks `Character::
//! CanPlayAnimation`'s test (the animation set's slot 37, `Flee`, has
//! entries); if so a `PlayAnimationAction` (type 37, played once, raising
//! "End") is queued and the `fleeSound` pak plays, with no sound roll; if
//! the slot is empty nothing plays and the flee goes on. Otherwise `rand() %
//! 100 < fleeSoundChance` plays `fleeSound` alone. While the roar plays
//! (`ControllerMonsterStateFlee.roarPlaying`) `DoFlee` is not called: the state's
//! `HandleEvent` calls it on "End" (so the point is picked, the run speed
//! and the `FleeTime` timer armed when the roar ends) and `OnUpdate` does not
//! count the timer.
//!
//! A flee asked for during a swing or cast starts at once, as the game's
//! `SetState("Flee")` does: the action is cut short (`combat::monster_ai`).
//! The roar is the `CharState::Roar` state and is left when its animation
//! ends or is interrupted. The "AlertBeforePursue" state ignores the
//! triggers (its `Attacked` / `LowHealth` handlers are empty).
//!
//! The fear influence's Scared state (`ControllerMonster::ScareMe`
//! → `ControllerMonsterStateScared`) reuses the flee point
//! pick: [`Flee::scare_point`] is its `PickRunToPoint` — always
//! away from the enemy (no `FleeTarget` variants), `max(fleeDistance, 5)` m
//! in 3D with ±1 m of horizontal jitter, re-picked every 1–2 s and on the
//! path's end / failure; no roar, no flee count. [`crate::combat::cc_states`] drives
//! it while the fear crowd-control entry runs.
use crate::combat::MonsterAi;
use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Behavior {
    Never,
    LowHealth,
    Damage,
    EnemyClose,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    AwayFromEnemy,
    Allies,
    Home,
}

/// The controller's flee settings and the monster's flee bookkeeping.
#[derive(Clone, Debug)]
pub struct Flee {
    pub behavior: Behavior,
    target: Target,
    max_count: i32,
    time: f32,
    delay: f32,
    /// `FleeChance` (%).
    pub chance: u32,
    distance: f32,
    clear_anger: bool,
    reset_origin: bool,
    /// `enemyTooClose`.
    pub too_close: f32,
    count: i32,
    delay_left: f32,
    /// `fleeAnimChance` / `fleeSoundChance` (Monster record, default 0) and
    /// the `fleeSound` pak.
    anim_chance: u32,
    sound_chance: u32,
    pub sound: Option<String>,
    /// The running state.
    pub run: Option<Run>,
    /// A trigger fired: flee from this enemy once free to.
    pub request: Option<Entity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Roar {
    /// No roar (or it ended).
    None,
    /// Rolled, the animation not started yet.
    Pending,
    /// The animation is playing: `DoFlee` waits for "End".
    Playing,
}

#[derive(Clone, Debug)]
pub struct Run {
    /// The flee's enemy (state data).
    pub enemy: Entity,
    pub roar: Roar,
    /// Seconds of `FleeTime` left.
    pub timer: f32,
    /// `DoFlee` found a point to run to (`OnEnd` acts only then).
    pub moved: bool,
}

impl Flee {
    pub fn from_controller(
        ctl: Option<&gd_formats::arz::Record>,
        mon: Option<&gd_formats::arz::Record>,
    ) -> Self {
        let s = |k: &str| ctl.and_then(|c| c.str(k));
        let behavior = match s("FleeBehavior").unwrap_or("NeverFlee") {
            "FleeOnLowHealth" => Behavior::LowHealth,
            "FleeOnDamage" => Behavior::Damage,
            "FleeWhenEnemyClose" => Behavior::EnemyClose,
            _ => Behavior::Never,
        };
        let target = match s("FleeTarget").unwrap_or("AwayFromEnemy") {
            "TowardsAllies" => Target::Allies,
            "TowardsHome" => Target::Home,
            _ => Target::AwayFromEnemy,
        };
        let i = |k: &str, d: i32| ctl.and_then(|c| c.i32(k)).unwrap_or(d);
        let f = |k: &str, d: f32| ctl.and_then(|c| c.f32(k)).unwrap_or(d);
        Self {
            behavior,
            target,
            max_count: i("maxFleeCount", 3),
            time: i("FleeTime", 3000) as f32 / 1000.0,
            delay: i("FleeDelay", 2000) as f32 / 1000.0,
            chance: i("FleeChance", 100).max(0) as u32,
            distance: f("fleeDistance", 5.0),
            clear_anger: ctl.is_some_and(|c| c.bool("ClearAngerWhenFleeing")),
            reset_origin: ctl.is_some_and(|c| c.bool("ResetOriginAfterFleeing")),
            too_close: f("enemyTooClose", 5.0),
            anim_chance: mon
                .and_then(|m| m.i32("fleeAnimChance"))
                .unwrap_or(0)
                .max(0) as u32,
            sound_chance: mon
                .and_then(|m| m.i32("fleeSoundChance"))
                .unwrap_or(0)
                .max(0) as u32,
            sound: mon
                .and_then(|m| m.str("fleeSound"))
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            count: 0,
            delay_left: 0.0,
            run: None,
            request: None,
        }
    }

    pub fn count(&self) -> i32 {
        self.count
    }

    pub fn max(&self) -> i32 {
        self.max_count
    }

    /// `ControllerMonster::Update`: the delay counts down.
    pub fn tick(&mut self, dt: f32) {
        if self.delay_left > 0.0 {
            self.delay_left -= dt;
        }
    }

    /// `ControllerMonster::CanFlee` (rolls `rand() % 100`).
    fn can_flee(&self) -> bool {
        self.count < self.max_count
            && self.delay_left <= 0.0
            && (gd_game_core::rng::u32(..32768) % 100) < self.chance
    }

    /// A trigger of `behavior` fired with `enemy`: enter Flee if it is
    /// allowed (a fleeing monster ignores triggers: the state handles only
    /// "End").
    pub fn trigger(&mut self, behavior: Behavior, enemy: Entity) -> bool {
        if self.behavior != behavior || self.run.is_some() || !self.can_flee() {
            return false;
        }
        self.request = Some(enemy);
        true
    }

    /// `OnBegin`: counts the flee and rolls the roar. `can_play`: the
    /// animation set has a `Flee` slot. Returns whether the `fleeSound`
    /// plays; the roar is `run.roar`.
    pub fn enter(&mut self, enemy: Entity, can_play: bool) -> bool {
        self.count += 1;
        let (roar, sound) = if (gd_game_core::rng::u32(..32768) % 100) < self.anim_chance {
            (can_play, can_play)
        } else {
            (
                false,
                (gd_game_core::rng::u32(..32768) % 100) < self.sound_chance,
            )
        };
        self.run = Some(Run {
            enemy,
            roar: if roar { Roar::Pending } else { Roar::None },
            timer: 0.0,
            moved: false,
        });
        sound && self.sound.is_some()
    }

    /// `DoFlee`'s run: arms the `FleeTime` timer (a point was reached by a
    /// path).
    pub fn arm(&mut self) {
        if let Some(r) = self.run.as_mut() {
            r.timer = self.time;
            r.moved = true;
        }
    }

    /// `OnEnd`; the caller applies the returned (clear anger, reset origin).
    pub fn end(&mut self) -> (bool, bool) {
        self.delay_left = self.delay;
        let moved = self.run.take().is_some_and(|r| r.moved);
        (moved && self.clear_anger, moved && self.reset_origin)
    }

    /// `PickFleePoint`: where to run from `me` away from `enemy`. `home` is
    /// the monster's home, `friends` the other monsters that are its
    /// friends as (position, radius), `my_radius` its own.
    pub fn pick_point(
        &self,
        me: Vec3,
        my_radius: f32,
        enemy: Vec3,
        home: Vec3,
        roam: f32,
        friends: &[(Vec3, f32)],
    ) -> Vec3 {
        let jitter = || gd_game_core::rng::u32(..32768) as f32 * 0.000_030_518_51 * 2.0 - 1.0;
        if self.target == Target::Home {
            return home;
        }
        let away = (me - enemy).with_y(0.0).normalize_or_zero();
        if self.target == Target::Allies {
            for (p, r) in friends {
                if p.distance(home) > roam {
                    continue;
                }
                let to = (*p - me).with_y(0.0);
                if to.dot(away) > 0.0 && to.length() > 3.0 {
                    let lo = my_radius + r;
                    let c = lo + gd_game_core::rng::u32(..32768) as f32 * 0.000_030_518_51 * lo;
                    return me + to + to.normalize_or_zero() * c;
                }
            }
        }
        me + away * self.distance + Vec3::new(jitter(), 0.0, jitter())
    }

    /// `ControllerMonsterStateScared::PickRunToPoint`:
    /// the fear flee's point — `max(fleeDistance, 5)` m directly
    /// away from the enemy in 3D, ±1 m of jitter on each horizontal axis
    /// (the Flee state's pick is horizontal and honors `FleeTarget`; the
    /// Scared state always runs away). The caller snaps it to the navmesh.
    pub fn scare_point(&self, me: Vec3, enemy: Vec3) -> Vec3 {
        let away = (me - enemy).normalize_or_zero();
        let dist = self.distance.max(5.0);
        let jitter = || gd_game_core::rng::u32(..32768) as f32 * 0.000_030_518_51 * 2.0 - 1.0;
        me + away * dist + Vec3::new(jitter(), 0.0, jitter())
    }
}

/// A flee's enemy position or None when it is gone (`PickFleePoint` fails
/// and the state ends at once).
pub fn enemy_pos(foes: &[(Entity, Vec3, f32)], e: Entity) -> Option<Vec3> {
    foes.iter().find(|f| f.0 == e).map(|f| f.1)
}

/// `FindEnemyUpdate` (FleeWhenEnemyClose): the 200 ms check.
pub fn too_close_check(ai: &mut MonsterAi, me: Vec3, foes: &[(Entity, Vec3, f32)]) {
    if ai.flee.too_close <= 0.0 || ai.flee.behavior != Behavior::EnemyClose || ai.flee.run.is_some()
    {
        return;
    }
    if !ai.flee.can_flee() {
        return;
    }
    let near = foes
        .iter()
        .map(|f| (f.0, f.1.distance(me)))
        .filter(|f| f.1 < ai.flee.too_close + 0.1)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((e, _)) = near {
        ai.flee.request = Some(e);
    }
}
