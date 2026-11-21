//! What a hit's convert and disruption do (`gd_sim::combat::Influence`; taunt is anger: `HitLanded::taunt`).
//!
//! Convert, `CombatAttributeInfluenceDamage_Convert::Execute` @0x180118d00 → `Monster::JoinMe(attacker,
//! value × 1000 ms, banner 1, 0)` @0x180375e40 when the attacker's level (vtable +1376) + 5 is at least the
//! defender's. `JoinMe` is a `Monster` method (`Pet` shares it; the player's slot is an empty function, so a player
//! is never converted). It refuses a monster whose join time (+18784) is above 0: one joined for a while stays
//! until that runs out (a minion that joined its summoner for good has -1 there and can be converted). Else it
//! saves its leader (+20168) and the new leader's level, sets the time, copies the attacker's faction pack onto
//! its own (+6640), `SetLeader(attacker)`, clears its controller's anger list and target, and puts up a convert
//! banner (`playerPetBannerName` when the copied faction is the player's, else `monsterPetBannerName`, read from
//! `GameEngine::GetDatabase` = `records/game/gameengine.dbr`; stock: that record has neither field, so the
//! name is empty and `SetConvertBanner` shows nothing; the port shows none).
//! `Monster::UpdateSelf` then, every update while it has a leader: a leader it now calls a foe (`FactionManager::
//! IsFoe`) ends the join at once; else the time counts down by the frame's milliseconds and at 0 ends it.
//! The end, `Monster::UnJoinLeader(true)` @0x18037a230: the banner goes, the time is -1 again, the faction pack
//! is the one it was loaded with (`GetOriginalFactionPack`, `Load` / `InitialUpdate`), the saved leader is its
//! leader again, and its controller's anger list and target are cleared.
//! A player's pet is a `Monster` here too (`Pet` keeps `JoinMe` / `UnJoinLeader`, `Pet::UpdateSelf`
//! runs `Monster::UpdateSelf` first): joined to its owner for good (-1), a monster's convert takes it over the
//! same way, and the end puts back its record's pack (`faction_player.dbr` on every stock pet) and its owner.
//! Port ([`join_converted`], [`unjoin_converted`], [`Converted`]): the monster's `MonsterAi::faction` takes the
//! attacker's pack (the player's or a pet's: [`crate::monsters::player_pack`]) and its `Allied` mark the
//! attacker's side, so it fights the attacker's foes and they fight it ([`FoeLists`], `Side`); it follows the
//! attacker (`PatrolState::join`) and its anger, enemy and target are cleared; the end puts back the record's pack,
//! the mark as at spawn (`monsters::is_friendly`), the saved leader, and clears them again. A converted pet gets
//! [`JoinedPack`] (its [`Side`] and a rival of the pack's foes) and [`Converted`] with its owner; the end removes
//! both. PORT: the leader-turned-foe check runs for a monster leader only (a player leader's pack is the one the
//! monster copied).
//! While joined (the engine asks `GetLeader` and the faction, which the join changed): the owner's pet
//! orders do not reach it (`Skill_PetAttack::GetValidTarget` @0x180503810 and `ControllerPlayer::HandlePetAction`
//! @0x18015fd70 take only the pets the player leads, `DefaultRequestAttackResponse` / `DefaultRequestMoveResponse`
//! refuse any other sender; a stance change, `Monster::UseController`, still applies; `pets::pet_commands`); its
//! buff-other goes to its joined pack's leaderless friends (`ChooseBestAllyToBuff`, `pets` module docs); and the
//! controller targeting reads it as any monster (`Monster::IsTargetable`, `Character::IsHiding` /
//! `CausesAnger` are its vtable slots too: its record's `targetable`, `hiddenFromCombat` and `causesAnger`;
//! grimdawn `controller_target.rs`).
//!
//! Disruption, `CombatAttributeAbsDamage_Disruption::Execute` @0x180118770 → `Character::
//! CombatAddCooldownDamage` @0x180069000 → `SkillManager::ApplyCooldownDamage` @0x18053d380: every skill in the
//! manager's four skill lists (not the default attacks, `LoadDefaultSkills` keeps them apart) gets
//! `Skill::ApplyDisruptionCooldownTime` @0x180491ad0, which for a skill disruption reaches
//! (`gd_sim::skills::disruptable`) is `Skill::ReplaceCooldownTime(ms, 0)` @0x180491a80: a cooldown of that time
//! unless the running one has more left ([`crate::casting::Cooldowns::replace`]). Port ([`disrupt_skills`]): the
//! player's learned, gear-granted and form-granted skills (`Caster::cooldowns`) and procs (`Autocasts`), a
//! monster's skills (`MonsterSkills`) and a pet's (`Pet`), each where its readiness is checked.
//! A monster's `attackSkillName` is a manager skill (`Monster::InitSkillsInController`: `FindSkillId`), so it is
//! disrupted too, and there is no fallback to the default attack: `ControllerMonster::ChooseBestSkill`
//! picks it whenever it is set, valid and in range (`CloseEnoughToUseSkill`), with no readiness check, so the
//! monster waits out the cooldown instead of swinging.
use super::*;

/// A hit's convert or disruption for its target (`gd_sim::combat::Influence`), from `apply_hits`.
#[derive(Message, Clone, Debug)]
pub struct Influenced {
    pub attacker: Entity,
    pub target: Entity,
    pub influence: gd_sim::combat::Influence,
}

/// A monster that joined another character's side for a while (`Monster::JoinMe` with a time, module docs).
#[derive(Component, Debug, Clone)]
pub struct Converted {
    /// Seconds left (`Monster` +18784, milliseconds there).
    pub left: f32,
    /// The leader it had before (`Monster` +20168), its leader again at the end.
    pub leader: Option<Entity>,
}

/// `Monster::JoinMe` for a hit's convert (module docs).
pub(super) fn join_converted(
    mut commands: Commands,
    mut events: MessageReader<Influenced>,
    data: Res<GameData>,
    factions: Res<crate::factions::PlayerFactions>,
    mut q: Query<
        (
            &mut Combatant,
            Option<&mut MonsterAi>,
            Has<Monster>,
            Has<Converted>,
            Option<&crate::pets::Pet>,
            Has<Allied>,
            Option<&JoinedPack>,
        ),
        Without<Dead>,
    >,
) {
    // The characters joined this frame: `Converted` is inserted deferred, so a second convert of one would not see
    // it and would save the first converter as the leader to go back to.
    let mut joined = std::collections::HashSet::new();
    for ev in events.read() {
        let secs = ev.influence.convert;
        if secs < 1.0 || ev.attacker == ev.target {
            continue;
        }
        let Ok((acb, aai, _, _, _, a_allied, ajoined)) = q.get(ev.attacker) else {
            continue;
        };
        let level = acb.profile.level;
        // The attacker's faction pack: a monster's own (or a converted pet's joined one), else the player's (the
        // player, or his pet, which carries the player's copy).
        let (pack, allied) = match (aai, ajoined) {
            (Some(ai), _) => (ai.faction.clone(), a_allied),
            (None, Some(j)) => (j.pack.clone(), j.allied),
            (None, None) => (
                Some(Arc::new(crate::monsters::player_pack(
                    &data.db,
                    Some(&factions),
                ))),
                true,
            ),
        };
        let Ok((mut cb, ai, monster, converted, pet, ..)) = q.get_mut(ev.target) else {
            continue;
        };
        // `JoinMe` refuses a character joined for a while (a pet joined its owner for good: -1, convertible).
        if converted || joined.contains(&ev.target) {
            debug!("convert {}: already joined", ev.target);
            continue;
        }
        if level + 5 < cb.profile.level {
            continue;
        }
        match (ai, pet) {
            (Some(mut ai), _) if monster => {
                debug!(
                    "convert {} joins {} for {secs:.1} s",
                    ev.target, ev.attacker
                );
                let leader = ai.patrol.leader;
                ai.faction = pack;
                ai.patrol.join(ev.attacker);
                ai.anger.clear();
                ai.enemy = None;
                ai.aggro = false;
                cb.target = None;
                let mut e = commands.entity(ev.target);
                if allied {
                    e.try_insert(Allied);
                } else {
                    e.try_remove::<Allied>();
                }
                e.try_insert(Converted { left: secs, leader });
                joined.insert(ev.target);
            }
            // A player's pet (`Pet` shares `Monster::JoinMe`): a monster's convert joins it to that monster's
            // pack and makes the monster its leader (the player's side never hits its own pets).
            (None, Some(p)) if !allied => {
                debug!(
                    "convert pet {} joins {} for {secs:.1} s",
                    ev.target, ev.attacker
                );
                cb.target = None;
                commands.entity(ev.target).try_insert((
                    Converted {
                        left: secs,
                        leader: Some(p.owner),
                    },
                    JoinedPack {
                        pack,
                        allied,
                        leader: ev.attacker,
                    },
                ));
                joined.insert(ev.target);
            }
            _ => {}
        }
    }
}

/// `Monster::UpdateSelf`'s join countdown and `UnJoinLeader(true)` (module docs).
pub(super) fn unjoin_converted(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    factions: Res<crate::factions::PlayerFactions>,
    mut q: Query<
        (
            Entity,
            &mut Converted,
            &mut MonsterAi,
            &mut Combatant,
            &Monster,
        ),
        Without<Dead>,
    >,
    leaders: Query<&MonsterAi, Without<Converted>>,
    mut pets: Query<
        (Entity, &mut Converted, &JoinedPack, &mut Combatant),
        (Without<Dead>, Without<MonsterAi>),
    >,
) {
    let dt = time.delta_secs();
    for (e, mut conv, mut ai, mut cb, monster) in &mut q {
        let foe = ai
            .patrol
            .leader
            .and_then(|l| leaders.get(l).ok())
            .is_some_and(|l| crate::monsters::FactionPack::foes(&ai.faction, &l.faction));
        conv.left -= dt;
        if !foe && conv.left > 0.0 {
            continue;
        }
        debug!("convert {e}: back to its own side");
        ai.faction = crate::monsters::FactionPack::shared(&data, &monster.record);
        ai.patrol.leader = conv.leader;
        ai.anger.clear();
        ai.enemy = None;
        ai.aggro = false;
        cb.target = None;
        let mut ec = commands.entity(e);
        if crate::monsters::is_friendly(&data.db, Some(&factions), &monster.record) {
            ec.try_insert(Allied);
        } else {
            ec.try_remove::<Allied>();
        }
        ec.try_remove::<Converted>();
    }
    // A converted pet: the same countdown and leader check; `UnJoinLeader(true)` puts back its record's pack (every
    // stock pet record's `factions` is `faction_player.dbr`: the player's side again) and its owner as leader.
    for (e, mut conv, joined, mut cb) in &mut pets {
        let foe = leaders
            .get(joined.leader)
            .is_ok_and(|l| crate::monsters::FactionPack::foes(&joined.pack, &l.faction));
        conv.left -= dt;
        if !foe && conv.left > 0.0 {
            continue;
        }
        debug!("convert pet {e}: back to its owner");
        cb.target = None;
        commands.entity(e).try_remove::<(Converted, JoinedPack)>();
    }
}

/// `SkillManager::ApplyCooldownDamage` for a hit's disruption (module docs).
pub(super) fn disrupt_skills(
    mut events: MessageReader<Influenced>,
    data: Res<GameData>,
    mut players: Query<(
        &mut crate::casting::Caster,
        &crate::player::Loadout,
        Option<&mut crate::autocast::Autocasts>,
    )>,
    mut monsters: Query<&mut crate::monster_skills::MonsterSkills>,
    mut pets: Query<&mut crate::pets::Pet>,
) {
    for ev in events.read() {
        let secs = ev.influence.disruption;
        if secs <= 0.0 {
            continue;
        }
        debug!("disruption on {}: {secs:.1} s", ev.target);
        if let Ok((mut caster, loadout, autocasts)) = players.get_mut(ev.target) {
            let mut names: Vec<String> = loadout
                .skills
                .skills
                .iter()
                .filter(|s| s.1 > 0)
                .map(|s| s.0.clone())
                .collect();
            names.extend(
                gd_sim::skills::item_skill_grants(&data.db, &loadout.items)
                    .into_iter()
                    .map(|g| g.skill),
            );
            names.extend(loadout.granted.iter().map(|g| g.0.clone()));
            // The default attacks (the book lists the kick) are not in the manager's skill lists.
            names.retain(|n| {
                !n.to_ascii_lowercase()
                    .starts_with("records/skills/default/")
            });
            names.sort();
            names.dedup();
            for n in names {
                if data.db.get(&n).is_some_and(gd_sim::skills::disruptable) {
                    caster.cooldowns.replace(&n, secs);
                }
            }
            if let Some(mut a) = autocasts {
                a.disrupt(&data.db, secs);
            }
        } else if let Ok(mut ms) = monsters.get_mut(ev.target) {
            ms.disrupt(&data.db, secs);
        } else if let Ok(mut pet) = pets.get_mut(ev.target) {
            pet.disrupt(&data.db, secs);
        }
    }
}
