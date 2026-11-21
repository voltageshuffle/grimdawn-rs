//! Proxy spawning against the installed game database ([`gd_sim::testdata`]).
use gd_sim::combat::Rng;
use gd_sim::mode::Mode;
use gd_sim::proxy::{run_pool, run_proxy};

#[test]
fn raptor_pool() {
    let Some(db) = gd_sim::testdata::db() else {
        return;
    };
    let mut counts = Vec::new();
    for seed in 1..200u32 {
        let mut rng = Rng(seed * 7919);
        let s = run_pool(
            &db,
            "records/proxies/pools/p_beastsswamp_raptor_n.dbr",
            5,
            5,
            Mode::default(),
            &mut rng,
        );
        // spawnMin 4, spawnMax 6 (+1 from `challengeAdjustment`, applied on
        // Normal / Elite with or without Veteran), champions included.
        // Champions count toward the roll, so they only raise the ceiling
        // when they alone outnumber it.
        let champions = s.iter().filter(|x| x.champion).count();
        assert!((4..=7.max(champions)).contains(&s.len()), "{s:?}");
        for x in &s {
            // lv2_normal: [L-1, L]; weak+: L-1; normal+ etc. — all ≥ 1 and close to 5.
            assert!((1..=8).contains(&x.level), "{x:?}");
        }
        counts.push(s.len());
    }
    assert!(counts.iter().any(|&c| c >= 5));
}

#[test]
fn experience_thresholds() {
    let Some(db) = gd_sim::testdata::db() else {
        return;
    };
    let rules = gd_sim::actor::Rules::load(&db).unwrap();
    use gd_sim::progress::Progress;
    let t = |l| Progress::threshold(&rules.levels, l);
    eprintln!(
        "eq(0)={} eq(1)={} eq(53)={} eq(54)={}",
        t(0),
        t(1),
        t(53),
        t(54)
    );
    // A real level-54 save has 3_475_085 total experience: it is past the
    // threshold to leave 53 and short of the one to leave 54.
    assert!(t(53) <= 3_475_085 && 3_475_085 < t(54));
    let mut p = Progress {
        level: 1,
        ..Default::default()
    };
    let gained = p.add_experience(&rules.levels, t(1) as u32, 0.0);
    assert_eq!((gained, p.level), (1, 2));
    assert_eq!(p.give_levels(&rules.levels, 3), 3);
    assert_eq!((p.level, p.experience), (5, t(4)));
    let xp = gd_sim::progress::experience_reward(
        &db,
        "records/creatures/enemies/zombie_a01.dbr",
        1,
        1,
        Mode::default(),
    );
    eprintln!("zombie lvl1 xp {xp}");
    assert!(xp > 0);
    let mut rng = Rng(5);
    let g: Vec<u32> = (0..50)
        .flat_map(|_| {
            gd_sim::progress::gold_drop(
                &db,
                "records/items/lootchests/chestloottables/a01_moneygenerator.dbr",
                0.0,
                5,
                &mut rng,
            )
        })
        .collect();
    eprintln!("gold piles {:?}", &g[..6]);
    assert!(g.iter().all(|&x| (1..=1500).contains(&x)));
}

#[test]
fn chest_proxies_and_container_loot() {
    let Some(db) = gd_sim::testdata::db() else {
        return;
    };
    let mut rng = Rng(7);
    // A chest proxy: no pools, `accessory1` → pool (100 % fixed item),
    // gated by chanceToRun 15.
    let runs: Vec<_> = (0..400)
        .map(|_| {
            run_proxy(
                &db,
                "records/items/lootchests/chestproxies/chestproxy_corpsecocoon_b01_r.dbr",
                10,
                10,
                Mode::default(),
                0.0,
                true,
                &mut rng,
            )
        })
        .collect();
    let n = runs.iter().filter(|r| !r.accessories.is_empty()).count();
    assert!(runs.iter().all(|r| r.monsters.is_empty()));
    assert!((30..100).contains(&n), "{n} of 400 ran");
    assert!(
        runs.iter()
            .flat_map(|r| &r.accessories)
            .all(|a| a.contains("lootchests/")),
        "{:?}",
        runs.iter().find(|r| !r.accessories.is_empty())
    );
    // A hero proxy's accessory1 names a chest *proxy*: no fixed items.
    let hero = run_proxy(
        &db,
        "records/proxies/area001/cave_spidergiant_t.dbr",
        10,
        10,
        Mode::default(),
        0.0,
        true,
        &mut rng,
    );
    assert!(hero.accessories.is_empty() && !hero.monsters.is_empty());

    use gd_sim::items::{ItemDb, LootParams};
    let idb = ItemDb::new(db);
    let chest = idb
        .get("records/items/lootchests/b01_chestcorpse02.dbr")
        .unwrap();
    assert_eq!(
        gd_sim::items::loot::container_level(chest, 10, 1, None, 0),
        11
    );
    assert_eq!(
        gd_sim::items::loot::container_level(chest, 10, 0, Some((12, 20, 1)), 0),
        12
    );
    let p = LootParams::at_level(10);
    // `numberOfPlayers*1+1` items, each from loot1..3 by the chances of its index.
    for _ in 0..20 {
        let d = gd_sim::items::loot::container_drops(
            &idb,
            "records/items/lootchests/b01_chestcorpse02.dbr",
            &p,
            &mut rng,
        );
        assert!(d.len() <= 2, "{d:?}");
    }
    let potions = gd_sim::items::loot::container_drops(
        &idb,
        "records/items/lootchests/questchests/questchest_intropotionchest.dbr",
        &p,
        &mut rng,
    );
    eprintln!("intro chest: {potions:?}");
    assert_eq!(potions.len(), 1);
}

#[test]
fn market_stock_and_prices() {
    use gd_sim::items::{ItemDb, market};
    let Some(db) = gd_sim::testdata::db() else {
        return;
    };
    let idb = ItemDb::new(db);
    let m = idb
        .get("records/creatures/npcs/merchants/merchanttbl_a01.dbr")
        .unwrap();
    let mut rng = Rng(3);
    let stock = market::roll_market(&idb, m, &gd_sim::items::LootParams::at_level(10), &mut rng);
    eprintln!("{} items", stock.len());
    assert!(stock.len() > 60, "{}", stock.len());
    let costs: Vec<u32> = stock
        .iter()
        .map(|r| gd_sim::items::resolve::item_cost(&idb, &r.to_item()))
        .collect();
    eprintln!("costs {:?}", &costs[..10]);
    // Markets roll with `SetNoBrokenItems` (`MarketMiniMart::Refresh`):
    // no Broken affixes, so every item has a price.
    let broken = |r: &gd_sim::items::loot::LootRoll| {
        r.prefix.contains("/broken/") || r.suffix.contains("/broken/")
    };
    assert!(!stock.iter().any(broken));
    assert!(
        costs.iter().all(|c| *c > 0),
        "{:?}",
        stock.iter().zip(&costs).find(|(_, c)| **c == 0)
    );
    let c = *costs.iter().find(|c| **c > 0).unwrap();
    assert!(market::sell_price(m, c, 1, 0) < market::buy_price(m, c));
    let high = market::roll_market(&idb, m, &gd_sim::items::LootParams::at_level(54), &mut rng);
    assert!(!high.iter().any(broken));
    // `AddIfUnique`: one category lists an item once (other categories may
    // pick it too). The axe category alone, at a level with few axes:
    let mut axes = (*m).clone();
    std::sync::Arc::make_mut(&mut axes.fields)
        .retain(|(k, _)| k.starts_with("marketAxe") || k.starts_with("marketNumAxe"));
    for _ in 0..20 {
        let roll = market::roll_market(
            &idb,
            &axes,
            &gd_sim::items::LootParams::at_level(3),
            &mut rng,
        );
        let names: std::collections::HashSet<_> = roll
            .iter()
            .map(|r| (&r.base, &r.prefix, &r.suffix))
            .collect();
        assert_eq!(names.len(), roll.len(), "{roll:?}");
    }
    // Affixed (magic / rare) stock is common once Broken is out.
    let affixed = high
        .iter()
        .filter(|r| !r.prefix.is_empty() || !r.suffix.is_empty())
        .count();
    eprintln!("level 54: {affixed} of {} affixed", high.len());
    let zero: Vec<_> = high
        .iter()
        .filter(|r| !broken(r) && gd_sim::items::resolve::item_cost(&idb, &r.to_item()) == 0)
        .map(|r| r.base.clone())
        .collect();
    eprintln!(
        "level 54: {} items, {} free: {:?}",
        high.len(),
        zero.len(),
        &zero[..zero.len().min(5)]
    );
    assert!(zero.len() * 20 < high.len());
}

#[test]
fn crafting_blueprints() {
    use gd_sim::items::{
        ItemDb,
        crafting::{self, Blueprint},
    };
    let Some(db) = gd_sim::testdata::db() else {
        return;
    };
    let idb = ItemDb::new(db);
    let smith = idb
        .get("records/creatures/npcs/merchants/blacksmitha02.dbr")
        .unwrap();
    let window = idb
        .get("records/ui/inventor/craftingpanel/crafting_table.dbr")
        .unwrap();
    let list = crafting::recipes(smith, &window.strs("craftingDefaultRecipes"), &[]);
    assert!(list.len() > 80, "{}", list.len());
    let mut rng = Rng(7);
    let axe = "records/items/crafting/blueprints/random/craft_randomaxe.dbr";
    let bp = Blueprint::load(axe, idb.get(axe).unwrap()).unwrap();
    assert_eq!(
        bp.reagents,
        vec![(
            vec!["records/items/questitems/scrapmetal.dbr".to_string()],
            2
        )]
    );
    // (54*16)^1.16 + 6
    assert_eq!(
        bp.cost(54),
        ((54.0f32 * 16.0).powf(1.16) + 6.0 + 0.5) as u64
    );
    let items = bp.craft(
        &idb,
        &gd_sim::items::LootParams::at_level(54),
        smith.str("enhancementTable"),
        &mut rng,
    );
    eprintln!("{items:?}");
    assert_eq!(items.len(), 1);
    assert!(items[0].base_name.contains("axe"), "{}", items[0].base_name);
    assert!(
        items[0].modifier_name.contains("lootaffixes/crafting"),
        "{}",
        items[0].modifier_name
    );
    let r = gd_sim::items::resolve(&idb, &items[0]).unwrap();
    assert!(r.is_weapon);
    let pot = "records/items/crafting/blueprints/other/craft_potion_fireresist.dbr";
    let bp = Blueprint::load(pot, idb.get(pot).unwrap()).unwrap();
    assert_eq!(bp.cost(54), 200);
    assert_eq!(bp.reagents.len(), 2);
    assert_eq!(
        bp.craft(
            &idb,
            &gd_sim::items::LootParams::at_level(54),
            None,
            &mut rng
        )[0]
        .base_name,
        "records/items/crafting/consumables/potion_fireresist01.dbr"
    );
}
