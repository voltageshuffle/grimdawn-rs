//! Headless checks of session-side rules: the fog of war reveals around the
//! player once it moved 1 m.
use bevy::prelude::*;
use gd_game::fog::FogOfWar;
use gd_game::testing::TestGame;

fn game() -> Option<TestGame> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    g.step();
    Some(g)
}

#[test]
fn fog_reveals_after_a_one_metre_move() {
    let Some(mut g) = game() else { return };
    let map = std::sync::Arc::new(
        gd_formats::map::MapFile::open_resource(&g.data().vfs, "levels/world001.map").unwrap(),
    );
    let (lo, hi) = gd_game::fog::bounds(&map.regions[0]);
    let at = (lo + hi) * 0.5;
    g.app
        .world_mut()
        .insert_resource(gd_game_core::save::MapLayout(map.clone()));
    let player = g.player();
    let put = |g: &mut TestGame, p: Vec3| {
        g.get_mut::<Transform>(player).translation = p;
        g.step();
        let fog = g.app.world().resource::<FogOfWar>();
        (fog.last, fog.is_in_fog(&map, p))
    };
    let (last, fogged) = put(&mut g, at);
    assert_eq!(last, Some(Vec2::new(at.x, at.z)));
    assert!(!fogged, "revealed around the player");
    let (last, _) = put(&mut g, at + Vec3::new(0.5, 0.0, 0.5));
    assert_eq!(last, Some(Vec2::new(at.x, at.z)), "under 1 m: no reveal");
    let moved = at + Vec3::new(1.5, 0.0, 0.0);
    let (last, _) = put(&mut g, moved);
    assert_eq!(last, Some(Vec2::new(moved.x, moved.z)));
}
