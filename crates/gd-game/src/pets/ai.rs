use super::*;
use bevy::ecs::system::SystemParam;
use std::cell::RefCell;

/// `ControllerMonster::Update`'s `WheneverPossible` self-buff check (4 s).
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
    // `AttackAction::Execute`: the pet turns to its target (`crate::turning`).
    ch.begin_cast(tf, Some((target, at)), at);
    commands.entity(e).insert(Casting {
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

/// The charge lookups a pet reads: one parameter, so `pet_ai` stays within Bevy's system-parameter limit.
#[derive(SystemParam)]
pub(super) struct PetCharges<'w, 's> {
    starts: Query<'w, 's, &'static crate::casting::ChargeStarts>,
    charging: Query<
        'w,
        's,
        (
            Has<crate::casting::Charge>,
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
    // The owner, and the monster a converted pet follows meanwhile (`combat::JoinedPack::leader`).
    owners: Query<&Transform, (With<Combatant>, Without<Pet>)>,
    mut slot_q: crate::melee::SlotQuery,
    charges: PetCharges,
    rooted: Query<(), With<crate::movement::Rooted>>,
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
    // The characters a pet may fight: those its side calls foes (`combat::Side::foe`, `IsFoe`): a pet
    // carries its owner's pack (`Monster::JoinMe` copies it), so a monster that is not the player's foe
    // (`combat::Allied`: a guard, a converted monster) is none of its foes; a pet a monster's convert joined
    // (`combat::JoinedPack`) fights that pack's foes: the player and the other pets. Pets are added below.
    chars: Query<
        (Entity, &Transform, &Combatant, crate::combat::SideData),
        (
            Or<(With<MonsterAi>, With<Player>)>,
            Without<Dead>,
            Without<Pet>,
        ),
    >,
    // The other characters a pet may buff (`ChooseBestAllyToBuff`): the player, monsters, a released quest
    // pet (pets are in `pets`).
    allies: Query<
        (
            Entity,
            &Transform,
            &Combatant,
            crate::combat::SideData,
            Option<&crate::player::Loadout>,
            Option<&crate::skill_buffs::ReceivedBuffs>,
            Option<&crate::monster_skills::MonsterSkills>,
        ),
        (
            Without<Dead>,
            Without<Pet>,
            Without<crate::world::Breakable>,
        ),
    >,
    pet_auras: Query<&crate::skill_buffs::ReceivedBuffs, With<Pet>>,
    mut crowd: Query<&mut crate::movement::CrowdAgent>,
) {
    if pets.is_empty() {
        landed.clear();
        return;
    }
    let dt = time.delta_secs();
    let slots = crate::melee::Slots::new(&mut slot_q, &scratch);
    // A pet's side: its owner's (the player's), or the pack a monster's convert joined it to.
    let side_of = |j: Option<&crate::combat::JoinedPack>| {
        j.map_or(crate::combat::Side::Player, |j| {
            crate::combat::Side::Monster {
                pack: j.pack.clone(),
                allied: j.allied,
            }
        })
    };
    // Buff-other candidates (`ChooseBestAllyToBuff`, `monster_skills::buff_ally_candidate`): the live
    // characters but the pet itself that are friends of its current side and have no leader or are quest pets
    // (another pet of the owner is led by him and never one). (entity, position, life, life fraction, side,
    // `monster_skills::buff_candidate_by_leader`), and per pet with a buff for its allies those that already have
    // it (`IsSkillBuffActive`). Listed only when such a pet exists.
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
    // Hits taken (`ControllerMonster::UnderAttack`): at most every 500 ms the
    // attacker gains `AttackedAnger` (a monster has no master to share it).
    for h in landed.read() {
        let Ok((_, _, mut pet, ..)) = pets.get_mut(h.target) else {
            continue;
        };
        if h.taunt > 0.0 {
            add_anger(&mut pet.anger, h.attacker, h.taunt);
        }
        if pet.anger_timer > 0.0 {
            continue;
        }
        pet.anger_timer = 0.5;
        let amount = pet.ctl.attacked_anger;
        add_anger(&mut pet.anger, h.attacker, amount);
    }
    // The pets as foes of one another (a converted one and the rest): (entity, position, radius, side).
    let pet_sides: Vec<(Entity, Vec3, f32, crate::combat::Side)> = pets
        .iter()
        .map(|(e, tf, _, cb, _, _, j)| (e, tf.translation, cb.radius, side_of(j)))
        .collect();
    for (e, mut tf, mut pet, mut cb, mut ch, casting, joined) in &mut pets {
        let Ok(otf) = owners.get(pet.owner) else {
            commands.entity(e).try_despawn();
            continue;
        };
        // A converted pet follows the monster that converted it while the join lasts, else its owner.
        let leader = joined.map_or(pet.owner, |j| j.leader);
        let otf = owners.get(leader).unwrap_or(otf);
        let side = side_of(joined);
        // `JoinMe` / `UnJoinLeader` clear the anger list, the most hated enemy and the target
        // (`Monster::JoinMe` @0x180375e40: controller +976 / +992 / +1000) and leave the state alone: a pet on a
        // move order runs on to its point, the rest picks its walk afresh. Its owner's orders reach it no more
        // meanwhile (`commands::pet_commands`).
        if joined.is_some() != pet.joined {
            pet.joined = joined.is_some();
            pet.anger.clear();
            pet.most_hated = None;
            if pet.moving.is_none() {
                pet.path.clear();
            }
            cb.target = None;
        }
        // The crowd failed the move (`Character::CrowdAgentError`, movement.rs): it stops and the state's
        // `PathFailed` runs: the fight states' are melee.rs'; FollowLeader's @0x180148720 is "DefendLeader" (it
        // stands and asks for its slot again in 300..699 ms); Move's @0x1801491c0 is "Idle", as the path's end.
        let mut agent = crowd.get_mut(e).ok();
        if agent.as_mut().is_some_and(|a| a.take_path_failed()) {
            debug!("pet {e}: path failed");
            pet.path.clear();
            pet.melee.note_path_failed();
            if std::mem::take(&mut pet.following) {
                pet.follow_check = 0.3 + crate::rng::f32() * 0.4;
            }
        }
        // The Startup state (`ControllerMonsterStateStartup::OnBegin`
        // @0x18013ead0 / `HandleEvent` @0x18013edc0): every spawned
        // pet has `EnableSpawnAnimation` set; it plays animation type 19
        // (Spawn) once and stays until its "End", or goes Idle at once when
        // the set has none. The pet does nothing else meanwhile; the initial
        // skills (`UseInitialSkillIfSet`) follow at the exit (below).
        if pet.startup {
            if !pet.startup_played {
                pet.startup_played = true;
                if ch.play_scripted(gd_sim::anim::AnimType::Spawn) {
                    continue;
                }
            } else if ch.state == CharState::Scripted {
                // PORT: "End" is the scripted animation leaving its state
                // (as the hidden monster's Startup reads it).
                continue;
            }
            pet.startup = false;
        }
        cb.cooldown -= dt;
        pet.anger_timer -= dt;
        // Energy regenerates up to the maximum (`CharacterBio::
        // UpdateHealthAndMana`).
        pet.energy =
            (pet.energy + cb.profile.sheet.energy_regen_per_sec * dt).min(cb.profile.sheet.energy);
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
        // A foe counts within `MaxPursuitDistance` of the leader and
        // `MaxYViewDistance` in height (`InPursuitRange`).
        let in_pursuit = |p: Vec3, me: Vec3| ctl.in_pursuit(owner, p, me);
        // Forgiving, then sight anger (`ProcessAngerTarget`).
        let forgive = ctl.forgive * dt;
        if forgive > 0.0 {
            pet.anger.iter_mut().for_each(|a| a.1 -= forgive);
            pet.anger.retain(|a| a.1 >= 0.0);
        }
        let me = tf.translation;
        // A foe of the pet's side, alive: (entity, position, radius).
        let foe_at = |m: Entity| -> Option<(Entity, Vec3, f32)> {
            match chars.get(m) {
                Ok((m, mt, mcb, sd)) => {
                    side.foe(sd.side_ref())
                        .then_some((m, mt.translation, mcb.radius))
                }
                Err(_) => pet_sides
                    .iter()
                    .find(|p| p.0 == m && m != e && side.foe(p.3.as_ref()))
                    .map(|p| (p.0, p.1, p.2)),
            }
        };
        let (inner, rate, inner_rate) = ctl.sight;
        // The sight sphere (`GetCharactersInSphere` @0x18013afe0, as the monsters' `combat::sight`) skips a
        // character that causes no anger (`CausesAnger`: a training dummy, a turret); an attack order or a hit
        // still makes it a target.
        let seen = chars
            .iter()
            .filter(|c| {
                side.foe(c.3.side_ref()) && c.3.ai.is_none_or(|ai| ai.as_target.causes_anger)
            })
            .map(|c| (c.0, c.1.translation))
            .chain(
                pet_sides
                    .iter()
                    .filter(|p| p.0 != e && side.foe(p.3.as_ref()))
                    .map(|p| (p.0, p.1)),
            );
        for (m, mt) in seen {
            let d = mt.distance(me);
            if d > ctl.view || !in_pursuit(mt, me) {
                continue;
            }
            let r = if d < inner {
                inner_rate
            } else {
                (1.0 - d / ctl.view.max(1e-3) * 0.1) * rate
            };
            add_anger(&mut pet.anger, m, r * dt);
        }
        // Down or getting up (`crate::ragdoll`): no state is requested until the get-up clip has ended.
        if matches!(ch.state, CharState::KnockedDown | CharState::GettingUp) {
            continue;
        }
        // A weapon charge's warm-up (`casting::charges`, as `combat::monster_ai`: `ControllerPet` registers the
        // monster states, `ControllerPet::RegisterStates` @0x1801265e0): a dash after the cast is the
        // casting's move; the Pursue's warm-up ends as the attack begins, the Pursue ends or a stun / death took
        // the charge.
        let (charge, marked) = charges.charging.get(e).unwrap_or((false, None));
        if charge && marked.is_none() {
            continue;
        }
        let mut pursuit = marked.map(|p| (p.skill.clone(), p.bonus, p.cap));
        // The Attack state sees its attack begin and end (`HandleEvent` "End").
        let busy = casting || matches!(ch.state, CharState::Attack | CharState::Cast);
        if pursuit.is_some() && (busy || !charge || pet.melee.st != crate::melee::St::Pursue) {
            commands
                .entity(e)
                .try_remove::<(crate::casting::Charge, crate::casting::ChargePursuit)>();
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
        // Buff other (`BuffAllyBehavior`): on finding an enemy
        // (WhenEnemyIsSeen) or with the 4 s poll (WheneverPossible).
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
        // Initial skills that are not toggles: the startup state's
        // `UseSkillOnAlly` on itself (`ControllerMonsterStateStartup::
        // UseInitialSkillIfSet` @0x18013eed0: `initialSkillName`,
        // then `initial2SkillName`, each pushed as a temporary state with
        // the pet as its target), e.g. the Concussive Rune's opening nova.
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
        // Self buffs (`ChooseBestBuffSelfSkill`, `monster_skills` module
        // docs): on finding an enemy, at an attack's end with an enemy, and
        // every 4 s for `WheneverPossible`, the first self toggle not yet on
        // is used: it switches on with its cast animation (none for
        // `instantCast`) and stays on.
        let found = has_target && !pet.had_target;
        pet.had_target = has_target;
        // `ChooseBestSkill` runs on finding an enemy, at an attack's end and
        // every 200 ms while pursuing (`monster_skills` docs).
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
                // A timed self buff is cast on itself (`casting.rs` puts it on
                // the pet when the cast goes off).
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
        // `ChooseBestAllyToBuff` (`monster_skills` module docs, @0x180137a60) among the candidates above
        // that lack the buff, by `BuffAllyTargeting`; the cast lands as the monsters' does
        // (`monster_skills::ally_casts_land`). A converted pet's friends are its joined pack's: it buffs the
        // leaderless monsters there (the one that converted it, when that follows nobody), never the player.
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
        // Target (`AngerManager::GetNewTarget`): the commanded enemy while it
        // is valid (`IsEnemyValid`; else Pursue "Return"s and the leader
        // states clear it), else the angriest valid foe. None while a move
        // command runs (the "Move" state does not fight).
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
            // A totem's comparator is the distance: the nearest first.
            let totem = ctl.class == crate::combat::ControllerClass::Totem;
            hated.or_else(|| {
                let valid = pet.anger.iter().filter_map(|(m, v)| {
                    foe_at(*m)
                        .filter(|(_, mt, _)| in_pursuit(*mt, me))
                        .map(|x| (x, *v))
                });
                if totem {
                    valid.min_by(|a, b| a.0.1.distance(me).total_cmp(&b.0.1.distance(me)))
                } else {
                    valid.max_by(|a, b| a.1.total_cmp(&b.1))
                }
                .map(|(x, _)| x)
            })
        };
        cb.target = target.map(|t| t.0);
        let mut want = CharState::Idle;
        // Where the pet is going: its defence slot.
        let mut goal: Option<Vec3> = None;
        if pet.moving.is_some() || pet.dodging {
            // "Move" / "DodgeAttack": run along the path; "Idle" at its end.
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
            // Specials first, then the primary attack.
            // `ChooseBestSkill`: delay and timeout both out.
            let gap = (d - cb.radius - mcb).max(0.5);
            // `ChooseBestSkill` @0x180137550: the first slot whose delay and
            // timeout are out and that passes `rand() % 100 < chance` (rolled on every
            // call) and whose band holds the target; `SkillUsed` re-arms the delay.
            // The choice is not tested for reach: one out of reach is held and
            // pursued for until the next choice (`monster_skills` docs). The 200 ms
            // re-choice skips a held skill whose class keeps its Pursue
            // (`melee::SkillMove::rechooses`: charges, leaps, disengage, evade).
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
                        && (crate::rng::u32(..100) as f32) < s.chance
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
            // The chosen skill as the approach states need it: the primary
            // skill, or the plain melee swing (`meleeRange` reach).
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
                        key: a.skill.clone(),
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
                    key: String::new(),
                    reach: cb.radius + mcb + crate::combat::MELEE_RANGE + SKILL_USE_TOLERANCE,
                    range: crate::combat::MELEE_RANGE,
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
            // The Pursue's move carries its skill: a weapon charge warms up on it (`casting::begin_pursuit_charge`,
            // as `combat::monster_ai`).
            let key = pet.melee.key();
            let pursue = act == crate::melee::Act::Run
                && pet.melee.st == crate::melee::St::Pursue
                && !key.is_empty();
            if !(pursue && pursuit.as_ref().is_some_and(|p| p.0 == key)) {
                if pursuit.take().is_some() {
                    commands
                        .entity(e)
                        .try_remove::<(crate::casting::Charge, crate::casting::ChargePursuit)>();
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
                    // "Return": the fight starts afresh at the next look.
                    pet.melee.leave(&slots, e);
                    pet.path.clear();
                }
                crate::melee::Act::Swing => {
                    pet.path.clear();
                    ch.begin_swing(&tf, Some((t, mt)), mt);
                    want = CharState::Attack;
                    // `ResetSwingTimer` @0x18013d030
                    // (`combat::monster_ai`): the controller's swing pause,
                    // a uniform draw in [min, max].
                    cb.cooldown = pet.ctl.swing_pause.0
                        + crate::rng::f32()
                            * (pet.ctl.swing_pause.1 - pet.ctl.swing_pause.0).max(0.0);
                }
                crate::melee::Act::Stand => {
                    if pet.melee.in_attack() {
                        ch.turn.look_at(mt);
                        // The Attack state uses the skill once the swing
                        // timer has run out, wherever the target now is.
                        if let Some(a) = primary
                            && cb.cooldown <= 0.0
                        {
                            pet.path.clear();
                            start_pet_cast(&mut commands, e, &data, &mut tf, &mut ch, &a, t, mt);
                            cb.cooldown = pet.ctl.swing_pause.0
                                + crate::rng::f32()
                                    * (pet.ctl.swing_pause.1 - pet.ctl.swing_pause.0).max(0.0);
                            continue;
                        }
                    }
                }
                crate::melee::Act::Run => {
                    // A warmed-up weapon charge: the pace's move clip at the warmed-up run speed
                    // (`crate::combat::charge_move`).
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
            // The fight is over: the slots of the foe are released and the
            // states start afresh.
            if pet.melee.engaged() {
                pet.melee.leave(&slots, e);
                pet.path.clear();
            }
            // `FollowLeader` / `DefendLeader`: teleport when too far, else
            // every 300–700 ms walk back to the defence slot.
            let far = tf.translation.distance(owner);
            if !ctl.class.can_move() {
                // A stationary controller's FollowLeader / DefendLeader are
                // the stub that goes "Idle": it stays where it was put.
                pet.following = false;
                pet.path.clear();
            } else if slots.take_lost(e) && pet.following {
                // `FollowLeader::LostSlot`: its `OnUpdate` makes it
                // "DefendLeader" (stands, checks again in 300..400 ms).
                pet.following = false;
                pet.path.clear();
                pet.follow_check = 0.3 + crate::rng::f32() * 0.1;
            } else if ctl.teleport > 0.0 && far > ctl.teleport {
                if let Some(at) = slots.defense(leader, e, &nav) {
                    tf.translation = at;
                    pet.path.clear();
                    pet.following = false;
                }
            } else {
                pet.follow_check -= dt;
                if pet.follow_check <= 0.0 {
                    pet.follow_check = 0.3 + crate::rng::f32() * 0.4;
                    // `GetMoveToPoint` → `RequestDefenseSlot`; run there when
                    // the slot is more than the pet's radius away.
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
                // Nothing to walk (the spot is in the pet's own cell, or no path): the follow ends as at the
                // path's end or its failure (`FollowLeader` → "DefendLeader"), it does not search again
                // every frame.
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
