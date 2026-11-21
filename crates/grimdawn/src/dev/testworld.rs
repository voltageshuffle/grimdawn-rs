//! `GD_TESTWORLD` (hooks.rs): a flat test arena for checking skills and effects.
//!
//! No level is streamed; instead a 96 m square of walkable ground (grid
//! texture, a flat terrain heightfield for collision and decals) lies at [`CENTER`], far from every real level so no story actor
//! or prop appears there. The player starts in the middle (any `GD_START` is
//! ignored) and training dummies stand in front of them: three rows at 6,
//! 12 and 18 m, plus one ring of eight at 4 m. Dummies are hostile
//! monsters (`GD_DUMMY=<record>`, default `zombie_a01`) that never move or
//! attack (no AI runs on a [`Dummy`]) and refill their life every frame, so
//! skills always have targets at known distances. `GD_TESTWORLD=far` leaves
//! out the ring (casts aim at the nearest dummy, so they fly ≥ 6 m).
//! `GD_ARENA_MONSTERS` adds real monsters (AI, skills,
//! loot) 10 m to the player's right, for testing monster behaviour.
use crate::combat::{CombatRules, Combatant, MonsterAi};
use crate::session::PlayerStart;
use crate::world::Nav;
use bevy::prelude::*;
pub use gd_game::world::Dummy;

pub struct TestWorldPlugin;

/// Arena centre (world coordinates; the real map lies around 0..3000).
pub const CENTER: Vec3 = Vec3::new(6000.0, 0.0, 6000.0);
/// Half the arena's side (3 × 3 nav tiles of 32 m).
const HALF: f32 = 48.0;
const DEFAULT_DUMMY: &str = "records/creatures/enemies/zombie_a01.dbr";

impl Plugin for TestWorldPlugin {
    fn build(&self, app: &mut App) {
        if app
            .world()
            .resource::<crate::hooks::Hooks>()
            .session
            .testworld
            .is_none()
        {
            return;
        }
        app.insert_resource(PlayerStart(CENTER + Vec3::Y))
            .add_systems(Startup, setup)
            .add_systems(Update, (spawn_dummies, heal_dummies));
    }
}

fn setup(
    mut commands: Commands,
    mut nav: ResMut<Nav>,
    mut coll: ResMut<crate::collision::WorldCollision>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Walkable ground: flat nav layers, 0.5 m cells, every side connected.
    let t0 = ((CENTER.x - HALF) / gd_sim::nav::TILE).floor() as i32;
    let n = (2.0 * HALF / gd_sim::nav::TILE).ceil() as i32;
    let (cs, cells) = (0.5f32, (gd_sim::nav::TILE / 0.5) as u32);
    let mut layers = Vec::new();
    for tx in t0..t0 + n {
        for tz in t0..t0 + n {
            let count = (cells * cells) as usize;
            layers.push(gd_sim::nav::NavLayer {
                tile: (tx, tz),
                bmin: [
                    tx as f32 * gd_sim::nav::TILE,
                    CENTER.y,
                    tz as f32 * gd_sim::nav::TILE,
                ],
                cs,
                ch: 0.1,
                w: cells,
                h: cells,
                heights: vec![0; count],
                areas: vec![1; count],
                cons: vec![0x0f; count],
            });
        }
    }
    nav.0.add(u64::MAX - 7, layers);
    let side = n as f32 * gd_sim::nav::TILE;
    let origin = Vec3::new(
        t0 as f32 * gd_sim::nav::TILE,
        CENTER.y,
        t0 as f32 * gd_sim::nav::TILE,
    );
    // A flat terrain heightfield under it (terrain-only decals, ground hits).
    let verts = side as u32 + 1;
    coll.add_level(
        usize::MAX - 7,
        origin,
        Some((verts, verts, vec![0.0; (verts * verts) as usize])),
        std::iter::empty(),
    );
    // A 1 m grid (lines every metre, stronger every 4 m) to read distances.
    let px = 64u32;
    let mut data = vec![0u8; (px * px * 4) as usize];
    for y in 0..px {
        for x in 0..px {
            let line = x % 16 == 0 || y % 16 == 0;
            let major = x == 0 || y == 0;
            let v = if major {
                150
            } else if line {
                110
            } else {
                82
            };
            let i = ((y * px + x) * 4) as usize;
            data[i..i + 4].copy_from_slice(&[v, v, (v as f32 * 0.95) as u8, 255]);
        }
    }
    let mut img = Image::new(
        bevy::render::render_resource::Extent3d {
            width: px,
            height: px,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        data,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    img.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        address_mode_u: bevy::image::ImageAddressMode::Repeat,
        address_mode_v: bevy::image::ImageAddressMode::Repeat,
        ..default()
    });
    // UVs: one texture repeat per 4 m.
    let mut mesh = Plane3d::default().mesh().size(side, side).build();
    if let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
    {
        for u in uv.iter_mut() {
            u[0] *= side / 4.0;
            u[1] *= side / 4.0;
        }
    }
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(images.add(img)),
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            ..default()
        })),
        Transform::from_translation(origin + Vec3::new(side * 0.5, 0.0, side * 0.5)),
        Name::new("test arena"),
    ));
    info!("test world: {side} m arena at {CENTER:?}");
}

/// Dummy positions: a ring of eight at 4 m, then rows of five at 6, 12 and
/// 18 m towards -z (the camera looks that way), 2.5 m apart.
fn dummy_spots(far: bool) -> Vec<Vec3> {
    let mut v: Vec<Vec3> = (0..8)
        .map(|i| {
            let a = i as f32 * std::f32::consts::FRAC_PI_4;
            CENTER + Vec3::new(a.cos() * 4.0, 0.0, a.sin() * 4.0)
        })
        .filter(|p| p.z > CENTER.z - 1.0 && !far)
        .collect();
    for d in [6.0, 12.0, 18.0] {
        for k in -2..=2 {
            v.push(CENTER + Vec3::new(k as f32 * 2.5, 0.0, -d));
        }
    }
    v
}

fn spawn_dummies(
    hooks: Res<crate::hooks::Hooks>,
    mut commands: Commands,

    mut done: Local<bool>,
    rules: Option<Res<CombatRules>>,
    players: Query<(), With<crate::player::Player>>,
    mut assets: gd_game::creature::Spawner,
) {
    if *done || players.is_empty() {
        return;
    }
    let Some(rules) = rules else { return };
    *done = true;
    let record = hooks
        .session
        .dummy
        .clone()
        .unwrap_or_else(|| DEFAULT_DUMMY.into());
    let level = 54;
    let Some(profile) = gd_sim::actor::monster(
        &assets.data.db,
        &rules.0,
        &record,
        level,
        gd_sim::actor::Side::Monster,
    ) else {
        error!("test world: no monster profile for {record}");
        return;
    };
    let mut profile = profile;
    // Effectively unkillable (life is also refilled every frame).
    profile.sheet.life = 1.0e9;
    let profile = std::sync::Arc::new(profile);
    let radius = gd_game::combat::get_radius(assets.data.db.get(&record), 0.5);
    let spots = dummy_spots(hooks.session.testworld.as_deref() == Some("far"));
    for (i, p) in spots.iter().enumerate() {
        // Facing the centre: +Z forward (`character::facing`), `looking_to` aims −Z.
        let tf = Transform::from_translation(*p)
            .looking_to(*p - Vec3::new(CENTER.x, p.y, CENTER.z), Vec3::Y);
        let Some(e) = gd_game::creature::spawn(&mut commands, &mut assets, tf, &record, i as u32)
        else {
            continue;
        };
        // `MonsterAi` marks a hostile target for skills; the AI itself skips dummies.
        let mut ai = MonsterAi::from_record(&assets.data, &record, *p);
        ai.view = 0.0;
        commands.entity(e).insert((
            Combatant::new(profile.clone(), radius),
            ai,
            Dummy,
            Name::new(format!("dummy {i}")),
        ));
    }
    info!("test world: {} dummies ({record})", spots.len());
    let mut n = 0;
    for (rec, count) in &hooks.session.arena_monsters {
        for _ in 0..*count {
            let p = CENTER + Vec3::new(10.0 + (n % 3) as f32 * 1.5, 0.0, (n / 3) as f32 * 1.5);
            n += 1;
            let tf = Transform::from_translation(p);
            let Some(e) = gd_game::creature::spawn(&mut commands, &mut assets, tf, rec, 1000 + n)
            else {
                continue;
            };
            let Some(prof) = gd_sim::actor::monster(
                &assets.data.db,
                &rules.0,
                rec,
                level,
                gd_sim::actor::Side::Monster,
            ) else {
                continue;
            };
            let radius = gd_game::combat::get_radius(assets.data.db.get(rec), 0.5);
            commands.entity(e).insert((
                crate::monsters::Monster {
                    record: rec.to_owned(),
                    level,
                    slot: (usize::MAX, 0, n as usize),
                },
                Combatant::new(std::sync::Arc::new(prof), radius),
                MonsterAi::from_record(&assets.data, rec, p),
            ));
        }
    }
    if n > 0 {
        info!("test world: {n} arena monsters");
    }
}

fn heal_dummies(mut q: Query<(&mut Combatant, &mut MonsterAi), With<Dummy>>) {
    for (mut c, mut ai) in &mut q {
        c.life = c.profile.sheet.life;
        ai.aggro = false;
    }
}
