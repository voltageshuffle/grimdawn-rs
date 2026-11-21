//! The single-player pause (`gd_game::pause`): while paused no time-based
//! system advances (regeneration, a monster's attack, the game clock); after
//! unpausing they run again.
use bevy::prelude::*;
use gd_game::combat::Combatant;
use gd_game::pause::GamePause;
use gd_game::testing::TestGame;

use crate::common::ZOMBIE;

#[test]
fn paused_game_does_not_advance() {
    let Some(mut g) = TestGame::new_character("Tester", false, false) else {
        return;
    };
    g.flat_ground(Vec3::ZERO, 24.0);
    let p = g.spawn_player();
    let max = g.get::<Combatant>(p).profile.sheet.life;
    g.get_mut::<Combatant>(p).life = max * 0.5;
    g.app.world_mut().resource_mut::<GamePause>().key = true;
    // The clock pauses from the frame after the request.
    g.steps(2);
    let z = g.spawn_monster(ZOMBIE, 54, Vec3::new(2.0, 0.0, 0.0));
    let (life, clock) = (
        g.get::<Combatant>(p).life,
        g.app.world().resource::<Time<Virtual>>().elapsed_secs(),
    );
    let zpos = g.get::<Transform>(z).translation;
    g.steps(60);
    assert_eq!(
        g.get::<Combatant>(p).life,
        life,
        "life changed while paused (regeneration or a hit)"
    );
    assert_eq!(
        g.app.world().resource::<Time<Virtual>>().elapsed_secs(),
        clock,
        "the game clock ran while paused"
    );
    assert_eq!(
        g.get::<Transform>(z).translation,
        zpos,
        "the monster moved while paused"
    );
    g.app.world_mut().resource_mut::<GamePause>().key = false;
    let resumed = g.run_until(10, |w| w.resource::<Time<Virtual>>().elapsed_secs() > clock);
    assert!(
        resumed.is_some(),
        "the game clock did not resume after unpausing"
    );
}
