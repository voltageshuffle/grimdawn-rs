//! Layered view of the install: base → gdx1 → gdx2 → gdx3. Later layers
//! override earlier ones, both for archive entries and database records.
//!
//! A mod (`Engine::InitializeMod`) is mounted over the install while a session runs: the Crucible
//! (`survivalmode`, [`SURVIVAL_LAYERS`]: its records and scripts replace stock ones) or a user mod
//! of a custom game (`mods/<name>/`). [`Vfs::initialize_mod`] / [`Database::initialize_mod`] mount
//! one, [`Vfs::remove_mod`] / [`Database::remove_mod`] go back to the install
//! (`Engine::RemoveModDirectory`).
//! PORT: the engine drops the mod's sources when it is removed; here a mod's index is built on its
//! first mount and kept (inactive) for the process, so a record borrowed from it stays valid.
//!
//! Mount order (exe `Game::Initialize`, Engine.dll `Engine::InitializeMod` and
//! `LoadMainDatabase`, `FileSystem::AddSource` / `AddSourceArchive` /
//! `OpenFile`): `FileSystem` holds three partitions of sources; `OpenFile` tries partition 0, 1, 2
//! in turn and, within one, the sources in the order they were added (push_back): the first that
//! has the file wins. The install's resources are partition 2, added `gdx3`, `gdx2`, `gdx1`,
//! `resources` (each expansion when enabled), so gdx3 wins over gdx2 over gdx1 over the base game.
//! A mod is partition 1 (`InitializeMod(name)` clears it first), so any mod file wins over the
//! install. `AddSource` of a folder (as `InitializeMod` calls it) mounts each `*.arc` directly in
//! it (`FindFirstFile "<dir>/*.arc"` → `AddSourceArchive`): no loose files, no subfolders.
//! `InitializeMod(name)` adds `mods/<name>/resources`, and for the mod `survivalmode` before it
//! `survivalmode3/resources`, `survivalmode2/resources`, `survivalmode1/resources` (each only with
//! expansion 3 / 2 / 1 enabled, `Engine.expansion3Enabled` / `expansion2Enabled` / `expansion1Enabled`, and
//! `HasSurvivalDLC`): the
//! highest pack wins a same-named file. The databases load in order: `database.arz`, `gdx1..3.arz`
//! (`LoadMainDatabase`), then the mod's `mods/<name>/database/<name>.arz` (`LoadDatabase`), then
//! for `survivalmode` `LoadSurvival1/2/3Database` (`survivalmodeN/database/survivalmodeN.arz`, same
//! gates), a later archive's record replacing an earlier one's as for the expansions. So
//! [`mod_layers`] in load order with "later wins" is the engine's order. The mod name is compared
//! with `survivalmode` exactly (`memcmp`, case-sensitive); file names are matched ignoring case
//! (the install has `SurvivalMode.arz`).
//! PORT: the Crucible packs are mounted whatever expansions are installed (the port always has all three).
//!
//! Resource paths are `<archive name>/<entry name>`, lower-cased, with `/`
//! separators, e.g. `creatures/pc/hero_00.msh` → `Creatures.arc : pc/hero_00.msh`.
use crate::arc::Arc;
use crate::arz::{Arz, Record};
use crate::collections::HashMap;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

pub const LAYERS: [&str; 4] = ["", "gdx1", "gdx2", "gdx3"];

/// The Crucible's mod name (`GameInfo::SetModName` in mode 1).
pub const SURVIVAL_MOD: &str = "survivalmode";

/// The Crucible's layers in load order (the install keeps the first pack under `mods/`, the
/// others in the game root).
pub const SURVIVAL_LAYERS: [&str; 4] = [
    "mods/survivalmode",
    "survivalmode1",
    "survivalmode2",
    "survivalmode3",
];

/// The folders `InitializeMod(name)` mounts, relative to the game root, in load order (a later
/// one wins): [`SURVIVAL_LAYERS`] for the Crucible, else `mods/<name>`.
pub fn mod_layers(name: &str) -> Vec<String> {
    if name == SURVIVAL_MOD {
        SURVIVAL_LAYERS.iter().map(|l| (*l).to_owned()).collect()
    } else {
        vec![format!("mods/{name}")]
    }
}

/// `path` as the indexes key it: `/` separators, no leading `/`, ASCII lower case. Borrowed
/// when it already has that form.
pub fn normalized(path: &str) -> Cow<'_, str> {
    let is_key = |b: u8| b != b'\\' && !b.is_ascii_uppercase();
    if !path.starts_with('/') && path.bytes().all(is_key) {
        return Cow::Borrowed(path);
    }
    Cow::Owned(
        path.trim_start_matches(['/', '\\'])
            .chars()
            .map(|c| {
                if c == '\\' {
                    '/'
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect(),
    )
}

/// [`normalized`] as an owned key.
pub fn normalize(path: &str) -> String {
    normalized(path).into_owned()
}

/// The files of `dir` with extension `ext` (ignoring case), sorted by name.
fn files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)))
        .collect();
    paths.sort();
    paths
}

/// `dir/<name>.arz`, the name matched ignoring case (Windows file names).
fn named_arz(dir: &Path, name: &str) -> Option<PathBuf> {
    files_with_ext(dir, "arz").into_iter().find(|p| {
        p.file_stem()
            .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(name))
    })
}

/// `Engine::SurvivalFilesExist`: the Crucible's first pack (`mods/survivalmode/database/survivalmode.arz`,
/// any case) is installed. The engine's `HasSurvivalDLC` is this and the Steam DLC being owned; the port
/// has no Steam, so this alone stands for it.
pub fn survival_files_exist(root: &Path) -> bool {
    named_arz(
        &root.join(SURVIVAL_LAYERS[0]).join("database"),
        SURVIVAL_MOD,
    )
    .is_some()
}

/// The mods mounted so far, each indexed once and kept, and which one is active (partition 1).
/// Append-only (a list of `OnceLock` links), so a borrow of a mod's data lives as long as `self`.
struct Mods<T> {
    head: OnceLock<Box<Mounted<T>>>,
    /// 0 = none, `n` = the n-th mod of the list.
    active: AtomicUsize,
    /// Bumped on every mount / unmount (even back to the same mod), so a reader can tell a
    /// swap happened mid-build and discard a cache entry keyed by the old mod ([`Vfs::mod_stamp`]).
    stamp: AtomicU64,
    /// Serialises the appends.
    mounting: Mutex<()>,
}

struct Mounted<T> {
    name: String,
    layer: T,
    next: OnceLock<Box<Mounted<T>>>,
}

impl<T> Default for Mods<T> {
    fn default() -> Self {
        Self {
            head: OnceLock::new(),
            active: AtomicUsize::new(0),
            stamp: AtomicU64::new(0),
            mounting: Mutex::new(()),
        }
    }
}

impl<T> Mods<T> {
    fn iter(&self) -> impl Iterator<Item = &Mounted<T>> {
        std::iter::successors(self.head.get().map(|m| &**m), |m| {
            m.next.get().map(|n| &**n)
        })
    }

    fn active(&self) -> Option<&Mounted<T>> {
        let n = self.active.load(Ordering::Acquire);
        n.checked_sub(1).and_then(|i| self.iter().nth(i))
    }

    /// Makes `name` the active mod, indexing it with `load` on its first mount. The previous mod is
    /// removed first, so a failed load leaves the install alone.
    fn initialize(&self, name: &str, load: impl FnOnce() -> Result<T>) -> Result<()> {
        let _lock = self.mounting.lock();
        self.remove();
        let i = match self.iter().position(|m| m.name == name) {
            Some(i) => i,
            None => {
                let node = Box::new(Mounted {
                    name: name.to_owned(),
                    layer: load()?,
                    next: OnceLock::new(),
                });
                let n = self.iter().count();
                let tail = self.iter().last().map_or(&self.head, |m| &m.next);
                // Appends hold `mounting`, so the tail's link is empty.
                let _set = tail.set(node);
                n
            }
        };
        self.active.store(i + 1, Ordering::Release);
        self.stamp.fetch_add(1, Ordering::Release);
        Ok(())
    }

    fn remove(&self) {
        self.active.store(0, Ordering::Release);
        self.stamp.fetch_add(1, Ordering::Release);
    }
}

/// Mounted archives and their entries by resource path (a later archive wins).
#[derive(Default)]
struct Files {
    archives: Vec<Arc>,
    /// normalized path → (archive index, entry index)
    index: HashMap<String, (u32, u32)>,
}

impl Files {
    /// Every `*.arc` directly in `dir` (`FileSystem::AddSource`).
    fn mount_dir(&mut self, dir: &Path) -> Result<()> {
        for p in files_with_ext(dir, "arc") {
            self.mount(&p)?;
        }
        Ok(())
    }

    fn mount(&mut self, path: &Path) -> Result<()> {
        let arc = Arc::open(path)?;
        let prefix = path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase();
        let ai = self.archives.len() as u32;
        for (ei, e) in arc.entries.iter().enumerate() {
            let key = normalize(&format!("{prefix}/{}", e.name));
            self.index.insert(key, (ai, ei as u32));
        }
        self.archives.push(arc);
        Ok(())
    }

    fn get(&self, key: &str) -> Option<(&Arc, &crate::arc::Entry)> {
        let &(a, e) = self.index.get(key)?;
        let arc = &self.archives[a as usize];
        Some((arc, &arc.entries[e as usize]))
    }
}

pub struct Vfs {
    pub root: PathBuf,
    base: Files,
    mods: Mods<Files>,
}

impl Vfs {
    /// Opens every `resources/*.arc` of each layer present under `root`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let mut base = Files::default();
        for layer in LAYERS {
            base.mount_dir(&root.join(layer).join("resources"))?;
        }
        Ok(Self {
            root,
            base,
            mods: Mods::default(),
        })
    }

    /// `Engine::InitializeMod(name)`'s resources: the archives of [`mod_layers`] win over the
    /// install's until [`Vfs::remove_mod`] or the next mod. A missing mod folder is an error
    /// (a typo'd name must not mount the install as a "mod"); the expansion packs of the
    /// Crucible stay optional.
    pub fn initialize_mod(&self, name: &str) -> Result<()> {
        let layers = mod_layers(name);
        let dir = self.root.join(&layers[0]);
        anyhow::ensure!(dir.is_dir(), "mod folder not found: {}", dir.display());
        self.mods.initialize(name, || {
            let mut files = Files::default();
            for layer in &layers {
                files.mount_dir(&self.root.join(layer).join("resources"))?;
            }
            Ok(files)
        })
    }

    /// `Engine::RemoveModDirectory`: back to the install's files.
    pub fn remove_mod(&self) {
        self.mods.remove();
    }

    /// The mounted mod's name (`None`: the install alone).
    pub fn active_mod(&self) -> Option<&str> {
        self.mods.active().map(|m| m.name.as_str())
    }

    /// A counter bumped on every mount / unmount. Capture it before reading mod-dependent data
    /// and re-check after: unchanged means no swap happened mid-read, so a cache entry keyed by
    /// [`Self::active_mod`] is safe to keep (a name-only re-check is fooled by a swap away and back).
    pub fn mod_stamp(&self) -> u64 {
        self.mods.stamp.load(Ordering::Acquire)
    }

    fn entry(&self, key: &str) -> Option<(&Arc, &crate::arc::Entry)> {
        self.mods
            .active()
            .and_then(|m| m.layer.get(key))
            .or_else(|| self.base.get(key))
    }

    pub fn exists(&self, path: &str) -> bool {
        self.entry(&normalized(path)).is_some()
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let (arc, e) = self
            .entry(&normalized(path))
            .with_context(|| format!("resource not found: {path}"))?;
        arc.read(e)
    }

    /// A seekable reader of a resource (the file [`Vfs::read`] would return), decoding only what is read.
    pub fn open_entry(&self, path: &str) -> Result<crate::arc::EntryReader> {
        let (arc, e) = self
            .entry(&normalized(path))
            .with_context(|| format!("resource not found: {path}"))?;
        arc.open_entry(e)
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        merged_keys(&self.base.index, self.mods.active().map(|m| &m.layer.index))
    }
}

/// The base layer's keys, then the mod layer's keys the base does not have.
fn merged_keys<'a, V>(
    base: &'a HashMap<String, V>,
    layer: Option<&'a HashMap<String, V>>,
) -> impl Iterator<Item = &'a str> {
    let extra = layer
        .into_iter()
        .flat_map(|l| l.keys())
        .filter(|k| !base.contains_key(*k));
    base.keys().chain(extra).map(|s| s.as_str())
}

/// One slot per record of a database: the decoded record, or `None` when it
/// does not decode; filled on first use (see [`Database::get`]).
type RecordSlots = Box<[OnceLock<Option<Box<Record>>>]>;

/// Loaded `.arz` databases and their records by name (a later database replaces whole records).
#[derive(Default)]
struct Records {
    dbs: Vec<Arz>,
    index: HashMap<String, (u32, u32)>,
    /// Decoded records, kept: one slot per record of each database, filled on first use
    /// (`None`: the record does not decode). A filled slot is read without a lock.
    cache: Vec<RecordSlots>,
}

impl Records {
    /// Indexes `db`'s records over the earlier databases'.
    fn push(&mut self, db: Arz) {
        let di = self.dbs.len() as u32;
        for (ri, h) in db.records.iter().enumerate() {
            self.index
                .insert(normalize(db.record_name(h)), (di, ri as u32));
        }
        self.cache
            .push(db.records.iter().map(|_| OnceLock::new()).collect());
        self.dbs.push(db);
    }

    /// The record `key` if this layer has it: `Some(None)` when it does not decode.
    fn get(&self, key: &str) -> Option<Option<&Record>> {
        let &(d, r) = self.index.get(key)?;
        let db = &self.dbs[d as usize];
        Some(
            self.cache[d as usize][r as usize]
                .get_or_init(|| db.decode(&db.records[r as usize]).ok().map(Box::new))
                .as_deref(),
        )
    }
}

/// All `.arz` databases merged; later layers replace whole records.
pub struct Database {
    base: Records,
    mods: Mods<Records>,
}

impl Database {
    /// The install's records in `Engine::LoadMainDatabase` order: `database/database.arz`, then
    /// `gdx1/database/gdx1.arz` .. `gdx3` (names matched ignoring case: the install ships
    /// `GDX1.arz`), a layer without its file adding nothing. Any other `.arz` in those folders is
    /// not loaded, as in the engine.
    /// PORT: the engine's `/basemods` command-line switch (mounts `mods/` resources over the
    /// install and loads `mods/database.arz` last) is not ported.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let mut base = Records::default();
        for layer in LAYERS {
            let name = if layer.is_empty() { "database" } else { layer };
            if let Some(p) = named_arz(&root.join(layer).join("database"), name) {
                base.push(Arz::open(&p)?);
            }
        }
        Ok(Self {
            base,
            mods: Mods::default(),
        })
    }

    /// `Engine::InitializeMod(name)`'s records: `<layer>/database/<layer name>.arz` of each of
    /// [`mod_layers`] (under the game `root`) replace the install's until
    /// [`Database::remove_mod`] or the next mod. `LoadDatabase` opens that one file: a layer
    /// without it adds nothing (the engine logs "Unable to open database archive" and mounts the
    /// mod all the same), and no other `.arz` of the folder is read. A missing mod folder is an
    /// error. Returns the `.arz` files of the mod's database folders that were not loaded (a
    /// misnamed mod database), for the caller to warn about; only on the mod's first mount.
    pub fn initialize_mod(&self, root: &Path, name: &str) -> Result<Vec<PathBuf>> {
        let layers = mod_layers(name);
        let dir = root.join(&layers[0]);
        anyhow::ensure!(dir.is_dir(), "mod folder not found: {}", dir.display());
        let mut skipped = Vec::new();
        self.mods.initialize(name, || {
            let mut records = Records::default();
            for layer in &layers {
                let file = layer.rsplit('/').next().unwrap_or(layer);
                let dir = root.join(layer).join("database");
                let found = named_arz(&dir, file);
                if let Some(p) = &found {
                    records.push(Arz::open(p)?);
                }
                skipped.extend(
                    files_with_ext(&dir, "arz")
                        .into_iter()
                        .filter(|p| Some(p) != found.as_ref()),
                );
            }
            Ok(records)
        })?;
        Ok(skipped)
    }

    /// `Engine::RemoveModDirectory` + `ReloadDatabase`: back to the install's records.
    pub fn remove_mod(&self) {
        self.mods.remove();
    }

    /// The mounted mod's name (`None`: the install alone).
    pub fn active_mod(&self) -> Option<&str> {
        self.mods.active().map(|m| m.name.as_str())
    }

    /// The record `name`, decoded on first use and kept.
    pub fn get(&self, name: &str) -> Option<&Record> {
        let key = normalized(name);
        self.mods
            .active()
            .and_then(|m| m.layer.get(&key))
            .or_else(|| self.base.get(&key))
            .flatten()
    }

    /// [`Database::get`] as an owned record: the name and class are copied, the fields
    /// shared. Code that only reads the record should borrow it instead.
    pub fn get_owned(&self, name: &str) -> Option<Record> {
        self.get(name).cloned()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        merged_keys(&self.base.index, self.mods.active().map(|m| &m.layer.index))
    }

    pub fn len(&self) -> usize {
        self.names().count()
    }

    pub fn is_empty(&self) -> bool {
        self.base.index.is_empty() && self.mods.active().is_none_or(|m| m.layer.index.is_empty())
    }
}
