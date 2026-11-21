//! Devotion window: the full-screen star map of `gd_sim::devotion::Devotions`
//! (`records/ui/skills/devotion/devotion_mastertable.dbr`, `devotionpane.tpl`).
//! The game has no devotion key binding (the input action table, GrimDawn.exe
//! `sub_1401F68B0`, lists `SkillWindow` but no devotion window): it opens from
//! the HUD's `devotionButton` (`sub_14022E100`, `hudDevotionWindow`) and the
//! skills window's devotion tab (`ui/skills.rs`). Open it with
//! `UiPanels { devotion: true, .. }`.
//!
//! Map (window render `sub_14018B980`, scroll clamp `sub_140184720`,
//! zoom `sub_14018AFC0`, tiling `sub_1401847D0`): every bitmap sits at
//! `centre - scroll + position × zoom` (record positions are relative to the
//! screen centre, texture sizes scale with the zoom, neither with the UI
//! scale); the starfield `bgTile` repeats over a 6144 × 5120 map (8192 wide
//! when the screen is wider than 2560) centred on the origin; zoom runs 0.5 to
//! 1.0 (start 0.8, `sub_1401857B0`), the buttons step it by 0.1 about the
//! screen centre (`sub_14018BEC0`), the mouse wheel by `0.001 × delta`
//! (at most 0.1) about the cursor; scroll is dragged with the left button and
//! clamped to `±(map × zoom - screen) / 2` per axis. Layers: tiles, the
//! `nebulaSections`, each constellation's `constellationBackground` art tinted
//! by its Unavailable / Available / Active colour (`sub_140180F00`), links,
//! stars (`skillbutton.tpl`, bitmap at `bitmapPosition`).
//! The chrome (undo, class tabs, devotion tab, help, close at offsets from the
//! top centre; the sidebar centred on the left edge, `sub_14018DB00`) is drawn
//! at the UI scale.
//! Rendering states (render `sub_1401895C0`, constellation `sub_140180F00`, star
//! `sub_14017E330`, state flags `sub_140184F20` / `sub_1401853B0` / `sub_140181900`): tiles
//! normal blend; nebulae (every `nebulaSections` record), constellation art, stars and glows
//! additive (`ui/additive.rs`). Art colour: Unavailable, Available when the affinities are met,
//! Active when any star is taken, plus a second white pass while partly taken. Stars: taken =
//! Down; else Disabled (alpha 0.5 when the affinities are unmet, 1 when the star has links and
//! none is taken), else Up; the hovered star also gets InFocus; celestial powers get the
//! `effectBgBitmap` glow (alpha 0.8 / 0.5). Links: `connectionWidth` × zoom to each side of the
//! centre line, Active texture when both ends are taken, else Inactive when the constellation is
//! available, else Locked. The `edgeFade` vignette covers the window under the chrome. Random
//! star glows: `pulse_stars`.
//! PORT (performance, not an engine fact): zooming, panning and the sidebar slide do not rebuild the window (a
//! rebuild costs 5-10 ms). The map layers (tiles, nebulae, art, links, stars, glows, pulse sprites, the `InFocus`
//! overlay) are children of one [`DevotionMap`] container built at zoom 1 for the screen enlarged by a margin that
//! covers the whole zoom range, the pan slack and the map bounds; the vignette, sidebar and top bar stay under the
//! root. [`pan_map`] scales the container by the zoom about the screen centre and pans it by
//! `built_scroll × zoom - scroll` (a render-time [`UiTransform`], as the engine's zoom); a drag moves only the
//! container. The hit rectangles of stars and art stay in build coordinates and are tested with [`to_map`]
//! ([`to_screen`] maps them back to the screen). The window is rebuilt (`version` bump) when the pan passes
//! [`REBUILD_PAN`] (a new root is not drawn on its first frame, so there is slack to the margin) and on state
//! changes — not on zoom. The sidebar content sits under one [`SidebarPanel`] container that [`sidebar_live`]
//! moves with the slide progress. The pulses, the constellation fx and the search caret run on real time (the UI
//! animates while the world is paused).
//! The link texture is drawn with normal blend (the engine's "filtered" shader technique is state-for-state the
//! `normal` one in the D3D11 profile, `basictexture.ssh`). The additive blend adds in gamma space as the engine's
//! UNORM target does (`shown = shown_dst + tex * alpha`; the UI target holds gamma values, `additive.wesl`): against
//! the real game at the same view (scroll 108,-187) the blurred map is within 2 % in every
//! brightness band, the strong nebulae included (the darkest reds about 1.5 levels of 255 brighter). PORT: the
//! tooltip (constellation name and description, plus the star's skill name) is a stand-in, placed centred above the
//! cursor.
//!
//! Celestial powers (`sub_14017E330`, `sub_14017EB80`, `sub_1401853B0`, `sub_140186960`): a left click on a
//! taken power, or taking one, opens the skill-select list centred on the star (a `SlotSelectWindow`,
//! `ui/slot_select.rs`; what it offers: [`power_options`]; a pick sends `BindPower`), and the star shows the bound
//! skill's icon at 0.75 x its size centred on it. A star acts on the release (event 9) over it after a press (event 1)
//! on it (`sub_14017EB80`: the press sets the star's +311 byte, the release over it with the byte set takes,
//! reclaims or opens the list, `sub_1401853B0`). PORT: a press elsewhere forgets the pressed star (the engine's byte
//! stays set until a release over the star), and an untaken power with no devotion point left (or a star flagged
//! +308 / +309) does not open the list (the engine's falls through to the open). The bound hotbar keys over the
//! star still bind too. `GD_DEVOTION_PICK` (hooks.rs, `auto` too) opens the list. The power classification is
//! cached in [`DevotionView::powers`] (session state, reset on session and mod change).
//!
//! Search box (`sub_140188870` / `sub_14017F5B0` / `sub_14017E330`): the
//! typed text, trimmed and lowercased, is looked for as one substring in each star's skill text ([`search_lines`]:
//! name, description, the stat lines of its current level); a match draws the
//! star's Down bitmap again in red (1, 0, 0) with normal blend. Live on every key; nothing else changes (no dimming,
//! the view does not move). The clear button shows while there is text; opening the window clears the box. Help
//! (`helpButton`): shows tutorial page 59 (`tutorial_p60_devotion`) in the journal's Help tab; opening the window unlocks
//! that page (its quick tip shows once). The box is an `editbox.tpl`: caret, selection and keys as the market search box (`gd_sim::edit`).
//!
//! Stars (click `sub_14017EB80`, only left button down / up reach a star, `sub_1401853B0`): a left click
//! takes a star (one point). Only in the reclaim mode (a spirit guide opened the window, pane `+9241`,
//! `gd_game::reclamation`) does a left click on a taken star remove it instead (a taken power too, no
//! skill-select list); the point comes back for gold and aether
//! (`SkillManager::UseDevotionReclamationPoints`: the player record's
//! `devotionReclamationPointCosts` entry of the `devotionReclamationPointTiers`
//! band holding the refunds made so far, and `devotionReclamationAetherCost` ×
//! `gameengine.dbr` `aetherCrystal`; both must be held or nothing happens; the
//! refund count goes up); nothing is taken there. In the mode every untaken star draws
//! its Disabled bitmap (`sub_14017E330`), the sidebar is held open and also draws `totalGoldText` /
//! `totalGoldNumber` (the money, `^r` when 0), `totalAetherText` / `totalAetherNumber` (`Player::GetCurrentAether`,
//! `^r` when 0) and `goldCostText` / `goldCostNumber` (`GetCurrentDevotionReclamationCost`) (`sub_14018EBA0`,
//! numbers `sub_14018F070`, records `sub_14018DB00`), and the mastery tabs move the mode to the skills window.
//! The star tooltip in the mode (`sub_14017F240` passes the mode and both prices to `GenerateUIDevotionText`) ends in
//! the reclaim line (`gd_sim::skill_tooltip::DevReclaim`). With a taken celestial power under the cursor, a
//! bound hotbar key binds it to the skill in that slot.
//!
//! Info tab (sidebar ctor `sub_14018D000`, toggle `sub_14018F940`, render `sub_14018EBA0`,
//! update `sub_14018F070`): the left sidebar is a panel that
//! slides in/out over 200 ms (`infoTabOpen` / `infoTabClose` are its tab handles, at (0,278) and
//! (222,380); the master table's `infoBox` field is a leftover the binary never reads). It starts
//! open; clicking the close handle slides it out (only the open handle stays), clicking the open
//! handle slides it back. The reclaim mode holds it open. [`DevotionView::sidebar`] is the progress
//! (1 open .. 0 closed); [`sidebar_live`] moves the panel container in place and flips the tab handles (no rebuild).
//!
//! Constellation highlight and completion flash (constellation render `sub_140180F00`,
//! update `sub_140180DC0`, window update `sub_140188870`, star click `sub_1401853B0`, complete
//! `sub_1401819E0`): a partly-taken constellation's art gets a second additive pass in the shared
//! `constellationColorPulse` colour (gold, 1500 ms, forever) — [`DevotionFx`] advances it and tints
//! the [`ConstHighlight`] overlays; a completed one is steady Active, an untouched one steady
//! Unavailable/Available. When a star click makes a constellation newly AVAILABLE (the affinity a
//! completion grants opens others), its art flashes white once (`constellationFlashPulse`, 350 ms
//! in + 350 ms out, the [`ConstFlash`] overlays); completing a constellation plays
//! `constellationCompleteSound`. [`devotion_fx`] detects both transitions against the previous
//! taken state (only while the window is open; opening initialise the snapshot without flashing).
//!
//! First-run star pulse phase (window loader `sub_140186E60`, update `sub_140188870`,
//! Engine.dll `ColorPulse` ctor @0x18004bc00): the 15 glow slots are built pulsing with no star,
//! and the update only advances while the window is open, so the first open runs one empty pulse
//! (~2 × [`PULSE_TIME`]) before any star attaches; [`StarPulses::warmup`] models it.
//!
//! Sounds: the enabled top-bar buttons (help, close, zoom, clear search, undo) and the mastery tabs register their
//! `soundNameDown` (`Ui::record_button_sound`, `ui/widgets.rs`).
use super::additive::{self, AdditiveCache, AdditiveMaterial};
use super::contains;
use super::hud::ColorPulse;
use super::{HudValues, Ui};
use crate::devotion::DevotionState;
use crate::inventory::UiCapture;
use crate::player::Player;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::ui::UiTransform;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, TextAlign, TextBox, Widget};

const MASTER: &str = "records/ui/skills/devotion/devotion_mastertable.dbr";
const TAB_STYLE: &str = "records/ui/styles/text/style_special_classtabs.dbr";
const TEXT_STYLE: &str = "records/ui/styles/text/style_nooutline_textwhite_sizen.dbr";
pub const START_ZOOM: f32 = 0.8;
const ZOOM_RANGE: (f32, f32) = (0.5, 1.0);
/// The pan (px, per axis) at which the window is rebuilt around the new scroll.
const REBUILD_PAN: f32 = 200.0;

/// The container of the map layers: built at zoom 1, scaled and panned at render time by [`pan_map`]
/// instead of rebuilding the window while zooming or dragging.
#[derive(Component)]
pub struct DevotionMap {
    /// The scroll the layers were built at.
    built: Vec2,
    /// Logical px per physical px.
    inv: f32,
}

/// Whether a hit rectangle belongs to the map layers (and so moves with the pan), not the chrome.
fn panned(hit: Option<DevHit>) -> bool {
    matches!(hit, None | Some(DevHit::Star(..)))
}

/// Scales each map container by the view's zoom about the screen centre and pans it to the scroll
/// (the layers are built at zoom 1; [`to_map`] / [`to_screen`] convert between screen and build coordinates).
fn pan_map(view: Res<DevotionView>, mut maps: Query<(&DevotionMap, &mut UiTransform)>) {
    for (m, mut t) in &mut maps {
        let pan = m.built * view.zoom - view.scroll;
        let scale = Vec2::splat(view.zoom);
        let translation = Val2::px(pan.x * m.inv, pan.y * m.inv);
        if t.scale != scale || t.translation != translation {
            t.scale = scale;
            t.translation = translation;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DevHit {
    Star(usize, usize),
    Close,
    Tab(u8),
    ZoomIn,
    ZoomOut,
    /// `undoButton`: revert the changes made since the window opened.
    Undo,
    /// Sidebar / top bar: blocks panning, nothing else.
    Panel,
    /// The search box (click activates it) and its clear button.
    SearchBox,
    ClearSearch,
    /// `helpButton`: shows the devotion tutorial page.
    Help,
    /// A row of the skill-select list (index into [`PickState::options`]) and the list's panel.
    Pick(usize),
    PickPanel,
    /// `infoTabOpen` (true) / `infoTabClose` (false): slide the sidebar in or out. Only the visible
    /// handle counts ([`sidebar_live`] flips them at progress 0).
    InfoTab(bool),
}

/// The open skill-select list (`devotion_skillselect`): the star it binds, the star's centre it is centred on
/// (physical px), what it offers ([`power_options`]) and the hovered row.
#[derive(Clone, Debug)]
pub struct PickState {
    pub star: (usize, usize),
    pub at: Vec2,
    pub options: Vec<PowerPick>,
    pub hover: Option<usize>,
}

/// A skill-select row's skill: its record and, for an item skill, which one (`gd_game::devotion::Binding::item`).
pub type PickSkill = (String, Option<gd_game::player::ItemSkillSource>);

/// One row of the skill-select list: the skill (none: unassigned), its look and the tag after its name.
#[derive(Clone, Debug, PartialEq)]
pub struct PowerPick {
    pub skill: Option<PickSkill>,
    pub look: super::slot_select::PickLook,
    pub suffix: Option<&'static str>,
}

#[derive(Resource, Default, Clone)]
pub struct DevotionLayout {
    pub window: Option<URect>,
    pub hits: Vec<(DevHit, URect)>,
    /// The sliding sidebar panel's width (physical px): its hit rect is stored at the open position,
    /// [`clicks`] shifts it by the slide progress.
    pub sidebar: Option<f32>,
    /// Constellation art rectangles (tooltip hover).
    pub arts: Vec<(usize, URect)>,
    /// Stars a pulse may land on (taken, or not locked) and the glow bitmap
    /// (image, offset and size in map px, logical px per screen px).
    pub pulse_stars: Vec<(usize, usize)>,
    pub pulse_art: Option<(Handle<Image>, Vec2, Vec2, f32)>,
    /// Every star's `InFocus` bitmap and rectangle (map build px), and logical px per screen px:
    /// [`focus`] puts the hovered star's on top without rebuilding the window.
    pub focus_art: Vec<((usize, usize), Handle<Image>, URect)>,
    pub focus_inv: f32,
    /// `DevotionView::version` of the build that made this layout: the pulse sprites of an older build (still shown for a
    /// frame or two until the new window root replaces the old one) are hidden instead of placed by this layout.
    pub build: u64,
}

/// A constellation art's partly-taken highlight pass: the shared `constellationColorPulse` colour
/// ([`devotion_fx`]) over the art's `image` (the overlay pans with the map).
#[derive(Component)]
pub struct ConstHighlight {
    pub image: Handle<Image>,
}

/// A constellation art's completion-flash pass ([`devotion_fx`] shows it white while the
/// constellation's `constellationFlashPulse` runs).
#[derive(Component)]
pub struct ConstFlash {
    pub ci: usize,
    pub image: Handle<Image>,
}

/// Whether a star's skill is a celestial power: the session cache ([`classify_powers`]), computed
/// on a miss (the build only reads the view).
fn is_power(
    db: &gd_formats::vfs::Database,
    powers: &std::collections::HashMap<String, bool>,
    skill: &str,
) -> bool {
    powers.get(skill).copied().unwrap_or_else(|| {
        gd_sim::skills::profile_record(db, skill)
            .is_some_and(|r| gd_sim::skill_tooltip::is_power(&r))
    })
}

/// Fills [`DevotionView::powers`] for every star skill (static game data; the rebuild reads it).
fn classify_powers(
    mut view: ResMut<DevotionView>,
    state: Option<Res<DevotionState>>,
    data: Res<crate::data::GameData>,
    mut fresh: gd_game::session::NewSession,
) {
    if fresh.begun() {
        view.powers.clear();
    }
    if view.powers.is_empty()
        && let Some(state) = state
    {
        for c in &state.data.list {
            for s in &c.stars {
                let power = gd_sim::skills::profile_record(&data.db, &s.skill)
                    .is_some_and(|r| gd_sim::skill_tooltip::is_power(&r));
                view.powers.insert(s.skill.clone(), power);
            }
        }
    }
}

/// The mounted mod changed: the skill classification is its data (`model::drop_on_mod_change`).
fn clear_is_power_on_mod_change(
    mut mod_changed: MessageReader<gd_render::model::ModChanged>,
    mut view: ResMut<DevotionView>,
) {
    if mod_changed.read().next().is_some() {
        view.powers.clear();
    }
}

/// The hovered star's `InFocus` bitmap (`sub_14017E330`: on top of the star, additive), moved
/// with the pointer instead of rebuilding the window (a rebuild of the whole map per hover change was
/// the cost of moving the cursor over the stars).
fn focus(
    mut commands: Commands,
    view: Res<DevotionView>,
    layout: Res<DevotionLayout>,
    roots: Query<(Entity, &super::UiRoot), Without<super::UiFresh>>,
    maps: Query<(Entity, &ChildOf), With<DevotionMap>>,
    mut shown: Local<Option<(Entity, Entity, (usize, usize))>>,
    mut fresh: gd_game::session::NewSession,
) {
    if fresh.begun() {
        *shown = None;
    }
    let root = roots
        .iter()
        .find(|(_, r)| r.0 == super::Group::Devotion)
        .and_then(|(e, _)| maps.iter().find(|(_, p)| p.parent() == e))
        .map(|(m, _)| m);
    let want = view
        .hover
        .and_then(|(c, s, _)| s.map(|s| (c, s)))
        .filter(|_| layout.window.is_some());
    // The overlay lives under the map container (it moves with the pan): a rebuilt root takes the old one along.
    if shown.map(|(_, r, w)| (r, w)) == root.zip(want) {
        return;
    }
    if let Some((e, ..)) = shown.take() {
        commands.entity(e).try_despawn();
    }
    let (Some(root), Some(want)) = (root, want) else {
        return;
    };
    let Some((_, h, r)) = layout.focus_art.iter().find(|(k, ..)| *k == want) else {
        return;
    };
    let inv = layout.focus_inv;
    let e = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(r.x * inv),
            top: Val::Px(r.y * inv),
            width: Val::Px(r.w * inv),
            height: Val::Px(r.h * inv),
            ..default()
        })
        .id();
    commands.entity(root).add_child(e);
    additive::queue_additive(&mut commands, e, h.clone(), Color::WHITE);
    *shown = Some((e, root, want));
}

/// Random star glows (`sub_140188870` update, `ColorPulse::Update`).
const PULSE_SLOTS: usize = 15;
const PULSE_DELAY: (f32, f32) = (2000.0, 7000.0);
const PULSE_TIME: f32 = 2000.0;
const PULSE_A: [f32; 4] = [0.3, 0.3, 0.3, 0.1];
const PULSE_B: [f32; 4] = [1.0, 1.0, 1.0, 0.8];
/// The sidebar slide lasts this many ms (sidebar ctor `sub_14018D000`, +7572 = 200).
const SIDEBAR_TIME: f32 = 200.0;

/// The sliding sidebar content container: drawn at the open position, [`sidebar_live`] moves it
/// with [`DevotionView::sidebar`] (the slide animates in place, no rebuild).
#[derive(Component)]
struct SidebarPanel {
    /// The open position's x (physical px).
    base_x: f32,
    /// The panel bitmap's UI-scaled width (physical px).
    width: f32,
    /// Logical px per physical px.
    inv: f32,
}

/// A sidebar tab handle: true = `infoTabOpen` (shown while the panel is away), false = `infoTabClose`.
#[derive(Component)]
struct SidebarHandle(bool);

/// Moves the sidebar container with the slide progress and shows the matching tab handle
/// (the visibility rule of the old per-frame rebuild's `tab_key`).
fn sidebar_live(
    view: Res<DevotionView>,
    mut panels: Query<(&SidebarPanel, &mut Node)>,
    mut handles: Query<(&SidebarHandle, &mut Visibility)>,
) {
    for (p, mut node) in &mut panels {
        let left = Val::Px((p.base_x + (view.sidebar - 1.0) * p.width) * p.inv);
        if node.left != left {
            node.left = left;
        }
    }
    let show_open = view.sidebar <= 0.0;
    for (h, mut vis) in &mut handles {
        let want = if h.0 == show_open {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
    }
}

#[derive(Clone, Copy, Default)]
struct PulseSlot {
    star: Option<(usize, usize)>,
    /// ms until the pulse starts / ms of pulse played (0 .. 2 × PULSE_TIME).
    delay: f32,
    played: f32,
}

#[derive(Resource)]
pub struct StarPulses {
    slots: [PulseSlot; PULSE_SLOTS],
    rng: u32,
    /// The first-run empty pulse (module doc): ms left before any star may attach. The engine's
    /// slots are built pulsing with no star and only advance while the window is open, so the first
    /// open runs one full pulse (~2 × PULSE_TIME) first; afterwards the slots cycle normally.
    warmup: f32,
}

impl Default for StarPulses {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_secs() as u32)
            .max(1);
        Self {
            slots: [PulseSlot::default(); PULSE_SLOTS],
            rng: seed,
            warmup: 2.0 * PULSE_TIME,
        }
    }
}

impl StarPulses {
    /// Park-Miller, as the engine's (`16807 × seed mod 2^31 - 1`).
    fn next(&mut self) -> u32 {
        self.rng = ((self.rng as u64 * 16807) % 0x7FFF_FFFF) as u32;
        self.rng
    }
}

#[derive(Component)]
pub struct PulseSprite(usize, u64);

pub struct DevotionUiPlugin;

impl Plugin for DevotionUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StarPulses>()
            .init_resource::<DevotionLayout>()
            .init_resource::<DevotionView>()
            .add_systems(
                First,
                clear_is_power_on_mod_change.after(gd_render::model::drop_on_mod_change),
            )
            .add_systems(Startup, start_view)
            .add_systems(
                PreUpdate,
                search_typing
                    .after(bevy::input::InputSystems)
                    .before(super::gamemenu::GameMenuInputSet)
                    .before(super::options::OptionsInputSet)
                    .run_if(in_state(crate::session::AppState::InGame)),
            )
            .add_systems(
                Update,
                classify_powers
                    .in_set(crate::ui::UiPlaySet)
                    .before(super::HudSet),
            )
            .add_systems(
                Update,
                (
                    pan_map,
                    pulse_stars,
                    devotion_fx,
                    sidebar_live,
                    tooltip,
                    focus,
                )
                    .chain()
                    .after(super::HudSet)
                    .in_set(crate::ui::UiPlaySet),
            );
    }
}

/// The star map's view: scroll (px), zoom, and the constellation (and star)
/// under the cursor with the cursor position the tooltip was anchored at.
/// `version` is bumped on a change that rebuilds the window (not by a small pan, zoom or the sidebar slide).
#[derive(Resource)]
pub struct DevotionView {
    pub scroll: Vec2,
    pub zoom: f32,
    pub hover: Option<(usize, Option<usize>, Vec2)>,
    pub version: u64,
    /// The scroll the window was last built at (set by `ui::rebuild`).
    pub built_scroll: Vec2,
    /// The search box's text and whether it takes the keyboard (module doc).
    pub search: String,
    pub typing: bool,
    /// The search box's caret, selection and key repeat (`gd_sim::edit`).
    pub edit: gd_sim::edit::EditState,
    pub pick: Option<PickState>,
    /// A celestial power just clicked to be taken and its centre: the list opens once it is taken.
    pub pick_after: Option<((usize, usize), Vec2)>,
    /// The sidebar slide progress: 1 open .. 0 closed (module doc, info tab). Starts open.
    pub sidebar: f32,
    /// Whether the sidebar is sliding open (false: closing); equal progress means idle.
    pub sidebar_open: bool,
    /// The highlight / flash animation and its transition snapshot (module doc).
    pub fx: DevotionFx,
    /// Whether each star skill is a celestial power ([`classify_powers`]): session state, reset with
    /// the view and on mod change.
    pub powers: std::collections::HashMap<String, bool>,
}

/// Screen point → map build coordinates (the container scales by zoom about the screen centre, then pans).
fn to_map(view: &DevotionView, screen: Vec2, p: Vec2) -> Vec2 {
    let c = screen * 0.5;
    c + (p - c + view.scroll) / view.zoom - view.built_scroll
}

/// Map build coordinates → screen point.
fn to_screen(view: &DevotionView, screen: Vec2, b: Vec2) -> Vec2 {
    let c = screen * 0.5;
    c + (b - c + view.built_scroll) * view.zoom - view.scroll
}

impl Default for DevotionView {
    fn default() -> Self {
        Self {
            scroll: Vec2::ZERO,
            zoom: START_ZOOM,
            hover: None,
            version: 1,
            built_scroll: Vec2::ZERO,
            search: String::new(),
            typing: false,
            edit: Default::default(),
            pick: None,
            pick_after: None,
            sidebar: 1.0,
            sidebar_open: true,
            fx: DevotionFx::default(),
            powers: Default::default(),
        }
    }
}

/// The constellation highlight and completion-flash state (module doc), kept in [`DevotionView`]
/// so it resets with the session. The pulses are loaded from the master table on first use.
#[derive(Default)]
pub struct DevotionFx {
    /// The shared partly-taken highlight pulse (`constellationColorPulse`); None until loaded.
    pub const_pulse: Option<ColorPulse>,
    /// Per-constellation flash pulse (`constellationFlashPulse`), running while `flash_on`.
    pub flash: Vec<ColorPulse>,
    pub flash_on: Vec<bool>,
    /// The previous available / complete state, to spot the transitions a star click causes.
    prev_avail: Vec<bool>,
    prev_complete: Vec<bool>,
    /// The snapshot is initialised (the first open frame does it without flashing).
    ready: bool,
}

/// The constellations that just became available (the flash triggers) and just completed (the
/// sound triggers), comparing the previous taken state to the current one
/// (`sub_1401853B0` / `sub_1401819E0`).
fn transitions(
    prev_avail: &[bool],
    prev_complete: &[bool],
    avail: &[bool],
    complete: &[bool],
) -> (Vec<usize>, Vec<usize>) {
    let flash = (0..avail.len())
        .filter(|&i| !prev_avail[i] && avail[i])
        .collect();
    let done = (0..complete.len())
        .filter(|&i| !prev_complete[i] && complete[i])
        .collect();
    (flash, done)
}

/// `GD_DEVOTION_SCROLL` / `GD_DEVOTION_ZOOM` (hooks.rs): the start view.
fn start_view(hooks: Res<crate::hooks::Hooks>, mut view: ResMut<DevotionView>) {
    if let Some(v) = hooks.ui.devotion_scroll {
        view.scroll = v;
    }
    if let Some(z) = hooks.ui.devotion_zoom {
        view.zoom = z;
    }
    if let Some(t) = &hooks.ui.devotion_search {
        view.search = t.clone();
    }
}

/// What the tooltip shows: (constellation, star, devotion version, points, screen, the reclaim mode's
/// (aether held, money, refunds made)), and the skill book it was drawn from (compared by reference, copied only
/// when the tooltip is redrawn).
type TipKey = (
    usize,
    Option<usize>,
    u64,
    u32,
    UVec2,
    Option<(u32, u64, u32)>,
);

/// Hover tooltip of the star map: a constellation's name and description, or
/// a star's skill text (`GameEngine::GenerateUIDevotionText`, see
/// `gd_sim::skill_tooltip::devotion_lines`), drawn in its own overlay.
#[allow(clippy::too_many_arguments)]
fn tooltip(
    hooks: Res<crate::hooks::Hooks>,
    window: Query<&Window, With<PrimaryWindow>>,
    panels: Res<super::UiPanels>,
    values: Res<HudValues>,
    state: Option<Res<DevotionState>>,
    view: Res<DevotionView>,
    mut canvas: super::tip::TipCanvas,
    mut fresh: gd_game::session::NewSession,
    mut shown: Local<Option<(TipKey, gd_sim::skills::SkillBook)>>,
    player: Query<&crate::combat::Combatant, With<Player>>,
    rules: Option<Res<crate::combat::CombatRules>>,
) {
    if fresh.begun() {
        *shown = Default::default();
    }
    use gd_sim::skill_tooltip::{
        DevReclaim, DevStatus, DevotionTip, SkillTipCtx, TextLine, class, devotion_lines,
    };
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    // `GD_DEVOTION_HOVER` (hooks.rs) fakes a hover.
    let faked = hooks.ui.devotion_hover.clone().and_then(|n| {
        state
            .as_ref()
            .and_then(|s| s.data.locate(&n))
            .map(|(c, st)| {
                (
                    c,
                    Some(st),
                    Vec2::new(size.x as f32 * 0.4, size.y as f32 * 0.1),
                )
            })
    });
    let hover = faked.or(view.hover).filter(|_| panels.devotion);
    let key: Option<TipKey> = hover.zip(state.as_ref()).map(|((c, st, _), s)| {
        let reclaim = values
            .devotion_reclaim
            .as_ref()
            .map(|r| (r.aether, values.gold, s.reclaimed));
        (c, st, s.version, s.points, size, reclaim)
    });
    let same = match (&*shown, &key) {
        (None, None) => true,
        (Some((k, book)), Some(key)) => k == key && *book == values.skills,
        _ => false,
    };
    if same {
        return;
    }
    *shown = key.map(|k| (k, values.skills.clone()));
    canvas.clear(super::tip::TipOwner::Devotion);
    let (Some((ci, si, at)), Some(state)) = (hover, state) else {
        return;
    };
    if size.x == 0 || size.y == 0 {
        return;
    }
    let Some(c) = state.data.list.get(ci) else {
        return;
    };
    let (data, fonts) = (&canvas.data, &canvas.assets.fonts);
    let ctx = SkillTipCtx {
        db: &data.db,
        trees: &data.skill_trees(),
        tags: &fonts.tags,
        book: &values.skills,
        player: rules
            .as_ref()
            .zip(player.iter().next())
            .map(|(r, c)| (&*r.0, &*c.profile)),
        pet_bonuses: &[],
    };
    let lines: Vec<TextLine> = match si.and_then(|i| c.stars.get(i)) {
        Some(star) => {
            let taken = |n: &str| values.skills.level(n) > 0;
            // The player's affinity once for this tip (the status and the reclaim locks both read it).
            let have = state.data.affinity(&taken);
            let status = if taken(&star.skill) {
                DevStatus::Taken
            } else {
                // `GenerateUIDevotionText` order of the reason flags: affinity
                // (`a4[12]`), then locked (`a4[2]`), then no points (`*a4`).
                if !state.data.available(ci, &have) {
                    DevStatus::Affinity(
                        (0..5)
                            .filter(|&i| c.required[i] > 0)
                            .map(|i| (i, c.required[i], have[i]))
                            .collect(),
                    )
                } else if !star.links.is_empty()
                    && !star
                        .links
                        .iter()
                        .any(|&l| c.stars.get(l).is_some_and(|p| taken(&p.skill)))
                {
                    DevStatus::Locked
                } else if state.points == 0 {
                    DevStatus::NoPoints
                } else {
                    DevStatus::Available
                }
            };
            let (level, experience) = state
                .powers
                .get(&star.skill.to_ascii_lowercase())
                .copied()
                .unwrap_or((1, 0));
            // The reclaim mode's line (`DevReclaim`; flags `sub_14017DF80` / `sub_140188870`,
            // locks `sub_140184F20` / `sub_14018C560`).
            let reclaim = values.devotion_reclaim.as_ref().map(|r| {
                let (gold, aether) = gd_game::devotion::reclaim_cost(&data.db, state.reclaimed);
                let line = if !taken(&star.skill) {
                    DevReclaim::NoPoints
                } else if gold > values.gold {
                    DevReclaim::NoGold
                } else if aether > r.aether {
                    DevReclaim::NoAether
                } else {
                    let deps = state.data.affinity_dependents_with(ci, &taken, &have);
                    let needed = si.is_some_and(|si| {
                        c.stars
                            .iter()
                            .enumerate()
                            .any(|(i, o)| i != si && taken(&o.skill) && o.links.contains(&si))
                    });
                    if deps.contains(&ci) {
                        DevReclaim::SelfLocked
                    } else if !deps.is_empty() {
                        DevReclaim::Dependency(
                            deps.iter()
                                .map(|&d| state.data.list[d].name_tag.clone())
                                .collect(),
                        )
                    } else if needed {
                        DevReclaim::Base
                    } else {
                        DevReclaim::Point
                    }
                };
                (line, gold, aether)
            });
            let tip = DevotionTip {
                reclaim,
                bound: state
                    .bindings
                    .iter()
                    .find(|b| b.power.eq_ignore_ascii_case(&star.skill))
                    .map(|b| b.skill.clone()),
                level,
                experience,
                status,
                bonus: (0..5)
                    .filter(|&i| c.given[i] > 0)
                    .map(|i| (i, c.given[i]))
                    .collect(),
            };
            devotion_lines(&ctx, &star.skill, &tip)
        }
        None => {
            let name = ctx.tag(&c.name_tag);
            let desc = data
                .db
                .get(&c.record)
                .and_then(|r| r.str("constellationInfoTag").map(|t| ctx.tag(t)))
                .unwrap_or_default();
            let mut v = vec![TextLine {
                class: class::SKILL_NAME,
                text: name,
                icon: None,
            }];
            if !desc.is_empty() {
                v.push(TextLine {
                    class: class::SKILL_DESCRIPTION,
                    text: desc,
                    icon: None,
                });
            }
            v
        }
    };
    if lines.is_empty() {
        return;
    }
    // PORT: the real window is centred on the cursor with its bottom edge just above it,
    // clamped to the screen.
    let gap = 10.0 * canvas.assets.scale(size);
    canvas.show(
        super::tip::TipOwner::Devotion,
        &lines,
        super::tip::Blank::DescriptionLine,
        |w, h, screen, _| {
            (
                (at.x - w * 0.5).clamp(0.0, (screen.x - w).max(0.0)),
                (at.y - gap - h).clamp(0.0, (screen.y - h).max(0.0)),
            )
        },
    );
}

/// Advances the star pulses and places their sprites (`sub_140188870`):
/// min(max(1, 0.2 × candidates), 15) slots; an idle slot takes a random candidate
/// that no other slot holds, with a random 2000-7000 ms delay (`starPulseDelayMin/Max`);
/// `ColorPulse::Update` blends colour A to B and back over `pulseTime` each way
/// (`devotion_starcolorpulse.dbr`), nothing is drawn during the delay.
/// PORT: the very first run of a slot has an extra half-cycle in the engine; every run is
/// the same here.
fn pulse_stars(
    time: Res<Time<Real>>,
    layout: Res<DevotionLayout>,
    mut pulses: ResMut<StarPulses>,
    mut sprites: Query<(&PulseSprite, &mut Node, &mut Visibility, Entity)>,
    mut commands: Commands,
    mut cache: ResMut<AdditiveCache>,
    mut mats: ResMut<Assets<AdditiveMaterial>>,
) {
    let Some((image, off, size, inv)) = layout.pulse_art.clone() else {
        return;
    };
    let dt = time.delta_secs() * 1000.0;
    // The first-run empty pulse (module doc): the slots run one pulse with no star before any
    // attaches; the sprites stay hidden meanwhile. Only advances while the window is open (the
    // layout has no pulse art when it is closed).
    if pulses.warmup > 0.0 {
        pulses.warmup -= dt;
        for (sp, _node, mut vis, _e) in &mut sprites {
            if sp.1 == layout.build {
                *vis = Visibility::Hidden;
            }
        }
        return;
    }
    let n = ((layout.pulse_stars.len() as f32 * 0.2) as usize).clamp(1, PULSE_SLOTS);
    for i in 0..n.min(PULSE_SLOTS) {
        let mut slot = pulses.slots[i];
        let gone = slot.star.is_some_and(|s| !layout.pulse_stars.contains(&s));
        if slot.star.is_none() || gone || slot.played >= 2.0 * PULSE_TIME {
            slot.star = None;
            if !layout.pulse_stars.is_empty() {
                for _ in 0..layout.pulse_stars.len() {
                    let c = layout.pulse_stars[pulses.next() as usize % layout.pulse_stars.len()];
                    if !pulses
                        .slots
                        .iter()
                        .enumerate()
                        .any(|(j, o)| j != i && o.star == Some(c))
                    {
                        slot.star = Some(c);
                        let span = PULSE_DELAY.1 - PULSE_DELAY.0 + 1.0;
                        slot.delay = PULSE_DELAY.0 + (pulses.next() as f32 % span);
                        slot.played = 0.0;
                        break;
                    }
                }
            }
        } else if slot.delay > 0.0 {
            slot.delay -= dt;
        } else {
            slot.played += dt;
        }
        pulses.slots[i] = slot;
    }
    for (sp, mut node, mut vis, e) in &mut sprites {
        // A sprite of an older build keeps its last place and look until its window root is replaced:
        // the layout is the new build's, and hiding it would flicker the glow for a frame.
        if sp.1 != layout.build {
            continue;
        }
        let slot = pulses.slots[sp.0];
        let rect = slot.star.and_then(|(c, s)| {
            layout
                .hits
                .iter()
                .find(|(h, _)| *h == DevHit::Star(c, s))
                .map(|(_, r)| *r)
        });
        let (Some(r), true) = (
            rect,
            sp.0 < n && slot.delay <= 0.0 && slot.played < 2.0 * PULSE_TIME,
        ) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let t = slot.played / PULSE_TIME;
        let k = if t < 1.0 { t } else { 2.0 - t };
        let col: Vec<f32> = (0..4)
            .map(|j| PULSE_A[j] + (PULSE_B[j] - PULSE_A[j]) * k)
            .collect();
        node.left = Val::Px((r.x + off.x) * inv);
        node.top = Val::Px((r.y + off.y) * inv);
        node.width = Val::Px(size.x * inv);
        node.height = Val::Px(size.y * inv);
        *vis = Visibility::Inherited;
        let h = cache.get(
            &mut mats,
            &image,
            Color::srgba(col[0], col[1], col[2], col[3]),
        );
        commands.entity(e).try_insert(MaterialNode(h));
    }
}

/// The constellation highlight pulse, the completion flash and sound, and the sidebar slide
/// (module doc). The pulses only advance while the window is open (the engine update
/// early-returns while hidden); the first open frame initialises the transition snapshot without
/// flashing (`sub_1401853B0` flashes on a star click, not on open).
#[allow(clippy::too_many_arguments)]
fn devotion_fx(
    time: Res<Time<Real>>,
    panels: Res<super::UiPanels>,
    layout: Res<DevotionLayout>,
    mut view: ResMut<DevotionView>,
    state: Option<Res<DevotionState>>,
    reclaim: Option<Res<gd_game::reclamation::SkillReclamation>>,
    loadouts: Query<&gd_game::player::Loadout, With<Player>>,
    data: Res<crate::data::GameData>,
    mut highlights: Query<(&ConstHighlight, Entity)>,
    mut flashes: Query<(&ConstFlash, &mut Visibility, Entity)>,
    mut commands: Commands,
    mut cache: ResMut<AdditiveCache>,
    mut mats: ResMut<Assets<AdditiveMaterial>>,
    mut sound: MessageWriter<gd_game::visuals::PlaySound>,
) {
    let dt = time.delta_secs() * 1000.0;
    let open = panels.devotion && layout.window.is_some();
    if !open {
        // Closed: freeze the flashes and forget the snapshot so the next open never flashes stale.
        view.fx.ready = false;
        view.fx.flash_on.fill(false);
        return;
    }
    let reclaiming = reclaim.as_ref().is_some_and(|m| m.devotion_mode());
    // The sidebar slide (`sub_14018F070` / `sub_14018F9F0`): toward the target over
    // SIDEBAR_TIME; the reclaim mode holds it open. [`sidebar_live`] moves the panel in place.
    if reclaiming {
        // The reclaim mode holds the sidebar open (`sub_14018F9F0` snaps it open).
        view.sidebar_open = true;
        view.sidebar = 1.0;
    }
    let target = if view.sidebar_open { 1.0 } else { 0.0 };
    if view.sidebar != target {
        let step = dt / SIDEBAR_TIME;
        view.sidebar = if target > view.sidebar {
            (view.sidebar + step).min(target)
        } else {
            (view.sidebar - step).max(target)
        };
    }
    let (Some(state), Ok(loadout)) = (state.as_deref(), loadouts.single()) else {
        return;
    };
    let d = &state.data;
    let n = d.list.len();
    let taken = |s: &str| loadout.skills.level(s) > 0;
    let aff = d.affinity(&taken);
    let Some(m) = data.db.get(MASTER) else {
        return;
    };
    if view.fx.const_pulse.is_none() {
        view.fx.const_pulse = m
            .str("constellationColorPulse")
            .and_then(|p| data.db.get(p))
            .map(ColorPulse::from_record);
    }
    let flash_rec = m
        .str("constellationFlashPulse")
        .and_then(|p| data.db.get(p));
    if view.fx.prev_avail.len() != n {
        view.fx.prev_avail = vec![false; n];
        view.fx.prev_complete = vec![false; n];
        view.fx.flash_on = vec![false; n];
        view.fx.flash = match &flash_rec {
            Some(r) => (0..n).map(|_| ColorPulse::from_record(r)).collect(),
            None => Vec::new(),
        };
        view.fx.ready = false;
    }
    // The transitions a star click causes (`sub_1401853B0` / `sub_1401819E0`): newly
    // available -> flash; newly complete -> the completion sound.
    let avail_now: Vec<bool> = (0..n).map(|ci| d.available(ci, &aff)).collect();
    let complete_now: Vec<bool> = (0..n).map(|ci| d.complete(ci, &taken)).collect();
    if !view.fx.ready {
        view.fx.prev_avail = avail_now;
        view.fx.prev_complete = complete_now;
        view.fx.ready = true;
    } else {
        let (flash, done) = transitions(
            &view.fx.prev_avail,
            &view.fx.prev_complete,
            &avail_now,
            &complete_now,
        );
        for ci in flash {
            if view.fx.flash.len() == n {
                view.fx.flash[ci].reset();
                view.fx.flash_on[ci] = true;
            }
        }
        for _ in done {
            if let Some(pak) = m.str("constellationCompleteSound") {
                sound.write(gd_game::visuals::PlaySound {
                    pak: pak.to_owned(),
                    at: None,
                });
            }
        }
        view.fx.prev_avail = avail_now;
        view.fx.prev_complete = complete_now;
    }
    // The shared highlight pulse tints every partly-taken overlay (`sub_140188870`).
    let hl = view.fx.const_pulse.as_mut().map(|p| {
        let [r, g, b, a] = p.update(dt);
        Color::srgba(r, g, b, a)
    });
    if let Some(col) = hl {
        for (h, e) in &mut highlights {
            let mh = cache.get(&mut mats, &h.image, col);
            commands.entity(e).try_insert(MaterialNode(mh));
        }
    }
    // Advance the running flashes once, then show / hide their overlays.
    if view.fx.flash.len() == n {
        for ci in 0..n {
            if view.fx.flash_on[ci] {
                view.fx.flash[ci].update(dt);
                if !view.fx.flash[ci].is_pulsing() {
                    view.fx.flash_on[ci] = false;
                }
            }
        }
    }
    for (f, mut vis, e) in &mut flashes {
        let active = view.fx.flash_on.get(f.ci).copied().unwrap_or(false)
            && view.fx.flash.get(f.ci).is_some_and(|p| p.is_pulsing());
        if active {
            let mh = cache.get(&mut mats, &f.image, view.fx.flash[f.ci].color());
            commands.entity(e).try_insert(MaterialNode(mh));
            *vis = Visibility::Inherited;
        } else {
            *vis = Visibility::Hidden;
        }
    }
}

/// Map size in record units (`sub_1401847D0`).
fn map_size(screen_w: f32) -> Vec2 {
    Vec2::new(if screen_w > 2560.0 { 8192.0 } else { 6144.0 }, 5120.0)
}

/// `sub_140184720`: scroll limits.
fn clamp_scroll(screen: Vec2, zoom: f32, scroll: Vec2) -> Vec2 {
    let lim = ((map_size(screen.x) * zoom - screen) * 0.5).max(Vec2::ZERO);
    scroll.clamp(-lim, lim)
}

/// `sub_14018AFC0`: zoom by `delta` keeping the point under `at` fixed (render-time: no rebuild).
fn zoom_by(view: &mut DevotionView, screen: Vec2, delta: f32, at: Vec2) {
    let old = view.zoom;
    let new = (old + delta).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
    // At the limit nothing moves.
    if new == old {
        return;
    }
    let c = screen * 0.5 - view.scroll;
    let k = at - c;
    view.scroll -= k - k * (new / old);
    view.zoom = new;
    view.scroll = clamp_scroll(screen, new, view.scroll);
}

fn rgba(r: &gd_formats::arz::Record, p: &str) -> Color {
    let f = |c: &str, d: f32| r.f32(&format!("{p}{c}")).unwrap_or(d);
    Color::srgba(
        f("Red", 1.0),
        f("Green", 1.0),
        f("Blue", 1.0),
        f("Alpha", 1.0),
    )
}

/// Whether the star's skill text (module doc) contains the lowercased `query`.
fn star_matches(
    ui: &Ui,
    values: &HudValues,
    state: &DevotionState,
    skill: &str,
    query: &str,
) -> bool {
    let ctx = gd_sim::skill_tooltip::SkillTipCtx {
        db: &ui.data.db,
        trees: &ui.data.skill_trees(),
        tags: &ui.fonts.tags,
        book: &values.skills,
        player: None,
        pet_bonuses: &[],
    };
    let level = if values.skills.level(skill) > 0 {
        state
            .powers
            .get(&skill.to_ascii_lowercase())
            .map_or(1, |p| p.0.max(1))
    } else {
        1
    };
    search_lines(&ctx, skill, level)
        .iter()
        .any(|l| l.contains(query))
}

/// A star's search text (`sub_14017F5B0`): its skill's heading
/// (`Skill::CreateUISkillHeading`: the name, a power's with its auto-cast suffix, then the description) and the
/// stat lines of its current level (`CreateUIParameterText`; level 1 while untaken, `CreateUINextParameterText`),
/// markup stripped, lowercased. Not the "Bound To", rank, experience, requirement or next-level lines of the
/// tooltip. PORT: the engine builds the lines once when the window attaches the skills (a power ranked up later
/// keeps its old lines); here at the current rank.
fn search_lines(ctx: &gd_sim::skill_tooltip::SkillTipCtx, skill: &str, level: u32) -> Vec<String> {
    use gd_sim::skill_tooltip::{autocast_suffix, class, is_power, stat_lines};
    let Some(prof) = gd_sim::skills::profile_record(ctx.db, skill) else {
        return Vec::new();
    };
    let disp = gd_sim::skills::display_record(ctx.db, skill).unwrap_or_else(|| prof.clone());
    let tag = |f: &str| disp.str(f).map(|t| ctx.tag(t)).unwrap_or_default();
    let mut name = tag("skillDisplayName");
    if is_power(&prof) {
        name.push_str(&autocast_suffix(ctx, &prof));
    }
    let mut out = vec![name, tag("skillBaseDescription")];
    out.extend(
        stat_lines(ctx, skill, level, class::SKILL_STATS_CURRENT)
            .into_iter()
            .map(|l| l.text),
    );
    out.into_iter()
        .filter(|l| !l.is_empty())
        .map(|l| gd_formats::text::strip_markup(&l).to_lowercase())
        .collect()
}

pub fn build(
    ui: &mut Ui,
    values: &HudValues,
    state: &DevotionState,
    view: &DevotionView,
    max_points: u32,
) {
    let d = &state.data;
    let taken = |n: &str| values.skills.level(n) > 0;
    let aff = d.affinity(&taken);
    let screen = ui.screen;
    let full = URect::new(0.0, 0.0, screen.x, screen.y);
    ui.sound_blocker(full);
    let mut layout = DevotionLayout {
        window: Some(full),
        focus_inv: ui.inv,
        build: view.version,
        ..default()
    };
    let Some(m) = ui.record(MASTER) else {
        ui.commands.insert_resource(layout);
        return;
    };
    let mf = |k: &str| m.str(k).map(str::to_owned).unwrap_or_default();
    let origin = screen * 0.5 - view.built_scroll;
    // Zoom is render-time ([`pan_map`]): zooming out shows more build-space map. The margin covers the
    // whole zoom range plus the pan slack, and the map bounds around the built scroll (a zoom-out after
    // a long pan can show any of the map until the next rebuild).
    let margin = ((screen * 0.5 + Vec2::splat(REBUILD_PAN + 100.0)) / ZOOM_RANGE.0 - screen * 0.5)
        .max(map_size(screen.x) * 0.5 - screen * 0.5 + view.built_scroll.abs());
    // Draws a map bitmap at its record position (zoom 1); skipped off screen.
    let map_bitmap = |ui: &mut Ui, path: &str, x: f32, y: f32, color: Color| -> Option<URect> {
        let (h, size) = ui.image(path)?;
        // Edges are rounded, not the size: neighbouring tiles share an edge (a rounded size left 1 px seams
        // between the starfield tiles at fractional zooms).
        let (x0, y0) = ((origin.x + x).round(), (origin.y + y).round());
        let (x1, y1) = (
            (origin.x + x + size.x as f32).round(),
            (origin.y + y + size.y as f32).round(),
        );
        let r = URect::new(x0, y0, x1 - x0, y1 - y0);
        if r.x + r.w > -margin.x
            && r.y + r.h > -margin.y
            && r.x < screen.x + margin.x
            && r.y < screen.y + margin.y
        {
            ui.quad_stretch(h, r, color);
        }
        Some(r)
    };
    ui.solid(full, Color::srgb(0.01, 0.01, 0.03));
    // The map layers go under one container that [`pan_map`] scales and pans; the chrome below stays under the root.
    let real_root = ui.root;
    let map = ui
        .commands
        .spawn((
            DevotionMap {
                built: view.built_scroll,
                inv: ui.inv,
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            UiTransform::IDENTITY,
        ))
        .id();
    ui.commands.entity(real_root).add_child(map);
    ui.root = map;
    // Starfield tiles.
    let tile = mf("bgTile");
    if let Some((_, ts)) = ui.image(&tile) {
        let ms = map_size(screen.x);
        let (mut y, y1) = (-ms.y * 0.5, ms.y * 0.5);
        while y < y1 {
            let mut x = -ms.x * 0.5;
            while x < ms.x * 0.5 {
                map_bitmap(ui, &tile, x, y, Color::WHITE);
                x += ts.x as f32;
            }
            y += ts.y as f32;
        }
    }
    // Additive map bitmap (`sub_1401895C0`: nebulae, constellation art, stars and glows
    // use the "additive" blend style).
    let add_bitmap = |ui: &mut Ui, path: &str, x: f32, y: f32, color: Color| -> Option<URect> {
        let (h, size) = ui.image(path)?;
        let r = URect::new(
            (origin.x + x).round(),
            (origin.y + y).round(),
            size.x as f32,
            size.y as f32,
        );
        if r.x + r.w > -margin.x
            && r.y + r.h > -margin.y
            && r.x < screen.x + margin.x
            && r.y < screen.y + margin.y
        {
            additive::quad(ui, h, r, color);
        }
        Some(r)
    };
    // `nebulaSections` is a list of bitmap records.
    for n in m.strs("nebulaSections") {
        if let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(n) {
            add_bitmap(ui, &bitmap, x, y, Color::WHITE);
        }
    }
    // Constellation art.
    for (ci, c) in d.list.iter().enumerate() {
        let Some(cr) = ui.record(&c.record) else {
            continue;
        };
        let complete = d.complete(ci, &taken);
        let avail = d.available(ci, &aff);
        let any = c.stars.iter().any(|s| taken(&s.skill));
        // `sub_140180F00`: Unavailable colour, Available when the affinities are met,
        // Active when any star is taken; a partly taken constellation gets a second pass in the
        // shared `constellationColorPulse` colour ([`devotion_fx`]), a newly available one a white
        // flash pass (`constellationFlashPulse`).
        let key = if any {
            "constellationActive"
        } else if avail {
            "constellationAvailable"
        } else {
            "constellationUnavailable"
        };
        let color = rgba(&cr, key);
        if let Some(Widget::Bitmap { bitmap, x, y }) =
            ui.widget(cr.str("constellationBackground").unwrap_or(""))
            && let Some(r) = add_bitmap(ui, &bitmap, x, y, color)
        {
            layout.arts.push((ci, r));
            if let Some((h, _)) = ui.image(&bitmap) {
                if any && !complete {
                    let e = additive::quad(ui, h.clone(), r, Color::srgba(1.0, 0.7, 0.4, 0.0));
                    ui.commands
                        .entity(e)
                        .insert(ConstHighlight { image: h.clone() });
                }
                let e = additive::quad(ui, h.clone(), r, Color::srgba(1.0, 1.0, 1.0, 0.0));
                ui.commands.entity(e).insert((
                    ConstFlash {
                        ci,
                        image: h.clone(),
                    },
                    Visibility::Hidden,
                ));
            }
        }
    }
    // Links, then stars.
    let star_tex = |ui: &Ui, ci: usize, si: usize, which: &str| -> Option<String> {
        let b = ui.data.db.get(&d.list[ci].stars[si].button)?;
        b.str(which).map(str::to_owned)
    };
    let centre = |ui: &mut Ui, ci: usize, si: usize| -> Vec2 {
        let p = d.list[ci].stars[si].pos;
        let size = star_tex(ui, ci, si, "bitmapNameUp")
            .and_then(|t| ui.image(&t))
            .map_or(Vec2::splat(64.0), |(_, s)| s.as_vec2());
        origin + Vec2::from(p) + size * 0.5
    };
    let conn = |k: &str| mf(k);
    let (on, off, locked) = (
        conn("connectionActiveTexture"),
        conn("connectionInactiveTexture"),
        conn("connectionLockedTexture"),
    );
    // `sub_1401895C0` (links): a quad from star centre to star centre, `connectionWidth`
    // × zoom to each side of the line; the Active texture when both ends are taken, else the
    // Inactive one when the constellation's affinities are met, else the Locked one.
    let link_half = m.f32("connectionWidth").unwrap_or(7.0);
    for (ci, c) in d.list.iter().enumerate() {
        let avail = d.available(ci, &aff);
        for (si, s) in c.stars.iter().enumerate() {
            for &l in &s.links {
                let Some(o) = c.stars.get(l) else { continue };
                let (a, b) = (centre(ui, ci, si), centre(ui, ci, l));
                let tex = if taken(&s.skill) && taken(&o.skill) {
                    &on
                } else if avail {
                    &off
                } else {
                    &locked
                };
                let Some((h, _)) = ui.image(tex) else {
                    continue;
                };
                let (len, mid) = ((b - a).length(), (a + b) * 0.5);
                let (w, hgt) = (len, link_half * 2.0);
                if mid.x + len < -margin.x
                    || mid.y + len < -margin.y
                    || mid.x - len > screen.x + margin.x
                    || mid.y - len > screen.y + margin.y
                {
                    continue;
                }
                let e = ui.quad_stretch(
                    h,
                    URect::new(mid.x - w * 0.5, mid.y - hgt * 0.5, w, hgt),
                    Color::WHITE,
                );
                let ang = (b - a).to_angle();
                ui.commands
                    .entity(e)
                    .insert(UiTransform::from_rotation(Rot2::radians(ang)));
            }
        }
    }
    // Stars (`sub_14017E330`, all additive): taken = Down; else Disabled when locked
    // (has links and none of them taken, alpha 1, or the constellation's affinities unmet,
    // alpha 0.5), else Up; the hovered star (pointer inside its Up bitmap) gets InFocus on top.
    // Celestial powers get the `effectBgBitmap` glow first (alpha 0.8 when the affinities are met,
    // else 0.5; `sub_1401895C0`).
    let glow = mf("effectBgBitmap");
    // The search query as the engine builds it (`sub_140188870`): trimmed of spaces and braces, lowercased.
    let query = view
        .search
        .trim_matches(|c: char| c == ' ' || c == '{' || c == '}')
        .to_lowercase();
    let mut pulse_stars = Vec::new();
    let reclaiming = values.devotion_reclaim.is_some();
    for (ci, c) in d.list.iter().enumerate() {
        let avail = d.available(ci, &aff);
        for (si, s) in c.stars.iter().enumerate() {
            let power = is_power(&ui.data.db, &view.powers, &s.skill);
            let is_taken = taken(&s.skill);
            let lock_links = !s.links.is_empty()
                && !s
                    .links
                    .iter()
                    .any(|&l| c.stars.get(l).is_some_and(|o| taken(&o.skill)));
            let (which, alpha) = if is_taken {
                ("bitmapNameDown", 1.0)
            } else if !avail {
                ("bitmapNameDisabled", 0.5)
            } else if lock_links || reclaiming {
                ("bitmapNameDisabled", 1.0)
            } else {
                ("bitmapNameUp", 1.0)
            };
            if is_taken || avail && !lock_links {
                pulse_stars.push((ci, si));
            }
            if power && let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(&glow) {
                add_bitmap(
                    ui,
                    &bitmap,
                    s.pos[0] + x,
                    s.pos[1] + y,
                    Color::srgba(1.0, 1.0, 1.0, if avail { 0.8 } else { 0.5 }),
                );
            }
            let Some(tex) = star_tex(ui, ci, si, which) else {
                continue;
            };
            if let Some(r) = add_bitmap(
                ui,
                &tex,
                s.pos[0],
                s.pos[1],
                Color::srgba(1.0, 1.0, 1.0, alpha),
            ) {
                layout.hits.push((DevHit::Star(ci, si), r));
            }
            // A taken celestial power shows the icon of the skill it is bound to, 0.75 x its size, centred on the star
            // (`sub_14017E330`).
            if power
                && is_taken
                && let Some(skill) = state
                    .bindings
                    .iter()
                    .find(|b| b.power.eq_ignore_ascii_case(&s.skill))
                    .map(|b| b.skill.clone())
                && let Some(path) =
                    super::potions::skill_icon(ui.data, &skill, values.skills.level(&skill))
                && let Some((h, isz)) = ui.image(&path)
            {
                let (cx, cy) = (origin.x + s.pos[0] + 32.0, origin.y + s.pos[1] + 32.0);
                let (w, hgt) = (isz.x as f32 * 0.75, isz.y as f32 * 0.75);
                ui.quad(
                    h,
                    URect::new(
                        (cx - w * 0.5).round(),
                        (cy - hgt * 0.5).round(),
                        w.round(),
                        hgt.round(),
                    ),
                    None,
                    Color::WHITE,
                );
            }
            // `sub_1401895C0` (op 3 stars): when the bound skill exists and its level is 0, the `disabledBitmap`
            // (32 x 32, default blend) is drawn at the star's top-left + its bitmapPosition x zoom (not size-scaled).
            if power
                && let Some(skill) = state
                    .bindings
                    .iter()
                    .find(|b| b.power.eq_ignore_ascii_case(&s.skill))
                    .map(|b| b.skill.clone())
                && values.skills.level(&skill) == 0
                && let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(&mf("disabledBitmap"))
                && let Some((h, isz)) = ui.image(&bitmap)
            {
                ui.quad(
                    h,
                    URect::new(
                        (origin.x + s.pos[0] + x).round(),
                        (origin.y + s.pos[1] + y).round(),
                        isz.x as f32,
                        isz.y as f32,
                    ),
                    None,
                    Color::WHITE,
                );
            }
            // The search match: the Down bitmap again, red (`sub_14017E330`).
            if !query.is_empty()
                && star_matches(ui, values, state, &s.skill, &query)
                && let Some(tex) = star_tex(ui, ci, si, "bitmapNameDown")
            {
                add_bitmap(ui, &tex, s.pos[0], s.pos[1], Color::srgb(1.0, 0.0, 0.0));
            }
            if let Some(tex) = star_tex(ui, ci, si, "bitmapNameInFocus")
                && let Some((h, size)) = ui.image(&tex)
            {
                let r = URect::new(
                    (origin.x + s.pos[0]).round(),
                    (origin.y + s.pos[1]).round(),
                    size.x as f32,
                    size.y as f32,
                );
                layout.focus_art.push(((ci, si), h, r));
            }
        }
    }
    // Random star glows (`starColorPulseBitmap`), driven by `pulse_stars`.
    layout.pulse_stars = pulse_stars;
    if let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(&mf("starColorPulseBitmap"))
        && let Some((h, size)) = ui.image(&bitmap)
    {
        layout.pulse_art = Some((h.clone(), Vec2::new(x, y), size.as_vec2(), ui.inv));
        for i in 0..PULSE_SLOTS {
            let e = ui
                .commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                    Visibility::Hidden,
                    PulseSprite(i, view.version),
                ))
                .id();
            ui.commands.entity(ui.root).add_child(e);
            additive::queue_additive(ui.commands, e, h.clone(), Color::srgba(1.0, 1.0, 1.0, 0.0));
        }
    }
    ui.root = real_root;
    // Vignette (`edgeFade`): stretched over the whole window, drawn over the map, under the chrome
    // (`sub_1401895C0`).
    if let Some((h, _)) = ui.image(&mf("edgeFade")) {
        ui.quad_stretch(h, full, Color::WHITE);
    }
    // Sidebar: affinities and points, on a panel that slides in/out (the info tab, module doc).
    // The content is drawn at the open position under one container ([`sidebar_live`] moves it);
    // the tab handles stay at the fixed left-edge origin.
    let sc = |ui: &Ui, v: f32| ui.sc(v);
    if let Some(Widget::Bitmap { bitmap, .. }) = ui.widget(&mf("sideBar")) {
        let (w, h) = ui
            .image(&bitmap)
            .map_or((0.0, 0.0), |(_, s)| (s.x as f32, s.y as f32));
        let o = Vec2::new(0.0, ((screen.y - sc(ui, h)) * 0.5).round());
        let (pw, ph) = (sc(ui, w), sc(ui, h));
        layout.sidebar = Some(pw);
        let panel = ui
            .commands
            .spawn((
                SidebarPanel {
                    base_x: o.x,
                    width: pw,
                    inv: ui.inv,
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(o.x * ui.inv),
                    top: Val::Px(o.y * ui.inv),
                    width: Val::Px(pw * ui.inv),
                    height: Val::Px(ph * ui.inv),
                    ..default()
                },
            ))
            .id();
        ui.commands.entity(real_root).add_child(panel);
        let chrome_root = ui.root;
        ui.root = panel;
        let po = Vec2::ZERO;
        if let Some((_, r)) = ui.bitmap(&bitmap, 0.0, 0.0, po) {
            layout
                .hits
                .push((DevHit::Panel, URect::new(o.x + r.x, o.y + r.y, r.w, r.h)));
        }
        if let Some(r) = ui.record(&mf("affinityTitle")) {
            let tb = TextBox::from_record(&r);
            let t = ui.fonts.tag("tagDevotionAffinity").to_owned();
            ui.text(&tb, po, &t);
        }
        for n in 1..=5 {
            if let Some(Widget::Bitmap { bitmap, x, y }) =
                ui.widget(&mf(&format!("affinity0{n}Bitmap")))
            {
                ui.bitmap(&bitmap, x, y, po);
            }
            if let Some(r) = ui.record(&mf(&format!("affinity0{n}Number"))) {
                let tb = TextBox::from_record(&r);
                ui.text(&tb, po, &aff[n - 1].to_string());
            }
        }
        let text = |ui: &Ui, key: &str, a: &str, b: &str| -> String {
            let t = ui.fonts.tag(key).to_owned();
            t.replace("{%d0}", a)
                .replace("{%d1}", b)
                .replace("%d0", a)
                .replace("%d1", b)
        };
        if let Some(r) = ui.record(&mf("pointsRemaining")) {
            // `sub_14018F070`: the singular tag at one point, `^r` (red) at none.
            let key = if state.points == 0 {
                "NoDevotionPointsAvailable"
            } else if state.points == 1 {
                "DevotionPointsAvailableSingular"
            } else {
                "DevotionPointsAvailable"
            };
            let mut tb = TextBox::from_record(&r);
            tb.style = None;
            let st = if state.points == 0 {
                "records/ui/styles/text/style_nooutline_textred_sizen.dbr"
            } else {
                TEXT_STYLE
            };
            let st = if ui.data.db.get(st).is_some() {
                st
            } else {
                TEXT_STYLE
            };
            let t = text(ui, key, &state.points.to_string(), "");
            ui.text_with_style(&tb, st, po, &t);
        }
        if let Some(r) = ui.record(&mf("pointsTotal")) {
            let mut tb = TextBox::from_record(&r);
            tb.style = None;
            let t = text(
                ui,
                "DevotionPointsTotal",
                &state.total.to_string(),
                &max_points.to_string(),
            );
            ui.text_with_style(&tb, TEXT_STYLE, po, &t);
        }
        if let Some(r) = &values.devotion_reclaim {
            reclaim_texts(ui, values, state, r, &m, po);
        }
        ui.root = chrome_root;
        // The tab handles (render `sub_14018EBA0`): the close handle while the panel is
        // out, the open handle while it is away; both at the fixed origin, [`sidebar_live`] flips
        // their visibility, both slide it.
        for (key, open) in [("infoTabClose", false), ("infoTabOpen", true)] {
            if let Some(Widget::Button { up, x, y, .. }) = ui.widget(&mf(key))
                && let Some(b) = up
                && let Some((e, r)) = ui.bitmap(&b, x, y, o)
            {
                layout.hits.push((DevHit::InfoTab(open), r));
                ui.record_path_button_sound(r, &mf(key));
                let shown = (view.sidebar > 0.0) != open;
                ui.commands.entity(e).insert((
                    SidebarHandle(open),
                    if shown {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                ));
            }
        }
    }
    // Top bar.
    let oc = Vec2::new((screen.x * 0.5).round(), 0.0);
    if let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(&mf("searchBoxBackground")) {
        ui.bitmap(&bitmap, x, y, oc);
    }
    let btn = |ui: &mut Ui,
               key: &str,
               hit: Option<DevHit>,
               layout: &mut DevotionLayout|
     -> Option<(f32, f32, URect)> {
        let Some(Widget::Button { up, x, y, .. }) = ui.widget(&mf(key)) else {
            return None;
        };
        let (_, r) = ui.bitmap(&up?, x, y, oc)?;
        if let Some(h) = hit {
            layout.hits.push((h, r));
            ui.record_path_button_sound(r, &mf(key));
        }
        Some((x, y, r))
    };
    // Undo: the disabled bitmap while nothing changed since the window opened
    // (sub_140188870, see `gd_game::devotion::DevotionUndo`).
    if let Some(Widget::Button {
        up, disabled, x, y, ..
    }) = ui.widget(&mf("undoButton"))
        && let Some(b) = if values.devotion_undo {
            up.or(disabled)
        } else {
            disabled.or(up)
        }
        && let Some((_, r)) = ui.bitmap(&b, x, y, oc)
        && values.devotion_undo
    {
        layout.hits.push((DevHit::Undo, r));
        ui.record_path_button_sound(r, &mf("undoButton"));
    }
    btn(ui, "helpButton", Some(DevHit::Help), &mut layout);
    // The search box: its typed text (the engine's box has no background of its own) and, while there is text, the
    // clear button.
    if let Some(r) = ui.record(&mf("searchBox")) {
        // The record is an `editbox.tpl`: `LocationX/Y` from the window's top centre, `Width`, `Height`, `Style`.
        let f = |k: &str| r.f32(k).unwrap_or(0.0);
        let rect = URect::new(
            oc.x + ui.sc(f("LocationX")),
            oc.y + ui.sc(f("LocationY")),
            ui.sc(f("Width")),
            ui.sc(f("Height")),
        );
        layout.hits.push((DevHit::SearchBox, rect));
        if let Some(style) = r.str("Style")
            && (!view.search.is_empty() || view.typing)
        {
            let lh = ui.line_height(style);
            let y = (rect.y + (rect.h - lh) * 0.5).round();
            ui.text_line(style, Vec2::new(rect.x, y), &view.search, 1.0);
            if view.typing {
                let pre = |ui: &mut Ui, n: usize| {
                    ui.text_width(style, &view.search.chars().take(n).collect::<String>())
                };
                // PORT: the selection highlight colour (as the market search box).
                if let Some((a, b)) = view.edit.selection().filter(|(a, b)| a < b) {
                    let (xa, xb) = (rect.x + pre(ui, a), rect.x + pre(ui, b));
                    ui.solid(
                        URect::new(xa, rect.y, xb - xa, rect.h),
                        Color::srgba(0.4, 0.5, 0.9, 0.5),
                    );
                }
                // The caret (`sub_140193800`: 1 px white, blinking: `trade::animate_search`).
                let x = rect.x + pre(ui, view.edit.caret.min(view.search.chars().count()));
                let e = ui.solid(
                    URect::new(x, rect.y, ui.sc(1.0).max(1.0), rect.h),
                    Color::WHITE,
                );
                ui.commands.entity(e).insert(super::trade::SearchCaret);
            }
        }
    }
    if !view.search.is_empty() {
        btn(
            ui,
            "clearSearchButton",
            Some(DevHit::ClearSearch),
            &mut layout,
        );
    }
    btn(ui, "closeButton", Some(DevHit::Close), &mut layout);
    btn(ui, "zoomInButton", Some(DevHit::ZoomIn), &mut layout);
    btn(ui, "zoomOutButton", Some(DevHit::ZoomOut), &mut layout);
    if let Some(r) = ui.record(&mf("zoomText")) {
        let tb = TextBox::from_record(&r);
        let t = ui.fonts.tag("tagAerialMapZoomText").to_owned();
        ui.text(&tb, oc, &t);
    }
    if let Some(r) = ui.record(&mf("undoButton")) {
        let _ = r;
        let t = ui.fonts.tag("tagSkillUndoGeneric").to_owned();
        if let Some(Widget::Button { x, y, .. }) = ui.widget(&mf("undoButton")) {
            let tb = TextBox {
                style: None,
                rect: URect::new(x, y, 68.0, 34.0),
                align_x: TextAlign::Center,
                align_y: TextAlign::Center,
                tag: None,
                glow: None,
            };
            ui.text_with_style(&tb, TAB_STYLE, oc, &t);
        }
    }
    // Tabs: the chosen masteries and this one (`devotionbuttons_tab1/2/3button`).
    let trees = ui.data.skill_trees();
    let masteries = trees.masteries(&values.skills);
    let shown = (masteries + (masteries < 2) as usize).min(2);
    for (key, tab) in [
        ("skillsTab1Button", 1u8),
        ("skillsTab2Button", 2),
        ("skillsTab3Button", 0),
    ] {
        if tab as usize > shown {
            continue;
        }
        let Some(Widget::Button { up, down, x, y, .. }) = ui.widget(&mf(key)) else {
            continue;
        };
        let Some(b) = (if tab == 0 { down.or(up) } else { up }) else {
            continue;
        };
        let Some((_, r)) = ui.bitmap(&b, x, y, oc) else {
            continue;
        };
        if tab != 0 {
            layout.hits.push((DevHit::Tab(tab), r));
            ui.record_path_button_sound(r, &mf(key));
        }
        let title = if tab == 0 {
            ui.fonts.tag("tagSkillDevotion").to_owned()
        } else {
            match trees.learned(&values.skills).nth(tab as usize - 1) {
                Some(t) => ui
                    .record("records/ui/skills/skills_mastertable.dbr")
                    .and_then(|mt| {
                        mt.str(&format!("skillCtrlPane{}", t.class))
                            .map(str::to_owned)
                    })
                    .and_then(|p| ui.record(&p))
                    .and_then(|t| t.str("skillTabTitle").map(str::to_owned))
                    .map(|t| ui.fonts.tag(&t).to_owned())
                    .unwrap_or_default(),
                None => ui.fonts.tag("tagSkillClassSelect").to_owned(),
            }
        };
        let tb = TextBox {
            style: None,
            rect: URect::new(x, y, r.w / ui.s, r.h / ui.s),
            align_x: TextAlign::Center,
            align_y: TextAlign::Center,
            tag: None,
            glow: None,
        };
        ui.text_with_style(&tb, TAB_STYLE, oc, &title);
    }
    if let Some(pk) = &view.pick {
        pick_list(ui, pk, values, &mut layout);
    }
    ui.commands.insert_resource(layout);
}

/// The reclaim mode's sidebar lines (module doc): each label (`textTag`, its `bitmapName` centred on the text box as
/// the skills window's, `ui::skills`) and its number.
fn reclaim_texts(
    ui: &mut Ui,
    values: &HudValues,
    state: &DevotionState,
    r: &super::reclamation::DevotionReclaimView,
    m: &gd_formats::arz::Record,
    o: Vec2,
) {
    let mf = |k: &str| m.str(k).map(str::to_owned).unwrap_or_default();
    let red = |n: u64, t: String| if n == 0 { format!("^r{t}") } else { t };
    let money = super::character::money_text;
    let (cost, _) = gd_game::devotion::reclaim_cost(&ui.data.db, state.reclaimed);
    for (label, number, text) in [
        (
            "totalGoldText",
            "totalGoldNumber",
            red(values.gold, money(values.gold)),
        ),
        (
            "totalAetherText",
            "totalAetherNumber",
            red(r.aether as u64, money(r.aether as u64)),
        ),
        ("goldCostText", "goldCostNumber", money(cost)),
    ] {
        if let Some(lr) = ui.record(&mf(label)) {
            let tb = TextBox::from_record(&lr);
            if let Some(b) = lr.str("bitmapName").map(str::to_string)
                && let Some((h, size)) = ui.image(&b)
            {
                let (w, hgt) = (ui.sc(size.x as f32), ui.sc(size.y as f32));
                let c = Vec2::new(
                    o.x + ui.sc(tb.rect.x + tb.rect.w * 0.5),
                    o.y + ui.sc(tb.rect.y + tb.rect.h * 0.5),
                );
                ui.quad(
                    h,
                    URect::new(c.x - w * 0.5, c.y - hgt * 0.5, w, hgt),
                    None,
                    Color::WHITE,
                );
            }
            let t = lr
                .str("textTag")
                .map(|t| ui.fonts.tag(t).to_string())
                .unwrap_or_default();
            ui.text(&tb, o, &t);
        }
        if let Some(nr) = ui.record(&mf(number)) {
            ui.text(&TextBox::from_record(&nr), o, &text);
        }
    }
}

/// The skill-select list for star `star` (module doc).
fn pick_state(
    star: (usize, usize),
    at: Vec2,
    loadout: &gd_game::player::Loadout,
    state: &DevotionState,
    db: &gd_formats::vfs::Database,
) -> PickState {
    let power = &state.data.list[star.0].stars[star.1].skill;
    let bound = state
        .bindings
        .iter()
        .find(|b| b.power.eq_ignore_ascii_case(power))
        .map(|b| (b.skill.clone(), b.item.clone()));
    use super::slot_select::PickLook;
    // sub_1401D46C0 / sub_1401D4C60 / sub_1401E5020: the bound skill's row is selected (+442) and reads
    // `tagDevotionSelected`; a skill at level 0 is disabled (+441); one of a level above 0 that is bound to another
    // power (`Skill::HasAutocastSkill`) reads `tagDevotionInUse` (+443).
    let options = power_options(db, power, loadout, bound.as_ref(), |n| {
        state.data.locate(n).is_some()
    })
    .into_iter()
    .enumerate()
    .map(|(i, (skill, level0))| {
        let (look, suffix) = if i == 1 && bound.is_some() {
            (PickLook::Selected, Some("tagDevotionSelected"))
        } else if level0 {
            (PickLook::Disabled, None)
        } else if skill.as_ref().is_some_and(|(sk, item)| {
            state
                .bindings
                .iter()
                .any(|b| b.skill.eq_ignore_ascii_case(sk) && &b.item == item)
        }) {
            (PickLook::Normal, Some("tagDevotionInUse"))
        } else {
            (PickLook::Normal, None)
        };
        PowerPick {
            skill,
            look,
            suffix,
        }
    })
    .collect();
    PickState {
        star,
        at,
        options,
        hover: None,
    }
}

/// What a celestial power's skill-select list offers, in order (`Grim Dawn.exe`: the window's list is the
/// `SlotSelectWindow` subclass vtable off_140318488: fill sub_1401D46C0, filter sub_1401D4C60): the empty pick
/// ("unassigned"); the skill the power is bound to, if any; then the player's skills (`Character::GetSkillList`),
/// then its item skills (`GetItemSkillList`, then `GetItemSkillCache`), each kept when it is not the bound one,
/// not the mastery skill (`Skill_Mastery`), not one of the default skills (`SkillManager::IsDefaultSkill`), not a
/// `Skill_Modifier`, has no `autoCastSkill` / `autoCastController` of its own (`Skill::HasAutocastInDbr`), is no
/// item skill with an item auto-cast controller (`SkillManager::AddItemSkills` sets +762 for those), is an
/// ordinary skill (operation 0: not a devotion star), its class is exactly the class of one of the power's
/// `skillTemplates` records (`Skill::IsSkillA` against the classes `Skill::LoadResources` made from them) and its
/// record is not in the power's `skillBlackList`; a player skill must also belong to one of the player's
/// masteries (always so for learned ones). The power's fields come from its profile record (a wrapper's
/// `buffSkillName`, as `templateAutoCast`). The `bool` is a skill at level 0 (greyed, [`pick_state`]): a skill of the
/// book left at no points, and every skill of the item skill cache (`gd_game::player::Loadout::set_items`: the
/// items taken off this session). PORT: the item skill list's sort is not ported (item skills in equipment order).
fn power_options(
    db: &gd_formats::vfs::Database,
    power: &str,
    loadout: &gd_game::player::Loadout,
    bound: Option<&PickSkill>,
    is_star: impl Fn(&str) -> bool,
) -> Vec<(Option<PickSkill>, bool)> {
    let mut out: Vec<(Option<PickSkill>, bool)> = vec![(None, false)];
    let Some(prof) = gd_sim::skills::profile_record(db, power) else {
        return out;
    };
    let classes: Vec<String> = prof
        .strs("skillTemplates")
        .iter()
        .filter_map(|t| db.get(t).map(|r| r.class.clone()))
        .collect();
    let black = prof.strs("skillBlackList");
    if let Some(b) = bound {
        out.push((Some(b.clone()), false));
    }
    // Each item skill object is its own row (the engine lists `Skill` objects): worn ones, then the cache's.
    let item_skills = gd_game::player::sourced_grants(db, &loadout.items)
        .into_iter()
        .chain(
            loadout
                .item_skill_cache
                .iter()
                .map(|c| (c.grant.clone(), c.source.clone())),
        )
        .filter(|(g, _)| g.controller.is_none())
        .map(|(g, src)| ((g.skill, Some(src)), g.level == 0));
    let learned = loadout
        .skills
        .skills
        .iter()
        .map(|(n, points)| ((n.clone(), None), *points == 0));
    for (key, level0) in learned.chain(item_skills) {
        let sk = &key.0;
        let same = |o: &PickSkill| o.0.eq_ignore_ascii_case(sk) && o.1 == key.1;
        if bound.is_some_and(same)
            || out.iter().any(|o| o.0.as_ref().is_some_and(same))
            || is_star(sk)
            || sk
                .to_ascii_lowercase()
                .starts_with("records/skills/default/")
            || black.iter().any(|b| b.eq_ignore_ascii_case(sk))
        {
            continue;
        }
        let Some(rec) = db.get(sk) else { continue };
        let own = gd_sim::skills::profile_record(db, sk).unwrap_or_else(|| rec.clone());
        let autocast = ["autoCastSkill", "autoCastController"]
            .iter()
            .any(|f| own.str(f).is_some_and(|v| !v.is_empty()));
        if rec.class == "Skill_Mastery"
            || rec.class.starts_with("Skill_Modifier")
            || autocast
            || !classes.contains(&rec.class)
        {
            continue;
        }
        out.push((Some(key), level0));
    }
    out
}

/// The skill-select list (`devotion_skillselect`, a `SlotSelectWindow`: [`super::slot_select`], centred on the
/// star; the empty pick shows the pick template's `displayBitmap` and `tagDevotionUnassigned`).
fn pick_list(ui: &mut Ui, pk: &PickState, values: &HudValues, layout: &mut DevotionLayout) {
    let Some(m) = ui.record(MASTER) else { return };
    let Some(sel) = m.str("skillSelect").and_then(|r| ui.record(r)) else {
        return;
    };
    let picks: Vec<super::slot_select::Pick> = pk
        .options
        .iter()
        .map(|o| match &o.skill {
            None => super::slot_select::empty_pick(ui, &sel),
            Some((sk, _)) => super::slot_select::Pick {
                icon: super::potions::skill_icon(ui.data, sk, values.skills.level(sk).max(1)),
                name: gd_sim::skills::profile_record(&ui.data.db, sk)
                    .and_then(|r| r.str("skillDisplayName").map(str::to_owned))
                    .map(|t| gd_formats::text::strip_markup(ui.fonts.tag(&t)))
                    .unwrap_or_else(|| sk.clone())
                    + &o.suffix
                        .map(|t| ui.fonts.tag(t).to_string())
                        .unwrap_or_default(),
                selected: false,
                look: o.look,
            },
        })
        .collect();
    let (panel, rows) = super::slot_select::draw(
        ui,
        &sel,
        &picks,
        super::slot_select::Place::Centred(pk.at),
        pk.hover,
        None,
    );
    layout.hits.push((DevHit::PickPanel, panel));
    for (i, row) in rows.into_iter().enumerate() {
        layout.hits.push((DevHit::Pick(i), row));
    }
}

/// Clicks, pan / zoom, hover and binding keys on the devotion window.
#[allow(clippy::too_many_arguments)]
pub fn clicks(
    hooks: Res<crate::hooks::Hooks>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<super::keys::KeyBindings>,
    wheel: Res<AccumulatedMouseScroll>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut panels: ResMut<super::UiPanels>,
    layout: Res<DevotionLayout>,
    mut capture: ResMut<UiCapture>,
    state: Option<Res<DevotionState>>,
    mut view: ResMut<DevotionView>,
    (casters, loadouts, data, pause, journal_layout): (
        Query<&crate::casting::Caster, With<Player>>,
        Query<&gd_game::player::Loadout, With<Player>>,
        Res<crate::data::GameData>,
        Res<gd_game::pause::GamePause>,
        Res<super::quests::JournalLayout>,
    ),
    mut requests: DevotionRequests,
    (mut drag, mut pressed): (Local<Option<Vec2>>, Local<Option<(usize, usize)>>),
    mut was_open: Local<bool>,
    mut hooked: Local<bool>,
) {
    // The window hid: its changes are final (sub_14018B120 / sub_14018B5B0).
    if *was_open && !panels.devotion {
        requests.commit.write(gd_game::devotion::CommitDevotionUndo);
    }
    if !*was_open && panels.devotion {
        requests
            .unlock
            .write(gd_game::tutorials::UnlockTutorial(HELP_INDEX));
    }
    *was_open = panels.devotion;
    let (layout, Some(state)) = (layout, state) else {
        return;
    };
    if !panels.devotion {
        *drag = None;
        return;
    }
    let (Ok(win), Some(_)) = (window.single(), layout.window) else {
        return;
    };
    let screen = Vec2::new(win.physical_width() as f32, win.physical_height() as f32);
    // `GD_DEVOTION_PICK` (hooks.rs): opens that star's skill-select list once.
    if let Some(n) = &hooks.ui.devotion_pick
        && !*hooked
        && let Ok(loadout) = loadouts.single()
        && let Some((ci, si)) = if n == "auto" {
            // `auto`: the first taken celestial power.
            state.data.list.iter().enumerate().find_map(|(ci, c)| {
                c.stars
                    .iter()
                    .position(|st| {
                        loadout.skills.level(&st.skill) > 0
                            && is_power(&data.db, &view.powers, &st.skill)
                    })
                    .map(|si| (ci, si))
            })
        } else {
            state.data.locate(n)
        }
    {
        *hooked = true;
        info!("devotion: pick list for star {ci},{si}");
        let at = star_centre(&layout, (ci, si), &view, screen).unwrap_or(screen * 0.5);
        view.pick = Some(pick_state((ci, si), at, loadout, &state, &data.db));
        view.version += 1;
    }
    // A celestial power clicked to be taken: its list opens once it is (`sub_14017EB80` sets the open flag when it
    // takes an operation-3 skill).
    if let Some((star, at)) = view.pick_after
        && let Ok(loadout) = loadouts.single()
        && loadout
            .skills
            .level(&state.data.list[star.0].stars[star.1].skill)
            > 0
    {
        view.pick_after = None;
        view.pick = Some(pick_state(star, at, loadout, &state, &data.db));
        view.version += 1;
    }
    // No cursor, the game menu / Options over the map, or the journal (its Help button opens it on top) under the
    // cursor: nothing here is hovered, dragged, zoomed, clicked or bound (the wheel is a per-frame accumulator, so
    // skipping it drops it). The pick list's row hover stays as it was.
    let p = super::cursor_px(&hooks, win)
        .filter(|p| !pause.menu && !(requests.journal.open && journal_layout.covers(*p)));
    let Some(p) = p else {
        if view.hover.is_some() {
            view.hover = None;
        }
        *drag = None;
        *pressed = None;
        return;
    };
    capture.0 = true;
    // The map layers live in build coordinates (the container scales and pans them); the chrome is
    // in screen px, the sidebar panel's hit rect shifted by its slide.
    let slide = layout.sidebar.map_or(0.0, |w| (view.sidebar - 1.0) * w);
    let map_p = to_map(&view, screen, p);
    let at = |h: Option<DevHit>| match h {
        Some(DevHit::Panel) => p - Vec2::new(slide, 0.0),
        _ if panned(h) => map_p,
        _ => p,
    };
    let top = layout
        .hits
        .iter()
        .rev()
        .find(|(h, r)| match h {
            // Only the visible tab handle counts ([`sidebar_live`] hides the other).
            DevHit::InfoTab(open) => *open == (view.sidebar <= 0.0) && contains(r, p),
            _ => contains(r, at(Some(*h))),
        })
        .map(|(h, _)| *h);
    // The hovered row of the skill-select list (tinted, `slot_select`).
    if let Some(pk) = &view.pick {
        // A disabled row is never hovered (sub_1401E4C50).
        let h = match top {
            Some(DevHit::Pick(i))
                if pk
                    .options
                    .get(i)
                    .is_some_and(|o| o.look != super::slot_select::PickLook::Disabled) =>
            {
                Some(i)
            }
            _ => None,
        };
        if pk.hover != h {
            if let Some(pk) = view.pick.as_mut() {
                pk.hover = h;
            }
            view.version += 1;
        }
    }
    if buttons.just_pressed(MouseButton::Left) {
        view.pick_after = None;
    }
    // A click outside the skill-select list closes it (Esc through the shared dispatcher,
    // `ui::esc_dispatcher`).
    if view.pick.is_some()
        && buttons.just_pressed(MouseButton::Left)
        && !matches!(top, Some(DevHit::Pick(_) | DevHit::PickPanel))
    {
        view.pick = None;
        view.version += 1;
    }
    // A click outside the search box leaves it (`EditBox` deactivation, `sub_140195780`).
    if view.typing && buttons.just_pressed(MouseButton::Left) && top != Some(DevHit::SearchBox) {
        view.typing = false;
        view.version += 1;
    }
    // Hover: the star, else the smallest constellation art under the cursor.
    let hover = match top {
        Some(DevHit::Star(c, s)) => Some((c, Some(s))),
        None => layout
            .arts
            .iter()
            .filter(|(_, r)| contains(r, at(None)))
            .min_by(|a, b| (a.1.w * a.1.h).total_cmp(&(b.1.w * b.1.h)))
            .map(|(c, _)| (*c, None)),
        _ => None,
    };
    if hover != view.hover.map(|(c, s, _)| (c, s)) {
        // Not a rebuild: [`focus`] draws the hovered star.
        view.hover = hover.map(|(c, s)| (c, s, p));
    }
    // Pan (left drag on the map) and wheel zoom.
    if buttons.just_pressed(MouseButton::Left) {
        *drag = matches!(top, None | Some(DevHit::Star(..))).then_some(p);
    }
    if !buttons.pressed(MouseButton::Left) {
        *drag = None;
    }
    if let Some(last) = *drag
        && p != last
    {
        let zoom = view.zoom;
        view.scroll = clamp_scroll(screen, zoom, view.scroll - (p - last));
        // A small pan only moves the container ([`pan_map`]); past `REBUILD_PAN` the layers are rebuilt around the scroll.
        let pan = view.built_scroll - view.scroll;
        if pan.x.abs().max(pan.y.abs()) > REBUILD_PAN {
            view.version += 1;
        }
        *drag = Some(p);
    }
    if wheel.delta.y != 0.0 && !matches!(top, Some(DevHit::Panel)) {
        let notches = match wheel.unit {
            MouseScrollUnit::Line => wheel.delta.y * 120.0,
            MouseScrollUnit::Pixel => wheel.delta.y,
        };
        zoom_by(&mut view, screen, (notches * 0.001).clamp(-0.1, 0.1), p);
    }
    let reclaiming = requests.mode.as_ref().is_some_and(|m| m.devotion_mode());
    // A star acts on the release over it after a press on it (module doc, `sub_14017EB80`).
    let star_press = *pressed;
    if buttons.just_pressed(MouseButton::Left) {
        *pressed = match top {
            Some(DevHit::Star(c, s)) => Some((c, s)),
            _ => None,
        };
    }
    let Some(hit) = top else { return };
    let (c, s) = match hit {
        DevHit::Close => {
            if buttons.just_pressed(MouseButton::Left) {
                panels.devotion = false;
            }
            return;
        }
        DevHit::Undo => {
            if buttons.just_pressed(MouseButton::Left) {
                requests.undo.write(gd_game::devotion::UndoDevotion);
            }
            return;
        }
        DevHit::Tab(t) => {
            if buttons.just_pressed(MouseButton::Left) {
                panels.skill_tab = t;
                if reclaiming {
                    // The mode moves to the skills window (`sub_14018C4D0`); the answer opens it.
                    requests
                        .reclaim
                        .write(gd_game::reclamation::ReclaimRequest::Switch { devotion: false });
                } else {
                    panels.devotion = false;
                    panels.skills = true;
                }
            }
            return;
        }
        DevHit::ZoomIn | DevHit::ZoomOut => {
            if buttons.just_pressed(MouseButton::Left) {
                zoom_by(
                    &mut view,
                    screen,
                    if hit == DevHit::ZoomIn { 0.1 } else { -0.1 },
                    screen * 0.5,
                );
            }
            return;
        }
        DevHit::Panel => return,
        DevHit::InfoTab(_) => {
            // The reclaim mode holds the sidebar open (`sub_14018F9F0`); otherwise the
            // handle toggles the slide ([`devotion_fx`] animates it).
            if buttons.just_pressed(MouseButton::Left) && !reclaiming {
                view.sidebar_open = !view.sidebar_open;
            }
            return;
        }
        DevHit::SearchBox => {
            if buttons.just_pressed(MouseButton::Left) && !view.typing {
                view.typing = true;
                let t = view.search.clone();
                view.edit.to_end(&t);
                view.version += 1;
            }
            return;
        }
        DevHit::ClearSearch => {
            if buttons.just_pressed(MouseButton::Left) {
                view.search.clear();
                view.edit = Default::default();
                view.version += 1;
            }
            return;
        }
        DevHit::PickPanel => return,
        DevHit::Pick(i) => {
            if buttons.just_pressed(MouseButton::Left)
                && let Some(pk) = view.pick.take()
            {
                let power = state.data.list[pk.star.0].stars[pk.star.1].skill.clone();
                match pk.options.get(i) {
                    // A disabled row takes no click (sub_1401E4C50): the list stays.
                    Some(o) if o.look == super::slot_select::PickLook::Disabled => {
                        view.pick = Some(pk);
                        return;
                    }
                    Some(o) => {
                        requests.bind.write(gd_game::devotion::BindPower {
                            power,
                            skill: o.skill.as_ref().map(|s| s.0.clone()),
                            item: o.skill.as_ref().and_then(|s| s.1.clone()),
                        });
                    }
                    None => {}
                }
                view.version += 1;
            }
            return;
        }
        DevHit::Help => {
            if buttons.just_pressed(MouseButton::Left) {
                requests.journal.open_help(HELP_PAGE);
            }
            return;
        }
        DevHit::Star(c, s) => (c, s),
    };
    let skill = state.data.list[c].stars[s].skill.clone();
    let released = buttons.just_released(MouseButton::Left)
        && (star_press == Some((c, s)) || *pressed == Some((c, s)));
    if released {
        *pressed = None;
    }
    // Binding: the bound hotbar key over a taken celestial power.
    if let Some(slot) =
        (0..10usize).find(|&i| bindings.just_pressed(super::keys::HOT_SLOT_FIRST + i as u32, &keys))
        && let Ok(caster) = casters.single()
        && let Some(target) = caster.slots[slot].clone()
    {
        requests.bind.write(gd_game::devotion::BindPower {
            power: skill,
            skill: Some(target),
            item: None,
        });
        return;
    }
    // In the reclaim mode a left click refunds the star (module doc).
    if reclaiming {
        if released {
            requests.remove.write(gd_game::devotion::RemoveStar {
                constellation: c,
                star: s,
            });
        }
        return;
    }
    // A left click on a taken celestial power opens the skill-select list, centred on the star (`sub_14017EB80`,
    // `sub_1401853B0`: the list's origin is the star's centre).
    let centre = star_centre(&layout, (c, s), &view, screen).unwrap_or(p);
    if released
        && let Ok(loadout) = loadouts.single()
        && loadout.skills.level(&skill) > 0
        && is_power(&data.db, &view.powers, &skill)
    {
        view.pick = Some(pick_state((c, s), centre, loadout, &state, &data.db));
        view.version += 1;
        return;
    }
    if released {
        requests.take.write(gd_game::devotion::TakeStar {
            constellation: c,
            star: s,
        });
        if is_power(&data.db, &view.powers, &skill) {
            view.pick_after = Some(((c, s), centre));
        }
    }
}

/// The centre of star `star` on screen (its hit rectangle is the Up bitmap in map build coordinates).
fn star_centre(
    layout: &DevotionLayout,
    star: (usize, usize),
    view: &DevotionView,
    screen: Vec2,
) -> Option<Vec2> {
    layout
        .hits
        .iter()
        .find(|(h, _)| *h == DevHit::Star(star.0, star.1))
        .map(|(_, r)| to_screen(view, screen, Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5)))
}

/// The devotion window's requests.
#[derive(bevy::ecs::system::SystemParam)]
pub struct DevotionRequests<'w> {
    take: MessageWriter<'w, gd_game::devotion::TakeStar>,
    remove: MessageWriter<'w, gd_game::devotion::RemoveStar>,
    bind: MessageWriter<'w, gd_game::devotion::BindPower>,
    /// The mode's tab switch (`gd_game::reclamation`).
    reclaim: MessageWriter<'w, gd_game::reclamation::ReclaimRequest>,
    /// Option: created with the player (`gd_game::reclamation::init_reclamation`).
    mode: Option<Res<'w, gd_game::reclamation::SkillReclamation>>,
    undo: MessageWriter<'w, gd_game::devotion::UndoDevotion>,
    commit: MessageWriter<'w, gd_game::devotion::CommitDevotionUndo>,
    journal: ResMut<'w, super::quests::QuestJournal>,
    unlock: MessageWriter<'w, gd_game::tutorials::UnlockTutorial>,
}

/// The tutorial page `helpButton` shows (`ShowTutorialPage(0x3B)`: entry 59 of the help index's `tutorialPages`).
const HELP_PAGE: &str = "records/ui/tutorial/openwindow_devotion/tutorial_p60_devotion.dbr";
/// Its index: unlocked when the window opens (`sub_14018B120`).
const HELP_INDEX: u32 = 59;

/// The keyboard goes to the search box while it is active: `gd_sim::edit` (caret, selection, repeat;
/// `sub_140194A60`, as the market search box in `trade.rs`) edits the text, Enter leaves the box (and only
/// that: the window stays; Esc leaves it through the shared dispatcher, `ui::esc_dispatcher`); the window
/// closing clears the text (`sub_14018B120`) and the box. It runs in `PreUpdate` ahead of every hotkey reader
/// and resets the `ButtonInput` (Escape stays for the dispatcher), so the game's keys see nothing of what
/// is typed (the leaving Enter included). PORT: the clipboard is the process's own (`trade::Clipboard`).
#[allow(clippy::too_many_arguments)]
fn search_typing(
    mut view: ResMut<DevotionView>,
    panels: Res<super::UiPanels>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    data: Res<crate::data::GameData>,
    mut fonts: ResMut<super::text::FontCache>,
    mut font_assets: ResMut<Assets<Font>>,
    time: Res<Time<Real>>,
    mut clipboard: ResMut<crate::trade::Clipboard>,
    mut mods: Local<(bool, bool)>,
) {
    use bevy::input::keyboard::{Key, KeyCode as K};
    use gd_sim::edit::EditKey;
    if !panels.devotion {
        if !view.search.is_empty() || view.typing {
            view.search.clear();
            view.typing = false;
            view.edit = Default::default();
            view.version += 1;
        }
        typed.clear();
        return;
    }
    if !view.typing {
        typed.clear();
        return;
    }
    let Some(rec) = data
        .db
        .get(MASTER)
        .and_then(|m| m.str("searchBox").map(str::to_owned))
        .and_then(|p| data.db.get(&p))
    else {
        typed.clear();
        return;
    };
    let style = rec.str("Style").unwrap_or_default().to_owned();
    let max = rec.f32("Width").unwrap_or(0.0) - 10.0;
    let mut text = view.search.clone();
    let mut edit = view.edit.clone();
    let mut typing = true;
    let measure = std::cell::RefCell::new((&mut *fonts, &mut *font_assets));
    let fits = |t: &str| {
        let mut m = measure.borrow_mut();
        let (f, a) = &mut *m;
        f.text_width(&data, a, &style, 1.0, t) <= max
    };
    edit.tick(&mut text, time.delta_secs() * 1000.0, &fits);
    for e in typed.read() {
        if !e.state.is_pressed() {
            if matches!(e.key_code, K::ShiftLeft | K::ShiftRight) {
                mods.0 = false;
            }
            if matches!(e.key_code, K::ControlLeft | K::ControlRight) {
                mods.1 = false;
            }
            edit.release();
            continue;
        }
        match e.key_code {
            K::ShiftLeft | K::ShiftRight => mods.0 = true,
            K::ControlLeft | K::ControlRight => mods.1 = true,
            _ => {}
        }
        if e.repeat {
            continue;
        }
        let (shift, ctrl) = *mods;
        let key = match (&e.logical_key, e.key_code) {
            (Key::Character(c), _) => {
                EditKey::Text(c.chars().filter(|ch| !ch.is_control()).collect())
            }
            (Key::Space, _) => EditKey::Text(" ".into()),
            (Key::Enter, _) => {
                typing = false;
                continue;
            }
            // Esc leaves the box through the shared dispatcher (`ui::esc_dispatcher`).
            _ => match super::widgets::edit_key(&e.logical_key, e.key_code, ctrl) {
                Some(k) => k,
                None => continue,
            },
        };
        if let Some(c) = edit.press(&mut text, key, shift, &clipboard.0, &fits) {
            clipboard.0 = c;
        }
    }
    if text != view.search
        || !typing
        || edit.caret != view.edit.caret
        || edit.selection() != view.edit.selection()
    {
        view.search = text;
        view.edit = edit;
        view.typing = typing;
        view.version += 1;
    } else {
        view.edit = edit;
    }
    // The game's keys see nothing while the box has the keyboard (Escape stays for the dispatcher).
    super::keys::reset_but_escape(&mut keys);
}
