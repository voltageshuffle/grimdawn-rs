//! Radius buffs reaching allies (`gd_game::auras`, `gd_game::pets` module
//! docs): the player's Elemental Infusion buffs the pet.
use bevy::prelude::*;
use gd_game::pets::Pet;
use gd_game::testing::TestGame;
use gd_game_core::casting::SpawnPet;
use gd_game_core::player::Loadout;
use gd_game_core::skill_buffs::ReceivedBuffs;

use crate::common::HOUND;
const INFUSION: &str = "records/skills/playerclass05/elementalinfusion1.dbr";
/// Its modifier.
const INFUSION_MODIFIER: &str = "records/skills/playerclass05/elementalinfusion2.dbr";
const INFUSION_BUFF: &str = "records/skills/playerclass05/elementalinfusion1_buff.dbr";

fn game() -> Option<(TestGame, Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 40.0);
    let p = g.spawn_player();
    Some((g, p))
}

fn summon(g: &mut TestGame, owner: Entity, skill: &str) -> Entity {
    g.app.world_mut().write_message(SpawnPet {
        owner,
        skill: skill.into(),
        level_index: 15,
        point: Vec3::new(2.0, 0.0, 2.0),
        target: None,
    });
    g.steps(3);
    let mut q = g.app.world_mut().query::<(Entity, &Pet)>();
    let w = g.app.world();
    q.iter(w)
        .filter(|(_, p)| p.owner == owner && p.summon == skill)
        .map(|(e, _)| e)
        .last()
        .expect("the pet spawned")
}

fn has_buff(w: &World, e: Entity, buff: &str) -> bool {
    w.get::<ReceivedBuffs>(e).is_some_and(|a| a.has(buff))
}

/// The player's Elemental Infusion aura buffs the pet: the buff with its
/// modifier's records counts in their profile; out of range it runs out
/// 1.5 s later.
#[test]
fn player_aura_buffs_the_pet() {
    let Some((mut g, p)) = game() else { return };
    let data = g.data();
    g.get_mut::<Loadout>(p)
        .skills
        .skills
        .extend([(INFUSION.into(), 1), (INFUSION_MODIFIER.into(), 1)]);
    g.steps(3);
    assert!(
        g.get::<Loadout>(p).skill_active(&data.db, INFUSION),
        "Elemental Infusion is on"
    );
    let hound = summon(&mut g, p, HOUND);
    let got = g.run_until(90, |w| has_buff(w, hound, INFUSION_BUFF));
    assert!(got.is_some(), "the infusion buff reaches the hound");
    let b = g
        .get::<ReceivedBuffs>(hound)
        .list
        .iter()
        .find(|b| b.buff == INFUSION_BUFF)
        .cloned()
        .unwrap();
    assert_eq!(b.from, p);
    assert!(
        b.mods
            .iter()
            .any(|m| m.0.ends_with("elementalinfusion2.dbr")),
        "the aura's modifiers travel with it: {:?}",
        b.mods
    );
    assert!(
        !has_buff(g.app.world(), p, INFUSION_BUFF),
        "the player's own aura is not its own aura buff"
    );
    // The aura gone (its points refunded: no user switch-off, the auto
    // toggle would switch it on again): no more passes, the buff runs out.
    g.get_mut::<Loadout>(p)
        .skills
        .skills
        .retain(|s| s.0 != INFUSION);
    let gone = g.run_until(150, |w| !has_buff(w, hound, INFUSION_BUFF));
    assert!(
        gone.is_some_and(|f| f >= 60),
        "it lasts 1.5 s after the last pass ({gone:?})"
    );
}
