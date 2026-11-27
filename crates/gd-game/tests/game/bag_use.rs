//! Right click on a bag item through its secondary handler (`Item::CreateSecondaryCursorHandler`):
//! a potion is drunk at once (with the record's cooldown).
use bevy::prelude::*;
use gd_formats::inventory::Sack;
use gd_formats::save::item::Item;
use gd_game::testing::TestGame;
use gd_game_core::casting::Caster;
use gd_game_core::combat::Combatant;
use gd_game_core::inventory::{InvRequest, Inventory};

const POTION: &str = "records/items/misc/potions/_oldpotion_healtha01.dbr";

fn game() -> Option<(TestGame, Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 24.0);
    let p = g.spawn_player();
    g.steps(3);
    inv(&mut g).bags = vec![Sack::new(8, 8, true)];
    g.steps(2);
    Some((g, p))
}

fn inv(g: &mut TestGame) -> Mut<'_, Inventory> {
    g.app.world_mut().resource_mut::<Inventory>()
}

fn put(g: &mut TestGame, record: &str, stack: u32) {
    let it = Item {
        base_name: record.into(),
        stack_count: stack,
        seed: 5,
        ..Default::default()
    };
    assert!(inv(g).add_to_bags(it), "{record} fits");
}

/// The stack of `record` in the bag (0: gone).
fn count(g: &mut TestGame, record: &str) -> u32 {
    inv(g).bags[0]
        .items
        .iter()
        .filter(|p| p.item.base_name == record)
        .map(|p| p.item.stack_count)
        .sum()
}

fn right_click(g: &mut TestGame) {
    let cell = {
        let i = inv(g);
        let p = &i.bags[0].items[0];
        (p.x, p.y)
    };
    g.app.world_mut().write_message(InvRequest::Bag {
        bag: 0,
        cell,
        cursor: Vec2::ZERO,
        right: true,
        stash_page: None,
    });
    g.steps(2);
}

#[test]
fn potion_is_drunk_then_cools_down() {
    let Some((mut g, p)) = game() else { return };
    let max = g.get::<Combatant>(p).profile.sheet.life;
    g.get_mut::<Combatant>(p).life = max * 0.1;
    put(&mut g, POTION, 3);
    right_click(&mut g);
    assert_eq!(count(&mut g, POTION), 2, "one drunk");
    assert!(g.get::<Combatant>(p).life > max * 0.3, "25 % + 800 at once");
    let key = format!("scroll:{POTION}");
    assert!(g.get::<Caster>(p).cooldowns.contains_key(&key));
    // Cooling down: refused, kept.
    let life = g.get::<Combatant>(p).life;
    right_click(&mut g);
    assert_eq!(count(&mut g, POTION), 2);
    assert!(g.get::<Combatant>(p).life <= life + max * 0.05);
    assert!(inv(&mut g).held.is_none());
}
