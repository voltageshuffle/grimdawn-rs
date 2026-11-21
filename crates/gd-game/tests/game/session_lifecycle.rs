//! What a second session in the same process relies on (`grimdawn::exit`): `NewSession` fires once per
//! teardown, and systems gated on session resources run before any session exists.
use bevy::prelude::*;
use gd_game::session::{NewSession, SessionEpoch};
use gd_game::testing::{data, headless_app};

#[derive(Resource, Default)]
struct Begun(u32);

fn count_begun(mut fresh: NewSession, mut n: ResMut<Begun>) {
    if fresh.begun() {
        n.0 += 1;
    }
}

#[test]
fn new_session_fires_once_per_epoch() {
    let mut app = App::new();
    app.init_resource::<SessionEpoch>()
        .init_resource::<Begun>()
        .add_systems(Update, count_begun);
    app.update();
    app.update();
    assert_eq!(
        app.world().resource::<Begun>().0,
        0,
        "the first session is not a new one"
    );
    app.world_mut().resource_mut::<SessionEpoch>().0 += 1;
    app.update();
    app.update();
    assert_eq!(app.world().resource::<Begun>().0, 1);
    app.world_mut().resource_mut::<SessionEpoch>().0 += 1;
    app.update();
    assert_eq!(app.world().resource::<Begun>().0, 2);
}

/// Before a session starts (the main menu) the headless systems must run without the session's resources:
/// the gated ones are skipped, none fails parameter validation.
#[test]
fn systems_run_before_a_session_starts() {
    let Some(data) = data() else { return };
    let mut app = headless_app(data, 7);
    for _ in 0..5 {
        app.update();
    }
    assert!(
        app.world()
            .get_resource::<gd_game::inventory::Inventory>()
            .is_none()
    );
    assert!(
        app.world()
            .get_resource::<gd_game::rules::CombatRules>()
            .is_none()
    );
}
