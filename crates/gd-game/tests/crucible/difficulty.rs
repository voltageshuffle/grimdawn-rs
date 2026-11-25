//! The Crucible's difficulties: Aspirant (Normal), Challenger (Elite), Gladiator (Ultimate). The engine's
//! differences are the game difficulty (no challenge, mode 1 in `lastDifficulty`), the ramp pak
//! (`survivalAdjustmentNormal / Elite / Ultimate`) and the score column (`playerscore.dbr`); the rest is the
//! scripts' (`SurvivalEvent_StartBonusTimer`, `rewards.lua`).
use super::*;

/// Each difficulty on arena A: the session's difficulty and no challenge, the first wave's bonus timer
/// (`(1 / (multiplier + 1) ^ 0.49) * defaultTimer`: 75 / 75 / 80 s at tier 0), a Common kill's score in the
/// difficulty's column (`commonBonus` 1 / 2 / 4 × (1 + wave 1 / 10)) and the ramp pak changing a monster.
#[test]
fn challenger_and_gladiator() {
    for d in 0..3usize {
        let Some(mut g) = TestGame::crucible_mode(ARENA, d) else {
            return;
        };
        g.flat_ground(Vec3::ZERO, 30.0);
        let p = g.spawn_player();
        g.steps(2);
        let m = g
            .app
            .world()
            .resource::<gd_game_core::session::GameMode>()
            .0;
        assert_eq!((m.difficulty, m.challenge), (d, 0));
        let objs = super::arenas::arena_objects(&g, ARENA);
        let lua_d = {
            let rt = g.app.world().non_send::<StoryRuntime>();
            for (id, rec, c) in &objs {
                rt.host.add_object(*id, rec, *c);
            }
            rt.host
                .lua
                .load("gd.survival.eventControl.startSurvivalModeEvent()")
                .exec()
                .unwrap();
            rt.host
                .lua
                .load("return Game.GetGameDifficulty()")
                .eval::<f64>()
                .unwrap()
        };
        assert_eq!(lua_d as usize, d);
        g.steps(2);
        let (wave, timer) = {
            let rt = g.app.world().non_send::<StoryRuntime>();
            let sv = &rt.story.borrow().game.survival;
            (sv.wave_tier, sv.timer_ms)
        };
        assert_eq!(wave, 1);
        let default = if d < 2 { 75_000.0 } else { 80_000.0 };
        let want = default / 2f32.powf(0.49);
        assert!(
            (timer as f32 - want).abs() < 200.0,
            "difficulty {d}: timer {timer} ms, want ~{want}"
        );
        // The ramp takes the difficulty's survival pak (`survivalinfo.dbr`), at its first column.
        let key = [
            "survivalAdjustmentNormal",
            "survivalAdjustmentElite",
            "survivalAdjustmentUltimate",
        ][d];
        let want = g
            .data()
            .db
            .get("records/game/survivalinfo.dbr")
            .and_then(|r| r.str(key).map(str::to_owned))
            .expect("the survival paks");
        let rules = g
            .app
            .world()
            .resource::<gd_game_core::rules::CombatRules>()
            .0
            .clone();
        let pak = rules
            .challenge
            .as_ref()
            .map(|r| r.name.to_ascii_lowercase());
        assert_eq!(pak, Some(want.to_ascii_lowercase()), "difficulty {d}");
        assert_eq!(rules.challenge_index, 0);
        g.app
            .world_mut()
            .write_message(gd_game::combat::MonsterKilled {
                record: ZOMBIE.into(),
                level: 30,
                position: Vec3::ZERO,
                entity: Entity::PLACEHOLDER,
                killer: Some((p, None)),
            });
        g.steps(2);
        let rt = g.app.world().non_send::<StoryRuntime>();
        let score = rt.story.borrow().game.survival.score;
        assert_eq!(score, [1, 2, 4][d], "difficulty {d}");
    }
}
