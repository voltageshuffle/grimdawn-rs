//! The options window (the game menu's "Options Menu"): tabs Gameplay,
//! Interface, Health Bars, Key Bindings, Audio, Video (and the inert
//! Network tab), Default / Apply / Close. It edits a draft of
//! [`gd_game::options::GameOptions`]; Apply replaces the session's options
//! and writes the port's own `options.txt` (`crate::settings`).
//!
//! Records: `records/ui/mainmenu/mainmenu_constants.dbr`
//! (`optionsWindow` = `optionswindow/options_mastertable.dbr`: window 880 ×
//! 613 at (−440, −306) from the screen centre, page rect (15, 114) 850 × 420,
//! background `options_background.dbr`, title `tagMenu04` in
//! `style_difficultywindowtitle`, the tab buttons at their
//! `bitmapPosition` from the window's top-left (toggle buttons: the open
//! tab shows its down bitmap), Default / Apply / Close aligned Center /
//! Bottom: x = window centre + `bitmapPositionX`, top = window bottom −
//! `bitmapPositionY`; Apply's up bitmap is `buttonthin01_upfocus`, disabled
//! while nothing changed); `optionsCheckboxBitmap` (`buttoncheckbox*.tex`:
//! up / down = off / on), `optionsFontName` (labels,
//! `style_mainmenu_optionsheader01`), `dropboxBorder` / `dropboxBackColor`
//! / `dropboxButtonUpBitmap`, `dropboxListFont`, `slideboxBorder`
//! (`slider{left,center,right}.tex` track),
//! `optionsKeyBindingHeaderColor` (0.7, 0.55, 0.4, 0.25) and
//! `optionsKeyBindingHeaderFontName` for the bindings' header.
//!
//! Key bindings (Grim Dawn.exe): the list (sub_1400AF8F0) has the
//! columns `tagKeybindingTitle02..04` (Function / Primary / Secondary) 370 /
//! 170 / 170 × UI scale wide, rows in the order of sub_1400B0380 (the
//! move-forward / back / left / right rows only for the WASD movement type),
//! each action's tag from sub_1401F6310; the defaults are sub_1401F54E0's
//! (`keybindings.txt` "action: primary secondary" in the engine's key codes:
//! DirectInput scan codes, 107 / 118 right Ctrl / Alt, 145+ mouse wheel /
//! buttons and gamepad, named by `InputDevice::GetDefaultButtonName`,
//! Engine.dll @0x1801234a0). PORT: keyboard names are Wine's for the
//! scan code (the real list asks the OS). Clicking a Primary / Secondary
//! cell waits for a key (Esc cancels) and binds it; Apply stores the
//! bindings (`super::keys`, which also says which the port honours). The list's scroll bar
//! (its input sub_1400DB910): a press on the thumb grabs it, a move places it at the cursor less the grab
//! offset, the release lets go. PORT: the list scrolls by whole rows (the row nearest the thumb's place); a press on
//! the track does not page.
//!
//! Sounds (`ui/widgets.rs`): the tabs and Default / Apply / Close are `menubutton`s (their `soundNameOver` /
//! `soundNameDown`, Apply only while enabled); the check boxes play the constants' `gemButtonRolloverSound` /
//! `gemButtonClickSound` (sub_1400A0F70). The sliders and dropdowns are silent: their constructors (slider
//! sub_1400EAC40 from sub_1400C4DD0, dropdown sub_1400AA480 from the page builders) take a copy of the options window's
//! `SoundDescriptor` +320 / +432, which its constructor sub_1400EFEE0 default-builds and nothing loads (no
//! `SoundManager::Load` or assignment reaches them), and `mainmenu_constants.dbr` has no slider / dropbox sound field.
//!
//! Page layouts: page builder sub_1400C8E60, see
//! [`Page`]: each page lays its controls down a row cursor from the page origin (`options_mastertable.dbr` `pageX`
//! / `pageY` 15 / 114 in the window). PORT: the drawn widgets' insides (label offsets, the dropdown box drawn
//! `width - 3` wide with its arrow outside, the anti-aliasing box in the value column) are the port's own; a
//! dropdown opens a list of its
//! values under the box ([`choices`]); a slider is set by dragging its knob ([`knob_bitmap`], `input`); the
//! controls' rollovers: [`rollover`]; Default
//! resets the open page's options. Video dropdown lists (sub_1400C5190 jump table @0x1400C59B0, pick handler
//! sub_1400C4720, page builder sub_1400C8E60): textureQuality Low..High, fxQuality / lightingQuality Low..Very High,
//! detailLevel Low..Ultra, weather Off + Low..Very High (Off = `weatherEnabled` false), shadows Off + Low..Ultra (Off =
//! `shadows` false); the shown item is the stored quality clamped to the entries (a file's "ultra" texture quality shows
//! High and stays stored until a pick). The `reflectionQuality` row (tagVideoOption04) is a Low..Very High
//! dropdown driving the water's planar reflection (gd-render `render/reflection.rs`). PORT: the Video rows of
//! options the port does not use (`tripleBuffer`, `alphatocoverage`, `deferredrendering`) are not shown (their
//! rows stay empty); the options file keeps their lines (`gd_game::options`).
//!
//! Performance (PORT, `docs/architecture.md`): the rebuild signature holds only
//! layout state (size, open, tab, dropdown, rebind, movement type, keys
//! version); hover, slider drags, list scrolls and the draft's values never
//! rebuild — [`live`] swaps the state bitmaps ([`Look`]), moves the knobs and
//! the clipped list panes ([`Scroll`]) and sets the dropdown texts
//! ([`DropText`]) in place. So the hits of what moves (knobs, the list rows, the key list's thumb) are stored at
//! value / scroll 0 and [`hit_at`] places them where [`live`] draws them. The controls' rollovers are the menu manager's
//! shared box (`super::rollover::menu_tip`) with its 500 ms one-shot delay
//! ([`rollover`]).
mod bindings;
mod build;
mod input;
mod live;
mod pages;

use super::contains;
use super::keys::{KeyBindings, action_tag, key_name};
use super::window::Rebuild;
use super::{LiveText, TextAt, Ui, text::FontCache};
use crate::data::GameData;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bindings::*;
use build::*;
use gd_formats::arz::Record;
use gd_formats::options::Value;
use gd_formats::ui::{Rect as URect, TextAlign, WindowDef};
use gd_game::options::GameOptions;
pub use input::input;
use live::*;
use pages::*;

pub const RECORD: &str = "records/ui/mainmenu/optionswindow/options_mastertable.dbr";
const CONSTANTS: &str = "records/ui/mainmenu/mainmenu_constants.dbr";
/// The slider knob's bitmaps: up, held (dragged), hovered (the slider constructor sub_1400EAC40, hard-coded).
const KNOB: [&str; 3] = [
    "ui/mainmenu/buttongemup.tex",
    "ui/mainmenu/buttongemdown.tex",
    "ui/mainmenu/buttongemover.tex",
];
const LABEL_STYLE: &str = "records/ui/styles/text/style_mainmenu_optionsheader01.dbr";
const VALUE_STYLE: &str = "records/ui/styles/text/style_mainmenu_optionsdropdown01.dbr";
const TAB_FIELDS: [&str; 7] = [
    "gameplayButton",
    "interfaceButton",
    "healthBarsButton",
    "keybindingButton",
    "audioButton",
    "videoButton",
    "networkButton",
];
/// The reference shots' window origin at 1920 × 1080 (layouts below are in
/// those screen coordinates).
const ORIGIN: Vec2 = Vec2::new(520.0, 234.0);

pub struct OptionsUiPlugin;

impl Plugin for OptionsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OptionsWindow>()
            .init_resource::<OptionsLayout>()
            .add_systems(
                PreUpdate,
                input
                    .in_set(OptionsInputSet)
                    .after(bevy::input::InputSystems)
                    .after(super::gamemenu::GameMenuInputSet),
            )
            .add_systems(
                Update,
                (build, live.after(build), rollover.after(build)).after(super::UiInputSet),
            );
        #[cfg(feature = "dev")]
        app.add_systems(PreStartup, open_window)
            .add_systems(Update, input::option_hook);
    }
}

/// `GD_OPTIONS_OPEN` (hooks.rs) opens the window at start.
#[cfg(feature = "dev")]
fn open_window(mut commands: Commands, hooks: Res<crate::hooks::Hooks>) {
    let Some(tab) = hooks.ui.options_open.clone().and_then(|t| Tab::parse(&t)) else {
        return;
    };
    // `GD_OPTIONS_DROP=<n>` also opens the page's n-th dropdown.
    let dropdown = hooks.ui.options_drop.and_then(|n| {
        page(tab)
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, Ctl::Drop(..)))
            .nth(n)
            .map(|(i, _)| i)
    });
    // `GD_OPTIONS_REBIND` waits for that binding's key.
    let rebind = hooks.ui.options_rebind;
    commands.insert_resource(OptionsWindow {
        open: true,
        tab,
        dropdown,
        rebind,
        ..default()
    });
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Gameplay,
    Interface,
    HealthBars,
    KeyBindings,
    Audio,
    Video,
    Network,
}

impl Tab {
    const ALL: [Tab; 7] = [
        Tab::Gameplay,
        Tab::Interface,
        Tab::HealthBars,
        Tab::KeyBindings,
        Tab::Audio,
        Tab::Video,
        Tab::Network,
    ];

    /// `GD_OPTIONS_OPEN`'s tab names.
    #[cfg(feature = "dev")]
    fn parse(s: &str) -> Option<Tab> {
        Some(match s {
            "gameplay" => Tab::Gameplay,
            "interface" => Tab::Interface,
            "healthbars" => Tab::HealthBars,
            "keybindings" => Tab::KeyBindings,
            "audio" => Tab::Audio,
            "video" => Tab::Video,
            "network" => Tab::Network,
            _ => return None,
        })
    }
}

/// The window's state: open, tab, the edited draft, the bindings' scroll.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct OptionsWindow {
    pub open: bool,
    pub tab: Tab,
    draft: Option<GameOptions>,
    scroll: usize,
    /// The slider being dragged (option name).
    drag: Option<&'static str>,
    /// The key-binding list's thumb being dragged: the press's offset from the thumb's top (physical px).
    key_grab: Option<f32>,
    /// The open dropdown (its control's index in the page).
    dropdown: Option<usize>,
    /// The open list's first row shown ([`DROP_ROWS`] at a time).
    drop_top: usize,
    /// The edited key bindings, and the cell waiting for a key (action, 0
    /// primary / 1 secondary).
    keys: Option<KeyBindings>,
    rebind: Option<(u32, usize)>,
    /// Bumped whenever the keys draft is replaced or bound: the key list's
    /// layout version in the rebuild signature (the draft's values are live).
    keys_ver: u64,
}

impl OptionsWindow {
    /// The shared Esc dispatcher's close (`ui::esc_dispatcher`): the waiting rebind cell first,
    /// then an open list, then the window itself. Whether something closed.
    pub fn escape(&mut self) -> bool {
        if !self.open {
            return false;
        }
        if self.rebind.take().is_some() || self.dropdown.take().is_some() {
            return true;
        }
        self.open = false;
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Tab(Tab),
    Check(&'static str),
    /// A slider: option, x range of the knob's centre (physical px). Only the
    /// knob takes a press.
    Slider(&'static str, f32, f32),
    /// A slider's knob: option, its centre's travel (physical px). Its rect is stored at value 0: the knob moves
    /// with the value without a rebuild ([`live`]), so [`hit_at`] places it at the value.
    Knob(&'static str, f32),
    Default,
    Apply,
    Close,
    List,
    /// A dropdown box (control index in the page).
    Drop(usize),
    /// A row of the open dropdown's list.
    DropRow(usize),
    /// The open list's scroll column (its right 20 px when it has more than [`DROP_ROWS`] rows): the list's top y and
    /// height (physical px) and its row count.
    DropBar(f32, f32, usize),
    /// A key binding cell: action, 0 primary / 1 secondary.
    Key(u32, usize),
    /// The key-binding list's scroll bar thumb: its rect stored at scroll 0 ([`hit_at`] moves it as [`live`] does).
    KeyThumb,
}

#[derive(Resource, Default, Clone, PartialEq)]
pub struct OptionsLayout {
    hits: Vec<(URect, Hit)>,
    /// The controls' rollover areas and tags ([`rollover_tag`]).
    tips: Vec<(URect, String)>,
    /// The key-binding list's last scroll position (rows − visible rows), set when the list is built.
    max_scroll: usize,
    /// The key-binding list's row height and visible row area (physical px):
    /// its cell hits are stored at scroll 0 ([`hit_at`] shifts and clips them).
    key_row_h: f32,
    key_clip: URect,
    /// The key-binding list's thumb: its top at scroll 0 and its travel (physical px).
    key_bar: (f32, f32),
    /// The open dropdown list's row height and visible box (physical px).
    drop_row_h: f32,
    drop_clip: URect,
}

/// The key-binding list's scroll after `lines` wheel lines (up is positive), kept in `0..=max`.
fn scrolled(scroll: usize, lines: f32, max: usize) -> usize {
    ((scroll as f32 - lines.signum() * lines.abs().ceil()).max(0.0) as usize).min(max)
}

/// The key-binding list thumb's top at `scroll` (as [`live`] places it).
fn key_thumb_top(layout: &OptionsLayout, scroll: usize) -> f32 {
    let (top, travel) = layout.key_bar;
    let span = layout.max_scroll.max(1) as f32;
    (top + travel * (scroll.min(layout.max_scroll) as f32 / span)).round()
}

/// The hit under `p` (physical px): the key-list cells and the dropdown rows
/// are stored at scroll 0, shifted by the list's scroll and clipped to its
/// visible area (a scrolled-away row takes no mouse); a slider knob is placed at its value in `o`, the key list's
/// thumb at the list's scroll.
fn hit_at(
    layout: &OptionsLayout,
    o: &GameOptions,
    scroll: usize,
    drop_top: usize,
    p: Vec2,
) -> Option<Hit> {
    for (r, h) in &layout.hits {
        let r = match *h {
            // The list's pane draws a row `scroll` rows up (`super::window::pane`).
            Hit::Key(..) => {
                if !contains(&layout.key_clip, p) {
                    continue;
                }
                URect::new(r.x, r.y - scroll as f32 * layout.key_row_h, r.w, r.h)
            }
            Hit::DropRow(_) => {
                if !contains(&layout.drop_clip, p) {
                    continue;
                }
                URect::new(r.x, r.y - drop_top as f32 * layout.drop_row_h, r.w, r.h)
            }
            // As [`live`] places the knob.
            Hit::Knob(k, travel) => {
                URect::new((r.x + travel * slider_get(o, k)).round(), r.y, r.w, r.h)
            }
            Hit::KeyThumb => URect::new(r.x, key_thumb_top(layout, scroll), r.w, r.h),
            _ => *r,
        };
        if contains(&r, p) {
            return Some(*h);
        }
    }
    None
}

/// The rows a dropdown list shows at a time (draw sub_1400ABF70 / input sub_1400AB4B0).
const DROP_ROWS: usize = 16;

/// The first row a dropdown list shows when it opens (sub_1400AB4B0): with more than [`DROP_ROWS`] rows the
/// selected one, kept within the last page; else the first.
fn drop_first_row(shown: Shown, o: &GameOptions, screen: Vec2) -> usize {
    let rows = choices(shown, o, screen);
    let current = shown_text(shown, o, screen);
    let selected = rows.iter().position(|(t, _)| *t == current).unwrap_or(0);
    selected.min(rows.len().saturating_sub(DROP_ROWS))
}

/// The first row after a press at `y` in a list's scroll column (sub_1400AB4B0): the press's place between
/// `top + 5` and `top + height - 5` (UI-scaled) as a fraction of the scroll range, truncated.
fn drop_bar_row(y: f32, top: f32, height: f32, s: f32, rows: usize) -> usize {
    let f = ((y - (s * 5.0 + top)) / (height - s * 10.0)).clamp(0.0, 1.0);
    (f * rows.saturating_sub(DROP_ROWS) as f32) as usize
}

/// A control of a page, in the reference shots' screen coordinates.
#[derive(Clone, Copy)]
enum Ctl {
    /// Checkbox: option, label tag, box top-left.
    Check(&'static str, &'static str, f32, f32),
    /// Slider: option, label tag, label x, centre y, track x0..x1.
    Slider(&'static str, &'static str, f32, f32, f32, f32),
    /// Dropdown: what it shows, label tag, label x, centre y, box x0..x1.
    Drop(Shown, &'static str, f32, f32, f32, f32),
}

/// A dropdown's value.
#[derive(Clone, Copy)]
enum Shown {
    Language,
    MainMenu,
    OutputDevice,
    InputDevice,
    RenderDevice,
    WindowMode,
    Resolution,
    Aniso,
    Quality(&'static str),
    Weather,
    Shadows,
    ColorBlind,
    Movement,
}

/// Whether a checkbox can be changed: "Evade To Cursor" (`tagGameplayOption36`) is greyed out exactly when the
/// movement type is Keyboard Only (options window `sub_1400C8E60`: enabled = `GetInt(13) != 2`; the
/// option is then ignored by `HotSlotOptionEvade::Activate`, see `gd_game::evade`).
fn check_enabled(key: &str, draft: &GameOptions) -> bool {
    !(key == "evadeFollowCursor" && draft.int("movementType") == 2)
}

/// A check box's state bitmaps (the `optionsCheckboxBitmap` pattern's `*`),
/// indexed by [`check_state`].
const CHECK_STATES: [&str; 5] = ["up", "over", "down", "downover", "disabled"];

/// A check box's bitmap index into [`CHECK_STATES`], drawn by `build` and `live`.
fn check_state(on: bool, enabled: bool, hovered: bool) -> usize {
    match (on, hovered) {
        _ if !enabled => 4,
        (true, true) => 3,
        (true, false) => 2,
        (false, true) => 1,
        (false, false) => 0,
    }
}

/// The options window's keyboard / mouse input.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OptionsInputSet;
