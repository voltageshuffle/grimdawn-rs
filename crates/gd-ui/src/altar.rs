//! The Ascendant Altar window (`records/ui/ascensionaltar/altar_table.dbr`, `ascensionaltarwindow.tpl`): the
//! reagent list, the six offering boxes and the bonus lists. It draws [`AltarMenu`] (the window state,
//! `crate::ascendant`) and sends the offer as `gd_game::ascendant::AltarOffer`; gd-game activates the altar.
//!
//! Grim Dawn.exe (`AltarWindow::AltarWindow`, loader `AltarWindow::Load`, draw
//! `AltarWindow::Render`, mouse `AltarWindow::HandleMouseEvent`, update
//! `AltarWindow::CloseIfFar`, show `AltarWindow::Show`, button listener
//! `AltarWindow::SetReagentTab`):
//! - the window rect is the record's (1001 × 664, centred); every widget is placed from its top-left corner:
//!   `altarBitmap`, `altarTitle`, the tab buttons, the reagent list in `altarScrollWindow`, the box glows, the
//!   boxes and their quantities, the two bonus lists, then the activate (`confirmButton`), cancel and close buttons;
//! - the tabs (`reagentTabButton1/2`, a radio group: the shown one is drawn down) pick the list
//!   (`AltarWindow.selectedTab`): tab 1 lists the inclusive formulas (`GameEngine::GetAltarInclusiveRecipes`),
//!   tab 2 the exclusive ones, in record order (`AltarWindow::RebuildReagentList`);
//! - a list row (`AltarWindow::AddReagentRow`, `reagentListItem` / `reagentListIcon` / `reagentListNameText`,
//!   class `AltarReagentListItem`): the background at its position, the reagent's bitmap scaled to the icon texture's
//!   width and centred on the row vertically, the text "[n] name" (n = the reagent count held minus the count already
//!   in the boxes, "99+" over 99; `AltarReagentListItem::SetCount`) drawn in `altarListDisabledFont` when n is 0; the
//!   next row starts 2 px below the background. A hovered row gets `ui/inventor/crafting_recipeselectionhighlight.tex`
//!   over it, 1 px larger each way (`AltarReagentListItem::RenderHighlight`). Rows are not grouped under headers
//!   (`headerListItem` is never added);
//! - a left press on a row with an altar and n > 0 plays `addItemSound` and puts the reagent in its box
//!   (`AltarReagentListItem::OnClick` → `AltarWindow::AddReagentToBox`): an exclusive formula's into the
//!   centre when empty, an inclusive one's into the first empty of the five; with no free box the UI notification
//!   `tagAscensionAltarFull` and the error sound;
//! - a left or right press on a filled box with an altar empties it (`AltarWindow::HandleMouseEvent`);
//! - a filled box shows its reagent (an `enchanteritembox`, drawn as the shrine's), its animated ring glow
//!   (`altarBox<n>Glow`, a `bitmapanimated` sheet, additive) and, when its formula takes more than one reagent, the
//!   quantity `tagCraftingQuantity` (held / needed; `tagCraftingLargeQuantity` over 99,
//!   `AltarWindow::UpdateBoxQuantity`);
//! - the activate button is enabled when a box is filled and the window has an altar (`AltarWindow::CloseIfFar`);
//!   it sends the boxes (`SendAltarOfferCmd`, `AltarWindow::OnActivate`); cancel and close hide the window. The window
//!   has no rollover on the activate button (`confirmButtonRollover` is not a string in the exe);
//! - opened by the Altar Window key (no altar), the boxes show the activated offering and take no clicks; the
//!   activate button is disabled (`AltarWindow::Show`);
//! - the loot bonus list (`AltarWindow::RebuildLootBonusList`): unless Ultimate Veteran, the boxes' formulas'
//!   `reagentHeaderTag`s, sorted and unique; then each formula's reagent text
//!   (`AscendantAltarFormula::GetReagentText`, Game.dll):
//!   `reagentBaseTag` with `reagentBaseTagRef` (`…UltimateVeteran` on UV) at the formula's level − 1 (clamped to
//!   the list), nothing for a 0 value, the bare tag with no list;
//! - the monster bonus list (`AltarWindow::RebuildMonsterBonusList`): the formulas'
//!   `bossMonsterAttributePak`, `nonBossMonsterAttributePak`
//!   and `monsterAttributePak` merged at index level + 5 × (difficulty, +1 on UV), listed under
//!   `tagAscensionBossBonuses`, `tagAscensionMonsterBonuses` and no title;
//! - a row's rollover (`AltarReagentListItem::ShowRollover` / `AltarWindow::AddReagentRow`): the reagent
//!   name, its loot lines at level 1, `tagTooltipMonsterBonusHeader` and its monster lines, then (with an altar)
//!   `tagAscensionAltarAddReagent`; a filled box's: its reagent and the box's `rolloverTag`
//!   (`tagAscensionAltarRemoveReagent`).
//!
//! PORT: the attribute lines are `gd_sim::items::tooltip::attribute_line` per merged attribute (the engine's
//! `AttributePak::GetText` is not reversed) with the section title before them; the rollovers go
//! through `tip::TipCanvas` with the style's delay ([`tooltip`]); the scroll windows scroll by the mouse wheel
//! with no bar, in place ([`live`], the engine's scroll window class); hovering moves the row highlights and
//! button looks in place; `removeItemSound` plays when a box is emptied; a box whose reagent count is short
//! tints its quantity `insufficientMaterialColor`.
use super::contains;
use crate::state::ascendant::AltarMenu;
use crate::state::inventory::UiCapture;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::save::item::Item;
use gd_formats::text::TagTable;
use gd_formats::ui::{Rect as URect, TextAlign, TextBox, WindowDef};
use gd_formats::vfs::Database;
use gd_game::ascendant::{AltarAnswer, AltarOffer};
use gd_game_core::ascendant::{Offering, formula_levels, formula_of, reagent_quantity};
use gd_game_core::data::GameData;
use gd_game_core::inventory::Inventory;
use gd_sim::skill_tooltip::TextLine;

pub const RECORD: &str = "records/ui/ascensionaltar/altar_table.dbr";
/// The hovered row's highlight (`AltarWindow::AltarWindow`).
const ROW_HIGHLIGHT: &str = "ui/inventor/crafting_recipeselectionhighlight.tex";
/// The offering boxes' record fields (box, glow, quantity), centre first.
const BOXES: [(&str, &str, &str); 6] = [
    (
        "altarBoxCenter",
        "altarBoxCenterGlow",
        "altarBoxCenterQuantity",
    ),
    ("altarBox1", "altarBox1Glow", "altarBox1Quantity"),
    ("altarBox2", "altarBox2Glow", "altarBox2Quantity"),
    ("altarBox3", "altarBox3Glow", "altarBox3Quantity"),
    ("altarBox4", "altarBox4Glow", "altarBox4Quantity"),
    ("altarBox5", "altarBox5Glow", "altarBox5Quantity"),
];

pub struct AltarUiPlugin;

impl Plugin for AltarUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AltarLayout>();
        super::window::add_window(
            app,
            build,
            clicks,
            |m: Res<AltarMenu>, l: Res<AltarLayout>| m.open || l.window.is_some(),
        );
        app.add_systems(
            Update,
            (
                animate_glows.run_if(|l: Res<AltarLayout>| l.window.is_some()),
                (live, tooltip).after(build),
            )
                .in_set(super::UiPlaySet),
        );
        #[cfg(feature = "dev")]
        app.add_systems(Update, open_hook.in_set(super::UiPlaySet));
    }
}

/// A button of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Activate,
    Cancel,
    Close,
    Tab(u8),
}

/// What the cursor is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Button(Part),
    /// A list row (index into [`AltarLayout::rows`]).
    Row(usize),
    /// A box: 0 = centre, 1..=5 the others.
    Box(usize),
}

/// The built window (physical px). Empty while closed.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct AltarLayout {
    pub window: Option<URect>,
    /// Enabled buttons.
    pub buttons: Vec<(URect, Part)>,
    /// Visible list rows and their reagents.
    pub rows: Vec<(URect, String)>,
    /// Filled boxes (0 = centre).
    pub boxes: Vec<(URect, usize)>,
    /// The scroll windows (list, loot bonuses, monster bonuses): clip rect and the largest scroll (window px).
    pub panes: [Option<(URect, f32)>; 3],
    /// The UI scale (the rows' scroll shift).
    pub s: f32,
}

impl AltarLayout {
    fn hit(&self, p: Vec2, scroll: &[f32; 3]) -> Option<Hit> {
        if let Some(b) = self.buttons.iter().find(|(r, _)| contains(r, p)) {
            return Some(Hit::Button(b.1));
        }
        // The rows are kept at scroll 0 (the pane moves them); shift the pointer's row test the same way,
        // and only inside the list pane (rows scrolled out of view are clipped, not hit).
        let dy = scroll[0] * self.s;
        if self.panes[0].is_some_and(|(c, _)| contains(&c, p))
            && let Some(i) = self
                .rows
                .iter()
                .position(|(r, _)| contains(&URect::new(r.x, r.y - dy, r.w, r.h), p))
        {
            return Some(Hit::Row(i));
        }
        self.boxes
            .iter()
            .find(|(r, _)| contains(r, p))
            .map(|b| Hit::Box(b.1))
    }
}

/// A non-selected tab's hidden `bitmapNameInFocus` overlay ([`live`] shows the hovered one's).
#[derive(Component)]
struct TabFocus(u8);

/// A list row's hidden hover highlight ([`live`] shows the hovered row's; it pans with the list).
#[derive(Component)]
struct RowHover(usize);

/// A confirm / cancel / close button quad: its part and enabled state ([`live`] swaps its look).
#[derive(Component)]
struct AltarButton(Part, bool);

/// A scroll window's inner node ([`live`] moves it by `AltarMenu::scroll[pane]`).
#[derive(Component)]
struct AltarPane {
    pane: usize,
    clip_y: f32,
    s: f32,
    inv: f32,
}

/// The reagents of a tab's list (module doc): tab 0 the inclusive formulas', tab 1 the exclusive ones'.
pub fn list_reagents(db: &Database, tab: u8) -> Vec<String> {
    let Some(g) = db.get(gd_sim::ascendant::GAME_ASCENDANT) else {
        return Vec::new();
    };
    let field = if tab == 0 {
        "altarInclusiveRecipes"
    } else {
        "altarExclusiveRecipes"
    };
    g.strs(field)
        .into_iter()
        .filter_map(|f| db.get(f)?.str("reagentBaseBaseName").map(str::to_owned))
        .collect()
}

/// A row's count: held minus what the boxes already take (`AltarWindow::RebuildReagentList` /
/// `AltarReagentListItem::OnClick`).
pub fn remaining(db: &Database, held: u32, offering: &Offering, reagent: &str) -> u32 {
    let placed = offering
        .placed()
        .iter()
        .filter(|r| r.eq_ignore_ascii_case(reagent))
        .count() as u32;
    held.saturating_sub(placed * reagent_quantity(db, reagent))
}

/// The box slot (0 = centre, 1..=5) of a filled box's reagent.
fn slot(o: &Offering, i: usize) -> Option<&str> {
    if i == 0 {
        o.center.as_deref()
    } else {
        o.boxes.get(i - 1)?.as_deref()
    }
}

fn slot_clear(o: &mut Offering, i: usize) {
    if i == 0 {
        o.center = None;
    } else if let Some(b) = o.boxes.get_mut(i - 1) {
        *b = None;
    }
}

fn tag(tags: &TagTable, t: &str) -> String {
    tags.get(t).unwrap_or(t).to_owned()
}

/// The loot bonus lines of `levels` (module doc; `uv`: Ultimate Veteran).
pub fn loot_lines(
    db: &Database,
    tags: &TagTable,
    uv: bool,
    levels: &[(String, u32)],
) -> Vec<String> {
    let mut out = Vec::new();
    if !uv {
        let mut heads: Vec<&str> = levels
            .iter()
            .filter_map(|(f, _)| db.get(f)?.str("reagentHeaderTag"))
            .filter(|t| !t.is_empty())
            .collect();
        heads.sort_unstable();
        heads.dedup();
        out.extend(heads.into_iter().map(|t| tag(tags, t)));
    }
    for (f, level) in levels {
        let Some(r) = db.get(f) else { continue };
        let Some(t) = r.str("reagentBaseTag").filter(|t| !t.is_empty()) else {
            continue;
        };
        let refs = r.f32s(if uv {
            "reagentBaseTagRefUltimateVeteran"
        } else {
            "reagentBaseTagRef"
        });
        let text = tags.get(t).unwrap_or(t);
        if refs.is_empty() {
            out.push(gd_sim::skill_tooltip::format(text, &[]));
            continue;
        }
        let v = refs[(*level as usize).saturating_sub(1).min(refs.len() - 1)];
        if v != 0.0 {
            out.push(gd_sim::skill_tooltip::format(
                text,
                &[gd_sim::skill_tooltip::Arg::Num(v)],
            ));
        }
    }
    out
}

/// The monster bonus lines of `levels` (module doc) at the session's `mode`.
pub fn monster_lines(
    db: &Database,
    tags: &TagTable,
    mode: gd_rules::mode::Mode,
    levels: &[(String, u32)],
) -> Vec<String> {
    use gd_sim::items::resolve::{Attr, Source, record_attrs_at};
    let base = 5 * mode.ascendant_index();
    let mut out = Vec::new();
    for (field, title) in [
        ("bossMonsterAttributePak", Some("tagAscensionBossBonuses")),
        (
            "nonBossMonsterAttributePak",
            Some("tagAscensionMonsterBonuses"),
        ),
        ("monsterAttributePak", None),
    ] {
        // `AttributePak::MergeAtLevel`: the same attribute's values add up.
        let mut merged: Vec<Attr> = Vec::new();
        for (f, level) in levels {
            let Some(pak) = db
                .get(f)
                .and_then(|r| r.str(field))
                .filter(|p| !p.is_empty())
                .and_then(|p| db.get(p))
            else {
                continue;
            };
            for a in record_attrs_at(
                pak,
                Source::Base,
                base + (*level as usize).saturating_sub(1),
            ) {
                match merged.iter_mut().find(|m| m.class == a.class) {
                    Some(m) => {
                        for (k, v) in a.values {
                            match m.values.iter_mut().find(|x| x.0 == k) {
                                Some(x) => x.1 += v,
                                None => m.values.push((k, v)),
                            }
                        }
                    }
                    None => merged.push(a),
                }
            }
        }
        let lines: Vec<String> = merged
            .iter()
            .filter_map(|a| gd_sim::items::tooltip::attribute_line(tags, a))
            .collect();
        if lines.is_empty() {
            continue;
        }
        if let Some(t) = title {
            out.push(tag(tags, t));
        }
        out.extend(lines);
    }
    out
}

/// The display name of a reagent record.
fn reagent_name(db: &Database, tags: &TagTable, reagent: &str) -> String {
    db.get(reagent)
        .and_then(|r| {
            r.str("description")
                .or(r.str("itemNameTag"))
                .map(str::to_owned)
        })
        .map(|t| gd_formats::text::strip_markup(tags.get(&t).unwrap_or(&t)))
        .unwrap_or_else(|| reagent.rsplit('/').next().unwrap_or("").to_owned())
}

fn reagent_item(rec: &str) -> Item {
    Item {
        base_name: rec.to_owned(),
        stack_count: 1,
        var1: 1,
        ..Default::default()
    }
}

#[derive(Component)]
struct AltarRoot;

/// An animated ring glow: the sheet node inside its clip node, moved to the shown frame.
#[derive(Component)]
struct Glow {
    /// Frame size (logical px).
    frame: Vec2,
    cols: u32,
    total: u32,
    fps: f32,
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    menu: Res<AltarMenu>,
    mode: Res<gd_game_core::session::GameMode>,
    inv: Res<Inventory>,
    roots: Query<Entity, With<AltarRoot>>,
    mut layout: ResMut<AltarLayout>,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<Option<(UVec3, Offering, Option<Entity>, u8, Vec<u32>)>>,
    // The formula reagents and their held counts at an inventory version.
    mut counts: Local<(Option<u64>, Vec<String>, Vec<u32>)>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        *built = None;
        *counts = Default::default();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let db = &data.db;
    if counts.0 != Some(inv.version) {
        let all = gd_game_core::ascendant::formula_reagents(db);
        counts.2 = all.iter().map(|r| inv.count_by_record(r)).collect();
        counts.1 = all;
        counts.0 = Some(inv.version);
    }
    let (all, held) = (&counts.1, &counts.2);
    // Hover and scroll are not in the signature: `live` moves the panes, the highlights and the button
    // looks, `tooltip` shows the hovered row's / box's rollover. Compared by reference, cloned on change.
    // The size carries the UI scale (an applied UI Scale rebuilds).
    let sk = size.extend(assets.scale(size).to_bits());
    let changed = !built.as_ref().is_some_and(|(s, offering, altar, tab, h)| {
        *s == sk
            && *offering == menu.offering
            && *altar == menu.altar
            && *tab == menu.tab
            && h == held
    });
    if changed {
        *built = Some((
            sk,
            menu.offering.clone(),
            menu.altar,
            menu.tab,
            held.clone(),
        ));
    }
    if !menu.open {
        if layout.window.is_some() {
            super::window::despawn_all(&mut commands, &roots);
            *layout = AltarLayout::default();
            *built = None;
        }
        return;
    }
    if size.x == 0 || !changed {
        return;
    }
    super::window::despawn_all(&mut commands, &roots);
    *layout = AltarLayout::default();
    let Some(rec) = db.get(RECORD) else { return };
    let list = list_reagents(db, menu.tab);
    let held_of = |r: &str| all.iter().position(|a| a == r).map_or(0, |i| held[i]);
    let at_altar = menu.altar.is_some();
    let s = assets.scale(size);
    let root = gd_ui_kit::spawn_root(&mut commands, AltarRoot, super::window::Z_WINDOW);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let tags = ui.fonts.tags.clone();
    let field = |k: &str| rec.str(k).unwrap_or("").to_string();
    let wr = WindowDef::from_record(rec).screen_rect(ui.screen.x, ui.screen.y, s);
    let o = Vec2::new(wr.x, wr.y);
    ui.sound_blocker(wr);
    let bitmap = |ui: &mut super::Ui, key: &str| {
        if let Some(b) = ui.record(&field(key))
            && let Some(path) = b.str("bitmapName")
        {
            let f = |k: &str| b.f32(k).unwrap_or(0.0);
            ui.bitmap(path, f("bitmapPositionX"), f("bitmapPositionY"), o);
        }
    };
    bitmap(&mut ui, "altarBitmap");
    // Title and the bonus titles (their `bitmapName` behind the text, grown by the padding).
    for key in [
        "altarTitle",
        "altarLootBonusTitle",
        "altarMonsterBonusTitle",
    ] {
        let Some(t) = ui.record(&field(key)) else {
            continue;
        };
        let tb = TextBox::from_record(&t);
        if let Some(path) = t.str("bitmapName")
            && let Some((h, _)) = ui.image(path)
        {
            let (px, py) = (
                t.f32("bitmapXPadding").unwrap_or(0.0),
                t.f32("bitmapYPadding").unwrap_or(0.0),
            );
            let r = URect::new(
                o.x + ui.sc(tb.rect.x - px),
                o.y + ui.sc(tb.rect.y - py),
                ui.sc(tb.rect.w + 2.0 * px),
                ui.sc(tb.rect.h + 2.0 * py),
            );
            ui.quad_stretch(h, r, Color::WHITE);
        }
        let text = tb.tag.as_deref().map(|t| tag(&tags, t)).unwrap_or_default();
        ui.text(&tb, o, &text);
    }
    // Tabs: a radio group, the shown one down.
    for (i, key) in ["reagentTabButton1", "reagentTabButton2"]
        .into_iter()
        .enumerate()
    {
        let Some(b) = ui.record(&field(key)) else {
            continue;
        };
        let tab = i as u8;
        let state = if menu.tab == tab {
            "bitmapNameDown"
        } else {
            "bitmapNameUp"
        };
        if let Some((h, sz)) = b.str(state).and_then(|p| ui.image(p)) {
            let r = URect::new(
                o.x + ui.sc(b.f32("bitmapPositionX").unwrap_or(0.0)),
                o.y + ui.sc(b.f32("bitmapPositionY").unwrap_or(0.0)),
                ui.sc(sz.x as f32),
                ui.sc(sz.y as f32),
            );
            ui.quad(h, r, None, Color::WHITE);
            if menu.tab != tab
                && let Some((fh, _)) = b.str("bitmapNameInFocus").and_then(|p| ui.image(p))
            {
                let e = ui.quad(fh, r, None, Color::WHITE);
                ui.commands
                    .entity(e)
                    .insert((TabFocus(tab), Visibility::Hidden));
            }
            ui.record_button_sound(r, &b);
            layout.buttons.push((r, Part::Tab(tab)));
        }
    }
    // The reagent list.
    let pane = |ui: &mut super::Ui, key: &str| -> Option<URect> {
        let w = ui.record(&field(key))?;
        let f = |k: &str| w.f32(k).unwrap_or(0.0);
        Some(URect::new(
            o.x + ui.sc(f("positionX")),
            o.y + ui.sc(f("positionY")),
            ui.sc(f("width")),
            ui.sc(f("height")),
        ))
    };
    if let Some(clip) = pane(&mut ui, "altarScrollWindow")
        && let (Some(bg), Some(icon), Some(name)) = (
            ui.record(&field("reagentListItem")),
            ui.record(&field("reagentListIcon")),
            ui.record(&field("reagentListNameText")),
        )
        && let Some((bg_img, bg_sz)) = bg.str("bitmapName").and_then(|p| ui.image(p))
    {
        let icon_w = icon
            .str("bitmapName")
            .and_then(|p| ui.image(p))
            .map_or(32.0, |(_, sz)| sz.x as f32);
        let ntb = TextBox::from_record(&name);
        let disabled = field("altarListDisabledFont");
        let step = bg_sz.y as f32 + 2.0;
        let content = list.len() as f32 * step;
        let max = (content - clip.h / s).max(0.0);
        layout.panes[0] = Some((clip, max));
        let scroll = menu.scroll[0].min(max);
        let outer = ui.root;
        let scroll_px = ui.sc(scroll);
        let inner = super::window::pane(&mut ui, clip, scroll_px);
        ui.commands.entity(inner).insert(AltarPane {
            pane: 0,
            clip_y: clip.y,
            s: ui.s,
            inv: ui.inv,
        });
        ui.root = inner;
        for (i, r) in list.iter().enumerate() {
            let y = 1.0 + i as f32 * step;
            let row = URect::new(
                clip.x + ui.sc(bg.f32("bitmapPositionX").unwrap_or(0.0)),
                clip.y + ui.sc(y + bg.f32("bitmapPositionY").unwrap_or(0.0)),
                ui.sc(bg_sz.x as f32),
                ui.sc(bg_sz.y as f32),
            );
            ui.quad(bg_img.clone(), row, None, Color::WHITE);
            let ro = Vec2::new(clip.x, row.y);
            // The reagent's bitmap at the icon's width, centred on the row.
            if let Some(info) = inv.info(&reagent_item(r))
                && let Some((h, isz)) = ui.image(&info.resolved.bitmap)
            {
                let w = ui.sc(icon_w);
                let hgt = w * isz.y as f32 / isz.x.max(1) as f32;
                let d = URect::new(
                    ro.x + ui.sc(icon.f32("bitmapPositionX").unwrap_or(0.0)),
                    row.y + (row.h - hgt) * 0.5,
                    w,
                    hgt,
                );
                ui.quad_stretch(h, d, Color::WHITE);
            }
            let n = remaining(db, held_of(r), &menu.offering, r);
            let count = if n > 99 {
                "99+".to_owned()
            } else {
                n.to_string()
            };
            let text = format!("[{count}] {}", reagent_name(db, &tags, r));
            let style = if n == 0 && !disabled.is_empty() {
                disabled.clone()
            } else {
                ntb.style.clone().unwrap_or_default()
            };
            ui.text_with_style(&ntb, &style, ro, &text);
            if let Some((h, _)) = ui.image(ROW_HIGHLIGHT) {
                let g = URect::new(row.x - 1.0, row.y - 1.0, row.w + 2.0, row.h + 2.0);
                let e = ui.quad_stretch(h, g, Color::WHITE);
                ui.commands
                    .entity(e)
                    .insert((RowHover(layout.rows.len()), Visibility::Hidden));
            }
            layout.rows.push((row, r.clone()));
        }
        ui.root = outer;
    }
    // Box glows (filled boxes), then the boxes and their quantities.
    for (i, (_, glow, _)) in BOXES.iter().enumerate() {
        if slot(&menu.offering, i).is_none() {
            continue;
        }
        let Some(g) = ui.record(&field(glow)) else {
            continue;
        };
        let Some((h, sz)) = g.str("bitmapName").and_then(|p| ui.image(p)) else {
            continue;
        };
        let f = |k: &str| g.f32(k).unwrap_or(0.0);
        let (cols, rows) = (f("xFrames").max(1.0), f("yFrames").max(1.0));
        let fr = Vec2::new(sz.x as f32 / cols, sz.y as f32 / rows);
        let clip = URect::new(
            o.x + ui.sc(f("bitmapPositionX")),
            o.y + ui.sc(f("bitmapPositionY")),
            ui.sc(fr.x),
            ui.sc(fr.y),
        );
        let mut node = ui.node(clip);
        node.overflow = Overflow::clip();
        let holder = ui.commands.spawn(node).id();
        ui.commands.entity(ui.root).add_child(holder);
        let inv_px = ui.inv;
        let sheet = ui
            .commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui.sc(sz.x as f32) * inv_px),
                    height: Val::Px(ui.sc(sz.y as f32) * inv_px),
                    ..default()
                },
                Glow {
                    frame: Vec2::new(ui.sc(fr.x), ui.sc(fr.y)) * inv_px,
                    cols: cols as u32,
                    total: f("totalFrames").max(1.0) as u32,
                    fps: f("framesPerSecond").max(1.0),
                },
                ChildOf(holder),
            ))
            .id();
        super::additive::queue_additive(ui.commands, sheet, h, Color::WHITE);
    }
    let insufficient = Color::srgb(
        rec.f32("insufficientMaterialColor.r").unwrap_or(0.75),
        rec.f32("insufficientMaterialColor.g").unwrap_or(0.0),
        rec.f32("insufficientMaterialColor.b").unwrap_or(0.0),
    );
    for (i, (bx, _, qty)) in BOXES.iter().enumerate() {
        let Some(r) = slot(&menu.offering, i) else {
            continue;
        };
        let Some(b) = ui.record(&field(bx)) else {
            continue;
        };
        let f = |k: &str| b.f32(k).unwrap_or(0.0);
        let br = URect::new(
            o.x + ui.sc(f("itemX")),
            o.y + ui.sc(f("itemY")),
            ui.sc(f("itemXSize")),
            ui.sc(f("itemYSize")),
        );
        let it = reagent_item(r);
        if !b.bool("disableItemBackground") {
            super::shrine::rarity_plate(&mut ui, &inv, &it, br);
        }
        super::character::item_icon(&mut ui, &inv, &it, br);
        let need = reagent_quantity(db, r);
        if need > 1
            && let Some(q) = ui.record(&field(qty))
        {
            let have = held_of(r);
            let text = if have <= 99 {
                gd_sim::skill_tooltip::format(
                    tags.get("tagCraftingQuantity").unwrap_or("{%d0}/{%d1}"),
                    &[
                        gd_sim::skill_tooltip::Arg::Num(have as f32),
                        gd_sim::skill_tooltip::Arg::Num(need as f32),
                    ],
                )
            } else {
                gd_sim::skill_tooltip::format(
                    tags.get("tagCraftingLargeQuantity").unwrap_or("{%d0}"),
                    &[gd_sim::skill_tooltip::Arg::Num(need as f32)],
                )
            };
            let qtb = TextBox::from_record(&q);
            if let Some(style) = qtb.style.as_deref() {
                let e = ui.text_with_style(&qtb, style, o, &text);
                if have < need
                    && let Some(e) = e
                {
                    ui.commands.entity(e).queue(move |mut em: EntityWorldMut| {
                        if let Some(mut t) = em.get_mut::<TextColor>() {
                            t.0 = insufficient;
                        }
                    });
                }
            }
        }
        if at_altar {
            layout.boxes.push((br, i));
        }
    }
    // The bonus lists.
    let levels = formula_levels(db, &menu.offering.placed());
    let lists = [
        (
            "altarLootBonusScrollWindow",
            "altarLootBonusBox",
            loot_lines(db, &tags, mode.0.ultimate_veteran(), &levels),
        ),
        (
            "altarMonsterBonusScrollWindow",
            "altarMonsterBonusBox",
            monster_lines(db, &tags, mode.0, &levels),
        ),
    ];
    for (k, (win_key, box_key, lines)) in lists.into_iter().enumerate() {
        let (Some(clip), Some(tb)) = (pane(&mut ui, win_key), ui.record(&field(box_key))) else {
            continue;
        };
        if lines.is_empty() {
            continue;
        }
        let Some(style) = tb.str("style").map(str::to_owned) else {
            continue;
        };
        let wrapped: Vec<String> = lines
            .iter()
            .flat_map(|l| ui.wrap_lines(&style, clip.w, l))
            .collect();
        let lh = ui.line_height(&style);
        let content = wrapped.len() as f32 * lh;
        let max = ((content - clip.h) / s).max(0.0);
        layout.panes[k + 1] = Some((clip, max));
        let scroll = ui.sc(menu.scroll[k + 1].min(max));
        let a = tb.f32("backgroundColorAlpha").unwrap_or(0.0);
        if a > 0.0 {
            let h = content.min(clip.h);
            ui.solid(
                URect::new(clip.x, clip.y, clip.w, h),
                Color::srgba(
                    tb.f32("backgroundColorRed").unwrap_or(0.0),
                    tb.f32("backgroundColorGreen").unwrap_or(0.0),
                    tb.f32("backgroundColorBlue").unwrap_or(0.0),
                    a,
                ),
            );
        }
        let outer = ui.root;
        let inner = super::window::pane(&mut ui, clip, scroll);
        ui.commands.entity(inner).insert(AltarPane {
            pane: k + 1,
            clip_y: clip.y,
            s: ui.s,
            inv: ui.inv,
        });
        ui.root = inner;
        for (j, l) in wrapped.iter().enumerate() {
            let y = clip.y + j as f32 * lh;
            super::widgets::label(
                &mut ui,
                URect::new(clip.x, y, clip.w, lh),
                &style,
                TextAlign::Start,
                l,
            );
        }
        ui.root = outer;
    }
    // Buttons.
    let ready = at_altar && !menu.offering.placed().is_empty();
    for (key, part, enabled) in [
        ("confirmButton", Part::Activate, ready),
        ("cancelButton", Part::Cancel, true),
        ("closeButton", Part::Close, true),
    ] {
        let Some(b) = ui.record(&field(key)) else {
            continue;
        };
        let text = b.str("textTag").map(|t| tag(&tags, t));
        if let Some((r, e)) = super::window::static_button_live(
            &mut ui,
            &b,
            o,
            enabled,
            text.as_deref()
                .map_or(super::widgets::Label::None, super::widgets::Label::Text),
        ) {
            ui.commands.entity(e).insert(AltarButton(part, enabled));
            if enabled {
                ui.record_button_sound(r, &b);
                layout.buttons.push((r, part));
            }
        }
    }
    layout.s = ui.s;
    layout.window = Some(wr);
}

/// Moves each glow sheet to its frame (`bitmapanimated`: `framesPerSecond`, `totalFrames`, `xFrames` per row).
fn animate_glows(time: Res<Time<Real>>, mut glows: Query<(&Glow, &mut Node)>) {
    let t = time.elapsed_secs();
    for (g, mut n) in &mut glows {
        let i = (t * g.fps) as u32 % g.total;
        let (left, top) = (
            Val::Px(-((i % g.cols) as f32) * g.frame.x),
            Val::Px(-((i / g.cols) as f32) * g.frame.y),
        );
        if n.left != left || n.top != top {
            n.left = left;
            n.top = top;
        }
    }
}

/// `GD_UI_OPEN=altar`: opens the window as the Altar Window key does, once (after the `GD_ALTAR_OFFER` activation
/// when that hook is set).
#[cfg(feature = "dev")]
fn open_hook(
    hooks: Res<gd_game_core::hooks::Hooks>,
    altar: Res<gd_game_core::ascendant::AltarState>,
    mut menu: ResMut<AltarMenu>,
    mut done: Local<bool>,
    mut fresh: gd_game_core::session::NewSession,
) {
    if fresh.begun() {
        *done = false;
    }
    if *done
        || hooks.ui.open.as_deref() != Some("altar")
        || menu.open
        || (hooks.items.altar_offer.is_some() && !altar.activated)
    {
        return;
    }
    *done = true;
    menu.show(None, altar.reagents.clone());
}

/// Clicks on the window (module doc), the wheel over a scroll window; the `GD_ALTAR_OFFER` hook; the answer to
/// an offer (accepted: the window hides). Esc closes it through the shared dispatcher (`ui::esc_dispatcher`).
#[allow(clippy::too_many_arguments)]
pub fn clicks(
    #[cfg(feature = "dev")] (hooks, mut scripted, mut fresh): (
        Res<gd_game_core::hooks::Hooks>,
        Local<bool>,
        gd_game_core::session::NewSession,
    ),
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<AltarLayout>,
    mut menu: ResMut<AltarMenu>,
    inv: Res<Inventory>,
    data: Res<GameData>,
    mut capture: ResMut<UiCapture>,
    (mut offers, mut answers): (MessageWriter<AltarOffer>, MessageReader<AltarAnswer>),
    (mut sound, mut notify): (
        MessageWriter<gd_game_core::visuals::PlaySound>,
        MessageWriter<gd_game_core::story::Notify>,
    ),
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    #[cfg(feature = "dev")]
    if fresh.begun() {
        *scripted = false;
    }
    let db = &data.db;
    for a in answers.read() {
        match &a.0 {
            Ok(_) => menu.open = false,
            Err(why) => info!("ascendant: {why}"),
        }
    }
    if !menu.open {
        wheel.clear();
        return;
    }
    // A shrinking list (a reagent removed) can leave a pane scrolled past its new end.
    for i in 0..3 {
        let max = layout.panes[i].map_or(0.0, |x| x.1);
        if menu.scroll[i] > max {
            menu.scroll[i] = max;
        }
    }
    let rec = db.get(RECORD);
    let play = |sound: &mut MessageWriter<gd_game_core::visuals::PlaySound>, key: &str| {
        if let Some(pak) = rec.and_then(|r| r.str(key)).filter(|p| !p.is_empty()) {
            sound.write(gd_game_core::visuals::PlaySound {
                pak: pak.to_owned(),
                at: None,
            });
        }
    };
    // `GD_ALTAR_OFFER`: fill the boxes in order and activate, once.
    #[cfg(feature = "dev")]
    if menu.altar.is_some()
        && !*scripted
        && let Some(list) = hooks.items.altar_offer.clone()
    {
        *scripted = true;
        let mut provide = Vec::new();
        for r in list.split(';').filter(|r| !r.is_empty()) {
            provide.push(r.to_owned());
            menu.offering.place(db, r);
        }
        offers.write(AltarOffer {
            placed: menu.offering.placed(),
            provide,
        });
        return;
    }
    // The game menu, its confirmation or Options sits over the window: it takes the wheel too.
    if pause.menu {
        wheel.clear();
        return;
    }
    // Esc closes the window through the shared dispatcher (`ui::esc_dispatcher`).
    let Some(wr) = layout.window else {
        wheel.clear();
        return;
    };
    let Ok(win) = window.single() else { return };
    let Some(p) = super::cursor_px(win) else {
        return;
    };
    if !contains(&wr, p) {
        wheel.clear();
        return;
    }
    capture.0 = true;
    for w in wheel.read() {
        let Some(i) = (0..3).find(|&i| layout.panes[i].is_some_and(|(r, _)| contains(&r, p)))
        else {
            continue;
        };
        let step = match w.unit {
            bevy::input::mouse::MouseScrollUnit::Line => w.y * 20.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => w.y,
        };
        let max = layout.panes[i].map_or(0.0, |x| x.1);
        let v = (menu.scroll[i] - step).clamp(0.0, max);
        if v != menu.scroll[i] {
            menu.scroll[i] = v;
        }
    }
    let left = buttons.just_pressed(MouseButton::Left);
    let right = buttons.just_pressed(MouseButton::Right);
    if !left && !right {
        return;
    }
    match layout.hit(p, &menu.scroll) {
        Some(Hit::Button(part)) if left => match part {
            Part::Activate => {
                offers.write(AltarOffer {
                    placed: menu.offering.placed(),
                    provide: Vec::new(),
                });
            }
            Part::Cancel | Part::Close => menu.open = false,
            Part::Tab(t) => {
                if menu.tab != t {
                    menu.tab = t;
                    menu.scroll[0] = 0.0;
                }
            }
        },
        Some(Hit::Row(i)) if left && menu.altar.is_some() => {
            let Some(r) = layout.rows.get(i).map(|r| r.1.clone()) else {
                return;
            };
            if remaining(db, inv.count_by_record(&r), &menu.offering, &r) == 0 {
                return;
            }
            play(&mut sound, "addItemSound");
            if !menu.offering.place(db, &r) {
                notify.write(gd_game_core::story::Notify("tagAscensionAltarFull".into()));
                if let Some(pak) = gd_game_core::stash::error_sound(&data) {
                    sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
                }
            }
        }
        Some(Hit::Box(i)) if menu.altar.is_some() => {
            slot_clear(&mut menu.offering, i);
            play(&mut sound, "removeItemSound");
        }
        _ => {}
    }
}

/// The window's hover looks and scroll in place (no rebuild): the tab focus overlays, the row
/// highlights, the button bitmaps and the three scroll panes.
#[allow(clippy::too_many_arguments)]
fn live(
    window: Query<&Window, With<PrimaryWindow>>,
    menu: Res<AltarMenu>,
    layout: Res<AltarLayout>,
    mut tabs: Query<(&TabFocus, &mut Visibility), Without<RowHover>>,
    mut rows: Query<(&RowHover, &mut Visibility), Without<TabFocus>>,
    mut buttons: Query<(&AltarButton, &super::window::ButtonLook, &mut ImageNode)>,
    pause: Res<gd_game_core::pause::GamePause>,
    mut panes: Query<(&AltarPane, &mut Node)>,
) {
    if layout.window.is_none() {
        return;
    }
    let Ok(win) = window.single() else { return };
    let hover = super::cursor_px(win)
        .filter(|_| !pause.menu)
        .and_then(|p| layout.hit(p, &menu.scroll));
    for (t, mut vis) in &mut tabs {
        vis.set_if_neq(if hover == Some(Hit::Button(Part::Tab(t.0))) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (r, mut vis) in &mut rows {
        vis.set_if_neq(if hover == Some(Hit::Row(r.0)) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (b, l, mut img) in &mut buttons {
        super::window::set_button_look(l, &mut img, b.1, hover == Some(Hit::Button(b.0)));
    }
    for (p, mut node) in &mut panes {
        let top = Val::Px(-(p.clip_y + menu.scroll[p.pane] * p.s) * p.inv);
        if node.top != top {
            node.top = top;
        }
    }
}

/// The hovered row's / box's rollover (module doc) through the shared `TipCanvas` with the style's
/// delay — not drawn into the window build.
#[allow(clippy::too_many_arguments)]
fn tooltip(
    window: Query<&Window, With<PrimaryWindow>>,
    menu: Res<AltarMenu>,
    mode: Res<gd_game_core::session::GameMode>,
    layout: Res<AltarLayout>,
    data: Res<GameData>,
    mut canvas: super::tip::TipCanvas,
    mut shown: Local<Option<(Option<Hit>, Offering, u8, bool, bool)>>,
    mut fresh: gd_game_core::session::NewSession,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    if fresh.begun() {
        *shown = None;
    }
    let Ok(win) = window.single() else { return };
    if !menu.open {
        if shown.is_some() {
            *shown = None;
            canvas.clear(super::tip::TipOwner::Altar);
        }
        return;
    }
    let hover = super::cursor_px(win)
        .filter(|_| !pause.menu)
        .and_then(|p| layout.hit(p, &menu.scroll));
    let uv = mode.0.ultimate_veteran();
    let same = shown.as_ref().is_some_and(|(h, o, t, a, u)| {
        *h == hover
            && *o == menu.offering
            && *t == menu.tab
            && *a == menu.altar.is_some()
            && *u == uv
    });
    if same {
        return;
    }
    *shown = Some((
        hover,
        menu.offering.clone(),
        menu.tab,
        menu.altar.is_some(),
        uv,
    ));
    canvas.clear(super::tip::TipOwner::Altar);
    let Some(h) = hover else { return };
    let db = &data.db;
    let tags = &canvas.assets.fonts.tags;
    let at_altar = menu.altar.is_some();
    let line = |class: u8, text: String| TextLine {
        class,
        text,
        icon: None,
    };
    let tip: Vec<TextLine> = match h {
        Hit::Row(i) => layout.rows.get(i).map_or(Vec::new(), |(_, r)| {
            let one = formula_of(db, r)
                .map(|(f, _)| vec![(f, 1)])
                .unwrap_or_default();
            let mut v = vec![line(12, reagent_name(db, tags, r))];
            v.extend(
                loot_lines(db, tags, uv, &one)
                    .into_iter()
                    .map(|t| line(49, t)),
            );
            v.push(line(70, tag(tags, "tagTooltipMonsterBonusHeader")));
            v.extend(
                monster_lines(db, tags, mode.0, &one)
                    .into_iter()
                    .map(|t| line(71, t)),
            );
            if at_altar {
                v.push(line(0, String::new()));
                v.push(line(51, tag(tags, "tagAscensionAltarAddReagent")));
            }
            v
        }),
        Hit::Box(i) => slot(&menu.offering, i).map_or(Vec::new(), |r| {
            let remove = db
                .get(RECORD)
                .and_then(|m| m.str(BOXES[i].0))
                .and_then(|p| db.get(p))
                .and_then(|b| b.str("rolloverTag").map(|t| tag(tags, t)))
                .unwrap_or_default();
            vec![
                line(12, reagent_name(db, tags, r)),
                line(0, String::new()),
                line(51, remove),
            ]
        }),
        _ => Vec::new(),
    };
    if tip.is_empty() {
        return;
    }
    let Some(p) = super::cursor_px(win) else {
        return;
    };
    canvas.show(
        super::tip::TipOwner::Altar,
        &tip,
        super::tip::Blank::DefaultLine,
        |w, h, screen, gap| {
            (
                (p.x + gap).min(screen.x - w).max(0.0),
                (p.y + gap).min(screen.y - h).max(0.0),
            )
        },
    );
}
