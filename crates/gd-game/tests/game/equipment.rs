//! Headless checks of equipment changes: the worn items are the player's
//! loadout, and an equipment change keeps the player entity (and its other
//! state) while the stats and skills follow.
use gd_game::combat::Combatant;
use gd_game::inventory::{EquipmentChanged, Inventory, Requirements, slot_index};
use gd_game::player::{EffectiveSkills, Loadout, Player};
use gd_game::testing::TestGame;
use gd_sim::items::inventory::EquipSlot;

/// A new character wearing a common amulet it can use, put on through `Inventory::give` (an empty slot takes it).
fn game() -> Option<(TestGame, bevy::prelude::Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    let p = g.spawn_player();
    // The inventory is created once the player exists.
    g.step();
    let db = g.data().db;
    let mut names: Vec<String> = db
        .names()
        .filter(|n| n.starts_with("records/items/gear"))
        .map(str::to_owned)
        .collect();
    names.sort();
    let req = *g.get::<Requirements>(p);
    let mut inv = g.app.world_mut().resource_mut::<Inventory>();
    let amulet = names
        .into_iter()
        .map(|n| gd_formats::save::item::Item {
            base_name: n,
            seed: 1,
            stack_count: 1,
            ..Default::default()
        })
        .find(|it| {
            db.get(&it.base_name).is_some_and(|r| {
                r.str("Class") == Some("ArmorJewelry_Amulet")
                    && r.str("itemClassification") == Some("Common")
            }) && inv.usable(it, &req)
        })
        .expect("a usable amulet");
    assert!(inv.give(amulet, false, &req).is_ok());
    g.step();
    Some((g, p))
}

/// The worn items are the loadout. Taking off the amulet: the loadout,
/// skill bonuses, profile and the dual-wield mask every equip check uses
/// follow on the same frame; the player stays the same entity with its life
/// fraction.
#[test]
fn unequip_updates_the_same_player() {
    let Some((mut g, p)) = game() else { return };
    let names = |items: &[gd_formats::save::item::Item]| {
        let mut v: Vec<_> = items.iter().map(|i| i.base_name.clone()).collect();
        v.sort();
        v
    };
    assert_eq!(
        names(&g.app.world().resource::<Inventory>().worn()),
        names(&g.get::<Loadout>(p).items)
    );
    let before_profile = g.get::<Combatant>(p).profile.clone();
    let removed = {
        let mut inv = g.app.world_mut().resource_mut::<Inventory>();
        let i = slot_index(EquipSlot::Amulet);
        let it = inv.equipped[i].take().expect("the amulet is worn");
        inv.mark_equipment_dirty();
        it
    };
    g.step();
    let players: Vec<_> = g
        .app
        .world_mut()
        .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<Player>>()
        .iter(g.app.world())
        .collect();
    assert_eq!(players, vec![p]);
    let loadout = g.get::<Loadout>(p);
    assert!(!loadout.items.contains(&removed));
    assert_eq!(
        loadout.items.len(),
        g.app.world().resource::<Inventory>().worn().len()
    );
    let cb = g.get::<Combatant>(p);
    assert!(
        !std::sync::Arc::ptr_eq(&cb.profile, &before_profile),
        "profile rebuilt"
    );
    let eff = g.get::<EffectiveSkills>(p);
    assert_eq!(
        eff.0,
        loadout.effective_skills(&g.data().db, &g.data().skill_trees())
    );
    let msgs = g
        .app
        .world()
        .resource::<bevy::ecs::message::Messages<EquipmentChanged>>();
    assert!(msgs.iter_current_update_messages().any(|m| m.player == p));
    let db = g.data().db;
    assert_eq!(
        g.get::<Requirements>(p).dual,
        g.get::<Loadout>(p).dual_mask(&db)
    );
}
