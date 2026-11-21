//! The credits roll (the main menu's Credits button, menu state 9).
//!
//! Grim Dawn.exe (sub_1400BE320 case 9, sub_1400A6440 window, parser sub_1400A6AF0, draw sub_1400A7620, update
//! sub_1400A7B30): the file `text_en/credits` (UTF-8, not localised), one entry per line,
//! its first four characters `<h1>` .. `<h5>` pick the style (`style_mainmenu_creditsh1` .. `h4`, `creditsbody1` for h5;
//! sizes 40, 34, 34, 32, 24) and are stripped, any other line (blank ones too) is the default style; a line advances
//! `floor(size * 1.1)` px (a blank line 17), centred on the screen's middle. The roll starts with the first line at
//! the screen's height and rises 0.05994 px/ms; Space toggles 4x; lines fade in over y/H 0.2 to 0.4 and out over 0.7 to
//! 0.9; it starts again when the last line has left the top (never ends by itself). The 3D menu scene stays behind. The
//! menu music fades out over 2.5 s while `Sound/Music/Music_Credits.ogg` loops; leaving (the Back button, `tagMenuButton04`,
//! or Esc) fades it back in. A mouse click elsewhere does nothing. Back activates on the left press (the shared rule,
//! `widgets::button_pressed`), read from the raw mouse messages: the menu's `swallow_input` has already reset this
//! frame's `ButtonInput` when the roll runs.
//! PORT: the clip band (80 px from the top, 100 from the bottom) skips whole lines; the Back button is a text button in
//! the h2 style at the bottom left; a closed roll keeps nothing (the offset restarts at the screen height).
use super::UiAssets;
use crate::data::GameData;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::Rect as URect;

const STYLES: [&str; 5] = [
    "records/ui/styles/text/style_mainmenu_creditsh1.dbr",
    "records/ui/styles/text/style_mainmenu_creditsh2.dbr",
    "records/ui/styles/text/style_mainmenu_creditsh3.dbr",
    "records/ui/styles/text/style_mainmenu_creditsh4.dbr",
    "records/ui/styles/text/style_mainmenu_creditsbody1.dbr",
];
const SIZES: [f32; 5] = [40.0, 34.0, 34.0, 32.0, 24.0];
const BLANK: f32 = 17.0;
const SPEED: f32 = 0.05994;
const FAST: f32 = 0.23976;
const MUSIC: &str = "sound/music/music_credits.ogg";
/// The menu music's fade, per ms (`0.0004`: 2.5 s).
const FADE: f32 = 0.0004;

#[derive(Resource)]
pub struct Credits {
    pub open: bool,
    offset: f32,
    fast: bool,
    /// (style index or `None` for the default, text).
    lines: Option<Vec<(Option<usize>, String)>>,
    music: Option<Entity>,
    /// The menu music's gain factor (1 normal, 0 while the roll plays) and its own base volume.
    menu_gain: f32,
    menu_base: Option<f32>,
    back: Option<URect>,
    was_open: bool,
}

/// The menu music starts at full gain: after Exit to Main Menu (`ui::reset_session` inserts the default) it plays
/// at once instead of fading in from silence.
impl Default for Credits {
    fn default() -> Self {
        Self {
            open: false,
            offset: 0.0,
            fast: false,
            lines: None,
            music: None,
            menu_gain: 1.0,
            menu_base: None,
            back: None,
            was_open: false,
        }
    }
}

#[derive(Component)]
struct CreditsRoot;

pub struct CreditsPlugin;

impl Plugin for CreditsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Credits>().add_systems(
            Update,
            roll.run_if(resource_exists::<crate::menu::MainMenu>),
        );
    }
}

fn parse(text: &str) -> Vec<(Option<usize>, String)> {
    text.lines()
        .map(|l| {
            for (i, tag) in ["<h1>", "<h2>", "<h3>", "<h4>", "<h5>"].iter().enumerate() {
                if let Some(rest) = l.strip_prefix(tag) {
                    return (Some(i), rest.to_owned());
                }
            }
            // `<img` lines have no text; anything else is shown in the default style.
            if l.starts_with("<img") {
                (None, String::new())
            } else {
                (None, l.to_owned())
            }
        })
        .collect()
}

/// A line's advance in px at UI scale `s`.
fn advance(style: Option<usize>, s: f32) -> f32 {
    ((style.map_or(BLANK, |i| SIZES[i] * 1.1)) * s).floor()
}

/// The roll's alpha at `y / H` (`sub_1400A7620`).
fn alpha(r: f32) -> f32 {
    if r > 0.7 {
        1.0 - ((r - 0.7) * 5.0).min(1.0)
    } else if r < 0.4 {
        ((r - 0.2) * 5.0).max(0.0)
    } else {
        1.0
    }
}

#[allow(clippy::too_many_arguments)]
fn roll(
    mut commands: Commands,
    time: Res<Time<Real>>,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    mut credits: ResMut<Credits>,
    mut typed: MessageReader<KeyboardInput>,
    mut clicks: MessageReader<bevy::input::mouse::MouseButtonInput>,
    scene: Option<Res<crate::menu_scene::MenuScene>>,
    mut volumes: Query<&mut crate::audio::BaseVolume>,
    mut sounds: ResMut<crate::audio::Sounds>,
    roots: Query<Entity, With<CreditsRoot>>,
    mut assets: UiAssets,
    (hooks, mut hooked): (Res<crate::hooks::Hooks>, Local<bool>),
    (mut built, mut fader, mut nodes): (
        Local<Option<Built>>,
        super::quicktip::Fader,
        Query<&mut Node, With<super::quicktip::FadeGroup>>,
    ),
) {
    // `GD_MENU_CREDITS` (hooks.rs): opens the roll once.
    if hooks.ui.menu_credits && !*hooked {
        *hooked = true;
        credits.open = true;
    }
    let dt = time.delta_secs() * 1000.0;
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let (w, h) = (size.x as f32, size.y as f32);
    // The press from the raw messages (module doc); every message is read, so none carries over a frame.
    let pressed = clicks
        .read()
        .filter(|e| e.button == MouseButton::Left && e.state == bevy::input::ButtonState::Pressed)
        .count()
        > 0;
    let menu_music = scene.as_ref().and_then(|s| s.music());
    // The menu music's fade toward its target (the roll plays: 0).
    let target = if credits.open { 0.0 } else { 1.0 };
    if credits.menu_gain != target {
        let step = dt * FADE;
        credits.menu_gain = if credits.menu_gain < target {
            (credits.menu_gain + step).min(target)
        } else {
            (credits.menu_gain - step).max(target)
        };
    }
    if let Some(e) = menu_music
        && let Ok(mut v) = volumes.get_mut(e)
    {
        let base = *credits.menu_base.get_or_insert(v.0);
        if credits.menu_gain < 1.0 || v.0 != base {
            v.0 = base * credits.menu_gain;
        }
    }
    // Opening: the text, the offset and the music.
    if credits.open && !credits.was_open {
        credits.lines = data
            .vfs
            .read("text_en/credits")
            .ok()
            .map(|b| parse(&String::from_utf8_lossy(&b)));
        credits.offset = h;
        credits.fast = false;
        if credits.menu_gain == 0.0 || credits.menu_gain == 1.0 {
            credits.menu_gain = 1.0;
        }
        if let Some(f) = sounds.file(&data, MUSIC) {
            let speed = f.speed(crate::audio::ENGINE_HZ);
            credits.music = Some(crate::audio::spawn_2d(
                &mut commands,
                crate::audio::SoundClass::Music,
                0.6,
                speed,
                f,
                true,
            ));
        }
    }
    // Closing: the roll's music goes (a fade of 2.5 s in the engine; here it ends at once).
    if !credits.open && credits.was_open {
        if let Some(e) = credits.music.take() {
            commands.entity(e).try_despawn();
        }
        credits.menu_gain = credits.menu_gain.max(0.001);
    }
    credits.was_open = credits.open;
    if !credits.open {
        if let Some(b) = built.take() {
            commands.entity(b.root).try_despawn();
        }
        credits.back = None;
        typed.clear();
        return;
    }
    for k in typed.read() {
        if !k.state.is_pressed() {
            continue;
        }
        match &k.logical_key {
            Key::Escape => credits.open = false,
            Key::Space => credits.fast = !credits.fast,
            _ => {}
        }
    }
    if pressed
        && let (Some(r), Some(c)) = (credits.back, super::cursor_px(&hooks, win))
        && super::contains(&r, c)
    {
        credits.open = false;
    }
    if !credits.open {
        return;
    }
    // The roll rises; after the last line it starts again at the bottom.
    let s = assets.scale(size);
    let inv = 1.0 / win.scale_factor();
    let speed = if credits.fast { FAST } else { SPEED };
    credits.offset -= dt * speed;
    // The root and the Back button once per (screen, scale); the line tops at that scale.
    let key = (size, s.to_bits());
    if built
        .as_ref()
        .is_none_or(|b| b.key != key || !roots.contains(b.root))
    {
        if let Some(b) = built.take() {
            commands.entity(b.root).try_despawn();
        }
        let root = crate::ui::spawn_root(&mut commands, CreditsRoot, 121);
        commands.entity(root).insert(super::hud::KeepsShown);
        let mut ui = assets.ui(&mut commands, &data, root, s, inv, Vec2::new(w, h));
        // The Back button (drawn whether or not the text loaded: it is the way out besides Esc).
        let back = ui.fonts.tag("tagMenuButton04").to_owned();
        let (bw, bh) = (ui.text_width(STYLES[1], &back), ui.line_height(STYLES[1]));
        let r = URect::new(10.0 * s, h - 30.0 * s - bh, bw, bh);
        ui.text_line(STYLES[1], Vec2::new(r.x, r.y), &back, 1.0);
        credits.back = Some(r);
        let mut y = 0.0;
        let tops = credits.lines.as_deref().map_or_else(Vec::new, |lines| {
            lines
                .iter()
                .map(|(style, _)| {
                    let t = y;
                    y += advance(*style, s);
                    t
                })
                .collect()
        });
        *built = Some(Built {
            key,
            root,
            tops,
            total: y,
            shown: Vec::new(),
        });
    }
    let Some(b) = built.as_mut() else { return };
    let Some(lines) = credits.lines.as_deref() else {
        return;
    };
    // Each line in the band is its own fade group, spawned when it enters and despawned when it leaves; the
    // group's `top` and alpha move in place.
    let off = credits.offset.floor();
    let mut want: Vec<(usize, f32, f32)> = Vec::new();
    for (i, ((style, text), top)) in lines.iter().zip(&b.tops).enumerate() {
        let y = off + top;
        if y > h {
            break;
        }
        let adv = advance(*style, s);
        if y > 80.0 && y + adv < h - 100.0 && !text.is_empty() {
            let a = alpha(y / h);
            if a > 0.0 {
                want.push((i, y, a));
            }
        }
    }
    b.shown.retain(|(i, e)| {
        let keep = want.iter().any(|w| w.0 == *i);
        if !keep {
            commands.entity(*e).try_despawn();
        }
        keep
    });
    let mut missing = Vec::new();
    for (i, y, a) in want {
        let top = Val::Px(y * inv);
        if let Some((_, e)) = b.shown.iter().find(|x| x.0 == i) {
            if let Ok(mut n) = nodes.get_mut(*e)
                && n.top != top
            {
                n.top = top;
            }
            fader.set(*e, a);
        } else {
            missing.push((i, top, a));
        }
    }
    if !missing.is_empty() {
        let mut ui = assets.ui(&mut commands, &data, b.root, s, inv, Vec2::new(w, h));
        for (i, top, a) in missing {
            let (style, text) = &lines[i];
            let st = STYLES[style.unwrap_or(4)];
            let g = super::quicktip::fade_group(&mut ui, b.root, a);
            ui.commands.entity(g).insert(Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                top,
                ..default()
            });
            let tw = ui.text_width(st, text);
            ui.text_line(st, Vec2::new((w * 0.5 - tw * 0.5).round(), 0.0), text, 1.0);
            super::quicktip::seal_fade_group(ui.commands, g);
            b.shown.push((i, g));
        }
    }
    if off + b.total < 0.0 {
        credits.offset = h;
    }
}

/// The built roll: its root, each line's top at offset 0 (physical px) and the lines in the band.
struct Built {
    key: (UVec2, u32),
    root: Entity,
    tops: Vec<f32>,
    total: f32,
    /// (line index, its fade group).
    shown: Vec<(usize, Entity)>,
}
