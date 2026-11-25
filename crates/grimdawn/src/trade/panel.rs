//! The item-slot panels' input (Runekeeper, crafter dismantle / reroll / salvage / convert tabs).
use super::*;
use gd_ui::state::trade::{Button, Panel, Trade, TradeLayout};
use gd_ui_kit::contains;

/// The item-slot windows: tab buttons, the slot, kind buttons, the action button and the dismantle prompt.
/// The slot (`AscensionItemBox_HandleClick`, `InventorItemBox::HandleClick`): with an item on the cursor that the
/// slot accepts, ANY press swaps (`InvRequest::Exchange`); empty cursor: left takes the item onto the cursor,
/// Shift + left gives it back to the player (`ItemBox_ReturnItemToPlayer`, `InvRequest::Stow`), right does
/// nothing. Closing the window or switching tab gives it back the same way.
#[allow(clippy::too_many_arguments)]
pub(super) fn panel_clicks(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<TradeLayout>,
    mut trade: ResMut<Trade>,
    inv: Option<Res<Inventory>>,
    data: Res<gd_game_core::data::GameData>,
    rt: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    mut notify: MessageWriter<gd_game_core::story::Notify>,
    mut capture: ResMut<UiCapture>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
    mut requests: MessageWriter<gd_game_core::inventory::InvRequest>,
    (confirm, stack): (
        Res<gd_ui::apply::InvConfirm>,
        Res<gd_ui::stack::StackWindow>,
    ),
) {
    let Some(inv) = inv else { return };
    // Which tabs the story tokens unlock.
    if matches!(trade.open, Some(TradeOpen::Inventor { .. })) {
        let reroll = gd_game::crafting::reroll_unlocked(&data, rt.as_deref());
        #[cfg(feature = "dev")]
        let reroll = reroll || hooks.items.reroll.is_some();
        let unlocked = [
            crate::crafting::dismantle_unlocked(&data, rt.as_deref()),
            crate::crafting::convert_unlocked(&data, rt.as_deref()),
            reroll,
        ];
        if trade.tabs_unlocked != unlocked {
            trade.tabs_unlocked = unlocked;
            trade.version += 1;
        }
    }
    if !trade.slot_panel() {
        for it in trade.take_boxes() {
            requests.write(gd_game_core::inventory::InvRequest::Stow(it));
        }
        trade.kind = None;
        trade.confirm = false;
        trade.go = false;
        return;
    }
    // The hovered box is found again below; off the window (or with no cursor) nothing is.
    if trade.hover.is_some() {
        trade.hover = None;
    }
    let Ok(win) = window.single() else { return };
    let Some(p) = gd_ui_kit::cursor_px(win) else {
        return;
    };
    // The dismantle warning (the dialog manager's Yes / No box) takes every click while up.
    if trade.confirm
        && let Some(rects) = layout.confirm
    {
        capture.0 = true;
        let look = rects.look(Some(p), buttons.pressed(MouseButton::Left));
        if trade.confirm_look != look {
            trade.confirm_look = look;
            trade.version += 1;
        }
        if buttons.just_pressed(MouseButton::Left)
            && let Some(hit) = rects.hit(p)
        {
            use gd_ui_kit::confirm::Hit;
            if hit == Hit::Close
                && let Some(pak) = gd_ui_kit::confirm::close_sound(&data)
            {
                sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
            }
            trade.confirm = false;
            trade.go = hit == Hit::Yes;
            trade.confirm_look = Default::default();
            trade.version += 1;
        }
        return;
    }
    // The stack window takes its own clicks; a modal question takes every click.
    if stack.covers(p) || gd_ui::apply::question_up(&inv, &trade, &confirm) {
        return;
    }
    let Some(wr) = layout.window.filter(|w| contains(w, p)) else {
        return;
    };
    let _ = wr;
    capture.0 = true;
    trade.hover = layout
        .boxes
        .iter()
        .find(|(_, r)| contains(r, p))
        .map(|(it, _)| (it.clone(), None));
    if layout.slot.is_some_and(|r| contains(&r, p))
        && let Some(it) = &trade.slot
    {
        trade.hover = Some((it.clone(), None));
    }
    let result = layout
        .results
        .iter()
        .find(|(_, r)| contains(r, p))
        .map(|(i, _)| *i);
    if let Some(it) = result.and_then(|i| trade.results[i].as_ref()) {
        trade.hover = Some((it.clone(), None));
    }
    let left = buttons.just_pressed(MouseButton::Left);
    if !left && !buttons.just_pressed(MouseButton::Right) {
        return;
    }
    if left && let Some((b, _)) = layout.buttons.iter().find(|(_, r)| contains(r, p)) {
        match *b {
            Button::Tab(t) => {
                for it in trade.take_boxes() {
                    requests.write(gd_game_core::inventory::InvRequest::Stow(it));
                }
                trade.panel = t;
                trade.kind = None;
                trade.confirm = false;
            }
            Button::Kind(k) => trade.kind = Some(k),
            Button::ConvertMode(r) => trade.random_set = r,
            Button::KeepItem | Button::KeepAddon | Button::RemoveAugment => {
                trade.salvage = Some(*b);
                trade.go = true;
            }
            Button::Go => match (&trade.panel, &trade.slot) {
                (Panel::Dismantle, Some(it))
                    if matches!(trade.open, Some(TradeOpen::Inventor { .. }))
                        && gd_game::crafting::dismantle_warns(&data.item_db(), it) =>
                {
                    trade.confirm = true
                }
                _ => trade.go = true,
            },
            Button::IllusionSlot(_)
            | Button::IllusionPick(_)
            | Button::IllusionPage(_)
            | Button::IllusionSwap
            | Button::Close => {}
        }
        trade.version += 1;
        return;
    }
    // A result box (`ResultItemBox_HandleClick`): take only. A held item is refused;
    // empty cursor, left: Shift gives it back to the player, else it goes onto the cursor; right does nothing.
    if let Some(i) = result
        && !trade.confirm
    {
        if inv.held.is_some() {
            info!("trade: a dismantle result box takes no item");
        } else if left && let Some(it) = trade.results[i].take() {
            if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
                requests.write(gd_game_core::inventory::InvRequest::Stow(it));
            } else {
                requests.write(gd_game_core::inventory::InvRequest::Exchange(Some(it)));
            }
            trade.version += 1;
        }
        return;
    }
    if layout.slot.is_some_and(|r| contains(&r, p)) && !trade.confirm {
        // The old cursor item comes back as a `SlotReturn`.
        if let Some(h) = &inv.held {
            if let Err(why) = trade.accepts(h, &data) {
                info!("trade: {why}");
                notify.write(gd_game_core::story::Notify(why));
                return;
            }
            trade.kind = None;
            requests.write(gd_game_core::inventory::InvRequest::Exchange(
                trade.slot.take(),
            ));
        } else if left && trade.slot.is_some() {
            let it = trade.slot.take();
            if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
                requests.write(gd_game_core::inventory::InvRequest::Stow(it.unwrap()));
            } else {
                requests.write(gd_game_core::inventory::InvRequest::Exchange(it));
            }
        } else {
            return;
        }
        trade.version += 1;
    }
}
