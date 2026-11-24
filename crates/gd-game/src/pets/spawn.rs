use super::*;

/// `ControllerAIStateT::GetSkillUseTolerance` (0.5 m).
pub(super) const SKILL_USE_TOLERANCE: f32 = 0.5;

/// Level of a pet skill: the owner's pet modifier unlocking it, else the
/// pet record's own `skillLevel{n}`.
pub(super) fn modifier_levels(data: &GameData, loadout: &Loadout) -> Vec<(String, u32)> {
    let book = loadout.effective_skills(&data.db, &data.skill_trees());
    book.skills
        .iter()
        .filter_map(|(s, _)| {
            let r = data.db.get(s)?;
            if r.class != "SkillSecondary_PetModifier" {
                return None;
            }
            let lvl = book.effective_level(&data.db, s);
            if lvl == 0 {
                return None;
            }
            Some((r.str("petSkillName")?.to_ascii_lowercase(), lvl))
        })
        // Gear pet modifiers (`modifierSkillName{n}` on worn items' records, the ascendant affix
        // included) at level 1 ([`gd_sim::skills::gear_pet_modifiers`]).
        .chain(book.gear_pet_mods.iter().cloned())
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_pets(
    mut commands: Commands,
    mut events: MessageReader<SpawnPet>,
    rules: Res<CombatRules>,
    // Option: optional input (the pet level falls back without progress).
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    owners: Query<(
        &Transform,
        &Loadout,
        Option<&PetStances>,
        Option<&gd_game_core::weapon_pool::SkillLinks>,
        Option<&gd_game_core::skill_buffs::ReceivedBuffs>,
    )>,
    mut pets: Query<(Entity, &Pet, &mut Combatant, &mut Character), Without<Dead>>,
    targets: Query<
        (
            &Transform,
            Option<&gd_game_core::monsters::Monster>,
            Option<&Pet>,
        ),
        Without<Dead>,
    >,
    nav: Res<Nav>,
    time: Res<Time>,
    mut assets: crate::creature::Spawner,
) {
    for ev in events.read() {
        // The database by its own handle: the record stays borrowed while `assets` spawns.
        let db = assets.data.db.clone();
        let Some(rec) = db.get(&ev.skill) else {
            continue;
        };
        let Ok((otf, loadout, stances, links, owner_buffs)) = owners.get(ev.owner) else {
            continue;
        };
        // The pet object placement measures (`Skill::PreLoadPet`): the
        // level's `spawnObjects`; each summon's own record is picked below.
        let Some(base_rec) = rec
            .strs_iter("spawnObjects")
            .nth(ev.level_index)
            .or_else(|| rec.strs_iter("spawnObjects").last())
            .map(|s| s.to_string())
        else {
            continue;
        };
        // `KillExtraPets`: after the new pet joined
        // the pen, `GetPetLimit` 0 = unlimited, else the oldest live pets of
        // this skill beyond the limit die through `KillMe(overkillOnDismiss)`
        // (dying skill runs). The new pet is not spawned yet, so the count
        // here is one short: kill while `old + 1 > limit`.
        // `Skill_SpawnQuestPet::SpawnPet`: before the
        // base spawn, with `SkillProfile.questPetVector` empty (PORT: the
        // default; no stock skill sets it, only the base template exists),
        // every unreleased pet of the owner's quest pen, of any skill, is
        // released alive (`UnJoinLeader(false)`): a new quest pet replaces the old.
        let quest = rec.class == "Skill_SpawnQuestPet";
        // `Skill::GetPetLimit`: the modifier-summed
        // `petLimit` / `petBurstSpawn` (the tooltip sums them too).
        let summon_mods: Vec<(gd_formats::arz::Record, usize)> = links
            .map(|l| {
                l.mods_of(&ev.skill)
                    .iter()
                    .filter_map(|(m, ml)| assets.data.db.get_owned(m).map(|r| (r, *ml)))
                    .collect()
            })
            .unwrap_or_default();
        let limit = gd_sim::skills::pet_limit(rec, ev.level_index, &summon_mods).max(0.0) as usize;
        let burst =
            gd_sim::skills::summed_attribute(rec, ev.level_index, &summon_mods, "petBurstSpawn");
        let mut mine: Vec<(Entity, f32)> = pets
            .iter()
            // (A quest pet's are all released before it spawns.)
            .filter(|(_, p, ..)| p.owner == ev.owner && p.summon == ev.skill && !p.quest && !quest)
            .map(|(e, p, ..)| (e, p.spawned_at))
            .collect();
        // Where and how many (`pets::placement`): a `Skill_TargetedSpawnPet`
        // packs its `petBurstSpawn` summons around the target / point, the
        // hit spawners (Reap Spirit) around what they hit, the
        // `Skill_SpawnPet` classes theirs around the owner.
        let target = ev
            .target
            .and_then(|t| targets.get(t).ok())
            .map(|(t, m, p)| {
                let r = m
                    .map(|m| m.record.as_str())
                    .or(p.map(|p| p.record.as_str()))
                    .and_then(|r| assets.data.db.get(r));
                (t.translation, gd_game_core::pets::placement::extents(r))
            });
        let base = assets.data.db.get(&base_rec);
        let spots: Vec<Vec3> = if rec.class == "Skill_TargetedSpawnPet" {
            super::placement::targeted_spots(
                &assets.data.db,
                &nav,
                rec,
                burst,
                base,
                &super::placement::Aim {
                    caster: otf.translation,
                    point: ev.point,
                    target,
                },
            )
        } else if super::placement::spawns_on_hit(&rec.class) {
            super::placement::hit_spots(&assets.data.db, &nav, rec, burst, base, ev.point, target)
        } else {
            let count = gd_game_core::pets::placement::spawn_pet_count(
                &rec.class,
                limit as u32,
                mine.len() as u32,
                burst.max(0.0) as u32,
            );
            if count == 0 {
                continue;
            }
            // The player's `GetExtents`: its record's `pathingSize` (Small).
            let radii = super::placement::spawn_pet_radii(&rec.class, 0.5, base);
            let mut set = super::placement::spawn_pet_set(&nav, otf.translation, radii, count);
            // `GetNewCasterCoords`: the caster keeps its place, the first
            // point is dropped.
            if !set.is_empty() {
                set.remove(0);
            }
            if quest {
                let y = nav.0.ground((ev.point + 2.0 * Vec3::Y).to_array());
                let at = ev.point.with_y(y.unwrap_or(ev.point.y));
                set.iter_mut().for_each(|s| *s = at);
            }
            set
        };
        if spots.is_empty() {
            continue;
        }
        if quest {
            for (e, p, mut cb, _) in &mut pets {
                if p.owner == ev.owner && p.quest {
                    release_quest_pet(&mut commands, e, &mut cb);
                }
            }
        }
        mine.sort_by(|a, b| a.1.total_cmp(&b.1));
        // The skill's learned and gear secondaries (`petChanges`,
        // [`gd_sim::skills::spawn_object`]).
        let key = ev.skill.to_ascii_lowercase();
        let names = |m: Option<&gd_formats::collections::HashMap<String, Vec<(String, usize)>>>| {
            m.and_then(|m| m.get(&key))
                .map(|v| v.iter().map(|s| s.0.clone()).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        let learned = names(links.map(|l| &l.secondaries));
        let gear = names(links.map(|l| &l.item_secondaries));
        for at in spots {
            // Each summon picks its record (`SpawnPet` per point).
            let pet_rec = gd_sim::skills::spawn_object(
                &assets.data.db,
                rec,
                ev.level_index,
                &learned,
                &gear,
                |n| gd_game_core::rng::u32(0..n),
            )
            .unwrap_or_else(|| base_rec.clone());
            let mut gone = Vec::new();
            while limit > 0 && mine.len() >= limit {
                let (old, _) = mine.remove(0);
                if let Ok((_, pet, mut cb, mut ch)) = pets.get_mut(old) {
                    dismiss(
                        &mut commands,
                        old,
                        pet.overkill_on_dismiss,
                        &mut cb,
                        &mut ch,
                    );
                }
                gone.push(old);
            }
            // Mini pets: also at most `miniPetLimit` of the owner's mini pets
            // over all mini summons, the oldest go (module docs).
            let mini = rec.class == "Skill_SpawnMiniPet";
            let mini_limit = assets.data.engine().mini_pet_limit;
            if mini && mini_limit > 0 {
                let mut minis: Vec<(Entity, f32)> = pets
                    .iter()
                    .filter(|(e, p, ..)| p.owner == ev.owner && p.mini && !gone.contains(e))
                    .map(|(e, p, ..)| (e, p.spawned_at))
                    .collect();
                minis.sort_by(|a, b| a.1.total_cmp(&b.1));
                while minis.len() >= mini_limit {
                    let (old, _) = minis.remove(0);
                    if let Ok((_, pet, mut cb, mut ch)) = pets.get_mut(old) {
                        dismiss(
                            &mut commands,
                            old,
                            pet.overkill_on_dismiss,
                            &mut cb,
                            &mut ch,
                        );
                    }
                }
            }
            let level = progress.as_ref().map_or(1, |p| p.progress.level).max(1);
            let types: Vec<String> = assets
                .data
                .db
                .get(&pet_rec)
                .map(|r| r.strs_iter("petTypeNames").map(|s| s.to_string()).collect())
                .unwrap_or_default();
            let bonuses: Vec<_> = pet_bonuses_with(&assets.data, loadout, owner_buffs)
                .into_iter()
                .filter(|(b, _)| bonus_applies(b, &types))
                .collect();
            let Some(profile) = gd_sim::actor::monster_with(
                &assets.data.db,
                &rules.0,
                &pet_rec,
                level,
                gd_sim::actor::Side::Pet,
                &bonuses,
            ) else {
                continue;
            };
            let Some(prec) = db.get(&pet_rec) else {
                continue;
            };
            // `Skill_SpawnPet::SpawnPet` → `Monster::SetLifetime` (module docs).
            let mods: Vec<(String, usize, bool)> = links
                .map(|l| {
                    l.mods_of(&ev.skill)
                        .iter()
                        .map(|(m, li)| {
                            (
                                m.clone(),
                                *li,
                                loadout
                                    .skills
                                    .skills
                                    .iter()
                                    .any(|(s, lv)| *lv > 0 && s.eq_ignore_ascii_case(m)),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let time_to_live = spawn_time_to_live(&assets.data.db, rec, ev.level_index, &mods);
            let tf = Transform::from_translation(at).with_rotation(otf.rotation);
            let Some(e) = crate::creature::spawn(
                &mut commands,
                &mut assets,
                tf,
                &pet_rec,
                gd_game_core::rng::u32(..),
            ) else {
                continue;
            };
            // Skills: record levels, modifier levels for unlocked ones.
            let mods = modifier_levels(&assets.data, loadout);
            let lvl = level as f32;
            let skill_level =
                |i: usize, name: &str| gd_sim::skills::pet_skill_level(prec, i, name, lvl, &mods);
            let mut primary = None;
            let mut auras = Vec::new();
            let mut levels = Vec::new();
            for i in 1..=40 {
                let Some(name) = prec.str(&format!("skillName{i}")) else {
                    continue;
                };
                let l = skill_level(i, name);
                levels.push((name.to_owned(), l));
                let Some(r) = assets.data.db.get(name) else {
                    continue;
                };
                if l > 0 && r.class == "Skill_BuffAttackRadiusToggled" {
                    auras.push(gd_game_core::casting::DamageAura::new(
                        r,
                        name,
                        (l - 1) as usize,
                        false,
                    ));
                }
                let k = kind_of(r);
                if l > 0
                    && primary.is_none()
                    && matches!(k, Kind::Projectile | Kind::TargetRadius | Kind::Weapon)
                {
                    primary = Some(PetAttack {
                        skill: name.to_owned(),
                        level_index: (l - 1) as usize,
                        kind: k,
                        range: if k == Kind::Weapon {
                            0.0
                        } else {
                            crate::casting::skill_range(&assets.data, r)
                        },
                        chance: 100.0,
                        band: None,
                        delay: 0.0,
                        timer: 0.0,
                        timeout: 0.0,
                    });
                }
            }
            if !auras.is_empty() {
                commands
                    .entity(e)
                    .insert(gd_game_core::casting::DamageAuras(auras));
            }
            // Its skills' autocast children (`crate::autocast`, skill-level autocasts: the Wendigo Totem's heal).
            if let Some(a) = crate::autocast::SkillAutocasts::of(&assets.data.db, &levels) {
                commands.entity(e).insert(a);
            }
            // Self buffs (`Monster::InitSkillsInController`): those among
            // `buffSelf{,2,3}SkillName` that it has, toggles and timed buffs;
            // its AI uses them ([`pet_ai`]). No stock pet has a timed one.
            let has_level = |name: &str| {
                let idx = (1..=40)
                    .find(|i| {
                        prec.str(&format!("skillName{i}"))
                            .is_some_and(|n| n.eq_ignore_ascii_case(name))
                    })
                    .unwrap_or(0);
                skill_level(idx, name)
            };
            let mut buff_self = Vec::new();
            for k in [
                "buffSelfSkillName",
                "buffSelf2SkillName",
                "buffSelf3SkillName",
            ] {
                let Some(name) = prec.str(k) else { continue };
                let l = has_level(name);
                let Some(r) = assets.data.db.get(name).filter(|_| l > 0) else {
                    continue;
                };
                // Damaging toggles are already on (above).
                if gd_sim::skills::is_toggle(&r.class) && r.class != "Skill_BuffAttackRadiusToggled"
                {
                    buff_self.push((name.to_owned(), (l - 1) as usize, true));
                } else if kind_of(r) == Kind::Buff {
                    buff_self.push((name.to_owned(), (l - 1) as usize, false));
                }
            }
            // Initial toggles go on at once (`ControllerMonsterStateStartup::
            // UseInitialSkillIfSet`, the monster's startup state, which
            // `ControllerPet` registers too); damaging ones are on above.
            let mut toggled = Vec::new();
            let mut initial_casts = Vec::new();
            for k in ["initialSkillName", "initial2SkillName"] {
                let Some(name) = prec.str(k) else { continue };
                let l = has_level(name);
                let Some(r) = assets.data.db.get(name).filter(|_| l > 0) else {
                    continue;
                };
                if gd_sim::skills::is_toggle(&r.class) {
                    if r.class != "Skill_BuffAttackRadiusToggled" {
                        toggled.push((name.to_owned(), (l - 1) as usize));
                    }
                } else if kind_of(r) != Kind::Unsupported {
                    initial_casts.push((name.to_owned(), (l - 1) as usize));
                }
            }
            // Buff other: only `buffOtherSkillName` (`monster_skills` module docs).
            let buff_other = prec.str("buffOtherSkillName").and_then(|name| {
                let l = has_level(name);
                let r = assets.data.db.get(name).filter(|_| l > 0)?;
                let li = (l - 1) as usize;
                let inner = gd_sim::skills::profile_name(r)
                    .map(str::to_owned)
                    .unwrap_or_else(|| name.to_owned());
                let secs = assets.data.db.get(&inner).map_or(0.0, |b| {
                    gd_game_core::casting::level_f(b, "skillActiveDuration", li)
                });
                (secs > 0.0).then(|| (name.to_owned(), li, inner, secs))
            });
            // `dyingSkillName` (`StartDyingSkill`, module docs).
            let dying = prec
                .str("dyingSkillName")
                .filter(|n| !n.is_empty())
                .and_then(|name| {
                    let l = has_level(name).max(1);
                    let r = assets.data.db.get(name)?;
                    let k = kind_of(r);
                    (!matches!(k, Kind::Unsupported | Kind::Beam | Kind::Toggle))
                        .then(|| (name.to_owned(), (l - 1) as usize, k))
                });
            let mut specials = Vec::new();
            for suffix in ["", "2", "3"] {
                let Some(name) = prec.str(&format!("specialAttack{suffix}SkillName")) else {
                    continue;
                };
                let idx = (1..=40)
                    .find(|i| {
                        prec.str(&format!("skillName{i}"))
                            .is_some_and(|n| n.eq_ignore_ascii_case(name))
                    })
                    .unwrap_or(0);
                let l = skill_level(idx, name);
                let Some(r) = assets.data.db.get(name) else {
                    continue;
                };
                let k = kind_of(r);
                if l == 0 || k == Kind::Unsupported {
                    continue;
                }
                specials.push(PetAttack {
                    skill: name.to_owned(),
                    level_index: (l - 1) as usize,
                    kind: k,
                    range: crate::casting::skill_range(&assets.data, r),
                    // `Monster::InitSkillsInController`: chance, delay and
                    // timeout from the pet record (0 when missing); the delay
                    // left starts at 0, the timeout left at the timeout.
                    chance: prec
                        .f32(&format!("specialAttack{suffix}Chance"))
                        .unwrap_or(0.0),
                    band: {
                        let f = |k: &str| prec.f32(k).unwrap_or(0.0);
                        match prec
                            .str(&format!("specialAttack{suffix}Range"))
                            .unwrap_or("")
                        {
                            "ShortRange" => Some((f("shortRangeMin"), f("shortRangeMax"))),
                            "MediumRange" => Some((f("mediumRangeMin"), f("mediumRangeMax"))),
                            "LongRange" => Some((f("longRangeMin"), f("longRangeMax"))),
                            _ => None,
                        }
                    },
                    // The record's delay, no floor: `ChooseBestSkill` rolls
                    // at the engine's moments (a foe found, an attack's end,
                    // every 200 ms pursuing), not every frame (`pet_ai`).
                    delay: prec
                        .f32(&format!("specialAttack{suffix}Delay"))
                        .unwrap_or(0.0),
                    timer: 0.0,
                    timeout: prec
                        .f32(&format!("specialAttack{suffix}Timeout"))
                        .unwrap_or(0.0),
                });
            }
            info!(
                "pet {} level {level}: life {:.0}, primary {:?}, specials {:?}, self buffs {:?}",
                pet_rec.rsplit('/').next().unwrap_or(""),
                profile.sheet.life,
                primary.as_ref().map(|p| &p.skill),
                specials
                    .iter()
                    .map(|s| (&s.skill, s.level_index + 1))
                    .collect::<Vec<_>>(),
                buff_self
                    .iter()
                    .map(|s| (&s.0, s.1 + 1))
                    .collect::<Vec<_>>()
            );
            let dirty = !toggled.is_empty();
            let radius = gd_game_core::combat::get_radius(Some(prec), 0.5);
            // The summon skill's stored stance (`Player::PostPetSpawn`).
            let stored = stances.map_or(Stance::Normal, |s| s.get(&ev.skill));
            let (ctl, stance) = match PetController::of_stance(&assets.data, prec, stored) {
                Some(c) => (c, stored),
                None => (
                    PetController::of_stance(&assets.data, prec, Stance::Normal)
                        .expect("Normal always resolves"),
                    Stance::Normal,
                ),
            };
            commands.entity(e).insert((
                Pet {
                    owner: ev.owner,
                    summon: ev.skill.clone(),
                    mini,
                    quest,
                    spawned_at: time.elapsed_secs(),
                    time_to_live,
                    found_enemy: false,
                    primary,
                    specials,
                    anger_multiplier: prec.f32("angerMultiplier").unwrap_or(1.0),
                    // `Character::Load` reads both with a default of 1.
                    causes_anger: prec.get("causesAnger").is_none() || prec.bool("causesAnger"),
                    targetable: prec.get("targetable").is_none() || prec.bool("targetable"),
                    portrait: prec.bool("showStatusWidgetWhenPet"),
                    player_scaling: prec.class == "PetPlayerScaling",
                    buff_self,
                    toggled,
                    buffs: gd_game_core::skill_buffs::TimedBuffs::default(),
                    buff_other,
                    ally_due: false,
                    cooldowns: Vec::new(),
                    startup: true,
                    startup_played: false,
                    initial_casts,
                    overkill_on_dismiss: prec.bool("overkillOnDismiss"),
                    gib_threshold: prec.i32("gibThreshold"),
                    held: None,
                    dying,
                    energy: profile.sheet.energy,
                    buff_poll: SELF_BUFF_POLL,
                    choose_poll: 0.0,
                    had_target: false,
                    was_busy: false,
                    joined: false,
                    dirty,
                    owner_buff_bonus: owner_buffs
                        .into_iter()
                        .flat_map(|a| &a.list)
                        .filter(|b| {
                            assets.data.db.get(&b.buff).is_some_and(|r| {
                                r.str("petBonusName").is_some_and(|p| !p.is_empty())
                            })
                        })
                        .map(|b| (b.buff.clone(), b.level_index))
                        .collect(),
                    record: pet_rec.clone(),
                    level,
                    ctl,
                    stance,
                    most_hated: None,
                    moving: None,
                    dodge_timer: 0.0,
                    dodging: false,
                    melee: crate::melee::MeleeAi::new(ctl.pursuit_ms, ctl.random_reposition),
                    walk: crate::melee::Walk::of(Some(prec)),
                    anger: Vec::new(),
                    anger_timer: 0.0,
                    follow_check: 0.0,
                    following: false,
                    slot: None,
                    path: Vec::new(),
                    repath: 0.0,
                },
                Combatant::new(Arc::new(profile), radius),
                Name::new(format!("pet {pet_rec}")),
            ));
            mine.push((e, time.elapsed_secs()));
        }
    }
}

/// Pet bonuses are read live by the game (module docs): when an owner's
/// loadout changes, its pets' profiles are rebuilt with the current bonuses,
/// keeping their life fraction; also when a self toggle goes on or an aura
/// buff (`auras.rs`) comes or goes. The profile = the record + pet bonuses +
/// the switched-on self toggles' records ([`Pet::self_stats`]) + the aura
/// buffs on it (its own radius auras' buffs come back to it as those).
pub(super) fn refresh_profiles(
    data: Res<GameData>,
    rules: Res<CombatRules>,
    owners: Query<
        (
            Entity,
            Ref<Loadout>,
            Option<&gd_game_core::skill_buffs::ReceivedBuffs>,
        ),
        Without<Pet>,
    >,
    // A corpse keeps its profile and its 0 life (a rebuild would floor the life at 1).
    mut pets: Query<
        (
            &mut Pet,
            &mut Combatant,
            Option<&mut gd_game_core::skill_buffs::ReceivedBuffs>,
            Option<&crate::challenge_areas::AreaRules>,
        ),
        Without<Dead>,
    >,
) {
    for (owner, loadout, owner_buffs) in &owners {
        // The owner's buffs that carry a pet bonus (a power-up's), their
        // coming or going rebuilds its pets' profiles like a loadout change.
        let buff_bonus: Vec<(String, usize)> = owner_buffs
            .into_iter()
            .flat_map(|a| &a.list)
            .filter(|b| {
                data.db
                    .get(&b.buff)
                    .is_some_and(|r| r.str("petBonusName").is_some_and(|p| !p.is_empty()))
            })
            .map(|b| (b.buff.clone(), b.level_index))
            .collect();
        let mut all: Option<Vec<(gd_formats::arz::Record, usize)>> = None;
        for (mut pet, mut cb, mut auras, area) in pets.iter_mut().filter(|(p, ..)| p.owner == owner)
        {
            let auras_changed = auras.as_mut().is_some_and(|a| a.take_changed());
            let buffs_changed = pet.owner_buff_bonus != buff_bonus;
            if !(loadout.is_changed()
                || std::mem::take(&mut pet.dirty)
                || auras_changed
                || buffs_changed)
            {
                continue;
            }
            pet.owner_buff_bonus.clone_from(&buff_bonus);
            let all = all.get_or_insert_with(|| pet_bonuses_with(&data, &loadout, owner_buffs));
            let types: Vec<String> = data
                .db
                .get(&pet.record)
                .map(|r| r.strs_iter("petTypeNames").map(|s| s.to_string()).collect())
                .unwrap_or_default();
            let mut extra: Vec<_> = all
                .iter()
                .filter(|(b, _)| bonus_applies(b, &types))
                .cloned()
                .collect();
            extra.extend(pet.self_stats(&data.db));
            if let Some(a) = &auras {
                extra.extend(a.stat_records(&data.db));
            }
            let Some(profile) = gd_sim::actor::monster_with(
                &data.db,
                area.map_or(&rules.0, |a| &a.0),
                &pet.record,
                pet.level,
                gd_sim::actor::Side::Pet,
                &extra,
            ) else {
                continue;
            };
            let frac = cb.life / cb.profile.sheet.life.max(1.0);
            cb.life = (profile.sheet.life * frac).max(1.0);
            cb.profile = Arc::new(profile);
        }
    }
}
