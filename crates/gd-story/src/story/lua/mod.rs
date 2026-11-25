//! Lua 5.1 host for the game's scripts (`Scripts.arc`: `scripts/main.lua` →
//! `libs/`, `game/`). Per binding:
//!
//! * Registration: `LuaGlue::RegisterAll` calls one `Lua<Class>::BindClass` per class
//!   (Vec, Coords, WorldCoords, Object, Entity, Actor, Character, Player, Proxy, Door,
//!   Lever, Destructible, Chest, Riftgate, Shrine, Npc, Item, AreaTrigger) and static
//!   table (Time, UI, Shared, Game + `Script.Event`, MpScript).
//!   `Script.Load/RegisterForUpdate/UnregisterForUpdate` are bound in Engine.dll
//!   `LuaScript::BindClass`; the exe also binds `Debug.AddStatisticText`.
//! * Each class table is the metatable of its objects (`__index` = itself,
//!   `__base` = parent class), so methods are also reachable as
//!   `Character.IsAlive(obj)` / `Player.HasMastery(player, id)`, which the
//!   stock scripts use; the port forwards such calls to the object method.
//! * Argument shapes: each binding's overload matcher (`LuaGlueFunction::MatchArgs`, e.g.
//!   `LuaPlayer::MatchHasItemArgs`) requires the exact argument count and Lua types; a call
//!   that matches no overload is a Lua error. PORT: the port accepts the same shapes and is
//!   lenient beyond them (missing counts default to 1).
//! * Objects are `{0, objectId}` userdata; `Class.Get(id)` returns nil unless
//!   the object exists and derives from that class (classInfo walk).
//!
//! The comment on each binding names the exe thunk and the Game.dll function it forwards
//! to. `Debug.AddStatisticText` is a no-op (no statistics display). Engine quirks are
//! preserved: `Character.Turn` is a no-op (the glue passes ms=0 to
//! `Character::RotateTowards`); `Player.GetPlayerName` returns ""; `Character.PlaySound`'s
//! chance roll matches (`Character::PlayAnimationSound`). `Object.Cache/Release`,
//! `Entity.NetworkEnable`, `Character.Say` and `Game.ScreenShake` are headless no-ops.
//! `Entity.GetAttachedCoords` reads host-published attach points
//! (`Entities::attach_coords`), else the entity coords.
//!
//! PORT: the VM is sandboxed (`ScriptHost::new`): no `os` / `io` / `package`
//! and no `dofile` / `loadfile` / `require`, a memory limit, and an instruction
//! budget per top-level call, script load or frame (`update` / `run_queued`).
//!
//! Coords are plain tables `{origin = Vec, xAxis, yAxis, zAxis}` and `Vec` is a small
//! Lua class (the scripts' `libs/vector.lua` adds methods to it).
//!
//! PORT: an update registration keeps the `mlua::Function` resolved at registration
//! (`Script.RegisterForUpdate`); the engine re-resolves the name from the globals every period,
//! so a script that reassigns the global after registering keeps the old function.
//!
//! Layout: `api` (the object methods the class tables forward), `obj` (object methods), `game` (global and
//! class tables), `host` (the host's entry points: frame update, world objects, hooks, conversations).
mod api;
mod game;
mod host;
mod obj;

use super::Story;
use super::quest::{LuaCall, LuaCond, LuaRet, LuaValue};
use super::state::{Coords, Effect, ObjClass, WorldObject};
use super::syntax::to_lua51;
use super::trigger::{AreaTrigger, TriggerCall};
use gd_formats::collections::HashSet;
use mlua::{
    Function, HookTriggers, IntoLua, Lua, LuaOptions, MetaMethod, MultiValue, StdLib, Table,
    UserData, UserDataMethods, Value, Variadic, VmState,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub trait ScriptEnv {
    /// Resource bytes (`scripts/...lua`), lower-case `/` path.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    /// A database record field (`Class`, `onAddToWorld`, `extentsX`, ...).
    fn record_field(&self, record: &str, field: &str) -> Option<String>;
    fn localize(&self, _tag: &str) -> Option<String> {
        None
    }
    /// The game database (the Shattered Realm generator reads its rulesets and floors).
    fn database(&self) -> Option<&gd_formats::vfs::Database> {
        None
    }
    /// The bag footprint of a new item of `record`; None when no item can be made of the record.
    fn item_footprint(&self, _record: &str) -> Option<gd_formats::inventory::Footprint> {
        None
    }
}

type Shared = Rc<RefCell<Story>>;

struct Ctx {
    story: Shared,
    env: Rc<dyn ScriptEnv>,
    updates: RefCell<Vec<UpdateReg>>,
    /// Normalised paths `Script.Load` has run (or tried to run).
    loaded: RefCell<HashSet<String>>,
    /// Remaining instruction-budget hook ticks of the open budget scope.
    budget: Cell<u32>,
    /// How many budget scopes are open (bindings can call back into Lua).
    depth: Cell<u32>,
    /// [`ScriptHost::update`]'s due registrations (reused): id, name, resolved function.
    due: RefCell<Vec<(u32, Rc<str>, Function)>>,
    calls: RefCell<Vec<TriggerCall>>,
}

/// PORT: the engine has no instruction limit. The outermost budget scope ([`Ctx::enter_scope`])
/// gets this many instructions, shared by everything inside it.
const INSTRUCTION_BUDGET: u32 = 10_000_000;
/// How often the budget hook runs (instructions per tick).
const HOOK_INTERVAL: u32 = 10_000;
/// PORT: the engine's Lua state has no memory limit.
const MEMORY_LIMIT: usize = 64 << 20;
/// PORT: `LuaManager::RegisterForUpdates` appends without a limit.
const MAX_UPDATE_REGS: usize = 1024;
/// PORT: the queue of Lua calls is a port construct; one drain runs at most this many and drops
/// the rest.
const MAX_QUEUED_CALLS: usize = 4096;
/// Rounds of one [`ScriptHost::run_queued`] drain (calls queue more calls).
const QUEUE_ROUNDS: usize = 64;

struct ScopeGuard(Rc<Ctx>);

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        self.0.depth.set(self.0.depth.get() - 1);
    }
}

impl Ctx {
    /// Opens a budget scope: the outermost one starts with a fresh budget,
    /// nested ones share it.
    fn enter_scope(self: &Rc<Self>) -> ScopeGuard {
        if self.depth.get() == 0 {
            self.budget.set(INSTRUCTION_BUDGET / HOOK_INTERVAL);
        }
        self.depth.set(self.depth.get() + 1);
        ScopeGuard(self.clone())
    }
}

struct UpdateReg {
    id: u32,
    /// The registered name (`Script.UnregisterForUpdate` matches it).
    function: Rc<str>,
    /// The function resolved at registration (PORT, module doc).
    func: Function,
    period: i32,
    timer: i32,
}

#[derive(Clone)]
struct Obj {
    id: u32,
    ctx: Rc<Ctx>,
}

const PRELUDE: &str = r#"
Vec = {}
Vec.__index = Vec
setmetatable(Vec, { __call = function(_, x, y, z) return setmetatable({ x = x or 0, y = y or 0, z = z or 0 }, Vec) end })
Vec.__add = function(a, b) return Vec(a.x + b.x, a.y + b.y, a.z + b.z) end
Vec.__sub = function(a, b) return Vec(a.x - b.x, a.y - b.y, a.z - b.z) end
Vec.__mul = function(a, b)
    if type(a) == "number" then return Vec(b.x * a, b.y * a, b.z * a) end
    if type(b) == "number" then return Vec(a.x * b, a.y * b, a.z * b) end
    return Vec(a.x * b.x, a.y * b.y, a.z * b.z)
end
Vec.__unm = function(a) return Vec(-a.x, -a.y, -a.z) end
Vector = Vec
WorldVec = Vec
__gd_vec = function(x, y, z) return Vec(x, y, z) end
"#;

fn get_vec(v: &Value) -> Option<[f32; 3]> {
    let t = v.as_table()?;
    let n = |k: &str| t.get::<f64>(k).ok().map(|x| x as f32);
    Some([n("x")?, n("y")?, n("z")?])
}

fn get_coords(v: &Value) -> Option<Coords> {
    let t = v.as_table()?;
    if let Ok(o) = t.get::<Value>("origin")
        && let Some(origin) = get_vec(&o)
    {
        let mut c = Coords::at(origin);
        for (i, k) in ["xAxis", "yAxis", "zAxis"].iter().enumerate() {
            if let Some(a) = t.get::<Value>(*k).ok().as_ref().and_then(get_vec) {
                c.axes[i] = a;
            }
        }
        return Some(c);
    }
    get_vec(v).map(Coords::at)
}

fn vec_value(lua: &Lua, v: [f32; 3]) -> mlua::Result<Value> {
    let ctor: Function = lua.globals().get("__gd_vec")?;
    ctor.call((v[0], v[1], v[2]))
}

fn coords_value(lua: &Lua, c: &Coords) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("origin", vec_value(lua, c.origin)?)?;
    t.set("xAxis", vec_value(lua, c.axes[0])?)?;
    t.set("yAxis", vec_value(lua, c.axes[1])?)?;
    t.set("zAxis", vec_value(lua, c.axes[2])?)?;
    Ok(t)
}

/// A binding's string argument: borrowed when valid UTF-8, else an owned conversion.
enum ArgStr {
    Lua(mlua::BorrowedStr),
    Owned(String),
}

impl ArgStr {
    fn into_owned(self) -> String {
        match self {
            Self::Lua(s) => s.to_string(),
            Self::Owned(s) => s,
        }
    }
}

impl std::ops::Deref for ArgStr {
    type Target = str;
    fn deref(&self) -> &str {
        match self {
            Self::Lua(s) => s,
            Self::Owned(s) => s,
        }
    }
}

fn arg_str(args: &MultiValue, i: usize) -> ArgStr {
    match args.get(i) {
        Some(Value::String(s)) => match s.to_str() {
            Ok(s) => ArgStr::Lua(s),
            Err(_) => ArgStr::Owned(s.to_string_lossy()),
        },
        Some(Value::Integer(n)) => ArgStr::Owned(n.to_string()),
        Some(Value::Number(n)) => ArgStr::Owned(n.to_string()),
        _ => ArgStr::Owned(String::new()),
    }
}

fn arg_num(args: &MultiValue, i: usize) -> f64 {
    match args.get(i) {
        Some(Value::Integer(n)) => *n as f64,
        Some(Value::Number(n)) => *n,
        Some(Value::Boolean(b)) => *b as i32 as f64,
        _ => 0.0,
    }
}

fn arg_id(args: &MultiValue, i: usize) -> Option<u32> {
    args.get(i).and_then(value_id)
}

/// An object id argument: a number, or an object.
fn value_id(v: &Value) -> Option<u32> {
    match v {
        Value::Integer(n) => Some(*n as u32),
        Value::Number(n) => Some(*n as u32),
        Value::UserData(u) => u.borrow::<Obj>().ok().map(|o| o.id),
        _ => None,
    }
}

/// Whether object `id` passes `<cls>.Get`/`IsType`'s classInfo check (`LuaEntity::Get`).
/// Objects the port could not classify (`ObjClass::Entity`/`ScriptEntity`) pass every
/// non-character check.
fn has_class(s: &Story, cls: &str, id: u32) -> bool {
    if id == s.player.id {
        return matches!(cls, "Object" | "Entity" | "Actor" | "Character" | "Player");
    }
    let Some(e) = s.entities.get(id) else {
        return false;
    };
    let want = match cls {
        "Object" | "Entity" => return true,
        "Actor" | "Character" => return e.class.is_character(),
        "Player" => return false,
        "Npc" => ObjClass::Npc,
        "Proxy" => ObjClass::Proxy,
        "Door" => ObjClass::Door,
        "Lever" => ObjClass::Lever,
        "Chest" => ObjClass::Chest,
        "Destructible" => ObjClass::Destructible,
        "Riftgate" => ObjClass::Riftgate,
        "Shrine" => ObjClass::Shrine,
        "Item" => ObjClass::Item,
        "AreaTrigger" => ObjClass::AreaTrigger,
        _ => return true,
    };
    e.class == want || matches!(e.class, ObjClass::Entity | ObjClass::ScriptEntity)
}

/// Resolves `a.b.c` from the globals (the engine strips anything from `(`).
fn resolve_function(lua: &Lua, path: &str) -> Option<Function> {
    let path = path.split('(').next().unwrap_or("").trim();
    let mut cur = Value::Table(lua.globals());
    for part in path.split('.') {
        cur = match cur {
            Value::Table(t) => t.get(part).ok()?,
            _ => return None,
        };
    }
    match cur {
        Value::Function(f) => Some(f),
        _ => None,
    }
}

impl Ctx {
    fn class_of(&self, record: &str) -> ObjClass {
        self.env
            .record_field(record, "Class")
            .map(|c| ObjClass::from_record_class(&c))
            .unwrap_or(ObjClass::Entity)
    }
    fn obj_value(self: &Rc<Self>, lua: &Lua, id: u32) -> mlua::Result<Value> {
        Obj {
            id,
            ctx: self.clone(),
        }
        .into_lua(lua)
    }
    fn create(&self, record: &str, at: Option<Coords>) -> u32 {
        let class = self.class_of(record);
        let conversation = self
            .env
            .record_field(record, "conversation")
            .filter(|c| !c.is_empty());
        let mut s = self.story.borrow_mut();
        let id = s.entities.create(record, class);
        s.entities.get_mut(id).unwrap().conversation = conversation;
        if let Some(c) = at {
            let o = s.entities.get_mut(id).unwrap();
            o.coords = c;
            o.in_world = true;
            s.effects.push(Effect::Spawn {
                id,
                record: record.to_ascii_lowercase(),
                class,
                coords: c,
            });
            // `World::AddEntity`'s `onAddToWorld` hook, then `onInitialUpdate`.
            for hook in ["onAddToWorld", "onInitialUpdate"] {
                if let Some(f) = self
                    .env
                    .record_field(record, hook)
                    .filter(|f| !f.is_empty())
                {
                    s.lua_calls.push(LuaCall {
                        function: f,
                        args: vec![LuaValue::Number(id as f64)],
                    });
                }
            }
        }
        id
    }
}

pub struct ScriptHost {
    pub lua: Lua,
    ctx: Rc<Ctx>,
}

impl ScriptHost {
    /// Creates the VM (sandboxed, module doc), registers the API and runs `scripts/main.lua`.
    pub fn new(story: Shared, env: Rc<dyn ScriptEnv>) -> mlua::Result<Self> {
        let lua = Lua::new_with(
            StdLib::ALL_SAFE & !(StdLib::OS | StdLib::IO | StdLib::PACKAGE),
            LuaOptions::default(),
        )?;
        lua.set_memory_limit(MEMORY_LIMIT)?;
        let ctx = Rc::new(Ctx {
            story,
            env,
            updates: RefCell::new(Vec::new()),
            loaded: RefCell::new(HashSet::default()),
            budget: Cell::new(0),
            depth: Cell::new(0),
            due: RefCell::new(Vec::new()),
            calls: RefCell::new(Vec::new()),
        });
        let host = Self { lua, ctx };
        host.register()?;
        // The hook errors out of a call that exceeds [`INSTRUCTION_BUDGET`].
        let c = host.ctx.clone();
        host.lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(HOOK_INTERVAL),
            move |_, _| {
                // A direct chunk outside a scope runs unbounded.
                if c.depth.get() == 0 {
                    return Ok(VmState::Continue);
                }
                let left = c.budget.get();
                if left == 0 {
                    return Err(mlua::Error::runtime("Lua instruction budget exceeded"));
                }
                c.budget.set(left - 1);
                Ok(VmState::Continue)
            },
        )?;
        host.load("scripts/main.lua");
        Ok(host)
    }

    pub fn story(&self) -> &Shared {
        &self.ctx.story
    }

    fn log(&self, line: String) {
        self.ctx.story.borrow_mut().log(line);
    }

    /// `Script.Load`: runs a script file (errors are logged, like the engine's
    /// "Failed to load Lua script").
    pub fn load(&self, path: &str) -> bool {
        load_script(&self.lua, &self.ctx, path)
    }

    /// Resolves `a.b.c` from the globals (the engine strips anything from `(`).
    pub fn function(&self, path: &str) -> Option<Function> {
        resolve_function(&self.lua, path)
    }

    /// Calls a Lua function by name; errors and missing functions are logged.
    pub fn call(&self, path: &str, args: impl mlua::IntoLuaMulti) -> Option<MultiValue> {
        let Some(f) = self.function(path) else {
            self.log(format!("Lua function '{path}' does not exist!"));
            return None;
        };
        self.call_function(path, &f, args)
    }

    /// Calls a resolved function; errors are logged under `name`.
    fn call_function(
        &self,
        name: &str,
        f: &Function,
        args: impl mlua::IntoLuaMulti,
    ) -> Option<MultiValue> {
        let _scope = self.ctx.enter_scope();
        match f.call::<MultiValue>(args) {
            Ok(v) => Some(v),
            Err(e) => {
                after_memory_error(&self.lua, &e);
                self.log(format!("Lua error in {name}: {e}"));
                None
            }
        }
    }

    fn lua_values(&self, args: &[LuaValue]) -> MultiValue {
        MultiValue::from_vec(
            args.iter()
                .map(|a| match a {
                    LuaValue::Nil => Value::Nil,
                    LuaValue::Bool(b) => Value::Boolean(*b),
                    LuaValue::Number(n) => Value::Number(*n),
                    LuaValue::Str(s) => self
                        .lua
                        .create_string(s)
                        .map(Value::String)
                        .unwrap_or(Value::Nil),
                })
                .collect(),
        )
    }

    /// Runs Lua functions queued by quest/conversation actions until the queue settles: one budget
    /// scope, at most `QUEUE_ROUNDS` rounds and `MAX_QUEUED_CALLS` calls.
    pub fn run_queued(&self) {
        let _scope = self.ctx.enter_scope();
        let mut ran = 0;
        for _ in 0..QUEUE_ROUNDS {
            let calls = std::mem::take(&mut self.ctx.story.borrow_mut().lua_calls);
            if calls.is_empty() {
                return;
            }
            // The quest events these calls may run read live Lua results.
            self.eval_event_lua_conditions();
            for c in calls {
                if ran == MAX_QUEUED_CALLS {
                    self.ctx.story.borrow_mut().lua_calls.clear();
                    self.log("Lua action queue overflow: queued calls dropped".into());
                    return;
                }
                ran += 1;
                let args = self.lua_values(&c.args);
                self.call(&c.function, args);
            }
        }
        self.log("Lua action queue did not settle".into());
    }
}

/// `Script.Load` / `LuaManager::Load` (Engine.dll): the path is normalised (`\\` → `/`, lower
/// case); a loaded path returns true without running again; a missing file logs and returns false
/// (not listed); otherwise the path is listed before the chunk runs, so a failed script is not retried.
fn load_script(lua: &Lua, ctx: &Rc<Ctx>, path: &str) -> bool {
    let norm = path.replace('\\', "/").to_ascii_lowercase();
    if ctx.loaded.borrow().contains(&norm) {
        return true;
    }
    let Some(bytes) = ctx.env.read(&norm) else {
        ctx.story.borrow_mut().log(format!(
            "Failed to load Lua script '{path}', file doesn't exist!"
        ));
        return false;
    };
    ctx.loaded.borrow_mut().insert(norm.clone());
    let src = to_lua51(&String::from_utf8_lossy(&bytes));
    let _scope = ctx.enter_scope();
    match lua.load(&src).set_name(format!("@{norm}")).exec() {
        Ok(()) => true,
        Err(e) => {
            after_memory_error(lua, &e);
            ctx.story
                .borrow_mut()
                .log(format!("Failed to load Lua script '{path}': {e}"));
            false
        }
    }
}

/// After a Lua memory error, collects in full (Lua 5.1 has no emergency collection).
fn after_memory_error(lua: &Lua, e: &mlua::Error) {
    fn is_memory_error(e: &mlua::Error) -> bool {
        match e {
            mlua::Error::MemoryError(_) => true,
            mlua::Error::CallbackError { cause, .. }
            | mlua::Error::WithContext { cause, .. }
            | mlua::Error::BadArgument { cause, .. } => is_memory_error(cause),
            _ => false,
        }
    }
    if is_memory_error(e) {
        let _collected = lua.gc_collect();
    }
}
