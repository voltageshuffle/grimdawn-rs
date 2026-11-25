//! The rigid body a dropped item flies as, until it lands (the throw itself:
//! [`crate::loot_toss`]; the ECS side: `gd_game::loot::toss_flight`).
//!
//! Engine.dll (Bullet 2.8x behind `PhysicsEngine2`):
//! - The world steps at a fixed 1/60 s ([`STEP`]), at most 4 steps a frame
//!   ([`MAX_SUBSTEPS`]; `PhysicsEngine2Impl::PhysicsEngine2Impl`,
//!   `btDiscreteDynamicsWorld::stepSimulation` drops the time beyond them: [`Clock`]),
//!   gravity (0, −14, 0) (`PhysicsEngine2::kGravity`, world init `PhysicsEngine2Impl::CreateWorld`).
//! - The body (`Entity::PhysicsSetup`): a box (`collisionShape`,
//!   default `Box`) of the mesh bounding box's half extents times the item's
//!   `scale`, each at least 0.1 m (`Actor::GetCollisionBox`,
//!   [`half_extents`]), centred on the coordinates moved by the turned, unscaled
//!   centre of mass (the bounding-box centre, `Actor::GetCenterOfMass`); its
//!   velocity the item's motion, its angular velocity 20 × the item's Z axis
//!   (`Item::PhysicsSetup`, [`SPIN`]); damping 0.05 linear and 0.95
//!   angular ([`LINEAR_DAMPING`], [`ANGULAR_DAMPING`]: the description's
//!   (1.6, 2.5, 0.05, 0.95) = sleeping thresholds and `setDamping`, `btRigidBody::setupRigidBody`);
//!   its friction the record's `physicsFriction` (items 5, shields and off-hands 1),
//!   its inertia the box's (`btBoxShape::calculateLocalInertia`, mass 1).
//! - Items collide with the terrain and the static scenery only: their
//!   collision type is 1 (Game.dll `Item::GetCollisionType`, folded with
//!   `HotSlotOption::Validate`) = group 32, mask 1, and only the
//!   terrain (`TerrainBase::PhysicsSetup`, a heightfield) and static meshes
//!   (`Actor::PhysicsSetup`) are in group 1 (mask 0x7C). Drops do not touch
//!   each other or characters (group 4, mask 0x6D). The world's restitution is
//!   0, so with Bullet's product combine nothing bounces; its friction is 1
//!   (terrain: `TerrainBase::PhysicsSetup`; scenery: their records'
//!   `physicsFriction`, 1 on all stock decorations but one), so a contact's
//!   friction is the item's (Bullet multiplies them, at most 10).
//! - A step ([`DropBody::step`]) is Bullet's discrete one: damping
//!   (`v · (1 − d)^dt`) and gravity into the velocity, contacts found at the
//!   pose the last step left (points closer than 0.02 m, [`CONTACT_THRESHOLD`]),
//!   the sequential-impulse solver (`btDiscreteDynamicsWorld::btDiscreteDynamicsWorld`: 10
//!   iterations, erp 0.2, split impulse below −0.04 m penetration with erp2 0.8
//!   and turn erp 0.1, one friction direction along the sliding velocity), then
//!   position and orientation (exponential map) from the solved velocities.
//! - A contact lands the item: the physics tick callback (`PhysicsEngine2Impl::TickCallback`)
//!   reports a pair that has a new contact point, the frame's
//!   `Entity::PhysicsCollision` calls `Item::CollisionCallback`
//!   (landed flag, drop sound with the surface), the frame's remaining steps
//!   still run with the contact solved (the box pivots on its touching corner),
//!   and the frame's `Item::PhysicsUpdate` takes it out of the
//!   simulation. The item rests in that pose: nothing lays it flat. It also lands
//!   when its centre of mass reaches the path mesh (gd-game).
//! - Contacts persist between steps as Bullet's `btPersistentManifold`: per
//!   world object at most 4 points, a new one matched to a kept one within
//!   0.02 m on the box, points dropped once they separate or slide past 0.02 m,
//!   and the solver starts from 0.85 of a kept point's last impulses
//!   (warm starting, `m_warmstartingFactor`, `SOLVER_USE_WARMSTARTING` in the
//!   solver mode 260).
//!
//! PORT: contacts are found by feature tests against the world's triangles
//! (box corners over a triangle, triangle corners in the box, and, when neither
//! touches, crossing box and triangle edges), not Bullet's GJK / EPA per
//! triangle.

pub type V3 = [f32; 3];
/// A rotation as a unit quaternion (x, y, z, w).
pub type Q = [f32; 4];
/// A world triangle (terrain or static scenery) and the object it belongs to
/// (contacts are kept per object, as Bullet keeps a manifold per pair).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tri {
    pub v: [V3; 3],
    pub owner: u64,
}

/// Fixed physics step (s).
pub const STEP: f32 = 1.0 / 60.0;
/// The most steps one frame runs.
pub const MAX_SUBSTEPS: u32 = 4;
/// `PhysicsEngine2::kGravity` (m/s², downwards).
pub const GRAVITY: f32 = 14.0;
/// Linear damping of item bodies.
pub const LINEAR_DAMPING: f32 = 0.05;
/// Angular damping of item bodies.
pub const ANGULAR_DAMPING: f32 = 0.95;
/// Per-step (linear, angular) velocity factors: `(1 - damping)^STEP`.
static STEP_DAMPING: std::sync::LazyLock<(f32, f32)> = std::sync::LazyLock::new(|| {
    (
        (1.0 - LINEAR_DAMPING).powf(STEP),
        (1.0 - ANGULAR_DAMPING).powf(STEP),
    )
});
/// Spin about the item's Z axis at its release (rad/s).
pub const SPIN: f32 = 20.0;
/// The least half extent of an item's box (m).
pub const MIN_HALF_EXTENT: f32 = 0.1;
/// Bullet's `gContactBreakingThreshold`: how close a point counts as a contact (m).
pub const CONTACT_THRESHOLD: f32 = 0.02;
/// Bullet's `ANGULAR_MOTION_THRESHOLD` (π/4 per step).
const ANGULAR_MOTION_THRESHOLD: f32 = std::f32::consts::FRAC_PI_4;
/// `btContactSolverInfo`: iterations, erp, erp2, split-impulse threshold, turn erp.
const ITERATIONS: usize = 10;
const ERP: f32 = 0.2;
const ERP2: f32 = 0.8;
const SPLIT_THRESHOLD: f32 = -0.04;
const SPLIT_TURN_ERP: f32 = 0.1;
/// Bullet's `MAX_FRICTION` for the combined friction.
const MAX_FRICTION: f32 = 10.0;
/// Points a contact manifold keeps per world object (`MANIFOLD_CACHE_SIZE`).
const MANIFOLD_POINTS: usize = 4;
/// `btContactSolverInfo::m_warmstartingFactor` (`btDiscreteDynamicsWorld::btDiscreteDynamicsWorld`: 0.85,
/// `SOLVER_USE_WARMSTARTING` set).
const WARMSTARTING: f32 = 0.85;

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}

/// `v` turned by `q`.
pub fn rotate(q: Q, v: V3) -> V3 {
    let u = [q[0], q[1], q[2]];
    let t = scale(cross(u, v), 2.0);
    add(add(v, scale(t, q[3])), cross(u, t))
}

fn unrotate(q: Q, v: V3) -> V3 {
    rotate([-q[0], -q[1], -q[2], q[3]], v)
}

/// `a` after `b` (`a · b`).
pub fn mul(a: Q, b: Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn normalize(q: Q) -> Q {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
}

/// The turn of angular velocity `w` over `dt` (Bullet's `integrateTransform`).
fn turn(w: V3, dt: f32) -> Q {
    let mut angle = length(w);
    if angle * dt > ANGULAR_MOTION_THRESHOLD {
        angle = ANGULAR_MOTION_THRESHOLD / dt;
    }
    let axis = if angle < 0.001 {
        // Taylor expansion of sin(x / 2) / x.
        scale(w, 0.5 * dt - dt * dt * dt * 0.020_833_334 * angle * angle)
    } else {
        scale(w, (0.5 * angle * dt).sin() / angle)
    };
    [axis[0], axis[1], axis[2], (0.5 * angle * dt).cos()]
}

/// The box half extents of an item whose mesh's bounding box has half extents
/// `mesh_half`, at record `scale` (`Actor::GetCollisionBox`).
pub fn half_extents(mesh_half: V3, scale: f32) -> V3 {
    mesh_half.map(|h| (h * scale).max(MIN_HALF_EXTENT))
}

/// A contact: the point on the box `a`, on the world `b`, the world's normal
/// (towards the box) and the signed distance (negative: penetration).
#[derive(Clone, Copy, Debug)]
struct Contact {
    a: V3,
    b: V3,
    n: V3,
    depth: f32,
}

/// A kept contact point: on the box (its frame), on the world, the normal, the
/// distance and the impulses the solver gave it (normal, friction).
#[derive(Clone, Copy, Debug, PartialEq)]
struct ManifoldPoint {
    local: V3,
    world: V3,
    n: V3,
    depth: f32,
    impulse: f32,
    lateral: f32,
}

/// Closest points of segments `p0 → p1` and `q0 → q1`: their parameters.
fn segment_params(p0: V3, p1: V3, q0: V3, q1: V3) -> Option<(f32, f32)> {
    let (d1, d2, r) = (sub(p1, p0), sub(q1, q0), sub(p0, q0));
    let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, r));
    let (b, c) = (dot(d1, d2), dot(d1, r));
    let den = a * e - b * b;
    if den < 1e-9 * a * e || a < 1e-12 || e < 1e-12 {
        return None;
    }
    let s = ((b * f - c * e) / den).clamp(0.0, 1.0);
    let t = ((b * s + f) / e).clamp(0.0, 1.0);
    let s = ((b * t - c) / a).clamp(0.0, 1.0);
    Some((s, t))
}

/// One solver row: impulse direction `d` at arm `r`.
struct Row {
    r: V3,
    d: V3,
    eff: f32,
    rhs: f32,
    rhs_pen: f32,
    acc: f32,
    acc_pen: f32,
}

/// A flying item: its centre of mass, rotation, velocities, box and friction.
#[derive(Clone, Debug, PartialEq)]
pub struct DropBody {
    pub pos: V3,
    pub rot: Q,
    pub vel: V3,
    pub spin: V3,
    pub half: V3,
    pub friction: f32,
    /// The kept contact points, per world object (module doc).
    manifolds: Vec<(u64, Vec<ManifoldPoint>)>,
}

impl DropBody {
    /// The body of an item turned by `rot` whose centre of mass is at `pos`,
    /// thrown at `vel` (`Item::PhysicsSetup`: spinning at [`SPIN`] about its Z
    /// axis), with box `half` and record friction `friction`.
    pub fn new(pos: V3, rot: Q, vel: V3, half: V3, friction: f32) -> DropBody {
        DropBody {
            pos,
            rot,
            vel,
            spin: scale(rotate(rot, [0.0, 0.0, 1.0]), SPIN),
            half,
            friction,
            manifolds: Vec::new(),
        }
    }

    /// The box's corners in the world (bit i of the index: the sign along axis i).
    pub fn corners(&self) -> [V3; 8] {
        std::array::from_fn(|i| {
            let h = self.half;
            let c = [
                if i & 1 != 0 { h[0] } else { -h[0] },
                if i & 2 != 0 { h[1] } else { -h[1] },
                if i & 4 != 0 { h[2] } else { -h[2] },
            ];
            add(self.pos, rotate(self.rot, c))
        })
    }

    /// The world box around the body.
    pub fn aabb(&self) -> (V3, V3) {
        let axes: [V3; 3] = std::array::from_fn(|a| {
            let mut axis = [0.0; 3];
            axis[a] = 1.0;
            rotate(self.rot, axis)
        });
        let ext: V3 =
            std::array::from_fn(|i| (0..3).map(|a| axes[a][i].abs() * self.half[a]).sum());
        (sub(self.pos, ext), add(self.pos, ext))
    }

    /// Inverse world inertia (mass 1) times `x`.
    fn inv_inertia(&self, x: V3) -> V3 {
        let [a, b, c] = self.half.map(|h| 4.0 * h * h);
        let inv = [12.0 / (b + c), 12.0 / (a + c), 12.0 / (a + b)];
        let l = unrotate(self.rot, x);
        rotate(self.rot, [l[0] * inv[0], l[1] * inv[1], l[2] * inv[2]])
    }

    fn local(&self, p: V3) -> V3 {
        unrotate(self.rot, sub(p, self.pos))
    }

    /// The contacts with `world` in the current pose (module doc).
    fn contacts(&self, world: &[Tri]) -> Vec<(u64, Contact)> {
        let corners = self.corners();
        let (lo, hi) = self.aabb();
        let limit = 2.0 * self.half.iter().fold(0.0f32, |m, h| m.max(*h));
        let mut out = Vec::new();
        for &Tri { v: tri, owner } in world {
            let near = (0..3).all(|i| {
                let (tl, th) = tri
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(v[i]), h.max(v[i])));
                tl <= hi[i] + CONTACT_THRESHOLD && th >= lo[i] - CONTACT_THRESHOLD
            });
            if !near {
                continue;
            }
            let face = cross(sub(tri[1], tri[0]), sub(tri[2], tri[0]));
            let len = length(face);
            if len < 1e-9 {
                continue;
            }
            let mut n = scale(face, 1.0 / len);
            if dot(n, sub(self.pos, tri[0])) < 0.0 {
                n = scale(n, -1.0);
            }
            let before = out.len();
            // Box corners over the triangle.
            for c in corners {
                let d = dot(n, sub(c, tri[0]));
                if d < CONTACT_THRESHOLD && d > -limit {
                    let side = |k: usize| {
                        let (a, b) = (tri[k], tri[(k + 1) % 3]);
                        dot(cross(sub(b, a), sub(c, a)), n)
                    };
                    let s = [side(0), side(1), side(2)];
                    if s.iter().all(|x| *x >= 0.0) || s.iter().all(|x| *x <= 0.0) {
                        out.push((
                            owner,
                            Contact {
                                a: c,
                                b: sub(c, scale(n, d)),
                                n,
                                depth: d,
                            },
                        ));
                    }
                }
            }
            // Triangle corners in the box: out through the nearest face.
            for v in &tri {
                let q = self.local(*v);
                let s: V3 = std::array::from_fn(|i| q[i].abs() - self.half[i]);
                let m = (0..3).fold(0, |m, i| if s[i] > s[m] { i } else { m });
                if s[m] < CONTACT_THRESHOLD && s[m] > -limit {
                    let mut axis = [0.0; 3];
                    axis[m] = if q[m] < 0.0 { -1.0 } else { 1.0 };
                    let f = rotate(self.rot, axis);
                    out.push((
                        owner,
                        Contact {
                            a: add(*v, scale(f, -s[m])),
                            b: *v,
                            n: scale(f, -1.0),
                            depth: s[m],
                        },
                    ));
                }
            }
            if out.len() > before {
                continue;
            }
            // Edge on edge.
            for i in 0..8usize {
                for bit in [1usize, 2, 4] {
                    if i & bit != 0 {
                        continue;
                    }
                    let (c0, c1) = (corners[i], corners[i | bit]);
                    for k in 0..3 {
                        let (a, b) = (tri[k], tri[(k + 1) % 3]);
                        let Some((s, t)) = segment_params(c0, c1, a, b) else {
                            continue;
                        };
                        if s <= 0.0 || s >= 1.0 || t <= 0.0 || t >= 1.0 {
                            continue;
                        }
                        let pb = add(c0, scale(sub(c1, c0), s));
                        let pt = add(a, scale(sub(b, a), t));
                        let axis = cross(sub(c1, c0), sub(b, a));
                        let al = length(axis);
                        if al < 1e-6 {
                            continue;
                        }
                        let mut en = scale(axis, 1.0 / al);
                        if dot(en, sub(self.pos, pt)) < 0.0 {
                            en = scale(en, -1.0);
                        }
                        let depth = dot(en, sub(pb, pt));
                        let q = self.local(pt);
                        let inside = (0..3).all(|j| q[j].abs() <= self.half[j] + CONTACT_THRESHOLD);
                        if depth < CONTACT_THRESHOLD && depth > -limit && inside {
                            out.push((
                                owner,
                                Contact {
                                    a: pb,
                                    b: pt,
                                    n: en,
                                    depth,
                                },
                            ));
                        }
                    }
                }
            }
        }
        out
    }

    fn row(&self, r: V3, d: V3) -> Row {
        let k = 1.0 + dot(d, cross(self.inv_inertia(cross(r, d)), r));
        Row {
            r,
            d,
            eff: 1.0 / k,
            rhs: 0.0,
            rhs_pen: 0.0,
            acc: 0.0,
            acc_pen: 0.0,
        }
    }

    fn point_vel(&self, v: V3, w: V3, r: V3) -> V3 {
        add(v, cross(w, r))
    }

    fn apply(&self, v: &mut V3, w: &mut V3, row: &Row, j: f32) {
        *v = add(*v, scale(row.d, j));
        *w = add(*w, scale(self.inv_inertia(cross(row.r, row.d)), j));
    }

    /// Bullet's `btPersistentManifold`: each new contact replaces the kept point of its owner within
    /// [`CONTACT_THRESHOLD`] of it on the box (keeping its impulses), else joins, at most
    /// [`MANIFOLD_POINTS`] per owner (`sortCachedPoints`: the deepest stays, the one whose replacement
    /// leaves the largest area goes); then every point is re-measured in the current pose and dropped once
    /// it separates past the threshold or slides off its world point by more (`refreshContactPoints`).
    fn update_manifolds(&mut self, found: Vec<(u64, Contact)>) {
        for (owner, c) in found {
            let point = ManifoldPoint {
                local: self.local(c.a),
                world: c.b,
                n: c.n,
                depth: c.depth,
                impulse: 0.0,
                lateral: 0.0,
            };
            let at = match self.manifolds.iter().position(|m| m.0 == owner) {
                Some(i) => i,
                None => {
                    self.manifolds.push((owner, Vec::new()));
                    self.manifolds.len() - 1
                }
            };
            let points = &mut self.manifolds[at].1;
            let near = points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let d = sub(p.local, point.local);
                    (i, dot(d, d))
                })
                .filter(|(_, d)| *d < CONTACT_THRESHOLD * CONTACT_THRESHOLD)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, _)) = near {
                let kept = points[i];
                points[i] = ManifoldPoint {
                    impulse: kept.impulse,
                    lateral: kept.lateral,
                    ..point
                };
            } else if points.len() < MANIFOLD_POINTS {
                points.push(point);
            } else {
                let i = replaced_point(points, point.local);
                points[i] = point;
            }
        }
        let (pos, rot) = (self.pos, self.rot);
        for (_, points) in &mut self.manifolds {
            points.retain_mut(|p| {
                let a = add(pos, rotate(rot, p.local));
                p.depth = dot(p.n, sub(a, p.world));
                let drift = sub(sub(a, scale(p.n, p.depth)), p.world);
                p.depth <= CONTACT_THRESHOLD
                    && dot(drift, drift) <= CONTACT_THRESHOLD * CONTACT_THRESHOLD
            });
        }
        self.manifolds.retain(|m| !m.1.is_empty());
    }

    /// One physics step against `world` (module doc); true when the box touches it.
    pub fn step(&mut self, world: &[Tri]) -> bool {
        let (lin, ang) = *STEP_DAMPING;
        self.vel = scale(self.vel, lin);
        self.spin = scale(self.spin, ang);
        self.vel[1] -= GRAVITY * STEP;
        let found = self.contacts(world);
        self.update_manifolds(found);
        let (mut push, mut turn_push) = ([0.0f32; 3], [0.0f32; 3]);
        let touching = !self.manifolds.is_empty();
        if touching {
            let mu = self.friction.clamp(0.0, MAX_FRICTION);
            let count = self.manifolds.iter().map(|m| m.1.len()).sum();
            let mut normals = Vec::with_capacity(count);
            let mut frictions = Vec::with_capacity(count);
            let (v0, w0) = (self.vel, self.spin);
            let (mut v, mut w) = (self.vel, self.spin);
            for c in self.manifolds.iter().flat_map(|m| &m.1) {
                let r = rotate(self.rot, c.local);
                let pv = self.point_vel(v0, w0, r);
                let vn = dot(c.n, pv);
                let mut row = self.row(r, c.n);
                let (mut vel_err, mut pos_err) = (-vn, 0.0);
                if c.depth > 0.0 {
                    vel_err -= c.depth / STEP;
                } else {
                    let erp = if c.depth > SPLIT_THRESHOLD { ERP } else { ERP2 };
                    pos_err = -c.depth * erp / STEP;
                }
                if c.depth > SPLIT_THRESHOLD {
                    row.rhs = (pos_err + vel_err) * row.eff;
                } else {
                    row.rhs = vel_err * row.eff;
                    row.rhs_pen = pos_err * row.eff;
                }
                let lateral = sub(pv, scale(c.n, vn));
                let t = if dot(lateral, lateral) > f32::EPSILON {
                    scale(lateral, 1.0 / length(lateral))
                } else {
                    // `btPlaneSpace1`.
                    let n = c.n;
                    if n[2].abs() > std::f32::consts::FRAC_1_SQRT_2 {
                        let k = 1.0 / (n[1] * n[1] + n[2] * n[2]).sqrt();
                        [0.0, -n[2] * k, n[1] * k]
                    } else {
                        let k = 1.0 / (n[0] * n[0] + n[1] * n[1]).sqrt();
                        [-n[1] * k, n[0] * k, 0.0]
                    }
                };
                let mut f = self.row(r, t);
                f.rhs = -dot(t, pv) * f.eff;
                // Warm start: the carried impulses, scaled (`setupContactConstraint`,
                // `setFrictionConstraintImpulse`).
                row.acc = c.impulse * WARMSTARTING;
                f.acc = c.lateral * WARMSTARTING;
                self.apply(&mut v, &mut w, &row, row.acc);
                self.apply(&mut v, &mut w, &f, f.acc);
                normals.push(row);
                frictions.push(f);
            }
            for _ in 0..ITERATIONS {
                for row in normals.iter_mut() {
                    let dv = dot(row.d, self.point_vel(sub(v, v0), sub(w, w0), row.r));
                    let next = (row.acc + row.rhs - row.eff * dv).max(0.0);
                    let j = next - row.acc;
                    row.acc = next;
                    self.apply(&mut v, &mut w, row, j);
                }
                for (row, n) in frictions.iter_mut().zip(&normals) {
                    let limit = mu * n.acc;
                    let dv = dot(row.d, self.point_vel(sub(v, v0), sub(w, w0), row.r));
                    let next = (row.acc + row.rhs - row.eff * dv).clamp(-limit, limit);
                    let j = next - row.acc;
                    row.acc = next;
                    self.apply(&mut v, &mut w, row, j);
                }
            }
            if normals.iter().any(|r| r.rhs_pen != 0.0) {
                for _ in 0..ITERATIONS {
                    for row in normals.iter_mut() {
                        let dv = dot(row.d, self.point_vel(push, turn_push, row.r));
                        let next = (row.acc_pen + row.rhs_pen - row.eff * dv).max(0.0);
                        let j = next - row.acc_pen;
                        row.acc_pen = next;
                        self.apply(&mut push, &mut turn_push, row, j);
                    }
                }
            }
            // The solved impulses go back to the kept points.
            let mut k = 0;
            for (_, kept) in &mut self.manifolds {
                for p in kept.iter_mut() {
                    p.impulse = normals[k].acc;
                    p.lateral = frictions[k].acc;
                    k += 1;
                }
            }
            self.vel = v;
            self.spin = w;
        }
        self.pos = add(self.pos, scale(add(self.vel, push), STEP));
        let pushed = mul(turn(scale(turn_push, SPLIT_TURN_ERP), STEP), self.rot);
        self.rot = normalize(mul(turn(self.spin, STEP), pushed));
        touching
    }
}

/// Which of 4 kept points `new` replaces (`btPersistentManifold::sortCachedPoints`): never the deepest; of
/// the others, the one whose replacement leaves the largest area.
fn replaced_point(points: &[ManifoldPoint], new: V3) -> usize {
    let deepest = (0..points.len()).fold(0, |m, i| {
        if points[i].depth < points[m].depth {
            i
        } else {
            m
        }
    });
    let p = |i: usize| points[i].local;
    let area = |a: V3, b: V3| {
        let c = cross(a, b);
        dot(c, c)
    };
    let areas = [
        area(sub(new, p(1)), sub(p(3), p(2))),
        area(sub(new, p(0)), sub(p(3), p(2))),
        area(sub(new, p(0)), sub(p(3), p(1))),
        area(sub(new, p(0)), sub(p(2), p(1))),
    ];
    (0..4)
        .filter(|i| *i != deepest)
        .max_by(|a, b| areas[*a].total_cmp(&areas[*b]))
        .unwrap_or(0)
}

/// Bullet's fixed stepping (`stepSimulation` with a fixed step): the frame's
/// time is added to what is left over, and the whole steps in it run, at most
/// [`MAX_SUBSTEPS`] (the rest of them is lost).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clock(pub f32);

impl Clock {
    /// Steps to run for a frame of `dt` seconds.
    pub fn steps(&mut self, dt: f32) -> u32 {
        self.0 += dt;
        if self.0 < STEP {
            return 0;
        }
        let n = (self.0 / STEP) as u32;
        self.0 -= n as f32 * STEP;
        n.min(MAX_SUBSTEPS)
    }
}
