//! `.lvl` level blob (`GAME::Level::Load(void const*, int, bool, bool)`).
//!
//! ```text
//! "LVL" u8 version (stock 15; loader needs >= 10)
//! v>=15: ABBox (f32 center[3], f32 half[3])   level-local bounds
//! chunks { u32 id, i32 len, data } :
//!   3  ImpassableData          5  entities          6  terrain
//!   9  water                  20  per-entity extra  23  sector layers
//!   28 pathing (PTHD)          other ids skipped by the loader
//! ```
use super::pathing::Pathing;
use super::sector::SectorLayers;
use super::terrain::{Impassable, Terrain};
use super::water::Water;
use super::{ABBox, chunks};
use crate::util::Reader;
use anyhow::{Context, Result, bail};

pub mod chunk {
    pub const IMPASSABLE: u32 = 3;
    pub const ENTITIES: u32 = 5;
    pub const TERRAIN: u32 = 6;
    pub const WATER: u32 = 9;
    pub const ENTITY_EXTRA: u32 = 20;
    pub const SECTOR_LAYERS: u32 = 23;
    pub const PATHING: u32 = 28;
}

/// A placed object. Transform is region-local (`GAME::WorldCoords` = region
/// pointer + Vec3 + 3x3 matrix). `rotation` is stored exactly as in the file
/// (9 floats, rows of the engine's Mat3).
#[derive(Debug, Clone)]
pub struct Entity {
    /// Index into [`Level::records`].
    pub record: u32,
    pub rotation: [f32; 9],
    pub position: [f32; 3],
    /// Optional GAME::UniqueId (used by instance groups / quests).
    pub uid: Option<[u32; 4]>,
    /// Per-instance scale (the first 12 bytes of the entity's chunk-20 payload; inferred), default 1.
    pub scale: [f32; 3],
    /// The rest of the chunk-20 payload (passed to the entity's virtual load-extra function):
    /// class-specific, e.g. paintable/decal data. `None` when the payload is only the scale.
    pub extra: Option<Vec<u8>>,
}

impl Entity {
    /// Per-instance scale, default 1.
    pub fn scale(&self) -> [f32; 3] {
        self.scale
    }

    /// A `TerrainDecoration`'s painted terrain layers (its extra data after the scale), if it has some.
    ///
    /// Game.dll `TerrainDecoration::Read` @0x180583ab0 (after `Actor::Read`, the scale): u32 has-patch;
    /// then Engine.dll `TerrainPatch::Read` @0x1801d27e0: u32 version, i32 width, i32 depth (none when
    /// width·depth ≤ 0), u32 layer count; version 2: per layer u32 used, then width·depth bytes when used;
    /// other versions: per layer width·depth bytes, then u32 used. Layer i is the level terrain's layer i.
    pub fn terrain_patch(&self) -> Option<TerrainPatch> {
        let e = self.extra.as_deref()?;
        let mut r = Reader::new(e);
        if r.u32().ok()? == 0 {
            return None;
        }
        let version = r.u32().ok()?;
        let (width, depth) = (r.i32().ok()?, r.i32().ok()?);
        let cells = usize::try_from(width.checked_mul(depth)?).ok()?;
        if cells == 0 {
            return None;
        }
        let n = r.u32().ok()? as usize;
        let mut layers = Vec::with_capacity(r.capacity(n, 4));
        for _ in 0..n {
            let layer = if version == 2 {
                match r.u32().ok()? {
                    0 => None,
                    _ => Some(r.bytes(cells).ok()?.to_vec()),
                }
            } else {
                let b = r.bytes(cells).ok()?;
                (r.u32().ok()? != 0).then(|| b.to_vec())
            };
            layers.push(layer);
        }
        Some(TerrainPatch {
            width: width as u32,
            depth: depth as u32,
            layers,
        })
    }
}

/// A `TerrainDecoration`'s terrain paint ([`Entity::terrain_patch`]).
#[derive(Debug, Clone)]
pub struct TerrainPatch {
    /// Opacity texels along x / z, one world unit each, centred on the entity.
    pub width: u32,
    pub depth: u32,
    /// Per level terrain layer: its opacities (rows of z, `width` bytes each) when painted.
    pub layers: Vec<Option<Vec<u8>>>,
}

#[derive(Debug, Clone)]
pub struct UnknownChunk {
    pub id: u32,
    pub len: u32,
}

#[derive(Debug, Default)]
pub struct Level {
    pub version: u8,
    pub bbox: Option<ABBox>,
    /// Record path string table (`records/...dbr`) referenced by entities.
    pub records: Vec<String>,
    pub entities: Vec<Entity>,
    /// Terrain kind (u32 at the start of chunk 6): 0 Terrain, 1 GridRegion,
    /// 2 TerrainRT (heightfield, all stock levels), 3 GridRegionRandom.
    pub terrain_kind: Option<u32>,
    pub terrain: Option<Terrain>,
    pub water: Option<Water>,
    pub impassable: Option<Impassable>,
    pub sector_layers: Option<SectorLayers>,
    pub pathing: Option<Pathing>,
    /// Chunks this parser skips (location only).
    pub unknown: Vec<UnknownChunk>,
}

impl Level {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 4 || &data[..3] != b"LVL" {
            bail!("not a LVL blob");
        }
        let version = data[3];
        if version < 10 {
            bail!("LVL version {version} < 10 unsupported");
        }
        let mut r = Reader::at(data, 4);
        let mut lvl = Level {
            version,
            ..Default::default()
        };
        if version >= 15 {
            lvl.bbox = Some(ABBox::read(&mut r)?);
        }
        let mut extra_chunk: Option<&[u8]> = None;
        for (id, pos, body) in chunks(&data[r.pos..])? {
            let ctx = || format!("lvl chunk {id} at {:#x}", pos + r.pos);
            let mut c = Reader::new(body);
            match id {
                chunk::ENTITIES => read_entities(&mut c, &mut lvl).with_context(ctx)?,
                chunk::ENTITY_EXTRA => {
                    extra_chunk = Some(body);
                    c.pos = body.len();
                }
                chunk::TERRAIN => {
                    let kind = if version >= 11 { c.u32()? } else { 0 };
                    lvl.terrain_kind = Some(kind);
                    if kind == 2 {
                        lvl.terrain = Some(Terrain::read(&mut c).with_context(ctx)?);
                    } else {
                        c.pos = body.len(); // other kinds are not in stock data and not rendered
                    }
                }
                chunk::WATER => lvl.water = Some(Water::read(&mut c).with_context(ctx)?),
                chunk::IMPASSABLE => {
                    lvl.impassable = Some(Impassable::read(&mut c).with_context(ctx)?)
                }
                chunk::SECTOR_LAYERS => {
                    lvl.sector_layers = Some(SectorLayers::read(&mut c).with_context(ctx)?)
                }
                chunk::PATHING => lvl.pathing = Some(Pathing::read(body).with_context(ctx)?),
                _ => {
                    lvl.unknown.push(UnknownChunk {
                        id,
                        len: body.len() as u32,
                    });
                    c.pos = body.len();
                }
            }
            if id != chunk::PATHING && c.remaining() != 0 {
                bail!(
                    "{}: {} of {} bytes not consumed",
                    ctx(),
                    c.remaining(),
                    body.len()
                );
            }
        }
        if let Some(body) = extra_chunk {
            // { u32 entity_index, u32 len, data } repeated; data = Vec3 scale, then class-specific bytes.
            let mut c = Reader::new(body);
            while c.remaining() > 0 {
                let i = c.u32()? as usize;
                let n = c.u32()? as usize;
                let d = c.bytes(n)?;
                let Some(e) = lvl.entities.get_mut(i) else {
                    bail!("entity extra for index {i} >= {}", lvl.entities.len());
                };
                if let Some((s, rest)) = d.split_first_chunk::<12>() {
                    let (s, _) = s.as_chunks::<4>();
                    e.scale = [0, 1, 2].map(|k| f32::from_le_bytes(s[k]));
                    e.extra = (!rest.is_empty()).then(|| rest.to_vec());
                } else {
                    e.extra = None;
                }
            }
        }
        Ok(lvl)
    }

    pub fn record_of(&self, e: &Entity) -> &str {
        self.records
            .get(e.record as usize)
            .map(|s| s.as_str())
            .unwrap_or("")
    }
}

fn read_entities(c: &mut Reader, lvl: &mut Level) -> Result<()> {
    let n = c.i32()?;
    for _ in 0..n.max(0) {
        lvl.records.push(c.string()?);
    }
    let count = c.u32()? as usize;
    lvl.entities.reserve(c.capacity(count, 56));
    for _ in 0..count {
        let record = c.u32()?;
        let mut rotation = [0f32; 9];
        for x in &mut rotation {
            *x = c.f32()?;
        }
        let position = c.vec3()?;
        let uid = if c.u32()? != 0 {
            Some([c.u32()?, c.u32()?, c.u32()?, c.u32()?])
        } else {
            None
        };
        if record as usize >= lvl.records.len() {
            bail!("entity record index {record} >= {}", lvl.records.len());
        }
        lvl.entities.push(Entity {
            record,
            rotation,
            position,
            uid,
            scale: [1.0; 3],
            extra: None,
        });
    }
    Ok(())
}
