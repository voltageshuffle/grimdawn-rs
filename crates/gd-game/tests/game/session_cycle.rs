//! Begin, tear down, begin again in one headless app: what `grimdawn::exit` does for the gameplay half
//! (`session::end_session_state`) leaves a world a new session can start in.
use std::path::{Path, PathBuf};

use gd_formats::save::gdc::Gdc;
use gd_game::session;
use gd_game::testing::{TempDir, TestGame};
use gd_game_core::inventory::Inventory;
use gd_game_core::progress::PlayerProgress;
use gd_game_core::session::{Choice, SessionReady};

/// A port-written save of a new character raised to level 7, in `tmp`.
fn saved_character(tmp: &TempDir) -> Option<PathBuf> {
    let mut g = TestGame::new_character("Cycler", false, false)?;
    g.spawn_player();
    let mut gdc = Gdc::parse(&std::fs::read(g.save_now()).unwrap()).unwrap();
    gdc.bio.level = 7;
    gdc.header.level = 7;
    let path = tmp.path().join("player.gdc");
    std::fs::write(&path, gdc.encode().unwrap()).unwrap();
    Some(path)
}

/// Ends the session the way `grimdawn::exit` does for the gameplay half: the session's entities go, the state
/// resets, the epoch moves.
fn end_session(g: &mut TestGame, entities: &[bevy::prelude::Entity]) {
    let world = g.app.world_mut();
    for e in entities {
        world.entity_mut(*e).despawn();
    }
    session::end_session_state(world);
    world
        .resource_mut::<gd_game_core::session::SessionEpoch>()
        .0 += 1;
    g.steps(3);
}

/// Begins a session with the character saved at `path` at `difficulty`.
fn begin_saved(g: &mut TestGame, path: &Path, difficulty: usize) {
    let gdc = Gdc::parse(&std::fs::read(path).unwrap()).unwrap();
    let choice = Choice {
        save: Some(path.to_owned()),
        name: gdc.header.name.clone(),
        female: gdc.header.sex == 0,
        hardcore: false,
        survival: None,
        custom: None,
    };
    let veteran = gd_game_core::session::is_veteran(&gdc);
    session::begin(
        &mut g.app.world_mut().commands(),
        choice,
        Some(gdc),
        difficulty,
        veteran,
    );
    g.app.world_mut().flush();
    g.step();
}

#[test]
fn begin_end_begin() {
    let tmp = TempDir::new("session_cycle_begin");
    let Some(path) = saved_character(&tmp) else {
        return;
    };
    let mut g = TestGame::with_save(&path).unwrap();
    let p = g.spawn_player();
    g.steps(5);
    let level = g.progress().progress.level;
    assert_eq!(level, 7);
    assert!(g.app.world().get_resource::<Inventory>().is_some());

    // End: the player goes (the game despawns the session's entities), the state resets.
    let world = g.app.world_mut();
    world.entity_mut(p).despawn();
    session::end_session_state(world);
    world
        .resource_mut::<gd_game_core::session::SessionEpoch>()
        .0 += 1;
    assert!(world.get_resource::<SessionReady>().is_none());
    assert!(world.get_resource::<PlayerProgress>().is_none());
    assert!(world.get_resource::<Inventory>().is_none());
    g.steps(3);

    // Begin again with the same character.
    begin_saved(&mut g, &path, 0);
    g.spawn_player();
    g.steps(5);
    assert!(g.app.world().get_resource::<Inventory>().is_some());
    assert_eq!(g.progress().progress.level, level);
}

/// Exit in the middle of a fight, then Create a new character: nothing of the first session survives
/// (the monster and the player are the game's entities, which `grimdawn::exit` despawns).
#[test]
fn exit_in_a_fight_then_create() {
    let tmp = TempDir::new("session_cycle_fight");
    let Some(path) = saved_character(&tmp) else {
        return;
    };
    let mut g = TestGame::with_save(&path).unwrap();
    g.flat_ground(bevy::prelude::Vec3::ZERO, 24.0);
    let p = g.spawn_player();
    let m = g.spawn_monster(
        "records/creatures/enemies/hero/zombie_h01.dbr",
        1,
        bevy::prelude::Vec3::new(3.0, 0.0, 0.0),
    );
    g.steps(20);
    end_session(&mut g, &[m, p]);

    let choice = Choice {
        save: None,
        name: "Fresh".into(),
        female: true,
        hardcore: false,
        survival: None,
        custom: None,
    };
    session::begin(&mut g.app.world_mut().commands(), choice, None, 0, false);
    g.app.world_mut().flush();
    g.step();
    g.spawn_player();
    g.steps(5);
    assert!(g.app.world().get_resource::<Inventory>().is_some());
    assert_eq!(g.progress().progress.level, 1);
    assert!(
        g.app
            .world()
            .get_resource::<gd_game_core::session::LoadedCharacter>()
            .is_none()
    );
}

/// Monster profiles are built from the session's rules (difficulty balance), so the cache does not outlive the
/// session: a Normal session followed by an Ultimate one spawns the Ultimate monster, not the cached Normal one.
#[test]
fn second_session_builds_its_own_monster_profiles() {
    use gd_game::monsters::ProfileCache;
    use gd_game_core::combat::Combatant;
    const RECORD: &str = "records/creatures/enemies/hero/zombie_h01.dbr";
    let at = bevy::prelude::Vec3::new(3.0, 0.0, 0.0);
    let life = |g: &mut TestGame| -> f32 {
        let m = g.spawn_monster(RECORD, 30, at);
        let life = g.get::<Combatant>(m).profile.sheet.life;
        g.app.world_mut().entity_mut(m).despawn();
        life
    };

    let tmp = TempDir::new("session_cycle_profiles");
    let Some(path) = saved_character(&tmp) else {
        return;
    };
    let mut ultimate = TestGame::with_save_mode(&path, 2, false).unwrap();
    let want = life(&mut ultimate);
    drop(ultimate);

    let mut g = TestGame::with_save_mode(&path, 0, false).unwrap();
    let p = g.spawn_player();
    let normal = life(&mut g);
    assert_ne!(normal, want, "the difficulties differ for this monster");
    assert!(!g.app.world().resource::<ProfileCache>().0.is_empty());

    end_session(&mut g, &[p]);
    assert!(g.app.world().resource::<ProfileCache>().0.is_empty());

    begin_saved(&mut g, &path, 2);
    g.spawn_player();
    g.steps(3);
    assert_eq!(life(&mut g), want);
}
