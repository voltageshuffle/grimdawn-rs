//! Quest repository + state machine, ported from Game.dll `Quest2Repository`,
//! `Quest2`, `Quest2Task`, `Quest2Objective`, `Quest2Event`.
//!
//! Task state (`Quest2Task+144`): 0 unavailable, 1 available (initial),
//! 2 in progress, 3 complete. Lua `QuestState` = {Invalid 0, Unavailable 1,
//! Available 2, InProgress 3, Complete 4} (exe binding).
//!
//! * `BeginQuestTask(quest, task, force)`: unless forced, ignored when the task
//!   is in progress or complete. `Start`: if the task was *available* its
//!   incentive events run (conditions → actions); state becomes 2 and the task
//!   joins the active list.
//! * Every 500 ms each active task runs `Update`: per objective, evaluate its
//!   conditions (all/any). Objective state 0 → 1/2 without running actions;
//!   2 → 1 runs the objective's actions; 1 → 2 whenever it stops holding.
//!   When no objective is in state 2 the task completes. (
//!   0x180427170: the conditions' changed flag `+20` only gates the
//!   objective-changed UI event; transitions and completion run on the
//!   evaluated result, so the port keeps no such flag.)
//!   Objectives with no condition list... have none (`Update` returns early for
//!   tasks without objectives, so those only complete explicitly).
//! * `Complete`: state 3; if it was in progress the reward events run. An
//!   *available* task completed explicitly becomes 3 without rewards (back to
//!   1 for repeatable quests).
//! * Condition type 17 ("manual") is always false: such objectives are
//!   completed by a `CompleteQuestTask` action (conversations, Lua).
use super::Story;
use super::state::{DIFFICULTIES, Effect, MAX_TRIBUTE};
use anyhow::{Context, Result};
use gd_formats::qst::{Action, Comparator, Condition, ConditionEntry, Conditions, LuaArg, Quest};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const STATE_UNAVAILABLE: u32 = 0;
pub const STATE_AVAILABLE: u32 = 1;
pub const STATE_IN_PROGRESS: u32 = 2;
pub const STATE_COMPLETE: u32 = 3;

/// `Quest2Repository::kUpdatePeriod` (Update resets its timer to 500).
pub const UPDATE_PERIOD_MS: i32 = 500;

pub struct QuestDef {
    /// Normalised resource path (`quests/mq_wakingtomisery.qst`).
    pub path: String,
    pub quest: Quest,
}

#[derive(Default)]
pub struct QuestDefs {
    pub list: Vec<QuestDef>,
    by_path: HashMap<String, usize>,
    by_id: HashMap<u32, usize>,
}

pub fn normalize_path(p: &str) -> String {
    p.replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

impl QuestDefs {
    /// From `(resource path, file bytes)` pairs.
    pub fn from_files<I: IntoIterator<Item = (String, Vec<u8>)>>(files: I) -> Result<Self> {
        let mut defs = Self::default();
        for (path, data) in files {
            let quest = Quest::parse(&data).with_context(|| path.clone())?;
            defs.add(QuestDef {
                path: normalize_path(&path),
                quest,
            });
        }
        Ok(defs)
    }
    pub fn add(&mut self, def: QuestDef) {
        let i = self.list.len();
        self.by_path.insert(def.path.clone(), i);
        self.by_id.insert(def.quest.id, i);
        self.list.push(def);
    }
    pub fn by_path(&self, path: &str) -> Option<usize> {
        self.by_path.get(&normalize_path(path)).copied()
    }
    pub fn by_id(&self, id: u32) -> Option<usize> {
        self.by_id.get(&id).copied()
    }

    /// The repository's quest order (what `Quest2Repository::GetQuests`
    /// returns, so the tracker's and the journal's order), as indices into
    /// [`Self::list`]. Game.dll `Quest2Repository::LoadQuests`
    /// (0x180422870) / `GetQuests` (0x180423150): the quests are loaded in
    /// the world's quest-file order (`World::GetQuestFile`, the `.map`
    /// header list) into an MSVC `std::unordered_map<u32 id, Quest2*>`
    /// (a later file with an id already loaded is dropped) and returned in
    /// that map's iteration order ([`crate::hash_order::msvc_unordered_order`]).
    /// Quests the map does not list follow in [`Self::list`] order (PORT: the
    /// engine never loads them).
    pub fn repository_order(&self, map_quests: &[String]) -> Vec<usize> {
        let mut seen = vec![false; self.list.len()];
        let mut ins: Vec<(u32, usize)> = Vec::new();
        for p in map_quests {
            let Some(i) = self.by_path(p) else { continue };
            let id = self.list[i].quest.id;
            if seen[i] || ins.iter().any(|(k, _)| *k == id) {
                continue;
            }
            seen[i] = true;
            ins.push((id, i));
        }
        let keys: Vec<[u8; 4]> = ins.iter().map(|(k, _)| k.to_le_bytes()).collect();
        let mut out: Vec<usize> = crate::hash_order::msvc_unordered_order(&keys)
            .into_iter()
            .map(|j| ins[j].1)
            .collect();
        out.extend((0..self.list.len()).filter(|i| !seen[*i]));
        out
    }
}

/// A creature death as the quest kill conditions see it (`GameEvent_CreatureDeath`, event 11: victim +40,
/// killer +44). Death handlers of type 10 (sub_180471180), 28 (sub_180471E30) and 29 (sub_1804718F0):
/// - the killer is a `Player`, or a `Monster` whose `GetLeader` (vtable +2256) is one (his pets);
/// - the victim is not in the Shattered Realm's dungeon (`Character` +8356 = `InEndlessDungeon`, which
///   `Monster::InitialUpdate` sets for a monster of a region `IsRegionInDungeon`);
/// - 10: the victim's record is one of the condition's (type 10 also counts a non-`Monster` victim);
/// - 28 / 29 (only with `IsExpansion3Loaded`; PORT: the port always loads every expansion): the victim gives
///   experience (`GiveExperience` vtable +1392 = the record's `giveXP`, default 1) and, for 28, is a `Monster`
///   whose classification is one of the two; for 29 its original faction is (not -1000 and) one of the two.
///
/// PORT: type 16 (no shipped quest) counts like 10.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Kill {
    pub record: String,
    /// The main player or his pet dealt the killing blow.
    pub by_player: bool,
    /// The victim is a monster of the Shattered Realm's running dungeon.
    pub in_dungeon: bool,
    /// The record's `giveXP` (missing: true, `Monster::Load`).
    pub give_xp: bool,
    /// [`monster_classification`] of the record's `monsterClassification`.
    pub classification: u32,
    /// `myFaction` of the record's `factions` pack ([`crate::faction::type_of`]); None = -1000.
    pub faction: Option<u32>,
}

/// `GameEngine::ResolveMonsterClassificationEnum`: exact, case-sensitive names; Common 0, Champion 1,
/// Hero 2, Boss 3, Quest 4, SuperBoss 5; anything else 0.
pub fn monster_classification(name: &str) -> u32 {
    match name {
        "Champion" => 1,
        "Hero" => 2,
        "Boss" => 3,
        "Quest" => 4,
        "SuperBoss" => 5,
        _ => 0,
    }
}

impl Kill {
    /// The player's kill of a creature record, its rule fields read from the database (`Monster::Load`:
    /// `monsterClassification`, `giveXP`; `Character::Load`: the `factions` pack's `myFaction`).
    pub fn of_record(db: &gd_formats::vfs::Database, record: &str) -> Self {
        let rec = db.get(record);
        let rec = rec.as_ref();
        let faction = rec
            .and_then(|r| db.get(r.str("factions")?))
            .and_then(|p| p.str("myFaction").and_then(crate::faction::type_of))
            .map(|f| f as u32);
        Self {
            record: record.to_owned(),
            by_player: true,
            in_dungeon: false,
            give_xp: rec.and_then(|r| r.f32("giveXP")).is_none_or(|v| v != 0.0),
            classification: monster_classification(
                rec.and_then(|r| r.str("monsterClassification"))
                    .unwrap_or(""),
            ),
            faction,
        }
    }

    /// The player's kill of a record no rule field is known of (a script's object): only types 10 / 16 can count it.
    pub fn record_only(record: &str) -> Self {
        Self {
            record: record.to_owned(),
            by_player: true,
            ..Self::default()
        }
    }

    /// Whether this (player's, out-of-dungeon) kill advances `cond`; `rec` = the normalised record.
    fn counts_for(&self, cond: &Condition, rec: &str) -> bool {
        match cond {
            Condition::KillCreature { records, .. } | Condition::KillCreature16 { records, .. } => {
                records.iter().any(|r| r == rec)
            }
            Condition::KillClassification { classes, .. } => {
                self.give_xp && classes.contains(&self.classification)
            }
            Condition::KillFaction { factions, .. } => {
                self.give_xp && self.faction.is_some_and(|f| factions.contains(&f))
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjectiveRt {
    /// `Quest2Objective+56`: 0 unknown, 1 satisfied, 2 unsatisfied.
    pub satisfied: u8,
    /// Progress per condition (counters of types 10/15/16/28/29; 0 otherwise).
    pub counters: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskRt {
    pub state: u32,
    /// Saved per-difficulty states (`+148`); `state` mirrors the current one.
    pub per_difficulty: [u32; DIFFICULTIES],
    /// `+160`: completed from in-progress (saved byte 1).
    pub finished: [bool; DIFFICULTIES],
    /// `+163`: rewards granted (saved byte 2).
    pub rewarded: [bool; DIFFICULTIES],
    pub objectives: Vec<ObjectiveRt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuestRt {
    pub tasks: Vec<TaskRt>,
    /// `Quest2+76`: something changed since load (drives the "quest done" check).
    pub changed: bool,
}

pub struct QuestLog {
    pub defs: Arc<QuestDefs>,
    pub rt: Vec<QuestRt>,
    /// Begun tasks, polled by `update` (quest index, task index).
    pub active: Vec<(usize, usize)>,
    pub timer_ms: i32,
    /// PORT: bumped whenever a task state, an objective's satisfied flag or a counter changes — the UI's change
    /// signal (`grimdawn::ui::quests::collect` rebuilds its view only when it moves).
    pub revision: u64,
    /// The quests whose [`QuestRt::changed`] is set, checked by `update_quests` (unordered).
    changed_quests: Vec<usize>,
    /// The `LuaScript` conditions of every task's incentives and rewards (unique keys).
    lua_events: Vec<Arc<LuaCond>>,
    /// `[quest][task]`: the `LuaScript` conditions of the task's objectives (unique keys).
    lua_objectives: Vec<Vec<Vec<Arc<LuaCond>>>>,
}

fn fresh_rt(q: &Quest) -> QuestRt {
    QuestRt {
        tasks: q
            .tasks
            .iter()
            .map(|t| TaskRt {
                state: STATE_AVAILABLE,
                per_difficulty: [STATE_AVAILABLE; DIFFICULTIES],
                finished: [false; DIFFICULTIES],
                rewarded: [false; DIFFICULTIES],
                objectives: t
                    .objectives
                    .iter()
                    .map(|o| ObjectiveRt {
                        satisfied: 0,
                        counters: vec![0; o.conditions.as_ref().map_or(0, |c| c.list.len())],
                    })
                    .collect(),
            })
            .collect(),
        changed: false,
    }
}

impl QuestLog {
    pub fn new(defs: Arc<QuestDefs>) -> Self {
        let rt = defs.list.iter().map(|d| fresh_rt(&d.quest)).collect();
        let mut seen = HashSet::new();
        let mut lua_events = Vec::new();
        for t in defs.list.iter().flat_map(|d| &d.quest.tasks) {
            for e in t.incentives.iter().chain(&t.rewards) {
                collect_lua_conds(e.conditions.as_ref(), &mut seen, &mut lua_events);
            }
        }
        let lua_objectives: Vec<Vec<Vec<Arc<LuaCond>>>> = defs
            .list
            .iter()
            .map(|d| {
                d.quest
                    .tasks
                    .iter()
                    .map(|t| {
                        seen.clear();
                        let mut out = Vec::new();
                        for o in &t.objectives {
                            collect_lua_conds(o.conditions.as_ref(), &mut seen, &mut out);
                        }
                        out
                    })
                    .collect()
            })
            .collect();
        Self {
            defs,
            rt,
            active: Vec::new(),
            timer_ms: UPDATE_PERIOD_MS,
            revision: 0,
            changed_quests: Vec::new(),
            lua_events,
            lua_objectives,
        }
    }
    /// Sets quest `qi`'s [`QuestRt::changed`].
    fn mark_changed(&mut self, qi: usize) {
        self.revision += 1;
        if !std::mem::replace(&mut self.rt[qi].changed, true) {
            self.changed_quests.push(qi);
        }
    }
    pub fn task_index(&self, qi: usize, uid: u32) -> Option<usize> {
        self.defs.list[qi]
            .quest
            .tasks
            .iter()
            .position(|t| t.uid == uid)
    }
    pub fn task_state(&self, qi: usize, ti: usize) -> u32 {
        self.rt[qi].tasks[ti].state
    }
    pub fn in_progress(&self, qi: usize, ti: usize) -> bool {
        self.task_state(qi, ti) == STATE_IN_PROGRESS
    }
    /// Lua `Player:GetQuestTaskState` (exe sub_140061a00).
    pub fn lua_task_state(&self, quest_id: u32, task: u32) -> u32 {
        let Some(qi) = self.defs.by_id(quest_id) else {
            return 0;
        };
        let Some(ti) = self.task_index(qi, task) else {
            return 0;
        };
        match self.task_state(qi, ti) {
            STATE_COMPLETE => 4,
            STATE_IN_PROGRESS => 3,
            s => (s == STATE_AVAILABLE) as u32 + 1,
        }
    }
    /// Lua `Player:GetQuestState` (exe sub_140061920).
    pub fn lua_quest_state(&self, quest_id: u32) -> u32 {
        let Some(qi) = self.defs.by_id(quest_id) else {
            return 0;
        };
        let tasks = &self.rt[qi].tasks;
        if tasks.iter().any(|t| t.state == STATE_IN_PROGRESS) {
            return 3;
        }
        if tasks.iter().any(|t| t.state == STATE_AVAILABLE) {
            return 2;
        }
        if tasks.iter().all(|t| t.state == STATE_COMPLETE) {
            4
        } else {
            1
        }
    }
    /// Tasks in progress with their visible objectives (for logs / HUD).
    pub fn journal(&self) -> Vec<JournalEntry> {
        let mut out = Vec::new();
        for (qi, def) in self.defs.list.iter().enumerate() {
            for (ti, t) in def.quest.tasks.iter().enumerate() {
                let rt = &self.rt[qi].tasks[ti];
                if rt.state != STATE_IN_PROGRESS {
                    continue;
                }
                let objectives = t
                    .objectives
                    .iter()
                    .zip(&rt.objectives)
                    .map(|(o, ort)| (def.quest.texts.get(o.text).to_string(), ort.satisfied == 1))
                    .filter(|(t, _)| !t.is_empty())
                    .collect();
                out.push(JournalEntry {
                    quest: def.quest.name().to_string(),
                    task: def.quest.texts.get(t.name_text).to_string(),
                    description: def.quest.texts.get(t.desc_text).to_string(),
                    objectives,
                });
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JournalEntry {
    pub quest: String,
    pub task: String,
    pub description: String,
    pub objectives: Vec<(String, bool)>,
}

/// Result of a Lua condition function, pre-evaluated by the script host.
#[derive(Debug, Clone, PartialEq)]
pub enum LuaRet {
    Bool(bool),
    Number(f64),
    Str(String),
    Nil,
}

/// A `LuaScript` condition's call with its arguments already converted ([`lua_arg_value`]), and the key of its
/// result in `Story::lua_results` ([`lua_key`]).
#[derive(Debug)]
pub struct LuaCond {
    pub key: u64,
    pub function: String,
    pub args: Vec<LuaValue>,
}

/// The key under which a Lua condition's result is cached (`Story::lua_results`): a hash of the function and its
/// raw arguments (`DefaultHasher::new` has fixed keys, so it is the same every run).
pub fn lua_key(function: &str, args: &[LuaArg]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    function.hash(&mut h);
    for a in args {
        (a.kind, a.flag, a.int, a.text.as_str()).hash(&mut h);
    }
    h.finish()
}

/// Appends the `LuaScript` conditions of `conds` whose key is not in `seen` yet.
pub(crate) fn collect_lua_conds(
    conds: Option<&Conditions>,
    seen: &mut HashSet<u64>,
    out: &mut Vec<Arc<LuaCond>>,
) {
    for e in conds.iter().flat_map(|c| &c.list) {
        if let Condition::LuaScript { function, args, .. } = &e.cond {
            let key = lua_key(function, args);
            if seen.insert(key) {
                out.push(Arc::new(LuaCond {
                    key,
                    function: function.clone(),
                    args: args.iter().map(lua_arg_value).collect(),
                }));
            }
        }
    }
}

impl Story {
    /// `Quest2Repository::BeginQuestTask`.
    pub fn begin_task(&mut self, qi: usize, uid: u32, force: bool) {
        let Some(ti) = self.quests.task_index(qi, uid) else {
            self.log(format!(
                "BeginQuestTask: invalid task {uid:#x} in {}",
                self.quests.defs.list[qi].path
            ));
            return;
        };
        let state = self.quests.task_state(qi, ti);
        if !force && (state == STATE_IN_PROGRESS || state == STATE_COMPLETE) {
            return;
        }
        let defs = self.quests.defs.clone();
        let task = &defs.list[qi].quest.tasks[ti];
        if state == STATE_AVAILABLE {
            for ev in &task.incentives {
                if self.eval_conditions(ev.conditions.as_ref(), &[]) {
                    self.exec_actions(&ev.actions, None);
                }
            }
        }
        let d = self.difficulty();
        let rt = &mut self.quests.rt[qi].tasks[ti];
        rt.state = STATE_IN_PROGRESS;
        rt.per_difficulty[d] = STATE_IN_PROGRESS;
        self.quests.mark_changed(qi);
        if !self.quests.active.contains(&(qi, ti)) {
            self.quests.active.push((qi, ti));
        }
        self.effects.push(Effect::QuestTaskStarted {
            quest: defs.list[qi].quest.id,
            task: uid,
        });
    }

    pub fn begin_task_by_path(&mut self, path: &str, uid: u32, force: bool) {
        match self.quests.defs.by_path(path) {
            Some(qi) => self.begin_task(qi, uid, force),
            None => self.log(format!("BeginQuestTask: unknown quest '{path}'")),
        }
    }

    /// `Quest2Repository::CompleteQuestTask` → `Quest2Task::Complete(true)`.
    pub fn complete_task(&mut self, qi: usize, uid: u32) {
        let Some(ti) = self.quests.task_index(qi, uid) else {
            self.log(format!(
                "CompleteQuestTask: invalid task {uid:#x} in {}",
                self.quests.defs.list[qi].path
            ));
            return;
        };
        self.complete_task_index(qi, ti, true);
        self.quests.active.retain(|&a| a != (qi, ti));
    }

    pub fn complete_task_by_path(&mut self, path: &str, uid: u32) {
        match self.quests.defs.by_path(path) {
            Some(qi) => self.complete_task(qi, uid),
            None => self.log(format!("CompleteQuestTask: unknown quest '{path}'")),
        }
    }

    fn complete_task_index(&mut self, qi: usize, ti: usize, rewards: bool) {
        let d = self.difficulty();
        let defs = self.quests.defs.clone();
        let quest = &defs.list[qi].quest;
        let rt = &mut self.quests.rt[qi].tasks[ti];
        let prev = rt.state;
        rt.state = STATE_COMPLETE;
        rt.per_difficulty[d] = STATE_COMPLETE;
        self.quests.mark_changed(qi);
        if prev == STATE_IN_PROGRESS {
            self.quests.rt[qi].tasks[ti].finished[d] = true;
            if rewards {
                let scale = self.adjust_rewards(qi, ti, d);
                for (ev, n) in quest.tasks[ti].rewards.iter().zip(scale) {
                    if self.eval_conditions(ev.conditions.as_ref(), &[]) {
                        match n {
                            Some(n) => {
                                let scaled: Vec<Action> = ev
                                    .actions
                                    .iter()
                                    .map(|a| match a {
                                        Action::GiveSkillPoint { amount } => {
                                            Action::GiveSkillPoint { amount: amount * n }
                                        }
                                        Action::GiveAttribPoint { amount } => {
                                            Action::GiveAttribPoint { amount: amount * n }
                                        }
                                        a => a.clone(),
                                    })
                                    .collect();
                                self.exec_actions(&scaled, None);
                            }
                            None => self.exec_actions(&ev.actions, None),
                        }
                    }
                }
                for r in &mut self.quests.rt[qi].tasks[ti].rewarded[..=d] {
                    *r = true;
                }
            }
            self.effects.push(Effect::QuestTaskCompleted {
                quest: quest.id,
                task: quest.tasks[ti].uid,
            });
        } else if prev == STATE_AVAILABLE && quest.repeatable() {
            let rt = &mut self.quests.rt[qi].tasks[ti];
            rt.state = STATE_AVAILABLE;
            rt.per_difficulty[d] = STATE_AVAILABLE;
            self.quests.revision += 1;
        }
    }

    /// `Quest2Task::AdjustRewards` (@0x1804277b0), run by `Complete(true)` before the reward events: an
    /// event whose conditions hold with the `Difficulty` condition forced to each of Normal, Elite and Ultimate
    /// gets the count of difficulties up to the current `d` whose rewards were not given (`+163`) and that were not
    /// skipped with a difficulty unlock item; its `GiveSkillPoint` / `GiveAttribPoint` actions then pay that many
    /// times their amount (`ModifyValue` @0x18046ac70; every other action ignores it). Other events: `None`.
    fn adjust_rewards(&mut self, qi: usize, ti: usize, d: usize) -> Vec<Option<u32>> {
        let defs = self.quests.defs.clone();
        let rt = &self.quests.rt[qi].tasks[ti];
        let n = (0..=d)
            .filter(|&k| !rt.rewarded[k] && !self.skipped(k))
            .count() as u32;
        let mut out = Vec::new();
        for ev in &defs.list[qi].quest.tasks[ti].rewards {
            let all = (0..DIFFICULTIES as u32).all(|k| {
                self.condition_difficulty = Some(k);
                let ok = self.eval_conditions(ev.conditions.as_ref(), &[]);
                self.condition_difficulty = None;
                ok
            });
            out.push(all.then_some(n));
        }
        out
    }

    /// `Quest2Repository::Update`: every 500 ms update active tasks, then quests.
    pub fn update_quests(&mut self, dt_ms: i32) {
        self.quests.timer_ms -= dt_ms;
        if self.quests.timer_ms > 0 {
            return;
        }
        self.quests.timer_ms = UPDATE_PERIOD_MS;
        let active = self.quests.active.clone();
        for (qi, ti) in active {
            self.update_task(qi, ti);
        }
        // Quest2::Update: repeatable quests reset once every task is complete.
        let mut changed = std::mem::take(&mut self.quests.changed_quests);
        changed.retain(|&qi| {
            let rt = &mut self.quests.rt[qi];
            if !rt.changed {
                return false;
            }
            if rt.tasks.is_empty() || rt.tasks.iter().any(|t| t.state != STATE_COMPLETE) {
                return true;
            }
            rt.changed = false;
            let quest = &self.quests.defs.list[qi].quest;
            if quest.repeatable() {
                *rt = fresh_rt(quest);
            }
            false
        });
        self.quests.changed_quests = changed;
    }

    /// `Quest2Task::Update` (0x180427170).
    fn update_task(&mut self, qi: usize, ti: usize) {
        if self.quests.task_state(qi, ti) != STATE_IN_PROGRESS {
            self.quests.active.retain(|&a| a != (qi, ti));
            return;
        }
        let defs = self.quests.defs.clone();
        let quest = &defs.list[qi].quest;
        let task = &quest.tasks[ti];
        if task.objectives.is_empty() {
            return;
        }
        let mut all_met = true;
        for (oi, obj) in task.objectives.iter().enumerate() {
            let sat = self.eval_conditions(
                obj.conditions.as_ref(),
                &self.quests.rt[qi].tasks[ti].objectives[oi].counters,
            );
            let ort = &mut self.quests.rt[qi].tasks[ti].objectives[oi];
            let before = ort.satisfied;
            match ort.satisfied {
                0 => ort.satisfied = if sat { 1 } else { 2 },
                2 if sat => {
                    ort.satisfied = 1;
                    self.effects.push(Effect::ObjectiveSatisfied {
                        quest: quest.id,
                        task: task.uid,
                        objective: obj.uid,
                    });
                    self.exec_actions(&obj.actions, None);
                }
                1 if !sat => ort.satisfied = 2,
                _ => {}
            }
            if self.quests.rt[qi].tasks[ti].objectives[oi].satisfied != before {
                self.quests.revision += 1;
            }
            if self.quests.rt[qi].tasks[ti].objectives[oi].satisfied == 2 {
                all_met = false;
            }
        }
        if all_met {
            self.complete_task_index(qi, ti, true);
            self.quests.active.retain(|&a| a != (qi, ti));
        }
    }

    pub fn difficulty(&self) -> usize {
        (self.game.difficulty as usize).min(DIFFICULTIES - 1)
    }

    /// The progress an objective's text shows, `(done, needed)`, from its
    /// first condition (Game.dll `Quest2Objective::GetText`
    /// @0x180422320: `Read` keeps the first condition at +40 and its
    /// `Localize` (vtable +48) rewrites the text): kill counters (types 10,
    /// 16, 28, 29: `tagConditionHasKilled` "{%t0} ({%d1}/{%d2})") and item
    /// counts (type 9, `tagConditionHasItem`), the done part capped at the
    /// needed count. Other conditions keep the text as is. PORT: type 15's
    /// `tagQuestObjectiveUsedQuestItem*` texts are not shown (no shipped
    /// quest uses it).
    pub fn objective_progress(&self, qi: usize, ti: usize, oi: usize) -> Option<(u32, u32)> {
        let obj = self
            .quests
            .defs
            .list
            .get(qi)?
            .quest
            .tasks
            .get(ti)?
            .objectives
            .get(oi)?;
        let first = obj.conditions.as_ref()?.list.first()?;
        let counter = || {
            self.quests.rt[qi].tasks[ti].objectives[oi]
                .counters
                .first()
                .copied()
                .unwrap_or(0)
        };
        let (done, need) = match &first.cond {
            Condition::KillCreature { count, .. } | Condition::KillCreature16 { count, .. } => {
                (counter(), *count)
            }
            Condition::KillClassification { count, .. } | Condition::KillFaction { count, .. } => {
                (counter(), *count)
            }
            Condition::HasItem { item, count, extra } => (
                self.player
                    .item_count_in_stashes(item, extra.unwrap_or(false)),
                *count,
            ),
            _ => return None,
        };
        Some((done.min(need), need))
    }

    /// `Quest2Task::AreObjectivesMet` (Game.dll 0x180426e10): every
    /// objective's condition collection holds, evaluated live, `Manual`
    /// conditions (type 17) skipped: an all-collection holds when the rest do,
    /// an any-collection needs one of the rest (none: unmet); an objective
    /// without conditions is met.
    pub fn task_objectives_met(&self, qi: usize, ti: usize) -> bool {
        let task = &self.quests.defs.list[qi].quest.tasks[ti];
        task.objectives.iter().enumerate().all(|(oi, obj)| {
            let Some(c) = obj.conditions.as_ref() else {
                return true;
            };
            let counters = &self.quests.rt[qi].tasks[ti].objectives[oi].counters;
            let mut rest = c
                .list
                .iter()
                .enumerate()
                .filter(|(_, e)| e.kind != 17)
                .map(|(i, e)| self.eval_condition(e, counters.get(i).copied().unwrap_or(0)));
            match c.mode {
                0 => rest.all(|b| b),
                1 => rest.any(|b| b),
                _ => false,
            }
        })
    }

    /// `ScriptableConditionCollection::Evaluate`; `None` holds.
    pub fn eval_conditions(&self, conds: Option<&Conditions>, counters: &[u32]) -> bool {
        let Some(c) = conds else { return true };
        let mut it = c
            .list
            .iter()
            .enumerate()
            .map(|(i, e)| self.eval_condition(e, counters.get(i).copied().unwrap_or(0)));
        match c.mode {
            0 => it.all(|b| b),
            1 => it.any(|b| b),
            _ => false,
        }
    }

    /// One `ScriptableCondition::Evaluate`. Single player: the local player is
    /// also the server and "anyone" (Server/AnyoneHasToken == HasToken), and no
    /// task is ever blocked by the host.
    pub fn eval_condition(&self, e: &ConditionEntry, counter: u32) -> bool {
        let p = &self.player;
        let cmp = e.comparator;
        let q = &self.quests;
        let quest = |path: &str| q.defs.by_path(path);
        match &e.cond {
            Condition::OnQuest { quest: path, value } => match quest(path) {
                Some(qi) => q.rt[qi].tasks.iter().any(|t| t.state == STATE_IN_PROGRESS) == *value,
                None => false,
            },
            Condition::QuestTaskComplete {
                quest: path,
                task,
                value,
            } => match quest(path).and_then(|qi| q.task_index(qi, *task).map(|ti| (qi, ti))) {
                Some((qi, ti)) => (q.task_state(qi, ti) == STATE_COMPLETE) == *value,
                None => false,
            },
            Condition::OnQuestTask {
                quest: path,
                task,
                value,
            } => {
                let on = quest(path)
                    .and_then(|qi| q.task_index(qi, *task).map(|ti| q.in_progress(qi, ti)))
                    .unwrap_or(false);
                on == *value
            }
            Condition::QuestBlocked { quest: path, value } => quest(path).is_some() && !*value,
            Condition::HasQuestObjective {
                quest: path,
                task,
                objective,
                value,
            } => {
                let Some((qi, ti)) =
                    quest(path).and_then(|qi| q.task_index(qi, *task).map(|ti| (qi, ti)))
                else {
                    return false;
                };
                let met = match objective {
                    Some(uid) => {
                        let def = &q.defs.list[qi].quest.tasks[ti];
                        match def.objectives.iter().position(|o| o.uid == *uid) {
                            Some(oi) => q.rt[qi].tasks[ti].objectives[oi].satisfied == 1,
                            None => return false,
                        }
                    }
                    None => self.task_objectives_met(qi, ti),
                };
                met == *value
            }
            Condition::Experience { value } => cmp.compare(p.experience, *value),
            Condition::Level { value } => cmp.compare(p.level, *value),
            // sub_180472f20: the difficulty override (`+36`, set by `Quest2Task::AdjustRewards`) when
            // not -1, else the game's.
            Condition::Difficulty { value } => cmp.compare(
                *value,
                self.condition_difficulty.unwrap_or(self.game.difficulty),
            ),
            Condition::Hardcore { value } => p.hardcore == *value,
            Condition::HasFaction { faction, value } => {
                cmp.compare(p.faction(faction), *value as f32)
            }
            Condition::HasToken { token, value }
            | Condition::ServerHasToken { token, value }
            | Condition::AnyoneHasToken { token, value } => p.tokens.contains(token) == *value,
            Condition::Money { value } => cmp.compare(p.money, *value),
            Condition::Tribute { value } => cmp.compare(p.tribute, *value),
            Condition::TotalDevotion { value } => cmp.compare(p.total_devotion, *value),
            Condition::Challenge { value } => cmp.compare(self.game.challenge, *value),
            // vtable 0x180796870 slot 5 (sub_1804702b0): Player vtable 190
            // = `GetItemCountInStashes`, its complete-only flag the condition's bool (`+120`, variant 1; else false).
            Condition::HasItem { item, count, extra } => cmp.compare(
                p.item_count_in_stashes(item, extra.unwrap_or(false)),
                *count,
            ),
            Condition::KillCreature { count, .. }
            | Condition::KillCreature16 { count, .. }
            | Condition::UsedQuestItem { count, .. } => cmp.compare(counter, *count),
            // sub_180471640 (both types' `Evaluate`): `numKilled <op> count`.
            Condition::KillClassification { count, .. } | Condition::KillFaction { count, .. } => {
                cmp.compare(counter, *count)
            }
            // type 13 `Evaluate` = `xor al,al`; no stock quest or conversation uses it
            // (by design: no stock caller), `Manual` is only satisfied by script.
            Condition::False13 { .. } | Condition::Manual => false,
            Condition::CheckRandomValue { name, value } => {
                self.random_values.get(name) == Some(value)
            }
            Condition::OwnsDlc { value } => self.game.owned_dlc.contains(value),
            Condition::HasMastery { value, flag } => p.masteries.contains(value) == *flag,
            Condition::SkillToggled { skill, value } => {
                p.toggled_skills.contains(&skill.to_ascii_lowercase()) == *value
            }
            Condition::LuaScript {
                function,
                args,
                ret_cmp,
                ret_type,
                want_bool,
                want_int,
                want_string,
            } => {
                let Some(ret) = self.lua_results.get(&lua_key(function, args)) else {
                    return false;
                };
                let cmp = Comparator::from_u32(*ret_cmp);
                match (ret_type, ret) {
                    (0, LuaRet::Bool(b)) => cmp.compare(*b, *want_bool),
                    // The wanted number is stored as `f32` bits and compared
                    // as floats (condition 12 `Evaluate` 0x1804736c0).
                    (1, LuaRet::Number(n)) => cmp.compare(*n as f32, f32::from_bits(*want_int)),
                    (1, LuaRet::Bool(b)) => {
                        cmp.compare(*b as i32 as f32, f32::from_bits(*want_int))
                    }
                    (2, LuaRet::Str(s)) => cmp.compare(s.as_str(), want_string.as_str()),
                    (_, LuaRet::Bool(b)) => cmp.compare(*b, *want_bool),
                    _ => false,
                }
            }
        }
    }

    /// Lua conditions reachable from active tasks (objectives), from incentives/rewards of every task and from
    /// the conversations of the NPCs in the world, so the script host can pre-evaluate them (results go to
    /// `lua_results`). Unique keys, in a fixed order.
    pub fn pending_lua_conditions(&self) -> Vec<Arc<LuaCond>> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let mut push = |list: &[Arc<LuaCond>]| {
            for c in list {
                if seen.insert(c.key) {
                    out.push(c.clone());
                }
            }
        };
        for &(qi, ti) in &self.quests.active {
            push(&self.quests.lua_objectives[qi][ti]);
        }
        // Incentives run when a task begins, rewards when it completes — either
        // can happen between two condition ticks, so every task's are
        // pre-evaluated, not only the active ones'.
        push(&self.quests.lua_events);
        // The conversations of NPCs in the world: a step's `LuaScript` condition decides
        // whether the NPC has anything to say (the Crucible's event NPC asks
        // `gd.survival.eventControl.eventStatus()`), so the dialog code reads `lua_results` too.
        for o in self.entities.objects.values() {
            if !o.in_world {
                continue;
            }
            if let Some(list) = o
                .conversation
                .as_deref()
                .and_then(|c| self.dialogs.lua_conds(c))
            {
                push(list);
            }
        }
        out
    }

    /// A creature died: advance the kill counters of active objectives ([`Kill::counts_for`]).
    pub fn on_kill(&mut self, kill: &Kill) {
        if !kill.by_player || kill.in_dungeon {
            return;
        }
        let rec = normalize_path(&kill.record);
        let QuestLog {
            defs,
            rt,
            active,
            revision,
            ..
        } = &mut self.quests;
        for &(qi, ti) in active.iter() {
            for (oi, o) in defs.list[qi].quest.tasks[ti].objectives.iter().enumerate() {
                for (ci, c) in o.conditions.iter().flat_map(|c| c.list.iter().enumerate()) {
                    if kill.counts_for(&c.cond, &rec) {
                        rt[qi].tasks[ti].objectives[oi].counters[ci] += 1;
                        *revision += 1;
                    }
                }
            }
        }
    }

    /// Runs a `ScriptableActionCollection` (quest events/objectives and
    /// conversation steps). `entity` is the conversation owner, if any.
    pub fn exec_actions(&mut self, actions: &[Action], entity: Option<u32>) {
        for a in actions {
            self.exec_action(a, entity);
        }
    }

    pub fn exec_action(&mut self, a: &Action, entity: Option<u32>) {
        match a {
            Action::BeginQuestTask { quest, task } => self.begin_task_by_path(quest, *task, false),
            Action::CompleteQuestTask { quest, task } => self.complete_task_by_path(quest, *task),
            // `QuestCommandCompleteQuest` → `Quest2::Complete(false)`:
            // every task completed without rewards, quest marked changed.
            Action::QuestCompleted { quest } => {
                if let Some(qi) = self.quests.defs.by_path(quest) {
                    for ti in 0..self.quests.rt[qi].tasks.len() {
                        self.complete_task_index(qi, ti, false);
                        self.quests.active.retain(|&a| a != (qi, ti));
                    }
                    self.quests.mark_changed(qi);
                }
                self.effects.push(Effect::QuestCompleted {
                    quest: quest.clone(),
                });
            }
            Action::GiveMoney { amount } => {
                // The amount is a signed int in the file (a spend is a wrapped u32), never below 0 (like
                // `Player:AdjustMoney`).
                let n = *amount as i32 as i64;
                self.player.money = (self.player.money as i64 + n).clamp(0, u32::MAX as i64) as u32;
                self.effects.push(Effect::GiveMoney(n));
            }
            Action::GiveFaction { faction, amount } => {
                *self
                    .player
                    .factions
                    .entry(faction.to_ascii_uppercase())
                    .or_default() += *amount as f32;
                self.effects.push(Effect::Faction {
                    faction: faction.clone(),
                    delta: *amount as f32,
                    set: false,
                });
                self.effects.push(Effect::UnlockTutorial(51));
            }
            Action::SetFaction { faction, value } => {
                self.player
                    .factions
                    .insert(faction.to_ascii_uppercase(), *value as f32);
                self.effects.push(Effect::Faction {
                    faction: faction.clone(),
                    delta: *value as f32,
                    set: true,
                });
            }
            Action::UnlockFaction { faction } => {
                self.effects.push(Effect::UnlockFaction(faction.clone()))
            }
            Action::GiveItem { item, count } => {
                self.player.add_items(item, *count as i64);
                self.effects.push(Effect::GiveItem {
                    record: item.clone(),
                    count: *count,
                });
            }
            Action::GiveRandomItem { table, count } => self.effects.push(Effect::GiveRandomItem {
                table: table.clone(),
                count: *count,
            }),
            Action::GiveLevel { amount } => {
                self.player.level += amount;
                self.effects.push(Effect::GiveLevels(*amount));
            }
            Action::GiveExperience { amount } => {
                self.player.experience = self.player.experience.saturating_add(*amount);
                self.effects.push(Effect::GiveExperience(*amount));
            }
            // `ScriptableAction_GiveSkillPoint::Execute` @0x18046ceb0 / `GiveAttribPoint` @0x18046aae0:
            // nothing on a difficulty the player skipped with a difficulty unlock item (its points came with it).
            Action::GiveSkillPoint { amount } if !self.skipped(self.difficulty()) => {
                self.effects.push(Effect::GiveSkillPoints(*amount))
            }
            Action::GiveAttribPoint { amount } if !self.skipped(self.difficulty()) => {
                self.effects.push(Effect::GiveAttributePoints(*amount))
            }
            Action::GiveSkillPoint { .. } | Action::GiveAttribPoint { .. } => {}
            Action::GiveDevotion { amount } => {
                self.player.total_devotion += amount;
                self.effects.push(Effect::GiveDevotion(*amount));
            }
            Action::GiveTribute { amount } => {
                // `ScriptableAction_GiveTribute::Execute` @0x18046d160: on the player only; signed (a spend
                // is a wrapped u32): above 0 `Character::AddTribute`, below 0 subtracted (never below 0), 0 empties it.
                let n = *amount as i32 as i64;
                if n == 0 {
                    self.player.tribute = 0;
                } else {
                    self.adjust_tribute(n);
                }
                self.effects.push(Effect::GiveTribute(n));
            }
            Action::GiveToken { token } => self.give_token(token),
            Action::RemoveToken { token } => self.remove_token(token),
            Action::LuaScript { function, args } => {
                let name = function.split('(').next().unwrap_or("").trim().to_string();
                self.lua_calls.push(LuaCall {
                    function: name,
                    args: args.iter().map(lua_arg_value).collect(),
                });
            }
            Action::Log { text } => self.log(text.clone()),
            Action::PlaySound { sound } => self.effects.push(Effect::PlaySound {
                record: sound.clone(),
                at: entity,
            }),
            // type 14 `Execute` 0x18046dd80: not a controller state
            // but `AnimationSet::PlayAnimationIfAvailable(character,
            // AnimationSet_Type value, noName)` on the entity's animation
            // set (`Character+9728` override sets, else the default set
            // `+9592`; slots 0..51). The stock data uses only value 9
            // (PassItem), in the first step of 29 NPC conversations.
            Action::ControllerState { value } => self.effects.push(Effect::PlayAnimation {
                id: entity,
                anim: *value,
            }),
            Action::CinematicText { tag } => self.effects.push(Effect::CinematicText(tag.clone())),
            Action::ScriptEvent { name } => self.effects.push(Effect::ScriptEvent(name.clone())),
            Action::GenerateRandomValue { name, min, max } => self.random_value(name, *min, *max),
            Action::CastSkill { skill } => self.effects.push(Effect::CastSkill {
                skill: skill.clone(),
                caster: entity,
            }),
            Action::UnlockTutorial { page } => self.effects.push(Effect::UnlockTutorial(*page)),
            // PORT: `ScriptableAction_PlayVideo` (the expansion intros): the port plays no videos.
            Action::PlayVideo { .. } => {}
        }
    }

    /// Whether difficulty `d` was skipped with a difficulty unlock item ([`super::state::Player::difficulty_skip`]).
    pub fn skipped(&self, d: usize) -> bool {
        d < 2 && self.player.difficulty_skip & (1 << d) != 0
    }

    /// The campaign's difficulty unlock item (`difficultyunlock_elite` / `_ultimate`) used for difficulty
    /// `unlocked` (1 Elite, 2 Ultimate) `ItemDifficultyUnlock::Use` @0x180336ef0: the difficulties it
    /// skips are Normal (unless skipped before) for Elite, and Normal (unless skipped before) and Elite for
    /// Ultimate; each is marked skipped (`PlayStats` +100 bits 1 / 2, [`super::state::Player::difficulty_skip`]). For every task
    /// of the record's `statQuests` that is not complete on a skipped difficulty and whose rewards were not given
    /// there (`+163`), the `GiveSkillPoint` / `GiveAttribPoint` actions of its reward events add up: the points
    /// the player gets now. Returns the skipped difficulties and (skill points, attribute points).
    pub fn skip_difficulties(
        &mut self,
        unlocked: usize,
        quests: &[&str],
    ) -> (Vec<usize>, u32, u32) {
        let mut skipped = Vec::new();
        if unlocked == 1 || (unlocked == 2 && !self.skipped(0)) {
            skipped.push(0);
            self.player.difficulty_skip |= 1;
        }
        if unlocked == 2 {
            skipped.push(1);
            self.player.difficulty_skip |= 2;
        }
        let (mut skill, mut attrib) = (0u32, 0u32);
        for q in quests {
            let Some(qi) = self.quests.defs.by_path(q) else {
                continue;
            };
            for (ti, task) in self.quests.defs.list[qi].quest.tasks.iter().enumerate() {
                let rt = &self.quests.rt[qi].tasks[ti];
                for &d in &skipped {
                    if rt.per_difficulty[d] == STATE_COMPLETE || rt.rewarded[d] {
                        continue;
                    }
                    for a in task.rewards.iter().flat_map(|e| &e.actions) {
                        match a {
                            Action::GiveSkillPoint { amount } => skill += amount,
                            Action::GiveAttribPoint { amount } => attrib += amount,
                            _ => {}
                        }
                    }
                }
            }
        }
        (skipped, skill, attrib)
    }

    /// The player's tributes by `n` (`Player:AdjustTribute`, the quest action). `Character::AddTribute`
    /// @0x180062290: a gain caps the count at 150 and, when it rose, sends GameEvent 63 (the HUD's tribute glow:
    /// `Survival::tribute_gains`); a loss (`SubtractTribute`) stops at 0 without an event.
    pub fn adjust_tribute(&mut self, n: i64) {
        let before = self.player.tribute;
        if n > 0 {
            let v = (before as i64 + n).min(MAX_TRIBUTE as i64) as u32;
            if v > before {
                self.game.survival.tribute_gains += 1;
            }
            self.player.tribute = v;
        } else {
            self.player.tribute = (before as i64 + n).max(0) as u32;
        }
    }

    pub fn give_token(&mut self, token: &str) {
        // `Player::BestowToken` → `PlayStats::TokenGranted` (the Crucible's counters).
        self.game.survival.token_granted(token);
        self.player.tokens.insert(token.to_string());
    }

    pub fn remove_token(&mut self, token: &str) {
        self.player.tokens.remove(token);
    }
}

/// A Lua function call queued by a quest/conversation action.
#[derive(Debug, Clone, PartialEq)]
pub struct LuaCall {
    pub function: String,
    pub args: Vec<LuaValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LuaValue {
    Nil,
    Bool(bool),
    Number(f64),
    Str(String),
}

/// Lua action/condition argument → value. (Game.dll 0x18036af60,
/// shared by `ScriptableAction_LuaScript` and condition 12): 0/1 the number 0,
/// 2 the flag as a boolean, 3 the `f32` bits as a number, 4 the string (all
/// stock calls use 4), anything else nil.
pub fn lua_arg_value(a: &LuaArg) -> LuaValue {
    match a.kind {
        0 | 1 => LuaValue::Number(0.0),
        2 => LuaValue::Bool(a.flag),
        3 => LuaValue::Number(f32::from_bits(a.int) as f64),
        4 => LuaValue::Str(a.text.clone()),
        _ => LuaValue::Nil,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::{Dialogs, Player};
    use gd_formats::qst::{Event, Objective, Task, Texts};

    fn cond(kind: u32, cmp: Comparator, cond: Condition) -> Conditions {
        Conditions {
            mode: 0,
            list: vec![ConditionEntry {
                kind,
                variant: 0,
                comparator: cmp,
                cond,
            }],
        }
    }

    fn task(
        uid: u32,
        incentives: Vec<Event>,
        objectives: Vec<Objective>,
        rewards: Vec<Event>,
    ) -> Task {
        Task {
            uid,
            flag: 0,
            name_text: 0,
            desc_text: 0,
            incentives,
            objectives,
            rewards,
            blocker: false,
            flag194: false,
            hash: [0; 16],
        }
    }

    fn event(actions: Vec<Action>) -> Event {
        Event {
            text: 0,
            unknown: 0,
            conditions: None,
            actions,
        }
    }

    fn objective(uid: u32, c: Conditions) -> Objective {
        Objective {
            text: 0,
            uid,
            unknown: 0,
            conditions: Some(c),
            actions: vec![],
        }
    }

    fn story(flags: u32) -> Story {
        let quest = Quest {
            version: 9,
            header_unknown: 0,
            id: 77,
            flags,
            tasks: vec![
                task(
                    1,
                    vec![event(vec![Action::GiveToken {
                        token: "A_STARTED".into(),
                    }])],
                    vec![objective(
                        10,
                        cond(
                            7,
                            Comparator::Equal,
                            Condition::HasToken {
                                token: "X".into(),
                                value: true,
                            },
                        ),
                    )],
                    vec![event(vec![
                        Action::GiveMoney { amount: 10 },
                        Action::BeginQuestTask {
                            quest: "quests/test.qst".into(),
                            task: 2,
                        },
                    ])],
                ),
                task(
                    2,
                    vec![],
                    vec![objective(
                        20,
                        cond(
                            10,
                            Comparator::GreaterEqual,
                            Condition::KillCreature {
                                records: vec!["records/boss.dbr".into()],
                                count: 2,
                            },
                        ),
                    )],
                    vec![event(vec![Action::GiveExperience { amount: 5 }])],
                ),
                task(
                    3,
                    vec![],
                    vec![objective(
                        30,
                        cond(17, Comparator::Equal, Condition::Manual),
                    )],
                    vec![event(vec![Action::GiveMoney { amount: 1 }])],
                ),
            ],
            texts: Texts::default(),
            hash: [0; 16],
        };
        let mut defs = QuestDefs::default();
        defs.add(QuestDef {
            path: "quests/test.qst".into(),
            quest,
        });
        Story::new(
            Arc::new(defs),
            Dialogs::default(),
            Player::new_character(1, "T"),
        )
    }

    fn state(s: &Story, uid: u32) -> u32 {
        s.quests.task_state(0, s.quests.task_index(0, uid).unwrap())
    }

    #[test]
    fn tasks_progress_and_chain() {
        let mut s = story(0);
        assert_eq!(s.quests.lua_quest_state(77), 2); // Available
        s.begin_task(0, 1, false);
        assert!(s.player.tokens.contains("A_STARTED"), "incentive ran");
        assert_eq!(state(&s, 1), STATE_IN_PROGRESS);
        assert_eq!(s.quests.lua_task_state(77, 1), 3);
        s.update_quests(UPDATE_PERIOD_MS);
        assert_eq!(state(&s, 1), STATE_IN_PROGRESS);
        s.give_token("X");
        s.update_quests(100); // not yet: 500 ms period
        assert_eq!(state(&s, 1), STATE_IN_PROGRESS);
        s.update_quests(400);
        assert_eq!(state(&s, 1), STATE_COMPLETE);
        assert_eq!(s.player.money, 10, "reward ran");
        assert_eq!(
            state(&s, 2),
            STATE_IN_PROGRESS,
            "reward began the next task"
        );
        s.on_kill(&Kill::record_only("Records\\Boss.dbr"));
        s.update_quests(UPDATE_PERIOD_MS);
        assert_eq!(state(&s, 2), STATE_IN_PROGRESS);
        s.on_kill(&Kill::record_only("records/boss.dbr"));
        s.update_quests(UPDATE_PERIOD_MS);
        assert_eq!(state(&s, 2), STATE_COMPLETE);
        assert_eq!(s.player.experience, 5);
        // Begin is ignored for a complete task unless forced.
        s.begin_task(0, 1, false);
        assert_eq!(state(&s, 1), STATE_COMPLETE);
    }
}
