//! Presentation of the marks over NPCs (`gd_game::npc_marks::NpcMark`): the mark's FX record is a
//! `Punctuation` object, a plain mesh (`fx/meshfx/ui_*.msh`) spawned like any record model.
//! * `Entity::AttachPunctuation`: the object is attached to the NPC's `Head` attach point; a new mark replaces
//!   the old one. A mesh without a `Head` point attaches at the actor's origin (`GraphicsMesh::GetAttachedCoords`
//!   returns the identity for an unknown name; `Actor::GetAttachedCoordsInRegion` scales it by the actor's scale).
//! * `Npc::BillboardPunctuation`: every frame the mark faces the camera, offset (0, 3, 0) in the head point's
//!   frame. PORT: 3 m straight up in world space (NPC scale applied), yaw facing the camera; the engine's exact
//!   angle was not decoded.
//! * No distance fade, bobbing or fog rule.
//! * `albedo.ssh` meshes draw unlit (`render::material::FLAG_UNLIT`).
use bevy::prelude::*;
use gd_game::npc_marks::NpcMark;
use gd_game_core::character::Character;
use gd_render::camera::GameCamera;
use gd_render::character::CharAssets;

pub struct NpcMarksViewPlugin;

impl Plugin for NpcMarksViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, dress_marks)
            .add_systems(PostUpdate, place_marks.after(TransformSystems::Propagate));
    }
}

#[derive(Component)]
#[relationship(relationship_target = MarkObjects)]
struct MarkOf(Entity);

#[derive(Component)]
#[relationship_target(relationship = MarkOf, linked_spawn)]
struct MarkObjects(Vec<Entity>);

/// (Re)creates mark objects for NPCs whose [`NpcMark`] changed or went.
fn dress_marks(
    mut commands: Commands,
    changed: Query<(Entity, &NpcMark, Option<&MarkObjects>), Changed<NpcMark>>,
    mut removed: RemovedComponents<NpcMark>,
    objects: Query<&MarkObjects>,
    mut a: CharAssets,
) {
    for npc in removed.read() {
        for &obj in objects.get(npc).map_or(&[][..], |o| &o.0) {
            commands.entity(obj).try_despawn();
        }
    }
    for (npc, mark, old) in &changed {
        for &obj in old.map_or(&[][..], |o| &o.0) {
            commands.entity(obj).try_despawn();
        }
        let db = a.data.db.clone();
        let Some(rec) = db.get(&mark.0) else {
            continue;
        };
        debug!("npc mark {} on {npc:?}", mark.0);
        let obj = commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                MarkOf(npc),
                Name::new(mark.0.clone()),
            ))
            .id();
        a.spawn_record_model(&mut commands, obj, rec);
    }
}

/// Floats each mark over its NPC's head, facing the camera. Runs after propagation, so it writes the mark's
/// and its mesh parts' `GlobalTransform` directly to avoid a frame of lag.
fn place_marks(
    mut objects: Query<(
        &MarkOf,
        &mut Transform,
        &mut GlobalTransform,
        Option<&Children>,
    )>,
    npcs: Query<(&GlobalTransform, &Character), Without<MarkOf>>,
    cams: Query<&GlobalTransform, (With<GameCamera>, Without<MarkOf>)>,
    mut parts: Query<
        (&Transform, &mut GlobalTransform),
        (Without<MarkOf>, Without<Character>, Without<GameCamera>),
    >,
) {
    let Ok(cam) = cams.single() else { return };
    for (m, mut tf, mut gt, children) in &mut objects {
        let Ok((ngt, ch)) = npcs.get(m.0) else {
            continue;
        };
        let head = gd_game_core::casting::attach_point_bind(&ch.body, "Head")
            .map_or(ngt.translation(), |l| ngt.transform_point(l));
        let scale = ngt.scale().y;
        let at = head + Vec3::Y * 3.0 * scale;
        let d = cam.translation() - at;
        let t = Transform {
            translation: at,
            rotation: Quat::from_rotation_y(d.x.atan2(d.z)),
            scale: Vec3::splat(scale),
        };
        // Written only when moved: a write flags the transforms changed for the renderer.
        tf.set_if_neq(t);
        let g = GlobalTransform::from(t);
        gt.set_if_neq(g);
        for c in children.iter().flat_map(|cs| cs.iter()) {
            if let Ok((ctf, mut cgt)) = parts.get_mut(c) {
                cgt.set_if_neq(g.mul_transform(*ctf));
            }
        }
    }
}
