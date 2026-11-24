//! The Inventor's windows only write requests (`CraftRequest`,
//! `RerollRequest`, ...); `gd_game::crafting` / `reroll` systems apply them.
//! Same rules as `crafting.rs` checks by calling the functions; here the
//! messages go through the schedule.
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game::crafting::{CraftRequest, SlotReturn};
use gd_game::testing::{TestGame, first_blueprint};
use gd_game_core::inventory::Inventory;
use gd_game_core::progress::PlayerProgress;
use gd_game_core::stash::Stash;

fn item(rec: &str) -> Item {
    Item {
        base_name: rec.into(),
        stack_count: 1,
        seed: 7,
        ..Default::default()
    }
}

/// The `SlotReturn`s written since the last call.
fn returns(g: &mut TestGame) -> Vec<SlotReturn> {
    g.app
        .world_mut()
        .resource_mut::<Messages<SlotReturn>>()
        .drain()
        .collect()
}

#[test]
fn craft_request_pays_and_takes_reagents() {
    let Some(mut g) = TestGame::new_character("Tester", false, false) else {
        return;
    };
    g.spawn_player();
    g.steps(2);
    let db = g.data().db;
    let bp = first_blueprint(&db);
    let level = g
        .app
        .world()
        .resource::<PlayerProgress>()
        .progress
        .level
        .max(1) as u32;
    // Provide the money and the reagents (and none of the others' leftovers).
    let w = g.app.world_mut();
    w.resource_scope(|w, mut inv: Mut<Inventory>| {
        let stash = w.resource::<Stash>();
        for (names, _) in &bp.reagents {
            let n = gd_game_core::stash::count(names, &inv, stash);
            inv.remove_by_record(&names[0], n);
        }
        for (names, n) in &bp.reagents {
            for _ in 0..*n {
                inv.add_to_bags(item(&names[0]));
            }
        }
        w.resource_mut::<PlayerProgress>().gold = bp.cost(level) + 5;
    });
    let version = g.app.world().resource::<Inventory>().version;
    g.app.world_mut().write_message(CraftRequest {
        bp: bp.clone(),
        enhancement: None,
    });
    g.steps(2);
    let w = g.app.world();
    assert_eq!(w.resource::<PlayerProgress>().gold, 5, "the cost was paid");
    let left: u32 = bp
        .reagents
        .iter()
        .map(|(names, _)| gd_game_core::stash::count(names, w.resource(), w.resource()))
        .sum();
    assert_eq!(left, 0, "reagents taken");
    assert!(
        w.resource::<Inventory>().version > version,
        "the window rebuilds"
    );
    assert_eq!(returns(&mut g).len(), 1, "the window is told");
}
