//! The blacksmith (`NpcCrafter`): its crafting window lists blueprints
//! (`gd_sim::items::crafting`), sorted into the window's five category tabs
//! (relics, melee, ranged, armour, other; by the made item's
//! type, [`GROUPS`]). Reagents are counted and taken from the bags, the material storage
//! (`reagents.gst`), the personal stash and the transfer stash, in that order.
//! Crafting costs the blueprint's creation cost and puts the items in the bags.
//! The crafting panel's search box (`trade.rs` [`crate::trade::TradeSearch`])
//! filters the list: a recipe shows when any of its search lines contains the
//! query, and a group whose recipes all filtered out hides its header
//! (row filter `sub_140199B20`, query cache `sub_14019B7F0`). The
//! lines are the product item's `GenerateSearchText`, format-stripped and
//! lowercased (`sub_140197DC0`: Item vtable +1112 with the player,
//! `LocalizerFormatStrip` + `towlower_l` per line; port
//! [`CraftList::fill_search_lines`], `gd_sim::items::market::search_line`).
//! The Inventor's tabs: dismantling ([`dismantle`]), salvage ([`salvage`]),
//! set conversion ([`convert`]); reroll in `reroll.rs`.
use crate::inventory::Inventory;
use crate::trade::Stash;
use bevy::prelude::*;
pub use gd_game::crafting::{
    can_craft, convert_unlocked, dismantle_cost, dismantle_unlocked, max_craftable, salvage_cost,
};
pub use gd_game::stash::count;
use gd_sim::items::crafting::Blueprint;

pub const WINDOW: &str = "records/ui/inventor/inventor_mastertable.dbr";

pub struct Recipe {
    pub bp: Blueprint,
    /// Name tag (the blueprint's `description`).
    pub name_tag: String,
    /// The made item's name tag (`itemNameTag` / `description`), shown in the
    /// list.
    pub item_tag: String,
    /// Whether it makes a relic (`ItemArtifact`; cyan in the list).
    pub artifact: bool,
    pub tab: usize,
    /// [`GROUPS`] index.
    pub group: usize,
    /// The product item's search lines ([`CraftList::fill_search_lines`]).
    pub search_lines: Vec<String>,
}

/// The list's groups: (tab, header tag), in the window's order.
/// (exe 0x140198670 builds the table): each group names the item-type
/// ranks it takes (Item vtable +1360, `gd_sim::items::inventory::sort_rank`) and, for weapon
/// groups, the weapon types (vtable +1712); relic groups by tier; see
/// [`group_of`].
pub const GROUPS: [(usize, &str); 26] = [
    (0, "tagCraftingRelicsTier01"),
    (0, "tagCraftingRelicsTier02"),
    (0, "tagCraftingRelicsTier03"),
    (0, "tagCraftingRunes"),
    (1, "tagCraftingAxes"),
    (1, "tagCraftingBlunt"),
    (1, "tagCraftingSwords"),
    (1, "tagCraftingImplements"),
    (1, "tagCraftingTwoHandedMelee"),
    (1, "tagCraftingSpears"),
    (2, "tagCraftingOneHanded"),
    (2, "tagCraftingTwoHanded"),
    (2, "tagCraftingStaves"),
    (3, "tagCraftingBelts"),
    (3, "tagCraftingHelms"),
    (3, "tagCraftingTorso"),
    (3, "tagCraftingShoulders"),
    (3, "tagCraftingLegs"),
    (3, "tagCraftingHands"),
    (3, "tagCraftingFeet"),
    (3, "tagCraftingOffhands"),
    (3, "tagCraftingShields"),
    (4, "tagCraftingComponents"),
    (4, "tagCraftingAccessories"),
    (4, "tagCraftingConsumables"),
    (4, "tagCraftingMaterials"),
];

/// A list row: a group header or a recipe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    Header(usize),
    Recipe(usize),
}

/// The open crafter's recipes.
#[derive(Resource, Default)]
pub struct CraftList {
    pub recipes: Vec<Recipe>,
    pub enhancement: Option<String>,
    /// The crafter's name and profession tags.
    pub name_tag: String,
    pub title_tag: String,
    /// `crafterBitmap` (shown in the craft button's circle).
    pub bitmap: Option<String>,
    /// Whether [`Recipe::search_lines`] are filled ([`CraftList::fill_search_lines`]).
    pub search_lines_ready: bool,
}

impl CraftList {
    /// `localize`: tag → text (the list is sorted by the shown names).
    pub fn build(
        data: &crate::data::GameData,
        crafter: &gd_formats::arz::Record,
        learned: &[&str],
        localize: &dyn Fn(&str) -> String,
    ) -> Self {
        let window = data
            .db
            .get(WINDOW)
            .and_then(|w| w.str("enchanterArtifactTab").and_then(|t| data.db.get(t)));
        let defaults = window
            .as_ref()
            .map(|w| w.strs("craftingDefaultRecipes"))
            .unwrap_or_default();
        // CraftList::build (exe 0x14019a1d0): own recipes, awakened recipes (gameascendant.dbr
        // `awakenedRecipes`) when the crafter has `awakenedRecipes`, learned blueprints, window defaults.
        let awakened_rec = data.db.get(gd_sim::ascendant::GAME_ASCENDANT);
        let awakened = awakened_rec
            .as_ref()
            .map(|a| a.strs("awakenedRecipes"))
            .unwrap_or_default();
        let mut recipes = Vec::new();
        for r in
            gd_sim::items::crafting::recipes_with_awakened(crafter, &awakened, &defaults, learned)
        {
            // exe sub_14019da70 / sub_14019c5a0: an invalid formula is not listed.
            let Some(rec) = data.db.get(&r) else { continue };
            if !gd_sim::items::crafting::formula_valid(&*data.db, rec) {
                continue;
            }
            let Some(bp) = Blueprint::load(&r, rec) else {
                continue;
            };
            let group = group_of(data, &bp.display);
            let shown = data.db.get(&bp.display);
            let item_tag = shown
                .as_ref()
                .and_then(|d| {
                    d.str("itemNameTag")
                        .or_else(|| d.str("description"))
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let artifact = shown.as_ref().is_some_and(|d| d.class == "ItemArtifact");
            recipes.push(Recipe {
                name_tag: rec.str("description").unwrap_or("").to_string(),
                item_tag,
                artifact,
                bp,
                tab: GROUPS[group].0,
                group,
                search_lines: Vec::new(),
            });
        }
        let tag = |k: &str| crafter.str(k).unwrap_or("").to_string();
        // Within a group: by shown name (as the real list).
        recipes.sort_by_cached_key(|r| {
            (
                r.group,
                localize(if r.item_tag.is_empty() {
                    &r.name_tag
                } else {
                    &r.item_tag
                }),
            )
        });
        Self {
            recipes,
            enhancement: crafter
                .str("enhancementTable")
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            name_tag: tag("description"),
            title_tag: tag("professionTag"),
            bitmap: crafter.str("crafterBitmap").map(str::to_string),
            search_lines_ready: false,
        }
    }

    /// The list rows of `tab`: each non-empty group's header, then its recipes.
    /// With a search query, only the recipes whose search lines match
    /// ([`gd_sim::items::market::search_matches`]) show, and a group left empty
    /// hides its header (`sub_140199B20`).
    pub fn rows(&self, tab: usize, query: &str) -> Vec<Row> {
        let mut out = Vec::new();
        for (g, (t, _)) in GROUPS.iter().enumerate() {
            if *t != tab {
                continue;
            }
            let header = out.len();
            out.extend(
                (0..self.recipes.len())
                    .filter(|i| self.shows(*i, g, query))
                    .map(Row::Recipe),
            );
            if out.len() > header {
                out.insert(header, Row::Header(g));
            }
        }
        out
    }

    /// `rows(tab, query).len()` without building the rows.
    pub fn row_count(&self, tab: usize, query: &str) -> usize {
        GROUPS
            .iter()
            .enumerate()
            .filter(|(_, (t, _))| *t == tab)
            .map(|(g, _)| {
                let n = (0..self.recipes.len())
                    .filter(|i| self.shows(*i, g, query))
                    .count();
                if n == 0 { 0 } else { n + 1 }
            })
            .sum()
    }

    fn shows(&self, i: usize, group: usize, query: &str) -> bool {
        let r = &self.recipes[i];
        r.group == group
            && (query.is_empty() || gd_sim::items::market::search_matches(&r.search_lines, query))
    }

    /// Fills every recipe's search lines: the product item's search text
    /// (`gd_sim::items::tooltip_item::item_search_text`) as search lines
    /// (module doc). Called once per list (`trade.rs` `search_hits`).
    pub fn fill_search_lines(&mut self, ctx: &gd_sim::items::tooltip_item::TipCtx) {
        for r in &mut self.recipes {
            let item = gd_formats::save::item::Item {
                base_name: r.bp.display.clone(),
                stack_count: r.bp.quantity,
                ..Default::default()
            };
            r.search_lines = gd_sim::items::tooltip_item::item_search_text(ctx, &item)
                .iter()
                .map(|l| gd_sim::items::market::search_line(&l.text))
                .collect();
        }
        self.search_lines_ready = true;
    }
}

/// The [`GROUPS`] entry of what a blueprint makes (exe 0x140198670):
/// relics (rank 11) by tier (`artifactClassification` Lesser /
/// Greater / Divine), runes = augments (21); weapons (rank 9) by weapon
/// type: axe, mace, sword, dagger + scepter, the four two-handed melee
/// classes (spear included), `WeaponHunting_Spear`, one- and two-handed
/// ranged, staff; armour by rank: waist, head, chest (+ clothing,
/// vestments), shoulders, legs, hands (+ bracelets, rank 7), feet;
/// off-hands and shields (rank 8, weapon type 12 / 11); components (16:
/// relics, charms), accessories (amulet 2, ring 6, medal 15), consumables
/// (`OneShot*` 17, attribute 26 / devotion 24 resets), materials
/// (`QuestItem`, 19). A class in no group (none among the blueprints'
/// items) is listed under Materials here.
fn group_of(data: &crate::data::GameData, display: &str) -> usize {
    let rec = data.db.get(display);
    let class = rec.as_ref().map(|r| r.class.clone()).unwrap_or_default();
    let group = |tag: &str| {
        GROUPS
            .iter()
            .position(|(_, t)| *t == tag)
            .unwrap_or(GROUPS.len() - 1)
    };
    match class.as_str() {
        "WeaponMelee_Axe" => return group("tagCraftingAxes"),
        "WeaponMelee_Mace" => return group("tagCraftingBlunt"),
        "WeaponMelee_Sword" => return group("tagCraftingSwords"),
        "WeaponMelee_Dagger" | "WeaponMelee_Scepter" => return group("tagCraftingImplements"),
        "WeaponMelee_Axe2h"
        | "WeaponMelee_Mace2h"
        | "WeaponMelee_Sword2h"
        | "WeaponMelee_Spear2h" => return group("tagCraftingTwoHandedMelee"),
        "WeaponHunting_Spear" => return group("tagCraftingSpears"),
        "WeaponHunting_Ranged1h" => return group("tagCraftingOneHanded"),
        "WeaponHunting_Ranged2h" => return group("tagCraftingTwoHanded"),
        "WeaponMagical_Staff" => return group("tagCraftingStaves"),
        "WeaponArmor_Offhand" => return group("tagCraftingOffhands"),
        "WeaponArmor_Shield" => return group("tagCraftingShields"),
        _ => {}
    }
    match gd_sim::items::inventory::sort_rank(&class) {
        11 => match rec
            .as_ref()
            .and_then(|r| r.str("artifactClassification").map(str::to_owned))
            .as_deref()
        {
            Some("Greater") => group("tagCraftingRelicsTier02"),
            Some("Divine") => group("tagCraftingRelicsTier03"),
            _ => group("tagCraftingRelicsTier01"),
        },
        21 => group("tagCraftingRunes"),
        13 => group("tagCraftingBelts"),
        1 => group("tagCraftingHelms"),
        3 => group("tagCraftingTorso"),
        14 => group("tagCraftingShoulders"),
        4 => group("tagCraftingLegs"),
        7 => group("tagCraftingHands"),
        5 => group("tagCraftingFeet"),
        16 => group("tagCraftingComponents"),
        2 | 6 | 15 => group("tagCraftingAccessories"),
        17 | 26 | 24 => group("tagCraftingConsumables"),
        _ => group("tagCraftingMaterials"),
    }
}

/// Clicks in the crafting window: category tabs, recipe rows, the craft
/// button; the wheel scrolls the list. `GD_CRAFT` (hooks.rs): once the window is open, select the first
/// recipe whose record contains the text and craft.
#[allow(clippy::too_many_arguments)]
pub fn craft_clicks(
    hooks: Res<gd_game::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    layout: Res<crate::trade::TradeLayout>,
    mut trade: ResMut<crate::trade::Trade>,
    list: Option<Res<CraftList>>,
    stash: Option<Res<Stash>>,
    inv: Option<Res<Inventory>>,
    progress: Option<Res<crate::loot::PlayerProgress>>,
    mut capture: ResMut<crate::inventory::UiCapture>,
    mut requests: MessageWriter<gd_game::crafting::CraftRequest>,
    (mut tested, mut rows, mut last_query): (
        Local<bool>,
        Local<Option<(u64, bool, usize)>>,
        Local<String>,
    ),
    (pause, confirm, stack): (
        Res<gd_game::pause::GamePause>,
        Res<crate::ui::apply::InvConfirm>,
        Res<crate::ui::stack::StackWindow>,
    ),
) {
    use crate::trade::contains;
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
        .and_then(|w| crate::ui::cursor_px(&hooks, w));
    trade.hover = None;
    // The game menu, a modal question (`inventory::question_up`) or the stack window takes every click and
    // the wheel: none reaches the window under it (as `trade_clicks`).
    if pause.menu
        || crate::inventory::question_up(&inv, &trade, &confirm)
        || p.is_some_and(|p| stack.covers(p))
    {
        return;
    }
    // The search box (trade.rs module doc): a click on it clears a non-empty
    // text and focuses it; the clear button empties it; any other click
    // unfocuses it. Only the crafting tab shows the box.
    if let Some(p) = p
        && buttons.just_pressed(MouseButton::Left)
        && trade.panel == crate::trade::Panel::Craft
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
        trade.version += 1;
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
            let s = trade.scroll as i32 - scroll.delta.y.signum() as i32 * 2;
            trade.scroll = s.clamp(0, max_scroll as i32) as usize;
            trade.version += 1;
        }
        if buttons.just_pressed(MouseButton::Left) {
            // The list's scroll bar: an arrow steps a row, the track pages (`Bar::press` in pixels, rounded to
            // rows). PORT: a held arrow or thumb does not repeat or drag (the list rebuilds per step).
            // A row is the list item's `LineHeight` (23 units) at the UI scale, so `line / 23` is that scale.
            let line = layout.rows.first().map_or(0.0, |(_, r)| r.h);
            let pressed = layout.list_bar.filter(|_| line > 0.0).and_then(|b| {
                b.press(
                    p,
                    line / 23.0,
                    trade.scroll as f32 * line,
                    max_scroll as f32 * line,
                )
            });
            if let Some((px, _)) = pressed {
                let s = ((px / line).round() as usize).min(max_scroll);
                if s != trade.scroll {
                    trade.scroll = s;
                    trade.version += 1;
                }
            } else if let Some((t, _)) = layout.tabs.iter().find(|(_, r)| contains(r, p)) {
                trade.tab = *t;
                trade.recipe = None;
                trade.scroll = 0;
                trade.version += 1;
            } else if let Some((i, _)) = layout.rows.iter().find(|(_, r)| contains(r, p)) {
                trade.recipe = Some(*i);
                trade.version += 1;
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

/// The dismantle tab's action (`trade.go`, set by `crate::trade` `panel_clicks` after its warning prompt): sends
/// the slot item to `gd_game::crafting` (rules, cost and notes there; window UI: `ui/trade.rs`
/// `dismantle_panel`; results: [`dismantle_results`]).
pub fn dismantle(
    hooks: Res<gd_game::hooks::Hooks>,
    mut trade: ResMut<crate::trade::Trade>,
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
        trade.panel = crate::trade::Panel::Dismantle;
        stage.write(gd_game::crafting::StageItem {
            which: gd_game::crafting::Stage::Contains(String::new()),
            go: true,
        });
    }
    if !trade.go
        || !matches!(trade.open, Some(crate::trade::TradeOpen::Inventor { .. }))
        || trade.panel != crate::trade::Panel::Dismantle
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
/// (`ControllerPlayer::GiveDismantledItemToPlayer` @0x18015f600: the UI call returns false ->
/// `GiveItemToPlayer`, `InvRequest::Stow`). Both boxes play the tab's sound (`+4432`; PORT: taken as its
/// `dismantleItemSound`).
pub fn dismantle_results(
    mut msgs: MessageReader<gd_game::crafting::DismantleResult>,
    mut trade: ResMut<crate::trade::Trade>,
    data: Res<crate::data::GameData>,
    mut requests: MessageWriter<crate::inventory::InvRequest>,
    mut sound: MessageWriter<gd_game::visuals::PlaySound>,
) {
    for gd_game::crafting::DismantleResult(d) in msgs.read() {
        let shown = matches!(trade.open, Some(crate::trade::TradeOpen::Inventor { .. }))
            && trade.panel == crate::trade::Panel::Dismantle;
        if !shown {
            for it in d.scrap.iter().chain(&d.bonus) {
                requests.write(crate::inventory::InvRequest::Stow(it.clone()));
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
                sound.write(gd_game::visuals::PlaySound {
                    pak: pak.clone(),
                    at: None,
                });
            }
        }
        for it in gd_game::crafting::receive_dismantled(&mut trade.results, d.clone()) {
            requests.write(crate::inventory::InvRequest::Stow(it));
        }
        trade.version += 1;
    }
}

/// Puts what the Inventor's requests leave back into the window's slot (a
/// refused item, what did not fit the bags, a staged hook item), presses the
/// button of a staged item and rebuilds the window.
pub fn slot_returns(
    mut msgs: MessageReader<gd_game::crafting::SlotReturn>,
    mut trade: ResMut<crate::trade::Trade>,
) {
    for m in msgs.read() {
        if let Some(item) = &m.item {
            trade.slot = Some(item.clone());
        }
        trade.go |= m.go;
        trade.version += 1;
    }
}

/// The salvage tab (`dividing_table.dbr`, window `sub_14026CF20`, panel
/// loader `sub_1401B2870`, update `sub_1401B20C0`).
/// The slot takes an equipment item; the cost is `salvage_cost`; the buttons
/// are all off when the item is missing or the money short (`sub_1401B20C0`:
/// `CurrentMoney < cost`); otherwise Keep Item and Keep Add-on need a
/// component (`HasRelic`) and Remove Augment an augment (`HasEnchantment`).
/// `EnchanterRecoveryConfigCmd::Execute`: Keep Add-on takes the component out
/// as an item of its own (given to the player) and destroys the item;
/// Remove Augment destroys only the augment (`RemoveEnchantment`); Keep Item
/// destroys the component (`RemoveRelic`) and the augment and gives the item
/// back. The cost is taken from the player's money in each case.
pub fn salvage(
    hooks: Res<gd_game::hooks::Hooks>,
    mut trade: ResMut<crate::trade::Trade>,
    mut requests: MessageWriter<gd_game::crafting::SalvageRequest>,
    mut stage: MessageWriter<gd_game::crafting::StageItem>,
    mut tested: Local<bool>,
) {
    use crate::trade::Button;
    if !matches!(trade.open, Some(crate::trade::TradeOpen::Inventor { .. }))
        || trade.panel != crate::trade::Panel::Recover
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
/// GrimDawn.exe `sub_1401A7890` button handler): the create button
/// converts the slot item at once (no prompt; a `blacklistedSets` item gets
/// the refusal notice instead). The new item goes to the bags; the slot
/// keeps it when they are full. `GD_CONVERT` (hooks.rs): once the Inventor is open, puts the first bag
/// set item into the slot and presses the button (money and reagents provided).
pub fn convert(
    hooks: Res<gd_game::hooks::Hooks>,
    mut trade: ResMut<crate::trade::Trade>,
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
        trade.panel = crate::trade::Panel::Convert;
        trade.random_set = k == "random";
        // The money and reagents are provided with the item.
        stage.write(gd_game::crafting::StageItem {
            which: gd_game::crafting::Stage::Convertible {
                random: trade.random_set,
            },
            go: true,
        });
    }
    if trade.panel != crate::trade::Panel::Convert || !trade.go {
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
