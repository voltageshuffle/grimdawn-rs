//! The achievements window (`hudAchievementsWindow` = `records/ui/achievements/achievements_mastertable.dbr`,
//! `UIPlayerHud.achievementsWindow`, `UIAchievementWindow`): the achievement groups as buttons on the left, the
//! selected group's achievements in a scrolled list on the right. Gameplay state: `gd_game::achievements` (a build
//! without Steam starts with every achievement locked).
//!
//! `Grim Dawn.exe` (`UIAchievementWindow::UIAchievementWindow`, loader `UIAchievementWindow::Load`, rebuild
//! `AchievementWindow::RefreshList`, update `UIAchievementWindow::Update`, draw `UIAchievementWindow::Render`, event 62
//! `UIAchievementWindow::OnAchievementUnlocked`):
//! - the groups list (`groupsList` scroll window) holds one `ButtonStaticText2` per group (the group's `button`
//!   record) at (0, y), y growing by each button's height; the buttons are a radio group, the first selected
//!   (bitmapNameDown) until another is pressed; the selection survives a rebuild. A `ButtonStaticText2`
//!   (vtable `ButtonStaticText2::vftable`, draw `ButtonStaticText2::Render`) writes its `textTag` in `style` centred
//!   at (w / 2, 0.37 h) and its second text in `style2` at (w / 2, 0.67 h): here `tagAchievementsComplete` with the
//!   group's unlocked and shown counts;
//! - every group has its own list (`achievementList` scroll window): per achievement, locked and hidden ones
//!   skipped, the `lockedBackground` / `unlockedBackground` bitmap at its position + (0, y), the achievement's
//!   `lockedIcon` / `unlockedIcon` texture at the `lockedIcon` / `unlockedIcon` record's position + (0, y), its
//!   `title` in the locked / unlocked title box and its `description` wrapped in the locked / unlocked description
//!   box (both + (0, y)); y grows by the background's height;
//! - `totalUnlockedText`: `tagAchievementsUnlocked` with the unlocked and shown counts of every group;
//! - draw order: `windowBackground`, the groups list, the selected list, the title (`titleText`), the close button;
//! - event 62 (an achievement unlocked, `gd_game::achievements::AchievementUnlocked`): plays `unlockSound`, and the
//!   window rebuilds (now if visible, else when shown);
//! - opening (HUD listener `UIPlayerHud::OnToolbarButton`, key action 53 `UIPlayerHud::HandleKeyAction` case 53 presses
//!   the same button): the classic HUD's `hudAchievementsButton` is a toggle: down closes the other windows
//!   (`UIPlayerHud::CloseWindows`) and shows the window, up hides it; it shows its down bitmap while the window is
//!   visible (`UIPlayerHud::OnWindowEvent`, `UIPlayerHud.achievementsButton`). Another window's opener hides it
//!   (`UIPlayerHud::ClosePanels`).
//!
//! PORT: each group's list scroll is not kept (a new group starts at the top); the description is wrapped at its box
//! width with the style's line height (`UIMultiLineTextBox::Layout` not read) and its row grows to fit it; the group
//! buttons share one auto-sized width (the widest label, clamped to the pane) instead of the bitmaps' fixed width.
use super::Ui;
use super::contains;
use super::factions::{Pane, fmt, num, pane_rects};
use super::window::Rebuild;
use crate::state::inventory::UiCapture;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, TextAlign, TextBox, Widget, WindowDef};
use gd_game::achievements::{Achievements, Group};
use gd_game_core::data::GameData;

pub const RECORD: &str = "records/ui/achievements/achievements_mastertable.dbr";

pub struct AchievementsUiPlugin;

impl Plugin for AchievementsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AchievementsView>()
            .init_resource::<AchievementsLayout>()
            .add_systems(
                Update,
                (exclusive, unlock_sound)
                    .chain()
                    .before(build)
                    .in_set(super::UiPlaySet),
            )
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
            |view: Res<AchievementsView>, layout: Res<AchievementsLayout>| {
                view.open || layout.window.is_some()
            },
        );
    }
}

/// UI-only state: open, the selected group (`UIAchievementWindow.selectedGroup`), the groups' (0) and the list's (1)
/// scroll.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct AchievementsView {
    pub open: bool,
    pub group: usize,
    pub scroll: [f32; 2],
}

/// The built window (physical px). Empty while closed.
#[derive(Resource, Default)]
pub struct AchievementsLayout {
    pub window: Option<URect>,
    close: Option<URect>,
    /// Group buttons at scroll 0 and their group indices (a group without a drawable button has none).
    groups: Vec<(URect, usize)>,
    panes: [Option<Pane>; 2],
    bars: [Option<super::window::Bar>; 2],
    version: u64,
}

impl AchievementsLayout {
    /// The group button under `p` (screen px), the groups list scrolled by `scroll`.
    fn group_at(&self, p: Vec2, scroll: f32) -> Option<usize> {
        let pane = self.panes[0]?;
        if !contains(&pane.clip, p) {
            return None;
        }
        let q = Vec2::new(p.x, p.y + scroll);
        self.groups
            .iter()
            .find(|(r, _)| contains(r, q))
            .map(|(_, g)| *g)
    }
}

#[derive(Component)]
struct AchievementsRoot;

#[derive(Component)]
struct BarsRoot;

#[derive(Component)]
struct Scrolled {
    pane: usize,
    clip_y: f32,
    inv: f32,
}

/// A group button's `bitmapNameInFocus`.
#[derive(Component)]
struct GroupFocus(usize);

#[derive(Component)]
struct CloseFocus;

/// The achievements a group lists (locked hidden ones are not) and how many of them are unlocked.
fn shown<'a>(
    g: &'a Group,
    state: &Achievements,
) -> (Vec<(&'a gd_game::achievements::Achievement, bool)>, u32) {
    let list: Vec<_> = g
        .achievements
        .iter()
        .map(|a| (a, state.is_unlocked(&a.id)))
        .filter(|(a, unlocked)| *unlocked || !a.hidden)
        .collect();
    let unlocked = list.iter().filter(|(_, u)| *u).count() as u32;
    (list, unlocked)
}

#[cfg(feature = "dev")]
fn test_hooks(
    hooks: Res<gd_game_core::hooks::Hooks>,
    players: Query<(), With<gd_game_core::player::Player>>,
    mut view: ResMut<AchievementsView>,
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
    if hooks.ui.open.as_deref() == Some("achievements") {
        view.open = true;
        view.group = hooks.ui.tab.unwrap_or(0) as usize;
    }
}

/// Key action 53 presses the HUD's achievements button (`UIPlayerHud::HandleKeyAction` case 53): a toggle.
fn toggle_key(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<super::keys::KeyBindings>,
    mut windows: super::window::Windows,
) {
    if bindings.just_pressed(super::keys::ACHIEVEMENTS, &keys) {
        windows.toggle(super::window::Window::Achievements);
    }
}

/// The window's open/close watcher: opening it applies the policy (`UIPlayerHud::CloseWindows`); a window
/// the policy excludes opening closes it (`UIPlayerHud::ClosePanels`).
fn exclusive(
    mut windows: super::window::Windows,
    policy: Res<super::window::WindowPolicy>,
    mut was: Local<[bool; super::window::Window::COUNT]>,
    mut fresh: gd_game_core::session::NewSession,
) {
    if fresh.begun() {
        *was = Default::default();
    }
    let w = super::window::Window::Achievements;
    let now = windows.snapshot();
    if windows.achievements.open && !was[w.index()] {
        windows.open(w);
    } else if windows.achievements.open && policy.opened_closer(w, &was, &now) {
        windows.achievements.open = false;
    }
    *was = windows.snapshot();
}

/// Event 62: the window's `unlockSound` (`UIAchievementWindow::OnAchievementUnlocked`, whether or not the
/// window shows).
fn unlock_sound(
    mut events: MessageReader<gd_game::achievements::AchievementUnlocked>,
    data: Res<GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
) {
    for _ in events.read() {
        if let Some(pak) = data
            .db
            .get(RECORD)
            .and_then(|r| r.str("unlockSound").map(str::to_owned))
        {
            sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    state: Res<Achievements>,
    mut view: ResMut<AchievementsView>,
    roots: Query<Entity, With<AchievementsRoot>>,
    bar_roots: Query<Entity, With<BarsRoot>>,
    mut layout: ResMut<AchievementsLayout>,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<Option<(UVec2, u32, bool, usize, Vec<String>, u64)>>,
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
    let sb = assets.scale(size).to_bits();
    let generation = assets.fonts.generation;
    let same = built.as_ref().is_some_and(|b| {
        b.0 == size
            && b.1 == sb
            && b.2 == view.open
            && b.3 == view.group
            && b.4 == state.unlocked
            && b.5 == generation
    });
    if !same {
        *built = Some((
            size,
            sb,
            view.open,
            view.group,
            state.unlocked.clone(),
            generation,
        ));
        super::window::despawn_all(&mut commands, &roots);
        let version = layout.version + 1;
        *layout = AchievementsLayout {
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
            ui.root = super::spawn_root(ui.commands, AchievementsRoot, super::window::Z_PANEL);
            let groups = gd_game::achievements::groups(&data.db);
            view.group = view.group.min(groups.len().saturating_sub(1));
            draw(&mut ui, &groups, &state, view.group, &mut layout);
        }
    }
    for (i, p) in layout.panes.iter().enumerate() {
        if let Some(p) = p {
            view.scroll[i] = view.scroll[i].clamp(0.0, p.max);
        }
    }
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
    for (i, field) in ["groupsList", "achievementList"].iter().enumerate() {
        let (Some(sw), Some(p)) = (rec.str(field).and_then(|r| ui.record(r)), layout.panes[i])
        else {
            continue;
        };
        let (window, clip) = pane_rects(&ui, &sw, o);
        if p.max > 0.0 || sw.bool("alwaysShowScroll") {
            let len = ui.sc(num(&sw, "height") + num(&sw, "scrollbarSizeAdjust"));
            let top = window.y + ui.sc(num(&sw, "scrollbarPositionAdjust"));
            layout.bars[i] = super::window::scrollbar_live(
                &mut ui,
                sw.str("verticalScrollbar").unwrap_or(""),
                clip.x + clip.w,
                top,
                len,
                (clip.h, p.max + clip.h, view.scroll[i]),
                super::window::ScrollOwner::Achievements,
                i,
            );
        }
    }
}

/// A `ButtonStaticText2` at `at`: its bitmap (down when `down`), its hidden in-focus bitmap, its two texts;
/// returns its rect. `w` is the column's shared width: the bitmaps stretch to it and a too-long label truncates.
fn group_button(
    ui: &mut Ui,
    b: &gd_formats::arz::Record,
    at: Vec2,
    down: bool,
    i: usize,
    sub: &str,
    w: f32,
) -> Option<URect> {
    let key = if down {
        "bitmapNameDown"
    } else {
        "bitmapNameUp"
    };
    let (img, size) = b.str(key).map(str::to_owned).and_then(|p| ui.image(&p))?;
    let r = URect::new(at.x, at.y, w, ui.sc(size.y as f32));
    if w > ui.sc(size.x as f32) {
        ui.quad_stretch(img, r, Color::WHITE);
    } else {
        ui.quad(img, r, None, Color::WHITE);
    }
    if !down
        && let Some((img, fs)) = b
            .str("bitmapNameInFocus")
            .map(str::to_owned)
            .and_then(|p| ui.image(&p))
    {
        let e = if w > ui.sc(fs.x as f32) {
            ui.quad_stretch(img, r, Color::WHITE)
        } else {
            ui.quad(img, r, None, Color::WHITE)
        };
        ui.commands
            .entity(e)
            .insert((GroupFocus(i), Visibility::Hidden));
    }
    // Each text centred on (w / 2, k h): a box as tall as the button, centred there.
    let centred = |k: f32| URect::new(r.x, r.y + r.h * k - r.h * 0.5, r.w, r.h);
    let max_w = (w - 2.0 * ui.sc(10.0)).max(0.0);
    if let (Some(style), Some(tag)) = (b.str("style"), b.str("textTag")) {
        let t = ui.fonts.tag(tag).to_owned();
        let t = ui.truncate_line(style, max_w, &t);
        super::widgets::label(ui, centred(0.37), style, TextAlign::Center, &t);
    }
    if let Some(style) = b.str("style2") {
        let t = ui.truncate_line(style, max_w, sub);
        super::widgets::label(ui, centred(0.67), style, TextAlign::Center, &t);
    }
    Some(r)
}

fn draw(
    ui: &mut Ui,
    groups: &[Group],
    state: &Achievements,
    selected: usize,
    layout: &mut AchievementsLayout,
) {
    use gd_sim::items::tooltip::Arg;
    let Some(rec) = ui.record(RECORD) else {
        return;
    };
    let field = |k: &str| rec.str(k).unwrap_or("").to_owned();
    let w = WindowDef::from_record(&rec).screen_rect(ui.screen.x, ui.screen.y, ui.s);
    let o = Vec2::new(w.x, w.y);
    layout.window = Some(w);
    ui.bitmap_field(&field("windowBackground"), o);
    let outer = ui.root;
    let (total, total_shown) = groups.iter().fold((0u32, 0u32), |(t, n), g| {
        let (list, unlocked) = shown(g, state);
        (t + unlocked, n + list.len() as u32)
    });
    if let Some(sw) = ui.record(&field("groupsList")) {
        let (_, clip) = pane_rects(ui, &sw, o);
        let inner = super::window::pane(ui, clip, 0.0);
        ui.commands.entity(inner).insert(Scrolled {
            pane: 0,
            clip_y: clip.y,
            inv: ui.inv,
        });
        ui.root = inner;
        // One width for the whole radio column: the widest button texture or label, clamped to the pane.
        let mut btns = Vec::new();
        let mut bw = 0.0f32;
        for (i, g) in groups.iter().enumerate() {
            let (list, unlocked) = shown(g, state);
            let sub = fmt(
                ui,
                "tagAchievementsComplete",
                &[Arg::Num(unlocked as f32), Arg::Num(list.len() as f32)],
            );
            let Some(b) = ui.record(&g.button) else {
                continue;
            };
            let Some(tex_w) = b
                .str("bitmapNameUp")
                .and_then(|p| ui.image(p))
                .map(|(_, size)| ui.sc(size.x as f32))
            else {
                continue;
            };
            let title = b.str("textTag").map(|t| ui.fonts.tag(t).to_owned());
            let mut w = super::widgets::button_width(ui, &b, tex_w, title.as_deref());
            if let Some(style) = b.str("style2") {
                w = w.max(ui.text_width(style, &sub) + 2.0 * ui.sc(10.0));
            }
            bw = bw.max(w);
            btns.push((i, b, sub));
        }
        let bw = bw.min(clip.w);
        let mut y = 0.0;
        for (i, b, sub) in &btns {
            if let Some(r) = group_button(
                ui,
                b,
                Vec2::new(clip.x, clip.y + y),
                *i == selected,
                *i,
                sub,
                bw,
            ) {
                y += r.h;
                layout.groups.push((r, *i));
            }
        }
        layout.panes[0] = Some(Pane {
            clip,
            max: (y - clip.h).max(0.0),
        });
        ui.root = outer;
    }
    if let (Some(sw), Some(g)) = (ui.record(&field("achievementList")), groups.get(selected)) {
        let (_, clip) = pane_rects(ui, &sw, o);
        let inner = super::window::pane(ui, clip, 0.0);
        ui.commands.entity(inner).insert(Scrolled {
            pane: 1,
            clip_y: clip.y,
            inv: ui.inv,
        });
        ui.root = inner;
        let parts = |unlocked: bool| {
            let k = |s: &str| {
                field(&format!(
                    "{}{s}",
                    if unlocked { "unlocked" } else { "locked" }
                ))
            };
            (k("Background"), k("Icon"), k("Title"), k("Description"))
        };
        let mut y = 0.0;
        for (a, unlocked) in shown(g, state).0 {
            let (bg, icon, title, desc) = parts(unlocked);
            let row = Vec2::new(clip.x, clip.y + y);
            let mut h = 0.0;
            if let Some((_, r)) = ui.bitmap_field(&bg, row) {
                h = r.h;
            }
            if let Some(ir) = ui.record(&icon) {
                let path = if unlocked {
                    &a.unlocked_icon
                } else {
                    &a.locked_icon
                };
                ui.bitmap(
                    path,
                    num(&ir, "bitmapPositionX"),
                    num(&ir, "bitmapPositionY"),
                    row,
                );
            }
            if let Some(t) = ui.record(&title) {
                let tb = TextBox::from_record(&t);
                let text = ui.fonts.tag(&a.title).to_owned();
                ui.text(&tb, row, &text);
            }
            if let Some(t) = ui.record(&desc) {
                let tb = TextBox::from_record(&t);
                if let Some(style) = tb.style.clone() {
                    let text = ui.fonts.tag(&a.description).to_owned();
                    let lh = ui.line_height(&style);
                    let bw = ui.sc(tb.rect.w);
                    let top = ui.sc(tb.rect.y);
                    // The row grows with the wrapped description.
                    h = h.max(top + gd_ui_kit::layout::row_height(ui, &style, &text, bw, 0.0));
                    let mut ly = row.y + top;
                    for l in ui.wrap_lines(&style, bw, &text) {
                        ui.text_line(&style, Vec2::new(row.x + ui.sc(tb.rect.x), ly), &l, 1.0);
                        ly += lh;
                    }
                }
            }
            y += h;
        }
        layout.panes[1] = Some(Pane {
            clip,
            max: (y - clip.h).max(0.0),
        });
        ui.root = outer;
    }
    if let Some(t) = ui.record(&field("titleText")) {
        let tb = TextBox::from_record(&t);
        let text = tb
            .tag
            .as_deref()
            .map(|t| ui.fonts.tag(t).to_owned())
            .unwrap_or_default();
        ui.text(&tb, o, &text);
    }
    if let Some(t) = ui.record(&field("totalUnlockedText")) {
        let tb = TextBox::from_record(&t);
        let text = fmt(
            ui,
            "tagAchievementsUnlocked",
            &[Arg::Num(total as f32), Arg::Num(total_shown as f32)],
        );
        ui.text(&tb, o, &text);
    }
    if let Some(Widget::Button {
        up: Some(up),
        over,
        x,
        y,
        ..
    }) = ui.widget(&field("closeButton"))
    {
        layout.close = ui.bitmap(&up, x, y, o).map(|(_, r)| r);
        if let Some((e, _)) = over.and_then(|f| ui.bitmap(&f, x, y, o)) {
            ui.commands
                .entity(e)
                .insert((CloseFocus, Visibility::Hidden));
        }
    }
}

/// Moves the scrolled content and shows the hovered button's in-focus bitmap.
#[allow(clippy::type_complexity)]
fn live(
    window: Query<&Window, With<PrimaryWindow>>,
    view: Res<AchievementsView>,
    layout: Res<AchievementsLayout>,
    mut panes: Query<(&Scrolled, &mut Node), Without<super::window::ScrollThumb>>,
    mut focus: Query<
        (&mut Visibility, Option<&GroupFocus>),
        Or<(With<GroupFocus>, With<CloseFocus>)>,
    >,
    mut thumbs: Query<(&super::window::ScrollThumb, &mut Node), Without<Scrolled>>,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    for (sc, mut node) in &mut panes {
        let top = Val::Px((-sc.clip_y - view.scroll[sc.pane]) * sc.inv);
        if node.top != top {
            node.top = top;
        }
    }
    for (t, mut node) in &mut thumbs {
        if t.owner != super::window::ScrollOwner::Achievements {
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
    // The game menu / Options holds the cursor: no hover under it.
    let p = super::cursor_px(win).filter(|_| !pause.menu);
    let group = p.and_then(|p| layout.group_at(p, view.scroll[0]));
    let close = p.is_some_and(|p| layout.close.is_some_and(|r| contains(&r, p)));
    for (mut vis, g) in &mut focus {
        let on = match g {
            Some(g) => group == Some(g.0),
            None => close,
        };
        vis.set_if_neq(if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Clicks, the wheel and the scroll bars.
#[allow(clippy::too_many_arguments)]
fn clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    scale: Res<gd_ui_kit::state::settings::UiScaleOption>,
    layout: Res<AchievementsLayout>,
    mut view: ResMut<AchievementsView>,
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
    let Ok(win) = window.single() else { return };
    let Some(p) = super::cursor_px(win) else {
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
    let sound_of = |field: &str| {
        rec.as_ref()
            .and_then(|r| r.str(field))
            .and_then(|b| data.db.get(b))
            .and_then(|b| b.str("soundNameDown").map(str::to_owned))
    };
    if layout.close.is_some_and(|r| contains(&r, p)) {
        if let Some(pak) = sound_of("closeButton") {
            sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
        }
        view.open = false;
    } else if let Some(g) = layout.group_at(p, view.scroll[0])
        && g != view.group
    {
        view.group = g;
        view.scroll[1] = 0.0;
    }
}
