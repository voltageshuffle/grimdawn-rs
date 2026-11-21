//! Hit-box obstacles of placed objects (doors, breakables, nav blockers).
use bevy::prelude::*;
use gd_formats::msh::{Coords, Mesh as Msh};
use gd_sim::nav::Obstacle;

/// The mesh's hit boxes placed by `tf`, in the bind pose.
pub fn hit_box_obstacles(msh: &Msh, tf: &Transform) -> Vec<Obstacle> {
    let bind: Vec<Coords> = msh.bones.iter().map(|b| b.abs).collect();
    posed_hit_box_obstacles(msh, tf, &bind)
}

/// The mesh's hit boxes placed by `tf`, their bones posed by `abs` (mesh-space
/// bone frames, `gd_formats::anm::pose_mesh`'s first result).
///
/// Engine.dll `GraphicsMesh::GetHitBox(index, SkeletalPose)`
/// (0x1800b7b30) and `Actor::GetHitBox` (0x180026690): a hit box with a bone
/// index is `GetAbsCoords(pose, bone)` × the box frame; one with bone −1 takes
/// the bone named like the hit box, and then that bone's posed frame alone
/// (the box frame unused); with no such bone, the box frame in mesh space.
/// The axes are normalised and the half sizes scaled by their lengths, then
/// placed by the entity's coords and scale. `GetAbsCoords(pose, bone)`
/// (0x1800b8320) chains `local × key` up to the root (a bone without a key
/// uses identity), as `pose_mesh` does; no root-motion pin.
pub fn posed_hit_box_obstacles(msh: &Msh, tf: &Transform, abs: &[Coords]) -> Vec<Obstacle> {
    placed_hit_boxes(msh, &tf.to_matrix(), abs, |_| true)
}

/// The hit boxes accepted by `keep`, placed by `m` (see
/// [`posed_hit_box_obstacles`]).
pub fn placed_hit_boxes(
    msh: &Msh,
    m: &Mat4,
    abs: &[Coords],
    keep: impl Fn(&gd_formats::msh::HitBox) -> bool,
) -> Vec<Obstacle> {
    msh.hit_boxes
        .iter()
        .filter(|hb| keep(hb))
        .map(|hb| {
            let frame = match hb.bone_index {
                Some(i) if hb.bone >= 0 => abs.get(i).map_or(hb.coords, |b| b.mul(&hb.coords)),
                Some(i) => abs.get(i).copied().unwrap_or(hb.coords),
                None => hb.coords,
            };
            let o = Vec3::from(frame.origin);
            let ax = [
                Vec3::from(frame.x),
                Vec3::from(frame.y),
                Vec3::from(frame.z),
            ];
            let center = m.transform_point3(o);
            let mut half = hb.extents;
            let axes = [0, 1, 2].map(|i| {
                let w = m.transform_vector3(ax[i]);
                half[i] *= w.length();
                w.normalize_or_zero().to_array()
            });
            Obstacle::Box {
                center: center.to_array(),
                axes,
                half,
            }
        })
        .collect()
}

/// A `NavBlocker` record's cylinder at `pos`.
pub fn nav_blocker(rec: &gd_formats::arz::Record, pos: Vec3) -> Obstacle {
    Obstacle::Cylinder {
        base: pos.to_array(),
        radius: rec.f32("radius").unwrap_or(1.0),
        height: rec.f32("height").unwrap_or(3.0),
    }
}
