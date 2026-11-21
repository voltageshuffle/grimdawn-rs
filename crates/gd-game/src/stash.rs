//! The account's stash, blacksmith materials and learned blueprints (the
//! game loads and saves them). The shared files are the hardcore twins (`.gsh`) for a hardcore character
//! ([`shared_path`], `GameEngine::GetSharedSavePath`): its transfer stash is its own; a new transfer
//! file gets the expansion flags [`LOADED_EXPANSIONS`].
//!
//! Move reagents ([`DepositReagents`], the character window's
//! `reagentDepositButton`): `PlayerInventoryCtrl::DepositReagents`
//! (Game.dll 0x1803f4c40) runs `InventorySack::DepositSackIntoReagents` on every
//! sack (main bag and extra bags, not the worn items): each item whose record
//! has `craftingMaterial` set (`Item::Load`, Item+0xC64) goes into the
//! material storage (`GameEngine::AddItemToReagents`: a store keyed by record
//! name, the stack added to the stored count; no capacity, it never refuses)
//! and is removed from the sack; when anything moved, `GameEngine::SaveReagents`.
//! PORT: `reagents.gst` is written with the next save (`save.rs`), not at once.
//!
//! A shared store the save will not write ([`SharedFiles`]: unreadable at load and not set aside) refuses
//! deposits (PORT, [`SharedFiles::accepts`]): placing, quick-moving or storing an item into a blocked transfer
//! stash or material storage, and the reagent deposit button, are answered like a refused transfer
//! ([`QuickActionError`] with its notification) and change nothing, so nothing goes where it would be lost.
//! Taking items out stays allowed (the unsaved store then still holds them: a duplicate, never a loss).
//!
//! Shift + left click on a bag item with the caravan open ([`StashRequest::QuickMove`]): [`quick_move`].
use crate::inventory::Inventory;
use crate::session::MessageDrain;
use bevy::prelude::*;
use gd_formats::save::Shared;
use gd_formats::save::item::Item;
use gd_sim::items::inventory::{Footprint, Sack};

/// The personal stash pages and the shared transfer stash.
#[derive(Resource)]
pub struct Stash {
    pub pages: Vec<Sack<Item>>,
    pub transfer: Vec<Sack<Item>>,
    /// The loaded `transfer.gst` (header kept when saving).
    pub transfer_file: gd_formats::save::gst::Transfer,
    /// The blacksmith's material storage (`reagents.gst`).
    pub materials: gd_formats::save::gst::Reagents,
    /// Learned blueprints and the `formulas.gst` file (the `itemName` records).
    pub formulas: gd_formats::save::tagged::Tagged,
    /// Stash items of the files that found no cell on load (`inventory::place_saved`), `true` = the
    /// transfer stash: written back at their saved page and cell (`to_sacks`).
    pub unplaced: Vec<(bool, crate::inventory::Unplaced)>,
}

impl Stash {
    /// Learns a blueprint (using an `ItemArtifactFormula` item); false when
    /// it was known already.
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
    sack: &mut gd_sim::items::inventory::Sack<Item>,
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

/// The expansion flags a shared file is written with (`GameEngine::WritePlayerTransfer`
/// @0x1802ddf30: on every write, 1 / 2 / 4 for `IsExpansion1/2/3Loaded`). PORT: the port always mounts the
/// three expansions (`gd_formats::vfs`), so all three are set.
pub const LOADED_EXPANSIONS: u8 = 7;

/// Where an account-wide file ([`Shared`]: `transfer`, `reagents`, `formulas`, `transmutes`; `.gsh` for a
/// hardcore character, [`Shared::file_name`]) is read from: the shared folder (`Choice::shared_save_dir`:
/// `save/`, or `save/<mod>/` in a custom game) of the loaded character's save tree
/// (`save/main/_<name>/player.gdc` → `save/`; read only), else the port's. Always written to the port's
/// (`Choice::shared_file`, `save.rs`).
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

/// The shared files this process must not write: a file that could not be read (I/O error, truncated, a
/// format this port does not parse) and could not be moved aside either. Keyed by the write target and
/// brought up to date by every [`read_shared`], so it needs no session reset.
#[derive(Resource, Default)]
pub struct SharedFiles {
    blocked: Vec<std::path::PathBuf>,
}

impl SharedFiles {
    /// The save may write `target` (`save.rs` `write_shared`).
    pub fn writable(&self, target: &std::path::Path) -> bool {
        !self.blocked.iter().any(|p| p == target)
    }

    /// The store of `file` (transfer stash, material storage) takes deposits: only while the save writes it
    /// (module doc).
    pub fn accepts(
        &self,
        choice: &crate::session::Choice,
        save_dir: &crate::session::SaveDir,
        file: Shared,
    ) -> bool {
        self.writable(&choice.shared_file(save_dir, file))
    }
}

/// Reads the account-wide file `file` ([`shared_path`]). `None` = start from a new empty file: right when
/// the file is absent. When it exists but cannot be read or parsed, the user's data must survive this
/// session's saves: whatever sits at the write target (the port's save folder) is renamed to `<file>.bak`
/// (`.bak1`, ... when taken) before anything can be written there; if that fails the target is blocked
/// ([`SharedFiles`]) and the saves skip it. PORT: the engine's handling of an unreadable shared file is not
/// traced.
pub fn read_shared<T>(
    shared: &mut SharedFiles,
    choice: &crate::session::Choice,
    save_dir: &crate::session::SaveDir,
    file: Shared,
    parse: impl FnOnce(&[u8]) -> anyhow::Result<T>,
) -> Option<T> {
    let path = shared_path(choice, save_dir, file);
    let target = choice.shared_file(save_dir, file);
    shared.blocked.retain(|p| *p != target);
    let error = match std::fs::read(&path) {
        Ok(bytes) => match parse(&bytes) {
            Ok(v) => return Some(v),
            Err(e) => e,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => e.into(),
    };
    error!("{}: unreadable ({error:#})", path.display());
    match set_aside(&target) {
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
            shared.blocked.push(target);
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

/// File sacks to stash pages ([`crate::inventory::place_saved`]); the items no page holds go to `unplaced`
/// flagged `transfer`.
pub fn to_pages(
    sacks: &[gd_formats::save::item::StashSack],
    inv: &mut Inventory,
    transfer: bool,
    unplaced: &mut Vec<(bool, crate::inventory::Unplaced)>,
) -> Vec<Sack<Item>> {
    let mut pages: Vec<Sack<Item>> = sacks
        .iter()
        .map(|s| Sack::new(s.width.max(1), s.height.max(1), true))
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

#[allow(clippy::too_many_arguments)]
pub fn init_stash(
    mut commands: Commands,
    // Option: `stash` is the existence check (this system creates it); `loaded` is absent for a new character.
    stash: Option<Res<Stash>>,
    loaded: Option<Res<crate::session::LoadedCharacter>>,
    mut inv: ResMut<Inventory>,
    choice: Res<crate::session::Choice>,
    save_dir: Res<crate::session::SaveDir>,
    data: Res<crate::data::GameData>,
    mut shared: ResMut<SharedFiles>,
) {
    if stash.is_some() {
        return;
    }
    let mut unplaced = Vec::new();
    let mut pages = loaded
        .as_ref()
        .map(|c| to_pages(&c.0.stash.sacks, &mut inv, false, &mut unplaced))
        .unwrap_or_default();
    if pages.is_empty() {
        pages.push(Sack::new(STASH_W, STASH_H, true));
    }
    let path = shared_path(&choice, &save_dir, Shared::Transfer);
    let transfer_file = read_shared(&mut shared, &choice, &save_dir, Shared::Transfer, |b| {
        gd_formats::save::gst::Transfer::parse(b)
    })
    .unwrap_or_else(|| gd_formats::save::gst::Transfer {
        seed: crate::rng::u32(..),
        file_version: 2,
        version: 11,
        mod_name: String::new(),
        expansion_status: LOADED_EXPANSIONS,
        sacks: Vec::new(),
    });
    let mut transfer = to_pages(&transfer_file.sacks, &mut inv, true, &mut unplaced);
    if transfer.is_empty() {
        transfer.push(Sack::new(STASH_W, STASH_H, true));
    }
    let count = |v: &[Sack<Item>]| v.iter().map(|p| p.items.len()).sum::<usize>();
    info!(
        "stash: {} pages, {} items; transfer ({}): {} pages, {} items",
        pages.len(),
        count(&pages),
        path.display(),
        transfer.len(),
        count(&transfer)
    );
    let materials = read_shared(&mut shared, &choice, &save_dir, Shared::Reagents, |b| {
        gd_formats::save::gst::Reagents::parse(b)
    })
    .unwrap_or_else(|| gd_formats::save::gst::Reagents {
        seed: crate::rng::u32(..),
        file_version: 1,
        version: 1,
        mod_name: String::new(),
        count: 0,
        entries: Vec::new(),
    });
    let mut formulas = read_shared(&mut shared, &choice, &save_dir, Shared::Formulas, |b| {
        gd_formats::save::tagged::Tagged::parse(b)
    })
    .unwrap_or_else(gd_formats::save::tagged::Tagged::new_formulas);
    // `GameEngine::StreamPlayerFormulas` @0x1802dc180: an invalid formula (missing record or
    // `gd_sim::items::crafting::formula_valid` false) is not added to the learned set.
    let dropped = formulas.retain_formulas(|r| {
        data.db
            .get(r)
            .is_some_and(|rec| gd_sim::items::crafting::formula_valid(&*data.db, rec))
    });
    if dropped > 0 {
        info!("crafting: {dropped} invalid learned blueprints dropped");
    }
    info!(
        "crafting: {} stored materials, {} learned blueprints",
        materials.entries.len(),
        formulas.formula_records().len()
    );
    commands.insert_resource(Stash {
        pages,
        transfer,
        transfer_file,
        materials,
        formulas,
        unplaced,
    });
}

/// The bags and the stash both exist.
fn has_bags_and_stash(inv: Option<Res<Inventory>>, stash: Option<Res<Stash>>) -> bool {
    inv.is_some() && stash.is_some()
}

pub struct StashPlugin;

impl Plugin for StashPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SharedFiles>()
            .add_message::<DepositReagents>()
            .drain_messages_unless::<DepositReagents, _>(has_bags_and_stash)
            .add_message::<StashRequest>()
            .add_message::<QuickActionError>()
            .add_systems(
                Update,
                quick_action_errors
                    .run_if(resource_exists::<crate::data::GameData>)
                    .after(apply_stash_requests),
            )
            .add_systems(
                Update,
                (
                    init_stash.run_if(resource_exists::<Inventory>),
                    deposit_reagents.run_if(has_bags_and_stash),
                    apply_stash_requests
                        .run_if(resource_exists::<Inventory>)
                        .run_if(resource_exists::<Stash>)
                        .run_if(resource_exists::<crate::progress::PlayerProgress>)
                        .run_if(resource_exists::<crate::data::GameData>),
                )
                    .chain()
                    .after(crate::inventory::InventorySet)
                    .before(crate::player::PlayerSet),
            );
    }
}

/// Move every crafting material from the bags into the material storage
/// (module doc).
#[derive(Message, Clone, Copy, Debug)]
pub struct DepositReagents;

/// Moves the bags' `craftingMaterial` items into `stash.materials`; returns
/// how many items (stacks) moved.
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

fn deposit_reagents(
    mut msgs: MessageReader<DepositReagents>,
    data: Res<crate::data::GameData>,
    mut inv: ResMut<Inventory>,
    mut stash: ResMut<Stash>,
    (shared, choice, save_dir): (
        Res<SharedFiles>,
        Res<crate::session::Choice>,
        Res<crate::session::SaveDir>,
    ),
    mut error: MessageWriter<QuickActionError>,
) {
    if msgs.read().count() == 0 {
        return;
    }
    if !shared.accepts(&choice, &save_dir, Shared::Reagents) {
        error.write(QuickActionError { notify: true });
        return;
    }
    let n = deposit_reagents_now(&data.db, &mut inv, &mut stash);
    debug!("stash: {n} reagent stacks moved to the material storage");
}

/// Stash pages back to file sacks, keeping each old sack's id, flags and name; the load's `unplaced` items
/// ([`Stash::unplaced`]) go back to their page at their saved cell.
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

/// What the next page of the personal (`transfer`: the transfer) stash costs: [`page_cost`] of the caravan
/// window's `StashWindow` / `TransferWindow`; `None` when no more can be bought.
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

/// The caravan window's clicks, resolved by the UI from its layout;
/// [`apply_stash_requests`] applies them and answers with a
/// [`TradeDone`](crate::trade::TradeDone) (the window rebuilds). `page` is
/// the shown page (clamped to the pages there are).
#[derive(Message, Clone, Debug)]
pub enum StashRequest {
    /// Left click on cell `cell` of the page: pick up / place / swap with
    /// the held item (`cursor`: in cells from the grid's top-left,
    /// [`drop_cell`](gd_sim::items::inventory::drop_cell)).
    Click {
        transfer: bool,
        page: usize,
        cell: (u32, u32),
        cursor: Vec2,
    },
    /// The sort button (`InventorySack::Sort` with flags 0, the
    /// first press; its sorter `sub_180596840` orders by height, then width,
    /// then the item-type rank (Item vtable +1360,
    /// [`sort_rank`](gd_sim::items::inventory::sort_rank)), all largest first, then
    /// by display name (`GetGameDescription`)), packed from the top left.
    Sort { transfer: bool, page: usize },
    /// The buy-a-page tab, sent once the player answered Yes to the window's question (GrimDawn.exe
    /// caravan click `sub_140136F30`: gold below [`page_cost`] → an Ok box `tagTransferError01`, else the
    /// Yes / No box `tagTransferPurchase01`, both with the cost; Yes → `sub_1401312B0` / `sub_140131830`): pays
    /// the cost and appends a page, the answer names it; still short of gold, it refuses with
    /// `tagTransferError01` ([`TradeRefused`](crate::trade::TradeRefused)), as the Yes handlers do.
    BuyPage { transfer: bool },
    /// Left click on the material box of `record` (transfer view): stores
    /// the held stack of it, or takes one (`shift`: a stack, straight into
    /// the bags) to the cursor. GrimDawn.exe caravan window click
    /// `sub_140132A30`: a left click puts one on the cursor; with Shift a
    /// stack of up to `gameiteminfo.dbr` `itemMaxStackSize`
    /// (`GameEngine::GetItemMaxStackSize`) goes straight into the bags when
    /// they have room. (A right click uses a stored component / augment on
    /// an item; not done.)
    Material { record: String, shift: bool },
    /// Test hook `GD_TRANSFER`: the first personal stash item to the transfer stash.
    TestTransfer,
    /// Shift + left click on the bag item at `cell` of `bag` while the caravan window is open: into the shown
    /// page `page` of the personal stash, or with the transfer view (`transfer`) into the material storage or
    /// page `page` of the transfer stash ([`quick_move`], the rules are there).
    QuickMove {
        transfer: bool,
        page: usize,
        bag: usize,
        cell: (u32, u32),
    },
    /// Shift + left click on the worn item of equipment slot `slot` (index into `crate::inventory::SLOTS`) while
    /// the caravan window is open ([`quick_move_worn`]).
    QuickMoveWorn {
        transfer: bool,
        page: usize,
        slot: usize,
    },
}

/// A refused bag quick action: `GameEngine::PlayErrorSound` (the `gameengine.dbr` `genericErrorSound`,
/// `GameEngine::LoadFromDatabase` / `PlayErrorSound` @0x1802c67b0, 2D), with `notify` first the
/// `tagTransferStashError` UI notification (`AddUINotification` type 0; GrimDawn.exe
/// `BagWidget_HandleMouseEvent` @0x1401EA910). Sent by [`quick_move`] and by the UI for the item-slot windows.
#[derive(Message, Clone, Copy, Debug)]
pub struct QuickActionError {
    pub notify: bool,
}

/// What [`quick_move`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickMove {
    /// The item left the bag (the engine then plays its `Item::PlayDropSound`, [`ui_drop_sound`]).
    Moved,
    /// The page had no room for (the rest of) it: the error sound; what merged onto the page's stacks stays
    /// merged, the bag keeps the rest (`SendUpdateItemStack`).
    Full,
    /// Not allowed in the transfer stash: notification + error sound, nothing changes.
    Refused,
}

/// Shift + left click on a bag item with the caravan window open (GrimDawn.exe
/// `BagWidget_HandleMouseEvent` @0x1401EA910, the Shift + caravan branch; Game.dll):
/// * personal view (`!GameEngine::IsTransferOpen`): `Player::AddItemToPrivateStash(GetSelectedStashSackNumber)`
///   @0x1803e3c60;
/// * transfer view: `GameEngine::AddItemToReagents` @0x1802e2690 first (only a `craftingMaterial` item,
///   Item+0xC64; into the material storage as [`deposit_reagents_now`], never refused), else when
///   `Item::CanBePlacedInTransferStash` (vt+1032 @0x180325380: not `soulbound` (+0xC00) and not a material, no
///   subclass overrides it) `GameEngine::AddItemToTransfer(GetSelectedTransferSackNumber)` @0x1802d7a30, else
///   [`QuickMove::Refused`].
///
/// Both adds are the same: the item first goes onto the not-full stacks of its record on that one page
/// (`InventorySack::GetNotFullItem` + `IncrementStack`, repeated while some is left, [`crate::stack::merge_into`]),
/// the rest is placed as a new item (`InventorySack::AddItem`, auto placement) or the add fails. `page` is the
/// page shown (the selected sack), clamped to the pages there are. None: no item there.
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

/// Shift + left click on a worn item with the caravan window open (GrimDawn.exe equipment slot handler
/// `sub_1401BC820`, left press with Shift while the caravan is shown): personal view
/// `Player::AddItemToPrivateStash(GetSelectedStashSackNumber, id, false)`; transfer view, when
/// `Item::CanBePlacedInTransferStash` (not soulbound, not a material), `AddItemToTransfer` into the selected
/// transfer sack, else [`QuickMove::Refused`] (`tagTransferStashError` + error sound). No material storage branch
/// and no sound of its own; added -> `EquipmentCtrl::RemoveItem`; not added (page full) -> nothing at all.
/// The add is [`quick_move`]'s (stacks of the record first, then auto placement). None: the slot is empty.
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

/// `Item::PlayDropSound` (vt+1040 @0x1803258a0, the 2D UI sound of Item+0xB48): `Item::Load`
/// @0x180324c60: the record's `brokenDropSound` (default `records/sounds/ui/spak_itemdropbrokendefault.dbr`) for a
/// Broken `itemClassification`, else its `dropSound`.
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

/// `GameEngine::PlayErrorSound`'s sound: the `gameengine.dbr` `genericErrorSound` ([`QuickActionError`]).
pub fn error_sound(data: &crate::data::GameData) -> Option<String> {
    engine_sound(data, "genericErrorSound")
}

/// `GameEngine::PlayMarketPurchaseSound` @0x1802c6750: the `gameengine.dbr` `marketPurchaseSound` (a buy).
pub fn market_purchase_sound(data: &crate::data::GameData) -> Option<String> {
    engine_sound(data, "marketPurchaseSound")
}

/// `GameEngine::PlayMarketSaleSound` @0x1802c6770: the `gameengine.dbr` `marketSaleSound` (a sale).
pub fn market_sale_sound(data: &crate::data::GameData) -> Option<String> {
    engine_sound(data, "marketSaleSound")
}

/// The blueprint learn sound: the `gameengine.dbr` `learnFormulaSoundName` (`ItemArtifactFormula::Use`).
pub fn learn_formula_sound(data: &crate::data::GameData) -> Option<String> {
    engine_sound(data, "learnFormulaSoundName")
}

/// [`QuickActionError`] to its notification and sound.
fn quick_action_errors(
    mut msgs: MessageReader<QuickActionError>,
    data: Res<crate::data::GameData>,
    mut notify: MessageWriter<crate::story::Notify>,
    mut sound: MessageWriter<crate::visuals::PlaySound>,
) {
    for m in msgs.read() {
        if m.notify {
            notify.write(crate::story::Notify("tagTransferStashError".into()));
        }
        if let Some(pak) = error_sound(&data) {
            sound.write(crate::visuals::PlaySound { pak, at: None });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_stash_requests(
    mut msgs: MessageReader<StashRequest>,
    mut done: MessageWriter<crate::trade::TradeDone>,
    mut stash: ResMut<Stash>,
    mut inv: ResMut<Inventory>,
    mut progress: ResMut<crate::progress::PlayerProgress>,
    data: Res<crate::data::GameData>,
    (mut error, mut sound, mut refused): (
        MessageWriter<QuickActionError>,
        MessageWriter<crate::visuals::PlaySound>,
        MessageWriter<crate::trade::TradeRefused>,
    ),
    (shared, choice, save_dir): (
        Res<SharedFiles>,
        Res<crate::session::Choice>,
        Res<crate::session::SaveDir>,
    ),
) {
    // A store the save does not write takes no deposits (module doc).
    let transfer_open = shared.accepts(&choice, &save_dir, Shared::Transfer);
    let reagents_open = shared.accepts(&choice, &save_dir, Shared::Reagents);
    for m in msgs.read() {
        match m {
            StashRequest::QuickMove {
                transfer,
                page,
                bag,
                cell,
            } => {
                let item = inv
                    .bags
                    .get(*bag)
                    .and_then(|b| b.item_at(cell.0, cell.1))
                    .map(|q| q.item.clone());
                // Into the transfer view a material goes to the material storage, the rest to the transfer stash.
                let blocked = *transfer
                    && item.as_ref().is_some_and(|i| {
                        let material = data
                            .db
                            .get(&i.base_name)
                            .is_some_and(|r| r.i32("craftingMaterial").unwrap_or(0) != 0);
                        if material {
                            !reagents_open
                        } else {
                            !transfer_open
                        }
                    });
                let outcome = if blocked {
                    Some(QuickMove::Refused)
                } else {
                    quick_move(
                        &data.db, &mut inv, &mut stash, *transfer, *page, *bag, *cell,
                    )
                };
                match outcome {
                    Some(QuickMove::Moved) => {
                        if let Some(pak) = item.and_then(|i| ui_drop_sound(&data.db, &i)) {
                            sound.write(crate::visuals::PlaySound { pak, at: None });
                        }
                    }
                    Some(QuickMove::Full) => {
                        error.write(QuickActionError { notify: false });
                    }
                    Some(QuickMove::Refused) => {
                        error.write(QuickActionError { notify: true });
                    }
                    None => continue,
                }
                done.write(Default::default());
            }
            StashRequest::QuickMoveWorn {
                transfer,
                page,
                slot,
            } => {
                let outcome = if *transfer && !transfer_open {
                    inv.equipped
                        .get(*slot)
                        .and_then(Option::as_ref)
                        .map(|_| QuickMove::Refused)
                } else {
                    quick_move_worn(&data.db, &mut inv, &mut stash, *transfer, *page, *slot)
                };
                match outcome {
                    Some(QuickMove::Moved) => {}
                    Some(QuickMove::Refused) => {
                        error.write(QuickActionError { notify: true });
                    }
                    Some(QuickMove::Full) | None => continue,
                }
                done.write(Default::default());
            }
            StashRequest::Click {
                transfer,
                page,
                cell: (cx, cy),
                cursor,
            } => {
                // `Item::CanBePlacedInTransferStash`: a soulbound item or a material stays on the cursor; so does
                // anything while the transfer stash is not saved (module doc).
                if *transfer
                    && inv.held.as_ref().is_some_and(|h| {
                        !transfer_open
                            || data.db.get(&h.base_name).is_some_and(|r| {
                                r.bool("soulbound") || r.i32("craftingMaterial").unwrap_or(0) != 0
                            })
                    })
                {
                    error.write(QuickActionError { notify: true });
                    continue;
                }
                // A held stack onto a stack of its record (`crate::stack::place_stacking`).
                let max = inv.held.as_ref().map_or(1, |h| {
                    crate::stack::max_of(&inv, h, crate::stack::engine_max(&data.db))
                });
                let pages = stash.view_mut(*transfer);
                let tab = (*page).min(pages.len().saturating_sub(1));
                let Some(page) = pages.get_mut(tab) else {
                    continue;
                };
                match inv.held.take() {
                    Some(h) => {
                        let size = inv
                            .info(&h)
                            .map(|i| i.size)
                            .unwrap_or(Footprint { w: 1, h: 1 });
                        let drop = ui_drop_sound(&data.db, &h);
                        match crate::stack::place_stacking(page, h, size, *cursor, max) {
                            Ok(under) => {
                                // The placed item's origin no longer applies; a swapped-in
                                // item has no cell to go back to (a cancel gives it).
                                inv.held_from = None;
                                inv.held = under;
                                if let Some(pak) = drop {
                                    sound.write(crate::visuals::PlaySound { pak, at: None });
                                }
                            }
                            Err(h) => {
                                inv.held = Some(h);
                                continue;
                            }
                        }
                    }
                    None => {
                        let Some(placed) = page.item_at(*cx, *cy).cloned() else {
                            continue;
                        };
                        page.items.retain(|q| !(q.x == placed.x && q.y == placed.y));
                        inv.held_from = Some((
                            crate::inventory::HeldFrom::At(crate::inventory::ItemLoc::Stash {
                                transfer: *transfer,
                                page: tab,
                                x: placed.x,
                                y: placed.y,
                            }),
                            placed.item.clone(),
                        ));
                        inv.held = Some(placed.item);
                    }
                }
                inv.changed();
                done.write(Default::default());
            }
            StashRequest::Sort { transfer, page } => {
                let pages = stash.view_mut(*transfer);
                let i = (*page).min(pages.len().saturating_sub(1));
                let tags = data.tags();
                if let Some(page) = pages.get_mut(i)
                    && gd_sim::items::inventory::sort_sack(page, |it| {
                        crate::inventory::sort_key(&inv, &tags, it)
                    })
                {
                    done.write(Default::default());
                }
            }
            StashRequest::BuyPage { transfer } => {
                let cost = next_page_cost(&data.db, &stash, *transfer);
                let pages = stash.view_mut(*transfer);
                if let Some(cost) = cost {
                    if progress.gold >= cost {
                        progress.gold -= cost;
                        pages.push(Sack::new(STASH_W, STASH_H, true));
                        info!("stash: bought page {} for {cost}", pages.len());
                        done.write(crate::trade::TradeDone {
                            page: Some(pages.len() - 1),
                        });
                    } else {
                        info!("stash: a new page costs {cost}");
                        refused.write(crate::trade::TradeRefused {
                            tag: "tagTransferError01",
                            arg: Some(cost),
                        });
                    }
                }
            }
            StashRequest::Material { record, shift } => {
                let have = stash
                    .materials
                    .entries
                    .iter()
                    .find(|e| e.record.eq_ignore_ascii_case(record))
                    .map_or(0, |e| e.count);
                match inv.held.take() {
                    // Store a held stack of this material (not into a store the save skips: module doc).
                    Some(h) if h.base_name.eq_ignore_ascii_case(record) && !reagents_open => {
                        inv.held = Some(h);
                        error.write(QuickActionError { notify: true });
                        continue;
                    }
                    Some(h) if h.base_name.eq_ignore_ascii_case(record) => {
                        if let Some(pak) = ui_drop_sound(&data.db, &h) {
                            sound.write(crate::visuals::PlaySound { pak, at: None });
                        }
                        store_material(&mut stash.materials, record, h.stack_count.max(1));
                    }
                    Some(h) => inv.held = Some(h),
                    // Take from the store.
                    None if have > 0 => {
                        let take = if *shift {
                            crate::stack::engine_max(&data.db).min(have)
                        } else {
                            1
                        };
                        let item = Item {
                            base_name: record.clone(),
                            seed: crate::rng::u32(1..0x7FFF_FFFF),
                            stack_count: take,
                            ..Default::default()
                        };
                        if *shift {
                            if !inv.add_to_bags(item) {
                                info!("trade: no room in the bags");
                                continue;
                            }
                        } else {
                            inv.held_from =
                                Some((crate::inventory::HeldFrom::Reagents, item.clone()));
                            inv.held = Some(item);
                        }
                        if let Some(e) = stash
                            .materials
                            .entries
                            .iter_mut()
                            .find(|e| e.record.eq_ignore_ascii_case(record))
                        {
                            e.count -= take;
                        }
                        stash.materials.entries.retain(|e| e.count > 0);
                    }
                    None => {}
                }
                inv.changed();
                done.write(Default::default());
            }
            StashRequest::TestTransfer => {
                if !transfer_open {
                    error.write(QuickActionError { notify: true });
                    continue;
                }
                if let Some(p) = stash
                    .pages
                    .iter_mut()
                    .find(|p| !p.items.is_empty())
                    .map(|p| p.items.remove(0))
                {
                    let size = inv
                        .info(&p.item)
                        .map(|i| i.size)
                        .unwrap_or(Footprint { w: 1, h: 1 });
                    let ok = stash.transfer[0].add(p.item.clone(), size);
                    if ok.is_none() {
                        // No room: back to the first page that takes it.
                        let _ = stash
                            .pages
                            .iter_mut()
                            .any(|pg| pg.add(p.item.clone(), size).is_some());
                    }
                    info!(
                        "transfer test: moved {} ({ok:?})",
                        p.item.base_name.rsplit('/').next().unwrap_or("")
                    );
                }
                done.write(Default::default());
            }
        }
    }
}
