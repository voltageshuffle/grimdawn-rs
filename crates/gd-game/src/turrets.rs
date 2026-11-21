//! `Skill_TurretFireControl` (Krieg's, Ekketzul's, the Witch Gods' and
//! Theodin's turrets, the Kraken's tentacles, Sylvarria's vines, Osyr's
//! tomb, the Beaver's wave towers, …): the boss orders the `Turret`
//! monsters around it to use one of their special skills.
//!
//! Game.dll:
//! - `LoadResources` @0x18050c270: `numToFire` (int), `specialSkillNum`
//!   (int), `findClosest`, `targetCharacter`, `searchRadius` (float),
//!   `turretName` (a record path), `useSpecialAnim`.
//! - `ActivateNow` @0x18050c370 (only with Ashes of Malmouth loaded, and the
//!   skill's launcher present): `GameEngine::GetTargetsInRadius` around the
//!   caster within `searchRadius` with the friend filter
//!   (`FilterInvalidTargets` @0x1802eca00: living characters, not NPCs,
//!   `FactionManager::IsFriend`), without the caster; of those the `Turret`s
//!   (RTTI) whose object name (record) equals `turretName` (sub_180593650:
//!   case-insensitive, `/` = `\`; an empty `turretName` takes every turret).
//!   `numToFire` < 0 orders them all, else min(n, found): the n nearest with
//!   `findClosest` (sub_1801515D0), else n drawn without replacement
//!   (sub_1804F3D30, as `geysers::pick`).
//! - Each turret aims at the skill's own target (point and object) with
//!   `targetCharacter`, else at 5 m along its own facing, no object. Its
//!   controller must be a `ControllerMonster`; `specialSkillNum` 1..7 picks
//!   its special slot (`specialAttackSkillName`, `specialAttack2SkillName`,
//!   …: controller +1304 + 28 × (n − 1), `Monster::InitSkillsInController`
//!   @0x18037a310), anything else nothing. With `useSpecialAnim` the
//!   controller gets an `AttackAction` with that skill (the cast animation
//!   plays first, `monster_skills::start`); without, an
//!   `ActivateSkillConfigCmd` ("RightHandHit") makes it go off at once
//!   (a [`ProcCast`]).
//!
//! The turrets are monsters placed straight in the levels (class `Turret`,
//! not proxies), streamed in with their level (`monsters::populate`,
//! `LevelProxies::placed`). `Character::Load` @0x180054e70 (through
//! `Monster::Load`): a character placed in a level file takes its record's
//! `charLevel` equation at the players' average level, clamped to
//! `minLevel` / `maxLevel` ([`crate::monsters::placed_char_level`]), for its
//! stats, skills (`SkillManager::LoadSkills`) and rewards; no area level is
//! consulted. Turrets a proxy or skill spawns keep their spawner's level.
//! The game-balance pak column (`Monster::OnGameBalanceLevel`) is the global
//! difficulty column every monster uses. PORT: the random pick uses the
//! port's RNG; a slot whose skill the port cannot cast is skipped.
use crate::casting::{Casting, ProcCast, kind_of};
use crate::character::Character;
use crate::combat::{Dead, MonsterAi};
use crate::data::GameData;
use crate::monsters::Monster;
use bevy::prelude::*;

pub struct TurretsPlugin;

impl Plugin for TurretsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Command>().add_systems(
            Update,
            command
                .after(crate::casting::CastingSet)
                .before(crate::combat::CombatSet),
        );
    }
}

/// A `Skill_TurretFireControl` cast went off (`casting::fire_casts`): the
/// skill's target point and object.
#[derive(Message, Clone, Debug)]
pub struct Command {
    pub caster: Entity,
    pub skill: String,
    pub point: Vec3,
    pub target: Option<Entity>,
}

/// sub_180593650: record names equal ignoring case and slash direction.
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
            |len| crate::rng::u32(..len as u32) as usize,
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
                crate::casting::Kind::Unsupported
                    | crate::casting::Kind::Toggle
                    | crate::casting::Kind::Beam
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
                    tf.translation + crate::character::facing_flat(&tf) * 5.0,
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
