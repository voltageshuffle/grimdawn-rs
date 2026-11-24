//! Headless checks of ground loot: gold is picked up within 4 m, a targeted
//! item is walked to and put in the bags, kills drop loot.
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_game::loot::{GroundItem, drop_loot};
use gd_game::movement::MovePath;
use gd_game::nav::Nav;
use gd_game::player_orders::InteractTarget;
use gd_game::testing::TestGame;
use gd_game_core::inventory::Inventory;
use gd_game_core::progress::PlayerProgress;

fn game() -> Option<(TestGame, Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 24.0);
    let p = g.spawn_player();
    g.step();
    Some((g, p))
}

fn drop(g: &mut TestGame, at: Vec3, items: Vec<Item>, gold: &[u32]) {
    let world = g.app.world_mut();
    let idb =
        gd_sim::items::ItemDb::new(world.resource::<gd_game_core::data::GameData>().db.clone());
    {
        let mut commands = world.commands();
        drop_loot(&mut commands, &idb, at, items, gold);
    }
    world.flush();
}

fn ground(g: &mut TestGame) -> Vec<(Entity, Vec3)> {
    g.app
        .world_mut()
        .query_filtered::<(Entity, &Transform), With<GroundItem>>()
        .iter(g.app.world())
        .map(|(e, t)| (e, t.translation))
        .collect()
}

/// Gold within 4 m is picked up; 12 m away it stays.
#[test]
fn gold_nearby_is_picked_up() {
    let Some((mut g, _)) = game() else { return };
    let before = g.progress().gold;
    drop(&mut g, Vec3::new(1.0, 0.0, 0.0), Vec::new(), &[25]);
    // The scan runs every 50 ms (`Player::kAutoPickupPeriod`), from the
    // release (250 ms) and the pick-up delay (750 ms) on.
    for _ in 0..150 {
        g.step();
        if g.progress().gold != before {
            break;
        }
    }
    assert_eq!(g.app.world().resource::<PlayerProgress>().gold, before + 25);
    assert!(ground(&mut g).is_empty());
    // "+25 Iron" floats up from the player (`Character::TakeBonus`, event 27).
    let texts: Vec<_> = g
        .app
        .world()
        .resource::<Messages<gd_game::floating::FloatingText>>()
        .iter_current_update_messages()
        .cloned()
        .collect();
    assert_eq!(texts.len(), 1);
    assert_eq!(
        (
            texts[0].tag.as_str(),
            texts[0].args.as_slice(),
            texts[0].flags
        ),
        (
            "GiveMoneyFormat",
            &[gd_game_core::floating::TextArg::Num(25.0)][..],
            28
        )
    );
    assert_eq!(
        texts[0].style,
        "records/ui/styles/text/style_special_moneypickup.dbr"
    );
    let before = g.progress().gold;
    drop(&mut g, Vec3::new(12.0, 0.0, 0.0), Vec::new(), &[25]);
    g.steps(60);
    assert_eq!(g.progress().gold, before, "far gold stays");
    assert_eq!(ground(&mut g).len(), 1);
}

/// Clicking an item (its target + a path) walks over and bags it.
#[test]
fn targeted_item_goes_to_the_bags() {
    let Some((mut g, p)) = game() else { return };
    let rec = "records/items/gearweapons/swords1h/a00_sword001.dbr";
    drop(
        &mut g,
        Vec3::new(8.0, 0.0, 0.0),
        vec![Item {
            base_name: rec.into(),
            stack_count: 1,
            seed: 1,
            ..Default::default()
        }],
        &[],
    );
    // Thrown: lands after ~0.9 s.
    g.steps(60);
    let (e, at) = ground(&mut g)[0];
    let have = g.app.world().resource::<Inventory>().count_by_record(rec);
    let mut scratch = gd_sim::nav::PathScratch::default();
    let path = g
        .app
        .world()
        .resource::<Nav>()
        .0
        .find_path(&mut scratch, [0.0, 0.0, 0.0], at.to_array(), 60_000)
        .expect("path");
    g.get_mut::<InteractTarget>(p).0 = Some(e);
    g.get_mut::<MovePath>(p).points = path.into_iter().skip(1).map(Vec3::from).collect();
    let done = g.run_until(600, |w| w.get_entity(e).is_err());
    assert!(done.is_some(), "never picked up");
    assert_eq!(
        g.app.world().resource::<Inventory>().count_by_record(rec),
        have + 1
    );
}

/// Killed monsters drop their loot table's items (over enough kills).
#[test]
fn kills_drop_loot() {
    let Some((mut g, _)) = game() else { return };
    for i in 0..20 {
        let z = g.spawn_monster(
            "records/creatures/enemies/humanoutlaw_melee_b02.dbr",
            30,
            Vec3::new(15.0, 0.0, i as f32 - 10.0),
        );
        g.app
            .world_mut()
            .write_message(gd_game::combat::MonsterKilled {
                record: "records/creatures/enemies/humanoutlaw_melee_b02.dbr".into(),
                level: 30,
                position: Vec3::new(15.0, 0.0, i as f32 - 10.0),
                entity: z,
                killer: None,
            });
        g.step();
    }
    assert!(!ground(&mut g).is_empty(), "20 outlaws dropped nothing");
}
