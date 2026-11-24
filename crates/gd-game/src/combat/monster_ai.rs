use super::*;
use std::cell::RefCell;

pub(super) type MonsterQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Transform,
        &'static mut MonsterAi,
        &'static mut Combatant,
        &'static mut Character,
        Option<&'static gd_game_core::casting::RangedWeapon>,
    ),
    // (A monster dashing on a leap / path charge is moved by `casting`.)
    (
        Without<Dead>,
        Without<Player>,
        Without<crate::movement::Stunned>,
        Without<gd_game_core::world::Dummy>,
        Without<gd_game_core::casting::PathCharge>,
        Without<QuestMove>,
        Without<crate::patrol::PatrolPoint>,
        Without<crate::graeae::ToggleEye>,
        Without<crate::spirit::Spirit>,
        Without<crate::spirit::Dormant>,
    ),
>;

#[allow(clippy::too_many_arguments)]
pub(super) fn monster_ai(
    time: Res<Time>,
    scratch: Local<RefCell<gd_sim::nav::PathScratch>>,
    nav: Res<Nav>,
    players: Query<
        (Entity, &Transform, &Combatant, Has<crate::pets::Pet>),
        (
            Or<(With<Player>, With<crate::pets::Pet>)>,
            Without<Dead>,
            Without<MonsterAi>,
            Without<JoinedPack>,
        ),
    >,
    mut set: ParamSet<(
        Query<(Entity, &Transform, &MonsterAi, &Combatant), Without<Dead>>,
        MonsterQuery,
        // Every monster (dead and AI-excluded ones too): the hit-anger / distress timer decay.
        Query<&mut MonsterAi>,
    )>,
    allied: Query<(), With<Allied>>,
    // A player's pet a monster's convert joined: a rival of its pack's foes, not a foe of hostile monsters.
    joined: Query<
        (Entity, &Transform, &Combatant, &JoinedPack),
        (Without<Dead>, Without<MonsterAi>),
    >,
    rooted: Query<(), With<crate::movement::Rooted>>,
    patrol_points: crate::patrol::Points,
    corpses: Query<(Entity, &Transform, &Combatant), With<Dead>>,
    data: Res<gd_game_core::data::GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
    mut slot_q: crate::melee::SlotQuery,
    skills: Query<&crate::monster_skills::MonsterSkills>,
    starts: Query<&gd_game_core::casting::ChargeStarts>,
    (mut commands, mode, lead_targets, mut anim_cache, mut crowd, sleepers, charging): (
        Commands,
        Res<gd_game_core::session::GameMode>,
        Query<
            (
                &Character,
                Option<&crate::movement::MovePath>,
                Option<&crate::pets::Pet>,
            ),
            Without<MonsterAi>,
        >,
        ResMut<gd_game_core::character::AnimCache>,
        Query<&mut crate::movement::CrowdAgent>,
        super::ai::Sleepers,
        Query<(
            Has<gd_game_core::casting::Charge>,
            Option<&crate::casting::ChargePursuit>,
        )>,
    ),
) {
    let dt = time.delta_secs();
    // The hit-anger and distress timers decay once per frame (`ControllerMonster::Update`), over every
    // monster — dead, stunned or dormant ones included. Folded into this system, before `anger_from_hits`'
    // hit-anger additions (a timer armed this frame keeps its full interval; nothing between the two
    // systems reads the timers), so that system can early-out on a hitless frame.
    for mut ai in set.p2().iter_mut() {
        if ai.anger_timer > 0.0 {
            ai.anger_timer -= dt;
        }
        if ai.distress.timer > 0.0 {
            ai.distress.timer -= dt;
        }
    }
    let slots = crate::melee::Slots::new(&mut slot_q, &scratch);
    // Every living `targetable` monster is a possible enemy of the monsters of a foe faction (stunned, charging or
    // walking ones too: they are not in `monsters`). Only the distinct packs are cloned (`FactionPack::shared`
    // hands out one `Arc` per record, so `same` is a pointer compare); a rival names its pack by index.
    let mut packs: Vec<Option<std::sync::Arc<gd_game_core::monsters::FactionPack>>> = Vec::new();
    let mut rivals: Vec<_> = set
        .p0()
        .iter()
        .filter(|(_, _, ai, _)| ai.as_target.targetable)
        .map(|(e, tf, ai, cb)| {
            (
                e,
                tf.translation,
                cb.radius,
                pack_index(&mut packs, &ai.faction),
            )
        })
        .collect();
    rivals.extend(joined.iter().map(|(e, tf, cb, j)| {
        (
            e,
            tf.translation,
            cb.radius,
            pack_index(&mut packs, &j.pack),
        )
    }));
    let mut monsters = set.p1();
    // The leaders the monsters follow (`Monster::GetLeader` through the
    // object manager): living monsters, the player and his pets (a converted monster's, `influence`), and
    // corpses not yet gone. Only the followed ones are looked up.
    let mut leaders: bevy::ecs::entity::EntityHashMap<crate::patrol::Leader> = Default::default();
    // The possible friends of a fleeing monster: listed only in a frame one flees.
    let mut fleeing = false;
    for (_, _, ai, ..) in &monsters {
        fleeing |= ai.flee.request.is_some() || ai.flee.run.is_some();
        let Some(l) = ai.patrol.leader else { continue };
        if leaders.contains_key(&l) {
            continue;
        }
        let found = if let Ok((_, ltf, _, lcb, _, _)) = monsters.get(l) {
            Some((ltf.translation, lcb.radius, true))
        } else if let Ok((_, ltf, lcb)) = corpses.get(l) {
            Some((ltf.translation, lcb.radius, false))
        } else if let Ok((_, ltf, lcb, _)) = players.get(l) {
            Some((ltf.translation, lcb.radius, true))
        } else {
            None
        };
        if let Some((pos, radius, alive)) = found {
            leaders.insert(
                l,
                crate::patrol::Leader {
                    entity: l,
                    pos,
                    radius,
                    alive,
                },
            );
        }
    }
    let friends_all: Vec<(
        Entity,
        Vec3,
        f32,
        Option<std::sync::Arc<gd_game_core::monsters::FactionPack>>,
    )> = if fleeing {
        monsters
            .iter()
            .map(|(e, tf, ai, cb, _, _)| (e, tf.translation, cb.radius, ai.faction.clone()))
            .collect()
    } else {
        Vec::new()
    };
    // Foes of the monsters: the player and their pets.
    let foes: Vec<(Entity, Vec3, f32)> = players
        .iter()
        .map(|(e, t, c, _)| (e, t.translation, c.radius))
        .collect();
    let pets: Vec<Entity> = players.iter().filter(|p| p.3).map(|p| p.0).collect();
    // The foes of each side (`FoeLists`: the player for hostile monsters, the monsters of foe factions for all).
    let mut lists = FoeLists::new(&foes, packs, rivals);
    // `ControllerMonster::FindEnemyUpdate`: every
    // 200 ms `FindEnemy` asks `AngerManager::GetNewTarget`
    // for a target: the living, attackable foes on the anger
    // list whose anger (× 1.3 for the current target) reaches
    // `AngerTolerance`, the angriest first (the default comparator; the
    // pet-only `petTargetLeastAttacked` / `petTargetGreatestHealth` keys
    // and `petTargetLevelRange` are unset for monsters). No such foe: no
    // enemy. The attack / pursue states drop an enemy that fails
    // `IsEnemyValid` (alive, foe, `InPursuitRange`) and return.
    for (e, tf, mut ai, _, _, _) in &mut monsters {
        let side = lists.side(&ai.faction, allied.contains(e));
        let foes = lists.get(side);
        ai.patrol.leader_pos = ai
            .patrol
            .leader
            .and_then(|l| leaders.get(&l))
            .map(|l| l.pos);
        // The Hidden and Startup states' `EnemyFound` is the empty handler.
        if ai.hidden.is_none() && ai.startup == SpawnStartup::Done {
            find_enemy(
                &mut ai,
                tf.translation,
                foes,
                &pets,
                |e| super::ai::asleep(&sleepers, e),
                dt,
            );
        }
    }

    let mut near = Vec::new();
    for (e, mut tf, mut ai, mut cb, mut ch, ranged) in &mut monsters {
        let side = lists.side(&ai.faction, allied.contains(e));
        let foes = lists.get(side);
        cb.cooldown -= dt;
        // The crowd failed the move (`Character::CrowdAgentError`, movement.rs): the move stops and the state's
        // `PathFailed` runs. The idle-time states' (patrol.rs: Patrol, Return, Roam, Wander, FollowLeader) and
        // DodgeAttack's do what the path's end does (their `PathFailed`); the fight states' are melee.rs'; Flee's is
        // "Return".
        let mut agent = crowd.get_mut(e).ok();
        let path_failed = agent.as_mut().is_some_and(|a| a.take_path_failed());
        if path_failed {
            debug!("monster {e}: path failed");
            ai.path.clear();
            ai.melee.note_path_failed();
        }
        // Down or getting up (`crate::ragdoll`): no state is requested until the get-up clip has ended.
        if matches!(ch.state, CharState::KnockedDown | CharState::GettingUp) {
            continue;
        }
        // A weapon charge's warm-up (`casting::charges`): a dash after the cast is the casting's move; the Pursue's
        // warm-up (`ChargePursuit`: skill, bonus, cap) ends as the attack begins, the Pursue ends or a stun / death
        // took the charge.
        let (charge, marked) = charging.get(e).unwrap_or((false, None));
        if charge && marked.is_none() {
            continue;
        }
        let mut pursuit = marked.map(|p| (p.skill.clone(), p.bonus, p.cap));
        // Swinging or casting (monster_skills.rs): keep still. The Attack
        // state sees its attack begin and end (`HandleEvent` "End").
        let busy = matches!(ch.state, CharState::Attack | CharState::Cast);
        if pursuit.is_some() && (busy || !charge || ai.melee.st != crate::melee::St::Pursue) {
            commands
                .entity(e)
                .try_remove::<(gd_game_core::casting::Charge, crate::casting::ChargePursuit)>();
            pursuit = None;
        }
        ai.melee.note_swing(busy);
        if busy {
            // Flee and DodgeAttack switch the state at once (`SetState`, `ControllerMonsterStateFlee`,
            // `DefaultProjectileNotificationResponse`, which the Attack state answers too): the
            // swing / cast is cut short, its hit and its cast never land.
            if ai.flee.request.is_none() && ai.dodge.start.is_none() {
                continue;
            }
            debug!("monster {e}: flee / dodge cuts its attack short");
            ch.state = CharState::Idle;
            ch.cast_anim = None;
            commands.entity(e).try_remove::<(
                gd_game_core::casting::Casting,
                crate::casting::AimPoint,
                crate::monster_skills::AllyCast,
            )>();
        }
        // "Startup" with the Spawn animation ([`SpawnStartup`]): its "End" is the
        // scripted animation leaving its state (as the Hidden state reads it).
        if ai.startup == SpawnStartup::Pending {
            ai.startup = if ch.play_scripted(gd_sim::anim::AnimType::Spawn) {
                debug!("monster {e} plays its Spawn animation");
                SpawnStartup::Playing
            } else {
                SpawnStartup::Done
            };
        }
        if ai.startup == SpawnStartup::Playing {
            if ch.state == CharState::Scripted {
                ai.flee.request = None;
                continue;
            }
            ai.startup = SpawnStartup::Done;
        }
        // The temporary state "AlertBeforePursue" (`Alert`): it turns to its enemy every update and ignores every
        // event; at the Alert animation's "End" the Pursue under it goes on.
        if let Some(foe) = ai.alert.on {
            ai.flee.request = None;
            ai.dodge.start = None;
            if ch.state == CharState::Alert {
                if let Some((_, fp, _)) = foes.get(foe) {
                    ch.turn.look_at(fp);
                }
                continue;
            }
            ai.alert.on = None;
        }
        // The hidden monster's "Startup" state ([`Hidden`]).
        if let Some(mut h) = ai.hidden {
            ai.flee.request = None;
            match h.target {
                None => {
                    let me = tf.translation;
                    let closest = foes.nearest(me, ai.view, &mut near);
                    if let Some((foe, d)) = closest
                        && d < h.appear
                    {
                        debug!("hidden monster {e} appears for {foe}");
                        h.target = Some(foe);
                        if h.dissolve_ms != 0 {
                            commands
                                .entity(e)
                                .try_insert(AmbushDissolve::begin(h.dissolve_ms));
                        }
                        // PORT: no Spawn animation in the set, no "End" to
                        // wait for: it pursues at once.
                        if !ch.play_scripted(gd_sim::anim::AnimType::Spawn) {
                            h.target = None;
                            ai.hidden = None;
                            ai.enemy = Some(foe);
                            ai.aggro = true;
                        }
                    }
                }
                // The Spawn animation's "End": "Pursue" the target.
                Some(foe) if ch.state != CharState::Scripted => {
                    ai.hidden = None;
                    ai.enemy = Some(foe);
                    ai.aggro = true;
                }
                Some(_) => {}
            }
            if let Some(slot) = ai.hidden.as_mut() {
                *slot = h;
                continue;
            }
        }
        // The DodgeAttack state (dodge.rs): run to the point at run speed,
        // ignoring every event; its end (or a failed path, or being held in place) is "Return".
        // A frame that does not move it (a paused clock) does not end it.
        if let Some(path) = ai.dodge.start.take() {
            ai.path = path;
            ai.dodge.running = true;
            ai.reposition = None;
        }
        if ai.dodge.running {
            ai.flee.request = None;
            let moving = !rooted.contains(e)
                && advance_steered(
                    &mut tf,
                    &mut ai.path,
                    run_speed(&ch),
                    dt,
                    &mut ch.turn,
                    agent.as_deref_mut(),
                );
            if let Some(g) =
                nav.0
                    .ground([tf.translation.x, tf.translation.y + 0.5, tf.translation.z])
            {
                tf.translation.y = g;
            }
            if ai.path.is_empty() || rooted.contains(e) {
                ai.dodge.running = false;
                ai.path.clear();
                ai.give_up = true;
            }
            let want = if moving {
                CharState::Run
            } else {
                CharState::Idle
            };
            if ch.state != want {
                ch.state = want;
            }
            continue;
        }
        // The Flee state (flee.rs); a stationary monster's Flee is the stub
        // that goes "Idle".
        if !ai.class.can_move() {
            ai.flee.request = None;
        }
        ai.flee.tick(dt);
        let friends_of = |ai: &MonsterAi| -> Vec<(Vec3, f32)> {
            friends_all
                .iter()
                .filter(|f| {
                    f.0 != e
                        && match (&ai.faction, &f.3) {
                            (Some(a), Some(b)) => a.is_friend(b),
                            _ => true,
                        }
                })
                .map(|f| (f.1, f.2))
                .collect()
        };
        if let Some(enemy) = ai.flee.request.take() {
            debug!(
                "monster flees from {enemy} (flee {} of {})",
                ai.flee.count() + 1,
                ai.flee.max()
            );
            if ai
                .flee
                .enter(enemy, ch.anims.has(gd_sim::anim::AnimType::Flee))
            {
                sound.write(gd_game_core::visuals::PlaySound {
                    pak: ai.flee.sound.clone().unwrap_or_default(),
                    at: Some(tf.translation),
                });
            }
            if ai
                .flee
                .run
                .as_ref()
                .is_some_and(|r| r.roar != crate::flee::Roar::None)
            {
                debug!("monster flees: roar first");
            } else {
                let friends = friends_of(&ai);
                do_flee(
                    &mut ai,
                    tf.translation,
                    cb.radius,
                    enemy,
                    foes.all(),
                    &friends,
                    &nav,
                    &mut scratch.borrow_mut(),
                );
            }
        }
        // The roar: `DoFlee` waits for the animation's "End" (it may also
        // be interrupted, which leaves the state).
        if let Some(run) = ai.flee.run.as_mut() {
            match run.roar {
                crate::flee::Roar::Pending => {
                    run.roar = crate::flee::Roar::Playing;
                    ch.state = CharState::Roar;
                    continue;
                }
                crate::flee::Roar::Playing if ch.state == CharState::Roar => {
                    continue;
                }
                crate::flee::Roar::Playing => {
                    run.roar = crate::flee::Roar::None;
                    let enemy = run.enemy;
                    let friends = friends_of(&ai);
                    do_flee(
                        &mut ai,
                        tf.translation,
                        cb.radius,
                        enemy,
                        foes.all(),
                        &friends,
                        &nav,
                        &mut scratch.borrow_mut(),
                    );
                }
                crate::flee::Roar::None => {}
            }
        }
        if ai.flee.run.is_some() {
            let run = ai.flee.run.as_mut().unwrap();
            run.timer -= dt;
            let expired = run.timer < 0.0;
            let moving = !rooted.contains(e)
                && advance_steered(
                    &mut tf,
                    &mut ai.path,
                    run_speed(&ch),
                    dt,
                    &mut ch.turn,
                    agent.as_deref_mut(),
                );
            if expired || ai.path.is_empty() {
                end_flee(&mut ai, tf.translation);
                if path_failed {
                    ai.give_up = true;
                }
            }
            if let Some(g) =
                nav.0
                    .ground([tf.translation.x, tf.translation.y + 0.5, tf.translation.z])
            {
                tf.translation.y = g;
            }
            let want = if moving {
                CharState::Run
            } else {
                CharState::Idle
            };
            if ch.state != want {
                ch.state = want;
            }
            continue;
        }
        // No spot that sees the target (monster_skills.rs): "Return".
        if std::mem::take(&mut ai.give_up) {
            let cx = crate::patrol::Cx {
                id: e,
                slots: &slots,
                me: tf.translation,
                radius: cb.radius,
                view_y: ai.view_y,
                leader: ai.patrol.leader.and_then(|l| leaders.get(&l).copied()),
                lost: false,
                nav: &nav,
                points: &patrol_points,
                scratch: &scratch,
            };
            let ai = &mut *ai;
            ai.melee.leave(&slots, e);
            ai.aggro = false;
            ai.returning = true;
            cb.target = None;
            ai.reposition = None;
            ai.patrol.interrupt();
            // A stationary monster's "Return" is the stub: "Idle" in place.
            if ai.class.can_move() {
                ai.patrol.enter_return(&cx, &mut ai.home, &mut ai.path);
            }
        }
        // Running to a spot that sees the target (monster_skills.rs).
        if !ai.aggro || ai.flee.run.is_some() {
            ai.reposition = None;
        }
        if let Some(p) = ai.reposition {
            if !ai.repos_pathed {
                ai.repos_pathed = true;
                match nav.0.find_path(
                    &mut scratch.borrow_mut(),
                    tf.translation.to_array(),
                    p.to_array(),
                    crate::nav::PATH_BUDGET,
                ) {
                    Some(path) => ai.path = path.into_iter().skip(1).map(Vec3::from).collect(),
                    None => ai.reposition = None,
                }
            }
            if ai.reposition.is_some() {
                let moving = !rooted.contains(e)
                    && advance_steered(
                        &mut tf,
                        &mut ai.path,
                        run_speed(&ch),
                        dt,
                        &mut ch.turn,
                        agent.as_deref_mut(),
                    );
                if ai.path.is_empty() {
                    ai.reposition = None;
                }
                if let Some(g) =
                    nav.0
                        .ground([tf.translation.x, tf.translation.y + 0.5, tf.translation.z])
                {
                    tf.translation.y = g;
                }
                let want = if moving {
                    CharState::Run
                } else {
                    CharState::Idle
                };
                if ch.state != want {
                    ch.state = want;
                }
                continue;
            }
            ai.repos_pathed = false;
        } else {
            ai.repos_pathed = false;
        }
        let player = ai.enemy.and_then(|t| foes.get(t));
        let want = match player {
            Some((pe, pp, pr)) if ai.aggro => {
                cb.target = Some(pe);
                // The fight states (melee.rs): Pursue, WaitToAttack, Attack
                // and RepositionForAttack bring the monster to its enemy
                // through the enemy's attack slots. Ranged weapons shoot from
                // their reach. No line-of-sight test
                // (`ControllerAIStateT<ControllerMonster>::UseSkill`
                // asks the skill's LOS virtuals,
                // `Skill::IsTargetInLOS`, only when its virtual `Skill::GetRequiresLOS` says
                // so; that virtual returns false for every weapon attack class
                // (`Skill`, `SkillActivatedWeapon`, `Skill_AttackWeapon`,
                // `Skill_WPAttack*`) and true only for the chain skills
                // (`Skill_AttackChain`, `Skill_AttackProjectileChain`,
                // `Skill_AttackRadiusLightning*`, the chain secondaries),
                // conditionally for `Skill_AttackProjectileBurst` /
                // `Skill_GiveBonus`, see monster_skills.rs).
                let approach = crate::monster_skills::approach(
                    &data,
                    skills.get(e).ok(),
                    cb.radius + pr,
                    ranged,
                    starts.get(e).ok(),
                );
                let frame = crate::melee::Frame {
                    me: e,
                    pos: tf.translation,
                    radius: cb.radius,
                    dt,
                    foe: (pe, pp, pr),
                    app: &approach,
                    can_move: !rooted.contains(e) && ai.class.can_move(),
                    stationary: !ai.class.can_move(),
                    swing_ready: cb.cooldown <= 0.0,
                    view_y: ai.view_y,
                    leader: ai.patrol.leader,
                    walk: ai.walk,
                    nav: &nav,
                    slots: &slots,
                    scratch: &scratch,
                };
                // `DefaultEnemyFoundResponse`: a new enemy (or none in the states yet) is pursued.
                let found = ai.melee.st == crate::melee::St::Idle || ai.melee.enemy() != Some(pe);
                let act = {
                    let ai = &mut *ai;
                    ai.melee.update(&frame, &mut ai.path)
                };
                // `Attack::OnBegin` zeroes the swing timer (charge skills).
                if ai.melee.take_swing_reset() {
                    cb.cooldown = 0.0;
                }
                // The same response's alert, when the state it made executing is "Pursue" (`Alert`).
                let alerted = found && ai.melee.st == crate::melee::St::Pursue && {
                    let far = tf.translation.distance(pp) > data.engine().alert_distance;
                    let diff = ai.anger_diff(pe);
                    let can_play = ch.anims.has(gd_sim::anim::AnimType::Alert);
                    let (alerted, pak) = ai.alert.enemy_found(pe, can_play, far, diff);
                    if let Some(pak) = pak {
                        sound.write(gd_game_core::visuals::PlaySound {
                            pak,
                            at: Some(tf.translation),
                        });
                    }
                    alerted
                };
                // The Pursue's move carries its skill (`ControllerAI::MoveTo`, the state data's): a weapon charge
                // warms up on it (`casting::begin_pursuit_charge`) and moves at the warmed-up run speed below.
                let key = ai.melee.key();
                let pursue = !alerted
                    && act == crate::melee::Act::Run
                    && ai.melee.st == crate::melee::St::Pursue
                    && !key.is_empty();
                if !(pursue && pursuit.as_ref().is_some_and(|p| p.0 == key)) {
                    if pursuit.take().is_some() {
                        commands
                            .entity(e)
                            .try_remove::<(gd_game_core::casting::Charge, crate::casting::ChargePursuit)>(
                            );
                    }
                    if pursue {
                        // The skill's level: the attack or a special slot's.
                        let li = skills.get(e).ok().and_then(|ms| {
                            ms.attack
                                .iter()
                                .map(|a| (&a.0, a.1))
                                .chain(ms.specials.iter().map(|s| (&s.skill, s.level_index)))
                                .find(|(s, _)| *s == key)
                                .map(|(_, li)| li)
                        });
                        crate::casting::begin_pursuit_charge(
                            &mut commands,
                            &data,
                            e,
                            pe,
                            key,
                            li.unwrap_or(0),
                            &tf,
                        );
                    }
                }
                if alerted {
                    debug!("monster {e} is alerted by {pe}");
                    ch.turn.look_at(pp);
                    CharState::Alert
                } else {
                    match act {
                        crate::melee::Act::Return => {
                            // The "Return" state: `give_up` below does its work.
                            ai.give_up = true;
                            ai.melee.leave(&slots, e);
                            CharState::Idle
                        }
                        crate::melee::Act::Swing => {
                            // The swing timer re-arms as the attack starts
                            // (`ControllerMonsterStateAttack::OnUpdate`
                            //: timer ≤ 0 → `ResetSwingTimer`
                            // (`min/maxSwingPause`) then
                            // `AttackEnemyOrReturn`; `ControllerMonster::Update`
                            // counts it down every update, the
                            // attack included).
                            ai.path.clear();
                            // `AttackEnemyOrReturn`: the swing's attack action turns to the enemy.
                            ch.begin_swing(&tf, Some((pe, pp)), pp);
                            cb.cooldown = ai.swing_pause.0
                                + gd_game_core::rng::f32()
                                    * (ai.swing_pause.1 - ai.swing_pause.0).max(0.0);
                            // `LeadTarget` for a ranged weapon's plain shot
                            // (lead.rs): the shot flies at the led point.
                            if let Some(rw) = ranged
                                && crate::lead::roll(&ai.lead, mode.0.difficulty)
                            {
                                let tch = lead_targets.get(pe).ok();
                                let path_target = tch.and_then(|(_, p, pet)| {
                                    p.and_then(|p| p.points.last().copied())
                                        .or_else(|| pet.and_then(|p| p.path_target()))
                                });
                                let vel = crate::lead::velocity(pp, path_target, tch.map(|t| t.0));
                                if let Some(p) = crate::lead::aim_projectile(
                                    &data,
                                    &rw.projectile,
                                    tf.translation,
                                    (pp, vel),
                                    crate::lead::delay(&data, &mut anim_cache, &ch),
                                    rw.reach,
                                ) {
                                    commands.entity(e).try_insert(crate::casting::AimPoint(p));
                                }
                            }
                            CharState::Attack
                        }
                        crate::melee::Act::Stand => {
                            if ai.melee.in_attack() {
                                ch.turn.look_at(pp);
                            }
                            CharState::Idle
                        }
                        crate::melee::Act::Run => {
                            // A warmed-up weapon charge: the pace's move clip at the warmed-up run speed
                            // (`ForceSpeedUpdate` re-adjusts whichever move clip plays, `charge_move`).
                            let paced = pace_speed(&ch, ai.melee.pace(), &ai.walk);
                            let (speed, state, rate) = match &pursuit {
                                Some((_, bonus, cap)) if paced.1 != CharState::Idle => {
                                    let (s, st, r) =
                                        charge_move(&ch, paced.1, Some((*bonus, *cap)));
                                    (s, st, Some(r))
                                }
                                _ => paced,
                            };
                            if !rooted.contains(e)
                                && advance_steered(
                                    &mut tf,
                                    &mut ai.path,
                                    speed,
                                    dt,
                                    &mut ch.turn,
                                    agent.as_deref_mut(),
                                )
                            {
                                ch.move_rate = rate;
                                state
                            } else {
                                CharState::Idle
                            }
                        }
                    }
                }
            }
            _ => {
                let ai = &mut *ai;
                ai.melee.leave(&slots, e);
                // Leash: forget the player and walk home.
                // The idle-time states (patrol.rs): patrol, roam, wander,
                // follow the leader; "Return" first after a fight.
                let cx = crate::patrol::Cx {
                    id: e,
                    slots: &slots,
                    me: tf.translation,
                    radius: cb.radius,
                    view_y: ai.view_y,
                    leader: ai.patrol.leader.and_then(|l| leaders.get(&l).copied()),
                    lost: slots.take_lost(e),
                    nav: &nav,
                    points: &patrol_points,
                    scratch: &scratch,
                };
                if ai.aggro {
                    ai.aggro = false;
                    cb.target = None;
                    ai.patrol.interrupt();
                    if ai.class.can_move() {
                        ai.patrol.enter_return(&cx, &mut ai.home, &mut ai.path);
                    }
                }
                // A stationary monster has no idle-time states but Idle.
                if !ai.class.can_move() {
                    ai.path.clear();
                } else if let Some(at) = ai.patrol.update(dt, &cx, &mut ai.home, &mut ai.path) {
                    tf.translation = at;
                }
                let walk = ai.patrol.walking();
                let speed = if walk {
                    walk_speed(&ch)
                } else {
                    run_speed(&ch)
                };
                if !rooted.contains(e)
                    && advance_steered(
                        &mut tf,
                        &mut ai.path,
                        speed,
                        dt,
                        &mut ch.turn,
                        agent.as_deref_mut(),
                    )
                {
                    if walk {
                        CharState::Walk
                    } else {
                        CharState::Run
                    }
                } else {
                    CharState::Idle
                }
            }
        };
        if let Some(g) = nav
            .0
            .ground([tf.translation.x, tf.translation.y + 0.5, tf.translation.z])
        {
            tf.translation.y = g;
        }
        if ch.state != want {
            ch.state = want;
        }
    }
}

/// `GD_AUTO_FIGHT` (hooks.rs): a character [`Side::Player`] calls a foe (an [`super::Allied`] monster, a Crucible
/// defense turret or a friendly guard, is none), attacked as with the button held (every frame re-arms the
/// [`crate::player_orders::PrimaryUse`]).
pub(super) fn auto_fight(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut players: Query<
        (
            &Transform,
            &mut Combatant,
            &mut crate::player_orders::PrimaryUse,
        ),
        (With<Player>, Without<Dead>),
    >,
    targets: Query<
        (Entity, &Transform, SideData),
        (With<Combatant>, Without<Player>, Without<Dead>),
    >,
) {
    if !hooks.gameplay.auto_fight {
        return;
    }
    let Ok((tf, mut cb, mut primary)) = players.single_mut() else {
        return;
    };
    let foe = |s: &SideDataItem| Side::Player.foe(s.side_ref());
    if cb
        .target
        .is_none_or(|t| !targets.get(t).is_ok_and(|(_, _, s)| foe(&s)))
    {
        cb.target = targets
            .iter()
            .filter(|(_, _, s)| foe(s))
            .map(|(e, t, _)| (e, t.translation.distance(tf.translation)))
            .filter(|(_, d)| *d < hooks.gameplay.auto_fight_range)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(e, _)| e);
    }
    primary.0 = cb.target.is_some();
}

/// The player's attack order on its target (set by clicking a monster): one use of the primary attack
/// ([`crate::player_orders::PrimaryUse`]): walk into reach, swing once; the held button's repeat arms the next. Once
/// spent, the target stays while the swing runs (its hit lands on it) and is dropped after. A skill waiting in the
/// pending-action slot goes first ([`gd_game_core::character::QueuedAction`]). A target no path leads any closer to is
/// dropped (a request whose `Player::FindPath` finds nothing is dropped with no state change,
/// `ControllerPlayerStateMoveTo`'s `PathFailed` goes to Idle), so the failed search is not
/// repeated every frame. A "Kick" breakable is kicked instead of swung at
/// (`crate::hotbar::kicks_breakable`, `SelectPrimaryAction`).
/// The range re-check carries the UseSkill state's tolerance (1.0, 2.0 for a moving current target,
/// `GetSkillUseTolerance`), so a kiting target does not stutter the swing chain into re-paths.
/// An evade's recovery is cancelable here (only its dash refuses); a held swing that ends chains into the next
/// in the same frame (`finish_actions` runs before this, `drive_characters` after).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn player_attack(
    mut commands: Commands,
    scratch: Local<RefCell<gd_sim::nav::PathScratch>>,
    mut players: Query<
        (
            Entity,
            &mut Transform,
            &mut MovePath,
            &mut Combatant,
            &mut Character,
            &mut gd_game_core::casting::Caster,
            (
                &gd_game_core::player::Loadout,
                &gd_game_core::player::EffectiveSkills,
            ),
            Option<&gd_game_core::casting::RangedWeapon>,
            Option<&gd_game_core::weapon_pool::SkillLinks>,
            Has<crate::movement::Rooted>,
            &mut crate::player_orders::PrimaryUse,
        ),
        (
            With<Player>,
            Without<Dead>,
            Without<gd_game_core::casting::SpellAttacker>,
            Without<crate::movement::Stunned>,
        ),
    >,
    targets: Query<
        (
            Entity,
            &Transform,
            &Combatant,
            Option<&Character>,
            Has<MonsterAi>,
            SideData,
        ),
        (Without<Player>, Without<Dead>),
    >,
    coll: Res<crate::collision::WorldCollision>,
    data: Res<GameData>,
    breakables: Query<&gd_game_core::world::Breakable>,
    options: Option<Res<gd_game_core::options::GameOptions>>,
    nav: Res<Nav>,
    time: Res<Time>,
    mut pool_rng: ResMut<crate::weapon_pool::PoolRng>,
) {
    let Ok((
        e,
        mut tf,
        mut path,
        mut cb,
        mut ch,
        mut caster,
        (loadout, effective),
        ranged,
        links,
        rooted,
        mut primary,
    )) = players.single_mut()
    else {
        return;
    };
    cb.cooldown -= time.delta_secs();
    let Some(t) = cb.target else { return };
    // The use is spent: the target was the swing's and goes once no attack / cast runs.
    if !primary.0 {
        if !matches!(ch.state, CharState::Attack | CharState::Cast) {
            cb.target = None;
        }
        return;
    }
    // Down or getting up: no action until the get-up clip has ended.
    if matches!(ch.state, CharState::KnockedDown | CharState::GettingUp) {
        return;
    }
    // A target that is no longer the player's to aim at (`player_target`: an `Allied` monster, a guard or a
    // converted one; a converted pet that went back to its owner) is dropped.
    let Ok((_, ttf, tcb, tch, is_monster, tside)) = targets.get(t) else {
        cb.target = None;
        return;
    };
    if !super::player_target(breakables.get(t).ok(), &tside) {
        cb.target = None;
        return;
    }
    // The Move slot's reset rule (`crate::hotbar::move_slot_keeps_target`,
    // `Grim Dawn.exe` `InGameUI::HandleMouseEvent`): Move keeps a breakable as its target
    // only with `breakingMoveTo`, never a character; a target it may not keep is
    // dropped (the press walks to it), not attacked.
    if caster.slots[gd_game_core::casting::SLOT_LEFT]
        .as_deref()
        .is_some_and(crate::hotbar::is_move_to)
        && !crate::hotbar::move_slot_keeps(
            false,
            breakables.get(t).is_ok(),
            options.is_none_or(|o| o.flag("breakingMoveTo")),
        )
    {
        cb.target = None;
        return;
    }
    let to = ttf.translation - tf.translation;
    // The kick is melee whatever the weapon (`Skill_Kick::GetValidTarget`
    // → `Skill::GetValidMeleeTarget`).
    let kick = breakables.get(t).is_ok_and(|b| {
        data.db
            .get(&b.record)
            .is_some_and(crate::hotbar::kicks_breakable)
    });
    let reach = if kick {
        cb.reach(tcb)
    } else {
        ranged.map_or(cb.reach(tcb), |r| r.reach.max(cb.reach(tcb)))
    };
    // A ranged shot at a character needs line of sight
    // (`ControllerAIStateT<ControllerPlayer, Player>::UseSkill`'s
    // `IsTargetInLOS` gate; PORT: the plain ranged swing repositions too — playtest:
    // a shot blocked by an obstacle moves to a clear spot instead of firing into it).
    let blocked = !kick && ranged.is_some() && is_monster && {
        let (head, aim) = crate::monster_skills::sight_ray(&data, &ch, &tf, ttf, tch);
        !crate::monster_skills::target_in_los(&coll, head, ttf.translation, aim)
    };
    // `ControllerPlayerStateUseSkill::GetSkillUseTolerance`:
    // 2.0 when the target is the current target and moving, else 1.0 — the ongoing
    // attack keeps its swing chain against a kiting target instead of re-pathing the
    // moment it steps past reach.
    let tolerance = if tch.is_some_and(|c| {
        matches!(
            c.state,
            CharState::Run | CharState::Walk | CharState::Charge
        )
    }) {
        2.0
    } else {
        1.0
    };
    if !blocked && Vec2::new(to.x, to.z).length() <= reach + tolerance {
        path.points.clear();
        // The evade refuses only its dash (warm-up); its recovery is cancelable
        // (`ControllerPlayerStateJumpToUseSkill::RequestSkillAction`, `evade.rs`).
        if !matches!(ch.state, CharState::Attack | CharState::Cast)
            && !(ch.state == CharState::Evade && ch.evade_warmup)
            // A queued skill goes first (`cast_requests` runs it next frame).
            && !matches!(ch.queued, Some(gd_game_core::character::QueuedAction::Cast(_)))
        {
            if ch.state == CharState::Evade {
                commands
                    .entity(e)
                    .try_remove::<(crate::evade::Evading, gd_game_core::casting::Leap)>();
            }
            if kick {
                primary.0 = !crate::casting::try_cast(
                    &mut commands,
                    &data,
                    e,
                    &mut tf,
                    &mut caster,
                    loadout,
                    effective,
                    links,
                    &cb.profile.totals,
                    &mut ch,
                    &mut path,
                    crate::casting::DEFAULT_KICK,
                    ttf.translation,
                    Some(t),
                    cb.radius + tcb.radius,
                    gd_game_core::casting::Use::Request,
                    gd_game_core::casting::Walk::new(false, 0, true),
                );
            } else {
                ch.state = CharState::Attack;
                ch.begin_swing(&tf, Some((t, ttf.translation)), ttf.translation);
                // The weapon pool rolls as the swing starts.
                crate::weapon_pool::start_swing(&mut commands, e, links, &mut pool_rng.0);
                primary.0 = false;
            }
        }
    } else if !matches!(ch.state, CharState::Attack | CharState::Cast)
        && !(ch.state == CharState::Evade && ch.evade_warmup)
        && path.points.is_empty()
        // Held in place the player does not set out (`follow_path` ends a rooted player's move).
        && !rooted
    {
        if ch.state == CharState::Evade {
            commands
                .entity(e)
                .try_remove::<(crate::evade::Evading, gd_game_core::casting::Leap)>();
            ch.state = CharState::Run;
        }
        // Blocked: a spot that sees the target (within reach), else straight at it.
        let dest = if blocked {
            crate::monster_skills::los_spot(
                &coll,
                &nav,
                &mut scratch.borrow_mut(),
                tf.translation,
                ttf.translation,
                reach,
                crate::monster_skills::character_center(&data, &ch).y,
            )
            .unwrap_or(ttf.translation)
        } else {
            ttf.translation
        };
        match nav.0.find_path(
            &mut scratch.borrow_mut(),
            tf.translation.to_array(),
            dest.to_array(),
            crate::nav::PATH_BUDGET_LONG,
        ) {
            Some(p) => path.points = p.into_iter().skip(1).map(Vec3::from).collect(),
            None => cb.target = None,
        }
    }
}
