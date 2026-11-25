//! The player's inventory and equipment for this session.
//!
//! Loaded from the save (or the starting gear), written back by `gd_game::save`. Grid rules:
//! `gd_formats::inventory`. The active weapon set is in `equipped`, the other in `other_weapons`. `sync_equipment`
//! copies the worn equipment to the player's `Loadout::items`; the game rebuilds the body on [`EquipmentChanged`].
//!
//! Components, augments and single-piece transmuters (`CreateSecondaryCursorHandler`): a right click
//! selects one (it stays in its cell), the next click ([`ApplyRequest::Click`]) on a bag, stash or worn item
//! uses it there ([`Inventory::apply_click`], rules [`Inventory::apply_check`]); dragging one onto an item only
//! swaps them. PORT: while the transfer stash is blocked, a component / augment from elsewhere is not used on a
//! transfer-stash item: `apply_requests` refuses the click and the mode stays.
//!
//! Detached items (`EquipmentCtrl::AttributesHaveChanged` / `Sift`, every 250 ms): a worn item whose
//! requirements fail (level and attributes with the gear's reductions, `gd_sim::items::req_reduction`) or an
//! off-hand one-hander whose dual-wield bit is gone stays in its slot (drawn) but gives no stats, skills, set
//! count or components (`sift_equipment`; the save's `attached` flag).
//!
//! Giving an item ([`Inventory::give`], `ControllerPlayer::GiveItemToPlayer`):
//! 1. [`Inventory::auto_insert`]: an EMPTY slot of its class with requirements met, in [`AUTO_INSERT_ORDER`],
//!    then the ACTIVE set's main hand, else off hand (strict hand rule). Nothing is swapped.
//! 2. [`Inventory::add_to_bags`]: OneShot use, (ground pickups) an incomplete component joins a partial, the
//!    stacks of its record, then a new cell: main bag, selected bag ([`Inventory::bag_tab`]), the others.
//!
//! The hand rule (`WeaponSet__PlaceRight/Left`; table `gd_formats::inventory::hand_fits`): melee +
//! melee and pistol + pistol only with dual wielding ([`Requirements::dual`]), melee + pistol never, a shield or
//! focus with any one-hander, a two-hander alone. Manual equip ([`Inventory::equip`]) is the non-strict test:
//! the occupied hand swaps onto the cursor; a two-hander sends the off hand to the bags; an off-hand item next
//! to a two-hander sends the two-hander there (no room: refused).
//!
//! Right click on a bag item ([`InvRequest::Bag`]): the item's secondary handler (apply mode, blueprint learn,
//! `crate::consumables` use), else smart insert ([`Inventory::smart_slot`] / [`Inventory::smart_insert`]): a
//! single slot replaces its item; a ring goes to ring 1 only when both are empty, else ring 2; a weapon to the
//! main hand unless an empty off hand may take it while the main hand is full. Displaced items go to the bags,
//! else the cursor; a two-hander over a pair: both to the bags, else the one that fits, else nothing changes.
//! With an item held, a right press on a bag is the merchant quick sale (`gd_game::trade::SellHeld`) else
//! [`Inventory::cancel_held`]. Right press on a worn item with a merchant open sells it
//! (`gd_game::trade::SellEquipped`).
//!
//! Item-slot windows give items back through [`InvRequest::Stow`].
use crate::stash::Stash;
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_formats::inventory::{
    EquipSlot, Footprint, Sack, bitmap_footprint, hand_types, is_two_handed_type, shard_target,
    slots_for_class, sort_rank,
};
use gd_formats::save::item::Item;
use gd_sim::items::{ItemDb, ResolvedItem, resolve};
use std::sync::Arc;

/// The inventory's systems: the game's inventory input runs between `init_inventory` and `sync_equipment`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InventorySet;

/// The UI's answer in the apply mode (`Inventory::applying`).
#[derive(Message, Clone, Copy, Debug)]
pub enum ApplyRequest {
    /// Ends the mode: Escape without a question up, the character or the stash window closing.
    Cancel,
    /// Yes / No to the confirmation (`Inventory::answer_apply`).
    Answer(bool),
    /// Right click on an item: selects it when it is a component / augment.
    Start(ItemLoc),
    /// Left or right click on `target` (None: an empty equipment slot); both buttons go to an active handler
    /// (`BagWidget_HandleMouseEvent`).
    Click(Option<ItemLoc>),
}

/// An item's (item-type rank, display name) for the sort (`gd_formats::inventory::sort_sack`); an unknown
/// record ranks 0 with no name.
pub fn sort_key(inv: &Inventory, tags: &gd_formats::text::TagTable, item: &Item) -> (u32, String) {
    let one = Item {
        stack_count: 1,
        ..item.clone()
    };
    inv.info(&one).map_or((0, String::new()), |i| {
        (
            sort_rank(&i.resolved.class),
            gd_sim::items::naming::item_name(tags, &i.resolved),
        )
    })
}

/// Auto-sort of bag `b` (`InventorySack::Sort`). Each press starts one layout further than the last
/// ([`Inventory::sort_presses`]), cycling the four layouts.
#[derive(Message, Clone, Copy, Debug)]
pub struct SortBag(pub usize);

/// A click in the character window, resolved by the UI to a bag cell / equipment slot. The cursor item is
/// state ([`Inventory::held`]); the UI only asks for the transitions.
#[derive(Message, Clone, Debug)]
pub enum InvRequest {
    /// The window closed: ends the apply mode and puts the held item back in a bag (stays in hand when full).
    Closed,
    /// The extra-bag tab button.
    BagTab(usize),
    /// `cursor` is in cells from the grid's top-left (the drop footprint is centred on it). Left: pick up /
    /// place / swap. Right: select a component, learn a blueprint, use a consumable or equip the item.
    Bag {
        bag: usize,
        cell: (u32, u32),
        cursor: Vec2,
        right: bool,
    },
    /// Not in the apply mode: left swaps with the held item; right acts as left with one held.
    Slot { slot: EquipSlot, right: bool },
    /// A held item clicked on the paper doll.
    Doll,
    /// An EMPTY item-slot window slot takes the bag item at `cell` of `bag` (Shift + left
    /// click); answered with a `crafting::SlotReturn`.
    ToWindowSlot { bag: usize, cell: (u32, u32) },
    /// The window's slot gives `item` back to the player (`ItemBox_ReturnItemToPlayer`).
    Stow(Item),
    /// The cursor item is exchanged with the slot's item (None: empty); the old cursor item comes back as a
    /// `crafting::SlotReturn`.
    Exchange(Option<Item>),
}

/// The slots a held item clicked on the paper doll tries, in order (`CharacterWindowScene::HandleItemClick`,
/// `ClickWeaponPair`): a ring the first ring slot when empty, else the second; a weapon main hand then off hand
/// (a shield off hand first); others their class's first slot.
pub fn doll_slots(inv: &Inventory, item: &Item) -> Vec<EquipSlot> {
    let Some(class) = inv.info(item).map(|i| i.resolved.class.clone()) else {
        return Vec::new();
    };
    match class.as_str() {
        "ArmorJewelry_Ring" => vec![if inv.equipped[slot_index(EquipSlot::Ring1)].is_none() {
            EquipSlot::Ring1
        } else {
            EquipSlot::Ring2
        }],
        "WeaponArmor_Shield" => vec![EquipSlot::OffHand, EquipSlot::MainHand],
        c if c.starts_with("Weapon") => vec![EquipSlot::MainHand, EquipSlot::OffHand],
        c => gd_formats::inventory::slots_for_class(c)
            .first()
            .copied()
            .into_iter()
            .collect(),
    }
}

#[derive(Debug)]
pub enum SmartInsert {
    /// Worn; what it displaced went to the bags or onto the cursor.
    Equipped,
    /// Not wearable now ([`Inventory::smart_slot`] None): handed back, nothing changed, no sound.
    Refused(Item),
    /// A two-hander whose two displaced items found no room: handed back, nothing changed, error sound.
    NoRoom(Item),
}

/// The player's worn equipment changed (its loadout has the new items).
#[derive(Message, Clone, Debug)]
pub struct EquipmentChanged {
    pub player: Entity,
}

pub const SLOTS: [EquipSlot; 14] = [
    EquipSlot::Head,
    EquipSlot::Amulet,
    EquipSlot::Chest,
    EquipSlot::Legs,
    EquipSlot::Feet,
    EquipSlot::Hands,
    EquipSlot::Ring1,
    EquipSlot::Ring2,
    EquipSlot::Waist,
    EquipSlot::Shoulders,
    EquipSlot::Medal,
    EquipSlot::Relic,
    EquipSlot::MainHand,
    EquipSlot::OffHand,
];

pub fn slot_index(s: EquipSlot) -> usize {
    SLOTS.iter().position(|x| *x == s).unwrap()
}

/// The sack (bag or stash page) and cell of a non-equipment location.
pub fn sack_cell<'a>(
    bags: &'a mut [Sack<Item>],
    stash: Option<&'a mut Stash>,
    loc: ItemLoc,
) -> Option<(&'a mut Sack<Item>, u32, u32)> {
    match loc {
        ItemLoc::Bag { bag, x, y } => Some((bags.get_mut(bag)?, x, y)),
        ItemLoc::Stash {
            transfer,
            page,
            x,
            y,
        } => Some((stash?.view_mut(transfer).get_mut(page)?, x, y)),
        ItemLoc::Equip(_) => None,
    }
}

pub struct ItemInfo {
    pub resolved: ResolvedItem,
    pub size: Footprint,
}

/// Where the held item came from ([`Inventory::cancel_held`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldFrom {
    At(ItemLoc),
    /// The caravan's reagent storage.
    Reagents,
}

/// Where an item sits: a bag cell (its top-left), a stash / transfer page
/// cell or an equipment slot index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemLoc {
    Bag {
        bag: usize,
        x: u32,
        y: u32,
    },
    Stash {
        transfer: bool,
        page: usize,
        x: u32,
        y: u32,
    },
    Equip(usize),
}

/// The component / augment in apply mode (its bag or stash cell) and the
/// pending confirmation (target, text tag).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applying {
    pub src: ItemLoc,
    pub confirm: Option<(ItemLoc, &'static str)>,
}

#[derive(Resource)]
pub struct Inventory {
    /// Worn items by [`SLOTS`]; the weapons are the active weapon set's.
    pub equipped: [Option<Item>; 14],
    /// The inactive weapon set's main / off hand.
    pub other_weapons: [Option<Item>; 2],
    /// Weapon set II is active (`EquipmentCtrl.alternate`).
    pub alternate: bool,
    /// The requested set (`ControllerCharacter.requestedAlternate`); `weapon_sets::apply_swap` makes it current.
    pub requested_alternate: bool,
    /// Set II was chosen once (`Player.alternateConfigEnabled`): the HUD swap button works.
    pub alternate_enabled: bool,
    /// The main bag, then the extra bags (one per save sack).
    pub bags: Vec<Sack<Item>>,
    /// The extra bag shown beside the main one (index into `bags`, ≥ 1; unused with no extra bag).
    pub bag_tab: usize,
    /// Auto-sort presses of the main / extra bags this session (`CharacterWindow.sortCountPrimary` /
    /// `sortCountSecondary`, not saved; one count for every extra bag).
    pub sort_presses: [u32; 2],
    pub held: Option<Item>,
    /// Where the held item was picked up, with the item: valid only while that item is still held.
    pub held_from: Option<(HeldFrom, Item)>,
    /// Save bag items that found no cell on load: written back at their saved cell, so a later load tries again.
    pub unplaced: Vec<Unplaced>,
    /// The held item's destroy question is up (its text tag); answered with `item_drop::DestroyAnswer`.
    pub destroying: Option<&'static str>,
    /// The tonic / writ / bounty a right click chose; its question is up once
    /// [`crate::consumables::UseAsk::question`] is set.
    pub use_ask: Option<crate::consumables::UseAsk>,
    /// The apply mode: a component / augment used on the next item clicked.
    pub applying: Option<Applying>,
    /// UI sounds of inventory actions.
    pub sounds: Vec<String>,
    /// Bumped on every change.
    pub version: u64,
    /// The worn equipment changed; `sync_equipment` passes it to the player's loadout.
    pub equipment_dirty: bool,
    /// The worn item each slot holds while detached; another item in the slot is attached.
    pub detached: [Option<Item>; 14],
    /// Consumables used this frame, one each: the item and its bag cell (None: hotbar / hook / food use,
    /// [`Self::return_used`]).
    pub used: Vec<(Item, Option<(usize, u32, u32)>)>,
    /// Ground pickups not yet shown, by (record, seed): they glow in the bag grid until hovered.
    pub fresh: Vec<(String, u32)>,
    db: ItemDb,
    /// Interior-mutable so [`Self::info`] reads through `&self` without ticking change detection.
    info: parking_lot::Mutex<HashMap<Item, Option<Arc<ItemInfo>>>>,
    vfs: Arc<gd_formats::vfs::Vfs>,
}

/// A saved item that fits nowhere in its grids ([`Inventory::unplaced`], `Stash::unplaced`).
#[derive(Clone, Debug)]
pub struct Unplaced {
    pub sack: usize,
    pub x: u32,
    pub y: u32,
    pub item: Item,
}

/// Puts a save's items into `sacks`: each at its saved cell first, then the colliding ones at the first free
/// cell of their own sack, else of the others; returns those no sack holds. PORT: the engine's load of a
/// colliding item is not traced.
pub fn place_saved(
    inv: &mut Inventory,
    what: &str,
    sacks: &mut [Sack<Item>],
    saved: impl Iterator<Item = Unplaced>,
) -> Vec<Unplaced> {
    let mut moved = Vec::new();
    for u in saved {
        let size = inv
            .info(&u.item)
            .map_or(Footprint { w: 1, h: 1 }, |i| i.size);
        let at_cell = sacks
            .get_mut(u.sack)
            .is_some_and(|s| s.add_at(u.item.clone(), u.x, u.y, size));
        if !at_cell {
            moved.push((u, size));
        }
    }
    let mut unplaced = Vec::new();
    for (u, size) in moved {
        let n = sacks.len();
        let order = std::iter::once(u.sack)
            .filter(|&b| b < n)
            .chain((0..n).filter(|&b| b != u.sack));
        let mut to = None;
        for b in order {
            if let Some(cell) = sacks[b].add(u.item.clone(), size) {
                to = Some((b, cell));
                break;
            }
        }
        match to {
            Some((b, (x, y))) => warn!(
                "{what}: {} does not fit its saved cell (sack {}, {},{}): moved to sack {b}, {x},{y}",
                u.item.base_name, u.sack, u.x, u.y
            ),
            None => {
                warn!(
                    "{what}: {} does not fit its saved cell (sack {}, {},{}) and nothing has room: kept out of \
                     the grid, saved back at that cell",
                    u.item.base_name, u.sack, u.x, u.y
                );
                unplaced.push(u);
            }
        }
    }
    unplaced
}

/// `PlayerInventoryCtrl::AddSack`: at most 6 sacks, the main one included.
pub const MAX_SACKS: usize = 6;

/// Entries [`Inventory::info`] keeps before it starts over.
const INFO_CACHE_MAX: usize = 4096;

/// The cursor passed over a glowing new item: its glow ends.
#[derive(Message, Clone, Debug)]
pub struct NewItemSeen(pub (String, u32));

impl Inventory {
    /// A new character's inventory: the main bag, no equipment.
    pub fn new(db: ItemDb, vfs: Arc<gd_formats::vfs::Vfs>, version: u64) -> Self {
        Self {
            equipped: Default::default(),
            other_weapons: Default::default(),
            alternate: false,
            requested_alternate: false,
            alternate_enabled: false,
            // Main bag 12×8, extra bags 8×8 (`UICharWindowInventorySack*Dims` / 32).
            bags: vec![Sack::new(12, 8, true)],
            bag_tab: 1,
            sort_presses: [0; 2],
            held: None,
            held_from: None,
            destroying: None,
            unplaced: Vec::new(),
            use_ask: None,
            applying: None,
            sounds: Vec::new(),
            version,
            equipment_dirty: false,
            detached: Default::default(),
            used: Vec::new(),
            fresh: Vec::new(),
            db,
            info: Default::default(),
            vfs,
        }
    }

    /// Marks a ground pickup ([`Inventory::fresh`]).
    pub fn mark_fresh(&mut self, item: &Item) {
        let key = (item.base_name.clone(), item.seed);
        if !self.fresh.contains(&key) {
            self.fresh.push(key);
        }
    }

    /// The item's resolved data and footprint, cached; cleared at [`INFO_CACHE_MAX`] entries. The lock is held
    /// only for the lookup and the insert, so parallel resolves never wait on each other.
    pub fn info(&self, item: &Item) -> Option<Arc<ItemInfo>> {
        if let Some(i) = self.info.lock().get(item) {
            return i.clone();
        }
        let r = resolve(&self.db, item).map(|resolved| {
            let size =
                bitmap_footprint(&self.vfs, &resolved.bitmap).unwrap_or(Footprint { w: 1, h: 1 });
            Arc::new(ItemInfo { resolved, size })
        });
        let mut cache = self.info.lock();
        if cache.len() >= INFO_CACHE_MAX {
            cache.clear();
        }
        cache.entry(item.clone()).or_insert(r).clone()
    }

    /// The item's footprint (1×1 for an unknown record), cached like [`Self::info`].
    pub fn size_of(&self, item: &Item) -> Footprint {
        self.info(item).map_or(Footprint { w: 1, h: 1 }, |i| i.size)
    }

    pub fn db(&self) -> &ItemDb {
        &self.db
    }

    /// The worn equipment was changed directly (not by [`Self::equip`]).
    pub fn mark_equipment_dirty(&mut self) {
        self.equipment_dirty = true;
    }

    pub fn changed(&mut self) {
        self.version += 1;
    }

    /// `CursorHandlerItemMove::Cancel`: the held item goes back to its bag or stash cell ([`Inventory::held_from`])
    /// when that cell has no conflict (`InventorySack::GetConflicts`); an item not `CanBePlacedInTransferStash`
    /// from the transfer stash goes to the bags (else stays held); a reagent goes back to the reagent storage.
    /// Otherwise `Err` for `ControllerPlayer::GiveItemToPlayer(id, false)`. `stash`: None before it is loaded.
    /// PORT: the stash sack is the one it came from; the engine's is the selected one.
    #[allow(clippy::result_large_err)]
    pub fn cancel_held(&mut self, stash: Option<&mut crate::stash::Stash>) -> Result<(), Item> {
        let Some(item) = self.held.take() else {
            return Ok(());
        };
        let from = self.held_from.take();
        self.changed();
        let Some((from, orig)) = from else {
            return Err(item);
        };
        if orig != item {
            return Err(item);
        }
        let size = self
            .info(&item)
            .map(|i| i.size)
            .unwrap_or(Footprint { w: 1, h: 1 });
        let sack = match (from, stash) {
            (HeldFrom::At(ItemLoc::Bag { bag, x, y }), _) => {
                self.bags.get_mut(bag).map(|s| (s, x, y))
            }
            (HeldFrom::Reagents, Some(stash)) => {
                crate::stash::store_material(
                    &mut stash.materials,
                    &item.base_name,
                    item.stack_count.max(1),
                );
                return Ok(());
            }
            (
                HeldFrom::At(ItemLoc::Stash {
                    transfer,
                    page,
                    x,
                    y,
                }),
                Some(stash),
            ) => {
                if transfer
                    && self.db.get(&item.base_name).is_some_and(|r| {
                        r.bool("soulbound") || r.i32("craftingMaterial").unwrap_or(0) != 0
                    })
                {
                    if !self.add_to_bags(item.clone()) {
                        self.held = Some(item);
                    }
                    return Ok(());
                }
                stash.view_mut(transfer).get_mut(page).map(|s| (s, x, y))
            }
            _ => None,
        };
        if let Some((sack, x, y)) = sack
            && sack.is_free(x, y, size)
        {
            sack.add_at(item, x, y, size);
            return Ok(());
        }
        Err(item)
    }

    /// `PlayerInventoryCtrl::AddItem` without the shard join; false when all bags are full.
    pub fn add_to_bags(&mut self, item: Item) -> bool {
        // `AddItem` runs `OneShotFilter` first: `OneShot_Sack` → `AddSack` (max 6), then the item is used or
        // removed; `OneShot_Food` is used at once.
        let (food, sack) = self.db.get(&item.base_name).map_or((false, false), |r| {
            (r.class == "OneShot_Food", r.class == "OneShot_Sack")
        });
        if food {
            self.used.push((item, None));
            return true;
        }
        if sack {
            if self.bags.len() < MAX_SACKS {
                self.bags.push(gd_formats::inventory::Sack::new(8, 8, true));
                info!("new bag ({} bags)", self.bags.len());
                self.changed();
            } else {
                info!("no room for another bag: the sack is lost");
            }
            return true;
        }
        let Some(size) = self.info(&item).map(|i| i.size) else {
            return false;
        };
        // Onto the stacks of its record first; the bags change only when all of it fits (a dry run first).
        let engine = crate::stack::engine_max_in(self.db.get(crate::stack::GAME_ITEM_INFO));
        let max = crate::stack::max_of(self, &item, engine);
        let room: u64 = if max > 1 {
            self.bags
                .iter()
                .flat_map(|s| &s.items)
                .filter(|q| q.item.base_name.eq_ignore_ascii_case(&item.base_name))
                .map(|q| max.saturating_sub(crate::stack::count(&q.item)) as u64)
                .sum()
        } else {
            0
        };
        let cell = if room >= crate::stack::count(&item) as u64 {
            None
        } else {
            let sel = self.bag_tab;
            let n = self.bags.len();
            let mut order = std::iter::once(0)
                .chain((sel > 0 && sel < n).then_some(sel))
                .chain((1..n).filter(move |&b| b != sel));
            let Some(found) =
                order.find_map(|b| self.bags[b].find_position(size).map(|at| (b, at)))
            else {
                return false;
            };
            Some(found)
        };
        if let Some(rest) = crate::stack::merge_into(&mut self.bags, item, max)
            && let Some((b, (x, y))) = cell
        {
            self.bags[b].add_at(rest, x, y, size);
        }
        self.changed();
        true
    }

    /// Puts a refused consumable back at its cell (onto its stack, else the empty cell), else first fit
    /// ([`Self::add_to_bags`]), else an empty cursor. `Err`: nowhere to go; a held item is never overwritten.
    #[allow(clippy::result_large_err)]
    pub fn return_used(&mut self, item: Item, from: Option<(usize, u32, u32)>) -> Result<(), Item> {
        if let Some((bag, x, y)) = from {
            let size = self
                .info(&item)
                .map(|i| i.size)
                .unwrap_or(Footprint { w: 1, h: 1 });
            let placed = self.bags.get_mut(bag).is_some_and(|sack| {
                if let Some(q) = sack
                    .items
                    .iter_mut()
                    .find(|q| q.x == x && q.y == y && q.item.base_name == item.base_name)
                {
                    q.item.stack_count += item.stack_count.max(1);
                    true
                } else {
                    sack.is_free(x, y, size) && sack.add_at(item.clone(), x, y, size)
                }
            });
            if placed {
                self.changed();
                return Ok(());
            }
        }
        if self.add_to_bags(item.clone()) {
            return Ok(());
        }
        if self.held.is_some() {
            return Err(item);
        }
        self.held = Some(item);
        self.changed();
        Ok(())
    }

    /// `ControllerPlayer::GiveItemToPlayer`: [`Self::auto_insert`], else the bags (`combine`: an incomplete
    /// component first joins a partial, [`shard_target`]; only `CharPickUpConfigCmd` asks for it). `Err` hands
    /// the item back.
    #[allow(clippy::result_large_err)]
    pub fn give(&mut self, item: Item, combine: bool, req: &Requirements) -> Result<(), Item> {
        let item = match self.auto_insert(item, req) {
            Ok(slot) => {
                info!("{slot:?} takes the new item");
                return Ok(());
            }
            Err(item) => item,
        };
        if combine && self.join_shards(&item) || self.add_to_bags(item.clone()) {
            Ok(())
        } else {
            Err(item)
        }
    }

    /// `CharPickUpConfigCmd::Execute` / `PickUpAction::Execute`: a Broken item and a `cannotPickUpMultiple`
    /// record the player already has ([`Self::has_record`]) are not picked up.
    pub fn refuses_pickup(&self, item: &Item) -> bool {
        let Some(rec) = self.db.get(&item.base_name) else {
            return false;
        };
        gd_sim::items::Classification::parse(rec.str("itemClassification").unwrap_or(""))
            == gd_sim::items::Classification::Broken
            || rec.bool("cannotPickUpMultiple") && self.has_record(&item.base_name)
    }

    /// `PlayerInventoryCtrl::AddItem` / `ItemRelic::UseRelicOn`: an incomplete `ItemRelic` / `ItemCharm` joins
    /// a partial ([`shard_target`]) and is gone.
    fn join_shards(&mut self, item: &Item) -> bool {
        let Some(rec) = self.db.get(&item.base_name) else {
            return false;
        };
        if !matches!(rec.class.as_str(), "ItemRelic" | "ItemCharm") {
            return false;
        }
        let need = gd_sim::items::components::shards_needed(rec);
        let Some((b, i)) = shard_target(&self.bags, item, need) else {
            return false;
        };
        let q = &mut self.bags[b].items[i].item;
        q.var1 = q.var1.max(1) + item.var1.max(1);
        let field = if q.var1 >= need {
            "relicCompleteSound"
        } else {
            "relicToRelicSound"
        };
        debug!("{} joins a partial ({} / {need})", item.base_name, q.var1);
        self.sounds
            .extend(rec.str(field).filter(|s| !s.is_empty()).map(str::to_owned));
        self.changed();
        true
    }

    /// How many bag items of base `record` there are (stacks counted).
    pub fn count_by_record(&self, record: &str) -> u32 {
        self.bags
            .iter()
            .flat_map(|b| &b.items)
            .filter(|p| p.item.base_name.eq_ignore_ascii_case(record))
            .map(|p| p.item.stack_count.max(1))
            .sum()
    }

    /// `Character::HasItem(name)`: an item whose base record equals `record` ignoring case and slash direction.
    /// PORT: the bags and both weapon sets' slots (not the held item).
    pub fn has_record(&self, record: &str) -> bool {
        let norm = |s: &str| s.replace('\\', "/").to_ascii_lowercase();
        let want = norm(record);
        let is = |i: &Item| norm(&i.base_name) == want;
        self.bags.iter().flat_map(|b| &b.items).any(|p| is(&p.item))
            || self.equipped.iter().flatten().any(is)
            || self.other_weapons.iter().flatten().any(is)
    }

    /// Removes up to `count` bag items of base `record` (stacks count
    /// down first); returns how many were removed.
    pub fn remove_by_record(&mut self, record: &str, count: u32) -> u32 {
        let mut left = count;
        for b in &mut self.bags {
            b.items.retain_mut(|p| {
                if left == 0 || !p.item.base_name.eq_ignore_ascii_case(record) {
                    return true;
                }
                let n = p.item.stack_count.max(1).min(left);
                left -= n;
                p.item.stack_count = p.item.stack_count.max(1) - n;
                p.item.stack_count > 0
            });
        }
        if left < count {
            self.changed();
        }
        count - left
    }

    /// The attached worn items (armour slots, then the weapons).
    pub fn worn(&self) -> Vec<Item> {
        (0..self.equipped.len())
            .filter(|&i| self.is_attached(i))
            .filter_map(|i| self.equipped[i].clone())
            .collect()
    }

    pub fn is_attached(&self, i: usize) -> bool {
        self.equipped[i].is_some() && self.detached[i] != self.equipped[i]
    }

    /// The worn gear for `compare_dps` with `candidate` in the main hand (`None`: as worn); a two-hander clears
    /// the off hand (`Player::CompareItems`).
    pub fn weapon_gear(&self, candidate: Option<&Item>) -> crate::sheet_dps::Gear {
        let (main, off) = (
            slot_index(EquipSlot::MainHand),
            slot_index(EquipSlot::OffHand),
        );
        let attached = |i: usize| {
            self.is_attached(i)
                .then(|| self.equipped[i].clone())
                .flatten()
        };
        let class = |it: &Item| gd_sim::items::resolve(&self.db, it).map(|r| r.class);
        let mut items: Vec<Item> = (0..main).filter_map(attached).collect();
        let new_main = candidate.cloned().or_else(|| attached(main));
        let two_handed = candidate
            .and_then(class)
            .is_some_and(|c| is_two_handed_type(hand_types(&c).0));
        let new_off = if two_handed { None } else { attached(off) };
        let wtype = |it: &Option<Item>| {
            it.as_ref()
                .and_then(class)
                .and_then(|c| gd_sim::equip::WeaponType::from_class(&c))
        };
        let hands = (wtype(&new_main), wtype(&new_off));
        items.extend(new_main.into_iter().chain(new_off));
        crate::sheet_dps::Gear { items, hands }
    }

    /// The worn gear with `candidate` in the off hand, when it may go there and the worn off hand is a one-handed
    /// weapon (`Player::CompareItems`).
    pub fn weapon_gear_offhand(&self, candidate: &Item) -> Option<crate::sheet_dps::Gear> {
        let (main, off) = (
            slot_index(EquipSlot::MainHand),
            slot_index(EquipSlot::OffHand),
        );
        let class = |it: &Item| gd_sim::items::resolve(&self.db, it).map(|r| r.class);
        let worn_off = self
            .is_attached(off)
            .then(|| self.equipped[off].clone())
            .flatten()?;
        let worn_class = class(&worn_off)?;
        if !worn_class.starts_with("Weapon") || worn_class.starts_with("WeaponArmor") {
            return None;
        }
        let cand_class = class(candidate)?;
        if slots_for_class(&cand_class) == [EquipSlot::MainHand]
            || !slots_for_class(&cand_class).contains(&EquipSlot::OffHand)
        {
            return None;
        }
        let attached = |i: usize| {
            self.is_attached(i)
                .then(|| self.equipped[i].clone())
                .flatten()
        };
        let mut items: Vec<Item> = (0..main).filter_map(attached).collect();
        let new_main = attached(main);
        let wtype = |c: &str| gd_sim::equip::WeaponType::from_class(c);
        let hands = (
            new_main.as_ref().and_then(class).and_then(|c| wtype(&c)),
            wtype(&cand_class),
        );
        items.extend(new_main.into_iter().chain(Some(candidate.clone())));
        Some(crate::sheet_dps::Gear { items, hands })
    }

    fn is_two_handed(&self, item: &Item) -> bool {
        is_two_handed_type(self.hand_type(item, EquipSlot::MainHand))
    }

    /// Right click on a bag / stash item: a component, augment or single-piece transmuter enters the apply mode
    /// with `relicCursorActivationSound`; false for other items.
    pub fn start_apply(&mut self, src: ItemLoc, stash: Option<&Stash>) -> bool {
        if matches!(src, ItemLoc::Equip(_)) {
            return false;
        }
        let Some(item) = self.item_in(src, stash) else {
            return false;
        };
        if !self.db.get(&item.base_name).is_some_and(|r| {
            matches!(
                r.class.as_str(),
                "ItemRelic" | "ItemEnchantment" | "ItemTransmuter"
            )
        }) {
            return false;
        }
        debug!("apply mode: {}", item.base_name);
        self.applying = Some(Applying { src, confirm: None });
        self.changed();
        let snd = self
            .db
            .get("records/ingameui/gameengine.dbr")
            .and_then(|r| r.str("relicCursorActivationSound").map(str::to_owned));
        self.sounds.extend(snd);
        true
    }

    /// The component / augment of the apply mode, if it is still in its cell.
    pub fn applying_item<'a>(&'a self, stash: Option<&'a Stash>) -> Option<&'a Item> {
        self.item_in(self.applying.as_ref()?.src, stash)
    }

    /// The item at `loc` (a stash cell needs the stash).
    pub fn item_in<'a>(&'a self, loc: ItemLoc, stash: Option<&'a Stash>) -> Option<&'a Item> {
        match loc {
            ItemLoc::Bag { bag, x, y } => self.bags.get(bag)?.item_at(x, y).map(|q| &q.item),
            ItemLoc::Stash {
                transfer,
                page,
                x,
                y,
            } => stash?
                .view(transfer)
                .get(page)?
                .item_at(x, y)
                .map(|q| &q.item),
            ItemLoc::Equip(i) => self.equipped.get(i)?.as_ref(),
        }
    }

    /// The item at `loc` (bags and equipment only).
    pub fn item_at(&self, loc: ItemLoc) -> Option<&Item> {
        self.item_in(loc, None)
    }

    /// Ends the apply mode (`CursorHandler::Cancel`): Esc, an answered confirmation, a use, the inventory or
    /// caravan window closing (`cancelOnClose`), or a press outside every window (`CursorHandlerMgr::HandleMouseEvent`
    /// → `ActivateWorld`).
    pub fn cancel_apply(&mut self) {
        if self.applying.take().is_some() {
            self.changed();
        }
    }

    /// Every item the apply mode's component can be used on, by the click's rule ([`Self::apply_check`],
    /// [`Self::apply_blocked`]). `transfer_open`: `stash::SharedFiles::accepts` of the transfer stash.
    pub fn apply_targets(
        &self,
        stash: Option<&Stash>,
        req: &Requirements,
        transfer_open: bool,
    ) -> Vec<ItemLoc> {
        let Some(src) = self.applying.as_ref().map(|a| a.src) else {
            return Vec::new();
        };
        let Some(comp) = self.applying_item(stash) else {
            return Vec::new();
        };
        let mut locs: Vec<(ItemLoc, &Item)> = Vec::new();
        for (b, bag) in self.bags.iter().enumerate() {
            locs.extend(bag.items.iter().map(|q| {
                (
                    ItemLoc::Bag {
                        bag: b,
                        x: q.x,
                        y: q.y,
                    },
                    &q.item,
                )
            }));
        }
        for transfer in [false, true] {
            for (page, sack) in stash
                .iter()
                .flat_map(|s| s.view(transfer).iter().enumerate())
            {
                locs.extend(sack.items.iter().map(|q| {
                    (
                        ItemLoc::Stash {
                            transfer,
                            page,
                            x: q.x,
                            y: q.y,
                        },
                        &q.item,
                    )
                }));
            }
        }
        locs.extend(
            self.equipped
                .iter()
                .enumerate()
                .filter_map(|(i, e)| e.as_ref().map(|it| (ItemLoc::Equip(i), it))),
        );
        locs.into_iter()
            .filter(|(l, it)| {
                *l != src
                    && !self.refused_at(comp, *l)
                    && !self.apply_blocked(Some(*l), transfer_open)
                    && self.apply_check(comp, it, req).is_some()
            })
            .map(|(l, _)| l)
            .collect()
    }

    /// A component from outside a blocked transfer stash onto an item in it: refused. One already in it may still
    /// be used there.
    pub fn apply_blocked(&self, target: Option<ItemLoc>, transfer_open: bool) -> bool {
        let in_transfer =
            |l: Option<ItemLoc>| matches!(l, Some(ItemLoc::Stash { transfer: true, .. }));
        !transfer_open && in_transfer(target) && !in_transfer(self.applying.as_ref().map(|a| a.src))
    }

    /// Whether `comp` can be used on `target`; `Some` holds the confirmation text tag, if any.
    /// `ItemRelic::CanRelicBeUsedOn`: a partial onto a partial of the same record, unconfirmed; onto equipment
    /// only with the level requirement met and no component socketed, `tagComponentConfirmation`.
    /// `ItemEnchantment::CanBeUsedOn`: equipment + `ItemUseFilter`, always confirmed (`tagEnchantment*` by the
    /// soulbound states). PORT: soulbound is the base record's or augment's `soulbound`; the component's attribute
    /// requirements are not tested. Transmuters: [`crate::illusion::transmute_check`].
    pub fn apply_check(
        &self,
        comp: &Item,
        target: &Item,
        req: &Requirements,
    ) -> Option<Option<&'static str>> {
        use gd_sim::items::components;
        let db = &self.db;
        let crec = db.get(&comp.base_name)?;
        let soulbound =
            |db: &ItemDb, n: &str| !n.is_empty() && db.get(n).is_some_and(|r| r.bool("soulbound"));
        let bound = soulbound(db, &target.base_name) || soulbound(db, &target.augment_name);
        if crec.class == "ItemTransmuter" {
            return crate::illusion::transmute_check(db, crec, target, bound).map(Some);
        }
        if crec.class == "ItemEnchantment" {
            let class = self.info(target).map(|i| i.resolved.class.clone())?;
            if !components::class_flag(&class).is_some_and(|f| crec.bool(f)) {
                return None;
            }
            let aug_bound = crec.bool("soulbound");
            return Some(Some(if target.augment_name.is_empty() {
                if aug_bound && !bound {
                    "tagEnchantmentSoulboundConfirmation"
                } else {
                    "tagEnchantmentConfirmation"
                }
            } else if aug_bound {
                if bound {
                    "tagEnchantmentOverwrite"
                } else {
                    "tagEnchantmentSoulboundOverwrite"
                }
            } else if soulbound(db, &target.augment_name) {
                "tagEnchantmentSoulboundRemoval"
            } else {
                "tagEnchantmentOverwrite"
            }));
        }
        if crec.class != "ItemRelic" {
            return None;
        }
        // A partial onto a partial of the same record.
        if target.base_name.eq_ignore_ascii_case(&comp.base_name) {
            let ok = !components::is_complete(crec, comp.var1)
                && !components::is_complete(crec, target.var1)
                && comp.stack_count <= 1
                && target.stack_count <= 1;
            return ok.then_some(None);
        }
        if crec.i32("levelRequirement").unwrap_or(0) > req.level || !target.relic_name.is_empty() {
            return None;
        }
        let class = self.info(target).map(|i| i.resolved.class.clone())?;
        (components::is_complete(crec, comp.var1) && components::fits(crec, &class))
            .then_some(Some("tagComponentConfirmation"))
    }

    /// Refused on a transfer-stash item: a soulbound augment (`CursorHandlerEnchant::PrimaryTransferActivate`) and a
    /// transmuter not `CanBePlacedInTransferStash` (`CursorHandlerTransmute::PrimaryTransferActivate`).
    fn refused_at(&self, comp: &Item, at: ItemLoc) -> bool {
        matches!(at, ItemLoc::Stash { transfer: true, .. })
            && self
                .db
                .get(&comp.base_name)
                .is_some_and(|r| match r.class.as_str() {
                    "ItemEnchantment" => r.bool("soulbound"),
                    "ItemTransmuter" => {
                        r.bool("soulbound") || r.i32("craftingMaterial").unwrap_or(0) != 0
                    }
                    _ => false,
                })
    }

    /// A click on `target` (`None`: an empty cell) in the apply mode (`CursorHandlerRelicCombine` /
    /// `CursorHandlerEnchant::Primary{Inventory,Stash,Transfer}Activate`): no target, the component itself or a
    /// gone component ends the mode; an unusable target does nothing; else the confirmation is asked
    /// ([`Self::answer_apply`]) or the use happens at once.
    pub fn apply_click(
        &mut self,
        target: Option<ItemLoc>,
        stash: Option<&mut Stash>,
        req: &Requirements,
    ) {
        let Some(a) = self.applying.clone() else {
            return;
        };
        if a.confirm.is_some() {
            return;
        }
        let src = a.src;
        let resolved = target.and_then(|t| match t {
            // The item under a bag / page cell (any cell of its footprint).
            ItemLoc::Bag { bag, x, y } => self.bags.get(bag)?.item_at(x, y).map(|q| ItemLoc::Bag {
                bag,
                x: q.x,
                y: q.y,
            }),
            ItemLoc::Stash {
                transfer,
                page,
                x,
                y,
            } => stash
                .as_deref()?
                .view(transfer)
                .get(page)?
                .item_at(x, y)
                .map(|q| ItemLoc::Stash {
                    transfer,
                    page,
                    x: q.x,
                    y: q.y,
                }),
            e => self.item_at(e).map(|_| e),
        });
        let (Some(comp), Some(t)) = (
            self.applying_item(stash.as_deref()).cloned(),
            resolved.filter(|t| *t != src),
        ) else {
            self.cancel_apply();
            return;
        };
        let Some(target_item) = self.item_in(t, stash.as_deref()).cloned() else {
            return;
        };
        if self.refused_at(&comp, t) {
            return;
        }
        match self.apply_check(&comp, &target_item, req) {
            None => {}
            Some(Some(tag)) => {
                debug!(
                    "apply {} on {}: confirm {tag}",
                    comp.base_name, target_item.base_name
                );
                if let Some(a) = self.applying.as_mut() {
                    a.confirm = Some((t, tag));
                }
                self.changed();
            }
            Some(None) => self.apply_to(t, stash, req),
        }
    }

    /// The confirmation's answer: Yes uses the component, No ends the mode
    /// (`CursorHandlerEnchant::Update`, response 1 / 0).
    pub fn answer_apply(&mut self, yes: bool, stash: Option<&mut Stash>, req: &Requirements) {
        let Some((t, _)) = self.applying.as_ref().and_then(|a| a.confirm) else {
            return;
        };
        if yes {
            self.apply_to(t, stash, req);
        } else {
            self.cancel_apply();
        }
    }

    /// Uses the apply mode's item on `t` and ends the mode, queueing its sound. `ItemRelic::UseRelicOn`: onto a
    /// partial adds its shards (the component keeps the overflow), onto equipment `AddRelic`;
    /// `ItemEnchantment::UseOn` replaces an augment; one of a stack is used. `ItemTransmuter::UseOn`:
    /// [`crate::illusion::transmute_result`], no sound.
    fn apply_to(&mut self, t: ItemLoc, mut stash: Option<&mut Stash>, req: &Requirements) {
        use gd_sim::items::components;
        let Some(a) = self.applying.take() else {
            return;
        };
        self.changed();
        let (Some(comp), Some(mut target)) = (
            self.item_in(a.src, stash.as_deref()).cloned(),
            self.item_in(t, stash.as_deref()).cloned(),
        ) else {
            return;
        };
        if self.apply_check(&comp, &target, req).is_none() {
            return;
        }
        let Some(crec) = self.db.get(&comp.base_name) else {
            return;
        };
        let sound = |r: &gd_formats::arz::Record, f: &str| {
            r.str(f).filter(|s| !s.is_empty()).map(str::to_owned)
        };
        let mut left = None;
        let snd;
        if crec.class == "ItemTransmuter" {
            target.transmute_name = crate::illusion::transmute_result(crec);
            snd = None;
        } else if crec.class == "ItemEnchantment" {
            target.augment_name = comp.base_name.clone();
            target.augment_seed = comp.seed;
            snd = sound(crec, "useSound");
        } else if target.base_name.eq_ignore_ascii_case(&comp.base_name) {
            let need = components::shards_needed(crec);
            let total = target.var1.max(1) + comp.var1.max(1);
            target.var1 = total.min(need);
            left = (total > need).then_some(total - need);
            snd = sound(
                crec,
                if total >= need {
                    "relicCompleteSound"
                } else {
                    "relicToRelicSound"
                },
            );
        } else {
            target.relic_name = comp.base_name.clone();
            target.relic_seed = comp.seed;
            target.relic_bonus = String::new();
            snd = sound(crec, "relicToItemSound");
        }
        debug!("applied {} to {}", comp.base_name, target.base_name);
        if let ItemLoc::Equip(i) = t {
            // A detached item stays detached until the next sift re-checks it.
            if self.equipped[i].is_some() && self.detached[i] == self.equipped[i] {
                self.detached[i] = Some(target.clone());
            }
            self.equipped[i] = Some(target);
            self.mark_equipment_dirty();
        } else if let Some((sack, x, y)) = sack_cell(&mut self.bags, stash.as_deref_mut(), t)
            && let Some(q) = sack.items.iter_mut().find(|q| q.x == x && q.y == y)
        {
            q.item = target;
        }
        if let Some((sack, x, y)) = sack_cell(&mut self.bags, stash, a.src)
            && let Some(i) = sack.items.iter().position(|q| q.x == x && q.y == y)
        {
            let q = &mut sack.items[i].item;
            match left {
                Some(v) => q.var1 = v,
                None if q.stack_count > 1 => q.stack_count -= 1,
                None => {
                    sack.items.remove(i);
                }
            }
        }
        self.sounds.extend(snd);
    }

    /// Whether the player meets `item`'s requirements.
    pub fn usable(&self, item: &Item, req: &Requirements) -> bool {
        let Some(i) = self.info(item) else {
            return true;
        };
        let r = &i.resolved.requirements;
        let a = req.attributes;
        let pct = req.reduction.for_class(&i.resolved.class);
        let met = gd_sim::items::req_reduction::met;
        met(req.level as f32, r.level as f32, req.reduction.level())
            && met(a[0], r.physique as f32, pct[0])
            && met(a[1], r.cunning as f32, pct[1])
            && met(a[2], r.spirit as f32, pct[2])
    }

    /// Why the player cannot wear `item`, if they cannot.
    pub fn requirement_failure(&self, item: &Item, req: &Requirements) -> Option<String> {
        let i = self.info(item)?;
        let r = &i.resolved.requirements;
        let need = r.level;
        let met = gd_sim::items::req_reduction::met;
        if !met(req.level as f32, need as f32, req.reduction.level()) {
            return Some(format!("needs level {need}"));
        }
        let pct = req.reduction.for_class(&i.resolved.class);
        let a = req.attributes;
        for (n, (need, have, name)) in [
            (r.physique, a[0], "Physique"),
            (r.cunning, a[1], "Cunning"),
            (r.spirit, a[2], "Spirit"),
        ]
        .into_iter()
        .enumerate()
        {
            if !met(have, need as f32, pct[n]) {
                return Some(format!("needs {need} {name}"));
            }
        }
        None
    }

    /// Slots `item` may go into: its class's, the off hand only when the hand rule allows it against empty hands.
    pub fn allowed_slots(&self, item: &Item, req: &Requirements) -> Vec<EquipSlot> {
        let Some(info) = self.info(item) else {
            return Vec::new();
        };
        let (_, left) = hand_types(&info.resolved.class);
        let dual = req.dual;
        slots_for_class(&info.resolved.class)
            .iter()
            .copied()
            .filter(|s| {
                *s != EquipSlot::OffHand
                    || gd_formats::inventory::hand_fits(left, 0, 0, dual, false)
            })
            .collect()
    }

    /// `item`'s hand type for `slot` (main = right, off = left, else 0).
    fn hand_type(&self, item: &Item, slot: EquipSlot) -> u32 {
        let Some(info) = self.info(item) else {
            return 0;
        };
        let (right, left) = hand_types(&info.resolved.class);
        match slot {
            EquipSlot::MainHand => right,
            EquipSlot::OffHand => left,
            _ => 0,
        }
    }

    /// The hand type hand `slot` of the active set holds (0: empty); a main-hand two-hander holds both.
    fn held_hand_type(&self, slot: EquipSlot) -> u32 {
        if let Some(m) = &self.equipped[slot_index(EquipSlot::MainHand)] {
            let t = self.hand_type(m, EquipSlot::MainHand);
            if is_two_handed_type(t) {
                return t;
            }
        }
        match &self.equipped[slot_index(slot)] {
            Some(it) => self.hand_type(it, slot),
            None => 0,
        }
    }

    /// The hand rule for `item` in hand `slot` of the active set: `strict` for auto insert, else manual equip.
    pub fn hand_fits(
        &self,
        item: &Item,
        slot: EquipSlot,
        strict: bool,
        req: &Requirements,
    ) -> bool {
        let other = match slot {
            EquipSlot::MainHand => EquipSlot::OffHand,
            EquipSlot::OffHand => EquipSlot::MainHand,
            _ => return false,
        };
        let t = self.hand_type(item, slot);
        let here = self.held_hand_type(slot);
        let o = self.held_hand_type(other);
        gd_formats::inventory::hand_fits(t, here, o, req.dual, strict)
    }

    /// `EquipmentCtrl::SmartAutoInsert`: the slot a right click equips `item` into; None when its requirements
    /// are unmet, it is not equipment or neither hand takes it.
    /// A single slot always (occupied or not); rings: ring 1 only when both are empty, else ring 2; weapons
    /// (active set, non-strict hand rule): the main hand, unless the off hand may take it, is empty and the
    /// main hand is full (a held two-hander fills both).
    pub fn smart_slot(&self, item: &Item, req: &Requirements) -> Option<EquipSlot> {
        let slots = self.allowed_slots(item, req);
        if slots.is_empty() || self.requirement_failure(item, req).is_some() {
            return None;
        }
        let empty = |inv: &Self, s: EquipSlot| inv.equipped[slot_index(s)].is_none();
        if slots.contains(&EquipSlot::Ring1) {
            return Some(
                if empty(self, EquipSlot::Ring1) && empty(self, EquipSlot::Ring2) {
                    EquipSlot::Ring1
                } else {
                    EquipSlot::Ring2
                },
            );
        }
        if !slots
            .iter()
            .any(|s| matches!(s, EquipSlot::MainHand | EquipSlot::OffHand))
        {
            return slots.first().copied();
        }
        let can_r = slots.contains(&EquipSlot::MainHand)
            && self.hand_fits(item, EquipSlot::MainHand, false, req);
        let can_l = slots.contains(&EquipSlot::OffHand)
            && self.hand_fits(item, EquipSlot::OffHand, false, req);
        let right_full = !empty(self, EquipSlot::MainHand);
        let main_2h = self.equipped[slot_index(EquipSlot::MainHand)]
            .as_ref()
            .is_some_and(|m| self.is_two_handed(m));
        let left_empty = empty(self, EquipSlot::OffHand) && !main_2h;
        if can_r && !(can_l && left_empty && right_full) {
            Some(EquipSlot::MainHand)
        } else if can_l {
            Some(EquipSlot::OffHand)
        } else {
            None
        }
    }

    /// `EquipmentCtrl::SmartAutoInsert`: equips `item` (already out of the bags, nothing held) into
    /// [`Self::smart_slot`]. One displaced item: the bags (`AddItem`), else the cursor. A two-hander displacing
    /// a main + off pair: both to the bags (`IsSpaceAvailable`), else main to the bags and off onto the cursor,
    /// else off to the bags and main onto the cursor, else [`SmartInsert::NoRoom`].
    pub fn smart_insert(&mut self, item: Item, req: &Requirements) -> SmartInsert {
        let Some(slot) = self.smart_slot(&item, req) else {
            return SmartInsert::Refused(item);
        };
        let main = slot_index(EquipSlot::MainHand);
        let off = slot_index(EquipSlot::OffHand);
        let main_2h = self.equipped[main]
            .as_ref()
            .is_some_and(|m| self.is_two_handed(m));
        let mut displaced: Vec<Item> = match slot {
            EquipSlot::MainHand if self.is_two_handed(&item) => [main, off]
                .iter()
                .filter_map(|&i| self.equipped[i].take())
                .collect(),
            // The engine's off-hand pointer holds a worn two-hander too.
            EquipSlot::OffHand if main_2h => self.equipped[main].take().into_iter().collect(),
            s => self.equipped[slot_index(s)].take().into_iter().collect(),
        };
        if displaced.len() == 2 {
            let l = displaced.pop().unwrap();
            let r = displaced.pop().unwrap();
            // Worn items never stack, so an add only pushes a new entry: undo by truncating each bag.
            let lens: Vec<usize> = self.bags.iter().map(|b| b.items.len()).collect();
            let undo = |inv: &mut Self| {
                for (b, n) in inv.bags.iter_mut().zip(&lens) {
                    b.items.truncate(*n);
                }
            };
            let both = self.add_to_bags(r.clone()) && self.add_to_bags(l.clone());
            if !both {
                undo(self);
                if self.add_to_bags(r.clone()) {
                    self.held = Some(l);
                } else {
                    undo(self);
                    if self.add_to_bags(l.clone()) {
                        self.held = Some(r);
                    } else {
                        undo(self);
                        self.equipped[main] = Some(r);
                        self.equipped[off] = Some(l);
                        return SmartInsert::NoRoom(item);
                    }
                }
            }
        } else if let Some(o) = displaced.pop()
            && !self.add_to_bags(o.clone())
        {
            self.held = Some(o);
        }
        self.equipped[slot_index(slot)] = Some(item);
        self.equipment_dirty = true;
        self.changed();
        SmartInsert::Equipped
    }

    /// Equips `item` into `slot`; returns what was there (or the item itself
    /// when it cannot go there).
    #[allow(clippy::result_large_err)]
    pub fn equip(
        &mut self,
        item: Item,
        slot: EquipSlot,
        req: &Requirements,
    ) -> Result<Option<Item>, Item> {
        if !self.allowed_slots(&item, req).contains(&slot) {
            return Err(item);
        }
        if matches!(slot, EquipSlot::MainHand | EquipSlot::OffHand)
            && !self.hand_fits(&item, slot, false, req)
        {
            info!(
                "cannot equip {}: does not fit next to the other hand",
                item.base_name
            );
            return Err(item);
        }
        if let Some(why) = self.requirement_failure(&item, req) {
            info!("cannot equip {}: {why}", item.base_name);
            return Err(item);
        }
        let main = slot_index(EquipSlot::MainHand);
        let off = slot_index(EquipSlot::OffHand);
        // An off-hand item replaces a two-hander, which goes back to a bag.
        if slot == EquipSlot::OffHand
            && self.equipped[main]
                .as_ref()
                .is_some_and(|m| self.is_two_handed(m))
            && let Some(m) = self.equipped[main].take()
            && !self.add_to_bags(m.clone())
        {
            self.equipped[main] = Some(m);
            return Err(item);
        }
        if slot == EquipSlot::MainHand
            && self.is_two_handed(&item)
            && let Some(o) = self.equipped[off].take()
            && !self.add_to_bags(o.clone())
        {
            self.equipped[off] = Some(o);
            return Err(item);
        }
        let old = self.equipped[slot_index(slot)].replace(item);
        self.equipment_dirty = true;
        self.changed();
        Ok(old)
    }

    /// `EquipmentCtrl::AutoInsertItem`: equips `item` into the first empty slot of [`AUTO_INSERT_ORDER`] it may
    /// go into, never swapping; `Err` hands it back.
    #[allow(clippy::result_large_err)]
    pub fn auto_insert(&mut self, item: Item, req: &Requirements) -> Result<EquipSlot, Item> {
        let slots = self.allowed_slots(&item, req);
        if slots.is_empty() || !self.usable(&item, req) {
            return Err(item);
        }
        for slot in AUTO_INSERT_ORDER {
            let i = slot_index(slot);
            if !slots.contains(&slot) || self.equipped[i].is_some() {
                continue;
            }
            let fits = match slot {
                EquipSlot::MainHand | EquipSlot::OffHand => self.hand_fits(&item, slot, true, req),
                _ => true,
            };
            if fits {
                self.equipped[i] = Some(item);
                self.equipment_dirty = true;
                self.changed();
                return Ok(slot);
            }
        }
        Err(item)
    }
}

/// `EquipmentCtrl::AutoInsertItem`'s slot order (the engine's Clothing location is not tried).
pub const AUTO_INSERT_ORDER: [EquipSlot; 14] = [
    EquipSlot::Head,
    EquipSlot::Amulet,
    EquipSlot::Relic,
    EquipSlot::Chest,
    EquipSlot::Legs,
    EquipSlot::Feet,
    EquipSlot::Hands,
    EquipSlot::Waist,
    EquipSlot::Shoulders,
    EquipSlot::Medal,
    EquipSlot::Ring1,
    EquipSlot::Ring2,
    EquipSlot::MainHand,
    EquipSlot::OffHand,
];

/// Whether the item worn in slot `i` is detached (an off-hand weapon the skills don't allow to dual wield, or
/// requirements unmet), and whether it was the dual-wield rule.
pub fn detach_rule(inv: &Inventory, i: usize, item: &Item, req: &Requirements) -> (bool, bool) {
    let left = if i == slot_index(EquipSlot::OffHand) {
        inv.hand_type(item, EquipSlot::OffHand)
    } else {
        0
    };
    let undual = left & 0x41 != 0 && left & req.dual == 0;
    (undual || !inv.usable(item, req), undual)
}

/// What the player's equip checks read; derived each frame by `sync_requirements`, before any reader.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct Requirements {
    pub level: i32,
    pub attributes: [f32; 3],
    pub reduction: gd_sim::items::req_reduction::Reduction,
    pub dual: u32,
}

/// Steps inside [`InventorySet`].
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InventoryStage {
    Init,
    Requirements,
    Equipment,
}
