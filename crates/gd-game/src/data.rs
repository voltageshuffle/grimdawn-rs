//! Game data access: the layered archive VFS and merged database, with the session's mod mounted
//! over them ([`GameData::set_mod`]: the Crucible or a custom game's user mod), and the rules derived
//! from the records once per mounted database ([`GameData::skill_trees`], [`GameData::engine`]; the
//! engine's managers hold these, here they are lazy caches `set_mod` drops).
use bevy::prelude::*;
use gd_formats::text::TagTable;
use gd_formats::vfs::{Database, Vfs};
use gd_sim::items::ItemDb;
use gd_sim::skills::SkillTrees;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Resource, Clone)]
pub struct GameData {
    pub vfs: Arc<Vfs>,
    pub db: Arc<Database>,
    /// The English localisation tags by mounted mod ("" = none), each read on first use ([`GameData::tags`]).
    tags: Arc<Mutex<HashMap<String, Arc<TagTable>>>>,
    /// The factions packs built so far, one shared `Arc` per `factions` record
    /// ([`crate::monsters::FactionPack::shared`]); dropped when the mod changes.
    packs: Packs,
    /// [`Self::skill_trees`]; dropped when the mod changes.
    trees: Lazy<SkillTrees>,
    /// [`Self::engine`]; dropped when the mod changes.
    engine: Lazy<crate::rules::GameEngineRules>,
}

type Packs = Arc<Mutex<HashMap<String, Option<Arc<crate::monsters::FactionPack>>>>>;
type Lazy<T> = Arc<Mutex<Option<Arc<T>>>>;

/// The value of `cell`, built by `load` on first use.
fn lazy<T>(cell: &Lazy<T>, load: impl FnOnce() -> T) -> Arc<T> {
    let mut v = cell.lock().unwrap_or_else(|p| p.into_inner());
    v.get_or_insert_with(|| Arc::new(load())).clone()
}

impl GameData {
    /// The cache of [`crate::monsters::FactionPack::shared`].
    pub(crate) fn packs(&self) -> &Packs {
        &self.packs
    }

    /// The class and potion trees of the mounted data ([`SkillTrees`]), built once per mod.
    pub fn skill_trees(&self) -> Arc<SkillTrees> {
        lazy(&self.trees, || SkillTrees::load(&self.db))
    }

    /// `records/game/gameengine.dbr` of the mounted data, parsed once per mod.
    pub fn engine(&self) -> Arc<crate::rules::GameEngineRules> {
        lazy(&self.engine, || {
            crate::rules::GameEngineRules::load(&self.db)
        })
    }

    /// Record access for the item code ([`ItemDb`]) over the mounted database: an `Arc` clone
    /// of [`Self::db`], so it follows the mounted mod; nothing is cached.
    pub fn item_db(&self) -> ItemDb {
        ItemDb::new(self.db.clone())
    }

    /// Mounts the archives and database of the game installed at `root`.
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            vfs: Arc::new(Vfs::open(root)?),
            db: Arc::new(Database::open(root)?),
            tags: Default::default(),
            packs: Default::default(),
            trees: Default::default(),
            engine: Default::default(),
        })
    }

    /// Mounts the mod `name` over the install for the whole process (`Engine::InitializeMod`: its
    /// records, scripts and maps win), or with `None` goes back to the install
    /// (`Engine::RemoveModDirectory`). A session sets it before anything reads the data. A mod
    /// that fails to mount leaves the install.
    pub fn set_mod(&self, name: Option<&str>) -> anyhow::Result<()> {
        self.packs.lock().unwrap().clear();
        self.trees.lock().unwrap().take();
        self.engine.lock().unwrap().take();
        let Some(name) = name else {
            self.vfs.remove_mod();
            self.db.remove_mod();
            return Ok(());
        };
        let mounted = self
            .vfs
            .initialize_mod(name)
            .and_then(|()| self.db.initialize_mod(&self.vfs.root, name));
        if mounted.is_err() {
            self.vfs.remove_mod();
            self.db.remove_mod();
        }
        mounted
    }

    /// The mounted mod's name (`None`: the install alone).
    pub fn active_mod(&self) -> Option<&str> {
        self.vfs.active_mod()
    }

    /// The install the data is mounted from (the binary's resolved game folder).
    pub fn root(&self) -> &Path {
        &self.vfs.root
    }

    /// The localisation tags (`text_en/*`) of the mounted data, loaded once per mod and shared by the
    /// story, the UI and the rules that sort by name.
    pub fn tags(&self) -> Arc<TagTable> {
        let key = self.active_mod().unwrap_or("");
        let mut tables = self.tags.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(t) = tables.get(key) {
            return t.clone();
        }
        let mut tags = TagTable::default();
        let mut paths: Vec<&str> = self
            .vfs
            .paths()
            .filter(|p| p.starts_with("text_en/") && TagTable::is_tag_file(p))
            .collect();
        TagTable::sort_files(&mut paths);
        for p in paths {
            if let Ok(b) = self.vfs.read(p) {
                tags.add_file(&b);
            }
        }
        let tags = Arc::new(tags);
        tables.insert(key.to_owned(), tags.clone());
        tags
    }
}

/// The file that marks a Grim Dawn install: the base game's database (the first one
/// [`Database::open`] reads).
pub const INSTALL_MARKER: &str = "database/database.arz";

/// The first of `candidates` that holds a Grim Dawn install ([`INSTALL_MARKER`]), else every
/// candidate tried (in order, for the error message).
pub fn find_install(
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Result<PathBuf, Vec<PathBuf>> {
    let mut tried = Vec::new();
    for dir in candidates {
        if dir.join(INSTALL_MARKER).is_file() {
            return Ok(dir);
        }
        tried.push(dir);
    }
    Err(tried)
}

/// Mounts the install at `root` as the [`GameData`] resource. The binary resolves `root` once
/// (`grimdawn`'s `main`: `--game-dir`, then the executable's and the working folder, see
/// [`find_install`]); everything after reads it back as [`GameData::root`].
pub struct DataPlugin {
    pub root: PathBuf,
}

impl Plugin for DataPlugin {
    fn build(&self, app: &mut App) {
        let root = &self.root;
        let t = std::time::Instant::now();
        let data = GameData::open(root).expect("open game data");
        // `GD_SURVIVAL`: a Crucible process (hooks are inserted before the plugins).
        if app
            .world()
            .get_resource::<crate::hooks::Hooks>()
            .is_some_and(|h| h.session.survival.is_some())
            && let Err(e) = data.set_mod(Some(gd_formats::vfs::SURVIVAL_MOD))
        {
            error!("GD_SURVIVAL: the Crucible does not mount: {e:#}");
        }
        info!(
            "mounted {} files, {} records from {} in {:?}",
            data.vfs.paths().count(),
            data.db.len(),
            root.display(),
            t.elapsed()
        );
        app.insert_resource(data);
    }
}
