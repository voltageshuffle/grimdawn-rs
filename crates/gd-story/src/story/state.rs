//! Player/world facts the story layer reads, and the effects it asks the host
//! to apply. Bevy-free; the host mirrors its own state into [`Player`] /
//! [`GameInfo`] / [`Entities`] and drains [`Effect`]s.
use gd_formats::collections::{HashMap, HashSet};
use std::collections::{BTreeMap, BTreeSet};

/// Game difficulty (`Game.Difficulty` in Lua: Normal 0, Epic 1, Legendary 2).
pub const DIFFICULTIES: usize = 3;

#[derive(Debug, Clone, Default)]
pub struct Player {
    /// Object id of the player entity (what `Game.GetLocalPlayer()` wraps).
    pub id: u32,
    pub name: String,
    /// Creature record (triggers' `detectableNames` match it).
    pub record: String,
    pub level: u32,
    pub experience: u32,
    pub money: u32,
    /// The Crucible's tributes (Character dword 1458), at most [`MAX_TRIBUTE`].
    pub tribute: u32,
    pub total_devotion: u32,
    /// Difficulties skipped with the campaign's difficulty unlock items (`PlayStats.difficultySkips`, saved in
    /// block 12):
    /// bit 1 Normal, bit 2 Elite ([`super::Story::skip_difficulties`]).
    pub difficulty_skip: u8,
    pub hardcore: bool,
    /// The player is dead (`Game.PlayersDead`; the host mirrors it).
    pub dead: bool,
    /// The map region the player stands in (index into the world map's regions; the host mirrors it):
    /// `Player:InEndlessDungeon` and the Shattered Realm's floor tests.
    pub region: Option<usize>,
    /// Mastery indices the player has (`HasMastery` condition / `Player:HasMastery`).
    pub masteries: Vec<u32>,
    /// Faction name (`USER5`, `SURVIVORS`, ...) → value.
    pub factions: BTreeMap<String, f32>,
    /// Item record (lower-case) → count in the inventory (bags and the held item, not the equipment).
    pub items: HashMap<String, u32>,
    /// Item record (lower-case) → count in the personal and transfer stashes
    /// and the blacksmith's materials (what `Player::GetItemCountInStashes`
    /// adds to the inventory count).
    pub stored_items: HashMap<String, u32>,
    /// Item record (lower-case) → count of incomplete relics and charms (`ItemRelic::IsComplete` false) among
    /// `items` and the stash sacks of `stored_items`: what the complete-only filter leaves out.
    pub incomplete_items: HashMap<String, u32>,
    /// The bags (main bag first) as the host has them: what `Player.IsSpaceAvailable` searches.
    pub bags: Vec<gd_formats::inventory::Sack<BagItem>>,
    /// Trigger tokens of the current difficulty (`Player::HasToken`).
    pub tokens: BTreeSet<String>,
    pub toggled_skills: HashSet<String>,
    pub buffs: HashSet<String>,
    pub position: [f32; 3],
}

/// An item in the player's bags as `Player.IsSpaceAvailable` sees it: its record and whether its stack has
/// room for one more (`InventorySack::GetNotFullItem`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BagItem {
    pub record: String,
    pub stack_room: bool,
}

/// The most tributes a character holds (`Character::AddTribute`).
pub const MAX_TRIBUTE: u32 = 150;

impl Player {
    pub fn new_character(id: u32, name: &str) -> Self {
        Self {
            id,
            name: name.into(),
            record: "records/creatures/pc/malepc01.dbr".into(),
            level: 1,
            ..Default::default()
        }
    }
    pub fn item_count(&self, record: &str) -> u32 {
        self.items
            .get(&record.to_ascii_lowercase())
            .copied()
            .unwrap_or(0)
    }
    /// `Player::GetItemCountInStashes` (Game.dll) as the `HasItem` condition
    /// (`ScriptableCondition_HasItem::Evaluate`) and Lua `HasItem` (`LuaPlayer::HasItem`) call it, flags
    /// (`complete_only`, 0, 1, 0):
    /// `Character::GetItemCount` with its equipment flag set (the inventory, skipping equipped items:
    /// `EquipManager::GetItemCount` only without it) + the blacksmith's materials (`GetReagentItemCount`,
    /// unfiltered) + each personal stash sack + the transfer stash. `complete_only` makes the inventory and
    /// the sacks skip items whose `Item::IsComplete` is false: an incomplete relic or charm (every
    /// other stock item class answers true).
    pub fn item_count_in_stashes(&self, record: &str, complete_only: bool) -> u32 {
        let key = record.to_ascii_lowercase();
        let get = |m: &HashMap<String, u32>| m.get(&key).copied().unwrap_or(0);
        let n = get(&self.items) + get(&self.stored_items);
        if complete_only {
            n.saturating_sub(get(&self.incomplete_items))
        } else {
            n
        }
    }
    pub fn add_items(&mut self, record: &str, delta: i64) {
        let e = self.items.entry(record.to_ascii_lowercase()).or_default();
        *e = (*e as i64 + delta).max(0) as u32;
    }
    pub fn faction(&self, name: &str) -> f32 {
        self.factions
            .get(&name.to_ascii_uppercase())
            .copied()
            .unwrap_or(0.0)
    }
}

#[derive(Debug, Clone)]
pub struct GameInfo {
    pub difficulty: u32,
    pub challenge: u32,
    pub owned_dlc: Vec<u32>,
    /// Milliseconds since the session started (`Time.Now`, `Game.GetGameTime`).
    pub time_ms: u64,
    /// Lua `Server` global; single player is its own server.
    pub server: bool,
    /// The installed altar formulas' `luaToken` levels
    /// (`gd_sim::ascendant::Ascendant::tokens`).
    pub ascendant_tokens: Vec<(String, u32)>,
    /// `GameInfo::GetMode() == 1`: a Crucible session (the bindings below are inert
    /// outside it, like the engine's).
    pub survival_mode: bool,
    pub survival: Survival,
}

/// Shattered Realm state behind the `Game.*Endless*` bindings (see `crate::endless`): the dungeons
/// generated, the floors used recently (kept across generations), the running dungeon and its flags.
#[derive(Clone, Debug, Default)]
pub struct Endless {
    /// `GameEngine::dungeonsGenerated`: the `tierDifficulty` of the equations.
    pub generated: u32,
    pub recent: std::collections::VecDeque<String>,
    pub dungeon: Option<crate::endless::Dungeon>,
    /// What the generator reads of the world map (the host pre-builds it at session load;
    /// [`super::Story::endless_provider`] is the fallback).
    pub map: Option<std::sync::Arc<crate::endless::EndlessMap>>,
    /// The next dungeon stamp.
    pub next_id: u32,
    /// `IsEndlessDungeonBossDead` / `IsEndlessDungeonBonusComplete` (`EndlessDungeon_Generator.bossDead` /
    /// `EndlessDungeon_Generator.bonusComplete`).
    pub boss_dead: bool,
    pub bonus_complete: bool,
    /// The hub door's object id the script passed to `GenerateEndlessDungeon` (`entranceId`,
    /// `EndlessDungeon_Generator.entranceId`):
    /// using that door, once open, teleports to the active floor's entrance.
    pub entrance_door: Option<u32>,
    /// Charges of the soul buff (the essences picked up while it ran).
    pub soul_charges: u32,
    /// The proxy keys (level, entity index) of the proxies the host activated in the running dungeon, by floor
    /// index (moved to `stale` when the next dungeon is generated).
    pub activated: Vec<(usize, (usize, u32))>,
    /// Keys of proxies activated in an older dungeon whose level state the host still keeps (dropped once
    /// their level is unloaded: `gd_game::endless::drop_stale_proxies`).
    pub stale: Vec<(usize, u32)>,
    /// The player entered the section's active floor: the timer start was decided
    /// (`EndlessDungeon_Generator.timerStarted`, cleared at each
    /// generation).
    pub timer_started: bool,
    /// A section's bonus timer ran in this session (`EndlessDungeon_Generator.timerEverRan`, kept across
    /// generations; `gd_game::endless`).
    pub timer_ever: bool,
    /// The kill chain: kills in a row and the ms left before the chain pays out.
    pub chain: KillChain,
    /// Essence items a monster of the dungeon dropped: used at once by the host (`endless::progress`).
    pub essence_queue: Vec<String>,
    /// Cinematic texts the host shows (`ShowCinematicText` type 9: the bonus time texts), already formatted.
    pub texts: Vec<String>,
    /// Proxy keys of a finished floor whose monsters the host must kill (`KillMonstersInRegion`).
    pub kill_keys: Vec<(usize, u32)>,
}

/// The kill chain (`GameEngine::OnCreatureDeath`): kills in a row and the ms left before the chain pays out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KillChain {
    pub count: u32,
    pub timer_ms: i32,
}

impl KillChain {
    /// A kill of the active floor's monsters joins the chain (it restarts the pause timer).
    pub fn kill(&mut self, reset_ms: i32) {
        self.count += 1;
        self.timer_ms = reset_ms;
    }

    /// The pause timer runs; once it is out the chain ends and, when it had enough kills and the boss is not dead,
    /// its bonus (ms) is returned for the caller to add to the running timer.
    pub fn tick(
        &mut self,
        dt_ms: i32,
        boss_dead: bool,
        rs: &crate::endless::Ruleset,
    ) -> Option<i32> {
        if self.count == 0 {
            return None;
        }
        self.timer_ms -= dt_ms;
        if self.timer_ms > 0 {
            return None;
        }
        let kills = std::mem::take(&mut self.count);
        if boss_dead {
            return None;
        }
        rs.kill_chain_bonus_ms(kills)
    }
}

impl Endless {
    /// `IsRegionInDungeon(region, activeOnly)`.
    pub fn in_dungeon(&self, region: usize, active_only: bool) -> bool {
        self.dungeon.as_ref().is_some_and(|d| {
            d.floors
                .iter()
                .enumerate()
                .any(|(i, f)| f.regions.contains(&region) && (!active_only || i == d.active))
        })
    }

    /// `GetFloorNumber`: the index of the floor holding `region` (the boss floor included).
    pub fn floor_of(&self, region: usize) -> Option<usize> {
        self.dungeon
            .as_ref()?
            .floors
            .iter()
            .position(|f| f.regions.contains(&region))
    }

    /// The progress of the floor holding `region` as a fraction of its total (the HUD's bar,
    /// `UIEndlessHud::Update`: floor progress / max floor progress of the player's region).
    pub fn progress_of(&self, region: usize) -> f32 {
        let Some(f) = self
            .floor_of(region)
            .and_then(|i| self.dungeon.as_ref()?.floors.get(i))
        else {
            return 0.0;
        };
        if f.total <= 0.0 {
            0.0
        } else {
            (f.progress / f.total).clamp(0.0, 1.0)
        }
    }

    /// `IsBossRegion`.
    pub fn is_boss_region(&self, region: usize) -> bool {
        self.dungeon.as_ref().is_some_and(|d| {
            d.floors
                .iter()
                .any(|f| f.floor.boss && f.regions.contains(&region))
        })
    }
}

/// Crucible session state behind the `Game.*Survival*` bindings. (Game.dll
/// `PlayStats` at `Player.playStats` and `GameEngine`):
/// wave tier is `PlayStats.waveTier`, the bonus multiplier `PlayStats.bonusMultiplier`, restarts
/// `PlayStats.restarts`, the bonus timer `PlayStats.bonusTimer` (ms, counts down); the difficulty ramp is
/// `GameEngine.difficultyRamp`.
/// None of these but the greatest-difficulty / greatest-wave / greatest-score fields is saved.
#[derive(Debug, Clone, PartialEq)]
pub struct Survival {
    pub wave_tier: u32,
    /// `SetDifficultyRamp` (`Monster::OnDifficultyRampLevel`).
    pub difficulty_ramp: u32,
    /// 1..=10: `SetSurvivalTimer` raises it when the timer still ran.
    pub multiplier: u32,
    pub timer_ms: i32,
    /// `PlayStats.score`: the run's score; kills stop adding once `score_locked` (`PlayStats.scoreLocked`,
    /// `SaveSurvivalScore`). Reset with the event.
    pub score: u32,
    pub score_locked: bool,
    /// The saved records (`player.gdc` header / block 12): greatest survival difficulty
    /// completed, greatest wave tier and score ever; the run's values merge in at save.
    pub greatest_difficulty: u8,
    pub greatest_wave: u32,
    pub greatest_score: u32,
    /// `PlayStats.defensesBuilt` / `PlayStats.powerUps`, saved in `player.gdc` block 12: the grants of
    /// `SURVIVALMODE_DEFENSEBUILT` /
    /// `SURVIVALMODE_POWERUPACTIVE` ([`Survival::token_granted`]).
    pub defenses_built: u32,
    pub power_ups: u32,
    /// Counters of the HUD's events (`UISurvivalHud`): the pane
    /// plays a glow (and sound) when one rises: 63 tribute gained, 64 multiplier up / reset, 65 timer add.
    pub tribute_gains: u32,
    pub multiplier_ups: u32,
    pub multiplier_resets: u32,
    pub timer_adds: u32,
    /// Outside the Crucible: event 64 with its byte false (the reset found the timer out), the Shattered Realm
    /// HUD's `timerFailedSound`; and event 65 with a negative amount (a death's penalty: the timer glow only).
    pub timer_failures: u32,
    pub timer_cuts: u32,
    /// The 10 s warning (GameEvent 66) fired for this timer.
    pub warned: bool,
    pub restarts: u32,
    pub event_in_progress: bool,
    /// `EnableRespawn` / `DisableRespawn` (`GameEngine.respawnEnabled`): the player's respawn.
    pub respawn: bool,
    /// `EnableRespawn` was called: dead players come back (a one-shot request the host consumes;
    /// `ControllerPlayer.respawnRequested`).
    pub respawn_requested: bool,
    /// `AllowNpcTalk` (`GameEngine.npcTalkAllowed`).
    pub npc_talk: bool,
    /// `KillAllMonstersOnNextRespawn` (`GameEngine.killAllMonstersOnNextRespawn`).
    pub kill_on_respawn: bool,
    /// Mutator record paths (`GameEngine.mutators`), deduplicated.
    pub mutators: Vec<String>,
    /// HUD objective lines: localisation tag and its int.
    pub objectives: Vec<(String, i32)>,
}

impl Default for Survival {
    fn default() -> Self {
        Self {
            wave_tier: 0,
            difficulty_ramp: 0,
            multiplier: 1,
            timer_ms: 0,
            score: 0,
            score_locked: false,
            greatest_difficulty: 0,
            greatest_wave: 0,
            greatest_score: 0,
            defenses_built: 0,
            power_ups: 0,
            tribute_gains: 0,
            multiplier_ups: 0,
            multiplier_resets: 0,
            timer_adds: 0,
            timer_failures: 0,
            timer_cuts: 0,
            warned: false,
            restarts: 0,
            event_in_progress: false,
            respawn: true,
            respawn_requested: false,
            npc_talk: true,
            kill_on_respawn: false,
            mutators: Vec::new(),
            objectives: Vec::new(),
        }
    }
}

impl Survival {
    /// `PlayStats::UpdateBonusTimer`: counts down; clamps at 0 and resets the multiplier
    /// (`crucible`: `GameInfo::GetMode() == 1`). Returns true when the 10 s warning fires (once per timer).
    pub fn tick(&mut self, dt_ms: i32, crucible: bool) -> bool {
        if self.timer_ms <= 0 {
            return false;
        }
        self.timer_ms -= dt_ms;
        let mut warn = false;
        if self.timer_ms < 10_000 && !self.warned {
            self.warned = true;
            warn = true;
        }
        if self.timer_ms <= 0 {
            self.timer_ms = 0;
            self.reset_multiplier(crucible);
        }
        warn
    }

    /// `PlayStats::SetBonusTimer`: the multiplier +1 (max 10) if the old timer was above 0
    /// (`IncrementSurvivalMultiplier`: only in the Crucible, `crucible`).
    pub fn set_timer(&mut self, ms: i32, crucible: bool) {
        if crucible && self.timer_ms > 0 {
            self.multiplier = (self.multiplier + 1).min(10);
            self.multiplier_ups += 1;
        }
        self.timer_ms = ms;
        self.warned = ms < 10_000;
    }

    /// `PlayStats::IncrementKills` (mode 1): while the score is unlocked, adds
    /// `eval(survivalScoreEquation)` of the kill's `monster_score`
    /// (`ResolveEquationVariable`: `survivalCurrentWaveTier`,
    /// `survivalMonsterScore`, `survivalBonusMultiplier`). The u32 truncation is a PORT.
    pub fn add_kill(&mut self, equation: &gd_rules::expr::Expr, monster_score: f32) {
        if self.score_locked {
            return;
        }
        let (tier, mult) = (self.wave_tier as f32, self.multiplier as f32);
        let v = equation.eval(&|name| match name {
            "survivalCurrentWaveTier" => Some(tier),
            "survivalMonsterScore" => Some(monster_score),
            "survivalBonusMultiplier" => Some(mult),
            _ => None,
        });
        self.score = self.score.saturating_add(v.max(0.0) as u32);
    }

    /// `GameEngine::CompleteSurvivalDifficulty(GameDifficulty)` overload: stores `unlocked` (the
    /// difficulty index an `ItemDifficultyUnlock` unlocks) when it is above the greatest. Returns true when stored.
    pub fn unlock_difficulty(&mut self, unlocked: u8) -> bool {
        if unlocked > self.greatest_difficulty {
            self.greatest_difficulty = unlocked.min(2);
            return true;
        }
        false
    }

    /// `GameEngine::CompleteSurvivalDifficulty`:
    /// completing the session's difficulty `d` at or above the greatest completed stores `d + 1`,
    /// capped at 2 (the Ultimate Crucible). Returns true when the stored value changed.
    pub fn complete_difficulty(&mut self, d: u32) -> bool {
        if d as u8 >= self.greatest_difficulty {
            let next = (d as u8 + 1).min(2);
            let changed = next != self.greatest_difficulty;
            self.greatest_difficulty = next;
            return changed;
        }
        false
    }

    /// `PlayStats::TokenGranted`, called by `Player::BestowToken` for every grant (in any
    /// mode, a token already held included): `SURVIVALMODE_DEFENSEBUILT` counts a defense built,
    /// `SURVIVALMODE_POWERUPACTIVE` a power-up (exact names).
    pub fn token_granted(&mut self, token: &str) {
        match token {
            "SURVIVALMODE_DEFENSEBUILT" => self.defenses_built += 1,
            "SURVIVALMODE_POWERUPACTIVE" => self.power_ups += 1,
            _ => {}
        }
    }

    /// The saved records with this run's wave tier and score merged in.
    pub fn merge_records(&mut self) {
        self.greatest_wave = self.greatest_wave.max(self.wave_tier);
        self.greatest_score = self.greatest_score.max(self.score);
    }

    /// `SaveSurvivalScore` locks the score (kills stop adding).
    pub fn lock_score(&mut self) {
        self.score_locked = true;
    }

    /// `ResetSurvivalTimer` / `PlayStats::ResetBonusTimer` / `SetEndlessTimerInbound`'s reset: the
    /// multiplier reset, then the timer 0.
    pub fn reset_timer(&mut self, crucible: bool) {
        self.reset_multiplier(crucible);
        self.timer_ms = 0;
    }

    /// `PlayStats::ResetSurvivalMultiplier`: in the Crucible a multiplier above 1 goes back
    /// to 1 with event 64 (false: the HUD's subtractor glow and `timerFailedSound`); in any other mode the
    /// multiplier stays and event 64 always goes out with its byte = timer > 0 (true: the Shattered Realm HUD
    /// clears its warning; false: `timerFailedSound`, counted in `timer_failures`).
    fn reset_multiplier(&mut self, crucible: bool) {
        if !crucible {
            if self.timer_ms <= 0 {
                self.timer_failures += 1;
            }
            return;
        }
        if self.multiplier != 1 {
            self.multiplier_resets += 1;
        }
        self.multiplier = 1;
    }

    /// `SurvivalTimerAdd`: only while the timer runs.
    pub fn timer_add(&mut self, ms: i32) {
        if self.timer_ms > 0 {
            // (bad data: the script-controlled amount must not overflow the timer.)
            self.timer_ms = self.timer_ms.saturating_add(ms);
            self.timer_adds += 1;
        }
    }

    /// `PlayStats::IncrementBonusTimer` with a negative amount (a death in the Shattered Realm,
    /// `SetEndlessTimer(-dyingPenalty ms)`): only while the timer runs, no text; its event 65 goes out
    /// with the negative amount (the HUD's glow pulses, `bonusTimeSound` needs > 0: `timer_cuts`).
    /// PORT: the timer stops at 0 (the engine leaves it negative; `UpdateBonusTimer` then skips it, so no
    /// multiplier reset follows either way).
    pub fn timer_subtract(&mut self, ms: i32) {
        if self.timer_ms > 0 {
            self.timer_ms = (self.timer_ms - ms).max(0);
            self.timer_cuts += 1;
        }
    }

    /// `ResetSurvivalEvent` after its autosave: `ResetSurvivalMode` (score, multiplier,
    /// timer, wave tier), clear the mutators, ramp 0. Restarts stay.
    pub fn reset_event(&mut self) {
        self.event_in_progress = false;
        self.score = 0;
        self.score_locked = false;
        self.wave_tier = 0;
        self.reset_timer(true);
        self.mutators.clear();
        self.difficulty_ramp = 0;
    }
}

impl Default for GameInfo {
    fn default() -> Self {
        // Steam app ids of the three expansions (OwnsDlc conditions use them).
        Self {
            difficulty: 0,
            challenge: 0,
            owned_dlc: vec![348780, 642280, 1272990],
            time_ms: 0,
            server: true,
            ascendant_tokens: Vec::new(),
            survival_mode: false,
            survival: Survival::default(),
        }
    }
}

/// Engine object classes the Lua API distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjClass {
    Player,
    Character,
    Npc,
    Proxy,
    Door,
    Lever,
    Chest,
    Destructible,
    Riftgate,
    Shrine,
    AreaTrigger,
    ScriptEntity,
    Item,
    Entity,
}

impl ObjClass {
    /// From a database record's `Class` field.
    pub fn from_record_class(class: &str) -> Self {
        let c = class.to_ascii_lowercase();
        match c.as_str() {
            "player" => Self::Player,
            "npc" | "npcmerchant" | "npcriftmerchant" => Self::Npc,
            "door"
            | "doorbossroom"
            | "fixeddoor"
            | "fixeditemdoor"
            | "dungeonentrance"
            | "endlessdungeonentrance"
            | "endlessdungeonexit" => Self::Door,
            "lever" | "fixedlever" => Self::Lever,
            "chest" | "lootchest" | "fixeditemcontainer" => Self::Chest,
            "destructible" => Self::Destructible,
            // The Lua `Riftgate` wraps a `StaticTeleporter` (`LuaRiftgate::SetLocked` →
            // `StaticTeleporter::SetLocked`): every stock gate record has that class.
            "staticteleporter" | "riftgate" | "travelingriftgate" => Self::Riftgate,
            "boxtrigger" | "spheretrigger" | "areatrigger" => Self::AreaTrigger,
            "scriptentity" => Self::ScriptEntity,
            _ if c.starts_with("proxy") => Self::Proxy,
            _ if c.starts_with("monster") || c.contains("character") || c.starts_with("pet") => {
                Self::Character
            }
            _ if c.contains("shrine") => Self::Shrine,
            _ if c.starts_with("item")
                || c.starts_with("weapon")
                || c.starts_with("armor")
                || c.starts_with("oneshot") =>
            {
                Self::Item
            }
            _ => Self::Entity,
        }
    }
    pub fn is_character(self) -> bool {
        matches!(self, Self::Player | Self::Character | Self::Npc)
    }
}

/// Position + orientation (rows = local X/Y/Z axes, like the `.map` Mat3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coords {
    pub origin: [f32; 3],
    pub axes: [[f32; 3]; 3],
}

impl Coords {
    pub fn at(origin: [f32; 3]) -> Self {
        Self {
            origin,
            axes: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorldObject {
    pub id: u32,
    pub record: String,
    pub class: ObjClass,
    pub coords: Coords,
    /// Placed in the world (false after `Create` until `SetCoords`).
    pub in_world: bool,
    pub alive: bool,
    pub locked: bool,
    pub open: bool,
    /// Destructible broken by `:Destroy` (engine `Destructible::BreakApart`): the object stays
    /// valid and queryable; the host shows the broken mesh / despawn.
    pub broken: bool,
    /// Npc: conversation file from the record (`conversation` field).
    pub conversation: Option<String>,
    pub available_for_conversation: bool,
    /// Created by a script (vs placed in a level).
    pub scripted: bool,
}

/// Every object the story layer knows about, keyed by object id.
#[derive(Debug, Default)]
pub struct Entities {
    pub objects: BTreeMap<u32, WorldObject>,
    /// Level a script gave `Character.Create(record, level, ...)` (absent:
    /// the host picks it).
    pub levels: BTreeMap<u32, u32>,
    /// `Character.Create(record, level, origin)`'s third arg: the host clamps the level to the
    /// region's limits around it (engine `GetLevelLimits`); absent = no origin given.
    pub origins: BTreeMap<u32, [f32; 3]>,
    /// `Proxy.Create(record, vec, bool)`'s vec: `Proxy::SetLimitsLocation` before load.
    pub proxy_limits: BTreeMap<u32, [f32; 3]>,
    /// `Proxy.Create(record, vec, true)`: `CreateAtIgnoreBoss` (byte +1612).
    pub proxy_ignore_boss: HashSet<u32>,
    /// Host-published world coords of the story characters' mesh attach points (object id → (attach name,
    /// coords)), for `Entity:GetAttachedCoords(name)`; an object without them falls back to its entity coords.
    pub attach_coords: HashMap<u32, Vec<(String, Coords)>>,
    /// Characters a script created with `Character.Create` / `CreateNearPlayer`: Grim Dawn.exe
    /// `LuaCharacter::Create` / `LuaCharacter::CreateAtLevel` / `LuaCharacter::CreateNearPlayer` call the new
    /// object's `Character::EnableSpawnAnimation`, so it starts with its Spawn animation (the host's startup);
    /// `Entity.Create` does not.
    pub spawn_animation: BTreeSet<u32>,
    next_id: u32,
}

impl Entities {
    /// Ids handed out to script-created objects start high so they never
    /// collide with host ids.
    pub const SCRIPT_ID_BASE: u32 = 0x4000_0000;

    pub fn get(&self, id: u32) -> Option<&WorldObject> {
        self.objects.get(&id)
    }
    pub fn get_mut(&mut self, id: u32) -> Option<&mut WorldObject> {
        self.objects.get_mut(&id)
    }
    pub fn insert(&mut self, obj: WorldObject) {
        self.objects.insert(obj.id, obj);
    }
    pub fn create(&mut self, record: &str, class: ObjClass) -> u32 {
        if self.next_id < Self::SCRIPT_ID_BASE {
            self.next_id = Self::SCRIPT_ID_BASE;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.objects.insert(
            id,
            WorldObject {
                id,
                record: record.to_ascii_lowercase(),
                class,
                coords: Coords::at([0.0; 3]),
                in_world: false,
                alive: true,
                locked: false,
                open: false,
                broken: false,
                conversation: None,
                available_for_conversation: true,
                scripted: true,
            },
        );
        id
    }
    pub fn remove(&mut self, id: u32) -> Option<WorldObject> {
        self.levels.remove(&id);
        self.origins.remove(&id);
        self.proxy_limits.remove(&id);
        self.proxy_ignore_boss.remove(&id);
        self.attach_coords.remove(&id);
        self.spawn_animation.remove(&id);
        self.objects.remove(&id)
    }
}

/// Facts only the host knows, published into the story so the Lua bindings
/// that query them (`Proxy:AllKilled`, `Shrine:IsRestored`) can answer.
#[derive(Debug, Default)]
pub struct HostFacts {
    /// Proxies that were run (placed in the world, not held back by
    /// `delayedRun`) and whose spawn is still pending or has a Monster
    /// alive. `LuaProxy::AllKilled` is false for exactly these (the story adds
    /// a proxy when it runs it; the host removes it once its monsters died).
    pub proxies_active: HashSet<u32>,
    /// Proxies placed in the world whose record has `delayedRun`: they run
    /// when the script calls `Run` (`Proxy::DelayedRun`); until then the host
    /// spawns no pool for them (`Story::is_present`).
    pub proxies_delayed: HashSet<u32>,
    /// Entity uids of level objects by object id (shrines).
    pub object_uids: HashMap<u32, [u32; 4]>,
    /// Uids of the devotion shrines restored on this difficulty
    /// (`StaticShrine::IsCleansed`).
    pub restored_shrines: HashSet<[u32; 4]>,
    /// Objects re-created from their level's saved state (`Entity::IsRestored`, Lua `IsReloaded`): the host
    /// adds a placed object here when its level comes back after being streamed out this session.
    pub restored_objects: HashSet<u32>,
}

/// Something the host must do (or may show). Drained with `Story::take_effects`.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// HUD notification text tag (`UI.Notify`).
    Notify(String),
    /// Centre-screen cinematic text tag (quest action 17).
    CinematicText(String),
    /// A script placed an object in the world (a proxy: it runs), `record` at `coords`. The host shows the
    /// world objects from `Story::entities` ([`super::Story::is_present`]: placed, removed and destroyed
    /// objects alike), so this is only the record of the placement (logs, tests).
    Spawn {
        id: u32,
        record: String,
        class: ObjClass,
        coords: Coords,
    },
    SetDoor {
        id: u32,
        open: bool,
    },
    SetLocked {
        id: u32,
        locked: bool,
    },
    Kill {
        id: u32,
    },
    GiveItem {
        record: String,
        count: i32,
        /// Fourth bool: a relic is given with a rolled completion bonus (`RollCompletionBonus`).
        complete_relic: bool,
    },
    GiveRandomItem {
        table: String,
        count: u32,
    },
    GiveMoney(i64),
    GiveTribute(i64),
    GiveExperience(u32),
    GiveLevels(u32),
    GiveSkillPoints(u32),
    GiveAttributePoints(u32),
    GiveDevotion(u32),
    /// `Game.UnlockNextDifficulty` (`GameEngine::GameWon`): the game was beaten.
    UnlockNextDifficulty,
    Faction {
        faction: String,
        delta: f32,
        set: bool,
    },
    UnlockFaction(String),
    /// `Game.KillAllMonsters`: every monster hostile to the player dies.
    KillAllMonsters,
    /// `Game.NotifyNewMutators(plural)`: cinematic `tagNewMutators` / `tagNewMutator`.
    NotifyNewMutators(bool),
    /// `Game.PlayFullscreenGlow` (`GameEngine::ShowGlowEffect`).
    FullscreenGlow,
    /// `Game.RemoveEndlessShrineBuffs` / `RemoveEndlessSoulBuffs` (`GameEngine::DispelEndlessShrineBuffs` /
    /// `DispelEndlessSoulBuffs`): the host removes those buffs from the player and the pets.
    RemoveEndlessShrineBuffs,
    RemoveEndlessSoulBuffs,
    /// `Game.ResetSurvivalEvent` (autosave first) / `Game.SaveSurvivalScore`.
    SurvivalAutosave,
    SaveSurvivalScore,
    /// `Game.UnlockNextSurvivalDifficulty` (`CompleteSurvivalDifficulty`).
    CompleteSurvivalDifficulty,
    PlaySound {
        record: String,
        at: Option<u32>,
    },
    /// Quest action 14 (`ScriptableAction_ControllerState`): the owner
    /// character (`id`, the conversation's NPC) plays the `AnimationSet_Type`
    /// `anim` once, if its animation set has that slot.
    PlayAnimation {
        id: Option<u32>,
        anim: u32,
        /// Third arg: hold the pose past the clip's end instead of reverting.
        hold: bool,
        /// `Some(allow)`: the controller's "QuestPlayAnimation" state (`Character.PlayAnimation`), `allow` its
        /// fourth arg, `ControllerAI.allowQuestInterruptions` (the monster fights an enemy only when set);
        /// `None`: a bare animation play with no controller state (quest action 14).
        quest: Option<bool>,
    },
    /// Quest action 22 (`ScriptableAction_CastSkill`): `caster` (the action's owner, a conversation's NPC) casts
    /// `skill` on the main player; None (a quest's action, no owner) does nothing (`gd_game::story::cast_story_skills`).
    CastSkill {
        skill: String,
        caster: Option<u32>,
    },
    /// `Character:UseSkillAction(skill, target, bool)`: the monster uses
    /// the named skill on `target` (an object id; none known: no target).
    UseSkill {
        id: u32,
        skill: String,
        target: u32,
    },
    /// `Character:MoveAction(target, coords, bool)`: the monster walks to
    /// the target object's position (else `coords`), then idles there.
    /// `interruptible` is the bool, the controller's `AllowQuestInterruptions`
    /// (`ControllerAI::QuestCommandMove` stores it): the "QuestMove" state
    /// reacts to enemies and attacks only when it is set.
    MoveAction {
        id: u32,
        target: u32,
        coords: Coords,
        interruptible: bool,
    },
    /// `Proxy:LinkPatrolPointGroup(name)`: the proxy's monsters patrol the
    /// level's named "Patrol Points" instance group.
    LinkPatrolPointGroup {
        proxy: u32,
        group: String,
    },
    /// `Character:TeleportHome()`: back to its spawn point, anger cleared.
    TeleportHome {
        id: u32,
    },
    /// `AreaTrigger:DestroyItems()`: every dropped item inside the trigger.
    DestroyItems {
        trigger: u32,
    },
    UnlockTutorial(u32),
    TeleportPlayer {
        coords: Coords,
    },
    ScriptEvent(String),
    /// Quest progress for logs / UI.
    QuestTaskStarted {
        quest: u32,
        task: u32,
    },
    QuestTaskCompleted {
        quest: u32,
        task: u32,
    },
    ObjectiveSatisfied {
        quest: u32,
        task: u32,
        objective: u32,
    },
    QuestCompleted {
        quest: String,
    },
    /// Open the dialog UI for this conversation (host calls `Story::talk`).
    ConversationAvailable {
        npc: u32,
    },
    /// `Player:TakeItem(record, count, bool)`: engine `QuestCommandTakeItem` → SP
    /// `TakeItemFromStashes`; `extra` is the forwarded bool (filter semantics traced by the host).
    TakeItem {
        record: String,
        count: i32,
        extra: bool,
    },
    /// `Character:ToggleInvincible(bool)` → `combat::Invincible` on the entity.
    SetInvincible {
        id: u32,
        on: bool,
    },
    /// `Character:ToggleSkills(bool)` → gate in the monster skill chooser.
    SetSkillsEnabled {
        id: u32,
        on: bool,
    },
    /// `Character:Move(vec)`: engine `RequestMove` — a plain, non-interruptible walk.
    Move {
        id: u32,
        coords: Coords,
    },
    /// `Character:Attack(target)`: engine `RequestAttack` — engage if factions are hostile.
    Attack {
        id: u32,
        target: u32,
    },
    /// `Character:Say(tag)`: presentational speech bubble (engine UI vcall); headless no-op.
    Say {
        id: u32,
        tag: String,
    },
    /// `Game.ScreenShake(vec, secs, amplitude)`: presentational camera shake; headless no-op.
    ScreenShake {
        at: Coords,
        ms: u32,
        amplitude: f32,
    },
    /// `Player:GiveAffixItem(record, prefix, suffix)`: item with fixed affixes, resolved at player level.
    GiveAffixItem {
        record: String,
        prefix: String,
        suffix: String,
    },
    /// `Destructible:Destroy(dir, force, bool)`: engine `BreakApart` — broken flag, object stays.
    BreakDestructible {
        id: u32,
        dir: Coords,
        force: f32,
        timed: bool,
    },
    /// `Door:SetAutoClose(bool)`: engine `FixedDoor::SetAutoClose` — close countdown while open.
    SetAutoClose {
        id: u32,
        on: bool,
    },
}
