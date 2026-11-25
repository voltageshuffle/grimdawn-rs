//! The controller's periodic anger rolls: `RandomAnger*` and `ignorePets*`.
//!
//! Game.dll `ControllerMonster::Update` (outside the
//! Dying / Dead states), timers from the constructor, fields from
//! `ControllerMonster::Load`:
//! * ignorePets: a timer (`ControllerMonster.ignorePetsTimer`, 5000 ms at construction) runs down; at or
//!   below 0 it is set to `ignorePetsInterval` (ms, default 5000) and the flag
//!   (`ControllerMonster.ignorePets`) re-rolled: `rand() % 101 < ignorePetsChance` (`Game::RandomInt(0,
//!   100)`, int %, default 0). `ShouldIgnorePets` = the flag
//!   unless `AngerManager::HasOnlyPetTargets` (no living
//!   character without a master on the anger list). While it holds:
//!   `AngerManager::GetNewTarget` skips the pets on the list
//!   (`FindEnemy`), `AngerUpdate`'s sight sphere and
//!   `FindEnemyUpdate`'s too-close sphere leave pets out
//!   (`GetCharactersInSphere`, last argument), and
//!   `UnderAttack` does not pass a pet's hit on to its master:
//!   the pet gets both shares. Quest pets (`Skill_SpawnQuestPet`,
//!   `Character::SetQuestPet`: `Character.questPet`) are never ignored; the port
//!   has none.
//! * RandomAnger: until it has once succeeded (`ControllerMonster.randomAngerDone`), a timer
//!   (`ControllerMonster.randomAngerTimer`,
//!   3000 ms at construction) runs down; below 0 it is set to
//!   `RandomAngerEvaluationTime` (ms, default 3000) and, when `rand() % 100 <
//!   RandomAngerChance` (default 0) and the current state has no target
//!   (`GetCurrentStateData()` first dword), `PickRandomEnemyInView`
//!   picks a uniformly random character among the living foes
//!   within `ViewDistance` (sphere) and `MaxYViewDistance` (when set) of the
//!   monster, pets left out while ignoring them: it gets 100 anger
//!   (`AngerManager::AddAnger`, × its anger multiplier) and the rolls stop for good. `FindEnemy` then
//!   takes it up.
use crate::combat::MonsterAi;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_game_core::combat::Dead;

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

/// The living characters (`Character::IsAlive`): [`ignores_pets`]' test of an anger-list entry.
pub type Living<'w, 's> = Query<'w, 's, (), (With<gd_game_core::combat::Combatant>, Without<Dead>)>;

/// `ShouldIgnorePets`: the rolled flag, unless `AngerManager::HasOnlyPetTargets` (no entry on the anger list
/// is a living character without a master: a dead or despawned target left on the list until the next
/// `AngerUpdate` tick does not count; `is_pet` tells the pets).
pub fn ignores_pets(ai: &MonsterAi, is_pet: impl Fn(Entity) -> bool, living: &Living) -> bool {
    ai.rolls.ignore_pets
        && ai
            .anger
            .iter()
            .any(|(e, _)| !is_pet(*e) && living.contains(*e))
}

/// The `Update` part of both rolls (runs before the monster AI).
pub fn update(
    time: Res<Time>,
    mut monsters: Query<(&Transform, &mut MonsterAi), Without<Dead>>,
    foe_q: Query<
        (Entity, &Transform, Option<&crate::pets::Pet>),
        (
            Or<(With<gd_game_core::player::Player>, With<crate::pets::Pet>)>,
            Without<Dead>,
            Without<MonsterAi>,
        ),
    >,
    living: Living,
) {
    let dt = time.delta_secs();
    // (foe, position, a pet, its anger multiplier), gathered at the frame's first roll.
    let mut foe_list: Option<Vec<(Entity, Vec3, bool, f32)>> = None;
    for (tf, mut ai) in &mut monsters {
        let r = &mut ai.rolls;
        r.ignore_timer -= dt;
        if r.ignore_timer <= 0.0 {
            r.ignore_timer = r.ignore_interval;
            r.ignore_pets = (gd_game_core::rng::u32(..=100) as i32) < r.ignore_chance;
        }
        if r.random_done {
            continue;
        }
        r.random_timer -= dt;
        if r.random_timer >= 0.0 {
            continue;
        }
        r.random_timer = r.random_interval;
        if (gd_game_core::rng::u32(..100) as i32) >= r.random_chance || ai.enemy.is_some() {
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
        let skip_pets = ignores_pets(&ai, |e| foes.iter().any(|f| f.0 == e && f.2), &living);
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
        let (pick, mult) = seen[gd_game_core::rng::usize(..seen.len())];
        debug!("random anger: {pick} picked");
        ai.add_anger(pick, 100.0 * mult);
        ai.rolls.random_done = true;
    }
}
