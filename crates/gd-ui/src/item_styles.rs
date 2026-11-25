//! The loot filter window's Item Styles page (a port addition; rules `gd_sim::items::item_style`, the
//! rules resource and file `gd_game::item_style`). The window's page button switches between the stock
//! checkboxes and this page (`super::lootfilter`).
//!
//! Left: the rules in order (the first that matches an item wins), each line its conditions in the file's
//! syntax, drawn in its colour with its tag; a click selects one, the wheel scrolls the list. Under it Add (a
//! rule matching everything, at the end), Remove, Move Up / Move Down. Right: the selected rule's editor:
//! rarity and type check boxes (none checked: any), the four traits cycling Any / Yes / No, the item level
//! range, the name substring, the colour (a palette of [`PALETTE`] swatches, Stock, or a `#RRGGBB` field),
//! the tag with its place (before / after the name), "Hide the label", and a preview line. The window's
//! bottom row has Load Preset (`StyleRules::preset`) and Clear All, each asking first (`super::lootfilter`).
//!
//! The page only reads `ItemStyles`: every change goes as an [`ItemStyleRequest`] (`Set` with the whole edited
//! rule) that gd-game applies and saves; the window rebuilds on the rules' generation and the page's own
//! [`StylesPage::version`] (selection, scroll, typing), never per frame.
//!
//! Text fields are `editbox` behaviour (`gd_sim::edit`, as the devotion search): a click on a field starts
//! typing; Enter or a click anywhere else commits the text (an invalid number or colour leaves the rule as it
//! was), Esc cancels it (the shared dispatcher, `super::esc_dispatcher`). While typing the keyboard is the
//! field's ([`typing`], `PreUpdate` ahead of every hotkey reader).
//!
//! PORT: the stock text has no strings for this page: the labels are the loot filter's own tags where one
//! fits (rarities, Sets, Monster Infrequent, Armor, Accessories, Components), English otherwise.
use super::Ui;
use super::contains;
use super::lootfilter::LootFilterView;
use bevy::prelude::*;
use gd_formats::arz::Record;
use gd_formats::ui::{Rect as URect, TextAlign, TextBox};
use gd_game::item_style::{ItemStyleRequest, ItemStyles};
use gd_sim::items::item_style::{
    Kind, PALETTE, Rarity, Rule, StyleRules, TagPos, Trait, parse_color, parse_level,
};

/// List rows shown at once.
const ROWS: usize = 13;

/// The page's state (module doc).
#[derive(Default)]
pub struct StylesPage {
    /// The page is shown instead of the checkboxes.
    pub shown: bool,
    pub selected: usize,
    scroll: usize,
    edit: Option<Edit>,
    /// Bumped by every page-local change: in the window's rebuild signature.
    pub version: u32,
    hits: Vec<(URect, Hit)>,
    list: Option<URect>,
}

struct Edit {
    field: Field,
    text: String,
    state: gd_sim::edit::EditState,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    LevelMin,
    LevelMax,
    Name,
    Tag,
    Color,
}

impl Field {
    fn max_chars(self) -> usize {
        match self {
            Field::LevelMin | Field::LevelMax => 4,
            Field::Name => 40,
            Field::Tag => 16,
            Field::Color => 7,
        }
    }

    fn value(self, r: &Rule) -> String {
        let n = |v: Option<u32>| v.map(|v| v.to_string()).unwrap_or_default();
        match self {
            Field::LevelMin => n(r.level_min),
            Field::LevelMax => n(r.level_max),
            Field::Name => r.name.clone(),
            Field::Tag => r.style.tag.clone(),
            Field::Color => r
                .style
                .color
                .map(|[a, b, c]| format!("#{a:02x}{b:02x}{c:02x}"))
                .unwrap_or_default(),
        }
    }

    /// `r` with this field set to `text`; `None` for an invalid number or colour.
    fn apply(self, r: &Rule, text: &str) -> Option<Rule> {
        let mut r = r.clone();
        let t = text.trim();
        match self {
            Field::LevelMin | Field::LevelMax => {
                let v = if t.is_empty() {
                    None
                } else {
                    Some(parse_level(t)?.0?)
                };
                if self == Field::LevelMin {
                    r.level_min = v;
                } else {
                    r.level_max = v;
                }
            }
            Field::Name => r.name = t.to_owned(),
            Field::Tag => r.style.tag = t.to_owned(),
            Field::Color => {
                r.style.color = if t.is_empty() {
                    None
                } else {
                    Some(parse_color(t)?)
                }
            }
        }
        Some(r)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Rule(usize),
    Add,
    Remove,
    Up,
    Down,
    Rarity(Rarity),
    Kind(Kind),
    Trait(Trait),
    Field(Field),
    Swatch(Option<[u8; 3]>),
    TagPos,
    Hide,
}

impl StylesPage {
    fn bump(&mut self) {
        self.version = self.version.wrapping_add(1);
    }

    /// The window closed: typing stops (uncommitted text is dropped).
    pub fn close(&mut self) {
        if self.edit.take().is_some() {
            self.bump();
        }
    }

    /// Esc while typing cancels the field (the shared dispatcher); whether it did.
    pub fn escape(&mut self) -> bool {
        let was = self.edit.take().is_some();
        if was {
            self.bump();
        }
        was
    }

    /// Every rule was replaced (preset, clear): back to the first.
    pub fn replaced(&mut self) {
        self.selected = 0;
        self.scroll = 0;
        self.edit = None;
        self.bump();
    }

    fn keep_visible(&mut self) {
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + ROWS {
            self.scroll = self.selected + 1 - ROWS;
        }
    }

    /// The wheel over the list scrolls it (`lines` > 0: up).
    pub fn wheel(&mut self, p: Vec2, lines: f32, count: usize) {
        if lines == 0.0 || !self.list.is_some_and(|r| contains(&r, p)) {
            return;
        }
        let max = count.saturating_sub(ROWS);
        let s = if lines > 0.0 {
            self.scroll.saturating_sub(1)
        } else {
            (self.scroll + 1).min(max)
        };
        if s != self.scroll {
            self.scroll = s;
            self.bump();
        }
    }
}

/// The typed text of the open field, applied to the selected rule (`None`: nothing typed, unchanged or invalid).
fn commit(page: &mut StylesPage, rules: &StyleRules) -> Option<Rule> {
    let e = page.edit.take()?;
    page.bump();
    let rule = rules.rules.get(page.selected)?;
    let new = e.field.apply(rule, &e.text)?;
    (new != *rule).then_some(new)
}

/// A left press outside the window leaves the open field, committing its text (as the devotion search box
/// leaves on a press elsewhere), so the keyboard goes back to the game.
pub fn commit_open(
    page: &mut StylesPage,
    styles: &ItemStyles,
    requests: &mut MessageWriter<ItemStyleRequest>,
) {
    if page.edit.is_some()
        && let Some(r) = commit(page, &styles.rules)
    {
        requests.write(ItemStyleRequest::Set(page.selected, r));
    }
}

/// A left press at `p` (physical px) on the page: commits an open field unless the press is on it, then acts on
/// what is under it. Whether the page took the press.
pub fn click(
    page: &mut StylesPage,
    styles: &ItemStyles,
    p: Vec2,
    requests: &mut MessageWriter<ItemStyleRequest>,
) -> bool {
    let hit = page.hits.iter().find(|(r, _)| contains(r, p)).map(|h| h.1);
    let sel = page.selected;
    let rules = &styles.rules.rules;
    let mut cur = rules.get(sel).cloned();
    if page
        .edit
        .as_ref()
        .is_some_and(|e| hit != Some(Hit::Field(e.field)))
        && let Some(r) = commit(page, &styles.rules)
    {
        requests.write(ItemStyleRequest::Set(sel, r.clone()));
        cur = Some(r);
    }
    let Some(hit) = hit else {
        return false;
    };
    let n = rules.len();
    // The selected rule edited by `f` (on top of a text committed by this press).
    let edited = |f: &dyn Fn(&mut Rule)| {
        cur.as_ref().map(|r| {
            let mut r = r.clone();
            f(&mut r);
            r
        })
    };
    let set = match hit {
        Hit::Rule(i) => {
            page.selected = i;
            None
        }
        Hit::Add => {
            requests.write(ItemStyleRequest::Add);
            page.selected = n;
            None
        }
        Hit::Remove if sel < n => {
            requests.write(ItemStyleRequest::Remove(sel));
            page.selected = sel.min(n.saturating_sub(2));
            None
        }
        Hit::Up if sel > 0 && sel < n => {
            requests.write(ItemStyleRequest::Move(sel, true));
            page.selected -= 1;
            None
        }
        Hit::Down if sel + 1 < n => {
            requests.write(ItemStyleRequest::Move(sel, false));
            page.selected += 1;
            None
        }
        Hit::Remove | Hit::Up | Hit::Down => None,
        Hit::Rarity(x) => edited(&|r| r.toggle_rarity(x)),
        Hit::Kind(k) => edited(&|r| r.toggle_kind(k)),
        Hit::Trait(t) => edited(&|r| {
            let v = &mut r.traits[t as usize];
            *v = match *v {
                None => Some(true),
                Some(true) => Some(false),
                Some(false) => None,
            };
        }),
        Hit::Swatch(c) => edited(&|r| r.style.color = c),
        Hit::TagPos => edited(&|r| {
            r.style.tag_pos = match r.style.tag_pos {
                TagPos::Prefix => TagPos::Suffix,
                TagPos::Suffix => TagPos::Prefix,
            };
        }),
        Hit::Hide => edited(&|r| r.style.hide = !r.style.hide),
        Hit::Field(f) => {
            if page.edit.is_none()
                && let Some(r) = &cur
            {
                let text = f.value(r);
                let mut state = gd_sim::edit::EditState::default();
                state.to_end(&text);
                page.edit = Some(Edit {
                    field: f,
                    text,
                    state,
                });
            }
            None
        }
    };
    if let Some(r) = set {
        requests.write(ItemStyleRequest::Set(sel, r));
    }
    page.keep_visible();
    page.bump();
    true
}

/// What the page draws with: the window's origin and rect and the loot filter records it reuses.
pub struct Look<'a> {
    pub o: Vec2,
    pub w: URect,
    /// `lootFilterCheckbox`: the check box bitmaps.
    pub checkbox: &'a Record,
    /// `lootFilterResetButton`: the small buttons' bitmap.
    pub button: Option<&'a Record>,
    /// The check boxes' label style.
    pub label: &'a str,
    /// The four stock column titles' text boxes (`lootFilterTitle01..04`: rect and style), relabelled.
    pub titles: [Option<TextBox>; 4],
}

/// A text in `r` in `style`, its colour replaced by `color` (as a text widget's override colour).
fn text_in(
    ui: &mut Ui,
    r: URect,
    style: &str,
    align: TextAlign,
    text: &str,
    color: Option<[f32; 3]>,
) {
    let tb = TextBox {
        style: None,
        rect: URect::new(r.x / ui.s, r.y / ui.s, r.w / ui.s, r.h / ui.s),
        align_x: align,
        align_y: TextAlign::Center,
        tag: None,
        glow: None,
    };
    ui.text_colored(&tb, style, Vec2::ZERO, text, color);
}

/// A small button: the reset button's bitmap stretched over `r`, the label centred (dimmed when disabled).
fn small_button(ui: &mut Ui, look: &Look, r: URect, text: &str, enabled: bool) {
    let key = if enabled {
        "bitmapNameUp"
    } else {
        "bitmapNameDisabled"
    };
    match look
        .button
        .and_then(|b| b.str(key).or(b.str("bitmapNameUp")))
        .map(str::to_owned)
        .and_then(|p| ui.image(&p))
    {
        Some((img, _)) => {
            ui.quad_stretch(img, r, Color::WHITE);
        }
        None => {
            ui.solid(r, Color::srgba(0.2, 0.18, 0.14, 0.9));
        }
    }
    let c = (!enabled).then_some([0.45; 3]);
    text_in(ui, r, look.label, TextAlign::Center, text, c);
    if enabled && let Some(b) = look.button {
        ui.record_button_sound(r, b);
    }
}

/// The word for a rarity / type / trait: the loot filter's own tag where one fits (module doc).
fn word(ui: &Ui, tag: Option<&str>, en: &'static str) -> String {
    tag.map_or_else(|| en.to_owned(), |t| ui.fonts.tag(t).to_string())
}

fn rarity_word(ui: &Ui, r: Rarity) -> String {
    let tag = match r {
        Rarity::Common => "tagLootFilter01",
        Rarity::Magic => "tagLootFilter02",
        Rarity::Rare => "tagLootFilter03",
        Rarity::Epic => "tagLootFilter05",
        Rarity::Legendary => "tagLootFilter06",
    };
    word(ui, Some(tag), "")
}

fn kind_word(ui: &Ui, k: Kind) -> String {
    match k {
        Kind::Weapon => word(ui, None, "Weapons"),
        Kind::Offhand => word(ui, None, "Off-Hands"),
        Kind::Armor => word(ui, Some("tagLootFilter16"), ""),
        Kind::Accessory => word(ui, Some("tagLootFilter17"), ""),
        Kind::Component => word(ui, Some("tagLootFilter38"), ""),
        Kind::Relic => word(ui, None, "Relics"),
        Kind::Blueprint => word(ui, None, "Blueprints"),
        Kind::Augment => word(ui, None, "Augments"),
        Kind::Consumable => word(ui, None, "Consumables"),
        Kind::Material => word(ui, None, "Materials"),
        Kind::Quest => word(ui, None, "Quest Items"),
        Kind::Lore => word(ui, None, "Lore"),
        Kind::Other => word(ui, None, "Other"),
    }
}

fn trait_word(ui: &Ui, t: Trait) -> String {
    match t {
        Trait::Set => word(ui, Some("tagLootFilter07"), ""),
        Trait::MonsterInfrequent => word(ui, Some("tagLootFilter04"), ""),
        Trait::DoubleRare => word(ui, None, "Double Rare"),
        Trait::Ascended => word(ui, None, "Ascended"),
    }
}

/// A rule's list line: number, tag, conditions ("any item" for none), "(hidden)".
fn summary(i: usize, r: &Rule) -> String {
    let cond = r.conditions();
    let cond = if cond.is_empty() {
        "any item"
    } else {
        cond.as_str()
    };
    let mut s = format!("{}. {cond}", i + 1);
    if !r.style.tag.is_empty() {
        s = match r.style.tag_pos {
            TagPos::Prefix => format!("{}. {} {cond}", i + 1, r.style.tag),
            TagPos::Suffix => format!("{s} {}", r.style.tag),
        };
    }
    if r.style.hide {
        s.push_str(" (hidden)");
    }
    s
}

/// Lays out `n` items of widths `w(i)` left to right from `x0`, wrapping at `right`; returns each one's
/// top-left and the y below the last row.
fn flow(
    n: usize,
    w: impl Fn(usize) -> f32,
    x0: f32,
    right: f32,
    y0: f32,
    row_h: f32,
) -> (Vec<Vec2>, f32) {
    let (mut x, mut y) = (x0, y0);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let wi = w(i);
        if x > x0 && x + wi > right {
            x = x0;
            y += row_h;
        }
        out.push(Vec2::new(x, y));
        x += wi;
    }
    (out, y + row_h)
}

/// Draws the page (module doc) into the window `look.w`.
pub fn build(ui: &mut Ui, page: &mut StylesPage, styles: &ItemStyles, look: &Look) {
    page.hits.clear();
    let rules = &styles.rules.rules;
    // Clamped for drawing only: the page's own selection stays (a just-sent Add selects the rule it adds).
    let sel = page.selected.min(rules.len().saturating_sub(1));
    page.scroll = page.scroll.min(rules.len().saturating_sub(ROWS));
    let o = look.o;
    let at = |ui: &Ui, x: f32, y: f32| Vec2::new(o.x + ui.sc(x), o.y + ui.sc(y));
    let label = look.label;
    let line_h = ui.line_height(label);
    // The four stock column titles, relabelled.
    let rule_of = format!("{} of {}", sel + 1, rules.len());
    let titles = [
        "Item Styles",
        "First Wins",
        "Edit Rule",
        if rules.is_empty() { "" } else { &rule_of },
    ];
    for (tb, text) in look.titles.iter().zip(titles) {
        if let (Some(tb), false) = (tb, text.is_empty()) {
            let text = match tb.style.as_deref() {
                Some(st) => ui.truncate_line(st, ui.sc(tb.rect.w), text),
                None => text.to_owned(),
            };
            ui.text(tb, o, &text);
        }
    }
    // The page's own backdrop over the content area: the stock background's column dividers (drawn for the
    // four checkbox columns) would run through the list and the editor.
    let a = at(ui, 22.0, 90.0);
    let b = at(ui, 980.0, 556.0);
    let panel = URect::new(a.x, a.y, b.x - a.x, b.y - a.y);
    ui.solid(panel, Color::srgb(0.045, 0.04, 0.032));
    let edge = Color::srgba(0.55, 0.48, 0.33, 0.8);
    let t1 = ui.sc(1.0).max(1.0);
    for r in [
        URect::new(panel.x, panel.y, panel.w, t1),
        URect::new(panel.x, panel.y + panel.h - t1, panel.w, t1),
        URect::new(panel.x, panel.y, t1, panel.h),
        URect::new(panel.x + panel.w - t1, panel.y, t1, panel.h),
        // Between the list and the editor.
        URect::new(at(ui, 382.0, 0.0).x, panel.y, t1, panel.h),
    ] {
        ui.solid(r, edge);
    }
    // The rule list.
    let p = at(ui, 30.0, 96.0);
    let row_h = ui.sc(26.0);
    let list = URect::new(p.x, p.y, ui.sc(342.0), row_h * ROWS as f32);
    page.list = Some(list);
    ui.solid(list, Color::srgba(0.0, 0.0, 0.0, 0.35));
    let pad = ui.sc(6.0);
    for (row, i) in (page.scroll..rules.len()).take(ROWS).enumerate() {
        let r = URect::new(list.x, list.y + row_h * row as f32, list.w, row_h);
        if i == sel {
            ui.solid(r, Color::srgba(0.55, 0.45, 0.25, 0.45));
        }
        let text = summary(i, &rules[i]);
        let text = ui.truncate_line(label, r.w - 2.0 * pad, &text);
        let tr = URect::new(r.x + pad, r.y, r.w - 2.0 * pad, r.h);
        text_in(ui, tr, label, TextAlign::Start, &text, rules[i].style.rgb());
        page.hits.push((r, Hit::Rule(i)));
    }
    if rules.is_empty() {
        let tr = URect::new(list.x + pad, list.y, list.w - 2.0 * pad, row_h);
        let t = ui.truncate_line(label, tr.w, "No rules: the stock look.");
        text_in(ui, tr, label, TextAlign::Start, &t, Some([0.7; 3]));
    }
    let n = rules.len();
    let by = list.y + list.h + ui.sc(8.0);
    let (bw, bh, gap) = (ui.sc(80.0), ui.sc(28.0), ui.sc(6.0));
    for (k, (hit, text, enabled)) in [
        (Hit::Add, "Add", true),
        (Hit::Remove, "Remove", sel < n),
        (Hit::Up, "Move Up", sel > 0 && sel < n),
        (Hit::Down, "Move Down", sel + 1 < n),
    ]
    .into_iter()
    .enumerate()
    {
        let r = URect::new(list.x + (bw + gap) * k as f32, by, bw, bh);
        small_button(ui, look, r, text, enabled);
        if enabled {
            page.hits.push((r, hit));
        }
    }
    let Some(rule) = rules.get(sel) else {
        return;
    };
    // The editor.
    let x0 = at(ui, 392.0, 0.0).x;
    let right = at(ui, 978.0, 0.0).x;
    let cap_w = ui.sc(90.0);
    let xc = x0 + cap_w;
    let row = ui.sc(28.0);
    let mut y = at(ui, 0.0, 96.0).y;
    let caption = |ui: &mut Ui, y: f32, text: &str| {
        let r = URect::new(x0, y, cap_w, row);
        text_in(
            ui,
            r,
            label,
            TextAlign::Start,
            text,
            Some([0.85, 0.75, 0.5]),
        );
    };
    let box_img = |ui: &mut Ui, on: bool| {
        let key = if on { "bitmapNameDown" } else { "bitmapNameUp" };
        look.checkbox
            .str(key)
            .map(str::to_owned)
            .and_then(|p| ui.image(&p))
            .map(|(img, sz)| (img, Vec2::new(ui.sc(sz.x as f32), ui.sc(sz.y as f32))))
    };
    let box_w = box_img(ui, false).map_or(ui.sc(24.0), |b| b.1.x);
    let gap = ui.sc(10.0);
    // Check boxes with labels, flowing (rarity, type, hide).
    let checks =
        |ui: &mut Ui, page: &mut StylesPage, y: f32, items: &[(String, bool, Hit)]| -> f32 {
            let widths: Vec<f32> = items
                .iter()
                .map(|(t, _, _)| box_w + ui.sc(4.0) + ui.text_width(label, t) + gap)
                .collect();
            let (pos, end) = flow(items.len(), |i| widths[i], xc, right, y, row);
            for ((t, on, hit), p) in items.iter().zip(pos) {
                let Some((img, sz)) = box_img(ui, *on) else {
                    continue;
                };
                let br = URect::new(p.x, (p.y + (row - sz.y) * 0.5).round(), sz.x, sz.y);
                ui.quad(img, br, None, Color::WHITE);
                ui.record_button_sound(br, look.checkbox);
                // Measured with the style the label is drawn in (same font, size and flags).
                let lw = ui.text_width(label, t);
                let lr = URect::new(br.x + br.w + ui.sc(4.0), p.y, lw, row);
                text_in(ui, lr, label, TextAlign::Start, t, None);
                page.hits
                    .push((URect::new(br.x, p.y, lr.x + lr.w - br.x, row), *hit));
            }
            end
        };
    caption(ui, y, "Rarity");
    let items: Vec<_> = Rarity::ALL
        .iter()
        .map(|r| (rarity_word(ui, *r), rule.has_rarity(*r), Hit::Rarity(*r)))
        .collect();
    y = checks(ui, page, y, &items) + ui.sc(2.0);
    caption(ui, y, "Type");
    let items: Vec<_> = Kind::ALL
        .iter()
        .map(|k| (kind_word(ui, *k), rule.has_kind(*k), Hit::Kind(*k)))
        .collect();
    y = checks(ui, page, y, &items) + ui.sc(2.0);
    // Traits: Any / Yes / No buttons.
    caption(ui, y, "Traits");
    let texts: Vec<String> = Trait::ALL
        .iter()
        .map(|t| {
            let v = match rule.traits[*t as usize] {
                None => "Any",
                Some(true) => "Yes",
                Some(false) => "No",
            };
            format!("{}: {v}", trait_word(ui, *t))
        })
        .collect();
    let widths: Vec<f32> = texts
        .iter()
        .map(|t| ui.text_width(label, t) + ui.sc(20.0))
        .collect();
    let (pos, end) = flow(
        texts.len(),
        |i| widths[i] + gap,
        xc,
        right,
        y,
        row + ui.sc(4.0),
    );
    for (i, p) in pos.into_iter().enumerate() {
        let r = URect::new(p.x, p.y, widths[i], row);
        small_button(ui, look, r, &texts[i], true);
        page.hits.push((r, Hit::Trait(Trait::ALL[i])));
    }
    y = end + ui.sc(2.0);
    // Text fields.
    let field = |ui: &mut Ui, page: &mut StylesPage, r: URect, f: Field| {
        ui.solid(r, Color::srgba(0.0, 0.0, 0.0, 0.6));
        let edge = Color::srgba(0.55, 0.48, 0.33, 1.0);
        let t1 = ui.sc(1.0).max(1.0);
        ui.solid(URect::new(r.x, r.y, r.w, t1), edge);
        ui.solid(URect::new(r.x, r.y + r.h - t1, r.w, t1), edge);
        let editing = page.edit.as_ref().filter(|e| e.field == f);
        let value = f.value(rule);
        let text = editing.map_or(value.as_str(), |e| e.text.as_str());
        let tx = r.x + ui.sc(4.0);
        let ty = (r.y + (r.h - line_h) * 0.5).round();
        if let Some(e) = editing {
            let pre = |ui: &mut Ui, n: usize| {
                ui.text_width(label, &e.text.chars().take(n).collect::<String>())
            };
            // PORT: the selection highlight colour (as the market search box).
            if let Some((a, b)) = e.state.selection().filter(|(a, b)| a < b) {
                let (xa, xb) = (tx + pre(ui, a), tx + pre(ui, b));
                ui.solid(
                    URect::new(xa, r.y, xb - xa, r.h),
                    Color::srgba(0.4, 0.5, 0.9, 0.5),
                );
            }
            let x = tx + pre(ui, e.state.caret.min(e.text.chars().count()));
            let c = ui.solid(URect::new(x, r.y + t1, t1, r.h - 2.0 * t1), Color::WHITE);
            ui.commands.entity(c).insert(super::trade::SearchCaret);
        }
        ui.text_line(label, Vec2::new(tx, ty), text, 1.0);
        page.hits.push((r, Hit::Field(f)));
    };
    let fh = row - ui.sc(4.0);
    let field_dy = ui.sc(2.0);
    let fy = |y: f32| y + field_dy;
    caption(ui, y, "Item Level");
    let fw = ui.sc(60.0);
    field(ui, page, URect::new(xc, fy(y), fw, fh), Field::LevelMin);
    let to = URect::new(xc + fw, y, ui.sc(36.0), row);
    text_in(ui, to, label, TextAlign::Center, "to", None);
    field(
        ui,
        page,
        URect::new(to.x + to.w, fy(y), fw, fh),
        Field::LevelMax,
    );
    y += row + ui.sc(4.0);
    caption(ui, y, "Name Has");
    field(
        ui,
        page,
        URect::new(xc, fy(y), (right - xc).min(ui.sc(300.0)), fh),
        Field::Name,
    );
    y += row + ui.sc(4.0);
    // Colour: the palette, Stock, the hex field and a sample.
    caption(ui, y, "Colour");
    let sw = ui.sc(22.0);
    let sgap = ui.sc(4.0);
    let (pos, end) = flow(PALETTE.len(), |_| sw + sgap, xc, right, y, row);
    for (c, p) in PALETTE.iter().zip(pos) {
        let r = URect::new(p.x, p.y + (row - sw) * 0.5, sw, sw);
        if rule.style.color == Some(*c) {
            let t = ui.sc(2.0);
            ui.solid(
                URect::new(r.x - t, r.y - t, r.w + 2.0 * t, r.h + 2.0 * t),
                Color::WHITE,
            );
        }
        ui.solid(r, Color::srgb_u8(c[0], c[1], c[2]));
        page.hits.push((r, Hit::Swatch(Some(*c))));
    }
    y = end + ui.sc(2.0);
    let stock = URect::new(xc, y, ui.text_width(label, "Stock") + ui.sc(20.0), row);
    small_button(ui, look, stock, "Stock", rule.style.color.is_some());
    if rule.style.color.is_some() {
        page.hits.push((stock, Hit::Swatch(None)));
    }
    let hex = URect::new(stock.x + stock.w + gap, fy(y), ui.sc(96.0), fh);
    field(ui, page, hex, Field::Color);
    y += row + ui.sc(4.0);
    // Tag and its place.
    caption(ui, y, "Tag");
    let tag = URect::new(xc, fy(y), ui.sc(150.0), fh);
    field(ui, page, tag, Field::Tag);
    let place = match rule.style.tag_pos {
        TagPos::Prefix => "Before Name",
        TagPos::Suffix => "After Name",
    };
    let pr = URect::new(
        tag.x + tag.w + gap,
        y,
        ui.text_width(label, "Before Name") + ui.sc(20.0),
        row,
    );
    small_button(ui, look, pr, place, true);
    page.hits.push((pr, Hit::TagPos));
    y += row + ui.sc(4.0);
    let items = [(
        "Hide the ground label".to_owned(),
        rule.style.hide,
        Hit::Hide,
    )];
    checks(ui, page, y, &items);
    // The preview, under the list's buttons.
    let py = by + bh + ui.sc(12.0);
    let cap = URect::new(list.x, py, ui.sc(80.0), row);
    text_in(
        ui,
        cap,
        label,
        TextAlign::Start,
        "Preview",
        Some([0.85, 0.75, 0.5]),
    );
    let mut sample = String::from("Item Name");
    rule.style.apply_tag(&mut sample);
    let r = URect::new(cap.x + cap.w, py, list.x + list.w - cap.x - cap.w, row);
    let sample = ui.truncate_line(label, r.w - ui.sc(8.0), &sample);
    let sample_w = ui.text_width(label, &sample);
    let bg = URect::new(r.x, r.y, sample_w + ui.sc(8.0), r.h);
    ui.solid(bg, Color::srgba(0.03, 0.03, 0.04, 0.75));
    text_in(
        ui,
        URect::new(r.x + ui.sc(4.0), r.y, r.w, r.h),
        label,
        TextAlign::Start,
        &sample,
        Some(rule.style.rgb().unwrap_or([1.0; 3])),
    );
}

/// The keyboard goes to the open field (module doc): `gd_sim::edit` edits the text (as the devotion search box),
/// Enter commits it ([`ItemStyleRequest::Set`]); Esc is the shared dispatcher's. Runs in `PreUpdate` ahead of every
/// hotkey reader and resets the `ButtonInput` (Escape stays for the dispatcher), so the game's keys see nothing
/// typed. PORT: the clipboard is the process's own (`trade::Clipboard`).
#[allow(clippy::too_many_arguments)]
pub fn typing(
    mut view: ResMut<LootFilterView>,
    styles: Option<Res<ItemStyles>>,
    mut requests: MessageWriter<ItemStyleRequest>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut clipboard: ResMut<crate::state::trade::Clipboard>,
    mut mods: Local<(bool, bool)>,
) {
    use bevy::input::keyboard::{Key, KeyCode as K, KeyboardInput};
    use gd_sim::edit::EditKey;
    // The held modifiers follow every key message, read or not (a release while no field is open must not leave
    // Shift / Ctrl stuck).
    let track = |mods: &mut (bool, bool), e: &KeyboardInput| match e.key_code {
        K::ShiftLeft | K::ShiftRight => mods.0 = e.state.is_pressed(),
        K::ControlLeft | K::ControlRight => mods.1 = e.state.is_pressed(),
        _ => {}
    };
    let (true, Some(styles)) = (view.open && view.styles.edit.is_some(), styles) else {
        for e in typed.read() {
            track(&mut mods, e);
        }
        return;
    };
    let page = &mut view.styles;
    let Some(edit) = page.edit.as_mut() else {
        return;
    };
    let max = edit.field.max_chars();
    let fits = |t: &str| t.chars().count() <= max;
    let mut text = edit.text.clone();
    let mut state = edit.state.clone();
    let mut enter = false;
    state.tick(&mut text, time.delta_secs() * 1000.0, &fits);
    for e in typed.read() {
        track(&mut mods, e);
        if !e.state.is_pressed() {
            state.release();
            continue;
        }
        if e.repeat {
            continue;
        }
        let (shift, ctrl) = *mods;
        let key = match (&e.logical_key, e.key_code) {
            (Key::Character(c), _) if !ctrl => {
                EditKey::Text(c.chars().filter(|ch| !ch.is_control()).collect())
            }
            (Key::Space, _) => EditKey::Text(" ".into()),
            (Key::Enter, _) => {
                enter = true;
                continue;
            }
            // Esc cancels through the shared dispatcher (`ui::esc_dispatcher`).
            _ => match super::widgets::edit_key(&e.logical_key, e.key_code, ctrl) {
                Some(k) => k,
                None => continue,
            },
        };
        if let Some(c) = state.press(&mut text, key, shift, &clipboard.0, &fits) {
            clipboard.0 = c;
        }
    }
    let changed = text != edit.text
        || state.caret != edit.state.caret
        || state.selection() != edit.state.selection();
    edit.text = text;
    edit.state = state;
    if changed {
        page.bump();
    }
    if enter && let Some(r) = commit(page, &styles.rules) {
        requests.write(ItemStyleRequest::Set(page.selected, r));
    }
    // The game's keys see nothing while a field has the keyboard (Escape stays for the dispatcher).
    super::keys::reset_but_escape(&mut keys);
}
