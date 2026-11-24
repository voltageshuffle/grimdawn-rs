//! Lua 5.1 host for the game's scripts (`Scripts.arc`: `scripts/main.lua` →
//! `libs/`, `game/`). Per binding:
//!
//! * Registration: `Grim Dawn.exe` `LuaGlue::RegisterAll` calls one LuaGlue
//!   `BindClass` function per class (Vec `LuaVec::BindClass`, Coords, WorldCoords,
//!   Object `LuaObject::BindClass`, Entity `LuaEntity::BindClass`, Actor
//!   `LuaActor::BindClass`, Character `LuaCharacter::BindClass`, Player
//!   `LuaPlayer::BindClass`, Proxy `LuaProxy::BindClass`, Door `LuaDoor::BindClass`,
//!   Lever `LuaLever::BindClass`, Destructible `LuaDestructible::BindClass`, Chest
//!   `LuaChest::BindClass`, Riftgate `LuaRiftgate::BindClass`, Shrine
//!   `LuaShrine::BindClass`, Npc `LuaNpc::BindClass`, Item `LuaItem::BindClass`,
//!   AreaTrigger `LuaAreaTrigger::BindClass`) and the static tables (Time
//!   `LuaTime::BindClass`, UI `LuaUi::BindClass`, Shared `LuaShared::BindClass`, Game +
//!   `Script.Event` `LuaGame::BindClass`, MpScript `LuaMpScript::BindClass`).
//!   `Script.Load/RegisterForUpdate/UnregisterForUpdate` are bound in Engine.dll
//!   `LuaScript::BindClass`. There is no
//!   `Debug` table in either binary (the port keeps an inert one).
//! * Each class table is the metatable of its objects (`__index` = itself,
//!   `__base` = parent class), so methods are also reachable as
//!   `Character.IsAlive(obj)` / `Player.HasMastery(player, id)`, which the
//!   stock scripts use; the port forwards such calls to the object method.
//! * Argument shapes: each binding's overload matcher (`LuaGlueFunction::MatchArgs`, a virtual of the
//!   LuaGlue function object, e.g. `LuaPlayer::MatchHasItemArgs` for `HasItem`) requires the
//!   exact argument count and Lua types (`lua_gettop == n`, `isstring`,
//!   `isnumber`, `type == LUA_TBOOLEAN`, userdata of the class); a call that
//!   matches no overload is a Lua error, so the engine has no defaulted
//!   arguments. The port accepts the same shapes and is lenient beyond them
//!   (missing counts default to 1); stock scripts always pass full lists.
//! * Objects are `{0, objectId}` userdata; `Class.Get(id)` returns nil unless
//!   the object exists and derives from that class (classInfo walk).
//!
//! The comment on each binding names the exe thunk and
//! the Game.dll function it forwards to; bindings the port leaves as stubs
//! say why. Stubs log once (`Story::unimplemented`).
//!
//! PORT: the VM is sandboxed (`ScriptHost::new`): no `os` / `io` / `package`
//! and no `dofile` / `loadfile` / `require` (no stock script uses them), a
//! memory limit, and an instruction budget per top-level call, script load
//! or frame (`update` / `run_queued`); the engine has neither limit.
//!
//! Objects are userdata wrapping an object id; class tables (`Entity`,
//! `Character`, `Player`, `Door`, ...) carry the static `Get`/`Create`. Coords
//! are plain tables `{origin = Vec, xAxis, yAxis, zAxis}` and `Vec` is a small
//! Lua class (the scripts' `libs/vector.lua` adds methods to it).
//!
//! PORT: an update registration keeps the `mlua::Function` resolved at registration
//! (`Script.RegisterForUpdate`); the engine re-resolves the name from the globals every period.
//! A script that reassigns the named global after registering keeps the old function (no stock
//! script does).
//!
//! Layout: `api` (the API table), `obj` (object methods), `game` (global and class tables), `host` (the host's
//! entry points: frame update, world objects, hooks, conversations).
mod api;
mod game;
mod host;
mod obj;

pub use api::API;

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

/// What the script host needs from the game data.
pub trait ScriptEnv {
    /// Resource bytes (`scripts/...lua`), lower-case `/` path.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    /// A database record field (`Class`, `onAddToWorld`, `extentsX`, ...).
    fn record_field(&self, record: &str, field: &str) -> Option<String>;
    /// Localised text of a tag.
    fn localize(&self, _tag: &str) -> Option<String> {
        None
    }
    /// The game database (the Shattered Realm generator reads its rulesets and floors).
    fn database(&self) -> Option<&gd_formats::vfs::Database> {
        None
    }
    /// The bag footprint of a new item of `record` (its bitmap's size, as the inventory places it); None when
    /// no item can be made of the record (`Player.IsSpaceAvailable`).
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
    /// How many budget scopes are open: Lua calls (bindings can call back
    /// into Lua) and whole frame drains ([`ScriptHost::update`],
    /// [`ScriptHost::run_queued`]).
    depth: Cell<u32>,
    /// [`ScriptHost::update`]'s due registrations (a reused buffer): id, name, resolved function.
    due: RefCell<Vec<(u32, Rc<str>, Function)>>,
    /// [`ScriptHost::update`]'s trigger hook calls (a reused buffer).
    calls: RefCell<Vec<TriggerCall>>,
}

/// PORT: the engine runs scripts with no instruction limit; a runaway script
/// (a mod or a data error) must raise a Lua error instead of freezing the
/// game. The outermost budget scope ([`Ctx::enter_scope`]: a top-level call,
/// a script load, or a whole [`ScriptHost::update`] / [`ScriptHost::run_queued`]
/// drain) gets [`INSTRUCTION_BUDGET`] instructions, shared by everything
/// inside it. Stock usage (base game + expansions): running every
/// script's top-level code takes ~50k instructions, a frame's updates and
/// triggers a few hundred.
const INSTRUCTION_BUDGET: u32 = 10_000_000;
/// How often the budget hook runs (instructions per tick).
const HOOK_INTERVAL: u32 = 10_000;
/// PORT: the engine's Lua state has no memory limit; a runaway script must
/// get a Lua error instead of exhausting the process. The stock scripts use
/// ~7 MB once `main.lua` has run, so this leaves ~9x headroom.
const MEMORY_LIMIT: usize = 64 << 20;
/// PORT: `LuaManager::RegisterForUpdates` appends without a limit; the port
/// refuses registrations past this many (the stock scripts register one or
/// two functions per scripted object).
const MAX_UPDATE_REGS: usize = 1024;
/// PORT: the queue of Lua calls is a port construct (quest/conversation
/// actions and the hooks of objects a script places run once the current
/// step returns); one drain runs at most this many and drops the rest, so a
/// feedback loop cannot grow the queue without bound.
const MAX_QUEUED_CALLS: usize = 4096;
/// Rounds of one [`ScriptHost::run_queued`] drain (calls queue more calls).
const QUEUE_ROUNDS: usize = 64;

/// Keeps [`Ctx::depth`] balanced while a budget scope is open.
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

/// A binding's string argument: the Lua string borrowed when valid UTF-8 (no copy), else an
/// owned conversion (a number, or lossy bytes).
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
    match args.get(i) {
        Some(Value::Integer(n)) => Some(*n as u32),
        Some(Value::Number(n)) => Some(*n as u32),
        Some(Value::UserData(u)) => u.borrow::<Obj>().ok().map(|o| o.id),
        _ => None,
    }
}

/// Whether object `id` passes `<cls>.Get`/`IsType`'s classInfo check
/// (e.g. Entity.Get `LuaEntity::Get`, Player.Get `LuaPlayer::Get`: nil
/// unless the object exists and derives from the class). The port's record
/// `Class` mapping is coarse, so objects it could not classify
/// (`ObjClass::Entity`/`ScriptEntity`) pass every non-character check.
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
            // `World::AddEntity`'s `onAddToWorld` hook (see `Entity.SetCoords`),
            // then `onInitialUpdate` (`ScriptHost::add_object`).
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

/// The Lua VM with the game's scripts loaded.
pub struct ScriptHost {
    pub lua: Lua,
    ctx: Rc<Ctx>,
}

impl ScriptHost {
    /// Creates the VM, registers the API and runs `scripts/main.lua`.
    ///
    /// PORT: the VM is sandboxed. It has no `os`, `io` or `package` library
    /// and no `dofile` / `loadfile` / `require` (no stock script of the base
    /// game, the expansions or the survival mods uses any of them; `loadstring`
    /// stays), a memory limit (`MEMORY_LIMIT`) and an instruction budget
    /// (`INSTRUCTION_BUDGET`).
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
        // The instruction budget (PORT, see [`INSTRUCTION_BUDGET`]): the hook
        // errors out of a call that runs too long, which `call` / `load` log.
        let c = host.ctx.clone();
        host.lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(HOOK_INTERVAL),
            move |_, _| {
                // Only an open scope ([`Ctx::enter_scope`]) carries a budget;
                // a direct chunk outside one runs unbounded.
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

    /// Runs Lua functions queued by quest/conversation actions, until the
    /// queue settles: one budget scope (PORT, see `INSTRUCTION_BUDGET`), at
    /// most `QUEUE_ROUNDS` rounds and `MAX_QUEUED_CALLS` calls.
    pub fn run_queued(&self) {
        let _scope = self.ctx.enter_scope();
        let mut ran = 0;
        for _ in 0..QUEUE_ROUNDS {
            let calls = std::mem::take(&mut self.ctx.story.borrow_mut().lua_calls);
            if calls.is_empty() {
                return;
            }
            // The quest events these calls may run read live Lua results (quest module doc PORT).
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

/// `Script.Load` / `LuaManager::Load` (Engine.dll): the path is
/// normalised (`\\` → `/`, lower case); a path already in the loaded list
/// returns true without running again; a missing file logs and returns
/// false (not listed); otherwise the path is listed before the chunk runs,
/// so a script that fails is not retried either.
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

/// PORT (see `MEMORY_LIMIT`): after a Lua memory error, collects in full.
/// Lua 5.1 has no emergency collection, so the garbage the failed call left
/// would otherwise fail the next allocations until the incremental collector
/// caught up.
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
        // A failed collection leaves the VM as it was; the error is logged by the caller.
        let _collected = lua.gc_collect();
    }
}
