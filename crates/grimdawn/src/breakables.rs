//! The dressing of broken breakables: their pieces' physics and fade (the rules: `gd_game::breakables`).
//!
//! The pieces (Game.dll `Destructible::PhysicsSetup`):
//! every bone of the broken mesh with a hit box becomes a Bullet box body at
//! the bone's frame (its origin × the mesh scale, placed by the object), the
//! box the hit box's half sizes × 0.75, each at least 0.1 m, of the record's
//! `physicsMass`, `physicsFriction` and `physicsRestitution`, with the items'
//! damping and sleeping thresholds (the description's (1.6, 2.5, 0.05, 0.95),
//! `gd_sim::drop_body`). The bones `PickStatics` names are static
//! bodies: every `fixedStatics` bone plus min..max (`minNumRandomStatics` /
//! `maxNumRandomStatics`, max below min: min) of the shuffled `randomStatics`.
//! A bone without a hit box gets no body. `Destructible::Load`
//! keeps `breakForceMin..Max` × 0.02 ([`FORCE_SCALE`]); each body gets its own
//! roll as a central impulse (Bullet `applyCentralImpulse`: velocity = impulse /
//! mass) and a torque impulse (`applyTorqueImpulse`: angular velocity = the
//! world inverse inertia × torque), all in world space, by `breakMode`
//! (`BreakModeFromString`): Explode along (r, 1, r') normalised (r in -1..1),
//! torque 0.05 × the impulse; React along `BreakApart`'s direction, both rolls × its force;
//! Fall along that direction turned by the object's rotation (`WorldCoords::TransformVector`)
//! and tilted by up to ±0.3 on two perpendicular axes; React and Fall torque
//! 0.025 × a second roll on a random -1..1 axis; any other mode no push. The
//! direction and force are [`Broke`]'s (`TakeAttack` → `BreakApart(dir, 1, …)`, a script's
//! `Destructible:Destroy(dir, force, …)`), which `BreakApart` stores over the
//! `fallDirectionX/Y/Z` the record loaded, so Fall never reads
//! them. The world's gravity is 14 m/s² and its restitution 0: the pieces do
//! not bounce; they tumble, slide to a stop on the terrain and the static
//! scenery, and sleep once below 1.6 m/s and 2.5 rad/s for 2 s (Bullet's
//! deactivation).
//!
//! PORT: each piece is a `gd_sim::drop_body::DropBody` (the dropped items'
//! Bullet box) against the terrain and the static scenery near it (where no
//! collision is loaded, a plane at the nav ground, as the ragdolls' floor); the
//! pieces do not collide with each other, and a piece's first contact is its
//! one landing (`Destructible::CollisionCallback`, at most every 250 ms in the
//! engine).
//!
//! `Destructible::UpdateSelf` / `BreakApart`:
//! the pieces lie for `rubbleLife`, then every dynamic piece
//! bone's transparency is `remaining / rubbleFade` (`SetBoneTransparancies`),
//! which `bonefadeskinned.ssh` draws as `base.a · factor` (render/material.rs
//! `FLAG_BONE_FADE`), while the static ones stay opaque; the port fades the
//! mesh's `bonefadeskinned` materials the same way (one factor per material,
//! so the static pieces fade with the rest and the broken mesh goes once
//! faded).
//!
//! PORT: the broken mesh loads in the background (`CharAssets::model_ready`); one not built at the break is
//! spawned on a later frame ([`PendingDebris`]), while the break sound and effects play at once.
use bevy::math::Affine3A;
use bevy::prelude::*;
pub use gd_game::breakables::*;
pub use gd_game_core::world::Breakable;
use gd_render::character::CharAssets;
use gd_render::model::Ready;
use gd_render::render::material::{FLAG_BONE_FADE, GdMaterial};
use gd_sim::drop_body::{self, DropBody};
use std::sync::Arc;

pub struct BreakablesPlugin;

impl Plugin for BreakablesPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<Breakable, Visibility>()
            .add_systems(
                Update,
                (
                    dress_breakables,
                    debris.after(gd_game::breakables::BreakableStage::Break),
                    pending_debris,
                    #[cfg(feature = "dev")]
                    test_debris,
                ),
            )
            .add_systems(PostUpdate, fly_debris.after(TransformSystems::Propagate));
    }
}

/// New breakables get their model (with the record's texture overrides,
/// `CharAssets::spawn_record_model`) once it is loaded (`CharAssets::record_ready`; loaded in the background, so a
/// streamed-in level's breakables never build their models on the game thread).
fn dress_breakables(
    mut commands: Commands,
    new: Query<(Entity, &Breakable), Without<gd_render::character::RecordDressed>>,
    mut a: CharAssets,
) {
    for (e, b) in &new {
        let db = a.data.db.clone();
        let rec = db.get(&b.record);
        if rec.is_some_and(|r| !a.record_ready(r)) {
            continue;
        }
        commands
            .entity(e)
            .try_insert(gd_render::character::RecordDressed);
        if let Some(rec) = rec {
            a.spawn_record_model(&mut commands, e, rec);
        }
    }
}

/// Broken objects throw their broken pieces and play their break effects.
fn debris(mut commands: Commands, mut broke: MessageReader<Broke>, mut a: CharAssets) {
    for b in broke.read() {
        let db = a.data.db.clone();
        if let Some(rec) = db.get(&b.record) {
            spawn_debris(&mut commands, &mut a, rec, b.transform, (b.dir, b.force));
        }
    }
}

/// A broken mesh and its pieces (one per bone of the broken mesh).
#[derive(Component)]
struct Debris {
    pieces: Vec<Piece>,
    /// The physics world's fixed steps (`gd_sim::drop_body::Clock`).
    clock: drop_body::Clock,
    age: f32,
    life: f32,
    fade: f32,
    /// The impulse range: `breakForceMin..Max` × [`FORCE_SCALE`].
    force: (f32, f32),
    /// `physicsMass` and `physicsFriction`.
    mass: f32,
    friction: f32,
    /// 0 React, 2 Explode, 3 Fall, else no push (`BreakModeFromString`).
    mode: u8,
    /// React: the attack direction; Fall: it turned by the object's rotation (world space, module doc).
    dir: Vec3,
    /// The bones that stay (`PickStatics`, module doc).
    statics: Vec<String>,
    /// The record's `impactSound`: each piece plays it once, where it first lands (`Destructible::CollisionCallback`;
    /// at the destructible's own position). PORT: a piece's landing is its
    /// first contact. `Destructible::PlayBodyFallSound`: a landing on water (surface 7) plays
    /// `splashSound` instead. PORT: the piece's surface is the one under it at its landing (`surface_below`).
    impact: Option<String>,
    splash: Option<String>,
    /// The model's part entities (their materials take the rubble fade).
    parts: Vec<Entity>,
    /// The fade materials: per-instance copies of the parts' `bonefadeskinned`
    /// materials ([`FLAG_BONE_FADE`]), made once when the fade starts
    /// (`fly_debris`); `Some([])` when the mesh has none (it stays opaque).
    faded: Option<Vec<Handle<GdMaterial>>>,
}

/// The pak of a piece's first landing on `surface` (`PlayBodyFallSound`: water is `splashSound`).
fn fall_pak<'a>(surface: u32, impact: Option<&'a str>, splash: Option<&'a str>) -> Option<&'a str> {
    if surface == gd_sim::surface::Surface::Water as u32 {
        splash
    } else {
        impact
    }
}

/// A broken piece: its bone's body, if it has one (module doc).
struct Piece {
    /// `None`: the piece stays (a static bone, or one without a hit box).
    body: Option<DropBody>,
    /// The body's centre and turn at the break (the bone's frame in the world).
    pos0: Vec3,
    rot0: Quat,
    /// Seconds below the sleeping speeds; asleep from [`SLEEP_TIME`].
    still: f32,
    landed: bool,
}

/// `Destructible::Load` (module doc): the impulse is the record's force × this.
const FORCE_SCALE: f32 = 0.02;
/// `Destructible::PhysicsSetup`: a piece's box is its hit box × this.
const BOX_SCALE: f32 = 0.75;
/// Bullet's deactivation (module doc): below these speeds (m/s, rad/s) for [`SLEEP_TIME`] s a body sleeps.
const SLEEP_SPEED: f32 = 1.6;
const SLEEP_SPIN: f32 = 2.5;
const SLEEP_TIME: f32 = 2.0;

impl Debris {
    /// The pieces of the broken mesh `msh` placed by `root` (`Destructible::PhysicsSetup`, module doc).
    fn launch(&self, msh: &gd_formats::msh::Mesh, root: &GlobalTransform) -> Vec<Piece> {
        let (lo, hi) = self.force;
        let roll = || lo + (hi - lo).max(0.0) * gd_game_core::rng::f32();
        let sym = || gd_game_core::rng::f32() * 2.0 - 1.0;
        let dir = self.dir;
        let (p1, p2) = {
            let a = if dir.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
            let p1 = dir.cross(a).normalize_or(Vec3::X);
            (p1, dir.cross(p1).normalize_or(Vec3::Z))
        };
        let root_m = root.affine();
        let root_rot = root.rotation();
        msh.bones
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let pos0 = root_m.transform_point3(Vec3::from(b.abs.origin));
                let rot0 = (root_rot * frame_rotation(&b.abs)).normalize();
                let fixed = self.statics.iter().any(|s| s.eq_ignore_ascii_case(&b.name));
                let hit_box = msh
                    .hit_boxes
                    .iter()
                    .find(|h| match usize::try_from(h.bone) {
                        Ok(bone) => bone == i,
                        Err(_) => h.name.eq_ignore_ascii_case(&b.name),
                    });
                let body = hit_box.filter(|_| !fixed).map(|h| {
                    let axes = [h.coords.x, h.coords.y, h.coords.z];
                    let half = [0, 1, 2].map(|k| {
                        (h.extents[k] * Vec3::from(axes[k]).length() * BOX_SCALE)
                            .max(drop_body::MIN_HALF_EXTENT)
                    });
                    let (impulse, torque) = match self.mode {
                        2 => {
                            let v = Vec3::new(sym(), 1.0, sym()).normalize() * roll();
                            (v, v * 0.05)
                        }
                        3 => {
                            let t = (dir
                                + p1 * (gd_game_core::rng::f32() * 0.6 - 0.3)
                                + p2 * (gd_game_core::rng::f32() * 0.6 - 0.3))
                                .normalize_or(dir);
                            (t * roll(), Vec3::new(sym(), sym(), sym()) * roll() * 0.025)
                        }
                        0 => (
                            dir * roll(),
                            Vec3::new(sym(), sym(), sym()) * roll() * 0.025,
                        ),
                        _ => (Vec3::ZERO, Vec3::ZERO),
                    };
                    // The box's inertia about its axes: m (b² + c²) / 3 of the half extents.
                    let h2 = Vec3::from(half) * Vec3::from(half);
                    let inertia =
                        Vec3::new(h2.y + h2.z, h2.x + h2.z, h2.x + h2.y) * (self.mass / 3.0);
                    let mut body = DropBody::new(
                        pos0.to_array(),
                        rot0.to_array(),
                        (impulse / self.mass).to_array(),
                        half,
                        self.friction,
                    );
                    body.spin = (rot0 * ((rot0.inverse() * torque) / inertia)).to_array();
                    body
                });
                Piece {
                    body,
                    pos0,
                    rot0,
                    still: 0.0,
                    landed: false,
                }
            })
            .collect()
    }
}

/// The turn of a mesh-space bone frame (its axes normalised; a mirrored frame's y axis negated, as
/// `Destructible::PhysicsSetup` does before it makes the body).
fn frame_rotation(c: &gd_formats::msh::Coords) -> Quat {
    let x = Vec3::from(c.x).normalize_or(Vec3::X);
    let mut y = Vec3::from(c.y).normalize_or(Vec3::Y);
    let z = Vec3::from(c.z).normalize_or(Vec3::Z);
    if x.cross(y).dot(z) < 0.0 {
        y = -y;
    }
    Quat::from_mat3(&Mat3::from_cols(x, y, z)).normalize()
}

/// The pieces' physics, their landings, the rubble fade (module doc).
#[allow(clippy::too_many_arguments)]
fn fly_debris(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(
        Entity,
        &GlobalTransform,
        &mut Debris,
        Option<&gd_render::model::Skeleton>,
    )>,
    mut joints: Query<&mut GlobalTransform, Without<Debris>>,
    mut parts: Query<&mut MeshMaterial3d<GdMaterial>>,
    mut materials: ResMut<Assets<GdMaterial>>,
    coll: Res<crate::collision::WorldCollision>,
    nav: Res<gd_game::nav::Nav>,
    mut tris: Local<Vec<drop_body::Tri>>,
) {
    let dt = time.delta_secs();
    for (e, root, mut d, skel) in &mut q {
        let d = &mut *d;
        d.age += dt;
        if d.age >= d.life + d.fade {
            commands.entity(e).try_despawn();
            continue;
        }
        let Some(skel) = skel else { continue };
        if d.pieces.is_empty() {
            d.pieces = d.launch(&skel.msh, root);
            debug!("debris: {} pieces, mode {}", d.pieces.len(), d.mode);
        }
        let steps = d.clock.steps(dt);
        for p in &mut d.pieces {
            let Some(body) = p.body.as_mut() else {
                continue;
            };
            if steps == 0 || p.still >= SLEEP_TIME {
                continue;
            }
            // The world near the box and where this frame's steps can take it (`gd_game::loot::toss_flight`).
            let span = steps as f32 * drop_body::STEP;
            let (lo, hi) = body.aabb();
            let reach = Vec3::splat(drop_body::CONTACT_THRESHOLD + 0.1)
                + Vec3::from(body.vel).abs() * span
                + Vec3::Y * (drop_body::GRAVITY * span * span);
            let (lo, hi) = (Vec3::from(lo) - reach, Vec3::from(hi) + reach);
            tris.clear();
            coll.triangles_near(lo, hi, &mut tris);
            tris.sort_by_key(|t| t.owner);
            // No collision loaded there: a floor at the nav ground under the box (as `gd_game::ragdoll::floor`).
            let c = Vec3::from(body.pos);
            if tris.is_empty()
                && let Some(y) = nav.0.ground([c.x, c.y + 0.5, c.z])
            {
                let v = |x: f32, z: f32| [x, y, z];
                let owner = u64::MAX;
                tris.push(drop_body::Tri {
                    v: [v(lo.x, lo.z), v(hi.x, lo.z), v(hi.x, hi.z)],
                    owner,
                });
                tris.push(drop_body::Tri {
                    v: [v(lo.x, lo.z), v(hi.x, hi.z), v(lo.x, hi.z)],
                    owner,
                });
            }
            for _ in 0..steps {
                let (vel, spin) = (Vec3::from(body.vel), Vec3::from(body.spin));
                if body.step(&tris) && !p.landed {
                    p.landed = true;
                    // `Destructible::CollisionCallback`: a piece's contact (at most every
                    // 250 ms) disturbs the grass and water there, radius 1, strength √(|v|² + |ω|²), falloff,
                    // splash 2 on water (surface 7). PORT: the first contact is the piece's one landing.
                    let at = Vec3::from(body.pos);
                    let surface = coll.surface_below(at);
                    let water = surface == gd_sim::surface::Surface::Water as u32;
                    commands.write_message(gd_game_core::visuals::PointDisturbance {
                        pos: at,
                        radius: 1.0,
                        strength: (vel.length_squared() + spin.length_squared()).sqrt(),
                        falloff: true,
                        splash: if water { 2 } else { 0 },
                    });
                    if let Some(pak) = fall_pak(surface, d.impact.as_deref(), d.splash.as_deref()) {
                        commands.write_message(gd_game_core::visuals::PlaySound {
                            pak: pak.to_owned(),
                            at: Some(root.translation()),
                        });
                    }
                }
                let still = Vec3::from(body.vel).length() < SLEEP_SPEED
                    && Vec3::from(body.spin).length() < SLEEP_SPIN;
                p.still = if still {
                    p.still + drop_body::STEP
                } else {
                    0.0
                };
                if p.still >= SLEEP_TIME {
                    break;
                }
            }
        }
        // The bones follow their bodies: turned about the body's centre at the break, then moved with it.
        let root_m = root.affine();
        for (i, (p, b)) in d.pieces.iter().zip(&skel.msh.bones).enumerate() {
            let moved = p.body.as_ref().map_or(Affine3A::IDENTITY, |body| {
                Affine3A::from_rotation_translation(
                    Quat::from_array(body.rot) * p.rot0.inverse(),
                    Vec3::from(body.pos),
                ) * Affine3A::from_translation(-p.pos0)
            });
            if let Some(&j) = skel.joints.get(i)
                && let Ok(mut gt) = joints.get_mut(j)
            {
                *gt = GlobalTransform::from(
                    moved * root_m * gd_render::model::coords_to_affine(&b.abs),
                );
            }
        }
        // The rubble fade (module doc): one factor for the whole mesh, on per-instance material copies of the
        // `bonefadeskinned` ones.
        if d.age > d.life {
            let factor = (1.0 - (d.age - d.life) / d.fade.max(0.01)).max(0.0);
            if d.faded.is_none() {
                d.faded = crate::fade::begin_fade(
                    d.parts.iter().copied(),
                    FLAG_BONE_FADE,
                    true,
                    &mut parts,
                    &mut materials,
                );
            }
            crate::fade::set_fade(d.faded.as_deref().unwrap_or(&[]), factor, &mut materials);
        }
    }
}

/// The broken pieces and break effects of a destructible record at `tf`; `push` is `BreakApart`'s direction
/// and force ([`Broke`], React and Fall modes). The break sound and effects play now;
/// the broken mesh follows once its model is built ([`PendingDebris`]).
fn spawn_debris(
    commands: &mut Commands,
    assets: &mut CharAssets,
    rec: &gd_formats::arz::Record,
    tf: Transform,
    push: (Vec3, f32),
) {
    if let Some(mesh) = rec.str("brokenMesh") {
        match assets.model_ready(mesh) {
            Ready::Done(model) => spawn_broken(commands, rec, &model, tf, push),
            Ready::Loading => {
                commands.spawn(PendingDebris {
                    record: rec.name.clone(),
                    tf,
                    push,
                });
            }
            Ready::Failed => {}
        }
    }
    // `Destructible::BreakApart`: `breakSound` once at the break
    // position, after the break effects. PORT: the looping `intactSound` (no stock record sets it) is not played.
    if let Some(pak) = rec.str("breakSound").filter(|p| !p.is_empty()) {
        commands.write_message(gd_game_core::visuals::PlaySound {
            pak: pak.to_owned(),
            at: Some(tf.translation),
        });
    }
    for fx in rec.strs("breakEffects") {
        gd_render::fx::spawn_effect_record(
            commands,
            fx,
            Transform::from_translation(tf.translation + Vec3::Y * 0.8),
        );
    }
}

/// A broken mesh whose model is still loading in the background (`CharAssets::model_ready`): [`pending_debris`]
/// spawns it once built.
#[derive(Component)]
struct PendingDebris {
    record: String,
    tf: Transform,
    push: (Vec3, f32),
}

/// Spawns the waiting broken meshes whose model is built ([`PendingDebris`]); one that cannot load is dropped.
fn pending_debris(
    mut commands: Commands,
    pending: Query<(Entity, &PendingDebris)>,
    mut a: CharAssets,
) {
    for (e, p) in &pending {
        let db = a.data.db.clone();
        if let Some(rec) = db.get(&p.record)
            && let Some(mesh) = rec.str("brokenMesh")
        {
            match a.model_ready(mesh) {
                Ready::Loading => continue,
                Ready::Done(model) => spawn_broken(&mut commands, rec, &model, p.tf, p.push),
                Ready::Failed => {}
            }
        }
        commands.entity(e).despawn();
    }
}

/// The broken mesh `model` of a destructible record at `tf` and its pieces' setup ([`Debris`]).
fn spawn_broken(
    commands: &mut Commands,
    rec: &gd_formats::arz::Record,
    model: &Arc<gd_render::model::ModelAsset>,
    tf: Transform,
    (dir, push): (Vec3, f32),
) {
    let mode = match rec.str("breakMode").unwrap_or("") {
        "React" => 0,
        "Explode" => 2,
        "Fall" => 3,
        _ => 1,
    };
    // React scales both rolls by `BreakApart`'s force (module doc).
    let scale = if mode == 0 { push } else { 1.0 };
    let force = (
        rec.f32("breakForceMin").unwrap_or(0.0) * FORCE_SCALE * scale,
        rec.f32("breakForceMax").unwrap_or(0.0) * FORCE_SCALE * scale,
    );
    let dir = match mode {
        0 => dir,
        // `WorldCoords::TransformVector` of the stored direction (module doc).
        3 => (tf.rotation * dir).normalize_or(Vec3::Y),
        _ => Vec3::Y,
    };
    // `PickStatics` (module doc).
    let mut statics: Vec<String> = rec
        .strs("fixedStatics")
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut random = rec.strs("randomStatics");
    let (lo, hi) = (
        rec.i32("minNumRandomStatics").unwrap_or(0),
        rec.i32("maxNumRandomStatics").unwrap_or(0),
    );
    let n = if (hi.wrapping_sub(lo) as u32) > 0x7FFD {
        lo
    } else {
        lo + gd_game_core::rng::i32(0..=hi - lo)
    };
    let n = usize::try_from(n).unwrap_or(0);
    if n < random.len() {
        gd_game_core::rng::shuffle(&mut random);
    }
    statics.extend(random.into_iter().take(n).map(str::to_owned));
    let ne = commands
        .spawn((
            tf,
            Visibility::default(),
            gd_render::model::ExternalPose,
            Name::new("broken"),
        ))
        .id();
    let parts = gd_render::model::spawn_model_with(commands, ne, model, |_, m| m.clone());
    let debris = Debris {
        impact: rec
            .str("impactSound")
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        splash: rec
            .str("splashSound")
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        pieces: Vec::new(),
        clock: drop_body::Clock::default(),
        age: 0.0,
        life: rec.f32("rubbleLife").unwrap_or(2.0),
        fade: rec.f32("rubbleFade").unwrap_or(1.0),
        force,
        mass: rec.f32("physicsMass").filter(|m| *m > 0.0).unwrap_or(1.0),
        friction: rec.f32("physicsFriction").unwrap_or(0.0),
        mode,
        dir,
        statics,
        parts: parts.clone(),
        faded: None,
    };
    commands.entity(ne).try_insert(debris);
    // The broken pieces cast sun shadows only with the record's `brokenCastShadows` (default 0):
    // `Destructible::PhysicsSetup` / `ReplicateBreakApart` (Game.dll) set `Actor::SetCastsShadows` from it
    // once there are pieces (render/shadow.rs: casters need `Actor.castsShadow`).
    if rec.i32("brokenCastShadows").unwrap_or(0) == 0 {
        for p in parts {
            commands.entity(p).try_insert(bevy::light::NotShadowCaster);
        }
    }
}

/// `GD_DEBRIS_TEST` (hooks.rs): the record's debris 2 m in
/// front of the player every 3 s.
#[cfg(feature = "dev")]
fn test_debris(
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    time: Res<Time>,
    mut timer: Local<f32>,
    players: Query<&Transform, With<gd_game_core::player::Player>>,
    mut assets: CharAssets,
) {
    let Some(rec) = hooks.gameplay.debris_test.clone() else {
        return;
    };
    let Ok(p) = players.single() else { return };
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 3.0;
    let db = assets.data.db.clone();
    if let Some(r) = db.get(&rec) {
        spawn_debris(
            &mut commands,
            &mut assets,
            r,
            Transform::from_translation(p.translation + Vec3::new(2.0, 0.0, 0.0)),
            // `TakeAttack`'s push: from the player to the object, force 1 (module doc).
            (Vec3::X, 1.0),
        );
    }
}
