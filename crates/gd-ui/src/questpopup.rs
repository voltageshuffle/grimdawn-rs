//! The Quest Complete / Quest Progress window (`hudQuestRewardWindow` =
//! `records/ingameui/questrewardwindow/questrewardwindow.dbr`, `UIQuestRewardWindow`; its content is
//! `gd_game::quest_reward`).
//!
//! `Grim Dawn.exe`:
//! - opening (`QuestRewardWindow::Open`, after the rebuild `QuestRewardWindow::Rebuild`): the open sound
//!   (`openSound`), a fade in at 0.002 per ms (500 ms); a new reward while open rebuilds it (the icons are cleared,
//!   `QuestRewardWindow::ClearIcons`). The update (`QuestRewardWindow::Update`) counts a 10000 ms timer down while the
//!   game is not paused and presses Accept at 0; Accept (`acceptButton`, events 13 / 14, `QuestRewardWindow::OnAccept`)
//!   or the timer close it: the close sound (`closeSound`) and a fade out at 0.002 per ms.
//! - layout (the window rect `windowDefault*`: 450 wide, centred, 175 px above the bottom): `backgroundTop`, then
//!   `backgroundMiddle` only with both an XP line and icons, then `backgroundBottom`, stacked from the window's top;
//!   the window's height is their sum and `acceptButton` sits `acceptButtonTopEdge` (90) px above its bottom (x from
//!   the button record). The title (`questTitleString`) is `tagQuestRewardWindowTitle` /
//!   `tagQuestRewardWindowInterim`, the quest's name in `questNameString`; the XP line `tagQuestRewardXP` in
//!   `XPValue` (its glow bitmap behind the text, padded by `bitmapXPadding` / `bitmapYPadding`).
//! - the icon row (`QuestRewardWindow::LayoutIcons`): centred vertically on `iconBoxVerticalCenterLow` (153, with an
//!   XP line) or `iconBoxVerticalCenterHigh` (124); icons `iconHorizontalSpace` (20) apart, or, when the row plus two
//!   spaces is wider than `iconBoxWidth` (400), `(400 − widths) / (n + 1)` but at least `iconMinHorizontalSpace` (5);
//!   the row centred on the window. Each icon is its record's `bitmapName` (`moneyIcon`, `factionIcon`, `levelIcon`,
//!   `skillPointIcon`, `attributePointIcon`, `devotionIcon`; an item's by its class, `QuestRewardWindow::GetItemIcon`:
//!   armour, jewellery, melee / ranged weapon, consumable, artifact formula, misc); an item icon shows its count above
//!   1 at its lower right − 10 px in `gameengine.dbr`'s `itemOverlayFontName`.
//!
//! - the icons' rollovers (`rollover_lines`): text icons in their `rolloverStyleName` window at the cursor
//!   (placement mode 2), items their own tooltip.
//!
//! PORT: an item's class is read from its record's `Class`; the window does not ease with the HUD shift;
//! the rollover shows at once (no style `delay`) and keeps the screen-edge placement of its first frame.
//! A GiveRandomItem shows the items the action rolled, one icon each; a GiveItem icon is its record with
//! seed 1 (the engine shows its first rolled info item).
use super::{UiAssets, widgets};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, TextBox, WindowDef};
use gd_game::quest_reward::{QuestRewardShown, RewardIcon};
use gd_game_core::data::GameData;

const WINDOW: &str = "records/ingameui/questrewardwindow/questrewardwindow.dbr";
const ENGINE: &str = "records/game/gameengine.dbr";
const FADE_PER_MS: f32 = 0.002;
const AUTO_ACCEPT_MS: f32 = 10000.0;

pub struct QuestPopupPlugin;

impl Plugin for QuestPopupPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuestPopup>()
            .add_systems(Update, clicks.in_set(super::UiInputSet))
            .add_systems(Update, (open, input, draw).chain().in_set(super::UiPlaySet));
        #[cfg(feature = "dev")]
        app.add_systems(Update, sample.before(open).in_set(super::UiPlaySet));
    }
}

#[cfg(feature = "dev")]
fn sample(
    hooks: Res<gd_game_core::hooks::Hooks>,
    time: Res<Time>,
    data: Res<GameData>,
    mut next: Local<Option<f32>>,
    mut out: MessageWriter<QuestRewardShown>,
    mut notify: MessageWriter<gd_game_core::story::Notify>,
) {
    let Some(at) = hooks.ui.quest_reward.filter(|a| *a > 0.0) else {
        return;
    };
    let now = time.elapsed_secs();
    if next.is_none() {
        *next = Some(now + at);
    }
    if next.is_none_or(|n| now < n) {
        return;
    }
    *next = Some(now + at);
    notify.write(gd_game_core::story::Notify("tagTransferStashError".into()));
    let mut icons = vec![
        RewardIcon::Money(1500),
        RewardIcon::Faction(vec![
            ("User0".to_owned(), 250),
            ("Outlaws".to_owned(), -100),
        ]),
        RewardIcon::SkillPoints(1),
        RewardIcon::Item {
            item: Box::new(gd_formats::save::item::Item {
                base_name: "records/items/gearweapons/swords1h/a01_sword001.dbr".to_owned(),
                seed: 1,
                stack_count: 2,
                ..Default::default()
            }),
        },
    ];
    let idb = data.item_db();
    let params = gd_sim::items::LootParams::new(20, 20, 0);
    let mut rng = gd_rules::Rng(0x0BAD_C0DE);
    if let Some(r) = gd_sim::items::loot::story_random_item(
        &idb,
        "records/items/loottables/mastertables/mt_gearweapons_c01.dbr",
        &params,
        &mut rng,
    ) {
        icons.push(RewardIcon::Item {
            item: Box::new(r.to_item()),
        });
    }
    out.write(QuestRewardShown {
        quest: "Sample Quest".to_owned(),
        complete: true,
        xp: 1200,
        icons,
    });
}

/// The window's state (the engine's `QuestRewardWindow.alpha`, `QuestRewardWindow.state`, `QuestRewardWindow.timer`).
#[derive(Default)]
struct Popup {
    shown: Option<QuestRewardShown>,
    alpha: f32,
    closing: bool,
    timer: f32,
    accept: Option<URect>,
    window: Option<URect>,
    /// The icons' rects as drawn (in `shown.icons` order, those with a bitmap).
    icons: Vec<(usize, URect)>,
    /// The icon under the cursor (index into `shown.icons`) and the cursor.
    hover: Option<(usize, Vec2)>,
}

#[derive(Component)]
struct PopupRoot;

fn play(data: &GameData, field: &str, sound: &mut MessageWriter<gd_game_core::visuals::PlaySound>) {
    if let Some(pak) = data
        .db
        .get(WINDOW)
        .and_then(|w| w.str(field).map(str::to_owned))
    {
        sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
    }
}

fn open(
    mut fresh: gd_game_core::session::NewSession,
    mut shown: MessageReader<QuestRewardShown>,
    data: Res<GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
    mut popup: ResMut<QuestPopup>,
) {
    if fresh.begun() {
        *popup = QuestPopup::default();
    }
    if let Some(r) = shown.read().last() {
        play(&data, "openSound", &mut sound);
        popup.0 = Popup {
            shown: Some(r.clone()),
            alpha: if popup.0.closing { 0.0 } else { popup.0.alpha },
            closing: false,
            timer: AUTO_ACCEPT_MS,
            ..Default::default()
        };
    }
}

#[derive(Resource, Default)]
pub struct QuestPopup(Popup);

impl QuestPopup {
    /// The hovered item icon's item and the cursor (`QuestRewardItemIcon::GetRolloverText`).
    pub fn hovered_item(&self) -> Option<(gd_formats::save::item::Item, Vec2)> {
        let (i, at) = self.0.hover?;
        match self.0.shown.as_ref()?.icons.get(i)? {
            RewardIcon::Item { item } => Some(((**item).clone(), at)),
            _ => None,
        }
    }
    /// The shown window's rect contains `p` (physical px).
    pub fn covers(&self, p: Vec2) -> bool {
        self.0.shown.is_some() && self.0.window.is_some_and(|w| super::contains(&w, p))
    }
}

/// Accept clicks, before the player's world click: the window takes the cursor over itself, a left press
/// on Accept closes it.
fn clicks(
    mut popup: ResMut<QuestPopup>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut capture: ResMut<crate::state::inventory::UiCapture>,
    data: Res<GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
) {
    let p = &mut popup.0;
    if p.shown.is_none() || p.closing {
        return;
    }
    let Some(c) = window.single().ok().and_then(super::cursor_px) else {
        return;
    };
    if p.window.is_some_and(|w| super::contains(&w, c)) {
        capture.0 = true;
        if buttons.just_pressed(MouseButton::Left)
            && p.accept.is_some_and(|a| super::contains(&a, c))
        {
            p.closing = true;
            play(&data, "closeSound", &mut sound);
        }
    }
}

/// The auto-accept timer and the fades.
fn input(
    mut popup: ResMut<QuestPopup>,
    time: Res<Time>,
    real: Res<Time<Real>>,
    data: Res<GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
) {
    let p = &mut popup.0;
    if p.shown.is_none() {
        return;
    }
    let dt = real.delta_secs() * 1000.0;
    if !p.closing && p.timer > 0.0 {
        p.timer -= time.delta_secs() * 1000.0;
        if p.timer <= 0.0 {
            p.closing = true;
            play(&data, "closeSound", &mut sound);
        }
    }
    if p.closing {
        p.alpha -= dt * FADE_PER_MS;
        if p.alpha <= 0.0 {
            *p = Popup::default();
        }
    } else {
        p.alpha = (p.alpha + dt * FADE_PER_MS).min(1.0);
    }
}

fn icon_field(data: &GameData, icon: &RewardIcon) -> &'static str {
    match icon {
        RewardIcon::Money(_) => "moneyIcon",
        RewardIcon::Faction(_) => "factionIcon",
        RewardIcon::Level(_) => "levelIcon",
        RewardIcon::SkillPoints(_) => "skillPointIcon",
        RewardIcon::AttributePoints(_) => "attributePointIcon",
        RewardIcon::Devotion(_) => "devotionIcon",
        RewardIcon::Item { item } => {
            let class = data.db.get(&item.base_name).and_then(|r| r.str("Class"));
            item_icon(class.unwrap_or(""))
        }
    }
}

/// An item class's icon field (`QuestRewardWindow::GetItemIcon`'s groups, by record class).
fn item_icon(class: &str) -> &'static str {
    if class.starts_with("ArmorJewelry") {
        "jewelryIcon"
    } else if class.starts_with("Armor") {
        "armorIcon"
    } else if class.starts_with("WeaponHunting") {
        "weaponRangedIcon"
    } else if class.starts_with("Weapon") {
        "weaponMeleeIcon"
    } else if class.starts_with("OneShot") {
        "consumableIcon"
    } else if class == "ItemArtifactFormula" {
        "artifactFormulaIcon"
    } else {
        "miscItemIcon"
    }
}

/// `QuestRewardWindow::LayoutIcons`'s spacing for icon widths `w` in a box `box_w` wide (`space`, `min`:
/// the record's, scaled).
fn icon_spacing(w: &[f32], box_w: f32, space: f32, min: f32) -> f32 {
    let sum: f32 = w.iter().sum();
    let total = sum + space * (w.len().max(1) - 1) as f32;
    if total + 2.0 * space > box_w {
        let even = (box_w - sum) / (w.len() + 1) as f32;
        if even <= min { min } else { even }
    } else {
        space
    }
}

/// Builds the window while it shows; the hovered icon's rollover is its own fade group that follows the
/// cursor.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    mut popup: ResMut<QuestPopup>,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    roots: Query<Entity, With<PopupRoot>>,
    mut assets: UiAssets,
    #[allow(clippy::type_complexity)] mut drawn: Local<
        Option<(UVec2, Option<QuestRewardShown>, bool, Option<usize>)>,
    >,
    (mut group, mut tip): (Local<Option<Entity>>, Local<Option<(Entity, Vec2)>>),
    mut fader: crate::fade::Fader,
    mut shift: Query<&mut Node, With<RolloverShift>>,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let p = &mut popup.0;
    // The game menu / Options holds the cursor: no Accept hover, no rollover under it.
    let cursor = super::cursor_px(win).filter(|_| !pause.menu);
    let hovered = !p.closing
        && cursor
            .zip(p.accept)
            .is_some_and(|(c, a)| super::contains(&a, c));
    p.hover = cursor
        .filter(|_| p.shown.is_some() && !p.closing)
        .and_then(|c| {
            p.icons
                .iter()
                .find(|(_, r)| super::contains(r, c))
                .map(|(i, _)| (*i, c))
        });
    let hover_icon = p.hover.map(|(i, _)| i);
    if drawn.as_ref().is_some_and(|(s, r, h, i)| {
        *s == size && *r == p.shown && *h == hovered && *i == hover_icon
    }) {
        let alpha = p.alpha.clamp(0.0, 1.0);
        if let Some(g) = *group {
            fader.set(g, alpha);
        }
        if let Some((t, at)) = *tip {
            fader.set(t, alpha);
            if let (Some((_, c)), Ok(mut node)) = (p.hover, shift.get_mut(t)) {
                let inv = 1.0 / win.scale_factor();
                let (left, top) = (Val::Px((c.x - at.x) * inv), Val::Px((c.y - at.y) * inv));
                if node.left != left || node.top != top {
                    node.left = left;
                    node.top = top;
                }
            }
        }
        return;
    }
    *drawn = Some((size, p.shown.clone(), hovered, hover_icon));
    *group = None;
    *tip = None;
    super::window::despawn_all(&mut commands, &roots);
    let (Some(shown), Some(rec)) = (
        drawn.as_ref().and_then(|d| d.1.as_ref()),
        data.db.get(WINDOW),
    ) else {
        return;
    };
    let s = assets.scale(size);
    let screen = size.as_vec2();
    let base = WindowDef::from_record(rec).screen_rect(screen.x, screen.y, s);
    let root = gd_ui_kit::spawn_root(&mut commands, PopupRoot, 120);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        screen,
    );
    let g = crate::fade::fade_group(&mut ui, root, p.alpha.clamp(0.0, 1.0));
    // Built opaque; the group fades it.
    let a = 1.0;
    let tint = Color::WHITE;
    let child = |k: &str| rec.str(k).and_then(|p| data.db.get(p));
    let bitmap_of = |k: &str| child(k).and_then(|r| r.str("bitmapName").map(str::to_owned));
    let o = Vec2::new(base.x, base.y);
    let has_xp = shown.xp > 0;
    let mut y = o.y;
    for (k, on) in [
        ("backgroundTop", true),
        ("backgroundMiddle", has_xp && !shown.icons.is_empty()),
        ("backgroundBottom", true),
    ] {
        if !on {
            continue;
        }
        if let Some((img, sz)) = bitmap_of(k).and_then(|b| ui.image(&b)) {
            let r = URect::new(o.x, y, ui.sc(sz.x as f32), ui.sc(sz.y as f32));
            ui.quad(img, r, None, tint);
            y += r.h;
        }
    }
    let height = y - o.y;
    p.window = Some(URect::new(o.x, o.y, base.w, height));
    let text_box = |ui: &super::Ui, k: &str| {
        let tb = TextBox::from_record(child(k)?);
        let r = tb.rect;
        Some((
            URect::new(o.x + ui.sc(r.x), o.y + ui.sc(r.y), ui.sc(r.w), ui.sc(r.h)),
            tb.style?,
        ))
    };
    let text_in = |ui: &mut super::Ui, k: &str, text: &str| {
        let (r, style) = text_box(ui, k)?;
        let w = ui.text_width(&style, text);
        let lh = ui.line_height(&style);
        let at = Vec2::new(
            (r.x + r.w * 0.5 - w * 0.5).round(),
            (r.y + r.h * 0.5 - lh * 0.5).round(),
        );
        ui.text_line(&style, at, text, a)
    };
    let title = if shown.complete {
        "tagQuestRewardWindowTitle"
    } else {
        "tagQuestRewardWindowInterim"
    };
    let title = ui.fonts.tag(title).to_owned();
    text_in(&mut ui, "questTitleString", &title);
    text_in(&mut ui, "questNameString", &shown.quest);
    if has_xp {
        let xp = super::rollover::fill(ui.fonts.tag("tagQuestRewardXP"), &[&shown.xp.to_string()]);
        // The glow behind the line first (padded around the text).
        if let Some(xr) = child("XPValue")
            && let Some((r, style)) = text_box(&ui, "XPValue")
        {
            let w = ui.text_width(&style, &xp);
            let lh = ui.line_height(&style);
            let (px, py) = (
                ui.sc(xr.f32("bitmapXPadding").unwrap_or(0.0)),
                ui.sc(xr.f32("bitmapYPadding").unwrap_or(0.0)),
            );
            if let Some((img, _)) = xr
                .str("bitmapName")
                .map(str::to_owned)
                .and_then(|b| ui.image(&b))
            {
                let g = URect::new(
                    (r.x + r.w * 0.5 - w * 0.5 - px).round(),
                    (r.y + r.h * 0.5 - lh * 0.5 - py).round(),
                    (w + 2.0 * px).round(),
                    (lh + 2.0 * py).round(),
                );
                ui.quad_stretch(img, g, tint);
            }
        }
        text_in(&mut ui, "XPValue", &xp);
    }
    let f = |k: &str, d: f32| rec.f32(k).unwrap_or(d);
    let icons: Vec<(usize, Handle<Image>, Vec2, &RewardIcon)> = shown
        .icons
        .iter()
        .enumerate()
        .filter_map(|(n, i)| {
            let (img, sz) = bitmap_of(icon_field(&data, i)).and_then(|b| ui.image(&b))?;
            Some((n, img, Vec2::new(ui.sc(sz.x as f32), ui.sc(sz.y as f32)), i))
        })
        .collect();
    let widths: Vec<f32> = icons.iter().map(|i| i.2.x).collect();
    let space = icon_spacing(
        &widths,
        ui.sc(f("iconBoxWidth", 400.0)),
        ui.sc(f("iconHorizontalSpace", 20.0)),
        ui.sc(f("iconMinHorizontalSpace", 5.0)),
    );
    let row_w = widths.iter().sum::<f32>() + space * (widths.len().max(1) - 1) as f32;
    let vcenter = ui.sc(if has_xp {
        f("iconBoxVerticalCenterLow", 153.0)
    } else {
        f("iconBoxVerticalCenterHigh", 124.0)
    });
    let max_h = icons.iter().map(|i| i.2.y).fold(0.0, f32::max);
    let mut x = (o.x + base.w * 0.5 - row_w * 0.5).round();
    let top = (o.y + vcenter - max_h * 0.5).round();
    let overlay = data
        .db
        .get(ENGINE)
        .and_then(|e| e.str("itemOverlayFontName").map(str::to_owned));
    p.icons.clear();
    for (n, img, sz, icon) in icons {
        let r = URect::new(x, top, sz.x, sz.y);
        p.icons.push((n, r));
        ui.quad(img, r, None, tint);
        if let (RewardIcon::Item { item }, Some(style)) = (icon, &overlay)
            && item.stack_count > 1
        {
            let t = item.stack_count.to_string();
            let w = ui.text_width(style, &t);
            let lh = ui.line_height(style);
            ui.text_line(
                style,
                Vec2::new(
                    (r.x + r.w - 10.0 - w).round(),
                    (r.y + r.h - 10.0 - lh).round(),
                ),
                &t,
                a,
            );
        }
        x = (x + sz.x + space).round();
    }
    p.accept = child("acceptButton").and_then(|b| {
        let by = ui.sc(b.f32("bitmapPositionY").unwrap_or(0.0));
        let edge = ui.sc(f("acceptButtonTopEdge", 90.0));
        widgets::static_button(
            &mut ui,
            b,
            Vec2::new(o.x, o.y + height - edge - by),
            true,
            hovered,
            widgets::Label::Record,
        )
    });
    crate::fade::seal_fade_group(ui.commands, g);
    *group = Some(g);
    // The hovered icon's rollover, in its own group that follows the cursor.
    if let Some((n, at)) = p.hover
        && let Some(icon) = shown.icons.get(n)
        && let Some(ir) = child(icon_field(&data, icon))
    {
        let st = ir.str("textStyleName").unwrap_or("").to_owned();
        let tag = |k: &str| {
            ir.str(k)
                .filter(|t| !t.is_empty())
                .map(|t| ui.fonts.tag(t).to_owned())
        };
        let (t1, t2) = (tag("rolloverTag"), tag("rolloverTag2"));
        let lines: Vec<(String, String)> =
            rollover_lines(icon, t1.as_deref(), t2.as_deref(), &|f| {
                gd_sim::items::tooltip_item::faction_tag(f)
                    .map(|t| ui.fonts.tag(&t).to_owned())
                    .unwrap_or_default()
            })
            .into_iter()
            .map(|l| (st.clone(), l))
            .collect();
        if !lines.is_empty() {
            let style = ir.str("rolloverStyleName").and_then(|s| ui.record(s));
            let t = crate::fade::fade_group(&mut ui, root, p.alpha.clamp(0.0, 1.0));
            ui.commands.entity(t).insert(RolloverShift);
            super::rollover::draw_window(
                &mut ui,
                style.as_ref(),
                &lines,
                at,
                super::rollover::Placement::RightOf,
            );
            crate::fade::seal_fade_group(ui.commands, t);
            *tip = Some((t, at));
        }
    }
}

/// The hovered icon's rollover group, moved by the cursor since its build.
#[derive(Component)]
struct RolloverShift;

/// A text icon's rollover lines (`QuestRewardTextIcon::GetRolloverText`: `rolloverTag` with the amount when it is 1 or
/// there is no `rolloverTag2`, else `rolloverTag2`; the faction icon `QuestRewardFactionIcon::GetRolloverText`: a line
/// per change, `rolloverTag` (`+{%d0} {%s1} Reputation`) or for a loss `rolloverTag2` with the amount negated, `{%s1}`
/// the faction's `GetFactionTag` name). `tag1` / `tag2`: the localised tags; items have none (their tooltip).
fn rollover_lines(
    icon: &RewardIcon,
    tag1: Option<&str>,
    tag2: Option<&str>,
    faction_name: &dyn Fn(&str) -> String,
) -> Vec<String> {
    let amount = |n: u32| {
        let t = if n == 1 { tag1 } else { tag2.or(tag1) };
        t.map(|t| super::rollover::fill(t, &[&n.to_string()]))
            .into_iter()
            .collect()
    };
    match icon {
        RewardIcon::Money(n)
        | RewardIcon::Level(n)
        | RewardIcon::SkillPoints(n)
        | RewardIcon::AttributePoints(n)
        | RewardIcon::Devotion(n) => amount(*n),
        RewardIcon::Faction(lines) => lines
            .iter()
            .filter_map(|(f, d)| {
                let t = if *d < 0 { tag2 } else { tag1 }?;
                Some(super::rollover::fill(
                    t,
                    &[&d.unsigned_abs().to_string(), &faction_name(f)],
                ))
            })
            .collect(),
        RewardIcon::Item { .. } => Vec::new(),
    }
}
