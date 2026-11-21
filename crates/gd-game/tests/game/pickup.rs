//! Giving the player an item (`Inventory::give`, `gd_game::loot::give_item`): an empty slot it fits takes it
//! first (never a swap; weapons into the active set with the hand rules), else the main bag, the selected bag,
//! the others; a full inventory throws it back down (soulbound: destroyed); Broken items are not picked up.
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game::inventory::{Inventory, Requirements, slot_index};
use gd_game::testing::TestGame;
use gd_sim::items::inventory::EquipSlot;

fn item(record: &str, seed: u32) -> Item {
    Item {
        base_name: record.into(),
        seed,
        stack_count: 1,
        ..Default::default()
    }
}

/// A new character on flat ground, the player at the origin, nothing worn, the bags empty.
fn game() -> Option<(TestGame, Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 24.0);
    let p = g.spawn_player();
    g.steps(3);
    let mut inv = g.app.world_mut().resource_mut::<Inventory>();
    inv.equipped = Default::default();
    inv.other_weapons = Default::default();
    inv.mark_equipment_dirty();
    for b in &mut inv.bags {
        b.items.clear();
    }
    g.steps(2);
    Some((g, p))
}

fn inv(g: &mut TestGame) -> Mut<'_, Inventory> {
    g.app.world_mut().resource_mut::<Inventory>()
}

/// [`Inventory::give`] with the player's requirements.
#[allow(clippy::result_large_err)]
fn give(g: &mut TestGame, it: Item) -> Result<(), Item> {
    let p = g.player();
    let req = *g.get::<Requirements>(p);
    inv(g).give(it, false, &req)
}

/// A base record of item class `class` the player can wear (requirements met).
fn record(g: &mut TestGame, class: &str) -> String {
    let db = g.data().db;
    let mut names: Vec<String> = db
        .names()
        .filter(|n| n.starts_with("records/items/gear"))
        .map(str::to_owned)
        .collect();
    names.sort();
    let p = g.player();
    let req = *g.get::<Requirements>(p);
    let inv = inv(g);
    names
        .into_iter()
        .find(|n| {
            db.get(n).is_some_and(|r| {
                r.str("Class") == Some(class)
                    && r.str("itemClassification") == Some("Common")
                    && !r.bool("soulbound")
            }) && inv.info(&item(n, 1)).is_some()
                && inv.usable(&item(n, 1), &req)
        })
        .unwrap_or_else(|| panic!("a usable {class}"))
}

fn worn(g: &mut TestGame, s: EquipSlot) -> Option<String> {
    inv(g).equipped[slot_index(s)]
        .as_ref()
        .map(|i| i.base_name.clone())
}

fn in_bags(g: &mut TestGame, record: &str) -> Vec<usize> {
    let inv = inv(g);
    (0..inv.bags.len())
        .filter(|&b| inv.bags[b].items.iter().any(|p| p.item.base_name == record))
        .collect()
}

/// An empty slot takes the item; a second one does not replace it (bags). Rings: ring 1, then ring 2.
#[test]
fn auto_equips_into_an_empty_slot_never_swaps() {
    let Some((mut g, _)) = game() else { return };
    let head = record(&mut g, "ArmorProtective_Head");
    assert!(give(&mut g, item(&head, 1)).is_ok());
    assert_eq!(
        worn(&mut g, EquipSlot::Head).as_deref(),
        Some(head.as_str())
    );
    assert!(give(&mut g, item(&head, 2)).is_ok());
    assert_eq!(
        inv(&mut g).equipped[slot_index(EquipSlot::Head)]
            .as_ref()
            .unwrap()
            .seed,
        1
    );
    assert_eq!(in_bags(&mut g, &head), vec![0]);
    let ring = record(&mut g, "ArmorJewelry_Ring");
    for seed in 1..=3 {
        assert!(give(&mut g, item(&ring, seed)).is_ok());
    }
    let seed = |g: &mut TestGame, s| inv(g).equipped[slot_index(s)].as_ref().map(|i| i.seed);
    assert_eq!(seed(&mut g, EquipSlot::Ring1), Some(1));
    assert_eq!(seed(&mut g, EquipSlot::Ring2), Some(2));
    assert_eq!(in_bags(&mut g, &ring), vec![0]);
}
