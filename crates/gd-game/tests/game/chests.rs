//! Loot chests: opening one in reach drops loot once, plays its open sound and FX and is remembered for the
//! session; locked ones do not open.
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use gd_game::chests::{Chest, spawn_chest};
use gd_game::loot::GroundItem;
use gd_game::testing::TestGame;
use gd_game_core::visuals::{EffectSource, EffectSpec, PlaySound};
use gd_game_core::world::{ChestKey, ChestStates};

const CHEST: &str = "records/items/lootchests/b01_chestmetal01.dbr";

fn game() -> Option<TestGame> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    // The chests' level is loaded (else they are culled with it).
    let root = g.app.world_mut().spawn_empty().id();
    g.app
        .world_mut()
        .resource_mut::<gd_game_core::levels::Levels>()
        .add(2, root, Vec3::ZERO, None);
    g.step();
    Some(g)
}

fn chest(g: &mut TestGame, key: ChestKey, locked: bool) -> Entity {
    let data = g.data();
    let world = g.app.world_mut();
    let e = spawn_chest(
        &mut world.commands(),
        &data.db,
        Transform::from_xyz(1.5, 0.0, 0.0),
        CHEST,
        key,
        30,
        false,
    )
    .expect("chest");
    world.flush();
    world.get_mut::<Chest>(e).unwrap().locked = locked;
    e
}

fn ground(g: &mut TestGame) -> usize {
    g.app
        .world_mut()
        .query::<&GroundItem>()
        .iter(g.app.world())
        .count()
}

/// A locked chest stays shut; an open one drops its loot once.
#[test]
fn open_chest_drops_loot_once() {
    let Some(mut g) = game() else { return };
    let locked = chest(&mut g, ChestKey::Level(2, 98), true);
    g.interact(locked);
    g.step();
    assert!(!g.get::<Chest>(locked).opened, "the locked chest opened");
    assert_eq!(ground(&mut g), 0);
    let key = ChestKey::Level(2, 99);
    let c = chest(&mut g, key, false);
    g.interact(c);
    g.step();
    assert!(g.get::<Chest>(c).opened);
    assert!(
        g.app
            .world()
            .resource::<ChestStates>()
            .opened
            .contains(&key)
    );
    let dropped = ground(&mut g);
    assert!(dropped > 0, "a metal chest dropped nothing");
    // `FixedItemContainer::Open`: `openSound` at the chest, `openFxPakName` there.
    let sounds: Vec<_> = g
        .app
        .world()
        .resource::<Messages<PlaySound>>()
        .iter_current_update_messages()
        .cloned()
        .collect();
    assert!(
        sounds
            .iter()
            .any(|s| s.pak.ends_with("spak_lootchestwood_a01.dbr")
                && s.at
                    .is_some_and(|a| a.distance(Vec3::new(1.5, 0.0, 0.0)) < 0.01)),
        "{sounds:?}"
    );
    let fx: Vec<_> = g
        .app
        .world_mut()
        .query::<(&EffectSpec, &Transform)>()
        .iter(g.app.world())
        .map(|(s, t)| (s.source.clone(), t.translation))
        .collect();
    assert!(
        fx.iter().any(
            |(s, t)| matches!(s, EffectSource::Record(r) if r.ends_with("chestopen01_fx.dbr"))
                && t.distance(Vec3::new(1.5, 0.0, 0.0)) < 0.01
        ),
        "{fx:?}"
    );
    g.interact(c);
    g.step();
    assert!(ground(&mut g) <= dropped, "opened twice");
}
