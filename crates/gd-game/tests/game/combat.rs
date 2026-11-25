//! Headless fights on flat ground: a monster attacks the player, the
//! player kills a monster, casts land.
use bevy::prelude::*;
use gd_game::combat::MonsterKilled;
use gd_game::testing::TestGame;
use gd_game_core::casting::CastRequest;
use gd_game_core::casting::Caster;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::player::Loadout;

use crate::common::ZOMBIE;

/// Panetti's Replicating Missile.
const MISSILE: &str = "records/skills/playerclass05/arcanemissile.dbr";

fn game() -> Option<(TestGame, Entity)> {
    let mut g = TestGame::new_character("Tester", false, false)?;
    g.flat_ground(Vec3::ZERO, 24.0);
    let p = g.spawn_player();
    Some((g, p))
}

fn killed(w: &World, e: Entity) -> bool {
    w.resource::<Messages<MonsterKilled>>()
        .iter_current_update_messages()
        .any(|m| m.entity == e)
}

/// A level-54 zombie next to the player notices it and hits it.
#[test]
fn monster_attacks_player() {
    let Some((mut g, p)) = game() else { return };
    let z = g.spawn_monster(ZOMBIE, 54, Vec3::new(2.0, 0.0, 0.0));
    let full = g.get::<Combatant>(p).life;
    let hit = g.run_until(900, |w| w.get::<Combatant>(p).unwrap().life < full);
    assert!(hit.is_some(), "the zombie never hit");
    assert_eq!(
        g.get::<Combatant>(z).target,
        Some(p),
        "the zombie targets the player"
    );
}

/// The player holds the attack on a weak zombie (every frame asks for the next use) until it dies.
#[test]
fn player_kills_monster() {
    let Some((mut g, _)) = game() else { return };
    let z = g.spawn_monster(ZOMBIE, 1, Vec3::new(2.0, 0.0, 0.0));
    let mut died = false;
    let frames = g.run_until(900, |w| {
        died |= killed(w, z);
        w.write_message(gd_game::player_orders::PlayerOrder::Attack(z));
        w.get::<Dead>(z).is_some()
    });
    assert!(frames.is_some(), "the zombie survived");
    assert!(died, "MonsterKilled was sent");
}

/// A hotbar cast at a monster spends energy and kills it.
#[test]
fn cast_request_kills_monster() {
    let Some((mut g, p)) = game() else { return };
    g.get_mut::<Loadout>(p)
        .skills
        .skills
        .push((MISSILE.into(), 5));
    g.get_mut::<Caster>(p).slots[0] = Some(MISSILE.into());
    g.steps(2);
    let z = g.spawn_monster(ZOMBIE, 1, Vec3::new(6.0, 0.0, 0.0));
    let energy = g.get::<Caster>(p).energy;
    g.app.world_mut().write_message(CastRequest {
        slot: 0,
        target: Some(z),
        point: Vec3::new(6.0, 0.0, 0.0),
        force: false,
        scripted: false,
    });
    let spent = g.run_until(120, |w| w.get::<Caster>(p).unwrap().energy < energy);
    assert!(spent.is_some(), "no energy spent");
    let frames = g.run_until(600, |w| w.get::<Dead>(z).is_some());
    assert!(frames.is_some(), "the missile never killed the zombie");
}
