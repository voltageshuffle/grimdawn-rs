//! Walkability and path finding over the levels' Detour tile-cache layers.
//! Each layer is a 2.5D grid of 0.25 m cells; we path
//! on the cells directly instead of rebuilding Detour polygons.
//!
//! Cell connectivity comes from the layer `cons` bytes (Recast direction order
//! dx = [-1, 0, 1, 0], dz = [0, 1, 0, -1]). The high nibble marks a portal to
//! another layer of the same tile; tile borders carry no bits, so neighbours
//! across tiles and through portals are matched by height (within `CLIMB`).
//!
//! Dynamic obstacles (`NavManager::AddDynamicObstacle`: closed doors' mesh hit
//! boxes, `NavBlocker` cylinders, blocking set-piece objects) make the cells
//! whose centre they cover unenterable ([`NavWorld::set_obstacles`]); a cell
//! already inside one can still be left. (Engine `NavManager::AddDynamicObstacle`
//! → `NavMesh::AddObstacleCylinder` / `NavMesh::AddObstacleBox`, tile build
//! `NavMesh::BuildTile`): the obstacle areas are marked after the walkable erosion,
//! so they get no agent margin; a cylinder spans `max(height, 3)` from half
//! of that below its base to that above it; a box marks its xz footprint
//! (the parallelogram of its first axis and the sum of the other two) from
//! twice its vertical half extent below its centre to one above.
//!
//! Path search ([`NavWorld::find_path`]) is A* over cells. PORT:
//! Detour searches polygons, a few hundred for a long path; a cell search of a long detour expands over 100k cells,
//! so the search goes coarse to fine. [`NavWorld::settle`] splits each layer into block nodes (the cells of an
//! 8 × 8 block joined within it); A* over block nodes gives a corridor (the path's nodes and their neighbours) and
//! the cell search stays inside it. Measured near Devil's Crossing: paths about 2 % longer than an unrestricted
//! cell search's (worst 22 %), strict searches half the time, detours past the old budget found. Dynamic obstacles
//! are seen by the cell search only: a corridor that does not reach the goal falls back to the whole nav, unless
//! (strict) an obstacle covers the goal cell or the obstacles shut the goal in (a flood from it within a quarter of
//! the budget, `NavWorld::enclosed`; the static connections are symmetric, every step of the Devil's Crossing nav
//! has its way back). A strict search toward another component fails at once ([`NavWorld::connected`]).
use gd_formats::collections::{HashMap, HashSet};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::marker::PhantomData;

const DX: [i32; 4] = [-1, 0, 1, 0];
const DZ: [i32; 4] = [0, 1, 0, -1];
/// Max step height between neighbouring cells of different layers/tiles.
pub const CLIMB: f32 = 0.9;
/// Tile edge length in world units.
pub const TILE: f32 = 32.0;

#[derive(Debug, Clone)]
pub struct NavLayer {
    pub tile: (i32, i32),
    pub bmin: [f32; 3],
    pub cs: f32,
    pub ch: f32,
    pub w: u32,
    pub h: u32,
    pub heights: Vec<u8>,
    pub areas: Vec<u8>,
    pub cons: Vec<u8>,
}

impl NavLayer {
    /// From a tile-cache layer header + payload. `ch` is the global cell height.
    pub fn from_tile(t: &gd_formats::map::pathing::TileLayer, ch: f32) -> Option<NavLayer> {
        let h = &t.header;
        let (w, hh) = (h.width as u32, h.height as u32);
        let n = (w * hh) as usize;
        if t.data.len() < n * 3 {
            return None;
        }
        Some(NavLayer {
            tile: (h.tx, h.ty),
            bmin: h.bmin,
            cs: (h.bmax[0] - h.bmin[0]) / w as f32,
            ch,
            w,
            h: hh,
            heights: t.data[..n].to_vec(),
            areas: t.data[n..2 * n].to_vec(),
            cons: t.data[2 * n..3 * n].to_vec(),
        })
    }

    fn idx(&self, x: i32, z: i32) -> Option<usize> {
        (x >= 0 && z >= 0 && (x as u32) < self.w && (z as u32) < self.h)
            .then(|| (z as u32 * self.w + x as u32) as usize)
    }
    pub fn walkable(&self, x: i32, z: i32) -> bool {
        self.idx(x, z)
            .is_some_and(|i| self.areas[i] != 0 && self.heights[i] != 0xff)
    }
    pub fn y(&self, i: usize) -> f32 {
        self.bmin[1] + self.heights[i] as f32 * self.ch
    }
    pub fn center(&self, x: i32, z: i32) -> [f32; 3] {
        let i = self.idx(x, z).unwrap_or(0);
        [
            self.bmin[0] + (x as f32 + 0.5) * self.cs,
            self.y(i),
            self.bmin[2] + (z as f32 + 0.5) * self.cs,
        ]
    }
}

/// A walkable cell: (layer slot, x, z). It borrows the [`NavWorld`] that
/// found it, so [`NavWorld::remove`] (which frees and reuses layer slots)
/// cannot run while a cell exists: a cell never names an unloaded layer.
///
/// ```compile_fail,E0502
/// let mut nav = gd_sim::nav::NavWorld::default();
/// let cell = nav.locate([0.0; 3]);
/// nav.remove(0);
/// assert!(cell.is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cell<'w> {
    pub layer: u32,
    pub x: i16,
    pub z: i16,
    world: PhantomData<&'w NavWorld>,
}

impl<'w> Cell<'w> {
    fn new(layer: u32, x: i32, z: i32) -> Self {
        Self {
            layer,
            x: x as i16,
            z: z as i16,
            world: PhantomData,
        }
    }

    /// The cell without its world borrow, for [`PathScratch`].
    fn key(self) -> CellKey {
        CellKey {
            layer: self.layer,
            x: self.x,
            z: self.z,
        }
    }

    /// A key of this world's search back into a cell (only keys the same search produced).
    fn from_key(k: CellKey) -> Self {
        Self::new(k.layer, k.x as i32, k.z as i32)
    }
}

/// A cell's identity without the world borrow: what [`PathScratch`] keeps between searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CellKey {
    layer: u32,
    x: i16,
    z: i16,
}

#[derive(Default)]
pub struct NavWorld {
    layers: Vec<Option<NavLayer>>,
    free: Vec<u32>,
    by_tile: HashMap<(i32, i32), Vec<u32>>,
    /// Owner key (e.g. region index) → layer slots, for unloading.
    owners: HashMap<u64, Vec<u32>>,
    /// Owner key → dynamic obstacles.
    obstacles: HashMap<u64, Vec<Obstacle>>,
    /// 4 m grid cell → (owner, index) of the obstacles overlapping it.
    obstacle_grid: HashMap<(i32, i32), Vec<(u64, usize)>>,
    /// Which components the loaded layers form ([`NavWorld::settle`]).
    conn: Conn,
}

/// A block node's component within its layer ([`BlockNode::comp`]).
const NO_COMP: u8 = 0xff;
/// Past this many components in one layer the rest share this id, which no strict search rejects on.
const MANY_COMPS: u8 = 0xfe;

/// Cells per side of a block, the unit of the coarse search (2 m at 0.25 m cells).
const BLOCK: i32 = 8;
/// A cell's block node ([`LayerConn::cell_node`]): none (not walkable). A layer is at most 255 cells a side, so
/// 32 × 32 blocks of at most 32 nodes each.
const NO_NODE: u16 = u16::MAX;
/// A coarse search expands at most a 16th of the cell budget in block nodes: about four times the area the cell
/// search covers, so it finds the long detours the cell search gives up on.
const COARSE_SHARE: usize = 16;
/// The enclosure check of a strict search (`NavWorld::enclosed`) lists at most a 4th of the cell budget: a room
/// behind a closed door up to about 900 m² at 0.25 m cells.
const ENCLOSED_SHARE: usize = 4;

/// A cell where a move can leave its layer, across the layer's edge or through a portal (direction `d`).
#[derive(Clone, Copy)]
struct Exit {
    node: u16,
    x: i16,
    z: i16,
    d: u8,
}

/// The walkable cells of one block joined by the moves within it: a node of the coarse search.
struct BlockNode {
    /// Its component within the layer.
    comp: u8,
    /// The centroid of its cells.
    pos: [f32; 3],
    /// Its range of [`LayerConn::adj`].
    adj: (u32, u32),
    /// Its range of [`LayerConn::exits`].
    exits: (u32, u32),
}

/// A block node of a coarse search: (layer slot, node).
type BlockKey = (u32, u16);

/// What the coarse pass of a search found ([`NavWorld::coarse`]).
enum Coarse {
    /// No coarse pass: a layer is not labelled yet.
    Unknown,
    /// A strict search's goal not reached within the budget.
    Failed,
    /// A corridor to the goal.
    Reached,
    /// A partial search's corridor to the explored node nearest the goal.
    Nearest,
}

/// One layer's connectivity: its cells' block nodes, the nodes' neighbours and components, its exits, and the
/// components of other layers they reach.
struct LayerConn {
    cell_node: Vec<u16>,
    nodes: Vec<BlockNode>,
    /// The block nodes of this layer that each node borders.
    adj: Vec<u16>,
    /// Grouped by node.
    exits: Vec<Exit>,
    /// (this layer's component, other slot, its generation, its component), each once.
    links: Vec<(u8, u32, u32, u8)>,
}

impl LayerConn {
    /// Component of cell `i`: [`NO_COMP`] when not walkable.
    fn comp(&self, i: usize) -> u8 {
        match self.cell_node[i] {
            NO_NODE => NO_COMP,
            n => self.nodes[n as usize].comp,
        }
    }
}

/// Work of [`NavWorld::settle`]: label a layer, resolve one exit of a labelled layer. (slot, generation, …)
enum ConnJob {
    Label(u32, u32),
    Resolve(u32, u32, u32),
}

/// A labelled exit: (slot, generation, exit index).
type ExitRef = (u32, u32, u32);
type TileSlots<'a> = Option<((i32, i32), &'a [u32])>;

/// The loaded nav's components (module doc of [`NavWorld::settle`]).
#[derive(Default)]
struct Conn {
    /// Per slot: its generation (bumped when its layer goes) and its connectivity once labelled.
    slots: Vec<(u32, Option<LayerConn>)>,
    pending: std::collections::VecDeque<ConnJob>,
    /// Target tile → the labelled exits stepping into it.
    into: HashMap<(i32, i32), Vec<ExitRef>>,
    /// (slot, component) → its component's root; `None` until rebuilt after a change.
    roots: Option<HashMap<(u32, u8), (u32, u8)>>,
}

/// Obstacle grid cell size.
const OBSTACLE_CELL: f32 = 4.0;

/// Grid cells one obstacle may cover (bad data guard: 256 m square; stock
/// doors and blockers cover a few).
const MAX_OBSTACLE_CELLS: i64 = 64 * 64;

/// A dynamic obstacle.
#[derive(Debug, Clone, PartialEq)]
pub enum Obstacle {
    /// Oriented box: centre, unit axes, half extents.
    Box {
        center: [f32; 3],
        axes: [[f32; 3]; 3],
        half: [f32; 3],
    },
    /// Upright cylinder standing on `base`.
    Cylinder {
        base: [f32; 3],
        radius: f32,
        height: f32,
    },
}

impl Obstacle {
    /// xz bounds (min x, min z, max x, max z).
    fn bounds(&self) -> [f32; 4] {
        match self {
            Obstacle::Box { center, axes, half } => {
                let ex = axes
                    .iter()
                    .zip(half)
                    .map(|(a, h)| a[0].abs() * h)
                    .sum::<f32>();
                let ez = axes
                    .iter()
                    .zip(half)
                    .map(|(a, h)| a[2].abs() * h)
                    .sum::<f32>();
                [
                    center[0] - ex,
                    center[2] - ez,
                    center[0] + ex,
                    center[2] + ez,
                ]
            }
            Obstacle::Cylinder { base, radius, .. } => [
                base[0] - radius,
                base[2] - radius,
                base[0] + radius,
                base[2] + radius,
            ],
        }
    }

    /// Whether the walkable surface at `p` lies in the marked area.
    pub fn covers(&self, p: [f32; 3]) -> bool {
        match self {
            Obstacle::Box { center, axes, half } => {
                let ey: f32 = axes.iter().zip(half).map(|(a, h)| (a[1] * h).abs()).sum();
                if p[1] < center[1] - 2.0 * ey || p[1] > center[1] + ey {
                    return false;
                }
                // Footprint: centre ± u ± w, u = axis 0, w = axes 1 + 2 (xz).
                let u = [axes[0][0] * half[0], axes[0][2] * half[0]];
                let w = [
                    axes[1][0] * half[1] + axes[2][0] * half[2],
                    axes[1][2] * half[1] + axes[2][2] * half[2],
                ];
                let det = u[0] * w[1] - u[1] * w[0];
                if det.abs() < 1e-6 {
                    return false;
                }
                let d = [p[0] - center[0], p[2] - center[2]];
                let s = (d[0] * w[1] - d[1] * w[0]) / det;
                let t = (u[0] * d[1] - u[1] * d[0]) / det;
                s.abs() <= 1.0 && t.abs() <= 1.0
            }
            Obstacle::Cylinder {
                base,
                radius,
                height,
            } => {
                let h = height.max(3.0);
                let (dx, dz) = (p[0] - base[0], p[2] - base[2]);
                dx * dx + dz * dz <= radius * radius
                    && p[1] >= base[1] - 0.5 * h
                    && p[1] <= base[1] + h
            }
        }
    }
}

/// An open-list entry of a search over cells (`K` = [`CellKey`]) or block nodes ([`BlockKey`]).
#[derive(Copy, Clone, PartialEq)]
struct Open<K> {
    f: f32,
    /// Cost so far when pushed; a pop with `g` above the best known is stale.
    g: f32,
    cell: K,
}
impl<K: PartialEq> Eq for Open<K> {}
impl<K: PartialEq> Ord for Open<K> {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.partial_cmp(&self.f).unwrap_or(Ordering::Equal)
    }
}
impl<K: PartialEq> PartialOrd for Open<K> {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// The scratch of a path search (the open heaps, the visited block nodes and cells, the corridor), reused between
/// searches: a caller keeps one in a `Local` and passes it to [`NavWorld::find_path`] /
/// [`NavWorld::find_path_strict`], so a search allocates nothing once it has grown.
#[derive(Default)]
pub struct PathScratch {
    open: BinaryHeap<Open<CellKey>>,
    /// Cell → (cost so far, parent) of the cell search.
    seen: HashMap<CellKey, (f32, CellKey)>,
    coarse_open: BinaryHeap<Open<BlockKey>>,
    /// Block node → (cost so far, parent) of the coarse search.
    coarse: HashMap<BlockKey, (f32, BlockKey)>,
    /// The block nodes the cell search may enter.
    corridor: HashSet<BlockKey>,
}

impl NavWorld {
    /// Global cell height of a set of layers: (bmax.y - bmin.y) / (hmax - hmin).
    pub fn cell_height(layers: &[gd_formats::map::pathing::TileLayer]) -> f32 {
        layers
            .iter()
            .find(|l| l.header.hmax > l.header.hmin)
            .map(|l| (l.header.bmax[1] - l.header.bmin[1]) / (l.header.hmax - l.header.hmin) as f32)
            .unwrap_or(0.2)
    }

    pub fn add(&mut self, owner: u64, layers: Vec<NavLayer>) {
        for l in layers {
            let slot = match self.free.pop() {
                Some(s) => {
                    self.layers[s as usize] = Some(l);
                    s
                }
                None => {
                    self.layers.push(Some(l));
                    (self.layers.len() - 1) as u32
                }
            };
            let tile = self.layers[slot as usize].as_ref().unwrap().tile;
            self.by_tile.entry(tile).or_default().push(slot);
            self.owners.entry(owner).or_default().push(slot);
            let conn = &mut self.conn;
            if conn.slots.len() <= slot as usize {
                conn.slots.resize_with(slot as usize + 1, || (0, None));
            }
            let generation = conn.slots[slot as usize].0;
            conn.pending.push_back(ConnJob::Label(slot, generation));
            conn.roots = None;
        }
    }

    /// Unloads `owner`'s layers; their slots are reused by later [`Self::add`]s.
    pub fn remove(&mut self, owner: u64) {
        for slot in self.owners.remove(&owner).unwrap_or_default() {
            if let Some(l) = self.layers[slot as usize].take() {
                if let Some(v) = self.by_tile.get_mut(&l.tile) {
                    v.retain(|&s| s != slot);
                }
                self.free.push(slot);
                // What still names this layer (links, exits into tiles, jobs) goes stale with its generation.
                let s = &mut self.conn.slots[slot as usize];
                s.0 = s.0.wrapping_add(1);
                s.1 = None;
                self.conn.roots = None;
            }
        }
    }

    /// Whether every loaded layer's connectivity is known ([`Self::settle`]).
    pub fn settled(&self) -> bool {
        self.conn.pending.is_empty() && self.conn.roots.is_some()
    }

    /// Works out which components the loaded layers form, for at most `budget` (a game system calls it every frame;
    /// new layers are known a few frames after they load). Each layer's cells are labelled by the moves within the
    /// layer, each exit (an edge or portal cell) is linked to every walkable cell of another layer it can step onto,
    /// and the links join the components. A link is made from either side and to every candidate within
    /// [`CLIMB`], a superset of the moves a search takes (one cell, the closest in height): two cells in different
    /// components cannot be joined by any path, whatever the dynamic obstacles. PORT: Detour's polygon search fails
    /// an unreachable goal after a few hundred polygons; the cell search here would expand its whole budget, so
    /// [`Self::find_path_strict`] rejects it up front ([`Self::connected`]).
    pub fn settle(&mut self, budget: std::time::Duration) {
        let start = std::time::Instant::now();
        while let Some(job) = self.conn.pending.pop_front() {
            match job {
                ConnJob::Label(slot, generation) => self.label(slot, generation),
                ConnJob::Resolve(slot, generation, exit) => self.resolve(slot, generation, exit),
            }
            if start.elapsed() >= budget {
                return;
            }
        }
        if self.conn.roots.is_none() {
            self.rebuild_roots();
        }
    }

    /// Labels the cells of layer `slot` with block nodes (each block's cells joined by the moves within the block),
    /// links the nodes that border each other, labels the nodes with the layer's components and lists its exits;
    /// queues the resolution of its exits and of the exits already stepping into its tile.
    fn label(&mut self, slot: u32, generation: u32) {
        if self.conn.slots[slot as usize].0 != generation {
            return;
        }
        let Some(l) = self.layers[slot as usize].as_ref() else {
            return;
        };
        let (w, h) = (l.w as i32, l.h as i32);
        let walk = |i: usize| l.areas[i] != 0 && l.heights[i] != 0xff;
        // Either cell's connection bit: the same test from both sides.
        let joined = |i: usize, j: usize, d: usize| {
            l.cons[i] & (1 << d) != 0 || l.cons[j] & (1 << ((d + 2) % 4)) != 0
        };
        let mut cell_node = vec![NO_NODE; (l.w * l.h) as usize];
        let mut nodes = Vec::new();
        let mut stack = Vec::new();
        let blocks = (0..h)
            .step_by(BLOCK as usize)
            .flat_map(|z| (0..w).step_by(BLOCK as usize).map(move |x| (z, x)));
        for (bz, bx) in blocks {
            let (x1, z1) = ((bx + BLOCK).min(w), (bz + BLOCK).min(h));
            for (z0, x0) in (bz..z1).flat_map(|z| (bx..x1).map(move |x| (z, x))) {
                let i0 = (z0 * w + x0) as usize;
                if cell_node[i0] != NO_NODE || !walk(i0) {
                    continue;
                }
                let id = nodes.len() as u16;
                cell_node[i0] = id;
                stack.push((x0, z0));
                let (mut sum, mut count) = ([0.0f32; 3], 0.0f32);
                while let Some((x, z)) = stack.pop() {
                    let c = l.center(x, z);
                    (0..3).for_each(|k| sum[k] += c[k]);
                    count += 1.0;
                    let i = (z * w + x) as usize;
                    for d in 0..4 {
                        let (nx, nz) = (x + DX[d], z + DZ[d]);
                        if nx < bx || nx >= x1 || nz < bz || nz >= z1 {
                            continue;
                        }
                        let j = (nz * w + nx) as usize;
                        if cell_node[j] == NO_NODE && walk(j) && joined(i, j, d) {
                            cell_node[j] = id;
                            stack.push((nx, nz));
                        }
                    }
                }
                nodes.push(BlockNode {
                    comp: NO_COMP,
                    pos: sum.map(|s| s / count),
                    adj: (0, 0),
                    exits: (0, 0),
                });
            }
        }
        let mut pairs = Vec::new();
        let mut exits = Vec::new();
        for (z, x) in (0..h).flat_map(|z| (0..w).map(move |x| (z, x))) {
            let i = (z * w + x) as usize;
            let a = cell_node[i];
            if a == NO_NODE {
                continue;
            }
            for d in 0..4 {
                let j = l.idx(x + DX[d], z + DZ[d]);
                if j.is_none() || l.cons[i] & (0x10 << d) != 0 {
                    exits.push(Exit {
                        node: a,
                        x: x as i16,
                        z: z as i16,
                        d: d as u8,
                    });
                }
                if let Some(j) = j
                    && cell_node[j] != NO_NODE
                    && cell_node[j] != a
                    && joined(i, j, d)
                {
                    pairs.push((a, cell_node[j]));
                }
            }
        }
        // Both directions of each pair were seen (the test is symmetric); sorted, they make each node's range.
        pairs.sort_unstable();
        pairs.dedup();
        exits.sort_by_key(|e| e.node);
        let adj: Vec<u16> = pairs.iter().map(|p| p.1).collect();
        let (mut pi, mut ei) = (0, 0);
        for (n, node) in nodes.iter_mut().enumerate() {
            let start = pi;
            while pairs.get(pi).is_some_and(|p| p.0 as usize == n) {
                pi += 1;
            }
            node.adj = (start as u32, pi as u32);
            let start = ei;
            while exits.get(ei).is_some_and(|e| e.node as usize == n) {
                ei += 1;
            }
            node.exits = (start as u32, ei as u32);
        }
        let mut next = 0u8;
        let mut stack = Vec::new();
        for n0 in 0..nodes.len() {
            if nodes[n0].comp != NO_COMP {
                continue;
            }
            let id = next;
            if next < MANY_COMPS {
                next += 1;
            }
            nodes[n0].comp = id;
            stack.push(n0);
            while let Some(a) = stack.pop() {
                let (s, e) = nodes[a].adj;
                for &b in &adj[s as usize..e as usize] {
                    if nodes[b as usize].comp == NO_COMP {
                        nodes[b as usize].comp = id;
                        stack.push(b as usize);
                    }
                }
            }
        }
        let tile = l.tile;
        for (e, ex) in exits.iter().enumerate() {
            let target = Self::exit_point(l, ex).1;
            self.conn
                .into
                .entry(target)
                .or_default()
                .push((slot, generation, e as u32));
            self.conn
                .pending
                .push_back(ConnJob::Resolve(slot, generation, e as u32));
        }
        // Exits of layers labelled before, stepping into this tile, may reach this layer.
        if let Some(v) = self.conn.into.get_mut(&tile) {
            let slots = &self.conn.slots;
            v.retain(|&(s, g, _)| slots[s as usize].0 == g);
            for &(s, g, e) in v.iter().filter(|&&(s, ..)| s != slot) {
                self.conn.pending.push_back(ConnJob::Resolve(s, g, e));
            }
        }
        self.conn.slots[slot as usize].1 = Some(LayerConn {
            cell_node,
            nodes,
            adj,
            exits,
            links: Vec::new(),
        });
        self.conn.roots = None;
    }

    /// Where exit `ex` of layer `l` steps: the point beside it (as [`Self::step_static`] makes it) and its tile.
    fn exit_point(l: &NavLayer, ex: &Exit) -> ([f32; 3], (i32, i32)) {
        let (x, z, d) = (ex.x as i32, ex.z as i32, ex.d as usize);
        let (nx, nz) = (x + DX[d], z + DZ[d]);
        let i = (z as u32 * l.w + x as u32) as usize;
        let p = [
            l.bmin[0] + (nx as f32 + 0.5) * l.cs,
            l.y(i),
            l.bmin[2] + (nz as f32 + 0.5) * l.cs,
        ];
        (
            p,
            ((p[0] / TILE).floor() as i32, (p[2] / TILE).floor() as i32),
        )
    }

    /// Links exit `exit` of layer `slot` to every labelled layer's walkable cell it can step onto.
    fn resolve(&mut self, slot: u32, generation: u32, exit: u32) {
        let Some((_, Some(lc))) = self
            .conn
            .slots
            .get(slot as usize)
            .filter(|s| s.0 == generation)
        else {
            return;
        };
        let l = self.layer(slot);
        let ex = lc.exits[exit as usize];
        let outside = l
            .idx(
                ex.x as i32 + DX[ex.d as usize],
                ex.z as i32 + DZ[ex.d as usize],
            )
            .is_none();
        let (p, tile) = Self::exit_point(l, &ex);
        let mut found = Vec::new();
        for &o in self.by_tile.get(&tile).into_iter().flatten() {
            if o == slot && !outside {
                continue;
            }
            let ol = self.layer(o);
            let (ox, oz) = (
                ((p[0] - ol.bmin[0]) / ol.cs).floor() as i32,
                ((p[2] - ol.bmin[2]) / ol.cs).floor() as i32,
            );
            let Some(oi) = ol.idx(ox, oz).filter(|_| ol.walkable(ox, oz)) else {
                continue;
            };
            if (ol.y(oi) - p[1]).abs() > CLIMB {
                continue;
            }
            let (og, Some(oc)) = &self.conn.slots[o as usize] else {
                // Not labelled yet: its own labelling resolves this exit again.
                continue;
            };
            found.push((lc.nodes[ex.node as usize].comp, o, *og, oc.comp(oi)));
        }
        let links = &mut self.conn.slots[slot as usize]
            .1
            .as_mut()
            .expect("checked above")
            .links;
        for f in found {
            if !links.contains(&f) {
                links.push(f);
                self.conn.roots = None;
            }
        }
    }

    /// Joins the components along every live link.
    fn rebuild_roots(&mut self) {
        let mut parent: HashMap<(u32, u8), (u32, u8)> = HashMap::default();
        fn find(p: &mut HashMap<(u32, u8), (u32, u8)>, mut k: (u32, u8)) -> (u32, u8) {
            while let Some(&up) = p.get(&k) {
                if up == k {
                    break;
                }
                let grand = p.get(&up).copied().unwrap_or(up);
                p.insert(k, grand);
                k = up;
            }
            k
        }
        let slots = &self.conn.slots;
        for (s, (_, lc)) in slots.iter().enumerate() {
            let Some(lc) = lc else { continue };
            for &(c, o, og, oc) in &lc.links {
                if slots[o as usize].0 != og || slots[o as usize].1.is_none() {
                    continue;
                }
                let (a, b) = (find(&mut parent, (s as u32, c)), find(&mut parent, (o, oc)));
                if a != b {
                    parent.insert(a, b);
                    parent.entry(b).or_insert(b);
                }
            }
        }
        let keys: Vec<(u32, u8)> = parent.keys().copied().collect();
        let mut roots = HashMap::default();
        for k in keys {
            let r = find(&mut parent, k);
            roots.insert(k, r);
        }
        self.conn.roots = Some(roots);
    }

    /// Whether `a` and `b` can be joined by some path: `Some(false)` only when they lie in different components,
    /// `None` while the connectivity is being worked out ([`Self::settle`]) or for a cell of a layer with more
    /// components than are counted.
    pub fn connected(&self, a: Cell<'_>, b: Cell<'_>) -> Option<bool> {
        if !self.conn.pending.is_empty() {
            return None;
        }
        let roots = self.conn.roots.as_ref()?;
        let comp = |c: Cell<'_>| {
            let l = self.layer(c.layer);
            let lc = self.conn.slots[c.layer as usize].1.as_ref()?;
            let k = lc.comp((c.z as u32 * l.w + c.x as u32) as usize);
            (k != NO_COMP && k != MANY_COMPS).then(|| {
                let key = (c.layer, k);
                roots.get(&key).copied().unwrap_or(key)
            })
        };
        Some(comp(a)? == comp(b)?)
    }

    /// Replaces the obstacles of `owner` (an empty list removes them).
    pub fn set_obstacles(&mut self, owner: u64, obstacles: Vec<Obstacle>) {
        if let Some(old) = self.obstacles.remove(&owner) {
            for o in &old {
                for k in Self::grid_cells(o) {
                    if let Some(v) = self.obstacle_grid.get_mut(&k) {
                        v.retain(|(w, _)| *w != owner);
                        if v.is_empty() {
                            self.obstacle_grid.remove(&k);
                        }
                    }
                }
            }
        }
        if obstacles.is_empty() {
            return;
        }
        for (i, o) in obstacles.iter().enumerate() {
            for k in Self::grid_cells(o) {
                self.obstacle_grid.entry(k).or_default().push((owner, i));
            }
        }
        self.obstacles.insert(owner, obstacles);
    }

    fn grid_cells(o: &Obstacle) -> impl Iterator<Item = (i32, i32)> {
        let b = o.bounds();
        let c = |v: f32| (v / OBSTACLE_CELL).floor() as i32;
        let (x0, z0, x1, z1) = (c(b[0]), c(b[1]), c(b[2]), c(b[3]));
        // Bad data: a huge radius / extent or a far centre would index up to
        // 2^64 cells; such an obstacle is left out of the grid.
        let n =
            (i64::from(x1) - i64::from(x0) + 1).saturating_mul(i64::from(z1) - i64::from(z0) + 1);
        (n <= MAX_OBSTACLE_CELLS)
            .then(|| (x0..=x1).flat_map(move |x| (z0..=z1).map(move |z| (x, z))))
            .into_iter()
            .flatten()
    }

    /// Whether a dynamic obstacle covers `p`.
    pub fn blocked(&self, p: [f32; 3]) -> bool {
        let k = (
            (p[0] / OBSTACLE_CELL).floor() as i32,
            (p[2] / OBSTACLE_CELL).floor() as i32,
        );
        self.obstacle_grid
            .get(&k)
            .is_some_and(|v| v.iter().any(|(w, i)| self.obstacles[w][*i].covers(p)))
    }

    /// A loaded layer: `slot` comes from a live [`Cell`] or from `by_tile`,
    /// both of which only name loaded layers.
    fn layer(&self, slot: u32) -> &NavLayer {
        self.layers[slot as usize]
            .as_ref()
            .expect("a cell or by_tile slot names a loaded layer")
    }

    /// Edge length of `c`'s cells.
    pub fn cell_size(&self, c: Cell<'_>) -> f32 {
        self.layer(c.layer).cs
    }

    pub fn pos(&self, c: Cell<'_>) -> [f32; 3] {
        self.layer(c.layer).center(c.x as i32, c.z as i32)
    }

    fn cell_y(&self, c: Cell<'_>) -> f32 {
        let l = self.layer(c.layer);
        l.y((c.z as u32 * l.w + c.x as u32) as usize)
    }

    /// Walkable cell under world (x, z) whose height is closest to `y`.
    pub fn locate(&self, p: [f32; 3]) -> Option<Cell<'_>> {
        let tile = ((p[0] / TILE).floor() as i32, (p[2] / TILE).floor() as i32);
        self.locate_in(self.by_tile.get(&tile)?, p)
    }

    /// [`Self::locate`] over an already-resolved tile's layer slots.
    fn locate_in(&self, slots: &[u32], p: [f32; 3]) -> Option<Cell<'_>> {
        let mut best: Option<(f32, Cell<'_>)> = None;
        for &slot in slots {
            let l = self.layer(slot);
            let x = ((p[0] - l.bmin[0]) / l.cs).floor() as i32;
            let z = ((p[2] - l.bmin[2]) / l.cs).floor() as i32;
            if !l.walkable(x, z) {
                continue;
            }
            let dy = (l.y(l.idx(x, z).unwrap()) - p[1]).abs();
            if best.is_none_or(|(b, _)| dy < b) {
                best = Some((dy, Cell::new(slot, x, z)));
            }
        }
        best.map(|b| b.1)
    }

    /// The walkable cell under world (x, z) with the highest floor at most `above` metres over `p`'s height: what a
    /// ray cast straight down from `above` metres over `p` meets first (`World::PutOnFloor`, `doors` teleports).
    pub fn floor_below(&self, p: [f32; 3], above: f32) -> Option<Cell<'_>> {
        let tile = ((p[0] / TILE).floor() as i32, (p[2] / TILE).floor() as i32);
        let top = p[1] + above;
        let mut best: Option<(f32, Cell<'_>)> = None;
        for &slot in self.by_tile.get(&tile)? {
            let l = self.layer(slot);
            let x = ((p[0] - l.bmin[0]) / l.cs).floor() as i32;
            let z = ((p[2] - l.bmin[2]) / l.cs).floor() as i32;
            if !l.walkable(x, z) {
                continue;
            }
            let y = l.y(l.idx(x, z).unwrap());
            if y <= top && best.is_none_or(|(b, _)| y > b) {
                best = Some((y, Cell::new(slot, x, z)));
            }
        }
        best.map(|b| b.1)
    }

    /// Nearest walkable cell within `radius` metres (spiral search). The
    /// spiral's points share a handful of tiles (`radius` ≪ [`TILE`]), so the
    /// tile → layers lookup is memoized (empty tiles included) instead of
    /// hashed per point: a failed endpoint (scenery / void) costs a few
    /// lookups, not one per probe. Probe order and results are unchanged.
    pub fn nearest(&self, p: [f32; 3], radius: f32) -> Option<Cell<'_>> {
        if let Some(c) = self.locate(p) {
            return Some(c);
        }
        let step = 0.25;
        let rings = (radius / step).ceil() as i32;
        let mut tiles: [TileSlots<'_>; 4] = [None; 4];
        let mut next = 0;
        for r in 1..=rings {
            let mut best: Option<(f32, Cell<'_>)> = None;
            for i in -r..=r {
                for (dx, dz) in [(i, -r), (i, r), (-r, i), (r, i)] {
                    let q = [p[0] + dx as f32 * step, p[1], p[2] + dz as f32 * step];
                    let tile = ((q[0] / TILE).floor() as i32, (q[2] / TILE).floor() as i32);
                    let slots = match tiles.iter().find_map(|t| match *t {
                        Some((k, slots)) if k == tile => Some(slots),
                        _ => None,
                    }) {
                        Some(slots) => slots,
                        None => {
                            let slots = self
                                .by_tile
                                .get(&tile)
                                .map(Vec::as_slice)
                                .unwrap_or_default();
                            tiles[next] = Some((tile, slots));
                            next = (next + 1) % tiles.len();
                            slots
                        }
                    };
                    if let Some(c) = self.locate_in(slots, q) {
                        let d = (dx * dx + dz * dz) as f32;
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, c));
                        }
                    }
                }
            }
            if let Some((_, c)) = best {
                return Some(c);
            }
        }
        None
    }

    /// Ground height at (x, z) nearest to `y`: bilinear across the cell centres
    /// (a cell's stored height is its centre sample), continuous over cell borders
    /// instead of stepping per cell. PORT: the engine's crowd agent height is
    /// Detour's interpolated polygon height (`getPolyHeight`); the port interpolates
    /// the cell grid. A neighbour off the layer or not walkable contributes the
    /// located cell's height (tile borders keep their step).
    pub fn ground(&self, p: [f32; 3]) -> Option<f32> {
        let c = self.locate(p)?;
        let l = self.layer(c.layer);
        let base = self.cell_y(c);
        // Fractional position in cell-centre coordinates.
        let u = (p[0] - l.bmin[0]) / l.cs - 0.5;
        let v = (p[2] - l.bmin[2]) / l.cs - 0.5;
        let (x0, z0) = (u.floor() as i32, v.floor() as i32);
        let (tu, tv) = (u - x0 as f32, v - z0 as f32);
        let h = |x: i32, z: i32| {
            if l.walkable(x, z) {
                l.y(l.idx(x, z).unwrap())
            } else {
                base
            }
        };
        let (h00, h10) = (h(x0, z0), h(x0 + 1, z0));
        let (h01, h11) = (h(x0, z0 + 1), h(x0 + 1, z0 + 1));
        Some((h00 * (1.0 - tu) + h10 * tu) * (1.0 - tv) + (h01 * (1.0 - tu) + h11 * tu) * tv)
    }

    /// The neighbour of `c` in direction `d` (0..4), if connected.
    pub fn step<'w>(&'w self, c: Cell<'w>, d: usize) -> Option<Cell<'w>> {
        let n = self.step_static(c, d)?;
        (self.obstacles.is_empty() || !self.blocked(self.pos(n))).then_some(n)
    }

    fn step_static<'w>(&'w self, c: Cell<'w>, d: usize) -> Option<Cell<'w>> {
        let l = self.layer(c.layer);
        let (x, z) = (c.x as i32, c.z as i32);
        let i = l.idx(x, z)?;
        let (nx, nz) = (x + DX[d], z + DZ[d]);
        if l.cons[i] & (1 << d) != 0 && l.walkable(nx, nz) {
            return Some(Cell::new(c.layer, nx, nz));
        }
        let portal = l.cons[i] & (0x10 << d) != 0;
        let outside = l.idx(nx, nz).is_none();
        if !portal && !outside {
            return None;
        }
        // Another layer of this tile, or a layer of the neighbouring tile.
        let p = [
            l.bmin[0] + (nx as f32 + 0.5) * l.cs,
            l.y(i),
            l.bmin[2] + (nz as f32 + 0.5) * l.cs,
        ];
        let tile = ((p[0] / TILE).floor() as i32, (p[2] / TILE).floor() as i32);
        let mut best: Option<(f32, Cell<'w>)> = None;
        for &slot in self.by_tile.get(&tile)? {
            if slot == c.layer && !outside {
                continue;
            }
            let o = self.layer(slot);
            let ox = ((p[0] - o.bmin[0]) / o.cs).floor() as i32;
            let oz = ((p[2] - o.bmin[2]) / o.cs).floor() as i32;
            if !o.walkable(ox, oz) {
                continue;
            }
            let dy = (o.y(o.idx(ox, oz).unwrap()) - p[1]).abs();
            if dy <= CLIMB && best.is_none_or(|(b, _)| dy < b) {
                best = Some((dy, Cell::new(slot, ox, oz)));
            }
        }
        best.map(|b| b.1)
    }

    /// A* from `from` to `to` over cells (8-connected; a diagonal needs both
    /// orthogonal steps). Expands at most `max_nodes`. Returns cell centres,
    /// smoothed by line of sight. When `to` cannot be reached (an obstacle, a
    /// gap) the path leads to the explored cell closest to it; `None` only
    /// without a start cell or when no step is possible. This is the game's
    /// partial mode (Engine `NavManager::FindPath` → `NavManager::HasPathTo`:
    /// Detour `findPath` keeps the path to the explored polygon nearest the
    /// goal, accepted only with the `allowPartial` flag, which Game.dll passes
    /// from `GetFurthestMoveToPoint`, `ControllerMonster::AngerUpdate` and
    /// `DefaultRequestInteractableAction`); plain moves use
    /// [`NavWorld::find_path_strict`].
    pub fn find_path(
        &self,
        scratch: &mut PathScratch,
        from: [f32; 3],
        to: [f32; 3],
        max_nodes: usize,
    ) -> Option<Vec<[f32; 3]>> {
        self.search(scratch, from, to, max_nodes, true)
    }

    /// [`NavWorld::find_path`] without the partial result: `None` when `to`
    /// is not reached (`CharacterMovementManager::FindPath`
    /// passes `allowPartial` false, failing the move; the
    /// player then walks to `NavManager::FindStraightMovePoint` instead,
    /// `ControllerPlayerState::DefaultRequestMoveAction`).
    pub fn find_path_strict(
        &self,
        scratch: &mut PathScratch,
        from: [f32; 3],
        to: [f32; 3],
        max_nodes: usize,
    ) -> Option<Vec<[f32; 3]>> {
        self.search(scratch, from, to, max_nodes, false)
    }

    /// The search: the coarse pass over block nodes, then the cell search kept to its corridor. When the corridor
    /// does not lead to a goal the blocks reach (an obstacle such as a closed door, which the blocks do not see),
    /// the cell search runs again over the whole nav.
    fn search(
        &self,
        scratch: &mut PathScratch,
        from: [f32; 3],
        to: [f32; 3],
        max_nodes: usize,
        partial: bool,
    ) -> Option<Vec<[f32; 3]>> {
        let start = self.nearest(from, 2.0)?;
        let goal = self.nearest(to, 3.0)?;
        // A goal no path can reach fails a strict search at once ([`Self::settle`]); the partial search still goes
        // to the explored cell nearest it.
        if !partial && self.connected(start, goal) == Some(false) {
            return None;
        }
        // An obstacle covering the goal cell keeps every path out of it.
        if !partial && start != goal && !self.obstacles.is_empty() && self.blocked(self.pos(goal)) {
            return None;
        }
        let fine = |scratch: &mut PathScratch, corridor| {
            self.fine(scratch, start, goal, max_nodes, partial, corridor)
        };
        match self.coarse(scratch, start, goal, max_nodes, partial) {
            Coarse::Unknown => fine(scratch, false).map(|p| p.0),
            Coarse::Failed => None,
            Coarse::Nearest => fine(scratch, true).map(|p| p.0),
            Coarse::Reached => match fine(scratch, true) {
                Some((p, true)) => Some(p),
                _ if !partial
                    && self.enclosed(scratch, goal, start, max_nodes / ENCLOSED_SHARE) =>
                {
                    None
                }
                _ => fine(scratch, false).map(|p| p.0),
            },
        }
    }

    /// Whether the obstacles shut `goal` in: every cell a path could reach it from (the cells stepping out of it
    /// reach, the connections being symmetric) is listed within `budget` without meeting `start`. A strict search
    /// toward a goal behind a closed door then fails at once instead of exhausting its budget over the whole nav.
    fn enclosed(
        &self,
        scratch: &mut PathScratch,
        goal: Cell<'_>,
        start: Cell<'_>,
        budget: usize,
    ) -> bool {
        let PathScratch { open, seen, .. } = scratch;
        open.clear();
        seen.clear();
        seen.insert(goal.key(), (0.0, goal.key()));
        open.push(Open {
            f: 0.0,
            g: 0.0,
            cell: goal.key(),
        });
        while let Some(Open { cell: key, .. }) = open.pop() {
            if seen.len() > budget {
                return false;
            }
            let cell = Cell::from_key(key);
            for d in 0..4 {
                // A path may leave an obstacle-covered start cell: its way back in counts.
                let Some(n) = self
                    .step_static(cell, d)
                    .filter(|&n| n == start || !self.blocked(self.pos(n)))
                else {
                    continue;
                };
                if n == start {
                    return false;
                }
                if let std::collections::hash_map::Entry::Vacant(v) = seen.entry(n.key()) {
                    v.insert((0.0, key));
                    open.push(Open {
                        f: 0.0,
                        g: 0.0,
                        cell: n.key(),
                    });
                }
            }
        }
        true
    }

    /// The block node of `c`, once its layer is labelled ([`Self::settle`]).
    fn block_of(&self, c: Cell<'_>) -> Option<BlockKey> {
        let lc = self.conn.slots[c.layer as usize].1.as_ref()?;
        let n = lc.cell_node[(c.z as u32 * self.layer(c.layer).w + c.x as u32) as usize];
        (n != NO_NODE).then_some((c.layer, n))
    }

    /// A labelled block node ([`Self::block_of`]).
    fn block_node(&self, k: BlockKey) -> &BlockNode {
        &self.conn.slots[k.0 as usize]
            .1
            .as_ref()
            .expect("a block key names a labelled layer")
            .nodes[k.1 as usize]
    }

    /// The block nodes one move leads to from `k`: the nodes of its layer it borders (by the cells' connection
    /// bits, as the layer's components) and those its exits step onto (as the cell search steps).
    fn block_neighbours(&self, k: BlockKey, mut f: impl FnMut(BlockKey)) {
        let Some(lc) = self.conn.slots[k.0 as usize].1.as_ref() else {
            return;
        };
        let n = &lc.nodes[k.1 as usize];
        for &m in &lc.adj[n.adj.0 as usize..n.adj.1 as usize] {
            f((k.0, m));
        }
        for ex in &lc.exits[n.exits.0 as usize..n.exits.1 as usize] {
            let c = Cell::new(k.0, ex.x as i32, ex.z as i32);
            if let Some(o) = self
                .step_static(c, ex.d as usize)
                .and_then(|o| self.block_of(o))
            {
                f(o);
            }
        }
    }

    /// The coarse pass: A* over block nodes from `start`'s to `goal`'s, expanding at most `max_nodes /`
    /// [`COARSE_SHARE`]; the corridor (`scratch.corridor`) is the path's nodes and their neighbours. A partial search
    /// that does not reach the goal leads to the explored node nearest it. Dynamic obstacles are left to the cell
    /// search.
    fn coarse(
        &self,
        scratch: &mut PathScratch,
        start: Cell<'_>,
        goal: Cell<'_>,
        max_nodes: usize,
        partial: bool,
    ) -> Coarse {
        // Every loaded layer labelled, so every move leads to a block node.
        if !self.conn.pending.is_empty() {
            return Coarse::Unknown;
        }
        let (Some(s), Some(t)) = (self.block_of(start), self.block_of(goal)) else {
            return Coarse::Unknown;
        };
        let b = self.pos(goal);
        let dist =
            |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let PathScratch {
            coarse_open: open,
            coarse: seen,
            corridor,
            ..
        } = scratch;
        open.clear();
        seen.clear();
        corridor.clear();
        seen.insert(s, (0.0, s));
        open.push(Open {
            f: dist(self.block_node(s).pos, b),
            g: 0.0,
            cell: s,
        });
        let mut closest = (f32::INFINITY, s);
        let mut expanded = 0;
        let mut reached = false;
        while let Some(Open { cell: k, g, .. }) = open.pop() {
            if g > seen[&k].0 {
                continue;
            }
            if k == t {
                reached = true;
                break;
            }
            let pk = self.block_node(k).pos;
            let hk = dist(pk, b);
            if hk < closest.0 {
                closest = (hk, k);
            }
            expanded += 1;
            if expanded > max_nodes / COARSE_SHARE {
                break;
            }
            self.block_neighbours(k, |m| {
                let pm = self.block_node(m).pos;
                let ng = g + dist(pk, pm);
                if seen.get(&m).is_none_or(|e| ng < e.0) {
                    seen.insert(m, (ng, k));
                    open.push(Open {
                        f: ng + dist(pm, b),
                        g: ng,
                        cell: m,
                    });
                }
            });
        }
        if !reached && !partial {
            return Coarse::Failed;
        }
        let mut k = if reached { t } else { closest.1 };
        loop {
            corridor.insert(k);
            self.block_neighbours(k, |m| {
                corridor.insert(m);
            });
            if k == s {
                break;
            }
            k = seen[&k].1;
        }
        if reached {
            Coarse::Reached
        } else {
            Coarse::Nearest
        }
    }

    /// A* over cells from `start` toward `goal`, kept to `scratch.corridor` with `corridor`; the smoothed path and
    /// whether it reaches `goal`.
    fn fine(
        &self,
        scratch: &mut PathScratch,
        start: Cell<'_>,
        goal: Cell<'_>,
        max_nodes: usize,
        partial: bool,
        corridor: bool,
    ) -> Option<(Vec<[f32; 3]>, bool)> {
        let b = self.pos(goal);
        let h = |c: Cell<'_>| {
            let a = self.pos(c);
            ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        };
        let PathScratch {
            open,
            seen,
            corridor: blocks,
            ..
        } = scratch;
        open.clear();
        seen.clear();
        seen.insert(start.key(), (0.0, start.key()));
        open.push(Open {
            f: h(start),
            g: 0.0,
            cell: start.key(),
        });
        let mut expanded = 0;
        let mut found = start == goal;
        let mut closest = (h(start), start);
        while let Some(Open { cell: key, g, .. }) = open.pop() {
            let cell = Cell::from_key(key);
            let gc = seen[&key].0;
            if g > gc {
                // Stale entry: the cell was re-pushed with a better cost.
                continue;
            }
            if cell == goal {
                found = true;
                break;
            }
            let hc = h(cell);
            if hc < closest.0 {
                closest = (hc, cell);
            }
            expanded += 1;
            if expanded > max_nodes {
                break;
            }
            let cs = self.layer(cell.layer).cs;
            let mut ortho = [None; 4];
            for (d, slot) in ortho.iter_mut().enumerate() {
                *slot = self.step(cell, d);
            }
            let mut cand = [(cell, 0.0f32); 8];
            let mut n_cand = 0;
            for n in ortho.iter().flatten() {
                cand[n_cand] = (*n, cs);
                n_cand += 1;
            }
            for d in 0..4 {
                let e = (d + 1) % 4;
                if let (Some(a), Some(_)) = (ortho[d], ortho[e])
                    && let Some(diag) = self.step(a, e)
                {
                    cand[n_cand] = (diag, cs * std::f32::consts::SQRT_2);
                    n_cand += 1;
                }
            }
            // A move within the block stays in the corridor.
            let here = if corridor { self.block_of(cell) } else { None };
            for &(n, cost) in &cand[..n_cand] {
                if corridor {
                    let k = self.block_of(n);
                    if k != here && !k.is_some_and(|k| blocks.contains(&k)) {
                        continue;
                    }
                }
                let ng = gc + cost;
                if seen.get(&n.key()).is_none_or(|&(old, _)| ng < old) {
                    seen.insert(n.key(), (ng, key));
                    open.push(Open {
                        f: ng + h(n),
                        g: ng,
                        cell: n.key(),
                    });
                }
            }
        }
        if !found && !partial {
            return None;
        }
        let end = if found { goal } else { closest.1 };
        if end == start && !found {
            return None;
        }
        let mut cells = vec![end];
        let mut c = end;
        while c != start {
            c = Cell::from_key(seen[&c.key()].1);
            cells.push(c);
        }
        cells.reverse();
        Some((self.smooth(&cells), found))
    }

    /// Greedy string pulling: keep the farthest cell, at most `SMOOTH_REACH`
    /// cells ahead, reachable in a straight walkable line from the anchor.
    fn smooth(&self, cells: &[Cell<'_>]) -> Vec<[f32; 3]> {
        /// Look-ahead cap: bounds `line_walkable` calls to O(n·reach).
        const SMOOTH_REACH: usize = 32;
        let mut out = Vec::with_capacity(cells.len().min(16));
        out.push(self.pos(cells[0]));
        let mut anchor = 0;
        while anchor < cells.len() - 1 {
            let mut next = anchor + 1;
            let last = (anchor + SMOOTH_REACH).min(cells.len() - 1);
            for j in (anchor + 2..=last).rev() {
                if self.line_walkable(cells[anchor], cells[j]) {
                    next = j;
                    break;
                }
            }
            out.push(self.pos(cells[next]));
            anchor = next;
        }
        out
    }

    /// Walks the straight line between two cells in quarter-cell steps,
    /// following connections; true if it reaches `b` without leaving the mesh.
    pub fn line_walkable<'w>(&'w self, a: Cell<'w>, b: Cell<'w>) -> bool {
        let (pa, pb) = (self.pos(a), self.pos(b));
        let len = ((pb[0] - pa[0]).powi(2) + (pb[2] - pa[2]).powi(2)).sqrt();
        let n = (len / 0.0625).ceil().max(1.0) as usize;
        let mut cur = a;
        for k in 1..=n {
            let t = k as f32 / n as f32;
            let q = [
                pa[0] + (pb[0] - pa[0]) * t,
                0.0,
                pa[2] + (pb[2] - pa[2]) * t,
            ];
            let l = self.layer(cur.layer);
            let (cx, cz) = (
                ((q[0] - l.bmin[0]) / l.cs).floor() as i32,
                ((q[2] - l.bmin[2]) / l.cs).floor() as i32,
            );
            if cx == cur.x as i32 && cz == cur.z as i32 && l.idx(cx, cz).is_some() {
                continue;
            }
            // Move one cell at a time; a diagonal crossing must be passable both ways.
            let mut moved = None;
            let (ddx, ddz) = ((cx - cur.x as i32).signum(), (cz - cur.z as i32).signum());
            // At most two directions match (one per axis).
            let mut dirs = [0usize; 2];
            let mut ndirs = 0;
            for d in 0..4 {
                if (DX[d] == ddx && DX[d] != 0) || (DZ[d] == ddz && DZ[d] != 0) {
                    dirs[ndirs] = d;
                    ndirs += 1;
                }
            }
            if ndirs == 2 {
                let (s1, s2) = (self.step(cur, dirs[0]), self.step(cur, dirs[1]));
                if let (Some(a1), Some(_)) = (s1, s2) {
                    moved = self.step(a1, dirs[1]);
                }
            } else if ndirs == 1 {
                moved = self.step(cur, dirs[0]);
            }
            match moved {
                Some(m) => cur = m,
                None => return false,
            }
        }
        cur == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(tile: (i32, i32), blocked: &[(i32, i32)]) -> NavLayer {
        let (w, h) = (128u32, 128u32);
        let n = (w * h) as usize;
        let mut areas = vec![63u8; n];
        for &(x, z) in blocked {
            areas[(z as u32 * w + x as u32) as usize] = 0;
        }
        let mut cons = vec![0u8; n];
        for z in 0..h as i32 {
            for x in 0..w as i32 {
                let i = (z as u32 * w + x as u32) as usize;
                if areas[i] == 0 {
                    continue;
                }
                for d in 0..4 {
                    let (nx, nz) = (x + DX[d], z + DZ[d]);
                    if nx >= 0
                        && nz >= 0
                        && nx < w as i32
                        && nz < h as i32
                        && areas[(nz as u32 * w + nx as u32) as usize] != 0
                    {
                        cons[i] |= 1 << d;
                    }
                }
            }
        }
        NavLayer {
            tile,
            bmin: [tile.0 as f32 * TILE, 0.0, tile.1 as f32 * TILE],
            cs: 0.25,
            ch: 0.2,
            w,
            h,
            heights: vec![0; n],
            areas,
            cons,
        }
    }

    #[test]
    fn paths_around_a_wall_and_across_tiles() {
        let wall: Vec<(i32, i32)> = (0..120).map(|z| (64, z)).collect();
        let mut nav = NavWorld::default();
        nav.add(0, vec![flat((0, 0), &wall), flat((1, 0), &[])]);
        let mut scratch = PathScratch::default();
        let p = nav
            .find_path(&mut scratch, [2.0, 0.0, 2.0], [40.0, 0.0, 2.0], 200_000)
            .expect("path");
        // Must detour around the wall's open end near z = 30..32.
        assert!(p.iter().any(|q| q[2] > 29.5), "{p:?}");
        let last = p.last().unwrap();
        assert!((last[0] - 40.0).abs() < 0.3 && (last[2] - 2.0).abs() < 0.3);
        // Straight line through the open region: every waypoint on the one line (`smooth` looks at most
        // `SMOOTH_REACH` cells ahead, so a long line comes back in collinear pieces).
        let s = nav
            .find_path(&mut scratch, [2.0, 0.0, 31.0], [20.0, 0.0, 31.0], 200_000)
            .unwrap();
        assert!(s.iter().all(|q| (q[2] - s[0][2]).abs() < 1e-3), "{s:?}");
    }
}
