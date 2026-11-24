//! Test hooks (`GD_*` environment variables, one typed field each, documented below), parsed once at startup into
//! [`Hooks`]; systems read the resource instead of `std::env`. A `dev` build of the game inserts
//! `Hooks::from_env()` (gated by this crate's `dev` feature, which gd-game's `dev` forwards); any other build inserts
//! the empty [`Hooks::default`] (= `from_vars([])`), so a release build reads no variable and its hook reads are
//! inert. Tests never depend on the process environment: a test that needs a hook builds the app with
//! `Hooks::from_vars`.
//!
//! Every variable has a typed field, grouped by area: [`SessionHooks`] (`session`: character, menu skipping, world
//! and save folder), [`UiHooks`] (`ui`: windows, hovers, scripted clicks, menus), [`RenderHooks`] (`render`:
//! switches, camera, time of day, character / model scenes), [`ShotHooks`] (`shot`: screenshots), [`GameplayHooks`]
//! (`gameplay`: scripted combat, world and story actions), [`ItemHooks`] (`items`: trade, crafting, loot, tooltips)
//! and [`AuditHooks`] (`audit`: census, exit cycles, memory). Parsing happens once in [`Hooks::from_vars`]; a malformed value
//! gives `None` / the documented default. Where the value is a small mini-language that only one system
//! understands (`GD_FX`, `GD_MODELS`, `GD_CHARS`, `GD_PORTAL`, ...) the field keeps the raw `String` and that system
//! parses it. To add a hook: add the field (with a doc comment giving its syntax) to the area struct, parse it in
//! `from_vars` and read it in a system through `Res<Hooks>`.
//!
//! There is no process-wide copy: `main` builds the hooks first and inserts them before the plugins, so
//! plugin `build`s (which decide what to register), `FromWorld` resources (`SaveDir`, `TimeOfDay`) and systems
//! all read the one resource of the world; helpers get the typed value from their calling system.
use bevy::math::Vec2;
use bevy::prelude::*;
use gd_formats::collections::HashMap;

#[derive(Resource, Clone, Debug)]
pub struct Hooks {
    /// Which character the session starts with, whether the menu is skipped, the world and the save folder.
    pub session: SessionHooks,
    pub ui: UiHooks,
    pub render: RenderHooks,
    pub shot: ShotHooks,
    pub gameplay: GameplayHooks,
    pub items: ItemHooks,
    pub audit: AuditHooks,
}

impl Default for Hooks {
    fn default() -> Self {
        Self::from_vars(std::iter::empty())
    }
}

/// `GD_SAVE` / `GD_NAME` / ...: the session's start without the main menu.
#[derive(Clone, Debug, Default)]
pub struct SessionHooks {
    /// The menu is skipped: a character or a test session is given (`GD_SAVE`, `GD_NAME`, `GD_TESTWORLD`,
    /// `GD_SHOT`, `GD_NO_MENU`, `GD_CHARS`) and `GD_MENU` is not set.
    pub skip_menu: bool,
    /// `GD_SAVE`: the `player.gdc` to load (none: a new character).
    pub save: Option<String>,
    /// `GD_NAME` (new character, default `Player`), `GD_FEMALE`, `GD_HARDCORE`.
    pub name: Option<String>,
    pub female: bool,
    pub hardcore: bool,
    /// `GD_VIDEO=<engine path>` (`video/intro.ogg`): plays that video when the player enters the game, whatever the
    /// character. Without it a session that skips the menu (the hooks above) plays no intro.
    pub video: Option<String>,
    /// `GD_DIFFICULTY` (0..=2) and `GD_VETERAN` (`0` = off) override the save's.
    pub difficulty: Option<usize>,
    pub veteran: Option<bool>,
    /// `GD_START=x,z` / `x,y,z`: the player's start position.
    pub start: Option<Vec<f32>>,
    /// `GD_SAVE_DIR`: where the port saves (default `save` in `session::data_dir`).
    pub save_dir: Option<String>,
    /// `GD_AUTOSAVE_SECS`: autosave after that many seconds.
    pub autosave_secs: Option<f32>,
    /// `GD_MAP`: the world map resource (default `levels/world001.map`; with `GD_SURVIVAL` the arena's).
    pub map: Option<String>,
    /// `GD_SURVIVAL=<letter>` (`a`..`j`): a Crucible session on `survivalworld_<letter>` without the menu
    /// (the menu's game-mode window is the player's way, `grimdawn::menu`): the Crucible mod
    /// (`gd_formats::vfs::SURVIVAL_MOD`) is mounted from startup and the story is in survival mode.
    pub survival: Option<String>,
    /// `GD_MOD=<name>`: a custom-game session on the user mod `mods/<name>` without the menu
    /// (`Choice::custom`, `gd_game::session::load_save`); `GD_MOD_MAP` picks its map resource
    /// (default `levels/world001.map`, read through the mounted mod).
    pub module: Option<String>,
    pub mod_map: Option<String>,
    /// `GD_OPTIONS`: an options.txt to load instead of the port's.
    pub options_file: Option<String>,
    /// `GD_TESTWORLD` (`far`: the dummies far away), `GD_DUMMY=<record>`, `GD_ARENA_MONSTERS=<record>[*n];...`.
    pub testworld: Option<String>,
    pub dummy: Option<String>,
    pub arena_monsters: Vec<(String, usize)>,
    /// `GD_CHARS=<spec>;...`: the character scene (raw, parsed by grimdawn's `dev::chars`).
    pub chars: Option<String>,
}

/// Windows, hovers and scripted input.
#[derive(Clone, Debug, Default)]
pub struct UiHooks {
    /// `GD_HOVER`: a fixed pointer position instead of the OS cursor (raw: any value hides the cursor).
    pub hover: Option<String>,
    /// `GD_HOVER=x,y` (logical px) when it is a position.
    pub hover_px: Option<Vec2>,
    /// `GD_UI_OPEN`: the window open at start (`inventory`/`character`, `skills`, `devotion`, `factions`,
    /// `quests`, `potions`, `achievements`, `localmap`, `worldmap`, `lootfilter`).
    pub open: Option<String>,
    /// `GD_UI_TAB`: the opened window's tab.
    pub tab: Option<u8>,
    /// `GD_SHEET_SCROLL`: character pages' list scroll (ui units).
    pub sheet_scroll: f32,
    /// `GD_SHEET_COLLAPSE`: collapsed header fields of the character pages.
    pub sheet_collapse: Vec<String>,
    /// `GD_HOTBAR_PICK` / `_CHOOSE` / `_PAGE` / `_HOVER`.
    pub hotbar_pick: Option<usize>,
    pub hotbar_choose: Option<usize>,
    pub hotbar_page: Option<usize>,
    pub hotbar_hover: Option<usize>,
    /// `GD_OPTION_SET=<secs>:<name>=<value>;...`: sets an option at runtime (what Apply does in memory, nothing
    /// is stored), `GD_HUD_CLICK=<secs>:<HUD master field>;...`: a left click on that HUD button.
    pub option_set: Vec<(f32, String, String)>,
    pub hud_click: Vec<(f32, String)>,
    /// `GD_MOUSE=<secs>:<event>;...` (real seconds after the player spawned): the real mouse, through the window's cursor
    /// and button input: `x,y` moves the pointer (physical px), `item` onto the label of the nearest ground item
    /// (no gold), `down` / `up` the left button, `rdown` / `rup` the right one, `log` only logs (grimdawn `dev`);
    /// at most one event a frame.
    pub mouse: Vec<(f32, String)>,
    /// `GD_HUD_HOVER=<button field>`, `GD_SHEET_HOVER=<row key suffix | button>`, `GD_POTION_HOVER=<part>`.
    pub hud_hover: Option<String>,
    pub sheet_hover: Option<String>,
    pub potion_hover: Option<String>,
    /// `GD_FACTION_SELECT=<row>`, `GD_FACTION_HOVER=bar|tier<n>|row<n>|rowbar<n>|booster<n>` (the factions window).
    pub faction_select: Option<usize>,
    pub faction_hover: Option<String>,
    /// `GD_SKILL_HOVER=x,y`, `GD_HOT_HOVER=x,y` (physical px), `GD_BUFF_HOVER=<n>`.
    pub skill_hover: Option<Vec2>,
    pub hot_hover: Option<Vec2>,
    pub buff_hover: Option<usize>,
    /// `GD_DEBUFF_TARGET=<skill record>`: the skill's debuff lands once on each character the pointer hovers
    /// (`hover.rs`, the target debuff strip checks).
    pub debuff_target: Option<String>,
    /// `GD_SUNDERED=<v>`: the HUD sees a type-61 duration entry of value `v` on the player (the sundered overlay).
    pub sundered: Option<f32>,
    /// `GD_SKILL_CLICKS`: scripted clicks of the skill window.
    pub skill_clicks: Vec<Click>,
    /// `GD_POTION_CONTAINER=<n>`, `GD_POTION_SLOT=0|1`, `GD_POTION_SCROLL`.
    pub potion_container: Option<usize>,
    pub potion_slot: Option<usize>,
    pub potion_scroll: f32,
    /// `GD_QUEST_SCROLL`, `GD_HELP_SELECT`, `GD_LORE_SELECT`, `GD_QUEST_SELECT`.
    pub quest_scroll: f32,
    pub help_select: Option<String>,
    /// `GD_TUTORIAL_TIP=<page>@<secs>`: unlocks that tutorial page at that time (its quick tip shows).
    pub tutorial_tip: Option<(u32, f32)>,
    pub lore_select: Option<String>,
    pub quest_select: Option<String>,
    /// `GD_QUEST_REWARD=<secs>`: opens the Quest Complete window with a sample reward (money, a faction gain and
    /// loss, a skill point, an item ×2) at that time (`grimdawn::ui::questpopup`).
    pub quest_reward: Option<f32>,
    /// `GD_MAP_ZOOM` (unclamped).
    pub map_zoom: Option<f32>,
    /// `GD_DEVOTION_SCROLL=x,y`, `GD_DEVOTION_ZOOM`, `GD_DEVOTION_HOVER=<star skill record>`.
    pub devotion_scroll: Option<Vec2>,
    pub devotion_zoom: Option<f32>,
    pub devotion_hover: Option<String>,
    /// `GD_DEVOTION_SEARCH=<text>`: the devotion window's search box starts with that text.
    pub devotion_search: Option<String>,
    /// `GD_DEVOTION_PICK=<star skill record>`: opens that star's skill-select list.
    pub devotion_pick: Option<String>,
    /// `GD_MENU_CREDITS=1`: the main menu opens on the credits roll.
    pub menu_credits: bool,
    /// `GD_OPTIONS_OPEN=<tab>`, `GD_OPTIONS_DROP=<n>`, `GD_OPTIONS_REBIND=<action>:<0|1>`.
    pub options_open: Option<String>,
    pub options_drop: Option<usize>,
    pub options_rebind: Option<(u32, usize)>,
    /// `GD_GAME_MENU=<secs>`, `GD_GAME_MENU_CONFIRM=exit|quit`, `GD_GAME_MENU_YES`, `GD_PAUSE=<secs>`.
    pub game_menu: Option<f32>,
    pub game_menu_confirm: Option<String>,
    pub game_menu_yes: bool,
    pub pause: Option<f32>,
    /// `GD_DIALOG_CLICK=<row>[,<row>...]@<secs>`: the Nth conversation of the run (the Nth time the window opens)
    /// clicks the Nth row of the list (the last one for the later ones).
    pub dialog_click: Option<(Vec<usize>, f32)>,
    /// `GD_SHRINE_ACCEPT=<secs>`.
    pub shrine_accept: Option<f32>,
    /// `GD_RIFT_PICK=<n>`.
    pub rift_pick: Option<usize>,
    /// `GD_WEAPON_SWAP=<secs>[,...]`.
    pub weapon_swap: Vec<f32>,
    /// `GD_PET_SELECT` / `GD_PET_MENU` (secs), `GD_PET_STANCE` (lower case), `GD_PET_ORDER=<secs>@monster|<secs>@x,z;...`.
    pub pet_select: Option<f32>,
    pub pet_menu: Option<f32>,
    pub pet_stance: Option<String>,
    pub pet_order: Vec<(f32, Option<Vec2>)>,
    /// Main menu: `GD_MENU_OPEN`, `GD_MENU_NAME`, `GD_MENU_ACCEPT`, `GD_MENU_PICK=i[,d]`, `GD_MENU_PICK_AFTER=<secs>`.
    pub menu_open: Option<String>,
    pub menu_name: Option<String>,
    pub menu_accept: bool,
    pub menu_pick: Option<(usize, Option<usize>)>,
    pub menu_pick_after: Option<f32>,
    /// `GD_MENU_CRUCIBLE=<n>`: the menu is in Crucible mode with the game-mode window's drop-down item `n` (0 = Random).
    pub menu_crucible: Option<usize>,
    /// `GD_MENU_CUSTOM=1`: an open game-mode window (`GD_MENU_OPEN=gametype`) shows the Custom view.
    pub menu_custom: bool,
}

/// Rendering switches, camera, time of day, model / character scenes.
#[derive(Clone, Debug, Default)]
pub struct RenderHooks {
    /// `GD_RES=WxH`: the window size.
    pub res: Option<(u32, u32)>,
    /// `GD_TIME=<hour>`, `GD_CAM_YAW=<deg>`, `GD_CAM_DIST`.
    pub time: Option<f32>,
    pub cam_yaw: Option<f32>,
    pub cam_dist: Option<f32>,
    /// `GD_MODELS="path.msh[@anim.anm];..."` (raw), `GD_FX=<rec>;...` (raw), `GD_FX_GRID` (default 1), `GD_FX_SKIP=<emitters>`.
    pub models: Option<String>,
    /// `GD_WEATHER=<climate_weather record>` (a name under `records/fx/climate_weather/` or a full path): that climate is
    /// the weather everywhere (it still re-rolls its intensities).
    pub weather: Option<String>,
    pub fx: Option<String>,
    pub fx_grid: usize,
    pub fx_skip: Option<String>,
    /// `GD_NO_FX`, `GD_NO_FX_LIGHTS`, `GD_NO_DECALS`, `GD_NO_TERRAIN`, `GD_NO_WATER`, `GD_NO_SEE_THROUGH`.
    pub no_fx: bool,
    pub no_fx_lights: bool,
    pub no_decals: bool,
    pub no_terrain: bool,
    pub no_water: bool,
    pub no_see_through: bool,
    /// `GD_SHADOWS=0`, `GD_GLOW=0`, `GD_DOF=0` turn the effect off; `GD_FOG=1` turns the fog option on.
    pub no_shadows: bool,
    pub no_glow: bool,
    pub no_dof: bool,
    pub fog: bool,
    /// `GD_CLEAR` (magenta clear colour), `GD_FPS` (frame time log).
    pub clear: bool,
    pub fps: bool,
    /// `GD_DOLL_ROT=<radians>`, `GD_DOLL_HOLD=0|1`.
    pub doll_rot: f32,
    pub doll_hold: Option<usize>,
    /// Character scene: `GD_CHAR_KEYS`, `GD_CHAR_CAM`, `GD_CHAR_YAW` (deg), `GD_CHAR_STATE=run|attack|death`,
    /// `GD_CHAR_HOLD=<secs>`.
    pub char_keys: bool,
    pub char_cam: Option<f32>,
    pub char_yaw: Option<f32>,
    pub char_state: Option<String>,
    pub char_hold: Option<f32>,
}

/// `GD_SHOT*`: the automatic screenshot.
#[derive(Clone, Debug, Default)]
pub struct ShotHooks {
    /// `GD_SHOT=<png>`: take the screenshot and exit.
    pub path: Option<String>,
    /// `GD_SHOT_FRAMES` (default 120), `GD_SHOT_SECS` (the watchdog, default 90).
    pub frames: u32,
    pub secs: f32,
    /// `GD_SHOT_ON=<record text>` and `GD_SHOT_DELAY=<s>,...` (default 0.3; empty without `GD_SHOT_ON`).
    pub on: Option<String>,
    pub delays: Vec<f32>,
}

/// Scripted combat, world and story actions.
#[derive(Clone, Debug, Default)]
pub struct GameplayHooks {
    /// `GD_XP`, `GD_NEMESIS_KILLS`, `GD_DREAD_KILLS`.
    pub xp: Option<u64>,
    pub nemesis_kills: Option<u32>,
    pub dread_kills: Option<u32>,
    /// `GD_LEARN=<skill>:<level>;...`
    pub learn: Vec<(String, u32)>,
    /// `GD_SLOT=<n>:<skill>;...`
    pub slot: Vec<(usize, String)>,
    /// `GD_CAST=<slot>@<secs>;...` as `(secs, slot)`, `GD_CAST_AT=x,z`.
    pub cast: Vec<(f32, usize)>,
    pub cast_at: Option<Vec2>,
    /// `GD_WALK_TO=x,z[;x,z...]`: a route walked point by point (each leg within 60 m: the nav is the loaded
    /// levels'), `GD_PATH_TEST=x,z`.
    pub walk_to: Vec<Vec2>,
    pub path_test: Option<Vec2>,
    /// `GD_SPAWN_MONSTER=<record>@x,z;...`
    pub spawn_monster: Vec<(String, f32, f32)>,
    /// `GD_EVADE=<secs>`, `GD_DIE=<secs>`, `GD_MOVE_KEY=<secs>` (holds the Move key that long).
    pub evade: Option<f32>,
    pub die: Option<f32>,
    pub move_key: Option<f32>,
    /// `GD_GOD`: the player's life is kept at its maximum (a test hook, e.g. to finish a Crucible tier).
    pub god: bool,
    /// `GD_LIFE_PCT=<percent>`: the player's life is held at that percent of its maximum (low-health overlay checks).
    pub life_pct: Option<f32>,
    /// `GD_NOT_READY=<secs>`: the "skill not ready" text is raised at that time (`ui/errormsg.rs`).
    pub not_ready: Option<f32>,
    /// `GD_FACTION=<NAME>=<value>;...`, `GD_TOKENS=A,B`.
    pub faction: Vec<(String, f32)>,
    pub tokens: Vec<String>,
    /// `GD_DEBUFFS`, `GD_AUTO_FIGHT`, `GD_OPEN_CHESTS`, `GD_NO_OBSTACLES`.
    pub debuffs: bool,
    pub auto_fight: bool,
    /// `GD_AUTO_FIGHT=<metres>` (default 20): how far the test bot looks for a target.
    pub auto_fight_range: f32,
    /// `GD_KILL_ALL=<secs>`: every `secs` of play `Game.KillAllMonsters` runs (a stand-in for a bot that reaches every
    /// monster: waves and floors complete).
    pub kill_all_secs: Option<f32>,
    /// `GD_OVERKILL=1`: the `GD_KILL_ALL` kills overkill (gibs).
    pub overkill: bool,
    /// `GD_USE_EXIT_GATE=1`: the player uses a Shattered Realm exit gate as soon as there is one (no walk).
    pub use_exit_gate: bool,
    /// `GD_USE_BUFF_SHRINE=1`: the player uses the nearest unused Shattered Realm buff shrine at once (no walk).
    pub use_buff_shrine: bool,
    /// `GD_ENDLESS_FLOOR=<floor>`: floor 0 of every generated Shattered Realm section is this floor record
    /// (a held override; a bare name like `specialfloor_06` or a full record path). A test hook
    /// (`endless::force_floor`); placed (story) monsters log their level once armed.
    pub endless_floor: Option<String>,
    pub open_chests: bool,
    /// `GD_OPEN_CHESTS=<metres>` (more than 1): how far a chest may be (default 30).
    pub open_chests_range: f32,
    pub no_obstacles: bool,
    /// `GD_USE_DOOR[=text]` (lower case; "1": any), `GD_USE_RIFTGATE`, `GD_USE_SHRINE`, `GD_USE_ALTAR`, `GD_USE_CAULDRON`, `GD_USE_SPIRIT_GUIDE`.
    pub use_door: Option<String>,
    pub use_riftgate: bool,
    pub use_shrine: bool,
    pub use_altar: bool,
    pub use_cauldron: bool,
    pub use_spirit_guide: bool,
    /// `GD_PORTAL=<secs>[:open]` (raw), `GD_BREAK` (raw), `GD_DEBRIS_TEST=<record>`, `GD_FX_TEST=<record>`.
    pub portal: Option<String>,
    pub break_mode: Option<String>,
    pub debris_test: Option<String>,
    pub fx_test: Option<String>,
    /// `GD_STORY_AUTOTALK`, `GD_STORY_TALK`.
    pub story_autotalk: bool,
    pub story_talk: bool,
    /// `GD_STORY_TALK=<text>[,<text>...]` (not "1"): only NPCs whose record contains one of the (lower case)
    /// texts; an NPC matching an earlier text is talked to first.
    pub story_talk_filter: Option<Vec<String>>,
    /// `GD_LUA=[@<secs>:]<code>`: runs this Lua once (after `secs` of play) after the story's first frame (a test hook: start a script flow directly).
    pub lua: Option<String>,
    /// `GD_POTION=<h|e>@<secs>;...` as `(secs, health)`, `GD_POTION_SELECT=<skill>;...`, `GD_USE_ITEM=<record>;...`.
    pub potion: Vec<(f32, bool)>,
    pub potion_select: Vec<String>,
    pub use_item: Vec<String>,
}

/// Trade, crafting, loot and tooltips.
#[derive(Clone, Debug, Default)]
pub struct ItemHooks {
    /// `GD_HOVER_ITEM=<record>[,<relic>[,<prefix>[,<suffix>]]]` (raw), `GD_HOVER_PRICE`, `GD_HOVER_SPOT=ground|equipped`.
    pub hover_item: Option<String>,
    pub hover_price: Option<u64>,
    pub hover_spot: Option<String>,
    /// `GD_TRADE=<window>` (raw; a service kind like `merchant`, or text of a service NPC's record such as
    /// `merchantfaction`), `GD_TRADE_BUY`, `GD_TRANSFER`, `GD_REAGENTS=<n>`,
    /// `GD_VENDOR_SEARCH=<text>` (types into the open vendor / stash / crafter window's search box).
    pub trade: Option<String>,
    pub trade_buy: bool,
    pub transfer: bool,
    pub reagents: Option<usize>,
    pub vendor_search: Option<String>,
    /// `GD_CRAFT=<text>`, `GD_DISMANTLE`, `GD_SALVAGE=item|addon|augment|look`, `GD_SALVAGE_PICK`, `GD_CONVERT=same|random`,
    /// `GD_REROLL=<kind>`.
    pub craft: Option<String>,
    pub dismantle: bool,
    pub salvage: Option<String>,
    pub salvage_pick: Option<String>,
    pub convert: Option<String>,
    pub reroll: Option<String>,
    /// `GD_ILLUSION=<n>` (a malformed number is 0), `GD_ILLUSION_APPLY`.
    pub illusion: Option<usize>,
    pub illusion_apply: bool,
    /// `GD_ALTAR_OFFER=<record>;...` (raw).
    pub altar_offer: Option<String>,
    /// `GD_EQUIP=<record>[@left];...` (raw), `GD_GIVE=<record>[*n];...` (raw), `GD_AUTO_LOOT`.
    pub equip: Option<String>,
    pub give: Option<String>,
    pub auto_loot: bool,
    /// `GD_DROP_ITEMS=<record>;...` (raw), `GD_DROP_AFTER=<secs>`, `GD_LABEL_MODE=all|common`.
    pub drop_items: Option<String>,
    pub drop_after: Option<f32>,
    pub label_mode: Option<String>,
    /// `GD_INV_CLICKS`: scripted clicks of the inventory window (and of the stash window).
    pub inv_clicks: Vec<Click>,
    /// `GD_APPLY_YES=<secs>`: answers Yes to the apply mode's confirmation once it has been up `secs`.
    pub apply_yes: Option<f32>,
    /// `GD_STACK=<amount>@<secs>[:ok]`: after `secs`, the stack window opens on the first bag stack (or, with the stash
    /// open, stash stack) of more than one and `amount` is typed in; `:ok` presses OK a second later.
    pub stack: Option<StackHook>,
}

/// [`ItemHooks::stack`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackHook {
    pub amount: u32,
    pub secs: f32,
    pub ok: bool,
}

impl StackHook {
    fn parse(v: &str) -> Option<Self> {
        let (v, ok) = match v.strip_suffix(":ok") {
            Some(v) => (v, true),
            None => (v, false),
        };
        let (a, t) = v.split_once('@')?;
        Some(Self {
            amount: a.trim().parse().ok()?,
            secs: t.trim().parse().ok()?,
            ok,
        })
    }
}

/// `GD_CENSUS` / `GD_EXIT_CYCLES` / `GD_MEM_*`: leak audits.
#[derive(Clone, Debug, Default)]
pub struct AuditHooks {
    /// `GD_CENSUS=<file>` (`GD_CENSUS_SECS`, default 25).
    pub census: Option<String>,
    pub census_secs: f32,
    /// `GD_EXIT_CYCLES=<n>` (`GD_EXIT_SECS`, default 12).
    pub exit_cycles: Option<u32>,
    pub exit_secs: f32,
    /// `GD_MEM_LOG=<secs>`: logs the memory audit (`mem:`, grimdawn `dev/memory.rs`) every `secs`; `GD_MEM_TRIM`:
    /// returns the allocator's free memory to the system (`malloc_trim`) before each log.
    pub mem_log: Option<f32>,
    pub mem_trim: bool,
    /// `GD_MEM_TOUR=<n>@<secs>`: travels to `n` riftgates of the map, one `secs` after each arrival.
    pub mem_tour: Option<(usize, f32)>,
    /// `GD_MEM_EXIT=<secs>`: the first session exits to the main menu `secs` after the player spawned (after
    /// the tour), and the same character starts again from the menu 5 s later (once).
    pub mem_exit: Option<f32>,
}

impl Hooks {
    /// The process environment's hooks: only `dev` builds read it (the game inserts `Hooks::default()` otherwise).
    #[cfg(feature = "dev")]
    pub fn from_env() -> Self {
        Self::from_vars(std::env::vars())
    }

    /// The hooks of the given variables (only `GD_*` names count).
    pub fn from_vars(vars: impl IntoIterator<Item = (String, String)>) -> Self {
        let vars: HashMap<String, String> = vars
            .into_iter()
            .filter(|(k, _)| k.starts_with("GD_"))
            .collect();
        let s = |k: &str| vars.get(k).cloned();
        let has = |k: &str| vars.contains_key(k);
        let p = |k: &str| -> Option<Vec2> { vars.get(k).and_then(|v| point(v)) };
        let n = |k: &str| -> Option<usize> { vars.get(k).and_then(|v| v.parse().ok()) };
        let f = |k: &str| -> Option<f32> { vars.get(k).and_then(|v| v.parse().ok()) };
        // `;`-separated entries without the empty ones.
        let list = |k: &str| -> Vec<String> {
            vars.get(k)
                .map(|v| {
                    v.split(';')
                        .filter(|e| !e.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let is = |k: &str, want: &str| vars.get(k).is_some_and(|v| v == want);
        let session = SessionHooks {
            skip_menu: !has("GD_MENU")
                && [
                    "GD_SAVE",
                    "GD_NAME",
                    "GD_TESTWORLD",
                    "GD_SHOT",
                    "GD_NO_MENU",
                    "GD_CHARS",
                    "GD_MOD",
                ]
                .iter()
                .any(|v| has(v)),
            save: s("GD_SAVE"),
            name: s("GD_NAME"),
            female: has("GD_FEMALE"),
            hardcore: has("GD_HARDCORE"),
            difficulty: n("GD_DIFFICULTY"),
            veteran: s("GD_VETERAN").map(|v| v != "0"),
            start: s("GD_START").map(|v| {
                v.split(',')
                    .filter_map(|x| x.trim().parse::<f32>().ok())
                    .collect()
            }),
            video: s("GD_VIDEO"),
            save_dir: s("GD_SAVE_DIR"),
            autosave_secs: f("GD_AUTOSAVE_SECS"),
            map: s("GD_MAP").or_else(|| {
                s("GD_SURVIVAL")
                    .map(|l| format!("maps/survivalworld_{}.map", l.to_ascii_lowercase()))
            }),
            survival: s("GD_SURVIVAL"),
            module: s("GD_MOD"),
            mod_map: s("GD_MOD_MAP"),
            options_file: s("GD_OPTIONS"),
            testworld: s("GD_TESTWORLD"),
            dummy: s("GD_DUMMY"),
            arena_monsters: list("GD_ARENA_MONSTERS")
                .iter()
                .map(|g| {
                    g.split_once('*')
                        .map_or((g.as_str(), 1), |(r, c)| (r, c.parse().unwrap_or(1)))
                })
                .map(|(r, c)| (r.to_owned(), c))
                .collect(),
            chars: s("GD_CHARS"),
        };
        let ui = UiHooks {
            hover: s("GD_HOVER"),
            hover_px: p("GD_HOVER"),
            open: s("GD_UI_OPEN"),
            tab: vars.get("GD_UI_TAB").and_then(|t| t.parse().ok()),
            sheet_scroll: f("GD_SHEET_SCROLL").unwrap_or(0.0),
            sheet_collapse: vars
                .get("GD_SHEET_COLLAPSE")
                .map(|v| {
                    v.split(',')
                        .filter(|c| !c.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            hotbar_pick: n("GD_HOTBAR_PICK"),
            hotbar_choose: n("GD_HOTBAR_CHOOSE"),
            hotbar_page: n("GD_HOTBAR_PAGE"),
            hotbar_hover: n("GD_HOTBAR_HOVER"),
            option_set: list("GD_OPTION_SET")
                .iter()
                .filter_map(|e| {
                    let (t, kv) = e.split_once(':')?;
                    let (k, v) = kv.split_once('=')?;
                    Some((t.parse().ok()?, k.to_owned(), v.to_owned()))
                })
                .collect(),
            hud_click: list("GD_HUD_CLICK")
                .iter()
                .filter_map(|e| {
                    let (t, f) = e.split_once(':')?;
                    Some((t.parse().ok()?, f.to_owned()))
                })
                .collect(),
            mouse: list("GD_MOUSE")
                .iter()
                .filter_map(|e| {
                    let (t, f) = e.split_once(':')?;
                    Some((t.parse().ok()?, f.to_owned()))
                })
                .collect(),
            hud_hover: s("GD_HUD_HOVER"),
            sheet_hover: s("GD_SHEET_HOVER"),
            potion_hover: s("GD_POTION_HOVER"),
            faction_select: n("GD_FACTION_SELECT"),
            faction_hover: s("GD_FACTION_HOVER"),
            skill_hover: p("GD_SKILL_HOVER"),
            hot_hover: p("GD_HOT_HOVER"),
            buff_hover: n("GD_BUFF_HOVER"),
            debuff_target: s("GD_DEBUFF_TARGET"),
            sundered: f("GD_SUNDERED"),
            skill_clicks: clicks(vars.get("GD_SKILL_CLICKS")),
            potion_container: n("GD_POTION_CONTAINER"),
            potion_slot: n("GD_POTION_SLOT").filter(|s| *s < 2),
            potion_scroll: f("GD_POTION_SCROLL").unwrap_or(0.0),
            quest_scroll: f("GD_QUEST_SCROLL").unwrap_or(0.0),
            help_select: s("GD_HELP_SELECT"),
            tutorial_tip: s("GD_TUTORIAL_TIP").and_then(|v| {
                let (p, t) = v.split_once('@').unwrap_or((&v, "1"));
                Some((p.parse().ok()?, t.parse().ok()?))
            }),
            lore_select: s("GD_LORE_SELECT"),
            quest_select: s("GD_QUEST_SELECT"),
            quest_reward: f("GD_QUEST_REWARD"),
            map_zoom: f("GD_MAP_ZOOM"),
            devotion_scroll: p("GD_DEVOTION_SCROLL"),
            devotion_zoom: f("GD_DEVOTION_ZOOM"),
            devotion_hover: s("GD_DEVOTION_HOVER"),
            devotion_search: s("GD_DEVOTION_SEARCH"),
            devotion_pick: s("GD_DEVOTION_PICK"),
            menu_credits: vars.get("GD_MENU_CREDITS").is_some_and(|v| v == "1"),
            options_open: s("GD_OPTIONS_OPEN"),
            options_drop: n("GD_OPTIONS_DROP"),
            options_rebind: vars.get("GD_OPTIONS_REBIND").and_then(|v| {
                v.split_once(':')
                    .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
            }),
            game_menu: f("GD_GAME_MENU"),
            game_menu_confirm: s("GD_GAME_MENU_CONFIRM"),
            game_menu_yes: has("GD_GAME_MENU_YES"),
            pause: f("GD_PAUSE"),
            dialog_click: vars.get("GD_DIALOG_CLICK").and_then(|v| {
                v.split_once('@').and_then(|(r, t)| {
                    let rows: Vec<usize> = r.split(',').filter_map(|x| x.parse().ok()).collect();
                    (!rows.is_empty()).then_some((rows, t.parse().ok()?))
                })
            }),
            shrine_accept: f("GD_SHRINE_ACCEPT"),
            rift_pick: n("GD_RIFT_PICK"),
            weapon_swap: vars
                .get("GD_WEAPON_SWAP")
                .map(|v| v.split(',').filter_map(|t| t.parse().ok()).collect())
                .unwrap_or_default(),
            pet_select: f("GD_PET_SELECT"),
            pet_menu: f("GD_PET_MENU"),
            pet_stance: s("GD_PET_STANCE").map(|v| v.to_ascii_lowercase()),
            pet_order: list("GD_PET_ORDER")
                .iter()
                .filter_map(|e| {
                    let (t, what) = e.split_once('@')?;
                    let at = match what.trim() {
                        "monster" => None,
                        xz => {
                            let (x, z) = xz.split_once(',')?;
                            Some(Vec2::new(x.trim().parse().ok()?, z.trim().parse().ok()?))
                        }
                    };
                    Some((t.trim().parse().ok()?, at))
                })
                .collect(),
            menu_open: s("GD_MENU_OPEN"),
            menu_name: s("GD_MENU_NAME"),
            menu_accept: has("GD_MENU_ACCEPT"),
            menu_pick: vars.get("GD_MENU_PICK").and_then(|v| {
                let mut it = v.split(',').map(|x| x.trim().parse::<usize>().ok());
                Some((it.next()??, it.next().flatten()))
            }),
            menu_pick_after: f("GD_MENU_PICK_AFTER"),
            menu_crucible: f("GD_MENU_CRUCIBLE").map(|v| v as usize),
            menu_custom: has("GD_MENU_CUSTOM"),
        };
        let render = RenderHooks {
            res: vars.get("GD_RES").and_then(|r| {
                r.split_once('x')
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
            }),
            time: f("GD_TIME"),
            cam_yaw: f("GD_CAM_YAW"),
            cam_dist: f("GD_CAM_DIST"),
            models: s("GD_MODELS"),
            weather: s("GD_WEATHER"),
            fx: s("GD_FX"),
            fx_grid: n("GD_FX_GRID").unwrap_or(1),
            fx_skip: s("GD_FX_SKIP"),
            no_fx: has("GD_NO_FX"),
            no_fx_lights: has("GD_NO_FX_LIGHTS"),
            no_decals: has("GD_NO_DECALS"),
            no_terrain: has("GD_NO_TERRAIN"),
            no_water: has("GD_NO_WATER"),
            no_see_through: has("GD_NO_SEE_THROUGH"),
            no_shadows: is("GD_SHADOWS", "0"),
            no_glow: is("GD_GLOW", "0"),
            no_dof: is("GD_DOF", "0"),
            fog: is("GD_FOG", "1"),
            clear: has("GD_CLEAR"),
            fps: has("GD_FPS"),
            doll_rot: f("GD_DOLL_ROT").unwrap_or(0.0),
            doll_hold: n("GD_DOLL_HOLD").filter(|i| *i < 2),
            char_keys: has("GD_CHAR_KEYS"),
            char_cam: f("GD_CHAR_CAM"),
            char_yaw: f("GD_CHAR_YAW"),
            char_state: s("GD_CHAR_STATE"),
            char_hold: f("GD_CHAR_HOLD"),
        };
        let shot = ShotHooks {
            path: s("GD_SHOT"),
            frames: vars
                .get("GD_SHOT_FRAMES")
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
            secs: f("GD_SHOT_SECS").unwrap_or(90.0),
            on: s("GD_SHOT_ON"),
            delays: if has("GD_SHOT_ON") {
                vars.get("GD_SHOT_DELAY")
                    .map_or("0.3", String::as_str)
                    .split(',')
                    .filter_map(|d| d.trim().parse().ok())
                    .collect()
            } else {
                Vec::new()
            },
        };
        let gameplay = GameplayHooks {
            xp: vars.get("GD_XP").and_then(|v| v.parse().ok()),
            nemesis_kills: vars.get("GD_NEMESIS_KILLS").and_then(|v| v.parse().ok()),
            dread_kills: vars.get("GD_DREAD_KILLS").and_then(|v| v.parse().ok()),
            learn: list("GD_LEARN")
                .iter()
                .filter_map(|x| x.rsplit_once(':'))
                .filter_map(|(r, lv)| Some((r.to_owned(), lv.parse().ok()?)))
                .collect(),
            slot: list("GD_SLOT")
                .iter()
                .filter_map(|x| x.split_once(':'))
                .filter_map(|(n, r)| Some((n.parse().ok()?, r.to_owned())))
                .collect(),
            cast: list("GD_CAST")
                .iter()
                .filter_map(|c| {
                    let (s, t) = c.split_once('@')?;
                    Some((t.parse().ok()?, s.parse().ok()?))
                })
                .collect(),
            cast_at: p("GD_CAST_AT"),
            walk_to: list("GD_WALK_TO").iter().filter_map(|v| point(v)).collect(),
            path_test: p("GD_PATH_TEST"),
            spawn_monster: list("GD_SPAWN_MONSTER")
                .iter()
                .filter_map(|g| {
                    let (rec, at) = g.split_once('@')?;
                    let (x, z) = at.split_once(',')?;
                    Some((
                        rec.to_owned(),
                        x.trim().parse().ok()?,
                        z.trim().parse().ok()?,
                    ))
                })
                .collect(),
            evade: f("GD_EVADE"),
            die: f("GD_DIE"),
            move_key: f("GD_MOVE_KEY"),
            god: has("GD_GOD"),
            life_pct: f("GD_LIFE_PCT"),
            not_ready: f("GD_NOT_READY"),
            faction: list("GD_FACTION")
                .iter()
                .filter_map(|x| x.split_once('='))
                .filter_map(|(n, v)| Some((n.to_owned(), v.parse().ok()?)))
                .collect(),
            tokens: vars
                .get("GD_TOKENS")
                .map(|v| {
                    v.split(',')
                        .filter(|t| !t.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            debuffs: has("GD_DEBUFFS"),
            auto_fight: has("GD_AUTO_FIGHT"),
            auto_fight_range: f("GD_AUTO_FIGHT").unwrap_or(20.0),
            kill_all_secs: f("GD_KILL_ALL"),
            overkill: vars.get("GD_OVERKILL").is_some_and(|v| v == "1"),
            use_exit_gate: has("GD_USE_EXIT_GATE"),
            use_buff_shrine: has("GD_USE_BUFF_SHRINE"),
            endless_floor: s("GD_ENDLESS_FLOOR").filter(|v| !v.is_empty()).map(|v| {
                let v = v.to_ascii_lowercase();
                if v.contains('/') {
                    v
                } else {
                    format!("records/endlessdungeon/floors/{v}.dbr")
                }
            }),
            open_chests: has("GD_OPEN_CHESTS"),
            open_chests_range: f("GD_OPEN_CHESTS").filter(|m| *m > 1.0).unwrap_or(30.0),
            no_obstacles: has("GD_NO_OBSTACLES"),
            use_door: s("GD_USE_DOOR").map(|v| v.to_ascii_lowercase()),
            use_riftgate: has("GD_USE_RIFTGATE"),
            use_shrine: has("GD_USE_SHRINE"),
            use_altar: has("GD_USE_ALTAR"),
            use_cauldron: has("GD_USE_CAULDRON"),
            use_spirit_guide: has("GD_USE_SPIRIT_GUIDE"),
            portal: s("GD_PORTAL"),
            break_mode: s("GD_BREAK"),
            debris_test: s("GD_DEBRIS_TEST"),
            fx_test: s("GD_FX_TEST"),
            story_autotalk: has("GD_STORY_AUTOTALK"),
            story_talk: has("GD_STORY_TALK"),
            story_talk_filter: s("GD_STORY_TALK")
                .map(|v| v.to_ascii_lowercase())
                .filter(|v| v != "1" && !v.is_empty())
                .map(|v| v.split(',').map(str::to_owned).collect()),
            lua: s("GD_LUA"),
            potion: list("GD_POTION")
                .iter()
                .filter_map(|c| c.split_once('@'))
                .filter_map(|(k, t)| Some((t.parse().ok()?, k == "h")))
                .collect(),
            potion_select: list("GD_POTION_SELECT"),
            use_item: list("GD_USE_ITEM"),
        };
        let items = ItemHooks {
            hover_item: s("GD_HOVER_ITEM"),
            hover_price: vars.get("GD_HOVER_PRICE").and_then(|v| v.parse().ok()),
            hover_spot: s("GD_HOVER_SPOT"),
            trade: s("GD_TRADE"),
            trade_buy: has("GD_TRADE_BUY"),
            transfer: has("GD_TRANSFER"),
            reagents: n("GD_REAGENTS"),
            vendor_search: s("GD_VENDOR_SEARCH"),
            craft: s("GD_CRAFT"),
            dismantle: has("GD_DISMANTLE"),
            salvage: s("GD_SALVAGE"),
            salvage_pick: s("GD_SALVAGE_PICK"),
            convert: s("GD_CONVERT"),
            reroll: s("GD_REROLL"),
            illusion: s("GD_ILLUSION").map(|v| v.parse().unwrap_or(0)),
            illusion_apply: has("GD_ILLUSION_APPLY"),
            altar_offer: s("GD_ALTAR_OFFER"),
            equip: s("GD_EQUIP"),
            give: s("GD_GIVE"),
            auto_loot: has("GD_AUTO_LOOT"),
            drop_items: s("GD_DROP_ITEMS"),
            drop_after: f("GD_DROP_AFTER"),
            label_mode: s("GD_LABEL_MODE"),
            inv_clicks: clicks(vars.get("GD_INV_CLICKS")),
            apply_yes: f("GD_APPLY_YES"),
            stack: vars.get("GD_STACK").and_then(|v| StackHook::parse(v)),
        };
        let audit = AuditHooks {
            census: s("GD_CENSUS"),
            census_secs: f("GD_CENSUS_SECS").unwrap_or(25.0),
            exit_cycles: vars.get("GD_EXIT_CYCLES").and_then(|v| v.parse().ok()),
            exit_secs: f("GD_EXIT_SECS").unwrap_or(12.0),
            mem_log: f("GD_MEM_LOG"),
            mem_trim: has("GD_MEM_TRIM"),
            mem_tour: vars.get("GD_MEM_TOUR").and_then(|v| {
                let (n, secs) = v.split_once('@')?;
                Some((n.parse().ok()?, secs.parse().ok()?))
            }),
            mem_exit: f("GD_MEM_EXIT"),
        };
        Self {
            session,
            ui,
            render,
            shot,
            gameplay,
            items,
            audit,
        }
    }
}

/// `x,y` as a point (no whitespace allowed, as the hooks always read it).
fn point(v: &str) -> Option<Vec2> {
    let (x, y) = v.split_once(',')?;
    Some(Vec2::new(x.parse().ok()?, y.parse().ok()?))
}

/// A scripted click list, `[l:|r:]x,y@secs;...` (physical px, seconds since the app started; `r:` is the right
/// button): the test hooks that click a window without a mouse (`GD_INV_CLICKS`, `GD_SKILL_CLICKS`). Malformed entries are skipped.
fn clicks(v: Option<&String>) -> Vec<Click> {
    let Some(v) = v else {
        return Vec::new();
    };
    v.split(';')
        .filter_map(|c| {
            let (right, rest) = match c.split_once(':') {
                Some((b, rest)) => (b == "r", rest),
                None => (false, c),
            };
            let (xy, t) = rest.split_once('@')?;
            let (x, y) = xy.split_once(',')?;
            Some(Click {
                secs: t.trim().parse().ok()?,
                right,
                at: Vec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?),
            })
        })
        .collect()
}

/// One scripted click (`GD_INV_CLICKS`, `GD_SKILL_CLICKS`: [`ItemHooks::inv_clicks`], [`UiHooks::skill_clicks`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Click {
    pub secs: f32,
    pub right: bool,
    pub at: bevy::math::Vec2,
}

/// Removes and returns the first click of `script` that is due at `elapsed` seconds.
pub fn take_due(script: &mut Vec<Click>, elapsed: f32) -> Option<Click> {
    script
        .iter()
        .position(|c| elapsed >= c.secs)
        .map(|i| script.remove(i))
}
