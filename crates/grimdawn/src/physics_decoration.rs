//! The dressing of `gd_game::physics_decoration`: an intact prop's mesh (the
//! record's, as `Actor::Load` sets it up, without shadows: the engine ctor's
//! `SetCastsShadows(false)`), and a tossed prop's mesh, which tumbles from the
//! hit's impulse (`PropTossed`'s velocity and spin) under the physics
//! world's gravity (`gd_sim::drop_body::GRAVITY`) onto the ragdolls' floor
//! (`gd_game::ragdoll::floor`), lies for the engine's 5 s
//! (`PhysicsDecoration::UpdateSelf`'s 5000 ms), then fades over 3 s and goes.
//!
//! The fade is the engine's `BeginFade(255, 255, 255, 3 s)` instance opacity:
//! the port draws it through the blended-fade material flag (`base.a · fade.x`,
//! `render/material.rs` `FLAG_BLENDED_FADE`, SrcAlpha / OneMinusSrcAlpha), a
//! per-instance copy of each part's material made once when the fade starts —
//! the static scenery shaders have no T variant, so this stands in for the
//! engine's transparency-list draw (as the gibs' fade does).
//!
//! PORT: the tossed mesh is a point with a spin (no box contacts): it stops
//! falling on the floor, slides with the gibs' friction and stops below
//! 0.3 m/s.
use bevy::prelude::*;
pub use gd_game::physics_decoration::*;
use gd_render::character::CharAssets;
use gd_render::render::material::{FLAG_BLENDED_FADE, GdMaterial};

pub struct PhysicsDecorationViewPlugin;

impl Plugin for PhysicsDecorationViewPlugin {
    fn build(&self, app: &mut App) {
        app.register_required_components::<PhysicsDecoration, Visibility>()
            .add_systems(
                Update,
                (
                    dress_props,
                    toss_props.after(PhysicsDecorationStage::Toss),
                    move_props,
                )
                    .chain(),
            );
    }
}

/// A tossed prop in flight / fading (the `gibs::GibBody` shape).
#[derive(Component)]
struct PropBody {
    vel: Vec3,
    spin: Vec3,
    /// 5 s of life, then −3..0 of fade (`UpdateSelf`).
    life: f32,
    pos: Vec3,
    rot: Quat,
    /// The model's part entities (their materials take the fade).
    parts: Vec<Entity>,
    /// The fade materials: per-instance copies made once when the fade starts;
    /// `Some([])` when the mesh has no parts.
    faded: Option<Vec<Handle<GdMaterial>>>,
}

/// An intact prop's mesh (module doc), once it is loaded (`CharAssets::record_ready`; loaded in the background, so a
/// streamed-in level's props never build their models on the game thread).
fn dress_props(
    mut commands: Commands,
    new: Query<(Entity, &PhysicsDecoration), Without<gd_render::character::RecordDressed>>,
    mut assets: CharAssets,
) {
    for (e, p) in &new {
        let db = assets.data.db.clone();
        let rec = db.get(&p.record);
        if rec.is_some_and(|r| !assets.record_ready(r)) {
            continue;
        }
        commands
            .entity(e)
            .try_insert(gd_render::character::RecordDressed);
        let Some(rec) = rec else {
            continue;
        };
        for part in assets
            .spawn_record_model(&mut commands, e, rec)
            .unwrap_or_default()
        {
            commands
                .entity(part)
                .try_insert(bevy::light::NotShadowCaster);
        }
    }
}

/// A [`PropTossed`] becomes a tumbling mesh, dressed synchronously from the
/// record (no frame of blank body).
fn toss_props(
    mut commands: Commands,
    mut tossed: MessageReader<PropTossed>,
    mut assets: CharAssets,
) {
    for t in tossed.read() {
        let db = assets.data.db.clone();
        let Some(rec) = db.get(&t.record) else {
            continue;
        };
        let e = commands
            .spawn((
                t.transform,
                Visibility::default(),
                Name::new("tossed physics prop"),
            ))
            .id();
        let parts = assets
            .spawn_record_model(&mut commands, e, rec)
            .unwrap_or_default();
        for p in &parts {
            commands.entity(*p).insert(bevy::light::NotShadowCaster);
        }
        commands.entity(e).insert(PropBody {
            vel: t.vel,
            spin: t.spin,
            life: 5.0,
            pos: t.transform.translation,
            rot: t.transform.rotation,
            parts,
            faded: None,
        });
    }
}

/// Gravity, the floor, friction, the 3 s fade, the removal (module doc; the
/// `gibs::move_gibs` motion).
#[allow(clippy::too_many_arguments)]
fn move_props(
    mut commands: Commands,
    time: Res<Time>,
    nav: Res<gd_game::nav::Nav>,
    coll: Res<crate::collision::WorldCollision>,
    mut q: Query<(Entity, &mut Transform, &mut PropBody)>,
    mut parts: Query<&mut MeshMaterial3d<GdMaterial>>,
    mut materials: ResMut<Assets<GdMaterial>>,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut p) in &mut q {
        p.life -= dt;
        if p.life <= -3.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        p.vel.y -= gd_sim::drop_body::GRAVITY * dt;
        let mut pos = p.pos + p.vel * dt;
        // Rising into a ceiling: stop there and fall back (as the gibs).
        if p.vel.y > 0.0
            && let Some((hit, _)) = coll.segment_hit(p.pos, pos)
        {
            pos = hit - (pos - p.pos).normalize_or_zero() * 0.05;
            p.vel.y = 0.0;
        }
        let (floor, _) = gd_game::ragdoll::floor(&coll, &nav, p.pos, pos);
        if pos.y < floor {
            pos.y = floor;
            // The world's restitution is 0: the floor stops the fall.
            if p.vel.y < 0.0 {
                p.vel.y = 0.0;
            }
            // Floor friction per contact in the engine; dt-scaled (0.7 per 1/60 s).
            let f = 0.7f32.powf(dt * 60.0);
            p.vel.x *= f;
            p.vel.z *= f;
            p.spin *= f;
            if p.vel.length() < 0.3 {
                p.vel = Vec3::ZERO;
                p.spin = Vec3::ZERO;
            }
        }
        p.pos = pos;
        p.rot =
            Quat::from_euler(EulerRot::XYZ, p.spin.x * dt, p.spin.y * dt, p.spin.z * dt) * p.rot;
        tf.translation = pos;
        tf.rotation = p.rot;
        if p.life < 0.0 {
            let factor = (1.0 + p.life / 3.0).max(0.0);
            if p.faded.is_none() {
                let mut copies = Vec::new();
                for part in &p.parts {
                    if let Ok(mat) = parts.get(*part)
                        && let Some(mut m) = materials.get(&mat.0).cloned()
                    {
                        m.params.flags.y |= FLAG_BLENDED_FADE;
                        m.states.blend = Some((6, 8));
                        m.params.fade.x = factor;
                        copies.push((*part, materials.add(m)));
                    }
                }
                for (part, h) in &copies {
                    if let Ok(mut mat) = parts.get_mut(*part) {
                        mat.0 = h.clone();
                    }
                }
                p.faded = Some(copies.into_iter().map(|(_, h)| h).collect());
            }
            for h in p.faded.as_deref().unwrap_or(&[]) {
                if materials.get(h).is_some_and(|m| m.params.fade.x != factor)
                    && let Some(mut m) = materials.get_mut(h)
                {
                    m.params.fade.x = factor;
                }
            }
        }
    }
}
