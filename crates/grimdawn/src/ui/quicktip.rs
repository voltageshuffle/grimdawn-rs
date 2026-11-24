//! The tutorial quick tip (`tutorialTips` option): a small box at the top left that names a tutorial page's hint when
//! the page unlocks for the first time ([`gd_game::tutorials::TutorialTip`], the engine's event 54).
//!
//! Grim Dawn.exe: `HelpPage::ShowQuickTip` reads the page's `quickTag` (nothing when empty) and `quickTagKey` (a key
//! action name, [`gd_ui::quests::key_operation`]: the key bound to it is the tag's `%s`), and with the option on hands
//! the text to the tip manager `TipManager::AddTip` at (20, 100); `TipManager::Update`: 250 ms fade in, 10 s opaque,
//! 500 ms fade out; `TipManager::OnTipClick`: a click on the box opens the Tutorial (Help) window on the page and fades
//! the tip out (500 ms). A tip created while another shows is queued behind it (FIFO). The option off: no tip is made
//! (the page still unlocks). The page number is the index into the Help window's `tutorialPages` list.
//!
//! HUD widget tips: every widget loaded from a record with `tutorialTag` (loader `UIWidget::LoadTutorialTip`:
//! `tutorialTrigger`, `tutorialTag`, `tutorialKey`, `tutorialOffsetY`) listens to event 54
//! (`UIWidget::RegisterTutorialListener`); when the unlocked page is its `tutorialTrigger` and the tag is not empty,
//! `UIWidget::ShowTutorialTip` hands the tag (with the `tutorialKey` action's key name as its `%s`, else without
//! parameters) to the tip manager as a pointer tip (`TipManager::AddTip` mode 1, kind 1) at the widget rect's centre,
//! `tutorialOffsetY` lower, when the option is on. A pointer tip starts at once (it is never queued and does not hold
//! back the page tips: `TipManager::Update` waits only for kind-0 tips); its box (`TipBox::Layout`) is 340 + 2 ×
//! padding wide and its text's height + 2 × padding high, centred on the point with its bottom on it, then raised 50 px
//! (top at least 20), with `ui/tutorialwindow/tutorialtipbottom.tex` centred under it; same fades, and a click opens
//! the trigger's Help page (`TipManager::OnTipClick`). The HUD widgets with one ([`POINTER_FIELDS`]): the health and
//! constitution bars, the teleport button, the character button and the classic HUD's level-up indicator
//! (`playerPortraitLevelUpIndicatorButton`, page 14). By design not the group button's (page 26: no multiplayer).
//!
//! PORT: the box is the black rect + `genericrolloverbox` border of the button rollover (the tip manager's own frame
//! is `genericbox_*.tex`, same pieces), [`TIP_TEXT`] wide like the pointer tips' (`TipBox::Layout`). The text
//! ([`layout`]): the title (the tag up to its first `{^-}`, `Title{^-}{^n}{^n}Body` in every `tagQuickTip*`) in
//! `style_rollover_title`, the body in `style_rollover_description` (its `{^h}` highlights), every line centred, a
//! blank line a short gap ([`PARAGRAPH_GAP`]): read off the real game's main-menu tip, not decompiled. The page tips'
//! point (20, 100) / (20, 150) is the box's left edge and vertical centre (the real main-menu box is centred on
//! y = 150). `GD_TUTORIAL_TIP` (hooks.rs) unlocks that page at that time.
//! PORT (a performance choice): the tips are built once per (window size, HUD table, set of tips, tags
//! generation), each in a [`FadeGroup`]; the fades change the nodes' alpha in place ([`Fader`]). The tags
//! generation (`FontCache::generation`) bumps on a mod or language swap: a rebuild re-resolves each tip's text
//! from its stored tag, so the tips follow the new language (the engine localises live, `MainMenu::SetState`).
use super::UiAssets;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::Rect as URect;
use gd_game_core::data::GameData;
use gd_ui::fade::{Fader, fade_group, seal_fade_group};
use std::collections::VecDeque;

const TITLE: &str = "records/ui/styles/text/style_rollover_title.dbr";
const BODY: &str = "records/ui/styles/text/style_rollover_description.dbr";
const BOX: &str = "records/ui/styles/rollover/style_genericrolloverbox.dbr";
const HELP: &str = "records/ingameui/questwindow/questwindow.dbr";
const FADE_IN: f32 = 250.0;
const HOLD: f32 = 10_000.0;
const FADE_OUT: f32 = 500.0;
/// Text width of every tip box (UI units).
const TIP_TEXT: f32 = 340.0;
/// A blank line in a tip's body: this share of a body line.
const PARAGRAPH_GAP: f32 = 0.4;

/// The tip on screen (page record, text, age ms; `out` = ms since it was dismissed) and the tips waiting.
#[derive(Resource, Default)]
pub struct QuickTip {
    shown: Option<Shown>,
    queue: VecDeque<u32>,
    /// The main menu's tip was made (once per process, `MainMenu.quickTipShown`; not reset with a session).
    menu_tip_done: bool,
    /// The HUD widgets' pointer tips on screen (module doc).
    pointers: Vec<Pointer>,
}

impl QuickTip {
    /// Clears the session's tips; keeps the once-per-process main-menu flag.
    pub fn end_session(&mut self) {
        let done = self.menu_tip_done;
        *self = Self::default();
        self.menu_tip_done = done;
    }
}

/// A pointer tip: the HUD master field of its widget, the page that triggered it, its tag (with the optional
/// key action), the resolved text (draw cache) and timing.
struct Pointer {
    field: &'static str,
    page: u32,
    tag: String,
    key: Option<u32>,
    text: String,
    age: f32,
    out_at: Option<f32>,
    rect: URect,
    /// Drawn by the current build ([`Built`]).
    built: bool,
}

/// The HUD master table's widgets with a tutorial tip (module doc).
const POINTER_FIELDS: [&str; 5] = [
    "hudHealthBar",
    "hudConstitutionBar",
    "hudTeleportButton",
    "hudCharacterButton",
    "playerPortraitLevelUpIndicatorButton",
];
const POINTER_RAISE: f32 = 50.0;
const POINTER_ARROW: &str = "ui/tutorialwindow/tutorialtipbottom.tex";

struct Shown {
    record: String,
    tag: String,
    key: Option<u32>,
    text: String,
    age: f32,
    /// Age at which the fade out started (a click, or `FADE_IN + HOLD`).
    out_at: Option<f32>,
    /// The box in physical px (for clicks).
    rect: URect,
    /// The box's top in UI units (100; the main menu's tip 150).
    y: f32,
    /// Drawn by the current build ([`Built`]).
    built: bool,
}

#[derive(Component)]
struct TipRoot;

/// What is on screen: the window size, HUD table and tags generation it was built for and the tips' fade
/// groups (the page tip's, the pointer tips' in `QuickTip::pointers` order).
#[derive(Default)]
struct Built {
    size: UVec2,
    classic: bool,
    generation: u64,
    page: Option<Entity>,
    pointers: Vec<Entity>,
}

pub struct QuickTipPlugin;

impl Plugin for QuickTipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuickTip>()
            // The in-game click half, before the player's world click.
            .add_systems(Update, clicks.in_set(super::UiInputSet))
            // In the game and, for the main menu's tip, in the menu. After the text tags load: the menu's tip
            // resolves its tag on the first menu frame.
            .add_systems(
                Update,
                tip.after(gd_ui::load_tags)
                    .run_if(not(in_state(gd_ui::state::session::AppState::Loading))),
            )
            // Every tip on screen goes when the loading screen comes up (the menu's tip as in the real game).
            .add_systems(OnEnter(gd_ui::state::session::AppState::Loading), drop_tips);
    }
}

/// Drops the tips on screen (the page tip, the main menu's and the pointer tips) and despawns their nodes as the
/// loading screen comes up ([`tip`] does not run while loading). Queued page tips stay and show after it.
fn drop_tips(
    mut commands: Commands,
    mut state: ResMut<QuickTip>,
    roots: Query<Entity, With<TipRoot>>,
) {
    state.shown = None;
    state.pointers.clear();
    gd_ui::window::despawn_all(&mut commands, &roots);
}

/// The tip click half ([`super::UiInputSet`], before the player's world click): a click inside a
/// tip's box opens its Help page and dismisses it, and takes the cursor so the click does not reach
/// the world.
fn clicks(
    mut state: ResMut<QuickTip>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    mut capture: ResMut<gd_ui::state::inventory::UiCapture>,
    mut journal: ResMut<gd_ui::quests::QuestJournal>,
    data: Res<GameData>,
) {
    let Ok(win) = window.single() else { return };
    click_tips(
        &mut state,
        super::cursor_px(&hooks, win),
        &buttons,
        &mut capture,
        &mut journal,
        &data,
    );
}

/// A click inside the page tip's box opens its page and dismisses it; a click on a pointer tip opens
/// its trigger's page (`TipManager::OnTipClick`). Both take the cursor.
fn click_tips(
    state: &mut QuickTip,
    cursor: Option<Vec2>,
    buttons: &ButtonInput<MouseButton>,
    capture: &mut gd_ui::state::inventory::UiCapture,
    journal: &mut gd_ui::quests::QuestJournal,
    data: &GameData,
) {
    if let Some(s) = &mut state.shown
        && s.out_at.is_none()
        && let Some(p) = cursor
        && super::contains(&s.rect, p)
    {
        capture.0 = true;
        if buttons.just_pressed(MouseButton::Left) {
            if !s.record.is_empty() {
                journal.open_help(&s.record);
            }
            s.out_at = Some(s.age);
        }
    }
    for t in &mut state.pointers {
        if t.out_at.is_none() && cursor.is_some_and(|p| super::contains(&t.rect, p)) {
            capture.0 = true;
            if buttons.just_pressed(MouseButton::Left) {
                if let Some(rec) = page_record(data, t.page) {
                    journal.open_help(&rec);
                }
                t.out_at = Some(t.age);
            }
        }
    }
}

/// The Help window's page record of a page index.
fn page_record(data: &GameData, page: u32) -> Option<String> {
    let list = data.db.get(HELP)?.str("helpIndex")?.to_string();
    let rec = data
        .db
        .get(&list)?
        .strs("tutorialPages")
        .get(page as usize)?
        .to_string();
    Some(rec)
}

/// A tag with an optional key action's key name as its `%s`. `^N` is the same line break as `^n` (the stock
/// tutorial tags use it; the font layout would otherwise eat it as a colour code and run the lines together).
fn with_key(
    fonts: &super::text::FontCache,
    bindings: &super::keys::KeyBindings,
    tag: &str,
    key: Option<u32>,
) -> String {
    match key {
        Some(a) => gd_sim::items::tooltip::format_exact(
            fonts.tag(tag),
            &[gd_sim::items::tooltip::Arg::Str(&super::keys::key_name(
                bindings.get(a)[0],
            ))],
        ),
        None => fonts.tag(tag).to_owned(),
    }
    .trim()
    .replace("^N", "^n")
}

/// The pointer tips a page unlock makes: the [`POINTER_FIELDS`] of the HUD table whose record's
/// `tutorialTrigger` is the page and whose `tutorialTag` is set.
fn pointer_tips(
    db: &gd_formats::vfs::Database,
    fonts: &super::text::FontCache,
    bindings: &super::keys::KeyBindings,
    master: &str,
    page: u32,
) -> Vec<Pointer> {
    let Some(m) = db.get(master) else {
        return Vec::new();
    };
    POINTER_FIELDS
        .iter()
        .filter_map(|&field| {
            let r = db.get(m.str(field)?)?;
            if r.f32("tutorialTrigger").unwrap_or(0.0) as i64 != page as i64 {
                return None;
            }
            let tag = r.str("tutorialTag").filter(|t| !t.is_empty())?;
            let key = r.str("tutorialKey").and_then(gd_ui::quests::key_operation);
            Some(Pointer {
                field,
                page,
                tag: tag.to_owned(),
                key,
                text: with_key(fonts, bindings, tag, key),
                age: 0.0,
                out_at: None,
                rect: URect::new(0.0, 0.0, 0.0, 0.0),
                built: false,
            })
        })
        .collect()
}

/// The point a pointer tip shows at: the widget rect's centre, `tutorialOffsetY` lower (screen px).
fn pointer_anchor(ui: &mut super::Ui, field: &str) -> Option<Vec2> {
    let r = match field {
        "hudHealthBar" | "hudConstitutionBar" => gd_ui::hud::bar_rect(ui, field)?,
        // A hidden button over the portrait (no bitmap: its record size).
        "playerPortraitLevelUpIndicatorButton" => {
            let po = gd_ui::hud::portrait_origin(ui)?;
            let rec = ui.record(ui.hud_master())?.str(field)?.to_string();
            let b = ui.record(&rec)?;
            let f = |k: &str| b.f32(k).unwrap_or(0.0);
            URect::new(
                po.x + ui.sc(f("bitmapPositionX")),
                po.y + ui.sc(f("bitmapPositionY")),
                ui.sc(f("buttonXSize")),
                ui.sc(f("buttonYSize")),
            )
        }
        _ => gd_ui::rollover::button_rect(ui, field)?,
    };
    let rec = ui.record(ui.hud_master())?.str(field)?.to_string();
    let dy = ui
        .record(&rec)
        .and_then(|b| b.f32("tutorialOffsetY"))
        .unwrap_or(0.0);
    Some(Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5 + dy))
}

/// The page's record and tip tag with its optional key action (`None`: no `quickTag`).
fn page_text(data: &GameData, page: u32) -> Option<(String, String, Option<u32>)> {
    let rec = page_record(data, page)?;
    let r = data.db.get(&rec)?;
    let tag = r.str("quickTag").filter(|t| !t.is_empty())?;
    let key = r.str("quickTagKey").and_then(gd_ui::quests::key_operation);
    Some((rec, tag.to_owned(), key))
}

#[allow(clippy::too_many_arguments)]
fn tip(
    mut fresh: gd_game_core::session::NewSession,
    mut commands: Commands,
    mut state: ResMut<QuickTip>,
    mut events: MessageReader<gd_game::tutorials::TutorialTip>,
    mut unlock: MessageWriter<gd_game::tutorials::UnlockTutorial>,
    options: Res<gd_game_core::options::GameOptions>,
    (hooks, mut fired): (Res<gd_game_core::hooks::Hooks>, Local<bool>),
    clock: Res<Time<Real>>,
    buttons: Res<ButtonInput<MouseButton>>,
    (mut journal, mut capture): (
        ResMut<gd_ui::quests::QuestJournal>,
        ResMut<gd_ui::state::inventory::UiCapture>,
    ),
    window: Query<&Window, With<PrimaryWindow>>,
    (data, bindings): (Res<GameData>, Res<super::keys::KeyBindings>),
    (roots, mut built, mut fader): (Query<Entity, With<TipRoot>>, Local<Built>, Fader),
    mut assets: UiAssets,
    menu: Option<Res<crate::menu::MainMenu>>,
) {
    if fresh.begun() {
        state.end_session();
        *fired = false;
        *built = Built::default();
    }
    // The main menu's tip (`MainMenu::SetState` case 3): `tagQuickTipMainMenu` at
    // (20, 150), once per process the first time the menu shows (the flag is set even with the option off).
    if menu.is_some() && !state.menu_tip_done {
        state.menu_tip_done = true;
        if options.flag("tutorialTips") {
            let text = assets.fonts.tag("tagQuickTipMainMenu").trim().to_owned();
            state.shown = Some(Shown {
                record: String::new(),
                tag: "tagQuickTipMainMenu".to_owned(),
                key: None,
                text,
                age: 0.0,
                out_at: None,
                rect: URect::new(0.0, 0.0, 0.0, 0.0),
                y: 150.0,
                built: false,
            });
        }
    }
    if let Some((page, t)) = hooks.ui.tutorial_tip
        && !*fired
        && clock.elapsed_secs() >= t
    {
        *fired = true;
        unlock.write(gd_game::tutorials::UnlockTutorial(page));
    }
    let master = gd_ui_kit::master(gd_ui_kit::classic(Some(&options)));
    for e in events.read() {
        if options.flag("tutorialTips") {
            state.queue.push_back(e.0);
            let tips = pointer_tips(&data.db, &assets.fonts, &bindings, master, e.0);
            state.pointers.extend(tips);
        }
    }
    let dt = clock.delta_secs() * 1000.0;
    if let Some(s) = &mut state.shown {
        s.age += dt;
        if s.out_at.is_none() && s.age >= FADE_IN + HOLD {
            s.out_at = Some(s.age);
        }
        if s.out_at.is_some_and(|o| s.age >= o + FADE_OUT) {
            state.shown = None;
        }
    }
    for t in &mut state.pointers {
        t.age += dt;
        if t.out_at.is_none() && t.age >= FADE_IN + HOLD {
            t.out_at = Some(t.age);
        }
    }
    state
        .pointers
        .retain(|t| t.out_at.is_none_or(|o| t.age < o + FADE_OUT));
    // The next queued tip starts once no page tip shows (`TipManager::AddTip`: queued while a tip is active).
    while state.shown.is_none() {
        let Some(page) = state.queue.pop_front() else {
            break;
        };
        if let Some((record, tag, key)) = page_text(&data, page) {
            let text = with_key(&assets.fonts, &bindings, &tag, key);
            state.shown = Some(Shown {
                record,
                tag,
                key,
                text,
                age: 0.0,
                out_at: None,
                rect: URect::new(0.0, 0.0, 0.0, 0.0),
                y: 100.0,
                built: false,
            });
        }
    }
    let Ok(win) = window.single() else { return };
    // The main menu's tip click only — the in-game clicks are [`clicks`]' ([`super::UiInputSet`],
    // before the player's world click).
    if menu.is_some() {
        click_tips(
            &mut state,
            super::cursor_px(&hooks, win),
            &buttons,
            &mut capture,
            &mut journal,
            &data,
        );
    }
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let classic = gd_ui_kit::classic(Some(&options));
    let wanted = state.shown.is_some() || !state.pointers.is_empty();
    let stale = built.size != size
        || built.classic != classic
        || built.generation != assets.fonts.generation
        || built.page.is_some() != state.shown.is_some()
        || built.pointers.len() != state.pointers.len()
        || state.shown.as_ref().is_some_and(|s| !s.built)
        || state.pointers.iter().any(|t| !t.built)
        || (wanted && roots.is_empty());
    if !stale {
        // The same tips as built: only the fades move.
        if let (Some(group), Some(s)) = (built.page, &state.shown) {
            fader.set(group, fade(s.age, s.out_at));
        }
        for (group, t) in built.pointers.iter().zip(&state.pointers) {
            fader.set(*group, fade(t.age, t.out_at));
        }
        return;
    }
    gd_ui::window::despawn_all(&mut commands, &roots);
    *built = Built {
        size,
        classic,
        generation: assets.fonts.generation,
        ..default()
    };
    if !wanted {
        return;
    }
    // A rebuild re-resolves each tip's text from its tag: a language swap bumps the tags generation.
    for t in &mut state.pointers {
        t.text = with_key(&assets.fonts, &bindings, &t.tag, t.key);
    }
    if let Some(s) = &mut state.shown {
        s.text = with_key(&assets.fonts, &bindings, &s.tag, s.key);
    }

    let root = gd_ui_kit::spawn_root(&mut commands, TipRoot, 205);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        assets.scale(size),
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    for t in &mut state.pointers {
        let group = fade_group(&mut ui, root, fade(t.age, t.out_at));
        if let Some(p) = pointer_anchor(&mut ui, t.field) {
            t.rect = draw_pointer(&mut ui, &t.text, p);
        }
        seal_fade_group(ui.commands, group);
        t.built = true;
        built.pointers.push(group);
    }
    let Some(s) = &mut state.shown else { return };
    let group = fade_group(&mut ui, root, fade(s.age, s.out_at));
    // The box is built opaque; the group fades it.
    let alpha = 1.0;
    let max_w = ui.sc(TIP_TEXT);
    let (lines, text_h) = layout(&mut ui, &s.text, max_w);
    let pad = 10.0;
    let (w, h) = (max_w + 2.0 * pad, text_h + 2.0 * pad);
    let r = URect::new(ui.sc(20.0), (ui.sc(s.y) - h * 0.5).max(20.0), w, h);
    s.rect = r;
    ui.solid(r, Color::srgba(0.0, 0.0, 0.0, alpha));
    if let Some(b) = ui.record(BOX) {
        ui.border(&b, "", r, alpha);
    }
    draw_lines(&mut ui, &lines, r, pad, alpha);
    seal_fade_group(ui.commands, group);
    s.built = true;
    built.page = Some(group);
}

/// A tip's opacity: the fade in, then the fade out from `out_at`.
fn fade(age: f32, out_at: Option<f32>) -> f32 {
    match out_at {
        Some(o) => 1.0 - ((age - o) / FADE_OUT).clamp(0.0, 1.0),
        None => (age / FADE_IN).clamp(0.0, 1.0),
    }
}

/// A pointer tip's box over `p` (module doc) and its arrow, built opaque (its [`FadeGroup`] fades it); returns
/// the box.
fn draw_pointer(ui: &mut super::Ui, text: &str, p: Vec2) -> URect {
    let alpha = 1.0;
    let max_w = ui.sc(TIP_TEXT);
    let (lines, text_h) = layout(ui, text, max_w);
    let pad = 10.0;
    let (w, h) = (max_w + 2.0 * pad, text_h + 2.0 * pad);
    let r = URect::new(p.x - w * 0.5, (p.y - h - POINTER_RAISE).max(20.0), w, h);
    ui.solid(r, Color::srgba(0.0, 0.0, 0.0, alpha));
    if let Some(b) = ui.record(BOX) {
        ui.border(&b, "", r, alpha);
    }
    draw_lines(ui, &lines, r, pad, alpha);
    if let Some((img, sz)) = ui.image(POINTER_ARROW) {
        let sz = sz.as_vec2();
        let a = URect::new(r.x + r.w * 0.5 - sz.x * 0.5, r.y + r.h, sz.x, sz.y);
        ui.quad(img, a, None, Color::srgba(1.0, 1.0, 1.0, alpha));
    }
    r
}

/// A tip's text laid out (module doc): its lines (style, text with markup, top relative to the text's top) at most
/// `max_w` px wide, and the text's height.
fn layout(ui: &mut super::Ui, text: &str, max_w: f32) -> (Vec<(&'static str, String, f32)>, f32) {
    let text = text.replace("{^n}", "\n").replace("^n", "\n");
    // The title ends at the first `^-` (braced or not, as the tag table hands it out).
    let (title, body) = match text.find("^-") {
        Some(i) => {
            let head = text[..i].strip_suffix('{').unwrap_or(&text[..i]);
            let rest = &text[i + "^-".len()..];
            if head.contains(['\n', '^']) {
                (None, text.as_str())
            } else {
                (Some(head), rest.strip_prefix('}').unwrap_or(rest))
            }
        }
        None => (None, text.as_str()),
    };
    let mut lines = Vec::new();
    let mut y = 0.0;
    if let Some(t) = title {
        let lh = ui.line_height(TITLE);
        for l in ui.wrap_lines(TITLE, max_w, t.trim()) {
            lines.push((TITLE, l, y));
            y += lh;
        }
    }
    let lh = ui.line_height(BODY);
    let gap = (lh * PARAGRAPH_GAP).round();
    for part in body.trim_start_matches(['\n', ' ']).trim_end().split('\n') {
        let part = part.trim();
        if part.is_empty() {
            y += gap;
            continue;
        }
        for l in ui.wrap_lines(BODY, max_w, part) {
            lines.push((BODY, l, y));
            y += lh;
        }
    }
    (lines, y)
}

/// Draws [`layout`]'s lines centred in box `r` (`pad` px inside).
fn draw_lines(
    ui: &mut super::Ui,
    lines: &[(&'static str, String, f32)],
    r: URect,
    pad: f32,
    alpha: f32,
) {
    let inner = r.w - 2.0 * pad;
    for (style, l, y) in lines {
        let x = r.x + pad + ((inner - ui.text_width(style, l)) * 0.5).max(0.0);
        ui.text_line(style, Vec2::new(x.round(), r.y + pad + y), l, alpha);
    }
}
