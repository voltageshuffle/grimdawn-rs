//! Merchants, the personal stash and the crafter windows' UI state ([`Trade`]).
//!
//! * Clicking an `NpcMerchant` walks to it and opens its market window
//!   (`records/ui/vendors/vendor_mastertable.dbr`) next to the inventory. Reach, the market, its stock and
//!   faction tiers are `gd_game::trade`'s (the interact order → `ServiceAnswer::Opened`), the tabs and
//!   prices `gd_sim::items::market`'s. Left click buys into the bags; right click on a bag item sells
//!   it into the buyback tab; a held item clicked on the window is sold (`gd_game::trade::SellHeld`).
//!   Ctrl + left click on a stash stack opens the stack window (`ui/stack.rs`). The window opens on the
//!   first stocked tab (`gd_sim::items::market::opening_tab`) unless the tab picked last
//!   ([`Trade::vendor_tab`], any merchant) is stocked; empty tabs cannot be picked ([`vendor`]).
//! * The search boxes of the vendor, caravan and crafting windows: [`search`].
//! * Clicking an `NpcCaravan` opens the personal stash (`records/ui/caravan/caravan_window.dbr`): its pages
//!   come from and go back to the character file; the toggle button (`ToggleStashButton`) switches to the
//!   shared transfer stash (`transfer.gst`). Pages can be bought and sorted.
//! * Clicking an `NpcCrafter` opens the crafting window (`crafting.rs`); an `NpcTransmuter` the
//!   Illusionist's (`illusion.rs`); the Inventor / Runekeeper item-slot panels: [`panel`].
//!
//! The windows close with Esc or the inventory toggle (`ServiceRequest::Close`, whoever sends it), or when
//! gd-game closes the service (walking away, `ServiceAnswer::Closed`); [`service_answers`] owns the reset.
mod panel;
mod search;
mod vendor;

use crate::loot::PlayerProgress;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::save::item::Item;
pub use gd_game::stash::init_stash;
pub use gd_game::trade::{Markets, TradeOpen};
use gd_game::trade::{ServiceAnswer, ServiceRequest};
use gd_game_core::inventory::Inventory;
use gd_game_core::player::Player;
pub use gd_game_core::stash::Stash;
use gd_ui::state::inventory::UiCapture;
use gd_ui::state::trade::{
    Clipboard, DismissPrompt, Panel, Trade, TradeInputSet, TradeLayout, TradeSearch,
};
use panel::panel_clicks;
use search::{search_hits, search_input};
use vendor::{trade_clicks, trade_done};

pub struct TradePlugin;

impl Plugin for TradePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Trade>()
            .init_resource::<TradeLayout>()
            .init_resource::<Clipboard>()
            .add_message::<DismissPrompt>()
            .add_systems(
                PreUpdate,
                search_input
                    .run_if(in_state(gd_ui::state::session::AppState::InGame))
                    .after(bevy::input::InputSystems)
                    .before(gd_ui::gamemenu::GameMenuInputSet)
                    .before(gd_ui::state::options::OptionsInputSet),
            )
            .add_systems(
                Update,
                (
                    search_hits.after(trade_clicks),
                    gd_ui::trade::animate_search,
                )
                    .in_set(gd_ui_kit::UiPlaySet),
            )
            .add_systems(
                Update,
                (
                    service_answers,
                    trade_clicks,
                    trade_done,
                    crate::crafting::craft_clicks,
                    panel_clicks,
                    crate::crafting::dismantle,
                    crate::crafting::salvage,
                    crate::crafting::convert,
                    crate::reroll::reroll,
                    crate::crafting::slot_returns,
                    crate::crafting::dismantle_results,
                    crate::illusion::illusion_clicks,
                )
                    .chain()
                    .in_set(TradeInputSet)
                    .in_set(gd_ui_kit::UiInputSet)
                    .after(gd_game_core::inventory::InventorySet)
                    .after(init_stash)
                    .after(gd_game::trade::run_services)
                    .before(gd_game_core::player::PlayerSet),
            )
            .add_systems(
                Update,
                boxed_items
                    .run_if(resource_changed::<Trade>)
                    .after(TradeInputSet)
                    .before(gd_game::save::SaveStage::Save),
            );
    }
}

/// Keeps `gd_game::save::BoxedItems` equal to the items in the window boxes ([`Trade::slot`],
/// [`Trade::results`]), so every save writes them (they are in no bag while they sit there).
fn boxed_items(trade: Res<Trade>, mut boxed: ResMut<gd_game::save::BoxedItems>) {
    let now = || trade.slot.iter().chain(trade.results.iter().flatten());
    if !now().eq(boxed.0.iter()) {
        boxed.0 = now().cloned().collect();
    }
}

/// Closes the window (the inventory window closed, the Illusionist's close button) with `ServiceRequest::Close`
/// (the other windows' close buttons send it from `trade_clicks`),
/// resets it on any `ServiceRequest::Close` (the UI's window manager sends them too), and shows what a
/// `ServiceAnswer` says (the rules: `gd_game::trade`).
#[allow(clippy::too_many_arguments)]
fn service_answers(
    mut commands: Commands,
    fonts: Res<gd_ui_kit::text::FontCache>,
    // Option: the stash exists once `init_stash` ran; without it the crafter knows no learned blueprints.
    stash: Option<Res<Stash>>,
    mut trade: ResMut<Trade>,
    markets: Res<Markets>,
    mut panels: ResMut<gd_ui::UiPanels>,
    data: Res<gd_game_core::data::GameData>,
    mut answers: MessageReader<ServiceAnswer>,
    (mut requests, mut seen): (
        ResMut<bevy::ecs::message::Messages<ServiceRequest>>,
        Local<bevy::ecs::message::MessageCursor<ServiceRequest>>,
    ),
) {
    let closed = seen
        .read(&requests)
        .filter(|r| matches!(r, ServiceRequest::Close))
        .count()
        > 0;
    if closed && trade.open.is_some() {
        trade.illusion = Default::default();
        trade.open = None;
        trade.version += 1;
    }
    if let Some(open) = &trade.open {
        // The Illusionist's window stands alone (no inventory beside it).
        let alone = matches!(open, TradeOpen::Illusionist { .. });
        if (!panels.character && !alone) || trade.illusion.close {
            trade.illusion = Default::default();
            trade.open = None;
            trade.version += 1;
            requests.write(ServiceRequest::Close);
        }
    }
    for answer in answers.read() {
        let (open, record, faction) = match answer {
            ServiceAnswer::Closed => {
                if trade.open.take().is_some() {
                    trade.illusion = Default::default();
                    trade.version += 1;
                }
                continue;
            }
            ServiceAnswer::Opened {
                open,
                record,
                faction,
            } => (open, record, faction),
        };
        match open {
            TradeOpen::Illusionist { .. } => trade.illusion = Default::default(),
            TradeOpen::Inventor { .. } => {
                trade.slot = None;
                trade.panel = Panel::Recover;
                trade.recipe = None;
                trade.scroll = 0;
            }
            TradeOpen::Crafter { .. } => {
                let Some(rec) = data.db.get(record) else {
                    continue;
                };
                let learned = stash
                    .as_ref()
                    .map(|s| s.formulas.formula_records())
                    .unwrap_or_default();
                let list =
                    gd_ui::state::crafting::CraftList::build(&data, rec, &learned, &|t: &str| {
                        fonts.tag(t).to_string()
                    });
                info!("crafting: {} recipes", list.recipes.len());
                commands.insert_resource(list);
                trade.recipe = None;
                trade.scroll = 0;
                trade.panel = Panel::Craft;
            }
            TradeOpen::Vendor { .. } | TradeOpen::Stash { .. } | TradeOpen::Ascension { .. } => {}
        }
        trade.open = Some(open.clone());
        trade.faction = faction.clone();
        trade.npc_record = record.clone();
        trade.tab = 0;
        trade.search = TradeSearch::default();
        if let Some(f) = &trade.faction {
            trade.tab = f
                .open
                .iter()
                .position(|o| *o)
                .unwrap_or(gd_sim::items::market::BUYBACK_TAB);
        } else if let TradeOpen::Vendor { key, .. } = open {
            // `MarketWindow::SetVisible` (window show): starts from the tab shown last time.
            trade.tab = markets.0.get(key).map_or(0, |pages| {
                gd_sim::items::market::opening_tab(pages, trade.vendor_tab)
            });
        }
        trade.version += 1;
        if matches!(open, TradeOpen::Illusionist { .. }) {
            panels.character = false;
            continue;
        }
        panels.character = true;
        panels.skills = false;
        if panels.tab == 0 {
            panels.tab = 1;
        }
    }
}
