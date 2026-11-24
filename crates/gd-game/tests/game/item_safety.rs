//! No path loses an item: a save writes the cursor item, the window boxes' items and an empty extra bag
//! (`save.rs`), a load keeps an item whose saved cell is taken or outside the grid (`inventory::place_saved`),
//! and an unreadable shared file is set aside instead of written over (`stash::read_shared`).
use bevy::prelude::*;
use gd_formats::inventory::{Footprint, Sack};
use gd_formats::save::gdc::{Gdc, SackItem};
use gd_formats::save::item::Item;
use gd_game::save::{BoxedItems, SaveRequest};
use gd_game::testing::TestGame;
use gd_game_core::inventory::Inventory;
use gd_game_core::stash::{STASH_H, STASH_W, Stash};
use std::path::PathBuf;

const SWORD: &str = "records/items/gearweapons/swords1h/a01_sword001.dbr";

fn item(seed: u32) -> Item {
    Item {
        base_name: SWORD.into(),
        seed,
        stack_count: 1,
        ..Default::default()
    }
}

/// A new character in play, and where its `player.gdc` is saved.
fn game(name: &str) -> Option<(TestGame, PathBuf)> {
    let mut g = TestGame::new_character(name, false, false)?;
    g.spawn_player();
    g.steps(3);
    let path = g
        .save_dir()
        .join("main")
        .join(format!("_{name}"))
        .join("player.gdc");
    Some((g, path))
}

fn inv(g: &mut TestGame) -> Mut<'_, Inventory> {
    g.app.world_mut().resource_mut::<Inventory>()
}

fn save(g: &mut TestGame, path: &std::path::Path) -> Gdc {
    g.app.world_mut().write_message(SaveRequest);
    g.step();
    Gdc::parse(&std::fs::read(path).expect("player.gdc written")).expect("the save parses")
}

/// The seeds in each inventory sack of a saved character.
fn sack_seeds(saved: &Gdc) -> Vec<Vec<u32>> {
    let data = saved.inventory.data.as_ref().expect("inventory block");
    data.sacks
        .iter()
        .map(|s| s.items.iter().map(|i| i.item.seed).collect())
        .collect()
}

#[test]
fn every_save_writes_the_items_outside_the_grids() {
    let Some((mut g, path)) = game("Loose Test") else {
        return;
    };
    // The cursor item, a window box's item and a new empty extra bag.
    {
        let mut inv = inv(&mut g);
        inv.bags = vec![Sack::new(12, 8, true), Sack::new(8, 8, true)];
        inv.held = Some(item(7));
    }
    g.app.world_mut().insert_resource(BoxedItems(vec![item(8)]));
    let saved = save(&mut g, &path);
    assert_eq!(sack_seeds(&saved), [vec![7, 8], vec![]]);
    // The save takes nothing off the cursor and puts nothing into the session's bags.
    assert_eq!(inv(&mut g).held, Some(item(7)));
    assert!(inv(&mut g).bags.iter().all(|b| b.items.is_empty()));
    // Full bags: a personal stash page takes them.
    {
        let mut inv = inv(&mut g);
        assert!(inv.bags[0].add_at(item(1), 0, 0, Footprint { w: 12, h: 8 }));
        assert!(inv.bags[1].add_at(item(2), 0, 0, Footprint { w: 8, h: 8 }));
    }
    let saved = save(&mut g, &path);
    assert_eq!(sack_seeds(&saved), [vec![1], vec![2]]);
    let stashed = |saved: &Gdc| -> Vec<u32> {
        saved
            .stash
            .sacks
            .iter()
            .flat_map(|s| s.items.iter().map(|i| i.item.seed))
            .collect()
    };
    assert_eq!(stashed(&saved), [7, 8]);
    // Nothing has room: they are written into the main bag's sack all the same, and the load finds them a
    // cell (the items' real footprints leave room).
    {
        let mut stash = g.app.world_mut().resource_mut::<Stash>();
        let whole = Footprint {
            w: STASH_W,
            h: STASH_H,
        };
        for (i, page) in stash.pages.iter_mut().enumerate() {
            assert!(page.add_at(item(100 + i as u32), 0, 0, whole));
        }
    }
    let saved = save(&mut g, &path);
    assert_eq!(sack_seeds(&saved), [vec![1, 7, 8], vec![2]]);
    assert!(stashed(&saved).iter().all(|s| *s >= 100));
    let Some(mut again) = TestGame::with_save(&path) else {
        return;
    };
    again.spawn_player();
    again.steps(3);
    let loaded = inv(&mut again);
    assert_eq!(loaded.bags.len(), 2);
    let mut seeds: Vec<u32> = loaded
        .bags
        .iter()
        .flat_map(|b| b.items.iter().map(|p| p.item.seed))
        .collect();
    seeds.sort();
    assert_eq!(seeds, [1, 2, 7, 8]);
    assert!(loaded.unplaced.is_empty());
}

#[test]
fn load_keeps_items_whose_saved_cell_is_unusable() {
    let Some((mut g, path)) = game("Collide Test") else {
        return;
    };
    let size = inv(&mut g).info(&item(1)).expect("the sword resolves").size;
    // The main bag tiled with swords: nothing more fits.
    let mut full = Sack::new(12, 8, true);
    let mut seed = 0;
    while full.add(item(seed), size).is_some() {
        seed += 1;
    }
    let tiled = full.items.len();
    inv(&mut g).bags = vec![Sack::new(12, 8, true)];
    inv(&mut g).bags[0].add_at(item(1), 0, 0, size);
    let mut saved = save(&mut g, &path);
    // A second item on the first one's cell and one far outside the grid (an edited or damaged save).
    let sack = &mut saved.inventory.data.as_mut().unwrap().sacks[0];
    sack.items.push(SackItem {
        item: item(2),
        x: 0,
        y: 0,
    });
    sack.items.push(SackItem {
        item: item(3),
        x: u32::MAX,
        y: u32::MAX,
    });
    std::fs::write(&path, saved.encode().unwrap()).unwrap();
    let Some(mut again) = TestGame::with_save(&path) else {
        return;
    };
    again.spawn_player();
    again.steps(3);
    {
        let loaded = inv(&mut again);
        let at = |seed: u32| {
            let p = loaded.bags[0].items.iter().find(|p| p.item.seed == seed);
            p.map(|p| (p.x, p.y))
        };
        assert_eq!(at(1), Some((0, 0)));
        assert!(at(2).is_some() && at(3).is_some(), "moved to a free cell");
        assert!(loaded.unplaced.is_empty());
    }
    // A full bag with one more item on a taken cell: kept out of the grid, and saved back.
    let sack = &mut saved.inventory.data.as_mut().unwrap().sacks[0];
    sack.items = full
        .items
        .iter()
        .map(|p| SackItem {
            item: p.item.clone(),
            x: p.x,
            y: p.y,
        })
        .collect();
    sack.items.push(SackItem {
        item: item(9000),
        x: 0,
        y: 0,
    });
    std::fs::write(&path, saved.encode().unwrap()).unwrap();
    let Some(mut again) = TestGame::with_save(&path) else {
        return;
    };
    again.spawn_player();
    again.steps(3);
    {
        let loaded = inv(&mut again);
        assert_eq!(loaded.bags[0].items.len(), tiled);
        let kept: Vec<u32> = loaded.unplaced.iter().map(|u| u.item.seed).collect();
        assert_eq!(kept, [9000]);
    }
    let out = again
        .save_dir()
        .join("main")
        .join("_Collide Test")
        .join("player.gdc");
    let resaved = save(&mut again, &out);
    let seeds = &sack_seeds(&resaved)[0];
    assert_eq!(seeds.len(), tiled + 1);
    assert!(seeds.contains(&9000));
}

#[test]
fn unreadable_shared_file_is_not_replaced() {
    const FILES: [&str; 4] = [
        "transfer.gst",
        "reagents.gst",
        "formulas.gst",
        "transmutes.gst",
    ];
    const DAMAGED: &[u8] = b"damaged shared file";
    let Some((mut g, path)) = game("Shared Test") else {
        return;
    };
    let dir = g.save_dir();
    let bak = |f: &str| dir.join(format!("{f}.bak"));
    // No file yet: new empty ones are written, nothing is set aside.
    save(&mut g, &path);
    for f in FILES {
        assert!(dir.join(f).exists(), "{f} written");
        assert!(!bak(f).exists(), "{f}: nothing to keep");
    }
    // The files are damaged, then the session's shared state loads again.
    for f in FILES {
        std::fs::write(dir.join(f), DAMAGED).unwrap();
    }
    g.app.world_mut().remove_resource::<Stash>();
    g.app
        .world_mut()
        .remove_resource::<gd_game::illusion::Illusions>();
    g.steps(2);
    save(&mut g, &path);
    for f in FILES {
        assert_eq!(
            std::fs::read(bak(f)).ok().as_deref(),
            Some(DAMAGED),
            "{f}: the unreadable file is kept"
        );
        assert_ne!(
            std::fs::read(dir.join(f)).unwrap(),
            DAMAGED,
            "{f} saved anew"
        );
    }
    gd_formats::save::gst::Transfer::parse(&std::fs::read(dir.join("transfer.gst")).unwrap())
        .expect("the new transfer.gst parses");
}
