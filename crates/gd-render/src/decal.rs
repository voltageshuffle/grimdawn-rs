//! Ground decals (`GAME::Decal`, Engine.dll): level-placed `Decal` entities
//! (`world.rs`) and effect decals (an `EffectEntity`'s `decal`, `casting.rs`
//! `fx_decals`) share this record parsing, geometry, material
//! (`shaders/decal.wesl`: `decal_standard` / `decal_glow` / `decal_bump` /
//! `decal_selfillum`, UNORM textures) and opacity.
//!
//! Geometry (`Decal::CreateGeometry` @0x1800593d0): the triangles of
//! `World::GeometryBusStop` in the box centred on the decal with half-extent
//! max(width, height) × 0.5, × 0.7 for a *dynamic* decal (`Decal+499`, set
//! by `EffectEntity::Load` on its decal; level decals are static). Terrain
//! (`Terrain::GeometryBusStop` @0x1801c6040): every heightfield triangle whose
//! AABB overlaps the box, vertices at height + 0.01 with the terrain normal,
//! whatever the slope.
//!
//! Meshes (`World::GeometryBusStop` @0x1802192f0, skipped with
//! `terrainOnly`): every entity of `GetEntitiesInBox` that is an `Actor`
//! (`Actor::GeometryBusStop` @0x18002ce50) with `IsStatic()` (vt +352; among
//! the level classes only Game.dll `Decoration::IsStatic` @0x1801fb610 =
//! `!allowPathing` is ever true, `Destructible` / `FixedItem` return false)
//! and `OccludesPathing()` (vt +224, true for an `Actor`) gives its mesh to
//! `GraphicsMesh::GeometryBusStop` @0x1800ba220. That walks the mesh's system
//! copy (+584: 56-byte position / uv / normal records, +592: the u16 faces of
//! chunk 5, every face set and material; `Initialize` keeps it when the bone
//! count +144 is still 0 as chunks 4 / 5 are read, and chunk 6 follows them
//! in every stock file, so every mesh has one). A vertex goes to the world as
//! coords × (scale ⊙ p) + 0.001 × its rotated normal; a face is kept when its
//! world AABB overlaps the box and the y of its normal ‖(v2 − v0) × (v1 − v0)‖
//! (the front side of a clockwise face) is ≥ sin(π/2 − `maxAngle`) (`Load`
//! stores π/2 − maxAngle and passes its sine): one-sided, so a face seen from
//! below is dropped, and `maxAngle` only culls mesh faces. No clipping: whole
//! faces go in, the shader discards outside the UV square. The port keeps
//! those meshes per level in [`DecalReceivers`]. When: a placed
//! decal builds its geometry in `Decal::InitialUpdate` @0x180059230, so with
//! what is loaded then; again only on `OnMoveInLevel`, the exported (and
//! uncalled) `RebuildMesh`, and after a device reset (`PostDeviceReset` sets
//! the flag `RenderPass` rebuilds on). A level loading later does not
//! re-project its neighbours' decals.
//!
//! Every vertex gets colour 0xFFFFFFFF: there is no per-vertex fade
//! (the `selfillum` "texel × colour" is × 1). The frame and UVs: see
//! [`decal_axes`]; N sums the vertex normals (a dynamic decal only its
//! terrain vertices'). UV u is clamped into the variant strip per vertex;
//! the port discards outside [0, 1] in the shader and remaps there instead.
//! PORT: terrain vertices are lifted 0.03 m (the engine 0.01) and the
//! material has a depth bias, against z-fighting.
//! The base layer is blended before SSAO, as into the engine's G-buffer
//! (render/gbuffer_blend.rs); the additive glow layer later, unoccluded.
//!
//! Opacity (`Decal::GetOpacity` @0x180058ee0, times in integer ms,
//! the age advanced by `UpdateSelf` @0x180059030): see [`DecalTiming::opacity`];
//! the shader's `opacity` scales the texel alpha (the glow layer's colour).
//! A decal with `life` > 0 is removed when its age reaches it, level (static)
//! decals too. `UpdateSelf`: while updating (+497, set by `Load`),
//! at age ≥ life the decal stops updating and, in the world, is removed and
//! destroyed unless `Engine::IsEditorMode` (`gEngine` +658) holds and it is
//! not dynamic (+499, `SetDynamic`; `EffectEntity::Load` sets it): only the
//! editor loops a static decal's age back to 0.
//!
//! Effect decals (`EffectEntity::Load` @0x180064000 /
//! `InitialUpdate` @0x180065460): with chance `decalChance` % (0 = 100,
//! `rand() % 101 ≤ chance`) one of the `decal` list at random, attached at the
//! identity offset, so its frame is the effect's coords, yawed at random
//! (`rand() × 2π / 32767` about the up axis) when `decalRotate`. The Decal
//! record's own `rotate` is only used by `Decal::AddToWorld` (not on this path).
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::math::Affine3A;
use bevy::mesh::{Indices, MeshTag, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use std::collections::HashMap;
use std::sync::Arc;

pub type DecalMaterial = ExtendedMaterial<StandardMaterial, DecalExt>;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct DecalExt {
    #[uniform(100)]
    pub params: DecalParams,
    #[texture(101)]
    #[sampler(102)]
    pub texture: Handle<Image>,
    /// The shared lighting environment (`render::GdDefaults::env`).
    #[storage(103, read_only)]
    pub env: Handle<bevy::render::storage::ShaderBuffer>,
    /// `decal_bump.ssh`'s `bumpTexture` (normal rgb, specular a); unused
    /// (any texture) unless [`DecalParams::bump`].
    #[texture(104)]
    #[sampler(105)]
    pub bump: Handle<Image>,
}

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct DecalParams {
    pub variant_offset: f32,
    pub variant_scale: f32,
    pub opacity: f32,
    /// 0: the lit base layer; > 0: the additive `decal_glow.ssh` layer
    /// (`bumpTexture` rgb × alpha × this gain).
    pub glow: f32,
    /// 1: `decal_bump.ssh` (normal + specular from [`DecalExt::bump`]).
    pub bump: f32,
    /// 1: `decal_selfillum.ssh` (emissive: texture × vertex colour, unlit).
    pub unlit: f32,
    pub _pad0: f32,
    pub _pad1: f32,
}

impl MaterialExtension for DecalExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://gd_render/shaders/decal.wesl".into()
    }
}

/// `decal_glow.ssh` (the burning coals under fires, Flash Freeze's frost,
/// Devastation / devotion cracks): deferredPS writes `bumpTexture`
/// rgb × its alpha × 4 into the G-buffer glow target (MRT 4) with the base
/// texel's alpha × opacity as the blend alpha (SrcAlpha / OneMinusSrcAlpha),
/// so like every glow it is added to the frame unblurred and through
/// `RenderSceneGlow`'s blur. Here the additive layer is drawn in both the
/// main and the glow view (`render::post`).
pub const DECAL_GLOW_GAIN: f32 = 4.0;

pub struct DecalPlugin;

impl Plugin for DecalPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/decal.wesl");
        app.add_plugins(MaterialPlugin::<DecalMaterial>::default())
            .init_resource::<DecalReceivers>()
            .add_systems(Update, (fade_decals, gate_level_decals, grow_decals));
    }
}

/// A decal placed by the level (`Decal` entity, `world.rs`), as opposed to an effect's.
#[derive(Component)]
pub struct LevelDecal;

/// `detailObjects` (bool 9) gates `DecalSet::AddToScene` (Engine.dll
/// @0x18005C0D0): with it off no decal is drawn.
/// PORT: the decals stay spawned and are hidden, so the option applies live
/// like the engine's per-frame `UpdateFromOptions`. All decals are walked when
/// the option flips (`last`: the value applied), else only new ones.
fn gate_level_decals(
    options: Option<Res<gd_game::options::GameOptions>>,
    mut last: Local<Option<bool>>,
    mut q: ParamSet<(
        Query<&mut Visibility, With<LevelDecal>>,
        Query<&mut Visibility, Added<LevelDecal>>,
    )>,
) {
    let on = options.is_none_or(|o| o.flag("detailObjects"));
    let all = *last != Some(on);
    *last = Some(on);
    let want = if on {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if all {
        for mut v in &mut q.p0() {
            v.set_if_neq(want);
        }
    } else {
        for mut v in &mut q.p1() {
            v.set_if_neq(want);
        }
    }
}

/// `Decal::Load`'s timing fields (s in the record, ms here).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DecalTiming {
    pub min: f32,
    pub max: f32,
    pub life_ms: i32,
    pub period_ms: i32,
    pub fade_in_ms: i32,
    pub fade_after_ms: i32,
}

impl DecalTiming {
    /// `Decal::GetOpacity` @0x180058ee0. With a `period`: a
    /// triangle wave min → max → min over the period (× (1 − age / life) with
    /// a life). Else max × age / fadeIn while fading in, max until
    /// `fadeAfter`, then max × (1 − (age − fadeAfter) / (life − fadeAfter))
    /// with a life; both ramps floored at `minOpacity`.
    pub fn opacity(&self, age_ms: i32) -> f32 {
        let (min, max) = (self.min, self.max);
        if self.period_ms != 0 {
            let p = self.period_ms;
            let half = p / 2;
            let t = age_ms % p;
            let tri = if t <= half {
                t as f32 / half as f32
            } else {
                1.0 - (t - half) as f32 / half as f32
            };
            let o = (max - min) * tri + min;
            return if self.life_ms != 0 {
                (1.0 - age_ms as f32 / self.life_ms as f32) * o
            } else {
                o
            };
        }
        if age_ms < self.fade_in_ms {
            return (age_ms as f32 / self.fade_in_ms as f32 * max).max(min);
        }
        if age_ms > self.fade_after_ms && self.life_ms != 0 {
            return ((1.0
                - (age_ms - self.fade_after_ms) as f32
                    / (self.life_ms - self.fade_after_ms) as f32)
                * max)
                .max(min);
        }
        max
    }

    /// Whether the opacity ever changes after it is first drawn.
    pub fn animated(&self) -> bool {
        self.period_ms != 0 || self.fade_in_ms > 0 || self.life_ms != 0
    }
}

/// A `Decal` record (`Decal::Load` @0x180058a30).
#[derive(Clone, Debug)]
pub struct DecalRecord {
    pub texture: String,
    /// `decal_bump.ssh`: its `bumpTexture` (normal map + specular).
    pub bump: Option<String>,
    /// `decal_glow.ssh`: its `bumpTexture`, added on top ([`DECAL_GLOW_GAIN`]).
    pub glow: Option<String>,
    pub unlit: bool,
    pub width: f32,
    pub height: f32,
    pub variant_offset: f32,
    pub variant_scale: f32,
    pub timing: DecalTiming,
    pub terrain_only: bool,
    /// cos(`maxAngle`): mesh faces flatter than this are kept.
    pub min_normal_y: f32,
}

impl DecalRecord {
    pub fn from_record(rec: &gd_formats::arz::Record) -> Option<Self> {
        let texture = rec.str("texture")?.to_owned();
        let shader = rec.str("shader").unwrap_or_default().to_ascii_lowercase();
        let bump_tex = rec
            .str("bumpTexture")
            .filter(|t| !t.is_empty())
            .map(str::to_owned);
        let variants = rec.f32("variantsOnTexture").unwrap_or(1.0).max(1.0);
        let index = rec.f32("variantIndex").unwrap_or(1.0).clamp(1.0, variants);
        let ms = |k: &str| (rec.f32(k).unwrap_or(0.0) * 1000.0) as i32;
        Some(Self {
            texture,
            bump: bump_tex.clone().filter(|_| shader.contains("decal_bump")),
            glow: bump_tex.filter(|_| shader.contains("decal_glow")),
            unlit: shader.contains("decal_selfillum"),
            width: rec.f32("width").unwrap_or(4.0).max(0.1),
            height: rec.f32("height").unwrap_or(4.0).max(0.1),
            variant_offset: (index - 1.0) / variants,
            variant_scale: 1.0 / variants,
            timing: DecalTiming {
                min: rec.f32("minOpacity").unwrap_or(0.0),
                max: rec.f32("maxOpacity").unwrap_or(1.0),
                life_ms: ms("life"),
                period_ms: ms("period"),
                fade_in_ms: ms("fadeIn"),
                fade_after_ms: ms("fadeAfter"),
            },
            terrain_only: rec.i32("terrainOnly").unwrap_or(0) != 0,
            min_normal_y: (std::f32::consts::FRAC_PI_2
                - rec.f32("maxAngle").unwrap_or(0.0).to_radians())
            .sin(),
        })
    }

    pub fn params(&self, opacity: f32) -> DecalParams {
        DecalParams {
            variant_offset: self.variant_offset,
            variant_scale: self.variant_scale,
            opacity,
            glow: 0.0,
            bump: 0.0,
            unlit: if self.unlit { 1.0 } else { 0.0 },
            _pad0: 0.0,
            _pad1: 0.0,
        }
    }
}

/// (U, V) of `Decal::CreateGeometry` for the decal's local Z axis `f`
/// (matrix row 2, `WorldCoords` +48) and the summed vertex normal `n_sum`:
/// A = ‖Y × F‖, B = A × Y (F flattened); N = ‖n_sum‖ (Y if < 0.1),
/// C = ‖N × X‖, D = ‖N × C‖; U = C.x·A + C.y·Y + C.z·B,
/// V = D.x·A + D.y·Y + D.z·B and u = ½ − (p−c)·U / width,
/// v = (p−c)·V / height + ½. On flat ground U = −B, V = −A: `width` runs
/// along the local Z axis, `height` along local X.
pub fn decal_axes(f: Vec3, n_sum: Vec3) -> (Vec3, Vec3) {
    let a = Vec3::Y.cross(f).normalize_or(Vec3::X);
    let b = a.cross(Vec3::Y).normalize_or(Vec3::Z);
    let n = if n_sum.length() < 0.1 {
        Vec3::Y
    } else {
        n_sum.normalize()
    };
    let c = n.cross(Vec3::X).normalize_or(Vec3::NEG_Z);
    let d = n.cross(c).normalize_or(Vec3::NEG_X);
    (
        c.x * a + c.y * Vec3::Y + c.z * b,
        d.x * a + d.y * Vec3::Y + d.z * b,
    )
}

/// A height grid to gather decal triangles from: vertex (x, z) of the grid
/// is at `origin + (x, h(x, z), z)`.
pub enum Surface<'a> {
    /// A level's terrain heightfield (`Terrain::GeometryBusStop`).
    Terrain {
        origin: Vec3,
        height: &'a dyn Fn(i32, i32) -> Option<f32>,
    },
}

/// A mesh's decal faces (`GraphicsMesh` +584 / +592), object space.
pub struct ReceiverMesh {
    pos: Vec<Vec3>,
    normals: Vec<Vec3>,
    tris: Vec<[u16; 3]>,
}

impl ReceiverMesh {
    /// Chunk 4 positions / normals and every chunk 5 face.
    pub fn from_msh(m: &gd_formats::msh::Mesh) -> Option<Self> {
        let v = &m.vertices;
        if m.indices.len() < 3 || v.positions.is_empty() {
            return None;
        }
        let n = v.positions.len();
        Some(Self {
            pos: v.positions.iter().map(|p| Vec3::from(*p)).collect(),
            normals: (0..n)
                .map(|i| v.normals.get(i).map_or(Vec3::Y, |n| Vec3::from(*n)))
                .collect(),
            tris: m
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter(|t| t.iter().all(|&i| (i as usize) < n))
                .copied()
                .collect(),
        })
    }
}

/// A placed receiver: its mesh, placement and world AABB.
struct Receiver {
    mesh: Arc<ReceiverMesh>,
    /// Coords × scale (positions).
    tf: Affine3A,
    /// The coords' rotation alone (normals, `GraphicsMesh::GeometryBusStop`).
    rot: Quat,
    min: Vec3,
    max: Vec3,
}

/// The meshes decals project onto, per level: the static `Decoration`s
/// (`World::GeometryBusStop`, module doc).
#[derive(Resource, Default)]
pub struct DecalReceivers {
    levels: HashMap<usize, Vec<Receiver>>,
    meshes: HashMap<String, Option<Arc<ReceiverMesh>>>,
}

/// Whether a scenery record's actor takes decals: `IsStatic()` (only a
/// `Decoration` without `allowPathing`) and `OccludesPathing()` (module doc).
pub fn receives_decals(class: &str, allow_pathing: bool) -> bool {
    class == "Decoration" && !allow_pathing
}

impl DecalReceivers {
    /// Registers a level's receivers: (mesh path, mesh, placement).
    pub fn add_level<'a>(
        &mut self,
        index: usize,
        objects: impl Iterator<Item = (&'a str, &'a gd_formats::msh::Mesh, Affine3A)>,
    ) {
        let mut list = Vec::new();
        for (path, msh, tf) in objects {
            let Some(mesh) = self
                .meshes
                .entry(path.to_owned())
                .or_insert_with(|| ReceiverMesh::from_msh(msh).map(Arc::new))
                .clone()
            else {
                continue;
            };
            list.push(Receiver::new(mesh, tf));
        }
        self.levels.insert(index, list);
    }

    pub fn remove_level(&mut self, index: usize) {
        self.levels.remove(&index);
    }

    /// Adds the faces of every receiver in the box (centre `c`,
    /// half-extent `e`) to `patch`.
    pub fn gather(
        &self,
        patch: &mut DecalPatch,
        c: Vec3,
        e: f32,
        dec: &DecalRecord,
        dynamic: bool,
    ) {
        let (lo, hi) = (c - Vec3::splat(e), c + Vec3::splat(e));
        for r in self.levels.values().flatten() {
            if (r.min - hi).max_element() <= 0.0 && (lo - r.max).max_element() <= 0.0 {
                patch.gather_mesh(&r.mesh, r.tf, r.rot, c, e, dec.min_normal_y, dynamic);
            }
        }
    }
}

impl Receiver {
    fn new(mesh: Arc<ReceiverMesh>, tf: Affine3A) -> Self {
        let (min, max) = mesh.pos.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(a, b), p| {
                let w = tf.transform_point3(*p);
                (a.min(w), b.max(w))
            },
        );
        let rot = tf.to_scale_rotation_translation().1;
        Self {
            mesh,
            tf,
            rot,
            min,
            max,
        }
    }
}

#[derive(Default)]
pub struct DecalPatch {
    pos: Vec<Vec3>,
    normals: Vec<Vec3>,
    /// Counted in N ([`DecalPatch::build`]).
    in_sum: Vec<bool>,
    idx: Vec<u32>,
}

impl DecalPatch {
    /// Adds the grid triangles of `s` whose AABB overlaps the box (centre `c`,
    /// half-extent `e`).
    pub fn gather(&mut self, s: &Surface, c: Vec3, e: f32) {
        let Surface::Terrain { origin, height } = s;
        let (origin, height) = (*origin, *height);
        let lift = 0.03;
        let l = c - origin;
        let (x0, x1) = ((l.x - e).floor() as i32, (l.x + e).ceil() as i32);
        let (z0, z1) = ((l.z - e).floor() as i32, (l.z + e).ceil() as i32);
        let p = |x: i32, z: i32| height(x, z).map(|y| origin + Vec3::new(x as f32, y, z as f32));
        let normal = |x: i32, z: i32, y: f32| {
            let h = |dx: i32, dz: i32| height(x + dx, z + dz).unwrap_or(y);
            Vec3::new(h(-1, 0) - h(1, 0), 2.0, h(0, -1) - h(0, 1)).normalize()
        };
        let mut map: HashMap<(i32, i32), u32> = HashMap::new();
        for z in z0..z1 {
            for x in x0..x1 {
                for tri in [
                    [(x, z), (x, z + 1), (x + 1, z)],
                    [(x + 1, z), (x, z + 1), (x + 1, z + 1)],
                ] {
                    let Some(v) = tri
                        .iter()
                        .map(|&(x, z)| p(x, z))
                        .collect::<Option<Vec<Vec3>>>()
                    else {
                        continue;
                    };
                    let (lo, hi) = (v[0].min(v[1]).min(v[2]), v[0].max(v[1]).max(v[2]));
                    if !box_overlaps(lo, hi, c, e) {
                        continue;
                    }
                    for (k, &(gx, gz)) in tri.iter().enumerate() {
                        let i = *map.entry((gx, gz)).or_insert_with(|| {
                            self.pos.push(v[k] + Vec3::Y * lift);
                            self.normals.push(normal(gx, gz, v[k].y - origin.y));
                            self.in_sum.push(true);
                            (self.pos.len() - 1) as u32
                        });
                        self.idx.push(i);
                    }
                }
            }
        }
    }

    /// `GraphicsMesh::GeometryBusStop`: the faces of `mesh` placed
    /// at `tf` (normals rotated by `rot`) whose world AABB overlaps the box
    /// and whose front normal has y ≥ `min_normal_y`; vertices pushed out
    /// 0.001 along their normal. A dynamic decal leaves them out of N.
    #[allow(clippy::too_many_arguments)]
    pub fn gather_mesh(
        &mut self,
        mesh: &ReceiverMesh,
        tf: Affine3A,
        rot: Quat,
        c: Vec3,
        e: f32,
        min_normal_y: f32,
        dynamic: bool,
    ) {
        let world: Vec<(Vec3, Vec3)> = mesh
            .pos
            .iter()
            .zip(&mesh.normals)
            .map(|(p, n)| {
                let n = rot * *n;
                (tf.transform_point3(*p) + n * 0.001, n)
            })
            .collect();
        let mut map: HashMap<u16, u32> = HashMap::new();
        for t in &mesh.tris {
            let v = t.map(|i| world[i as usize].0);
            let (lo, hi) = (v[0].min(v[1]).min(v[2]), v[0].max(v[1]).max(v[2]));
            if !box_overlaps(lo, hi, c, e) {
                continue;
            }
            if face_normal(v).y < min_normal_y {
                continue;
            }
            for &i in t {
                let k = *map.entry(i).or_insert_with(|| {
                    let (p, n) = world[i as usize];
                    self.pos.push(p);
                    self.normals.push(n);
                    self.in_sum.push(!dynamic);
                    (self.pos.len() - 1) as u32
                });
                self.idx.push(k);
            }
        }
    }

    /// The mesh (world positions) with the decal's UVs for frame axis `f`.
    pub fn build(self, c: Vec3, f: Vec3, w: f32, h: f32) -> Option<Mesh> {
        if self.idx.is_empty() {
            return None;
        }
        let n_sum: Vec3 = self
            .normals
            .iter()
            .zip(&self.in_sum)
            .filter(|(_, s)| **s)
            .map(|(n, _)| *n)
            .sum();
        let (u_axis, v_axis) = decal_axes(f, n_sum);
        let uvs: Vec<[f32; 2]> = self
            .pos
            .iter()
            .map(|&p| {
                let d = p - c;
                [0.5 - d.dot(u_axis) / w, d.dot(v_axis) / h + 0.5]
            })
            .collect();
        Some(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_POSITION,
                self.pos.iter().map(|p| p.to_array()).collect::<Vec<_>>(),
            )
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_NORMAL,
                self.normals
                    .iter()
                    .map(|n| n.to_array())
                    .collect::<Vec<_>>(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_indices(Indices::U32(self.idx)),
        )
    }
}

/// Whether the AABB `lo`..`hi` overlaps the box (centre `c`, half-extent
/// `e`), touching counts (`GraphicsMesh::GeometryBusStop`'s centre / half
/// size test).
fn box_overlaps(lo: Vec3, hi: Vec3, c: Vec3, e: f32) -> bool {
    let (tc, th) = ((lo + hi) * 0.5, (hi - lo) * 0.5);
    ((c - tc).abs() - (th + Vec3::splat(e))).max_element() <= 0.0
}

/// The front normal of a clockwise face: ‖(v2 − v0) × (v1 − v0)‖.
pub fn face_normal(v: [Vec3; 3]) -> Vec3 {
    (v[2] - v[0]).cross(v[1] - v[0]).normalize_or_zero()
}

/// A particle decal's quad (`DecalSet::RenderPass` @0x18005c200;
/// `DecalSet::AddDecal` @0x18005bd20 stores the coords as given): no
/// projection, four vertices at origin ± X·w/2 ± Z·h/2 of the placement
/// (× its grow scale, 1 here) with the coords' Y as the normal, u along X
/// (0 → 1), v along Z (0 → 1). `lift` raises it along Y (`World::PlaceDecal`
/// @0x180220540 moves the origin 0.01 along the coords' Y).
pub fn quad(c: Vec3, rot: Quat, w: f32, h: f32, lift: f32) -> Mesh {
    let (x, y, z) = (
        rot * Vec3::X * w * 0.5,
        rot * Vec3::Y,
        rot * Vec3::Z * h * 0.5,
    );
    let o = c + y * lift;
    let pos = [o - x - z, o + x - z, o + x + z, o - x + z];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        pos.iter().map(|p| p.to_array()).collect::<Vec<_>>(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![y.to_array(); 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    )
    .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

/// The vertex-colour alpha `DecalSet::Update` @0x18005bfe0 gives the decal
/// `k` (0 = oldest) of a ring holding `n` (≤ 256): its rank from the ring head,
/// 256 − n + k, capped at 255, so the newest is opaque and each later decal
/// fades every older one by 1/255. The decal shaders' deferredPS multiply the
/// texel by the vertex colour (rgb white), so it scales the alpha
/// ([`set_layer_alpha`]).
pub fn ring_alpha(n: usize, k: usize) -> f32 {
    ((256 - n.min(256) + k).min(255)) as f32 / 255.0
}

/// A decal layer's alpha: the base layer's opacity, the glow layer's gain.
pub fn set_layer_alpha(mat: &mut DecalMaterial, a: f32) {
    let p = &mut mat.extension.params;
    if p.glow > 0.0 {
        p.glow = DECAL_GLOW_GAIN * a;
    } else {
        p.opacity = a;
    }
}

/// A particle decal placed with `DecalSet` flag 0x40000 (per
/// `Actor::CollisionCallback` / Game.dll `Character::CollisionCallback`: a
/// body's contact decal; particles and footprints pass 0): `AddDecal` starts
/// its scale at 0 and `DecalSet::Update` adds 0.00025 per ms up to 1 (4 s).
/// The quad's layers are scaled about the decal's origin.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct GrowIn(pub f32);

impl GrowIn {
    pub fn step(self, dt_ms: f32) -> Self {
        if self.0 < 1.0 {
            Self((dt_ms * 0.00025 + self.0).min(1.0))
        } else {
            self
        }
    }
}

fn grow_decals(time: Res<Time>, mut q: Query<(&mut GrowIn, &mut Transform)>) {
    let ms = time.delta_secs() * 1000.0;
    for (mut g, mut tf) in &mut q {
        if g.0 < 1.0 {
            *g = g.step(ms);
            tf.scale = Vec3::splat(g.0);
        }
    }
}

/// Half-extent of the `GeometryBusStop` box.
pub fn extent(w: f32, h: f32, dynamic: bool) -> f32 {
    w.max(h) * if dynamic { 0.7 } else { 0.5 }
}

/// On a decal layer entity whose opacity changes ([`DecalTiming::animated`]). The opacity reaches the shader through
/// the entity's `MeshTag` ([`fade_tag`]), not its material: a fading decal changes no asset, so a fight's blood and
/// scorch decals re-prepare no material and rebuild no bind group each frame.
#[derive(Component)]
pub struct DecalFade {
    pub age: f32,
    pub timing: DecalTiming,
    /// The looping effect the decal belongs to: it goes with it.
    pub owner: Option<Entity>,
    last: f32,
}

impl DecalFade {
    pub fn new(timing: DecalTiming, owner: Option<Entity>) -> Self {
        Self {
            age: 0.0,
            timing,
            owner,
            last: f32::NAN,
        }
    }
}

/// The `MeshTag` of a fading decal at opacity `o`: the bits of `1 − o` (`shaders/decal.wesl` scales its alpha /
/// glow by `1 − tag`, so an untagged decal, tag 0, is unfaded).
fn fade_tag(o: f32) -> MeshTag {
    MeshTag::new((1.0 - o).to_bits())
}

/// Whether [`spawn`] of `dec` decodes nothing on the game thread: its texture, bump and glow are decoded (or failed).
/// What is not decodes in the background ([`crate::textures::Textures::ready_held`]) and this is false; the decoded
/// ones go into `held`, so a caller waiting on the rest keeps them.
pub fn textures_ready(
    dec: &DecalRecord,
    data: &crate::data::GameData,
    textures: &mut crate::textures::Textures,
    images: &mut Assets<Image>,
    held: &mut Vec<Handle<Image>>,
) -> bool {
    let mut ready = true;
    for t in std::iter::once(dec.texture.as_str())
        .chain(dec.bump.as_deref())
        .chain(dec.glow.as_deref())
    {
        // UNORM, as `spawn` reads them.
        ready &= textures.ready_held(data, t, false, images, held);
    }
    ready
}

/// Spawns the decal's layers (lit / unlit base, the additive glow) on `mesh`
/// and returns them; each with its own material (its own opacity). Decodes a texture not loaded yet on the spot
/// (a streaming caller waits on [`textures_ready`] first).
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    dec: &DecalRecord,
    mesh: Handle<Mesh>,
    defaults: &crate::render::GdDefaults,
    data: &crate::data::GameData,
    textures: &mut crate::textures::Textures,
    images: &mut Assets<Image>,
    mats: &mut Assets<DecalMaterial>,
    fade: Option<Option<Entity>>,
) -> Vec<Entity> {
    // UNORM like every texture: the decal is shaded in gamma space (an sRGB
    // view darkened the rugs to black and the road tracks to dark stripes).
    let Some(tex) = textures.get(data, &dec.texture, false, images) else {
        return Vec::new();
    };
    let bump = dec
        .bump
        .as_deref()
        .and_then(|t| textures.get(data, t, false, images));
    let o = dec.timing.opacity(0);
    // A fading decal's material holds the unfaded values; its `MeshTag` carries the opacity ([`DecalFade`]).
    let base = if fade.is_some() { 1.0 } else { o };
    let env = defaults.env.clone();
    let mut out = Vec::new();
    let params = DecalParams {
        bump: if bump.is_some() { 1.0 } else { 0.0 },
        ..dec.params(base)
    };
    // decal_selfillum.ssh has no `deferred` technique: its only pass
    // (`emissiveStyle`, sort 3500) adds texel × alpha × opacity
    // (SrcAlpha / One, DepthWrite false) after the lighting, so it is
    // additive and not part of the G-buffer blend (a green glyph turns the
    // sand under it yellow-green rather than painting it green).
    let mat = mats.add(ExtendedMaterial {
        base: StandardMaterial {
            alpha_mode: if dec.unlit {
                AlphaMode::Add
            } else {
                AlphaMode::Blend
            },
            perceptual_roughness: 0.95,
            depth_bias: 10.0,
            cull_mode: None,
            ..default()
        },
        extension: DecalExt {
            params,
            texture: tex.clone(),
            env: env.clone(),
            bump: bump.unwrap_or_else(|| tex.clone()),
        },
    });
    let mut e = commands.spawn((
        Mesh3d(mesh.clone()),
        MeshMaterial3d(mat),
        bevy::light::NotShadowCaster,
        Name::new("decal"),
    ));
    if !dec.unlit {
        e.insert(crate::render::gbuffer_blend::GBufferBlend::Decal);
    }
    if let Some(owner) = fade {
        e.insert((DecalFade::new(dec.timing, owner), fade_tag(o)));
    }
    out.push(e.id());
    if let Some(glow) = dec
        .glow
        .as_deref()
        .and_then(|t| textures.get(data, t, false, images))
    {
        let mat = mats.add(ExtendedMaterial {
            base: StandardMaterial {
                alpha_mode: AlphaMode::Add,
                depth_bias: 11.0,
                cull_mode: None,
                ..default()
            },
            extension: DecalExt {
                params: DecalParams {
                    glow: DECAL_GLOW_GAIN * base,
                    ..dec.params(base)
                },
                texture: tex,
                env,
                bump: glow,
            },
        });
        let mut e = commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            bevy::light::NotShadowCaster,
            crate::render::post::glow_aware_layers(),
            Name::new("decal glow"),
        ));
        if let Some(owner) = fade {
            e.insert((DecalFade::new(dec.timing, owner), fade_tag(o)));
        }
        out.push(e.id());
    }
    out
}

/// Ages decals (`Decal::UpdateSelf`), removes them at `life` or with their
/// owner, and sets their opacity ([`DecalTiming::opacity`], through [`fade_tag`]).
fn fade_decals(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut DecalFade, &mut MeshTag)>,
    alive: Query<()>,
) {
    for (e, mut d, mut tag) in &mut q {
        d.age += time.delta_secs();
        let age_ms = (d.age * 1000.0) as i32;
        if (d.timing.life_ms > 0 && age_ms >= d.timing.life_ms)
            || d.owner.is_some_and(|o| !alive.contains(o))
        {
            commands.entity(e).try_despawn();
            continue;
        }
        let o = d.timing.opacity(age_ms);
        if o == d.last {
            continue;
        }
        d.last = o;
        *tag = fade_tag(o);
    }
}
