use super::*;
use std::cell::RefCell;

/// Pet orders and stance changes: the Pet Attack skill's hotbar casts, [`PetCommand`] and
/// [`SetPetStance`].
#[allow(clippy::too_many_arguments)]
pub(super) fn pet_commands(
    mut commands: Commands,
    scratch: Local<RefCell<gd_sim::nav::PathScratch>>,
    data: Res<GameData>,
    nav: Res<Nav>,
    mut slot_q: crate::melee::SlotQuery,
    mut casts: MessageReader<gd_game_core::casting::CastRequest>,
    mut orders: MessageReader<PetCommand>,
    mut stance_requests: MessageReader<SetPetStance>,
    casters: Query<(Entity, &gd_game_core::casting::Caster), (With<Player>, Without<Dead>)>,
    mut stances: Query<&mut PetStances>,
    leaders: Query<&Transform, Without<Pet>>,
    // An order's foe: `FactionManager::IsFoe` of the player's side (`ControllerPlayer::HandlePetAction`),
    // so not a guard or an allied monster.
    monsters: Query<
        (&Transform, crate::combat::SideData),
        (
            Or<(With<MonsterAi>, With<Pet>)>,
            Without<Dead>,
            Without<Player>,
        ),
    >,
    mut pets: Query<
        (
            Entity,
            &Transform,
            &mut Pet,
            &mut Combatant,
            Option<&crate::combat::JoinedPack>,
        ),
        (Without<Dead>, Without<crate::movement::Stunned>),
    >,
) {
    let slots = crate::melee::Slots::new(&mut slot_q, &scratch);
    let mut list: Vec<PetCommand> = Vec::new();
    // `Skill_PetAttack::GetValidTarget`: the cursor's monster, else its ground point.
    for req in casts.read() {
        let Ok((owner, caster)) = casters.single() else {
            continue;
        };
        let Some(skill) = caster.slots.get(req.slot).cloned().flatten() else {
            continue;
        };
        if data
            .db
            .get(&skill)
            .is_some_and(|r| r.class == PET_ATTACK_CLASS)
        {
            list.push(PetCommand {
                owner,
                pet: None,
                target: req.target,
                point: req.point,
            });
        }
    }
    list.extend(orders.read().copied());
    for cmd in list {
        let foe = cmd.target.filter(|t| {
            monsters
                .get(*t)
                .is_ok_and(|(_, sd)| crate::combat::Side::Player.foe(sd.side_ref()))
        });
        let Ok(leader) = leaders.get(cmd.owner).map(|t| t.translation) else {
            continue;
        };
        for (e, tf, mut pet, mut cb, joined) in &mut pets {
            if pet.owner != cmd.owner || cmd.pet.is_some_and(|p| p != e) {
                continue;
            }
            // Only a pet the player leads takes his orders (`Monster::GetLeader`; a convert's
            // `combat::JoinedPack` takes over): the request responses refuse any other sender.
            if joined.is_some() {
                debug!("pet {e}: order ignored while a convert holds it");
                continue;
            }
            if pet.startup {
                debug!("pet {e}: order ignored in the Startup state");
                continue;
            }
            // `HandlePetAction`: clicking the selected pet itself is no order.
            if cmd.pet.is_some() && cmd.target == Some(e) {
                continue;
            }
            // The skill attacks whatever it targets (the pet refuses a non-foe); the selected pet
            // attacks a foe, else moves.
            let attack = if cmd.pet.is_none() {
                cmd.target.map(|_| foe)
            } else {
                foe.map(Some)
            };
            match attack {
                Some(Some(t)) => request_attack(
                    &mut pet,
                    &mut cb,
                    (e, tf.translation),
                    t,
                    monsters.get(t).ok().map(|m| m.0.translation),
                    leader,
                    &slots,
                ),
                Some(None) => {}
                None => request_move(
                    &mut pet,
                    &mut cb,
                    e,
                    tf.translation,
                    cmd.point,
                    &nav,
                    &scratch,
                    &slots,
                ),
            }
        }
    }
    for req in stance_requests.read() {
        let Ok((e, _, mut pet, mut cb, _)) = pets.get_mut(req.pet) else {
            continue;
        };
        // `Player::SetPetControllerType`, then `Monster::UseController` (no leader test).
        match stances.get_mut(pet.owner) {
            Ok(mut s) => s.set(&pet.summon, req.stance),
            Err(_) => {
                let mut s = PetStances::default();
                s.set(&pet.summon, req.stance);
                commands.entity(pet.owner).try_insert(s);
            }
        }
        if apply_stance(&data, &mut pet, e, req.stance, &slots) {
            cb.target = None;
            debug!("pet {e}: stance {:?}", req.stance);
        }
    }
}

/// `DefaultRequestAttackResponse`: `foe` is at `at` (None: gone); `leader` is the pursuit range's home.
pub(super) fn request_attack(
    pet: &mut Pet,
    cb: &mut Combatant,
    (e, me): (Entity, Vec3),
    foe: Entity,
    at: Option<Vec3>,
    leader: Vec3,
    slots: &crate::melee::Slots,
) {
    // "Move" ignores it (its slot 17 is empty).
    if pet.moving.is_some() || pet.dodging {
        debug!("pet {e}: attack order on {foe} ignored while moving");
        return;
    }
    // `IsEnemyValid`: a live foe within the pursuit range.
    let Some(at) = at else { return };
    if !pet.ctl.in_pursuit(leader, at, me) || cb.target == Some(foe) {
        debug!(
            "pet {e}: attack order on {foe} refused (out of pursuit range or already its target)"
        );
        return;
    }
    debug!("pet {e}: attack order on {foe}");
    pet.most_hated = Some(foe);
    if !pet.anger.iter().any(|a| a.0 == foe) {
        pet.anger.push((foe, 0.0));
    }
    // "Pursue" from the start.
    pet.melee.leave(slots, e);
    pet.path.clear();
    pet.following = false;
    cb.target = Some(foe);
}

/// `DefaultRequestMoveResponse`: forget the fight and run to `point`; unreachable leaves it idle.
#[allow(clippy::too_many_arguments)]
pub(super) fn request_move(
    pet: &mut Pet,
    cb: &mut Combatant,
    e: Entity,
    from: Vec3,
    point: Vec3,
    nav: &Nav,
    scratch: &RefCell<gd_sim::nav::PathScratch>,
    slots: &crate::melee::Slots,
) {
    pet.anger.clear();
    pet.most_hated = None;
    cb.target = None;
    pet.melee.leave(slots, e);
    pet.following = false;
    // A stationary controller's "Move" is the stub that goes "Idle".
    pet.path = if pet.ctl.class.can_move() {
        nav.0
            .find_path(
                &mut scratch.borrow_mut(),
                from.to_array(),
                point.to_array(),
                crate::nav::PATH_BUDGET,
            )
            .map(|p| p.into_iter().skip(1).map(Vec3::from).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    pet.moving = (!pet.path.is_empty()).then_some(point);
    debug!(
        "pet {e}: move order to {point} ({} waypoints)",
        pet.path.len()
    );
}

/// `Monster::UseController`: the pet runs on the stance's controller, its AI started afresh; false
/// when the record names no such controller.
pub(super) fn apply_stance(
    data: &GameData,
    pet: &mut Pet,
    e: Entity,
    stance: Stance,
    slots: &crate::melee::Slots,
) -> bool {
    let Some(ctl) = data
        .db
        .get(&pet.record)
        .and_then(|r| PetController::of_stance(data, r, stance))
    else {
        return false;
    };
    pet.melee.leave(slots, e);
    pet.ctl = ctl;
    pet.stance = stance;
    pet.melee = crate::melee::MeleeAi::new(ctl.pursuit_ms, ctl.random_reposition);
    pet.anger.clear();
    pet.most_hated = None;
    pet.moving = None;
    pet.dodging = false;
    pet.path.clear();
    pet.following = false;
    true
}

/// `ControllerCombat::KillMe` on a pet: it dies through the one death path (`combat::kill_me`) and its
/// dying skill goes off. Life, target and death state are written at once so the systems before the
/// frame's commands do not act for a pet that is going.
pub(super) fn kill_me(commands: &mut Commands, e: Entity, cb: &mut Combatant, ch: &mut Character) {
    cb.life = 0.0;
    cb.target = None;
    ch.state = CharState::Death;
    crate::combat::kill_me(commands, e);
}

/// `KillMe(overkillOnDismiss)`: the kill of an expiry, `StopSkill` and `KillExtraPets`.
pub(super) fn dismiss(
    commands: &mut Commands,
    e: Entity,
    overkill: bool,
    cb: &mut Combatant,
    ch: &mut Character,
) {
    if overkill {
        commands.entity(e).try_insert(crate::combat::Overkill);
    }
    kill_me(commands, e, cb, ch);
}

/// `Monster::UnJoinLeader(false)` on a quest pet (`PetPen::ReleaseEntry` mode 2): it stays alive
/// with the owner's faction; it loses banner, leader, anger and target, and leaves the pen.
/// PORT: the leaderless pet stands still ([`ReleasedQuestPet`]).
pub(super) fn release_quest_pet(commands: &mut Commands, e: Entity, cb: &mut Combatant) {
    cb.target = None;
    commands
        .entity(e)
        .try_remove::<Pet>()
        .try_insert(ReleasedQuestPet);
}

/// `Skill_SpawnPet::StopSkill`: a [`StopSummonSkill`] or the owner's death kills the owner's live pets
/// of that skill. A quest pet is killed only on the owner's death; with the owner alive:
/// [`release_quest_pet`].
pub(super) fn stop_summon_skills(
    mut commands: Commands,
    mut stops: MessageReader<StopSummonSkill>,
    died: Query<Entity, Added<Dead>>,
    mut pets: Query<(Entity, &Pet, &mut Combatant, &mut Character), Without<Dead>>,
) {
    let stops: Vec<_> = stops.read().cloned().collect();
    let died: Vec<Entity> = died.iter().collect();
    for (e, pet, mut cb, mut ch) in &mut pets {
        let owner_died = died.contains(&pet.owner);
        let stopped = stops
            .iter()
            .any(|s| s.owner == pet.owner && s.skill.eq_ignore_ascii_case(&pet.summon));
        if stopped && pet.quest && !owner_died {
            debug!("pet {e}: its quest skill stopped, released alive");
            release_quest_pet(&mut commands, e, &mut cb);
        } else if owner_died || stopped {
            debug!("pet {e}: its summon skill stopped");
            dismiss(&mut commands, e, pet.overkill_on_dismiss, &mut cb, &mut ch);
        }
    }
}

/// [`Pet::time_to_live`] counts down with game time (`ControllerMonster::Update`); at 0 the pet dies.
pub(super) fn expire_pets(
    mut commands: Commands,
    time: Res<Time>,
    mut pets: Query<(Entity, &mut Pet, &mut Combatant, &mut Character), Without<Dead>>,
) {
    let dt = time.delta_secs();
    for (e, mut pet, mut cb, mut ch) in &mut pets {
        let Some(left) = pet.time_to_live.as_mut() else {
            continue;
        };
        *left -= dt;
        if *left <= 0.0 {
            debug!("pet {e}: its time to live ran out");
            dismiss(&mut commands, e, pet.overkill_on_dismiss, &mut cb, &mut ch);
        }
    }
}

/// [`DisbandPet`] (flag false) and [`ReleasePet`]: `ReleasePetConfigCmd::Execute`.
pub(super) fn disband_pets(
    mut commands: Commands,
    mut disband: MessageReader<DisbandPet>,
    mut release: MessageReader<ReleasePet>,
    mut pets: Query<(&Pet, &mut Combatant, &mut Character), Without<Dead>>,
) {
    let reqs: Vec<ReleasePet> = disband
        .read()
        .map(|d| ReleasePet {
            pet: d.pet,
            suppress_dying_skill: false,
        })
        .chain(release.read().copied())
        .collect();
    for req in reqs {
        let Ok((pet, mut cb, mut ch)) = pets.get_mut(req.pet) else {
            continue;
        };
        let overkill = pet
            .gib_threshold
            .is_some_and(|t| cb.life.max(0.0) >= cb.profile.sheet.life * t as f32 * 0.01);
        if overkill {
            commands.entity(req.pet).try_insert(crate::combat::Overkill);
        }
        if req.suppress_dying_skill {
            commands.entity(req.pet).try_insert(SuppressDyingSkill);
        }
        kill_me(&mut commands, req.pet, &mut cb, &mut ch);
        debug!("pet {}: released (overkill {overkill})", req.pet);
    }
}

/// The skill bonuses that came off the player's worn gear in one equipment change, sent by
/// `crate::inventory::sync_equipment`: names lower-case, each with a level other than 0.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct AugmentsRemoved {
    pub owner: Entity,
    pub skills: Vec<String>,
    pub masteries: Vec<String>,
    pub all: bool,
}

impl AugmentsRemoved {
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty() && self.masteries.is_empty() && !self.all
    }
}

/// The records of a worn item that hold skill bonuses, each its own container (same list as
/// `gd_sim::skills::item_skill_bonuses`).
fn bonus_records(it: &gd_formats::save::item::Item) -> [&String; 8] {
    [
        &it.base_name,
        &it.prefix_name,
        &it.suffix_name,
        &it.modifier_name,
        &it.relic_name,
        &it.relic_bonus,
        &it.augment_name,
        &it.ascendant_name,
    ]
}

/// What came off when the worn items went from `old` to `new`: every bonus record of an item no
/// longer worn or replaced (same record and seed = same item), at level index 0; and each set whose
/// piece count changed, at every count it leaves (`SkillAugmentInstall` / `SkillAugmentRemove`).
pub fn removed_augments(
    db: &gd_formats::vfs::Database,
    owner: Entity,
    old: &[gd_formats::save::item::Item],
    new: &[gd_formats::save::item::Item],
) -> AugmentsRemoved {
    let mut out = AugmentsRemoved {
        owner,
        skills: Vec::new(),
        masteries: Vec::new(),
        all: false,
    };
    let add = |out: &mut AugmentsRemoved, rec: &gd_formats::arz::Record, li: usize| {
        let on = |f: &str| gd_sim::stats::level_value(rec, f, li).is_some_and(|v| v != 0.0);
        for n in 1..=5 {
            if let Some(s) = rec
                .str(&format!("augmentSkillName{n}"))
                .filter(|s| !s.is_empty())
                && on(&format!("augmentSkillLevel{n}"))
            {
                out.skills.push(s.to_ascii_lowercase());
            }
            if let Some(m) = rec
                .str(&format!("augmentMasteryName{n}"))
                .filter(|s| !s.is_empty())
                && on(&format!("augmentMasteryLevel{n}"))
            {
                out.masteries.push(m.to_ascii_lowercase());
            }
        }
        out.all |= on("augmentAllLevel");
    };
    let mut kept: Vec<&gd_formats::save::item::Item> = new.iter().collect();
    for o in old {
        let same = kept
            .iter()
            .position(|n| n.seed == o.seed && n.base_name.eq_ignore_ascii_case(&o.base_name))
            .map(|i| kept.swap_remove(i));
        for (k, rec) in bonus_records(o).into_iter().enumerate() {
            if rec.is_empty() || same.is_some_and(|n| bonus_records(n)[k].eq_ignore_ascii_case(rec))
            {
                continue;
            }
            if let Some(r) = db.get(rec) {
                add(&mut out, r, 0);
            }
        }
    }
    let after = gd_sim::items::sets::worn_sets(db, new);
    for (set, was) in gd_sim::items::sets::worn_sets(db, old) {
        let now = after.iter().find(|(s, _)| *s == set).map_or(0, |x| x.1);
        let Some(rec) = db.get(&set) else {
            continue;
        };
        let left = if now < was {
            now + 1..was + 1
        } else {
            was..now
        };
        for count in left.filter(|&c| c > 0) {
            add(&mut out, rec, count as usize - 1);
        }
    }
    out
}

/// Whether the removed bonuses `r` release the pet summoned by `summon`: `book` the owner's learned
/// skills, `links` its modifiers / secondaries by base skill.
pub fn released_by_augments(
    trees: &gd_sim::skills::SkillTrees,
    book: &gd_sim::skills::SkillBook,
    links: Option<&gd_game_core::weapon_pool::SkillLinks>,
    r: &AugmentsRemoved,
    summon: &str,
) -> bool {
    let has = |s: &str| book.skills.iter().any(|(n, _)| n.eq_ignore_ascii_case(s));
    let summon = summon.to_ascii_lowercase();
    // `Skill::ReleasePets`: the skill's own pets and its base skills' (`Skill.baseSkills`).
    let hangs_on_summon = |s: &str| {
        links.is_some_and(|l| {
            [&l.mods, &l.secondaries].iter().any(|m| {
                m.get(&summon)
                    .is_some_and(|v| v.iter().any(|(c, _)| c.eq_ignore_ascii_case(s)))
            })
        })
    };
    if r.skills
        .iter()
        .any(|s| has(s) && (*s == summon || hangs_on_summon(s)))
    {
        return true;
    }
    // `PetPen::ReleaseMasteryPets`.
    if r.masteries.iter().any(|m| has(m))
        && let Some(m) = trees
            .trees
            .iter()
            .find(|t| t.contains(&summon))
            .and_then(|t| t.mastery())
        && r.masteries
            .iter()
            .any(|x| has(x) && x.eq_ignore_ascii_case(m))
    {
        return true;
    }
    // `PetPen::ReleaseLearnedSkillPets`: every pet whose skill is the owner's and no item skill.
    r.all && has(&summon)
}

/// [`AugmentsRemoved`]: each matching pet of the owner's ordinary pen is released with the suppress
/// flag.
pub(super) fn release_augment_pets(
    data: Res<GameData>,
    mut removed: MessageReader<AugmentsRemoved>,
    owners: Query<(&Loadout, Option<&gd_game_core::weapon_pool::SkillLinks>)>,
    pets: Query<(Entity, &Pet), Without<Dead>>,
    mut release: MessageWriter<ReleasePet>,
) {
    let trees = data.skill_trees();
    for r in removed.read() {
        let Ok((loadout, links)) = owners.get(r.owner) else {
            continue;
        };
        for (e, pet) in &pets {
            if pet.owner == r.owner
                && !pet.quest
                && released_by_augments(&trees, &loadout.skills, links, r, &pet.summon)
            {
                debug!("pet {e}: its skill's bonus came off ({})", pet.summon);
                release.write(ReleasePet {
                    pet: e,
                    suppress_dying_skill: true,
                });
            }
        }
    }
}

/// A pet's dying skill: `ControllerMonsterStateDying::OnBegin` → `StartDyingSkill` casts
/// `dyingSkillName` where it died unless `Character::ShouldSuppressDyingSkill`.
pub(super) fn pet_dying_skills(
    mut procs: MessageWriter<gd_game_core::casting::ProcCast>,
    q: Query<(Entity, &Transform, &Pet), (Added<Dead>, Without<SuppressDyingSkill>)>,
) {
    for (e, tf, pet) in &q {
        if let Some((skill, li, kind)) = &pet.dying {
            debug!("pet {e} dying skill {skill}");
            procs.write(gd_game_core::casting::ProcCast {
                caster: e,
                cast: Casting {
                    skill: skill.clone(),
                    level_index: *li,
                    kind: *kind,
                    point: tf.translation,
                    target: None,
                    hostile: false,
                    parent: None,
                },
            });
        }
    }
}

pub use gd_sim::skills::spawn_time_to_live;

impl Pet {
    /// `Character::GetPathTarget`: its path's end.
    pub fn path_target(&self) -> Option<Vec3> {
        self.path.last().copied()
    }

    pub fn record(&self) -> &str {
        &self.record
    }

    pub fn stance(&self) -> Stance {
        self.stance
    }

    /// The commanded enemy.
    pub fn most_hated(&self) -> Option<Entity> {
        self.most_hated
    }

    /// The toggled skills its AI switched on (skill, level index).
    pub fn toggled(&self) -> &[(String, usize)] {
        &self.toggled
    }

    /// The timed buffs on it (record, level index, seconds left).
    pub fn buffs(&self) -> &[(String, usize, f32)] {
        &self.buffs.list
    }

    pub(crate) fn buffs_mut(&mut self) -> &mut gd_game_core::skill_buffs::TimedBuffs {
        &mut self.buffs
    }

    pub fn add_buff(&mut self, skill: &str, li: usize, secs: f32) {
        self.buffs.add(skill, li, secs);
        self.dirty = true;
    }

    /// `Skill.active`: a toggle switched on or a timed buff running.
    pub(crate) fn skill_running(&self, skill: &str) -> bool {
        self.toggled
            .iter()
            .map(|t| &t.0)
            .chain(self.buffs.list.iter().map(|b| &b.0))
            .any(|s| s.eq_ignore_ascii_case(skill))
    }

    /// Dispelled (`DispellSelfBuff` → `StopSkill`): timed buffs end and toggles switch off, except
    /// `keep` names (`notDispelable`); a toggle comes back only when its AI uses it again. Returns the
    /// ended ones.
    pub fn dispel(&mut self, keep: impl Fn(&str) -> bool) -> Vec<String> {
        let gone: Vec<String> = self
            .buffs
            .list
            .iter()
            .map(|b| &b.0)
            .chain(self.toggled.iter().map(|t| &t.0))
            .filter(|s| !keep(s))
            .cloned()
            .collect();
        if !gone.is_empty() {
            self.buffs.list.retain(|b| keep(&b.0));
            self.toggled.retain(|t| keep(&t.0));
            self.dirty = true;
        }
        gone
    }

    /// Its auras (it is its own target): switched-on radius toggles and running `Skill_BuffRadius` buffs.
    pub fn emissions(&self, db: &gd_formats::vfs::Database) -> Vec<crate::auras::Emission> {
        let toggles = self.toggled.iter().filter(|(s, _)| {
            db.get(s)
                .is_some_and(|r| r.class == "Skill_BuffRadiusToggled")
        });
        let timed = self
            .buffs
            .list
            .iter()
            .filter(|b| db.get(&b.0).is_some_and(|r| r.class == "Skill_BuffRadius"))
            .map(|b| (&b.0, &b.1));
        toggles
            .map(|(s, li)| (s, li))
            .chain(timed)
            .filter_map(|(s, li)| crate::auras::emission(db, s, *li, Vec::new(), true))
            .collect()
    }

    /// The stats of its toggles and timed buffs other than radius buffs (those reach it as aura
    /// buffs): each record and its `buffSkillName` record.
    pub(super) fn self_stats(
        &self,
        db: &gd_formats::vfs::Database,
    ) -> Vec<(gd_formats::arz::Record, usize)> {
        let mut out = Vec::new();
        for (s, li) in self
            .toggled
            .iter()
            .map(|(s, li)| (s, *li))
            .chain(self.buffs.list.iter().map(|b| (&b.0, b.1)))
        {
            let Some(r) = db.get_owned(s) else { continue };
            if matches!(
                r.class.as_str(),
                "Skill_BuffRadiusToggled" | "Skill_BuffAttackRadiusToggled" | "Skill_BuffRadius"
            ) {
                continue;
            }
            if let Some(b) = gd_sim::skills::profile_name(&r).and_then(|b| db.get_owned(b)) {
                out.push((b, li));
            }
            out.push((r, li));
        }
        out
    }

    /// Every buff skill on it (`SkillManager::GetBuffSkill` list): its own and the aura buffs received.
    pub fn buff_stats(
        &self,
        db: &gd_formats::vfs::Database,
        received: Option<&gd_game_core::skill_buffs::ReceivedBuffs>,
    ) -> Vec<(gd_formats::arz::Record, usize)> {
        let mut out = self.self_stats(db);
        if let Some(a) = received {
            out.extend(a.stat_records(db));
        }
        out
    }

    /// `ChooseBestBuffSelfSkill`: the first self buff not on yet: (skill, level index, a toggle).
    pub(super) fn next_self_buff(&self) -> Option<(String, usize, bool)> {
        self.buff_self
            .iter()
            .filter(|(s, ..)| self.ready(s))
            .find(|(s, _, toggle)| {
                if *toggle {
                    !self.toggled.iter().any(|t| &t.0 == s)
                } else {
                    !self.buffs.list.iter().any(|b| &b.0 == s)
                }
            })
            .cloned()
    }

    pub(super) fn ready(&self, skill: &str) -> bool {
        !self.cooldowns.iter().any(|c| c.0 == skill)
    }

    pub(super) fn used(&mut self, data: &GameData, cb: &Combatant, skill: &str, li: usize) {
        let cd = data.db.get(skill).map_or(0.0, |r| {
            crate::casting::cooldown_for(
                r,
                li,
                &[],
                cb.profile.skill_attribute("skillCooldownReduction"),
            )
        });
        if cd > 0.0 {
            self.cooldowns.push((skill.to_owned(), cd));
        }
    }

    /// Disruption (`SkillManager::ApplyCooldownDamage`): its skills that disruption reaches cool down
    /// for `secs` unless they already do for longer.
    pub(crate) fn disrupt(&mut self, db: &gd_formats::vfs::Database, secs: f32) {
        let mut names: Vec<String> = self.specials.iter().map(|s| s.skill.clone()).collect();
        names.extend(self.primary.iter().map(|a| a.skill.clone()));
        names.extend(self.buff_self.iter().map(|b| b.0.clone()));
        names.extend(self.buff_other.iter().map(|b| b.0.clone()));
        names.extend(self.initial_casts.iter().map(|b| b.0.clone()));
        names.sort();
        names.dedup();
        for n in names {
            if db.get(&n).is_some_and(gd_sim::skills::disruptable) {
                gd_game_core::casting::replace_cooldown(&mut self.cooldowns, &n, secs);
            }
        }
    }
}
