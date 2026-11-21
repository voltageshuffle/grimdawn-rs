use super::*;

/// A Devastation-like rain of drops ([`Kind::Storm`]): every `skillTargetInterval`
/// around the cast point (`targetingMode` `Point`) or else around the
/// caster where it stands then (`Skill_BuffAttackRadiusDrop`'s `Update`
/// sub_1804D58B0; `gd_sim::skills::TargetingMode`): trap and
/// obelisk firestorms, Valaxteria's. Each tick drops `projectileLaunchNumber`
/// (0 → 1) projectiles: the landing radius is `1 + rand·(skillTargetRadius − 1)`
/// (an annulus, no drops within 1 m), the launch point is the landing spot at
/// `dropHeight ± dropVariation` plus a `dropRadius` disc offset (sub_1804D54C0),
/// and each flies back to its landing spot at its own
/// `projectileVelocity` (`ProjectileBase::MoveStraightLine`).
#[derive(Component)]
pub struct Storm {
    pub(super) cast: Casting,
    pub(super) around_caster: bool,
    pub(super) projectile: String,
    pub(super) left: f32,
    pub(super) interval: f32,
    pub(super) timer: f32,
    pub(super) per_tick: usize,
    pub(super) radius: f32,
    pub(super) height: f32,
    /// `dropVariation`: the launch height is `height` ± this (sub_1804D54C0).
    pub(super) drop_variation: f32,
    /// `dropRadius`: an extra uniform-disc sideways offset of the launch point.
    pub(super) drop_radius: f32,
    pub(super) explosion: f32,
    pub(super) attacker: Entity,
}

/// The landing offset of one storm drop (`Skill_BuffAttackRadiusDrop::Update`
/// sub_1804D58B0): the radius is `1 + u·(radius − 1)` — an annulus, no drop within
/// 1 m of the centre, densest near the inner edge — at the angle `a` (radians).
fn storm_landing(radius: f32, u: f32, a: f32) -> Vec3 {
    let r = 1.0 + u * (radius - 1.0).max(0.0);
    Vec3::new(a.sin() * r, 0.0, a.cos() * r)
}

pub(super) fn storms(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    nav: Res<Nav>,
    mut q: Query<(Entity, &mut Storm)>,
    casters: Query<&Transform, Without<Storm>>,
) {
    let dt = time.delta_secs();
    for (e, mut s) in &mut q {
        s.left -= dt;
        s.timer -= dt;
        if s.left <= 0.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        if s.timer > 0.0 {
            continue;
        }
        s.timer += s.interval;
        let Some(prec) = data.db.get(&s.projectile) else {
            continue;
        };
        let centre = if s.around_caster {
            casters
                .get(s.attacker)
                .map_or(s.cast.point, |t| t.translation)
        } else {
            s.cast.point
        };
        for _ in 0..s.per_tick {
            // `Skill_BuffAttackRadiusDrop::Update` sub_1804D58B0: the
            // landing radius is `1 + rand·(skillTargetRadius − 1)` (an annulus,
            // nothing within 1 m of the centre), the angle uniform over 2π.
            let a = crate::rng::f32() * std::f32::consts::TAU;
            let land = centre + storm_landing(s.radius, crate::rng::f32(), a);
            let ground = nav
                .0
                .ground((land + Vec3::Y).to_array())
                .unwrap_or(centre.y);
            let p = Vec3::new(land.x, ground, land.z);
            // sub_1804D54C0: the launch point is the landing spot at
            // `dropHeight ± dropVariation`, shifted sideways by a uniform disc
            // of `dropRadius` (0 for Devastation); the projectile flies back to
            // the landing spot at its own `projectileVelocity`
            // (`ProjectileBase::MoveStraightLine`, gravity off).
            let h = s.height + (crate::rng::f32() * 2.0 - 1.0) * s.drop_variation;
            let mut start = p + Vec3::Y * h;
            if s.drop_radius > 0.0 {
                let a2 = crate::rng::f32() * std::f32::consts::TAU;
                let r2 = crate::rng::f32() * s.drop_radius;
                start += Vec3::new(a2.cos() * r2, 0.0, a2.sin() * r2);
            }
            let speed = prec.f32("projectileVelocity").unwrap_or(20.0).max(1.0);
            skill_launch_sound(&mut commands, &data, &s.cast.skill, start);
            spawn_projectile(
                &mut commands,
                &data,
                prec,
                s.attacker,
                &s.cast,
                start,
                (p - start).normalize_or(Vec3::NEG_Y) * speed,
                (p - start).length() + 2.0,
                s.explosion,
                true,
                &ProjMods::default(),
            );
        }
    }
}

/// A radius whose `expansionTime` (`SkillProfile` +1168, s; `GetRadiusTime`
/// @0x180548480 gives it x 1000) is above 0: the radius grows from its centre
/// to its full size over that time and each foe is hit as it reaches it. Two
/// classes do this:
/// - `Skill_AttackRadius`: `ActivateNow` @0x1804c5230 records {elapsed 0,
///   time = `expansionTime` x 1000 whole ms, radius, the caster's coords, no
///   one hit} (the Primary secondaries run at once on the full radius,
///   `fire.rs`). Each attack's `TargetResult` then runs the special pair; the
///   `+1576` check in `ActivateNow` is the warm-up flag guarding
///   `SendSkillActiveUpdate` (no attack), so a radius that hits nothing runs
///   no special pair.
/// - `SkillSecondary_AttackRadius`: `ActivateNow` @0x1805505c0 records the
///   same at the activation point (the first target's coords when it is
///   valid, `secondary.rs`); its `TargetResult` @0x1805508e0 hits and runs the
///   ordinary pair only (`ActivateSecondarySkills`, no special pair).
///
/// `Update` (@0x1804c4b40 / @0x18054fed0, the same code), per frame: an entry
/// already past its time goes; else the frame's time is added and the radius
/// is `full x elapsed / time` (full once past it); the foes in it
/// (`GetTargetsInRadius`) not hit yet form one attack (the hits, the ordinary
/// secondaries on them), none: nothing. So the far edge is hit
/// `expansionTime` after the cast.
///
/// No other class grows its hits (every `GetRadiusTime` caller and
/// inline read of +1168): `Skill_AttackRadiusLeap` / `Teleport` /
/// `Skill_AttackBuffRadius` / `Skill_AttackPathCharge` / `Skill_AttackRadius`
/// pass it only to `CreateVisualEffect` (the ring fx's time);
/// `SkillSecondary_OnKillAttackRadius::OnEnemyDeath` @0x1804accb0 and
/// `Skill_OnHitAttackRadius::OnHitActivation` @0x1804fe9c0 store it but take
/// the full-radius list at once, and their `Update`s attack every valid entry
/// on the next frame (the time only paces the debug sphere / list clear);
/// the lightning radii (`Skill_AttackRadiusLightning` /
/// `SkillSecondary_AttackRadiusLightning` / `Skill_AttackRadiusLightning2`)
/// store it beside their bolt, but hit when the bolt finishes or its 5000 ms
/// cap runs out. The debuff radii never read it.
#[derive(Component)]
pub struct ExpandingRadius {
    pub(super) cast: Casting,
    pub(super) caster: Entity,
    pub(super) centre: Vec3,
    pub(super) radius: f32,
    pub(super) time: f32,
    pub(super) elapsed: f32,
    pub(super) hit: Vec<Entity>,
    /// Each attack runs the special pair (`Skill_AttackRadius`).
    pub(super) special: bool,
}

#[allow(clippy::type_complexity)]
pub(super) fn expanding_radii(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut ExpandingRadius)>,
    // The bodies where they stand now (`Transform`, as every other target query).
    all: Query<(Entity, &Transform, &Combatant, crate::combat::VictimSide), Without<Dead>>,
    sides: Query<crate::combat::SideData>,
    mut hits: MessageWriter<SkillHit>,
    mut activations: MessageWriter<crate::secondary::Activate>,
) {
    let dt = time.delta_secs();
    for (e, mut x) in &mut q {
        if x.elapsed > x.time {
            commands.entity(e).try_despawn();
            continue;
        }
        x.elapsed += dt;
        let r = if x.elapsed > 0.0 && x.elapsed <= x.time {
            x.radius * (x.elapsed / x.time)
        } else {
            x.radius
        };
        let side = crate::combat::Side::lookup(&sides, x.caster, x.cast.hostile);
        let (centre, caster) = (x.centre, x.caster);
        let struck: Vec<Entity> = all
            .iter()
            .filter(|(t, tf, cb, v)| {
                *t != caster
                    && side.hits(v)
                    && !x.hit.contains(t)
                    && cb.in_radius(&tf.compute_affine(), centre, r)
            })
            .map(|t| t.0)
            .collect();
        if struck.is_empty() {
            continue;
        }
        for &t in &struck {
            hits.write(SkillHit {
                attacker: caster,
                target: t,
                skill: x.cast.skill.clone(),
                level_index: x.cast.level_index,
                parent: x.cast.parent.clone(),
                scale: 1.0,
                projectile: false,
            });
        }
        x.hit.extend(struck.iter().copied());
        let c = &x.cast;
        activations.write_batch(crate::secondary::Activate::on_hits(
            caster,
            &c.skill,
            c.level_index,
            &c.parent,
            struck,
            centre,
        ));
        if x.special
            && let Some(a) = crate::secondary::Activate::special(
                caster,
                &c.skill,
                c.level_index,
                &c.parent,
                centre,
            )
        {
            activations.write(a);
        }
    }
}

/// A lasting damaging area ([`Kind::Area`]): a `ProjectileAreaEffect` (Game.dll `ProjectileGo`
/// @0x1804047f0, `UpdateSelf` @0x180404ba0, `ProjectileStop` @0x1804049e0). Its life is the `ProjectileInfo`'s whole
/// seconds x 1000 ms (an `int` at +0xCAC: the skill profile's `skillActiveDuration` truncated by the creator,
/// `Skill_AttackProjectileAreaEffect::CreateProjectile` @0x1804bd550 and a grenade's `ProjectileStop` @0x18040e220
/// alike; a 2.5 s Blackwater pool lasts 2 s). The hit timer starts at 0, or at `targetInterval` when the life is longer
/// than one interval, and `ProjectileGo` runs one `UpdateSelf(0)`: a hit at once only for an area no longer than its
/// interval, else the first after one interval. Every update takes the elapsed time off both; while the life is
/// above 0 (or in that first, zero-time update) a hit runs when the hit timer is at or below 0 and re-arms it; else
/// `ProjectileStop`. That timer is the only end: the area outlives its caster and its skill (nothing else stops it).
#[derive(Component)]
pub struct AreaEffect {
    pub(super) cast: Casting,
    pub(super) left: f32,
    pub(super) timer: f32,
    /// `ProjectileGo`'s `UpdateSelf(0)` has not run yet.
    pub(super) fresh: bool,
    pub(super) radius: f32,
    pub(super) attacker: Entity,
    pub(super) fx: Option<Entity>,
    /// Seconds between hits: the projectile's `targetInterval` ms (default
    /// 1000, at least 400; `ProjectileAreaEffect::Load` @0x180404740,
    /// the first hit: the type doc).
    pub(super) interval: f32,
    /// Its projectile record (the sounds).
    pub(super) projectile: Option<String>,
    /// Its caster's side, kept for when the caster is gone
    /// ([`crate::combat::Side::lookup_kept`]).
    pub(super) side: Option<crate::combat::Side>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn areas(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut AreaEffect)>,
    all: Query<(Entity, &Transform, &Combatant, crate::combat::VictimSide), Without<Dead>>,
    sides: Query<crate::combat::SideData>,
    mut hits: MessageWriter<SkillHit>,
    mut activations: MessageWriter<crate::secondary::Activate>,
    data: Res<GameData>,
) {
    for (e, mut a) in &mut q {
        // `ProjectileGo`'s first update takes no time; the faction pack is the caster's then (copied into the
        // `ProjectileInfo` at creation).
        let dt = if std::mem::take(&mut a.fresh) {
            let a = &mut *a;
            crate::combat::Side::lookup_kept(&sides, a.attacker, a.cast.hostile, &mut a.side);
            0.0
        } else {
            time.delta_secs()
        };
        a.left -= dt;
        a.timer -= dt;
        if a.left <= 0.0 && dt > 0.0 {
            if let Some(fx) = a.fx {
                commands.entity(fx).try_despawn();
            }
            // `ProjectileAreaEffect::ProjectileStop` @0x1804049e0.
            let end = a.projectile.as_deref().and_then(|r| {
                crate::combat_sounds::projectile_hit(
                    &data,
                    r,
                    crate::combat_sounds::ProjectileEvent::Hit { exploding: false },
                )
            });
            if let Some(pak) = end {
                commands.write_message(crate::visuals::PlaySound {
                    pak,
                    at: Some(a.cast.point),
                });
            }
            commands.entity(e).try_despawn();
            continue;
        }
        if a.timer > 0.0 {
            continue;
        }
        a.timer = a.interval;
        let mut struck = Vec::new();
        let a = &mut *a;
        let side = match &a.side {
            Some(s) => s.clone(),
            None => {
                crate::combat::Side::lookup_kept(&sides, a.attacker, a.cast.hostile, &mut a.side)
            }
        };
        for (t, tf, cb, v) in &all {
            if t != a.attacker
                && side.hits(&v)
                && cb.in_radius(&tf.compute_affine(), a.cast.point, a.radius)
            {
                hits.write(SkillHit {
                    attacker: a.attacker,
                    target: t,
                    skill: a.cast.skill.clone(),
                    level_index: a.cast.level_index,
                    parent: None,
                    scale: 1.0,
                    projectile: false,
                });
                struck.push(t);
            }
        }
        // Each tick is a `TargetResult` (`ProjectileAreaEffect::UpdateSelf`).
        activations.write_batch(crate::secondary::Activate::on_hits(
            a.attacker,
            &a.cast.skill,
            a.cast.level_index,
            &None,
            struck,
            a.cast.point,
        ));
    }
}

/// A lasting area of `rec`'s projectile (`Skill_AttackProjectileAreaEffect`,
/// a geyser) at `cast.point` for `duration` s, `radius` m (the projectile's
/// `targetInterval` between hits, its flight effect looping there).
pub(crate) fn spawn_area(
    commands: &mut Commands,
    data: &GameData,
    rec: &Record,
    cast: Casting,
    attacker: Entity,
    duration: f32,
    radius: f32,
) {
    let li = cast.level_index;
    let name = rec
        .strs_iter("skillProjectileName")
        .nth(li)
        .or_else(|| rec.strs_iter("skillProjectileName").next())
        .map(|s| s.to_string());
    spawn_area_of(
        commands,
        data,
        name.as_deref(),
        cast,
        attacker,
        duration,
        radius,
    );
}

/// [`spawn_area`] with the `ProjectileAreaEffect` record given (a grenade's `aeProjectileName`).
pub(crate) fn spawn_area_of(
    commands: &mut Commands,
    data: &GameData,
    projectile: Option<&str>,
    cast: Casting,
    attacker: Entity,
    duration: f32,
    radius: f32,
) {
    let prec = projectile.and_then(|p| data.db.get(p));
    let interval = prec
        .as_ref()
        .and_then(|p| p.f32("targetInterval"))
        .unwrap_or(1000.0)
        .max(400.0)
        / 1000.0;
    let record = prec.as_ref().map(|p| p.name.clone());
    let fx = prec
        .and_then(|p| p.str("projectileFlightFX").map(str::to_owned))
        .and_then(|f| {
            spawn_fx_looping(commands, data, &f, Transform::from_translation(cast.point))
        });
    let at = cast.point;
    // `ProjectileGo`: whole seconds; the first hit waits an interval when the area outlasts one.
    let left = duration.max(0.0).trunc();
    let area = commands
        .spawn((
            AreaEffect {
                cast,
                left,
                timer: if left > interval { interval } else { 0.0 },
                fresh: true,
                radius: radius.max(1.0),
                attacker,
                fx,
                interval,
                projectile: record.clone(),
                side: None,
            },
            Transform::from_translation(at),
        ))
        .id();
    // `ProjectileAreaEffect::PlaySwipeSound` @0x180404b50: at its own
    // coordinates, tracked with it.
    if let Some(pak) = record.and_then(|r| crate::combat_sounds::projectile_launch(data, &r)) {
        commands.write_message(crate::visuals::PlaySoundOn { pak, on: area, at });
    }
}
