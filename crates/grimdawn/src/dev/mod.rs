//! Dev tools, compiled only with the `dev` feature (`cargo dev`, docs/running.md): the `GD_*` hooks
//! ([`gd_game_core::hooks`], which exists only with `dev`) are read from the environment and these exist:
//! screenshots ([`autoshot`]), the session census ([`audit`]), exit cycles ([`exit_cycles`]), the memory audit
//! ([`memory`]), the test arena ([`testworld`]), the character scene ([`chars`]), the effect checks ([`fx`],
//! registered here unless `GD_NO_FX`), the model viewer (`GD_MODELS`), the frame-time log (`GD_FPS`), the per-pass
//! GPU times (`GD_GPU_TIMES`), the path test (`GD_PATH_TEST`), the scripted mouse ([`mouse`], `GD_MOUSE`) and the
//! fixed pointer (`GD_HOVER`, [`hover_pointer`]). A build without `dev` has none of them, no hooks and no hook
//! read: the scripted test systems next to the systems they drive are gated the same way.
pub mod audit;
pub mod autoshot;
pub mod chars;
pub mod exit_cycles;
pub mod fx;
pub mod memory;
pub mod mouse;
pub mod testworld;

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use gd_game_core::data::GameData;
use gd_game_core::hooks::Hooks;
use gd_render::model::{Animator, ModelCache};
use gd_render::textures::Textures;

pub struct DevPlugin;

impl Plugin for DevPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            autoshot::AutoShotPlugin,
            audit::AuditPlugin,
            testworld::TestWorldPlugin,
        ))
        .add_systems(Startup, models)
        .add_systems(
            Update,
            (
                fps_log.run_if(|hooks: Res<Hooks>| hooks.render.fps),
                path_test.run_if(|hooks: Res<Hooks>| hooks.gameplay.path_test.is_some()),
            ),
        );
        chars::plugin(app);
        exit_cycles::plugin(app);
        memory::plugin(app);
        mouse::plugin(app);
        // The effect checks (`GD_FX` / `GD_FPS`): only with effects on (`GD_NO_FX` leaves them out).
        if !app.world().resource::<Hooks>().render.no_fx {
            fx::plugin(app);
        }
        if app.world().resource::<Hooks>().render.gpu_times {
            app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin)
                .add_systems(Last, gpu_times_log);
        }
        // After the `GD_MOUSE` script, so a set `GD_HOVER` always wins the pointer.
        if app.world().resource::<Hooks>().ui.hover_px.is_some() {
            app.add_systems(First, hover_pointer.after(mouse::scripted_mouse));
        }
    }
}

/// `GD_HOVER=x,y` (logical px): the window's cursor stands there every frame, set before the input systems
/// (`First`, as [`mouse`]), so every pointer read (`gd_ui_kit::cursor_px`, the world hover) sees it as the OS
/// pointer. Only a change is written: bevy_winit then tries to warp the OS pointer once (Wayland refuses and it logs
/// one `could not set cursor position` error; the window's position stands either way).
fn hover_pointer(
    hooks: Res<Hooks>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    let Some(p) = hooks.ui.hover_px else { return };
    for mut win in &mut windows {
        if win.cursor_position() != Some(p) {
            win.set_cursor_position(Some(p));
        }
    }
}

/// `GD_GPU_TIMES`: every second, the passes' GPU times (timestamp queries; Bevy's passes and the port's post
/// passes), largest first, and their sum.
fn gpu_times_log(
    time: Res<Time>,
    mut since: Local<f32>,
    store: Res<bevy::diagnostic::DiagnosticsStore>,
) {
    *since += time.delta_secs();
    if *since < 1.0 {
        return;
    }
    *since = 0.0;
    let mut spans: Vec<(f64, &str)> = store
        .iter()
        .filter_map(|d| {
            let path = d.path().as_str().strip_suffix("/elapsed_gpu")?;
            Some((d.average()?, path.strip_prefix("render/").unwrap_or(path)))
        })
        .collect();
    spans.sort_by(|a, b| b.0.total_cmp(&a.0));
    let total: f64 = spans.iter().map(|s| s.0).sum();
    let list: Vec<String> = spans
        .iter()
        .take(14)
        .map(|(ms, p)| format!("{p} {ms:.2}"))
        .collect();
    info!("gpu ms (sum {total:.2}): {}", list.join(", "));
}

/// A dev scene (`GD_MODELS`, `GD_CHARS`) placed the camera: `main`'s `place_camera` leaves it.
#[derive(Resource)]
pub struct SceneCamera;

/// Model viewer: `GD_MODELS` (hooks.rs) in a row on a ground plane.
#[allow(clippy::too_many_arguments)]
fn models(
    mut commands: Commands,
    data: Res<GameData>,
    mut cache: ResMut<ModelCache>,
    mut bodies: ResMut<gd_game_core::creature::BodyCache>,
    mut textures: ResMut<Textures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut mats: gd_render::render::material::MaterialAssets,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut images: ResMut<Assets<Image>>,
    mut cams: Query<&mut gd_render::camera::GameCamera>,
    hooks: Res<Hooks>,
) {
    let Some(list) = &hooks.render.models else {
        return;
    };
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(60.0, 60.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.32, 0.25))),
    ));
    for (i, spec) in list.split(';').filter(|s| !s.is_empty()).enumerate() {
        let (msh, anm) = spec
            .split_once('@')
            .map_or((spec, None), |(a, b)| (a, Some(b)));
        let Some(model) = cache.model(
            &data,
            &mut bodies,
            msh,
            &mut meshes,
            &mut mats,
            &mut bindposes,
            &mut textures,
            &mut images,
        ) else {
            continue;
        };
        let root = commands
            .spawn((
                Transform::from_xyz(i as f32 * 4.0 - 4.0, 0.0, 0.0),
                Visibility::default(),
                Name::new(msh.to_owned()),
            ))
            .id();
        gd_render::model::spawn_model(&mut commands, root, &model);
        if let Some(a) = anm.and_then(|a| cache.anim(&data, a)) {
            commands.entity(root).insert(Animator {
                anim: a,
                time: 0.0,
                speed: 1.0,
                looped: true,
            });
        }
    }
    for mut c in &mut cams {
        c.distance = 14.0;
        c.target = Vec3::new(0.0, 1.5, 0.0);
    }
    commands.insert_resource(SceneCamera);
}

/// `GD_FPS`: logs the average / worst frame time every second, with the entity and animated-character
/// counts, the loaded level count and the player's position (walk profiling).
fn fps_log(
    time: Res<Time>,
    mut acc: Local<(f32, f32, u32)>,
    all: Query<Entity>,
    chars: Query<(), With<gd_game_core::character::Character>>,
    levels: Res<gd_game_core::levels::Levels>,
    player: Query<&Transform, With<gd_game_core::player::Player>>,
) {
    let dt = time.delta_secs();
    acc.0 += dt;
    acc.1 = acc.1.max(dt);
    acc.2 += 1;
    if acc.0 >= 1.0 {
        let p = player.single().map_or(Vec3::NAN, |t| t.translation);
        info!(
            "frames {} avg {:.1} ms worst {:.1} ms; {} entities, {} characters; {} levels, player {:.0},{:.0}",
            acc.2,
            acc.0 * 1000.0 / acc.2 as f32,
            acc.1 * 1000.0,
            all.iter().len(),
            chars.iter().len(),
            levels.loaded.len(),
            p.x,
            p.z
        );
        *acc = (0.0, 0.0, 0);
    }
}

/// `GD_PATH_TEST` (hooks.rs): every 5 s logs the nav obstacle count and whether the player can path to (x, z).
fn path_test(
    hooks: Res<Hooks>,
    nav: Res<gd_game::nav::Nav>,
    time: Res<Time>,
    mut t: Local<f32>,
    players: Query<&Transform, With<gd_game_core::player::Player>>,
    q: Query<&gd_game::nav::NavObstacle>,
    mut scratch: Local<gd_sim::nav::PathScratch>,
) {
    let Some(Vec2 { x, y: z }) = hooks.gameplay.path_test else {
        return;
    };
    *t -= time.delta_secs();
    let Ok(p) = players.single() else { return };
    if *t > 0.0 {
        return;
    }
    *t = 5.0;
    let goal = Vec3::new(x, p.translation.y, z);
    let t0 = std::time::Instant::now();
    let path = nav.0.find_path(
        &mut scratch,
        p.translation.to_array(),
        goal.to_array(),
        gd_game::nav::PATH_BUDGET_LONG,
    );
    let took = t0.elapsed();
    info!(
        "path test ({took:?}): {} obstacles; {:?} -> ({x}, {z}): {}",
        q.iter().map(|o| o.0.len()).sum::<usize>(),
        p.translation,
        match &path {
            Some(v) => format!("{} points, ends {:?}", v.len(), v.last()),
            None => "no path".into(),
        }
    );
}
