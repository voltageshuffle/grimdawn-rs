//! Headless check of the Illusionist's unlocks: a transmute set unlocks its looks into the account-wide
//! `transmutes.gst`, and they survive a save and a load.
use bevy::prelude::*;
use gd_game::illusion::Illusions;
use gd_game::testing::TestGame;
use gd_game_core::inventory::Inventory;

/// A transmute set (`ItemTransmuterSet::Use`): a right click uses it up, unlocks
/// every look of `transmuteDbrs`, writes `transmutes.gst` at once and sends the type-8 cinematic text; the looks
/// come back when the saved character is loaded again.
#[test]
fn transmute_set_unlocks_saves_and_reloads() {
    use bevy::ecs::message::Messages;
    use gd_formats::save::item::Item;
    use gd_game_core::inventory::InvRequest;
    use gd_game_core::story::CinematicText;
    const SET: &str = "records/items/transmutes/transmute_blackknight_set.dbr";
    const LOOK: &str = "records/items/transmutes/knight/dlc_blackknight_head.dbr";
    let Some(mut g) = TestGame::new_character("Illusionist", false, false) else {
        return;
    };
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    g.steps(3);
    let db = g.data().db;
    let looks: Vec<String> = db
        .get(SET)
        .unwrap()
        .strs("transmuteDbrs")
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(looks.len(), 8);
    let known = |g: &TestGame, r: &str| {
        let ill = g.app.world().resource::<Illusions>();
        ill.file
            .entries
            .iter()
            .any(|e| e.records.iter().any(|x| x.eq_ignore_ascii_case(r)))
    };
    // A new character starts without the set's looks.
    assert!(!known(&g, LOOK));
    assert!(
        g.app
            .world_mut()
            .resource_mut::<Inventory>()
            .add_to_bags(Item {
                base_name: SET.into(),
                stack_count: 1,
                seed: 4242,
                ..Default::default()
            })
    );
    g.step();
    let (bag, cell) = {
        let inv = g.app.world().resource::<Inventory>();
        inv.bags
            .iter()
            .enumerate()
            .find_map(|(b, s)| {
                s.items
                    .iter()
                    .find(|q| q.item.base_name == SET)
                    .map(|q| (b, (q.x, q.y)))
            })
            .expect("the set is in a bag")
    };
    let mut cursor = g
        .app
        .world_mut()
        .resource_mut::<Messages<CinematicText>>()
        .get_cursor();
    g.app.world_mut().write_message(InvRequest::Bag {
        bag,
        cell,
        cursor: Vec2::ZERO,
        right: true,
        stash_page: None,
    });
    // Messages live two frames: read them after each step.
    let mut texts: Vec<CinematicText> = Vec::new();
    for _ in 0..4 {
        g.step();
        let msgs = g.app.world().resource::<Messages<CinematicText>>();
        texts.extend(cursor.read(msgs).filter(|m| m.kind == 8).cloned());
    }
    assert_eq!(
        g.app.world().resource::<Inventory>().count_by_record(SET),
        0,
        "used up"
    );
    assert!(looks.iter().all(|l| known(&g, l)), "every look unlocked");
    assert_eq!(texts.len(), 1);
    assert_eq!(
        texts[0].head,
        "You have gained a new Illusion: Black Knight Armor"
    );
    assert!(texts[0].sub.is_empty() && !texts[0].front);
    // `SaveTransmutes` at once.
    let file = gd_formats::save::gst::Transmutes::parse(
        &std::fs::read(g.save_dir().join("transmutes.gst")).expect("transmutes.gst written"),
    )
    .unwrap();
    assert!(
        file.entries
            .iter()
            .any(|e| e.records.iter().any(|r| r.eq_ignore_ascii_case(LOOK)))
    );
    // Save the character and load it back: the looks are still unlocked (the first game keeps its save
    // folder alive).
    let saved = g.save_now();
    let Some(mut again) = TestGame::with_save(&saved) else {
        return;
    };
    again.flat_ground(Vec3::ZERO, 24.0);
    again.spawn_player();
    again.steps(3);
    assert!(
        looks.iter().all(|l| known(&again, l)),
        "unlocked after reload"
    );
}
