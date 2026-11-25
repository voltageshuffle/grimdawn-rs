//! World geometry a projectile collides with.
//!
//! Game.dll: a projectile is a physics entity
//! (`ProjectileBase::GetCollisionType` 2 / 3) and the physics engine reports
//! its contact with level objects and terrain through
//! `ProjectileFireballLike::CollisionCallback` (the collision is
//! with the objects' physics meshes, Engine.dll `PhysicsMesh_Bullet`; terrain
//! through `Terrain::GetIntersection`'s heightfield). Contact with anything
//! but another projectile is a "miss": `ProjectileStop` (state 3), no
//! damage; the next update runs `ProcessExplosion` only when the record's
//! `explodeOnMiss` is set, then deletes it (`ProjectileFireballLike::
//! UpdateSelf`).
//!
//! The physics mesh is the `.msh` chunk 2 blob (`gd_formats::msh::PhysicsMesh`, whose module docs give the layout).
//!
//! Which level objects get a body: `Actor::InitialUpdate` (Engine.dll)
//! calls `Entity::SetPhysicsSimulation(1)` when `IsStatic()`,
//! `IsInWorld()` and `GetIsPartOfLevel()`; Game.dll `Decoration::IsStatic` is
//! `!allowPathing` and `Decoration::GetIsPartOfLevel` is "empty `description`".
//! Props, destructibles, fixed items (doors, shrines, chests), NPCs and
//! monsters keep the `false` defaults, so only such `Decoration` records are
//! solid (`gd_world` picks them). `Actor::PhysicsSetup` mode 1
//! builds the body from the mesh's physics mesh (`GraphicsMesh::
//! GetPhysicsMesh`, chunk 2) when it has one, otherwise from every hit box
//! whose name is not `Visibility` / `Transparency` (so `BlockLOS` boxes are
//! solid too; `GraphicsMeshInstance::CheckLOS` only uses them for sight
//! checks, never for projectiles), the box half sizes and origins scaled by
//! the entity scale. The body's collision group is 1 and its mask 0x7C
//! (includes the projectile group 64; `ProjectileBase` bodies use group 64,
//! mask 1, plus 64 when `collidesWithProjectiles`).
//!
//! No body: intact `Destructible`s only get a nav obstacle
//! (`SetBlocked` → `SetDynamicObstacle`); their `PhysicsSetup` runs for the
//! dynamic debris once `BreakApart` sets simulation mode 2. `FixedItem*` /
//! `FixedDoor` / `DynamicDoor` only create nav obstacles from their hit boxes
//! (`CreatePathObstacle(s)`), and `TerrainDecoration` only gets a body through
//! `Decoration::IsStatic` with its generated geometry (the port has none).
//! No Game.dll projectile code queries anything but the physics world.
//!
//! PORT: the game's physics world is a rigid-body simulation (Bullet); here a
//! projectile's swept segment is tested against those triangles / boxes and
//! the terrain heightfield. Closed doors' hit boxes also stop projectiles
//! because that is what the game was observed to do; the decompile shows no
//! physics body behind it (mechanism not found).
use bevy::math::Affine3A;
use bevy::prelude::*;
use gd_formats::collections::HashMap;
use gd_formats::msh::{Mesh as Msh, PhysicsMesh};
use gd_sim::nav::Obstacle;
use gd_sim::surface::{Ground, NO_SURFACE};
use std::collections::BTreeMap;
use std::ops::ControlFlow;
use std::sync::Arc;

enum Shape {
    Mesh {
        mesh: Arc<PhysicsMesh>,
        inv: Affine3A,
    },
    /// World-space hit boxes.
    Boxes(Vec<Obstacle>),
}

struct Instance {
    shape: Shape,
    min: Vec3,
    max: Vec3,
}

impl Instance {
    /// First contact of the segment `a → b` with this object: the segment
    /// parameter, the `PhysicsSurface` and what was hit.
    fn first_hit(&self, a: Vec3, b: Vec3) -> Option<(f32, u32, &'static str)> {
        self.first_hit_normal(a, b).map(|(t, s, k, _)| (t, s, k))
    }

    /// [`Self::first_hit`] with the face's world normal, turned towards `a`
    /// (a hit box gives −(b − a)).
    fn first_hit_normal(&self, a: Vec3, b: Vec3) -> Option<(f32, u32, &'static str, Vec3)> {
        let d = b - a;
        if !seg_aabb(a, d, self.min, self.max) {
            return None;
        }
        let mut best: Option<(f32, u32, &'static str, Vec3)> = None;
        let back = -d.normalize_or(Vec3::NEG_Y);
        match &self.shape {
            Shape::Mesh { mesh, inv } => {
                let (la, ld) = (inv.transform_point3(a), inv.transform_vector3(d));
                if !seg_aabb(la, ld, mesh.min.into(), mesh.max.into()) {
                    return None;
                }
                for (t, surface) in mesh.tris.iter().zip(&mesh.surfaces) {
                    let v = t.map(|i| Vec3::from(mesh.verts[i as usize]));
                    if let Some(s) = seg_tri(la, ld, v)
                        && best.is_none_or(|x| s < x.0)
                    {
                        // Local → world normal: the inverse transpose of the placement.
                        let local = (v[1] - v[0]).cross(v[2] - v[0]);
                        let n = Mat3::from(inv.matrix3)
                            .transpose()
                            .mul_vec3(local)
                            .normalize_or(back);
                        let n = if n.dot(back) < 0.0 { -n } else { n };
                        best = Some((s, *surface, "object", n));
                    }
                }
            }
            Shape::Boxes(boxes) => {
                for o in boxes {
                    if let Some(s) = segment_obstacle(a, b, o)
                        && best.is_none_or(|x| s < x.0)
                    {
                        best = Some((s, NO_SURFACE, "object hit box", back));
                    }
                }
            }
        }
        best
    }
}

/// What a mesh path resolves to once: its physics mesh, else its hit boxes.
#[derive(Clone)]
enum MeshKind {
    Physics(Arc<PhysicsMesh>),
    HitBoxes,
    None,
}

/// The bin side (m) of a level's object index.
const BIN: f32 = 8.0;
/// Objects spanning more bins than this per axis go to [`Bins::wide`], always tested.
const MAX_SPAN: i64 = 16;
/// A query covering more bins than this tests every object (in index order) instead.
const MAX_QUERY: i64 = 64;

/// A level's objects with a uniform XZ grid over their world boxes. Queries visit the candidates of the bins the
/// query box touches in ascending index order, so first-hit ties resolve exactly as a full scan would.
#[derive(Default)]
struct Bins {
    items: Vec<Instance>,
    cells: HashMap<(i64, i64), Vec<u32>>,
    wide: Vec<u32>,
}

fn bin_range(lo: f32, hi: f32) -> (i64, i64) {
    ((lo / BIN).floor() as i64, (hi / BIN).floor() as i64)
}

impl Bins {
    fn new(items: Vec<Instance>) -> Self {
        let mut b = Self::default();
        for ins in items {
            b.push(ins);
        }
        b
    }

    fn push(&mut self, ins: Instance) {
        let i = self.items.len() as u32;
        if ins.min.cmple(ins.max).all() {
            let (x0, x1) = bin_range(ins.min.x, ins.max.x);
            let (z0, z1) = bin_range(ins.min.z, ins.max.z);
            if x1 - x0 >= MAX_SPAN || z1 - z0 >= MAX_SPAN {
                self.wide.push(i);
            } else {
                for z in z0..=z1 {
                    for x in x0..=x1 {
                        self.cells.entry((x, z)).or_default().push(i);
                    }
                }
            }
        }
        // An empty box (no solid hit box) is never hit: not binned.
        self.items.push(ins);
    }

    /// Visits (index ascending) every object whose world box may meet the box `min ..= max`.
    fn near(
        &self,
        min: Vec3,
        max: Vec3,
        mut f: impl FnMut(usize, &Instance) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        let (x0, x1) = bin_range(min.x, max.x);
        let (z0, z1) = bin_range(min.z, max.z);
        let mut visit = |i: u32| f(i as usize, &self.items[i as usize]);
        if (x1 - x0 + 1).saturating_mul(z1 - z0 + 1) > MAX_QUERY {
            for i in 0..self.items.len() as u32 {
                visit(i)?;
            }
            return ControlFlow::Continue(());
        }
        if x0 == x1 && z0 == z1 {
            // One bin: merge its sorted list with `wide`, no allocation.
            let cell = self.cells.get(&(x0, z0)).map_or(&[][..], Vec::as_slice);
            let (mut a, mut b) = (cell.iter().peekable(), self.wide.iter().peekable());
            loop {
                let i = match (a.peek(), b.peek()) {
                    (Some(&&x), Some(&&y)) if x < y => a.next(),
                    (Some(_), None) => a.next(),
                    (_, Some(_)) => b.next(),
                    (None, None) => break,
                };
                visit(*i.unwrap())?;
            }
            return ControlFlow::Continue(());
        }
        let mut ids: Vec<u32> = self.wide.clone();
        for z in z0..=z1 {
            for x in x0..=x1 {
                if let Some(c) = self.cells.get(&(x, z)) {
                    ids.extend_from_slice(c);
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        for i in ids {
            visit(i)?;
        }
        ControlFlow::Continue(())
    }

    /// [`Self::near`] for the segment `a → b`.
    fn along(
        &self,
        a: Vec3,
        b: Vec3,
        f: impl FnMut(usize, &Instance) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        self.near(a.min(b), a.max(b), f)
    }
}

struct LevelCollision {
    origin: Vec3,
    width: u32,
    height: u32,
    heights: Vec<f32>,
    instances: Bins,
    /// The other scenery objects: only the ground a character stands on ([`WorldCollision::add_surfaces`]).
    floors: Bins,
    /// The `BlockLOS` hit boxes of the level's objects ([`WorldCollision::los_clear`]).
    los: Bins,
    /// What its terrain and water are made of ([`WorldCollision::surface_below`]).
    ground: Option<Ground>,
}

#[derive(Resource, Default)]
pub struct WorldCollision {
    /// Ordered by level index: overlapping levels resolve deterministically.
    levels: BTreeMap<usize, LevelCollision>,
    meshes: HashMap<String, MeshKind>,
}

/// Segment (a → b, `len` long) against an AABB: whether it may touch it.
fn seg_aabb(a: Vec3, d: Vec3, min: Vec3, max: Vec3) -> bool {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for i in 0..3 {
        if d[i].abs() < 1e-9 {
            if a[i] < min[i] || a[i] > max[i] {
                return false;
            }
        } else {
            let (x, y) = ((min[i] - a[i]) / d[i], (max[i] - a[i]) / d[i]);
            t0 = t0.max(x.min(y));
            t1 = t1.min(x.max(y));
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// Möller–Trumbore, double sided; the segment parameter in 0..=1.
fn seg_tri(a: Vec3, d: Vec3, p: [Vec3; 3]) -> Option<f32> {
    let (e1, e2) = (p[1] - p[0], p[2] - p[0]);
    let h = d.cross(e2);
    let det = e1.dot(h);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = a - p[0];
    let u = s.dot(h) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (0.0..=1.0).contains(&t).then_some(t)
}

impl LevelCollision {
    /// The heightfield's normal at (x, z): central differences of the
    /// bilinear height over ±1 m (the vertex normals' interpolation).
    fn normal(&self, p: Vec3) -> Option<Vec3> {
        let y = self.ground(p)?;
        let h = |dx: f32, dz: f32| self.ground(p + Vec3::new(dx, 0.0, dz)).unwrap_or(y);
        Some(Vec3::new(h(-1.0, 0.0) - h(1.0, 0.0), 2.0, h(0.0, -1.0) - h(0.0, 1.0)).normalize())
    }

    /// None off the heightfield, and for a level without terrain (no vertices).
    fn ground(&self, p: Vec3) -> Option<f32> {
        let (x, z) = (p.x - self.origin.x, p.z - self.origin.z);
        let (Some(mx), Some(mz)) = (self.width.checked_sub(1), self.height.checked_sub(1)) else {
            return None;
        };
        let (mx, mz) = (mx as f32, mz as f32);
        if x < 0.0 || z < 0.0 || x > mx || z > mz {
            return None;
        }
        let (x0, z0) = (x.floor() as u32, z.floor() as u32);
        let (x1, z1) = ((x0 + 1).min(self.width - 1), (z0 + 1).min(self.height - 1));
        let (fx, fz) = (x - x0 as f32, z - z0 as f32);
        let h = |a: u32, b: u32| self.heights[(b * self.width + a) as usize];
        let top = h(x0, z0) * (1.0 - fx) + h(x1, z0) * fx;
        let bot = h(x0, z1) * (1.0 - fx) + h(x1, z1) * fx;
        Some(self.origin.y + top * (1.0 - fz) + bot * fz)
    }
}

impl WorldCollision {
    /// Registers a level's terrain and scenery physics meshes.
    pub fn add_level(
        &mut self,
        index: usize,
        origin: Vec3,
        terrain: Option<(u32, u32, Vec<f32>)>,
        objects: impl Iterator<Item = (String, Arc<Msh>, Affine3A)>,
    ) {
        let (width, height, heights) = terrain.unwrap_or((0, 0, Vec::new()));
        let instances = Bins::new(
            objects
                .filter_map(|(path, msh, tf)| self.instance(path, &msh, tf))
                .collect(),
        );
        debug!(
            "collision level {index}: {} physics objects, terrain {}x{}",
            instances.items.len(),
            width,
            height
        );
        self.levels.insert(
            index,
            LevelCollision {
                origin,
                width,
                height,
                heights,
                instances,
                floors: Bins::default(),
                los: Bins::default(),
                ground: None,
            },
        );
    }

    /// Registers a level's scenery objects without a physics body (call after [`Self::add_level`]): what
    /// [`Self::surface_below`] finds besides the solid ones. `World::GetSurfaceTypeBelow`'s object
    /// test (Engine.dll `World::IntersectObjectsBelow`) takes every entity of the region's tree whose `GetVisibility`
    /// is 1 or 3 (an `Actor` with a mesh instance, not faded out) but the character itself, through
    /// `Actor::GetIntersection`
    /// → `GraphicsMeshInstance::GetIntersection` (the mesh's physics mesh, else its hit boxes,
    /// else its bounding box: surface 100), with a physics body or not: a `TerrainDecoration` bridge
    /// (`bridgestonea02cntr.msh`: stone deck faces) or an `allowPathing` floor tile is stood on like a solid
    /// `Decoration`. PORT: built like the solid objects ([`Self::instance`]: no bounding-box fallback,
    /// `Visibility` / `Transparency` boxes left out).
    pub fn add_surfaces(
        &mut self,
        index: usize,
        objects: impl Iterator<Item = (String, Arc<Msh>, Affine3A)>,
    ) {
        let floors = Bins::new(
            objects
                .filter_map(|(path, msh, tf)| self.instance(path, &msh, tf))
                .collect(),
        );
        if let Some(lv) = self.levels.get_mut(&index) {
            lv.floors = floors;
        }
    }

    /// One placed object's shape: its physics mesh, else its solid hit boxes (none: neither).
    fn instance(&mut self, path: String, msh: &Msh, tf: Affine3A) -> Option<Instance> {
        let kind = self
            .meshes
            .entry(path)
            .or_insert_with(
                || match msh.physics.as_ref().filter(|m| !m.tris.is_empty()) {
                    Some(m) => MeshKind::Physics(m.clone()),
                    None if solid_hit_boxes(msh).next().is_some() => MeshKind::HitBoxes,
                    None => MeshKind::None,
                },
            )
            .clone();
        let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let shape = match kind {
            MeshKind::None => return None,
            MeshKind::Physics(mesh) => {
                for i in 0..8 {
                    let c = Vec3::select(
                        BVec3::new(i & 1 != 0, i & 2 != 0, i & 4 != 0),
                        mesh.max.into(),
                        mesh.min.into(),
                    );
                    let w = tf.transform_point3(c);
                    min = min.min(w);
                    max = max.max(w);
                }
                Shape::Mesh {
                    mesh,
                    inv: tf.inverse(),
                }
            }
            MeshKind::HitBoxes => {
                let bind: Vec<_> = msh.bones.iter().map(|b| b.abs).collect();
                let boxes = crate::obstacles::placed_hit_boxes(msh, &Mat4::from(tf), &bind, |h| {
                    is_solid_box(&h.name)
                });
                (min, max) = boxes_bounds(&boxes);
                Shape::Boxes(boxes)
            }
        };
        Some(Instance { shape, min, max })
    }

    /// Registers a level's sight blockers: the `BlockLOS` hit boxes of its
    /// objects (call after [`Self::add_level`]).
    pub fn add_los_boxes(
        &mut self,
        index: usize,
        objects: impl Iterator<Item = (Arc<Msh>, Affine3A)>,
    ) {
        let Some(lv) = self.levels.get_mut(&index) else {
            return;
        };
        for (msh, tf) in objects {
            if !msh
                .hit_boxes
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case("BlockLOS"))
            {
                continue;
            }
            let bind: Vec<_> = msh.bones.iter().map(|b| b.abs).collect();
            let boxes = crate::obstacles::placed_hit_boxes(&msh, &Mat4::from(tf), &bind, |h| {
                h.name.eq_ignore_ascii_case("BlockLOS")
            });
            let (min, max) = boxes_bounds(&boxes);
            lv.los.push(Instance {
                shape: Shape::Boxes(boxes),
                min,
                max,
            });
        }
    }

    /// `World::CheckLOS` (Engine.dll): the segment
    /// `a → b` is clear unless it meets a `BlockLOS` hit box of a level
    /// object (`GraphicsMeshInstance::CheckLOS` tests exactly
    /// the mesh's hit boxes named `BlockLOS`, scaled by the instance scale;
    /// `Level::CheckLOS` walks the level's object tree, `World::CheckLOS`
    /// every loaded level). The terrain is not tested.
    pub fn los_clear(&self, a: Vec3, b: Vec3) -> bool {
        let d = b - a;
        self.levels.values().all(|lv| {
            lv.los
                .along(a, b, |_, ins| {
                    if seg_aabb(a, d, ins.min, ins.max)
                        && let Shape::Boxes(boxes) = &ins.shape
                        && boxes.iter().any(|o| segment_obstacle(a, b, o).is_some())
                    {
                        return ControlFlow::Break(());
                    }
                    ControlFlow::Continue(())
                })
                .is_continue()
        })
    }

    /// The terrain heightfield of the level under (x, z) of `p`: its origin,
    /// width, height (vertices) and level-local vertex heights (decals).
    pub fn terrain_at(&self, p: Vec3) -> Option<(Vec3, u32, u32, &[f32])> {
        self.levels
            .values()
            .find(|l| l.ground(p).is_some())
            .map(|l| (l.origin, l.width, l.height, l.heights.as_slice()))
    }

    /// The terrain's normal under (x, z) of `p` (`Terrain::GetNormalInterpolated`).
    pub fn terrain_normal(&self, p: Vec3) -> Option<Vec3> {
        self.levels.values().find_map(|l| l.normal(p))
    }

    /// The first physics surface straight below `top` within `depth` (the
    /// terrain or a scenery physics mesh / hit box): the contact point and the
    /// surface normal, what a falling rigid body lands on (the engine's
    /// Bullet world holds exactly these, module doc). An object `top` is
    /// already inside (a gib launched inside a spire's hit box) is no floor:
    /// counting its contact at the start would lift the body by its step
    /// every frame up to the box's top. PORT: Bullet would push such a body
    /// out of the box.
    pub fn floor_below(&self, top: Vec3, depth: f32) -> Option<(Vec3, Vec3)> {
        let (a, b) = (top, top - Vec3::Y * depth);
        let mut best: Option<(f32, Vec3)> = None;
        for lv in self.levels.values() {
            if lv.width > 1
                && let (Some(ga), Some(gb)) = (lv.ground(a), lv.ground(b))
                && b.y < gb
            {
                let (da, db) = (a.y - ga, b.y - gb);
                let t = if da > 0.0 { da / (da - db) } else { 0.0 };
                let t = t.clamp(0.0, 1.0);
                if best.is_none_or(|x| t < x.0) {
                    let n = lv.normal(a + (b - a) * t).unwrap_or(Vec3::Y);
                    best = Some((t, n));
                }
            }
            let _flow = lv.instances.along(a, b, |_, ins| {
                if let Some((t, _, _, n)) = ins.first_hit_normal(a, b)
                    && t > 0.0
                    && best.is_none_or(|x| t < x.0)
                {
                    best = Some((t, n));
                }
                ControlFlow::Continue(())
            });
        }
        best.map(|(t, n)| (a + (b - a) * t, n))
    }

    /// The terrain height under (x, z) of `p`, if a loaded level has terrain there.
    pub fn terrain_ground(&self, p: Vec3) -> Option<f32> {
        self.levels.values().find_map(|l| l.ground(p))
    }

    /// Registers what a level's terrain and water are made of (call after
    /// [`Self::add_level`]).
    pub fn add_ground(&mut self, index: usize, ground: Ground) {
        if let Some(l) = self.levels.get_mut(&index) {
            l.ground = Some(ground);
        }
    }

    /// The `PhysicsSurface` under a character at `p`, [`NO_SURFACE`] if
    /// nothing is in reach. `World::GetSurfaceTypeBelow`
    /// casts a ray from 1 m above the character straight down, 2 m long
    /// (`World::RaycastLevelsBelow`); of every level within reach the nearest of its
    /// terrain, its objects (closer than that) and its water (closer than
    /// both) counts. Terrain: [`Ground::terrain_surface`]; objects: the face's
    /// surface of the physics mesh (`PhysicsMesh::GetIntersection`),
    /// or [`NO_SURFACE`] for hit boxes
    /// (`GraphicsMeshInstance::GetIntersection`: no physics mesh); water
    /// always 7 ([`gd_sim::surface`]).
    ///
    /// PORT: the terrain is the bilinear height, not the cell's two triangles;
    /// the objects are the static scenery of [`Self::add_level`] and
    /// [`Self::add_surfaces`] (the engine's tree also holds characters, moving
    /// physics actors and an actor's bounding box if its mesh has no hit box,
    /// surface 100). A level without
    /// [`Ground`] (the test arena) is dirt.
    pub fn surface_below(&self, p: Vec3) -> u32 {
        const REACH: f32 = 2.0;
        let (a, b) = (p + Vec3::Y, p - Vec3::Y);
        let mut best = (f32::INFINITY, NO_SURFACE);
        for lv in self.levels.values() {
            let (mut near, mut surface) = (f32::INFINITY, NO_SURFACE);
            if lv.width > 1
                && let Some(g) = lv.ground(p)
                && (0.0..REACH).contains(&(a.y - g))
            {
                near = (a.y - g) / REACH;
                surface =
                    lv.ground
                        .as_ref()
                        .map_or(gd_sim::surface::Surface::Dirt as u32, |ground| {
                            let cell = |v: f32, size: u32| {
                                (v.floor().max(0.0) as u32).min(size.saturating_sub(2))
                            };
                            ground.terrain_surface(
                                cell(p.x - lv.origin.x, lv.width),
                                cell(p.z - lv.origin.z, lv.height),
                            )
                        });
            }
            for bins in [&lv.instances, &lv.floors] {
                let _flow = bins.along(a, b, |_, ins| {
                    if let Some((t, s, _)) = ins.first_hit(a, b)
                        && t < near
                    {
                        (near, surface) = (t, s);
                    }
                    ControlFlow::Continue(())
                });
            }
            if let Some(d) = lv.ground.as_ref().and_then(|g| {
                g.water_below(
                    p.x - lv.origin.x,
                    p.z - lv.origin.z,
                    a.y - lv.origin.y,
                    REACH,
                )
            }) && d / REACH < near
            {
                (near, surface) = (d / REACH, gd_sim::surface::Surface::Water as u32);
            }
            if near < best.0 {
                best = (near, surface);
            }
        }
        best.1
    }

    pub fn remove_level(&mut self, index: usize) {
        self.levels.remove(&index);
    }

    /// First contact of the segment `a → b` with the terrain or a scenery
    /// physics mesh: the point and what it was.
    pub fn segment_hit(&self, a: Vec3, b: Vec3) -> Option<(Vec3, &'static str)> {
        let d = b - a;
        let mut best: Option<(f32, &'static str)> = None;
        for lv in self.levels.values() {
            // Terrain: below the heightfield at the end (the step is short).
            if lv.width > 1
                && let (Some(ga), Some(gb)) = (lv.ground(a), lv.ground(b))
                && b.y < gb
            {
                // Where the segment crosses the surface (linear in the step).
                let (da, db) = (a.y - ga, b.y - gb);
                let t = if da > 0.0 { da / (da - db) } else { 0.0 };
                if best.is_none_or(|x| t < x.0) {
                    best = Some((t.clamp(0.0, 1.0), "terrain"));
                }
            }
            let _flow = lv.instances.along(a, b, |_, ins| {
                if let Some((t, _, kind)) = ins.first_hit(a, b)
                    && best.is_none_or(|x| t < x.0)
                {
                    best = Some((t, kind));
                }
                ControlFlow::Continue(())
            });
        }
        best.map(|(t, k)| (a + d * t, k))
    }

    /// The terrain and solid scenery triangles that may touch the world box `min ..= max`, appended to `out`:
    /// what a rigid body collides with (`gd_sim::drop_body`, module doc). The terrain is its heightfield's two
    /// triangles per 1 m cell, split along the (x + 1, z) – (x, z + 1) diagonal (`TerrainBase::
    /// PhysicsSetup` builds a `btHeightfieldTerrainShape` without flipped or diamond quad edges, whose
    /// `processAllTriangles` splits that way); the scenery its physics-mesh faces or its hit boxes' faces, placed.
    /// Each triangle carries its object (the terrain or a scenery object of a level), as Bullet keeps contacts
    /// per object pair.
    pub fn triangles_near(&self, min: Vec3, max: Vec3, out: &mut Vec<gd_sim::drop_body::Tri>) {
        let overlaps = |lo: Vec3, hi: Vec3| lo.cmple(max).all() && hi.cmpge(min).all();
        let tri = |v, owner| gd_sim::drop_body::Tri { v, owner };
        for (&index, lv) in &self.levels {
            let level = (index as u64) << 32;
            if lv.width > 1 && lv.height > 1 {
                let cells = |lo: f32, hi: f32, o: f32, n: u32| {
                    let a = ((lo - o).floor().max(0.0) as u32).min(n - 1);
                    let b = ((hi - o).ceil().max(0.0) as u32).min(n - 1);
                    a..b
                };
                let xs = cells(min.x, max.x, lv.origin.x, lv.width);
                let zs = cells(min.z, max.z, lv.origin.z, lv.height);
                let p = |x: u32, z: u32| {
                    [
                        lv.origin.x + x as f32,
                        lv.origin.y + lv.heights[(z * lv.width + x) as usize],
                        lv.origin.z + z as f32,
                    ]
                };
                for z in zs {
                    for x in xs.clone() {
                        out.push(tri([p(x, z), p(x, z + 1), p(x + 1, z)], level));
                        out.push(tri([p(x + 1, z), p(x, z + 1), p(x + 1, z + 1)], level));
                    }
                }
            }
            let _flow = lv.instances.near(min, max, |i, ins| {
                if !overlaps(ins.min, ins.max) {
                    return ControlFlow::Continue(());
                }
                let owner = level | (i as u64 + 1);
                match &ins.shape {
                    Shape::Mesh { mesh, inv } => {
                        let fwd = inv.inverse();
                        for t in &mesh.tris {
                            let v =
                                t.map(|i| fwd.transform_point3(Vec3::from(mesh.verts[i as usize])));
                            if overlaps(v[0].min(v[1]).min(v[2]), v[0].max(v[1]).max(v[2])) {
                                out.push(tri(v.map(|p| p.to_array()), owner));
                            }
                        }
                    }
                    Shape::Boxes(boxes) => {
                        for o in boxes {
                            let Obstacle::Box { center, axes, half } = o else {
                                continue;
                            };
                            let v = |i: usize| {
                                (0..3)
                                    .fold(Vec3::from(*center), |p, a| {
                                        let s = if i & (1 << a) != 0 { half[a] } else { -half[a] };
                                        p + Vec3::from(axes[a]) * s
                                    })
                                    .to_array()
                            };
                            for q in [
                                [0, 1, 3, 2],
                                [4, 5, 7, 6],
                                [0, 1, 5, 4],
                                [2, 3, 7, 6],
                                [0, 2, 6, 4],
                                [1, 3, 7, 5],
                            ] {
                                out.push(tri([v(q[0]), v(q[1]), v(q[2])], owner));
                                out.push(tri([v(q[0]), v(q[2]), v(q[3])], owner));
                            }
                        }
                    }
                }
                ControlFlow::Continue(())
            });
        }
    }
}

/// The world box around placed hit boxes (an empty, inverted box for none).
fn boxes_bounds(boxes: &[Obstacle]) -> (Vec3, Vec3) {
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for b in boxes {
        if let Obstacle::Box { center, axes, half } = b {
            let c = Vec3::from(*center);
            let e = (0..3)
                .map(|i| Vec3::from(axes[i]).abs() * half[i])
                .sum::<Vec3>();
            min = min.min(c - e);
            max = max.max(c + e);
        }
    }
    (min, max)
}

/// `Actor::PhysicsSetup`: every hit box but `Visibility` and
/// `Transparency` (names compared as `Name` ids) is part of the body.
fn is_solid_box(name: &str) -> bool {
    !name.eq_ignore_ascii_case("Visibility") && !name.eq_ignore_ascii_case("Transparency")
}

fn solid_hit_boxes(msh: &Msh) -> impl Iterator<Item = &gd_formats::msh::HitBox> {
    msh.hit_boxes.iter().filter(|h| is_solid_box(&h.name))
}

/// The segment against a closed door's hit boxes.
pub fn segment_obstacle(a: Vec3, b: Vec3, o: &gd_sim::nav::Obstacle) -> Option<f32> {
    use gd_sim::nav::Obstacle;
    let d = b - a;
    match o {
        Obstacle::Box { center, axes, half } => {
            let c = Vec3::from(*center);
            let ax = axes.map(Vec3::from);
            let (la, ld) = (
                Vec3::new((a - c).dot(ax[0]), (a - c).dot(ax[1]), (a - c).dot(ax[2])),
                Vec3::new(d.dot(ax[0]), d.dot(ax[1]), d.dot(ax[2])),
            );
            let h = Vec3::from(*half);
            let (mut t0, mut t1) = (0.0f32, 1.0f32);
            for i in 0..3 {
                if ld[i].abs() < 1e-9 {
                    if la[i].abs() > h[i] {
                        return None;
                    }
                } else {
                    let (x, y) = ((-h[i] - la[i]) / ld[i], (h[i] - la[i]) / ld[i]);
                    t0 = t0.max(x.min(y));
                    t1 = t1.min(x.max(y));
                    if t0 > t1 {
                        return None;
                    }
                }
            }
            Some(t0)
        }
        Obstacle::Cylinder {
            base,
            radius,
            height,
        } => {
            let base = Vec3::from(*base);
            // Sampled: the cylinder only matters as a coarse blocker.
            let n = (d.length() / (radius * 0.5).max(0.05)).ceil().max(1.0) as usize;
            (0..=n).map(|k| k as f32 / n as f32).find(|t| {
                let p = a + d * *t - base;
                p.y >= 0.0 && p.y <= *height && p.with_y(0.0).length() <= *radius
            })
        }
    }
}
