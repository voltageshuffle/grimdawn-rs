//! Registration of the global tables (`Script`, `MpScript`, `Shared`, `Time`, `UI`, `Game`, `QuestState`) and the
//! class tables with their static functions.
use super::*;

impl ScriptHost {
    pub(super) fn register(&self) -> mlua::Result<()> {
        let lua = &self.lua;
        let g = lua.globals();
        let ctx = &self.ctx;
        lua.load(PRELUDE).set_name("prelude").exec()?;
        for name in ["dofile", "loadfile", "require"] {
            g.set(name, Value::Nil)?;
        }
        g.set("Server", ctx.story.borrow().game.server)?;
        {
            let c = ctx.clone();
            g.set(
                "print",
                lua.create_function(move |_, args: Variadic<Value>| {
                    let line = args
                        .iter()
                        .map(|v| v.to_string().unwrap_or_default())
                        .collect::<Vec<_>>()
                        .join("\t");
                    c.story.borrow_mut().log(format!("lua: {line}"));
                    Ok(())
                })?,
            )?;
        }
        // PORT: `math.random` draws from the session stream (`Story::rng`) instead of the VM's unseeded CRT
        // `rand()`, with Lua 5.1's argument rules; `math.randomseed` does nothing.
        {
            let math: Table = g.get("math")?;
            let c = ctx.clone();
            math.set(
                "random",
                lua.create_function(move |_, (m, n): (Option<f64>, Option<f64>)| {
                    let r = c.story.borrow_mut().rng.next() as f64 / 2_147_483_647.0;
                    let (lo, hi) = match (m, n) {
                        (None, _) => return Ok(r),
                        (Some(m), None) => (1, m as i64),
                        (Some(m), Some(n)) => (m as i64, n as i64),
                    };
                    if lo > hi {
                        return Err(mlua::Error::runtime(
                            "bad argument to 'random' (interval is empty)",
                        ));
                    }
                    Ok((r * (hi - lo + 1) as f64).floor() + lo as f64)
                })?,
            )?;
            math.set(
                "randomseed",
                lua.create_function(|_, _: MultiValue| Ok(()))?,
            )?;
        }

        // Script (Engine.dll `LuaScript::BindClass`).
        let script = lua.create_table()?;
        {
            // Script.Load(path) (`LuaScript::Load` → `LuaManager::Load`).
            let c = ctx.clone();
            script.set(
                "Load",
                lua.create_function(move |lua, path: String| Ok(load_script(lua, &c, &path)))?,
            )?;
            // Script.RegisterForUpdate(uint id, function name, int period ms)
            // (`LuaManager::RegisterForUpdates`): an unknown function logs and is not registered; entries are
            // appended (no de-duplication) with their countdown at 0. PORT: past `MAX_UPDATE_REGS` entries the
            // call logs and registers nothing.
            let c = ctx.clone();
            script.set(
                "RegisterForUpdate",
                lua.create_function(move |lua, (id, f, period): (u32, String, Option<f64>)| {
                    let Some(func) = resolve_function(lua, &f) else {
                        c.story.borrow_mut().log(format!("Error registering object {id} for updates, function '{f}' does not exist!"));
                        return Ok(());
                    };
                    let mut u = c.updates.borrow_mut();
                    if u.len() >= MAX_UPDATE_REGS {
                        c.story.borrow_mut().log(format!("Error registering object {id} for updates, function '{f}': too many registrations"));
                        return Ok(());
                    }
                    let period = period.unwrap_or(0.0) as i32;
                    u.push(UpdateReg { id, function: f.into(), func, period, timer: 0 });
                    Ok(())
                })?,
            )?;
            // Script.UnregisterForUpdate(uint id, function name) (`LuaManager::UnregisterForUpdates`): removes
            // the first match, moving the last entry into its place.
            let c = ctx.clone();
            script.set(
                "UnregisterForUpdate",
                lua.create_function(move |_, (id, f): (u32, String)| {
                    let mut u = c.updates.borrow_mut();
                    if let Some(i) = u.iter().position(|r| r.id == id && *r.function == *f) {
                        u.swap_remove(i);
                    }
                    Ok(())
                })?,
            )?;
            // Script.Event(name) (`GameEngine::ScriptEvent`): only unlocks Steam achievements; passed on as an
            // effect.
            let c = ctx.clone();
            script.set(
                "Event",
                lua.create_function(move |_, args: MultiValue| {
                    let name = arg_str(&args, 0);
                    c.story
                        .borrow_mut()
                        .effects
                        .push(Effect::ScriptEvent(name.into_owned()));
                    Ok(())
                })?,
            )?;
        }
        g.set("Script", script)?;

        // MpScript.QuestEvent / QuestGlobalEvent / LuaGlobalEvent(string) (`GameEngine::QuestCommandEvent` /
        // `QuestCommandGlobalEvent` / `LuaCommandGlobalEvent`): all three call `GameEngine.questEventDispatch`
        // with the event; single player is its own server.
        let mp = lua.create_table()?;
        for name in ["QuestEvent", "QuestGlobalEvent", "LuaGlobalEvent"] {
            mp.set(
                name,
                lua.create_function(move |lua, ev: Value| {
                    let f: Value = lua.globals().get("QuestEventDispatch")?;
                    if let Value::Function(f) = f {
                        f.call::<()>(ev)?;
                    }
                    Ok(())
                })?,
            )?;
        }
        g.set("MpScript", mp)?;

        // Shared.Localize(tag) (`LuaShared::Localize` →
        // `LocalizationManager::LocalizeWithoutParams`; the no-argument
        // overload `LuaShared::LocalizeEmpty` returns "").
        let shared = lua.create_table()?;
        {
            let c = ctx.clone();
            shared.set(
                "Localize",
                lua.create_function(move |_, tag: Option<String>| {
                    Ok(tag
                        .map(|tag| c.env.localize(&tag).unwrap_or(tag))
                        .unwrap_or_default())
                })?,
            )?;
        }
        g.set("Shared", shared)?;

        // Time.Now() (`timeGetTime`, ms). PORT: the session time in ms; differences match except while paused.
        let time = lua.create_table()?;
        {
            let c = ctx.clone();
            time.set(
                "Now",
                lua.create_function(move |_, ()| Ok(c.story.borrow().game.time_ms as f64))?,
            )?;
        }
        g.set("Time", time)?;

        // UI.Notify(tag) (`LuaUi::Notify` →
        // `GameEngine::QuestCommandUiNotify`: localise the tag and show it as
        // cinematic text).
        let ui = lua.create_table()?;
        {
            let c = ctx.clone();
            ui.set(
                "Notify",
                lua.create_function(move |_, tag: String| {
                    c.story.borrow_mut().effects.push(Effect::Notify(tag));
                    Ok(())
                })?,
            )?;
        }
        g.set("UI", ui)?;

        // Debug.AddStatisticText(text) (`Engine::AddStatisticText`). PORT: no statistics display, a no-op.
        let debug = lua.create_table()?;
        debug.set(
            "AddStatisticText",
            lua.create_function(|_, _: MultiValue| Ok(()))?,
        )?;
        g.set("Debug", debug)?;

        // QuestState (`LuaPlayer::BindClass`: Invalid..Complete = 0..4).
        let qs = lua.create_table()?;
        for (i, n) in [
            "Invalid",
            "Unavailable",
            "Available",
            "InProgress",
            "Complete",
        ]
        .iter()
        .enumerate()
        {
            qs.set(*n, i)?;
        }
        g.set("QuestState", qs)?;

        self.register_game()?;
        self.register_classes()?;
        Ok(())
    }

    fn register_game(&self) -> mlua::Result<()> {
        let lua = &self.lua;
        let ctx = &self.ctx;
        let game = lua.create_table()?;
        // Game.Difficulty (`LuaGame::BindClass`: Normal 0, Epic 1, Legendary 2).
        let diff = lua.create_table()?;
        for (i, n) in ["Normal", "Epic", "Legendary"].iter().enumerate() {
            diff.set(*n, i)?;
        }
        game.set("Difficulty", diff)?;
        {
            // GetLocalPlayer() (`LuaGame::GetLocalPlayer`: the main player's
            // object, nil without one).
            let c = ctx.clone();
            game.set(
                "GetLocalPlayer",
                lua.create_function(move |lua, _: MultiValue| {
                    let id = c.story.borrow().player.id;
                    c.obj_value(lua, id)
                })?,
            )?;
            // GetPlayer(uint index) (`LuaGame::GetPlayer`: the index-th
            // player in game, nil past the end — only index 0 in single player).
            let c = ctx.clone();
            game.set(
                "GetPlayer",
                lua.create_function(move |lua, args: MultiValue| {
                    if arg_num(&args, 0) as i64 != 0 {
                        return Ok(Value::Nil);
                    }
                    let id = c.story.borrow().player.id;
                    c.obj_value(lua, id)
                })?,
            )?;
        }
        // GetNumPlayers / GetMinPlayerLevel / GetMaxPlayerLevel /
        // GetAveragePlayerLevel / GetGameDifficulty (`LuaGame::GetNumPlayers`,
        // `GetMinPlayerLevel`, `GetMaxPlayerLevel`, `GetAveragePlayerLevel`, `GetGameDifficulty`: fields
        // of `GameEngine::GetPlayerInfo` — count, min/max level, the rounded
        // mean level, and `GameInfo::GetDifficulty`; one player here).
        game.set("GetNumPlayers", lua.create_function(|_, ()| Ok(1))?)?;
        for name in [
            "GetMinPlayerLevel",
            "GetMaxPlayerLevel",
            "GetAveragePlayerLevel",
        ] {
            let c = ctx.clone();
            game.set(
                name,
                lua.create_function(move |_, ()| Ok(c.story.borrow().player.level))?,
            )?;
        }
        {
            let c = ctx.clone();
            game.set(
                "GetGameDifficulty",
                lua.create_function(move |_, ()| Ok(c.story.borrow().game.difficulty))?,
            )?;
            // GetGameChallenge() (`LuaGame::GetGameChallenge` → `GameInfo::GetChallenge`).
            let c = ctx.clone();
            game.set(
                "GetGameChallenge",
                lua.create_function(move |_, ()| Ok(c.story.borrow().game.challenge))?,
            )?;
            // GetAscendantTokenModifier(token) (`LuaGame::GetAscendantTokenModifier` →
            // `GameEngine::GetAscendantAltarModifier`: the installed altar
            // formula's level for its `luaToken`, 0 if none).
            let c = ctx.clone();
            game.set(
                "GetAscendantTokenModifier",
                lua.create_function(move |_, t: String| {
                    Ok(c.story
                        .borrow()
                        .game
                        .ascendant_tokens
                        .iter()
                        .find(|(k, _)| *k == t)
                        .map_or(0, |(_, l)| *l))
                })?,
            )?;
            // GetGameTime() (`GameEngine.scriptClock`). PORT: the session milliseconds.
            let c = ctx.clone();
            game.set(
                "GetGameTime",
                lua.create_function(move |_, ()| Ok(c.story.borrow().game.time_ms as f64))?,
            )?;
            // IsHardcore() (`LuaGame::IsHardcore` → `GameInfo::GetHardcore`).
            let c = ctx.clone();
            game.set(
                "IsHardcore",
                lua.create_function(move |_, ()| Ok(c.story.borrow().player.hardcore))?,
            )?;
            // PlayersDead() (`LuaGame::PlayersDead`: true unless some player
            // in game is alive): the one player's death, mirrored by the host.
            let c = ctx.clone();
            game.set(
                "PlayersDead",
                lua.create_function(move |_, ()| Ok(c.story.borrow().player.dead))?,
            )?;
            // TeleportPlayer(uint playerId, WorldVec3) (`LuaGame::TeleportPlayer`:
            // the local player → `InitiatePlayerTeleport`, others → a network
            // teleport; nothing on a client).
            let c = ctx.clone();
            game.set(
                "TeleportPlayer",
                lua.create_function(move |_, args: MultiValue| {
                    let me = c.story.borrow().player.id;
                    if arg_id(&args, 0) == Some(me)
                        && let Some(co) = args.get(1).and_then(get_coords)
                    {
                        c.story
                            .borrow_mut()
                            .effects
                            .push(Effect::TeleportPlayer { coords: co });
                    }
                    Ok(())
                })?,
            )?;
            // UnlockNextDifficulty() (`LuaGame::UnlockNextDifficulty` →
            // `GameEngine::GameWon(local player)`).
            let c = ctx.clone();
            game.set(
                "UnlockNextDifficulty",
                lua.create_function(move |_, _: MultiValue| {
                    c.story
                        .borrow_mut()
                        .effects
                        .push(Effect::UnlockNextDifficulty);
                    Ok(())
                })?,
            )?;
            // UnlockTutorial(uint page) (`LuaGame::UnlockTutorial` →
            // `GameEngine::UnlockTutorialPage(page, true)`).
            let c = ctx.clone();
            game.set(
                "UnlockTutorial",
                lua.create_function(move |_, n: f64| {
                    c.story
                        .borrow_mut()
                        .effects
                        .push(Effect::UnlockTutorial(n as u32));
                    Ok(())
                })?,
            )?;
        }
        // Crucible bindings (`LuaGame`, Game.dll `GameEngine` / `PlayStats`): Get* return 0 outside a
        // survival session; Set* / Increment* / Reset* need survival.
        {
            macro_rules! surv {
                ($name:expr, |$sv:ident, $args:ident| $body:expr) => {{
                    let c = ctx.clone();
                    game.set(
                        $name,
                        lua.create_function(move |_, $args: MultiValue| {
                            let mut st = c.story.borrow_mut();
                            if !st.game.survival_mode {
                                return Ok(0.0);
                            }
                            let $sv = &mut st;
                            Ok($body)
                        })?,
                    )?;
                }};
            }
            let num = |a: &MultiValue, i: usize| -> f64 {
                match a.get(i) {
                    Some(Value::Integer(n)) => *n as f64,
                    Some(Value::Number(n)) => *n,
                    _ => 0.0,
                }
            };
            surv!("GetSurvivalWaveTier", |st, _a| st.game.survival.wave_tier
                as f64);
            surv!("SetSurvivalWaveTier", |st, a| {
                st.game.survival.wave_tier = num(&a, 0) as u32;
                0.0
            });
            surv!("IncrementSurvivalWaveTier", |st, _a| {
                st.game.survival.wave_tier += 1;
                0.0
            });
            surv!("SetSurvivalDifficulty", |st, a| {
                st.game.survival.difficulty_ramp = num(&a, 0) as u32;
                0.0
            });
            surv!("IncrementSurvivalDifficulty", |st, _a| {
                st.game.survival.difficulty_ramp += 1;
                0.0
            });
            surv!(
                "GetSurvivalMultiplier",
                |st, _a| st.game.survival.multiplier as f64
            );
            surv!("SetSurvivalTimer", |st, a| {
                let crucible = st.game.survival_mode;
                st.game.survival.set_timer(num(&a, 0) as i32, crucible);
                0.0
            });
            surv!("ResetSurvivalTimer", |st, _a| {
                let crucible = st.game.survival_mode;
                st.game.survival.reset_timer(crucible);
                0.0
            });
            surv!("SurvivalTimerAdd", |st, a| {
                st.game.survival.timer_add(num(&a, 0) as i32);
                0.0
            });
            surv!("GetSurvivalRestarts", |st, _a| st.game.survival.restarts
                as f64);
            surv!("IncrementSurvivalRestarts", |st, _a| {
                st.game.survival.restarts += 1;
                0.0
            });
            surv!("ResetSurvivalRestarts", |st, _a| {
                st.game.survival.restarts = 0;
                0.0
            });
            surv!("StartSurvivalEvent", |st, _a| {
                st.game.survival.event_in_progress = true;
                0.0
            });
            surv!("EndSurvivalEvent", |st, _a| {
                st.game.survival.event_in_progress = false;
                0.0
            });
            surv!("ResetSurvivalEvent", |st, _a| {
                st.effects.push(Effect::SurvivalAutosave);
                st.game.survival.reset_event();
                0.0
            });
            surv!("UnlockNextSurvivalDifficulty", |st, _a| {
                let d = st.game.difficulty;
                st.game.survival.complete_difficulty(d);
                st.effects.push(Effect::CompleteSurvivalDifficulty);
                0.0
            });
            surv!("SaveSurvivalScore", |st, _a| {
                st.game.survival.lock_score();
                st.effects.push(Effect::SaveSurvivalScore);
                0.0
            });
            // Enable does nothing in hardcore.
            surv!("EnableRespawn", |st, _a| {
                if !st.player.hardcore {
                    st.game.survival.respawn = true;
                    st.game.survival.respawn_requested = true;
                }
                0.0
            });
            surv!("DisableRespawn", |st, _a| {
                st.game.survival.respawn = false;
                0.0
            });
            surv!("KillAllMonsters", |st, _a| {
                st.game.survival.kill_on_respawn = false;
                st.effects.push(Effect::KillAllMonsters);
                0.0
            });
            surv!("KillAllMonstersOnNextRespawn", |st, _a| {
                st.game.survival.kill_on_respawn = true;
                0.0
            });
            surv!("ResetMutators", |st, _a| {
                st.game.survival.mutators.clear();
                0.0
            });
            surv!("NotifyNewMutators", |st, a| {
                let plural = matches!(a.front(), Some(Value::Boolean(true)));
                st.effects.push(Effect::NotifyNewMutators(plural));
                0.0
            });
            // AddMutator(record): deduplicated by path.
            let c = ctx.clone();
            game.set(
                "AddMutator",
                lua.create_function(move |_, p: String| {
                    let mut st = c.story.borrow_mut();
                    let p = p.replace('\\', "/").to_ascii_lowercase();
                    // The Crucible's and, while a dungeon runs, the Shattered Realm's (the engine tags them with a mode:
                    // 0 / 1; PORT: both apply to every monster of the session).
                    if (st.game.survival_mode || st.endless.dungeon.is_some())
                        && !st.game.survival.mutators.contains(&p)
                    {
                        st.game.survival.mutators.push(p);
                    }
                    Ok(())
                })?,
            )?;
            // AddObjective(tag [, int]) / RemoveObjective(tag) / ClearObjectives().
            let c = ctx.clone();
            game.set(
                "AddObjective",
                lua.create_function(move |_, (tag, n): (String, Option<i32>)| {
                    let mut st = c.story.borrow_mut();
                    if st.game.survival_mode {
                        let o = &mut st.game.survival.objectives;
                        match o.iter_mut().find(|(t, _)| *t == tag) {
                            Some(e) => e.1 = n.unwrap_or(0),
                            None => o.push((tag, n.unwrap_or(0))),
                        }
                    }
                    Ok(())
                })?,
            )?;
            let c = ctx.clone();
            game.set(
                "RemoveObjective",
                lua.create_function(move |_, tag: String| {
                    c.story
                        .borrow_mut()
                        .game
                        .survival
                        .objectives
                        .retain(|(t, _)| *t != tag);
                    Ok(())
                })?,
            )?;
            let c = ctx.clone();
            game.set(
                "ClearObjectives",
                lua.create_function(move |_, _: MultiValue| {
                    c.story.borrow_mut().game.survival.objectives.clear();
                    Ok(())
                })?,
            )?;
            // PlayFullscreenGlow / AllowNpcTalk(bool) / PlayNetSound(record).
            let c = ctx.clone();
            game.set(
                "PlayFullscreenGlow",
                lua.create_function(move |_, _: MultiValue| {
                    c.story.borrow_mut().effects.push(Effect::FullscreenGlow);
                    Ok(())
                })?,
            )?;
            let c = ctx.clone();
            game.set(
                "AllowNpcTalk",
                lua.create_function(move |_, b: bool| {
                    c.story.borrow_mut().game.survival.npc_talk = b;
                    Ok(())
                })?,
            )?;
            let c = ctx.clone();
            game.set(
                "PlayNetSound",
                lua.create_function(move |_, r: String| {
                    c.story.borrow_mut().effects.push(Effect::PlaySound {
                        record: r,
                        at: None,
                    });
                    Ok(())
                })?,
            )?;
        }
        // Shattered Realm bindings (`LuaGame` exe thunks and the Game.dll
        // `GameEngine` functions).
        {
            // GenerateEndlessDungeon(ruleset, entranceDoorId [, startTier]) -> WorldCoords of floor 0's
            // entrance (`GameEngine::GenerateEndlessDungeon`): dungeonsGenerated becomes `startTier` if given,
            // else +1. The Ultimate Veteran takes the fourth timer / adjustment slot. PORT: the entrance door
            // id is not used.
            let c = ctx.clone();
            game.set(
                "GenerateEndlessDungeon",
                lua.create_function(move |lua, args: MultiValue| {
                    let ruleset = match args.front() {
                        Some(Value::String(s)) => s.to_string_lossy(),
                        _ => String::new(),
                    };
                    let start = match args.get(2) {
                        Some(Value::Integer(n)) => Some(*n as u32),
                        Some(Value::Number(n)) => Some(*n as u32),
                        _ => None,
                    };
                    let door = arg_id(&args, 1);
                    // The story borrow must not span a Lua call (`coords_value`) or the host's map provider.
                    let at = 'entrance: {
                        let mut st = c.story.borrow_mut();
                        let Some(db) = c.env.database() else {
                            break 'entrance [0.0; 3];
                        };
                        let difficulty = match (st.game.difficulty, st.game.challenge) {
                            (2, 3) => 3,
                            (d, _) => d as usize,
                        };
                        let Some(rs) = crate::endless::Ruleset::load(db, &ruleset, difficulty)
                        else {
                            st.log(format!("GenerateEndlessDungeon: no ruleset {ruleset}"));
                            break 'entrance [0.0; 3];
                        };
                        let built = if st.endless.map.is_none() {
                            let provider = st.endless_provider.take();
                            drop(st);
                            let built = provider.as_ref().map(|p| std::sync::Arc::new(p()));
                            st = c.story.borrow_mut();
                            st.endless_provider = provider;
                            built
                        } else {
                            None
                        };
                        if st.endless.map.is_none() {
                            st.endless.map = built;
                        }
                        let Some(map) = st.endless.map.clone() else {
                            break 'entrance [0.0; 3];
                        };
                        let mut rng = st.rng;
                        let e = &mut st.endless;
                        e.entrance_door = door;
                        e.generated = start.unwrap_or(e.generated + 1);
                        e.next_id += 1;
                        // `EndlessDungeon_Generator::Clear`: bossDead, the timer latch and the kill chain's
                        // counter reset; the timer-ran / bonus flags are kept.
                        e.boss_dead = false;
                        e.timer_started = false;
                        e.chain.count = 0;
                        let mut recent = std::mem::take(&mut e.recent);
                        let d = crate::endless::generate(
                            &rs,
                            db,
                            &map,
                            &mut recent,
                            &mut rng,
                            e.generated,
                            e.next_id,
                        );
                        e.recent = recent;
                        let at = d
                            .floors
                            .first()
                            .and_then(|f| f.entrance.as_ref())
                            .map_or([0.0; 3], |en| en.pos);
                        e.dungeon = Some(d);
                        st.rng = rng;
                        at
                    };
                    coords_value(lua, &Coords::at(at))
                })?,
            )?;
            let c = ctx.clone();
            game.set(
                "GetEndlessDungeonsGenerated",
                lua.create_function(move |_, ()| Ok(c.story.borrow().endless.generated))?,
            )?;
            // ResetEndlessDungeon() (`LuaGame::ResetEndlessDungeon`): the count back to 0, the mutators cleared.
            let c = ctx.clone();
            game.set(
                "ResetEndlessDungeon",
                lua.create_function(move |_, ()| {
                    let mut st = c.story.borrow_mut();
                    st.endless.generated = 0;
                    st.game.survival.mutators.clear();
                    Ok(())
                })?,
            )?;
            let c = ctx.clone();
            game.set(
                "IsEndlessDungeonBossDead",
                lua.create_function(move |_, ()| Ok(c.story.borrow().endless.boss_dead))?,
            )?;
            let c = ctx.clone();
            game.set(
                "IsEndlessDungeonBonusComplete",
                lua.create_function(move |_, ()| Ok(c.story.borrow().endless.bonus_complete))?,
            )?;
            // InActiveEndlessDungeonFloor(): the player's region is in the active floor.
            let c = ctx.clone();
            game.set(
                "InActiveEndlessDungeonFloor",
                lua.create_function(move |_, ()| {
                    let st = c.story.borrow();
                    Ok(st
                        .player
                        .region
                        .is_some_and(|r| st.endless.in_dungeon(r, true)))
                })?,
            )?;
            for (name, effect) in [
                ("RemoveEndlessShrineBuffs", Effect::RemoveEndlessShrineBuffs),
                ("RemoveEndlessSoulBuffs", Effect::RemoveEndlessSoulBuffs),
            ] {
                let c = ctx.clone();
                game.set(
                    name,
                    lua.create_function(move |_, _: MultiValue| {
                        c.story.borrow_mut().effects.push(effect.clone());
                        Ok(())
                    })?,
                )?;
            }
        }
        // Game.ScreenShake(WorldVec3, float secs, float amplitude) (`LuaGame::ScreenShake`): a type-7
        // camera-shake event, seconds converted to ms.
        {
            let c = ctx.clone();
            game.set(
                "ScreenShake",
                lua.create_function(move |_, args: MultiValue| {
                    let at = args
                        .front()
                        .and_then(get_coords)
                        .unwrap_or(Coords::at([0.0; 3]));
                    let ms = (arg_num(&args, 1) * 1000.0).max(0.0) as u32;
                    let amplitude = arg_num(&args, 2) as f32;
                    c.story
                        .borrow_mut()
                        .effects
                        .push(Effect::ScreenShake { at, ms, amplitude });
                    Ok(())
                })?,
            )?;
        }
        lua.globals().set("Game", game)
    }

    fn register_classes(&self) -> mlua::Result<()> {
        let lua = &self.lua;
        let ctx = &self.ctx;
        let g = lua.globals();
        // Methods reachable through a class table (`Character.IsAlive(obj)`).
        let names = lua.create_table()?;
        for (_, fns) in super::api::CLASS_METHODS {
            for n in fns.iter() {
                names.set(*n, true)?;
            }
        }
        let forward: Function = lua
            .load(
                r#"local names = ...
                return { __index = function(_, k)
                    if names[k] then return function(o, ...) return o[k](o, ...) end end
                end }"#,
            )
            .set_name("class_index")
            .into_function()?;
        let class_mt: Table = forward.call(names)?;
        for cls in [
            "Object",
            "Entity",
            "Actor",
            "Character",
            "Player",
            "Door",
            "Lever",
            "Chest",
            "Destructible",
            "Riftgate",
            "Shrine",
            "Npc",
            "Proxy",
            "Item",
            "AreaTrigger",
        ] {
            let t = lua.create_table()?;
            // <Class>.Get(uint id) (e.g. `LuaObject::Get`, `LuaEntity::Get`,
            // `LuaCharacter::Get`, `LuaPlayer::Get`, `LuaProxy::Get`): the object if it exists and is
            // of the class.
            let c = ctx.clone();
            t.set(
                "Get",
                lua.create_function(move |lua, id: Value| {
                    let Some(id) = value_id(&id) else {
                        return Ok(Value::Nil);
                    };
                    let ok = has_class(&c.story.borrow(), cls, id);
                    if ok {
                        c.obj_value(lua, id)
                    } else {
                        Ok(Value::Nil)
                    }
                })?,
            )?;
            // <Class>.IsType(uint id) (`LuaEntity::IsType`, `LuaCharacter::IsType`,
            // `LuaPlayer::IsType`, ...): the same class check.
            let c = ctx.clone();
            t.set(
                "IsType",
                lua.create_function(move |_, id: Value| {
                    let id = value_id(&id);
                    Ok(id.is_some_and(|id| has_class(&c.story.borrow(), cls, id)))
                })?,
            )?;
            // Create: none of the overloads places the object; `SetCoords` does.
            // * Entity.Create(record) (`LuaEntity::Create`), Item.Create, AreaTrigger.Create; the others inherit
            //   Entity.Create.
            // * Character.Create(record [, uint level, WorldVec3|nil origin]) (`LuaCharacter::CreateAtLevel`):
            //   level 0 keeps the record's limits; with an origin the host clamps the level to that region's
            //   limits. Both overloads (and `CreateNearPlayer`) enable the spawn animation.
            // * Proxy.Create(record [, WorldVec3|nil [, bool]]) (`LuaProxy::CreateAt` / `CreateAtIgnoreBoss`):
            //   the vector is `Proxy::SetLimitsLocation`, the bool `Proxy::SetIgnoreBossRestriction`.
            let c = ctx.clone();
            t.set(
                "Create",
                lua.create_function(move |lua, args: MultiValue| {
                    let rec = arg_str(&args, 0);
                    if rec.is_empty() {
                        return Ok(Value::Nil);
                    }
                    let id = c.create(&rec, None);
                    if cls == "Character" {
                        let level = arg_num(&args, 1).max(0.0) as u32;
                        let mut s = c.story.borrow_mut();
                        s.entities.spawn_animation.insert(id);
                        if level > 0 {
                            s.entities.levels.insert(id, level);
                        }
                        if let Some(origin) = args.get(2).and_then(get_coords) {
                            s.entities.origins.insert(id, origin.origin);
                        }
                    } else if cls == "Proxy" {
                        let mut s = c.story.borrow_mut();
                        if let Some(at) = args.get(1).and_then(get_coords) {
                            s.entities.proxy_limits.insert(id, at.origin);
                        }
                        if matches!(args.get(2), Some(Value::Boolean(true))) {
                            s.entities.proxy_ignore_boss.insert(id);
                        }
                    }
                    c.obj_value(lua, id)
                })?,
            )?;
            if cls == "Player" {
                // Player.Teleport(WorldVec3): a static function (`LuaPlayer::Teleport`) →
                // `GameEngine::InitiatePlayerTeleport` of the main player. PORT: floats, not integers.
                let c = ctx.clone();
                t.set(
                    "Teleport",
                    lua.create_function(move |_, v: Value| {
                        if let Some(co) = get_coords(&v) {
                            c.story
                                .borrow_mut()
                                .effects
                                .push(Effect::TeleportPlayer { coords: co });
                        }
                        Ok(())
                    })?,
                )?;
            }
            if cls == "Character" {
                // Character.CreateNearPlayer(record, uint playerId, bool aggro) (`LuaCharacter::CreateNearPlayer`:
                // nil unless the id is a player; `FindSuperBossSpawnPoint`, the bool makes the player most hated).
                // PORT: placed 2 m off the player; aggro left to the host AI.
                let c = ctx.clone();
                t.set(
                    "CreateNearPlayer",
                    lua.create_function(move |lua, args: MultiValue| {
                        let rec = arg_str(&args, 0);
                        let (me, p) = {
                            let s = c.story.borrow();
                            (s.player.id, s.player.position)
                        };
                        if rec.is_empty() || arg_id(&args, 1).is_some_and(|id| id != me) {
                            return Ok(Value::Nil);
                        }
                        let id = c.create(&rec, Some(Coords::at([p[0] + 2.0, p[1], p[2] + 2.0])));
                        c.story.borrow_mut().entities.spawn_animation.insert(id);
                        c.obj_value(lua, id)
                    })?,
                )?;
                // Character.SoundType (`LuaCharacter::BindClass`: VoxSound 0 ..
                // VoiceSound3 11).
                let st = lua.create_table()?;
                for (i, n) in [
                    "VoxSound",
                    "SpecialAttackSound1",
                    "SpecialAttackSound2",
                    "SpecialAttackSound3",
                    "SpecialAttackSound4",
                    "GenericSound1",
                    "GenericSound2",
                    "GenericSound3",
                    "GenericSound4",
                    "VoiceSound1",
                    "VoiceSound2",
                    "VoiceSound3",
                ]
                .iter()
                .enumerate()
                {
                    st.set(*n, i)?;
                }
                t.set("SoundType", st)?;
            }
            if cls == "Object" {
                // Object.Cache/Release(record) (`ObjectManager::CacheObjectEx` / `Release`): no-op.
                t.set("Cache", lua.create_function(|_, _: MultiValue| Ok(()))?)?;
                t.set("Release", lua.create_function(|_, _: MultiValue| Ok(()))?)?;
            }
            t.set_metatable(Some(class_mt.clone()))?;
            g.set(cls, t)?;
        }
        Ok(())
    }
}
