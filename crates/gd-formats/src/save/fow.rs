//! `map.fow` — fog of war. Engine.dll `FOWManager::Write`
//! / `FOWManager::Read`.
//!
//! File: `u32 uncompressed_size` + LZ4 block (`LZ4_compress_fast`, accel 1) of:
//! `u32 'FOWX'`, `u32 version (1)`, `u32 count`, then per region
//! `{u32 name_len, name bytes (e.g. "Levels/Region0A047.lvl"), u32 data_len, data}`.
//! The per-region data is the region's visibility bitmap; kept opaque here.
//!
//! Game bug: `Write` hands `SaveManager::AddJob` the buffer
//! `[u32 size][lz4 data]` but the *compressed* length, so every map.fow on disk
//! is missing the last 4 bytes of the LZ4 stream (the tail of the final literal
//! run). `Read` uses `LZ4_decompress_fast`, which reads those 4 bytes past the
//! end of its buffer. We decode the truncated stream with the missing literal
//! bytes as zeros ([`Fow::decompress_lossy`] reports how many), and write the
//! complete stream (which the game's reader handles fine).
use super::cipher::{bytes_to_string, string_to_bytes};
use crate::util::Reader;
use anyhow::{Context, Result, bail};

pub const MAGIC: u32 = 0x5857_4F46; // "FOWX"

#[derive(Debug, Clone, PartialEq)]
pub struct FowRegion {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fow {
    pub version: u32,
    pub regions: Vec<FowRegion>,
}

impl Fow {
    pub fn decompress(file: &[u8]) -> Result<Vec<u8>> {
        Ok(Self::decompress_lossy(file)?.0)
    }

    /// Decompressed payload plus the number of trailing literal bytes that were
    /// missing from the file (0 for a complete stream, 4 for game-written files).
    pub fn decompress_lossy(file: &[u8]) -> Result<(Vec<u8>, usize)> {
        if file.len() < 4 {
            bail!("map.fow too short");
        }
        let size = u32::from_le_bytes(file[0..4].try_into().unwrap()) as usize;
        // The claimed size drives LZ4's up-front allocation; bound it by what the
        // compressed bytes (plus the padding attempts) can actually decode to.
        if size > crate::util::lz4_max_size(file.len() - 4 + 8) {
            bail!(
                "map.fow: claimed size {size} too large for {} bytes",
                file.len() - 4
            );
        }
        if let Ok(v) = lz4_flex::block::decompress(&file[4..], size) {
            return Ok((v, 0));
        }
        // Game-written files miss exactly 4 bytes; try that first.
        let mut padded = Vec::with_capacity(file.len() - 4 + 8);
        for missing in [4, 1, 2, 3, 5, 6, 7, 8] {
            padded.clear();
            padded.extend_from_slice(&file[4..]);
            padded.resize(padded.len() + missing, 0);
            if let Ok(v) = lz4_flex::block::decompress(&padded, size) {
                return Ok((v, missing));
            }
        }
        bail!("map.fow: LZ4 stream does not decode")
    }

    pub fn parse(file: &[u8]) -> Result<Self> {
        Self::parse_payload(&Self::decompress(file)?)
    }

    pub fn parse_payload(raw: &[u8]) -> Result<Self> {
        let mut r = Reader::new(raw);
        let magic = r.u32()?;
        if magic != MAGIC {
            bail!("not a FOW file (magic {magic:#x})");
        }
        let version = r.u32()?;
        if version != 1 {
            bail!("FOW version {version} not supported");
        }
        let n = r.u32()?;
        let mut regions = Vec::new();
        for _ in 0..n {
            let len = r.u32()? as usize;
            let name = bytes_to_string(r.bytes(len)?);
            let dlen = r.u32()? as usize;
            let data = r.bytes(dlen)?.to_vec();
            regions.push(FowRegion { name, data });
        }
        if r.remaining() != 0 {
            bail!("{} trailing bytes in FOW payload", r.remaining());
        }
        Ok(Self { version, regions })
    }

    pub fn payload(&self) -> Result<Vec<u8>> {
        let mut o = Vec::new();
        o.extend_from_slice(&MAGIC.to_le_bytes());
        o.extend_from_slice(&self.version.to_le_bytes());
        o.extend_from_slice(&(self.regions.len() as u32).to_le_bytes());
        for r in &self.regions {
            let n = string_to_bytes(&r.name)?;
            o.extend_from_slice(&(n.len() as u32).to_le_bytes());
            o.extend_from_slice(&n);
            o.extend_from_slice(&(r.data.len() as u32).to_le_bytes());
            o.extend_from_slice(&r.data);
        }
        Ok(o)
    }

    /// Re-compresses with lz4_flex and writes the complete stream (see module docs).
    /// The payload is identical, the compressed bytes differ from the game's.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let p = self.payload()?;
        let mut o = (p.len() as u32).to_le_bytes().to_vec();
        o.extend_from_slice(&lz4_flex::block::compress(&p));
        Ok(o)
    }
}

/// Cell size in meters (`FogOfWar::kScale` 0.125 per meter, `kInvScale` 8.0).
pub const CELL: f32 = 8.0;
/// Vision radius in meters (`FogOfWar::kVisibleRange`, 30.0).
pub const VISIBLE_RANGE: f32 = 30.0;
/// Inside this distance a cell is fully revealed (`225 = 15^2`, the constant in
/// the reveal loop of `World::AddFOWVisibility` -> `FogOfWar::Reveal`).
pub const CLEAR_RANGE: f32 = 15.0;
/// `FogOfWar::IsInFog` treats cells with a value above this as fogged.
pub const FOG_THRESHOLD: u8 = 0x96;

/// One region's fog grid. `FogOfWar::Write` / `Read`: region data = `u32 width, u32 height, u32 byte count (= w*h),
/// u32 format` then the cells. `Write`: format 0 = raw cells, 1 = every cell 255 and
/// 2 = every cell 0 (both without cells); `Read` decodes 2 as all 0 and any
/// other non-0 format as all 255. Cell 255 =
/// fogged, 0 = fully revealed. `FogOfWar::Initialize`: width =
/// `2 + floor(region_width_m / 8)` (height likewise from the depth), all 255.
/// Row 0 of the data is the region's far (high z) edge: the cell for local
/// `(x, z)` is `col = trunc(x/8 + 1)`, `row = h - trunc(z/8 + 1) - 1`
/// (`FogOfWar::IsInFog`).
#[derive(Debug, Clone, PartialEq)]
pub struct FogGrid {
    pub w: u32,
    pub h: u32,
    pub cells: Vec<u8>,
}

/// The largest fog grid [`FogGrid::parse`] accepts (32 km square at [`CELL`] metres per cell):
/// stops a corrupt width / height from allocating gigabytes.
pub const MAX_CELLS: usize = 4096 * 4096;

impl FogGrid {
    /// A fully fogged grid for a region `width` x `depth` meters
    /// (`FogOfWar::Initialize`).
    pub fn new(width: f32, depth: f32) -> Self {
        let w = 2 + (width / CELL).floor().max(0.0) as u32;
        let h = 2 + (depth / CELL).floor().max(0.0) as u32;
        Self {
            w,
            h,
            cells: vec![255; w as usize * h as usize],
        }
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let (w, h, n) = (r.u32()?, r.u32()?, r.u32()? as usize);
        // A flipped bit in the save's width or height must not overflow the
        // product or allocate gigabytes of cells (LZ4 has no checksum).
        let count = (w as usize)
            .checked_mul(h as usize)
            .filter(|&c| c <= MAX_CELLS)
            .with_context(|| format!("fog grid {w}x{h} out of range"))?;
        if n == 0 {
            return Ok(Self {
                w,
                h,
                cells: vec![255; count],
            });
        }
        let format = r.u32()?;
        if n != count {
            bail!("fog grid {w}x{h} with {n} cells");
        }
        Ok(match format {
            0 if r.remaining() == n => Self {
                w,
                h,
                cells: r.bytes(n)?.to_vec(),
            },
            2 => Self {
                w,
                h,
                cells: vec![0; n],
            },
            _ => Self {
                w,
                h,
                cells: vec![255; n],
            },
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        // `Write`: all cells 255 -> format 1, all 0 -> format 2, both without
        // data; otherwise format 0 with the cells.
        let first = self.cells.first().copied();
        let uniform = first.filter(|f| matches!(f, 0 | 255) && self.cells.iter().all(|c| c == f));
        let format = match uniform {
            Some(255) => 1,
            Some(_) => 2,
            None => 0,
        };
        let mut o = Vec::with_capacity(16 + self.cells.len());
        for v in [self.w, self.h, self.cells.len() as u32, format] {
            o.extend_from_slice(&v.to_le_bytes());
        }
        if format == 0 {
            o.extend_from_slice(&self.cells);
        }
        o
    }

    fn index(&self, x: f32, z: f32) -> Option<usize> {
        let col = (x * 0.125 + 1.0) as i64;
        let row = self.h as i64 - (z * 0.125 + 1.0) as i64 - 1;
        (col >= 0 && col < self.w as i64 && row >= 0 && row < self.h as i64)
            .then(|| (row * self.w as i64 + col) as usize)
    }

    /// `FogOfWar::IsInFog` for a region-local position.
    pub fn is_in_fog(&self, x: f32, z: f32) -> bool {
        self.index(x, z)
            .is_some_and(|i| self.cells[i] > FOG_THRESHOLD)
    }

    /// Reveals around the region-local `(x, z)`. Per `FogOfWar::Reveal` (called
    /// by `World::AddFOWVisibility` with the position moved by
    /// +8 m on both axes so that a cell's sample point is its low corner): every
    /// cell whose sample point is within 30 m gets
    /// `min(cell, d2 < 225 ? 0 : (d2 - 225) / 675 * 255)`. Returns whether any
    /// cell changed. A non-finite position reveals nothing (the float → int
    /// casts would otherwise saturate a NaN to cell 0).
    pub fn reveal(&mut self, x: f32, z: f32) -> bool {
        if !x.is_finite() || !z.is_finite() {
            return false;
        }
        let (px, pz) = (x + CELL, z + CELL);
        let (w, h) = (self.w as i32, self.h as i32);
        let (c0, c1) = (
            (((px - VISIBLE_RANGE) / CELL).floor() as i32).max(0),
            (((px + VISIBLE_RANGE) / CELL).ceil() as i32).min(w - 1),
        );
        let (j0, j1) = (
            (((pz - VISIBLE_RANGE) / CELL).floor() as i32).max(0),
            (((pz + VISIBLE_RANGE) / CELL).ceil() as i32).min(h - 1),
        );
        let mut changed = false;
        for j in j0..=j1 {
            let dz = j as f32 * CELL - pz;
            let row = (h - j - 1) as usize * w as usize;
            for c in c0..=c1 {
                let dx = c as f32 * CELL - px;
                let d2 = dx * dx + dz * dz;
                if d2 >= VISIBLE_RANGE * VISIBLE_RANGE {
                    continue;
                }
                let v = if d2 >= CLEAR_RANGE * CLEAR_RANGE {
                    ((d2 - 225.0) * 0.0014814815 * 255.0) as u8
                } else {
                    0
                };
                let cell = &mut self.cells[row + c as usize];
                if *cell > v {
                    *cell = v;
                    changed = true;
                }
            }
        }
        changed
    }
}

impl Fow {
    pub fn region(&self, name: &str) -> Option<&FowRegion> {
        self.regions
            .iter()
            .find(|r| r.name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A revealed grid survives a whole `map.fow` encode / parse (grid, region, LZ4 file), and the
    /// game-written stream missing its last 4 bytes still decodes (module docs).
    #[test]
    fn revealed_grid_round_trips() {
        // Reveal rule: full clearing inside 15 m, fading to fog at 30 m.
        let mut g = FogGrid::new(100.0, 100.0);
        assert!(g.is_in_fog(50.0, 50.0));
        assert!(g.reveal(50.0, 50.0));
        assert!(!g.is_in_fog(50.0, 50.0) && !g.is_in_fog(60.0, 50.0) && g.is_in_fog(95.0, 95.0));
        assert!(!g.reveal(50.0, 50.0));
        let fow = Fow {
            version: 1,
            regions: vec![FowRegion {
                name: "Levels/Region0A047.lvl".into(),
                data: g.encode(),
            }],
        };
        let file = fow.encode().unwrap();
        let back = Fow::parse(&file).unwrap();
        assert_eq!(back, fow);
        assert_eq!(FogGrid::parse(&back.regions[0].data).unwrap(), g);
        let (payload, _) = Fow::decompress_lossy(&file[..file.len() - 4]).unwrap();
        assert_eq!(payload.len(), fow.payload().unwrap().len());
    }
}
