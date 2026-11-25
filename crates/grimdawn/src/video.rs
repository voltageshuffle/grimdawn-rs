//! Video playback (Engine.dll `TheoraVideoPlayer`, exe `Game::LoadModMap` / `Game::UpdateVideo`): the new character's intro
//! and the quest action `PlayVideo` (the expansion intros).
//!
//! The engine keeps a FIFO of video paths. `Game::LoadModMap` queues `video/intro.ogg` for a character that has not
//! been in the game, in single player and without a mod; `ScriptableAction_PlayVideo::Execute` queues its path.
//! One plays at a time (`Game::OpenVideo`: the sounds are paused, `Play`, the screen switches to the video mode);
//! it ends with its stream, or at the first key press, key action or mouse click (`InGameUI::HandleKeyEvent` /
//! `HandleMouseEvent` / `HandleKeyAction`, which swallow the event); then the sounds go on and the next one starts.
//! The file is opened relative to the install (no archive, so no mod replaces it). Its picture is drawn centred,
//! scaled to fit the screen with the bands black (`TheoraVideoPlayer::Render`); its subtitles are
//! `<video>.txt` (or `text_<language>/<name>.txt` in another language), `gd_formats::video::subtitles`. The clock is
//! the sound stream's played time (`GetExternalStreamTime`).
//!
//! Subtitles (`TheoraVideoPlayer::Update`): a line shows from its time on, fades in at 2 per second, and fades out
//! the same way once it is 5 s old; `style_videosubtitle`'s font, at most `min(1, width / 1920)` of its size,
//! centred, the newest line 50 px above the bottom. PORT: two lines at most show together, the older above
//! (the engine's list scrolls them with an animated offset).
//!
//! PORT: the decoder runs on its own thread and stays at most [`AHEAD`] pictures in front of the clock; the
//! file's sound is queued into the mixer as it is decoded ([`gd_audio::LiveAudio`], a voice the pause does not
//! hold) and `GamePause::video` holds the world. A mixer that does not move (no sound device) lets the clock
//! follow the wall time instead. The engine's `PlayVideo` flag is not read in single player. A session that
//! skips the menu plays no intro; `GD_VIDEO=<path>` plays that video at the player's entry.
use bevy::asset::RenderAssetUsages;
use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use gd_audio::{LiveAudio, SoundClass};
use gd_formats::video::{Event, Video};
use gd_game_core::data::GameData;
use gd_game_core::entry::EnteredGame;
use gd_game_core::pause::GamePause;
use gd_game_core::story::StoryEffect;
use gd_story::story::Effect;
use gd_ui::state::session::AppState;
use gd_ui_kit::UiAssets;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

/// The new character's intro (`Game::LoadModMap`).
const INTRO: &str = "video/intro.ogg";
const STYLE: &str = "records/ui/styles/text/style_videosubtitle.dbr";
/// Above the HUD, the windows and the loading cover; the cursor stays on top.
const Z: i32 = 500;
/// Decoded pictures the thread holds in front of the clock.
const AHEAD: usize = 4;
/// A subtitle line's hold before it fades out, and the fades' rate per second (`TheoraVideoPlayer::Update`).
const SUB_HOLD: f32 = 5.0;
const SUB_FADE: f32 = 2.0;
const SUB_LINES: usize = 2;
/// Seconds the sound stream may stand still before the clock follows the wall time.
const STALL: f32 = 0.25;
/// A picture shows this early (one frame at 24 fps).
const FRAME: f32 = 1.0 / 24.0;

pub struct VideoPlugin;

impl Plugin for VideoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VideoQueue>()
            .add_systems(PreUpdate, skip.after(InputSystems))
            .add_systems(
                Update,
                (
                    queue_intro.run_if(resource_added::<EnteredGame>),
                    queue_story,
                    start,
                    pump,
                )
                    .chain(),
            );
    }
}

/// The paths waiting to play (the engine's FIFO).
#[derive(Resource, Default)]
pub struct VideoQueue(VecDeque<String>);

enum Msg {
    /// The picture size and the sound's rate (none: no sound track), before anything else.
    Header {
        size: UVec2,
        rate: Option<u32>,
    },
    /// A picture (RGBA) that ends at `time` seconds.
    Frame {
        time: f32,
        rgba: Vec<u8>,
    },
    /// Interleaved stereo samples.
    Audio(Vec<f32>),
    End,
}

/// The video on screen.
#[derive(Resource)]
pub struct Playing {
    /// Behind a mutex only to make the resource `Sync`.
    rx: Mutex<mpsc::Receiver<Msg>>,
    stop: Arc<AtomicBool>,
    /// Pictures sent and not yet shown or dropped.
    ahead: Arc<AtomicUsize>,
    root: Entity,
    live: Option<LiveAudio>,
    image: Option<(Handle<Image>, Entity, UVec2)>,
    frames: VecDeque<(f32, Vec<u8>)>,
    clock: f32,
    last_played: f32,
    stall: f32,
    ended: bool,
    subs: Vec<(f32, String)>,
    shown: Vec<(usize, Entity)>,
    skipped: bool,
}

impl Drop for Playing {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// `Game::LoadModMap`: a character that has not been in the game gets the intro, in single player without a mod.
fn queue_intro(
    mut queue: ResMut<VideoQueue>,
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    data: Res<GameData>,
    loaded: Option<Res<gd_game_core::session::LoadedCharacter>>,
) {
    // A session that skips the menu plays no intro.
    #[cfg(feature = "dev")]
    if let Some(video) = &hooks.session.video {
        queue.0.push_back(video.clone());
        return;
    } else if hooks.session.skip_menu {
        return;
    }
    if data.active_mod().is_none() && gd_game::entry::first_time_in_game(loaded.as_deref()) {
        queue.0.push_back(INTRO.to_owned());
    }
}

/// `ScriptableAction_PlayVideo::Execute`.
fn queue_story(mut effects: MessageReader<StoryEffect>, mut queue: ResMut<VideoQueue>) {
    for e in effects.read() {
        if let Effect::PlayVideo { video } = &e.0 {
            queue.0.push_back(video.clone());
        }
    }
}

/// Any key press or click ends the video and is not seen by the game.
fn skip(
    playing: Option<ResMut<Playing>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
) {
    let Some(mut p) = playing else { return };
    if keys.get_just_pressed().next().is_some() || mouse.get_just_pressed().next().is_some() {
        p.skipped = true;
        keys.clear();
        mouse.clear();
    }
}

/// The subtitle file's text for the video at the engine path `video`: `text_<language>/<name>.txt` of the archives
/// when the language is not the default and has one, else `<video>.txt` beside it (found ignoring case).
fn subtitle_text(data: &GameData, video: &str) -> String {
    let txt = std::path::Path::new(video).with_extension("txt");
    let txt = txt.to_string_lossy().replace('\\', "/");
    let name = txt.rsplit('/').next().unwrap_or(&txt);
    let lang = data.language();
    if lang != "en"
        && let Ok(b) = data.vfs.read(&format!("text_{lang}/{name}"))
    {
        return String::from_utf8_lossy(&b).into_owned();
    }
    gd_formats::video::find(data.root(), &txt)
        .and_then(|p| Ok(std::fs::read_to_string(p)?))
        .unwrap_or_default()
}

/// Starts the next queued video when none plays (a missing path is skipped with a warning).
fn start(
    mut commands: Commands,
    mut queue: ResMut<VideoQueue>,
    playing: Option<Res<Playing>>,
    data: Res<GameData>,
    mut pause: ResMut<GamePause>,
) {
    if playing.is_some() {
        return;
    }
    while let Some(path) = queue.0.pop_front() {
        let file = match gd_formats::video::find(data.root(), &path) {
            Ok(f) => f,
            Err(e) => {
                warn!("video {path}: {e:#}");
                continue;
            }
        };
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let ahead = Arc::new(AtomicUsize::new(0));
        let spawned = std::thread::Builder::new().name("video".into()).spawn({
            let (file, stop, ahead) = (file.clone(), stop.clone(), ahead.clone());
            move || decode(&file, &tx, &ahead, &stop)
        });
        if let Err(e) = spawned {
            warn!("video {path}: {e}");
            continue;
        }
        info!("playing video {path}");
        let root = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::BLACK),
                GlobalZIndex(Z),
                DespawnOnExit(AppState::InGame),
            ))
            .id();
        commands.insert_resource(Playing {
            rx: Mutex::new(rx),
            stop,
            ahead,
            root,
            live: None,
            image: None,
            frames: VecDeque::new(),
            clock: 0.0,
            last_played: 0.0,
            stall: 0.0,
            ended: false,
            subs: gd_formats::video::subtitles(&subtitle_text(&data, &path)),
            shown: Vec::new(),
            skipped: false,
        });
        pause.video = true;
        return;
    }
    // Read first: a write marks the resource changed.
    if pause.video {
        pause.video = false;
    }
}

/// The decoder thread: demuxes and decodes the file, keeping [`AHEAD`] pictures in front of the clock.
fn decode(file: &std::path::Path, tx: &mpsc::Sender<Msg>, ahead: &AtomicUsize, stop: &AtomicBool) {
    let run = || -> anyhow::Result<()> {
        let f = std::fs::File::open(file)?;
        let mut v = Video::new(std::io::BufReader::with_capacity(1 << 20, f));
        let mut header = false;
        let mut rgba = Vec::new();
        while let Some(e) = v.step()? {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            if !header && let Some((w, h)) = v.picture_size() {
                header = true;
                let rate = v.audio().map(|(_, r)| r);
                rgba = vec![0; w as usize * h as usize * 4];
                if tx
                    .send(Msg::Header {
                        size: UVec2::new(w, h),
                        rate,
                    })
                    .is_err()
                {
                    return Ok(());
                }
            }
            match e {
                Event::Frame { time, new } => {
                    if !new {
                        continue;
                    }
                    while ahead.load(Ordering::Relaxed) >= AHEAD {
                        if stop.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        std::thread::sleep(std::time::Duration::from_millis(4));
                    }
                    v.picture_rgba(&mut rgba)?;
                    ahead.fetch_add(1, Ordering::Relaxed);
                    let frame = Msg::Frame {
                        time: time as f32,
                        rgba: rgba.clone(),
                    };
                    if tx.send(frame).is_err() {
                        return Ok(());
                    }
                }
                Event::Audio(s) => {
                    let channels = v.audio().map_or(2, |(c, _)| c as usize);
                    let s = if channels == 1 {
                        s.iter().flat_map(|&x| [x, x]).collect()
                    } else {
                        s
                    };
                    if tx.send(Msg::Audio(s)).is_err() {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    };
    if let Err(e) = run() {
        warn!("video {}: {e:#}", file.display());
    }
    // The player is gone (the video was skipped or the session ended): nobody to tell.
    if tx.send(Msg::End).is_err() {
        debug!("video {}: the player is gone", file.display());
    }
}

/// Takes what the thread sent, moves the clock, shows the picture and the subtitles, ends the video.
#[allow(clippy::too_many_arguments)]
fn pump(
    mut commands: Commands,
    playing: Option<ResMut<Playing>>,
    real: Res<Time<Real>>,
    pause: Res<GamePause>,
    data: Res<GameData>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut nodes: Query<&mut Node>,
    mut tints: (Query<&mut ImageNode>, Query<&mut TextColor>),
    children: Query<&Children>,
    mut assets: UiAssets,
) {
    let Some(mut p) = playing else { return };
    let Ok(win) = window.single() else { return };
    if p.skipped || (p.ended && p.frames.is_empty()) {
        commands.entity(p.root).despawn();
        info!("video {}", if p.skipped { "skipped" } else { "ended" });
        commands.remove_resource::<Playing>();
        return;
    }
    let p = &mut *p;
    let msgs: Vec<Msg> = p.rx.get_mut().try_iter().collect();
    for m in msgs {
        match m {
            Msg::Header { size, rate } => {
                if let Some(rate) = rate {
                    let live = LiveAudio::new(rate);
                    let voice =
                        gd_audio::spawn_live(&mut commands, SoundClass::Effects, 1.0, live.clone());
                    commands.entity(p.root).add_child(voice);
                    p.live = Some(live);
                }
                let img = Image::new_fill(
                    Extent3d {
                        width: size.x,
                        height: size.y,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    &[0, 0, 0, 255],
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                );
                let handle = assets.images.add(img);
                let node = commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            ..default()
                        },
                        ImageNode::new(handle.clone()),
                        ChildOf(p.root),
                    ))
                    .id();
                p.image = Some((handle, node, size));
            }
            Msg::Frame { time, rgba } => p.frames.push_back((time, rgba)),
            Msg::Audio(s) => {
                if let Some(l) = &p.live {
                    l.push(&s);
                }
            }
            Msg::End => {
                p.ended = true;
                if let Some(l) = &p.live {
                    l.close();
                }
            }
        }
    }
    // The sound's played time while it moves, the wall time once it stands still.
    let dt = real.delta_secs();
    match &p.live {
        Some(l) => {
            let played = l.played();
            if played != p.last_played {
                p.last_played = played;
                p.stall = 0.0;
                p.clock = played;
            } else {
                p.stall += dt;
                if p.stall >= STALL && !pause.unfocused {
                    p.clock += dt;
                }
            }
        }
        None if p.image.is_some() => p.clock += dt,
        None => {}
    }
    // The newest due picture; the node fits the window, centred.
    let Some((handle, node, size)) = p.image.clone() else {
        return;
    };
    let mut due = None;
    while p.frames.front().is_some_and(|f| f.0 <= p.clock + FRAME) {
        due = p.frames.pop_front();
        p.ahead.fetch_sub(1, Ordering::Relaxed);
    }
    if let Some((_, rgba)) = due
        && let Some(mut img) = assets.images.get_mut(&handle)
    {
        img.data = Some(rgba);
    }
    let (w, h) = (win.width(), win.height());
    let fit = (w / size.x as f32).min(h / size.y as f32);
    let (nw, nh) = (size.x as f32 * fit, size.y as f32 * fit);
    if let Ok(mut n) = nodes.get_mut(node) {
        let (left, top, width, height) = (
            Val::Px(((w - nw) * 0.5).floor()),
            Val::Px(((h - nh) * 0.5).floor()),
            Val::Px(nw.ceil()),
            Val::Px(nh.ceil()),
        );
        if n.left != left || n.top != top || n.width != width || n.height != height {
            (n.left, n.top, n.width, n.height) = (left, top, width, height);
        }
    }
    let alive = |t: f32| p.clock >= t && p.clock < t + SUB_HOLD + 1.0 / SUB_FADE;
    let mut want: Vec<usize> = (0..p.subs.len()).filter(|&i| alive(p.subs[i].0)).collect();
    if want.len() > SUB_LINES {
        want.drain(..want.len() - SUB_LINES);
    }
    if !want.iter().copied().eq(p.shown.iter().map(|s| s.0)) {
        for (_, e) in p.shown.drain(..) {
            commands.entity(e).despawn();
        }
        let screen = Vec2::new(win.physical_width() as f32, win.physical_height() as f32);
        let s = (screen.x / 1920.0).min(1.0);
        for (k, &i) in want.iter().enumerate() {
            let line = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    ChildOf(p.root),
                ))
                .id();
            let text = assets.fonts.tag(&p.subs[i].1).to_owned();
            let mut ui = assets.ui(
                &mut commands,
                &data,
                line,
                s,
                1.0 / win.scale_factor(),
                screen,
            );
            let lh = ui.line_height(STYLE);
            let width = ui.text_width(STYLE, &text);
            let up = (want.len() - 1 - k) as f32 * lh;
            ui.text_line_colored(
                STYLE,
                Vec2::new(
                    (screen.x * 0.5 - width * 0.5).round(),
                    (screen.y - 50.0 - up - lh * 0.5).round(),
                ),
                &text,
                Color::WHITE.with_alpha(0.0),
                None,
            );
            p.shown.push((i, line));
        }
    }
    for &(i, line) in &p.shown {
        let t = p.subs[i].0;
        let alpha = ((p.clock - t) * SUB_FADE)
            .min(1.0 - (p.clock - t - SUB_HOLD) * SUB_FADE)
            .clamp(0.0, 1.0);
        for c in children.get(line).into_iter().flatten() {
            if let Ok(mut img) = tints.0.get_mut(*c) {
                img.color.set_alpha(alpha);
            }
            if let Ok(mut text) = tints.1.get_mut(*c) {
                text.0.set_alpha(alpha);
            }
        }
    }
}
