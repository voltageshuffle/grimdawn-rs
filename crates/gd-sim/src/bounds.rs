//! The volumes skill target queries test an actor by: its axis-aligned bounding box
//! (`ABBox`: centre + half extents), not its radius (`Actor::GetRadius` is the crowd / reach radius only).
//!
//! Engine.dll `GraphicsMeshInstance::UpdateBoundingBox` @0x1800c3230: the object-space box
//! (`GetObjectSpaceBoundingBox`, [`Aabb::object`]) is the box around the mesh's posed hit boxes
//! (`GraphicsMesh::GetHitBox(i, pose)`), else the mesh's bounds (msh chunk 10), else (no mesh) a point at the
//! origin; its centre and half extents × the actor's scale vector. `Actor::UpdateBoundingBox` @0x18002a990 puts
//! it in the world by the entity's coords (position, rotation): centre = R c + t, half = |R| h
//! ([`Aabb::transformed`]). That box (`Entity::GetRegionBoundingBox(true)`, Entity +92) is what
//! `GameEngine::GetTargetsInRadius` @0x1802ebe50 (`World::GetEntitiesInSphere`: [`Aabb::meets_sphere`]) and
//! `GetTargetsAroundRay` @0x1802ec0b0 (`World::GetEntitiesAroundRay`: [`Aabb::around_ray`]) test.
//! The hit boxes take the actor's current pose ([`Aabb::posed`], `GetHitBox(i, pose)`: the bones'
//! `GetAbsCoords(pose, bone)`; gd-game `combat::posed_bounds` per frame); [`Aabb::object`] is the bind pose.
//!
//! The port's cursor picks (grimdawn `hover`, `loot`, `riftgates`, `death`) use the same maths: [`Aabb::ray_entry`]
//! into a mesh or model box (the ray taken into the box's frame), [`ray_sphere`] for the objects picked by a sphere
//! (PORT stand-ins for the engine's mesh hit tests; each caller names its sphere).
use gd_formats::msh::{Coords, Mesh};

/// An axis-aligned box: centre and half extents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub center: [f32; 3],
    pub half: [f32; 3],
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Where the ray `o + t d` (unit `d`) passes within `r` of `c`: the `t` (≥ 0) of its point nearest to `c`, when that
/// point is closer than `r`.
pub fn ray_sphere(o: [f32; 3], d: [f32; 3], c: [f32; 3], r: f32) -> Option<f32> {
    let t = dot(sub(c, o), d).max(0.0);
    let near = sub([0, 1, 2].map(|i| o[i] + d[i] * t), c);
    (dot(near, near).sqrt() < r).then_some(t)
}

impl Aabb {
    /// A point at the origin (no mesh: `GraphicsMeshInstance` +56 is 0).
    pub const POINT: Aabb = Aabb {
        center: [0.0; 3],
        half: [0.0; 3],
    };

    /// `GraphicsMeshInstance::GetObjectSpaceBoundingBox` of `mesh` at `scale` (module doc), the hit boxes in the
    /// bind pose (their frames cached on the mesh at load).
    pub fn object(mesh: &Mesh, scale: f32) -> Aabb {
        let b = if !mesh.bones.is_empty() && !mesh.hit_boxes.is_empty() {
            Self::around_hit_boxes(mesh, mesh.hit_box_bind.iter().copied())
        } else {
            Self::mesh_box(mesh)
        };
        Self::scaled(b, scale)
    }

    /// [`Self::object`] with the bones posed by `abs` (mesh-space bone frames, `gd_sim::anim::pose_abs`);
    /// each box's bone was resolved at load (`gd_formats::msh::HitBox::bone_index`).
    pub fn posed(mesh: &Mesh, scale: f32, abs: &[Coords]) -> Aabb {
        let b = if !mesh.bones.is_empty() && !mesh.hit_boxes.is_empty() {
            Self::around_hit_boxes(
                mesh,
                mesh.hit_boxes.iter().map(|hb| {
                    // `GraphicsMesh::GetHitBox` (crate `gd_game::obstacles` doc): a bone index takes
                    // the bone's frame × the box frame, bone −1 the named bone's frame alone,
                    // no bone the box frame.
                    match hb.bone_index {
                        Some(i) if hb.bone >= 0 => {
                            abs.get(i).map_or(hb.coords, |b| b.mul(&hb.coords))
                        }
                        Some(i) => abs.get(i).copied().unwrap_or(hb.coords),
                        None => hb.coords,
                    }
                }),
            )
        } else {
            Self::mesh_box(mesh)
        };
        Self::scaled(b, scale)
    }

    /// The box around the hit boxes' `frames` (one per box, in order): each box's extents along
    /// its frame's axes.
    fn around_hit_boxes(mesh: &Mesh, frames: impl Iterator<Item = Coords>) -> Aabb {
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for (hb, frame) in mesh.hit_boxes.iter().zip(frames) {
            let Coords { x, y, z, origin } = frame;
            for i in 0..3 {
                let h = x[i].abs() * hb.extents[0]
                    + y[i].abs() * hb.extents[1]
                    + z[i].abs() * hb.extents[2];
                lo[i] = lo[i].min(origin[i] - h);
                hi[i] = hi[i].max(origin[i] + h);
            }
        }
        Aabb {
            center: [0, 1, 2].map(|i| (hi[i] + lo[i]) * 0.5),
            half: [0, 1, 2].map(|i| (hi[i] - lo[i]) * 0.5),
        }
    }

    /// The mesh's own bounds (msh chunk 10), a point at the origin without them.
    fn mesh_box(mesh: &Mesh) -> Aabb {
        if let Some((lo, hi)) = mesh.bounding_box {
            Aabb {
                center: [0, 1, 2].map(|i| (hi[i] + lo[i]) * 0.5),
                half: [0, 1, 2].map(|i| (hi[i] - lo[i]) * 0.5),
            }
        } else {
            Aabb::POINT
        }
    }

    fn scaled(b: Aabb, scale: f32) -> Aabb {
        Aabb {
            center: b.center.map(|v| v * scale),
            half: b.half.map(|v| v * scale),
        }
    }

    /// The box around this one under the linear map with columns `cols` and the offset `t` (`Actor::UpdateBoundingBox`).
    pub fn transformed(&self, cols: [[f32; 3]; 3], t: [f32; 3]) -> Aabb {
        let (c, h) = (self.center, self.half);
        Aabb {
            center: [0, 1, 2]
                .map(|i| cols[0][i] * c[0] + cols[1][i] * c[1] + cols[2][i] * c[2] + t[i]),
            half: [0, 1, 2].map(|i| {
                cols[0][i].abs() * h[0] + cols[1][i].abs() * h[1] + cols[2][i].abs() * h[2]
            }),
        }
    }

    /// Whether the sphere at `c` of radius `r` meets the box (sub_18027E1F0: the squared distance from `c` to
    /// the box ≤ r²).
    pub fn meets_sphere(&self, c: [f32; 3], r: f32) -> bool {
        let d = sub(c, self.center);
        let s: f32 = (0..3)
            .map(|i| (d[i].abs() - self.half[i]).max(0.0).powi(2))
            .sum();
        r * r >= s
    }

    /// Where the ray `o + t d` enters the box (sub_18003D780, Woo's test): 0 from inside, `INFINITY` on a miss
    /// (behind the origin too). `t` is in units of `d`.
    pub fn ray_entry(&self, o: [f32; 3], d: [f32; 3]) -> f32 {
        let lo = sub(self.center, self.half);
        let hi = [0, 1, 2].map(|i| self.center[i] + self.half[i]);
        let mut inside = true;
        let mut cand = [-1.0f32; 3];
        let mut near = [0.0f32; 3];
        for i in 0..3 {
            if o[i] < lo[i] {
                inside = false;
                near[i] = lo[i];
            } else if o[i] > hi[i] {
                inside = false;
                near[i] = hi[i];
            } else {
                continue;
            }
            if d[i] != 0.0 {
                cand[i] = (near[i] - o[i]) / d[i];
            }
        }
        if inside {
            return 0.0;
        }
        let mut w = 0;
        for i in 1..3 {
            if cand[i] > cand[w] {
                w = i;
            }
        }
        if cand[w] <= 0.0 {
            return f32::INFINITY;
        }
        for i in 0..3 {
            if i != w {
                let p = o[i] + cand[w] * d[i];
                if p < lo[i] || p > hi[i] {
                    return f32::INFINITY;
                }
            }
        }
        cand[w]
    }

    /// `World::GetEntitiesAroundRay`'s test: the box grown by `thick` on every half extent is entered by the ray
    /// from `o` along the unit `d` before `len`.
    pub fn around_ray(&self, o: [f32; 3], d: [f32; 3], len: f32, thick: f32) -> bool {
        Aabb {
            center: self.center,
            half: self.half.map(|h| h + thick),
        }
        .ray_entry(o, d)
            < len
    }
}
