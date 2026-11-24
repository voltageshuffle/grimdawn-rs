//! Headless checks of point spending (the UI sends requests; the rules are
//! applied here).
use gd_game::points::{Attribute, RaiseSkill, SpendAttribute};
use gd_game::testing::TestGame;
use gd_game_core::combat::Combatant;
use gd_game_core::player::{EffectiveSkills, Loadout};
use gd_game_core::progress::PlayerProgress;

fn game() -> Option<(TestGame, bevy::prelude::Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    let p = g.spawn_player();
    g.step();
    Some((g, p))
}

fn points(g: &mut TestGame, attr: i32, skill: i32) {
    let mut pr = g.app.world_mut().resource_mut::<PlayerProgress>();
    pr.progress.attribute_points = attr;
    pr.progress.skill_points = skill;
}

#[test]
fn attribute_points_raise_the_profile() {
    let Some((mut g, p)) = game() else { return };
    points(&mut g, 2, 0);
    let before = g.get::<Combatant>(p).profile.sheet.physique;
    for _ in 0..3 {
        g.app
            .world_mut()
            .write_message(SpendAttribute(Attribute::Physique));
    }
    g.step();
    // Only two points to spend.
    assert_eq!(g.get::<Loadout>(p).allocated.physique, 2);
    assert_eq!(g.progress().progress.attribute_points, 0);
    assert!(g.get::<Combatant>(p).profile.sheet.physique > before);
}

/// A raised skill (here a level-2 new character's first mastery) shows on
/// the next frame; without points nothing changes.
#[test]
fn skill_points_raise_a_skill() {
    let Some((mut g, p)) = game() else { return };
    let db = g.data().db;
    // Level 2 opens the first mastery slot (`masteries_allowed`).
    g.app
        .world_mut()
        .resource_mut::<PlayerProgress>()
        .progress
        .level = 2;
    let level = g.progress().progress.level;
    let book = g.get::<Loadout>(p).skills.clone();
    let trees = g.data().skill_trees();
    let (tree, skill) = trees
        .trees
        .iter()
        .flat_map(|t| t.skills.iter().map(move |s| (t, s.clone())))
        .find(|(t, s)| book.cannot_raise(&db, &trees, t, s, level).is_none())
        .map(|(t, s)| (t.path.clone(), s))
        .expect("a new character can raise a mastery");
    let base = book.level(&skill);
    points(&mut g, 0, 0);
    g.app.world_mut().write_message(RaiseSkill {
        tree: tree.clone(),
        skill: skill.clone(),
    });
    g.step();
    assert_eq!(
        g.get::<Loadout>(p).skills.level(&skill),
        base,
        "no points, no raise"
    );
    points(&mut g, 0, 1);
    g.app.world_mut().write_message(RaiseSkill {
        tree,
        skill: skill.clone(),
    });
    g.step();
    assert_eq!(g.get::<Loadout>(p).skills.level(&skill), base + 1);
    assert_eq!(g.progress().progress.skill_points, 0);
    let eff = g.get::<EffectiveSkills>(p);
    assert_eq!(eff.level(&skill), base + 1);
    assert!(eff.effective_level(&db, &skill) > base);
}
