use super::*;

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
            // `Skill_BuffAttackRadiusDrop::Update`: landing radius `1 + rand·(skillTargetRadius − 1)`, angle uniform.
            let a = gd_game_core::rng::f32() * std::f32::consts::TAU;
            let land = centre + storm_landing(s.radius, gd_game_core::rng::f32(), a);
            let ground = nav
                .0
                .ground((land + Vec3::Y).to_array())
                .unwrap_or(centre.y);
            let p = Vec3::new(land.x, ground, land.z);
            // `Skill_BuffAttackRadiusDrop::DropLaunchPoint`: the landing spot at `dropHeight ± dropVariation`, shifted
            // by a uniform disc of `dropRadius`; the projectile flies straight back at its `projectileVelocity`.
            let h = s.height + (gd_game_core::rng::f32() * 2.0 - 1.0) * s.drop_variation;
            let mut start = p + Vec3::Y * h;
            if s.drop_radius > 0.0 {
                let a2 = gd_game_core::rng::f32() * std::f32::consts::TAU;
                let r2 = gd_game_core::rng::f32() * s.drop_radius;
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

#[allow(clippy::type_complexity)]
pub(super) fn expanding_radii(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut ExpandingRadius)>,
    // The bodies where they stand now (`Transform`).
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

/// A lasting damaging area ([`Kind::Area`]): a `ProjectileAreaEffect` (`ProjectileGo`, `UpdateSelf`, `ProjectileStop`).
/// Its life is the creator's duration in whole seconds (`lifeMs`; a 2.5 s pool lasts 2 s). The hit timer starts at
/// 0, or at `targetInterval` when the life is longer than one interval, and `ProjectileGo` runs one `UpdateSelf(0)`:
/// a hit at once only for an area no longer than its interval. Each update takes the elapsed time off both; while
/// the life is above 0 (or in that first update) a hit runs when the hit timer is at or below 0 and re-arms it; else
/// `ProjectileStop`. The area outlives its caster and its skill.
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
    /// Seconds between hits: `targetInterval` ms (default 1000, at least 400; `ProjectileAreaEffect::Load`).
    pub(super) interval: f32,
    pub(super) projectile: Option<String>,
    /// The caster's side, kept for when the caster is gone.
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
        // The first update takes no time; the faction pack is the caster's at creation.
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
            // `ProjectileAreaEffect::ProjectileStop`.
            let end = a.projectile.as_deref().and_then(|r| {
                crate::combat_sounds::projectile_hit(
                    &data,
                    r,
                    crate::combat_sounds::ProjectileEvent::Hit { exploding: false },
                )
            });
            if let Some(pak) = end {
                commands.write_message(gd_game_core::visuals::PlaySound {
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

/// A lasting area of `rec`'s projectile (`Skill_AttackProjectileAreaEffect`) at `cast.point` for `duration` s,
/// `radius` m.
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
    // `ProjectileAreaEffect::PlaySwipeSound`: at its own coordinates, tracked with it.
    if let Some(pak) = record.and_then(|r| crate::combat_sounds::projectile_launch(data, &r)) {
        commands.write_message(gd_game_core::visuals::PlaySoundOn { pak, on: area, at });
    }
}
