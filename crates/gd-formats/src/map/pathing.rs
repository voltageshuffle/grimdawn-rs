//! Level pathing (chunk 28): "PTHD" container of DetourTileCache compressed
//! layers ("DTLR" magic, i.e. dtTileCacheLayerHeader). Parsed by
//! sub_1801099B0 (NavManager), layers added via dtTileCache::addTile-like
//! sub_180264200 which checks magic 'DTLR' + version 1 and keys tiles by
//! (tx, ty, tlayer); data after the 56-byte header is the compressed payload.
use crate::util::Reader;
use anyhow::{Result, bail};

pub const PTHD: u32 = u32::from_le_bytes(*b"PTHD");
pub const DTLR: u32 = u32::from_le_bytes(*b"RLTD");

/// `dtTileCacheLayerHeader` (Recast/Detour layout).
#[derive(Debug, Clone)]
pub struct LayerHeader {
    pub version: i32,
    pub tx: i32,
    pub ty: i32,
    pub tlayer: i32,
    pub bmin: [f32; 3],
    pub bmax: [f32; 3],
    pub hmin: u16,
    pub hmax: u16,
    pub width: u8,
    pub height: u8,
    pub minx: u8,
    pub maxx: u8,
    pub miny: u8,
    pub maxy: u8,
}

#[derive(Debug, Clone)]
pub struct TileLayer {
    pub header: LayerHeader,
    /// Offset of the layer blob within the chunk, and its total size
    /// (header + compressed data).
    pub offset: usize,
    pub size: usize,
    /// Payload after the 56-byte header: stored **uncompressed** in stock data
    /// as `heights[w*h]` (relative to hmin, 0xff = empty), `areas[w*h]`
    /// (0 = not walkable), `cons[w*h]` (low nibble: same-layer neighbour bits
    /// for dirs (-x, +z, +x, -z); high nibble: portal to another layer).
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Pathing {
    pub version: u32,
    /// Inclusive tile index range: region origin / tile size .. max.
    pub tile_min: [i32; 2],
    pub tile_max: [i32; 2],
    /// Layers grouped per tile column as stored.
    pub tiles: Vec<Vec<TileLayer>>,
}

impl Pathing {
    pub fn read(body: &[u8]) -> Result<Self> {
        let mut r = Reader::new(body);
        if r.u32()? != PTHD {
            bail!("pathing: missing PTHD magic");
        }
        let version = r.u32()?;
        if version != 1 {
            bail!("pathing version {version}");
        }
        let tile_min = [r.i32()?, r.i32()?];
        let tile_max = [r.i32()?, r.i32()?];
        let mut tiles = vec![];
        while r.remaining() > 0 {
            let group = r.u32()? as usize;
            let end = r.pos + group;
            if end > body.len() {
                bail!("pathing tile group overruns chunk");
            }
            let mut layers = vec![];
            while r.pos < end {
                let n = r.u32()? as usize;
                let offset = r.pos;
                let blob = r.bytes(n)?;
                layers.push(TileLayer {
                    header: read_layer_header(blob)?,
                    offset,
                    size: n,
                    data: blob.get(56..).unwrap_or(&[]).to_vec(),
                });
            }
            if r.pos != end {
                bail!("pathing tile group size mismatch");
            }
            tiles.push(layers);
        }
        Ok(Self {
            version,
            tile_min,
            tile_max,
            tiles,
        })
    }
    pub fn layer_count(&self) -> usize {
        self.tiles.iter().map(|t| t.len()).sum()
    }
}

fn read_layer_header(b: &[u8]) -> Result<LayerHeader> {
    let mut r = Reader::new(b);
    if b.len() < 56 || r.u32()? != DTLR {
        bail!("pathing layer: missing DTLR header");
    }
    Ok(LayerHeader {
        version: r.i32()?,
        tx: r.i32()?,
        ty: r.i32()?,
        tlayer: r.i32()?,
        bmin: r.vec3()?,
        bmax: r.vec3()?,
        hmin: r.u16()?,
        hmax: r.u16()?,
        width: r.u8()?,
        height: r.u8()?,
        minx: r.u8()?,
        maxx: r.u8()?,
        miny: r.u8()?,
        maxy: r.u8()?,
    })
}
