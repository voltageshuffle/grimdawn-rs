//! The Crucible's saved play stats.
use super::*;

/// Every grant of `SURVIVALMODE_DEFENSEBUILT` / `SURVIVALMODE_POWERUPACTIVE` counts (`PlayStats::TokenGranted`,
/// a token already held included), and the counts are saved in `player.gdc` block 12.
#[test]
fn defenses_built_and_power_ups_count_and_save() {
    let Some((mut g, _, _)) = setup() else {
        return;
    };
    let base = {
        let rt = g.app.world().non_send::<StoryRuntime>();
        let sv = &rt.story.borrow().game.survival;
        (sv.defenses_built, sv.power_ups)
    };
    {
        let rt = g.app.world().non_send::<StoryRuntime>();
        rt.host
            .lua
            .load(
                r#"local p = Game.GetLocalPlayer()
                p:GiveToken("SURVIVALMODE_DEFENSEBUILT")
                p:GiveToken("SURVIVALMODE_DEFENSEBUILT")
                p:GiveToken("SURVIVALMODE_POWERUPACTIVE")
                p:GiveToken("SURVIVALMODE_OTHER")"#,
            )
            .exec()
            .unwrap();
    }
    g.steps(2);
    let now = {
        let rt = g.app.world().non_send::<StoryRuntime>();
        let sv = &rt.story.borrow().game.survival;
        (sv.defenses_built, sv.power_ups)
    };
    assert_eq!(now, (base.0 + 2, base.1 + 1));
    let gdc = std::fs::read(g.save_now()).unwrap();
    let gdc = gd_formats::save::gdc::Gdc::parse(&gdc).unwrap();
    assert_eq!(
        (
            gdc.stats.survival_defenses_built,
            gdc.stats.survival_powerups_activated
        ),
        now
    );
}
