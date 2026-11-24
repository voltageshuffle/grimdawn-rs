//! Character assembly and animation (players and creatures), following the
//! engine:
//! - body: the record's `mesh` (for players the head, which also carries the
//!   skeleton and attach points) with its `CreateEntity` effects on its bones
//!   (Engine.dll `Actor::CreateAttachmentsFromMesh`,
//!   every actor), `scale`, texture overrides
//!   (`baseTexture`/`bumpTexture` for all materials, `baseTextures[i]` per
//!   material index; players: `playerTextures[0]`; monsters: one of
//!   `monsterMesh`/`monsterBaseTexture`/`monsterBumpTexture` picked by
//!   object id).
//! - armour: every equipped (or default) piece is its own skinned mesh posed
//!   with the character's skeletal pose through cached bone maps, its root
//!   bone on the wearer's (`anim::pose_abs_attached_into`); the gendered mesh
//!   (`armorMaleMesh`/`armorFemaleMesh` + textures) replaces the item's own
//!   `mesh` when set.
//! - weapons: static meshes at the body's "R Hand"/"L Hand" attach points.
//!   (Game.dll `EquipManager::AttachItemAction` -> `Item::AttachItem`
//!   -> `Actor::Attach` with the identity offset; Engine.dll
//!   `Entity::GetCoords` = `Actor::GetAttachedCoordsInRegion` times the
//!   offset, which is the actor coords * scale * the point in
//!   mesh space, `GraphicsMesh::GetAttachedCoords` = parent
//!   bone's posed abs * the point): no grip or per-weapon-type correction, and
//!   the hero's mirrored "R Hand" basis (x, -y, z) is kept; it cancels the
//!   mirrored bind of `Weapon_R_Parent_Fix`, so the right weapon frame is a
//!   proper rotation, the left one's mirror image (`AttachPoint::coords_in`, tests).
//!   The parent bone's posed abs is `GraphicsMesh::GetAbsCoords`
//!   (`local * key` per bone up to the root, the skinning chain of
//!   `GraphicsMesh::GetSkinningCoords`: `parent * local * key`; the key `SkeletalPose::CacheCoords`
//!   `T * R * S * SR`, one `AnimChannel` per mesh instance: no additive layer,
//!   IK or per-bone override; `Actor::GetAttachedCoordsInRegion` scales it by
//!   the mesh-instance scale, `mmPlayerScale` on the menu). The weapon's own
//!   scale is the item's base scale times the wearer's `weaponScale`
//!   (Game.dll `Weapon::AttachItem`,
//!   `Character::GetWeaponScale`, `Character.weaponScale`), the attach offset the identity
//!   (`Weapon::GetAttachOffset`, static `Coords` in .data).
//! - animation: `gd_sim::anim` sets by hand state, one channel with 200 ms
//!   cross-fades, root motion, callback points.
//! - hidden (`gd_game_core::visuals::Hidden`, a blink's warm-up): the root and its
//!   weapons are not drawn, with everything attached to them (armour, the
//!   effects on the body and on the weapons) and their shadows ([`hide_actors`]).
//! - shapeshift form (`gd_game_core::character::Character::form_mesh`, worn by
//!   `gd_game::shapeshift::wear_form`): [`wear_forms`] draws the form mesh
//!   (its own materials, under the root so the character's scale applies)
//!   instead of the body; armour and weapons are not drawn while it is on
//!   (`Character::SetHideEquipment`, `Character::AddAttachedEntitiesToScene`:
//!   every attached `ItemEquipment` is skipped), their effects
//!   with them. The entity stays the same (hover, targeting, effect anchors).
//!   The form mesh's own `CreateEntity` effects are made on its bones
//!   (`Character::SetOverrideMesh`: the old mesh's attachments removed, the
//!   new one's made). The paper doll wears the form too, and keeps its armour,
//!   default pieces and weapons drawn (`doll::doll_forms`). PORT: the form
//!   mesh loads in the background, so the old look stays a few frames until
//!   it is in (the engine's `SetOverrideMesh` swaps at once).
//! - equipment change: the player's body is rebuilt once every new part is
//!   loaded ([`player_body_ready`]); the old body stays until then.
use crate::model::{ExternalPose, ModelAsset, ModelCache, Skeleton, coords_to_affine};
use crate::render::material::{MaterialAssets, PartMaterial};
use crate::textures::Textures;
use bevy::camera::primitives::{Frustum, Sphere};
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::math::Vec3A;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::collections::HashMap;
use gd_formats::msh::{AttachPoint, Coords};
use gd_game_core::data::GameData;
use gd_sim::anim::{self, AnimType};
use gd_sim::equip::WeaponType;
use std::sync::Arc;

pub use gd_game_core::character::{AnimCallback, CharState, Character};

pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        use bevy::transform::TransformSystems;
        app.register_required_components::<Character, Visibility>()
            .add_systems(
                Update,
                (
                    dress_bodies,
                    hide_actors,
                    wear_forms
                        .in_set(CharacterRenderStage::WearForms)
                        .after(gd_game_core::shapeshift::ShapeshiftSet::Wear),
                ),
            )
            // The animation advance dispatches in Update (`CharacterStage::Animate`); the pose
            // reads the characters' propagated `GlobalTransform` and writes their parts' and
            // weapons', so it runs after this frame's propagation (what draws on the bodies
            // orders after `CharacterRenderStage::Pose`).
            .configure_sets(
                PostUpdate,
                CharacterRenderStage::Pose.after(TransformSystems::Propagate),
            )
            .add_systems(PostUpdate, pose_bodies.in_set(CharacterRenderStage::Pose));
    }
}

/// `gd_game_core::visuals::Hidden` on a character: its root and weapon roots are
/// hidden while it is on (kept every frame: a re-dressed body's new weapons,
/// a `ShowRightHand` callback), shown again when it goes, except a weapon
/// whose hand the animation hid ([`AttachedWeapon::hand_hidden`]): the actor's
/// visibility only decides whether its attached entities are drawn, each
/// keeps its own (`Actor::AddToScene`, `gd_game_core::visuals::Hidden`).
fn hide_actors(
    hidden: Query<(Entity, &Body), With<gd_game_core::visuals::Hidden>>,
    mut shown: RemovedComponents<gd_game_core::visuals::Hidden>,
    bodies: Query<&Body>,
    mut vis: Query<&mut Visibility>,
) {
    let set = |e: Entity, v: Visibility, vis: &mut Query<&mut Visibility>| {
        if let Ok(mut cur) = vis.get_mut(e) {
            cur.set_if_neq(v);
        }
    };
    for (e, body) in &hidden {
        set(e, Visibility::Hidden, &mut vis);
        for w in &body.weapons {
            set(w.root, Visibility::Hidden, &mut vis);
        }
    }
    for e in shown.read() {
        if hidden.contains(e) {
            continue;
        }
        set(e, Visibility::Inherited, &mut vis);
        // A shapeshift form keeps its weapons hidden ([`wear_forms`]).
        for w in bodies
            .get(e)
            .ok()
            .filter(|b| b.form.is_none())
            .map(|b| b.weapons.as_slice())
            .unwrap_or_default()
        {
            if !w.hand_hidden {
                set(w.root, Visibility::Inherited, &mut vis);
            }
        }
    }
}

/// On a weapon's root: the character holding it. Weapons are top-level
/// entities placed at the hand each frame; they are despawned with it (`linked_spawn`).
#[derive(Component)]
#[relationship(relationship_target = HeldWeapons)]
pub struct WeaponOf(pub Entity);

/// A character's weapon roots ([`WeaponOf`]).
#[derive(Component)]
#[relationship_target(relationship = WeaponOf, linked_spawn)]
pub struct HeldWeapons(Vec<Entity>);

/// The windows with a doll.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DollView {
    Character,
    Illusionist,
    /// The main menu's selected character: not a view but a body standing in
    /// the menu's 3D scene (`render::menu`: the record's place, scale and
    /// rotation; the world's layers, lights and shadows).
    Menu,
}

impl DollView {
    /// The doll's own render layer, if it is drawn by its own camera.
    pub fn own_layer(self) -> Option<RenderLayers> {
        (self != DollView::Menu).then(|| RenderLayers::layer(self.layer()))
    }

    /// The doll's render layer (one per view: both windows can be open).
    pub fn layer(self) -> usize {
        match self {
            DollView::Character => 21,
            DollView::Illusionist => 22,
            DollView::Menu => 23,
        }
    }

    /// Where the doll stands, far from every level (like `GD_CHARS`).
    pub fn origin(self) -> Vec3 {
        match self {
            DollView::Character => Vec3::new(-20000.0, 3000.0, -20000.0),
            DollView::Illusionist => Vec3::new(-20200.0, 3000.0, -20000.0),
            DollView::Menu => Vec3::new(-20400.0, 3000.0, -20000.0),
        }
    }
}

/// On a doll's root.
#[derive(Component)]
pub struct DollBody(pub DollView);

/// A character's visual body: its mesh parts (posed from
/// [`Character::pose`] by [`pose_bodies`]) and hand weapons.
#[derive(Component)]
pub struct Body {
    pub parts: Vec<Entity>,
    pub weapons: Vec<AttachedWeapon>,
    /// The armour and default pieces among `parts`, by slot.
    pub armour: Vec<(Entity, Slot)>,
    /// A worn shapeshift form ([`wear_forms`]): `parts` starts with the form
    /// mesh; a character has nothing else, a doll keeps its armour.
    pub form: Option<FormPart>,
}

/// The form mesh drawn in place of a body ([`wear_forms`]).
pub struct FormPart {
    pub mesh: String,
    /// The form mesh's part, in `Body::parts`.
    pub part: Entity,
    /// The body's own parts, hidden (not posed) while the form is on.
    pub hidden: Vec<Entity>,
    /// A doll's armour pieces whose slot name is an attach point of the form
    /// mesh, with that point (`doll::doll_forms`).
    pub attached: Vec<(Entity, AttachPoint)>,
}

/// The attach point a slot's piece goes on, `Paperdoll::GetAttachPointName`
/// (Game.dll). Stock player meshes have only `Head`
/// and `R Hand` / `L Hand` of these (`Head` at the origin), so on the player's
/// own mesh an armour piece's frame is the actor's.
fn slot_point(slot: Slot) -> &'static str {
    match slot {
        Slot::RightHand => "R Hand",
        Slot::LeftHand => "L Hand",
        Slot::Head => "Head",
        Slot::Chest => "Chest",
        Slot::Shoulders => "Shoulders",
        Slot::Hands => "Hands",
        Slot::Legs => "Legs",
        Slot::Feet => "Feet",
    }
}

/// Draws the form mesh of a shapeshifted character (`Character::form_mesh`)
/// in place of its body and hides its armour and weapons; puts the body back
/// when the form goes (module doc). A doll hides only its own mesh and moves
/// its weapons to the current mesh's hands (`doll::doll_forms`).
pub(crate) fn wear_forms(
    mut commands: Commands,
    mut a: CharAssets,
    mut chars: Query<(Entity, &Character, &mut Body, Has<DollBody>)>,
    skels: Query<&Skeleton>,
    mut vis: Query<&mut Visibility>,
) {
    for (root, c, mut body, doll) in &mut chars {
        let wanted = c.form_mesh();
        // Shared borrow first: only DerefMut `Body` when the form actually
        // changes, so `tag_meshes`' change filter is not defeated every frame.
        if body.form.as_ref().map(|f| f.mesh.as_str()) == wanted {
            continue;
        }
        // The form mesh is built in the background ([`CharAssets::model_ready`]): the character keeps what it wears
        // until it is in, a few frames, instead of parsing it and decoding its textures on the game thread.
        let form = match wanted {
            Some(m) => match a.model_ready(m) {
                crate::model::Ready::Loading => continue,
                crate::model::Ready::Done(model) => Some((m, model)),
                crate::model::Ready::Failed => None,
            },
            None => None,
        };
        let body = &mut *body;
        if let Some(f) = body.form.take() {
            if let Ok(s) = skels.get(f.part) {
                for j in &s.joints {
                    commands.entity(*j).try_despawn();
                }
            }
            commands.entity(f.part).try_despawn();
            body.parts.retain(|p| *p != f.part);
            let kept = std::mem::replace(&mut body.parts, f.hidden);
            body.parts.extend(kept);
        }
        // `Entity::GetCoords` asks the parent for the attach point by name
        // each frame; a name the mesh lacks is the identity
        // (`GraphicsMesh::GetAttachedCoords`). The port resolves
        // the point and its parent bone here, when the form changes.
        if doll {
            for w in &mut body.weapons {
                let name = slot_point(if w.right {
                    Slot::RightHand
                } else {
                    Slot::LeftHand
                });
                w.point = c.body.attach_point(name).cloned().unwrap_or(AttachPoint {
                    name: name.to_owned(),
                    parent: None,
                    parent_bone: None,
                    coords: Coords::IDENTITY,
                });
            }
        }
        let show = if form.is_some() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        let own = if doll { 1 } else { body.parts.len() };
        let held = if doll { Visibility::Inherited } else { show };
        for (e, v) in body
            .parts
            .iter()
            .take(own)
            .map(|e| (e, show))
            .chain(body.weapons.iter().map(|w| {
                let v = if w.hand_hidden {
                    Visibility::Hidden
                } else {
                    held
                };
                (&w.root, v)
            }))
        {
            if let Ok(mut cur) = vis.get_mut(*e) {
                cur.set_if_neq(v);
            }
        }
        let Some((mesh, model)) = form else { continue };
        let (e, _) = a.spawn_textured(&mut commands, root, &model, &TexOverride::default());
        commands.entity(e).insert(ExternalPose);
        // The form mesh's own `CreateEntity` effects (`SetOverrideMesh` makes
        // the new mesh's attachments), on its bones; they go with the part.
        crate::fx::items::attach_armour(&mut commands, &model, &c.body, root, e);
        let attached = if doll {
            body.armour
                .iter()
                .filter_map(|(p, s)| Some((*p, model.msh.attach_point(slot_point(*s))?.clone())))
                .collect()
        } else {
            Vec::new()
        };
        let hidden: Vec<Entity> = body.parts.drain(..own).collect();
        body.parts.insert(0, e);
        info!("{}: shapeshift mesh {mesh}", c.record);
        body.form = Some(FormPart {
            mesh: mesh.to_owned(),
            part: e,
            hidden,
            attached,
        });
    }
}

pub struct AttachedWeapon {
    pub root: Entity,
    pub parts: Vec<Entity>,
    /// A point of the wearer's body mesh (its parent bone resolved at load).
    pub point: AttachPoint,
    pub scale: f32,
    pub right: bool,
    /// The item record drawn (its illusion's when it has one).
    pub item: String,
    /// The weapon mesh (attach points: the trail's `Anchor1` / `Anchor2`).
    pub msh: Arc<gd_formats::msh::Mesh>,
    /// The animation hid the hand (Game.dll `Character::
    /// AnimationCallback`: `HideRightHand` / `HideLeftHand` set the weapon
    /// item's own visibility to 0 and the character's `Character.rightHandHidden` / `leftHandHidden`, the
    /// `Show…` callbacks undo both): the weapon stays hidden whatever the
    /// actor's visibility does until the animation shows it.
    pub hand_hidden: bool,
}

/// On an entity a dressing system gave its record model once it was ready ([`CharAssets::record_ready`]): those
/// systems take the entities without it, so one still loading is dressed on a later frame, and once.
#[derive(Component)]
pub struct RecordDressed;

/// Everything needed to build models.
#[derive(SystemParam)]
pub struct CharAssets<'w> {
    pub data: Res<'w, GameData>,
    pub cache: ResMut<'w, ModelCache>,
    pub textures: ResMut<'w, Textures>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    /// Bevy's own material (the trails, beams and lightning quads).
    pub materials: ResMut<'w, Assets<StandardMaterial>>,
    pub mats: MaterialAssets<'w>,
    pub bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
    pub images: ResMut<'w, Assets<Image>>,
    pub bodies: ResMut<'w, gd_game_core::creature::BodyCache>,
}

impl CharAssets<'_> {
    pub fn model(&mut self, path: &str) -> Option<Arc<ModelAsset>> {
        self.cache.model(
            &self.data,
            &mut self.bodies,
            path,
            &mut self.meshes,
            &mut self.mats,
            &mut self.bindposes,
            &mut self.textures,
            &mut self.images,
        )
    }

    /// [`ModelCache::model_ready`]: the model when it is built, else it loads in the background and the caller
    /// spawns it on a later frame.
    pub fn model_ready(&mut self, path: &str) -> crate::model::Ready<Arc<ModelAsset>> {
        self.cache.model_ready(&self.data, &self.bodies, path)
    }

    /// Whether every model of `paths` is built (or failed, which spawns without it); the ones that are not start
    /// loading in the background.
    pub fn models_ready<'p>(&mut self, paths: impl IntoIterator<Item = &'p str>) -> bool {
        let mut ready = true;
        for p in paths {
            ready &= !matches!(self.model_ready(p), crate::model::Ready::Loading);
        }
        ready
    }

    /// Whether [`Self::spawn_record_model`] of `rec` builds and decodes nothing on the game thread: its `mesh` model,
    /// its texture overrides and its shader override are in (or failed); the ones that are not start loading in the
    /// background. A dressing system that waits for this marks what it dressed [`RecordDressed`].
    pub fn record_ready(&mut self, rec: &Record) -> bool {
        let Some(mesh) = rec.str("mesh") else {
            return true;
        };
        let mut ready = self.models_ready([mesh]);
        let tex = TexOverride::from_record(rec);
        let bumps = texture_list(rec, "bumpTextures");
        let data = self.data.clone();
        for p in tex
            .paths()
            .chain(bumps.iter().map(String::as_str).filter(|p| !p.is_empty()))
        {
            ready &= self.textures.ready(&data, p, false, &mut self.images);
        }
        if let Some(sh) = tex.shader.as_deref().filter(|s| !s.trim().is_empty()) {
            let skinned =
                matches!(self.model_ready(mesh), crate::model::Ready::Done(m) if m.mesh.skinned);
            ready &= self.mats.cache.shader_ready(&data, sh, skinned);
        }
        ready
    }

    /// Spawns an object record's `mesh` under `root` the way `Actor::Load`
    /// (Engine.dll) sets it up: the texture overrides
    /// ([`TexOverride`]: `baseTexture` / `bumpTexture` for every material,
    /// else `baseTextures[i]` / `bumpTextures[i]`; e.g. a chest's colour
    /// variant, or `system/textures/invisible.tex`, fully transparent, for a
    /// mesh that only carries attach points), `castsShadows` (default
    /// true) and the mesh's `CreateEntity` effects
    /// ([`crate::fx::spawn_mesh_attachments`]). Returns the part entities (None without a model).
    pub fn spawn_record_model(
        &mut self,
        commands: &mut Commands,
        root: Entity,
        rec: &Record,
    ) -> Option<Vec<Entity>> {
        let mesh = rec.str("mesh")?;
        let model = self.model(mesh)?;
        let tex = TexOverride::from_record(rec);
        let bumps = texture_list(rec, "bumpTextures");
        let mut mats: HashMap<u32, PartMaterial> = HashMap::default();
        for (mi, (_, def)) in model.part_materials.iter().zip(&model.parts) {
            let i = *mi as usize;
            let base = tex
                .base
                .clone()
                .filter(|s| !s.is_empty())
                .or_else(|| tex.base_per_material.get(i).cloned());
            let bump = tex
                .bump
                .clone()
                .filter(|s| !s.is_empty())
                .or_else(|| bumps.get(i).map(|s| s.to_string()));
            let Some(m) = model.msh_material(*mi) else {
                continue;
            };
            let h = self.mats.reshade(
                &self.data,
                def,
                m,
                model.mesh,
                tex.reshade(i),
                &mut self.textures,
                &mut self.images,
            );
            let m = self.mats.retexture(
                &self.data,
                &h,
                base.as_deref(),
                bump.as_deref(),
                &mut self.textures,
                &mut self.images,
            );
            mats.insert(*mi, m);
        }
        let parts = crate::model::spawn_model_with(commands, root, &model, |mi, d| {
            mats.get(&mi).cloned().unwrap_or_else(|| d.clone())
        });
        // The mesh's own `CreateEntity` effects (`Actor::CreateAttachmentsFromMesh`).
        crate::fx::spawn_mesh_attachments(commands, &self.data, &model, root);
        if rec.i32("castsShadows") == Some(0) {
            for p in &parts {
                commands.entity(*p).try_insert(bevy::light::NotShadowCaster);
            }
        }
        Some(parts)
    }

    /// Spawns `model` under `parent` with texture overrides (engine priority:
    /// global override > per-material-index override > material's own).
    fn spawn_textured(
        &mut self,
        commands: &mut Commands,
        parent: Entity,
        model: &Arc<ModelAsset>,
        tex: &TexOverride,
    ) -> (Entity, Vec<Entity>) {
        let e = commands
            .spawn((Transform::IDENTITY, Visibility::default()))
            .id();
        crate::model::try_add_child(commands, parent, e);
        // The record's own textures and shader (`MaterialAssets`), so the override renders (and outlines)
        // like the mesh's own.
        let bump = tex.bump.clone().filter(|s| !s.is_empty());
        let mut made: HashMap<u32, PartMaterial> = HashMap::default();
        let (data, textures, images, mats) = (
            &self.data,
            &mut self.textures,
            &mut self.images,
            &mut self.mats,
        );
        let mut choose = |mi: u32, def: &PartMaterial| -> PartMaterial {
            let ov = tex.reshade(mi as usize);
            let reshaded = ov.shader.is_some() || ov.glow.is_some_and(|g| !g.trim().is_empty());
            let b = tex.base.clone().filter(|s| !s.is_empty()).or_else(|| {
                tex.base_per_material
                    .get(mi as usize)
                    .cloned()
                    .filter(|s| !s.is_empty())
            });
            let Some(m) = model.msh_material(mi) else {
                return def.clone();
            };
            // The bump only replaces a material's own normal map.
            let n = bump.clone().filter(|_| m.texture("bumpTexture").is_some());
            if b.is_none() && n.is_none() && !reshaded {
                return def.clone();
            }
            made.entry(mi)
                .or_insert_with(|| {
                    let def = mats.reshade(data, def, m, model.mesh, ov, textures, images);
                    mats.retexture(data, &def, b.as_deref(), n.as_deref(), textures, images)
                })
                .clone()
        };
        let parts = crate::model::spawn_model_with(commands, e, model, |mi, def| choose(mi, def));
        (e, parts)
    }
}

/// A per-material texture list (`baseTextures` / `bumpTextures`): every
/// entry of the array field with empty ones kept (they keep the material's
/// own texture), so entry i is msh material i.
pub fn texture_list(r: &Record, key: &str) -> Vec<String> {
    r.get(key)
        .unwrap_or(&[])
        .iter()
        .map(|v| match v {
            gd_formats::arz::RawValue::Str(s) => s.trim().to_owned(),
            _ => String::new(),
        })
        .collect()
}

#[derive(Default, Debug, Clone)]
pub struct TexOverride {
    pub base: Option<String>,
    pub bump: Option<String>,
    pub base_per_material: Vec<String>,
    /// `shader`, `glowTexture`, `glowTextures`
    /// ([`crate::render::material::Reshade`]).
    pub shader: Option<String>,
    pub glow: Option<String>,
    pub glow_per_material: Vec<String>,
}

impl TexOverride {
    fn from_record(r: &Record) -> Self {
        Self {
            base: r.str("baseTexture").map(str::to_owned),
            bump: r.str("bumpTexture").map(str::to_owned),
            base_per_material: texture_list(r, "baseTextures"),
            shader: r.str("shader").map(str::to_owned),
            glow: r.str("glowTexture").map(str::to_owned),
            glow_per_material: texture_list(r, "glowTextures"),
        }
    }

    /// Every texture the override names (what dressing with it decodes).
    fn paths(&self) -> impl Iterator<Item = &str> {
        [&self.base, &self.bump, &self.glow]
            .into_iter()
            .flatten()
            .chain(&self.base_per_material)
            .chain(&self.glow_per_material)
            .map(String::as_str)
            .filter(|p| !p.is_empty())
    }

    /// The shader / glow overrides of msh material `i` (the global glow first).
    fn reshade(&self, i: usize) -> crate::render::material::Reshade<'_> {
        crate::render::material::Reshade {
            shader: self.shader.as_deref(),
            glow: self
                .glow
                .as_deref()
                .filter(|s| !s.is_empty())
                .or(self.glow_per_material.get(i).map(String::as_str)),
        }
    }
}

pub use gd_game_core::creature::{Slot, slot_of};

/// Item mesh + textures for a gender (`Armor::SetArmorGender` →
/// `ChangeArmorMesh`): the gendered mesh with its textures, each texture
/// falling back to the record's; no gendered mesh → the item's own `mesh`
/// and record textures.
fn item_visual(r: &Record, female: bool) -> Option<(String, TexOverride)> {
    let g = if female { "Female" } else { "Male" };
    let rec_tex = TexOverride::from_record(r);
    if let Some(m) = r.str(&format!("armor{g}Mesh")) {
        let base = r
            .str(&format!("armor{g}BaseTexture"))
            .map(str::to_owned)
            .or(rec_tex.base.clone());
        let bump = r
            .str(&format!("armor{g}BumpTexture"))
            .map(str::to_owned)
            .or(rec_tex.bump.clone());
        return Some((
            m.to_owned(),
            TexOverride {
                base,
                bump,
                ..rec_tex
            },
        ));
    }
    Some((r.str("mesh")?.to_owned(), rec_tex))
}

pub use gd_game_core::player::player_record;

/// Spawns a player character (`female` selects femalepc01) wearing
/// `equipment` (item record paths) over the record's default pieces.
pub fn spawn_player(
    commands: &mut Commands,
    a: &mut CharAssets,
    transform: Transform,
    female: bool,
    equipment: &[String],
) -> Option<Entity> {
    player_body(commands, a, None, transform, female, equipment)
}

/// Gives the existing player entity `root` a new body for `equipment` (after
/// [`despawn_body`]); its gameplay components stay. A caller in game waits for [`player_body_ready`] first, so the
/// rebuild builds and decodes nothing on the game thread.
pub fn rebuild_player_body(
    commands: &mut Commands,
    a: &mut CharAssets,
    root: Entity,
    transform: Transform,
    female: bool,
    equipment: &[String],
) -> Option<Entity> {
    player_body(commands, a, Some(root), transform, female, equipment)
}

/// Despawns a character's body (mesh parts, their joints, weapons) but not
/// the character entity itself.
pub fn despawn_body(
    commands: &mut Commands,
    root: Entity,
    ch: &Body,
    skels: &Query<&crate::model::Skeleton>,
) {
    let hidden = ch
        .form
        .as_ref()
        .map(|f| f.hidden.as_slice())
        .unwrap_or_default();
    // Joints are not children of the parts.
    for e in std::iter::once(&root).chain(&ch.parts).chain(hidden).chain(
        ch.weapons
            .iter()
            .flat_map(|w| std::iter::once(&w.root).chain(&w.parts)),
    ) {
        if let Ok(s) = skels.get(*e) {
            for j in &s.joints {
                commands.entity(*j).try_despawn();
            }
        }
    }
    commands.entity(root).try_remove::<crate::model::Skeleton>();
    // Weapons are not children either (their transforms are written directly).
    for e in ch.parts.iter().chain(hidden).chain(
        ch.weapons
            .iter()
            .flat_map(|w| std::iter::once(&w.root).chain(&w.parts)),
    ) {
        commands.entity(*e).try_despawn();
    }
}

/// Whether [`rebuild_player_body`] of `equipment` builds and decodes nothing on the game thread, as [`dress_bodies`]
/// waits for a creature: the body, armour and weapon models, the override textures and the record shaders are in (or
/// failed); the ones that are not start loading in the background. `held` takes what is already in (the last call's
/// holds go only once this call's are taken): the caller keeps it while it waits, so the warm sets cannot drop an
/// early part before the last one arrives ([`Waiting`]).
pub fn player_body_ready(
    a: &mut CharAssets,
    female: bool,
    equipment: &[String],
    held: &mut Waiting,
) -> bool {
    let db = a.data.db.clone();
    let Some(look) = player_look(&db, female, equipment) else {
        return true;
    };
    let items: Vec<(String, TexOverride)> = look
        .by_slot
        .values()
        .filter_map(|item| item_visual(db.get(item)?, female))
        .collect();
    let data = a.data.clone();
    let mut taken = Waiting::default();
    let mut ready = true;
    for (mesh, tex) in std::iter::once((look.mesh.as_str(), &look.tex))
        .chain(items.iter().map(|(m, t)| (m.as_str(), t)))
    {
        match a.model_ready(mesh) {
            crate::model::Ready::Done(m) => {
                // A record shader override is analysed in the background too (a `reshade` would parse it on the
                // spot).
                if let Some(sh) = tex.shader.as_deref().filter(|s| !s.trim().is_empty()) {
                    ready &= a.mats.cache.shader_ready(&data, sh, m.mesh.skinned);
                }
                taken.models.push(m);
            }
            crate::model::Ready::Loading => ready = false,
            crate::model::Ready::Failed => {}
        }
        for p in tex.paths() {
            ready &= a
                .textures
                .ready_held(&data, p, false, &mut a.images, &mut taken.textures);
        }
    }
    *held = taken;
    ready
}

/// The player record's look wearing `equipment` ([`player_body`], [`player_body_ready`]).
struct PlayerLook<'d> {
    path: &'static str,
    rec: &'d Record,
    mesh: String,
    tex: TexOverride,
    /// The item record of each slot: the worn item, else the record's default piece.
    by_slot: HashMap<Slot, String>,
}

fn player_look<'d>(
    db: &'d gd_formats::vfs::Database,
    female: bool,
    equipment: &[String],
) -> Option<PlayerLook<'d>> {
    let path = player_record(female);
    let rec = db.get(path)?;
    let mut tex = TexOverride::from_record(rec);
    // Player::PickNewTexture(0) → SetBaseTexture(playerTextures[0]).
    if let Some(t) = rec.strs("playerTextures").first() {
        tex.base = Some((*t).to_owned());
    }
    let mesh = rec.str("mesh")?.to_owned();
    // EquipManager::Load: default pieces, hidden while a real item occupies the slot.
    let mut by_slot: HashMap<Slot, String> = HashMap::default();
    for (key, slot) in [
        ("defaultHeadPiece", Slot::Head),
        ("defaultShouldersPiece", Slot::Shoulders),
        ("defaultChestPiece", Slot::Chest),
        ("defaultLegsPiece", Slot::Legs),
        ("defaultFeetPiece", Slot::Feet),
        ("defaultHandsPiece", Slot::Hands),
    ] {
        if let Some(p) = rec.str(key) {
            by_slot.insert(slot, p.to_owned());
        }
    }
    for item in equipment {
        if let Some(r) = db.get(item)
            && let Some(s) = slot_of(&r.class)
        {
            by_slot.insert(s, item.clone());
        }
    }
    Some(PlayerLook {
        path,
        rec,
        mesh,
        tex,
        by_slot,
    })
}

fn player_body(
    commands: &mut Commands,
    a: &mut CharAssets,
    root: Option<Entity>,
    transform: Transform,
    female: bool,
    equipment: &[String],
) -> Option<Entity> {
    // The database by its own handle: the record stays borrowed while `a` spawns.
    let db = a.data.db.clone();
    let look = player_look(&db, female, equipment)?;
    spawn_character(
        commands,
        a,
        root,
        transform,
        look.path,
        look.rec,
        &look.mesh,
        look.tex,
        &look.by_slot,
        female,
        true,
    )
}

/// Dresses new creatures (`gd_game_core::creature::BodyRequest`) with their
/// body, armour and weapon meshes, once every one of them and every override texture they name is loaded: what is
/// not yet loads in the background ([`CharAssets::model_ready`], [`crate::textures::Textures::ready_held`]) and the
/// creature is dressed on a later frame (it is drawn a few frames after it appears instead of stalling the frame).
/// While it waits the creature holds what is already in ([`Waiting`]): the caches only keep their last
/// [`gd_game_core::weak_cache::RECENT`] values warm, and a crowd (the Conclave of the Three: ~40 NPCs with a dozen parts
/// each, beside level streaming) would otherwise evict one creature's early parts before its last one arrives and
/// load them again, so it never had everything at once and stayed bodiless.
fn dress_bodies(
    mut commands: Commands,
    mut a: CharAssets,
    new: Query<(Entity, &gd_game_core::creature::BodyRequest, &Character)>,
    mut waiting: Local<HashMap<Entity, Waiting>>,
    mut taken: Local<Waiting>,
) {
    waiting.retain(|e, _| new.contains(*e));
    for (e, req, ch) in &new {
        let db = a.data.db.clone();
        let Some(rec) = db.get(&req.record) else {
            commands
                .entity(e)
                .remove::<gd_game_core::creature::BodyRequest>();
            continue;
        };
        let mut tex = TexOverride::from_record(rec);
        if req.look.base_texture.is_some() {
            tex.base = req.look.base_texture.clone();
        }
        if req.look.bump_texture.is_some() {
            tex.bump = req.look.bump_texture.clone();
        }
        let items: Vec<(String, TexOverride)> = req
            .look
            .items
            .values()
            .filter_map(|item| item_visual(db.get(item)?, false))
            .collect();
        // This frame's holds go into `taken` and swap with last frame's, which are dropped only after: a part the
        // caches already let go of stays alive in between.
        let mut models = true;
        for mesh in
            std::iter::once(req.look.mesh.as_str()).chain(items.iter().map(|(m, _)| m.as_str()))
        {
            match a.model_ready(mesh) {
                crate::model::Ready::Done(m) => taken.models.push(m),
                crate::model::Ready::Loading => models = false,
                crate::model::Ready::Failed => {}
            }
        }
        let data = a.data.clone();
        let mut textures = true;
        for p in tex.paths().chain(items.iter().flat_map(|(_, t)| t.paths())) {
            textures &= a
                .textures
                .ready_held(&data, p, false, &mut a.images, &mut taken.textures);
        }
        let w = waiting.entry(e).or_default();
        std::mem::swap(&mut w.models, &mut taken.models);
        std::mem::swap(&mut w.textures, &mut taken.textures);
        taken.models.clear();
        taken.textures.clear();
        // A record shader override is analysed in the background too (a `reshade` would parse it on the spot).
        let looks = std::iter::once((req.look.mesh.as_str(), &tex))
            .chain(items.iter().map(|(m, t)| (m.as_str(), t)));
        let mut shaders = true;
        for (mesh, t) in looks {
            let Some(sh) = t.shader.as_deref().filter(|s| !s.trim().is_empty()) else {
                continue;
            };
            let skinned =
                matches!(a.model_ready(mesh), crate::model::Ready::Done(m) if m.mesh.skinned);
            shaders &= a.mats.cache.shader_ready(&data, sh, skinned);
        }
        if !(models && textures && shaders) {
            continue;
        }
        commands
            .entity(e)
            .remove::<gd_game_core::creature::BodyRequest>();
        let body = dress(
            &mut commands,
            &mut a,
            e,
            &req.record,
            rec,
            &req.look.mesh,
            tex,
            &req.look.items,
            false,
        );
        // The spawned parts hold their assets now.
        waiting.remove(&e);
        let Some(body) = body else {
            continue;
        };
        log_body(&req.record, &req.look.mesh, ch, &body);
        commands.entity(e).insert(body);
    }
}

/// What [`dress_bodies`] holds for a creature still waiting: the models and textures already in (also the player's
/// body rebuild, [`player_body_ready`]).
#[derive(Default)]
pub struct Waiting {
    models: Vec<Arc<crate::model::ModelAsset>>,
    textures: Vec<Handle<Image>>,
}

#[allow(clippy::too_many_arguments)]
fn spawn_character(
    commands: &mut Commands,
    a: &mut CharAssets,
    root: Option<Entity>,
    transform: Transform,
    path: &str,
    rec: &Record,
    mesh: &str,
    tex: TexOverride,
    items: &HashMap<Slot, String>,
    female: bool,
    is_player: bool,
) -> Option<Entity> {
    let body = a.model(mesh)?;
    let scale = gd_game_core::creature::record_scale(rec);
    // An explicit GlobalTransform: the camera follows the player before
    // transform propagation, and a default one would put a respawned body
    // (equipment change) at the world origin for a frame.
    let tf = transform.with_scale(Vec3::splat(scale));
    let root = match root {
        Some(r) => commands
            .entity(r)
            .insert((tf, GlobalTransform::from(tf), Visibility::default()))
            .id(),
        None => commands
            .spawn((
                tf,
                GlobalTransform::from(tf),
                Visibility::default(),
                Name::new(path.to_owned()),
            ))
            .id(),
    };
    // Weapons decide the hand state (EquipManager::GetHandState).
    let wtype = |s: Slot| {
        items
            .get(&s)
            .and_then(|p| a.data.db.get(p))
            .and_then(|r| WeaponType::from_class(&r.class))
    };
    let character = gd_game_core::character::character_for(
        &a.data,
        path,
        rec,
        body.msh.clone(),
        wtype(Slot::RightHand),
        wtype(Slot::LeftHand),
        is_player,
        scale,
        root.to_bits(),
    );
    let parts = dress(commands, a, root, path, rec, mesh, tex, items, female)?;
    log_body(path, mesh, &character, &parts);
    commands.entity(root).insert((character, parts));
    Some(root)
}

fn log_body(path: &str, mesh: &str, character: &Character, body: &Body) {
    // Animation table (Character::LoadAnimations).
    let set = &character.anims;
    info!(
        "{path}: mesh {mesh}, scale {}, hand {:?}, {} parts, {} weapons, idle {:?} run {:?} attack {:?}",
        character.scale,
        character.hand,
        body.parts.len(),
        body.weapons.len(),
        set.slot(AnimType::AttackIdle)
            .entries
            .iter()
            .map(|e| &e.path)
            .collect::<Vec<_>>(),
        set.slot(AnimType::Run).entries.first().map(|e| &e.path),
        set.slot(AnimType::Attack)
            .entries
            .iter()
            .map(|e| (&e.path, e.weight))
            .collect::<Vec<_>>()
    );
}

/// Spawns `root`'s body: the `mesh` with `tex`, armour pieces posed with its
/// skeleton and weapons at its hand attach points.
#[allow(clippy::too_many_arguments)]
fn dress(
    commands: &mut Commands,
    a: &mut CharAssets,
    root: Entity,
    path: &str,
    rec: &Record,
    mesh: &str,
    tex: TexOverride,
    items: &HashMap<Slot, String>,
    female: bool,
) -> Option<Body> {
    let body = a.model(mesh)?;
    let mut parts = Vec::new();
    let mut armour = Vec::new();
    let (e, _) = a.spawn_textured(commands, root, &body, &tex);
    commands.entity(e).insert(ExternalPose);
    // The body mesh's own `CreateEntity` effects (`Actor::CreateAttachmentsFromMesh`, every actor), on its bones:
    // the whole look of a wisp or an Aetherial phantom (a discarded sphere + `FX_AetherialPhantom01` at
    // `FXCentered`), the flames of a fire zombie. They go with the part (a shapeshift form hides them).
    crate::fx::items::attach_armour(commands, &body, &body.msh, root, e);
    parts.push(e);
    let weapon_scale = rec.f32("weaponScale").filter(|s| *s > 0.0).unwrap_or(1.0);
    let mut weapons = Vec::new();
    let mut slots: Vec<_> = items.iter().collect();
    slots.sort_by_key(|(s, _)| format!("{s:?}"));
    for (slot, item) in slots {
        let db = a.data.db.clone();
        let Some(r) = db.get(item) else {
            continue;
        };
        let Some((m, t)) = item_visual(r, female) else {
            continue;
        };
        let Some(model) = a.model(&m) else { continue };
        match slot {
            Slot::RightHand | Slot::LeftHand => {
                let name = if *slot == Slot::RightHand {
                    "R Hand"
                } else {
                    "L Hand"
                };
                let Some(point) = body.msh.attach_point(name).cloned() else {
                    warn!("{path}: no attach point {name}");
                    continue;
                };
                let w = commands
                    .spawn((Transform::IDENTITY, Visibility::default(), WeaponOf(root)))
                    .id();
                let (e, wparts) = a.spawn_textured(commands, w, &model, &t);
                crate::fx::items::attach_weapon(commands, &model, root, w);
                let item_scale = r.f32("scale").filter(|s| *s > 0.0).unwrap_or(1.0);
                weapons.push(AttachedWeapon {
                    root: w,
                    parts: [vec![e], wparts].concat(),
                    point,
                    scale: weapon_scale * item_scale,
                    right: *slot == Slot::RightHand,
                    item: item.clone(),
                    msh: model.msh.clone(),
                    hand_hidden: false,
                });
            }
            _ => {
                if model.inverse_bindposes.is_none() {
                    warn!("{path}: armour mesh {m} is not skinned");
                    continue;
                }
                let (e, _) = a.spawn_textured(commands, root, &model, &t);
                crate::fx::items::attach_armour(commands, &model, &model.msh, root, e);
                debug!("{path}: part {m} ({} bones)", model.msh.bones.len());
                commands.entity(e).insert(ExternalPose);
                parts.push(e);
                armour.push((e, *slot));
            }
        }
    }
    Some(Body {
        parts,
        weapons,
        armour,
        form: None,
    })
}

/// PORT: radius (m) around a character that must touch an active camera's frustum for `pose_bodies` to pose it. A character
/// further out is invisible and casts no shadow into the view (the engine's casters lie in the camera volume extruded toward
/// the sun; a 10 m tall character's shadow is under 40 m for a sun above ~14 degrees), and nothing on screen reads its joints
/// (the skinned-mesh AABB, item FX, trails and weapon dressing all belong to the same character). It resumes with the current
/// pose the frame it comes back; the animation clock (`advance_animations`, gameplay) never stops. Paper dolls and the player are
/// always posed. The engine's own rule for off-screen animation was not decompiled.
const POSE_MARGIN: f32 = 40.0;

/// Writes joints of every part and places weapons from the character's posed bones
/// (`Character::posed`), shows and hides hands on their callbacks. Runs after transform
/// propagation like `animate_skeletons`. The characters are posed in parallel: each one
/// writes only its own parts' joints and its own weapons (see the SAFETY notes).
pub(crate) fn pose_bodies(
    mut chars: Query<(
        &GlobalTransform,
        &Character,
        &mut Body,
        Has<DollBody>,
        Has<gd_game_core::player::Player>,
        Has<gd_game_core::visuals::Hidden>,
    )>,
    cameras: Query<(&Camera, &Frustum)>,
    skels: Query<&Skeleton>,
    globals: Query<&mut GlobalTransform, Without<Character>>,
    mut vis: Query<&mut Visibility>,
    mut callbacks: MessageReader<AnimCallback>,
    mut frusta: Local<Vec<Frustum>>,
) {
    for cb in callbacks.read() {
        let name = cb.name.as_str();
        if !matches!(
            name,
            "HideRightHand" | "ShowRightHand" | "HideLeftHand" | "ShowLeftHand"
        ) {
            continue;
        }
        let Ok((_, _, mut body, _, _, actor_hidden)) = chars.get_mut(cb.entity) else {
            continue;
        };
        if body.form.is_some() {
            continue;
        }
        let right = name.contains("Right");
        let show = name.starts_with("Show");
        for w in body.weapons.iter_mut().filter(|w| w.right == right) {
            w.hand_hidden = !show;
            if let Ok(mut v) = vis.get_mut(w.root) {
                v.set_if_neq(if show && !actor_hidden {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
        }
    }
    frusta.clear();
    frusta.extend(cameras.iter().filter(|(c, _)| c.is_active).map(|(_, f)| *f));
    let frusta = &*frusta;
    chars.par_iter().for_each_init(
        Vec::<Coords>::new,
        |part_abs, (root_tf, c, body, doll, player, _)| {
            let root = root_tf.affine();
            // PORT: characters well outside every camera's view keep their last pose (see POSE_MARGIN).
            if !doll && !player && !frusta.is_empty() {
                let sphere = Sphere {
                    center: root.translation + Vec3A::Y * 1.5,
                    radius: POSE_MARGIN,
                };
                if !frusta.iter().any(|f| f.intersects_sphere(&sphere, false)) {
                    return;
                }
            }
            // The root as `Coords`: `Coords::mul` (gd-formats, opt 3 in dev builds) does the per-joint
            // `root * joint`, faster than glam's generic `Affine3A` maths in an unoptimised dev build.
            let root_c = Coords {
                x: root.matrix3.x_axis.to_array(),
                y: root.matrix3.y_axis.to_array(),
                z: root.matrix3.z_axis.to_array(),
                origin: root.translation.to_array(),
            };
            // Every part's joints: the body's own from the posed bones, another (armour) posed on them.
            let body_abs = c.posed();
            for &p in &body.parts {
                let Ok(skel) = skels.get(p) else { continue };
                let abs = if Arc::ptr_eq(&skel.msh, &c.body) {
                    body_abs
                } else {
                    anim::pose_abs_attached_into(&skel.msh, &c.pose, &c.body, body_abs, part_abs);
                    &part_abs[..]
                };
                // A doll's piece on a form's attach point (`doll::doll_forms`).
                let frame = body
                    .form
                    .iter()
                    .flat_map(|f| &f.attached)
                    .find(|(e, _)| *e == p)
                    .map_or(root_c, |(_, at)| root_c.mul(&at.coords_in(body_abs)));
                for (j, m) in skel.joints.iter().zip(abs) {
                    // SAFETY: a joint entity belongs to the skeletons of one character's parts, and
                    // this task is the only one posing that character, so no other task holds it.
                    if let Ok(mut g) = unsafe { globals.get_unchecked(*j) } {
                        let w = frame.mul(m);
                        *g = GlobalTransform::from(bevy::math::Affine3A::from_cols(
                            w.x.into(),
                            w.y.into(),
                            w.z.into(),
                            w.origin.into(),
                        ));
                    }
                }
            }
            for w in &body.weapons {
                let m = root
                    * coords_to_affine(&w.point.coords_in(body_abs))
                    * bevy::math::Affine3A::from_scale(Vec3::splat(w.scale));
                for &e in std::iter::once(&w.root).chain(&w.parts) {
                    // SAFETY: a weapon's root and parts are spawned for this character alone (`WeaponOf`).
                    if let Ok(mut g) = unsafe { globals.get_unchecked(e) } {
                        *g = GlobalTransform::from(m);
                    }
                }
            }
        },
    );
}

/// The character's render steps: `Pose` poses the bodies, `WearForms` swaps bodies for shape changes; what draws on them orders after.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CharacterRenderStage {
    Pose,
    WearForms,
}
