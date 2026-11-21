//! Merchant stock (`NpcMerchant` `marketFileName` → `market.tpl` record).
//!
//! (`MarketMiniMart` constructor @0x18036b8c0, the category
//! classes' `GetTableName` / `GetNumItemsMin` / `Max` /
//! `GetDuplicatesAllowed`): a merchant's stock is [`CATEGORIES`] in that
//! order, each `marketXTable` with `marketNumXMin..Max`. A faction merchant
//! (`factionmarket.tpl`, constructor @0x180298cc0, refresh @0x1802991c0)
//! refreshes each tier record (`friendly..reveredNormalTable`) through
//! [`TIER_CATEGORIES`] (`marketWeaponTable`, `marketArmorTable`,
//! `marketPotionTable` and the `marketStaticItems` list, which has no count)
//! for that tier's tab, then its own record's `marketStaticTable` (tier 0).
//!
//! `MarketMiniMart::Refresh` @0x18036fdf0, per category: `n = min + r %
//! (max - min + 1)` (always drawn); then by the class of the field's first
//! record: a `LootMasterTable` gives `n` rolls (`LoadItem` @0x18036fd10, at
//! the market's level, parent and player level both); a `LevelTable` picks
//! its table by that level (`sub_180355D40`) and gives `n` rolls of it;
//! anything else (item records, `marketStaticItems`) adds every listed
//! record once, `n` unused. Each pick goes through `AddIfUnique`
//! @0x18036fb20 (a repeat is refused unless the category allows
//! duplicates; a new item draws its seed) and `UploadItems` @0x180370260.
//! The game restocks every `marketRefreshTimeMin..Max` minutes
//! (`sub_180371130`) at the merchant's level.
//!
//! Prices (`sub_18036CD80`, `sub_18036D7F0`): buying costs
//! `cost + trunc(cost × marketCostAdjustment % + 0.00001)` as a wrapping u32;
//! selling pays `cost − floor(cost × marketDepreciation % + 0.5)`, capped
//! (when non-zero, negative included) at `maxPlayerPurchase[difficulty]` ×
//! stack size (int, wrapping), never below 0.
//!
//! Vendor search (GrimDawn.exe market window, Game.dll `Item::SearchText`):
//! the box's text, trimmed ([`search_query`] `sub_1402A5300`) and
//! lowercased (`towlower_l`, the game locale), is looked for as a substring of
//! each of the item's search lines ([`search_matches`],
//! `Item::SearchText` @0x180329720: one line must contain the whole query; an
//! empty query matches nothing). The lines (`Item::GenerateSearchText`
//! @0x180329370, cached per item) are the item's display text
//! (`ItemEquipment::GetSearchText` @0x18033e4a0 for equipment: the tooltip's
//! name / attribute / skill / pet / granted skill / set / requirement blocks;
//! the class's `GetUIDisplayText` otherwise), each format-stripped
//! (`LocalizerFormatStrip`) and lowercased ([`search_line`]).
//!
//! The market window: [`VENDOR_TABS`] tabs, the four item categories in their order ([`vendor_tab`]) and the
//! buyback tab ([`BUYBACK_TAB`]). It opens on [`opening_tab`]; an empty tab cannot be picked ([`tab_enabled`]);
//! the search marks per tab what [`search_market`] finds. A merchant's stock is kept per [`market_key`] and
//! rolled again after [`refresh_minutes`]. Prices: [`prices`] (with the faction discount), [`stock_price`] (the
//! buyback tab charges the stored sale price).
use super::ItemDb;
use super::inventory::Sack;
use super::loot::{LootParams, LootRoll, roll_table_names};
use crate::combat::Rng;
use gd_formats::arz::Record;
use gd_formats::save::item::Item;
use std::collections::HashMap;

/// A market category: its list field and its `marketNum<X>Min/Max` name
/// (`None`: no count, 0..0).
pub type Category = (&'static str, Option<&'static str>);

/// A merchant's categories, in the engine's refresh order.
pub const CATEGORIES: [Category; 23] = [
    ("marketAxeTable", Some("Axe")),
    ("marketMaceTable", Some("Mace")),
    ("marketSpearTable", Some("Spear")),
    ("marketStaffTable", Some("Staff")),
    ("marketSwordTable", Some("Sword")),
    ("marketDaggerTable", Some("Dagger")),
    ("marketScepterTable", Some("Scepter")),
    ("marketRanged1hTable", Some("Ranged1h")),
    ("marketRanged2hTable", Some("Ranged2h")),
    ("marketHandsTable", Some("Hands")),
    ("marketChestTable", Some("Chest")),
    ("marketFeetTable", Some("Feet")),
    ("marketHeadTable", Some("Head")),
    ("marketLegsTable", Some("Legs")),
    ("marketWaistTable", Some("Waist")),
    ("marketShouldersTable", Some("Shoulders")),
    ("marketShieldTable", Some("Shield")),
    ("marketPotionTable", Some("Potion")),
    ("marketRingTable", Some("Ring")),
    ("marketAmuletTable", Some("Amulet")),
    ("marketMedalTable", Some("Medal")),
    ("marketScrollTable", Some("Scroll")),
    ("marketStaticTable", Some("Static")),
];

/// A faction tier record's categories.
pub const TIER_CATEGORIES: [Category; 4] = [
    ("marketWeaponTable", Some("Weapon")),
    ("marketArmorTable", Some("Armor")),
    ("marketPotionTable", Some("Potion")),
    ("marketStaticItems", None),
];

/// The faction merchant's own category (after the tiers, into tier 0).
pub const FACTION_STATIC: Category = ("marketStaticTable", Some("Static"));

/// The market record of an `NpcMerchant` on `difficulty` (0 Normal, 1 Elite, 2 Ultimate).
/// `NpcMerchant::Load` @0x1803bd830: `marketFileName[GameInfo::GetDifficulty()]` through
/// `LoadTableBinary::GetArrayValue` (Engine.dll @0x180140900), which clamps the index to the last entry (no challenge term: Ultimate Veteran reads the
/// Ultimate entry). Stock:
/// the Shattered Realm's reward merchants (`merchant_reward_02..05`) list one table per difficulty, and only the
/// Ultimate one (`merchanttbl_reward_0Nu`) sells the Shattered set (`lt_shatteredset_*`).
pub fn market_file(npc: &Record, difficulty: usize) -> Option<&str> {
    let list = npc.strs("marketFileName");
    list.get(difficulty.min(list.len().checked_sub(1)?))
        .copied()
}

/// The market's stock rolled with `params` (at its `parent_level`).
///
/// No Broken items: `Refresh` calls `SetNoBrokenItems(true, false)` on each
/// category's table and sets no randomizer weight modifiers (
/// `MarketMiniMart::Refresh`; `LootBase` constructs with zero modifiers and
/// the altar flag on), so `brokenOnly` is 0 and the other weights are the
/// tables' own.
pub fn roll_market(
    db: &ItemDb,
    market: &Record,
    params: &LootParams,
    rng: &mut Rng,
) -> Vec<LootRoll> {
    roll_categories(db, market, &CATEGORIES, params, rng)
}

/// The stock of `record` over `categories` (see the module docs).
pub fn roll_categories(
    db: &ItemDb,
    record: &Record,
    categories: &[Category],
    params: &LootParams,
    rng: &mut Rng,
) -> Vec<LootRoll> {
    let params = &LootParams {
        no_broken: true,
        weight_mods: [0; 10],
        ..*params
    };
    categories
        .iter()
        .flat_map(|c| refresh(db, record, *c, params, rng))
        .collect()
}

/// `MarketMiniMart::Refresh` of one category.
fn refresh(
    db: &ItemDb,
    record: &Record,
    (field, num): Category,
    params: &LootParams,
    rng: &mut Rng,
) -> Vec<LootRoll> {
    let count = |k: &str| {
        num.map_or(0, |c| {
            record.i32(&format!("marketNum{c}{k}")).unwrap_or(0).max(0)
        })
    };
    let (lo, hi) = (count("Min"), count("Max").max(count("Min")));
    // `MarketMiniMart::Refresh`: int arithmetic, wrapping.
    let span = hi.wrapping_sub(lo).wrapping_add(1) as u32;
    // Bad data guard: a huge `marketNum*Max` would loop for minutes.
    let n = lo.wrapping_add((rng.next() % span) as i32).min(1000);
    let Some(first) = record.str(field).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    // Only the scroll category allows duplicates (`GetDuplicatesAllowed`:
    // 0, `MarketMisc_Scroll` reads `marketNumScrollDuplicates`).
    let dups = if num == Some("Scroll") {
        record.i32("marketNumScrollDuplicates").unwrap_or(0).max(0) as u32
    } else {
        0
    };
    // (item, extra copies)
    let mut list: Vec<(LootRoll, u32)> = Vec::new();
    // `AddIfUnique`: a pick equal to a listed item (base, prefix, suffix)
    // adds a copy while fewer than `dups` were added, else it is refused; a
    // new item draws its seed; nothing (no base) counts as added.
    let mut add = |r: LootRoll, rng: &mut Rng| -> bool {
        match list
            .iter_mut()
            .find(|(e, _)| (&e.base, &e.prefix, &e.suffix) == (&r.base, &r.prefix, &r.suffix))
        {
            Some((_, extra)) if *extra < dups => *extra += 1,
            Some(_) => return false,
            None if r.base.is_empty() => {}
            None => list.push((
                LootRoll {
                    seed: rng.next(),
                    ..r
                },
                0,
            )),
        }
        true
    };
    if db
        .get(first)
        .is_some_and(|r| matches!(r.class.as_str(), "LootMasterTable" | "LevelTable"))
    {
        for _ in 0..n {
            // `LoadItem`: up to 5 tries while `AddIfUnique` refuses the pick,
            // sharing one table depth budget of 5.
            let mut depth = 5;
            for _ in 0..5 {
                let r = roll_table_names(db, first, params, rng, &mut depth);
                if add(r, rng) {
                    break;
                }
            }
        }
    } else {
        // The static branch: every listed record once.
        for name in record.strs(field) {
            add(
                LootRoll {
                    base: name.to_owned(),
                    prefix: String::new(),
                    suffix: String::new(),
                    seed: 0,
                },
                rng,
            );
        }
    }
    // `UploadItems`: each entry as 1 + its copies, all with its seed.
    list.into_iter()
        .flat_map(|(r, extra)| std::iter::repeat_n(r, extra as usize + 1))
        .collect()
}

/// What the merchant asks for an item worth `cost` (`sub_18036CD80`:
/// `cost + (int64)(adj*0.01*cost + 0.00001)` stored as a u32, wrapping).
pub fn buy_price(market: &Record, cost: u32) -> u64 {
    let adj = market.f32("marketCostAdjustment").unwrap_or(0.0);
    let extra = (adj * 0.01 * cost as f32 + 0.00001) as i64;
    (cost as i64).wrapping_add(extra) as u32 as u64
}

/// What the merchant pays for an item worth `cost` (stack of `count`)
/// (`sub_18036D7F0`): `cost - floor(dep*0.01*cost + 0.5)` in
/// floats, truncated to an int; a non-zero cap (times the stack size when
/// that is non-zero, wrapping) lowers it, negative caps included; then at
/// least 0.
pub fn sell_price(market: &Record, cost: u32, count: u32, difficulty: usize) -> u64 {
    let dep = market.f32("marketDepreciation").unwrap_or(0.0);
    let cost = cost as f32;
    let mut v = (cost - (dep * 0.01 * cost + 0.5).floor()) as i64 as i32;
    let caps = market.f32s("maxPlayerPurchase");
    let cap = caps
        .get(difficulty.min(caps.len().saturating_sub(1)))
        .map_or(0, |c| *c as i32);
    if cap != 0 {
        let cap = if count != 0 {
            cap.wrapping_mul(count as i32)
        } else {
            cap
        };
        v = v.min(cap);
    }
    v.max(0) as u64
}

/// The search box text as the market window searches for it (
/// `sub_1402A5300` then `towlower_l`): spaces trimmed at both ends, a `{`
/// or `}` among the trimmed characters kept at its end (the last one met
/// from the outside in), lowercased.
pub fn search_query(raw: &str) -> String {
    let c: Vec<char> = raw.chars().collect();
    let edge = |ch: char| ch == ' ' || ch == '{' || ch == '}';
    let (mut i, mut lead) = (0, None);
    while i < c.len() && edge(c[i]) {
        if c[i] != ' ' {
            lead = Some(c[i]);
        }
        i += 1;
    }
    let (mut j, mut trail) = (c.len(), None);
    while j > i && edge(c[j - 1]) {
        if c[j - 1] != ' ' {
            trail = Some(c[j - 1]);
        }
        j -= 1;
    }
    let mut out: String = lead.into_iter().collect();
    out.extend(&c[i..j]);
    out.extend(trail);
    out.to_lowercase()
}

/// One tooltip line as a search line: the `^X` colour codes (and `{` `}`
/// grouping) stripped, lowercased. PORT: `LocalizerFormatStrip` itself is not
/// decompiled; these are the codes the port's tooltip text keeps.
pub fn search_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut it = text.chars();
    while let Some(ch) = it.next() {
        match ch {
            '^' => {
                it.next();
            }
            '{' | '}' => {}
            _ => out.extend(ch.to_lowercase()),
        }
    }
    out
}

/// Whether an item with these search lines ([`search_line`]) matches
/// `query` ([`search_query`]).
pub fn search_matches(lines: &[String], query: &str) -> bool {
    !query.is_empty() && lines.iter().any(|l| l.contains(query))
}

/// The market window's tabs: the four item categories ([`vendor_tab`]) and the buyback tab.
pub const VENDOR_TABS: usize = 5;

pub const BUYBACK_TAB: usize = 4;

/// A merchant is its record at its spot (`x`, `z`: world position).
pub fn market_key(npc_record: &str, x: f32, z: f32) -> String {
    format!("{npc_record}@{x:.0},{z:.0}")
}

/// The refresh delay (minutes) of a market record: `marketRefreshTimeMin..Max` at `roll` (0..1).
pub fn refresh_minutes(mrec: &Record, roll: f32) -> f32 {
    let (lo, hi) = (
        mrec.f32("marketRefreshTimeMin").unwrap_or(10.0),
        mrec.f32("marketRefreshTimeMax").unwrap_or(15.0),
    );
    lo + roll * (hi - lo).max(0.0)
}

/// The vendor tab of an item class. The market client sorts each
/// item into a sack by the item's market category (Item vtable +1384:
/// 2 = melee weapons incl. spears, daggers and scepters, 3 = ranged
/// weapons and `WeaponMagical*` (staves), 1 = `ArmorProtective*`,
/// `ArmorMisc*`, shields and off-hands, anything else (jewellery, relics,
/// consumables) the fourth sack; `sub_18036CA50`), and the window's tabs
/// show the categories in the order 2, 3, 1, 4 (GrimDawn.exe
/// `sub_140275370`, table @0x14031EC80), then the buyback sack.
pub fn vendor_tab(class: &str) -> usize {
    if class.starts_with("WeaponMelee") || class == "WeaponHunting_Spear" {
        0
    } else if class.starts_with("WeaponHunting_Ranged") || class.starts_with("WeaponMagical") {
        1
    } else if class.starts_with("ArmorProtective")
        || class.starts_with("ArmorMisc")
        || class == "WeaponArmor_Shield"
        || class == "WeaponArmor_Offhand"
    {
        2
    } else {
        3
    }
}

/// The tab a market window opens on (GrimDawn.exe window show
/// `sub_140275370`): the remembered one (`tab`) unless its page is empty,
/// then the first stocked of the four category tabs in their order (table
/// @0x14031EC80: categories 2, 3, 1, 4 = tabs 0–3; not the buyback tab);
/// `tab` when all are empty.
pub fn opening_tab(pages: &[Sack<Item>], tab: usize) -> usize {
    if tab_enabled(pages, tab) {
        return tab;
    }
    (0..BUYBACK_TAB)
        .find(|t| tab_enabled(pages, *t))
        .unwrap_or(tab)
}

/// Whether a market tab can be picked: an empty page's button is disabled
/// and drawn dimmed (window update `sub_1402731E0`: every frame,
/// each tab button whose `GetMarketInventorySack` is empty gets its
/// disabled flags, +641 / +644; draw in the module doc of `ui/trade.rs`).
pub fn tab_enabled(pages: &[Sack<Item>], tab: usize) -> bool {
    pages.get(tab).is_some_and(|p| !p.items.is_empty())
}

/// What the vendor search finds in a market: per tab, the grid cells of the
/// matching items. (grid `sub_1401FC8E0`, every window update,
/// [`search_matches`]): with a non-empty query the
/// shown page's items are split into matches and the rest (the grid marks
/// both, see `ui/trade.rs`), and every other tab whose sack has a match
/// (`InventorySack::SearchItems`) is listed for its button's highlight.
/// `lines` gives an item's search lines ([`search_line`]).
pub fn search_market(
    pages: &[Sack<Item>],
    query: &str,
    mut lines: impl FnMut(&Item) -> Vec<String>,
) -> Vec<Vec<(u32, u32)>> {
    pages
        .iter()
        .map(|p| {
            if query.is_empty() {
                Vec::new()
            } else {
                p.items
                    .iter()
                    .filter(|q| search_matches(&lines(&q.item), query))
                    .map(|q| (q.x, q.y))
                    .collect()
            }
        })
        .collect()
}

/// Buy and sell price of `item` at market record `market`; `discount` = the faction rank's market discount (%,
/// read as an integer `GameEngine::LoadFromDatabase` /
/// `GetFactionDiscountMultiplier`: the buy price × (1 − discount / 100),
/// rounded half up; a plain sell price is not affected).
pub fn prices(
    idb: &ItemDb,
    market: &str,
    item: &Item,
    difficulty: usize,
    discount: i32,
) -> Option<(u64, u64)> {
    let m = idb.get(market)?;
    let cost = super::resolve::item_cost(idb, item);
    let buy = buy_price(m, cost) as f64 * (1.0 - discount as f64 / 100.0);
    Some((
        buy.max(0.0).round() as u64,
        sell_price(m, cost, item.stack_count, difficulty),
    ))
}

/// The item at `cell` of a merchant's tab `tab` (`pages`: its stock, `buyback`: the stored sale price per
/// buyback cell) and its price: the buyback tab charges the stored sale price (`sub_18036D720`:
/// without the faction discount or the `marketCostAdjustment` markup), the rest [`prices`]' buy price with
/// the faction `discount`.
#[allow(clippy::too_many_arguments)]
pub fn stock_price(
    pages: &[Sack<Item>],
    buyback: Option<&HashMap<(u32, u32), u64>>,
    idb: &ItemDb,
    market: &str,
    tab: usize,
    cell: (u32, u32),
    difficulty: usize,
    discount: i32,
) -> Option<(Item, u64)> {
    let placed = pages
        .get(tab.min(VENDOR_TABS - 1))?
        .item_at(cell.0, cell.1)?;
    let stored = (tab == BUYBACK_TAB)
        .then(|| buyback.and_then(|b| b.get(&(placed.x, placed.y))).copied())
        .flatten();
    let buy = match stored {
        Some(p) => p,
        None => {
            prices(idb, market, &placed.item, difficulty, discount)
                .unwrap_or((0, 0))
                .0
        }
    };
    Some((placed.item.clone(), buy))
}
