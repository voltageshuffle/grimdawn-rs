//! `SlotSelectWindow` pick lists (`slotselectwindow.tpl` records): the
//! potions window's modifier list (`skillSelect`, `ui/potions.rs`) and the
//! HUD's hot slot list (`hudSlotSelectWindow`, `ui/hotbar_pick.rs`).
//!
//! `SlotSelectWindow` (vtable off_140318388; loader sub_1401D53D0,
//! layout sub_1401D4960 / sub_1401D5030, draw sub_1401D5680, mouse
//! sub_1401D5A30; the HUD's subclass vtable off_14031D418, layout
//! sub_140270B50, mouse sub_14026F8F0) and its picks (sub_1401E4190; loader
//! sub_1401E44F0, size sub_1401E47B0, draw sub_1401E4A40):
//! - one column per `maxVerticalSkills` picks, every column as wide as the
//!   widest pick (icon + max(`textBoxXSize`, text + 10)), rows the icon's
//!   height, `button{X,Y}PixelSeparator` between columns / rows;
//! - the background colour rect with the `genericrolloverbox_*` frame
//!   outside it, kept a frame corner + 25 px inside the screen;
//! - a hovered pick tints the pick template's `displayBitmap`
//!   (1, 1, 1, 0.55) over its text part (right of the icon); [`draw_live`] draws the tint hidden per
//!   row ([`PickHover`]) for the owner window's live system to show (no rebuild on hover);
//! - an empty pick (no option) shows the template's `displayBitmap` as its
//!   icon and its `displayTag` text (sub_1401E47B0).
//! - a pick's look ([`PickLook`], update sub_1401E4FC0, draw sub_1401E4A40,
//!   hit test sub_1401E4C50): a disabled pick (+441) draws its icon with the
//!   `normal` shader tinted (0.3, 0.3, 0.3), its text in (1, 0, 0) (the text
//!   widget's colour override) and is never hovered or picked; a selected
//!   one (+442) gets a (0.4, 0.6, 1, 0.15) rect over the row (colour +528,
//!   set by the constructor sub_1401E4190) and its text in (0.4, 0.6, 1).
//!
//! - the HUD list pages its columns (layout sub_140270B50, draw
//!   sub_14026F490, mouse sub_14026F8F0, next sub_140270E50): when
//!   `cols × (col_w + gap)` exceeds the screen width less 50 px, only
//!   `(width − 50) / col_w` columns from the first shown one are drawn and
//!   the list grows by one row height for the `prevTemplate` /
//!   `nextTemplate` picks (their `displayBitmap` icon and `displayTag`
//!   text, a column wide) in its bottom row: previous at the left (while
//!   the first column > 0), next at the right edge (while columns remain).
//!   Previous steps back a page (floored at 0), next forward (capped at
//!   `cols − page`); the first column resets when the column count changes.
use super::Ui;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::ui::Rect as URect;

/// One pick: its icon, text, whether it gets `selectedBorder` and its look.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pick {
    pub icon: Option<String>,
    pub name: String,
    pub selected: bool,
    pub look: PickLook,
}

/// A pick's row look (module doc; the devotion list sets them, sub_1401D4C60).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PickLook {
    #[default]
    Normal,
    /// +441: greyed, not pickable.
    Disabled,
    /// +442: the current choice.
    Selected,
}

/// A pick row's hidden hover tint ([`draw_live`]): the owner window's live system shows the hovered
/// row's, so hovering the list does not rebuild the window.
#[derive(Component)]
pub struct PickHover(pub usize);

/// A paged list's arrow hidden hover tint ([`draw_paged`]): previous (`false`) / next (`true`), shown
/// in place by the owner window's live system.
#[derive(Component)]
pub struct ArrowHover(pub bool);

/// Which hidden hover tint a live-drawn pick gets.
#[derive(Clone, Copy)]
enum LiveTint {
    Row(usize),
    Arrow(bool),
}

/// Where the list goes relative to its anchor (the clicked slot), less the
/// record's `windowLeftPixelOffset` / `windowBottomPixelOffset`.
#[derive(Clone, Copy, Debug)]
pub enum Place {
    /// The potions list: its top at the slot's bottom-left corner.
    Below(URect),
    /// The HUD list: its bottom at the slot's top (sub_14026F8F0: y = slot
    /// y − bottom offset − list height).
    Above(URect),
    /// The base window's own placement (draw sub_1401D5680, flag +1240 set by
    /// the constructor sub_1401D5100 and cleared by nothing): centred on the
    /// point (the devotion window's star centre).
    Centred(Vec2),
}

const FRAME: [&str; 8] = [
    "ui/generic/genericrolloverbox_cornertopleft.tex",
    "ui/generic/genericrolloverbox_cornertopright.tex",
    "ui/generic/genericrolloverbox_cornerbottomleft.tex",
    "ui/generic/genericrolloverbox_cornerbottomright.tex",
    "ui/generic/genericrolloverbox_edgetop.tex",
    "ui/generic/genericrolloverbox_edgebottom.tex",
    "ui/generic/genericrolloverbox_edgeleft.tex",
    "ui/generic/genericrolloverbox_edgeright.tex",
];

/// The pick template record's empty pick (`displayBitmap`, `displayTag`
/// localized).
pub fn empty_pick(ui: &mut Ui, sel: &Record) -> Pick {
    let pick = ui.record(sel.str("pickTemplate").unwrap_or(""));
    let name = pick
        .as_ref()
        .and_then(|r| r.str("displayTag"))
        .map(|t| ui.fonts.tag(t).to_string())
        .unwrap_or_default();
    Pick {
        icon: pick
            .as_ref()
            .and_then(|r| r.str("displayBitmap"))
            .map(str::to_owned),
        name,
        selected: false,
        look: PickLook::Normal,
    }
}

/// Paging input of the HUD list: the first shown column.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Paging {
    pub first: usize,
}

/// A paged list as drawn: the clamped first column, the page size (shown
/// columns), the column count and the arrows' rects.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Paged {
    pub first: usize,
    pub page: usize,
    pub cols: usize,
    pub prev: Option<URect>,
    pub next: Option<URect>,
}

impl Paged {
    /// The first column after a previous (`false`) / next (`true`) click
    /// (sub_14026F8F0 / sub_140270E50).
    pub fn step(&self, next: bool) -> usize {
        if next {
            (self.first + self.page).min(self.cols.saturating_sub(self.page))
        } else {
            self.first.saturating_sub(self.page)
        }
    }
}

/// Lays out and draws the list of `picks` for the window record `sel`;
/// `hover` = the hovered pick, `border` = the `selectedBorder` bitmap.
/// Returns the list rect and every pick's row rect.
pub fn draw(
    ui: &mut Ui,
    sel: &Record,
    picks: &[Pick],
    place: Place,
    hover: Option<usize>,
    border: Option<&str>,
) -> (URect, Vec<URect>) {
    let (lr, rows, _) = draw_paged_live(ui, sel, picks, place, hover, border, None, false);
    (lr, rows)
}

/// [`draw`] whose hover tint moves in place: every enabled row gets a hidden [`PickHover`] quad the
/// owner window's live system shows (no `hover` argument, no rebuild on hover).
pub fn draw_live(
    ui: &mut Ui,
    sel: &Record,
    picks: &[Pick],
    place: Place,
    border: Option<&str>,
) -> (URect, Vec<URect>) {
    let (lr, rows, _) = draw_paged_live(ui, sel, picks, place, None, border, None, true);
    (lr, rows)
}

/// [`draw_live`] with the HUD list's paging (`None` = the base window, no
/// paging): the rows' and the arrows' hover tints are hidden ([`PickHover`], [`ArrowHover`]) for the
/// owner window's live system. Picks off the page get an empty rect.
pub fn draw_paged(
    ui: &mut Ui,
    sel: &Record,
    picks: &[Pick],
    place: Place,
    border: Option<&str>,
    paging: Option<Paging>,
) -> (URect, Vec<URect>, Paged) {
    draw_paged_live(ui, sel, picks, place, None, border, paging, true)
}

/// [`draw_paged`] with the live-hover mode ([`draw_live`]).
// The paging draw's seven arguments plus the live-hover flag.
#[allow(clippy::too_many_arguments)]
fn draw_paged_live(
    ui: &mut Ui,
    sel: &Record,
    picks: &[Pick],
    place: Place,
    hover: Option<usize>,
    border: Option<&str>,
    paging: Option<Paging>,
    live: bool,
) -> (URect, Vec<URect>, Paged) {
    let pick = ui.record(sel.str("pickTemplate").unwrap_or(""));
    let f = |k: &str, d: f32| sel.f32(k).unwrap_or(d);
    let p = |k: &str, d: f32| pick.as_ref().and_then(|r| r.f32(k)).unwrap_or(d);
    let style = pick
        .as_ref()
        .and_then(|r| r.str("style"))
        .map(str::to_owned)
        .unwrap_or_default();
    let (icon_w, icon_h) = (
        ui.sc(p("bitmapWidth", 32.0)),
        ui.sc(p("bitmapHeight", 32.0)),
    );
    let (gap_x, gap_y) = (
        ui.sc(f("buttonXPixelSeparator", 2.0)),
        ui.sc(f("buttonYPixelSeparator", 2.0)),
    );
    let rows_max = (f("maxVerticalSkills", 11.0) as usize).max(1);
    let col_w = picks
        .iter()
        .map(|k| {
            icon_w
                + ui.sc(p("textBoxXSize", 150.0))
                    .max(ui.text_width(&style, &k.name) + 10.0)
        })
        .fold(1.0f32, f32::max);
    let rows = picks.len().clamp(1, rows_max);
    let cols = picks.len().div_ceil(rows_max).max(1);
    let mut paged = Paged {
        first: 0,
        page: cols,
        cols,
        prev: None,
        next: None,
    };
    if let Some(pg) = paging {
        paged.first = if pg.first < cols { pg.first } else { 0 };
        let room = (ui.screen.x - ui.sc(50.0)).max(0.0);
        if cols as f32 * (col_w + gap_x) > room {
            paged.page = ((room / col_w) as usize).max(1);
        } else {
            paged.first = 0;
        }
    }
    let (first, shown) = (
        paged.first,
        paged.page.min(cols - paged.first.min(cols - 1)),
    );
    let (has_prev, has_next) = (
        paged.page < cols && first > 0,
        paged.page < cols && first + paged.page < cols,
    );
    let lw = shown as f32 * col_w + (shown - 1) as f32 * gap_x;
    let lh = rows as f32 * icon_h
        + (rows - 1) as f32 * gap_y
        + if has_prev || has_next { icon_h } else { 0.0 };
    let corner = |ui: &mut Ui, i: usize| {
        let s = ui.image(FRAME[i]).map_or(UVec2::ZERO, |(_, s)| s);
        Vec2::new(ui.sc(s.x as f32), ui.sc(s.y as f32))
    };
    let (mx, my) = (
        corner(ui, 1).x.round() + 25.0,
        corner(ui, 0).y.round() + 25.0,
    );
    let (left, bottom) = (
        ui.sc(f("windowLeftPixelOffset", 0.0)),
        ui.sc(f("windowBottomPixelOffset", 0.0)),
    );
    let (ax, ay) = match place {
        Place::Below(a) => (a.x - left, a.y + a.h - bottom),
        Place::Above(a) => (a.x - left, a.y - bottom - lh),
        Place::Centred(p) => (p.x - left - lw * 0.5, p.y - bottom - lh * 0.5),
    };
    let x = ax.max(mx).min(ui.screen.x - (mx + lw));
    let y = ay.max(my).min(ui.screen.y - (lh + my));
    let lr = URect::new(x, y, lw, lh);
    ui.solid(
        lr,
        Color::srgba(
            f("backgroundColorRed", 0.08),
            f("backgroundColorGreen", 0.08),
            f("backgroundColorBlue", 0.08),
            f("backgroundAlpha", 0.9),
        ),
    );
    ui.border_paths(&FRAME, lr, 1.0);
    let hover_bitmap = pick
        .as_ref()
        .and_then(|r| r.str("displayBitmap"))
        .map(str::to_owned);
    let (text_x, text_h) = (ui.sc(p("textBoxX", 37.0)), ui.sc(p("textBoxYSize", 32.0)));
    let text_lh = ui.line_height(&style);
    let draw_pick =
        |ui: &mut Ui, k: &Pick, cx: f32, cy: f32, hovered: bool, t: Option<LiveTint>| {
            let tint_rect = URect::new(cx + icon_w, cy, col_w - icon_w, icon_h);
            if let Some(t) = t {
                if k.look != PickLook::Disabled
                    && let Some((img, _)) = hover_bitmap.as_deref().and_then(|p| ui.image(p))
                {
                    let e = ui.quad_stretch(img, tint_rect, Color::srgba(1.0, 1.0, 1.0, 0.55));
                    let mut e = ui.commands.entity(e);
                    e.insert(Visibility::Hidden);
                    match t {
                        LiveTint::Row(i) => e.insert(PickHover(i)),
                        LiveTint::Arrow(next) => e.insert(ArrowHover(next)),
                    };
                }
            } else if hovered
                && let Some((img, _)) = hover_bitmap.as_deref().and_then(|p| ui.image(p))
            {
                ui.quad_stretch(img, tint_rect, Color::srgba(1.0, 1.0, 1.0, 0.55));
            }
            let disabled = k.look == PickLook::Disabled;
            if let Some((img, _)) = k.icon.as_deref().and_then(|p| ui.image(p)) {
                let tint = if disabled {
                    Color::srgb(0.3, 0.3, 0.3)
                } else {
                    Color::WHITE
                };
                ui.quad_stretch(img, URect::new(cx, cy, icon_w, icon_h), tint);
            }
            if k.look == PickLook::Selected {
                ui.solid(
                    URect::new(cx, cy, col_w, icon_h),
                    Color::srgba(0.4, 0.6, 1.0, 0.15),
                );
            }
            if k.selected
                && let Some((img, _)) = border.and_then(|p| ui.image(p))
            {
                ui.quad_stretch(img, URect::new(cx, cy, icon_w, icon_h), Color::WHITE);
            }
            let color = match k.look {
                PickLook::Normal => None,
                PickLook::Disabled => Some([1.0, 0.0, 0.0]),
                PickLook::Selected => Some([0.4, 0.6, 1.0]),
            };
            ui.text_line_colored(
                &style,
                Vec2::new(cx + text_x, (cy + (text_h - text_lh) * 0.5).round()),
                &k.name,
                Color::WHITE,
                color,
            );
        };
    let mut rects = Vec::with_capacity(picks.len());
    for (i, k) in picks.iter().enumerate() {
        let col = i / rows_max;
        if col < first || col >= first + shown {
            rects.push(URect::new(0.0, 0.0, 0.0, 0.0));
            continue;
        }
        let (cx, cy) = (
            x + (col - first) as f32 * (col_w + gap_x),
            y + (i % rows_max) as f32 * (icon_h + gap_y),
        );
        draw_pick(
            ui,
            k,
            cx,
            cy,
            hover == Some(i) && k.look != PickLook::Disabled,
            live.then_some(LiveTint::Row(i)),
        );
        rects.push(URect::new(cx, cy, col_w, icon_h));
    }
    // The arrows in the bottom row (sub_14026F490).
    let ay = y + lh - icon_h;
    for (on, field, ax, next) in [
        (has_prev, "prevTemplate", x, false),
        (has_next, "nextTemplate", x + lw - col_w, true),
    ] {
        if !on {
            continue;
        }
        let rec = ui.record(sel.str(field).unwrap_or(""));
        let name = rec
            .as_ref()
            .and_then(|r| r.str("displayTag"))
            .map(|t| ui.fonts.tag(t).to_string())
            .unwrap_or_default();
        let k = Pick {
            icon: rec
                .as_ref()
                .and_then(|r| r.str("displayBitmap"))
                .map(str::to_owned),
            name,
            selected: false,
            look: PickLook::Normal,
        };
        draw_pick(ui, &k, ax, ay, false, live.then_some(LiveTint::Arrow(next)));
        let r = Some(URect::new(ax, ay, col_w, icon_h));
        if next { paged.next = r } else { paged.prev = r }
    }
    (lr, rects, paged)
}
