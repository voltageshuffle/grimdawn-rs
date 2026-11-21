//! Curses (`gd_game::skill_buffs` module docs): a `Skill_AttackBuff` lands
//! its `SkillBuff_Debuf` record on the foes in radius as a received buff
//! (record, level, caster, time left) under `CreateUpdateSkillBuff`'s rule;
//! its stats count in the holder's profile, and it runs out. A level-1
//! zombie curses the player.
use crate::common::ZOMBIE;
use bevy::prelude::*;
use gd_game::casting::{Casting, Kind, ProcCast};
use gd_game::combat::Combatant;
use gd_game::skill_buffs::{ReceivedBuff, ReceivedBuffs};
use gd_game::testing::TestGame;

/// The Necromancer's Curse of Vitality: `skillActiveDuration` 5, radius 10,
/// `characterLifeModifier` -12 (-25 from level 13), vitality damage,
/// `skillCastSound`.
const CURSE: &str = "records/skills/nonplayerskills/factionskills/necro_curseofvitality.dbr";
const CURSE_BUFF: &str =
    "records/skills/nonplayerskills/factionskills/necro_curseofvitality_buff.dbr";

fn game() -> Option<(TestGame, Entity, Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 40.0);
    let p = g.spawn_player();
    let z = g.spawn_monster(ZOMBIE, 1, Vec3::new(3.0, 0.0, 0.0));
    g.steps(3);
    Some((g, p, z))
}

/// The zombie's curse at the player.
fn curse(g: &mut TestGame, z: Entity, p: Entity, level_index: usize) {
    let point = g.get::<Transform>(p).translation;
    g.app.world_mut().write_message(ProcCast {
        caster: z,
        cast: Casting {
            skill: CURSE.into(),
            level_index,
            kind: Kind::TargetRadius,
            point,
            target: Some(p),
            hostile: true,
            parent: None,
        },
    });
}

fn entry(g: &TestGame, e: Entity) -> Option<ReceivedBuff> {
    g.app
        .world()
        .get::<ReceivedBuffs>(e)
        .and_then(|r| r.list.iter().find(|b| b.buff == CURSE_BUFF).cloned())
}

fn max_life(g: &TestGame, e: Entity) -> f32 {
    g.get::<Combatant>(e).profile.sheet.life
}

/// New, refreshed, replaced, expired: the entry and the holder's profile.
#[test]
fn curse_lands_refreshes_replaces_and_expires() {
    let Some((mut g, p, z)) = game() else { return };
    let unbuffed = max_life(&g, p);
    curse(&mut g, z, p, 0);
    g.steps(3);
    let b = entry(&g, p).expect("the curse lands on the player");
    assert_eq!(
        (b.skill.as_str(), b.level_index, b.from, b.total),
        (CURSE, 0, z, 5.0)
    );
    assert!(b.left > 4.8 && b.left < 5.0, "{}", b.left);
    assert!(entry(&g, z).is_none(), "foes only");
    let level1 = max_life(&g, p);
    assert!(level1 < unbuffed, "-12 % life: {unbuffed} -> {level1}");
    // Refresh: the same caster's equal curse resets its time.
    g.steps(60);
    assert!(entry(&g, p).unwrap().left < 4.1);
    curse(&mut g, z, p, 0);
    g.steps(2);
    assert!(entry(&g, p).unwrap().left > 4.9, "refreshed");
    assert_eq!(max_life(&g, p), level1);
    // Replace: another level from the same caster is a new buff.
    curse(&mut g, z, p, 20);
    g.steps(3);
    assert_eq!(entry(&g, p).unwrap().level_index, 20);
    assert_eq!(
        g.get::<ReceivedBuffs>(p)
            .list
            .iter()
            .filter(|b| b.buff == CURSE_BUFF)
            .count(),
        1
    );
    assert!(max_life(&g, p) < level1, "-25 % life at level 21");
    // Expiry: 5 s later it is gone and the profile is back.
    g.run_until(400, |w| {
        w.get::<ReceivedBuffs>(p).is_none_or(|r| !r.has(CURSE_BUFF))
    })
    .expect("the curse runs out");
    g.steps(3);
    assert_eq!(max_life(&g, p), unbuffed);
}
