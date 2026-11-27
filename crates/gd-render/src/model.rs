//! `.msh` models in Bevy: one mesh per material and fading section, skeletons as joint entities
//! whose `GlobalTransform` is written directly from the engine's pose math
//! (`gd_sim::anim::pose_abs_into`, the engine's `GetAbsCoords` math) so non-uniform/sheared bone keys are exact.
//!
//! PORT: the engine draws a mesh face set by face set (`GraphicsMesh`); the port merges the face sets that
//! share a material and fading section (`part_sections`) into one Bevy mesh with their index ranges
//! concatenated in face-set order, holding only the vertices they use. Bone indices are already global
//! (`Msh::global_bone_indices`), so the face sets' palettes are not needed to draw.
//!
//! Conventions: object space is right-handed, Y-up, matching
//! Bevy; the game winds front faces clockwise, so triangles are reversed.
//!
//! Animation entity events ([`anim_entities`] for object models,
//! [`character_anim_entities`] for characters), Engine.dll
//! `AnimChannel::Advance`: each `CreateEntity { frame, entity,
//! attach }` of the playing `.anm` fires as the channel's advance crosses its
//! frame — the same interval as the callback points, one wrap rule
//! (`gd_sim::anim::ChannelEvents::entities`; again on every pass of a looped
//! one) — and creates `entity` at the mesh's `attach` point (`Entity::
//! CreateAttachedEntity`: `ObjectManager` create, then the
//! actor's `Attach` by point name with the identity offset); an
//! `EffectEntity` (or subclass) made so is a self-deleting child
//! (`EffectEntity::MakeSelfDeletingChild`: it goes once its emitters are
//! done). Only the channel's current animation fires: `AnimChannel::Update`
//! passes the entity to the fading ones as null. Objects animate through the
//! same channel as characters ([`Animator`] + its `ObjectAnim` channel:
//! cross-fades, callbacks and entity events from the one advance).
//! A `RemoveEntity { frame, entity, attach }` passing removes the
//! first entity attached to the root with that record and attach point
//! (`Entity::RemoveAttachedEntity`: `Detach` + `DestroyObjectEx`, no fade).
//! The effect follows its attach point as posed this frame ([`AnimFx`]:
//! `Actor::GetAttachedCoordsInRegion`, the parent bone's posed frame × the
//! point, scaled by the actor) and goes with the root, unless the root has
//! [`DetachEffects`] (it then stops emitting and fades out where it is).
//! A `GibEffectEntity` also launches its gib meshes from the point
//! ([`AnimGibs`]; `GibEffectEntity::InitialUpdate`).
//! PORT:
//! the engine's `RemoveAttachedEntity` compares the attach name with a
//! function-static `Name` made from its first call's argument, the port
//! compares each event's own; paper dolls fire nothing.
//! PORT: the model cache holds its models weakly and each spawned part holds its model
//! ([`ModelRef`]), so a model and its meshes go with its last instance instead of 5 s after (the engine's
//! `BaseResourceManager`).
//! PORT: an animated object model out of every view (its bounds beyond `character::POSE_MARGIN` of every active
//! camera's frustum, the characters' rule) still advances its channel, so its clock, callbacks and entity events run
//! on and it resumes in phase, but is not posed and its joints are not written until it comes back
//! ([`animate_skeletons`]).
use crate::render::material::{MaterialAssets, MeshFacts, PartMaterial};
use crate::textures::Textures;
use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::math::{Affine3A, Vec3A};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use gd_formats::anm::Animation;
use gd_formats::msh::{AttachPoint, Coords, Mesh as Msh};
use gd_game_core::character::{AnimCallback, Character, SharedAnims};
use gd_game_core::data::GameData;
use gd_sim::anim::{self, AnimChannel, AnimEntityEvent, AnimType, Pose};
use std::sync::Arc;

pub struct ModelPlugin;

impl Plugin for ModelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModelCache>()
            .init_resource::<gd_game_core::character::AnimCache>()
            .init_resource::<crate::render::terrain::TerrainLayers>()
            .add_message::<ModChanged>()
            .add_message::<AnimGibs>()
            .add_systems(First, (drop_on_mod_change, collect_model_loads).chain())
            .register_required_components::<Animator, ObjectAnim>()
            .add_systems(
                PostUpdate,
                (
                    (
                        animate_skeletons,
                        anim_entities.in_set(AnimEntitySet),
                        follow_bone_fx,
                    )
                        .chain()
                        .after(TransformSystems::Propagate),
                    character_anim_entities
                        .in_set(AnimEntitySet)
                        .after(TransformSystems::Propagate),
                    // After the same-frame pose (characters and object models)
                    // and the events that create the effects, before the effect
                    // systems read their frames.
                    follow_anim_fx
                        .after(TransformSystems::Propagate)
                        .after(crate::character::CharacterRenderStage::Pose)
                        .after(animate_skeletons)
                        .after(anim_entities)
                        .after(character_anim_entities)
                        .before(crate::fx::FxSystems),
                ),
            );
        // The render cache shares gd-game's parsed-`.anm` store (`drop_on_mod_change` re-links it).
        let shared = app
            .world_mut()
            .resource::<gd_game_core::character::AnimCache>()
            .shared();
        app.world_mut().resource_mut::<ModelCache>().anims = shared;
        // `gd_game::session::set_mod_data` runs the same drop synchronously with the mod swap; the `First` run above
        // is the backstop for a mount that did not go through it.
        let drop = app.world_mut().register_system(drop_on_mod_change);
        app.world_mut()
            .insert_resource(gd_game_core::data::ModCacheDrop(drop));
    }
}

/// GPU-ready parts of a model, shared by every instance (each part holds it, [`ModelRef`]). A part is the face
/// sets of one material and fading section merged (module doc).
pub struct ModelAsset {
    pub msh: Arc<Msh>,
    pub parts: Vec<(Handle<Mesh>, PartMaterial)>,
    /// Mesh material index of each part (for per-material texture overrides).
    pub part_materials: Vec<u32>,
    /// Mesh section (`Msh::sections` index, the face set's +64 int) of each
    /// part; `None` when the part's material can never fade (no "…T.ssh"
    /// variant of its shader, [`transparent_variant_exists`]).
    pub part_sections: Vec<Option<u32>>,
    /// Static culling AABB of every skinned part, the whole mesh's ([`conservative_bounds`]).
    pub skinned_bounds: Aabb,
    /// What the materials were built for ([`MaterialAssets::reshade`]).
    pub mesh: MeshFacts,
    pub inverse_bindposes: Option<Handle<SkinnedMeshInverseBindposes>>,
    /// The objects the mesh creates at its attach points.
    pub creations: Arc<[crate::fx::MeshCreation]>,
}

impl ModelAsset {
    /// The msh material of material index `i`, when the mesh has one.
    pub fn msh_material(&self, i: u32) -> Option<&gd_formats::msh::Material> {
        self.msh.materials.get(i as usize)
    }
}

/// On each part entity of a spawned model: the model, kept while the instance lives ([`ModelCache`] holds it
/// weakly).
#[derive(Component)]
pub struct ModelRef(pub Arc<ModelAsset>);

#[derive(Resource, Default)]
pub struct ModelCache {
    /// Built models by normalized mesh path, held weakly ([`ModelRef`]).
    models: gd_game_core::weak_cache::WeakCache<ModelAsset>,
    /// The shared parsed-`.anm` store ([`SharedAnims`], gd-game's `AnimCache`).
    anims: SharedAnims,
    /// The mod mounted when the caches were filled ([`drop_on_mod_change`]).
    mounted: Option<String>,
    /// Models read and built in the background ([`ModelCache::model_ready`], [`collect_model_loads`]).
    loading: Vec<bevy::tasks::Task<(String, Option<ModelCpu>)>>,
    /// The paths of `loading`, so a model is read once.
    in_flight: gd_formats::collections::HashSet<String>,
    /// Models just built, held until their waiters take them ([`gd_game_core::weak_cache::Fresh`]).
    fresh: gd_game_core::weak_cache::Fresh<Arc<ModelAsset>>,
}

/// The mounted mod changed and the render caches were dropped ([`drop_on_mod_change`]).
#[derive(Message)]
pub struct ModChanged;

/// The render caches keyed by asset path (models, animations, textures, effects, terrain layers, shaders and grass
/// materials) hold files of the mounted data; when `GameData`'s mounted mod changes they are emptied, so a mod's
/// overrides are loaded and models share the new `BodyCache` meshes.
#[allow(clippy::too_many_arguments)]
pub fn drop_on_mod_change(
    data: Option<Res<GameData>>,
    mut cache: ResMut<ModelCache>,
    anims: ResMut<gd_game_core::character::AnimCache>,
    textures: Option<ResMut<Textures>>,
    fx: Option<ResMut<crate::fx::FxAssets>>,
    layers: Res<crate::render::terrain::TerrainLayers>,
    mut materials: ResMut<crate::render::material::MaterialCache>,
    mut mod_changed: MessageWriter<ModChanged>,
) {
    let Some(data) = data else { return };
    let active = data.active_mod();
    if cache.mounted.as_deref() == active {
        return;
    }
    cache.models.clear();
    cache.loading.clear();
    cache.in_flight.clear();
    cache.fresh.clear();
    // Re-link the shared anim store (a session's mod mount replaces `AnimCache`) and clear it.
    cache.anims = anims.shared();
    cache.anims.lock().clear();
    cache.mounted = active.map(str::to_owned);
    if let Some(mut t) = textures {
        t.clear();
    }
    if let Some(mut fx) = fx {
        fx.clear();
    }
    layers.clear();
    materials.clear();
    mod_changed.write(ModChanged);
}

/// Skeleton state on a model root.
#[derive(Component)]
pub struct Skeleton {
    pub msh: Arc<Msh>,
    pub joints: Vec<Entity>,
}

/// Plays one animation on a skeleton: the fields are the clip to play (a replaced or mutated `Animator` restarts it
/// with a cross-fade, one whose time alone moved seeks it); the playback runs through the required `ObjectAnim`
/// channel. `time` is seconds into the clip, written back each frame as the channel advances.
#[derive(Component)]
pub struct Animator {
    pub anim: Arc<Animation>,
    pub time: f32,
    pub speed: f32,
    pub looped: bool,
}

/// The [`AnimChannel`] behind an [`Animator`] (required component): objects animate through the same channel as
/// characters (module doc).
#[derive(Component, Default)]
pub(crate) struct ObjectAnim {
    channel: AnimChannel,
    /// The keyed pose: the channel's blended keys, posed into [`Self::abs`] ([`animate_skeletons`]).
    pose: Pose,
    /// The last mesh-space pose: a settled clip keeps it.
    abs: Vec<Coords>,
    /// The joints lag [`Self::pose`] or the root: posing was skipped while the model was out of
    /// view (`character::near_view`); the first frame back in view poses it again.
    stale: bool,
    /// (anim, time, speed, looped) the channel was last synced with.
    synced: Option<(Arc<Animation>, f32, f32, bool)>,
    /// The entity events the advance crossed, until [`anim_entities`] drains them.
    events: Vec<AnimEntityEvent>,
}

/// An animation event's `GibEffectEntity` record fired at the point `at` ([`AnimEntitySet`]): its gib meshes launch
/// the same frame (after the set, before this frame's bounds and visibility).
#[derive(Message)]
pub struct AnimGibs {
    pub record: String,
    pub at: Vec3,
}

/// The systems firing the animation entity events ([`anim_entities`], [`character_anim_entities`];
/// PostUpdate, after transform propagation), which write [`AnimGibs`].
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnimEntitySet;

/// The PostUpdate systems that spawn models; a part has its material from the spawn, so what reads its material or
/// render layers orders after them. Every other spawner runs in `Update`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelSpawnSet;

/// An entity an animation created at an attach point of `root` (module doc):
/// it follows the point as posed each frame.
#[derive(Component)]
pub struct AnimFx {
    pub root: Entity,
    /// The created record (a `RemoveEntity` event finds it by record and point).
    pub record: String,
    pub point: AttachPoint,
    /// The root has [`DetachEffects`]: when it goes, stop emitting instead of vanishing.
    pub detach: bool,
    /// An `FxMesh`'s record `scale` (`fx::mesh` applies it once), kept over the point's frame.
    pub scale: f32,
}

/// An effect or light at a bone-parented attach point of `root`'s mesh
/// (record models, animated level scenery): follows the posed bone, `local` being the attach frame relative to the
/// bone (`Actor::Attach` to a bone). Assets hide an attachment this way (the Mogdrogen shrine's `FX_Rune` sits on an
/// unskinned bone 15 m below until the restore animation raises it).
#[derive(Component)]
pub struct BoneFx {
    pub root: Entity,
    pub bone: usize,
    pub local: Affine3A,
}

/// The world frame of an attachment this frame: `anchor`, the posed frame it hangs on (a joint, a character's posed
/// attach point, a weapon root, the root itself), × `local`, its frame relative to it; a `stationary` one (an
/// `FxMesh` with `stationaryAttachment`, the root given) keeps its place on the root but not the root's turn
/// (Engine.dll `Entity::UpdateAttachedEntities` / `Actor::FastUpdate`: each update its attach
/// offset becomes the parent's rotation transposed, applied after the attach frame). Every follower places by it.
pub fn attach_frame(
    anchor: Affine3A,
    local: Affine3A,
    stationary: Option<&GlobalTransform>,
) -> Affine3A {
    let world = anchor * local;
    match stationary {
        Some(root) => world * Affine3A::from_quat(root.rotation().inverse()),
        None => world,
    }
}

/// Places [`BoneFx`] attachments (a child of their root) at their bone's pose.
pub(crate) fn follow_bone_fx(
    mut fx: Query<(&BoneFx, &mut Transform, &mut GlobalTransform)>,
    roots: Query<(&GlobalTransform, &Skeleton), Without<BoneFx>>,
    joints: Query<&GlobalTransform, (Without<Skeleton>, Without<BoneFx>)>,
) {
    for (f, mut tf, mut g) in &mut fx {
        let Ok((root, skel)) = roots.get(f.root) else {
            continue;
        };
        let Some(joint) = skel.joints.get(f.bone).and_then(|j| joints.get(*j).ok()) else {
            continue;
        };
        let world = attach_frame(joint.affine(), f.local, None);
        let local = root.affine().inverse() * world;
        let t = crate::fx::affine_transform(local);
        tf.set_if_neq(t);
        // After Propagate: write the global frame too so the attachment follows this frame's posed bone.
        g.set_if_neq(GlobalTransform::from(world));
    }
}

/// On a model root: effects its animations attached stay when it goes,
/// stop emitting and finish (`FixedItemTeleport::UpdateSelf`:
/// a removed personal portal detaches its `EffectEntity` children,
/// `StopEmitting`, and leaves them in the world).
#[derive(Component)]
pub struct DetachEffects;

/// Fires the entity events the objects' channel advances crossed this frame
/// (module doc): `animate_skeletons` just collected them.
#[allow(clippy::type_complexity)]
fn anim_entities(
    mut commands: Commands,
    data: Res<GameData>,
    mut roots: Query<
        (
            Entity,
            &GlobalTransform,
            &Skeleton,
            &mut ObjectAnim,
            Has<DetachEffects>,
        ),
        Without<ExternalPose>,
    >,
    joints: Query<&GlobalTransform, Without<Skeleton>>,
    made: Query<(Entity, &AnimFx)>,
) {
    for (e, gt, skel, mut st, detach) in &mut roots {
        // Read first: draining an empty list would still flag every animated object changed.
        if st.events.is_empty() {
            continue;
        }
        for ev in st.events.drain(..) {
            if ev.create {
                let Some(point) = skel.msh.attach_point(&ev.attach) else {
                    continue;
                };
                let world = attach_world(point, gt, None, Some(skel), &joints, None);
                create_attached(&mut commands, &data, e, point, world, &ev.entity, detach);
            } else {
                remove_attached(&mut commands, &made, e, &ev.entity, &ev.attach);
            }
        }
    }
}

/// Fires the entity events each character's advance crossed this frame ([`Character::entity_events`], module doc).
fn character_anim_entities(
    mut commands: Commands,
    data: Res<GameData>,
    mut chars: Query<(
        Entity,
        &GlobalTransform,
        &mut Character,
        Has<crate::character::DollBody>,
    )>,
    joints: Query<&GlobalTransform, Without<Skeleton>>,
    made: Query<(Entity, &AnimFx)>,
) {
    for (e, gt, mut c, doll) in &mut chars {
        if c.entity_events.is_empty() {
            continue;
        }
        // Paper dolls fire nothing (module doc); drain so events do not pile up.
        let events = std::mem::take(&mut c.entity_events);
        if doll {
            continue;
        }
        for ev in events {
            if ev.create {
                let Some(point) = c.body.attach_point(&ev.attach) else {
                    continue;
                };
                let world = attach_world(point, gt, Some(&c), None, &joints, None);
                create_attached(&mut commands, &data, e, point, world, &ev.entity, false);
            } else {
                remove_attached(&mut commands, &made, e, &ev.entity, &ev.attach);
            }
        }
    }
}

/// `point`'s world frame on `root` as posed now: a character's posed bones, an
/// object model's posed joint, or the point in the root's space without a
/// parent bone (`Actor::GetAttachedCoordsInRegion`, module doc); `stationary`: [`attach_frame`].
fn attach_world<F: bevy::ecs::query::QueryFilter>(
    point: &AttachPoint,
    root: &GlobalTransform,
    character: Option<&Character>,
    skel: Option<&Skeleton>,
    joints: &Query<&GlobalTransform, F>,
    stationary: Option<&GlobalTransform>,
) -> Affine3A {
    if let Some(c) = character {
        let local = coords_to_affine(&point.coords_in(c.posed()));
        return attach_frame(root.affine(), local, stationary);
    }
    let anchor = point
        .parent_bone
        .zip(skel)
        .and_then(|(b, s)| s.joints.get(b))
        .and_then(|j| joints.get(*j).ok())
        .unwrap_or(root);
    attach_frame(anchor.affine(), coords_to_affine(&point.coords), stationary)
}

/// `Entity::CreateAttachedEntity` of an animation event (module doc). Attach
/// frames can be mirrored (`FX_Floor` on the endless rift: y = -Y); a
/// quaternion cannot hold a mirror, so the transform keeps it as a negative scale.
fn create_attached(
    commands: &mut Commands,
    data: &GameData,
    root: Entity,
    point: &AttachPoint,
    world: Affine3A,
    record: &str,
    detach: bool,
) {
    let Some(rec) = data.db.get(record) else {
        return;
    };
    let tf = Transform::from_matrix(Mat4::from(world));
    let fx = AnimFx {
        root,
        record: record.to_owned(),
        point: point.clone(),
        detach,
        scale: 1.0,
    };
    match rec.class.as_str() {
        "EffectEntity" | "GibEffectEntity" => {
            let mut spec = gd_game_core::visuals::EffectSpec::record(record);
            spec.despawn_when_complete = true;
            commands.spawn((
                spec,
                tf,
                GlobalTransform::from(tf),
                Visibility::default(),
                fx,
            ));
            if rec.class == "GibEffectEntity" {
                commands.write_message(AnimGibs {
                    record: record.to_owned(),
                    at: tf.translation,
                });
            }
        }
        "FxMesh" => {
            commands.spawn((
                gd_game_core::visuals::FxMeshRequest(record.to_owned()),
                tf,
                GlobalTransform::from(tf),
                AnimFx {
                    scale: rec.f32("scale").unwrap_or(1.0),
                    ..fx
                },
            ));
        }
        _ => {}
    }
}

/// `Entity::RemoveAttachedEntity`: the first entity attached to
/// `root` with this record (and this attach point, when one is named) goes now.
fn remove_attached(
    commands: &mut Commands,
    made: &Query<(Entity, &AnimFx)>,
    root: Entity,
    record: &str,
    attach: &str,
) {
    let same = |a: &str, b: &str| {
        a.len() == b.len()
            && a.bytes().zip(b.bytes()).all(|(x, y)| {
                x.eq_ignore_ascii_case(&y) || matches!((x, y), (b'\\', b'/') | (b'/', b'\\'))
            })
    };
    if let Some((e, _)) = made.iter().find(|(_, f)| {
        f.root == root
            && same(&f.record, record)
            && (attach.is_empty() || f.point.name.eq_ignore_ascii_case(attach))
    }) {
        commands.entity(e).try_despawn();
    }
}

/// Animation effects follow their attach point as posed; when the root goes
/// they go too, or stop emitting ([`DetachEffects`]). A `stationaryAttachment`
/// mesh effect keeps its place on the root but not the root's turn
/// (`Entity::UpdateAttachedEntities`). A root
/// can itself be such an effect (an animated `FxMesh` firing its own events):
/// parents go first, and a child is carried by its parent's move this pass
/// (the parent's joints were posed from its old frame).
#[allow(clippy::type_complexity)]
fn follow_anim_fx(
    mut commands: Commands,
    mut fx: Query<
        (
            Entity,
            &AnimFx,
            &mut Transform,
            Has<crate::fx::mesh::Stationary>,
        ),
        Without<gd_game_core::visuals::FxMeshRequest>,
    >,
    roots: Query<(Option<&Character>, Option<&Skeleton>)>,
    mut globals: Query<&mut GlobalTransform>,
    mut order: Local<Vec<(u8, Entity)>>,
    mut moved: Local<bevy::ecs::entity::EntityHashMap<(GlobalTransform, Affine3A)>>,
) {
    // Nesting depth: the effect roots above each effect (bounded, so a
    // malformed cycle cannot hang).
    order.clear();
    order.extend(fx.iter().map(|(e, f, ..)| {
        let mut depth = 0u8;
        let mut r = f.root;
        while depth < 8
            && let Ok((_, p, ..)) = fx.get(r)
        {
            r = p.root;
            depth += 1;
        }
        (depth, e)
    }));
    order.sort_unstable_by_key(|&(d, _)| d);
    // Effects moved this pass: their old frame and the move (old → new).
    moved.clear();
    for &(_, e) in order.iter() {
        let Ok((_, f, mut tf, stationary)) = fx.get_mut(e) else {
            continue;
        };
        let frame = match (roots.get(f.root), globals.get(f.root)) {
            (Ok((c, s)), Ok(&now)) => {
                let (root, carry) = moved
                    .get(&f.root)
                    .copied()
                    .unwrap_or((now, Affine3A::IDENTITY));
                let stationary = stationary.then_some(&now);
                Some(
                    carry * attach_world(&f.point, &root, c, s, &globals.as_readonly(), stationary),
                )
            }
            _ => None,
        };
        match frame {
            Some(m) => {
                let mut t = Transform::from_matrix(Mat4::from(m));
                t.scale *= f.scale;
                tf.set_if_neq(t);
                // After Propagate and the same-frame pose: write the global frame directly so the effect follows
                // this frame's attach point.
                if let Ok(mut g) = globals.get_mut(e) {
                    let (old, new) = (*g, GlobalTransform::from(t));
                    if g.set_if_neq(new) {
                        moved.insert(e, (old, new.affine() * old.affine().inverse()));
                    }
                }
            }
            None if f.detach => {
                commands
                    .entity(e)
                    .try_remove::<AnimFx>()
                    .try_insert(crate::fx::StopEmitting);
            }
            None => commands.entity(e).try_despawn(),
        }
    }
}

pub fn coords_to_mat4(c: &Coords) -> Mat4 {
    Mat4::from_cols_array(&c.to_mat4())
}

pub fn coords_to_affine(c: &Coords) -> Affine3A {
    Affine3A::from_mat4(coords_to_mat4(c))
}

impl ModelCache {
    /// Cached models still in use.
    pub fn count(&self) -> usize {
        self.models.live()
    }

    /// The parsed animation at `path`, from the shared gd-game store ([`SharedAnims`]).
    pub fn anim(&mut self, data: &GameData, path: &str) -> Option<Arc<Animation>> {
        gd_game_core::character::shared_anim(&self.anims, data, path)
    }

    /// The render model of the mesh at `path`, built once while an instance holds it ([`ModelRef`]). The parsed
    /// mesh is gd-game's (`gd_game_core::creature::BodyCache`), so a character's `Character::body` and its body
    /// part's `Skeleton::msh` are the same `Arc`. Loads on the spot; see [`ModelCache::model_ready`].
    #[allow(clippy::too_many_arguments)]
    pub fn model(
        &mut self,
        data: &GameData,
        bodies: &mut gd_game_core::creature::BodyCache,
        path: &str,
        meshes: &mut Assets<Mesh>,
        mats: &mut MaterialAssets,
        bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
        textures: &mut Textures,
        images: &mut Assets<Image>,
    ) -> Option<Arc<ModelAsset>> {
        let key = gd_formats::vfs::normalized(path);
        self.models.get_or_load(&key, || {
            bodies.get(data, &key).map(|msh| {
                finish_model(
                    model_cpu(data, msh, false),
                    data,
                    meshes,
                    mats,
                    bindposes,
                    textures,
                    images,
                )
            })
        })
    }

    /// The render model of the mesh at `path` when it is built; otherwise its load starts in the background
    /// ([`collect_model_loads`] finishes it on a later frame) and this returns [`Ready::Loading`].
    pub fn model_ready(
        &mut self,
        data: &GameData,
        bodies: &gd_game_core::creature::BodyCache,
        path: &str,
    ) -> Ready<Arc<ModelAsset>> {
        let key = gd_formats::vfs::normalized(path);
        match self.models.get_kept(&key) {
            Some(Some(m)) => return Ready::Done(m),
            Some(None) => return Ready::Failed,
            None => {}
        }
        if self.in_flight.contains(key.as_ref()) {
            return Ready::Loading;
        }
        // A mesh gd-game already parsed (a creature's body) only has its textures and parts built here.
        let msh = match bodies.loaded(&key) {
            Some(None) => {
                self.models.insert(&key, None);
                return Ready::Failed;
            }
            Some(Some(m)) => Some(m),
            None => None,
        };
        let key = key.into_owned();
        self.in_flight.insert(key.clone());
        let data = data.clone();
        self.loading
            .push(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                let msh = msh.or_else(|| {
                    data.vfs
                        .read(&key)
                        .and_then(|b| Msh::parse(&b))
                        .inspect_err(|e| warn!("mesh {key}: {e:#}"))
                        .ok()
                        .map(Arc::new)
                });
                let cpu = msh.map(|m| model_cpu(&data, m, true));
                (key, cpu)
            }));
        Ready::Loading
    }
}

/// Whether an asset a spawn needs is loaded ([`ModelCache::model_ready`]).
pub enum Ready<T> {
    Done(T),
    Loading,
    Failed,
}

/// Main-thread time a frame spends finishing background model loads (`finish_model`, GPU uploads) before leaving
/// the rest for the next frame. Outside gameplay ([`LoadFast`]) the cap widens: no gameplay needs the frame
/// time, and draining the queue fast is the whole job.
pub const FINISH_BUDGET: std::time::Duration = std::time::Duration::from_millis(2);

/// Not in gameplay (`AppState` other than `InGame` — the loading cover, the menu and its backdrop build,
/// set by the binary): background-load finishers and level spawning may spend most of a frame instead of
/// their in-game slice.
#[derive(Resource, Default)]
pub struct LoadFast(pub bool);

/// [`FINISH_BUDGET`] widened while [`LoadFast`] holds.
pub fn finish_budget(fast: Option<Res<LoadFast>>) -> std::time::Duration {
    if fast.is_some_and(|f| f.0) {
        std::time::Duration::from_millis(12)
    } else {
        FINISH_BUDGET
    }
}

/// Finishes the models read in the background ([`ModelCache::model_ready`], within [`finish_budget`]):
/// files their meshes and textures and
/// adds their GPU assets, kept warm until the waiting spawns take them. Stops after [`finish_budget`] is spent.
#[allow(clippy::too_many_arguments)]
pub fn collect_model_loads(
    mut cache: ResMut<ModelCache>,
    data: Res<GameData>,
    mut bodies: ResMut<gd_game_core::creature::BodyCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: MaterialAssets,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut textures: ResMut<Textures>,
    mut images: ResMut<Assets<Image>>,
    fast: Option<Res<LoadFast>>,
) {
    use bevy::tasks::{block_on, futures_lite::future};
    cache.fresh.tick();
    if cache.loading.is_empty() {
        return;
    }
    let cache = &mut *cache;
    let deadline = std::time::Instant::now() + finish_budget(fast);
    let mut done = 0;
    let mut i = 0;
    while i < cache.loading.len() {
        // The rest stay queued (finished tasks keep their result) for the next frame.
        if done > 0 && std::time::Instant::now() >= deadline {
            break;
        }
        let Some((key, cpu)) = block_on(future::poll_once(&mut cache.loading[i])) else {
            i += 1;
            continue;
        };
        drop(cache.loading.swap_remove(i));
        cache.in_flight.remove(&key);
        done += 1;
        let Some(mut cpu) = cpu else {
            cache.models.insert(&key, None);
            continue;
        };
        // One parsed mesh per path: a body gd-game parsed meanwhile is the one the model keeps.
        match bodies.loaded(&key) {
            Some(Some(m)) => cpu.msh = m,
            _ => bodies.put(&key, &cpu.msh),
        }
        let model = Arc::new(finish_model(
            cpu,
            &data,
            &mut meshes,
            &mut mats,
            &mut bindposes,
            &mut textures,
            &mut images,
        ));
        cache.models.insert_kept(&key, &model);
        cache.fresh.hold(model);
    }
}

/// Whether a material shader has the transparent variant a faded section
/// draws with.
///
/// Engine.dll `GraphicsMesh::LoadMaterials`: each material loads its shader (`MeshMaterial.shader`) and the same name
/// with 'T' inserted before the extension (`Engine::TransparentShaderName`) as its transparent shader
/// (`MeshMaterial.transparentShader`), or the shader itself when that file does not exist.
/// `GraphicsMeshInstance::GetShader2` draws a section with opacity < 1 with the transparent one, and
/// `GraphicsMeshInstance::RenderPass` always draws a material whose two shaders are the same. So
/// "standardstatic_nofade.ssh" (no "…_nofadeT.ssh" exists) marks the parts of a see-through object that stay.
pub fn transparent_variant_exists(vfs: &gd_formats::vfs::Vfs, shader: &str) -> bool {
    let s = shader.replace('\\', "/");
    match s.rfind('.') {
        Some(i) => vfs.exists(&format!("{}T{}", &s[..i], &s[i..])),
        None => false,
    }
}

/// Bevy tangents (xyz, w = handedness of the stored bitangent) of a mesh's vertices; empty without
/// normals and tangents.
pub fn msh_tangents(msh: &Msh) -> Vec<[f32; 4]> {
    let v = &msh.vertices;
    let n = msh.num_vertices as usize;
    if v.tangents.len() != n || v.normals.len() != n {
        return Vec::new();
    }
    (0..n)
        .map(|i| {
            let (nn, t) = (Vec3::from(v.normals[i]), Vec3::from(v.tangents[i]));
            let b = v.bitangents.get(i).map(|b| Vec3::from(*b));
            let w = b.map_or(1.0, |b| if nn.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 });
            [t.x, t.y, t.z, w]
        })
        .collect()
}

/// A model's data built off the game thread ([`model_cpu`]): the parsed mesh, each part's vertex data, the inverse
/// bind poses, the mesh's creations and (for a background load) its materials' textures decoded. [`finish_model`]
/// adds the GPU assets and materials.
struct ModelCpu {
    msh: Arc<Msh>,
    facts: MeshFacts,
    /// Each part's mesh (no colours yet), material index and fading section ([`model_cpu`]).
    parts: Vec<CpuPart>,
    /// The skinned parts' culling bound ([`conservative_bounds`] of the whole mesh).
    bounds: Option<Aabb>,
    inverse_bindposes: Option<Vec<Mat4>>,
    creations: Arc<[crate::fx::MeshCreation]>,
    /// The materials' textures by normalized path (`None`: unreadable), decoded for a background load.
    textures: Vec<(String, Option<Image>)>,
    /// The materials' `.ssh` analyses (cache key, info), for a background load.
    shaders: Vec<(String, Option<crate::render::material::ShaderInfo>)>,
}

/// One part of a [`ModelCpu`]: the face sets of one material and fading section merged into one mesh over the
/// vertices they use (module doc).
struct CpuPart {
    mesh: Mesh,
    material: u32,
    section: Option<u32>,
    /// The part's vertex colours, given to it only when its material uses them ([`finish_model`]).
    colors: Option<Vec<[f32; 4]>>,
}

/// `src`'s values at the vertices `used`, in that order.
fn gather<T: Copy>(src: &[T], used: &[u32]) -> Vec<T> {
    used.iter().map(|&u| src[u as usize]).collect()
}

fn model_cpu(data: &GameData, msh: Arc<Msh>, decode_textures: bool) -> ModelCpu {
    let v = &msh.vertices;
    let skinned = !v.bone_weights.is_empty() && !msh.bones.is_empty();
    let n = msh.num_vertices as usize;

    let tangents = msh_tangents(&msh);
    let joints = skinned.then(|| msh.global_bone_indices());
    // Vertex colours drive the foliage wind and tint the vertex-colour shaders (`FLAG_VERTEX_COLOR`,
    // `FLAG_VERTEX_ALPHA`); only those parts get them. B8G8R8A8 (Direct3D11.dll
    // `Direct3DVertexDeclaration::Direct3DVertexDeclaration`).
    let has_colors = v.colors.len() == n;
    let colors: Vec<[f32; 4]> = if has_colors {
        v.colors
            .iter()
            .map(|c| [c[2], c[1], c[0], c[3]].map(|x| x as f32 / 255.0))
            .collect()
    } else {
        Vec::new()
    };
    // Some meshes store weights summing to < 1 (influences beyond four dropped, e.g. coat tails). Skinning here is
    // in world space, where the missing part would pull a vertex toward the world origin; the game skins in model
    // space. Normalise them.
    let weights: Vec<[f32; 4]> = if skinned {
        v.bone_weights
            .iter()
            .map(|w| {
                let s: f32 = w.iter().sum();
                if s > 1e-4 {
                    w.map(|x| x / s)
                } else {
                    [1.0, 0.0, 0.0, 0.0]
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    // Whether each material can fade: its shader has a "…T.ssh" variant ([`transparent_variant_exists`]).
    let fades: Vec<bool> = msh
        .materials
        .iter()
        .map(|m| transparent_variant_exists(&data.vfs, &m.shader))
        .collect();
    // The face sets merged by (material, fading section), in face-set order (module doc): their triangles in the
    // mesh's vertex numbering.
    let mut groups: Vec<(u32, Option<u32>, Vec<u16>)> = Vec::new();
    for fs in &msh.face_sets {
        // A skinned mesh's set with an empty bone palette has only zero blend weights (`Msh::validate`): the
        // engine's skinning collapses it to the model origin, so it never shows (the weight fallback below would
        // pin it to bone 0).
        if skinned && fs.bones.is_empty() {
            continue;
        }
        let s = fs.start_face as usize * 3;
        let e = (s + fs.num_faces as usize * 3).min(msh.indices.len());
        if s >= e {
            continue;
        }
        let section = fades
            .get(fs.material as usize)
            .is_some_and(|f| *f)
            .then_some(fs.section.max(0) as u32);
        let g = match groups
            .iter()
            .position(|(m, sec, _)| *m == fs.material && *sec == section)
        {
            Some(g) => g,
            None => {
                groups.push((fs.material, section, Vec::new()));
                groups.len() - 1
            }
        };
        // Reverse winding: game front faces are clockwise.
        for t in msh.indices[s..e].as_chunks::<3>().0 {
            groups[g].2.extend_from_slice(&[t[0], t[2], t[1]]);
        }
    }
    // Each part keeps only the vertices its triangles use, renumbered in first-use order.
    let mut remap = vec![u32::MAX; n];
    let mut parts = Vec::with_capacity(groups.len());
    for (material, section, mut idx) in groups {
        let mut used: Vec<u32> = Vec::new();
        for i in &mut idx {
            let r = &mut remap[*i as usize];
            if *r == u32::MAX {
                *r = used.len() as u32;
                used.push(*i as u32);
            }
            *i = *r as u16;
        }
        for &u in &used {
            remap[u as usize] = u32::MAX;
        }
        let usage = RenderAssetUsages::RENDER_WORLD;
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, usage)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, gather(&v.positions, &used));
        if v.normals.len() == n {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, gather(&v.normals, &used));
        }
        if let Some(uv) = v.uvs.first().filter(|u| u.len() == n) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, gather(uv, &used));
        }
        // TEXCOORD1: detail-map UVs (standard*detail shaders).
        if let Some(uv) = v.uvs.get(1).filter(|u| u.len() == n) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, gather(uv, &used));
        }
        if !tangents.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, gather(&tangents, &used));
        }
        if let Some(j) = &joints {
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_JOINT_INDEX,
                VertexAttributeValues::Uint16x4(gather(j, &used)),
            );
            mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, gather(&weights, &used));
        }
        mesh.insert_indices(Indices::U16(idx));
        if skinned {
            mesh = mesh
                .with_generated_skinned_mesh_bounds()
                .unwrap_or_else(|_| {
                    Mesh::new(
                        PrimitiveTopology::TriangleList,
                        RenderAssetUsages::RENDER_WORLD,
                    )
                });
        }
        parts.push(CpuPart {
            mesh,
            material,
            section,
            colors: has_colors.then(|| gather(&colors, &used)),
        });
    }
    let mut textures = Vec::new();
    let mut analysed = Vec::new();
    if decode_textures {
        let mut keys: Vec<String> = msh
            .materials
            .iter()
            .flat_map(|m| &m.params)
            .filter_map(|p| match &p.value {
                gd_formats::msh::ParamValue::Texture(t) if !t.is_empty() => {
                    Some(gd_formats::vfs::normalized(t).into_owned())
                }
                _ => None,
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();
        textures = keys
            .into_iter()
            .map(|k| {
                let img = crate::textures::decode(data, &k, false)
                    .inspect_err(|e| warn!("texture {k}: {e:#}"))
                    .ok();
                (k, img)
            })
            .collect();
        let mut shaders: Vec<&str> = msh.materials.iter().map(|m| m.shader.as_str()).collect();
        shaders.sort_unstable();
        shaders.dedup();
        analysed = shaders
            .into_iter()
            .map(|s| crate::render::material::analyse_shader(data, s, skinned))
            .collect();
    }
    ModelCpu {
        facts: MeshFacts {
            skinned,
            tangents: !tangents.is_empty(),
            colors: has_colors,
        },
        parts,
        bounds: skinned.then(|| conservative_bounds(&v.positions)),
        inverse_bindposes: skinned.then(|| {
            msh.bones
                .iter()
                .map(|b| coords_to_mat4(&b.abs.inverse()))
                .collect()
        }),
        creations: crate::fx::mesh_creations(data, &msh),
        textures,
        shaders: analysed,
        msh,
    }
}

/// Adds a [`ModelCpu`]'s GPU assets and materials (the textures it decoded go into the texture cache first).
fn finish_model(
    cpu: ModelCpu,
    data: &GameData,
    meshes: &mut Assets<Mesh>,
    mats: &mut MaterialAssets,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    textures: &mut Textures,
    images: &mut Assets<Image>,
) -> ModelAsset {
    let ModelCpu {
        msh,
        facts,
        parts: cpu_parts,
        bounds,
        inverse_bindposes,
        creations,
        textures: decoded,
        shaders,
    } = cpu;
    for (key, img) in decoded {
        textures.put(&key, false, img, images);
    }
    for (path, info) in shaders {
        mats.cache.put_shader(path, info);
    }
    let color_material: Vec<bool> = msh
        .materials
        .iter()
        .map(|m| {
            facts.colors
                && mats
                    .cache
                    .shader_info(data, &m.shader, facts.skinned)
                    .is_some_and(|i| i.wind.is_some() || i.vertex_color || i.vertex_alpha)
        })
        .collect();
    let mut materials: Vec<PartMaterial> = msh
        .materials
        .iter()
        .map(|m| mats.msh_material(data, m, facts, textures, images))
        .collect();
    if materials.is_empty() {
        materials.push(mats.plain());
    }
    let mut parts = Vec::with_capacity(cpu_parts.len());
    let mut part_materials = Vec::with_capacity(cpu_parts.len());
    let mut part_sections = Vec::with_capacity(cpu_parts.len());
    for CpuPart {
        mut mesh,
        material,
        section,
        colors,
    } in cpu_parts
    {
        if color_material.get(material as usize) == Some(&true)
            && let Some(c) = colors
        {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, c);
        }
        let mat = materials
            .get(material as usize)
            .cloned()
            .unwrap_or_else(|| materials[0].clone());
        parts.push((meshes.add(mesh), mat));
        part_materials.push(material);
        part_sections.push(section);
    }
    ModelAsset {
        creations,
        msh,
        parts,
        part_materials,
        part_sections,
        skinned_bounds: bounds.unwrap_or_default(),
        mesh: facts,
        inverse_bindposes: inverse_bindposes
            .map(|b| bindposes.add(SkinnedMeshInverseBindposes::from(b))),
    }
}

/// A static culling AABB generous enough to enclose a skinned mesh's animation range: twice the whole mesh's
/// bind-pose half-extents plus half a metre.
fn conservative_bounds(positions: &[[f32; 3]]) -> Aabb {
    let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for &v in positions {
        let v = Vec3::from_array(v);
        min = min.min(v);
        max = max.max(v);
    }
    if positions.is_empty() {
        (min, max) = (Vec3::splat(-0.5), Vec3::splat(0.5));
    }
    let a = Aabb::from_min_max(min, max);
    Aabb {
        center: a.center,
        half_extents: a.half_extents * 2.0 + Vec3A::splat(0.5),
    }
}

/// Skeleton roots whose joints are written by another system instead of `animate_skeletons`.
#[derive(Component)]
pub struct ExternalPose;

/// Spawns a model instance as children of `root`; its parts hold the model ([`ModelRef`]).
pub fn spawn_model(commands: &mut Commands, root: Entity, model: &Arc<ModelAsset>) {
    spawn_model_with(commands, root, model, |_, m| m.clone());
}

/// [`spawn_model`] under a root spawned after this frame's transform propagation with its world
/// frame `at` already written: the parts and the joints take their world frames from it now, so the model draws
/// this frame.
pub fn spawn_model_at(
    commands: &mut Commands,
    root: Entity,
    model: &Arc<ModelAsset>,
    at: &GlobalTransform,
) {
    spawn_parts(commands, root, model, |_, m| m.clone(), Some(at));
}

/// `commands.entity(parent).add_child(child)` that survives a parent another system despawned in the same
/// frame (a level unloaded right after its breakables were added): the orphaned child is despawned instead
/// of the command panicking.
pub fn try_add_child(commands: &mut Commands, parent: Entity, child: Entity) {
    commands.queue(move |world: &mut World| {
        if let Ok(mut p) = world.get_entity_mut(parent) {
            p.add_child(child);
        } else if let Ok(c) = world.get_entity_mut(child) {
            c.despawn();
        }
    });
}

/// Like [`spawn_model`], with a per-part material choice
/// `(part material index, default material) -> material`. Returns the part entities.
pub fn spawn_model_with(
    commands: &mut Commands,
    root: Entity,
    model: &Arc<ModelAsset>,
    material: impl FnMut(u32, &PartMaterial) -> PartMaterial,
) -> Vec<Entity> {
    spawn_parts(commands, root, model, material, None)
}

/// [`spawn_model_with`]; `at`: the root's world frame, written to the parts and joints at once
/// ([`spawn_model_at`]), else propagation and [`animate_skeletons`] place them.
fn spawn_parts(
    commands: &mut Commands,
    root: Entity,
    model: &Arc<ModelAsset>,
    mut material: impl FnMut(u32, &PartMaterial) -> PartMaterial,
    at: Option<&GlobalTransform>,
) -> Vec<Entity> {
    let mut out = Vec::new();
    let joints: Vec<Entity> = match &model.inverse_bindposes {
        Some(_) => model
            .msh
            .bones
            .iter()
            .map(|b| {
                let mesh = coords_to_affine(&b.abs);
                commands
                    .spawn((
                        GlobalTransform::from(at.map_or(mesh, |g| g.affine() * mesh)),
                        Name::new(b.name.clone()),
                    ))
                    .id()
            })
            .collect(),
        None => Vec::new(),
    };
    if !joints.is_empty() {
        // Linked to the root once it is known to exist (a root despawned this frame takes them along).
        let linked = joints.clone();
        commands.queue(move |world: &mut World| {
            if let Ok(mut r) = world.get_entity_mut(root) {
                r.add_related::<JointOf>(&linked);
            } else {
                for j in linked {
                    if let Ok(j) = world.get_entity_mut(j) {
                        j.despawn();
                    }
                }
            }
        });
    }
    for ((mesh, mat), mi) in model.parts.iter().zip(&model.part_materials) {
        let mut e = commands.spawn((
            Mesh3d(mesh.clone()),
            Transform::IDENTITY,
            ModelRef(model.clone()),
        ));
        material(*mi, mat).insert(&mut e);
        if let Some(g) = at {
            e.insert(*g);
        }
        if let Some(ibp) = &model.inverse_bindposes {
            e.insert((
                SkinnedMesh {
                    inverse_bindposes: ibp.clone(),
                    joints: joints.clone(),
                },
                model.skinned_bounds,
            ));
        }
        let id = e.id();
        try_add_child(commands, root, id);
        out.push(id);
    }
    if !joints.is_empty() {
        commands.entity(root).try_insert(Skeleton {
            msh: model.msh.clone(),
            joints,
        });
    }
    out
}

/// On a skeleton joint: its model root. Joints are top-level (posed in world space each frame); despawning the
/// root despawns them ([`Joints`]).
#[derive(Component)]
#[relationship(relationship_target = Joints)]
pub struct JointOf(pub Entity);

/// On a model root: the joints of the skeletons spawned under it ([`JointOf`]), despawned with it.
#[derive(Component, Default)]
#[relationship_target(relationship = JointOf, linked_spawn)]
pub struct Joints(Vec<Entity>);

/// Poses each skeleton: an [`Animator`]'s clip through its `ObjectAnim`
/// channel every frame (cross-fades, callbacks and entity events as for
/// characters, module doc); a skeleton with none (a skinned prop placed without an `animation`) stays at the bind
/// pose, written only when the skeleton is new or its root moved. A settled one-shot (an opened chest or door) keeps
/// its last pose. Off view, the channel advances but nothing is posed (module doc).
pub(crate) fn animate_skeletons(
    time: Res<Time>,
    mut roots: Query<
        (
            Entity,
            Ref<GlobalTransform>,
            Ref<Skeleton>,
            Option<(&mut Animator, &mut ObjectAnim)>,
        ),
        Without<ExternalPose>,
    >,
    mut joints: Query<&mut GlobalTransform, Without<Skeleton>>,
    cameras: Query<(&Camera, &bevy::camera::primitives::Frustum)>,
    mut out: MessageWriter<AnimCallback>,
    mut buf: Local<Vec<Coords>>,
    mut frusta: Local<Vec<bevy::camera::primitives::Frustum>>,
) {
    let dt_ms = time.delta_secs() * 1000.0;
    crate::character::view_frusta(&cameras, &mut frusta);
    let frusta = &*frusta;
    for (e, root_tf, skel, animator) in &mut roots {
        let root = root_tf.affine();
        // Whether the model's bounds are near a view (`character::near_view`).
        let near = || match skel.msh.bounding_box {
            Some((lo, hi)) => {
                let (lo, hi) = (Vec3A::from(lo), Vec3A::from(hi));
                let m = root.matrix3;
                let scale = m
                    .x_axis
                    .length()
                    .max(m.y_axis.length())
                    .max(m.z_axis.length());
                let centre = root.transform_point3a((lo + hi) * 0.5);
                crate::character::near_view(frusta, centre, (hi - lo).length() * 0.5 * scale)
            }
            None => crate::character::near_view(frusta, root.translation, 0.0),
        };
        let coords: &[Coords] = match animator {
            Some((mut a, st)) => {
                // A replaced or mutated Animator restarts the channel (with a
                // cross-fade) at its own time: an already-open chest starts at
                // the clip's end, a decoration's next clip carries its overshoot.
                // Only the time moved (the same clip again) is a seek, no restart: a short clip
                // otherwise piles up a cross-fade every frame or two.
                let (changed, seek) = match &st.synced {
                    Some((anim, t, s, l)) => {
                        let same = Arc::ptr_eq(anim, &a.anim) && *s == a.speed && *l == a.looped;
                        (!same || *t != a.time, same)
                    }
                    None => (true, false),
                };
                // A settled clip keeps its pose (`abs`, or `pose` while stale).
                if !changed && st.channel.settled() && (!st.abs.is_empty() || st.stale) {
                    if !st.stale && !root_tf.is_changed() && !skel.is_changed() {
                        continue;
                    }
                    let st = st.into_inner();
                    if !near() {
                        st.stale = true;
                        continue;
                    }
                    if st.stale {
                        pose_object(&skel.msh, &st.pose, &mut st.abs);
                        st.stale = false;
                    }
                    &st.abs
                } else {
                    let st = st.into_inner();
                    if changed {
                        if !seek {
                            st.channel.play(a.anim.clone(), a.looped, a.speed, 200.0);
                        }
                        st.channel.seek(a.time * 1000.0);
                        st.synced = Some((a.anim.clone(), a.time, a.speed, a.looped));
                    }
                    let ev = st.channel.update(dt_ms, &skel.msh, &mut st.pose);
                    // Objects pin root motion themselves (`pose_object`): put the
                    // sampled root translation back for the pose.
                    if let Some(ri) = skel.msh.root_bone {
                        st.pose.set_root_translation(ri, ev.root_translation);
                    }
                    st.events.extend(ev.entities);
                    // The same callback path as characters (the engine walks the
                    // callback array for every actor, doors and chests included).
                    for name in ev.callbacks {
                        out.write(AnimCallback {
                            entity: e,
                            anim: AnimType::None,
                            name,
                            cast: false,
                        });
                    }
                    a.time = st
                        .channel
                        .current
                        .as_ref()
                        .map_or(0.0, |c| c.time_ms * 0.001);
                    if let Some(s) = &mut st.synced {
                        s.1 = a.time;
                    }
                    if !near() {
                        st.stale = true;
                        continue;
                    }
                    pose_object(&skel.msh, &st.pose, &mut st.abs);
                    st.stale = false;
                    &st.abs
                }
            }
            None if !root_tf.is_changed() && !skel.is_changed() => continue,
            None => {
                buf.clear();
                buf.extend(skel.msh.bones.iter().map(|b| b.abs));
                &buf
            }
        };
        for (j, c) in skel.joints.iter().zip(coords) {
            if let Ok(mut g) = joints.get_mut(*j) {
                *g = GlobalTransform::from(root * coords_to_affine(c));
            }
        }
    }
}

/// An object's mesh-space pose from its channel's blended keys (root translation included) into
/// `abs` ([`animate_skeletons`]). Root motion is applied by the movement code, not the pose: the
/// root-motion bone's translation is pinned to the bind pose.
fn pose_object(msh: &Arc<Msh>, pose: &Pose, abs: &mut Vec<Coords>) {
    anim::pose_abs_into(msh, pose, abs);
    if let Some(ri) = msh.root_bone
        && let Some(&posed) = abs.get(ri)
    {
        let bind = msh.bones[ri].abs.origin;
        let delta = [
            posed.origin[0] - bind[0],
            posed.origin[1] - bind[1],
            posed.origin[2] - bind[2],
        ];
        for c in abs.iter_mut() {
            for k in [0, 2] {
                c.origin[k] -= delta[k];
            }
        }
    }
}
