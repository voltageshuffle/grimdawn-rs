//! The port's own settings file: `Settings/options.txt` in the port's data folder
//! (`gd_game_core::session::data_dir`), in the game's
//! `options.txt` format (`gd_game_core::options`). Read before start-up into
//! [`GameOptions`]; written by the options window's Apply.
//!
//! The `language` option ([`apply_language`]) selects the localisation tags and fonts live; the story
//! snapshots its tags, text overrides and paging separators per session, so they follow on the next
//! session. PORT: the `wordmode` flag is not honoured; CJK break-at-character wrapping is always on
//! (`gd_ui_kit::text`).
use bevy::prelude::*;
use gd_game_core::options::GameOptions;
use gd_ui_kit::state::settings::{UiScaleOption, path};

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
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    mut grass: ResMut<gd_render::render::grass::GrassQuality>,
    mut data: ResMut<gd_game_core::data::GameData>,
) {
    #[cfg(feature = "dev")]
    let file = hooks
        .session
        .options_file
        .as_ref()
        .map_or_else(path, std::path::PathBuf::from);
    #[cfg(not(feature = "dev"))]
    let file = path();

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

/// The `language` option. PORT: the engine reads it at start-up only (`LocalizationManager::Load`); the
/// port also follows it live on Apply.
fn apply_language(
    options: Option<Res<GameOptions>>,
    mut data: ResMut<gd_game_core::data::GameData>,
) {
    let Some(options) = options.filter(|o| o.is_changed()) else {
        return;
    };
    data.set_language(language_code(&options));
}

/// The video options (`GraphicsEngine::UpdateFromOptions` runs every frame): `syncToRefresh` is vsync;
/// `screenMode` and `resolution` (see [`window_mode`], [`window_size`]). `antiAliasing` has no effect in the
/// engine's deferred renderer (`GraphicsEngine::BuildDeviceSettings` zeroes the sample count).
/// With no options file the engine defaults apply: fullscreen at `screenMode` 0 with the resolution
/// auto-detected from [`primary_monitor`].
fn apply_video(
    options: Option<Res<GameOptions>>,
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
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
    #[cfg(feature = "dev")]
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
    // The factory default (0, 0) or an out-of-clamp size falls back to the primary monitor's physical size
    // (the engine's fallback); with no monitor the window keeps its size.
    .or_else(|| {
        primary_monitor(&monitors).map(|m| {
            let s = m.physical_size();
            (s.x, s.y)
        })
    });
    // Only on change: a window the player resized by hand keeps its size.
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
        #[cfg(feature = "dev")]
        let keep_size = hooks.render.res.is_some();
        #[cfg(not(feature = "dev"))]
        let keep_size = false;
        if w.mode != mode {
            w.mode = mode;
        }
        if mode == bevy::window::WindowMode::Windowed
            && !keep_size
            && let Some((width, height)) = size
            && w.resolution.physical_size() != UVec2::new(width, height)
        {
            w.resolution.set_physical_resolution(width, height);
        }
    }
}

/// The options applied live on every change (`GraphicsEngine::UpdateFromOptions`, `RenderDevice` sampler
/// anisotropy, `Engine::SetFxQualityModifier`).
/// PORT: textures already loaded keep their anisotropy and mip reduction, running emitters keep their
/// density; the engine reads the values every frame.
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

/// `screenMode` as a window mode (`Game::Initialize` / `Game::MainLoop`): 1 windowed, 2 borderless at monitor
/// size, anything else exclusive fullscreen. PORT: exclusive fullscreen keeps the monitor's current video mode.
pub fn window_mode(screen_mode: i32) -> bevy::window::WindowMode {
    use bevy::window::{MonitorSelection, VideoModeSelection, WindowMode};
    match screen_mode {
        1 => WindowMode::Windowed,
        2 => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
        _ => WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current),
    }
}

/// The `resolution` option (Int2 0) as a window size; `None` for the factory default (0, 0) or a size outside
/// the clamp.
pub fn window_size(width: i32, height: i32) -> Option<(u32, u32)> {
    ((640..=16384).contains(&width) && (480..=16384).contains(&height))
        .then_some((width as u32, height as u32))
}

/// The effective primary monitor: the `PrimaryMonitor`-marked one, else the first monitor in a stable order
/// (top-left-most `physical_position`, then entity). PORT: Wayland reports no primary monitor.
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
