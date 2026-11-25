use super::*;

/// Sight anger, `ControllerMonster::AngerUpdate`: every living monster runs a 250 ms tick (phase
/// `rand() % 251` ms). Each tick it gathers the foes within `ViewDistance` (`GetCharactersInSphere`:
/// |Δy| ≤ `MaxYViewDistance` when set, `CausesAnger`), keeps those whose nav path is no longer than
/// `pathingViewDistance` when set (a failed search passes), and adds `ProcessAngerTarget` per second:
/// nothing outside `InPursuitRange`, else `InnerSightAngerRate` within `InnerViewDistance`, else
/// `SightAngerRate` × (1 − 0.1 × distance / `ViewDistance`). No line-of-sight test. The same tick runs
/// `AngerManager::Update` (forgiving, `ShouldRemoveEnemy`).
#[allow(clippy::too_many_arguments)]
pub(super) fn sight_anger(
    time: Res<Time>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    nav: Res<Nav>,
    mut monsters: Query<(Entity, &GlobalTransform, &mut MonsterAi, Has<Allied>), Without<Dead>>,
    foes: Query<
        (Entity, &GlobalTransform, Option<&crate::pets::Pet>),
        (
            Or<(With<Player>, With<crate::pets::Pet>)>,
            Without<Dead>,
            Without<JoinedPack>,
        ),
    >,
    pets: Query<(), With<crate::pets::Pet>>,
    joined: Query<(Entity, &GlobalTransform, &crate::pets::Pet, &JoinedPack), Without<Dead>>,
    sleepers: super::ai::Sleepers,
    living: crate::anger_rolls::Living,
) {
    let dt = time.delta_secs();
    let seen: Vec<(Entity, Vec3, f32)> = foes
        .iter()
        .map(|(e, t, p)| (e, t.translation(), p.map_or(1.0, |p| p.anger_multiplier)))
        .collect();
    // The monsters a foe-faction monster may see: the `causesAnger` ones (`GetCharactersInSphere`), anger
    // scaled by `angerMultiplier` (`AngerManager::AddAnger`). Built only on a frame where a monster ticks.
    let ticking = monsters.iter().any(|m| m.2.anger_tick - dt < 0.0);
    let mut packs = Vec::new();
    let mut rivals: Vec<(Entity, Vec3, f32, Option<usize>)> = Vec::new();
    if ticking {
        rivals.extend(
            monsters
                .iter()
                .filter(|m| m.2.as_target.causes_anger && m.2.faction.is_some())
                .map(|m| {
                    (
                        m.0,
                        m.1.translation(),
                        m.2.as_target.anger_multiplier,
                        pack_index(&mut packs, &m.2.faction),
                    )
                }),
        );
        rivals.extend(joined.iter().filter(|j| j.3.pack.is_some()).map(|j| {
            (
                j.0,
                j.1.translation(),
                j.2.anger_multiplier,
                pack_index(&mut packs, &j.3.pack),
            )
        }));
    }
    let index = gd_sim::grid::Grid::new(grid::CELL, rivals.iter().map(|r| grid::xz(r.1)));
    // `ShouldRemoveEnemy`'s friend test of a monster on an anger list.
    let rival_of: bevy::ecs::entity::EntityHashMap<usize> =
        rivals.iter().enumerate().map(|(i, r)| (r.0, i)).collect();
    let (mut near, mut mine): (Vec<usize>, Vec<(Entity, Vec3, f32)>) = (Vec::new(), Vec::new());
    for (me_e, gt, mut ai, allied) in &mut monsters {
        ai.anger_tick -= dt;
        if ai.anger_tick >= 0.0 {
            continue;
        }
        let ticks = (1.0 - ai.anger_tick / 0.25).floor().max(1.0);
        ai.anger_tick += 0.25 * ticks;
        let secs = 0.25 * ticks;
        // `AngerManager::Update`: every entry loses `ForgiveRate` per second; below 0 it is dropped.
        let forgive = ai.forgive * secs;
        if forgive > 0.0 && !ai.anger.is_empty() {
            ai.anger.iter_mut().for_each(|a| a.1 -= forgive);
            ai.anger.retain(|a| a.1 >= 0.0);
        }
        // `ignoreSleepingEnemies` (`ShouldRemoveEnemy`): runs after this tick's `AddAnger`, so a sleeper in
        // view gains nothing either.
        let ignore_sleeping = ai.ignore_sleeping;
        if ignore_sleeping {
            ai.anger.retain(|a| !super::ai::asleep(&sleepers, a.0));
        }
        // `ShouldRemoveEnemy`: gone, dead, or a monster / converted pet whose pack is now a friend.
        // PORT: the player's side is never dropped as a friend.
        {
            let ai = &mut *ai;
            let faction = &ai.faction;
            ai.anger.retain(|a| {
                let friend = rival_of.get(&a.0).is_some_and(|&i| {
                    match (faction, rivals[i].3.and_then(|p| packs[p].as_ref())) {
                        (Some(mine), Some(theirs)) => mine.is_friend(theirs),
                        _ => false,
                    }
                });
                living.contains(a.0) && !friend
            });
        }
        // The sight anger first, then `AngerManager::Update`'s snapshot (`GetAngerDiff`'s base).
        'sight: {
            if ai.view <= 0.0 {
                break 'sight;
            }
            let me = gt.translation();
            let (inner, rate, inner_rate) = ai.sight;
            // `ShouldIgnorePets`.
            let skip_pets = crate::anger_rolls::ignores_pets(&ai, |e| pets.contains(e), &living);
            // The player and their pets for a hostile monster, and foe-faction monsters (`IsFoe(me, it)`).
            let players: &[(Entity, Vec3, f32)] = if allied { &[] } else { &seen };
            mine.clear();
            if ai.faction.is_some() {
                index.near(grid::xz(me), grid::pad(ai.view), &mut near);
                mine.extend(
                    near.iter()
                        .map(|&i| &rivals[i])
                        .filter(|r| {
                            r.0 != me_e
                                && r.1.distance_squared(me) <= ai.view * ai.view
                                && r.3.is_some_and(|i| {
                                    gd_game_core::monsters::FactionPack::foes(
                                        &ai.faction,
                                        &packs[i],
                                    )
                                })
                        })
                        .map(|r| (r.0, r.1, r.2)),
                );
            }
            for (e, p, m) in players.iter().chain(&mine) {
                if (skip_pets && pets.contains(*e))
                    || (ignore_sleeping && super::ai::asleep(&sleepers, *e))
                {
                    continue;
                }
                let d = p.distance(me);
                if d > ai.view || !ai.in_pursuit(me, *p) {
                    continue;
                }
                if ai.pathing_view > 0.0
                    && let Some(path) = nav.0.find_path(
                        &mut scratch,
                        me.to_array(),
                        p.to_array(),
                        crate::nav::PATH_BUDGET,
                    )
                    && path
                        .windows(2)
                        .map(|w| Vec3::from(w[0]).distance(Vec3::from(w[1])))
                        .sum::<f32>()
                        > ai.pathing_view
                {
                    continue;
                }
                let r = if d < inner {
                    inner_rate
                } else {
                    (1.0 - d / ai.view * 0.1) * rate
                };
                ai.add_anger(*e, r * secs * m);
            }
        }
        ai.anger_ticked(forgive);
    }
}

/// `ControllerMonster::UnderAttack`: only when the attacker or its master is `InPursuitRange`
/// ([`MonsterAi::in_pursuit`]): at most every 500 ms the attacker gets (1 − `petAngerTransference`) ×
/// `AttackedAnger` and the master the rest, and the state's `Attacked` runs (`DefaultAttackedResponse`:
/// `LeadOnDamage`, FleeOnDamage; "AlertBeforePursue"'s is empty).
///
/// Distress calls (`Character::UnderAttack`): when `ShouldUseDistressCall` allows (the record's
/// `distressCallWhenDying`, default 1; calls left; timer run out) a call is spent and the timer re-armed
/// (`distressCallTime` ms, default 5000; `maxDistressCalls`, default 1). `CharacterIsDying` always calls
/// once more on the killer. `DoDistressCall` reaches every friend within `distressCallRange`; each runs
/// `ControllerMonster::AllyAttacked`: when the attacker is in its pursuit range and
/// `ShouldCareAboutDistressCall` holds (RespondToAll; RespondToSameRace: a shared
/// `characterRacialProfile` entry; RespondToSameGroup: always equal outside the Shattered Realm) it adds
/// `AllyAttackedAnger`. An idle monster also answers (`DefaultAllyAttackedResponse`) at
/// `ChanceToRespondToDistressCall` % (default 100) with 100 anger when the attacker is not yet its enemy.
/// The timers decay in [`monster_ai`] before this system, so a timer armed this frame keeps its interval.
pub(super) fn anger_from_hits(
    mut landed: MessageReader<HitLanded>,
    mut killed: MessageReader<MonsterKilled>,
    mut monsters: Query<(Entity, &GlobalTransform, &mut MonsterAi, Has<Dead>)>,
    tfs: Query<&GlobalTransform>,
    pets: Query<&crate::pets::Pet>,
    mut calls: Local<Vec<(Entity, Entity)>>,
    living: crate::anger_rolls::Living,
) {
    if landed.is_empty() && killed.is_empty() {
        return;
    }
    // An attacker's `angerMultiplier`; none without `causesAnger` (`ShouldRemoveEnemy`).
    let mult = |monsters: &Query<(Entity, &GlobalTransform, &mut MonsterAi, Has<Dead>)>,
                e: Entity| match pets.get(e) {
        Ok(p) => p.anger_multiplier,
        Err(_) => monsters.get(e).map_or(1.0, |(_, _, ai, _)| {
            let t = ai.as_target;
            if t.causes_anger {
                t.anger_multiplier
            } else {
                0.0
            }
        }),
    };
    // (caller, attacker)
    calls.clear();
    for h in landed.read() {
        let m_att = mult(&monsters, h.attacker);
        let m_owner = pets
            .get(h.attacker)
            .map_or(m_att, |p| mult(&monsters, p.owner));
        let Ok((_, mgt, mut ai, _)) = monsters.get_mut(h.target) else {
            continue;
        };
        if h.taunt > 0.0 {
            ai.add_anger(h.attacker, h.taunt * m_att);
        }
        let d = &mut ai.distress;
        if d.on && d.calls_left != 0 && d.timer <= 0.0 {
            calls.push((h.target, h.attacker));
            d.calls_left = (d.calls_left - 1).max(0);
            d.timer = d.interval;
        }
        // While ignoring pets a pet's hit is not passed on to its master: the pet gets both shares.
        let ignoring = crate::anger_rolls::ignores_pets(&ai, |e| pets.contains(e), &living);
        let master = pets
            .get(h.attacker)
            .map_or(h.attacker, |p| if ignoring { h.attacker } else { p.owner });
        let me = mgt.translation();
        let in_range = {
            let ai = &*ai;
            let near = |e: Entity| tfs.get(e).is_ok_and(|t| ai.in_pursuit(me, t.translation()));
            near(h.attacker) || near(master)
        };
        if !in_range {
            continue;
        }
        if ai.anger_timer <= 0.0 {
            ai.anger_timer = 0.5;
            let (amount, t) = ai.anger_cfg;
            let m_master = if master == h.attacker { m_att } else { m_owner };
            ai.add_anger(h.attacker, (1.0 - t) * amount * m_att);
            ai.add_anger(master, t * amount * m_master);
        }
        if ai.alerting() {
            continue;
        }
        // `DefaultAttackedResponse`: an attacker that is not the current target makes a `LeadOnDamage`
        // monster call its followers.
        if ai.enemy != Some(h.attacker)
            && ai.patrol.settings.leader == crate::patrol::LeaderBehavior::OnDamage
        {
            ai.patrol.call = true;
        }
        ai.flee.trigger(crate::flee::Behavior::Damage, h.attacker);
    }
    for k in killed.read() {
        if let Some((by, _)) = k.killer
            && monsters.contains(k.entity)
        {
            calls.push((k.entity, by));
        }
    }
    for &(caller, attacker) in calls.iter() {
        let Ok((_, ctf, cai, _)) = monsters.get(caller) else {
            continue;
        };
        let (from, range, race) = (ctf.translation(), cai.distress.range, cai.race.clone());
        let faction = cai.faction.clone();
        let Ok(apos) = tfs.get(attacker).map(|t| t.translation()) else {
            continue;
        };
        let m = mult(&monsters, attacker);
        for (e, tf, mut ai, dead) in &mut monsters {
            if e == caller
                || dead
                || tf.translation().distance(from) > range
                || !ai.in_pursuit(tf.translation(), apos)
            {
                continue;
            }
            let cares = match ai.respond_behavior {
                1 => ai
                    .race
                    .iter()
                    .any(|r| race.iter().any(|c| c.eq_ignore_ascii_case(r))),
                _ => true,
            };
            // Friends only (`FactionPack::is_friend`; a monster without a pack answers anyone).
            let friend = match (&ai.faction, &faction) {
                (Some(a), Some(b)) => a.is_friend(b),
                _ => true,
            };
            if !cares || !friend {
                continue;
            }
            let ally = ai.ally_anger;
            ai.add_anger(attacker, ally * m);
            if !ai.aggro
                && (gd_game_core::rng::u32(..100) as f32) < ai.respond
                && ai.enemy != Some(attacker)
            {
                ai.add_anger(attacker, 100.0 * m);
            }
        }
    }
}

/// Animation speeds follow the profile's capped run / attack / cast speeds, reaching the playing clip at
/// once (`Character::ForceSpeedUpdate` → `AnimationSet::AdjustAnimation`).
pub(super) fn sync_speeds(mut q: Query<(&Combatant, &mut Character), Changed<Combatant>>) {
    for (cb, mut ch) in &mut q {
        let [run, attack, cast] = cb.profile.speeds;
        if ch.run_speed != run || ch.attack_speed != attack || ch.spell_speed != cast {
            ch.run_speed = run;
            ch.attack_speed = attack;
            ch.spell_speed = cast;
            ch.force_speed_update();
        }
    }
}
