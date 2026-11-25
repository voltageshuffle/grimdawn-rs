//! Headless test harness: an `App` with the gameplay systems and no window,
//! renderer or input, stepped frame by frame with a fixed time step so runs
//! are deterministic.
//!
//! ```ignore
//! let Some(mut game) = TestGame::new_character("Tester", false, false) else { return }; // GD_SKIP_DATA_TESTS=1
//! let p = game.spawn_player();
//! game.step();
//! let skills = game.app.world().get::<EffectiveSkills>(p).unwrap();
//! ```
//!
//! Session state (mode, altar, dropped epics, the character choice) is in
//! each app's resources ([`crate::session::begin`]), so tests in one binary
//! do not affect each other; the read-only [`GameData`] is opened once per
//! binary ([`data`]). Each game saves into its own temp
//! [`SaveDir`] ([`TestGame::save_dir`]); no test sets environment
//! variables.
//!
//! The only outside input is the game install: a test's character is a new
//! one ([`TestGame::new_character`]) or one the port saved into a test's own
//! save folder ([`TestGame::save_now`], loaded with [`TestGame::with_save`]).
//! Without the game the constructors panic, or return None under
//! `GD_SKIP_DATA_TESTS=1` ([`gd_formats::testdata`]). Temp folders: [`TempDir`].
use crate::session;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use gd_formats::msh::Mesh as Msh;
use gd_formats::save::gdc::Gdc;
use gd_game_core::character::{Character, character_for};
use gd_game_core::data::GameData;
use gd_game_core::player::Player;
use gd_game_core::progress::PlayerProgress;
use gd_game_core::rules::CombatRules;
use gd_game_core::session::{Choice, LoadedCharacter, SaveDir, mode_for};
use gd_sim::equip::WeaponType;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use gd_formats::testdata::TempDir;

/// The fixed frame time of [`TestGame::step`].
pub const FRAME: Duration = Duration::from_millis(16);

/// The random seed of a [`TestGame`] unless one is given.
pub const SEED: u64 = 1;

/// An app running [`crate::GameLogicPlugin`] over `data`, without a window,
/// renderer or input; every `update()` advances time by [`FRAME`].
///
/// Deterministic: the schedules run single-threaded on the calling thread, in
/// a fixed order, so the gameplay's random draws ([`gd_game_core::rng`], a
/// thread-local generator nothing else draws from) all come from this
/// thread's generator, seeded with `seed` (each test runs on its own thread).
pub fn headless_app(data: GameData, seed: u64) -> App {
    use bevy::ecs::schedule::{InternedScheduleLabel, ScheduleLabel, SingleThreadedExecutor};
    gd_game_core::rng::seed(seed);
    let mut app = App::new();
    // Dev builds: the hook systems read the resource; a test never gets the environment's hooks.
    #[cfg(feature = "dev")]
    app.insert_resource(gd_game_core::hooks::Hooks::from_vars(std::iter::empty()));
    // Each game saves into its own temp folder (removed with the app), never
    // the player's save folder and never one shared by parallel tests.
    let save = TempDir::new("gd-game-save");
    app.insert_resource(SaveDir(save.path().to_owned()))
        .insert_resource(SaveDirGuard(save));
    app.add_plugins((
        MinimalPlugins,
        bevy::transform::TransformPlugin,
        crate::GameLogicPlugin,
    ))
    .insert_resource(data)
    .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    let labels: [InternedScheduleLabel; 9] = [
        First.intern(),
        PreUpdate.intern(),
        Update.intern(),
        PostUpdate.intern(),
        Last.intern(),
        FixedUpdate.intern(),
        Startup.intern(),
        PreStartup.intern(),
        PostStartup.intern(),
    ];
    for label in labels {
        app.edit_schedule(label, |s| {
            s.set_executor(SingleThreadedExecutor::new());
        });
    }
    app
}

/// The installed game's archives and database, opened once per test binary
/// (read-only, shared by every [`TestGame`] of the binary). Panics when the
/// game is missing unless `GD_SKIP_DATA_TESTS=1` (then None).
pub fn data() -> Option<GameData> {
    static DATA: std::sync::OnceLock<GameData> = std::sync::OnceLock::new();
    let root = gd_formats::testdata::game()?;
    Some(
        DATA.get_or_init(|| GameData::open(&root).expect("open the game data"))
            .clone(),
    )
}

/// The first inventor blueprint (by record name) with reagents, each reagent
/// naming at least one item. The database's name index is a `HashMap`
/// (a different order every process), so a test must not take "the first" of
/// `db.names()`: that picked a different blueprint each run.
pub fn first_blueprint(db: &gd_formats::vfs::Database) -> gd_sim::items::crafting::Blueprint {
    use gd_sim::items::crafting::Blueprint;
    let mut names: Vec<&str> = db
        .names()
        .filter(|n| n.contains("/crafting/blueprints/"))
        .collect();
    names.sort_unstable();
    names
        .into_iter()
        .filter_map(|n| {
            db.get(n)
                .filter(|r| gd_sim::items::crafting::formula_valid(db, r))
                .and_then(|r| Blueprint::load(n, r))
        })
        .find(|bp| {
            !bp.reagents.is_empty() && bp.reagents.iter().all(|(names, _)| !names.is_empty())
        })
        .expect("a blueprint")
}

/// Keeps a [`TestGame`]'s save folder alive (removed when the app drops).
#[derive(Resource)]
struct SaveDirGuard(#[allow(dead_code)] TempDir);

/// A headless game session.
pub struct TestGame {
    pub app: App,
}

impl TestGame {
    /// A session with the character saved at `path` (written by the port into
    /// a test's save folder, [`Self::save_now`]; its difficulty and mode).
    /// Without the game it panics, or gives None under `GD_SKIP_DATA_TESTS=1`.
    pub fn with_save(path: &Path) -> Option<Self> {
        Self::start(Some(path), Start::default())
    }

    /// [`Self::with_save`] started on `difficulty` (0..=2; `veteran`: Veteran on Normal, Ultimate Veteran on
    /// Ultimate), as the main menu's difficulty choice starts it.
    pub fn with_save_mode(path: &Path, difficulty: usize, veteran: bool) -> Option<Self> {
        Self::start(
            Some(path),
            Start {
                mode: Some((difficulty, veteran)),
                ..Start::default()
            },
        )
    }

    /// A session with a new character (no save; the main menu's Create
    /// path); None only under `GD_SKIP_DATA_TESTS=1` without the game.
    pub fn new_character(name: &str, female: bool, hardcore: bool) -> Option<Self> {
        Self::start(
            None,
            Start {
                name: name.to_owned(),
                female,
                hardcore,
                ..Start::default()
            },
        )
    }

    /// [`Self::new_character`] ("Tester", male, softcore) with the random draws seeded by `seed`.
    pub fn new_character_seeded(seed: u64) -> Option<Self> {
        Self::start(
            None,
            Start {
                seed,
                ..Start::default()
            },
        )
    }

    /// A Crucible session (`Choice::survival`: the arena map, e.g. `maps/survivalworld_a.map`) with a new
    /// character. It turns the Crucible data overlay on for the shared [`data`]: only for a test binary whose
    /// tests all run in the Crucible (`tests/crucible`).
    pub fn crucible(arena: &str) -> Option<Self> {
        Self::start(
            None,
            Start {
                survival: Some(arena.to_owned()),
                ..Start::default()
            },
        )
    }

    /// [`Self::crucible`] on game `difficulty` (0..=2: Aspirant, Challenger, Gladiator).
    pub fn crucible_mode(arena: &str, difficulty: usize) -> Option<Self> {
        Self::start(
            None,
            Start {
                survival: Some(arena.to_owned()),
                mode: Some((difficulty, false)),
                ..Start::default()
            },
        )
    }

    fn start(save: Option<&Path>, start: Start) -> Option<Self> {
        let data = data()?;
        let gdc = save.map(|path| {
            Gdc::parse(&std::fs::read(path).expect("read the save"))
                .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
        });
        let Start {
            seed,
            survival,
            mode,
            name,
            female,
            hardcore,
        } = start;
        let saved_mode = gdc.as_ref().map_or((0, false), mode_for);
        let mut app = headless_app(data, seed);
        let (difficulty, veteran) = mode.unwrap_or(saved_mode);
        let choice = match &gdc {
            Some(gdc) => Choice {
                save: save.map(Path::to_owned),
                name: gdc.header.name.clone(),
                female: gdc.header.sex == 0,
                hardcore,
                survival,
                custom: None,
            },
            None => Choice {
                save: None,
                name,
                female,
                hardcore,
                survival,
                custom: None,
            },
        };
        session::begin(
            &mut app.world_mut().commands(),
            choice,
            gdc,
            difficulty,
            veteran,
        );
        app.world_mut().flush();
        let mut game = TestGame { app };
        // The rules load once the session is ready.
        game.step();
        Some(game)
    }

    /// This game's own save folder ([`SaveDir`]): where a
    /// `SaveRequest` writes, empty at the start.
    pub fn save_dir(&self) -> PathBuf {
        self.app.world().resource::<SaveDir>().0.clone()
    }

    /// Saves the character now (a `SaveRequest`, one frame; the player must be spawned, [`Self::spawn_player`])
    /// and returns its `player.gdc` in [`Self::save_dir`] (valid while this game lives): the save for
    /// [`Self::with_save`].
    pub fn save_now(&mut self) -> PathBuf {
        self.app.world_mut().write_message(crate::save::SaveRequest);
        self.step();
        let name = self.app.world().resource::<Choice>().name.clone();
        self.save_dir()
            .join("main")
            .join(format!("_{name}"))
            .join("player.gdc")
    }

    /// Flat walkable ground around `center` (± `half` m).
    pub fn flat_ground(&mut self, center: Vec3, half: f32) {
        let layers = crate::nav::flat_layers(center, half);
        self.app
            .world_mut()
            .resource_mut::<crate::nav::Nav>()
            .0
            .add(u64::MAX, layers);
    }

    /// Runs one frame.
    pub fn step(&mut self) {
        self.app.update();
    }

    pub fn steps(&mut self, n: usize) {
        for _ in 0..n {
            self.step();
        }
    }

    /// Spawns the loaded character's player entity (gameplay components
    /// only, as the game builds them minus the account unlocks file and test
    /// hooks) and runs a frame so its derived state exists.
    pub fn spawn_player(&mut self) -> Entity {
        let world = self.app.world_mut();
        let data = world.resource::<GameData>().clone();
        let rules = world.resource::<CombatRules>().0.clone();
        let gdc = world.get_resource::<LoadedCharacter>().map(|c| c.0.clone());
        let female = world.resource::<Choice>().female;
        let spawn = crate::player::spawn_data(&data, &rules, gdc.as_deref(), female, |_| {})
            .expect("player profile");
        let character = player_character(&data, gdc.as_deref(), spawn.female);
        let e = world
            .spawn((
                spawn.bundle,
                character,
                crate::movement::MovePath::default(),
                Transform::default(),
            ))
            .id();
        self.step();
        e
    }

    /// Runs frames until `done` holds (at most `max`); returns the frames run.
    pub fn run_until(
        &mut self,
        max: usize,
        mut done: impl FnMut(&mut World) -> bool,
    ) -> Option<usize> {
        for n in 1..=max {
            self.step();
            if done(self.app.world_mut()) {
                return Some(n);
            }
        }
        None
    }

    /// Spawns a monster of `record` at `level` standing at `at` (as a level
    /// proxy spawns it: creature, profile, AI), and runs a frame.
    pub fn spawn_monster(&mut self, record: &str, level: i32, at: Vec3) -> Entity {
        let world = self.app.world_mut();
        let data = world.resource::<GameData>().clone();
        let rules = world.resource::<CombatRules>().clone();
        let factions = world
            .get_resource::<crate::factions::PlayerFactions>()
            .cloned();
        let m = crate::monsters::MonsterState {
            record: record.to_owned(),
            level,
            position: at,
            yaw: 0.0,
            variant: 1,
            alive: true,
            pending: false,
            scale: None,
        };
        let e = world.resource_scope(
            |world, mut bodies: Mut<gd_game_core::creature::BodyCache>| {
                world.resource_scope(|world, mut profiles: Mut<crate::monsters::ProfileCache>| {
                    let mut commands = world.commands();
                    let e = crate::monsters::spawn_state(
                        &mut commands,
                        &data,
                        &mut bodies,
                        &rules,
                        factions.as_ref(),
                        &mut profiles,
                        &m,
                        (usize::MAX, 0, 0),
                        &[],
                    );
                    world.flush();
                    e
                })
            },
        );
        let e = e.expect("monster spawns");
        self.step();
        e
    }

    /// The player entity.
    pub fn player(&mut self) -> Entity {
        self.app
            .world_mut()
            .query_filtered::<Entity, With<Player>>()
            .single(self.app.world())
            .expect("one player")
    }

    /// Clicks `target`: the `PlayerOrder::Interact` a click on it sends, routed by component on the next step
    /// (`crate::player_orders::apply_orders`), so tests exercise the same path as the game.
    pub fn interact(&mut self, target: Entity) {
        self.app
            .world_mut()
            .write_message(crate::player_orders::PlayerOrder::Interact(target));
    }

    pub fn get<C: Component>(&self, e: Entity) -> &C {
        self.app
            .world()
            .get::<C>(e)
            .unwrap_or_else(|| panic!("{e} has no {}", std::any::type_name::<C>()))
    }

    pub fn get_mut<C: Component<Mutability = bevy::ecs::component::Mutable>>(
        &mut self,
        e: Entity,
    ) -> Mut<'_, C> {
        self.app
            .world_mut()
            .get_mut::<C>(e)
            .unwrap_or_else(|| panic!("{e} has no {}", std::any::type_name::<C>()))
    }

    pub fn progress(&self) -> &PlayerProgress {
        self.app.world().resource::<PlayerProgress>()
    }

    pub fn data(&self) -> GameData {
        self.app.world().resource::<GameData>().clone()
    }

    pub fn rules(&self) -> std::sync::Arc<gd_sim::actor::Rules> {
        self.app.world().resource::<CombatRules>().0.clone()
    }
}

/// How [`TestGame::start`] begins the session.
struct Start {
    seed: u64,
    survival: Option<String>,
    mode: Option<(usize, bool)>,
    /// A new character's name, sex and hardcore flag (a save's header gives them).
    name: String,
    female: bool,
    hardcore: bool,
}

impl Default for Start {
    fn default() -> Self {
        Start {
            seed: SEED,
            survival: None,
            mode: None,
            name: "Tester".to_owned(),
            female: false,
            hardcore: false,
        }
    }
}

/// The player's animation state as the game builds it: the player record's
/// body mesh and the hand state of the save's active weapon set.
fn player_character(data: &GameData, gdc: Option<&Gdc>, female: bool) -> Character {
    let record = gd_game_core::player::player_record(female);
    let rec = data.db.get(record).expect("player record");
    let mesh = rec.str("mesh").expect("player mesh");
    let body = std::sync::Arc::new(
        Msh::parse(
            &data
                .vfs
                .read(&gd_formats::vfs::normalize(mesh))
                .expect("player mesh file"),
        )
        .expect("player mesh parses"),
    );
    let weapon = |k: usize| {
        let inv = gdc?.inventory.data.as_ref()?;
        let set = &inv.weapons[if inv.use_alternate != 0 { 1 } else { 0 }];
        let item = &set.slots.get(k)?.item;
        data.db
            .get(&item.base_name)
            .and_then(|r| WeaponType::from_class(&r.class))
    };
    let scale = gd_game_core::creature::record_scale(rec);
    character_for(
        data,
        record,
        rec,
        body,
        weapon(0),
        weapon(1),
        true,
        scale,
        1,
    )
}
