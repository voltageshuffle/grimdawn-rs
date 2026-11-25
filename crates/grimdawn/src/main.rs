//! Grim Dawn reimplementation on Bevy. Reads the user's installed game data.
//!
//! The game folder is found once at start ([`game_root`]) and mounted by [`data::DataPlugin`]: the
//! `--game-dir <path>` argument (or `--game-dir=<path>`) when given, else (`dev` builds only) `GD_ROOT`,
//! else the folder of the executable, then the working folder, as Grim Dawn finds its data next to
//! itself. A folder is the install when it holds `database/database.arz` ([`data::find_install`]).
// Bevy system parameters (queries with filters, param tuples) are long by
// nature; Bevy recommends allowing this lint for them.
#![allow(clippy::type_complexity)]
use gd_game::{afflictions, collision, combat, consumables, data};
mod ascendant;
#[cfg(feature = "dev")]
mod dev;
mod exit;
mod hooks;
use gd_game_core::hooks::Hooks;
mod breakables;
mod camera_input;
mod casting;
mod chests;
mod controller_target;
mod crafting;
mod death;
mod devotion;
mod doll;
mod doors;
mod endless_shrine;
mod evade;
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

fn main() {
    let root = match game_root() {
        Ok(root) => root,
        Err(e) => {
            // Before the app: no logger yet.
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    // Parsed once, first: the window below needs two of them, and the resource must exist before the
    // plugins are built (plugin builds and `FromWorld` resources read it). Only `dev` builds read the
    // environment (`dev` module); otherwise every hook is off.
    #[cfg(feature = "dev")]
    let hooks = Hooks::from_env();
    #[cfg(not(feature = "dev"))]
    let hooks = Hooks::default();
    // The `device` option (int 7) picks the launch adapter: read the options file early (the same file
    // `settings::load` reads at PreStartup), enumerate the wgpu adapters once and pass the choice to
    // `WgpuSettings` below. PORT: applied at the NEXT launch only (no hot-swap); an out-of-range index
    // keeps Bevy's default adapter. `WGPU_ADAPTER_NAME` still overrides the name (Bevy reads it itself).
    let options_file = hooks
        .session
        .options_file
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(gd_ui_kit::state::settings::path);
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
            suballocated_device_memory_block_size: (512 << 20)..(512 << 20),
        },
        ..default()
    };
    if let Some((name, backend)) = &adapter {
        wgpu_settings.adapter_name = Some(name.clone());
        // The name match is a substring: restrict the backend too, so an adapter of another backend
        // sharing the name cannot win.
        wgpu_settings.backends = Some(wgpu::Backends::from(*backend));
    }
    let mut app = App::new();
    app.insert_resource(hooks.clone())
        .insert_resource(render_devices::RenderAdapters { entries: adapters })
        .add_plugins(
            DefaultPlugins
                // The doll and portrait cameras carry the light rig in their `ColorGrading` with
                // `Tonemapping::None` on purpose (`doll.rs` module doc); Bevy warns about that once per
                // camera spawn. The module's only other warning is the same check for `DebandDither`,
                // which the port never enables. (The default filter plus that module.)
                .set(bevy::log::LogPlugin {
                    filter: "wgpu=error,naga=warn,bevy_core_pipeline::tonemapping=error".into(),
                    ..default()
                })
                .set(bevy::render::RenderPlugin {
                    render_creation: wgpu_settings.into(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Grim Dawn (Rust/Bevy port)".into(),
                        // `GD_RES=1920x1080` e.g. to match a reference screenshot.
                        resolution: hooks.render.res.unwrap_or((1600, 900)).into(),
                        // Automated screenshot runs must not block on vsync: a hidden
                        // window gets no frame callbacks from the compositor.
                        present_mode: if hooks.shot.path.is_some() {
                            bevy::window::PresentMode::AutoNoVsync
                        } else {
                            bevy::window::PresentMode::AutoVsync
                        },
                        ..default()
                    }),
                    ..default()
                }),
        )
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
        // Debug: `GD_CLEAR=1` shows the clear colour as magenta (finds holes).
        .insert_resource(ClearColor(if hooks.render.clear {
            Color::srgb(1.0, 0.0, 1.0)
        } else {
            Color::BLACK
        }))
        .add_systems(Update, follow_focus);
    // The logger exists once DefaultPlugins are built (LogPlugin installs its subscriber in `build`).
    match &adapter {
        Some((name, backend)) => info!("render adapter: {name} ({backend:?})"),
        None => info!("render adapter: Bevy default"),
    }
    #[cfg(feature = "dev")]
    app.add_plugins(dev::DevPlugin);
    app.run();
}

/// The Grim Dawn install the session mounts: `--game-dir <path>` / `--game-dir=<path>` when given (only
/// that folder), else the first install among `GD_ROOT` (`dev` builds), the executable's folder and the
/// working folder ([`data::find_install`]). The error names the folders tried and the option.
/// PORT: the real game reads the install it runs from (Steam starts it in that folder); the port is
/// built elsewhere, so it also takes the folder as an argument.
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
