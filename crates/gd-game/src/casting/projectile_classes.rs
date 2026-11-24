//! The `ProjectileFireballLike` subclasses of the expansions. Boomerang, Spark and Inverse steer themselves: each
//! overrides `Load`, `ProjectileGo` and `UpdateSelf` (Game.dll vtables, unexported classes: the RTTI names are
//! registered by the static initialisers that reference the class strings). Each `ProjectileGo` turns gravity off and the physics simulation to 2
//! (`Entity::SetPhysicsSimulation`), sets its speed to `projectileVelocity` × (1 + the speed modifier % / 100) and
//! starts the flight (state 1); each `UpdateSelf` moves the projectile by `SetCoords` while in state 1, then runs
//! `ProjectileFireballLike::UpdateSelf`, which does the hits along the step (as for every projectile: `fly_projectiles`)
//! and stops it (`ProjectileStop`: state 3, an `explodeOnMiss` explosion) once it is
//! `projectileDistance` or more from its launch coordinates (`ProjectileBase.launchCoords`). A stop by the class itself
//! (its end below) makes no hit that frame.
//!
//! - `ProjectileBoomerang` (its vtable: `Load`, `ProjectileGo`,
//!   `UpdateSelf`; ctor). `Load`: `projectileFollow`, `projectileHoverDuration` and
//!   `projectileFollowDuration` (× 1000, int ms), `projectileTravelDistance`. `ProjectileGo` (Ashes of Malmouth
//!   content: `IsExpansion1Loaded`) heads at the target's hit point (else along the launch direction) and keeps the
//!   return point: the owner's centre of mass in the world (`Actor::GetCenterOfMass` × its coords; no owner
//!   character: the launch coordinates). `UpdateSelf`: while the distance flown (speed × dt summed) is under the
//!   travel distance, it flies on along its heading; then it hovers in place for the hover duration, and on the
//!   first hover frame forgets every target it hit (the known-target set, so the way back hits them again); then it
//!   flies straight at the return point and stops within 1 m of it (3D). With `projectileFollow` the return point is
//!   the owner's centre of mass again each frame while he lives, and the way back stops when the follow duration
//!   (counted down on the way back) runs out.
//! - `ProjectileSpark` (its vtable: `Load`, `ProjectileGo`,
//!   `UpdateSelf`, `PhysicsSetup`; ctor). `Load`: `projectileDuration`
//!   and `changeDirectionTime` (× 1000, int ms; the ctor's 3000 / 0 are overwritten, a missing field is 0),
//!   `disableCollision`, `minRotation` / `maxRotation` (degrees). `ProjectileGo` (Forgotten Gods content:
//!   `IsExpansion2Loaded`) heads at the target's hit point, else at the aimed point 1 m up (when it is not the
//!   launch point), else along the launch direction (the aim `fire_casts` already gives). `UpdateSelf`: the duration counts down and its
//!   end stops it; the change timer (0 at launch, so the first frame turns) counts down and at 0 restarts from
//!   `changeDirectionTime` and, when min ≤ max, turns the heading by a uniform draw in [min, max] degrees about +Y
//!   (`RandomUniformLocked::FGenerate`, the engine's stream; turns add up, so the spark wanders); it flies on at
//!   its speed along the heading. `disableCollision` clears the physics body's collision mask: no wall or door
//!   stops it (23 of the 45 stock sparks).
//! - `ProjectileInverse` (its vtable: `Load`, `ProjectileGo`,
//!   `UpdateSelf`; ctor). `Load`: `projectileStartDistance`, `projectileFollow`,
//!   `projectileDuration` (× 1000, int ms; ctor 3000). `ProjectileGo` (`IsExpansion1Loaded`): the target point is
//!   the owner's centre of mass in the world (no owner character: the launch coordinates); the launch direction
//!   made level (y = 0) times max(start distance, 1) from that point is where it appears, facing out along that
//!   direction (a ring of them: the ring around the caster; the launch coordinates, which the `projectileDistance`
//!   stop measures from, stay the caster's). `UpdateSelf`: the duration's end stops it; with `projectileFollow`
//!   the target point is the owner's centre of mass each frame, and a dead or gone owner stops it; within 1 m of
//!   the target point (level distance) it stops; else it flies straight at it (3D).
//! - `ProjectileTerrainFollowing` (`UpdateSelf`): steps along its heading, then
//!   `World::GetNearestFloorHeight` re-seats it at floor + its `actorHeight`, eased (when |Δy| > 1.25×height it
//!   moves 0.75 of the excess); it stops only when the new point is off the nav mesh
//!   (`NavManager::IsPointOnPathMesh`). The terrain is its floor, not a wall (`fly_projectiles` skips the terrain
//!   contact for it).
//! - `ProjectileHoming` (`UpdateSelf`, `ProjectileGo`): each frame it
//!   re-resolves its target, takes `ProjectileBase::GetHitPoint` (the body box centre) and steps straight at it
//!   at its speed; a dead or gone target or the `projectileDuration`'s end stops it; `ProjectileGo` aborts the
//!   launch entirely without a valid target (`fire_casts`).
//! - `ProjectileChain` flies as any fireball; its skill (`Skill_AttackProjectileChain`) collects the spark list
//!   at launch and the impact attacks every entry (`fire`'s `chain_sparks`, [`super::Projectile::chain`]).
//! - `ProjectileFragmenting` (exported: `Load`, `ProjectileGo`, `UpdateSelf`,
//!   `ProjectileStop`, `ProjectileHit`) flies as any fireball (the base's
//!   physics flight, no steering). `Load`: `fragmentInterval` (× 1000, int ms) and `fragmentOnStop`. `ProjectileGo`
//!   (`IsExpansion3Loaded`) starts the interval timer full. `UpdateSelf` (a copy of the base's with one addition):
//!   in flight the timer counts down and at 0 restarts and runs `ProcessFragmentation(0, its coords)`: the skill's
//!   `projectileFragmentsName` set splits where it is, besides the usual split at each target hit (Yurra's chaos
//!   orb: 3-4 grenade fragments every 0.4 s). `ProjectileHit` / `ProjectileStop` split once more with
//!   `fragmentOnStop` (no stock record sets it). [`Fragmenting`].
//!
//! PORT: the owner's centre of mass is `monster_skills::character_center` (the mesh's bind-pose bounding box); a
//! wall or a closed door stops these projectiles as it does the others (`fly_projectiles`).
use super::*;

/// How a self-steering projectile flies (see the module docs).
#[derive(Component, Clone, Debug)]
pub(crate) enum Flight {
    Boomerang {
        speed: f32,
        /// The way out's heading (unit).
        dir: Vec3,
        /// `projectileTravelDistance` and the distance flown so far.
        travel: f32,
        flown: f32,
        /// Hover time left (s) and whether it started (the hit targets forgotten).
        hover: f32,
        hovered: bool,
        follow: bool,
        /// `projectileFollowDuration` left (s), counted on the way back.
        follow_left: f32,
        /// The return point.
        target: Vec3,
    },
    Spark {
        speed: f32,
        heading: Vec3,
        /// Duration left and the change timer (s); `changeDirectionTime`; the turn range (degrees).
        life: f32,
        change: f32,
        every: f32,
        turn: (f32, f32),
        /// `!disableCollision`.
        collides: bool,
    },
    Inverse {
        speed: f32,
        /// The launch direction made level (unit) and `projectileStartDistance`.
        dir: Vec3,
        start_distance: f32,
        life: f32,
        follow: bool,
        target: Vec3,
    },
    /// `ProjectileTerrainFollowing`: the level heading and the height above
    /// the floor it rides (`actorHeight`); `fly_projectiles` re-seats y.
    Terrain { speed: f32, dir: Vec3, height: f32 },
    /// `ProjectileHoming`: the cast's target and the duration left (s).
    Homing {
        speed: f32,
        target: Option<Entity>,
        life: f32,
    },
}

impl Flight {
    /// Whether the world (a wall, a closed door) stops it.
    pub(super) fn collides(&self) -> bool {
        match self {
            Flight::Spark { collides, .. } => *collides,
            _ => true,
        }
    }
}

/// A `ProjectileFragmenting`'s timed splits (see the module docs).
#[derive(Component, Clone, Debug)]
pub(crate) struct Fragmenting {
    /// `fragmentInterval` and the time to the next split (s).
    pub(super) every: f32,
    pub(super) left: f32,
    pub(super) on_stop: bool,
}

pub(super) fn fragmenting_of(prec: &Record) -> Option<Fragmenting> {
    (prec.class == "ProjectileFragmenting").then(|| {
        let every = (prec.f32("fragmentInterval").unwrap_or(0.0) * 1000.0) as i32 as f32 / 1000.0;
        Fragmenting {
            every,
            left: every,
            on_stop: prec.bool("fragmentOnStop"),
        }
    })
}

/// The [`Flight`] of a projectile record of a self-steering class, launched at `vel` from `start`
/// (`target`: the cast's target, a homing projectile's).
pub(super) fn flight_of(
    prec: &Record,
    start: Vec3,
    vel: Vec3,
    target: Option<Entity>,
) -> Option<Flight> {
    let f = |k: &str| prec.f32(k).unwrap_or(0.0);
    // `(int)(x × 1000)` ms, as the loaders keep them.
    let ms = |k: &str| (f(k) * 1000.0) as i32 as f32 / 1000.0;
    match prec.class.as_str() {
        "ProjectileBoomerang" => Some(Flight::Boomerang {
            speed: vel.length(),
            dir: vel.normalize_or(Vec3::Z),
            travel: prec.f32("projectileTravelDistance").unwrap_or(1.0),
            flown: 0.0,
            hover: ms("projectileHoverDuration"),
            hovered: false,
            follow: prec.bool("projectileFollow"),
            follow_left: ms("projectileFollowDuration"),
            // `ProjectileGo` replaces it with the owner's centre of mass ([`set_return_point`]).
            target: start,
        }),
        "ProjectileSpark" => Some(Flight::Spark {
            speed: vel.length(),
            heading: vel.normalize_or(Vec3::Z),
            life: ms("projectileDuration"),
            change: 0.0,
            every: ms("changeDirectionTime"),
            turn: (f("minRotation"), f("maxRotation")),
            collides: !prec.bool("disableCollision"),
        }),
        "ProjectileInverse" => Some(Flight::Inverse {
            speed: vel.length(),
            dir: vel.with_y(0.0).normalize_or(Vec3::Z),
            start_distance: f("projectileStartDistance"),
            life: ms("projectileDuration"),
            follow: prec.bool("projectileFollow"),
            target: start,
        }),
        "ProjectileTerrainFollowing" => Some(Flight::Terrain {
            speed: vel.length(),
            dir: vel.with_y(0.0).normalize_or(Vec3::Z),
            height: f("actorHeight"),
        }),
        "ProjectileHoming" => Some(Flight::Homing {
            speed: vel.length(),
            target,
            life: ms("projectileDuration"),
        }),
        _ => None,
    }
}

/// `ProjectileGo` with the owner's centre of mass in the world (when the owner is a character): the return /
/// target point; for an inverse, where it appears instead of the launch point.
pub(super) fn go(flight: &mut Flight, center: Vec3) -> Option<Vec3> {
    match flight {
        Flight::Boomerang { target, .. } => *target = center,
        Flight::Spark { .. } | Flight::Terrain { .. } | Flight::Homing { .. } => {}
        Flight::Inverse {
            dir,
            start_distance,
            target,
            ..
        } => {
            *target = center;
            return Some(center + *dir * start_distance.max(1.0));
        }
    }
    None
}

/// An inverse without an owner character appears around the launch point ([`go`]).
pub(super) fn inverse_start(flight: &Flight, start: Vec3) -> Option<Vec3> {
    match flight {
        Flight::Inverse {
            dir,
            start_distance,
            ..
        } => Some(start + *dir * start_distance.max(1.0)),
        _ => None,
    }
}

/// The owner's centre of mass in the world (`GetCenterOfMass` × its coords).
pub(super) fn owner_center(data: &GameData, tf: &Transform, ch: &Character) -> Vec3 {
    tf.translation + tf.rotation * crate::monster_skills::character_center(data, ch)
}

/// One frame of the class's `UpdateSelf` before the base's: sets the projectile's velocity for this frame's step;
/// `true` when the class stops it (`ProjectileStop`). `owner`: the living owner's centre of mass; `target_point`:
/// a homing target's hit point (None: dead or gone).
pub(super) fn steer(
    flight: &mut Flight,
    p: &mut Projectile,
    at: Vec3,
    dt: f32,
    owner: Option<Vec3>,
    target_point: Option<Vec3>,
) -> bool {
    match flight {
        Flight::Boomerang {
            speed,
            dir,
            travel,
            flown,
            hover,
            hovered,
            follow,
            follow_left,
            target,
        } => {
            if *flown < *travel {
                *flown += dt * *speed;
                p.vel = *dir * *speed;
            } else if *hover > 0.0 {
                *hover -= dt;
                if !*hovered {
                    *hovered = true;
                    p.passed.clear();
                }
                p.vel = Vec3::ZERO;
            } else {
                if at.distance(*target) <= 1.0 {
                    return true;
                }
                if *follow {
                    *follow_left -= dt;
                    if *follow_left <= 0.0 {
                        return true;
                    }
                    if let Some(c) = owner {
                        *target = c;
                    }
                }
                p.vel = (*target - at).normalize_or_zero() * *speed;
            }
            false
        }
        Flight::Spark {
            speed,
            heading,
            life,
            change,
            every,
            turn,
            ..
        } => {
            *life -= dt;
            if *life <= 0.0 {
                return true;
            }
            *change -= dt;
            if *change <= 0.0 {
                *change = *every;
                if turn.0 <= turn.1 {
                    let a = turn.0 + (turn.1 - turn.0) * gd_game_core::rng::f32();
                    *heading = Quat::from_rotation_y(a.to_radians()) * *heading;
                }
            }
            p.vel = *heading * *speed;
            false
        }
        Flight::Inverse {
            speed,
            life,
            follow,
            target,
            ..
        } => {
            *life -= dt;
            if *life <= 0.0 {
                return true;
            }
            if *follow {
                match owner {
                    Some(c) => *target = c,
                    None => return true,
                }
            }
            let to = *target - at;
            if to.with_y(0.0).length() <= 1.0 {
                return true;
            }
            p.vel = to.normalize_or_zero() * *speed;
            false
        }
        Flight::Terrain { speed, dir, .. } => {
            // The step is level; `fly_projectiles` re-seats y to the floor.
            p.vel = *dir * *speed;
            false
        }
        Flight::Homing { speed, life, .. } => {
            *life -= dt;
            if *life <= 0.0 {
                return true;
            }
            match target_point {
                Some(pt) => p.vel = (pt - at).normalize_or_zero() * *speed,
                None => return true,
            }
            false
        }
    }
}
