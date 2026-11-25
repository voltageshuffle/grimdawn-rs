//! The merchant, stash and inventory windows only write requests; the gd-game systems apply them.
use bevy::prelude::*;
use gd_formats::inventory::{EquipSlot, Footprint, Sack, drop_cell};
use gd_formats::save::item::Item;
use gd_game::testing::TestGame;
use gd_game::trade::{BuyRequest, Markets, SellRequest, Shop, TradeDone};
use gd_game_core::inventory::{InvRequest, Inventory, slot_index};
use gd_game_core::progress::PlayerProgress;
use gd_game_core::stash::{Stash, StashRequest};
use gd_sim::items::market::{BUYBACK_TAB, VENDOR_TABS};

const MARKET: &str = "records/creatures/npcs/merchants/merchanttbl_a101.dbr";
const SWORD: &str = "records/items/gearweapons/swords1h/a00_sword001.dbr";

fn sword(seed: u32) -> Item {
    Item {
        base_name: SWORD.into(),
        stack_count: 1,
        seed,
        ..Default::default()
    }
}

fn game() -> Option<TestGame> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.spawn_player();
    g.steps(3);
    Some(g)
}

fn shop(key: &str) -> Shop {
    Shop {
        key: key.into(),
        market: MARKET.into(),
        difficulty: 0,
    }
}

fn sword_size(g: &mut TestGame) -> Footprint {
    g.app
        .world_mut()
        .resource_mut::<Inventory>()
        .info(&sword(1))
        .expect("sword")
        .size
}

/// How many `TradeDone` answers were written since the last call.
fn done(g: &mut TestGame) -> usize {
    g.app
        .world_mut()
        .resource_mut::<Messages<TradeDone>>()
        .drain()
        .count()
}

#[test]
fn buy_and_sell_requests() {
    let Some(mut g) = game() else { return };
    let mut stock: Vec<Sack<Item>> = (0..VENDOR_TABS).map(|_| Sack::new(10, 18, true)).collect();
    stock[0].add(sword(4242), Footprint { w: 2, h: 4 });
    g.app
        .world_mut()
        .resource_mut::<Markets>()
        .0
        .insert("buy".into(), stock);
    g.app.world_mut().resource_mut::<Markets>().0.insert(
        "sell".into(),
        (0..VENDOR_TABS).map(|_| Sack::new(10, 18, true)).collect(),
    );
    g.app.world_mut().resource_mut::<PlayerProgress>().gold = 1_000_000;
    let (have, version) = {
        let inv = g.app.world().resource::<Inventory>();
        (inv.count_by_record(SWORD), inv.version)
    };
    // Buying: pays, the item arrives in the bags and leaves the stock.
    g.app.world_mut().write_message(BuyRequest {
        shop: shop("buy"),
        tab: 0,
        cell: (0, 0),
    });
    g.step();
    let w = g.app.world();
    assert_eq!(w.resource::<Inventory>().count_by_record(SWORD), have + 1);
    assert!(w.resource::<Inventory>().version > version);
    assert!(w.resource::<Markets>().0["buy"][0].items.is_empty());
    assert!(w.resource::<PlayerProgress>().gold < 1_000_000);
    assert_eq!(done(&mut g), 1);
    // Too poor: nothing happens and no answer.
    g.app
        .world_mut()
        .resource_mut::<Markets>()
        .0
        .get_mut("buy")
        .unwrap()[0]
        .add(sword(1), Footprint { w: 2, h: 4 });
    g.app.world_mut().resource_mut::<PlayerProgress>().gold = 0;
    g.app.world_mut().write_message(BuyRequest {
        shop: shop("buy"),
        tab: 0,
        cell: (0, 0),
    });
    g.step();
    assert_eq!(
        g.app.world().resource::<Markets>().0["buy"][0].items.len(),
        1
    );
    assert_eq!(done(&mut g), 0);
    // Selling the bought sword: out of the bag, gold paid, on the buyback tab.
    let cell = {
        let inv = g.app.world().resource::<Inventory>();
        let p = inv.bags[0]
            .items
            .iter()
            .find(|q| q.item.seed == 4242)
            .expect("bought sword in the main bag");
        (p.x, p.y)
    };
    g.app.world_mut().write_message(SellRequest {
        shop: shop("sell"),
        bag: 0,
        cell,
    });
    g.step();
    let w = g.app.world();
    assert!(
        w.resource::<Inventory>().bags[0]
            .item_at(cell.0, cell.1)
            .is_none()
    );
    assert!(w.resource::<PlayerProgress>().gold > 0, "paid");
    assert_eq!(
        w.resource::<Markets>().0["sell"][BUYBACK_TAB].items.len(),
        1
    );
    assert_eq!(done(&mut g), 1);
}

#[test]
fn stash_requests() {
    let Some(mut g) = game() else { return };
    let size = sword_size(&mut g);
    {
        let w = g.app.world_mut();
        let mut stash = w.resource_mut::<Stash>();
        stash.pages[0].items.clear();
        stash.pages[0].add(sword(77), size);
    }
    // Pick up the item, then put it down one cell lower (the cursor centres the footprint).
    g.app.world_mut().write_message(StashRequest::Click {
        transfer: false,
        page: 0,
        cell: (0, 0),
        cursor: Vec2::new(1.0, 2.0),
    });
    g.step();
    let w = g.app.world();
    assert_eq!(
        w.resource::<Inventory>().held.as_ref().map(|h| h.seed),
        Some(77)
    );
    assert!(w.resource::<Stash>().pages[0].items.is_empty());
    g.app.world_mut().write_message(StashRequest::Click {
        transfer: false,
        page: 0,
        cell: (0, 0),
        cursor: Vec2::new(1.0, 3.0),
    });
    g.step();
    let w = g.app.world();
    assert!(w.resource::<Inventory>().held.is_none());
    let page = &w.resource::<Stash>().pages[0];
    let want = drop_cell([1.0, 3.0], (page.width, page.height), size);
    assert_eq!((page.items[0].x, page.items[0].y), want);
    assert_ne!(want, (0, 0));
    // Sorting packs from the top left again.
    g.app.world_mut().write_message(StashRequest::Sort {
        transfer: false,
        page: 0,
    });
    g.step();
    let page = &g.app.world().resource::<Stash>().pages[0];
    assert_eq!((page.items[0].x, page.items[0].y), (0, 0));
    // A page costs gold; the answer names the new page.
    let pages = g.app.world().resource::<Stash>().pages.len();
    done(&mut g);
    g.app.world_mut().resource_mut::<PlayerProgress>().gold = 100_000_000;
    g.app
        .world_mut()
        .write_message(StashRequest::BuyPage { transfer: false });
    g.step();
    let w = g.app.world();
    assert_eq!(w.resource::<Stash>().pages.len(), pages + 1);
    let answers: Vec<_> = g
        .app
        .world_mut()
        .resource_mut::<Messages<TradeDone>>()
        .drain()
        .collect();
    assert_eq!(answers.last().and_then(|d| d.page), Some(pages));
}

#[test]
fn pick_up_drop_and_equip_requests() {
    let Some(mut g) = game() else { return };
    let size = sword_size(&mut g);
    {
        let mut inv = g.app.world_mut().resource_mut::<Inventory>();
        inv.held = None;
        // An empty corner of a fresh bag: a new, empty main bag cell.
        inv.bags[0].items.clear();
        inv.bags[0].add(sword(9001), size);
    }
    // Left click: the item goes to the hand.
    g.app.world_mut().write_message(InvRequest::Bag {
        bag: 0,
        cell: (0, 0),
        cursor: Vec2::new(1.0, 2.0),
        right: false,
    });
    g.step();
    let inv = g.app.world().resource::<Inventory>();
    assert_eq!(inv.held.as_ref().map(|h| h.seed), Some(9001));
    assert!(inv.bags[0].items.is_empty());
    // Dropped on the bag with the cursor centring the footprint.
    g.app.world_mut().write_message(InvRequest::Bag {
        bag: 0,
        cell: (3, 3),
        cursor: Vec2::new(4.0, 5.0),
        right: false,
    });
    g.step();
    let inv = g.app.world().resource::<Inventory>();
    assert!(inv.held.is_none());
    let want = drop_cell([4.0, 5.0], (inv.bags[0].width, inv.bags[0].height), size);
    assert_eq!((inv.bags[0].items[0].x, inv.bags[0].items[0].y), want);
    // Pick it up again and click the main hand: equipped (the old weapon in hand).
    g.app.world_mut().write_message(InvRequest::Bag {
        bag: 0,
        cell: want,
        cursor: Vec2::new(4.0, 5.0),
        right: false,
    });
    g.step();
    g.app.world_mut().write_message(InvRequest::Slot {
        slot: EquipSlot::MainHand,
        right: false,
    });
    g.step();
    let inv = g.app.world().resource::<Inventory>();
    assert_eq!(
        inv.equipped[slot_index(EquipSlot::MainHand)]
            .as_ref()
            .map(|i| i.seed),
        Some(9001)
    );
    g.app.world_mut().resource_mut::<Inventory>().held = None;
    // Right click on the slot with nothing held: nothing.
    let slot = |g: &mut TestGame, right: bool| {
        g.app.world_mut().write_message(InvRequest::Slot {
            slot: EquipSlot::MainHand,
            right,
        });
        g.step();
    };
    let bag_click = |g: &mut TestGame, cursor: Vec2, right: bool| {
        g.app.world_mut().write_message(InvRequest::Bag {
            bag: 0,
            cell: (cursor.x as u32, cursor.y as u32),
            cursor,
            right,
        });
        g.step();
    };
    let main_hand = |g: &TestGame| {
        g.app.world().resource::<Inventory>().equipped[slot_index(EquipSlot::MainHand)]
            .as_ref()
            .map(|i| i.seed)
    };
    slot(&mut g, true);
    assert_eq!(main_hand(&g), Some(9001), "a right click leaves it worn");
    // Picked up from the slot, a right click on a bag cancels the move: not a bag source, so it is
    // equipped into the empty slot again.
    slot(&mut g, false);
    assert_eq!(main_hand(&g), None);
    bag_click(&mut g, Vec2::new(8.0, 5.0), true);
    assert_eq!(main_hand(&g), Some(9001), "given back: equip-empty first");
    assert!(g.app.world().resource::<Inventory>().held.is_none());
    // From a bag cell: a right click elsewhere in the bag puts it back at its cell.
    slot(&mut g, false);
    bag_click(&mut g, Vec2::new(4.0, 5.0), false);
    let at = {
        let inv = g.app.world().resource::<Inventory>();
        let q = inv.bags[0]
            .items
            .iter()
            .find(|q| q.item.seed == 9001)
            .unwrap();
        (q.x, q.y)
    };
    bag_click(
        &mut g,
        Vec2::new(at.0 as f32 + 0.5, at.1 as f32 + 0.5),
        false,
    );
    assert_eq!(
        g.app
            .world()
            .resource::<Inventory>()
            .held
            .as_ref()
            .map(|h| h.seed),
        Some(9001)
    );
    bag_click(&mut g, Vec2::new(9.0, 1.0), true);
    let inv = g.app.world().resource::<Inventory>();
    assert!(inv.held.is_none());
    assert_eq!(
        inv.bags[0]
            .item_at(at.0, at.1)
            .map(|q| (q.x, q.y, q.item.seed)),
        Some((at.0, at.1, 9001)),
        "back at its own cell"
    );
    // With an item held, a right click on a slot is a left one (no merchant).
    bag_click(
        &mut g,
        Vec2::new(at.0 as f32 + 0.5, at.1 as f32 + 0.5),
        false,
    );
    slot(&mut g, true);
    assert_eq!(main_hand(&g), Some(9001));
    slot(&mut g, false);
    bag_click(&mut g, Vec2::new(4.0, 5.0), false);
    // Right click on a bag item equips it (nothing held).
    g.app.world_mut().resource_mut::<Inventory>().held = None;
    let inv = g.app.world().resource::<Inventory>();
    let cell = {
        let p = inv
            .bags
            .iter()
            .flat_map(|b| &b.items)
            .find(|q| q.item.seed == 9001)
            .expect("the sword is in a bag");
        (p.x, p.y)
    };
    let bag = inv
        .bags
        .iter()
        .position(|b| {
            b.item_at(cell.0, cell.1)
                .is_some_and(|q| q.item.seed == 9001)
        })
        .unwrap();
    g.app.world_mut().write_message(InvRequest::Bag {
        bag,
        cell,
        cursor: Vec2::ZERO,
        right: true,
    });
    g.step();
    let inv = g.app.world().resource::<Inventory>();
    assert!(
        inv.worn().iter().any(|i| i.seed == 9001),
        "equipped by right click"
    );
}
