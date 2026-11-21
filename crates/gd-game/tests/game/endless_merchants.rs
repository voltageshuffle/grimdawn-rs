//! The Splendors of the Shattered Realm merchants (`merchant_reward_02..05`) on Ultimate, headless: what each sells
//! (`NpcMerchant::Load` @0x1803bd830: `marketFileName[GameInfo::GetDifficulty()]`, `gd_sim::items::market`).
use bevy::prelude::*;
use gd_game::session::{GameMode, LoadedCharacter};
use gd_game::testing::TestGame;

const ED: &str = "records/endlessdungeon/";

/// The expected stock of a market record: each category's table (a `LevelTable` of one `LootItemTable_DynWeight`
/// with one item), once per category (`marketNum<X>Min..Max` 1..1), as `MarketMiniMart::Refresh` rolls it.
fn expected_stock(db: &gd_formats::vfs::Database, market: &str) -> Vec<String> {
    let m = db.get(market).expect("market record");
    let mut want = Vec::new();
    for (field, num) in gd_sim::items::market::CATEGORIES {
        let Some(table) = m.str(field).filter(|s| !s.is_empty()) else {
            continue;
        };
        let num = num.unwrap();
        let (lo, hi) = (
            m.i32(&format!("marketNum{num}Min")).unwrap_or(0),
            m.i32(&format!("marketNum{num}Max")).unwrap_or(0),
        );
        assert_eq!((lo, hi), (1, 1), "{market} {field}");
        let level = db.get(table).expect("level table");
        assert_eq!(level.class, "LevelTable");
        let dynw = db.get(level.strs("records")[0]).expect("item table");
        assert!(
            dynw.str("lootName2").is_none_or(str::is_empty),
            "{table}: one item"
        );
        want.push(dynw.str("lootName1").unwrap().to_owned());
    }
    want.sort();
    want
}

/// Each Splendor merchant sells its Ultimate table (`merchanttbl_reward_0Nu`, the session difficulty's entry of
/// `marketFileName`): one Shattered set piece per category, every one stocked (the port knows each item); on
/// Normal / Elite the shared `_01` / `_01e` tables sell nothing.
#[test]
fn the_splendor_merchants_sell_their_ultimate_tables() {
    use gd_game::inventory::Inventory;
    use gd_game::trade::{MarketTimers, Markets, stock_market};
    use gd_sim::items::market::market_file;
    // An Ultimate session (the menu's Ultimate choice) of a character the port saved.
    let Some(mut new) = TestGame::new_character("Tester", false, false) else {
        return;
    };
    new.spawn_player();
    new.steps(2);
    let Some(mut g) = TestGame::with_save_mode(&new.save_now(), 2, false) else {
        return;
    };
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    g.steps(2);
    let data = g.data();
    let difficulty = g
        .app
        .world()
        .resource::<LoadedCharacter>()
        .0
        .info
        .difficulty();
    assert_eq!(
        difficulty, 2,
        "the session's difficulty reaches the save info"
    );
    let mode = g.app.world().resource::<GameMode>().0;
    let level = g.progress().progress.level.max(1) as u32;
    let altar = g
        .app
        .world()
        .resource::<gd_game::ascendant::AltarState>()
        .formulas
        .clone();
    let params =
        gd_sim::items::LootParams::new(level, level, difficulty as u32).with_session(mode, &altar);
    for n in 2..=5u32 {
        let npc = data
            .db
            .get(&format!("{ED}creatures/npcs/merchant_reward_0{n}.dbr"))
            .expect("merchant");
        assert_eq!(npc.class, "NpcMerchant");
        let market = market_file(npc, difficulty).expect("a market").to_owned();
        assert_eq!(
            market,
            format!("{ED}creatures/npcs/merchanttbl_reward_0{n}u.dbr")
        );
        let want = expected_stock(&data.db, &market);
        assert!(want.len() >= 5, "merchant {n}: {want:?}");
        let mrec = data.db.get(&market).unwrap();
        let key = format!("splendor{n}");
        let w = g.app.world_mut();
        let got = w.resource_scope(|w, mut markets: Mut<Markets>| {
            let mut timers = MarketTimers::default();
            let mut inv = w.resource_mut::<Inventory>();
            stock_market(
                &mut markets,
                &mut timers,
                0.0,
                &key,
                mrec,
                &data,
                &mut inv,
                &params,
            );
            let mut got: Vec<String> = markets.0[&key]
                .iter()
                .flat_map(|p| p.items.iter().map(|q| q.item.base_name.clone()))
                .collect();
            got.sort();
            got
        });
        assert_eq!(got, want, "merchant {n}");
        // Normal and Elite: the shared tables have no categories.
        for d in 0..2 {
            let low = market_file(npc, d).unwrap();
            assert!(expected_stock(&data.db, low).is_empty(), "{low}");
        }
    }
}
