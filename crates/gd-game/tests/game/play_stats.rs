//! Headless checks of the session's play stats (`gd_game::play_stats`): a boss kill, a one-shot chest and a
//! health potion count, and the counts are in the save the next session loads.
use bevy::prelude::*;
use gd_formats::save::gdc::Gdc;
use gd_formats::save::item::Item;
use gd_game::chests::spawn_chest;
use gd_game::combat::MonsterKilled;
use gd_game::inventory::{InvRequest, Inventory};
use gd_game::play_stats::PlayCounts;
use gd_game::session::{GameMode, LoadedCharacter};
use gd_game::testing::{TempDir, TestGame};
use gd_game::world::ChestKey;
use gd_sim::items::inventory::Sack;

const ONE_SHOT_CHEST: &str = "records/items/lootchests/oneshotchests/d01_oneshotchest_warden.dbr";
const POTION: &str = "records/items/misc/potions/_oldpotion_healtha01.dbr";

/// A session with a port-saved new character (its save in `tmp`), the player spawned.
fn game(tmp: &TempDir) -> Option<(TestGame, Entity)> {
    let path = tmp.path().join("player.gdc");
    let mut new = TestGame::new_character("Counter", false, false)?;
    new.spawn_player();
    std::fs::copy(new.save_now(), &path).unwrap();
    drop(new);
    let mut g = TestGame::with_save(&path)?;
    g.flat_ground(Vec3::ZERO, 24.0);
    let p = g.spawn_player();
    // The chest's level is loaded (else it is culled with it).
    let root = g.app.world_mut().spawn_empty().id();
    g.app
        .world_mut()
        .resource_mut::<gd_game::levels::Levels>()
        .add(2, root, Vec3::ZERO, None);
    g.steps(3);
    Some((g, p))
}

/// The first enemy record (sorted) that is a Boss and gives experience.
fn boss(g: &TestGame) -> String {
    let db = &g.data().db;
    let mut names: Vec<&str> = db
        .names()
        .filter(|n| n.starts_with("records/creatures/enemies/"))
        .collect();
    names.sort();
    names
        .into_iter()
        .find(|n| {
            db.get(n).is_some_and(|r| {
                r.str("monsterClassification") == Some("Boss")
                    && r.f32("giveXP").is_none_or(|v| v != 0.0)
            })
        })
        .expect("a boss")
        .to_owned()
}

fn counts(g: &TestGame) -> &PlayCounts {
    g.app.world().resource::<PlayCounts>()
}

#[test]
fn boss_chest_and_potion_count_and_survive_save_and_load() {
    let tmp = TempDir::new("gd-play-stats");
    let Some((mut g, player)) = game(&tmp) else {
        return;
    };
    let difficulty = g.app.world().resource::<GameMode>().0.difficulty;
    let before = g.app.world().resource::<LoadedCharacter>().0.stats.clone();

    // The player's boss kill: a boss kill of the difficulty, its experience, and the greatest monster (a
    // live body, so its life and mana are known).
    let record = boss(&g);
    let body = g.spawn_monster(&record, 100, Vec3::new(6.0, 0.0, 0.0));
    g.step();
    g.app.world_mut().write_message(MonsterKilled {
        record: record.clone(),
        level: 100,
        position: Vec3::new(6.0, 0.0, 0.0),
        entity: body,
        killer: Some((player, None)),
    });
    g.step();
    // Another's kill counts nothing.
    g.app.world_mut().write_message(MonsterKilled {
        record: record.clone(),
        level: 100,
        position: Vec3::ZERO,
        entity: Entity::PLACEHOLDER,
        killer: None,
    });
    g.step();
    assert_eq!(counts(&g).boss_kills[difficulty], 1);
    assert!(counts(&g).experience_from_kills > 0);
    let greatest = counts(&g).greatest[difficulty]
        .clone()
        .expect("a level 100 boss beats the stored greatest monster");
    assert_eq!(greatest.level, 100);

    // A one-shot chest.
    let data = g.data();
    let chest = {
        let world = g.app.world_mut();
        let e = spawn_chest(
            &mut world.commands(),
            &data.db,
            Transform::from_xyz(1.5, 0.0, 0.0),
            ONE_SHOT_CHEST,
            ChestKey::Level(2, 99),
            30,
            false,
        )
        .expect("chest");
        world.flush();
        // Its guards' lock (`LockedRadius`) is not what is tested.
        world.get_mut::<gd_game::chests::Chest>(e).unwrap().locked = false;
        e
    };
    g.interact(chest);
    g.step();
    assert_eq!(counts(&g).one_shot_chests_opened, 1);
    assert_eq!(counts(&g).hidden_chests_opened, 0);

    // A health potion drunk from the bag.
    {
        let mut inv = g.app.world_mut().resource_mut::<Inventory>();
        inv.bags = vec![Sack::new(8, 8, true)];
        assert!(inv.add_to_bags(Item {
            base_name: POTION.into(),
            stack_count: 1,
            seed: 5,
            ..Default::default()
        }));
    }
    g.step();
    let cell = {
        let inv = g.app.world().resource::<Inventory>();
        let p = &inv.bags[0].items[0];
        (p.x, p.y)
    };
    g.app.world_mut().write_message(InvRequest::Bag {
        bag: 0,
        cell,
        cursor: Vec2::ZERO,
        right: true,
    });
    g.steps(2);
    assert_eq!(counts(&g).health_potions_used, 1);
    assert_eq!(counts(&g).mana_potions_used, 0);

    // Saved, then loaded by the next session.
    let written = g.save_now();
    let saved = Gdc::parse(&std::fs::read(&written).unwrap()).unwrap().stats;
    assert_eq!(
        saved.boss_kills[difficulty],
        before.boss_kills[difficulty] + 1
    );
    assert_eq!(
        saved.one_shot_chests_opened,
        before.one_shot_chests_opened + 1
    );
    assert_eq!(saved.health_potions_used, before.health_potions_used + 1);
    assert!(saved.experience_from_kills > before.experience_from_kills);
    assert_eq!(
        saved.per_difficulty[difficulty].greatest_monster_killed_name,
        greatest.name
    );
    let Some(mut again) = TestGame::with_save(&written) else {
        return;
    };
    again.spawn_player();
    again.steps(2);
    let loaded = &again.app.world().resource::<LoadedCharacter>().0.stats;
    assert_eq!(loaded.boss_kills, saved.boss_kills);
    assert_eq!(loaded.one_shot_chests_opened, saved.one_shot_chests_opened);
    assert_eq!(loaded.health_potions_used, saved.health_potions_used);
}
