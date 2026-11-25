//! `Skill_TurretFireControl` (Krieg's, the Witch Gods', the Kraken's tentacles, …): the boss orders the `Turret`
//! monsters around it to use one of their special skills.
//!
//! Game.dll:
//! - `LoadResources`: `numToFire`, `specialSkillNum`, `findClosest`, `targetCharacter`, `searchRadius`,
//!   `turretName`, `useSpecialAnim`.
//! - `ActivateNow` (Ashes of Malmouth loaded, launcher present): the friends within `searchRadius` of the caster
//!   (`GameEngine::GetTargetsInRadius`, `FilterInvalidTargets`), without the caster; of those the `Turret`s whose
//!   record equals `turretName` (`Game::RecordNamesEqual`; empty: every turret). `numToFire` < 0 orders them all,
//!   else min(n, found): the n nearest with `findClosest`, else n drawn without replacement
//!   (`Game::DrawWithoutReplacement`).
//! - Each turret aims at the skill's own target with `targetCharacter`, else 5 m along its own facing, no object.
//!   `specialSkillNum` 1..7 picks its `ControllerMonster` special slot (`specialSkills[n − 1]`), anything else
//!   nothing. With `useSpecialAnim` the controller gets an `AttackAction` with that skill (the cast animation plays
//!   first); without, `ActivateSkillConfigCmd` ("RightHandHit") fires it at once (a [`ProcCast`]).
//!
//! Turrets are monsters placed straight in the levels (class `Turret`, not proxies). A placed character takes its
//! record's `charLevel` equation at the players' average level, clamped to `minLevel` / `maxLevel`
//! ([`crate::monsters::placed_char_level`]); turrets a proxy or skill spawns keep their spawner's level. PORT: the
//! random pick uses the port's RNG; a slot whose skill the port cannot cast is skipped.
use crate::combat::MonsterAi;
use bevy::prelude::*;
use gd_game_core::casting::{Casting, ProcCast, kind_of};
use gd_game_core::character::Character;
use gd_game_core::combat::Dead;
use gd_game_core::data::GameData;
use gd_game_core::monsters::Monster;

pub struct TurretsPlugin;

impl Plugin for TurretsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Command>().add_systems(
            Update,
            command
                .after(crate::casting::CastingSet)
                .before(gd_game_core::combat::CombatSet),
        );
    }
}

/// A `Skill_TurretFireControl` cast went off: the skill's target point and object.
#[derive(Message, Clone, Debug)]
pub struct Command {
    pub caster: Entity,
    pub skill: String,
    pub point: Vec3,
    pub target: Option<Entity>,
}

/// `Game::RecordNamesEqual`: record names equal ignoring case and slash direction.
fn same_record(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes().zip(b.bytes()).all(|(x, y)| {
            x.eq_ignore_ascii_case(&y) || (matches!(x, b'/' | b'\\') && matches!(y, b'/' | b'\\'))
        })
}

#[allow(clippy::type_complexity)]
fn command(
    mut commands: Commands,
    mut msgs: MessageReader<Command>,
    data: Res<GameData>,
    mut q: Query<(Entity, &mut Transform, &Monster, &MonsterAi, &mut Character), Without<Dead>>,
    mut procs: MessageWriter<ProcCast>,
) {
    for m in msgs.read() {
        let Some(rec) = data.db.get(&m.skill) else {
            continue;
        };
        let Ok((_, ctf, _, cai, _)) = q.get(m.caster) else {
            continue;
        };
        let (centre, faction) = (ctf.translation, cai.faction.clone());
        let radius = rec.f32("searchRadius").unwrap_or(0.0);
        let name = rec.str("turretName").unwrap_or("").to_owned();
        let found: Vec<(f32, Entity)> = q
            .iter()
            .filter(|(e, tf, mon, ai, _)| {
                let friend = match (&faction, &ai.faction) {
                    (Some(a), Some(b)) => a.is_friend(b),
                    _ => true,
                };
                *e != m.caster
                    && friend
                    && tf.translation.distance(centre) <= radius
                    && data
                        .db
                        .get(&mon.record)
                        .is_some_and(|r| r.class == "Turret")
                    && (name.is_empty() || same_record(&mon.record, &name))
            })
            .map(|(e, tf, ..)| (tf.translation.distance(centre), e))
            .collect();
        // `geysers::pick` on the entities (as their index).
        let order: Vec<(f32, Vec3)> = found
            .iter()
            .enumerate()
            .map(|(i, (d, _))| (*d, Vec3::X * i as f32))
            .collect();
        let picked: Vec<Entity> = crate::geysers::pick(
            order,
            rec.i32("numToFire").unwrap_or(0),
            rec.bool("findClosest"),
            |len| gd_game_core::rng::u32(..len as u32) as usize,
        )
        .into_iter()
        .map(|v| found[v.x as usize].1)
        .collect();
        let slot = rec.i32("specialSkillNum").unwrap_or(0);
        if !(1..=7).contains(&slot) {
            continue;
        }
        let key = if slot == 1 {
            "specialAttackSkillName".to_owned()
        } else {
            format!("specialAttack{slot}SkillName")
        };
        let (aim_target, anim) = (rec.bool("targetCharacter"), rec.bool("useSpecialAnim"));
        debug!(
            "turret control {}: {} of {} turrets",
            m.skill.rsplit('/').next().unwrap_or(""),
            picked.len(),
            found.len()
        );
        for t in picked {
            let Ok((_, mut tf, mon, _, mut ch)) = q.get_mut(t) else {
                continue;
            };
            let Some(trec) = data.db.get(&mon.record) else {
                continue;
            };
            let Some(skill) = trec.str(&key).filter(|s| !s.is_empty()) else {
                continue;
            };
            let levels = gd_sim::actor::monster_skill_levels(trec, mon.level);
            let Some((skill, level)) = levels.iter().find(|(n, _)| n.eq_ignore_ascii_case(skill))
            else {
                continue;
            };
            let Some(srec) = data.db.get(skill) else {
                continue;
            };
            let kind = kind_of(srec);
            if matches!(
                kind,
                gd_game_core::casting::Kind::Unsupported
                    | gd_game_core::casting::Kind::Toggle
                    | gd_game_core::casting::Kind::Beam
            ) {
                debug!(
                    "turret {}: {skill} ({}) not supported",
                    mon.record, srec.class
                );
                continue;
            }
            let (point, target) = if aim_target {
                (m.point, m.target)
            } else {
                (
                    tf.translation + gd_game_core::character::facing_flat(&tf) * 5.0,
                    None,
                )
            };
            let li = (*level).max(1) as usize - 1;
            if anim {
                crate::monster_skills::start(
                    &mut commands,
                    &data,
                    t,
                    &mut tf,
                    &mut ch,
                    skill,
                    li,
                    kind,
                    point,
                    target,
                );
            } else {
                procs.write(ProcCast {
                    caster: t,
                    cast: Casting {
                        skill: skill.clone(),
                        level_index: li,
                        kind,
                        point,
                        target,
                        hostile: true,
                        parent: None,
                    },
                });
            }
        }
    }
}
