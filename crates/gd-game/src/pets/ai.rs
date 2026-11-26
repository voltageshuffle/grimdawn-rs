use super::*;
use bevy::ecs::system::SystemParam;
use std::cell::RefCell;

/// `ControllerMonster::Update`'s `WheneverPossible` self-buff poll (s).
pub(super) const SELF_BUFF_POLL: f32 = 4.0;

#[allow(clippy::too_many_arguments)]
pub(super) fn start_pet_cast(
    commands: &mut Commands,
    e: Entity,
    data: &GameData,
    tf: &mut Transform,
    ch: &mut Character,
    a: &PetAttack,
    target: Entity,
    at: Vec3,
) {
    let Some(rec) = data.db.get(&a.skill) else {
        return;
    };
    let special = rec.str("skillSpecialAnimationName").map(str::to_owned);
    if !ch.start_cast(&rec.class, special) {
        ch.start_cast("Skill_AttackSpell", None);
    }
    // `AttackAction::Execute`: the pet turns to its target.
    ch.begin_cast(tf, Some((target, at)), at);
    commands.entity(e).try_insert(Casting {
        skill: a.skill.clone(),
        level_index: a.level_index,
        kind: a.kind,
        point: at,
        target: Some(target),
        hostile: false,
        parent: None,
    });
    if let Some(fx) = rec.str("skillActivatedAuraName") {
        spawn_fx(
            commands,
            data,
            fx,
            Transform::from_translation(tf.translation + Vec3::Y),
        );
    }
}

/// One parameter, so `pet_ai` stays within Bevy's system-parameter limit.
#[derive(SystemParam)]
pub(super) struct PetCharges<'w, 's> {
    starts: Query<'w, 's, &'static gd_game_core::casting::ChargeStarts>,
    charging: Query<
        'w,
        's,
        (
            Has<gd_game_core::casting::Charge>,
            Option<&'static crate::casting::ChargePursuit>,
        ),
    >,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn pet_ai(
    mut commands: Commands,
    scratch: Local<RefCell<gd_sim::nav::PathScratch>>,
    time: Res<Time>,
    nav: Res<Nav>,
    data: Res<GameData>,
    mut landed: MessageReader<crate::combat::HitLanded>,
    mut ally_now: MessageWriter<crate::monster_skills::AllyLanded>,
    // The owner, and the monster a converted pet follows (`combat::JoinedPack::leader`).
    owners: Query<&Transform, (With<Combatant>, Without<Pet>)>,
    mut slot_q: crate::melee::SlotQuery,
    charges: PetCharges,
    (rooted, sleepers): (
        Query<(), With<crate::movement::Rooted>>,
        crate::combat::Sleepers,
    ),
    mut pets: Query<
        (
            Entity,
            &mut Transform,
            &mut Pet,
            &mut Combatant,
            &mut Character,
            Has<Casting>,
            Option<&crate::combat::JoinedPack>,
        ),
        (
            Without<Dead>,
            Without<Player>,
            Without<MonsterAi>,
            Without<crate::movement::Stunned>,
        ),
    >,
    // The characters a pet may fight (`IsFoe`): a pet carries its owner's pack (`Monster::JoinMe`), so an
    // `Allied` monster is none of its foes; a pet a monster's convert joined fights that pack's foes. Pets
    // are added below.
    chars: Query<
        (Entity, &Transform, &Combatant, crate::combat::SideData),
        (
            Or<(With<MonsterAi>, With<Player>)>,
            Without<Dead>,
            Without<Pet>,
        ),
    >,
    // `ChooseBestAllyToBuff` candidates besides the pets.
    allies: Query<
        (
            Entity,
            &Transform,
            &Combatant,
            crate::combat::SideData,
            Option<&gd_game_core::player::Loadout>,
            Option<&gd_game_core::skill_buffs::ReceivedBuffs>,
            Option<&crate::monster_skills::MonsterSkills>,
        ),
        (
            Without<Dead>,
            Without<Pet>,
            Without<gd_game_core::world::Breakable>,
        ),
    >,
    pet_auras: Query<&gd_game_core::skill_buffs::ReceivedBuffs, With<Pet>>,
    mut crowd: Query<&mut crate::movement::CrowdAgent>,
) {
    if pets.is_empty() {
        landed.clear();
        return;
    }
    let dt = time.delta_secs();
    let slots = crate::melee::Slots::new(&mut slot_q, &scratch);
    // Its owner's side, or the pack a monster's convert joined it to.
    let side_of = |j: Option<&crate::combat::JoinedPack>| {
        j.map_or(crate::combat::Side::Player, |j| {
            crate::combat::Side::Monster {
                pack: j.pack.clone(),
                allied: j.allied,
            }
        })
    };
    // Buff-other candidates (`ChooseBestAllyToBuff`): friends of the pet's side with no leader or quest
    // pets, and per pet with an ally buff those that already have it (`IsSkillBuffActive`). Listed only
    // when such a pet exists.
    let mut have: Vec<(Entity, Vec<Entity>)> = Vec::new();
    for (caster, _, pet, ..) in &pets {
        let Some((_, _, inner, _)) = &pet.buff_other else {
            continue;
        };
        let mut on: Vec<Entity> = Vec::new();
        for (e, _, _, _, l, auras, ms) in &allies {
            if l.is_some_and(|l| l.buffs.list.iter().any(|b| &b.0 == inner))
                || auras.is_some_and(|a| a.list.iter().any(|b| &b.buff == inner))
                || ms.is_some_and(|m| m.buffs.list.iter().any(|b| &b.0 == inner))
            {
                on.push(e);
            }
        }
        for (e, _, other, ..) in &pets {
            if other.buffs.list.iter().any(|b| &b.0 == inner)
                || pet_auras
                    .get(e)
                    .is_ok_and(|a| a.list.iter().any(|b| &b.buff == inner))
            {
                on.push(e);
            }
        }
        have.push((caster, on));
    }
    let mut friends: Vec<(Entity, Vec3, f32, f32, crate::combat::Side, bool)> = Vec::new();
    if !have.is_empty() {
        let frac = |cb: &Combatant| cb.life / cb.profile.sheet.life.max(1.0);
        friends.extend(allies.iter().map(|(e, tf, cb, sd, ..)| {
            let by_leader = crate::monster_skills::buff_candidate_by_leader(sd.ai, None);
            (e, tf.translation, cb.life, frac(cb), sd.side(), by_leader)
        }));
        friends.extend(pets.iter().map(|(e, tf, pet, cb, _, _, j)| {
            let by_leader = crate::monster_skills::buff_candidate_by_leader(None, Some(pet));
            (e, tf.translation, cb.life, frac(cb), side_of(j), by_leader)
        }));
    }
    struct OtherPet {
        e: Entity,
        at: Vec3,
        radius: f32,
        side: crate::combat::Side,
        /// 0 without `causesAnger`.
        mult: f32,
        /// Life percent, for the health comparator.
        life: f32,
    }
    let life_pct = |cb: &Combatant| cb.life.max(0.0) / cb.profile.sheet.life.max(1.0) * 100.0;
    let pet_sides: Vec<OtherPet> = pets
        .iter()
        .map(|(e, tf, pet, cb, _, _, j)| OtherPet {
            e,
            at: tf.translation,
            radius: cb.radius,
            side: side_of(j),
            mult: if pet.causes_anger {
                pet.anger_multiplier
            } else {
                0.0
            },
            life: life_pct(cb),
        })
        .collect();
    // `AngerManager::AddAnger` scales anger by the target's `angerMultiplier`; none without `causesAnger`.
    let anger_mult = |m: Entity| -> f32 {
        match chars.get(m) {
            Ok((.., sd)) => sd.ai.map_or(1.0, |ai| {
                if ai.as_target.causes_anger {
                    ai.as_target.anger_multiplier
                } else {
                    0.0
                }
            }),
            Err(_) => pet_sides.iter().find(|p| p.e == m).map_or(1.0, |p| p.mult),
        }
    };
    let position = |m: Entity| -> Option<Vec3> {
        chars
            .get(m)
            .map(|c| c.1.translation)
            .ok()
            .or_else(|| pet_sides.iter().find(|p| p.e == m).map(|p| p.at))
    };
    // The hit-anger gate decays before this frame's hits, so one armed now keeps 500 ms.
    for (.., mut pet, _, _, _, _) in &mut pets {
        if pet.anger_timer > 0.0 {
            pet.anger_timer -= dt;
        }
    }
    // `ControllerMonster::UnderAttack`: only an attacker `InPursuitRange` from the leader counts; at most
    // every 500 ms it gains `AttackedAnger`.
    for h in landed.read() {
        let Ok((_, tf, mut pet, _, _, _, joined)) = pets.get_mut(h.target) else {
            continue;
        };
        let m = anger_mult(h.attacker);
        if h.taunt > 0.0 {
            add_anger(&mut pet.anger, h.attacker, h.taunt * m);
        }
        let leader = joined.map_or(pet.owner, |j| j.leader);
        let in_range = owners
            .get(leader)
            .or_else(|_| owners.get(pet.owner))
            .is_ok_and(|l| {
                position(h.attacker)
                    .is_some_and(|p| pet.ctl.in_pursuit(l.translation, p, tf.translation))
            });
        if !in_range || pet.anger_timer > 0.0 {
            continue;
        }
        pet.anger_timer = 0.5;
        let amount = pet.ctl.attacked_anger * m;
        add_anger(&mut pet.anger, h.attacker, amount);
    }
    for (e, mut tf, mut pet, mut cb, mut ch, casting, joined) in &mut pets {
        let Ok(otf) = owners.get(pet.owner) else {
            commands.entity(e).try_despawn();
            continue;
        };
        // A converted pet follows the monster that converted it while the join lasts.
        let leader = joined.map_or(pet.owner, |j| j.leader);
        let otf = owners.get(leader).unwrap_or(otf);
        let side = side_of(joined);
        // `Monster::JoinMe` / `UnJoinLeader` clear the anger list, the most hated enemy and the target, and
        // leave the state alone.
        if joined.is_some() != pet.joined {
            pet.joined = joined.is_some();
            pet.anger.clear();
            pet.most_hated = None;
            if pet.moving.is_none() {
                pet.path.clear();
            }
            cb.target = None;
        }
        // `Character::CrowdAgentError`: it stops and the state's `PathFailed` runs: FollowLeader's is
        // "DefendLeader" (asks for its slot again in 300..699 ms); Move's is "Idle".
        let mut agent = crowd.get_mut(e).ok();
        if agent.as_mut().is_some_and(|a| a.take_path_failed()) {
            debug!("pet {e}: path failed");
            pet.path.clear();
            pet.melee.note_path_failed();
            if std::mem::take(&mut pet.following) {
                pet.follow_check = 0.3 + gd_game_core::rng::f32() * 0.4;
            }
        }
        // `ControllerMonsterStateStartup`: every spawned pet plays animation type 19 (Spawn) once and does
        // nothing else until its "End" (Idle at once without one); the initial skills follow.
        if pet.startup {
            if !pet.startup_played {
                pet.startup_played = true;
                if ch.play_scripted(gd_sim::anim::AnimType::Spawn) {
                    continue;
                }
            } else if ch.state == CharState::Scripted {
                // PORT: "End" is the scripted animation leaving its state.
                continue;
            }
            pet.startup = false;
        }
        // Only tick while positive: an unconditional write would dirty `Combatant` every frame for
        // its `Changed<Combatant>` consumers; `<= 0.0` means ready either way.
        if cb.cooldown > 0.0 {
            cb.cooldown -= dt;
        }
        pet.find_timer -= dt;
        // `CharacterBio::UpdateHealthAndMana`: up to max − reserve.
        pet.energy = (pet.energy + cb.profile.sheet.energy_regen_per_sec * dt)
            .min(cb.profile.sheet.usable_energy());
        let found_enemy = pet.found_enemy;
        for s in &mut pet.specials {
            s.timer -= dt;
            if found_enemy && s.timeout > 0.0 {
                s.timeout -= dt;
            }
        }
        pet.cooldowns.iter_mut().for_each(|c| c.1 -= dt);
        pet.cooldowns.retain(|c| c.1 > 0.0);
        let owner = otf.translation;
        let ctl = pet.ctl;
        // `InPursuitRange`: `MaxPursuitDistance` of the leader, `MaxYViewDistance` in height.
        let in_pursuit = |p: Vec3, me: Vec3| ctl.in_pursuit(owner, p, me);
        let me = tf.translation;
        let foe_at = |m: Entity| -> Option<(Entity, Vec3, f32)> {
            match chars.get(m) {
                Ok((m, mt, mcb, sd)) => {
                    side.foe(sd.side_ref())
                        .then_some((m, mt.translation, mcb.radius))
                }
                Err(_) => pet_sides
                    .iter()
                    .find(|p| p.e == m && m != e && side.foe(p.side.as_ref()))
                    .map(|p| (p.e, p.at, p.radius)),
            }
        };
        let ignored = |m: Entity| ctl.ignore_sleeping && crate::combat::asleep(&sleepers, m);
        // `AngerUpdate`: every 250 ms forgiving and dropping what `ShouldRemoveEnemy` refuses, then the
        // sight anger × the elapsed ticks.
        pet.anger_tick -= dt;
        if pet.anger_tick < 0.0 {
            let ticks = (1.0 - pet.anger_tick / 0.25).floor().max(1.0);
            pet.anger_tick += 0.25 * ticks;
            let secs = 0.25 * ticks;
            let forgive = ctl.forgive * secs;
            pet.anger.retain_mut(|a| {
                a.1 -= forgive;
                a.1 >= 0.0 && foe_at(a.0).is_some() && !ignored(a.0)
            });
            let (inner, rate, inner_rate) = ctl.sight;
            // `GetCharactersInSphere`: foes within `ViewDistance`; one without anger multiplier gains none
            // (an attack order still makes it a target).
            let seen = chars
                .iter()
                .filter(|c| c.1.translation.distance(me) <= ctl.view && side.foe(c.3.side_ref()))
                .map(|c| (c.0, c.1.translation))
                .chain(
                    pet_sides
                        .iter()
                        .filter(|p| {
                            p.e != e && p.at.distance(me) <= ctl.view && side.foe(p.side.as_ref())
                        })
                        .map(|p| (p.e, p.at)),
                );
            for (m, mt) in seen {
                let mult = anger_mult(m);
                if mult <= 0.0 || !in_pursuit(mt, me) || ignored(m) {
                    continue;
                }
                // `pathingViewDistance` (a failed search passes).
                if ctl.pathing_view > 0.0
                    && let Some(path) = nav.0.find_path(
                        &mut scratch.borrow_mut(),
                        me.to_array(),
                        mt.to_array(),
                        crate::nav::PATH_BUDGET,
                    )
                    && path
                        .windows(2)
                        .map(|w| Vec3::from(w[0]).distance(Vec3::from(w[1])))
                        .sum::<f32>()
                        > ctl.pathing_view
                {
                    continue;
                }
                let d = mt.distance(me);
                let r = if d < inner {
                    inner_rate
                } else {
                    (1.0 - d / ctl.view.max(1e-3) * 0.1) * rate
                };
                add_anger(&mut pet.anger, m, r * secs * mult);
            }
        }
        // Down or getting up: no state is requested until the get-up clip has ended.
        if matches!(ch.state, CharState::KnockedDown | CharState::GettingUp) {
            continue;
        }
        // A weapon charge's warm-up: the Pursue's warm-up ends as the attack begins, the Pursue ends or a
        // stun / death took the charge.
        let (charge, marked) = charges.charging.get(e).unwrap_or((false, None));
        if charge && marked.is_none() {
            continue;
        }
        let mut pursuit = marked.map(|p| (p.skill.clone(), p.bonus, p.cap));
        // The Attack state sees its attack begin and end.
        let busy = casting || matches!(ch.state, CharState::Attack | CharState::Cast);
        if pursuit.is_some() && (busy || !charge || pet.melee.st != crate::melee::St::Pursue) {
            commands
                .entity(e)
                .try_remove::<(gd_game_core::casting::Charge, crate::casting::ChargePursuit)>();
            pursuit = None;
        }
        pet.melee.note_swing(busy);
        pet.buff_poll -= dt;
        pet.choose_poll -= dt;
        let poll = pet.buff_poll <= 0.0;
        if poll {
            pet.buff_poll = SELF_BUFF_POLL;
        }
        let ended = std::mem::replace(&mut pet.was_busy, busy) && !busy;
        // `BuffAllyBehavior`: WhenEnemyIsSeen (2) or the poll (WheneverPossible, 3).
        let has_target = cb.target.is_some();
        pet.found_enemy |= has_target;
        if (ctl.buff_ally == 3 && poll) || (ctl.buff_ally == 2 && has_target && !pet.had_target) {
            pet.ally_due = true;
        }
        if !has_target {
            pet.held = None;
        }
        if busy {
            continue;
        }
        // `ControllerMonsterStateStartup::UseInitialSkillIfSet`: `initialSkillName`, then
        // `initial2SkillName`, used on itself (e.g. the Concussive Rune's opening nova).
        if !pet.initial_casts.is_empty() {
            let (s, li) = pet.initial_casts.remove(0);
            if let Some(r) = data.db.get(&s) {
                debug!(
                    "pet {e}: initial skill {}",
                    s.rsplit('/').next().unwrap_or("")
                );
                let a = PetAttack {
                    skill: s.clone(),
                    level_index: li,
                    kind: kind_of(r),
                    range: 0.0,
                    chance: 100.0,
                    band: None,
                    delay: 0.0,
                    timer: 0.0,
                    timeout: 0.0,
                };
                pet.used(&data, &cb, &s, li);
                let here = tf.translation;
                pet.path.clear();
                start_pet_cast(&mut commands, e, &data, &mut tf, &mut ch, &a, e, here);
                continue;
            }
        }
        // `ChooseBestBuffSelfSkill`: on finding an enemy, at an attack's end with an enemy, and on the
        // `WheneverPossible` poll, the first self toggle not yet on is used.
        let found = has_target && !pet.had_target;
        pet.had_target = has_target;
        // `ChooseBestSkill`: on finding an enemy, at an attack's end and every 200 ms while pursuing.
        let choose = found || ended || pet.choose_poll <= 0.0;
        if choose {
            pet.choose_poll = 0.2;
        }
        let buff_now = match ctl.buff_self {
            0 => false,
            b => found || (ended && has_target) || (b == 3 && poll),
        };
        if buff_now && let Some((s, li, toggle)) = pet.next_self_buff() {
            let Some(r) = data.db.get(&s) else { continue };
            pet.used(&data, &cb, &s, li);
            if toggle {
                debug!("pet {e}: toggles {}", s.rsplit('/').next().unwrap_or(""));
                pet.toggled.push((s, li));
                pet.dirty = true;
                if !r.bool("instantCast")
                    && ch.start_cast(
                        &r.class,
                        r.str("skillSpecialAnimationName").map(str::to_owned),
                    )
                {
                    pet.path.clear();
                    continue;
                }
            } else {
                // A timed self buff is cast on itself.
                debug!("pet {e}: casts {}", s.rsplit('/').next().unwrap_or(""));
                let a = PetAttack {
                    skill: s,
                    level_index: li,
                    kind: Kind::Buff,
                    range: 0.0,
                    chance: 100.0,
                    band: None,
                    delay: 0.0,
                    timer: 0.0,
                    timeout: 0.0,
                };
                let here = tf.translation;
                pet.path.clear();
                start_pet_cast(&mut commands, e, &data, &mut tf, &mut ch, &a, e, here);
                continue;
            }
        }
        // `ChooseBestAllyToBuff` among the candidates that lack the buff, by `BuffAllyTargeting`. A
        // converted pet buffs its joined pack's leaderless monsters, never the player.
        if std::mem::take(&mut pet.ally_due)
            && let Some((skill, li, inner, secs)) =
                pet.buff_other.clone().filter(|b| pet.ready(&b.0))
        {
            let me = tf.translation;
            let buffed = have
                .iter()
                .find(|h| h.0 == e)
                .map_or(&[][..], |h| h.1.as_slice());
            let sphere = crate::monster_skills::BuffSphere {
                side: &side,
                at: me,
                view: ctl.view,
                view_y: ctl.view_y,
            };
            let cands = friends.iter().filter(|f| {
                f.0 != e
                    && crate::monster_skills::buff_ally_candidate(&sphere, f.1, f.4.as_ref(), f.5)
                    && !buffed.contains(&f.0)
            });
            let pick = match ctl.buff_targeting {
                0 => cands.min_by(|a, b| a.1.distance(me).total_cmp(&b.1.distance(me))),
                2 => cands.min_by(|a, b| a.3.total_cmp(&b.3)),
                _ => cands.max_by(|a, b| a.2.total_cmp(&b.2)),
            };
            if let Some(t) = pick {
                debug!(
                    "pet {e}: {} on {}",
                    skill.rsplit('/').next().unwrap_or(""),
                    t.0
                );
                pet.used(&data, &cb, &skill, li);
                ch.turn.look_at(t.1);
                let cast = crate::monster_skills::AllyCast {
                    caster: e,
                    target: t.0,
                    skill,
                    level_index: li,
                    buff: Some((inner, secs)),
                };
                if let Some(c) =
                    crate::monster_skills::cast_on_ally(&mut commands, &data, e, &mut ch, cast)
                {
                    ally_now.write(crate::monster_skills::AllyLanded(c));
                } else {
                    pet.path.clear();
                    continue;
                }
            }
        }
        // `AngerManager::GetNewTarget`: the commanded enemy while valid (`IsEnemyValid`), else every 200 ms
        // (`FindEnemyUpdate`) the valid foes whose anger (× 1.3 for the current target) reaches
        // `AngerTolerance` and that are not ignored asleep, ranked: the angriest; `petTargetGreatestHealth`
        // the highest life percent first; a totem's (`SetDistanceAngerComparator`) the nearest. None while a
        // move command runs. PORT: `petTargetLeastAttacked` and `petTargetLevelRange` take no part (unused
        // in stock data).
        let target = if pet.moving.is_some() || pet.dodging {
            None
        } else {
            let hated = pet
                .most_hated
                .and_then(foe_at)
                .filter(|(_, mt, _)| in_pursuit(*mt, me));
            if hated.is_none() {
                pet.most_hated = None;
            }
            hated.or_else(|| {
                if pet.find_timer > 0.0 {
                    return cb
                        .target
                        .and_then(foe_at)
                        .filter(|(_, mt, _)| in_pursuit(*mt, me));
                }
                pet.find_timer += 0.2 * (1.0 - pet.find_timer / 0.2).floor().max(1.0);
                let totem = ctl.class == gd_game_core::combat::ControllerClass::Totem;
                let life = |m: Entity| {
                    chars.get(m).map_or_else(
                        |_| pet_sides.iter().find(|p| p.e == m).map_or(0.0, |p| p.life),
                        |c| life_pct(c.2),
                    )
                };
                let valid = pet.anger.iter().filter_map(|&(m, v)| {
                    let v = v * if cb.target == Some(m) { 1.3 } else { 1.0 };
                    (v >= ctl.tolerance && !ignored(m))
                        .then(|| foe_at(m))
                        .flatten()
                        .filter(|(_, mt, _)| in_pursuit(*mt, me))
                        .map(|x| (x, v))
                });
                if totem {
                    valid.min_by(|a, b| a.0.1.distance(me).total_cmp(&b.0.1.distance(me)))
                } else if ctl.greatest_health {
                    valid
                        .map(|x| (x, life(x.0.0)))
                        .max_by(|a, b| a.1.total_cmp(&b.1).then(a.0.1.total_cmp(&b.0.1)))
                        .map(|(x, _)| x)
                } else {
                    valid.max_by(|a, b| a.1.total_cmp(&b.1))
                }
                .map(|(x, _)| x)
            })
        };
        cb.target = target.map(|t| t.0);
        let mut want = CharState::Idle;
        let mut goal: Option<Vec3> = None;
        if pet.moving.is_some() || pet.dodging {
            // "Move" / "DodgeAttack": "Idle" at the path's end.
            let speed = run_speed(&ch);
            let mut path = std::mem::take(&mut pet.path);
            if !rooted.contains(e)
                && advance_steered(
                    &mut tf,
                    &mut path,
                    speed,
                    dt,
                    &mut ch.turn,
                    agent.as_deref_mut(),
                )
            {
                want = CharState::Run;
            }
            pet.path = path;
            if pet.path.is_empty() {
                pet.moving = None;
                pet.dodging = false;
                debug!("pet {e}: move order done at {}", tf.translation);
            }
        } else if let Some((t, mt, mcb)) = target {
            pet.following = false;
            let to = mt - tf.translation;
            let d = to.with_y(0.0).length();
            // `CloseEnoughToUseSkill`: both radii + `Skill::GetRange` + 0.5.
            let close = |range: f32| d <= cb.radius + mcb + range + SKILL_USE_TOLERANCE;
            let gap = (d - cb.radius - mcb).max(0.5);
            // `ChooseBestSkill`: specials first, the first slot whose delay and timeout are out, that passes
            // `rand() % 100 < chance` and whose band holds the target. Not tested for reach: one out of
            // reach is held and pursued until the next choice, which keeps a held skill whose class keeps
            // its Pursue (`melee::SkillMove::rechooses`).
            let kept = choose
                && !found
                && !ended
                && pet.held.is_some_and(|i| {
                    let s = &pet.specials[i];
                    !close(s.range)
                        && data
                            .db
                            .get(&s.skill)
                            .is_some_and(|r| !crate::melee::SkillMove::of_class(&r.class).rechooses)
                });
            if choose && !kept {
                pet.held = pet.specials.iter().position(|s| {
                    s.timer <= 0.0
                        && s.timeout <= 0.0
                        && (gd_game_core::rng::u32(..100) as f32) < s.chance
                        && s.band.is_none_or(|(lo, hi)| gap > lo && gap < hi)
                });
            }
            let special = pet.held.filter(|&i| close(pet.specials[i].range));
            if let Some(i) = special {
                pet.held = None;
                pet.specials[i].timer = pet.specials[i].delay;
                let a = pet.specials[i].clone();
                pet.path.clear();
                start_pet_cast(&mut commands, e, &data, &mut tf, &mut ch, &a, t, mt);
                continue;
            }
            // The primary skill, or the plain melee swing (`meleeRange` reach).
            let radii = cb.radius + mcb;
            let primary = pet
                .held
                .map(|i| pet.specials[i].clone())
                .or_else(|| pet.primary.clone())
                .filter(|a| a.range > 0.0);
            let approach = match &primary {
                Some(a) => {
                    let rec = data.db.get(&a.skill);
                    crate::melee::Approach {
                        key: std::sync::Arc::from(a.skill.as_str()),
                        reach: radii + a.range + SKILL_USE_TOLERANCE,
                        range: a.range,
                        melee: !matches!(
                            rec.as_ref().and_then(|r| r.str("distanceProfile")),
                            Some("Short" | "Moderate" | "Long" | "Maximum" | "Boss")
                        ),
                        plain: false,
                        mv: rec.as_ref().map_or_else(Default::default, |r| {
                            crate::melee::SkillMove::of_class(&r.class)
                        }),
                        charge_limit: rec
                            .as_ref()
                            .filter(|r| {
                                matches!(
                                    r.class.as_str(),
                                    "Skill_AttackWeaponCharge"
                                        | "Skill_AttackWeaponBlink"
                                        | "Skill_AttackPathCharge"
                                )
                            })
                            .and_then(|r| r.f32("maxDistanceBuffer").filter(|b| *b != 0.0))
                            .and_then(|b| {
                                charges
                                    .starts
                                    .get(e)
                                    .ok()
                                    .and_then(|s| s.0.get(&a.skill).copied())
                                    .map(|at| (at, b))
                            }),
                    }
                }
                None => crate::melee::Approach {
                    key: "".into(),
                    reach: cb.radius
                        + mcb
                        + gd_game_core::combat::MELEE_RANGE
                        + SKILL_USE_TOLERANCE,
                    range: gd_game_core::combat::MELEE_RANGE,
                    melee: true,
                    plain: true,
                    mv: Default::default(),
                    charge_limit: None,
                },
            };
            let frame = crate::melee::Frame {
                me: e,
                pos: tf.translation,
                radius: cb.radius,
                dt,
                foe: (t, mt, mcb),
                app: &approach,
                can_move: !rooted.contains(e) && ctl.class.can_move(),
                stationary: !ctl.class.can_move(),
                swing_ready: cb.cooldown <= 0.0,
                view_y: ctl.view_y,
                leader: Some(leader),
                walk: pet.walk,
                nav: &nav,
                slots: &slots,
                scratch: &scratch,
            };
            let act = {
                let pet = &mut *pet;
                pet.melee.update(&frame, &mut pet.path)
            };
            if pet.melee.take_swing_reset() {
                cb.cooldown = 0.0;
            }
            // The Pursue's move carries its skill: a weapon charge warms up on it.
            let key = pet.melee.key();
            let pursue = act == crate::melee::Act::Run
                && pet.melee.st == crate::melee::St::Pursue
                && !key.is_empty();
            if !(pursue && pursuit.as_ref().is_some_and(|p| p.0 == key)) {
                if pursuit.take().is_some() {
                    commands
                        .entity(e)
                        .try_remove::<(gd_game_core::casting::Charge, crate::casting::ChargePursuit)>();
                }
                if pursue {
                    let li = pet
                        .primary
                        .iter()
                        .chain(pet.specials.iter())
                        .find(|a| a.skill == key)
                        .map_or(0, |a| a.level_index);
                    crate::casting::begin_pursuit_charge(&mut commands, &data, e, t, key, li, &tf);
                }
            }
            match act {
                crate::melee::Act::Return => {
                    pet.melee.leave(&slots, e);
                    pet.path.clear();
                }
                crate::melee::Act::Swing => {
                    pet.path.clear();
                    ch.begin_swing(&tf, Some((t, mt)), mt);
                    want = CharState::Attack;
                    // `ResetSwingTimer`: a uniform draw in the controller's swing pause.
                    cb.cooldown = pet.ctl.swing_pause.0
                        + gd_game_core::rng::f32()
                            * (pet.ctl.swing_pause.1 - pet.ctl.swing_pause.0).max(0.0);
                }
                crate::melee::Act::Stand => {
                    if pet.melee.in_attack() {
                        ch.turn.look_at(mt);
                        // Once the swing timer has run out, wherever the target now is.
                        if let Some(a) = primary
                            && cb.cooldown <= 0.0
                        {
                            pet.path.clear();
                            start_pet_cast(&mut commands, e, &data, &mut tf, &mut ch, &a, t, mt);
                            cb.cooldown = pet.ctl.swing_pause.0
                                + gd_game_core::rng::f32()
                                    * (pet.ctl.swing_pause.1 - pet.ctl.swing_pause.0).max(0.0);
                            continue;
                        }
                    }
                }
                crate::melee::Act::Run => {
                    // A warmed-up weapon charge: the pace's clip at the warmed-up run speed.
                    let paced = crate::combat::pace_speed(&ch, pet.melee.pace(), &pet.walk);
                    let (speed, state, rate) = match &pursuit {
                        Some((_, bonus, cap)) if paced.1 != CharState::Idle => {
                            let (s, st, r) =
                                crate::combat::charge_move(&ch, paced.1, Some((*bonus, *cap)));
                            (s, st, Some(r))
                        }
                        _ => paced,
                    };
                    let mut path = std::mem::take(&mut pet.path);
                    if !rooted.contains(e)
                        && advance_steered(
                            &mut tf,
                            &mut path,
                            speed,
                            dt,
                            &mut ch.turn,
                            agent.as_deref_mut(),
                        )
                    {
                        ch.move_rate = rate;
                        want = state;
                    }
                    pet.path = path;
                }
            }
        } else {
            if pet.melee.engaged() {
                pet.melee.leave(&slots, e);
                pet.path.clear();
            }
            // `FollowLeader` / `DefendLeader`: teleport when too far, else every 300–700 ms walk back to the
            // defence slot.
            let far = tf.translation.distance(owner);
            if !ctl.class.can_move() {
                // A stationary controller's FollowLeader / DefendLeader go "Idle".
                pet.following = false;
                pet.path.clear();
            } else if slots.take_lost(e) && pet.following {
                // `ControllerMonsterStateFollowLeader::LostSlot` → "DefendLeader" (checks again in 300..400 ms).
                pet.following = false;
                pet.path.clear();
                pet.follow_check = 0.3 + gd_game_core::rng::f32() * 0.1;
            } else if ctl.teleport > 0.0 && far > ctl.teleport {
                if let Some(at) = slots.defense(leader, e, &nav) {
                    tf.translation = at;
                    pet.path.clear();
                    pet.following = false;
                }
            } else {
                pet.follow_check -= dt;
                if pet.follow_check <= 0.0 {
                    pet.follow_check = 0.3 + gd_game_core::rng::f32() * 0.4;
                    // `RequestDefenseSlot`; run there when more than the pet's radius away.
                    pet.slot = slots.defense(leader, e, &nav);
                    if pet
                        .slot
                        .is_some_and(|s| s.distance(tf.translation) > cb.radius)
                    {
                        pet.following = true;
                        pet.path.clear();
                        pet.repath = 0.0;
                    }
                }
                if pet.following {
                    match pet.slot {
                        Some(s) if s.distance(tf.translation) > 0.025 => goal = Some(s),
                        _ => {
                            pet.following = false;
                            pet.path.clear();
                        }
                    }
                }
            }
        }
        if let Some(g) = goal {
            pet.repath -= dt;
            if pet.repath <= 0.0 || pet.path.is_empty() {
                pet.repath = 0.6;
                pet.path = nav
                    .0
                    .find_path(
                        &mut scratch.borrow_mut(),
                        tf.translation.to_array(),
                        g.to_array(),
                        crate::nav::PATH_BUDGET,
                    )
                    .map(|p| p.into_iter().skip(1).map(Vec3::from).collect())
                    .unwrap_or_default();
                // Nothing to walk: the follow ends (`FollowLeader` → "DefendLeader") rather than searching
                // again every frame.
                if pet.path.is_empty() {
                    pet.following = false;
                }
            }
            let speed = run_speed(&ch);
            let mut path = std::mem::take(&mut pet.path);
            if !rooted.contains(e)
                && advance_steered(
                    &mut tf,
                    &mut path,
                    speed,
                    dt,
                    &mut ch.turn,
                    agent.as_deref_mut(),
                )
            {
                want = CharState::Run;
            }
            pet.path = path;
        }
        if let Some(gy) = nav
            .0
            .ground([tf.translation.x, tf.translation.y + 0.5, tf.translation.z])
        {
            tf.translation.y = gy;
        }
        if ch.state != want {
            ch.state = want;
        }
    }
}
