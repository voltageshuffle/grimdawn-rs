//! What a hit's convert and disruption do (taunt is anger: `HitLanded::taunt`).
//!
//! Convert, `CombatAttributeInfluenceDamage_Convert::Execute` → `Monster::JoinMe(attacker, value × 1000 ms)` when
//! the attacker's level + 5 is at least the defender's. The player's `JoinMe` is empty: a player is never
//! converted. It refuses a monster whose join time is above 0 (a minion joined to its summoner for good has -1 and
//! can be converted). Else it saves its leader, sets the time, copies the attacker's faction pack,
//! `SetLeader(attacker)`, clears its anger list and target, and puts up a convert banner (stock
//! `gameengine.dbr` names none, so nothing shows).
//! `Monster::UpdateSelf`, every update while it has a leader: a leader it now calls a foe ends the join at once;
//! else the time counts down and at 0 ends it.
//! `Monster::UnJoinLeader(true)`: the banner goes, the time is -1 again, the faction pack is the loaded one, the
//! saved leader is its leader again, and the anger list and target are cleared.
//! A player's pet is a `Monster` here too: joined to its owner for good (-1), a monster's convert takes it over the
//! same way, and the end puts back its record's pack and its owner.
//! Port ([`join_converted`], [`unjoin_converted`]): the monster's `MonsterAi::faction` takes the attacker's pack
//! and its `Allied` mark the attacker's side ([`FoeLists`], `Side`); it follows the attacker and its anger, enemy
//! and target are cleared; the end puts back the record's pack, the spawn mark and the saved leader. A converted
//! pet gets [`JoinedPack`] and [`Converted`]; the end removes both. PORT: the leader-turned-foe check runs for a
//! monster leader only.
//! While joined: the owner's pet orders do not reach it (a stance change still applies); its buff-other goes to
//! its joined pack's leaderless friends (`ChooseBestAllyToBuff`); controller targeting reads it as any monster
//! (its record's `targetable`, `hiddenFromCombat` and `causesAnger`).
//!
//! Disruption, `CombatAttributeAbsDamage_Disruption::Execute` → `SkillManager::ApplyCooldownDamage`: every skill in
//! the manager's four skill lists (not the default attacks) that disruption reaches (`gd_sim::skills::disruptable`)
//! gets `Skill::ReplaceCooldownTime(ms, 0)`: a cooldown of that time unless the running one has more left. Port
//! ([`disrupt_skills`]): the player's learned, gear- and form-granted skills and procs, a monster's and a pet's
//! skills. A monster's `attackSkillName` is a manager skill, so it is disrupted too, and
//! `ControllerMonster::ChooseBestSkill` picks it with no readiness check: the monster waits out the cooldown
//! instead of swinging.
use super::*;

/// From `apply_hits`.
#[derive(Message, Clone, Debug)]
pub struct Influenced {
    pub attacker: Entity,
    pub target: Entity,
    pub influence: gd_sim::combat::Influence,
}

/// A monster that joined another character's side for a while.
#[derive(Component, Debug, Clone)]
pub struct Converted {
    /// Seconds left (`Monster.joinTime`).
    pub left: f32,
    /// `Monster.previousLeader`, its leader again at the end.
    pub leader: Option<Entity>,
}

/// `Monster::JoinMe` for a hit's convert.
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
    // `Converted` is inserted deferred: a second convert this frame must not save the first converter as leader.
    let mut joined = gd_formats::collections::HashSet::default();
    for ev in events.read() {
        let secs = ev.influence.convert;
        if secs < 1.0 || ev.attacker == ev.target {
            continue;
        }
        let Ok((acb, aai, _, _, _, a_allied, ajoined)) = q.get(ev.attacker) else {
            continue;
        };
        let level = acb.profile.level;
        // The attacker's faction pack: a monster's own (or a converted pet's joined one), else the player's.
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
            // A player's pet: a monster's convert joins it to that monster's pack and makes the monster its leader.
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

/// `Monster::UpdateSelf`'s join countdown and `UnJoinLeader(true)`.
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
            .is_some_and(|l| gd_game_core::monsters::FactionPack::foes(&ai.faction, &l.faction));
        conv.left -= dt;
        if !foe && conv.left > 0.0 {
            continue;
        }
        debug!("convert {e}: back to its own side");
        ai.faction = gd_game_core::monsters::FactionPack::shared(&data, &monster.record);
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
    // A converted pet: the same countdown and leader check; the end puts back its record's pack and its owner.
    for (e, mut conv, joined, mut cb) in &mut pets {
        let foe = leaders
            .get(joined.leader)
            .is_ok_and(|l| gd_game_core::monsters::FactionPack::foes(&joined.pack, &l.faction));
        conv.left -= dt;
        if !foe && conv.left > 0.0 {
            continue;
        }
        debug!("convert pet {e}: back to its owner");
        cb.target = None;
        commands.entity(e).try_remove::<(Converted, JoinedPack)>();
    }
}

/// `SkillManager::ApplyCooldownDamage` for a hit's disruption.
pub(super) fn disrupt_skills(
    mut events: MessageReader<Influenced>,
    data: Res<GameData>,
    mut players: Query<(
        &mut gd_game_core::casting::Caster,
        &gd_game_core::player::Loadout,
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
