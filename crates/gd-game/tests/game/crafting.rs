//! Headless checks of the inventor's and blacksmith's transactions:
//! crafting pays and takes reagents, dismantling needs gold and Dynamite,
//! salvaging returns the component, a seed reroll pays its plan, a set
//! piece converts. No frames needed: one game runs every check in turn.
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game::crafting::{Refusal, SalvageKind, craft, dismantle_item, salvage_item};
use gd_game::reroll::{Kind, reroll_item};
use gd_game::testing::{TestGame, first_blueprint};
use gd_game_core::inventory::{Inventory, Requirements};
use gd_game_core::progress::PlayerProgress;
use gd_game_core::stash::Stash;
use gd_sim::items::LootParams;

const SWORD: &str = "records/items/gearweapons/swords1h/a00_sword001.dbr";

#[test]
fn inventor_and_blacksmith_transactions() {
    let Some(mut g) = TestGame::new_character("Tester", false, false) else {
        return;
    };
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    g.steps(2);
    crafting_pays_and_takes_reagents(&mut g);
    dismantle_needs_dynamite_and_gold(&mut g);
    salvage_keeps_the_component(&mut g);
    seed_reroll_changes_the_seed(&mut g);
    convert_makes_another_set_piece(&mut g);
    deposit_moves_crafting_materials(&mut g);
}

/// The character window's move-reagents button: every `craftingMaterial`
/// stack in the bags goes into the material storage, other items stay.
fn deposit_moves_crafting_materials(g: &mut TestGame) {
    const MAT: &str = "records/items/materia/compa_frozenheart.dbr";
    let before = with(g, |_, inv, stash, _| {
        inv.bags.iter_mut().for_each(|b| {
            b.items
                .retain(|p| !p.item.base_name.eq_ignore_ascii_case(MAT))
        });
        assert!(inv.add_to_bags(Item {
            stack_count: 3,
            ..item(MAT)
        }));
        assert!(inv.add_to_bags(item(SWORD)));
        stash
            .materials
            .entries
            .iter()
            .find(|e| e.record.eq_ignore_ascii_case(MAT))
            .map_or(0, |e| e.count)
    });
    g.app
        .world_mut()
        .write_message(gd_game_core::stash::DepositReagents);
    g.steps(1);
    with(g, |data, inv, stash, _| {
        let stored = stash
            .materials
            .entries
            .iter()
            .find(|e| e.record.eq_ignore_ascii_case(MAT))
            .map_or(0, |e| e.count);
        assert_eq!(stored, before + 3);
        assert_eq!(inv.count_by_record(MAT), 0);
        assert!(inv.count_by_record(SWORD) > 0, "non-materials stay");
        let left = inv
            .bags
            .iter()
            .flat_map(|b| &b.items)
            .filter(|p| {
                data.db
                    .get(&p.item.base_name)
                    .is_some_and(|r| r.i32("craftingMaterial").unwrap_or(0) != 0)
            })
            .count();
        assert_eq!(left, 0);
    });
}

fn item(rec: &str) -> Item {
    Item {
        base_name: rec.into(),
        stack_count: 1,
        seed: 7,
        ..Default::default()
    }
}

/// Runs `f` with the inventory, stash and gold.
fn with<R>(
    g: &mut TestGame,
    f: impl FnOnce(&gd_game_core::data::GameData, &mut Inventory, &mut Stash, &mut u64) -> R,
) -> R {
    let data = g.data();
    let w = g.app.world_mut();
    w.resource_scope(|w, mut inv: Mut<Inventory>| {
        w.resource_scope(|w, mut stash: Mut<Stash>| {
            let mut gold = w.resource::<PlayerProgress>().gold;
            let r = f(&data, &mut inv, &mut stash, &mut gold);
            w.resource_mut::<PlayerProgress>().gold = gold;
            r
        })
    })
}

fn crafting_pays_and_takes_reagents(g: &mut TestGame) {
    let db = g.data().db;
    let bp = first_blueprint(&db);
    let p = g.player();
    let req = *g.get::<Requirements>(p);
    // Not enough reagents: refused, nothing taken.
    let refused = with(g, |data, inv, stash, gold| {
        for (names, _) in &bp.reagents {
            let n = gd_game_core::stash::count(names, inv, stash);
            gd_game_core::stash::take(names, n, inv, stash);
        }
        craft(
            data,
            None,
            &bp,
            gold,
            &LootParams::at_level(54),
            inv,
            stash,
            &req,
        )
    });
    refused.unwrap_err();
    let made = with(g, |data, inv, stash, gold| {
        *gold += bp.cost(54) + 1;
        for (names, n) in &bp.reagents {
            for _ in 0..*n {
                inv.add_to_bags(item(&names[0]));
            }
        }
        let before = *gold;
        let r = craft(
            data,
            None,
            &bp,
            gold,
            &LootParams::at_level(54),
            inv,
            stash,
            &req,
        );
        (
            r,
            before - *gold,
            bp.reagents
                .iter()
                .map(|(names, _)| gd_game_core::stash::count(names, inv, stash))
                .sum::<u32>(),
        )
    });
    let (r, paid, left) = made;
    assert!(r.is_ok_and(|items| !items.is_empty()), "crafting failed");
    assert_eq!(paid, bp.cost(54));
    assert_eq!(left, 0, "reagents taken");
}

fn dismantle_needs_dynamite_and_gold(g: &mut TestGame) {
    let dynamite = g
        .data()
        .db
        .get("records/game/gameengine.dbr")
        .and_then(|e| e.str("dynamite").map(str::to_owned))
        .unwrap();
    let (first, second) = with(g, |data, inv, stash, gold| {
        let n = gd_game_core::stash::count(std::slice::from_ref(&dynamite), inv, stash);
        gd_game_core::stash::take(std::slice::from_ref(&dynamite), n, inv, stash);
        // Enough gold: only the Dynamite is missing.
        *gold += 1_000_000;
        let first = dismantle_item(
            data,
            item(SWORD),
            true,
            &LootParams::at_level(54),
            gold,
            inv,
            stash,
        )
        .map(|_| ())
        .map_err(|(w, _)| w);
        inv.add_to_bags(item(&dynamite));
        let second = dismantle_item(
            data,
            item(SWORD),
            true,
            &LootParams::at_level(54),
            gold,
            inv,
            stash,
        )
        .map(|_| ());
        (
            first,
            second.is_ok()
                && gd_game_core::stash::count(std::slice::from_ref(&dynamite), inv, stash) == 0,
        )
    });
    assert_eq!(first, Err(Refusal::Missing(dynamite.clone(), 1)));
    assert!(second, "dismantled, Dynamite used");
}

#[allow(clippy::result_large_err)] // `salvage_item` hands the item back
fn salvage_keeps_the_component(g: &mut TestGame) {
    let comp = "records/items/materia/compa_serratedspike.dbr";
    let p = g.player();
    let req = *g.get::<Requirements>(p);
    let r = with(g, |data, inv, _, gold| {
        *gold += 1_000_000;
        let mut it = item(SWORD);
        it.relic_name = comp.into();
        let before = inv.count_by_record(comp);
        salvage_item(data, it, SalvageKind::KeepAddon, gold, inv, &req)
            .map(|_| inv.count_by_record(comp) - before)
    });
    assert_eq!(r.ok(), Some(1));
}

fn seed_reroll_changes_the_seed(g: &mut TestGame) {
    let r = with(g, |data, inv, stash, gold| {
        let idb = gd_sim::items::ItemDb::new(data.db.clone());
        let mut it = item(SWORD);
        let res = reroll_item(
            data,
            &idb,
            &mut it,
            Kind::Seed,
            true,
            true,
            &[],
            54,
            gold,
            inv,
            stash,
        );
        (res, it.seed, it.seed_rerolls)
    });
    assert!(r.0.is_ok(), "{:?}", r.0);
    assert_ne!(r.1, item(SWORD).seed, "the seed changed");
    assert_eq!(r.2, 1);
}

#[allow(clippy::result_large_err)] // `convert_item` hands the item back
fn convert_makes_another_set_piece(g: &mut TestGame) {
    use gd_game::crafting::convert_item;
    // Guardsman's Raiment (`itemset_c001`, level 20).
    let head = "records/items/gearhead/c006_head.dbr";
    let scrap = "records/items/questitems/scrapmetal.dbr";
    let set_of = |data: &gd_game_core::data::GameData, name: &str| {
        data.db
            .get(name)
            .and_then(|r| r.str("itemSetName").map(str::to_owned))
            .unwrap_or_default()
    };
    let (none, same, random, sword) = with(g, |data, inv, stash, gold| {
        *gold = 0;
        let none = convert_item(data, item(head), false, true, 54, gold, inv, stash)
            .map(|_| ())
            .map_err(|(w, _)| w);
        *gold = 1_000_000;
        for n in [
            "records/items/crafting/materials/craft_eldritchessence.dbr",
            "records/items/materia/compa_aethercrystal.dbr",
            "records/items/materia/compa_polishedemerald.dbr",
        ] {
            for _ in 0..3 {
                inv.add_to_bags(item(n));
            }
        }
        let before = *gold;
        let in_bags = |inv: &Inventory| {
            inv.bags
                .iter()
                .flat_map(|b| b.items.iter())
                .filter(|p| set_of(data, &p.item.base_name).contains("itemset_c001"))
                .count()
        };
        let bagged = in_bags(inv);
        // The new piece goes into the tab's slot (`GiveSetItemToCharacter` -> the HUD's convert slot), never
        // the bags: full bags do not matter.
        let same =
            convert_item(data, item(head), false, true, 54, gold, inv, stash).map(|(_, made)| {
                (
                    (before - *gold, in_bags(inv) == bagged),
                    Some(made.base_name).filter(|m| set_of(data, m).contains("itemset_c001")),
                )
            });
        for _ in 0..5 {
            inv.add_to_bags(item(scrap));
        }
        for n in [
            "records/items/materia/compa_aethercrystal.dbr",
            "records/items/materia/compa_polishedemerald.dbr",
        ] {
            for _ in 0..3 {
                inv.add_to_bags(item(n));
            }
        }
        let random = convert_item(data, item(head), true, true, 54, gold, inv, stash).map(|_| ());
        let sword = convert_item(data, item(SWORD), false, true, 54, gold, inv, stash)
            .map(|_| ())
            .map_err(|(w, _)| w);
        (none, same, random, sword)
    });
    assert_eq!(none, Err(Refusal::Cost(120_000)));
    let ((paid, fitted), made) = same.expect("same-set convert");
    assert_eq!(paid, 120_000);
    assert!(
        fitted && made.is_some_and(|m| m != head),
        "the new piece is the slot's"
    );
    assert!(random.is_ok(), "random-set convert");
    assert_eq!(sword, Err(Refusal::NotSet));
}
