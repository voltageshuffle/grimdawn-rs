//! Quest files (`Quests.arc`, `*.qst`) and the scriptable condition/action
//! objects they share with conversations (`.cnv`).
//!
//! Per Game.dll (x64): `Quest2::Load`,
//! `Quest2Task::Read`, `Quest2Objective::Read`,
//! `Quest2Event::Read`, `ScriptableConditionCollection::Read`,
//! `ScriptableActionCollection::Read`,
//! `ScriptableCondition::Create`, `ScriptableAction::Create`
//! and each type's virtual `Read`. Text block:
//! Engine.dll `PortableLocalization::Read`.
//!
//! Type *names* come from exported class names where they exist
//! (`ScriptableAction_GiveItem`, `ScriptableCondition_HasFaction`, ...), from
//! the type's debug strings (`ScriptableCondition_OnQuestTask evaluates ...`)
//! or from what `Evaluate`/`Execute` touch (e.g. `Character::GetCurrentMoney`
//! offset); for those the behaviour is the engine's but the variant name is the
//! port's own.
use crate::util::{Reader, md5};
use anyhow::{Context, Result, bail};

pub const MAGIC: u32 = 0x3254_5351; // "QST2"

/// Engine-side cap of `BinaryReader` strings (longer ones are skipped).
const MAX_STR: u32 = 0x10_0000;

fn string(r: &mut Reader) -> Result<String> {
    let n = r.u32()?;
    if n > MAX_STR {
        bail!("string length {n:#x} at {:#x}", r.pos - 4);
    }
    Ok(crate::util::latin1(r.bytes(n as usize)?))
}

fn boolean(r: &mut Reader) -> Result<bool> {
    Ok(r.u8()? != 0)
}

/// `ScriptableCondition::GetComparisonText` (0x18046f... ): 0 `<`, 1 `<=`,
/// 2 `=`, 3 `>=`, 4 `>`, 5 `<>`. Numeric conditions compare
/// `actual <op> wanted` (except Difficulty, which compares `wanted <op> actual`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparator {
    Less,
    LessEqual,
    Equal,
    GreaterEqual,
    Greater,
    NotEqual,
    Other(u32),
}

impl Comparator {
    pub fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Less,
            1 => Self::LessEqual,
            2 => Self::Equal,
            3 => Self::GreaterEqual,
            4 => Self::Greater,
            5 => Self::NotEqual,
            o => Self::Other(o),
        }
    }
    pub fn to_u32(self) -> u32 {
        match self {
            Self::Less => 0,
            Self::LessEqual => 1,
            Self::Equal => 2,
            Self::GreaterEqual => 3,
            Self::Greater => 4,
            Self::NotEqual => 5,
            Self::Other(o) => o,
        }
    }
    /// `a <op> b`; unknown comparators are false (engine `default: false`).
    pub fn compare<T: PartialOrd>(self, a: T, b: T) -> bool {
        match self {
            Self::Less => a < b,
            Self::LessEqual => a <= b,
            Self::Equal => a == b,
            Self::GreaterEqual => a >= b,
            Self::Greater => a > b,
            Self::NotEqual => a != b,
            Self::Other(_) => false,
        }
    }
}

/// One argument of a Lua condition/action (48-byte engine record:
/// `{u32 kind, u8, f32, string}`) (the arg→Lua conversion shared by
/// `ScriptableAction_LuaScript` and condition 12, Game.dll `ScriptableLua::ArgumentsToValues`): kind
/// 0/1 push the number 0, 2 pushes `flag` as a boolean, 3 pushes `int`
/// reinterpreted as an `f32` number, 4 pushes `text`; any other kind is nil.
#[derive(Debug, Clone, PartialEq)]
pub struct LuaArg {
    pub kind: u32,
    pub flag: bool,
    pub int: u32,
    pub text: String,
}

fn lua_args(r: &mut Reader) -> Result<Vec<LuaArg>> {
    let n = r.u32()?;
    (0..n)
        .map(|_| {
            Ok(LuaArg {
                kind: r.u32()?,
                flag: boolean(r)?,
                int: r.u32()?,
                text: string(r)?,
            })
        })
        .collect()
}

/// Typed condition payload, by `ScriptableCondition::GetType`.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// 0 `ScriptableCondition_OnQuest`: quest started (in progress or done) == value.
    OnQuest { quest: String, value: bool },
    /// 1 `ScriptableCondition_QuestTaskComplete`.
    QuestTaskComplete {
        quest: String,
        task: u32,
        value: bool,
    },
    /// 2 player experience (`Character.experiencePoints`, `GetExperiencePoints`) <op> value. No class name exists.
    Experience { value: u32 },
    /// 3 player level (`Character.charLevel`, `SetCharLevel`) <op> value. No class name exists.
    Level { value: u32 },
    /// 4 game difficulty: `value <op> difficulty`.
    Difficulty { value: u32 },
    /// 5 `Player::IsHardcore() == value`.
    Hardcore { value: bool },
    /// 6 `ScriptableCondition_HasFaction`: faction value <op> value.
    HasFaction { faction: String, value: i32 },
    /// 7 `Player::HasToken(token) == value`.
    HasToken { token: String, value: bool },
    /// 8 money (`Character::GetCurrentMoney`) <op> value.
    Money { value: u32 },
    /// 9 item count in the player's inventory <op> count; `extra` only for variant 1.
    HasItem {
        item: String,
        count: u32,
        extra: Option<bool>,
    },
    /// 10 `ScriptableCondition_HasKilled`: counter over a list of creature
    /// records (lower-cased, `/`), stored progress `u32` (`numKilled`) in the
    /// save. Its vtable (`ScriptableCondition_HasKilled::vftable`) registers with the
    /// `EventManager` and localizes `tagConditionHasKilled`.
    KillCreature { records: Vec<String>, count: u32 },
    /// 11 `ScriptableCondition_HasQuestObjective`; `objective` only for variant 1.
    HasQuestObjective {
        quest: String,
        task: u32,
        objective: Option<u32>,
        value: bool,
    },
    /// 12 Lua function returning a value compared against `compare` fields.
    /// Return value check (`ScriptableCondition_LuaScript::Evaluate`): `ret_type` 0 bool, 1 number,
    /// 2 string (0 reads the result as a boolean against `want_bool`,
    /// 1 as a number against `want_int` reinterpreted as an `f32`, 2 as a
    /// string; 3 is a nil/type check no shipped file uses); `ret_cmp` is a
    /// [`Comparator`] code.
    LuaScript {
        function: String,
        args: Vec<LuaArg>,
        ret_cmp: u32,
        ret_type: u32,
        want_bool: bool,
        want_int: u32,
        want_string: String,
    },
    /// 13 always false (`Evaluate` = `xor al,al`); payload one byte.
    False13 { value: bool },
    /// 14 `ScriptableCondition_OnQuestTask`: task in progress == value.
    OnQuestTask {
        quest: String,
        task: u32,
        value: bool,
    },
    /// 15 `ScriptableCondition_UsedQuestItem`: use count of a quest item with
    /// stored progress (`count`). Named from its debug text
    /// (`UsedQuestItem '%s' ...`, `tagQuestObjectiveUsedQuestItem*`, vtable
    /// `ScriptableCondition_UsedQuestItem::vftable`). No shipped quest uses it.
    UsedQuestItem { name: String, count: u32 },
    /// 16 a `HasKilled` subclass (same vtable slots plus an event handler,
    /// `ScriptableCondition_HasKilledSpawned::vftable`): it also listens for spawned monsters and remembers the
    /// ids of those whose spawn source matches `records`, counting their
    /// deaths. Class name not tied (likely `HasKilledProxy`); no shipped quest
    /// uses it, so the port counts it like 10.
    KillCreature16 { records: Vec<String>, count: u32 },
    /// 17 no payload, `Evaluate` = false and skipped by `AreObjectivesMet`:
    /// objective completed by an explicit CompleteQuestTask (talk to X ...).
    Manual,
    /// 18 `Quest2Repository::ServerHasToken(token) == value`.
    ServerHasToken { token: String, value: bool },
    /// 19 `ScriptableCondition_QuestBlocked`.
    QuestBlocked { quest: String, value: bool },
    /// 20 `Quest2Repository::AnyoneHasToken(token) == value`.
    AnyoneHasToken { token: String, value: bool },
    /// 21 `ScriptableCondition_CheckRandomValue`: named random value == value.
    CheckRandomValue { name: String, value: u32 },
    /// 22 `Steamworks::OwnsDLC(value)`.
    OwnsDlc { value: u32 },
    /// 23 tribute (`GetCurrentTribute`) <op> value.
    Tribute { value: u32 },
    /// 24 total devotion points <op> value.
    TotalDevotion { value: u32 },
    /// 25 player has mastery `value` (SkillManager::GetSkillMasteries) == flag.
    HasMastery { value: u32, flag: bool },
    /// 26 game challenge (`GameInfo::GetChallenge`) <op> value.
    Challenge { value: u32 },
    /// 27 `SkillManager::IsSkillToggled(skill) == value`.
    SkillToggled { skill: String, value: bool },
    /// 28 kill counter by monster classification (`ScriptableCondition::Create`
    /// case 28, vtable `ScriptableCondition_KillClassification::vftable`: `Read`
    /// `ScriptableCondition_KillClassification::Read` = `u32 class0, class1, count` into `class0` / `class1` /
    /// `count`; the counter `numKilled` is the saved word; `Evaluate`
    /// `ScriptableCondition_KillClassification::Evaluate` = `numKilled <op> count`; text
    /// `tagConditionHasKilled`). The death handler (`ScriptableCondition_KillClassification::OnCreatureDeath`, event
    /// 11, only with `IsExpansion3Loaded`) counts a `Monster` whose `GetClassification` (`Monster.classification`,
    /// `GameEngine::ResolveMonsterClassificationEnum`: Common 0, Champion 1, Hero 2, Boss 3, Quest 4,
    /// SuperBoss 5) is either class; the killer and victim rules are `gd_story::story::Kill`'s.
    /// Stock: `bq_av06` (Boss / Quest, 8 kills).
    KillClassification { classes: [u32; 2], count: u32 },
    /// 29 kill counter by faction (`Create` case 29, vtable `ScriptableCondition_KillFaction::vftable`, same
    /// layout and `Read`; the constructor sets both factions to -1000 = none): the death handler
    /// (`ScriptableCondition_KillFaction::OnCreatureDeath`) counts a `Character` whose `GetOriginalFaction`
    /// (`Monster.myFaction` = the `myFaction` of its record's `factions` pack, `FactionPack::GetFactionFromString` order:
    /// Player 0, Survivors 1, Aetherials 2 ... User0 6 ... User12 18) is not -1000 and is either
    /// faction. Stock (200 kills each): `bq_av07` Aetherials or User12 (Aetherial Vanguard),
    /// `bq_av08` Cthonians or User18 (Chthonic Kurn), `bq_av09` User16 (Eldritch) only (second -1000).
    KillFaction { factions: [u32; 2], count: u32 },
}

/// A condition as stored: `u32 type, u8 variant, u32 comparator, payload`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionEntry {
    pub kind: u32,
    /// Second argument of `ScriptableCondition::Create`; selects optional
    /// fields in types 9 and 11.
    pub variant: u8,
    pub comparator: Comparator,
    pub cond: Condition,
}

impl ConditionEntry {
    /// Number of `u32` progress words this condition writes into a save's
    /// task block (virtual `WriteProperties`; only 10/15/16/28/29).
    pub fn saved_words(&self) -> usize {
        matches!(self.kind, 10 | 15 | 16 | 28 | 29) as usize
    }
}

fn read_condition(r: &mut Reader) -> Result<ConditionEntry> {
    let kind = r.u32()?;
    let variant = r.u8()?;
    let comparator = Comparator::from_u32(r.u32()?);
    let cond = match kind {
        0 => Condition::OnQuest {
            quest: string(r)?,
            value: boolean(r)?,
        },
        1 => Condition::QuestTaskComplete {
            quest: string(r)?,
            task: r.u32()?,
            value: boolean(r)?,
        },
        2 => Condition::Experience { value: r.u32()? },
        3 => Condition::Level { value: r.u32()? },
        4 => Condition::Difficulty { value: r.u32()? },
        5 => Condition::Hardcore { value: boolean(r)? },
        6 => Condition::HasFaction {
            faction: string(r)?,
            value: r.i32()?,
        },
        7 => Condition::HasToken {
            token: string(r)?,
            value: boolean(r)?,
        },
        8 => Condition::Money { value: r.u32()? },
        9 => {
            let item = string(r)?;
            let count = r.u32()?;
            let extra = if variant == 1 {
                Some(boolean(r)?)
            } else {
                None
            };
            Condition::HasItem { item, count, extra }
        }
        10 | 16 => {
            let n = r.u32()?;
            let records = (0..n)
                .map(|_| string(r).map(|s| s.replace('\\', "/").to_ascii_lowercase()))
                .collect::<Result<_>>()?;
            let count = r.u32()?;
            if kind == 10 {
                Condition::KillCreature { records, count }
            } else {
                Condition::KillCreature16 { records, count }
            }
        }
        11 => {
            let quest = string(r)?;
            let task = r.u32()?;
            let objective = if variant == 1 { Some(r.u32()?) } else { None };
            Condition::HasQuestObjective {
                quest,
                task,
                objective,
                value: boolean(r)?,
            }
        }
        12 => {
            let function = string(r)?;
            let args = lua_args(r)?;
            let ret_cmp = r.u32()?;
            let ret_type = r.u32()?;
            let want_bool = boolean(r)?;
            let want_int = r.u32()?;
            let want_string = string(r)?;
            Condition::LuaScript {
                function,
                args,
                ret_cmp,
                ret_type,
                want_bool,
                want_int,
                want_string,
            }
        }
        13 => Condition::False13 { value: boolean(r)? },
        14 => Condition::OnQuestTask {
            quest: string(r)?,
            task: r.u32()?,
            value: boolean(r)?,
        },
        15 => Condition::UsedQuestItem {
            name: string(r)?.to_ascii_lowercase(),
            count: r.u32()?,
        },
        17 => Condition::Manual,
        18 => Condition::ServerHasToken {
            token: string(r)?,
            value: boolean(r)?,
        },
        19 => Condition::QuestBlocked {
            quest: string(r)?,
            value: boolean(r)?,
        },
        20 => Condition::AnyoneHasToken {
            token: string(r)?,
            value: boolean(r)?,
        },
        21 => Condition::CheckRandomValue {
            name: string(r)?,
            value: r.u32()?,
        },
        22 => Condition::OwnsDlc { value: r.u32()? },
        23 => Condition::Tribute { value: r.u32()? },
        24 => Condition::TotalDevotion { value: r.u32()? },
        25 => Condition::HasMastery {
            value: r.u32()?,
            flag: boolean(r)?,
        },
        26 => Condition::Challenge { value: r.u32()? },
        27 => Condition::SkillToggled {
            skill: string(r)?,
            value: boolean(r)?,
        },
        28 => Condition::KillClassification {
            classes: [r.u32()?, r.u32()?],
            count: r.u32()?,
        },
        29 => Condition::KillFaction {
            factions: [r.u32()?, r.u32()?],
            count: r.u32()?,
        },
        k => bail!("unknown condition type {k} at {:#x}", r.pos - 9),
    };
    Ok(ConditionEntry {
        kind,
        variant,
        comparator,
        cond,
    })
}

/// `ScriptableConditionCollection`: `mode` 0 = all must hold, 1 = any.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Conditions {
    pub mode: u32,
    pub list: Vec<ConditionEntry>,
}

impl Conditions {
    /// Reads `u32 n` and, when `n > 0`, `u32 mode` + n conditions.
    pub fn read(r: &mut Reader) -> Result<Option<Self>> {
        let n = r.u32()?;
        if n == 0 {
            return Ok(None);
        }
        let mode = r.u32()?;
        let list = (0..n).map(|_| read_condition(r)).collect::<Result<_>>()?;
        Ok(Some(Self { mode, list }))
    }
    pub fn any(&self) -> bool {
        self.mode == 1
    }
}

/// Typed action payload, by `ScriptableAction::GetType`.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// 1 `ScriptableAction_BeginQuestTask`.
    BeginQuestTask { quest: String, task: u32 },
    /// 2 quest file handed to a GameEngine subsystem (`GameEngine.outboundInterface`), always
    /// the last reward of a quest's final task. The slot is
    /// `QuestCommandCompleteQuest`, whose handler
    /// (`GameEngineOutboundInterface::QuestCommandCompleteQuest`) runs
    /// `Quest2::Complete(false)`: every task completed without rewards.
    QuestCompleted { quest: String },
    /// 3 `Quest2Repository::CompleteQuestTask`.
    CompleteQuestTask { quest: String, task: u32 },
    /// 4 `ScriptableAction_GiveMoney`.
    GiveMoney { amount: u32 },
    /// 5 `AdjustFactionConfigCmd` (faction, delta); also unlocks tutorial page 51.
    GiveFaction { faction: String, amount: i32 },
    /// 6 `ScriptableAction_GiveItem`; a negative count takes items.
    GiveItem { item: String, count: i32 },
    /// 7 `ScriptableAction_GiveLevel`.
    GiveLevel { amount: u32 },
    /// 8 `ScriptableAction_GiveExperience`.
    GiveExperience { amount: u32 },
    /// 9 `Player::BestowToken`.
    GiveToken { token: String },
    /// 10 `Player::RemoveToken`.
    RemoveToken { token: String },
    /// 11 `ScriptableAction_LuaScript` (debug string): call a Lua function.
    LuaScript { function: String, args: Vec<LuaArg> },
    /// 12 engine log line.
    Log { text: String },
    /// 13 play a SoundPak at the entity.
    PlaySound { sound: String },
    /// 14 plays an `AnimationSet_Type` slot on the entity (`ScriptableAction_ControllerState`'s
    /// `Execute` calls `AnimationSet::PlayAnimationIfAvailable`).
    ControllerState { value: u32 },
    /// 15 `ScriptableAction_GiveSkillPoint`.
    GiveSkillPoint { amount: u32 },
    /// 17 `Engine::ShowCinematicText(Localize(tag))`.
    CinematicText { tag: String },
    /// 18 `ScriptableAction_GiveAttribPoint`.
    GiveAttribPoint { amount: u32 },
    /// 19 `GameEngine::ScriptEvent(name)`.
    ScriptEvent { name: String },
    /// 20 `ScriptableAction_GiveRandomItem` (loot table record, count).
    GiveRandomItem { table: String, count: u32 },
    /// 21 `ScriptableAction_GenerateRandomValue` (name, min, max).
    GenerateRandomValue { name: String, min: u32, max: u32 },
    /// 22 `ScriptableAction_CastSkill`.
    CastSkill { skill: String },
    /// 23 marks a faction as unlocked/visible on the player.
    UnlockFaction { faction: String },
    /// 24 `FactionPack::SetValue`.
    SetFaction { faction: String, value: i32 },
    /// 25 `ScriptableAction_GiveDevotion`.
    GiveDevotion { amount: u32 },
    /// 26 `ScriptableAction_GiveTribute`.
    GiveTribute { amount: u32 },
    /// 27 `ScriptableAction_UnlockTutorial`.
    UnlockTutorial { page: u32 },
    /// 28 `ScriptableAction_PlayVideo`.
    PlayVideo { video: String, flag: bool },
}

impl Action {
    /// Types the engine counts as rewards: `ScriptableAction::IsReward`
    /// (types 4, 5, 6, 7, 8, 15, 18, 20, 25, 26).
    pub fn is_reward(&self) -> bool {
        matches!(
            self,
            Action::GiveMoney { .. }
                | Action::GiveItem { .. }
                | Action::GiveLevel { .. }
                | Action::GiveExperience { .. }
                | Action::GiveSkillPoint { .. }
                | Action::GiveAttribPoint { .. }
                | Action::GiveRandomItem { .. }
                | Action::GiveDevotion { .. }
                | Action::GiveTribute { .. }
                | Action::GiveFaction { .. }
        )
    }
}

fn read_action(r: &mut Reader) -> Result<Action> {
    let kind = r.u32()?;
    let _variant = r.u8()?;
    Ok(match kind {
        1 => Action::BeginQuestTask {
            quest: string(r)?,
            task: r.u32()?,
        },
        2 => Action::QuestCompleted { quest: string(r)? },
        3 => Action::CompleteQuestTask {
            quest: string(r)?,
            task: r.u32()?,
        },
        4 => Action::GiveMoney { amount: r.u32()? },
        5 => Action::GiveFaction {
            faction: string(r)?,
            amount: r.i32()?,
        },
        6 => Action::GiveItem {
            item: string(r)?,
            count: r.i32()?,
        },
        7 => Action::GiveLevel { amount: r.u32()? },
        8 => Action::GiveExperience { amount: r.u32()? },
        9 => Action::GiveToken { token: string(r)? },
        10 => Action::RemoveToken { token: string(r)? },
        11 => Action::LuaScript {
            function: string(r)?,
            args: lua_args(r)?,
        },
        12 => Action::Log { text: string(r)? },
        13 => Action::PlaySound { sound: string(r)? },
        14 => Action::ControllerState { value: r.u32()? },
        15 => Action::GiveSkillPoint { amount: r.u32()? },
        17 => Action::CinematicText { tag: string(r)? },
        18 => Action::GiveAttribPoint { amount: r.u32()? },
        19 => Action::ScriptEvent { name: string(r)? },
        20 => Action::GiveRandomItem {
            table: string(r)?,
            count: r.u32()?,
        },
        21 => Action::GenerateRandomValue {
            name: string(r)?,
            min: r.u32()?,
            max: r.u32()?,
        },
        22 => Action::CastSkill { skill: string(r)? },
        23 => Action::UnlockFaction {
            faction: string(r)?,
        },
        24 => Action::SetFaction {
            faction: string(r)?,
            value: r.i32()?,
        },
        25 => Action::GiveDevotion { amount: r.u32()? },
        26 => Action::GiveTribute { amount: r.u32()? },
        27 => Action::UnlockTutorial { page: r.u32()? },
        28 => Action::PlayVideo {
            video: string(r)?,
            flag: boolean(r)?,
        },
        k => bail!("unknown action type {k} at {:#x}", r.pos - 5),
    })
}

/// Reads `u32 n` + n actions.
pub fn read_actions(r: &mut Reader) -> Result<Vec<Action>> {
    let n = r.u32()?;
    (0..n).map(|_| read_action(r)).collect()
}

/// `Quest2Event`: an incentive (run when a task starts) or reward (run when it
/// completes) — conditions gate the actions.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Index into the quest's text table.
    pub text: u32,
    pub unknown: u32,
    pub conditions: Option<Conditions>,
    pub actions: Vec<Action>,
}

/// `Quest2Objective`: conditions that must hold; actions run when it becomes
/// satisfied.
#[derive(Debug, Clone, PartialEq)]
pub struct Objective {
    pub text: u32,
    /// Objective uid: `Quest2Objective::Read` stores the first u32 at
    /// +16, which `GetUid` / `Quest2Task::GetObjectiveByUid` compare.
    pub uid: u32,
    pub unknown: u32,
    pub conditions: Option<Conditions>,
    pub actions: Vec<Action>,
}

impl Objective {
    pub fn saved_words(&self) -> usize {
        self.conditions
            .as_ref()
            .map_or(0, |c| c.list.iter().map(|c| c.saved_words()).sum())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub uid: u32,
    /// `Quest2Task::GetTaskFlag`.
    pub flag: u32,
    pub name_text: u32,
    pub desc_text: u32,
    pub incentives: Vec<Event>,
    pub objectives: Vec<Objective>,
    pub rewards: Vec<Event>,
    /// v>=8 (`IsBlocker`).
    pub blocker: bool,
    /// v>=9, `Quest2Task.flag194` (unknown).
    pub flag194: bool,
    /// MD5 of the task's bytes (uid .. rewards, without the v8/v9 flags):
    /// the task uid stored in `quests.gdd`; a mismatch makes the engine drop
    /// the saved objective state (`Quest2::ReadProperties`).
    pub hash: [u8; 16],
}

/// Language block of a quest/conversation file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Texts {
    pub languages: Vec<(String, Vec<String>)>,
}

impl Texts {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let n = r.u32()?;
        let mut languages = Vec::new();
        for _ in 0..n {
            let code = string(r)?;
            let m = r.u32()?;
            let strings = (0..m).map(|_| string(r)).collect::<Result<_>>()?;
            languages.push((code, strings));
        }
        Ok(Self { languages })
    }
    /// A text table from a line-ordered override file
    /// ([`crate::text::load_text_override`], Engine.dll
    /// `PortableLocalization::Read`): the lines replace the embedded tables
    /// wholesale; [`Self::get`] serves them.
    pub fn from_lines(lines: Vec<String>) -> Self {
        Self {
            languages: vec![("override".into(), lines)],
        }
    }
    /// Text `i` of the first table — the embedded English, or the session
    /// language's override ([`Self::from_lines`]); out-of-range → `""`
    /// (`Quest2::GetText` → noTag). `{^n}` etc. markup kept.
    pub fn get(&self, i: u32) -> &str {
        self.languages
            .first()
            .and_then(|(_, t)| t.get(i as usize))
            .map_or("", |s| s.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Quest {
    pub version: u32,
    pub header_unknown: u32,
    /// Quest id (save files key quests by it).
    pub id: u32,
    /// `Quest2.flags`: bit 2 (value 4) = repeatable (Quest2::Update
    /// resets the quest when all tasks are complete).
    pub flags: u32,
    pub tasks: Vec<Task>,
    pub texts: Texts,
    /// MD5 of the whole file: the quest uid stored in `quests.gdd`.
    pub hash: [u8; 16],
}

fn read_event(r: &mut Reader, idx: &mut u32) -> Result<Event> {
    *idx += 1;
    let text = *idx;
    let unknown = r.u32()?;
    let conditions = Conditions::read(r)?;
    let actions = read_actions(r)?;
    Ok(Event {
        text,
        unknown,
        conditions,
        actions,
    })
}

fn read_task(r: &mut Reader, idx: &mut u32, version: u32) -> Result<Task> {
    let start = r.pos;
    *idx += 1;
    let name_text = *idx;
    *idx += 1;
    let desc_text = *idx;
    let uid = r.u32()?;
    let flag = r.u32()?;
    let n = r.u32()?;
    let incentives = (0..n).map(|_| read_event(r, idx)).collect::<Result<_>>()?;
    let n = r.u32()?;
    let mut objectives = Vec::new();
    for _ in 0..n {
        *idx += 1;
        let text = *idx;
        let uid = r.u32()?;
        let unknown = r.u32()?;
        let conditions = Conditions::read(r)?;
        let actions = read_actions(r)?;
        objectives.push(Objective {
            text,
            uid,
            unknown,
            conditions,
            actions,
        });
    }
    let n = r.u32()?;
    let rewards = (0..n).map(|_| read_event(r, idx)).collect::<Result<_>>()?;
    let hash = md5(&r.data[start..r.pos]);
    let blocker = if version >= 8 { boolean(r)? } else { false };
    let flag194 = if version >= 9 { boolean(r)? } else { false };
    Ok(Task {
        uid,
        flag,
        name_text,
        desc_text,
        incentives,
        objectives,
        rewards,
        blocker,
        flag194,
        hash,
    })
}

impl Quest {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let magic = r.u32()?;
        if magic != MAGIC {
            bail!("not a QST2 file (magic {magic:#x})");
        }
        let version = r.u32()?;
        if version < 7 {
            bail!("quest version {version} < 7");
        }
        let header_unknown = r.u32()?;
        let id = r.u32()?;
        let flags = r.u32()?;
        let n = r.u32()?;
        // Texts 0/1 are the quest group and name; tasks start at 2.
        let mut idx = 1;
        let mut tasks = Vec::new();
        for i in 0..n {
            tasks.push(read_task(&mut r, &mut idx, version).with_context(|| format!("task {i}"))?);
        }
        let texts = Texts::read(&mut r).context("text block")?;
        if r.remaining() != 0 {
            bail!("{} trailing bytes", r.remaining());
        }
        Ok(Self {
            version,
            header_unknown,
            id,
            flags,
            tasks,
            texts,
            hash: md5(data),
        })
    }
    pub fn group(&self) -> &str {
        self.texts.get(0)
    }
    pub fn name(&self) -> &str {
        self.texts.get(1)
    }
    pub fn task(&self, uid: u32) -> Option<&Task> {
        self.tasks.iter().find(|t| t.uid == uid)
    }
    pub fn repeatable(&self) -> bool {
        self.flags & 4 != 0
    }
}
