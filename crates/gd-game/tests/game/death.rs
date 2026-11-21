//! Headless checks of dying: the experience penalty and the gravestone that
//! gives part of it back, and the respawn.
use bevy::prelude::*;
use gd_game::combat::{Combatant, Dead};
use gd_game::death::Gravestone;
use gd_game::play_stats::PlayCounts;
use gd_game::player_orders::InteractTarget;
use gd_game::progress::PlayerProgress;
use gd_game::session::PlayerStart;
use gd_game::testing::TestGame;

fn game() -> Option<(TestGame, Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 24.0);
    g.app
        .world_mut()
        .insert_resource(PlayerStart(Vec3::new(-5.0, 0.0, 0.0)));
    let p = g.spawn_player();
    g.step();
    Some((g, p))
}

fn graves(g: &mut TestGame) -> Vec<(Entity, u64)> {
    g.app
        .world_mut()
        .query::<(Entity, &Gravestone)>()
        .iter(g.app.world())
        .map(|(e, s)| (e, s.redeem))
        .collect()
}

/// Death costs experience (never below the level's start), leaves a stone
/// worth part of it; using the stone gives that back.
#[test]
fn death_penalty_and_gravestone() {
    let Some((mut g, p)) = game() else { return };
    let rules = g.rules();
    let level = 10;
    let start = gd_sim::progress::Progress::threshold(&rules.levels, level - 1);
    {
        let mut pr = g.app.world_mut().resource_mut::<PlayerProgress>();
        pr.progress.level = level;
        pr.progress.experience = start + 10_000;
    }
    let before = g.progress().progress.experience;
    g.app.world_mut().entity_mut(p).insert(Dead { timer: 4.0 });
    g.step();
    let after = g.progress().progress.experience;
    assert!(after < before && after >= start, "{before} -> {after}");
    assert_eq!(g.app.world().resource::<PlayCounts>().deaths, 1);
    let stones = graves(&mut g);
    assert_eq!(stones.len(), 1);
    let (stone, redeem) = stones[0];
    assert!(redeem > 0 && redeem <= before - after);
    // Respawned after the dead timer, at the start point, alive.
    let back = g.run_until(600, |w| w.get::<Dead>(p).is_none());
    assert!(back.is_some(), "no respawn");
    assert!(
        g.get::<Transform>(p)
            .translation
            .distance(Vec3::new(-5.0, 0.0, 0.0))
            < 0.5
    );
    assert!(g.get::<Combatant>(p).life > 0.0);
    // Standing at the stone and using it.
    let at = g.get::<Transform>(stone).translation;
    g.get_mut::<Transform>(p).translation = at;
    g.get_mut::<InteractTarget>(p).0 = Some(stone);
    g.step();
    assert_eq!(g.progress().progress.experience, after + redeem);
    assert!(graves(&mut g).is_empty());
}
