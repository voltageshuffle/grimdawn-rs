//! `ChaosBeam` tendrils (Engine.dll `GAME::ChaosBeam`: the synergy beams `synergyChaosBeam`, drain, Telekinesis, ...):
//! wobbling ribbons between two points.
//!
//! `ChaosBeam::UpdateSelf` @0x1800468d0: `N = floor(length *
//! TotalSegments)` points (at most 500) at `a + i * length / N`, i in 0..N (the last one a step short of `b`); a common
//! sway `w[i] = ramp(i) * 2 * TotalAmplitude * (sin(0.04347826 i - T/3) + cos(0.032258064 i + 2T)) * perp1`
//! (`ramp = min(0.05 i, 0.05 (N - i), 1)`, `T` seconds), and each tendril adds `WaveAmplitude * scale * (perp1 cos(0.1 i /
//! period + t'/3) sin(2t' + 0.25 i) + perp2 sin(0.5 i period + t'/2) cos(2t' + 0.1 i))` with `t' = T * WaveSpeed` and its
//! own period (0.5 to 1.5 s). `scale` ([`scale_at`], d = the point's distance from `a`): without `DynamicScale` 1, but
//! `d * StartScale` within 1 m of `a` and `(1 - e) * EndScale + 1` within `e` < 1 m of `b` (the tendrils fan out at the
//! far end); with it `d / p1 * StartScale` up to `p1 = ScaleLengthPercent1 %` of the length, then a linear blend to
//! `EndScale` at `p2 = ScaleLengthPercent2 %`, then down to 0 at `b` (each scale capped at 2 when its part is under
//! 3 m). Record defaults (`ChaosBeam::ChaosBeam` @0x1800460d0 / `Load` @0x180046530): `WaveSpeed` 1, `TotalAmplitude` 1,
//! `WaveAmplitude` 0.3, `StartScale` 1, `EndScale` 1.5, `TotalSegments` 5, `EndAlpha` 0, colour `Red255` .. `Alpha255`.
//! `LineEffect::RenderPass` @0x180139fb0 (a ChaosBeam's points drawn between the `SetTopBottomPercent`
//! fractions): one quad strip per tendril, `Width` wide, its side the same for every point: perpendicular to the whole
//! beam's on-screen direction (`atan2` of the start - end vector in the view's right / up axes), so the strip does
//! not turn with the wiggles; texture u 0 / 1 across, v = the path length up to the next point / `TextureStretch`; the
//! vertex colour is the record's × a ramp over the first and last 5 points (fade count 5, set by the ChaosBeam
//! constructor; `(i + 1) / 5` and `(n - i) / 5`) × `EndAlpha` at the first and last drawn point; `Shader` additive or
//! alpha ([`crate::beams::ribbon_material`]).
//! Synergy (`ControllerMonsterSynergy::Update` @0x18014ec00): one beam per member to the leader's `Target` point,
//! `synergyNumBeams` tendrils, `WaveSpeed = 40 .. 89.5` (random per beam), `TotalAmplitude = 0.02`, drawn to the leader's
//! charge (`SetTopBottomPercent`: the first part of the way from the member). Drain beams keep the record's values.
//! Drain beams: [`draw_drain`]; the chaos skills' beams: [`draw_chaos`]. The end points are the posed bodies' attach
//! points (`Actor::GetAttachPointCoords`).
//! PORT: `T` is the app's clock (the engine's is the beam's own age from its creation); every beam has its own period
//! list from the cosmetic generator.
//! Each beam is a root entity (its state component, its tendrils as children); the synergy and chaos-skill roots are
//! [`crate::beams::VisualOf`] their owner and go when it does. The record's shape is parsed once per beam, and every
//! tendril mesh is rewritten in place.
use crate::beams::{VisualOf, Visuals};
use bevy::prelude::*;
use gd_game::synergy::{Mode, Synergy};

pub struct ChaosBeamPlugin;

impl Plugin for ChaosBeamPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (draw_synergy, draw_drain, draw_chaos).after(TransformSystems::Propagate),
        );
    }
}

/// One tendril: its entity, persistent mesh (grow-only capacity in points) and wave period.
struct Tendril {
    entity: Entity,
    mesh: Option<Handle<Mesh>>,
    cap: usize,
    period: f32,
}

/// One drawn ChaosBeam: the record's shape (with the synergy controller's overrides) and its tendrils.
struct Beam {
    shape: Shape,
    tendrils: Vec<Tendril>,
}

impl Beam {
    /// The beam's `n` tendril entities under `root`, all with the record's material.
    fn spawn(
        commands: &mut Commands,
        assets: &mut crate::character::CharAssets,
        root: Entity,
        rec: &gd_formats::arz::Record,
        n: usize,
        name: &'static str,
    ) -> Self {
        let mat = crate::beams::ribbon_material(rec, assets);
        Beam {
            shape: Shape::of(rec),
            tendrils: (0..n)
                .map(|_| Tendril {
                    entity: commands
                        .spawn((
                            MeshMaterial3d(mat.clone()),
                            Transform::IDENTITY,
                            Visibility::default(),
                            bevy::light::NotShadowCaster,
                            bevy::camera::visibility::NoFrustumCulling,
                            Name::new(name),
                            ChildOf(root),
                        ))
                        .id(),
                    mesh: None,
                    cap: 0,
                    period: 0.5 + gd_game::rng::f32(),
                })
                .collect(),
        }
    }
}

/// A visual root entity: identity frame, so its tendril children draw in world space.
fn spawn_root(commands: &mut Commands, name: &'static str) -> Entity {
    commands
        .spawn((Transform::IDENTITY, Visibility::default(), Name::new(name)))
        .id()
}

/// Reused per-frame buffers.
#[derive(Default)]
struct Scratch {
    pts: Vec<Vec3>,
    pos: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

/// A synergy member's beam to its leader.
#[derive(Component)]
struct SynergyBeam(Beam);

#[allow(clippy::too_many_arguments)]
fn draw_synergy(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<crate::data::GameData>,
    cams: Query<&GlobalTransform, With<crate::camera::GameCamera>>,
    members: Query<(
        Entity,
        &Synergy,
        &gd_game::monsters::Monster,
        &GlobalTransform,
        Option<&gd_game::character::Character>,
        Has<gd_game::combat::Dead>,
    )>,
    mut drawn: Query<(Entity, &mut SynergyBeam, Option<&VisualOf>)>,
    mut assets: crate::character::CharAssets,
    mut wanted: Local<Vec<(Entity, Entity, f32)>>,
    mut have: Local<Vec<Entity>>,
    mut scratch: Local<Scratch>,
) {
    let Some(cam) = cams.iter().next() else {
        return;
    };
    let fwd = cam.forward().as_vec3();
    let t = time.elapsed_secs();
    // Wanted: every comrade of a living leader.
    wanted.clear();
    for (e, s, _, _, _, dead) in &members {
        if matches!(s.mode, Mode::Leader(_)) && !dead {
            for c in &s.comrades {
                if *c != e {
                    wanted.push((*c, e, s.charge()));
                }
            }
        }
    }
    wanted.sort_by_key(|w| w.0);
    wanted.dedup_by_key(|w| w.0);
    let point = |e: Entity| -> Option<Vec3> {
        let (_, _, _, gt, ch, _) = members.get(e).ok()?;
        let local = ch.and_then(|c| c.posed_point("Target")).unwrap_or(Vec3::Y);
        Some(gt.transform_point(local))
    };
    have.clear();
    for (root, mut beam, owner) in &mut drawn {
        let w = owner.and_then(|o| {
            let i = wanted.binary_search_by_key(&o.0, |w| w.0).ok()?;
            Some(wanted[i])
        });
        let Some((member, leader, charge)) = w else {
            commands.entity(root).despawn();
            continue;
        };
        have.push(member);
        if let (Some(a), Some(b)) = (point(member), point(leader)) {
            update_beam(
                &mut commands,
                &mut assets,
                &mut beam.0,
                &mut scratch,
                a,
                b,
                charge,
                0.0,
                t,
                fwd,
            );
        }
    }
    have.sort_unstable();
    for &(member, leader, charge) in wanted.iter() {
        if have.binary_search(&member).is_ok() {
            continue;
        }
        let (Some(a), Some(b)) = (point(member), point(leader)) else {
            continue;
        };
        let Ok((_, _, mon, ..)) = members.get(leader) else {
            continue;
        };
        let Some(controller) = data
            .db
            .get(&mon.record)
            .and_then(|r| r.str("controller").and_then(|c| data.db.get(c)))
        else {
            continue;
        };
        let Some(rec) = controller
            .str("synergyChaosBeam")
            .and_then(|b| data.db.get(b))
        else {
            continue;
        };
        let n = controller.i32("synergyNumBeams").unwrap_or(1).max(1) as usize;
        gd_game::visuals::notify(&mut commands, &rec.name);
        let root = spawn_root(&mut commands, "synergy beam");
        let mut beam = Beam::spawn(&mut commands, &mut assets, root, rec, n, "synergy beam");
        beam.shape.wave_speed = 40.0 + (gd_game::rng::f32() * 100.0).floor() * 0.5;
        beam.shape.total_amp = 0.02;
        update_beam(
            &mut commands,
            &mut assets,
            &mut beam,
            &mut scratch,
            a,
            b,
            charge,
            0.0,
            t,
            fwd,
        );
        commands
            .entity(root)
            .insert((SynergyBeam(beam), VisualOf(member)));
    }
}

/// The record's tendril shape (the fields `UpdateSelf` and `RenderPass` read).
struct Shape {
    width: f32,
    stretch: f32,
    segs: f32,
    wave_speed: f32,
    total_amp: f32,
    wave_amp: f32,
    start_scale: f32,
    end_scale: f32,
    end_alpha: f32,
    /// `DynamicScale`: `ScaleLengthPercent1` / `2` as fractions.
    dynamic: Option<(f32, f32)>,
    color: [f32; 4],
}

impl Shape {
    fn of(rec: &gd_formats::arz::Record) -> Self {
        let f = |k: &str, def: f32| rec.f32(k).unwrap_or(def);
        let c = |k: &str| f(k, 255.0) / 255.0;
        Shape {
            width: f("Width", 0.5),
            stretch: f("TextureStretch", 5.0).max(0.01),
            segs: f("TotalSegments", 5.0),
            wave_speed: f("WaveSpeed", 1.0),
            total_amp: f("TotalAmplitude", 1.0),
            wave_amp: f("WaveAmplitude", 0.3),
            start_scale: f("StartScale", 1.0),
            end_scale: f("EndScale", 1.5),
            end_alpha: f("EndAlpha", 0.0),
            dynamic: (rec.i32("DynamicScale").unwrap_or(0) != 0).then(|| {
                (
                    rec.i32("ScaleLengthPercent1").unwrap_or(0) as f32 * 0.01,
                    rec.i32("ScaleLengthPercent2").unwrap_or(0) as f32 * 0.01,
                )
            }),
            color: [c("Red255"), c("Green255"), c("Blue255"), c("Alpha255")],
        }
    }
}

/// The wave scale at `d` metres from the start of a `len` long beam (module doc).
fn scale_at(s: &Shape, d: f32, len: f32) -> f32 {
    match s.dynamic {
        None => {
            let mut k = 1.0;
            if d < 1.0 {
                k = d * s.start_scale;
            }
            let e = len - d;
            if e < 1.0 {
                k = (1.0 - e) * s.end_scale + 1.0;
            }
            k
        }
        Some((p1, p2)) => {
            let (l1, l2) = (p1 * len, p2 * len);
            let start = if l1 < 3.0 {
                s.start_scale.min(2.0)
            } else {
                s.start_scale
            };
            let end = if len - l2 < 3.0 {
                s.end_scale.min(2.0)
            } else {
                s.end_scale
            };
            let k = if d <= l1 {
                d / l1 * start
            } else if d <= l2 {
                (d - l1) / (l2 - l1) * (end - start) + start
            } else {
                (1.0 - (d - l2) / (len - l2)) * end
            };
            k.max(0.0)
        }
    }
}

/// The colour ramp of point `i` of the `n` drawn ones (`RenderPass`'s fade over 5 points and `EndAlpha` at both ends).
fn point_alpha(i: usize, n: usize, end_alpha: f32) -> f32 {
    const FADE: usize = 5;
    let (i, n) = (i as i32, n as i32);
    let k = (FADE as i32).min(n);
    let mut lo = 1.0;
    if i < k {
        lo = (k - i - 1) as f32 / k as f32;
        if k > 1 {
            lo = 1.0 - lo;
        }
    }
    let from = if n - k > 0 { n - k - 1 } else { n - 2 };
    let mut hi = 1.0;
    if i >= from {
        hi = (i + k - n) as f32 / k as f32;
        if k > 1 {
            hi = 1.0 - hi;
        }
    }
    if lo > hi && hi > 0.0 {
        lo = hi;
    }
    let ends = if i == 0 || i == n - 1 { end_alpha } else { 1.0 };
    lo * ends
}

/// Moves one beam's tendrils between `a` and `b` (the formulas of the module doc), drawn from `bottom` to `top`
/// (0..1) of the way (`SetTopBottomPercent`).
#[allow(clippy::too_many_arguments)]
fn update_beam(
    commands: &mut Commands,
    assets: &mut crate::character::CharAssets,
    beam: &mut Beam,
    scratch: &mut Scratch,
    a: Vec3,
    b: Vec3,
    top: f32,
    bottom: f32,
    t: f32,
    fwd: Vec3,
) {
    let d = b - a;
    let len = d.length();
    if len < 0.1 {
        return;
    }
    let shape = &beam.shape;
    let dir = d / len;
    let perp1 = dir.cross(Vec3::Y).normalize_or(Vec3::X);
    let perp2 = dir.cross(perp1).normalize_or(Vec3::Y);
    let n = ((len * shape.segs) as usize).clamp(1, 500);
    let step = len / n as f32;
    // `RenderPass`: the points (int)(n * bottom) .. (int)(n * top), at least 2.
    let hi = ((n as f32 * top.clamp(0.0, 1.0)) as usize).min(n);
    let lo = ((n as f32 * bottom.clamp(0.0, 1.0)) as usize).min(hi);
    // The strip's side: across the whole beam's on-screen direction.
    let side = (-d).cross(fwd).normalize_or(Vec3::X) * shape.width * 0.5;
    let tp = t * shape.wave_speed;
    for tendril in &mut beam.tendrils {
        if hi - lo < 2 {
            // Hidden: every index degenerate, the mesh kept for when it shows again.
            if let Some(h) = &tendril.mesh
                && let Some(mut m) = assets.meshes.get_mut(h)
                && let Some(bevy::mesh::Indices::U32(idx)) = m.indices_mut()
            {
                idx.fill(0);
            }
            continue;
        }
        let period = tendril.period;
        scratch.pts.clear();
        scratch.pts.extend((0..hi).map(|i| {
            let fi = i as f32;
            let along = fi * step;
            let ramp = (fi * 0.05).min((n as f32 - fi) * 0.05).min(1.0);
            let common = ramp
                * 2.0
                * shape.total_amp
                * ((0.043_478_26 * fi - t / 3.0).sin() + (0.032_258_064 * fi + 2.0 * t).cos());
            let wave = shape.wave_amp
                * scale_at(shape, along, len)
                * (perp1 * ((0.1 / period) * fi + tp / 3.0).cos() * (2.0 * tp + 0.25 * fi).sin()
                    + perp2 * (fi * period * 0.5 + tp / 2.0).sin() * (2.0 * tp + 0.1 * fi).cos());
            a + dir * along + perp1 * common + wave
        }));
        strip(scratch, lo, hi, side, shape);
        write_strip(commands, assets, tendril, scratch, hi - lo);
    }
}

/// `LineEffect::RenderPass`'s strip over the points `lo..hi` of `scratch.pts` into the scratch vertex buffers: `side`
/// the half-width vector (the same at every point), v = the path length from point 0 up to the next point /
/// `TextureStretch`, the colour ramp of [`point_alpha`] over the drawn points.
fn strip(scratch: &mut Scratch, lo: usize, hi: usize, side: Vec3, s: &Shape) {
    let Scratch {
        pts,
        pos,
        uv,
        col,
        idx,
    } = scratch;
    pos.clear();
    uv.clear();
    col.clear();
    idx.clear();
    let mut acc = 0.0;
    for i in 0..hi {
        let p = pts[i];
        if let Some(next) = pts.get(i + 1) {
            acc += p.distance(*next);
        }
        if i < lo {
            continue;
        }
        let v = acc / s.stretch;
        let a = s.color[3] * point_alpha(i - lo, hi - lo, s.end_alpha);
        pos.push((p - side).to_array());
        pos.push((p + side).to_array());
        uv.push([0.0, v]);
        uv.push([1.0, v]);
        col.push([s.color[0], s.color[1], s.color[2], a]);
        col.push([s.color[0], s.color[1], s.color[2], a]);
    }
    for i in 0..((hi - lo) as u32 - 1) {
        let k = i * 2;
        idx.extend_from_slice(&[k, k + 1, k + 2, k + 1, k + 3, k + 2]);
    }
}

/// Writes the scratch strip of `m` points into the tendril's mesh in place, growing it (power-of-two points) only when
/// the strip outgrows it; the unused tail is degenerate and sits on the last point.
fn write_strip(
    commands: &mut Commands,
    assets: &mut crate::character::CharAssets,
    tendril: &mut Tendril,
    scratch: &mut Scratch,
    m: usize,
) {
    if tendril.mesh.is_none() || tendril.cap < m {
        let cap = m.next_power_of_two().max(8);
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; cap * 2]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; cap * 2]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0; 2]; cap * 2]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0; 4]; cap * 2]);
        mesh.insert_indices(bevy::mesh::Indices::U32(vec![0; cap * 6]));
        let h = assets.meshes.add(mesh);
        commands
            .entity(tendril.entity)
            .try_insert(Mesh3d(h.clone()));
        tendril.mesh = Some(h);
        tendril.cap = cap;
    }
    let cap = tendril.cap;
    let last = scratch.pos[m * 2 - 1];
    scratch.pos.resize(cap * 2, last);
    scratch.uv.resize(cap * 2, [0.0; 2]);
    scratch.col.resize(cap * 2, [0.0; 4]);
    scratch.idx.resize(cap * 6, 0);
    let Some(h) = &tendril.mesh else {
        return;
    };
    if let Some(mut mesh) = assets.meshes.get_mut(h) {
        use bevy::mesh::VertexAttributeValues;
        if let Some(VertexAttributeValues::Float32x3(a)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        {
            a.copy_from_slice(&scratch.pos);
        }
        if let Some(VertexAttributeValues::Float32x2(a)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
        {
            a.copy_from_slice(&scratch.uv);
        }
        if let Some(VertexAttributeValues::Float32x4(a)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
        {
            a.copy_from_slice(&scratch.col);
        }
        if let Some(bevy::mesh::Indices::U32(a)) = mesh.indices_mut() {
            a.copy_from_slice(&scratch.idx);
        }
    }
}

/// `Skill_AttackSpellDrain` beams (G `Update` @0x1804d04c0, `TargetResult` @0x1804d1820, `Load` @0x1804cf590):
/// each drain keeps a list of ChaosBeams, each from its target's `Target` point to the caster's
/// `emitPointName` (default `L Hand`), both re-read every frame while the drain runs. A new target gets a beam at
/// once; a beam's head travels from the target at `travelSpeed` (default 3) of the way per second, its tail follows at
/// `trailTravelSpeed` (default 2; the stock records' `tailTravelSpeed` is not read) once the head is at 70 %, and it
/// goes when the tail arrives; when the head arrives the next beam to the same (living) target starts after
/// `newBeamDelay` (default 50) + `rand() % 500` ms. When the drain stops no beam spawns another and the rest finish
/// their way where they were. The beam record is `chaosBeamName` for the primary target, `secondaryBeamName` for the
/// extra ones (`FindNewTargets`' `beamNumber`, 1 in every stock drain and modifier, so the port's one target is the
/// primary); tendrils: the beam record's `BeamCount`. `TargetResult` / `Update`: an `fxChanges` list
/// (`gd_game::skill_fx::fx_changes`) of four or more replaces them by entries 0 / 1 (and the caster / target
/// effects by 2 / 3, which the port does not draw for any drain), e.g. Drain Essence's elemental tendrils.
#[allow(clippy::too_many_arguments)]
fn draw_drain(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<crate::data::GameData>,
    cams: Query<&GlobalTransform, With<crate::camera::GameCamera>>,
    casters: Query<(
        Entity,
        &gd_game::casting::Channel,
        &GlobalTransform,
        &gd_game::character::Character,
        Option<&gd_game::weapon_pool::SkillLinks>,
        Option<&gd_game::player::Loadout>,
    )>,
    points: Query<(
        &GlobalTransform,
        Option<&gd_game::character::Character>,
        Has<gd_game::combat::Dead>,
    )>,
    mut groups: Query<(Entity, &mut DrainGroup)>,
    mut assets: crate::character::CharAssets,
    mut fresh: Local<Vec<(Entity, DrainGroup)>>,
    mut scratch: Local<Scratch>,
) {
    let Some(cam) = cams.iter().next() else {
        return;
    };
    let fwd = cam.forward().as_vec3();
    let t = time.elapsed_secs();
    let dt = time.delta_secs() * 1000.0;
    let alive = |e: Entity| points.get(e).is_ok_and(|(.., dead)| !dead);
    // `StopEmitting`: a group whose drain ended runs out (no beam spawns another).
    for (_, mut g) in &mut groups {
        if !g.running {
            continue;
        }
        let still = casters
            .get(g.caster)
            .is_ok_and(|(_, ch, ..)| ch.target.is_some() && ch.skill == g.skill);
        if !still {
            g.running = false;
            for b in &mut g.beams {
                b.spawn_next = false;
            }
        }
    }
    fresh.clear();
    for (e, ch, gt, character, links, loadout) in &casters {
        let Some(target) = ch.target else {
            continue;
        };
        let Some(rec) = data
            .db
            .get(&ch.skill)
            .filter(|r| r.class == "Skill_AttackSpellDrain")
        else {
            continue;
        };
        let mods = gd_game::skill_fx::override_mods(
            &data.db,
            links,
            loadout.map(|l| &l.skills),
            &ch.skill,
        );
        let beam = gd_game::skill_fx::fx_changes(&mods)
            .filter(|v| v.len() >= 4)
            .map(|mut v| v.swap_remove(0))
            .or_else(|| rec.str("chaosBeamName").map(str::to_owned));
        let mut found = groups.iter_mut().find(|(_, g)| g.running && g.caster == e);
        let g = match &mut found {
            Some((_, g)) => &mut **g,
            None => {
                let root = spawn_root(&mut commands, "drain beams");
                fresh.push((
                    root,
                    DrainGroup {
                        caster: e,
                        skill: ch.skill.clone(),
                        beam: None,
                        travel: rec.f32("travelSpeed").unwrap_or(3.0),
                        trail: rec.f32("trailTravelSpeed").unwrap_or(2.0),
                        delay: rec.i32("newBeamDelay").unwrap_or(50) as f32,
                        running: true,
                        beams: Vec::new(),
                    },
                ));
                &mut fresh.last_mut().unwrap().1
            }
        };
        g.beam = beam;
        // `TargetResult`: a target without a beam gets one now.
        if !g.beams.iter().any(|b| b.target == target) {
            g.beams.push(DrainBeam::new(target, 0.0));
        }
        // The end points of the running group, every frame.
        let emit = character
            .posed_point(rec.str("emitPointName").unwrap_or("L Hand"))
            .unwrap_or(Vec3::new(0.0, 1.2, 0.0));
        let caster_at = gt.transform_point(emit);
        for b in &mut g.beams {
            b.caster_at = caster_at;
            if let Ok((tgt, tch, _)) = points.get(b.target) {
                b.target_at = tgt
                    .transform_point(tch.and_then(|c| c.posed_point("Target")).unwrap_or(Vec3::Y));
            }
        }
    }
    let mut run = |root: Entity, g: &mut DrainGroup, commands: &mut Commands| -> bool {
        let mut born = Vec::new();
        for b in &mut g.beams {
            if b.step(dt, g.travel, g.trail, alive(b.target)) {
                born.push(DrainBeam::new(
                    b.target,
                    g.delay + gd_game::rng::u32(0..500) as f32,
                ));
            }
            if b.state == DrainState::Dead {
                b.free(commands);
                continue;
            }
            if b.state != DrainState::Shown {
                continue;
            }
            if b.beam.is_none() {
                let Some(rec) = g.beam.as_deref().and_then(|r| data.db.get(r)) else {
                    continue;
                };
                let n = rec.i32("BeamCount").unwrap_or(1).max(1) as usize;
                b.beam = Some(Beam::spawn(
                    commands,
                    &mut assets,
                    root,
                    rec,
                    n,
                    "drain beam",
                ));
            }
            if let Some(beam) = &mut b.beam {
                update_beam(
                    commands,
                    &mut assets,
                    beam,
                    &mut scratch,
                    b.target_at,
                    b.caster_at,
                    b.top,
                    b.bottom,
                    t,
                    fwd,
                );
            }
        }
        g.beams.retain(|b| b.state != DrainState::Dead);
        for mut n in born {
            if let Some(p) = g.beams.iter().find(|b| b.target == n.target) {
                (n.target_at, n.caster_at) = (p.target_at, p.caster_at);
            }
            g.beams.push(n);
        }
        g.running || !g.beams.is_empty()
    };
    for (root, mut g) in &mut groups {
        if !run(root, &mut g, &mut commands) {
            commands.entity(root).despawn();
        }
    }
    for (root, mut g) in fresh.drain(..) {
        if run(root, &mut g, &mut commands) {
            commands.entity(root).insert(g);
        } else {
            commands.entity(root).despawn();
        }
    }
}

/// One drain's beams (`Skill_AttackSpellDrain`'s group list) on its own root entity, so a group whose drain stopped
/// runs out after its caster is gone: the running one, or one running out.
#[derive(Component)]
struct DrainGroup {
    caster: Entity,
    skill: String,
    /// The primary beam record (`chaosBeamName`, or the modifiers' `fxChanges[0]`), as of the drain's last frame.
    beam: Option<String>,
    /// The skill's `travelSpeed`, `trailTravelSpeed` and `newBeamDelay` (ms).
    travel: f32,
    trail: f32,
    delay: f32,
    running: bool,
    beams: Vec<DrainBeam>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DrainState {
    /// Waiting for its delay (state 0).
    Pending,
    /// In the world, travelling (states 2 / 4).
    Shown,
    /// Done (state 5).
    Dead,
}

/// One drain beam (the group's 80-byte entry): head / tail fractions of the way from the target, its delay (ms).
struct DrainBeam {
    target: Entity,
    top: f32,
    bottom: f32,
    spawn_next: bool,
    delay: f32,
    state: DrainState,
    target_at: Vec3,
    caster_at: Vec3,
    beam: Option<Beam>,
}

impl DrainBeam {
    fn new(target: Entity, delay: f32) -> Self {
        DrainBeam {
            target,
            top: 0.0,
            bottom: 0.0,
            spawn_next: true,
            delay,
            state: DrainState::Pending,
            target_at: Vec3::ZERO,
            caster_at: Vec3::ZERO,
            beam: None,
        }
    }

    /// One `Update` step of `dt` ms ([`draw_drain`]); true when the head just arrived and the next beam to the
    /// target is due.
    fn step(&mut self, dt: f32, travel: f32, trail: f32, target_alive: bool) -> bool {
        self.delay -= dt;
        if self.delay > 0.0 {
            return false;
        }
        if self.state == DrainState::Pending {
            self.state = if target_alive {
                DrainState::Shown
            } else {
                DrainState::Dead
            };
        }
        if self.state != DrainState::Shown {
            return false;
        }
        let mut next = false;
        if self.top < 1.0 {
            self.top = (self.top + dt * 0.001 * travel).clamp(0.0, 1.0);
            next = self.spawn_next && self.top >= 1.0 && target_alive;
        }
        if self.top >= 0.7 {
            if self.bottom < 1.0 {
                self.bottom = (self.bottom + dt * 0.001 * trail).clamp(0.0, 1.0);
            }
            if self.bottom >= 1.0 {
                self.bottom = self.top;
                self.state = DrainState::Dead;
            }
        }
        next
    }

    /// `ChaosBeam::Liberate`: its tendrils go.
    fn free(&mut self, commands: &mut Commands) {
        for t in self.beam.take().iter().flat_map(|b| b.tendrils.iter()) {
            commands.entity(t.entity).try_despawn();
        }
    }
}

/// `Skill_AttackSpellChaos` beams (Reap Spirit; `Update` @0x1804dc590): each
/// [`gd_game::casting::ChaosBeam`] drawn from its caster's `R Hand` to its far end between its `bottom` and `top`
/// fractions (`SetTopBottomPercent`), all of which gd-game moves (`casting::chaos`, which also lands the hit when the
/// head arrives); its tendrils (the beam record's `BeamCount`) go with it (`ChaosBeam::Liberate`).
/// A [`gd_game::casting::ChaosBeam`]'s drawing.
#[derive(Component)]
struct ChaosDrawn(Beam);

#[allow(clippy::too_many_arguments)]
fn draw_chaos(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<crate::data::GameData>,
    cams: Query<&GlobalTransform, With<crate::camera::GameCamera>>,
    beams: Query<(Entity, &gd_game::casting::ChaosBeam, Has<Visuals>)>,
    mut drawn: Query<(Entity, &mut ChaosDrawn, Option<&VisualOf>)>,
    mut assets: crate::character::CharAssets,
    mut scratch: Local<Scratch>,
) {
    let cam = cams.iter().next().map(|c| c.forward().as_vec3());
    let t = time.elapsed_secs();
    for (root, mut beam, owner) in &mut drawn {
        let Some((_, c, _)) = owner.and_then(|o| beams.get(o.0).ok()) else {
            commands.entity(root).despawn();
            continue;
        };
        if let (Some(fwd), Some(start)) = (cam, c.start) {
            update_beam(
                &mut commands,
                &mut assets,
                &mut beam.0,
                &mut scratch,
                start,
                c.end,
                c.top,
                c.bottom,
                t,
                fwd,
            );
        }
    }
    let Some(fwd) = cam else {
        return;
    };
    for (e, c, has) in &beams {
        if has {
            continue;
        }
        let (Some(start), Some(rec)) = (c.start, data.db.get(&c.beam)) else {
            continue;
        };
        let root = spawn_root(&mut commands, "chaos beam");
        let n = rec.i32("BeamCount").unwrap_or(1).max(1) as usize;
        let mut beam = Beam::spawn(&mut commands, &mut assets, root, rec, n, "chaos beam");
        update_beam(
            &mut commands,
            &mut assets,
            &mut beam,
            &mut scratch,
            start,
            c.end,
            c.top,
            c.bottom,
            t,
            fwd,
        );
        commands
            .entity(root)
            .insert((ChaosDrawn(beam), VisualOf(e)));
    }
}
