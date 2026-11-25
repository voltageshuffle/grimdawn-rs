//! Headless check of saving: a save request writes the character, and with
//! nothing changed it equals the loaded one except for the potion skills
//! every character gets (`gd_sim::potions::ensure_skills`), and two saves
//! are byte-identical.
use gd_formats::save::gdc::Gdc;
use gd_game::testing::{TempDir, TestGame};
use gd_game_core::save::DIFFICULTY_DIRS;

/// A port-written save of a new character with some money and experience, in `tmp`.
fn saved_character(tmp: &TempDir) -> Option<std::path::PathBuf> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.spawn_player();
    let src = g.save_now();
    let mut gdc = Gdc::parse(&std::fs::read(src).unwrap()).unwrap();
    gdc.info.money = 12_345;
    gdc.bio.experience = 77;
    let path = tmp.path().join("player.gdc");
    std::fs::write(&path, gdc.encode().unwrap()).unwrap();
    Some(path)
}

#[test]
fn unchanged_character_round_trips() {
    let tmp = TempDir::new("save_round_trip");
    let Some(path) = saved_character(&tmp) else {
        return;
    };
    let Some(mut g) = TestGame::with_save(&path) else {
        return;
    };
    g.spawn_player();
    g.steps(3);
    let out = g.save_now();
    let written = std::fs::read(&out).expect("player.gdc written");
    // Saving again writes the same bytes (no run-to-run variation).
    let again = std::fs::read(g.save_now()).unwrap();
    assert!(written == again, "two saves differ");
    let original = Gdc::parse(&std::fs::read(&path).unwrap()).unwrap();
    let saved = Gdc::parse(&written).unwrap();
    let blank = |n: &str| n.contains("/potionmodifiers/") && n.ends_with("_blank.dbr");
    let skills = |g: &Gdc| {
        g.skills
            .skills
            .iter()
            .filter(|s| !blank(&s.name))
            .map(|s| (s.name.clone(), s.level))
            .collect::<Vec<_>>()
    };
    assert_eq!(skills(&saved), skills(&original));
    assert_eq!(
        (saved.bio.level, saved.bio.experience, saved.info.money),
        (
            original.bio.level,
            original.bio.experience,
            original.info.money
        )
    );
    assert_eq!(saved.header.name, original.header.name);
}

/// A Crucible session saves its tokens into the `player.gdc` survival vector of the difficulty (shared by the
/// arenas), keeps the arena's `quests.gdd` token-free, and a campaign session leaves the vectors alone.
#[test]
fn crucible_tokens_go_to_the_character() {
    use gd_game_core::session::{Choice, LoadedCharacter};
    use gd_game_core::story::StoryRuntime;
    let tmp = TempDir::new("save_crucible_tokens");
    let Some(path) = saved_character(&tmp) else {
        return;
    };
    let Some(mut g) = TestGame::with_save(&path) else {
        return;
    };
    g.spawn_player();
    g.steps(3);
    let (d, before) = {
        let c = &g.app.world().resource::<LoadedCharacter>().0;
        (c.info.difficulty(), c.tokens.clone())
    };
    let save = |g: &mut TestGame| {
        let player = g.save_now();
        let tokens = Gdc::parse(&std::fs::read(&player).unwrap()).unwrap().tokens;
        (player, tokens)
    };
    let grant = |g: &mut TestGame| {
        let rt = g.app.world().non_send::<StoryRuntime>();
        rt.story
            .borrow_mut()
            .player
            .tokens
            .insert("CRUCIBLE_TEST".into());
    };
    grant(&mut g);
    assert_eq!(save(&mut g).1, before, "campaign tokens stay in quests.gdd");
    g.app.world_mut().resource_mut::<Choice>().survival = Some("maps/survivalworld_c.map".into());
    let (player, t) = save(&mut g);
    assert!(t.lists[d].contains(&"CRUCIBLE_TEST".to_owned()));
    for other in (0..3).filter(|i| *i != d) {
        assert_eq!(t.lists[other], before.lists[other]);
    }
    let q = player
        .with_file_name("maps_survivalworld_c.map")
        .join(DIFFICULTY_DIRS[d])
        .join("quests.gdd");
    let q = gd_formats::save::gdd::Quests::parse(&std::fs::read(q).unwrap()).unwrap();
    assert!(q.tokens.is_empty());
}

/// `map.dat` (`gd_game::save::write_map_dat`): every save writes the map path and mod beside the
/// quests (the Crucible's arena folder with `survivalmode`); headless (no map layout) without spawn.
#[test]
fn saves_write_map_dat() {
    use gd_formats::save::map_dat::MapDat;
    let tmp = TempDir::new("save_map_dat");
    let Some(path) = saved_character(&tmp) else {
        return;
    };
    let Some(mut g) = TestGame::with_save(&path) else {
        return;
    };
    g.spawn_player();
    g.steps(3);
    let d = g
        .app
        .world()
        .resource::<gd_game_core::session::LoadedCharacter>()
        .0
        .info
        .difficulty();
    let read = |g: &mut TestGame, folder: &str| {
        let p = g
            .save_now()
            .with_file_name(folder)
            .join(DIFFICULTY_DIRS[d])
            .join("map.dat");
        MapDat::parse(&std::fs::read(p).expect("map.dat written")).unwrap()
    };
    let m = read(&mut g, "levels_world001.map");
    assert_eq!(
        (m.version, m.map_path.as_str(), m.mod_name.as_str(), m.spawn),
        (2, "levels/world001.map", "", None)
    );
    g.app
        .world_mut()
        .resource_mut::<gd_game_core::session::Choice>()
        .survival = Some("maps/survivalworld_c.map".into());
    let m = read(&mut g, "maps_survivalworld_c.map");
    assert_eq!(
        (m.map_path.as_str(), m.mod_name.as_str()),
        ("maps/survivalworld_c.map", "survivalmode")
    );
}

/// A `quests.gdd` / `map.fow` the load cannot read is set aside to `.bak` and never rewritten from the
/// empty runtime. The session's save folder IS the loaded character's here, so an unguarded save
/// would overwrite the corrupt files in place; the guard keeps their bytes in `.bak` and writes fresh,
/// parseable files beside them.
#[test]
fn unreadable_map_files_are_set_aside_not_overwritten() {
    use gd_game::session;
    use gd_game::testing::{SEED, headless_app};
    use gd_game_core::session::Choice;
    let Some(mut g) = TestGame::new_character("Map Guard", false, false) else {
        return;
    };
    g.spawn_player();
    g.steps(2);
    let src = g.save_now();
    let map_dir = src
        .parent()
        .unwrap()
        .join("levels_world001.map")
        .join("Normal");
    let quests = map_dir.join("quests.gdd");
    let fow = map_dir.join("map.fow");
    // Truncate both so the load's parse fails (the formats are fail-closed).
    let corrupt = |p: &std::path::Path| {
        let b = std::fs::read(p).expect("map file written");
        let cut = b[..b.len() / 2].to_vec();
        std::fs::write(p, &cut).unwrap();
        cut
    };
    let bad_q = corrupt(&quests);
    let bad_f = corrupt(&fow);
    // A session whose save folder is this one: the map files' read path and write target coincide.
    let gdc = Gdc::parse(&std::fs::read(&src).unwrap()).unwrap();
    let choice = Choice {
        save: Some(src.clone()),
        name: gdc.header.name.clone(),
        female: false,
        hardcore: false,
        survival: None,
        custom: None,
    };
    let mut app = headless_app(g.data(), SEED);
    app.world_mut()
        .insert_resource(gd_game_core::session::SaveDir(g.save_dir()));
    session::begin(&mut app.world_mut().commands(), choice, Some(gdc), 0, false);
    app.world_mut().flush();
    let mut g2 = TestGame { app };
    g2.step(); // the story / fog load runs here, under the guard
    g2.spawn_player();
    g2.steps(2);
    // The corrupt bytes survive in `.bak`; nothing overwrote them in place.
    assert_eq!(
        std::fs::read(map_dir.join("quests.gdd.bak")).unwrap(),
        bad_q,
        "quests.gdd kept as .bak"
    );
    assert_eq!(
        std::fs::read(map_dir.join("map.fow.bak")).unwrap(),
        bad_f,
        "map.fow kept as .bak"
    );
    // The save writes fresh, parseable files beside the kept `.bak`s.
    g2.save_now();
    gd_formats::save::gdd::Quests::parse(&std::fs::read(&quests).unwrap()).unwrap();
    gd_formats::save::fow::Fow::parse(&std::fs::read(&fow).unwrap()).unwrap();
}
