//! Headless checks of merchants: selling pays and puts the item on the
//! buyback tab at that price; buying it back costs the same and empties the
//! slot; restocking; faction tiers. One game runs every check in turn (each
//! on its own market key).
use bevy::prelude::*;
use gd_formats::inventory::Sack;
use gd_formats::save::item::Item;
use gd_game::testing::TestGame;
use gd_game::trade::{Markets, buy_item, sell_item};
use gd_game_core::inventory::Inventory;
use gd_game_core::progress::PlayerProgress;
use gd_sim::items::market::{BUYBACK_TAB, VENDOR_TABS};

const MARKET: &str = "records/creatures/npcs/merchants/merchanttbl_a101.dbr";
const SWORD: &str = "records/items/gearweapons/swords1h/a00_sword001.dbr";

#[test]
fn merchants() {
    let Some(mut g) = TestGame::new_character("Tester", false, false) else {
        return;
    };
    g.spawn_player();
    g.steps(2);
    sell_then_buy_back(&mut g);
    restock_on_refresh(&mut g);
    faction_market_lists_each_tier_item_once(&mut g);
}

fn sell_then_buy_back(g: &mut TestGame) {
    let data = g.data();
    let idb = gd_sim::items::ItemDb::new(data.db.clone());
    let w = g.app.world_mut();
    w.resource_mut::<Markets>().0.insert(
        "npc".into(),
        (0..VENDOR_TABS).map(|_| Sack::new(10, 18, true)).collect(),
    );
    let gold0 = w.resource::<PlayerProgress>().gold;
    let (sold, bought) = w.resource_scope(|w, mut markets: Mut<Markets>| {
        w.resource_scope(|w, mut inv: Mut<Inventory>| {
            let mut gold = w.resource::<PlayerProgress>().gold;
            let sword = Item {
                base_name: SWORD.into(),
                stack_count: 1,
                seed: 3,
                ..Default::default()
            };
            let sold = sell_item(
                &mut markets,
                &idb,
                "npc",
                MARKET,
                sword,
                0,
                0,
                &mut gold,
                &mut inv,
            );
            let cell = markets.0["npc"][BUYBACK_TAB].items[0].clone();
            let have = inv.count_by_record(SWORD);
            let bought = buy_item(
                &mut markets,
                &idb,
                "npc",
                MARKET,
                BUYBACK_TAB,
                (cell.x, cell.y),
                false,
                0,
                0,
                &mut gold,
                &mut inv,
            );
            assert_eq!(inv.count_by_record(SWORD), have + 1);
            assert!(markets.0["npc"][BUYBACK_TAB].items.is_empty());
            w.resource_mut::<PlayerProgress>().gold = gold;
            (sold, bought)
        })
    });
    assert!(sold > 0);
    assert_eq!(bought, Ok(sold), "bought back for the sale price");
    assert_eq!(g.progress().gold, gold0);
}

fn restock_on_refresh(g: &mut TestGame) {
    use gd_game::trade::{MarketTimers, stock_market};
    let data = g.data();
    let mrec = data.db.get(MARKET).expect("market");
    let params = gd_sim::items::LootParams::at_level(30);
    let w = g.app.world_mut();
    w.resource_scope(|w, mut markets: Mut<Markets>| {
        w.resource_scope(|w, mut timers: Mut<MarketTimers>| {
            let mut inv = w.resource_mut::<Inventory>();
            let ids = |m: &Markets| {
                m.0["restock"]
                    .iter()
                    .map(|p| p.items.iter().map(|q| q.item.seed).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            };
            stock_market(
                &mut markets,
                &mut timers,
                0.0,
                "restock",
                mrec,
                &data,
                &mut inv,
                &params,
            );
            let first = ids(&markets);
            assert!(
                first[..BUYBACK_TAB].iter().map(Vec::len).sum::<usize>() > 0,
                "stocked"
            );
            assert!(first[BUYBACK_TAB].is_empty());
            // Before the refresh time: the same stock, buyback kept.
            markets.1.insert("restock".into(), Default::default());
            stock_market(
                &mut markets,
                &mut timers,
                60.0,
                "restock",
                mrec,
                &data,
                &mut inv,
                &params,
            );
            assert_eq!(ids(&markets), first);
            assert!(markets.1.contains_key("restock"));
            // After it: new stock, buyback prices gone.
            let due = timers.0["restock"];
            stock_market(
                &mut markets,
                &mut timers,
                due + 1.0,
                "restock",
                mrec,
                &data,
                &mut inv,
                &params,
            );
            assert_ne!(ids(&markets), first, "restocked");
            assert!(!markets.1.contains_key("restock"));
            assert!(timers.0["restock"] > due);
        })
    });
}

fn faction_market_lists_each_tier_item_once(g: &mut TestGame) {
    use gd_game::trade::{MarketTimers, stock_market};
    let data = g.data();
    let mrec = data
        .db
        .get("records/creatures/npcs/merchants/factiontables/_merchanttbl_faction_coven.dbr")
        .expect("faction market");
    let tier = data
        .db
        .get(mrec.str("friendlyNormalTable").unwrap())
        .unwrap();
    let mut want: Vec<String> = tier
        .strs("marketStaticItems")
        .into_iter()
        .map(str::to_owned)
        .collect();
    want.sort();
    want.dedup();
    let w = g.app.world_mut();
    w.resource_scope(|w, mut markets: Mut<Markets>| {
        let mut timers = MarketTimers::default();
        let mut inv = w.resource_mut::<Inventory>();
        stock_market(
            &mut markets,
            &mut timers,
            0.0,
            "f",
            mrec,
            &data,
            &mut inv,
            &gd_sim::items::LootParams::at_level(30),
        );
        let mut got: Vec<String> = markets.0["f"][0]
            .items
            .iter()
            .map(|q| q.item.base_name.clone())
            .collect();
        got.sort();
        assert_eq!(got, want);
        assert!(markets.0["f"][0].items.iter().all(|q| q.item.seed != 0));
    });
}
