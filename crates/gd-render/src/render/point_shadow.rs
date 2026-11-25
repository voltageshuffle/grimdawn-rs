//! Point-light shadows the way the engine picks and builds them: which scene
//! lights get a map, of which size, and the per-light tap spread the
//! materials read, and the shadow views.
//!
//! Engine.dll:
//! - A light record's `shadowPriority` (`Light::Load`, also
//!   `FireLight` / `DayNightLight`, `Light.shadowPriority`) goes to the scene light by
//!   `Light::AddToScene` only when > 0; `GraphicsLight::SetShadowPriority`
//!   turns it into the light's shadow flag (`GraphicsLight.castsShadow`) as a threshold on
//!   `shadowQuality`: priority ≤ 1 / 2 / 3 / 4 / 4 for low / medium / high /
//!   very high / ultra ([`priority_casts`]). Stock values: 1-4 (campfires 4,
//!   `light_firecauldron01` 1), the `_noshadow` lights 8-9999 (never). Not for
//!   a light in simple mode (`Light.simpleMode`), and with `GraphicsEngine::UseLowQualityLighting`
//!   (`lightingQuality` low) only for a light with `Light.forceShadows`
//!   set (`Light::SetForceShadows`, called by none of Engine.dll /
//!   Game.dll / Grim Dawn.exe), so then none ([`lights_cast`]). Effect lights:
//!   priority 1 with the emitter's bools[7].
//! - Map pools (`GraphicsEngine::GetShadowMapSizes` → `GraphicsMTRenderer::RecreateRenderTargets` /
//!   `CreateRenderTargets`): `full` maps of the point size
//!   256 / 384 / 512 / 1024 / 2048 (`GraphicsMTRenderer.pointShadowSize`, `fullShadowCount` 1 / 2 / 3 / 3 / 4)
//!   and `half` maps of half that size (`halfShadowCount` 3 / 4 / 6 / 8 / 12)
//!   ([`pools`]); a D16 depth target each.
//! - Order: `SetupLights` sorts the frame's lights by their
//!   scissor-rect area in pixels, largest first (`GraphicsMTRenderer::CompareLightScissorArea`, key
//!   `(int)(w·h)`); the rect is `GetSphereScissorRect` of the
//!   light's sphere with radius + 1 m (the whole viewport when the camera is
//!   within radius + 2 m of it along the view axis, empty when it is behind);
//!   a light with an empty rect is not in the frame. `BuildShadows`
//!   then hands the flagged lights, in that order, the full maps
//!   first, then the half ones; the rest get the renderer's 16² dummy (cleared,
//!   lit). Nothing when `shadows` is off (`GraphicsMTRenderer.shadows`).
//! - The map (`BuildShadowPoint`, `pLightShadowVS` of the
//!   material shaders): ONE paraboloid of the hemisphere below the light. The
//!   light frame has rows (−1, 0, 0), (0, 1, 0), (0, 0, −1) about the light
//!   position; for a unit direction q in it, clip xy = (q.x, q.z) / (q.y − 1),
//!   depth = (d − 0.1) / 29.9 (d the distance; beyond 30 m clipped), and the
//!   alpha test is `a · (0.5 − q.y) ≥ 0.5` (nothing above the light's
//!   horizon casts). The viewport leaves a 1-texel border, cleared to 1.
//! - Casters: `Region::GetEntitiesInSphere` (light position, radius, list 2):
//!   Actors whose mesh `GetCastsShadows(lightGroup)` (`GraphicsMeshInstance.castsShadows` and not the
//!   `ignoreLightShadows` group); no visibility test.
//! - Receiver (deferred.ssh `pointPS`, `RenderDeferredPointLight`):
//!   the same paraboloid uv of the light→pixel direction,
//!   uv = (0.5 + 0.5 s, 0.5 − 0.5 t), 4 taps at ±½·`shadowBluriness` in a plus
//!   (bluriness = softness 1.5 (`SetShadowSoftness`, `GraphicsMTRenderer.shadowSoftness`) / the
//!   map's width, so ±0.75 texel of a full or a half map), each ¼ lit unless
//!   the stored depth < sat((d − 0.1)·0.0334448 + `shadowBias`), bias =
//!   `Engine::GetDeferredPointShadowBias` −0.0003 (Engine ctor).
//!   The result scales the light's diffuse term only.
//!
//! The port draws each map as the engine does, one view per light: a
//! shadow-only [`SpotLight`] ([`ShadowSpot`]: intensity 0, the light's range,
//! pointing down, a cone just under the lower hemisphere) whose shadow view
//! Bevy culls and renders; its projection is a tag ([`spot_code`]) that
//! gd_prepass.wesl / gd_wind.wesl (the Grim Dawn materials' shadow vertex
//! programs) read to write the engine's paraboloid instead, into the top-left
//! W×W of the spot's layer of the directional shadow array (S texels, the sun
//! map size), with the engine's 1-texel viewport border; gd_standard.wesl's
//! shadow fragment applies the `a·(0.5 − q.y)` alpha test. The receivers
//! (`point_shadow` in gd_standard / gd_terrain / decal.wesl) find that layer
//! by the spot's position in Bevy's light list and take the engine's 4
//! bilinear-depth taps.
//!
//! PORT: Bevy's 32-bit depth holds 1 − the engine's 16-bit linear depth; a
//! triangle crossing the light's horizon is clipped per vertex (z < 0) where
//! the engine interpolates the alpha factor; casters are the meshes without
//! `NotShadowCaster` in the spot's cone and range (0.1 m slab under the light
//! culled, the cone ≤ 0.9° short of the horizon). Non-Grim Dawn materials would draw with the spot's
//! own projection (trails, beams, FX, weather and water are `NotShadowCaster`). The player light's priority (4 at
//! `lightingQuality` 3) is a [`ShadowPriority`] like the others.
//!
//! PORT: below ultra [`pools`] hands out half the engine's half maps (rounded up: 2 / 2 / 3 / 4 at low /
//! medium / high / very high); the lights of least screen area past them get no map, as past the engine's
//! pools. Ultra keeps the engine's 4 + 12.
use bevy::camera::primitives::{Frustum, Sphere};
use bevy::ecs::entity::EntityHashMap;
use bevy::light::{DirectionalLightShadowMap, SimulationLightSystems};
use bevy::prelude::*;

/// A light record's `shadowPriority` (> 0).
#[derive(Component, Clone, Copy, Debug)]
pub struct ShadowPriority(pub i32);

/// The engine map this frame gave the light (its width in texels; none when
/// it has no map).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct PointShadowMap(pub Option<u32>);

impl PointShadowMap {
    /// `shadowBluriness`: the softness over the map width (0 = no shadow).
    pub fn bluriness(&self) -> f32 {
        self.0
            .map_or(0.0, |w| super::shadow::SHADOW_SOFTNESS / w as f32)
    }
}

pub struct PointShadowPlugin;

impl Plugin for PointShadowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            assign_point_shadows
                .after(TransformSystems::Propagate)
                // Bevy's bounding-sphere systems queue `insert(Sphere)` on a new shadow spot; despawning that
                // spot ahead of them makes the insert panic.
                .after(SimulationLightSystems::UpdateBounds)
                .before(super::env::EnvSystems)
                .before(SimulationLightSystems::UpdateLightFrusta)
                .before(SimulationLightSystems::AssignLightsToClusters),
        );
    }
}

/// `GraphicsLight::SetShadowPriority`: does a light of this priority cast at
/// this `shadowQuality` index?
pub fn priority_casts(priority: i32, quality: u32) -> bool {
    let max = match quality {
        0 => 1,
        1 => 2,
        2 => 3,
        3 | 4 => 4,
        _ => 0,
    };
    priority > 0 && priority <= max
}

/// `Light::AddToScene`: does any light get a shadow priority at
/// this `lightingQuality` (none when low)?
pub fn lights_cast(options: Option<&gd_game_core::options::GameOptions>) -> bool {
    !matches!(
        options.and_then(|o| o.get("lightingQuality")),
        Some(gd_formats::options::Value::Quality(q)) if super::shadow::quality_index(q) == 0
    )
}

/// (full map size, full count, half count) of a quality (`GraphicsEngine::GetShadowMapSizes`; half
/// the half maps below ultra).
pub fn pools(quality: u32) -> (u32, usize, usize) {
    match quality {
        0 => (256, 1, 2),
        1 => (384, 2, 2),
        2 => (512, 3, 3),
        3 => (1024, 3, 4),
        _ => (2048, 4, 12),
    }
}

/// `GetSphereScissorRect`'s area in pixels of a sphere (camera-space centre
/// `c`: x right, y up, z forward; radius already + 1 m) for a vertical field
/// of view `fov` and a `w`×`h` viewport.
pub fn scissor_area(c: Vec3, r: f32, fov: f32, w: f32, h: f32) -> f32 {
    if c.z < -r {
        return 0.0;
    }
    if c.z < r + 1.0 {
        return w * h;
    }
    let t = (fov * 0.5).tan();
    // The tangents from the eye to the circle in one axis' plane, in NDC.
    let span = |a: f32, k: f32| -> Option<(f32, f32)> {
        let d = (a * a + c.z * c.z).sqrt();
        if d <= r {
            return None;
        }
        let phi = a.atan2(c.z);
        let delta = (r / d).asin();
        let lo = (phi - delta).tan() / k;
        let hi = (phi + delta).tan() / k;
        Some((lo.clamp(-1.0, 1.0), hi.clamp(-1.0, 1.0)))
    };
    let (Some((x0, x1)), Some((y0, y1))) = (span(c.x, t * w / h), span(c.y, t)) else {
        return w * h;
    };
    let px = ((x1 - x0) * 0.5 * w).max(0.0);
    let py = ((y1 - y0) * 0.5 * h).max(0.0);
    px * py
}

/// The engine's map assignment: `(area, key)` of the flagged lights in the
/// frame (keys in the frame's order) → (key, map width) of the lights that get a map, largest area first.
/// Sorts `areas` in place.
pub fn assign(areas: &mut [(i64, usize)], quality: u32) -> impl Iterator<Item = (usize, u32)> + '_ {
    let (size, full, half) = pools(quality);
    // Largest area first, ties in key order: stable like the engine's insertion sort for ≤ 32.
    areas.sort_unstable_by_key(|&(a, k)| (std::cmp::Reverse(a), k));
    areas
        .iter()
        .take_while(|(a, _)| *a > 0)
        .take(full + half)
        .enumerate()
        .map(move |(i, &(_, k))| (k, if i < full { size } else { size / 2 }))
}

/// The shadow-only spot light that renders a point light's paraboloid map.
#[derive(Component)]
pub struct ShadowSpot(pub Entity);

/// The spot's `outer_angle` that tags its view for gd_prepass.wesl: focal
/// length 1 / tan(outer) = (n + 64 k)·1e-4 with n = 64 W / S and S = 1024·2^k
/// (W the engine map, S the directional shadow layer; all stock pairs give an
/// integer n ≤ 32). The cone is just under the lower hemisphere (≤ 0.9° short).
pub fn spot_code(w: u32, s: u32) -> f32 {
    let n = (64 * w / s.max(1)) as f64;
    let k = (s.max(1024) / 1024).ilog2() as f64;
    (1.0 / ((n + 64.0 * k) * 1e-4)).atan() as f32
}

#[derive(Default)]
struct AssignScratch {
    areas: Vec<(i64, usize)>,
    ids: Vec<Entity>,
    want: EntityHashMap<u32>,
    spot_of: EntityHashMap<(u32, Vec3, f32)>,
    new_spots: Vec<(Entity, (u32, Vec3, f32))>,
}

/// `Light::AddToScene`'s frustum test: the light's sphere (position, radius)
/// meets the camera frustum.
pub fn in_view(frustum: &Frustum, pos: Vec3, range: f32) -> bool {
    frustum.intersects_sphere(
        &Sphere {
            center: pos.into(),
            radius: range,
        },
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn assign_point_shadows(
    mut commands: Commands,
    settings: Option<Res<super::shadow::SunShadowSettings>>,
    options: Option<Res<gd_game_core::options::GameOptions>>,
    layer: Res<DirectionalLightShadowMap>,
    cams: Query<
        (&GlobalTransform, &Projection, &Camera, &Frustum),
        (With<crate::camera::GameCamera>, Without<ShadowSpot>),
    >,
    mut lights: Query<
        (
            Entity,
            &GlobalTransform,
            &mut PointLight,
            &ShadowPriority,
            Option<&PointShadowMap>,
        ),
        Without<ShadowSpot>,
    >,
    mut spots: Query<(
        Entity,
        &ShadowSpot,
        &mut SpotLight,
        &mut Transform,
        &mut GlobalTransform,
    )>,
    mut scratch: Local<AssignScratch>,
) {
    let s = settings.map(|s| *s).unwrap_or_default();
    let cam = cams.single().ok();
    let AssignScratch {
        areas,
        ids,
        want,
        spot_of,
        new_spots,
    } = &mut *scratch;
    areas.clear();
    ids.clear();
    want.clear();
    spot_of.clear();
    new_spots.clear();
    if s.enabled
        && lights_cast(options.as_deref())
        && let Some((gt, Projection::Perspective(pp), camera, frustum)) = cam
    {
        let (w, h) = camera
            .physical_viewport_size()
            .map_or((1920.0, 1080.0), |v| (v.x as f32, v.y as f32));
        let view = gt.affine().inverse();
        for (e, lgt, l, p, _) in &lights {
            let pos = lgt.translation();
            // Light::AddToScene: frustum test with the radius, near-black lights skipped.
            let k = if l.range > 0.0 {
                l.intensity / (40_000.0 * l.range)
            } else {
                0.0
            };
            let c = l.color.to_linear();
            if !priority_casts(p.0, s.quality)
                || l.range <= 0.0
                || c.red.max(c.green).max(c.blue) * k <= 0.03
                || !in_view(frustum, pos, l.range)
            {
                continue;
            }
            // Bevy's view space looks down −Z.
            let v = view.transform_point3(pos);
            let a = scissor_area(Vec3::new(v.x, v.y, -v.z), l.range + 1.0, pp.fov, w, h);
            areas.push((a as i64, ids.len()));
            ids.push(e);
        }
    }
    let size = layer.size as u32;
    want.extend(assign(areas, s.quality).map(|(k, w)| (ids[k], w.min(size))));
    for (e, gt, mut l, pr, cur) in &mut lights {
        // The cube maps of Bevy's point lights are not used.
        if l.shadow_maps_enabled {
            l.shadow_maps_enabled = false;
        }
        let m = PointShadowMap(want.remove(&e));
        if let Some(w) = m.0 {
            spot_of.insert(e, (w, gt.translation(), l.range));
        }
        if cur != Some(&m) {
            debug!(
                "point shadow: {e} priority {} map {:?} at {}",
                pr.0,
                m.0,
                gt.translation()
            );
            commands.entity(e).try_insert(m);
        }
    }
    let place = |pos: Vec3| Transform::from_translation(pos).looking_to(Vec3::NEG_Y, Vec3::Z);
    for (e, owner, mut spot, mut tf, mut gtf) in &mut spots {
        let Some((w, pos, range)) = spot_of.remove(&owner.0) else {
            commands.entity(e).try_despawn();
            continue;
        };
        let outer = spot_code(w, size);
        if spot.outer_angle != outer || spot.range != range {
            spot.outer_angle = outer;
            spot.range = range;
        }
        if tf.translation != pos {
            *tf = place(pos);
            *gtf = GlobalTransform::from(*tf);
        }
    }
    // Spawn in entity order (deterministic, unlike the map's iteration).
    new_spots.extend(spot_of.drain());
    new_spots.sort_unstable_by_key(|(owner, _)| *owner);
    for &(owner, (w, pos, range)) in new_spots.iter() {
        let tf = place(pos);
        commands.spawn((
            SpotLight {
                intensity: 0.0,
                range,
                radius: 0.0,
                shadow_maps_enabled: true,
                // Unused by the paraboloid (gd_prepass.wesl); culls the 0.1 m
                // slab below the light, where the engine clips d < 0.1.
                shadow_map_near_z: 0.1,
                inner_angle: 0.0,
                outer_angle: spot_code(w, size),
                ..default()
            },
            tf,
            GlobalTransform::from(tf),
            ShadowSpot(owner),
            Name::new("point shadow view"),
        ));
    }
}
