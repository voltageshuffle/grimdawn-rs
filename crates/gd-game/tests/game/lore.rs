//! The lore codex (`gd_game::lore`): collected notes come from `player.gdc`
//! block 12, non-note records are dropped, and the list goes back into the save.
use bevy::prelude::*;
use gd_formats::save::gdc::Gdc;
use gd_game::lore::LoreCodex;
use gd_game::testing::{TempDir, TestGame};

const NEW_NOTE: &str = "records/items/loreobjects/loreobject_aetherialcity_02.dbr";

#[test]
fn lore_codex_loads_cleans_and_saves() {
    let Some(mut new) = TestGame::new_character("Lore Keeper", false, false) else {
        return;
    };
    new.spawn_player();
    let mut src = Gdc::parse(&std::fs::read(new.save_now()).unwrap()).unwrap();
    // Three collected notes (by record name: the name index is unordered).
    let data = new.data();
    drop(new);
    let mut notes: Vec<String> = data
        .db
        .names()
        .filter(|n| n.starts_with("records/items/loreobjects/") && *n != NEW_NOTE)
        .filter(|n| gd_game::lore::is_note(&data.db, n))
        .map(str::to_owned)
        .collect();
    notes.sort_unstable();
    notes.truncate(3);
    assert_eq!(notes.len(), 3, "the database has lore notes");
    // A record that is not a note is dropped at load (`CleanLoreCodex`).
    let bogus = "records/items/lootchests/chestloottables/a01_moneygenerator.dbr".to_string();
    src.notes.notes = std::iter::once(bogus.clone()).chain(notes).collect();
    let tmp = TempDir::new("gd-lore-test");
    let edited = tmp.path().join("player.gdc");
    std::fs::write(&edited, src.encode().unwrap()).unwrap();
    let mut g = TestGame::with_save(&edited).expect("the game is installed");
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    g.step();
    let codex = g.app.world().resource::<LoreCodex>().clone();
    assert!(
        !codex.notes.contains(&bogus)
            && codex
                .notes
                .iter()
                .all(|n| gd_game::lore::is_note(&data.db, n))
    );
    assert_eq!(
        codex.notes[..],
        src.notes.notes[1..],
        "the save's notes in order"
    );
    // A collected note goes back into the save.
    g.app
        .world_mut()
        .resource_mut::<LoreCodex>()
        .notes
        .push(NEW_NOTE.into());
    let written = g.save_now();
    let saved = Gdc::parse(&std::fs::read(&written).expect("player.gdc written")).unwrap();
    assert_eq!(saved.notes.notes.len(), codex.notes.len() + 1);
    assert_eq!(saved.notes.notes.last().map(String::as_str), Some(NEW_NOTE));
}
