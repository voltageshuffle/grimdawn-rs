//! The account's stash, blacksmith materials and learned blueprints. A hardcore character uses the
//! `.gsh` shared files ([`shared_path`], `GameEngine::GetSharedSavePath`); a new transfer file gets
//! [`LOADED_EXPANSIONS`].
//!
//! Move reagents ([`DepositReagents`]): `PlayerInventoryCtrl::DepositReagents` (Game.dll) runs
//! `InventorySack::DepositSackIntoReagents` on every bag (not worn items): each `craftingMaterial`
//! item goes into the material storage (`GameEngine::AddItemToReagents`: keyed by record, no
//! capacity) and leaves the sack, and `reagents.gst` is written at once (`gd_game::save`).
//!
//! PORT: a shared store the save will not write ([`SharedFiles::accepts`]) refuses deposits like a
//! refused transfer ([`QuickActionError`]), so nothing goes where it would be lost; taking out stays
//! allowed. The per-character map files (`quests.gdd`, `conversations.gdd`, `map.fow`) get the same
//! guard ([`read_guarded`]): an unreadable one is set aside to `.bak` and never rewritten from empty.
use crate::inventory::Inventory;
use bevy::prelude::*;
use gd_formats::inventory::{Footprint, Sack};
use gd_formats::save::Shared;
use gd_formats::save::item::Item;

/// The personal stash pages and the shared transfer stash.
#[derive(Resource)]
pub struct Stash {
    pub pages: Vec<Sack<Item>>,
    pub transfer: Vec<Sack<Item>>,
    /// The loaded `transfer.gst` (header kept when saving).
    pub transfer_file: gd_formats::save::gst::Transfer,
    /// `reagents.gst`.
    pub materials: gd_formats::save::gst::Reagents,
    /// Learned blueprints and the `formulas.gst` file.
    pub formulas: gd_formats::save::tagged::Tagged,
    /// Stash items that found no cell on load, `true` = transfer stash: written back at their saved
    /// page and cell.
    pub unplaced: Vec<(bool, crate::inventory::Unplaced)>,
    /// Per page, the layout its sort button starts with, `[personal, transfer]` (`StashTab.sortCounts`).
    /// Never saved.
    pub sort_layouts: [Vec<u32>; 2],
}

impl Stash {
    /// Learns a blueprint; false when it was known already.
    pub fn learn(&mut self, record: &str) -> bool {
        self.formulas.learn_formula(record)
    }

    pub fn view_mut(&mut self, transfer: bool) -> &mut Vec<Sack<Item>> {
        if transfer {
            &mut self.transfer
        } else {
            &mut self.pages
        }
    }

    pub fn view(&self, transfer: bool) -> &[Sack<Item>] {
        if transfer {
            &self.transfer
        } else {
            &self.pages
        }
    }
}

pub fn matches(names: &[String], item: &Item) -> bool {
    names
        .iter()
        .any(|n| n.eq_ignore_ascii_case(&item.base_name))
}

/// How many of a reagent the player has, everywhere it can be taken from.
pub fn count(names: &[String], inv: &Inventory, stash: &Stash) -> u32 {
    let sacks = inv.bags.iter().chain(&stash.pages).chain(&stash.transfer);
    let items: u32 = sacks
        .flat_map(|s| &s.items)
        .filter(|p| matches(names, &p.item))
        .map(|p| p.item.stack_count.max(1))
        .sum();
    let stored: u32 = stash
        .materials
        .entries
        .iter()
        .filter(|e| names.iter().any(|n| n.eq_ignore_ascii_case(&e.record)))
        .map(|e| e.count)
        .sum();
    items + stored
}

pub fn take_from_sack(
    names: &[String],
    left: &mut u32,
    sack: &mut gd_formats::inventory::Sack<Item>,
) {
    sack.items.retain_mut(|p| {
        if *left == 0 || !matches(names, &p.item) {
            return true;
        }
        let n = p.item.stack_count.max(1).min(*left);
        *left -= n;
        p.item.stack_count = p.item.stack_count.max(1) - n;
        p.item.stack_count > 0
    });
}

/// Adds `n` of `record` to the material storage.
pub fn store_material(m: &mut gd_formats::save::gst::Reagents, record: &str, n: u32) {
    match m
        .entries
        .iter_mut()
        .find(|e| e.record.eq_ignore_ascii_case(record))
    {
        Some(e) => e.count += n,
        None => m.entries.push(gd_formats::save::gst::Reagent {
            block_id: 0,
            record: record.to_owned(),
            count: n,
        }),
    }
}

/// Takes `n` of a reagent (`CreateArtifactConfigCmd::TakeReagents` order).
pub fn take(names: &[String], n: u32, inv: &mut Inventory, stash: &mut Stash) {
    let mut left = n;
    for b in &mut inv.bags {
        take_from_sack(names, &mut left, b);
    }
    for e in &mut stash.materials.entries {
        if left > 0 && names.iter().any(|m| m.eq_ignore_ascii_case(&e.record)) {
            let k = e.count.min(left);
            e.count -= k;
            left -= k;
        }
    }
    stash.materials.entries.retain(|e| e.count > 0);
    for p in stash.pages.iter_mut().chain(stash.transfer.iter_mut()) {
        take_from_sack(names, &mut left, p);
    }
    inv.changed();
}

pub const STASH_W: u32 = 10;

pub const STASH_H: u32 = 19;

/// The expansion flags a shared file is written with (`GameEngine::WritePlayerTransfer`: 1 / 2 / 4
/// for `IsExpansion1/2/3Loaded`). PORT: the port always mounts all three.
pub const LOADED_EXPANSIONS: u8 = 7;

/// Where an account-wide file ([`Shared`]) is read from: the shared folder (`Choice::shared_save_dir`)
/// of the loaded character's save tree, else the port's. Always written to the port's.
pub fn shared_path(
    choice: &crate::session::Choice,
    save_dir: &crate::session::SaveDir,
    file: Shared,
) -> std::path::PathBuf {
    let in_tree = |root: &crate::session::SaveDir| choice.shared_file(root, file);
    choice
        .save
        .as_ref()
        .and_then(|p| p.ancestors().nth(3))
        .map(|d| in_tree(&crate::session::SaveDir(d.to_path_buf())))
        .filter(|p| p.exists())
        .unwrap_or_else(|| in_tree(save_dir))
}

/// Save files this process must not write: a shared store or per-character map file that could not
/// be read and could not be moved aside either. Keyed by the write target and refreshed by every
/// read, so it needs no session reset.
#[derive(Resource, Default)]
pub struct SharedFiles {
    blocked: Vec<std::path::PathBuf>,
}

impl SharedFiles {
    pub fn writable(&self, target: &std::path::Path) -> bool {
        !self.blocked.iter().any(|p| p == target)
    }

    /// The store of `file` takes deposits: only while the save writes it.
    pub fn accepts(
        &self,
        choice: &crate::session::Choice,
        save_dir: &crate::session::SaveDir,
        file: Shared,
    ) -> bool {
        self.writable(&choice.shared_file(save_dir, file))
    }
}

pub fn read_shared<T>(
    shared: &mut SharedFiles,
    choice: &crate::session::Choice,
    save_dir: &crate::session::SaveDir,
    file: Shared,
    parse: impl FnOnce(&[u8]) -> anyhow::Result<T>,
) -> Option<T> {
    let path = shared_path(choice, save_dir, file);
    let target = choice.shared_file(save_dir, file);
    read_guarded(shared, &path, &target, parse)
}

/// Reads `path` under the save guard. `None` = start from a new empty file. An existing file that
/// cannot be read or parsed has its write `target` renamed to `<file>.bak` (`.bak1`, ...) first; if
/// that fails the target is blocked ([`SharedFiles`]). PORT: the engine's handling of an unreadable
/// save file is not traced.
pub fn read_guarded<T>(
    files: &mut SharedFiles,
    path: &std::path::Path,
    target: &std::path::Path,
    parse: impl FnOnce(&[u8]) -> anyhow::Result<T>,
) -> Option<T> {
    files.blocked.retain(|p| *p != target);
    let error = match std::fs::read(path) {
        Ok(bytes) => match parse(&bytes) {
            Ok(v) => return Some(v),
            Err(e) => e,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => e.into(),
    };
    error!("{}: unreadable ({error:#})", path.display());
    match set_aside(target) {
        Ok(Some(bak)) => error!(
            "{} kept as {}; this session starts a new empty file",
            target.display(),
            bak.display()
        ),
        Ok(None) => {}
        Err(e) => {
            error!(
                "{} could not be set aside ({e}): it will not be written this session",
                target.display()
            );
            files.blocked.push(target.to_owned());
        }
    }
    None
}

/// Renames an existing `target` to the first free `<name>.bak`, `.bak1`, ...; `None` when there is none.
fn set_aside(target: &std::path::Path) -> std::io::Result<Option<std::path::PathBuf>> {
    if !target.try_exists()? {
        return Ok(None);
    }
    let mut name = target.as_os_str().to_owned();
    name.push(".bak");
    let mut bak = std::path::PathBuf::from(&name);
    let mut n = 0u32;
    while bak.try_exists()? {
        n += 1;
        let mut numbered = name.clone();
        numbered.push(n.to_string());
        bak = numbered.into();
    }
    std::fs::rename(target, &bak)?;
    Ok(Some(bak))
}

/// File sacks to stash pages; items no page holds go to `unplaced` flagged `transfer`.
pub fn to_pages(
    sacks: &[gd_formats::save::item::StashSack],
    inv: &mut Inventory,
    transfer: bool,
    unplaced: &mut Vec<(bool, crate::inventory::Unplaced)>,
) -> Vec<Sack<Item>> {
    let mut pages: Vec<Sack<Item>> = sacks
        .iter()
        .map(|s| {
            let (w, h) = page_size(s.width, s.height, transfer);
            Sack::new(w, h, true)
        })
        .collect();
    let saved = sacks.iter().enumerate().flat_map(|(sack, s)| {
        s.items.iter().map(move |it| crate::inventory::Unplaced {
            sack,
            // Saturating: a negative or NaN coordinate is 0, a huge one is outside the grid.
            x: it.x as u32,
            y: it.y as u32,
            item: it.item.clone(),
        })
    });
    let what = if transfer { "transfer stash" } else { "stash" };
    let left = crate::inventory::place_saved(inv, what, &mut pages, saved);
    unplaced.extend(left.into_iter().map(|u| (transfer, u)));
    pages
}

/// The page size of a file sack: a zero dimension is the default page (the engine writes a fresh
/// character's empty stash sack as 0×0). A 1×1 personal page is healed to the default too; a transfer
/// page keeps its saved size. PORT: the engine's reading of a 0 / 1×1 sack is not traced.
fn page_size(width: u32, height: u32, transfer: bool) -> (u32, u32) {
    let w = if width == 0 { STASH_W } else { width };
    let h = if height == 0 { STASH_H } else { height };
    if !transfer && w == 1 && h == 1 {
        (STASH_W, STASH_H)
    } else {
        (w, h)
    }
}

/// Move every crafting material from the bags into the material storage.
#[derive(Message, Clone, Copy, Debug)]
pub struct DepositReagents;

/// Returns how many items (stacks) moved.
pub fn deposit_reagents_now(
    db: &gd_formats::vfs::Database,
    inv: &mut Inventory,
    stash: &mut Stash,
) -> usize {
    let mut moved = 0;
    for bag in &mut inv.bags {
        bag.items.retain(|p| {
            let material = db
                .get(&p.item.base_name)
                .is_some_and(|r| r.i32("craftingMaterial").unwrap_or(0) != 0);
            if material {
                store_material(
                    &mut stash.materials,
                    &p.item.base_name,
                    p.item.stack_count.max(1),
                );
                moved += 1;
            }
            !material
        });
    }
    if moved > 0 {
        inv.changed();
    }
    moved
}

/// Stash pages back to file sacks, keeping each old sack's id, flags and name; `unplaced` items go
/// back to their saved cell.
pub fn to_sacks<'a>(
    old: &[gd_formats::save::item::StashSack],
    pages: &[Sack<Item>],
    unplaced: impl Iterator<Item = &'a crate::inventory::Unplaced>,
) -> Vec<gd_formats::save::item::StashSack> {
    use gd_formats::save::item::{StashItem, StashSack};
    let mut sacks: Vec<StashSack> = pages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut sack = old.get(i).cloned().unwrap_or(StashSack {
                block_id: 0,
                width: p.width,
                height: p.height,
                items: Vec::new(),
                unk: [0; 4],
                name: String::new(),
            });
            sack.width = p.width;
            sack.height = p.height;
            sack.items = p
                .items
                .iter()
                .map(|q| StashItem {
                    item: q.item.clone(),
                    x: q.x as f32,
                    y: q.y as f32,
                })
                .collect();
            sack
        })
        .collect();
    for u in unplaced {
        let last = sacks.len().saturating_sub(1);
        if let Some(sack) = sacks.get_mut(u.sack.min(last)) {
            sack.items.push(StashItem {
                item: u.item.clone(),
                x: u.x as f32,
                y: u.y as f32,
            });
        }
    }
    sacks
}

/// What the next stash page (`pages` owned) costs: `InventoryCostArray` /
/// `TransferPageCostArray` of the stash / transfer window record, by page
/// index; `None` when no more can be bought.
pub fn page_cost(window: &gd_formats::arz::Record, pages: usize) -> Option<u64> {
    let costs = if window.get("TransferPageCostArray").is_some() {
        window.f32s("TransferPageCostArray")
    } else {
        window.f32s("InventoryCostArray")
    };
    costs.get(pages).map(|c| *c as u64)
}

/// [`page_cost`] of the caravan window's `StashWindow` / `TransferWindow`.
pub fn next_page_cost(
    db: &gd_formats::vfs::Database,
    stash: &Stash,
    transfer: bool,
) -> Option<u64> {
    let window = db
        .get("records/ui/caravan/caravan_window.dbr")?
        .str(if transfer {
            "TransferWindow"
        } else {
            "StashWindow"
        })
        .and_then(|k| db.get(k))?;
    page_cost(window, stash.view(transfer).len())
}

/// The caravan window's clicks, resolved by the UI; answered with a `gd_game::trade::TradeDone`.
/// `page` is the shown page (clamped).
#[derive(Message, Clone, Debug)]
pub enum StashRequest {
    /// Left click on cell `cell` of the page: pick up / place / swap with the held item (`cursor`: in
    /// cells from the grid's top-left).
    Click {
        transfer: bool,
        page: usize,
        cell: (u32, u32),
        cursor: Vec2,
    },
    /// The page's sort button (`StashTab::SortSelectedSack` → `InventorySack::Sort`). Starts with the
    /// page's [`Stash::sort_layouts`] and adds the retries that moved nothing, not the press: a messy
    /// page sorts in its current layout, a sorted one goes on to the next layout that moves anything.
    Sort { transfer: bool, page: usize },
    /// The buy-a-page tab, sent after Yes (`CaravanWindow::PromptBuy` → `CaravanWindow::BuyPage`):
    /// pays and appends a page; still short of gold, it refuses with `tagTransferError01`.
    BuyPage { transfer: bool },
    /// Left click on the material box of `record` (`CaravanWindow::OnMaterialClick`): stores the held
    /// stack, or takes one to the cursor; `shift`: a stack of up to `itemMaxStackSize` straight into
    /// the bags. Right click (use on an item) is not ported.
    Material { record: String, shift: bool },
    /// Test hook `GD_TRANSFER`: the first personal stash item to the transfer stash.
    TestTransfer,
    /// Shift + left click on the bag item at `cell` of `bag` with the caravan open ([`quick_move`]).
    QuickMove {
        transfer: bool,
        page: usize,
        bag: usize,
        cell: (u32, u32),
    },
    /// Shift + left click on the worn item of slot `slot` (index into `crate::inventory::SLOTS`)
    /// ([`quick_move_worn`]).
    QuickMoveWorn {
        transfer: bool,
        page: usize,
        slot: usize,
    },
}

/// A refused bag quick action: `GameEngine::PlayErrorSound`, with `notify` first the
/// `tagTransferStashError` notification (`BagWidget_HandleMouseEvent`).
#[derive(Message, Clone, Copy, Debug)]
pub struct QuickActionError {
    pub notify: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickMove {
    /// The item left the bag (the engine then plays `Item::PlayDropSound`, [`ui_drop_sound`]).
    Moved,
    /// The page had no room for (the rest of) it: error sound; merged stacks stay merged.
    Full,
    /// Not allowed in the transfer stash: notification + error sound.
    Refused,
}

/// Shift + left click on a bag item with the caravan open (`BagWidget_HandleMouseEvent`, Game.dll):
/// * personal view: `Player::AddItemToPrivateStash(GetSelectedStashSackNumber)`;
/// * transfer view: `GameEngine::AddItemToReagents` for a `craftingMaterial`, else when
///   `Item::CanBePlacedInTransferStash` (not soulbound, not a material) `AddItemToTransfer`, else
///   [`QuickMove::Refused`].
///
/// An add first fills the record's not-full stacks on that page ([`crate::stack::merge_into`]), then
/// places the rest as a new item or fails. None: no item there.
pub fn quick_move(
    db: &gd_formats::vfs::Database,
    inv: &mut Inventory,
    stash: &mut Stash,
    transfer: bool,
    page: usize,
    bag: usize,
    cell: (u32, u32),
) -> Option<QuickMove> {
    let placed = inv.bags.get(bag)?.item_at(cell.0, cell.1)?.clone();
    let rec = db.get(&placed.item.base_name);
    let material = rec
        .as_ref()
        .is_some_and(|r| r.i32("craftingMaterial").unwrap_or(0) != 0);
    let remove = |inv: &mut Inventory| {
        inv.bags[bag]
            .items
            .retain(|q| !(q.x == placed.x && q.y == placed.y));
        inv.changed();
    };
    if transfer && material {
        store_material(
            &mut stash.materials,
            &placed.item.base_name,
            placed.item.stack_count.max(1),
        );
        remove(inv);
        return Some(QuickMove::Moved);
    }
    if transfer && rec.as_ref().is_some_and(|r| r.bool("soulbound")) {
        return Some(QuickMove::Refused);
    }
    let max = crate::stack::max_of(inv, &placed.item, crate::stack::engine_max(db));
    let pages = stash.view_mut(transfer);
    let tab = page.min(pages.len().saturating_sub(1));
    let Some(sack) = pages.get_mut(tab) else {
        return Some(QuickMove::Full);
    };
    let added = match crate::stack::merge_into(std::slice::from_mut(sack), placed.item.clone(), max)
    {
        None => true,
        Some(rest) => {
            let n = rest.stack_count;
            let ok = sack.add(rest, placed.size).is_some();
            if !ok && n != placed.item.stack_count {
                if let Some(q) = inv.bags[bag]
                    .items
                    .iter_mut()
                    .find(|q| q.x == placed.x && q.y == placed.y)
                {
                    q.item.stack_count = n;
                }
                inv.changed();
            }
            ok
        }
    };
    if !added {
        return Some(QuickMove::Full);
    }
    remove(inv);
    Some(QuickMove::Moved)
}

/// Shift + left click on a worn item with the caravan open (`EquipSlotWidget::HandleMouseEvent`): as
/// [`quick_move`] but no material storage branch and no sound of its own; not added (page full) →
/// nothing at all. None: the slot is empty.
pub fn quick_move_worn(
    db: &gd_formats::vfs::Database,
    inv: &mut Inventory,
    stash: &mut Stash,
    transfer: bool,
    page: usize,
    slot: usize,
) -> Option<QuickMove> {
    let item = inv.equipped.get(slot)?.clone()?;
    if transfer
        && db
            .get(&item.base_name)
            .is_some_and(|r| r.bool("soulbound") || r.i32("craftingMaterial").unwrap_or(0) != 0)
    {
        return Some(QuickMove::Refused);
    }
    let size = inv
        .info(&item)
        .map(|i| i.size)
        .unwrap_or(Footprint { w: 1, h: 1 });
    let max = crate::stack::max_of(inv, &item, crate::stack::engine_max(db));
    let pages = stash.view_mut(transfer);
    let tab = page.min(pages.len().saturating_sub(1));
    let sack = pages.get_mut(tab)?;
    let added = match crate::stack::merge_into(std::slice::from_mut(sack), item, max) {
        None => true,
        Some(rest) => sack.add(rest, size).is_some(),
    };
    if !added {
        return Some(QuickMove::Full);
    }
    inv.equipped[slot] = None;
    inv.mark_equipment_dirty();
    inv.changed();
    Some(QuickMove::Moved)
}

/// `Item::PlayDropSound`: the record's `brokenDropSound` (default
/// `records/sounds/ui/spak_itemdropbrokendefault.dbr`) for a Broken item, else its `dropSound`.
pub fn ui_drop_sound(db: &gd_formats::vfs::Database, item: &Item) -> Option<String> {
    let rec = db.get(&item.base_name)?;
    let broken = gd_sim::items::Classification::parse(rec.str("itemClassification").unwrap_or(""))
        == gd_sim::items::Classification::Broken;
    if broken {
        return Some(
            rec.str("brokenDropSound")
                .filter(|p| !p.is_empty())
                .unwrap_or("records/sounds/ui/spak_itemdropbrokendefault.dbr")
                .to_owned(),
        );
    }
    rec.str("dropSound")
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
}

/// A `gameengine.dbr` sound field (empty = none).
fn engine_sound(data: &crate::data::GameData, field: &str) -> Option<String> {
    data.db
        .get("records/game/gameengine.dbr")
        .and_then(|r| r.str(field).map(str::to_owned))
        .filter(|p| !p.is_empty())
}

/// `GameEngine::PlayErrorSound`: `genericErrorSound`.
pub fn error_sound(data: &crate::data::GameData) -> Option<String> {
    engine_sound(data, "genericErrorSound")
}

/// `GameEngine::PlayMarketPurchaseSound`: `marketPurchaseSound`.
pub fn market_purchase_sound(data: &crate::data::GameData) -> Option<String> {
    engine_sound(data, "marketPurchaseSound")
}

/// `GameEngine::PlayMarketSaleSound`: `marketSaleSound`.
pub fn market_sale_sound(data: &crate::data::GameData) -> Option<String> {
    engine_sound(data, "marketSaleSound")
}

/// The blueprint learn sound: `learnFormulaSoundName` (`ItemArtifactFormula::Use`).
pub fn learn_formula_sound(data: &crate::data::GameData) -> Option<String> {
    engine_sound(data, "learnFormulaSoundName")
}
