//! The factions window (`hudFactionsWindow` = `records/ui/factions/factions_mastertable.dbr`,
//! `UIPlayerHud.factionWindow`): the
//! player's reputation with every faction they know, one row each in a scrolled list, and the selected faction's
//! standing, large bar, description and reputation sources. It reads `HudValues::factions` (the player's
//! `PlayerFactions`) and only keeps UI state ([`FactionsView`]); the maths is `gd_rules::faction`.
//!
//! `Grim Dawn.exe` (ctor `FactionWindow::FactionWindow`, loader `FactionWindow::Load`, update
//! `FactionWindow::UpdateBars`, draw `FactionWindow::Render`, mouse `FactionWindow::HandleMouse`):
//! - the rows are the factions 1..46 sorted by their localised names (`GetFactionTag`, ordinal UTF-16 order; two
//!   equal names keep the last), shown when the player's pack has the faction modified (the save's flag) or it is
//!   a `startingFactions` one, and it is not in `hiddenFactions`. There are no friendly / hostile sections: a
//!   hostile faction's row shows the negative bar. The selection starts at the first row;
//! - a row (`factionListItem` with the faction's `factionIcon` as `factionSmallSymbol`, `factionNameText`, the
//!   `factionPositiveBooster` / `factionNegativeBooster` marks while the pack has a reputation / infamy boost, and
//!   one of `factionPositiveBar` / `factionNeutralBar` / `factionNegativeBar`, [`gd_rules::faction::row_bar`],
//!   labelled with the level name, `GetFactionLevelName`); row placement: the cursor starts at `s`, the bar sits
//!   at its record y below the name box (+ 2 s), the next row starts 2 s below the row's content (item, name
//!   or bar, whichever is lowest). A row bar under the
//!   cursor, while not full, shows `value − min / max − min` instead of the name (bar text mode 1,
//!   `UIBar::GetDisplayNumbers`). The bars draw their empty bitmap, then the full one stretched into the filled width
//!   (`UIBar::Render`);
//! - the selected row: `factionTitleText` = `tagFactionWindowFactionTitle` (name - level) with the level name in
//!   red (negative), green (a positive level) or yellow (the level from 0); the pack's `factionLargeIcon` as
//!   `factionSymbol`; `factionLargePositiveBar` / `factionLargeNegativeBar` with its overlay
//!   ([`gd_rules::faction::large_bar`], numbers always); the faction's `Info` tag wrapped at `factionInfoBoxWidth`
//!   in `factionInfoBox`; "Reputation Gained From:" (`factionBountyBoxTitle`, its bitmap) and the sources in
//!   `factionBountyBox` while the faction can gain reputation (not `noRepGainFactions`, and not negative in a
//!   `noRepGainIfNegativeFactions` one): `tagFactionReputationKilling` per faction that gives it a kill bonus
//!   ([`gd_rules::faction::enemies`]), then Quests / Bounties when the pack enables them;
//! - the selection and the hovered row get `crafting_recipeselectionhighlight.tex` over the list item (1 px
//!   larger each side) in (0.68, 0.58, 0.40), the hovered one at alpha 0.75, clipped to the list; a left press on a
//!   row selects it with `factionSoundNameDown`; the close button closes the window;
//! - rollovers (`factionBarRollover`, a simple rollover in its `defaultTextStyle`, placement mode 3: centred above
//!   the cursor): over a row's booster mark `tagFactionBonusRep` / `tagFactionBonusInfamy` with the boost's percent;
//!   over the large bar or one of its tier marks (`positiveTier1..4` / `negativeTier1..3`: never drawn, hit areas
//!   only; the overlay bitmap shows the ticks) the level ([`gd_rules::faction::tier_target`]): its name, or "Required
//!   to <level>: n" ([`gd_rules::faction::remaining`]), a blank line and "Unlocked at <level>:", the market discount
//!   (for a non-negative value) and the level's `factionRewards` text wrapped at 400 px.
//! - opening (HUD listener `UIPlayerHud::OnToolbarButton`, key handler `InGameUI::HandleKeyAction`): the classic HUD's
//!   faction button (`hudFactionButton`, a toggle, drawn with its down bitmap while the window is open:
//!   `UIPlayerHud::OnWindowEvent`) and the key action 52 "Factions Window" (`tagKeybind47`, default J) close the other
//!   windows and show it; the button again closes it, the key only while the factions window is the open one (in UI
//!   mode it never opens); opening another window closes it.
//!
//! PORT: the info text sits at the info scroll window's origin (its widget has no record); the name box height is
//! its record height; the 400 px reward wrap is UI-scaled; the scroll windows are the journal's
//! (`window::pane` / `scrollbar_live`). The faction name and the bar texts truncate to their box / bar, and the
//! row pitch follows the content (name + bar) rather than the item texture's height, so long localised labels and
//! swapped bitmaps flow instead of overflowing (UX over parity). Hooks (hooks.rs): `GD_UI_OPEN=factions`,
//! `GD_FACTION_SELECT=<row>`, `GD_FACTION_HOVER=bar|tier<n>|row<n>|rowbar<n>|booster<n>`.
use super::Ui;
use super::contains;
use super::window::Rebuild;
use crate::state::inventory::UiCapture;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::save::gdc::Faction;
use gd_formats::ui::{Rect as URect, TextAlign, TextBox, Widget, WindowDef};
use gd_game_core::data::GameData;
use gd_rules::faction::{BarKind, BarValue, Levels, Pack};

pub const RECORD: &str = "records/ui/factions/factions_mastertable.dbr";
const HIGHLIGHT: &str = "ui/inventor/crafting_recipeselectionhighlight.tex";
/// The selection highlight's colour (ctor `FactionWindow::FactionWindow`, `FactionWindow.highlightColor`).
const HIGHLIGHT_RGB: [f32; 3] = [0.68, 0.58, 0.40];
const TITLE_STYLE: &str = "records/ui/styles/text/style_rollover_title.dbr";
const DESC_STYLE: &str = "records/ui/styles/text/style_rollover_description.dbr";

pub struct FactionsUiPlugin;

impl Plugin for FactionsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FactionsView>()
            .init_resource::<FactionsLayout>()
            .init_resource::<super::window::WindowPolicy>()
            .add_systems(Update, exclusive.before(build).in_set(super::UiPlaySet))
            .add_systems(Update, toggle_key.in_set(super::UiInputSet));
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            test_hooks.before(exclusive).in_set(super::UiPlaySet),
        );
        super::window::add_window(
            app,
            (build, live).chain(),
            clicks,
            |view: Res<FactionsView>, layout: Res<FactionsLayout>| {
                view.open || layout.window.is_some()
            },
        );
    }
}

/// UI-only state: open, the selected row (`FactionWindow.selectedRow`: none = the first), the list's and the info box's
/// scroll.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct FactionsView {
    pub open: bool,
    pub selected: Option<usize>,
    pub scroll: [f32; 2],
}

/// A hit area of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Close,
    /// A list row (its list item).
    Row(usize),
    RowBar(usize),
    /// A row's booster mark (`true`: the positive one).
    Booster(usize, bool),
    LargeBar,
    /// A tier mark of the large bar, by its hover index (1..4).
    Tier(u8),
}

/// A scroll window of the built window (screen px).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Pane {
    pub clip: URect,
    pub max: f32,
}

/// The built window (physical px). Empty while it is closed.
#[derive(Resource, Default)]
pub struct FactionsLayout {
    pub window: Option<URect>,
    /// Close button, tier marks, large bar (in hit-test order).
    parts: Vec<(URect, Part)>,
    /// List parts at scroll 0 (boosters before the bar before the row).
    rows: Vec<(URect, Part)>,
    /// The list (0) and the info box (1).
    pub panes: [Option<Pane>; 2],
    bars: [Option<super::window::Bar>; 2],
    /// Rollover lines (style, text, wrapped) per part.
    tips: Vec<(Part, Vec<(String, String, bool)>)>,
    /// The row bars whose text switches to numbers under the cursor (not full).
    numbered: Vec<usize>,
    version: u64,
}

impl FactionsLayout {
    /// The part under `p` (screen px), the list scrolled by `scroll`.
    pub fn part_at(&self, p: Vec2, scroll: f32) -> Option<Part> {
        if let Some((_, part)) = self.parts.iter().find(|(r, _)| contains(r, p)) {
            return Some(*part);
        }
        let list = self.panes[0]?;
        if !contains(&list.clip, p) {
            return None;
        }
        let q = Vec2::new(p.x, p.y + scroll);
        self.rows
            .iter()
            .find(|(r, _)| contains(r, q))
            .map(|(_, part)| *part)
    }

    /// The centre of a named part (`GD_FACTION_HOVER`), screen px at `scroll`.
    fn named(&self, name: &str, scroll: f32) -> Option<(Part, Vec2)> {
        let n = |k: &str| name.strip_prefix(k).and_then(|v| v.parse::<usize>().ok());
        let want = match name {
            "bar" => Part::LargeBar,
            _ if n("tier").is_some() => Part::Tier(n("tier")? as u8),
            _ if n("rowbar").is_some() => Part::RowBar(n("rowbar")?),
            _ if n("booster").is_some() => {
                let i = n("booster")?;
                let pos = self.rows.iter().any(|(_, p)| *p == Part::Booster(i, true));
                Part::Booster(i, pos)
            }
            _ => Part::Row(n("row")?),
        };
        let centre = |r: &URect| Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5);
        self.parts
            .iter()
            .find(|(_, p)| *p == want)
            .map(|(r, _)| centre(r))
            .or_else(|| {
                self.rows
                    .iter()
                    .find(|(_, p)| *p == want)
                    .map(|(r, _)| centre(r) - Vec2::new(0.0, scroll))
            })
            .map(|c| (want, c))
    }

    /// The rollover lines of `part`.
    pub fn tip(&self, part: Part) -> Option<&Vec<(String, String, bool)>> {
        self.tips.iter().find(|(p, _)| *p == part).map(|(_, l)| l)
    }
}

/// The hovered part and the pointer (`GD_FACTION_HOVER`, else `GD_HOVER` / the cursor), while the window is built.
pub fn hovered(
    layout: &FactionsLayout,
    view: &FactionsView,
    hooks: &gd_game_core::hooks::Hooks,
    win: &Window,
) -> Option<(Part, Vec2)> {
    layout.window?;
    if let Some(name) = hooks.ui.faction_hover.as_deref() {
        return layout.named(name, view.scroll[0]);
    }
    let p = super::cursor_px(hooks, win)?;
    layout.part_at(p, view.scroll[0]).map(|part| (part, p))
}

#[derive(Component)]
struct FactionsRoot;

#[derive(Component)]
struct BarsRoot;

/// The scrolled child of a scroll window ([`super::window::pane`]): `pane` 0 the list, 1 the info box.
#[derive(Component)]
struct Scrolled {
    pane: usize,
    clip_y: f32,
    inv: f32,
}

/// The hovered row's highlight (moved by [`live`]).
#[derive(Component)]
struct HoverMark;

/// A row bar's text: the label, or the numbers shown under the cursor.
#[derive(Component)]
struct BarText {
    row: usize,
    numbers: bool,
}

/// The close button's `bitmapNameInFocus`.
#[derive(Component)]
struct CloseFocus;

/// The rows in the engine's order: (faction index, localised name), sorted by UTF-16 code units (later equal names
/// replace earlier ones), shown ones only.
fn rows(
    fonts: &super::text::FontCache,
    list: &[Faction],
    gf: &gd_formats::arz::Record,
) -> Vec<(usize, String)> {
    let names = |k: &str| -> Vec<usize> {
        gf.strs(k)
            .iter()
            .flat_map(|x| x.split(';'))
            .filter_map(gd_rules::faction::type_of)
            .collect()
    };
    let (starting, hidden) = (names("startingFactions"), names("hiddenFactions"));
    let mut map: std::collections::BTreeMap<Vec<u16>, (usize, String)> = Default::default();
    for i in 1..47 {
        let Some(tag) = gd_rules::faction::tag(i) else {
            continue;
        };
        let name = fonts.tag(&tag).to_owned();
        map.insert(name.encode_utf16().collect(), (i, name));
    }
    map.into_values()
        .filter(|(i, _)| {
            let modified = list.get(*i).is_some_and(|f| f.modified != 0);
            (modified || starting.contains(i)) && !hidden.contains(i)
        })
        .collect()
}

/// The faction packs by type (`gamefactions.dbr`'s keys; an empty key has none).
fn packs(data: &GameData, gf: &gd_formats::arz::Record) -> Vec<Pack> {
    gd_rules::faction::KEYS
        .iter()
        .map(|k| {
            gf.str(k)
                .and_then(|p| data.db.get(p))
                .map(Pack::from_record)
                .unwrap_or_default()
        })
        .collect()
}

/// A tag formatted with its arguments, keeping colour codes and `^n` (split by the callers).
fn fmt(ui: &Ui, tag: &str, args: &[gd_sim::items::tooltip::Arg]) -> String {
    gd_sim::items::tooltip::format_rich(ui.fonts.tag(tag), args)
}

/// `text` with `code` before the first occurrence of `name` (the engine's `find` + `insert`).
fn colour_name(text: &str, name: &str, code: &str) -> String {
    match text.find(name).filter(|_| !name.is_empty()) {
        Some(i) => format!("{}{code}{}", &text[..i], &text[i..]),
        None => text.to_owned(),
    }
}

fn paragraphs(text: &str) -> String {
    text.replace("{^n}", "\n")
        .replace("^n", "\n")
        .replace("^N", "\n")
}

fn num(r: &gd_formats::arz::Record, k: &str) -> f32 {
    r.f32(k).unwrap_or(0.0)
}

/// A scroll window record's rect and its clip (inside the guard bands), screen px.
fn pane_rects(ui: &Ui, sw: &gd_formats::arz::Record, o: Vec2) -> (URect, URect) {
    let (top, bottom) = (num(sw, "topGuardBand"), num(sw, "bottomGuardBand"));
    let window = URect::new(
        o.x + ui.sc(num(sw, "positionX")),
        o.y + ui.sc(num(sw, "positionY")),
        ui.sc(num(sw, "width")),
        ui.sc(num(sw, "height")),
    );
    let clip = URect::new(
        window.x,
        window.y + ui.sc(top),
        window.w,
        ui.sc(num(sw, "height") - top - bottom),
    );
    (window, clip)
}

/// A `BarGraph` (`bargraph.tpl`) at `at` (px): the empty bitmap, the full one stretched into the filled width
/// (`UIBar::Render`); returns its rect.
fn draw_bar(ui: &mut Ui, bar: &gd_formats::ui::BarGraph, at: Vec2, frac: f32) -> Option<URect> {
    let empty = bar.empty.as_deref().and_then(|p| ui.image(p));
    let full = bar.full.as_deref().and_then(|p| ui.image(p));
    let size = empty.as_ref().or(full.as_ref())?.1;
    let r = URect::new(at.x, at.y, ui.sc(size.x as f32), ui.sc(size.y as f32));
    if let Some((img, _)) = empty {
        ui.quad_stretch(img, r, Color::WHITE);
    }
    let w = r.w * frac;
    if let Some((img, _)) = full
        && w > 0.0
    {
        ui.quad_stretch(img, URect::new(r.x, r.y, w, r.h), Color::WHITE);
    }
    Some(r)
}

/// A bar's centred text (`UIBar::GetDisplayNumbers`) in its `FontStyle`.
fn bar_text(ui: &mut Ui, bar: &gd_formats::ui::BarGraph, r: URect, text: &str) -> Option<Entity> {
    let style = bar.font_style.clone()?;
    // PORT: stock centres the text in the fixed bar and lets a long standing name spill; cut it to the bar.
    let text = ui.truncate_line(&style, r.w, text);
    let tb = TextBox {
        rect: URect::new(0.0, 0.0, r.w / ui.s, r.h / ui.s),
        align_x: TextAlign::Center,
        align_y: TextAlign::Center,
        ..Default::default()
    };
    ui.text_with_style(&tb, &style, Vec2::new(r.x, r.y), &text)
}

fn numbers(v: &BarValue) -> String {
    let (a, b) = v.numbers();
    format!("{a}/{b}")
}

/// The large bar's rollover for hover index `h` (module doc; `FactionWindow::UpdateBars`).
fn standing_lines(
    ui: &mut Ui,
    levels: &Levels,
    pack: Option<&Pack>,
    v: i32,
    h: u8,
    default_style: &str,
) -> Vec<(String, String, bool)> {
    use gd_sim::items::tooltip::Arg;
    let Some(target) = gd_rules::faction::tier_target(levels, v, h) else {
        return Vec::new();
    };
    let tag = if h == 0 {
        levels.tag(v as f32)
    } else {
        levels.tag(levels.value_of(target))
    };
    if tag.is_empty() {
        return Vec::new();
    }
    let name = ui.fonts.tag(&tag).to_owned();
    let mut colour = gd_rules::faction::standing_colour(levels, v);
    let mut out = Vec::new();
    let (first, rewards_tier) = if h != 0 {
        let left = gd_rules::faction::remaining(levels, v, target);
        if v >= 0 {
            colour = "^g";
        }
        (
            fmt(
                ui,
                "tagFactionProgress",
                &[Arg::Str(&name), Arg::Num(left as f32)],
            ),
            fmt(ui, "tagFactionRewardsTier", &[Arg::Str(&name)]),
        )
    } else {
        (name.clone(), String::new())
    };
    out.push((
        TITLE_STYLE.to_owned(),
        colour_name(&first, &name, colour),
        false,
    ));
    if h != 0 {
        out.push((default_style.to_owned(), " ".to_owned(), false));
    }
    if !rewards_tier.is_empty() {
        out.push((DESC_STYLE.to_owned(), rewards_tier, false));
    }
    let (_, hi) = levels.bounds(v as f32);
    let at = if hi > 0 && h != 0 {
        levels.value_of(target)
    } else {
        v as f32
    };
    let d = (levels.discount_mult(at) - 1.0) * 100.0;
    if v >= 0 {
        let t = if d > 0.0 {
            "tagFactionDiscountPositive"
        } else if d < 0.0 {
            "tagFactionDiscountNegative"
        } else {
            "tagFactionDiscountZero"
        };
        out.push((
            default_style.to_owned(),
            fmt(ui, t, &[Arg::Num(d.abs())]),
            false,
        ));
    }
    if target != 3
        && let Some(reward) = pack.and_then(|p| p.reward(target))
    {
        let text = paragraphs(ui.fonts.tag(reward));
        let w = 400.0 * ui.s;
        for l in ui.wrap_lines(default_style, w, &text) {
            out.push((default_style.to_owned(), l, false));
        }
    }
    out
}

/// `GD_UI_OPEN` / `GD_FACTION_SELECT` (hooks.rs): the window once the player exists, on that row.
#[cfg(feature = "dev")]
fn test_hooks(
    hooks: Res<gd_game_core::hooks::Hooks>,
    players: Query<(), With<gd_game_core::player::Player>>,
    mut view: ResMut<FactionsView>,
    mut done: Local<bool>,
    mut fresh: gd_game_core::session::NewSession,
) {
    if fresh.begun() {
        *done = false;
    }
    if *done || players.is_empty() {
        return;
    }
    *done = true;
    if hooks.ui.open.as_deref() == Some("factions") {
        view.open = true;
        view.selected = hooks.ui.faction_select;
    }
}

/// The factions window key (action 52; key handler `InGameUI::HandleKeyAction` case 52): in game mode (no window open)
/// it opens the window; in UI mode (some window open) it only closes the factions window, when that is the one
/// open, and does nothing while another window has the focus.
fn toggle_key(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<super::keys::KeyBindings>,
    mut sets: ParamSet<(super::window::Windows, super::gamemenu::OpenWindows)>,
) {
    if bindings.just_pressed(super::keys::FACTIONS, &keys)
        && (!sets.p1().any() || sets.p0().is_open(super::window::Window::Factions))
    {
        sets.p0().toggle(super::window::Window::Factions);
    }
}

/// The window's open/close watcher: opening it applies the one policy ([`super::window::WindowPolicy`],
/// `UIPlayerHud::CloseWindows` from the button and the key); a window the policy excludes opening closes it
/// (their openers call `UIPlayerHud::ClosePanels`, which hides `UIPlayerHud.factionWindow`).
fn exclusive(
    mut windows: super::window::Windows,
    policy: Res<super::window::WindowPolicy>,
    mut was: Local<[bool; super::window::Window::COUNT]>,
    mut fresh: gd_game_core::session::NewSession,
) {
    if fresh.begun() {
        *was = Default::default();
    }
    let w = super::window::Window::Factions;
    let now = windows.snapshot();
    if windows.factions.open && !was[w.index()] {
        windows.open(w);
    } else if windows.factions.open && policy.opened_closer(w, &was, &now) {
        windows.factions.open = false;
    }
    *was = windows.snapshot();
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    values: Res<super::HudValues>,
    mut view: ResMut<FactionsView>,
    roots: Query<Entity, With<FactionsRoot>>,
    bar_roots: Query<Entity, With<BarsRoot>>,
    mut layout: ResMut<FactionsLayout>,
    mut fresh: gd_game_core::session::NewSession,
    // The built signature, compared with the live values by reference (the faction list is copied on a rebuild only).
    mut built: Local<Option<(UVec2, bool, Option<usize>, Vec<Faction>, u64)>>,
    mut built_bars: Local<Rebuild<u64>>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        *built = None;
        built_bars.reset();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 {
        return;
    }
    let generation = assets.fonts.generation;
    let same = built.as_ref().is_some_and(|b| {
        b.0 == size
            && b.1 == view.open
            && b.2 == view.selected
            && b.3 == values.factions
            && b.4 == generation
    });
    if !same {
        *built = Some((
            size,
            view.open,
            view.selected,
            values.factions.clone(),
            generation,
        ));
        super::window::despawn_all(&mut commands, &roots);
        let version = layout.version + 1;
        *layout = FactionsLayout {
            version,
            ..Default::default()
        };
        if view.open {
            let s = assets.scale(size);
            let mut ui = assets.ui(
                &mut commands,
                &data,
                Entity::PLACEHOLDER,
                s,
                1.0 / win.scale_factor(),
                size.as_vec2(),
            );
            ui.root = super::spawn_root(ui.commands, FactionsRoot, super::window::Z_PANEL);
            draw(&mut ui, &values.factions, view.selected, &mut layout);
        }
    }
    // The scroll bars follow the scroll (the panes' content moves in place, [`live`]).
    for (i, p) in layout.panes.iter().enumerate() {
        if let Some(p) = p {
            view.scroll[i] = view.scroll[i].clamp(0.0, p.max);
        }
    }
    // The scroll is not keyed: `live` moves the thumbs in place.
    let bar_sig = layout.version;
    if !built_bars.changed(bar_sig) {
        return;
    }
    super::window::despawn_all(&mut commands, &bar_roots);
    layout.bars = [None, None];
    let Some(w) = layout.window else { return };
    let Some(rec) = data.db.get(RECORD) else {
        return;
    };
    let s = assets.scale(size);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        Entity::PLACEHOLDER,
        s,
        1.0 / win.scale_factor(),
        size.as_vec2(),
    );
    ui.root = super::spawn_root(ui.commands, BarsRoot, super::window::Z_PANEL + 1);
    let o = Vec2::new(w.x, w.y);
    for (i, field) in ["factionScrollWindow", "factionInfoBox"].iter().enumerate() {
        let (Some(sw), Some(p)) = (rec.str(field).and_then(|r| ui.record(r)), layout.panes[i])
        else {
            continue;
        };
        let (window, clip) = pane_rects(&ui, &sw, o);
        let content = p.max + clip.h;
        if p.max > 0.0 || sw.bool("alwaysShowScroll") {
            let len = ui.sc(num(&sw, "height") + num(&sw, "scrollbarSizeAdjust"));
            let top = window.y + ui.sc(num(&sw, "scrollbarPositionAdjust"));
            layout.bars[i] = super::window::scrollbar_live(
                &mut ui,
                sw.str("verticalScrollbar").unwrap_or(""),
                clip.x + clip.w,
                top,
                len,
                (clip.h, content, view.scroll[i]),
                super::window::ScrollOwner::Factions,
                i,
            );
        }
    }
}

/// Draws the open window; the selected row is clamped to the rows (none: the first, `FactionWindow::UpdateBars`).
fn draw(ui: &mut Ui, factions: &[Faction], selected: Option<usize>, layout: &mut FactionsLayout) {
    use gd_sim::items::tooltip::Arg;
    let (Some(rec), Some(gf)) = (
        ui.record(RECORD),
        ui.record(gd_rules::faction::GAME_FACTIONS),
    ) else {
        return;
    };
    let levels = Levels::from_record(&gf);
    let packs = packs(ui.data, &gf);
    let rows = rows(ui.fonts, factions, &gf);
    let field = |k: &str| rec.str(k).unwrap_or("").to_owned();
    let r = WindowDef::from_record(&rec).screen_rect(ui.screen.x, ui.screen.y, ui.s);
    let o = Vec2::new(r.x, r.y);
    layout.window = Some(r);
    let sel = selected.unwrap_or(0).min(rows.len().saturating_sub(1));
    let sel = (!rows.is_empty()).then_some(sel);
    let value = |i: usize| factions.get(i).map_or(0.0, |f| f.value) as i32;
    let bitmap =
        |ui: &mut Ui, f: &str, at: Vec2, image: Option<&str>| bitmap_icon(ui, &field(f), at, image);
    let text_rec = |ui: &mut Ui, f: &str| ui.record(&field(f)).map(|t| TextBox::from_record(&t));
    // The selected faction's sources (module doc).
    let detail = sel.map(|row| rows[row].0);
    let sources: Vec<String> = detail
        .map(|i| {
            let v = value(i);
            let no_gain = |k: &str| {
                gf.strs(k)
                    .iter()
                    .flat_map(|x| x.split(';'))
                    .filter_map(gd_rules::faction::type_of)
                    .any(|t| t == i)
            };
            if no_gain("noRepGainFactions") || (v < 0 && no_gain("noRepGainIfNegativeFactions")) {
                return Vec::new();
            }
            let mut out: Vec<String> = gd_rules::faction::enemies(&packs, i)
                .iter()
                .map(|t| {
                    let n = ui.fonts.tag(t).to_owned();
                    fmt(ui, "tagFactionReputationKilling", &[Arg::Str(&n)])
                })
                .collect();
            if packs.get(i).is_some_and(|p| p.quest) {
                out.push(fmt(ui, "tagFactionReputationQuests", &[]));
            }
            if packs.get(i).is_some_and(|p| p.bounty) {
                out.push(fmt(ui, "tagFactionReputationBounties", &[]));
            }
            paragraphs(&out.concat())
                .split('\n')
                .filter(|l| !l.trim().is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    // Draw order: `FactionWindow::Render`.
    bitmap(ui, "factionBitmap", o, None);
    if !sources.is_empty() {
        bitmap(ui, "factionBountyBoxBitmap", o, None);
    }
    if let Some(tb) = text_rec(ui, "factionText") {
        let t = tb
            .tag
            .as_deref()
            .map(|t| ui.fonts.tag(t).to_owned())
            .unwrap_or_default();
        ui.text(&tb, o, &t);
    }
    let default_style = rec
        .str("factionBarRollover")
        .and_then(|r| ui.record(r))
        .and_then(|r| r.str("defaultTextStyle").map(str::to_owned))
        .unwrap_or_else(|| DESC_STYLE.to_owned());
    if let Some(i) = detail {
        let v = value(i);
        let level_tag = levels.tag(v as f32);
        let level_name = ui.fonts.tag(&level_tag).to_owned();
        let name = ui
            .fonts
            .tag(&gd_rules::faction::tag(i).unwrap_or_default())
            .to_owned();
        if let Some(tb) = text_rec(ui, "factionTitleText") {
            let title = fmt(
                ui,
                "tagFactionWindowFactionTitle",
                &[Arg::Str(&name), Arg::Str(&level_name)],
            );
            let code = gd_rules::faction::standing_colour(&levels, v);
            ui.text(&tb, o, &colour_name(&title, &level_name, code));
        }
        let large = packs.get(i).map(|p| p.large_icon.clone());
        bitmap(ui, "factionSymbol", o, large.as_deref());
        if !sources.is_empty() {
            if let Some(tb) = text_rec(ui, "factionBountyBoxTitle") {
                let t = tb
                    .tag
                    .as_deref()
                    .map(|t| ui.fonts.tag(t).to_owned())
                    .unwrap_or_default();
                ui.text(&tb, o, &t);
            }
            if let Some(tb) = text_rec(ui, "factionBountyBox")
                && let Some(style) = tb.style.clone()
            {
                let lh = ui.line_height(&style);
                for (n, l) in sources.iter().enumerate() {
                    let mut line = tb.clone();
                    line.rect.y += n as f32 * lh / ui.s;
                    line.rect.h = lh / ui.s;
                    ui.text(&line, o, l);
                }
            }
        }
        // The large bar and its overlay, the tier marks' hit areas (tested first, `FactionWindow::HandleMouse`).
        let big = gd_rules::faction::large_bar(&levels, v);
        let (bar_f, overlay_f, tiers): (&str, &str, &[(&str, u8)]) =
            if big.kind == BarKind::Negative {
                (
                    "factionLargeNegativeBar",
                    "factionNegativeBarOverlay",
                    &[
                        ("negativeTier1", 1),
                        ("negativeTier2", 3),
                        ("negativeTier3", 4),
                    ],
                )
            } else {
                (
                    "factionLargePositiveBar",
                    "factionPositiveBarOverlay",
                    &[
                        ("positiveTier1", 1),
                        ("positiveTier2", 2),
                        ("positiveTier3", 3),
                        ("positiveTier4", 4),
                    ],
                )
            };
        let pack = packs.get(i).cloned();
        for &(f, h) in tiers {
            if let Some(Widget::Bitmap { bitmap, x, y }) = ui.widget(&field(f))
                && let Some((_, sz)) = ui.image(&bitmap)
            {
                let tr = URect::new(
                    o.x + ui.sc(x),
                    o.y + ui.sc(y),
                    ui.sc(sz.x as f32),
                    ui.sc(sz.y as f32),
                );
                layout.parts.push((tr, Part::Tier(h)));
                let lines = standing_lines(ui, &levels, pack.as_ref(), v, h, &default_style);
                layout.tips.push((Part::Tier(h), lines));
            }
        }
        if let Some(Widget::BarGraph(bar)) = ui.widget(&field(bar_f)) {
            let at = Vec2::new(o.x + ui.sc(bar.x), o.y + ui.sc(bar.y));
            if let Some(br) = draw_bar(ui, &bar, at, big.frac()) {
                bar_text(ui, &bar, br, &numbers(&big));
                layout.parts.push((br, Part::LargeBar));
                let lines = standing_lines(ui, &levels, pack.as_ref(), v, 0, &default_style);
                layout.tips.push((Part::LargeBar, lines));
            }
        }
        bitmap(ui, overlay_f, o, None);
    }
    // The list (loader `FactionWindow::Load` row layout, module doc).
    let outer = ui.root;
    if let Some(sw) = ui.record(&field("factionScrollWindow")) {
        let (_, clip) = pane_rects(ui, &sw, o);
        let inner = super::window::pane(ui, clip, 0.0);
        ui.commands.entity(inner).insert(Scrolled {
            pane: 0,
            clip_y: clip.y,
            inv: ui.inv,
        });
        ui.root = inner;
        let item = ui.widget(&field("factionListItem"));
        let name_tb = text_rec(ui, "factionNameText");
        let bar_w = |k: &str| match ui.widget(&field(k)) {
            Some(Widget::BarGraph(b)) => Some(b),
            _ => None,
        };
        let (pos_bar, neu_bar, neg_bar) = (
            bar_w("factionPositiveBar"),
            bar_w("factionNeutralBar"),
            bar_w("factionNegativeBar"),
        );
        let s = ui.s;
        let mut c = s;
        for (row, (i, name)) in rows.iter().enumerate() {
            let f = factions.get(*i);
            let at = Vec2::new(clip.x, clip.y + c);
            let Some(Widget::Bitmap {
                bitmap: item_bitmap,
                x,
                y,
            }) = item.clone()
            else {
                break;
            };
            let Some((_, ir)) = ui.bitmap(&item_bitmap, x, y, at) else {
                break;
            };
            layout.rows.push((ir, Part::Row(row)));
            bitmap_icon(
                ui,
                &field("factionSmallSymbol"),
                at,
                packs.get(*i).map(|p| p.icon.as_str()),
            );
            for (k, positive, boost) in [
                (
                    "factionPositiveBooster",
                    true,
                    f.map_or(0.0, |f| f.positive_boost),
                ),
                (
                    "factionNegativeBooster",
                    false,
                    f.map_or(0.0, |f| f.negative_boost),
                ),
            ] {
                if boost == 0.0 {
                    continue;
                }
                let Some(br) = bitmap(ui, k, at, None) else {
                    continue;
                };
                layout.rows.insert(0, (br, Part::Booster(row, positive)));
                let pct = ((boost - 1.0) * 100.0) as i32;
                if pct > 0 {
                    let tag = if positive {
                        "tagFactionBonusRep"
                    } else {
                        "tagFactionBonusInfamy"
                    };
                    let t = fmt(ui, tag, &[Arg::Num(pct as f32)]);
                    layout.tips.push((
                        Part::Booster(row, positive),
                        vec![(TITLE_STYLE.to_owned(), t, false)],
                    ));
                }
            }
            let (name_y, name_h) = match &name_tb {
                Some(tb) => {
                    // PORT: the name is one line cut to its box (stock lets a localised name spill).
                    let shown = match tb.style.as_deref() {
                        Some(st) => ui.truncate_line(st, ui.sc(tb.rect.w), name),
                        None => name.clone(),
                    };
                    ui.text(tb, at, &shown);
                    (at.y + ui.sc(tb.rect.y), ui.sc(tb.rect.h))
                }
                None => (at.y, 0.0),
            };
            // Row pitch follows the content (name + bar), not the item texture's height, so a swapped
            // bitmap never clips the bar below it (PORT: stock pitches by the texture).
            let mut row_bottom = (ir.y + ir.h).max(name_y + name_h);
            let below = name_y + name_h + 2.0 * s;
            let v = value(*i);
            let bv = gd_rules::faction::row_bar(&levels, v);
            let bar = match bv.kind {
                BarKind::Positive => pos_bar.as_ref(),
                BarKind::Neutral => neu_bar.as_ref(),
                BarKind::Negative => neg_bar.as_ref(),
            };
            if let Some(bar) = bar {
                let bat = Vec2::new(at.x + ui.sc(bar.x), (below + ui.sc(bar.y)).round());
                if let Some(br) = draw_bar(ui, bar, bat, bv.frac()) {
                    row_bottom = row_bottom.max(br.y + br.h);
                    let label = ui.fonts.tag(&levels.tag(v as f32)).to_owned();
                    if let Some(e) = bar_text(ui, bar, br, &label) {
                        ui.commands.entity(e).insert(BarText {
                            row,
                            numbers: false,
                        });
                    }
                    if bv.frac() != 1.0 {
                        if let Some(e) = bar_text(ui, bar, br, &numbers(&bv)) {
                            ui.commands
                                .entity(e)
                                .insert((BarText { row, numbers: true }, Visibility::Hidden));
                        }
                        layout.numbered.push(row);
                    }
                    let at_bar = layout
                        .rows
                        .iter()
                        .position(|(_, p)| matches!(p, Part::Row(_)));
                    layout
                        .rows
                        .insert(at_bar.unwrap_or(0), (br, Part::RowBar(row)));
                }
            }
            c = row_bottom - clip.y + 2.0 * s;
        }
        // The selection's and the hovered row's highlights (`FactionWindow::Render`).
        if let Some((tex, _)) = ui.image(HIGHLIGHT) {
            let [r, g, b] = HIGHLIGHT_RGB;
            if let Some(ir) = sel.and_then(|row| {
                layout
                    .rows
                    .iter()
                    .find(|(_, p)| *p == Part::Row(row))
                    .map(|(r, _)| *r)
            }) {
                let hr = URect::new(ir.x - 1.0, ir.y - 1.0, ir.w + 2.0, ir.h + 2.0);
                ui.quad_stretch(tex.clone(), hr, Color::srgba(r, g, b, 1.0));
            }
            let e = ui.quad_stretch(
                tex,
                URect::new(0.0, 0.0, 0.0, 0.0),
                Color::srgba(r, g, b, 0.75),
            );
            ui.commands
                .entity(e)
                .insert((HoverMark, Visibility::Hidden));
        }
        layout.panes[0] = Some(Pane {
            clip,
            max: (c - clip.h).max(0.0),
        });
        ui.root = outer;
    }
    // The info box: the faction's `Info` text wrapped at `factionInfoBoxWidth`.
    if let (Some(sw), Some(i)) = (ui.record(&field("factionInfoBox")), detail) {
        let (_, clip) = pane_rects(ui, &sw, o);
        let inner = super::window::pane(ui, clip, 0.0);
        ui.commands.entity(inner).insert(Scrolled {
            pane: 1,
            clip_y: clip.y,
            inv: ui.inv,
        });
        ui.root = inner;
        let style = field("factionInfoBoxStyle");
        let w = ui.s * num(&rec, "factionInfoBoxWidth");
        let text = paragraphs(
            ui.fonts
                .tag(&gd_rules::faction::info_tag(i).unwrap_or_default()),
        );
        let lh = ui.line_height(&style);
        let mut y = clip.y;
        for l in ui.wrap_lines(&style, w, &text) {
            ui.text_line(&style, Vec2::new(clip.x, y), &l, 1.0);
            y += lh;
        }
        layout.panes[1] = Some(Pane {
            clip,
            max: (y - clip.y - clip.h).max(0.0),
        });
        ui.root = outer;
    }
    // The close button (its in-focus bitmap shown by [`live`]).
    if let Some(Widget::Button { up, over, x, y, .. }) = ui.widget(&field("factionCloseButton")) {
        if let Some((_, cr)) = up.as_deref().and_then(|u| ui.bitmap(u, x, y, o)) {
            layout.parts.insert(0, (cr, Part::Close));
        }
        if let Some((e, _)) = over.as_deref().and_then(|u| ui.bitmap(u, x, y, o)) {
            ui.commands
                .entity(e)
                .insert((CloseFocus, Visibility::Hidden));
        }
    }
}

/// A bitmap field drawn with `icon` in place of the record's own bitmap when it is set (the faction's
/// `factionIcon` / pack icon); its rect, `None` when the field is no bitmap.
fn bitmap_icon(ui: &mut Ui, rec: &str, at: Vec2, icon: Option<&str>) -> Option<URect> {
    let Widget::Bitmap { bitmap, x, y } = ui.widget(rec)? else {
        return None;
    };
    let path = icon.filter(|p| !p.is_empty()).unwrap_or(&bitmap);
    ui.bitmap(path, x, y, at).map(|(_, r)| r)
}

/// Per frame, without a rebuild: the panes' scroll, the hovered row's highlight, the hovered row bar's numbers and
/// the close button's focus bitmap.
#[allow(clippy::too_many_arguments)]
fn live(
    hooks: Res<gd_game_core::hooks::Hooks>,
    window: Query<&Window, With<PrimaryWindow>>,
    view: Res<FactionsView>,
    layout: Res<FactionsLayout>,
    mut panes: Query<
        (&Scrolled, &mut Node),
        (
            Without<HoverMark>,
            Without<BarText>,
            Without<super::window::ScrollThumb>,
        ),
    >,
    mut mark: Query<(&mut Node, &mut Visibility), (With<HoverMark>, Without<BarText>)>,
    mut texts: Query<(&BarText, &mut Visibility), (Without<HoverMark>, Without<CloseFocus>)>,
    mut focus: Query<&mut Visibility, (With<CloseFocus>, Without<HoverMark>, Without<BarText>)>,
    mut thumbs: Query<
        (&super::window::ScrollThumb, &mut Node),
        (Without<Scrolled>, Without<HoverMark>, Without<BarText>),
    >,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    for (sc, mut node) in &mut panes {
        let top = Val::Px((-sc.clip_y - view.scroll[sc.pane]) * sc.inv);
        if node.top != top {
            node.top = top;
        }
    }
    for (t, mut node) in &mut thumbs {
        if t.owner != super::window::ScrollOwner::Factions {
            continue;
        }
        let v = view.scroll[t.pane];
        let off = t.travel * (v / t.max).clamp(0.0, 1.0);
        let top = Val::Px(((t.base_y + off).round() + t.round_add) * t.inv);
        if node.top != top {
            node.top = top;
        }
    }
    let Ok(win) = window.single() else { return };
    let inv = 1.0 / win.scale_factor();
    // The game menu / Options holds the cursor: no hover under it.
    let hover = hovered(&layout, &view, &hooks, win)
        .filter(|_| !pause.menu)
        .map(|(p, _)| p);
    let row = match hover {
        Some(Part::Row(r) | Part::RowBar(r) | Part::Booster(r, _)) => Some(r),
        _ => None,
    };
    let item = row.and_then(|r| {
        layout
            .rows
            .iter()
            .find(|(_, p)| *p == Part::Row(r))
            .map(|(r, _)| *r)
    });
    for (mut node, mut vis) in &mut mark {
        match item {
            Some(ir) => {
                let (l, t) = (Val::Px((ir.x - 1.0) * inv), Val::Px((ir.y - 1.0) * inv));
                let (w, h) = (Val::Px((ir.w + 2.0) * inv), Val::Px((ir.h + 2.0) * inv));
                if node.left != l || node.top != t || node.width != w || node.height != h {
                    (node.left, node.top, node.width, node.height) = (l, t, w, h);
                }
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
    let numbered = match hover {
        Some(Part::RowBar(r)) if layout.numbered.contains(&r) => Some(r),
        _ => None,
    };
    for (t, mut vis) in &mut texts {
        let shown = (numbered == Some(t.row)) == t.numbers;
        vis.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for mut vis in &mut focus {
        vis.set_if_neq(if hover == Some(Part::Close) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Clicks, the wheel and the scroll bars (mouse `FactionWindow::HandleMouse`; the scroll windows as `potions.rs`).
#[allow(clippy::too_many_arguments)]
fn clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    scale: Res<gd_ui_kit::state::settings::UiScaleOption>,
    layout: Res<FactionsLayout>,
    mut view: ResMut<FactionsView>,
    mut capture: ResMut<UiCapture>,
    data: Res<GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    time: Res<Time<Real>>,
    mut held: Local<Option<(usize, super::window::BarHold)>>,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    let Some(wr) = layout.window.filter(|_| view.open) else {
        wheel.clear();
        *held = None;
        return;
    };
    if pause.menu {
        // Modal: the game menu / Options takes the wheel and presses.
        wheel.clear();
        *held = None;
        return;
    }
    // Esc closes the window through the shared dispatcher (`ui::esc_dispatcher`).
    let Ok(win) = window.single() else { return };
    let Some(p) = super::cursor_px(&hooks, win) else {
        return;
    };
    let max = |i: usize| layout.panes[i].map_or(0.0, |p| p.max);
    if !buttons.pressed(MouseButton::Left) {
        *held = None;
    }
    if let Some((i, hold)) = held.as_mut() {
        let v = super::window::bar_held(
            layout.bars[*i],
            hold,
            p.y,
            time.delta_secs() * 1000.0,
            view.scroll[*i],
            max(*i),
        );
        if v != view.scroll[*i] {
            view.scroll[*i] = v;
        }
        capture.0 = true;
        wheel.clear();
        return;
    }
    if !contains(&wr, p) {
        wheel.clear();
        return;
    }
    capture.0 = true;
    let s = scale.factor(win.physical_width(), win.physical_height());
    for w in wheel.read() {
        let Some(i) = (0..2).find(|&i| layout.panes[i].is_some_and(|pn| contains(&pn.clip, p)))
        else {
            continue;
        };
        let step = match w.unit {
            bevy::input::mouse::MouseScrollUnit::Line => (w.y * 120.0 * 0.2 * s).trunc(),
            bevy::input::mouse::MouseScrollUnit::Pixel => w.y,
        };
        let v = (view.scroll[i] - step).clamp(0.0, max(i));
        if v != view.scroll[i] {
            view.scroll[i] = v;
        }
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    for i in 0..2 {
        if let Some((v, hold)) = layout.bars[i].and_then(|b| b.press(p, s, view.scroll[i], max(i)))
        {
            view.scroll[i] = v;
            *held = Some((i, hold));
            return;
        }
    }
    let rec = data.db.get(RECORD);
    let play = |sound: &mut MessageWriter<gd_game_core::visuals::PlaySound>,
                pak: Option<String>| {
        if let Some(pak) = pak.filter(|p| !p.is_empty()) {
            sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
        }
    };
    match layout.part_at(p, view.scroll[0]) {
        Some(Part::Close) => {
            let pak = rec
                .as_ref()
                .and_then(|r| r.str("factionCloseButton"))
                .and_then(|b| data.db.get(b))
                .and_then(|b| b.str("soundNameDown").map(str::to_owned));
            play(&mut sound, pak);
            view.open = false;
        }
        Some(Part::Row(r) | Part::RowBar(r) | Part::Booster(r, _)) => {
            play(
                &mut sound,
                rec.as_ref()
                    .and_then(|m| m.str("factionSoundNameDown").map(str::to_owned)),
            );
            if view.selected != Some(r) {
                view.selected = Some(r);
                // The info box returns to the top on a new selection (`FactionWindow.selectedRow` !=
                // `FactionWindow.infoRow`).
                view.scroll[1] = 0.0;
            }
        }
        _ => {}
    }
}
