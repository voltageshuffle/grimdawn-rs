//! `Skill_BuffSelfColossus` (e.g. the Forsaken's Overflowing Rage): a `Skill_BuffSelfDuration` that also
//! grows the caster to `actorScale` while it lasts and back after.
//!
//! Game.dll:
//! - `SkillProfile::LoadProfile` reads `actorScaleTime` (s) and `actorScale` once per skill (not per level).
//! - `ReceiveActiveUpdate` (buff on or off) turns scaling on: the goal is `actorScale` (absolute, not × the
//!   actor's own) going on, `Actor.baseScale` going off; the step is the distance from `GetCurrentScale`.
//! - `Update` calls `Scale` while scaling: current ± dt / time × step toward the goal, clamped (reaching it
//!   stops), so the size changes linearly over `actorScaleTime` both ways.
//!
//! The scale lives on the entity's `Transform`; [`Colossus`] runs while the caster's timed buff is listed.
//! The radius follows the scale (`Actor::GetRadius`: the scale vector's length x `actorRadius`), so the reach
//! radii grow, while `Character::GetExtents` (spawn-set spacing) does not: [`Combatant::radius`] is the base
//! radius x scale / base scale.
use bevy::prelude::*;
use gd_game_core::combat::Combatant;
use gd_game_core::data::GameData;

pub struct ColossusPlugin;

impl Plugin for ColossusPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Grow>().add_systems(
            Update,
            (start, scale)
                .chain()
                .after(crate::casting::CastingSet)
                .before(gd_game_core::combat::CombatSet),
        );
    }
}

/// A `Skill_BuffSelfColossus` cast went off on `caster`.
#[derive(Message, Clone, Debug)]
pub struct Grow {
    pub caster: Entity,
    pub skill: String,
}

/// `Skill_BuffSelfColossus.scaling` … `scaleStep`.
#[derive(Component, Clone, Debug)]
pub struct Colossus {
    pub skill: String,
    /// `Actor.baseScale`: what it shrinks back to.
    pub base: f32,
    base_radius: f32,
    pub goal: f32,
    /// `actorScaleTime` (s).
    time: f32,
    /// The distance from the start scale to the goal.
    step: f32,
    /// The buff is on (the goal is `actorScale`).
    pub grown: bool,
    /// `Skill_BuffSelfColossus.scaling`.
    moving: bool,
}

fn record_values(data: &GameData, skill: &str) -> Option<(f32, f32)> {
    let r = data.db.get(skill)?;
    Some((
        r.f32("actorScale").unwrap_or(0.0),
        r.f32("actorScaleTime").unwrap_or(0.0),
    ))
}

fn start(
    mut commands: Commands,
    mut msgs: MessageReader<Grow>,
    data: Res<GameData>,
    mut q: Query<(&Transform, Option<&mut Colossus>, Option<&Combatant>)>,
) {
    for m in msgs.read() {
        let Some((goal, time)) = record_values(&data, &m.skill) else {
            continue;
        };
        let Ok((tf, c, cb)) = q.get_mut(m.caster) else {
            continue;
        };
        let current = tf.scale.x;
        match c {
            Some(mut c) => {
                c.skill = m.skill.clone();
                c.goal = goal;
                c.time = time;
                c.step = (goal - current).abs();
                c.grown = true;
                c.moving = true;
            }
            None => {
                commands.entity(m.caster).try_insert(Colossus {
                    skill: m.skill.clone(),
                    base: current,
                    base_radius: cb.map_or(0.0, |c| c.radius),
                    goal,
                    time,
                    step: (goal - current).abs(),
                    grown: true,
                    moving: true,
                });
            }
        }
        debug!("colossus {}: {current} -> {goal} in {time} s", m.caster);
    }
}

/// `Actor::GetRadius` at `scale`.
fn radius_at(base_radius: f32, scale: f32, base: f32) -> f32 {
    if base > 0.0 {
        base_radius * scale / base
    } else {
        base_radius
    }
}

/// `Scale`: one update's move toward the goal; true once it is reached.
fn step_toward(current: f32, goal: f32, dt: f32, time: f32, step: f32) -> (f32, bool) {
    let d = if time > 0.0 {
        dt / time * step
    } else {
        f32::INFINITY
    };
    if goal > current {
        let n = current + d;
        if n < goal { (n, false) } else { (goal, true) }
    } else if goal < current {
        let n = current - d;
        if n > goal { (n, false) } else { (goal, true) }
    } else {
        (goal, true)
    }
}

fn scale(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(
        Entity,
        &mut Transform,
        &mut Colossus,
        Option<&mut Combatant>,
        Option<&crate::monster_skills::MonsterSkills>,
        Option<&crate::pets::Pet>,
    )>,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut c, cb, ms, pet) in &mut q {
        let running = ms.is_some_and(|m| m.buffs.list.iter().any(|b| b.0 == c.skill))
            || pet.is_some_and(|p| p.buffs().iter().any(|b| b.0 == c.skill));
        if c.grown && !running {
            c.grown = false;
            c.goal = c.base;
            c.step = (c.base - tf.scale.x).abs();
            c.moving = true;
        }
        if c.moving {
            let (n, done) = step_toward(tf.scale.x, c.goal, dt, c.time, c.step);
            tf.scale = Vec3::splat(n);
            c.moving = !done;
        }
        if let Some(mut cb) = cb {
            cb.radius = radius_at(c.base_radius, tf.scale.x, c.base);
        }
        if !c.grown && !c.moving {
            commands.entity(e).try_remove::<Colossus>();
        }
    }
}
