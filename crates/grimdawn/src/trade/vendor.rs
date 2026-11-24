//! The vendor and caravan windows' input and the trade answers.
use super::*;
use gd_ui::state::trade::{Button, DismissPrompt, Prompt, Trade, TradeLayout};
use gd_ui_kit::contains;

/// The vendor / caravan window's clicks: the dialog box ([`Prompt`]), buying, selling, the stash pages.
#[allow(clippy::too_many_arguments)]
pub(super) fn trade_clicks(
    hooks: Res<gd_game_core::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<TradeLayout>,
    mut trade: ResMut<Trade>,
    markets: Res<Markets>,
    stash: Option<Res<Stash>>,
    inv: Option<Res<Inventory>>,
    progress: Option<Res<PlayerProgress>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    mut capture: ResMut<UiCapture>,
    data: Res<gd_game_core::data::GameData>,
    (mut tested, mut script, time, mut sound, mut dismiss, mut close): (
        Local<bool>,
        Local<Option<Vec<gd_game_core::hooks::Click>>>,
        Res<Time>,
        MessageWriter<gd_game_core::visuals::PlaySound>,
        MessageReader<DismissPrompt>,
        MessageWriter<ServiceRequest>,
    ),
    (mut buy_req, mut sell_req, mut stash_req, mut apply, mut sell_held, mut stack): (
        MessageWriter<gd_game::trade::BuyRequest>,
        MessageWriter<gd_game::trade::SellRequest>,
        MessageWriter<gd_game_core::stash::StashRequest>,
        MessageWriter<gd_game_core::inventory::ApplyRequest>,
        MessageWriter<gd_game::trade::SellHeld>,
        ResMut<gd_ui::stack::StackWindow>,
    ),
    confirm: Res<gd_ui::apply::InvConfirm>,
) {
    if dismiss.read().count() > 0 && (trade.prompt.is_some() || trade.confirm) {
        trade.prompt = None;
        trade.confirm = false;
        trade.go = false;
        trade.confirm_look = Default::default();
        trade.version += 1;
    }
    let (Some(inv), Some(progress)) = (inv, progress) else {
        return;
    };
    let idb = &data.item_db();
    let difficulty = loaded.as_ref().map(|c| c.0.info.difficulty()).unwrap_or(0);
    let discount = trade
        .faction
        .as_ref()
        .map_or(0, gd_game::trade::FactionView::discount);
    let Some(open) = trade.open.clone() else {
        if trade.hover.is_some() || trade.prompt.is_some() {
            trade.hover = None;
            trade.prompt = None;
        }
        // A component taken from the stash stays selected only while it is open.
        if inv
            .applying
            .as_ref()
            .is_some_and(|a| matches!(a.src, gd_game_core::inventory::ItemLoc::Stash { .. }))
        {
            apply.write(gd_game_core::inventory::ApplyRequest::Cancel);
        }
        return;
    };
    // The dialog box ([`Prompt`]) takes every click while up: Yes on the buy question sends the purchase, Ok /
    // No / the close button only close it.
    if let Some(prompt) = trade.prompt {
        use gd_ui_kit::confirm::Hit;
        capture.0 = true;
        trade.hover = None;
        let Ok(win) = window.single() else { return };
        let p = gd_ui_kit::cursor_px(&hooks, win);
        let hit = match (prompt, layout.confirm, layout.ok) {
            (Prompt::BuyPage { .. }, Some(r), _) => p.and_then(|p| r.hit(p)),
            (Prompt::Message(_), _, Some((ok, close))) => p.and_then(|p| {
                if contains(&ok, p) {
                    Some(Hit::Yes)
                } else if close.is_some_and(|c| contains(&c, p)) {
                    Some(Hit::Close)
                } else {
                    None
                }
            }),
            _ => None,
        };
        let look = (hit, hit.is_some() && buttons.pressed(MouseButton::Left));
        if trade.confirm_look != look {
            trade.confirm_look = look;
            trade.version += 1;
        }
        if buttons.just_pressed(MouseButton::Left)
            && let Some(hit) = hit
        {
            if hit == Hit::Close
                && let Some(pak) = gd_ui_kit::confirm::close_sound(&data)
            {
                sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
            }
            if let (Hit::Yes, Prompt::BuyPage { transfer, cost }) = (hit, prompt) {
                info!("stash: buying a page for {cost}");
                stash_req.write(gd_game_core::stash::StashRequest::BuyPage { transfer });
            }
            trade.prompt = None;
            trade.confirm_look = Default::default();
            trade.version += 1;
        }
        return;
    }
    // `GD_TRANSFER` (hooks.rs): once, show the transfer stash and move the first personal stash item into it.
    if hooks.items.transfer
        && matches!(open, TradeOpen::Stash { .. })
        && stash.is_some()
        && !*tested
    {
        *tested = true;
        trade.transfer = true;
        // `GD_REAGENTS`: show the Materials / Components tab.
        trade.reagents = hooks.items.reagents;

        stash_req.write(gd_game_core::stash::StashRequest::TestTransfer);
        trade.version += 1;
        return;
    }
    // `GD_TRADE_BUY` (hooks.rs): once, buy the first item of the open merchant's first tab and sell the
    // first bag item.
    if hooks.items.trade_buy
        && let TradeOpen::Vendor { key, market, .. } = &open
        && let Some(first) = markets.0.get(key).and_then(|m| m[0].items.first())
        && !*tested
        && let Some(shop) = trade.shop(loaded.as_deref())
    {
        *tested = true;
        let (buy, _) =
            gd_sim::items::market::prices(idb, market, &first.item, difficulty, discount)
                .unwrap_or((0, 0));
        buy_req.write(gd_game::trade::BuyRequest {
            shop: shop.clone(),
            tab: 0,
            cell: (first.x, first.y),
        });
        if let Some(p) = inv.bags[0].items.first() {
            sell_req.write(gd_game::trade::SellRequest {
                shop,
                bag: 0,
                cell: (p.x, p.y),
            });
        }
        info!("trade test: buying for {buy} (gold {})", progress.gold);
        trade.version += 1;
        return;
    }
    // `GD_INV_CLICKS` (hooks.rs): a scripted click inside this window clicks it (the stash's apply mode in a run).
    let script = script.get_or_insert_with(|| hooks.items.inv_clicks.clone());
    let scripted = matches!(open, TradeOpen::Stash { .. })
        .then(|| gd_game_core::hooks::take_due(script, time.elapsed_secs()))
        .flatten()
        .filter(|c| layout.window.is_some_and(|r| contains(&r, c.at)));
    let Ok(win) = window.single() else { return };
    let Some(p) = scripted
        .map(|c| c.at)
        .or_else(|| gd_ui_kit::cursor_px(&hooks, win))
    else {
        return;
    };
    // The vendor / caravan / crafter / Runekeeper close button (`ui/trade.rs` `close_field`; the Illusionist's
    // is `illusion_clicks`'): closes the window as Esc does. Not under the stack window or a modal question.
    if !matches!(open, TradeOpen::Illusionist { .. })
        && buttons.just_pressed(MouseButton::Left)
        && layout
            .buttons
            .iter()
            .any(|(b, r)| *b == Button::Close && contains(r, p))
        && !stack.covers(p)
        && !gd_ui::apply::question_up(&inv, &trade, &confirm)
    {
        capture.0 = true;
        close.write(ServiceRequest::Close);
        return;
    }
    if matches!(
        open,
        TradeOpen::Crafter { .. }
            | TradeOpen::Inventor { .. }
            | TradeOpen::Ascension { .. }
            | TradeOpen::Illusionist { .. }
    ) {
        return;
    }
    let Some(wr) = layout.window else { return };
    trade.hover = None;
    // The stack window takes its own clicks (`ui/stack.rs`).
    if stack.covers(p) {
        return;
    }
    // A modal question (destroy, tonic/writ use, apply / reagent confirmations: `inventory::question_up`)
    // takes every click: none reaches the window under it.
    if gd_ui::apply::question_up(&inv, &trade, &confirm) {
        return;
    }
    let (left, right) = match scripted {
        Some(c) => {
            info!(
                "scripted stash {} click at {}",
                if c.right { "right" } else { "left" },
                c.at
            );
            (!c.right, c.right)
        }
        None => (
            buttons.just_pressed(MouseButton::Left),
            buttons.just_pressed(MouseButton::Right),
        ),
    };
    // The search box (module doc): a click on it clears a non-empty text and
    // focuses it; the clear button empties it; any other click unfocuses it.
    if left && matches!(open, TradeOpen::Vendor { .. } | TradeOpen::Stash { .. }) {
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
    if !contains(&wr, p) {
        return;
    }
    capture.0 = true;
    // Every left click in the window hides the stack window first (a tab, toggle or sort would leave its
    // OK splitting a hidden page's item); Ctrl + left on a stash stack opens it again below.
    if left {
        stack.hide();
    }
    // Transfer view: the Materials / Components tabs and their boxes.
    if left && let Some((t, _)) = layout.reagent_tabs.iter().find(|(_, r)| contains(r, p)) {
        trade.reagents = Some(*t);
        trade.version += 1;
        return;
    }
    if let Some((record, _)) = layout.reagent_boxes.iter().find(|(_, r)| contains(r, p))
        && let Some(stash) = stash.as_ref()
    {
        let have = stash
            .materials
            .entries
            .iter()
            .find(|e| e.record.eq_ignore_ascii_case(record))
            .map_or(0, |e| e.count);
        trade.hover = Some((
            Item {
                base_name: record.clone(),
                stack_count: have.max(1),
                ..Default::default()
            },
            None,
        ));
        if !left {
            return;
        }
        // Store a held stack of the material or take from the store
        // (`StashRequest::Material`; Shift takes a stack into the bags).
        stash_req.write(gd_game_core::stash::StashRequest::Material {
            record: record.clone(),
            shift: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        });
        return;
    }
    // Sort the shown page (`StashRequest::Sort`, the rule is documented there).
    if left
        && let Some(r) = layout.sort
        && contains(&r, p)
        && stash.is_some()
    {
        stash_req.write(gd_game_core::stash::StashRequest::Sort {
            transfer: trade.transfer,
            page: trade.tab,
        });
        return;
    }
    // The buy tab asks first ([`Prompt`], `CaravanWindow::PromptBuy`): short of gold, the Ok box
    // `tagTransferError01`; else the Yes / No question.
    if left
        && let Some(r) = layout.buy
        && contains(&r, p)
        && let Some(cost) = stash
            .as_deref()
            .and_then(|s| gd_game_core::stash::next_page_cost(&data.db, s, trade.transfer))
    {
        trade.prompt = Some(if progress.gold < cost {
            Prompt::Message(gd_game::trade::TradeRefused {
                tag: "tagTransferError01",
                arg: Some(cost),
            })
        } else {
            Prompt::BuyPage {
                transfer: trade.transfer,
                cost,
            }
        });
        trade.confirm_look = Default::default();
        trade.version += 1;
        return;
    }
    if left
        && let Some(r) = layout.toggle
        && contains(&r, p)
    {
        trade.transfer = !trade.transfer;
        trade.reagents = None;
        trade.tab = 0;
        trade.version += 1;
        return;
    }
    if left && let Some((t, _)) = layout.tabs.iter().find(|(_, r)| contains(r, p)) {
        trade.reagents = None;
        // A faction tier above the player's rank is browsed; its purchases are refused
        // (`gd_game::trade::BuyRequest`).
        // An empty vendor tab's button is disabled (`tab_enabled`).
        if let TradeOpen::Vendor { key, .. } = &open
            && !markets
                .0
                .get(key)
                .is_some_and(|m| gd_sim::items::market::tab_enabled(m, *t))
        {
            return;
        }
        trade.tab = *t;
        if matches!(open, TradeOpen::Vendor { .. }) && trade.faction.is_none() {
            trade.vendor_tab = *t;
        }
        trade.version += 1;
        return;
    }
    let Some((gr, cell)) = layout.grid else {
        return;
    };
    if !contains(&gr, p) {
        return;
    }
    let (cx, cy) = (((p.x - gr.x) / cell) as u32, ((p.y - gr.y) / cell) as u32);
    match &open {
        TradeOpen::Crafter { .. }
        | TradeOpen::Inventor { .. }
        | TradeOpen::Ascension { .. }
        | TradeOpen::Illusionist { .. } => {}
        TradeOpen::Vendor { key, market, .. } => {
            // A held item goes to the merchant: sold whole, a split-off part of a stack too
            // (`CursorHandlerItemMove::PrimaryMarketActivate`, `gd_game::trade::SellHeld`); nothing is bought
            // while an item is held (the click is the cursor handler's).
            if inv.held.is_some() {
                if left && let Some(shop) = trade.shop(loaded.as_deref()) {
                    sell_held.write(gd_game::trade::SellHeld { shop });
                }
                return;
            }
            let Some((item, buy)) = gd_sim::items::market::stock_price(
                markets.0.get(key).map_or(&[][..], Vec::as_slice),
                markets.1.get(key),
                idb,
                market,
                trade.tab,
                (cx, cy),
                difficulty,
                discount,
            ) else {
                return;
            };
            trade.hover = Some((item, Some(buy)));
            if !(left || right) {
                return;
            }
            // The purchase (`gd_game::trade::BuyRequest`; refused when the gold
            // or the bag room is short).
            if let Some(shop) = trade.shop(loaded.as_deref()) {
                buy_req.write(gd_game::trade::BuyRequest {
                    shop,
                    tab: trade.tab,
                    cell: (cx, cy),
                });
            }
        }
        TradeOpen::Stash { .. } => {
            let Some(stash) = stash else { return };
            let tab = trade
                .tab
                .min(stash.view(trade.transfer).len().saturating_sub(1));
            // Apply mode (`gd_game::inventory`, look `ui/apply.rs`): right click
            // selects a component / augment here; in the mode either button uses
            // it on the item under the cursor (`ApplyRequest::Click`: the
            // handlers' `SecondaryStashActivate` / `SecondaryTransferActivate`
            // are the primary ones).
            let transfer = trade.transfer;
            let loc = move |x, y| gd_game_core::inventory::ItemLoc::Stash {
                transfer,
                page: tab,
                x,
                y,
            };
            if (left || right) && inv.held.is_none() && inv.applying.is_some() {
                if inv.applying.as_ref().is_some_and(|a| a.confirm.is_none()) {
                    apply.write(gd_game_core::inventory::ApplyRequest::Click(Some(loc(
                        cx, cy,
                    ))));
                }
                return;
            }
            let under = stash
                .view(trade.transfer)
                .get(tab)
                .and_then(|p| p.item_at(cx, cy));
            if right
                && inv.held.is_none()
                && let Some(q) = under
            {
                apply.write(gd_game_core::inventory::ApplyRequest::Start(loc(q.x, q.y)));
            }
            if let Some(placed) = under {
                trade.hover = Some((placed.item.clone(), None));
            }
            if !left {
                return;
            }
            // Ctrl + left click (Shift not held) on a stack of more than one opens the stack window
            // (`UIPlayerHud::OpenStackWindow`, `ui/stack.rs`; hidden above by every click).
            let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
            if inv.held.is_none()
                && !shift
                && keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
            {
                if let Some(q) = under
                    && gd_game_core::stack::count(&q.item) > 1
                {
                    stack.open(
                        loc(q.x, q.y),
                        Vec2::new(
                            gr.x + (q.x + q.size.w) as f32 * cell,
                            gr.y + q.y as f32 * cell,
                        ),
                        gd_game_core::stack::count(&q.item),
                        &data,
                    );
                }
                return;
            }
            // Pick up / place / swap with the held item (`StashRequest::Click`).
            stash_req.write(gd_game_core::stash::StashRequest::Click {
                transfer: trade.transfer,
                page: tab,
                cell: (cx, cy),
                cursor: Vec2::new((p.x - gr.x) / cell, (p.y - gr.y) / cell),
            });
        }
    }
}

/// The trade answers: a done request rebuilds the window (a bought page shows), a refusal shows its Ok box
/// ([`Prompt::Message`]) while the window is open.
pub(super) fn trade_done(
    mut done: MessageReader<gd_game::trade::TradeDone>,
    mut refused: MessageReader<gd_game::trade::TradeRefused>,
    mut trade: ResMut<Trade>,
) {
    for d in done.read() {
        if let Some(page) = d.page {
            trade.tab = page;
        }
        trade.version += 1;
    }
    for r in refused.read() {
        if trade.open.is_some() {
            info!("trade: refused: {}", r.tag);
            trade.prompt = Some(Prompt::Message(*r));
            trade.confirm_look = Default::default();
            trade.version += 1;
        }
    }
}
