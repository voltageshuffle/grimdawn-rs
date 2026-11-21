//! See-through scenery: roofs, upper floors and walls between the camera and
//! the player fade out, so the player and chests inside houses stay visible.
//!
//! (Game.dll `GameCamera::UpdateTransparencyFrustum` @0x1802bebd0,
//! frustum builder sub_180594FB0, defaults in `GameEngine::GameEngine`
//! @0x1802c1ad4, `SetTransparencyFrustumParams`): every frame the camera builds
//! a "transparency frustum" with its apex face at the camera target (the
//! player), pointing at the eye raised by 5 m, as long as that distance:
//! near face 1 × 1 m, far face 20 m (right axis) × 25 m (up axis); the axes
//! are `right = Y × f`, `up = f × right` (not normalised, so both shrink with
//! the camera pitch). With a player set, a 7th plane keeps only what is more
//! than 1 m above the target (`GetTransparencyHeight`).
//!
//! (Engine.dll `Level::AddToScene` @0x1801311d0 → `Actor::
//! SetTransparency(Frustum)` @0x18002b120 → `GraphicsMeshInstance::
//! SetTransparency` @0x1800c0470): each visible actor whose record has
//! `allowTransparency` (`Actor::Load`: `transparencyMode` "Instant" = 1, else
//! 2; never set in stock data) tests its mesh's hit boxes (scaled, placed;
//! boxes named "Visibility" skipped; a mesh without hit boxes never fades)
//! against that frustum; if one intersects, every mesh section except one
//! named "NoTransparency" is marked for this frame. Characters, the cursor and
//! other actors are not involved: only the scenery's own records opt in.
//! There is no other way in: the mode (+944) is written only by the
//! `Actor` constructor (0) and `Actor::Load`; a level instance carries only
//! its scale (`Actor::Read`); `GameCamera::UpdateTransparencyFrustum` adds
//! `GridRegion::UpdateTransparency` only for underground grid levels (no
//! stock level has a grid). So the Devil's Crossing prison front
//! (`prisonmodular/*`, `woodbeam*`, the gate `door_devilscrossinggate`: all
//! `allowTransparency` 0; `prison_2ndfloor_corner_large01.msh` has no hit
//! boxes either) never fades over the player at the gate, in the game too.
//!
//! (`GraphicsMeshInstance::UpdateSectionOpacities` @0x1800c0ae0,
//! called from `Actor::UpdateSelf` / `FastUpdate` with `1 − maxTransparency`):
//! per section, marked → `opacity = clamp(opacity − dt_ms·0.004, min, 0.75)`
//! (an instant drop to 0.75, then 4 / s down to `min`); unmarked → `opacity +=
//! dt_ms·0.004`, snapping to 1 once ≥ 0.75. Opacities exist per entry of the
//! mesh's section-name chunk (a mesh without one never fades); sections named
//! "FadeIn" draw `1 − opacity` (no stock mesh has one, not ported). The
//! shader gets `transparencyValue = (o⁸, o⁸, o, o⁸)` (per
//! `SetShaderParameters` @0x1800c2d70: the stores are `pow(o, 7.994)` ×2, o,
//! `pow` again).
//!
//! (`GraphicsMeshInstance::GetShader2` @0x1800c3ed0, `RenderPass`
//! @0x1800c42b0): a section at opacity < 1 draws with its material's "…T.ssh"
//! shader and only in passes of priority ≥ 10000 or the `shadow` /
//! `pLightShadowStyle` / `depthStyle` / `transparencyMaskStyle` /
//! `reflectionStyle` passes; at 0 only in the shadow ones.
//!
//! Where those draw: `GraphicsDeferredRenderer::ExecutePrepareScene`
//! @0x1800adbc0 takes `deferred` passes of priority 0..5999 only
//! (`ConstructRenderPass` @0x1800cdc00 drops the rest), and every T shader's
//! `deferred` technique is 10000, so a faded section never enters the
//! G-buffer. It is drawn by the transparency list: `RenderScene` →
//! `ExecutePrepareTransparency` @0x1800c8cc0 collects `depthStyle` and the
//! light styles of priority 10000..11000 (`CollectPassesForAllLights`
//! @0x1800ce670: the first light's `dLightDiffuseAmbientStyle`, then each
//! other light's style), sorts them far to near by the object's position
//! (sub_1800D72B0) and draws them after the deferred lighting
//! (`RenderScenePasses`). The DirectX11 programs (every "…t.ssh" alike,
//! per standardstatict.ssh): `depthStyle` (depthPS, value 35) writes
//! depth only, dropping texels with base alpha < 0.5;
//! `dLightDiffuseAmbientStyle` (lightDiffuseAmbientPS, value 17) lights
//! (hemisphere ambient + sun with its shadow) with alpha = `v` and no discard,
//! blending SrcAlpha / OneMinusSrcAlpha with DepthFunc EQUAL and no depth
//! write; each point light's `pLightDiffuseStyle` (value 14) adds at full
//! strength (One / BLEND_FACTOR) and discards `v < 0.5`. `v =
//! alphaMaskFactor·(transparencyValue.w − mask − .z) + .z` = o: the factor
//! (`GraphicsMTRenderer` +752, read only by `SetShaderParamsForScene`
//! @0x1800cf220) is set to 0 by the constructor and never written again, and
//! no pass collects `transparencyMaskStyle`, so the screen mask is dead. Per
//! pixel of the section's front-most surface with base alpha ≥ 0.5:
//! `o·(ambient + sun) + (1 − o)·behind`, plus the point lights while o ≥ 0.5.
//! So a marked part drops to 75 % and fades evenly to `1 − maxTransparency`
//! and stays there (0.5 → 50 %, 0.6 → 40 %, 0.75 → 25 %); only records with
//! `maxTransparency` 1 (roofs, many trees) end at 0 and disappear. The shadow
//! stays, whole: the T shaders' `lightShadowPS` (standardstatict.ssh
//! value 38) cuts `base.a · vcol.a < 0.5` only, without the opacity, so a
//! part at opacity 0 still casts its full shadow. A material without a T variant
//! (`*_nofade.ssh`; `model::transparent_variant_exists`) is always drawn: the
//! round pillars' base and broken stump stay, only the shaft goes.
//!
//! PORT: hit boxes ignore bones. The box–frustum test is the engine's two
//! stages (separating-plane reject sub_18027E2E0 + per-face clip sub_18027EC40,
//! `Obb::touches`). A faded part draws its two passes in Bevy's transparent
//! phase (after all opaque and alpha-masked meshes, far to near): a child
//! entity on the part's mesh with the [`FadePass::Depth`] clone, sorted just
//! before the part itself, which swaps to the [`FadePass::Colour`] clone
//! (`fade.x = o`; gd_standard.wesl) and keeps casting the shadow. The order is
//! per part (Bevy sorts by mesh centre), not per object as the engine's list.
//! The colour pass lights with the port's deferred formulas and tests the
//! depth pass's texels instead of EQUAL depth.
//!
//! PORT: the two fade pipeline variants are pre-warmed behind the loading cover ([`warm_fade_pipelines`]):
//! the engine's T shader variants exist up front (`GetShader2` @0x1800c3ed0), but a Bevy pipeline
//! compiles on first use, so the first fade of a material key would pop while it compiles. During `Loading`
//! one invisible dummy draw per (fade states, wind) key of the dressed scenery queues the pipelines; the
//! dummies go when play starts. A streamed area's scenery spawns far outside the transparency frustum, so
//! its variants compile long before the player reaches them.
use crate::camera::GameCamera;
use crate::render::material::{FadePass, GdMaterial, GdParams, PassStates};
use bevy::camera::visibility::DynamicSkinnedMeshBounds;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::mesh::MeshTag;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use gd_formats::msh::Mesh as Msh;
use std::collections::HashSet;

/// `GameEngine` transparency-frustum defaults: near width / height, far width
/// / height, eye height offset.
const FRUSTUM: [f32; 5] = [1.0, 1.0, 20.0, 25.0, 5.0];
/// Opacity change per millisecond (`UpdateSectionOpacities`).
const RATE: f32 = 0.004;
/// Opacity a marked section starts from / an unmarked one snaps to 1 from.
const KNEE: f32 = 0.75;
/// The T lit pass's blend: SrcAlpha / OneMinusSrcAlpha (module doc).
const T_BLEND: (u8, u8) = (6, 8);

/// The states of a faded part's pass: the material's cull, the T pass's blend
/// for the lit one (module doc).
fn fade_states(s: PassStates, pass: FadePass) -> PassStates {
    PassStates {
        blend: (pass == FadePass::Colour).then_some(T_BLEND),
        fade: Some(pass),
        ..s
    }
}

pub struct SeeThroughPlugin;

impl Plugin for SeeThroughPlugin {
    fn build(&self, app: &mut App) {
        // Test hook: `GD_NO_SEE_THROUGH` keeps all scenery opaque (before / after shots).
        if !app
            .world()
            .resource::<crate::hooks::Hooks>()
            .render
            .no_see_through
        {
            app.init_resource::<FadeWarm>().add_systems(
                Update,
                (
                    fade_scenery.run_if(not(resource_exists::<
                        crate::render::backdrop::MenuBackdrop,
                    >)),
                    warm_fade_pipelines
                        .run_if(resource_exists::<gd_game::session::SessionReady>)
                        .run_if(not(gd_game::session::in_game)),
                    clear_fade_warm.run_if(gd_game::session::in_game),
                ),
            );
        }
    }
}

/// The pre-warm dummies and the fade pipeline keys already queued (module doc).
#[derive(Resource, Default)]
struct FadeWarm {
    dummies: Vec<Entity>,
    /// (the fade pass's states, wind) — the material's bind-group key; the dummy carries the part's own
    /// mesh, so the vertex-layout half of the pipeline key matches too.
    keys: HashSet<(PassStates, bool)>,
}

/// Behind the loading cover: queue every dressed scenery part's two fade pipeline variants once, as invisible
/// dummy draws (the colour clone at opacity 0, the depth clone writes no colour; both sit beyond the far plane).
/// The first real fade then finds its pipelines compiled — no pop (module doc).
fn warm_fade_pipelines(
    mut commands: Commands,
    mut warm: ResMut<FadeWarm>,
    mut materials: ResMut<Assets<GdMaterial>>,
    objects: Query<&SeeThrough>,
    parts: Query<(&Mesh3d, &MeshMaterial3d<GdMaterial>, Option<&SkinnedMesh>)>,
    mut fresh: gd_game::session::NewSession,
) {
    if fresh.begun() {
        // A new session may mount another mod (its shaders rebuild the pipeline cache).
        warm.keys.clear();
    }
    for st in &objects {
        for part in &st.parts {
            if part.section.is_none() {
                continue;
            }
            let Ok((mesh, mat, skinned)) = parts.get(part.entity) else {
                continue;
            };
            let Some(m) = materials.get(&mat.0).cloned() else {
                continue;
            };
            let wind = m.params.wind.y != 0.0;
            for pass in [FadePass::Depth, FadePass::Colour] {
                let states = fade_states(m.states, pass);
                if !warm.keys.insert((states, wind)) {
                    continue;
                }
                let mut dummy = commands.spawn((
                    Mesh3d(mesh.0.clone()),
                    MeshMaterial3d(materials.add(GdMaterial {
                        states,
                        params: GdParams {
                            fade: Vec4::new(0.0, m.params.fade.y, m.params.fade.z, m.params.fade.w),
                            ..m.params
                        },
                        ..m.clone()
                    })),
                    Transform::from_xyz(0.0, -1000.0, 0.0),
                    Visibility::Visible,
                    NoFrustumCulling,
                    NotShadowCaster,
                ));
                if let Some(s) = skinned {
                    dummy.insert((s.clone(), DynamicSkinnedMeshBounds));
                }
                warm.dummies.push(dummy.id());
            }
        }
    }
}

/// Play started: the cover is gone, the dummies go (their pipelines stay cached).
fn clear_fade_warm(mut commands: Commands, mut warm: ResMut<FadeWarm>) {
    for e in warm.dummies.drain(..) {
        commands.entity(e).try_despawn();
    }
}

/// A placed box (world centre, half-extent axes).
#[derive(Clone, Copy, Debug)]
struct Obb {
    centre: Vec3,
    axes: [Vec3; 3],
}

impl Obb {
    /// The engine's box–frustum test (`GraphicsMeshInstance::SetTransparency`
    /// @0x1800c0470): a separating-plane reject (sub_18027E2E0) AND an exact
    /// confirm (sub_18027EC40 → sub_18027BD90) that clips each box face against
    /// every frustum plane. The plane test alone also accepts a box that sits
    /// beside the frustum but inside the infinite slab of one of its planes;
    /// the face clip keeps only boxes that really reach into the frustum, so a
    /// house's far walls are not faded just because the player stands near it.
    fn touches(&self, planes: &[Vec4]) -> bool {
        let sep = planes.iter().all(|p| {
            let n = p.truncate();
            let r: f32 = self.axes.iter().map(|a| n.dot(*a).abs()).sum();
            n.dot(self.centre) + p.w >= -r
        });
        if !sep {
            return false;
        }
        let [a, b, c] = self.axes;
        let faces = [
            (a, b, c),
            (-a, b, c),
            (b, a, c),
            (-b, a, c),
            (c, a, b),
            (-c, a, b),
        ];
        let mut poly = [Vec3::ZERO; 16];
        for (f, u, v) in faces {
            poly[0] = self.centre + f + u + v;
            poly[1] = self.centre + f - u + v;
            poly[2] = self.centre + f - u - v;
            poly[3] = self.centre + f + u - v;
            let mut len = 4usize;
            for p in planes {
                clip_plane(&mut poly, &mut len, p.truncate(), p.w);
                if len == 0 {
                    break;
                }
            }
            if len > 0 {
                return true;
            }
        }
        false
    }
}

/// Sutherland–Hodgman clip of `poly[..len]` against `n·p + d ≥ 0`, in place.
fn clip_plane(poly: &mut [Vec3; 16], len: &mut usize, n: Vec3, d: f32) {
    let mut out = [Vec3::ZERO; 16];
    let mut ol = 0;
    let l = *len;
    for i in 0..l {
        let p = poly[i];
        let q = poly[(i + 1) % l];
        let dp = n.dot(p) + d;
        let dq = n.dot(q) + d;
        if dp >= 0.0 {
            out[ol] = p;
            ol += 1;
        }
        if (dp >= 0.0) != (dq >= 0.0) {
            out[ol] = p.lerp(q, dp / (dp - dq));
            ol += 1;
        }
    }
    *poly = out;
    *len = ol;
}

#[derive(Clone, Copy)]
struct Part {
    entity: Entity,
    /// Section index (`None`: outside the section list or a material without
    /// a transparent variant, never fades).
    section: Option<usize>,
    /// Opacity last applied to the entity.
    shown: f32,
}

/// A part's faded draw, made on its first fade.
struct Faded {
    /// The part's own material, back at opacity 1.
    orig: Handle<GdMaterial>,
    /// The [`FadePass::Colour`] clone carrying the opacity.
    colour: Handle<GdMaterial>,
    /// The child drawing the [`FadePass::Depth`] clone.
    depth: Entity,
}

/// Fading state of one scenery object (on its root entity).
#[derive(Component)]
pub struct SeeThrough {
    /// `1 − maxTransparency`.
    min: f32,
    boxes: Vec<Obb>,
    centre: Vec3,
    radius: f32,
    /// Per mesh section: (opacity, can fade — not "NoTransparency").
    sections: Vec<(f32, bool)>,
    parts: Vec<Part>,
    faded: Vec<Option<Faded>>,
}

/// A record's `maxTransparency` (clamped to 0..1) if it has `allowTransparency`.
pub fn max_transparency(rec: &gd_formats::arz::Record) -> Option<f32> {
    rec.bool("allowTransparency")
        .then(|| rec.f32("maxTransparency").unwrap_or(0.0).clamp(0.0, 1.0))
}

impl SeeThrough {
    /// The mesh's hit boxes placed by `tf`, its sections, and the spawned part
    /// entities (`model::spawn_model*`) with their sections (`part_sections`).
    pub fn new(
        max_transparency: f32,
        msh: &Msh,
        tf: Affine3A,
        parts: &[Entity],
        part_sections: &[Option<u32>],
    ) -> Option<Self> {
        if msh.sections.is_empty() {
            return None;
        }
        let boxes: Vec<Obb> = msh
            .hit_boxes
            .iter()
            .filter(|h| !h.name.eq_ignore_ascii_case("Visibility"))
            .map(|h| {
                let c = &h.coords;
                let axis = |v: [f32; 3], e: f32| tf.transform_vector3(Vec3::from(v) * e);
                Obb {
                    centre: tf.transform_point3(Vec3::from(c.origin)),
                    axes: [
                        axis(c.x, h.extents[0]),
                        axis(c.y, h.extents[1]),
                        axis(c.z, h.extents[2]),
                    ],
                }
            })
            .collect();
        if boxes.is_empty() {
            return None;
        }
        let centre = boxes.iter().map(|b| b.centre).sum::<Vec3>() / boxes.len() as f32;
        let radius = boxes
            .iter()
            .map(|b| b.centre.distance(centre) + b.axes.iter().map(|a| a.length()).sum::<f32>())
            .fold(0.0, f32::max);
        let n = msh.sections.len();
        Some(Self {
            min: 1.0 - max_transparency,
            boxes,
            centre,
            radius,
            sections: msh
                .sections
                .iter()
                .map(|s| (1.0, !s.eq_ignore_ascii_case("NoTransparency")))
                .collect(),
            parts: parts
                .iter()
                .zip(part_sections)
                .map(|(e, s)| Part {
                    entity: *e,
                    section: s.map(|s| s as usize).filter(|s| *s < n),
                    shown: 1.0,
                })
                .collect(),
            faded: parts.iter().map(|_| None).collect(),
        })
    }
}

/// The transparency frustum planes (inside: `n·p + d ≥ 0`), from the camera
/// target and eye (module doc).
pub fn frustum(target: Vec3, eye: Vec3) -> [Vec4; 7] {
    let [near_w, near_h, far_w, far_h, lift] = FRUSTUM;
    let mut v = eye - target;
    v.y += lift;
    let dist = v.length();
    let f = v / dist.max(1e-6);
    let r = Vec3::new(f.z, 0.0, -f.x);
    let u = f.cross(r);
    let o = target;
    let far = o + f * dist;
    let plane = |n: Vec3, p: Vec3| n.extend(-n.dot(p));
    let side = |axis: Vec3, near: f32, far_s: f32, sign: f32, cross: &dyn Fn(Vec3) -> Vec3| {
        let p = o + axis * (sign * near * 0.5);
        let e = (far + axis * (sign * far_s * 0.5) - p).normalize_or_zero();
        plane(cross(e), p)
    };
    [
        plane(f, o),
        plane(-f, far),
        side(r, near_w, far_w, 1.0, &|e| e.cross(u)),
        side(r, near_w, far_w, -1.0, &|e| u.cross(e)),
        side(u, near_h, far_h, 1.0, &|e| r.cross(e)),
        side(u, near_h, far_h, -1.0, &|e| e.cross(r)),
        Vec4::new(0.0, 1.0, 0.0, -(target.y + 1.0)),
    ]
}

/// One `UpdateSectionOpacities` step.
pub fn step_opacity(o: f32, marked: bool, min: f32, dt_ms: f32) -> f32 {
    if marked {
        (o - dt_ms * RATE).max(min).min(KNEE)
    } else {
        let o = o + dt_ms * RATE;
        if o >= KNEE { 1.0 } else { o }
    }
}

fn fade_scenery(
    mut commands: Commands,
    time: Res<Time>,
    cams: Query<&GameCamera>,
    mut objects: Query<&mut SeeThrough>,
    mut parts: Query<(
        &mut MeshMaterial3d<GdMaterial>,
        &Mesh3d,
        Option<&SkinnedMesh>,
        Option<&MeshTag>,
    )>,
    mut visibility: Query<&mut Visibility>,
    mut materials: ResMut<Assets<GdMaterial>>,
) {
    let Ok(cam) = cams.single() else { return };
    let (target, eye) = (cam.target, cam.eye());
    let planes = frustum(target, eye);
    let reach = (eye - target).length() + FRUSTUM[4] + 1.0;
    let dt = time.delta_secs() * 1000.0;
    for mut st in &mut objects {
        let near = st.centre.distance(target) <= reach + st.radius;
        if !near
            && st.sections.iter().all(|s| s.0 >= 1.0)
            && st.parts.iter().all(|p| p.shown >= 1.0)
        {
            continue;
        }
        let marked = near && st.boxes.iter().any(|b| b.touches(&planes));
        let min = st.min;
        for s in &mut st.sections {
            s.0 = step_opacity(s.0, marked && s.1, min, dt);
        }
        let st = &mut *st;
        for (i, part) in st.parts.iter_mut().enumerate() {
            let o = part.section.map_or(1.0, |s| st.sections[s].0);
            if o == part.shown {
                continue;
            }
            let Ok((mut mat, mesh, skinned, tag)) = parts.get_mut(part.entity) else {
                continue;
            };
            // The depth pass draws while 0 < o < 1 (at 0 the engine draws only the shadow).
            let (depth_shown, material) = if o >= 1.0 {
                let Some(f) = &st.faded[i] else {
                    part.shown = o;
                    continue;
                };
                (false, f.orig.clone())
            } else {
                if st.faded[i].is_none() {
                    let orig = mat.0.clone();
                    let Some(m) = materials.get(&orig).cloned() else {
                        continue;
                    };
                    let depth = GdMaterial {
                        states: fade_states(m.states, FadePass::Depth),
                        ..m.clone()
                    };
                    let colour = GdMaterial {
                        states: fade_states(m.states, FadePass::Colour),
                        ..m
                    };
                    let mut child = commands.spawn((
                        Mesh3d(mesh.0.clone()),
                        MeshMaterial3d(materials.add(depth)),
                        Transform::IDENTITY,
                        Visibility::Inherited,
                        NotShadowCaster,
                        ChildOf(part.entity),
                    ));
                    if let Some(s) = skinned {
                        child.insert((s.clone(), DynamicSkinnedMeshBounds));
                    }
                    if let Some(t) = tag {
                        child.insert(t.clone());
                    }
                    st.faded[i] = Some(Faded {
                        orig,
                        colour: materials.add(colour),
                        depth: child.id(),
                    });
                }
                let Some(f) = &st.faded[i] else {
                    continue;
                };
                if let Some(mut m) = materials.get_mut(&f.colour) {
                    m.params.fade.x = o;
                }
                (o > 0.0, f.colour.clone())
            };
            if mat.0 != material {
                mat.0 = material;
            }
            if let Some(f) = &st.faded[i]
                && let Ok(mut v) = visibility.get_mut(f.depth)
            {
                let want = if depth_shown {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                v.set_if_neq(want);
            }
            part.shown = o;
        }
    }
}
