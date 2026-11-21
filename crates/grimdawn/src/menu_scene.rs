//! Main-menu scene: the menu record's map drawn behind the main menu, seen
//! from the record's fixed camera, with the selected character standing in it
//! (`crate::doll`, [`DollView::Menu`](crate::doll::DollView)).
//!
//! Grim Dawn.exe:
//! - the record (sub_1400BB310): one of the menu's record list (MainMenu ctor
//!   sub_1400B9B30: `records/game/gameengine.dbr` `menuRecord`, then
//!   `menuRecordExpansion1..3` of each loaded expansion when the field is set;
//!   `database/templates/gamemenu.tpl`), picked by the menu-scene option
//!   (`mainMenu`, `Options` int 12, at most the list's last). Before the pick,
//!   once (`gdx3FirstRun`, bool 74, with the third expansion enabled): the
//!   option is set to 3 and the options are saved. A record other than the
//!   loaded one unloads the scene (world, ambience, weather, jukebox) and
//!   loads the new one; the options' Apply calls it too (sub_1400CD780).
//!   Its camera: eye `mmCameraPosition`, looking at `mmCameraTarget` with up
//!   +Y (sub_1402A2F50), `mmCameraFOV` (degrees; vertical, as every
//!   `GAME::Camera`), `mmCameraNearPlane` / `mmCameraFarPlane`; fixed (no path
//!   or orbit). The character: `mmPlayerPosition`, `mmPlayerRotation`
//!   (degrees), `mmPlayerScale`; `mmTime`, `mmMusic`, `mmVignetteBitmap`,
//!   `mmLogo`.
//! - loading (sub_1400BBAD0): `AmbianceManager::SetTime(mmTime)`,
//!   `SetTimeEnabled(false)`, `SetDepthFogClamp(0)`, the jukebox starts the
//!   ambience and the music, `World::Load(mmMap)` and region 0 is loaded.
//! - the selected character (sub_1400D95A0): its `Player` entity is added to
//!   the menu region at `mmPlayerPosition` with `Actor::SetScale(mmPlayerScale)`
//!   (Engine.dll: replaces the record's `scale`), `Player::RemoveLight` and
//!   `Character::SetMenuCharacter`; sub_1400D9ED0 sets its rotation
//!   R(X, x) · R(Y, y + turn) · R(Z, z) (row vectors; `turn` is a menu
//!   rotation, 0 here). Its controller state is `ControllerPlayerStateMenuIdle`
//!   → `MenuIdleAction` (Game.dll; shares `LongIdleAction`'s transition timer).
//! - an `AmbientShadow` with `fx/textures/ambientshadowmainmenu.tex` under it
//!   (sub_1400BEAD0 `SetTexture`; drawn by sub_1400BF650 with
//!   `AddToScene(coords, 1.0, 1.0)` 0.25 m below the scene's point at
//!   +976, unrotated). Game.dll `AmbientShadow::RenderPass` @0x18001d2e0 /
//!   `AddToScene` @0x18001d120: a horizontal quad `size` (1 m) to each side of
//!   the point, white with alpha `scale` × 255, drawn with
//!   `shaders/effects/ambientshadow.ssh` `emissiveStyle` (sort 6000; alpha
//!   blend, depth test LessEqual without depth write, depth bias −0.001;
//!   [`ambient_shadow`]).
//!
//! The scene is the map's levels built by the world streamer's own code
//! (`world::build_level` on a background task, `world::spawn_level` without
//! gameplay content: no proxies, chests, breakables, nav or collision); it is
//! despawned when the menu closes, before the session's world streams
//! (`SessionReady`). The environment (`env.rs`) reads the menu map's sector
//! data, `mmTime` and a fog clamp of 0 while it exists.
//!
//! The scenery never fades in the menu (see_through.rs: only
//! `GameCamera::UpdateTransparencyFrustum` marks it; the menu's camera is a
//! plain `Camera` with `SetCoords`): with the player-centred frustum the
//! character's round stone platform (`allowTransparency`, between eye and
//! target) faded out and left one stray paving block, a dark square, under
//! the character's feet.
//!
//! PORT: the three expansions are always loaded (`gd_formats::vfs`), so the
//! list is the four records; the first-run pick is not saved under
//! `GD_OPTIONS` (the port's own file is not the one read); the menu idle is
//! the doll's looped long idle; the menu ambience (`StartAmbient`) and the
//! scene's two breakable containers are not shown; the music plays at the
//! area music's volume (0.6).
use crate::data::GameData;
use crate::world::{LevelCpu, Levels};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future};
use gd_formats::map::MapFile;
use gd_formats::options::Value;
use gd_game::options::GameOptions;
use gd_render::camera::GameCamera;
use gd_render::render::backdrop::MenuBackdrop;
use std::sync::Arc;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                open_scene
                    .run_if(resource_exists::<crate::menu::MainMenu>)
                    .run_if(
                        |scene: Option<Res<MenuScene>>, options: Option<Res<GameOptions>>| {
                            scene.is_none() || options.is_some_and(|o| o.is_changed())
                        },
                    ),
                finish_scene.run_if(resource_exists::<MenuScene>),
                close_scene
                    .run_if(resource_exists::<MenuScene>)
                    .run_if(not(resource_exists::<crate::menu::MainMenu>)),
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            menu_camera
                .after(gd_render::camera::CameraSet)
                .before(TransformSystems::Propagate)
                .run_if(resource_exists::<MenuScene>),
        );
    }
}

/// A `gamemenu.tpl` record (module doc).
#[derive(Clone, Debug)]
pub struct MenuView {
    /// The record's place in the menu's record list (module doc).
    pub index: usize,
    pub map: String,
    pub eye: Vec3,
    pub target: Vec3,
    /// Vertical, radians.
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    /// The character's place: `mmPlayerPosition`, rotation, `mmPlayerScale`.
    pub player: Transform,
    pub music: Option<String>,
    /// `mmVignetteBitmap` and `mmLogo` (drawn by `ui/mainmenu.rs`).
    pub vignette: Option<String>,
    pub logo: Option<String>,
}

/// The menu record `option` (the `mainMenu` option) picks (module doc).
pub fn menu_view(
    data: &GameData,
    hooks: &gd_game::hooks::Hooks,
    option: i32,
) -> Option<(MenuView, f32)> {
    let eng = data.db.get("records/game/gameengine.dbr")?;
    let records: Vec<&str> = [
        "menuRecord",
        "menuRecordExpansion1",
        "menuRecordExpansion2",
        "menuRecordExpansion3",
    ]
    .iter()
    .filter_map(|k| eng.str(k).filter(|r| !r.is_empty()))
    .collect();
    let index = (option.max(0) as usize).min(records.len().checked_sub(1)?);
    let rec = data.db.get(records[index])?;
    let name = |k: &str| rec.str(k).filter(|m| !m.is_empty()).map(str::to_owned);
    let v = |p: &str| {
        Vec3::new(
            rec.f32(&format!("{p}.X")).unwrap_or(0.0),
            rec.f32(&format!("{p}.Y")).unwrap_or(0.0),
            rec.f32(&format!("{p}.Z")).unwrap_or(0.0),
        )
    };
    let r = v("mmPlayerRotation") * (std::f32::consts::PI / 180.0);
    // Row-vector R(X)·R(Y)·R(Z) = column-vector R(Z)·R(Y)·R(X).
    let rot = Quat::from_euler(EulerRot::ZYX, r.z, r.y, r.x);
    let time = hooks.render.time.or(rec.f32("mmTime")).unwrap_or(12.0);
    Some((
        MenuView {
            index,
            map: rec.str("mmMap")?.to_owned(),
            eye: v("mmCameraPosition"),
            target: v("mmCameraTarget"),
            fov: rec.f32("mmCameraFOV").unwrap_or(30.0).to_radians(),
            near: rec.f32("mmCameraNearPlane").unwrap_or(1.0),
            far: rec.f32("mmCameraFarPlane").unwrap_or(300.0),
            player: Transform::from_translation(v("mmPlayerPosition"))
                .with_rotation(rot)
                .with_scale(Vec3::splat(rec.f32("mmPlayerScale").unwrap_or(1.0))),
            music: name("mmMusic"),
            vignette: name("mmVignetteBitmap"),
            logo: name("mmLogo"),
        },
        time,
    ))
}

type Built = anyhow::Result<(Arc<MapFile>, Vec<(usize, LevelCpu)>)>;

/// The menu scene while the main menu is open.
#[derive(Resource)]
pub struct MenuScene {
    pub view: MenuView,
    task: Option<Task<Built>>,
    music: Option<Entity>,
}

/// An entity of the menu scene (level roots, the ambient shadow, the music): despawned with the scene.
#[derive(Component)]
struct MenuSceneEntity;

impl MenuScene {
    /// The menu music's entity (faded by the credits roll).
    pub fn music(&self) -> Option<Entity> {
        self.music
    }
}

/// Opens the picked record's scene, in place of another record's (module doc: sub_1400BB310).
#[allow(clippy::too_many_arguments)]
fn open_scene(
    mut commands: Commands,
    data: Res<GameData>,
    mut sounds: ResMut<crate::audio::Sounds>,
    layers: Res<gd_render::render::terrain::TerrainLayers>,
    grass: Res<gd_render::render::grass::GrassQuality>,
    hooks: Res<crate::hooks::Hooks>,
    mut options: Option<ResMut<GameOptions>>,
    scene: Option<Res<MenuScene>>,
    owned: Query<Entity, With<MenuSceneEntity>>,
) {
    // No options file: the engine's defaults, whose first run picks the third expansion's.
    let mut option = 3;
    if let Some(o) = options.as_mut() {
        if !o.flag("gdx3FirstRun") {
            o.set("mainMenu", Value::Int(3));
            o.set("gdx3FirstRun", Value::Bool(true));
            if hooks.session.options_file.is_none() {
                crate::settings::store(o);
            }
        }
        option = o.int("mainMenu");
    }
    let Some((view, time)) = menu_view(&data, &hooks, option) else {
        warn!("main menu: no menu record");
        return;
    };
    if let Some(old) = &scene {
        if old.view.index == view.index {
            return;
        }
        for e in &owned {
            commands.entity(e).try_despawn();
        }
    }
    info!(
        "main-menu scene {}: eye {} target {} fov {:.0}°",
        view.map,
        view.eye,
        view.target,
        view.fov.to_degrees()
    );
    let (d, res, layers) = (data.clone(), view.map.clone(), layers.clone());
    let high = grass.0;
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let map = Arc::new(MapFile::open_resource(&d.vfs, &res)?);
        let mut levels = Vec::new();
        for (i, r) in map.regions.iter().enumerate() {
            if r.level_size > 0 {
                levels.push((i, crate::world::build_level(&map, &d, &layers, high, i)?));
            }
        }
        Ok((map, levels))
    });
    let music = view
        .music
        .as_deref()
        .and_then(|m| sounds.file(&data, m))
        .map(|f| {
            use crate::audio::{ENGINE_HZ, SoundClass};
            let speed = f.speed(ENGINE_HZ);
            let e = crate::audio::spawn_2d(&mut commands, SoundClass::Music, 0.6, speed, f, true);
            commands
                .entity(e)
                .insert((Name::new("menu music"), MenuSceneEntity));
            e
        });
    let cam_distance = view.eye.distance(view.player.translation);
    commands.insert_resource(MenuScene {
        view,
        task: Some(task),
        music,
    });
    commands.insert_resource(MenuBackdrop {
        map: None,
        levels: Levels::new(Vec::new()),
        time,
        cam_distance,
    });
}

#[allow(clippy::too_many_arguments)]
fn finish_scene(
    mut scene: ResMut<MenuScene>,
    mut commands: Commands,
    data: Res<GameData>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut terrain_mats: ResMut<Assets<crate::world::TerrainMaterial>>,
    mut decal_mats: ResMut<Assets<crate::world::DecalMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    mut mats: gd_render::render::material::MaterialAssets,
    mut cache: ResMut<gd_render::model::ModelCache>,
    mut bodies: ResMut<gd_game::creature::BodyCache>,
    mut textures: ResMut<gd_render::textures::Textures>,
    hooks: Res<crate::hooks::Hooks>,
    mut backdrop: ResMut<MenuBackdrop>,
) {
    let Some(task) = scene.task.as_mut() else {
        return;
    };
    let Some(r) = block_on(future::poll_once(task)) else {
        return;
    };
    scene.task = None;
    let (map, levels) = match r {
        Ok(v) => v,
        Err(e) => {
            warn!("main-menu scene: {e:#}");
            return;
        }
    };
    // Nothing walks or shoots in the scene: its collision is dropped.
    let mut coll = crate::collision::WorldCollision::default();
    let mut receivers = gd_render::decal::DecalReceivers::default();
    for (i, mut cpu) in levels {
        let (origin, sectors) = (cpu.origin(), cpu.take_sectors());
        let root = crate::world::spawn_level(
            cpu,
            false,
            &mut commands,
            &data,
            &mut meshes,
            &mut images,
            &mut terrain_mats,
            &mut decal_mats,
            &mut mats,
            &mut bindposes,
            &mut cache,
            &mut bodies,
            &mut textures,
            &mut coll,
            &mut receivers,
            &hooks.render,
        );
        commands
            .entity(root)
            .insert((Name::new(format!("menu level {i}")), MenuSceneEntity));
        backdrop.levels.add(i, root, origin, sectors);
    }
    if let Some(e) = ambient_shadow(
        &mut commands,
        &data,
        &scene.view,
        &mut meshes,
        &mut materials,
        &mut images,
        &mut textures,
    ) {
        commands.entity(e).insert(MenuSceneEntity);
    }
    backdrop.map = Some(map);
}

/// The character's `AmbientShadow` quad (module doc). PORT: 2 cm above
/// `mmPlayerPosition` (the source of the engine's point, 0.25 m above its
/// quad, was not traced; 0.25 m below `mmPlayerPosition` it is under the
/// platform and depth-tested away); the shader's depth bias is left out.
fn ambient_shadow(
    commands: &mut Commands,
    data: &GameData,
    view: &MenuView,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    textures: &mut gd_render::textures::Textures,
) -> Option<Entity> {
    let tex = textures.get(data, "fx/textures/ambientshadowmainmenu.tex", false, images)?;
    let mat = materials.add(StandardMaterial {
        base_color_texture: Some(tex),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        fog_enabled: false,
        ..default()
    });
    let mesh = meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(1.0)));
    let at = view.player.translation + Vec3::Y * 0.02;
    Some(
        commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(mat),
                Transform::from_translation(at),
                bevy::light::NotShadowCaster,
                Name::new("menu ambient shadow"),
            ))
            .id(),
    )
}

/// The menu closed (a session starts): the scene, its music and camera go.
fn close_scene(
    mut commands: Commands,
    owned: Query<Entity, With<MenuSceneEntity>>,
    mut cams: Query<&mut Projection, With<GameCamera>>,
) {
    for e in &owned {
        commands.entity(e).try_despawn();
    }
    for mut p in &mut cams {
        *p = Projection::Perspective(gd_render::camera::world_projection());
    }
    commands.remove_resource::<MenuScene>();
    commands.remove_resource::<MenuBackdrop>();
}

/// The record's fixed camera over the world camera's orbit.
fn menu_camera(
    scene: Res<MenuScene>,
    mut cams: Query<(&mut GameCamera, &mut Transform, &mut Projection)>,
) {
    let v = &scene.view;
    for (mut c, mut t, mut p) in &mut cams {
        // The environment picks its lights and sun around the target.
        c.target = v.target;
        *t = Transform::from_translation(v.eye).looking_at(v.target, Vec3::Y);
        if let Projection::Perspective(pp) = &mut *p
            && (pp.fov, pp.near, pp.far) != (v.fov, v.near, v.far)
        {
            (pp.fov, pp.near, pp.far) = (v.fov, v.near, v.far);
        }
    }
}
