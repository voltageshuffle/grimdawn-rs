//! The port's own settings file: `$XDG_DATA_HOME/grimdawn-rs/Settings/options.txt`
//! (default `~/.local/share/grimdawn-rs/Settings/options.txt`), in the game's
//! `options.txt` format (`gd_game_core::options`). Read before start-up into
//! [`GameOptions`]; written by the options window's Apply.
//!
//! `GD_OPTIONS` (hooks.rs) reads another options file instead; Apply still
//! writes the port's own file only.
//!
//! The `language` option ([`apply_language`]) selects the localisation tags and fonts live
//! (`gd_game_core::data::GameData::set_language`); the story snapshots its tags, the quest / dialog text
//! overrides (`text_<code>/<basename>.txt`) and the `language.def` paging separators per session
//! (`gd_game::story::setup_story`), so they follow on the next session. PORT: `intro.txt` cinematic
//! subtitles (the port plays no videos) and CJK font word-wrap (`language.def` `wordmode`) are out
//! of scope.
use bevy::prelude::*;
use gd_game_core::options::GameOptions;
use gd_ui_kit::state::settings::{UiScaleOption, path};
use std::path::PathBuf;

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiScaleOption>()
            .add_systems(PreStartup, load)
            .add_systems(Update, (apply_video, apply_ui_scale, apply_language));
    }
}

fn load(
    mut commands: Commands,
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut grass: ResMut<gd_render::render::grass::GrassQuality>,
    mut data: ResMut<gd_game_core::data::GameData>,
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
            data.set_language(language_code(&options));
            commands.insert_resource(options);
        }
        Err(_) => info!("options: {} not found, engine defaults", file.display()),
    }
}

/// The `language` option's stored name (e.g. "English") as its archive code
/// (`gd_formats::text::LANGUAGES`); "en" for an unknown name.
fn language_code(options: &GameOptions) -> &'static str {
    let name = match options.get("language") {
        Some(gd_formats::options::Value::Str(s)) => s.as_str(),
        _ => "English",
    };
    gd_formats::text::LANGUAGES
        .iter()
        .find(|l| l.option == name)
        .map_or("en", |l| l.code)
}

/// The `language` option: the engine reads it at start-up (`LocalizationManager::Load`); the port
/// also follows it live on Apply — the UI's tags and fonts switch with it (`ui::load_tags` reloads
/// them and every UI group rebuilds). The story snapshots its tags, quest / dialog text overrides
/// and paging separators per session (`gd_game::story::setup_story`), so they follow on the next
/// session (module doc).
fn apply_language(
    options: Option<Res<GameOptions>>,
    mut data: ResMut<gd_game_core::data::GameData>,
) {
    let Some(options) = options.filter(|o| o.is_changed()) else {
        return;
    };
    data.set_language(language_code(&options));
}

/// The video options this port honours (`GraphicsEngine::UpdateFromOptions` runs every frame):
/// `syncToRefresh` is vsync; `screenMode` and `resolution` (see [`window_mode`],
/// [`window_size`]); `textureQuality` (mip reduction, read by newly loaded textures: `textures.rs`); `shadowQuality` /
/// `shadows` (`render/shadow.rs`); `detailLevel`'s terrain quality (grass of levels built from now on, `render/grass.rs`); `weatherEnabled` / `weatherQuality` (`weather.rs`, read every frame). `fxaa` (`render/post.rs`, every frame); `softParticles` (`weather.rs`). `antiAliasing` is not stored: it has no effect in the engine's
/// deferred renderer (`GraphicsEngine::BuildDeviceSettings` zeroes the sample count, `render/post.rs`); the options this port does not use keep their file lines (`gd_game_core::options`). Elsewhere: `lightingQuality` (`render/light_rig.rs`, `render/point_shadow.rs`, `render/env.rs`), `detailObjects` (decals: `decal.rs`, `casting.rs`), `ambientocclusion` (`render/post.rs`), `hdrLighting` / `advancedEffects` (`render/env.rs`), `reflectionQuality` (the water's planar reflection, `render/reflection.rs`), `uiScale` ([`apply_ui_scale`]). PORT: with no options file the engine defaults apply on frame 1 (`GameOptions` is default-initialized), so the first run is fullscreen at `screenMode` 0 with the resolution auto-detected from the effective primary monitor ([`primary_monitor`], engine parity), not a windowed placeholder; screenshot runs and `GD_RES` keep their window.
fn apply_video(
    options: Option<Res<GameOptions>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    monitors: Query<(
        Entity,
        &bevy::window::Monitor,
        Option<&bevy::window::PrimaryMonitor>,
    )>,
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
    }
    // The factory default (0, 0) or an out-of-clamp size falls back to the effective primary monitor's physical
    // size ([`primary_monitor`], the engine's fallback); with no monitor (headless) the window keeps its size.
    .or_else(|| {
        primary_monitor(&monitors).map(|m| {
            let s = m.physical_size();
            (s.x, s.y)
        })
    });
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

/// The options applied live on every change. `GraphicsEngine::UpdateFromOptions` (every frame:
/// texture reduction, detail level) and `RenderDevice` sampler setup (anisotropy from `anisotropicFiltering`, applied
/// every frame), `Engine::SetFxQualityModifier` (called on a fxQuality pick, Grim Dawn.exe
/// `OptionsWindow::HandleVideoPick` case 5).
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

/// `screenMode` as a window mode. Grim Dawn.exe `Game::Initialize` / `Game::MainLoop`: 0 is exclusive
/// fullscreen, 1 a decorated
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

/// The `resolution` option (Int2 0, width and height) as a window size; `None` for the factory default (0, 0) or a
/// size outside the clamp — [`apply_video`] then uses the effective primary monitor's physical size
/// ([`primary_monitor`], the engine's fallback), keeping the window's size when there is no monitor.
pub fn window_size(width: i32, height: i32) -> Option<(u32, u32)> {
    ((640..=16384).contains(&width) && (480..=16384).contains(&height))
        .then_some((width as u32, height as u32))
}

/// The effective primary monitor: the `PrimaryMonitor`-marked one when the platform reports it, else the first
/// monitor in a stable order (top-left-most `physical_position`, then entity).
/// PORT: Wayland exposes no primary monitor — winit's `primary_monitor()` returns None, so bevy_winit marks no
/// entity `PrimaryMonitor` and a `With<PrimaryMonitor>` query is empty there; the engine's "primary monitor" is a
/// Windows concept. The deterministic fallback keeps the resolution list / auto-detect working on a Wayland
/// session (and is unchanged where a primary monitor exists).
pub fn primary_monitor<'a>(
    monitors: &'a Query<(
        Entity,
        &bevy::window::Monitor,
        Option<&bevy::window::PrimaryMonitor>,
    )>,
) -> Option<&'a bevy::window::Monitor> {
    let mut best: Option<(i32, i32, Entity, &bevy::window::Monitor)> = None;
    for (e, m, primary) in monitors.iter() {
        if primary.is_some() {
            return Some(m);
        }
        let key = (m.physical_position.x, m.physical_position.y, e);
        if best
            .as_ref()
            .is_none_or(|(x, y, be, _)| key < (*x, *y, *be))
        {
            best = Some((key.0, key.1, e, m));
        }
    }
    best.map(|(_, _, _, m)| m)
}

fn apply_ui_scale(
    options: Option<Res<GameOptions>>,
    state: Res<State<gd_ui::state::session::AppState>>,
    mut scale: ResMut<UiScaleOption>,
) {
    let v = match (options, state.get()) {
        (Some(o), gd_ui::state::session::AppState::InGame) => o.float("uiScale").clamp(0.0, 1.0),
        _ => 0.5,
    };
    scale.set_if_neq(UiScaleOption(v));
}
