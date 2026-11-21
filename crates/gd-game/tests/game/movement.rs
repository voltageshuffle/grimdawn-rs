//! Headless checks of player movement along a nav path.
use bevy::prelude::*;
use gd_game::character::{CharState, Character};
use gd_game::movement::MovePath;
use gd_game::nav::Nav;
use gd_game::testing::{FRAME, TestGame};

fn game() -> Option<(TestGame, Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 24.0);
    let p = g.spawn_player();
    Some((g, p))
}

/// The player runs a nav path to its end at the run animation's speed,
/// facing the way it goes, then idles.
#[test]
fn walks_a_path() {
    let Some((mut g, p)) = game() else { return };
    let mut scratch = gd_sim::nav::PathScratch::default();
    let path = g
        .app
        .world()
        .resource::<Nav>()
        .0
        .find_path(&mut scratch, [0.0, 0.0, 0.0], [10.0, 0.0, 6.0], 60_000)
        .expect("a path on flat ground");
    g.get_mut::<MovePath>(p).points = path.into_iter().skip(1).map(Vec3::from).collect();
    let frames = g
        .run_until(1000, |w| w.get::<MovePath>(p).unwrap().points.is_empty())
        .expect("arrives");
    let at = g.get::<Transform>(p).translation;
    assert!(at.distance(Vec3::new(10.0, 0.0, 6.0)) < 0.6, "at {at}");
    let secs = frames as f32 * FRAME.as_secs_f32();
    let speed = 11.66 / secs;
    assert!((3.0..12.0).contains(&speed), "{speed} m/s");
    g.step();
    assert_eq!(g.get::<Character>(p).state, CharState::Idle);
}
