//! The play session: the loaded character ([`LoadedCharacter`]), the player's [`Choice`] and the resulting
//! [`GameMode`]. `gd_game::session::begin` starts a session and inserts [`SessionReady`];
//! `gd_game::session::end_session_state` tears it down and moves [`SessionEpoch`]. The port never writes to the
//! game's save folder: [`SaveDir`] is its own.
use bevy::prelude::*;
use gd_formats::save::Shared;
use gd_formats::save::gdc::Gdc;
use gd_formats::save::map_dat::MapDat;
use std::path::PathBuf;
use std::sync::Arc;

/// How many sessions the process has torn down. Version counters cached in a `Local` start at
/// [`SessionEpoch::version_base`], so a stale `Local` of a finished session never equals a new one's.
#[derive(Resource, Default)]
pub struct SessionEpoch(pub u64);

impl SessionEpoch {
    pub fn version_base(&self) -> u64 {
        self.0 << 32
    }
}

/// `begun()` is true once when a new session started after a teardown: the cue for a system to drop
/// `Local` caches (signatures, entity ids) of the old session.
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

#[derive(Resource)]
pub struct SessionReady;

/// The entities alive when the session began; the exit to the main menu despawns every other. Taken in the
/// same step that inserts [`SessionReady`]: the player can spawn in the frame the session begins.
#[derive(Resource, Default)]
pub struct SessionBaseline(pub gd_formats::collections::HashSet<Entity>);

/// Run condition of everything that shows or reads the playing session: [`SessionReady`] and the player
/// spawned with its progress and inventory applied. PORT: the engine creates the HUD only on entering
/// playing mode (`GameEngine::EnableGameEngine`); here the widgets exist from startup.
pub fn in_game(
    // Option: run condition params are validated before any state gate.
    ready: Option<Res<SessionReady>>,
    progress: Option<Res<crate::progress::PlayerProgress>>,
    inventory: Option<Res<crate::inventory::Inventory>>,
    players: Query<(), With<crate::player::Player>>,
) -> bool {
    ready.is_some() && progress.is_some() && inventory.is_some() && !players.is_empty()
}

/// The session's difficulty and challenge (`GameInfo`).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct GameMode(pub gd_rules::mode::Mode);

/// The session's character choice.
#[derive(Resource, Debug, Clone, Default)]
pub struct Choice {
    /// The loaded `player.gdc` (None: a new character).
    pub save: Option<std::path::PathBuf>,
    /// A new character's name and sex.
    pub name: String,
    pub female: bool,
    /// The menu's choice for a new character, the header flag of a loaded one. Picks the shared files'
    /// hardcore twins.
    pub hardcore: bool,
    /// A Crucible session (`GameInfo::SetMode(1)`): the arena's map resource.
    pub survival: Option<String>,
    /// A custom game (`GameInfo::SetMode(2)`): the user mod and its map.
    pub custom: Option<CustomMap>,
}

/// A custom game's map (`GameInfo` mode 2, `SetModName` / `SetLevelName`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomMap {
    pub module: String,
    /// The map's resource path (`levels/world001.map`).
    pub level: String,
}

impl CustomMap {
    /// The map's file name: the `modText` box (`GameModeWindow::UpdateModText`).
    pub fn file_name(&self) -> &str {
        self.level.rsplit('/').next().unwrap_or(&self.level)
    }

    /// The list row, "<mod> ~ <file name>" (`GameModeWindow::FormatCustomMapRow`).
    pub fn row(&self) -> String {
        format!("{} ~ {}", self.module, self.file_name())
    }
}

/// The Crucible arenas in the game-mode window's drop-down order: the name tag and the map letter
/// (`GameModeWindow::Load`). g, j need `survivalmode1`; h, i need `survivalmode2`.
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

/// The arenas the install offers, as indices into [`ARENAS`]; empty without the Crucible
/// (`Engine::HasSurvivalDLC`), which hides the game-mode window's Crucible toggle.
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

pub fn arena_map(letter: &str) -> String {
    format!("maps/survivalworld_{}.map", letter.to_ascii_lowercase())
}

impl Choice {
    /// A file of the loaded character's folder, if a character is loaded.
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

    /// The session's mod (`GameInfo::GetModName`): `survivalmode` for the Crucible, the user mod of a
    /// custom game, empty for the campaign.
    pub fn mod_name(&self) -> &str {
        match (&self.survival, &self.custom) {
            (Some(_), _) => gd_formats::vfs::SURVIVAL_MOD,
            (None, Some(c)) => &c.module,
            (None, None) => "",
        }
    }

    /// The session's map (`map.dat`'s `mapPath`). PORT: `GD_MAP` is not reflected; its saves name the
    /// world map.
    pub fn map_path(&self) -> &str {
        match (&self.survival, &self.custom) {
            (Some(m), _) => m,
            (None, Some(c)) => &c.level,
            (None, None) => "levels/world001.map",
        }
    }
}

/// The character's `last_difficulty` (`Player::SaveNewFormatData`):
/// `difficulty | ((game mode << 2) | challenge) << 4`; Veteran is challenge 1 on Normal, Ultimate Veteran
/// challenge 3 on Ultimate.
pub fn last_difficulty(mode: gd_rules::mode::Mode, choice: &Choice) -> u8 {
    (choice.game_mode() << 6) | ((mode.challenge as u8) << 4) | mode.difficulty.min(2) as u8
}

pub fn is_veteran(g: &Gdc) -> bool {
    (g.info.last_difficulty >> 4) & 3 == 1
}

pub fn is_ultimate_veteran(g: &Gdc) -> bool {
    (g.info.last_difficulty >> 4) & 3 == 3 && g.info.difficulty() == 2
}

#[derive(Resource, Clone)]
pub struct LoadedCharacter(pub Arc<Gdc>);

/// Where the player should appear; absent until known.
#[derive(Resource, Clone, Copy)]
pub struct PlayerStart(pub Vec3);

pub fn mode_for(gdc: &Gdc) -> (usize, bool) {
    (
        gdc.info.difficulty(),
        is_veteran(gdc) || is_ultimate_veteran(gdc),
    )
}

pub fn is_hardcore(loaded: Option<&LoadedCharacter>, choice: &Choice) -> bool {
    match loaded {
        Some(l) => l.0.header.hardcore != 0,
        None => choice.hardcore,
    }
}

/// The game's own save folder: never written.
pub const LIVE_SAVES: &str = "My Games/Grim Dawn";

/// Writes a port-owned settings file, creating its folder; refuses a path inside [`LIVE_SAVES`].
pub fn write_user_file(what: &str, file: &std::path::Path, contents: impl AsRef<[u8]>) {
    try_write_user_file(what, file, contents);
}

/// [`write_user_file`], telling whether the file was written.
pub fn try_write_user_file(what: &str, file: &std::path::Path, contents: impl AsRef<[u8]>) -> bool {
    if file.to_string_lossy().contains(LIVE_SAVES) {
        error!("{what}: refusing to write {}", file.display());
        return false;
    }
    match file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(file, contents))
    {
        Ok(()) => {
            info!("{what}: saved {}", file.display());
            true
        }
        Err(e) => {
            error!("{what}: could not write {}: {e}", file.display());
            false
        }
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

/// The port's save folder; default `<`[`data_dir`]`>/save`, a test gives each game its own.
#[derive(Resource, Clone, Debug)]
pub struct SaveDir(pub PathBuf);

impl Default for SaveDir {
    fn default() -> Self {
        SaveDir(data_dir().join("save"))
    }
}

impl SaveDir {
    pub fn join(&self, p: impl AsRef<std::path::Path>) -> PathBuf {
        self.0.join(p)
    }
}

/// The character's highest unlocked difficulty this session (`greatest_difficulty`).
#[derive(Resource)]
pub struct DifficultyUnlock(pub usize);

/// Highest unlocked difficulty of a character (`greatest_difficulty` 3 means none).
pub fn unlocked(g: &Gdc) -> usize {
    match g.info.greatest_difficulty {
        3 => 0,
        d => (d as usize).min(2),
    }
}

/// A hardcore character that died (`CharacterList::Draw`).
pub fn is_dead(g: &Gdc) -> bool {
    g.header.hardcore != 0 && g.stats.deaths > 0
}

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

    /// (difficulty 0..=2, veteran / challenge flag).
    pub fn mode(self) -> (usize, bool) {
        match self {
            Self::Normal => (0, false),
            Self::Veteran => (0, true),
            Self::Elite => (1, false),
            Self::Ultimate => (2, false),
            Self::UltimateVeteran => (2, true),
        }
    }

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

    /// This option, or the highest enabled one below it ([`DifficultyOption::enabled_in`]).
    pub fn clamp(self, g: Option<&Gdc>, crucible: bool) -> Self {
        let i = Self::ALL.iter().position(|d| *d == self).unwrap_or(0);
        Self::ALL[..=i]
            .iter()
            .rev()
            .copied()
            .find(|d| d.enabled_in(g, crucible))
            .unwrap_or(Self::Normal)
    }

    /// Whether a character (None: a new one) may pick this option (`DifficultyWindow::UpdateEnabled`,
    /// campaign): Elite needs [`unlocked`] ≥ 1, Ultimate ≥ 2, Ultimate Veteran also level 100+.
    pub fn enabled(self, g: Option<&Gdc>) -> bool {
        let u = g.map_or(0, unlocked);
        match self {
            Self::Normal | Self::Veteran => true,
            Self::Elite => u >= 1,
            Self::Ultimate => u >= 2,
            Self::UltimateVeteran => u >= 2 && g.is_some_and(|g| g.header.level >= 100),
        }
    }

    /// [`DifficultyOption::enabled`] in a game mode: in the Crucible, Elite / Ultimate need
    /// `greatest_survival_difficulty` ≥ 1 / 2, and there are no Veteran challenges.
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
    /// The per-map save folder name under `_<Name>/` (`GameEngine::GetDifficultyFolder`): the map path with
    /// `/` and `\` turned into `_`. Holds `map.dat` and `<Normal|Elite|Ultimate>/{quests.gdd,
    /// conversations.gdd, map.fow}`.
    pub fn map_folder(&self) -> String {
        self.map_path().replace(['/', '\\'], "_")
    }

    /// The folder the character `name` is saved in (`GameEngine::GetPlayerFolder`): `main/_<name>/` in the
    /// main quest or the Crucible, else `user/_<name>/`.
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

    /// The folder of the account-wide files (`GameEngine::GetSharedSavePath`): the save folder for the
    /// campaign and the Crucible, `<save>/<mod>/` for a custom game.
    pub fn shared_save_dir(&self, save_dir: &SaveDir) -> std::path::PathBuf {
        match self.mod_name() {
            m if m.is_empty() || self.game_mode() == 1 => save_dir.0.clone(),
            m => save_dir.join(m),
        }
    }

    /// Where the account-wide `file` is written (the hardcore twin for a hardcore character).
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

    /// Why this pick must not start an existing save: its `map.dat` names a different mod, whose quest/fog
    /// state it would overwrite (the per-map folder is the map path alone). The Crucible is exempt.
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

/// A mod name in a refusal message: the campaign when empty.
fn mod_label(m: &str) -> &str {
    if m.is_empty() { "the campaign" } else { m }
}
