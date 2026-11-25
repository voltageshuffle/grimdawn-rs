//! The character sheet's armour breakdown window (`tab1CombinedArmorRollover`
//! = `charinfo_statsarmortotalrolloverstyle.dbr`, template
//! `combinedarmorrolloverwindow.tpl`); its numbers are
//! [`gd_sim::sheet::armor_breakdown`].
//!
//! Grim Dawn.exe:
//! - load (`ArmorWindow::Load`): `rolloverStyle`, `unprotectedTag` and the
//!   `bonusColor*` (both loaded, never read), `unprotectedColor*` (alpha 1),
//!   `minSpacingBetweenColumns`, `edgeClearance`, `verticalTextLineSpacing`,
//!   the text widgets `titleText` (the armour column's title), `hitTitleText`,
//!   `absorptionTitle`, per region `<r>Text` / `<r>Number` / `<r>HitNumber` /
//!   `<r>AbsorptionNumber` (head, chest, hands, legs, feet, shoulders) and
//!   `infoText`; then the layout;
//! - values (page I update `CharacterWindow::UpdatePage1` → `ArmorWindow::SetRegionArmor` armour `itow`,
//!   `ArmorWindow::SetRegionAbsorption` chance / absorption `tagFormatPercent`): a 0 shows in the
//!   unprotected colour (the widget's colour override), others in the style's;
//! - layout (`ArmorWindow::Layout`, rerun by the page's mouse handler `CharacterSheetPage1::HandleMouseEvent`
//!   each time it shows the window): [`layout`];
//! - placement (`CharacterSheetPage1::HandleMouseEvent`, hovering `tab1CombinedArmorNumber` or
//!   `tab1CombinedArmorText`): the window's right edge at the cursor x, its top
//!   0.8 × its height above the cursor; not kept on screen;
//! - draw (`ArmorWindow::Render`): once the style's `delay` has passed, the style's
//!   background colour over the window rect, its border bitmaps around it,
//!   then every text widget.
use super::Ui;
use super::text::FontCache;
use bevy::prelude::*;
use gd_formats::ui::{Rect as URect, TextAlign, TextBox};
use gd_sim::sheet::ArmorBreakdown;

/// (record field stem, index into [`ArmorBreakdown::rows`]), in `ArmorWindow::Layout` order.
pub const ROWS: [(&str, usize); 6] = [
    ("head", 0),
    ("shoulders", 5),
    ("chest", 1),
    ("hands", 2),
    ("legs", 3),
    ("feet", 4),
];

/// What [`layout`] measures (UI units).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Measures {
    pub names: [f32; 6],
    /// The widest armour, chance and absorption number.
    pub armor: f32,
    pub hit: f32,
    pub absorption: f32,
    /// `textBoxXSize` of `titleText` (armour), `hitTitleText`, `absorptionTitle`.
    pub title_w: [f32; 3],
    pub title_h: f32,
    pub row_h: [f32; 6],
    /// `infoText`'s box height before it is wrapped.
    pub info_h: f32,
    pub edge: f32,
    pub spacing: f32,
    pub line_spacing: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Layout {
    pub size: Vec2,
    pub titles: [Vec2; 3],
    /// x of the armour, chance and absorption columns (each as wide as its title).
    pub columns: [f32; 3],
    pub name_x: f32,
    pub rows: [(f32, f32); 6],
    /// `infoText`'s position and wrap width.
    pub info: (Vec2, f32),
}

/// `ArmorWindow::Layout`: columns sit from the right edge (clearance, absorption,
/// spacing, armour, spacing, chance); the height counts the info box's record
/// height (not its wrapped text) and three clearances.
pub fn layout(m: &Measures) -> Layout {
    let name_w = m.names.iter().copied().fold(0.0, f32::max).trunc();
    let [ta, th, tabs] = m.title_w.map(f32::trunc);
    let (e, sp) = (m.edge, m.spacing);
    let e2 = 2.0 * e;
    let w = (ta + th + tabs + name_w + e2 + 2.0 * sp)
        .max(name_w + m.armor.trunc() + m.absorption.trunc() + m.hit.trunc() + 3.0 * sp + e2);
    let abs_x = w - e - tabs;
    let armor_x = abs_x - sp - ta;
    let hit_x = armor_x - sp - th;
    let hit_title = Vec2::new(e2 + name_w, e);
    let armor_title = Vec2::new(th + name_w + e2 + sp, e);
    let abs_title = Vec2::new(armor_title.x + ta + sp, e);
    let mut y = (e + m.title_h).trunc();
    let mut rows = [(0.0, 0.0); 6];
    for (r, h) in rows.iter_mut().zip(m.row_h) {
        y += m.line_spacing;
        *r = (y, h);
        y += h;
    }
    let h = e + m.title_h + m.row_h.iter().map(|h| h + m.line_spacing).sum::<f32>() + m.info_h + e2;
    Layout {
        size: Vec2::new(w, h),
        titles: [armor_title, hit_title, abs_title],
        columns: [armor_x, hit_x, abs_x],
        name_x: e2,
        rows,
        info: (Vec2::new(e2, y + m.line_spacing), w - e2),
    }
}

/// (text, shown in the unprotected colour).
pub fn number_text(fonts: &FontCache, v: i32, percent: bool) -> (String, bool) {
    use gd_sim::items::tooltip::{Arg, format_rich};
    let t = if percent {
        format_rich(fonts.tag("tagFormatPercent"), &[Arg::Num(v as f32)])
    } else {
        v.to_string()
    };
    (t, v == 0)
}

fn text_box(ui: &Ui, path: &str) -> Option<TextBox> {
    ui.data.db.get(path).map(TextBox::from_record)
}

pub fn draw(ui: &mut Ui, rec: &gd_formats::arz::Record, armor: &ArmorBreakdown, cursor: Vec2) {
    let field = |k: &str| rec.str(k).unwrap_or("");
    let int = |k: &str| rec.i32(k).unwrap_or(0) as f32;
    let unprotected = [
        rec.f32("unprotectedColorRed").unwrap_or(0.0),
        rec.f32("unprotectedColorGreen").unwrap_or(0.0),
        rec.f32("unprotectedColorBlue").unwrap_or(0.0),
    ];
    let s = ui.s;
    let titles = ["titleText", "hitTitleText", "absorptionTitle"].map(|k| text_box(ui, field(k)));
    let title_text = |ui: &Ui, tb: Option<&TextBox>| {
        tb.and_then(|t| t.tag.as_deref())
            .map(|t| ui.fonts.tag(t).to_owned())
            .unwrap_or_default()
    };
    let mut cells: Vec<[(Option<TextBox>, String, bool); 4]> = Vec::new();
    let mut m = Measures {
        title_w: titles
            .each_ref()
            .map(|t| t.as_ref().map_or(0.0, |t| t.rect.w)),
        title_h: titles
            .iter()
            .flatten()
            .map(|t| t.rect.h)
            .fold(0.0, f32::max),
        edge: int("edgeClearance"),
        spacing: int("minSpacingBetweenColumns"),
        line_spacing: int("verticalTextLineSpacing"),
        ..Default::default()
    };
    let width = |ui: &mut Ui, tb: &Option<TextBox>, t: &str| {
        tb.as_ref()
            .and_then(|b| b.style.as_deref())
            .map_or(0.0, |st| (ui.text_width(st, t) / s).ceil())
    };
    for (i, (stem, v)) in ROWS.iter().enumerate() {
        let row = armor.rows[*v];
        let name_tb = text_box(ui, field(&format!("{stem}Text")));
        let name = title_text(ui, name_tb.as_ref());
        let (a, a0) = number_text(ui.fonts, row.armor, false);
        let (c, c0) = number_text(ui.fonts, row.chance, true);
        let (b, b0) = number_text(ui.fonts, row.absorption, true);
        let tbs = ["Number", "HitNumber", "AbsorptionNumber"]
            .map(|k| text_box(ui, field(&format!("{stem}{k}"))));
        m.names[i] = width(ui, &name_tb, &name);
        m.armor = m.armor.max(width(ui, &tbs[0], &a));
        m.hit = m.hit.max(width(ui, &tbs[1], &c));
        m.absorption = m.absorption.max(width(ui, &tbs[2], &b));
        let [t0, t1, t2] = tbs;
        let cell = [
            (name_tb, name, false),
            (t0, a, a0),
            (t1, c, c0),
            (t2, b, b0),
        ];
        m.row_h[i] = cell
            .iter()
            .filter_map(|(tb, _, _)| {
                tb.as_ref()?
                    .style
                    .as_deref()
                    .and_then(|st| ui.fonts.style(ui.data, st))
            })
            .map(|st| st.size as f32)
            .fold(0.0, f32::max);
        cells.push(cell);
    }
    let info_tb = text_box(ui, field("infoText"));
    m.info_h = info_tb.as_ref().map_or(0.0, |t| t.rect.h);
    let l = layout(&m);
    let size = l.size * s;
    let origin = Vec2::new(
        (cursor.x - size.x).round(),
        (cursor.y - size.y * 0.8).round(),
    );
    if let Some(st) = rec.str("rolloverStyle").and_then(|p| ui.record(p)) {
        let f = |k: &str, d: f32| st.f32(k).unwrap_or(d);
        let bg = Color::srgba(
            f("backgroundColorRed", 0.03),
            f("backgroundColorGreen", 0.03),
            f("backgroundColorBlue", 0.04),
            f("backgroundColorAlpha", 0.9),
        );
        let r = URect::new(origin.x, origin.y, size.x, size.y);
        ui.solid(r, bg);
        ui.border(&st, "", r, 1.0);
    }
    for (tb, pos) in titles.iter().zip(l.titles) {
        if let Some(tb) = tb {
            let t = title_text(ui, Some(tb));
            let tb = TextBox {
                rect: URect::new(pos.x, pos.y, tb.rect.w, tb.rect.h),
                ..tb.clone()
            };
            ui.text(&tb, origin, &t);
        }
    }
    for (i, cell) in cells.iter().enumerate() {
        let (y, h) = l.rows[i];
        let xs = [
            (l.name_x, m.names[i]),
            (l.columns[0], m.title_w[0]),
            (l.columns[1], m.title_w[1]),
            (l.columns[2], m.title_w[2]),
        ];
        for ((tb, text, zero), (x, w)) in cell.iter().zip(xs) {
            let Some(tb) = tb else { continue };
            let Some(st) = tb.style.as_deref() else {
                continue;
            };
            let placed = TextBox {
                rect: URect::new(x, y, w, h),
                ..tb.clone()
            };
            ui.text_colored(&placed, st, origin, text, zero.then_some(unprotected));
        }
    }
    // `TextWidget::WrapToWidth`.
    if let Some(tb) = &info_tb
        && let Some(st) = tb.style.as_deref()
    {
        let text = tb
            .tag
            .as_deref()
            .map(|t| ui.fonts.tag(t).to_owned())
            .unwrap_or_default()
            .replace("{^n}", "\n")
            .replace("^n", "\n");
        let mut y = origin.y + l.info.0.y * s;
        for line in ui.wrap_lines(st, l.info.1 * s, &text) {
            let x = match tb.align_x {
                TextAlign::Start => origin.x + l.info.0.x * s,
                _ => {
                    origin.x
                        + l.info.0.x * s
                        + ((l.info.1 * s - ui.text_width(st, &line))
                            * if tb.align_x == TextAlign::End {
                                1.0
                            } else {
                                0.5
                            })
                        .trunc()
                }
            };
            ui.text_line(st, Vec2::new(x, y), &line, 1.0);
            y += ui.line_height(st);
        }
    }
}
