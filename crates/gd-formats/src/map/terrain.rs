//! Level terrain (chunk 6, kind 2 = TerrainRT, loader virtual
//! `TerrainRT::Load`) and ImpassableData (chunk 3, `ImpassableData::Load`).
use crate::util::Reader;
use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct TerrainLayer {
    /// Terrain type record / texture path for this layer.
    pub name: String,
    /// Per-cell blend weights, (width-1)*(height-1) bytes, row-major (z rows of
    /// x). `None` for layer 0 (the base layer, implicitly full weight).
    pub weights: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Terrain {
    /// First header u32 (1 in all stock levels; meaning unknown).
    pub header0: u32,
    /// Vertices along x / z (2^n + 1, e.g. 129). One world unit apart.
    pub width: u32,
    pub height: u32,
    /// Header words 4..7 (always 0 in stock data).
    pub header_rest: [u32; 4],
    /// width*height heights, row-major: index = z * width + x. Region-local y.
    pub heights: Vec<f32>,
    /// width*height*bytes_per_vertex per-vertex bitmask bytes
    /// (bytes_per_vertex = layers/8 + 1). Bit 0: the cell is visible; bit
    /// i + 1: layer i is used here (`TerrainRT::GetBlockLayers`, layers of a block).
    pub layer_mask: Vec<u8>,
    pub layers: Vec<TerrainLayer>,
    /// Trailing u32 skipped by the loader.
    pub trailer: u32,
}

impl Terrain {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let header0 = r.u32()?;
        let num_layers = r.u32()? as usize;
        let width = r.u32()?;
        let height = r.u32()?;
        let header_rest = [r.u32()?, r.u32()?, r.u32()?, r.u32()?];
        if width == 0
            || height == 0
            || !(width - 1).is_power_of_two()
            || !(height - 1).is_power_of_two()
        {
            bail!("terrain size {width}x{height} not 2^n+1");
        }
        let n = width as usize * height as usize;
        let heights: Vec<f32> = r
            .bytes(n.checked_mul(4).context("terrain size overflows")?)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        let bpv = num_layers / 8 + 1;
        let mask_len = n
            .checked_mul(bpv)
            .context("terrain layer mask size overflows")?;
        let layer_mask = r.bytes(mask_len)?.to_vec();
        let cells = (width - 1) as usize * (height - 1) as usize;
        let mut layers = Vec::with_capacity(r.capacity(num_layers, 4));
        for i in 0..num_layers {
            let name = r.string()?;
            let weights = if i > 0 {
                Some(r.bytes(cells)?.to_vec())
            } else {
                None
            };
            layers.push(TerrainLayer { name, weights });
        }
        let trailer = r.u32()?;
        Ok(Self {
            header0,
            width,
            height,
            header_rest,
            heights,
            layer_mask,
            layers,
            trailer,
        })
    }

    pub fn height_at_vertex(&self, x: u32, z: u32) -> f32 {
        self.heights[(z * self.width + x) as usize]
    }

    /// Bilinear height at region-local (x, z); clamps to the grid.
    pub fn sample(&self, x: f32, z: f32) -> f32 {
        let mx = (self.width - 1) as f32;
        let mz = (self.height - 1) as f32;
        let x = x.clamp(0.0, mx);
        let z = z.clamp(0.0, mz);
        let (x0, z0) = (x.floor() as u32, z.floor() as u32);
        let (x1, z1) = ((x0 + 1).min(self.width - 1), (z0 + 1).min(self.height - 1));
        let (fx, fz) = (x - x0 as f32, z - z0 as f32);
        let h = |a, b| self.height_at_vertex(a, b);
        let a = h(x0, z0) * (1.0 - fx) + h(x1, z0) * fx;
        let b = h(x0, z1) * (1.0 - fx) + h(x1, z1) * fx;
        a * (1.0 - fz) + b * fz
    }
}

/// `GAME::ImpassableData` (level chunk 3). Not present in stock levels; kept
/// mostly raw.
#[derive(Debug, Clone)]
pub struct Impassable {
    pub width: i32,
    pub unk: i32,
    pub height: i32,
    pub grid: Vec<u8>,
    pub skipped: [u8; 8],
    pub blob: Vec<u8>,
    pub arrays_a: [Vec<u8>; 3],
    pub arrays_b: [Vec<u8>; 3],
}

impl Impassable {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let v = r.i32()?;
        if v != 116 {
            bail!("ImpassableData version {v} != 116");
        }
        let width = r.i32()?;
        let unk = r.i32()?;
        let height = r.i32()?;
        let n = r.i32()?.max(0) as usize;
        let grid = r.bytes(n)?.to_vec();
        let skipped = r.bytes(8)?.try_into().unwrap();
        let n = r.u32()? as usize;
        let blob = r.bytes(n)?.to_vec();
        let rd = |r: &mut Reader| -> Result<Vec<u8>> {
            let n = r.u32()? as usize;
            Ok(r.bytes(n)?.to_vec())
        };
        let arrays_a = [rd(r)?, rd(r)?, rd(r)?];
        let arrays_b = [rd(r)?, rd(r)?, rd(r)?];
        Ok(Self {
            width,
            unk,
            height,
            grid,
            skipped,
            blob,
            arrays_a,
            arrays_b,
        })
    }
}
