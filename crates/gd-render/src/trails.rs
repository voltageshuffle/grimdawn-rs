//! Projectile trails (`projectileWeaponTrail` → a `WeaponTrail` record):
//! a ribbon through the recent positions of two anchor points, textured with
//! `Texture` (additive `trailadditive.ssh`: texture × vertex colour, nothing
//! more, per its ps_1_1 code), coloured by `Red/Green/Blue/Alpha256`.
//!
//! Engine.dll `TrailEffect` (`Load`, `InternalUpdate`, `ScaleSegments`, `DecrementAlpha`,
//! `CleanSegmentList`, `RenderPass`):
//! - record defaults: `TextureRepeat`, `DragUVs` and `FadeAlpha` true,
//!   `MSFadeTime` 500, colours 0, `Texture` `System/Textures/White.tex`;
//! - a segment (both anchor positions, a texture u, colour and alpha) is
//!   added once the first anchor moved 0.05 m from the newest segment; its u
//!   is the newest one's plus that distance / `TextureDistance`;
//! - every update `TrailShrinkSpeed` pulls both edges of every segment toward
//!   their midpoint by the factor 1 − dt(s) × speed, and with `FadeAlpha`
//!   each alpha drops by dt / `MSFadeTime` (from `Alpha256`, clamped at 0);
//! - the oldest segments go once faded (alpha ≤ 0.0001), or without
//!   `FadeAlpha` once collapsed (edges ≤ 0.01 m apart); at most 500 are drawn;
//! - u: with `DragUVs` or without `TextureRepeat` it is the distance from
//!   the head / `TextureDistance` (without `TextureRepeat` the ribbon turns
//!   transparent past u ≈ 1); otherwise each segment keeps its stored u.
//!
//! The anchors are the projectile mesh's `Anchor1`/`Anchor2` attach points
//! (`Anchor3`/`Anchor4` for a second trail): `ProjectileBase::CountWeaponTrails`
//! (Game.dll) makes no trail without `Anchor1`, `WeaponTrail::GetAnchorPoints`
//! (Engine.dll) reads them every update ([`Trail::with_anchors`],
//! `gd_game_core::visuals::anchors_of`); a mesh with `Anchor3` gets a second trail between
//! `Anchor3`/`Anchor4`. The projectile spawner (`casting.rs`) gives every
//! trail its mesh's anchors and makes none without `Anchor1`. PORT: a trail
//! created without anchors (no caller does) falls back to a camera-facing
//! ribbon `width` wide through the owner's position.
//!
//! `TrailEffect::StartFade` sets the fade flag (`TrailEffect.fading`, the
//! record's `FadeAlpha`), `Liberate` stops new segments (`TrailEffect.liberated`):
//! gameplay asks for both with `StopEmitting` on the trail entity
//! (`gd_game::line_fan`). A let-go trail destroys itself once it has fewer
//! than two segments (`TrailEffect::UpdateSelf`: `TrailEffect.liberated` and
//! segment count < 2 → `ObjectManager::DestroyObjectEx` on itself). A trail
//! that was not let go is an attached entity of its owner (`Entity::Attach`
//! in `ProjectileBase::SwitchWeaponTrail` and the weapon's),
//! and `ObjectManager::DestroyObjectEx` detaches and
//! destroys every attached entity with the entity itself
//! (`ProjectileBase::DeleteProjectile` ends there; no code
//! stops or fades the projectile's trails first): it goes at once with its
//! owner, segments and all ([`ended`]). `StartPulse` starts the alpha pulse
//! (`pulseAlphaMin` / `…Max` / `…CycleTime`, `PulseAlpha` in the update; the
//! `StartPulse` component, a line fan's line at the end of its travel).
//!
//! Weapon trails (`Weapon::SwitchWeaponTrail`, Game.dll): the
//! weapon record's `weaponTrail` (or its weapon enchantment's), attached to
//! the weapon with its mesh's `Anchor1`/`Anchor2`; it adds segments only
//! between `Start` and `Stop` ([`Trail::start`], driven by `fx::enchant`).
use bevy::prelude::*;
use gd_formats::msh::AttachPoint;
use std::collections::VecDeque;
use std::sync::Arc;

pub struct TrailsPlugin;

impl Plugin for TrailsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, dress_trails).add_systems(
            PostUpdate,
            update_trails
                .after(TransformSystems::Propagate)
                .after(crate::character::CharacterRenderStage::Pose),
        );
    }
}

/// Segments drawn at most (`RenderPass`).
const MAX_SEGMENTS: usize = 500;

/// One stored segment: edge positions (world), texture u, alpha.
#[derive(Clone, Copy)]
struct Segment {
    upper: Vec3,
    lower: Vec3,
    u: f32,
    alpha: f32,
}

/// A trail following `owner`; its end: [`ended`].
#[derive(Component)]
#[require(Visibility)]
pub struct Trail {
    pub owner: Entity,
    pub record: String,
    /// Fallback ribbon width when there are no anchors.
    pub width: f32,
    /// `Anchor1`/`Anchor2` in the owner's local space.
    pub anchors: Option<(Vec3, Vec3)>,
    /// Attach points of the owner character's body, posed each update
    /// instead of fixed `anchors` (a character's unarmed trail, `fx::enchant`).
    /// The names, kept to re-resolve when a form swap replaces the body mesh.
    pub points: Option<(String, String)>,
    /// `points` resolved on the body mesh they were read from (at spawn,
    /// [`Trail::with_points`]): no per-frame attach-point name scan.
    resolved: Option<ResolvedPoints>,
    /// Adding segments (`TrailEffect::Start` / `Stop`): a weapon's trail
    /// runs only during a swing (`fx::enchant`); a projectile's always.
    active: bool,
    /// Newest first.
    segments: VecDeque<Segment>,
    state: Option<TrailState>,
}

/// A trail's [`Trail::points`] on the body mesh they were resolved from;
/// re-read (by name) when a form swap replaces the mesh.
struct ResolvedPoints {
    body: Arc<gd_formats::msh::Mesh>,
    a: Option<AttachPoint>,
    b: Option<AttachPoint>,
}

struct TrailState {
    /// Made once the ribbon first has 2+ segments (an empty `Mesh` asset is never added: the allocator warns about
    /// it every frame), then kept: a persistent grow-only mesh rewritten in place (no per-frame replacement), the
    /// entity hidden while fewer than 2 segments are left (a weapon's trail between swings).
    mesh: Option<Handle<Mesh>>,
    /// The mesh's capacity in segments (grow-only, power of two).
    cap: usize,
    fade: f32,
    fade_alpha: bool,
    drag_uvs: bool,
    repeat: bool,
    distance: f32,
    shrink: f32,
    color: [f32; 4],
    /// `trailcombine.ssh`: alpha blended instead of additive.
    combine: bool,
    /// `pulseAlphaMin`, `pulseAlphaMax`, `pulseAlphaCycleTime` (s).
    pulse: [f32; 3],
    /// A running pulse (`StartPulse`): (time left in the cycle, the cycle runs backwards).
    pulsing: Option<(f32, bool)>,
}

/// Reused scratch for the ribbon's vertex / index data ([`update_trails`]).
#[derive(Default)]
struct TrailScratch {
    pos: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl Trail {
    pub fn new(owner: Entity, record: &str, width: f32) -> Self {
        Self {
            owner,
            record: record.to_owned(),
            width,
            anchors: None,
            points: None,
            resolved: None,
            active: true,
            segments: VecDeque::new(),
            state: None,
        }
    }
    /// Starts adding segments again. `TrailEffect::Start` (Engine.dll) only refreshes the
    /// bounding box and sets the emitting flag (`TrailEffect.emitting`): the previous swing's segments stay, still
    /// fading, and the
    /// new ribbon joins on to them.
    pub fn start(&mut self) {
        self.active = true;
    }
    /// Stops adding segments; the ones there fade out.
    pub fn stop(&mut self) {
        self.active = false;
    }
    /// `TrailEffect::IsActive`: it has segments (emitting or not).
    pub fn has_segments(&self) -> bool {
        !self.segments.is_empty()
    }
    /// A trail that adds no segments until [`Trail::start`].
    pub fn stopped(mut self) -> Self {
        self.active = false;
        self
    }
    /// Use these owner-local anchor points as the ribbon's edges.
    pub fn with_anchors(mut self, anchors: (Vec3, Vec3)) -> Self {
        self.anchors = Some(anchors);
        self
    }
    /// Use these attach points of the owner character's posed body as the
    /// ribbon's edges (`WeaponTrail::GetAnchorPoints` reads the actor's attach
    /// point coords every update). Resolved on `body` once here; a form swap
    /// that replaces the mesh re-resolves the names.
    pub fn with_points(mut self, body: &Arc<gd_formats::msh::Mesh>, a: &str, b: &str) -> Self {
        self.resolved = Some(ResolvedPoints {
            body: body.clone(),
            a: body.attach_point(a).cloned(),
            b: body.attach_point(b).cloned(),
        });
        self.points = Some((a.to_owned(), b.to_owned()));
        self
    }
}

/// The trail is over and its entity goes (with it the mesh and material,
/// whose only handles it holds). Module doc: a let-go one (`StopEmitting`)
/// once fewer than two segments are left, wherever its owner is; any other
/// one together with its owner.
fn ended(owner_alive: bool, let_go: bool, segments: usize) -> bool {
    if let_go { segments < 2 } else { !owner_alive }
}

// The reused ribbon scratch is a system Local alongside the queries and assets.
#[allow(clippy::too_many_arguments)]
fn update_trails(
    mut commands: Commands,
    time: Res<Time>,
    mut trails: Query<(
        Entity,
        &mut Trail,
        &mut Visibility,
        Has<gd_game_core::visuals::StopEmitting>,
        Has<gd_game_core::visuals::StartPulse>,
    )>,
    owners: Query<&GlobalTransform>,
    chars: Query<&gd_game_core::character::Character>,
    cams: Query<&GlobalTransform, With<crate::camera::GameCamera>>,
    mut assets: crate::character::CharAssets,
    mut scratch: Local<TrailScratch>,
) {
    let dt = time.delta_secs();
    let cam = cams.iter().next().map(|c| c.translation());
    for (e, mut t, mut vis, stopped, pulse) in &mut trails {
        let t = &mut *t;
        if t.state.is_none() {
            let Some(rec) = assets.data.db.get(&t.record) else {
                commands.entity(e).try_despawn();
                continue;
            };
            let img = assets.textures.get(
                &assets.data,
                rec.str("Texture")
                    .filter(|p| !p.is_empty())
                    .unwrap_or("System/Textures/White.tex"),
                false,
                &mut assets.images,
            );
            let c = |k: &str| rec.i32(k).unwrap_or(0) as f32 / 255.0;
            let flag = |k: &str| rec.get(k).is_none() || rec.bool(k);
            // `Shader`: `trailadditive.ssh` (source alpha, destination ONE) or `trailcombine.ssh` (plain alpha
            // blending); the programs are identical.
            let combine = rec
                .str("Shader")
                .or(rec.str("shader"))
                .is_some_and(|s| s.to_ascii_lowercase().contains("trailcombine"));
            let mat = assets.materials.add(StandardMaterial {
                base_color_texture: img,
                unlit: true,
                alpha_mode: if combine {
                    AlphaMode::Blend
                } else {
                    AlphaMode::Add
                },
                cull_mode: None,
                ..default()
            });
            commands.entity(e).insert((
                MeshMaterial3d(mat),
                Transform::IDENTITY,
                GlobalTransform::IDENTITY,
                bevy::light::NotShadowCaster,
                bevy::camera::visibility::NoFrustumCulling,
            ));
            t.state = Some(TrailState {
                mesh: None,
                cap: 0,
                fade: rec.i32("MSFadeTime").unwrap_or(500).max(1) as f32 / 1000.0,
                fade_alpha: flag("FadeAlpha"),
                drag_uvs: flag("DragUVs"),
                repeat: flag("TextureRepeat"),
                distance: rec.f32("TextureDistance").unwrap_or(0.0).max(0.01),
                shrink: rec.f32("TrailShrinkSpeed").unwrap_or(0.0),
                color: [c("Red256"), c("Green256"), c("Blue256"), c("Alpha256")],
                combine,
                pulse: [
                    rec.f32("pulseAlphaMin").unwrap_or(0.0),
                    rec.f32("pulseAlphaMax").unwrap_or(0.0),
                    (rec.f32("pulseAlphaCycleTime").unwrap_or(0.0) * 1000.0) as i32 as f32 / 1000.0,
                ],
                pulsing: None,
            });
        }
        // Gameplay ended it (`StopEmitting`: a line fan's line,
        // `TrailEffect::StartFade` + `Liberate`): no more segments, and the
        // ones there fade whatever `FadeAlpha` says (`StartFade` sets the
        // fade flag `TrailEffect.fading` that `DecrementAlpha` / `CleanSegmentList` test).
        if stopped {
            t.active = false;
            if let Some(s) = &mut t.state {
                s.fade_alpha = true;
            }
        }
        // `StartPulse` (once): the cycle timer full, running forwards.
        if pulse
            && let Some(s) = &mut t.state
            && s.pulsing.is_none()
        {
            s.pulsing = Some((s.pulse[2], false));
        }
        let Some(s) = &t.state else { continue };
        if s.shrink > 0.0 {
            let f = (1.0 - dt * s.shrink).max(0.0);
            for g in t.segments.iter_mut() {
                let mid = (g.upper + g.lower) * 0.5;
                g.upper = mid + (g.upper - mid) * f;
                g.lower = mid + (g.lower - mid) * f;
            }
        }
        if s.fade_alpha {
            let d = dt / s.fade;
            for g in t.segments.iter_mut() {
                g.alpha = (g.alpha - d).max(0.0);
            }
        }
        // `TrailEffect::PulseAlpha`, after `DecrementAlpha`, while not fading: the
        // cycle counts down; every segment's alpha = min + (max − min) × left / cycle (clamped 0..1; 1 − that
        // on a backwards cycle), ≥ 0; at 0 the cycle restarts the other way.
        let fading = s.fade_alpha;
        if let Some(st) = t.state.as_mut()
            && !fading
            && let Some((left, back)) = st.pulsing.as_mut()
        {
            *left -= dt;
            let cycle = st.pulse[2];
            let k = if cycle > 0.0 {
                (*left / cycle).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let k = if *back { 1.0 - k } else { k };
            let a = (st.pulse[0] + (st.pulse[1] - st.pulse[0]) * k).max(0.0);
            for g in t.segments.iter_mut() {
                g.alpha = a;
            }
            if *left <= 0.0 {
                *left = cycle;
                *back = !*back;
            }
        }
        let Some(s) = &t.state else { continue };
        while t.segments.back().is_some_and(|g| {
            if s.fade_alpha {
                g.alpha <= 0.0001
            } else {
                g.upper.distance(g.lower) <= 0.01
            }
        }) {
            t.segments.pop_back();
        }
        let owner = owners.get(t.owner);
        if ended(owner.is_ok(), stopped, t.segments.len()) {
            commands.entity(e).try_despawn();
            continue;
        }
        match owner {
            Ok(_) if !t.active => {}
            Ok(g) => 'add: {
                let posed = t.points.as_ref().and_then(|(a, b)| {
                    let c = chars.get(t.owner).ok()?;
                    if t.resolved
                        .as_ref()
                        .is_none_or(|r| !Arc::ptr_eq(&r.body, &c.body))
                    {
                        t.resolved = Some(ResolvedPoints {
                            body: c.body.clone(),
                            a: c.body.attach_point(a).cloned(),
                            b: c.body.attach_point(b).cloned(),
                        });
                    }
                    let r = t.resolved.as_ref()?;
                    let upper = Vec3::from(r.a.as_ref()?.coords_in(c.posed()).origin);
                    let lower =
                        r.b.as_ref()
                            .map_or(Vec3::ZERO, |p| Vec3::from(p.coords_in(c.posed()).origin));
                    Some((upper, lower))
                });
                let (upper, lower) = match posed.or(t.anchors) {
                    Some((a, b)) => (g.transform_point(a), g.transform_point(b)),
                    None if t.points.is_some() => break 'add,
                    None => {
                        let head = g.translation();
                        let dir = t.segments.front().map_or(Vec3::X, |p| {
                            (head - (p.upper + p.lower) * 0.5).normalize_or(Vec3::X)
                        });
                        let view = cam.map_or(Vec3::Y, |c| (c - head).normalize_or(Vec3::Y));
                        let side = dir.cross(view).normalize_or(Vec3::Z) * (0.5 * t.width);
                        (head + side, head - side)
                    }
                };
                let moved = t.segments.front().map(|p| p.upper.distance(upper));
                if moved.is_none_or(|m| m >= 0.05) && t.segments.len() < MAX_SEGMENTS {
                    let u = t
                        .segments
                        .front()
                        .map_or(0.0, |p| p.u + moved.unwrap_or(0.0) / s.distance);
                    t.segments.push_front(Segment {
                        upper,
                        lower,
                        u,
                        alpha: s.color[3],
                    });
                }
            }
            Err(_) => {}
        }
        if t.segments.len() < 2 {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        vis.set_if_neq(Visibility::Inherited);
        let n = t.segments.len();
        // Grow the persistent mesh only when the ribbon outgrows it; otherwise
        // rewrite its attributes in place (no asset replacement / re-upload).
        let cap = {
            let st = t.state.as_mut().unwrap();
            if st.mesh.is_none() || st.cap < n {
                let cap = n.next_power_of_two().max(8);
                let mut m = Mesh::new(
                    bevy::mesh::PrimitiveTopology::TriangleList,
                    bevy::asset::RenderAssetUsages::default(),
                );
                m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; cap * 2]);
                m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; cap * 2]);
                m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0; 2]; cap * 2]);
                m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0; 4]; cap * 2]);
                m.insert_indices(bevy::mesh::Indices::U32(vec![0; cap * 6]));
                let h = assets.meshes.add(m);
                commands.entity(e).insert(Mesh3d(h.clone()));
                st.mesh = Some(h);
                st.cap = cap;
            }
            st.cap
        };
        let s = t.state.as_ref().unwrap();
        let scratch = &mut *scratch;
        scratch.pos.clear();
        scratch.uv.clear();
        scratch.col.clear();
        scratch.idx.clear();
        let (mut u_acc, mut step) = (0.0, 0.0);
        for i in 0..n {
            let g = t.segments[i];
            scratch.pos.push(g.upper.to_array());
            scratch.pos.push(g.lower.to_array());
            let dragged = s.drag_uvs || !s.repeat;
            let alpha = if s.repeat || u_acc + step <= 0.99 {
                g.alpha
            } else {
                0.0
            };
            let u = if dragged { u_acc } else { g.u };
            if dragged && i + 1 < n {
                step = g.upper.distance(t.segments[i + 1].upper) / s.distance;
                u_acc += step;
            }
            scratch.uv.push([u, 0.0]);
            scratch.uv.push([u, 1.0]);
            // Additive: fading = darkening the colour; combine (alpha blend): fading = the vertex alpha.
            let c = if s.combine {
                [s.color[0], s.color[1], s.color[2], alpha]
            } else {
                [
                    s.color[0] * alpha,
                    s.color[1] * alpha,
                    s.color[2] * alpha,
                    1.0,
                ]
            };
            scratch.col.push(c);
            scratch.col.push(c);
        }
        for i in 0..(n - 1) as u32 {
            let a = i * 2;
            scratch
                .idx
                .extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
        // Pad to capacity with degenerate (zero-index) triangles; the unused
        // vertices keep the last position so the mesh's AABB stays on the ribbon.
        let last = scratch.pos[n * 2 - 1];
        scratch.pos.resize(cap * 2, last);
        scratch.uv.resize(cap * 2, [0.0; 2]);
        scratch.col.resize(cap * 2, [0.0; 4]);
        scratch.idx.resize(cap * 6, 0);
        let h = s.mesh.as_ref().unwrap().clone();
        if let Some(mut mesh) = assets.meshes.get_mut(&h) {
            use bevy::mesh::VertexAttributeValues;
            if let Some(VertexAttributeValues::Float32x3(a)) =
                mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
            {
                a.copy_from_slice(&scratch.pos);
            }
            if let Some(VertexAttributeValues::Float32x2(a)) =
                mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
            {
                a.copy_from_slice(&scratch.uv);
            }
            if let Some(VertexAttributeValues::Float32x4(a)) =
                mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
            {
                a.copy_from_slice(&scratch.col);
            }
            if let Some(bevy::mesh::Indices::U32(a)) = mesh.indices_mut() {
                a.copy_from_slice(&scratch.idx);
            }
        }
    }
}

/// Gameplay's trail requests become trails.
fn dress_trails(
    mut commands: Commands,
    new: Query<
        (Entity, &gd_game_core::visuals::TrailRequest),
        Added<gd_game_core::visuals::TrailRequest>,
    >,
) {
    for (e, r) in &new {
        let mut t = Trail::new(r.owner, &r.record, r.width);
        if let Some(a) = r.anchors {
            t = t.with_anchors(a);
        }
        commands
            .entity(e)
            .insert(t)
            .remove::<gd_game_core::visuals::TrailRequest>();
    }
}
