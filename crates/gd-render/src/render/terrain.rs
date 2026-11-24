//! Terrain (`TerrainRT`) with the game's layer compositing.
//!
//! Engine (Engine.dll): `TerrainRenderInterfaceRT::RenderGround` draws one
//! pass per layer, top layer first, only over cells whose top-left vertex has
//! the layer's bit (bit layer+1, plus bit 0) in the per-vertex mask. Each pass
//! (shaders/terrain/{terrain,snow}*quality.ssh, `deferred` technique) writes
//! G-buffer values ·(1−a) with blend `src·dstAlpha + dst`, `dstAlpha *= a`,
//! where `a = sat(4·(base.a − opacity.a))`; layer 0's opacity is all 255.
//! UVs: base/bump `world.xz·invBlockSize(0.1)` (the shader's
//! `regionXZ·0.1 + textureOffset`, textureOffset = (regionX % 10)·0.1);
//! opacity `(x/(w−1), 1 − z/(h−1))` over the layer's (w−1)×(h−1) byte weights
//! (created as an A8 texture by `TerrainRT::CreateOpacityTexture`). `terrainParams` =
//! (TerrainType.specularPower, …), ambientIntensity 0.65, dSpecularIntensity
//! 1.56 (ssh defaults). `TerrainType::Load` reads
//! `specularPower` with the default 10.0 (also the ctor's, `TerrainType::TerrainType`):
//! 287 of the 322 stock terrain types leave it out.
//!
//! The terrain casts no sun shadow: `GraphicsMTRenderer::
//! BuildShadowDirectional` fills the shadow scene with actors only
//! (`castsShadows` set, `Actor::AddToScene`; `AddRegionToScene` only sets the
//! region's coordinates) and `ExecutePrepareShadow` collects their `shadow`
//! technique passes, which the terrain*.ssh shaders do not have. It receives
//! them (deferred directional pass, gd_terrain.wesl).
//!
//! Terrain quality 3 (`detailLevel` ultra). `GraphicsEngine::UpdateFromOptions`: quality
//! (`GraphicsEngine.terrainQuality`) 3 when `detailLevel` = 4 (ultra) and the deferred flag; then
//! `TerrainRT::SetupLayerShader` (the layer shader) takes the TerrainType's
//! `{terrain|snow}[parallax][glow]ultraquality.ssh` (`TerrainType::Load`, `TerrainType.ultraShader`) and
//! draws the layers in reverse (index n − a2: bottom layer first). terrainultraquality `deferredPS` (value 38):
//! o0 = base.rgb, every RT alpha α = (1 − sat(4(base.a − opacity))) · soft, blend SrcAlpha / InvSrcAlpha
//! (states 6 / 8): bottom-first "over" with α = coverage equals high's top-first dest-alpha composite, so only
//! `soft` changes the result. `soft` (snow and parallax variants alike): the `depthTexture` (t3) at the pixel,
//! linearised (n·f / (f − d(f − n))) against the fragment's own depth: dz; when |dz| ≤ 0.35 m,
//! soft = k + (1 − k)·nAgree with k = |dz| / 0.35 and nAgree = sat((max(face·vertexNormal, 0) − 0.9)·10), the face
//! normal from coarse screen derivatives of the position rebuilt from that depth (`screenToCameraMatrix`); else 1.
//! The cover left over (Π(1 − α)) keeps the cleared G-buffer (0).
//! Which depth: `TerrainRenderInterfaceRT::RenderGround` binds `depthTexture` at quality 3 only: the
//! canvas's primary surface's depth texture (`GraphicsCanvas.primarySurface`, the virtual
//! `RenderSurface::GetDepthTexture` = `Direct3DSurface.depthTexture`, the texture's virtual `Copy`
//! copies). The canvas's primary surface is the device's swap-chain surface:
//! `GraphicsEngine::PostDeviceReset` / `CreateDevice` store `Direct3DDevice11::CreatePrimarySurface`
//! (Direct3D11.dll `Direct3DDevice11.primarySurface`) in `GraphicsEngine.primarySurface` and the canvas's
//! `primarySurface` / `target` / `depthTarget`, and
//! `GraphicsDeferredRenderer::RenderFinalScene` never touches `primarySurface`: it points the canvas target / depth
//! at the G-buffer (`GraphicsDeferredRenderer.gBuffer`), then the scene surfaces (`sceneTarget` / `hdrTarget`,
//! depth `sceneDepth`). So the deferred scene
//! never writes the depth the terrain reads (no terrain, no meshes in it): |dz| > 0.35 m and `soft` = 1 everywhere,
//! i.e. quality 3 looks like quality 2. Port: the material keeps [`GdDefaults::depth`] (depth 0, reverse-Z far), for
//! which `ultra_soft` returns 1. (Binding the previous frame's scene depth instead blacked out every facet more than
//! ~25° off its smooth vertex normal: report R1.) PORT: the face normal is turned to face the camera.
//!
//! Terrain patches: a `TerrainDecoration` (the `*_paintable` floors, bridges, platforms) carries terrain paint in its
//! level data (`gd_formats::map::level::Entity::terrain_patch`), drawn over its own upward faces. Engine.dll
//! `TerrainPatch::CreateGeometry`: the mesh's vertices with the triangles whose mean vertex normal,
//! turned by the placement's rotation, has up ≥ cos(`maxPaintableAngle`) (`TerrainDecoration::Load`, default 88°);
//! base / bump UV world.xz·0.1 as the terrain; opacity UV (x − cx) / width + ½ (z alike), c = floor(position + ½)
//! region-local, over a width × depth A8 texture per painted layer (1 unit per texel, linear, clamped).
//! `RenderPass` draws a base pass, then one pass per painted layer of the level terrain's layer list with that
//! TerrainType's patch shader (`TerrainType.patchShaders`, by quality). terrainpatch.ssh `deferred` (sort 1050,
//! DirectX11 `deferredPS`): G-buffer values as the terrain layer's (o0 = base.rgb, normal = bump in the mesh's
//! tangent / binormal / normal, ambientIntensity, log2(specularPower) / 10.5, specular · dSpecularIntensity) at
//! alpha = 1 − sat(4(base.a − opacity)), SrcAlpha / InvSrcAlpha, no depth write, DepthFunc LessEqual: the layers
//! composited bottom first over the mesh. Here: one [`GdTerrainMaterial`] per patch in patch mode (`grid.w` = 1:
//! its opacities, the level terrain's layer textures and parameters, `layers[i].z` = painted) on the patch triangles,
//! drawn with the G-buffer blends (render/gbuffer_blend.rs) on the same vertices and transform as the decoration's
//! parts, so the depth matches (LessEqual). PORT: the vertices within 0.5 m of the terrain do not take the
//! terrain's normal frame (`CreateGeometry` does), and the patch grass (`RenderGrass`) is not drawn.
use super::GdDefaults;
use super::shadow::SceneBlocks;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDataOrder, TextureFormat, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use gd_formats::map::terrain::Terrain;
use gd_formats::tex::Tex;
use gd_game_core::data::GameData;
use parking_lot::Mutex;
use std::sync::Arc;

/// Layer textures are resampled to this size for the per-level arrays.
const LAYER_SIZE: u32 = 512;
pub const MAX_LAYERS: usize = 16;

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct TerrainParams {
    /// xyz level origin, w layer count.
    pub origin: Vec4,
    /// x: w−1, y: h−1, z: 1 at terrain quality 3 (module doc).
    pub grid: Vec4,
    /// Per layer: x kind (0 terrain, 1 snow), y specular power.
    pub layers: [Vec4; MAX_LAYERS],
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct GdTerrainMaterial {
    #[uniform(0)]
    pub params: TerrainParams,
    #[texture(1, dimension = "2d_array")]
    #[sampler(2)]
    pub colors: Handle<Image>,
    #[texture(3, dimension = "2d_array")]
    #[sampler(4)]
    pub normals: Handle<Image>,
    #[texture(5, dimension = "2d_array")]
    #[sampler(6)]
    pub opacity: Handle<Image>,
    #[storage(7, read_only)]
    pub env: Handle<ShaderBuffer>,
    /// Quality 3's `depthTexture`: the swap-chain surface's depth, which holds no scene (module doc).
    #[texture(8, sample_type = "depth")]
    pub depth: Handle<Image>,
}

impl Material for GdTerrainMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://gd_render/render/shaders/gd_terrain.wesl".into()
    }
    /// A terrain patch blends over its decoration (module doc); the terrain is opaque.
    fn alpha_mode(&self) -> AlphaMode {
        if self.params.grid.w > 0.5 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        }
    }
}

/// `TerrainType::Load`'s default `maxPaintableAngle` (degrees; Game.dll `TerrainDecoration::Load`:
/// the record's value, else 88).
pub const MAX_PAINTABLE_ANGLE: f32 = 88.0;

/// A terrain patch's triangles (module doc): the mesh's faces whose mean vertex normal, turned by `rotation`
/// (the placement without its scale), has an up component of at least cos(`max_angle_deg`). Same vertices as
/// the decoration's own parts (`model.rs`, winding reversed the same way). `None` when no face qualifies.
pub fn patch_mesh(msh: &gd_formats::msh::Mesh, rotation: Mat3, max_angle_deg: f32) -> Option<Mesh> {
    let v = &msh.vertices;
    let n = msh.num_vertices as usize;
    if v.normals.len() != n {
        return None;
    }
    let cos = max_angle_deg.to_radians().cos();
    let up = rotation.transpose().y_axis;
    let mut idx: Vec<u16> = Vec::new();
    for t in msh.indices.as_chunks::<3>().0 {
        let nn = t
            .iter()
            .map(|&i| Vec3::from(v.normals.get(i as usize).copied().unwrap_or_default()))
            .sum::<Vec3>()
            * 0.333_333_34;
        if up.dot(nn) >= cos {
            idx.extend_from_slice(&[t[0], t[2], t[1]]);
        }
    }
    if idx.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, v.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, v.normals.clone());
    let tangents = crate::model::msh_tangents(msh);
    if !tangents.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    }
    mesh.insert_indices(Indices::U16(idx));
    Some(mesh)
}

/// A terrain patch's material: the level terrain's layer textures and parameters (`level`), the patch's
/// opacities over its `width`×`depth` texel grid whose corner is `corner` (world x / z: the level origin plus
/// the entity's rounded region-local x / z, minus half the grid).
pub fn patch_material(
    level: &GdTerrainMaterial,
    patch: &gd_formats::map::level::TerrainPatch,
    corner: Vec3,
    images: &mut Assets<Image>,
) -> Option<GdTerrainMaterial> {
    let n = (level.params.origin.w as usize).min(patch.layers.len());
    let cells = patch.width as usize * patch.depth as usize;
    let mut params = level.params;
    let mut data = vec![0u8; n * cells];
    let mut any = false;
    for (i, l) in patch.layers.iter().take(n).enumerate() {
        params.layers[i].z = 0.0;
        if let Some(w) = l.as_ref().filter(|w| w.len() == cells) {
            data[i * cells..(i + 1) * cells].copy_from_slice(w);
            params.layers[i].z = 1.0;
            any = true;
        }
    }
    if !any {
        return None;
    }
    params.origin = corner.extend(n as f32);
    params.grid = Vec4::new(
        patch.width as f32,
        patch.depth as f32,
        level.params.grid.z,
        1.0,
    );
    let mut opacity = array_image(
        patch.width,
        patch.depth,
        n as u32,
        TextureFormat::R8Unorm,
        data,
        1,
    );
    opacity.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    Some(GdTerrainMaterial {
        params,
        opacity: images.add(opacity),
        ..level.clone()
    })
}

/// Terrain quality 3 from `detailLevel` (module doc), read every frame as the engine reads
/// `GraphicsEngine.terrainQuality`:
/// sets each terrain material's quality flag (its `depthTexture` stays [`GdDefaults::depth`], module doc). PORT: the
/// engine also needs its deferred flag (`GraphicsEngineOptions.flags` bit 0x80000, generation 7 / 8); the port is
/// always deferred-like.
pub fn sync_terrain_quality(
    options: Res<gd_game_core::options::GameOptions>,
    mut mats: ResMut<Assets<GdTerrainMaterial>>,
) {
    let q = if is_ultra(&options) { 1.0 } else { 0.0 };
    let stale: Vec<_> = mats
        .iter()
        .filter(|(_, m)| m.params.grid.z != q)
        .map(|(id, _)| id)
        .collect();
    for id in stale {
        if let Some(mut m) = mats.get_mut(id) {
            m.params.grid.z = q;
        }
    }
}

/// `detailLevel` ultra: terrain quality 3 (module doc).
pub fn is_ultra(options: &gd_game_core::options::GameOptions) -> bool {
    matches!(options.get("detailLevel"), Some(gd_formats::options::Value::Quality(q)) if q == "ultra")
}

/// The terrain's blocks of 9×9 cells as the engine culls them, local AABBs
/// (min, max). Engine.dll `TerrainBase::ConstructBlocks`:
/// ⌈(w − 1) / 9⌉ × ⌈(h − 1) / 9⌉ blocks, block (i, j) spans
/// the vertices 9i ..= min(9i + 9, w − 1) (same in z); its box
/// (`TerrainBlock::ComputeBounds`) covers them in x / z and their height range in y.
/// `Terrain::AddToScene` adds the blocks in the view frustum as
/// one render batch whose box is their union
/// (`TerrainRenderInterfaceBase::AddToScene`): that, not the
/// level's whole terrain, is what the scene bounds see (render/shadow.rs
/// [`SceneBlocks`]).
pub fn terrain_blocks(t: &Terrain) -> SceneBlocks {
    let (w, h) = (t.width as usize, t.height as usize);
    let mut out = Vec::new();
    if w < 2 || h < 2 {
        return SceneBlocks(out);
    }
    for z0 in (0..h - 1).step_by(9) {
        let z1 = (z0 + 9).min(h - 1);
        for x0 in (0..w - 1).step_by(9) {
            let x1 = (x0 + 9).min(w - 1);
            let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let y = t.heights[z * w + x];
                    lo = lo.min(y);
                    hi = hi.max(y);
                }
            }
            out.push((
                Vec3::new(x0 as f32, lo, z0 as f32),
                Vec3::new(x1 as f32, hi, z1 as f32),
            ));
        }
    }
    SceneBlocks(out)
}

/// Everything built off the main thread for one level's terrain.
pub struct TerrainCpu {
    pub mesh: Mesh,
    pub blocks: SceneBlocks,
    /// The layers' grass blades (render/grass.rs).
    pub grass: super::grass::TerrainGrass,
    colors: Image,
    normals: Image,
    opacity: Image,
    params: TerrainParams,
    /// The layer textures it was built from, for the terrain entity to hold ([`HeldLayers`]).
    pub held: HeldLayers,
}

impl TerrainCpu {
    pub fn material(
        self,
        origin: Vec3,
        images: &mut Assets<Image>,
        defaults: &GdDefaults,
    ) -> (Mesh, GdTerrainMaterial) {
        let mut params = self.params;
        params.origin = origin.extend(params.origin.w);
        let m = GdTerrainMaterial {
            params,
            colors: images.add(self.colors),
            normals: images.add(self.normals),
            opacity: images.add(self.opacity),
            env: defaults.env.clone(),
            depth: defaults.depth.clone(),
        };
        (self.mesh, m)
    }
}

// `i` indexes the layer list, the layer table and the mask bit together. `high`: the terrain quality
// ([`super::grass::GrassQuality`]), for the grass blades.
#[allow(clippy::needless_range_loop)]
pub fn build(
    t: &Terrain,
    data: &GameData,
    cache: &TerrainLayers,
    high: bool,
) -> anyhow::Result<TerrainCpu> {
    let (w, h) = (t.width as usize, t.height as usize);
    let hgt = |x: usize, z: usize| t.heights[z.min(h - 1) * w + x.min(w - 1)];
    let mut positions = Vec::with_capacity(w * h);
    let mut normals = Vec::with_capacity(w * h);
    let mut uvs = Vec::with_capacity(w * h);
    for z in 0..h {
        for x in 0..w {
            positions.push([x as f32, hgt(x, z), z as f32]);
            // TerrainRT normal (`TerrainRT::ComputeNormal`): (h[x−1]−h[x+1], 2, h[z−1]−h[z+1]), doubled at edges.
            let mut dx = hgt(x.saturating_sub(1), z) - hgt(x + 1, z);
            if x == 0 || x == w - 1 {
                dx *= 2.0;
            }
            let mut dz = hgt(x, z.saturating_sub(1)) - hgt(x, z + 1);
            if z == 0 || z == h - 1 {
                dz *= 2.0;
            }
            normals.push(Vec3::new(dx, 2.0, dz).normalize().to_array());
            uvs.push([x as f32, z as f32]);
        }
    }
    // Terrain holes: a vertex is invisible when bit 0 of its first layer-mask
    // byte is clear (Engine.dll `TerrainRT::IsInvisible`); a cell
    // is left out when its own (lowest x, lowest z) corner is (the
    // TerrainRT index builders `TerrainRT::BuildIndices` / `TerrainRT::BuildBlockIndices` test only that
    // vertex), revealing water, caves and set pieces below.
    let bpv = t.layers.len() / 8 + 1;
    let visible = |x: usize, z: usize| {
        t.layer_mask
            .get((z * w + x) * bpv)
            .is_none_or(|m| m & 1 != 0)
    };
    let mut idx = Vec::with_capacity((w - 1) * (h - 1) * 6);
    for z in 0..h - 1 {
        for x in 0..w - 1 {
            if !visible(x, z) {
                continue;
            }
            let i = (z * w + x) as u32;
            let (a, b, c, d) = (i, i + 1, i + w as u32, i + w as u32 + 1);
            // Counter-clockwise seen from +Y.
            idx.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let usage = RenderAssetUsages::RENDER_WORLD;
    let mesh = Mesh::new(PrimitiveTopology::TriangleList, usage)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(idx));

    let n = t.layers.len().clamp(1, MAX_LAYERS);
    let (cw, ch) = (w - 1, h - 1);
    let stride = t.layer_mask.len() / (w * h).max(1);
    let mut colors = Vec::new();
    let mut nrms = Vec::new();
    let mut opacity = Vec::with_capacity(n * cw * ch * 2);
    let mut layers = [Vec4::ZERO; MAX_LAYERS];
    let mut held = Vec::with_capacity(n);
    for i in 0..n {
        let layer = t.layers.get(i);
        let tex = layer
            .map(|l| layer_textures(data, cache, &l.name))
            .unwrap_or_else(|| Arc::new(LayerTex::fallback()));
        colors.extend_from_slice(&tex.color);
        nrms.extend_from_slice(&tex.normal);
        layers[i] = Vec4::new(if tex.snow { 1.0 } else { 0.0 }, tex.spec_power, 0.0, 0.0);
        held.push(tex);
        let bit = i + 1;
        for z in 0..ch {
            for x in 0..cw {
                let wt = match layer.and_then(|l| l.weights.as_ref()) {
                    Some(wts) if i > 0 => wts.get(z * cw + x).copied().unwrap_or(0),
                    _ => 255,
                };
                let v = (z * w + x) * stride;
                let present = stride == 0
                    || (t.layer_mask.get(v).is_some_and(|m| m & 1 != 0)
                        && t.layer_mask
                            .get(v + bit / 8)
                            .is_some_and(|m| m & (1 << (bit % 8)) != 0));
                opacity.push(wt);
                opacity.push(if present { 255 } else { 0 });
            }
        }
    }
    let mips = LAYER_SIZE.ilog2() + 1;
    let repeat = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        anisotropy_clamp: 8,
        ..ImageSamplerDescriptor::linear()
    });
    let mut colors_img = array_image(
        LAYER_SIZE,
        LAYER_SIZE,
        n as u32,
        TextureFormat::Rgba8Unorm,
        colors,
        mips,
    );
    colors_img.sampler = repeat.clone();
    let mut normals_img = array_image(
        LAYER_SIZE,
        LAYER_SIZE,
        n as u32,
        TextureFormat::Rgba8Unorm,
        nrms,
        mips,
    );
    normals_img.sampler = repeat;
    let mut opacity_img = array_image(
        cw as u32,
        ch as u32,
        n as u32,
        TextureFormat::Rg8Unorm,
        opacity,
        1,
    );
    opacity_img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    Ok(TerrainCpu {
        mesh,
        blocks: terrain_blocks(t),
        grass: super::grass::build(t, data, high),
        colors: colors_img,
        normals: normals_img,
        opacity: opacity_img,
        params: TerrainParams {
            origin: Vec4::new(0.0, 0.0, 0.0, n as f32),
            grid: Vec4::new(cw as f32, ch as f32, 0.0, 0.0),
            layers,
        },
        held: HeldLayers { _layers: held },
    })
}

fn array_image(
    w: u32,
    h: u32,
    layers: u32,
    format: TextureFormat,
    data: Vec<u8>,
    mips: u32,
) -> Image {
    let mut img = crate::textures::raw_image(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: layers,
        },
        format,
        data,
    );
    img.data_order = TextureDataOrder::LayerMajor;
    img.texture_descriptor.mip_level_count = mips;
    img.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    img
}

/// `TerrainType`'s `specularPower` when the record has none (module doc).
const TERRAIN_SPECULAR_POWER: f32 = 10.0;

/// Decoded, resampled layer textures with full mip chains, shared by levels.
struct LayerTex {
    color: Vec<u8>,
    normal: Vec<u8>,
    snow: bool,
    spec_power: f32,
}

impl LayerTex {
    fn fallback() -> Self {
        Self {
            color: solid([255, 0, 255, 255]),
            normal: solid([128, 128, 255, 0]),
            snow: false,
            spec_power: TERRAIN_SPECULAR_POWER,
        }
    }
}

/// The decoded layer textures by `TerrainType` record, shared by the level builds (background tasks hold a clone)
/// and held weakly (`gd_game_core::weak_cache`): a loaded level's terrain holds its layers ([`HeldLayers`]), and the last
/// [`LAYERS_WARM`] layers used stay decoded after their last level unloads, so walking out of a level and back does
/// not decode, resample and mip them again. `model::drop_on_mod_change` clears it when the mounted mod changes.
///
/// PORT: the engine frees a released resource 5 s after its last use; this keeps a count-bounded recent set instead.
#[derive(Resource, Clone)]
pub struct TerrainLayers(Arc<Mutex<gd_game_core::weak_cache::WeakCache<LayerTex>>>);

/// How many terrain layers [`TerrainLayers`] keeps decoded past their last level (~2.8 MB each with mips: ~135 MB;
/// a level uses a handful, so this covers the levels around the player).
const LAYERS_WARM: usize = 48;

impl Default for TerrainLayers {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(
            gd_game_core::weak_cache::WeakCache::with_recent(LAYERS_WARM),
        )))
    }
}

impl TerrainLayers {
    /// Drops the cached layers (the mounted mod changed).
    pub fn clear(&self) {
        self.0.lock().clear();
    }

    /// Cached layers still in use (the memory audit).
    pub fn count(&self) -> usize {
        self.0.lock().live()
    }
}

/// On a level's terrain: the layer textures it was built from, kept decoded for the next level builds while the
/// level is loaded ([`TerrainLayers`]).
#[derive(Component, Default)]
pub struct HeldLayers {
    _layers: Vec<Arc<LayerTex>>,
}

fn load_tex(data: &GameData, path: &str) -> anyhow::Result<Vec<u8>> {
    let tex = Tex::parse_owned(data.vfs.read(path)?)?;
    let dds = &tex.frames[0];
    let rgba = dds.decode_rgba8(0)?;
    Ok(with_mips(
        resample(&rgba, dds.width, dds.height, LAYER_SIZE),
        LAYER_SIZE,
    ))
}

fn layer_textures(data: &GameData, cache: &TerrainLayers, record: &str) -> Arc<LayerTex> {
    let key = gd_formats::vfs::normalized(record);
    // Not locked while decoding: the level builds run in parallel.
    if let Some(Some(v)) = cache.0.lock().get_kept(&key) {
        return v;
    }
    let rec = data.db.get(&key);
    let s = |k: &str| rec.as_ref().and_then(|r| r.str(k).map(str::to_owned));
    let color = s("TextureFileName1")
        .ok_or_else(|| anyhow::anyhow!("no TextureFileName1"))
        .and_then(|p| load_tex(data, &p));
    let normal = s("BumpMapFileName1").map(|p| load_tex(data, &p));
    let v = Arc::new(LayerTex {
        color: color.unwrap_or_else(|e| {
            warn!("terrain layer {key}: {e:#}");
            solid([255, 0, 255, 255])
        }),
        normal: match normal {
            Some(Ok(n)) => n,
            _ => solid([128, 128, 255, 0]),
        },
        snow: s("ShaderFileName").is_some_and(|p| p.to_ascii_lowercase().contains("snow")),
        spec_power: rec
            .as_ref()
            .and_then(|r| r.f32("specularPower"))
            .unwrap_or(TERRAIN_SPECULAR_POWER)
            .max(1e-3),
    });
    let mut cache = cache.0.lock();
    // Another build may have decoded it meanwhile: share theirs.
    if let Some(Some(v)) = cache.get_kept(&key) {
        return v;
    }
    cache.insert_kept(&key, &v);
    v
}

fn solid(c: [u8; 4]) -> Vec<u8> {
    let base: Vec<u8> = std::iter::repeat_n(c, (LAYER_SIZE * LAYER_SIZE) as usize)
        .flatten()
        .collect();
    with_mips(base, LAYER_SIZE)
}

/// Bilinear resample of RGBA8 to `size`×`size`.
fn resample(src: &[u8], w: u32, h: u32, size: u32) -> Vec<u8> {
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        let fy = ((y as f32 + 0.5) * h as f32 / size as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (y0, ty) = (fy.floor() as u32, fy.fract());
        let y1 = (y0 + 1).min(h - 1);
        for x in 0..size {
            let fx = ((x as f32 + 0.5) * w as f32 / size as f32 - 0.5).clamp(0.0, (w - 1) as f32);
            let (x0, tx) = (fx.floor() as u32, fx.fract());
            let x1 = (x0 + 1).min(w - 1);
            for c in 0..4 {
                let p = |xx: u32, yy: u32| src[((yy * w + xx) * 4 + c) as usize] as f32;
                let a = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
                let b = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
                out[((y * size + x) * 4 + c) as usize] = (a * (1.0 - ty) + b * ty).round() as u8;
            }
        }
    }
    out
}

/// Appends a box-filtered mip chain (square power-of-two input).
fn with_mips(mut level: Vec<u8>, size: u32) -> Vec<u8> {
    let mut out = level.clone();
    let mut s = size;
    while s > 1 {
        let n = s / 2;
        let mut next = vec![0u8; (n * n * 4) as usize];
        for y in 0..n {
            for x in 0..n {
                for c in 0..4 {
                    let p = |xx: u32, yy: u32| level[((yy * s + xx) * 4 + c) as usize] as u32;
                    let sum = p(2 * x, 2 * y)
                        + p(2 * x + 1, 2 * y)
                        + p(2 * x, 2 * y + 1)
                        + p(2 * x + 1, 2 * y + 1);
                    next[((y * n + x) * 4 + c) as usize] = sum.div_ceil(4) as u8;
                }
            }
        }
        out.extend_from_slice(&next);
        level = next;
        s = n;
    }
    out
}
