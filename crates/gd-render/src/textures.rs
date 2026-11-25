//! `.tex` → Bevy `Image`, cached by path. BC blocks are uploaded as-is.
//!
//! The engine creates every texture UNORM and lights in gamma space; colour maps use the `_srgb` view. UI shaders
//! encode the decoded texel back to its stored value (`render/post.rs`, "UI").
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDataOrder, TextureDimension, TextureFormat, TextureViewDescriptor,
    TextureViewDimension,
};
use gd_formats::collections::HashMap;
use gd_formats::tex::{Dds, PixelFormat, Tex};
use gd_game_core::data::GameData;

/// The entries a [`WeakHandles`] holds before it first drops the dead ones.
const PRUNE_MIN: usize = 64;

/// Asset handles by normalized path, held weakly (as `AssetId`s): a hit recovers a strong handle while the asset
/// lives, so an asset goes when its last user drops it (PORT: no timed unload) — unless it is one of the last
/// [`gd_game_core::weak_cache::RECENT`] handed out. `X`: facts kept with the asset (a flipbook's frame count). A file
/// that failed to load stays failed until [`WeakHandles::clear`].
pub struct WeakHandles<A: Asset, X: Copy = ()> {
    map: HashMap<String, Option<(AssetId<A>, X)>>,
    /// The size at which the next insert drops the dead entries.
    prune_at: usize,
    recent: gd_game_core::weak_cache::Recent<Held<A>>,
}

/// A strong handle in a [`WeakHandles`]' recent set.
struct Held<A: Asset>(Handle<A>);

impl<A: Asset> Clone for Held<A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<A: Asset> gd_game_core::weak_cache::Shared for Held<A> {
    fn id(&self) -> usize {
        use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
        BuildHasherDefault::<DefaultHasher>::default().hash_one(self.0.id()) as usize
    }
}

impl<A: Asset, X: Copy> Default for WeakHandles<A, X> {
    fn default() -> Self {
        Self {
            map: HashMap::default(),
            prune_at: PRUNE_MIN,
            recent: Default::default(),
        }
    }
}

impl<A: Asset, X: Copy> WeakHandles<A, X> {
    /// The entries whose asset still exists.
    pub fn live(&self, assets: &Assets<A>) -> usize {
        self.map
            .values()
            .filter(|v| v.is_some_and(|(id, _)| assets.contains(id)))
            .count()
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.recent.clear();
        self.prune_at = PRUNE_MIN;
    }

    /// `Some(handle)` for a live entry (kept warm), `Some(None)` for a failed load, `None` when never loaded or gone.
    pub fn get(&mut self, key: &str, assets: &mut Assets<A>) -> Option<Option<(Handle<A>, X)>> {
        match *self.map.get(key)? {
            None => Some(None),
            Some((id, x)) => {
                let h = assets.get_strong_handle(id)?;
                self.recent.keep(&Held(h.clone()));
                Some(Some((h, x)))
            }
        }
    }

    /// Records the asset loaded for `key` (`None`: the load failed), reusing a dead entry's key; a loaded one is kept
    /// warm.
    pub fn insert(&mut self, key: &str, value: Option<(&Handle<A>, X)>, assets: &Assets<A>) {
        if let Some((h, _)) = value {
            self.recent.keep(&Held(h.clone()));
        }
        let weak = value.map(|(h, x)| (h.id(), x));
        if let Some(slot) = self.map.get_mut(key) {
            *slot = weak;
            return;
        }
        if self.map.len() >= self.prune_at {
            self.map
                .retain(|_, v| v.is_none_or(|(id, _)| assets.contains(id)));
            self.prune_at = (self.map.len() * 2).max(PRUNE_MIN);
        }
        self.map.insert(key.to_owned(), weak);
    }
}

/// How a [`Textures`] entry was decoded: data (UNORM), colour (sRGB), the desaturated UI copy, a cube map.
#[derive(Clone, Copy)]
enum Kind {
    Data,
    Color,
    Gray,
    Cube,
}

/// Loaded textures by path, one map per [`Kind`]; an image goes when nothing holds it ([`WeakHandles`]).
#[derive(Resource, Default)]
pub struct Textures {
    cache: [WeakHandles<Image>; 4],
    /// Textures decoded in the background ([`Textures::ready`], [`collect_texture_loads`]): key, sRGB, image.
    loading: Vec<bevy::tasks::Task<(String, bool, Option<Image>)>>,
    /// The (key, sRGB) pairs of `loading`, so a texture is decoded once.
    in_flight: gd_formats::collections::HashSet<(String, bool)>,
    /// Decodes just filed, held until their waiters take them ([`gd_game_core::weak_cache::Fresh`]).
    fresh: gd_game_core::weak_cache::Fresh<Handle<Image>>,
}

pub struct TexturePlugin;

impl Textures {
    /// Cached textures still loaded.
    pub fn count(&self, images: &Assets<Image>) -> usize {
        self.cache.iter().map(|c| c.live(images)).sum()
    }

    /// Drops every loaded texture and pending decode (the mounted mod changed).
    pub fn clear(&mut self) {
        self.cache.iter_mut().for_each(WeakHandles::clear);
        self.loading.clear();
        self.in_flight.clear();
        self.fresh.clear();
    }

    /// Whether [`Textures::get`] of `path` costs nothing (loaded, or failed before). Otherwise its decode starts in
    /// the background and this is false.
    pub fn ready(
        &mut self,
        data: &GameData,
        path: &str,
        srgb: bool,
        images: &mut Assets<Image>,
    ) -> bool {
        self.load(data, path, srgb, images).is_some()
    }

    /// [`Self::ready`] that also hands out a decoded texture's strong handle into `held`, so a caller waiting on
    /// several textures keeps the ones already in from being dropped before the last one arrives.
    pub fn ready_held(
        &mut self,
        data: &GameData,
        path: &str,
        srgb: bool,
        images: &mut Assets<Image>,
        held: &mut Vec<Handle<Image>>,
    ) -> bool {
        match self.load(data, path, srgb, images) {
            Some(h) => {
                held.extend(h);
                true
            }
            None => false,
        }
    }

    /// The texture once decoded (`Some(None)`: it failed), else `None` with its background decode started.
    fn load(
        &mut self,
        data: &GameData,
        path: &str,
        srgb: bool,
        images: &mut Assets<Image>,
    ) -> Option<Option<Handle<Image>>> {
        let kind = if srgb { Kind::Color } else { Kind::Data };
        let key = gd_formats::vfs::normalized(path);
        if let Some(h) = self.hit(kind, &key, images) {
            return Some(h);
        }
        if self.in_flight.insert((key.to_string(), srgb)) {
            let (key, data) = (key.into_owned(), data.clone());
            self.loading
                .push(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                    let img = decode(&data, &key, srgb)
                        .inspect_err(|e| warn!("texture {key}: {e:#}"))
                        .ok();
                    (key, srgb, img)
                }));
        }
        None
    }

    /// The cached handle of `key`, if it is loaded (or failed before).
    fn hit(
        &mut self,
        kind: Kind,
        key: &str,
        images: &mut Assets<Image>,
    ) -> Option<Option<Handle<Image>>> {
        self.cache[kind as usize]
            .get(key, images)
            .map(|h| h.map(|(h, ())| h))
    }

    fn insert(&mut self, kind: Kind, key: &str, h: Option<&Handle<Image>>, images: &Assets<Image>) {
        self.cache[kind as usize].insert(key, h.map(|h| (h, ())), images);
    }

    /// The first frame desaturated to its luma (alpha kept): the UI's
    /// `desaturate` style (PORT: Rec. 601 luma in gamma space).
    pub fn get_gray(
        &mut self,
        data: &GameData,
        path: &str,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        let key = gd_formats::vfs::normalized(path);
        if let Some(h) = self.hit(Kind::Gray, &key, images) {
            return h;
        }
        let h = data
            .vfs
            .read(&key)
            .ok()
            .and_then(|b| Tex::parse_owned(b).ok())
            .and_then(|t| {
                let f = t.frames.first()?;
                let mut px = f.decode_rgba8(0).ok()?;
                for p in px.as_chunks_mut::<4>().0 {
                    let l = (0.299f32 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
                        .round()
                        .min(255.0) as u8;
                    p[..3].fill(l);
                }
                Some(images.add(Image::new(
                    Extent3d {
                        width: f.width,
                        height: f.height,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    px,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                )))
            });
        self.insert(Kind::Gray, &key, h.as_ref(), images);
        h
    }
}

impl Plugin for TexturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Textures>()
            .add_systems(First, collect_texture_loads);
    }
}

/// Files the textures [`Textures::ready`] decoded in the background, kept warm until their users take them.
fn collect_texture_loads(mut textures: ResMut<Textures>, mut images: ResMut<Assets<Image>>) {
    use bevy::tasks::{block_on, futures_lite::future};
    textures.fresh.tick();
    if textures.loading.is_empty() {
        return;
    }
    let textures = &mut *textures;
    let deadline = std::time::Instant::now() + crate::model::FINISH_BUDGET;
    let mut done = 0;
    let mut i = 0;
    while i < textures.loading.len() {
        // The rest stay queued (finished tasks keep their result) for the next frame.
        if done > 0 && std::time::Instant::now() >= deadline {
            break;
        }
        let Some((key, srgb, img)) = block_on(future::poll_once(&mut textures.loading[i])) else {
            i += 1;
            continue;
        };
        drop(textures.loading.swap_remove(i));
        let in_flight = (key, srgb);
        textures.in_flight.remove(&in_flight);
        let (key, srgb) = in_flight;
        done += 1;
        textures.put(&key, srgb, img, &mut images);
        let kind = if srgb { Kind::Color } else { Kind::Data };
        if let Some(Some(h)) = textures.hit(kind, &key, &mut images) {
            textures.fresh.hold(h);
        }
    }
}

/// Decodes the texture at `key` (normalized; first flipbook frame), on any thread.
pub fn decode(data: &GameData, key: &str, srgb: bool) -> anyhow::Result<Image> {
    data.vfs
        .read(key)
        .and_then(Tex::parse_owned)
        .and_then(|t| dds_to_image(&t.frames[0], srgb))
}

impl Textures {
    /// Loads a texture (first flipbook frame). `srgb` selects colour vs data.
    pub fn get(
        &mut self,
        data: &GameData,
        path: &str,
        srgb: bool,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        let key = gd_formats::vfs::normalized(path);
        let kind = if srgb { Kind::Color } else { Kind::Data };
        if let Some(h) = self.hit(kind, &key, images) {
            return h;
        }
        let img = decode(data, &key, srgb).inspect_err(|e| warn!("texture {key}: {e:#}"));
        let h = img.ok().map(|img| images.add(img));
        self.insert(kind, &key, h.as_ref(), images);
        h
    }

    /// Whether [`Textures::get`] of `path` would not decode: the texture is loaded, or failed before.
    pub fn cached(&mut self, path: &str, srgb: bool, images: &mut Assets<Image>) -> bool {
        let kind = if srgb { Kind::Color } else { Kind::Data };
        self.hit(kind, &gd_formats::vfs::normalized(path), images)
            .is_some()
    }

    /// Files an image decoded elsewhere ([`decode`], `None`: it failed) for `key` (normalized), unless it is loaded
    /// already.
    pub fn put(&mut self, key: &str, srgb: bool, img: Option<Image>, images: &mut Assets<Image>) {
        let kind = if srgb { Kind::Color } else { Kind::Data };
        if self.hit(kind, key, images).is_some() {
            return;
        }
        let h = img.map(|img| images.add(img));
        self.insert(kind, key, h.as_ref(), images);
    }
}

impl Textures {
    /// A cube map (`.tex` with 6 faces, D3D face order +X −X +Y −Y +Z −Z, the
    /// same as wgpu's), every stored mip, UNORM; `None` if it is not a cube.
    pub fn get_cube(
        &mut self,
        data: &GameData,
        path: &str,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        let key = gd_formats::vfs::normalized(path);
        if let Some(h) = self.hit(Kind::Cube, &key, images) {
            return h;
        }
        let h = data
            .vfs
            .read(&key)
            .ok()
            .and_then(|b| Tex::parse_owned(b).ok())
            .and_then(|t| {
                let dds = t.frames.into_iter().next()?;
                let format = match dds.format {
                    PixelFormat::Bc1 => TextureFormat::Bc1RgbaUnorm,
                    PixelFormat::Bc2 => TextureFormat::Bc2RgbaUnorm,
                    PixelFormat::Bc3 => TextureFormat::Bc3RgbaUnorm,
                    PixelFormat::Bgra8 | PixelFormat::Bgrx8 => TextureFormat::Bgra8Unorm,
                    _ => return None,
                };
                if dds.faces != 6 || dds.width % 4 != 0 {
                    return None;
                }
                let mut img = raw_image(
                    Extent3d {
                        width: dds.width,
                        height: dds.height,
                        depth_or_array_layers: 6,
                    },
                    format,
                    dds.data,
                );
                img.data_order = TextureDataOrder::LayerMajor;
                img.texture_descriptor.mip_level_count = dds.mip_levels;
                img.texture_view_descriptor = Some(TextureViewDescriptor {
                    dimension: Some(TextureViewDimension::Cube),
                    ..default()
                });
                img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
                Some(images.add(img))
            });
        if h.is_none() {
            warn!("cube texture {path}: not a usable cube map");
        }
        self.insert(Kind::Cube, &key, h.as_ref(), images);
        h
    }
}

/// The `anisotropicFiltering` option (`GraphicsEngine::UpdateFromOptions`: the literal level, 1 to 16). PORT: read at
/// load, so textures already loaded keep theirs.
static ANISOTROPY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(8);

pub fn anisotropy() -> u32 {
    ANISOTROPY.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_anisotropy(level: i32) {
    ANISOTROPY.store(
        level.clamp(1, 16) as u32,
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// The `textureQuality` option (`GraphicsEngine::UpdateFromOptions`: `GraphicsEngine.textureReduction`): the number of
/// top mips a mipmapped texture skips. PORT: read at load, so textures already loaded keep theirs (the engine unloads
/// all resources on a change); the per-directory reduction (`SetDirectoryTextureReduction`) is not ported.
static TEXTURE_REDUCTION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Mips dropped for a `textureQuality` text (UpdateFromOptions: low 2, medium 1, high and above 0; the option
/// parser's "anything else is medium" applies).
pub fn texture_reduction(quality: &str) -> u32 {
    match quality {
        "low" => 2,
        "high" | "very high" | "ultra" => 0,
        _ => 1,
    }
}

pub fn set_texture_quality(quality: &str) {
    TEXTURE_REDUCTION.store(
        texture_reduction(quality),
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// Top mips to skip for a texture of `mips` levels: the global reduction, never past the last level (a texture without
/// a mip chain keeps its only level), and not when the reduced size would break the 4 x 4 block grid of BC formats.
fn skipped_mips(width: u32, height: u32, mips: u32, reduction: u32, block: bool) -> u32 {
    let mut skip = reduction.min(mips.saturating_sub(1));
    while skip > 0 && block && ((width >> skip) % 4 != 0 || (height >> skip) % 4 != 0) {
        skip -= 1;
    }
    skip
}

pub fn dds_to_image(dds: &Dds, srgb: bool) -> anyhow::Result<Image> {
    let bc_ok = dds.width % 4 == 0 && dds.height % 4 == 0;
    // Only the formats uploaded as they are skip mips; the CPU-decoded ones keep their top level.
    let is_bc = matches!(
        dds.format,
        PixelFormat::Bc1 | PixelFormat::Bc2 | PixelFormat::Bc3
    );
    let raw = (is_bc && bc_ok) || matches!(dds.format, PixelFormat::Bgra8 | PixelFormat::Bgrx8);
    let skip = if raw {
        skipped_mips(
            dds.width,
            dds.height,
            dds.mip_levels,
            TEXTURE_REDUCTION.load(std::sync::atomic::Ordering::Relaxed),
            is_bc,
        )
    } else {
        0
    };
    let (width, height) = dds.level_dims(skip);
    let (format, bytes, mips) = match dds.format {
        PixelFormat::Bc1 if bc_ok => (
            if srgb {
                TextureFormat::Bc1RgbaUnormSrgb
            } else {
                TextureFormat::Bc1RgbaUnorm
            },
            face0(dds, skip),
            dds.mip_levels - skip,
        ),
        PixelFormat::Bc2 if bc_ok => (
            if srgb {
                TextureFormat::Bc2RgbaUnormSrgb
            } else {
                TextureFormat::Bc2RgbaUnorm
            },
            face0(dds, skip),
            dds.mip_levels - skip,
        ),
        PixelFormat::Bc3 if bc_ok => (
            if srgb {
                TextureFormat::Bc3RgbaUnormSrgb
            } else {
                TextureFormat::Bc3RgbaUnorm
            },
            face0(dds, skip),
            dds.mip_levels - skip,
        ),
        PixelFormat::Bgra8 | PixelFormat::Bgrx8 => (
            if srgb {
                TextureFormat::Bgra8UnormSrgb
            } else {
                TextureFormat::Bgra8Unorm
            },
            face0(dds, skip),
            dds.mip_levels - skip,
        ),
        _ => {
            // Odd-sized BC, SNORM bump maps, A8: decode the top level on the CPU.
            let rgba = dds.decode_rgba8(0)?;
            (
                if srgb {
                    TextureFormat::Rgba8UnormSrgb
                } else {
                    TextureFormat::Rgba8Unorm
                },
                rgba,
                1,
            )
        }
    };
    let mut img = raw_image(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        format,
        bytes,
    );
    img.data_order = TextureDataOrder::LayerMajor;
    img.texture_descriptor.mip_level_count = mips;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        anisotropy_clamp: if mips > 1 {
            ANISOTROPY.load(std::sync::atomic::Ordering::Relaxed) as u16
        } else {
            1
        },
        ..ImageSamplerDescriptor::linear()
    });
    Ok(img)
}

/// A 2D (array) image over pre-laid-out bytes (mips included), bypassing
/// `Image::new`'s single-level size check.
pub fn raw_image(size: Extent3d, format: TextureFormat, data: Vec<u8>) -> Image {
    let mut img = Image::default();
    img.texture_descriptor.size = size;
    img.texture_descriptor.format = format;
    img.texture_descriptor.dimension = TextureDimension::D2;
    img.asset_usage = RenderAssetUsages::RENDER_WORLD;
    img.data = Some(data);
    img
}

/// Mips `skip..` of face 0, largest first.
fn face0(dds: &Dds, skip: u32) -> Vec<u8> {
    let start = dds.level_range(0, skip).start;
    let end = dds.level_range(0, dds.mip_levels - 1).end;
    dds.data[start..end].to_vec()
}
