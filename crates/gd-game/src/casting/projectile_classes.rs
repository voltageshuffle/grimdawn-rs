//! The `ProjectileFireballLike` subclasses of the expansions. Boomerang, Spark and Inverse steer themselves
//! (their own `Load`, `ProjectileGo`, `UpdateSelf`; unexported Game.dll classes). Each `ProjectileGo` turns
//! gravity off, sets the speed to `projectileVelocity` × (1 + the speed modifier % / 100) and starts the
//! flight; each `UpdateSelf` moves the projectile, then runs `ProjectileFireballLike::UpdateSelf` (hits along
//! the step, `fly_projectiles`), which stops it (`ProjectileStop`, `explodeOnMiss`) once it is
//! `projectileDistance` or more from its launch coordinates. A stop by the class itself makes no hit that frame.
//!
//! - `ProjectileBoomerang` (`IsExpansion1Loaded`). `Load`: `projectileFollow`, `projectileHoverDuration` and
//!   `projectileFollowDuration` (× 1000, int ms), `projectileTravelDistance`. `ProjectileGo` heads at the
//!   target's hit point (else along the launch direction); the return point is the owner's centre of mass
//!   (no owner character: the launch coordinates). `UpdateSelf`: flies the travel distance along its heading,
//!   hovers for the hover duration (on the first hover frame it forgets every target it hit), then flies
//!   straight at the return point and stops within 1 m (3D). With `projectileFollow` the return point follows
//!   the living owner, and the way back stops when the follow duration runs out.
//! - `ProjectileSpark` (`IsExpansion2Loaded`). `Load`: `projectileDuration` and `changeDirectionTime` (× 1000,
//!   int ms; missing: 0), `disableCollision`, `minRotation` / `maxRotation` (degrees). `ProjectileGo` heads at
//!   the target's hit point, else at the aimed point 1 m up, else along the launch direction. `UpdateSelf`:
//!   the duration's end stops it; the change timer (0 at launch) at 0 restarts and, when min ≤ max, turns the
//!   heading by a uniform draw in [min, max] degrees about +Y (`RandomUniformLocked::FGenerate`).
//!   `disableCollision` clears the collision mask: no wall or door stops it.
//! - `ProjectileInverse` (`IsExpansion1Loaded`). `Load`: `projectileStartDistance`, `projectileFollow`,
//!   `projectileDuration` (× 1000, int ms). `ProjectileGo`: the target point is the owner's centre of mass (no
//!   owner character: the launch coordinates); it appears at the level launch direction ×
//!   max(start distance, 1) from that point, facing out (the launch coordinates stay the caster's). `UpdateSelf`: the duration's
//!   end stops it; with `projectileFollow` the target point follows the owner and a dead or gone owner stops
//!   it; within 1 m of the target point (level distance) it stops; else it flies straight at it (3D).
//! - `ProjectileTerrainFollowing` (`UpdateSelf`): steps along its heading, then `World::GetNearestFloorHeight`
//!   re-seats it at floor + its `actorHeight`, eased (when |Δy| > 1.25×height it moves 0.75 of the excess); it
//!   stops only off the nav mesh (`NavManager::IsPointOnPathMesh`). The terrain is not a wall for it.
//! - `ProjectileHoming` (`UpdateSelf`, `ProjectileGo`): each frame steps straight at its target's
//!   `ProjectileBase::GetHitPoint`; a dead or gone target or the `projectileDuration`'s end stops it;
//!   `ProjectileGo` aborts the launch without a valid target (`fire_casts`).
//! - `ProjectileChain` flies as any fireball; `Skill_AttackProjectileChain` collects the spark list at launch
//!   and the impact attacks every entry ([`super::Projectile::chain`]).
//! - `ProjectileFragmenting` (`IsExpansion3Loaded`) flies as any fireball. `Load`: `fragmentInterval` (× 1000,
//!   int ms) and `fragmentOnStop`. `ProjectileGo` starts the interval timer full; in flight at 0 it restarts
//!   and runs `ProcessFragmentation(0, its coords)` (the `projectileFragmentsName` split, besides the one at
//!   each hit). `ProjectileHit` / `ProjectileStop` split once more with `fragmentOnStop`. [`Fragmenting`].
//!
//! PORT: the owner's centre of mass is `monster_skills::character_center` (the mesh's bind-pose bounding box); a
//! wall or a closed door stops these projectiles as it does the others (`fly_projectiles`).
use super::*;

#[derive(Component, Clone, Debug)]
pub(crate) enum Flight {
    Boomerang {
        speed: f32,
        dir: Vec3,
        /// `projectileTravelDistance` and the distance flown so far.
        travel: f32,
        flown: f32,
        /// Hover time left (s) and whether it started.
        hover: f32,
        hovered: bool,
        follow: bool,
        /// `projectileFollowDuration` left (s), counted on the way back.
        follow_left: f32,
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
    /// Whether a wall or closed door stops it.
    pub(super) fn collides(&self) -> bool {
        match self {
            Flight::Spark { collides, .. } => *collides,
            _ => true,
        }
    }
}

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

pub(super) fn flight_of(
    prec: &Record,
    start: Vec3,
    vel: Vec3,
    target: Option<Entity>,
) -> Option<Flight> {
    let f = |k: &str| prec.f32(k).unwrap_or(0.0);
    // `(int)(x × 1000)` ms.
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
            // `ProjectileGo` replaces it ([`go`]).
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

/// `ProjectileGo` with the owner's centre of mass: the return / target point; an inverse's spawn point.
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

/// An inverse without an owner character appears around the launch point.
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

/// `GetCenterOfMass` × its coords.
pub(super) fn owner_center(data: &GameData, tf: &Transform, ch: &Character) -> Vec3 {
    tf.translation + tf.rotation * crate::monster_skills::character_center(data, ch)
}

/// The class's `UpdateSelf` before the base's: sets this frame's velocity; `true` when the class stops it.
/// `owner`: the living owner's centre of mass; `target_point`: a homing target's hit point.
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
