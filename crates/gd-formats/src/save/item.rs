//! Item records shared by the character file and the shared stash files.
//!
//! Layout: `ItemReplicaInfo::WriteProperties` (Game.dll)
//! and `ReadProperties`; the reader only accepts the current
//! layout for container versions >= 11 (older versions drop fields).
//! Field names follow their use in game code (see each field).
use super::cipher::{Decoder, Encoder};
use anyhow::Result;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Item {
    pub base_name: String,
    pub prefix_name: String,
    pub suffix_name: String,
    pub modifier_name: String,
    pub transmute_name: String,
    pub seed: u32,
    /// Relic / component inserted into the item.
    pub relic_name: String,
    pub relic_bonus: String,
    pub relic_seed: u32,
    /// Augment (faction enchantment).
    pub augment_name: String,
    /// The augment's own `var1` (its level): `ItemEquipment::AddEnchantment`
    /// (Game.dll) creates the augment item with this as replica
    /// `var1` (`ItemEnchantment::SetEnchantmentLevel`)
    /// and `augment_seed` as its seed; the attach path (`ItemEquipment::AttachEnchantment`) stores
    /// them back from the augment's replica.
    pub unk_augment: u32,
    pub augment_seed: u32,
    /// The ascendant affix (container version >= 7: `ItemReplicaInfo` +288 →
    /// `ItemEquipment` +1624, read by `AddAscendantBonus`; was
    /// taken for a second component slot). The game reader discards
    /// `ascendant_unused` but the writer emits it.
    pub ascendant_name: String,
    pub ascendant_unused: String,
    /// Replica `var1` → `Item.relicLevel`: the relic/augment level
    /// (`ItemRelic::SetRelicLevel`, `ItemEnchantment::SetEnchantmentLevel`;
    /// components count their completed parts here).
    pub var1: u32,
    /// container version >= 4
    pub stack_count: u32,
    /// Blacksmith seed rerolls (container version >= 8; `ItemReplicaInfo.seedRerolls` →
    /// `Item.seedRerolls`, `GetSeedRerolls`).
    pub seed_rerolls: u32,
    /// Blacksmith affix rerolls (container version >= 11; `ItemReplicaInfo.affixRerolls` →
    /// `Item.affixRerolls`, `GetAffixRerolls`).
    pub affix_rerolls: u32,
}

impl Item {
    pub fn is_empty(&self) -> bool {
        self.base_name.is_empty()
    }

    pub fn read(d: &mut Decoder) -> Result<Self> {
        Ok(Self {
            base_name: d.string()?,
            prefix_name: d.string()?,
            suffix_name: d.string()?,
            modifier_name: d.string()?,
            transmute_name: d.string()?,
            seed: d.u32()?,
            relic_name: d.string()?,
            relic_bonus: d.string()?,
            relic_seed: d.u32()?,
            augment_name: d.string()?,
            unk_augment: d.u32()?,
            augment_seed: d.u32()?,
            ascendant_name: d.string()?,
            ascendant_unused: d.string()?,
            var1: d.u32()?,
            stack_count: d.u32()?,
            seed_rerolls: d.u32()?,
            affix_rerolls: d.u32()?,
        })
    }

    pub fn write(&self, e: &mut Encoder) -> Result<()> {
        e.string(&self.base_name)?;
        e.string(&self.prefix_name)?;
        e.string(&self.suffix_name)?;
        e.string(&self.modifier_name)?;
        e.string(&self.transmute_name)?;
        e.u32(self.seed);
        e.string(&self.relic_name)?;
        e.string(&self.relic_bonus)?;
        e.u32(self.relic_seed);
        e.string(&self.augment_name)?;
        e.u32(self.unk_augment);
        e.u32(self.augment_seed);
        e.string(&self.ascendant_name)?;
        e.string(&self.ascendant_unused)?;
        e.u32(self.var1);
        e.u32(self.stack_count);
        e.u32(self.seed_rerolls);
        e.u32(self.affix_rerolls);
        Ok(())
    }

    /// Every database record this item references (non-empty names).
    pub fn records(&self) -> Vec<&str> {
        [
            &self.base_name,
            &self.prefix_name,
            &self.suffix_name,
            &self.modifier_name,
            &self.transmute_name,
            &self.relic_name,
            &self.relic_bonus,
            &self.augment_name,
            &self.ascendant_name,
            &self.ascendant_unused,
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.as_str())
        .collect()
    }
}

/// An item placed in a stash page; position is stored as float cells
/// (the game writes `pixel_pos * 1/32`).
#[derive(Debug, Clone, PartialEq)]
pub struct StashItem {
    pub item: Item,
    pub x: f32,
    pub y: f32,
}

/// One stash page (`u32 0` block). Used by `player.gdc` block 4 and `transfer.gst`.
/// Game.dll `Player::SaveNewFormatData` / `GameEngine::WritePlayerTransfer`.
#[derive(Debug, Clone, PartialEq)]
pub struct StashSack {
    pub block_id: u32,
    pub width: u32,
    pub height: u32,
    pub items: Vec<StashItem>,
    /// Four u32s written after the items: the sack's `InventorySack.unused0`, `InventorySack.unused1`, symbol
    /// (`InventorySack::GetSymbolIndex`) and symbol colour
    /// (`GetSymbolColorIndex`); `name` is its button name (set by
    /// `SetButtonName`). Per `GameEngine::WritePlayerTransfer`. The first
    /// two are unused: `ReadPlayerTransfer` stores them and the sack
    /// copy/constructor (zero) are the only other code touching them.
    pub unk: [u32; 4],
    pub name: String,
}

impl StashSack {
    pub fn read(d: &mut Decoder) -> Result<Self> {
        let (block_id, end) = d.begin_block(None)?;
        let width = d.u32()?;
        let height = d.u32()?;
        let n = d.u32()?;
        let mut items = Vec::with_capacity(n.min(4096) as usize);
        for _ in 0..n {
            let item = Item::read(d)?;
            let x = d.f32()?;
            let y = d.f32()?;
            items.push(StashItem { item, x, y });
        }
        let unk = [d.u32()?, d.u32()?, d.u32()?, d.u32()?];
        let name = d.wstring()?;
        d.end_block(block_id, end)?;
        Ok(Self {
            block_id,
            width,
            height,
            items,
            unk,
            name,
        })
    }

    pub fn write(&self, e: &mut Encoder) -> Result<()> {
        e.begin_block(self.block_id);
        e.u32(self.width);
        e.u32(self.height);
        e.u32(self.items.len() as u32);
        for it in &self.items {
            it.item.write(e)?;
            e.f32(it.x);
            e.f32(it.y);
        }
        for v in self.unk {
            e.u32(v);
        }
        e.wstring(&self.name);
        e.end_block()
    }
}
