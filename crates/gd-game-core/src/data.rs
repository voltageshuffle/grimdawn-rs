//! Game data access: the layered archive VFS and merged database, with the session's mod mounted
//! over them ([`GameData::set_mod`]: the Crucible or a custom game's user mod), and the rules derived
//! from the records once per mounted database ([`GameData::skill_trees`], [`GameData::engine`]; the
//! engine's managers hold these, here they are lazy caches `set_mod` drops).
//!
//! The localisation language ([`GameData::set_language`], the `language` option) selects the tags'
//! `text_<code>/` archive ([`GameData::tags`]); the UI follows it live, while the story snapshots its
//! tags, the quest / dialog text overrides (`text_<code>/<basename>.txt`) and the `language.def`
//! paging separators once per session (`gd_game::story::setup_story`), so they follow on the next
//! session. PORT: `intro.txt` cinematic subtitles (the port plays no videos) and CJK font word-wrap
//! (`language.def` `wordmode`) are out of scope.
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
    /// The localisation tags by mounted mod ("" = none) and language, each read on first use
    /// ([`GameData::tags`]).
    tags: Arc<Mutex<HashMap<String, HashMap<String, Arc<TagTable>>>>>,
    /// The localisation language's archive code ([`LANGUAGES`]; "en" default), from the `language` option.
    language: String,
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
    let mut v = cell.lock();
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
            language: "en".to_owned(),
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
        self.packs.lock().clear();
        self.trees.lock().take();
        self.engine.lock().take();
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

    /// The localisation language's archive code ([`LANGUAGES`]; "en" default).
    pub fn language(&self) -> String {
        self.language.clone()
    }

    /// Sets the localisation language from its archive code; an unknown code falls back to "en"
    /// (the engine's `LocalizationManager::Load` keeps the install's default for an unsupported
    /// language). The tag tables are keyed by language ([`Self::tags`]), so nothing is dropped.
    pub fn set_language(&mut self, code: &str) {
        let code = LANGUAGES
            .iter()
            .find(|l| l.code == code)
            .map_or("en", |l| l.code);
        if self.language != code {
            self.language = code.to_owned();
        }
    }

    /// The languages this install offers: the [`LANGUAGES`] entries whose `text_<code>/` tag files
    /// are mounted in the VFS (the `Text_*.arc` present), as the engine's dropdown lists them
    /// (Grim Dawn.exe `OptionsWindow::Load` via `GetNumLanguages` / `IsLanguageSupported`).
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
    /// language and shared by the story, the UI and the rules that sort by name.
    pub fn tags(&self) -> Arc<TagTable> {
        let mod_key = self.active_mod().unwrap_or("");
        let mut tables = self.tags.lock();
        if let Some(t) = tables
            .get(mod_key)
            .and_then(|m| m.get(self.language.as_str()))
        {
            return t.clone();
        }
        let prefix = format!("text_{}/", self.language);
        let mut tags = TagTable::default();
        let mut paths: Vec<&str> = self
            .vfs
            .paths()
            .filter(|p| p.starts_with(&prefix) && TagTable::is_tag_file(p))
            .collect();
        TagTable::sort_files(&mut paths);
        for p in paths {
            if let Ok(b) = self.vfs.read(p) {
                tags.add_file(&b);
            }
        }
        let tags = Arc::new(tags);
        tables
            .entry(mod_key.to_owned())
            .or_default()
            .insert(self.language.clone(), tags.clone());
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
