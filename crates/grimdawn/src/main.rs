//! Grim Dawn reimplementation on Bevy. Reads the user's installed game data.
//!
//! The game folder ([`game_root`]): `--game-dir <path>` (or `--game-dir=<path>`) when given, else (`dev`
//! builds only) `GD_ROOT`, else the folder of the executable, then the working folder. A folder is the install
//! when it holds `database/database.arz` ([`data::find_install`]).
// Bevy system parameters are long by nature; Bevy recommends allowing this lint for them.
#![allow(clippy::type_complexity)]
use gd_game::{afflictions, collision, combat, consumables, data};
mod ascendant;
mod breakables;
mod camera_input;
mod casting;
mod chests;
mod controller_target;
mod crafting;
mod death;
#[cfg(feature = "dev")]
mod dev;
mod devotion;
mod doll;
mod doors;
mod endless_shrine;
mod evade;
mod exit;
mod fade;
mod fps_limit;
mod gibs;
mod heap;
mod hover;
mod illusion;
mod inventory;
mod loot;
mod menu;
mod menu_scene;
mod monsters;
mod npc_marks;
mod pets;
mod physics_decoration;
mod player;
mod ragdoll;
mod record_facts;
mod render_devices;
mod reroll;
mod riftgates;
mod session;
mod settings;
mod story;
mod trade;
mod ui;
mod video;
mod weather;

use bevy::prelude::*;

/// glibc's dynamic mmap threshold grows toward 32 MB as big blocks free, so a level unload's mesh /
/// texture / nav buffers go back to the OS with `munmap`/`madvise` — and the *next* travel's first
/// allocations fault every page back in on the frame thread (travel spikes). Pin the threshold and
/// keep a top pad so those blocks recycle through the heap instead. Linux/gnu only; other allocators
/// ignore this.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn tune_allocator() {
    // SAFETY: mallopt only tunes this process's allocator parameters; no pointers, no threads yet.
    unsafe { libc::mallopt(libc::M_MMAP_THRESHOLD, 64 * 1024) };
    // SAFETY: as above.
    unsafe { libc::mallopt(libc::M_TRIM_THRESHOLD, 64 * 1024 * 1024) };
    // SAFETY: as above.
    unsafe { libc::mallopt(libc::M_TOP_PAD, 16 * 1024 * 1024) };
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn tune_allocator() {}

fn main() {
    tune_allocator();
    let root = match game_root() {
        Ok(root) => root,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    // Before the plugins are built: plugin builds read the hooks.
    #[cfg(feature = "dev")]
    let hooks = gd_game_core::hooks::Hooks::from_env();
    // The `device` option (int 7) picks the launch adapter. PORT: applied at the next launch only; an
    // out-of-range index keeps Bevy's default adapter.
    let options_file = gd_ui_kit::state::settings::path();
    #[cfg(feature = "dev")]
    let options_file = hooks
        .session
        .options_file
        .as_ref()
        .map_or(options_file, std::path::PathBuf::from);
    let device = std::fs::read_to_string(&options_file)
        .map(|text| gd_game_core::options::GameOptions::parse(&text).int("device"))
        .unwrap_or(0);
    let adapters = render_devices::enumerate();
    let adapter = usize::try_from(device)
        .ok()
        .and_then(|i| adapters.get(i))
        .and_then(|a| render_devices::backend_of(&a.backend).map(|b| (a.name.clone(), b)));
    let mut wgpu_settings = bevy::render::settings::WgpuSettings {
        memory_hints: bevy::render::settings::MemoryHints::Manual {
            // Grow blocks from 64MB instead of committing 512MB up front, which spills VRAM
            // to system RAM under WDDM on small (4-6GB) Windows GPUs. Cap blocks at 512MB.
            suballocated_device_memory_block_size: (64 << 20)..(512 << 20),
        },
        ..default()
    };
    if let Some((name, backend)) = &adapter {
        wgpu_settings.adapter_name = Some(name.clone());
        // The name match is a substring: restrict the backend too.
        wgpu_settings.backends = Some(wgpu::Backends::from(*backend));
    }
    let resolution: (u32, u32) = (1600, 900);
    #[cfg(feature = "dev")]
    let resolution = hooks.render.res.unwrap_or(resolution);
    // A hidden window gets no frame callbacks from the compositor: no vsync for `GD_SHOT`.
    let present_mode = bevy::window::PresentMode::AutoVsync;
    #[cfg(feature = "dev")]
    let present_mode = if hooks.shot.path.is_some() {
        bevy::window::PresentMode::AutoNoVsync
    } else {
        present_mode
    };
    let clear = Color::BLACK;
    #[cfg(feature = "dev")]
    let clear = if hooks.render.clear {
        Color::srgb(1.0, 0.0, 1.0)
    } else {
        clear
    };
    let mut app = App::new();
    #[cfg(feature = "dev")]
    if let Some(d) = &hooks.session.save_dir {
        app.insert_resource(gd_game_core::session::SaveDir(d.into()));
    }
    #[cfg(feature = "dev")]
    app.insert_resource(hooks);
    let plugins = DefaultPlugins
        .set(bevy::render::RenderPlugin {
            render_creation: wgpu_settings.into(),
            ..default()
        })
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "Grim Dawn (Rust/Bevy port)".into(),
                resolution: resolution.into(),
                present_mode,
                ..default()
            }),
            ..default()
        });
    // A release build compiles every log macro out; `dev` (and the profiling build) keeps the logger.
    // `tonemapping=error`: the doll and portrait cameras use `Tonemapping::None`.
    #[cfg(any(debug_assertions, feature = "dev"))]
    let plugins = plugins.set(bevy::log::LogPlugin {
        filter: "wgpu=error,naga=warn,bevy_core_pipeline::tonemapping=error".into(),
        ..default()
    });
    #[cfg(not(any(debug_assertions, feature = "dev")))]
    let plugins = plugins.disable::<bevy::log::LogPlugin>();
    app.insert_resource(render_devices::RenderAdapters { entries: adapters })
        .add_plugins(plugins)
        .add_plugins((
            data::DataPlugin { root },
            gd_game::GameLogicPlugin,
            gd_render::camera::CameraPlugin,
            camera_input::CameraInputPlugin,
            gd_render::textures::TexturePlugin,
            gd_render::model::ModelPlugin,
        ))
        .add_plugins((
            gd_render::character::CharacterPlugin,
            gd_render::render::GdRenderPlugin,
            menu_scene::MenuPlugin,
        ))
        .add_plugins(gd_render::fx::FxPlugin)
        .add_plugins((gd_ui::UiPlugin, ui::UiPlugin))
        .add_plugins(story::StoryPlugin)
        .add_plugins(heap::HeapPlugin)
        .add_plugins((
            exit::ExitPlugin,
            gd_world::WorldPlugin,
            player::PlayerPlugin,
            gd_audio::GameAudioPlugin,
            session::SessionPlugin,
            gd_render::water::WaterPlugin,
            weather::WeatherPlugin,
            monsters::MonstersPlugin,
            casting::CastingViewPlugin,
            loot::LootPlugin,
            inventory::InventoryPlugin,
            gd_ui::tooltip::TooltipPlugin,
        ))
        .add_plugins((
            chests::ChestsPlugin,
            doors::DoorsPlugin,
            breakables::BreakablesPlugin,
            (
                physics_decoration::PhysicsDecorationViewPlugin,
                ragdoll::RagdollViewPlugin,
            ),
            riftgates::RiftgatesPlugin,
            death::DeathPlugin,
            trade::TradePlugin,
            npc_marks::NpcMarksViewPlugin,
            gd_render::trails::TrailsPlugin,
            gd_render::beams::BeamsPlugin,
            gd_render::lightning::LightningPlugin,
            devotion::DevotionViewPlugin,
            menu::MenuPlugin,
        ))
        .add_plugins((
            endless_shrine::EndlessShrineViewPlugin,
            gd_render::see_through::SeeThroughPlugin,
            gd_world::decoration_anim::DecorationAnimPlugin,
            hover::HoverPlugin,
            gd_render::icy::IcyPlugin,
            controller_target::ControllerTargetPlugin,
            record_facts::RecordFactsPlugin,
            doll::DollPlugin,
            gd_render::chaos_beam::ChaosBeamPlugin,
            video::VideoPlugin,
        ))
        .add_plugins((
            evade::EvadeInputPlugin,
            pets::PetInputPlugin,
            ascendant::AscendantPlugin,
            settings::SettingsPlugin,
            fps_limit::FpsLimitPlugin,
        ))
        .add_systems(Startup, setup_lighting)
        .add_systems(PostStartup, place_camera)
        .insert_resource(ClearColor(clear))
        .add_systems(Update, follow_focus);
    // The logger exists once DefaultPlugins are built.
    #[cfg(any(debug_assertions, feature = "dev"))]
    match &adapter {
        Some((name, backend)) => info!("render adapter: {name} ({backend:?})"),
        None => info!("render adapter: Bevy default"),
    }
    #[cfg(feature = "dev")]
    app.add_plugins(dev::DevPlugin);
    app.run();
}

/// The Grim Dawn install the session mounts (module doc). The error names the folders tried and the option.
/// PORT: the real game reads the install it runs from; the port also takes the folder as an argument.
fn game_root() -> Result<std::path::PathBuf, String> {
    use std::path::PathBuf;
    let mut given = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        if a == "--game-dir" {
            given = Some(PathBuf::from(
                args.next().ok_or("--game-dir needs the Grim Dawn folder")?,
            ));
        } else if let Some(p) = a.to_str().and_then(|a| a.strip_prefix("--game-dir=")) {
            given = Some(PathBuf::from(p));
        }
    }
    let candidates: Vec<PathBuf> = match given {
        Some(dir) => vec![dir],
        None => {
            let mut c = Vec::new();
            #[cfg(feature = "dev")]
            c.extend(std::env::var_os("GD_ROOT").map(PathBuf::from));
            c.extend(
                std::env::current_exe()
                    .ok()
                    .and_then(|e| e.parent().map(PathBuf::from)),
            );
            c.extend(std::env::current_dir().ok());
            c
        }
    };
    gd_game_core::data::find_install(candidates).map_err(|tried| {
        let tried: Vec<String> = tried.iter().map(|p| p.display().to_string()).collect();
        format!(
            "no Grim Dawn install found (no {} in: {}); start with --game-dir <the Grim Dawn folder>",
            gd_game_core::data::INSTALL_MARKER,
            tried.join(", ")
        )
    })
}

fn setup_lighting(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            illuminance: 9000.0,
            ..default()
        },
        Transform::from_xyz(-4.0, 10.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 600.0,
        ..default()
    });
}

/// Start the camera at the player spawn point (world mode).
fn place_camera(
    world: Option<Res<gd_world::StreamedMap>>,
    #[cfg(feature = "dev")] scene: Option<Res<dev::SceneCamera>>,
    mut cams: Query<&mut gd_render::camera::GameCamera>,
) {
    #[cfg(feature = "dev")]
    if scene.is_some() {
        return;
    }
    let Some(p) = world.and_then(|w| w.spawn_point()) else {
        return;
    };
    for mut c in &mut cams {
        c.target = p;
    }
}

fn follow_focus(
    cams: Query<&gd_render::camera::GameCamera>,
    focus: Option<ResMut<gd_game_core::levels::StreamFocus>>,
) {
    if let (Ok(c), Some(mut f)) = (cams.single(), focus) {
        f.set_if_neq(gd_game_core::levels::StreamFocus(c.target));
    }
}
