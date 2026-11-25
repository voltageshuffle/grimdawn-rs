//! The rays of a channelled beam skill (`Skill_AttackSpellBeam`: Aether Ray): the skill's `BeamInfo` nodes on the
//! caster ([`SpellBeams`]), their life, fan, emit point, directions and lengths, every frame ([`spell_beams`]).
//!
//! Game.dll `Skill_AttackSpellBeam::ActivateNow` (a node per cast; `beamDelay` for the main player only),
//! `UpdateBeams`: the emit point is the caster's `emitPointName` (default `L Hand`) attach point, re-read every
//! frame; the heading is the caster's, its slope toward the aim point and ray k turned by the fan's angle k; the
//! held cast's rays grow to `GetMaxBeamLength`. Stopping (`StopEmitting`) the node keeps its last directions while
//! it retracts.
//!
//! Damage (`Update`, `GetTargets`): every `timeBetweenAttacks` / cast speed ms while the ribbons are in the world,
//! each ray's drawn ribbon (from its retract point, its grown length + 1.5 m) hits the foes within `beamThickness`
//! of it, each foe once (`GetTargetsAroundRay`; `passThrough` 0: the nearest foe only). `ActivateNow` pays the
//! energy once, then each strike of the held cast pays it and the channel ends when what is left is under the cost.
//! PORT: the bind-pose emit point; the fan turns about the world up (the engine turns the emit frame about its
//! attach Y axis).
use super::*;
use gd_sim::beam::{SpellBeam, State, fan_angles};

/// `GetMaxBeamLength` / `Skill_AttackSpellCone::GetMaxConeLength`: the ray from 1 m behind `emit` along `dir`,
/// `range` + 1 long, cut at its first contact with the world or a solid visible entity, minus 1 and at least 0; and
/// whether it was cut.
pub fn max_beam_length(
    coll: &crate::collision::WorldCollision,
    emit: Vec3,
    dir: Vec3,
    range: f32,
) -> (f32, bool) {
    let dir = dir.normalize_or_zero();
    let a = emit - dir;
    match coll.segment_hit(a, a + dir * (range + 1.0)) {
        Some((p, _)) => ((p.distance(a) - 1.0).max(0.0), true),
        None => (range, false),
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn spell_beams(
    time: Res<Time>,
    data: Res<GameData>,
    coll: Res<crate::collision::WorldCollision>,
    mut commands: Commands,
    mut casters: Query<(
        Entity,
        &Transform,
        &Character,
        Option<&Channel>,
        Option<&mut SpellBeams>,
        Has<Player>,
        Option<&gd_game_core::weapon_pool::SkillLinks>,
        Option<&mut Caster>,
        Option<&gd_game_core::player::Loadout>,
    )>,
    // `FilterInvalidTargets`: the caster's foes.
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
    mut ids: Local<u64>,
) {
    let dt = time.delta_secs();
    for (e, tf, character, ch, beams, player, links, mut caster, loadout) in &mut casters {
        let channel = ch.filter(|ch| {
            data.db
                .get(&ch.skill)
                .is_some_and(|r| r.class == "Skill_AttackSpellBeam")
        });
        let Some(mut beams) = beams else {
            if channel.is_some() {
                commands.entity(e).try_insert(SpellBeams::default());
            }
            continue;
        };
        // `StopEmitting`: the channel ended (or moved to another skill).
        for n in beams.0.iter_mut().filter(|n| !n.beam.stopped()) {
            if channel.is_none_or(|ch| ch.skill != n.skill) {
                n.beam.stop();
            }
        }
        // `ActivateNow`: a new channel's node.
        if let Some(ch) = channel
            && !beams.0.iter().any(|n| !n.beam.stopped())
            && let Some(rec) = data.db.get(&ch.skill)
        {
            let mods = links.map_or(&[][..], |l| l.mods_of(&ch.skill));
            let (n, spread) = fan(&data, rec, ch.level_index, mods);
            let delay = if player {
                rec.f32("beamDelay").unwrap_or(0.0)
            } else {
                0.0
            };
            // `ActivateNow` pays the energy; short of it the channel ends.
            let cost = level_f(rec, "skillManaCost", ch.level_index);
            let paid = match caster.as_deref_mut() {
                Some(c) if c.energy < cost => false,
                Some(c) => {
                    c.energy -= cost;
                    true
                }
                None => true,
            };
            if paid {
                *ids += 1;
                beams.0.push(BeamNode {
                    id: *ids,
                    skill: ch.skill.clone(),
                    level_index: ch.level_index,
                    beam: SpellBeam::new(delay, n, strike_period(rec, Some(character))),
                    angles: fan_angles(n, spread),
                    heading: gd_game_core::character::facing_flat(tf),
                    emit: tf.translation,
                    dirs: vec![gd_game_core::character::facing_flat(tf); n],
                    surface: vec![false; n],
                    effects: gd_game_core::skill_fx::beam_effects(
                        rec,
                        &gd_game_core::skill_fx::override_mods(
                            &data.db,
                            links,
                            loadout.map(|l| &l.skills),
                            &ch.skill,
                        ),
                    ),
                });
            } else {
                commands.entity(e).try_remove::<Channel>();
            }
        }
        for node in beams.0.iter_mut() {
            let Some(rec) = data.db.get(&node.skill) else {
                node.beam.state = State::Done;
                continue;
            };
            let range = {
                let v = level_f(rec, "maxRange", node.level_index);
                if v > 0.0 { v } else { 16.0 }
            };
            let local = attach_point_bind(
                &character.body,
                rec.str("emitPointName").unwrap_or("L Hand"),
            )
            .unwrap_or(Vec3::new(0.0, 1.2, 0.0));
            node.emit = tf.transform_point(local);
            // The held cast's heading, the slope toward the aim point; a retracting node keeps its last.
            if !node.beam.stopped()
                && let Some(ch) = channel
            {
                let flat = gd_game_core::character::facing_flat(tf);
                let aim = ch.aim.unwrap_or(node.emit + flat * range);
                let to = aim - node.emit;
                let slope = (to.y / to.with_y(0.0).length().max(0.5)).clamp(-1.0, 1.0);
                node.heading = (flat + Vec3::Y * slope).normalize_or(flat);
            }
            let mut max_lens = Vec::with_capacity(node.angles.len());
            for (k, a) in node.angles.iter().enumerate() {
                let d = Quat::from_rotation_y(*a) * node.heading;
                let (len, surface) = max_beam_length(&coll, node.emit, d, range);
                node.dirs[k] = d;
                node.surface[k] = surface;
                max_lens.push(len);
            }
            let speed = rec.f32("travelSpeed").unwrap_or(200.0);
            node.beam.update(dt, speed, &max_lens);
            if !node
                .beam
                .attack_due(dt, strike_period(rec, Some(character)))
            {
                continue;
            }
            let thick = rec.f32("beamThickness").unwrap_or(0.0);
            let first_only = rec.get("passThrough").is_some() && !rec.bool("passThrough");
            let side = crate::combat::Side::lookup(&sides, e, !player);
            let candidates: Vec<(Entity, &GlobalTransform, &Combatant)> = foes
                .iter()
                .filter(|(t, _, _, v)| *t != e && side.hits(v))
                .map(|(t, gt, cb, _)| (t, gt, cb))
                .collect();
            let mut struck: Vec<Entity> = Vec::new();
            for k in 0..node.dirs.len() {
                let (a, _) = node.segment(k);
                let len = (node.beam.lens[k] - node.beam.retract).max(0.0);
                for t in ray_targets(&candidates, a, node.dirs[k], len + 1.5, thick, first_only) {
                    if !struck.contains(&t) {
                        struck.push(t);
                    }
                }
            }
            for &t in &struck {
                hits.write(SkillHit {
                    attacker: e,
                    target: t,
                    skill: node.skill.clone(),
                    level_index: node.level_index,
                    parent: None,
                    scale: 1.0,
                    projectile: false,
                });
            }
            gd_game_core::visuals::notify(&mut commands, &node.skill);
            // The held cast pays each strike and ends when the energy left is under the cost.
            if !node.beam.stopped()
                && let Some(c) = caster.as_deref_mut()
            {
                let cost = level_f(rec, "skillManaCost", node.level_index);
                c.energy = (c.energy - cost).max(0.0);
                if c.energy < cost {
                    commands.entity(e).try_remove::<Channel>();
                }
            }
        }
        beams.0.retain(|n| n.beam.state != State::Done);
    }
}

/// The strike period (s) of a beam or cone: `timeBetweenAttacks` (default 100 ms) / the spell-cast speed
/// (`character`: the caster; none: 1).
pub(super) fn strike_period(rec: &Record, character: Option<&Character>) -> f32 {
    rec.f32("timeBetweenAttacks").unwrap_or(100.0)
        / 1000.0
        / character.map_or(1.0, |c| c.spell_speed.max(0.2))
}

/// `GetTargetsAroundRay`: the `candidates` whose world box grown by `thick` the ray from `a` along `d` enters within
/// `reach` metres; with `first_only` the nearest along it.
fn ray_targets(
    candidates: &[(Entity, &GlobalTransform, &Combatant)],
    a: Vec3,
    d: Vec3,
    reach: f32,
    thick: f32,
    first_only: bool,
) -> Vec<Entity> {
    let Some(dir) = d.try_normalize() else {
        return Vec::new();
    };
    let mut on: Vec<(Entity, f32)> = candidates
        .iter()
        .filter_map(|&(m, gt, mcb)| {
            let mut b = mcb.body_box(&gt.affine());
            b.half = b.half.map(|h| h + thick);
            let t = b.ray_entry(a.to_array(), dir.to_array());
            (t < reach).then_some((m, t))
        })
        .collect();
    if first_only {
        on.sort_by(|x, y| x.1.total_cmp(&y.1));
        on.truncate(1);
    }
    on.into_iter().map(|x| x.0).collect()
}
