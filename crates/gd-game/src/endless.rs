//! The Shattered Realm's running dungeon (host side of `gd_story::endless`).
//! [`sync_endless`] gives the generator the world map and keeps the player's map region.
//!
//! `EndlessDungeon_Generator::Update`: while the main player stands in the active floor
//! the generator starts the bonus timer at the section's first entry (latch `EndlessDungeon_Generator.sectionEntered`,
//! cleared by `Clear` at each generation) if no timer ever ran (`EndlessDungeon_Generator.timerRan`) or the last boss
//! beat the clock (`EndlessDungeon_Generator.bonusComplete`); starting it sets `timerRan` and clears `bonusComplete`
//! (one word write). Both flags live as
//! long as the generator, i.e. the session (`Clear` and `ResetEndlessDungeon` keep them): once a boss is
//! killed after the clock ran out, no later section of the session is timed, so no bonus / `bonusEarned`
//! chests either. The reward room reads `IsEndlessDungeonBonusComplete` (`rewards.lua` `dispenseReward`: the
//! two bonus chests of `bonusTable[tier]`, or `checkpointBonusTable` when the run is cashed out at its
//! checkpoint tier) after the boss set it, before the next section clears it. The generator also activates each of the
//! floor's `ProxyEndless` whose level is loaded (the dormant proxy, [`crate::monsters::EndlessProxy`], spawns
//! its held monsters); a floor ends when all its proxies ran and their monsters are dead — counted in the
//! level state ([`ProxyStates`]), so a monster whose level streamed out is not dead
//! (`IncrementProgress(force)`). `GameEngine::OnCreatureDeath`: a kill of a dungeon monster (any floor;
//! none on `blacklistedHeroes`) adds a hero 4 s / boss 12 s / nemesis 12 s of bonus time while the boss lives and
//! counts toward the kill chain (pays `killCountBonusTime` after [`gd_story::endless::Ruleset::kill_count_reset_ms`]
//! without a kill once `killCountMinimum` kills are in); one in the active, non-boss floor adds 1 to its progress.
//! `IncrementProgress`: at the floor total the next floor
//! becomes active, the finished floor's proxies are disabled and its monsters killed
//! (`KillMonstersInRegion`). A boss floor (total 100) ends when every boss of its proxies is dead
//! (`IsReallyDead`): `bossDead`, and `bonusComplete` when the timer still ran; then the generator
//! resets the bonus timer (`SetEndlessTimer` reset, `ResetSurvivalMultiplier`'s event 64: `timerFailedSound` when
//! it had already run out) and sends `Endless_SoundType` 1 (event 67), whose HUD handler (exe
//! `UIEndlessHud::OnSoundEvent`, only while the HUD shows) plays `bossCompleteSound` and unlocks tutorial page 0x42
//! ([`BOSS_TUTORIAL_PAGE`]).
//!
//! The ruleset's per-difficulty fields are read at the session's mode ([`gd_game_core::session::GameMode`],
//! `gd_story::endless::Ruleset::slot`: Ultimate Veteran has its own adjustment pak, the Ultimate timer).
//!
//! Buff shrines: `ProxyEndless::InitializeAccessoryPool`:
//! a shrine proxy activated after the dungeon used every accessory pool places no shrine
//! ([`spawn_activated_shrines`]); `EndlessBuffShrine::RequestToUse`: the player casts the
//! shrine's `Skill_BuffOther`, so its pets within 30 m get the buff too ([`use_shrine`]);
//! `DispelEndlessShrineBuffs`: the dispels reach the pets of the player's pet pen
//! ([`dispel_endless_buffs`]).
//!
//! PORT: the dungeon's monsters are its activated proxies' (the engine flags a monster in any of its regions).
//!
//! PORT test hook `GD_ENDLESS_FLOOR` (`force_floor`, dev builds): a held override — floor 0 of every generated section is
//! the named floor record (through [`gd_story::endless::EndlessMap::forced_floor`], which `pick_floors` reads);
//! placed (story) monsters log their level once armed.
//!
//! Save / quit / reload: nothing of a run reaches the save. The
//! generator has no save code and `GameEngine::ExitPlayingMode` deletes it; `EnableGameEngine`
//! sets `dungeonsGenerated` to 0 at every game start; the game start (exe `Game::LoadModMap`) reloads the
//! Lua scripts and puts the player at its respawn point. The dungeon objects' `SaveState` (`ProxyEndless`
//! state / active / stamp, `EndlessBuffShrine`, the exit gates, the essences) goes only
//! into the level state, which Engine.dll keeps in memory while a level is unloaded (region unload `Region::Unload`
//! -> `Level::SaveState` into a buffer of the region, restored and freed by the region load
//! `Region::Load`); the character files are `player.gdc`, `quests.gdd`, `conversations.gdd`, `map.fow`. What a
//! run leaves in them: the quests and tokens the scripts gave, and the `PlayStats` endless souls / essence
//! (counted where the essence buff lands, `crate::play_stats`). Here the run is session
//! state (`gd_story::story::Endless`, [`ProxyStates`], [`ShrineMemory`]) that the session's end resets; within a
//! session the level states are [`ProxyStates`] / [`ShrineMemory`] and the stamp rule is [`drop_stale_proxies`].
use crate::combat::MonsterKilled;
use crate::monsters::{EndlessProxy, ProfileCache, ProxyStates, spawn_state};
use gd_game_core::combat::Dead;
use gd_game_core::monsters::Monster;
use gd_game_core::skill_buffs::{ReceivedBuff, ReceivedBuffs};
use gd_game_core::story::StoryRuntime;
use gd_story::story::lua::ScriptEnv as _;

use bevy::prelude::*;

/// The tutorial page the Shattered Realm HUD unlocks when a Shard's boss dies (index 66 of `helpindex.dbr`'s
/// `tutorialPages`: `endlessdungeonend/tutorial_p67_end.dbr`, `tagQuickTip67`).
pub const BOSS_TUTORIAL_PAGE: u32 = 0x42;

/// Activates the active floor's proxies (see the module docs).
#[allow(clippy::too_many_arguments)]
pub fn activate_proxies(
    mut commands: Commands,
    rt: Option<NonSend<StoryRuntime>>,
    proxies: Query<&EndlessProxy>,
    mut states: ResMut<ProxyStates>,
    mut profiles: ResMut<ProfileCache>,
    rules: Res<gd_game_core::rules::CombatRules>,
    mut assets: crate::creature::Spawner,
    factions: Option<Res<crate::factions::PlayerFactions>>,
    mode: Res<gd_game_core::session::GameMode>,
    mut anims: ResMut<gd_game_core::character::AnimCache>,
    mut clips: Local<crate::creature::ClipLists>,
    mut new: gd_game_core::session::NewSession,
) {
    if new.begun() {
        clips.clear();
    }
    let Some(rt) = rt else { return };
    let mut story = rt.story.borrow_mut();
    let region = story.player.region;
    let st = &mut *story;
    let e = &mut st.endless;
    let Some(d) = e.dungeon.as_mut() else { return };
    let Some(floor) = d.floors.get_mut(d.active) else {
        return;
    };
    // Only while the player stands in the active floor.
    if !region.is_some_and(|r| floor.regions.contains(&r)) {
        return;
    }
    // The section's first entry (`sectionEntered`): its bonus timer (`SetEndlessTimer(ms, 0, reset 0, addBonus 0)` =
    // `SetBonusTimer`, the `timer*` equation) only in the session's first timed section or after a boss beat
    // the clock (see the module docs); starting it clears the bonus flag. The ruleset is re-read here.
    if !e.timer_started {
        e.timer_started = true;
        if e.timer_ever && !e.bonus_complete {
            debug!("endless: no timer (the last boss did not beat the clock)");
        } else {
            e.timer_ever = true;
            e.bonus_complete = false;
            if let Some(rs) = gd_story::endless::Ruleset::load(
                &assets.data.db,
                &d.ruleset,
                gd_story::endless::Ruleset::slot(&mode.0),
            ) {
                let ms = rs.timer_ms(e.generated);
                st.game.survival.set_timer(ms, false);
            }
        }
    }
    // uid → proxy entity, built once per frame and only while a proxy of the floor is not activated.
    let mut by_uid: Option<gd_formats::collections::HashMap<_, &EndlessProxy>> = None;
    // PORT: the engine activates the floor's proxies in one frame; here a proxy waits until its monsters' meshes
    // and animation clips are parsed in the background ([`crate::creature::ready_to_spawn`]) and at most
    // [`crate::creature::SPAWNS_PER_FRAME`] monsters spawn a frame, so entering a floor does not stall a frame on
    // dozens of spawns and a new monster never parses a clip mid-fight.
    let mut budget = crate::creature::SPAWNS_PER_FRAME;
    let deadline = std::time::Instant::now() + crate::creature::CLIPS_BUDGET;
    for p in floor.proxies.iter_mut().filter(|p| !p.activated) {
        let by_uid = by_uid.get_or_insert_with(|| {
            proxies
                .iter()
                .filter_map(|ep| Some((ep.uid?, ep)))
                .collect()
        });
        let Some(&ep) = by_uid.get(&p.entity.uid) else {
            continue;
        };
        let Some(list) = states.0.get_mut(&ep.key) else {
            continue;
        };
        let mut ready = true;
        for m in list.iter().filter(|m| m.alive && m.pending) {
            ready &= crate::creature::ready_to_spawn(
                &mut assets,
                &mut anims,
                &mut clips,
                &m.record,
                m.variant,
                deadline,
            );
        }
        if !ready || budget == 0 {
            continue;
        }
        p.activated = true;
        debug!(
            "endless: floor {} activates a {:?} proxy {:?}",
            d.active, p.kind, ep.key
        );
        e.activated.push((d.active, ep.key));
        // Active again (`SetActive` restamps it): no longer stale, its state is the running dungeon's.
        e.stale.retain(|k| *k != ep.key);
        for (i, m) in list.iter_mut().enumerate() {
            if m.alive && m.pending {
                m.pending = false;
                budget = budget.saturating_sub(1);
                spawn_state(
                    &mut commands,
                    &assets.data,
                    &mut assets.bodies,
                    &rules,
                    factions.as_deref(),
                    &mut profiles,
                    m,
                    (ep.key.0, ep.key.1, i),
                    &ep.patrol,
                );
            }
        }
    }
}

/// A monster's classification (`monsterClassification`).
fn class_of<'a>(data: &'a gd_game_core::data::GameData, record: &str) -> &'a str {
    data.db
        .get(record)
        .and_then(|r| r.str("monsterClassification"))
        .unwrap_or_default()
}

/// Kills, the kill chain's bonus time, floor progress and the boss floor (see the module docs).
#[allow(clippy::too_many_arguments)]
pub fn progress(
    mut commands: Commands,
    mut players: Query<(Entity, Option<&mut ReceivedBuffs>), With<gd_game_core::player::Player>>,
    time: Res<Time>,
    mut carry: Local<f32>,
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<gd_game_core::data::GameData>,
    mut killed: MessageReader<MonsterKilled>,
    monsters: Query<(&Monster, Has<Dead>)>,
    mut states: ResMut<ProxyStates>,
    (mut tutorial, mut stats): (
        MessageWriter<crate::tutorials::UnlockTutorial>,
        MessageWriter<crate::play_stats::PlayStat>,
    ),
    mode: Res<gd_game_core::session::GameMode>,
    (mut fresh, mut cached): (
        gd_game_core::session::NewSession,
        Local<Option<(String, usize, gd_story::endless::Ruleset)>>,
    ),
) {
    if fresh.begun() {
        *cached = None;
    }
    let Some(rt) = rt else { return };
    let mut story = rt.story.borrow_mut();
    let st = &mut *story;
    let region = st.player.region;
    let e = &mut st.endless;
    let Some(d) = e.dungeon.as_mut() else {
        killed.clear();
        return;
    };
    // The ruleset is parsed once per (record, slot), not every frame.
    let slot = gd_story::endless::Ruleset::slot(&mode.0);
    if cached
        .as_ref()
        .is_none_or(|(r, s, _)| r != &d.ruleset || *s != slot)
    {
        *cached = gd_story::endless::Ruleset::load(&data.db, &d.ruleset, slot)
            .map(|rs| (d.ruleset.clone(), slot, rs));
    }
    let Some((_, _, rs)) = cached.as_ref() else {
        return;
    };
    let dt = gd_game_core::session::frame_ms(&mut carry, &time);
    let revealed = d.reveal_monsters();
    let active = d.active;
    let keys: Vec<(usize, u32)> = e
        .activated
        .iter()
        .filter(|(f, _)| *f == active)
        .map(|(_, k)| *k)
        .collect();
    let boss_floor = d.floors.get(active).is_some_and(|f| f.floor.boss);
    // Kills (`GameEngine::OnCreatureDeath`): a monster of the dungeon (any of its floors)
    // not on `blacklistedHeroes` (the blacklist skips all of it) adds its classification's bonus time while the
    // boss lives and joins the kill chain; one of the active non-boss floor adds 1 to its progress
    // (`IncrementProgress` counts only there) and rolls its essence. Their `Monster` stays on the corpse.
    // Essences the dungeon's monsters dropped (`loot::on_kill`: the special essence of a boss).
    let mut essences: Vec<String> = std::mem::take(&mut e.essence_queue);
    for k in killed.read() {
        let Ok((m, _)) = monsters.get(k.entity) else {
            continue;
        };
        let key = (m.slot.0, m.slot.1);
        if !e.activated.iter().any(|(_, a)| *a == key)
            || rs
                .blacklisted_heroes
                .contains(&m.record.to_ascii_lowercase())
        {
            continue;
        }
        if !e.boss_dead {
            let bonus = rs.bonus_for_class(class_of(&data, &m.record));
            if bonus > 0 && st.game.survival.timer_ms > 0 {
                st.game.survival.timer_add(bonus);
                info!("endless: +{bonus} ms bonus time ({})", m.record);
                if let Some(t) = rt.env.localize("tagEndlessDungeonHeroBonusTime") {
                    e.texts.push(gd_story::endless::bonus_text(&t, None, bonus));
                }
            }
        }
        e.chain.kill(rs.kill_count_reset_ms);
        if keys.contains(&key)
            && !boss_floor
            && let Some(f) = d.floors.get_mut(active)
        {
            f.progress += 1.0;
            // Essence drops (`ControllerMonster::DropLoot`): a monster not on the
            // blacklist, in the active non-boss floor, rolls its classification's chance (percent,
            // `rand(0, 100) <= chance`). The item is picked up and used at once (auto-pickup 4 m,
            // `OneShotFilter`), so it is applied here without an item entity.
            let idx = gd_story::endless::Ruleset::essence_index(class_of(&data, &m.record));
            if !rs.essence_item[idx].is_empty()
                && gd_game_core::rng::f32() * 100.0 <= rs.essence_chance[idx]
            {
                essences.push(rs.essence_item[idx].clone());
            }
        }
    }
    // Using the essences: the item's progress, and the soul buff with one more charge.
    for item in essences {
        let Some(rec) = data.db.get(&item) else {
            continue;
        };
        if let Some(f) = d.floors.get_mut(active) {
            f.progress += rec.f32("progressAmount").unwrap_or(0.0);
        }
        let Some(skill) = rec.str("skillName").map(str::to_owned) else {
            continue;
        };
        let Some(buff) = data
            .db
            .get(&skill)
            .and_then(|s| s.str("buffSkillName").map(str::to_owned))
        else {
            continue;
        };
        // `OneShot_Skill::Use` -> `Skill_ChargedBuffOther::IncrementCharge(skillIncrement)` (default 1): below
        // the cap the charge grows by the increment, then is clamped to the cap, the buff's `skillChargeLevel` at
        // the skill's level (one value in every stock record).
        let buff_rec = data.db.get(&buff);
        let max = buff_rec
            .and_then(|b| gd_sim::stats::level_value(b, "skillChargeLevel", 0))
            .unwrap_or(0.0)
            .max(0.0) as u32;
        let inc = rec.f32("skillIncrement").unwrap_or(1.0).max(0.0) as u32;
        e.soul_charges = e.soul_charges.saturating_add(inc).min(max);
        debug!("endless: essence {item}: soul charges {}", e.soul_charges);
        if e.soul_charges == 0 {
            continue;
        }
        if let Ok((player, list)) = players.single_mut() {
            // `Skill_ChargedBuffOther::TargetResult` at the charge's level (`crate::play_stats`).
            stats.write(crate::play_stats::PlayStat::Essence {
                buff: buff.clone(),
                level: e.soul_charges,
            });
            // Each pickup refreshes the buff for the buff's `skillChargeDuration` at the charge level (`TargetResult`).
            let secs = buff_rec
                .and_then(|b| {
                    gd_sim::stats::level_value(
                        b,
                        "skillChargeDuration",
                        e.soul_charges as usize - 1,
                    )
                })
                .unwrap_or(0.0);
            let b = ReceivedBuff::new(
                &data.db,
                &buff,
                &skill,
                e.soul_charges as usize - 1,
                player,
                Vec::new(),
                secs,
            );
            match list {
                Some(mut l) => {
                    l.land(b, |_| true);
                }
                None => {
                    let mut l = ReceivedBuffs::default();
                    l.land(b, |_| true);
                    commands.entity(player).try_insert(l);
                }
            }
            commands
                .entity(player)
                .try_insert(gd_game_core::player::ProfileDirty);
        }
    }
    // The kill chain pays out once it pauses (`Update`: only a positive bonus reaches the timer).
    let kills = e.chain.count;
    if let Some(ms) = e.chain.tick(dt, e.boss_dead, rs)
        && ms > 0
        && st.game.survival.timer_ms > 0
    {
        st.game.survival.timer_add(ms);
        info!("endless: kill chain of {kills}: +{ms} ms bonus time");
        if let Some(t) = rt.env.localize("tagEndlessDungeonBonusTime") {
            e.texts
                .push(gd_story::endless::bonus_text(&t, Some(kills), ms));
        }
    }
    // The maps start marking the floor's last monsters (`grimdawn::ui::localmap`); `GD_SHOT_ON=endlessreveal`.
    if !revealed && d.reveal_monsters() {
        info!("endless: floor {active} at 80 %: its monsters are revealed");
        gd_game_core::visuals::notify(&mut commands, "endlessreveal");
    }
    // The floor's living monsters come from the level state ([`ProxyStates`]),
    // not the loaded entities: a monster whose level streamed out is still
    // alive in it (the engine keeps it in the level state; `IsReallyDead`
    // reads the proxy's spawned list, not the world's entities).
    let alive = |states: &ProxyStates, keys: &[(usize, u32)], bosses_only: bool| {
        keys.iter()
            .filter_map(|k| states.0.get(k))
            .flatten()
            .filter(|m| {
                m.alive
                    && !m.pending
                    && (!bosses_only || matches!(class_of(&data, &m.record), "Boss" | "Quest"))
            })
            .count()
    };
    let finish = |d: &mut gd_story::endless::Dungeon,
                  e_keys: &mut Vec<(usize, u32)>,
                  keys: &[(usize, u32)],
                  states: &mut ProxyStates| {
        if let Some(f) = d.floors.get_mut(active) {
            f.progress = f.total;
        }
        // The floor's proxies are disabled for good: nothing held spawns later.
        for k in keys {
            if let Some(list) = states.0.get_mut(k) {
                for m in list.iter_mut().filter(|m| m.pending) {
                    m.alive = false;
                }
            }
        }
        e_keys.extend(keys);
    };
    if boss_floor {
        if !e.boss_dead && d.boss_floor_done(alive(&states, &keys, true)) {
            e.boss_dead = true;
            if st.game.survival.timer_ms > 0 {
                e.bonus_complete = true;
            }
            info!("endless: the boss is dead (bonus {})", e.bonus_complete);
            finish(d, &mut e.kill_keys, &keys, &mut states);
            // The timer stops (`SetEndlessTimer(0, 0, reset 1)`; event 64), then event 67 type 1: the HUD,
            // while shown (the player in a floor), unlocks the end-of-Shard tutorial page.
            st.game.survival.reset_timer(false);
            if region.is_some_and(|r| d.floors.iter().any(|f| f.regions.contains(&r))) {
                tutorial.write(crate::tutorials::UnlockTutorial(BOSS_TUTORIAL_PAGE));
            }
            // `GD_SHOT_ON=endlessbosscomplete` (hooks.rs).
            gd_game_core::visuals::notify(&mut commands, "endlessbosscomplete");
        }
        return;
    }
    // A normal floor: all its proxies ran and their monsters are dead, or the progress reached the total.
    if d.normal_floor_done(alive(&states, &keys, false)) {
        finish(d, &mut e.kill_keys, &keys, &mut states);
        d.active += 1;
        info!("endless: floor {active} done, floor {} is active", d.active);
    }
}

/// The cinematic texts (`ShowCinematicText` type 9: scale 1.25, 3 s, `ui/cinematic.rs`).
pub fn show_texts(
    mut rt: Option<NonSendMut<StoryRuntime>>,
    mut out: MessageWriter<gd_game_core::story::CinematicText>,
) {
    let Some(rt) = rt.as_mut() else { return };
    let texts = std::mem::take(&mut rt.story.borrow_mut().endless.texts);
    for t in texts {
        out.write(gd_game_core::story::CinematicText::plain(t, 9));
    }
}

/// `KillMonstersInRegion` for a finished floor (`Endless::kill_keys`): its remaining monsters die.
pub fn kill_finished_floor(
    mut commands: Commands,
    rt: Option<NonSend<StoryRuntime>>,
    mut monsters: Query<
        (
            Entity,
            &Monster,
            &Transform,
            &mut gd_game_core::character::Character,
            &mut gd_game_core::combat::Combatant,
        ),
        Without<Dead>,
    >,
    mut states: ResMut<ProxyStates>,
    mut killed: MessageWriter<MonsterKilled>,
) {
    let Some(rt) = rt else { return };
    let keys = std::mem::take(&mut rt.story.borrow_mut().endless.kill_keys);
    if keys.is_empty() {
        return;
    }
    for (e, m, tf, mut ch, mut cb) in &mut monsters {
        if !keys.contains(&(m.slot.0, m.slot.1)) {
            continue;
        }
        cb.life = 0.0;
        crate::combat::kill(
            &mut commands,
            e,
            tf.translation,
            Some(&mut ch),
            Some(m),
            false,
            &mut states,
            &mut killed,
            None,
        );
    }
}

/// The objective line the HUD shows in the dungeon (`UIEndlessHud::Update`): in the boss
/// region `tagEndlessFloorBoss` (`...BossDead` once the boss is dead), elsewhere `tagEndlessFloorNotComplete`
/// in the active floor and `tagEndlessFloorComplete` in a finished one. The tag replaces the whole objective list
/// (`SyncObjectivesInbound` with add and clear) when it is not already the list.
pub fn sync_objectives(rt: Option<NonSend<StoryRuntime>>) {
    let Some(rt) = rt else { return };
    let mut story = rt.story.borrow_mut();
    let st = &mut *story;
    let Some(region) = st.player.region else {
        return;
    };
    if !st.endless.in_dungeon(region, false) {
        return;
    }
    let tag = if st.endless.is_boss_region(region) {
        if st.endless.boss_dead {
            "tagEndlessFloorBossDead"
        } else {
            "tagEndlessFloorBoss"
        }
    } else if st.endless.in_dungeon(region, true) {
        "tagEndlessFloorNotComplete"
    } else {
        "tagEndlessFloorComplete"
    };
    let obj = &mut st.game.survival.objectives;
    if !obj.iter().any(|(t, _)| t == tag) {
        *obj = vec![(tag.to_owned(), 0)];
    }
}

/// `Game.RemoveEndlessSoulBuffs` / `RemoveEndlessShrineBuffs` (`DispelEndlessSoulBuffs` /
/// `DispelEndlessShrineBuffs`): the `SkillBuff_PassiveCharged` (soul) / `SkillBuff_PassiveEndless`
/// (shrine) buffs end on the main player and on every pet of its pet pen (the player's SkillManager,
/// then each `Character` of `GetPetPen`'s list; the quest pet pen is not dispelled); the soul
/// charges reset. Entering through a door or an exit gate dispels the souls.
pub fn dispel_endless_buffs(
    mut commands: Commands,
    mut events: MessageReader<gd_game_core::story::StoryEffect>,
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<gd_game_core::data::GameData>,
    players: Query<Entity, With<gd_game_core::player::Player>>,
    mut buffs: Query<(Entity, &mut ReceivedBuffs, Option<&crate::pets::Pet>)>,
) {
    use gd_story::story::Effect;
    let (mut souls, mut shrines) = (false, false);
    for e in events.read() {
        match e.0 {
            Effect::RemoveEndlessSoulBuffs => souls = true,
            Effect::RemoveEndlessShrineBuffs => shrines = true,
            _ => {}
        }
    }
    if !souls && !shrines {
        return;
    }
    if souls && let Some(rt) = rt {
        rt.story.borrow_mut().endless.soul_charges = 0;
    }
    let Ok(player) = players.single() else {
        return;
    };
    for (e, mut list, pet) in &mut buffs {
        let is_player = e == player;
        if !is_player && !pet.is_some_and(|p| p.owner == player && !p.quest) {
            continue;
        }
        let before = list.list.len();
        list.list.retain(|b| {
            let class = data.db.get(&b.buff).map(|r| r.class.as_str()).unwrap_or("");
            !(souls && class == "SkillBuff_PassiveCharged"
                || shrines && class == "SkillBuff_PassiveEndless")
        });
        if list.list.len() != before {
            // A pet's profile is rebuilt when its received buffs changed (`pets::refresh_profiles`).
            list.changed = true;
            if is_player {
                commands
                    .entity(e)
                    .try_insert(gd_game_core::player::ProfileDirty);
            }
        }
    }
}

/// The soul buff ran out: its charges reset (a buff the player no longer has).
pub fn soul_expiry(
    mut commands: Commands,
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<gd_game_core::data::GameData>,
    players: Query<(Entity, Option<&ReceivedBuffs>), With<gd_game_core::player::Player>>,
) {
    let Some(rt) = rt else { return };
    let mut story = rt.story.borrow_mut();
    if story.endless.soul_charges == 0 {
        return;
    }
    let Ok((player, list)) = players.single() else {
        return;
    };
    let has = list.is_some_and(|l| {
        l.list.iter().any(|b| {
            data.db
                .get(&b.buff)
                .is_some_and(|r| r.class == "SkillBuff_PassiveCharged")
        })
    });
    if !has {
        story.endless.soul_charges = 0;
        commands
            .entity(player)
            .try_insert(gd_game_core::player::ProfileDirty);
    }
}

/// `Player::CharacterIsDying`: a death in any region of the dungeon takes the ruleset's
/// `dyingPenalty` from the bonus timer (only the bonus is lost; respawn is the normal bound respawn point outside,
/// `death.rs`). The gravestone goes when the next dungeon is generated (`RemoveGravestoneInDungeon`, see
/// [`sync_endless`]).
pub fn dying_penalty(
    died: Query<(), (With<gd_game_core::player::Player>, Added<Dead>)>,
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<gd_game_core::data::GameData>,
    mode: Res<gd_game_core::session::GameMode>,
    mut monsters: Query<
        (
            &mut gd_game_core::combat::Combatant,
            &crate::combat::MonsterAi,
        ),
        (Without<Dead>, Without<crate::combat::Allied>),
    >,
) {
    if died.is_empty() {
        return;
    }
    let Some(rt) = rt else { return };
    let mut story = rt.story.borrow_mut();
    let st = &mut *story;
    let Some(d) = st.endless.dungeon.as_ref() else {
        return;
    };
    if !st
        .player
        .region
        .is_some_and(|r| st.endless.in_dungeon(r, false))
    {
        return;
    }
    let Some(rs) = gd_story::endless::Ruleset::load(
        &data.db,
        &d.ruleset,
        gd_story::endless::Ruleset::slot(&mode.0),
    ) else {
        return;
    };
    info!("endless: died, bonus timer -{} ms", rs.dying_penalty_ms);
    st.game.survival.timer_subtract(rs.dying_penalty_ms);
    // `Monster::OnKilledPlayer`: every monster that was after
    // the player heals `healthGainOnKillPct` % of its own max life (`CharacterBio::TakeBonus`), the ruleset's
    // 10000 % a full heal; the ruleset sets it for the Veteran challenge and above (`healthGainOnKillDifficulty`).
    // PORT: the life bonus modifiers (`1 + lifeBonusMods`) are not applied.
    if rs.health_gain_pct > 0.0 && mode.0.challenge >= rs.health_gain_challenge {
        for (mut cb, ai) in &mut monsters {
            if ai.aggro {
                let max = cb.profile.sheet.life;
                cb.life = (cb.life + rs.health_gain_pct / 100.0 * max).min(max);
            }
        }
    }
}

/// An exit gate (`FixedItemDungeonTeleport`, `endlessdungeon_riftgate_personal`): stands where the player was
/// in a finished floor, travels to the active floor's entrance when used, and goes when its dungeon is over.
#[derive(Component, Debug)]
pub struct EndlessGate {
    /// The dungeon stamp (`Dungeon::id`): a gate of an older dungeon is stale.
    pub stamp: u32,
    pub floor: usize,
}

/// The exit gates' record.
pub const EXIT_GATE_RECORD: &str =
    "records/endlessdungeon/doors/endlessdungeon_riftgate_personal.dbr";

/// `EndlessDungeon_Generator::Update`: while the player stands in a floor behind the active
/// one, one exit gate is created for that floor at the player's position; a gate whose dungeon stamp is stale is
/// removed (the engine waits 500 ms; PORT: at once).
/// Its look (the record model: an invisible mesh whose `DoorFX` creates the small rift effect) is grimdawn
/// `riftgates.rs`'s; its hover name is `FixedItemTeleport::GetGameDescription`'s `tagRiftgatePersonal` (grimdawn
/// `hover.rs`).
pub fn exit_gates(
    mut commands: Commands,
    rt: Option<NonSend<StoryRuntime>>,
    gates: Query<(Entity, &EndlessGate)>,
    players: Query<&Transform, With<gd_game_core::player::Player>>,
) {
    let Some(rt) = rt else { return };
    let story = rt.story.borrow();
    let d = story.endless.dungeon.as_ref();
    for (e, g) in &gates {
        if d.is_none_or(|d| d.id != g.stamp) {
            commands.entity(e).try_despawn();
        }
    }
    let (Some(d), Some(region), Ok(tf)) = (d, story.player.region, players.single()) else {
        return;
    };
    let Some(k) = d.floors.iter().position(|f| f.regions.contains(&region)) else {
        return;
    };
    if k < d.active && !gates.iter().any(|(_, g)| g.stamp == d.id && g.floor == k) {
        info!("endless: exit gate for floor {k}");
        // `GD_SHOT_ON=riftgate_personal` (hooks.rs).
        gd_game_core::visuals::notify(&mut commands, EXIT_GATE_RECORD);
        commands.spawn((
            EndlessGate {
                stamp: d.id,
                floor: k,
            },
            crate::player_orders::Interactable::fixed(1.0),
            Transform::from_translation(tf.translation),
        ));
    }
}

/// `FixedItemDungeonTeleport::RequestToUse`: in reach, the player travels to the active
/// floor's entrance and the soul buffs are dispelled (no other condition, the gate is reusable).
#[allow(clippy::too_many_arguments)]
pub fn use_exit_gate(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    gates: Query<(Entity, &GlobalTransform), With<EndlessGate>>,
    mut players: Query<
        (
            &Transform,
            &mut crate::movement::MovePath,
            &mut crate::player_orders::InteractTarget,
            &gd_game_core::combat::Combatant,
        ),
        With<gd_game_core::player::Player>,
    >,
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<gd_game_core::data::GameData>,
    mut travel: MessageWriter<crate::riftgates::GateTravel>,
    activity: Res<crate::teleport::TeleportActivity>,
    #[cfg(feature = "dev")] mut fresh: gd_game_core::session::NewSession,
    #[cfg(feature = "dev")] mut tried: Local<Vec<Entity>>,
) {
    // The gate ids are the session's (a reused id is a new gate).
    #[cfg(feature = "dev")]
    if fresh.begun() {
        tried.clear();
    }
    let Ok((ptf, mut path, mut interact, pcb)) = players.single_mut() else {
        return;
    };
    // `GD_USE_EXIT_GATE` (hooks.rs): use each gate once, from anywhere.
    #[cfg(feature = "dev")]
    let test = hooks.gameplay.use_exit_gate;
    #[cfg(feature = "dev")]
    if test
        && interact.0.is_none()
        && let Some((e, _)) = gates.iter().find(|(e, _)| !tried.contains(e))
    {
        tried.push(e);
        interact.0 = Some(e);
    }
    #[cfg(not(feature = "dev"))]
    let test = false;
    // The player is frozen through a teleport's activity (`doors::run_teleport`): the gate is used once it
    // ends, not dropped (a travel granted while another is pending would be lost).
    if activity.active() {
        return;
    }
    let Some(e) = interact.0 else { return };
    let Ok((_, gt)) = gates.get(e) else {
        return;
    };
    let Some(rt) = rt else { return };
    // The fixed actors' use range (`combat::fixed_use_range`): the player's radius + the gate's `Actor::GetRadius`
    // (`actorRadius` 3) + its `usableRange` (4), 3D, +1 m at the walk's end.
    let range = gd_game_core::combat::fixed_use_range(
        data.db.get(EXIT_GATE_RECORD),
        pcb.radius,
        path.points.is_empty(),
    );
    if !test && ptf.translation.distance(gt.translation()) > range {
        return;
    }
    interact.0 = None;
    path.points.clear();
    let mut story = rt.story.borrow_mut();
    let to = story
        .endless
        .dungeon
        .as_ref()
        .and_then(|d| d.floors.get(d.active))
        .and_then(|f| f.entrance.as_ref())
        .map(|e| Vec3::from(e.pos));
    if let Some(to) = to {
        info!("endless: exit gate -> {to:?}");
        story
            .effects
            .push(gd_story::story::Effect::RemoveEndlessSoulBuffs);
        travel.write(crate::riftgates::GateTravel(to));
    }
}

/// A buff shrine (`EndlessBuffShrine`, `records/endlessdungeon/shrines/buffshrine_NN.dbr`) placed by a shrine
/// proxy's accessory pool.
#[derive(Component, Debug)]
pub struct EndlessShrine {
    pub record: String,
    /// `Actor::GetRadius` (`combat::get_radius`): the use range's and the hover pick's.
    pub radius: f32,
    pub used: bool,
    /// The proxy (level, entity index) that placed it.
    pub key: (usize, u32),
}

impl EndlessShrine {
    /// `EndlessBuffShrine::IsOfInterest`: state 1 (unused), or 4 without the `EndlessBuffShrine.used` flag
    /// (PORT: not traced; the port's shrine is unused or used).
    pub fn is_of_interest(&self) -> bool {
        !self.used
    }
}

pub fn spawn_shrine(
    commands: &mut Commands,
    db: &gd_formats::vfs::Database,
    tf: Transform,
    record: &str,
    key: (usize, u32),
    used: bool,
) {
    let Some(rec) = db.get(record) else { return };
    debug!("endless: shrine {record} at {:?}", tf.translation);
    let radius = gd_game_core::combat::get_radius(Some(rec), gd_game_core::combat::ACTOR_RADIUS);
    commands.spawn((
        tf,
        GlobalTransform::from(tf),
        EndlessShrine {
            record: record.to_owned(),
            radius,
            used,
            key,
        },
        crate::player_orders::Interactable::fixed(radius),
        crate::monsters::LevelProp(key.0),
        Name::new("endless shrine"),
    ));
}

/// What the running dungeon remembers of its shrines (the generator's state: the pools used,
/// `EndlessDungeon_Generator.shrinePools`; each
/// shrine's used state). `EndlessBuffShrine::InitialUpdate` / `UpdateSelf`: a
/// shrine carries the stamp of the dungeon that placed it and is destroyed once the stamp is stale, so all of it
/// belongs to one dungeon and resets with the next (`Dungeon::id`).
/// `EndlessBuffShrine::SaveState` / `RestoreState`: the state and stamp go into
/// the level state, kept in memory while the level is unloaded (see the module docs, "Save / quit / reload"),
/// so a shrine used stays used when its level streams back in; here that memory is this resource (session state).
#[derive(Resource, Default)]
pub struct ShrineMemory {
    /// The dungeon stamp the memory belongs to.
    dungeon: Option<u32>,
    /// The accessory pools the dungeon's shrine proxies picked (`InitializeAccessoryPool` excludes them).
    pools: Vec<String>,
    /// Proxies whose pool was picked.
    chosen: gd_formats::collections::HashSet<(usize, u32)>,
    /// Shrines used.
    used: gd_formats::collections::HashSet<(usize, u32)>,
}

/// A shrine proxy ran: the shrine appears at its spot (`proxyActivateSound` plays there, see
/// `grimdawn::endless_shrine`).
#[derive(Message, Clone, Debug)]
pub struct ShrineProxyRan {
    pub record: String,
    pub at: Vec3,
}

/// A new dungeon (another stamp) ends the shrines of the old one and clears [`ShrineMemory`]; the proxies the
/// old dungeon activated become stale ([`drop_stale_proxies`]).
pub fn shrine_memory(
    mut commands: Commands,
    rt: Option<NonSend<StoryRuntime>>,
    mut memory: ResMut<ShrineMemory>,
    shrines: Query<Entity, With<EndlessShrine>>,
) {
    let Some(rt) = rt else { return };
    let mut story = rt.story.borrow_mut();
    let e = &mut story.endless;
    let Some(id) = e.dungeon.as_ref().map(|d| d.id) else {
        return;
    };
    if memory.dungeon != Some(id) {
        if memory.dungeon.is_some() {
            for e in &shrines {
                commands.entity(e).try_despawn();
            }
            let old: Vec<(usize, u32)> = e.activated.drain(..).map(|(_, k)| k).collect();
            for k in old {
                if !e.stale.contains(&k) {
                    e.stale.push(k);
                }
            }
        }
        *memory = ShrineMemory {
            dungeon: Some(id),
            ..default()
        };
    }
}

/// `ProxyEndless::RestoreState`: a proxy whose level comes back with the stamp of an older
/// dungeon (`SetActive` stamps it with the dungeon that activated it) is reset to dormant (state 1),
/// so a floor used again by a later dungeon runs its proxies afresh (`Proxy::RunProxy` picks pools
/// and spawns anew from state 0). While the level stays loaded nothing changes (the generator only flips 1 -> 0).
/// Here: the [`ProxyStates`] (and accessory chests) of a stale proxy are dropped once its level is unloaded, so
/// the next load rolls it again, held for the generator. PORT: the engine also disables (state 2) a proxy restored
/// outside the active floor, or with no generator; here such a proxy stays dormant, and the generator activates
/// only the active floor's proxies. A stale proxy whose level stayed loaded and that a new dungeon activates is
/// no longer stale (its old, spent state is kept, as the engine's state 6 never runs again while loaded).
/// The dungeon rules for a kill (no experience, `progress.rs`; drops, `loot.rs`) follow `activated`, i.e. the
/// running dungeon's proxies; the engine decides by the monster's region (`IsRegionInDungeon` of the running
/// generator), so a leftover monster of an older dungeon outside the new floors gets the normal rules in both.
pub fn drop_stale_proxies(
    rt: Option<NonSend<StoryRuntime>>,
    levels: Res<gd_game_core::levels::Levels>,
    mut states: ResMut<ProxyStates>,
    mut chests: ResMut<gd_game_core::world::ChestStates>,
) {
    let Some(rt) = rt else { return };
    let mut story = rt.story.borrow_mut();
    let stale = &mut story.endless.stale;
    if stale.is_empty() {
        return;
    }
    stale.retain(|k| {
        if levels.loaded.contains_key(&k.0) {
            return true;
        }
        states.0.remove(k);
        chests.accessories.remove(k);
        false
    });
}

/// A shrine proxy's shrine exists only once the generator activated the proxy (the engine runs only the proxies
/// `PickProxies` chose; a level's other `ProxyEndless` entities stay dormant, shrines included): the shrines of the
/// activated proxies of loaded levels are placed here, and again when their level streams back in.
/// `ProxyEndless::InitializeAccessoryPool`: at activation the proxy picks its accessory
/// pool among those the dungeon has not used yet and adds it to the set (the five stock pools are the five buff
/// shrines): the shrine rolled when the level loaded keeps only its spot. With every pool used (a sixth shrine
/// proxy in one dungeon) there is no candidate, no pool is made and the proxy places no shrine
/// ([`gd_sim::proxy::pick_endless_accessory`]).
#[allow(clippy::too_many_arguments)]
pub fn spawn_activated_shrines(
    mut commands: Commands,
    rt: Option<NonSend<StoryRuntime>>,
    mut chests: ResMut<gd_game_core::world::ChestStates>,
    mut memory: ResMut<ShrineMemory>,
    proxies: Query<&EndlessProxy>,
    shrines: Query<&EndlessShrine>,
    data: Res<gd_game_core::data::GameData>,
    mode: Res<gd_game_core::session::GameMode>,
    mut ran: MessageWriter<ShrineProxyRan>,
) {
    let Some(rt) = rt else { return };
    let story = rt.story.borrow();
    for (_, key) in &story.endless.activated {
        let Some(proxy) = proxies.iter().find(|p| p.key == *key) else {
            continue;
        };
        if shrines.iter().any(|s| s.key == *key) {
            continue;
        }
        let Some(list) = chests.accessories.get_mut(key) else {
            continue;
        };
        let fresh = memory.chosen.insert(*key);
        let is_shrine = |rec: &str| {
            data.db
                .get(rec)
                .is_some_and(|r| r.class == "EndlessBuffShrine")
        };
        if fresh {
            // The generator's pick replaces the shrine rolled at the level load; no pick (every pool used, or the
            // pool's item chance failed): no shrine, for as long as the dungeon lasts.
            let mut rng = gd_rules::Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
            let mut item = gd_sim::proxy::pick_endless_accessory(
                &data.db,
                &proxy.record,
                mode.0.difficulty,
                &memory.pools,
                &mut rng,
            )
            .and_then(|(pool, item)| {
                memory.pools.push(pool);
                item
            });
            list.retain_mut(|(rec, pos, _)| {
                if !is_shrine(rec) {
                    return true;
                }
                let Some(new) = item.take() else {
                    debug!("endless: proxy {key:?} places no shrine");
                    return false;
                };
                *rec = new;
                ran.write(ShrineProxyRan {
                    record: rec.clone(),
                    at: *pos,
                });
                true
            });
        }
        for (rec, pos, yaw) in list.iter() {
            if !is_shrine(rec) {
                continue;
            }
            let tf = Transform::from_translation(*pos).with_rotation(Quat::from_rotation_y(*yaw));
            spawn_shrine(
                &mut commands,
                &data.db,
                tf,
                rec,
                *key,
                memory.used.contains(key),
            );
        }
    }
}

/// The Death shrine's buff (`damageAbsorptionPercent` 100) is a damage filter on whoever has it (the player and the
/// pets it reached, [`use_shrine`]) while it runs.
/// `SkillBuff_PassiveEndless` records (`damageAbsorptionPercent` 100, `skillActiveDuration` 30): the
/// buff absorbs all damage. The filter is a [`gd_game_core::casting::ShieldKind::Shrine`] entry of the holder's
/// [`gd_game_core::casting::Shields`], there for as long as a received shrine buff with an absorb is on.
pub fn shrine_shield(
    data: Res<gd_game_core::data::GameData>,
    rt: Option<NonSend<StoryRuntime>>,
    mut holders: Query<(&ReceivedBuffs, &mut gd_game_core::casting::Shields)>,
) {
    use gd_game_core::casting::ShieldKind;
    // Shattered Realm only: without a running dungeon no shrine buff can be on
    // anyone, so the per-buff record scan is skipped entirely.
    let Some(rt) = rt else { return };
    if rt.story.borrow().endless.dungeon.is_none() {
        return;
    }
    for (buffs, mut shields) in &mut holders {
        let absorb = buffs.list.iter().find_map(|b| {
            let r = data.db.get(&b.buff)?;
            let pct = r.f32("damageAbsorptionPercent").filter(|p| *p > 0.0)?;
            (r.class == "SkillBuff_PassiveEndless").then(|| {
                let absorb = gd_sim::combat::Absorb {
                    pct,
                    flat: 0.0,
                    pool: false,
                    types: gd_game_core::casting::qualifying_types(r),
                };
                (b.buff.clone(), absorb)
            })
        });
        let has = shields.0.iter().any(|s| s.kind == ShieldKind::Shrine);
        match (absorb, has) {
            (Some((skill, absorb)), false) => shields.add(gd_game_core::casting::Shield {
                skill,
                absorb,
                left: None,
                kind: ShieldKind::Shrine,
            }),
            (None, true) => shields.0.retain(|s| s.kind != ShieldKind::Shrine),
            _ => {}
        }
    }
}

/// `EndlessBuffShrine::RequestToUse`: in reach (`usableRange`), an unused shrine of the
/// running dungeon is used at once (no offering, one use) by the main player: the player casts the record's
/// `skillName` (`Load`; a `Skill_BuffOther` of the quest skill tree) on itself
/// (`ActivateSkillConfigCmd`, caster and target the player). `Skill_BuffOther::ActivateNow` puts its buff
/// (`SkillBuff_PassiveEndless`, 30 s) on the player and on every friend of the player within the buff's
/// `skillTargetRadius` (30 m): the pets too. Here the cast is a
/// [`gd_game_core::story::PlayerSkillCast`], which [`crate::story::cast_story_skills`] casts from the player that way.
/// The used state is [`ShrineMemory`]'s (per dungeon); the used animation and the `dormantToRestored` sound are
/// the dressing's (`grimdawn::endless_shrine`). PORT: only while a dungeon runs.
#[allow(clippy::too_many_arguments)]
pub fn use_shrine(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    mut shrines: Query<(Entity, &GlobalTransform, &mut EndlessShrine)>,
    mut players: Query<
        (
            &Transform,
            &mut crate::movement::MovePath,
            &mut crate::player_orders::InteractTarget,
            &gd_game_core::combat::Combatant,
        ),
        With<gd_game_core::player::Player>,
    >,
    rt: Option<NonSend<StoryRuntime>>,
    data: Res<gd_game_core::data::GameData>,
    mut memory: ResMut<ShrineMemory>,
    mut cast: MessageWriter<gd_game_core::story::PlayerSkillCast>,
) {
    let Ok((ptf, mut path, mut interact, pcb)) = players.single_mut() else {
        return;
    };
    // `GD_USE_BUFF_SHRINE` (hooks.rs): use the nearest unused shrine, from anywhere.
    #[cfg(feature = "dev")]
    let test = hooks.gameplay.use_buff_shrine;
    #[cfg(feature = "dev")]
    if test && interact.0.is_none() {
        let nearest = shrines
            .iter()
            .filter(|(_, _, s)| !s.used)
            .min_by(|a, b| {
                let d = |g: &GlobalTransform| g.translation().distance(ptf.translation);
                d(a.1).total_cmp(&d(b.1))
            })
            .map(|(e, ..)| e);
        interact.0 = nearest;
    }
    #[cfg(not(feature = "dev"))]
    let test = false;
    let Some(e) = interact.0 else { return };
    let Ok((_, gt, mut shrine)) = shrines.get_mut(e) else {
        return;
    };
    let Some(rt) = rt else { return };
    let Some(rec) = data.db.get(&shrine.record) else {
        interact.0 = None;
        return;
    };
    // Use range (`player_orders::usable_range`): player radius + shrine radius + `usableRange`, 3D, +1 m at the
    // walk's end.
    let reach = gd_game_core::player_orders::usable_range(
        pcb.radius,
        shrine.radius,
        gd_game_core::combat::fixed_usable_range(Some(rec)),
        path.points.is_empty(),
    );
    if !test && ptf.translation.distance(gt.translation()) > reach {
        return;
    }
    interact.0 = None;
    path.points.clear();
    if shrine.used || rt.story.borrow().endless.dungeon.is_none() {
        return;
    }
    let Some(skill) = rec.str("skillName").filter(|s| !s.is_empty()) else {
        return;
    };
    shrine.used = true;
    memory.used.insert(shrine.key);
    info!("endless: shrine {} used", shrine.record);
    cast.write(gd_game_core::story::PlayerSkillCast(skill.to_owned()));
}

/// PORT test hook `GD_ENDLESS_FLOOR=<floor record>` ([`gd_game_core::hooks::GameplayHooks::endless_floor`]): a held
/// override — floor 0 of every generated section is the named floor. The scripts generate inside `gd-story`, so
/// the override travels through the generator's map ([`gd_story::endless::EndlessMap::forced_floor`], read by
/// `pick_floors`): this system builds the map ahead of the first generation (the Lua binding builds it lazily)
/// and keeps the field set. While the hook is set, placed (story) monsters log their level once armed
/// (`Character::Load`'s clamp, [`crate::monsters::placed_char_level`]).
#[cfg(feature = "dev")]
pub fn force_floor(
    hooks: Res<gd_game_core::hooks::Hooks>,
    rt: Option<NonSend<StoryRuntime>>,
    armed: Query<(&gd_game_core::story::StoryActor, &Monster), Added<Monster>>,
) {
    let Some(force) = hooks.gameplay.endless_floor.as_deref() else {
        return;
    };
    for (actor, m) in &armed {
        // Story monsters carry the story's slot marker (`arm_story_characters`).
        if m.slot.0 == usize::MAX {
            info!(
                "endless: forced floor's story monster {} (id {}) level {}",
                m.record, actor.0, m.level
            );
        }
    }
    let Some(rt) = rt else { return };
    let mut st = rt.story.borrow_mut();
    if st.endless.map.is_none() {
        st.endless.map = st
            .endless_provider
            .as_ref()
            .map(|p| std::sync::Arc::new(p()));
    }
    if let Some(map) = st.endless.map.as_mut()
        && map.forced_floor.as_deref() != Some(force)
    {
        std::sync::Arc::make_mut(map).forced_floor = Some(force.to_owned());
    }
}

/// The Shattered Realm's host side: the story's generator reads the world map ([`gd_game_core::save::MapLayout`]),
/// pre-built here at session load (the provider closure remains the Lua binding's fallback), and while a
/// dungeon exists the player's map region is kept in `Player::region` (the first region whose box holds the
/// player on the ground plane; the dungeon's regions are far from the normal world, where they overlap nothing else).
pub fn sync_endless(
    mut commands: Commands,
    rt: Option<NonSend<StoryRuntime>>,
    layout: Res<gd_game_core::save::MapLayout>,
    player: Query<&Transform, With<gd_game_core::player::Player>>,
    graves: Query<(Entity, &Transform), With<crate::death::Gravestone>>,
    // The dungeon stamp and regions seen last frame.
    mut last: Local<Option<(u32, Vec<usize>)>>,
    mut fresh: gd_game_core::session::NewSession,
) {
    use gd_game_core::levels::region_distance;
    if fresh.begun() {
        *last = None;
    }
    let Some(rt) = rt else { return };
    let map = &layout.0;
    let mut story = rt.story.borrow_mut();
    if story.endless_provider.is_none() {
        let map = map.clone();
        story.endless_provider = Some(Box::new(move || {
            gd_story::endless::EndlessMap::from_world(&map)
        }));
    }
    // The ~500-level scan of `EndlessMap::from_world` runs at session load, not inside
    // the first `GenerateEndlessDungeon` Lua call (a one-time Shattered Realm hitch).
    if story.endless.map.is_none() {
        story.endless.map = Some(std::sync::Arc::new(
            gd_story::endless::EndlessMap::from_world(map),
        ));
    }
    if story.endless.dungeon.is_none() {
        story.player.region = None;
        return;
    }
    // `GameEngine::RemoveGravestoneInDungeon` (from the generator's `Clear`): a new
    // dungeon removes the gravestones standing in any region of the old one.
    if let Some(d) = story.endless.dungeon.as_ref()
        && last.as_ref().is_none_or(|(id, _)| *id != d.id)
    {
        if let Some((_, regions)) = last.as_ref() {
            for (e, t) in &graves {
                let at = t.translation;
                if regions
                    .iter()
                    .any(|i| region_distance(&map.regions[*i], at) == 0.0)
                {
                    commands.entity(e).try_despawn();
                }
            }
        }
        let regions = d
            .floors
            .iter()
            .flat_map(|f| f.regions.iter().copied())
            .collect();
        *last = Some((d.id, regions));
    }
    let Ok(t) = player.single() else { return };
    let p = t.translation;
    story.player.region = map
        .regions
        .iter()
        .position(|r| region_distance(r, p) == 0.0);
}

pub struct EndlessPlugin;

impl Plugin for EndlessPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShrineMemory>()
            .add_message::<ShrineProxyRan>()
            .add_systems(
                Update,
                (
                    // First: a new dungeon's stamp retires the old one's activated proxies before the
                    // activation and the progress read `Endless::activated`.
                    shrine_memory,
                    activate_proxies.run_if(resource_exists::<gd_game_core::rules::CombatRules>),
                    progress,
                    show_texts,
                    kill_finished_floor,
                    sync_objectives,
                    dispel_endless_buffs,
                    soul_expiry,
                    exit_gates,
                    drop_stale_proxies,
                    spawn_activated_shrines,
                    shrine_shield,
                    dying_penalty,
                    use_exit_gate.before(crate::riftgates::use_gate),
                    use_shrine.before(crate::riftgates::use_gate),
                )
                    .chain()
                    .run_if(resource_exists::<ProxyStates>),
            );
        #[cfg(feature = "dev")]
        app.add_systems(Update, force_floor.before(crate::story::tick_story));
        app.add_systems(
            Update,
            sync_endless
                .after(gd_game_core::story::StoryStage::Sync)
                .before(gd_game_core::story::StoryStage::Tick)
                .run_if(resource_exists::<gd_game_core::save::MapLayout>),
        );
    }
}
