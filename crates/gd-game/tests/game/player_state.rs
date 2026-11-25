//! Headless checks of the player's derived state: one source for each
//! derived value, kept current when the raw state changes.
use gd_formats::save::item::Item;
use gd_game::testing::TestGame;
use gd_game_core::combat::Combatant;
use gd_game_core::player::{EffectiveSkills, Loadout, ProfileDirty};

fn game() -> Option<(TestGame, bevy::prelude::Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    let p = g.spawn_player();
    Some((g, p))
}

/// The first item (by record name: the name index is unordered) whose base record raises a class skill
/// (`augmentSkillName1`), and that skill.
fn skill_bonus_item(db: &gd_formats::vfs::Database) -> (Item, String) {
    let mut names: Vec<&str> = db
        .names()
        .filter(|n| n.starts_with("records/items/"))
        .collect();
    names.sort_unstable();
    names
        .into_iter()
        .find_map(|n| {
            let r = db.get(n)?;
            let skill = r.str("augmentSkillName1")?;
            (r.i32("augmentSkillLevel1")? > 0
                && skill
                    .to_ascii_lowercase()
                    .starts_with("records/skills/playerclass"))
            .then(|| {
                let item = Item {
                    base_name: n.into(),
                    stack_count: 1,
                    seed: 7,
                    ..Default::default()
                };
                (item, skill.to_owned())
            })
        })
        .expect("some item raises a class skill")
}

/// The skills every reader sees carry the equipment's `+N` bonuses.
#[test]
fn effective_skills_include_item_bonuses() {
    let Some((mut g, p)) = game() else { return };
    let db = g.data().db;
    let (item, skill) = skill_bonus_item(&db);
    {
        let mut loadout = g.get_mut::<Loadout>(p);
        loadout.skills.skills.push((skill, 1));
        loadout.set_items(&db, vec![item]);
    }
    g.app.world_mut().entity_mut(p).insert(ProfileDirty);
    g.steps(2);
    let loadout = g.get::<Loadout>(p);
    let eff = g.get::<EffectiveSkills>(p);
    assert_eq!(
        eff.0,
        loadout.effective_skills(&db, &g.data().skill_trees())
    );
    let raised: Vec<_> = loadout
        .skills
        .skills
        .iter()
        .filter(|(s, l)| *l > 0 && eff.effective_level(&db, s) > *l)
        .collect();
    assert!(!raised.is_empty(), "the worn item raises its skill");
    for (s, _) in &raised {
        assert_eq!(
            loadout.skill_level(&db, eff, s),
            eff.effective_level(&db, s)
                .max(gd_sim::skills::granted_level(&eff.item_grants, s))
        );
    }
}

/// Spent attribute points rebuild the profile; life keeps its fraction.
#[test]
fn profile_rebuild_keeps_life_fraction() {
    let Some((mut g, p)) = game() else { return };
    let before = g.get::<Combatant>(p).profile.clone();
    {
        let mut cb = g.get_mut::<Combatant>(p);
        cb.life = before.sheet.life * 0.5;
    }
    g.get_mut::<Loadout>(p).allocated.physique += 10;
    g.app.world_mut().entity_mut(p).insert(ProfileDirty);
    g.step();
    let cb = g.get::<Combatant>(p);
    assert!(cb.profile.sheet.physique > before.sheet.physique);
    assert!(cb.profile.sheet.life > before.sheet.life);
    // Within the frame's regeneration.
    assert!((cb.life / cb.profile.sheet.life - 0.5).abs() < 1e-3);
    assert!(g.app.world().get::<ProfileDirty>(p).is_none());
}
