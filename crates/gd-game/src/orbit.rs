//! Orbiting projectiles: `Skill_AttackProjectileOrbiting`, `SkillSecondary_AttackProjectileOrbiting` and
//! the `ProjectileOrbiting` projectile they launch; some `Skill_AttackProjectileRing`s launch it too.
//!
//! Game.dll:
//! - `Skill_AttackProjectileOrbiting::Load` (the secondary's alike): `projectilePeriod` × 1000 → int ms;
//!   `projectileDirection`: `Clockwise` 0, `Counterclockwise` 1, `Alternate` 2, `Randomize` 3, else 0;
//!   `projectileStart`: `Front` 0, `Randomize` 1, else 0.
//! - `ActivateNow` starts a burst only while none runs (count left 0 and launch timer `<= 0`; a cast during
//!   one is still paid): count `GetNumberOfProjectiles(level)` (`skillProjectileNumber`, no modifiers; 1
//!   without it), timer 100 ms, the direction (`Alternate` starting at 1), and with `Randomize` one start
//!   angle `rand()` in [0, 2π] for the whole burst. The secondary also keeps the caster's coordinates at
//!   the activation point for the burst.
//! - `Update`: the timer counts down while `>= 0`; below 0 with projectiles left: launch coordinates are
//!   the caster's now (the secondary: the kept ones), `Alternate` flips the direction, `Randomize` turns
//!   them to face (cos a, 0, sin a); then `ProjectileLimit`: `skillProjectileMaximumNumber` (0 = none) live
//!   projectiles of this skill end the whole burst; else one projectile is created, the timer becomes the
//!   period, the count drops by one. A period of 0 launches one per frame.
//! - `Skill_AttackProjectileRing::ActivateNow` creates each orbiter at caster + 1 m up + heading ×
//!   `GetRadius` with direction 0, facing its ring heading; that is the `projectileDistance` origin.
//! - `ProjectileOrbiting::Load`: `projectileStartDistance`, `projectileOrbitRate` (° → rad/s),
//!   `projectileFollow`, `projectileDuration` (× 1000 ms), `inverse` (`disableCollision` is never used);
//!   the re-hit time π / rate (int ms). Missing fields are 0 (no duration: it ends on its first update).
//! - `ProjectileGo`: direction 3 is a random bit. The centre is the launch position (a character owner's
//!   position), 1 m up; the level launch facing × start distance is the offset. The radial speed is
//!   `GetTotalSpeed` ([`crate::casting::proj_speed`]; negative spirals in).
//! - `UpdateSelf`: the duration counts down; at 0 `ProjectileStop`. Else the offset grows by its unit
//!   vector × radial speed × dt and turns by rate × dt about +Y (x' = x cos θ − z sin θ, z' = z cos θ + x
//!   sin θ; direction 1 negates θ); `projectileFollow` moves the centre to the living owner + 1 m (owner
//!   gone or dead: stop); `inverse` stops within 1 m of the centre (horizontally); then it is put at the
//!   centre plus the offset. Then `ProjectileFireballLike::UpdateSelf` as for any flight, with
//!   `ProcessPathTargets` overridden to first forget hits older than the re-hit time (a foe is hit again
//!   each half orbit).
//! - Explosions (`ProjectileBase::ProcessExplosion`, `FilterAndAddKnownTargets`): a sphere query at the hit
//!   foe; a foe passes only if in neither the explosion's own list (never forgotten) nor the projectile's
//!   hit map, so the hit foe itself is spared; every path target of a frame is in the map before the first
//!   explosion runs. Foes found in the map are added to the list without being hit.
//! - `GameEngine::FilterInvalidTargets` drops hiding characters from every target query.
//!
//! PORT: the random start angle and direction bit use the port's generator; secondaries are fired by the
//! projectile's hits only. Only the steering lives here ([`steer`]); flight, hits and explosions are the
//! shared `casting::fly_projectiles`.
use crate::casting::Projectile;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::casting::Casting;
use gd_game_core::casting::ProjMods;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;

pub struct OrbitPlugin;

impl Plugin for OrbitPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Launch>().add_systems(
            Update,
            (launches, bursts)
                .chain()
                .in_set(crate::casting::CastingSet)
                .after(gd_game_core::player::PlayerSet)
                .before(gd_game_core::combat::CombatSet),
        );
    }
}

pub const PROJECTILE_CLASS: &str = "ProjectileOrbiting";

/// `projectileDirection` (`Skill_AttackProjectileOrbiting::ResolveEnum_ProjectileDirection`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Clockwise,
    Counterclockwise,
    Alternate,
    Randomize,
}

impl Direction {
    pub fn parse(s: &str) -> Self {
        match s {
            "Counterclockwise" => Self::Counterclockwise,
            "Alternate" => Self::Alternate,
            "Randomize" => Self::Randomize,
            _ => Self::Clockwise,
        }
    }
}

/// What an orbiting skill's activation starts (its `Load` fields at the cast's level).
#[derive(Clone, Debug)]
pub struct BurstSpec {
    pub number: u32,
    pub period: f32,
    pub direction: Direction,
    /// `projectileStart` `Randomize`.
    pub randomize: bool,
    /// `skillProjectileMaximumNumber` (0 = none).
    pub max: usize,
    /// The secondary's launch position and facing, kept for the burst.
    pub fixed: Option<(Vec3, Vec3)>,
}

impl BurstSpec {
    /// `None`: another class. `at`: the activation point and the caster's facing (kept by the secondary).
    pub fn of(rec: &Record, li: usize, at: (Vec3, Vec3)) -> Option<Self> {
        let secondary = match rec.class.as_str() {
            "Skill_AttackProjectileOrbiting" => false,
            "SkillSecondary_AttackProjectileOrbiting" => true,
            _ => return None,
        };
        let lv = |k: &str| gd_game_core::casting::level_f(rec, k, li);
        Some(Self {
            number: if rec.get("skillProjectileNumber").is_some() {
                lv("skillProjectileNumber").max(0.0) as u32
            } else {
                1
            },
            period: (rec.f32("projectilePeriod").unwrap_or(0.0) * 1000.0) as i32 as f32 / 1000.0,
            direction: Direction::parse(rec.str("projectileDirection").unwrap_or("")),
            randomize: rec.str("projectileStart") == Some("Randomize"),
            max: lv("skillProjectileMaximumNumber").max(0.0) as usize,
            fixed: secondary.then_some(at),
        })
    }
}

#[derive(Clone, Debug)]
pub enum How {
    /// An orbiting skill's activation.
    Burst(BurstSpec),
    /// One projectile now (a ring's), clockwise, starting along `forward`.
    Now { from: Vec3, forward: Vec3 },
}

/// An orbiting launch fired.
#[derive(Message, Clone)]
pub struct Launch {
    pub caster: Entity,
    pub cast: Casting,
    /// The `ProjectileOrbiting` (or other projectile) record.
    pub projectile: String,
    pub mods: ProjMods,
    /// `projectileExplosionRadius` of the skill.
    pub explosion: f32,
    /// The speed factor (1 + speed modifier %).
    pub speed: f32,
    pub how: How,
}

/// A running burst of one caster's orbiting skill.
#[derive(Component)]
pub struct Burst {
    pub caster: Entity,
    launch: Launch,
    spec: BurstSpec,
    /// Projectiles left to launch.
    pub left: u32,
    /// Until the next launch (s); counts only while `>= 0`.
    pub timer: f32,
    /// The current direction: 0 clockwise, 1 counter, 3 random per projectile.
    dir: u8,
    angle: f32,
}

/// The orbit of a `ProjectileOrbiting` [`Projectile`]; [`steer`] runs its steering.
#[derive(Component, Clone, Debug)]
pub struct Orbiting {
    pub center: Vec3,
    pub offset: Vec3,
    pub follow: bool,
    pub inverse: bool,
    /// `projectileOrbitRate` (rad/s) and the direction.
    pub rate: f32,
    pub counter: bool,
    /// Radial speed (m/s).
    pub radial: f32,
    /// Duration left (s).
    pub left: f32,
    /// The re-hit time (s) and the path hits with their time left.
    pub rehit: f32,
    pub hits: Vec<(Entity, f32)>,
    /// Hit by its explosions (once each).
    pub exploded: Vec<Entity>,
}

fn launches(
    mut commands: Commands,
    data: Res<GameData>,
    mut msgs: MessageReader<Launch>,
    mut q: Query<&mut Burst>,
    casters: Query<&Transform, (With<Combatant>, Without<Dead>)>,
) {
    // Bursts started this frame (their entities appear at the next sync).
    let mut started: Vec<(Entity, String)> = Vec::new();
    for l in msgs.read() {
        match &l.how {
            How::Now { forward, .. } => {
                let Ok(tf) = casters.get(l.caster) else {
                    continue;
                };
                spawn_one(&mut commands, &data, l, tf.translation, *forward, 0);
            }
            How::Burst(spec) => {
                let dir = match spec.direction {
                    Direction::Clockwise => 0,
                    Direction::Counterclockwise | Direction::Alternate => 1,
                    Direction::Randomize => 3,
                };
                let angle = if spec.randomize {
                    gd_game_core::rng::f32() * std::f32::consts::TAU
                } else {
                    0.0
                };
                let fresh = |l: &Launch| Burst {
                    caster: l.caster,
                    launch: l.clone(),
                    spec: spec.clone(),
                    left: spec.number,
                    timer: 0.1,
                    dir,
                    angle,
                };
                match q.iter_mut().find(|b| {
                    b.caster == l.caster && b.launch.cast.skill.eq_ignore_ascii_case(&l.cast.skill)
                }) {
                    // Ignored while a burst runs.
                    Some(b) if b.left > 0 || b.timer > 0.0 => {
                        debug!("orbit: {} still launching", l.cast.skill)
                    }
                    Some(mut b) => *b = fresh(l),
                    None if started
                        .iter()
                        .any(|(c, s)| *c == l.caster && s.eq_ignore_ascii_case(&l.cast.skill)) => {}
                    None => {
                        started.push((l.caster, l.cast.skill.clone()));
                        commands.spawn(fresh(l));
                    }
                }
            }
        }
    }
}

fn bursts(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut q: Query<(Entity, &mut Burst)>,
    casters: Query<&Transform, (With<Combatant>, Without<Dead>)>,
    live: Query<&Projectile, With<Orbiting>>,
) {
    let dt = time.delta_secs();
    for (e, mut b) in &mut q {
        let Ok(tf) = casters.get(b.caster) else {
            commands.entity(e).try_despawn();
            continue;
        };
        if b.timer >= 0.0 {
            b.timer -= dt;
            if b.timer < 0.0 && b.left > 0 {
                let mut forward = b
                    .spec
                    .fixed
                    .map_or_else(|| gd_game_core::character::facing_flat(tf), |(_, f)| f);
                if b.spec.direction == Direction::Alternate {
                    b.dir = (b.dir != 1) as u8;
                }
                if b.spec.randomize {
                    forward = Vec3::new(b.angle.cos(), 0.0, b.angle.sin());
                }
                let count = live
                    .iter()
                    .filter(|p| {
                        p.attacker == b.caster && p.skill.eq_ignore_ascii_case(&b.launch.cast.skill)
                    })
                    .count();
                if b.spec.max > 0 && count >= b.spec.max {
                    b.left = 0;
                } else {
                    spawn_one(
                        &mut commands,
                        &data,
                        &b.launch,
                        tf.translation,
                        forward,
                        b.dir,
                    );
                    b.timer = b.spec.period;
                    b.left -= 1;
                }
            }
        }
        if b.timer < 0.0 && b.left == 0 {
            commands.entity(e).try_despawn();
        }
    }
}

/// `ProjectileGo`.
fn spawn_one(
    commands: &mut Commands,
    data: &GameData,
    l: &Launch,
    owner: Vec3,
    forward: Vec3,
    dir: u8,
) {
    let Some(prec) = data.db.get(&l.projectile) else {
        return;
    };
    let f = |k: &str| prec.f32(k).unwrap_or(0.0);
    let center = owner + Vec3::Y;
    let flat = forward.with_y(0.0);
    let offset = if flat.length_squared() > 1e-4 {
        flat.normalize()
    } else {
        forward
    } * f("projectileStartDistance");
    let rate = f("projectileOrbitRate").to_radians();
    let counter = match dir {
        3 => gd_game_core::rng::f32() < 0.5,
        d => d == 1,
    };
    let start = center + offset;
    crate::casting::skill_launch_sound(commands, data, &l.cast.skill, start);
    let proj = crate::casting::spawn_projectile(
        commands,
        data,
        prec,
        l.caster,
        &l.cast,
        start,
        Vec3::ZERO,
        f("projectileDistance"),
        l.explosion,
        false,
        &l.mods,
    );
    commands.entity(proj).insert(Orbiting {
        center,
        offset,
        follow: prec.bool("projectileFollow"),
        inverse: prec.bool("inverse"),
        rate,
        counter,
        radial: f("projectileVelocity") * l.speed,
        left: (f("projectileDuration") * 1000.0) as i32 as f32 / 1000.0,
        rehit: if rate > 0.0 {
            (std::f32::consts::PI * 1000.0 / rate) as i32 as f32 / 1000.0
        } else {
            0.0
        },
        hits: Vec::new(),
        exploded: Vec::new(),
    });
}

/// One orbit step: the offset grows by its unit vector × `radial` × `dt` and turns by `rate` × `dt`
/// about +Y (clockwise from above; `counter` the other way).
pub fn step_offset(offset: Vec3, radial: f32, rate: f32, counter: bool, dt: f32) -> Vec3 {
    let o = offset + offset.normalize_or_zero() * radial * dt;
    let t = if counter { -rate * dt } else { rate * dt };
    let (s, c) = t.sin_cos();
    Vec3::new(c * o.x - s * o.z, o.y, c * o.z + s * o.x)
}

/// One frame of `ProjectileOrbiting::UpdateSelf`'s steering (module doc); the re-hit map forgets its expired
/// entries. `owner`: the living owner's position. True when the orbit stops it (`ProjectileStop`).
pub(crate) fn steer(o: &mut Orbiting, tf: &mut Transform, dt: f32, owner: Option<Vec3>) -> bool {
    o.left -= dt;
    if o.left <= 0.0 {
        return true;
    }
    o.offset = step_offset(o.offset, o.radial, o.rate, o.counter, dt);
    if o.follow {
        match owner {
            Some(c) => o.center = c + Vec3::Y,
            None => return true,
        }
    }
    if o.inverse && (tf.translation - o.center).with_y(0.0).length() <= 1.0 {
        return true;
    }
    tf.translation = o.center + o.offset;
    o.hits.retain_mut(|h| {
        h.1 -= dt;
        h.1 > 0.0
    });
    false
}
