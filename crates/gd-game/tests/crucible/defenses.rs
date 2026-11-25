//! The defenses (`survival/defenses.lua`).
use super::*;

/// Building a Fire Turret at site 1 spawns `turret_fire` (level `avg + avg / 50 + 1`), swaps the site NPC for
/// its upgrade NPC and grants `SURVIVALMODE_DEFENSEBUILT`; the upgrades replace it by `turret_fire02`, then
/// `turret_fire03` with no NPC left, granting no token.
#[test]
fn a_defense_is_built_and_upgraded_twice() {
    let Some((g, _, _)) = setup() else {
        return;
    };
    let objs = super::arenas::arena_objects(&g, ARENA);
    let rt = g.app.world().non_send::<StoryRuntime>();
    for (id, rec, c) in &objs {
        rt.host.add_object(*id, rec, *c);
    }
    rt.host.run_queued();
    let point = objs
        .iter()
        .find(|o| o.1.ends_with("defensepoint_01.dbr"))
        .expect("defense point 1")
        .2
        .origin;
    let npcs = || -> Vec<String> {
        let s = rt.story.borrow();
        let mut v: Vec<String> = s
            .entities
            .objects
            .values()
            .filter(|o| o.record.contains("npc_defensesite_01") && o.alive && o.in_world)
            .map(|o| o.record.rsplit('/').next().unwrap().to_owned())
            .collect();
        v.sort();
        v
    };
    assert_eq!(npcs(), ["npc_defensesite_01.dbr"]);
    rt.story.borrow_mut().take_effects();
    let step = |code: &str| -> Vec<(String, [f32; 3])> {
        rt.host.lua.load(code).exec().unwrap();
        rt.host.run_queued();
        rt.story
            .borrow_mut()
            .take_effects()
            .into_iter()
            .filter_map(|e| match e {
                gd_story::story::Effect::Spawn { record, coords, .. }
                    if record.contains("/creatures/defenses/") =>
                {
                    Some((record, coords.origin))
                }
                _ => None,
            })
            .collect()
    };
    let built = rt.story.borrow().game.survival.defenses_built;
    let spawned = step("gd.survival.defenses.setDefensePoint01_TurretFire()");
    assert_eq!(
        spawned,
        [(
            "records/creatures/defenses/turret_fire.dbr".to_owned(),
            point
        )]
    );
    assert_eq!(npcs(), ["npc_defensesite_01b.dbr"]);
    assert_eq!(rt.story.borrow().game.survival.defenses_built, built + 1);
    let spawned = step("gd.survival.defenses.upgradeDefensePoint01()");
    assert_eq!(spawned[0].0, "records/creatures/defenses/turret_fire02.dbr");
    assert_eq!(npcs(), ["npc_defensesite_01c.dbr"]);
    let spawned = step("gd.survival.defenses.upgradeDefensePoint01()");
    assert_eq!(spawned[0].0, "records/creatures/defenses/turret_fire03.dbr");
    assert!(npcs().is_empty());
    assert_eq!(rt.story.borrow().game.survival.defenses_built, built + 1);
}
