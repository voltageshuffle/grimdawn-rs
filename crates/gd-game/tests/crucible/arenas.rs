//! The ten arenas (`gd_game_core::session::ARENAS`): their script objects.
use super::*;

/// A map's story objects (records with a script hook): `(id, record, coords)`, as the host streams them.
pub fn arena_objects(g: &TestGame, map: &str) -> Vec<(u32, String, Coords)> {
    let data = g.data();
    let m = gd_formats::map::MapFile::open_resource(&data.vfs, map).expect("the arena map");
    let mut out = Vec::new();
    for (index, r) in m.regions.iter().enumerate() {
        let Ok(level) = m.level(index) else { continue };
        for (i, e) in level.entities.iter().enumerate() {
            let rec = level.record_of(e).to_ascii_lowercase();
            let Some(dbr) = data.db.get(&rec) else {
                continue;
            };
            if dbr.str("onAddToWorld").is_none() && dbr.str("conversation").is_none() {
                continue;
            }
            let p = [
                r.position[0] as f32 + e.position[0],
                r.position[1] as f32 + e.position[1],
                r.position[2] as f32 + e.position[2],
            ];
            out.push((((index as u32) << 16) | (i as u32 + 1), rec, Coords::at(p)));
        }
    }
    out
}

/// Every arena loads: its script objects' hooks all exist in the Crucible's scripts, it has the event's script
/// objects (the event NPC, the player spawn point, the merchant, the five spawn beacons, the six wave spawn points
/// of every tier) and a player spawn point; and an event starts there (the first tier's first wave: a proxy at
/// spawn points 1 and 2, `tier01waves.lua`; point 6 is the bonus spawn, off until `checkBonusStatus`).
#[test]
fn every_arena_loads_and_starts_an_event() {
    for (_, letter) in gd_game_core::session::ARENAS {
        let map = gd_game_core::session::arena_map(letter);
        let Some((g, _, _)) = setup() else { return };
        assert!(g.data().vfs.exists(&map), "{map}");
        let objs = arena_objects(&g, &map);
        let hook = |g: &TestGame, rec: &str| {
            g.data()
                .db
                .get(rec)
                .and_then(|r| r.str("onAddToWorld").map(str::to_owned))
        };
        let hooks: std::collections::BTreeSet<String> =
            objs.iter().filter_map(|o| hook(&g, &o.1)).collect();
        {
            let rt = g.app.world().non_send::<StoryRuntime>();
            for h in &hooks {
                assert!(rt.host.function(h).is_some(), "{map}: {h} exists");
            }
        }
        let mut want: Vec<String> = [
            "eventControl.eventNPCScriptEntityOnAddToWorld",
            "eventControl.spawnPointOnAddToWorld",
            "eventControl.merchantNPCScriptEntityOnAddToWorld",
        ]
        .iter()
        .map(|h| format!("gd.survival.{h}"))
        .collect();
        want.extend(
            (1..=5).map(|i| format!("gd.survival.eventControl.spawnBeacon0{i}OnAddToWorld")),
        );
        want.extend(
            (1..=20).map(|t| format!("gd.survival.tier{t:02}Waves.spawnPoint01OnAddToWorld")),
        );
        want.extend((2..=6).map(|i| format!("spawnPoint0{i}OnAddToWorld")));
        for w in &want {
            assert!(hooks.contains(w), "{map}: a {w} object");
        }
        let spawns = gd_formats::map::MapFile::open_resource(&g.data().vfs, &map)
            .unwrap()
            .spawn_points
            .len();
        assert!(spawns > 0, "{map}: a player spawn point");
        // Stream the objects in, then start the event as the event NPC's first choice does.
        {
            let rt = g.app.world().non_send::<StoryRuntime>();
            for (id, rec, c) in &objs {
                rt.host.add_object(*id, rec, *c);
            }
            rt.story.borrow_mut().take_effects();
            rt.host
                .lua
                .load("gd.survival.eventControl.startSurvivalModeEvent()")
                .exec()
                .unwrap();
        }
        let rt = g.app.world().non_send::<StoryRuntime>();
        let mut s = rt.story.borrow_mut();
        let proxies: Vec<String> = s
            .take_effects()
            .into_iter()
            .filter_map(|e| match e {
                gd_story::story::Effect::Spawn {
                    record,
                    class: gd_story::story::state::ObjClass::Proxy,
                    ..
                } => Some(record),
                _ => None,
            })
            .collect();
        assert_eq!(s.game.survival.wave_tier, 1, "{map}: wave 1");
        assert!(s.game.survival.event_in_progress, "{map}: the event runs");
        let mut proxies = proxies;
        proxies.sort();
        assert_eq!(
            proxies,
            [
                "records/proxies/tier01waves/proxy_w01_p01a.dbr",
                "records/proxies/tier01waves/proxy_w01_p02a.dbr",
            ],
            "{map}"
        );
    }
}
