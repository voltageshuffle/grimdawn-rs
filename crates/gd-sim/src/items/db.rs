//! Record access for the item code over any record source (normally the merged [`Database`]).
//!
//! [`ItemDb`] holds no records of its own: the source decides what a name means now.
//! `Database::get` follows the Crucible overlay, so an `ItemDb` from `GameData::item_db`
//! serves the campaign's record or the overlay's as the session's mode changes.
use gd_formats::arz::{RawValue, Record};
use gd_formats::collections::HashMap;
use gd_formats::vfs::{Database, normalize};
use std::sync::Arc;

/// Anything that can lend decoded database records by name.
pub trait RecordSource: Send + Sync {
    fn record(&self, name: &str) -> Option<&Record>;
    /// Every record name (lower-case, `/` separators).
    fn names(&self) -> Vec<String> {
        Vec::new()
    }
}

impl RecordSource for Database {
    fn record(&self, name: &str) -> Option<&Record> {
        self.get(name)
    }
    fn names(&self) -> Vec<String> {
        Database::names(self).map(|s| s.to_string()).collect()
    }
}

/// In-memory records (tests, tools).
#[derive(Default)]
pub struct MemorySource(pub HashMap<String, Record>);

impl MemorySource {
    pub fn insert(&mut self, name: &str, class: &str, fields: &[(&str, RawValue)]) {
        let rec = Record::new(
            name,
            class,
            fields
                .iter()
                .map(|(k, v)| (Arc::from(*k), vec![v.clone()]))
                .collect(),
        );
        self.0.insert(normalize(name), rec);
    }
}

impl RecordSource for MemorySource {
    fn record(&self, name: &str) -> Option<&Record> {
        self.0.get(&normalize(name))
    }
    fn names(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

/// Record lookups shared by item resolution and loot generation.
#[derive(Clone)]
pub struct ItemDb {
    src: Arc<dyn RecordSource>,
}

impl ItemDb {
    pub fn new(src: Arc<dyn RecordSource>) -> Self {
        Self { src }
    }

    /// `None` for an empty name.
    pub fn get(&self, name: &str) -> Option<&Record> {
        if name.is_empty() {
            return None;
        }
        self.src.record(name)
    }

    pub fn source(&self) -> &dyn RecordSource {
        &*self.src
    }
}

/// Engine `LoadTable` accessors.
pub trait Fields {
    /// `GetFloat(name)`: first value, 0 when absent.
    fn float(&self, key: &str) -> f32;
    /// `GetInt(name)`: first value truncated, 0 when absent.
    fn int(&self, key: &str) -> i32;
    /// `GetArray(name)` followed by the attribute loaders' rule that a
    /// single `0` entry means "no values" (in every `LoadFromTable`).
    fn values(&self, key: &str) -> Vec<f32>;
    /// `GetArray(name)` without the zero rule.
    fn raw_values(&self, key: &str) -> Vec<f32>;
    /// Entry `index` of [`Fields::raw_values`], clamped to the last one (`None` when there are
    /// none).
    fn value_at(&self, key: &str, index: usize) -> Option<f32>;
    /// `GetString(name)` ("" when absent).
    fn string(&self, key: &str) -> &str;
    /// `GetString(name, index)` with the index clamped to the last entry.
    fn string_at(&self, key: &str, index: usize) -> &str;
}

impl Fields for Record {
    fn float(&self, key: &str) -> f32 {
        self.f32(key).unwrap_or(0.0)
    }
    fn int(&self, key: &str) -> i32 {
        self.get(key)
            .and_then(|v| v.first())
            .map(|v| match v {
                RawValue::Int(i) => *i,
                other => other.as_f32().unwrap_or(0.0) as i32,
            })
            .unwrap_or(0)
    }
    fn values(&self, key: &str) -> Vec<f32> {
        let v = self.f32s(key);
        if v.len() == 1 && v[0] == 0.0 {
            Vec::new()
        } else {
            v
        }
    }
    fn raw_values(&self, key: &str) -> Vec<f32> {
        self.f32s(key)
    }
    fn value_at(&self, key: &str, index: usize) -> Option<f32> {
        crate::stats::level_value(self, key, index)
    }
    fn string(&self, key: &str) -> &str {
        self.str(key).unwrap_or("")
    }
    fn string_at(&self, key: &str, index: usize) -> &str {
        let Some(vals) = self.get(key) else { return "" };
        if vals.is_empty() {
            return "";
        }
        match &vals[index.min(vals.len() - 1)] {
            RawValue::Str(s) => s,
            _ => "",
        }
    }
}
