//! Achievements: the list (`gameengine.dbr`'s `achievementRecord`, its `achievementGroups` each with a `button`
//! record and an `achievements` list) and which are unlocked ([`Achievements`]). Game.dll
//! `AchievementManager` / `Achievement`:
//! - The unlock state is Steam's only: nothing is saved locally and the manager lives for the process (no session
//!   reset); without Steam every achievement starts locked.
//! - An unlock (of a locked, listed id) shows `tagAchievementUnlocked` over the `title` (type 1) and sends
//!   [`AchievementUnlocked`].
//! - Script events unlock by name ([`script_event_unlocks`]); `Token` ones when the player gets a token in their
//!   `tokenName` list ([`token_unlocks`]).
//! - `Kill`: the player's or his pet's kill of a monster that gives experience and is not in a Shattered Realm
//!   dungeon counts for each locked achievement whose `killDbrs` holds the victim, when `killHardcore` matches and
//!   the difficulty is at least `killDifficulty` ([`at_least_difficulty`]); it unlocks at `killCount`
//!   ([`kill_matches`], [`KillCounts`]: process life).
//! - `Stat` / `Faction`: every locked one is checked every 3 s ([`stat_unlocks`], [`StatContext`],
//!   [`faction_unlocks`]).
//!
//! PORT: tokens are seen as new entries in the story player's set. The engine's periodic checks wait for a Steam
//! sync; here they always run, and the loaded save stands in for that sync: tokens held at story start and what
//! the session's first periodic check finds unlock silently ([`SyncAchievement`]). `Stat` counters are the loaded
//! save's plus this session's [`crate::play_stats::PlayCounts`]. The dungeon test is the monster's proxy being one
//! the running dungeon activated. `NUM_PETS_ACTIVE` counts the player's live pets.
use bevy::prelude::*;

/// The game engine record (its `achievementRecord`).
pub const ENGINE: &str = "records/game/gameengine.dbr";

/// One achievement record (`achievement.tpl`).
#[derive(Clone, Debug, PartialEq)]
pub struct Achievement {
    pub id: String,
    /// Text tags.
    pub title: String,
    pub description: String,
    /// Not listed while locked (the window, `AchievementWindow::RefreshList`).
    pub hidden: bool,
    pub locked_icon: String,
    pub unlocked_icon: String,
    /// `type`: `Unlock`, `Token`, `Kill`, `Stat` or `Faction`.
    pub kind: String,
    /// `tokenName` (a `Token` achievement).
    pub tokens: Vec<String>,
    /// `Kill`: the victim records (`killDbrs`), how many kills (`killCount`, default 1), the displayed difficulty
    /// (`killDifficulty`, [`at_least_difficulty`]) and the hardcore-only flag (`killHardcore`).
    pub kill_dbrs: Vec<String>,
    pub kill_count: u32,
    pub kill_difficulty: u32,
    pub kill_hardcore: bool,
    /// `Stat`: the counter (`statName`), its threshold (`statCount`), and for `NUM_SKILL_TRACKING` /
    /// `NUM_BUFFS_ACTIVE` the skill records (`skillNames`) and `requiresAll`.
    pub stat_name: String,
    pub stat_count: u32,
    pub skill_names: Vec<String>,
    pub requires_all: bool,
    /// `Faction`: how many factions (`factionCount`) at which reputation (`factionValue`), of which
    /// (`factionNames`; empty: every faction but the player's).
    pub faction_count: u32,
    pub faction_value: i32,
    pub faction_names: Vec<String>,
}

/// A group: its button (`ButtonStaticText2` record) and achievements, in record order.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub button: String,
    pub achievements: Vec<Achievement>,
}

/// The groups of the achievement list (records missing from the database are skipped).
pub fn groups(db: &gd_formats::vfs::Database) -> Vec<Group> {
    let Some(list) = db
        .get(ENGINE)
        .and_then(|e| e.str("achievementRecord").map(str::to_owned))
        .and_then(|r| db.get(&r))
    else {
        return Vec::new();
    };
    list.strs("achievementGroups")
        .iter()
        .filter_map(|g| db.get(g))
        .map(|g| Group {
            button: g.str("button").unwrap_or("").to_owned(),
            achievements: g
                .strs("achievements")
                .iter()
                .filter_map(|a| db.get(a))
                .map(|a| {
                    let s = |k: &str| a.str(k).unwrap_or("").to_owned();
                    Achievement {
                        id: s("id"),
                        title: s("title"),
                        description: s("description"),
                        hidden: a.bool("hidden"),
                        locked_icon: s("lockedIcon"),
                        unlocked_icon: s("unlockedIcon"),
                        kind: s("type"),
                        tokens: a.strs("tokenName").iter().map(|t| t.to_string()).collect(),
                        kill_dbrs: a.strs("killDbrs").iter().map(|t| t.to_string()).collect(),
                        kill_count: a.i32("killCount").unwrap_or(1).max(0) as u32,
                        kill_difficulty: a.i32("killDifficulty").unwrap_or(0).max(0) as u32,
                        kill_hardcore: a.bool("killHardcore"),
                        stat_name: s("statName"),
                        stat_count: a.i32("statCount").unwrap_or(0).max(0) as u32,
                        skill_names: a.strs("skillNames").iter().map(|t| t.to_string()).collect(),
                        requires_all: a.bool("requiresAll"),
                        faction_count: a.i32("factionCount").unwrap_or(0).max(0) as u32,
                        faction_value: a.i32("factionValue").unwrap_or(0),
                        faction_names: a
                            .strs("factionNames")
                            .iter()
                            .map(|t| t.to_string())
                            .collect(),
                    }
                })
                .collect(),
        })
        .collect()
}

/// The ids unlocked in this process (module doc: a build without Steam has no other source).
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct Achievements {
    pub unlocked: Vec<String>,
}

impl Achievements {
    pub fn is_unlocked(&self, id: &str) -> bool {
        self.unlocked.iter().any(|u| u == id)
    }
}

/// The `Kill` achievements' kill counters by id (`AchievementKill::OnCreatureKilled`: the counter lives in the
/// achievement object; the manager is the game engine's, so it keeps counting across sessions of the process).
#[derive(Resource, Default)]
pub struct KillCounts(pub gd_formats::collections::HashMap<String, u32>);

/// Unlocks an achievement by id (`GameEngine::UnlockAchievement`).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct UnlockAchievement(pub String);

/// Marks an achievement unlocked without the text or event 62 (`Achievement::Sync`; the module doc's
/// PORT note: the loaded save stands in for Steam).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct SyncAchievement(pub String);

/// Event 62: the achievement with this id was just unlocked.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct AchievementUnlocked(pub String);

/// The ids `GameEngine::ScriptEvent` unlocks for event `name`. `mode`: the session's
/// difficulty (0 Normal, 1 Elite, 2 Ultimate) and challenge; `level`: the player's (`Player.level`). The displayed
/// difficulties (`GameInfo::GetIsPlayingAtLeastThisDifficulty`, Engine.dll): 1 Veteran = Elite or above
/// or the Veteran challenge, 2 Elite = difficulty above Normal, 3 Ultimate.
pub fn script_event_unlocks(
    name: &str,
    mode: gd_rules::mode::Mode,
    hardcore: bool,
    level: u32,
) -> Vec<&'static str> {
    let veteran = mode.difficulty != 0 || mode.challenge == 1;
    let elite = mode.difficulty != 0;
    let ultimate = mode.difficulty == 2;
    // The boss kills: the base id, Elite's, Ultimate's, then hardcore Ultimate's.
    let boss = |ids: [&'static str; 4]| {
        let mut out = vec![ids[0]];
        if elite {
            out.push(ids[1]);
        }
        if ultimate {
            out.push(ids[2]);
            if hardcore {
                out.push(ids[3]);
            }
        }
        out
    };
    match name {
        "EV_WARDEN" => {
            let mut out = vec!["ACH018"];
            if level <= 10 && veteran {
                out.push("ACH048");
            }
            out
        }
        "EV_INVENTOR" => vec!["ACH026"],
        "EV_BLACKSMITH" => vec!["ACH027"],
        "EV_COOK" => vec!["ACH028"],
        "EV_SUNKENRELIQUARY" => vec!["ACH050"],
        "EV_CRONLEYCRYSTAL" => vec!["ACH051"],
        "EV_HIDDENWEALTH" => vec!["ACH052"],
        "EV_BURRWITCH" if veteran => vec!["ACH049"],
        "EV_BANEOFCAIRN" => boss(["ACH067", "ACH068", "ACH044", "ACH045"]),
        "EV_FLESHFACTORY" => boss(["ACH129", "ACH130", "ACH131", "ACH140"]),
        "EV_KORVAAK" => boss(["ACH228", "ACH229", "ACH230", "ACH231"]),
        "EV_BERONATH" => boss(["ACH328", "ACH329", "ACH330", "ACH331"]),
        _ => Vec::new(),
    }
}

/// The `Token` achievements that `token` unlocks.
pub fn token_unlocks<'a>(groups: &'a [Group], token: &str) -> Vec<&'a str> {
    groups
        .iter()
        .flat_map(|g| &g.achievements)
        .filter(|a| a.kind == "Token" && a.tokens.iter().any(|t| t.eq_ignore_ascii_case(token)))
        .map(|a| a.id.as_str())
        .collect()
}

/// `GameInfo::GetIsPlayingAtLeastThisDifficulty` (Engine.dll): the displayed difficulties —
/// 0 any, 1 Veteran (Elite or above, or the Veteran challenge), 2 Elite (above Normal), 3 Ultimate, 4 Ascendant
/// (Ultimate Veteran); anything else is false.
pub fn at_least_difficulty(mode: gd_rules::mode::Mode, displayed: u32) -> bool {
    match displayed {
        0 => true,
        1 => mode.difficulty != 0 || mode.challenge == 1,
        2 => mode.difficulty != 0,
        3 => mode.difficulty == 2,
        4 => mode.difficulty == 2 && mode.challenge == 3,
        _ => false,
    }
}

/// The `Kill` achievements a kill of `record` counts for (`AchievementKill::OnCreatureKilled`): the record in
/// `killDbrs`, `killHardcore` matching the session and the session at least `killDifficulty`. The caller keeps the
/// counters ([`KillCounts`]) and unlocks at `kill_count`.
pub fn kill_matches<'a>(
    groups: &'a [Group],
    record: &str,
    mode: gd_rules::mode::Mode,
    hardcore: bool,
) -> Vec<&'a Achievement> {
    groups
        .iter()
        .flat_map(|g| &g.achievements)
        .filter(|a| {
            a.kind == "Kill"
                && a.kill_dbrs.iter().any(|r| r == record)
                && (!a.kill_hardcore || hardcore)
                && at_least_difficulty(mode, a.kill_difficulty)
        })
        .collect()
}

/// The player's counters the `Stat` achievements read (`AchievementStat::CheckUnlocks`): the loaded save's play stats
/// plus this session's [`crate::play_stats::PlayCounts`], the level, the Crucible records and the live pets / buffs.
pub struct StatContext<'a> {
    /// The loaded character's stats; `None` for a new character (every counter 0).
    pub stats: Option<&'a gd_formats::save::gdc::PlayStats>,
    pub counts: &'a crate::play_stats::PlayCounts,
    pub level: u32,
    pub hardcore: bool,
    pub difficulty: usize,
    /// The story's Crucible records (`story.game.survival`), when a story runs.
    pub survival: Option<&'a gd_story::story::state::Survival>,
    /// The player's live pets (`NUM_PETS_ACTIVE`).
    pub pets: u32,
    /// The player's active buff records and their profile names, lower-case (`NUM_BUFFS_ACTIVE`).
    pub active_buffs: Vec<String>,
}

impl StatContext<'_> {
    /// The counter of `statName` (the `Stat` loader's name → enum map, `AchievementStat::Load`).
    fn value(&self, name: &str) -> u32 {
        let s = self.stats;
        let f = |k: fn(&gd_formats::save::gdc::PlayStats) -> u32| s.map_or(0, k);
        match name {
            "ENEMY_MONSTERS_KILLED" => f(|p| p.kills) + self.counts.kills,
            "ENEMY_HEROES_KILLED" => f(|p| p.hero_kills) + self.counts.hero_kills,
            // `GetNumberOfBossKills`: the current difficulty's boss kills.
            "ENEMY_BOSSES_KILLED" => {
                s.map_or(0, |p| {
                    p.boss_kills.get(self.difficulty).copied().unwrap_or(0)
                }) + self
                    .counts
                    .boss_kills
                    .get(self.difficulty)
                    .copied()
                    .unwrap_or(0)
            }
            "ELITE_OR_ULTIMATE_ENEMY_BOSSES_KILLED" => {
                s.map_or(0, |p| {
                    p.boss_kills.get(1).copied().unwrap_or(0)
                        + p.boss_kills.get(2).copied().unwrap_or(0)
                }) + self.counts.boss_kills[1]
                    + self.counts.boss_kills[2]
            }
            "ASCENDANT_ENEMY_BOSSES_KILLED" => {
                f(|p| p.ultimate_veteran_boss_kills) + self.counts.ultimate_veteran_boss_kills
            }
            "NUM_ITEMS_CRAFTED" => f(|p| p.items_crafted) + self.counts.items_crafted,
            "PLAYER_LEVEL" | "PLAYER_LEVEL_HARDCORE" => self.level,
            "NUM_RELICS_CRAFTED" => f(|p| p.relics_crafted) + self.counts.relics_crafted,
            "NUM_RELICS2_CRAFTED" => {
                f(|p| p.transcendent_relics_crafted) + self.counts.transcendent_relics_crafted
            }
            "NUM_RELICS3_CRAFTED" => {
                f(|p| p.mythical_relics_crafted) + self.counts.mythical_relics_crafted
            }
            "NUM_NOTES_COLLECTED" => f(|p| p.lore_notes_collected) + self.counts.lore_notes,
            "NUM_ONESHOTCHESTS_OPENED" => {
                f(|p| p.one_shot_chests_opened) + self.counts.one_shot_chests_opened
            }
            "NUM_SHRINES_RESTORED" => f(|p| p.shrines_restored) + self.counts.shrines_restored,
            "NUM_HIDDENCHESTS_OPENED" => {
                f(|p| p.hidden_chests_opened) + self.counts.hidden_chests_opened
            }
            // `GetGreatestSurvivalScore`: the greater of the stored greatest and the run's score.
            "SURVIVALMODE_SCORE" => {
                let mut v = f(|p| p.survival_greatest_score);
                if let Some(sv) = self.survival {
                    v = v.max(sv.greatest_score).max(sv.score);
                }
                v
            }
            "NUM_DEFENSES_BUILT" => self
                .survival
                .map_or(f(|p| p.survival_defenses_built), |sv| sv.defenses_built),
            "NUM_POWERUPS_ACTIVE" => self
                .survival
                .map_or(f(|p| p.survival_powerups_activated), |sv| sv.power_ups),
            "NUM_DUNGEONSOULS" => f(|p| p.endless_souls) + self.counts.endless_souls,
            "NUM_DUNGEONESSENCE" => f(|p| p.endless_essence).max(self.counts.endless_essence),
            _ => 0,
        }
    }
}

/// The `Stat` achievements whose counter reached `statCount` (`AchievementStat::CheckUnlocks`).
pub fn stat_unlocks<'a>(groups: &'a [Group], ctx: &StatContext<'_>) -> Vec<&'a str> {
    groups
        .iter()
        .flat_map(|g| &g.achievements)
        .filter(|a| {
            if a.kind != "Stat" {
                return false;
            }
            match a.stat_name.as_str() {
                "PLAYER_LEVEL_HARDCORE" => ctx.hardcore && ctx.level >= a.stat_count,
                "NUM_SKILL_TRACKING" => {
                    let hit = |n: &String| {
                        ctx.stats
                            .and_then(|p| p.skill_stats.iter().find(|(r, _)| r == n))
                            .is_some_and(|(_, v)| *v >= a.stat_count)
                    };
                    !a.skill_names.is_empty()
                        && if a.requires_all {
                            a.skill_names.iter().all(hit)
                        } else {
                            a.skill_names.iter().any(hit)
                        }
                }
                "NUM_BUFFS_ACTIVE" => {
                    a.stat_count > 0
                        && !a.skill_names.is_empty()
                        && a.skill_names
                            .iter()
                            .filter(|n| ctx.active_buffs.iter().any(|b| b.eq_ignore_ascii_case(n)))
                            .count() as u32
                            >= a.stat_count
                }
                "NUM_PETS_ACTIVE" => ctx.pets >= a.stat_count,
                _ => ctx.value(&a.stat_name) >= a.stat_count,
            }
        })
        .map(|a| a.id.as_str())
        .collect()
}

/// The `Faction` achievements reached (`AchievementFaction::CheckUnlocks`): `factionCount` of the listed factions
/// (empty: every faction but the player's, the loader `AchievementFaction::Load`'s 1..46) at `factionValue`
/// reputation — at most when negative. The reputation is truncated to an integer first (`GetValue`'s float cast).
pub fn faction_unlocks<'a>(
    groups: &'a [Group],
    factions: &crate::factions::PlayerFactions,
) -> Vec<&'a str> {
    let all = gd_rules::faction::FACTION_COUNT;
    groups
        .iter()
        .flat_map(|g| &g.achievements)
        .filter(|a| {
            if a.kind != "Faction" {
                return false;
            }
            let indices: Vec<usize> = if a.faction_names.is_empty() {
                (1..all).collect()
            } else {
                a.faction_names
                    .iter()
                    .filter_map(|n| gd_game_core::factions::index_of_name(n))
                    .collect()
            };
            let mut count = 0u32;
            for i in indices {
                let v = factions.value(i) as i32;
                let hit = if a.faction_value < 0 {
                    v <= a.faction_value
                } else {
                    v >= a.faction_value
                };
                if hit {
                    count += 1;
                    if count == a.faction_count {
                        break;
                    }
                }
            }
            count >= a.faction_count
        })
        .map(|a| a.id.as_str())
        .collect()
}

pub struct AchievementsPlugin;

impl Plugin for AchievementsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Achievements>()
            .init_resource::<KillCounts>()
            .add_message::<UnlockAchievement>()
            .add_message::<SyncAchievement>()
            .add_message::<AchievementUnlocked>()
            .add_systems(
                Update,
                (
                    (script_events, token_events, kill_events, periodic_events)
                        .after(gd_game_core::story::StoryStage::Tick),
                    unlock,
                )
                    .chain(),
            );
    }
}

/// The scripts' events ([`script_event_unlocks`]).
fn script_events(
    mut effects: MessageReader<gd_game_core::story::StoryEffect>,
    mode: Res<gd_game_core::session::GameMode>,
    // Option: a new character has no loaded save; the progress exists once the player does.
    (loaded, choice, progress): (
        Option<Res<gd_game_core::session::LoadedCharacter>>,
        Res<gd_game_core::session::Choice>,
        Option<Res<gd_game_core::progress::PlayerProgress>>,
    ),
    mut out: MessageWriter<UnlockAchievement>,
) {
    for e in effects.read() {
        let gd_story::story::state::Effect::ScriptEvent(name) = &e.0 else {
            continue;
        };
        let hardcore = gd_game_core::session::is_hardcore(loaded.as_deref(), &choice);
        let level = progress
            .as_ref()
            .map_or(1, |p| p.progress.level.max(1) as u32);
        for id in script_event_unlocks(name, mode.0, hardcore, level) {
            out.write(UnlockAchievement(id.to_owned()));
        }
    }
}

/// The achievement list ([`groups`]) built once per session (a session may mount another mod).
#[derive(bevy::ecs::system::SystemParam)]
struct GroupList<'w, 's> {
    data: Res<'w, gd_game_core::data::GameData>,
    fresh: gd_game_core::session::NewSession<'w, 's>,
    cached: Local<'s, Option<Vec<Group>>>,
}

impl GroupList<'_, '_> {
    fn get(&mut self) -> &[Group] {
        if self.fresh.begun() {
            *self.cached = None;
        }
        let db = &self.data.db;
        self.cached.get_or_insert_with(|| groups(db))
    }
}

/// Tokens the player newly holds ([`token_unlocks`]; the module doc's PORT note). The ones held when the session's
/// story starts are synced.
fn token_events(
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    mut list: GroupList,
    mut seen: Local<Option<gd_formats::collections::HashSet<String>>>,
    mut fresh: gd_game_core::session::NewSession,
    mut out: MessageWriter<UnlockAchievement>,
    mut sync: MessageWriter<SyncAchievement>,
) {
    if fresh.begun() {
        *seen = None;
    }
    let Some(rt) = rt else { return };
    let story = rt.story.borrow();
    let tokens = &story.player.tokens;
    let Some(seen) = seen.as_mut() else {
        let groups = list.get();
        for t in tokens.iter() {
            for id in token_unlocks(groups, t) {
                sync.write(SyncAchievement(id.to_owned()));
            }
        }
        *seen = Some(tokens.iter().cloned().collect());
        return;
    };
    if tokens.len() == seen.len() && tokens.iter().all(|t| seen.contains(t)) {
        return;
    }
    let groups = list.get();
    for t in tokens.iter().filter(|t| !seen.contains(*t)) {
        for id in token_unlocks(groups, t) {
            out.write(UnlockAchievement(id.to_owned()));
        }
    }
    *seen = tokens.iter().cloned().collect();
}

/// The player's kills ([`kill_matches`]; the module doc's handler rules: the killer, the victim's
/// `giveXP` and dungeon flags).
#[allow(clippy::too_many_arguments)]
fn kill_events(
    mut killed: MessageReader<crate::combat::MonsterKilled>,
    data: Res<gd_game_core::data::GameData>,
    mut list: GroupList,
    mode: Res<gd_game_core::session::GameMode>,
    // Option: a new character has no loaded save.
    (loaded, choice): (
        Option<Res<gd_game_core::session::LoadedCharacter>>,
        Res<gd_game_core::session::Choice>,
    ),
    credit: crate::combat::KillCredit,
    monsters: Query<&gd_game_core::monsters::Monster>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    state: Res<Achievements>,
    mut counts: ResMut<KillCounts>,
    mut out: MessageWriter<UnlockAchievement>,
) {
    for k in killed.read() {
        if !credit.of(k) {
            continue;
        }
        // `Monster::GiveExperience`: the record's `giveXP` (missing: true).
        let give_xp = data
            .db
            .get(&k.record)
            .and_then(|r| r.f32("giveXP"))
            .is_none_or(|v| v != 0.0);
        if !give_xp {
            continue;
        }
        // `InEndlessDungeon` (module doc's PORT note).
        let in_dungeon = rt.as_ref().is_some_and(|rt| {
            let story = rt.story.borrow();
            monsters.get(k.entity).is_ok_and(|m| {
                story
                    .endless
                    .activated
                    .iter()
                    .any(|(_, key)| *key == (m.slot.0, m.slot.1))
            })
        });
        if in_dungeon {
            continue;
        }
        let groups = list.get();
        let hardcore = gd_game_core::session::is_hardcore(loaded.as_deref(), &choice);
        for a in kill_matches(groups, &k.record, mode.0, hardcore) {
            if state.is_unlocked(&a.id) {
                continue;
            }
            let n = counts.0.entry(a.id.clone()).or_insert(0);
            *n += 1;
            if *n == a.kill_count {
                out.write(UnlockAchievement(a.id.clone()));
            }
        }
    }
}

/// The periodic `Stat` / `Faction` checks ([`stat_unlocks`], [`faction_unlocks`]; the manager update
/// `AchievementManager::Update`'s 3000 ms, the module doc's PORT note on the Steam gate). A session's first check is
/// the sync.
#[allow(clippy::too_many_arguments)]
fn periodic_events(
    time: Res<Time>,
    (mut timer, mut synced): (Local<f32>, Local<bool>),
    mut fresh: gd_game_core::session::NewSession,
    mut list: GroupList,
    mode: Res<gd_game_core::session::GameMode>,
    // Option: a new character has no loaded save; the progress exists once the player does.
    (loaded, choice, progress): (
        Option<Res<gd_game_core::session::LoadedCharacter>>,
        Res<gd_game_core::session::Choice>,
        Option<Res<gd_game_core::progress::PlayerProgress>>,
    ),
    counts: Res<crate::play_stats::PlayCounts>,
    // Option: created with the player; the session's sync waits for it.
    factions: Option<Res<crate::factions::PlayerFactions>>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    players: Query<Entity, With<gd_game_core::player::Player>>,
    pets: Query<&crate::pets::Pet, Without<gd_game_core::combat::Dead>>,
    buffed: Query<
        (
            Option<&gd_game_core::skill_buffs::ReceivedBuffs>,
            Option<&gd_game_core::player::Loadout>,
        ),
        With<gd_game_core::player::Player>,
    >,
    state: Res<Achievements>,
    (mut out, mut sync): (
        MessageWriter<UnlockAchievement>,
        MessageWriter<SyncAchievement>,
    ),
) {
    if fresh.begun() {
        *synced = false;
    }
    *timer += time.delta_secs();
    if *timer < 3.0 {
        return;
    }
    *timer -= 3.0;
    // The engine's checks read the main player; nothing without one.
    let Ok(player) = players.single() else {
        return;
    };
    let story = rt.as_ref().map(|rt| rt.story.borrow());
    let mut active_buffs = Vec::new();
    if let Ok((received, loadout)) = buffed.get(player) {
        let mut add = |s: &str| {
            active_buffs.push(s.to_ascii_lowercase());
            if let Some(p) = list
                .data
                .db
                .get(s)
                .and_then(|r| gd_sim::skills::profile_name(r).map(str::to_owned))
            {
                active_buffs.push(p.to_ascii_lowercase());
            }
        };
        if let Some(r) = received {
            for b in &r.list {
                add(&b.buff);
            }
        }
        if let Some(l) = loadout {
            for b in &l.buffs.list {
                add(&b.0);
            }
        }
    }
    let ctx = StatContext {
        stats: loaded.as_deref().map(|c| &c.0.stats),
        counts: &counts,
        level: progress
            .as_ref()
            .map_or(1, |p| p.progress.level.max(1) as u32),
        hardcore: gd_game_core::session::is_hardcore(loaded.as_deref(), &choice),
        difficulty: mode.0.difficulty,
        survival: story.as_ref().map(|s| &s.game.survival),
        pets: pets.iter().filter(|p| p.owner == player).count() as u32,
        active_buffs,
    };
    let groups = list.get();
    let silent = !*synced;
    let mut send = |id: &str| {
        if state.is_unlocked(id) {
            return;
        }
        if silent {
            sync.write(SyncAchievement(id.to_owned()));
        } else {
            out.write(UnlockAchievement(id.to_owned()));
        }
    };
    for id in stat_unlocks(groups, &ctx) {
        send(id);
    }
    if let Some(factions) = factions.as_deref() {
        for id in faction_unlocks(groups, factions) {
            send(id);
        }
        *synced = true;
    }
}

/// Applies the syncs (the flag alone) and the unlocks: a listed, locked id is unlocked, with its cinematic text
/// and event 62.
fn unlock(
    mut syncs: MessageReader<SyncAchievement>,
    mut requests: MessageReader<UnlockAchievement>,
    mut list: GroupList,
    mut state: ResMut<Achievements>,
    mut cinematic: MessageWriter<gd_game_core::story::CinematicText>,
    mut events: MessageWriter<AchievementUnlocked>,
) {
    for SyncAchievement(id) in syncs.read() {
        if !state.is_unlocked(id) {
            state.unlocked.push(id.clone());
        }
    }
    for UnlockAchievement(id) in requests.read() {
        if state.is_unlocked(id) {
            continue;
        }
        let groups = list.get();
        let Some(a) = groups
            .iter()
            .flat_map(|g| &g.achievements)
            .find(|a| a.id == *id)
        else {
            continue;
        };
        info!("achievements: {id} unlocked");
        state.unlocked.push(id.clone());
        cinematic.write(gd_game_core::story::CinematicText {
            head: "tagAchievementUnlocked".into(),
            sub: a.title.clone(),
            sub_args: Vec::new(),
            kind: 1,
            front: false,
        });
        events.write(AchievementUnlocked(id.clone()));
    }
}
