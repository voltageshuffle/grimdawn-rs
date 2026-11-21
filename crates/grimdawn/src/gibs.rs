//! Gibs: the meshes a `GibEffectEntity` launches.
//!
//! `GibEffectEntity::Load` / `InitialUpdate`: `minGibs`..`maxGibs` gib meshes (random `gibNames`, each
//! name once unless `multipleGibs`; scale `minGibScale`..`maxGibScale`), launched `launchDistance` m from the
//! effect's position in the upper hemisphere at `minGibSpeed`..`maxGibSpeed` m/s. Two makers: an overkill death
//! (`monsters::gib_deaths`, the `gibEffect` at the `Target` point, gated by `gore` in the death handler) and an
//! animation's `CreateEntity` event of a `GibEffectEntity` record (`gd_render::model::AnimGibs`, at the event's
//! attach point; no option gate: the gate is in the death path only). A gib whose model is built is dressed with
//! it and given its world frame as it launches, so it draws the frame the body it bursts from is hidden, and an
//! animation's gibs the frame the event fires (`anim_gibs` runs after the PostUpdate event systems, before bounds
//! and visibility read the new parts). PORT: a gib whose model is still loading in the background
//! (`ModelCache::model_ready`) flies bare and is dressed once it is built ([`dress_gibs`]).
use bevy::prelude::*;
use gd_formats::arz::Record;

pub struct GibsPlugin;

impl Plugin for GibsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (dress_gibs.before(move_gibs), move_gibs))
            .add_systems(
                PostUpdate,
                anim_gibs
                    .after(gd_render::model::AnimEntitySet)
                    .before(bevy::asset::AssetEventSystems)
                    .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate)
                    .in_set(gd_render::model::ModelSpawnSet),
            );
    }
}

/// A flying gib (`Gib`, an `Actor` on free rigid-body physics; `GibEffectEntity::InitialUpdate` /
/// `Gib::UpdateSelf` @0x1803182a0): it lives `clamp(scale, 0.5, 2) * 10` s, then fades for 3 s.
/// Each floor contact plays `impactGroundSound` and drops the gib's `ragdollDecal` (`Gib::CollisionCallback`
/// @0x1803183f0 / `Actor::CollisionCallback`: world contact only, `impactCooldown` and the decal's per-body cooldown
/// both 1000 ms; the sound only while `lifetime > 0`; `World::PlaceDecal` is gated by the `decals` option, here
/// `detailObjects` in `casting::drop_decals`, which also keeps the 256-decal ring; the decal grows in, flag 0x40000).
/// PORT: ballistic motion with a bounce off the floor (no rigid-body physics: a contact is a descending hit on the
/// physics floor below, `gd_game::ragdoll::floor`, and a rising gib stops under what it meets; the water material 7
/// exclusion is not applied, the port has no per-contact material).
/// `Gib::UpdateSelf`: the fade is `GraphicsMeshInstance::BeginFade(255, 255, 255, 3 s)` —
/// the instance opacity 1→0 (`blendedGlobalFadeFactor`), which the `standardblended*` shaders draw
/// as `base.a · factor` (render/material.rs `FLAG_BLENDED_FADE`, `fade.x` here). The harpy gibs
/// (`bonefadeskinned`) and the albedo / glow ones do not read the factor: they stay opaque until
/// removed, as in the engine.
#[derive(Component)]
struct GibBody {
    vel: Vec3,
    spin: Vec3,
    life: f32,
    /// The record's `ragdollDecal` and `impactGroundSound`.
    decal: Option<String>,
    sound: Option<String>,
    /// Seconds until the next contact may place a decal / play the sound ([`contact_ready`]).
    cooldown: f32,
    /// The fade materials ([`crate::fade::begin_fade`] of the model's `standardblended*` materials), made once
    /// when the 3 s fade starts; `Some([])` when the mesh has none (it stays opaque, engine parity).
    faded: Option<Vec<Handle<gd_render::render::material::GdMaterial>>>,
}

/// A launched gib's mesh, still loading in the background ([`dress_gibs`]).
#[derive(Component)]
struct GibMesh(String);

/// Dresses the gibs whose model was still loading at launch, once it is built; one that cannot load stays bare.
fn dress_gibs(
    mut commands: Commands,
    pending: Query<(Entity, &GibMesh)>,
    mut assets: gd_render::character::CharAssets,
) {
    for (e, m) in &pending {
        match assets.model_ready(&m.0) {
            gd_render::model::Ready::Loading => continue,
            gd_render::model::Ready::Done(model) => {
                gd_render::model::spawn_model(&mut commands, e, &model);
            }
            gd_render::model::Ready::Failed => {}
        }
        commands.entity(e).try_remove::<GibMesh>();
    }
}

/// Launches the gib meshes of the `GibEffectEntity` record `gib` from `at` (module doc), each dressed at once
/// when its model is built (else by [`dress_gibs`]); its `effectFile` particles are the caller's.
pub fn launch(
    commands: &mut Commands,
    assets: &mut gd_render::character::CharAssets,
    gib: &Record,
    at: Vec3,
) {
    use gd_game::rng;
    // `GibEffectEntity::Load`: the count and the names.
    let mut names: Vec<String> = gib.strs("gibNames").iter().map(|s| s.to_string()).collect();
    let (lo, hi) = (
        gib.i32("minGibs").unwrap_or(0).max(0),
        gib.i32("maxGibs").unwrap_or(0).max(0),
    );
    let count = if hi >= lo {
        lo + (rng::f32() * (hi - lo + 1) as f32) as i32
    } else {
        lo
    };
    let multiple = gib.bool("multipleGibs");
    let f = |k: &str, d: f32| gib.f32(k).unwrap_or(d);
    let (smin, smax) = (f("minGibScale", 1.0), f("maxGibScale", 1.0));
    let (vmin, vmax) = (f("minGibSpeed", 0.0), f("maxGibSpeed", 0.0));
    let dist = f("launchDistance", 0.0);
    for _ in 0..count {
        if names.is_empty() {
            break;
        }
        let i = ((rng::f32() * names.len() as f32) as usize).min(names.len() - 1);
        let name = if multiple {
            names[i].clone()
        } else {
            names.remove(i)
        };
        let Some(grec) = assets.data.db.get(&name) else {
            continue;
        };
        let Some(mesh) = grec.str("mesh").map(str::to_owned) else {
            continue;
        };
        let nonempty = |k: &str| grec.str(k).filter(|s| !s.is_empty()).map(str::to_owned);
        let scale = smin + rng::f32() * (smax - smin);
        // The upper hemisphere: x, z in -1..1, y in 0..1.
        let d = Vec3::new(rng::f32() * 2.0 - 1.0, rng::f32(), rng::f32() * 2.0 - 1.0)
            .normalize_or(Vec3::Y);
        let speed = vmin + rng::f32() * (vmax - vmin);
        let tau = std::f32::consts::TAU;
        let tf = Transform::from_translation(at + d * dist).with_scale(Vec3::splat(scale));
        // The world frame now: an animation's gibs spawn after this frame's propagation (`anim_gibs`).
        let world = GlobalTransform::from(tf);
        let e = commands
            .spawn((
                tf,
                world,
                Visibility::default(),
                GibBody {
                    vel: d * speed,
                    spin: Vec3::new(rng::f32(), rng::f32(), rng::f32()) * tau,
                    life: scale.clamp(0.5, 2.0) * 10.0,
                    decal: nonempty("ragdollDecal"),
                    sound: nonempty("impactGroundSound"),
                    cooldown: 0.0,
                    faded: None,
                },
            ))
            .id();
        match assets.model_ready(&mesh) {
            gd_render::model::Ready::Done(model) => {
                gd_render::model::spawn_model_at(commands, e, &model, &world);
            }
            gd_render::model::Ready::Loading => {
                commands.entity(e).insert(GibMesh(mesh));
            }
            gd_render::model::Ready::Failed => {}
        }
    }
}

/// An animation event's `GibEffectEntity` (`gd_render::model::AnimGibs`): its gibs fly from the attach point,
/// drawn this frame (module doc).
fn anim_gibs(
    mut commands: Commands,
    mut assets: gd_render::character::CharAssets,
    mut events: MessageReader<gd_render::model::AnimGibs>,
) {
    for g in events.read() {
        let db = assets.data.db.clone();
        if let Some(rec) = db.get(&g.record) {
            launch(&mut commands, &mut assets, rec, g.at);
        }
    }
}

/// The contact cooldown (`impactCooldown`, 1000 ms): true, and re-armed, when a contact may act.
fn contact_ready(cooldown: &mut f32) -> bool {
    if *cooldown > 0.0 {
        return false;
    }
    *cooldown = 1.0;
    true
}

/// Where a landed gib's decal goes: on the ground under it (`World::PlaceDecal` then lifts it 0.01 m along the normal).
fn decal_spot(p: Vec3, floor: f32) -> Vec3 {
    Vec3::new(p.x, floor, p.z)
}

/// Moves the gibs: gravity, a bounce off the floor, spin; at the end of its life a gib fades out over 3 s
/// (the `BeginFade` opacity, module doc).
#[allow(clippy::too_many_arguments)]
fn move_gibs(
    mut commands: Commands,
    time: Res<Time>,
    nav: Res<gd_game::nav::Nav>,
    coll: Res<crate::collision::WorldCollision>,
    mut q: Query<(Entity, &mut Transform, &mut GibBody)>,
    children: Query<&Children>,
    mut parts: Query<&mut MeshMaterial3d<gd_render::render::material::GdMaterial>>,
    mut materials: ResMut<Assets<gd_render::render::material::GdMaterial>>,
) {
    use gd_game::rng;
    const GRAVITY: f32 = 9.8;
    let dt = time.delta_secs();
    for (e, mut tf, mut g) in &mut q {
        g.life -= dt;
        if g.life <= -3.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        g.vel.y -= GRAVITY * dt;
        let pos = tf.translation;
        let mut p = pos + g.vel * dt;
        // Rising into a ceiling (a mesh's underside): it stops there and falls back.
        if g.vel.y > 0.0
            && let Some((hit, _)) = coll.segment_hit(pos, p)
        {
            p = hit - (p - pos).normalize_or_zero() * 0.05;
            g.vel.y = -g.vel.y * 0.3;
        }
        let (floor, normal) = gd_game::ragdoll::floor(&coll, &nav, pos, p);
        g.cooldown = (g.cooldown - dt).max(0.0);
        if p.y < floor + 0.1 {
            p.y = floor + 0.1;
            let landing = g.vel.y < -0.5;
            if g.vel.y < 0.0 {
                g.vel.y = -g.vel.y * 0.3;
            }
            if landing && contact_ready(&mut g.cooldown) {
                // `Actor::CollisionCallback`: the decal's Y is the contact normal, its yaw random.
                let yaw = Quat::from_rotation_arc(Vec3::Y, normal)
                    * Quat::from_rotation_y(rng::f32() * std::f32::consts::TAU);
                if let Some(d) = g.decal.clone() {
                    commands.spawn((
                        gd_render::fx::DropDecal(d),
                        // `Actor::CollisionCallback`'s flag 0x40000: it grows in.
                        gd_render::decal::GrowIn::default(),
                        Transform::from_translation(decal_spot(p, floor)).with_rotation(yaw),
                    ));
                }
                if let (Some(pak), true) = (g.sound.clone(), g.life > 0.0) {
                    commands.write_message(gd_game::visuals::PlaySound { pak, at: Some(p) });
                }
            }
            // Floor friction is per-contact in the engine; scale it by dt so
            // it is framerate-independent (0.7 per 1/60 s).
            let f = 0.7f32.powf(dt * 60.0);
            g.vel.x *= f;
            g.vel.z *= f;
            g.spin *= f;
            if g.vel.length() < 0.3 {
                g.vel = Vec3::ZERO;
                g.spin = Vec3::ZERO;
            }
        }
        tf.translation = p;
        tf.rotation = Quat::from_euler(EulerRot::XYZ, g.spin.x * dt, g.spin.y * dt, g.spin.z * dt)
            * tf.rotation;
        if g.life < 0.0 {
            let factor = (1.0 + g.life / 3.0).max(0.0);
            if g.faded.is_none() {
                use gd_render::render::material::{FLAG_BLENDED_FADE, FLAG_VERTEX_ALPHA};
                g.faded = crate::fade::begin_fade(
                    children.iter_descendants(e),
                    FLAG_BLENDED_FADE | FLAG_VERTEX_ALPHA,
                    false,
                    &mut parts,
                    &mut materials,
                );
            }
            crate::fade::set_fade(g.faded.as_deref().unwrap_or(&[]), factor, &mut materials);
        }
    }
}
