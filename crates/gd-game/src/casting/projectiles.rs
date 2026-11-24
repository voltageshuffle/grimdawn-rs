use super::*;

/// `Skill_AttackProjectileDebuf` (Game.dll): `Load`
/// reads `projectileNames` and `buffSkillName` and loads the skill's profile
/// from the `buffSkillName` table (duration, radius, cost are the buff
/// record's); `GetProjectileName` = `projectileNames[min(level
/// − 1, n − 1)]` (none at level 0). `TargetResult` makes no
/// attack and runs no secondaries: per target hit (the impact and its
/// explosion radius) `CreateTargetFx`, then `ApplyBuffOnTarget(buff, level =
/// GetCurrentLevel, duration = (int)(skillActiveDuration with the modifiers'
/// (`GetSkillModifierData`) × 1000), debuff)` — the curse's
/// landing (`crate::skill_buffs`). It runs at the impact on the caster's live
/// skill (the projectile sends `SkillTargetResultConfigCmd`, whose `Execute`
/// finds the caster by id, a corpse too, and calls
/// `SkillManager::SkillTargetResult` → the skill's `TargetResult`), so the
/// modifiers, the devotion level and the duration are the caster's at the impact,
/// and a caster that is gone lands nothing ([`fly_projectiles`]). The level is
/// the caster's `GetCurrentLevel` at the impact ([`impact_level_index`]), as for
/// every projectile hit. PORT: the target effect is not made.
#[derive(Clone, Debug)]
pub(super) struct ProjectileDebuf {
    /// The `buffSkillName` record it lands.
    pub(super) buff: String,
}

/// The `Skill_AttackProjectileDebuf` record of a cast: the record itself, or
/// for the player's cast (which runs as the wrapper's `buffSkillName`
/// record, `try_cast`) the learned wrapper of that buff.
pub(super) fn projectile_debuf_wrapper(
    db: &gd_formats::vfs::Database,
    rec: &Record,
    loadout: Option<&Loadout>,
) -> Option<Record> {
    const CLASS: &str = "Skill_AttackProjectileDebuf";
    if rec.class == CLASS {
        return Some(rec.clone());
    }
    if !gd_game_core::skill_buffs::is_debuff(rec) {
        return None;
    }
    loadout?
        .skills
        .skills
        .iter()
        .filter_map(|(w, _)| db.get_owned(w))
        .find(|w| {
            w.class == CLASS
                && gd_sim::skills::profile_name(w)
                    .is_some_and(|b| b.eq_ignore_ascii_case(&rec.name))
        })
}

/// `Skill_AttackProjectileDebuf::GetProjectileName` ([`ProjectileDebuf`]).
pub(super) fn projectile_debuf_name(wrapper: &Record, level_index: usize) -> Option<String> {
    wrapper
        .strs_iter("projectileNames")
        .nth(level_index)
        .or_else(|| wrapper.strs_iter("projectileNames").last())
        .map(|n| n.to_string())
}

/// `kGravity` (Engine.dll).
const GRENADE_GRAVITY: f32 = 14.0;

/// `ProjectileGrenade` (Game.dll `ProjectileGo`, `CollisionCallback`, `UpdateSelf`):
/// a ballistic body, gravity 14 m/s², launched at the angle
/// `launchAngle` (clamped 10 to 80 degrees) with the speed that reaches the target distance (at most `projectileVelocity`,
/// at least 5). The first ground contact (water ignored) plays `projectileBounceSound` once and starts the countdown (the
/// record's miss TTL, a uniform draw of `projectileMissTTLMin` / `Max`); the second contact freezes it. A grenade
/// that is not `harmlessInFlight` explodes at once on touching an enemy in flight (and while it lies), shortening the
/// countdown to its hit TTL. When the countdown ends the usual impact runs (explosion, fragments, sounds, then the `aeProjectileName`
/// area: duration and radius from the skill). A wall is a contact like the ground (the body collides with every
/// level physics mesh, group 2 / 3 against the scenery's mask 0x7C, `crate::collision`; closed doors as for the other
/// projectiles). PORT: no rigid body (Bullet): the bounce height is `restitution` x the fall speed, sliding slowed by
/// `friction`; a wall's first contact sends the grenade back level-wise at `restitution` x its speed (no surface
/// normal), a scenery hit at floor height (within 0.3 m of the ground) is the ground contact.
#[derive(Component)]
pub(crate) struct Grenade {
    harmless: bool,
    bounce_sound: Option<String>,
    restitution: f32,
    friction: f32,
    hit_ttl: f32,
    miss_ttl: f32,
    contacts: u8,
    countdown: Option<f32>,
    frozen: bool,
}

/// `ProjectileBase::GetTargetsAlongPath`: `GetTargetsAroundRay` from the last position
/// `prev` along this frame's `step`, max(|step|, 1) m long, with the projectile's `touch` as the thickness; a zero
/// step (a lying grenade) finds what it lies inside (the engine's direction is NaN then: only the inside test passes).
/// `body` is the target's world box ([`Combatant::body_box`]). Its candidates are the bodies within
/// [`path_reach`] of `prev` ([`crate::combat::grid::BodyGrid`]).
fn along_path(prev: Vec3, step: Vec3, touch: f32, body: &gd_sim::bounds::Aabb) -> bool {
    let mut b = *body;
    b.half = b.half.map(|h| h + touch);
    b.ray_entry(prev.to_array(), step.normalize_or_zero().to_array()) < step.length().max(1.0)
}

/// How far from `prev` (beyond a body's half extent) [`along_path`] can find a body.
fn path_reach(step: Vec3, touch: f32) -> f32 {
    step.length().max(1.0) + touch
}

/// Where a projectile cast at a target aims. `SkillActivated::ExecuteRangedAttack`: at the
/// request point, the target's `Entity::GetCoords` (its feet), so a shot descends onto low breakables.
/// PORT: a target whose body floats clear above its coords (a hero's loot orb: `chestsphere02.msh` sits 2.4 to
/// 3.1 m up) is aimed at its body's centre instead, or the shot would pass under it.
pub(crate) fn aim_point(point: Vec3, body: Option<&gd_sim::bounds::Aabb>) -> Vec3 {
    match body {
        Some(b) if b.center[1] - b.half[1] > point.y + 0.5 => Vec3::from(b.center),
        _ => point,
    }
}

/// The cast's launch coordinates (`Skill::GetCoordsFromCallback`,
/// `Skill_AttackProjectile::ActivateNow`): the callback's attach
/// point in the pose (`R Hand` / `L Hand`), the profile's
/// `launchAttachPointName` when the mesh has no such hand point, else the
/// body-box centre (`monster_skills::character_center`); a caster without a
/// character keeps the old belly height.
pub(crate) fn launch_coords(
    data: &GameData,
    tf: &Transform,
    ch: Option<&Character>,
    rec: &Record,
    hand: &str,
) -> Vec3 {
    let named = rec.str("launchAttachPointName").filter(|n| !n.is_empty());
    let local = ch.and_then(|c| {
        c.posed_point(hand)
            .or_else(|| named.and_then(|n| c.posed_point(n)))
    });
    match (local, ch) {
        (Some(p), _) => tf.transform_point(p),
        (None, Some(c)) => tf.transform_point(crate::monster_skills::character_center(data, c)),
        (None, None) => tf.translation + Vec3::Y * 1.2,
    }
}

/// The ballistic launch velocity of a grenade from `start` toward `flat_dir` over `dist` metres (`dy`: the target's height
/// less the start's): `v = sqrt(d² g / (2 cos²a (d tan a - dy)))`, g = 14, capped by the record's velocity, at least 5.
pub(crate) fn grenade_velocity(prec: &Record, flat_dir: Vec3, dist: f32, dy: f32) -> Vec3 {
    const G: f32 = 14.0;
    let a = prec
        .f32("launchAngle")
        .unwrap_or(30.0)
        .clamp(10.0, 80.0)
        .to_radians();
    let d = dist.max(0.1);
    let denom = 2.0 * a.cos().powi(2) * (d * a.tan() - dy);
    let v = if denom > 0.0 {
        (d * d * G / denom).sqrt()
    } else {
        5.0
    };
    let cap = prec.f32("projectileVelocity").unwrap_or(20.0);
    let speed = cap.min(v.max(5.0));
    flat_dir.with_y(0.0).normalize_or(Vec3::Z) * (speed * a.cos()) + Vec3::Y * (speed * a.sin())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_projectile(
    commands: &mut Commands,
    data: &GameData,
    prec: &Record,
    attacker: Entity,
    cast: &Casting,
    start: Vec3,
    vel: Vec3,
    dist: f32,
    explosion: f32,
    drop: bool,
    mods: &ProjMods,
) -> Entity {
    let grenade = prec.class == "ProjectileGrenade";
    // A grenade is lobbed: the straight launch velocity only gives its heading and the distance.
    let vel = if grenade {
        // The real launch→aim height difference over the flight distance
        // (the aim direction's slope), not a fixed −1.0.
        let dy = vel.normalize_or_zero().y * dist;
        grenade_velocity(prec, vel, dist, dy)
    } else {
        vel
    };
    let dist = if grenade { 1.0e6 } else { dist };
    // The self-steering classes (`projectile_classes`); an inverse appears out on its ring.
    let flight = flight_of(prec, start, vel, cast.target);
    let at = flight
        .as_ref()
        .and_then(|f| inverse_start(f, start))
        .unwrap_or(start);
    // An entity's forward is its local +Z ([`gd_game_core::character::facing`]), the
    // way its mesh is built (an arrow's tip, Dreeg's eye's iris): `looking_to`
    // aims −Z, so it is given the reverse direction.
    let tf = Transform::from_translation(at).looking_to(-vel.normalize_or(Vec3::Z), Vec3::Y);
    let mut flight_fx = prec
        .str("projectileFlightFX")
        .and_then(|f| spawn_fx_looping(commands, data, f, tf));
    // The skill's added projectile effects (`ProjMods::effects`) ride with the flight effect.
    for f in &mods.effects {
        match flight_fx {
            Some(p) => {
                if let Some(e) = spawn_fx_looping(commands, data, f, tf) {
                    commands.entity(e).insert(Transform::IDENTITY);
                    commands.entity(p).add_child(e);
                }
            }
            None => flight_fx = spawn_fx_looping(commands, data, f, tf),
        }
    }
    let proj = commands.spawn_empty().id();
    // `PlaySwipeSound` as it is made, tracked with it (`crate::combat_sounds`).
    if let Some(pak) = crate::combat_sounds::projectile_launch(data, &prec.name) {
        commands.write_message(gd_game_core::visuals::PlaySoundOn {
            pak,
            on: proj,
            at: start,
        });
    }
    let skill_rec = data.db.get(&cast.skill);
    let spawns_pet = skill_rec
        .as_ref()
        .is_some_and(|s| s.class == "Skill_AttackProjectileSpawnPet");
    let mut fragments: Vec<(String, u32, u32)> = skill_rec
        .and_then(|s| {
            let name = mods
                .fragments_name
                .clone()
                .or_else(|| s.str("projectileFragmentsName").map(str::to_owned))?;
            let li = cast.level_index;
            (!name.eq_ignore_ascii_case(&prec.name)).then(|| {
                let lo = level_f(s, "projectileFragmentsLaunchNumberMin", li).max(0.0) as u32;
                let hi = (level_f(s, "projectileFragmentsLaunchNumberMax", li) as u32).max(lo);
                (name, lo, hi)
            })
        })
        .into_iter()
        .collect();
    // Fragments don't split again.
    // (`ProjectileFragmenting` records are named "...fragmenting...": not fragments.)
    let fragment = prec.name.contains("fragment") && prec.class != "ProjectileFragmenting";
    if !fragment {
        fragments.extend(mods.fragments.iter().cloned());
    }
    commands.entity(proj).insert((
        Projectile {
            attacker,
            record: prec.name.clone(),
            skill: cast.skill.clone(),
            level_index: cast.level_index,
            parent: cast.parent.clone(),
            vel,
            left: dist,
            explosion,
            impact_fx: prec
                .str("projectileExplodingImpactFX")
                .or(prec.str("projectileImpactFX"))
                .map(str::to_owned),
            flight_fx,
            drop,
            hostile: cast.hostile,
            side: None,
            fragments,
            exclude: cast.target.filter(|_| fragment),
            pierce: mods.pierce,
            passed: Vec::new(),
            origin: start,
            touch: prec.f32("actorRadius").unwrap_or(0.0).max(0.0),
            explode_on_miss: prec.bool("explodeOnMiss"),
            spawns_pet,
            debuf: None,
            chain: Vec::new(),
        },
        tf,
    ));
    if let Some(f) = fragmenting_of(prec) {
        commands.entity(proj).insert(f);
    }
    // `ProjectileGo` of the self-steering classes, with the owner's centre of mass at launch.
    if let Some(flight) = flight {
        commands.entity(proj).insert(flight);
        commands.queue(move |w: &mut World| {
            let center = {
                let (Some(tf), Some(ch), Some(data)) = (
                    w.get::<Transform>(attacker),
                    w.get::<Character>(attacker),
                    w.get_resource::<GameData>(),
                ) else {
                    return;
                };
                owner_center(data, tf, ch)
            };
            let Some(at) = w
                .get_mut::<Flight>(proj)
                .and_then(|mut f| go(&mut f, center))
            else {
                return;
            };
            for e in [Some(proj), flight_fx].into_iter().flatten() {
                if let Some(mut t) = w.get_mut::<Transform>(e) {
                    t.translation = at;
                }
            }
        });
    }
    if grenade {
        let draw = |lo: &str, hi: &str| {
            let (a, b) = (prec.f32(lo).unwrap_or(0.0), prec.f32(hi).unwrap_or(0.0));
            a.min(b) + (a.max(b) - a.min(b)) * gd_game_core::rng::f32()
        };
        commands.entity(proj).insert(Grenade {
            harmless: prec.bool("harmlessInFlight"),
            bounce_sound: prec
                .str("projectileBounceSound")
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            restitution: prec
                .f32("physicsRestitution")
                .unwrap_or(0.0)
                .clamp(0.0, 1.0),
            friction: prec.f32("physicsFriction").unwrap_or(0.0),
            hit_ttl: draw("projectileHitTTLMin", "projectileHitTTLMax"),
            miss_ttl: draw("projectileMissTTLMin", "projectileMissTTLMax"),
            contacts: 0,
            countdown: None,
            frozen: false,
        });
    }
    // `inflightGroundFxPakName` / `inflightGroundFxDropTime` (`ProjectileFireballLike::Load`): an effect
    // dropped on the floor under the projectile every interval while it flies.
    if let Some(pak) = prec
        .str("inflightGroundFxPakName")
        .filter(|p| !p.is_empty())
    {
        let interval = (prec.f32("inflightGroundFxDropTime").unwrap_or(0.0) * 1000.0) as i32;
        if interval > 0 {
            commands.entity(proj).insert(GroundDrops {
                pak: pak.to_owned(),
                interval: interval as f32 / 1000.0,
                left: interval as f32 / 1000.0,
            });
        }
    }
    // Its trails run between the mesh's `Anchor1`/`Anchor2` and, when
    // there, `Anchor3`/`Anchor4`; no `Anchor1`, no trail
    // (`ProjectileBase::CountWeaponTrails`). The
    // anchors scale with the model (`scale`).
    if let Some(trail) = prec.str("projectileWeaponTrail").filter(|t| !t.is_empty())
        && let Some(m) = prec.str("mesh")
    {
        let width = 0.35 * prec.f32("scale").unwrap_or(1.0).max(0.3) * 2.0;
        let scale = prec.f32("scale").unwrap_or(1.0);
        weapon_trails(commands, proj, m, trail, width, scale);
    }
    // The projectile's own model (an eyeball, an arrow), unless the record
    // hides it (`baseTexture` invisible, the sphere placeholder of area FX).
    let hidden = prec
        .str("baseTexture")
        .is_some_and(|t| t.contains("invisible"))
        || prec
            .str("mesh")
            .is_some_and(|m| m.ends_with("meshfx/sphere.msh"));
    if let (Some(m), false) = (prec.str("mesh"), hidden) {
        commands.spawn((
            ProjectileMesh(
                m.to_owned(),
                prec.f32("scale").unwrap_or(1.0),
                prec.str("glowTexture")
                    .filter(|g| !g.is_empty())
                    .map(str::to_owned),
            ),
            tf,
            ProjectileModelOf(proj),
        ));
    }
    proj
}

/// A flight effect that keeps playing until removed with its projectile.
pub(crate) fn spawn_fx_looping(
    commands: &mut Commands,
    data: &GameData,
    record: &str,
    tf: Transform,
) -> Option<Entity> {
    let rec = data.db.get(record)?;
    let rec_name = if rec.class == "FxPak" || rec.get("particleEffectNames").is_some() {
        if let Some(pak) = gd_game_core::combat_sounds::fx_pak_sound(rec) {
            commands.write_message(gd_game_core::visuals::PlaySound {
                pak,
                at: Some(tf.translation),
            });
        }
        rec.strs_iter("particleEffectNames")
            .next()
            .map(|s| s.to_string())?
    } else {
        record.to_owned()
    };
    Some(
        commands
            .spawn((gd_game_core::visuals::EffectSpec::record(rec_name), tf))
            .id(),
    )
}

/// A projectile's ground effect drops (`inflightGroundFxPakName`; `ProjectileFireballLike::UpdateSelf`):
/// the first one interval after launch, then every interval, at the projectile's
/// position snapped to the floor with its heading; ownerless, never stopped (the pak runs its own life); one drop per
/// frame at most. The Boomerang / Spark / Inverse classes run that `UpdateSelf` after their own and
/// `ProjectileFragmenting::UpdateSelf` repeats the drop (`projectile_classes`). PORT: every
/// other record class with the field drops too (the engine's Orbiting / Homing classes were assumed to inherit it).
#[derive(Component)]
pub(crate) struct GroundDrops {
    pak: String,
    interval: f32,
    left: f32,
}

pub(super) fn drop_ground_fx(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    nav: Res<Nav>,
    world_coll: Res<crate::collision::WorldCollision>,
    mut q: Query<(&mut GroundDrops, &Transform), With<Projectile>>,
) {
    for (mut d, tf) in &mut q {
        d.left -= time.delta_secs();
        if d.left > 0.0 {
            continue;
        }
        d.left = d.interval;
        let p = tf.translation;
        let y = nav
            .0
            .ground([p.x, p.y + 0.5, p.z])
            .or_else(|| world_coll.terrain_ground(p))
            .unwrap_or(p.y);
        let at = Transform::from_translation(Vec3::new(p.x, y, p.z)).with_rotation(tf.rotation);
        gd_game_core::visuals::spawn_fx(&mut commands, &data, &d.pak, at);
    }
}

/// Where a grenade's step `a -> b` first meets a scenery physics mesh or a closed door (the terrain is its ground
/// contact, handled apart).
fn grenade_wall(
    world: &crate::collision::WorldCollision,
    doors: &Query<&crate::nav::NavObstacle, With<gd_game_core::world::DoorMarker>>,
    a: Vec3,
    b: Vec3,
) -> Option<Vec3> {
    let door = doors
        .iter()
        .flat_map(|o| o.0.iter())
        .filter_map(|o| crate::collision::segment_obstacle(a, b, o))
        .min_by(|x, y| x.total_cmp(y))
        .map(|t| a + (b - a) * t);
    world
        .segment_hit(a, b)
        .filter(|(_, what)| *what != "terrain")
        .map(|h| h.0)
        .into_iter()
        .chain(door)
        .min_by(|x, y| {
            (*x - a)
                .length_squared()
                .total_cmp(&(*y - a).length_squared())
        })
}

/// The caster's skill level at the impact (Game.dll: the projectile sends
/// `SkillTargetResultConfigCmd`, whose `Execute` finds the caster by id
/// and runs `SkillManager::SkillTargetResult` on its live skill object;
/// `SkillActivatedSpell::TargetResult` → `Skill::CollectCombatParameters`
/// and `Skill_AttackProjectileDebuf::TargetResult` read `GetCurrentLevel`
/// there): the player's current level of the skill the cast ran as — a
/// wrapper cast's `skill` is its `buffSkillName` record, the level is the learned
/// wrapper's ([`projectile_debuf_wrapper`] the same scan). Monsters' and pets' skill
/// levels are fixed at their spawn: the cast's level index is still the current one.
pub(crate) fn impact_level_index(
    db: &gd_formats::vfs::Database,
    p: &Projectile,
    caster: Option<(&Loadout, &gd_game_core::player::EffectiveSkills)>,
) -> usize {
    let Some((loadout, effective)) = caster else {
        return p.level_index;
    };
    let mut skill = p.skill.as_str();
    if loadout.skill_level(db, effective, skill) == 0
        && let Some(w) = loadout
            .skills
            .skills
            .iter()
            .map(|(s, _)| s.as_str())
            .find(|s| {
                db.get(s).is_some_and(|r| {
                    gd_sim::skills::profile_name(r)
                        .is_some_and(|b| b.eq_ignore_ascii_case(&p.skill))
                })
            })
    {
        skill = w;
    }
    (loadout.skill_level(db, effective, skill).max(1) - 1) as usize
}

#[allow(clippy::too_many_arguments)]
pub(super) fn fly_projectiles(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    nav: Res<Nav>,
    world_coll: Res<crate::collision::WorldCollision>,
    doors: Query<&crate::nav::NavObstacle, With<gd_game_core::world::DoorMarker>>,
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &mut Projectile,
            Option<&mut Grenade>,
            Option<&mut Flight>,
            Option<&mut Fragmenting>,
            Option<&mut crate::orbit::Orbiting>,
        ),
        Without<Combatant>,
    >,
    mut fx: Query<&mut Transform, (Without<Projectile>, Without<Combatant>)>,
    all: Query<
        (Entity, &Transform, &Combatant, crate::combat::VictimSide),
        (Without<Dead>, Without<Projectile>),
    >,
    sides: Query<crate::combat::SideData>,
    mut hits: MessageWriter<SkillHit>,
    mut activations: MessageWriter<crate::secondary::Activate>,
    mut debufs: MessageWriter<crate::skill_buffs::DebufTarget>,
    owners: Query<(&Transform, &Character), (With<Combatant>, Without<Dead>)>,
    // The caster at the impact: its live skill level, a debuff's modifiers,
    // whether it is the player (devotion).
    (attackers, devotion): (
        Query<(
            Option<&gd_game_core::weapon_pool::SkillLinks>,
            Has<Player>,
            Option<(&Loadout, &gd_game_core::player::EffectiveSkills)>,
        )>,
        Option<Res<crate::devotion::DevotionState>>,
    ),
) {
    let dt = time.delta_secs();
    // The bodies a projectile can touch, indexed at the first projectile (`combat::grid`: the candidates come in
    // `all`'s order, so the first found and the nearest-first ties are the full scan's).
    let mut bodies = None;
    let mut near = Vec::new();
    for (e, mut tf, mut p, mut grenade, mut flight, mut fragmenting, mut orbiting) in &mut q {
        let p = &mut *p;
        let side = crate::combat::Side::lookup_kept(&sides, p.attacker, p.hostile, &mut p.side);
        // What it can touch this frame. By the `passed` list as the frame began: `steer` may clear it (a boomerang
        // turning back); empty (no allocation) for all but piercers.
        let passed = p.passed.clone();
        // An orbiter's re-hit map as the frame began (its path hits forget
        // themselves after the re-hit time, `crate::orbit`).
        let rehit_hits: Vec<Entity> = orbiting
            .as_ref()
            .map_or_else(Vec::new, |o| o.hits.iter().map(|h| h.0).collect());
        let (attacker, exclude) = (p.attacker, p.exclude);
        let (list, grid) = &*bodies.get_or_insert_with(|| {
            let list: Vec<_> = all.iter().collect();
            let grid = crate::combat::grid::BodyGrid::new(
                list.iter()
                    .map(|(_, t, c, _)| c.body_box(&t.compute_affine()))
                    .collect(),
            );
            (list, grid)
        });
        let can_hit = |i: usize| {
            let (m, _, _, v) = &list[i];
            side.hits(v)
                && *m != attacker
                && Some(*m) != exclude
                && !passed.contains(m)
                && !rehit_hits.contains(m)
        };
        if let Some(g) = grenade.as_deref()
            && !g.frozen
        {
            p.vel.y -= GRENADE_GRAVITY * dt;
        }
        // A self-steering class sets this frame's velocity, or stops
        // (`projectile_classes`); an orbiter steers its position directly
        // (`crate::orbit`, the engine's `ProjectileOrbiting::UpdateSelf`
        // before the shared `ProjectileFireballLike::UpdateSelf`).
        let mut stopped = false;
        let prev = tf.translation;
        if let Some(o) = orbiting.as_deref_mut() {
            let owner = owners.get(p.attacker).ok().map(|(t, _)| t.translation);
            stopped = crate::orbit::steer(o, &mut tf, dt, owner);
            if stopped {
                p.vel = Vec3::ZERO;
            }
        } else {
            if let Some(f) = flight.as_deref_mut() {
                let owner = owners
                    .get(p.attacker)
                    .ok()
                    .map(|(t, c)| owner_center(&data, t, c));
                // A homing projectile re-aims at its target's hit point each
                // frame (`ProjectileBase::GetHitPoint`: the body box centre).
                let target_point = match f {
                    Flight::Homing { target, .. } => target
                        .and_then(|t| all.get(t).ok())
                        .map(|(_, t, c, _)| Vec3::from(c.body_box(&t.compute_affine()).center)),
                    _ => None,
                };
                stopped = steer(f, p, tf.translation, dt, owner, target_point);
                if stopped {
                    p.vel = Vec3::ZERO;
                }
            }
            tf.translation += p.vel * dt;
        }
        // `ProjectileTerrainFollowing::UpdateSelf`:
        // re-seat y to the floor + the projectile's height, eased (past
        // 1.25 × height it moves 0.75 of the excess); off the nav mesh it
        // stops (`NavManager::IsPointOnPathMesh`).
        if let Some(Flight::Terrain { height, .. }) = flight.as_deref() {
            let at = tf.translation;
            match nav.0.ground([at.x, at.y + 0.5, at.z]) {
                Some(floor) => {
                    let want = floor + height;
                    let dy = want - at.y;
                    tf.translation.y = if dy.abs() > 1.25 * height {
                        at.y + dy.signum() * (dy.abs() - 1.25 * height) * 0.75
                    } else {
                        want
                    };
                }
                None => {
                    stopped = true;
                    p.vel = Vec3::ZERO;
                }
            }
        }
        let step = tf.translation - prev;
        if flight.is_some() || orbiting.is_some() {
            // `ProjectileFireballLike::UpdateSelf`: `projectileDistance` (`left`) from the launch point stops it.
            if step.length_squared() > 0.0 {
                tf.rotation = Transform::IDENTITY.looking_to(-step, Vec3::Y).rotation;
            }
            if stopped || (tf.translation - p.origin).length() >= p.left {
                p.left = 0.0;
            }
        } else {
            p.left -= step.length();
        }
        // `ProjectileFragmenting::UpdateSelf`: a split where it flies every `fragmentInterval`.
        if let Some(f) = fragmenting.as_deref_mut() {
            f.left -= dt;
            if f.left <= 0.0 {
                f.left = f.every;
                split(&mut commands, &data, p, tf.translation, None);
            }
        }
        // A grenade's ground contacts, touch and countdown (`Grenade`).
        let mut grenade_boom = false;
        if let Some(g) = grenade.as_deref_mut() {
            // A wall (a scenery physics mesh or a closed door) on the step is a contact like the ground's.
            // A hit at floor height is the ground contact below (a dungeon's floor is a physics mesh too).
            let wall = (!g.frozen && step.length_squared() > 0.0)
                .then(|| grenade_wall(&world_coll, &doors, prev, tf.translation))
                .flatten()
                .filter(|h| {
                    let floor = nav
                        .0
                        .ground([h.x, h.y + 1.0, h.z])
                        .or_else(|| world_coll.terrain_ground(*h));
                    floor.is_none_or(|f| h.y > f + 0.3)
                });
            if let Some(hit) = wall {
                g.contacts += 1;
                // Back off the surface a little along the step.
                tf.translation = hit - step.normalize_or_zero() * 0.05;
                if g.contacts == 1 {
                    if let Some(pak) = &g.bounce_sound {
                        commands.write_message(gd_game_core::visuals::PlaySoundOn {
                            pak: pak.clone(),
                            on: e,
                            at: tf.translation,
                        });
                    }
                    g.countdown = Some(g.miss_ttl);
                    let k = g.restitution;
                    p.vel.x *= -k;
                    p.vel.z *= -k;
                } else {
                    g.frozen = true;
                    p.vel = Vec3::ZERO;
                }
            }
            let at = tf.translation;
            let ground = nav
                .0
                .ground([at.x, at.y + 1.0, at.z])
                .or_else(|| world_coll.terrain_ground(at))
                .unwrap_or(f32::MIN);
            if !g.frozen && at.y <= ground + 0.1 && p.vel.y <= 0.0 {
                tf.translation.y = ground + 0.1;
                g.contacts += 1;
                if g.contacts == 1 {
                    if let Some(pak) = &g.bounce_sound {
                        commands.write_message(gd_game_core::visuals::PlaySoundOn {
                            pak: pak.clone(),
                            on: e,
                            at: tf.translation,
                        });
                    }
                    g.countdown = Some(g.miss_ttl);
                }
                if g.contacts >= 2 {
                    g.frozen = true;
                    p.vel = Vec3::ZERO;
                } else {
                    p.vel.y = -p.vel.y * g.restitution;
                    let k = 1.0 / (1.0 + g.friction * 0.05);
                    p.vel.x *= k;
                    p.vel.z *= k;
                }
            }
            // An enemy on its path (in flight or lying) shortens the countdown to the hit TTL.
            if !g.harmless || g.frozen || g.contacts > 0 {
                let from = tf.translation - step;
                grid.near(from, path_reach(step, p.touch), &mut near);
                let touched = near
                    .iter()
                    .any(|&i| can_hit(i) && along_path(from, step, p.touch, grid.body(i)));
                if touched && !g.harmless {
                    g.countdown = Some(g.countdown.map_or(g.hit_ttl, |c| c.min(g.hit_ttl)));
                }
            }
            if let Some(c) = &mut g.countdown {
                *c -= dt;
                grenade_boom = *c <= 0.0;
            }
        }
        if let Some(f) = p.flight_fx
            && let Ok(mut ft) = fx.get_mut(f)
        {
            *ft = *tf;
        }
        let hit = if grenade.is_some() {
            // At the countdown's end: the foe the grenade lies at, if any (the explosion radius adds the rest).
            grenade_boom
                .then(|| {
                    let from = tf.translation - step;
                    grid.near(from, path_reach(step, p.touch), &mut near);
                    near.iter()
                        .copied()
                        .filter(|&i| can_hit(i) && along_path(from, step, p.touch, grid.body(i)))
                        .min_by(|&a, &b| {
                            (list[a].1.translation - tf.translation)
                                .length_squared()
                                .total_cmp(
                                    &(list[b].1.translation - tf.translation).length_squared(),
                                )
                        })
                })
                .flatten()
        } else if p.drop || stopped {
            None
        } else {
            let from = tf.translation - step;
            grid.near(from, path_reach(step, p.touch), &mut near);
            near.iter()
                .copied()
                .find(|&i| can_hit(i) && along_path(from, step, p.touch, grid.body(i)))
        };
        let hit_monster = hit.map(|i| list[i].0);
        // World contact (`crate::collision` module docs): the
        // projectile stops where its path meets terrain, a scenery physics
        // mesh or a closed door, unless it hit a target first. Drops fall
        // from above and land instead (`landed`).
        let mut wall = None;
        if hit_monster.is_none()
            && !p.drop
            && !stopped
            && flight.as_deref().is_none_or(Flight::collides)
            && grenade.is_none()
            && step.length_squared() > 0.0
        {
            let door = doors
                .iter()
                .flat_map(|o| o.0.iter())
                .filter_map(|o| crate::collision::segment_obstacle(prev, tf.translation, o))
                .min_by(|a, b| a.total_cmp(b))
                .map(|t| (prev + step * t, "door"));
            // A terrain follower rides the ground: only scenery and doors
            // stop it (the terrain contact is its floor, not a wall).
            let terrain_follow = matches!(flight.as_deref(), Some(Flight::Terrain { .. }));
            wall = world_coll
                .segment_hit(prev, tf.translation)
                .into_iter()
                .filter(|(_, what)| !terrain_follow || *what != "terrain")
                .chain(door)
                .min_by(|a, b| {
                    (a.0 - prev)
                        .length_squared()
                        .total_cmp(&(b.0 - prev).length_squared())
                });
            if let Some((at, what)) = wall {
                debug!(
                    "projectile {} stopped by {what} at {at:?} (from {prev:?}, launched {:?})",
                    p.skill.rsplit('/').next().unwrap_or(&p.skill),
                    p.origin
                );
                commands.write_message(crate::melee::ProjectileCollided {
                    attacker: p.attacker,
                });
                tf.translation = at;
                if let Some(f) = p.flight_fx
                    && let Ok(mut ft) = fx.get_mut(f)
                {
                    *ft = *tf;
                }
            }
        }
        let landed = grenade_boom
            || (p.drop
                && nav
                    .0
                    .ground((tf.translation).to_array())
                    .is_some_and(|g| tf.translation.y <= g + 0.1));
        if hit_monster.is_none() && !landed && wall.is_none() && p.left > 0.0 {
            continue;
        }
        // The impact runs on the caster's live skill (`SkillTargetResultConfigCmd`
        // → `SkillManager::SkillTargetResult`): its `GetCurrentLevel` now, not the
        // cast's level ([`impact_level_index`]).
        p.level_index = impact_level_index(
            &data.db,
            p,
            attackers.get(p.attacker).ok().and_then(|(_, _, l)| l),
        );
        // A miss (world contact or end of flight) only explodes with
        // `explodeOnMiss` (state 3 of `ProjectileFireballLike::UpdateSelf`).
        let boom = hit_monster.is_some() || landed || (p.explode_on_miss && p.explosion > 0.0);
        // Its sound (`crate::combat_sounds::projectile_hit`): at the target
        // hit, else where it stopped (PORT: a drop's landing counts as a
        // hit there).
        {
            use crate::combat_sounds::ProjectileEvent as Ev;
            let exploding = p.explosion > 0.0;
            let (ev, at) = match hit {
                Some(i) => (Ev::Hit { exploding }, list[i].1.translation),
                None if landed => (Ev::Hit { exploding }, tf.translation),
                None if boom => (Ev::MissExplode, tf.translation),
                None => (Ev::Destruct, tf.translation),
            };
            if let Some(pak) = crate::combat_sounds::projectile_hit(&data, &p.record, ev) {
                commands.write_message(gd_game_core::visuals::PlaySound { pak, at: Some(at) });
            }
        }
        let mut targets: Vec<Entity> = hit_monster.into_iter().collect();
        if p.explosion > 0.0 && boom {
            grid.near(tf.translation, p.explosion, &mut near);
            for &i in &near {
                let t = list[i].0;
                // `Combatant::in_radius` on the indexed box.
                if Some(t) == hit_monster
                    || !can_hit(i)
                    || !grid
                        .body(i)
                        .meets_sphere(tf.translation.to_array(), p.explosion)
                {
                    continue;
                }
                if let Some(o) = orbiting.as_deref_mut() {
                    // The explosion's own list never forgets; a foe already
                    // in the hit map joins it without being hit
                    // (`FilterAndAddKnownTargets`, `crate::orbit`).
                    if o.exploded.contains(&t) {
                        continue;
                    }
                    o.exploded.push(t);
                    if o.hits.iter().any(|h| h.0 == t) {
                        continue;
                    }
                    o.hits.push((t, o.rehit));
                }
                targets.push(t);
            }
        }
        // A chain projectile's spark list: the impact attacks every entry
        // (`Skill_AttackProjectileChain::TargetResult`).
        if !p.chain.is_empty() && boom {
            for &t in &p.chain {
                if !targets.contains(&t) && list.iter().any(|(m, _, _, v)| *m == t && side.hits(v))
                {
                    targets.push(t);
                }
            }
        }
        // An orbiter's path hits join its re-hit map (forgotten after the
        // re-hit time, so a foe is hit again each half orbit).
        if let Some(o) = orbiting.as_deref_mut() {
            for &t in &targets {
                if !o.hits.iter().any(|h| h.0 == t) {
                    o.hits.push((t, o.rehit));
                }
            }
        }
        // Burst / ring range falloff by the distance flown (the
        // same for every target of the impact).
        let scale = if targets.is_empty() {
            1.0
        } else {
            data.db
                .get(&p.skill)
                .and_then(|r| gd_sim::actor::range_scale(r, (tf.translation - p.origin).length()))
                .unwrap_or(1.0)
        };
        if let Some(d) = &p.debuf
            && !targets.is_empty()
            && let Ok((links, player, _)) = attackers.get(p.attacker)
        {
            // `Skill_AttackProjectileDebuf::TargetResult` ([`ProjectileDebuf`]), on the caster's skill as it is
            // now: the buff record at the level with the skill's modifiers for its `skillActiveDuration`
            // (whole ms).
            let mods = links
                .map(|l| l.mods_of(&p.skill).to_vec())
                .unwrap_or_default();
            let li = p.level_index;
            let secs = data.db.get(&d.buff).map_or(0.0, |r| {
                level_f(r, "skillActiveDuration", li)
                    + mods
                        .iter()
                        .filter_map(|(m, ml)| {
                            data.db
                                .get(m)
                                .map(|mr| level_f(mr, "skillActiveDuration", *ml))
                        })
                        .sum::<f32>()
            });
            let devotion = devotion
                .as_deref()
                .filter(|_| player)
                .map_or(0, |dv| dv.devotion_level(&p.skill));
            for &t in &targets {
                debufs.write(crate::skill_buffs::DebufTarget {
                    caster: p.attacker,
                    target: t,
                    buff: d.buff.clone(),
                    skill: p.skill.clone(),
                    level_index: li,
                    mods: mods.clone(),
                    devotion,
                    secs: (secs * 1000.0) as i32 as f32 / 1000.0,
                });
            }
        } else if p.debuf.is_none() {
            for &t in &targets {
                hits.write(SkillHit {
                    attacker: p.attacker,
                    target: t,
                    skill: p.skill.clone(),
                    level_index: p.level_index,
                    parent: p.parent.clone(),
                    scale,
                    projectile: true,
                });
            }
            // The impact resolves the attack: the parent's secondaries on what
            // it hit, at the projectile (`TargetInformation` → `TargetResult`,
            // `secondary.rs`); nothing hit, none.
            activations.write_batch(crate::secondary::Activate::on_hits(
                p.attacker,
                &p.skill,
                p.level_index,
                &p.parent,
                targets,
                tf.translation,
            ));
        }
        // `Skill_AttackProjectileSpawnPet::TargetInformation`: its
        // end summons at the impact / the foe hit (`pets::placement::hit_spots`).
        if p.spawns_pet {
            commands.write_message(SpawnPet {
                owner: p.attacker,
                skill: p.skill.clone(),
                level_index: p.level_index,
                point: tf.translation,
                target: hit_monster,
            });
        }
        if let Some(i) = &p.impact_fx
            && boom
        {
            spawn_fx(
                &mut commands,
                &data,
                i,
                Transform::from_translation(tf.translation),
            );
        }
        // `ProjectileGrenade::ProjectileStop`: a grenade's `aeProjectileName` area goes down where it ended, with the
        // skill's duration and radius (`Skill_AttackProjectileAreaEffect` values).
        if grenade_boom
            && let Some(ae) = data
                .db
                .get(&p.record)
                .and_then(|r| r.str("aeProjectileName").map(str::to_owned))
            && !ae.is_empty()
            && let Some(skill) = data.db.get(&p.skill)
        {
            let lf = |k: &str| level_f(skill, k, p.level_index);
            let cast = Casting {
                skill: p.skill.clone(),
                level_index: p.level_index,
                kind: Kind::Area,
                point: tf.translation,
                target: None,
                hostile: p.hostile,
                parent: p.parent.clone(),
            };
            spawn_area_of(
                &mut commands,
                &data,
                Some(&ae),
                cast,
                p.attacker,
                lf("skillActiveDuration"),
                lf("projectileExplosionRadius"),
            );
        }
        if boom {
            split(&mut commands, &data, p, tf.translation, hit_monster);
        }
        // Piercing: fly on through the target; fragments split at every hit
        // and each target is hit once (`ProjectileFireballLike::UpdateSelf`
        // rolls 0.1–100 against the pierce chance, `ProcessFragmentation` per
        // target hit, `FilterAndAddKnownTargets`).
        if let Some(m) = hit_monster
            && p.left > 0.0
            && p.pierce > 0.0
            && gd_game_core::rng::f32() * 100.0 < p.pierce
        {
            if orbiting.is_none() {
                p.passed.push(m);
            }
            debug!("projectile pierced {m:?} ({:.0} %)", p.pierce);
            continue;
        }
        // `ProjectileFragmenting::ProjectileHit` / `ProjectileStop` with `fragmentOnStop`: one more split as it ends.
        if fragmenting.as_deref().is_some_and(|f| f.on_stop) {
            split(&mut commands, &data, p, tf.translation, None);
        }
        // The projectile ends: its skill's special secondaries run at its
        // position with no targets (`ProjectileBase::SpecialSecondaryActivation`).
        // Its only callers are `ProjectileHoming`'s
        // `ProjectileHit` (state 2) and `ProjectileStop`
        // (state 3), which `ProjectileChain` inherits: the same
        // activation on a hit and at the end of the flight (the state only
        // decides `UpdateSelf`'s `explodeOnMiss` explosion); no other projectile
        // class runs the special pair.
        if data
            .db
            .get(&p.record)
            .is_some_and(|r| matches!(r.class.as_str(), "ProjectileHoming" | "ProjectileChain"))
        {
            activations.write_batch(crate::secondary::Activate::special(
                p.attacker,
                &p.skill,
                p.level_index,
                &p.parent,
                tf.translation,
            ));
        }
        if let Some(f) = p.flight_fx {
            commands.entity(f).try_despawn();
        }
        commands.entity(e).try_despawn();
    }
}

/// `ProjectileBase::ProcessFragmentation` ([`Projectile::fragments`]): each fragment set splits at `at` into
/// its count of fragments flying off level with the ground; `target` (the target hit) is never hit by them.
fn split(
    commands: &mut Commands,
    data: &GameData,
    p: &Projectile,
    at: Vec3,
    target: Option<Entity>,
) {
    for (name, lo, hi) in &p.fragments {
        let Some(frec) = data.db.get(name) else {
            continue;
        };
        let n = if hi > lo {
            gd_game_core::rng::u32(*lo..=*hi)
        } else {
            *lo
        };
        let speed = frec
            .f32("projectileVelocity")
            .unwrap_or(p.vel.length())
            .max(1.0);
        let dist = frec.f32("projectileDistance").unwrap_or(10.0);
        let cast = Casting {
            skill: p.skill.clone(),
            level_index: p.level_index,
            kind: Kind::Projectile,
            point: at,
            target,
            hostile: p.hostile,
            parent: p.parent.clone(),
        };
        let mut a = 0.0f32;
        for _ in 0..n {
            a += (360 / n.max(1)) as f32 + gd_game_core::rng::i32(-30..=30) as f32;
            let dir = Vec3::new(a.to_radians().sin(), 0.0, a.to_radians().cos());
            spawn_projectile(
                commands,
                data,
                frec,
                p.attacker,
                &cast,
                at + dir * 0.3,
                dir * speed,
                dist,
                0.0,
                false,
                &ProjMods::default(),
            );
        }
    }
}
