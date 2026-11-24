//! Tagged `IOStream` files: `formulas.gst`, read with its typed schema
//! ([`Tagged::parse`]; `map.dat` has its own, [`super::map_dat`]).
//!
//! Per Engine.dll `IOStreamWrite::*`: every
//! property is `u32 len + key bytes` (`HandlePropertyTag`) followed by the raw
//! little-endian value; `begin_block`/`end_block` are properties with the u32
//! values 0xB01DFACE / 0xDEADC0DE. Value widths depend on the C++ type passed
//! to `StreamPropertyEx` (u8/char = 1, short = 2, int/uint/float/bool/Name = 4,
//! string = u32 len + bytes, wstring = u32 len + 2*len, Vec3 = 12, UniqueId = 16,
//! Coords = 48, WorldCoords = 64) and are not stored in the file.
//!
//! PORT: the game never has to guess a width: its readers (`IOStreamRead::
//! StreamPropertyEx`, Engine.dll, and siblings) are called by each
//! object's `StreamProperties` in a fixed order with typed arguments; each
//! checks that the next key is the expected one (virtual `HandlePropertyTag`)
//! and reads the width its C++ type implies. `formulas.gst` has that
//! schema (`GameEngine::StreamPlayerFormulas`, Game.dll)
//! and is read with it. For a file without one, the generic
//! decoder picks the first candidate width (string, then 4/1/2/16/12/48/64
//! bytes) after which the next key (or EOF) parses. Encoding writes the same
//! bytes back, so the round trip is exact independent of the guess.
use super::cipher::{bytes_to_string, string_to_bytes};
use anyhow::{Result, bail};

pub const BEGIN_BLOCK: u32 = 0xB01D_FACE;
pub const END_BLOCK: u32 = 0xDEAD_C0DE;

#[derive(Debug, Clone, PartialEq)]
pub enum TagValue {
    U32(u32),
    Str(String),
    Bytes(Vec<u8>),
}

impl TagValue {
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            TagValue::U32(v) => Some(*v),
            TagValue::Str(s) if s.is_empty() => Some(0),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            TagValue::Str(s) => Some(s),
            TagValue::U32(0) => Some(""),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TagEntry {
    pub key: String,
    pub value: TagValue,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tagged {
    pub entries: Vec<TagEntry>,
}

fn u32_at(d: &[u8], p: usize) -> Option<u32> {
    d.get(p..p + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn is_key_at(d: &[u8], p: usize) -> bool {
    if p == d.len() {
        return true;
    }
    let Some(n) = u32_at(d, p) else { return false };
    let n = n as usize;
    if !(1..=128).contains(&n) || p + 4 + n > d.len() {
        return false;
    }
    let k = &d[p + 4..p + 4 + n];
    k[0].is_ascii_alphabetic()
        && k.iter()
            .all(|c| c.is_ascii_alphanumeric() || b"_.[]".contains(c))
}

fn stringy_key(k: &str) -> bool {
    let l = k.to_ascii_lowercase();
    l.contains("name") || l.contains("path") || l.contains("file")
}

/// The key at `p` and the offset just past it, when one parses there.
fn key_at(d: &[u8], p: usize) -> Option<(String, usize)> {
    if p == d.len() || !is_key_at(d, p) {
        return None;
    }
    let n = u32_at(d, p)? as usize;
    Some((bytes_to_string(&d[p + 4..p + 4 + n]), p + 4 + n))
}

/// A `formulas.gst` header: `begin_block` then `formulasVersion`. Detected by
/// the key names alone; the typed reader validates the block markers.
fn formulas_header(d: &[u8]) -> bool {
    let Some((key, p)) = key_at(d, 0) else {
        return false;
    };
    key == "begin_block" && key_at(d, p + 4).is_some_and(|(k, _)| k == "formulasVersion")
}

fn expect_key(d: &[u8], p: &mut usize, expect: &str) -> Result<()> {
    match key_at(d, *p) {
        Some((key, next)) if key == expect => {
            *p = next;
            Ok(())
        }
        Some((key, _)) => bail!(
            "formulas.gst: expected {expect:?}, found {key:?} at {:#x}",
            *p
        ),
        None => bail!("formulas.gst: expected {expect:?} at {:#x}", *p),
    }
}

impl Tagged {
    pub fn parse(d: &[u8]) -> Result<Self> {
        if formulas_header(d) {
            return Self::parse_formulas(d);
        }
        let mut p = 0;
        let mut entries = Vec::new();
        while p < d.len() {
            if !is_key_at(d, p) {
                bail!("expected a property key at {p:#x}");
            }
            let n = u32_at(d, p).unwrap() as usize;
            let key = bytes_to_string(&d[p + 4..p + 4 + n]);
            p += 4 + n;
            if key == "begin_block" || key == "end_block" {
                let Some(v) = u32_at(d, p) else {
                    bail!("truncated {key}")
                };
                entries.push(TagEntry {
                    key,
                    value: TagValue::U32(v),
                });
                p += 4;
                continue;
            }
            let mut chosen = None;
            if let Some(l) = u32_at(d, p) {
                let l = l as usize;
                let end = p + 4 + l;
                if end <= d.len() && is_key_at(d, end) && (l > 0 || stringy_key(&key)) {
                    chosen = Some((TagValue::Str(bytes_to_string(&d[p + 4..end])), 4 + l));
                }
            }
            if chosen.is_none() {
                for sz in [4usize, 1, 2, 16, 12, 48, 64] {
                    if p + sz <= d.len() && is_key_at(d, p + sz) {
                        let v = if sz == 4 {
                            TagValue::U32(u32_at(d, p).unwrap())
                        } else {
                            TagValue::Bytes(d[p..p + sz].to_vec())
                        };
                        chosen = Some((v, sz));
                        break;
                    }
                }
            }
            let Some((value, sz)) = chosen else {
                bail!("cannot determine the value width of {key:?} at {p:#x}")
            };
            entries.push(TagEntry { key, value });
            p += sz;
        }
        Ok(Self { entries })
    }

    /// Per `GameEngine::StreamPlayerFormulas` (Game.dll):
    /// `begin_block`, `formulasVersion` int, `numEntries` int, `expansionStatus`
    /// byte (version >= 3), then `numEntries` pairs of `itemName` string and
    /// `formulaRead` int (version >= 2), `end_block`. The widths are the C++
    /// types the game's reader passes, not a guess: with 20 entries the guess
    /// reads `numEntries` as a string (20 bytes on lands the first `itemName`
    /// key), and the `expansionStatus` byte reads as a ~2 KB string length.
    fn parse_formulas(d: &[u8]) -> Result<Self> {
        let mut p = 0;
        let mut entries = Vec::new();
        let word = |p: &mut usize| -> Result<u32> {
            let Some(v) = u32_at(d, *p) else {
                bail!("formulas.gst: truncated value at {p:#x}");
            };
            *p += 4;
            Ok(v)
        };
        let mut push = |key: &str, value: TagValue| {
            entries.push(TagEntry {
                key: key.to_owned(),
                value,
            });
        };
        expect_key(d, &mut p, "begin_block")?;
        let marker = word(&mut p)?;
        if marker != BEGIN_BLOCK {
            bail!("formulas.gst: bad begin_block marker {marker:#x}");
        }
        push("begin_block", TagValue::U32(marker));
        expect_key(d, &mut p, "formulasVersion")?;
        let version = word(&mut p)?;
        push("formulasVersion", TagValue::U32(version));
        expect_key(d, &mut p, "numEntries")?;
        let num = word(&mut p)?;
        push("numEntries", TagValue::U32(num));
        if version >= 3 {
            expect_key(d, &mut p, "expansionStatus")?;
            let Some(&status) = d.get(p) else {
                bail!("formulas.gst: truncated expansionStatus at {p:#x}");
            };
            p += 1;
            push("expansionStatus", TagValue::Bytes(vec![status]));
        }
        for _ in 0..num {
            expect_key(d, &mut p, "itemName")?;
            let Some(l) = u32_at(d, p) else {
                bail!("formulas.gst: truncated itemName length at {p:#x}");
            };
            let l = l as usize;
            let Some(s) = d.get(p + 4..).and_then(|s| s.get(..l)) else {
                bail!("formulas.gst: itemName of {l} bytes past end at {p:#x}");
            };
            let name = bytes_to_string(s);
            p += 4 + l;
            push("itemName", TagValue::Str(name));
            if version >= 2 {
                expect_key(d, &mut p, "formulaRead")?;
                let read = word(&mut p)?;
                push("formulaRead", TagValue::U32(read));
            }
        }
        expect_key(d, &mut p, "end_block")?;
        let marker = word(&mut p)?;
        if marker != END_BLOCK {
            bail!("formulas.gst: bad end_block marker {marker:#x}");
        }
        push("end_block", TagValue::U32(marker));
        if p != d.len() {
            bail!("formulas.gst: {} trailing bytes", d.len() - p);
        }
        Ok(Self { entries })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut o = Vec::new();
        for e in &self.entries {
            let k = string_to_bytes(&e.key)?;
            o.extend_from_slice(&(k.len() as u32).to_le_bytes());
            o.extend_from_slice(&k);
            match &e.value {
                TagValue::U32(v) => o.extend_from_slice(&v.to_le_bytes()),
                TagValue::Str(s) => {
                    let b = string_to_bytes(s)?;
                    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
                    o.extend_from_slice(&b);
                }
                TagValue::Bytes(b) => o.extend_from_slice(b),
            }
        }
        Ok(o)
    }

    pub fn get(&self, key: &str) -> Option<&TagValue> {
        self.entries.iter().find(|e| e.key == key).map(|e| &e.value)
    }

    /// `formulas.gst`: the `itemName` records (blueprints the player has learned).
    pub fn formula_records(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.key == "itemName")
            .filter_map(|e| e.value.as_str())
            .collect()
    }

    /// An empty `formulas.gst` (`formulasVersion` 3, as the game writes it;
    /// `expansionStatus` 0 = base game only).
    pub fn new_formulas() -> Self {
        let e = |key: &str, value: TagValue| TagEntry {
            key: key.to_owned(),
            value,
        };
        Self {
            entries: vec![
                e("begin_block", TagValue::U32(BEGIN_BLOCK)),
                e("formulasVersion", TagValue::U32(3)),
                e("numEntries", TagValue::U32(0)),
                e("expansionStatus", TagValue::Bytes(vec![0])),
                e("end_block", TagValue::U32(END_BLOCK)),
            ],
        }
    }

    /// Adds a learned blueprint (`itemName`, plus `formulaRead` 0 when
    /// `formulasVersion` >= 2 as [`Self::parse_formulas`] expects, before the
    /// closing `end_block`) and bumps `numEntries`; false if already known.
    pub fn learn_formula(&mut self, record: &str) -> bool {
        if self
            .formula_records()
            .iter()
            .any(|r| r.eq_ignore_ascii_case(record))
        {
            return false;
        }
        let at = self
            .entries
            .iter()
            .rposition(|e| e.key == "end_block")
            .unwrap_or(self.entries.len());
        let read = self
            .get("formulasVersion")
            .and_then(TagValue::as_u32)
            .is_some_and(|v| v >= 2);
        if read {
            self.entries.insert(
                at,
                TagEntry {
                    key: "formulaRead".into(),
                    value: TagValue::U32(0),
                },
            );
        }
        self.entries.insert(
            at,
            TagEntry {
                key: "itemName".into(),
                value: TagValue::Str(record.to_owned()),
            },
        );
        if let Some(n) = self.entries.iter_mut().find(|e| e.key == "numEntries")
            && let TagValue::U32(v) = &mut n.value
        {
            *v += 1;
        }
        true
    }

    /// Keeps the learned blueprints `keep` accepts: each other `itemName` goes with the `formulaRead` that
    /// follows it, and `numEntries` drops with them. The count removed.
    pub fn retain_formulas(&mut self, mut keep: impl FnMut(&str) -> bool) -> usize {
        let mut out = Vec::with_capacity(self.entries.len());
        let mut removed = 0;
        let mut entries = std::mem::take(&mut self.entries).into_iter().peekable();
        while let Some(e) = entries.next() {
            if e.key == "itemName" && e.value.as_str().is_some_and(|r| !keep(r)) {
                removed += 1;
                entries.next_if(|n| n.key == "formulaRead");
                continue;
            }
            out.push(e);
        }
        self.entries = out;
        if let Some(n) = self.entries.iter_mut().find(|e| e.key == "numEntries")
            && let TagValue::U32(v) = &mut n.value
        {
            *v = v.saturating_sub(removed as u32);
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `formulas.gst`: learned and forgotten blueprints survive an encode / parse with their count
    /// (20 entries once made the width guess read `numEntries` as a string).
    #[test]
    fn formulas_round_trip() {
        let name = |i: usize| format!("records/items/crafting/blueprints/{i:040}.dbr");
        let mut f = Tagged::new_formulas();
        for i in 0..20 {
            assert!(f.learn_formula(&name(i)));
        }
        assert!(!f.learn_formula(&name(0)), "learned once");
        let mut back = Tagged::parse(&f.encode().unwrap()).unwrap();
        assert_eq!(back.get("numEntries"), Some(&TagValue::U32(20)));
        assert_eq!(back.formula_records().len(), 20);
        assert_eq!(back.get("expansionStatus"), Some(&TagValue::Bytes(vec![0])));
        assert_eq!(back.retain_formulas(|r| r != name(3)), 1);
        let back = Tagged::parse(&back.encode().unwrap()).unwrap();
        assert_eq!(back.get("numEntries"), Some(&TagValue::U32(19)));
        assert_eq!(
            back.entries
                .iter()
                .filter(|e| e.key == "formulaRead")
                .count(),
            19
        );
        assert!(!back.formula_records().contains(&name(3).as_str()));
    }
}
