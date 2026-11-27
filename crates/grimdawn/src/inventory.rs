//! The player's inventory and equipment for this session.
//!
//! Loaded read-only from the save (or the starting gear), changed by pickups and the character window; never
//! written back to disk. Grid rules are `gd_formats::inventory`.
//! - Left click picks up / places / swaps with the item on the cursor (`BagWidget_HandleMouseEvent` /
//!   `EquipSlotWidget::HandleMouseEvent` hand the item to its cursor handler, `Item::GetCursorHandler`).
//! - Right click on a bag item sells it to an open merchant (only with `quickBuy`, else nothing) or equips it
//!   (`EquipmentCtrl::SmartAutoInsert`, displaced items go back to the inventory); on a component or augment it
//!   selects it and the next left click uses it on an item.
//! - Right click on an equipped item: sold to an open merchant with `quickBuy`, else nothing; with an item held a
//!   right click is its handler's (bag: quick sale or put back, slot: quick sale, or as a left click with no
//!   merchant).
//! - Shift + left on an equipped item with the caravan open moves it to the shown stash page.
//! - A held item clicked on the paper doll goes to its slot. Equipping checks the level and attribute
//!   requirements and the two-handed rule; any change rebuilds the player's profile and body.
//! - Ctrl + left click on a stack opens the stack quantity window.
//! - Shift + left click on a bag item sends it to the open caravan, Inventor or Runekeeper slot; with none open
//!   Shift is ignored.
//! - A bag item never goes to the ground from a click (`BagWidget_HandleMouseEvent` has no drop branch).
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::inventory::{Footprint, Sack, overlaps};
use gd_formats::save::item::Item;
use gd_formats::ui::Rect as URect;
use gd_game_core::player::{Loadout, Player};
use gd_render::character::{self, CharAssets};
use gd_ui::UiPanels;
use gd_ui::apply::question_up;
use gd_ui::state::inventory::{InvLayout, UiCapture};
use gd_ui_kit::contains;

pub use gd_game_core::inventory::{InvRequest, Inventory, InventorySet, slot_index};

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiCapture>()
            .init_resource::<InvLayout>()
            .add_systems(
                Update,
                (clicks, gd_ui::skills::clicks, gd_ui::devotion::clicks)
                    .chain()
                    .in_set(InventorySet)
                    .after(gd_game_core::inventory::InventoryStage::Requirements)
                    .before(gd_game::points::PlayerRequests),
            )
            .add_systems(
                Update,
                rebuild_body
                    .in_set(InventorySet)
                    .after(gd_game_core::inventory::InventoryStage::Equipment),
            )
            .add_systems(
                Update,
                (held_icon, held_overlay)
                    .in_set(gd_ui_kit::UiPlaySet)
                    // After the click's `InvRequest` is applied: the carried icon moves on the click frame.
                    .after(InventorySet),
            )
            .add_systems(Update, sheet_wheel.in_set(gd_ui_kit::UiInputSet));
    }
}

#[allow(clippy::too_many_arguments)]
fn clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut panels: ResMut<UiPanels>,
    layout: Res<InvLayout>,
    inv: Option<Res<Inventory>>,
    mut capture: ResMut<UiCapture>,
    players: Query<(), With<Player>>,
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    data: Res<gd_game_core::data::GameData>,
    mut trade: ResMut<gd_ui::state::trade::Trade>,
    (loaded, mut requests, mut apply, mut sell, mut sell_held, mut sell_worn): (
        Option<Res<gd_game_core::session::LoadedCharacter>>,
        MessageWriter<InvRequest>,
        MessageWriter<gd_game_core::inventory::ApplyRequest>,
        MessageWriter<gd_game::trade::SellRequest>,
        MessageWriter<gd_game::trade::SellHeld>,
        MessageWriter<gd_game::trade::SellEquipped>,
    ),
    mut sort: MessageWriter<gd_game_core::inventory::SortBag>,
    (mut spend, mut swap, mut stack, keys, real, mut bar_hold): (
        MessageWriter<gd_game::points::SpendAttribute>,
        MessageWriter<gd_game::weapon_sets::SwapWeapons>,
        ResMut<gd_ui::stack::StackWindow>,
        Res<ButtonInput<KeyCode>>,
        Res<Time<Real>>,
        Local<Option<(u8, gd_ui::window::BarHold)>>,
    ),
    #[cfg(feature = "dev")] (hooks, time, mut script): (
        Res<gd_game_core::hooks::Hooks>,
        Res<Time>,
        Local<Option<Vec<gd_game_core::hooks::Click>>>,
    ),
    (mut deposit, options, mut confirm, mut sound, mut stash_req, mut quick_error, above): (
        MessageWriter<gd_game_core::stash::DepositReagents>,
        Res<gd_game_core::options::GameOptions>,
        ResMut<gd_ui::apply::InvConfirm>,
        MessageWriter<gd_game_core::visuals::PlaySound>,
        MessageWriter<gd_game_core::stash::StashRequest>,
        MessageWriter<gd_game_core::stash::QuickActionError>,
        gd_ui::skills::Above,
    ),
) {
    capture.0 = false;
    #[cfg(feature = "dev")]
    let scripted = {
        let script = script.get_or_insert_with(|| hooks.items.inv_clicks.clone());
        gd_game_core::hooks::take_due(script, time.elapsed_secs()).map(|c| (c.right, c.at))
    };
    let Some(inv) = inv else { return };
    if !panels.character {
        // Closing the window ends the apply mode; the held item goes back to a bag.
        *bar_hold = None;
        if inv.applying.is_some() || inv.held.is_some() {
            requests.write(InvRequest::Closed);
        }
        return;
    }
    let Ok(win) = window.single() else { return };
    #[cfg(feature = "dev")]
    let scripted = scripted.map(|(r, p)| {
        info!("scripted {} click at {p}", if r { "right" } else { "left" });
        (p, !r, r)
    });
    #[cfg(not(feature = "dev"))]
    let scripted = None;
    let (p, left, right) = match scripted {
        Some(click) => click,
        None => {
            let Some(cursor) = win.cursor_position() else {
                return;
            };
            (
                cursor * win.scale_factor(),
                buttons.just_pressed(MouseButton::Left),
                buttons.just_pressed(MouseButton::Right),
            )
        }
    };
    // The local map / quest journal draw above this window.
    if stack.covers(p) || above.covers(p) {
        return;
    }
    let Some(wr) = layout.window else { return };
    let on_close = layout.close_button.is_some_and(|r| contains(&r, p));
    if !contains(&wr, p) && !on_close {
        return;
    }
    capture.0 = true;
    // A held sheet-bar part keeps scrolling / dragging without a new press.
    if let Some((t, hold)) = bar_hold.as_mut() {
        if buttons.pressed(MouseButton::Left)
            && let Some(sl) = layout.sheet_list.as_ref().filter(|sl| sl.tab == *t)
            && let Some(bar) = sl.bar
        {
            let t = *t as usize;
            let cur = panels.sheet_scroll[t].clamp(0.0, sl.max);
            let v = gd_ui::window::bar_held(
                Some(bar),
                hold,
                p.y,
                real.delta_secs() * 1000.0,
                cur * sl.s,
                sl.max * sl.s,
            );
            panels.sheet_scroll[t] = v / sl.s;
            return;
        }
        *bar_hold = None;
    }
    if !left && !right {
        return;
    }
    // Apply mode's confirmation and every other Yes/No question take the clicks: a click under it must not swap
    // the held item.
    if question_up(&inv, &trade, &confirm) {
        return;
    }
    // The close button: the held item goes back to a bag next frame.
    if left && on_close {
        panels.character = false;
        return;
    }
    if left && layout.swap_button.is_some_and(|r| contains(&r, p)) {
        swap.write(gd_game::weapon_sets::SwapWeapons { from_window: true });
        return;
    }
    if left && let Some((n, _)) = layout.sheet_tabs.iter().find(|(_, r)| contains(r, p)) {
        if panels.tab != *n {
            panels.tab = *n;
        }
        return;
    }
    // Page II / III list: section headers fold; the shared bar scrolls.
    if left && let Some(sl) = layout.sheet_list.clone() {
        let t = sl.tab as usize;
        let cur = panels.sheet_scroll[t].clamp(0.0, sl.max);
        if let Some(name) = sl.header_at(cur, p) {
            if let Some(i) = panels.sheet_collapsed.iter().position(|c| c == name) {
                panels.sheet_collapsed.remove(i);
            } else {
                panels.sheet_collapsed.push(name.clone());
            }
            return;
        }
        if let Some(bar) = sl.bar
            && let Some((v, hold)) = bar.press(p, sl.s, cur * sl.s, sl.max * sl.s)
        {
            panels.sheet_scroll[t] = v / sl.s;
            *bar_hold = Some((sl.tab, hold));
            return;
        }
    }
    // Move reagents (`CharacterWindow::OnButtonEvent`): ignored without a player or while
    // `InventoryWindow.reagentGuard` is set (PORT: taken as the held item); with `transferConfirmBypass` off it asks
    // `tagMoveReagentsConfirm`, else `PlayerInventoryCtrl::DepositReagents` at once. `soundNameDown` plays on the
    // press.
    if left && layout.reagent_button.is_some_and(|r| contains(&r, p)) {
        if let Some(pak) = data
            .db
            .get(&gd_ui::character::field_record(
                &data,
                "reagentDepositButton",
            ))
            .and_then(|r| r.str("soundNameDown").map(str::to_owned))
        {
            sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
        }
        if inv.held.is_none() && !players.is_empty() {
            if options.flag("transferConfirmBypass") {
                *confirm = gd_ui::apply::InvConfirm::Reagents;
            } else {
                deposit.write(gd_game_core::stash::DepositReagents);
            }
        }
        return;
    }
    // Auto-sort (each sort starts one layout further), at once with `sortConfirmBypass`, else after
    // `tagCaravanSort`; not while an item is held (`CharacterWindow::OnButtonEvent`).
    if left && let Some((b, _)) = layout.sort_buttons.iter().find(|(_, r)| contains(r, p)) {
        if inv.held.is_none() && !players.is_empty() {
            if options.flag("sortConfirmBypass") {
                sort.write(gd_game_core::inventory::SortBag(*b));
            } else {
                *confirm = gd_ui::apply::InvConfirm::Sort(*b);
            }
        }
        return;
    }
    if left && let Some((b, _)) = layout.bag_tabs.iter().find(|(_, r)| contains(r, p)) {
        requests.write(InvRequest::BagTab(*b));
        return;
    }
    if players.single().is_err() {
        return;
    }
    if left
        && let Some((which, _)) = layout.attr_buttons.iter().find(|(_, r)| contains(r, p))
        && progress
            .as_ref()
            .is_some_and(|pr| pr.progress.attribute_points > 0)
    {
        use gd_game::points::{Attribute, SpendAttribute};
        spend.write(SpendAttribute(
            [Attribute::Physique, Attribute::Cunning, Attribute::Spirit][(*which as usize).min(2)],
        ));
        return;
    }

    for (b, r, cell) in &layout.grids {
        if !contains(r, p) {
            continue;
        }
        let (cx, cy) = (((p.x - r.x) / cell) as u32, ((p.y - r.y) / cell) as u32);
        // Every bag click hides the stack window first; Ctrl + left click on a stack of more than one opens it
        // there, on anything else does nothing.
        stack.hide();
        // The apply mode's handler takes both buttons first.
        if inv.applying.is_some() && inv.held.is_none() {
            apply.write(gd_game_core::inventory::ApplyRequest::Click(Some(
                gd_game_core::inventory::ItemLoc::Bag {
                    bag: *b,
                    x: cx,
                    y: cy,
                },
            )));
            return;
        }
        // Shift + left click on a bag item, nothing held: the first open window of the chain takes it
        // (`BagWidget_HandleMouseEvent`): the caravan, the Inventor's item slot (`UIInventor::QuickAddItem`: tabs
        // 0-3 only, refused while the dismantle tab's guard bytes are set, PORT: taken as its warning prompt), the
        // Runekeeper's slot (`UIRunekeeper::QuickAddItem`); a refused or occupied slot shows `tagTransferStashError`
        // with the error sound. PORT: the slot's own test is `Trade::accepts`; the chat link and player trade steps
        // have no port window. There is no merchant step.
        if left
            && inv.held.is_none()
            && keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
            && let Some(placed) = inv.bags[*b].item_at(cx, cy)
        {
            use crate::trade::TradeOpen;
            use gd_ui::state::trade::Panel;
            match trade.open {
                Some(TradeOpen::Stash { .. }) => {
                    stash_req.write(gd_game_core::stash::StashRequest::QuickMove {
                        transfer: trade.transfer,
                        page: trade.tab,
                        bag: *b,
                        cell: (cx, cy),
                    });
                    return;
                }
                Some(TradeOpen::Inventor { .. } | TradeOpen::Ascension { .. }) => {
                    let t = &mut *trade;
                    let tab_takes = matches!(t.open, Some(TradeOpen::Ascension { .. }))
                        || match t.panel {
                            Panel::Craft => false,
                            Panel::Dismantle => !t.confirm,
                            Panel::Recover | Panel::Convert | Panel::Reroll => true,
                        };
                    match t.accepts(&placed.item, &data) {
                        Ok(()) if tab_takes && t.slot.is_none() => {
                            requests.write(InvRequest::ToWindowSlot {
                                bag: *b,
                                cell: (cx, cy),
                            });
                            t.kind = None;
                            t.version += 1;
                        }
                        r => {
                            if let Err(why) = r {
                                info!("trade: {why}");
                            }
                            quick_error
                                .write(gd_game_core::stash::QuickActionError { notify: true });
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        if left
            && inv.held.is_none()
            && inv.applying.is_none()
            && keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
        {
            if let Some(q) = inv.bags[*b].item_at(cx, cy)
                && gd_game_core::stack::count(&q.item) > 1
            {
                stack.open(
                    gd_game_core::inventory::ItemLoc::Bag {
                        bag: *b,
                        x: q.x,
                        y: q.y,
                    },
                    Vec2::new(
                        r.x + (q.x + q.size.w) as f32 * cell,
                        r.y + q.y as f32 * cell,
                    ),
                    gd_game_core::stack::count(&q.item),
                    &data,
                );
            }
            return;
        }
        // A merchant is open: right click sells with `quickBuy`, else does nothing (`BagWidget_HandleMouseEvent`:
        // `GetBool(0)` gates `PlayerSaleRequest`). With an item held (`SecondaryInventoryActivate`): sold with
        // `quickBuy`, else put back.
        if right && matches!(trade.open, Some(crate::trade::TradeOpen::Vendor { .. })) {
            let quick = options.flag("quickBuy");
            if inv.held.is_some() {
                if quick && let Some(shop) = trade.shop(loaded.as_deref()) {
                    sell_held.write(gd_game::trade::SellHeld { shop });
                    return;
                }
            } else {
                if quick
                    && let Some(shop) = trade.shop(loaded.as_deref())
                    && inv.bags[*b].item_at(cx, cy).is_some()
                {
                    sell.write(gd_game::trade::SellRequest {
                        shop,
                        bag: *b,
                        cell: (cx, cy),
                    });
                }
                return;
            }
        }
        // The caravan's selected personal stash page (`Player::GetSelectedStashSackNumber`): a Cancel
        // (`CursorHandlerItemMove::Cancel`) puts the held item there. The transfer view selects no
        // personal page.
        let stash_page = matches!(trade.open, Some(crate::trade::TradeOpen::Stash { .. }))
            .then_some(trade.tab)
            .filter(|_| !trade.transfer);
        if right {
            requests.write(InvRequest::Bag {
                bag: *b,
                cell: (cx, cy),
                cursor: Vec2::new((p.x - r.x) / cell, (p.y - r.y) / cell),
                right: true,
                stash_page,
            });
            return;
        }
        requests.write(InvRequest::Bag {
            bag: *b,
            cell: (cx, cy),
            cursor: Vec2::new((p.x - r.x) / cell, (p.y - r.y) / cell),
            right: false,
            stash_page,
        });
        return;
    }
    // A held item clicked on the paper doll goes to its slot, as a click on that slot.
    if left && layout.doll.is_some_and(|r| contains(&r, p)) && inv.held.is_some() {
        requests.write(InvRequest::Doll);
        return;
    }
    for (slot, r) in &layout.slots {
        if !contains(r, p) {
            continue;
        }
        stack.hide();
        let i = slot_index(*slot);
        if inv.applying.is_some() && inv.held.is_none() {
            let t = inv.equipped[i]
                .is_some()
                .then_some(gd_game_core::inventory::ItemLoc::Equip(i));
            apply.write(gd_game_core::inventory::ApplyRequest::Click(t));
            return;
        }
        // Shift + left on a worn item with the caravan open (`EquipSlotWidget::HandleMouseEvent`): to the shown
        // stash page.
        if left
            && inv.held.is_none()
            && inv.equipped[i].is_some()
            && keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
            && matches!(trade.open, Some(crate::trade::TradeOpen::Stash { .. }))
        {
            stash_req.write(gd_game_core::stash::StashRequest::QuickMoveWorn {
                transfer: trade.transfer,
                page: trade.tab,
                slot: i,
            });
            return;
        }
        // A right press with a merchant open (`SecondaryEquipActivate`): `quickBuy` sells the held item or, with
        // none, the worn one; without it nothing.
        if right && matches!(trade.open, Some(crate::trade::TradeOpen::Vendor { .. })) {
            if options.flag("quickBuy")
                && let Some(shop) = trade.shop(loaded.as_deref())
            {
                if inv.held.is_some() {
                    sell_held.write(gd_game::trade::SellHeld { shop });
                } else if inv.equipped[i].is_some() {
                    sell_worn.write(gd_game::trade::SellEquipped { shop, slot: i });
                }
            }
            return;
        }
        requests.write(InvRequest::Slot { slot: *slot, right });
        return;
    }
}

/// Rebuilds the player's body for its new equipment, each item drawn with its illusion; the entity and its
/// gameplay state stay. The old body stays until every new part has loaded (`pending`); another change meanwhile
/// only changes what it waits for.
fn rebuild_body(
    mut commands: Commands,
    mut changed: MessageReader<gd_game_core::inventory::EquipmentChanged>,
    players: Query<(&Transform, &Loadout, &character::Body)>,
    skels: Query<&gd_render::model::Skeleton>,
    mut assets: CharAssets,
    mut pending: Local<gd_formats::collections::HashMap<Entity, character::Waiting>>,
    mut session: gd_game_core::session::NewSession,
) {
    if session.begun() {
        pending.clear();
    }
    for ev in changed.read() {
        pending.entry(ev.player).or_default();
    }
    pending.retain(|player, held| {
        let Ok((tf, loadout, ch)) = players.get(*player) else {
            return false;
        };
        let names: Vec<String> = loadout
            .items
            .iter()
            .map(|i| gd_sim::items::illusion::look_record(i).to_owned())
            .collect();
        if !character::player_body_ready(&mut assets, loadout.female, &names, held) {
            return true;
        }
        character::despawn_body(&mut commands, *player, ch, &skels);
        character::rebuild_player_body(
            &mut commands,
            &mut assets,
            *player,
            *tf,
            loadout.female,
            &names,
        );
        info!("equipment changed: {} items worn", names.len());
        false
    });
}

/// Mouse wheel over a character-sheet list scrolls it 20 × s px a notch, over the view or the bar. Not under the
/// game menu / Options or a modal question.
#[allow(clippy::too_many_arguments)]
fn sheet_wheel(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<InvLayout>,
    mut panels: ResMut<UiPanels>,
    pause: Res<gd_game_core::pause::GamePause>,
    (inv, trade, confirm): (
        Option<Res<Inventory>>,
        Res<gd_ui::state::trade::Trade>,
        Res<gd_ui::apply::InvConfirm>,
    ),
    above: gd_ui::skills::Above,
) {
    if pause.menu || inv.is_some_and(|inv| question_up(&inv, &trade, &confirm)) {
        wheel.clear();
        return;
    }
    let Some(sl) = layout.sheet_list.as_ref() else {
        wheel.clear();
        return;
    };
    let Ok(win) = window.single() else {
        wheel.clear();
        return;
    };
    let Some(p) = gd_ui_kit::cursor_px(win) else {
        wheel.clear();
        return;
    };
    let over_bar = sl.bar.is_some_and(|b| {
        p.x >= b.up.x && p.x < b.up.x + b.up.w && p.y >= b.up.y && p.y < b.down.y + b.down.h
    });
    if !panels.character || !(contains(&sl.view, p) || over_bar) || above.covers(p) {
        wheel.clear();
        return;
    }
    let dy: f32 = wheel
        .read()
        .map(|e| match e.unit {
            bevy::input::mouse::MouseScrollUnit::Line => e.y * 20.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => e.y / sl.s,
        })
        .sum();
    if dy == 0.0 {
        return;
    }
    let t = sl.tab as usize;
    let v = (panels.sheet_scroll[t] - dy).clamp(0.0, sl.max);
    if v != panels.sheet_scroll[t] {
        panels.sheet_scroll[t] = v;
    }
}

/// Top-left cell of the footprint of a held item of `size` over a grid at `r` (cell size `cell`, `grid` cells)
/// with the cursor at `p`.
pub fn drop_cell(p: Vec2, r: &URect, cell: f32, grid: (u32, u32), size: Footprint) -> (u32, u32) {
    gd_formats::inventory::drop_cell([(p.x - r.x) / cell, (p.y - r.y) / cell], grid, size)
}

#[derive(Component)]
struct HeldOverlay;

/// Placement preview of the held item (`CursorHandlerItemMove::GetInventoryHighlights`, `BagWidget::Render`): the
/// footprint in green (0, 1, 0, 0.5) while it overlaps fewer than two items, the one overlapped item in blue
/// (0, 0, 1, 0.5); with two or more, those items in red (1, 0, 0, 0.5) and no footprint.
#[allow(clippy::too_many_arguments)]
fn held_overlay(
    mut commands: Commands,
    mut nodes: Query<(Entity, &mut Node, &mut BackgroundColor), With<HeldOverlay>>,
    inv: Option<Res<Inventory>>,
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<InvLayout>,
    trade_layout: Res<gd_ui::state::trade::TradeLayout>,
    trade: Res<gd_ui::state::trade::Trade>,
    stash: Option<Res<crate::trade::Stash>>,
    panels: Res<UiPanels>,
    (pause, confirm): (
        Res<gd_game_core::pause::GamePause>,
        Res<gd_ui::apply::InvConfirm>,
    ),
) {
    // None under the game menu or a modal question (the preview's z is above the question box).
    let mut wanted: Vec<(URect, Color)> = Vec::new();
    if let Some(inv) = inv
        && !pause.menu
        && !question_up(&inv, &trade, &confirm)
        && let Some(h) = inv.held.as_ref()
        && let Ok(win) = window.single()
        && let Some(p) = gd_ui_kit::cursor_px(win)
    {
        let size = inv
            .info(h)
            .map(|i| i.size)
            .unwrap_or(Footprint { w: 1, h: 1 });
        let mut grids: Vec<(URect, f32, &Sack<Item>)> = Vec::new();
        if panels.character {
            for (b, r, cell) in &layout.grids {
                if let Some(bag) = inv.bags.get(*b) {
                    grids.push((*r, *cell, bag));
                }
            }
        }
        if let Some(st) = stash.as_ref()
            && matches!(trade.open, Some(crate::trade::TradeOpen::Stash { .. }))
            && let Some((r, cell)) = trade_layout.grid
        {
            let pages = st.view(trade.transfer);
            if let Some(page) = pages.get(trade.tab.min(pages.len().saturating_sub(1))) {
                grids.push((r, cell, page));
            }
        }
        let inv_scale = 1.0 / win.scale_factor();
        let mut fill = |r: URect, c: Color| {
            wanted.push((
                URect::new(
                    r.x * inv_scale,
                    r.y * inv_scale,
                    r.w * inv_scale,
                    r.h * inv_scale,
                ),
                c,
            ));
        };
        for (r, cell, sack) in grids {
            if !contains(&r, p) {
                continue;
            }
            let (x, y) = drop_cell(p, &r, cell, (sack.width, sack.height), size);
            let overlap: Vec<_> = sack
                .items
                .iter()
                .filter(|q| overlaps(q, x, y, size))
                .collect();
            let cell_rect = |x: u32, y: u32, w: u32, h: u32| {
                URect::new(
                    r.x + x as f32 * cell,
                    r.y + y as f32 * cell,
                    w as f32 * cell,
                    h as f32 * cell,
                )
            };
            if overlap.len() >= 2 {
                for q in overlap {
                    fill(
                        cell_rect(q.x, q.y, q.size.w, q.size.h),
                        Color::srgba(1.0, 0.0, 0.0, 0.5),
                    );
                }
            } else {
                for q in overlap {
                    fill(
                        cell_rect(q.x, q.y, q.size.w, q.size.h),
                        Color::srgba(0.0, 0.0, 1.0, 0.5),
                    );
                }
                fill(
                    cell_rect(x, y, size.w, size.h),
                    Color::srgba(0.0, 1.0, 0.0, 0.5),
                );
            }
            break;
        }
    }
    let mut it = nodes.iter_mut();
    for (r, c) in &wanted {
        if let Some((_, mut node, mut bg)) = it.next() {
            let (l, t, w, h) = (Val::Px(r.x), Val::Px(r.y), Val::Px(r.w), Val::Px(r.h));
            if node.left != l {
                node.left = l;
            }
            if node.top != t {
                node.top = t;
            }
            if node.width != w {
                node.width = w;
            }
            if node.height != h {
                node.height = h;
            }
            bg.set_if_neq(BackgroundColor(*c));
        } else {
            commands.spawn((
                HeldOverlay,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(r.x),
                    top: Val::Px(r.y),
                    width: Val::Px(r.w),
                    height: Val::Px(r.h),
                    ..default()
                },
                BackgroundColor(*c),
                GlobalZIndex(90),
            ));
        }
    }
    for (e, _, _) in it {
        commands.entity(e).try_despawn();
    }
}

#[derive(Component)]
struct HeldIcon(Item);

/// The item on the cursor, drawn at the mouse position; the game cursor stays over it
/// (`CursorHandler::GetGameCursorType` 1).
#[allow(clippy::too_many_arguments)]
fn held_icon(
    mut commands: Commands,
    inv: Option<Res<Inventory>>,
    window: Query<&Window, With<PrimaryWindow>>,
    scale: Res<gd_ui_kit::state::settings::UiScaleOption>,
    mut icons: Query<(Entity, &HeldIcon, &mut Node)>,
    data: Res<gd_game_core::data::GameData>,
    mut tex: ResMut<gd_render::textures::Textures>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(inv) = inv else { return };
    let Ok(win) = window.single() else { return };
    let held = inv.held.as_ref();
    for (e, icon, mut node) in &mut icons {
        if held != Some(&icon.0) {
            commands.entity(e).try_despawn();
            continue;
        }
        // Centred on the cursor (`CursorHandlerItemMove::Render`).
        if let Some(c) = win.cursor_position() {
            let (w, h) = (
                if let Val::Px(w) = node.width { w } else { 0.0 },
                if let Val::Px(h) = node.height { h } else { 0.0 },
            );
            node.left = Val::Px(c.x - w * 0.5);
            node.top = Val::Px(c.y - h * 0.5);
        }
    }
    if let Some(h) = held
        && !icons.iter().any(|(_, i, _)| i.0 == *h)
        && let Some(info) = inv.info(h)
        && let Some(img) = tex.get(&data, &info.resolved.bitmap, true, &mut images)
    {
        let s = scale.factor(win.physical_width(), win.physical_height()) / win.scale_factor();
        let size = images
            .get(&img)
            .map(|i| i.size().as_vec2())
            .unwrap_or(Vec2::splat(32.0))
            * s;
        commands.spawn((
            HeldIcon(h.clone()),
            ImageNode::new(img),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(size.x),
                height: Val::Px(size.y),
                ..default()
            },
            GlobalZIndex(100),
        ));
    }
}
