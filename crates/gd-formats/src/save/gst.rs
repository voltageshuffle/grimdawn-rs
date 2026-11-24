//! Shared (account-wide) stash files using the CheckedWriter cipher:
//! `transfer.gst` (shared stash), `transmutes.gst`, `reagents.gst`.
//! (`formulas.gst` is a tagged IOStream file, see [`super::tagged`].)
//!
//! Game.dll `GameEngine::WritePlayerTransfer` /
//! `ReadPlayerTransfer`, `WritePlayerTransmutes`,
//! `WritePlayerReagents`. Each file is
//! `u32 file_version; block <id> { u32 version; raw u32 == key; string mod_name; ... }`.
use super::cipher::{Decoder, Encoder};
use super::item::StashSack;
use anyhow::{Result, bail};

struct Head {
    file_version: u32,
    version: u32,
    block_end: usize,
    mod_name: String,
}

fn read_head(d: &mut Decoder, block_id: u32) -> Result<Head> {
    let file_version = d.u32()?;
    let (_, block_end) = d.begin_block(Some(block_id))?;
    let version = d.u32()?;
    d.check_key("stash header")?;
    let mod_name = d.string()?;
    Ok(Head {
        file_version,
        version,
        block_end,
        mod_name,
    })
}

fn write_head(
    e: &mut Encoder,
    file_version: u32,
    block_id: u32,
    version: u32,
    mod_name: &str,
) -> Result<()> {
    e.u32(file_version);
    e.begin_block(block_id);
    e.u32(version);
    e.key_word();
    e.string(mod_name)
}

fn finish_read(d: &mut Decoder, id: u32, end: usize) -> Result<()> {
    d.end_block(id, end)?;
    if !d.at_end() {
        bail!("{} trailing bytes", d.len() - d.pos);
    }
    Ok(())
}

/// `transfer.gst`: block 18.
#[derive(Debug, Clone, PartialEq)]
pub struct Transfer {
    pub seed: u32,
    pub file_version: u32,
    pub version: u32,
    pub mod_name: String,
    /// Present for version >= 5 (bit0 AoM, bit1 FG, bit2 FoA).
    pub expansion_status: u8,
    pub sacks: Vec<StashSack>,
}

impl Transfer {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(data)?;
        let h = read_head(&mut d, 18)?;
        if h.file_version > 2 || h.version != 11 {
            bail!(
                "transfer.gst version {}/{} not supported",
                h.file_version,
                h.version
            );
        }
        let expansion_status = d.u8()?;
        let n = d.u32()?;
        let sacks = (0..n)
            .map(|_| StashSack::read(&mut d))
            .collect::<Result<Vec<_>>>()?;
        finish_read(&mut d, 18, h.block_end)?;
        Ok(Self {
            seed: d.seed,
            file_version: h.file_version,
            version: h.version,
            mod_name: h.mod_name,
            expansion_status,
            sacks,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut e = Encoder::new(self.seed);
        write_head(&mut e, self.file_version, 18, self.version, &self.mod_name)?;
        e.u8(self.expansion_status);
        e.u32(self.sacks.len() as u32);
        for s in &self.sacks {
            s.write(&mut e)?;
        }
        e.end_block()?;
        e.finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransmuteEntry {
    pub block_id: u32,
    /// Map key: the `GAME::Item_Type` of the records (per
    /// `GameEngine::GetPlayerTransmutes`, a map Item_Type → TransmuteInfo list).
    pub key: u32,
    pub records: Vec<String>,
}

/// `transmutes.gst`: block 19 (v2), unlocked illusions/transmutes.
#[derive(Debug, Clone, PartialEq)]
pub struct Transmutes {
    pub seed: u32,
    pub file_version: u32,
    pub version: u32,
    pub mod_name: String,
    pub expansion_status: u8,
    /// Count field written by the game (`TransferStash.sackCount`); normally == entries.len().
    pub count: u32,
    pub entries: Vec<TransmuteEntry>,
}

impl Transmutes {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(data)?;
        let h = read_head(&mut d, 19)?;
        if h.version != 2 {
            bail!("transmutes.gst version {} not supported", h.version);
        }
        let expansion_status = d.u8()?;
        let count = d.u32()?;
        let mut entries = Vec::new();
        while d.pos < h.block_end {
            let (block_id, end) = d.begin_block(None)?;
            let key = d.u32()?;
            let n = d.u32()?;
            let records = (0..n).map(|_| d.string()).collect::<Result<Vec<_>>>()?;
            d.end_block(block_id, end)?;
            entries.push(TransmuteEntry {
                block_id,
                key,
                records,
            });
        }
        finish_read(&mut d, 19, h.block_end)?;
        Ok(Self {
            seed: d.seed,
            file_version: h.file_version,
            version: h.version,
            mod_name: h.mod_name,
            expansion_status,
            count,
            entries,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut e = Encoder::new(self.seed);
        write_head(&mut e, self.file_version, 19, self.version, &self.mod_name)?;
        e.u8(self.expansion_status);
        e.u32(self.count);
        for t in &self.entries {
            e.begin_block(t.block_id);
            e.u32(t.key);
            e.u32(t.records.len() as u32);
            for r in &t.records {
                e.string(r)?;
            }
            e.end_block()?;
        }
        e.end_block()?;
        e.finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reagent {
    pub block_id: u32,
    pub record: String,
    /// Stack count (`Item::GetStackSize` when the item object exists).
    pub count: u32,
}

/// `reagents.gst`: block 20 (v1), crafting materials stored at the blacksmith.
#[derive(Debug, Clone, PartialEq)]
pub struct Reagents {
    pub seed: u32,
    pub file_version: u32,
    pub version: u32,
    pub mod_name: String,
    pub count: u32,
    pub entries: Vec<Reagent>,
}

impl Reagents {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut d = Decoder::new(data)?;
        let h = read_head(&mut d, 20)?;
        if h.version != 1 {
            bail!("reagents.gst version {} not supported", h.version);
        }
        let count = d.u32()?;
        let mut entries = Vec::new();
        while d.pos < h.block_end {
            let (block_id, end) = d.begin_block(None)?;
            let record = d.string()?;
            let n = d.u32()?;
            d.end_block(block_id, end)?;
            entries.push(Reagent {
                block_id,
                record,
                count: n,
            });
        }
        finish_read(&mut d, 20, h.block_end)?;
        Ok(Self {
            seed: d.seed,
            file_version: h.file_version,
            version: h.version,
            mod_name: h.mod_name,
            count,
            entries,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut e = Encoder::new(self.seed);
        write_head(&mut e, self.file_version, 20, self.version, &self.mod_name)?;
        e.u32(self.count);
        for r in &self.entries {
            e.begin_block(r.block_id);
            e.string(&r.record)?;
            e.u32(r.count);
            e.end_block()?;
        }
        e.end_block()?;
        e.finish()
    }
}
