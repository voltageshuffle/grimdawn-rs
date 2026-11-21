//! The base visuals of the channelled cone / spin / grow skills and the charged-scaling charge effect (their records:
//! [`crate::skill_fx`]'s `cone_effect` / `spin_aura` / `grow_effect` / `charge_aura`, which apply the modifiers'
//! `fxChanges`). Game.dll:
//! - Cone (`Skill_AttackSpellCone::ActivateNow` @0x1804cd370, `Update` @0x1804cdab0, `StopEmitting` @0x1804ce8b0,
//!   `GetValidTarget` @0x1804cd870, `GetMaxConeLength` @0x1804cc560, `Load` @0x1804cd0a0): the cone effect is created
//!   at the activation and starts emitting after `coneDelay` s (the main player's; others' at once). Every update
//!   while it runs it is set in the world (`World::SetCoords`, not attached) at the caster's `emitPointName` (default
//!   `L Hand`, `GetAttachPointCoords`), with the caster's coords whose forward (Z) row is `(fwd.x, slope, fwd.z)`:
//!   `slope` the y of the normalised aim point - emit point ([`cone_slope`]); the aim point is the target's `Target`
//!   point while it lives, else the used point raised by half the caster's height (`Actor::GetHeight`); each
//!   re-request retargets it (`GetValidTarget`, [`super::Channel::target`]). Its length grows `travelSpeed` m/s up
//!   to `GetMaxConeLength` ([`super::max_beam_length`]). The channel's stop puts it in state 3, then retracting
//!   (state 4): the placement stops (it stays where it was, still emitting) while the retract grows
//!   `travelSpeed * 0.002` m per ms until it reaches the length; then `StopEmitting` + `SetSelfRemove` ([`ConeFx`]).
//!   Strikes (`Update` / `GetTargets` @0x1804ce270): the entry's timer (`timeBetweenAttacks` / cast speed, armed by
//!   `ActivateNow` @0x1804cd370, which also pays the energy once) runs while it emits and while it retracts; each
//!   strike hits the foes in the frustum from the retract to the grown length ([`ConeFx`]) with no secondaries; the
//!   current (not retracting) entry pays each strike and stops the channel when the energy left is under the cost.
//! - Spin (`Skill_AttackRadiusSpin::ApplyCastVisualEffects` @0x1804c9c70, from `ActivateNow` @0x1804c9f20 / off from
//!   `StopSpinning` @0x1804ca6f0): `Character::AddParticleEffect(aura, "")` once (attached at the character's
//!   origin), `StartParticleEffect` while spinning, `RemoveParticleEffect` at the stop.
//! - Grow (`Skill_AttackRadiusGrow::ApplyCastVisualEffects` @0x1804c6a80, after the activation and each growth in
//!   `Update` @0x1804c7130, off from `StopEmitting` @0x1804c77c0): when the radius reaches another
//!   `radiusEffectSize` step the old effect stops emitting (self-removing) and the step's effect is added to the
//!   world at the caster's coords, rotation cleared (not attached); off stops it.
//! - Charge (`Skill_WeaponPool_ChargedScaling::SetChargeEffect` @0x180510c00, from `ReceiveActiveUpdate`
//!   @0x180511400 with the charge level): the effect shown stops (`StopParticleEffect`); with charges the level's
//!   effect is added once per name (`AddParticleEffect(name, "")`, cached) and started.
//!
//! The chaos skills' beams: `super::chaos`.
//!
//! PORT: channels are the player's ([`super::Channel`]); a charge effect is spawned anew at each change instead of
//! restarting a cached one. The cone's forward is the caster's rotation pitched by `atan(slope)`
//! ([`cone_rotation`]): the engine writes the unnormalised `(fwd.x, slope, fwd.z)` into the matrix, a shear a
//! `Transform` cannot hold (the same direction; the engine's axis is `sqrt(1 + slope²)` long and its up row stays the
//! caster's).
use super::*;
use crate::visuals::{BuffFx, EffectSpec, StopEmitting};

/// The height of the cone's damage frustum (`Skill_AttackSpellCone` +1848, set to 5.0 by the constructor
/// @0x1804cc920 and read by nothing else).
const CONE_HEIGHT: f32 = 5.0;

/// A cone entry (`Skill_AttackSpellCone`'s cone list, its own entity): created by the activation, it starts its
/// effect after the delay, grows, strikes and retracts (module doc). The one source of the cone's length.
#[derive(Component, Debug)]
pub struct ConeFx {
    owner: Entity,
    skill: String,
    level_index: usize,
    /// The cone effect record (`coneName` / `fxChanges`, resolved by the activation).
    record: String,
    /// The effect entity once emitting (state 2).
    effect: Option<Entity>,
    /// Seconds before it starts emitting (state 1).
    delay: f32,
    /// Seconds to the next strike.
    tick: f32,
    /// The grown length and the retract (m), `travelSpeed`.
    len: f32,
    retract: Option<f32>,
    speed: f32,
    /// The last placement (frozen once retracting): the emit point, the aim point (`+1736`), the effect's rotation,
    /// the caster's rotation and `GetMaxConeLength`.
    emit: Vec3,
    aim: Vec3,
    rotation: Quat,
    caster_rot: Quat,
    max: f32,
}

impl ConeFx {
    /// One retracting update of `dt` s (state 4: `dt` ms × 0.002 × `travelSpeed`): true when the retract reached
    /// the length (state 5).
    fn retract(&mut self, dt: f32) -> bool {
        let r = self.retract.get_or_insert(0.0);
        *r += dt * 2.0 * self.speed;
        if *r >= self.len {
            *r = self.len;
            return true;
        }
        false
    }

    /// The damage frustum (`GetTargets` @0x1804ce270): from `emit + dir * retract` to `emit + dir * len` along `dir`
    /// = the normalised aim - emit, `startWidth` wide at the near face and the width the length reached at the far
    /// one (`startWidth + len / maxRange * (endWidth - startWidth)`), [`CONE_HEIGHT`] high, faced along the
    /// caster's axes. `foe_box` meets it.
    fn strikes(&self, rec: &Record, foe_box: &gd_sim::bounds::Aabb) -> bool {
        let li = self.level_index;
        let (w0, w1, range) = (
            level_f(rec, "startWidth", li),
            level_f(rec, "endWidth", li),
            level_f(rec, "maxRange", li),
        );
        let far_w = if range > 0.0 {
            w0 + self.len / range * (w1 - w0)
        } else {
            w0
        };
        let dir = (self.aim - self.emit).normalize_or_zero();
        super::cone_frustum_meets_box(
            self.emit + dir * self.retract.unwrap_or(0.0),
            self.emit + dir * self.len,
            self.caster_rot * Vec3::Z,
            self.caster_rot * Vec3::X,
            self.caster_rot * Vec3::Y,
            w0,
            far_w,
            CONE_HEIGHT,
            foe_box,
        )
    }

    /// One running update (state 2): the length grows `travelSpeed` m/s up to `max` (`GetMaxConeLength`).
    fn grow(&mut self, dt: f32, max: f32) {
        if self.len < max {
            self.len += dt * self.speed;
        }
        if max <= self.len {
            self.len = max;
        }
    }
}

/// The cone's slope: the y of the normalised `aim - emit` (`Update` / `GetMaxConeLength`).
pub fn cone_slope(emit: Vec3, aim: Vec3) -> f32 {
    (aim - emit).normalize_or_zero().y
}

/// The cone's rotation: the caster's with its forward turned up to `slope` (module doc, PORT).
pub fn cone_rotation(caster: Quat, slope: f32) -> Quat {
    caster * Quat::from_rotation_x(-slope.atan())
}

/// The running channel's base visual on its caster (module doc).
#[derive(Component, Debug)]
pub struct ChannelFx {
    skill: String,
    /// The effect entity, once created (a cone: its [`ConeFx`] entry).
    fx: Option<Entity>,
    /// Grow: the step shown.
    step: Option<usize>,
    /// Cone: the caster's emit point.
    point: String,
}

/// The charge effect each charged-scaling skill shows on its caster: (skill, charges, effect).
#[derive(Component, Default, Debug)]
pub struct ChargeFx(Vec<(String, u32, Entity)>);

fn spawn_on(commands: &mut Commands, owner: Entity, skill: &str, record: &str, at: Vec3) -> Entity {
    crate::visuals::notify(commands, record);
    let f = commands
        .spawn((
            EffectSpec {
                despawn_when_complete: true,
                ..EffectSpec::record(record)
            },
            Transform::from_translation(at),
            BuffFx {
                owner,
                skill: skill.to_owned(),
                toggled: false,
                point: None,
            },
        ))
        .id();
    commands.entity(owner).add_child(f);
    f
}

/// The channel's visual ends: a cone entry retracts ([`ConeFx`]; one not emitting yet goes), the others stop
/// emitting.
fn stop(commands: &mut Commands, cones: &mut Cones, fx: Option<Entity>) {
    let Some(f) = fx else { return };
    match cones.get_mut(f) {
        Ok((_, mut c)) => c.retract = c.retract.or(Some(0.0)),
        Err(_) => {
            commands.entity(f).try_insert(StopEmitting);
        }
    }
}

type Cones<'w, 's> = Query<'w, 's, (Entity, &'static mut ConeFx)>;

/// The caster side of a cone's placement: its transform, character, emit point name, channel and the living
/// target's `Target` point.
struct ConeCaster<'a> {
    tf: &'a Transform,
    character: &'a Character,
    point: &'a str,
    ch: &'a Channel,
    target: Option<Vec3>,
}

/// The cone's placement this update (module doc): the emit point, the aim point, the effect's rotation and
/// `GetMaxConeLength`.
fn cone_frame(
    data: &GameData,
    coll: &crate::collision::WorldCollision,
    rec: &Record,
    c: ConeCaster,
) -> (Vec3, Vec3, Quat, f32) {
    let ConeCaster {
        tf,
        character,
        point,
        ch,
        target,
    } = c;
    let emit = tf.transform_point(character.posed_point(point).unwrap_or(Vec3::Y));
    let half_height = data
        .db
        .get(&character.record)
        .and_then(|r| r.f32("actorHeight"))
        .unwrap_or(0.0)
        * character.scale
        * 0.5;
    let range = level_f(rec, "maxRange", ch.level_index);
    let aim = target
        .or_else(|| ch.aim.map(|a| a + Vec3::Y * half_height))
        .unwrap_or(emit + tf.rotation * Vec3::Z * range.max(1.0));
    let rotation = cone_rotation(tf.rotation, cone_slope(emit, aim));
    let max = if range > 0.0 {
        max_beam_length(coll, emit, rotation * Vec3::Z, range).0
    } else {
        0.0
    };
    (emit, aim, rotation, max)
}

/// The strike period (s): `timeBetweenAttacks` (default 100 ms) / the spell-cast speed.
fn cone_period(rec: &Record, character: Option<&Character>) -> f32 {
    rec.f32("timeBetweenAttacks").unwrap_or(100.0)
        / 1000.0
        / character.map_or(1.0, |c| c.spell_speed.max(0.2))
}

/// The cone / spin / grow visual of the player's channel and the cone entries' strikes (module doc).
#[allow(clippy::too_many_arguments)]
pub(super) fn channel_fx(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    coll: Res<crate::collision::WorldCollision>,
    mut q: Query<(
        Entity,
        &Transform,
        &Character,
        Option<&Channel>,
        Option<&mut ChannelFx>,
        Option<&crate::weapon_pool::SkillLinks>,
        Option<&Loadout>,
        Option<&mut Caster>,
    )>,
    targets: Query<(&Transform, &Character), Without<Dead>>,
    mut cones: Cones,
    mut placed: Query<&mut Transform, (With<EffectSpec>, Without<Character>)>,
    // `FilterInvalidTargets`: the owner's foes ([`crate::combat::Side::hits`]).
    foes: Query<
        (
            Entity,
            &GlobalTransform,
            &Combatant,
            crate::combat::VictimSide,
        ),
        Without<Dead>,
    >,
    sides: Query<crate::combat::SideData>,
    mut hits: MessageWriter<SkillHit>,
) {
    let dt = time.delta_secs();
    for (e, tf, character, ch, fx, links, loadout, mut caster) in &mut q {
        let ch = ch.filter(|c| {
            data.db.get(&c.skill).is_some_and(|r| {
                matches!(
                    r.class.as_str(),
                    "Skill_AttackSpellCone" | "Skill_AttackRadiusSpin" | "Skill_AttackRadiusGrow"
                )
            })
        });
        let Some(ch) = ch else {
            if let Some(fx) = fx {
                stop(&mut commands, &mut cones, fx.fx);
                commands.entity(e).try_remove::<ChannelFx>();
            }
            continue;
        };
        let Some(rec) = data.db.get(&ch.skill) else {
            continue;
        };
        let mods =
            crate::skill_fx::override_mods(&data.db, links, loadout.map(|l| &l.skills), &ch.skill);
        // `GetValidTarget`: a living character target's `Target` point.
        let target = ch
            .target
            .and_then(|t| targets.get(t).ok())
            .map(|(ttf, tch)| ttf.transform_point(tch.posed_point("Target").unwrap_or(Vec3::Y)));
        let mut fx = match fx {
            Some(fx) if fx.skill == ch.skill => fx,
            other => {
                if let Some(old) = other {
                    stop(&mut commands, &mut cones, old.fx);
                }
                let mut new = ChannelFx {
                    skill: ch.skill.clone(),
                    fx: None,
                    step: None,
                    point: rec.str("emitPointName").unwrap_or("L Hand").to_owned(),
                };
                if rec.class == "Skill_AttackRadiusSpin"
                    && let Some(aura) = crate::skill_fx::spin_aura(rec, ch.level_index, &mods)
                {
                    new.fx = Some(spawn_on(&mut commands, e, &ch.skill, &aura, Vec3::ZERO));
                }
                // `Skill_AttackSpellCone::ActivateNow`: pays the energy once and adds the cone entry (none without
                // a cone effect record).
                if rec.class == "Skill_AttackSpellCone"
                    && let Some(record) = crate::skill_fx::cone_effect(rec, ch.level_index, &mods)
                {
                    let cost = level_f(rec, "skillManaCost", ch.level_index);
                    if let Some(c) = caster.as_deref_mut() {
                        if c.energy < cost {
                            commands.entity(e).try_remove::<Channel>();
                            continue;
                        }
                        c.energy -= cost;
                    }
                    let (emit, aim, rotation, max) = cone_frame(
                        &data,
                        &coll,
                        rec,
                        ConeCaster {
                            tf,
                            character,
                            point: &new.point,
                            ch,
                            target,
                        },
                    );
                    new.fx = Some(
                        commands
                            .spawn(ConeFx {
                                owner: e,
                                skill: ch.skill.clone(),
                                level_index: ch.level_index,
                                record,
                                effect: None,
                                delay: rec.f32("coneDelay").unwrap_or(0.0),
                                tick: cone_period(rec, Some(character)),
                                len: 0.0,
                                retract: None,
                                speed: rec.f32("travelSpeed").unwrap_or(0.0),
                                emit,
                                aim,
                                rotation,
                                caster_rot: tf.rotation,
                                max,
                            })
                            .id(),
                    );
                }
                commands.entity(e).try_insert(new);
                continue;
            }
        };
        match rec.class.as_str() {
            "Skill_AttackSpellCone" => {
                if let Some((_, mut c)) = fx.fx.and_then(|f| cones.get_mut(f).ok()) {
                    (c.emit, c.aim, c.rotation, c.max) = cone_frame(
                        &data,
                        &coll,
                        rec,
                        ConeCaster {
                            tf,
                            character,
                            point: &fx.point,
                            ch,
                            target,
                        },
                    );
                    c.caster_rot = tf.rotation;
                }
            }
            "Skill_AttackRadiusGrow" => {
                let radius = grow_radius(rec, ch.level_index, ch.held, character.spell_speed);
                let step = crate::skill_fx::grow_step(rec, radius);
                if fx.step != Some(step) {
                    fx.step = Some(step);
                    stop(&mut commands, &mut cones, fx.fx.take());
                    if let Some(n) = crate::skill_fx::grow_effect(rec, step, &mods) {
                        crate::visuals::notify(&mut commands, &n);
                        fx.fx = Some(
                            commands
                                .spawn((
                                    EffectSpec {
                                        despawn_when_complete: true,
                                        ..EffectSpec::record(n)
                                    },
                                    Transform::from_translation(tf.translation),
                                ))
                                .id(),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    // The cone entries (`Update`'s list walk).
    for (f, mut c) in &mut cones {
        let owner = q.get_mut(c.owner).ok();
        if owner.is_none() {
            c.retract = c.retract.or(Some(0.0));
        }
        let Some(rec) = data.db.get(&c.skill) else {
            commands.entity(f).try_despawn();
            continue;
        };
        // State 1: the delay, then the effect starts emitting where the cone is.
        let placement = Transform {
            translation: c.emit,
            rotation: c.rotation,
            ..default()
        };
        if c.effect.is_none() {
            c.delay -= dt;
            if c.retract.is_some() {
                commands.entity(f).try_despawn();
                continue;
            }
            if c.delay > 0.0 {
                continue;
            }
            crate::visuals::notify(&mut commands, &c.record);
            c.effect = Some(
                commands
                    .spawn((
                        EffectSpec {
                            despawn_when_complete: true,
                            ..EffectSpec::record(c.record.clone())
                        },
                        placement,
                    ))
                    .id(),
            );
        }
        if c.retract.is_some() {
            // State 4: the effect stays; at the length it stops emitting (state 5).
            if c.retract(dt) {
                if let Some(fx) = c.effect {
                    commands.entity(fx).try_insert(StopEmitting);
                }
                commands.entity(f).try_despawn();
                continue;
            }
        } else {
            // State 2: the length grows and the effect follows the caster.
            let max = c.max;
            c.grow(dt, max);
            if let Some(mut t) = c.effect.and_then(|fx| placed.get_mut(fx).ok()) {
                *t = placement;
            }
        }
        // The strike timer runs in states 2 and 4; the current entry pays each strike and stops the channel when
        // the energy left is under the cost.
        c.tick -= dt;
        if c.tick > 0.0 {
            continue;
        }
        let (_, otf, character, .., caster) = match owner {
            Some(o) => o,
            None => {
                c.tick = cone_period(rec, None);
                continue;
            }
        };
        c.tick = cone_period(rec, Some(character));
        c.caster_rot = otf.rotation;
        let side = crate::combat::Side::lookup(&sides, c.owner, false);
        for (m, gt, mcb, v) in &foes {
            if m != c.owner && side.hits(&v) && c.strikes(rec, &mcb.body_box(&gt.affine())) {
                hits.write(SkillHit {
                    attacker: c.owner,
                    target: m,
                    skill: c.skill.clone(),
                    level_index: c.level_index,
                    parent: None,
                    scale: 1.0,
                    projectile: false,
                });
            }
        }
        crate::visuals::notify(&mut commands, &c.skill);
        if c.retract.is_none()
            && let Some(mut caster) = caster
        {
            let cost = level_f(rec, "skillManaCost", c.level_index);
            caster.energy = (caster.energy - cost).max(0.0);
            if caster.energy < cost {
                commands.entity(c.owner).try_remove::<Channel>();
            }
        }
    }
}

/// The charged-scaling skills' charge effects (`SetChargeEffect`, module doc).
pub(super) fn charge_fx(
    mut commands: Commands,
    data: Res<GameData>,
    mut q: Query<(
        Entity,
        &crate::weapon_pool::PoolCharges,
        Option<&mut ChargeFx>,
        Option<&crate::weapon_pool::SkillLinks>,
        Option<&Loadout>,
    )>,
) {
    for (e, charges, shown, links, loadout) in &mut q {
        let want: Vec<(&String, u32)> = charges
            .0
            .iter()
            .filter(|(s, _)| {
                data.db
                    .get(s)
                    .is_some_and(|r| r.class == "Skill_WeaponPool_ChargedScaling")
            })
            .map(|(s, c)| (s, c.count))
            .collect();
        let Some(mut shown) = shown else {
            if want.iter().any(|w| w.1 > 0) {
                commands.entity(e).try_insert(ChargeFx::default());
            }
            continue;
        };
        // A skill whose charge changed (or is gone) stops its effect.
        shown.0.retain(|(s, n, f)| {
            let same = want.iter().any(|(w, c)| *w == s && c == n);
            if !same {
                commands.entity(*f).try_insert(StopEmitting);
            }
            same
        });
        for (skill, count) in want {
            if count == 0 || shown.0.iter().any(|(s, ..)| s == skill) {
                continue;
            }
            let Some(rec) = data.db.get(skill) else {
                continue;
            };
            let mods =
                crate::skill_fx::override_mods(&data.db, links, loadout.map(|l| &l.skills), skill);
            if let Some(aura) = crate::skill_fx::charge_aura(rec, count, &mods) {
                let f = spawn_on(&mut commands, e, skill, &aura, Vec3::ZERO);
                shown.0.push((skill.clone(), count, f));
            }
        }
    }
}
