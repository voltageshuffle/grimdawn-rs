//! Merchant and stash windows (logic: `trade.rs`). Vendor: the market window
//! record's background, close button and item grid; stash: the caravan
//! window's background, the stash grid of `caravan_stashwindow` and one tab
//! button per page (`privateScrollWindow` position, `tabSpacingY`); with the
//! toggle (`ToggleStashButton`) down, the transfer stash instead
//! (`TransferWindow`, `transferScrollWindow`, `transferTabButton`,
//! `TransferTitleBitmap`). Crafter: `inventor_mastertable`'s crafting tab
//! (`enchanterArtifactTab`): category buttons, the recipe list
//! (`craftingListBox` area, `craftingListBoxItem` rows), the selected
//! blueprint's product and reagent boxes with have/need counts, the cost and
//! the craft button. Illusionist: `transmuter_mastertable` (see
//! [`illusionist_window`]).
//!
//! Vendor tabs: an empty tab is drawn from its up bitmap tinted 0.6 grey
//! and takes no clicks (window update `sub_1402731E0` sets the
//! button's disabled flags +641 / +644 while its sack is empty; `ButtonStatic`
//! draw `sub_140123C50`: disabled + dim with no disabled bitmap → the up
//! bitmap in colour 0.6, 0.6, 0.6, 1; PORT: the down bitmap's case, flag
//! +648, is not read). Search (`trade.rs` [`crate::trade::TradeSearch`];
//! draw `sub_140272630`, grid draw `sub_1401FB740`): the edit box
//! (`vendor_searchbox.dbr`: its `Style` text left-aligned, vertically
//! centred, a 1 px white caret after it while focused, blinking every 600
//! ms, editbox draw `sub_140193800` / update `sub_140194760`); the clear
//! button while the text is not empty; with a query, over the shown page's
//! items `ui/character/itemsearchborder.tex` stretched over each match in
//! 0.918, 0.553, 0.118, 0.75 (grid constructor `sub_1401FB320`) and black
//! at 0.75 over the rest; every other tab with a match gets a rect 1 px
//! inside its button in 0.73, 0.68, 0.43 whose alpha runs 0.05 → 0.325 → 0.05
//! every 2 s (`t = (ms % 2000) × 0.0005 × 0.55 + 0.05`, mirrored above
//! 0.325; [`animate_search`], on the real clock: the pulse and the caret keep
//! going while the game is paused).
//! The caravan window's `searchBox` / `clearSearchButton`
//! (`caravan_searchbox.dbr`) search the shown stash's pages the same way
//! (`sub_140136E00` draw, `sub_140133A10` grid split); the crafting
//! panel's (`crafting_table.dbr`, `crafting_searchbox.dbr`) filters its recipe
//! list instead (`crafting.rs`, draw `sub_14019C410`).
//!
//! Sounds: every clickable button and tab (market / stash / reagent / inventor tabs, buy tab, toggle, sort, clear
//! search, action buttons, illusionist pages / swap / close) registers its `soundNameDown` (`ui/widgets.rs`).
use super::Ui;
use crate::crafting::CraftList;
use crate::inventory::Inventory;
use crate::trade::{
    Button, Markets, Panel, STASH_W, Stash, Trade, TradeLayout, TradeOpen, VENDOR_W,
};
use bevy::prelude::*;
use gd_formats::save::item::Item;
use gd_formats::ui::{Rect as URect, Widget, WindowDef};

const VENDOR: &str = "records/ui/vendors/vendor_mastertable.dbr";
const FACTION_VENDOR: &str = "records/ui/vendors/faction_vendor_mastertable.dbr";
const CARAVAN: &str = "records/ui/caravan/caravan_window.dbr";
const ASCENSION: &str = "records/ui/itemascension/itemascension_table.dbr";

pub fn build(
    ui: &mut Ui,
    trade: &Trade,
    markets: &Markets,
    stash: Option<&Stash>,
    craft: Option<&CraftList>,
    (gold, level): (u64, u32),
    inv: Option<&Inventory>,
) {
    let mut layout = TradeLayout::default();
    if let (Some(open), Some(inv)) = (&trade.open, inv) {
        match open {
            TradeOpen::Vendor { key, .. } => vendor(
                ui,
                markets.0.get(key).map(Vec::as_slice).unwrap_or_default(),
                trade,
                inv,
                &mut layout,
            ),
            TradeOpen::Stash { .. } => stash_window(
                ui,
                stash,
                trade.tab,
                trade.transfer,
                trade.reagents,
                &trade.npc_record,
                &trade.search,
                inv,
                &mut layout,
            ),
            TradeOpen::Crafter { .. } => {
                if let (Some(list), Some(stash)) = (craft, stash) {
                    crafter_window(ui, list, trade, stash, gold, level, inv, &mut layout);
                }
            }
            TradeOpen::Inventor { .. } => {
                if let Some(stash) = stash {
                    inventor_window(ui, trade, stash, gold, level, inv, &mut layout);
                }
            }
            TradeOpen::Ascension { .. } => {
                if let Some(stash) = stash {
                    ascension_window(ui, trade, stash, gold, inv, &mut layout);
                }
            }
            TradeOpen::Illusionist { .. } => {
                illusionist_window(ui, trade, (gold, level), inv, &mut layout)
            }
        }
        // The window takes every click over it: no HUD button behind it sounds or presses there.
        if let Some(w) = layout.window {
            ui.sound_blocker(w);
        }
        // The dialog box over the window (`crate::trade::Prompt`): drawn under its own `Z_MODAL` node (a child of
        // the group root, so it is shown / retired with it) so that its full-screen blocker (`confirm::frame`)
        // outranks every window below in `widgets::frontmost`.
        if let Some(prompt) = trade.prompt {
            let (tag, arg) = prompt.text();
            let text = super::floating::format_tag(
                ui.fonts.tag(tag),
                &arg.map(|a| a as i64).into_iter().collect::<Vec<_>>(),
            );
            let group = ui.root;
            ui.root = ui
                .commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    GlobalZIndex(super::window::Z_MODAL),
                    ChildOf(group),
                ))
                .id();
            match prompt {
                crate::trade::Prompt::BuyPage { .. } => {
                    layout.confirm = super::confirm::confirm_text(ui, &text, trade.confirm_look)
                }
                crate::trade::Prompt::Message(_) => {
                    layout.ok = super::confirm::ok_text(ui, &text, trade.confirm_look)
                }
            }
            ui.root = group;
        }
    }
    ui.commands.insert_resource(layout);
}

fn window(ui: &Ui, rec: &gd_formats::arz::Record) -> URect {
    let mut w = WindowDef::from_record(rec).screen_rect(ui.screen.x, ui.screen.y, ui.s);
    if let Some((x, _)) = ui.docked {
        w.x = x;
    }
    w
}

/// The open NPC window's record.
pub fn window_record(trade: &Trade) -> Option<&'static str> {
    Some(match trade.open.as_ref()? {
        TradeOpen::Vendor { .. } if trade.faction.is_some() => FACTION_VENDOR,
        TradeOpen::Vendor { .. } => VENDOR,
        TradeOpen::Stash { .. } => CARAVAN,
        TradeOpen::Crafter { .. } | TradeOpen::Inventor { .. } => crate::crafting::WINDOW,
        TradeOpen::Ascension { .. } => ASCENSION,
        TradeOpen::Illusionist { .. } => crate::illusion::WINDOW,
    })
}

fn draw_field(ui: &mut Ui, rec: &gd_formats::arz::Record, field: &str, o: Vec2) {
    let Some(path) = rec.str(field) else { return };
    match ui.widget(path) {
        Some(Widget::Bitmap { bitmap, x, y }) => {
            ui.bitmap(&bitmap, x, y, o);
        }
        Some(Widget::Button {
            up: Some(up), x, y, ..
        }) => {
            ui.bitmap(&up, x, y, o);
        }
        _ => {}
    }
}

/// The window's close button (a `field` button widget): drawn up, hit-tested as [`Button::Close`]
/// (`trade_clicks` sends `ServiceRequest::Close`).
fn close_field(
    ui: &mut Ui,
    rec: &gd_formats::arz::Record,
    field: &str,
    o: Vec2,
    layout: &mut TradeLayout,
) {
    let Some(path) = rec.str(field) else { return };
    if let Some(Widget::Button {
        up: Some(up), x, y, ..
    }) = ui.widget(path)
        && let Some((_, r)) = ui.bitmap(&up, x, y, o)
    {
        layout.buttons.push((Button::Close, r));
        ui.record_path_button_sound(r, path);
    }
}

fn grid_items(
    ui: &mut Ui,
    inv: &Inventory,
    sack: &gd_sim::items::inventory::Sack<gd_formats::save::item::Item>,
    r: URect,
    cell: f32,
) {
    for p in &sack.items {
        let cr = URect::new(
            r.x + p.x as f32 * cell,
            r.y + p.y as f32 * cell,
            p.size.w as f32 * cell,
            p.size.h as f32 * cell,
        );
        super::character::grid_item(ui, inv, &p.item, cr);
    }
}

fn vendor(
    ui: &mut Ui,
    pages: &[gd_sim::items::inventory::Sack<Item>],
    trade: &Trade,
    inv: &Inventory,
    layout: &mut TradeLayout,
) {
    let (tab, npc, faction) = (trade.tab, trade.npc_record.as_str(), trade.faction.as_ref());
    let Some(m) = ui.record(if faction.is_some() {
        FACTION_VENDOR
    } else {
        VENDOR
    }) else {
        return;
    };
    let w = window(ui, &m);
    let o = Vec2::new(w.x, w.y);
    layout.window = Some(w);
    draw_field(ui, &m, "marketBaseBitmap", o);
    close_field(ui, &m, "marketCloseButton", o, layout);
    npc_headings(ui, &m, npc, ("marketHeading1Text", "marketHeading2Text"), o);
    // The merchant's faction icon (`factionIcon` position, the faction
    // pack's `factionIcon` bitmap).
    if let Some(p) = m.str("factionIcon").and_then(|p| ui.record(p))
        && let Some(icon) = ui
            .data
            .db
            .get(npc)
            .and_then(|r| r.str("factions").map(str::to_owned))
            .and_then(|f| ui.data.db.get(&f))
            .and_then(|f| f.str("factionIcon").map(str::to_owned))
    {
        ui.bitmap(
            &icon,
            p.f32("bitmapPositionX").unwrap_or(0.0),
            p.f32("bitmapPositionY").unwrap_or(0.0),
            o,
        );
    }
    let search = &trade.search;
    let searching = !search.query().is_empty();
    // Category tabs (melee, ranged, armour, accessories, buyback).
    let mut tab_rects = Vec::new();
    for i in 0..gd_sim::items::market::VENDOR_TABS {
        let Some(b) = m
            .str(&format!("marketTab{}Button", i + 1))
            .and_then(|b| ui.record(b))
        else {
            continue;
        };
        // An empty tab: dimmed up bitmap, no clicks (module doc).
        let enabled = gd_sim::items::market::tab_enabled(pages, i);
        let key = if i == tab && enabled {
            "bitmapNameDown"
        } else {
            "bitmapNameUp"
        };
        let Some((h, size)) = b.str(key).map(str::to_owned).and_then(|t| ui.image(&t)) else {
            continue;
        };
        let r = URect::new(
            o.x + ui.sc(b.f32("bitmapPositionX").unwrap_or(0.0)),
            o.y + ui.sc(b.f32("bitmapPositionY").unwrap_or(0.0)),
            ui.sc(size.x as f32),
            ui.sc(size.y as f32),
        );
        ui.quad(
            h,
            r,
            None,
            if enabled {
                Color::WHITE
            } else {
                Color::srgba(0.6, 0.6, 0.6, 1.0)
            },
        );
        if enabled {
            layout.tabs.push((i, r));
            ui.record_button_sound(r, &b);
        }
        tab_rects.push((i, r));
        // Tiers the player's rank has not unlocked: the red lock plate over
        // the button, blended additively in white, so the button shows through
        // tinted red (market window draw `sub_140272630`: in faction
        // mode the `marketTab<n>Bitmap` widgets are drawn with the "additive"
        // style, colour 1,1,1,1). The tab still opens: a locked tier is
        // browsed, its purchases refused (`gd_game::trade::BuyRequest`).
        if faction.is_some_and(|f| i < 4 && !f.open[i])
            && let Some(lock) = m
                .str(&format!("marketTab{}Bitmap", i + 1))
                .and_then(|p| ui.record(p))
            && let Some((lh, _)) = lock
                .str("bitmapName")
                .map(str::to_owned)
                .and_then(|t| ui.image(&t))
        {
            super::additive::quad(ui, lh, r, Color::WHITE);
        }
    }
    search_box(
        ui,
        &m,
        search,
        o,
        layout,
        ("marketSearchBox", "marketClearSearchButton"),
    );
    if let Some(f) = faction {
        faction_header(ui, &m, f, o);
    }
    let Some(g) = m.str("marketGrid").and_then(|g| ui.record(g)) else {
        return;
    };
    let f = |k: &str| g.f32(k).unwrap_or(0.0);
    let r = URect::new(
        o.x + ui.sc(f("inventoryX")),
        o.y + ui.sc(f("inventoryY")),
        ui.sc(f("inventoryXSize")),
        ui.sc(f("inventoryYSize")),
    );
    let cell = r.w / VENDOR_W as f32;
    layout.grid = Some((r, cell));
    if let Some(sack) = pages.get(tab) {
        grid_items(ui, inv, sack, r, cell);
        // The search marks over the shown page (module doc).
        if searching {
            search_marks(
                ui,
                sack,
                search.hits.get(tab).map(Vec::as_slice).unwrap_or_default(),
                r,
                cell,
            );
        }
    }
    // Other tabs with matches pulse (module doc).
    if searching {
        search_pulse(ui, &tab_rects, tab, &search.hits);
    }
}

/// A vendor tab with search matches (its alpha pulses, [`animate_search`]).
#[derive(Component)]
pub struct SearchPulse;

/// The search pulse's colour (its alpha animates).
const PULSE: Color = Color::srgb(0.73, 0.68, 0.43);

/// The focused search box's caret (blinks, [`animate_search`]).
#[derive(Component)]
pub struct SearchCaret;

/// The search box's text, caret and clear button (module doc). `box_field` /
/// `clear_field` name the window record's edit box and clear button
/// (`marketSearchBox` / `marketClearSearchButton` for the market windows,
/// `searchBox` / `clearSearchButton` for the caravan window and the crafting
/// panel).
fn search_box(
    ui: &mut Ui,
    m: &gd_formats::arz::Record,
    search: &crate::trade::TradeSearch,
    o: Vec2,
    layout: &mut TradeLayout,
    (box_field, clear_field): (&str, &str),
) {
    let Some(b) = m.str(box_field).and_then(|p| ui.record(p)) else {
        return;
    };
    let f = |k: &str| b.f32(k).unwrap_or(0.0);
    let r = URect::new(
        o.x + ui.sc(f("LocationX")),
        o.y + ui.sc(f("LocationY")),
        ui.sc(f("Width")),
        ui.sc(f("Height")),
    );
    layout.search = Some(r);
    if let Some(style) = b.str("Style") {
        let lh = ui.line_height(style);
        let y = (r.y + (r.h - lh) * 0.5).round();
        ui.text_line(style, Vec2::new(r.x, y), &search.text, 1.0);
        if search.focused {
            let pre = |ui: &mut Ui, n: usize| {
                ui.text_width(style, &search.text.chars().take(n).collect::<String>())
            };
            // PORT: the selection highlight colour.
            if let Some((a, b)) = search.edit.selection().filter(|(a, b)| a < b) {
                let (xa, xb) = (r.x + pre(ui, a), r.x + pre(ui, b));
                ui.solid(
                    URect::new(xa, r.y, xb - xa, r.h),
                    Color::srgba(0.4, 0.5, 0.9, 0.5),
                );
            }
            let x = r.x + pre(ui, search.edit.caret.min(search.text.chars().count()));
            let e = ui.solid(URect::new(x, r.y, ui.sc(1.0).max(1.0), r.h), Color::WHITE);
            ui.commands.entity(e).insert(SearchCaret);
        }
    }
    if !search.text.is_empty()
        && let Some(cb) = m.str(clear_field).and_then(|p| ui.record(p))
        && let Some((h, cr)) = button_rect(ui, &cb, "bitmapNameUp", o)
    {
        ui.quad(h, cr, None, Color::WHITE);
        layout.clear_search = Some(cr);
        ui.record_button_sound(cr, &cb);
    }
}

/// The search marks over a shown grid page (module doc): the border over each
/// match, black over the rest.
fn search_marks(
    ui: &mut Ui,
    sack: &gd_sim::items::inventory::Sack<Item>,
    hits: &[(u32, u32)],
    r: URect,
    cell: f32,
) {
    let border = ui
        .image("ui/character/itemsearchborder.tex")
        .map(|(h, _)| h);
    for p in &sack.items {
        let cr = URect::new(
            r.x + p.x as f32 * cell,
            r.y + p.y as f32 * cell,
            p.size.w as f32 * cell,
            p.size.h as f32 * cell,
        );
        match (&border, hits.contains(&(p.x, p.y))) {
            (Some(b), true) => {
                ui.quad_stretch(b.clone(), cr, Color::srgba(0.918, 0.553, 0.118, 0.75));
            }
            (None, true) => {}
            (_, false) => {
                ui.solid(cr, Color::srgba(0.0, 0.0, 0.0, 0.75));
            }
        }
    }
}

/// Every tab but the shown one whose page has a search match pulses (module doc).
fn search_pulse(ui: &mut Ui, tabs: &[(usize, URect)], shown: usize, hits: &[Vec<(u32, u32)>]) {
    for (i, tr) in tabs {
        if *i != shown && hits.get(*i).is_some_and(|h| !h.is_empty()) {
            let one = ui.sc(1.0);
            let e = ui.solid(
                URect::new(tr.x + one, tr.y + one, tr.w - 2.0 * one, tr.h - 2.0 * one),
                PULSE.with_alpha(0.05),
            );
            ui.commands.entity(e).insert(SearchPulse);
        }
    }
}

/// The search pulse alpha and the caret blink (module doc), every frame, on real time.
pub fn animate_search(
    time: Res<Time<Real>>,
    mut pulses: Query<&mut BackgroundColor, With<SearchPulse>>,
    mut carets: Query<&mut Visibility, With<SearchCaret>>,
) {
    let ms = (time.elapsed_secs_f64() * 1000.0) as u64;
    let mut a = (ms % 2000) as f32 * 0.0005 * 0.55 + 0.05;
    if a > 0.325 {
        a = 0.325 - (a - 0.325);
    }
    for mut c in &mut pulses {
        c.0 = PULSE.with_alpha(a);
    }
    let on = (ms / 600) % 2 == 0;
    for mut v in &mut carets {
        let want = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *v != want {
            *v = want;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn stash_window(
    ui: &mut Ui,
    stash: Option<&Stash>,
    tab: usize,
    transfer: bool,
    reagents: Option<usize>,
    npc: &str,
    search: &crate::trade::TradeSearch,
    inv: &Inventory,
    layout: &mut TradeLayout,
) {
    let Some(m) = ui.record(CARAVAN) else { return };
    let w = window(ui, &m);
    let o = Vec2::new(w.x, w.y);
    layout.window = Some(w);
    draw_field(ui, &m, "BackgroundImage", o);
    close_field(ui, &m, "CloseButton", o, layout);
    if transfer {
        draw_field(ui, &m, "TransferTitleBitmap", o);
    }
    npc_headings(ui, &m, npc, ("CaravanDriverName", "caravanDriverText"), o);
    // The toggle: "transfer stash" up, "personal stash" down.
    if let Some(t) = m.str("ToggleStashButton").and_then(|s| ui.record(s)) {
        let key = if transfer {
            "bitmapNameDown"
        } else {
            "bitmapNameUp"
        };
        if let Some((h, size)) = t.str(key).and_then(|b| ui.image(b)) {
            let r = URect::new(
                o.x + ui.sc(t.f32("bitmapPositionX").unwrap_or(0.0)),
                o.y + ui.sc(t.f32("bitmapPositionY").unwrap_or(0.0)),
                ui.sc(size.x as f32),
                ui.sc(size.y as f32),
            );
            ui.quad(h, r, None, Color::WHITE);
            layout.toggle = Some(r);
            ui.record_button_sound(r, &t);
        }
    }
    // Auto-sort (`StashSortButton` / `TransferStashSortButton`).
    if let Some(b) = m
        .str(if transfer {
            "TransferStashSortButton"
        } else {
            "StashSortButton"
        })
        .and_then(|b| ui.record(b))
        && let Some((h, r)) = button_rect(ui, &b, "bitmapNameUp", o)
    {
        ui.quad(h, r, None, Color::WHITE);
        layout.sort = Some(r);
        ui.record_button_sound(r, &b);
    }
    // The search box (module doc): the caravan window searches the shown
    // stash's pages like the vendor window (`trade.rs` `search_hits`).
    search_box(
        ui,
        &m,
        search,
        o,
        layout,
        ("searchBox", "clearSearchButton"),
    );
    let (win_key, scroll_key, tab_key) = if transfer {
        (
            "TransferWindow",
            "transferScrollWindow",
            "transferTabButton",
        )
    } else {
        ("StashWindow", "privateScrollWindow", "tabButton")
    };
    let Some(sw) = m.str(win_key).and_then(|s| ui.record(s)) else {
        return;
    };
    let Some(g) = sw.str("InventoryDBR").and_then(|g| ui.record(g)) else {
        return;
    };
    let f = |r: &gd_formats::arz::Record, k: &str| r.f32(k).unwrap_or(0.0);
    let (gx, gy) = (
        f(&sw, "WindowLocationX") + f(&g, "inventoryX"),
        f(&sw, "WindowLocationY") + f(&g, "inventoryY"),
    );
    let r = URect::new(
        o.x + ui.sc(gx),
        o.y + ui.sc(gy),
        ui.sc(f(&g, "inventoryXSize")),
        ui.sc(f(&g, "inventoryYSize")),
    );
    let cell = r.w / STASH_W as f32;
    layout.grid = Some((r, cell));
    let Some(stash) = stash else { return };
    let pages = stash.view(transfer);
    // One tab per page, down the left side.
    let (tx, ty0) = m
        .str(scroll_key)
        .and_then(|s| ui.record(s))
        .map_or((39.0, 225.0), |s| (f(&s, "positionX"), f(&s, "positionY")));
    let dy = m.f32("tabSpacingY").unwrap_or(38.0);
    // Transfer view: the Components (`relicButton`) and Materials
    // (`materialButton`) tabs come first, in that order, then the pages
    // (caravan tab builder GrimDawn.exe `sub_1401385F0`).
    let reagent_view = if transfer { reagents } else { None };
    // The Materials / Components plate (it includes the tab column's art,
    // so it goes under the tabs).
    let reagent_rec = reagent_view.and_then(|k| {
        m.str(if k == 0 {
            "MaterialWindow"
        } else {
            "RelicWindow"
        })
        .and_then(|w| ui.record(w))
    });
    if let Some(w) = &reagent_rec {
        let base = o + Vec2::new(
            ui.sc(w.f32("WindowLocationX").unwrap_or(0.0)),
            ui.sc(w.f32("WindowLocationY").unwrap_or(0.0)),
        );
        draw_field(ui, w, "TransferPlate", base);
    }
    let ty = if transfer { ty0 + 2.0 * dy } else { ty0 };
    if transfer {
        for (slot, (i, k)) in [(1, "relicButton"), (0, "materialButton")]
            .into_iter()
            .enumerate()
        {
            let Some(b) = m.str(k).and_then(|b| ui.record(b)) else {
                continue;
            };
            let key = if reagent_view == Some(i) {
                "bitmapNameDown"
            } else {
                "bitmapNameUp"
            };
            let Some((h, size)) = b.str(key).map(str::to_owned).and_then(|t| ui.image(&t)) else {
                continue;
            };
            let tr = URect::new(
                o.x + ui.sc(tx),
                o.y + ui.sc(ty0 + dy * slot as f32),
                ui.sc(size.x as f32),
                ui.sc(size.y as f32),
            );
            ui.quad(h, tr, None, Color::WHITE);
            layout.reagent_tabs.push((i, tr));
            ui.record_button_sound(tr, &b);
        }
    }
    // A tab: its background, then its `defaultBorder` and `defaultSymbol`
    // layers; the selected one uses the down bitmaps.
    let layers: Vec<gd_formats::arz::Record> = m
        .str(tab_key)
        .and_then(|t| ui.record(t))
        .map(|t| {
            let extra: Vec<_> = ["defaultBorder", "defaultSymbol"]
                .iter()
                .filter_map(|k| t.str(k).and_then(|r| ui.record(r)))
                .collect();
            std::iter::once(t).chain(extra).collect()
        })
        .unwrap_or_default();
    let mut tab_rects = Vec::new();
    for i in 0..pages.len() {
        let key = if i == tab && reagent_view.is_none() {
            "bitmapNameDown"
        } else {
            "bitmapNameUp"
        };
        for (n, l) in layers.iter().enumerate() {
            let Some((h, size)) = l.str(key).and_then(|t| ui.image(t)) else {
                continue;
            };
            let tr = URect::new(
                o.x + ui.sc(tx),
                o.y + ui.sc(ty + dy * i as f32),
                ui.sc(size.x as f32),
                ui.sc(size.y as f32),
            );
            ui.quad(h, tr, None, Color::WHITE);
            if n == 0 {
                layout.tabs.push((i, tr));
                tab_rects.push((i, tr));
                ui.record_button_sound(tr, l);
            }
        }
    }
    // After the pages: the buy tab while a page can be bought, then locks
    // (the buy tab's disabled bitmap) filling the scroll window's slots.
    let buy_rec = m
        .str(if transfer {
            "buyTransferTabButton"
        } else {
            "buyTabButton"
        })
        .and_then(|b| ui.record(b));
    let window_h = m
        .str(scroll_key)
        .and_then(|s| ui.record(s))
        .and_then(|s| s.f32("height"))
        .unwrap_or(304.0);
    let slots = (((window_h - (ty - ty0)) / dy) as usize).max(pages.len() + 1);
    let can_buy = crate::trade::page_cost(&sw, pages.len()).is_some();
    if let Some(b) = buy_rec {
        for i in pages.len()..slots {
            let key = if i == pages.len() && can_buy {
                "bitmapNameUp"
            } else {
                "bitmapNameDisabled"
            };
            let Some((h, size)) = b.str(key).map(str::to_owned).and_then(|t| ui.image(&t)) else {
                break;
            };
            let tr = URect::new(
                o.x + ui.sc(tx),
                o.y + ui.sc(ty + dy * i as f32),
                ui.sc(size.x as f32),
                ui.sc(size.y as f32),
            );
            ui.quad(h, tr, None, Color::WHITE);
            if i == pages.len() && can_buy {
                layout.buy = Some(tr);
                ui.record_button_sound(tr, &b);
            }
        }
    }
    if let Some(w) = reagent_rec {
        layout.grid = None;
        reagent_boxes(ui, &w, stash, inv, o, layout);
        return;
    }
    let shown = tab.min(pages.len().saturating_sub(1));
    if let Some(page) = pages.get(shown) {
        grid_items(ui, inv, page, r, cell);
        // The search marks and the other pages' pulses (module doc).
        if !search.query().is_empty() {
            search_marks(
                ui,
                page,
                search
                    .hits
                    .get(shown)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                r,
                cell,
            );
            search_pulse(ui, &tab_rects, shown, &search.hits);
        }
    }
}

/// The transfer view's Materials (`MaterialWindow`) / Components
/// (`RelicWindow`) tab: its plate (`TransferPlate`) at `WindowLocation`, and
/// one box per `reagentBoxes` entry (`reagentName` at `itemBoxX/Y` on the
/// plate) with the stored count from `reagents.gst`. Empty boxes are drawn
/// like the others (caravan draw GrimDawn.exe `sub_1401335A0`: the
/// only overlay is the blue highlight, colour 0.4, 0.6, 1, 0.3, of boxes
/// flagged as usable on the held item). Entries not stored are drawn grey
/// with a `0`, stored ones on their rarity frame with the count (the box
/// draw is `sub_1401EDF90`, `GetItemBackground`).
fn reagent_boxes(
    ui: &mut Ui,
    w: &gd_formats::arz::Record,
    stash: &Stash,
    inv: &Inventory,
    o: Vec2,
    layout: &mut TradeLayout,
) {
    let base = o + Vec2::new(
        ui.sc(w.f32("WindowLocationX").unwrap_or(0.0)),
        ui.sc(w.f32("WindowLocationY").unwrap_or(0.0)),
    );
    for b in w.strs("reagentBoxes") {
        let Some(rb) = ui.record(b) else { continue };
        let Some(record) = rb.str("reagentName").map(str::to_owned) else {
            continue;
        };
        let count = stash
            .materials
            .entries
            .iter()
            .find(|e| e.record.eq_ignore_ascii_case(&record))
            .map_or(0, |e| e.count);
        // Shown as a single item; the count is drawn below (the box shows it
        // for every entry, `0` included).
        let item = gd_formats::save::item::Item {
            base_name: record.clone(),
            stack_count: 1,
            ..Default::default()
        };
        let Some(size) = inv.info(&item).map(|i| i.size) else {
            continue;
        };
        let r = URect::new(
            base.x + ui.sc(rb.f32("itemBoxX").unwrap_or(0.0)),
            base.y + ui.sc(rb.f32("itemBoxY").unwrap_or(0.0)),
            ui.sc(32.0 * size.w as f32),
            ui.sc(32.0 * size.h as f32),
        );
        if count > 0 {
            // Owned: the rarity frame behind the icon (`GetItemBackground`).
            super::character::item_cell(ui, inv, &item, r);
        } else {
            // Unowned: the icon alone, desaturated (`sub_1401EDF90` renders
            // it with the `desaturate` style); PORT: luma, `Ui::image_gray`.
            if let Some(info) = inv.info(&item)
                && let Some((h, isz)) = ui.image_gray(&info.resolved.bitmap)
            {
                let (sw, sh) = (ui.sc(isz.x as f32), ui.sc(isz.y as f32));
                let d = URect::new(
                    r.x + ((r.w - sw) * 0.5).trunc(),
                    r.y + ((r.h - sh) * 0.5).trunc(),
                    sw,
                    sh,
                );
                ui.quad(h, d, None, Color::WHITE);
            }
        }
        let tb = gd_formats::ui::TextBox {
            style: None,
            rect: gd_formats::ui::Rect::new(
                (r.x + 2.0) / ui.s,
                (r.y + 2.0) / ui.s,
                (r.w - 4.0) / ui.s,
                (r.h - 2.0) / ui.s,
            ),
            align_x: gd_formats::ui::TextAlign::End,
            align_y: gd_formats::ui::TextAlign::End,
            tag: None,
            glow: None,
        };
        ui.text_with_style(
            &tb,
            "records/ui/styles/text/style_textwhite_sizes_bold.dbr",
            Vec2::ZERO,
            &count.to_string(),
        );
        layout.reagent_boxes.push((record, r));
    }
}

fn button_rect(
    ui: &mut Ui,
    rec: &gd_formats::arz::Record,
    key: &str,
    o: Vec2,
) -> Option<(Handle<Image>, URect)> {
    let (h, size) = rec.str(key).map(str::to_owned).and_then(|t| ui.image(&t))?;
    let r = URect::new(
        o.x + ui.sc(rec.f32("bitmapPositionX").unwrap_or(0.0)),
        o.y + ui.sc(rec.f32("bitmapPositionY").unwrap_or(0.0)),
        ui.sc(size.x as f32),
        ui.sc(size.y as f32),
    );
    Some((h, r))
}

/// A text box record's rect, scaled and placed at `o`.
fn item_box(ui: &Ui, rec: &gd_formats::arz::Record, o: Vec2) -> URect {
    let f = |k: &str| rec.f32(k).unwrap_or(0.0);
    URect::new(
        o.x + ui.sc(f("itemX")),
        o.y + ui.sc(f("itemY")),
        ui.sc(f("itemXSize")),
        ui.sc(f("itemYSize")),
    )
}

fn text_field(ui: &mut Ui, rec: &gd_formats::arz::Record, field: &str, o: Vec2, text: &str) {
    if let Some(r) = rec.str(field).and_then(|p| ui.record(p)) {
        ui.text(&gd_formats::ui::TextBox::from_record(&r), o, text);
    }
}

/// The Inventor's window (`sub_14026CF20` loader, `sub_14026DAC0`
/// open handler): the four tabs salvage (`enchanterRecoverTab`), dismantle,
/// convert and reroll (`*TabButton` records); salvage is always open, the
/// others need their story token (greyed otherwise). The name and the
/// `enchanterHeadingText` title are the NPC's.
#[allow(clippy::too_many_arguments)]
fn inventor_window(
    ui: &mut Ui,
    trade: &Trade,
    stash: &Stash,
    gold: u64,
    level: u32,
    inv: &Inventory,
    layout: &mut TradeLayout,
) {
    let Some(m) = ui.record(crate::crafting::WINDOW) else {
        return;
    };
    let w = window(ui, &m);
    let o = Vec2::new(w.x, w.y);
    layout.window = Some(w);
    draw_field(
        ui,
        &m,
        match trade.panel {
            Panel::Craft | Panel::Recover => "enchanterRecoverTabBitmap",
            Panel::Dismantle => "enchanterDismantleTabBitmap",
            Panel::Convert => "enchanterConvertTabBitmap",
            Panel::Reroll => "enchanterRerollTabBitmap",
        },
        o,
    );
    close_field(ui, &m, "enchanterCloseButton", o, layout);
    npc_headings(
        ui,
        &m,
        &trade.npc_record,
        ("enchanterNameText", "enchanterHeadingText"),
        o,
    );
    let tabs = [
        (Panel::Recover, "enchanterRecoverTabButton", true),
        (
            Panel::Dismantle,
            "enchanterDismantleTabButton",
            trade.tabs_unlocked[0],
        ),
        (
            Panel::Convert,
            "enchanterConvertTabButton",
            trade.tabs_unlocked[1],
        ),
        (
            Panel::Reroll,
            "enchanterRerollTabButton",
            trade.tabs_unlocked[2],
        ),
    ];
    for (panel, key, unlocked) in tabs {
        let Some(b) = m.str(key).and_then(|b| ui.record(b)) else {
            continue;
        };
        let state = if trade.panel == panel {
            "bitmapNameDown"
        } else if unlocked {
            "bitmapNameUp"
        } else {
            "bitmapNameDisabled"
        };
        let Some((h, r)) = button_rect(ui, &b, state, o) else {
            continue;
        };
        ui.quad(h, r, None, Color::WHITE);
        if unlocked {
            layout.buttons.push((Button::Tab(panel), r));
            ui.record_button_sound(r, &b);
        }
    }
    match trade.panel {
        Panel::Dismantle => dismantle_panel(ui, &m, trade, stash, gold, inv, o, layout),
        Panel::Reroll => reroll_panel(ui, &m, trade, stash, gold, inv, o, layout),
        Panel::Recover | Panel::Craft => recover_panel(ui, &m, trade, gold, inv, o, layout),
        Panel::Convert => convert_panel(ui, &m, trade, stash, gold, level, inv, o, layout),
    }
}

/// The convert tab (`sub_1401A7AF0` loader / `sub_1401A7260`
/// update / `sub_1401A93D0` ready test): item slot (`reagentBaseBox`), the
/// `sameSetButton` / `randomSetButton` radio pair (the picked one down), the
/// formula's reagent boxes, the cost and `createItemButton` (item, every
/// reagent, money). The table's `productBox` is not read by the loader.
#[allow(clippy::too_many_arguments)]
fn convert_panel(
    ui: &mut Ui,
    m: &gd_formats::arz::Record,
    trade: &Trade,
    stash: &Stash,
    gold: u64,
    level: u32,
    inv: &Inventory,
    o: Vec2,
    layout: &mut TradeLayout,
) {
    let Some(tab) = m.str("enchanterConvertTab").and_then(|t| ui.record(t)) else {
        return;
    };
    if let Some(b) = tab.str("reagentBaseBox").and_then(|b| ui.record(b)) {
        let r = item_box(ui, &b, o);
        item_slot(ui, r, trade.slot.as_ref(), inv, layout);
    }
    for (random, key) in [(false, "sameSetButton"), (true, "randomSetButton")] {
        let Some(b) = tab.str(key).and_then(|b| ui.record(b)) else {
            continue;
        };
        let Some((h, r)) = button_rect(
            ui,
            &b,
            if trade.random_set == random {
                "bitmapNameDown"
            } else {
                "bitmapNameUp"
            },
            o,
        ) else {
            continue;
        };
        ui.quad(h, r, None, Color::WHITE);
        layout.buttons.push((Button::ConvertMode(random), r));
        ui.record_button_sound(r, &b);
        if let (Some(style), Some(t)) = (b.str("style"), b.str("textTag")) {
            let text = ui.fonts.tag(t).to_string();
            label(ui, r, style, gd_formats::ui::TextAlign::Center, &text);
        }
    }
    let idb = ui.data.item_db();
    let plan = trade
        .slot
        .as_ref()
        .and_then(|it| gd_game::crafting::convert_plan(ui.data, &idb, it, trade.random_set, level));
    if let Some(p) = &plan {
        reagent_boxes_plan(ui, &tab, p, o, inv, stash, layout);
    }
    cost_texts(
        ui,
        &tab,
        "costText",
        "costNumber",
        "tooExpensiveCostNumber",
        o,
        plan.as_ref().map(|p| p.cost),
        gold,
    );
    if let Some(b) = tab.str("createItemButton").and_then(|b| ui.record(b)) {
        let ok = plan
            .as_ref()
            .is_some_and(|p| crate::reroll::shortfall(p, gold, inv, stash).is_none());
        action_button(ui, &b, o, ok, Button::Go, layout);
    }
}

/// The salvage tab (`sub_1401B2870` loader / `sub_1401B20C0`
/// update): item slot (`recoveryBox`), cost label and number (red when
/// short) and the three buttons, enabled per `crafting::salvage`.
#[allow(clippy::too_many_arguments)]
fn recover_panel(
    ui: &mut Ui,
    m: &gd_formats::arz::Record,
    trade: &Trade,
    gold: u64,
    inv: &Inventory,
    o: Vec2,
    layout: &mut TradeLayout,
) {
    let Some(tab) = m.str("enchanterRecoverTab").and_then(|t| ui.record(t)) else {
        return;
    };
    if let Some(b) = tab.str("recoveryBox").and_then(|b| ui.record(b)) {
        let r = item_box(ui, &b, o);
        item_slot(ui, r, trade.slot.as_ref(), inv, layout);
    }
    let idb = ui.data.item_db();
    let data = ui.data.clone();
    let cost = trade
        .slot
        .as_ref()
        .map(|it| crate::crafting::salvage_cost(&data, &idb, it));
    cost_texts(
        ui,
        &tab,
        "recoverCostText",
        "recoverCostNumber",
        "tooExpensiveCostNumber",
        o,
        cost,
        gold,
    );
    let affordable = cost.is_some_and(|c| gold >= c);
    let (relic, augment) = trade.slot.as_ref().map_or((false, false), |i| {
        (!i.relic_name.is_empty(), !i.augment_name.is_empty())
    });
    for (field, which, enabled) in [
        ("recoverItemButton", Button::KeepItem, affordable && relic),
        ("recoverRelicButton", Button::KeepAddon, affordable && relic),
        (
            "removeAugmentButton",
            Button::RemoveAugment,
            affordable && augment,
        ),
    ] {
        if let Some(b) = tab.str(field).and_then(|b| ui.record(b)) {
            action_button(ui, &b, o, enabled, which, layout);
        }
    }
}

/// The selected recipe row's frame, 1 px larger than the row each way (as the altar's and the factions' lists).
const ROW_HIGHLIGHT: &str = "ui/inventor/crafting_recipeselectionhighlight.tex";

/// A crafting count as the window prints it: "99+" over 99 (as the altar list, `altar.rs` module doc).
fn count_text(n: u32) -> String {
    if n > 99 {
        "99+".to_owned()
    } else {
        n.to_string()
    }
}

#[allow(clippy::too_many_arguments)]
fn crafter_window(
    ui: &mut Ui,
    list: &CraftList,
    trade: &Trade,
    stash: &Stash,
    gold: u64,
    level: u32,
    inv: &Inventory,
    layout: &mut TradeLayout,
) {
    let Some(m) = ui.record(crate::crafting::WINDOW) else {
        return;
    };
    let w = window(ui, &m);
    let o = Vec2::new(w.x, w.y);
    layout.window = Some(w);
    draw_field(ui, &m, "enchanterBaseBitmap", o);
    close_field(ui, &m, "enchanterCloseButton", o, layout);
    let name = ui.fonts.tag(&list.name_tag).to_string();
    text_field(ui, &m, "enchanterNameText", o, &name);
    let title = ui.fonts.tag(&list.title_tag).to_string();
    text_field(ui, &m, "crafterHeadingText", o, &title);
    // Category buttons: relics, then tabs 1-4.
    let keys = [
        "craftingTabArtifactButton",
        "craftingTab1Button",
        "craftingTab2Button",
        "craftingTab3Button",
        "craftingTab4Button",
    ];
    for (i, k) in keys.iter().enumerate() {
        let Some(b) = m.str(k).and_then(|b| ui.record(b)) else {
            continue;
        };
        let Some((h, r)) = button_rect(
            ui,
            &b,
            if i == trade.tab {
                "bitmapNameDown"
            } else {
                "bitmapNameUp"
            },
            o,
        ) else {
            continue;
        };
        ui.quad(h, r, None, Color::WHITE);
        layout.tabs.push((i, r));
        ui.record_button_sound(r, &b);
    }
    let Some(tab) = m.str("enchanterArtifactTab").and_then(|t| ui.record(t)) else {
        return;
    };
    // The search box (module doc): the crafting panel filters its recipe list
    // with the query (`crafting.rs` `CraftList::rows`).
    search_box(
        ui,
        &tab,
        &trade.search,
        o,
        layout,
        ("searchBox", "clearSearchButton"),
    );
    // The recipe list: group headers, then "[craftable] item name" rows.
    let rows = list.rows(trade.tab, &trade.search.query());
    let item_rec = tab.str("craftingListBoxItem").and_then(|r| ui.record(r));
    if let Some(lw) = tab.str("craftingListBox").and_then(|r| ui.record(r)) {
        let f = |k: &str| lw.f32(k).unwrap_or(0.0);
        let lr = URect::new(
            o.x + ui.sc(f("positionX")),
            o.y + ui.sc(f("positionY")),
            ui.sc(f("width")),
            ui.sc(f("height")),
        );
        layout.list = Some(lr);
        let line = item_rec
            .as_ref()
            .and_then(|r| r.f32("LineHeight"))
            .unwrap_or(23.0);
        let name_x = item_rec
            .as_ref()
            .and_then(|r| r.f32("NameXPosition"))
            .unwrap_or(10.0);
        let style = item_rec
            .as_ref()
            .and_then(|r| r.str("FontStyle").map(str::to_owned))
            .unwrap_or_default();
        let header_style = tab
            .str("craftingListBoxHeaderFont")
            .map(str::to_owned)
            .unwrap_or_else(|| style.clone());
        let visible = (f("height") / line) as usize;
        // Relic rows: the list font in the crafted relics' name colour (`name_color`, `artifactColor`).
        let relic_color = super::character::name_color(
            &ui.data.db,
            gd_sim::items::Classification::Artifact as u32,
        );
        for (n, row) in rows.iter().skip(trade.scroll).take(visible).enumerate() {
            let rr = URect::new(lr.x, lr.y + ui.sc(line * n as f32), lr.w, ui.sc(line));
            let tb = |indent: f32| gd_formats::ui::TextBox {
                style: None,
                rect: gd_formats::ui::Rect::new(
                    f("positionX") + indent,
                    f("positionY") + line * n as f32,
                    f("width") - indent,
                    line,
                ),
                align_x: gd_formats::ui::TextAlign::Start,
                align_y: gd_formats::ui::TextAlign::Center,
                tag: None,
                glow: None,
            };
            match *row {
                crate::crafting::Row::Header(g) => {
                    // The group's open state: `listboxuncollapsedicon` before its name (the groups never
                    // collapse here, PORT).
                    let mut indent = 2.0;
                    if let Some((h, size)) = ui.image("ui/generic/listboxuncollapsedicon.tex") {
                        let (w, hh) = (size.x as f32, size.y as f32);
                        ui.quad(
                            h,
                            URect::new(
                                rr.x + ui.sc(indent),
                                rr.y + ((rr.h - ui.sc(hh)) * 0.5).round(),
                                ui.sc(w),
                                ui.sc(hh),
                            ),
                            None,
                            Color::WHITE,
                        );
                        indent += w + 3.0;
                    }
                    let text = ui.fonts.tag(crate::crafting::GROUPS[g].1).to_string();
                    ui.text_with_style(&tb(indent), &header_style, o, &text);
                }
                crate::crafting::Row::Recipe(i) => {
                    let rec = &list.recipes[i];
                    let chosen = trade.recipe == Some(i);
                    if chosen && let Some((h, _)) = ui.image(ROW_HIGHLIGHT) {
                        let g = URect::new(rr.x - 1.0, rr.y - 1.0, rr.w + 2.0, rr.h + 2.0);
                        ui.quad_stretch(h, g, Color::WHITE);
                    }
                    let n = crate::crafting::max_craftable(&rec.bp, gold, level, inv, stash);
                    let color = if chosen {
                        Some([1.0; 3])
                    } else {
                        rec.artifact.then_some(relic_color)
                    };
                    let name = ui
                        .fonts
                        .tag(if rec.item_tag.is_empty() {
                            &rec.name_tag
                        } else {
                            &rec.item_tag
                        })
                        .to_string();
                    ui.text_colored(
                        &tb(name_x),
                        &style,
                        o,
                        &format!("[{}]{name}", count_text(n)),
                        color,
                    );
                    layout.rows.push((i, rr));
                }
            }
        }
        // The scroll bar along the list's right edge (`verticalScrollbar`, its length adjusted by
        // `scrollbarSizeAdjust`), shown while the rows overflow.
        if rows.len() > visible
            && let Some(sb) = lw.str("verticalScrollbar")
        {
            let lp = ui.sc(line);
            layout.list_bar = super::window::scrollbar(
                ui,
                sb,
                lr.x + lr.w,
                lr.y,
                lr.h + ui.sc(f("scrollbarSizeAdjust")),
                (
                    visible as f32 * lp,
                    rows.len() as f32 * lp,
                    trade.scroll as f32 * lp,
                ),
            );
        }
    }
    // The selected blueprint.
    let selected = trade.recipe.and_then(|i| list.recipes.get(i));
    if let Some(rec) = selected {
        let bp = &rec.bp;
        let text = ui
            .fonts
            .tag(if rec.item_tag.is_empty() {
                &rec.name_tag
            } else {
                &rec.item_tag
            })
            .to_string();
        let product = Item {
            base_name: bp.display.clone(),
            stack_count: bp.quantity,
            ..Default::default()
        };
        // The name in the product's name colour (`name_color`: a relic's cyan), as the list row.
        let name_color = inv
            .info(&product)
            .map(|i| super::character::name_color(&ui.data.db, i.resolved.display_color_index()));
        if let Some(r) = tab.str("artifactNameText").and_then(|p| ui.record(p)) {
            let tb = gd_formats::ui::TextBox::from_record(&r);
            if let Some(style) = tb.style.clone() {
                ui.text_colored(&tb, &style, o, &text, name_color);
            }
        }
        // The product and the base reagent (the two large boxes) in item cells: their class background
        // (`item_cell`), as the bags draw them; the small reagent boxes show the bare icon (as the real window).
        if let Some(b) = tab.str("artifactBox").and_then(|b| ui.record(b)) {
            let r = item_box(ui, &b, o);
            super::character::item_cell(ui, inv, &product, r);
            layout.boxes.push((product, r));
        }
        let slots = ["Base", "1", "2", "3", "4", "5", "6"];
        // A short reagent's count takes `insufficientMaterialColor` (as the altar's quantities).
        let red = [
            tab.f32("insufficientMaterialColor.r").unwrap_or(0.75),
            tab.f32("insufficientMaterialColor.g").unwrap_or(0.0),
            tab.f32("insufficientMaterialColor.b").unwrap_or(0.0),
        ];
        for (k, (names, need)) in slots.iter().zip(&bp.reagents) {
            let Some(b) = tab
                .str(&format!("reagent{k}Box"))
                .and_then(|b| ui.record(b))
            else {
                continue;
            };
            let r = item_box(ui, &b, o);
            let have = crate::crafting::count(names, inv, stash);
            let item = Item {
                base_name: names[0].clone(),
                stack_count: 1,
                ..Default::default()
            };
            if *k == "Base" {
                super::character::item_cell(ui, inv, &item, r);
            } else {
                super::character::item_icon(ui, inv, &item, r);
            }
            layout.boxes.push((item, r));
            if let Some(q) = tab
                .str(&format!("reagent{k}Quantity"))
                .and_then(|p| ui.record(p))
            {
                let tb = gd_formats::ui::TextBox::from_record(&q);
                if let Some(style) = tb.style.clone() {
                    ui.text_colored(
                        &tb,
                        &style,
                        o,
                        &format!("{}/{need}", count_text(have)),
                        (have < *need).then_some(red),
                    );
                }
            }
        }
    }
    // Cost and the craft button are always shown (empty / disabled without
    // a selection); the crafter's emblem sits in the button's circle.
    let label = tab
        .str("costText")
        .and_then(|t| ui.record(t))
        .and_then(|t| t.str("textTag").map(str::to_owned))
        .map(|t| ui.fonts.tag(&t).to_string())
        .unwrap_or_default();
    text_field(ui, &tab, "costText", o, &label);
    if let Some(rec) = selected {
        let cost = rec.bp.cost(level);
        text_field(
            ui,
            &tab,
            if gold >= cost {
                "costNumber"
            } else {
                "tooExpensiveCostNumber"
            },
            o,
            &super::character::money_text(cost),
        );
    }
    if let Some(b) = tab.str("createArtifactButton").and_then(|b| ui.record(b)) {
        let ok =
            selected.is_some_and(|r| crate::crafting::can_craft(&r.bp, gold, level, inv, stash));
        if let Some((h, r)) = button_rect(
            ui,
            &b,
            if ok {
                "bitmapNameUp"
            } else {
                "bitmapNameDisabled"
            },
            o,
        ) {
            ui.quad(h, r, None, Color::WHITE);
            layout.craft = Some(r);
            if ok {
                ui.record_button_sound(r, &b);
            }
            if let (Some(style), Some(t)) = (b.str("style"), b.str("textTag")) {
                let tb = gd_formats::ui::TextBox {
                    style: None,
                    rect: gd_formats::ui::Rect::new(
                        b.f32("bitmapPositionX").unwrap_or(0.0),
                        b.f32("bitmapPositionY").unwrap_or(0.0),
                        r.w / ui.s,
                        r.h / ui.s,
                    ),
                    align_x: gd_formats::ui::TextAlign::Center,
                    align_y: gd_formats::ui::TextAlign::Center,
                    tag: None,
                    glow: None,
                };
                let text = ui.fonts.tag(t).to_string();
                // Disabled: the label dimmed as a dim disabled button's bitmap (0.6, module doc). PORT: the
                // engine's disabled text colour is not decompiled.
                let dim = (!ok)
                    .then(|| ui.fonts.style(ui.data, style))
                    .flatten()
                    .map(|s| s.color0.map(|c| c * 0.6));
                ui.text_colored(&tb, style, o, &text, dim);
            }
        }
    }
    if let (Some(icon), Some(p)) = (
        list.bitmap.clone(),
        tab.str("crafterIcon").and_then(|p| ui.record(p)),
    ) {
        ui.bitmap(
            &icon,
            p.f32("bitmapPositionX").unwrap_or(0.0),
            p.f32("bitmapPositionY").unwrap_or(0.0),
            o,
        );
    }
}

/// The NPC's name (`description` tag) and profession (`professionTag`, else
/// the text box's own `textTag`) in the window's two heading text boxes.
fn npc_headings(
    ui: &mut Ui,
    m: &gd_formats::arz::Record,
    npc: &str,
    (name_field, title_field): (&str, &str),
    o: Vec2,
) {
    let Some(rec) = ui.data.db.get(npc) else {
        return;
    };
    let name = rec
        .str("description")
        .map(|t| ui.fonts.tag(t).to_string())
        .unwrap_or_default();
    text_field(ui, m, name_field, o, &name);
    let tb = m.str(title_field).and_then(|p| ui.record(p));
    let title = rec
        .str("professionTag")
        .or_else(|| tb.as_ref().and_then(|t| t.str("textTag")))
        .map(|t| ui.fonts.tag(t).to_string())
        .unwrap_or_default();
    text_field(ui, m, title_field, o, &title);
}

/// Faction merchant: the faction's name and the reputation bar (the
/// positive / negative bar filled within the current level, labelled with it).
fn faction_header(
    ui: &mut Ui,
    m: &gd_formats::arz::Record,
    f: &crate::trade::FactionView,
    o: Vec2,
) {
    let name = ui.fonts.tag(&f.name_tag).to_string();
    text_field(ui, m, "factionName", o, &name);
    let bar_key = if f.value >= 0.0 {
        "factionPositiveBar"
    } else {
        "factionNegativeBar"
    };
    let Some(Widget::BarGraph(bar)) = m.str(bar_key).and_then(|p| ui.widget(p)) else {
        return;
    };
    let (Some(empty), Some(full)) = (
        bar.empty.as_deref().and_then(|p| ui.image(p)),
        bar.full.as_deref().and_then(|p| ui.image(p)),
    ) else {
        return;
    };
    let r = URect::new(
        o.x + ui.sc(bar.x),
        o.y + ui.sc(bar.y),
        ui.sc(empty.1.x as f32),
        ui.sc(empty.1.y as f32),
    );
    ui.quad(empty.0, r, None, Color::WHITE);
    let (lo, hi) = f.levels.bounds(f.value);
    let frac = if hi != lo {
        ((f.value - lo as f32) / (hi - lo) as f32).clamp(0.0, 1.0)
    } else {
        1.0
    };
    if frac > 0.0 {
        ui.quad(
            full.0,
            URect::new(r.x, r.y, r.w * frac, r.h),
            Some(URect::new(
                0.0,
                0.0,
                full.1.x as f32 * frac,
                full.1.y as f32,
            )),
            Color::WHITE,
        );
    }
    if let Some(style) = bar.font_style.clone() {
        let tb = gd_formats::ui::TextBox {
            style: None,
            rect: gd_formats::ui::Rect::new(bar.x, bar.y, r.w / ui.s, r.h / ui.s),
            align_x: gd_formats::ui::TextAlign::Center,
            align_y: gd_formats::ui::TextAlign::Center,
            tag: None,
            glow: None,
        };
        let text = ui.fonts.tag(&f.levels.tag(f.value)).to_string();
        ui.text_with_style(&tb, &style, o, &text);
    }
}

/// A text in `r` (pixels), drawn with a style from the text styles.
pub(crate) fn label(
    ui: &mut Ui,
    r: URect,
    style: &str,
    align: gd_formats::ui::TextAlign,
    text: &str,
) -> Option<Entity> {
    let tb = gd_formats::ui::TextBox {
        style: None,
        rect: gd_formats::ui::Rect::new(r.x / ui.s, r.y / ui.s, r.w / ui.s, r.h / ui.s),
        align_x: align,
        align_y: gd_formats::ui::TextAlign::Center,
        tag: None,
        glow: None,
    };
    ui.text_with_style(&tb, style, Vec2::ZERO, text)
}

/// An action button (`bitmapNameUp` / `Disabled`, its `textTag` centred
/// with the record's `style`); enabled ones are clickable.
fn action_button(
    ui: &mut Ui,
    rec: &gd_formats::arz::Record,
    o: Vec2,
    enabled: bool,
    which: Button,
    layout: &mut TradeLayout,
) {
    let Some((h, r)) = button_rect(
        ui,
        rec,
        if enabled {
            "bitmapNameUp"
        } else {
            "bitmapNameDisabled"
        },
        o,
    ) else {
        return;
    };
    ui.quad(h, r, None, Color::WHITE);
    if enabled {
        layout.buttons.push((which, r));
        ui.record_button_sound(r, rec);
    }
    if let (Some(style), Some(t)) = (rec.str("style"), rec.str("textTag")) {
        let text = ui.fonts.tag(t).to_string();
        let e = label(ui, r, style, gd_formats::ui::TextAlign::Center, &text);
        // PORT: the button record has no disabled text style; the label is
        // drawn at 40% brightness when the button is off (reference shot).
        if let (false, Some(e)) = (enabled, e) {
            ui.commands.entity(e).queue(|mut em: EntityWorldMut| {
                let dim = Color::srgba(0.4, 0.4, 0.4, 1.0);
                if let Some(mut i) = em.get_mut::<ImageNode>() {
                    i.color = dim;
                }
                if let Some(mut t) = em.get_mut::<TextColor>() {
                    t.0 = t.0.mix(&Color::BLACK, 0.6);
                }
            });
        }
    }
}

/// An item slot: a faint plate over `r` and the item's icon (the slot is
/// clickable, `layout.slot`).
fn item_slot(
    ui: &mut Ui,
    r: URect,
    item: Option<&Item>,
    inv: &Inventory,
    layout: &mut TradeLayout,
) {
    // The slot plate is drawn dark; the item box's highlight colour
    // (`HighlightColor*`) shows while an item is carried (PORT).
    if inv.held.is_some() {
        ui.solid(r, Color::srgba(0.4, 0.63, 0.7, 0.2));
    }
    layout.slot = Some(r);
    if let Some(it) = item {
        slot_item(ui, r, it, inv);
    }
}

/// The item on its rarity frame, one grid cell per 32 px, centred in the box (`item_cell` is the inventory's
/// `GetItemBackground` draw).
fn slot_item(ui: &mut Ui, r: URect, it: &Item, inv: &Inventory) {
    let size = inv.info(it).map_or((2, 2), |i| (i.size.w, i.size.h));
    let (w, h) = (ui.sc(32.0 * size.0 as f32), ui.sc(32.0 * size.1 as f32));
    let cell = URect::new(
        r.x + ((r.w - w) * 0.5).trunc(),
        r.y + ((r.h - h) * 0.5).trunc(),
        w,
        h,
    );
    super::character::item_cell(ui, inv, it, cell);
}

/// The reagent boxes of a reroll / ascension plan: icon, red plate when
/// short, "have/need".
fn reagent_boxes_plan(
    ui: &mut Ui,
    tab: &gd_formats::arz::Record,
    plan: &crate::reroll::Plan,
    o: Vec2,
    inv: &Inventory,
    stash: &Stash,
    layout: &mut TradeLayout,
) {
    let red = Color::srgba(
        tab.f32("insufficientMaterialColor.r").unwrap_or(0.75),
        tab.f32("insufficientMaterialColor.g").unwrap_or(0.0),
        tab.f32("insufficientMaterialColor.b").unwrap_or(0.0),
        0.35,
    );
    for (n, (name, need)) in plan.reagents.iter().enumerate() {
        let Some(b) = tab
            .str(&format!("reagent{}Box", n + 1))
            .and_then(|b| ui.record(b))
        else {
            continue;
        };
        let r = item_box(ui, &b, o);
        let have = crate::crafting::count(std::slice::from_ref(name), inv, stash);
        if have < *need {
            ui.solid(r, red);
        }
        let item = Item {
            base_name: name.clone(),
            stack_count: 1,
            ..Default::default()
        };
        super::character::item_icon(ui, inv, &item, r);
        layout.boxes.push((item, r));
        text_field(
            ui,
            tab,
            &format!("reagent{}Quantity", n + 1),
            o,
            &format!("{have}/{need}"),
        );
    }
}

/// The cost label and number (red when the gold falls short).
#[allow(clippy::too_many_arguments)]
fn cost_texts(
    ui: &mut Ui,
    tab: &gd_formats::arz::Record,
    label_field: &str,
    number_field: &str,
    short_field: &str,
    o: Vec2,
    cost: Option<u64>,
    gold: u64,
) {
    let tag = tab
        .str(label_field)
        .and_then(|t| ui.record(t))
        .and_then(|t| t.str("textTag").map(str::to_owned))
        .map(|t| ui.fonts.tag(&t).to_string())
        .unwrap_or_default();
    text_field(ui, tab, label_field, o, &tag);
    if let Some(c) = cost {
        text_field(
            ui,
            tab,
            if gold >= c { number_field } else { short_field },
            o,
            &super::character::money_text(c),
        );
    }
}

/// The dismantle tab (loader `sub_1401AAA00`, update `sub_1401AA370`, render `sub_1401AA160`, mouse
/// `sub_1401AA2C0`). Render order: the boxes, `dynamiteNumberBoxBitmap` (the Dynamite icon), the button, the cost
/// (`dismantleCostNumber`, `tooExpensiveCostNumber` when short), the Dynamite total (`dynamiteNotEnoughNumber` at
/// 0), the two texts, `dynamiteBitmap` (no texture in the stock record). The boxes: while a result box holds an
/// item the two take-only result boxes are drawn and take the mouse in the item box's place (they share its area),
/// else the item box alone. The warning is a Yes / No dialog of the dialog manager (`sub_1401AA790`:
/// `DialogManager::AddDialog` with the warning tag), drawn by `ui/confirm.rs`.
#[allow(clippy::too_many_arguments)]
fn dismantle_panel(
    ui: &mut Ui,
    m: &gd_formats::arz::Record,
    trade: &Trade,
    stash: &Stash,
    gold: u64,
    inv: &Inventory,
    o: Vec2,
    layout: &mut TradeLayout,
) {
    let Some(tab) = m.str("enchanterDismantleTab").and_then(|t| ui.record(t)) else {
        return;
    };
    // Clicks in `trade.rs` `panel_clicks`: only the boxes pushed here take any.
    if trade.results.iter().any(Option::is_some) {
        for (i, key) in ["dismantleBox1", "dismantleBox2"].into_iter().enumerate() {
            if let Some(b) = tab.str(key).and_then(|b| ui.record(b)) {
                let r = item_box(ui, &b, o);
                layout.results.push((i, r));
                if let Some(it) = &trade.results[i] {
                    slot_item(ui, r, it, inv);
                }
            }
        }
    } else if let Some(b) = tab.str("itemBox").and_then(|b| ui.record(b)) {
        let r = item_box(ui, &b, o);
        item_slot(ui, r, trade.slot.as_ref(), inv, layout);
    }
    let bitmap = |ui: &mut Ui, key: &str| {
        if let Some(b) = tab.str(key).and_then(|b| ui.record(b))
            && let Some(tex) = b.str("bitmapName")
        {
            ui.bitmap(
                tex,
                b.f32("bitmapPositionX").unwrap_or(0.0),
                b.f32("bitmapPositionY").unwrap_or(0.0),
                o,
            );
        }
    };
    bitmap(ui, "dynamiteNumberBoxBitmap");
    let idb = ui.data.item_db();
    let cost = trade.slot.as_ref().map(|it| {
        crate::crafting::dismantle_cost(
            &tab,
            gd_sim::items::resolve(&idb, it).map_or(1, |r| r.item_level),
        )
    });
    cost_texts(
        ui,
        &tab,
        "dismantleCostText",
        "dismantleCostNumber",
        "tooExpensiveCostNumber",
        o,
        cost,
        gold,
    );
    let dynamite = ui
        .data
        .db
        .get("records/game/gameengine.dbr")
        .and_then(|e| e.str("dynamite").map(str::to_owned))
        .unwrap_or_default();
    let have = crate::crafting::count(&[dynamite], inv, stash);
    let tag = tab
        .str("dynamiteTotalText")
        .and_then(|t| ui.record(t))
        .and_then(|t| t.str("textTag").map(str::to_owned))
        .map(|t| ui.fonts.tag(&t).to_string())
        .unwrap_or_default();
    text_field(ui, &tab, "dynamiteTotalText", o, &tag);
    text_field(
        ui,
        &tab,
        if have == 0 {
            "dynamiteNotEnoughNumber"
        } else {
            "dynamiteTotalNumber"
        },
        o,
        &have.to_string(),
    );
    if let Some(b) = tab.str("dismantleItemButton").and_then(|b| ui.record(b)) {
        let ok = !trade.confirm && cost.is_some_and(|c| gold >= c) && have > 0;
        action_button(ui, &b, o, ok, Button::Go, layout);
    }
    bitmap(ui, "dynamiteBitmap");
    if trade.confirm
        && let Some(it) = &trade.slot
    {
        let tag = if it.relic_name.is_empty() {
            "tagDismantleDestroyItemWarning"
        } else {
            "tagDismantleDestroyItemComponentWarning"
        };
        layout.confirm = super::confirm::confirm_box(ui, tag, trade.confirm_look);
    }
}

/// The reroll tab (`sub_1401B59A0` / `sub_1401B4E60` /
/// `sub_1401B83C0`): item slot (`reagentBaseBox`), the four kind buttons
/// (the picked one down, off without the affix), the reagent box, the reroll
/// count, the cost and `rerollNowButton`. The count is
/// `Localize("tagRerollQuantity", n)` set on `rerollQuantityNumber` (`sub_1401B4E60`).
#[allow(clippy::too_many_arguments)]
fn reroll_panel(
    ui: &mut Ui,
    m: &gd_formats::arz::Record,
    trade: &Trade,
    stash: &Stash,
    gold: u64,
    inv: &Inventory,
    o: Vec2,
    layout: &mut TradeLayout,
) {
    use crate::reroll::Kind;
    let Some(tab) = m.str("enchanterRerollTab").and_then(|t| ui.record(t)) else {
        return;
    };
    if let Some(b) = tab.str("reagentBaseBox").and_then(|b| ui.record(b)) {
        let r = item_box(ui, &b, o);
        item_slot(ui, r, trade.slot.as_ref(), inv, layout);
    }
    let idb = ui.data.item_db();
    let test = ui.hooks.items.reroll.is_some();

    for (kind, key) in [
        (Kind::Seed, "rerollSeedButton"),
        (Kind::Prefix, "rerollPrefixButton"),
        (Kind::Suffix, "rerollSuffixButton"),
        (Kind::Ascended, "rerollAscendedAffixButton"),
    ] {
        let Some(b) = tab.str(key).and_then(|b| ui.record(b)) else {
            continue;
        };
        let usable = trade
            .slot
            .as_ref()
            .is_some_and(|it| crate::reroll::kind_state(&idb, it, kind, test).is_ok());
        let state = if trade.kind == Some(kind) && usable {
            "bitmapNameDown"
        } else if usable {
            "bitmapNameUp"
        } else {
            "bitmapNameDisabled"
        };
        let Some((h, r)) = button_rect(ui, &b, state, o) else {
            continue;
        };
        ui.quad(h, r, None, Color::WHITE);
        if usable {
            layout.buttons.push((Button::Kind(kind), r));
            ui.record_button_sound(r, &b);
        }
    }
    let picked = trade.slot.as_ref().zip(trade.kind);
    let plan = picked.and_then(|(it, k)| crate::reroll::plan(ui.data, &idb, it, k));
    if let Some((it, k)) = picked {
        let count = if k == Kind::Seed {
            it.seed_rerolls
        } else {
            it.affix_rerolls
        };
        let text = gd_sim::items::tooltip::format(
            ui.fonts.tag("tagRerollQuantity"),
            &[gd_sim::items::tooltip::Arg::Num(count as f32)],
        );
        text_field(ui, &tab, "rerollQuantityNumber", o, &text);
    }
    if let Some(p) = &plan {
        reagent_boxes_plan(ui, &tab, p, o, inv, stash, layout);
    }
    cost_texts(
        ui,
        &tab,
        "costText",
        "costNumber",
        "tooExpensiveCostNumber",
        o,
        plan.as_ref().map(|p| p.cost),
        gold,
    );
    if let Some(b) = tab.str("rerollNowButton").and_then(|b| ui.record(b)) {
        let ok = picked.is_some_and(|(it, k)| crate::reroll::kind_state(&idb, it, k, test).is_ok())
            && plan
                .as_ref()
                .is_some_and(|p| crate::reroll::shortfall(p, gold, inv, stash).is_none());
        action_button(ui, &b, o, ok, Button::Go, layout);
    }
}

/// The Runekeeper's item-ascension window (`itemascension_table.dbr`,
/// `sub_140112610` loader / `sub_140114C40` ready test): item slot
/// (`reagentBaseBox`), up to five reagent boxes, the cost and the ascend
/// button (item without an ascendant affix, money, every reagent).
fn ascension_window(
    ui: &mut Ui,
    trade: &Trade,
    stash: &Stash,
    gold: u64,
    inv: &Inventory,
    layout: &mut TradeLayout,
) {
    let Some(m) = ui.record(ASCENSION) else {
        return;
    };
    let w = window(ui, &m);
    let o = Vec2::new(w.x, w.y);
    layout.window = Some(w);
    draw_field(ui, &m, "backgroundBitmap", o);
    draw_field(ui, &m, "closeButton", o);
    npc_headings(
        ui,
        &m,
        &trade.npc_record,
        ("npcNameText", "npcHeadingText"),
        o,
    );
    if trade.slot.is_none() {
        draw_field(ui, &m, "previewIconBitmap", o);
    }
    if let Some(b) = m.str("reagentBaseBox").and_then(|b| ui.record(b)) {
        let r = item_box(ui, &b, o);
        item_slot(ui, r, trade.slot.as_ref(), inv, layout);
    }
    let idb = ui.data.item_db();
    let kind = crate::reroll::Kind::Ascend;
    let plan = trade
        .slot
        .as_ref()
        .and_then(|it| crate::reroll::plan(ui.data, &idb, it, kind));
    if let Some(p) = &plan {
        reagent_boxes_plan(ui, &m, p, o, inv, stash, layout);
    }
    cost_texts(
        ui,
        &m,
        "costText",
        "costNumber",
        "tooExpensiveCostNumber",
        o,
        plan.as_ref().map(|p| p.cost),
        gold,
    );
    if let Some(b) = m.str("ascendItemButton").and_then(|b| ui.record(b)) {
        let ok = trade
            .slot
            .as_ref()
            .is_some_and(|it| crate::reroll::kind_state(&idb, it, kind, false).is_ok())
            && plan
                .as_ref()
                .is_some_and(|p| crate::reroll::shortfall(p, gold, inv, stash).is_none());
        action_button(ui, &b, o, ok, Button::Go, layout);
    }
}

/// The Illusionist's window (`transmuter_mastertable.dbr`, logic
/// `crate::illusion`): background, title, close button, the 6 × 3 look boxes
/// (`displayBox` at `displayBoxSpacingX/Y`; dark plate, the look's icon,
/// `selectionBorder` on the shown look, the window's `transmutedBitmap` on a
/// pending pick), the page buttons and "n / m" (`tagTransmutePageFormat`),
/// the nine equipment boxes (worn icon or silhouette, `selectionBorder` on the
/// selected one), the iron totals and the
/// Apply Illusion button (enabled with an affordable pick). PORT: the box
/// plate shade (`backgroundShadeColor*`) as a plain overlay. The character
/// view shows the paper doll with the pending picks (`crate::doll`).
fn illusionist_window(
    ui: &mut Ui,
    trade: &Trade,
    (gold, level): (u64, u32),
    inv: &Inventory,
    layout: &mut TradeLayout,
) {
    use crate::illusion::{EQUIP_BOXES, PAGE};
    let Some(m) = ui.record(crate::illusion::WINDOW) else {
        return;
    };
    let w = window(ui, &m);
    let o = Vec2::new(w.x, w.y);
    layout.window = Some(w);
    let v = &trade.illusion;
    draw_field(ui, &m, "backgroundBitmap", o);
    // The paper doll with the pending picks (`crate::doll`).
    if let Some(path) = m.str("characterView")
        && let Some(Widget::SceneView { rect, .. }) = ui.widget(path)
    {
        let r = URect::new(
            o.x + ui.sc(rect.x),
            o.y + ui.sc(rect.y),
            ui.sc(rect.w),
            ui.sc(rect.h),
        );
        crate::doll::panel(ui, crate::doll::DollView::Illusionist, path, r);
    }
    let rotate = ["rotateLeftButton", "rotateRightButton"].map(|f| {
        let w = m.str(f).and_then(|p| ui.widget(p));
        crate::doll::rotate_button(ui, w, o)
    });
    crate::doll::rotate_buttons(ui, crate::doll::DollView::Illusionist, rotate);
    for f in ["totalGoldBoxBitmap", "costBoxBitmap"] {
        draw_field(ui, &m, f, o);
    }
    // The weapon-swap button (the loader reads only `equipSwap1LeftButton`):
    // down while set II is worn (`crate::illusion`).
    if let Some(Widget::Button {
        up: Some(up),
        down,
        x,
        y,
        ..
    }) = m.str("equipSwap1LeftButton").and_then(|p| ui.widget(p))
    {
        let bitmap = down.filter(|_| inv.alternate).unwrap_or(up);
        if let Some((_, r)) = ui.bitmap(&bitmap, x, y, o) {
            layout.swap_button = Some(r);
            layout.buttons.push((Button::IllusionSwap, r));
            if let Some(p) = m.str("equipSwap1LeftButton") {
                ui.record_path_button_sound(r, p);
            }
        }
    }
    if let Some(b) = m.str("closeButton").and_then(|b| ui.record(b))
        && let Some((h, r)) = button_rect(ui, &b, "bitmapNameUp", o)
    {
        ui.quad(h, r, None, Color::WHITE);
        layout.buttons.push((Button::Close, r));
        ui.record_button_sound(r, &b);
    }
    if let Some(t) = m.str("windowTitle").and_then(|t| ui.record(t)) {
        let text = t
            .str("textTag")
            .map(|k| ui.fonts.tag(k).to_string())
            .unwrap_or_default();
        ui.text(&gd_formats::ui::TextBox::from_record(&t), o, &text);
    }
    let shown = v.shown_look(inv);
    // The look boxes.
    if let Some(b) = m.str("displayBox").and_then(|b| ui.record(b)) {
        let base = item_box(ui, &b, o);
        let cols = m.i32("displayBoxColumns").unwrap_or(6).max(1) as usize;
        let (dx, dy) = (
            ui.sc(m.f32("displayBoxSpacingX").unwrap_or(90.0)),
            ui.sc(m.f32("displayBoxSpacingY").unwrap_or(151.0)),
        );
        let shade = Color::srgba(
            b.f32("backgroundShadeColorRed").unwrap_or(0.0),
            b.f32("backgroundShadeColorGreen").unwrap_or(0.0),
            b.f32("backgroundShadeColorBlue").unwrap_or(0.0),
            b.f32("backgroundShadeColorAlpha").unwrap_or(0.75),
        );
        let border = b.str("selectionBorder").map(str::to_owned);
        let picked = v
            .slot
            .and_then(|s| v.picks.iter().find(|(p, _)| *p == s))
            .map(|(_, l)| l.clone());
        for i in 0..PAGE {
            let r = URect::new(
                base.x + (i % cols) as f32 * dx,
                base.y + (i / cols) as f32 * dy,
                base.w,
                base.h,
            );
            ui.solid(r, shade);
            let n = v.page * PAGE + i;
            let Some(look) = v.list.get(n) else { continue };
            if picked
                .as_deref()
                .is_some_and(|p| p.eq_ignore_ascii_case(look))
                && let Some(t) = m.str("transmutedBitmap").map(str::to_owned)
                && let Some((h, _)) = ui.image(&t)
            {
                ui.quad_stretch(h, r, Color::WHITE);
            }
            let item = Item {
                base_name: look.clone(),
                stack_count: 1,
                var1: 1,
                ..Default::default()
            };
            super::character::item_icon(ui, inv, &item, r);
            if shown
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(look))
                && let Some((h, _)) = border.as_deref().and_then(|t| ui.image(t))
            {
                ui.quad_stretch(h, r, Color::WHITE);
            }
            layout.boxes.push((item, r));
            layout.buttons.push((Button::IllusionPick(n), r));
        }
    }
    for (field, next) in [("previousPageButton", false), ("nextPageButton", true)] {
        if let Some(b) = m.str(field).and_then(|b| ui.record(b))
            && let Some((h, r)) = button_rect(ui, &b, "bitmapNameUp", o)
        {
            ui.quad(h, r, None, Color::WHITE);
            layout.buttons.push((Button::IllusionPage(next), r));
            ui.record_button_sound(r, &b);
        }
    }
    let fmt = ui.fonts.tag("tagTransmutePageFormat").to_string();
    let page = gd_sim::items::tooltip::format(
        &fmt,
        &[
            gd_sim::items::tooltip::Arg::Num((v.page + 1) as f32),
            gd_sim::items::tooltip::Arg::Num(v.pages() as f32),
        ],
    );
    text_field(ui, &m, "pageText", o, &page);
    // The equipment boxes.
    for (field, slot) in EQUIP_BOXES {
        let Some(b) = m.str(field).and_then(|b| ui.record(b)) else {
            continue;
        };
        let r = item_box(ui, &b, o);
        match inv.equipped[slot].clone() {
            Some(it) => {
                let look = Item {
                    base_name: gd_sim::items::illusion::look_record(&it).to_owned(),
                    stack_count: 1,
                    var1: 1,
                    ..Default::default()
                };
                super::character::item_icon(ui, inv, &look, r);
                layout.boxes.push((it, r));
                layout.buttons.push((Button::IllusionSlot(slot), r));
            }
            None => {
                if let Some((h, size)) = b
                    .str("silhouette")
                    .map(str::to_owned)
                    .and_then(|t| ui.image(&t))
                {
                    let (sw, sh) = (ui.sc(size.x as f32), ui.sc(size.y as f32));
                    ui.quad(
                        h,
                        URect::new(
                            r.x + ((r.w - sw) * 0.5).trunc(),
                            r.y + ((r.h - sh) * 0.5).trunc(),
                            sw,
                            sh,
                        ),
                        None,
                        Color::WHITE,
                    );
                }
            }
        }
        if v.slot == Some(slot)
            && let Some((h, _)) = b
                .str("selectionBorder")
                .map(str::to_owned)
                .and_then(|t| ui.image(&t))
        {
            ui.quad_stretch(h, r, Color::WHITE);
        }
    }
    // Iron.
    let cost = gd_game::illusion::picks_cost(&ui.data.db, inv, &v.picks, level);
    for (label, number) in [
        ("totalGoldText", Some(("totalGoldNumber", gold))),
        ("costText", None),
    ] {
        let tag = m
            .str(label)
            .and_then(|t| ui.record(t))
            .and_then(|t| t.str("textTag").map(str::to_owned))
            .map(|t| ui.fonts.tag(&t).to_string())
            .unwrap_or_default();
        text_field(ui, &m, label, o, &tag);
        if let Some((f, n)) = number {
            text_field(ui, &m, f, o, &super::character::money_text(n));
        }
    }
    text_field(
        ui,
        &m,
        if gold >= cost {
            "costNumber"
        } else {
            "tooExpensiveNumber"
        },
        o,
        &super::character::money_text(cost),
    );
    if let Some(b) = m.str("transmuteButton").and_then(|b| ui.record(b)) {
        action_button(
            ui,
            &b,
            o,
            !v.picks.is_empty() && gold >= cost,
            Button::Go,
            layout,
        );
    }
}
