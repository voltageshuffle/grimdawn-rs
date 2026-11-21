//! `Skill_BuffSelfColossus` (the Forsaken's Overflowing Rage, the cultist
//! leaders' Possession): a timed self buff (a `Skill_BuffSelfDuration`, its
//! stats and absorption run through the ordinary `Kind::Buff` path) that
//! also grows the caster to `actorScale` while it lasts and back after.
//!
//! Game.dll:
//! - `SkillProfile::LoadProfile` @0x180546c27 reads `actorScaleTime` (s →
//!   ms, +0x4F8) and `actorScale` (+0x4FC), once per skill (not per level).
//! - `ReceiveActiveUpdate` @0x1804ea3f0 (the buff going on or off): turns the
//!   scaling on (+1592); going on, the goal is `actorScale` (an absolute
//!   scale, not × the actor's own), going off `Actor::GetBaseScale`; the time
//!   is `actorScaleTime` either way and the step the distance from
//!   `GetCurrentScale` to the goal (+1604). Then the usual self-buff cast
//!   (cost, activate sound, secondaries, visual effects).
//! - `Update` @0x1804ea390 calls `Scale` @0x1804ea930 every update while
//!   scaling is on: current ± dt / time × step toward the goal, clamped at it
//!   (reaching it turns scaling off), through `Actor::SetScale` (vtable
//!   +0x310). So the size changes linearly over `actorScaleTime` both ways.
//!
//! The port keeps the scale on the entity's `Transform` (the actor's scale,
//! `creature.rs`); [`Colossus`] runs the growth while the caster's timed buff
//! of the skill is listed (`MonsterSkills::buffs`, `Pet::buffs`).
//!
//! The radius follows the scale. `Actor::GetRadius` (Engine.dll
//! @0x18002b410, vtable +456): the length of the actor's scale vector x
//! `actorRadius`, so the reach / melee-distance radii
//! (`Character::GetTargetDistance` @0x18005df40, `NearTarget` @0x18005e0b0)
//! grow with the Colossus scale, while `Character::GetExtents` (spawn-set
//! spacing) does not scale. [`Combatant::radius`] is the radius at the
//! base scale (sqrt(3) x `scale` x `actorRadius`, `combat::get_radius`)
//! x scale / base scale.
use crate::combat::Combatant;
use crate::data::GameData;
use bevy::prelude::*;

pub struct ColossusPlugin;

impl Plugin for ColossusPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Grow>().add_systems(
            Update,
            (start, scale)
                .chain()
                .after(crate::casting::CastingSet)
                .before(crate::combat::CombatSet),
        );
    }
}

/// A `Skill_BuffSelfColossus` cast went off on `caster` (`casting::fire_casts`).
#[derive(Message, Clone, Debug)]
pub struct Grow {
    pub caster: Entity,
    pub skill: String,
}

/// The scaling of one caster (`Skill_BuffSelfColossus` +1592 … +1604).
#[derive(Component, Clone, Debug)]
pub struct Colossus {
    pub skill: String,
    /// `Actor::GetBaseScale`: what it shrinks back to.
    pub base: f32,
    /// The radius at the base scale (`combat::get_radius`).
    base_radius: f32,
    /// The scale it is going to.
    pub goal: f32,
    /// `actorScaleTime` (s).
    time: f32,
    /// The distance from the scale at the start to the goal.
    step: f32,
    /// The buff is on (the goal is `actorScale`).
    pub grown: bool,
    /// Scaling is on.
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

/// `Actor::GetRadius` at `scale` (see the module doc).
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
            // The buff went off: back to the base scale.
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
