//! Pet orders (`gd_game::pets` module docs): the Pet Attack skill sets the
//! pet's target.
use bevy::prelude::*;
use gd_game::casting::{CastRequest, Caster, SpawnPet};
use gd_game::combat::{Combatant, Invincible};
use gd_game::pets::Pet;
use gd_game::player::Loadout;
use gd_game::testing::TestGame;

use crate::common::{HOUND, ZOMBIE};
/// The Hellhound modifier that gives it Hellfire Aura (its `buffSelfSkillName`).
const HELLFIRE_MODIFIER: &str = "records/skills/playerclass03/summon_hellhound3_petmodifier.dbr";
const PET_ATTACK: &str = "records/skills/default/defaultpetattack.dbr";

/// A new character (invincible: the zombies must not end the session) with
/// the Hellhound and its Hellfire modifier learned.
fn game() -> Option<(TestGame, Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 30.0);
    let p = g.spawn_player();
    g.app.world_mut().entity_mut(p).insert(Invincible);
    g.get_mut::<Loadout>(p)
        .skills
        .skills
        .extend([(HOUND.into(), 1), (HELLFIRE_MODIFIER.into(), 1)]);
    g.step();
    Some((g, p))
}

/// Summons the Hellhound at the player (`selfTarget`, one per cast:
/// `gd_game::pets::placement`) and returns it.
fn summon(g: &mut TestGame, owner: Entity) -> Entity {
    g.app.world_mut().write_message(SpawnPet {
        owner,
        skill: HOUND.into(),
        level_index: 0,
        point: Vec3::new(2.0, 0.0, 2.0),
        target: None,
    });
    g.steps(3);
    let mut q = g.app.world_mut().query::<(Entity, &Pet)>();
    let w = g.app.world();
    let mut pets: Vec<(Entity, f32)> = q
        .iter(w)
        .filter(|(_, p)| p.owner == owner)
        .map(|(e, p)| (e, p.spawned_at))
        .collect();
    pets.sort_by(|a, b| a.1.total_cmp(&b.1));
    pets.last().expect("the hound spawned").0
}

fn pos(g: &TestGame, e: Entity) -> Vec3 {
    g.get::<Transform>(e).translation
}

/// The Pet Attack skill on the hotbar, cast at a monster: the hound leaves
/// the nearer zombie it was angry at for the commanded one and keeps it.
#[test]
fn pet_attack_skill_sets_the_target() {
    let Some((mut g, p)) = game() else { return };
    let hound = summon(&mut g, p);
    let near = g.spawn_monster(ZOMBIE, 54, Vec3::new(5.0, 0.0, 0.0));
    let far = g.spawn_monster(ZOMBIE, 54, Vec3::new(-9.0, 0.0, 0.0));
    let picked = g.run_until(120, |w| {
        w.get::<Combatant>(hound).unwrap().target == Some(near)
    });
    assert!(
        picked.is_some(),
        "the hound goes for the nearer zombie first"
    );
    // Finding the zombie, it switches Hellfire Aura on (its self buff, with
    // the cast animation) first.
    let buffed = g.run_until(300, |w| {
        !w.get::<Pet>(hound).unwrap().toggled().is_empty()
            && w.get::<gd_game::character::Character>(hound).unwrap().state
                != gd_game::character::CharState::Cast
    });
    assert!(buffed.is_some(), "the hound's self buff");
    g.get_mut::<Caster>(p).slots[3] = Some(PET_ATTACK.into());
    let at = pos(&g, far);
    g.app.world_mut().write_message(CastRequest {
        slot: 3,
        target: Some(far),
        point: at,
        force: false,
        scripted: false,
    });
    g.steps(2);
    assert_eq!(g.get::<Pet>(hound).most_hated(), Some(far));
    let start = pos(&g, hound).distance(pos(&g, far));
    g.steps(40);
    assert_eq!(
        g.get::<Combatant>(hound).target,
        Some(far),
        "the commanded zombie stays the target"
    );
    assert!(
        pos(&g, hound).distance(pos(&g, far)) < start - 1.0,
        "the hound runs at it"
    );
}
