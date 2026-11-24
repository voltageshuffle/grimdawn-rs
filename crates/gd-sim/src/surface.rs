//! What a character stands on: the `PhysicsSurface` of a level's terrain and
//! water at a point (the world's ray: `gd_game::collision::WorldCollision::surface_below`).
//!
//! (Engine.dll): `TerrainType::Load` reads the terrain
//! record's `surfaceType` by exact name ([`surface_type`]; anything else,
//! also nothing, 100). `Terrain::GetIntersection` of a block (`TerrainBlock::GetIntersection`,
//! 9 x 9 cell blocks) finds the cell the ray hits and walks the block's layers
//! from the top ([`Ground::terrain_surface`]). Those layers are every layer
//! with its bit set at any vertex of the block (`TerrainRT::GetBlockLayers`, the block's
//! `TerrainBlock.layers` list; the mask's bit i + 1 is layer i, bit 0 the cell's visibility).
//! A layer's opacity at a cell is its weight byte, layer 0 is always 255
//! (`TerrainBlock::GetLayerOpacity`).
//!
//! Water: `Water::GetIntersection` tries every layer and path,
//! the type's own surface does not matter: any hit is surface 7, a lava river
//! too. `WaterLayer::RayIntersection` meets the layer's plane
//! and `IsPointInLayer` wants that point in a block whose coverage byte is not
//! 0 (10 x 10 over the 9 m block). `WaterPath::RayIntersection`
//! meets the plane at the path's height and looks for a segment holding the
//! point ([`in_path_segment`]).
use gd_formats::map::terrain::Terrain;
use gd_formats::map::water::{Water, WaterPath};

/// No surface (no hit, a hit box, an unnamed terrain type): `Intersection`'s
/// default.
pub const NO_SURFACE: u32 = 100;

/// `PhysicsSurface` values with a foot sound (the `AnimationCallback`
/// switch; others play and place nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Grass = 1,
    Dirt = 2,
    Sand = 3,
    Snow = 4,
    Stone = 5,
    Wood = 6,
    Water = 7,
    Mud = 10,
}

impl Surface {
    /// The surface of a `PhysicsSurface` value, if it has foot sounds
    /// (Unwalkable 0, Lava 9 and [`NO_SURFACE`] have none).
    pub fn footstep(code: u32) -> Option<Surface> {
        Some(match code {
            1 => Surface::Grass,
            2 => Surface::Dirt,
            3 => Surface::Sand,
            4 => Surface::Snow,
            5 => Surface::Stone,
            6 => Surface::Wood,
            7 => Surface::Water,
            10 => Surface::Mud,
            _ => return None,
        })
    }

    /// The name in the `foot<Surface>` / `footPrint<Surface>` fields.
    pub fn name(self) -> &'static str {
        match self {
            Surface::Grass => "Grass",
            Surface::Dirt => "Dirt",
            Surface::Sand => "Sand",
            Surface::Snow => "Snow",
            Surface::Stone => "Stone",
            Surface::Wood => "Wood",
            Surface::Water => "Water",
            Surface::Mud => "Mud",
        }
    }
}

/// `TerrainType::Load`: the `PhysicsSurface` of a `surfaceType` value.
pub fn surface_type(name: &str) -> u32 {
    match name {
        "Unwalkable" => 0,
        "Grass" => 1,
        "Dirt" => 2,
        "Sand" => 3,
        "Snow" => 4,
        "Stone" => 5,
        "Wood" => 6,
        "Water" => 7,
        "Lava" => 9,
        "Mud" => 10,
        _ => NO_SURFACE,
    }
}

/// Cells per side of a terrain block (and metres of a water block).
const BLOCK: u32 = 9;

/// What decides a level's surfaces, level-local (a clone of its map data, the
/// heights left out: `gd_game::collision` has them).
pub struct Ground {
    terrain: Option<Terrain>,
    /// [`surface_type`] of each terrain layer.
    layer_surfaces: Vec<u32>,
    water: Option<Water>,
    /// Water blocks along x (`Level::NewWater`: one per 9 cells of terrain).
    water_grid: u32,
}

impl Ground {
    /// `surface_of` gives a terrain layer's record's [`surface_type`].
    pub fn new(
        terrain: Option<&Terrain>,
        water: Option<&Water>,
        surface_of: impl Fn(&str) -> u32,
    ) -> Self {
        let layer_surfaces = terrain
            .map(|t| t.layers.iter().map(|l| surface_of(&l.name)).collect())
            .unwrap_or_default();
        let water_grid = terrain.map_or(1, |t| t.width.saturating_sub(1).max(1).div_ceil(BLOCK));
        let terrain = terrain.map(|t| Terrain {
            header0: t.header0,
            width: t.width,
            height: t.height,
            header_rest: t.header_rest,
            heights: Vec::new(),
            layer_mask: t.layer_mask.clone(),
            layers: t.layers.clone(),
            trailer: t.trailer,
        });
        Ground {
            terrain,
            layer_surfaces,
            water: water.cloned(),
            water_grid,
        }
    }

    /// The surface of the terrain cell (`x`, `z`): from the top layer down,
    /// `v` = this layer's opacity × (1 − the previous `v`), stopping once the
    /// best so far is more than what is left, and the layer with the largest
    /// `v` counts; the block's lowest layer when none shows.
    pub fn terrain_surface(&self, x: u32, z: u32) -> u32 {
        let Some(t) = &self.terrain else {
            return NO_SURFACE;
        };
        let in_block = block_layer_test(t, x, z);
        let n = t.layers.len();
        let (mut v, mut best, mut pick) = (0.0f32, 0.0f32, None);
        for layer in (0..n).rev().filter(|l| in_block(*l)) {
            let left = 1.0 - v;
            if best > left {
                break;
            }
            v = opacity(t, layer, x, z) * 0.003_921_569 * left;
            if v > best {
                pick = Some(layer);
                best = v;
            }
        }
        pick.or_else(|| (0..n).find(|l| in_block(*l)))
            .and_then(|l| self.layer_surfaces.get(l))
            .copied()
            .unwrap_or(NO_SURFACE)
    }

    /// How far below `from_y` the nearest water surface at (`x`, `z`) is,
    /// closer than `reach`.
    pub fn water_below(&self, x: f32, z: f32, from_y: f32, reach: f32) -> Option<f32> {
        let w = self.water.as_ref()?;
        let down = |h: f32| Some(from_y - h).filter(|d| (0.0..reach).contains(d));
        let layers = w
            .layers
            .iter()
            .filter_map(|l| down(l.height).filter(|_| in_water_layer(l, self.water_grid, x, z)));
        let paths = w
            .paths
            .iter()
            .filter_map(|p| down(p.value).filter(|_| in_path(p, x, z)));
        layers.chain(paths).min_by(f32::total_cmp)
    }
}

/// Whether a layer is present in the block holding cell (`x`, `z`).
fn block_layer_test(t: &Terrain, x: u32, z: u32) -> impl Fn(usize) -> bool + '_ {
    let (w, n) = (t.width as usize, t.layers.len());
    let per_vertex = n / 8 + 1;
    let (x0, z0) = (x - x % BLOCK, z - z % BLOCK);
    let (x1, z1) = (
        (x0 + BLOCK).min(t.width - 2),
        (z0 + BLOCK).min(t.height - 2),
    );
    let set = move |layer: usize, vx: u32, vz: u32| {
        let bit = layer + 1;
        t.layer_mask
            .get((vz as usize * w + vx as usize) * per_vertex + bit / 8)
            .is_some_and(|m| m & (1 << (bit % 8)) != 0)
    };
    move |l| (z0..=z1).any(|vz| (x0..=x1).any(|vx| set(l, vx, vz)))
}

/// A layer's opacity byte at a cell.
fn opacity(t: &Terrain, layer: usize, x: u32, z: u32) -> f32 {
    let cell = (x + z * (t.width - 1)) as usize;
    match t.layers[layer].weights.as_ref() {
        Some(w) => w.get(cell).copied().unwrap_or(0) as f32,
        None => 255.0,
    }
}

/// `IsPointInLayer`: the point's block exists and covers it.
fn in_water_layer(l: &gd_formats::map::water::WaterLayer, grid: u32, x: f32, z: f32) -> bool {
    let size = BLOCK as f32;
    if x < 0.0 || z < 0.0 || x >= (grid * BLOCK) as f32 {
        return false;
    }
    let (bx, bz) = (x / size, z / size);
    // Bad data / far point: a z past the blocks must not overflow the index.
    let Some(index) = (bz as u32)
        .checked_mul(grid)
        .and_then(|i| i.checked_add(bx as u32))
    else {
        return false;
    };
    let cell = |f: f32| ((f.fract() * 10.0) as usize).min(9);
    l.blocks
        .iter()
        .find(|b| b.index == index)
        .is_some_and(|b| b.data[cell(bx) + 10 * cell(bz)] != 0)
}

/// Whether (`x`, `z`) is in any segment of a path.
fn in_path(p: &WaterPath, x: f32, z: f32) -> bool {
    p.points
        .windows(2)
        .any(|s| in_path_segment([x, z], &s[0], &s[1]))
}

/// `WaterPath::IsPointInSegment`: a control point is [x, z, tangent x, tangent z,
/// _, _, half width]. The point must lie between the two ends' planes across
/// their tangents, and within the half width, lerped along the curve, of the
/// Bézier (p0, p0 + t0, p1 − t1, p1) at the nearest parameter (`WaterPath::FindNearestParameter`:
/// bisected to 0.05, the curve's direction over a step of 0.025 telling which
/// way).
pub fn in_path_segment(q: [f32; 2], a: &[f32; 16], b: &[f32; 16]) -> bool {
    let side = |p: &[f32; 16]| (q[1] - p[1]) * p[3] + (q[0] - p[0]) * p[2];
    if side(a) < 0.0 || side(b) > 0.0 {
        return false;
    }
    let c = [
        [a[0], a[1]],
        [a[0] + a[2], a[1] + a[3]],
        [b[0] - b[2], b[1] - b[3]],
        [b[0], b[1]],
    ];
    let at = |t: f32| {
        let s = 1.0 - t;
        let w = [s * s * s, 3.0 * t * s * s, 3.0 * t * t * s, t * t * t];
        [0, 1].map(|i| (0..4).map(|k| w[k] * c[k][i]).sum::<f32>())
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    let mut t = 0.5;
    while hi - lo > 0.05 {
        let (p, next) = (at(t), at(t + 0.025));
        if (next[0] - p[0]) * (q[0] - p[0]) + (next[1] - p[1]) * (q[1] - p[1]) <= 0.0 {
            hi = t;
        } else {
            lo = t;
        }
        t = (lo + hi) * 0.5;
    }
    let p = at(t);
    let half = t * b[6] + (1.0 - t) * a[6];
    half * half > (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)
}
