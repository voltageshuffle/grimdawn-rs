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

use crate::inventory::{Inventory, UiCapture};
use crate::loot::PlayerProgress;
use crate::player::Player;
pub(crate) use crate::ui::contains;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::save::item::Item;
use gd_formats::ui::Rect as URect;
pub use gd_game::stash::Stash;
pub use gd_game::stash::{STASH_W, init_stash};
pub use gd_game::trade::{FactionView, Markets, TradeOpen, VENDOR_W};
use gd_game::trade::{ServiceAnswer, ServiceRequest};
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
                    .run_if(in_state(crate::session::AppState::InGame))
                    .after(bevy::input::InputSystems)
                    .before(crate::ui::gamemenu::GameMenuInputSet)
                    .before(crate::ui::options::OptionsInputSet),
            )
            .add_systems(
                Update,
                (
                    search_hits.after(trade_clicks),
                    crate::ui::trade::animate_search,
                )
                    .in_set(crate::ui::UiPlaySet),
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
                    .in_set(crate::ui::UiInputSet)
                    .after(crate::inventory::InventorySet)
                    .after(init_stash)
                    .after(gd_game::trade::run_services)
                    .before(crate::player::PlayerSet),
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

/// The edit boxes' clipboard (vendor search, stack window, devotion search). PORT: process-local; the engine uses
/// the OS one.
#[derive(Resource, Default)]
pub struct Clipboard(pub String);

/// The NPC windows' input (merchant, stash, crafting panels): the systems that
/// take only the clicks no window took order after it (`ui/apply.rs` `world_click`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TradeInputSet;

/// The crafter window's panels (`enchanterArtifactTab` crafting, the
/// dismantle and reroll tabs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Panel {
    #[default]
    Craft,
    /// The Inventor's component salvage tab (`enchanterRecoverTab`,
    /// `dividing_table.dbr`).
    Recover,
    Dismantle,
    Convert,
    Reroll,
}

/// A clickable control of the item-slot panels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Button {
    Tab(Panel),
    Kind(crate::reroll::Kind),
    /// The panel's action button (reroll / dismantle / ascend).
    Go,
    /// The salvage tab's buttons (`recoverItemButton`, `recoverRelicButton`,
    /// `removeAugmentButton`).
    KeepItem,
    KeepAddon,
    RemoveAugment,
    /// The convert tab's mode buttons (`true`: random set).
    ConvertMode(bool),
    /// The Illusionist's equipment slots (worn slot index), unlocked looks
    /// (index into the list), page buttons (`true`: next) and close button.
    IllusionSlot(usize),
    IllusionPick(usize),
    IllusionPage(bool),
    /// The Illusionist's weapon-swap button (`illusion.rs`).
    IllusionSwap,
    Close,
}

#[derive(Resource, Default)]
pub struct Trade {
    pub open: Option<TradeOpen>,
    /// Bumped on every change (UI rebuild signature).
    pub version: u64,
    pub tab: usize,
    /// The tab the vendor window showed last (kept across merchants and
    /// openings; the window object persists in the engine).
    pub vendor_tab: usize,
    /// The caravan window shows the transfer stash (not the personal one).
    pub transfer: bool,
    /// In the transfer view: the Materials (0) or Components (1) tab instead
    /// of a page (`MaterialWindow` / `RelicWindow`, the `reagents.gst` store).
    pub reagents: Option<usize>,
    /// Crafting: the selected recipe (`CraftList` index) and the list scroll.
    pub recipe: Option<usize>,
    pub scroll: usize,
    /// The open window's NPC record (name / profession headings).
    pub npc_record: String,
    /// A faction merchant's faction standing.
    pub faction: Option<FactionView>,
    /// The Inventor-style window's open panel (`inventor_mastertable` tabs).
    pub panel: Panel,
    /// The item in the open panel's item slot (dismantle / reroll tab, the
    /// Runekeeper's window), and the reroll kind picked for it.
    pub slot: Option<Item>,
    /// The dismantle tab's take-only result boxes `dismantleBox1` (scrap) / `dismantleBox2` (bonus or the
    /// returned component), filled by `gd_game::crafting::receive_dismantled` (`crafting::dismantle_results`).
    pub results: [Option<Item>; 2],
    pub kind: Option<crate::reroll::Kind>,
    /// The dismantle warning prompt is showing (`ui/confirm.rs`'s box) and what it shows of the mouse (the
    /// look of [`Trade::prompt`]'s box too).
    pub confirm: bool,
    pub confirm_look: crate::ui::confirm::Look,
    /// The vendor / caravan window's dialog box ([`Prompt`]).
    pub prompt: Option<Prompt>,
    /// The panel's action button was pressed (`reroll` / `dismantle` run it).
    pub go: bool,
    /// Which of the dismantle / reroll tabs the player's story tokens unlock
    /// (dismantle / convert / reroll: `MainPlayerCanUseDismantle` / `Convert` / `Reroll`).
    pub tabs_unlocked: [bool; 3],
    /// The salvage button pressed (`go` is set with it).
    pub salvage: Option<Button>,
    /// The convert tab's mode: `randomSetButton` (else `sameSetButton`, the
    /// default).
    pub random_set: bool,
    /// Item under the cursor in the trade window and its price
    /// (`tooltip.rs`).
    pub hover: Option<(Item, Option<u64>)>,
    /// The Illusionist window's state (`illusion.rs`).
    pub illusion: crate::illusion::IllusionView,
    /// The open window's search box (vendor, stash, crafting panel).
    pub search: TradeSearch,
}

/// The trade windows' search box (module doc): its text, focus, and what the
/// text finds in the open market or stash ([`search_hits`]); the crafting
/// panel filters its recipe list with the query instead (`crafting.rs`).
#[derive(Default)]
pub struct TradeSearch {
    pub text: String,
    pub focused: bool,
    /// Caret, selection and key repeat (`gd_sim::edit`).
    pub edit: gd_sim::edit::EditState,
    /// Per tab / page, the grid cells of the matching items (`gd_sim::items::market::search_market`).
    pub hits: Vec<Vec<(u32, u32)>>,
    /// The query and window key `hits` were found for.
    searched: Option<(String, String)>,
}

impl TradeSearch {
    /// The trimmed, lowercased query (`gd_sim::items::market::search_query`).
    pub fn query(&self) -> String {
        gd_sim::items::market::search_query(&self.text)
    }
}

/// Answers No to [`Trade::prompt`] or the dismantle warning ([`Trade::confirm`]) (the Esc dispatcher's;
/// [`trade_clicks`] owns the prompt state).
#[derive(Message)]
pub struct DismissPrompt;

/// The dialog manager's box over the vendor / caravan window (`ui/confirm.rs`); it takes every click while up,
/// Esc and its close button answer No ([`trade_clicks`]). `BuyPage`: the caravan's buy tab question
/// `tagTransferPurchase01` with the cost (`gd_game::stash::StashRequest::BuyPage`, sent on Yes); `Message`: an
/// Ok box, a refusal of gd-game ([`gd_game::trade::TradeRefused`]) or the buy tab's short-of-gold box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prompt {
    BuyPage { transfer: bool, cost: u64 },
    Message(gd_game::trade::TradeRefused),
}

impl Prompt {
    /// The box's text tag and its `{%d0}`.
    pub fn text(&self) -> (&'static str, Option<u64>) {
        match *self {
            Prompt::BuyPage { cost, .. } => ("tagTransferPurchase01", Some(cost)),
            Prompt::Message(r) => (r.tag, r.arg),
        }
    }
}

/// The vendor search box record (`marketSearchBox` of both market windows).
pub const SEARCH_BOX: &str = "records/ui/vendors/vendor_searchbox.dbr";

/// The open window's search box record (its `Style` and `Width` size the text,
/// [`search_input`]): the vendor's, the caravan window's `searchBox`, the
/// crafting panel's `searchBox`; `None` for a window without one.
pub fn search_box_record(open: &TradeOpen) -> Option<&'static str> {
    match open {
        TradeOpen::Vendor { .. } => Some(SEARCH_BOX),
        TradeOpen::Stash { .. } => Some("records/ui/caravan/caravan_searchbox.dbr"),
        TradeOpen::Crafter { .. } => {
            Some("records/ui/inventor/craftingpanel/crafting_searchbox.dbr")
        }
        _ => None,
    }
}

impl Trade {
    /// The open merchant as `gd_game::trade` requests name it (None: no
    /// merchant window).
    pub fn shop(
        &self,
        loaded: Option<&crate::session::LoadedCharacter>,
    ) -> Option<gd_game::trade::Shop> {
        let Some(TradeOpen::Vendor { key, market, .. }) = &self.open else {
            return None;
        };
        Some(gd_game::trade::Shop {
            key: key.clone(),
            market: market.clone(),
            difficulty: loaded.map_or(0, |c| c.0.info.difficulty()),
        })
    }

    /// Empties the item slot and the dismantle result boxes (window closed, tab switched: each box's
    /// show(false), sub_1401ab520 / sub_1401aa740 for the dismantle tab's three boxes).
    pub fn take_boxes(&mut self) -> Vec<Item> {
        let [a, b] = std::mem::take(&mut self.results);
        self.slot.take().into_iter().chain(a).chain(b).collect()
    }

    /// An item-slot window is open: the Runekeeper's, or the crafter's
    /// dismantle / reroll tab.
    pub fn slot_panel(&self) -> bool {
        matches!(
            self.open,
            Some(TradeOpen::Ascension { .. } | TradeOpen::Inventor { .. })
        )
    }

    /// The open window's item slot (`gd_game::crafting::SlotBox`): the Runekeeper's, else the open panel's.
    pub fn slot_box(&self) -> gd_game::crafting::SlotBox {
        use gd_game::crafting::SlotBox;
        match (&self.open, self.panel) {
            (Some(TradeOpen::Ascension { .. }), _) => SlotBox::Ascension,
            (_, Panel::Recover) => SlotBox::Recover,
            (_, Panel::Reroll) => SlotBox::Reroll,
            (_, Panel::Convert) => SlotBox::Convert,
            (_, Panel::Craft | Panel::Dismantle) => SlotBox::Dismantle,
        }
    }

    /// Whether the open window's slot takes the item (`gd_game::crafting::slot_accepts`).
    pub fn accepts(&self, item: &Item, data: &crate::data::GameData) -> Result<(), String> {
        let idb = data.item_db();
        gd_game::crafting::slot_accepts(&idb, self.slot_box(), item)
    }
}

/// Screen rects of the open trade window (physical px; `ui/trade.rs`).
#[derive(Resource, Default, Clone)]
pub struct TradeLayout {
    pub window: Option<URect>,
    pub grid: Option<(URect, f32)>,
    pub tabs: Vec<(usize, URect)>,
    /// The personal / transfer stash toggle.
    pub toggle: Option<URect>,
    /// Crafting: recipe rows (`CraftList` index), the craft button, the
    /// product / reagent boxes (for tooltips) and the list area (scrolling).
    pub rows: Vec<(usize, URect)>,
    pub craft: Option<URect>,
    pub boxes: Vec<(Item, URect)>,
    pub list: Option<URect>,
    /// The recipe list's scroll bar (its arrows and track take clicks, `crafting::craft_clicks`).
    pub list_bar: Option<crate::ui::window::Bar>,
    /// The transfer view's Materials / Components tabs and the shown
    /// reagent boxes (record, rect).
    pub reagent_tabs: Vec<(usize, URect)>,
    pub reagent_boxes: Vec<(String, URect)>,
    /// The stash's buy-a-page tab and sort button.
    pub buy: Option<URect>,
    pub sort: Option<URect>,
    /// The item-slot panels: the slot and the buttons (enabled ones only).
    pub slot: Option<URect>,
    /// The dismantle tab's result boxes (index into [`Trade::results`]).
    pub results: Vec<(usize, URect)>,
    pub buttons: Vec<(Button, URect)>,
    /// The Illusionist's weapon-swap button (its rollover: `ui/rollover.rs`).
    pub swap_button: Option<URect>,
    /// The dismantle warning's / [`Trade::prompt`]'s Yes / No box while it shows.
    pub confirm: Option<crate::ui::confirm::ConfirmRects>,
    /// [`Trade::prompt`]'s Ok box: its Ok and close buttons.
    pub ok: Option<(URect, Option<URect>)>,
    /// The open window's search box and (while it has text) clear button.
    pub search: Option<URect>,
    pub clear_search: Option<URect>,
}

pub use gd_game::stash::page_cost;

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
    fonts: Res<crate::ui::text::FontCache>,
    // Option: the stash exists once `init_stash` ran; without it the crafter knows no learned blueprints.
    stash: Option<Res<Stash>>,
    mut trade: ResMut<Trade>,
    markets: Res<Markets>,
    mut panels: ResMut<crate::ui::UiPanels>,
    data: Res<crate::data::GameData>,
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
                let list = crate::crafting::CraftList::build(&data, rec, &learned, &|t: &str| {
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
            // `sub_140275370` (window show): starts from the tab shown last time.
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
