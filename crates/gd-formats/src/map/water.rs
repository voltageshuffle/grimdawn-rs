//! Level water (chunk 9): `GAME::Water::Load` @0x1801F81E0, WaterType::Load
//! @0x180207030, WaterLayer::Load @0x1801FDCE0, WaterPath::Load @0x180200B20,
//! WaterWaveSet::Load @0x18020CBD0.
//!
//! Water paths (rivers), per `WaterPath::Load` @0x180200b20: the water type's name, the path's surface height
//! (region-local y, `WaterPath+0x6C`), then the control points, 64 bytes each of which the engine keeps the first 28
//! ([`ControlPoint`]); a path of n points has n - 1 segments, each the cubic Bézier between two neighbours (geometry and
//! hits: grimdawn `water.rs`, `gd_sim::surface::in_path_segment`). A path whose water type is not loaded logs "Failed to
//! load water type '%s' for water path." and is not used.
use crate::util::Reader;
use anyhow::{Result, bail};

pub const WATER_MAGIC: u32 = 0xFEED_CBA9;

#[derive(Debug, Clone)]
pub struct WaterType {
    pub version: u32,
    /// 14 floats (colors / fog / wave parameters; unnamed).
    pub params: [f32; 14],
    pub name: String,
    /// Shader path (v>=2) or default.
    pub shader: String,
    /// v1: ("skyTexture", value); v>=2: shader parameter (name, value) pairs.
    pub shader_params: Vec<(String, String)>,
    pub texture: String,
    /// v>=3: 3 splash effects, wake texture, object texture, body texture.
    pub effects: Vec<String>,
    /// v>=4: surface/physics type (default 7, lava 9).
    pub surface: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct WaterBlock {
    /// Block index in the layer grid (x = idx % grid_w, z = idx / grid_w).
    pub index: u32,
    /// 100 bytes copied verbatim into WaterBlockBase+88 (layout unknown; stock
    /// samples are often all-NaN floats).
    pub data: [u8; 100],
}

#[derive(Debug, Clone)]
pub struct WaterLayer {
    pub water_type: String,
    /// Surface height (region-local y).
    pub height: f32,
    pub uv: [f32; 2],
    pub blocks: Vec<WaterBlock>,
}

#[derive(Debug, Clone)]
pub struct WaterPath {
    pub water_type: String,
    /// The surface height (region-local y).
    pub value: f32,
    /// Control points; 64 bytes each on disk, first 28 used (7 floats, [`ControlPoint::from_raw`]).
    pub points: Vec<[f32; 16]>,
}

impl WaterPath {
    /// The control points' used fields.
    pub fn control_points(&self) -> impl Iterator<Item = ControlPoint> + '_ {
        self.points.iter().map(ControlPoint::from_raw)
    }
}

/// A water path's control point (`GAME::ControlPoint`, 28 bytes), all in the region's x / z plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlPoint {
    /// The centre line's point.
    pub pos: [f32; 2],
    /// The Bézier handle: the segment from here has `pos + tangent` as its second point, the one ending here
    /// `pos - tangent` as its third. The flow runs along it.
    pub tangent: [f32; 2],
    /// The unit side direction, `(-tangent.z, tangent.x) / |tangent|` in stock data: the bank at `pos + side * width`.
    pub side: [f32; 2],
    /// Half the river's width here.
    pub width: f32,
}

impl ControlPoint {
    pub fn from_raw(p: &[f32; 16]) -> Self {
        Self {
            pos: [p[0], p[1]],
            tangent: [p[2], p[3]],
            side: [p[4], p[5]],
            width: p[6],
        }
    }
}

#[derive(Debug, Clone)]
pub struct WaterWaveSet {
    pub value: f32,
    /// 56 bytes; first 4 floats used.
    pub params: [f32; 14],
    pub points: Vec<[f32; 16]>,
}

#[derive(Debug, Clone)]
pub struct Water {
    pub version: u32,
    pub types: Vec<WaterType>,
    pub layers: Vec<WaterLayer>,
    pub paths: Vec<WaterPath>,
    pub wave_sets: Vec<WaterWaveSet>,
}

fn floats<const N: usize>(r: &mut Reader) -> Result<[f32; N]> {
    let mut a = [0f32; N];
    for x in &mut a {
        *x = r.f32()?;
    }
    Ok(a)
}

impl Water {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let magic = r.u32()?;
        if magic != WATER_MAGIC {
            bail!("bad water magic {magic:#x}");
        }
        let version = r.u32()?;
        let ntypes = r.u32()?;
        let nlayers = r.u32()?;
        let npaths = r.u32()?;
        let nwaves = if version >= 2 { r.u32()? } else { 0 };
        let mut types = vec![];
        for _ in 0..ntypes {
            types.push(read_type(r)?);
        }
        let mut layers = vec![];
        for _ in 0..nlayers {
            let water_type = r.string()?;
            let height = r.f32()?;
            let uv = [r.f32()?, r.f32()?];
            let n = r.u32()?;
            let mut blocks = Vec::with_capacity(r.capacity(n as usize, 104));
            for _ in 0..n {
                blocks.push(WaterBlock {
                    index: r.u32()?,
                    data: r.bytes(100)?.try_into().unwrap(),
                });
            }
            layers.push(WaterLayer {
                water_type,
                height,
                uv,
                blocks,
            });
        }
        let mut paths = vec![];
        for _ in 0..npaths {
            let water_type = r.string()?;
            let value = r.f32()?;
            let n = r.u32()?;
            let mut points = vec![];
            for _ in 0..n {
                points.push(floats::<16>(r)?);
            }
            paths.push(WaterPath {
                water_type,
                value,
                points,
            });
        }
        let mut wave_sets = vec![];
        for _ in 0..nwaves {
            let value = r.f32()?;
            let params = floats::<14>(r)?;
            let n = r.u32()?;
            let mut points = vec![];
            for _ in 0..n {
                points.push(floats::<16>(r)?);
            }
            wave_sets.push(WaterWaveSet {
                value,
                params,
                points,
            });
        }
        Ok(Self {
            version,
            types,
            layers,
            paths,
            wave_sets,
        })
    }
}

fn read_type(r: &mut Reader) -> Result<WaterType> {
    let version = r.u32()?;
    let params = floats::<14>(r)?;
    let name = r.string()?;
    let mut shader = String::new();
    let mut shader_params = vec![];
    if version == 1 {
        shader_params.push(("skyTexture".to_string(), r.string()?));
    } else {
        shader = r.string()?;
        let n = r.u32()?;
        for _ in 0..n {
            shader_params.push((r.string()?, r.string()?));
        }
    }
    let texture = r.string()?;
    let mut effects = vec![];
    if version >= 3 {
        for _ in 0..6 {
            effects.push(r.string()?);
        }
    }
    let surface = if version >= 4 { Some(r.u32()?) } else { None };
    Ok(WaterType {
        version,
        params,
        name,
        shader,
        shader_params,
        texture,
        effects,
        surface,
    })
}
