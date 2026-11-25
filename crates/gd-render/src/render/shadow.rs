//! The sun's shadow map as the engine builds it: one light-space
//! perspective shadow map (LiSPSM) over the visible scene, not Bevy's
//! cascades. The map is rendered by Bevy's directional-light shadow pass
//! (one "cascade" whose matrix [`sun_shadow_cascade`] replaces), the
//! materials sample it themselves.
//!
//! Engine.dll:
//! - Map size by `shadowQuality` (`GraphicsEngine::GetShadowMapSizes`): low 1024,
//!   medium 1536, high / very high 2048, ultra 4096
//!   ([`map_size`]). `shadows = false` (`GraphicsEngine::EnableShadows`)
//!   turns the sun shadow off.
//! - `GraphicsMTRenderer::BuildShadowDirectional`, mode 1 of
//!   `GraphicsMTRenderer.shadowMode` (LiSPSM): the receiver body is the camera frustum cut at
//!   `GraphicsEngine::GetMaxShadowDistance` (`GraphicsEngine.maxShadowDistance`, 80 m from the
//!   `GraphicsEngine` ctor; `Camera::GetSubFrustum` far plane) below ultra
//!   quality, clipped by the scene bounds (`GraphicsScene::CalculateSceneBounds`:
//!   the AABB of everything drawn in the view) and extruded
//!   toward the sun (`ConvexBody::ExtrudeTowardLight`: the planes facing away from the sun plus
//!   silhouette planes). The casters are the actors in that volume
//!   (`Actor::GetVisibility` 1 or 3, `castsShadows` set: `Actor.castsShadows`, 1 from the
//!   `Actor` ctor; Game.dll clears it only for `PhysicsDecoration` and `Rubble`
//!   (their ctors) and a `Destructible`'s pieces (its record's
//!   `brokenCastShadows`, default 0); gibs (`Gib`, a plain
//!   `Actor`) cast), below ultra only those
//!   whose bounding sphere comes within 80 m of the camera's ray toward the
//!   sun; the nearest of them (`L·c − |h|·|L|`) caps the volume on the sun
//!   side (else the scene box does): the volume is clipped at that plane, so
//!   body vertices above the highest caster's top are not in the fit.
//! - `LightSpacePerspectiveShadowMapMatrix` ([`lispsm`]): light
//!   frame with z = the view direction made perpendicular to the light, y =
//!   toward the sun; sinγ = √(1 − (D·L)²), d = the body's depth along z,
//!   n = (1/sinγ + √(1/sinγ · (1/sinγ + d·sinγ))) / sinγ, f = n + d; the
//!   projection centre moves back from the camera by 10·(n − 1) along z; a
//!   perspective along z (near n, far f, x mirrored), then the body's
//!   post-projection bounds are fitted: x → [−1, 1], z → clip y, and the
//!   light axis becomes the depth, (y_max − y/z) / (y_max − y_min) ∈ [0, 1].
//! - The receiver test (deferred.ssh `directionalPS`, set up by
//!   `GraphicsDeferredRenderer::RenderDeferredDirectionalLight`):
//!   4 point taps at ±½·`shadowBluriness` (= softness 1.5 (World ctor, never
//!   set) / map size, so ±0.75 texel) in a plus pattern, each lit (¼) unless
//!   the stored depth < receiver depth + `shadowBias`
//!   (`Engine::GetDeferredDirectionalShadowBias` = −0.001, Engine ctor). The
//!   result scales the direct sun term only (ambient stays); no shadow
//!   intensity (`lightShadowIntensity` is point lights only,
//!   `SetShaderParamsForScene`; `AmbianceManager`'s shadow intensity only
//!   feeds its statistics text). The shadow pass (`shadow` technique) is the
//!   plain depth of the casters with the base-alpha test at 0.5.
//!
//! - The scene bounds are the union of the scene's renderable boxes
//!   (`GraphicsScene::AddRenderable`; `CalculateSceneBounds`
//!   skips entries whose box is not finite, as here); the terrain's renderable
//!   is one batch over its 9×9-cell blocks inside the view frustum, not the level's whole terrain;
//!   water likewise (`WaterRenderInterface::AddToScene`: its
//!   9-unit blocks in the frustum, one batch per water type).
//! - `RenderDebug` (`SetShadowDebugging`) draws, at (10, 64)
//!   and 256² each with UV 0..1 through shaders/debug/shadowmap.ssh (a plain
//!   copy), `GraphicsMTRenderer.dummyShadowTarget` = a 16² target cleared to 1 that `BuildShadows`
//!   never renders (uniform), then, when a directional map was built this
//!   frame (`GraphicsMTRenderer.sunShadowBuilt`), `GraphicsMTRenderer.sunShadowTarget` = the whole sun map (its
//!   depth view, red),
//!   then the point-light maps; the text is the point-map counts.
//!
//! PORT: Bevy needs a power-of-two map, so the matrix squeezes the engine's
//! map into the middle `size` texels of the next power of two (same texel
//! grid and tap offsets). The scene box is the AABB of the main camera's
//! visible meshes (terrain / water: their blocks in the view frustum); a caster is
//! the union of the boxes of a model's shadow-casting meshes (not
//! `NotShadowCaster`, children of one root), standing in for the actor's box.
//! A receiver outside the whole map (past the 80 m cut or the scene box) is lit: the clamped edge texels
//! would stretch the casters on the map's border.
//!
//! Hook: `GD_SHADOWS=0` (as `shadows = false`).
use bevy::camera::primitives::{Aabb, Frustum};
use bevy::camera::visibility::{VisibilitySystems, VisibleEntities};
use bevy::light::cascade::{Cascade, Cascades};
use bevy::light::{
    CascadeShadowConfig, DirectionalLightShadowMap, NotShadowCaster, SimulationLightSystems,
};
use bevy::prelude::*;
use std::any::TypeId;

/// `GraphicsEngine` max shadow distance (`GraphicsEngine.maxShadowDistance`): the receiver body's far plane.
pub const MAX_SHADOW_DISTANCE: f32 = 80.0;
/// `World` shadow softness: the tap spread in texels.
pub const SHADOW_SOFTNESS: f32 = 1.5;

pub struct SunShadowPlugin;

impl Plugin for SunShadowPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DirectionalLightShadowMap {
            size: map_size(1).next_power_of_two() as usize,
        })
        .add_systems(Update, (configure_sun, apply_options))
        .add_systems(
            PostUpdate,
            sun_shadow_cascade
                .after(super::env::EnvSystems)
                .after(bevy::light::cascade::build_directional_light_cascades)
                .after(VisibilitySystems::CheckVisibility)
                .in_set(SimulationLightSystems::UpdateDirectionalLightCascades),
        )
        .add_systems(
            PostUpdate,
            drop_unlit_cascades
                .after(sun_shadow_cascade)
                .in_set(SimulationLightSystems::UpdateDirectionalLightCascades),
        );
    }
}

/// On a camera: the sun's shadow map is not rendered for this view (its materials never sample it; Bevy
/// renders a separate map per view).
#[derive(Component)]
pub struct NoSunShadow;

/// Drops the sun's cascades of cameras that do not see its render layers (paper dolls, the conversation
/// portrait). A [`NoSunShadow`] camera that does share them keeps an empty set (`prepare_lights` unwraps the
/// view's entry): no shadow view, no caster culling.
fn drop_unlit_cascades(
    cams: Query<(
        Entity,
        &Camera,
        Option<&bevy::camera::visibility::RenderLayers>,
        Has<NoSunShadow>,
    )>,
    mut suns: Query<
        (
            &mut Cascades,
            Option<&bevy::camera::visibility::RenderLayers>,
        ),
        With<DirectionalLight>,
    >,
) {
    for (mut cascades, light_layers) in &mut suns {
        let light_layers = light_layers.cloned().unwrap_or_default();
        for (e, cam, layers, no_shadow) in &cams {
            if !cam.is_active {
                continue;
            }
            if !layers
                .cloned()
                .unwrap_or_default()
                .intersects(&light_layers)
            {
                if cascades.cascades.contains_key(&e) {
                    cascades.cascades.remove(&e);
                }
            } else if no_shadow && let Some(c) = cascades.cascades.get_mut(&e) {
                c.clear();
            }
        }
    }
}

/// Engine shadow quality index of the option text (`Options` file reader
/// `Options::LoadFile`: low 0, medium 1 (anything else), high 2, very high 3,
/// ultra 4).
pub fn quality_index(text: &str) -> u32 {
    match text {
        "low" => 0,
        "high" => 2,
        "very high" => 3,
        "ultra" => 4,
        _ => 1,
    }
}

/// Directional shadow map size of a quality (`GraphicsEngine::GetShadowMapSizes`).
pub fn map_size(quality: u32) -> u32 {
    match quality {
        0 => 1024,
        1 => 1536,
        2 | 3 => 2048,
        _ => 4096,
    }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SunShadowSettings {
    pub enabled: bool,
    pub quality: u32,
}

impl Default for SunShadowSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            quality: 1,
        }
    }
}

fn apply_options(
    options: Res<gd_game_core::options::GameOptions>,
    settings: Option<Res<SunShadowSettings>>,
    mut commands: Commands,
    mut map: ResMut<DirectionalLightShadowMap>,
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
) {
    if settings.is_some() && !options.is_changed() {
        return;
    }
    let quality = match options.get("shadowQuality") {
        Some(gd_formats::options::Value::Quality(q)) => quality_index(q),
        _ => 1,
    };
    let enabled = options.flag("shadows");
    #[cfg(feature = "dev")]
    let enabled = enabled && !hooks.render.no_shadows;

    let s = SunShadowSettings { enabled, quality };
    if settings.as_deref() != Some(&s) {
        commands.insert_resource(s);
    }
    let size = map_size(quality).next_power_of_two() as usize;
    if map.size != size {
        map.size = size;
    }
}

/// One shadow view per camera (the engine's single map).
fn configure_sun(mut commands: Commands, suns: Query<Entity, Added<DirectionalLight>>) {
    for e in &suns {
        commands.entity(e).insert(CascadeShadowConfig {
            bounds: vec![MAX_SHADOW_DISTANCE],
            overlap_proportion: 0.0,
            minimum_distance: 0.1,
        });
    }
}

/// A mesh that enters the scene bounds as its blocks inside the view frustum
/// (local AABBs, min / max): the terrain and water render batches.
#[derive(Component, Clone, Debug, Default)]
pub struct SceneBlocks(pub Vec<(Vec3, Vec3)>);

/// A plane `n·p + d ≥ 0` (inside).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub n: Vec3,
    pub d: f32,
}

impl Plane {
    fn through(n: Vec3, p: Vec3) -> Self {
        Self { n, d: -n.dot(p) }
    }
    fn dist(&self, p: Vec3) -> f32 {
        self.n.dot(p) + self.d
    }
}

/// The inside planes of a frustum from its corners (near: bottom-right,
/// top-right, top-left, bottom-left, then the same for the far plane, as
/// `Projection::get_frustum_corners` returns them).
pub fn frustum_planes(c: [Vec3; 8]) -> [Plane; 6] {
    let centre = c.iter().copied().sum::<Vec3>() / 8.0;
    let face = |a: Vec3, b: Vec3, d: Vec3| {
        let n = (b - a).cross(d - a).normalize_or_zero();
        let p = Plane::through(n, a);
        if p.dist(centre) < 0.0 {
            Plane::through(-n, a)
        } else {
            p
        }
    };
    [
        face(c[0], c[1], c[2]), // near
        face(c[4], c[5], c[6]), // far
        face(c[0], c[1], c[4]), // right
        face(c[2], c[3], c[6]), // left
        face(c[1], c[2], c[5]), // top
        face(c[3], c[0], c[7]), // bottom
    ]
}

/// The inside planes of an AABB.
pub fn box_planes(min: Vec3, max: Vec3) -> [Plane; 6] {
    [
        Plane {
            n: Vec3::X,
            d: -min.x,
        },
        Plane {
            n: -Vec3::X,
            d: max.x,
        },
        Plane {
            n: Vec3::Y,
            d: -min.y,
        },
        Plane {
            n: -Vec3::Y,
            d: max.y,
        },
        Plane {
            n: Vec3::Z,
            d: -min.z,
        },
        Plane {
            n: -Vec3::Z,
            d: max.z,
        },
    ]
}

/// The vertices of the convex polytope bounded by `planes` (every triple
/// intersection inside all planes).
pub fn polytope_vertices(planes: &[Plane]) -> Vec<Vec3> {
    let mut out = Vec::new();
    for i in 0..planes.len() {
        for j in i + 1..planes.len() {
            for k in j + 1..planes.len() {
                let (a, b, c) = (planes[i], planes[j], planes[k]);
                let det = a.n.dot(b.n.cross(c.n));
                if det.abs() < 1e-6 {
                    continue;
                }
                let p =
                    (b.n.cross(c.n) * -a.d + c.n.cross(a.n) * -b.d + a.n.cross(b.n) * -c.d) / det;
                let tol = 1e-3 * (1.0 + p.abs().max_element());
                if planes.iter().all(|q| q.dist(p) >= -tol) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// The body's planes extruded toward the sun (`light` = the direction the
/// light travels), `ConvexBody::ExtrudeTowardLight`: the planes the sweep does not cross plus,
/// for each of them and each plane it does cross, the plane through their
/// intersection line parallel to the light (oriented to keep `inside`).
pub fn extrude_toward_light(planes: &[Plane], light: Vec3, inside: Vec3) -> Vec<Plane> {
    let mut out = Vec::new();
    for (i, a) in planes.iter().enumerate() {
        if a.n.dot(light) >= 0.0 {
            continue;
        }
        out.push(*a);
        for (j, b) in planes.iter().enumerate() {
            if i == j || b.n.dot(light) < 0.0 {
                continue;
            }
            let dir = a.n.cross(b.n);
            let len2 = dir.length_squared();
            if len2 < 1e-10 {
                continue;
            }
            // A point on both planes.
            let p = (b.n.cross(dir) * -a.d + dir.cross(a.n) * -b.d) / len2;
            let n = dir.normalize().cross(-light).normalize_or_zero();
            if n == Vec3::ZERO {
                continue;
            }
            let q = Plane::through(n, p);
            out.push(if q.dist(inside) < 0.0 {
                Plane::through(-n, p)
            } else {
                q
            });
        }
    }
    out
}

/// Whether an AABB (centre, half extents) is not entirely outside one of the planes.
fn aabb_touches(planes: &[Plane], c: Vec3, h: Vec3) -> bool {
    planes.iter().all(|p| p.dist(c) + h.dot(p.n.abs()) >= 0.0)
}

/// `LightSpacePerspectiveShadowMapMatrix`: the D3D clip-from-world matrix
/// (column vectors; depth 0 nearest the sun) fitted to `points`, for a
/// camera at `eye` looking along `view`, the light travelling along `light`.
pub fn lispsm(eye: Vec3, view: Vec3, light: Vec3, points: &[Vec3]) -> Option<Mat4> {
    if points.is_empty() {
        return None;
    }
    let up = -light; // toward the sun: the light-space y (depth) axis
    let z = (view - light * light.dot(view)).normalize_or_zero();
    if z == Vec3::ZERO {
        return None;
    }
    let x = up.cross(z).normalize();
    let frame = |e: Vec3| {
        Mat4::from_cols(
            Vec4::new(x.x, up.x, z.x, 0.0),
            Vec4::new(x.y, up.y, z.y, 0.0),
            Vec4::new(x.z, up.z, z.z, 0.0),
            Vec4::new(-x.dot(e), -up.dot(e), -z.dot(e), 1.0),
        )
    };
    let v0 = frame(eye);
    let (mut zmin, mut zmax) = (f32::INFINITY, f32::NEG_INFINITY);
    for p in points {
        let q = v0.transform_point3(*p);
        zmin = zmin.min(q.z);
        zmax = zmax.max(q.z);
    }
    let d = (zmax - zmin).abs();
    let sin_g = (1.0 - view.dot(light).powi(2)).max(0.0).sqrt().max(1e-3);
    let zn = 1.0 / sin_g;
    let n = ((zn * (d * sin_g + zn)).sqrt() + zn) / sin_g;
    let f = n + d;
    let v = frame(eye + z * ((n - 1.0) * -10.0));
    let (a, b) = ((f + n) / (f - n), -2.0 * f * n / (f - n));
    // Perspective along z: x mirrored, w = z.
    let p = Mat4::from_cols(
        Vec4::new(-1.0, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 1.0, 0.0, 0.0),
        Vec4::new(0.0, 0.0, a, 1.0),
        Vec4::new(0.0, 0.0, b, 0.0),
    );
    let pv = p * v;
    let (mut lo, mut hi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for q in points {
        let c = pv * q.extend(1.0);
        if c.w <= 1e-6 {
            continue;
        }
        let s = c.truncate() / c.w;
        lo = lo.min(s);
        hi = hi.max(s);
    }
    if !(hi.cmpgt(lo).all()) {
        return None;
    }
    let (dx, dy, dz) = (hi.x - lo.x, hi.y - lo.y, hi.z - lo.z);
    // Crop: x' → clip x, z' → clip y, y' → depth (y_max − y') / (y_max − y_min).
    let crop = Mat4::from_cols(
        Vec4::new(2.0 / dx, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 0.0, -1.0 / dy, 0.0),
        Vec4::new(0.0, 2.0 / dz, 0.0, 0.0),
        Vec4::new(-(lo.x + hi.x) / dx, -(lo.z + hi.z) / dz, hi.y / dy, 1.0),
    );
    Some(crop * pv)
}

pub struct ShadowScene<'a> {
    pub eye: Vec3,
    pub view: Vec3,
    /// Direction the sunlight travels.
    pub light: Vec3,
    /// Camera frustum corners (near ... far).
    pub frustum: [Vec3; 8],
    /// AABB of the visible scene, if anything is visible.
    pub scene: Option<(Vec3, Vec3)>,
    /// Casters: (AABB centre, half extents).
    pub casters: &'a [(Vec3, Vec3)],
    /// Below ultra: the 80 m caster cylinder around the camera's sun ray.
    pub limit_casters: bool,
}

/// The casters' cap on the sun side of the volume (`L·c − |h|·|L|`, the
/// least over the casters in it) and the index of the caster that sets it.
pub fn caster_cap(s: &ShadowScene, volume: &[Plane]) -> (f32, Option<usize>) {
    let l = s.light;
    let mut cap = (f32::INFINITY, None);
    for (i, (c, h)) in s.casters.iter().enumerate() {
        if !aabb_touches(volume, *c, *h) {
            continue;
        }
        if s.limit_casters {
            let r = h.length();
            let to_eye = s.eye - *c;
            let along = l.dot(to_eye).max(0.0);
            if (to_eye - l * along).length_squared() >= (r + MAX_SHADOW_DISTANCE).powi(2) {
                continue;
            }
        }
        let v = l.dot(*c) - h.dot(l.abs());
        if v < cap.0 {
            cap = (v, Some(i));
        }
    }
    cap
}

/// The receiver body and its volume extruded toward the sun.
pub fn shadow_volume(s: &ShadowScene) -> Option<(Vec<Vec3>, Vec<Plane>)> {
    let mut planes = Vec::with_capacity(12);
    planes.extend(frustum_planes(s.frustum));
    if let Some((min, max)) = s.scene {
        planes.extend(box_planes(min, max));
    }
    let body = polytope_vertices(&planes);
    if body.is_empty() {
        return None;
    }
    let inside = body.iter().copied().sum::<Vec3>() / body.len() as f32;
    let volume = extrude_toward_light(&planes, s.light, inside);
    Some((body, volume))
}

/// `BuildShadowDirectional`: the D3D clip-from-world matrix of the sun map.
pub fn build_directional(s: &ShadowScene) -> Option<Mat4> {
    let (body, volume) = shadow_volume(s)?;
    let l = s.light;
    let (mut cap, _) = caster_cap(s, &volume);
    if !cap.is_finite()
        && let Some((min, max)) = s.scene
    {
        cap = l.dot((min + max) * 0.5) - ((max - min) * 0.5).dot(l.abs());
    }
    if !cap.is_finite() {
        return lispsm(s.eye, s.view, l, &body);
    }
    // The extruded volume clipped at the cap plane (`L·p ≥ cap`): body
    // vertices above the highest caster's top go, the cut and the
    // extrusion's end on the cap plane come in.
    let mut planes = volume;
    planes.push(Plane { n: l, d: -cap });
    lispsm(s.eye, s.view, l, &polytope_vertices(&planes))
}

/// Bevy shadow-view matrix of an engine map: reverse Z, the engine's
/// `size`² map in the middle of Bevy's `bevy_size`² one.
pub fn to_bevy_clip(m: Mat4, size: u32, bevy_size: u32) -> Mat4 {
    let k = size as f32 / bevy_size as f32;
    let r = m.transpose(); // rows
    Mat4::from_cols(r.x_axis * k, r.y_axis * k, r.w_axis - r.z_axis, r.w_axis).transpose()
}

#[allow(clippy::too_many_arguments)]
fn sun_shadow_cascade(
    settings: Option<Res<SunShadowSettings>>,
    map: Res<DirectionalLightShadowMap>,
    cams: Query<
        (
            Entity,
            &GlobalTransform,
            &Projection,
            &VisibleEntities,
            &Frustum,
        ),
        With<crate::camera::GameCamera>,
    >,
    meshes: Query<(&Aabb, &GlobalTransform, Option<&SceneBlocks>)>,
    // Shadow casters: meshes with a box, not `NotShadowCaster`, not terrain /
    // water (`SceneBlocks`).
    casters: Query<
        (
            Entity,
            &Aabb,
            &GlobalTransform,
            &InheritedVisibility,
            Option<&ChildOf>,
        ),
        (With<Mesh3d>, Without<NotShadowCaster>, Without<SceneBlocks>),
    >,
    mut suns: Query<(&Transform, &mut DirectionalLight, &mut Cascades)>,
    mut actors: Local<bevy::ecs::entity::EntityHashMap<(Vec3, Vec3)>>,
    mut list: Local<Vec<(Vec3, Vec3)>>,
) {
    let s = settings.map(|s| *s).unwrap_or_default();
    let world_aabb = |aabb: &Aabb, gt: &GlobalTransform| {
        let m = gt.affine();
        let c = m.transform_point3(aabb.center.into());
        let he: Vec3 = aabb.half_extents.into();
        let h = m.matrix3.x_axis.abs() * he.x
            + m.matrix3.y_axis.abs() * he.y
            + m.matrix3.z_axis.abs() * he.z;
        (c, Vec3::from(h))
    };
    // Other cameras (the glow view) keep Bevy's cascade: the shadow pass
    // needs an entry per view.
    for (_, mut sun, _) in &mut suns {
        if !s.enabled && sun.shadow_maps_enabled {
            sun.shadow_maps_enabled = false;
        }
    }
    if !s.enabled {
        return;
    }
    let Ok((cam_e, cam_gt, proj, visible, view_frustum)) = cams.single() else {
        return;
    };
    let Projection::Perspective(pp) = proj else {
        return;
    };
    let far = if s.quality < 4 {
        MAX_SHADOW_DISTANCE.min(pp.far)
    } else {
        pp.far
    };
    let world_from_view = cam_gt.to_matrix();
    let frustum = proj
        .get_frustum_corners(-0.001, -far)
        .map(|c| world_from_view.transform_point3(c.into()));
    let mut scene: Option<(Vec3, Vec3)> = None;
    let mut grow = |c: Vec3, h: Vec3| {
        if c.is_finite() && h.is_finite() {
            scene = Some(match scene {
                Some((lo, hi)) => (lo.min(c - h), hi.max(c + h)),
                None => (c - h, c + h),
            });
        }
    };
    for e in visible.iter(TypeId::of::<Mesh3d>()) {
        let Ok((aabb, gt, blocks)) = meshes.get(*e) else {
            continue;
        };
        match blocks {
            // Terrain / water count with their blocks in the view frustum.
            Some(blocks) => {
                let affine = gt.affine();
                for (lo, hi) in &blocks.0 {
                    let b = Aabb::from_min_max(*lo, *hi);
                    if view_frustum.intersects_obb(&b, &affine, true, true) {
                        let (c, h) = world_aabb(&b, gt);
                        grow(c, h);
                    }
                }
            }
            None => {
                let (c, h) = world_aabb(aabb, gt);
                grow(c, h);
            }
        }
    }
    // One box per actor (`Actor.boundsCenter`, `Actor.boundsHalfExtents`): the union of
    // its meshes' boxes (the meshes of a model are children of its root).
    actors.clear();
    for (e, a, gt, v, parent) in &casters {
        if !v.get() {
            continue;
        }
        let (c, h) = world_aabb(a, gt);
        if !(c.is_finite() && h.is_finite()) {
            continue;
        }
        actors
            .entry(parent.map_or(e, ChildOf::parent))
            .and_modify(|(lo, hi)| {
                *lo = lo.min(c - h);
                *hi = hi.max(c + h);
            })
            .or_insert((c - h, c + h));
    }
    list.clear();
    list.extend(
        actors
            .values()
            .map(|&(lo, hi)| ((lo + hi) * 0.5, (hi - lo) * 0.5)),
    );
    let size = map_size(s.quality);
    let bevy_size = map.size as u32;
    for (tf, sun, mut cascades) in &mut suns {
        if !sun.shadow_maps_enabled {
            continue;
        }
        let light = tf.forward().as_vec3();
        let shadow_scene = ShadowScene {
            eye: cam_gt.translation(),
            view: cam_gt.forward().as_vec3(),
            light,
            frustum,
            scene,
            casters: &list,
            limit_casters: s.quality < 4,
        };
        let Some(m) = build_directional(&shadow_scene) else {
            continue;
        };
        let clip_from_world = to_bevy_clip(m, size, bevy_size);
        let world_from_cascade = Mat4::from_rotation_translation(tf.rotation, cam_gt.translation());
        let cascade = Cascade {
            world_from_cascade,
            clip_from_cascade: clip_from_world * world_from_cascade,
            clip_from_world,
            // Read by the materials' `sun_shadow` as the tap offset in map
            // UV: ½·shadowBluriness = ½ · softness / size, in the squeezed map.
            texel_size: 0.5 * SHADOW_SOFTNESS / bevy_size as f32,
        };
        let slot = cascades.cascades.entry(cam_e).or_default();
        slot.clear();
        slot.push(cascade);
    }
}
