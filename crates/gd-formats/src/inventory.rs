//! Inventory grids (`InventorySack`, Game.dll):
//! - a cell is 32×32 px (`InventorySack` ctor: cell w/h = 32.0f);
//! - an item's footprint is its bitmap's pixel size, at least one cell
//!   (`FindNextPosition`); positions are cell-aligned so the item
//!   covers `ceil(px / 32)` cells;
//! - grid size = sack pixel dims / 32 (`SetDims`); player sacks come from
//!   `UICharWindowInventorySack0Dims{X,Y}` (main bag) and `...Sack1Dims` (extra
//!   bags) read by `GameEngine::LoadFromDatabase`; at most 6 sacks
//!   (`PlayerInventoryCtrl::MaxSacks`);
//! - auto placement scans column-major for player sacks (`SearchVertical`,
//!   `AddSack` sets the vertical flag) and row-major otherwise
//!   (`SearchHorizantal`), first non-conflicting position wins;
//! - a held item goes down with its footprint centred on the cursor ([`drop_cell`]), swapping out the one
//!   item it overlaps ([`place_held`]); [`sort_sack`] repacks a grid; [`shard_target`] picks the partial
//!   component a picked-up one joins.
use crate::collections::HashMap;
use crate::save::item::Item;
use crate::tex::Tex;
use crate::vfs::{Vfs, normalized};
use parking_lot::Mutex;
use std::borrow::Cow;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::LazyLock;

pub const CELL_PX: u32 = 32;
pub const MAX_SACKS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Footprint {
    pub w: u32,
    pub h: u32,
}

impl Footprint {
    /// Cells covered by a bitmap of `px_w × px_h` pixels.
    pub fn from_pixels(px_w: u32, px_h: u32) -> Self {
        Self {
            w: px_w.max(CELL_PX).div_ceil(CELL_PX),
            h: px_h.max(CELL_PX).div_ceil(CELL_PX),
        }
    }
}

type FootprintCache = LazyLock<Mutex<HashMap<PathBuf, HashMap<String, Option<Footprint>>>>>;

/// Footprints by install root, then by bitmap path (mod-prefixed while a mod is mounted):
/// the install's bitmaps are immutable, so each footprint is read once per process (the
/// `vfs` `Records::cache` pattern). `None` is kept too: a missing or broken bitmap stays
/// 1×1 without re-reading.
static FOOTPRINTS: FootprintCache = LazyLock::new(Default::default);

/// Reads an item bitmap (`.tex`) header to get its footprint, memoised ([`FOOTPRINTS`]).
pub fn bitmap_footprint(vfs: &Vfs, bitmap: &str) -> Option<Footprint> {
    let path = normalized(bitmap);
    let key = |m: Option<&str>| -> Cow<str> {
        match m {
            Some(m) => Cow::Owned(format!("{m}\0{path}")),
            None => Cow::Borrowed(path.as_ref()),
        }
    };
    if let Some(&fp) = FOOTPRINTS
        .lock()
        .get(&vfs.root)
        .and_then(|m| m.get(key(vfs.active_mod()).as_ref()))
    {
        return fp;
    }
    let stamp = vfs.mod_stamp();
    let mounted = vfs.active_mod().map(str::to_owned);
    let fp = read_footprint(vfs, bitmap);
    // A mod swap mid-read (even away and back) must not cache the bitmap under the key it was
    // read from: keep the value for this call only when the stamp is unchanged, else re-read next call.
    if vfs.mod_stamp() == stamp {
        FOOTPRINTS
            .lock()
            .entry(vfs.root.clone())
            .or_default()
            .insert(key(mounted.as_deref()).into_owned(), fp);
    }
    fp
}

/// The footprint from the bitmap's header alone when provable ([`Tex::header_dims`]),
/// else from the whole file — identical results either way.
fn read_footprint(vfs: &Vfs, bitmap: &str) -> Option<Footprint> {
    if let Some(fp) = header_footprint(vfs, bitmap) {
        return Some(fp);
    }
    let tex = Tex::parse_owned(vfs.read(bitmap).ok()?).ok()?;
    let f = tex.frames.first()?;
    Some(Footprint::from_pixels(f.width, f.height))
}

/// TEX header (12) + frame 0's DDS header (128); the dims sit at DDS + 12..20.
fn header_footprint(vfs: &Vfs, bitmap: &str) -> Option<Footprint> {
    let mut r = vfs.open_entry(bitmap).ok()?;
    let mut hdr = [0u8; 140];
    r.read_exact(&mut hdr).ok()?;
    let len = r.seek(SeekFrom::End(0)).ok()?;
    let (w, h) = Tex::header_dims(&hdr, len)?;
    Some(Footprint::from_pixels(w, h))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed<T> {
    pub item: T,
    pub x: u32,
    pub y: u32,
    pub size: Footprint,
}

#[derive(Debug, Clone)]
pub struct Sack<T> {
    pub width: u32,
    pub height: u32,
    /// `true` for player bags (column-major search).
    pub vertical: bool,
    pub items: Vec<Placed<T>>,
}

impl<T: Clone> Sack<T> {
    pub fn new(width: u32, height: u32, vertical: bool) -> Self {
        Self {
            width,
            height,
            vertical,
            items: Vec::new(),
        }
    }

    /// `InventorySack::IsConflict` (rect overlap against every item). `x` / `y` may be any value (save
    /// coordinates are not trusted): a rect that is not wholly inside the grid is never free.
    pub fn is_free(&self, x: u32, y: u32, size: Footprint) -> bool {
        if size.w > self.width
            || x > self.width - size.w
            || size.h > self.height
            || y > self.height - size.h
        {
            return false;
        }
        // In the grid, so `x + size.w` / `y + size.h` cannot overflow; `items` is public, so a placed
        // item's own edge saturates.
        !self.items.iter().any(|p| {
            x < p.x.saturating_add(p.size.w)
                && p.x < x + size.w
                && y < p.y.saturating_add(p.size.h)
                && p.y < y + size.h
        })
    }

    /// `FindNextPosition`: first free cell in the sack's scan order.
    pub fn find_position(&self, size: Footprint) -> Option<(u32, u32)> {
        if self.vertical {
            for x in 0..self.width {
                if x + size.w > self.width {
                    return None;
                }
                for y in 0..self.height {
                    if y + size.h > self.height {
                        break;
                    }
                    if self.is_free(x, y, size) {
                        return Some((x, y));
                    }
                }
            }
        } else {
            for y in 0..self.height {
                if y + size.h > self.height {
                    return None;
                }
                for x in 0..self.width {
                    if x + size.w > self.width {
                        break;
                    }
                    if self.is_free(x, y, size) {
                        return Some((x, y));
                    }
                }
            }
        }
        None
    }

    /// `AddItemAndReturnPoint`: place at the first free position.
    pub fn add(&mut self, item: T, size: Footprint) -> Option<(u32, u32)> {
        let (x, y) = self.find_position(size)?;
        self.items.push(Placed { item, x, y, size });
        Some((x, y))
    }

    /// Place at an explicit cell (drag & drop / save restore).
    pub fn add_at(&mut self, item: T, x: u32, y: u32, size: Footprint) -> bool {
        if !self.is_free(x, y, size) {
            return false;
        }
        self.items.push(Placed { item, x, y, size });
        true
    }

    pub fn item_at(&self, x: u32, y: u32) -> Option<&Placed<T>> {
        self.items
            .iter()
            .find(|p| x >= p.x && x - p.x < p.size.w && y >= p.y && y - p.y < p.size.h)
    }
}

/// Top-left cell of the footprint of a held item of `size` over a grid of
/// `grid` cells with the cursor at `cursor` (x, y in cells from the grid's top-left).
/// `InventorySack::GetConflicts` + `AlignRect` (Game.dll): the rect is the item's size centred on the
/// cursor (offset half a cell), clamped into the grid, then truncated to
/// whole cells.
pub fn drop_cell(cursor: [f32; 2], grid: (u32, u32), size: Footprint) -> (u32, u32) {
    let f = |v: f32, s: u32, n: u32| {
        (v - s as f32 * 0.5 + 0.5)
            .max(0.0)
            .min(n.saturating_sub(s) as f32) as u32
    };
    (f(cursor[0], size.w, grid.0), f(cursor[1], size.h, grid.1))
}

/// Whether `q` overlaps the footprint `size` at (`x`, `y`).
pub fn overlaps<T>(q: &Placed<T>, x: u32, y: u32, size: Footprint) -> bool {
    x < q.x.saturating_add(q.size.w)
        && q.x < x.saturating_add(size.w)
        && y < q.y.saturating_add(q.size.h)
        && q.y < y.saturating_add(size.h)
}

/// Puts `held` (footprint `size`) on `sack` with its footprint centred on
/// `cursor` ([`drop_cell`]), swapping out the one item it overlaps: `Ok`
/// holds that item (None: the place was free). Two or more overlapped, or
/// the place does not fit: `Err` hands `held` back (the sack unchanged).
pub fn place_held<T: Clone>(
    sack: &mut Sack<T>,
    held: T,
    size: Footprint,
    cursor: [f32; 2],
) -> Result<Option<T>, T> {
    let (x, y) = drop_cell(cursor, (sack.width, sack.height), size);
    let mut under = (0..sack.items.len()).filter(|&i| overlaps(&sack.items[i], x, y, size));
    match (under.next(), under.next()) {
        (None, _) => {
            if !sack.is_free(x, y, size) {
                return Err(held);
            }
            sack.add_at(held, x, y, size);
            Ok(None)
        }
        (Some(i), None) => {
            let under = sack.items.remove(i);
            if !sack.is_free(x, y, size) {
                sack.items.insert(i, under);
                return Err(held);
            }
            sack.add_at(held, x, y, size);
            Ok(Some(under.item))
        }
        _ => Err(held),
    }
}

/// The stash sort's item-type rank of an item class (the value
/// each item class returns from `Item::GetSortRank`).
pub fn sort_rank(class: &str) -> u32 {
    match class {
        "ArmorProtective_Head" => 1,
        "ArmorJewelry_Amulet" => 2,
        "ArmorProtective_Chest" | "ArmorMisc_Clothing" | "ArmorMisc_Vestment" => 3,
        "ArmorProtective_Legs" => 4,
        "ArmorProtective_Feet" => 5,
        "ArmorJewelry_Ring" => 6,
        "ArmorProtective_Hands" | "ArmorJewelry_Bracelet" => 7,
        c if c.starts_with("WeaponArmor") => 8,
        c if c.starts_with("WeaponMelee")
            || c.starts_with("WeaponHunting")
            || c.starts_with("WeaponMagical") =>
        {
            9
        }
        "ItemArtifact" => 11,
        c if c.ends_with("Formula") => 12,
        "ArmorProtective_Waist" => 13,
        "ArmorProtective_Shoulders" => 14,
        "ArmorJewelry_Medal" => 15,
        "ItemRelic" | "ItemCharm" => 16,
        "OneShot_Sack" => 18,
        c if c.starts_with("OneShot") => 17,
        "ItemNote" => 20,
        "ItemEnchantment" => 21,
        "ItemTransmuter" | "ItemTransmuterSet" => 22,
        "ItemFactionWarrant" | "ItemFactionBooster" => 23,
        "ItemDevotionReset" => 24,
        "ItemDifficultyUnlock" => 25,
        "ItemAttributeReset" => 26,
        _ => 0,
    }
}

/// `InventorySack::Sort` on one grid: the items ordered by footprint height, then width, then the item-type
/// rank ([`sort_rank`]), all largest first, then display name (`key` gives an item's (rank, name), asked once
/// per item in grid order), each at the first free cell. False (nothing changed) when the new layout cannot
/// hold every item. `InventorySack::Sort`: the items go into a packer (`InventoryPacker::AddItem` /
/// `InventoryPacker::Place`) whose solve (`InventoryPacker::Solve`) ANDs every item's placement result; the
/// sack's positions are rewritten only when it returns true, so a sort that does not fit leaves the sack as it
/// was and no item ever leaves it.
pub fn sort_sack<T: Clone>(sack: &mut Sack<T>, mut key: impl FnMut(&T) -> (u32, String)) -> bool {
    let mut keyed: Vec<_> = sack
        .items
        .iter()
        .map(|q| {
            let (rank, name) = key(&q.item);
            (q.size.h, q.size.w, rank, name, q)
        })
        .collect();
    keyed.sort_by(|a, b| {
        (b.0, b.1, b.2)
            .cmp(&(a.0, a.1, a.2))
            .then_with(|| a.3.cmp(&b.3))
    });
    let mut sorted = Sack::new(sack.width, sack.height, sack.vertical);
    for k in keyed {
        if sorted.add(k.4.item.clone(), k.4.size).is_none() {
            return false;
        }
    }
    sack.items = sorted.items;
    true
}

/// The bag component an incomplete component (`item`, `need` shards complete it, `completedRelicLevel`)
/// joins, as (sack, item index). `InventorySack::GetNotFullRelic`, sack by sack: the
/// first one of the same record (case-insensitive, `/` = `\`), itself not complete, whose shards plus the
/// item's are at most `need` (one that would overflow is passed over: a pickup never splits). None when
/// `item` is complete. PORT: a sack's items in the port's order (the engine's is its id map's).
pub fn shard_target(sacks: &[Sack<Item>], item: &Item, need: u32) -> Option<(usize, usize)> {
    let shards = item.var1.max(1);
    if shards >= need {
        return None;
    }
    let same = |a: &str, b: &str| {
        a.len() == b.len()
            && a.bytes().zip(b.bytes()).all(|(x, y)| {
                let n = |c: u8| {
                    if c == b'\\' {
                        b'/'
                    } else {
                        c.to_ascii_lowercase()
                    }
                };
                n(x) == n(y)
            })
    };
    sacks.iter().enumerate().find_map(|(b, s)| {
        s.items
            .iter()
            .position(|q| {
                let have = q.item.var1.max(1);
                same(&q.item.base_name, &item.base_name) && have < need && have + shards <= need
            })
            .map(|i| (b, i))
    })
}

/// Equipment slots in save order (`EquipmentCtrl`),
/// plus the two weapon sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipSlot {
    Head,
    Amulet,
    Chest,
    Legs,
    Feet,
    Hands,
    Ring1,
    Ring2,
    Waist,
    Shoulders,
    Medal,
    Relic,
    MainHand,
    OffHand,
}

/// Which slots an item record class may go into (
/// `EquipmentCtrl::ValidateItemType`: each armour slot accepts
/// one item type, the class's virtual type constant; `CanItemBePlaced`
/// tests the hands against the class's right/left hand type
/// masks: two-handers, staves and the hunting spear have no left-hand type,
/// shields/off-hands no right-hand type, other one-handers both, the left
/// gated by dual wielding, see the client's `allowed_slots`). The chest piece
/// also fits the unused "Clothing" location, which the port has no slot for.
pub fn slots_for_class(class: &str) -> &'static [EquipSlot] {
    use EquipSlot::*;
    match class {
        "ArmorProtective_Head" => &[Head],
        "ArmorJewelry_Amulet" => &[Amulet],
        "ArmorProtective_Chest" => &[Chest],
        "ArmorProtective_Legs" => &[Legs],
        "ArmorProtective_Feet" => &[Feet],
        "ArmorProtective_Hands" => &[Hands],
        "ArmorJewelry_Ring" => &[Ring1, Ring2],
        "ArmorProtective_Waist" => &[Waist],
        "ArmorProtective_Shoulders" => &[Shoulders],
        "ArmorJewelry_Medal" => &[Medal],
        "ItemArtifact" => &[Relic],
        "WeaponArmor_Shield" | "WeaponArmor_Offhand" => &[OffHand],
        c if c.starts_with("Weapon")
            && (c.ends_with("2h") || c == "WeaponMagical_Staff" || c == "WeaponHunting_Spear") =>
        {
            &[MainHand]
        }
        c if c.starts_with("Weapon") => &[MainHand, OffHand],
        _ => &[],
    }
}

/// A weapon class's hand types (right, left): Game.dll, `Item::GetRightHandType` /
/// `GetLeftHandType` per class. 0 = never fits that hand; [`HANDS_TWO`] types are
/// two-handed (the engine stores such an item in both hands). PORT: `WeaponHunting_Spear` (right 0x8, one-handed,
/// pairs only with a shield or focus) has no stock record, so it is untested.
pub fn hand_types(class: &str) -> (u32, u32) {
    match class {
        "WeaponHunting_Ranged1h" => (0x80, 0x40),
        "WeaponHunting_Ranged2h" => (0x4, 0),
        "WeaponHunting_Spear" => (0x8, 0),
        "WeaponMagical_Staff" => (0x10, 0),
        "WeaponArmor_Shield" => (0, 0x20),
        "WeaponArmor_Offhand" => (0, 0x100),
        c if c.starts_with("WeaponMelee") && c.ends_with("2h") => (0x200, 0),
        c if c.starts_with("WeaponMelee") => (0x2, 0x1),
        _ => (0, 0),
    }
}

/// The two-handed hand types (rifles / crossbows, staves, melee two-handers).
pub const HANDS_TWO: u32 = 0x4 | 0x10 | 0x200;

/// Whether hand type `t` is two-handed.
pub fn is_two_handed_type(t: u32) -> bool {
    t & HANDS_TWO != 0
}

/// `WeaponSet__OtherHandMask`: the hand types the other hand may hold next to a hand
/// holding type `held` (0 = empty: anything).
pub fn other_hand_mask(held: u32) -> u32 {
    match held {
        0x2 => 0x121,
        0x1 => 0x2,
        0x80 => 0x160,
        0x40 => 0x80,
        0x8 => 0x120,
        0x20 | 0x100 => 0x8A,
        0x4 | 0x10 | 0x200 => 0,
        _ => u32::MAX,
    }
}

/// `WeaponSet__PlaceRight` / `WeaponSet__PlaceLeft` (`CanItemBePlaced`
/// non-strict, `AutoInsertItem`'s `CanPlaceRightHandEmpty` / `…LeftHandEmpty` strict): whether an
/// item of hand type `t` (for the target hand) fits it, with `here` the target hand's held type and `other` the
/// other hand's (0 = empty), `dual` the character's dual-wield mask (`SetAllowDualWeapons`: 0x1 melee, 0x40
/// pistols). Strict: the target hand is empty, the other holds no two-hander, a two-hander needs the other empty
/// too. Then the allowed types are `~gate & 0x3ff` (gate: 0x1 without dual melee, 0x40 without dual pistols),
/// ANDed with the other hand's [`other_hand_mask`] unless either is two-handed. The requirements test is the
/// caller's. Non-strict, an occupied target swaps and a displaced two-hander / other hand goes to the bags.
pub fn hand_fits(t: u32, here: u32, other: u32, dual: u32, strict: bool) -> bool {
    let two = is_two_handed_type(t);
    if strict && (here != 0 || is_two_handed_type(other) || (two && other != 0)) {
        return false;
    }
    let gate = if dual & 0x1 != 0 { 0 } else { 0x1 } | if dual & 0x40 != 0 { 0 } else { 0x40 };
    let mut allowed = !gate & 0x3ff;
    if !two && !is_two_handed_type(other) {
        allowed &= other_hand_mask(other);
    }
    allowed & t != 0
}
