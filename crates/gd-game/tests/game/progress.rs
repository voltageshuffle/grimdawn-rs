//! Headless checks of experience, level-ups and story progress rewards.
use bevy::prelude::*;
use gd_game::combat::MonsterKilled;
use gd_game::testing::TestGame;
use gd_game_core::combat::Combatant;
use gd_game_core::progress::{LeveledUp, PlayerProgress};
use gd_game_core::story::StoryEffect;
use gd_sim::progress::Progress;
use gd_story::story::Effect;

use crate::common::ZOMBIE;

fn new_game() -> Option<TestGame> {
    TestGame::new_character("Tester", false, false)
}

fn game() -> Option<(TestGame, Entity)> {
    let mut g = new_game()?;
    let p = g.spawn_player();
    g.step();
    Some((g, p))
}

fn kill(g: &mut TestGame, record: &str, level: i32) {
    let p = g
        .app
        .world_mut()
        .query_filtered::<Entity, With<gd_game_core::player::Player>>()
        .single(g.app.world())
        .unwrap();
    // All the body's damage was the player's.
    let mut by = gd_game_core::combat::Attackers::default();
    by.register(p, p, 1.0, true);
    let entity = g.app.world_mut().spawn(by).id();
    g.app.world_mut().write_message(MonsterKilled {
        record: record.into(),
        level,
        position: Vec3::ZERO,
        entity,
        killer: Some((p, None)),
    });
}

/// Experience = reward × the player's share of the damage (`HandleBonus` × `GetExperienceMultiplier`):
/// his and his pets' hits count for him, an ally's dilute it; a suicide is its last attacker's kill.
#[test]
fn experience_is_the_damage_share() {
    use gd_game::combat::Suicide;
    use gd_game_core::combat::Attackers;
    let Some(mut g) = new_game() else {
        return;
    };
    g.flat_ground(Vec3::ZERO, 60.0);
    let p = g.spawn_player();
    // A level the zombie's reward is worth a quarter of without rounding to nothing.
    let level = 20;
    let xp = |g: &TestGame| g.progress().progress.experience;
    let die = |g: &mut TestGame, shares: &[(Entity, f32)]| -> u64 {
        let z = g.spawn_monster(ZOMBIE, level, Vec3::new(-40.0, 0.0, 0.0));
        g.steps(2);
        for (by, d) in shares {
            g.get_mut::<Attackers>(z).register(*by, *by, *d, true);
        }
        let before = xp(g);
        g.app.world_mut().entity_mut(z).insert(Suicide);
        for _ in 0..5 {
            g.step();
        }
        xp(g) - before
    };
    let other = g.spawn_monster(ZOMBIE, level, Vec3::new(40.0, 0.0, 0.0));
    let full = die(&mut g, &[(p, 100.0)]);
    let quarter = die(&mut g, &[(other, 300.0), (p, 100.0)]);
    let none = die(&mut g, &[(p, 100.0), (other, 1.0)]);
    assert!(full > 0, "the player's own kill pays");
    let want = (full as f32 * 0.25) as i64 as u64;
    assert!(
        quarter.abs_diff(want) <= 1,
        "a quarter of the damage, a quarter of the reward ({quarter} / {full})"
    );
    assert_eq!(none, 0, "its last attacker (a monster) killed it: nothing");
}

/// Crossing the threshold levels up: new level, full life, energy refilled.
#[test]
fn level_up_refills_life() {
    let Some((mut g, p)) = game() else { return };
    let before = g.progress().progress.experience;
    kill(&mut g, ZOMBIE, 1);
    g.step();
    assert!(
        g.progress().progress.experience > before,
        "a kill gives experience"
    );
    let rules = g.rules();
    let next = {
        let mut pr = g.app.world_mut().resource_mut::<PlayerProgress>();
        pr.progress.experience = Progress::threshold(&rules.levels, pr.progress.level) - 1;
        pr.progress.level + 1
    };
    {
        let mut cb = g.get_mut::<Combatant>(p);
        cb.life = 1.0;
    }
    kill(&mut g, ZOMBIE, 1);
    g.step();
    assert_eq!(g.progress().progress.level, next);
    let cb = g.get::<Combatant>(p);
    assert_eq!(cb.profile.level, next);
    assert_eq!(cb.life, cb.profile.sheet.life);
    let msgs = g.app.world().resource::<Messages<LeveledUp>>();
    assert!(
        msgs.iter_current_update_messages()
            .any(|m| m.player == p && m.level == next)
    );
}

#[test]
fn story_rewards_change_progress() {
    let Some((mut g, _)) = game() else { return };
    let gold = g.progress().gold;
    let (sp, ap) = (
        g.progress().progress.skill_points,
        g.progress().progress.attribute_points,
    );
    for e in [
        Effect::GiveSkillPoints(2),
        Effect::GiveAttributePoints(1),
        Effect::GiveMoney(-(gold as i64) - 100),
    ] {
        g.app.world_mut().write_message(StoryEffect(e));
    }
    g.step();
    let pr = g.progress();
    assert_eq!(
        (
            pr.progress.skill_points,
            pr.progress.attribute_points,
            pr.gold
        ),
        (sp + 2, ap + 1, 0)
    );
}
