//! The inventor's and blacksmith's transactions: crafting a blueprint,
//! dismantling equipment, salvaging a component or augment, converting a
//! set item (`gd_sim::items::convert`). Grimdawn's
//! windows pick what to do and call these. Which item a window's slot takes is [`slot_accepts`]; the
//! dismantle prompt shows for [`dismantle_warns`].
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game_core::inventory::Inventory;
use gd_game_core::stash::{Stash, count, take};
use gd_sim::items::crafting::Blueprint;

pub const DISMANTLE: &str = "records/ui/inventor/dismantlepanel/dismantle_table.dbr";

/// How many times the player could craft `bp` now
/// (`ItemArtifactFormula::GetMaximumCraftable`): the least of
/// held / needed over the reagents (a reagent listed twice needs both
/// amounts) and, when it costs anything, money / cost; no reagents → 0.
pub fn max_craftable(bp: &Blueprint, gold: u64, level: u32, inv: &Inventory, stash: &Stash) -> u32 {
    let Some(by_reagents) = summed_reagents(blueprint_reagents(bp))
        .iter()
        .map(|(names, n)| count(names, inv, stash) / (*n).max(1))
        .min()
    else {
        return 0;
    };
    match bp.cost(level) {
        0 => by_reagents,
        c => by_reagents.min((gold / c).min(u32::MAX as u64) as u32),
    }
}

fn blueprint_reagents(bp: &Blueprint) -> impl Iterator<Item = (&[String], u32)> {
    bp.reagents.iter().map(|(names, n)| (names.as_slice(), *n))
}

/// A recipe's reagents with the counts of a reagent listed more than once summed (checking each entry alone
/// against the full held count would pass a recipe needing more than is held).
pub(crate) fn summed_reagents<'a, K: PartialEq + ?Sized>(
    reagents: impl IntoIterator<Item = (&'a K, u32)>,
) -> Vec<(&'a K, u32)> {
    let mut need: Vec<(&K, u32)> = Vec::new();
    for (k, n) in reagents {
        match need.iter_mut().find(|(m, _)| *m == k) {
            Some((_, s)) => *s += n,
            None => need.push((k, n)),
        }
    }
    need
}

/// Whether the player can craft `bp` now (gold and every reagent, a reagent listed twice needing both amounts).
pub fn can_craft(bp: &Blueprint, gold: u64, level: u32, inv: &Inventory, stash: &Stash) -> bool {
    gold >= bp.cost(level)
        && summed_reagents(blueprint_reagents(bp))
            .iter()
            .all(|(names, n)| count(names, inv, stash) >= *n)
}

/// Crafts `bp` at the player's level (`params.player_level`, the items
/// rolled with `params`): pays, takes the reagents and puts the made items
/// in the bags. `Err` says why not.
#[allow(clippy::too_many_arguments)]
pub fn craft(
    data: &gd_game_core::data::GameData,
    enhancement: Option<&str>,
    bp: &Blueprint,
    gold: &mut u64,
    params: &gd_sim::items::LootParams,
    inv: &mut Inventory,
    stash: &mut Stash,
    req: &gd_game_core::inventory::Requirements,
) -> Result<Vec<Item>, &'static str> {
    let level = params.player_level;
    // Exe craft button `BlacksmithWindow::UpdateCraftButton`: disabled unless the formula is valid.
    if !data
        .db
        .get(&bp.record)
        .is_some_and(|r| gd_sim::items::crafting::formula_valid(&*data.db, r))
    {
        return Err("invalid blueprint");
    }
    if !can_craft(bp, *gold, level, inv, stash) {
        return Err(if *gold < bp.cost(level) {
            "not enough iron bits"
        } else {
            "missing materials"
        });
    }
    let idb = data.item_db();
    let mut rng = gd_rules::Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
    let items = bp.craft(&idb, params, enhancement, &mut rng);
    if items.is_empty() {
        return Err("the blueprint made nothing");
    }
    // Room check on a copy of the bags (new cells only, so the real gives, which merge stacks first, fit).
    let mut bags = inv.bags.clone();
    for it in &items {
        let Some(size) = inv.info(it).map(|i| i.size) else {
            return Err("unknown item");
        };
        if !bags.iter_mut().any(|b| b.add(it.clone(), size).is_some()) {
            return Err("no room in the bags");
        }
    }
    *gold -= bp.cost(level);
    for (names, n) in &bp.reagents {
        take(names, *n, inv, stash);
    }
    for it in &items {
        if let Err(it) = inv.give(it.clone(), false, req) {
            if inv.held.is_none() {
                inv.held = Some(it);
            } else {
                error!("craft: no room for {} after the room check", it.base_name);
            }
        }
    }
    Ok(items)
}

/// The dismantle cost of an item (`dismantleCost` at its item level).
pub fn dismantle_cost(table: &gd_formats::arz::Record, item_level: u32) -> u64 {
    table
        .str("dismantleCost")
        .and_then(|e| gd_rules::expr::Expr::parse(e).ok())
        .map_or(0.0, |e| {
            e.eval(&|n| {
                n.eq_ignore_ascii_case("itemLevel")
                    .then_some(item_level as f32)
            })
        })
        .max(0.0) as u64
}

/// The salvage cost of an item (`UISalvagePanel::Update` and
/// `EnchanterRecoveryConfigCmd::Execute`): `gameengine.dbr`
/// `enchanterRecoveryFactor` (0.05) x `Item::GetItemCost` (the item's price
/// with its component and augment, see `item_cost`), truncated.
pub fn salvage_cost(
    data: &gd_game_core::data::GameData,
    idb: &gd_sim::items::ItemDb,
    item: &Item,
) -> u64 {
    let factor = data
        .db
        .get("records/game/gameengine.dbr")
        .and_then(|e| e.f32("enchanterRecoveryFactor"))
        .unwrap_or(0.05);
    (factor * gd_sim::items::resolve::item_cost(idb, item) as f32) as u64
}

/// Why the Inventor's dismantle or convert did nothing (logged only). `Grim Dawn.exe` enchanter tabs (dismantle
/// `DismantleTab::OnButton` / `DismantleTab::Update` … `UIEnchanterDismantleTab::RollBonus`, convert
/// `UIEnchanterConvertTab::Convert`, salvage `UISalvagePanel::Update`): they send no UI
/// notification, success or not (their only messages are the confirm dialogs, `tagDismantleDestroyItemWarning`,
/// `tagDismantleDestroyItemComponentWarning`, `tagConvertBlacklist`); a refused action's button is not enabled
/// there, so the port's requests reaching a refusal show nothing either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The story token is missing.
    Locked,
    NotEquipment,
    /// Not a set item ([`convert_accepts`]).
    NotSet,
    UnknownItem,
    /// The dismantle or convert table record is missing.
    NoTable,
    /// `blacklistedSets`.
    Blacklisted,
    NoRecipe,
    /// The cost in iron bits is more than the player has.
    Cost(u64),
    /// Too few of a reagent (record, count needed); Dynamite for dismantling.
    Missing(String, u32),
    /// The formula made nothing (the game logs "Item Set Formula (%s) failed").
    NoResult,
}

/// Dismantles `item` (module docs of grimdawn's `crafting.rs`):
/// equipment only, `unlocked` (the story's token), its cost in iron bits
/// and one Dynamite; gives the scrap picked by `itemWeights`, the item's
/// component back (else a rarity bonus roll). `Ok`: the cost and the results (the [`DismantleResult`] for the tab's result boxes,
/// given back through `InvRequest::Stow`); `Err`: why not ([`Refusal`]) and the item back.
#[allow(clippy::too_many_arguments)]
// `Err` hands the item back to the caller.
#[allow(clippy::result_large_err)]
pub fn dismantle_item(
    data: &gd_game_core::data::GameData,
    item: Item,
    unlocked: bool,
    params: &gd_sim::items::LootParams,
    gold: &mut u64,
    inv: &mut Inventory,
    stash: &mut Stash,
) -> Result<(u64, Dismantled), (Refusal, Item)> {
    let Some(table) = data.db.get(DISMANTLE) else {
        return Err((Refusal::NoTable, item));
    };
    let dynamite = data
        .db
        .get("records/game/gameengine.dbr")
        .and_then(|e| e.str("dynamite").map(str::to_owned))
        .unwrap_or_default();
    let idb = data.item_db();
    let resolved = gd_sim::items::resolve(&idb, &item);
    let item_level = resolved.as_ref().map_or(1, |r| r.item_level) as f32;
    let cost = dismantle_cost(table, item_level as u32);
    let names = vec![dynamite.clone()];
    let why = if !unlocked {
        Some(Refusal::Locked)
    } else if resolved
        .as_ref()
        .is_none_or(|r| !gd_sim::items::resolve::is_equipment_class(&r.class))
    {
        Some(Refusal::NotEquipment)
    } else if *gold < cost {
        Some(Refusal::Cost(cost))
    } else if count(&names, inv, stash) == 0 {
        Some(Refusal::Missing(dynamite, 1))
    } else {
        None
    };
    if let Some(w) = why {
        return Err((w, item));
    }
    *gold -= cost;
    take(&names, 1, inv, stash);
    let mut rng = gd_rules::Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
    let mut out = Dismantled::default();
    // Scrap: the index picked by `itemWeights`.
    let weights: Vec<u32> = table
        .f32s("itemWeights")
        .iter()
        .map(|w| *w as u32)
        .collect();
    let sum: u32 = weights.iter().sum();
    let roll = rng.next() % (sum + 1);
    let mut acc = 0;
    let n = weights
        .iter()
        .position(|w| {
            acc += w;
            roll < acc
        })
        .unwrap_or(0) as u32;
    if let Some(scrap) = table.str("itemGenerated").filter(|_| n > 0) {
        out.scrap = Some(Item {
            base_name: scrap.to_owned(),
            seed: gd_game_core::rng::u32(1..0x7FFF_FFFF),
            stack_count: n,
            ..Default::default()
        });
    }
    if !item.relic_name.is_empty() {
        // The component comes back instead of the bonus.
        out.bonus = Some(Item {
            base_name: item.relic_name.clone(),
            relic_bonus: item.relic_bonus.clone(),
            seed: item.relic_seed,
            stack_count: 1,
            ..Default::default()
        });
    } else if let Some(r) = &resolved {
        // The rarity picks the entry; other classes get no bonus.
        // `UIEnchanterDismantleTab::RollBonus`: an item with an ascendant bonus first
        // picks `ascendantItemBonus`, but the rarity branch always
        // replaces it, so that entry is never used.
        let key = match gd_sim::items::loot::rarity(r) {
            gd_sim::items::Classification::Common => Some("common"),
            gd_sim::items::Classification::Magical => Some("magic"),
            gd_sim::items::Classification::Rare => Some("rare"),
            gd_sim::items::Classification::Epic => Some("epic"),
            gd_sim::items::Classification::Legendary => Some("legendary"),
            _ => None,
        };
        let chance = key
            .and_then(|key| table.f32(&format!("{key}ItemBonusWeight")))
            .unwrap_or(0.0);
        if chance > 0.0
            && rng.unit() * 100.0 <= chance
            && let Some(t) = key.and_then(|key| table.str(&format!("{key}ItemBonus")))
        {
            out.bonus =
                gd_sim::items::loot::roll_table(&idb, t, params, &mut rng).map(|r| r.to_item());
        }
    }
    for o in out.scrap.iter().chain(&out.bonus) {
        info!(
            "dismantle: got {} x{}",
            o.base_name.rsplit('/').next().unwrap_or(""),
            o.stack_count
        );
    }
    Ok((cost, out))
}

/// What a dismantle gives: the scrap (`HudUI_DismantleResult1`, result box 1) and the bonus roll or the
/// returned component (`HudUI_DismantleResult2`, box 2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dismantled {
    pub scrap: Option<Item>,
    pub bonus: Option<Item>,
}

/// A dismantle's results for the dismantle tab's take-only result boxes
/// (`Player::GiveDismantledItemToCharacter` -> `ControllerPlayer::GiveDismantledItemToPlayer`
/// -> `GameUIInterface::SetDismantleResult1` / `SetDismantleResult2`); the window keeps them (UI state)
/// and, when the tab is not shown, gives them back with `InvRequest::Stow` (`GiveItemToPlayer`).
#[derive(Message, Clone, Debug)]
pub struct DismantleResult(pub Dismantled);

/// Puts a dismantle's results into the tab's result boxes `[dismantleBox1, dismantleBox2]`; what has to go
/// back to the player (`InvRequest::Stow`) is returned. `DismantleTab_SetResult1` (exe):
/// box 1 empty takes the scrap, else its item grows by it (`IncrementStack`, the new object removed: scrap
/// stacks across dismantles). Box 2 (`HudUI_DismantleResult2`) is set, no merge. PORT: box 1
/// holding another record, or box 2 holding an item, gives the old item back (the base box's set is not
/// traced).
pub fn receive_dismantled(boxes: &mut [Option<Item>; 2], d: Dismantled) -> Vec<Item> {
    let mut back = Vec::new();
    if let Some(scrap) = d.scrap {
        match &mut boxes[0] {
            Some(old) if old.base_name.eq_ignore_ascii_case(&scrap.base_name) => {
                old.stack_count += scrap.stack_count.max(1);
            }
            slot => back.extend(slot.replace(scrap)),
        }
    }
    if let Some(bonus) = d.bonus {
        back.extend(boxes[1].replace(bonus));
    }
    back
}

/// What a salvage keeps (the inventor's recover panel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SalvageKind {
    /// Keep the item, destroy its component.
    KeepItem,
    /// Keep the component, destroy the item.
    KeepAddon,
    /// Remove the augment.
    RemoveAugment,
}

/// Salvages `item` for its cost (`salvage_cost`); `Ok`: the cost and the
/// items that did not fit the bags (they stay in the slot); `Err`: the item
/// back (not enough gold, or nothing to salvage).
// `Err` hands the item back to the caller.
#[allow(clippy::result_large_err)]
pub fn salvage_item(
    data: &gd_game_core::data::GameData,
    mut item: Item,
    which: SalvageKind,
    gold: &mut u64,
    inv: &mut Inventory,
    req: &gd_game_core::inventory::Requirements,
) -> Result<(u64, Vec<Item>), Item> {
    let idb = data.item_db();
    let cost = salvage_cost(data, &idb, &item);
    let has_relic = !item.relic_name.is_empty();
    let has_augment = !item.augment_name.is_empty();
    let ok = *gold >= cost
        && match which {
            SalvageKind::RemoveAugment => has_augment,
            _ => has_relic,
        };
    if !ok {
        return Err(item);
    }
    *gold -= cost;
    let mut out = Vec::new();
    match which {
        SalvageKind::KeepAddon => {
            out.push(Item {
                base_name: item.relic_name.clone(),
                relic_bonus: item.relic_bonus.clone(),
                seed: item.relic_seed,
                var1: item.var1,
                stack_count: 1,
                ..Default::default()
            });
        }
        _ => {
            if which == SalvageKind::KeepItem || !has_augment {
                item.relic_name.clear();
                item.relic_bonus.clear();
                item.relic_seed = 0;
            }
            item.augment_name.clear();
            item.augment_seed = 0;
            item.unk_augment = 0;
            out.push(item);
        }
    }
    let mut left = Vec::new();
    for o in out {
        info!(
            "salvage: gave back {}",
            o.base_name.rsplit('/').next().unwrap_or("")
        );
        if inv.give(o.clone(), false, req).is_err() {
            left.push(o);
        }
    }
    inv.changed();
    Ok((cost, left))
}

/// Whether the player has a story token (case-insensitive).
fn has_token(rt: &gd_game_core::story::StoryRuntime, token: &str) -> bool {
    rt.story
        .borrow()
        .player
        .tokens
        .iter()
        .any(|t| t.eq_ignore_ascii_case(token))
}

/// `MainPlayerCanUseDismantle`: the table's `dismantleToken` story token
/// (unlocked without a story).
pub fn dismantle_unlocked(
    data: &gd_game_core::data::GameData,
    rt: Option<&gd_game_core::story::StoryRuntime>,
) -> bool {
    let token = data
        .db
        .get(DISMANTLE)
        .and_then(|t| t.str("dismantleToken").map(str::to_owned))
        .unwrap_or_else(|| "DISMANTLING_UNLOCKED".to_owned());
    rt.is_none_or(|r| has_token(r, &token))
}

/// `MainPlayerCanUseConvert`: the `gameengine.dbr` `convertToken` story
/// token (`InventorWindow::SetVisible`: the convert tab is enabled by it, like
/// dismantle / reroll by their tokens; unlocked without a story).
pub fn convert_unlocked(
    data: &gd_game_core::data::GameData,
    rt: Option<&gd_game_core::story::StoryRuntime>,
) -> bool {
    let token = data
        .db
        .get("records/game/gameengine.dbr")
        .and_then(|t| t.str("convertToken").map(str::to_owned))
        .unwrap_or_else(|| "CONVERTING_UNLOCKED".to_owned());
    rt.is_none_or(|r| has_token(r, &token))
}

/// Whether the convert tab's slot takes the item
/// (`UIEnchanterConvertTab::CanAcceptItem`: equipment whose record has an `itemSetName`).
pub fn convert_accepts(idb: &gd_sim::items::ItemDb, item: &Item) -> Result<(), String> {
    match gd_sim::items::resolve(idb, item) {
        Some(r)
            if gd_sim::items::resolve::is_equipment_class(&r.class)
                && r.item_set.as_deref().is_some_and(|s| !s.is_empty()) =>
        {
            Ok(())
        }
        _ => Err("Only set items can be converted".to_owned()),
    }
}

/// The item slot of the Inventor's tabs and the Runekeeper's window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotBox {
    /// The Runekeeper's item ascension slot.
    Ascension,
    /// The salvage tab (`enchanterRecoverTab`).
    Recover,
    Reroll,
    Convert,
    Dismantle,
}

/// Whether the slot takes the item (reroll tab
/// `UIEnchanterRerollTab::AcceptsItem`; the dismantle slot only needs equipment, PORT: the
/// game's slot hint says "greater than common quality" but its bonus
/// table has a common row). `Err`: the refusal text.
pub fn slot_accepts(idb: &gd_sim::items::ItemDb, slot: SlotBox, item: &Item) -> Result<(), String> {
    let equipment = |what: &str| match gd_sim::items::resolve(idb, item) {
        Some(r) if gd_sim::items::resolve::is_equipment_class(&r.class) => Ok(()),
        _ => Err(format!("Only equipment can be {what}")),
    };
    match slot {
        SlotBox::Ascension => crate::reroll::accepts(idb, item, true),
        SlotBox::Recover => equipment("salvaged"),
        SlotBox::Reroll => crate::reroll::accepts(idb, item, false),
        SlotBox::Convert => convert_accepts(idb, item),
        SlotBox::Dismantle => equipment("dismantled"),
    }
}

/// The dismantle prompt applies (`DismantleTab::OnButton`): the item has a
/// component or is Epic / Legendary.
pub fn dismantle_warns(idb: &gd_sim::items::ItemDb, item: &Item) -> bool {
    use gd_sim::items::resolve::Classification;
    !item.relic_name.is_empty()
        || gd_sim::items::resolve(idb, item).is_some_and(|r| {
            matches!(
                r.classification,
                Classification::Epic | Classification::Legendary
            )
        })
}

/// The convert formula for the item (`random`: the random-set mode; a
/// Legendary item then uses the legendary formula), see
/// `gd_sim::items::convert`.
pub fn convert_formula(
    data: &gd_game_core::data::GameData,
    idb: &gd_sim::items::ItemDb,
    item: &Item,
    random: bool,
) -> Option<gd_sim::items::convert::SetFormula> {
    let table = data.db.get(gd_sim::items::convert::TABLE)?;
    let legendary = gd_sim::items::resolve(idb, item)
        .is_some_and(|r| r.classification == gd_sim::items::Classification::Legendary);
    let name = gd_sim::items::convert::formula_record(table, random, legendary)?;
    data.db
        .get(name)
        .map(|r| gd_sim::items::convert::SetFormula::load(name, r))
}

/// The cost and reagents of converting the item at `level`.
pub fn convert_plan(
    data: &gd_game_core::data::GameData,
    idb: &gd_sim::items::ItemDb,
    item: &Item,
    random: bool,
    level: u32,
) -> Option<crate::reroll::Plan> {
    let f = convert_formula(data, idb, item, random)?;
    Some(crate::reroll::Plan {
        cost: f.cost(level),
        reagents: f.reagents,
    })
}

/// Converts `item` (the slot's) into another set piece (module
/// docs of `gd_sim::items::convert`): `unlocked` (the story's token), not a
/// `blacklistedSets` set, the formula's money and reagents. `Ok`: the
/// cost and the new item, which goes into the tab's slot:
/// `Player::GiveSetItemToCharacter` hands it to the HUD's `GameUIInterface::GiveSetItem` (GrimDawn.exe
/// `UIPlayerHud::GiveSetItem` -> `UIEnchanterConvertTab::SetResultItem`: the shown convert tab returns the slot's old
/// item to the player and holds the new one), so
/// full bags never matter there; only with the tab hidden would it be `GiveItemToPlayer` (equip-empty, bags,
/// else tutorial page 33, `SendDropItemRandom` and the inventory-full sound), which the button never reaches.
/// `Err`: why not ([`Refusal`]) and the item back.
#[allow(clippy::too_many_arguments)]
// `Err` hands the item back to the caller.
#[allow(clippy::result_large_err)]
pub fn convert_item(
    data: &gd_game_core::data::GameData,
    item: Item,
    random: bool,
    unlocked: bool,
    level: u32,
    gold: &mut u64,
    inv: &mut Inventory,
    stash: &mut Stash,
) -> Result<(u64, Item), (Refusal, Item)> {
    let idb = data.item_db();
    if !unlocked {
        return Err((Refusal::Locked, item));
    }
    if convert_accepts(&idb, &item).is_err() {
        return Err((Refusal::NotSet, item));
    }
    let Some(r) = gd_sim::items::resolve(&idb, &item) else {
        return Err((Refusal::UnknownItem, item));
    };
    let set = r.item_set.clone().unwrap_or_default();
    let Some(table) = data.db.get(gd_sim::items::convert::TABLE) else {
        return Err((Refusal::NoTable, item));
    };
    if gd_sim::items::convert::blacklisted(table, &set) {
        // The game's `tagConvertBlacklist` dialog.
        return Err((Refusal::Blacklisted, item));
    }
    let Some(f) = convert_formula(data, &idb, &item, random) else {
        return Err((Refusal::NoRecipe, item));
    };
    let cost = f.cost(level);
    if *gold < cost {
        return Err((Refusal::Cost(cost), item));
    }
    if let Some((n, q)) = summed_reagents(f.reagents.iter().map(|(n, q)| (n, *q)))
        .into_iter()
        .find(|(n, q)| count(std::slice::from_ref(*n), inv, stash) < *q)
    {
        return Err((Refusal::Missing(n.clone(), q), item));
    }
    let mut rng = gd_rules::Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
    let Some(name) =
        gd_sim::items::convert::convert(&idb, &f, &item.base_name, &set, r.item_level, &mut rng)
    else {
        // The game logs "Item Set Formula (%s) failed" and keeps the item.
        return Err((Refusal::NoResult, item));
    };
    *gold -= cost;
    for (n, q) in &f.reagents {
        take(std::slice::from_ref(n), *q, inv, stash);
    }
    let made = Item {
        base_name: name,
        seed: gd_game_core::rng::u32(1..0x7FFF_FFFF),
        stack_count: 1,
        ..Default::default()
    };
    info!(
        "convert: {} -> {}",
        item.base_name.rsplit('/').next().unwrap_or(""),
        made.base_name.rsplit('/').next().unwrap_or("")
    );
    inv.changed();
    Ok((cost, made))
}

/// `MainPlayerCanUseReroll`: the `gameengine.dbr` `rerollToken` story token
/// (exact match; unlocked when the record names none, locked without a
/// story).
pub fn reroll_unlocked(
    data: &gd_game_core::data::GameData,
    rt: Option<&gd_game_core::story::StoryRuntime>,
) -> bool {
    let token = data
        .db
        .get("records/game/gameengine.dbr")
        .and_then(|r| r.str("rerollToken").map(str::to_owned))
        .unwrap_or_default();
    token.is_empty() || rt.is_some_and(|r| r.story.borrow().player.tokens.contains(&token))
}

/// The inventor's, blacksmith's and enchanter's requests. The windows only
/// decide what a click means and write one of these; [`apply_inventor`]
/// applies it (the rules above) and answers with a [`SlotReturn`]. Same
/// shape as `inventory::ApplyRequest`.
pub struct CraftingPlugin;

impl Plugin for CraftingPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CraftRequest>()
            .add_message::<DismantleRequest>()
            .add_message::<DismantleResult>()
            .add_message::<SalvageRequest>()
            .add_message::<ConvertRequest>()
            .add_message::<StageItem>()
            .add_message::<SlotReturn>()
            .add_systems(
                Update,
                apply_inventor
                    .run_if(resource_exists::<Inventory>)
                    .run_if(resource_exists::<Stash>)
                    .run_if(resource_exists::<gd_game_core::progress::PlayerProgress>)
                    .run_if(resource_exists::<gd_game_core::data::GameData>)
                    .after(gd_game_core::inventory::InventorySet)
                    .before(gd_game_core::player::PlayerSet),
            );
    }
}

/// The crafter's craft button: craft `bp` (`enhancement`: the crafter's).
#[derive(Message, Clone, Debug)]
pub struct CraftRequest {
    pub bp: Blueprint,
    pub enhancement: Option<String>,
}

/// The dismantle tab's button on the slot's `item`.
#[derive(Message, Clone, Debug)]
pub struct DismantleRequest(pub Item);

/// The salvage tab's button (`kind`) on the slot's `item`.
#[derive(Message, Clone, Debug)]
pub struct SalvageRequest {
    pub item: Item,
    pub kind: SalvageKind,
}

/// The convert tab's button on the slot's `item`; `random`: another set
/// (else the same); `test`: the `GD_CONVERT` hook (the token is not needed).
#[derive(Message, Clone, Debug)]
pub struct ConvertRequest {
    pub item: Item,
    pub random: bool,
    pub test: bool,
}

/// Which bag item a test hook wants in the window's slot.
#[derive(Clone, Debug)]
pub enum Stage {
    /// The first bag item whose record name contains this (empty: the first).
    Contains(String),
    /// The first item the convert tab takes; the money and reagents of its
    /// formula are added to the player (`random`: the random-set formula).
    Convertible { random: bool },
}

/// Test hooks (`GD_DISMANTLE`, `GD_SALVAGE`, `GD_CONVERT`, `GD_REROLL`): takes
/// a bag item out for the window's slot; the answer is a [`SlotReturn`] with
/// `go` as given (the hook then presses the button).
#[derive(Message, Clone, Debug)]
pub struct StageItem {
    pub which: Stage,
    pub go: bool,
}

/// What a request leaves for the window: the item that goes back into the
/// slot (a refused item, what did not fit the bags, a rerolled item; None:
/// the slot stays empty) and `go` for a staged item. The window rebuilds.
#[derive(Message, Clone, Debug, Default)]
pub struct SlotReturn {
    pub item: Option<Item>,
    pub go: bool,
}

#[allow(clippy::too_many_arguments)]
fn apply_inventor(
    mut craft_msgs: MessageReader<CraftRequest>,
    mut dismantle_msgs: MessageReader<DismantleRequest>,
    mut salvage_msgs: MessageReader<SalvageRequest>,
    mut convert_msgs: MessageReader<ConvertRequest>,
    mut stage_msgs: MessageReader<StageItem>,
    (mut back, mut results, mut stats): (
        MessageWriter<SlotReturn>,
        MessageWriter<DismantleResult>,
        MessageWriter<crate::play_stats::PlayStat>,
    ),
    data: Res<gd_game_core::data::GameData>,
    mut inv: ResMut<Inventory>,
    mut stash: ResMut<Stash>,
    mut progress: ResMut<gd_game_core::progress::PlayerProgress>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    session: crate::session::Session,
    players: Query<&gd_game_core::inventory::Requirements, With<gd_game_core::player::Player>>,
) {
    let Ok(req) = players.single() else {
        return;
    };
    let level = progress.progress.level.max(1) as u32;
    for m in stage_msgs.read() {
        let idb = data.item_db();
        let found = inv.bags.iter_mut().find_map(|b| {
            let i = b.items.iter().position(|p| match &m.which {
                Stage::Contains(s) => p.item.base_name.contains(s.as_str()),
                Stage::Convertible { .. } => convert_accepts(&idb, &p.item).is_ok(),
            })?;
            Some(b.items.remove(i))
        });
        let Some(p) = found else { continue };
        if let Stage::Convertible { random } = m.which
            && let Some(plan) = convert_plan(&data, &idb, &p.item, random, level)
        {
            crate::loot::add_gold(&mut progress.gold, plan.cost as i64);
            for (r, q) in &plan.reagents {
                for _ in 0..*q {
                    inv.add_to_bags(Item {
                        base_name: r.clone(),
                        stack_count: 1,
                        seed: 1,
                        ..Default::default()
                    });
                }
            }
        }
        inv.changed();
        back.write(SlotReturn {
            item: Some(p.item),
            go: m.go,
        });
    }
    for m in craft_msgs.read() {
        let r = &m.bp;
        let mut gold = progress.gold;
        match craft(
            &data,
            m.enhancement.as_deref(),
            r,
            &mut gold,
            &session.loot(level, level),
            &mut inv,
            &mut stash,
            req,
        ) {
            Ok(items) => {
                progress.gold = gold;
                stats.write(crate::play_stats::PlayStat::Crafted(
                    items[0].base_name.clone(),
                ));
                let names: Vec<String> = items
                    .iter()
                    .map(|i| {
                        format!(
                            "{} [{}]",
                            i.base_name.rsplit('/').next().unwrap_or(""),
                            i.modifier_name.rsplit('/').next().unwrap_or("")
                        )
                    })
                    .collect();
                info!("crafting: made {names:?} for {}", r.cost(level));
            }
            Err(e) => info!(
                "crafting: cannot craft {}: {e}",
                r.record.rsplit('/').next().unwrap_or("")
            ),
        }
        back.write(SlotReturn::default());
    }
    for DismantleRequest(item) in dismantle_msgs.read() {
        let unlocked = dismantle_unlocked(&data, rt.as_deref());
        let mut ret = SlotReturn::default();
        match dismantle_item(
            &data,
            item.clone(),
            unlocked,
            &session.loot(level, level),
            &mut progress.gold,
            &mut inv,
            &mut stash,
        ) {
            Ok((cost, out)) => {
                info!("dismantle: for {cost} iron bits");
                results.write(DismantleResult(out));
            }
            Err((why, item)) => {
                info!("dismantle: refused, {why:?}");
                ret.item = Some(item);
            }
        }
        back.write(ret);
    }
    for m in salvage_msgs.read() {
        let mut ret = SlotReturn::default();
        match salvage_item(
            &data,
            m.item.clone(),
            m.kind,
            &mut progress.gold,
            &mut inv,
            req,
        ) {
            Ok((cost, left)) => {
                info!("salvage: for {cost} iron bits");
                ret.item = left.into_iter().next();
            }
            Err(item) => ret.item = Some(item),
        }
        back.write(ret);
    }
    for m in convert_msgs.read() {
        let unlocked = m.test || convert_unlocked(&data, rt.as_deref());
        let mut ret = SlotReturn::default();
        match convert_item(
            &data,
            m.item.clone(),
            m.random,
            unlocked,
            level,
            &mut progress.gold,
            &mut inv,
            &mut stash,
        ) {
            Ok((cost, made)) => {
                info!("convert: for {cost} iron bits");
                ret.item = Some(made);
            }
            Err((why, item)) => {
                info!("convert: refused, {why:?}");
                ret.item = Some(item);
            }
        }
        back.write(ret);
    }
}
