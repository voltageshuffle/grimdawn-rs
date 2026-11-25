//! Character light rigs (`GAME::LightRig`) and the `lightingQuality` option's character side.
//!
//! From Engine.dll / Game.dll:
//! - Load (`LightRig::LightRig(string)`): up to 3 lights, `radius1..3` (a light is kept when its
//!   radius > 0.0001), `position1..3.x/y/z`, `color1..3.r/g/b`; `cameraRelative` (default 0), `heightRelative`
//!   (default 1), `useSpecular` (default 0). Every character reads its record's `lightRig` (`Character::Load`,
//!   Game.dll: none when empty) and `lqLightingHighlight`.
//! - Use (`Character::AddToScene`, scenes of type 0 only; no menu-character test): with
//!   `Options::GetQuality(6)` (`lightingQuality`) >= 1 and a rig with lights, the rig gets a scene light group
//!   (`AssignLightGroup`) and the character's mesh is put in it; at `GetVisibility` 3 (shown) its attached
//!   entities' meshes too (armour, weapons, the mesh's own effects), at 1 the body only, otherwise nothing.
//! - The lights (`LightRig::AddToScene`): a point light per rig light, colour = the record colour
//!   (no intensity), radius = the record radius, the rig's light group, skipped when no channel > 0.03. Its
//!   position is the character's origin plus the offset turned by the camera's axes (`cameraRelative`; the
//!   scene camera's `LookAt` frame, `Math::LookAtMatrix`: z = toward the target, x = up × z, y = z × x) or by the
//!   character's own axes; without `heightRelative` the offset's y is added untouched. Specular (the light's
//!   `GraphicsLight.specular`, `pointSpecular` instead of `point`) = `useSpecular` when `lightingQuality` >= 2 and
//!   `Engine::AllowLightrigSpec` (`Engine.allowLightrigSpec`, 1 from the Engine ctor; only a console command clears
//!   it), else off.
//! - Drawing (`GraphicsDeferredRenderer::RenderDeferredPointLight`): a grouped light is the
//!   `pointGrouped` / `pointSpecularGrouped` technique, the same `pointPS` / `pointSpecularPS` maths as a level
//!   light under a stencil test against the group, which only the group's meshes write: a rig lights its own
//!   character's pixels only. Colour × the HDR factor with `hdrLighting` (`render/env.rs`).
//! - The ambient highlight (`Character::GetAmbientHighlight`, the mesh's `GraphicsMeshInstance.ambientHighlight` that
//!   `GraphicsMeshInstance::SetShaderParameters` sends as `diffuseColor.w`, the ambient add): 0 for a character
//!   with a rig at `lightingQuality` >= 1, else its `lqLightingHighlight` (0.3 on most monsters, 0.5 on the
//!   player). Its underground clause never applies (`Region::IsUnderground` = a grid level; none in stock).
//! - The hover highlight (`Actor::AddToScene` Engine.dll): the mesh's `ambientHighlight` is
//!   `GetAmbientHighlight()` + 0.25 while the actor's highlight flag (`Actor.highlight`) is on. `Actor::SetHighlight`
//!   sets it and passes it on to the attached actors (weapons, armour); Grim Dawn.exe sets it with the
//!   hover outline (`InGameUI::SetHoverTarget` → `InGameUI::OutlineHoverTarget`, which ends in the virtual
//!   `SetHighlight(true)` even with `targetOutline` off; `InGameUI::ClearHoverTarget` / `InGameUI::SetHoverTarget`
//!   clear it). A hovered NPC that is not `IsOfInterest` skips that call (`InGameUI::HandleMouseEvent`): named,
//!   neither outlined nor highlighted.
//!   So on the hover target that has an outline
//!   colour (`gd_render::hover::HoverGlow`, grimdawn `hover.rs`). Shaded as deferred.ssh `ambientPS` /
//!   `directionalAmbientPS` do (the `deferredPS` writes `diffuseColor.w` to the material target's z): albedo ×
//!   (hemisphere ambient + it). A non-character actor's base is 0
//!   (`Actor::GetAmbientHighlight`: its attach parent's, else 0). The ground label manager
//!   (`UIWorldDescManager`) sets it too: on every item whose label it lists (`UIWorldDescManager::ListLabel`
//!   from the frustum scan `UIWorldDescManager::ScanFrustum`, with `EnableOutline(true)`), again on the selected
//!   label's item (`UIWorldDescManager::Update`), and clears both on the last frame's list before the next scan
//!   (`UIWorldDescManager::ClearHighlights`), so on the listed items (`gd_render::hover::LabelGlow`).
//! - The rest of `lightingQuality`: 0 sets `GraphicsEngine::UseLowQualityLighting`
//!   (`GraphicsEngine.lowQualityLighting`, `UpdateFromOptions`), which draws level lights without specular
//!   (`Light::AddToScene`: `GraphicsLight.specular` = !low quality; `render/env.rs`); 3 gives the player light a
//!   shadow (`render/env.rs`).
//!
//! - Scene types (Engine.dll `GraphicsMTRenderer::CreateScene` callers): 0 = the world scene
//!   (`CreateBuildSceneJob`, so the main menu's world too: the menu hero gets `playerlightrig.dbr`) and
//!   `GetCustomScene`; 1 = shadow, 2 = reflection scenes.
//!
//! - The paper dolls (`DollView::Character` / `Illusionist`, own camera and layer) get no rig (Game.dll
//!   `Paperdoll` (`Paperdoll::vftable`): an `Actor`, so `Actor::AddToScene` / `Actor::GetAmbientHighlight`, no
//!   rig and highlight 0); the window's `UISceneView` record lights them. NPCs and monsters share
//!   `Character::AddToScene` (their own `lightRig`, e.g. `npclightrig.dbr`).
//!
//! PORT: the stencil group is the mesh's `MeshTag` (bits 0-15 the group, 16-31 the ambient highlight × 1000) and
//! each rig light is a Bevy `PointLight` (never shadowed) whose `radius` carries the group (+ 0.5 when
//! specular) and `intensity` 4π (Bevy divides by 4π: the clustered colour is the light's colour);
//! gd_standard.wesl walks the pixel's light cluster for the lights of its tag's group. `render/env.rs` leaves
//! these lights out of the level light buffer. Every Grim Dawn mesh under the character (or a weapon of it)
//! carries the tag while the character is shown; the engine's visibility 1 (body only) has no port state.
use crate::character::{Body, DollBody, WeaponOf};
use crate::hover::{HoverGlow, LabelGlow};
use bevy::light::SimulationLightSystems;
use bevy::mesh::MeshTag;
use bevy::prelude::*;
use gd_game_core::character::Character;
use std::sync::Arc;

pub struct LightRigPlugin;

impl Plugin for LightRigPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (load_rigs, place_rig_lights, tag_meshes, highlight_objects)
                .chain()
                .after(TransformSystems::Propagate)
                .after(crate::character::CharacterRenderStage::Pose)
                .before(SimulationLightSystems::AssignLightsToClusters),
        );
    }
}

/// One `RiggedLight` (36 bytes in the engine: offset, radius, colour with alpha 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiggedLight {
    pub offset: Vec3,
    pub radius: f32,
    pub color: Vec3,
}

/// A `lightRig` record.
#[derive(Clone, Debug, PartialEq)]
pub struct LightRig {
    pub lights: Vec<RiggedLight>,
    pub camera_relative: bool,
    pub height_relative: bool,
    pub use_specular: bool,
}

impl LightRig {
    /// `LightRig::LightRig(string)`: a light per `radiusN` > 0.0001.
    pub fn from_record(r: &gd_formats::arz::Record) -> Self {
        let f = |k: String| r.f32(&k).unwrap_or(0.0);
        let lights = (1..=3)
            .filter_map(|i| {
                let radius = f(format!("radius{i}"));
                (radius > 0.0001).then(|| RiggedLight {
                    offset: Vec3::new(
                        f(format!("position{i}.x")),
                        f(format!("position{i}.y")),
                        f(format!("position{i}.z")),
                    ),
                    radius,
                    color: Vec3::new(
                        f(format!("color{i}.r")),
                        f(format!("color{i}.g")),
                        f(format!("color{i}.b")),
                    ),
                })
            })
            .collect();
        let flag = |k: &str, default: bool| r.i32(k).map_or(default, |v| v != 0);
        Self {
            lights,
            camera_relative: flag("cameraRelative", false),
            height_relative: flag("heightRelative", true),
            use_specular: flag("useSpecular", false),
        }
    }

    /// The world position of `light` (`LightRig::AddToScene`): `origin` the character's, `rotation` its
    /// facing, `cam_forward` the camera's view direction.
    pub fn light_position(
        &self,
        light: &RiggedLight,
        origin: Vec3,
        rotation: Quat,
        cam_forward: Vec3,
    ) -> Vec3 {
        let o = light.offset;
        let mut v = if self.camera_relative {
            let z = cam_forward.normalize_or(Vec3::Z);
            let x = Vec3::Y.cross(z).normalize_or(Vec3::X);
            let y = z.cross(x);
            x * o.x + y * o.y + z * o.z
        } else {
            rotation * o
        };
        if !self.height_relative {
            v.y = o.y;
        }
        origin + v
    }
}

/// `lightingQuality` as `Options::GetQuality(6)` (0 low .. 3 very high).
pub fn lighting_quality(options: &gd_game_core::options::GameOptions) -> u32 {
    match options.get("lightingQuality") {
        Some(gd_formats::options::Value::Quality(q)) => super::shadow::quality_index(q),
        _ => 1,
    }
}

/// A character's rig state ([`load_rigs`]).
#[derive(Component)]
pub struct CharacterLighting {
    rig: Option<Arc<LightRig>>,
    /// `lqLightingHighlight`.
    lq_highlight: f32,
    /// The scene light group (1..=65535).
    group: u32,
    /// The tag its meshes carry ([`mesh_tag`]).
    tag: u32,
}

/// On a rig light: the character it belongs to. Despawned with it (`linked_spawn`).
#[derive(Component)]
#[relationship(relationship_target = RigLights)]
pub struct RigLight(pub Entity);

/// A character's rig lights ([`RigLight`]), in the rig's light order.
#[derive(Component)]
#[relationship_target(relationship = RigLight, linked_spawn)]
pub struct RigLights(Vec<Entity>);

/// The `MeshTag` of a character's meshes: the light group (0 = none, bits 0-15), the base ambient highlight
/// (bits 16-30, × 1000) and the actor's highlight flag (bit 31: `Actor::AddToScene`'s + 0.25, added by gd_standard.wesl in the world view only; the
/// conversation portrait clears the flag before it draws the NPC, `ui/dialog.rs`).
pub fn mesh_tag(group: u32, highlight: f32, hovered: bool) -> u32 {
    (group & 0xFFFF)
        | (((highlight * 1000.0).round().clamp(0.0, 32767.0) as u32) << 16)
        | (u32::from(hovered) << 31)
}

/// Whether a character is in the world scene: everyone but the paper dolls (the menu hero is; module docs).
fn in_world_scene(doll: Option<&DollBody>) -> bool {
    doll.is_none_or(|d| d.0.own_layer().is_none())
}

/// Reads each new world-scene character's `lightRig` / `lqLightingHighlight` (`Character::Load`).
fn load_rigs(
    mut commands: Commands,
    data: Res<gd_game_core::data::GameData>,
    chars: Query<(Entity, &Character, Option<&DollBody>), Without<CharacterLighting>>,
    mut next: Local<u32>,
) {
    for (e, c, doll) in &chars {
        if !in_world_scene(doll) {
            continue;
        }
        let rec = data.db.get(&c.record);
        let rig = rec
            .as_ref()
            .and_then(|r| r.str("lightRig"))
            .filter(|p| !p.is_empty())
            .and_then(|p| data.db.get(p))
            .map(|r| Arc::new(LightRig::from_record(r)));
        let lq_highlight = rec
            .as_ref()
            .and_then(|r| r.f32("lqLightingHighlight"))
            .unwrap_or(0.0);
        *next = *next % 0xFFFF + 1;
        commands.entity(e).try_insert(CharacterLighting {
            rig,
            lq_highlight,
            group: *next,
            tag: u32::MAX,
        });
    }
}

/// Spawns, places and removes the rig lights (`Character::AddToScene` / `LightRig::AddToScene`).
fn place_rig_lights(
    mut commands: Commands,
    options: Res<gd_game_core::options::GameOptions>,
    cams: Query<&GlobalTransform, (With<crate::camera::GameCamera>, Without<RigLight>)>,
    mut chars: Query<
        (
            Entity,
            &GlobalTransform,
            &InheritedVisibility,
            &mut CharacterLighting,
            Option<&RigLights>,
        ),
        Without<RigLight>,
    >,
    mut lights: Query<
        (
            &RigLight,
            &mut PointLight,
            &mut Transform,
            &mut GlobalTransform,
            &mut Visibility,
        ),
        Without<CharacterLighting>,
    >,
    glow: Query<Entity, With<HoverGlow>>,
) {
    let q = lighting_quality(&options);
    let hovered = glow.iter().next();
    let gain = if options.flag("hdrLighting") {
        super::env::HDR_LIGHTING_FACTOR
    } else {
        1.0
    };
    let fwd = cams
        .iter()
        .next()
        .map_or(Vec3::NEG_Z, |g| g.forward().as_vec3());
    for (owner, gt, vis, mut cl, held) in &mut chars {
        let has_rig = q >= 1 && cl.rig.as_ref().is_some_and(|r| !r.lights.is_empty());
        let hl = hovered == Some(owner);
        let tag = if has_rig {
            mesh_tag(cl.group, 0.0, hl)
        } else {
            mesh_tag(0, cl.lq_highlight, hl)
        };
        if cl.tag != tag {
            cl.tag = tag;
        }
        let rig = cl.rig.as_deref().filter(|_| has_rig);
        let n = rig.map_or(0, |r| r.lights.len());
        let held = held.map_or(&[][..], |h| h.0.as_slice());
        if held.len() != n {
            for &l in held {
                commands.entity(l).try_despawn();
            }
            for _ in 0..n {
                commands.spawn((
                    PointLight {
                        shadow_maps_enabled: false,
                        intensity: 0.0,
                        ..default()
                    },
                    Transform::IDENTITY,
                    Visibility::Hidden,
                    RigLight(owner),
                    Name::new("light rig"),
                ));
            }
            // Placed from the next frame on.
            continue;
        }
        let Some(rig) = rig else { continue };
        let (_, rot, origin) = gt.to_scale_rotation_translation();
        let spec = rig.use_specular && q >= 2;
        let shown = vis.get();
        for (rl, e) in rig.lights.iter().zip(held) {
            let Ok((_, mut pl, mut tf, mut lgt, mut lvis)) = lights.get_mut(*e) else {
                continue;
            };
            // A near-black light is not added (a channel > 0.03).
            let on = shown && rl.color.max_element() > 0.03;
            lvis.set_if_neq(if on {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
            let c = rl.color * gain;
            let want = PointLight {
                color: Color::linear_rgb(c.x, c.y, c.z),
                intensity: 4.0 * std::f32::consts::PI,
                range: rl.radius,
                radius: cl.group as f32 + if spec { 0.5 } else { 0.0 },
                shadow_maps_enabled: false,
                ..*pl
            };
            if pl.color != want.color
                || pl.range != want.range
                || pl.radius != want.radius
                || pl.intensity != want.intensity
            {
                *pl = want;
            }
            let p = rig.light_position(rl, origin, rot, fwd);
            if tf.translation != p {
                tf.translation = p;
                *lgt = GlobalTransform::from(*tf);
            }
        }
    }
}

/// Puts each character's [`mesh_tag`] on its meshes: all of them when the tag changes, a new mesh when it
/// appears under a character (or one of its weapons).
#[allow(clippy::too_many_arguments)]
fn tag_meshes(
    mut commands: Commands,
    chars: Query<(Entity, Ref<CharacterLighting>, Option<Ref<Body>>)>,
    new_meshes: Query<Entity, Added<Mesh3d>>,
    meshes: Query<Option<&MeshTag>, With<Mesh3d>>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    weapons: Query<&WeaponOf>,
) {
    let set = |e: Entity, tag: u32, commands: &mut Commands| {
        if let Ok(cur) = meshes.get(e)
            && cur.map(|t| t.value) != Some(tag)
        {
            commands.entity(e).try_insert(MeshTag::new(tag));
        }
    };
    for (root, cl, body) in &chars {
        if !(cl.is_changed() && cl.tag != u32::MAX)
            && !body.as_ref().is_some_and(|b| b.is_changed())
        {
            continue;
        }
        let tops =
            std::iter::once(root).chain(body.iter().flat_map(|b| b.weapons.iter().map(|w| w.root)));
        for top in tops {
            set(top, cl.tag, &mut commands);
            for d in children.iter_descendants(top) {
                set(d, cl.tag, &mut commands);
            }
        }
    }
    for m in &new_meshes {
        // Up to the top entity; a weapon stands for its holder.
        let mut top = m;
        while let Ok(p) = parents.get(top) {
            top = p.parent();
        }
        let owner = weapons.get(top).map_or(top, |w| w.0);
        if let Ok((_, cl, _)) = chars.get(owner) {
            set(m, cl.tag, &mut commands);
        }
    }
}

/// The hover highlight of the objects that are not characters (`Actor::GetAmbientHighlight` is 0 for them, so the
/// tag is the highlight alone): the hovered one (container, door, shrine, riftgate, breakable) and the ground items
/// with a listed label (module doc). Characters take it in their own tag ([`place_rig_lights`]). Applied every
/// frame, as an item's or object's meshes may appear after it got the highlight.
fn highlight_objects(
    mut commands: Commands,
    glow: Query<Entity, Or<(With<HoverGlow>, With<LabelGlow>)>>,
    chars: Query<(), With<CharacterLighting>>,
    meshes: Query<Option<&MeshTag>, With<Mesh3d>>,
    children: Query<&Children>,
    // Last frame's highlighted objects, then this frame's (kept to reuse their buffers).
    mut lists: Local<[Vec<Entity>; 2]>,
) {
    let [last, now] = &mut *lists;
    now.clear();
    now.extend(glow.iter().filter(|e| !chars.contains(*e)));
    now.sort_unstable();
    let on = mesh_tag(0, 0.0, true);
    let mut set = |root: Entity, from: u32, to: u32| {
        for e in std::iter::once(root).chain(children.iter_descendants(root)) {
            if let Ok(cur) = meshes.get(e)
                && cur.map_or(0, |t| t.value) == from
            {
                commands.entity(e).try_insert(MeshTag::new(to));
            }
        }
    };
    // Only meshes still carrying the highlight are cleared (an old entity may be gone or reused).
    for old in last.iter().filter(|e| now.binary_search(e).is_err()) {
        set(*old, on, 0);
    }
    for new in now.iter() {
        set(*new, 0, on);
    }
    std::mem::swap(last, now);
}
