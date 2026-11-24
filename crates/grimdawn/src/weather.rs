//! Weather rendering: Engine.dll's `WeatherManager`, drawn from the rules in `gd_sim::weather` (record facts, the climate
//! pick and the spawn rules are documented there).
//!
//! Each frame ([`simulate_weather`]): the sector type 6 ("climate") cell under the camera focus names a climate set
//! record (`WeatherManager::Update` / `ChangeClimate`; the table is read once from
//! the map's sector data); `gd_sim::weather::Weather` picks / re-rolls / moves the particle systems. [`draw_weather`]
//! draws them, [`weather_flash`] makes the lightning flash a point light of the lighting environment (`render::env`
//! gathers it like any level light), [`weather_sounds`] loops the template `sound` and the manager's `windSound` as 2D
//! ambient sounds. The climate's ambience adjustments go into `render::env`'s ambiance
//! (`AmbianceManager::SetWeatherAdjustments`: the darkness comes off the sky and ground ambient
//! only; the sun colour and the shadows are untouched, so a storm (darkness 0.3 x intensity) keeps its sunlight; fog
//! density / colour added, the distance moves the depth fog start).
//!
//! Drawing (render functions `WeatherSystem::Render`, Rain `RainSystem::Render`, and the `weatherparticle*.ssh`
//! DirectX11
//! programs, `shaders/weather.wesl`): one quad per particle of side = the particle size (template `particleSize` x the
//! spawn phase: rain 0.25..0.75 m), white vertex colour with the alpha of [`alpha`], the texture as is. Rain and Snow
//! use the directional program (a square along the velocity, sideways = velocity x camera direction: no stretch by
//! speed); the others a vertical billboard facing the camera horizontally, turned by the spin. Mist, Fog and GroundFog
//! use the soft variants when `softParticles` is on (`GetShader`): the scene depth (copied after
//! the opaque passes, [`WeatherDepth`]) discards hidden fragments and fades the alpha over the last metre. Texture
//! flipbook frame 0 only (nothing in the weather code reads the frame count / rate; the engine shows frame 0
//! too). The programs carry their own pass states (`weatherparticle*.ssh`, also the soft ones): Combine
//! SrcAlpha / InvSrcAlpha, Additive SrcAlpha / One, no depth or alpha write, and sort 6200, after the effect particles
//! (combine 6000, additive 6100).
//!
//! Options (`WeatherManager::SetEnabled`): `weatherEnabled` off removes every system and the update returns at
//! once; `weatherQuality` (low / medium / high / very high = 0..3) sets the spawn delay factor 3 / 2 / 1 / 1
//! (`gd_sim::weather::quality_factor`), re-applied when it changes. `GD_WEATHER=<climate_weather record>` forces a
//! climate everywhere (test hook).
//!
//! The day / night filter reads `Ambiance::is_day` (`AmbianceManager::GetIsDay`, the area's own cycle). The main menu
//! scene weathers from its own map's climate sector layer and clock; the FoA menu map's only climate entry has an empty
//! record path, so it has no weather there. Quality 3 rain ends at the terrain (`RainSystem::SpawnParticle`: no splash
//! effect exists, the life is cut to the ground ray's hit against the physics world, [`Floor`]). Above quality 1 a drop
//! that dies asks for a water ripple where it ends (`RainSystem::OnParticleDeath`,
//! `Water::CreateRipple(pos, 0.5, 5.0)`;
//! `water.rs` places and draws them). At quality 2 and 3 (`advancedWind`) strong wind blows gusts
//! (`gd_sim::weather::Weather::winds`): each push may ripple the water there (`World::CreateDirectionalWind`, only at
//! strength >= 1) and bends the grass (`Terrain::CreateGrassWind`, render/grass.rs). The `windSound` loop plays while
//! the wind factor is above 1.25 at `min(gusts, 3) * 0.1 + (factor - 1) * 0.35` (`Update`).
//!
//! Looping sounds are 2D at the pak's volume times the system's particle share: the template loops
//! (`WeatherSystem::Update`) and `windSound` start with `SoundPak::Play2D` and nothing in the weather code
//! places them; a
//! destroyed system's sound fades out over 3 s.
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::light::NotShadowCaster;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, ColorWrites, Extent3d,
    RenderPipelineDescriptor, SpecializedMeshPipelineError, TextureDimension, TextureFormat,
    TextureUsages,
};
use bevy::render::renderer::{RenderContext, ViewQuery};
use bevy::render::storage::ShaderBuffer;
use bevy::render::texture::GpuImage;
use bevy::render::view::ViewDepthStencilTexture;
use bevy::shader::ShaderRef;
use gd_audio::{BaseVolume, ENGINE_HZ, Fade, SoundClass, spawn_2d};
use gd_game_core::options::GameOptions;
use gd_render::camera::GameCamera;
use gd_render::fx::{FxAssets, render};
use gd_render::render::weather::WeatherState;
use gd_sim::weather::{ClimateSet, Kind, Particle, Weather};
use gd_ui::state::session::AppState;
use std::borrow::Cow;

/// The fade-out of a destroyed system's sound (ms).
const FADE_MS: f32 = 3000.0;

const SHADER: &str = "embedded://grimdawn/shaders/weather.wesl";

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/weather.wesl");
        app.init_resource::<WeatherRuntime>()
            .init_resource::<WeatherState>()
            .add_plugins((
                MaterialPlugin::<WeatherMaterial>::default(),
                ExtractComponentPlugin::<WeatherDepth>::default(),
            ))
            .add_systems(
                Update,
                (
                    scene_depth,
                    (
                        simulate_weather,
                        draw_weather,
                        weather_flash,
                        weather_sounds,
                    )
                        .chain()
                        .in_set(gd_render::render::weather::WeatherSet),
                    soft_live,
                )
                    .chain()
                    .run_if(in_game_or_menu_scene)
                    .run_if(resource_exists::<FxAssets>),
            );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(
            Core3d,
            copy_scene_depth
                .after(main_opaque_pass_3d)
                .before(main_transparent_pass_3d)
                .in_set(Core3dSystems::MainPass),
        );
    }
}

/// The weather particles' material: the `weatherparticle*` programs (`shaders/weather.wesl`).
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(WeatherKey)]
pub struct WeatherMaterial {
    /// x 1 = directional (Rain, Snow), y 1 = soft (Mist, Fog, GroundFog with `softParticles`).
    #[uniform(0)]
    params: Vec4,
    #[texture(1, dimension = "2d_array")]
    #[sampler(2)]
    texture: Handle<Image>,
    /// Header vec4 (count), then [centre, size], [velocity, spin], [colour] per particle.
    #[storage(3, read_only)]
    particles: Handle<ShaderBuffer>,
    /// The scene depth after the opaque passes ([`WeatherDepth`]).
    #[texture(4, sample_type = "depth")]
    depth: Handle<Image>,
    additive: bool,
    order: u32,
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct WeatherKey {
    additive: bool,
}

impl From<&WeatherMaterial> for WeatherKey {
    fn from(m: &WeatherMaterial) -> Self {
        Self {
            additive: m.additive,
        }
    }
}

impl Material for WeatherMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    /// The effect particles' ordering (fx/render.rs: 10 m per sort step above 5900) at the programs' sort 6200.
    fn depth_bias(&self) -> f32 {
        (6200 - 5900) as f32 * 10.0 + self.order as f32 * 0.01
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vl = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
        ])?;
        descriptor.vertex.buffers = vec![vl];
        descriptor.primitive.cull_mode = None;
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(false);
        }
        // The pass states: Combine SrcAlpha / InvSrcAlpha, Additive SrcAlpha / One, alpha not written.
        let c = BlendComponent {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: if key.bind_group_data.additive {
                BlendFactor::One
            } else {
                BlendFactor::OneMinusSrcAlpha
            },
            operation: BlendOperation::Add,
        };
        if let Some(frag) = descriptor.fragment.as_mut() {
            for t in frag.targets.iter_mut().flatten() {
                t.blend = Some(BlendState { color: c, alpha: c });
                t.write_mask = ColorWrites::COLOR;
            }
        }
        Ok(())
    }
}

/// On the game camera: the image the scene depth is copied into for the soft weather particles.
#[derive(Component, ExtractComponent, Clone)]
#[extract_app(RenderApp)]
pub struct WeatherDepth(Handle<Image>, bool);

/// Copy the scene depth only while a soft system (Mist / Fog / GroundFog with `softParticles`) is live:
/// [`WeatherDepth`]'s flag, set after [`draw_weather`] so the frame a soft system appears has its depth.
fn soft_live(
    state: Res<WeatherState>,
    rt: Res<WeatherRuntime>,
    mut cams: Query<&mut WeatherDepth, With<GameCamera>>,
) {
    let live = rt.options.is_some_and(|o| o.soft)
        && state
            .weather
            .as_ref()
            .is_some_and(|w| w.systems.iter().any(|s| program(s.template.kind).1));
    for mut d in &mut cams {
        if d.1 != live {
            d.1 = live;
        }
    }
}

/// Keeps a depth image of the game camera's target size and asks for a copyable depth buffer.
fn scene_depth(
    mut rt: ResMut<WeatherRuntime>,
    mut cams: Query<
        (Entity, &Camera, &mut Camera3d, Option<&WeatherDepth>),
        With<gd_render::camera::GameCamera>,
    >,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    let Some((e, cam, mut c3d, have)) = cams.iter_mut().next() else {
        return;
    };
    let usage = TextureUsages::from(c3d.depth_texture_usages);
    if !usage.contains(TextureUsages::COPY_SRC) {
        c3d.depth_texture_usages = (usage | TextureUsages::COPY_SRC).into();
    }
    let size = cam
        .physical_target_size()
        .unwrap_or(UVec2::ONE)
        .max(UVec2::ONE);
    if rt.depth.as_ref().is_none_or(|(_, s)| *s != size) {
        let mut img = Image::new_uninit(
            Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Depth32Float,
            RenderAssetUsages::RENDER_WORLD,
        );
        img.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
        rt.depth = Some((images.add(img), size));
    }
    let Some((h, _)) = &rt.depth else {
        return;
    };
    if have.is_none_or(|d| d.0 != *h) {
        commands
            .entity(e)
            .try_insert(WeatherDepth(h.clone(), have.is_some_and(|d| d.1)));
    }
}

/// After the opaque passes: the game camera's depth buffer into its [`WeatherDepth`] image (the soft
/// programs' `depthTexture`).
fn copy_scene_depth(
    view: ViewQuery<(&ViewDepthStencilTexture, &WeatherDepth)>,
    images: Res<RenderAssets<GpuImage>>,
    mut ctx: RenderContext,
) {
    let (depth, target) = view.into_inner();
    if !target.1 {
        return;
    }
    let Some(img) = images.get(&target.0) else {
        return;
    };
    let src = depth.texture();
    if src.size() != img.texture.size() || src.format() != img.texture.format() {
        return;
    }
    ctx.command_encoder().copy_texture_to_texture(
        src.as_image_copy(),
        img.texture.as_image_copy(),
        src.size(),
    );
}

fn in_game_or_menu_scene(
    state: Res<State<AppState>>,
    menu: Option<Res<crate::menu_scene::MenuScene>>,
) -> bool {
    *state.get() == AppState::InGame || menu.is_some()
}

/// One system's drawing, on its drawn entity: the system's id, its storage buffer, quad mesh and material.
#[derive(Component)]
struct WeatherDraw {
    system: u32,
    buffer: Handle<ShaderBuffer>,
    mesh: Handle<Mesh>,
    mat: Handle<WeatherMaterial>,
    cap: usize,
}

/// A weather loop's sound entity: the template `sound` or the manager's `windSound` pak it plays.
#[derive(Component)]
struct WeatherLoop(String);

/// The weather options, read when the options change ([`simulate_weather`]).
#[derive(Clone, Copy, PartialEq)]
struct WeatherOptions {
    /// `weatherEnabled`.
    enabled: bool,
    /// `weatherQuality` as an index 0..3.
    quality: u8,
    /// `softParticles`.
    soft: bool,
}

impl WeatherOptions {
    fn read(options: &GameOptions) -> Self {
        WeatherOptions {
            enabled: options.flag("weatherEnabled"),
            quality: match options.get("weatherQuality") {
                Some(gd_formats::options::Value::Quality(q)) => gd_sim::weather::quality_index(q),
                _ => 0,
            },
            soft: options.flag("softParticles"),
        }
    }
}

/// The weather of the running session (reset by `exit.rs`).
#[derive(Resource, Default)]
pub struct WeatherRuntime {
    /// The map's climate sector entries (GUID, set record), read once per map ([`WeatherRuntime::map`]).
    table: Option<Vec<([i32; 4], String)>>,
    /// The map the table and the state belong to (identity only; the menu scene's map can change under it).
    map: std::sync::Weak<gd_formats::map::MapFile>,
    flash: Option<Entity>,
    /// The options as last read; `None` until the first frame.
    options: Option<WeatherOptions>,
    /// The quality the systems were last refreshed for.
    quality: Option<u8>,
    /// The scene the state belongs to: the main menu's (true) or the world's.
    menu: bool,
    picks: u32,
    scratch: Vec<[f32; 4]>,
    /// The scene depth image of the game camera and its size ([`scene_depth`]).
    depth: Option<(Handle<Image>, UVec2)>,
}

/// The program of a system type: (directional, soft-capable). The constructors (`GetShader` index 0 simple,
/// 1 directional, +2 additive, +4 soft): Rain `RainSystem::RainSystem` and Snow `SnowSystem::SnowSystem` directional,
/// never soft; Mist `MistSystem::MistSystem`, Fog / GroundFog `FogSystem::FogSystem` simple with the soft request;
/// Leaves / WindParticle `LeavesSystem::LeavesSystem` simple.
fn program(kind: Kind) -> (bool, bool) {
    match kind {
        Kind::Rain | Kind::Snow => (true, false),
        Kind::Mist | Kind::Fog | Kind::GroundFog => (false, true),
        Kind::Leaves | Kind::WindParticle | Kind::Lightning => (false, false),
    }
}

/// A particle's vertex alpha (the colour is always white). Rain / Snow / Mist spawn (1, 1, 1, 1) and keep
/// it; Fog (`FogSystem::SpawnParticle`, move `FogSystem::MoveParticle`) spawns at the system alpha
/// (`WeatherSystem.systemAlpha`: 1.0 on soft-particle-capable hardware, else 0.5; here 1.0) and fades out over its
/// last 500 ms; GroundFog (move `GroundFogSystem::MoveParticle`) fades in over the
/// first and out over the last 1000 ms, times the system alpha; Leaves / WindParticle (`LeavesSystem::SpawnParticle`)
/// spawn at 0 and
/// fade in / out over 500 ms.
fn alpha(kind: Kind, p: &Particle) -> f32 {
    let age = p.total_ms - p.life_ms;
    let ramp = |ms: f32| ((age / ms).min(p.life_ms / ms)).clamp(0.0, 1.0);
    match kind {
        Kind::Fog => (p.life_ms * 0.002).clamp(0.0, 1.0),
        Kind::GroundFog => ramp(1000.0),
        Kind::Leaves | Kind::WindParticle => ramp(500.0),
        Kind::Rain | Kind::Snow | Kind::Mist | Kind::Lightning => 1.0,
    }
}

/// vec4s per particle in a weather buffer.
const VEC4S: usize = 3;

fn buffer_bytes(cap: usize) -> u64 {
    ((1 + cap * VEC4S) * 16) as u64
}

/// The weather simulation: the options, the climate under the focus, the systems' update, and the rain's ripples
/// and the gusts' pushes for the water and the grass.
#[allow(clippy::too_many_arguments)]
fn simulate_weather(
    (mut rt, mut state): (ResMut<WeatherRuntime>, ResMut<WeatherState>),
    world: Option<Res<gd_world::StreamedMap>>,
    levels: Res<gd_game_core::levels::Levels>,
    data: Res<gd_game_core::data::GameData>,
    options: Res<GameOptions>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    time: Res<Time<Virtual>>,
    (tod, ambiance, menu, backdrop, coll): (
        Res<gd_render::render::env::TimeOfDay>,
        Option<Res<gd_render::render::env::Ambiance>>,
        Option<Res<crate::menu_scene::MenuScene>>,
        Option<Res<gd_render::render::backdrop::MenuBackdrop>>,
        Option<Res<crate::collision::WorldCollision>>,
    ),
    cams: Query<&GameCamera>,
    story: Option<NonSend<gd_game_core::story::StoryRuntime>>,
    owned: Owned,
    mut commands: Commands,
) {
    let rt = &mut *rt;
    let state = &mut *state;
    if options.is_changed() || rt.options.is_none() {
        rt.options = Some(WeatherOptions::read(&options));
    }
    let Some(opts) = rt.options else {
        return;
    };
    let Some(cam) = cams.iter().next() else {
        return;
    };
    // The main menu scene takes its climate from its own map (mainmenu.map); the world and the scene never mix.
    if rt.menu != menu.is_some() {
        clear_all(rt, state, &owned, &mut commands);
        state.weather = None;
        rt.table = None;
        rt.menu = menu.is_some();
    }
    let map = match &backdrop {
        Some(b) => b.map.as_ref(),
        None => world.as_ref().map(|w| &w.map),
    };
    let Some(map) = map else {
        return;
    };
    // Another map (a menu scene's change, or the world's): its own climate table, the weather afresh.
    if !std::ptr::eq(rt.map.as_ptr(), std::sync::Arc::as_ptr(map)) {
        clear_all(rt, state, &owned, &mut commands);
        state.weather = None;
        rt.table = None;
        rt.map = std::sync::Arc::downgrade(map);
    }
    let sector_data = &map.sector_data;
    let levels = backdrop.as_ref().map_or(&*levels, |b| &b.levels);
    // `SetEnabled(false)`: everything stops and the update returns at once.
    if !opts.enabled {
        if state.weather.is_some() {
            clear_all(rt, state, &owned, &mut commands);
            state.weather = None;
        }
        return;
    }
    let quality = opts.quality;
    if rt.table.is_none() {
        rt.table = Some(
            sector_data
                .tail_layers()
                .map(|t| {
                    t.later
                        .climate
                        .into_iter()
                        .map(|c| (c.base.ints, c.record))
                        .collect()
                })
                .unwrap_or_default(),
        );
    }
    let weather = state.weather.get_or_insert_with(|| {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32);
        Weather::new(seed, gd_sim::weather::Templates::load(&data.db))
    });
    // `AmbianceManager::GetIsDay`; before the first ambiance the default cycle at the clock.
    let day = ambiance.map_or_else(
        || {
            let c = gd_render::render::env::DayNightCycle::default();
            gd_sim::jukebox::is_day(
                backdrop.as_ref().map_or(tod.0, |b| b.time),
                c.sunrise,
                c.sunset,
            )
        },
        |a| a.is_day,
    );
    if rt.quality != Some(quality) {
        weather.refresh_quality(quality);
        rt.quality = Some(quality);
    }

    // The climate set of the sector under the camera focus (`GetTargetId` type 6 + `GetSectorData`). A set is
    // loaded only when the name changes (`ChangeClimate` is a no-op for the same name).
    let forced: Cow<str>;
    let (name, set) = match hooks.render.weather.as_deref() {
        Some(f) => {
            forced = if f.contains('/') {
                Cow::Borrowed(f)
            } else {
                Cow::Owned(format!("records/fx/climate_weather/{f}.dbr"))
            };
            let set = (*forced != weather.set_name)
                .then(|| ClimateSet::single(&data.db, &forced))
                .flatten();
            (&*forced, set)
        }
        None => {
            let focus = backdrop
                .as_ref()
                .map_or(cam.target, |b| b.view.player.translation);
            // In the game the sector of the player's own region (`Player::UpdateSelf`; the Shattered Realm's
            // regions overlap main-world ground). PORT: the engine's climate sector source is not traced; the
            // menu scene has one map, so the position alone.
            let region =
                gd_game_core::riftgates::player_region(story.as_deref()).filter(|_| menu.is_none());
            let guid = match region {
                Some(r) => levels.sector_guid_in(r, focus, 6),
                None => levels.sector_guid(focus, 6),
            };
            let name = guid
                .and_then(|g| rt.table.as_ref()?.iter().find(|(id, _)| *id == g))
                .map_or("", |(_, r)| r.as_str());
            let set = if name != weather.set_name && !name.is_empty() {
                ClimateSet::load(&data.db, name)
            } else {
                None
            };
            (name, set)
        }
    };
    weather.change_climate(name, set, day, quality);
    let dt_ms = time.delta_secs() * 1000.0;
    // The floor for quality 3 rain and the gusts (the world only; the menu scene has no collision world).
    let floor = coll.as_deref().map(|c| Floor(c, cam.target.y));
    let ground: Option<&dyn gd_sim::weather::Ground> = match (&floor, &menu) {
        (Some(f), None) => Some(f),
        _ => None,
    };
    weather.update(
        backdrop
            .as_ref()
            .map_or(cam.target, |b| b.view.player.translation)
            .to_array(),
        dt_ms,
        day,
        quality,
        ground,
    );

    // The rain's death hook `RainSystem::OnParticleDeath`: above `weatherQuality` 1 every drop asks for a ripple
    // where it
    // ends, `Water::CreateRipple(pos, 0.5, 5.0, 0)` on the camera region's water.
    if quality > 1 {
        for s in weather
            .systems
            .iter()
            .filter(|s| s.template.kind == Kind::Rain)
        {
            state
                .ripples
                .requests
                .extend(s.died.iter().map(|p| (Vec3::from_array(*p), 0.5, 5.0)));
        }
    }
    // The gusts' `World::CreateDirectionalWind`: the grass offset field (`Terrain::CreateGrassWind` with falloff) and
    // a ripple where the push is strong enough (strength >= 1).
    state.ripples.requests.extend(
        weather
            .winds
            .iter()
            .map(|w| (Vec3::from_array(w.pos), w.radius, w.strength)),
    );
    state.grass_winds.extend(weather.winds.iter().map(|w| {
        gd_render::render::grass::GrassWindPush {
            pos: Vec3::from_array(w.pos),
            dir: Vec3::new(w.dir[0], 0.0, w.dir[1]),
            radius: w.radius,
            strength: w.strength,
            falloff: true,
        }
    }));

    if rt.picks != weather.picks {
        rt.picks = weather.picks;
        debug!(
            "weather: set '{}' day {day} quality {quality}: {:?}",
            weather.set_name, weather.used
        );
    }
}

/// One entity per particle system ([`WeatherDraw`]): its quads, material and particle buffer, refreshed each frame.
fn draw_weather(
    mut rt: ResMut<WeatherRuntime>,
    state: Res<WeatherState>,
    data: Res<gd_game_core::data::GameData>,
    mut assets: ResMut<FxAssets>,
    mut res: (
        ResMut<Assets<Image>>,
        ResMut<Assets<Mesh>>,
        ResMut<Assets<WeatherMaterial>>,
        ResMut<Assets<ShaderBuffer>>,
    ),
    mut draws: Query<(Entity, &mut WeatherDraw)>,
    mut commands: Commands,
) {
    let (images, meshes, mats, buffers) = &mut res;
    let WeatherRuntime {
        depth,
        scratch,
        options,
        ..
    } = &mut *rt;
    let soft = options.is_some_and(|o| o.soft);
    let systems = state.weather.as_ref().map_or(&[][..], |w| &w.systems[..]);
    for (e, d) in &draws {
        if !systems.iter().any(|s| s.id == d.system) {
            commands.entity(e).try_despawn();
        }
    }
    let Some((depth, _)) = depth.as_ref() else {
        return;
    };
    for s in systems {
        let kind = s.template.kind;
        let (directional, soft_kind) = program(kind);
        let params = Vec4::new(
            directional as u8 as f32,
            (soft_kind && soft) as u8 as f32,
            0.0,
            0.0,
        );
        // A new system's drawing is spawned here and filled in this frame.
        let mut created = None;
        let (entity, d): (Entity, &mut WeatherDraw) =
            match draws.iter_mut().find(|(_, d)| d.system == s.id) {
                Some((e, d)) => (e, d.into_inner()),
                None => {
                    let Some((img, _)) = assets.texture(&data, &s.template.texture, images) else {
                        continue;
                    };
                    let cap = render::MIN_CAPACITY;
                    let mut buf =
                        ShaderBuffer::new(vec![[0.0f32; 4]], RenderAssetUsages::RENDER_WORLD);
                    buf.resize_buffer(buffer_bytes(cap));
                    let buffer = buffers.add(buf);
                    let mat = mats.add(WeatherMaterial {
                        params,
                        texture: img,
                        particles: buffer.clone(),
                        depth: depth.clone(),
                        additive: s.template.additive,
                        order: s.id,
                    });
                    let mesh = assets.quads(cap, meshes);
                    let entity = commands
                        .spawn((
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(mat.clone()),
                            Transform::IDENTITY,
                            Visibility::Visible,
                            NoFrustumCulling,
                            NotShadowCaster,
                            Name::new("weather"),
                        ))
                        .id();
                    let (e, d) = created.insert((
                        entity,
                        WeatherDraw {
                            system: s.id,
                            buffer,
                            mesh,
                            mat,
                            cap,
                        },
                    ));
                    (*e, d)
                }
            };
        // A new depth image (window resized, session start) or a toggled `softParticles`.
        if let Some(m) = mats.get(&d.mat)
            && (m.depth != *depth || m.params != params)
            && let Some(mut m) = mats.get_mut(&d.mat)
        {
            m.depth = depth.clone();
            m.params = params;
        }
        let n = s.particles.len();
        if n > d.cap {
            d.cap = n.next_power_of_two();
            d.mesh = assets.quads(d.cap, meshes);
            commands.entity(entity).insert(Mesh3d(d.mesh.clone()));
        }
        // The vertex data of the render functions (`WeatherSystem::Render`, Rain `RainSystem::Render`): the centre,
        // the colour,
        // (u, v, size, spin) and the velocity (Rain) / the base wind tilt (the others; unused by the simple program).
        scratch.clear();
        scratch.push([f32::from_bits(n as u32), 0.0, 0.0, 0.0]);
        for p in &s.particles {
            scratch.push([p.pos[0], p.pos[1], p.pos[2], p.size]);
            scratch.push([p.vel[0], p.vel[1], p.vel[2], p.spin]);
            scratch.push([1.0, 1.0, 1.0, alpha(kind, p)]);
        }
        if let Some(mut b) = buffers.get_mut(&d.buffer) {
            b.clear();
            b.extend_from_slice(&scratch[..]);
            b.resize_buffer(buffer_bytes(d.cap));
        }
        if let Some((e, d)) = created {
            commands.entity(e).insert(d);
        }
    }
}

/// Lightning: a white light (range 100) near the camera while a flash is on.
fn weather_flash(
    mut rt: ResMut<WeatherRuntime>,
    state: Res<WeatherState>,
    cams: Query<&GameCamera>,
    mut lights: Query<(&mut PointLight, &mut Transform)>,
    mut commands: Commands,
) {
    let (Some(weather), Some(cam)) = (state.weather.as_ref(), cams.iter().next()) else {
        return;
    };
    let flash_on = weather.lightning.is_on();
    let at = Vec3::from(cam.target.to_array()) + Vec3::from(weather.lightning.offset);
    let intensity = if flash_on {
        40_000.0 * gd_sim::weather::Lightning::RANGE
    } else {
        0.0
    };
    match rt.flash.and_then(|e| lights.get_mut(e).ok()) {
        Some((mut l, mut t)) => {
            if l.intensity != intensity {
                l.intensity = intensity;
            }
            if flash_on {
                t.translation = at;
            }
        }
        None if weather.lightning_intensity() > 0.0 => {
            rt.flash = Some(
                commands
                    .spawn((
                        PointLight {
                            color: Color::WHITE,
                            intensity,
                            range: gd_sim::weather::Lightning::RANGE,
                            shadow_maps_enabled: false,
                            ..default()
                        },
                        Transform::from_translation(at),
                        Name::new("lightning"),
                    ))
                    .id(),
            );
        }
        None => {}
    }
}

/// The volume factor `pak` should loop at: the manager's `windSound` while on (`wind`), else the loudest system whose
/// template `sound` it is; `None`: not wanted.
fn wanted(weather: &Weather, wind: Option<f32>, pak: &str) -> Option<f32> {
    if pak == weather.templates.wind_sound
        && let Some(v) = wind
    {
        return Some(v);
    }
    weather
        .systems
        .iter()
        .filter(|s| s.template.sound == pak)
        .map(|s| s.volume())
        .reduce(f32::max)
}

/// The looping sounds: the template `sound` of each system, and the manager's `windSound` (on while the wind factor is
/// above 1.25, stopped at once below). `WeatherManager`: a destroyed system's sound
/// fades out over 3000 ms (`SoundManager::FadeSound`, linear, stopped at the end: [`Fade`]).
fn weather_sounds(
    state: Res<WeatherState>,
    data: Res<gd_game_core::data::GameData>,
    mut sounds: ResMut<gd_audio::Sounds>,
    mut loops: Query<(Entity, &WeatherLoop, &mut BaseVolume), Without<Fade>>,
    mut commands: Commands,
) {
    let Some(weather) = state.weather.as_ref() else {
        return;
    };
    let wind_pak = weather.templates.wind_sound.as_str();
    let wind = (!wind_pak.is_empty() && weather.wind_sound_on()).then(|| weather.wind_volume());
    for (e, l, mut b) in &mut loops {
        match wanted(weather, wind, &l.0) {
            Some(v) => {
                let Some(info) = sounds.pak(&data, &l.0) else {
                    continue;
                };
                let volume = info.volume * v;
                if (b.0 - volume).abs() > 0.005 {
                    b.0 = volume;
                }
            }
            None if l.0 == wind_pak => commands.entity(e).try_despawn(),
            None => {
                commands.entity(e).try_insert(Fade::out(b.0, FADE_MS));
            }
        }
    }
    // Each wanted pak once: the systems' sounds, then the wind's unless a system loops the same pak.
    let systems = &weather.systems;
    let paks = systems
        .iter()
        .enumerate()
        .map(|(i, s)| (i, s.template.sound.as_str()))
        .filter(|(i, p)| !p.is_empty() && !systems[..*i].iter().any(|o| o.template.sound == *p))
        .map(|(_, p)| p)
        .chain(
            wind.filter(|_| !systems.iter().any(|s| s.template.sound == wind_pak))
                .map(|_| wind_pak),
        );
    for pak in paks {
        let Some(v) = wanted(weather, wind, pak).filter(|v| *v > 0.0) else {
            continue;
        };
        if loops.iter().any(|(_, l, _)| l.0 == pak) {
            continue;
        }
        let Some(info) = sounds.pak(&data, pak) else {
            continue;
        };
        // Read in the background the first time: the loop starts on a later frame (it is not in `loops` until then).
        let Some(file) = info
            .names
            .first()
            .and_then(|f| sounds.file_or_fetch(&data, f).ok())
        else {
            continue;
        };
        let speed = file.speed(ENGINE_HZ);
        let e = spawn_2d(
            &mut commands,
            SoundClass::Ambient,
            info.volume * v,
            speed,
            file,
            true,
        );
        commands
            .entity(e)
            .insert((WeatherLoop(pak.to_owned()), Name::new("weather sound")));
    }
}

/// The world floor for weather: the rain ray against every physics surface, the gusts on the terrain height (from the
/// camera focus height).
struct Floor<'a>(&'a crate::collision::WorldCollision, f32);

impl gd_sim::weather::Ground for Floor<'_> {
    fn height(&self, x: f32, z: f32) -> Option<f32> {
        self.0.terrain_ground(Vec3::new(x, self.1, z))
    }

    fn ray(&self, origin: [f32; 3], dir: [f32; 3], len: f32) -> Option<f32> {
        let a = Vec3::from_array(origin);
        let (hit, _) = self.0.segment_hit(a, a + Vec3::from_array(dir) * len)?;
        Some(hit.distance(a))
    }
}

/// The weather's own entities, despawned by [`clear_all`].
type Owned<'w, 's> = (
    Query<'w, 's, Entity, With<WeatherDraw>>,
    Query<'w, 's, Entity, With<WeatherLoop>>,
);

/// Despawns every weather entity (the scene switched, the options switched off).
fn clear_all(
    rt: &mut WeatherRuntime,
    state: &mut WeatherState,
    (draws, loops): &Owned,
    commands: &mut Commands,
) {
    for e in draws.iter().chain(loops.iter()) {
        commands.entity(e).try_despawn();
    }
    if let Some(e) = rt.flash.take() {
        commands.entity(e).try_despawn();
    }
    state.ripples.clear(commands);
}
