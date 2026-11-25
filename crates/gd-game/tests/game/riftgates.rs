//! Travel points: discovered riftgates round-trip through the save; a personal portal brings the player back.
use bevy::prelude::*;
use gd_game::riftgates::{
    GateTravel, IndexedPoint, PersonalPortal, Portal, Rift, RiftgateMap, RiftgateRequest,
    TravelState, Waypoint, WaypointIndex, WaypointKind,
};
use gd_game::testing::TestGame;
use gd_game_core::story::Notify;

fn game() -> Option<TestGame> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    g.step();
    Some(g)
}

fn waypoint(g: &mut TestGame, kind: WaypointKind, uid: [u32; 4], at: Vec3) -> Entity {
    let tf = Transform::from_translation(at);
    let mut e = g.app.world_mut().spawn((
        tf,
        GlobalTransform::from(tf),
        Waypoint {
            kind,
            uid,
            record: "records/interactive/test.dbr".into(),
            radius: 4.0,
            bind_radius: 0.0,
            locked: false,
        },
    ));
    if kind == WaypointKind::Riftgate {
        e.insert(gd_game::player_orders::Interactable::fixed(2.0));
    }
    e.id()
}

/// A gate discovered within `bindRadius` is saved for the session difficulty, reloaded, and a new one appended.
#[test]
fn discovered_riftgates_load_and_save() {
    use gd_formats::save::gdc::Gdc;
    let Some(mut first) = game() else { return };
    let gate = |g: &mut TestGame, uid: [u32; 4]| {
        let e = waypoint(g, WaypointKind::Riftgate, uid, Vec3::new(2.0, 0.0, 0.0));
        g.get_mut::<Waypoint>(e).bind_radius = 8.0;
        g.step();
        g.app.world_mut().despawn(e);
    };
    let old = [0x1111, 0x2222, 0x3333, 0x4444];
    gate(&mut first, old);
    // Valid while `first` lives.
    let path = first.save_now();
    let src = Gdc::parse(&std::fs::read(&path).unwrap()).unwrap();
    let d = src.info.difficulty();
    let mut g = TestGame::with_save(&path).unwrap();
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    g.step();
    let known = g.app.world().resource::<TravelState>().teleports.clone();
    assert!(known.contains(&old), "the saved riftgate is known");
    assert_eq!(
        known,
        src.teleports.lists[d]
            .iter()
            .map(gd_formats::map::uid_words)
            .collect::<Vec<_>>()
    );
    let new = [0x1234, 0x5678, 0x9abc, 0xdef0];
    gate(&mut g, new);
    let written = g.save_now();
    let saved = Gdc::parse(&std::fs::read(&written).expect("player.gdc written")).unwrap();
    let list: Vec<_> = saved.teleports.lists[d]
        .iter()
        .map(gd_formats::map::uid_words)
        .collect();
    assert_eq!(list.len(), known.len() + 1);
    assert_eq!(list.last(), Some(&new));
    assert_eq!(
        saved.teleports.lists[(d + 1) % 3],
        src.teleports.lists[(d + 1) % 3]
    );
}

fn travels(g: &TestGame) -> Vec<Vec3> {
    g.app
        .world()
        .resource::<Messages<GateTravel>>()
        .iter_current_update_messages()
        .map(|m| m.0)
        .collect()
}

fn notices(g: &TestGame) -> Vec<String> {
    g.app
        .world()
        .resource::<Messages<Notify>>()
        .iter_current_update_messages()
        .map(|m| m.0.clone())
        .collect()
}

fn request(g: &mut TestGame, r: RiftgateRequest) {
    g.app.world_mut().write_message(r);
    g.step();
}

fn set_player(g: &mut TestGame, at: Vec3) {
    let player = g.player();
    g.app
        .world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation = at;
}

fn portal(g: &TestGame) -> Option<(u32, Vec3)> {
    g.app
        .world()
        .resource::<PersonalPortal>()
        .portal
        .as_ref()
        .map(|p| (p.id, p.pos))
}

fn portal_entities(g: &mut TestGame) -> Vec<(Entity, u32, bool)> {
    let mut q = g.app.world_mut().query::<(Entity, &Portal)>();
    q.iter(g.app.world())
        .map(|(e, p)| (e, p.id, p.closing.is_some()))
        .collect()
}

/// A used gate at (1, 0, 0) and a discovered one far away.
fn at_gate(g: &mut TestGame) -> ([u32; 4], [u32; 4], [u32; 4], Vec3) {
    let (here, there, unknown) = ([7, 7, 7, 7], [8, 8, 8, 8], [9, 9, 9, 9]);
    let e = waypoint(g, WaypointKind::Riftgate, here, Vec3::new(1.0, 0.0, 0.0));
    let far = Vec3::new(500.0, 0.0, 300.0);
    let point = |pos| IndexedPoint {
        pos,
        record: "records/interactive/test.dbr".into(),
        map_record: String::new(),
    };
    g.app.world_mut().insert_resource(WaypointIndex(
        [(there, point(far)), (unknown, point(far))]
            .into_iter()
            .collect(),
    ));
    g.app
        .world_mut()
        .resource_mut::<TravelState>()
        .teleports
        .push(there);
    g.interact(e);
    g.step();
    (here, there, unknown, far)
}

#[test]
fn travel_away_from_the_gate_opens_a_portal_and_it_brings_back() {
    let Some(mut g) = game() else { return };
    let (_, there, _, far) = at_gate(&mut g);
    // Away from the gate (> 8 m): the player's own portal is no destination ...
    let away = Vec3::new(12.0, 0.0, 0.0);
    set_player(&mut g, away);
    request(&mut g, RiftgateRequest::Travel(Rift::Portal(1)));
    assert_eq!(notices(&g), ["tagRiftgateError03"]);
    assert!(!g.app.world().resource::<RiftgateMap>().at_gate);
    assert!(portal(&g).is_none());
    // ... but a gate is: a portal opens about 2 m from the player, and the travel goes on.
    request(&mut g, RiftgateRequest::Travel(Rift::Gate(there)));
    assert_eq!(travels(&g), [far]);
    let (id, at) = portal(&g).expect("a personal portal");
    // `GetNearSpawnCoords`: on the 4.1 m ring around the player.
    assert!(
        (at.xz().distance(away.xz()) - 4.1).abs() < 1e-3,
        "portal at {at:?}"
    );
    let ents = portal_entities(&mut g);
    assert_eq!(
        ents.iter().map(|e| (e.1, e.2)).collect::<Vec<_>>(),
        [(id, false)]
    );
    // At the far gate: the map travels back to the portal, which stays.
    set_player(&mut g, far + Vec3::X * 2.0);
    let e = waypoint(&mut g, WaypointKind::Riftgate, there, far);
    g.interact(e);
    g.step();
    assert_eq!(
        g.app.world().resource::<RiftgateMap>().gate,
        Some(Rift::Gate(there))
    );
    request(&mut g, RiftgateRequest::Travel(Rift::Portal(id)));
    assert_eq!(travels(&g), [at]);
    assert_eq!(portal(&g).map(|p| p.0), Some(id));
    // Back at the portal: using it opens the map with it in use; it is no destination then.
    set_player(&mut g, at + Vec3::X);
    let pe = portal_entities(&mut g)[0].0;
    g.interact(pe);
    g.step();
    assert_eq!(
        *g.app.world().resource::<RiftgateMap>(),
        RiftgateMap {
            open: true,
            gate: Some(Rift::Portal(id)),
            at_gate: true
        }
    );
    request(&mut g, RiftgateRequest::Travel(Rift::Portal(id)));
    assert_eq!(notices(&g), ["tagRiftgateError02"]);
    // From the portal (within 8 m) a gate is reached without a new portal.
    request(&mut g, RiftgateRequest::Travel(Rift::Gate(there)));
    assert_eq!(travels(&g), [far]);
    assert_eq!(portal(&g).map(|p| p.0), Some(id));
}
