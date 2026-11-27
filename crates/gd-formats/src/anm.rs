//! `.anm` skeletal animations. Layout from `GraphicsAnim::LoadANMData` and the
//! per-bone reader `GraphicsAnim::ReadBoneKeys`; sampling follows
//! `GraphicsAnim::SampleBone`.
//!
//! Each frame is a full TRS key per bone ("AffineParts"). Rotations here are in
//! the engine's in-memory convention: the file stores the conjugate, and the
//! loader negates x,y,z of both quaternions. Quaternions are (x, y, z, w).
use crate::msh::{Coords, MifBlock, parse_mif};
use crate::util::{Reader, latin1};
use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub translation: [f32; 3],
    /// Engine convention (already conjugated relative to the file).
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// Stretch rotation (identity in version 1 files).
    pub scale_rotation: [f32; 4],
}

pub const IDENTITY_KEY: Key = Key {
    translation: [0.0; 3],
    rotation: [0.0, 0.0, 0.0, 1.0],
    scale: [1.0; 3],
    scale_rotation: [0.0, 0.0, 0.0, 1.0],
};

/// Row-major 3x3 rotation of a unit quaternion (engine `Quaternion::ToMatrix3`, the
/// standard column-vector rotation matrix).
pub fn quat_to_mat3(q: [f32; 4]) -> [[f32; 3]; 3] {
    let [x, y, z, w] = q;
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

impl Key {
    /// `T * R(rotation) * R(scale_rotation) * S` exactly as
    /// `SkeletalPose::CacheCoords` builds it: the scale scales SR's columns
    /// (post-multiplied), not its rows.
    pub fn to_coords(&self) -> Coords {
        // Unit scale and no stretch rotation: `T*R*SR*S` reduces exactly to `T*R`.
        if self.scale == [1.0, 1.0, 1.0] && self.scale_rotation == [0.0, 0.0, 0.0, 1.0] {
            let m = quat_to_mat3(self.rotation);
            return Coords {
                x: [m[0][0], m[1][0], m[2][0]],
                y: [m[0][1], m[1][1], m[2][1]],
                z: [m[0][2], m[1][2], m[2][2]],
                origin: self.translation,
            };
        }
        let r = quat_to_mat3(self.rotation);
        let mut sr = quat_to_mat3(self.scale_rotation);
        for (j, s) in self.scale.iter().enumerate() {
            for row in sr.iter_mut() {
                row[j] *= s;
            }
        }
        let mut m = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] = r[i][0] * sr[0][j] + r[i][1] * sr[1][j] + r[i][2] * sr[2][j];
            }
        }
        let col = |j: usize| [m[0][j], m[1][j], m[2][j]];
        Coords {
            x: col(0),
            y: col(1),
            z: col(2),
            origin: self.translation,
        }
    }
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Shortest-path slerp, linear when nearly parallel (engine `Quaternion::Slerp`).
pub fn slerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let mut d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let mut b = b;
    if d < 0.0 {
        d = -d;
        b = b.map(|v| -v);
    }
    let (wa, wb) = if d >= 0.9999 {
        (1.0 - t, t)
    } else {
        let th = d.acos();
        let s = 1.0 / th.sin();
        (((1.0 - t) * th).sin() * s, (th * t).sin() * s)
    };
    [0, 1, 2, 3].map(|i| a[i] * wa + b[i] * wb)
}

/// One bone's keys. The columns are the same data as [`Self::keys`], split per component: sampling
/// reads two adjacent frames of each component, so the columns keep those reads in one cache line
/// instead of striding 56-byte keys.
#[derive(Debug, Clone)]
pub struct Track {
    pub bone: String,
    pub keys: Vec<Key>,
    /// Per key: translation xyz, rotation xyzw, scale xyz, scale-rotation xyzw.
    columns: KeyColumns,
}

#[derive(Debug, Clone, Default)]
struct KeyColumns {
    t: Vec<[f32; 3]>,
    r: Vec<[f32; 4]>,
    s: Vec<[f32; 3]>,
    sr: Vec<[f32; 4]>,
}

impl Track {
    /// A track from its keys; builds the sample columns.
    pub fn new(bone: String, keys: Vec<Key>) -> Self {
        let columns = KeyColumns {
            t: keys.iter().map(|k| k.translation).collect(),
            r: keys.iter().map(|k| k.rotation).collect(),
            s: keys.iter().map(|k| k.scale).collect(),
            sr: keys.iter().map(|k| k.scale_rotation).collect(),
        };
        Self {
            bone,
            keys,
            columns,
        }
    }
}

impl Track {
    /// Sample at fractional `frame`. `looped` reproduces the engine's wrap
    /// (frames 0..n-1 form a cycle and translation accumulates per cycle, i.e.
    /// root motion); otherwise the time is clamped.
    pub fn sample(&self, frame: f32, looped: bool) -> Key {
        let n = self.keys.len();
        if n == 0 || frame < 0.0 {
            return IDENTITY_KEY;
        }
        let f0 = frame.floor();
        let t = frame - f0;
        let i0 = f0 as usize;
        let c = &self.columns;
        if !looped {
            if i0 >= n {
                return self.keys[n - 1];
            }
            let i1 = (i0 + 1).min(n - 1);
            return Key {
                translation: lerp3(c.t[i0], c.t[i1], t),
                rotation: slerp(c.r[i0], c.r[i1], t),
                scale: lerp3(c.s[i0], c.s[i1], t),
                scale_rotation: slerp(c.sr[i0], c.sr[i1], t),
            };
        }
        let last = n.saturating_sub(1).max(1);
        // Engine: key[i] = key[i % last] with translation (key[last] - key[0]) * (i / last) added.
        // One div for the cycle, the remainder by multiply-subtract (no second div); O(1) however long
        // the clip has looped (`frame` grows with `time_ms`). `frame = +inf` saturates to the last cycle.
        let cyc_of = |i: usize| {
            let cyc = i / last;
            (cyc, i - cyc * last)
        };
        let pos = |i: usize| -> [f32; 3] {
            if i < last || n == 1 {
                c.t[i.min(n - 1)]
            } else {
                let (cyc, k) = cyc_of(i);
                let (a, b) = (c.t[last], c.t[0]);
                let f = cyc as f32;
                let p = c.t[k];
                [0, 1, 2].map(|j| (a[j] - b[j]) * f + p[j])
            }
        };
        let idx = |i: usize| {
            if i < last { i } else { cyc_of(i).1 }
        };
        // Saturating: `frame = +inf` gives `i0 = usize::MAX`.
        let i1 = i0.saturating_add(1);
        let (a, b) = (idx(i0), idx(i1));
        Key {
            translation: lerp3(pos(i0), pos(i1), t),
            rotation: slerp(c.r[a], c.r[b], t),
            scale: lerp3(c.s[a], c.s[b], t),
            scale_rotation: slerp(c.sr[a], c.sr[b], t),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CallbackPoint {
    pub name: String,
    pub frame: i32,
}

#[derive(Debug, Clone)]
pub struct EntityEvent {
    pub frame: i32,
    pub entity: String,
    pub attach: String,
}

#[derive(Debug)]
pub struct Animation {
    /// 1 = headerless legacy, 2 = "ANM\x02".
    pub version: u32,
    pub num_frames: u32,
    /// Frames per second (`GetPose` uses frame = fps * ms / 1000).
    pub frame_rate: u32,
    pub tracks: Vec<Track>,
    /// Parsed text trailer (CallbackPoint / CreateEntity / RemoveEntity /
    /// RagDollData blocks).
    pub trailer: Vec<MifBlock>,
    /// Each skeleton's bone → track map for [`pose_mesh_abs_into`]. Entries of dropped skeletons
    /// are dropped with them.
    bone_maps: parking_lot::Mutex<Vec<BoneMap>>,
}

/// One skeleton's entry of [`Animation::bone_maps`]: per bone, its track index (`u16::MAX`: none).
#[derive(Debug, Clone)]
struct BoneMap {
    mesh: std::sync::Weak<crate::msh::Mesh>,
    tracks: Vec<u16>,
}

impl Clone for Animation {
    /// The bone maps are derived data: a copy builds its own.
    fn clone(&self) -> Self {
        Self {
            version: self.version,
            num_frames: self.num_frames,
            frame_rate: self.frame_rate,
            tracks: self.tracks.clone(),
            trailer: self.trailer.clone(),
            bone_maps: Default::default(),
        }
    }
}

impl Animation {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let (version, body) = if data.len() >= 4 && &data[..3] == b"ANM" {
            if data[3] != 2 {
                bail!("unsupported ANM version {}", data[3]);
            }
            (2, &data[4..])
        } else {
            (1, data)
        };
        let mut r = Reader::new(body);
        let nbones = r.i32()?;
        let num_frames = r.u32()?;
        let frame_rate = r.u32()?;
        if !(0..=4096).contains(&nbones) {
            bail!("bad bone count {nbones}");
        }
        let mut tracks = Vec::with_capacity(nbones as usize);
        for _ in 0..nbones {
            let bone = if version == 1 {
                crate::msh::name32(&mut r)?
            } else {
                r.string()?
            };
            let mut keys = Vec::with_capacity(r.capacity(num_frames as usize, 28));
            for _ in 0..num_frames {
                let translation = r.vec3()?;
                let q = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                let (scale, sq) = if version == 1 {
                    ([1.0; 3], [0.0, 0.0, 0.0, 1.0])
                } else {
                    (r.vec3()?, [r.f32()?, r.f32()?, r.f32()?, r.f32()?])
                };
                let conj = |q: [f32; 4]| [-q[0], -q[1], -q[2], q[3]];
                keys.push(Key {
                    translation,
                    rotation: conj(q),
                    scale,
                    scale_rotation: conj(sq),
                });
            }
            tracks.push(Track::new(bone, keys));
        }
        let text = latin1(r.bytes(r.remaining())?);
        let trailer = parse_mif(&text).context("anm text trailer")?;
        Ok(Self {
            version,
            num_frames,
            frame_rate,
            tracks,
            trailer,
            bone_maps: Default::default(),
        })
    }

    pub fn track(&self, bone: &str) -> Option<&Track> {
        self.tracks
            .iter()
            .find(|t| t.bone.eq_ignore_ascii_case(bone))
    }

    pub fn callback_points(&self) -> Vec<CallbackPoint> {
        self.trailer
            .iter()
            .filter(|b| b.kind == "CallbackPoint")
            .map(|b| CallbackPoint {
                name: b.str("name").unwrap_or_default().to_owned(),
                frame: b.int("frame").unwrap_or(0),
            })
            .collect()
    }

    pub fn create_entities(&self) -> Vec<EntityEvent> {
        self.trailer
            .iter()
            .filter(|b| b.kind == "CreateEntity")
            .map(|b| EntityEvent {
                frame: b.int("frame").unwrap_or(-1),
                entity: b.str("entity").unwrap_or_default().to_owned(),
                attach: b.str("attach").unwrap_or_default().to_owned(),
            })
            .collect()
    }
    /// `RagDollData { effectType, direction (Push|Pull), elevation (Upward|Downward), push (Left|Right) }`:
    /// `GraphicsAnim::GetRagDollEffectType` & co. A skill captures the block of the clip it starts with
    /// (`Skill::CaptureAnimationRagDollInfo`); its `effectType` is the hit's ragdoll effect
    /// when the skill names none.
    pub fn ragdoll(&self) -> Option<&MifBlock> {
        self.trailer.iter().find(|b| b.kind == "RagDollData")
    }
}

/// Mesh-space skinning matrices for a pose, as `GraphicsMesh::GetAbsCoords`
/// computes them:
/// `anim_abs[i] = anim_abs[parent] * bone.local * key(i)`,
/// `skin[i] = anim_abs[i] * inverse(bone.abs)`. Bones without a track use an
/// identity key (i.e. stay at their bind pose relative to the parent).
pub fn pose_mesh(
    mesh: &std::sync::Arc<crate::msh::Mesh>,
    anim: &Animation,
    frame: f32,
    looped: bool,
) -> (Vec<Coords>, Vec<Coords>) {
    let mut abs = Vec::new();
    pose_mesh_abs_into(mesh, anim, frame, looped, &mut abs);
    let skin = abs
        .iter()
        .zip(&mesh.bones)
        .map(|(a, b)| a.mul(&b.abs.inverse()))
        .collect();
    (abs, skin)
}

/// [`pose_mesh`]'s `anim_abs` alone, written into `abs`.
pub fn pose_mesh_abs_into(
    mesh: &std::sync::Arc<crate::msh::Mesh>,
    anim: &Animation,
    frame: f32,
    looped: bool,
    abs: &mut Vec<Coords>,
) {
    let n = mesh.bones.len();
    abs.clear();
    abs.resize(n, Coords::IDENTITY);
    let mut maps = anim.bone_maps.lock();
    maps.retain(|m| m.mesh.strong_count() > 0);
    // The dead entries are gone, so a live `Weak` pointing at `mesh` is `mesh`'s.
    let at = maps
        .iter()
        .position(|m| std::ptr::eq(m.mesh.as_ptr(), std::sync::Arc::as_ptr(mesh)));
    let tracks = match at {
        Some(i) => &maps[i].tracks,
        None => {
            maps.push(BoneMap {
                mesh: std::sync::Arc::downgrade(mesh),
                tracks: mesh
                    .bones
                    .iter()
                    .map(|b| {
                        anim.tracks
                            .iter()
                            .position(|t| t.bone.eq_ignore_ascii_case(&b.name))
                            .and_then(|i| u16::try_from(i).ok())
                            .unwrap_or(u16::MAX)
                    })
                    .collect(),
            });
            &maps.last().unwrap().tracks
        }
    };
    let mut done = crate::msh::BoneSet::new(n);
    /// The skeleton, its clip and the bone → track map the walk reads.
    struct Ctx<'a> {
        mesh: &'a crate::msh::Mesh,
        anim: &'a Animation,
        tracks: &'a [u16],
    }
    fn go(
        i: usize,
        ctx: &Ctx<'_>,
        f: f32,
        l: bool,
        abs: &mut [Coords],
        done: &mut crate::msh::BoneSet,
    ) -> Coords {
        if done.contains(i) {
            return abs[i];
        }
        let b = &ctx.mesh.bones[i];
        let parent = b
            .parent
            .map(|p| go(p, ctx, f, l, abs, done))
            .unwrap_or(Coords::IDENTITY);
        let key = match ctx.tracks[i] {
            u16::MAX => IDENTITY_KEY,
            t => ctx.anim.tracks[t as usize].sample(f, l),
        };
        abs[i] = parent.mul(&b.local).mul(&key.to_coords());
        done.insert(i);
        abs[i]
    }
    let ctx = Ctx { mesh, anim, tracks };
    for i in 0..n {
        go(i, &ctx, frame, looped, abs, &mut done);
    }
}
