//! The Illusionist's window, input side (drawn by `ui/trade.rs`
//! `illusionist_window`; rules in `gd_game::illusion`).
//!
//! `TransmuteWindow::Load` / `BuildPage` / `Apply`: clicking one of the
//! nine equipment boxes (`equipHead` … `equipHandLeft`) selects that worn
//! item and lists the unlocked looks for it, 18 to a page
//! (`displayBoxCount`), opening on the page of its current look; clicking a
//! look makes it the slot's pending pick (one per slot, kept while other
//! slots are browsed); the cost is one `transmuteCost` per pick that is not
//! the item's own look; Apply Illusion sends every pick
//! ([`gd_game::illusion::ApplyIllusions`]) and clears them. PORT: the slot
//! selected on opening is the first worn item that takes an illusion, head
//! first.
//!
//! Weapon swap (`TransmuteWindow::OnButtonEvent` / `TransmuteWindow::SwapWeapons`):
//! `equipSwap1LeftButton` is down while set II is worn; a click swaps the
//! weapon sets like the character window's button
//! ([`gd_game::weapon_sets::SwapWeapons`] `from_window`). When the active set
//! changes the window drops the pending weapon picks.
//! PORT: the engine drops picks whose item is a `GAME::Weapon`; the port
//! drops both hand slots' picks.
//! The character view is the paper doll (`crate::doll`), wearing the pending
//! picks (`Paperdoll::SetItemAppearance`).
use crate::trade::TradeOpen;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::save::item::Item;
use gd_game::illusion::{ApplyIllusions, Illusions, choices};
use gd_game_core::inventory::Inventory;
use gd_ui::state::illusion::{EQUIP_BOXES, PAGE};
use gd_ui::state::inventory::UiCapture;
use gd_ui::state::trade::{Button, Trade, TradeLayout};
use gd_ui_kit::contains;

#[allow(clippy::too_many_arguments)]
pub fn illusion_clicks(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<TradeLayout>,
    mut trade: ResMut<Trade>,
    inv: Option<Res<Inventory>>,
    illusions: Option<Res<Illusions>>,
    choice: Res<gd_game_core::session::Choice>,
    data: Res<gd_game_core::data::GameData>,
    fonts: Res<gd_ui_kit::text::FontCache>,
    mut capture: ResMut<UiCapture>,
    mut apply: MessageWriter<ApplyIllusions>,
    mut swap: MessageWriter<gd_game::weapon_sets::SwapWeapons>,
    mut alternate: Local<Option<bool>>,
    #[cfg(feature = "dev")] mut tested: Local<bool>,
) {
    if !matches!(trade.open, Some(TradeOpen::Illusionist { .. })) {
        *alternate = None;
        return;
    }
    let (Some(inv), Some(ill)) = (inv, illusions) else {
        return;
    };
    // The active set changed: weapon picks go.
    if alternate.is_some_and(|a| a != inv.alternate) {
        trade.illusion.picks.retain(|(s, _)| !matches!(*s, 12 | 13));
        trade.version += 1;
    }
    *alternate = Some(inv.alternate);
    if trade.illusion.slot.is_none() {
        let first = EQUIP_BOXES.iter().map(|(_, s)| *s).find(|s| {
            inv.equipped[*s].as_ref().is_some_and(|it| {
                data.db.get(&it.base_name).is_some_and(|r| {
                    gd_sim::items::illusion::takes_illusion(gd_sim::items::illusion::item_type(
                        &r.class,
                    ))
                })
            })
        });
        if first.is_some() {
            trade.illusion.slot = first;
            trade.version += 1;
        }
    }
    let unlocked: usize = ill.file.entries.iter().map(|e| e.records.len()).sum();
    let current = trade.illusion.slot.and_then(|s| inv.equipped[s].as_ref());
    if current.map(|it| (it, unlocked)) != trade.illusion.built.as_ref().map(|(it, n)| (it, *n)) {
        let key = current.map(|it| (it.clone(), unlocked));
        let name = |r: &str| {
            data.db
                .get(r)
                .map(|rec| gd_sim::items::naming::record_description(&fonts.tags, rec))
                .unwrap_or_default()
        };
        let list = key
            .as_ref()
            .map(|(it, _)| choices(&data.db, &ill, it, choice.female, &name))
            .unwrap_or_default();
        let v = &mut trade.illusion;
        v.list = list;
        v.built = key;
        // Open on the page of the current look.
        let look = v.shown_look(&inv);
        v.page = look
            .and_then(|l| {
                v.list.iter().position(|r| {
                    r.eq_ignore_ascii_case(&l)
                        || gd_game::illusion::is_own_look(
                            &data.db,
                            &Item {
                                base_name: l.clone(),
                                ..Default::default()
                            },
                            r,
                        )
                })
            })
            .map_or(0, |i| i / PAGE);
        trade.version += 1;
    }
    #[cfg(feature = "dev")]
    if let Some(n) = hooks.items.illusion
        && let Some(s) = trade.illusion.slot
        && !trade.illusion.list.is_empty()
        && !*tested
    {
        *tested = true;
        let n = n.min(trade.illusion.list.len() - 1);
        let look = trade.illusion.list[n].clone();
        trade.illusion.page = n / PAGE;
        trade.illusion.picks.push((s, look));
        if hooks.items.illusion_apply {
            apply.write(ApplyIllusions {
                picks: std::mem::take(&mut trade.illusion.picks),
            });
            trade.illusion.close = true;
        }
        trade.version += 1;
    }
    let Ok(win) = window.single() else { return };
    let Some(p) = gd_ui_kit::cursor_px(win) else {
        return;
    };
    trade.hover = None;
    if !layout.window.is_some_and(|w| contains(&w, p)) {
        return;
    }
    capture.0 = true;
    trade.hover = layout
        .boxes
        .iter()
        .find(|(_, r)| contains(r, p))
        .map(|(it, _)| (it.clone(), None));
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some((b, _)) = layout.buttons.iter().find(|(_, r)| contains(r, p)) else {
        return;
    };
    let v = &mut trade.illusion;
    match *b {
        Button::IllusionSlot(s) => {
            if v.slot != Some(s) {
                v.slot = Some(s);
            }
        }
        Button::IllusionPick(i) => {
            if let (Some(s), Some(look)) = (v.slot, v.list.get(i).cloned()) {
                v.picks.retain(|(p, _)| *p != s);
                // Picking the worn look back drops the pick.
                let current = inv.equipped[s]
                    .as_ref()
                    .map(|it| gd_sim::items::illusion::look_record(it).to_owned());
                if current
                    .as_deref()
                    .is_none_or(|c| !c.eq_ignore_ascii_case(&look))
                {
                    v.picks.push((s, look));
                }
            }
        }
        Button::IllusionPage(next) => {
            let n = v.pages();
            v.page = if next {
                (v.page + 1).min(n - 1)
            } else {
                v.page.saturating_sub(1)
            };
        }
        Button::Go => {
            apply.write(ApplyIllusions {
                picks: std::mem::take(&mut v.picks),
            });
        }
        Button::IllusionSwap => {
            swap.write(gd_game::weapon_sets::SwapWeapons { from_window: true });
        }
        Button::Close => v.close = true,
        _ => return,
    }
    trade.version += 1;
}
