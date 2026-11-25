//! HUD button rollovers ("Rift Travel  [L]" and its description): the
//! `RolloverLine2` records the HUD master table names per button
//! (`hudTeleportButtonRollover`, `hudButtonWeaponSwapRollover`,
//! `hudCompassButtonRollover`).
//!
//! Grim Dawn.exe:
//! - the HUD's mouse handler (`UIPlayerHud::HandleMouseEvent`) picks the rollover of the
//!   button under the cursor (teleport and active-teleport buttons share
//!   `hudTeleportButtonRollover`) and keeps the cursor position
//!   (`UIPlayerHud::rolloverCursorPos`); `UIPlayerHud::UpdateRollover` hands it to the rollover window with
//!   that point as its anchor;
//! - `RolloverLine2` (load `RolloverLine2::Load` / `RolloverLine1::Load`, text `RolloverLine2::GetText`):
//!   line 1 = `Line1FormatTag` localised with `{%s0}` = `Line1Tag` and
//!   `{%t1}` = the name of the key bound to its action (`SetKeyAction`
//!   `RolloverLine2::SetKeyAction`: 33 teleport, 34 weapon swap, set in the HUD constructor
//!   `UIPlayerHud::UIPlayerHud`), in `Line1Style`; line 2 = `Line2Tag` in `Line2Style`;
//! - the window (`RolloverStyleUIHelp` from `rolloverStyle`, loader
//!   `RolloverStyleUIHelp::Load`): background colour + alpha, border bitmaps around the
//!   text area (its size grows by the corner bitmaps, `RolloverWindow::GetSize`);
//! - placement mode 0 (`RolloverWindow::Place`): the window's right edge at the anchor
//!   x, its top at the anchor y; moved right of the anchor when it would leave
//!   the left edge, and kept inside the screen vertically (y ≤ H − height),
//!   so a rollover at the bottom HUD sits bottom-aligned left of the cursor.
//!
//! - line 2 wraps at the width of its own first `minCharsBeforeWrapping`
//!   characters cut at a word ([`wrap_sample`], `RolloverLine::GetWrapSample`; layout
//!   `RolloverWindow::Layout`: the window is as wide as that or the widest unwrapped
//!   line); the window shows once it has been up the style's `delay` ms
//!   (`RolloverWindow::Update`; at once while game time is paused), counted in real time.
//! - while the game menu (the exit window, `gamemenu.rs`) is up, the HUD's
//!   mouse handler (`UIPlayerHud::HandleMouseEvent`) tests only `hudMenuOptionsButton` (its
//!   rollover, and a press toggles the menu) and returns: nothing else under
//!   it shows a rollover; with the options window or a confirmation up, not
//!   even that.
//!
//! - window widgets' rollovers: the character window's mouse handler (`CharacterWindow::HandleMouseEvent`; the
//!   Illusionist's `TransmuteWindow::HandleMouseEvent` alike) sets the window's rollover to `equipSwapButtonRollover`
//!   while the cursor is over the weapon-swap button; the window draws it (`UIWindow::DrawRollover`) in placement mode
//!   2 anchored at the cursor: the left edge at the cursor x, top at the cursor y, moved left (x = W − width) and up by
//!   its height when it would leave the right edge, then clamped like mode 0;
//! - `ButtonStatic` draw (`ButtonStatic::Render`): the down bitmap while the button is
//!   held down (`ButtonStatic.isDown`), else `bitmapNameInFocus` while hovered, else up.
//!   The character window sets the swap button's down flag to
//!   `Player::GetAlternateConfig` whenever it changes (`CharacterWindow::Update`), so set
//!   II shows the down bitmap, hovered or not.
//!
//! - the same handler shows `AutoSortRollover` over either auto-sort button
//!   (the second only with more than one bag) and `reagentDepositRollover`
//!   over the move-reagents button ("Move Reagents", `RolloverLine1`), the
//!   hovered button drawn with its `bitmapNameInFocus` (`ButtonStatic` as
//!   below);
//! - the market window's search box shows `marketSearchRollover`
//!   (`tagVendorSearchBoxInfo`, a `RolloverLine1` shown as above; PORT:
//!   placement mode 2, the handler `MarketWindow::HandleMouseEvent`'s own is not read);
//! - the local map's legend buttons (`localmap.rs`, `LocalMap::HandleMouseEvent`) show
//!   their `iconbutton_<kind>rollover` records the same way (mode 2).
//! - the caravan window's buy tab: "Expand Stash" and the next page's cost
//!   (PORT; mode 2).
//!
//! - the health / energy orbs and the experience bar (`UIPlayerHud::HandleMouseEvent`):
//!   after the HUD buttons above it tests the `hudManaBar`,
//!   `hudHealthBar` and `hudExperienceBar` widgets (the `BarGraph` hit test
//!   `BarGraph::HitTest`: the widget's whole rect, not its filled part) and shows
//!   their `hudManaBarRollover` / `hudHealthBarRollover` /
//!   `hudExperienceBarRollover` (mode 0 like the buttons). These are
//!   `RolloverLine1` records (loader `RolloverLine1::Load`: `Line1FormatTag`,
//!   `Line1FormatTag2`, `Line1Tag`, `Line1Style`, `rolloverStyle`; one line,
//!   no second line), their values set by the HUD update (`UIPlayerHud::Update`) and
//!   formatted per class: health (`RolloverLineHealth::GetText`) `Line1FormatTag`
//!   (`LifeDisplayFormat`) with `GetCurrentLifeInt` and `(int)` total life;
//!   energy (`RolloverLineEnergy::GetText`) `ManaDisplayFormat` with `(int)` current, total
//!   and reserved energy; experience (`RolloverLineExperience::GetText`) at the last level
//!   `Line1FormatTag2` (`tagMaxLevel`), else `tagExpDisplayFormat` with the
//!   experience and the experience left to the next level as f32 values
//!   (`(float)(int)` xp, `(float)(int)next − (float)xp`) written with a comma
//!   every three digits (`UI::FormatThousands`);
//! - the level-up indicator's rollover (`playerPortraitLevelUpIndicatorRollover`,
//!   `RolloverLine1` `RolloverLine1::GetText`: `Line1FormatTag` with `Line1Tag`) is tested
//!   on `playerPortraitLevelUpIndicatorButton` while the HUD's level-up flag is
//!   set (from the player's level up, `UIPlayerHud::OnAttributePointsGained`, until `UIPlayerHud::ShowCharacterWindow`
//!   clears it; [`super::hud::LevelUpIndicator`]); the orb HUD master table has
//!   no such button, so it never shows in the orb HUD.
//!
//! The HUD's hover capture is click-through prevention, not a window over the HUD: it does not hide
//! these rollovers; only a registered window button in front of the rect (`widgets::frontmost`) does.
//!
//! - the loot filter button (`hudLootFilterButton`, `UIPlayerHud::HandleMouseEvent`:
//!   tested after the compass button, in mouse input mode) shows
//!   `hudLootFilterButtonRollover`, a `RolloverLine3` (same class,
//!   `RolloverLine2`; the HUD constructor sets its key actions 56 and 57):
//!   line 1 as above, line 2 = `Line2FormatTag` with `{%s0}` = `Line2Tag`
//!   and `{%t1}` = the key of the second action ("Toggle Hide All Items",
//!   unbound by default) when the record has a `Line2FormatTag` (unwrapped),
//!   else `Line2Tag` wrapped as above; line 3 = `Line3Tag` in `Line3Style`,
//!   wrapped the same way (`RolloverLine2::GetText`).
//!
//! The key names are the bound keys' primary, else the secondary, else
//! `tagKeybindUnassigned` (defaults 33 = DIK_L, 34 = DIK_W). The text area has a 7 px
//! padding, as the item tooltips.
use super::text::FontCache;
use super::{HudValues, Ui};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, Widget};
use gd_game_core::data::GameData;

pub struct RolloverPlugin;

impl Plugin for RolloverPlugin {
    fn build(&self, app: &mut App) {
        // The HUD's hit rects are read by several modules' systems.
        app.init_resource::<super::hud::HudLayout>()
            .init_resource::<super::status::StatusLayout>()
            .add_systems(
                Update,
                (
                    hud_rollover.after(super::HudSet),
                    button_tip,
                    super::tip::delay_tips,
                )
                    .in_set(gd_ui_kit::UiPlaySet),
            );
    }
}

#[derive(Component)]
struct RolloverRoot;

/// The menu manager's rollover box root: not part of any rebuilt window tree, moved with the cursor.
#[derive(Component)]
pub struct MenuTipRoot;

const MENU_TIP_STYLE: &str = "records/ui/styles/text/style_rollover_title.dbr";
/// 500 ms since the text was set (`MenuRolloverBox::Draw`; the class does not read a style delay).
pub const MENU_TIP_DELAY: f32 = 500.0;

/// The menu manager's rollover box (`MenuManager::SetRollover`, `MenuRolloverBox::SetText` / `Draw`):
/// an opaque black rect (the text grown by 10 px each side), the `style_genericrolloverbox` border,
/// the lines in `style_rollover_title` in white. Drawn in SCREEN pixels (the engine's `RenderText2d`
/// does not UI-scale it); the caller places the root at [`menu_tip_origin`]. Returns the text size.
pub fn menu_tip(ui: &mut Ui, tag: &str) -> Vec2 {
    let text = ui
        .fonts
        .tag(tag)
        .replace(['{', '}'], "")
        .replace("^N", "^n");
    menu_tip_text(ui, &text)
}

/// [`menu_tip`] with already-localised text (no tag lookup, no `{}` stripping).
pub fn menu_tip_text(ui: &mut Ui, text: &str) -> Vec2 {
    let lines: Vec<&str> = text.split("^n").map(str::trim).collect();
    let lh = ui.line_height(MENU_TIP_STYLE);
    let tw = lines
        .iter()
        .map(|l| ui.text_width(MENU_TIP_STYLE, l))
        .fold(0.0f32, f32::max);
    let size = Vec2::new(tw.round(), lh * lines.len() as f32);
    let r = URect::new(0.0, 0.0, size.x + 20.0, size.y + 20.0);
    ui.solid(r, Color::BLACK);
    if let Some(b) = ui.record("records/ui/styles/rollover/style_genericrolloverbox.dbr") {
        ui.border(&b, "", r, 1.0);
    }
    for (i, l) in lines.iter().enumerate() {
        ui.text_line_colored(
            MENU_TIP_STYLE,
            Vec2::new(10.0, 10.0 + i as f32 * lh),
            l,
            Color::WHITE,
            Some([1.0; 3]),
        );
    }
    size
}

/// `MenuRolloverBox::Place`: the cursor + 64 px in both axes, pulled back so the text ends 20 px
/// inside the screen; the box is vertically centred on the anchor.
pub fn menu_tip_origin(p: Vec2, text: Vec2, screen: Vec2) -> Vec2 {
    let mut a = p + Vec2::splat(64.0);
    a.x = a.x.min(screen.x - 20.0 - text.x);
    a.y = a.y.min(screen.y - 20.0 - text.y);
    Vec2::new(a.x - 10.0, a.y - 10.0 - text.y * 0.5)
}

/// The riftgate map's centre button tooltip (`tagAerialMapCenteredButton`) in the menu manager's box.
#[allow(clippy::too_many_arguments)]
fn button_tip(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    mut roots: Query<(Entity, &MenuTipRoot, &mut Node)>,
    (map, rift, view, pause): (
        Res<crate::state::map::MapWindow>,
        Res<gd_game::riftgates::RiftgateMap>,
        Res<crate::state::riftmap::RiftMapView>,
        Res<gd_game_core::pause::GamePause>,
    ),
    mut fresh: gd_game_core::session::NewSession,
    mut shown: Local<Option<(Entity, Vec2)>>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        *shown = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let cursor = super::cursor_px(win);
    let hovered = if map.world_shown(&rift) {
        cursor.and_then(|p| view.button_at(p))
    } else if map.local_shown(&rift) {
        map.button_hover
    } else {
        None
    };
    let tip = cursor.filter(|_| size.x > 0 && hovered == Some(1) && !pause.menu);
    let Some(p) = tip else {
        if let Some((e, _)) = shown.take() {
            commands.entity(e).try_despawn();
        }
        return;
    };
    const TAG: &str = "tagAerialMapCenteredButton";
    let screen = Vec2::new(size.x as f32, size.y as f32);
    if let Some((e, text)) = *shown {
        let o = menu_tip_origin(p, text, screen) / win.scale_factor();
        if let Ok((_, _, mut node)) = roots.get_mut(e) {
            node.left = Val::Px(o.x);
            node.top = Val::Px(o.y);
            return;
        }
        *shown = None;
    }
    let root = gd_ui_kit::spawn_root(&mut commands, MenuTipRoot, 210);
    commands.entity(root).insert((
        Visibility::Hidden,
        super::tip::TipDelay::menu(TAG.to_owned(), MENU_TIP_DELAY),
    ));
    // The style's font size and the paddings are not UI-scaled.
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        1.0,
        1.0 / win.scale_factor(),
        screen,
    );
    let text = menu_tip(&mut ui, TAG);
    let o = menu_tip_origin(p, text, screen) / win.scale_factor();
    commands.entity(root).insert(Node {
        position_type: PositionType::Absolute,
        left: Val::Px(o.x),
        top: Val::Px(o.y),
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    });
    *shown = Some((root, text));
}

#[derive(Component)]
struct FocusRoot;

#[derive(Clone, Copy)]
pub(super) enum Placement {
    /// Mode 0: the window's right edge at the anchor.
    LeftOf,
    /// Mode 2: the window's left edge at the anchor.
    RightOf,
    /// Mode 3: centred above the anchor (`FactionsWindow::PlaceRollover`).
    Above,
}

/// The classic HUD's toolbar buttons with a rollover; the orb HUD's sit inside its open menu folder.
const MENU_BUTTONS: [&str; 6] = [
    "hudCharacterButton",
    "hudAchievementsButton",
    "hudSkillsButton",
    "hudQuestButton",
    "hudMiniMapButton",
    "hudFactionButton",
];

/// (button, rollover, key actions of lines 1 and 2: 0 none). The rollover is the HUD table's field, or a record path
/// (`records/...`).
const BUTTONS: [(&str, &str, u32, u32); 13] = [
    // `UIPlayerHud::Load` hard-codes this rollover; key action 43 set by `UIPlayerHud::UIPlayerHud`; tested first.
    (
        "hotSlotSelectUpButton",
        "records/ui/hud/hud_hotslotswaprollover.dbr",
        super::keys::QUICKBAR_SWITCH,
        0,
    ),
    // The group button has no window or key here; faction key action 52, achievements 53 (`UIPlayerHud::UIPlayerHud`).
    (
        "hudCharacterButton",
        "hudCharacterButtonRollover",
        super::keys::CHARACTER,
        0,
    ),
    (
        "hudSkillsButton",
        "hudSkillsButtonRollover",
        super::keys::SKILLS,
        0,
    ),
    (
        "hudQuestButton",
        "hudQuestButtonRollover",
        super::keys::CODEX,
        0,
    ),
    (
        "hudMiniMapButton",
        "hudMiniMapButtonRollover",
        super::keys::MAP,
        0,
    ),
    (
        "hudFactionButton",
        "hudFactionButtonRollover",
        super::keys::FACTIONS,
        0,
    ),
    (
        "hudAchievementsButton",
        "hudAchievementsButtonRollover",
        super::keys::ACHIEVEMENTS,
        0,
    ),
    ("hudTeleportButton", "hudTeleportButtonRollover", 33, 0),
    (
        "hudActiveTeleportButton",
        "hudTeleportButtonRollover",
        33,
        0,
    ),
    (
        "hudButtonWeaponSwap",
        "hudButtonWeaponSwapRollover",
        super::keys::WEAPON_SWAP,
        0,
    ),
    ("hudCompassButton", "hudCompassButtonRollover", 0, 0),
    (
        "hudLootFilterButton",
        "hudLootFilterButtonRollover",
        super::keys::LOOT_FILTER,
        super::keys::HIDE_ALL_ITEMS,
    ),
    (
        "hudMenuOptionsButton",
        "hudMenuOptionsButtonRollover",
        super::keys::GAME_MENU,
        0,
    ),
];

/// `{%t1}`: the primary, else the secondary, else `tagKeybindUnassigned` (`RolloverLine2::GetText`).
fn bound_key(bindings: &super::keys::KeyBindings, fonts: &FontCache, action: u32) -> String {
    let [p, s] = bindings.get(action);
    match if p != 0 { p } else { s } {
        0 => fonts.tag("tagKeybindUnassigned").to_owned(),
        c => super::keys::key_name(c),
    }
}

/// In the handler's test order.
const BARS: [(&str, &str); 3] = [
    ("hudManaBar", "hudManaBarRollover"),
    ("hudHealthBar", "hudHealthBarRollover"),
    ("hudExperienceBar", "hudExperienceBarRollover"),
];

#[derive(Clone, PartialEq)]
enum Line1 {
    /// `RolloverLine2` / `RolloverLine3`: `{%t1}` = this key name (and line 2's).
    Key(String, String),
    /// `RolloverLine1`: the line, formatted by its class.
    Text(String),
    Lines(Vec<(String, String, bool)>),
    Armor(gd_sim::sheet::ArmorBreakdown),
}

/// (format tag field, arguments).
fn bar_text(field: &str, v: &HudValues, vit: &super::HudVitals) -> (&'static str, Vec<String>) {
    match field {
        "hudHealthBar" => (
            "Line1FormatTag",
            vec![
                (vit.health.trunc() as i32).to_string(),
                (vit.health_max as i32).to_string(),
            ],
        ),
        "hudManaBar" => (
            "Line1FormatTag",
            vec![
                (vit.energy as i32).to_string(),
                (vit.energy_max as i32).to_string(),
                (vit.energy_reserved as i32).to_string(),
            ],
        ),
        _ if v.max_level => ("Line1FormatTag2", Vec::new()),
        _ => {
            let xp = v.experience_points as i32 as f32;
            let left = v.experience_next as i32 as f32 - v.experience_points as u32 as f32;
            (
                "Line1FormatTag",
                vec![
                    super::overhead::thousands(xp as i64),
                    super::overhead::thousands(left as i64),
                ],
            )
        }
    }
}

fn rollover_record(ui: &Ui, master: &str, field: &str) -> Option<String> {
    ui.record(master)
        .and_then(|m| m.str(field).map(str::to_owned))
}

pub fn button_rect(ui: &mut Ui, field: &str) -> Option<URect> {
    let to = super::hud::toolbar_origin(ui)?;
    let rec = ui.record(ui.hud_master())?.str(field)?.to_string();
    let Widget::Button {
        up: Some(up), x, y, ..
    } = ui.widget(&rec)?
    else {
        return None;
    };
    let (_, sz) = ui.image(&up)?;
    Some(URect::new(
        to.x + ui.sc(x),
        to.y + ui.sc(y),
        ui.sc(sz.x as f32),
        ui.sc(sz.y as f32),
    ))
}

/// `{%s0}` / `{%t1}`-style arguments into a localised format (the loaded tags may have lost their braces).
pub(super) fn fill(fmt: &str, args: &[&str]) -> String {
    let mut out = fmt.to_string();
    for (i, a) in args.iter().enumerate() {
        for k in ["s", "t", "d"] {
            out = out
                .replace(&format!("{{%{k}{i}}}"), a)
                .replace(&format!("%{k}{i}"), a);
        }
    }
    out
}

/// A stats row's rollover lines (style, text, wrapped). The page updates (`CharacterWindow::UpdatePage1..3`)
/// store each line's two values ([`gd_sim::sheet::RollValues`]) and its class formats them:
/// - `rolloverline2.tpl` rows (page I: class `SheetRolloverLine2`, text
///   `SheetRolloverLine2::GetText`; the list rows' `SheetListRolloverLine2::GetText` pass floats):
///   line 1 = `Line1FormatTag` with `{%s0}` = `Line1Tag` and both values as
///   `(int)`, or `Line1FormatTag2` with the first only when the second is 0;
///   line 2 = `Line2Tag`, wrapped;
/// - `rolloverline5/7/8.tpl` (one class, `SheetRolloverLine5`, load
///   `SheetRolloverLine5::Load`, text `SheetRolloverLine5::GetText`): line 1 as above; lines 2..4 (when
///   their `LineNTag` is set) the same with `LineNFormatTag` / `...Tag2` and
///   the values as floats, line 2 instead `SimpleStringFormat(Line2Tag)` +
///   the row's name ([`gd_sim::sheet::RollValues::name`]) when it has one;
///   lines 5..7 formatted when they have a `LineNFormatTag`, else their
///   `LineNTag` as a wrapped description; line 8 the description.
///
/// Rows without computed values read line 1's values back from the row's own text (PORT), the second being
/// the over-cap amount on capped rows (`CharacterWindow::GetCappedDefense`; `over.` keys) or a range's maximum.
/// The lines keep their colour codes. `None`: not a line class.
fn sheet_lines(
    ui: &Ui,
    roll: &str,
    key: &str,
    values: &HudValues,
) -> Option<Vec<(String, String, bool)>> {
    use gd_sim::items::tooltip::Arg;
    let rec = ui.record(roll)?;
    let tpl = rec.str("templateName").unwrap_or("");
    if tpl.ends_with("simplerollover.tpl") {
        return skill_lines(ui, &rec, key, values);
    }
    let multi = [
        "rolloverline5.tpl",
        "rolloverline7.tpl",
        "rolloverline8.tpl",
    ]
    .iter()
    .any(|t| tpl.ends_with(t));
    if !multi && !tpl.ends_with("rolloverline2.tpl") {
        return None;
    }
    let computed = values.rollovers.get(key);
    let first = match computed {
        Some(r) => r.lines[0],
        None => {
            let nums = row_numbers(values.stats.get(key)?);
            let over_key = match key.strip_prefix("list.") {
                Some(k) => format!("list.over.{k}"),
                None => format!("over.{key}"),
            };
            let n2 = values
                .stats
                .get(&over_key)
                .and_then(|v| v.parse::<f32>().ok())
                .or(nums.get(1).copied())
                .unwrap_or(0.0);
            (nums.first().copied().unwrap_or(0.0), n2)
        }
    };
    let field = |k: &str| rec.str(k).filter(|t| !t.is_empty()).map(str::to_owned);
    let tag = |k: &str| field(k).map(|t| ui.fonts.tag(&t).to_owned());
    // `LineNFormatTag` when the second value is set, else `...Tag2` (the
    // first only); a missing `...Tag2` falls back to the first.
    let line = |n: usize, (v1, v2): (f32, f32), int: bool, own_fmt: Option<&str>| -> String {
        let (f1, f2) = (format!("Line{n}FormatTag"), format!("Line{n}FormatTag2"));
        let fmt = match (own_fmt, v2 != 0.0) {
            (Some(f), true) => Some(f.to_owned()),
            (_, true) => field(&f1),
            (_, false) => field(&f2).or_else(|| field(&f1)),
        };
        let fmt = fmt.map(|f| ui.fonts.tag(&f).to_owned()).unwrap_or_default();
        let label = computed
            .and_then(|r| r.labels[n - 1].as_deref())
            .map(|t| ui.fonts.tag(t).to_owned())
            .or_else(|| tag(&format!("Line{n}Tag")))
            .unwrap_or_default();
        let (v1, v2) = if int {
            (v1.trunc(), v2.trunc())
        } else {
            (v1, v2)
        };
        format(&fmt, &[Arg::Str(&label), Arg::Num(v1), Arg::Num(v2)])
    };
    let style = |n: usize| rec.str(&format!("Line{n}Style")).unwrap_or("").to_owned();
    // Page I's rows and the 8-line class pass `(int)`, page II / III's list rows floats
    // (`SheetListRolloverLine2::GetText`).
    let int = multi || !key.starts_with("list.");
    let mut out = vec![(
        style(1),
        line(1, first, int, computed.and_then(|r| r.format.as_deref())),
        false,
    )];
    if !multi {
        if let Some(t) = tag("Line2Tag") {
            out.push((style(2), t, true));
        }
        return Some(out);
    }
    let vals = |n: usize| computed.map_or((0.0, 0.0), |r| r.lines[n - 1]);
    for n in 2..=8 {
        let Some(label) = computed
            .and_then(|r| r.labels[n - 1].as_deref())
            .map(|t| ui.fonts.tag(t).to_owned())
            .or_else(|| tag(&format!("Line{n}Tag")))
        else {
            continue;
        };
        let formatted = n <= 4 || (n < 8 && field(&format!("Line{n}FormatTag")).is_some());
        let text = if n == 2
            && let Some(name) = computed.and_then(|r| r.name.as_deref())
        {
            // `SimpleStringFormat` of the label, then the foe's name.
            // PORT: not in the monster colour; a space after the label (the tag loader loses the trailing one).
            let simple = ui.fonts.tag("SimpleStringFormat").to_owned();
            format!(
                "{} {}",
                format(&simple, &[Arg::Str(&label)]).trim_end(),
                ui.fonts.tag(name)
            )
        } else if formatted {
            line(n, vals(n), false, None)
        } else {
            out.push((style(n), label, true));
            continue;
        };
        out.push((style(n), text, false));
    }
    Some(out)
}

/// The page II LMB / RMB rollovers (`tab2LMBRollover` / `tab2RMBRollover`,
/// a simple rollover filled by the page update, `CharacterWindow::UpdatePage2` +
/// `CharacterWindow::BuildSkillDamageRollover`): the skill's display name, one line per damage type
/// (`tagCharStats<Type>` for a hit, `tagCharStatsDur<Type>` for a duration
/// damage, names by `CharacterWindow::GetDamageTypeName` / `CharacterWindow::GetDotTypeName`, then " " and
/// `tagRollFormat18` "min - max" or `tagRollFormat19` when equal) in the
/// record's `defaultTextStyle`, then `tagCharStatsDPSLeftDescription` /
/// `...RightDescription` in `style_rollover_description.dbr`, wrapped.
fn skill_lines(
    ui: &Ui,
    rec: &gd_formats::arz::Record,
    key: &str,
    values: &HudValues,
) -> Option<Vec<(String, String, bool)>> {
    use gd_sim::attributes::CombatType as T;
    use gd_sim::items::tooltip::Arg;
    let n: usize = key.strip_prefix("list.Skill")?.parse().ok()?;
    let st = rec.str("defaultTextStyle").unwrap_or("").to_owned();
    let title = ui
        .fonts
        .tag(values.stats.get(&format!("list.SkillTag{n}"))?)
        .to_owned();
    let mut out = vec![(st.clone(), title, false)];
    for &(dot, ty, min, max) in values.skill_breakdown.get(n)? {
        let name = if dot {
            match ty {
                T::Physical => "InternalTrauma",
                T::Cold => "Frostburn",
                T::Fire => "Burn",
                T::Acid => "Poison",
                T::Lightning => "Electrocute",
                T::Vitality => "VitalityDecay",
                T::Bleeding => "Bleeding",
                T::LifeLeech => "LifeLeech",
                T::ManaLeech => "ManaLeech",
                _ => "Unknown",
            }
            .to_owned()
        } else {
            ty.name().to_owned()
        };
        let label = ui
            .fonts
            .tag(&format!(
                "{}{name}",
                if dot {
                    "tagCharStatsDur"
                } else {
                    "tagCharStats"
                }
            ))
            .to_owned();
        let range = if min == max {
            format(ui.fonts.tag("tagRollFormat19"), &[Arg::Num(min)])
        } else {
            format(
                ui.fonts.tag("tagRollFormat18"),
                &[Arg::Num(min), Arg::Num(max)],
            )
        };
        out.push((st.clone(), format!("{label} {range}"), false));
    }
    let desc = if n == 0 {
        "tagCharStatsDPSLeftDescription"
    } else {
        "tagCharStatsDPSRightDescription"
    };
    out.push((
        "records/ui/styles/text/style_rollover_description.dbr".into(),
        ui.fonts.tag(desc).to_owned(),
        true,
    ));
    Some(out)
}

/// Keeps the `^X` colour codes: the font renderer colours the spans (`GraphicsFont2::ProcessCharacter`).
fn format(fmt: &str, args: &[gd_sim::items::tooltip::Arg]) -> String {
    gd_sim::items::tooltip::format_rich(fmt, args)
}

/// "+15%" → 15, "12 - 20" → 12, 20, "-22%" → -22.
fn row_numbers(text: &str) -> Vec<f32> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let neg = c[i] == '-' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit());
        if c[i].is_ascii_digit() || neg {
            let start = i;
            i += 1;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                i += 1;
            }
            if let Ok(v) = c[start..i].iter().collect::<String>().parse() {
                out.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// `RolloverLine::GetWrapSample`: a text longer than `n` is cut at the space nearest to index n (the later one
/// on a tie), keeping 2 more characters. `None`: the line is not wrapped.
pub fn wrap_sample(text: &str, n: usize) -> Option<String> {
    let c: Vec<char> = text.chars().collect();
    if n == 0 || c.len() <= n {
        return None;
    }
    let mut back = n;
    while back > 0 && c[back] != ' ' {
        back -= 1;
    }
    let mut fwd = n;
    while fwd < c.len() && c[fwd] != ' ' {
        fwd += 1;
    }
    let len = if n - back >= fwd - n {
        fwd + 2
    } else {
        back + 2
    };
    Some(c[..len.min(c.len())].iter().collect())
}

/// Wrapped at its own sample's width or the widest line so far.
fn push_wrapped(
    ui: &mut Ui,
    lines: &mut Vec<(String, String)>,
    text: &str,
    st: &str,
    chars: usize,
) {
    let text = text.replace("{^n}", "\n").replace("^n", "\n");
    match wrap_sample(&text, chars) {
        Some(sample) => {
            let widest = lines
                .iter()
                .map(|(s, t)| ui.text_width(s, t))
                .fold(0.0, f32::max);
            let wrap_w = ui.text_width(st, &sample).max(widest);
            lines.extend(
                ui.wrap_lines(st, wrap_w, &text)
                    .into_iter()
                    .map(|l| (st.to_owned(), l)),
            );
        }
        None => lines.extend(text.split('\n').map(|l| (st.to_owned(), l.to_string()))),
    }
}

#[allow(clippy::too_many_arguments)]
fn hud_rollover(
    #[cfg(feature = "dev")] hooks: Res<gd_game_core::hooks::Hooks>,
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    (values, vitals): (Res<HudValues>, Res<super::HudVitals>),
    time: Res<Time<Real>>,
    mut roots: Query<(Entity, &mut Node), With<RolloverRoot>>,
    // (rollover, anchor and line-1 text, ms hovered, drawn, anchor and outer box (physical px) it was built at).
    mut state: Local<(
        Option<(String, Vec2, Line1)>,
        f32,
        bool,
        Option<(Vec2, URect)>,
    )>,
    (panels, layout, inv, trade, trade_layout, stash): (
        Res<super::UiPanels>,
        Res<crate::state::inventory::InvLayout>,
        Res<gd_game_core::inventory::Inventory>,
        Res<crate::state::trade::Trade>,
        Res<crate::state::trade::TradeLayout>,
        Option<Res<gd_game_core::stash::Stash>>,
    ),
    focus: Query<Entity, With<FocusRoot>>,
    (map, rift, bindings, mut fresh, (pause, menu)): (
        Res<crate::state::map::MapWindow>,
        Res<gd_game::riftgates::RiftgateMap>,
        Res<super::keys::KeyBindings>,
        gd_game_core::session::NewSession,
        (
            Res<gd_game_core::pause::GamePause>,
            Res<super::gamemenu::GameMenu>,
        ),
    ),
    (mut focused, hud_layout, level_up): (
        Local<Option<(URect, String)>>,
        Res<super::hud::HudLayout>,
        Res<super::hud::LevelUpIndicator>,
    ),
    (loot, pane_zones, (dialog, conv), (factions, faction_view)): (
        Res<super::lootfilter::LootFilterView>,
        Res<crate::state::survival::PaneRollovers>,
        (
            Res<super::dialog::DialogLayout>,
            Res<gd_game::conversation::Conversation>,
        ),
        (
            Res<super::factions::FactionsLayout>,
            Res<super::factions::FactionsView>,
        ),
    ),
    mut assets: gd_ui_kit::UiAssets,
    (button_sounds, gz, confirm): (
        Query<(&super::widgets::ButtonSound, &ChildOf, &InheritedVisibility)>,
        super::widgets::RootZ,
        Res<super::apply::InvConfirm>,
    ),
) {
    if fresh.begun() {
        *state = Default::default();
        *focused = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 {
        return;
    }
    let mut ui = assets.ui(
        &mut commands,
        &data,
        Entity::PLACEHOLDER,
        assets.scale(size),
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let shown_teleport = values.teleport.map(super::hud::teleport_field);
    #[cfg(feature = "dev")]
    let (test, sheet_hover) = (
        hooks.ui.hud_hover.as_deref(),
        hooks.ui.sheet_hover.as_deref(),
    );
    #[cfg(not(feature = "dev"))]
    let (test, sheet_hover): (Option<&str>, Option<&str>) = (None, None);
    let mut hit = None;
    // A HUD rect hovers only while no registered window blocker / button is in front of it (`widgets::frontmost`).
    let cursor_p = super::cursor_px(win);
    let front = cursor_p.and_then(|p| super::widgets::frontmost(p, &button_sounds, &gz));
    let in_front = |r: &URect| front.is_none_or(|f| f.rect == *r);
    for (field, roll, key, key2) in BUTTONS {
        if field.contains("Teleport") && shown_teleport != Some(field) {
            continue;
        }
        if menu.blocks_hud(&pause, field) {
            continue;
        }
        let menu_button = MENU_BUTTONS.contains(&field);
        if menu_button && !(ui.classic_hud || values.menu_folder.buttons) {
            continue;
        }
        let Some(r) = hud_layout.rect(field) else {
            continue;
        };
        let cursor = if test == Some(field) {
            Some(Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5))
        } else {
            cursor_p.filter(|_| in_front(&r))
        };
        if let Some(p) = cursor.filter(|p| super::contains(&r, *p)) {
            // The disabled weapon-swap button explains itself (`UIPlayerHud::HandleMouseEvent`).
            let roll = if field == "hudButtonWeaponSwap" && !values.weapon_swap {
                "hudButtonWeaponSwapRolloverSpecial"
            } else {
                roll
            };
            let name = |k: u32| {
                if k == 0 {
                    String::new()
                } else {
                    bound_key(&bindings, ui.fonts, k)
                }
            };
            hit = Some((
                if roll.starts_with("records/") {
                    Some(roll.to_owned())
                } else {
                    rollover_record(&ui, ui.hud_master(), roll)
                },
                Line1::Key(name(key), name(key2)),
                p,
                Placement::LeftOf,
            ));
            break;
        }
    }
    // The orb HUD's level-up buttons: `UIButton::GetRolloverText` = `SimpleStringFormat` of its `rolloverTag` in its
    // `textStyleName`, the window in its `rolloverStyleName`, placement mode 3. PORT: anchored at the button's top centre.
    if hit.is_none() && !pause.menu && !ui.classic_hud {
        let points = [values.attribute_points, values.skill_points];
        for (field, n) in super::hud::LEVEL_UP_BUTTONS.into_iter().zip(points) {
            let Some(r) = hud_layout.rect(field).filter(|_| n > 0) else {
                continue;
            };
            let over = test == Some(field)
                || (in_front(&r) && cursor_p.is_some_and(|p| super::contains(&r, p)));
            if over
                && let Some(path) = rollover_record(&ui, ui.hud_master(), field)
                && let Some(rec) = ui.record(&path)
                && let Some(tag) = rec.str("rolloverTag")
            {
                let simple = ui.fonts.tag("SimpleStringFormat").to_owned();
                let text = fill(&simple, &[ui.fonts.tag(tag)]);
                let style = rec.str("textStyleName").unwrap_or("").to_owned();
                hit = Some((
                    Some(path),
                    Line1::Lines(vec![(style, text, false)]),
                    Vec2::new(r.x + r.w * 0.5, r.y),
                    Placement::Above,
                ));
                break;
            }
        }
    }
    // The menu or a modal question took the mouse: a hit without a rollover stops every test below.
    if (pause.menu || crate::apply::question_up(&inv, &trade, &confirm)) && hit.is_none() {
        hit = Some((
            None,
            Line1::Key(String::new(), String::new()),
            Vec2::ZERO,
            Placement::LeftOf,
        ));
    }
    // The classic HUD's level-up indicator: a hidden 60 x 60 button at the portrait origin.
    if hit.is_none()
        && ui.classic_hud
        && level_up.0
        && let Some(r) = hud_layout.rect(super::hud::LEVEL_UP_BUTTON)
    {
        let test_hit = test == Some("playerPortraitLevelUpIndicatorButton");
        let cursor = if test_hit {
            Some(Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5))
        } else {
            cursor_p.filter(|_| in_front(&r))
        };
        if let Some(p) = cursor.filter(|p| super::contains(&r, *p)) {
            hit = Some((
                rollover_record(
                    &ui,
                    ui.hud_master(),
                    "playerPortraitLevelUpIndicatorRollover",
                ),
                Line1::Key(String::new(), String::new()),
                p,
                Placement::LeftOf,
            ));
        }
    }
    for (field, roll) in BARS {
        if hit.is_some() {
            break;
        }
        let Some(r) = hud_layout.rect(field) else {
            continue;
        };
        let cursor = if test == Some(field) {
            Some(Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5))
        } else {
            cursor_p.filter(|_| in_front(&r))
        };
        if let Some(p) = cursor.filter(|p| super::contains(&r, *p))
            && let Some(rec) = rollover_record(&ui, ui.hud_master(), roll)
            && let Some(rr) = ui.record(&rec)
        {
            let (fmt_field, args) = bar_text(field, &values, &vitals);
            let fmt = rr
                .str(fmt_field)
                .map(|t| ui.fonts.tag(t).to_owned())
                .unwrap_or_default();
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            hit = Some((
                Some(rec),
                Line1::Text(fill(&fmt, &args)),
                p,
                Placement::LeftOf,
            ));
        }
    }
    // Mode 0 like the HUD buttons.
    for z in pane_zones
        .crucible
        .iter()
        .chain(&pane_zones.endless)
        .chain(&pane_zones.mutators)
        .chain(&pane_zones.challenge)
    {
        if hit.is_some() {
            break;
        }
        let cursor = if test == Some(z.field) {
            Some(Vec2::new(
                z.rect.x + z.rect.w * 0.5,
                z.rect.y + z.rect.h * 0.5,
            ))
        } else {
            cursor_p.filter(|_| in_front(&z.rect))
        };
        if let Some(p) = cursor.filter(|p| super::contains(&z.rect, *p)) {
            let line1 = match &z.lines {
                Some(lines) => Line1::Lines(lines.clone()),
                None => Line1::Key(String::new(), String::new()),
            };
            hit = Some((Some(z.record.clone()), line1, p, Placement::LeftOf));
        }
    }
    let mut focus_rect = None;
    let illusionist = matches!(
        trade.open,
        Some(gd_game::trade::TradeOpen::Illusionist { .. })
    );
    let windows = [
        (
            illusionist,
            trade_layout.swap_button,
            gd_game::illusion::WINDOW,
        ),
        (
            panels.character,
            layout.swap_button,
            super::character::MASTER,
        ),
    ];
    for (open, rect, master) in windows {
        if hit.is_none()
            && open
            && let Some(r) = rect
            && let Some(p) = (if test == Some("equipSwap1LeftButton") {
                Some(Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5))
            } else {
                super::cursor_px(win)
            })
            .filter(|p| super::contains(&r, *p))
        {
            hit = Some((
                rollover_record(&ui, master, "equipSwapButtonRollover"),
                Line1::Key(String::new(), String::new()),
                p,
                Placement::RightOf,
            ));
            if !inv.alternate {
                focus_rect = ui
                    .record(master)
                    .and_then(|m| m.str("equipSwap1LeftButton").map(|f| (r, f.to_owned())));
            }
        }
    }
    let centre = |r: &URect| Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5);
    let test_button = match sheet_hover {
        Some("invSortButton1") => layout.sort_buttons.first().map(|b| centre(&b.1)),
        Some("invSortButton2") => layout.sort_buttons.get(1).map(|b| centre(&b.1)),
        Some("reagentDepositButton") => layout.reagent_button.as_ref().map(centre),
        _ => None,
    };
    if hit.is_none()
        && panels.character
        && let Some(p) = test_button.or(super::cursor_px(win))
    {
        let over = |r: &URect| super::contains(r, p);
        // `ButtonStatic::Render`'s `bitmapNameInFocus`.
        let button = |k: &str| {
            ui.record(super::character::MASTER)
                .and_then(|m| m.str(k).map(str::to_owned))
        };
        let roll = if let Some(i) = layout.sort_buttons.iter().position(|(_, r)| over(r)) {
            focus_rect = button(if i == 0 {
                "invSortButton1"
            } else {
                "invSortButton2"
            })
            .map(|f| (layout.sort_buttons[i].1, f));
            Some("AutoSortRollover")
        } else if let Some(r) = layout.reagent_button.filter(|r| over(r)) {
            focus_rect = button("reagentDepositButton").map(|f| (r, f));
            Some("reagentDepositRollover")
        } else {
            None
        };
        if let Some(roll) = roll {
            hit = Some((
                rollover_record(&ui, super::character::MASTER, roll),
                Line1::Key(String::new(), String::new()),
                p,
                Placement::RightOf,
            ));
        }
    }
    // Mode 2 at the cursor like the window's other rollovers.
    if hit.is_none()
        && panels.character
        && let Some(p) = sheet_hover
            .and_then(|f| {
                layout
                    .sheet_rows(&panels.sheet_scroll)
                    .find(|(_, _, k)| k.ends_with(f))
                    .map(|(r, _, _)| Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5))
            })
            .or(super::cursor_px(win))
        && let Some((_, roll, key)) = layout
            .sheet_rows(&panels.sheet_scroll)
            .find(|(r, _, _)| super::contains(r, p))
        && let Some(line1) = if ui.record(roll).is_some_and(|r| {
            r.str("templateName")
                .is_some_and(|t| t.ends_with("combinedarmorrolloverwindow.tpl"))
        }) {
            Some(Line1::Armor(values.armor.clone()))
        } else {
            sheet_lines(&ui, roll, key, &values).map(Line1::Lines)
        }
    {
        hit = Some((Some(roll.clone()), line1, p, Placement::RightOf));
    }
    if focus_rect != *focused {
        focused.clone_from(&focus_rect);
        for e in &focus {
            ui.commands.entity(e).try_despawn();
        }
        let over = focus_rect.as_ref().and_then(|(_, f)| ui.widget(f));
        if let (
            Some((r, _)),
            Some(Widget::Button {
                over: Some(over), ..
            }),
        ) = (focus_rect, over)
            && let Some((img, _)) = ui.image(&over)
        {
            ui.root = gd_ui_kit::spawn_root(ui.commands, FocusRoot, 5);
            ui.quad(img, r, None, Color::WHITE);
        }
    }
    if hit.is_none()
        && let Some((r, path)) = map.legend_rollover(&rift)
    {
        let p = super::cursor_px(win).unwrap_or(Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5));
        hit = Some((
            Some(path),
            Line1::Key(String::new(), String::new()),
            p,
            Placement::RightOf,
        ));
    }
    // `ConversationWindow::GetFactionRollover`; mode 2 at the cursor (`UIWindow::DrawRollover`).
    if hit.is_none()
        && let (Some(tip), Some(ir)) = (&conv.faction_tip, dialog.icon)
        && conv.view.is_some()
        && let Some(p) = super::cursor_px(win)
        && super::contains(&ir, p)
        && let Some(rec) = ui.record(super::dialog::RECORD)
    {
        hit = Some((
            Some(super::dialog::RECORD.to_owned()),
            Line1::Lines(super::dialog::faction_lines(&ui, &rec, tip)),
            p,
            Placement::RightOf,
        ));
    }
    // The trade windows' search box: `marketSearchRollover`, the caravan's `searchBoxRollover`, the crafting
    // panel's `crafterRollover`. PORT: the engine's placement is not read; mode 2.
    if hit.is_none()
        && let Some(r) = trade_layout.search
        && let Some(p) = super::cursor_px(win)
        && super::contains(&r, p)
    {
        let (master, field) = match &trade.open {
            Some(gd_game::trade::TradeOpen::Stash { .. }) => {
                ("records/ui/caravan/caravan_window.dbr", "searchBoxRollover")
            }
            Some(gd_game::trade::TradeOpen::Crafter { .. }) => (
                "records/ui/inventor/craftingpanel/crafting_table.dbr",
                "crafterRollover",
            ),
            _ => (
                if trade.faction.is_some() {
                    "records/ui/vendors/faction_vendor_mastertable.dbr"
                } else {
                    "records/ui/vendors/vendor_mastertable.dbr"
                },
                "marketSearchRollover",
            ),
        };
        hit = Some((
            rollover_record(&ui, master, field),
            Line1::Key(String::new(), String::new()),
            p,
            Placement::RightOf,
        ));
    }
    // The caravan window's buy tab. PORT: no engine rollover found; built from the stock parts: `tagCaravanExpand`
    // in the title style, then `tagCaravanCost` in `CanAffordFontStyle` / `CannotAffordFontStyle`, mode 2.
    if hit.is_none()
        && trade.prompt.is_none()
        && let Some(r) = trade_layout.buy
        && let Some(p) = (if test == Some("buyTabButton") {
            Some(Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5))
        } else {
            super::cursor_px(win)
        })
        .filter(|p| super::contains(&r, *p))
        && let Some(cost) = stash
            .as_deref()
            .and_then(|s| gd_game_core::stash::next_page_cost(&data.db, s, trade.transfer))
        && let Some(window) = ui.record("records/ui/caravan/caravan_window.dbr")
        && let Some(sw) = window.str("StashWindow").and_then(|s| ui.record(s))
    {
        let afford = sw
            .str(if values.gold >= cost {
                "CanAffordFontStyle"
            } else {
                "CannotAffordFontStyle"
            })
            .unwrap_or_default()
            .to_owned();
        let title = "records/ui/styles/text/style_rollover_title.dbr".to_owned();
        let cost_text = super::floating::format_tag(ui.fonts.tag("tagCaravanCost"), &[cost as i64]);
        hit = Some((
            window.str("StashRollover").map(str::to_owned),
            Line1::Lines(vec![
                (title, ui.fonts.tag("tagCaravanExpand").to_owned(), false),
                (afford, cost_text, false),
            ]),
            p,
            Placement::RightOf,
        ));
    }
    // Mode 3 at the pointer (`FactionsWindow::PlaceRollover`).
    if hit.is_none()
        && let Some((part, p)) = super::factions::hovered(&factions, &faction_view, win)
        && let Some(lines) = factions.tip(part).filter(|l| !l.is_empty())
        && let Some(roll) = ui
            .record(super::factions::RECORD)
            .and_then(|m| m.str("factionBarRollover").map(str::to_owned))
    {
        hit = Some((Some(roll), Line1::Lines(lines.clone()), p, Placement::Above));
    }
    // The loot filter window's checkboxes and Defaults button: their `...Info` text in the default rollover style.
    if hit.is_none()
        && let Some(p) = super::cursor_px(win)
        && let Some(tag) = loot.tip_tag(p)
        && ui.fonts.tags.get(&tag).is_some()
    {
        let text = ui
            .fonts
            .tag(&tag)
            .replace("{^N}", "\n")
            .replace("{^n}", "\n");
        let desc = "records/ui/styles/text/style_rollover_description.dbr";
        let lines = text
            .split('\n')
            .map(|para| (desc.to_owned(), para.to_owned(), true))
            .collect();
        hit = Some((
            Some(super::lootfilter::RECORD.to_owned()),
            Line1::Lines(lines),
            p,
            Placement::RightOf,
        ));
    }
    let key = hit
        .as_ref()
        .and_then(|(rec, l1, p, _)| rec.clone().map(|r| (r, *p, l1.clone())));
    if key.as_ref().map(|k| &k.0) != state.0.as_ref().map(|k| &k.0) {
        state.1 = 0.0;
        state.2 = false;
    }
    // Only the anchor moved: keep the nodes and move the root. The armour window places itself, so it rebuilds.
    let same_content = match (&key, &state.0) {
        (Some(a), Some(b)) => {
            a.0 == b.0 && a.2 == b.2 && (!matches!(a.2, Line1::Armor(_)) || a.1 == b.1)
        }
        (None, None) => true,
        _ => false,
    };
    if !same_content {
        state.0 = key.clone();
        state.2 = false;
        state.3 = None;
        for (e, _) in &roots {
            ui.commands.entity(e).try_despawn();
        }
    } else if key != state.0 {
        // `UIPlayerHud::UpdateRollover` follows the cursor each frame.
        state.0 = key.clone();
        if let (Some((_, now, _)), Some((built, r))) = (&key, state.3) {
            let (sw, sh) = (size.x as f32, size.y as f32);
            let d = *now - built;
            let dx = d.x.clamp(-r.x, (sw - r.x - r.w).max(-r.x));
            let dy = d.y.clamp(-r.y, (sh - r.y - r.h).max(-r.y));
            let inv = 1.0 / win.scale_factor();
            for (_, mut node) in &mut roots {
                node.left = Val::Px(dx * inv);
                node.top = Val::Px(dy * inv);
            }
        }
    }
    let Some((Some(path), line1_arg, _, placement)) = hit else {
        return;
    };
    state.1 += time.delta_secs() * 1000.0;
    let Some(rec) = ui.record(&path) else { return };
    // PORT: a rollover without a style of its own uses the default one. A `UIButton`'s names it `rolloverStyleName`.
    let style = rec
        .str("rolloverStyle")
        .or(rec.str("rolloverStyleName"))
        .or(Some("records/ui/styles/rollover/style_default.dbr"))
        .and_then(|p| ui.record(p));
    let delay = style.as_ref().and_then(|s| s.i32("delay")).unwrap_or(0) as f32;
    // Paused game time shows it at once (`RolloverWindow::Update`).
    if state.2 || (state.1 < delay && !pause.paused()) {
        return;
    }
    state.2 = true;
    let Some(anchor) = state.0.as_ref().map(|k| k.1) else {
        return;
    };
    ui.root = gd_ui_kit::spawn_root(ui.commands, RolloverRoot, 210);
    if let Line1::Armor(armor) = &line1_arg {
        super::armor_window::draw(&mut ui, &rec, armor, anchor);
        return;
    }
    let tag = |ui: &Ui, k: &str| {
        rec.str(k)
            .map(|t| ui.fonts.tag(t).to_owned())
            .unwrap_or_default()
    };
    let chars = style
        .as_ref()
        .and_then(|s| s.i32("minCharsBeforeWrapping"))
        .unwrap_or(0)
        .max(0) as usize;
    let mut lines: Vec<(String, String)> = Vec::new();
    let (line1, key2) = match line1_arg {
        Line1::Key(keyname, key2) => (
            fill(
                &tag(&ui, "Line1FormatTag"),
                &[&tag(&ui, "Line1Tag"), &keyname],
            ),
            key2,
        ),
        Line1::Text(t) => (t, String::new()),
        Line1::Armor(_) => return,
        Line1::Lines(all) => {
            for (st, t, wrap) in all {
                if wrap {
                    push_wrapped(&mut ui, &mut lines, &t, &st, chars);
                } else {
                    lines.push((st, t));
                }
            }
            (String::new(), String::new())
        }
    };
    if lines.is_empty() {
        let st = |k: &str| rec.str(k).unwrap_or("").to_owned();
        let (st1, st2, st3) = (st("Line1Style"), st("Line2Style"), st("Line3Style"));
        lines.push((st1.clone(), line1));
        if rec.str("Line2FormatTag").is_some_and(|t| !t.is_empty()) {
            // `RolloverLine3` with a formatted line 2: `{%t1}` = the second key, unwrapped (`RolloverLine2::GetText`).
            if rec.str("Line2Tag").is_some_and(|t| !t.is_empty()) {
                lines.push((
                    st2.clone(),
                    fill(&tag(&ui, "Line2FormatTag"), &[&tag(&ui, "Line2Tag"), &key2]),
                ));
            }
        } else {
            let t = tag(&ui, "Line2Tag");
            push_wrapped(&mut ui, &mut lines, &t, &st2, chars);
        }
        let t = tag(&ui, "Line3Tag");
        push_wrapped(&mut ui, &mut lines, &t, &st3, chars);
    }
    let outer = draw_window(&mut ui, style.as_ref(), &lines, anchor, placement);
    state.3 = Some((anchor, outer));
}

/// A `RolloverStyleUIHelp` window holding `lines` (style, text; empty ones dropped). Returns its outer box.
pub(super) fn draw_window(
    ui: &mut Ui,
    style: Option<&gd_formats::arz::Record>,
    lines: &[(String, String)],
    anchor: Vec2,
    placement: Placement,
) -> URect {
    let lines: Vec<&(String, String)> = lines.iter().filter(|(_, t)| !t.is_empty()).collect();
    let mut text_w: f32 = 0.0;
    let mut text_h = 0.0;
    for (st, t) in lines.iter().copied() {
        text_w = text_w.max(ui.text_width(st, t));
        text_h += ui.line_height(st);
    }
    let pad = (7.0 * ui.s).round();
    let (cw, ch) = style
        .and_then(|s| s.str("upperLeftTexture").map(str::to_owned))
        .and_then(|p| ui.image(&p))
        .map_or((0.0, 0.0), |(_, sz)| {
            (ui.sc(sz.x as f32), ui.sc(sz.y as f32))
        });
    let (iw, ih) = (text_w.ceil() + 2.0 * pad, text_h + 2.0 * pad);
    let (ow, oh) = (iw + 2.0 * cw, ih + 2.0 * ch);
    let (mut x, mut y) = (anchor.x, anchor.y);
    match placement {
        Placement::LeftOf => {
            if x - ow < 0.0 {
                x = ow;
                y -= oh;
            }
            x -= ow;
        }
        // Mode 2: moved left and up when it would leave the right edge.
        Placement::RightOf => {
            if x + ow > ui.screen.x {
                x = ui.screen.x - ow;
                y -= oh;
            }
        }
        // Mode 3 (a zero-size anchor): centred above, kept on screen; above the top edge: at y 0 left of
        // the anchor, or right of it when that leaves the screen.
        Placement::Above => {
            y -= oh;
            if y >= 0.0 {
                x = x.clamp(ow * 0.5, (ui.screen.x - ow * 0.5).max(ow * 0.5)) - ow * 0.5;
            } else {
                y = 0.0;
                x = if x - ow < 0.0 { x } else { x - ow };
            }
        }
    }
    if y < 0.0 {
        y = 0.0;
    } else if y + oh > ui.screen.y {
        y = (ui.screen.y - oh).max(0.0);
    }
    let outer = URect::new(x, y, ow, oh);
    let inner = URect::new((x + cw).round(), (y + ch).round(), iw, ih);
    if let Some(st) = style {
        let f = |k: &str, d: f32| st.f32(k).unwrap_or(d);
        let bg = Color::srgba(
            f("backgroundColorRed", 0.03),
            f("backgroundColorGreen", 0.03),
            f("backgroundColorBlue", 0.04),
            f("backgroundColorAlpha", 0.9),
        );
        ui.solid(inner, bg);
        ui.border(st, "", inner, 1.0);
    }
    let mut ty = inner.y + pad;
    for (st, t) in lines.iter().copied() {
        ui.text_line(st, Vec2::new(inner.x + pad, ty), t, 1.0);
        ty += ui.line_height(st);
    }
    outer
}
