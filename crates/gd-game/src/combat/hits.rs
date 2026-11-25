use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_hits(
    mut commands: Commands,
    mut events: MessageReader<AnimCallback>,
    rules: Res<CombatRules>,
    // The main query and the cone fallback's victim view conflict on `Combatant`: the ParamSet keeps them sequential.
    mut qs: ParamSet<(
        Query<
            (
                &Transform,
                &mut Combatant,
                Option<&mut Character>,
                Option<&Monster>,
                Has<Player>,
                Option<&mut crate::afflictions::Afflictions>,
                &mut Attackers,
                crate::combat::SideData,
            ),
            Without<Dead>,
        >,
        // The cone fallback's candidates: every live combatant's side / hiding / breakable.
        Query<(Entity, &Transform, &Combatant, crate::combat::VictimSide), Without<Dead>>,
    )>,
    mut states: ResMut<ProxyStates>,
    (mut rng, mut fresh): (Local<Option<Rng>>, gd_game_core::session::NewSession),
    mut killed: MessageWriter<MonsterKilled>,
    (mut skill_hits, mut ticks, corpses): (
        MessageReader<gd_game_core::casting::SkillHit>,
        MessageReader<crate::skill_buffs::DebuffTick>,
        Query<(&Combatant, Option<&crate::afflictions::Afflictions>), With<Dead>>,
    ),
    // (casting / ranged / invincible / dummy marks)
    marks: Query<(
        Has<gd_game_core::casting::Casting>,
        Has<gd_game_core::casting::RangedWeapon>,
        Has<Invincible>,
        Has<gd_game_core::world::Dummy>,
    )>,
    data: Res<GameData>,
    mut casters: Query<AnyOf<(&mut gd_game_core::casting::Caster, &mut crate::pets::Pet)>>,
    (mut landed, mut made, mut combos, combo_rules, mut counters): (
        MessageWriter<HitLanded>,
        MessageWriter<AttackMade>,
        Query<(
            &mut crate::combo::Combo,
            Option<&mut crate::combo::ComboUse>,
        )>,
        // Option: no combo multipliers before `combo::init` made the rules.
        Option<Res<crate::combo::ComboRules>>,
        MessageWriter<CounterHit>,
    ),
    links: Query<(
        &gd_game_core::weapon_pool::SkillLinks,
        Option<&gd_game_core::weapon_pool::PoolCharges>,
    )>,
    picks: Query<&crate::weapon_pool::PoolPick>,
    mut procs: MessageWriter<gd_game_core::casting::ProcCast>,
    (mut shields, reflect_links, mut linked_attacks, loadouts, mut influenced, mut lifes): (
        Query<&mut gd_game_core::casting::Shields>,
        Query<&gd_game_core::reflect_link::ReflectLinks>,
        MessageWriter<LinkedAttack>,
        Query<&gd_game_core::player::Loadout>,
        MessageWriter<Influenced>,
        MessageWriter<LifeSubtracted>,
    ),
) {
    if fresh.begun() {
        *rng = None;
    }
    let rng = rng.get_or_insert_with(|| Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF)));
    // Melee swings, then skill hits: (attacker, target, skill, its weapon-pool parent, off-hand swing, damage scale,
    // projectile, a debuff's attack: its holder and modifiers).
    type Todo = (
        Entity,
        Entity,
        Option<(Arc<str>, usize)>,
        Option<(Arc<str>, usize)>,
        bool,
        f32,
        bool,
        Option<(Entity, Arc<[(String, usize)]>)>,
    );
    let mut todo: Vec<Todo> = Vec::new();
    let mtd = data.engine().melee_target_distance;
    // Copied out: the ParamSet's two queries cannot be borrowed at once.
    struct Swing {
        attacker: Entity,
        at: Vec3,
        radius: f32,
        side: crate::combat::Side,
        fwd: Vec3,
        is_monster: bool,
        left: bool,
        primary: (Entity, Vec3, f32),
    }
    let swings: Vec<Swing> = {
        let q = qs.p0();
        let mut out = Vec::new();
        for ev in events.read() {
            // Ranged weapons shoot instead.
            if !matches!(ev.name.as_str(), "RightHandHit" | "LeftHandHit")
                || marks.get(ev.entity).is_ok_and(|m| m.0 || m.1)
            {
                continue;
            }
            let Ok((atf, acb, _, amon, _, _, _, aside)) = q.get(ev.entity) else {
                continue;
            };
            // A skill cast's hit point is the callback's own identity, not the live state: the same step's "End" may
            // already have left `Cast`.
            if ev.cast {
                continue;
            }
            let Some(target) = acb.target else { continue };
            let Ok((ttf, tcb, ..)) = q.get(target) else {
                continue;
            };
            out.push(Swing {
                attacker: ev.entity,
                at: atf.translation,
                radius: acb.radius,
                side: aside.side(),
                fwd: gd_game_core::character::facing_flat(atf),
                is_monster: amon.is_some(),
                left: ev.name == "LeftHandHit",
                primary: (target, ttf.translation, tcb.radius),
            });
        }
        out
    };
    // The hit-frame re-check (`Skill::IsTargetInRange`); out of reach, the plain attack's 10° cone may catch a foe
    // who stepped in (`ExecuteMeleeAttack` → `GetTargetsInCone`).
    let mut resolved: Vec<(Entity, Entity, Vec3, bool, bool)> = Vec::new();
    {
        let victims = qs.p1();
        for s in swings {
            let (t, ttf, tr) = s.primary;
            let (target, hit_at) = if crate::combat::in_melee_reach(s.at, ttf, s.radius + tr, mtd) {
                (t, ttf)
            } else {
                let cone = crate::combat::targets_in_cone(
                    s.at,
                    s.fwd,
                    s.radius,
                    mtd,
                    5.0_f32.to_radians(),
                    victims
                        .iter()
                        .filter(|(e, _, _, v)| *e != s.attacker && s.side.hits(v))
                        .map(|(e, t, c, _)| (e, t.translation, c.radius)),
                );
                match cone.into_iter().next() {
                    Some(c) => {
                        let Ok((_, ttf, ..)) = victims.get(c) else {
                            continue;
                        };
                        (c, ttf.translation)
                    }
                    None => continue,
                }
            };
            resolved.push((s.attacker, target, hit_at, s.is_monster, s.left));
        }
    }
    for (attacker, target, hit_at, is_monster, left) in resolved {
        // A plain swing is a weapon-pool attack: the pool skill picked at swing start replaces it; the hit point is
        // the playing clip's (`SkillActivatedWeaponPool::StartAction`).
        if let Ok(p) = picks.get(attacker)
            && p.is_for(gd_game_core::casting::DEFAULT_ATTACK)
            && let Some(pick) = p.pick.clone()
            && let Some(cast) =
                crate::weapon_pool::pick_cast(&data, pick, None, hit_at, Some(target), is_monster)
        {
            procs.write(gd_game_core::casting::ProcCast {
                caster: attacker,
                cast,
            });
            continue;
        }
        todo.push((attacker, target, None, None, left, 1.0, false, None));
    }
    for h in skill_hits.read() {
        todo.push((
            h.attacker,
            h.target,
            Some((Arc::from(h.skill.as_str()), h.level_index)),
            h.parent
                .as_ref()
                .map(|(p, li)| (Arc::from(p.as_str()), *li)),
            false,
            h.scale,
            h.projectile,
            None,
        ));
    }
    // Debuffs' attacks on their holders.
    for t in ticks.read() {
        todo.push((
            t.caster,
            t.target,
            Some((Arc::from(t.buff.as_str()), t.level_index)),
            None,
            false,
            1.0,
            false,
            Some((t.holder, t.mods.clone())),
        ));
    }
    let mut q = qs.p0();
    let mut records: HashMap<Arc<str>, Option<&gd_formats::arz::Record>> = HashMap::default();
    for (attacker, target, skill, parent, left, damage_scale, projectile, debuff) in todo {
        // A dead caster's debuffs and skills still attack (`StartDyingSkill`; `Character::AttackTarget` has no life
        // check); plain swings need a live attacker.
        let Some((acb, aaff)) = q.get(attacker).ok().map(|a| (a.1, a.5)).or_else(|| {
            corpses
                .get(attacker)
                .ok()
                .filter(|_| debuff.is_some() || skill.is_some())
        }) else {
            continue;
        };
        // `PetPlayerScaling` pets attack as their player owner (`Skill::CollectCombatParameters`).
        let owner_cb = casters
            .get(attacker)
            .ok()
            .and_then(|(_, p)| p)
            .filter(|p| p.player_scaling)
            .and_then(|p| q.get(p.owner).ok().filter(|o| o.4).map(|o| o.1));
        let pcb = owner_cb.unwrap_or(acb);
        // What the attacker takes back is defended by its own profile: the owner swap is only the attack's collection.
        let own_prof = acb.profile.clone();
        let aprof = pcb.profile.clone();
        // A left-hand swing uses the off-hand weapon, and a charged `Skill_WeaponPool_ChargedScaling` attack leaves
        // its passive offensive modifiers out (`gd_sim::actor::Attacker`).
        let hand = left
            .then(|| aprof.off_hand())
            .flatten()
            .unwrap_or_else(|| gd_sim::actor::Attacker::from(&*aprof));
        let swing = skill
            .as_ref()
            .and_then(|(s, _)| hand.without_skill(s))
            .unwrap_or(hand);
        let adeb = aaff.map(|a| a.debuffs()).unwrap_or_default();
        // A debuff's attack rolls against its holder's defensive ability.
        let holder_da = debuff.as_ref().and_then(|(h, _)| q.get(*h).ok()).map(
            |(_, hcb, _, _, _, haff, _, _)| {
                gd_sim::actor::debuffed_defensive_ability(
                    &hcb.profile,
                    &haff.map(|a| a.debuffs()).unwrap_or_default(),
                )
            },
        );
        if debuff.is_some() && holder_da.is_none() {
            continue;
        }
        let Ok((ttf, mut tcb, mut tch, monster, is_player, mut taff, mut tatk, _)) =
            q.get_mut(target)
        else {
            continue;
        };
        if tcb.life <= 0.0 {
            continue;
        }
        let tprof = tcb.profile.clone();
        let tdeb = taff.as_ref().map(|a| a.debuffs()).unwrap_or_default();
        let rec = skill
            .as_ref()
            .and_then(|(s, _)| *records.entry(s.clone()).or_insert_with(|| data.db.get(s)));
        // The skill's learned modifiers and its weapon-pool parent (with its modifiers) add to it.
        let (alinks, charges) = links
            .get(attacker)
            .ok()
            .map_or((None, None), |(l, c)| (Some(l), c));
        let mod_recs = |s: &str| -> Vec<(&gd_formats::arz::Record, usize)> {
            alinks
                .map(|l| {
                    l.mods_of(s)
                        .iter()
                        .filter_map(|(m, li)| data.db.get(m).map(|r| (r, *li)))
                        .collect()
                })
                .unwrap_or_default()
        };
        // A debuff's are the buff's own.
        let own_mods = match &debuff {
            Some((_, mods)) => mods
                .iter()
                .filter_map(|(m, li)| data.db.get(m).map(|r| (r, *li)))
                .collect(),
            None => skill.as_ref().map(|(s, _)| mod_recs(s)).unwrap_or_default(),
        };
        let parent_rec = parent
            .as_ref()
            .and_then(|(p, li)| data.db.get(p).map(|r| (r, *li, mod_recs(p))));
        // Righteous Fervor's charges scale its own damage.
        let scale = |r: &gd_formats::arz::Record, s: &str| charges.map_or(1.0, |c| c.scale(r, s));
        // Combo charges: the use's multiplier (`gd_sim::combo`).
        let hit_combo = match (combo_rules.as_deref(), combos.get_mut(attacker)) {
            (Some(t), Ok((c, u))) => crate::combo::before_hit(
                &t.0,
                &c,
                u.map(|u| u.into_inner()),
                skill.as_ref().map(|s| &*s.0),
            ),
            _ => Default::default(),
        };
        let use_ = rec
            .zip(skill.as_ref())
            .map(|(r, (s, li))| gd_sim::actor::SkillUse {
                record: r,
                level_index: *li,
                mods: own_mods.iter().map(|(m, l)| (*m, *l)).collect(),
                parent: parent_rec
                    .as_ref()
                    .zip(parent.as_ref())
                    .map(|((pr, pli, pm), (ps, _))| {
                        Box::new(gd_sim::actor::SkillUse {
                            record: pr,
                            level_index: *pli,
                            mods: pm.iter().map(|(m, l)| (*m, *l)).collect(),
                            parent: None,
                            scale: scale(pr, ps),
                            damage_scale: 1.0,
                            style: gd_sim::combat::AttackKind::Melee,
                            combo: 0.0,
                            hands: alinks.map(|l| l.hands),
                            item_skill: false,
                        })
                    }),
                scale: scale(r, s),
                damage_scale,
                // Weapon attacks melee, projectiles ranged, other skills style 0.
                style: if let Some(holder_da) = holder_da {
                    // `SkillBuff_Debuf::Update`: style 4 with a multiplier of at least 1 rolled from the attack's
                    // OA and the holder's DA.
                    let pth = gd_sim::combat::probability_to_hit(
                        &rules.0.formulas,
                        gd_sim::actor::debuffed_offensive_ability(&aprof, &adeb),
                        holder_da,
                    );
                    gd_sim::combat::AttackKind::Debuff(
                        gd_sim::combat::damage_modifier(&rules.0.formulas, pth, rng.unit())
                            .multiplier
                            .max(1.0),
                    )
                } else if projectile {
                    gd_sim::combat::AttackKind::Ranged
                } else if gd_game_core::casting::kind_of(r) == gd_game_core::casting::Kind::Weapon {
                    gd_sim::combat::AttackKind::Melee
                } else {
                    gd_sim::combat::AttackKind::Direct
                },
                combo: hit_combo.scale,
                hands: alinks.map(|l| l.hands),
                item_skill: loadouts
                    .get(attacker)
                    .is_ok_and(|l| l.is_item_skill(&data.db, s)),
            });
        // The target's absorb filters and reflect links run on each damage type in turn.
        let mut sh = shields.get_mut(target).ok();
        let tlinks = reflect_links.get(target).ok();
        let own_filters = sh.as_ref().map_or(0, |s| s.0.len());
        let mut absorb: Vec<gd_sim::combat::Absorb> = sh
            .iter()
            .flat_map(|s| s.0.iter().map(|s| s.absorb.clone()))
            .collect();
        absorb.extend(
            tlinks
                .iter()
                .flat_map(|l| l.0.iter())
                .map(|l| gd_sim::combat::Absorb {
                    pct: l.absorb,
                    ..Default::default()
                }),
        );
        // Mana burn takes from the target's current energy (monsters' energy is not tracked: full).
        let energy = casters
            .get(target)
            .ok()
            .and_then(|(c, p)| c.map(|c| c.energy).or(p.map(|p| p.energy)))
            .map(|e| e.min(tprof.sheet.energy));
        let life_before = tcb.life;
        let mut hit = {
            let tcb = &mut *tcb;
            gd_sim::actor::attack_linked(
                &rules.0,
                swing,
                &tprof,
                &mut tcb.life,
                energy,
                &mut tcb.block_cooldown_ms,
                rng,
                use_,
                &adeb,
                &tdeb,
                &mut absorb,
                tlinks.map_or(0.0, |l| l.absorb()),
            )
        };
        // `Character::SubtractLife` takes nothing from an invincible character.
        if marks.get(target).is_ok_and(|m| m.2) {
            tcb.life = life_before;
            // No life removed: leech is % of the life actually lost.
            hit.leeched = 0.0;
            hit.applied.clear();
        }
        // A training dummy never dies.
        if marks.get(target).is_ok_and(|m| m.3) {
            tcb.life = tcb.life.max(1.0);
        }
        // `TakeAttack`'s attacker and `ApplyDamage`'s `RegisterDamage` ([`Attackers`]).
        let master = casters
            .get(attacker)
            .ok()
            .and_then(|(_, p)| p.map(|p| p.owner))
            .unwrap_or(attacker);
        tatk.register(attacker, master, hit.applied_total(), !is_player);
        lifes.write(LifeSubtracted {
            target,
            amount: hit.largest_applied(),
        });
        // The defender's own master, for what it sends back (reflection, links, retaliation).
        let tmaster = casters
            .get(target)
            .ok()
            .and_then(|(_, p)| p.map(|p| p.owner))
            .unwrap_or(target);
        if let Some(s) = sh.as_deref_mut()
            && own_filters > 0
        {
            for (f, a) in s.0.iter_mut().zip(&absorb[..own_filters]) {
                f.absorb = a.clone();
            }
            s.drop_spent();
        }
        // Spend, then gain (`SkillTargetResult`, then `IncrementComboCharge`).
        if let (Some(t), Ok((mut c, _))) = (combo_rules.as_deref(), combos.get_mut(attacker))
            && let (Some(r), Some((_, li))) = (rec, skill.as_ref())
        {
            let gain: Vec<(&gd_formats::arz::Record, usize)> = match parent_rec {
                Some((pr, pli, pm)) if crate::combo::gains_by_parent(r) => {
                    std::iter::once((pr, pli))
                        .chain(pm.iter().map(|(m, l)| (*m, *l)))
                        .collect()
                }
                _ => std::iter::once((r, *li))
                    .chain(own_mods.iter().map(|(m, l)| (*m, *l)))
                    .collect(),
            };
            let melee = gd_game_core::casting::kind_of(r) == gd_game_core::casting::Kind::Weapon
                && !marks.get(attacker).is_ok_and(|m| m.1);
            let reduction = aprof.skill_attribute("skillComboChargeSpendReduction");
            crate::combo::on_hit(
                &t.0,
                &mut c,
                hit_combo,
                reduction,
                rng,
                &gain,
                melee,
                aprof.attack_speed,
            );
        }
        made.write(AttackMade {
            attacker,
            target,
            skill: skill.as_ref().map(|s| s.0.clone()),
            critical: hit.critical,
            weapon: rec.as_ref().is_none_or(|r| {
                gd_game_core::casting::kind_of(r) == gd_game_core::casting::Kind::Weapon
            }),
            melee: !projectile
                && rec.as_ref().is_none_or(|r| {
                    gd_game_core::casting::kind_of(r) == gd_game_core::casting::Kind::Weapon
                }),
            miss: hit.miss,
            blocked: hit.blocked,
            multiplier: hit.multiplier,
            tier: hit.tier,
            total: hit.total,
            offensive_ability: gd_sim::actor::debuffed_offensive_ability(&aprof, &adeb),
            defensive_ability: gd_sim::actor::debuffed_defensive_ability(&tprof, &tdeb),
        });
        if !hit.missed {
            let melee = rec.as_ref().is_none_or(|r| {
                gd_game_core::casting::kind_of(r) == gd_game_core::casting::Kind::Weapon
            }) && !marks.get(attacker).is_ok_and(|m| m.1);
            // Gear, passive and skill taunt alike.
            let taunt = hit.influence.taunt;
            landed.write(HitLanded {
                attacker,
                target,
                skill: skill.as_ref().map(|s| s.0.clone()),
                critical: hit.critical,
                melee,
                blocked: hit.blocked,
                taunt,
            });
            if hit.influence.convert > 0.0 || hit.influence.disruption > 0.0 {
                influenced.write(Influenced {
                    attacker,
                    target,
                    influence: hit.influence,
                });
            }
        }
        if let Some(a) = taff.as_mut() {
            if !hit.dots.is_empty() || !hit.reflex.is_empty() {
                debug!(
                    "{attacker} -> {target}{}: {:?} {:?}",
                    if is_player { " (player)" } else { "" },
                    hit.dots
                        .iter()
                        .map(|d| (d.ty, d.dps[0].round(), (d.duration * 10.0).round() / 10.0))
                        .collect::<Vec<_>>(),
                    hit.reflex
                );
            }
            a.receive(&hit, attacker, skill.as_ref().map(|s| &*s.0));
        }
        if hit.energy_burned > 0.0
            && let Ok((c, p)) = casters.get_mut(target)
        {
            if let Some(mut c) = c {
                c.energy = (c.energy - hit.energy_burned).max(0.0);
            } else if let Some(mut p) = p {
                p.energy = (p.energy - hit.energy_burned).max(0.0);
            }
        }
        // Energy absorption: the defender gains that much energy (`CharacterBio::AddManaSlow`); monsters track none.
        if hit.energy_absorbed > 0.0
            && let Ok((c, p)) = casters.get_mut(target)
        {
            if let Some(mut c) = c {
                c.energy = (c.energy + hit.energy_absorbed).min(tprof.sheet.usable_energy());
            } else if let Some(mut p) = p {
                p.energy = (p.energy + hit.energy_absorbed).min(tprof.sheet.usable_energy());
            }
        }
        debug!(
            "hit {attacker} -> {target}: {:?} -> {:.1}/{:.1} {:?} by {:?}",
            hit,
            tcb.life,
            tprof.sheet.life,
            monster.map(|m| (m.record.rsplit('/').next().unwrap_or(""), m.level)),
            skill.as_ref().map(|s| s.0.rsplit('/').next().unwrap_or(""))
        );
        if tcb.life <= 0.0 {
            // `gibThreshold` (record int, percent of the max life; absent: never).
            if let Some(m) = monster
                && data
                    .db
                    .get(&m.record)
                    .and_then(|r| r.i32("gibThreshold"))
                    .is_some_and(|t| hit.overkills(life_before, tprof.sheet.life * t as f32 * 0.01))
            {
                commands.entity(target).try_insert(super::Overkill);
            }
            kill(
                &mut commands,
                target,
                ttf.translation,
                tch.as_deref_mut(),
                monster,
                is_player,
                &mut states,
                &mut killed,
                Some((attacker, skill.as_ref().map(|s| s.0.to_string()))),
            );
        }
        // Life leech: the leech percent of the life removed heals the attacker (`Character::TakeBonus`: heal
        // increase applies, capped at its maximum).
        if hit.leeched > 0.0
            && let Ok((_, mut acb, _, _, _, _, _, _)) = q.get_mut(attacker)
            && acb.life > 0.0
        {
            crate::casting::Bonus {
                life: hit.leeched,
                ..Default::default()
            }
            .apply(&own_prof.sheet, &mut acb.life, None);
        }
        // The target's reflect links pass their share on to each link's target.
        if !hit.link_copy.is_empty()
            && let Some(l) = tlinks
        {
            for link in &l.0 {
                let Ok((ltf, mut lcb, mut lch, lmonster, l_is_player, mut laff, mut latk, _)) =
                    q.get_mut(link.target)
                else {
                    continue;
                };
                if lcb.life <= 0.0 {
                    continue;
                }
                let ldeb = laff.as_ref().map(|a| a.debuffs()).unwrap_or_default();
                let lprof = lcb.profile.clone();
                let damage =
                    gd_sim::actor::link_reflection(&rules.0, &tprof, &hit.link_copy, link.reflect);
                // `ReflectLinkDamage` → `Character::AttackTarget`: the caster's attack triggers run; the copy keeps
                // the original attack's skill, weapon ids and crit.
                if link.reflect > 0.0 {
                    linked_attacks.write(LinkedAttack {
                        attacker: target,
                        target: link.target,
                        skill: skill.as_ref().map(|s| s.0.clone()),
                        critical: hit.critical,
                        weapon: rec.as_ref().is_none_or(|r| {
                            gd_game_core::casting::kind_of(r) == gd_game_core::casting::Kind::Weapon
                        }),
                    });
                }
                let mut link_leech = 0.0;
                if let Some(r) = gd_sim::actor::reflect(
                    &rules.0,
                    &tprof,
                    &lprof,
                    &mut lcb.life,
                    damage,
                    rng,
                    &ldeb,
                ) {
                    if marks.get(link.target).is_ok_and(|m| m.3) {
                        lcb.life = lcb.life.max(1.0);
                    }
                    latk.register(target, tmaster, r.applied_total(), !l_is_player);
                    lifes.write(LifeSubtracted {
                        target: link.target,
                        amount: r.largest_applied(),
                    });
                    debug!("reflect link {target} -> {}: {:.1}", link.target, r.damage);
                    if let Some(a) = laff.as_mut() {
                        a.receive(&r, target, None);
                    }
                    link_leech = r.leeched;
                    if r.energy_absorbed > 0.0
                        && let Ok((lc, lp)) = casters.get_mut(link.target)
                    {
                        if let Some(mut c) = lc {
                            c.energy =
                                (c.energy + r.energy_absorbed).min(lprof.sheet.usable_energy());
                        } else if let Some(mut p) = lp {
                            p.energy =
                                (p.energy + r.energy_absorbed).min(lprof.sheet.usable_energy());
                        }
                    }
                    counters.write(CounterHit {
                        attacker: target,
                        target: link.target,
                        hit: r,
                    });
                }
                if lcb.life <= 0.0 {
                    kill(
                        &mut commands,
                        link.target,
                        ltf.translation,
                        lch.as_deref_mut(),
                        lmonster,
                        l_is_player,
                        &mut states,
                        &mut killed,
                        Some((target, Some(link.skill.clone()))),
                    );
                }
                // The link attack's leech heals its attacker (this target).
                if link_leech > 0.0
                    && let Ok((_, mut tcb, _, _, _, _, _, _)) = q.get_mut(target)
                    && tcb.life > 0.0
                {
                    crate::casting::Bonus {
                        life: link_leech,
                        ..Default::default()
                    }
                    .apply(&tprof.sheet, &mut tcb.life, None);
                }
            }
        }
        // Reflected damage back at the attacker (any attack style).
        let mut reflect_leech = 0.0;
        if !hit.reflected.is_empty()
            && let Ok((atf, mut acb, mut ach, amonster, a_is_player, mut aaff, mut aatk, _)) =
                q.get_mut(attacker)
            && acb.life > 0.0
        {
            let adeb = aaff.as_ref().map(|a| a.debuffs()).unwrap_or_default();
            let r = gd_sim::actor::reflect(
                &rules.0,
                &tprof,
                &own_prof,
                &mut acb.life,
                hit.reflected.clone(),
                rng,
                &adeb,
            );
            let applied = r.as_ref().map_or(0.0, |r| r.applied_total());
            aatk.register(target, tmaster, applied, !a_is_player);
            lifes.write(LifeSubtracted {
                target: attacker,
                amount: r.as_ref().map_or(0.0, |r| r.largest_applied()),
            });
            if let Some(r) = r {
                debug!("reflected {target} -> {attacker}: {:.1}", r.damage);
                if let Some(a) = aaff.as_mut() {
                    a.receive(&r, target, None);
                }
                reflect_leech = r.leeched;
                if r.energy_absorbed > 0.0
                    && let Ok((c, p)) = casters.get_mut(attacker)
                {
                    if let Some(mut c) = c {
                        c.energy =
                            (c.energy + r.energy_absorbed).min(own_prof.sheet.usable_energy());
                    } else if let Some(mut p) = p {
                        p.energy =
                            (p.energy + r.energy_absorbed).min(own_prof.sheet.usable_energy());
                    }
                }
                counters.write(CounterHit {
                    attacker: target,
                    target: attacker,
                    hit: r,
                });
            }
            let attacker_died = acb.life <= 0.0;
            if attacker_died {
                kill(
                    &mut commands,
                    attacker,
                    atf.translation,
                    ach.as_deref_mut(),
                    amonster,
                    a_is_player,
                    &mut states,
                    &mut killed,
                    Some((target, None)),
                );
            }
            // The reflection's leech heals the reflector (even a dead attacker's).
            if reflect_leech > 0.0
                && let Ok((_, mut tcb, _, _, _, _, _, _)) = q.get_mut(target)
                && tcb.life > 0.0
            {
                crate::casting::Bonus {
                    life: reflect_leech,
                    ..Default::default()
                }
                .apply(&tprof.sheet, &mut tcb.life, None);
            }
            if attacker_died {
                continue;
            }
        }
        // Retaliation on landed melee hits (`TakeAttack` style 1: plain swings and weapon skills).
        let melee = rec.as_ref().is_none_or(|r| {
            gd_game_core::casting::kind_of(r) == gd_game_core::casting::Kind::Weapon
        }) && !marks.get(attacker).is_ok_and(|m| m.1);
        if hit.missed || !melee {
            continue;
        }
        let Ok((atf, mut acb, mut ach, amonster, a_is_player, mut aaff, mut aatk, _)) =
            q.get_mut(attacker)
        else {
            continue;
        };
        if acb.life <= 0.0 {
            continue;
        }
        let r = gd_sim::actor::retaliate(
            &rules.0,
            &tprof,
            &own_prof,
            &mut acb.life,
            rng,
            &tdeb,
            &adeb,
        );
        let applied = r.as_ref().map_or(0.0, |r| r.applied_total());
        aatk.register(target, tmaster, applied, !a_is_player);
        lifes.write(LifeSubtracted {
            target: attacker,
            amount: r.as_ref().map_or(0.0, |r| r.largest_applied()),
        });
        let Some(r) = r else {
            continue;
        };
        debug!("retaliation {target} -> {attacker}: {:.1}", r.damage);
        if let Some(a) = aaff.as_mut() {
            a.receive(&r, target, None);
        }
        let ret_leech = r.leeched;
        if r.energy_absorbed > 0.0
            && let Ok((c, p)) = casters.get_mut(attacker)
        {
            if let Some(mut c) = c {
                c.energy = (c.energy + r.energy_absorbed).min(own_prof.sheet.usable_energy());
            } else if let Some(mut p) = p {
                p.energy = (p.energy + r.energy_absorbed).min(own_prof.sheet.usable_energy());
            }
        }
        // Retaliation convert joins the attacker to the defender (`TakeAttack` style 3 runs `Execute` alike).
        if r.influence.convert > 0.0 {
            influenced.write(Influenced {
                attacker: target,
                target: attacker,
                influence: r.influence,
            });
        }
        counters.write(CounterHit {
            attacker: target,
            target: attacker,
            hit: r,
        });
        if acb.life <= 0.0 {
            kill(
                &mut commands,
                attacker,
                atf.translation,
                ach.as_deref_mut(),
                amonster,
                a_is_player,
                &mut states,
                &mut killed,
                Some((target, None)),
            );
        }
        // The retaliation's leech heals its holder (the target).
        if ret_leech > 0.0
            && let Ok((_, mut tcb, _, _, _, _, _, _)) = q.get_mut(target)
            && tcb.life > 0.0
        {
            crate::casting::Bonus {
                life: ret_leech,
                ..Default::default()
            }
            .apply(&tprof.sheet, &mut tcb.life, None);
        }
    }
}

/// `CombatManager::Update`: the shield's block recovery runs down.
pub(super) fn block_recovery(time: Res<Time>, mut q: Query<&mut Combatant>, mut carry: Local<f32>) {
    let ms = gd_game_core::session::frame_ms(&mut carry, &time);
    for mut c in &mut q {
        if c.block_cooldown_ms > 0 {
            c.block_cooldown_ms -= ms;
        }
    }
}

/// The attacked monster's aggro (`Character::UnderAttack`): every attack, hit or miss. Split from [`apply_hits`]
/// to avoid its conflicting `MonsterAi` access; runs right after it, before the conversion pass clears aggro.
pub(super) fn hit_aggro(
    mut made: MessageReader<AttackMade>,
    mut monsters: Query<(&mut MonsterAi, Has<Dead>)>,
) {
    for m in made.read() {
        if let Ok((mut ai, dead)) = monsters.get_mut(m.target)
            && !dead
        {
            ai.aggro = true;
        }
    }
}
