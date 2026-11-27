use super::*;
use bevy::ecs::system::SystemParam;
use std::cell::RefCell;

#[allow(clippy::too_many_arguments)]
pub(super) fn move_to_use_skill(
    mut commands: Commands,
    data: Res<GameData>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    options: Option<Res<gd_game_core::options::GameOptions>>,
    nav: Res<Nav>,
    mut players: Query<
        (
            Entity,
            &mut Transform,
            &mut Caster,
            (&Loadout, &gd_game_core::player::EffectiveSkills),
            &mut Character,
            &mut MovePath,
            &mut Combatant,
            Option<&gd_game_core::weapon_pool::SkillLinks>,
            &mut MoveToUseSkill,
            Has<crate::movement::Stunned>,
            Has<Dead>,
        ),
        With<Player>,
    >,
    foes: Query<
        (&GlobalTransform, &Combatant),
        (
            Or<(With<MonsterAi>, With<gd_game_core::world::Breakable>)>,
            Without<Dead>,
            Without<Player>,
        ),
    >,
) {
    let classic = options.is_some_and(|o| o.flag("classicCasting"));
    for (
        e,
        mut tf,
        mut caster,
        (loadout, effective),
        mut ch,
        mut path,
        cb,
        links,
        mut mv,
        stunned,
        dead,
    ) in &mut players
    {
        // Stunned: the state is left; dead target: Idle. A cancelled walk drops its [`WarmPaid`]
        // (the charge keeps what it paid).
        let foe = mv.target.map(|t| foes.get(t).ok());
        if stunned || dead || matches!(foe, Some(None)) {
            commands
                .entity(e)
                .try_remove::<(MoveToUseSkill, WarmPaid)>();
            path.points.clear();
            continue;
        }
        let foe = foe.flatten();
        let goal = foe.map_or(mv.point, |(gt, _)| gt.translation());
        let radii = cb.radius + foe.map_or(0.0, |(_, c)| c.radius);
        let Some(rec) = data.db.get(&mv.skill) else {
            commands
                .entity(e)
                .try_remove::<(MoveToUseSkill, WarmPaid)>();
            continue;
        };
        let d = (goal - tf.translation).with_y(0.0).length();
        if let Some(dest) = mv.dest
            && !path.points.is_empty()
            && path.points.last() != Some(&dest)
        {
            commands
                .entity(e)
                .try_remove::<(MoveToUseSkill, WarmPaid)>();
            continue;
        }
        let close = d <= usable_range(&data, rec, radii);
        let walked = mv.dest.is_some_and(|_| path.points.is_empty());
        let failed = mv.dest.is_none() && {
            let p = nav.0.find_path(
                &mut scratch,
                tf.translation.to_array(),
                goal.to_array(),
                crate::nav::PATH_BUDGET_LONG,
            );
            let ok = p.is_some();
            path.points = p
                .map(|p| p.into_iter().skip(1).map(Vec3::from).collect())
                .unwrap_or_default();
            mv.dest = path.points.last().copied();
            !ok
        };
        if let (Some(dest), Some(_)) = (mv.dest, foe)
            && (goal - dest).with_y(0.0).length() > 1.5
            && let Some(p) = nav.0.find_path(
                &mut scratch,
                tf.translation.to_array(),
                goal.to_array(),
                crate::nav::PATH_BUDGET_LONG,
            )
        {
            path.points = p.into_iter().skip(1).map(Vec3::from).collect();
            mv.dest = path.points.last().copied();
        }
        if !(close || walked || failed) {
            continue;
        }
        if matches!(
            ch.state,
            CharState::Cast | CharState::Attack | CharState::Evade
        ) {
            continue;
        }
        // In range, end of path or no path: UseSkill at the clicked point.
        path.points.clear();
        let (skill, point, target) = (mv.skill.clone(), mv.point, mv.target);
        let cast = try_cast(
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
            &skill,
            point,
            target,
            radii,
            if mv.warm_up.is_some() {
                Use::ArrivedWarm
            } else {
                Use::Arrived
            },
            Walk::new(classic, 0, true),
        );
        commands.entity(e).remove::<MoveToUseSkill>();
        // An arrival that cast nothing leaves no warmed cast to fire.
        if !cast {
            commands.entity(e).try_remove::<WarmPaid>();
        }
    }
}

/// Left-click spells: casts the left skill at the clicked monster when in range, else walks closer,
/// once per armed [`crate::player_orders::PrimaryUse`]; a pending-action skill goes first.
#[allow(clippy::too_many_arguments)]
pub(super) fn spell_attack_target(
    mut commands: Commands,
    data: Res<GameData>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
    options: Option<Res<gd_game_core::options::GameOptions>>,
    nav: Res<Nav>,
    coll: Res<crate::collision::WorldCollision>,
    mut players: Query<
        (
            Entity,
            &mut Transform,
            &mut Caster,
            (&Loadout, &gd_game_core::player::EffectiveSkills),
            &mut Character,
            &mut MovePath,
            &mut Combatant,
            Option<&RangedWeapon>,
            Option<&gd_game_core::weapon_pool::SkillLinks>,
            Has<MoveToUseSkill>,
            &mut crate::player_orders::PrimaryUse,
            Option<&Casting>,
            Option<&LastCast>,
        ),
        (
            With<SpellAttacker>,
            Without<Dead>,
            Without<crate::movement::Stunned>,
        ),
    >,
    targets: Query<
        (
            &Transform,
            &Combatant,
            Option<&gd_game_core::world::Breakable>,
            Option<&Character>,
            Has<MonsterAi>,
        ),
        (
            Without<Player>,
            Without<SpellAttacker>,
            Or<(With<MonsterAi>, With<gd_game_core::world::Breakable>)>,
            Without<Dead>,
        ),
    >,
) {
    let classic = options.is_some_and(|o| o.flag("classicCasting"));
    let Ok((
        e,
        mut tf,
        mut caster,
        (loadout, effective),
        mut ch,
        mut path,
        mut cb,
        ranged,
        links,
        pending,
        mut primary,
        casting,
        last_cast,
    )) = players.single_mut()
    else {
        return;
    };
    // A charge to the target is under way (`move_to_use_skill` ends it).
    if pending {
        return;
    }
    let Some(t) = cb.target else { return };
    if !primary.0 {
        if !matches!(ch.state, CharState::Attack | CharState::Cast) {
            cb.target = None;
        }
        return;
    }
    // A queued skill goes first (`cast_requests` runs it next frame).
    if matches!(
        ch.queued,
        Some(gd_game_core::character::QueuedAction::Cast(_))
    ) {
        return;
    }
    let Ok((ttf, tcb, brk, tch, is_monster)) = targets.get(t) else {
        cb.target = None;
        return;
    };
    let Some(skill) = caster.slots[SLOT_LEFT].clone() else {
        return;
    };
    // A "Kick" breakable is kicked whatever the left slot holds (`SelectPrimaryAction`).
    let kick = brk.is_some_and(|b| {
        data.db
            .get(&b.record)
            .is_some_and(crate::hotbar::kicks_breakable)
    });
    let skill = if kick { DEFAULT_KICK.to_owned() } else { skill };
    // The held button's repeat of the running skill is dropped (`AttackAction::QueryActionPermission`):
    // it spends the armed use without a new cast, so a press casts once and only a repeat past the
    // clip's `AllowInterrupt` chains the next.
    if matches!(ch.state, CharState::Attack | CharState::Cast) {
        let running = running_skill(&ch, casting, last_cast);
        if repeats_running(&data, running, &skill) || !ch.allow_interrupt {
            primary.0 = false;
            return;
        }
    }
    let d = (ttf.translation - tf.translation).with_y(0.0).length();
    // Weapon skills need weapon reach (a ranged weapon's for shooters); the kick is always melee
    // (`Skill_Kick::GetValidTarget` → `Skill::GetValidMeleeTarget`).
    let rec = data.db.get(&skill);
    let kind = rec.map(kind_of);
    let reach = if kick {
        cb.reach(tcb)
    } else {
        match kind {
            Some(Kind::Weapon) => ranged.map_or(cb.reach(tcb), |r| r.reach),
            _ => {
                rec.map_or(gd_game_core::combat::MELEE_RANGE, |r| skill_range(&data, r))
                    + cb.radius
                    + tcb.radius
                    + 0.5
            }
        }
    };
    // A weapon charge charges from afar (ChargeToUseSkill).
    let charge = rec.is_some_and(|r| gd_sim::skills::is_weapon_charge(&r.class));
    // A skill that `Skill::GetRequiresLOS` is used only when `Skill::IsTargetInLOS` is clear, else
    // the player moves to a spot that sees the target (`ControllerAIStateT<..>::UseSkill`).
    // PORT: also for a character target of a projectile / beam / ranged-weapon attack.
    let shot = matches!(kind, Some(Kind::Projectile | Kind::Beam))
        || (kind == Some(Kind::Weapon) && ranged.is_some());
    let blocked =
        !kick && (crate::monster_skills::requires_los(&data, &skill) || (is_monster && shot)) && {
            let (head, aim) = crate::monster_skills::sight_ray(&data, &ch, &tf, ttf, tch);
            !crate::monster_skills::target_in_los(&coll, head, ttf.translation, aim)
        };
    if !blocked
        && (d <= reach
            || charge
            || (kind != Some(Kind::Weapon)
                && !rec
                    .is_some_and(|r| walks_into_range(r, kind_of(r), Walk::new(classic, 0, true)))))
    {
        primary.0 = !try_cast(
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
            &skill,
            ttf.translation,
            Some(t),
            cb.radius + tcb.radius,
            Use::Request,
            Walk::new(classic, 0, true),
        );
    } else if path.points.is_empty() && !matches!(ch.state, CharState::Cast) {
        // Blocked: a spot that sees the target (within reach), else straight at it.
        let dest = if blocked {
            crate::monster_skills::los_spot(
                &coll,
                &nav,
                &mut scratch,
                tf.translation,
                ttf.translation,
                reach,
                crate::monster_skills::character_center(&data, &ch).y,
            )
            .unwrap_or(ttf.translation)
        } else {
            ttf.translation
        };
        if let Some(p) = nav.0.find_path(
            &mut scratch,
            tf.translation.to_array(),
            dest.to_array(),
            crate::nav::PATH_BUDGET_LONG,
        ) {
            path.points = p.into_iter().skip(1).map(Vec3::from).collect();
        }
    }
}

/// Kinds whose secondaries wait for `TargetResult`: at the hit point for what it struck (swings,
/// radii, chains, waves, dispels), else later (impacts, area ticks, path charges, channel pulses).
pub(super) fn resolves_on_hits(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Weapon
            | Kind::SelfRadius
            | Kind::TargetRadius
            | Kind::Chain
            | Kind::Wave
            | Kind::LineFan
            | Kind::Line
            | Kind::Dispel
            | Kind::Projectile
            | Kind::Drop
            | Kind::Storm
            | Kind::Area
            | Kind::PathCharge
            | Kind::Beam
            | Kind::Geysers
            | Kind::Pattern
            | Kind::TurretFire
    )
}

/// Spells that hit only their target foe (no radius).
/// `Skill_AttackSpellTeleport::TargetResult` (Valdaran's swap): caster and target trade places
/// (`Character::TeleportToLocation`), the hit sound and target effect play at the caster's new
/// place, then the ordinary hit and secondaries. Both stop walking.
pub(super) fn swap_places(commands: &mut Commands, caster: Entity, target: Entity) {
    commands.queue(move |w: &mut World| {
        let (Some(a), Some(b)) = (
            w.get::<Transform>(caster).map(|t| t.translation),
            w.get::<Transform>(target).map(|t| t.translation),
        ) else {
            return;
        };
        for (e, to) in [(caster, b), (target, a)] {
            if let Some(mut tf) = w.get_mut::<Transform>(e) {
                tf.translation = to;
            }
            if let Some(mut p) = w.get_mut::<crate::movement::MovePath>(e) {
                p.points.clear();
            }
        }
    });
}

pub(super) fn single_target(class: &str) -> bool {
    matches!(
        class,
        "Skill_AttackSpell"
            | "Skill_AttackSpellChaos"
            | "Skill_AttackSpellChaosSpawnPet"
            | "Skill_AttackSpellTeleport"
    )
}

/// `Skill_AttackProjectileChain::ActivateNow`: the spark list at launch — the primary target, then
/// up to `sparkMaxNumber` jumps, each rolling `sparkChance` (a miss spends the jump) and picking a
/// random unhit foe within `sparkGap` of the last, LOS-tested. The impact attacks every entry.
#[allow(clippy::too_many_arguments)]
fn chain_sparks(
    data: &GameData,
    coll: &crate::collision::WorldCollision,
    chars: &Query<&gd_game_core::character::Character>,
    rec: &Record,
    li: usize,
    lf: &dyn Fn(&str) -> f32,
    caster_tf: &Transform,
    caster_ch: Option<&gd_game_core::character::Character>,
    first: (Entity, Vec3),
    monsters: &[(Entity, &Transform, &Combatant)],
) -> Vec<Entity> {
    let gap = lf("sparkGap").max(1.0);
    let jumps = lf("sparkMaxNumber").max(0.0) as usize;
    let chance = rec
        .get("sparkChance")
        .map_or(100.0, |_| level_f(rec, "sparkChance", li));
    let clear = |m: &(Entity, &Transform, &Combatant)| match caster_ch {
        Some(ch) => {
            let (head, aim) =
                crate::monster_skills::sight_ray(data, ch, caster_tf, m.1, chars.get(m.0).ok());
            crate::monster_skills::target_in_los(coll, head, m.1.translation, aim)
        }
        None => true,
    };
    let mut list = vec![first.0];
    let mut cur = first.1;
    let mut tries = 0;
    while tries < jumps {
        tries += 1;
        if (gd_game_core::rng::u32(0..=100) as f32) > chance {
            continue;
        }
        let near: Vec<_> = monsters
            .iter()
            .filter(|m| !list.contains(&m.0) && m.2.in_radius(&m.1.compute_affine(), cur, gap))
            .filter(|m| clear(m))
            .collect();
        if near.is_empty() {
            break;
        }
        let pick = near[gd_game_core::rng::usize(..near.len())];
        list.push(pick.0);
        cur = pick.1.translation;
    }
    list
}

/// World access shared by `fire_casts` and `fire_late_procs`.
#[derive(SystemParam)]
pub(super) struct FireCtx<'w, 's> {
    commands: Commands<'w, 's>,
    data: Res<'w, GameData>,
    scratch: Local<'s, RefCell<gd_sim::nav::PathScratch>>,
    nav: Res<'w, Nav>,
    casters: Query<
        'w,
        's,
        (
            &'static Transform,
            Option<&'static Casting>,
            &'static Combatant,
            Option<&'static mut Caster>,
            Option<&'static mut Loadout>,
            Option<&'static mut crate::monster_skills::MonsterSkills>,
            Option<&'static RangedWeapon>,
            Option<&'static mut crate::pets::Pet>,
            Option<&'static mut crate::autocast::Autocasts>,
            Option<&'static mut crate::autocast::SkillAutocasts>,
            Option<&'static WarmPaid>,
        ),
    >,
    all: Query<
        'w,
        's,
        (
            Entity,
            &'static Transform,
            &'static Combatant,
            crate::combat::VictimSide,
        ),
        Without<Dead>,
    >,
    hits: MessageWriter<'w, SkillHit>,
    pets: MessageWriter<'w, SpawnPet>,
    dispels: MessageWriter<'w, Dispel>,
    links: Query<
        'w,
        's,
        (
            &'static gd_game_core::weapon_pool::SkillLinks,
            Option<&'static mut gd_game_core::weapon_pool::PoolCharges>,
            Option<&'static crate::weapon_pool::PoolPick>,
        ),
    >,
    pool_rng: ResMut<'w, crate::weapon_pool::PoolRng>,
    activations: MessageWriter<'w, crate::secondary::Activate>,
    toggles: MessageWriter<'w, crate::shapeshift::Toggle>,
    enchanted: Query<
        'w,
        's,
        (
            Option<&'static gd_game_core::skill_fx::WeaponEnchantments>,
            Has<Player>,
        ),
    >,
    options: Res<'w, gd_game_core::options::GameOptions>,
    holdings: crate::combat_sounds::Holdings<'w, 's>,
    chars: Query<'w, 's, &'static gd_game_core::character::Character>,
    debufs: MessageWriter<'w, crate::skill_buffs::DebufTarget>,
    // Option: devotion is optional.
    devotion: Option<Res<'w, crate::devotion::DevotionState>>,
    world_coll: Res<'w, crate::collision::WorldCollision>,
    line_fans: MessageWriter<'w, crate::line_fan::Launch>,
    orbits: MessageWriter<'w, crate::orbit::Launch>,
    aims: Query<'w, 's, &'static AimPoint, With<MonsterAi>>,
    sides: Query<'w, 's, crate::combat::SideData>,
    wrappers: Query<'w, 's, &'static CastCooldownKey>,
}
/// Fires casts on the animation's hit callback, and the procs written before it.
pub(super) fn fire_casts(
    mut events: MessageReader<AnimCallback>,
    procs: Res<Messages<ProcCast>>,
    mut cursor: ResMut<ProcCursor>,
    mut ctx: FireCtx,
) {
    // `Casting` is removed with deferred commands: a hit and the `End` of
    // the same frame must not both fire it.
    let mut fired = gd_formats::collections::HashSet::default();
    // Animation hit points of a pending `Casting`, then autocast procs (no `Casting`).
    let mut work: Vec<(Entity, Casting, String)> = Vec::new();
    for ev in events.read() {
        let fire = matches!(ev.name.as_str(), "RightHandHit" | "LeftHandHit");
        let end = ev.name == "End";
        if !fire && !end {
            continue;
        }
        let Ok((_, Some(cast), ..)) = ctx.casters.get(ev.entity) else {
            continue;
        };
        // Only the cast's own clip fires it: the ended action's "End" may arrive the frame a new
        // cast starts on the idle character.
        if ctx
            .chars
            .get(ev.entity)
            .is_ok_and(|c| !c.callbacks_after_cast())
        {
            continue;
        }
        if !fired.insert(ev.entity) {
            continue;
        }
        work.push((ev.entity, cast.clone(), ev.name.clone()));
        ctx.commands
            .entity(ev.entity)
            .try_remove::<Casting>()
            .try_insert(LastCast {
                skill: cast.skill.clone(),
                weapon: cast.kind == Kind::Weapon,
                anim: ev.anim,
            });
    }
    work.extend(
        cursor
            .0
            .read(&procs)
            .map(|p| (p.caster, p.cast.clone(), "proc".to_owned())),
    );
    ctx.fire(work);
}

/// Procs sent after `fire_casts`, fired the same frame (`SkillAutoCastController::AppraiseAndExecute`
/// fires inside the hit).
pub(super) fn fire_late_procs(
    procs: Res<Messages<ProcCast>>,
    mut cursor: ResMut<ProcCursor>,
    mut ctx: FireCtx,
) {
    let work: Vec<(Entity, Casting, String)> = cursor
        .0
        .read(&procs)
        .map(|p| (p.caster, p.cast.clone(), "proc".to_owned()))
        .collect();
    if !work.is_empty() {
        ctx.fire(work);
    }
}

impl<'w, 's> FireCtx<'w, 's> {
    fn fire(&mut self, mut work: Vec<(Entity, Casting, String)>) {
        let Self {
            commands,
            data,
            nav,
            scratch,
            casters,
            all,
            hits,
            pets,
            dispels,
            links,
            pool_rng,
            activations,
            toggles,
            enchanted,
            options,
            holdings,
            chars,
            debufs,
            devotion,
            world_coll,
            line_fans,
            orbits,
            aims,
            sides,
            wrappers,
        } = self;
        let mut commands = commands.reborrow();
        // Pool skills picked by weapon-pool casts are appended and fired too.
        let mut wi = 0;
        while wi < work.len() {
            let (entity, mut cast, name) = work[wi].clone();
            wi += 1;
            let Ok((
                tf,
                _,
                cb,
                mut caster,
                loadout,
                mskills,
                ranged,
                pet,
                autocasts,
                skill_autocasts,
                warm_paid,
            )) = casters.get_mut(entity)
            else {
                continue;
            };
            // `ActivateNow` plays its activation sounds at the caster (PORT: every non-weapon cast).
            if cast.kind != Kind::Weapon
                && let Ok(c) = chars.get(entity)
            {
                for pak in crate::combat_sounds::activate(data, c, &cast.skill) {
                    debug!(
                        "{entity}: activate {} -> {pak}",
                        cast.skill.rsplit('/').next().unwrap_or("")
                    );
                    commands.write_message(gd_game_core::visuals::PlaySound {
                        pak,
                        at: Some(tf.translation),
                    });
                }
                // Radius attacks, summons, bursts: `skillHitSound` once at the caster.
                if let Some(pak) = crate::combat_sounds::spell_hit(
                    data,
                    &cast.skill,
                    crate::combat_sounds::HitAt::Caster,
                ) {
                    commands.write_message(gd_game_core::visuals::PlaySound {
                        pak,
                        at: Some(tf.translation),
                    });
                }
                // Summons, buff wrappers: once at the skill's point.
                if let Some(pak) = crate::combat_sounds::spell_hit(
                    data,
                    &cast.skill,
                    crate::combat_sounds::HitAt::Point,
                ) {
                    let at = if cast.kind == Kind::SelfRadius {
                        tf.translation
                    } else {
                        cast.point
                    };
                    commands.write_message(gd_game_core::visuals::PlaySound { pak, at: Some(at) });
                }
            }
            // The pool parent's secondaries for a pool pick, else the skill's own.
            let mut secondaries = Some(crate::secondary::Mode::Normal);
            // A weapon-pool attack: the finale or the pool skill decided as the action started
            // (`PoolPick`; a proc decides at once), then the charges; a picked pool skill fires instead.
            if cast.kind == Kind::Weapon
                && cast.parent.is_none()
                && let Some(prec) = data
                    .db
                    .get(&cast.skill)
                    .filter(|r| gd_game_core::weapon_pool::is_pool_parent(r))
                && let Ok((l, Some(mut pc), stored)) = links.get_mut(entity)
            {
                let started = match stored.filter(|p| name != "proc" && p.is_for(&cast.skill)) {
                    Some(p) => {
                        commands
                            .entity(entity)
                            .try_remove::<crate::weapon_pool::PoolPick>();
                        p.clone()
                    }
                    None => crate::weapon_pool::start_pool_action(
                        l,
                        &pc,
                        prec,
                        &cast.skill,
                        cast.level_index,
                        &mut pool_rng.0,
                    ),
                };
                let t = cast.target.or(cb.target).and_then(|t| all.get(t).ok());
                let has_target = t.is_some_and(|t| {
                    ranged.is_some()
                        || (t.1.translation - tf.translation).with_y(0.0).length()
                            <= cb.reach(t.2) + 1.0
                });
                let step = pc.step(
                    prec,
                    &cast.skill,
                    cast.level_index,
                    cb.profile.attack_speed,
                    has_target,
                    ranged.is_some(),
                    started.finale,
                );
                if step == gd_game_core::weapon_pool::PoolStep::Finale {
                    secondaries = Some(crate::secondary::Mode::All);
                }
                if let gd_game_core::weapon_pool::PoolStep::Attack { parent_damage } = step {
                    let parent = parent_damage.then(|| (cast.skill.clone(), cast.level_index));
                    if let Some(c) = started.pick.and_then(|p| {
                        crate::weapon_pool::pick_cast(
                            data,
                            p,
                            parent.clone(),
                            cast.point,
                            cast.target.or(cb.target),
                            cast.hostile,
                        )
                    }) {
                        if parent.is_none() {
                            // Cadence's ordinary attacks run no secondaries (only the finale runs
                            // them, `AlternateActivateSecondarySkills`); the data gives picked pool
                            // skills none.
                            let mut c = c;
                            c.parent = Some((String::new(), 0));
                            work.push((entity, c, name));
                            continue;
                        }
                        work.push((entity, c, name));
                        continue;
                    }
                    if !parent_damage {
                        // Cadence between finales: a plain default attack.
                        cast.skill = DEFAULT_ATTACK.into();
                        cast.level_index = 0;
                        secondaries = None;
                    }
                }
            }
            // `FactionManager::IsFoe(caster, it)`.
            let side = crate::combat::Side::lookup(sides, entity, cast.hostile);
            let monsters = std::cell::LazyCell::new(|| {
                all.iter()
                    .filter(|(e, _, _, v)| side.hits(v) && *e != entity)
                    .map(|(e, t, c, _)| (e, t, c))
                    .collect::<Vec<(Entity, &Transform, &Combatant)>>()
            });
            let foe = |e: Entity| {
                all.get(e)
                    .ok()
                    .filter(|(e, _, _, v)| side.hits(v) && *e != entity)
                    .map(|(e, t, c, _)| (e, t, c))
            };
            // Where a projectile at the cast's target aims ([`super::projectiles::aim_point`]).
            let target_body = cast
                .target
                .and_then(|t| all.get(t).ok())
                .map(|(_, t, c, _)| c.body_box(&t.compute_affine()));
            // What the hit point struck and where (a swing's: the attacker's coordinates).
            let mut struck: Vec<Entity> = Vec::new();
            let mut centre = tf.translation;
            let Some(rec) = data.db.get(&cast.skill) else {
                continue;
            };
            // A wrapper (curses, projectile debuffs, `Skill_BuffRadius`, `Skill_BuffOther`) loads its
            // profile from its `buffSkillName` record however it was cast. A `SkillBuff_Debuf*` record
            // lands as a debuff rather than a hit (`Skill_AttackBuff::TargetResult`); a projectile
            // debuff's projectile stays the wrapper's ([`projectile_debuf_wrapper`]).
            let wrapper = projectile_debuf_wrapper(&data.db, rec, loadout.as_deref());
            let wrapped = gd_sim::skills::profile_name(rec).map(str::to_owned);
            let rec = match wrapped.as_deref().and_then(|b| data.db.get(b)) {
                Some(b) => b,
                None => rec,
            };
            let debuff = gd_game_core::skill_buffs::lands_as_debuff(rec)
                .then(|| wrapped.clone().unwrap_or_else(|| cast.skill.clone()));
            // `Skill::GetDevotionLevel`.
            let devotion_level = devotion
                .as_deref()
                .filter(|_| enchanted.get(entity).is_ok_and(|e| e.1))
                .map_or(0, |d| d.devotion_level(&cast.skill));
            debug!("fire {} ({:?}) on {}", cast.skill, cast.kind, name);
            let li = cast.level_index;
            // Skill attributes are the profile values plus the learned modifiers'
            // (`Skill::GetSkillModifierData`); no % part.
            let mod_recs: Vec<(&Record, usize)> = links
                .get(entity)
                .map(|l| {
                    l.0.mods_of(&cast.skill)
                        .iter()
                        .filter_map(|(m, ml)| data.db.get(m).map(|r| (r, *ml)))
                        .collect()
                })
                .unwrap_or_default();
            let fx_mods = gd_game_core::skill_fx::override_mods(
                &data.db,
                links.get(entity).ok().map(|l| l.0),
                loadout.as_deref().map(|l| &l.skills),
                &cast.skill,
            );
            let lf = |k: &str| {
                level_f(rec, k, li) + mod_recs.iter().map(|(r, l)| level_f(r, k, *l)).sum::<f32>()
            };
            // Cost and cooldown are paid at the fire (`GetManaCost` → `SubtractMana` →
            // `StartCooldown`, then the effect); a wind-up cut short pays nothing. Only the player's
            // own cast pays here: procs, pool picks, monsters / pets and warmed charges ([`WarmPaid`])
            // paid elsewhere or not at all.
            if name != "proc"
                && cast.parent.is_none()
                && let Some(c) = caster.as_deref_mut()
            {
                if warm_paid.is_some_and(|w| w.0 == cast.skill) {
                    commands.entity(entity).try_remove::<WarmPaid>();
                } else {
                    let item = loadout
                        .as_ref()
                        .is_some_and(|l| l.is_item_skill(&data.db, &cast.skill))
                        || wrappers.get(entity).is_ok_and(|w| {
                            w.0.buff == cast.skill
                                && loadout
                                    .as_ref()
                                    .is_some_and(|l| l.is_item_skill(&data.db, &w.0.wrapper))
                        });
                    let cost = gd_sim::actor::skill_energy_cost(
                        rec,
                        li,
                        &mod_recs,
                        item,
                        gd_sim::actor::skill_attribute(
                            &cb.profile.totals,
                            "skillManaCostReduction",
                        ),
                        |c| gd_game_core::rng::f32() * 100.0 <= c,
                    );
                    if c.energy < cost {
                        debug!(
                            "cast {} fizzes: energy {:.0} < {cost:.0} at its fire",
                            cast.skill.rsplit('/').next().unwrap_or(&cast.skill),
                            c.energy
                        );
                        continue;
                    }
                    c.energy -= cost;
                    let cd = cooldown_for(
                        rec,
                        li,
                        &mod_recs,
                        gd_sim::actor::skill_attribute(
                            &cb.profile.totals,
                            "skillCooldownReduction",
                        ),
                    );
                    // A hot slot wrapper's cast runs as its `buffSkillName` record, but the cooldown
                    // is the wrapper's.
                    let key = wrappers
                        .get(entity)
                        .ok()
                        .filter(|w| w.0.buff == cast.skill)
                        .map_or_else(|| cast.skill.clone(), |w| w.0.wrapper.clone());
                    c.cooldowns
                        .start(key, cd, lf("cooldownCharges").max(1.0) as u32);
                }
            }
            // Attacks run the ordinary secondaries at `TargetResult` (below for the struck list,
            // later for impacts / ticks / arrivals); the rest and Cadence's finale here.
            let on_hits =
                secondaries == Some(crate::secondary::Mode::Normal) && resolves_on_hits(cast.kind);
            // The Primary pair runs at the cast of buff / radius / wave parents
            // (`PrimaryActivateSecondarySkills`); a `Skill_BuffSelfDuration` runs nothing else.
            let parent_class = data.db.get(&cast.skill).map_or("", |r| r.class.as_str());
            if cast.parent.is_none() && crate::secondary::primary_activates(parent_class) {
                activations.write(crate::secondary::Activate::primary(
                    entity,
                    &cast.skill,
                    cast.level_index,
                    tf.translation,
                ));
            }
            if let Some(mode) = secondaries
                .filter(|_| !on_hits)
                .filter(|_| !crate::secondary::primary_only(parent_class))
            {
                let (skill, level_index) = match &cast.parent {
                    Some((p, l)) => (p.clone(), *l),
                    None => (cast.skill.clone(), cast.level_index),
                };
                if !skill.is_empty() {
                    let targets = cast
                        .target
                        .or(cb.target)
                        .filter(|_| cast.kind == Kind::Weapon || cast.target.is_some())
                        .into_iter()
                        .collect();
                    activations.write(crate::secondary::Activate {
                        caster: entity,
                        skill,
                        targets,
                        point: cast.point,
                        mode,
                        level_index,
                        replica: false,
                    });
                }
            }
            // An `expansionTime` radius hits as it grows (`ExpandingRadius`, which also runs its
            // secondaries).
            let expanding = debuff.is_none()
                && matches!(
                    (cast.kind, rec.class.as_str()),
                    (Kind::SelfRadius, "Skill_AttackRadius")
                        | (Kind::TargetRadius, "SkillSecondary_AttackRadius")
                )
                && level_f(rec, "expansionTime", li) > 0.0;
            let hit =
                |hits: &mut MessageWriter<SkillHit>, struck: &mut Vec<Entity>, target: Entity| {
                    hits.write(SkillHit {
                        attacker: entity,
                        target,
                        skill: cast.skill.clone(),
                        level_index: li,
                        parent: cast.parent.clone(),
                        scale: 1.0,
                        projectile: false,
                    });
                    struck.push(target);
                };
            let in_radius = |c: Vec3, r: f32| -> Vec<Entity> {
                monsters
                    .iter()
                    .filter(|(_, mt, mcb)| mcb.in_radius(&mt.compute_affine(), c, r))
                    .map(|m| m.0)
                    .collect()
            };
            // `SkillActivated::CameraShake` (per bolt: `Skill_AttackRadiusLightning*::OnLightningComplete`).
            if rec.class.starts_with("Skill_AttackRadiusLightning") {
                let (secs, amp) = (lf("cameraShakeDurationSecs"), lf("cameraShakeAmplitude"));
                if secs > 0.0001 {
                    commands.write_message(gd_game_core::visuals::CameraShake {
                        ms: (secs * 1000.0).trunc(),
                        amp,
                        at: tf.translation,
                        own: enchanted.get(entity).is_ok_and(|e| e.1),
                    });
                }
            }
            match cast.kind {
                // A weapon skill with a ranged weapon shoots the weapon's projectile; the kick stays melee.
                Kind::Weapon if ranged.is_some() && rec.class != "Skill_Kick" => {
                    let (ench, player) = enchanted.get(entity).unwrap_or((None, false));
                    let Some(prec) = ranged.and_then(|r| {
                        data.db.get(&gd_game_core::skill_fx::weapon_projectile(
                            &data.db,
                            &r.projectile,
                            ench,
                            player,
                            options,
                        ))
                    }) else {
                        continue;
                    };
                    let mut c = cast.clone();
                    // A led shot keeps its aim point; else re-aim.
                    if let Ok(a) = aims.get(entity) {
                        c.point = a.0;
                        commands.entity(entity).try_remove::<AimPoint>();
                    } else if let Some(t) = c.target.or(cb.target).and_then(foe) {
                        c.point = t.1.translation;
                    }
                    if c.target == cast.target {
                        c.point = super::projectiles::aim_point(c.point, target_body.as_ref());
                    }
                    debug!(
                        "ranged weapon skill {} at {:?}",
                        c.skill.rsplit('/').next().unwrap_or(""),
                        c.point
                    );
                    let proj_mods = ProjMods {
                        effects: gd_game_core::skill_fx::projectile_effects(
                            &data.db,
                            links.get(entity).ok().map(|l| l.0),
                            loadout.as_deref().map(|l| &l.skills),
                            rec,
                            &fx_mods,
                            cast.parent.as_ref().map(|p| p.0.as_str()),
                        ),
                        ..default()
                    };
                    fire_weapon_projectile(
                        &mut commands,
                        data,
                        prec,
                        entity,
                        &c,
                        tf,
                        proj_speed(cb, lf("skillProjectileSpeedModifier")),
                        &holdings.of(entity),
                        &proj_mods,
                        chars.get(entity).ok(),
                        if name == "LeftHandHit" {
                            "L Hand"
                        } else {
                            "R Hand"
                        },
                    );
                }
                Kind::Weapon => {
                    let target = cast.target.or(cb.target);
                    if let Some(t) = target
                        && let Some((_, mt, mcb)) = foe(t)
                    {
                        // Hit-frame re-check: 3-D distance minus both radii within
                        // `meleeTargetDistance` (`Skill::IsTargetInRange`).
                        let mtd = data.engine().melee_target_distance;
                        let in_reach = crate::combat::in_melee_reach(
                            tf.translation,
                            mt.translation,
                            cb.radius + mcb.radius,
                            mtd,
                        );
                        // Out of reach: the cone at radius + meleeAutoTargetDistance (4.0) may still
                        // catch a foe who stepped in; the most centred one becomes the target.
                        let mut t = t;
                        let mut caught = in_reach;
                        if !in_reach {
                            let angle = lf("skillTargetAngle").clamp(0.0, 360.0);
                            if angle > 0.0 {
                                let cone = crate::combat::targets_in_cone(
                                    tf.translation,
                                    gd_game_core::character::facing_flat(tf),
                                    cb.radius,
                                    data.engine().melee_auto_target_distance,
                                    angle.to_radians() * 0.5,
                                    monsters.iter().map(|m| (m.0, m.1.translation, m.2.radius)),
                                );
                                if let Some(&c) = cone.first() {
                                    t = c;
                                    caught = true;
                                }
                            }
                        }
                        if caught {
                            hit(&mut *hits, &mut struck, t);
                            // `skillTargetAngle` (full width, 0–360; 0 = the target alone) cone of the
                            // facing, the target first, cut to `skillTargetNumber` (0 = no cap)
                            // (`GameEngine::GetTargetsInCone`). Extras past the cap: most centred first
                            // (`World::CompareConeHits`: descending flat facing · unit 3-D direction).
                            let n = lf("skillTargetNumber") as usize;
                            let angle = lf("skillTargetAngle").clamp(0.0, 360.0);
                            if angle > 0.0 && n != 1 {
                                let half = angle.to_radians() * 0.5;
                                let fwd = gd_game_core::character::facing_flat(tf);
                                let mut extra: Vec<(Entity, f32)> = monsters
                                    .iter()
                                    .filter(|m| m.0 != t)
                                    .filter_map(|m| {
                                        let d = (m.1.translation - tf.translation).with_y(0.0);
                                        let full = m.1.translation - tf.translation;
                                        let key =
                                            full.with_y(0.0).dot(fwd) / full.length().max(1e-6);
                                        (d.length() <= cb.reach(m.2) + 1.0
                                            && d.normalize_or_zero()
                                                .dot(fwd)
                                                .clamp(-1.0, 1.0)
                                                .acos()
                                                <= half)
                                            .then_some((m.0, key))
                                    })
                                    .collect();
                                extra.sort_by(|a, b| b.1.total_cmp(&a.1));
                                for (m, _) in
                                    extra
                                        .into_iter()
                                        .take(if n == 0 { usize::MAX } else { n - 1 })
                                {
                                    hit(&mut *hits, &mut struck, m);
                                }
                            }
                            // `Skill_AttackWeaponRadius::OnHitTarget`: each first-level hit splashes a
                            // separate result (no secondaries, no second splash) on the foes within
                            // `skillTargetRadius` of the hit one, that one excluded.
                            let splash = lf("skillTargetRadius");
                            if rec.class == "Skill_AttackWeaponRadius" && splash > 0.0 {
                                let primaries: Vec<(Entity, Vec3)> = struck
                                    .iter()
                                    .filter_map(|p| foe(*p).map(|(_, pt, _)| (*p, pt.translation)))
                                    .collect();
                                for (p, at) in primaries {
                                    for (m, mt, mcb) in monsters.iter() {
                                        if *m != p
                                            && mcb.in_radius(&mt.compute_affine(), at, splash)
                                        {
                                            hits.write(SkillHit {
                                                attacker: entity,
                                                target: *m,
                                                skill: cast.skill.clone(),
                                                level_index: li,
                                                parent: cast.parent.clone(),
                                                scale: 1.0,
                                                projectile: false,
                                            });
                                        }
                                    }
                                }
                            }
                        } else if rec.class == "Skill_AttackWeaponCharge"
                            || rec.class == "Skill_AttackWeaponBlink"
                        {
                            // Charges and blinks (Shadow Strike) run to the target at the warmed-up
                            // run speed and hit on arrival.
                            if warms_up(rec) {
                                start_warm_up(
                                    &mut commands,
                                    data,
                                    entity,
                                    rec,
                                    &cast.skill,
                                    li,
                                    *tf,
                                );
                            }
                            commands.entity(entity).try_insert(Charge::new(
                                rec,
                                &cast.skill,
                                li,
                                lf("characterRunSpeedModifier"),
                                tf.translation,
                                t,
                                lf("skillTargetAngle"),
                                lf("skillTargetNumber") as usize,
                                cast.hostile,
                            ));
                        }
                    }
                }
                Kind::Chain => {
                    let first = cast.target.and_then(foe).or_else(|| {
                        monsters
                            .iter()
                            .filter(|m| m.2.in_radius(&m.1.compute_affine(), cast.point, 3.0))
                            .min_by(|a, b| {
                                a.1.translation
                                    .distance(cast.point)
                                    .total_cmp(&b.1.translation.distance(cast.point))
                            })
                            .copied()
                    });
                    let Some(first) = first else { continue };
                    let gap = lf("sparkGap").max(1.0);
                    let jumps = lf("sparkMaxNumber").max(0.0) as usize;
                    let chance = rec
                        .get("sparkChance")
                        .map_or(100.0, |_| level_f(rec, "sparkChance", li));
                    let bolt = gd_game_core::skill_fx::lightning(rec, &fx_mods);
                    let mut chain = vec![first];
                    let mut from = launch_coords(data, tf, chars.get(entity).ok(), rec, "R Hand");
                    let mut tries = 0;
                    let mut fresh = true;
                    loop {
                        let cur = *chain.last().unwrap();
                        if fresh {
                            hit(&mut *hits, &mut struck, cur.0);
                            let to = cur.1.transform_point(chars.get(cur.0).map_or(Vec3::Y, |c| {
                                crate::monster_skills::character_center(data, c)
                            }));
                            if let Some(l) = &bolt {
                                commands.spawn(gd_game_core::visuals::PendingBolt {
                                    record: l.clone(),
                                    from,
                                    to,
                                });
                            }
                            from = to;
                        }
                        if tries >= jumps {
                            break;
                        }
                        tries += 1;
                        // A failed roll spends the jump without ending the chain.
                        fresh = (gd_game_core::rng::u32(0..=100) as f32) <= chance;
                        if !fresh {
                            continue;
                        }
                        let near: Vec<_> = monsters
                            .iter()
                            .filter(|m| {
                                !chain.iter().any(|c| c.0 == m.0)
                                    && m.2.in_radius(&m.1.compute_affine(), cur.1.translation, gap)
                            })
                            .copied()
                            .collect();
                        if near.is_empty() {
                            break;
                        }
                        chain.push(near[gd_game_core::rng::usize(..near.len())]);
                    }
                }
                Kind::PathCharge => {
                    // A teleport has no move (`Skill_AttackRadiusTeleport::ActivateNow` attacks at the
                    // target point). Leaps and disengages already moved between `StartJump` and
                    // `StopJump` ([`Leap`]) and hit around the caster. Path charges run at the
                    // warm-up run speed.
                    let leap = is_leap(&rec.class);
                    let teleport = rec.class.contains("Teleport");
                    let speed = if teleport { Some(f32::INFINITY) } else { None };
                    // `Skill_AttackSpellTeleportSelf` with `targetingMode` `Target` lands short of its
                    // target by both radii (PORT: on the straight line, not along the path).
                    let mut to = cast.point;
                    if rec.class == "Skill_AttackSpellTeleportSelf"
                        && gd_sim::skills::targeting_mode(rec)
                            == gd_sim::skills::TargetingMode::Target
                        && let Some((_, mt, mcb, ..)) = cast.target.and_then(|t| all.get(t).ok())
                    {
                        let away = (tf.translation - mt.translation).with_y(0.0);
                        let gap = cb.radius + mcb.radius;
                        to = if away.length() > gap {
                            mt.translation + away.normalize() * gap
                        } else {
                            tf.translation
                        };
                    }
                    let route = if leap {
                        Vec::new()
                    } else {
                        move_route(
                            nav,
                            &mut scratch.borrow_mut(),
                            rec,
                            tf.translation,
                            to,
                            teleport,
                        )
                    };
                    let mut radius = lf("skillTargetRadius").max(1.0)
                        * rec.f32("endRadiusMultiplier").unwrap_or(1.0).max(1.0);
                    if teleport && lf("skillTargetRadius") <= 0.0 {
                        radius = 0.0;
                    }
                    let path = match rec.class.as_str() {
                        "Skill_AttackPathCharge" => (
                            lf("skillTargetRadius"),
                            rec.f32("timeBetweenAttacks").unwrap_or(100.0).max(16.0) / 1000.0,
                            0.0,
                            Vec::new(),
                        ),
                        _ => (0.0, 1.0, 0.0, Vec::new()),
                    };
                    // `Skill_AttackPathCharge::WarmUpStart` ends in the weapon warm-up; the run
                    // stands for its move.
                    let warm =
                        rec.class == "Skill_AttackPathCharge" && rec.bool("skillAllowsWarmUp");
                    if warm {
                        start_warm_up(&mut commands, data, entity, rec, &cast.skill, li, *tf);
                    }
                    commands.entity(entity).try_insert(PathCharge {
                        to: route,
                        speed,
                        warm,
                        bonus: lf("characterRunSpeedModifier"),
                        skill: cast.skill.clone(),
                        level_index: li,
                        radius,
                        hostile: cast.hostile,
                        left: 3.0,
                        start: tf.translation,
                        path,
                        secondaries: (rec.class == "Skill_AttackPathCharge").then(|| {
                            (
                                rec.f32("secondarySkillDistance").unwrap_or(0.0),
                                tf.translation,
                            )
                        }),
                        radius_fx: gd_game_core::skill_fx::radius_effect(rec, li, &fx_mods),
                    });
                }
                Kind::Dispel => {
                    let r = lf("projectileExplosionRadius").max(1.0);
                    let foes = in_radius(cast.point, r);
                    centre = cast.point;
                    for t in &foes {
                        hit(&mut *hits, &mut struck, *t);
                    }
                    if let Some(fx) = rec
                        .strs_iter("skillProjectileName")
                        .nth(li)
                        .or_else(|| rec.strs_iter("skillProjectileName").last())
                    {
                        spawn_fx(
                            &mut commands,
                            data,
                            fx,
                            Transform::from_translation(cast.point + Vec3::Y),
                        );
                    }
                    dispels.write(Dispel {
                        caster: entity,
                        at: cast.point,
                        radius: r,
                        foes,
                        dots: rec.bool("dispelDamageOverTime"),
                    });
                }
                Kind::SelfRadius | Kind::TargetRadius => {
                    let c = if cast.kind == Kind::SelfRadius {
                        tf.translation
                    } else {
                        cast.point
                    };
                    centre = c;
                    if let Some(buff) = &debuff {
                        // `ApplyBuffOnTarget`: the buff record at the level with the skill's
                        // modifiers for its `skillActiveDuration` (whole ms).
                        let secs = (lf("skillActiveDuration") * 1000.0) as i32 as f32 / 1000.0;
                        let mods = links
                            .get(entity)
                            .map(|l| l.0.mods_of(&cast.skill).to_vec())
                            .unwrap_or_default();
                        for t in in_radius(c, lf("skillTargetRadius").max(1.0)) {
                            debufs.write(crate::skill_buffs::DebufTarget {
                                caster: entity,
                                target: t,
                                buff: buff.clone(),
                                skill: cast.skill.clone(),
                                level_index: li,
                                mods: mods.clone(),
                                devotion: devotion_level,
                                secs,
                            });
                            struck.push(t);
                        }
                    } else if single_target(&rec.class) {
                        if let Some(t) = cast.target.and_then(foe) {
                            if rec.class == "Skill_AttackSpellTeleport" {
                                swap_places(&mut commands, entity, t.0);
                            }
                            // `Skill_AttackSpellChaos::ActivateNow`: a beam that hits when its head
                            // arrives; a beam record that does not load never hits.
                            if rec.class.starts_with("Skill_AttackSpellChaos") {
                                if let Some(beam) =
                                    gd_game_core::skill_fx::chaos_beam(rec, &fx_mods)
                                {
                                    gd_game_core::visuals::notify(&mut commands, &beam);
                                    commands.spawn(gd_game_core::casting::ChaosBeam::new(
                                        entity,
                                        t.0,
                                        cast.skill.clone(),
                                        li,
                                        cast.parent.clone(),
                                        beam,
                                    ));
                                }
                            } else {
                                hit(&mut *hits, &mut struck, t.0);
                            }
                        }
                    } else if expanding {
                        commands.spawn(ExpandingRadius {
                            cast: cast.clone(),
                            caster: entity,
                            centre: c,
                            radius: lf("skillTargetRadius").max(1.0),
                            time: (level_f(rec, "expansionTime", li) * 1000.0) as i32 as f32
                                / 1000.0,
                            elapsed: 0.0,
                            hit: Vec::new(),
                            special: rec.class == "Skill_AttackRadius",
                        });
                    } else {
                        let r = lf("skillTargetRadius").max(1.0);
                        for t in in_radius(c, r) {
                            hit(&mut *hits, &mut struck, t);
                        }
                    }
                    // `Skill_AttackRadiusLightning` (Doom Bolt): the bolt itself (`Skill::GetLightning`).
                    if let Some(l) = gd_game_core::skill_fx::lightning(rec, &fx_mods) {
                        commands.spawn(gd_game_core::visuals::PendingBolt {
                            record: l,
                            from: launch_coords(data, tf, chars.get(entity).ok(), rec, "R Hand"),
                            to: c,
                        });
                    }
                    // One radius effect per level (sized to that level's radius; `fxChanges` replace it).
                    if let Some(fx) = gd_game_core::skill_fx::radius_effect(rec, li, &fx_mods)
                        .or_else(|| gd_game_core::skill_fx::target_fx(rec, li, &fx_mods))
                    {
                        spawn_fx(&mut commands, data, &fx, Transform::from_translation(c));
                    }
                }
                Kind::Projectile => {
                    let prec = match &wrapper {
                        Some(w) => projectile_debuf_name(w, li),
                        None => gd_game_core::skill_fx::projectile_name(rec, li, &fx_mods),
                    };
                    let Some(prec) = prec.and_then(|p| data.db.get(&p)) else {
                        continue;
                    };
                    // Modifiers, devotion level and duration are read at the impact.
                    let landing = wrapper
                        .as_ref()
                        .and(debuff.as_ref())
                        .map(|buff| ProjectileDebuf { buff: buff.clone() });
                    let trees = data.skill_trees();
                    let mut mods = loadout
                        .as_ref()
                        .map(|l| ProjMods::of(&data.db, &trees.parents, &l.skills, &cast.skill))
                        .unwrap_or_default();
                    mods.pierce += lf("projectilePiercingChance");
                    mods.fragments_name = gd_game_core::skill_fx::fragments_name(rec, &fx_mods);
                    mods.effects = gd_game_core::skill_fx::projectile_effects(
                        &data.db,
                        links.get(entity).ok().map(|l| l.0),
                        loadout.as_deref().map(|l| &l.skills),
                        rec,
                        &fx_mods,
                        cast.parent.as_ref().map(|p| p.0.as_str()),
                    );
                    // Rings / fans / bursts launch `projectileLaunchNumber` evenly over
                    // `projectileLaunchRotation` degrees centred on the aim. Others fire
                    // `skillProjectileNumber`: the first at the target, the rest at other foes within
                    // 1 m of it, then at a random sideways offset within ± tan(`skillProjectileAngle`
                    // / 2) × distance (default 45°; `SkillActivated::ExecuteRangedAttack`).
                    let launch = lf("projectileLaunchNumber");
                    let rot = lf("projectileLaunchRotation");
                    let spread = rec
                        .get("skillProjectileAngle")
                        .map_or(45.0, |_| level_f(rec, "skillProjectileAngle", li))
                        .to_radians();
                    let n = if launch > 0.0 {
                        launch
                    } else {
                        lf("skillProjectileNumber")
                    }
                    .max(1.0) as usize;
                    let speed = prec.f32("projectileVelocity").unwrap_or(20.0).max(1.0)
                        * proj_speed(cb, lf("skillProjectileSpeedModifier"));
                    let dist = prec.f32("projectileDistance").unwrap_or(MAX_RANGE).max(1.0);
                    let hand = if name == "LeftHandHit" {
                        "L Hand"
                    } else {
                        "R Hand"
                    };
                    let start = launch_coords(data, tf, chars.get(entity).ok(), rec, hand);
                    // `Skill_AttackProjectileBurst::ActivateNow`: `targetingMode` `Target` with a
                    // target aims at its `Target` attach point (PORT: bind pose). Else the request
                    // point itself, no height lift: the shot descends onto low breakables.
                    let burst_point = (matches!(
                        rec.class.as_str(),
                        "Skill_AttackProjectileBurst" | "Skill_WPAttack_ProjectileBurst"
                    ) && gd_sim::skills::targeting_mode(rec)
                        == gd_sim::skills::TargetingMode::Target)
                        .then(|| {
                            let t = cast.target?;
                            let (_, ttf, ..) = all.get(t).ok()?;
                            let local = attach_point_bind(&chars.get(t).ok()?.body, "Target")?;
                            Some(ttf.transform_point(local))
                        })
                        .flatten();
                    let aim = (burst_point.unwrap_or_else(|| {
                        super::projectiles::aim_point(cast.point, target_body.as_ref())
                    }) - start)
                        .normalize_or(gd_game_core::character::facing(tf));
                    // `ProjectileHoming::ProjectileGo`: no valid target, no launch.
                    if prec.class == "ProjectileHoming"
                        && cast.target.is_none_or(|t| all.get(t).is_err())
                    {
                        continue;
                    }
                    // The extra shots' foes within 1 m of the target (itself excluded).
                    let extra_aims: Vec<Vec3> = if n > 1 && rot <= 0.0 {
                        let at = cast
                            .target
                            .and_then(|t| all.get(t).ok())
                            .map(|(_, t, _, _)| t.translation)
                            .unwrap_or(cast.point);
                        monsters
                            .iter()
                            .filter(|m| {
                                Some(m.0) != cast.target && m.1.translation.distance(at) <= 1.0
                            })
                            .map(|m| {
                                super::projectiles::aim_point(
                                    m.1.translation,
                                    Some(&m.2.body_box(&m.1.compute_affine())),
                                )
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    // Orbiting projectiles launch one by one ([`crate::orbit`]).
                    if let Some(spec) = crate::orbit::BurstSpec::of(
                        rec,
                        li,
                        (cast.point, gd_game_core::character::facing_flat(tf)),
                    ) {
                        orbits.write(crate::orbit::Launch {
                            caster: entity,
                            cast: cast.clone(),
                            projectile: prec.name.clone(),
                            mods,
                            explosion: lf("projectileExplosionRadius"),
                            speed: proj_speed(cb, lf("skillProjectileSpeedModifier")),
                            how: crate::orbit::How::Burst(spec),
                        });
                        continue;
                    }
                    for i in 0..n {
                        let dir = if rot >= 360.0 {
                            Quat::from_rotation_y(i as f32 / n as f32 * std::f32::consts::TAU) * aim
                        } else if rot > 0.0 && n > 1 {
                            Quat::from_rotation_y(
                                (i as f32 / (n as f32 - 1.0) - 0.5) * rot.to_radians(),
                            ) * aim
                        } else if i == 0 {
                            aim
                        } else if let Some(p) = extra_aims.get(i - 1) {
                            (p - start).normalize_or(aim)
                        } else {
                            let half = (spread * 0.5).tan();
                            let a = ((gd_game_core::rng::f32() * 2.0 - 1.0) * half).atan();
                            Quat::from_rotation_y(a) * aim
                        };
                        // A ring of `ProjectileOrbiting` (Gaze of Korvaak): each starts along its ring
                        // heading, clockwise.
                        if prec.class == crate::orbit::PROJECTILE_CLASS {
                            orbits.write(crate::orbit::Launch {
                                caster: entity,
                                cast: cast.clone(),
                                projectile: prec.name.clone(),
                                mods: mods.clone(),
                                explosion: lf("projectileExplosionRadius"),
                                speed: proj_speed(cb, lf("skillProjectileSpeedModifier")),
                                how: crate::orbit::How::Now {
                                    from: tf.translation
                                        + Vec3::Y
                                        + dir.with_y(0.0).normalize_or_zero() * cb.radius,
                                    forward: dir,
                                },
                            });
                            continue;
                        }
                        skill_launch_sound(&mut commands, data, &cast.skill, start);
                        let proj = spawn_projectile(
                            &mut commands,
                            data,
                            prec,
                            entity,
                            &cast,
                            start,
                            dir * speed,
                            dist,
                            lf("projectileExplosionRadius"),
                            false,
                            &mods,
                        );
                        if let Some(l) = landing.clone() {
                            commands.queue(move |w: &mut World| {
                                if let Some(mut p) = w.get_mut::<Projectile>(proj) {
                                    p.debuf = Some(l);
                                }
                            });
                        }
                        if (rec.class == "Skill_AttackProjectileChain"
                            || prec.class == "ProjectileChain")
                            && let Some(first) = cast.target.and_then(foe)
                        {
                            let list = chain_sparks(
                                data,
                                world_coll,
                                chars,
                                rec,
                                li,
                                &lf,
                                tf,
                                chars.get(entity).ok(),
                                (first.0, first.1.translation),
                                &monsters,
                            );
                            commands.queue(move |w: &mut World| {
                                if let Some(mut p) = w.get_mut::<Projectile>(proj) {
                                    p.chain = list;
                                }
                            });
                        }
                    }
                }
                Kind::Storm => {
                    let Some(prec) = gd_game_core::skill_fx::projectile_name(rec, li, &fx_mods)
                    else {
                        continue;
                    };
                    commands.spawn((
                        Storm {
                            cast: cast.clone(),
                            around_caster: gd_sim::skills::targeting_mode(rec)
                                != gd_sim::skills::TargetingMode::Point,
                            projectile: prec,
                            left: lf("skillActiveDuration").max(0.5),
                            interval: level_f(rec, "skillTargetInterval", li).max(0.05),
                            timer: 0.0,
                            per_tick: lf("projectileLaunchNumber").max(1.0) as usize,
                            radius: lf("skillTargetRadius").max(1.0),
                            height: rec.f32("dropHeight").unwrap_or(12.0),
                            drop_variation: rec.f32("dropVariation").unwrap_or(0.0),
                            drop_radius: rec.f32("dropRadius").unwrap_or(0.0),
                            explosion: lf("projectileExplosionRadius").max(1.0),
                            attacker: entity,
                        },
                        Transform::from_translation(cast.point),
                    ));
                }
                Kind::Area => {
                    skill_launch_sound(&mut commands, data, &cast.skill, cast.point);
                    spawn_area(
                        &mut commands,
                        data,
                        rec,
                        cast.clone(),
                        entity,
                        lf("skillActiveDuration"),
                        lf("projectileExplosionRadius"),
                    );
                }
                Kind::Pattern => {
                    let target = cast.target.or(cb.target);
                    commands.write_message(crate::patterns::Fire {
                        caster: entity,
                        skill: cast.skill.clone(),
                        level_index: li,
                        point: cast.point,
                        target,
                    });
                }
                Kind::TurretFire => {
                    let target = cast.target.or(cb.target);
                    commands.write_message(crate::turrets::Command {
                        caster: entity,
                        skill: cast.skill.clone(),
                        point: cast.point,
                        target,
                    });
                }
                Kind::Geysers => {
                    commands.write_message(crate::geysers::Fire {
                        caster: entity,
                        cast: cast.clone(),
                        duration: lf("skillActiveDuration"),
                        radius: lf("projectileExplosionRadius"),
                    });
                }
                Kind::Drop => {
                    let Some(prec) = gd_game_core::skill_fx::projectile_name(rec, li, &fx_mods)
                        .and_then(|p| data.db.get(&p))
                    else {
                        continue;
                    };
                    // `Skill_AttackProjectileDrop::ActivateNow`: `numProjectiles` +
                    // `projectileLaunchNumber` land at a random angle and uniform distance up to
                    // `skillTargetRadius` from the target; each launches above its spot
                    // (`launchAboveTarget`, else from the caster), offset up to `dropRadius` at
                    // `dropHeight` ± `dropVariation`, at its own `projectileVelocity`.
                    let n = (lf("numProjectiles") + lf("projectileLaunchNumber")).max(1.0) as usize;
                    let h = rec.f32("dropHeight").unwrap_or(12.0);
                    let var = rec.f32("dropVariation").unwrap_or(0.0);
                    let side = rec.f32("dropRadius").unwrap_or(0.0);
                    let spread = lf("skillTargetRadius").max(0.0);
                    let above = rec.bool("launchAboveTarget");
                    let speed = prec.f32("projectileVelocity").unwrap_or(20.0).max(1.0);
                    let explosion = lf("projectileExplosionRadius").max(0.0);
                    let ring = |r: f32| {
                        let (a, d) = (
                            gd_game_core::rng::f32() * std::f32::consts::TAU,
                            gd_game_core::rng::f32() * r,
                        );
                        Vec3::new(a.cos() * d, 0.0, a.sin() * d)
                    };
                    for _ in 0..n {
                        let at = cast.point + ring(spread);
                        let p = at.with_y(
                            nav.0
                                .ground((at + Vec3::Y).to_array())
                                .unwrap_or(cast.point.y),
                        );
                        let base = if above {
                            p
                        } else {
                            tf.translation + Vec3::Y * 1.2
                        };
                        let start = base
                            + ring(side)
                            + Vec3::Y * (h + (gd_game_core::rng::f32() * 2.0 - 1.0) * var);
                        // Travel a bit past the landing spot: the ground test must see it land.
                        skill_launch_sound(&mut commands, data, &cast.skill, start);
                        spawn_projectile(
                            &mut commands,
                            data,
                            prec,
                            entity,
                            &cast,
                            start,
                            (p - start).normalize_or(Vec3::NEG_Y) * speed,
                            (p - start).length() + 2.0,
                            explosion,
                            true,
                            &ProjMods::default(),
                        );
                    }
                }
                Kind::Buff => {
                    if rec.class == "Skill_BuffSelfColossus" {
                        commands.write_message(crate::colossus::Grow {
                            caster: entity,
                            skill: cast.skill.clone(),
                        });
                    }
                    // Blade Barrier: no moving while it runs.
                    if rec.class == "Skill_BuffSelfImmobilize" {
                        commands
                            .entity(entity)
                            .try_insert_if_new(ImmobilizeRefresh {
                                skill: cast.skill.clone(),
                                timer: 0.0,
                            });
                    }
                    // `Skill_BuffSelfToggled::ModifyDamage`: see [`Shield`].
                    let (pct, flat) = (
                        level_f(rec, "damageAbsorptionPercent", li),
                        level_f(rec, "damageAbsorption", li),
                    );
                    if pct > 0.0 || flat > 0.0 {
                        let dur = rec
                            .get("skillActiveDuration")
                            .map(|_| lf("skillActiveDuration"))
                            .filter(|d| *d > 0.0);
                        let absorb = gd_sim::combat::Absorb {
                            pct,
                            flat,
                            pool: rec.class.ends_with("Shield"),
                            types: qualifying_types(rec),
                        };
                        let shield = Shield {
                            skill: cast.skill.clone(),
                            absorb,
                            left: dur,
                            kind: ShieldKind::Buff,
                        };
                        commands.queue(move |w: &mut World| {
                            if let Some(mut s) = w.get_mut::<Shields>(entity) {
                                s.add(shield);
                            }
                        });
                    }
                    // A timed damaging aura: 1 s ticks.
                    if rec.class.ends_with("BuffAttackRadiusDuration")
                        || rec.class == "Skill_BuffAttackRadiusLightning"
                    {
                        let mut aura = DamageAura::new(rec, &cast.skill, li, cast.hostile);
                        aura.left = Some(lf("skillActiveDuration"));
                        // The storm's bolts (`Skill::GetLightning` in `TargetResult`).
                        if rec.class == "Skill_BuffAttackRadiusLightning" {
                            aura.bolt = gd_game_core::skill_fx::lightning(rec, &fx_mods);
                        }
                        let skill = cast.skill.clone();
                        commands.queue(move |w: &mut World| {
                            let Ok(mut em) = w.get_entity_mut(entity) else {
                                return;
                            };
                            match em.get_mut::<DamageAuras>() {
                                Some(mut a) => {
                                    a.0.retain(|x| x.skill != skill);
                                    a.0.push(aura);
                                }
                                None => {
                                    em.insert(DamageAuras(vec![aura]));
                                }
                            }
                        });
                    }
                    // The buff going on gives its caster the profile's [`Bonus`]
                    // (`Skill_BuffSelfToggled::ReceiveActiveUpdate`), e.g. Blood of Dreeg's
                    // `skillLifePercent`; a radius buff's comes from its activation pass instead.
                    let is_radius = |s: &str| {
                        data.db
                            .get(s)
                            .is_some_and(|r| r.class == "Skill_BuffRadius")
                    };
                    let radius = if is_radius(&cast.skill) {
                        Some(cast.skill.clone())
                    } else {
                        loadout.as_deref().and_then(|l| {
                            l.skills
                                .skills
                                .iter()
                                .map(|(w, _)| w)
                                .find(|w| {
                                    is_radius(w)
                                        && data.db.get(w).is_some_and(|r| {
                                            gd_sim::skills::profile_name(r).is_some_and(|b| {
                                                b.eq_ignore_ascii_case(&cast.skill)
                                            })
                                        })
                                })
                                .cloned()
                        })
                    };
                    if let Some(w) = radius {
                        commands.write_message(crate::auras::Activated {
                            caster: entity,
                            mods: links
                                .get(entity)
                                .map(|l| l.0.mods_of(&w).to_vec())
                                .unwrap_or_default(),
                            skill: w,
                            level_index: li,
                        });
                    } else {
                        Bonus::of(lf).queue(&mut commands, entity);
                    }
                    // A wrapper's buff runs under the wrapper's name for the profile record's
                    // `skillActiveDuration`.
                    let dur = lf("skillActiveDuration");
                    if let Some(mut ms) = mskills {
                        ms.add_buff(&cast.skill, li, dur);
                        if let Some(fx) = rec.strs_iter("charFxPakSelfNames").next() {
                            spawn_fx(
                                &mut commands,
                                data,
                                fx,
                                Transform::from_translation(tf.translation),
                            );
                        }
                        continue;
                    }
                    if let Some(mut pet) = pet {
                        pet.add_buff(&cast.skill, li, dur);
                        if let Some(fx) = rec.strs_iter("charFxPakSelfNames").next() {
                            attach_buff_fx(&mut commands, data, fx, entity, &cast.skill, false);
                        }
                        continue;
                    }
                    // The player: the running buff under the cast's name; its stats derive at the
                    // profile rebuild ([`gd_game_core::skill_buffs::TimedBuffs`]).
                    let Some(mut loadout) = loadout else {
                        continue;
                    };
                    loadout.buffs.add(&cast.skill, li, dur);
                    if let Ok(l) = links.get(entity) {
                        let mods = l.0.mods_of(&cast.skill).to_vec();
                        debug!(
                            "buff {} with {} modifiers",
                            cast.skill.rsplit('/').next().unwrap_or(""),
                            mods.len()
                        );
                        loadout.buffs.mods.insert(cast.skill.clone(), mods);
                    }
                    commands.entity(entity).insert(ProfileDirty);
                    if let Some(fx) = rec.strs_iter("charFxPakSelfNames").next() {
                        attach_buff_fx(&mut commands, data, fx, entity, &cast.skill, false);
                    }
                }
                Kind::Pet => {
                    pets.write(SpawnPet {
                        owner: entity,
                        skill: cast.skill.clone(),
                        level_index: li,
                        point: cast.point,
                        target: cast.target,
                    });
                    if let Some(fx) = gd_game_core::skill_fx::target_fx(rec, li, &fx_mods) {
                        spawn_fx(
                            &mut commands,
                            data,
                            &fx,
                            Transform::from_translation(cast.point),
                        );
                    }
                }
                // Hits and secondaries land as the wave's front reaches each foe.
                Kind::Wave => {
                    let target = cast
                        .target
                        .or(cb.target)
                        .and_then(foe)
                        .map(|t| t.1.translation);
                    match crate::waves::start(
                        data,
                        nav,
                        world_coll,
                        rec,
                        &lf,
                        &cast,
                        entity,
                        tf,
                        target,
                        &mod_recs,
                        chars.get(entity).ok().map(|c| &*c.body),
                    ) {
                        Some(w) => crate::waves::spawn(&mut commands, rec, w),
                        None => debug!("wave {}: no target to start from", cast.skill),
                    }
                }
                // Hits, bursts and secondaries land as the lines reach the foes; number and angle
                // include the learned modifiers'.
                Kind::LineFan => {
                    let target = cast
                        .target
                        .or(cb.target)
                        .and_then(foe)
                        .map(|t| t.1.translation);
                    line_fans.write(crate::line_fan::Launch {
                        caster: entity,
                        skill: cast.skill.clone(),
                        level_index: li,
                        parent: cast.parent.clone(),
                        hostile: cast.hostile,
                        angle: lf("skillTargetAngle"),
                        number: lf("skillTargetNumber").max(0.0) as u32,
                        life: level_f(rec, "lifeTime", li),
                        target,
                    });
                }
                // Hits and secondaries land every period.
                Kind::Line => start_attack_line(&mut commands, data, rec, &cast, entity, tf),
                Kind::ReflectLink => {
                    let target = cast.target.or(cb.target).and_then(foe).map(|t| t.0);
                    commands.write_message(gd_game_core::reflect_link::StartLink {
                        caster: entity,
                        target,
                        skill: cast.skill.clone(),
                        level_index: li,
                    });
                }
                Kind::Instant => {
                    if rec.class == "SkillActivated_Suicide" {
                        commands.entity(entity).try_insert(crate::combat::Suicide);
                    } else if rec.class == "Skill_RefreshCooldown" {
                        // Every skill's cooldown shortens by `refreshTime` s
                        // (`SkillManager::RefreshCooldown`), its own excepted.
                        let t = level_f(rec, "refreshTime", li);
                        // The active, item and sub-skill lists: hotbar (potion keys skipped),
                        // autocast procs and skill-level children. A finished cooldown returns its charge.
                        let except = [
                            cast.skill.clone(),
                            HEALTH_POTION.to_owned(),
                            ENERGY_POTION.to_owned(),
                        ];
                        if let Some(mut c) = caster {
                            c.cooldowns.refresh_all(t, &except);
                        }
                        if let Some(mut a) = autocasts {
                            a.refresh_cooldowns(t, &except);
                        }
                        if let Some(mut sa) = skill_autocasts {
                            sa.refresh_cooldowns(t, &except);
                        }
                    } else {
                        // The profile's [`Bonus`], at once (`Skill_GiveBonus::TargetResult`). The
                        // player's friend target takes it only within 20 m edge to edge
                        // (`Skill_GiveBonus::ActivateNow`), else nobody (the cast is paid). PORT: the
                        // range is checked when the bonus lands. Other casters give it to their
                        // friend target or themselves.
                        let bonus = Bonus::of(lf);
                        let ally = cast.target.filter(|t| *t != entity);
                        let reach = (skill_range(data, rec) + MOVE_TOLERANCE).min(GIVE_BONUS_REACH);
                        let fx = gd_game_core::skill_fx::target_fx(rec, li, &fx_mods);
                        commands.queue(move |w: &mut World| {
                            let friend = ally.filter(|t| {
                                w.get::<Dead>(*t).is_none()
                                    && w.get::<crate::pets::Pet>(*t)
                                        .is_some_and(|p| p.owner == entity)
                            });
                            let who = if w.get::<Player>(entity).is_some() && ally.is_some() {
                                let edge = |t: Entity| {
                                    let (a, b) =
                                        (w.get::<Transform>(entity)?, w.get::<Transform>(t)?);
                                    let radii =
                                        w.get::<Combatant>(entity).map_or(0.0, |c| c.radius)
                                            + w.get::<Combatant>(t).map_or(0.0, |c| c.radius);
                                    Some(
                                        (a.translation - b.translation).with_y(0.0).length()
                                            - radii,
                                    )
                                };
                                friend.filter(|t| edge(*t).is_some_and(|d| d <= reach))
                            } else {
                                Some(friend.unwrap_or(entity))
                            };
                            let Some(who) = who else {
                                debug!("give bonus: the target is gone or out of reach");
                                return;
                            };
                            bonus.apply_to(w, who);
                            if let (Some(fx), Some(at)) =
                                (fx, w.get::<Transform>(who).map(|t| t.translation))
                            {
                                let data = w.resource::<GameData>().clone();
                                spawn_fx(
                                    &mut w.commands(),
                                    &data,
                                    &fx,
                                    Transform::from_translation(at),
                                );
                            }
                        });
                    }
                    if rec.class == "Skill_RefreshCooldown"
                        && let Some(fx) = gd_game_core::skill_fx::target_fx(rec, li, &fx_mods)
                    {
                        spawn_fx(
                            &mut commands,
                            data,
                            &fx,
                            Transform::from_translation(tf.translation),
                        );
                    }
                }
                Kind::Shapeshift => {
                    toggles.write(crate::shapeshift::Toggle {
                        caster: entity,
                        skill: cast.skill.clone(),
                        level_index: li,
                    });
                    if let Some(fx) = gd_game_core::skill_fx::target_fx(rec, li, &fx_mods) {
                        spawn_fx(
                            &mut commands,
                            data,
                            &fx,
                            Transform::from_translation(tf.translation),
                        );
                    }
                }
                // Toggles cast like any buff and switch on / off on the hit callback, paying the
                // cost either way; switching on plays the activate effect.
                Kind::Toggle => {
                    let Some(mut loadout) = loadout else { continue };
                    let low = cast.skill.to_ascii_lowercase();
                    // A toggle a dispel stopped is off: the use switches it on, `inactive` untouched.
                    let on = loadout.skills.dispelled.remove(&low)
                        || loadout.skills.inactive.remove(&low);
                    if !on {
                        loadout.skills.inactive.insert(low);
                    }
                    loadout.skills.touched();
                    info!(
                        "toggle {} {}",
                        cast.skill.rsplit('/').next().unwrap_or(&cast.skill),
                        if on { "on" } else { "off" }
                    );
                    if !on && let Some(pak) = crate::combat_sounds::deactivate(data, &cast.skill) {
                        commands.write_message(gd_game_core::visuals::PlaySound {
                            pak,
                            at: Some(tf.translation),
                        });
                    }
                    commands.entity(entity).insert(ProfileDirty);
                }
                Kind::Unsupported | Kind::Beam => {}
            }
            // `Skill_AttackRadius::Update` sends its attack only with foes in it.
            let struck_any = !struck.is_empty();
            if on_hits
                && let Some(a) = crate::secondary::Activate::on_hits(
                    entity,
                    &cast.skill,
                    li,
                    &cast.parent,
                    struck,
                    centre,
                )
            {
                activations.write(a);
            }
            // `Skill_AttackRadius::TargetResult` runs the special pair after the ordinary one,
            // whatever the radius held.
            if on_hits
                && rec.class == "Skill_AttackRadius"
                && !expanding
                && struck_any
                && let Some(a) = crate::secondary::Activate::special(
                    entity,
                    &cast.skill,
                    li,
                    &cast.parent,
                    centre,
                )
            {
                activations.write(a);
            }
        }
    }
}
