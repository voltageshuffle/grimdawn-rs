//! Monsters noticing projectiles: the AI dodge and `ProjectileAnger`.
//!
//! Game.dll:
//! * `ProjectileBase::UpdateSelf`: the notify timer (0 at launch) runs down and, once below 0, is set
//!   to 200 ms and `ProjectileBase::NotifyMonsters` runs: every foe of the owner within
//!   `notificationRadius` (default 5) gets `Character::ProjectileNotification`.
//! * `ControllerMonster::ProjectileNotification`: while the dodge timer runs, it only resets it to
//!   `DodgeDelay` (so a stream of projectiles keeps it from dodging). Else the state's handler runs,
//!   the owner gets `ProjectileAnger` (`AngerManager::AddAnger`, × anger multiplier) and the timer is
//!   set to `DodgeDelay` (ms, default 5000).
//! * Idle, Patrol, Roam, Wander, FollowLeader, DefendLeader, Pursue, WaitToAttack, Attack,
//!   RepositionForAttack and Return use `DefaultProjectileNotificationResponse`: `rand() % 100 <
//!   DodgeChance`, the projectile at least radius + `MinDodgeDistance` away (3D), and a dodge point
//!   found (`ControllerMonster::FindRandomPoint`: five tries at `DodgeDistance` (default 4), on the
//!   floor, |Δy| < `MaxYViewDistance`, more than `DodgeDistance` − 0.1 away, reachable): "DodgeAttack"
//!   at once, cutting a swing or cast short. Other states have the empty handler.
//! * `ControllerMonsterStateDodgeAttack`: runs to the point (unreachable: "Return"), ignoring every
//!   event; `EndOfPathReached` / `PathFailed`: "Return".
//! * Player controller states have no handler, so of the player's side only pets (a
//!   `ControllerMonster`) react. The foe test is the skill victim rule (`combat::Side::foe`).
//!
//! PORT: a pet does not release its attack slot when it dodges and "Return" is its idle (nor does a
//! pet dodge mid-swing); the sphere test is centre to centre.
use crate::combat::{MonsterAi, QuestMove, Side, SideRef};
use crate::nav::Nav;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::character::{CharState, Character};
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;

/// The controller's dodge fields (`ControllerMonster::Load`) and its dodge timer and state.
#[derive(Clone, Debug, Default)]
pub struct Dodge {
    /// `DodgeChance` (%, int).
    pub chance: i32,
    /// `DodgeDelay` (s).
    pub delay: f32,
    /// `DodgeDistance`.
    pub distance: f32,
    pub min: f32,
    /// `ProjectileAnger` (default 20).
    pub anger: f32,
    /// `ControllerMonster.dodgeTimer` (s).
    pub timer: f32,
    /// A dodge picked this frame: the path to its point.
    pub start: Option<Vec<Vec3>>,
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

/// A projectile's notify timer (s; 0 notifies at once).
#[derive(Component)]
pub struct NotifyTimer(f32);

/// `ProjectileBase::UpdateSelf`'s notification and `ControllerMonster::ProjectileNotification`.
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
            Has<crate::combat::Allied>,
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
            Option<&crate::combat::JoinedPack>,
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
        // `AngerManager::AddAnger` scales by the owner's `angerMultiplier` (0 without `causesAnger`);
        // the player: 1.
        let mult = monsters
            .get(owner)
            .map(|m| {
                let t = m.2.as_target;
                if t.causes_anger {
                    t.anger_multiplier
                } else {
                    0.0
                }
            })
            .or_else(|_| {
                pets.get(owner).map(|p| {
                    if p.2.causes_anger {
                        p.2.anger_multiplier
                    } else {
                        0.0
                    }
                })
            })
            .unwrap_or(1.0);
        // A live monster's or pet's side, else the one the projectile kept.
        let side = monsters
            .get(owner)
            .ok()
            .map(|m| Side::Monster {
                pack: m.2.faction.clone(),
                allied: m.7,
            })
            .or_else(|| {
                pets.get(owner).ok().map(|p| {
                    p.5.map_or(Side::Player, |j| Side::Monster {
                        pack: j.pack.clone(),
                        allied: j.allied,
                    })
                })
            })
            .or_else(|| p.side.clone())
            .unwrap_or_else(|| Side::from_hostile(hostile));
        for (e, tf, mut pet, cb, ch, joined) in &mut pets {
            let victim = joined.map_or(SideRef::Player, |j| SideRef::Monster {
                pack: &j.pack,
                allied: j.allied,
            });
            if e != owner && tf.translation.distance(at) <= radius && side.foe(victim) {
                pet.projectile_notified(
                    e,
                    (tf.translation, cb.radius, at),
                    (owner, ch.state, mult),
                    &nav,
                    &mut scratch,
                );
            }
        }
        for (e, tf, mut ai, cb, ch, stunned, quest, allied) in &mut monsters {
            let me = tf.translation;
            let victim = SideRef::Monster {
                pack: &ai.faction,
                allied,
            };
            if e == owner || me.distance(at) > radius || !side.foe(victim) {
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
