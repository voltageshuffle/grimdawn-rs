//! Story layer: quests (`.qst`), conversations (`.cnv`), area triggers and the
//! game's Lua scripts. Bevy-free.
//!
//! * [`Story`] holds all story state (quest runtime, conversation memory,
//!   tokens/player facts, known world objects, triggers) and the queue of
//!   [`Effect`]s for the host.
//! * [`lua::ScriptHost`] runs the game's Lua 5.1 scripts (after
//!   [`syntax::to_lua51`]) against Rust implementations of the bound API and
//!   drives `Story` (pre-evaluates Lua conditions, runs queued Lua actions,
//!   calls record hooks).
pub mod dialog;
pub mod load;
pub mod lua;
pub mod quest;
pub mod save;
pub mod state;
pub mod syntax;
pub mod trigger;

pub use dialog::{DialogView, Dialogs, Punctuation};
pub use quest::{JournalEntry, Kill, LuaCall, LuaRet, LuaValue, QuestDefs, QuestLog};
pub use state::{
    Coords, Effect, Endless, Entities, GameInfo, HostFacts, ObjClass, Player, WorldObject,
};
pub use trigger::{AreaTrigger, TriggerCall};

use gd_formats::collections::HashMap;
use std::sync::Arc;

pub struct Story {
    pub quests: QuestLog,
    pub dialogs: Dialogs,
    /// The session language's sentence separators for dialog paging
    /// (`text_<code>/language.def`, `LocalizationManager::GetSentenceSeperators`);
    /// [`dialog::SENTENCE_SEPARATORS`] until the host sets it
    /// (`gd_game::story::setup_story` reads the language's def).
    pub separators: String,
    pub player: Player,
    pub game: GameInfo,
    pub entities: Entities,
    pub triggers: Vec<AreaTrigger>,
    /// The area-trigger candidate snapshot (the player and the living story monsters), rebuilt
    /// every frame into reused buffers; while it holds unchanged, empty armed triggers skip
    /// collection ([`AreaTrigger::last_gen`]).
    trigger_candidates: Vec<trigger::Candidate>,
    /// The rebuild scratch, swapped with `trigger_candidates` when the set changes.
    trigger_next: Vec<trigger::Candidate>,
    /// Bumped whenever the candidate snapshot changes.
    trigger_gen: u64,
    /// A reused inside buffer passed between collections ([`AreaTrigger::fire`] returns the old one).
    trigger_spare: Vec<u32>,
    pub effects: Vec<Effect>,
    /// What the host published for the Lua queries (see [`HostFacts`]).
    pub host_facts: HostFacts,
    /// Lua functions queued by actions; run by the script host.
    pub lua_calls: Vec<LuaCall>,
    /// `GenerateRandomValue` results (`CheckRandomValue`).
    pub random_values: HashMap<String, u32>,
    /// `GenerateRandomValue`'s per-name value lists (shuffled `min..=max`) and the next index.
    random_lists: HashMap<String, (Vec<u32>, usize)>,
    /// The `Difficulty` condition's override while `Quest2Task::AdjustRewards` evaluates
    /// (`ScriptableCondition_Difficulty.difficultyOverride`, -1 = none).
    pub(crate) condition_difficulty: Option<u32>,
    /// Pre-evaluated Lua condition results, keyed by [`quest::lua_key`].
    pub lua_results: HashMap<u64, LuaRet>,
    pub log_lines: Vec<String>,
    /// The Shattered Realm's running state (`Game.GenerateEndlessDungeon`).
    pub endless: state::Endless,
    /// Builds the generator's view of the world map (the host closes over the map): the fallback
    /// for a `GenerateEndlessDungeon` before the host pre-built it (`gd_game::endless::sync_endless`
    /// does at session load).
    pub endless_provider: Option<Box<dyn Fn() -> crate::endless::EndlessMap>>,
    /// The session's random stream (`gd_game::story::setup_story` seeds it): `GenerateRandomValue` shuffles, the
    /// random-speech pick and the Shattered Realm generator. PORT: one stream for the engine's CRT `rand()` draws.
    pub rng: gd_rules::Rng,
}

/// PORT (bad data): the longest `GenerateRandomValue` list; a wider range keeps its first values.
const MAX_RANDOM_VALUES: i64 = 1 << 16;

impl Story {
    pub fn new(defs: Arc<QuestDefs>, dialogs: Dialogs, player: Player) -> Self {
        Self {
            quests: QuestLog::new(defs),
            dialogs,
            separators: dialog::SENTENCE_SEPARATORS.to_owned(),
            player,
            game: GameInfo::default(),
            entities: Entities::default(),
            triggers: Vec::new(),
            trigger_candidates: Vec::new(),
            trigger_next: Vec::new(),
            trigger_gen: 0,
            trigger_spare: Vec::new(),
            effects: Vec::new(),
            host_facts: HostFacts::default(),
            lua_calls: Vec::new(),
            random_values: HashMap::default(),
            random_lists: HashMap::default(),
            condition_difficulty: None,
            lua_results: HashMap::default(),
            log_lines: Vec::new(),
            endless: state::Endless::default(),
            endless_provider: None,
            rng: gd_rules::Rng(0x02f6_e2b1),
        }
    }

    pub fn log(&mut self, line: String) {
        self.log_lines.push(line);
    }

    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log_lines)
    }

    /// `GenerateRandomValue` (Game.dll `ScriptableAction_GenerateRandomValue::Read` /
    /// `Execute`): the first action of a value name builds its list, `min..=max` as signed ints (so
    /// `max < min`, e.g. a full `0..u32::MAX` read as `0..-1`, is empty), shuffled with CRT `rand()` after
    /// `srand(FNV-1a of the name followed by the engine value `GameEngine.randomSeed`)`; the index
    /// starts at -1 and each execution steps it to the next entry, wrapping to 0, so no value repeats until the
    /// list is used up. `CheckRandomValue` reads the entry at the index ([`Self::random_values`]; none for an
    /// empty list). PORT: the list is built on the first execution instead of at quest load (same list: the first
    /// action's range), and shuffled (Fisher–Yates) with [`Self::rng`].
    pub fn random_value(&mut self, name: &str, min: u32, max: u32) {
        if !self.random_lists.contains_key(name) {
            let (lo, hi) = (min as i32 as i64, max as i32 as i64);
            let hi = hi.min(lo + MAX_RANDOM_VALUES - 1);
            let mut list: Vec<u32> = (lo..=hi).map(|v| v as i32 as u32).collect();
            for i in (1..list.len()).rev() {
                let j = self.rng.next() as usize % (i + 1);
                list.swap(i, j);
            }
            self.random_lists
                .insert(name.to_string(), (list, usize::MAX));
        }
        let (list, idx) = self.random_lists.get_mut(name).expect("inserted above");
        *idx = if *idx == usize::MAX || *idx + 1 >= list.len() {
            0
        } else {
            *idx + 1
        };
        match list.get(*idx) {
            Some(&v) => {
                self.random_values.insert(name.to_string(), v);
            }
            None => {
                self.random_values.remove(name);
            }
        }
    }

    /// Whether the host has object `o` in the world: placed and alive, and a proxy only once it runs.
    /// `Proxy::DelayedRun` / `RunProxy`: a `delayedRun` proxy is loaded
    /// in state 1 and spawns nothing until the script's `Proxy:Run()` puts it in state 0 and runs it
    /// ([`HostFacts::proxies_delayed`]).
    pub fn is_present(&self, o: &WorldObject) -> bool {
        o.in_world
            && o.alive
            && !(o.class == ObjClass::Proxy && self.host_facts.proxies_delayed.contains(&o.id))
    }

    /// Advances the area triggers by `dt_ms` against the detectable entities: the player and the living
    /// story monsters in the world (class `Monster`: a `Monsters` trigger's candidates; their position is
    /// the host's, mirrored into the coords). The hook calls to make are pushed to `calls`. A trigger that
    /// collected entities its `shouldTrigger` hook must accept first ([`AreaTrigger::hook`]) is returned
    /// instead: the script host asks the hook and ends its step with [`Self::fire_trigger`].
    pub fn update_triggers(
        &mut self,
        dt_ms: i32,
        calls: &mut Vec<TriggerCall>,
    ) -> Vec<trigger::Hooked> {
        let mut hooked = Vec::new();
        // Rebuild the snapshot into the reused scratch; a changed set advances the generation
        // (positions do not change between a trigger's collection and its step).
        self.trigger_next.clear();
        self.trigger_next
            .push((self.player.id, self.player.position));
        self.trigger_next.extend(
            self.entities
                .objects
                .values()
                .filter(|o| o.class == ObjClass::Character && o.alive && o.in_world)
                .map(|o| (o.id, o.coords.origin)),
        );
        if self.trigger_next != self.trigger_candidates {
            std::mem::swap(&mut self.trigger_next, &mut self.trigger_candidates);
            self.trigger_gen += 1;
        }
        let generation = self.trigger_gen;
        let candidates = &self.trigger_candidates;
        let player = &self.player;
        let entities = &self.entities;
        let record = |id: u32| -> &str {
            if id == player.id {
                &player.record
            } else {
                entities.objects.get(&id).map_or("", |o| &o.record)
            }
        };
        let mut spare = std::mem::take(&mut self.trigger_spare);
        for t in &mut self.triggers {
            if !t.tick(dt_ms) {
                continue;
            }
            // Armed and empty, and the last collection under this same snapshot found nobody:
            // collecting again would only fire a no-op step (trigger module doc).
            if t.state == 1 && t.inside.is_empty() && t.last_empty && t.last_gen == generation {
                continue;
            }
            t.collect(candidates.iter().copied(), player.id, record, &mut spare);
            t.last_gen = generation;
            t.last_empty = spare.is_empty();
            match t.hook().filter(|_| !spare.is_empty()) {
                Some(f) => hooked.push(trigger::Hooked {
                    trigger: t.id,
                    function: f.into(),
                    inside: std::mem::take(&mut spare),
                }),
                None => spare = t.fire(spare, calls),
            }
        }
        self.trigger_spare = spare;
        self.triggers.retain(|t| !t.destroyed);
        hooked
    }

    /// Ends the step of a [`trigger::Hooked`] trigger with the entities its hook accepted (a trigger the
    /// hook removed does nothing).
    pub fn fire_trigger(&mut self, id: u32, inside: Vec<u32>, calls: &mut Vec<TriggerCall>) {
        if let Some(t) = self.triggers.iter_mut().find(|t| t.id == id) {
            self.trigger_spare = t.fire(inside, calls);
        }
        self.triggers.retain(|t| !t.destroyed);
    }
}
