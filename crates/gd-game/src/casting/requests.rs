use super::*;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn cast_requests(
    mut commands: Commands,
    mut requests: MessageReader<CastRequest>,
    data: Res<GameData>,
    options: Option<Res<gd_game_core::options::GameOptions>>,
    mut players: Query<
        (
            Entity,
            &mut Transform,
            &mut Caster,
            &Loadout,
            &mut Character,
            &mut MovePath,
            Option<&gd_game_core::weapon_pool::SkillLinks>,
            Option<&MoveToUseSkill>,
            &gd_game_core::player::ControllerDirection,
            Option<&Casting>,
            Option<&mut Channel>,
            Option<&LastCast>,
            &gd_game_core::player::EffectiveSkills,
        ),
        (
            With<Player>,
            Without<Dead>,
            Without<crate::movement::Stunned>,
        ),
    >,
    monsters: Query<
        (Entity, &GlobalTransform, &Combatant),
        (
            Or<(With<MonsterAi>, With<gd_game_core::world::Breakable>)>,
            Without<Dead>,
            Without<Player>,
        ),
    >,
    breakables: Query<&gd_game_core::world::Breakable>,
    mut player_cb: Query<&mut Combatant, (With<Player>, Without<MonsterAi>)>,
    ranged_q: Query<(), (With<Player>, With<RangedWeapon>)>,
    mut pool_rng: ResMut<crate::weapon_pool::PoolRng>,
    friends: Query<
        (&Transform, &Combatant, &crate::pets::Pet),
        (Without<Dead>, Without<Player>, Without<MonsterAi>),
    >,
    nav: Res<Nav>,
    mut primary_use: Query<&mut crate::player_orders::PrimaryUse, With<Player>>,
) {
    let ranged = ranged_q.iter().next();
    let classic = options.as_ref().is_some_and(|o| o.flag("classicCasting"));
    let movement = options.as_ref().map_or(0, |o| o.int("movementType"));
    let lock = options.as_ref().is_none_or(|o| o.flag("gamepadTargetLock"));
    // After this frame's requests, the pending-action slot re-asks permission (`CharacterHandlerUpdate` runs
    // after the input): the gate stores it back while the current action still refuses it.
    let mut reqs = requests
        .read()
        .cloned()
        .collect::<Vec<CastRequest>>()
        .into_iter();
    let mut reask = true;
    // A skill that starts replaces the click's attack order ([`crate::player_orders::PrimaryUse`]); the held
    // button's next repeat arms it again.
    let mut disarm = || {
        if let Ok(mut u) = primary_use.single_mut() {
            u.0 = false;
        }
    };
    loop {
        let req = match reqs.next() {
            Some(r) => r,
            None if std::mem::take(&mut reask) => {
                let Ok((_, _, _, _, mut ch, ..)) = players.single_mut() else {
                    break;
                };
                match ch
                    .queued
                    .take_if(|q| matches!(q, gd_game_core::character::QueuedAction::Cast(_)))
                {
                    Some(gd_game_core::character::QueuedAction::Cast(r)) => r,
                    _ => break,
                }
            }
            None => break,
        };
        let req = &req;
        let Ok((
            e,
            mut tf,
            mut caster,
            loadout,
            mut ch,
            mut path,
            links,
            pending,
            controller,
            casting,
            channel,
            last,
            effective,
        )) = players.single_mut()
        else {
            continue;
        };
        let (slot, picked, point, force, tested) =
            (req.slot, req.target, req.point, req.force, req.scripted);
        let Some(skill) = caster.slots.get(slot).cloned().flatten() else {
            continue;
        };
        // The Move slot's reset (`InGameUI::HandleMouseEvent`): a left press keeps a breakable only with
        // `breakingMoveTo`, never a character; else the fallback weapon attack swings at the point.
        let picked = if crate::hotbar::is_move_to(&skill)
            && slot == SLOT_LEFT
            && force
            && !crate::hotbar::move_slot_keeps(
                false,
                picked.is_some_and(|t| breakables.get(t).is_ok()),
                options.as_ref().is_none_or(|o| o.flag("breakingMoveTo")),
            ) {
            None
        } else {
            picked
        };
        // The Move skill's request always fails (`Skill_Move::GetValidTarget`): from a hot slot key or the armed
        // slot nothing happens (`UIHotSlotButton.fallbackToDefault` is on for the two mouse buttons only); the
        // left button's `force` press falls back to the default weapon attack (`SelectPrimaryAction`).
        let skill = match crate::hotbar::is_move_to(&skill) {
            true if slot == SLOT_LEFT && force => DEFAULT_ATTACK.to_owned(),
            true => continue,
            false => skill,
        };
        // `SelectPrimaryAction`: a left-button action on a "Kick" breakable requests the kick whatever the slot
        // holds, the Move fallback included. Right-button and hot-slot requests have no such branch.
        let skill = if slot == SLOT_LEFT
            && picked.is_some_and(|t| {
                breakables.get(t).is_ok_and(|b| {
                    data.db
                        .get(&b.record)
                        .is_some_and(crate::hotbar::kicks_breakable)
                })
            }) {
            DEFAULT_KICK.to_owned()
        } else {
            skill
        };
        if crate::hotbar::option_kind(&data.db, &skill) != crate::hotbar::OptionKind::Skill {
            continue;
        }
        // Test casts keep their aim.
        let (picked, point) = match data.db.get(&skill).filter(|_| movement > 0 && !tested) {
            Some(rec) => {
                let aim = KeyboardAim {
                    movement_type: movement,
                    move_type: gd_sim::skills::move_type(&rec.class),
                    controller: *controller,
                    pos: tf.translation,
                    facing: tf.rotation * Vec3::Z,
                    range: gamepad_range(&data, rec),
                    gamepad_lock: lock,
                };
                let (t, p, straight) = keyboard_aim(&aim, picked, point);
                (
                    t,
                    if straight {
                        crate::nav::straight_move_point(&nav.0, tf.translation, p)
                    } else {
                        p
                    },
                )
            }
            None => (picked, point),
        };
        let friend = picked.filter(|t| friends.get(*t).is_ok_and(|(_, _, p)| p.owner == e));
        let mut target = picked.filter(|_| friend.is_none());
        let mut point = point;
        match data
            .db
            .get(&skill)
            .map(|r| gd_sim::skills::target_type(&r.class))
        {
            Some(gd_sim::skills::TargetType::Ally) => {
                let Some((ftf, ..)) = friend.and_then(|f| friends.get(f).ok()) else {
                    debug!("{skill}: no friend to cast it on");
                    continue;
                };
                target = friend;
                point = ftf.translation;
            }
            Some(gd_sim::skills::TargetType::Enemy) if friend.is_some() => {
                debug!("{skill} cannot be cast on an ally");
                continue;
            }
            _ => {}
        }
        // A `Point` / ground-only spell drops the foe, any other needs one.
        if let Some(rec) = data.db.get(&skill)
            && let Some(profile) = gd_sim::skills::profile_record(&data.db, &skill)
        {
            match gd_sim::skills::request_aim(&rec.class, &profile) {
                gd_sim::skills::RequestAim::Point => {
                    // A floating body is aimed at, not the ground under it.
                    if let Some((_, gt, mcb)) = target.and_then(|t| monsters.get(t).ok()) {
                        point =
                            super::projectiles::aim_point(point, Some(&mcb.body_box(&gt.affine())));
                    }
                    target = None;
                }
                gd_sim::skills::RequestAim::Foe
                    if target.is_none_or(|t| monsters.get(t).is_err()) =>
                {
                    debug!("{skill}: needs a foe as its target");
                    continue;
                }
                _ => {}
            }
        }
        // A plain weapon attack held with Shift: swing where the cursor is, standing still; melee only hits a
        // target within reach.
        if force
            && slot == SLOT_LEFT
            && data
                .db
                .get(&skill)
                .is_some_and(|r| kind_of(r) == Kind::Weapon)
            && skill == DEFAULT_ATTACK
        {
            if queue_if_busy(&data, &mut ch, casting, last, &skill, req) {
                continue;
            }
            if ch.state == CharState::Evade {
                commands
                    .entity(e)
                    .try_remove::<(crate::evade::Evading, gd_game_core::casting::Leap)>();
            }
            path.points.clear();
            // A new action: the held channel stops (`SkillManager::StopCurrentSkill`) and a pending walk to a
            // skill goes, with its warm-up marker.
            commands
                .entity(e)
                .try_remove::<(Channel, MoveToUseSkill, WarmPaid)>();
            if let Ok(mut cb) = player_cb.single_mut() {
                let reach = |t: Entity| {
                    monsters.get(t).ok().is_some_and(|(_, gt, mcb)| {
                        (gt.translation() - tf.translation).with_y(0.0).length() <= cb.reach(mcb)
                    })
                };
                cb.target = target.filter(|t| ranged.is_some() || reach(*t));
            }
            if ranged.is_some() {
                commands.entity(e).insert(AimPoint(point));
            }
            ch.state = CharState::Attack;
            // A recovery cancel restarts the swing at once (the ended clip must not satisfy
            // `drive_characters`' wanted check).
            ch.playing = gd_sim::anim::AnimType::None;
            // `AttackAction::Execute`: the swing turns to the cursor.
            let foe = target
                .and_then(|t| monsters.get(t).ok())
                .map(|m| (m.0, m.1.translation()));
            ch.begin_swing(&tf, foe, point);
            crate::weapon_pool::start_swing(&mut commands, e, links, &mut pool_rng.0);
            disarm();
            continue;
        }
        if let Some(rec) = data.db.get(&skill).filter(|r| kind_of(r) == Kind::Beam) {
            if queue_if_busy(&data, &mut ch, casting, last, &skill, req) {
                continue;
            }
            // The held key's re-request of the running channel is not a new use.
            let li = match channel.as_ref().filter(|c| c.skill == skill) {
                Some(c) => c.level_index,
                None => match request_gates(
                    &mut commands,
                    &data,
                    &caster,
                    loadout,
                    effective,
                    links,
                    rec,
                    &skill,
                    Use::Request,
                ) {
                    Some(level) => (level - 1) as usize,
                    None => continue,
                },
            };
            path.points.clear();
            // `ChannelStart` → `Channel` (`Channel2` when the set names it), the whirlwind `SpinStart` →
            // `Spin`. A re-request of the running channel keeps it and only retargets; its pair plays again
            // when another state took the character and the set has the clips (without them the channel runs
            // in the character's own pose). A new one restarts the pair.
            use gd_sim::anim::AnimType;
            let spin = rec.class == "Skill_AttackRadiusSpin";
            let pair = if spin {
                [AnimType::SpinStart, AnimType::Spin]
            } else if ch.anims.has(AnimType::Channel2Start) {
                [AnimType::Channel2Start, AnimType::Channel2]
            } else {
                [AnimType::ChannelStart, AnimType::Channel]
            };
            let begin = |ch: &mut Character| match pair[0] {
                AnimType::SpinStart => ch.begin_spin(),
                t => ch.begin_channel(t == AnimType::Channel2Start),
            };
            if let Some(mut c) = channel.filter(|c| c.skill == skill) {
                c.target = target;
                if !matches!(
                    ch.state,
                    CharState::Channel | CharState::Channel2 | CharState::Spin
                ) && pair.iter().any(|t| ch.anims.has(*t))
                {
                    begin(&mut ch);
                }
                continue;
            }
            // `DefaultRequestSkillAction`: a channel short of its first payment is refused like any request.
            let cost = level_f(rec, "skillManaCost", li);
            if caster.energy < cost {
                debug!(
                    "not enough energy for {skill} ({cost:.0} > {:.0})",
                    caster.energy
                );
                commands.write_message(crate::error_vox::ErrorVox::new(
                    crate::error_vox::VoxKind::NotEnoughEnergy,
                    &skill,
                ));
                continue;
            }
            begin(&mut ch);
            // A new action replaces a pending walk to a skill (and its warm-up marker).
            commands
                .entity(e)
                .try_remove::<(MoveToUseSkill, WarmPaid)>();
            commands.entity(e).try_insert(Channel {
                skill,
                level_index: li,
                slot,
                tick: 0.0,
                scripted: tested,
                input_held: true,
                input_aim: None,
                aim: None,
                held: 0.0,
                target,
            });
            disarm();
            continue;
        }
        let profile = player_cb.iter().next().map(|c| c.profile.clone());
        let none = std::collections::BTreeMap::new();
        // `ControllerPlayerStateMoveToUseSkill::RequestSkillAction`: the pending walk to the same skill and target
        // stays.
        if pending.is_some_and(|p| p.skill == skill && p.target == target) {
            continue;
        }
        let radii = player_cb.iter().next().map_or(0.0, |c| c.radius)
            + target
                .and_then(|t| {
                    monsters
                        .get(t)
                        .ok()
                        .map(|m| m.2.radius)
                        .or(friends.get(t).ok().map(|f| f.1.radius))
                })
                .unwrap_or(0.0);
        if queue_if_busy(&data, &mut ch, casting, last, &skill, req) {
            continue;
        }
        if try_cast(
            &mut commands,
            &data,
            e,
            &mut tf,
            &mut caster,
            loadout,
            effective,
            links,
            profile.as_ref().map_or(&none, |p| &p.totals),
            &mut ch,
            &mut path,
            &skill,
            point,
            target,
            radii,
            Use::Request,
            Walk::new(classic, movement, target.is_some()),
        ) {
            disarm();
        }
    }
}

/// The action queue's busy gate (`LocalHandleAction` → `CheckAction`): a request the current action refuses is
/// remembered in the one pending slot (last wins) or dropped; one it allows runs now and clears the slot. During
/// a swing or cast, `AttackAction::QueryActionPermission`, in order:
/// - the running skill itself (the held button's repeat) → dropped, even past `AllowInterrupt`: a held skill's
///   cadence is the whole clip;
/// - a [`gd_sim::skills::chains_at_end`] skill while a skill request is queued → dropped;
/// - before the clip's `AllowInterrupt` → queued;
/// - a `chains_at_end` skill during a `chains_at_end` one → queued until the "End";
/// - else → runs now (the recovery cancel).
///
/// Other busy states (an evade's dash, a stun, a flinch) and a pending attack-delay
/// (`ControllerPlayerStateUseSkill::OnBegin`) queue it; a dead character's request is dropped. True when the
/// caller must skip the request. The running skill is the cast's [`Casting`], after its fire point the
/// [`LastCast`] of the playing clip; a swing's is the default attack.
fn queue_if_busy(
    data: &GameData,
    ch: &mut Character,
    casting: Option<&Casting>,
    last: Option<&LastCast>,
    skill: &str,
    req: &CastRequest,
) -> bool {
    let rec = data.db.get(skill);
    if rec.as_ref().is_some_and(|r| r.bool("instantCast")) {
        return false;
    }
    let chains = |r: &Record| gd_sim::skills::chains_at_end(&r.class);
    let queue = match ch.state {
        CharState::Attack | CharState::Cast => {
            let running = match ch.state {
                CharState::Cast => casting.map(|c| c.skill.as_str()).or_else(|| {
                    last.filter(|l| l.anim == ch.playing)
                        .map(|l| l.skill.as_str())
                }),
                _ => Some(DEFAULT_ATTACK),
            };
            let same = running.is_some_and(|r| {
                skill.eq_ignore_ascii_case(r)
                    || rec.as_ref().is_some_and(|rec| {
                        gd_sim::skills::profile_name(rec).is_some_and(|p| p.eq_ignore_ascii_case(r))
                    })
            });
            let attack = rec.is_some_and(chains);
            if same
                || (attack
                    && matches!(
                        ch.queued,
                        Some(gd_game_core::character::QueuedAction::Cast(_))
                    ))
            {
                return true;
            }
            !ch.allow_interrupt
                || (attack && running.and_then(|r| data.db.get(r)).is_some_and(chains))
        }
        CharState::Death => return true,
        _ => !ch.can_act(),
    };
    if queue || ch.attack_delay > 0.0 {
        ch.queued = Some(gd_game_core::character::QueuedAction::Cast(req.clone()));
        return true;
    }
    ch.queued = None;
    false
}

/// The gates every player skill use passes: a learned level, the form's allowed skill types, the worn weapons
/// and the cooldown (skipped for a charge's arrival: its warm-up started it). The skill's level, or `None`.
#[allow(clippy::too_many_arguments)]
fn request_gates(
    commands: &mut Commands,
    data: &GameData,
    caster: &Caster,
    loadout: &Loadout,
    effective: &gd_sim::skills::SkillBook,
    links: Option<&gd_game_core::weapon_pool::SkillLinks>,
    rec: &Record,
    skill: &str,
    how: Use,
) -> Option<u32> {
    let level = loadout.skill_level(&data.db, effective, skill);
    if level == 0 {
        return None;
    }
    if let Some((form, allowed)) = &loadout.form
        && !skill.eq_ignore_ascii_case(form)
        && !loadout
            .granted
            .iter()
            .any(|(g, _)| g.eq_ignore_ascii_case(skill))
        && !allowed.is_empty()
        && !allowed.iter().any(|a| a.eq_ignore_ascii_case(&rec.class))
    {
        info!("{skill} cannot be used in this form");
        return None;
    }
    // `Skill::SetAvailability`. PORT: the log line is the port's; the game shows nothing.
    if let Some(l) = links
        && !l.weapons_allow(&data.db, rec, skill)
    {
        info!(
            "{} cannot be used with these weapons",
            skill.rsplit('/').next().unwrap_or(skill)
        );
        return None;
    }
    if how != Use::ArrivedWarm && !caster.cooldowns.usable(skill) {
        debug!("cast {skill}: cooling down");
        // `HotSlotOptionSkill::PlayCooldownVox`: also the red "That skill is not ready" text.
        if how == Use::Request {
            commands.write_message(SkillNotReady);
            commands.write_message(crate::error_vox::ErrorVox::new(
                crate::error_vox::VoxKind::SkillCooldown,
                skill,
            ));
        }
        return None;
    }
    Some(level)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn try_cast(
    commands: &mut Commands,
    data: &GameData,
    player: Entity,
    tf: &mut Transform,
    caster: &mut Caster,
    loadout: &Loadout,
    effective: &gd_sim::skills::SkillBook,
    links: Option<&gd_game_core::weapon_pool::SkillLinks>,
    totals: &std::collections::BTreeMap<String, f32>,
    ch: &mut Character,
    path: &mut MovePath,
    skill: &str,
    point: Vec3,
    target: Option<Entity>,
    radii: f32,
    how: Use,
    classic: Walk,
) -> bool {
    let Some(rec) = data.db.get(skill) else {
        return false;
    };
    // `instantCast` bypasses the busy guard and the cast-animation lock (`DefaultRequestSkillAction` sends
    // `ActivateSkillConfigCmd` directly): the skill fires mid-swing / mid-cast.
    let instant = rec.bool("instantCast");
    if !instant && (!ch.can_act() || ch.attack_delay > 0.0) {
        debug!("cast {skill}: busy ({:?})", ch.state);
        return false;
    }
    let kind = kind_of(rec);
    if kind == Kind::Unsupported {
        // The Pet Attack skill is an order to the pets.
        if rec.class != crate::pets::PET_ATTACK_CLASS {
            info!("skill {skill} ({}) is not castable yet", rec.class);
        }
        return false;
    }
    // A charge's warm-up started the cooldown and paid.
    let warm = how == Use::ArrivedWarm;
    let Some(level) = request_gates(
        commands, data, caster, loadout, effective, links, rec, skill, how,
    ) else {
        return false;
    };
    let li = (level - 1) as usize;
    // Wrapper skills (`Skill_BuffRadius`, ...) keep cost, cooldown, radius, damage and duration in
    // `buffSkillName`; a hot slot cast runs as that record.
    let data_name = gd_sim::skills::profile_name(rec)
        .unwrap_or(skill)
        .to_owned();
    // The animation follows the skill the player cast.
    let anim_class = rec.class.as_str();
    let Some(rec) = data.db.get(&data_name) else {
        return false;
    };
    // Cost and cooldown include the learned modifiers' (`Skill::GetSkillModifierData`).
    let mods: Vec<(&Record, usize)> = links
        .map(|l| {
            l.mods_of(skill)
                .iter()
                .filter_map(|(m, ml)| data.db.get(m).map(|r| (r, *ml)))
                .collect()
        })
        .unwrap_or_default();
    let lf =
        |k: &str| level_f(rec, k, li) + mods.iter().map(|(r, l)| level_f(r, k, *l)).sum::<f32>();
    // `skillManaCostReduction` / `skillCooldownReduction` % (`Skill::GetTotalSkillAttribute`): the skill's own
    // total (record + modifiers) plus the character's (`totals`); a value with a `…Chance` strictly between 0
    // and 100 counts only when a fresh 0–100 roll is at or under it (`SkillAttribute::GetTotalValue`, rolled
    // per call). The cost's reduction rolls before the cooldown's, as in the engine.
    let cost = gd_sim::actor::skill_energy_cost(
        rec,
        li,
        &mods,
        loadout.is_item_skill(&data.db, skill),
        gd_sim::actor::skill_attribute(totals, "skillManaCostReduction"),
        |c| gd_game_core::rng::f32() * 100.0 <= c,
    );
    let cost = if warm { 0.0 } else { cost };
    if caster.energy < cost {
        info!(
            "not enough energy for {skill} ({cost:.0} > {:.0})",
            caster.energy
        );
        // `DefaultRequestSkillAction`: a requested skill short of energy says so.
        if how == Use::Request {
            commands.write_message(crate::error_vox::ErrorVox::new(
                crate::error_vox::VoxKind::NotEnoughEnergy,
                skill,
            ));
        }
        return false;
    }
    let cd = cooldown_for(
        rec,
        li,
        &mods,
        gd_sim::actor::skill_attribute(totals, "skillCooldownReduction"),
    );
    let charges = lf("cooldownCharges").max(1.0) as u32;
    if instant {
        // The config-command path: no state change, no animation, the effect at once.
        caster.energy -= cost;
        if !warm {
            caster.cooldowns.start(skill.to_owned(), cd, charges);
        }
        gd_game_core::visuals::notify(commands, skill);
        commands
            .entity(player)
            .insert(crate::combo::ComboUse::new(&data_name, rec, cd));
        if data_name != skill {
            commands.entity(player).insert(CastWrapper {
                buff: data_name.clone(),
                wrapper: skill.to_owned(),
            });
        } else {
            commands.entity(player).try_remove::<CastWrapper>();
        }
        commands.write_message(ProcCast {
            caster: player,
            cast: Casting {
                skill: data_name,
                level_index: li,
                kind,
                point,
                target,
                hostile: false,
                parent: None,
            },
        });
        return true;
    }
    // Out of reach: walk into range first; the cast follows when the walk ends (`MoveToUseSkill`). A weapon
    // charge's warm-up pays and starts the cooldown now.
    if how == Use::Request
        && walks_into_range(rec, kind, classic)
        && (point - tf.translation).with_y(0.0).length() > usable_range(data, rec, radii)
    {
        let warm_up = warms_up(rec).then(|| {
            caster.energy -= cost;
            caster.cooldowns.start(skill.to_owned(), cd, charges);
            info!(
                "charge {} L{level} for {cost:.0} energy",
                skill.rsplit('/').next().unwrap_or(skill)
            );
            start_warm_up(commands, data, player, rec, skill, li, *tf);
            (
                lf("characterRunSpeedModifier"),
                data.engine().absolute_run_cap,
            )
        });
        if warm_up.is_some() {
            commands.entity(player).try_insert(WarmPaid(data_name));
        } else {
            commands.entity(player).try_remove::<WarmPaid>();
        }
        commands.entity(player).try_insert(MoveToUseSkill {
            charge: charges_to_range(rec),
            warm_up,
            skill: skill.to_owned(),
            point,
            target,
            dest: None,
        });
        return true;
    }
    // Ranged-profile skills fire from where the player stands: past the usable distance the target is dropped
    // and the point becomes the skill's range along the line (`ControllerPlayerStateUseSkill::OnBegin`); a
    // `targetingMode` `Target` skill is refused instead, unpaid.
    let to = (point - tf.translation).with_y(0.0);
    let range = skill_range(data, rec);
    let clamped = !walks_into_range(rec, kind, classic)
        && kind != Kind::Buff
        && to.length() > usable_range(data, rec, radii);
    if clamped && gd_sim::skills::refused_out_of_range(rec) {
        debug!("cast {skill}: target out of range");
        return false;
    }
    // `Skill_AttackSpellReflectLink::ActivateNow`: a target beyond `maxRange` starts no link, cooldown or cost.
    // PORT: measured feet to aim point, not emit point to emit point; a link already on the target is not
    // looked up.
    if kind == Kind::ReflectLink
        && rec
            .f32("maxRange")
            .is_some_and(|m| (point - tf.translation).with_y(0.0).length() > m)
    {
        debug!("cast {skill}: link target out of range");
        return false;
    }
    commands.entity(player).remove::<MoveToUseSkill>();
    // A new action ends a running channel (`SkillManager::StopCurrentSkill`); a cut-short charge's warm-up
    // marker goes too (the charge keeps what it paid). Cost and cooldown are paid at the cast's fire.
    commands.entity(player).try_remove::<Channel>();
    if how != Use::ArrivedWarm {
        commands.entity(player).try_remove::<WarmPaid>();
    }
    info!(
        "cast {} L{level} ({kind:?}) at {:.1} m",
        skill.rsplit('/').next().unwrap_or(skill),
        (point - tf.translation).with_y(0.0).length()
    );
    gd_game_core::visuals::notify(commands, skill);
    let mut point = point;
    let mut target = target;
    // An ally skill keeps its target: its effect checks the range when it lands. (The engine drops the target
    // here and `Skill_GiveBonus::ActivateNow` then finds none.)
    let ally_skill = gd_sim::skills::target_type(&rec.class) == gd_sim::skills::TargetType::Ally;
    if clamped {
        point = tf.translation + to.normalize() * range;
        if !ally_skill {
            target = None;
        }
    }
    // An evade's recovery is cancelable once its dash is over.
    if ch.state == CharState::Evade {
        commands
            .entity(player)
            .try_remove::<(crate::evade::Evading, gd_game_core::casting::Leap)>();
    }
    path.points.clear();
    let special = loadout
        .skills
        .special_animation(&data.db, &data.skill_trees(), skill);
    if !ch.start_cast(anim_class, special) {
        ch.start_cast("Skill_AttackSpell", None);
    }
    // `AttackAction::Execute`: the player turns to the target as the skill starts.
    if kind != Kind::Buff {
        ch.begin_cast(tf, target.map(|t| (t, point)), point);
    }
    // `ControllerPlayerState::DefaultRequestSkillAction` resets the use's combo multiplier.
    commands
        .entity(player)
        .insert(crate::combo::ComboUse::new(&data_name, rec, cd));
    // The wrapper the hot slot named, for the cast triggers and this cast's cooldown.
    let wrapper = (data_name != skill).then(|| CastWrapper {
        buff: data_name.clone(),
        wrapper: skill.to_owned(),
    });
    if let Some(w) = wrapper {
        commands
            .entity(player)
            .insert((w.clone(), CastCooldownKey(w)));
    } else {
        commands
            .entity(player)
            .try_remove::<(CastWrapper, CastCooldownKey)>();
    }
    commands.entity(player).insert(Casting {
        skill: data_name,
        level_index: li,
        kind,
        point,
        target,
        hostile: false,
        parent: None,
    });
    // `skillActivatedAuraName`, else the hand FX (`Skill::GetParticleEffect1`); a toggle's loops while on.
    if kind != Kind::Toggle
        && gd_game_core::skill_fx::timed_aura(rec).is_none()
        && let Some(fx) = rec
            .str("skillActivatedAuraName")
            .map(str::to_owned)
            .or_else(|| {
                let mods = gd_game_core::skill_fx::override_mods(
                    &data.db,
                    links,
                    Some(&loadout.skills),
                    skill,
                );
                gd_game_core::skill_fx::particle_effect1(rec, &mods)
            })
    {
        spawn_fx(
            commands,
            data,
            &fx,
            Transform::from_translation(tf.translation + Vec3::Y),
        );
    }
    true
}

#[allow(clippy::too_many_arguments)]
pub fn channels(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut players: Query<
        (
            Entity,
            &mut Transform,
            &mut Caster,
            &mut Channel,
            &mut Character,
            &gd_game_core::player::ControllerDirection,
        ),
        (With<Player>, Without<Dead>),
    >,
    // `FilterInvalidTargets`: the player's foes (tested at each use).
    foes: Query<
        (
            Entity,
            &GlobalTransform,
            &Combatant,
            crate::combat::VictimSide,
        ),
        Without<Dead>,
    >,
    mut hits: MessageWriter<SkillHit>,
    mut elapsed: Local<f32>,
    mut activations: MessageWriter<crate::secondary::Activate>,
    (options, record): (
        Option<Res<gd_game_core::options::GameOptions>>,
        Res<crate::hotbar::WorldCursor>,
    ),
) {
    let Ok((e, tf, mut caster, mut ch, mut character, controller)) = players.single_mut() else {
        *elapsed = 0.0;
        return;
    };
    let dt = time.delta_secs();
    *elapsed += dt;
    let held = if ch.scripted {
        *elapsed < 2.0 && ch.slot < SLOTS
    } else {
        ch.input_held
    };
    let Some(rec) = data.db.get(&ch.skill) else {
        return;
    };
    let li = ch.level_index;
    let cost = level_f(rec, "skillManaCost", li);
    // The release: the character's channel state ends with the `Channel` (`character::drive_characters`).
    if !held {
        commands.entity(e).try_remove::<Channel>();
        *elapsed = 0.0;
        return;
    }
    // Aim: the cursor's ground point (scripted: the nearest foe). The held key re-requests the skill
    // (`ControllerPlayerStateUseSkill::RequestSkillAction`), so the keyboard modes aim it by `keyboard_aim`
    // (PORT: every frame, not per repeat).
    let movement = options.as_ref().map_or(0, |o| o.int("movementType"));
    let side = crate::combat::Side::Player;
    let (target, aim) = if ch.scripted {
        let near = foes
            .iter()
            .filter(|f| side.hits(&f.3))
            .map(|f| (f.0, f.1.translation()))
            .min_by(|a, b| {
                a.1.distance(tf.translation)
                    .total_cmp(&b.1.distance(tf.translation))
            });
        (near.map(|f| f.0), near.map(|f| f.1))
    } else if movement > 0 {
        let (target, point) = if movement == 2 {
            let alive = record
                .target
                .filter(|t| foes.get(*t).is_ok_and(|f| side.hits(&f.3)));
            (alive, record.point.or(Some(tf.translation)))
        } else {
            (None, ch.input_aim)
        };
        match point {
            Some(point) => {
                let aim = KeyboardAim {
                    movement_type: movement,
                    move_type: gd_sim::skills::move_type(&rec.class),
                    controller: *controller,
                    pos: tf.translation,
                    facing: tf.rotation * Vec3::Z,
                    range: gamepad_range(&data, rec),
                    gamepad_lock: options.as_ref().is_none_or(|o| o.flag("gamepadTargetLock")),
                };
                let (t, p, _) = keyboard_aim(&aim, target, point);
                (t, Some(p))
            }
            None => (None, None),
        }
    } else {
        (record.target, ch.input_aim)
    };
    let Some(aim) = aim else { return };
    ch.aim = Some(aim);
    // `Skill_AttackSpellCone::GetValidTarget` on each re-request: the request's target becomes the cone's.
    if rec.class == "Skill_AttackSpellCone" {
        ch.target = target;
    }
    // No direction (an aim at the player): the facing stays.
    let dir = (aim - tf.translation).with_y(0.0).normalize_or_zero();
    let dir = if dir == Vec3::ZERO {
        (tf.rotation * Vec3::Z).with_y(0.0).normalize_or_zero()
    } else {
        dir
    };
    if dir == Vec3::ZERO {
        return;
    }
    // The held channel turns the player to its aim at the skill's `rotationSpeedMultiplier`.
    character.turn.mult =
        gd_sim::turn::speed_multiplier(&rec.class, rec.f32("rotationSpeedMultiplier"))
            .unwrap_or(1.0);
    character.turn.look_at(tf.translation + dir);
    // A beam strikes and pays on its own rays' timer (`spell_beams`), a cone on its cone entry's (`class_fx`).
    if matches!(
        rec.class.as_str(),
        "Skill_AttackSpellBeam" | "Skill_AttackSpellCone"
    ) {
        return;
    }
    ch.tick -= dt;
    ch.held += dt;
    if ch.tick > 0.0 {
        return;
    }
    if caster.energy < cost {
        commands.entity(e).try_remove::<Channel>();
        *elapsed = 0.0;
        return;
    }
    caster.energy -= cost;
    let speed = if rec.class == "Skill_AttackRadiusSpin" {
        character.attack_speed
    } else {
        character.spell_speed
    };
    // The engine's re-arm: the whole period, the frame's overshoot dropped.
    let period = rec.f32("timeBetweenAttacks").unwrap_or(100.0) / 1000.0 / speed.max(0.2);
    ch.tick = period;
    let range = level_f(rec, "maxRange", li);
    let range = if range > 0.0 { range } else { 16.0 };
    let start = tf.translation;
    // - drain (`FindNewTargets`): the targeted foe within `maxRange` (`sparkMaxNumber` extras: no stock drain
    //   has them);
    // - spin: `skillTargetRadius` around the caster;
    // - grow (`Update`): that radius plus `radiusIncrement` per `timeBetweenGrow` ms (default 100) ÷ cast
    //   speed, up to `explosionRadius`.
    let end = start + dir * range;
    let targets: Vec<Entity> = match rec.class.as_str() {
        "Skill_AttackSpellDrain" => ch
            .target
            .and_then(|t| foes.get(t).ok())
            .filter(|(_, gt, mcb, v)| side.hits(v) && mcb.in_radius(&gt.affine(), start, range))
            .map(|f| f.0)
            .into_iter()
            .collect(),
        "Skill_AttackRadiusSpin" | "Skill_AttackRadiusGrow" => {
            let r = if rec.class == "Skill_AttackRadiusGrow" {
                grow_radius(rec, li, ch.held, character.spell_speed)
            } else {
                level_f(rec, "skillTargetRadius", li).max(1.0)
            };
            foes.iter()
                .filter(|(_, gt, mcb, v)| side.hits(v) && mcb.in_radius(&gt.affine(), start, r))
                .map(|f| f.0)
                .collect()
        }
        _ => Vec::new(),
    };
    for &m in &targets {
        hits.write(SkillHit {
            attacker: e,
            target: m,
            skill: ch.skill.clone(),
            level_index: li,
            parent: None,
            scale: 1.0,
            projectile: false,
        });
    }
    // A spin / grow pulse or a drain tick is a `TargetResult`; beams and cones run no secondaries.
    if matches!(
        rec.class.as_str(),
        "Skill_AttackRadiusSpin" | "Skill_AttackRadiusGrow" | "Skill_AttackSpellDrain"
    ) {
        activations.write_batch(crate::secondary::Activate::on_hits(
            e, &ch.skill, li, &None, targets, start,
        ));
    }
    if let Some(fx) = rec.str("endEffect") {
        spawn_fx(
            &mut commands,
            &data,
            fx,
            Transform::from_translation(end + Vec3::Y),
        );
    }
    gd_game_core::visuals::notify(&mut commands, &ch.skill);
}
