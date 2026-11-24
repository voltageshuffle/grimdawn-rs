//! The blacksmith (`NpcCrafter`): its crafting window lists blueprints
//! (`gd_sim::items::crafting`), sorted into the window's five category tabs
//! (relics, melee, ranged, armour, other; by the made item's
//! type, [`GROUPS`]). Reagents are counted and taken from the bags, the material storage
//! (`reagents.gst`), the personal stash and the transfer stash, in that order.
//! Crafting costs the blueprint's creation cost and puts the items in the bags.
//! The crafting panel's search box (`trade.rs` [`gd_ui::state::trade::TradeSearch`])
//! filters the list: a recipe shows when any of its search lines contains the
//! query, and a group whose recipes all filtered out hides its header
//! (row filter `CraftList::FilterRows`, query cache `CraftList::CacheQuery`). The
//! lines are the product item's `GenerateSearchText`, format-stripped and
//! lowercased (`CraftList::BuildSearchLines`: `Item::GenerateSearchText` with the player,
//! `LocalizerFormatStrip` + `towlower_l` per line; port
//! [`CraftList::fill_search_lines`], `gd_sim::items::market::search_line`).
//! The Inventor's tabs: dismantling ([`dismantle`]), salvage ([`salvage`]),
//! set conversion ([`convert`]); reroll in `reroll.rs`.
use crate::trade::Stash;
use bevy::prelude::*;
pub use gd_game::crafting::{convert_unlocked, dismantle_unlocked};
use gd_game_core::inventory::Inventory;
pub use gd_game_core::stash::count;
use gd_ui::state::crafting::CraftList;
use gd_ui::trade::CraftScrolled;
use gd_ui::window::{BarHold, ScrollOwner, ScrollThumb, bar_held};

/// Clicks in the crafting window: category tabs, recipe rows, the craft
/// button; the wheel scrolls the list. `GD_CRAFT` (hooks.rs): once the window is open, select the first
/// recipe whose record contains the text and craft.
#[allow(clippy::too_many_arguments)]
pub fn craft_clicks(
    hooks: Res<gd_game_core::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    layout: Res<gd_ui::state::trade::TradeLayout>,
    mut trade: ResMut<gd_ui::state::trade::Trade>,
    list: Option<Res<CraftList>>,
    stash: Option<Res<Stash>>,
    inv: Option<Res<Inventory>>,
    progress: Option<Res<crate::loot::PlayerProgress>>,
    mut capture: ResMut<gd_ui::state::inventory::UiCapture>,
    mut requests: MessageWriter<gd_game::crafting::CraftRequest>,
    (mut tested, mut rows, mut last_query): (
        Local<bool>,
        Local<Option<(u64, bool, usize)>>,
        Local<String>,
    ),
    (pause, confirm, stack): (
        Res<gd_game_core::pause::GamePause>,
        Res<gd_ui::apply::InvConfirm>,
        Res<gd_ui::stack::StackWindow>,
    ),
) {
    use gd_ui_kit::contains;
    if !matches!(trade.open, Some(crate::trade::TradeOpen::Crafter { .. })) {
        return;
    }
    let (Some(list), Some(stash), Some(inv), Some(progress)) = (list, stash, inv, progress) else {
        return;
    };
    let level = progress.progress.level.max(1) as u32;
    let mut do_craft = false;
    if let Some(want) = hooks.items.craft.clone()
        && !*tested
    {
        *tested = true;
        trade.recipe = list
            .recipes
            .iter()
            .position(|r| r.bp.record.contains(&want));
        if let Some(i) = trade.recipe {
            trade.tab = list.recipes[i].tab;
            let have: Vec<String> = list.recipes[i]
                .bp
                .reagents
                .iter()
                .map(|(n, q)| format!("{}/{q}", count(n, &inv, &stash)))
                .collect();
            info!(
                "craft test: {} cost {} reagents {have:?}",
                list.recipes[i].bp.record,
                list.recipes[i].bp.cost(level)
            );
        }
        do_craft = true;
        trade.version += 1;
    }
    let p = window
        .single()
        .ok()
        .and_then(|w| gd_ui_kit::cursor_px(&hooks, w));
    trade.hover = None;
    // The game menu, a modal question (`inventory::question_up`) or the stack window takes every click and
    // the wheel: none reaches the window under it (as `trade_clicks`).
    if pause.menu
        || gd_ui::apply::question_up(&inv, &trade, &confirm)
        || p.is_some_and(|p| stack.covers(p))
    {
        return;
    }
    // The search box (trade.rs module doc): a click on it clears a non-empty
    // text and focuses it; the clear button empties it; any other click
    // unfocuses it. Only the crafting tab shows the box.
    if let Some(p) = p
        && buttons.just_pressed(MouseButton::Left)
        && trade.panel == gd_ui::state::trade::Panel::Craft
    {
        if layout.search.is_some_and(|r| contains(&r, p)) {
            trade.search.text.clear();
            trade.search.edit = Default::default();
            trade.search.focused = true;
            trade.version += 1;
            capture.0 = true;
            return;
        }
        if layout.clear_search.is_some_and(|r| contains(&r, p)) {
            trade.search.text.clear();
            trade.search.edit = Default::default();
            trade.version += 1;
            capture.0 = true;
            return;
        }
        if trade.search.focused {
            trade.search.focused = false;
            trade.version += 1;
        }
    }
    // A new query starts the list at its top; one that shrinks the list must not strand the scroll past
    // its last page (counted once per `version`).
    let query = trade.search.query();
    if *last_query != query {
        *last_query = query;
        if trade.scroll != 0 {
            trade.scroll = 0;
            trade.version += 1;
        }
    }
    let key = (trade.version, list.search_lines_ready);
    let n = match *rows {
        Some((v, ready, n)) if (v, ready) == key => n,
        _ => {
            let n = list.row_count(trade.tab, &trade.search.query());
            *rows = Some((key.0, key.1, n));
            n
        }
    };
    // The rows the list shows at once (`ui/trade.rs`: its height over the row height).
    let visible = match (layout.list, layout.rows.first()) {
        (Some(l), Some((_, r))) if r.h > 0.0 => (l.h / r.h) as usize,
        _ => 1,
    }
    .max(1);
    let max_scroll = n.saturating_sub(visible);
    if trade.scroll > max_scroll {
        trade.scroll = max_scroll;
    }
    if let (Some(p), Some(wr)) = (p, layout.window)
        && contains(&wr, p)
    {
        capture.0 = true;
        trade.hover = layout
            .boxes
            .iter()
            .find(|(_, r)| contains(r, p))
            .map(|(it, _)| (it.clone(), None));
        if let Some(lr) = layout.list
            && contains(&lr, p)
            && scroll.delta.y != 0.0
        {
            let s = (trade.scroll as i32 - scroll.delta.y.signum() as i32 * 2)
                .clamp(0, max_scroll as i32) as usize;
            // A wheel step moves the pane in place ([`craft_scroll`], PERF-23): no rebuild.
            if s != trade.scroll {
                trade.scroll = s;
            }
        }
        if buttons.just_pressed(MouseButton::Left) {
            // The scroll bar's press and held repeat are [`craft_scroll`]'s (it moves the pane and
            // thumb in place, PERF-23); the bar sits outside the list rect, so a press on it matches
            // no row here.
            if let Some((t, _)) = layout.tabs.iter().find(|(_, r)| contains(r, p)) {
                trade.tab = *t;
                trade.recipe = None;
                trade.scroll = 0;
                trade.version += 1;
            } else if layout.list.is_some_and(|lr| contains(&lr, p)) {
                // Rows keep content coordinates (scroll 0, the pane shifts them): hit-test with the
                // scroll-shifted point, only inside the list rect.
                let line = layout.rows.first().map_or(0.0, |(_, r)| r.h);
                let q = Vec2::new(p.x, p.y + trade.scroll as f32 * line);
                if let Some((i, _)) = layout.rows.iter().find(|(_, r)| contains(r, q)) {
                    trade.recipe = Some(*i);
                    trade.version += 1;
                }
            } else if layout.craft.is_some_and(|r| contains(&r, p)) {
                do_craft = true;
            }
        }
    }
    if !do_craft {
        return;
    }
    let Some(r) = trade.recipe.and_then(|i| list.recipes.get(i)) else {
        return;
    };
    // `gd_game::crafting::apply_inventor` pays, takes the reagents and fills the bags.
    requests.write(gd_game::crafting::CraftRequest {
        bp: r.bp.clone(),
        enhancement: list.enhancement.clone(),
    });
    trade.version += 1;
}

/// Scrolls the crafting recipe list in place (PERF-23, the journal's `journal_live` pattern):
/// `Trade::scroll` is not in `TradeSig`, so a wheel step, a bar press or a held arrow / thumb drag
/// moves the pane ([`CraftScrolled`]) and the bar's thumb ([`ScrollThumb`]) instead of rebuilding the
/// window. The bar's press and held repeat ([`gd_ui::window::Bar::press`] / [`bar_held`]) live
/// here too; the wheel and the row clicks are [`craft_clicks`]'. Runs after the rebuild and clicks.
#[allow(clippy::too_many_arguments)]
pub fn craft_scroll(
    hooks: Res<gd_game_core::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    time: Res<Time<Real>>,
    mut trade: ResMut<gd_ui::state::trade::Trade>,
    layout: Res<gd_ui::state::trade::TradeLayout>,
    list: Option<Res<CraftList>>,
    inv: Res<Inventory>,
    mut capture: ResMut<gd_ui::state::inventory::UiCapture>,
    mut panes: Query<(&CraftScrolled, &mut Node), Without<ScrollThumb>>,
    mut thumbs: Query<(&ScrollThumb, &mut Node), Without<CraftScrolled>>,
    (pause, confirm, stack): (
        Res<gd_game_core::pause::GamePause>,
        Res<gd_ui::apply::InvConfirm>,
        Res<gd_ui::stack::StackWindow>,
    ),
    mut held: Local<Option<BarHold>>,
    mut px: Local<f32>,
    mut rows: Local<Option<(u64, bool, usize)>>,
) {
    if !matches!(trade.open, Some(crate::trade::TradeOpen::Crafter { .. })) {
        *held = None;
        return;
    }
    let line = layout.rows.first().map_or(0.0, |(_, r)| r.h);
    let scroll_px = trade.scroll as f32 * line;
    for (s, mut node) in &mut panes {
        let top = Val::Px((-s.clip_y - scroll_px) * s.inv);
        if node.top != top {
            node.top = top;
        }
    }
    for (t, mut node) in &mut thumbs {
        if t.owner != ScrollOwner::Craft {
            continue;
        }
        let off = t.travel * (scroll_px / t.max).clamp(0.0, 1.0);
        let top = Val::Px(((t.base_y + off).round() + t.round_add) * t.inv);
        if node.top != top {
            node.top = top;
        }
    }
    let Some(list) = list.as_deref() else {
        *held = None;
        return;
    };
    if line <= 0.0 {
        *held = None;
        return;
    }
    let key = (trade.version, list.search_lines_ready);
    let n = match *rows {
        Some((v, ready, n)) if (v, ready) == key => n,
        _ => {
            let n = list.row_count(trade.tab, &trade.search.query());
            *rows = Some((key.0, key.1, n));
            n
        }
    };
    let visible = match (layout.list, layout.rows.first()) {
        (Some(l), Some((_, r))) if r.h > 0.0 => (l.h / r.h) as usize,
        _ => 1,
    }
    .max(1);
    let max_scroll = n.saturating_sub(visible);
    let max_px = max_scroll as f32 * line;
    let p = window
        .single()
        .ok()
        .and_then(|w| gd_ui_kit::cursor_px(&hooks, w));
    // A modal (the game menu, a question, the stack box) takes the press and the hold.
    if pause.menu
        || gd_ui::apply::question_up(&inv, &trade, &confirm)
        || p.is_some_and(|p| stack.covers(p))
    {
        *held = None;
        return;
    }
    if !buttons.pressed(MouseButton::Left) {
        *held = None;
    }
    // A held arrow repeats, a held thumb follows the mouse; `px` keeps the sub-row remainder so the
    // repeat accumulates smoothly while the shown scroll stays whole rows.
    if let Some(hold) = held.as_mut() {
        let Some(p) = p else {
            *held = None;
            return;
        };
        let v = bar_held(
            layout.list_bar,
            hold,
            p.y,
            time.delta_secs() * 1000.0,
            *px,
            max_px,
        );
        *px = v;
        let s = ((v / line).round() as usize).min(max_scroll);
        if s != trade.scroll {
            trade.scroll = s;
        }
        capture.0 = true;
        return;
    }
    // A row is the list item's `LineHeight` (23 units) at the UI scale, so `line / 23` is that scale.
    if buttons.just_pressed(MouseButton::Left)
        && let Some(p) = p
        && let Some(wr) = layout.window
        && gd_ui_kit::contains(&wr, p)
        && let Some(bar) = layout.list_bar
        && let Some((v, hold)) = bar.press(p, line / 23.0, scroll_px, max_px)
    {
        *px = v;
        *held = Some(hold);
        let s = ((v / line).round() as usize).min(max_scroll);
        if s != trade.scroll {
            trade.scroll = s;
        }
        capture.0 = true;
    }
}

/// The dismantle tab's action (`trade.go`, set by `crate::trade` `panel_clicks` after its warning prompt): sends
/// the slot item to `gd_game::crafting` (rules, cost and notes there; window UI: `ui/trade.rs`
/// `dismantle_panel`; results: [`dismantle_results`]).
pub fn dismantle(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut trade: ResMut<gd_ui::state::trade::Trade>,
    mut requests: MessageWriter<gd_game::crafting::DismantleRequest>,
    mut stage: MessageWriter<gd_game::crafting::StageItem>,
    mut tested: Local<bool>,
) {
    // `GD_DISMANTLE` (hooks.rs): once the crafter is open, put the first bag item into the dismantle slot
    // and press the button.
    if hooks.items.dismantle
        && !*tested
        && matches!(trade.open, Some(crate::trade::TradeOpen::Inventor { .. }))
        && trade.slot.is_none()
    {
        *tested = true;
        trade.panel = gd_ui::state::trade::Panel::Dismantle;
        stage.write(gd_game::crafting::StageItem {
            which: gd_game::crafting::Stage::Contains(String::new()),
            go: true,
        });
    }
    if !trade.go
        || !matches!(trade.open, Some(crate::trade::TradeOpen::Inventor { .. }))
        || trade.panel != gd_ui::state::trade::Panel::Dismantle
    {
        return;
    }
    trade.go = false;
    // `gd_game::crafting::apply_inventor` runs the rules; a refused item comes back as a `SlotReturn`.
    if let Some(item) = trade.slot.take() {
        requests.write(gd_game::crafting::DismantleRequest(item));
    }
}

/// A dismantle's results ([`gd_game::crafting::DismantleResult`]): into the dismantle tab's result boxes
/// (`gd_game::crafting::receive_dismantled`) while the tab is shown, else back to the player
/// (`ControllerPlayer::GiveDismantledItemToPlayer`: the UI call returns false ->
/// `GiveItemToPlayer`, `InvRequest::Stow`). Both boxes play the tab's sound (`UIDismantleTab.sound`; PORT: taken
/// as its
/// `dismantleItemSound`).
pub fn dismantle_results(
    mut msgs: MessageReader<gd_game::crafting::DismantleResult>,
    mut trade: ResMut<gd_ui::state::trade::Trade>,
    data: Res<gd_game_core::data::GameData>,
    mut requests: MessageWriter<gd_game_core::inventory::InvRequest>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
) {
    for gd_game::crafting::DismantleResult(d) in msgs.read() {
        let shown = matches!(trade.open, Some(crate::trade::TradeOpen::Inventor { .. }))
            && trade.panel == gd_ui::state::trade::Panel::Dismantle;
        if !shown {
            for it in d.scrap.iter().chain(&d.bonus) {
                requests.write(gd_game_core::inventory::InvRequest::Stow(it.clone()));
            }
            continue;
        }
        let pak = data
            .db
            .get(gd_game::crafting::DISMANTLE)
            .and_then(|t| t.str("dismantleItemSound").map(str::to_owned))
            .filter(|p| !p.is_empty());
        for _ in d.scrap.iter().chain(&d.bonus) {
            if let Some(pak) = &pak {
                sound.write(gd_game_core::visuals::PlaySound {
                    pak: pak.clone(),
                    at: None,
                });
            }
        }
        for it in gd_game::crafting::receive_dismantled(&mut trade.results, d.clone()) {
            requests.write(gd_game_core::inventory::InvRequest::Stow(it));
        }
        trade.version += 1;
    }
}

/// Puts what the Inventor's requests leave back into the window's slot (a
/// refused item, what did not fit the bags, a staged hook item), presses the
/// button of a staged item and rebuilds the window.
pub fn slot_returns(
    mut msgs: MessageReader<gd_game::crafting::SlotReturn>,
    mut trade: ResMut<gd_ui::state::trade::Trade>,
) {
    for m in msgs.read() {
        if let Some(item) = &m.item {
            trade.slot = Some(item.clone());
        }
        trade.go |= m.go;
        trade.version += 1;
    }
}

/// The salvage tab (`dividing_table.dbr`, window `InventorWindow::Load`, panel
/// loader `UISalvagePanel::Load`, update `UISalvagePanel::Update`).
/// The slot takes an equipment item; the cost is `salvage_cost`; the buttons
/// are all off when the item is missing or the money short (`UISalvagePanel::Update`:
/// `CurrentMoney < cost`); otherwise Keep Item and Keep Add-on need a
/// component (`HasRelic`) and Remove Augment an augment (`HasEnchantment`).
/// `EnchanterRecoveryConfigCmd::Execute`: Keep Add-on takes the component out
/// as an item of its own (given to the player) and destroys the item;
/// Remove Augment destroys only the augment (`RemoveEnchantment`); Keep Item
/// destroys the component (`RemoveRelic`) and the augment and gives the item
/// back. The cost is taken from the player's money in each case.
pub fn salvage(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut trade: ResMut<gd_ui::state::trade::Trade>,
    mut requests: MessageWriter<gd_game::crafting::SalvageRequest>,
    mut stage: MessageWriter<gd_game::crafting::StageItem>,
    mut tested: Local<bool>,
) {
    use gd_ui::state::trade::Button;
    if !matches!(trade.open, Some(crate::trade::TradeOpen::Inventor { .. }))
        || trade.panel != gd_ui::state::trade::Panel::Recover
    {
        return;
    }
    // `GD_SALVAGE` (hooks.rs): once, put the first bag item in the slot and press the button.
    if let Some(k) = hooks.items.salvage.clone()
        && !*tested
        && trade.slot.is_none()
    {
        *tested = true;
        trade.salvage = Some(match k.as_str() {
            "addon" => Button::KeepAddon,
            "augment" => Button::RemoveAugment,
            _ => Button::KeepItem,
        });
        stage.write(gd_game::crafting::StageItem {
            which: gd_game::crafting::Stage::Contains(
                hooks.items.salvage_pick.clone().unwrap_or_default(),
            ),
            go: k != "look",
        });
    }
    if !trade.go {
        return;
    }
    trade.go = false;
    let (Some(which), Some(item)) = (trade.salvage.take(), trade.slot.take()) else {
        return;
    };
    let kind = match which {
        Button::KeepAddon => gd_game::crafting::SalvageKind::KeepAddon,
        Button::RemoveAugment => gd_game::crafting::SalvageKind::RemoveAugment,
        _ => gd_game::crafting::SalvageKind::KeepItem,
    };
    requests.write(gd_game::crafting::SalvageRequest { item, kind });
}

/// The convert tab (`convert_table.dbr`, rules in `gd_sim::items::convert`,
/// GrimDawn.exe `UIEnchanterConvertTab::OnButton` button handler): the create button
/// converts the slot item at once (no prompt; a `blacklistedSets` item gets
/// the refusal notice instead). The new item goes to the bags; the slot
/// keeps it when they are full. `GD_CONVERT` (hooks.rs): once the Inventor is open, puts the first bag
/// set item into the slot and presses the button (money and reagents provided).
pub fn convert(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut trade: ResMut<gd_ui::state::trade::Trade>,
    mut requests: MessageWriter<gd_game::crafting::ConvertRequest>,
    mut stage: MessageWriter<gd_game::crafting::StageItem>,
    mut tested: Local<bool>,
) {
    if !matches!(trade.open, Some(crate::trade::TradeOpen::Inventor { .. })) {
        return;
    }
    let test = hooks.items.convert.clone();
    if let Some(k) = &test
        && !*tested
        && trade.slot.is_none()
    {
        *tested = true;
        trade.panel = gd_ui::state::trade::Panel::Convert;
        trade.random_set = k == "random";
        // The money and reagents are provided with the item.
        stage.write(gd_game::crafting::StageItem {
            which: gd_game::crafting::Stage::Convertible {
                random: trade.random_set,
            },
            go: true,
        });
    }
    if trade.panel != gd_ui::state::trade::Panel::Convert || !trade.go {
        return;
    }
    trade.go = false;
    let Some(item) = trade.slot.take() else {
        return;
    };
    requests.write(gd_game::crafting::ConvertRequest {
        item,
        random: trade.random_set,
        test: test.is_some(),
    });
}
