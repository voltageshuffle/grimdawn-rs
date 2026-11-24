//! Headless checks of level population: a loaded level's proxies roll and
//! spawn their monsters once per session.
use bevy::prelude::*;
use gd_game::combat::MonsterAi;
use gd_game::monsters::{LevelPopulated, LevelProxies, ProxyCpu, ProxyStates};
use gd_game::testing::TestGame;
use gd_game_core::combat::Combatant;
use gd_game_core::levels::Levels;
use gd_game_core::monsters::Monster;

const PROXY: &str = "records/proxies/area001/beastsburrwitchoutskirts_n.dbr";

fn level_with_proxy(g: &mut TestGame, level: usize) -> Entity {
    let world = g.app.world_mut();
    let root = world
        .spawn(LevelProxies {
            level,
            proxies: vec![ProxyCpu {
                entity: 7,
                record: PROXY.into(),
                position: Vec3::new(8.0, 0.0, 0.0),
                uid: None,
            }],
            npcs: Vec::new(),
            chests: Vec::new(),
            set_pieces: Vec::new(),
            doors: Vec::new(),
            breakables: Vec::new(),
            blockers: Vec::new(),
            waypoints: Vec::new(),
            shrines: Vec::new(),
            sounds: Vec::new(),
            patrol_points: Vec::new(),
            placed: Vec::new(),
        })
        .id();
    world
        .resource_mut::<Levels>()
        .add(level, root, Vec3::ZERO, None);
    root
}

fn monsters(g: &mut TestGame) -> Vec<(Entity, Monster)> {
    g.app
        .world_mut()
        .query_filtered::<(Entity, &Monster), (With<Combatant>, With<MonsterAi>)>()
        .iter(g.app.world())
        .map(|(e, m)| (e, m.clone()))
        .collect()
}

#[test]
fn proxies_spawn_their_monsters_once() {
    // Seed 4 rolls a pack from the proxy's pool (rolls are seeded; this
    // proxy's pool may roll nothing, as seeds 1-3 do).
    let Some(mut g) = TestGame::new_character_seeded(4) else {
        return;
    };
    g.flat_ground(Vec3::ZERO, 24.0);
    g.spawn_player();
    let root = level_with_proxy(&mut g, 3);
    g.step();
    assert!(g.app.world().get::<LevelPopulated>(root).is_some());
    // The monsters come once their meshes and clips are parsed in the background (`spawn_level_monsters`).
    let rolled_alive = g.app.world().resource::<ProxyStates>().0[&(3, 7)].len();
    assert!(rolled_alive > 0, "the proxy rolled nothing");
    let spawned = |w: &mut World| {
        let mut q = w.query_filtered::<(), (With<Monster>, With<Combatant>, With<MonsterAi>)>();
        q.iter(w).count()
    };
    g.run_until(600, |w| spawned(w) == rolled_alive)
        .expect("the proxy's monsters spawned");
    let first = monsters(&mut g);
    assert_eq!(first.len(), rolled_alive);
    assert!(first.iter().all(|(_, m)| m.slot.0 == 3 && m.slot.1 == 7));
    // Unloading culls them; loading again restores the same roll.
    g.app.world_mut().resource_mut::<Levels>().remove(3);
    g.app.world_mut().despawn(root);
    g.step();
    assert!(monsters(&mut g).is_empty(), "culled with the level");
    level_with_proxy(&mut g, 3);
    g.run_until(600, |w| spawned(w) == rolled_alive)
        .expect("the restored monsters spawned");
    let again = monsters(&mut g);
    let recs = |v: &[(Entity, Monster)]| {
        let mut r: Vec<_> = v.iter().map(|(_, m)| (m.record.clone(), m.level)).collect();
        r.sort();
        r
    };
    assert_eq!(recs(&again), recs(&first), "the session's roll is kept");
}
