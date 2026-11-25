//! The headless game repeats exactly for a seed; another seed plays differently.
//! A non-reproducing mismatch points at hash-map iteration order in the combat inputs.
use bevy::prelude::*;
use gd_game::testing::TestGame;
use gd_game_core::combat::Combatant;

use crate::common::ZOMBIE;

/// Per frame: the player's life, each zombie's life and position (mm).
fn fight(seed: u64, frames: usize) -> Option<Vec<Vec<i64>>> {
    let mut g = TestGame::new_character_seeded(seed)?;
    g.flat_ground(Vec3::ZERO, 24.0);
    let p = g.spawn_player();
    // Level 3: their hits do not one-shot the level-1 character, so the damage rolls show.
    let zs: Vec<Entity> = (0..3)
        .map(|i| g.spawn_monster(ZOMBIE, 3, Vec3::new(3.0 + i as f32, 0.0, i as f32 - 1.0)))
        .collect();
    let mut trace = Vec::new();
    for _ in 0..frames {
        g.step();
        let w = g.app.world();
        let mut row = vec![(w.get::<Combatant>(p).unwrap().life * 1000.0) as i64];
        for z in &zs {
            match (w.get::<Combatant>(*z), w.get::<Transform>(*z)) {
                (Some(c), Some(t)) => row.extend([
                    (c.life * 1000.0) as i64,
                    (t.translation.x * 1000.0) as i64,
                    (t.translation.z * 1000.0) as i64,
                ]),
                _ => row.push(-1),
            }
        }
        trace.push(row);
    }
    Some(trace)
}

#[test]
fn same_seed_same_fight() {
    let Some(a) = fight(7, 150) else { return };
    let b = fight(7, 150).unwrap();
    if let Some(i) = a.iter().zip(&b).position(|(x, y)| x != y) {
        panic!("frame {i} differs: {:?} vs {:?}", a[i], b[i]);
    }
    assert_ne!(a.first(), a.last(), "nothing moved or got hurt");
    // No frame is pinned (unordered systems shift the first hit), and one seed may roll the
    // same outcome on the window's only swing, so some seed of a handful must differ.
    let differs = (1..=8)
        .filter(|&s| s != 7)
        .any(|s| fight(s, 150).unwrap() != a);
    assert!(differs, "the seed changes nothing");
}
