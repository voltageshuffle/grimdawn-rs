//! Monsters from level proxies.
//!
//! A proxy runs once per session, when its level (and so its nav data) is
//! streamed in. What it produced is kept per proxy, so re-streaming a level
//! brings back the same monsters, minus the dead ones.
use bevy::prelude::*;
use gd_formats::collections::HashMap;
pub use gd_game::monsters::{LevelProp, LevelProxies, SetPieceStates};
use gd_game_core::monsters::Monster;
use gd_render::character::CharAssets;
use gd_render::model::Ready;
use gd_render::render::material::GdMaterial;
use std::sync::Arc;

pub struct MonstersPlugin;

impl Plugin for MonstersPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(crate::gibs::GibsPlugin)
            // The frame of the kill: after the hits' and the damage over time's deaths.
            .add_systems(
                Update,
                (death_effects, gib_deaths)
                    .after(gd_game_core::combat::CombatSet)
                    .after(gd_game_core::afflictions::AfflictionStage::Tick),
            )
            .add_systems(
                Update,
                (
                    spawn_level_props.after(gd_game::monsters::MonsterStage::LevelObjects),
                    #[cfg(feature = "dev")]
                    spawn_test_monsters,
                ),
            )
            .add_systems(
                Update,
                (draw_ambush_dissolve, draw_corpses).after(gd_game_core::combat::CombatSet),
            );
    }
}

/// `GD_SPAWN_MONSTER` (hooks.rs) spawns story-slot monsters
/// (never culled) at those level coordinates, each once the nav mesh covers
/// its spot, at the player's level.
#[allow(clippy::too_many_arguments)]
#[cfg(feature = "dev")]
fn spawn_test_monsters(
    mut commands: Commands,
    mut pending: Local<Option<Vec<(String, f32, f32)>>>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
    nav: Res<gd_game::nav::Nav>,
    mut assets: CharAssets,
    rules: Option<Res<gd_game_core::rules::CombatRules>>,
    mut profiles: ResMut<gd_game::monsters::ProfileCache>,
    factions: Option<Res<gd_game::factions::PlayerFactions>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
) {
    let (Some(rules), Some(loaded)) = (rules, loaded) else {
        return;
    };
    let list = pending.get_or_insert_with(|| hooks.gameplay.spawn_monster.clone());
    list.retain(|(rec, x, z)| {
        let Some(y) = nav.0.ground([*x, 100.0, *z]) else {
            return true;
        };
        let m = gd_game::monsters::MonsterState {
            record: rec.clone(),
            level: loaded.0.bio.level as i32,
            position: Vec3::new(*x, y, *z),
            yaw: 0.0,
            variant: 0,
            alive: true,
            pending: false,
            scale: None,
        };
        gd_game::monsters::spawn_state(
            &mut commands,
            &assets.data,
            &mut assets.bodies,
            &rules,
            factions.as_deref(),
            &mut profiles,
            &m,
            (usize::MAX, 0, 9000),
            &[],
        );
        info!("test monster {rec} at {x},{z}");
        false
    });
}
/// A newly loaded level's shown-only props, after
/// `gd_game::monsters::spawn_level_objects` spawned its gameplay objects (set-piece
/// nav obstacles and the ascendant altar included): static set-piece meshes (with
/// their mesh `CreateEntity` effects) and sound emitters. The props' models load in the background
/// (`CharAssets::model_ready`): a level's props spawn together once none is still loading. While it waits the level
/// holds the models already built (`waiting`, as `gd_render::character`'s `dress_bodies` does): the model cache only
/// keeps its last `gd_game_core::weak_cache::RECENT` models warm, so a crowd or level streaming beside it would otherwise
/// evict its early props before its last one arrives, over and over.
fn spawn_level_props(
    mut commands: Commands,
    levels: Query<(Entity, &LevelProxies), With<gd_game::monsters::LevelObjectsSpawned>>,
    set_states: Res<SetPieceStates>,
    mut assets: CharAssets,
    mut waiting: Local<HashMap<Entity, Vec<Arc<gd_render::model::ModelAsset>>>>,
) {
    waiting.retain(|e, _| levels.contains(*e));
    for (root, lp) in &levels {
        let db = assets.data.db.clone();
        let props = lp
            .set_pieces
            .iter()
            .filter_map(|(ei, ..)| set_states.0.get(&(lp.level, *ei)))
            .flatten()
            .filter(|o| {
                !matches!(
                    o.class.as_str(),
                    "Proxy" | "ProxyAmbush" | "Monster" | "Destructible" | "FixedItemContainer"
                )
            });
        // This frame's holds replace last frame's only after they were taken again.
        let (mut held, mut ready) = (Vec::new(), true);
        for mesh in props.clone().filter_map(|o| db.get(&o.record)?.str("mesh")) {
            match assets.model_ready(mesh) {
                Ready::Done(m) => held.push(m),
                Ready::Loading => ready = false,
                Ready::Failed => {}
            }
        }
        if !ready {
            waiting.insert(root, held);
            continue;
        }
        // The spawned props hold their models now (`held` lives to the end of this level's spawn).
        waiting.remove(&root);
        commands.entity(root).try_remove::<LevelProxies>();
        for o in props {
            let tf = Transform::from_translation(o.position)
                .with_rotation(o.rot)
                .with_scale(Vec3::splat(o.scale));
            let rec = db.get(&o.record);
            let see_through = rec.and_then(gd_render::see_through::max_transparency);
            // Built or failed (none is loading, above).
            let Some(Ready::Done(model)) = rec
                .and_then(|r| r.str("mesh"))
                .map(|m| assets.model_ready(m))
            else {
                continue;
            };
            let e = commands
                .spawn((
                    tf,
                    Visibility::default(),
                    LevelProp(lp.level),
                    Name::new("set piece object"),
                ))
                .id();
            let parts =
                gd_render::model::spawn_model_with(&mut commands, e, &model, |_, d| d.clone());
            // E.g. the flames of a burning barricade (`Actor::CreateAttachmentsFromMesh`).
            gd_render::fx::spawn_mesh_attachments(&mut commands, &assets.data, &model, e);
            if let Some(st) = see_through.and_then(|max| {
                gd_render::see_through::SeeThrough::new(
                    max,
                    &model.msh,
                    tf.compute_affine(),
                    &parts,
                    &model.part_sections,
                )
            }) {
                commands.entity(e).insert(st);
            }
        }
        for (rec, pos) in &lp.sounds {
            if let Some(e) = gd_audio::spawn_emitter(&mut commands, &assets.data, rec, *pos) {
                commands.entity(e).insert(LevelProp(lp.level));
            }
        }
    }
}

/// The record a dying monster or pet was made from (its death handlers read it).
fn death_record<'a>(
    m: Option<&'a Monster>,
    pet: Option<&'a gd_game::pets::Pet>,
) -> Option<&'a str> {
    m.map(|m| m.record.as_str())
        .or(pet.map(|p| p.record.as_str()))
}

/// An overkilled monster or pet with a `gibEffect` bursts when `gore` is on (`DefaultDeathHandler::Execute`):
/// the effect's particles and its gib meshes from the character's `Target` point
/// ([`crate::gibs`]), `gibSound` at the point, and the body is hidden (no corpse; the `Dead` timer still removes
/// the entity). `deathEffect` plays as usual ([`death_effects`]). A dismissed pet whose record sets
/// `overkillOnDismiss` / whose `gibThreshold` its life reaches dies this way (`gd_game::pets` docs, Release).
#[allow(clippy::too_many_arguments)]
fn gib_deaths(
    mut commands: Commands,
    mut assets: gd_render::character::CharAssets,
    options: Res<gd_game_core::options::GameOptions>,
    q: Query<
        (
            Entity,
            &GlobalTransform,
            Option<&Monster>,
            Option<&gd_game::pets::Pet>,
            Option<&gd_game_core::character::Character>,
        ),
        (
            Added<gd_game::combat::Overkill>,
            Or<(With<Monster>, With<gd_game::pets::Pet>)>,
        ),
    >,
) {
    if !options.flag("gore") {
        return;
    }
    for (e, gt, m, pet, ch) in &q {
        let db = assets.data.db.clone();
        let Some(rec) = death_record(m, pet).and_then(|r| db.get(r)) else {
            continue;
        };
        let Some(gib) = rec
            .str("gibEffect")
            .filter(|g| !g.is_empty())
            .and_then(|g| db.get(g))
        else {
            continue;
        };
        let target = ch.and_then(|c| gd_game_core::casting::attach_point_bind(&c.body, "Target"));
        let at = target.map_or(gt.translation() + Vec3::Y, |l| gt.transform_point(l));
        if let Some(pfx) = gib.str("effectFile").filter(|f| !f.is_empty()) {
            let mut spec = gd_game_core::visuals::EffectSpec::pfx(pfx);
            spec.despawn_when_complete = true;
            spec.no_loop = true;
            commands.spawn((spec, Transform::from_translation(at), Visibility::default()));
        }
        if let Some(pak) = rec.str("gibSound").filter(|s| !s.is_empty()) {
            commands.write_message(gd_game_core::visuals::PlaySound {
                pak: pak.to_owned(),
                at: Some(at),
            });
        }
        commands.entity(e).try_insert(gd_game_core::visuals::Hidden);
        crate::gibs::launch(&mut commands, &mut assets, gib, at);
    }
}

/// A dying character's `deathEffect` (the blood burst whose particles drop
/// the blood splatters, `casting::drop_decals`).
/// `Character::InitialUpdate` creates a `PlayEffectDeathHandler` when the
/// record has a `deathEffect`; `PlayEffectDeathHandler::Execute`: the effect
/// with looping disabled starts emitting, attached to the character as a
/// self-deleting child at its `Target` point with `attachDeathEffect`, else
/// placed at the `Target` point's coords; no gore option check. PORT:
/// monsters and pets, not the player; the `gibEffect` branch is [`gib_deaths`].
fn death_effects(
    mut commands: Commands,
    data: Res<gd_game_core::data::GameData>,
    q: Query<
        (
            Entity,
            &GlobalTransform,
            Option<&Monster>,
            Option<&gd_game::pets::Pet>,
            Option<&gd_game_core::character::Character>,
        ),
        (
            Added<gd_game_core::combat::Dead>,
            Or<(With<Monster>, With<gd_game::pets::Pet>)>,
        ),
    >,
) {
    for (e, gt, m, pet, ch) in &q {
        let Some(rec) = death_record(m, pet).and_then(|r| data.db.get(r)) else {
            continue;
        };
        let Some(fx) = rec
            .str("deathEffect")
            .filter(|f| !f.is_empty())
            .map(str::to_owned)
        else {
            continue;
        };
        let target = ch.and_then(|c| gd_game_core::casting::attach_point_bind(&c.body, "Target"));
        let mut spec = gd_game_core::visuals::EffectSpec::record(fx);
        spec.despawn_when_complete = true;
        spec.no_loop = true;
        if rec.bool("attachDeathEffect") {
            let local = target.unwrap_or(Vec3::Y);
            let child = commands
                .spawn((
                    spec,
                    Transform::from_translation(local),
                    Visibility::default(),
                    gd_render::fx::WithDecal,
                ))
                .id();
            commands.entity(e).add_child(child);
        } else {
            let at = target.map_or(gt.translation() + Vec3::Y, |l| gt.transform_point(l));
            commands.spawn((
                spec,
                Transform::from_translation(at).with_rotation(gt.compute_transform().rotation),
                Visibility::default(),
                gd_render::fx::WithDecal,
            ));
        }
    }
}

/// The parts of a monster drawn with the dissolve shader: (part, its material, the dissolve copy).
#[derive(Component)]
struct DissolveParts(Vec<(Entity, Handle<GdMaterial>, Handle<GdMaterial>)>);

/// Draws a hidden monster's un-dissolve ([`gd_game::combat::AmbushDissolve`]): every lit mesh part under it
/// (its body and attached items, as `ControllerMonsterStateHidden::OnUpdate` begins it on the mesh and
/// each attached actor) swaps to a copy of its material drawn as the `standarddissolve*` override shader
/// (no glow or detail map, [`FLAG_DISSOLVE`]: `fade.z` = the value, the monster record's
/// `ambushDissolveTexture` in the detail slot, default `System/Textures/Cloud.tex`,
/// `Monster::Load`); the originals come back when the component goes (the override shader
/// is unloaded, `GraphicsMeshInstance::Update`).
#[allow(clippy::too_many_arguments)]
fn draw_ambush_dissolve(
    mut commands: Commands,
    data: Res<gd_game_core::data::GameData>,
    mut textures: ResMut<gd_render::textures::Textures>,
    mut images: ResMut<Assets<Image>>,
    dissolving: Query<
        (
            Entity,
            &gd_game::combat::AmbushDissolve,
            &Monster,
            Option<&DissolveParts>,
        ),
        Changed<gd_game::combat::AmbushDissolve>,
    >,
    held: Query<&DissolveParts, Without<gd_game::combat::AmbushDissolve>>,
    mut ended: RemovedComponents<gd_game::combat::AmbushDissolve>,
    children: Query<&Children>,
    mut parts: Query<&mut MeshMaterial3d<GdMaterial>>,
    mut materials: ResMut<Assets<GdMaterial>>,
) {
    use gd_render::render::material::*;
    for e in ended.read() {
        let Ok(d) = held.get(e) else { continue };
        for (part, orig, _) in &d.0 {
            if let Ok(mut m) = parts.get_mut(*part) {
                m.0 = orig.clone();
            }
        }
        commands.entity(e).try_remove::<DissolveParts>();
    }
    for (e, d, monster, swapped) in &dissolving {
        if let Some(swapped) = swapped {
            // Write only on change: `get_mut` marks the material modified (re-prepared).
            for (_, _, copy) in &swapped.0 {
                if materials
                    .get(copy)
                    .is_some_and(|m| m.params.fade.z != d.value)
                    && let Some(mut m) = materials.get_mut(copy)
                {
                    m.params.fade.z = d.value;
                }
            }
            continue;
        }
        let path = data
            .db
            .get(&monster.record)
            .and_then(|r| r.str("ambushDissolveTexture").map(str::to_owned))
            .unwrap_or_else(|| "System/Textures/Cloud.tex".into());
        let Some(tex) = textures.get(&data, &path, false, &mut images) else {
            continue;
        };
        let mut list = Vec::new();
        for part in std::iter::once(e).chain(children.iter_descendants(e)) {
            let Ok(mut mat) = parts.get_mut(part) else {
                continue;
            };
            let Some(mut m) = materials.get(&mat.0).cloned() else {
                continue;
            };
            if m.params.flags.x != KIND_LIT || m.params.outline.w > 0.0 {
                continue;
            }
            m.params.flags.y &=
                !(FLAG_GLOW | FLAG_GLOW_SCROLL | FLAG_GLOW_SCROLL_ALPHA | FLAG_DETAIL);
            m.params.flags.y |= FLAG_DISSOLVE;
            m.params.fade.z = d.value;
            m.detail = tex.clone();
            let copy = materials.add(m);
            list.push((part, mat.0.clone(), copy.clone()));
            mat.0 = copy;
        }
        commands.entity(e).try_insert(DissolveParts(list));
    }
}

/// What a corpse's death handler draws on it ([`draw_corpses`]): its lit parts' per-instance copies (part, copy,
/// the material's own diffuse colour, an attachment: armour or a weapon), the dissolve's light and its radius.
#[derive(Component)]
struct CorpseLook {
    parts: Vec<(Entity, Handle<GdMaterial>, Vec4, bool)>,
    light: Option<Entity>,
    radius: f32,
}

/// A corpse's dissolve or fade, from its death-handler data ([`gd_game::combat::Corpse`]) over the last
/// `dissolveTime` s of its `Dead` countdown (`gd_game::combat::corpse_time`: the dissolve starts after the
/// `corpsePersistence` wait or at the death clip's `dissolve` callback, the fade at the death):
/// - `Dissolve` / `DissolveOnCallback`: `DissolveActorDeathHandler::StartDissolve` →
///   `GraphicsMeshInstance::BeginDissolve`: the dissolve value 0 → 1 over the time, drawn by the
///   `standarddissolve*` override shader with the record's `dissolveTexture` and `dissolveColorR/G/B` / 255
///   ([`FLAG_DISSOLVE`](gd_render::render::material::FLAG_DISSOLVE) and its `emissiveStyle` edge);
///   `Character::DissolveAttachments` dissolves the worn items (armour, weapons) in 0.9 of the
///   time. It attaches the record's `dissolveEffect` to the body and its `dissolveLight` at the `Target` point,
///   red, radius 10 shrinking 5 m/s (`DissolveActorDeathHandler::Update`; no stock record sets a
///   light). Without a `dissolveTexture` the engine binds none and the cut takes the whole mesh at once: the body
///   is hidden then.
/// - `Fade`: `FadeActorDeathHandler::Update`: with k = 1 − elapsed / time the tint goes to
///   black (the diffuse colour × k) and the instance's blend alpha multiplier to 0 (`fade.x` = k, read by the
///   `standardblended*` / vertex-alpha materials), on the body and its attached meshes.
///
/// The copies stay until the body goes (`dead_bodies` removes it as the countdown ends). A hidden body
/// (`gd_game_core::visuals::Hidden`: died out of view, or a dissolve without a texture) draws nothing, so its copies
/// are not rewritten each frame (each write re-prepares the material); they catch up the frame it shows.
#[allow(clippy::too_many_arguments)]
fn draw_corpses(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<gd_game_core::data::GameData>,
    mut textures: ResMut<gd_render::textures::Textures>,
    mut images: ResMut<Assets<Image>>,
    mut corpses: Query<(
        Entity,
        &gd_game_core::combat::Dead,
        &gd_game::combat::Corpse,
        &gd_game_core::character::Character,
        Option<&gd_render::character::Body>,
        Option<&mut CorpseLook>,
        Has<gd_game_core::visuals::Hidden>,
    )>,
    children: Query<&Children>,
    mut parts: Query<&mut MeshMaterial3d<GdMaterial>>,
    mut materials: ResMut<Assets<GdMaterial>>,
    mut lights: Query<&mut PointLight>,
) {
    use gd_render::render::material::*;
    let dt = time.delta_secs();
    for (e, dead, corpse, ch, body, look, hidden) in &mut corpses {
        let dissolve = matches!(
            corpse.delete_behavior.as_str(),
            "Dissolve" | "DissolveOnCallback"
        );
        if !(dissolve || corpse.delete_behavior == "Fade")
            || corpse.dissolve_time <= 0.0
            || !dead.timer.is_finite()
            || dead.timer > corpse.dissolve_time
        {
            continue;
        }
        let p = (1.0 - dead.timer / corpse.dissolve_time).clamp(0.0, 1.0);
        let Some(mut look) = look else {
            // The handler starts: the copies, the effect and the light.
            let texture = if dissolve {
                let t = corpse
                    .dissolve_texture
                    .as_deref()
                    .and_then(|t| textures.get(&data, t, false, &mut images));
                if t.is_none() {
                    commands.entity(e).try_insert(gd_game_core::visuals::Hidden);
                }
                t
            } else {
                None
            };
            let worn: Vec<Entity> = body.map_or(Vec::new(), |b| {
                b.armour
                    .iter()
                    .map(|a| a.0)
                    .chain(b.weapons.iter().flat_map(|w| w.parts.iter().copied()))
                    .collect()
            });
            let weapons: Vec<Entity> = body.map_or(Vec::new(), |b| {
                b.weapons
                    .iter()
                    .flat_map(|w| w.parts.iter().copied())
                    .collect()
            });
            let mut list = Vec::new();
            if !dissolve || texture.is_some() {
                for part in children.iter_descendants(e).chain(weapons) {
                    let Ok(mut mat) = parts.get_mut(part) else {
                        continue;
                    };
                    let Some(mut m) = materials.get(&mat.0).cloned() else {
                        continue;
                    };
                    if m.params.flags.x != KIND_LIT || m.params.outline.w > 0.0 {
                        continue;
                    }
                    let own = m.params.diffuse;
                    if let Some(t) = &texture {
                        m.params.flags.y &=
                            !(FLAG_GLOW | FLAG_GLOW_SCROLL | FLAG_GLOW_SCROLL_ALPHA | FLAG_DETAIL);
                        m.params.flags.y |= FLAG_DISSOLVE;
                        m.params.fade.z = 0.0;
                        m.params.dissolve = (Vec3::from(corpse.dissolve_color) / 255.0).extend(0.0);
                        m.detail = t.clone();
                    }
                    let copy = materials.add(m);
                    mat.0 = copy.clone();
                    list.push((part, copy, own, worn.contains(&part)));
                }
            }
            let mut light = None;
            if dissolve {
                if let Some(fx) = &corpse.dissolve_effect {
                    let child = commands
                        .spawn((
                            gd_game_core::visuals::EffectSpec::record(fx.clone()),
                            Transform::IDENTITY,
                            Visibility::default(),
                        ))
                        .id();
                    gd_render::model::try_add_child(&mut commands, e, child);
                }
                if let Some(rec) = corpse
                    .dissolve_light
                    .as_deref()
                    .and_then(|l| data.db.get(l))
                    && let Some(mut l) = gd_render::render::lights::light_from_record(
                        rec,
                        ch.posed_point("Target").unwrap_or(Vec3::Y),
                    )
                {
                    l.color = Vec3::new(1.0, 0.0, 0.0);
                    l.radius = 10.0;
                    let id = gd_render::render::lights::spawn_light(&mut commands, &l);
                    gd_render::model::try_add_child(&mut commands, e, id);
                    light = Some(id);
                }
            }
            commands.entity(e).try_insert(CorpseLook {
                parts: list,
                light,
                radius: 10.0,
            });
            continue;
        };
        let look = &mut *look;
        let shown: &[_] = if hidden { &[] } else { &look.parts };
        for (_, copy, own, worn) in shown {
            let Some((fade, diffuse, flags)) = materials
                .get(copy)
                .map(|m| (m.params.fade, m.params.diffuse, m.params.flags.y))
            else {
                continue;
            };
            if dissolve {
                let v = if *worn { (p / 0.9).min(1.0) } else { p };
                if fade.z != v
                    && let Some(mut m) = materials.get_mut(copy)
                {
                    m.params.fade.z = v;
                }
            } else {
                let k = 1.0 - p;
                let tint = (own.truncate() * k).extend(own.w);
                let blended = flags & (FLAG_BLENDED_FADE | FLAG_VERTEX_ALPHA) != 0;
                if (diffuse != tint || (blended && fade.x != k))
                    && let Some(mut m) = materials.get_mut(copy)
                {
                    m.params.diffuse = tint;
                    if blended {
                        m.params.fade.x = k;
                    }
                }
            }
        }
        if let Some(l) = look.light {
            look.radius = (look.radius - dt * 5.0).max(0.0);
            if let Ok(mut pl) = lights.get_mut(l) {
                pl.range = look.radius;
                pl.intensity = 40_000.0 * look.radius;
            }
        }
    }
}
