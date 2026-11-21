//! The player's inventory and equipment for this session.
//!
//! Loaded from the save (or the starting gear), changed by pickups and the character window, written back by
//! `crate::save` (equipment, both weapon sets, the bags, [`Inventory::held`], [`Inventory::unplaced`]). Grid
//! rules: `gd_sim::items::inventory` (32-px cells, footprint = bitmap size, column-major auto placement). Two
//! weapon sets: the active one in `equipped`, the other in `other_weapons` (`crate::weapon_sets`). The worn
//! equipment is the player's [`Loadout::items`]: [`sync_equipment`] copies every change there; the game rebuilds
//! the body on [`EquipmentChanged`].
//!
//! Components, augments and single-piece transmuters (`CreateSecondaryCursorHandler`): a right click
//! selects one (it stays in its cell), the next click ([`ApplyRequest::Click`]) on a bag, stash or worn item
//! uses it there ([`Inventory::apply_click`], rules [`Inventory::apply_check`]); dragging one onto an item only
//! swaps them. PORT: while the transfer stash is blocked (`crate::stash` module doc) a component / augment
//! from elsewhere is not used on a transfer-stash item (it would be lost with the unsaved store):
//! [`apply_requests`] refuses that click like a refused transfer, and the mode stays.
//!
//! Detached items (`EquipmentCtrl::AttributesHaveChanged` / `Sift`, every 250 ms): a worn item whose
//! requirements fail (level and attributes with the gear's reductions, `gd_sim::items::req_reduction`) or an
//! off-hand one-hander whose dual-wield bit is gone stays in its slot (drawn) but gives no stats, skills, set
//! count or components ([`sift_equipment`]; the save's `attached` flag).
//!
//! Giving an item ([`Inventory::give`], `ControllerPlayer::GiveItemToPlayer`; nothing takes it:
//! `crate::loot::give_item`):
//! 1. [`Inventory::auto_insert`]: an EMPTY slot of its class with requirements met, in [`AUTO_INSERT_ORDER`],
//!    then the ACTIVE set's main hand, else off hand (strict hand rule). Nothing is swapped.
//! 2. [`Inventory::add_to_bags`]: OneShot use, (ground pickups) an incomplete component joins a partial, the
//!    stacks of its record, then a new cell: main bag, selected bag ([`Inventory::bag_tab`]), the others.
//!
//! The hand rule (`WeaponSet__PlaceRight/Left`; table `gd_sim::items::inventory::hand_fits`): melee +
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
//! With an item held, a right press on a bag is the merchant quick sale ([`crate::trade::SellHeld`]) else
//! [`Inventory::cancel_held`]. Right press on a worn item with a merchant open sells it
//! ([`crate::trade::SellEquipped`]).
//!
//! Item-slot windows give items back through [`InvRequest::Stow`] = [`crate::loot::give_item`]; never lost.
//! Stacks: `crate::stack`.
use crate::combat::{Combatant, Dead};
use crate::data::GameData;
use crate::player::{Loadout, Player, PlayerSet, ProfileDirty};
use crate::progress::PlayerProgress;
use crate::rules::CombatRules;
use crate::session::LoadedCharacter;
use crate::stash::Stash;
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_sim::items::inventory::{
    EquipSlot, Footprint, Sack, bitmap_footprint, hand_types, is_two_handed_type, shard_target,
    slots_for_class, sort_rank, sort_sack,
};
use gd_sim::items::{ItemDb, ResolvedItem, resolve};
use std::collections::HashMap;
use std::sync::Arc;

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<EquipmentChanged>()
            .add_message::<NewItemSeen>()
            .add_systems(
                Update,
                clear_fresh
                    .run_if(resource_exists::<Inventory>)
                    .after(InventorySet)
                    .before(PlayerSet),
            )
            .add_message::<ApplyRequest>()
            .add_message::<SortBag>()
            .add_message::<InvRequest>()
            .add_systems(
                Update,
                apply_inv_requests
                    .run_if(resource_exists::<Inventory>)
                    .after(InventorySet)
                    .before(PlayerSet),
            )
            .add_systems(
                Update,
                sort_bags
                    .run_if(resource_exists::<Inventory>)
                    .after(InventorySet)
                    .before(PlayerSet),
            )
            .add_systems(
                Update,
                apply_requests
                    .run_if(resource_exists::<Inventory>)
                    .after(InventorySet)
                    .before(PlayerSet),
            )
            .add_systems(
                Update,
                (
                    init_inventory.in_set(InventoryStage::Init),
                    sync_requirements
                        .in_set(InventoryStage::Requirements)
                        .run_if(resource_exists::<Inventory>),
                    sift_equipment.run_if(resource_exists::<Inventory>),
                    sync_equipment
                        .in_set(InventoryStage::Equipment)
                        .run_if(resource_exists::<Inventory>)
                        .run_if(resource_exists::<CombatRules>),
                )
                    .chain()
                    .in_set(InventorySet)
                    .before(PlayerSet),
            )
            .add_systems(
                Update,
                play_sounds
                    .after(InventorySet)
                    .run_if(resource_exists::<Inventory>),
            );
    }
}

/// The inventory's systems: the game's inventory input runs between
/// [`init_inventory`] and [`sync_equipment`] (`trade.rs` runs after the set).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InventorySet;

/// The UI's answer in the apply mode (enchant / component use, `Inventory::applying`): the window
/// sends it, [`apply_requests`] applies it.
#[derive(Message, Clone, Copy, Debug)]
pub enum ApplyRequest {
    /// Ends the mode (`Inventory::cancel_apply`): Escape without a question
    /// up, the character or the stash window closing.
    Cancel,
    /// Yes / No to the confirmation (`Inventory::answer_apply`).
    Answer(bool),
    /// Right click on an item: select it when it is a component / augment
    /// (`Inventory::start_apply`; the other right-click uses are [`InvRequest::Bag`]).
    Start(ItemLoc),
    /// Left or right click on `target` (None: an empty equipment slot) in
    /// the mode (`Inventory::apply_click`; the handlers'
    /// `Secondary{Inventory,Equip,Stash,Transfer}Activate` only call the
    /// `Primary…` one, and the bag sack's click handler sub_1401EA910 in
    /// Grim Dawn.exe gives both buttons to an active handler first).
    Click(Option<ItemLoc>),
}

fn apply_requests(
    mut msgs: MessageReader<ApplyRequest>,
    mut inv: ResMut<Inventory>,
    // The stash is absent until `init_stash` ran; the answer then applies to the bags alone.
    mut stash: Option<ResMut<crate::stash::Stash>>,
    reqs: Query<&Requirements, With<Player>>,
    (shared, choice, save_dir, mut error): (
        Res<crate::stash::SharedFiles>,
        Res<crate::session::Choice>,
        Res<crate::session::SaveDir>,
        MessageWriter<crate::stash::QuickActionError>,
    ),
) {
    let Ok(req) = reqs.single() else {
        return;
    };
    let transfer_open = shared.accepts(&choice, &save_dir, gd_formats::save::Shared::Transfer);
    for m in msgs.read() {
        match *m {
            ApplyRequest::Cancel => inv.cancel_apply(),
            ApplyRequest::Answer(yes) => inv.answer_apply(yes, stash.as_deref_mut(), req),
            ApplyRequest::Start(src) => {
                inv.start_apply(src, stash.as_deref());
            }
            // A component from outside the blocked transfer stash onto an item in it (module doc); the
            // confirmation is only ever asked by a click, so this covers its Yes too.
            ApplyRequest::Click(target) if inv.apply_blocked(target, transfer_open) => {
                error.write(crate::stash::QuickActionError { notify: true });
            }
            ApplyRequest::Click(target) => inv.apply_click(target, stash.as_deref_mut(), req),
        }
    }
}

/// An item's (item-type rank, display name) for the sort (`gd_sim::items::inventory::sort_sack`); an unknown
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

/// Sort bag `b` (`PlayerInventoryCtrl::SortPrimarySack` / `SortSecondarySack` → `InventorySack::Sort`).
#[derive(Message, Clone, Copy, Debug)]
pub struct SortBag(pub usize);

fn sort_bags(
    mut msgs: MessageReader<SortBag>,
    mut inv: ResMut<Inventory>,
    data: Res<crate::data::GameData>,
) {
    for SortBag(b) in msgs.read() {
        if *b >= inv.bags.len() {
            continue;
        }
        let mut bag = std::mem::replace(&mut inv.bags[*b], Sack::new(0, 0, true));
        let tags = data.tags();
        let sorted = sort_sack(&mut bag, |it| sort_key(&inv, &tags, it));
        inv.bags[*b] = bag;
        if sorted {
            inv.changed();
        }
    }
}

/// A click in the character window, resolved by the UI from its layout to
/// the bag cell / equipment slot under the cursor; [`apply_inv_requests`]
/// applies the rule. The cursor item stays state ([`Inventory::held`]), the
/// UI only asks for the transitions and reads the state next frame.
#[derive(Message, Clone, Debug)]
pub enum InvRequest {
    /// The window is closed: ends the apply mode and puts the held item back
    /// in a bag (it stays in hand when they are full).
    Closed,
    /// The extra-bag tab button.
    BagTab(usize),
    /// A click on the item (or empty cell) `cell` of bag `bag`; `cursor` is
    /// the cursor in cells from the grid's top-left (the drop footprint is
    /// centred on it, `gd_sim::items::inventory::drop_cell`). Left: pick up / place / swap with the
    /// held item. Right: select a component, learn a blueprint, use a
    /// consumable or equip the item.
    Bag {
        bag: usize,
        cell: (u32, u32),
        cursor: Vec2,
        right: bool,
    },
    /// A click on an equipment slot (not in the apply mode): left swaps with
    /// the held item; right with an item held as left, else nothing (module doc).
    Slot { slot: EquipSlot, right: bool },
    /// A held item clicked on the paper doll.
    Doll,
    /// An EMPTY item-slot window slot (Inventor, Runekeeper, crafter) takes the bag item at `cell` of `bag`
    /// (Shift + left click; the UI refuses an occupied slot, `AscensionItemBox_AddItem` @0x1401B1950).
    /// Answered with a `crafting::SlotReturn` holding the item.
    ToWindowSlot { bag: usize, cell: (u32, u32) },
    /// The window's slot gives `item` back to the player (window closed, tab switched, Shift + left on the
    /// slot): `ItemBox_ReturnItemToPlayer` @0x1401AEB20 = [`crate::loot::give_item`] (module doc).
    Stow(Item),
    /// A click on the window's slot: the cursor item is exchanged with
    /// `give` (the slot's item, None for an empty slot); the old cursor item
    /// comes back as a `crafting::SlotReturn`.
    Exchange(Option<Item>),
}

/// The slots a held item clicked on the paper doll tries, in order.
/// GrimDawn.exe sub_14023A7C0 (the character window's scene view, vtable
/// +152 from its mouse handler sub_14023A5F0): the held item's equip type picks
/// one slot of the window (sub_1401E3D80) and hands it the click; a ring the
/// first ring slot when that is empty, else the second; a weapon or off-hand
/// item the weapon pair (sub_1401BE6E0: main hand first, then the off hand;
/// a `WeaponArmor_Shield` the off hand first); other types (components) nothing.
fn doll_slots(inv: &Inventory, item: &Item) -> Vec<EquipSlot> {
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
        c => gd_sim::items::inventory::slots_for_class(c)
            .first()
            .copied()
            .into_iter()
            .collect(),
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_inv_requests(
    mut commands: Commands,
    mut msgs: MessageReader<InvRequest>,
    mut inv: ResMut<Inventory>,
    mut stash: Option<ResMut<crate::stash::Stash>>,
    players: Query<(&Transform, &crate::character::Character, &Requirements), With<Player>>,
    nav: Option<Res<crate::nav::Nav>>,
    mut back: MessageWriter<crate::crafting::SlotReturn>,
    data: Res<crate::data::GameData>,
    mut cinematic: MessageWriter<crate::story::CinematicText>,
) {
    for m in msgs.read() {
        match m {
            InvRequest::Closed => {
                inv.cancel_apply();
                inv.destroying = None;
                // Closing the window drops nothing: the held item goes back to a bag.
                if let Some(h) = inv.held.take()
                    && !inv.add_to_bags(h.clone())
                {
                    inv.held = Some(h);
                }
            }
            InvRequest::BagTab(b) => {
                inv.bag_tab = *b;
                inv.changed();
            }
            InvRequest::Stow(item) => stow(
                &mut commands,
                &mut inv,
                &players,
                &data,
                nav.as_deref(),
                item.clone(),
            ),
            // A right press on a bag with an item held: the item-move handler's `SecondaryInventoryActivate`
            // @0x180185360 (the merchant's quick sale is the UI's `trade::SellHeld`): `Cancel`.
            InvRequest::Bag { right: true, .. } if inv.held.is_some() => {
                if let Err(it) = inv.cancel_held(stash.as_deref_mut()) {
                    stow(&mut commands, &mut inv, &players, &data, nav.as_deref(), it);
                }
            }
            InvRequest::Exchange(give) => {
                let old = std::mem::replace(&mut inv.held, give.clone());
                back.write(crate::crafting::SlotReturn {
                    item: old,
                    go: false,
                });
                inv.changed();
            }
            InvRequest::ToWindowSlot { bag, cell } => {
                let placed = inv
                    .bags
                    .get(*bag)
                    .and_then(|b| b.item_at(cell.0, cell.1))
                    .cloned();
                let Some(placed) = placed else {
                    // Gone since the click: the slot stays empty.
                    back.write(crate::crafting::SlotReturn {
                        item: None,
                        go: false,
                    });
                    continue;
                };
                inv.bags[*bag]
                    .items
                    .retain(|q| !(q.x == placed.x && q.y == placed.y));
                back.write(crate::crafting::SlotReturn {
                    item: Some(placed.item),
                    go: false,
                });
                inv.changed();
            }
            // The rest needs the player's equip requirements.
            rest => {
                let Ok((.., req)) = players.single() else {
                    continue;
                };
                match rest {
                    InvRequest::Bag {
                        bag,
                        cell,
                        right: true,
                        ..
                    } => {
                        if let Some(name) =
                            right_click_bag(&mut inv, stash.as_deref_mut(), *bag, *cell, req, &data)
                        {
                            // `ItemArtifactFormula::Use` @0x180332eb0: type 2,
                            // `tagFormulaLearned` over `tagFormulaLearn01(name)`. PORT: the name is not rarity coloured.
                            cinematic.write(crate::story::CinematicText {
                                head: "tagFormulaLearned".into(),
                                sub: "tagFormulaLearn01".into(),
                                sub_args: vec![crate::floating::TextArg::Tag(name)],
                                kind: 2,
                                front: false,
                            });
                        }
                    }
                    InvRequest::Bag {
                        bag, cursor, cell, ..
                    } => match inv.held.take() {
                        None => {
                            if let Some(placed) = inv
                                .bags
                                .get(*bag)
                                .and_then(|b| b.item_at(cell.0, cell.1))
                                .cloned()
                            {
                                inv.bags[*bag]
                                    .items
                                    .retain(|q| !(q.x == placed.x && q.y == placed.y));
                                inv.held_from = Some((
                                    HeldFrom::At(ItemLoc::Bag {
                                        bag: *bag,
                                        x: placed.x,
                                        y: placed.y,
                                    }),
                                    placed.item.clone(),
                                ));
                                inv.held = Some(placed.item);
                                inv.changed();
                            }
                        }
                        Some(h) => {
                            let size = inv
                                .info(&h)
                                .map(|i| i.size)
                                .unwrap_or(Footprint { w: 1, h: 1 });
                            let drop = crate::stash::ui_drop_sound(&data.db, &h);
                            // The footprint is centred on the cursor; it may
                            // overlap at most one item, which is swapped into the hand
                            // or, the same record with room, takes the stack (`crate::stack`).
                            let max =
                                crate::stack::max_of(&inv, &h, crate::stack::engine_max(&data.db));
                            let Some(sack) = inv.bags.get_mut(*bag) else {
                                inv.held = Some(h);
                                continue;
                            };
                            match crate::stack::place_stacking(sack, h, size, *cursor, max) {
                                Ok(under) => {
                                    inv.sounds.extend(drop);
                                    inv.held = under;
                                    inv.changed();
                                }
                                Err(h) => inv.held = Some(h),
                            }
                        }
                    },
                    InvRequest::Slot { slot, right } => {
                        let i = slot_index(*slot);
                        match inv.held.take() {
                            // A right press on a filled slot with an empty cursor: the item's secondary
                            // handler, none for equipment, so nothing (the merchant sale is the UI's).
                            None if *right => {}
                            None => {
                                if let Some(it) = inv.equipped[i].take() {
                                    inv.held_from =
                                        Some((HeldFrom::At(ItemLoc::Equip(i)), it.clone()));
                                    inv.held = Some(it);
                                    inv.mark_equipment_dirty();
                                    inv.changed();
                                }
                            }
                            Some(h) => {
                                let drop = crate::stash::ui_drop_sound(&data.db, &h);
                                match inv.equip(h, *slot, req) {
                                    Ok(old) => {
                                        inv.sounds.extend(drop);
                                        inv.held = old;
                                    }
                                    Err(h) => inv.held = Some(h),
                                }
                            }
                        }
                    }
                    InvRequest::Doll => {
                        if let Some(h) = inv.held.take() {
                            let mut item = Some(h);
                            for slot in doll_slots(&inv, item.as_ref().unwrap()) {
                                let drop =
                                    crate::stash::ui_drop_sound(&data.db, item.as_ref().unwrap());
                                match inv.equip(item.take().unwrap(), slot, req) {
                                    Ok(old) => {
                                        inv.sounds.extend(drop);
                                        inv.held = old;
                                        inv.changed();
                                        break;
                                    }
                                    Err(back) => item = Some(back),
                                }
                            }
                            if item.is_some() {
                                inv.held = item;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// `ControllerPlayer::GiveItemToPlayer(id, false)` from the player's centre ([`crate::loot::give_item`]); with
/// no level loaded (nothing to throw onto) the item stays on the cursor rather than being lost.
fn stow(
    commands: &mut Commands,
    inv: &mut Inventory,
    players: &Query<(&Transform, &crate::character::Character, &Requirements), With<Player>>,
    data: &crate::data::GameData,
    nav: Option<&crate::nav::Nav>,
    item: Item,
) {
    let Ok((t, ch, req)) = players.single() else {
        // No player (nothing to equip on or throw from): the bags, else the cursor.
        if !inv.add_to_bags(item.clone()) {
            inv.held.get_or_insert(item);
        }
        return;
    };
    let center = t.translation + crate::monster_skills::character_center(data, ch);
    match nav {
        Some(_) => {
            let to = crate::loot::Receiver { center, req };
            crate::loot::give_item(commands, inv, data, to, item, false);
        }
        None => {
            if let Err(it) = inv.give(item, false, req) {
                warn!(
                    "no room and no ground: {} stays on the cursor",
                    it.base_name
                );
                inv.held.get_or_insert(it);
            }
        }
    }
    inv.changed();
}

/// Right click on the bag item at `cell` of `bag`.
fn right_click_bag(
    inv: &mut Inventory,
    mut stash: Option<&mut crate::stash::Stash>,
    bag: usize,
    cell: (u32, u32),
    req: &Requirements,
    data: &crate::data::GameData,
) -> Option<String> {
    // A held item's right press is its handler's (`InvRequest::Bag` arm, module doc).
    if inv.held.is_some() {
        return None;
    }
    let placed = inv
        .bags
        .get(bag)
        .and_then(|b| b.item_at(cell.0, cell.1))
        .cloned()?;
    // A component or augment is selected for use on an item.
    if inv.start_apply(
        ItemLoc::Bag {
            bag,
            x: placed.x,
            y: placed.y,
        },
        stash.as_deref(),
    ) {
        return None;
    }
    let info = inv.info(&placed.item)?;
    let class = info.resolved.class.as_str();
    // The item-use handler of a scroll / formula / note / unlock exists only when its requirements are met
    // (`crate::consumables::use_gated`); without it `SmartAutoInsert` refuses it: nothing happens.
    if crate::consumables::use_gated(class)
        && let Some(why) = inv.requirement_failure(&placed.item, req)
    {
        info!("cannot use {}: {why}", placed.item.base_name);
        return None;
    }
    // Blueprints are learned (`formulas.gst`) and used up; a known or invalid one is kept
    // (`ItemArtifactFormula::AllowUse` @0x180333300, shared by every formula class: refused when
    // `IsItemInFormulas` or not `IsBluePrintValid`, `gd_sim::items::crafting::formula_valid`; refused ->
    // `PlayerInventoryCtrl::UseItem` @0x1803f42c0 plays the "cannot" sound, `genericErrorSound`).
    if crate::consumables::is_formula(class) {
        let valid = data
            .db
            .get(&placed.item.base_name)
            .is_some_and(|r| gd_sim::items::crafting::formula_valid(&*data.db, r));
        if valid
            && let Some(st) = stash.as_mut()
            && st.learn(&placed.item.base_name)
        {
            info!("learned blueprint {}", placed.item.base_name);
            inv.bags[bag]
                .items
                .retain(|q| !(q.x == placed.x && q.y == placed.y));
            inv.changed();
            inv.sounds.extend(crate::stash::learn_formula_sound(data));
            // What it makes (`gd_sim::items::crafting::made_item`), named as an item.
            let made = data
                .db
                .get(&placed.item.base_name)
                .and_then(|r| gd_sim::items::crafting::made_item(r).map(str::to_owned))
                .unwrap_or_else(|| placed.item.base_name.clone());
            let item = Item {
                base_name: made.clone(),
                stack_count: 1,
                ..Default::default()
            };
            let name = inv
                .info(&item)
                .map(|i| gd_sim::items::naming::item_name(&data.tags(), &i.resolved))
                .filter(|n| !n.is_empty())
                .unwrap_or(made);
            return Some(name);
        } else {
            info!(
                "blueprint refused (known or invalid): {}",
                placed.item.base_name
            );
            inv.sounds.extend(crate::stash::error_sound(data));
        }
        return None;
    }
    // The tonics' and writs' handlers may ask first (`consumables::ask_use`).
    if crate::consumables::asks(class) {
        inv.use_ask = Some(crate::consumables::UseAsk {
            bag,
            x: placed.x,
            y: placed.y,
            record: placed.item.base_name.clone(),
            class: class.to_owned(),
            question: None,
        });
        return None;
    }
    // Consumables are used (`consumables.rs`): one of the stack
    // goes, except a single `infiniteUse` skill item.
    if crate::consumables::is_usable(class) {
        let mut one = placed.item.clone();
        one.stack_count = 1;
        let consumed = inv
            .db()
            .get(&placed.item.base_name)
            .is_none_or(|r| crate::consumables::consumed(r, placed.item.stack_count));
        if consumed {
            let idx = inv.bags[bag]
                .items
                .iter()
                .position(|q| q.x == placed.x && q.y == placed.y);
            if let Some(i) = idx {
                if inv.bags[bag].items[i].item.stack_count > 1 {
                    inv.bags[bag].items[i].item.stack_count -= 1;
                } else {
                    inv.bags[bag].items.remove(i);
                }
            }
        }
        inv.used.push((one, Some((bag, placed.x, placed.y))));
        inv.changed();
        return None;
    }
    // SmartAutoInsert: the item leaves its cell first (the displaced items may take it), and returns
    // there when nothing is equipped.
    inv.smart_slot(&placed.item, req)?;
    inv.bags[bag]
        .items
        .retain(|q| !(q.x == placed.x && q.y == placed.y));
    let (back, sound) = match inv.smart_insert(placed.item.clone(), req) {
        SmartInsert::Equipped => (None, false),
        SmartInsert::Refused(it) => (Some(it), false),
        SmartInsert::NoRoom(it) => (Some(it), true),
    };
    if let Some(it) = back {
        inv.bags[bag].add_at(it, placed.x, placed.y, placed.size);
    }
    if sound {
        inv.sounds.extend(crate::stash::error_sound(data));
    }
    inv.changed();
    None
}

/// What [`Inventory::smart_insert`] did.
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

/// Passes changed equipment to the living player's loadout and rebuilds its
/// profile (life keeps its fraction, at the current level; `ProfileDirty`).
/// The skill bonuses that came off release pets ([`crate::pets::AugmentsRemoved`]).
pub fn sync_equipment(
    mut commands: Commands,
    mut inv: ResMut<Inventory>,
    data: Res<crate::data::GameData>,
    mut players: Query<(Entity, &mut Loadout), (With<Player>, Without<Dead>)>,
    mut changed: MessageWriter<EquipmentChanged>,
    mut removed: MessageWriter<crate::pets::AugmentsRemoved>,
) {
    if !inv.equipment_dirty {
        return;
    }
    let Ok((player, mut loadout)) = players.single_mut() else {
        return;
    };
    inv.equipment_dirty = false;
    let worn = inv.worn();
    let lost = crate::pets::removed_augments(&data.db, player, &loadout.items, &worn);
    if !lost.is_empty() {
        removed.write(lost);
    }
    loadout.set_items(&data.db, worn);
    commands.entity(player).insert(ProfileDirty);
    changed.write(EquipmentChanged { player });
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
fn sack_cell<'a>(
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

/// Resolved item data the UI and rules need.
pub struct ItemInfo {
    pub resolved: ResolvedItem,
    pub size: Footprint,
}

/// Where the held item came from (`CursorHandlerItemMove` +68 source, [`Inventory::cancel_held`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldFrom {
    /// A bag cell, a stash / transfer page cell or an equipment slot.
    At(ItemLoc),
    /// The caravan's reagent (material) storage.
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
    /// The inactive weapon set's main / off hand (`crate::weapon_sets`).
    pub other_weapons: [Option<Item>; 2],
    /// Weapon set II is the active one (`EquipmentCtrl` +25, the save's
    /// `use_alternate`).
    pub alternate: bool,
    /// The requested set (`ControllerCharacter` +616, block 1
    /// `alternate_config`); `weapon_sets::apply_swap` makes it current.
    pub requested_alternate: bool,
    /// Set II was chosen once in the character window (`Player` +19008,
    /// `alternate_config_enabled`): the HUD swap button works.
    pub alternate_enabled: bool,
    /// The main bag, then the extra bags the character has (one per save
    /// sack; a new character has only the main bag, the game adds bags by
    /// quest).
    pub bags: Vec<Sack<Item>>,
    /// The extra bag shown beside the main one (index into `bags`, ≥ 1).
    pub bag_tab: usize,
    pub held: Option<Item>,
    /// Where the held item was picked up from, with the item itself: used only while that item is still the
    /// held one ([`Inventory::cancel_held`]).
    pub held_from: Option<(HeldFrom, Item)>,
    /// Bag items of the save that found no cell on load (their cell taken or outside the grid, and no bag has
    /// room): kept, and written back at their saved sack and cell (`crate::save`), so a later load tries again.
    pub unplaced: Vec<Unplaced>,
    /// The held item's destroy question is up (`crate::item_drop`: a soulbound or untradeable item clicked on
    /// the world), its text tag; answered with `item_drop::DestroyAnswer`.
    pub destroying: Option<&'static str>,
    /// The tonic / writ / bounty a right click chose (`crate::consumables::ask_use`): its yes / no question is up
    /// once [`crate::consumables::UseAsk::question`] is set.
    pub use_ask: Option<crate::consumables::UseAsk>,
    /// The apply mode: a component / augment chosen by right click, used on
    /// the next item clicked (module docs).
    pub applying: Option<Applying>,
    /// UI sounds of inventory actions, played by [`play_sounds`].
    pub sounds: Vec<String>,
    /// Bumped on every change (UI rebuild signature).
    pub version: u64,
    /// The worn equipment changed: [`sync_equipment`] passes it to the
    /// player's loadout.
    equipment_dirty: bool,
    /// The worn item each slot holds while it is detached (its requirements
    /// failed at a sift, module docs); another item in the slot is attached.
    pub(crate) detached: [Option<Item>; 14],
    /// Consumables used this frame (right click; `consumables.rs` applies them, one of the stack each): the item
    /// and the bag cell it was taken from (None: a hotbar / hook / food use, no cell — [`Self::return_used`]).
    pub used: Vec<(Item, Option<(usize, u32, u32)>)>,
    /// Items picked up from the ground that the bag window has not shown yet, by (record, seed): they glow in
    /// the bag grid until the cursor passes over them (`Item::OnPickup` @0x180328690 sets the flag the grid's
    /// rebuild reads; the `newItemHighlight` option, `ui/newitems.rs`).
    pub fresh: Vec<(String, u32)>,
    db: ItemDb,
    /// Derived-item cache, interior-mutable so the UI reads it through `&self`
    /// ([`Self::info`]) without ticking the resource's change detection.
    info: std::sync::Mutex<HashMap<Item, Option<Arc<ItemInfo>>>>,
    vfs: Arc<gd_formats::vfs::Vfs>,
}

/// A saved item that fits nowhere in its grids ([`Inventory::unplaced`], `Stash::unplaced`): its sack index
/// and saved cell.
#[derive(Clone, Debug)]
pub struct Unplaced {
    pub sack: usize,
    pub x: u32,
    pub y: u32,
    pub item: Item,
}

/// Puts a save's items into `sacks` (`saved`: sack index, cell, item): every item at its saved cell first,
/// then each one whose cell is taken or outside the grid at the first free cell of its own sack, else of the
/// others in order; the ones no sack holds are returned. PORT: the engine's load of a colliding item is not
/// traced; nothing is dropped here.
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

/// The cursor passed over a glowing new item (`ui/newitems.rs`): its glow ends.
#[derive(Message, Clone, Debug)]
pub struct NewItemSeen(pub (String, u32));

/// Ends the glow of the items the cursor passed over.
pub fn clear_fresh(mut seen: MessageReader<NewItemSeen>, mut inv: ResMut<Inventory>) {
    for NewItemSeen(key) in seen.read() {
        let before = inv.fresh.len();
        inv.fresh.retain(|k| k != key);
        if inv.fresh.len() != before {
            inv.changed();
        }
    }
}

impl Inventory {
    /// Marks an item picked up from the ground (its bag glow, [`Inventory::fresh`]).
    pub fn mark_fresh(&mut self, item: &Item) {
        let key = (item.base_name.clone(), item.seed);
        if !self.fresh.contains(&key) {
            self.fresh.push(key);
        }
    }

    /// The item's resolved data and footprint, cached (a cache miss resolves and
    /// stores it). `&self`: the cache is interior-mutable, so a UI read never marks
    /// the inventory changed. Cleared at [`INFO_CACHE_MAX`] entries, so stack sizes
    /// and passing ground drops cannot grow it without bound. The lock is held only
    /// for the lookup and the insert: systems resolving different items in parallel
    /// (a drop being named and filtered) never wait on each other's resolve.
    pub fn info(&self, item: &Item) -> Option<Arc<ItemInfo>> {
        if let Some(i) = self.info.lock().unwrap().get(item) {
            return i.clone();
        }
        let r = resolve(&self.db, item).map(|resolved| {
            let size =
                bitmap_footprint(&self.vfs, &resolved.bitmap).unwrap_or(Footprint { w: 1, h: 1 });
            Arc::new(ItemInfo { resolved, size })
        });
        let mut cache = self.info.lock().unwrap();
        if cache.len() >= INFO_CACHE_MAX {
            cache.clear();
        }
        cache.entry(item.clone()).or_insert(r).clone()
    }

    /// The item's footprint (1×1 for an unknown record), without touching the cache when it is not there.
    pub fn size_of(&self, item: &Item) -> Footprint {
        const ONE: Footprint = Footprint { w: 1, h: 1 };
        if let Some(i) = self.info.lock().unwrap().get(item) {
            return i.as_ref().map_or(ONE, |i| i.size);
        }
        resolve(&self.db, item)
            .and_then(|r| bitmap_footprint(&self.vfs, &r.bitmap))
            .unwrap_or(ONE)
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

    /// `CursorHandlerItemMove::Cancel` @0x180184820: the held
    /// item goes back where it came from ([`Inventory::held_from`]): a bag cell, or a stash / transfer page cell,
    /// when that cell has no conflict (`InventorySack::GetConflicts`, then `PlayerInventoryCtrl::AddItem` /
    /// `Player::AddItemToPrivateStash` / `GameEngine::AddItemToTransfer` at its old position; PORT: the stash
    /// sack is the one it came from, the engine's the selected one, which is that page while it is shown); an
    /// item not `CanBePlacedInTransferStash` from the transfer stash goes to the bags (`AddItem`), staying on the
    /// cursor when they are full; a material taken from the reagent storage goes back into it
    /// (`AddItemToReagents`). Otherwise (an equipment slot, a window, a refused add) it is returned as `Err` for
    /// `ControllerPlayer::GiveItemToPlayer(id, false)` ([`crate::loot::give_item`]: equip-empty, bags, else
    /// thrown down). `stash`: None before the stash is loaded.
    // `Err` hands the item back to the caller (as `Inventory::give`).
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

    /// `PlayerInventoryCtrl::AddItem` without the shard join (module doc, step 2); false when all bags are full.
    pub fn add_to_bags(&mut self, item: Item) -> bool {
        // A sack never stays an item: `PlayerInventoryCtrl::AddItem`
        // @0x1803ef340 runs `OneShotFilter` @0x1803f28c0 on a `OneShot_*`
        // item first; for `OneShot_Sack` it calls `AddSack` @0x1803eed70
        // (refused with 6 sacks, the main one included) and then uses the
        // item (`ControllerPlayer::UseItem`) or, refused, removes it
        // (`SendRemoveItemFromInventory`): handled either way.
        // `OneShot_Food` is used at once the same way (`consumables.rs`).
        let (food, sack) = self.db.get(&item.base_name).map_or((false, false), |r| {
            (r.class == "OneShot_Food", r.class == "OneShot_Sack")
        });
        if food {
            self.used.push((item, None));
            return true;
        }
        if sack {
            if self.bags.len() < MAX_SACKS {
                self.bags
                    .push(gd_sim::items::inventory::Sack::new(8, 8, true));
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
        // Onto the stacks of its record with room first (`crate::stack::merge_into`); the bags change only when
        // all of it finds a place (a refused item stays whole with the caller): a dry run first, no bag copies.
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
        // New cell for what the stacks leave: the main bag, the selected one, then the rest (module doc).
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

    /// Puts a refused consumable back where it was taken from ([`Self::used`]): onto the remaining stack of its
    /// record at that cell, else into the empty cell; no source or the cell taken meanwhile: first fit
    /// ([`Self::add_to_bags`]), else onto an empty cursor. `Err`: nowhere to go (the caller throws it down,
    /// [`crate::loot::give_item`]); a held item is never overwritten.
    // `Err` hands the item back to the caller.
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

    /// `ControllerPlayer::GiveItemToPlayer` (module doc): [`Self::auto_insert`], else the bags
    /// (`PlayerInventoryCtrl::AddItem`; `combine` = an incomplete component first joins a partial,
    /// [`shard_target`], which only `CharPickUpConfigCmd` asks for, @0x1803ef340 callers).
    /// `Err` hands the item back (nothing took it: `crate::loot::give_item`).
    // `Err` hands the item back to the caller.
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

    /// `CharPickUpConfigCmd::Execute` @0x1800ea480 / `PickUpAction::Execute` @0x1800834a0: a Broken
    /// item (base `itemClassification`, `GetItemClassification(1)` == 5) and a `cannotPickUpMultiple`
    /// record the player already has ([`Self::has_record`]) are not picked up (nothing happens).
    pub fn refuses_pickup(&self, item: &Item) -> bool {
        let Some(rec) = self.db.get(&item.base_name) else {
            return false;
        };
        gd_sim::items::Classification::parse(rec.str("itemClassification").unwrap_or(""))
            == gd_sim::items::Classification::Broken
            || rec.bool("cannotPickUpMultiple") && self.has_record(&item.base_name)
    }

    /// `PlayerInventoryCtrl::AddItem`: an `ItemRelic` (components, charms) that is not complete
    /// found a partial to join ([`shard_target`]) -> `ControllerPlayer::UseItemOn(item, target)` =
    /// `ItemRelic::UseRelicOn`: the target gets the shards (`relicCompleteSound` when that completes it,
    /// else `relicToRelicSound`) and the item is gone. (Stock components all complete at 1 shard.)
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

    /// `Character::HasItem(name)` @0x1800639d0: `Inventory::IsItemInInventory` or
    /// `EquipManager::HasItem`, an item whose object name (its base record) equals `record` ignoring case and
    /// the slash direction. PORT: the bags and both weapon sets' slots (not the held item).
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

    /// Equipped items as worn (armour slots, then the weapons): the attached
    /// ones (module docs).
    pub fn worn(&self) -> Vec<Item> {
        (0..self.equipped.len())
            .filter(|&i| self.is_attached(i))
            .filter_map(|i| self.equipped[i].clone())
            .collect()
    }

    /// Whether equipment slot `i` holds an attached item.
    pub fn is_attached(&self, i: usize) -> bool {
        self.equipped[i].is_some() && self.detached[i] != self.equipped[i]
    }

    /// The worn gear for `compare_dps` with `candidate` in the main hand
    /// (`None`: as worn): a two-hander clears the off hand
    /// (`Player::CompareItems` equips it into slot 0).
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

    /// The worn gear with `candidate` in the off hand, when it may go there and the worn off hand holds a one-handed
    /// weapon (`Player::CompareItems`: the off-hand compare runs only against a worn off-hand WEAPON, not a shield or
    /// focus; a two-handed candidate never prints the line). `None` otherwise.
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

    /// Right click on bag item (`bag`, cell `x`, `y`): a component, augment
    /// or single-piece transmuter (`crate::illusion` docs) enters the apply
    /// mode (module docs) with the activation sound
    /// (`relicCursorActivationSound` of `records/ingameui/gameengine.dbr`,
    /// `CursorHandlerRelicCombine` / `CursorHandlerEnchant` /
    /// `CursorHandlerTransmute` constructors); false for other items.
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

    /// Ends the apply mode. Callers of the handlers' `Cancel`
    /// (vtable +152): `CursorHandlerEnchant::Escape` (Esc, refused while the
    /// confirmation is up), `Update` (the confirmation answered) and, after a
    /// use, `Primary…Activate`; in Grim Dawn.exe the inventory windows' close
    /// (sub_1401E1710, the character window's sub_140266AE0) and the caravan
    /// (stash) window's hide (sub_140138020), each when the handler's +48
    /// (true for both handlers) allows it. A press outside every window
    /// (the handler manager's sub_1401ED7C0, either button) calls
    /// `ActivateWorld`, which ends the mode the same way and takes the press
    /// (`grimdawn` `ui/apply.rs` `world_click` sends [`ApplyRequest::Cancel`]).
    pub fn cancel_apply(&mut self) {
        if self.applying.take().is_some() {
            self.changed();
        }
    }

    /// Every item the apply mode's component can be used on (bags, stash and
    /// transfer pages, equipped slots), by the same rule as the click
    /// ([`Self::apply_check`], [`Self::apply_blocked`]). `transfer_open`:
    /// `stash::SharedFiles::accepts` of the transfer stash.
    pub fn apply_targets(
        &self,
        stash: Option<&Stash>,
        req: &Requirements,
        transfer_open: bool,
    ) -> Vec<ItemLoc> {
        let Some(a) = self.applying.clone() else {
            return Vec::new();
        };
        let Some(comp) = self.applying_item(stash).cloned() else {
            return Vec::new();
        };
        let mut locs: Vec<(ItemLoc, Item)> = Vec::new();
        for (b, bag) in self.bags.iter().enumerate() {
            locs.extend(bag.items.iter().map(|q| {
                (
                    ItemLoc::Bag {
                        bag: b,
                        x: q.x,
                        y: q.y,
                    },
                    q.item.clone(),
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
                        q.item.clone(),
                    )
                }));
            }
        }
        locs.extend(
            self.equipped
                .iter()
                .enumerate()
                .filter_map(|(i, e)| e.clone().map(|it| (ItemLoc::Equip(i), it))),
        );
        locs.into_iter()
            .filter(|(l, it)| {
                *l != a.src
                    && !self.refused_at(&comp, *l)
                    && !self.apply_blocked(Some(*l), transfer_open)
                    && self.apply_check(&comp, it, req).is_some()
            })
            .map(|(l, _)| l)
            .collect()
    }

    /// A component from outside a blocked transfer stash (`transfer_open` false) onto an item in it: refused,
    /// the store takes no deposits that session (module doc). A component already in it may still be used there.
    pub fn apply_blocked(&self, target: Option<ItemLoc>, transfer_open: bool) -> bool {
        let in_transfer =
            |l: Option<ItemLoc>| matches!(l, Some(ItemLoc::Stash { transfer: true, .. }));
        !transfer_open && in_transfer(target) && !in_transfer(self.applying.as_ref().map(|a| a.src))
    }

    /// Whether `comp` (a component or augment) can be used on `target`; `Some`
    /// holds the confirmation text tag the game asks first, if any.
    /// `ItemRelic::CanRelicBeUsedOn` @0x180348700: a partial
    /// component onto another partial of the same record (`TestCompatibility`
    /// @0x18034a000: same record, target incomplete) without confirmation;
    /// onto equipment only when the component's requirements are met (the
    /// cursor handler's flag, `AreRequirementsMet` at its creation): an item
    /// that already has a component accepts only a partial of the same record
    /// (`TestCompatibility` with the socketed one), so a socketed component is
    /// never replaced; else `ItemUseFilter` with `tagComponentConfirmation`
    /// (`CursorHandlerRelicCombine::PrimaryInventoryActivate` @0x1801880f0 /
    /// `PrimaryEquipActivate` @0x180188430). Augments:
    /// `ItemEnchantment::CanBeUsedOn` @0x180339330 = equipment +
    /// `ItemUseFilter`, no requirement test, always confirmed; the text
    /// (`CursorHandlerEnchant::PrimaryInventoryActivate` @0x180182010):
    /// `tagEnchantmentConfirmation`, a soulbound augment on an unbound item
    /// `…SoulboundConfirmation`; onto an augmented item `tagEnchantmentOverwrite`,
    /// `…SoulboundOverwrite` (soulbound augment, unbound item) or
    /// `…SoulboundRemoval` (a plain augment replacing a soulbound one).
    /// PORT: an item's soulbound state (`Item` +3072) is read as its base
    /// record's or its augment's `soulbound`; the component's attribute
    /// requirements (no stock component has any) are not tested.
    /// Transmuters: [`crate::illusion::transmute_check`].
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

    /// A soulbound augment is never used on a transfer-stash item
    /// (`CursorHandlerEnchant::PrimaryTransferActivate` @0x180182d00: returns
    /// before the item under the cursor when the augment's `Item` +3072 is
    /// set, and `GetTransferHighlights` @0x180183060 marks it red; PORT: that
    /// flag read as the record's `soulbound`, as in [`Self::apply_check`]);
    /// `CursorHandlerRelicCombine::PrimaryTransferActivate` has no such test.
    /// A transmuter only when it `CanBePlacedInTransferStash` (vt+1032: not
    /// soulbound, no `craftingMaterial`; `CursorHandlerTransmute::
    /// PrimaryTransferActivate` @0x180189d60), so never a stock one.
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

    /// A click on `target` (`None`: an empty cell) in the apply mode.
    /// `CursorHandlerRelicCombine::PrimaryInventoryActivate` /
    /// `CursorHandlerEnchant::PrimaryInventoryActivate`, the same in
    /// `PrimaryStashActivate` @0x1801827a0 / @0x180188680 (the item under the
    /// cursor in the shown stash tab, `Player` +2308[+4614]) and
    /// `PrimaryTransferActivate` (the shown transfer tab, `GameEngine`
    /// +28079[+56157]; [`Self::refused_at`]): no target, the
    /// component itself or a gone component ends the mode; a target it cannot
    /// be used on does nothing; else the confirmation is asked
    /// ([`Applying::confirm`], answered by [`Self::answer_apply`]) or the use
    /// happens at once.
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
    /// (`CursorHandlerEnchant::Update` @0x180181bf0, response 1 / 0).
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

    /// Uses the apply mode's component / augment on the item at `t` and ends
    /// the mode, queueing its sound ([`Inventory::sounds`]).
    /// `ItemRelic::UseRelicOn` @0x180348410: onto a partial,
    /// `AddToRelicLevel` @0x180348030 adds its shards (`relicCompleteSound`
    /// when that completes it, else `relicToRelicSound`), the component keeps
    /// the overflow or is destroyed; onto equipment `AddRelic` and
    /// `relicToItemSound`; of a stack one is used, the rest stays in its cell.
    /// `ItemEnchantment::UseOn` @0x180339180: an augment already there is
    /// destroyed, one of a stack is used, `useSound`. The completion bonus of
    /// an `ItemRelic` component is empty (`components::roll_bonus`).
    /// `ItemTransmuter::UseOn`: [`crate::illusion::transmute_result`], no sound.
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
            // (No stock component has more than one shard: `completedRelicLevel` 1.)
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
        // Write the target back, then the component's cell.
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
    fn requirement_failure(&self, item: &Item, req: &Requirements) -> Option<String> {
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

    /// Slots `item` may go into: its class's slots, with one-handed weapons
    /// in the off hand only when a skill grants dual wielding of that kind
    /// (the hand rule against empty hands, module docs).
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
                    || gd_sim::items::inventory::hand_fits(left, 0, 0, dual, false)
            })
            .collect()
    }

    /// `item`'s hand type for `slot` (main = right, off = left, else 0; `gd_sim::items::inventory::hand_types`).
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

    /// The hand type hand `slot` of the active set holds (0: empty); a main-hand two-hander holds both (the
    /// engine stores it in both hands).
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

    /// The hand rule (module docs, `gd_sim::items::inventory::hand_fits`) for `item` in hand `slot` of the
    /// active set: `strict` for auto insert, else manual equip.
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
        gd_sim::items::inventory::hand_fits(t, here, o, req.dual, strict)
    }

    /// `EquipmentCtrl::SmartAutoInsert` @0x180289df0 (module doc): the slot a right click equips
    /// `item` into; None when its requirements are unmet, it is not equipment or neither hand takes it.
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

    /// `EquipmentCtrl::SmartAutoInsert` @0x180289df0 and its caller `BagWidget_HandleMouseEvent`
    /// @0x1401EA910 (module doc): equips `item` (already out of the bags, nothing held) into
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
            // Worn items never stack, so an add only pushes a new entry: undo by truncating each bag
            // back to its length (no bag copies).
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
    // `Err` hands the item back to the caller.
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
                .clone()
                .is_some_and(|m| self.is_two_handed(&m))
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

    /// `EquipmentCtrl::AutoInsertItem` @0x18028a5c0 (module doc): equips `item` into the first
    /// empty slot of [`AUTO_INSERT_ORDER`] it may go into, never swapping; `Err` hands it back.
    // `Err` hands the item back to the caller.
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

/// `EquipmentCtrl::AutoInsertItem`'s slot order (module doc; the engine's Clothing location is not tried).
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

pub fn init_inventory(
    hooks: Res<crate::hooks::Hooks>,
    epoch: Res<crate::session::SessionEpoch>,
    mut commands: Commands,
    // Option: `inv` is the existence check (this system creates it); `loaded` is absent for a new character.
    inv: Option<Res<Inventory>>,
    loaded: Option<Res<LoadedCharacter>>,
    data: Res<GameData>,
    // The player's [`Loadout`]: its `female` is the sex `Loadout::from_save` built the character with.
    players: Query<&Loadout, With<Player>>,
) {
    if inv.is_some() {
        return;
    }
    let Ok(female) = players.single().map(|l| l.female) else {
        return;
    };
    let mut inv = Inventory {
        equipped: Default::default(),
        other_weapons: Default::default(),
        alternate: false,
        requested_alternate: false,
        alternate_enabled: false,
        // Main bag 12×8, extra bags 8×8 (`UICharWindowInventorySack*Dims` / 32).
        bags: vec![Sack::new(12, 8, true)],
        bag_tab: 1,
        held: None,
        held_from: None,
        destroying: None,
        unplaced: Vec::new(),
        use_ask: None,
        applying: None,
        sounds: Vec::new(),
        version: epoch.version_base() + 1,
        equipment_dirty: false,
        detached: Default::default(),
        used: Vec::new(),
        fresh: Vec::new(),
        db: data.item_db(),
        info: Default::default(),
        vfs: data.vfs.clone(),
    };
    match loaded.as_ref().and_then(|c| c.0.inventory.data.clone()) {
        Some(d) => {
            for (i, s) in d.equipment.iter().enumerate().take(12) {
                if !s.item.is_empty() {
                    inv.equipped[i] = Some(s.item.clone());
                }
            }
            // Both weapon sets: the active one is worn (`crate::weapon_sets`).
            let alt = d.use_alternate != 0;
            for (set, w) in d.weapons.iter().enumerate() {
                for (k, s) in w
                    .slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| !s.item.is_empty())
                {
                    *(if (set == 1) == alt {
                        &mut inv.equipped[12 + k]
                    } else {
                        &mut inv.other_weapons[k]
                    }) = Some(s.item.clone());
                }
            }
            let info = loaded.as_ref().map(|c| &c.0.info);
            inv.alternate = alt;
            inv.requested_alternate = info.map_or(alt, |i| i.alternate_config != 0);
            inv.alternate_enabled = info.is_some_and(|i| i.alternate_config_enabled != 0);
            while inv.bags.len() < d.sacks.len() {
                inv.bags.push(Sack::new(8, 8, true));
            }
            let saved = d.sacks.iter().enumerate().flat_map(|(sack, s)| {
                s.items.iter().map(move |si| Unplaced {
                    sack,
                    x: si.x,
                    y: si.y,
                    item: si.item.clone(),
                })
            });
            let mut bags = std::mem::take(&mut inv.bags);
            inv.unplaced = place_saved(&mut inv, "bags", &mut bags, saved);
            inv.bags = bags;
        }
        None => {
            for r in crate::player::starting_gear(&data.db, female) {
                let item = Item {
                    base_name: r,
                    stack_count: 1,
                    ..Default::default()
                };
                let Some(class) = inv.info(&item).map(|i| i.resolved.class.clone()) else {
                    continue;
                };
                if let Some(slot) = slots_for_class(&class)
                    .iter()
                    .find(|s| inv.equipped[slot_index(**s)].is_none())
                {
                    inv.equipped[slot_index(*slot)] = Some(item);
                }
            }
        }
    }
    // `GD_EQUIP` (hooks.rs).
    if let Some(list) = hooks.items.equip.clone() {
        for r in list.split(';').filter(|r| !r.is_empty()) {
            let (r, left) = r.strip_suffix("@left").map_or((r, false), |r| (r, true));
            let item = Item {
                base_name: r.to_owned(),
                stack_count: 1,
                seed: 1,
                ..Default::default()
            };
            let Some(class) = inv.info(&item).map(|i| i.resolved.class.clone()) else {
                continue;
            };
            let slot = if left {
                Some(EquipSlot::OffHand)
            } else {
                slots_for_class(&class).first().copied()
            };
            if let Some(slot) = slot {
                inv.equipped[slot_index(slot)] = Some(item);
                inv.equipment_dirty = true;
            }
        }
    }
    // `GD_GIVE` (hooks.rs).
    for g in hooks
        .items
        .give
        .clone()
        .unwrap_or_default()
        .split(';')
        .filter(|s| !s.is_empty())
    {
        let (rec, n) = g
            .split_once('*')
            .map_or((g, 1), |(r, n)| (r, n.parse().unwrap_or(1)));
        for _ in 0..n {
            inv.add_to_bags(gd_formats::save::item::Item {
                base_name: rec.to_owned(),
                seed: crate::rng::u32(1..0x7FFF_FFFF),
                stack_count: 1,
                ..Default::default()
            });
        }
    }
    commands.insert_resource(inv);
}

/// `EquipmentCtrl::AttributesHaveChanged` every 250 ms (module docs): each
/// worn item attaches or detaches by the player's [`Requirements`]; an
/// off-hand one-handed weapon also by its dual-wield bit
/// (`WeaponSet__SiftLeftHand`, module docs: the hand rule).
pub fn sift_equipment(
    mut inv: ResMut<Inventory>,
    players: Query<&Requirements, With<Player>>,
    time: Res<Time>,
    mut wait: Local<f32>,
) {
    let Ok(req) = players.single() else {
        return;
    };
    *wait -= time.delta_secs();
    if *wait > 0.0 {
        return;
    }
    *wait += SIFT_SECS;
    if *wait <= 0.0 {
        *wait = SIFT_SECS;
    }
    // Read through `&Inventory`; only a flipped slot takes `&mut` (change detection).
    let mut flips = [false; 14];
    let ro: &Inventory = &inv;
    for (i, slot) in ro.equipped.iter().enumerate() {
        let Some(item) = slot else {
            continue;
        };
        let (detach, undual) = detach_rule(ro, i, item, req);
        if detach != (ro.detached[i].as_ref() == Some(item)) {
            info!(
                "{} {}",
                match (detach, undual) {
                    (false, _) => "attached:",
                    (true, true) => "detached (no dual wielding):",
                    (true, false) => "detached (requirements not met):",
                },
                item.base_name
            );
            flips[i] = true;
        }
    }
    if flips.contains(&true) {
        let inv = &mut *inv;
        for (i, _) in flips.iter().enumerate().filter(|f| *f.1) {
            inv.detached[i] = if inv.detached[i].is_some() && inv.detached[i] == inv.equipped[i] {
                None
            } else {
                inv.equipped[i].clone()
            };
        }
        inv.equipment_dirty = true;
        inv.changed();
    }
}

/// Whether the item worn in equipment slot `i` is detached ([`sift_equipment`],
/// and `weapon_sets::apply_swap` for the swapped-in weapons): an off-hand
/// weapon the skills don't allow to dual wield, or requirements not met. Also
/// whether it was the dual-wield rule.
pub fn detach_rule(inv: &Inventory, i: usize, item: &Item, req: &Requirements) -> (bool, bool) {
    let left = if i == slot_index(EquipSlot::OffHand) {
        inv.hand_type(item, EquipSlot::OffHand)
    } else {
        0
    };
    let undual = left & 0x41 != 0 && left & req.dual == 0;
    (undual || !inv.usable(item, req), undual)
}

/// `ControllerCharacter` +608: ms between sifts.
const SIFT_SECS: f32 = 0.25;

/// What the player's equip checks read: level, physique / cunning / spirit,
/// the worn gear's requirement reductions (`Profile::req_reduction`) and the
/// skills' dual-wield mask ([`Loadout::dual_mask`]). Derived each frame by
/// [`sync_requirements`] (required by [`Player`]; it runs before any reader).
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct Requirements {
    pub level: i32,
    pub attributes: [f32; 3],
    pub reduction: gd_sim::items::req_reduction::Reduction,
    pub dual: u32,
}

/// Derives the player's [`Requirements`]; a change bumps the inventory's
/// version so the unusable marks are redrawn.
pub fn sync_requirements(
    mut inv: ResMut<Inventory>,
    data: Res<GameData>,
    // Option: created by `init_progress`; level 1 until then (as a new character).
    progress: Option<Res<PlayerProgress>>,
    mut players: Query<(&Combatant, &Loadout, &mut Requirements), With<Player>>,
) {
    let Ok((cb, loadout, mut req)) = players.single_mut() else {
        return;
    };
    let s = &cb.profile.sheet;
    let new = Requirements {
        level: progress.map_or(1, |p| p.progress.level),
        attributes: [s.physique, s.cunning, s.spirit],
        reduction: cb.profile.req_reduction,
        dual: loadout.dual_mask(&data.db),
    };
    if *req != new {
        *req = new;
        inv.changed();
    }
}

/// Plays the queued inventory sounds ([`Inventory::sounds`]) as UI sounds.
pub fn play_sounds(mut inv: ResMut<Inventory>, mut out: MessageWriter<crate::visuals::PlaySound>) {
    if inv.sounds.is_empty() {
        return;
    }
    for pak in std::mem::take(&mut inv.sounds) {
        out.write(crate::visuals::PlaySound { pak, at: None });
    }
}

/// The inventory's per-frame steps inside [`InventorySet`]: `Init`, `Requirements`, `Equipment`.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InventoryStage {
    Init,
    Requirements,
    Equipment,
}
