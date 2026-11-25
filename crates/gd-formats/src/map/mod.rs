//! `.map` world container (`Levels/world001.map`, `mainmenu.map`) and the
//! `.lvl` level blobs embedded in it.
//!
//! Layout (per `World::LoadMap` in Engine.dll):
//! ```text
//! "MAP" u8 version (stock 9; loader needs >= 4)
//! u32 header_bytes
//! header_bytes of chunks { u32 id, u32 len, data }   (ids below)
//! chunk 0x1A { u32 id, u32 len, minimap TGA images }  (outside the header)
//! level blobs ("LVL\x0f" ...) at absolute offsets listed in chunk 0x01
//! ```
//!
//! A map is read lazily: [`MapFile`] keeps its source (a file on disk, or the archive entry through
//! [`crate::vfs::Vfs::open_entry`] as the game reads it) and reads a level or minimap when asked.
pub mod level;
pub mod links;
pub mod minimap;
pub mod pathing;
pub mod sector;
pub mod terrain;
pub mod water;

use crate::util::Reader;
use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub use level::Level;

/// Map header chunk ids (World::LoadMap switch).
pub mod chunk {
    /// Region (level) directory.
    pub const REGIONS: u32 = 0x01;
    /// Player spawn points.
    pub const SPAWN_POINTS: u32 = 0x10;
    /// InstanceGroupManager::Load.
    pub const INSTANCE_GROUPS: u32 = 0x11;
    /// Old minimap format (ignored by the loader).
    pub const OLD_MINIMAP: u32 = 0x16;
    /// SectorDataManager::Load (fog/names/ambient/... per sector id).
    pub const SECTOR_DATA: u32 = 0x18;
    /// Minimap image directory (per region offset/size).
    pub const MINIMAPS: u32 = 0x19;
    /// Minimap image blob (follows the header; not read by LoadMap).
    pub const MINIMAP_BLOB: u32 = 0x1A;
    /// Quest file list.
    pub const QUESTS: u32 = 0x1B;
}

/// Engine `GAME::ABBox`: center + half extents.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ABBox {
    pub center: [f32; 3],
    pub half: [f32; 3],
}

impl ABBox {
    pub fn min(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| self.center[i] - self.half[i])
    }
    pub fn max(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| self.center[i] + self.half[i])
    }
    pub(crate) fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            center: r.vec3()?,
            half: r.vec3()?,
        })
    }
}

/// An entity's `GAME::UniqueId` as the map stores it (level entities, instance group members, region ids):
/// four u32.
pub type Uid = [u32; 4];

/// A save's 16-byte `UniqueId` (`save::gdc::Uid`) as the map's [`Uid`]: the four words little-endian, in order.
pub fn uid_words(u: &[u8; 16]) -> Uid {
    let w = |i: usize| u32::from_le_bytes(u[i * 4..i * 4 + 4].try_into().unwrap());
    [w(0), w(1), w(2), w(3)]
}

/// [`uid_words`] backwards: a [`Uid`] as a save's 16 bytes.
pub fn uid_bytes(u: &Uid) -> [u8; 16] {
    let mut b = [0u8; 16];
    for (i, w) in u.iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    b
}

/// One entry of the region directory (chunk 0x01). A region owns one level.
#[derive(Debug, Clone)]
pub struct RegionInfo {
    pub index: usize,
    /// Level file name, e.g. `Levels/Region0A001.lvl`.
    pub name: String,
    /// Coarse bounds stored as integers (center/half), region-local.
    pub bbox: ABBox,
    /// World-space offset of the region origin (integer world units).
    pub position: [i32; 3],
    /// GAME::RegionId (16-byte unique id).
    pub id: [u32; 4],
    /// v>=5: string stored in `Region.zoneRecord`, the region's riftgate map zone
    /// (`Region::SetZoneRecord`, Engine.dll): a
    /// `records/ui/riftgatemap/locations/*.dbr`.
    pub location_record: String,
    /// v>=7: string in `Region.shrineRecord`, the region's riftgate map shrine icon
    /// (`Region::SetShrineRecord`): a
    /// `records/ui/riftgatemap/devotionshrines/*.dbr`, usually empty.
    pub shrine_record: String,
    /// v>=8: string in `Region.extraRecord` (purpose unknown; usually empty).
    pub extra2: String,
    /// Absolute file offset / size of the embedded `.lvl` blob (0 = load from
    /// a loose file `name`).
    pub level_offset: u32,
    pub level_size: u32,
    /// Absolute offset/size of the minimap TGA (chunk 0x19), 0 if none.
    pub minimap_offset: u32,
    pub minimap_size: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct SpawnPoint {
    pub region: i32,
    /// Region-local position.
    pub position: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct RawChunk {
    pub id: u32,
    /// File offset of the chunk payload.
    pub offset: u64,
    pub len: u32,
}

/// What a [`MapFile`] reads from: a file on disk or an archive entry ([`crate::arc::EntryReader`]).
pub trait Source: Read + Seek + Send {}

impl<T: Read + Seek + Send> Source for T {}

pub struct MapFile {
    /// The file's path or the resource name, for messages.
    pub name: String,
    pub version: u8,
    pub file_len: u64,
    pub quests: Vec<String>,
    pub regions: Vec<RegionInfo>,
    pub spawn_points: Vec<SpawnPoint>,
    pub instance_groups: std::sync::Arc<sector::InstanceGroups>,
    pub sector_data: sector::SectorDataSet,
    /// Header chunks not decoded here (location only).
    pub other_chunks: Vec<RawChunk>,
    /// Offset/len of the 0x1A minimap blob, if present.
    pub minimap_blob: Option<(u64, u32)>,
    file: parking_lot::Mutex<Box<dyn Source>>,
}

impl MapFile {
    /// A map file on disk.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        Self::from_source(path.display().to_string(), file)
    }

    /// A map resource of the mounted install / mod (`World::Load` opens it through the
    /// engine's `FileSystem`), read from its archive entry.
    pub fn open_resource(vfs: &crate::vfs::Vfs, res: &str) -> Result<Self> {
        Self::from_source(res.to_owned(), vfs.open_entry(res)?).with_context(|| res.to_owned())
    }

    pub fn from_source(name: String, mut file: impl Source + 'static) -> Result<Self> {
        let file_len = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;
        let mut hdr = [0u8; 8];
        file.read_exact(&mut hdr)?;
        if &hdr[..3] != b"MAP" {
            bail!("{name}: not a MAP file");
        }
        let version = hdr[3];
        if version < 4 {
            bail!("MAP version {version} < 4 unsupported (engine rejects too)");
        }
        let hsize = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
        if 8 + hsize as u64 > file_len {
            bail!("{name}: header size {hsize} past end of file ({file_len})");
        }
        let mut header = vec![0u8; hsize];
        file.read_exact(&mut header)?;

        let mut m = MapFile {
            name,
            version,
            file_len,
            quests: vec![],
            regions: vec![],
            spawn_points: vec![],
            instance_groups: Default::default(),
            sector_data: Default::default(),
            other_chunks: vec![],
            minimap_blob: None,
            file: parking_lot::Mutex::new(Box::new(file)),
        };
        let mut minimaps: Vec<(u32, u32)> = vec![];
        let mut groups = vec![];
        let mut r = Reader::new(&header);
        while r.remaining() > 0 {
            let id = r.u32()?;
            let len = r.u32()? as usize;
            let start = r.pos;
            let data = r.bytes(len)?;
            let mut c = Reader::new(data);
            let ctx = || format!("map chunk {id:#x} at {:#x}", start + 8);
            match id {
                chunk::QUESTS => {
                    let n = c.u32()?;
                    for _ in 0..n {
                        m.quests.push(c.string()?);
                    }
                }
                chunk::REGIONS => m.regions = read_regions(&mut c, version).with_context(ctx)?,
                chunk::SPAWN_POINTS => {
                    let n = c.i32()?;
                    for _ in 0..n.max(0) {
                        m.spawn_points.push(SpawnPoint {
                            region: c.i32()?,
                            position: c.vec3()?,
                        });
                    }
                }
                chunk::MINIMAPS => {
                    c.u32()?; // skipped by the loader
                    let n = c.u32()?;
                    for _ in 0..n {
                        minimaps.push((c.u32()?, c.u32()?));
                    }
                }
                chunk::INSTANCE_GROUPS => {
                    if len > 0 {
                        groups = sector::read_instance_groups(&mut c).with_context(ctx)?;
                    }
                }
                chunk::SECTOR_DATA => {
                    m.sector_data = sector::SectorDataSet::read(&mut c).with_context(ctx)?;
                    c.pos = c.data.len();
                }
                _ => {
                    m.other_chunks.push(RawChunk {
                        id,
                        offset: start as u64 + 8,
                        len: len as u32,
                    });
                    c.pos = c.data.len();
                }
            }
            if c.remaining() != 0 {
                bail!("{}: {} bytes left over", ctx(), c.remaining());
            }
        }
        let origins: crate::collections::HashMap<[u32; 4], [i32; 3]> =
            m.regions.iter().map(|r| (r.id, r.position)).collect();
        for e in groups.iter_mut().flat_map(|g| &mut g.entries) {
            e.world = origins
                .get(&e.ints.map(|i| i as u32))
                .map(|o| [0, 1, 2].map(|k| o[k] as f32 + e.floats[k]));
        }
        m.instance_groups = std::sync::Arc::new(sector::InstanceGroups::new(groups));
        if minimaps.len() == m.regions.len() {
            for (reg, (o, s)) in m.regions.iter_mut().zip(minimaps) {
                reg.minimap_offset = o;
                reg.minimap_size = s;
            }
        } else if !minimaps.is_empty() {
            bail!(
                "minimap count {} != region count {}",
                minimaps.len(),
                m.regions.len()
            );
        }
        // Optional 0x1A minimap blob right after the header.
        let after = 8 + hsize as u64;
        if after + 8 <= file_len {
            let f = m.file.get_mut();
            f.seek(SeekFrom::Start(after))?;
            let mut b = [0u8; 8];
            f.read_exact(&mut b)?;
            let id = u32::from_le_bytes(b[..4].try_into().unwrap());
            let len = u32::from_le_bytes(b[4..].try_into().unwrap());
            if id == chunk::MINIMAP_BLOB && after + 8 + len as u64 <= file_len {
                m.minimap_blob = Some((after + 8, len));
            }
        }
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<()> {
        for r in &self.regions {
            if r.level_size > 0 && r.level_offset as u64 + r.level_size as u64 > self.file_len {
                bail!(
                    "region {} ({}) level blob past end of file",
                    r.index,
                    r.name
                );
            }
            if r.minimap_size > 0 && r.minimap_offset as u64 + r.minimap_size as u64 > self.file_len
            {
                bail!("region {} minimap past end of file", r.index);
            }
        }
        for s in &self.spawn_points {
            if s.region < 0 || s.region as usize >= self.regions.len() {
                bail!("spawn point region {} out of range", s.region);
            }
        }
        Ok(())
    }

    fn read_at(&self, off: u64, len: usize) -> Result<Vec<u8>> {
        let mut f = self.file.lock();
        f.seek(SeekFrom::Start(off))?;
        let mut v = vec![0u8; len];
        f.read_exact(&mut v)?;
        Ok(v)
    }

    /// Region `i`, or an error for an index past the directory.
    fn region(&self, i: usize) -> Result<&RegionInfo> {
        self.regions
            .get(i)
            .with_context(|| format!("region {i} of {}", self.regions.len()))
    }

    /// Raw `.lvl` bytes of region `i`.
    fn level_bytes(&self, i: usize) -> Result<Vec<u8>> {
        let r = self.region(i)?;
        if r.level_size == 0 {
            bail!("region {} ({}) has no embedded level", i, r.name);
        }
        self.read_at(r.level_offset as u64, r.level_size as usize)
    }

    pub fn level(&self, i: usize) -> Result<Level> {
        let b = self.level_bytes(i)?;
        Level::parse(&b).with_context(|| format!("level {} ({})", i, self.regions[i].name))
    }

    /// Minimap image of region `i` (a 32-bit TGA, run-length encoded or not: `minimap::MinimapImage`).
    pub fn minimap_bytes(&self, i: usize) -> Result<Option<Vec<u8>>> {
        let r = self.region(i)?;
        if r.minimap_size == 0 {
            return Ok(None);
        }
        Ok(Some(self.read_at(
            r.minimap_offset as u64,
            r.minimap_size as usize,
        )?))
    }

    pub fn find_region(&self, name_substr: &str) -> Option<&RegionInfo> {
        let n = name_substr.to_ascii_lowercase();
        self.regions
            .iter()
            .find(|r| r.name.to_ascii_lowercase().contains(&n))
    }
}

fn read_regions(c: &mut Reader, version: u8) -> Result<Vec<RegionInfo>> {
    let n = c.u32()? as usize;
    let mut v = Vec::with_capacity(c.capacity(n, 52));
    for index in 0..n {
        let mut b = [0i32; 6];
        for x in &mut b {
            *x = c.i32()?;
        }
        let bbox = ABBox {
            center: [b[0] as f32, b[1] as f32, b[2] as f32],
            half: [b[3] as f32, b[4] as f32, b[5] as f32],
        };
        let position = [c.i32()?, c.i32()?, c.i32()?];
        let id = [c.u32()?, c.u32()?, c.u32()?, c.u32()?];
        let location_record = if version >= 5 {
            c.string()?
        } else {
            String::new()
        };
        let shrine_record = if version >= 7 {
            c.string()?
        } else {
            String::new()
        };
        let extra2 = if version >= 8 {
            c.string()?
        } else {
            String::new()
        };
        let name = c.string()?;
        let level_offset = c.u32()?;
        let level_size = c.u32()?;
        v.push(RegionInfo {
            index,
            name,
            bbox,
            position,
            id,
            location_record,
            shrine_record,
            extra2,
            level_offset,
            level_size,
            minimap_offset: 0,
            minimap_size: 0,
        });
    }
    Ok(v)
}

/// Parse a generic `{u32 id, i32 len, data}` chunk stream.
pub(crate) fn chunks(data: &[u8]) -> Result<Vec<(u32, usize, &[u8])>> {
    let mut r = Reader::new(data);
    let mut out = vec![];
    while r.remaining() > 0 {
        let id = r.u32()?;
        let len = r.i32()?;
        if len < 0 {
            bail!("chunk {id} negative length at {:#x}", r.pos - 8);
        }
        let pos = r.pos;
        out.push((id, pos, r.bytes(len as usize)?));
    }
    Ok(out)
}
