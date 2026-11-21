//! [`ScriptEnv`] over the layered install (VFS + merged database + EN tags),
//! and loaders for every quest/conversation file.
//!
//! * Quests: PORT. [`DataEnv::quest_defs`] loads every `quests/*.qst` of
//!   the VFS up front; the engine loads the current world's quest-file list
//!   (`Quest2Repository::LoadQuests` 0x180422870 via
//!   `World::GetQuestFile`, the `.map` header; see
//!   [`QuestDefs::repository_order`]).
//! * Conversations: PORT. [`DataEnv::dialogs`] loads every
//!   `conversations/*.cnv` up front; the engine loads a character's
//!   conversation from its record's `conversation` field (Game.dll
//!   `Character::Load` reads it).
//! * Text: PORT. The engine loads the tag table of the configured language
//!   (Engine.dll `LocalizationManager::Load`); the port's host passes the
//!   English one.
//! * Item footprints ([`ScriptEnv::item_footprint`], `Player.IsSpaceAvailable`): a fresh item of the record,
//!   resolved and sized from its bitmap as the inventory places it.
use super::dialog::Dialogs;
use super::lua::ScriptEnv;
use super::quest::QuestDefs;
use anyhow::Result;
use gd_formats::arz::{RawValue, Record};
use gd_formats::text::TagTable;
use gd_formats::vfs::{Database, Vfs};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

pub struct DataEnv {
    pub vfs: Arc<Vfs>,
    pub db: Arc<Database>,
    pub tags: Option<Arc<TagTable>>,
    cache: RefCell<HashMap<String, Option<Arc<Record>>>>,
}

impl DataEnv {
    pub fn new(vfs: Arc<Vfs>, db: Arc<Database>, tags: Option<Arc<TagTable>>) -> Self {
        Self {
            vfs,
            db,
            tags,
            cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn record(&self, name: &str) -> Option<Arc<Record>> {
        let key = gd_formats::vfs::normalized(name);
        if let Some(r) = self.cache.borrow().get(key.as_ref()) {
            return r.clone();
        }
        let r = self.db.get_owned(&key).map(Arc::new);
        self.cache.borrow_mut().insert(key.into_owned(), r.clone());
        r
    }

    /// Every `quests/*.qst` of the VFS (later layers already override).
    pub fn quest_defs(&self) -> Result<QuestDefs> {
        let mut paths: Vec<&str> = self
            .vfs
            .paths()
            .filter(|p| p.starts_with("quests/") && p.ends_with(".qst"))
            .collect();
        paths.sort();
        QuestDefs::from_files(
            paths
                .into_iter()
                .map(|p| Ok::<_, anyhow::Error>((p.to_string(), self.vfs.read(p)?)))
                .collect::<Result<Vec<_>>>()?,
        )
    }

    pub fn dialogs(&self) -> Result<Dialogs> {
        let mut paths: Vec<&str> = self
            .vfs
            .paths()
            .filter(|p| p.starts_with("conversations/") && p.ends_with(".cnv"))
            .collect();
        paths.sort();
        Dialogs::from_files(
            paths
                .into_iter()
                .map(|p| Ok::<_, anyhow::Error>((p.to_string(), self.vfs.read(p)?)))
                .collect::<Result<Vec<_>>>()?,
        )
    }
}

impl ScriptEnv for DataEnv {
    fn database(&self) -> Option<&Database> {
        Some(&self.db)
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.vfs.read(path).ok()
    }

    fn record_field(&self, record: &str, field: &str) -> Option<String> {
        let r = self.record(record)?;
        if field.eq_ignore_ascii_case("Class") {
            return Some(r.class.clone());
        }
        let vals = r.get(field)?;
        let parts: Vec<String> = vals
            .iter()
            .map(|v| match v {
                RawValue::Str(s) => s.clone(),
                RawValue::Int(i) => i.to_string(),
                RawValue::Float(f) => f.to_string(),
                RawValue::Bool(b) => (*b as i32).to_string(),
            })
            .collect();
        Some(parts.join(";"))
    }

    fn localize(&self, tag: &str) -> Option<String> {
        self.tags.as_ref()?.get(tag).map(str::to_string)
    }

    fn item_footprint(&self, record: &str) -> Option<crate::items::inventory::Footprint> {
        use crate::items::inventory::{Footprint, bitmap_footprint};
        let item = gd_formats::save::item::Item {
            base_name: record.to_string(),
            ..Default::default()
        };
        let r = crate::items::resolve::resolve(&crate::items::ItemDb::new(self.db.clone()), &item)?;
        // As the inventory sizes it (`gd_game::inventory::Inventory::info`): 1×1 without a readable bitmap.
        Some(bitmap_footprint(&self.vfs, &r.bitmap).unwrap_or(Footprint { w: 1, h: 1 }))
    }
}
