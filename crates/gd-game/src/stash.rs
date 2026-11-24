//! The stash systems: initialization, reagent deposits and the stash request
//! handler (the stash data and pure moves are [`gd_game_core::stash`]).
use bevy::prelude::*;
use gd_formats::inventory::{Footprint, Sack};
use gd_formats::save::Shared;
use gd_formats::save::item::Item;
use gd_game_core::inventory::Inventory;
use gd_game_core::session::MessageDrain;
use gd_game_core::stash::{
    DepositReagents, LOADED_EXPANSIONS, QuickActionError, QuickMove, STASH_H, STASH_W, SharedFiles,
    Stash, StashRequest, deposit_reagents_now, error_sound, next_page_cost, quick_move,
    quick_move_worn, read_shared, shared_path, store_material, to_pages, ui_drop_sound,
};

#[allow(clippy::too_many_arguments)]
pub fn init_stash(
    mut commands: Commands,
    // Option: `stash` is the existence check (this system creates it); `loaded` is absent for a new character.
    stash: Option<Res<Stash>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    mut inv: ResMut<Inventory>,
    choice: Res<gd_game_core::session::Choice>,
    save_dir: Res<gd_game_core::session::SaveDir>,
    data: Res<gd_game_core::data::GameData>,
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
        seed: gd_game_core::rng::u32(..),
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
        seed: gd_game_core::rng::u32(..),
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
    // `GameEngine::StreamPlayerFormulas`: an invalid formula (missing record or
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
                    .run_if(resource_exists::<gd_game_core::data::GameData>)
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
                        .run_if(resource_exists::<gd_game_core::progress::PlayerProgress>)
                        .run_if(resource_exists::<gd_game_core::data::GameData>),
                )
                    .chain()
                    .after(gd_game_core::inventory::InventorySet)
                    .before(gd_game_core::player::PlayerSet),
            );
    }
}

fn deposit_reagents(
    mut msgs: MessageReader<DepositReagents>,
    data: Res<gd_game_core::data::GameData>,
    mut inv: ResMut<Inventory>,
    mut stash: ResMut<Stash>,
    (shared, choice, save_dir): (
        Res<SharedFiles>,
        Res<gd_game_core::session::Choice>,
        Res<gd_game_core::session::SaveDir>,
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

/// [`QuickActionError`] to its notification and sound.
fn quick_action_errors(
    mut msgs: MessageReader<QuickActionError>,
    data: Res<gd_game_core::data::GameData>,
    mut notify: MessageWriter<gd_game_core::story::Notify>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
) {
    for m in msgs.read() {
        if m.notify {
            notify.write(gd_game_core::story::Notify("tagTransferStashError".into()));
        }
        if let Some(pak) = error_sound(&data) {
            sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_stash_requests(
    mut msgs: MessageReader<StashRequest>,
    mut done: MessageWriter<crate::trade::TradeDone>,
    mut stash: ResMut<Stash>,
    mut inv: ResMut<Inventory>,
    mut progress: ResMut<gd_game_core::progress::PlayerProgress>,
    data: Res<gd_game_core::data::GameData>,
    (mut error, mut sound, mut refused): (
        MessageWriter<QuickActionError>,
        MessageWriter<gd_game_core::visuals::PlaySound>,
        MessageWriter<crate::trade::TradeRefused>,
    ),
    (shared, choice, save_dir): (
        Res<SharedFiles>,
        Res<gd_game_core::session::Choice>,
        Res<gd_game_core::session::SaveDir>,
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
                            sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
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
                // A held stack onto a stack of its record (`gd_game_core::stack::place_stacking`).
                let max = inv.held.as_ref().map_or(1, |h| {
                    gd_game_core::stack::max_of(&inv, h, gd_game_core::stack::engine_max(&data.db))
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
                        match gd_game_core::stack::place_stacking(page, h, size, *cursor, max) {
                            Ok(under) => {
                                // The placed item's origin no longer applies; a swapped-in
                                // item has no cell to go back to (a cancel gives it).
                                inv.held_from = None;
                                inv.held = under;
                                if let Some(pak) = drop {
                                    sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
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
                            gd_game_core::inventory::HeldFrom::At(
                                gd_game_core::inventory::ItemLoc::Stash {
                                    transfer: *transfer,
                                    page: tab,
                                    x: placed.x,
                                    y: placed.y,
                                },
                            ),
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
                    && gd_formats::inventory::sort_sack(page, |it| {
                        gd_game_core::inventory::sort_key(&inv, &tags, it)
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
                            sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
                        }
                        store_material(&mut stash.materials, record, h.stack_count.max(1));
                    }
                    Some(h) => inv.held = Some(h),
                    // Take from the store.
                    None if have > 0 => {
                        let take = if *shift {
                            gd_game_core::stack::engine_max(&data.db).min(have)
                        } else {
                            1
                        };
                        let item = Item {
                            base_name: record.clone(),
                            seed: gd_game_core::rng::u32(1..0x7FFF_FFFF),
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
                                Some((gd_game_core::inventory::HeldFrom::Reagents, item.clone()));
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
