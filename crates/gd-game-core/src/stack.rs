//! Item stacks: how many of an item one cell holds and merging a held
//! stack onto a bag / stash stack. The stack split window's systems are
//! `gd_game::stack` (`grimdawn` `ui/stack.rs` draws it); its amount rules
//! ([`split_max`], [`typed_amount`], [`increase`], [`decrease`]) are here.
//!
//! Stack size (Game.dll): `Item::GetMaxStackSize` is
//! the record's `maxStackSize` (`Item::Load`, `Item.maxStackSize`) when it is not 0,
//! else `itemMaxStackSize` of `records/game/gameiteminfo.dbr`
//! (`GameEngine::LoadFromDatabase`, `GameEngine.itemMaxStackSize`; 1000); `IsStackFull` is
//! count ≥ that. Components and charms (`ItemRelic::GetMaxStackSize`
//! / `IsStackFull` / `ItemRelic::IncrementStack`) stack only
//! when complete (`ItemRelic::IsComplete`, `gd_sim::items::components::
//! is_complete`), else 1. Equipment, relics (`ItemArtifact`) and the formula
//! classes never stack (their `IncrementStack` slot returns false,
//! `IsStackFull` true, max 1). PORT: `ItemTransmuter` (illusions) stacks only
//! when "complete" (`ItemTransmuter::IsComplete`, two fields not
//! traced); taken as never.
//! The rule is `gd_sim::items::resolve::max_stack` (also `ResolvedItem::max_stack`); [`max_stack`] delegates.
//!
//! Adding an item to the bags (pickups, purchases, a stash item moved over;
//! `PlayerInventoryCtrl::AddItem`) first puts it onto
//! the stacks of its record that are not full (`InventorySack::
//! GetNotFullItem`, sack by sack), the rest going on as a new item
//! ([`merge_into`], `Inventory::add_to_bags`). A ground pickup first joins
//! an incomplete component to a bag one (`Inventory::give`,
//! `gd_formats::inventory::shard_target`).
//!
//! Merging (`CursorHandlerItemMove::PrimaryInventoryActivate`,
//! the same in `PrimaryStashActivate` and
//! `PrimaryTransferActivate`): a held item put down over exactly
//! one item of the same record (`GetObjectName`) that is not full, the held
//! item being stackable, goes onto it (`Item::IncrementStack`:
//! above the maximum the rest is left over); the rest stays on the cursor,
//! else the held item is gone. Otherwise the usual swap
//! ([`place_held`]). [`place_stacking`].
use crate::inventory::Inventory;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::inventory::{Footprint, Sack, drop_cell, overlaps, place_held};
use gd_formats::save::item::Item;

/// The game's item settings record (`itemMaxStackSize`).
pub const GAME_ITEM_INFO: &str = gd_sim::items::resolve::GAME_ITEM_INFO;

/// `GameEngine::GetItemMaxStackSize`: `itemMaxStackSize` of [`GAME_ITEM_INFO`] (1000 in stock).
pub fn engine_max(db: &gd_formats::vfs::Database) -> u32 {
    engine_max_in(db.get(GAME_ITEM_INFO))
}

/// [`engine_max`] from the record itself.
pub fn engine_max_in(info: Option<&Record>) -> u32 {
    gd_sim::items::resolve::engine_max_stack(info)
}

/// `PlayerInventoryCtrl::AddItem`'s merge (module doc): `item` onto the first stacks of its record with room in
/// `sacks`, in order; returns what is left (None: all of it went).
pub fn merge_into(sacks: &mut [Sack<Item>], mut item: Item, max: u32) -> Option<Item> {
    if max <= 1 {
        return Some(item);
    }
    for q in sacks.iter_mut().flat_map(|s| s.items.iter_mut()) {
        if q.item.base_name.eq_ignore_ascii_case(&item.base_name) && count(&q.item) < max {
            match increment(&mut q.item, count(&item), max) {
                0 => return None,
                left => item.stack_count = left,
            }
        }
    }
    Some(item)
}

/// The count of a stack (a save's single item may store 0).
pub fn count(item: &Item) -> u32 {
    item.stack_count.max(1)
}

/// `GetMaxStackSize` of `item` (record `rec`, module doc); `engine` = [`engine_max`].
pub fn max_stack(rec: &Record, item: &Item, engine: u32) -> u32 {
    gd_sim::items::resolve::max_stack(rec, item, engine)
}

/// [`max_stack`] from the item database (1 for an unknown record).
pub fn max_of(inv: &Inventory, item: &Item, engine: u32) -> u32 {
    inv.db()
        .get(&item.base_name)
        .map_or(1, |r| max_stack(r, item, engine))
}

/// The window's largest amount (`StackWindow::ValidateAmount`): one less than the
/// stack, at most `GetItemMaxStackSize`.
pub fn split_max(stack: u32, engine: u32) -> u32 {
    stack.saturating_sub(1).min(engine)
}

/// The amount the typed text gives (`StackWindow::ValidateAmount`: `atoi`; empty or
/// 0 is 0, which disables OK; above `max` it is `max`, and the text is
/// rewritten to it).
pub fn typed_amount(text: &str, max: u32) -> u32 {
    let digits: String = text
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits
        .parse::<u64>()
        .map_or(0, |v| v.min(max as u64) as u32)
}

/// The + button (`StackWindow::OnIncrease`: amount + 1, then validated).
pub fn increase(amount: u32, max: u32) -> u32 {
    (amount + 1).min(max)
}

/// The − button (`StackWindow::OnDecrease`: amount − 1, at least 1).
pub fn decrease(amount: u32) -> u32 {
    amount.saturating_sub(1).max(1)
}

/// `Item::IncrementStack`: adds `n` to `target` up to `max`; returns what is left over.
pub fn increment(target: &mut Item, n: u32, max: u32) -> u32 {
    let sum = count(target) + n;
    target.stack_count = sum.min(max);
    sum.saturating_sub(max)
}

/// Puts `held` (footprint `size`, its [`max_stack`] `max`) down on `sack` at
/// `cursor` (module doc): onto the one overlapped stack of its record when
/// that has room (`Ok`: the rest still held, None when all went), else
/// [`place_held`].
// `Err` hands the item back to the caller.
#[allow(clippy::result_large_err)]
pub fn place_stacking(
    sack: &mut Sack<Item>,
    mut held: Item,
    size: Footprint,
    cursor: Vec2,
    max: u32,
) -> Result<Option<Item>, Item> {
    let (x, y) = drop_cell(cursor.to_array(), (sack.width, sack.height), size);
    let one = {
        let mut under = (0..sack.items.len()).filter(|&i| overlaps(&sack.items[i], x, y, size));
        match (under.next(), under.next()) {
            (Some(i), None) => Some(i),
            _ => None,
        }
    };
    if let Some(i) = one
        && max > 1
        && sack.items[i]
            .item
            .base_name
            .eq_ignore_ascii_case(&held.base_name)
        && count(&sack.items[i].item) < max
    {
        let left = increment(&mut sack.items[i].item, count(&held), max);
        if left == 0 {
            return Ok(None);
        }
        held.stack_count = left;
        return Ok(Some(held));
    }
    place_held(sack, held, size, cursor.to_array())
}
