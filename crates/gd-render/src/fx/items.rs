//! Effects of worn equipment: the `CreateEntity` effects of a weapon's or an
//! armour piece's mesh (the glow along a sword's blade, an Aether glove's
//! wisps), on every character that wears the item: the player, monsters /
//! NPCs with gear, the paper dolls.
//!
//! Engine.dll `Actor::CreateAttachmentsFromMesh` @0x18002c4e0 (see
//! [`super::mesh_creations`]): a weapon or armour item is an actor like any
//! other, so its mesh creates its entities at its attach points once it has
//! loaded; they are attached to the item (`Actor::Attach`) and follow it, and
//! an attach point with a parent bone follows that bone of the posed mesh.
//! Item records add no effect fields of their own (`Item::Load`,
//! `ItemEquipment::Load`, `Weapon::Load`, `Armor::Load` read none). A
//! character's own body mesh goes the same way ([`attach_armour`] from
//! `character::dress`, a shapeshift form's from `wear_forms`). The
//! "toggled weapon effects" interface option (bool 64) does not touch these:
//! `EquipManager::UpdateWeaponFxVisibility` @0x180286c50 only sets or clears
//! the skill-applied weapon enchantment (`SetWeaponEnchantment`, a
//! `WeaponEnchantmentPak` from a buff's `weaponEnchantment`: [`super::enchant`]).
//!
//! Port: a weapon's effects are children of the weapon's root (hidden with
//! it by `HideRightHand` etc.), an armour piece's of its part; each frame
//! [`follow`] writes their world frame from the posed weapon / bone, after
//! `character::pose_bodies` and before the effect systems. A paper doll's get
//! its render layer ([`super::FxView`]).
use super::{FxSystems, FxView, mesh_effect_points, spawn_effect_record};
use crate::character::{Character, WeaponOf};
use crate::model::{ModelAsset, attach_frame, coords_to_affine};
use bevy::math::Affine3A;
use bevy::prelude::*;
use gd_formats::msh::Mesh as Msh;
use std::sync::Arc;

pub struct ItemFxPlugin;

impl Plugin for ItemFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            follow
                .after(TransformSystems::Propagate)
                .after(crate::character::CharacterRenderStage::Pose)
                .before(crate::model::animate_skeletons)
                .before(FxSystems),
        );
    }
}

/// An effect a worn item's mesh created.
#[derive(Component)]
pub struct ItemFx {
    /// The character wearing the item.
    pub owner: Entity,
    pub anchor: Anchor,
    /// The attach point's frame relative to the anchor.
    pub local: Affine3A,
}

pub enum Anchor {
    /// A weapon's root (its world frame is written by `pose_bodies`).
    Weapon(Entity),
    /// A bone of an armour mesh, posed with the owner's skeleton.
    Bone(Arc<Msh>, usize),
    /// The owner's model space (an armour attach point without a bone).
    Owner,
}

/// Spawns the effects of the weapon model `model` held by `owner` as children
/// of the weapon's root `weapon`.
pub fn attach_weapon(commands: &mut Commands, model: &ModelAsset, owner: Entity, weapon: Entity) {
    for (record, local, _, mesh) in mesh_effect_points(model) {
        spawn(
            commands,
            record,
            ItemFx {
                owner,
                anchor: Anchor::Weapon(weapon),
                local,
            },
            weapon,
            mesh,
        );
    }
}

/// Spawns the effects of the armour model `model` worn by `owner` as children of its part entity `part`, a
/// bone-parented one on that bone of `msh`.
pub fn attach_armour(
    commands: &mut Commands,
    model: &ModelAsset,
    msh: &Arc<Msh>,
    owner: Entity,
    part: Entity,
) {
    for (record, local, parent, mesh) in mesh_effect_points(model) {
        let anchor = match parent.and_then(|p| {
            msh.bones
                .iter()
                .position(|b| b.name.eq_ignore_ascii_case(p))
        }) {
            Some(i) => Anchor::Bone(msh.clone(), i),
            None => Anchor::Owner,
        };
        spawn(
            commands,
            record,
            ItemFx {
                owner,
                anchor,
                local,
            },
            part,
            mesh,
        );
    }
}

/// An `EffectEntity` starts as an effect, an `FxMesh` as a mesh effect (`super::mesh`).
fn spawn(commands: &mut Commands, record: &str, fx: ItemFx, parent: Entity, mesh: bool) {
    let e = if mesh {
        commands
            .spawn((
                gd_game::visuals::FxMeshRequest(record.to_owned()),
                Transform::IDENTITY,
            ))
            .id()
    } else {
        spawn_effect_record(commands, record, Transform::IDENTITY)
    };
    commands.entity(e).insert(fx);
    commands.entity(parent).add_child(e);
}

/// Places every item effect at its weapon / bone this frame (a mesh
/// effect's parts and its scale along, see `follow_points` in `enchant.rs`);
/// a doll's effects take the doll's render layer.
pub(crate) fn follow(
    mut commands: Commands,
    owners: Query<
        (
            &GlobalTransform,
            &Character,
            Option<&crate::character::DollBody>,
        ),
        Without<ItemFx>,
    >,
    weapons: Query<&GlobalTransform, (With<WeaponOf>, Without<ItemFx>, Without<Character>)>,
    mut fx: Query<(
        Entity,
        &ItemFx,
        &Transform,
        &mut GlobalTransform,
        Has<FxView>,
        Option<&Children>,
    )>,
    mut parts: Query<
        (&Transform, &mut GlobalTransform),
        (Without<ItemFx>, Without<Character>, Without<WeaponOf>),
    >,
    mut scratch: Local<Vec<gd_formats::msh::Coords>>,
    // This frame's poses of other meshes with their owners' keys: ((owner, mesh), range in `posed`), so each is
    // posed once however many effects it carries.
    mut poses: Local<(
        Vec<((Entity, usize), (usize, usize))>,
        Vec<gd_formats::msh::Coords>,
    )>,
) {
    let (keys, posed) = &mut *poses;
    keys.clear();
    posed.clear();
    for (e, f, tf, mut g, has_view, children) in &mut fx {
        let Ok((root, c, doll)) = owners.get(f.owner) else {
            continue;
        };
        let frame = match &f.anchor {
            Anchor::Weapon(w) => {
                let Ok(wg) = weapons.get(*w) else { continue };
                wg.affine()
            }
            Anchor::Bone(msh, i) => {
                // The body's own bones are posed already; another mesh is posed with the owner's keys.
                let abs = if Arc::ptr_eq(msh, &c.body) {
                    c.posed()
                } else {
                    let key = (f.owner, Arc::as_ptr(msh) as usize);
                    let (start, len) = match keys.iter().find(|(k, _)| *k == key) {
                        Some((_, r)) => *r,
                        None => {
                            gd_sim::anim::pose_abs_into(msh, &c.pose, &mut scratch);
                            let r = (posed.len(), scratch.len());
                            posed.extend_from_slice(&scratch);
                            keys.push((key, r));
                            r
                        }
                    };
                    &posed[start..start + len]
                };
                let Some(b) = abs.get(*i) else { continue };
                root.affine() * coords_to_affine(b)
            }
            Anchor::Owner => root.affine(),
        };
        *g = GlobalTransform::from(attach_frame(
            frame,
            f.local * Affine3A::from_scale(tf.scale),
            None,
        ));
        let layer = doll.and_then(|d| d.0.own_layer());
        if let (Some(layer), false) = (&layer, has_view) {
            commands.entity(e).insert(FxView(layer.clone()));
        }
        for p in children.into_iter().flat_map(|c| c.iter()) {
            if let Ok((ptf, mut pg)) = parts.get_mut(p) {
                *pg = g.mul_transform(*ptf);
                if let (Some(layer), false) = (&layer, has_view) {
                    commands.entity(p).insert(layer.clone());
                }
            }
        }
    }
}
