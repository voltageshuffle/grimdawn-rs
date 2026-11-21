use super::*;

/// Sight anger, `ControllerMonster::AngerUpdate` @0x18013bd20:
/// every living monster (aggroed or not; `ControllerMonster::Update` calls
/// it outside the dying states) runs a 250 ms tick, its phase randomised at
/// construction (`rand() % 251` ms). Each tick it gathers the characters
/// within `ViewDistance` (a sphere, `GetCharactersInSphere` @0x18013afe0:
/// alive, foes, |Δy| ≤ `MaxYViewDistance` when set, `CausesAnger`), keeps
/// those whose nav path is no longer than `pathingViewDistance` when that
/// is set (a failed path search passes), and adds
/// `ProcessAngerTarget` @0x180139600 × the elapsed ticks: nothing outside
/// `InPursuitRange`, else `InnerSightAngerRate` within `InnerViewDistance`,
/// else `SightAngerRate` × (1 − 0.1 × distance / `ViewDistance`), per
/// second. There is no line-of-sight test anywhere on this path. The same
/// tick runs `AngerManager::Update` (forgiving) with the elapsed time.
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
    // A converted pet is seen as a creature of its joined pack (`JoinedPack`).
    joined: Query<(Entity, &GlobalTransform, &crate::pets::Pet, &JoinedPack), Without<Dead>>,
    sleepers: super::ai::Sleepers,
) {
    let dt = time.delta_secs();
    let seen: Vec<(Entity, Vec3, f32)> = foes
        .iter()
        .map(|(e, t, p)| (e, t.translation(), p.map_or(1.0, |p| p.anger_multiplier)))
        .collect();
    // The monsters a monster of a foe faction may see: the `causesAnger` ones (`GetCharactersInSphere` @0x18013afe0
    // skips Character +8376 == 0: a defense turret, a training dummy), anger scaled by their `angerMultiplier`
    // (`AngerManager::AddAnger` @0x18001dfa0). Built only on a frame where a
    // monster ticks; each ticking monster checks the distance before the faction. A rival names its pack by index
    // (`pack_index`: one `Arc` per distinct pack); the grid (`combat::grid`) lists the ones near a monster in
    // this list's order.
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
    let (mut near, mut mine): (Vec<usize>, Vec<(Entity, Vec3, f32)>) = (Vec::new(), Vec::new());
    for (me_e, gt, mut ai, allied) in &mut monsters {
        ai.anger_tick -= dt;
        if ai.anger_tick >= 0.0 {
            continue;
        }
        // Whole 250 ms periods elapsed (at least one).
        let ticks = (1.0 - ai.anger_tick / 0.25).floor().max(1.0);
        ai.anger_tick += 0.25 * ticks;
        let secs = 0.25 * ticks;
        // Forgiving (`AngerManager::Update` @0x18001dbb0): every
        // entry loses `ForgiveRate` per second; below 0 it is dropped.
        let forgive = ai.forgive * secs;
        if forgive > 0.0 && !ai.anger.is_empty() {
            ai.anger.iter_mut().for_each(|a| a.1 -= forgive);
            ai.anger.retain(|a| a.1 >= 0.0);
        }
        // `ignoreSleepingEnemies`: the same `Update` drops a sleeping non-player (`ShouldRemoveEnemy` @0x18001eec0);
        // it runs after this tick's `AddAnger`, so one in view gains nothing either.
        let ignore_sleeping = ai.ignore_sleeping;
        if ignore_sleeping {
            ai.anger.retain(|a| !super::ai::asleep(&sleepers, a.0));
        }
        // The sight anger first, then `AngerManager::Update`'s snapshot (`GetAngerDiff`'s base).
        'sight: {
            if ai.view <= 0.0 {
                break 'sight;
            }
            let me = gt.translation();
            let (inner, rate, inner_rate) = ai.sight;
            // `ShouldIgnorePets` leaves pets out of the sphere (anger_rolls.rs).
            let skip_pets = crate::anger_rolls::ignores_pets(&ai, |e| pets.contains(e));
            // The player and their pets for a hostile monster, and the monsters of foe factions in view (`IsFoe(me, it)`).
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
                                    crate::monsters::FactionPack::foes(&ai.faction, &packs[i])
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

/// `ControllerMonster::UnderAttack` @0x18013c8e0: only when the
/// attacker or its master (a pet's owner; else the attacker again) is
/// `InPursuitRange` ([`MonsterAi::in_pursuit`]) does anything happen: at most
/// every 500 ms the attacker gets (1 − `petAngerTransference`) ×
/// `AttackedAnger` and the master the rest, and on every such hit the current
/// state's `Attacked` runs (`DefaultAttackedResponse`: `LeadOnDamage`,
/// FleeOnDamage; the "AlertBeforePursue" state's is empty). A sniped monster
/// beyond its leash gains no anger. Anger decay is in [`sight_anger`].
///
/// Distress calls: `Character::UnderAttack` @0x180068be0 calls
/// for help when `ShouldUseDistressCall` @0x18005a870 allows (the flag,
/// calls left, its timer run out), then spends a call and re-arms the timer
/// (`distressCallTime` ms, default 5000; `maxDistressCalls`, default 1).
/// The flag is the record's `distressCallWhenDying` (default 1): `Character::
/// Load` @0x180054e70 stores it over `distressCall` in the same byte.
/// `CharacterIsDying` @0x180068db0 always calls once more on the killer (its
/// gate, Character +0x45A4, is only ever set to 1 by the constructor).
/// `DoDistressCall` @0x18005a8a0 reaches every friend within
/// `distressCallRange`; each runs `ControllerMonster::AllyAttacked`
/// @0x18013ca50: when the attacker is in its pursuit range and
/// `ShouldCareAboutDistressCall` @0x18013ba90 holds (`DistressResponseBehavior`
/// RespondToAll; RespondToSameRace: a shared `characterRacialProfile` entry;
/// RespondToSameGroup: equal `Character` distress groups, which only the
/// Shattered Realm generator sets (`Monster::InitialUpdate`), so always
/// equal here) it adds `AllyAttackedAnger` for the attacker. An idle monster
/// also answers (`DefaultAllyAttackedResponse` @0x18014d390, Wander state):
/// at `ChanceToRespondToDistressCall` % (default 100), when the attacker is
/// not already its enemy and is valid, 100 anger and pursuit.
pub(super) fn anger_from_hits(
    time: Res<Time>,
    mut landed: MessageReader<HitLanded>,
    mut killed: MessageReader<MonsterKilled>,
    mut monsters: Query<(Entity, &GlobalTransform, &mut MonsterAi, Has<Dead>)>,
    tfs: Query<&GlobalTransform>,
    pets: Query<&crate::pets::Pet>,
) {
    let dt = time.delta_secs();
    for (_, _, mut ai, _) in &mut monsters {
        if ai.anger_timer > 0.0 {
            ai.anger_timer -= dt;
        }
        if ai.distress.timer > 0.0 {
            ai.distress.timer -= dt;
        }
    }
    if landed.is_empty() && killed.is_empty() {
        return;
    }
    // An attacking monster (an `Allied` guard) is angered at by its `angerMultiplier`, and not at all without
    // `causesAnger` (`AngerManager::AddAnger` @0x18001dfa0 / `ShouldRemoveEnemy` @0x18001eec0 drops it: a turret).
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
    let mut calls: Vec<(Entity, Entity)> = Vec::new();
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
        // While ignoring pets a pet's hit is not passed on to its master
        // (`UnderAttack`, anger_rolls.rs): the pet gets both shares.
        let ignoring = crate::anger_rolls::ignores_pets(&ai, |e| pets.contains(e));
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
        // `DefaultAttackedResponse`: an attacker that is not the
        // current target makes a `LeadOnDamage` monster call its followers.
        if ai.enemy != Some(h.attacker)
            && ai.patrol.settings.leader == crate::patrol::LeaderBehavior::OnDamage
        {
            ai.patrol.call = true;
        }
        // FleeOnDamage (`DefaultAttackedResponse`).
        ai.flee.trigger(crate::flee::Behavior::Damage, h.attacker);
    }
    for k in killed.read() {
        if let Some((by, _)) = k.killer
            && monsters.contains(k.entity)
        {
            calls.push((k.entity, by));
        }
    }
    for (caller, attacker) in calls {
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
            // `DoDistressCall` reaches the caller's friends only (`FactionPack::is_friend`, as `monster_ai`'s
            // `friends_of`; a monster without a pack answers anyone).
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
                && (crate::rng::u32(..100) as f32) < ai.respond
                && ai.enemy != Some(attacker)
            {
                ai.add_anger(attacker, 100.0 * m);
            }
        }
    }
}

/// Animation speeds follow the profile's capped run / attack / cast speeds
/// ([`gd_sim::actor::Profile::speeds`]; the profile includes gear, skills and buffs).
/// A changed speed reaches the playing clip in place
/// ([`Character::force_speed_update`], `Character::ForceSpeedUpdate`
/// @0x180068ca0 → `AnimationSet::AdjustAnimation` @0x180024f20): a run-speed
/// buff gained mid-run speeds the run clip up at once, not at the next
/// stop-start (slows already scale the advance's dt).
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
