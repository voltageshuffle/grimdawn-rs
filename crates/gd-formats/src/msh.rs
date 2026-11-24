//! `.msh` meshes ("MSH" + u8 version, then `{u32 id, u32 len, data}` chunks).
//!
//! Layouts follow the chunk readers called from `GraphicsMesh::Initialize`
//! (Engine.dll). All stock files are
//! version 3; the engine accepts >= 2 and the chunk layouts do not depend on it.
//!
//! Coordinates are the game's object space: Y up, right-handed basis as
//! exported (bone/attach transforms are proper rotations), triangles wound
//! clockwise when seen from the front (normal side). Units ~ metres.
//!
//! The physics mesh is the chunk 2 blob ([`PhysicsMesh`]; `PhysicsMesh::Initialize`):
//! `i32 version (1|2)`, `u32 faces`, `u32 vertices`, [v2: `u32 nodes`], faces as 4 × u32 (3 vertex indices + the
//! face's `PhysicsSurface`), vertices as 3 × f32, then the 40-byte tree nodes (not read).
use crate::util::{Reader, latin1};
use anyhow::{Context, Result, bail, ensure};

/// A 4x3 affine transform as the engine stores it: three basis vectors and an
/// origin. It maps local to parent as `p' = x*p.x + y*p.y + z*p.z + origin`
/// (column-vector convention; `a.mul(&b)` applies `b` first).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coords {
    pub x: [f32; 3],
    pub y: [f32; 3],
    pub z: [f32; 3],
    pub origin: [f32; 3],
}

impl Coords {
    pub const IDENTITY: Coords = Coords {
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        z: [0.0, 0.0, 1.0],
        origin: [0.0, 0.0, 0.0],
    };

    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            x: r.vec3()?,
            y: r.vec3()?,
            z: r.vec3()?,
            origin: r.vec3()?,
        })
    }

    pub fn transform_vector(&self, v: [f32; 3]) -> [f32; 3] {
        let mut o = [0.0; 3];
        for i in 0..3 {
            o[i] = self.x[i] * v[0] + self.y[i] * v[1] + self.z[i] * v[2];
        }
        o
    }

    pub fn transform_point(&self, p: [f32; 3]) -> [f32; 3] {
        let v = self.transform_vector(p);
        [
            v[0] + self.origin[0],
            v[1] + self.origin[1],
            v[2] + self.origin[2],
        ]
    }

    /// `self * other` (engine `Coords::Multiply`): apply `other`, then `self`.
    pub fn mul(&self, o: &Coords) -> Coords {
        Coords {
            x: self.transform_vector(o.x),
            y: self.transform_vector(o.y),
            z: self.transform_vector(o.z),
            origin: self.transform_point(o.origin),
        }
    }

    /// General affine inverse (handles scale; the engine assumes orthonormal).
    pub fn inverse(&self) -> Coords {
        let [a, b, c] = [self.x, self.y, self.z];
        // Rows of the inverse 3x3 = cross products / det.
        let cross = |u: [f32; 3], v: [f32; 3]| {
            [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ]
        };
        let dot = |u: [f32; 3], v: [f32; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
        let (r0, r1, r2) = (cross(b, c), cross(c, a), cross(a, b));
        let det = dot(a, r0);
        let inv = if det.abs() > 1e-20 { 1.0 / det } else { 0.0 };
        let (r0, r1, r2) = (
            r0.map(|v| v * inv),
            r1.map(|v| v * inv),
            r2.map(|v| v * inv),
        );
        // Columns of the inverse matrix are (r0[i], r1[i], r2[i]).
        let col = |i: usize| [r0[i], r1[i], r2[i]];
        let m = Coords {
            x: col(0),
            y: col(1),
            z: col(2),
            origin: [0.0; 3],
        };
        let t = m.transform_vector(self.origin);
        Coords {
            origin: [-t[0], -t[1], -t[2]],
            ..m
        }
    }

    /// Column-major 4x4 (glTF / glam `Mat4::from_cols_array`).
    pub fn to_mat4(&self) -> [f32; 16] {
        let [x, y, z, o] = [self.x, self.y, self.z, self.origin];
        [
            x[0], x[1], x[2], 0.0, y[0], y[1], y[2], 0.0, z[0], z[1], z[2], 0.0, o[0], o[1], o[2],
            1.0,
        ]
    }
}

/// Vertex attribute type ids from chunk 4. Semantic names from the
/// `Direct3DVertexDeclaration` ctor (Direct3D11.dll); byte sizes
/// from the chunk-4 reader (the size is implied by the type, not stored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VertexAttr {
    /// 0 POSITION, float3
    Position,
    /// 1 NORMAL, float3
    Normal,
    /// 2 TANGENT, float3
    Tangent,
    /// 3 BINORMAL, float3
    Bitangent,
    /// 4 TEXCOORD0, 7..=13 TEXCOORD1..7, float2
    TexCoord(u8),
    /// 5 BLENDWEIGHT, float4
    BlendWeight,
    /// 6 BLENDINDICES, 4 bytes (DXGI B8G8R8A8_UNORM); indices into the face
    /// set's bone palette
    BlendIndices,
    /// 14 COLOR, 4 bytes (DXGI B8G8R8A8_UNORM, i.e. bytes B,G,R,A)
    Color,
    /// 15 POSITIONT (never seen in files); size unknown, treated as float4
    PositionT,
}

impl VertexAttr {
    pub fn from_id(id: u32) -> Result<Self> {
        Ok(match id {
            0 => Self::Position,
            1 => Self::Normal,
            2 => Self::Tangent,
            3 => Self::Bitangent,
            4 => Self::TexCoord(0),
            7..=13 => Self::TexCoord((id - 6) as u8),
            5 => Self::BlendWeight,
            6 => Self::BlendIndices,
            14 => Self::Color,
            15 => Self::PositionT,
            _ => bail!("unknown vertex attribute type {id}"),
        })
    }
    pub fn id(&self) -> u32 {
        match *self {
            Self::Position => 0,
            Self::Normal => 1,
            Self::Tangent => 2,
            Self::Bitangent => 3,
            Self::TexCoord(0) => 4,
            Self::TexCoord(n) => 6 + n as u32,
            Self::BlendWeight => 5,
            Self::BlendIndices => 6,
            Self::Color => 14,
            Self::PositionT => 15,
        }
    }
    pub fn size(&self) -> usize {
        match self {
            Self::Position | Self::Normal | Self::Tangent | Self::Bitangent => 12,
            Self::TexCoord(_) => 8,
            Self::BlendWeight | Self::PositionT => 16,
            Self::BlendIndices | Self::Color => 4,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Vertices {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 3]>,
    pub bitangents: Vec<[f32; 3]>,
    /// `uvs[n]` = TEXCOORDn (only sets present in the declaration, in order).
    pub uvs: Vec<Vec<[f32; 2]>>,
    pub bone_weights: Vec<[f32; 4]>,
    /// Raw bytes in file order; byte k pairs with weight k. Values index the
    /// owning face set's `bones` palette.
    pub bone_indices: Vec<[u8; 4]>,
    /// Raw bytes as stored (B, G, R, A).
    pub colors: Vec<[u8; 4]>,
}

#[derive(Debug, Clone)]
pub struct FaceSet {
    pub material: u32,
    /// First triangle (index into `indices` / 3).
    pub start_face: u32,
    pub num_faces: u32,
    /// Fourth int: the set's section, an index into
    /// `Mesh::sections` (Engine.dll `GraphicsMeshInstance::
    /// SetTransparency` marks `MeshSet.section` of each set; the
    /// per-section opacity lookup `GraphicsMeshInstance::GetSectionOpacity` takes it). Almost always 0.
    pub section: i32,
    /// Bounding box of the set (center, half extents) - inferred from values.
    pub center: [f32; 3],
    pub half_extents: [f32; 3],
    /// Bone palette: vertex blend index -> mesh bone index.
    pub bones: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    pub first_child: i32,
    pub num_children: i32,
    /// Derived from the children ranges like the engine does.
    pub parent: Option<usize>,
    /// Bind transform relative to the parent bone.
    pub local: Coords,
    /// Bind transform in mesh space (`parent.abs * local`).
    pub abs: Coords,
}

/// The deepest bone chain accepted (root = depth 0). Stock skeletons are a few dozen
/// deep; the cap keeps the recursive pose walks ([`crate::anm`]) off the stack-overflow
/// path on corrupt or crafted meshes (a 10 MB bone chunk can encode a 100k-deep chain).
const MAX_BONE_DEPTH: u32 = 1024;

/// A set of bone indices for posing a skeleton without allocating: a stack bitset up to 512 bones, the heap past
/// that.
pub struct BoneSet {
    small: [u64; 8],
    big: Vec<u64>,
}

impl BoneSet {
    /// An empty set for a skeleton of `n` bones.
    pub fn new(n: usize) -> Self {
        Self {
            small: [0; 8],
            big: if n > 512 {
                vec![0; n.div_ceil(64)]
            } else {
                Vec::new()
            },
        }
    }

    fn words(&self) -> &[u64] {
        if self.big.is_empty() {
            &self.small
        } else {
            &self.big
        }
    }

    pub fn contains(&self, i: usize) -> bool {
        self.words()[i / 64] & (1 << (i % 64)) != 0
    }

    pub fn insert(&mut self, i: usize) {
        let w = if self.big.is_empty() {
            &mut self.small[..]
        } else {
            &mut self.big[..]
        };
        w[i / 64] |= 1 << (i % 64);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    /// type 0
    Int(i32),
    /// type 1 (stored as u32)
    Bool(bool),
    /// type 2 (read and discarded by the engine)
    String(String),
    /// type 3: 12 floats
    Float4x3([f32; 12]),
    /// type 4: 16 floats
    Float4x4([f32; 16]),
    /// type 7: resource path (empty = unset)
    Texture(String),
    /// type 8
    Float2([f32; 2]),
    /// type 9
    Float3([f32; 3]),
    /// type 10
    Float(f32),
    /// type 11
    Float4([f32; 4]),
}

#[derive(Debug, Clone)]
pub struct MaterialParam {
    pub name: String,
    pub value: ParamValue,
}

#[derive(Debug, Clone)]
pub struct Material {
    /// Shader path as stored (the engine lowercases and turns `\` into `/`).
    pub shader: String,
    pub params: Vec<MaterialParam>,
}

impl Material {
    pub fn param(&self, name: &str) -> Option<&ParamValue> {
        self.params
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .map(|p| &p.value)
    }
    pub fn texture(&self, name: &str) -> Option<&str> {
        match self.param(name) {
            Some(ParamValue::Texture(t)) if !t.is_empty() => Some(t),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HitBox {
    pub name: String,
    /// Half sizes along the box axes (scaled by axis lengths in GetHitBox).
    pub extents: [f32; 3],
    /// Box frame (axes normalized on load) relative to `bone`, or mesh space.
    pub coords: Coords,
    /// Bone index, or -1 = look up a bone with the hit box's name.
    pub bone: i32,
    /// [`Self::bone`] resolved against the skeleton at load: the index itself when ≥ 0 (the box
    /// frame is relative to that bone), else the first bone named like the box (its frame alone
    /// is used); `None` without one.
    pub bone_index: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub struct ContourPoint {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

/// Named attach point from the MIF text block (chunk 3), parsed once with the mesh ([`Mesh::attach`]). Per
/// `GraphicsMesh::ReadMif` (the MIF reader of `GraphicsMesh::Initialize`): a missing axis is the identity's, a given
/// `xAxis` / `yAxis` / `zAxis` is normalised as read (no orthonormalisation: a mirrored point such as `hero_00.msh`'s
/// "R Hand", basis (x, -y, z), stays mirrored), the origin is kept as is.
#[derive(Debug, Clone)]
pub struct AttachPoint {
    pub name: String,
    pub parent: Option<String>,
    /// [`Self::parent`]'s index in [`Mesh::bones`] (the first bone of that name, case-insensitive); `None` without a
    /// parent or for a name the skeleton lacks.
    pub parent_bone: Option<usize>,
    pub coords: Coords,
}

impl AttachPoint {
    /// The point in mesh space for the bone frames `abs` (`GraphicsMesh::GetAttachedCoords`): its
    /// parent bone's frame (the identity without one) × the point.
    pub fn coords_in(&self, abs: &[Coords]) -> Coords {
        self.parent_bone
            .and_then(|i| abs.get(i))
            .unwrap_or(&Coords::IDENTITY)
            .mul(&self.coords)
    }
}

/// A `RigidBodyDescription` block of the MIF text (chunk 3): one body of the mesh's ragdoll, laid between the
/// `parentName` and `childName` bones (`Character::PhysicsSetup` builds a Bullet body per block
/// from the two bones' frames × `parentOrigin` / `childOrigin`; the reader is `GraphicsMesh::ReadMif`). Only the
/// fields the port's ragdoll reads (`gd_game::ragdoll`): no stock mesh sets `playFallSound`.
#[derive(Debug, Clone, PartialEq)]
pub struct RigidBody {
    pub parent: String,
    pub child: String,
    /// The body's ends in the parent's / child's bone frame.
    pub parent_origin: [f32; 3],
    pub child_origin: [f32; 3],
    pub mass: f32,
    pub friction: f32,
    /// Its first ground contact plays the character's `bodyFallSound` (`Character::CollisionCallback`: the
    /// body's user data `playFallSound` flag, copied from `RigidBodyDescription.playFallSound`).
    pub play_fall_sound: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MifValue {
    Str(String),
    Tuple(Vec<f32>),
    Bare(String),
}

/// One `Name { key = value ... }` block of the MIF text format, which is shared
/// with the text trailer of .anm files.
#[derive(Debug, Clone, Default)]
pub struct MifBlock {
    pub kind: String,
    pub props: Vec<(String, MifValue)>,
    pub children: Vec<MifBlock>,
    /// Bare words inside the block (e.g. SkeletonEmitterBones entries).
    pub words: Vec<String>,
}

impl MifBlock {
    pub fn get(&self, key: &str) -> Option<&MifValue> {
        self.props.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            MifValue::Str(s) | MifValue::Bare(s) => Some(s),
            _ => None,
        }
    }
    pub fn vec3(&self, key: &str) -> Option<[f32; 3]> {
        match self.get(key)? {
            MifValue::Tuple(v) if v.len() == 3 => Some([v[0], v[1], v[2]]),
            _ => None,
        }
    }
    pub fn int(&self, key: &str) -> Option<i32> {
        self.str(key)?.trim().parse().ok()
    }
}

/// A chunk 2 physics mesh (module docs), with its object-space bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsMesh {
    pub verts: Vec<[f32; 3]>,
    pub tris: Vec<[u32; 3]>,
    /// Each face's `PhysicsSurface` (+12 of the 16 byte face).
    pub surfaces: Vec<u32>,
    /// The bounds of [`Self::verts`] (`f32::MAX` / `f32::MIN` without vertices).
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl PhysicsMesh {
    /// Parses a chunk 2 blob; `None` for another version, a blob too short for its counts or a vertex index out
    /// of range.
    pub fn parse(b: &[u8]) -> Option<Self> {
        let u = |o: usize| {
            b.get(o..o + 4)
                .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        };
        let f = |o: usize| u(o).map(f32::from_bits);
        let version = u(0)?;
        if !(1..=2).contains(&version) {
            return None;
        }
        let (nf, nv) = (u(4)? as usize, u(8)? as usize);
        let mut o = if version > 1 { 16 } else { 12 };
        if b.len() < o + 16 * nf + 12 * nv {
            return None;
        }
        let mut tris = Vec::with_capacity(nf);
        let mut surfaces = Vec::with_capacity(nf);
        for _ in 0..nf {
            let t = [u(o)?, u(o + 4)?, u(o + 8)?];
            if t.iter().any(|i| *i as usize >= nv) {
                return None;
            }
            tris.push(t);
            surfaces.push(u(o + 12)?);
            o += 16;
        }
        let mut verts = Vec::with_capacity(nv);
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for _ in 0..nv {
            let v = [f(o)?, f(o + 4)?, f(o + 8)?];
            for ((lo, hi), c) in min.iter_mut().zip(&mut max).zip(v) {
                *lo = lo.min(c);
                *hi = hi.max(c);
            }
            verts.push(v);
            o += 12;
        }
        Some(Self {
            verts,
            tris,
            surfaces,
            min,
            max,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub version: u8,
    /// Chunk 0: source .mif path.
    pub mif_path: Option<String>,
    /// Chunk 2: physics (collision) mesh, parsed at load (`None`: no chunk or
    /// a blob [`PhysicsMesh::parse`] rejects).
    pub physics: Option<std::sync::Arc<PhysicsMesh>>,
    pub mif: Vec<MifBlock>,
    /// The `AttachPoint` blocks of [`Self::mif`], parsed at load ([`Self::parse`]).
    pub attach: Vec<AttachPoint>,
    /// Chunk 4.
    pub vertex_attrs: Vec<VertexAttr>,
    pub vertex_stride: u32,
    pub num_vertices: u32,
    pub vertices: Vertices,
    /// Chunk 5: u16 triangle list.
    pub indices: Vec<u16>,
    pub face_sets: Vec<FaceSet>,
    /// Chunk 6.
    pub bones: Vec<Bone>,
    /// The root-motion bone: the first bone without a parent ([`Self::root_motion_bone`]),
    /// resolved at load.
    pub root_bone: Option<usize>,
    /// Chunk 7.
    pub materials: Vec<Material>,
    /// Chunk 8.
    pub hit_boxes: Vec<HitBox>,
    /// The hit boxes' bind-pose frames ([`Self::hit_boxes`] order), resolved at load: the bone's
    /// bind frame × the box frame (a bone index), the named bone's bind frame alone (bone −1),
    /// else the box frame. gd-sim `bounds::Aabb::object` reads them.
    pub hit_box_bind: Vec<Coords>,
    /// Chunk 9: closed outlines (point + outward normal).
    pub contours: Vec<Vec<ContourPoint>>,
    /// Chunk 10: (min, max).
    pub bounding_box: Option<([f32; 3], [f32; 3])>,
    /// Chunk 11: extra u16 triangle list drawn by RenderBlendedFaces.
    pub blended_faces: Vec<u16>,
    /// Chunk 12: portals, each a list of triangles.
    pub portals: Vec<Vec<[[f32; 3]; 3]>>,
    /// Chunk 13: section names.
    pub sections: Vec<String>,
    /// Chunk 14: extra u16 triangle list drawn by RenderPaintableFaces.
    pub paintable_faces: Vec<u16>,
    /// Ids of chunks this parser does not know (the engine skips them too).
    pub unknown_chunks: Vec<u32>,
}

fn name32(r: &mut Reader) -> Result<String> {
    let b = r.bytes(32)?;
    let end = b.iter().position(|&c| c == 0).unwrap_or(32);
    Ok(latin1(&b[..end]))
}

fn read_tris(r: &mut Reader) -> Result<Vec<u16>> {
    let n = r.u32()? as usize;
    let b = r.bytes(n * 6)?;
    Ok(b.as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect())
}

impl Mesh {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 4 || &data[..3] != b"MSH" {
            bail!("not an MSH file");
        }
        let version = data[3];
        if version < 2 {
            bail!("unsupported MSH version {version}");
        }
        let mut m = Mesh {
            version,
            ..Default::default()
        };
        let mut seen = 0u64;
        let mut r = Reader::at(data, 4);
        while r.remaining() >= 8 {
            let id = r.u32()?;
            let len = r.u32()? as usize;
            let body = r
                .bytes(len)
                .with_context(|| format!("chunk {id} length {len}"))?;
            // Repeated geometry/skeleton chunks would mix two meshes' arrays.
            if (4..=6).contains(&id) && seen & (1 << id) != 0 {
                bail!("duplicate chunk {id}");
            }
            if id < 64 {
                seen |= 1 << id;
            }
            let mut c = Reader::new(body);
            m.read_chunk(id, &mut c)
                .with_context(|| format!("chunk {id}"))?;
            if id != 2 && id != 3 && c.remaining() != 0 {
                bail!("chunk {id}: {} trailing bytes", c.remaining());
            }
        }
        if r.remaining() != 0 {
            bail!("{} trailing bytes after chunks", r.remaining());
        }
        m.validate()?;
        m.resolve_hit_boxes();
        m.attach = m.parse_attach_points();
        Ok(m)
    }

    /// Cross-chunk consistency: every index, palette and material reference is in range, so
    /// renderers and skinning can index without checks.
    fn validate(&self) -> Result<()> {
        let nv = self.num_vertices as usize;
        ensure!(self.vertices.positions.len() == nv, "no positions");
        if let Some(&i) = self.indices.iter().find(|&&i| i as usize >= nv) {
            bail!("index {i} >= {nv} vertices");
        }
        let v = &self.vertices;
        ensure!(
            v.bone_indices.is_empty() || v.bone_weights.len() == nv,
            "blend indices without blend weights"
        );
        for fs in &self.face_sets {
            let start = fs.start_face as usize * 3;
            let end = start + fs.num_faces as usize * 3;
            ensure!(end <= self.indices.len(), "face set out of range");
            ensure!(
                self.materials.is_empty() || (fs.material as usize) < self.materials.len(),
                "face set material {} of {}",
                fs.material,
                self.materials.len()
            );
            if let Some(&b) = fs.bones.iter().find(|&&b| b as usize >= self.bones.len()) {
                bail!("palette bone {b} >= {}", self.bones.len());
            }
            if v.bone_indices.is_empty() {
                continue;
            }
            for &vi in &self.indices[start..end] {
                let w = v.bone_weights[vi as usize];
                for (k, &b) in v.bone_indices[vi as usize].iter().enumerate() {
                    ensure!(
                        w[k] == 0.0 || (b as usize) < fs.bones.len(),
                        "blend index {b} outside palette of {}",
                        fs.bones.len()
                    );
                }
            }
        }
        Ok(())
    }

    fn read_chunk(&mut self, id: u32, r: &mut Reader) -> Result<()> {
        match id {
            0 => self.mif_path = Some(r.string()?),
            2 => {
                self.physics = PhysicsMesh::parse(r.bytes(r.remaining())?).map(std::sync::Arc::new)
            }
            3 => self.mif = parse_mif(&latin1(r.bytes(r.remaining())?))?,
            4 => self.read_vertices(r)?,
            5 => self.read_faces(r)?,
            6 => self.read_bones(r)?,
            7 => self.read_materials(r)?,
            8 => {
                let n = r.u32()?;
                for _ in 0..n {
                    let name = name32(r)?;
                    let extents = r.vec3()?;
                    let coords = Coords::read(r)?;
                    let bone = r.i32()?;
                    self.hit_boxes.push(HitBox {
                        name,
                        extents,
                        coords,
                        bone,
                        // Resolved by `resolve_hit_boxes` once every chunk is read.
                        bone_index: None,
                    });
                }
            }
            9 => {
                let n = r.u32()?;
                for _ in 0..n {
                    let np = r.u32()?;
                    let mut pts = Vec::with_capacity(r.capacity(np as usize, 24));
                    for _ in 0..np {
                        pts.push(ContourPoint {
                            position: r.vec3()?,
                            normal: r.vec3()?,
                        });
                    }
                    self.contours.push(pts);
                }
            }
            10 => self.bounding_box = Some((r.vec3()?, r.vec3()?)),
            11 => self.blended_faces = read_tris(r)?,
            12 => {
                let n = r.u32()?;
                for _ in 0..n {
                    let nt = r.u32()?;
                    let mut tris = Vec::with_capacity(r.capacity(nt as usize, 36));
                    for _ in 0..nt {
                        tris.push([r.vec3()?, r.vec3()?, r.vec3()?]);
                    }
                    self.portals.push(tris);
                }
            }
            13 => {
                let n = r.u32()?;
                for _ in 0..n {
                    self.sections.push(r.string()?);
                }
            }
            14 => self.paintable_faces = read_tris(r)?,
            _ => self.unknown_chunks.push(id),
        }
        Ok(())
    }

    fn read_vertices(&mut self, r: &mut Reader) -> Result<()> {
        let nattr = r.u32()?;
        self.vertex_stride = r.u32()?;
        self.num_vertices = r.u32()?;
        self.vertex_attrs.clear();
        self.vertices = Vertices::default();
        let mut offs = Vec::new();
        let mut off = 0usize;
        for _ in 0..nattr {
            let a = VertexAttr::from_id(r.u32()?)?;
            self.vertex_attrs.push(a);
            offs.push(off);
            off += a.size();
        }
        let stride = self.vertex_stride as usize;
        if off > stride {
            bail!("attributes need {off} bytes but stride is {stride}");
        }
        let n = self.num_vertices as usize;
        let data = r.bytes(n * stride)?;
        let f = |b: &[u8], o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let v = &mut self.vertices;
        let mut uv_sets: Vec<u8> = Vec::new();
        for (&a, &o) in self.vertex_attrs.iter().zip(&offs) {
            let it = data.chunks_exact(stride);
            let f3 = |vb: &[u8]| [f(vb, o), f(vb, o + 4), f(vb, o + 8)];
            match a {
                VertexAttr::Position => v.positions = it.map(f3).collect(),
                VertexAttr::Normal => v.normals = it.map(f3).collect(),
                VertexAttr::Tangent => v.tangents = it.map(f3).collect(),
                VertexAttr::Bitangent => v.bitangents = it.map(f3).collect(),
                VertexAttr::TexCoord(s) => {
                    uv_sets.push(s);
                    v.uvs.push(it.map(|vb| [f(vb, o), f(vb, o + 4)]).collect());
                }
                VertexAttr::BlendWeight => {
                    v.bone_weights = it
                        .map(|vb| [f(vb, o), f(vb, o + 4), f(vb, o + 8), f(vb, o + 12)])
                        .collect()
                }
                VertexAttr::BlendIndices => {
                    v.bone_indices = it.map(|vb| vb[o..o + 4].try_into().unwrap()).collect()
                }
                VertexAttr::Color => {
                    v.colors = it.map(|vb| vb[o..o + 4].try_into().unwrap()).collect()
                }
                VertexAttr::PositionT => {}
            }
        }
        if uv_sets.iter().enumerate().any(|(i, &s)| i != s as usize) {
            bail!("non-contiguous texcoord sets {uv_sets:?}");
        }
        Ok(())
    }

    fn read_faces(&mut self, r: &mut Reader) -> Result<()> {
        let nfaces = r.u32()? as usize;
        let nsets = r.u32()?;
        let b = r.bytes(nfaces * 6)?;
        self.indices = b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        for _ in 0..nsets {
            let material = r.u32()?;
            let start_face = r.u32()?;
            let num_faces = r.u32()?;
            let section = r.i32()?;
            let center = r.vec3()?;
            let half_extents = r.vec3()?;
            let nb = r.u32()?;
            let bones = (0..nb).map(|_| r.u32()).collect::<Result<_>>()?;
            self.face_sets.push(FaceSet {
                material,
                start_face,
                num_faces,
                section,
                center,
                half_extents,
                bones,
            });
        }
        Ok(())
    }

    fn read_bones(&mut self, r: &mut Reader) -> Result<()> {
        let n = r.u32()? as usize;
        for _ in 0..n {
            let name = name32(r)?;
            let first_child = r.i32()?;
            let num_children = r.i32()?;
            let local = Coords::read(r)?;
            self.bones.push(Bone {
                name,
                first_child,
                num_children,
                parent: None,
                local,
                abs: local,
            });
        }
        for i in 0..n {
            let (fc, nc) = (self.bones[i].first_child, self.bones[i].num_children);
            let end = fc
                .checked_add(nc)
                .with_context(|| format!("bone {i} child range {fc}+{nc} overflows"))?;
            for c in fc.max(0)..end {
                let c = c as usize;
                if c >= n {
                    bail!("bone {i} child {c} out of range");
                }
                if self.bones[c].parent.is_some() {
                    bail!("bone {c} has two parents");
                }
                self.bones[c].parent = Some(i);
            }
        }
        // Absolute bind transforms (children may precede parents in theory): walk up to the
        // nearest resolved ancestor, then resolve back down. A chain longer than the bone count
        // is a cycle; a chain deeper than MAX_BONE_DEPTH is rejected (the pose walks recurse).
        let mut done = vec![false; n];
        let mut depth = vec![0u32; n];
        let mut chain = Vec::new();
        for i in 0..n {
            let mut j = i;
            while !done[j] {
                if chain.len() > n {
                    bail!("bone hierarchy cycle");
                }
                chain.push(j);
                match self.bones[j].parent {
                    Some(p) => j = p,
                    None => break,
                }
            }
            while let Some(k) = chain.pop() {
                self.bones[k].abs = match self.bones[k].parent {
                    Some(p) => self.bones[p].abs.mul(&self.bones[k].local),
                    None => self.bones[k].local,
                };
                depth[k] = match self.bones[k].parent {
                    Some(p) => depth[p] + 1,
                    None => 0,
                };
                ensure!(
                    depth[k] < MAX_BONE_DEPTH,
                    "bone hierarchy deeper than {MAX_BONE_DEPTH}"
                );
                done[k] = true;
            }
        }
        self.root_bone = self.bones.iter().position(|b| b.parent.is_none());
        Ok(())
    }

    fn read_materials(&mut self, r: &mut Reader) -> Result<()> {
        let n = r.u32()?;
        for _ in 0..n {
            let shader = r.string()?;
            let np = r.u32()?;
            let mut params = Vec::with_capacity(r.capacity(np as usize, 12));
            for _ in 0..np {
                let name = r.string()?;
                let ty = r.u32()?;
                let mut fl = |k: usize| -> Result<Vec<f32>> { (0..k).map(|_| r.f32()).collect() };
                let value = match ty {
                    0 => ParamValue::Int(r.i32()?),
                    1 => ParamValue::Bool(r.u32()? != 0),
                    2 => ParamValue::String(r.string()?),
                    3 => ParamValue::Float4x3(fl(12)?.try_into().unwrap()),
                    4 => ParamValue::Float4x4(fl(16)?.try_into().unwrap()),
                    7 => ParamValue::Texture(r.string()?),
                    8 => ParamValue::Float2(fl(2)?.try_into().unwrap()),
                    9 => ParamValue::Float3(fl(3)?.try_into().unwrap()),
                    10 => ParamValue::Float(r.f32()?),
                    11 => ParamValue::Float4(fl(4)?.try_into().unwrap()),
                    _ => bail!("material param {name}: unknown type {ty}"),
                };
                params.push(MaterialParam { name, value });
            }
            self.materials.push(Material { shader, params });
        }
        Ok(())
    }

    pub fn bone_index(&self, name: &str) -> Option<usize> {
        self.bones
            .iter()
            .position(|b| b.name.eq_ignore_ascii_case(name))
    }

    /// The engine's root-motion bone: the first bone without a parent ([`Self::root_bone`]).
    pub fn root_motion_bone(&self) -> Option<&Bone> {
        self.root_bone.map(|i| &self.bones[i])
    }

    /// Resolves the hit boxes' bones once ([`HitBox::bone_index`]) and caches their bind-pose
    /// frames ([`Self::hit_box_bind`]): `GraphicsMesh::GetHitBox` takes the bone's frame × the
    /// box frame for a bone index, the named bone's frame alone for bone −1, else the box frame.
    fn resolve_hit_boxes(&mut self) {
        self.hit_box_bind.clear();
        for i in 0..self.hit_boxes.len() {
            let hb = &self.hit_boxes[i];
            let bone = usize::try_from(hb.bone)
                .ok()
                .or_else(|| self.bone_index(&hb.name));
            let frame = match bone {
                Some(b) if hb.bone >= 0 => self
                    .bones
                    .get(b)
                    .map_or(hb.coords, |b| b.abs.mul(&hb.coords)),
                Some(b) => self.bones.get(b).map_or(hb.coords, |b| b.abs),
                None => hb.coords,
            };
            self.hit_boxes[i].bone_index = bone;
            self.hit_box_bind.push(frame);
        }
    }

    /// Per-vertex global bone indices (palette resolved through the face set
    /// that references the vertex). Unreferenced vertices keep 0.
    pub fn global_bone_indices(&self) -> Vec<[u16; 4]> {
        let mut out = vec![[0u16; 4]; self.num_vertices as usize];
        if self.vertices.bone_indices.is_empty() {
            return out;
        }
        for fs in &self.face_sets {
            let s = fs.start_face as usize * 3;
            let e = (s + fs.num_faces as usize * 3).min(self.indices.len());
            for &vi in &self.indices[s.min(e)..e] {
                let vi = vi as usize;
                let Some(&raw) = self.vertices.bone_indices.get(vi) else {
                    continue;
                };
                if let Some(slot) = out.get_mut(vi) {
                    *slot = raw.map(|b| fs.bones.get(b as usize).copied().unwrap_or(0) as u16);
                }
            }
        }
        out
    }

    /// The mesh's attach points ([`Self::attach`]).
    pub fn attach_points(&self) -> &[AttachPoint] {
        &self.attach
    }

    /// The attach point `name` (case-insensitive, the first of that name).
    pub fn attach_point(&self, name: &str) -> Option<&AttachPoint> {
        self.attach
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// The mesh's ragdoll bodies ([`RigidBody`], read from [`Self::mif`] on each call: a death or a knockdown
    /// asks once). `Actor::HasRigidBodyData` is "any". The numbers are quoted in the text
    /// (`mass = "1.000000"`); a missing one reads 1.
    pub fn rigid_bodies(&self) -> impl Iterator<Item = RigidBody> + '_ {
        self.mif
            .iter()
            .filter(|b| b.kind == "RigidBodyDescription")
            .map(|b| {
                let num = |k: &str| {
                    b.str(k)
                        .and_then(|s| s.trim().parse::<f32>().ok())
                        .unwrap_or(1.0)
                };
                RigidBody {
                    parent: b.str("parentName").unwrap_or_default().to_owned(),
                    child: b.str("childName").unwrap_or_default().to_owned(),
                    parent_origin: b.vec3("parentOrigin").unwrap_or([0.0; 3]),
                    child_origin: b.vec3("childOrigin").unwrap_or([0.0; 3]),
                    mass: num("mass"),
                    friction: num("friction"),
                    play_fall_sound: b
                        .str("playFallSound")
                        .is_some_and(|s| s.eq_ignore_ascii_case("true")),
                }
            })
    }

    fn parse_attach_points(&self) -> Vec<AttachPoint> {
        self.mif
            .iter()
            .filter(|b| b.kind == "AttachPoint")
            .map(|b| {
                let axis = |key: &str, default: [f32; 3]| {
                    b.vec3(key).map_or(default, |v| {
                        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                        if len > 0.0 { v.map(|c| c / len) } else { v }
                    })
                };
                let parent = b.str("parent");
                AttachPoint {
                    name: b.str("name").unwrap_or_default().to_owned(),
                    parent: parent.map(str::to_owned),
                    parent_bone: parent.and_then(|n| self.bone_index(n)),
                    coords: Coords {
                        x: axis("xAxis", [1.0, 0.0, 0.0]),
                        y: axis("yAxis", [0.0, 1.0, 0.0]),
                        z: axis("zAxis", [0.0, 0.0, 1.0]),
                        origin: b.vec3("origin").unwrap_or([0.0; 3]),
                    },
                }
            })
            .collect()
    }

    /// `CreateEntity { attach = .., entity = .. }` pairs from the MIF block.
    pub fn create_entities(&self) -> Vec<(String, String)> {
        self.mif
            .iter()
            .filter(|b| b.kind == "CreateEntity")
            .map(|b| {
                (
                    b.str("attach").unwrap_or_default().to_owned(),
                    b.str("entity").unwrap_or_default().to_owned(),
                )
            })
            .collect()
    }
}

// MIF text.

#[derive(Debug, Clone, PartialEq)]
enum Tok<'a> {
    Word(&'a str),
    Str(&'a str),
    Tuple(Vec<f32>),
    Open,
    Close,
    Eq,
}

fn tokenize(s: &str) -> Result<Vec<Tok<'_>>> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        match c {
            _ if c.is_ascii_whitespace() || c == 0 => i += 1,
            b'{' => (out.push(Tok::Open), i += 1).1,
            b'}' => (out.push(Tok::Close), i += 1).1,
            b'=' => (out.push(Tok::Eq), i += 1).1,
            b'"' => {
                let end = s[i + 1..].find('"').map(|e| i + 1 + e).unwrap_or(b.len());
                out.push(Tok::Str(&s[i + 1..end]));
                i = end + 1;
            }
            b'(' => {
                let end = s[i..].find(')').map(|e| i + e).context("unterminated (")?;
                let vals = s[i + 1..end]
                    .split(',')
                    .map(|t| {
                        t.trim()
                            .parse::<f32>()
                            .with_context(|| format!("bad number {t:?}"))
                    })
                    .collect::<Result<_>>()?;
                out.push(Tok::Tuple(vals));
                i = end + 1;
            }
            _ => {
                let st = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && !b"{}=\"(".contains(&b[i]) {
                    i += 1;
                }
                out.push(Tok::Word(&s[st..i]));
            }
        }
    }
    Ok(out)
}

/// Deepest block nesting accepted (stock files nest a few levels).
const MIF_MAX_DEPTH: usize = 64;

/// Parse MIF-style text (`Kind { key = value  Child { ... }  word }` ...).
/// Lenient like the engine's tokenizer loop: stray top-level tokens are
/// skipped (stock files contain stray `}` and truncated words), a block cut
/// off by end of text is kept, and a key with a missing value gets "".
pub fn parse_mif(text: &str) -> Result<Vec<MifBlock>> {
    let toks = tokenize(text)?;
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < toks.len() {
        match toks[pos] {
            Tok::Word(w) if toks.get(pos + 1) == Some(&Tok::Open) => {
                pos += 2;
                out.push(parse_block(w, &toks, &mut pos, 1)?);
            }
            _ => pos += 1,
        }
    }
    Ok(out)
}

fn parse_block(kind: &str, toks: &[Tok], pos: &mut usize, depth: usize) -> Result<MifBlock> {
    if depth > MIF_MAX_DEPTH {
        bail!("MIF blocks nested deeper than {MIF_MAX_DEPTH}");
    }
    let mut b = MifBlock {
        kind: kind.to_owned(),
        ..Default::default()
    };
    loop {
        let Some(t) = toks.get(*pos) else {
            return Ok(b);
        };
        *pos += 1;
        match *t {
            Tok::Close => return Ok(b),
            Tok::Word(w) | Tok::Str(w) => match toks.get(*pos) {
                Some(Tok::Eq) => {
                    let v = match toks.get(*pos + 1) {
                        Some(Tok::Str(s)) => MifValue::Str((*s).to_owned()),
                        Some(Tok::Word(s)) => MifValue::Bare((*s).to_owned()),
                        Some(Tok::Tuple(v)) => MifValue::Tuple(v.clone()),
                        _ => {
                            *pos += 1;
                            b.props.push((w.to_owned(), MifValue::Bare(String::new())));
                            continue;
                        }
                    };
                    *pos += 2;
                    b.props.push((w.to_owned(), v));
                }
                Some(Tok::Open) => {
                    *pos += 1;
                    b.children.push(parse_block(w, toks, pos, depth + 1)?);
                }
                _ => b.words.push(w.to_owned()),
            },
            _ => {} // stray `{`, `=` or tuple: skip
        }
    }
}
