//! Game data access: the layered archive VFS and merged database, with the session's mod mounted
//! over them ([`GameData::set_mod`]), and the rules derived from the records as lazy caches
//! `set_mod` drops ([`GameData::skill_trees`], [`GameData::engine`]).
//!
//! The localisation language ([`GameData::set_language`]) selects the tags' `text_<code>/` archive
//! ([`GameData::tags`]); the UI follows it live, the story snapshots its text once per session.
//! PORT: CJK font word-wrap (`language.def` `wordmode`) is out of scope.
use bevy::prelude::*;
use gd_formats::collections::{HashMap, HashSet};
use gd_formats::text::{LANGUAGES, Language, TagTable};
use gd_formats::vfs::{Database, Vfs};
use gd_sim::items::ItemDb;
use gd_sim::skills::SkillTrees;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Resource, Clone)]
pub struct GameData {
    pub vfs: Arc<Vfs>,
    pub db: Arc<Database>,
    /// By mounted mod ("" = none) and language.
    tags: Arc<Mutex<HashMap<String, HashMap<String, Arc<TagTable>>>>>,
    /// Archive code ([`LANGUAGES`]; "en" default).
    language: String,
    /// [`crate::monsters::FactionPack::shared`]'s cache.
    packs: Packs,
    trees: Lazy<SkillTrees>,
    engine: Lazy<crate::rules::GameEngineRules>,
}

type Packs = Arc<Mutex<HashMap<String, Option<Arc<crate::monsters::FactionPack>>>>>;
type Lazy<T> = Arc<Mutex<Option<Arc<T>>>>;

/// The value of `cell`, built by `load` on first use.
fn lazy<T>(cell: &Lazy<T>, load: impl FnOnce() -> T) -> Arc<T> {
    let mut v = cell.lock();
    v.get_or_insert_with(|| Arc::new(load())).clone()
}

impl GameData {
    pub(crate) fn packs(&self) -> &Packs {
        &self.packs
    }

    /// The class and potion trees of the mounted data.
    pub fn skill_trees(&self) -> Arc<SkillTrees> {
        lazy(&self.trees, || SkillTrees::load(&self.db))
    }

    /// `records/game/gameengine.dbr` of the mounted data.
    pub fn engine(&self) -> Arc<crate::rules::GameEngineRules> {
        lazy(&self.engine, || {
            crate::rules::GameEngineRules::load(&self.db)
        })
    }

    /// An `Arc` clone of [`Self::db`]; nothing is cached.
    pub fn item_db(&self) -> ItemDb {
        ItemDb::new(self.db.clone())
    }

    /// Mounts the archives and database of the game installed at `root`.
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            vfs: Arc::new(Vfs::open(root)?),
            db: Arc::new(Database::open(root)?),
            tags: Default::default(),
            language: "en".to_owned(),
            packs: Default::default(),
            trees: Default::default(),
            engine: Default::default(),
        })
    }

    /// Mounts the mod `name` over the install (`Engine::InitializeMod`), or with `None` goes back to
    /// the install (`Engine::RemoveModDirectory`). A mod that fails to mount leaves the install. Only
    /// `<name>.arz` of the mod's database folder is loaded, as in the engine.
    pub fn set_mod(&self, name: Option<&str>) -> anyhow::Result<()> {
        let mounted = match name {
            None => {
                self.vfs.remove_mod();
                self.db.remove_mod();
                Ok(())
            }
            Some(name) => self
                .vfs
                .initialize_mod(name)
                .and_then(|()| self.db.initialize_mod(&self.vfs.root, name))
                .map(|skipped| {
                    for p in skipped {
                        bevy::log::warn!(
                            "mod {name}: {} is not loaded (only {name}.arz is)",
                            p.display()
                        );
                    }
                })
                .inspect_err(|_| {
                    self.vfs.remove_mod();
                    self.db.remove_mod();
                }),
        };
        // Drop after the swap: a reader that refilled a cache during the mount built from old data.
        self.packs.lock().clear();
        self.trees.lock().take();
        self.engine.lock().take();
        mounted
    }

    /// `None`: the install alone.
    pub fn active_mod(&self) -> Option<&str> {
        self.vfs.active_mod()
    }

    /// The install the data is mounted from (the binary's resolved game folder).
    pub fn root(&self) -> &Path {
        &self.vfs.root
    }

    /// Archive code ([`LANGUAGES`]; "en" default).
    pub fn language(&self) -> String {
        self.language.clone()
    }

    /// Sets the localisation language from its archive code; an unknown or unmounted code falls back
    /// to "en" (`LocalizationManager::Load`).
    pub fn set_language(&mut self, code: &str) {
        let code = LANGUAGES
            .iter()
            .find(|l| l.code == code)
            .map_or("en", |l| l.code);
        let code = if self.text_mounted(code) { code } else { "en" };
        if self.language != code {
            self.language = code.to_owned();
        }
    }

    fn text_mounted(&self, code: &str) -> bool {
        let prefix = format!("text_{code}/");
        self.vfs
            .paths()
            .any(|p| p.starts_with(&prefix) && TagTable::is_tag_file(p))
    }

    /// The [`LANGUAGES`] whose `text_<code>/` tag files are mounted (`OptionsWindow::Load` via
    /// `IsLanguageSupported`).
    pub fn available_languages(&self) -> Vec<&'static Language> {
        let mut mounted: HashSet<&str> = HashSet::default();
        for p in self.vfs.paths() {
            if let Some(rest) = p.strip_prefix("text_")
                && let Some((code, _)) = rest.split_once('/')
                && TagTable::is_tag_file(p)
            {
                mounted.insert(code);
            }
        }
        LANGUAGES
            .iter()
            .filter(|l| mounted.contains(l.code))
            .collect()
    }

    /// The localisation tags (`text_<language>/`) of the mounted data, loaded once per mod and
    /// language. An unmounted language reads "en" ([`Self::set_language`]).
    pub fn tags(&self) -> Arc<TagTable> {
        let stamp = self.vfs.mod_stamp();
        let mod_key = self.active_mod().unwrap_or("");
        let mut tables = self.tags.lock();
        if let Some(t) = tables
            .get(mod_key)
            .and_then(|m| m.get(self.language.as_str()))
        {
            return t.clone();
        }
        let prefix = format!("text_{}/", self.language);
        let mut paths: Vec<&str> = self
            .vfs
            .paths()
            .filter(|p| p.starts_with(&prefix) && TagTable::is_tag_file(p))
            .collect();
        if paths.is_empty() && self.language != "en" {
            paths = self
                .vfs
                .paths()
                .filter(|p| p.starts_with("text_en/") && TagTable::is_tag_file(p))
                .collect();
        }
        TagTable::sort_files(&mut paths);
        let mut tags = TagTable::default();
        for p in paths {
            if let Ok(b) = self.vfs.read(p) {
                tags.add_file(&b);
            }
        }
        let tags = Arc::new(tags);
        // The mount may have swapped mid-build: cache only when the stamp is unchanged.
        if self.vfs.mod_stamp() == stamp {
            tables
                .entry(mod_key.to_owned())
                .or_default()
                .insert(self.language.clone(), tags.clone());
        }
        tags
    }
}

/// The render caches' synchronous drop on a mod swap: gd-render registers its system here and
/// `gd_game::session::set_mod_data` runs it in the same call (gd-game cannot depend on gd-render).
#[derive(Resource)]
pub struct ModCacheDrop(pub bevy::ecs::system::SystemId);

/// The base game's database (the first one [`Database::open`] reads).
pub const INSTALL_MARKER: &str = "database/database.arz";

/// The first of `candidates` that holds a Grim Dawn install ([`INSTALL_MARKER`]), else every
/// candidate tried, in order.
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
