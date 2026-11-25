//! Grim Dawn's look on Bevy: the game's own surface/terrain shading in gamma
//! space, its lighting environment, and the deferred renderer's glow.
//!
//! Pipeline:
//! - Textures are sampled as UNORM (no sRGB decode) and every shader computes
//!   the D3D11 deferred renderer's result directly in gamma space
//!   (ambient + sun + point lights + glow, clamped to [0,1] like the game's
//!   UNORM targets).
//! - The camera renders into an HDR (float) target with tonemapping off, so
//!   blending happens on the raw gamma values exactly as on the game's
//!   back buffer. The final passes ([`post::GammaOut`]) run the engine's
//!   post chain (bloom, enhancement), the UI blends over the gamma values,
//!   then the frame is converted gamma → linear, which the sRGB swapchain
//!   turns back into the original gamma value.
//! - Every view draws directly, the shadow views included
//!   ([`no_indirect_drawing`]): Bevy's indirect path rebuilds its preprocessing
//!   bind groups and instance buffers per view and phase each frame, about half
//!   the render schedule in a busy scene, and the scene is CPU-bound there, not
//!   on draw calls.
pub mod backdrop;
pub mod distort;
pub mod env;
pub mod envmap;
pub mod gbuffer_blend;
pub mod grass;
pub mod light_rig;
pub mod lights;
pub mod material;
pub mod point_shadow;
pub mod post;
pub mod reflection;
pub mod shadow;
pub mod terrain;
pub mod weather;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use bevy::render::storage::ShaderBuffer;

pub use material::GdMaterial;

pub struct GdRenderPlugin;

impl Plugin for GdRenderPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/gd_standard.wesl");
        embedded_asset!(app, "shaders/gd_prepass.wesl");
        embedded_asset!(app, "shaders/gd_forward.wesl");
        embedded_asset!(app, "shaders/gd_terrain.wesl");
        embedded_asset!(app, "shaders/gd_post.wesl");
        embedded_asset!(app, "shaders/gd_glow.wesl");
        embedded_asset!(app, "shaders/gd_distort.wesl");
        embedded_asset!(app, "shaders/gd_envmap.wesl");
        bevy::asset::load_internal_asset!(
            app,
            material::WIND_SHADER,
            "shaders/gd_wind.wesl",
            Shader::from_wesl
        );
        app.add_plugins((
            MaterialPlugin::<GdMaterial>::default(),
            MaterialPlugin::<terrain::GdTerrainMaterial>::default(),
            post::PostPlugin,
            post::GlowPlugin,
            env::EnvPlugin,
            light_rig::LightRigPlugin,
            shadow::SunShadowPlugin,
            point_shadow::PointShadowPlugin,
            distort::DistortPlugin,
            gbuffer_blend::GBufferBlendPlugin,
            reflection::ReflectionPlugin,
            MaterialPlugin::<envmap::EnvmapMaterial>::default(),
        ))
        .init_resource::<material::MaterialCache>()
        .init_resource::<grass::GrassQuality>()
        .add_systems(PreStartup, setup_defaults)
        .add_systems(First, material::evict_variants)
        .add_observer(grass::release_field)
        .add_systems(
            PostUpdate,
            (
                grass::grass_wind,
                grass::character_disturbances
                    .run_if(resource_exists::<gd_game_core::data::GameData>)
                    .before(grass::grass_offsets),
                grass::line_disturbances.before(grass::grass_offsets),
                grass::grass_offsets,
                terrain::sync_terrain_quality
                    .run_if(resource_exists::<gd_game_core::options::GameOptions>),
            ),
        );
        // The grass offset fields go to the GPU as writes into their shared buffer (grass.rs `GrassOffsets`).
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app
                .init_resource::<grass::ExtractedGrassWrites>()
                .add_systems(
                    bevy::render::ExtractSchedule,
                    (
                        grass::extract_offsets,
                        // Before the cameras read the mode: a first frame extracted in culling mode leaves the
                        // views' retained work-item buffers indirect while the next frame's `NoIndirectDrawing`
                        // selects the direct preprocess pipeline (wgpu bind group mismatch, quits at startup).
                        no_indirect_drawing.before(bevy::render::camera::extract_cameras),
                    ),
                )
                .add_systems(
                    bevy::render::Render,
                    grass::write_offsets.in_set(bevy::render::RenderSystems::PrepareResources),
                );
        }
    }
}

/// Caps Bevy's GPU preprocessing below indirect drawing / GPU culling for every
/// view (module doc). A per-camera `NoIndirectDrawing` does not reach the point
/// lights' shadow views, which follow this device-wide mode.
fn no_indirect_drawing(
    support: Option<ResMut<bevy::render::batching::gpu_preprocessing::GpuPreprocessingSupport>>,
) {
    use bevy::render::batching::gpu_preprocessing::GpuPreprocessingMode;
    if let Some(mut s) = support
        && s.max_supported_mode == GpuPreprocessingMode::Culling
    {
        s.max_supported_mode = GpuPreprocessingMode::PreprocessingOnly;
    }
}

/// Whether a render-world copy of the main-world entities marked `T` (holding `len` entries) is stale: some `T`
/// was added or changed, or the count differs (a removal). Lets the extract systems skip their rebuild.
pub(crate) fn marked_changed<T: Component>(len: usize, parts: &Query<(Entity, Ref<T>)>) -> bool {
    let mut n = 0;
    for (_, t) in parts {
        if t.is_changed() {
            return true;
        }
        n += 1;
    }
    n != len
}

/// Shared GPU resources: fallback textures and the environment buffer every
/// Grim Dawn material reads (sun, ambient, point lights).
#[derive(Resource, Clone)]
pub struct GdDefaults {
    pub white: Handle<Image>,
    pub black: Handle<Image>,
    /// Flat tangent-space normal with zero specular mask (alpha 0).
    pub flat_normal: Handle<Image>,
    /// Black cube map (a missing `envmapTexture`).
    pub black_cube: Handle<Image>,
    /// 1×1 depth 0 (reverse Z: infinitely far): the terrain's `depthTexture` below quality 3.
    pub depth: Handle<Image>,
    pub env: Handle<ShaderBuffer>,
    /// The grass offset fields' buffer ([`grass::GrassOffsets`]).
    pub grass: Handle<ShaderBuffer>,
}

fn solid(images: &mut Assets<Image>, c: [u8; 4]) -> Handle<Image> {
    images.add(Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &c,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

fn setup_defaults(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let env = buffers.add(ShaderBuffer::from(env::EnvGpu::default().to_vec4s()));
    let grass = grass::GrassOffsets::new(&mut buffers);
    let d = GdDefaults {
        white: solid(&mut images, [255, 255, 255, 255]),
        black: solid(&mut images, [0, 0, 0, 255]),
        flat_normal: solid(&mut images, [128, 128, 255, 0]),
        black_cube: {
            let mut img = Image::new_fill(
                Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 6,
                },
                TextureDimension::D2,
                &[0, 0, 0, 255],
                TextureFormat::Rgba8Unorm,
                RenderAssetUsages::RENDER_WORLD,
            );
            img.texture_view_descriptor = Some(TextureViewDescriptor {
                dimension: Some(TextureViewDimension::Cube),
                ..default()
            });
            images.add(img)
        },
        // Not uploaded (depth formats take no writes): wgpu zero-initialises it.
        depth: images.add(Image::new_uninit(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Depth32Float,
            RenderAssetUsages::RENDER_WORLD,
        )),
        env,
        grass: grass.buffer.clone(),
    };
    commands.insert_resource(d);
    commands.insert_resource(grass);
}
