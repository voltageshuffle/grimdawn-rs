//! The object methods (`Object`, `Entity`, `Character`, `Player`, `Door`, `Proxy`, ...): Lua userdata
//! wrapping an object id.
use super::*;

impl UserData for Obj {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Eq, |_, a, b: Value| {
            Ok(matches!(&b, Value::UserData(u) if u.borrow::<Obj>().is_ok_and(|o| o.id == a.id)))
        });
        m.add_meta_method(MetaMethod::ToString, |_, a, ()| {
            Ok(format!("Object({})", a.id))
        });
        // Object (`LuaObject::GetId` / `LuaObject::GetName`): the id; the
        // object's resource (record) name, `Object::GetObjectName`.
        m.add_method("GetId", |_, o, ()| Ok(o.id));
        m.add_method("GetName", |_, o, ()| {
            let s = o.ctx.story.borrow();
            Ok(if o.id == s.player.id {
                s.player.record.clone()
            } else {
                s.entities
                    .get(o.id)
                    .map(|e| e.record.clone())
                    .unwrap_or_default()
            })
        });
        // Player.GetPlayerName (`LuaPlayer::GetPlayerName`): always returns the
        // empty wide string constant `Game::emptyWideString`.
        m.add_method("GetPlayerName", |_, _, _: MultiValue| Ok(String::new()));
        // `obj:IsType()` resolves through the class the object was fetched
        // with, which already passed that class check; the static form
        // `Class.IsType(id)` is on the class tables.
        m.add_method("IsType", |_, _, _: MultiValue| Ok(true));
        // Entity.IsReloaded (`LuaEntity::IsReloaded` → `Entity::IsRestored`,
        // Engine.dll `Entity.restored`: only `Level::RestoreState` sets it, for every
        // entity it re-creates from the saved state of a level streamed out
        // earlier this session). The host publishes those objects
        // (`HostFacts::restored_objects`).
        m.add_method("IsReloaded", |_, o, ()| {
            Ok(o.ctx
                .story
                .borrow()
                .host_facts
                .restored_objects
                .contains(&o.id))
        });
        // Entity.NetworkEnable (`LuaEntity::NetworkEnable` →
        // `Entity::SetShouldServerSpawn(true)`): multiplayer replication only.
        m.add_method("NetworkEnable", |_, _, _: MultiValue| Ok(()));
        // Entity.GetCoords (`LuaEntity::GetCoords` → `Entity::GetCoords`, a
        // WorldCoords; identity coords if gone). Actor.GetAttachedCoords(name)
        // (`LuaActor::GetAttachedCoords`) is the coords of a mesh attach point; the port has
        // no meshes here and returns the entity coords (its one stock use
        // spawns an effect at a boss).
        let get_coords_fn = |lua: &Lua, o: &Obj, _: MultiValue| {
            let s = o.ctx.story.borrow();
            let c = if o.id == s.player.id {
                Coords::at(s.player.position)
            } else {
                s.entities
                    .get(o.id)
                    .map(|e| e.coords)
                    .unwrap_or(Coords::at([0.0; 3]))
            };
            drop(s);
            coords_value(lua, &c)
        };
        m.add_method("GetCoords", get_coords_fn);
        m.add_method("GetAttachedCoords", get_coords_fn);
        // Entity.SetCoords(WorldCoords) (`LuaEntity::SetCoords`): moves an
        // entity in the world, or adds it (`World::AddEntity`) if it is not
        // in the world yet — this is how every `Create`d object gets placed.
        // PORT: the host shows an object from its `in_world` state and coords
        // when it first appears; an object it already spawned is not moved.
        m.add_method("SetCoords", |_, o, v: Value| {
            let Some(c) = get_coords(&v) else {
                return Ok(());
            };
            // A proxy placed in the world runs at once (`Proxy::RunProxy`)
            // unless its record's `delayedRun` holds it back until the
            // script's `Run()` (`Proxy::Load` sets state 1 for
            // `delayedRun`; `RunProxy` only runs in state 0): the host spawns
            // no pool for it meanwhile (`Story::is_present`).
            let delayed = {
                let s = o.ctx.story.borrow();
                s.entities
                    .get(o.id)
                    .is_some_and(|e| e.class == ObjClass::Proxy && !e.in_world)
                    && o.ctx
                        .env
                        .record_field(&s.entities.get(o.id).unwrap().record, "delayedRun")
                        .is_some_and(|v| v != "0" && !v.is_empty())
            };
            let mut s = o.ctx.story.borrow_mut();
            let Some(e) = s.entities.get_mut(o.id) else {
                return Ok(());
            };
            e.coords = c;
            // Moving an object already in the world runs nothing.
            if e.in_world {
                return Ok(());
            }
            e.in_world = true;
            let (record, class) = (e.record.clone(), e.class);
            // `World::AddEntity` runs the record's `onAddToWorld` hook with the object id
            // (as for streamed level objects, `ScriptHost::add_object`): queued, since the
            // script is still running.
            for hook in ["onAddToWorld", "onInitialUpdate"] {
                if let Some(f) = o
                    .ctx
                    .env
                    .record_field(&record, hook)
                    .filter(|f| !f.is_empty())
                {
                    s.lua_calls.push(LuaCall {
                        function: f,
                        args: vec![LuaValue::Number(o.id as f64)],
                    });
                }
            }
            if class == ObjClass::Proxy {
                if delayed {
                    s.host_facts.proxies_delayed.insert(o.id);
                    return Ok(());
                }
                s.host_facts.proxies_active.insert(o.id);
            }
            s.effects.push(Effect::Spawn {
                id: o.id,
                record,
                class,
                coords: c,
            });
            Ok(())
        });
        // Object.Destroy() (`LuaObject::Destroy`: `World::RemoveEntity` +
        // `ObjectManager::DestroyObjectEx`, so the record's
        // `onRemoveFromWorld` (`Entity::OnRemoveFromWorld`, if it was in the
        // world) then `onDestroy` (`Entity::~Entity`) run, queued since the
        // script is still running) and the Destructible overload
        // Destroy(Vec3 direction, float force, bool) (`LuaDestructible::Destroy`
        // → `QuestCommandDestroyDestructible` → `Destructible::BreakApart`:
        // the object breaks but stays a valid object; `onBroken` is only
        // fired by `Destructible::TakeAttack`). The host takes an object
        // that is gone or out of the world away (`Story::is_present`).
        m.add_method("Destroy", |_, o, args: MultiValue| {
            let mut s = o.ctx.story.borrow_mut();
            if args.is_empty() {
                if let Some(e) = s.entities.remove(o.id) {
                    // Its trigger and proxy state go with it (as `host.rs` does for a host-side removal).
                    s.triggers.retain(|t| t.id != o.id);
                    s.host_facts.proxies_active.remove(&o.id);
                    s.host_facts.proxies_delayed.remove(&o.id);
                    let hooks: &[&str] = if e.in_world {
                        &["onRemoveFromWorld", "onDestroy"]
                    } else {
                        &["onDestroy"]
                    };
                    for hook in hooks {
                        if let Some(f) = o
                            .ctx
                            .env
                            .record_field(&e.record, hook)
                            .filter(|f| !f.is_empty())
                        {
                            s.lua_calls.push(LuaCall {
                                function: f,
                                args: vec![LuaValue::Number(o.id as f64)],
                            });
                        }
                    }
                }
            } else if let Some(e) = s.entities.get_mut(o.id)
                && e.alive
            {
                e.alive = false;
                e.in_world = false;
            }
            Ok(())
        });
        // Door/Lever.Open/Close (`LuaDoor::Open` / `LuaDoor::Close` →
        // `GameEngine::QuestCommandOpenDoor(id, bool)`).
        for (name, open) in [("Open", true), ("Close", false)] {
            m.add_method(name, move |_, o, _: MultiValue| {
                let mut s = o.ctx.story.borrow_mut();
                if let Some(e) = s.entities.get_mut(o.id) {
                    e.open = open;
                }
                s.effects.push(Effect::SetDoor { id: o.id, open });
                Ok(())
            });
        }
        // SetLocked(bool) (Door/Lever `LuaDoor::SetLocked` →
        // `QuestCommandLockDoor`; Chest `LuaChest::SetLocked` → `QuestCommandLockChest`;
        // Riftgate `LuaRiftgate::SetLocked` → `StaticTeleporter::SetLocked`; Shrine
        // `LuaShrine::SetLocked` → `StaticShrine::SetLocked`).
        m.add_method("SetLocked", |_, o, locked: bool| {
            let mut s = o.ctx.story.borrow_mut();
            if let Some(e) = s.entities.get_mut(o.id) {
                e.locked = locked;
            }
            s.effects.push(Effect::SetLocked { id: o.id, locked });
            Ok(())
        });
        // IsLocked (Door `LuaDoor::IsLocked` `FixedDoor::IsLocked` /
        // FixedItemDoor vfunc, Lever `LuaLever::IsLocked`, Shrine `LuaShrine::IsLocked`) and
        // Lever.IsOpen (`LuaLever::IsOpen`: open/close state 2 or 3). The port
        // reads the state its own SetLocked/Open calls recorded.
        m.add_method("IsLocked", |_, o, ()| {
            Ok(o.ctx
                .story
                .borrow()
                .entities
                .get(o.id)
                .is_some_and(|e| e.locked))
        });
        m.add_method("IsOpen", |_, o, ()| {
            Ok(o.ctx
                .story
                .borrow()
                .entities
                .get(o.id)
                .is_some_and(|e| e.open))
        });
        // SetAutoClose(bool) (`LuaDoor::SetAutoClose` `FixedDoor::SetAutoClose`
        // / `LuaLever::SetAutoClose` `FixedLever::SetAutoClose`). Kept as a no-op: the
        // host's doors carry no auto-close state the story layer can set.
        m.add_method("SetAutoClose", |_, _, _: MultiValue| Ok(()));
        // Npc.SetAvailableForConversation(bool) (`LuaNpc::SetAvailableForConversation` →
        // `Npc::SetIsAvailabeForConversations`).
        m.add_method("SetAvailableForConversation", |_, o, v: bool| {
            if let Some(e) = o.ctx.story.borrow_mut().entities.get_mut(o.id) {
                e.available_for_conversation = v;
            }
            Ok(())
        });
        // Character.IsAlive (`LuaCharacter::IsAlive` → `Character::IsAlive`).
        m.add_method("IsAlive", |_, o, ()| {
            let s = o.ctx.story.borrow();
            Ok(o.id == s.player.id || s.entities.get(o.id).is_some_and(|e| e.alive))
        });
        // Character.Kill() (`LuaCharacter::Kill`: the controller's kill).
        m.add_method("Kill", |_, o, _: MultiValue| {
            let mut s = o.ctx.story.borrow_mut();
            if let Some(e) = s.entities.get_mut(o.id) {
                e.alive = false;
            }
            s.effects.push(Effect::Kill { id: o.id });
            Ok(())
        });
        // Character.GetLevel (`LuaCharacter::GetLevel` → `GetCharLevel`). The
        // stock scripts only call it on the player; other characters' levels
        // live in the host, so they read 1 here.
        m.add_method("GetLevel", |_, o, ()| {
            let s = o.ctx.story.borrow();
            Ok(if o.id == s.player.id {
                s.player.level
            } else {
                1
            })
        });
        // AdjustMoney/AdjustTribute(int) (`LuaPlayer::AdjustMoney` /
        // `LuaPlayer::AdjustTribute`: Add* for > 0, Subtract*(-n) for < 0).
        m.add_method("AdjustMoney", |_, o, n: f64| {
            let n = n as i32 as i64;
            let mut s = o.ctx.story.borrow_mut();
            s.player.money = (s.player.money as i64 + n).max(0) as u32;
            if n != 0 {
                s.effects.push(Effect::GiveMoney(n));
            }
            Ok(())
        });
        m.add_method("AdjustTribute", |_, o, n: f64| {
            let n = n as i32 as i64;
            let mut s = o.ctx.story.borrow_mut();
            s.adjust_tribute(n);
            if n != 0 {
                s.effects.push(Effect::GiveTribute(n));
            }
            Ok(())
        });
        // Player.InEndlessDungeon / InEndlessActiveFloor / InEndlessBossFloor (`LuaPlayer::InEndlessDungeon` ..
        // `LuaPlayer::InEndlessBossFloor`):
        // the player's region against the generator (`IsRegionInDungeon`, `IsBossRegion`).
        m.add_method("InEndlessDungeon", |_, o, _: MultiValue| {
            let s = o.ctx.story.borrow();
            Ok(s.player
                .region
                .is_some_and(|r| s.endless.in_dungeon(r, false)))
        });
        m.add_method("InEndlessActiveFloor", |_, o, _: MultiValue| {
            let s = o.ctx.story.borrow();
            Ok(s.player
                .region
                .is_some_and(|r| s.endless.in_dungeon(r, true)))
        });
        m.add_method("InEndlessBossFloor", |_, o, _: MultiValue| {
            let s = o.ctx.story.borrow();
            Ok(s.player.region.is_some_and(|r| s.endless.is_boss_region(r)))
        });
        // Player.GetSurvivalScore() (`LuaPlayer::GetSurvivalScore`: the run's current score,
        // `PlayStats.survivalScore`; 0 outside survival).
        m.add_method("GetSurvivalScore", |_, o, _: MultiValue| {
            let s = o.ctx.story.borrow();
            Ok(if s.game.survival_mode {
                s.game.survival.score as f64
            } else {
                0.0
            })
        });
        // HasTribute(uint) (`LuaPlayer::HasTribute`: current tribute >= n).
        m.add_method("HasTribute", |_, o, n: f64| {
            Ok(o.ctx.story.borrow().player.tribute as f64 >= n)
        });
        // GiveExperience(uint) (`LuaPlayer::GiveExperience`: nothing for 0, else
        // `CharacterExperienceOutbound`).
        m.add_method("GiveExperience", |_, o, n: f64| {
            let n = n as u32;
            if n == 0 {
                return Ok(());
            }
            let mut s = o.ctx.story.borrow_mut();
            s.player.experience = s.player.experience.saturating_add(n);
            s.effects.push(Effect::GiveExperience(n));
            Ok(())
        });
        // GiveLevels(uint) (`LuaPlayer::GiveLevels`: `IncrementCharLevel` n
        // times).
        m.add_method("GiveLevels", |_, o, n: f64| {
            let n = n as u32;
            if n == 0 {
                return Ok(());
            }
            let mut s = o.ctx.story.borrow_mut();
            s.player.level = s.player.level.saturating_add(n);
            s.effects.push(Effect::GiveLevels(n));
            Ok(())
        });
        // GiveSkillPoints(uint) (`LuaPlayer::GiveSkillPoints` → `AddSkillPoints`).
        m.add_method("GiveSkillPoints", |_, o, n: f64| {
            o.ctx
                .story
                .borrow_mut()
                .effects
                .push(Effect::GiveSkillPoints(n as u32));
            Ok(())
        });
        // GiveFaction/SetFaction(string, number) (`LuaPlayer::GiveFaction` /
        // `LuaPlayer::SetFaction`): unknown names do nothing; the faction is unlocked
        // first if it is not, then `FactionPack::AdjustValue` / `SetValue`.
        for (name, set) in [("GiveFaction", false), ("SetFaction", true)] {
            m.add_method(name, move |_, o, (f, n): (String, f64)| {
                let n = n as f32;
                let mut s = o.ctx.story.borrow_mut();
                let key = f.to_ascii_uppercase();
                let value = if set {
                    n
                } else {
                    s.player.factions.get(&key).copied().unwrap_or_default() + n
                };
                // (bad data: an infinite or NaN standing would poison the
                // faction; the call does nothing)
                if !value.is_finite() {
                    return Ok(());
                }
                s.player.factions.insert(key, value);
                s.effects.push(Effect::UnlockFaction(f.clone()));
                s.effects.push(Effect::Faction {
                    faction: f,
                    delta: n,
                    set,
                });
                Ok(())
            });
        }
        // UnlockFaction(string) (`LuaPlayer::UnlockFaction` →
        // `FactionPack::UnlockFaction`).
        m.add_method("UnlockFaction", |_, o, f: String| {
            o.ctx
                .story
                .borrow_mut()
                .effects
                .push(Effect::UnlockFaction(f));
            Ok(())
        });
        // Player.GetFaction(string) (`LuaPlayer::GetFaction`: 0 for an unknown
        // name, else `(int)FactionPack::GetValue`, truncated toward zero).
        m.add_method("GetFaction", |_, o, f: String| {
            Ok(o.ctx.story.borrow().player.faction(&f) as i32 as i64)
        });
        // GiveItem(record, count, bool) (`LuaPlayer::GiveItem`: `count`
        // copies via `Item::CreateItem` + give-to-character; the bool fills a
        // relic's completion — `Effect::GiveItem` has no such flag, and the
        // stock scripts always pass false).
        m.add_method("GiveItem", |_, o, args: MultiValue| {
            let (rec, n) = (
                arg_str(&args, 0),
                args.get(1).map_or(1.0, |_| arg_num(&args, 1)) as i32,
            );
            if n <= 0 {
                return Ok(());
            }
            let mut s = o.ctx.story.borrow_mut();
            s.player.add_items(&rec, n as i64);
            s.effects.push(Effect::GiveItem {
                record: rec.into_owned(),
                count: n,
            });
            Ok(())
        });
        // TakeItem(record, count, bool) (`LuaPlayer::TakeItem` →
        // `QuestCommandTakeItem(id, record, count, bool)`, which in single
        // player calls `Player::TakeItemFromStashes(record, count)` — the bool
        // is not forwarded) and Player.DestroyItem(record)
        // (`LuaPlayer::DestroyItem`: the same virtual with count 1).
        let take = |o: &Obj, rec: &str, n: i32| -> mlua::Result<()> {
            // (bad data: a count below 1 takes nothing, as `GiveItem` gives
            // nothing)
            if n <= 0 {
                return Ok(());
            }
            let mut s = o.ctx.story.borrow_mut();
            s.player.add_items(rec, -i64::from(n));
            s.effects.push(Effect::GiveItem {
                record: rec.to_owned(),
                count: -n,
            });
            Ok(())
        };
        m.add_method("TakeItem", move |_, o, args: MultiValue| {
            let n = args.get(1).map_or(1.0, |_| arg_num(&args, 1)) as i32;
            take(o, &arg_str(&args, 0), n)
        });
        m.add_method("DestroyItem", move |_, o, args: MultiValue| {
            take(o, &arg_str(&args, 0), 1)
        });
        // HasItem(record, count, bool) (`LuaPlayer::HasItem` → the Player virtual
        // `Player::GetItemCountInStashes`(record, …, bool, 0, 1, 0) >= count:
        // [`crate::story::Player::item_count_in_stashes`], the bool its
        // complete-only filter).
        m.add_method("HasItem", |_, o, args: MultiValue| {
            let (rec, n) = (
                arg_str(&args, 0),
                args.get(1).map_or(1.0, |_| arg_num(&args, 1)),
            );
            let complete = matches!(args.get(2), Some(Value::Boolean(true)));
            Ok(o.ctx
                .story
                .borrow()
                .player
                .item_count_in_stashes(&rec, complete) as f64
                >= n)
        });
        // Player.IsSpaceAvailable(record) (`LuaPlayer::IsSpaceAvailable`: creates the
        // item, false when it can't, and asks `Player::
        // IsInventorySpaceAvailable` → `PlayerInventoryCtrl::IsSpaceAvailable`:
        // per sack, a not-full stack of the item
        // (`InventorySack::GetNotFullItem`) or a free rect of its bitmap
        // size). The host mirrors its bags (`Player::bags`).
        m.add_method("IsSpaceAvailable", |_, o, args: MultiValue| {
            let rec = arg_str(&args, 0);
            let Some(size) = o.ctx.env.item_footprint(&rec) else {
                return Ok(false);
            };
            Ok(o.ctx.story.borrow().player.bags.iter().any(|b| {
                b.items
                    .iter()
                    .any(|p| p.item.stack_room && p.item.record.eq_ignore_ascii_case(&rec))
                    || b.find_position(size).is_some()
            }))
        });
        // Player.HasToken(string) (`LuaPlayer::HasToken` →
        // `Player::HasToken`); AnyoneHasToken / ServerHasToken (both
        // `LuaPlayer::AnyoneHasToken` → `Quest2Repository::AnyoneHasToken`, the same set as
        // the one player's in single player).
        m.add_method("HasToken", |_, o, t: String| {
            Ok(o.ctx.story.borrow().player.tokens.contains(&t))
        });
        m.add_method("AnyoneHasToken", |_, o, t: String| {
            Ok(o.ctx.story.borrow().player.tokens.contains(&t))
        });
        m.add_method("ServerHasToken", |_, o, t: String| {
            Ok(o.ctx.story.borrow().player.tokens.contains(&t))
        });
        // GiveToken/RemoveToken(string) (`LuaPlayer::GiveToken` /
        // `LuaPlayer::RemoveToken`: `Player::BestowToken` / `RemoveToken` on the local
        // player, whatever `self` is).
        m.add_method("GiveToken", |_, o, t: String| {
            o.ctx.story.borrow_mut().give_token(&t);
            Ok(())
        });
        m.add_method("RemoveToken", |_, o, t: String| {
            o.ctx.story.borrow_mut().remove_token(&t);
            Ok(())
        });
        // GetQuestState(uint quest) (`LuaPlayer::GetQuestState`) and
        // GetQuestTaskState(uint quest, uint task) (`LuaPlayer::GetQuestTaskState`);
        // the state rules are in `QuestLog::lua_quest_state` / `lua_task_state`.
        m.add_method("GetQuestState", |_, o, q: f64| {
            Ok(o.ctx.story.borrow().quests.lua_quest_state(q as u32))
        });
        m.add_method("GetQuestTaskState", |_, o, (q, t): (f64, f64)| {
            Ok(o.ctx
                .story
                .borrow()
                .quests
                .lua_task_state(q as u32, t as u32))
        });
        // GrantQuest/CompleteQuest(uint quest, uint task) (
        // `LuaPlayer::GrantQuest` → `Quest2Repository::BeginQuestTask(file, task, 0)`,
        // `LuaPlayer::CompleteQuest` → `CompleteQuestTask(file, task)`; unknown quest: no-op).
        m.add_method("GrantQuest", |_, o, (q, t): (f64, f64)| {
            let mut s = o.ctx.story.borrow_mut();
            if let Some(qi) = s.quests.defs.by_id(q as u32) {
                s.begin_task(qi, t as u32, false);
            }
            Ok(())
        });
        m.add_method("CompleteQuest", |_, o, (q, t): (f64, f64)| {
            let mut s = o.ctx.story.borrow_mut();
            if let Some(qi) = s.quests.defs.by_id(q as u32) {
                s.complete_task(qi, t as u32);
            }
            Ok(())
        });
        // Player.HasMastery(int) (`LuaPlayer::HasMastery`: the id is in
        // `Character::GetSkillMasteries`).
        m.add_method("HasMastery", |_, o, n: f64| {
            Ok(o.ctx.story.borrow().player.masteries.contains(&(n as u32)))
        });
        // Player.HasBuff(skill record) (`LuaPlayer::HasBuff`:
        // `IsSkillBuffActive || IsSkillToggled`).
        m.add_method("HasBuff", |_, o, b: String| {
            let b = b.to_ascii_lowercase();
            let s = o.ctx.story.borrow();
            Ok(s.player.buffs.contains(&b) || s.player.toggled_skills.contains(&b))
        });
        // Character.TeleportHome() (`LuaCharacter::TeleportHome`: the Character's
        // virtual SetCoords(`Character::GetSpawnPoint`), then the monster
        // controller's `AngerManager::Clear` and `ControllerAI::GoToIdleState`).
        m.add_method("TeleportHome", |_, o, _: MultiValue| {
            let mut s = o.ctx.story.borrow_mut();
            if s.entities.get(o.id).is_some_and(|e| e.class.is_character()) {
                s.effects.push(Effect::TeleportHome { id: o.id });
            }
            Ok(())
        });
        // Character.MoveAction(uint target, WorldCoords, bool) (
        // `LuaCharacter::MoveAction` → `GameEngine::QuestCommandMove` → `Character::
        // QuestCommandMove` → `ControllerAI::QuestCommandMove` (stores the
        // bool, then the controller's state response `ControllerMonsterState::
        // DefaultQuestCommandMoveResponse`: the destination is the target
        // entity's coords if that object exists, else the given coords; if
        // the monster can path there it enters the "QuestMove" state). The
        // bool is the controller's `AllowQuestInterruptions` (read by
        // `ControllerMonsterStateQuestMove`: enemy/attack/low-health events
        // are answered only when it is set).
        m.add_method("MoveAction", |_, o, args: MultiValue| {
            let target = arg_id(&args, 0).unwrap_or(0);
            let coords = args
                .get(1)
                .and_then(get_coords)
                .unwrap_or(Coords::at([0.0; 3]));
            let interruptible = matches!(args.get(2), Some(Value::Boolean(true)));
            let mut s = o.ctx.story.borrow_mut();
            if s.entities.get(o.id).is_some_and(|e| e.class.is_character()) {
                s.effects.push(Effect::MoveAction {
                    id: o.id,
                    target,
                    coords,
                    interruptible,
                });
            }
            Ok(())
        });
        // Character.UseSkillAction(string skill, uint target, bool) (
        // `LuaCharacter::UseSkillAction` → `GameEngine::QuestCommandUseSkill` → `Character::
        // QuestCommandUseSkill`: `SkillManager::FindSkillId(name)` among the
        // character's own skills, then `ControllerAI::QuestCommandUseSkill`
        // → the "QuestUseSkill" state; `OnBegin` uses the target Character's
        // coords, any other target id makes the monster use the skill on
        // itself at its own coords). The bool is stored in the same
        // controller flag, which the "QuestUseSkill" state never reads.
        m.add_method("UseSkillAction", |_, o, args: MultiValue| {
            let skill = arg_str(&args, 0);
            let target = arg_id(&args, 1).unwrap_or(0);
            let mut s = o.ctx.story.borrow_mut();
            if !skill.is_empty() && s.entities.get(o.id).is_some_and(|e| e.class.is_character()) {
                s.effects.push(Effect::UseSkill {
                    id: o.id,
                    skill: skill.into_owned(),
                    target,
                });
            }
            Ok(())
        });
        // Shrine.IsRestored() (`LuaShrine::IsRestored` → `StaticShrine::
        // IsCleansed`, state 6; false when the id is not a StaticShrine).
        // The host publishes the restored shrines' uids (`HostFacts`).
        m.add_method("IsRestored", |_, o, _: MultiValue| {
            let s = o.ctx.story.borrow();
            let is_shrine = s
                .entities
                .get(o.id)
                .is_some_and(|e| e.class == ObjClass::Shrine);
            Ok(is_shrine
                && s.host_facts
                    .object_uids
                    .get(&o.id)
                    .is_some_and(|u| s.host_facts.restored_shrines.contains(u)))
        });
        // AreaTrigger.DestroyItems() (`LuaAreaTrigger::DestroyItems`: for every id of
        // `AreaTrigger::GetItems` that is an Entity: `World::RemoveEntity`
        // + `ObjectManager::DestroyObjectEx`, replicated to clients).
        m.add_method("DestroyItems", |_, o, _: MultiValue| {
            let mut s = o.ctx.story.borrow_mut();
            if s.entities
                .get(o.id)
                .is_some_and(|e| e.class == ObjClass::AreaTrigger)
            {
                s.effects.push(Effect::DestroyItems { trigger: o.id });
            }
            Ok(())
        });
        // Proxy.LinkPatrolPointGroup(name) (`LuaProxy::LinkPatrolPointGroup`): the
        // name must be an instance group of type "Patrol Points" (else an
        // error is logged and nothing changes); the host does the group
        // bookkeeping and re-points the proxy's spawned monsters
        // (`Monster::SetPatrolPoints` + `ControllerMonster::ReplacePatrolPoints`).
        // The proxy must exist (the Lua `Proxy.Get` object is a Proxy).
        m.add_method("LinkPatrolPointGroup", |_, o, args: MultiValue| {
            let mut s = o.ctx.story.borrow_mut();
            if s.entities
                .get(o.id)
                .is_some_and(|e| e.class == ObjClass::Proxy)
            {
                let group = arg_str(&args, 0);
                s.effects.push(Effect::LinkPatrolPointGroup {
                    proxy: o.id,
                    group: group.into_owned(),
                });
            }
            Ok(())
        });
        // Proxy.AllKilled() (`LuaProxy::AllKilled`: true when the proxy
        // does not exist; else false if any id of the proxy's spawned list
        // (the Proxy virtual `Proxy::GetSpawnedIds`) is a living Monster, `Character::IsAlive`
        // virtual). `HostFacts::proxies_active` is that list:
        // PORT: a run proxy's monsters spawn a few frames after it is placed
        // (once nav data is in), so it counts as alive until the host says
        // its spawned monsters are dead; a proxy held back by `delayedRun`
        // has no spawned list yet, so it answers true.
        m.add_method("AllKilled", |_, o, _: MultiValue| {
            let s = o.ctx.story.borrow();
            Ok(!s.host_facts.proxies_active.contains(&o.id))
        });
        // Proxy.Run() (`LuaProxy::Run` → `Proxy::DelayedRun`: only a
        // proxy in state 1 (`delayedRun`) is released, state 0, then
        // `Proxy::RunProxy` spawns its pool).
        m.add_method("Run", |_, o, _: MultiValue| {
            let mut s = o.ctx.story.borrow_mut();
            if s.host_facts.proxies_delayed.remove(&o.id)
                && let Some(e) = s.entities.get(o.id).cloned()
            {
                s.host_facts.proxies_active.insert(o.id);
                s.effects.push(Effect::Spawn {
                    id: o.id,
                    record: e.record,
                    class: e.class,
                    coords: e.coords,
                });
            }
            Ok(())
        });
        // Proxy.IsAmbush() (`LuaProxy::IsAmbush`: the proxy's class is
        // `ProxyAmbush` or derives from it).
        m.add_method("IsAmbush", |_, o, _: MultiValue| {
            let rec = o
                .ctx
                .story
                .borrow()
                .entities
                .get(o.id)
                .map(|e| e.record.clone());
            Ok(rec
                .and_then(|r| o.ctx.env.record_field(&r, "Class"))
                .is_some_and(|c| c.eq_ignore_ascii_case("ProxyAmbush")))
        });
        // Stubs (why each is not implemented):
        // * Character.Attack(uint target) `LuaCharacter::Attack` `RequestAttack`,
        //   Move(WorldVec3) `LuaCharacter::Move`, Turn(Vec3) `LuaCharacter::Turn`,
        //   PlayAnimation(uint, bool, bool) `LuaCharacter::PlayAnimation`, ToggleSkills(bool)
        //   `LuaCharacter::ToggleSkills`, ToggleInvincible(bool) `LuaCharacter::ToggleInvincible`: no stock
        //   script calls them.
        // * PlaySound(uint SoundType) `LuaCharacter::PlaySound` and Say(tag) `LuaCharacter::Say`
        //   (speech bubble): host audio/UI; no stock script calls them.
        // * GiveAffixItem(record, prefix, suffix) `LuaPlayer::GiveAffixItem`: unused.
        // * Item.HasAffix(record) `LuaItem::HasAffix`: unused by the scripts.
        for (name, ret) in [
            ("Attack", 0),
            ("Move", 0),
            ("Turn", 0),
            ("PlaySound", 0),
            ("Say", 0),
            ("PlayAnimation", 0),
            ("ToggleSkills", 0),
            ("ToggleInvincible", 0),
            ("GiveAffixItem", 0),
            ("HasAffix", 2),
        ] {
            m.add_method(name, move |_, o, _: MultiValue| {
                o.ctx
                    .story
                    .borrow_mut()
                    .unimplemented(&format!("Lua method :{name}"));
                Ok(match ret {
                    1 => Value::Boolean(true),
                    2 => Value::Boolean(false),
                    3 => Value::Integer(0),
                    _ => Value::Nil,
                })
            });
        }
    }
}
