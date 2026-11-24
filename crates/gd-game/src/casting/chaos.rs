//! The chaos skills' beams (`Skill_AttackSpellChaos` / `…ChaosSpawnPet`: Reap Spirit, the tendril item skills, the
//! aether zaps): the beam's travel and its hit when the head arrives. gd-render `chaos_beam.rs` draws each
//! [`ChaosBeam`] from its fields (one source for the head and tail; it never recomputes them).
//!
//! Game.dll: `ActivateNow` adds an 80-byte entry for
//! the target and sends its attack with an empty target list (no hit). `Update` runs the entries:
//! - state 0: the beam record (the modifiers' first `fxChanges` entry, else `chaosBeamName`;
//!   [`gd_game_core::skill_fx::chaos_beam`]) is created from the caster's `R Hand` (read once, here) to the target's
//!   `Target` point; a target that is no longer a character ends the entry (state 3, no hit);
//! - state 1: while the target resolves (`ResolveValidateTarget`, alive or not) the far end follows its `Target`
//!   point and the head moves `dt * headVelocity` m (clamped to 0..length), `top = head / length`; at `top >= 1` the
//!   far end is kept, the state is 2 and the skill manager sends the attack on the target (`TargetResult`: the
//!   damage, `Skill_AttackSpellChaosSpawnPet`'s summons by the target) and `Skill::ActivateSecondarySkills`
//!   runs with the target at its `Target` point. A target gone first moves the beam to state 2 without a hit;
//! - state 2: a living target (`IsAliveOrIntact`) still moves the far end; the tail moves `dt * tailVelocity` m,
//!   `bottom = tail / length`, and at `bottom >= top` the beam goes (state 3, `ChaosBeam::Liberate`).
//!
//! `headVelocity` / `tailVelocity` are plain floats of the skill record (`Load`, default 0).
//! A beam record that does not load leaves the entry without a beam: it never hits (the port drops it).
use super::*;

/// A character's attach point in the world (`GetAttachPointCoords`), `fallback` (local) without one.
fn point_of(tf: &Transform, ch: Option<&Character>, name: &str, fallback: Vec3) -> Vec3 {
    tf.transform_point(ch.and_then(|c| c.posed_point(name)).unwrap_or(fallback))
}

/// Runs the chaos beams (module doc).
#[allow(clippy::too_many_arguments)]
pub(super) fn chaos_beams(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut beams: Query<(Entity, &mut ChaosBeam)>,
    points: Query<(&Transform, Option<&Character>, Has<Dead>)>,
    mut hits: MessageWriter<SkillHit>,
    mut pets: MessageWriter<SpawnPet>,
    mut activations: MessageWriter<crate::secondary::Activate>,
) {
    let dt = time.delta_secs();
    for (e, mut b) in &mut beams {
        let target = points
            .get(b.target)
            .ok()
            .map(|(tf, ch, dead)| (point_of(tf, ch, "Target", Vec3::Y), !dead, tf.translation));
        let (Some(skill), true) = (data.db.get(&b.skill), data.db.get(&b.beam).is_some()) else {
            commands.entity(e).try_despawn();
            continue;
        };
        if b.start.is_none() {
            // State 0: the beam from the caster's hand to a target that is still a character.
            let (Some((end, ..)), Ok((ctf, cch, _))) = (target, points.get(b.caster)) else {
                commands.entity(e).try_despawn();
                continue;
            };
            b.start = Some(point_of(ctf, cch, "R Hand", Vec3::new(0.0, 1.2, 0.0)));
            b.end = end;
        }
        let (arrived, done) = b.step(
            dt,
            skill.f32("headVelocity").unwrap_or(0.0),
            skill.f32("tailVelocity").unwrap_or(0.0),
            target.map(|(p, alive, _)| (p, alive)),
        );
        if arrived && let Some((point, _, at)) = target {
            hits.write(SkillHit {
                attacker: b.caster,
                target: b.target,
                skill: b.skill.clone(),
                level_index: b.level_index,
                parent: b.parent.clone(),
                scale: 1.0,
                projectile: false,
            });
            // `Skill_AttackSpellChaosSpawnPet::TargetResult` (Reap Spirit, `pets::placement`): its
            // `petBurstSpawn` summons around the foe hit.
            if skill.class == "Skill_AttackSpellChaosSpawnPet" {
                pets.write(SpawnPet {
                    owner: b.caster,
                    skill: b.skill.clone(),
                    level_index: b.level_index,
                    point: at,
                    target: Some(b.target),
                });
            }
            activations.write_batch(crate::secondary::Activate::on_hits(
                b.caster,
                &b.skill,
                b.level_index,
                &b.parent,
                vec![b.target],
                point,
            ));
        }
        if done {
            commands.entity(e).try_despawn();
        }
    }
}
