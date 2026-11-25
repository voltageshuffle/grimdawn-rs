//! `Skill_AttackLineFan` (Beronath's frontal cones, the Questing Beast's ice cracks, the Beronath claw item
//! skills): a fan of lines that crack out along the ground from the caster, hit what each line's growing reach
//! covers, and burst along their whole length when their life ends.
//!
//! - `Load` (class-level): `useTargetDir`, `useHandLaunch`, `followTerrain`, `launchAttachPointName`,
//!   `attackWidth`, `lineLength`, `segmentLengthMin` / `…Max`, `segmentRotationMin` / `…Max` (degrees),
//!   `travelTime` (× 1000 → int ms), `fxMeshDistance`, `explosionDistance`, `launchOffset`,
//!   `alternateLaunchRotation`, `explosionDamageScale` (× 100: a damage percentage), `textureWidthStart` /
//!   `…Middle` / `…End`, `trailEffect`, `endEffect`, `fxMesh`, `explosionEffect`, `startSound`, `endSound`,
//!   `loopSound`; `lifeTime` is per level (`lifeTime[min(level, n) − 1]` × 1000 ms). The speed is `lineLength` ×
//!   1000 / travel ms (0 without a travel time). The number of lines and the fan angle are `skillTargetNumber`
//!   (0 → 1) and `skillTargetAngle` with the learned modifiers'.
//! - `ActivateNow` (Forgotten Gods only): the fan's coordinates are the caster's (with `useHandLaunch` the hand's
//!   attach point, else the `launchAttachPointName` point when named); `useTargetDir` with a target turns them to
//!   it (when the horizontal direction is not zero); with neither launch point, or with `followTerrain`, they go
//!   on the floor and 0.1 m up; then `launchOffset` along the heading. Its `TargetResult` pays the mana, starts the
//!   cooldown and creates the fan: when another fan of this skill is still alive the whole fan is turned
//!   `alternateLaunchRotation`° about +Y; line i of n heads at `−angle / 2 + i × step` (`step` = angle / (n − 1),
//!   angle / n for a full circle; one line: straight ahead). Each line gets its trail and `endEffect` at its
//!   start; then `startSound` plays at the fan's coordinates and `loopSound` starts there.
//! - `Update`, per line each frame (rotations about +Y, degrees; a positive angle turns +Z toward +X):
//!   - while its travel time lasts: the tip moves speed × dt along its own heading (`followTerrain`: on the
//!     floor, 0.1 m up), the end effect with it. When the tip is a segment length (first 0.125–0.25 m, then
//!     random in `segmentLength*`) from the last segment point, or the travel ends, a segment is laid: the tip's
//!     heading becomes the line's start heading turned by a random `segmentRotation*`, and the trail's anchors
//!     are set across the tip `GetTextureWidth(distance)` wide (`textureWidthStart` → `…Middle` over the first
//!     half of `lineLength`, `…Middle` → `…End` over the rest). Every `fxMeshDistance` travelled an `fxMesh`
//!     drops at the tip; every `explosionDistance` the tip's coordinates are kept as an explosion point. Then
//!     `GetTargetsAroundRay` from the start along the start heading, as long as the tip's distance from the
//!     start, `attackWidth` wide: targets the fan has not hit yet (one list for all its lines) are added and
//!     attacked at the tip's coordinates.
//!   - its life counts down from the start; when it runs out: `GetTargetsAroundRay` over the full `lineLength`,
//!     every target (the fan's list is not checked) attacked at the start's coordinates as an explosion. Then
//!     `explosionEffect` plays at each explosion point, the end effect stops, the trail fades; the fan's first
//!     line to end fades the loop sound (250 ms) and plays `endSound` at its tip. The fan goes once all lines
//!     are done. `StopSkill` ends every line at once, without the burst.
//!   - No wall, door or line of sight stops a line.
//! - `TargetResult` with targets: per target the explosion's damage percentage (`explosionDamageScale` × 100, 0
//!   = 100 % for travel hits and for records without one), the rag doll, `CreateTargetFx` and `PlayHitSound`, the
//!   attack; then the secondaries at the attack's coordinates.
//! - `World::GetEntitiesAroundRay`: an entity whose bounds, grown by the width on every side, the ray meets
//!   within its length.
//!
//! PORT: the hand / named launch points are their bind-pose positions, the hand that of RightHandHit; the
//! segment rotations and lengths draw from the gameplay stream; `fxMesh` drops as an
//! [`FxMeshRequest`](gd_game_core::visuals::FxMeshRequest); the travel's end puts
//! [`StartPulse`](gd_game_core::visuals::StartPulse) on the trail. The renderer draws each line's trail from its
//! [`TrailAnchor`] entity and fades it on `StopEmitting`.
use crate::nav::Nav;
use bevy::prelude::*;
use gd_game_core::casting::SkillHit;
use gd_game_core::combat::{Combatant, Dead};
use gd_game_core::data::GameData;

pub struct LineFanPlugin;

impl Plugin for LineFanPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Launch>().add_systems(
            Update,
            update
                .in_set(crate::casting::CastingSet)
                .after(gd_game_core::player::PlayerSet)
                .before(gd_game_core::combat::CombatSet),
        );
    }
}

/// A line-fan cast fired: its `TargetResult` with no targets.
#[derive(Message, Clone, Debug)]
pub struct Launch {
    pub caster: Entity,
    pub skill: String,
    pub level_index: usize,
    pub parent: Option<(String, usize)>,
    pub hostile: bool,
    /// `skillTargetAngle` (degrees) and `skillTargetNumber`, with the learned modifiers'.
    pub angle: f32,
    pub number: u32,
    /// `lifeTime` at the cast's level (s).
    pub life: f32,
    /// `useTargetDir`.
    pub target: Option<Vec3>,
}

#[derive(Debug, Clone)]
pub struct Line {
    /// The start and its unit heading (the targeting ray).
    pub start: Vec3,
    pub dir: Vec3,
    /// The tip and its heading: the start heading turned by the last segment's rotation.
    pub tip: Vec3,
    pub heading: Vec3,
    pub dist: f32,
    /// Travel time and life left (s).
    pub travel: f32,
    pub life: f32,
    /// (position, heading) of the tip.
    pub explosions: Vec<(Vec3, Vec3)>,
    pub done: bool,
    last_segment: Vec3,
    next_segment: f32,
    last_explosion: f32,
    last_mesh: f32,
    /// The trail's anchor frame and the trail entity; the end effects.
    anchor: Option<Entity>,
    trail: Option<Entity>,
    tip_fx: Vec<Entity>,
}

/// A fan of travelling lines (the renderer reads its lines' [`TrailAnchor`] and effect entities).
#[derive(Component, Debug, Clone)]
pub struct LineFan {
    pub caster: Entity,
    pub skill: String,
    pub level_index: usize,
    pub parent: Option<(String, usize)>,
    pub hostile: bool,
    /// `attackWidth`, `lineLength`, speed (m/s).
    pub width: f32,
    pub length: f32,
    pub speed: f32,
    /// `explosionDamageScale`, 0 → 1.
    pub explosion_scale: f32,
    pub lines: Vec<Line>,
    /// Foes hit by the travelling lines, each once per fan.
    pub hit: Vec<Entity>,
    follow_terrain: bool,
    segment_length: (f32, f32),
    segment_rotation: (f32, f32),
    texture_width: [f32; 3],
    explosion_spacing: f32,
    fx_mesh: Option<(String, f32)>,
    trail: bool,
    explosion_fx: Option<String>,
    end_sound: Option<String>,
    loop_sound: Option<Entity>,
    ended: bool,
}

/// A line's trail anchor frame: at the last segment point, turned to the tip's heading, scaled across (x) to the
/// trail's width there; the trail's edges are its local ±0.5 x.
#[derive(Component, Debug, Clone, Copy)]
pub struct TrailAnchor;

/// Degrees from straight ahead (`TargetResult`).
pub fn fan_angles(angle: f32, number: u32) -> Vec<f32> {
    let n = number.max(1);
    let (step, first) = if n > 1 {
        (
            angle
                / if angle < 360.0 {
                    (n - 1) as f32
                } else {
                    n as f32
                },
            -angle * 0.5,
        )
    } else {
        (0.0, 0.0)
    };
    (0..n).map(|i| first + step * i as f32).collect()
}

/// `GetTextureWidth`: the trail's width `d` metres out.
pub fn texture_width(w: [f32; 3], length: f32, d: f32) -> f32 {
    let (a, b) = if d > length * 0.5 {
        (w[1], w[2])
    } else {
        (w[0], w[1])
    };
    d / length * (b - a) + a
}

/// +Z toward +X for a positive angle.
fn turn(v: Vec3, deg: f32) -> Vec3 {
    Quat::from_rotation_y(deg.to_radians()) * v
}

/// The nav mesh, else the terrain, else `p`'s.
fn floor(nav: &Nav, coll: &crate::collision::WorldCollision, p: Vec3) -> f32 {
    nav.0
        .ground([p.x, p.y + 0.5, p.z])
        .or_else(|| coll.terrain_ground(p))
        .unwrap_or(p.y)
}

fn frame(at: Vec3, heading: Vec3, width: f32) -> Transform {
    Transform::from_translation(at)
        .with_rotation(Quat::from_rotation_y(heading.x.atan2(heading.z)))
        .with_scale(Vec3::new(width.max(0.0), 1.0, 1.0))
}

fn create(
    commands: &mut Commands,
    data: &GameData,
    nav: &Nav,
    coll: &crate::collision::WorldCollision,
    l: &Launch,
    (caster_tf, body): (&Transform, Option<&gd_formats::msh::Mesh>),
    alternate: bool,
) -> Option<LineFan> {
    let rec = data.db.get(&l.skill)?;
    let f = |k: &str| rec.f32(k).unwrap_or(0.0);
    let facing = gd_game_core::character::facing_flat(caster_tf);
    // `useHandLaunch` the hand of the cast's callback, else the named point, else the caster; its position
    // only. PORT: the point's bind pose, and the callback is taken as RightHandHit.
    let named = rec.str("launchAttachPointName").filter(|n| !n.is_empty());
    let hand = rec.bool("useHandLaunch");
    let mut origin = match (hand, named) {
        (true, _) => crate::waves::launch_point(caster_tf, body, "R Hand"),
        (false, Some(n)) => crate::waves::launch_point(caster_tf, body, n),
        _ => caster_tf.translation,
    };
    let mut dir = facing;
    if rec.bool("useTargetDir")
        && let Some(t) = l.target
    {
        let d = (t - origin).with_y(0.0);
        if d.length_squared() > 1e-4 {
            dir = d.normalize();
        }
    }
    if (!hand && named.is_none()) || rec.bool("followTerrain") {
        origin.y = floor(nav, coll, origin) + 0.1;
    }
    origin += dir * f("launchOffset");
    if alternate && f("alternateLaunchRotation") != 0.0 {
        dir = turn(dir, f("alternateLaunchRotation"));
    }
    let length = f("lineLength");
    let travel = (f("travelTime") * 1000.0) as i32;
    let speed = if travel > 0 {
        length * 1000.0 / travel as f32
    } else {
        0.0
    };
    let tw = [
        f("textureWidthStart"),
        f("textureWidthMiddle"),
        f("textureWidthEnd"),
    ];
    let exists = |k: &str| {
        rec.str(k)
            .filter(|s| data.db.get(s).is_some())
            .map(str::to_owned)
    };
    let (trail, end_fx) = (exists("trailEffect"), exists("endEffect"));
    let lines = fan_angles(l.angle, l.number)
        .into_iter()
        .map(|a| {
            let d = turn(dir, a);
            let anchor = trail.as_ref().map(|_| {
                commands
                    .spawn((
                        TrailAnchor,
                        frame(origin, d, texture_width(tw, length, 0.0)),
                    ))
                    .id()
            });
            let trail = trail.as_ref().zip(anchor).map(|(t, owner)| {
                commands
                    .spawn(gd_game_core::visuals::TrailRequest {
                        owner,
                        record: t.clone(),
                        width: 0.0,
                        anchors: Some((Vec3::new(-0.5, 0.0, 0.0), Vec3::new(0.5, 0.0, 0.0))),
                    })
                    .id()
            });
            let tip_fx = end_fx
                .as_ref()
                .map(|fx| effect_entities(commands, data, fx, frame(origin, d, 1.0)))
                .unwrap_or_default();
            Line {
                start: origin,
                dir: d,
                tip: origin,
                heading: d,
                dist: 0.0,
                travel: travel.max(0) as f32 / 1000.0,
                life: l.life,
                explosions: Vec::new(),
                done: false,
                last_segment: origin,
                next_segment: 0.125 + gd_game_core::rng::f32() * 0.125,
                last_explosion: 0.0,
                last_mesh: 0.0,
                anchor,
                trail,
                tip_fx,
            }
        })
        .collect();
    if let Some(pak) = rec.str("startSound") {
        commands.write_message(gd_game_core::visuals::PlaySound {
            pak: pak.to_owned(),
            at: Some(origin),
        });
    }
    let loop_sound = rec.str("loopSound").filter(|s| !s.is_empty()).map(|pak| {
        commands
            .spawn(gd_game_core::visuals::SoundLoop {
                pak: pak.to_owned(),
                on: None,
                at: origin,
                fade: crate::combat_sounds::SHORT_LOOP_FADE,
            })
            .id()
    });
    let scale = f("explosionDamageScale");
    Some(LineFan {
        caster: l.caster,
        skill: l.skill.clone(),
        level_index: l.level_index,
        parent: l.parent.clone(),
        hostile: l.hostile,
        width: f("attackWidth"),
        length,
        speed,
        explosion_scale: if scale == 0.0 { 1.0 } else { scale },
        lines,
        hit: Vec::new(),
        follow_terrain: rec.bool("followTerrain"),
        segment_length: (f("segmentLengthMin"), f("segmentLengthMax")),
        segment_rotation: (f("segmentRotationMin"), f("segmentRotationMax")),
        texture_width: tw,
        explosion_spacing: f("explosionDistance"),
        fx_mesh: exists("fxMesh").map(|m| (m, f("fxMeshDistance"))),
        trail: trail.is_some(),
        explosion_fx: exists("explosionEffect"),
        end_sound: rec.str("endSound").map(str::to_owned),
        loop_sound,
        ended: false,
    })
}

fn effect_entities(
    commands: &mut Commands,
    data: &GameData,
    fx: &str,
    tf: Transform,
) -> Vec<Entity> {
    let Some(r) = data.db.get(fx) else {
        return Vec::new();
    };
    let names: Vec<String> = if r.class == "FxPak" || r.get("particleEffectNames").is_some() {
        r.strs_iter("particleEffectNames")
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![fx.to_owned()]
    };
    names
        .into_iter()
        .map(|n| {
            let mut spec = gd_game_core::visuals::EffectSpec::record(n);
            spec.despawn_when_complete = true;
            commands.spawn((spec, tf)).id()
        })
        .collect()
}

fn range((a, b): (f32, f32)) -> f32 {
    a + gd_game_core::rng::f32() * (b - a)
}

/// Creates the cast fans, moves the lines, hits what they reach and bursts them at the end of their life.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    nav: Res<Nav>,
    coll: Res<crate::collision::WorldCollision>,
    mut launches: MessageReader<Launch>,
    mut fans: Query<(Entity, &mut LineFan)>,
    mut frames: Query<&mut Transform, (Without<Combatant>, Without<LineFan>)>,
    casters: Query<(&Transform, Option<&gd_game_core::character::Character>), With<Combatant>>,
    foes: Query<(Entity, &Transform, &Combatant, crate::combat::VictimSide), Without<Dead>>,
    sides: Query<crate::combat::SideData>,
    mut hits: MessageWriter<SkillHit>,
    mut activations: MessageWriter<crate::secondary::Activate>,
) {
    let mut alive: Vec<(Entity, &str)> = Vec::new();
    if !launches.is_empty() {
        alive.extend(fans.iter().map(|(_, f)| (f.caster, f.skill.as_str())));
    }
    for l in launches.read() {
        let Ok((tf, ch)) = casters.get(l.caster) else {
            continue;
        };
        let body = ch.map(|c| &*c.body);
        let alternate = alive
            .iter()
            .any(|(c, s)| *c == l.caster && s.eq_ignore_ascii_case(&l.skill));
        if let Some(fan) = create(&mut commands, &data, &nav, &coll, l, (tf, body), alternate) {
            debug!(
                "line fan {}: {} lines, {:.0}°",
                l.skill.rsplit('/').next().unwrap_or(""),
                fan.lines.len(),
                l.angle
            );
            alive.push((l.caster, l.skill.as_str()));
            commands.spawn(fan);
        }
    }
    let dt = time.delta_secs();
    for (e, mut fan) in &mut fans {
        if casters.get(fan.caster).is_err() {
            // `StopSkill`: every line ends, no burst.
            for line in &fan.lines {
                stop_line(&mut commands, line);
            }
            if let Some(l) = fan.loop_sound {
                commands.entity(l).try_despawn();
            }
            commands.entity(e).try_despawn();
            continue;
        }
        let fan = &mut *fan;
        let side = crate::combat::Side::lookup(&sides, fan.caster, fan.hostile);
        let candidates: Vec<(Entity, gd_sim::bounds::Aabb)> = foes
            .iter()
            .filter(|(t, _, _, v)| *t != fan.caster && side.hits(v))
            .map(|(t, tf, cb, ..)| (t, cb.body_box(&tf.compute_affine())))
            .collect();
        let ray = |start: Vec3, dir: Vec3, len: f32, width: f32| {
            crate::secondary::beam_targets(
                start,
                start + dir * len,
                Entity::PLACEHOLDER,
                width,
                true,
                candidates.iter().copied(),
            )
        };
        for i in 0..fan.lines.len() {
            if fan.lines[i].done {
                continue;
            }
            if fan.lines[i].travel > 0.0 {
                let (speed, follow, width, length, tw) = (
                    fan.speed,
                    fan.follow_terrain,
                    fan.width,
                    fan.length,
                    fan.texture_width,
                );
                let (seg_len, seg_rot, trail, spacing) = (
                    fan.segment_length,
                    fan.segment_rotation,
                    fan.trail,
                    fan.explosion_spacing,
                );
                let line = &mut fan.lines[i];
                line.travel -= dt;
                if line.travel <= 0.0
                    && let Some(t) = line.trail
                {
                    commands
                        .entity(t)
                        .try_insert(gd_game_core::visuals::StartPulse);
                }
                let step = dt * speed;
                line.dist += step;
                line.tip += line.heading * step;
                if follow {
                    line.tip.y = floor(&nav, &coll, line.tip) + 0.1;
                }
                for fx in &line.tip_fx {
                    if let Ok(mut t) = frames.get_mut(*fx) {
                        *t = frame(line.tip, line.heading, 1.0);
                    }
                }
                if trail
                    && (line.tip.distance(line.last_segment) >= line.next_segment
                        || line.travel <= 0.0)
                {
                    line.heading = turn(line.dir, range(seg_rot));
                    if let Some(a) = line.anchor
                        && let Ok(mut t) = frames.get_mut(a)
                    {
                        *t = frame(line.tip, line.heading, texture_width(tw, length, line.dist));
                    }
                    line.last_segment = line.tip;
                    line.next_segment = range(seg_len);
                }
                if let Some((m, d)) = &fan.fx_mesh
                    && *d > 0.0
                    && line.dist - line.last_mesh > *d
                {
                    line.last_mesh = line.dist;
                    commands.spawn((
                        gd_game_core::visuals::FxMeshRequest(m.clone()),
                        frame(line.tip, line.heading, 1.0),
                    ));
                }
                if line.dist - line.last_explosion > spacing {
                    line.explosions.push((line.tip, line.heading));
                    line.last_explosion = line.dist;
                }
                let (start, dir, tip) = (line.start, line.dir, line.tip);
                let new: Vec<Entity> = ray(start, dir, tip.distance(start), width)
                    .into_iter()
                    .filter(|t| !fan.hit.contains(t))
                    .collect();
                if !new.is_empty() {
                    debug!(
                        "line fan {} line {i} hits {} at {:.1} m",
                        fan.skill.rsplit('/').next().unwrap_or(""),
                        new.len(),
                        fan.lines[i].dist
                    );
                    fan.hit.extend(new.iter().copied());
                    for &t in &new {
                        hits.write(SkillHit {
                            attacker: fan.caster,
                            target: t,
                            skill: fan.skill.clone(),
                            level_index: fan.level_index,
                            parent: fan.parent.clone(),
                            scale: 1.0,
                            projectile: false,
                        });
                    }
                    activations.write_batch(crate::secondary::Activate::on_hits(
                        fan.caster,
                        &fan.skill,
                        fan.level_index,
                        &fan.parent,
                        new,
                        tip,
                    ));
                }
            }
            fan.lines[i].life -= dt;
            if fan.lines[i].life <= 0.0 {
                let line = fan.lines[i].clone();
                let burst = ray(line.start, line.dir, fan.length, fan.width);
                if !burst.is_empty() {
                    debug!(
                        "line fan {} line {i} bursts on {}",
                        fan.skill.rsplit('/').next().unwrap_or(""),
                        burst.len()
                    );
                    for &t in &burst {
                        hits.write(SkillHit {
                            attacker: fan.caster,
                            target: t,
                            skill: fan.skill.clone(),
                            level_index: fan.level_index,
                            parent: fan.parent.clone(),
                            scale: fan.explosion_scale,
                            projectile: false,
                        });
                    }
                    activations.write_batch(crate::secondary::Activate::on_hits(
                        fan.caster,
                        &fan.skill,
                        fan.level_index,
                        &fan.parent,
                        burst,
                        line.start,
                    ));
                }
                if let Some(fx) = &fan.explosion_fx {
                    for (p, h) in &line.explosions {
                        gd_game_core::visuals::spawn_fx(
                            &mut commands,
                            &data,
                            fx,
                            frame(*p, *h, 1.0),
                        );
                    }
                }
                stop_line(&mut commands, &line);
                if !fan.ended {
                    if let Some(l) = fan.loop_sound {
                        commands.entity(l).try_despawn();
                    }
                    if let Some(pak) = &fan.end_sound {
                        commands.write_message(gd_game_core::visuals::PlaySound {
                            pak: pak.clone(),
                            at: Some(line.tip),
                        });
                    }
                    fan.ended = true;
                }
                fan.lines[i].done = true;
            }
        }
        if fan.lines.iter().all(|l| l.done) {
            commands.entity(e).try_despawn();
        }
    }
}

/// A line's end: the end effects stop, the trail fades, its anchor goes.
fn stop_line(commands: &mut Commands, line: &Line) {
    for fx in &line.tip_fx {
        commands
            .entity(*fx)
            .try_insert(gd_game_core::visuals::StopEmitting);
    }
    if let Some(t) = line.trail {
        commands
            .entity(t)
            .try_insert(gd_game_core::visuals::StopEmitting);
    }
    if let Some(a) = line.anchor {
        commands.entity(a).try_despawn();
    }
}
