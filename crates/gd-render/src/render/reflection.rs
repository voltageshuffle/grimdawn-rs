//! The water's planar scene reflection (`waterbasic.ssh` `reflectPS` + every
//! mesh's `reflectionStyle`), the `reflectionQuality` option.
//!
//! Engine.dll:
//! - `Water::AddToScene` gives a layer / path a reflection
//!   plane only when its `surfaceParams.z` (the lighting blend, `p[7]`) > 0.
//! - `GraphicsMTRenderer::BuildReflections`: `reflectionQuality`
//!   (quality[3]) 0 = off. The reflection scene is rendered by a camera mirrored
//!   across the water plane: `GraphicsMTRenderer::ReflectViewMatrix` reflects the view matrix across the
//!   plane (per column `c' = c − 2n(n·c)`, the translation column `+ d`), then
//!   negates one basis column (the handedness flip, so the mirrored image samples
//!   correctly). The reflection is clipped at the water plane and the casters are
//!   cut at 64 m (ultra: the far plane) by `Camera::GetMinFrustum`.
//! - The reflection target is a FIXED 512×512 (`GraphicsMTRenderer::ReflectionViewport` viewport,
//!   `RenderReflection` creates the surface at the viewport size).
//!   The quality does NOT size it: it sets the reflection scene's content flags
//!   (`GraphicsScene.contentFlags`: medium 0x408, high 0x40A, very high 0x60A, ultra 0x61A; the main
//!   scene is 0x7FF) which select what reflects, and the caster distance.
//! - `reflectionStyle` (every mesh): `base × (sat(N·L)·sun + hemisphere ambient)`,
//!   alpha `base.a`, clipped at the water plane — no point lights, shadows,
//!   specular or glow. `reflectPS` then samples the target at the water pixel's
//!   screen uv (+ the bump distortion) and blends it over the sky reflection
//!   (`water.rs` / `shaders/water.wesl`).
//!
//! PORT: Bevy draws the reflection with a second [`ReflectionCamera`] (the glow
//! camera's convention, `render/post.rs`): the main camera mirrored across the
//! water plane (`bevy::math::reflection_matrix`, [`reflection_camera`]),
//! `Camera::invert_culling` for the flipped winding, and the water plane as the
//! projection's oblique `near_clip_plane` (Bevy's Lengyel clip). The scene
//! materials recognise the view by its exposure ([`REFLECTION_EV100`]) and output
//! `reflectionStyle` (`gd_standard.wesl` / `gd_terrain.wesl`); the water itself
//! draws nothing there. The target is the engine's fixed 512 wide
//! ([`reflection_target_size`]), kept at the screen's aspect so the reflection
//! lines up with the screen uv the water samples; the caster distance is the
//! engine's 64 m / far ([`reflection_caster_distance`]). The content flags select
//! which renderables reflect per-renderable in the engine; PORT: the port reflects
//! the whole scene (terrain + meshes) at every enabled quality. One pass for the
//! nearest reflecting plane (the engine: one per plane).
//!
//! PORT (performance): the pass runs only while a reflecting surface has a block
//! in the main camera's frustum (last frame's, so the reflection starts one frame
//! after the water enters the screen edge), and the view renders no sun shadow
//! map (`shadow::NoSunShadow`: `reflectionStyle` has no shadows).
use super::shadow::{NoSunShadow, SceneBlocks};
use crate::camera::GameCamera;
use crate::water::WaterPoints;
use bevy::camera::primitives::{Aabb, Frustum};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Exposure, RenderTarget};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::math::reflection_matrix;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};

/// The reflection view's exposure marker (ev100 −2 → exposure 2^2 / 1.2 ≈ 3.33).
/// The scene shaders test `view.exposure > 2.0` (`is_reflection_view`); the glow
/// view is 0.83 and the main camera ≈ 0, so the three never collide.
pub const REFLECTION_EV100: f32 = -2.0;
/// The engine's reflection target width (`GraphicsMTRenderer::ReflectionViewport` viewport), fixed for
/// every enabled quality (module doc).
pub const REFLECTION_SIZE: u32 = 512;
/// The engine's reflection caster distance (`BuildReflections` v8): 64 m below
/// ultra, the scene far plane at ultra.
pub const REFLECTION_DISTANCE: f32 = 64.0;

pub struct ReflectionPlugin;

impl Plugin for ReflectionPlugin {
    fn build(&self, app: &mut App) {
        // Before the transform propagation, after the camera moves: Bevy builds the
        // sun's shadow cascades (after propagation) only for active views, so a camera
        // switched on after them reaches the renderer with none (a panic in
        // `prepare_lights`), and its mirrored transform would lag a frame.
        app.add_systems(PostStartup, setup_reflection).add_systems(
            PostUpdate,
            sync_reflection
                .after(crate::camera::CameraSet)
                .before(bevy::transform::TransformSystems::Propagate),
        );
    }
}

/// The reflection camera (mirrors the [`GameCamera`] across the water plane).
#[derive(Component)]
pub struct ReflectionCamera;

/// The reflection render target, shared with the water material (`water.rs`).
/// Kept across sessions like the other render targets (`exit.rs` KEEP).
#[derive(Resource)]
pub struct ReflectionTarget {
    pub image: Handle<Image>,
}

/// The reflection target's width for a `reflectionQuality`: the engine's fixed
/// 512 for every enabled quality, 0 = off (module doc — the quality selects the
/// reflected content + caster distance, not the resolution).
pub fn reflection_target_size(quality: u32) -> u32 {
    if quality == 0 { 0 } else { REFLECTION_SIZE }
}

/// The reflection caster distance of a quality: 64 m below ultra, the main
/// camera's far plane at ultra (`BuildReflections` v8 → `GetMinFrustum`).
pub fn reflection_caster_distance(quality: u32, far: f32) -> f32 {
    if quality >= 4 {
        far
    } else {
        REFLECTION_DISTANCE
    }
}

/// The reflection quality of the session options (0 = off).
pub fn option_quality(options: &gd_game_core::options::GameOptions) -> u32 {
    match options.get("reflectionQuality") {
        Some(gd_formats::options::Value::Quality(q)) => super::shadow::quality_index(q),
        _ => 1,
    }
}

/// The mirrored camera for a water plane at `plane_height`: the main camera
/// reflected across `y = plane_height` (the engine's `GraphicsMTRenderer::ReflectViewMatrix` reflection +
/// handedness flip, which Bevy expresses as the reflection matrix +
/// `Camera::invert_culling`), and the water plane as the oblique near clip plane
/// so nothing below the surface reflects. The reflection matrix /
/// clip follow `BuildReflections` (module doc).
pub fn reflection_camera(
    main: &Transform,
    proj: &PerspectiveProjection,
    plane_height: f32,
) -> (Transform, PerspectiveProjection) {
    // Householder reflection across y = h: linear part I − 2·Y·Yᵀ, translation 2h·Y.
    let mut refl = Mat4::from_mat3a(reflection_matrix(Vec3::Y));
    refl.w_axis.y = 2.0 * plane_height;
    let mirror = Transform::from_matrix(refl * main.to_matrix());
    // The water plane in the mirror camera's view space: normal away from the
    // camera (toward the above-water geometry it keeps), w = −(n·q) for a point q
    // on the plane (Bevy `PerspectiveProjection::near_clip_plane`).
    let view = mirror.compute_affine().inverse();
    let n = (view.matrix3 * Vec3::Y).normalize();
    let q = view.transform_point3(Vec3::new(0.0, plane_height, 0.0));
    let mirror_proj = PerspectiveProjection {
        near_clip_plane: n.extend(-n.dot(q)),
        ..*proj
    };
    (mirror, mirror_proj)
}

fn setup_reflection(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    options: Res<gd_game_core::options::GameOptions>,
) {
    let size = reflection_target_size(option_quality(&options)).max(1);
    let image = images.add(Image::new_target_texture(
        size,
        size,
        TextureFormat::Rgba8Unorm,
        None,
    ));
    commands.insert_resource(ReflectionTarget {
        image: image.clone(),
    });
    commands.spawn((
        ReflectionCamera,
        NoSunShadow,
        Camera3d::default(),
        Camera {
            order: -1,
            // The reflection flips every polygon's winding (module doc).
            invert_culling: true,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            is_active: false,
            ..default()
        },
        RenderTarget::Image(image.into()),
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
        Exposure {
            ev100: REFLECTION_EV100,
        },
        RenderLayers::layer(0),
        Transform::default(),
        Projection::Perspective(crate::camera::world_projection()),
        Name::new("water reflection camera"),
    ));
}

/// Follows the main camera across the nearest reflecting water plane, sizes the
/// target by the option, and turns the pass off when there is nothing to reflect.
fn sync_reflection(
    options: Res<gd_game_core::options::GameOptions>,
    mains: Query<
        (&Camera, &Transform, &Projection, &Frustum),
        (With<GameCamera>, Without<ReflectionCamera>),
    >,
    water: Query<(
        &WaterPoints,
        &SceneBlocks,
        &GlobalTransform,
        &InheritedVisibility,
    )>,
    target: Res<ReflectionTarget>,
    mut images: ResMut<Assets<Image>>,
    mut refl: Query<(&mut Camera, &mut Transform, &mut Projection), With<ReflectionCamera>>,
) {
    let quality = option_quality(&options);
    let main = mains.single().ok();
    let plane = main.and_then(|(.., frustum)| {
        water
            .iter()
            .filter(|(w, _, _, vis)| vis.get() && w.reflection_blend() > 0.0)
            .find(|(_, blocks, gt, _)| {
                let affine = gt.affine();
                blocks.0.iter().any(|(lo, hi)| {
                    frustum.intersects_obb(&Aabb::from_min_max(*lo, *hi), &affine, true, true)
                })
            })
            .map(|(w, ..)| w.height())
    });
    let setup = (|| {
        if quality == 0 {
            return None;
        }
        let height = plane?;
        let (cam, tf, proj, _) = main?;
        let Projection::Perspective(persp) = proj else {
            return None;
        };
        let size = cam.physical_target_size()?;
        // The game camera is a root entity: its `Transform` is its world transform.
        Some((*tf, persp, size, height))
    })();
    let Some((main_tf, persp, size, height)) = setup else {
        for (mut c, ..) in &mut refl {
            c.is_active = false;
        }
        return;
    };
    // Keep the screen's aspect so the reflection lines up with the screen uv the
    // water samples (the engine's square target is stretched the same way).
    let width = reflection_target_size(quality).max(1);
    let height_px = ((width as f32 * size.y as f32 / size.x as f32).round() as u32).max(1);
    if images
        .get(&target.image)
        .is_some_and(|i| i.size() != UVec2::new(width, height_px))
        && let Some(mut img) = images.get_mut(&target.image)
    {
        img.resize(Extent3d {
            width,
            height: height_px,
            depth_or_array_layers: 1,
        });
    }
    let (mirror, mut mirror_proj) = reflection_camera(&main_tf, persp, height);
    // The engine cuts the reflection casters at 64 m (ultra: the far plane).
    mirror_proj.far = reflection_caster_distance(quality, persp.far);
    for (mut c, mut t, mut p) in &mut refl {
        c.is_active = true;
        *t = mirror;
        *p = Projection::Perspective(mirror_proj.clone());
    }
}
