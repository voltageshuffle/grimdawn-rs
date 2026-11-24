//! The inventory systems: the request handlers, equipment sync, initialization
//! and the requirement sift (the inventory data and pure moves are
//! [`gd_game_core::inventory`]).
use bevy::prelude::*;
use gd_formats::inventory::{EquipSlot, Sack, slots_for_class};
use gd_formats::inventory::{Footprint, sort_sack};
use gd_formats::save::item::Item;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;
use gd_game_core::inventory::{
    ApplyRequest, EquipmentChanged, HeldFrom, InvRequest, Inventory, InventorySet, InventoryStage,
    ItemLoc, NewItemSeen, Requirements, SmartInsert, SortBag, Unplaced, detach_rule, doll_slots,
    place_saved, slot_index, sort_key,
};
use gd_game_core::player::{Loadout, Player, PlayerSet, ProfileDirty};
use gd_game_core::progress::PlayerProgress;
use gd_game_core::rules::CombatRules;
use gd_game_core::session::LoadedCharacter;
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

fn apply_requests(
    mut msgs: MessageReader<ApplyRequest>,
    mut inv: ResMut<Inventory>,
    // The stash is absent until `init_stash` ran; the answer then applies to the bags alone.
    mut stash: Option<ResMut<gd_game_core::stash::Stash>>,
    reqs: Query<&Requirements, With<Player>>,
    (shared, choice, save_dir, mut error): (
        Res<gd_game_core::stash::SharedFiles>,
        Res<gd_game_core::session::Choice>,
        Res<gd_game_core::session::SaveDir>,
        MessageWriter<gd_game_core::stash::QuickActionError>,
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
                error.write(gd_game_core::stash::QuickActionError { notify: true });
            }
            ApplyRequest::Click(target) => inv.apply_click(target, stash.as_deref_mut(), req),
        }
    }
}

fn sort_bags(
    mut msgs: MessageReader<SortBag>,
    mut inv: ResMut<Inventory>,
    data: Res<gd_game_core::data::GameData>,
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

#[allow(clippy::too_many_arguments)]
fn apply_inv_requests(
    mut commands: Commands,
    mut msgs: MessageReader<InvRequest>,
    mut inv: ResMut<Inventory>,
    mut stash: Option<ResMut<gd_game_core::stash::Stash>>,
    players: Query<
        (
            &Transform,
            &gd_game_core::character::Character,
            &Requirements,
        ),
        With<Player>,
    >,
    nav: Option<Res<crate::nav::Nav>>,
    mut back: MessageWriter<crate::crafting::SlotReturn>,
    data: Res<gd_game_core::data::GameData>,
    mut cinematic: MessageWriter<gd_game_core::story::CinematicText>,
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
            // (the merchant's quick sale is the UI's `trade::SellHeld`): `Cancel`.
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
                            // `ItemArtifactFormula::Use`: type 2,
                            // `tagFormulaLearned` over `tagFormulaLearn01(name)`. PORT: the name is not rarity coloured.
                            cinematic.write(gd_game_core::story::CinematicText {
                                head: "tagFormulaLearned".into(),
                                sub: "tagFormulaLearn01".into(),
                                sub_args: vec![gd_game_core::floating::TextArg::Tag(name)],
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
                            let drop = gd_game_core::stash::ui_drop_sound(&data.db, &h);
                            // The footprint is centred on the cursor; it may
                            // overlap at most one item, which is swapped into the hand
                            // or, the same record with room, takes the stack (`crate::stack`).
                            let max = gd_game_core::stack::max_of(
                                &inv,
                                &h,
                                gd_game_core::stack::engine_max(&data.db),
                            );
                            let Some(sack) = inv.bags.get_mut(*bag) else {
                                inv.held = Some(h);
                                continue;
                            };
                            match gd_game_core::stack::place_stacking(sack, h, size, *cursor, max) {
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
                                let drop = gd_game_core::stash::ui_drop_sound(&data.db, &h);
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
                                let drop = gd_game_core::stash::ui_drop_sound(
                                    &data.db,
                                    item.as_ref().unwrap(),
                                );
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
    players: &Query<
        (
            &Transform,
            &gd_game_core::character::Character,
            &Requirements,
        ),
        With<Player>,
    >,
    data: &gd_game_core::data::GameData,
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
    mut stash: Option<&mut gd_game_core::stash::Stash>,
    bag: usize,
    cell: (u32, u32),
    req: &Requirements,
    data: &gd_game_core::data::GameData,
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
    // (`ItemArtifactFormula::AllowUse`, shared by every formula class: refused when
    // `IsItemInFormulas` or not `IsBluePrintValid`, `gd_sim::items::crafting::formula_valid`; refused ->
    // `PlayerInventoryCtrl::UseItem` plays the "cannot" sound, `genericErrorSound`).
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
            inv.sounds
                .extend(gd_game_core::stash::learn_formula_sound(data));
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
            inv.sounds.extend(gd_game_core::stash::error_sound(data));
        }
        return None;
    }
    // The tonics' and writs' handlers may ask first (`consumables::ask_use`).
    if crate::consumables::asks(class) {
        inv.use_ask = Some(gd_game_core::consumables::UseAsk {
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
        inv.sounds.extend(gd_game_core::stash::error_sound(data));
    }
    inv.changed();
    None
}

/// Passes changed equipment to the living player's loadout and rebuilds its
/// profile (life keeps its fraction, at the current level; `ProfileDirty`).
/// The skill bonuses that came off release pets ([`crate::pets::AugmentsRemoved`]).
pub fn sync_equipment(
    mut commands: Commands,
    mut inv: ResMut<Inventory>,
    data: Res<gd_game_core::data::GameData>,
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

pub fn init_inventory(
    hooks: Res<gd_game_core::hooks::Hooks>,
    epoch: Res<gd_game_core::session::SessionEpoch>,
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
    let mut inv = Inventory::new(data.item_db(), data.vfs.clone(), epoch.version_base() + 1);
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
            for r in gd_game_core::player::starting_gear(&data.db, female) {
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
                seed: gd_game_core::rng::u32(1..0x7FFF_FFFF),
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

const SIFT_SECS: f32 = 0.25;

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
pub fn play_sounds(
    mut inv: ResMut<Inventory>,
    mut out: MessageWriter<gd_game_core::visuals::PlaySound>,
) {
    if inv.sounds.is_empty() {
        return;
    }
    for pak in std::mem::take(&mut inv.sounds) {
        out.write(gd_game_core::visuals::PlaySound { pak, at: None });
    }
}
