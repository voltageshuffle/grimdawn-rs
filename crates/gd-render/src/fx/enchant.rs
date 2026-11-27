//! Skill effects drawn on characters: the weapon enchantment
//! (`gd_game_core::skill_fx::WeaponEnchantments`) on each held weapon, weapon
//! trails during swings, and the `toggledAurasSelf` option on the player's
//! toggled-skill effects.
//!
//! Game.dll `Weapon::SetWeaponEnchantment`: it first
//! clears the old pak (`Weapon::ClearWeaponEnchantment`: tint back to
//! white, the trail back to the weapon's own (its transmute's when it has
//! one), the effect stopped and detached where it is so its particles
//! finish), then loads the new `WeaponEnchantmentPak` (loader `WeaponEnchantmentPak::Load`,
//! only for a weapon whose kind (`Weapon.kind`) is set: 1 melee (`Weapon`'s
//! constructor), 2 `Ranged1h`, 3 `Ranged2h`, 4 `Staff`; `WeaponArmor_Offhand`
//! and `WeaponArmor_Shield` are 0 and get nothing): `weaponTrail`,
//! `weaponEffect`, `weaponActiveEffect`, `useWeaponTint` with
//! `weaponTint{Red,Green,Blue,Alpha}` / 255, per kind the swipe / hit sounds
//! and (ranged) `projectileName`. With the tint set it calls
//! `Actor::SetTintColor`; with a trail `SwitchWeaponTrail(trail, false)`; the
//! `weaponEffect` is attached to the weapon at its origin (identity coords)
//! and starts emitting; the `weaponActiveEffect` is attached stopped and
//! emits only while the weapon trail is on (`EnableWeaponTrail` /
//! `DisableWeaponTrail`; no stock pak has one).
//!
//! Weapon trails (`Weapon::SwitchWeaponTrail`,
//! `SkillActivatedWeapon::SwipeAction`): a weapon's
//! trail (`weaponTrail` of its record) is a `TrailEffect` attached to it;
//! an attack animation's `SwipeRight` / `SwipeLeft` / `SwipeBoth` callback
//! calls `EnableWeaponTrail` on the right / left / both weapons (`Start`),
//! the attack action's `Finish` `DisableWeaponTrail` (`Stop`); only weapon
//! skills have a `SwipeAction` (`SkillActivatedWeapon`, `…WeaponPool`,
//! Telekinesis, Kick), not spells. A trail
//! switched mid-swing keeps running until that stop.
//!
//! The paper doll copies its items from replicas (`Paperdoll::SyncItems`:
//! `Item::CreateItem`, no `SetWeaponEnchantment`), so it shows no
//! enchantment; it gets no trails either.
//!
//! PORT: `weaponActiveEffect` is not loaded; the swipe / hit sounds are loaded and played by
//! `gd_game::combat_sounds` (`enchant_sounds`, the same per-kind fields; the pak's sound in
//! addition to the weapon's own, `Weapon::PlaySwipeSound` / `PlayHitSound`); the trail stops when
//! the animation the swipe started in stops playing (the attack action's end); swipes count in
//! the Attack slot and in a weapon skill's cast.
use super::items::{Anchor, ItemFx};
use super::{Effect, StopEmitting, spawn_effect_record};
use crate::character::Body;
use crate::model::{attach_frame, coords_to_affine};
use crate::render::material::GdMaterial;
use crate::trails::Trail;
use bevy::prelude::*;
use gd_formats::msh::AttachPoint;
use gd_game_core::character::{AnimCallback, Character};
use gd_game_core::data::GameData;
use gd_game_core::options::GameOptions;
use gd_game_core::player::Player;
use gd_game_core::skill_fx::{WeaponEnchantments, enchantment_visible, toggle_fx_visible};
use gd_game_core::visuals::BuffFx;
use gd_sim::anim::AnimType;
use gd_sim::equip::WeaponType;
use std::sync::Arc;

pub struct EnchantPlugin;

impl Plugin for EnchantPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (
                dress_weapons,
                tint_weapons,
                form_trails,
                swipe_trails,
                toggle_fx_visibility,
            )
                .chain()
                .before(super::FxSystems)
                .before(super::items::follow),
        )
        .add_systems(
            PostUpdate,
            follow_points
                .after(TransformSystems::Propagate)
                .after(crate::character::CharacterRenderStage::Pose)
                .before(crate::model::animate_skeletons)
                .before(super::FxSystems),
        );
    }
}

/// On a held weapon's root: what its enchantment and trail are now.
#[derive(Component)]
pub struct WeaponDress {
    /// The pak applied (None: none or not a kind that loads one).
    pak: Option<String>,
    /// The weapon's class takes a pak (`takes_pak`, fixed per weapon).
    takes_pak: bool,
    effect: Option<Entity>,
    /// The trail record and entity.
    trail: Option<(String, Entity)>,
    /// A switched trail finishing its swing (`Weapon.finishingTrail`).
    finishing: Option<Entity>,
    /// The animation a swipe started the trail in.
    swing: Option<AnimType>,
    /// The pak's `weaponTint` when `useWeaponTint` is set (rgb, 0..1).
    tint: Option<Vec3>,
}

/// On a weapon root whose parts wear a tinted copy of their material: the
/// tint applied and the materials to put back.
#[derive(Component, Default)]
struct WeaponTinted {
    applied: Option<Vec3>,
    saved: Vec<(Entity, Handle<GdMaterial>)>,
}

/// `Weapon.kind`: 0 for an off-hand or shield (no pak loads).
fn takes_pak(class: &str) -> bool {
    !matches!(
        WeaponType::from_class(class),
        Some(WeaponType::Shield | WeaponType::Offhand)
    )
}

/// Applies each character's shown weapon enchantment and trail to its
/// held weapons (the paper doll's excluded, module docs).
#[allow(clippy::too_many_arguments)]
fn dress_weapons(
    mut commands: Commands,
    data: Res<GameData>,
    options: Res<GameOptions>,
    chars: Query<
        (Entity, &Body, Option<&WeaponEnchantments>, Has<Player>),
        Without<crate::character::DollBody>,
    >,
    mut weapons: Query<Option<&mut WeaponDress>>,
    effects: Query<&GlobalTransform>,
    mut running: Query<&mut Effect>,
    trails: Query<&Trail>,
) {
    for (owner, body, ench, player) in &chars {
        let shown = ench
            .filter(|_| enchantment_visible(&options, player))
            .and_then(|w| w.shown());
        for w in &body.weapons {
            let Ok(dress) = weapons.get_mut(w.root) else {
                continue;
            };
            if dress
                .as_ref()
                .is_some_and(|d| d.pak.as_deref() == shown.filter(|_| d.takes_pak))
            {
                continue;
            }
            let Some(rec) = data.db.get(&w.item) else {
                continue;
            };
            let takes = takes_pak(&rec.class);
            let pak = shown.filter(|_| takes).map(str::to_owned);
            if dress.as_ref().is_some_and(|d| d.pak == pak) {
                continue;
            }
            let prec = pak.as_deref().and_then(|p| data.db.get(p));
            let (old_effect, old_trail) = dress
                .as_ref()
                .map_or((None, None), |d| (d.effect, d.trail.clone()));
            // The old effect stops where it is; its particles finish.
            if let Some(f) = old_effect {
                if let Ok(g) = effects.get(f) {
                    commands
                        .entity(f)
                        .try_insert(g.compute_transform())
                        .remove::<(ItemFx, ChildOf)>();
                }
                if let Ok(mut e) = running.get_mut(f) {
                    e.despawn_when_complete = true;
                    commands.entity(f).try_insert(StopEmitting);
                } else {
                    commands.entity(f).try_despawn();
                }
            }
            let effect = prec.as_ref().and_then(|p| p.str("weaponEffect")).map(|fx| {
                let e = spawn_effect_record(&mut commands, fx, Transform::IDENTITY);
                commands.entity(e).insert(ItemFx {
                    owner,
                    anchor: Anchor::Weapon(w.root),
                    local: bevy::math::Affine3A::IDENTITY,
                });
                commands.entity(w.root).add_child(e);
                e
            });
            // The trail: the pak's, else the weapon's own.
            let want = prec
                .as_ref()
                .and_then(|p| p.str("weaponTrail"))
                .or(rec.str("weaponTrail"))
                .map(str::to_owned);
            let anchors = gd_game_core::visuals::anchors_of(&w.msh, false);
            let old_finishing = dress.as_ref().and_then(|d| d.finishing);
            let mut finishing = old_finishing;
            let trail = match (old_trail, want, anchors) {
                (Some((r, e)), Some(want), _) if r == want => Some((r, e)),
                (old, want, anchors) => {
                    // `Weapon::SwitchWeaponTrail`: an old trail with segments stays attached and runs on until the
                    // swing stops it and its ribbon is gone ([`swipe_trails`]); one without goes at once.
                    if let Some((_, e)) = old {
                        if trails.get(e).is_ok_and(|t| t.has_segments()) {
                            finishing = Some(e);
                        } else {
                            commands.entity(e).try_despawn();
                        }
                    }
                    want.zip(anchors).map(|(r, a)| {
                        let e = commands
                            .spawn(Trail::new(w.root, &r, 0.0).with_anchors(a).stopped())
                            .id();
                        (r, e)
                    })
                }
            };
            // A trail replaced while another was finishing: that one is let go to finish on its own.
            if let Some(f) = old_finishing.filter(|f| Some(*f) != finishing) {
                commands.entity(f).try_insert(StopEmitting);
            }
            if pak.as_deref() != dress.as_ref().and_then(|d| d.pak.as_deref()) {
                debug!(
                    "{owner}: weapon {} enchantment {:?}",
                    w.item.rsplit('/').next().unwrap_or(""),
                    pak.as_deref().map(|p| p.rsplit('/').next().unwrap_or(""))
                );
            }
            let tint = prec
                .as_ref()
                .filter(|p| p.f32("useWeaponTint").unwrap_or(0.0) != 0.0)
                .map(|p| {
                    let c = |k: &str| p.f32(k).unwrap_or(255.0) / 255.0;
                    Vec3::new(
                        c("weaponTintRed"),
                        c("weaponTintGreen"),
                        c("weaponTintBlue"),
                    )
                });
            let new = WeaponDress {
                pak,
                takes_pak: takes,
                effect,
                trail,
                finishing,
                swing: dress.as_ref().and_then(|d| d.swing),
                tint,
            };
            match dress {
                Some(mut d) => *d = new,
                None => {
                    commands.entity(w.root).try_insert(new);
                }
            }
        }
    }
}

/// The weapon enchantment's tint on the weapon's meshes.
///
/// `Actor::SetTintColor` (Engine.dll) stores the colour
/// in `Actor.tintColor`; `Actor::AddToScene` copies it to the mesh
/// instance (`GraphicsMeshInstance.diffuseColor`, `GraphicsMeshInstance::SetDiffuseColor`) and
/// `GraphicsMeshInstance::SetShaderParameters` sends the shader
/// parameter `diffuseColor` = (instance colour rgb x 0.5, ambient add): the
/// default white is the 0.5 every [`GdMaterial`] carries. The tint alpha is
/// not used there. PORT: a tinted weapon's parts get a copy of their material
/// with `diffuse.rgb` = 0.5 x tint (the shader squares `2 x diffuse` like the
/// engine's); no stock pak sets `useWeaponTint`, so this only shows with modded data.
fn tint_weapons(
    mut commands: Commands,
    chars: Query<&Body, Without<crate::character::DollBody>>,
    dress: Query<(&WeaponDress, Option<&WeaponTinted>)>,
    mut parts: Query<&mut MeshMaterial3d<GdMaterial>>,
    mut materials: ResMut<Assets<GdMaterial>>,
) {
    for body in &chars {
        for w in &body.weapons {
            let Ok((d, tinted)) = dress.get(w.root) else {
                continue;
            };
            let have = tinted.and_then(|t| t.applied);
            if d.tint == have {
                continue;
            }
            let mut state = WeaponTinted::default();
            if let Some(t) = tinted {
                for (e, h) in &t.saved {
                    if let Ok(mut m) = parts.get_mut(*e) {
                        m.0 = h.clone();
                    }
                }
            }
            if let Some(tint) = d.tint {
                // Parts whose Grim Dawn material has not been swapped in yet: try again next frame.
                if !w.parts.iter().any(|p| parts.get(*p).is_ok()) {
                    continue;
                }
                for p in &w.parts {
                    let Ok(mut m) = parts.get_mut(*p) else {
                        continue;
                    };
                    let Some(mut copy) = materials.get(&m.0).cloned() else {
                        continue;
                    };
                    copy.params.diffuse = (tint * 0.5).extend(copy.params.diffuse.w);
                    state.saved.push((*p, m.0.clone()));
                    m.0 = materials.add(copy);
                }
                state.applied = Some(tint);
            }
            commands.entity(w.root).try_insert(state);
        }
    }
}

/// Moves each [`BuffFx`] with an attach point onto it: the point's frame on
/// the posed bone, in the owner's frame (`PFxManager::StartParticleEffect`:
/// `Attach` to the attach point by name; the parent bone is
/// followed as for [`super::items`] armour effects). A mesh effect with
/// `stationaryAttachment` ([`super::mesh::Stationary`]) keeps its position on
/// the owner but not the owner's turn: Engine.dll
/// `Entity::UpdateAttachedEntities` / `Actor::FastUpdate`: each
/// update its attach offset (`Entity.attachOffset`, `SetAttachOffset`) becomes the parent's
/// rotation transposed, applied after the attach frame.
fn follow_points(
    mut commands: Commands,
    owners: Query<(&GlobalTransform, &Character), Without<BuffFx>>,
    mut fx: Query<(
        Entity,
        &BuffFx,
        &Transform,
        &mut GlobalTransform,
        Option<&Children>,
        Has<super::mesh::Stationary>,
        Option<&FollowPoint>,
    )>,
    mut parts: Query<(&Transform, &mut GlobalTransform), (Without<BuffFx>, Without<Character>)>,
) {
    for (e, b, tf, mut g, children, stationary, cached) in &mut fx {
        let Ok((root, c)) = owners.get(b.owner) else {
            continue;
        };
        let frame = match &b.point {
            Some(point) => {
                // The point resolved on the body mesh once; a form swap that replaces the mesh re-resolves it.
                let fresh;
                let ap = match cached {
                    Some(f) if Arc::ptr_eq(&f.body, &c.body) => f.point.as_ref(),
                    _ => {
                        fresh = c.body.attach_point(point).cloned();
                        commands.entity(e).try_insert(FollowPoint {
                            body: c.body.clone(),
                            point: fresh.clone(),
                        });
                        fresh.as_ref()
                    }
                };
                let Some(ap) = ap else {
                    continue;
                };
                coords_to_affine(&ap.coords_in(c.posed()))
            }
            // No point: the propagation placed it; only a stationary one needs the turn taken out.
            None if stationary => tf.compute_affine(),
            None => continue,
        };
        let world = attach_frame(root.affine(), frame, stationary.then_some(root));
        *g = GlobalTransform::from(world);
        // A mesh effect's parts were placed by the transform propagation before: move them along.
        for c in children.into_iter().flat_map(|c| c.iter()) {
            if let Ok((tf, mut pg)) = parts.get_mut(c) {
                *pg = g.mul_transform(*tf);
            }
        }
    }
}

/// A [`BuffFx`]'s attach point resolved on its owner's body mesh; re-resolved when a form swap replaces the mesh.
/// `None`: the mesh lacks the name.
#[derive(Component)]
struct FollowPoint {
    body: Arc<gd_formats::msh::Mesh>,
    point: Option<AttachPoint>,
}

/// A character's unarmed weapon trails (`Character::SetUnarmedWeaponTrail`):
/// the form's `attackTrail` (`Character::form_records`)
/// twice, attached to the character: the right hand's (`Character.unarmedTrailRight`,
/// `GetUnarmedWeaponTrailRH`) with point set 1, i.e. between the body's
/// `Anchor3` / `Anchor4`, the left hand's (`Character.unarmedTrailLeft`) with set 0, `Anchor1` /
/// `Anchor2` (Engine.dll `WeaponTrail::GetAnchorPoints`; on the
/// forms' meshes `Anchor1` / `2` sit on `Wrist_R`, `3` / `4` on `Wrist_L`).
/// Setting a new one destroys the old; coming out of the form sets none.
/// PORT: no trail for a mesh without the first anchor of the set (the
/// engine's would sit at the origin); an old trail fades out.
#[derive(Component)]
pub struct UnarmedTrails {
    record: String,
    right: Option<Entity>,
    left: Option<Entity>,
    swing: Option<AnimType>,
}

fn form_trails(
    mut commands: Commands,
    chars: Query<(Entity, &Character, Option<&UnarmedTrails>), Without<crate::character::DollBody>>,
    mut trails: Query<&mut Trail>,
) {
    for (e, c, cur) in &chars {
        let want = c
            .form_records()
            .map(|f| f.trail.as_str())
            .filter(|t| !t.is_empty());
        if cur.map(|u| u.record.as_str()) == want {
            continue;
        }
        for t in cur.into_iter().flat_map(|u| [u.right, u.left]).flatten() {
            // Let go (`StopEmitting`): its segments fade out, then it goes.
            if let Ok(mut tr) = trails.get_mut(t) {
                tr.stop();
                commands.entity(t).try_insert(StopEmitting);
            }
        }
        let Some(record) = want else {
            commands.entity(e).try_remove::<UnarmedTrails>();
            continue;
        };
        let has = |n: &str| c.body.attach_point(n).is_some();
        let mut make = |a: &str, b: &str| {
            has(a).then(|| {
                commands
                    .spawn(
                        Trail::new(e, record, 0.0)
                            .with_points(&c.body, a, b)
                            .stopped(),
                    )
                    .id()
            })
        };
        let (right, left) = (make("Anchor3", "Anchor4"), make("Anchor1", "Anchor2"));
        debug!(
            "{e}: unarmed trail {record} (right {}, left {})",
            right.is_some(),
            left.is_some()
        );
        commands.entity(e).try_insert(UnarmedTrails {
            record: record.to_owned(),
            right,
            left,
            swing: None,
        });
    }
}

/// `SwipeRight` / `SwipeLeft` / `SwipeBoth` start the hand's weapon trail;
/// it stops when that animation stops playing. A shifted character
/// (`SkillActivatedWeapon::SwipeAction`: the active
/// skill set (`Character.activeSkillSet`) is not 0; PORT: a worn form) starts its unarmed trails
/// instead ([`UnarmedTrails`]: right for `SwipeRight` / `SwipeBoth`, left
/// for `SwipeLeft` / `SwipeBoth`), never the (hidden) weapons'; its
/// `SwipeRightOff` / `SwipeLeftOff` / `SwipeBothOff` stop them
/// (`Skill::DisableWeaponTrails`).
fn swipe_trails(
    mut callbacks: MessageReader<AnimCallback>,
    mut chars: Query<(
        &Character,
        &Body,
        Option<&mut UnarmedTrails>,
        Option<&gd_game_core::casting::Casting>,
    )>,
    mut weapons: Query<&mut WeaponDress>,
    mut trails: Query<&mut Trail>,
    mut commands: Commands,
) {
    for cb in callbacks.read() {
        let (right, left, on) = match cb.name.as_str() {
            "SwipeRight" => (true, false, true),
            "SwipeLeft" => (false, true, true),
            "SwipeBoth" => (true, true, true),
            "SwipeRightOff" => (true, false, false),
            "SwipeLeftOff" => (false, true, false),
            "SwipeBothOff" => (true, true, false),
            _ => continue,
        };
        // Only weapon attacks swipe (`SkillActivatedWeapon::SwipeAction`;
        // a spell's animation callbacks reach no weapon).
        let Ok((c, body, unarmed, cast)) = chars.get_mut(cb.entity) else {
            continue;
        };
        if !gd_game_core::char_sounds::weapon_swipe(cb.anim, cast) {
            continue;
        }
        if c.form_mesh().is_some() {
            let Some(mut u) = unarmed else { continue };
            for t in [u.right.filter(|_| right), u.left.filter(|_| left)]
                .into_iter()
                .flatten()
            {
                if let Ok(mut t) = trails.get_mut(t) {
                    if on {
                        gd_game_core::visuals::notify(&mut commands, &u.record);
                        debug!("{}: unarmed trail starts ({})", cb.entity, cb.name);
                        t.start();
                    } else {
                        t.stop();
                    }
                }
            }
            if on {
                u.swing = Some(cb.anim);
            }
            continue;
        }
        if !on {
            continue;
        }
        for w in body
            .weapons
            .iter()
            .filter(|w| (w.right && right) || (!w.right && left))
        {
            let Ok(mut d) = weapons.get_mut(w.root) else {
                continue;
            };
            let Some((rec, t)) = d.trail.clone() else {
                continue;
            };
            if let Ok(mut t) = trails.get_mut(t) {
                gd_game_core::visuals::notify(&mut commands, &rec);
                debug!(
                    "{}: trail {} starts",
                    cb.entity,
                    w.item.rsplit('/').next().unwrap_or("")
                );
                t.start();
                d.swing = Some(cb.anim);
            }
        }
    }
    for (c, body, unarmed, _) in &mut chars {
        if let Some(mut u) = unarmed
            && u.swing.is_some_and(|a| a != c.playing)
        {
            u.swing = None;
            for t in [u.right, u.left].into_iter().flatten() {
                if let Ok(mut t) = trails.get_mut(t) {
                    t.stop();
                }
            }
        }
        for w in &body.weapons {
            let Ok(mut d) = weapons.get_mut(w.root) else {
                continue;
            };
            if d.swing.is_some_and(|a| a != c.playing) {
                d.swing = None;
                // `DisableWeaponTrail`: the current and a finishing trail stop.
                for t in d.trail.iter().map(|(_, t)| *t).chain(d.finishing) {
                    if let Ok(mut t) = trails.get_mut(t) {
                        t.stop();
                    }
                }
            }
            // `Weapon::UpdateSelf`: a finishing trail goes once its ribbon is gone.
            if let Some(f) = d.finishing
                && trails.get(f).is_ok_and(|t| !t.has_segments())
            {
                commands.entity(f).try_despawn();
                d.finishing = None;
            }
        }
    }
}

/// `CharFxPak::UpdateVisibility` / `Skill::UpdateFxVisibility`: the player's
/// toggled-skill effects follow `toggledAurasSelf`.
fn toggle_fx_visibility(
    options: Res<GameOptions>,
    players: Query<(), With<Player>>,
    mut fx: Query<(&BuffFx, &mut Visibility)>,
) {
    for (b, mut v) in &mut fx {
        let want = if toggle_fx_visible(&options, b, players.get(b.owner).is_ok()) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        v.set_if_neq(want);
    }
}
