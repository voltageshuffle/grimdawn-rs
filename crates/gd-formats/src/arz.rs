//! `.arz` game database (version 3): a string table plus LZ4-compressed
//! records of typed key/value arrays.
//!
//! Field lookup ([`Record::get`]) is case-insensitive, first match wins. A record built by
//! [`Record::new`] (every decoded one) carries a sorted hash index of its keys, so a lookup is
//! a binary search; a record whose `fields` were replaced or edited since scans them in order.
//!
//! Decoded string values and field keys are `Arc<str>` shared with the archive's string
//! table, so caching decoded records does not duplicate the table.
//!
//! [`Record::derived`] memoizes a consumer's value derived from a record's fields in one of
//! [`DERIVED_SLOTS`] fixed slots, addressed by a const-generic index (lock-free reads, a stable
//! `&T`). Slot assignments (a consumer owns its index; a second type in the same slot reads `None`):
//! - 0: gd-sim `actor::RecordContributions` (per-record stat contributions by level/owner).
//! - 1: gd-sim `actor::RecordOffenseFields` (per-record offense field values by level index).
//! - 2: gd-sim `stats::RecordExprs` (parsed field equations: bio / proxy / loot).
//! - 3: gd-sim `[skills::SkillLevel; 64]` (parsed `skillLevelN` numbers / equations).
//!
//! A record that claims more than [`MAX_RECORD`] decoded bytes is refused (the largest stock or
//! shipped-mod record is about 43 KB), so a hostile mod cannot have one record allocate 255× its
//! compressed size.
use crate::util::{Reader, lz4_block};
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::sync::{Arc, Weak};

/// The most decoded bytes of one record (a hundredfold the largest known one).
pub const MAX_RECORD: u32 = 4 << 20;

#[derive(Debug, Clone)]
pub struct RecordHeader {
    pub name: u32,
    pub class: String,
    pub offset: u32,
    pub compressed: u32,
    pub size: u32,
    pub filetime: u64,
}

pub struct Arz {
    pub strings: Vec<Arc<str>>,
    pub records: Vec<RecordHeader>,
    data: Vec<u8>,
}

/// One decoded record: ordered fields with their value arrays.
#[derive(Debug, Clone, Default)]
pub struct Record {
    pub name: String,
    pub class: String,
    /// Shared: cloning a record (`Database::get_owned` clones the cached one) copies no values;
    /// `Arc::make_mut` to modify.
    pub fields: Arc<Vec<(Arc<str>, Vec<RawValue>)>>,
    index: FieldIndex,
}

/// The case-insensitive key index of a record's `fields` ([`Record::new`]): (key hash, field
/// index), sorted. It holds a `Weak` of the fields it was built from and is used only while that
/// is still `Record::fields`; the `Weak` also makes `Arc::make_mut` move the fields to a new
/// allocation, so an edit can never leave a stale index (or [`Record::derived`] value) in use.
#[derive(Clone, Default)]
struct FieldIndex(Option<Arc<IndexOf>>);

struct IndexOf {
    of: Weak<Vec<(Arc<str>, Vec<RawValue>)>>,
    keys: Box<[(u32, u32)]>,
    /// [`Record::derived`]'s values, one per consumer slot.
    derived: [std::sync::OnceLock<Box<dyn std::any::Any + Send + Sync>>; DERIVED_SLOTS],
}

/// Number of [`Record::derived`] slots (see the module doc for the assignments).
pub const DERIVED_SLOTS: usize = 4;

impl std::fmt::Debug for FieldIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FieldIndex({})",
            self.0.as_ref().map_or(0, |i| i.keys.len())
        )
    }
}

/// FNV-1a of the ASCII-lowercased key (keys compare with `eq_ignore_ascii_case`).
fn key_hash(key: &str) -> u32 {
    key.bytes().fold(0x811c_9dc5, |h, b| {
        (h ^ u32::from(b.to_ascii_lowercase())).wrapping_mul(0x0100_0193)
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum RawValue {
    Int(i32),
    Float(f32),
    Str(Arc<str>),
    Bool(bool),
}

impl Arz {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let data = std::fs::read(path.as_ref())
            .with_context(|| format!("read {}", path.as_ref().display()))?;
        Self::parse(data)
    }

    pub fn parse(data: Vec<u8>) -> Result<Self> {
        let mut r = Reader::new(&data);
        let magic = r.u16()?;
        let version = r.u16()?;
        if magic != 2 || version != 3 {
            bail!("unsupported ARZ header {magic}/{version}");
        }
        let rec_start = r.u32()? as usize;
        let _rec_size = r.u32()?;
        let rec_count = r.u32()? as usize;
        let str_start = r.u32()? as usize;
        let str_size = r.u32()? as usize;

        let mut strings = Vec::new();
        let mut r = Reader::at(&data, str_start);
        while r.pos < str_start + str_size {
            let n = r.u32()?;
            for _ in 0..n {
                strings.push(r.string()?.into());
            }
        }
        let mut r = Reader::at(&data, rec_start);
        // A header is at least 28 bytes (an empty class name).
        let mut records = Vec::with_capacity(r.capacity(rec_count, 28));
        for _ in 0..rec_count {
            let name = r.u32()?;
            if name as usize >= strings.len() {
                bail!(
                    "record name {name} outside the string table ({})",
                    strings.len()
                );
            }
            let class = r.string()?;
            let Some(offset) = r.u32()?.checked_add(24) else {
                bail!("record data offset overflows");
            };
            let compressed = r.u32()?;
            let size = r.u32()?;
            let filetime = r.u64()?;
            records.push(RecordHeader {
                name,
                class,
                offset,
                compressed,
                size,
                filetime,
            });
        }
        Ok(Self {
            strings,
            records,
            data,
        })
    }

    /// The name of a header of this database (`parse` checked its string index).
    pub fn record_name(&self, h: &RecordHeader) -> &str {
        &self.strings[h.name as usize]
    }

    fn string(&self, i: u32) -> Result<&Arc<str>> {
        self.strings.get(i as usize).with_context(|| {
            format!(
                "string {i} outside the string table ({})",
                self.strings.len()
            )
        })
    }

    pub fn decode(&self, h: &RecordHeader) -> Result<Record> {
        let start = h.offset as usize;
        let src = start
            .checked_add(h.compressed as usize)
            .and_then(|end| self.data.get(start..end))
            .with_context(|| {
                format!(
                    "record data ({} bytes at {start:#x}) past end of file",
                    h.compressed
                )
            })?;
        if h.size > MAX_RECORD {
            bail!("record of {} bytes exceeds {MAX_RECORD}", h.size);
        }
        let buf = lz4_block(src, h.size as usize)?;
        let mut r = Reader::new(&buf);
        let mut fields = Vec::with_capacity(r.capacity(r.remaining() / 8, 8));
        while r.remaining() >= 8 {
            let ty = r.u16()?;
            let count = r.u16()? as usize;
            let key = r.u32()?;
            let mut vals = Vec::with_capacity(r.capacity(count, 4));
            for _ in 0..count {
                let raw = r.u32()?;
                vals.push(match ty {
                    0 => RawValue::Int(raw as i32),
                    1 => RawValue::Float(f32::from_bits(raw)),
                    2 => RawValue::Str(self.string(raw)?.clone()),
                    3 => RawValue::Bool(raw != 0),
                    _ => bail!("unknown value type {ty}"),
                });
            }
            fields.push((self.string(key)?.clone(), vals));
        }
        Ok(Record::new(self.record_name(h), h.class.clone(), fields))
    }

    /// The header of record `name` (case-insensitive; the first match).
    pub fn find(&self, name: &str) -> Option<&RecordHeader> {
        self.records
            .iter()
            .find(|h| self.record_name(h).eq_ignore_ascii_case(name))
    }
}

impl Record {
    /// A record of `fields`, with its key index ([`Record::get`]).
    pub fn new(
        name: impl Into<String>,
        class: impl Into<String>,
        fields: Vec<(Arc<str>, Vec<RawValue>)>,
    ) -> Self {
        let mut keys: Box<[(u32, u32)]> = fields
            .iter()
            .enumerate()
            .map(|(i, (k, _))| (key_hash(k), i as u32))
            .collect();
        keys.sort_unstable();
        let fields = Arc::new(fields);
        let index = FieldIndex(Some(Arc::new(IndexOf {
            of: Arc::downgrade(&fields),
            keys,
            derived: [const { std::sync::OnceLock::new() }; DERIVED_SLOTS],
        })));
        Self {
            name: name.into(),
            class: class.into(),
            fields,
            index,
        }
    }

    /// A consumer's data derived from this record's fields, made by `make` on first use and shared by the
    /// record's clones (the record's own cache). `IDX` is the consumer's fixed slot (module doc); reads are
    /// lock-free and return a stable `&T`. `None` when the fields were replaced since the record was indexed,
    /// or the slot already holds another type.
    pub fn derived<const IDX: usize, T: std::any::Any + Send + Sync>(
        &self,
        make: impl FnOnce() -> T,
    ) -> Option<&T> {
        const { assert!(IDX < DERIVED_SLOTS, "derived slot index out of range") };
        let ix = self
            .index
            .0
            .as_deref()
            .filter(|ix| std::ptr::eq(ix.of.as_ptr(), Arc::as_ptr(&self.fields)))?;
        ix.derived[IDX]
            .get_or_init(|| Box::new(make()))
            .downcast_ref()
    }

    /// The values of field `key` (case-insensitive; the first such field).
    pub fn get(&self, key: &str) -> Option<&[RawValue]> {
        let i = match self.index.0.as_deref() {
            Some(ix) if std::ptr::eq(ix.of.as_ptr(), Arc::as_ptr(&self.fields)) => {
                // Equal hashes are sorted by field index: the first matching key is the first field.
                let h = key_hash(key);
                ix.keys[ix.keys.partition_point(|e| e.0 < h)..]
                    .iter()
                    .take_while(|e| e.0 == h)
                    .map(|e| e.1 as usize)
                    .find(|&i| self.fields[i].0.eq_ignore_ascii_case(key))
            }
            _ => self
                .fields
                .iter()
                .position(|(k, _)| k.eq_ignore_ascii_case(key)),
        }?;
        Some(&self.fields[i].1)
    }

    /// First value as a string (empty strings count as missing).
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)?.first()? {
            RawValue::Str(s) if !s.is_empty() => Some(s),
            _ => None,
        }
    }

    /// The non-empty string values of field `key`, borrowed: no allocation per call.
    pub fn strs_iter(&self, key: &str) -> impl Iterator<Item = &str> {
        self.get(key).unwrap_or(&[]).iter().filter_map(|v| match v {
            RawValue::Str(s) if !s.is_empty() => Some(s.as_ref()),
            _ => None,
        })
    }

    pub fn strs(&self, key: &str) -> Vec<&str> {
        self.strs_iter(key).collect()
    }

    pub fn f32(&self, key: &str) -> Option<f32> {
        self.get(key)?.first()?.as_f32()
    }

    /// The numeric values of field `key`, borrowed: no allocation per call.
    pub fn f32s_iter(&self, key: &str) -> impl Iterator<Item = f32> {
        self.get(key)
            .unwrap_or(&[])
            .iter()
            .filter_map(RawValue::as_f32)
    }

    pub fn f32s(&self, key: &str) -> Vec<f32> {
        self.f32s_iter(key).collect()
    }

    /// An integer field as stored; other value types go through [`Record::f32`] (truncated).
    pub fn i32(&self, key: &str) -> Option<i32> {
        match self.get(key)?.first()? {
            RawValue::Int(i) => Some(*i),
            v => v.as_f32().map(|f| f as i32),
        }
    }

    pub fn bool(&self, key: &str) -> bool {
        self.f32(key).is_some_and(|f| f != 0.0)
    }
}

impl RawValue {
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            RawValue::Int(i) => Some(*i as f32),
            RawValue::Float(f) => Some(*f),
            RawValue::Bool(b) => Some(*b as i32 as f32),
            RawValue::Str(s) => s.parse().ok(),
        }
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    /// The lookup without the index: the first case-insensitive match in field order.
    fn linear<'a>(r: &'a Record, key: &str) -> Option<&'a [RawValue]> {
        r.fields
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_slice())
    }

    /// Every key of `r`, in its own, upper and lower case, plus a missing one, finds what the scan does.
    fn agrees(r: &Record) {
        for (k, _) in r.fields.iter() {
            for q in [
                k.to_string(),
                k.to_ascii_uppercase(),
                k.to_ascii_lowercase(),
            ] {
                assert_eq!(r.get(&q), linear(r, &q), "{} {q}", r.name);
            }
            let missing = format!("{k}#");
            assert_eq!(r.get(&missing), None);
        }
    }

    #[test]
    fn indexed_get_equals_the_linear_scan_on_game_records() {
        let Some(root) = crate::testdata::game() else {
            return;
        };
        let db = Arz::open(root.join("database/database.arz")).unwrap();
        for h in db.records.iter().step_by(97) {
            agrees(&db.decode(h).unwrap());
        }
    }
}
