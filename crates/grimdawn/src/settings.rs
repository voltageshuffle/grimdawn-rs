//! The port's own settings file: `$XDG_DATA_HOME/grimdawn-rs/Settings/options.txt`
//! (default `~/.local/share/grimdawn-rs/Settings/options.txt`), in the game's
//! `options.txt` format (`gd_game::options`). Read before start-up into
//! [`GameOptions`]; written by the options window's Apply.
//!
//! `GD_OPTIONS` (hooks.rs) reads another options file instead; Apply still
//! writes the port's own file only.
use bevy::prelude::*;
use gd_game::options::GameOptions;
use std::path::PathBuf;

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiScaleOption>()
            .add_systems(PreStartup, load)
            .add_systems(Update, (apply_video, apply_ui_scale));
    }
}

/// The port's settings file.
pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    base.join("grimdawn-rs/Settings/options.txt")
}

fn load(
    mut commands: Commands,
    hooks: Res<crate::hooks::Hooks>,
    mut grass: ResMut<gd_render::render::grass::GrassQuality>,
) {
    let file = hooks
        .session
        .options_file
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(path);

    match std::fs::read_to_string(&file) {
        Ok(text) => {
            info!("options: {}", file.display());
            let options = GameOptions::parse(&text);
            apply_quality(&options, &mut grass);
            commands.insert_resource(options);
        }
        Err(_) => info!("options: {} not found, engine defaults", file.display()),
    }
}

/// The video options this port honours (`GraphicsEngine::UpdateFromOptions` @0x180093020 runs every frame):
/// `syncToRefresh` (bool 6) is vsync; `screenMode` and `resolution` (see [`window_mode`],
/// [`window_size`]); `textureQuality` (mip reduction, read by newly loaded textures: `textures.rs`); `shadowQuality` /
/// `shadows` (`render/shadow.rs`); `detailLevel`'s terrain quality (grass of levels built from now on, `render/grass.rs`); `weatherEnabled` / `weatherQuality` (`weather.rs`, read every frame). `fxaa` (`render/post.rs`, every frame); `softParticles` (`weather.rs`). `antiAliasing` is not stored: it has no effect in the engine's
/// deferred renderer (sub_180093240 zeroes the sample count, `render/post.rs`); the options this port does not use keep their file lines (`gd_game::options`). Elsewhere: `lightingQuality` (`render/light_rig.rs`, `render/point_shadow.rs`, `render/env.rs`), `detailObjects` (decals: `decal.rs`, `casting.rs`), `ambientocclusion` (`render/post.rs`), `hdrLighting` / `advancedEffects` (`render/env.rs`), `reflectionQuality` (the water's planar reflection, `render/reflection.rs`), `uiScale` ([`apply_ui_scale`]). PORT: the window opens windowed at
/// `GD_RES` or 1600 x 900 until an options file is read; screenshot runs and `GD_RES` keep their window.
fn apply_video(
    options: Option<Res<GameOptions>>,
    hooks: Res<crate::hooks::Hooks>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut grass: ResMut<gd_render::render::grass::GrassQuality>,
    mut applied: Local<Option<(i32, Option<(u32, u32)>)>>,
) {
    let Some(options) = options.filter(|o| o.is_changed()) else {
        return;
    };
    apply_quality(&options, &mut grass);
    if hooks.shot.path.is_some() {
        return;
    }
    let want = if options.flag("syncToRefresh") {
        bevy::window::PresentMode::AutoVsync
    } else {
        bevy::window::PresentMode::AutoNoVsync
    };
    let screen_mode = options.int("screenMode");
    let size = match options.get("resolution") {
        Some(gd_formats::options::Value::Vec2(w, h)) => window_size(*w, *h),
        _ => None,
    };
    // Mode and size are set when they change (not on every options change: a window the player resized by hand keeps its size).
    let changed = *applied != Some((screen_mode, size));
    *applied = Some((screen_mode, size));
    for mut w in &mut windows {
        if w.present_mode != want {
            w.present_mode = want;
        }
        if !changed {
            continue;
        }
        let mode = window_mode(screen_mode);
        if w.mode != mode {
            w.mode = mode;
        }
        if mode == bevy::window::WindowMode::Windowed
            && hooks.render.res.is_none()
            && let Some((width, height)) = size
            && w.resolution.physical_size() != UVec2::new(width, height)
        {
            w.resolution.set_physical_resolution(width, height);
        }
    }
}

/// The options applied live on every change. `GraphicsEngine::UpdateFromOptions` @0x180093020 (every frame:
/// texture reduction, detail level) and `RenderDevice` sampler setup (anisotropy from `anisotropicFiltering`, applied
/// every frame), `Engine::SetFxQualityModifier` @0x180079480 (called on a fxQuality pick, Grim Dawn.exe sub_1400C4720 case 5).
/// PORT: textures already loaded keep the sampler (anisotropy) and mip reduction they were created with, emitters
/// already running keep their density; the engine unloads all resources / reads the values every frame.
fn apply_quality(options: &GameOptions, grass: &mut gd_render::render::grass::GrassQuality) {
    use gd_formats::options::Value;
    gd_render::textures::set_anisotropy(options.int("anisotropicFiltering"));
    if let Some(Value::Quality(q)) = options.get("textureQuality") {
        gd_render::textures::set_texture_quality(q);
    }
    if let Some(Value::Quality(q)) = options.get("fxQuality") {
        gd_render::fx::sim::set_fx_quality(q);
    }
    if let Some(Value::Quality(q)) = options.get("detailLevel") {
        let high = gd_render::render::shadow::quality_index(q) > 0;
        if grass.0 != high {
            grass.0 = high;
        }
    }
}

/// `screenMode` (int 5) as a window mode. Grim Dawn.exe sub_14000DC10 / sub_14000E9F0: 0 is exclusive fullscreen, 1 a decorated
/// window, 2 a borderless window at monitor size (anything else counts as 0 in the engine's `v == 1` / `(v & ~2) == 0` tests:
/// 1 windowed, 2 borderless, the rest fullscreen). PORT: exclusive fullscreen keeps the monitor's current video mode (the
/// back buffer is the monitor size, not `resolution`; Wayland has no mode switching and Bevy falls back to borderless).
pub fn window_mode(screen_mode: i32) -> bevy::window::WindowMode {
    use bevy::window::{MonitorSelection, VideoModeSelection, WindowMode};
    match screen_mode {
        1 => WindowMode::Windowed,
        2 => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
        _ => WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current),
    }
}

/// The `resolution` option (Int2 0, width and height) as a window size; `None` for a size no display has (the engine
/// falls back to 1024 x 768 / the monitor size, PORT: the window keeps its size).
pub fn window_size(width: i32, height: i32) -> Option<(u32, u32)> {
    ((640..=16384).contains(&width) && (480..=16384).contains(&height))
        .then_some((width as u32, height as u32))
}

/// `uiScale` (float 11): the slider in [0, 1]; it counts in game, the menus use 0.5
/// (the default). PORT: it takes effect on Apply, not the frame the slider moves; the UI reads it when built, and the
/// built HUD, windows and panes carry the scale factor in their rebuild signatures, so an Apply rebuilds what is shown.
#[derive(Resource, Clone, Copy, PartialEq)]
pub struct UiScaleOption(pub f32);

impl Default for UiScaleOption {
    fn default() -> Self {
        Self(0.5)
    }
}

impl UiScaleOption {
    /// The UI scale factor at a `width` × `height` physical-pixel screen.
    pub fn factor(self, width: u32, height: u32) -> f32 {
        gd_formats::ui::ui_scale_factor(width, height, self.0)
    }
}

fn apply_ui_scale(
    options: Option<Res<GameOptions>>,
    state: Res<State<crate::session::AppState>>,
    mut scale: ResMut<UiScaleOption>,
) {
    let v = match (options, state.get()) {
        (Some(o), crate::session::AppState::InGame) => o.float("uiScale").clamp(0.0, 1.0),
        _ => 0.5,
    };
    scale.set_if_neq(UiScaleOption(v));
}

/// Writes the options to the port's file (never into the game's folders).
pub fn store(options: &GameOptions) {
    gd_game::session::write_user_file("options", &path(), options.write());
}
