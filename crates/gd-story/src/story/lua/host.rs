//! The host's entry points: the frame update, world objects coming and going, record hooks and conversations.
use super::*;

impl ScriptHost {
    fn eval_lua_condition(&self, function: &str, args: &[LuaValue]) -> LuaRet {
        let args = self.lua_values(args);
        match self.call(function, args).and_then(|v| v.into_iter().next()) {
            Some(Value::Boolean(b)) => LuaRet::Bool(b),
            Some(Value::Integer(n)) => LuaRet::Number(n as f64),
            Some(Value::Number(n)) => LuaRet::Number(n),
            Some(Value::String(s)) => LuaRet::Str(s.to_string_lossy()),
            _ => LuaRet::Nil,
        }
    }

    /// Evaluates `conds` into `Story::lua_results`.
    fn eval_lua_conds(&self, conds: &[std::sync::Arc<LuaCond>]) {
        for c in conds {
            let r = self.eval_lua_condition(&c.function, &c.args);
            self.ctx.story.borrow_mut().lua_results.insert(c.key, r);
        }
    }

    /// Pre-evaluates the Lua conditions the quest update may read.
    pub fn eval_quest_lua_conditions(&self) {
        let pending = self.ctx.story.borrow().pending_lua_conditions();
        self.eval_lua_conds(&pending);
    }

    /// Refreshes the Lua conditions of the quest events that can run now (AVAILABLE tasks'
    /// incentives, IN_PROGRESS tasks' rewards) so a begin/complete reached from this entry point
    /// reads live results, not the 500 ms tick's (quest module doc PORT).
    pub(super) fn eval_event_lua_conditions(&self) {
        let conds = self.ctx.story.borrow().event_lua_conditions();
        self.eval_lua_conds(&conds);
    }

    /// One frame: quests (with Lua conditions), update registrations, area
    /// triggers against the player, queued Lua actions; all in one budget
    /// scope (PORT, see `INSTRUCTION_BUDGET`).
    pub fn update(&self, dt_ms: i32) {
        let _scope = self.ctx.enter_scope();
        {
            let mut s = self.ctx.story.borrow_mut();
            s.game.time_ms += dt_ms.max(0) as u64;
        }
        if self.ctx.story.borrow().quests.timer_ms - dt_ms <= 0 {
            self.eval_quest_lua_conditions();
        }
        self.ctx.story.borrow_mut().update_quests(dt_ms);
        self.run_queued();

        let mut due = std::mem::take(&mut *self.ctx.due.borrow_mut());
        // `LuaManager::Update` (Engine.dll): a positive countdown
        // drops by the frame time; at <= 0 the function runs with the
        // object id and the countdown restarts at the period.
        for r in self.ctx.updates.borrow_mut().iter_mut() {
            if r.timer > 0 {
                r.timer -= dt_ms;
            }
            if r.timer <= 0 {
                r.timer = r.period;
                due.push((r.id, r.function.clone(), r.func.clone()));
            }
        }
        if !due.is_empty() {
            self.eval_event_lua_conditions();
        }
        for (id, name, f) in due.drain(..) {
            self.call_function(&name, &f, id);
        }
        *self.ctx.due.borrow_mut() = due;

        let mut calls = std::mem::take(&mut *self.ctx.calls.borrow_mut());
        let hooked = self
            .ctx
            .story
            .borrow_mut()
            .update_triggers(dt_ms, &mut calls);
        for h in hooked {
            self.eval_event_lua_conditions();
            // `shouldTrigger` (`AreaTrigger::ShouldTrigger` → `AreaTrigger::CallHook`):
            // called with the entity's object id, accepted only on a boolean true; a Lua error, a
            // missing function or any other result rejects (the engine logs the error, as `call` does).
            let accepted = h
                .inside
                .into_iter()
                .filter(|&id| {
                    matches!(
                        self.call(&h.function, id)
                            .and_then(|v| v.into_iter().next()),
                        Some(Value::Boolean(true))
                    )
                })
                .collect();
            self.ctx
                .story
                .borrow_mut()
                .fire_trigger(h.trigger, accepted, &mut calls);
        }
        if !calls.is_empty() {
            self.eval_event_lua_conditions();
        }
        for c in calls.drain(..) {
            self.call(&c.function, c.entity);
        }
        *self.ctx.calls.borrow_mut() = calls;
        self.run_queued();
    }

    /// A world object came into existence (level streamed in or host spawn):
    /// registers it (class/conversation from its record, triggers for
    /// `BoxTrigger`s) and calls its `onAddToWorld` hook with the object id.
    pub fn add_object(&self, id: u32, record: &str, coords: Coords) {
        let env = &self.ctx.env;
        let rec = record.to_ascii_lowercase();
        let class = self.ctx.class_of(&rec);
        let conversation = env
            .record_field(&rec, "conversation")
            .filter(|s| !s.is_empty());
        // The object's own lock / open state starts from its record, as the
        // host's object does (`IsLocked` / `IsOpen` read it; doors spawned
        // after a script's `Open` / `SetLocked` take it from here). A
        // `FixedItemDoor` reads no `open` (`gd_game::doors`).
        let flag = |k: &str| env.record_field(&rec, k).is_some_and(|v| v.trim() == "1");
        let item_door = env.record_field(&rec, "Class").as_deref() == Some("FixedItemDoor");
        self.ctx.story.borrow_mut().entities.insert(WorldObject {
            id,
            record: rec.clone(),
            class,
            coords,
            in_world: true,
            alive: true,
            locked: flag("locked"),
            open: !item_door && flag("open"),
            broken: false,
            conversation,
            available_for_conversation: true,
            scripted: false,
        });
        // A level-placed proxy publishes its state like a script-placed one
        // (`Proxy.SetCoords`, `obj.rs`): `delayedRun` holds it back until
        // `Proxy:Run()`, else it is active — its pool spawns and `AllKilled`
        // is false until the host reports its monsters dead.
        if class == ObjClass::Proxy {
            let mut s = self.ctx.story.borrow_mut();
            if env
                .record_field(&rec, "delayedRun")
                .is_some_and(|v| v != "0" && !v.is_empty())
            {
                s.host_facts.proxies_delayed.insert(id);
            } else {
                s.host_facts.proxies_active.insert(id);
            }
        }
        if class == ObjClass::AreaTrigger {
            let get = |k: &str| env.record_field(&rec, k);
            let t = AreaTrigger::from_record(id, &rec, coords, &get);
            self.ctx.story.borrow_mut().triggers.push(t);
        }
        // `onAddToWorld`, then `onInitialUpdate` (the locked Korvan rifts
        // unlock and open there). Engine.dll `Entity::LoadScriptHooks` (the
        // entity hook pack's loader): event 0 `onAddToWorld`, 2
        // `onInitialUpdate`. PORT: the engine fires the latter on the
        // object's first update (caller not located); here right after.
        for hook in ["onAddToWorld", "onInitialUpdate"] {
            if let Some(f) = env.record_field(&rec, hook).filter(|s| !s.is_empty()) {
                self.eval_event_lua_conditions();
                self.call(&f, id);
            }
        }
        self.run_queued();
    }

    /// Streamed out: the record's `onRemoveFromWorld` then `onDestroy` hooks, then forget it. Engine.dll
    /// `Region::UnloadLevel` → `Level::UnloadEntities` (each entity's `OnUnload`: `onRemoveFromWorld`), then
    /// `Level::Unload` → `Level::DestroyAllEntities` (`DestroyObjectEx` → `Entity::~Entity`: `onDestroy`).
    pub fn remove_object(&self, id: u32) {
        let rec = self
            .ctx
            .story
            .borrow()
            .entities
            .get(id)
            .map(|e| e.record.clone());
        if let Some(rec) = rec {
            for hook in ["onRemoveFromWorld", "onDestroy"] {
                if let Some(f) = self
                    .ctx
                    .env
                    .record_field(&rec, hook)
                    .filter(|s| !s.is_empty())
                {
                    self.eval_event_lua_conditions();
                    self.call(&f, id);
                }
            }
        }
        let mut s = self.ctx.story.borrow_mut();
        s.entities.remove(id);
        s.triggers.retain(|t| t.id != id);
        s.host_facts.proxies_active.remove(&id);
        s.host_facts.proxies_delayed.remove(&id);
    }

    /// Calls an object's record hook (`onInteract`, `onDie`, `onOpen`, ...) with the object id.
    pub fn object_hook(&self, id: u32, hook: &str) {
        self.object_hook_arg(id, hook, id);
    }

    /// Calls object `id`'s record hook with `arg` (a proxy's `onSpawn` gets the spawned entity's id).
    pub fn object_hook_arg(&self, id: u32, hook: &str, arg: u32) {
        let rec = self
            .ctx
            .story
            .borrow()
            .entities
            .get(id)
            .map(|e| e.record.clone());
        if let Some(rec) = rec
            && let Some(f) = self
                .ctx
                .env
                .record_field(&rec, hook)
                .filter(|s| !s.is_empty())
        {
            self.eval_event_lua_conditions();
            self.call(&f, arg);
            self.run_queued();
        }
    }

    /// A use of a door or lever: its `onInteract` hook with (object id, user
    /// id). Returns whether the hook returned boolean `true`, which makes the
    /// use stop there (`FixedDoor::RequestToUse` /
    /// `FixedLever::RequestToUse`, Game.dll, via
    /// `Entity::RunUseHook`: the hook runs first on every use, two arguments, one
    /// result, "handled" only for a boolean true).
    pub fn interact_hook(&self, id: u32) -> bool {
        let (rec, user) = {
            let s = self.ctx.story.borrow();
            (s.entities.get(id).map(|e| e.record.clone()), s.player.id)
        };
        let Some(f) = rec
            .and_then(|rec| self.ctx.env.record_field(&rec, "onInteract"))
            .filter(|s| !s.is_empty())
        else {
            return false;
        };
        self.eval_event_lua_conditions();
        let handled = self
            .call(&f, (id, user))
            .is_some_and(|v| matches!(v.into_iter().next(), Some(Value::Boolean(true))));
        self.run_queued();
        handled
    }

    /// A creature died: kill counters + its `onDie` hook.
    pub fn on_kill(&self, id: u32) {
        let rec = self
            .ctx
            .story
            .borrow()
            .entities
            .get(id)
            .map(|e| e.record.clone());
        if let Some(rec) = rec {
            self.ctx
                .story
                .borrow_mut()
                .on_kill(&crate::story::Kill::record_only(&rec));
            self.on_died(id);
        }
    }

    /// Only the object's `onDie` hook (a host that counts every creature
    /// kill with `Story::on_kill` itself).
    pub fn on_died(&self, id: u32) {
        self.object_hook(id, "onDie");
    }

    fn eval_conversation_lua(&self, npc: u32) {
        let conds = {
            let s = self.ctx.story.borrow();
            let Some(path) = s.entities.get(npc).and_then(|e| e.conversation.as_deref()) else {
                return;
            };
            s.dialogs.lua_conds(path).map(<[_]>::to_vec)
        };
        if let Some(conds) = conds {
            self.eval_lua_conds(&conds);
        }
    }

    /// Player interacts with an NPC: Lua conditions of its conversation are
    /// evaluated, then the first available speech is shown.
    pub fn talk(&self, npc: u32) -> Option<crate::story::DialogView> {
        self.eval_conversation_lua(npc);
        self.eval_event_lua_conditions();
        let v = self.ctx.story.borrow_mut().talk(npc);
        self.run_queued();
        v
    }

    pub fn choose(&self, response: u32) -> Option<crate::story::DialogView> {
        let npc = self
            .ctx
            .story
            .borrow()
            .dialogs
            .session
            .as_ref()
            .map(|s| s.npc);
        if let Some(npc) = npc {
            self.eval_conversation_lua(npc);
        }
        self.eval_event_lua_conditions();
        let v = self.ctx.story.borrow_mut().choose(response);
        self.run_queued();
        v
    }
}
