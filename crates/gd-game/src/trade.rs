//! Merchants as gameplay: their stock and buyback per market, prices, and buying / selling. The game shows
//! the windows and picks what to trade.
//!
//! Service NPCs ([`service_of`]): a click is the player's interact order (`player_orders::InteractTarget`);
//! in reach (`conversation::in_talk_range`: player radius + NPC radius + 3 m, or the walk's end) the
//! service opens ([`ServiceAnswer::Opened`]). A merchant's market is its `marketFileName` for the
//! difficulty, stocked on opening ([`stock_market`]) at the merchant's resolved `charLevel`
//! ([`crate::monsters::placed_char_level`]). Faction merchants (`factionmarket.tpl`): one tab of fixed items
//! per reputation tier, sold by rank ([`FactionView`]; a locked tier is browsed, its purchases refused). The
//! service closes with the window ([`ServiceRequest::Close`]) or when the player is more than 6 m away
//! ([`ServiceAnswer::Closed`]); closing a market starts its refresh countdown ([`close_market`]).
//!
//! Voices (the NPC record's `dialogPak`, [`DialogPak`]): opening a service makes its NPC say hello
//! (`Npc::SayHello`), every service window's hide says goodbye (`Npc::SayGoodbye`), and `Npc::Converse`
//! greets when the NPC has no speech to give. Both `Say*` do nothing for an NPC with a `conversation`. These
//! send [`NpcSay`]; [`say_lines`] picks the line ([`DialogPak::pick`]) and sends [`NpcVoice`], played in 3D at
//! the NPC. `DialogPak::Say`: nothing is said while the previous line still plays (PORT: the game's audio
//! drops the new line, after the pick, so a dropped line still restarts the `Chat` animation and the
//! long-greeting countdown). `DialogPak.cooldown` never blocks. After the line the NPC plays the pak's
//! `animation` slot.
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_formats::inventory::{Footprint, Sack};
use gd_formats::save::item::Item;
use gd_game_core::inventory::Inventory;
use gd_sim::items::market::{
    BUYBACK_TAB, VENDOR_TABS, prices, refresh_minutes, stock_price, vendor_tab,
};

/// Each merchant's stock this session, by merchant key: one page per tab; and the buyback price of each item
/// on the buyback tab, by grid cell. `MarketManager::Sell` stores the gold paid out with the item
/// (`Market::StoreBuybackItem`) and `Market::GetBuyPrice` returns it as is, without discount or markup.
#[derive(Resource, Default)]
pub struct Markets(
    pub HashMap<String, Vec<Sack<Item>>>,
    pub HashMap<String, HashMap<(u32, u32), u64>>,
);

/// When each merchant's stock is rolled again (app seconds): `marketRefreshTimeMin..Max` minutes.
/// `ServerMarket::Update` re-rolls the countdown while the merchant chats, so it runs from when the window
/// closes; a refresh also empties the buyback tab (`ClientMarket::ClearBuyback`). PORT: the new stock is
/// rolled when the window next opens.
#[derive(Resource, Default)]
pub struct MarketTimers(pub HashMap<String, f64>);

pub const VENDOR_W: u32 = 10;
pub const VENDOR_H: u32 = 18;

/// `faction_vendor_inventorygridconfiguration` (480 px).
pub const FACTION_VENDOR_H: u32 = 15;

/// Stocks merchant `key` (market record `mrec`) on opening at app time `now` (s). Faction markets
/// (`friendlyNormalTable`): one tab per reputation tier ([`gd_sim::items::market::TIER_CATEGORIES`]),
/// stocked once. Others: rolled ([`gd_sim::items::market::roll_market`]) when there is no stock or the
/// refresh time has passed; a refresh empties the buyback tab and restarts the countdown.
#[allow(clippy::too_many_arguments)]
pub fn stock_market(
    markets: &mut Markets,
    timers: &mut MarketTimers,
    now: f64,
    key: &str,
    mrec: &gd_formats::arz::Record,
    data: &gd_game_core::data::GameData,
    inv: &mut Inventory,
    params: &gd_sim::items::LootParams,
) {
    if mrec.str("friendlyNormalTable").is_some() {
        if markets.0.contains_key(key) {
            return;
        }
        let mut pages: Vec<Sack<Item>> = (0..VENDOR_TABS)
            .map(|_| Sack::new(VENDOR_W, FACTION_VENDOR_H, false))
            .collect();
        let idb = data.item_db();
        let mut rng = gd_rules::Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
        use gd_sim::items::market::{FACTION_STATIC, TIER_CATEGORIES, roll_categories};
        let mut stock: Vec<(usize, Vec<gd_sim::items::LootRoll>)> = gd_rules::faction::TIERS
            .iter()
            .enumerate()
            .filter_map(|(t, (field, _))| {
                let tier = mrec.str(field).and_then(|f| data.db.get(f))?;
                Some((
                    t,
                    roll_categories(&idb, tier, &TIER_CATEGORIES, params, &mut rng),
                ))
            })
            .collect();
        stock.push((
            0,
            roll_categories(&idb, mrec, &[FACTION_STATIC], params, &mut rng),
        ));
        for (t, rolls) in stock {
            for r in rolls {
                let item = r.to_item();
                let Some(info) = inv.info(&item) else {
                    continue;
                };
                pages[t].add(item, info.size);
            }
        }
        markets.0.insert(key.to_owned(), pages);
        return;
    }
    if markets.0.contains_key(key) && timers.0.get(key).is_none_or(|t| now < *t) {
        return;
    }
    timers.0.insert(
        key.to_owned(),
        now + refresh_minutes(mrec, gd_game_core::rng::f32()) as f64 * 60.0,
    );
    markets.0.remove(key);
    markets.1.remove(key);
    let idb = data.item_db();
    let mut rng = gd_rules::Rng(gd_game_core::rng::u32(1..0x7FFF_FFFF));
    let mut pages: Vec<Sack<Item>> = (0..VENDOR_TABS)
        .map(|_| Sack::new(VENDOR_W, VENDOR_H, false))
        .collect();
    let stock = gd_sim::items::market::roll_market(&idb, mrec, params, &mut rng);
    let n = stock.len();
    for r in stock {
        let item = r.to_item();
        let Some(info) = inv.info(&item) else {
            continue;
        };
        let tab = vendor_tab(&info.resolved.class);
        pages[tab].add(item, info.size);
    }
    let kept: usize = pages.iter().map(|p| p.items.len()).sum();
    info!(
        "trade: {key} stocks {kept} of {n} items (level {})",
        params.parent_level
    );
    markets.0.insert(key.to_owned(), pages);
}

/// The chat with merchant `key` ended at `now`: its refresh countdown starts over. Faction markets do not
/// refresh.
pub fn close_market(
    timers: &mut MarketTimers,
    now: f64,
    key: &str,
    mrec: &gd_formats::arz::Record,
) {
    if mrec.str("friendlyNormalTable").is_none() {
        timers.0.insert(
            key.to_owned(),
            now + refresh_minutes(mrec, gd_game_core::rng::f32()) as f64 * 60.0,
        );
    }
}

/// Sells `item` to the merchant `key`: pays its sell price and puts it on the buyback tab at that price.
/// Returns the price.
#[allow(clippy::too_many_arguments)]
pub fn sell_item(
    markets: &mut Markets,
    idb: &gd_sim::items::ItemDb,
    key: &str,
    market: &str,
    item: Item,
    difficulty: usize,
    discount: i32,
    gold: &mut u64,
    inv: &mut Inventory,
) -> u64 {
    let (_, sell) = prices(idb, market, &item, difficulty, discount).unwrap_or((0, 0));
    crate::loot::add_gold(gold, sell as i64);
    let size = inv
        .info(&item)
        .map(|i| i.size)
        .unwrap_or(Footprint { w: 1, h: 1 });
    if let Some(cell) = markets
        .0
        .get_mut(key)
        .and_then(|m| m.get_mut(BUYBACK_TAB))
        .and_then(|m| m.add(item.clone(), size))
    {
        markets
            .1
            .entry(key.to_owned())
            .or_default()
            .insert(cell, sell);
    }
    info!(
        "trade: sold {} for {sell}",
        item.base_name.rsplit('/').next().unwrap_or("")
    );
    sell
}

pub const NO_ROOM: &str = "no room in the bags";

/// Buys the item at `cell` of tab `tab` into the bags. `endless`: a faction tier tab, whose stock never runs
/// out (`MarketManager::PurchaseTierItem` hands out a copy). `Ok`: the price paid.
#[allow(clippy::too_many_arguments)]
pub fn buy_item(
    markets: &mut Markets,
    idb: &gd_sim::items::ItemDb,
    key: &str,
    market: &str,
    tab: usize,
    cell: (u32, u32),
    endless: bool,
    difficulty: usize,
    discount: i32,
    gold: &mut u64,
    inv: &mut Inventory,
) -> Result<u64, &'static str> {
    let (item, buy) = stock_price(
        markets.0.get(key).map_or(&[][..], Vec::as_slice),
        markets.1.get(key),
        idb,
        market,
        tab,
        cell,
        difficulty,
        discount,
    )
    .ok_or("nothing there")?;
    if *gold < buy {
        info!(
            "trade: cannot afford {} ({buy})",
            item.base_name.rsplit('/').next().unwrap_or("")
        );
        return Err("cannot afford");
    }
    if !inv.add_to_bags(item.clone()) {
        info!("trade: no room in the bags");
        return Err(NO_ROOM);
    }
    *gold -= buy;
    if !endless || tab == BUYBACK_TAB {
        let Markets(stock, buyback) = markets;
        if let Some(m) = stock
            .get_mut(key)
            .and_then(|m| m.get_mut(tab.min(VENDOR_TABS - 1)))
        {
            let p = m.item_at(cell.0, cell.1).map(|q| (q.x, q.y));
            if let Some((x, y)) = p {
                m.items.retain(|q| !(q.x == x && q.y == y));
                if tab == BUYBACK_TAB
                    && let Some(b) = buyback.get_mut(key)
                {
                    b.remove(&(x, y));
                }
            }
        }
    }
    info!(
        "trade: bought {} for {buy}",
        item.base_name.rsplit('/').next().unwrap_or("")
    );
    Ok(buy)
}

/// The open merchant a request is for. The faction discount is the open service's
/// ([`ServiceVisit::discount`]).
#[derive(Clone, Debug)]
pub struct Shop {
    pub key: String,
    pub market: String,
    pub difficulty: usize,
}

/// Buys the item at `cell` of tab `tab` ([`buy_item`]). A faction tier the player's rank has not unlocked is
/// refused with `tagMarketError05` ([`TradeRefused`]; `MarketFaction::Purchase` → `Market::CanPurchase`),
/// before the price and bag-room checks.
#[derive(Message, Clone, Debug)]
pub struct BuyRequest {
    pub shop: Shop,
    pub tab: usize,
    pub cell: (u32, u32),
}

/// A refused trade's Ok box: its text tag and the `{%d0}` argument.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeRefused {
    pub tag: &'static str,
    pub arg: Option<u64>,
}

#[derive(Message, Clone, Debug)]
pub struct SellRequest {
    pub shop: Shop,
    pub bag: usize,
    pub cell: (u32, u32),
}

/// The held item clicked on the merchant window: sold whole (`CursorHandlerItemMove::PrimaryMarketActivate`).
#[derive(Message, Clone, Debug)]
pub struct SellHeld {
    pub shop: Shop,
}

/// Sells the item worn in equipment slot `slot` (index into `gd_game_core::inventory::SLOTS`): a right press
/// while a merchant is open with `quickBuy` (`EquipSlotWidget::HandleMouseEvent`).
#[derive(Message, Clone, Debug)]
pub struct SellEquipped {
    pub shop: Shop,
    pub slot: usize,
}

/// A request changed the merchant's or the stash's state; `page`: the stash page to show.
#[derive(Message, Clone, Copy, Debug, Default)]
pub struct TradeDone {
    pub page: Option<usize>,
}

#[allow(clippy::too_many_arguments)]
fn apply_trade(
    mut buys: MessageReader<BuyRequest>,
    mut sells: MessageReader<SellRequest>,
    mut held: MessageReader<SellHeld>,
    mut worn: MessageReader<SellEquipped>,
    mut done: MessageWriter<TradeDone>,
    (mut vox, mut refused): (
        MessageWriter<crate::error_vox::ErrorVox>,
        MessageWriter<TradeRefused>,
    ),
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
    visit: Res<ServiceVisit>,
    mut markets: ResMut<Markets>,
    data: Res<gd_game_core::data::GameData>,
    mut inv: ResMut<Inventory>,
    mut progress: ResMut<gd_game_core::progress::PlayerProgress>,
) {
    if buys.is_empty() && sells.is_empty() && held.is_empty() && worn.is_empty() {
        return;
    }
    let idb = &data.item_db();
    let discount = visit.discount();
    let endless = visit.faction.is_some();
    let sale_sound = gd_game_core::stash::market_sale_sound(&data);
    let buy_sound = gd_game_core::stash::market_purchase_sound(&data);
    for r in held.read() {
        let Some(item) = inv.held.take() else {
            continue;
        };
        inv.changed();
        sell_item(
            &mut markets,
            idb,
            &r.shop.key,
            &r.shop.market,
            item,
            r.shop.difficulty,
            discount,
            &mut progress.gold,
            &mut inv,
        );
        if let Some(pak) = sale_sound.clone() {
            sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
        }
        done.write(TradeDone::default());
    }
    for r in worn.read() {
        let Some(item) = inv.equipped.get_mut(r.slot).and_then(Option::take) else {
            continue;
        };
        inv.mark_equipment_dirty();
        inv.changed();
        sell_item(
            &mut markets,
            idb,
            &r.shop.key,
            &r.shop.market,
            item,
            r.shop.difficulty,
            discount,
            &mut progress.gold,
            &mut inv,
        );
        if let Some(pak) = sale_sound.clone() {
            sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
        }
        done.write(TradeDone::default());
    }
    // Sales from the bags first.
    for r in sells.read() {
        let Some(placed) = inv
            .bags
            .get(r.bag)
            .and_then(|b| b.item_at(r.cell.0, r.cell.1))
            .cloned()
        else {
            continue;
        };
        inv.bags[r.bag]
            .items
            .retain(|q| !(q.x == placed.x && q.y == placed.y));
        inv.changed();
        sell_item(
            &mut markets,
            idb,
            &r.shop.key,
            &r.shop.market,
            placed.item,
            r.shop.difficulty,
            discount,
            &mut progress.gold,
            &mut inv,
        );
        if let Some(pak) = sale_sound.clone() {
            sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
        }
        done.write(TradeDone::default());
    }
    for r in buys.read() {
        if visit
            .faction
            .as_ref()
            .is_some_and(|f| r.tab < f.open.len() && !f.open[r.tab])
        {
            info!(
                "trade: tier {} is locked: faction standing too low",
                r.tab + 1
            );
            refused.write(TradeRefused {
                tag: "tagMarketError05",
                arg: None,
            });
            continue;
        }
        let s = &r.shop;
        let bought = buy_item(
            &mut markets,
            idb,
            &s.key,
            &s.market,
            r.tab,
            r.cell,
            endless,
            s.difficulty,
            discount,
            &mut progress.gold,
            &mut inv,
        );
        match bought {
            Ok(_) => {
                if let Some(pak) = buy_sound.clone() {
                    sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
                }
                done.write(TradeDone::default());
            }
            // `MarketFaction::Purchase`: no room after the price check says so.
            Err(NO_ROOM) => {
                vox.write(crate::error_vox::ErrorVox::new(
                    crate::error_vox::VoxKind::InventoryFull,
                    "",
                ));
            }
            Err(_) => {}
        }
    }
}

pub struct TradePlugin;

impl Plugin for TradePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Markets>()
            .init_resource::<MarketTimers>()
            .init_resource::<ServiceVisit>()
            .add_message::<BuyRequest>()
            .add_message::<TradeRefused>()
            .add_message::<SellRequest>()
            .add_message::<SellHeld>()
            .add_message::<SellEquipped>()
            .add_message::<TradeDone>()
            .add_message::<ServiceRequest>()
            .add_message::<ServiceAnswer>()
            .add_message::<NpcSay>()
            .add_message::<NpcVoice>()
            .add_systems(
                Update,
                apply_trade
                    .run_if(resource_exists::<Inventory>)
                    .run_if(resource_exists::<gd_game_core::progress::PlayerProgress>)
                    .run_if(resource_exists::<gd_game_core::data::GameData>)
                    .after(gd_game_core::inventory::InventorySet)
                    .before(gd_game_core::player::PlayerSet),
            )
            .add_systems(
                Update,
                run_services
                    .run_if(services_ready)
                    .after(gd_game_core::inventory::InventorySet)
                    .before(gd_game_core::player::PlayerSet),
            )
            .add_systems(
                Update,
                say_lines
                    .run_if(resource_exists::<gd_game_core::data::GameData>)
                    .after(run_services),
            )
            .add_plugins((
                crate::illusion::IllusionPlugin,
                crate::loot_filter::LootFilterPlugin,
            ));
    }
}

fn services_ready(
    inv: Option<Res<Inventory>>,
    progress: Option<Res<gd_game_core::progress::PlayerProgress>>,
    data: Option<Res<gd_game_core::data::GameData>>,
    mode: Option<Res<gd_game_core::session::GameMode>>,
    altar: Option<Res<gd_game_core::ascendant::AltarState>>,
) -> bool {
    inv.is_some() && progress.is_some() && data.is_some() && mode.is_some() && altar.is_some()
}

pub fn service_of(db: &gd_formats::vfs::Database, record: &str) -> Option<&'static str> {
    match db.get(record)?.class.as_str() {
        "NpcMerchant" => Some("merchant"),
        "NpcCaravan" => Some("caravan"),
        "NpcCrafter" => Some("crafter"),
        "NpcEnchanter" => Some("enchanter"),
        "NpcItemAscension" => Some("ascension"),
        "NpcTransmuter" => Some("illusionist"),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TradeOpen {
    /// Market key ([`gd_sim::items::market::market_key`]) and market record.
    Vendor {
        key: String,
        market: String,
        npc: Entity,
    },
    /// `NpcCaravan`: the personal / transfer stash.
    Stash { npc: Entity },
    /// `NpcCrafter`.
    Crafter { npc: Entity },
    /// `NpcEnchanter`: the salvage / dismantle / convert / reroll tabs (`InventorWindow::Load`).
    Inventor { npc: Entity },
    /// `NpcItemAscension`.
    Ascension { npc: Entity },
    /// `NpcTransmuter`.
    Illusionist { npc: Entity },
}

impl TradeOpen {
    pub fn npc(&self) -> Entity {
        match self {
            TradeOpen::Vendor { npc, .. }
            | TradeOpen::Stash { npc }
            | TradeOpen::Crafter { npc }
            | TradeOpen::Inventor { npc }
            | TradeOpen::Ascension { npc }
            | TradeOpen::Illusionist { npc } => *npc,
        }
    }
}

/// A faction merchant's faction and the player's standing with it.
#[derive(Clone, Debug)]
pub struct FactionView {
    pub name_tag: String,
    pub value: f32,
    pub levels: gd_rules::faction::Levels,
    pub open: [bool; 4],
}

impl FactionView {
    pub fn discount(&self) -> i32 {
        self.levels.discount(self.value)
    }
}

#[derive(Resource, Default)]
pub struct ServiceVisit {
    pub open: Option<TradeOpen>,
    /// The open faction merchant's standing.
    pub faction: Option<FactionView>,
}

impl ServiceVisit {
    /// The faction standing's, 0 for a plain merchant.
    pub fn discount(&self) -> i32 {
        self.faction.as_ref().map_or(0, FactionView::discount)
    }
}

/// The player's requests to a service NPC (opening goes through `player_orders::InteractTarget`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum ServiceRequest {
    /// The window closed.
    Close,
}

#[derive(Message, Clone, Debug)]
pub enum ServiceAnswer {
    /// The window to show, the NPC's record and a faction merchant's standing.
    Opened {
        open: TradeOpen,
        record: String,
        faction: Option<FactionView>,
    },
    /// The player walked away (or the NPC is gone).
    Closed,
}

/// `Actor::GetUICloseDistance` stand-in.
const CLOSE_DISTANCE: f32 = 6.0;

/// For a faction market (`friendlyNormalTable`).
fn faction_view(
    db: &gd_formats::vfs::Database,
    npc: &gd_formats::arz::Record,
    mrec: &gd_formats::arz::Record,
    factions: Option<&crate::factions::PlayerFactions>,
    loaded: Option<&gd_game_core::session::LoadedCharacter>,
) -> Option<FactionView> {
    mrec.str("friendlyNormalTable")?;
    let gf = db.get(gd_rules::faction::GAME_FACTIONS)?;
    let (idx, fkey) = gd_rules::faction::index_of(gf, npc.str("factions")?)?;
    let value = factions
        .map(|f| f.value(idx))
        .or_else(|| loaded.and_then(|c| c.0.factions.factions.get(idx).map(|f| f.value)))
        .unwrap_or(0.0);
    let levels = gd_rules::faction::Levels::from_record(gf);
    let level = levels.level_of(value).map_or(0, |i| i as u32 + 1);
    let open = gd_rules::faction::TIERS.map(|(_, need)| level >= need);
    info!("trade: faction {fkey} = {value} ({})", levels.tag(value));
    Some(FactionView {
        name_tag: gd_rules::faction::tag(idx).unwrap_or_default(),
        value,
        levels,
        open,
    })
}

/// Closes the open service; a market's refresh countdown starts and the NPC says goodbye.
fn close_service(
    visit: &mut ServiceVisit,
    timers: &mut MarketTimers,
    now: f64,
    db: &gd_formats::vfs::Database,
    says: &mut MessageWriter<NpcSay>,
) {
    visit.faction = None;
    let Some(open) = visit.open.take() else {
        return;
    };
    says.write(NpcSay::Goodbye(open.npc()));
    if let TradeOpen::Vendor { key, market, .. } = open
        && let Some(mrec) = db.get(&market)
    {
        close_market(timers, now, &key, mrec);
    }
}

/// Applies the [`ServiceRequest`]s, opens the service in reach and closes it when the player walks away.
#[allow(clippy::too_many_arguments)]
pub fn run_services(
    mut requests: MessageReader<ServiceRequest>,
    #[cfg(feature = "dev")] mut scratch: Local<gd_sim::nav::PathScratch>,
    mut answers: MessageWriter<ServiceAnswer>,
    mut visit: ResMut<ServiceVisit>,
    (mut markets, mut timers, time, session): (
        ResMut<Markets>,
        ResMut<MarketTimers>,
        Res<Time>,
        crate::session::Session,
    ),
    data: Res<gd_game_core::data::GameData>,
    mut inv: ResMut<Inventory>,
    progress: Res<gd_game_core::progress::PlayerProgress>,
    // Option: a new character has no save.
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    // Option: before `PlayerFactions` exists the save's standings are read.
    factions: Option<Res<crate::factions::PlayerFactions>>,
    // Plain NPCs have no `Combatant`: their radius is the record's.
    npcs: Query<(
        Entity,
        &GlobalTransform,
        &gd_game_core::character::Character,
    )>,
    mut players: Query<
        (
            &Transform,
            &mut crate::movement::MovePath,
            &mut crate::player_orders::InteractTarget,
            &gd_game_core::combat::Combatant,
        ),
        With<gd_game_core::player::Player>,
    >,
    mut says: MessageWriter<NpcSay>,
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    #[cfg(feature = "dev")] nav: Res<crate::nav::Nav>,
    // (opened, seconds the player has been in the world).
    #[cfg(feature = "dev")] (mut hook, mut fresh): (
        Local<(bool, f32)>,
        gd_game_core::session::NewSession,
    ),
) {
    #[cfg(feature = "dev")]
    if fresh.begun() {
        *hook = Default::default();
    }
    let now = time.elapsed_secs_f64();
    for r in requests.read() {
        match r {
            ServiceRequest::Close => {
                close_service(&mut visit, &mut timers, now, &data.db, &mut says)
            }
        }
    }
    let Ok((ptf, mut path, mut interact, pcb)) = players.single_mut() else {
        return;
    };
    // 3 s in: an answer sent before the UI is built after the loading screen is lost.
    #[cfg(feature = "dev")]
    if hooks.items.trade.is_some() {
        hook.1 += time.delta_secs();
    }
    #[cfg(feature = "dev")]
    if let Some(kind) = &hooks.items.trade
        && !hook.0
        && hook.1 >= 3.0
        && visit.open.is_none()
        && interact.0.is_none()
    {
        let nearest = npcs
            .iter()
            .filter(|(_, _, ch)| {
                service_of(&data.db, &ch.record)
                    .is_some_and(|k| k == kind || ch.record.contains(kind.as_str()))
            })
            .map(|(e, gt, _)| (e, gt.translation()))
            .min_by(|a, b| {
                a.1.distance(ptf.translation)
                    .total_cmp(&b.1.distance(ptf.translation))
            });
        match nearest {
            Some((e, at)) if at.distance(ptf.translation) < 40.0 => {
                hook.0 = true;
                interact.0 = Some(e);
                if let Some(p) =
                    crate::chests::path_near(&nav.0, &mut scratch, ptf.translation, at, 0.6)
                {
                    path.points = p;
                }
            }
            Some((_, at)) => info!("trade test: nearest {kind} is at {at:?}, too far"),
            None => {}
        }
    }
    if let Some(open) = &visit.open {
        let far = npcs.get(open.npc()).map_or(true, |(_, gt, _)| {
            gt.translation().distance(ptf.translation) > CLOSE_DISTANCE
        });
        if far {
            close_service(&mut visit, &mut timers, now, &data.db, &mut says);
            answers.write(ServiceAnswer::Closed);
        }
        return;
    }
    let Some(e) = interact.0 else { return };
    let Ok((_, gt, ch)) = npcs.get(e) else {
        return;
    };
    let Some(rec) = data.db.get(&ch.record) else {
        return;
    };
    // A plain NPC is `talk_input`'s.
    if service_of(&data.db, &ch.record).is_none() {
        return;
    }
    // `DefaultRequestNpcAction`: open at once within reach (3D), else at the walk's end
    // (`ControllerPlayerStateMoveToNpc::EndOfPathReached`).
    let d = gt.translation().distance(ptf.translation);
    let npc_radius =
        gd_game_core::combat::get_radius(Some(rec), gd_game_core::combat::ACTOR_RADIUS);
    if !crate::conversation::in_talk_range(d, pcb.radius, npc_radius) && !path.points.is_empty() {
        return;
    }
    interact.0 = None;
    path.points.clear();
    let mut faction = None;
    let open = match rec.class.as_str() {
        "NpcCaravan" => TradeOpen::Stash { npc: e },
        "NpcItemAscension" => TradeOpen::Ascension { npc: e },
        "NpcTransmuter" => TradeOpen::Illusionist { npc: e },
        "NpcEnchanter" => TradeOpen::Inventor { npc: e },
        "NpcCrafter" => TradeOpen::Crafter { npc: e },
        _ => {
            let difficulty = session.mode().difficulty;
            let Some(market) =
                gd_sim::items::market::market_file(rec, difficulty).map(str::to_owned)
            else {
                return;
            };
            let at = gt.translation();
            let key = gd_sim::items::market::market_key(&ch.record, at.x, at.z);
            match data.db.get(&market) {
                Some(mrec) => {
                    faction =
                        faction_view(&data.db, rec, mrec, factions.as_deref(), loaded.as_deref());
                    let level = crate::monsters::placed_char_level(
                        &data.db,
                        &ch.record,
                        progress.progress.level.max(1),
                    )
                    .max(1) as u32;
                    stock_market(
                        &mut markets,
                        &mut timers,
                        now,
                        &key,
                        mrec,
                        &data,
                        &mut inv,
                        &session.loot(level, level),
                    );
                }
                None if !markets.0.contains_key(&key) => return,
                None => {}
            }
            TradeOpen::Vendor {
                key,
                market,
                npc: e,
            }
        }
    };
    info!("trade: open {:?}", open);
    says.write(NpcSay::Hello(e));
    visit.open = Some(open.clone());
    visit.faction.clone_from(&faction);
    answers.write(ServiceAnswer::Opened {
        open,
        record: ch.record.clone(),
        faction,
    });
}

/// An NPC's spoken line request: `Npc::SayHello` / `Npc::SayGoodbye`, or `Npc::Converse`'s greeting.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NpcSay {
    /// `SayHello` (only an NPC without a `conversation`).
    Hello(Entity),
    /// `SayGoodbye` (only an NPC without a `conversation`).
    Goodbye(Entity),
    /// `Npc::Converse`'s greeting, whatever the NPC's `conversation`.
    Greet(Entity),
}

/// One `dialogPak` line: a `.wav` path played once in 3D at `at` (volume 0.8, fall-off 5.5, 44 100 Hz) in
/// the Dialog volume set.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct NpcVoice {
    pub npc: Entity,
    pub file: String,
    pub at: Vec3,
}

/// `DialogPak.longGreetingCountdown` (app seconds), reset to `longGreetingPeriod` by each greeting that found
/// it run out.
#[derive(Component, Clone, Copy, Debug)]
pub struct LongGreeting(pub f64);

#[derive(Clone, Debug, PartialEq)]
pub struct DialogPak {
    pub short: Vec<String>,
    pub long: Vec<String>,
    pub goodbye: Vec<String>,
    /// `longGreetingPeriod` (s): `(int)(value × 1000)` ms, 30 000 when that is 0.
    pub period: f64,
    /// The animation set slot of that name (case-insensitive), `None` when no slot has it.
    pub animation: gd_sim::anim::AnimType,
}

impl DialogPak {
    /// `DialogPak::Load`: `shortGreeting1..5`, `longGreeting1..5`, `goodbye1..5`, each non-empty one in order.
    pub fn load(rec: &gd_formats::arz::Record) -> Self {
        let list = |key: &str| {
            (1..=5)
                .filter_map(|i| rec.str(&format!("{key}{i}")).map(str::to_owned))
                .collect()
        };
        let ms = (rec.f32("longGreetingPeriod").unwrap_or(0.0) * 1000.0) as i32;
        DialogPak {
            short: list("shortGreeting"),
            long: list("longGreeting"),
            goodbye: list("goodbye"),
            period: if ms == 0 { 30.0 } else { ms as f64 / 1000.0 },
            animation: rec
                .str("animation")
                .and_then(|a| {
                    gd_sim::anim::ANIM_TYPE_NAMES
                        .iter()
                        .position(|n| n.eq_ignore_ascii_case(a))
                })
                .and_then(gd_sim::anim::AnimType::from_index)
                .unwrap_or(gd_sim::anim::AnimType::None),
        }
    }

    /// `DialogPak::PickSound`: a goodbye picks from `goodbye`; a greeting picks a short one while the
    /// countdown runs, else restarts it and picks a long one (else a short one). `rand() % n`; a `.ogg`
    /// stream is refused (`SoundManager::Load`).
    pub fn pick(&self, goodbye: bool, now: f64, long_until: &mut f64) -> Option<&str> {
        fn one(list: &[String]) -> Option<&str> {
            (!list.is_empty()).then(|| list[gd_game_core::rng::usize(..list.len())].as_str())
        }
        let line = if goodbye {
            one(&self.goodbye)
        } else if now < *long_until {
            one(&self.short)
        } else {
            *long_until = now + self.period;
            one(&self.long).or_else(|| one(&self.short))
        }?;
        (!line.to_ascii_lowercase().ends_with(".ogg")).then_some(line)
    }
}

/// Answers the [`NpcSay`]s with [`NpcVoice`]s; `DialogPak::Play` then plays the pak's `animation` slot
/// whether or not a line played (`AnimationSet::PlayAnimationIfAvailable`).
fn say_lines(
    mut commands: Commands,
    mut says: MessageReader<NpcSay>,
    mut voices: MessageWriter<NpcVoice>,
    data: Res<gd_game_core::data::GameData>,
    time: Res<Time>,
    mut npcs: Query<(
        &GlobalTransform,
        &mut gd_game_core::character::Character,
        Option<&mut LongGreeting>,
    )>,
) {
    let now = time.elapsed_secs_f64();
    for say in says.read() {
        let (npc, goodbye, checked) = match *say {
            NpcSay::Hello(e) => (e, false, true),
            NpcSay::Goodbye(e) => (e, true, true),
            NpcSay::Greet(e) => (e, false, false),
        };
        let Ok((gt, mut ch, long)) = npcs.get_mut(npc) else {
            continue;
        };
        let Some(rec) = data.db.get(&ch.record) else {
            continue;
        };
        // Only an NPC without a `conversation`.
        if checked && rec.str("conversation").is_some() {
            continue;
        }
        let Some(pak) = rec.str("dialogPak").and_then(|p| data.db.get(p)) else {
            continue;
        };
        let mut until = long.as_deref().map_or(0.0, |l| l.0);
        let pak = DialogPak::load(pak);
        let line = pak.pick(goodbye, now, &mut until).map(str::to_owned);
        ch.play_scripted(pak.animation);
        match long {
            Some(mut l) => l.0 = until,
            None => {
                commands.entity(npc).try_insert(LongGreeting(until));
            }
        }
        if let Some(file) = line {
            debug!("trade: {} says {file}", ch.record);
            voices.write(NpcVoice {
                npc,
                file,
                at: gt.translation(),
            });
        }
    }
}
