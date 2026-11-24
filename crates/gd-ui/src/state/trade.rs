//! The NPC windows' UI state: the open merchant / stash / crafter ([`Trade`]), its screen layout
//! ([`TradeLayout`]), the search box, the dialog prompts and the item-slot panels. The binary's
//! `trade` / `crafting` / `illusion` input systems read and change it; `gd_ui::trade` draws it.
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_formats::ui::Rect as URect;
use gd_game::trade::{FactionView, TradeOpen};

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
    Kind(gd_game::reroll::Kind),
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
    pub kind: Option<gd_game::reroll::Kind>,
    /// The dismantle warning prompt is showing (`ui/confirm.rs`'s box) and what it shows of the mouse (the
    /// look of [`Trade::prompt`]'s box too).
    pub confirm: bool,
    pub confirm_look: gd_ui_kit::confirm::Look,
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
    pub illusion: crate::state::illusion::IllusionView,
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
    pub searched: Option<(String, String)>,
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
/// `tagTransferPurchase01` with the cost (`gd_game_core::stash::StashRequest::BuyPage`, sent on Yes); `Message`: an
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
        loaded: Option<&gd_game_core::session::LoadedCharacter>,
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
    /// show(false), `DismantleItemBox::Show` / `ResultItemBox::Show` for the dismantle tab's three boxes).
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
    pub fn accepts(&self, item: &Item, data: &gd_game_core::data::GameData) -> Result<(), String> {
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
    pub list_bar: Option<crate::window::Bar>,
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
    pub confirm: Option<gd_ui_kit::confirm::ConfirmRects>,
    /// [`Trade::prompt`]'s Ok box: its Ok and close buttons.
    pub ok: Option<(URect, Option<URect>)>,
    /// The open window's search box and (while it has text) clear button.
    pub search: Option<URect>,
    pub clear_search: Option<URect>,
}
