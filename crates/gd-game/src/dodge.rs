//! Monsters noticing projectiles: the AI dodge and `ProjectileAnger`.
//!
//! Game.dll:
//! * `ProjectileBase::UpdateSelf`: a flying projectile's notify timer
//!   (`ProjectileBase.notifyTimer`, 0 at launch) runs down and, once below 0, is set to 200 ms
//!   (`ProjectileBase.notifyInterval`) and `ProjectileBase::NotifyMonsters` runs: every
//!   foe of the projectile's owner within the record's `notificationRadius`
//!   (default 5, `ProjectileBase::Load`) of it gets
//!   `Character::ProjectileNotification` → its controller's.
//! * `ControllerMonster::ProjectileNotification`: while the
//!   monster's dodge timer (`ControllerMonster.dodgeTimer`, counted down by `ControllerMonster::Update`)
//!   runs, the notification only sets it to `DodgeDelay` again (so a stream of
//!   projectiles keeps it from dodging). Else the current state's
//!   `ProjectileNotification` runs, the owner gets `ProjectileAnger`
//!   (`AngerManager::AddAnger`, × its anger multiplier) and the timer is set
//!   to `DodgeDelay` (ms, default 5000).
//! * The state handler of Idle, Patrol, Roam, Wander, FollowLeader,
//!   DefendLeader, Pursue, WaitToAttack, Attack, RepositionForAttack and Return
//!   is `DefaultProjectileNotificationResponse`: `rand() % 100 <
//!   DodgeChance`, the projectile at least the monster's radius +
//!   `MinDodgeDistance` away (3D), and a dodge point found (`ControllerMonster::FindRandomPoint`,
//!   `patrol::pick_spot`: five tries at `DodgeDistance` (default 4) from the
//!   monster at a random angle, on the floor, |Δy| < `MaxYViewDistance`, more
//!   than `DodgeDistance` − 0.1 away, reachable): "DodgeAttack", at once: a
//!   swing or cast under way is cut short (`combat::monster_ai`). The other
//!   states (Startup / Hidden, AlertBeforePursue, Flee, Stunned, QuestMove,
//!   NavigateObstacle, DodgeAttack itself, Dying) have the empty handler; the
//!   stationary classes have no DodgeAttack state (`combat::ControllerClass`).
//! * `ControllerMonsterStateDodgeAttack` (`OnBegin`): runs to the
//!   point at its run speed (unreachable: "Return"), ignoring every event;
//!   `EndOfPathReached` / `PathFailed`: "Return".
//!
//! * The player's controller states have no handler (`ControllerAI::
//!   ProjectileNotification` forwards to the state's slot, empty
//!   in every `ControllerPlayerState*`), so only pets react to a monster's
//!   projectile: a pet is a `ControllerMonster` (`pets.rs`), same rules and
//!   its controller record's `DodgeChance` (`Pet::projectile_notified`).
//!
//! PORT: a pet does not release its attack slot when it dodges and "Return"
//! is its idle (nor does a pet dodge mid-swing); the sphere test is centre to
//! centre.
use crate::combat::{MonsterAi, QuestMove};
use crate::nav::Nav;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::character::{CharState, Character};
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;

/// The controller's dodge fields (`ControllerMonster::Load`)
/// and its dodge timer and state.
#[derive(Clone, Debug, Default)]
pub struct Dodge {
    /// `DodgeChance` (%, int).
    pub chance: i32,
    /// `DodgeDelay` (s).
    pub delay: f32,
    /// `DodgeDistance`, `MinDodgeDistance`.
    pub distance: f32,
    pub min: f32,
    /// `ProjectileAnger` (default 20).
    pub anger: f32,
    /// The dodge timer (s).
    pub timer: f32,
    /// A dodge picked this frame: the path to its point.
    pub start: Option<Vec<Vec3>>,
    /// In the DodgeAttack state.
    pub running: bool,
}

impl Dodge {
    pub fn from_controller(ctl: Option<&Record>) -> Self {
        let f = |k: &str, d: f32| ctl.and_then(|c| c.f32(k)).unwrap_or(d);
        Self {
            chance: ctl.and_then(|c| c.i32("DodgeChance")).unwrap_or(0),
            delay: ctl.and_then(|c| c.i32("DodgeDelay")).unwrap_or(5000) as f32 / 1000.0,
            distance: f("DodgeDistance", 4.0),
            min: f("MinDodgeDistance", 0.0),
            anger: f("ProjectileAnger", 20.0),
            ..Default::default()
        }
    }
}

/// A projectile's notify timer (s; none yet: 0, it notifies at once).
#[derive(Component)]
pub struct NotifyTimer(f32);

/// `ProjectileBase::UpdateSelf`'s notification and
/// `ControllerMonster::ProjectileNotification` (see the module docs).
#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
pub fn notify_monsters(
    mut commands: Commands,
    time: Res<Time>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    data: Res<GameData>,
    nav: Res<Nav>,
    mut projectiles: Query<(
        Entity,
        &Transform,
        &crate::casting::Projectile,
        Option<&mut NotifyTimer>,
    )>,
    mut monsters: Query<
        (
            Entity,
            &Transform,
            &mut MonsterAi,
            &Combatant,
            &Character,
            Has<crate::movement::Stunned>,
            Has<QuestMove>,
        ),
        Without<Dead>,
    >,
    mut pets: Query<
        (
            Entity,
            &Transform,
            &mut crate::pets::Pet,
            &Combatant,
            &Character,
        ),
        (
            Without<Dead>,
            Without<MonsterAi>,
            Without<crate::movement::Stunned>,
        ),
    >,
) {
    let dt = time.delta_secs();
    for (_, _, mut ai, ..) in &mut monsters {
        if ai.dodge.timer > 0.0 {
            ai.dodge.timer -= dt;
        }
    }
    for (_, _, mut p, ..) in &mut pets {
        if p.dodge_timer > 0.0 {
            p.dodge_timer -= dt;
        }
    }
    for (pe, ptf, p, timer) in &mut projectiles {
        let left = timer.as_ref().map_or(0.0, |t| t.0) - dt;
        let due = left < 0.0;
        let left = if due { 0.2 } else { left };
        match timer {
            Some(mut t) => t.0 = left,
            None => {
                commands.entity(pe).try_insert(NotifyTimer(left));
            }
        }
        let (owner, hostile, record) = p.owner();
        if !due {
            continue;
        }
        let radius = data
            .db
            .get(record)
            .and_then(|r| r.f32("notificationRadius"))
            .unwrap_or(5.0);
        let at = ptf.translation;
        let mult = pets.get(owner).map_or(1.0, |p| p.2.anger_multiplier);
        if hostile {
            // A monster's projectile tells the player's pets (the player's
            // controller states have no handler).
            for (e, tf, mut pet, cb, ch) in &mut pets {
                if e != owner && tf.translation.distance(at) <= radius {
                    pet.projectile_notified(
                        e,
                        (tf.translation, cb.radius, at),
                        (owner, ch.state),
                        &nav,
                        &mut scratch,
                    );
                }
            }
            continue;
        }
        for (e, tf, mut ai, cb, ch, stunned, quest) in &mut monsters {
            let me = tf.translation;
            if e == owner || me.distance(at) > radius {
                continue;
            }
            if ai.dodge.timer > 0.0 {
                ai.dodge.timer = ai.dodge.delay;
                continue;
            }
            let responds = ai.hidden.is_none()
                && ai.startup == crate::combat::SpawnStartup::Done
                && !ai.alerting()
                && ai.class.can_move()
                && !ai.dodge.running
                && ai.flee.run.is_none()
                && !stunned
                && !quest
                && ai.melee.st != crate::melee::St::Navigate
                && ch.state != CharState::Death;
            if responds
                && gd_game_core::rng::i32(0..100) < ai.dodge.chance
                && me.distance(at) >= cb.radius + ai.dodge.min
            {
                let d = ai.dodge.distance;
                if let Some(path) =
                    crate::patrol::pick_spot(me, me, d, d, d - 0.1, ai.view_y(), &nav, &mut scratch)
                {
                    debug!("monster {e} dodges projectile {pe}");
                    ai.dodge.start = Some(path);
                }
            }
            let anger = ai.dodge.anger * mult;
            ai.add_anger(owner, anger);
            ai.dodge.timer = ai.dodge.delay;
        }
    }
}
