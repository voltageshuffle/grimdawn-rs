//! The main menu's character creation path: a new character (no save) plays, saves and loads back with its
//! name, sex and hardcore flag; what every save writes (one identity per character, the hardcore shared files).
use gd_formats::save::gdc::{Gdc, HotSlot};
use gd_formats::save::gst::Transfer;
use gd_game::testing::TestGame;
use gd_game_core::casting::{DEFAULT_ATTACK, SLOT_LEFT};

fn read_gdc(path: &std::path::Path) -> Gdc {
    Gdc::parse(&std::fs::read(path).expect("player.gdc written")).expect("parses")
}

/// A new character is written as soon as it plays (`Player::ForceSave` on creation, no request), and every
/// later save keeps its UID and seed. A new transfer stash carries the expansion flags 7.
#[test]
fn new_character_written_at_once_with_one_identity() {
    let Some(mut g) = TestGame::new_character("One Uid", false, false) else {
        return;
    };
    let dir = g.save_dir();
    g.spawn_player();
    g.steps(3);
    let path = dir.join("main").join("_One Uid").join("player.gdc");
    let first = read_gdc(&path);
    g.save_now();
    let second = read_gdc(&path);
    assert_eq!((first.uid, first.seed), (second.uid, second.seed));
    let t = Transfer::parse(&std::fs::read(dir.join("transfer.gst")).unwrap()).unwrap();
    assert_eq!(t.expansion_status, 7);
    assert!(!dir.join("transfer.gsh").exists());
}

/// A hardcore character's account-wide files are the `.gsh` twins (`GetSharedSavePath`): it never shares a
/// transfer stash, blueprints or reagents with the softcore characters.
#[test]
fn hardcore_shared_files_are_separate() {
    let Some(mut g) = TestGame::new_character("Hc Shared", false, true) else {
        return;
    };
    let dir = g.save_dir();
    g.spawn_player();
    g.steps(3);
    for f in ["transfer", "formulas", "reagents"] {
        assert!(dir.join(format!("{f}.gsh")).exists(), "{f}.gsh written");
        assert!(!dir.join(format!("{f}.gst")).exists(), "no {f}.gst");
    }
    assert!(!dir.join("transmutes.gst").exists());
    let t = Transfer::parse(&std::fs::read(dir.join("transfer.gsh")).unwrap()).unwrap();
    assert_eq!(t.expansion_status, 7);
}

#[test]
fn created_character_saves_and_loads() {
    let Some(mut g) = TestGame::new_character("Menu Test", true, true) else {
        return;
    };
    g.spawn_player();
    g.steps(3);
    let path = g.save_now();
    let saved = read_gdc(&path);
    assert_eq!(saved.header.name, "Menu Test");
    assert_eq!(saved.header.sex, 0, "female");
    assert_ne!(saved.header.hardcore, 0, "hardcore");
    assert_eq!((saved.bio.level, saved.info.difficulty()), (1, 0));
    // The game's new hotbar: the default attack on both sets' left mouse
    // slots (10, 11) and slot 26 (`Gdc::new_character`).
    let attack = |i: usize| matches!(saved.ui.bars.first().and_then(|b| b.slots.get(i)), Some(HotSlot::Skill { skill, .. }) if skill == DEFAULT_ATTACK);
    assert!(attack(SLOT_LEFT) && attack(SLOT_LEFT + 1) && attack(26));
    // It plays again from the file, as the menu's Play does.
    let again = TestGame::with_save(&path).expect("loads");
    let loaded = &again
        .app
        .world()
        .resource::<gd_game_core::session::LoadedCharacter>()
        .0;
    assert_eq!(loaded.header.name, "Menu Test");
}

/// A new character survives an encode / parse, also after editing block 10 (the Crucible's tokens, one
/// list per difficulty).
#[test]
fn new_character_round_trips() {
    for (female, hardcore) in [(false, false), (true, true)] {
        let mut g = gd_game::save::new_character("Ωmega", female, hardcore, [7; 16], 12345);
        let h = &g.header;
        assert_eq!((h.sex, h.hardcore), (u8::from(!female), u8::from(hardcore)));
        assert_eq!(Gdc::parse(&g.encode().unwrap()).unwrap(), g);
        g.tokens.lists[1] = vec!["SURVIVALMODE_X".into(), "Y".into()];
        assert_eq!(Gdc::parse(&g.encode().unwrap()).unwrap(), g);
    }
}

/// A new character's stash sack is written 0×0 (as the engine writes it) and loads back as a full-size
/// first personal page; a 1×1 personal page is healed to the fixed personal size on load.
#[test]
fn new_character_stash_page_is_full_size() {
    use gd_game::testing::TempDir;
    use gd_game_core::stash::{STASH_H, STASH_W, Stash};
    let tmp = TempDir::new("stash_page");
    let mut gdc = gd_game::save::new_character("Stash Size", false, false, [3; 16], 99);
    assert_eq!(
        (gdc.stash.sacks[0].width, gdc.stash.sacks[0].height),
        (0, 0),
        "the engine writes a fresh stash sack 0×0"
    );
    let path = tmp.path().join("player.gdc");
    std::fs::write(&path, gdc.encode().unwrap()).unwrap();
    let page = |g: &TestGame| {
        let s = g.app.world().resource::<Stash>();
        (s.pages[0].width, s.pages[0].height)
    };
    // Loading the 0×0 sack gives the default page size.
    let Some(mut g) = TestGame::with_save(&path) else {
        return;
    };
    g.spawn_player();
    g.steps(2);
    assert_eq!(page(&g), (STASH_W, STASH_H));
    // A 1×1 personal page is healed on load.
    gdc.stash.sacks[0].width = 1;
    gdc.stash.sacks[0].height = 1;
    let bad = tmp.path().join("bad.gdc");
    std::fs::write(&bad, gdc.encode().unwrap()).unwrap();
    let Some(mut g2) = TestGame::with_save(&bad) else {
        return;
    };
    g2.spawn_player();
    g2.steps(2);
    assert_eq!(page(&g2), (STASH_W, STASH_H));
}
