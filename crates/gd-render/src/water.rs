//! Water layers (level chunk 9) rendered with a port of `waterbasic.ssh` (lava types: `lavabasic.ssh`, below).
//!
//! Geometry (`WaterLayer::Load`, block VB builder `WaterLayer::BuildBlockVertices`):
//! a layer is a grid of 9-unit blocks, `(terrain width - 1 + 8) / 9` per row
//! (Water::Initialize); each stored block is a 10×10 vertex grid at 1-unit
//! spacing clamped to the block bounds, at the layer height.
//! The block's 100 bytes are the per-vertex alpha. Depth opacity per vertex is
//! `lerp(p0, p1, sat((depth − p2) / max(p3 − p2, 1)))` (VS), where depth is
//! water height − terrain height. Parameters (`WaterType::SetShaderParams`):
//! surfaceParams = p[5..8], waterColor = p[11..14],
//! textureScale = 1/(p9, p10), textureDirection = angle p8°, textureScroll =
//! p4/p9, depthParams = (p2, p3, p0, p1).
//!
//! Shading (`waterbasic.ssh` `basePS`, its DX11 bytecode): the bump is the water type's
//! NoiseTexture record ([`wt_noise`]), NOT a static texture. The engine generates 256 32×32 value-noise
//! textures (Engine.dll `NoiseTexture::GenerateNoise` / `NoiseTexture::GenerateNoiseSlice`: lattice 31×31, hash
//! `(n·(15731·n² + 789221) − 771171059) & 0x7FFFFFFF`, tent weights 31.75 / 15.875 / 7.9375, byte = trunc(Σ + 127)),
//! crossfades each of 4 octaves between two of them at the record's `rates` (`NoiseTexture::UpdateLayers`),
//! composites them at UV scales (1, 2, 4, 8) weighted by `levels` (`noisetexturegen.ssh` `compositeBaseStyle`, Engine.dll
//! `Engine::NoiseOctaveUvScales`) into a `size`² target, and converts the height to a normal map with the ×8 gradient
//! at ±1/size (`noisetexturegen.ssh` `convertStyle`), which the water samples bilinearly (repeat). The port does the
//! same: the bank is one 256-layer 32×32 array ([`noise_bank`]), the octave state lives in [`WaterMaterials`] and
//! steps in [`update_water_noise`], and [`render_noise`] draws each type's composite + convert into its `size`²
//! target (`shaders/water_noise.wesl`) before the cameras render; every material of the type binds that target
//! (binding 1). `normalMap = 0` (lava records) writes the raw height, which `basePS` reads as the bump. PORT: one
//! pass evaluates the three heights each texel's convert needs (the engine composites, then converts in a second
//! pass); the target is `Rg16Float` (the engine's is 8-bit); a type with no water entity visible in any view skips
//! its pass that frame (its target keeps the last image).
//! The sun / sky ambient are the scene constants every material reads ([`crate::render::GdDefaults`]): `basePS`'s
//! `lightColor·lightDirection.y + skyAmbientColor` and the specular direction are the same per-frame cb1 values.
//!
//! Scene reflection (`waterbasic.ssh` `reflectPS`, [`crate::render::reflection`]): a mirrored
//! camera draws every mesh's `reflectionStyle` into a 512-wide target (the `reflectionQuality`
//! option gates it); the water samples it at the pixel's screen uv (+ the bump distortion) and
//! blends it over the sky reflection by `min(refl.a·1.5, 0.75)`, the `surfaceParams.z` lerp below
//! gating it to the reflecting types (`Water::AddReflectionPlanes`: `surfaceParams.z > 0`).
//!
//! Lava (water types whose shader is `lavabasic.ssh`, e.g. `lava_pool` / `lava_river_slow`), from its
//! DX11 `basePS` / `waterBlockVS` / `waterPathVS` bytecode, ported in `shaders/lava.wesl` (the same [`WaterMaterial`],
//! [`WaterKey`] swaps the fragment program; the type's `lavaTexture` / `lavaNormal` / `ashTexture` / `ashNormal`
//! shader params bound at 9-16). The VSs are waterbasic's but pass the scroll apart from the uv. `basePS`: the melt
//! `m = h · (surfaceParams.y − 10) / 495 · depth opacity` (h = the NoiseTexture height at uv + time · (0.0025,
//! 0.00125)) against the ash texture's alpha `a` picks ash (m ≤ a), lava (m > a + 0.1) or their blend by
//! k = 10 (m − a); the ash uv wobbles by 0.0006 · (sin, cos)(time / 8 + 30h) + scroll, the lava's by 0.003 ·
//! (sin, cos)(time / 2 + 30h) + 1.2 · scroll. Colour = albedo · lerp(1, sun · N·L + max(ground ambient, (0.5, 0.4,
//! 0.4)), w + 0.2), w = 1 on ash, 1 − k blending, 0 on lava (the lava is all but unlit), plus a red rim 0.35 ·
//! ((m / a)³ or 1 − k³, + 0.15 · sat(a − h) · a) on the crust; alpha = the painted vertex alpha only. No sky,
//! reflection, specular or glow style; `waterColor` and `surfaceParams.z` unused.
//!
//! Points on water ([`WaterPoints`], [`water_above`]): `Water::GetWaterPointInfo`, an upward ray
//! against every layer, the nearest hit wins; a layer holds a point when its block exists and the block's alpha byte at
//! the point is non-zero (`WaterLayer::IsPointInLayer`: cell `min(int(frac(x / 9) * 10), 9)` per axis).
//! A path (river) holds a point that lies in one of its segments (`WaterPath::RayIntersection`), so rain and gusts
//! ripple rivers too.
//!
//! Water paths (rivers), `WaterPath::Load` and the segment builder
//! `WaterPath::BuildSegment` ([`path_segment`]): each pair of neighbouring control points is a segment whose centre is
//! the Bézier (p0, p0 + t0, p1 − t1, p1). Its rows: n = min(int(L₂₀ / 4), 64) (L₂₀ = the centre's polyline length over
//! 20 steps, `WaterPath::CentreLength`; n = 0: no geometry), t = r / n for r = 0..=n. The two banks are Béziers too:
//! the ends at p ± width · side, the handles at the centre's handles ± width (lerped) · the centre's left normal over
//! the chords
//! t = 1/3 → 0.4333 and 2/3 → 0.7666. Each row has 10 vertices from the left bank to the right (s = k · 0.11111111),
//! at the path's height; vertex texture u = width(t) · s / p10, v = the along distance in `p9` repeats (each row adds
//! the mean bank step / L · max(int(L / p9), 1), L the length over n steps); the flow tangent (bump frame) is
//! (tz, −tx) lerped between the banks' step directions (row 0: the handle t0). Vertex alpha 255, but 0 / 127 on the
//! first two rows of the path's first segment and the last two of its last (the river fades in and out); the depth
//! opacity as for layers. Drawn with the water type's program, technique `waterPathStyle` (sort 4000 like the layers;
//! `waterbasic.ssh` `waterPathVS`): uv = (u + textureOffset.y, v + textureOffset.x + textureScroll · time), with
//! textureOffset = frac(region offset / (p9, p10)) (`SetShaderParams`), and the bump frame from the vertex tangent.
//! PORT: one mesh per path (the engine a vertex buffer per segment), triangles wound to face up (the engine's strip is
//! the same surface); `WaterPath::CalcSurfaceVelocity` (the flow at a point, `GetWaterPointInfo`'s velocity) has no
//! consumer in the port.
//!
//! Ripples (`Water::CreateRipple`, the set's rules in `gd_sim::weather::Ripples`): the weather adds them
//! (rain drops at `weatherQuality` > 1, strong directional wind), and so does every
//! `World::CreatePointDisturbance` ([`gd_game_core::visuals::PointDisturbance`]: moving characters, debris,
//! FX paks, floaters, waves), which calls `CreateRipple(pos, radius, strength, splash)` on the
//! water of each region whose box grown by the radius holds the point until one is hit. `CreateRipple`'s
//! splash: when the point is on water, type 1 (the player walking; only when the point lies under the surface), 2
//! (debris landing on water) or 3 starts the water type's matching splash effect ([`splash_effects`]) at the water
//! point, whether or not a ripple was added. [`update_ripples`] places, grows and draws the set. Drawn only
//! with `detailObjects` (`Water::AddToScene`) by `WaterRipple.ssh` (`shaders/ripple.wesl`): one
//! quad per live ripple, texture `WaterWalkRipple_01.tex`, SrcAlpha / InvSrcAlpha, no depth write, sort 4020 (before
//! the effect particles, after the water). PORT: the engine keeps one set per region's water; the port one for all.
use crate::render::GdDefaults;
use crate::textures::Textures;
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::core_pipeline::FullscreenShader;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    sampler, storage_buffer_read_only_sized, texture_2d_array,
};
use bevy::render::render_resource::{
    AsBindGroup, BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
    BlendComponent, BlendFactor, BlendOperation, BlendState, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, Extent3d, FragmentState, LoadOp, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, SamplerBindingType,
    ShaderStages, ShaderType, SpecializedMeshPipelineError, StoreOp, TextureDataOrder,
    TextureFormat, TextureSampleType, TextureView, TextureViewDescriptor, TextureViewDimension,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems};
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer};
use bevy::render::texture::GpuImage;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::ShaderRef;
use gd_formats::collections::HashMap;
use gd_formats::map::terrain::Terrain;
use gd_formats::map::water::{ControlPoint, Water, WaterPath, WaterType};
use gd_game_core::data::GameData;

const NOISE_SHADER: &str = "embedded://gd_render/shaders/water_noise.wesl";
/// The NoiseTexture targets' format ([`noise_target`]): the convert's bump (or the raw height) in rg.
const NOISE_FORMAT: TextureFormat = TextureFormat::Rg16Float;

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/water.wesl");
        embedded_asset!(app, "shaders/ripple.wesl");
        embedded_asset!(app, "shaders/water_noise.wesl");
        bevy::asset::load_internal_asset!(app, LAVA_SHADER, "shaders/lava.wesl", Shader::from_wesl);
        app.add_plugins((
            MaterialPlugin::<WaterMaterial>::default(),
            MaterialPlugin::<RippleMaterial>::default(),
        ))
        .init_resource::<WaterMaterials>()
        .add_systems(
            Update,
            (
                finish_water,
                update_water_noise.run_if(any_with_component::<WaterPoints>),
                update_ripples.after(crate::render::weather::WeatherSet),
            ),
        );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<NoisePasses>()
            .add_systems(RenderStartup, init_noise_pipeline)
            .add_systems(ExtractSchedule, extract_noise)
            .add_systems(
                Render,
                prepare_noise.in_set(RenderSystems::PrepareBindGroups),
            )
            // `Begin` runs before the cameras render, so every view samples this frame's target.
            .add_systems(RenderGraph, render_noise.in_set(RenderGraphSystems::Begin));
    }
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct WaterUniform {
    pub surface: Vec4,
    pub water_color: Vec4,
    pub tex_scale_dir: Vec4,
    /// x textureScroll, y 1 = a path, z unused, w 1 = normalMap.
    pub scroll_time: Vec4,
}

/// The `lavabasic.ssh` fragment program (`shaders/lava.wesl`), swapped in by [`WaterMaterial::specialize`].
const LAVA_SHADER: Handle<Shader> =
    bevy::asset::uuid_handle!("7e2dea42-052c-4fe8-901d-6180cf563207");

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(WaterKey)]
pub struct WaterMaterial {
    #[uniform(0)]
    pub params: WaterUniform,
    /// The type's NoiseTexture target ([`noise_target`], drawn every frame by [`render_noise`]; shared by the
    /// type's materials).
    #[texture(1)]
    #[sampler(2)]
    pub noise: Handle<Image>,
    #[texture(3, dimension = "cube")]
    #[sampler(4)]
    pub sky: Handle<Image>,
    /// The scene lighting every material reads (`render/env.rs` `EnvGpu`).
    #[storage(5, read_only)]
    pub env: Handle<ShaderBuffer>,
    /// The planar scene reflection (`render/reflection.rs`), sampled by reflectPS.
    #[texture(6)]
    #[sampler(7)]
    pub reflection: Handle<Image>,
    /// `lavabasic.ssh`'s `lavaTexture`, `lavaNormal`, `ashTexture`, `ashNormal` (the type's shader params; the
    /// default white image on water, whose program reads none of them).
    #[texture(9)]
    #[sampler(10)]
    pub lava: Handle<Image>,
    #[texture(11)]
    #[sampler(12)]
    pub lava_normal: Handle<Image>,
    #[texture(13)]
    #[sampler(14)]
    pub ash: Handle<Image>,
    #[texture(15)]
    #[sampler(16)]
    pub ash_normal: Handle<Image>,
    /// The type's shader is `lavabasic.ssh`: draws with [`LAVA_SHADER`].
    pub is_lava: bool,
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct WaterKey {
    lava: bool,
}

impl From<&WaterMaterial> for WaterKey {
    fn from(m: &WaterMaterial) -> Self {
        Self { lava: m.is_lava }
    }
}

impl Material for WaterMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://gd_render/shaders/water.wesl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.bind_group_data.lava
            && let Some(frag) = descriptor.fragment.as_mut()
        {
            frag.shader = LAVA_SHADER;
        }
        Ok(())
    }
}

/// CPU result for one water layer, spawned as a component by the world
/// streamer and turned into GPU assets by [`finish_water`].
#[derive(Component)]
pub struct WaterSurface {
    pub mesh: Option<Mesh>,
    /// The layer's blocks for the shadow scene bounds.
    pub blocks: crate::render::shadow::SceneBlocks,
    pub water_type: WaterType,
    /// A path (`waterPathStyle`: the vertex's texture coordinates and flow) rather than a layer.
    pub path: bool,
    pub points: WaterPoints,
}

/// On a water layer's or path's entity: what `WaterLayer::IsPointInLayer` / `WaterPath::RayIntersection` read
/// (world space).
#[derive(Component, Default)]
pub struct WaterPoints {
    /// World x / z of the region origin (the layer grid's corner).
    origin: Vec2,
    /// World height of the surface.
    height: f32,
    shape: WaterShape,
    /// The water type's splash effects (`CreateRipple`'s splash types 1, 2, 3; [`splash_effects`]).
    splash: [String; 3],
    /// `surfaceParams.z` (the lighting blend): > 0 = this surface reflects (`Water::AddReflectionPlanes`).
    reflection_blend: f32,
}

/// `WaterType::kDefaultSplashEffect`: a water type file before version 3 splashes with it.
const DEFAULT_SPLASH: &str = "records/level art/water/watersplash.dbr";

/// `WaterType::Load`: version ≥ 3 stores the three splash effect records (`WaterType.splashEffects`),
/// older ones take `kDefaultSplashEffect` for all three.
fn splash_effects(wt: &WaterType) -> [String; 3] {
    std::array::from_fn(|i| {
        wt.effects
            .get(i)
            .filter(|_| wt.version >= 3)
            .cloned()
            .unwrap_or_else(|| DEFAULT_SPLASH.to_owned())
    })
}

#[derive(Default)]
enum WaterShape {
    #[default]
    None,
    Layer {
        /// Blocks per row and column (`Water::Initialize`).
        grid: UVec2,
        /// The stored blocks' alpha bytes by block index.
        blocks: HashMap<u32, [u8; 100]>,
    },
    /// The path's raw control points (region-local).
    Path(Vec<[f32; 16]>),
}

impl WaterPoints {
    /// World height of the surface (the reflection plane).
    pub fn height(&self) -> f32 {
        self.height
    }

    /// `surfaceParams.z`: > 0 = this surface reflects.
    pub fn reflection_blend(&self) -> f32 {
        self.reflection_blend
    }

    /// `IsPointInLayer`: inside the grid, on a stored block, on a cell of non-zero alpha; a path: in one of its
    /// segments (`WaterPath::RayIntersection`).
    pub fn holds(&self, x: f32, z: f32) -> bool {
        let (x, z) = (x - self.origin.x, z - self.origin.y);
        match &self.shape {
            WaterShape::None => false,
            WaterShape::Path(points) => points
                .windows(2)
                .any(|s| gd_sim::surface::in_path_segment([x, z], &s[0], &s[1])),
            WaterShape::Layer { grid, blocks } => {
                if x < 0.0 || z < 0.0 || x >= grid.x as f32 * 9.0 || z >= grid.y as f32 * 9.0 {
                    return false;
                }
                let (bx, bz) = (x / 9.0, z / 9.0);
                let Some(b) = blocks.get(&(bx as u32 + bz as u32 * grid.x)) else {
                    return false;
                };
                let cell = |f: f32| ((f.fract() * 10.0) as i32).clamp(0, 9) as usize;
                b[cell(bx) + 10 * cell(bz)] != 0
            }
        }
    }
}

/// `GetWaterPointInfo` from `p`: the height of the nearest water surface at or above `p.y` holding (x, z).
pub fn water_above<'a>(layers: impl IntoIterator<Item = &'a WaterPoints>, p: Vec3) -> Option<f32> {
    water_hit(layers, p).map(|l| l.height)
}

/// [`water_above`]'s layer.
fn water_hit<'a>(
    layers: impl IntoIterator<Item = &'a WaterPoints>,
    p: Vec3,
) -> Option<&'a WaterPoints> {
    layers
        .into_iter()
        .filter(|l| l.height >= p.y && l.holds(p.x, p.z))
        .min_by(|a, b| a.height.total_cmp(&b.height))
}

/// The water material cache (keyed by type + params + path) with its NoiseTexture animation state; kept across
/// sessions.
#[derive(Resource, Default)]
struct WaterMaterials {
    mats: HashMap<(String, [u32; 14], bool), Handle<WaterMaterial>>,
    /// The shared noise bank, generated on first use.
    bank: Option<Handle<Image>>,
    /// One state per water type name (the engine: one NoiseTexture per WaterType).
    states: HashMap<String, NoiseState>,
    /// The GPU side of each state, keyed by the same water type name.
    gpu: HashMap<String, NoiseGpu>,
    /// Sky cubemaps by path (`None`: unreadable), decoded in the background ([`finish_water`] waits for them).
    skies: HashMap<String, Option<Handle<Image>>>,
    sky_loads: HashMap<String, bevy::tasks::Task<Option<Image>>>,
}

/// A water type's NoiseTexture: the record's octave weights / speeds and the live crossfade
/// (`NoiseTexture::Load` / `UpdateLayers`; the ctor zeroes everything, texture 0).
#[derive(Clone, Debug)]
struct NoiseState {
    levels: [f32; 4],
    rates: [f32; 4],
    size: u32,
    normal_map: bool,
    phase: [f32; 4],
    prev: [u32; 4],
    cur: [u32; 4],
}

/// A water type's noise pass ([`render_noise`]): its state as the pass reads it ([`noise_gpu`]) and the `size`²
/// target its materials sample ([`noise_target`]).
struct NoiseGpu {
    state: Handle<ShaderBuffer>,
    target: Handle<Image>,
}

/// `NoiseTexture::UpdateLayers`: phase += dt·rate; on wrap prev = cur,
/// cur = (cur + rand() % 256) % 256, phase = frac (one wrap per frame, as the engine).
fn advance_noise(s: &mut NoiseState, dt: f32) {
    for i in 0..4 {
        s.phase[i] += dt * s.rates[i];
        if s.phase[i] >= 1.0 {
            s.prev[i] = s.cur[i];
            s.cur[i] = (s.cur[i] + gd_game_core::rng::u32(0..256)) % 256;
            s.phase[i] = s.phase[i].fract();
        }
    }
}

/// The octave state as the noise pass reads it (`shaders/water_noise.wesl`): [phase, prev, cur, levels,
/// (size, normalMap, 0, 0)], the slice indices and flags as f32.
fn noise_gpu(s: &NoiseState) -> [[f32; 4]; 5] {
    [
        s.phase,
        s.prev.map(|i| i as f32),
        s.cur.map(|i| i as f32),
        s.levels,
        [s.size as f32, f32::from(u8::from(s.normal_map)), 0.0, 0.0],
    ]
}

/// A type's NoiseTexture target: `size`², [`NOISE_FORMAT`], sampled bilinear and repeating (the composite tiles
/// over one uv unit), only ever written by [`render_noise`].
fn noise_target(size: u32) -> Image {
    let mut img = Image::new_target_texture(size, size, NOISE_FORMAT, None);
    img.asset_usage = RenderAssetUsages::RENDER_WORLD;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

/// The engine's 256 noise textures as one 32×32 × 256 array (Engine.dll `NoiseTexture::GenerateNoise` /
/// `NoiseTexture::GenerateNoiseSlice`): value noise on a 31×31 lattice, seed = slice index.
/// h(n) = (n·(15731·n² + 789221) − 771171059) & 0x7FFFFFFF (i32 wrapping), v = 1 − h·2⁻³⁰;
/// tent weights centre 31.75, edges 15.875, diagonals 7.9375 (sum 127); byte = trunc(Σ + 127).
/// Texel 31 duplicates texel 0 (the lattice wraps mod 31), as in the engine.
fn noise_bank() -> Image {
    fn value(seed: i32, x: i32, y: i32) -> f32 {
        let n = 67777i32
            .wrapping_mul(seed)
            .wrapping_add(521i32.wrapping_mul(y))
            .wrapping_add(x);
        let h = n
            .wrapping_mul(
                15731i32
                    .wrapping_mul(n)
                    .wrapping_mul(n)
                    .wrapping_add(789221),
            )
            .wrapping_sub(771171059)
            & 0x7FFF_FFFF;
        1.0 - (h as f64 * 9.313225746154785e-10) as f32
    }
    let mut data = Vec::with_capacity(256 * 32 * 32);
    for seed in 0..256i32 {
        for y in 0..32i32 {
            for x in 0..32i32 {
                let (xm, xc, xp) = ((x + 30) % 31, x % 31, (x + 1) % 31);
                let (ym, yc, yp) = ((y + 30) % 31, y % 31, (y + 1) % 31);
                let diag = value(seed, xm, ym)
                    + value(seed, xp, ym)
                    + value(seed, xm, yp)
                    + value(seed, xp, yp);
                let edge = value(seed, xm, yc)
                    + value(seed, xp, yc)
                    + value(seed, xc, ym)
                    + value(seed, xc, yp);
                let s = diag * 7.9375 + edge * 15.875 + value(seed, xc, yc) * 31.75;
                data.push((s + 127.0) as u8);
            }
        }
    }
    let mut img = crate::textures::raw_image(
        Extent3d {
            width: 32,
            height: 32,
            depth_or_array_layers: 256,
        },
        TextureFormat::R8Unorm,
        data,
    );
    img.data_order = TextureDataOrder::LayerMajor;
    img.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

/// Builds one mesh per water layer of a level (region-local → world via `origin`).
pub fn build(water: &Water, terrain: Option<&Terrain>, origin: Vec3) -> Vec<WaterSurface> {
    let mut out = Vec::new();
    for layer in &water.layers {
        let Some(wt) = water
            .types
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(&layer.water_type))
            .or(water.types.first())
        else {
            continue;
        };
        let p = wt.params;
        let (ext_x, ext_z) = (layer.uv[0].max(1.0), layer.uv[1].max(1.0));
        // `Water::Initialize` (Engine.dll): blocks are 9 units (10 vertices
        // 1 unit apart), the grid is `(size + 8) / 9` wide where size is the
        // terrain width - 1 (`Level::NewWater`); a block spans
        // [i·9, min(i·9 + 9, layer extent)] (`CalculateBlockBounds`).
        let block = 9.0f32;
        let size = terrain
            .map(|t| t.width.saturating_sub(1))
            .unwrap_or(ext_x as u32)
            .max(1);
        let grid_w = size.div_ceil(9);
        let grid_h = terrain
            .map(|t| t.height.saturating_sub(1))
            .unwrap_or(ext_z as u32)
            .max(1)
            .div_ceil(9);
        let mut grid_blocks = HashMap::default();
        let mut pos = Vec::new();
        let mut colors = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        let mut blocks = Vec::new();
        for b in &layer.blocks {
            let (bx, bz) = (b.index % grid_w, b.index / grid_w);
            grid_blocks.insert(b.index, b.data);
            let (x0, z0) = (bx as f32 * block, bz as f32 * block);
            let (x1, z1) = ((x0 + block).min(ext_x), (z0 + block).min(ext_z));
            blocks.push((
                origin + Vec3::new(x0, layer.height, z0),
                origin + Vec3::new(x1, layer.height, z1),
            ));
            let base = pos.len() as u32;
            for j in 0..10u32 {
                let z = (z0 + j as f32).min(z1);
                for i in 0..10u32 {
                    let x = (x0 + i as f32).min(x1);
                    let ground = terrain
                        .map(|t| t.sample(x, z))
                        .unwrap_or(layer.height - 10.0);
                    let opacity = depth_opacity(&p, layer.height - ground);
                    let alpha = b.data[(j * 10 + i) as usize] as f32 / 255.0;
                    pos.push([origin.x + x, origin.y + layer.height, origin.z + z]);
                    colors.push([opacity, 0.0, 0.0, alpha]);
                }
            }
            for j in 0..9u32 {
                for i in 0..9u32 {
                    let a = base + j * 10 + i;
                    idx.extend_from_slice(&[a, a + 10, a + 1, a + 1, a + 10, a + 11]);
                }
            }
        }
        if idx.is_empty() {
            continue;
        }
        let n = pos.len();
        out.push(WaterSurface {
            mesh: Some(water_mesh(
                pos,
                colors,
                vec![[0.0; 2]; n],
                vec![[0.0; 2]; n],
                idx,
            )),
            blocks: crate::render::shadow::SceneBlocks(blocks),
            water_type: wt.clone(),
            path: false,
            points: WaterPoints {
                origin: Vec2::new(origin.x, origin.z),
                height: origin.y + layer.height,
                shape: WaterShape::Layer {
                    grid: UVec2::new(grid_w, grid_h),
                    blocks: grid_blocks,
                },
                splash: splash_effects(wt),
                reflection_blend: wt.params[7],
            },
        });
    }
    for path in &water.paths {
        // `WaterPath::Load`: a path whose type is not loaded is not used.
        let Some(wt) = water
            .types
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(&path.water_type))
        else {
            warn!(
                "Failed to load water type '{}' for water path.",
                path.water_type
            );
            continue;
        };
        if let Some(surface) = build_path(path, wt, terrain, origin) {
            out.push(surface);
        }
    }
    out
}

/// The water depth opacity of the programs' VS: `lerp(p0, p1, sat((depth − p2) / max(p3 − p2, 1)))`.
fn depth_opacity(p: &[f32; 14], depth: f32) -> f32 {
    // Both vertex builders clamp the stored depth to 0..=255 (8.8 fixed point in the vertex colour).
    let d = depth.clamp(0.0, 255.0);
    let d = d.trunc() + ((d - d.trunc()) * 255.0).trunc() / 255.0;
    let t = ((d - p[2]) / (p[3] - p[2]).max(1.0)).clamp(0.0, 1.0);
    p[0] + (p[1] - p[0]) * t
}

/// A water mesh: colour = (depth opacity, 0, 0, vertex alpha), uv = the path's texture coordinates, uv_b = its
/// flow tangent (both 0 on layers, whose program takes them from the world position and the type).
fn water_mesh(
    pos: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uv: Vec<[f32; 2]>,
    flow: Vec<[f32; 2]>,
    idx: Vec<u32>,
) -> Mesh {
    let n = pos.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, flow)
    .with_inserted_indices(Indices::U32(idx))
}

/// Cubic Bézier in x / z.
fn bezier(c: &[Vec2; 4], t: f32) -> Vec2 {
    let s = 1.0 - t;
    c[0] * (s * s * s) + c[1] * (t * 3.0 * s * s) + c[2] * (t * 3.0 * t * s) + c[3] * (t * t * t)
}

/// A segment's centre line (`p0`, `p0 + t0`, `p1 − t1`, `p1`).
fn centre(a: &ControlPoint, b: &ControlPoint) -> [Vec2; 4] {
    let (p0, t0) = (Vec2::from(a.pos), Vec2::from(a.tangent));
    let (p1, t1) = (Vec2::from(b.pos), Vec2::from(b.tangent));
    [p0, p0 + t0, p1 - t1, p1]
}

/// `WaterPath::CentreLength`: the polyline length of the centre over `n` steps.
fn centre_length(c: &[Vec2; 4], n: u32) -> f32 {
    let mut prev = bezier(c, 0.0);
    let mut len = 0.0;
    for i in 1..=n {
        let p = bezier(c, i as f32 / n as f32);
        len += p.distance(prev);
        prev = p;
    }
    len
}

/// One path's mesh and hit shape (module doc, `WaterPath::BuildSegment` per segment).
fn build_path(
    path: &WaterPath,
    wt: &WaterType,
    terrain: Option<&Terrain>,
    origin: Vec3,
) -> Option<WaterSurface> {
    let p = wt.params;
    let points: Vec<ControlPoint> = path.control_points().collect();
    let segments = points.len().saturating_sub(1);
    // `WaterType::SetShaderParams`: textureOffset = frac(region offset / textureScale size); the path VS adds it
    // swapped (u + offset.z term, v + offset.x term).
    let frac = |v: f64| (v - v.trunc()) as f32;
    let tex_off = Vec2::new(
        frac(origin.z as f64 * (1.0 / p[10]) as f64),
        frac(origin.x as f64 * (1.0 / p[9]) as f64),
    );
    let (mut pos, mut colors, mut uv, mut flow, mut idx) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut blocks = Vec::new();
    for (si, w) in points.windows(2).enumerate() {
        let flags = (si == 0) as u8 | if si + 1 == segments { 2 } else { 0 };
        let base = pos.len() as u32;
        let rows = path_segment(&w[0], &w[1], flags, &p, |x, z, s| {
            let ground = terrain.map(|t| t.sample(x, z)).unwrap_or(path.value - 10.0);
            pos.push([origin.x + x, origin.y + path.value, origin.z + z]);
            colors.push([depth_opacity(&p, path.value - ground), 0.0, 0.0, s.alpha]);
            uv.push([s.uv[0] + tex_off.x, s.uv[1] + tex_off.y]);
            flow.push(s.flow);
        });
        let Some(rows) = rows else { continue };
        let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        for v in &pos[base as usize..] {
            lo = lo.min(Vec2::new(v[0], v[2]));
            hi = hi.max(Vec2::new(v[0], v[2]));
        }
        let y = origin.y + path.value;
        blocks.push((Vec3::new(lo.x, y, lo.y), Vec3::new(hi.x, y, hi.y)));
        for r in 0..rows {
            for k in 0..9u32 {
                let a = base + r * 10 + k;
                for tri in [[a, a + 10, a + 1], [a + 1, a + 10, a + 11]] {
                    // Faces up whichever bank the side points to (the engine draws a strip, unculled).
                    let [p0, p1, p2] = tri.map(|i| Vec3::from(pos[i as usize]));
                    if (p1 - p0).cross(p2 - p0).y >= 0.0 {
                        idx.extend_from_slice(&tri);
                    } else {
                        idx.extend_from_slice(&[tri[0], tri[2], tri[1]]);
                    }
                }
            }
        }
    }
    if idx.is_empty() {
        return None;
    }
    Some(WaterSurface {
        mesh: Some(water_mesh(pos, colors, uv, flow, idx)),
        blocks: crate::render::shadow::SceneBlocks(blocks),
        water_type: wt.clone(),
        path: true,
        points: WaterPoints {
            origin: Vec2::new(origin.x, origin.z),
            height: origin.y + path.value,
            shape: WaterShape::Path(path.points.clone()),
            splash: splash_effects(wt),
            reflection_blend: wt.params[7],
        },
    })
}

/// One vertex of a path segment (before the region offset).
struct PathVertex {
    uv: [f32; 2],
    flow: [f32; 2],
    alpha: f32,
}

/// `WaterPath::BuildSegment`: one segment's grid, rows along × 10 vertices across, each passed to `vertex(x, z, v)`
/// (region-local x / z); returns the number of row gaps (`None`: too short for geometry). `flags` 1 = the path's
/// first segment, 2 = its last.
fn path_segment(
    a: &ControlPoint,
    b: &ControlPoint,
    flags: u8,
    p: &[f32; 14],
    mut vertex: impl FnMut(f32, f32, PathVertex),
) -> Option<u32> {
    let c = centre(a, b);
    let n = ((centre_length(&c, 20) * 0.25) as u32).min(64);
    if n == 0 {
        return None;
    }
    let len = centre_length(&c, n);
    // The inner bank handles: the centre's handles moved by the width along the normal of the curve near them
    // (the chord t = 1/3 → 0.4333 and t = 2/3 → 0.7666).
    let normal = |t0: f32, t1: f32| {
        let d = bezier(&c, t1) - bezier(&c, t0);
        Vec2::new(-d.y, d.x) * (1.0 / d.length())
    };
    let (n1, n2) = (
        normal(1.0 / 3.0, 0.433_333_3),
        normal(2.0 / 3.0, 0.766_666_6),
    );
    let (s0, s1) = (Vec2::from(a.side), Vec2::from(b.side));
    let bank = |sign: f32| {
        [
            c[0] + s0 * (a.width * sign),
            c[1] + n1 * (a.width * sign),
            c[2] + n2 * (b.width * sign),
            c[3] + s1 * (b.width * sign),
        ]
    };
    let (left, right) = (bank(1.0), bank(-1.0));
    // The along coordinate: a whole number of `p9` repeats over the segment; across: metres / `p10`.
    let repeats = ((len / p[9]) as i32).max(1) as f32;
    let (mut along, mut prev) = (0.0f32, (left[0], right[0]));
    let t0 = Vec2::from(a.tangent).normalize_or_zero();
    for r in 0..=n {
        let t = (r as f64 / n as f64) as f32;
        let (ea, eb) = (bezier(&left, t), bezier(&right, t));
        let (ta, tb) = if r == 0 {
            (t0, t0)
        } else {
            let (da, db) = (ea - prev.0, eb - prev.1);
            along += (da.length() + db.length()) * 0.5 / len * repeats;
            (da.normalize_or_zero(), db.normalize_or_zero())
        };
        prev = (ea, eb);
        let mut alpha = 255u8;
        if flags & 2 != 0 {
            if r == n {
                alpha = 0;
            } else if r + 1 == n {
                alpha = 127;
            }
        }
        if flags & 1 != 0 {
            if r == 0 {
                alpha = 0;
            } else if r == 1 {
                alpha = 127;
            }
        }
        let width = (1.0 - t) * a.width + t * b.width;
        let mut s = 0.0f32;
        for _ in 0..10 {
            let q = ea * (1.0 - s) + eb * s;
            vertex(
                q.x,
                q.y,
                PathVertex {
                    uv: [width * s / p[10], along],
                    flow: [ta.y * (1.0 - s) + tb.y * s, -(ta.x * (1.0 - s) + tb.x * s)],
                    alpha: alpha as f32 / 255.0,
                },
            );
            s += 0.111_111_11;
        }
    }
    Some(n)
}

fn uniform_for(wt: &WaterType, path: bool, noise: &NoiseState) -> WaterUniform {
    let p = wt.params;
    let ang = p[8].to_radians();
    WaterUniform {
        surface: Vec4::new(p[5], p[6], p[7], 0.0),
        water_color: Vec4::new(p[11], p[12], p[13], 1.0),
        tex_scale_dir: Vec4::new(
            1.0 / p[9].max(1e-3),
            1.0 / p[10].max(1e-3),
            ang.cos(),
            ang.sin(),
        ),
        scroll_time: Vec4::new(
            p[4] / p[9].max(1e-3),
            if path { 1.0 } else { 0.0 },
            0.0,
            if noise.normal_map { 1.0 } else { 0.0 },
        ),
    }
}

/// The water type's NoiseTexture record (`NoiseTexture::Load`: `size`, `normalMap`,
/// `levels[4]`, `rates[4]`); the animation starts at texture 0.
fn wt_noise(wt: &WaterType, data: &GameData) -> NoiseState {
    let rec = data.db.get(&wt.texture);
    let v = |k: &str, d: [f32; 4]| {
        let f = rec.as_ref().map(|r| r.f32s(k)).unwrap_or_default();
        std::array::from_fn(|i| f.get(i).copied().unwrap_or(d[i]))
    };
    NoiseState {
        levels: v("levels", [0.8, 0.4, 0.2, 0.1]),
        rates: v("rates", [0.95, 1.75, 2.85, 3.64]),
        size: rec
            .as_ref()
            .and_then(|r| r.i32("size"))
            .unwrap_or(256)
            .max(1) as u32,
        normal_map: rec.as_ref().map(|r| r.bool("normalMap")).unwrap_or(true),
        phase: [0.0; 4],
        prev: [0; 4],
        cur: [0; 4],
    }
}

/// A cubemap `.tex` (6 faces) as a Bevy cube image.
fn load_cube(data: &GameData, path: &str) -> Option<Image> {
    let tex = gd_formats::tex::Tex::parse_owned(data.vfs.read(path).ok()?).ok()?;
    let dds = &tex.frames[0];
    if dds.faces != 6 {
        return None;
    }
    let mut bytes = Vec::new();
    for f in 0..6 {
        bytes.extend_from_slice(&dds.decode_face_rgba8(f).ok()?);
    }
    let mut img = crate::textures::raw_image(
        Extent3d {
            width: dds.width,
            height: dds.height,
            depth_or_array_layers: 6,
        },
        TextureFormat::Rgba8Unorm,
        bytes,
    );
    img.data_order = TextureDataOrder::LayerMajor;
    img.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    Some(img)
}

#[allow(clippy::too_many_arguments)]
fn finish_water(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut WaterSurface)>,
    data: Res<GameData>,
    defaults: Res<GdDefaults>,
    reflection_target: Res<crate::render::reflection::ReflectionTarget>,
    mut cache: ResMut<WaterMaterials>,
    mut mats: ResMut<Assets<WaterMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut textures: ResMut<Textures>,
) {
    for (e, mut ws) in &mut q {
        if ws.mesh.is_none() {
            continue;
        }
        #[cfg(feature = "dev")]
        if hooks.render.no_water {
            commands.entity(e).try_remove::<WaterSurface>();
            continue;
        }
        let wt = &ws.water_type;
        let key = (wt.name.clone(), wt.params.map(f32::to_bits), ws.path);
        let param = |name: &str| {
            wt.shader_params
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        let is_lava = wt.shader.to_ascii_lowercase().ends_with("lavabasic.ssh");
        let sky_path = param("skyTexture").map(str::to_owned);
        // A new material waits for its textures (a lava pool's four, the sky cubemap) to decode in the background.
        if !cache.mats.contains_key(&key) {
            let mut ready = true;
            if is_lava {
                for name in ["lavaTexture", "lavaNormal", "ashTexture", "ashNormal"] {
                    if let Some(p) = param(name) {
                        ready &= textures.ready(&data, p, false, &mut images);
                    }
                }
            }
            if let Some(p) = &sky_path {
                ready &= sky_ready(&mut cache, &data, p, &mut images);
            }
            if !ready {
                continue;
            }
        }
        let Some(mesh) = ws.mesh.take() else { continue };
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        {
            debug!("water {} verts, e.g. {:?}", p.len(), p.first());
        }
        let wt = &ws.water_type;
        let WaterMaterials {
            mats: mat_cache,
            bank,
            states,
            gpu,
            skies,
            ..
        } = &mut *cache;
        let mat = mat_cache
            .entry(key)
            .or_insert_with(|| {
                bank.get_or_insert_with(|| images.add(noise_bank()));
                let state = states
                    .entry(wt.name.clone())
                    .or_insert_with(|| wt_noise(wt, &data));
                let noise = gpu
                    .entry(wt.name.clone())
                    .or_insert_with(|| NoiseGpu {
                        state: buffers.add(ShaderBuffer::new(
                            noise_gpu(state).to_vec(),
                            RenderAssetUsages::RENDER_WORLD,
                        )),
                        target: images.add(noise_target(state.size)),
                    })
                    .target
                    .clone();
                let param = |name: &str| {
                    wt.shader_params
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(name))
                        .map(|(_, v)| v.as_str())
                };
                let mut lava_tex = |name: &str| {
                    param(name)
                        .filter(|_| is_lava)
                        .and_then(|p| textures.get(&data, p, false, &mut images))
                        .unwrap_or_default()
                };
                let lava = lava_tex("lavaTexture");
                let lava_normal = lava_tex("lavaNormal");
                let ash = lava_tex("ashTexture");
                let ash_normal = lava_tex("ashNormal");
                let sky = sky_path
                    .as_ref()
                    .and_then(|p| skies.get(p).cloned().flatten())
                    .unwrap_or_else(|| {
                        let mut i = crate::textures::raw_image(
                            Extent3d {
                                width: 1,
                                height: 1,
                                depth_or_array_layers: 6,
                            },
                            TextureFormat::Rgba8Unorm,
                            [150, 160, 170, 255].repeat(6),
                        );
                        i.texture_view_descriptor = Some(TextureViewDescriptor {
                            dimension: Some(TextureViewDimension::Cube),
                            ..default()
                        });
                        images.add(i)
                    });
                mats.add(WaterMaterial {
                    params: uniform_for(wt, ws.path, state),
                    noise,
                    sky,
                    env: defaults.env.clone(),
                    reflection: reflection_target.image.clone(),
                    lava,
                    lava_normal,
                    ash,
                    ash_normal,
                    is_lava,
                })
            })
            .clone();
        let blocks = std::mem::take(&mut ws.blocks);
        let points = std::mem::take(&mut ws.points);
        commands.entity(e).try_remove::<WaterSurface>().try_insert((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(mat),
            bevy::light::NotShadowCaster,
            blocks,
            points,
        ));
    }
}

/// Whether the sky cubemap at `path` is decoded (or failed): polls its background decode, or starts it.
fn sky_ready(
    cache: &mut WaterMaterials,
    data: &GameData,
    path: &str,
    images: &mut Assets<Image>,
) -> bool {
    use bevy::tasks::{block_on, futures_lite::future};
    if cache.skies.contains_key(path) {
        return true;
    }
    let Some(task) = cache.sky_loads.get_mut(path) else {
        let (key, data) = (path.to_owned(), data.clone());
        cache.sky_loads.insert(
            path.to_owned(),
            bevy::tasks::AsyncComputeTaskPool::get().spawn(async move { load_cube(&data, &key) }),
        );
        return false;
    };
    let Some(img) = block_on(future::poll_once(task)) else {
        return false;
    };
    cache.sky_loads.remove(path);
    cache
        .skies
        .insert(path.to_owned(), img.map(|i| images.add(i)));
    true
}

/// Steps each water type's NoiseTexture crossfade (`NoiseTexture::UpdateLayers`) and rewrites its shared
/// [`ShaderBuffer`].
fn update_water_noise(
    time: Res<Time<Virtual>>,
    mut cache: ResMut<WaterMaterials>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let WaterMaterials { states, gpu, .. } = &mut *cache;
    for (name, s) in states.iter_mut() {
        advance_noise(s, dt);
        if let Some(g) = gpu.get(name)
            && let Some(mut b) = buffers.get_mut(&g.state)
        {
            b.clear();
            b.extend_from_slice(&noise_gpu(s));
        }
    }
}

/// The noise pass's pipeline (`shaders/water_noise.wesl` `composite`, fullscreen into a [`NOISE_FORMAT`] target).
#[derive(Resource)]
struct NoisePipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline: CachedRenderPipelineId,
}

/// This frame's noise passes: per water type with water visible in some view, its bank, state buffer and target
/// ([`extract_noise`]), then each pass's bind group and target view ([`prepare_noise`]).
#[derive(Resource, Default)]
struct NoisePasses {
    jobs: Vec<(AssetId<Image>, AssetId<ShaderBuffer>, AssetId<Image>)>,
    binds: Vec<(BindGroup, TextureView)>,
}

fn init_noise_pipeline(
    mut commands: Commands,
    assets: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "gd_water_noise_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d_array(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                storage_buffer_read_only_sized(false, None),
            ),
        ),
    );
    let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("gd_water_noise".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: assets.load(NOISE_SHADER),
            entry_point: Some("composite".into()),
            targets: vec![Some(ColorTargetState {
                format: NOISE_FORMAT,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(NoisePipeline { layout, pipeline });
}

/// The water types to composite this frame: those with a water entity visible in any view (after frustum culling).
fn extract_noise(
    mut passes: ResMut<NoisePasses>,
    cache: Extract<Res<WaterMaterials>>,
    water: Extract<Query<(&MeshMaterial3d<WaterMaterial>, &ViewVisibility)>>,
    mut seen: Local<Vec<AssetId<WaterMaterial>>>,
) {
    passes.jobs.clear();
    let Some(bank) = &cache.bank else {
        return;
    };
    seen.clear();
    seen.extend(water.iter().filter(|(_, v)| v.get()).map(|(m, _)| m.id()));
    for ((name, ..), mat) in &cache.mats {
        if !seen.contains(&mat.id()) {
            continue;
        }
        let Some(g) = cache.gpu.get(name) else {
            continue;
        };
        let job = (bank.id(), g.state.id(), g.target.id());
        if !passes.jobs.contains(&job) {
            passes.jobs.push(job);
        }
    }
}

fn prepare_noise(
    mut passes: ResMut<NoisePasses>,
    pipeline: Option<Res<NoisePipeline>>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    device: Res<RenderDevice>,
) {
    let NoisePasses { jobs, binds } = &mut *passes;
    binds.clear();
    let Some(p) = pipeline.as_deref() else {
        return;
    };
    if jobs.is_empty() {
        return;
    }
    let layout = cache.get_bind_group_layout(&p.layout);
    for &(bank, state, target) in jobs.iter() {
        let (Some(bank), Some(state), Some(target)) =
            (images.get(bank), buffers.get(state), images.get(target))
        else {
            continue;
        };
        let bind = device.create_bind_group(
            "gd_water_noise",
            &layout,
            &BindGroupEntries::sequential((
                &bank.texture_view,
                &bank.sampler,
                state.buffer.as_entire_binding(),
            )),
        );
        binds.push((bind, target.texture_view.clone()));
    }
}

/// `NoiseTexture`'s composite + convert (module doc): each visible water type's target, overwritten whole.
fn render_noise(
    passes: Res<NoisePasses>,
    pipeline: Option<Res<NoisePipeline>>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let Some(p) = pipeline.as_deref() else {
        return;
    };
    let Some(pipe) = cache.get_render_pipeline(p.pipeline) else {
        return;
    };
    for (bind, target) in &passes.binds {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("gd_water_noise"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Default::default()),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(pipe);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

const RIPPLE_SHADER: &str = "embedded://gd_render/shaders/ripple.wesl";
/// `WaterRipple.ssh`'s texture.
const RIPPLE_TEXTURE: &str = "fx/textures/waterwalkripple_01.tex";

/// `WaterRipple.ssh` (`shaders/ripple.wesl`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct RippleMaterial {
    /// x = `dot(lightDirection, (-1, -1, -1))` (the VS's light term).
    #[uniform(0)]
    params: Vec4,
    #[texture(1)]
    #[sampler(2)]
    texture: Handle<Image>,
    /// Header vec4 (count), then [centre, 0], [r.x, r.z, alpha, 0] per drawn ripple.
    #[storage(3, read_only)]
    ripples: Handle<ShaderBuffer>,
}

impl Material for RippleMaterial {
    fn vertex_shader() -> ShaderRef {
        RIPPLE_SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        RIPPLE_SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    /// Sort 4020: after the water (bias 0), before the effect particles (1000 and up).
    fn depth_bias(&self) -> f32 {
        500.0
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vl = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
        ])?;
        descriptor.vertex.buffers = vec![vl];
        descriptor.primitive.cull_mode = None;
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(false);
        }
        // The pass states: SrcAlpha / InvSrcAlpha, alpha not written.
        let c = BlendComponent {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        };
        if let Some(frag) = descriptor.fragment.as_mut() {
            for t in frag.targets.iter_mut().flatten() {
                t.blend = Some(BlendState { color: c, alpha: c });
                t.write_mask = ColorWrites::COLOR;
            }
        }
        Ok(())
    }
}

/// The ripple set and its drawing; held by `render::weather::WeatherState` (reset with the session).
#[derive(Default)]
pub struct RippleSet {
    pub sim: gd_sim::weather::Ripples,
    /// This frame's `CreateRipple(pos, size, strength)` calls, placed by [`update_ripples`].
    pub requests: Vec<(Vec3, f32, f32)>,
    draw: Option<(Entity, Handle<ShaderBuffer>, Handle<RippleMaterial>)>,
    /// Whether the draw entity is `Visible` (hidden while no ripple is drawn).
    shown: bool,
    scratch: Vec<[f32; 4]>,
}

impl RippleSet {
    /// Drops every ripple and the draw entity (scene change, weather off).
    pub fn clear(&mut self, commands: &mut Commands) {
        if let Some((e, ..)) = self.draw.take() {
            commands.entity(e).try_despawn();
        }
        self.shown = false;
        self.sim = Default::default();
        self.requests.clear();
    }
}

fn ripple_bytes() -> u64 {
    ((1 + gd_sim::weather::Ripples::SLOTS * 2) * 16) as u64
}

/// The frame's ripple requests onto the water (`CreateRipple`), then `Water::Update` (growth) and the quads.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_ripples(
    mut rt: ResMut<crate::render::weather::WeatherState>,
    mut disturbances: MessageReader<gd_game_core::visuals::PointDisturbance>,
    layers: Query<&WaterPoints>,
    options: Res<gd_game_core::options::GameOptions>,
    time: Res<Time<Virtual>>,
    data: Res<GameData>,
    lights: Query<&GlobalTransform, With<DirectionalLight>>,
    mut textures: ResMut<Textures>,
    mut res: (
        ResMut<Assets<Image>>,
        ResMut<Assets<Mesh>>,
        ResMut<Assets<RippleMaterial>>,
        ResMut<Assets<ShaderBuffer>>,
    ),
    mut commands: Commands,
) {
    let (images, meshes, mats, buffers) = &mut res;
    let set = &mut rt.ripples;
    for (p, size, strength) in set.requests.drain(..) {
        let w = water_above(layers.iter(), p - Vec3::Y * 2.0);
        set.sim.create(p.to_array(), w, size, strength);
    }
    // `World::CreatePointDisturbance`'s water half: `CreateRipple(pos, radius, strength, splash)`.
    for g in disturbances.read() {
        let hit = water_hit(layers.iter(), g.pos - Vec3::Y * 2.0);
        if !set.sim.create(
            g.pos.to_array(),
            hit.map(|l| l.height),
            g.radius,
            g.strength,
        ) {
            continue;
        }
        let Some(l) = hit else { continue };
        // `CreateRipple`: splash type 1 only under the surface; the effect starts at the water point.
        let effect = match g.splash {
            1 if l.height - g.pos.y > 0.0 => &l.splash[0],
            2 => &l.splash[1],
            3 => &l.splash[2],
            _ => continue,
        };
        let at = Transform::from_xyz(g.pos.x, l.height, g.pos.z);
        gd_game_core::visuals::spawn_fx(&mut commands, &data, effect, at);
    }
    let dt_ms = time.delta_secs() * 1000.0;
    if dt_ms > 0.0 {
        set.sim.update(dt_ms);
    }
    let show = options.flag("detailObjects");
    set.scratch.clear();
    set.scratch.push([0.0; 4]);
    if show {
        for (i, r) in set.sim.slots.iter().enumerate() {
            if !r.drawn {
                continue;
            }
            let a = gd_sim::weather::Ripples::angle(i);
            set.scratch.push([r.pos[0], r.pos[1], r.pos[2], 0.0]);
            set.scratch
                .push([a.cos() * r.size, a.sin() * r.size, r.alpha, 0.0]);
        }
    }
    let n = (set.scratch.len() - 1) / 2;
    set.scratch[0][0] = f32::from_bits(n as u32);
    if n == 0 {
        if set.shown
            && let Some((e, ..)) = &set.draw
        {
            commands.entity(*e).try_insert(Visibility::Hidden);
        }
        set.shown = false;
        return;
    }
    // `lightDirection` points towards the light (the water programs' convention).
    let to_light = lights
        .iter()
        .next()
        .map_or(Vec3::Y, |t| -t.forward().as_vec3());
    let params = Vec4::new(-(to_light.x + to_light.y + to_light.z), 0.0, 0.0, 0.0);
    if set.draw.is_none() {
        let Some(texture) = textures.get(&data, RIPPLE_TEXTURE, false, images) else {
            return;
        };
        let mut buf = ShaderBuffer::new(vec![[0.0f32; 4]], RenderAssetUsages::RENDER_WORLD);
        buf.resize_buffer(ripple_bytes());
        let buffer = buffers.add(buf);
        let mat = mats.add(RippleMaterial {
            params,
            texture,
            ripples: buffer.clone(),
        });
        let mesh = meshes.add(crate::fx::render::quad_mesh(
            gd_sim::weather::Ripples::SLOTS,
        ));
        let e = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(mat.clone()),
                Transform::IDENTITY,
                Visibility::Visible,
                bevy::camera::visibility::NoFrustumCulling,
                bevy::light::NotShadowCaster,
                Name::new("water ripples"),
            ))
            .id();
        set.draw = Some((e, buffer, mat));
        set.shown = true;
    }
    let Some((e, buffer, mat)) = &set.draw else {
        return;
    };
    if !set.shown {
        commands.entity(*e).try_insert(Visibility::Visible);
        set.shown = true;
    }
    if mats.get(mat).is_some_and(|m| m.params != params)
        && let Some(mut m) = mats.get_mut(mat)
    {
        m.params = params;
    }
    if let Some(mut b) = buffers.get_mut(buffer) {
        b.clear();
        b.extend_from_slice(&set.scratch);
        b.resize_buffer(ripple_bytes());
    }
}
