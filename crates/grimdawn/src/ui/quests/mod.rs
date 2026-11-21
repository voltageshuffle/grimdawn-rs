//! The quest tracker (HUD widget, `records/ui/quest/questwidget.dbr`) and the
//! quest journal (the Codex window's quest pages,
//! `records/ingameui/questwindow/questwindow.dbr`). Both read [`QuestView`],
//! which [`collect`] assembles from the story's quest state
//! (`gd_sim::story::quest`); nothing here changes quests.
//!
//! Tracker, `Grim Dawn.exe` (loader 0x14022ab80, rebuild
//! 0x140229a20, draw 0x14022b0a0):
//! - the quests are `Quest2Repository::GetQuests(filter 12)`: a task in
//!   progress and the quest tracked (`Quest2::IsTracked`, true from the
//!   constructor), or blocked (multiplayer only); stably sorted with main
//!   quests (`Quest2::GetFlags` bit 0) first (0x14022cad0), else in the
//!   repository's order (`gd_sim::story::quest::QuestDefs::repository_order`);
//! - drawn at the record's rect (241 wide, right-aligned, 325 from the top):
//!   a `backgroundColor` rect and the border, the `titleTag` text
//!   (`titleFontStyle`), then 3 px lower each quest's name (`taskFontStyle`,
//!   one line cut to the width less the main-quest icon's with "...",
//!   sub_140240930 wrap flag 0) with the main-quest icon in front of main
//!   quests (text 4 px after it, 1 px up), and below it every objective with
//!   text of each task in progress: its check bitmap (`objectiveBitmap`,
//!   frame 1 when satisfied) and its wrapped text (`objectiveFontStyle`);
//! - the height is the title's line height plus one quest-style line per
//!   quest and the objectives' lines.
//!
//! - the toggle button (`toggleButton`, at the widget's right edge less its
//!   width and 1 px, centred on the title line; loader 0x14022ab80) shows or
//!   hides the tracker (`Options` bool 25 `questwidgetopen`, stored; hidden:
//!   only the button is drawn, sub_14022B0A0) and is the only part that fades
//!   (drawn at the fade alpha). The fade (update sub_14022AF90, ms): starts
//!   held at 1 for 5 s, then falls 0.001 per ms to its minimum (0 while the
//!   tracker shows, 1 while hidden), rises 0.004 per ms back to 1 when the
//!   mouse is over the title line (sub_14022B7E0, not while held) or the
//!   button is pressed (sub_14022BB60), then holds 5 s again;
//! - untracking (the journal's track bitmaps, `Quest2::SetTracked` +77) is
//!   not saved by the engine either (`Quest2::WriteProperties` skips it).
//!
//! - dragging the title line moves the tracker while it shows (sub_14022B7E0:
//!   kept inside the screen; not stored, PORT: the engine's saving of the
//!   position is unchecked).
//!
//! Journal (the Codex key, `super::keys::CODEX`, Q by default; Esc and its close button close it): the window's
//! five tabs (help, active, completed, lore, devotion), the upper page (`questJournal`, sub_14021FC80) and the
//! tree (`questLog` / `questlogtree.dbr`, sub_1402235A0 / sub_140223920), both
//! scroll panes (`topGuardBand`, `height`) with a vertical scroll bar when the
//! content is taller (`verticalScrollbar`, `scrollbarPositionAdjust` /
//! `scrollbarSizeAdjust`, sub_14023AC40). The tabs list the repository's
//! quests with a task in progress / completed ones (a task complete, none in
//! progress), grouped by `Quest2` group in the order of the groups' first
//! quests; the first is selected by default. Details in [`build_journal`].
//! Scroll bars: arrows, track pages, repeat and thumb drag in the shared widget
//! ([`super::window::Bar::press`] / [`super::window::bar_held`]). The window title (`windowTitle`, a `textstaticstring.tpl`
//! with only `textTag` `quest2windowtitle`, loaded by sub_14022E100 and drawn
//! by sub_14022EC80) has no style or text box, so its draw (sub_140259A70)
//! shows nothing: the real window has no title (as the reference shot).
//! The lore page: [`codex_pages`]; the devotion page: [`devotion_page`]; a note read opens the
//! lore page at it ([`lore_read`]).
//! Help page ([`help_pages`]), `Grim Dawn.exe`: the index (`helpIndex`, loader sub_1401C4B10,
//! rebuild sub_1401C5060 from the journal refresh sub_14022DCC0) lists every distinct record of
//! its `tutorialPages` (no player state read: all pages show), named by the localized `TextTag`
//! of the page's first `NuggetArray` nugget, sorted, rows `entryNameText` in `helpindextree.dbr`;
//! with nothing shown the first entry is selected. The page (`helpPage`, loader sub_1401C7BA0:
//! `contentX` / `contentY` / `contentWidth`; content sub_1401C70C0) lays out the page's nuggets
//! (`TopSpacing`, `Image`, `TextTag`, `TextCentering`, `TextStyle`, `keyOperation`, `altPage`).
//! Hooks (hooks.rs): `GD_UI_TAB` 0 help, 2 completed, 3 lore, 4 devotion; `GD_QUEST_SCROLL` scrolls the upper page;
//! `GD_QUEST_SELECT` / `GD_LORE_SELECT` / `GD_HELP_SELECT` select an entry.
//!
//! Sounds: the journal's tabs and close button and the tracker's toggle register their record sounds
//! (`Ui::record_button_sound`, `ui/widgets.rs`).
mod collect;
mod journal;
mod lore;
mod tracker;

use super::window::{Bar, BarHold, ScrollOwner, ScrollThumb, bar_held, pane, scrollbar_live};
use super::{Ui, text::FontCache};
use crate::data::GameData;
use crate::inventory::UiCapture;
use crate::story::StoryRuntime;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use collect::*;
use gd_formats::save::item::Item;
use gd_formats::ui::{Rect as URect, TextBox, WindowDef};
use gd_sim::story::quest::{STATE_COMPLETE, STATE_IN_PROGRESS};
use journal::*;
pub(super) use lore::key_operation;
use lore::*;
use std::hash::{Hash, Hasher};
use tracker::*;

pub const TRACKER: &str = "records/ui/quest/questwidget.dbr";
pub const JOURNAL: &str = "records/ingameui/questwindow/questwindow.dbr";

pub struct QuestUiPlugin;

impl Plugin for QuestUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuestView>()
            .init_resource::<QuestJournal>()
            .init_resource::<JournalLayout>()
            .init_resource::<TrackerFade>()
            .add_systems(
                Update,
                (
                    collect.in_set(crate::ui::UiPlaySet),
                    toggle.in_set(crate::ui::UiPlaySet),
                    lore_read.in_set(crate::ui::UiPlaySet),
                    tracker_input.in_set(crate::ui::UiInputSet),
                    (build_tracker, build_journal, journal_live).in_set(crate::ui::UiPlaySet),
                    tracker_button.in_set(crate::ui::UiPlaySet),
                    clicks.in_set(crate::ui::UiInputSet),
                )
                    .chain(),
            );
        app.add_systems(PreStartup, open_journal);
    }
}

/// `GD_UI_OPEN=quests` opens the journal at start (`GD_UI_TAB=2`: the completed quests).
fn open_journal(mut commands: Commands, hooks: Res<crate::hooks::Hooks>) {
    if hooks.ui.open.as_deref() != Some("quests") {
        return;
    }
    let scroll = hooks.ui.quest_scroll;

    let tab = hooks.ui.tab;
    commands.insert_resource(QuestJournal {
        open: true,
        help: tab == Some(0),
        help_page: hooks.ui.help_select.clone(),
        completed: tab == Some(2),
        lore: tab == Some(3),
        devotion: tab == Some(4),
        note: hooks.ui.lore_select.clone(),
        scroll: [scroll, 0.0],
        ..default()
    });
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskView {
    pub name: String,
    pub description: String,
    pub state: u32,
    /// Completed after being in progress (`Quest2Task::WasInProgress`).
    pub finished: bool,
    /// Objectives with text: (text, satisfied).
    pub objectives: Vec<(String, bool)>,
    /// Reward events: their texts, and the items of `GiveItem` actions of
    /// those without text (sub_14021F5E0).
    pub rewards: Vec<String>,
    pub reward_items: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuestEntry {
    pub id: u32,
    pub name: String,
    pub group: String,
    /// `Quest2::GetFlags` bit 0.
    pub main: bool,
    pub tasks: Vec<TaskView>,
}

impl QuestEntry {
    pub fn in_progress(&self) -> bool {
        self.tasks.iter().any(|t| t.state == STATE_IN_PROGRESS)
    }
    pub fn completed(&self) -> bool {
        !self.in_progress() && self.tasks.iter().any(|t| t.state == STATE_COMPLETE)
    }
}

/// The quests the player has started (a task in progress or complete), in
/// the quest repository's order (`QuestDefs::repository_order`). Written
/// only by [`collect`].
#[derive(Resource, Default, Clone, PartialEq)]
pub struct QuestView {
    pub quests: Vec<QuestEntry>,
}

/// The journal window: open, tab (completed quests), selected quest id,
/// collapsed groups, untracked quests (PORT: not saved) and the two panes'
/// scroll offsets (px: journal, log).
#[derive(Resource, Default, Clone, PartialEq)]
pub struct QuestJournal {
    pub open: bool,
    pub completed: bool,
    /// The lore page (Codex tab) and its selected note (record path).
    pub lore: bool,
    pub note: Option<String>,
    /// The devotion page (Devotion tab, [`devotion_page`]).
    pub devotion: bool,
    /// The help page (Help tab, [`help_pages`]) and its selected tutorial page (record path).
    pub help: bool,
    pub help_page: Option<String>,
    pub selected: Option<u32>,
    pub collapsed: Vec<String>,
    pub untracked: Vec<u32>,
    pub scroll: [f32; 2],
    /// The quest tree row under the mouse (`q<id>` / `g<group>`), drawn in
    /// `hoverColor`.
    pub hover: Option<String>,
}

impl QuestJournal {
    /// Opens the Help tab on a tutorial page record (the quick tip's click, sub_140109A60).
    pub fn open_help(&mut self, page: &str) {
        self.open = true;
        self.help = true;
        self.lore = false;
        self.devotion = false;
        self.completed = false;
        self.help_page = Some(page.to_owned());
        self.scroll[0] = 0.0;
    }

    /// Shows tab `i` (help, active, completed, lore, devotion): a press of its tab button. Another tab than the one
    /// shown drops the selection, the hover and the scroll.
    pub fn select_tab(&mut self, i: usize) {
        if self.help != (i == 0)
            || self.completed != (i == 2)
            || self.lore != (i == 3)
            || self.devotion != (i == 4)
        {
            self.help = i == 0;
            self.completed = i == 2;
            self.lore = i == 3;
            self.devotion = i == 4;
            self.selected = None;
            self.hover = None;
            self.scroll = [0.0; 2];
        }
    }
}

/// The built journal (physical px) for clicks.
#[derive(Resource, Default)]
pub struct JournalLayout {
    window: Option<URect>,
    close: Option<URect>,
    /// Help, active, completed, lore and devotion tabs.
    tabs: [Option<URect>; 5],
    rows: Vec<(URect, u32)>,
    /// Lore note rows (record path).
    notes: Vec<(URect, String)>,
    /// Group expand / contract bitmaps, and the group rows.
    groups: Vec<(URect, String)>,
    group_rows: Vec<(URect, String)>,
    /// Quest track bitmaps.
    tracks: Vec<(URect, u32)>,
    /// The panes (journal, log) and how far each can scroll.
    panes: [(URect, f32); 2],
    /// Each pane's scroll bar.
    bars: [Option<Bar>; 2],
}

impl JournalLayout {
    /// The built journal window contains `p` (physical px).
    pub fn covers(&self, p: Vec2) -> bool {
        self.window.is_some_and(|r| super::contains(&r, p))
    }
}

/// The tracker's toggle button fade (module doc; sub_14022AF90's state:
/// 0 start, 1 held, 2 falling, 3 rising, 4 at the minimum) and its rects.
#[derive(Resource)]
pub struct TrackerFade {
    alpha: f32,
    state: u8,
    hold_ms: f32,
    hover: bool,
    title: Option<URect>,
    button: Option<URect>,
    /// The tracker's top-left once dragged (physical px; the session's only),
    /// the drag's grab offset, and the drawn size (width, height).
    pos: Option<Vec2>,
    grab: Option<Vec2>,
    size: Vec2,
}

impl Default for TrackerFade {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            state: 0,
            hold_ms: 5000.0,
            hover: false,
            title: None,
            button: None,
            pos: None,
            grab: None,
            size: Vec2::ZERO,
        }
    }
}

/// The tracker shows (`Options` bool 25).
fn tracker_open(options: &gd_game::options::GameOptions) -> bool {
    options.flag("questwidgetopen")
}

/// The fade, the title hover and the toggle button (module doc).
fn tracker_input(
    (hooks, time): (Res<crate::hooks::Hooks>, Res<Time<Real>>),
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut options: ResMut<gd_game::options::GameOptions>,
    mut fade: ResMut<TrackerFade>,
    mut capture: ResMut<UiCapture>,
    pause: Res<gd_game::pause::GamePause>,
) {
    let dt = time.delta_secs() * 1000.0;
    let open = tracker_open(&options);
    let min = if open { 0.0 } else { 1.0 };
    match fade.state {
        0 => fade.state = 1,
        1 => {
            fade.hold_ms -= dt;
            if fade.hold_ms <= 0.0 {
                fade.state = 2;
            }
        }
        2 => {
            fade.alpha -= dt * 0.001;
            if fade.alpha <= min {
                fade.alpha = min;
                fade.state = 4;
            }
        }
        3 => {
            fade.alpha += dt * 0.004;
            if fade.alpha >= 1.0 {
                fade.alpha = 1.0;
                fade.hold_ms = 5000.0;
                fade.state = 1;
            }
        }
        _ => {}
    }
    // The game menu / Options holds the cursor: no hover, no fade-in.
    let p = window
        .single()
        .ok()
        .and_then(|w| super::cursor_px(&hooks, w))
        .filter(|_| !pause.menu);
    let inside = |r: Option<URect>| r.zip(p).is_some_and(|(r, p)| super::contains(&r, p));
    let over_button = inside(fade.button);
    if open && inside(fade.title) && fade.state != 1 {
        fade.state = 3;
    }
    if fade.hover != over_button {
        fade.hover = over_button;
    }
    // Dragging by the title (sub_14022B7E0, only while the tracker shows): a
    // press on the title line grabs it, moving keeps the grab offset with the
    // tracker inside the screen and holds the fade 5 s, a release lets go.
    if !open || !buttons.pressed(MouseButton::Left) {
        fade.grab = None;
    }
    if let (Some(g), Some(p), Ok(w)) = (fade.grab, p, window.single()) {
        let screen = Vec2::new(w.physical_width() as f32, w.physical_height() as f32);
        let to = (p - g).min(screen - fade.size).max(Vec2::ZERO);
        if fade.pos != Some(to) {
            fade.pos = Some(to);
            fade.hold_ms = 5000.0;
        }
        capture.0 = true;
    } else if open
        && !over_button
        && buttons.just_pressed(MouseButton::Left)
        && let (Some(t), Some(p)) = (fade.title.filter(|_| inside(fade.title)), p)
    {
        fade.grab = Some(p - Vec2::new(t.x, t.y));
        capture.0 = true;
    }
    if over_button {
        capture.0 = true;
        if buttons.just_pressed(MouseButton::Left) {
            // sub_14022BB60: toggle, fade in, store the option.
            options.set("questwidgetopen", gd_formats::options::Value::Bool(!open));
            crate::settings::store(&options);
            fade.state = 3;
        }
    }
}

/// The Codex key (`super::keys::CODEX`, Q by default) toggles the journal. The Help key (`super::keys::HELP`, H by
/// default) toggles it too and shows its Help tab: HUD key action sub_140211AF0 case 8 flips the journal's
/// visibility (HUD +165280, the quest window, loader sub_14022E100), presses its `helpButton` tab (window +4728,
/// through its button manager +544) and rebuilds the help index (sub_1401C5060); no other window closes.
fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<super::keys::KeyBindings>,
    mut journal: ResMut<QuestJournal>,
) {
    if bindings.just_pressed(super::keys::CODEX, &keys) {
        journal.open = !journal.open;
    }
    if bindings.just_pressed(super::keys::HELP, &keys) {
        journal.open = !journal.open;
        journal.select_tab(0);
    }
    // Esc closes the journal through the shared dispatcher (`ui::esc_dispatcher`).
}

/// A lore note was read (`gd_game::lore::LoreNoteRead`, the engine's event
/// 61): GrimDawn.exe sub_14022FB30 (a new note plays the record's
/// `loreCollectedSound`, the note's page is shown and the tree rebuilt) and
/// Game.dll `ItemNote::Use` (the `tagLoreBonus` / `ExperienceFormat` notice
/// for the bonus: `ShowCinematicText` type 5, sent as `CinematicText` kind 5).
/// PORT: the journal opens on the lore page at the note (`ItemNote::Use` /
/// `PlayerInventoryCtrl::UseItem` call the game UI's slot 5, not decompiled).
fn lore_read(
    mut events: MessageReader<gd_game::lore::LoreNoteRead>,
    mut journal: ResMut<QuestJournal>,
    data: Res<GameData>,
    fonts: Res<FontCache>,
    mut sound: MessageWriter<gd_game::visuals::PlaySound>,
    mut notify: MessageWriter<gd_game::story::CinematicText>,
) {
    for ev in events.read() {
        if ev.new
            && let Some(pak) = data.db.get(JOURNAL).and_then(|r| {
                r.str("loreCollectedSound")
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            })
        {
            sound.write(gd_game::visuals::PlaySound { pak, at: None });
        }
        if ev.xp > 0 {
            // `ItemNote::Use`: cinematic text type 5, `tagLoreBonus` over the XP line.
            notify.write(gd_game::story::CinematicText {
                head: "tagLoreBonus".into(),
                sub: super::floating::format_tag(fonts.tag("ExperienceFormat"), &[ev.xp as i64]),
                sub_args: Vec::new(),
                kind: 5,
                front: false,
            });
        }
        journal.open = true;
        journal.lore = true;
        journal.devotion = false;
        journal.help = false;
        journal.completed = false;
        journal.note = Some(ev.record.clone());
        journal.scroll[0] = 0.0;
    }
}

#[derive(Component)]
struct TrackerRoot;

#[derive(Component)]
struct JournalRoot;

/// The tracker's quests: in progress (and tracked), main quests first.
pub fn tracked(view: &QuestView) -> Vec<&QuestEntry> {
    let mut q: Vec<&QuestEntry> = view.quests.iter().filter(|q| q.in_progress()).collect();
    q.sort_by_key(|q| !q.main);
    q
}

/// Journal clicks: tabs, group expand bitmaps, track bitmaps, quest rows,
/// the close button, the scroll bars (module doc); the wheel scrolls the
/// pane under the mouse (int(120 × 0.2 × UI scale) px a notch); the hovered tree
/// row; the window takes the mouse.
#[allow(clippy::too_many_arguments)]
fn clicks(
    hooks: Res<crate::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    window: Query<&Window, With<PrimaryWindow>>,
    scale: Res<crate::settings::UiScaleOption>,
    layout: Res<JournalLayout>,
    mut journal: ResMut<QuestJournal>,
    mut capture: ResMut<UiCapture>,
    time: Res<Time<Real>>,
    // (pane, arrow step or thumb grab offset, held ms) of a held scroll bar.
    mut held: Local<Option<(usize, BarHold)>>,
    pause: Res<gd_game::pause::GamePause>,
) {
    let (Some(wr), Ok(win)) = (layout.window, window.single()) else {
        wheel.clear();
        *held = None;
        return;
    };
    // A rebuild can shorten a pane (a collapsed group, a shorter text): keep
    // the scroll within its content.
    for (i, (_, max)) in layout.panes.iter().enumerate() {
        if journal.scroll[i] > *max {
            journal.scroll[i] = *max;
        }
    }
    if pause.menu {
        // Modal: the game menu / Options takes wheel, hover and presses.
        wheel.clear();
        *held = None;
        if journal.hover.is_some() {
            journal.hover = None;
        }
        return;
    }
    let Some(p) = super::cursor_px(&hooks, win) else {
        return;
    };
    let inside = |r: &URect| super::contains(r, p);
    let s = scale.factor(win.physical_width(), win.physical_height());
    // A held arrow repeats; a held thumb follows the mouse.
    if !buttons.pressed(MouseButton::Left) {
        *held = None;
    }
    if let Some((i, hold)) = held.as_mut() {
        let i = *i;
        let v = bar_held(
            layout.bars[i],
            hold,
            p.y,
            time.delta_secs() * 1000.0,
            journal.scroll[i],
            layout.panes[i].1,
        );
        if v != journal.scroll[i] {
            journal.scroll[i] = v;
        }
        capture.0 = true;
        return;
    }
    if !inside(&wr) {
        wheel.clear();
        if journal.hover.is_some() {
            journal.hover = None;
        }
        return;
    }
    capture.0 = true;
    // The hovered tree row.
    let q = Vec2::new(p.x, p.y + journal.scroll[1]);
    let hit = |r: &URect| super::contains(r, q);
    let hover = if inside(&layout.panes[1].0) {
        layout
            .rows
            .iter()
            .find(|(r, _)| hit(r))
            .map(|(_, id)| format!("q{id}"))
            .or_else(|| {
                layout
                    .notes
                    .iter()
                    .find(|(r, _)| hit(r))
                    .map(|(_, n)| format!("n{n}"))
            })
            .or_else(|| {
                layout
                    .group_rows
                    .iter()
                    .find(|(r, _)| hit(r))
                    .map(|(_, g)| format!("g{g}"))
            })
    } else {
        None
    };
    if journal.hover != hover {
        journal.hover = hover;
    }
    if buttons.just_pressed(MouseButton::Left) {
        for (i, bar) in layout.bars.iter().enumerate() {
            if let Some((v, hold)) =
                bar.and_then(|b| b.press(p, s, journal.scroll[i], layout.panes[i].1))
            {
                journal.scroll[i] = v;
                *held = Some((i, hold));
                return;
            }
        }
    }
    for w in wheel.read() {
        // `sub_14023D4C0`: a notch (120 wheel units) scrolls int(120 × 0.2 × UI scale) px.
        let step = match w.unit {
            bevy::input::mouse::MouseScrollUnit::Line => w.y * (120.0 * 0.2 * s).trunc(),
            bevy::input::mouse::MouseScrollUnit::Pixel => w.y,
        };
        for (i, (r, max)) in layout.panes.iter().enumerate() {
            if inside(r) {
                let v = (journal.scroll[i] - step).clamp(0.0, *max);
                if v != journal.scroll[i] {
                    journal.scroll[i] = v;
                }
            }
        }
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    // Clicks inside a pane are on its scrolled content.
    let shifted = |i: usize| Vec2::new(p.x, p.y + journal.scroll[i]);
    let hit = |r: &URect, q: Vec2| super::contains(r, q);
    let in_log = inside(&layout.panes[1].0);
    if layout.close.as_ref().is_some_and(inside) {
        journal.open = false;
    } else if let Some(i) = layout
        .tabs
        .iter()
        .position(|t| t.as_ref().is_some_and(inside))
    {
        journal.select_tab(i);
    } else if !in_log {
        // Nothing clickable on the upper page.
    } else if let Some((_, g)) = layout.groups.iter().find(|(r, _)| hit(r, shifted(1))) {
        let g = g.clone();
        if let Some(i) = journal.collapsed.iter().position(|c| *c == g) {
            journal.collapsed.remove(i);
        } else {
            journal.collapsed.push(g);
        }
    } else if let Some((_, id)) = layout.tracks.iter().find(|(r, _)| hit(r, shifted(1))) {
        let id = *id;
        if let Some(i) = journal.untracked.iter().position(|u| *u == id) {
            journal.untracked.remove(i);
        } else {
            journal.untracked.push(id);
        }
    } else if let Some((_, id)) = layout.rows.iter().find(|(r, _)| hit(r, shifted(1)))
        && journal.selected != Some(*id)
    {
        journal.selected = Some(*id);
        journal.scroll[0] = 0.0;
    } else if let Some((_, n)) = layout.notes.iter().find(|(r, _)| hit(r, shifted(1)))
        && journal.help
    {
        if journal.help_page.as_ref() != Some(n) {
            journal.help_page = Some(n.clone());
            journal.scroll[0] = 0.0;
        }
    } else if let Some((_, n)) = layout.notes.iter().find(|(r, _)| hit(r, shifted(1)))
        && journal.note.as_ref() != Some(n)
    {
        // A new note scrolls the page back to the top (sub_140168860).
        journal.note = Some(n.clone());
        journal.scroll[0] = 0.0;
    }
}
