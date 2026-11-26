//! The ribbons of a channelled beam skill (`Skill_AttackSpellBeam`: Aether Ray), and of reflect links and tethers: a
//! `LineEffect2` record's entity ([`draw_links`]).
//!
//! Engine.dll `LineEffect2` (`Load`, `Generate`, `UpdateSelf`, `RenderPass`) and Game.dll
//! `Skill_AttackSpellBeam::UpdateBeams`:
//! a camera-facing ribbon `lineWidth` wide along a 4-point polyline from the start to the end (`P1` and `P2` a tenth
//! of the length, at most `fadeDistance`, in from the ends); texture v = the path length / `textureLength` + a
//! scroll that moves `-textureSpeed` repeats per second; vertex alpha `endAlpha` at both ends and 1 inside; the blend
//! is the record's shader (`trailcombine` alpha blend, else additive). The rays themselves (the node's life, the
//! `beamNumber` / `beamAngle` fan, the emit point, each ray's direction and length) are
//! `gd_game_core::casting::SpellBeams`'; each ray is its own `LineEffect2` with its own end (`endEffect`, while the ray is
//! at its end) and ground (`groundEffect`, while it meets a surface) effects at its far end, the start effect
//! (`startEffect`, one per node) at `emit + heading * retract` until the stop; each is removed when stopped
//! (`ActivateNow`, `Update`, `PlaceEffects`, `StartEndEffects`, `DestroyBeams`). The beam, start and end names are the
//! node's (`BeamNode::effects`: a modifier's `fxChanges` replaces them). Each node's drawing and each link ribbon is an
//! entity [`VisualOf`] its caster.
use bevy::prelude::*;
use gd_game_core::casting::{BeamNode, SpellBeams};
use gd_game_core::visuals::EffectSpec;

pub struct BeamsPlugin;

impl Plugin for BeamsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (draw_beams, draw_links)
                .after(TransformSystems::Propagate)
                .before(crate::fx::FxSystems),
        );
    }
}

/// A beam visual's gameplay owner. The relationship is removed when the owner despawns, so a visual without it is
/// an orphan and its system drops it (stopping its effects first, which `linked_spawn` would not).
#[derive(Component)]
#[relationship(relationship_target = Visuals)]
pub(crate) struct VisualOf(pub(crate) Entity);

/// An owner's beam visuals ([`VisualOf`]).
#[derive(Component)]
#[relationship_target(relationship = VisualOf)]
pub(crate) struct Visuals(Vec<Entity>);

/// One drawn ray: the ribbon entity, its end and ground effect entities.
struct Ray {
    entity: Entity,
    mesh: Option<Handle<Mesh>>,
    end_fx: Option<Entity>,
    ground_fx: Option<Entity>,
}

/// The drawing of one beam node ([`gd_game_core::casting::BeamNode`]) on its own root entity: its rays (children) and
/// start effect.
#[derive(Component)]
struct Drawn {
    id: u64,
    rays: Vec<Ray>,
    start_fx: Option<Entity>,
    /// `StopEmitting` was seen (the start effect stopped).
    stopped: bool,
    scroll: f32,
    ribbon: Ribbon,
    /// The skill's `groundEffect`.
    ground: Option<String>,
}

/// The `LineEffect2` record's fields the ribbon reads.
struct Ribbon {
    width: f32,
    tex_len: f32,
    end_alpha: f32,
    fade: f32,
    color: [f32; 3],
    speed: f32,
}

impl Ribbon {
    fn of(beam: &gd_formats::arz::Record) -> Self {
        Ribbon {
            width: beam.f32("lineWidth").unwrap_or(1.0),
            tex_len: beam.f32("textureLength").unwrap_or(5.0).max(0.01),
            end_alpha: beam.f32("endAlpha").unwrap_or(0.0),
            fade: beam.f32("fadeDistance").unwrap_or(0.0),
            color: [
                beam.f32("color.r").unwrap_or(1.0),
                beam.f32("color.g").unwrap_or(1.0),
                beam.f32("color.b").unwrap_or(1.0),
            ],
            speed: beam.f32("textureSpeed").unwrap_or(1.0),
        }
    }

    /// `LineEffect2::UpdateSelf`: the scroll slides the texture from the start to the end.
    fn scroll(&self, scroll: &mut f32, dt: f32) {
        *scroll -= dt * self.speed;
        if *scroll <= -1.0 {
            *scroll += 1.0;
        }
    }
}

/// An emitting `EffectEntity` record at `at`, removed once stopped and finished.
fn start_effect(commands: &mut Commands, record: Option<&str>, at: Vec3) -> Option<Entity> {
    let r = record.filter(|r| !r.is_empty())?;
    gd_game_core::visuals::notify(commands, r);
    let mut spec = EffectSpec::record(r);
    spec.despawn_when_complete = true;
    Some(
        commands
            .spawn((
                spec,
                Transform::from_translation(at),
                Name::new("beam effect"),
            ))
            .id(),
    )
}

/// `EffectEntity::StopEmitting` + `SetSelfRemove(true)`.
fn stop_effect(commands: &mut Commands, e: &mut Option<Entity>) {
    if let Some(e) = e.take() {
        commands
            .entity(e)
            .try_insert(gd_game_core::visuals::StopEmitting);
    }
}

/// Moves an effect entity (its coords; `World::SetCoords`, position only).
fn place(
    fx: &mut Query<(&mut Transform, &mut GlobalTransform), FxOnly>,
    e: Option<Entity>,
    at: Vec3,
) {
    if let Some((mut tf, mut gt)) = e.and_then(|e| fx.get_mut(e).ok()) {
        tf.translation = at;
        // After Propagate: write the global frame too so the effect follows this frame's pose.
        *gt = GlobalTransform::from_translation(at);
    }
}

type FxOnly = (
    Without<gd_game_core::character::Character>,
    Without<crate::camera::GameCamera>,
);

/// The ribbon's 18 indices (4 points, 3 quads), fixed for every beam.
const RIBBON_IDX: [u32; 18] = [0, 1, 2, 1, 3, 2, 2, 3, 4, 3, 5, 4, 4, 5, 6, 5, 7, 6];

fn spawn_ribbon(
    commands: &mut Commands,
    mat: Handle<StandardMaterial>,
    name: &'static str,
) -> Entity {
    commands
        .spawn((
            MeshMaterial3d(mat),
            Transform::IDENTITY,
            Visibility::default(),
            bevy::light::NotShadowCaster,
            bevy::camera::visibility::NoFrustumCulling,
            Name::new(name),
        ))
        .id()
}

/// Sets a ribbon entity's mesh: a persistent 8-vertex mesh made on first use, then rewritten in place.
#[allow(clippy::too_many_arguments)]
fn set_ribbon(
    commands: &mut Commands,
    assets: &mut crate::character::CharAssets,
    entity: Entity,
    slot: &mut Option<Handle<Mesh>>,
    emit: Vec3,
    dir: Vec3,
    len: f32,
    scroll: f32,
    ribbon: &Ribbon,
    fwd: Vec3,
) {
    let (pos, uv, col) = ribbon_data(emit, dir, len, scroll, ribbon, fwd);
    match slot {
        Some(h) => {
            if let Some(mut mesh) = assets.meshes.get_mut(h) {
                use bevy::mesh::VertexAttributeValues;
                if let Some(VertexAttributeValues::Float32x3(a)) =
                    mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
                {
                    a.copy_from_slice(&pos);
                }
                if let Some(VertexAttributeValues::Float32x2(a)) =
                    mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
                {
                    a.copy_from_slice(&uv);
                }
                if let Some(VertexAttributeValues::Float32x4(a)) =
                    mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
                {
                    a.copy_from_slice(&col);
                }
            }
        }
        None => {
            let mut m = Mesh::new(
                bevy::mesh::PrimitiveTopology::TriangleList,
                bevy::asset::RenderAssetUsages::default(),
            );
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos.to_vec());
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 8]);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv.to_vec());
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col.to_vec());
            m.insert_indices(bevy::mesh::Indices::U32(RIBBON_IDX.to_vec()));
            let h = assets.meshes.add(m);
            commands.entity(entity).try_insert(Mesh3d(h.clone()));
            *slot = Some(h);
        }
    }
}

/// `ActivateNow` .. `DestroyBeams`: every node of every caster's [`SpellBeams`] drawn; a node gone (retracted, or
/// its caster gone) stops its effects and its drawing goes.
#[allow(clippy::too_many_arguments)]
fn draw_beams(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<gd_game_core::data::GameData>,
    cams: Query<&GlobalTransform, With<crate::camera::GameCamera>>,
    casters: Query<(Entity, &SpellBeams, Option<&Visuals>)>,
    mut fx: Query<(&mut Transform, &mut GlobalTransform), FxOnly>,
    mut drawn: Query<(Entity, &mut Drawn, Option<&VisualOf>)>,
    mut assets: crate::character::CharAssets,
) {
    let dt = time.delta_secs();
    let cam_fwd = cams.iter().next().map(|c| c.forward().as_vec3());
    for (root, mut d, owner) in &mut drawn {
        let node = owner
            .and_then(|o| casters.get(o.0).ok())
            .and_then(|(_, b, _)| b.0.iter().find(|n| n.id == d.id));
        match (node, cam_fwd) {
            (Some(node), Some(fwd)) => {
                update_node(&mut commands, &mut assets, &mut fx, &mut d, node, dt, fwd)
            }
            (Some(_), None) => {}
            (None, _) => {
                stop_effect(&mut commands, &mut d.start_fx);
                for r in &mut d.rays {
                    stop_effect(&mut commands, &mut r.end_fx);
                    stop_effect(&mut commands, &mut r.ground_fx);
                }
                commands.entity(root).despawn();
            }
        }
    }
    let Some(fwd) = cam_fwd else {
        return;
    };
    for (owner, beams, visuals) in &casters {
        for node in &beams.0 {
            let shown = visuals.is_some_and(|v| {
                v.0.iter()
                    .any(|e| drawn.get(*e).is_ok_and(|(_, d, _)| d.id == node.id))
            });
            if shown {
                continue;
            }
            let [beam_name, start_name, _] = &node.effects;
            let (Some(rec), Some(beam)) = (
                data.db.get(&node.skill),
                beam_name.as_deref().and_then(|b| data.db.get(b)),
            ) else {
                continue;
            };
            // `ActivateNow` / `PlaceEffects`: the node's ribbons and its start effect at the emit point.
            if !ribbon_ready(beam, &mut assets) {
                continue;
            }
            let mat = ribbon_material(beam, &mut assets);
            let root = commands
                .spawn((
                    Transform::IDENTITY,
                    Visibility::default(),
                    Name::new("beam node"),
                ))
                .id();
            let mut d = Drawn {
                id: node.id,
                rays: (0..node.dirs.len())
                    .map(|_| Ray {
                        entity: {
                            let e = spawn_ribbon(&mut commands, mat.clone(), "beam");
                            commands.entity(e).insert(ChildOf(root));
                            e
                        },
                        mesh: None,
                        end_fx: None,
                        ground_fx: None,
                    })
                    .collect(),
                start_fx: start_effect(&mut commands, start_name.as_deref(), node.emit),
                stopped: false,
                scroll: 0.0,
                ribbon: Ribbon::of(beam),
                ground: rec.str("groundEffect").map(str::to_owned),
            };
            update_node(&mut commands, &mut assets, &mut fx, &mut d, node, dt, fwd);
            commands.entity(root).insert((d, VisualOf(owner)));
        }
    }
}

/// One frame of a node's drawing: its start effect, each ray's end / ground effects and ribbon.
fn update_node(
    commands: &mut Commands,
    assets: &mut crate::character::CharAssets,
    fx: &mut Query<(&mut Transform, &mut GlobalTransform), FxOnly>,
    d: &mut Drawn,
    node: &BeamNode,
    dt: f32,
    fwd: Vec3,
) {
    let end_name = node.effects[2].as_deref();
    let start = node.emit + node.heading * node.beam.retract;
    if node.beam.stopped() && !d.stopped {
        d.stopped = true;
        stop_effect(commands, &mut d.start_fx);
    }
    place(fx, d.start_fx, start);
    let Drawn {
        rays,
        scroll,
        ribbon,
        ground,
        ..
    } = d;
    ribbon.scroll(scroll, dt);
    for (k, ray) in rays.iter_mut().enumerate() {
        let (a, b) = node.segment(k);
        if node.beam.shown() {
            // `StartEndEffects`: the end effect while the ray is at its end, the ground one while it meets a
            // surface.
            if node.beam.end_on[k] && ray.end_fx.is_none() {
                ray.end_fx = start_effect(commands, end_name, b);
            } else if !node.beam.end_on[k] {
                stop_effect(commands, &mut ray.end_fx);
            }
            if node.surface[k] && ray.ground_fx.is_none() {
                ray.ground_fx = start_effect(commands, ground.as_deref(), b);
            } else if !node.surface[k] {
                stop_effect(commands, &mut ray.ground_fx);
            }
            place(fx, ray.end_fx, b);
            place(fx, ray.ground_fx, b);
        }
        let len = node.beam.lens[k] - node.beam.retract;
        if !node.beam.shown() || len <= 0.1 {
            if ray.mesh.take().is_some() {
                commands.entity(ray.entity).try_remove::<Mesh3d>();
            }
            continue;
        }
        set_ribbon(
            commands,
            assets,
            ray.entity,
            &mut ray.mesh,
            a,
            node.dirs[k],
            len,
            *scroll,
            ribbon,
            fwd,
        );
    }
}

/// The `LineEffect2` record's ribbon texture.
fn ribbon_texture(beam: &gd_formats::arz::Record) -> &str {
    beam.str("texture")
        .or(beam.str("Texture"))
        .filter(|p| !p.is_empty())
        .unwrap_or("FX/Textures/beam.tex")
}

/// Whether the beam's ribbon texture is decoded; else its decode starts in the background.
fn ribbon_ready(beam: &gd_formats::arz::Record, assets: &mut crate::character::CharAssets) -> bool {
    let data = assets.data.clone();
    assets
        .textures
        .ready(&data, ribbon_texture(beam), false, &mut assets.images)
}

/// The `LineEffect2` record's material: its texture, alpha blended for `trailcombine`, else additive.
pub(crate) fn ribbon_material(
    beam: &gd_formats::arz::Record,
    assets: &mut crate::character::CharAssets,
) -> Handle<StandardMaterial> {
    let img = assets.textures.get(
        &assets.data,
        ribbon_texture(beam),
        false,
        &mut assets.images,
    );
    let combine = beam
        .str("shader")
        .or(beam.str("Shader"))
        .is_some_and(|s| s.to_ascii_lowercase().contains("trailcombine"));
    assets.materials.add(StandardMaterial {
        base_color_texture: img,
        unlit: true,
        alpha_mode: if combine {
            AlphaMode::Blend
        } else {
            AlphaMode::Add
        },
        cull_mode: None,
        ..default()
    })
}

/// `LineEffect2::Generate` + `RenderPass`: the ribbon's vertex data (8 vertices)
/// from `emit` along `dir` for `len` metres.
fn ribbon_data(
    emit: Vec3,
    dir: Vec3,
    len: f32,
    scroll: f32,
    r: &Ribbon,
    fwd: Vec3,
) -> ([[f32; 3]; 8], [[f32; 2]; 8], [[f32; 4]; 8]) {
    // `Generate`: P0 = start, P1 / P2 = `f` in from the ends, P3 = end.
    let f = (len * 0.1).min(r.fade);
    let pts = [0.0, f, len - f, len];
    let side = dir.cross(fwd).normalize_or(Vec3::X) * r.width * 0.5;
    let col = r.color;
    let mut pos = [[0.0; 3]; 8];
    let mut uv = [[0.0; 2]; 8];
    let mut colors = [[0.0; 4]; 8];
    for (i, d) in pts.iter().enumerate() {
        let p = emit + dir * *d;
        let a = if i == 0 || i == 3 { r.end_alpha } else { 1.0 };
        let v = *d / r.tex_len + scroll;
        pos[i * 2] = (p + side).to_array();
        pos[i * 2 + 1] = (p - side).to_array();
        uv[i * 2] = [1.0, v];
        uv[i * 2 + 1] = [0.0, v];
        colors[i * 2] = [col[0], col[1], col[2], a];
        colors[i * 2 + 1] = [col[0], col[1], col[2], a];
    }
    (pos, uv, colors)
}

/// A link or tether ribbon entity, [`VisualOf`] its caster: its target, record data and start / end effects.
#[derive(Component)]
struct LinkRibbon {
    target: Entity,
    tether: bool,
    mesh: Option<Handle<Mesh>>,
    scroll: f32,
    ribbon: Ribbon,
    /// The skill's `emitPointName` (default `FXCentered`) and `endEffect`.
    emit: String,
    end_effect: Option<String>,
    start_fx: Option<Entity>,
    end_fx: Option<Entity>,
}

/// Reflect-link and tether ribbons (`Skill_AttackSpellReflectLink::Update`, `SkillSecondary_Tether::Update`):
/// a `LineEffect2` (`linkName` / `tetherName`) from the caster's `emitPointName` (default `FXCentered`) to the same
/// point of the linked character, re-read every frame on the posed bodies. The start effect (`startEffect`) emits at
/// the caster's point from the link's start; the ribbon shows after the delay (`linkDelay` for the main player,
/// `tetherDelay`) with the end effect (`endEffect`) at the target's point; when the link ends both effects stop and
/// the ribbon goes.
#[allow(clippy::too_many_arguments)]
fn draw_links(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<gd_game_core::data::GameData>,
    cams: Query<&GlobalTransform, With<crate::camera::GameCamera>>,
    // Only entities that can carry a link: an unfiltered all-optional query walks every entity in the
    // world every frame (decoration, UI, FX included).
    casters: Query<
        (
            Entity,
            Option<&gd_game_core::reflect_link::ReflectLinks>,
            Option<&gd_game_core::secondary::Tethers>,
            Option<&Visuals>,
        ),
        Or<(
            With<gd_game_core::reflect_link::ReflectLinks>,
            With<gd_game_core::secondary::Tethers>,
            With<Visuals>,
        )>,
    >,
    mut ribbons: Query<(Entity, &mut LinkRibbon, Option<&VisualOf>)>,
    // The link's end points read `GlobalTransform`, the effects at them write it: one after the other.
    mut pf: ParamSet<(
        Query<(
            &GlobalTransform,
            Option<&gd_game_core::character::Character>,
        )>,
        Query<(&mut Transform, &mut GlobalTransform), FxOnly>,
    )>,
    mut assets: crate::character::CharAssets,
) {
    let fwd = cams.iter().next().map(|c| c.forward().as_vec3());
    let dt = time.delta_secs();
    // `Actor::GetAttachPointCoords`: the point on the posed body, every frame.
    fn ends(
        points: &Query<(
            &GlobalTransform,
            Option<&gd_game_core::character::Character>,
        )>,
        emit: &str,
        a: Entity,
        b: Entity,
    ) -> Option<(Vec3, Vec3)> {
        let point = |e: Entity| -> Option<Vec3> {
            let (gt, ch) = points.get(e).ok()?;
            let local = ch.and_then(|c| c.posed_point(emit)).unwrap_or(Vec3::Y);
            Some(gt.transform_point(local))
        };
        Some((point(a)?, point(b)?))
    }
    // The link's skill and whether its ribbon shows (`linkDelay` / `tetherDelay` passed).
    let link = |caster: Entity, target: Entity, tether: bool| -> Option<(&str, bool)> {
        let (_, links, tethers, _) = casters.get(caster).ok()?;
        if tether {
            let t = tethers?.0.iter().find(|t| t.target == target)?;
            Some((t.skill.as_str(), t.delay <= 0.0))
        } else {
            let l = links?.0.iter().find(|l| l.target == target)?;
            Some((l.skill.as_str(), l.shown()))
        }
    };
    for (e, mut r, owner) in &mut ribbons {
        let Some((caster, shown)) =
            owner.and_then(|o| link(o.0, r.target, r.tether).map(|(_, s)| (o.0, s)))
        else {
            stop_effect(&mut commands, &mut r.start_fx);
            stop_effect(&mut commands, &mut r.end_fx);
            commands.entity(e).despawn();
            continue;
        };
        let (Some(fwd), Some((a, b))) = (fwd, ends(&pf.p0(), &r.emit, caster, r.target)) else {
            continue;
        };
        update_link(
            &mut commands,
            &mut assets,
            &mut pf.p1(),
            e,
            &mut r,
            a,
            b,
            shown,
            dt,
            fwd,
        );
    }
    let Some(fwd) = fwd else {
        return;
    };
    for (caster, links, tethers, visuals) in &casters {
        let all = links
            .map(|l| l.0.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|l| (l.target, false))
            .chain(
                tethers
                    .map(|t| t.0.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .map(|t| (t.target, true)),
            );
        for (target, tether) in all {
            let drawn = visuals.is_some_and(|v| {
                v.0.iter().any(|e| {
                    ribbons
                        .get(*e)
                        .is_ok_and(|(_, r, _)| r.target == target && r.tether == tether)
                })
            });
            if drawn {
                continue;
            }
            let Some((skill, shown)) = link(caster, target, tether) else {
                continue;
            };
            let Some(rec) = data.db.get(skill) else {
                continue;
            };
            let field = if tether { "tetherName" } else { "linkName" };
            let Some(beam) = rec.str(field).and_then(|b| data.db.get(b)) else {
                continue;
            };
            let emit = rec.str("emitPointName").unwrap_or("FXCentered");
            let Some((a, b)) = ends(&pf.p0(), emit, caster, target) else {
                continue;
            };
            if !ribbon_ready(beam, &mut assets) {
                continue;
            }
            let mat = ribbon_material(beam, &mut assets);
            let e = spawn_ribbon(&mut commands, mat, "link beam");
            let mut r = LinkRibbon {
                target,
                tether,
                mesh: None,
                scroll: 0.0,
                ribbon: Ribbon::of(beam),
                emit: emit.to_owned(),
                end_effect: rec.str("endEffect").map(str::to_owned),
                start_fx: start_effect(&mut commands, rec.str("startEffect"), a),
                end_fx: None,
            };
            update_link(
                &mut commands,
                &mut assets,
                &mut pf.p1(),
                e,
                &mut r,
                a,
                b,
                shown,
                dt,
                fwd,
            );
            commands.entity(e).insert((r, VisualOf(caster)));
        }
    }
}

/// One frame of a link ribbon from `a` to `b`: its effects placed, the ribbon drawn once shown.
#[allow(clippy::too_many_arguments)]
fn update_link(
    commands: &mut Commands,
    assets: &mut crate::character::CharAssets,
    fx: &mut Query<(&mut Transform, &mut GlobalTransform), FxOnly>,
    e: Entity,
    r: &mut LinkRibbon,
    a: Vec3,
    b: Vec3,
    shown: bool,
    dt: f32,
    fwd: Vec3,
) {
    place(fx, r.start_fx, a);
    if !shown {
        return;
    }
    if r.end_fx.is_none() {
        r.end_fx = start_effect(commands, r.end_effect.as_deref(), b);
    }
    place(fx, r.end_fx, b);
    r.ribbon.scroll(&mut r.scroll, dt);
    let d = b - a;
    let len = d.length();
    // The ends together: no ribbon, rather than last frame's left where it was.
    if len <= 0.1 {
        if r.mesh.take().is_some() {
            commands.entity(e).try_remove::<Mesh3d>();
        }
        return;
    }
    set_ribbon(
        commands,
        assets,
        e,
        &mut r.mesh,
        a,
        d / len,
        len,
        r.scroll,
        &r.ribbon,
        fwd,
    );
}
