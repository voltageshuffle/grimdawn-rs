//! The play session: which character is loaded ([`LoadedCharacter`], from `GD_SAVE` or the main menu,
//! `grimdawn::menu`), the player's choices ([`Choice`]: difficulty, mode, custom game) and the resulting
//! [`GameMode`], read through the `gd_game::session::Session` parameter. `gd_game::session::begin` starts a session
//! and inserts [`SessionReady`]; `gd_game::session::end_session_state` tears it down (Exit to Main Menu, `grimdawn::exit`, which also moves
//! [`SessionEpoch`] so [`NewSession`] caches reset). The port never writes to the game's save folder: [`SaveDir`]
//! is its own.
//!
//! Where the player appears ([`PlayerStart`]) is written by whoever knows the place: grimdawn's session
//! start (the respawn point's level entity in the streamed world) and `riftgates` (a bound respawn point).
use bevy::prelude::*;
use gd_formats::save::Shared;
use gd_formats::save::gdc::Gdc;
use gd_formats::save::map_dat::MapDat;
use std::path::PathBuf;
use std::sync::Arc;

/// How many sessions the process has torn down. Version counters that systems cache in a `Local`
/// and compare for equality (`Inventory::version`, `PlayerFactions::version`) start at
/// [`SessionEpoch::version_base`], so a stale `Local` of a finished session never equals a new one's.
#[derive(Resource, Default)]
pub struct SessionEpoch(pub u64);

impl SessionEpoch {
    pub fn version_base(&self) -> u64 {
        self.0 << 32
    }
}

/// For a system that caches what it built or saw in a `Local` (a signature, entity ids): `begun()`
/// is true once when a new session started after a teardown (`SessionEpoch` moved), the cue to drop
/// those caches, since the entities they describe are gone and a stale signature could equal the new
/// session's and skip a rebuild.
#[derive(bevy::ecs::system::SystemParam)]
pub struct NewSession<'w, 's> {
    epoch: Res<'w, SessionEpoch>,
    seen: Local<'s, u64>,
}

impl NewSession<'_, '_> {
    pub fn begun(&mut self) -> bool {
        let new = *self.seen != self.epoch.0;
        *self.seen = self.epoch.0;
        new
    }
}

pub fn frame_ms(carry: &mut f32, time: &Time) -> i32 {
    let ms = *carry + time.delta_secs() * 1000.0;
    let whole = ms.floor();
    *carry = ms - whole;
    whole as i32
}

/// The character and difficulty are chosen; the world may start.
#[derive(Resource)]
pub struct SessionReady;

/// The entities alive when the session began: `gd_game::session::begin` takes it in the same step that inserts
/// [`SessionReady`], and the exit to the main menu despawns every entity not in it (`grimdawn::exit`). Not on a later
/// frame: a session started from the menu can resolve its start point and spawn the player in the frame it begins,
/// and a player kept by the baseline would outlive the exit.
#[derive(Resource, Default)]
pub struct SessionBaseline(pub gd_formats::collections::HashSet<Entity>);

/// Run condition of everything that shows or reads the playing session (HUD,
/// windows, tooltips, hover): the session is chosen ([`SessionReady`]) and its
/// player is fully loaded, i.e. spawned with its progress and inventory
/// applied from the save (`init_progress`, `init_inventory`, a frame after the
/// spawn). PORT: the engine only creates the HUD when entering playing mode
/// (`GameEngine::EnableGameEngine`); here the widgets exist from startup, so
/// each system waits for this instead.
pub fn in_game(
    // Option: a run condition's params are validated before any state gate.
    ready: Option<Res<SessionReady>>,
    progress: Option<Res<crate::progress::PlayerProgress>>,
    inventory: Option<Res<crate::inventory::Inventory>>,
    players: Query<(), With<crate::player::Player>>,
) -> bool {
    ready.is_some() && progress.is_some() && inventory.is_some() && !players.is_empty()
}

/// The session's difficulty and challenge (`GameInfo`), set by `gd_game::session::begin`.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct GameMode(pub gd_rules::mode::Mode);

/// The session's character choice (from the environment or the menu, set by
/// `gd_game::session::begin`).
#[derive(Resource, Debug, Clone, Default)]
pub struct Choice {
    /// The loaded `player.gdc` (None: a new character).
    pub save: Option<std::path::PathBuf>,
    /// A new character's name and sex.
    pub name: String,
    pub female: bool,
    /// The character is hardcore: the menu's choice for a new one, the header flag of a loaded one (set by
    /// `gd_game::session::begin`). It picks the shared files' hardcore twins (`gd_formats::save::Shared`).
    pub hardcore: bool,
    /// A Crucible session (`GameInfo::SetMode(1)`): the arena's map resource (`maps/survivalworld_a.map`).
    pub survival: Option<String>,
    /// A custom game (`GameInfo::SetMode(2)`): the user mod and its map.
    pub custom: Option<CustomMap>,
}

/// A custom game's map (`GameInfo` mode 2, `SetModName` / `SetLevelName`): a user mod
/// (`mods/<module>/`) and one of its maps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomMap {
    pub module: String,
    /// The map's resource path (`levels/world001.map`).
    pub level: String,
}

impl CustomMap {
    /// The map's file name (the part after the last `/`): the `modText` box (`GameModeWindow::UpdateModText`).
    pub fn file_name(&self) -> &str {
        self.level.rsplit('/').next().unwrap_or(&self.level)
    }

    /// The list row, "<mod> ~ <file name>" (`GameModeWindow::FormatCustomMapRow`).
    pub fn row(&self) -> String {
        format!("{} ~ {}", self.module, self.file_name())
    }
}

/// The Crucible arenas in the game-mode window's drop-down order (after "Random"): the name tag and the
/// map letter (`GameModeWindow::Load` / the arena vector of `MainMenu::MainMenu`).
/// Bog / Woods (g, j) are the Ashes of Malmouth pack's (`survivalmode1`), Stars / Crags (h, i) Forgotten Gods'
/// (`survivalmode2`): they are listed when the pack is installed.
pub const ARENAS: [(&str, &str); 10] = [
    ("tagSurvivalArena_02", "a"),
    ("tagSurvivalArena_01", "b"),
    ("tagSurvivalArena_03", "c"),
    ("tagSurvivalArena_04", "d"),
    ("tagSurvivalArena_05", "e"),
    ("tagSurvivalArena_06", "f"),
    ("tagSurvivalArena_07", "g"),
    ("tagSurvivalArena_08", "j"),
    ("tagSurvivalArena_09", "h"),
    ("tagSurvivalArena_10", "i"),
];

/// The arenas the install offers, as indices into [`ARENAS`]: none without the Crucible
/// ([`gd_formats::vfs::survival_files_exist`], the port's `Engine::HasSurvivalDLC`); with it the base
/// arenas, and those of each expansion pack present. Empty means no Crucible: the game-mode window has no
/// Crucible toggle (`GameModeWindow::Load` creates `survivalButton` only when the DLC is owned).
pub fn available_arenas(root: &std::path::Path) -> Vec<usize> {
    if !gd_formats::vfs::survival_files_exist(root) {
        return Vec::new();
    }
    (0..ARENAS.len())
        .filter(|i| match ARENAS[*i].1 {
            "g" | "j" => root.join("survivalmode1").is_dir(),
            "h" | "i" => root.join("survivalmode2").is_dir(),
            _ => true,
        })
        .collect()
}

/// The arena map resource of a Crucible arena letter (`a`..`j`).
pub fn arena_map(letter: &str) -> String {
    format!("maps/survivalworld_{}.map", letter.to_ascii_lowercase())
}

impl Choice {
    /// A file of the loaded character's folder (`player.gdc`'s), if a
    /// character file is loaded.
    pub fn save_file(&self, rel: impl AsRef<std::path::Path>) -> Option<PathBuf> {
        self.save
            .as_ref()
            .map(|p| p.parent().unwrap_or(std::path::Path::new(".")).join(rel))
    }

    /// `GameInfo::GetMode`: 0 the campaign, 1 the Crucible, 2 a custom game.
    pub fn game_mode(&self) -> u8 {
        if self.survival.is_some() {
            1
        } else if self.custom.is_some() {
            2
        } else {
            0
        }
    }

    /// The session's mod (`GameInfo::GetModName`, `map.dat`'s `modName`): `survivalmode` for the
    /// Crucible (`MainMenu::SetMapData` sets it), the user mod of a custom game, empty for the campaign.
    pub fn mod_name(&self) -> &str {
        match (&self.survival, &self.custom) {
            (Some(_), _) => gd_formats::vfs::SURVIVAL_MOD,
            (None, Some(c)) => &c.module,
            (None, None) => "",
        }
    }

    /// The session's map (`map.dat`'s `mapPath`, `World::Load`): the Crucible arena, the custom
    /// game's map or the campaign's world.
    /// PORT: `GD_MAP` (a test hook) is not reflected; its saves name the world map.
    pub fn map_path(&self) -> &str {
        match (&self.survival, &self.custom) {
            (Some(m), _) => m,
            (None, Some(c)) => &c.level,
            (None, None) => "levels/world001.map",
        }
    }
}

/// The character's `last_difficulty` for a session in `mode` (`Player::SaveNewFormatData`):
/// `difficulty | ((game mode << 2) | challenge) << 4`; Veteran is challenge 1 on Normal,
/// Ultimate Veteran challenge 3 on Ultimate. Set on a loaded character when the session begins and written by
/// every save, a new character's too.
pub fn last_difficulty(mode: gd_rules::mode::Mode, choice: &Choice) -> u8 {
    (choice.game_mode() << 6) | ((mode.challenge as u8) << 4) | mode.difficulty.min(2) as u8
}

/// The save's last mode was Veteran (challenge 1).
pub fn is_veteran(g: &Gdc) -> bool {
    (g.info.last_difficulty >> 4) & 3 == 1
}

/// The save's last mode was Ultimate Veteran (challenge 3 on Ultimate).
pub fn is_ultimate_veteran(g: &Gdc) -> bool {
    (g.info.last_difficulty >> 4) & 3 == 3 && g.info.difficulty() == 2
}

/// The loaded character, if any.
#[derive(Resource, Clone)]
pub struct LoadedCharacter(pub Arc<Gdc>);

/// Where the player should appear. Absent until known.
#[derive(Resource, Clone, Copy)]
pub struct PlayerStart(pub Vec3);

/// The (difficulty, veteran) a session of `gdc` starts on: the save's, unless `GD_DIFFICULTY` / `GD_VETERAN` override it.
pub fn mode_for(hooks: &crate::hooks::SessionHooks, gdc: &Gdc) -> (usize, bool) {
    (
        hooks.difficulty.unwrap_or(gdc.info.difficulty()),
        hooks
            .veteran
            .unwrap_or(is_veteran(gdc) || is_ultimate_veteran(gdc)),
    )
}

/// Whether the session's character is hardcore.
pub fn is_hardcore(loaded: Option<&LoadedCharacter>, choice: &Choice) -> bool {
    match loaded {
        Some(l) => l.0.header.hardcore != 0,
        None => choice.hardcore,
    }
}

/// The game's own save folder: never written.
pub const LIVE_SAVES: &str = "My Games/Grim Dawn";

/// Writes a port-owned settings file (`what` names it in the log), creating its folder; refuses a path inside the game's own save
/// folder ([`LIVE_SAVES`]: the port never writes there). The one writer of the options and key binding files.
pub fn write_user_file(what: &str, file: &std::path::Path, contents: impl AsRef<[u8]>) {
    if file.to_string_lossy().contains(LIVE_SAVES) {
        error!("{what}: refusing to write {}", file.display());
        return;
    }
    match file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(file, contents))
    {
        Ok(()) => info!("{what}: saved {}", file.display()),
        Err(e) => error!("{what}: could not write {}: {e}", file.display()),
    }
}

/// The port's own data folder (saves, settings, key bindings): `%APPDATA%\grimdawn-rs` on Windows,
/// else `$XDG_DATA_HOME/grimdawn-rs`, else `~/.local/share/grimdawn-rs`.
pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    let base = PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default());
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    base.join("grimdawn-rs")
}

/// The port's save folder (`main/_<name>/player.gdc`, `transfer.gst`, ...):
/// a resource, so every save system reads the same value and a test gives
/// each game its own folder. The default (`GD_SAVE_DIR`, else
/// `<`[`data_dir`]`>/save`) is the only place the environment is read.
#[derive(Resource, Clone, Debug)]
pub struct SaveDir(pub PathBuf);

impl FromWorld for SaveDir {
    fn from_world(world: &mut World) -> Self {
        // Runs at plugin build (`init_resource`): the game inserts its `Hooks` before the plugins.
        if let Some(d) = world
            .get_resource::<crate::hooks::Hooks>()
            .and_then(|h| h.session.save_dir.clone())
        {
            return SaveDir(PathBuf::from(d));
        }
        SaveDir(data_dir().join("save"))
    }
}

impl SaveDir {
    pub fn join(&self, p: impl AsRef<std::path::Path>) -> PathBuf {
        self.0.join(p)
    }
}

/// The character's highest unlocked difficulty this session (saved as
/// `greatest_difficulty`).
#[derive(Resource)]
pub struct DifficultyUnlock(pub usize);

/// Highest unlocked difficulty of a character (see the module docs).
pub fn unlocked(g: &Gdc) -> usize {
    match g.info.greatest_difficulty {
        3 => 0,
        d => (d as usize).min(2),
    }
}

/// A hardcore character that died. `Grim Dawn.exe` character list
/// (`CharacterList::Draw`): an entry is drawn as dead when the player is hardcore and
/// `PlayStats::GetNumberOfDeaths` is non-zero (unless an app flag, cleared
/// at start-up, is set).
pub fn is_dead(g: &Gdc) -> bool {
    g.header.hardcore != 0 && g.stats.deaths > 0
}

/// The main menu's difficulty options, in the difficulty window's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DifficultyOption {
    Normal,
    Veteran,
    Elite,
    Ultimate,
    /// "Ascendant": Ultimate with challenge 3.
    UltimateVeteran,
}

impl DifficultyOption {
    pub const ALL: [DifficultyOption; 5] = [
        Self::Normal,
        Self::Veteran,
        Self::Elite,
        Self::Ultimate,
        Self::UltimateVeteran,
    ];

    /// (difficulty 0..=2, veteran / challenge flag) for `gd_game::session::begin`.
    pub fn mode(self) -> (usize, bool) {
        match self {
            Self::Normal => (0, false),
            Self::Veteran => (0, true),
            Self::Elite => (1, false),
            Self::Ultimate => (2, false),
            Self::UltimateVeteran => (2, true),
        }
    }

    /// The option a save last played.
    pub fn of_save(g: &Gdc) -> Self {
        match g.info.difficulty() {
            0 if is_veteran(g) => Self::Veteran,
            0 => Self::Normal,
            1 => Self::Elite,
            _ if is_ultimate_veteran(g) => Self::UltimateVeteran,
            _ => Self::Ultimate,
        }
    }

    /// The option a remembered (difficulty, challenge) pair names (`playmenu.cpn`).
    pub fn remembered(difficulty: u32, challenge: u32) -> Self {
        match (difficulty, challenge > 0) {
            (0, false) => Self::Normal,
            (0, true) => Self::Veteran,
            (1, _) => Self::Elite,
            (_, false) => Self::Ultimate,
            _ => Self::UltimateVeteran,
        }
    }

    /// This option, or the highest enabled one below it ([`DifficultyOption::enabled_in`]; in the Crucible a saved
    /// Veteran has no equivalent and falls to the Normal / Elite / Ultimate it sits on).
    pub fn clamp(self, g: Option<&Gdc>, crucible: bool) -> Self {
        let i = Self::ALL.iter().position(|d| *d == self).unwrap_or(0);
        Self::ALL[..=i]
            .iter()
            .rev()
            .copied()
            .find(|d| d.enabled_in(g, crucible))
            .unwrap_or(Self::Normal)
    }

    /// Whether a character (None: a new one) may pick this option.
    /// `Grim Dawn.exe` difficulty window enables (`DifficultyWindow::UpdateEnabled`, campaign
    /// mode): Normal and Veteran always; Elite once Elite is unlocked
    /// ([`unlocked`] ≥ 1); Ultimate once Ultimate is (≥ 2); Ultimate
    /// Veteran once Ultimate is unlocked and the character is level 100+.
    pub fn enabled(self, g: Option<&Gdc>) -> bool {
        let u = g.map_or(0, unlocked);
        match self {
            Self::Normal | Self::Veteran => true,
            Self::Elite => u >= 1,
            Self::Ultimate => u >= 2,
            Self::UltimateVeteran => u >= 2 && g.is_some_and(|g| g.header.level >= 100),
        }
    }

    /// [`DifficultyOption::enabled`] in a game mode: the Crucible's three difficulties
    /// (`DifficultyWindow::UpdateEnabled`): Normal always, Elite once the character's greatest survival
    /// difficulty completed is 1 or more, Ultimate once it is 2 (`info.greatest_survival_difficulty`);
    /// the Veteran challenges do not exist there.
    pub fn enabled_in(self, g: Option<&Gdc>, crucible: bool) -> bool {
        if !crucible {
            return self.enabled(g);
        }
        let u = g.map_or(0, |g| g.info.greatest_survival_difficulty as usize);
        match self {
            Self::Normal => true,
            Self::Elite => u >= 1,
            Self::Ultimate => u >= 2,
            Self::Veteran | Self::UltimateVeteran => false,
        }
    }

    /// The main menu's difficulty button text (`MainMenu::GetDifficultyButtonText`).
    pub fn button_tag(self) -> &'static str {
        match self {
            Self::Normal => "tagDifficultyNormal",
            Self::Veteran => "tagDifficultyNormalVeteran",
            Self::Elite => "tagDifficultyElite",
            Self::Ultimate => "tagDifficultyUltimate",
            Self::UltimateVeteran => "tagDifficultyUltimateVeteran",
        }
    }
}

impl Choice {
    /// The per-map save folder name under `_<Name>/` (`GameEngine::GetDifficultyFolder`:
    /// the map path with `/` and `\` turned into `_`): the campaign's world map
    /// (`levels_world001.map`), the Crucible arena (`maps_survivalworld_a.map`) or a custom game's map. It holds
    /// `<Normal|Elite|Ultimate>/{quests.gdd, conversations.gdd, map.fow}`. The Crucible's tokens are not in it:
    /// they are the survival vectors of `player.gdc` (`gd_game::survival::load_tokens`).
    /// The folder also holds `map.dat` (`gd_game::save::write_map_dat`).
    pub fn map_folder(&self) -> String {
        self.map_path().replace(['/', '\\'], "_")
    }

    /// The folder the character `name` is saved in (`GameEngine::GetPlayerFolder`):
    /// `main/_<name>/` for a character in the main quest (`Player::IsInMainQuest`, the save's
    /// `is_in_main_quest`) or in a Crucible session (mode 1), else `user/_<name>/` (a character created
    /// in a custom game).
    pub fn player_folder(
        &self,
        save_dir: &SaveDir,
        name: &str,
        in_main_quest: bool,
    ) -> std::path::PathBuf {
        let kind = if in_main_quest || self.game_mode() == 1 {
            "main"
        } else {
            "user"
        };
        save_dir.join(kind).join(format!("_{name}"))
    }

    /// The folder of the account-wide files (`transfer.gst`, `formulas.gst`, `transmutes.gst`,
    /// `reagents.gst`, the potion unlocks; `GameEngine::GetSharedSavePath`): the
    /// save folder for the campaign and the Crucible (mode 1 uses an empty mod name), `<save>/<mod>/` for
    /// a custom game, so a user mod has its own transfer stash. Every read and write of those files goes
    /// through here.
    pub fn shared_save_dir(&self, save_dir: &SaveDir) -> std::path::PathBuf {
        match self.mod_name() {
            m if m.is_empty() || self.game_mode() == 1 => save_dir.0.clone(),
            m => save_dir.join(m),
        }
    }

    /// Where the account-wide `file` is written ([`Self::shared_save_dir`] and its name, the hardcore twin for a
    /// hardcore character: `gd_formats::save::Shared::file_name`).
    pub fn shared_file(&self, save_dir: &SaveDir, file: Shared) -> std::path::PathBuf {
        self.shared_save_dir(save_dir)
            .join(file.file_name(self.hardcore))
    }

    /// The loaded character's `map.dat` for this map and `difficulty` (0..=2), if any.
    pub fn read_map_dat(&self, difficulty: usize) -> Option<MapDat> {
        let path = self.save_file(
            std::path::Path::new(&self.map_folder())
                .join(crate::save::DIFFICULTY_DIRS[difficulty.min(2)])
                .join("map.dat"),
        )?;
        let bytes = std::fs::read(&path).ok()?;
        MapDat::parse(&bytes)
            .inspect_err(|e| warn!("{}: {e:#}", path.display()))
            .ok()
    }

    /// Why this pick must not start an existing save: its `map.dat` names a different mod, and the
    /// per-map folder is the map path alone, so starting would read and overwrite another mod's (or
    /// the campaign's) quest/fog state. The Crucible (mode 1) is exempt: its arena folders are its own.
    /// `None` when the save is fresh or agrees with the pick.
    pub fn mod_mismatch(&self, difficulty: usize) -> Option<String> {
        if self.game_mode() == 1 {
            return None;
        }
        let dat = self.read_map_dat(difficulty)?;
        (!dat.mod_name.eq_ignore_ascii_case(self.mod_name())).then(|| {
            format!(
                "{} was saved in {}; the pick is {}",
                self.map_path(),
                mod_label(&dat.mod_name),
                mod_label(self.mod_name()),
            )
        })
    }
}

/// A mod name in a refusal message: the campaign when empty (`map.dat`'s `modName`).
fn mod_label(m: &str) -> &str {
    if m.is_empty() { "the campaign" } else { m }
}
