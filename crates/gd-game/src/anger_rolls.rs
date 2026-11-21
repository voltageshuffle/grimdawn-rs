//! The controller's periodic anger rolls: `RandomAnger*` and `ignorePets*`.
//!
//! Game.dll `ControllerMonster::Update` @0x180134ec0 (outside the
//! Dying / Dead states), timers from the constructor @0x1801346c0, fields from
//! `ControllerMonster::Load` @0x180135e60:
//! * ignorePets: a timer (+0x6B0, 5000 ms at construction) runs down; at or
//!   below 0 it is set to `ignorePetsInterval` (ms, default 5000) and the flag
//!   (+0x6B4) re-rolled: `rand() % 101 < ignorePetsChance` (sub_180025C60(0,
//!   100), int %, default 0). `ShouldIgnorePets` @0x18013b780 = the flag
//!   unless `AngerManager::HasOnlyPetTargets` @0x18001e9c0 (no living
//!   character without a master on the anger list). While it holds:
//!   `AngerManager::GetNewTarget` @0x18001e1c0 skips the pets on the list
//!   (`FindEnemy` @0x18013b7b0), `AngerUpdate`'s sight sphere and
//!   `FindEnemyUpdate`'s too-close sphere leave pets out
//!   (`GetCharactersInSphere` @0x18013afe0, last argument), and
//!   `UnderAttack` @0x18013c8e0 does not pass a pet's hit on to its master:
//!   the pet gets both shares. Quest pets (`Skill_SpawnQuestPet`,
//!   `Character::SetQuestPet`: Character +0x2080) are never ignored; the port
//!   has none.
//! * RandomAnger: until it has once succeeded (+0x640), a timer (+0x638,
//!   3000 ms at construction) runs down; below 0 it is set to
//!   `RandomAngerEvaluationTime` (ms, default 3000) and, when `rand() % 100 <
//!   RandomAngerChance` (default 0) and the current state has no target
//!   (`GetCurrentStateData()` first dword), `PickRandomEnemyInView`
//!   @0x18013cb40 picks a uniformly random character among the living foes
//!   within `ViewDistance` (sphere) and `MaxYViewDistance` (when set) of the
//!   monster, pets left out while ignoring them: it gets 100 anger
//!   (`AngerManager::AddAnger`, × its anger multiplier) and the rolls stop for good. `FindEnemy` then
//!   takes it up.
use crate::combat::{Dead, MonsterAi};
use bevy::prelude::*;
use gd_formats::arz::Record;

/// The roll fields, timers and flags of a monster's controller.
#[derive(Clone, Debug)]
pub struct AngerRolls {
    /// `RandomAngerChance` (%), `RandomAngerEvaluationTime` (s).
    pub random_chance: i32,
    pub random_interval: f32,
    /// Seconds to the next random-anger roll; set once one succeeded.
    pub random_timer: f32,
    pub random_done: bool,
    /// `ignorePetsChance` (%), `ignorePetsInterval` (s).
    pub ignore_chance: i32,
    pub ignore_interval: f32,
    /// Seconds to the next re-roll and the rolled flag.
    pub ignore_timer: f32,
    pub ignore_pets: bool,
}

impl AngerRolls {
    pub fn from_controller(ctl: Option<&Record>) -> Self {
        let i = |k: &str, d: i32| ctl.and_then(|c| c.i32(k)).unwrap_or(d);
        Self {
            random_chance: i("RandomAngerChance", 0),
            random_interval: i("RandomAngerEvaluationTime", 3000) as f32 / 1000.0,
            random_timer: 3.0,
            random_done: false,
            ignore_chance: i("ignorePetsChance", 0),
            ignore_interval: i("ignorePetsInterval", 5000) as f32 / 1000.0,
            ignore_timer: 5.0,
            ignore_pets: false,
        }
    }
}

/// `ShouldIgnorePets`: the rolled flag, unless every foe on the anger list
/// is a pet (`is_pet`).
pub fn ignores_pets(ai: &MonsterAi, is_pet: impl Fn(Entity) -> bool) -> bool {
    ai.rolls.ignore_pets && ai.anger.iter().any(|(e, _)| !is_pet(*e))
}

/// The `Update` part of both rolls (runs before the monster AI).
pub fn update(
    time: Res<Time>,
    mut monsters: Query<(&Transform, &mut MonsterAi), Without<Dead>>,
    foe_q: Query<
        (Entity, &Transform, Option<&crate::pets::Pet>),
        (
            Or<(With<crate::player::Player>, With<crate::pets::Pet>)>,
            Without<Dead>,
            Without<MonsterAi>,
        ),
    >,
) {
    let dt = time.delta_secs();
    // (foe, position, a pet, its anger multiplier), gathered at the frame's first roll.
    let mut foe_list: Option<Vec<(Entity, Vec3, bool, f32)>> = None;
    for (tf, mut ai) in &mut monsters {
        let r = &mut ai.rolls;
        r.ignore_timer -= dt;
        if r.ignore_timer <= 0.0 {
            r.ignore_timer = r.ignore_interval;
            r.ignore_pets = (crate::rng::u32(..=100) as i32) < r.ignore_chance;
        }
        if r.random_done {
            continue;
        }
        r.random_timer -= dt;
        if r.random_timer >= 0.0 {
            continue;
        }
        r.random_timer = r.random_interval;
        if (crate::rng::u32(..100) as i32) >= r.random_chance || ai.enemy.is_some() {
            continue;
        }
        let me = tf.translation;
        let foes = foe_list.get_or_insert_with(|| {
            foe_q
                .iter()
                .map(|(e, t, p)| {
                    (
                        e,
                        t.translation,
                        p.is_some(),
                        p.map_or(1.0, |p| p.anger_multiplier),
                    )
                })
                .collect()
        });
        let skip_pets = ignores_pets(&ai, |e| foes.iter().any(|f| f.0 == e && f.2));
        let (view, view_y) = (ai.view, ai.view_y());
        let seen: Vec<(Entity, f32)> = foes
            .iter()
            .filter(|f| {
                !(skip_pets && f.2)
                    && f.1.distance(me) <= view
                    && (view_y <= 0.0 || (f.1.y - me.y).abs() <= view_y)
            })
            .map(|f| (f.0, f.3))
            .collect();
        if seen.is_empty() {
            continue;
        }
        let (pick, mult) = seen[crate::rng::usize(..seen.len())];
        debug!("random anger: {pick} picked");
        ai.add_anger(pick, 100.0 * mult);
        ai.rolls.random_done = true;
    }
}
