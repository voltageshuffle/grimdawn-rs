//! [`ScriptEnv`] over the layered install (VFS + merged database + EN tags):
//! the gd-story script host's view of the game data, plus the item-footprint
//! query that resolves a fresh item through gd-sim's item rules.
//!
//! Lives in gd-game-core (not gd-story) because [`ScriptEnv::item_footprint`] needs
//! gd-sim's `items::resolve`, which sits above the story crate. The quest /
//! conversation loaders themselves are [`gd_story::story::load`]; [`DataEnv`]
//! delegates to them.
//!
//! * Text: the tag table of the configured language (Engine.dll
//!   `LocalizationManager::Load`), passed by the host. Quest / dialog text:
//!   a non-English session replaces each file's embedded text block with the
//!   line-ordered `text_<code>/<basename>.txt` override
//!   ([`gd_formats::text::load_text_override`], `PortableLocalization::Read`).
//! * Item footprints ([`ScriptEnv::item_footprint`], `Player.IsSpaceAvailable`): a fresh item of the record,
//!   resolved and sized from its bitmap as the inventory places it.
use gd_formats::arz::{RawValue, Record};
use gd_formats::collections::HashMap;
use gd_formats::text::TagTable;
use gd_formats::vfs::{Database, Vfs};
use gd_story::story::dialog::Dialogs;
use gd_story::story::load;
use gd_story::story::lua::ScriptEnv;
use gd_story::story::quest::QuestDefs;
use std::cell::RefCell;
use std::sync::Arc;

pub struct DataEnv {
    pub vfs: Arc<Vfs>,
    pub db: Arc<Database>,
    pub tags: Option<Arc<TagTable>>,
    language: String,
    cache: RefCell<HashMap<String, Option<Arc<Record>>>>,
}

impl DataEnv {
    pub fn new(
        vfs: Arc<Vfs>,
        db: Arc<Database>,
        tags: Option<Arc<TagTable>>,
        language: String,
    ) -> Self {
        Self {
            vfs,
            db,
            tags,
            language,
            cache: RefCell::new(HashMap::default()),
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

    /// Every `quests/*.qst` of the VFS ([`gd_story::story::load::quest_defs`]).
    pub fn quest_defs(&self) -> QuestDefs {
        load::quest_defs(&self.vfs, &self.language)
    }

    /// Every `conversations/*.cnv` of the VFS ([`gd_story::story::load::dialogs`]).
    pub fn dialogs(&self) -> Dialogs {
        load::dialogs(&self.vfs, &self.language)
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
                RawValue::Str(s) => s.to_string(),
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

    fn item_footprint(&self, record: &str) -> Option<gd_formats::inventory::Footprint> {
        use gd_formats::inventory::{Footprint, bitmap_footprint};
        let item = gd_formats::save::item::Item {
            base_name: record.to_string(),
            ..Default::default()
        };
        let r =
            gd_sim::items::resolve::resolve(&gd_sim::items::ItemDb::new(self.db.clone()), &item)?;
        // As the inventory sizes it (`crate::inventory::Inventory::info`): 1×1 without a readable bitmap.
        Some(bitmap_footprint(&self.vfs, &r.bitmap).unwrap_or(Footprint { w: 1, h: 1 }))
    }
}
