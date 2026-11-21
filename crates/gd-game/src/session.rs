//! The play session: which character is loaded ([`LoadedCharacter`], from `GD_SAVE` or the main menu,
//! `grimdawn::menu`), the player's choices ([`Choice`]: difficulty, mode, custom game) and the resulting
//! [`GameMode`], read through the [`Session`] parameter. [`begin`] starts a session and inserts
//! [`SessionReady`]; [`end_session_state`] tears it down (Exit to Main Menu, `grimdawn::exit`, which also moves
//! [`SessionEpoch`] so [`NewSession`] caches reset). The port never writes to the game's save folder: [`SaveDir`]
//! is its own.
//!
//! Where the player appears ([`PlayerStart`]) is written by whoever knows the place: grimdawn's session
//! start (the respawn point's level entity in the streamed world) and `riftgates` (a bound respawn point).
use bevy::prelude::*;
use gd_formats::save::gdc::Gdc;
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

/// The frame time in whole milliseconds for the game's integer-ms timers, carrying the sub-ms
/// remainder in `carry` (a system `Local`) so the timers advance at real time at any frame rate
/// (truncating would lose up to 1 ms per frame, ~13% at 144 Hz).
pub fn frame_ms(carry: &mut f32, time: &Time) -> i32 {
    let ms = *carry + time.delta_secs() * 1000.0;
    let whole = ms.floor();
    *carry = ms - whole;
    whole as i32
}

/// Request messages of a system that is gated with `run_if(cond)` on the resources it needs: while
/// `cond` is false nobody reads them, and Bevy keeps a message for two frames, so a request made
/// before the resource existed would be applied when it appears. This system reads (drops) them
/// instead. Register it with [`MessageDrain::drain_messages_unless`], next to the gated consumer.
/// PORT: the engine's requests are method calls that do nothing without the object; the message
/// queue is the stand-in, and "no object, no effect" is this drain.
pub fn clear_messages<M: Message>(mut messages: MessageReader<M>) {
    messages.clear();
}

/// `App::drain_messages_unless::<M>(cond)`: [`clear_messages`] for `M` while `cond` (the run condition
/// of the consumer) is false.
pub trait MessageDrain {
    fn drain_messages_unless<M: Message, Marker>(
        &mut self,
        cond: impl SystemCondition<Marker> + Clone,
    ) -> &mut Self;
}

impl MessageDrain for App {
    fn drain_messages_unless<M: Message, Marker>(
        &mut self,
        cond: impl SystemCondition<Marker> + Clone,
    ) -> &mut Self {
        self.add_systems(Update, clear_messages::<M>.run_if(not(cond)))
    }
}

/// The character and difficulty are chosen; the world may start.
#[derive(Resource)]
pub struct SessionReady;

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

/// The session's difficulty and challenge (`GameInfo`), set by [`begin`].
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct GameMode(pub gd_sim::mode::Mode);

/// The session state the rules read beyond the database: the mode and the
/// altar's formulas.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Session<'w> {
    pub mode: Res<'w, GameMode>,
    pub altar: Res<'w, crate::ascendant::AltarState>,
}

impl Session<'_> {
    pub fn mode(&self) -> gd_sim::mode::Mode {
        self.mode.0
    }

    pub fn ascendant(&self) -> &gd_sim::ascendant::Ascendant {
        &self.altar.formulas
    }

    /// Loot parameters with the session's mode (its difficulty) and altar modifiers.
    pub fn loot(&self, parent_level: u32, player_level: u32) -> gd_sim::items::LootParams {
        let mode = self.mode.0;
        gd_sim::items::LootParams::new(parent_level, player_level, mode.difficulty as u32)
            .with_session(mode, &self.altar.formulas)
    }
}

/// The session's character choice (from the environment or the menu, set by
/// [`begin`]).
#[derive(Resource, Debug, Clone, Default)]
pub struct Choice {
    /// The loaded `player.gdc` (None: a new character).
    pub save: Option<std::path::PathBuf>,
    /// A new character's name and sex.
    pub name: String,
    pub female: bool,
    /// The character is hardcore: the menu's choice for a new one, the header flag of a loaded one (set by
    /// [`begin`]). It picks the shared files' hardcore twins (`gd_formats::save::Shared`).
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
    /// The map's file name (the part after the last `/`): the `modText` box (sub_1400913D0).
    pub fn file_name(&self) -> &str {
        self.level.rsplit('/').next().unwrap_or(&self.level)
    }

    /// The list row, "<mod> ~ <file name>" (sub_1400C1E10).
    pub fn row(&self) -> String {
        format!("{} ~ {}", self.module, self.file_name())
    }
}

/// The Crucible arenas in the game-mode window's drop-down order (after "Random"): the name tag and the
/// map letter (`sub_140091820` / the arena vector of `sub_1400B9B30`).
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

/// The arenas the install offers, as indices into [`ARENAS`].
pub fn available_arenas(root: &std::path::Path) -> Vec<usize> {
    (0..ARENAS.len())
        .filter(|i| match ARENAS[*i].1 {
            "g" | "j" => root.join("survivalmode1").is_dir(),
            "h" | "i" => root.join("survivalmode2").is_dir(),
            _ => root.join("mods/survivalmode").is_dir(),
        })
        .collect()
}

/// The arena map resource of a Crucible arena letter (`a`..`j`).
pub fn arena_map(letter: &str) -> String {
    format!("maps/survivalworld_{}.map", letter.to_ascii_lowercase())
}

/// Mounts the session's mod (`GameData::set_mod`: `Engine::InitializeMod`, or `None`:
/// `RemoveModDirectory`) and drops the caches built from the other data: monster profiles, creature
/// bodies and animations (a mod overrides records and files). Nothing is dropped when that mod is
/// already mounted.
pub fn set_mod_data(world: &mut World, name: Option<&str>) -> anyhow::Result<()> {
    let data = world.resource::<crate::data::GameData>().clone();
    if data.active_mod() == name {
        return Ok(());
    }
    let mounted = data.set_mod(name);
    if let Some(mut p) = world.get_resource_mut::<crate::monsters::ProfileCache>() {
        p.0.clear();
    }
    if let Some(mut b) = world.get_resource_mut::<crate::creature::BodyCache>() {
        *b = Default::default();
    }
    if let Some(mut a) = world.get_resource_mut::<crate::character::AnimCache>() {
        *a = Default::default();
    }
    mounted
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
    /// Crucible (`sub_1400C0F30` sets it), the user mod of a custom game, empty for the campaign.
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

/// The character's `last_difficulty` for a session in `mode` (`Player::SaveNewFormatData`
/// @0x1803dba00): `difficulty | ((game mode << 2) | challenge) << 4`; Veteran is challenge 1 on Normal,
/// Ultimate Veteran challenge 3 on Ultimate. Set on a loaded character when the session begins and written by
/// every save, a new character's too.
pub fn last_difficulty(mode: gd_sim::mode::Mode, choice: &Choice) -> u8 {
    (choice.game_mode() << 6) | ((mode.challenge as u8) << 4) | mode.difficulty.min(2) as u8
}

/// Starts the session: `difficulty` (0..=2) becomes the character's last
/// played difficulty (`CharacterInfo::difficulty`).
pub fn begin(
    commands: &mut Commands,
    mut choice: Choice,
    gdc: Option<Gdc>,
    difficulty: usize,
    veteran: bool,
) {
    // No challenge in the Crucible (the game type window's Accept sub_140091210
    // and the difficulty window's click sub_14008F220 set `GameInfo::SetChallenge(0)` in mode 1), whatever the
    // character last played (a `GD_SAVE` of a Veteran character).
    let veteran = veteran && choice.survival.is_none();
    let mode = gd_sim::mode::Mode::new(difficulty, veteran);
    let (veteran, ultimate_veteran) = (mode.veteran(), mode.ultimate_veteran());
    commands.insert_resource(GameMode(mode));
    // The session's mod is mounted before anything of the session reads it (a Crucible or custom game
    // started from the menu mounted it already, `grimdawn::world::open_session_map`).
    let mod_name = (!choice.mod_name().is_empty()).then(|| choice.mod_name().to_owned());
    commands.queue(move |world: &mut World| {
        if let Err(e) = set_mod_data(world, mod_name.as_deref()) {
            error!("session: mod {mod_name:?} does not mount: {e:#}");
        }
    });
    if let Some(mut g) = gdc {
        g.info.last_difficulty = last_difficulty(mode, &choice);
        choice.hardcore = g.header.hardcore != 0;
        info!(
            "session: {:?} level {} ({}), difficulty {}{}",
            g.header.name,
            g.bio.level,
            g.header.class_tag,
            g.info.difficulty(),
            if veteran {
                " (Veteran)"
            } else if ultimate_veteran {
                " (Ultimate Veteran)"
            } else {
                ""
            }
        );
        commands.insert_resource(LoadedCharacter(Arc::new(g)));
        commands.insert_resource(crate::save::AutoSave::default());
    } else {
        info!(
            "session: new character {:?}{}",
            choice.name,
            if choice.female { " (female)" } else { "" }
        );
        commands.insert_resource(crate::save::NewCharacter::roll(&choice));
        // The engine writes a new character at creation (`save::AutoSave::created`).
        commands.insert_resource(crate::save::AutoSave::created());
    }
    commands.insert_resource(choice);
    commands.insert_resource(SessionReady);
}

/// Ends the session's gameplay state (gd-game's part of Exit to Main Menu; `grimdawn::exit` adds the
/// entities, the render / UI side and the reopening of the world): session-created resources are removed
/// (the `init_*` systems create them again), startup resources that hold session state go back to `Default`.
/// That includes the monster profiles: a profile is built from the session's `CombatRules` (difficulty
/// balance, Veteran), so a (record, level) entry of one session is wrong in the next.
pub fn end_session_state(world: &mut World) {
    if let Err(e) = set_mod_data(world, None) {
        error!("session: {e:#}");
    }
    world.insert_resource(crate::monsters::ProfileCache::default());
    world.remove_resource::<crate::session::SessionReady>();
    world.remove_resource::<crate::session::LoadedCharacter>();
    world.remove_resource::<crate::save::NewCharacter>();
    world.remove_resource::<crate::save::AutoSave>();
    world.remove_resource::<crate::session::PlayerStart>();
    world.remove_resource::<crate::session::DifficultyUnlock>();
    world.remove_resource::<crate::combo::ComboRules>();
    world.remove_resource::<crate::devotion::DevotionState>();
    world.remove_resource::<crate::factions::PlayerFactions>();
    world.remove_resource::<crate::fog::FogOfWar>();
    world.remove_resource::<crate::illusion::Illusions>();
    world.remove_resource::<crate::inventory::Inventory>();
    world.remove_resource::<crate::loot_filter::LootFilter>();
    world.remove_resource::<crate::lore::LoreCodex>();
    world.remove_resource::<crate::progress::PlayerProgress>();
    world.remove_resource::<crate::reclamation::SkillReclamation>();
    world.remove_resource::<crate::riftgates::TravelState>();
    world.remove_resource::<crate::riftgates::WaypointIndex>();
    world.remove_resource::<crate::riftgates::RiftgateGroup>();
    world.remove_resource::<crate::rules::CombatRules>();
    world.remove_resource::<crate::rules::RulesFailed>();
    world.remove_resource::<crate::stash::Stash>();
    world.remove_resource::<crate::story::StorySetUp>();
    world.remove_resource::<crate::tutorials::TutorialPages>();
    world.remove_resource::<crate::entry::EnteredGame>();
    world.insert_non_send(crate::story_world::StoryWorld::default());
    world.insert_resource(crate::story_world::SpawnedActors::default());
    world.insert_resource(crate::day_clock::DayClock::default());
    world.insert_resource(crate::session::Choice::default());
    world.insert_resource(crate::session::GameMode::default());
    world.insert_resource(crate::nav::Nav::default());
    world.insert_resource(crate::collision::WorldCollision::default());
    world.insert_resource(crate::world::ChestStates::default());
    world.insert_resource(crate::save::BoxedItems::default());
    world.insert_resource(crate::patrol::PatrolGroups::default());
    world.insert_resource(crate::monsters::ProxyStates::default());
    world.insert_resource(crate::endless::ShrineMemory::default());
    world.insert_resource(crate::monsters::UnifiedPools::default());
    world.insert_resource(crate::monsters::SetPieceStates::default());
    world.insert_resource(crate::play_stats::PlayCounts::default());
    world.insert_resource(crate::riftgates::PersonalPortal::default());
    world.insert_resource(crate::unique::UniqueSelection::default());
    world.insert_resource(crate::loot::DroppedEpics::default());
    world.insert_resource(crate::loot::LevelItems::default());
    world.insert_resource(crate::ascendant::AltarState::default());
    world.insert_resource(crate::hotbar::HeldHotSlots::default());
    world.insert_resource(crate::hotbar::ArmedSlot::default());
    world.insert_resource(crate::hotbar::WorldCursor::default());
    world.insert_resource(crate::hotbar::NewMouseCommand::default());
    world.insert_resource(crate::sheet_dps::SheetDpsValues::default());
    world.insert_resource(crate::riftgates::RiftgateMap::default());
    world.insert_resource(crate::devotion::DevotionUndo::default());
    world.insert_resource(crate::devotion::ShrineWindow::default());
    world.insert_resource(crate::points::SkillUndo::default());
    world.insert_resource(crate::ascendant::DreadCount::default());
    world.insert_resource(crate::nemesis::Nemeses::default());
    // `GameEngine::ClearAllChallengeAreas` (`ExitPlayingMode`).
    world.insert_resource(crate::challenge_areas::ChallengeAreas::default());
    // `TickManager::Reset`.
    world.insert_resource(crate::damage_sectors::TickManager::default());
    world.insert_resource(crate::synergy::TeamPool::default());
    world.insert_resource(crate::graeae::FirstFire::default());
    world.insert_resource(crate::conversation::Conversation::default());
    world.insert_resource(crate::cauldron::PotionWindow::default());
    world.insert_resource(crate::world::Broken::default());
    world.insert_resource(crate::trade::MarketTimers::default());
    world.insert_resource(crate::trade::Markets::default());
    world.insert_resource(crate::trade::ServiceVisit::default());
    world.insert_resource(crate::pause::GamePause::default());
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

pub fn load_save(
    mut commands: Commands,
    hooks: Res<crate::hooks::Hooks>,
    epoch: Res<SessionEpoch>,
) {
    let hooks = &hooks.session;
    if !hooks.skip_menu {
        return;
    }
    let new = Choice {
        save: None,
        name: hooks.name.clone().unwrap_or_else(|| "Player".into()),
        female: hooks.female,
        hardcore: hooks.hardcore,
        // `GD_SURVIVAL` starts the first session of the process in the Crucible (a restart after an exit
        // is the campaign: `GD_EXIT_CYCLES`).
        survival: hooks
            .survival
            .as_deref()
            .filter(|_| epoch.0 == 0)
            .map(arena_map),
        custom: None,
    };
    let Some(path) = hooks.save.clone() else {
        begin(&mut commands, new, None, 0, false);
        return;
    };
    match std::fs::read(&path)
        .map_err(anyhow::Error::from)
        .and_then(|b| Gdc::parse(&b))
    {
        Ok(gdc) => {
            info!("loaded character {:?} from {path}", gdc.header.name);
            let (d, vet) = mode_for(hooks, &gdc);
            begin(
                &mut commands,
                Choice {
                    save: Some(path.into()),
                    ..new
                },
                Some(gdc),
                d,
                vet,
            );
        }
        Err(e) => {
            error!("GD_SAVE {path}: {e:#}");
            begin(&mut commands, new, None, 0, false);
        }
    }
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

/// The port's save folder (`main/_<name>/player.gdc`, `transfer.gst`, ...):
/// a resource, so every save system reads the same value and a test gives
/// each game its own folder. The default (`GD_SAVE_DIR`, else
/// `$XDG_DATA_HOME/grimdawn-rs/save`, else `~/.local/share/grimdawn-rs/save`)
/// is the only place the environment is read.
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

        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
            });
        SaveDir(base.join("grimdawn-rs/save"))
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
/// (0x1400892e0): an entry is drawn as dead when the player is hardcore and
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

    /// (difficulty 0..=2, veteran / challenge flag) for [`begin`].
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
    /// `Grim Dawn.exe` difficulty window enables (sub_14008F360, campaign
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
    /// (`sub_14008F360`): Normal always, Elite once the character's greatest survival
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

    /// The main menu's difficulty button text (sub_1400BE8D0).
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
