//! Presentation of the marks over NPCs (`gd_game::npc_marks::NpcMark`, which
//! holds the rules): the mark's FX record is a `Punctuation` object, a plain
//! mesh (`fx/meshfx/ui_*.msh`, its `baseTexture` override) spawned like any
//! record model ([`gd_render::character::CharAssets::spawn_record_model`]).
//! * `Entity::AttachPunctuation` (Engine.dll): the object
//!   is created from the record and attached to the NPC's `Head` attach point;
//!   `DetachPunctuation` destroys it. A new mark replaces the old one. A mesh
//!   without a `Head` point (the bounty tables, `*_tablebounty*.msh`: only
//!   candle / light points) attaches at the actor's origin:
//!   `GraphicsMesh::GetAttachedCoords` returns the identity for an
//!   unknown name, and `Actor::GetAttachedCoordsInRegion` scales that frame by
//!   the actor's scale before placing it at the actor's coords.
//! * `Npc::BillboardPunctuation` (Game.dll): every frame
//!   the attach offset is set to a rotation about the vertical axis derived
//!   from the camera's direction and a translation of (0, 3, 0) in the head
//!   point's frame. PORT: the offset is 3 m straight up from the head point
//!   (in world space, NPC scale applied) and the yaw faces the camera; the
//!   engine's exact angle (acos of two horizontal vectors) was not decoded.
//! * No distance fade, bobbing or fog rule exists in the engine for it; the
//!   mesh's own `CreateEntity` effects, if any, come with the model.
//! * The gem, `!` / `?` and most service meshes use `albedo.ssh`, whose deferred
//!   pass is unlit (`render::material::FLAG_UNLIT`): drawn as the texel, so the
//!   gem is an opaque grey square-topped pyramid in any light (lit, its top went
//!   white in the sun over a dark, see-through-looking point).
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

/// The mark object of an NPC (a root entity, despawned with its mark or its NPC).
#[derive(Component)]
#[relationship(relationship_target = MarkObjects)]
struct MarkOf(Entity);

/// An NPC's mark objects ([`MarkOf`]).
#[derive(Component)]
#[relationship_target(relationship = MarkOf, linked_spawn)]
struct MarkObjects(Vec<Entity>);

/// (Re)creates mark objects for NPCs whose [`NpcMark`] changed or went (a despawned NPC's go with it).
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

/// Floats each mark over its NPC's head, facing the camera. Runs after
/// propagation (the NPC's and camera's `GlobalTransform` are this frame's),
/// so it writes the mark's `GlobalTransform` directly — and its mesh parts',
/// which propagation has already computed from the old transform — to keep
/// the mark on the NPC this frame instead of a frame behind.
fn place_marks(
    mut objects: Query<(
        &MarkOf,
        &mut Transform,
        &mut GlobalTransform,
        Option<&Children>,
    )>,
    // (Disjoint from the marks and their parts, whose `GlobalTransform` is written.)
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
        // Written only when they moved: an unchanged write still flags the
        // mark's and its parts' transforms changed for the renderer every frame.
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
