//! The main menu screens (state and input: `menu.rs`): the play menu, the
//! Difficulty Select, Create Character and Delete Character windows. PORT: the engine's version label in the
//! bottom-right corner (`MainMenu::Initialize`, `Engine::GetVersion`) is not drawn.
//!
//! Grim Dawn.exe:
//! - the play menu is `records/ui/mainmenu/playmenu/playmenu_mastertable.dbr`
//!   (`playMenu` of `mainmenu_constants.dbr`); each widget record places its
//!   bitmap by `bitmapAlignmentX/Y` and `bitmapPosition` against the screen
//!   (or its window) with `GraphicsAlign` (Engine.dll):
//!   Left x, Right W − x, Center W / 2 + x; Top y, Bottom H − y, Center
//!   H / 2 + y; the bitmap's top-left corner goes there;
//! - the character list (`characterPicker`, draw `CharacterList::Draw`): a
//!   `backgroundColor` rect with the ragged border; rows `characterHeight`
//!   apart from `scrollSize` down, each the name (`nameFont`, the selected
//!   row `selectNameFont`, a dead hardcore one `hardcoreDeadNameFont`)
//!   centred at `nameTextPosition` and "Level N Class" (`tagCharLevelClass`,
//!   the class tag's `[ms]` / `[fs]` form by sex; `tagCharLevel` without a
//!   class) at `classTextPosition`; a hardcore row's `hardcoreTexture` at
//!   `hardcorePosition` from its right / centre; `nameTexture` (hardcore:
//!   `hardcoreNameTexture`) additive around the name's text rect padded by
//!   `nameTexturePadding`; a dead one's `hardcoreSlashTexture` across it; the
//!   selected row's `selectTexture` over the row. The scroll buttons are
//!   enabled while there is a row above / below the selection (`CharacterList::Update`);
//! - the difficulty window (`difficultyWindow`, picks `DifficultyWindow::ApplySelection`, enables
//!   `DifficultyWindow::UpdateEnabled`, text `DifficultyWindow::UpdateText`): the picked option's title and
//!   description tags, its large image, its button's `…ButtonSelect`
//!   overlay; the bonus lines `LootTag`, `DamageTag`, `HealthTag`,
//!   `ResistsTag`, `ExpTag` with the constants' `difficultyRegularLoot`,
//!   `…Damage`, `…Health`, `…Resists`, `…Exp` entry of the option (Normal 0,
//!   Veteran 1, Elite 2, Ultimate 3, Ultimate Veteran 4), each shown only when
//!   its value is non-zero;
//! - the main difficulty button's text is the option's `button_tag`
//!   (`MainMenu::GetDifficultyButtonText`).
//! - Create, Delete and the difficulty button are toggle buttons
//!   (`toggleButton`): held down while their window is open (not decompiled).
//!
//! Behind the screens: the menu record's 3D scene with the selected character
//! standing in it (`render/menu.rs`, `gd_ui::doll::menu_character`), the
//! record's vignette frame (`mmVignetteBitmap`, e.g.
//! `mainmenu_vignetteframe_gdx3.tex`, a black shade) and its logo record
//! (`mmLogo`, e.g. `gdx3_menulogo.dbr`: `x`, `y`, `width`, `height` as screen
//! fractions, additive). PORT: the logo is a flat quad (no particles).
//! Sounds: each enabled `menubutton` / `menucheckbutton` registers its `soundNameOver` / `soundNameDown`
//! (`MenuButton::HandleMouseEvent`).
//!
//! Hover looks move in place: every hover-driven widget is drawn in its base state with its state bitmaps
//! resolved once ([`HoverLook`]); [`live`] swaps them by the cursor.
//! Not decompiled: the list's height (rows × `characterHeight` + 2 × `scrollSize`, bottom at
//! `positionY`), label placement (text centred on the anchor unless `textAlignmentX` is Left, vertically
//! centred unless Top), check-button text 2 px right of its bitmap, disabled-text alpha 0.5, a refused name's
//! error line under the name box (the real one is a message box).
use super::Ui;
use super::text::FontCache;
use crate::menu::{CONSTANTS, DifficultyOption as D, Hit, MainMenu, MenuLayout, MenuWindow};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::arz::Record;
use gd_formats::ui::{Rect as URect, WindowDef};
use gd_game_core::data::GameData;
use gd_game_core::session::is_dead;

const PLAY: &str = "records/ui/mainmenu/playmenu/playmenu_mastertable.dbr";

pub struct MainMenuUiPlugin;

impl Plugin for MainMenuUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                build.run_if(resource_exists::<MainMenu>),
                live.run_if(resource_exists::<MainMenu>),
                clear.run_if(not(resource_exists::<MainMenu>)),
            )
                .chain(),
        );
    }
}

const MENU_Z: i32 = 120;

#[derive(Component)]
struct MenuRoot;

fn clear(mut commands: Commands, roots: Query<Entity, With<MenuRoot>>) {
    for e in &roots {
        commands.entity(e).try_despawn();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Up,
    Disabled,
    /// A toggle button held down (its window is open).
    Down,
}

/// A hover-driven widget's state bitmaps, swapped by [`live`]. Sizes are physical px (the node's are
/// `size × inv`).
#[derive(Component)]
enum HoverLook {
    /// A `menubutton` drawn up: its `bitmapNameInFocus` while hovered (`None`: the up look stays).
    Button {
        hit: Hit,
        up: (Handle<Image>, Vec2),
        over: Option<(Handle<Image>, Vec2)>,
    },
    /// A `menucheckbutton`'s bitmaps in (down, hover) order — up, over, down, down-over — each fallen back to up.
    Check {
        hit: Hit,
        down: bool,
        states: [(Handle<Image>, Vec2); 4],
    },
    /// A list row's hover highlight, hidden until hovered.
    Row { hit: Hit },
}

/// `GraphicsAlign` of a record position inside `c`.
fn align(c: URect, ax: &str, ay: &str, x: f32, y: f32) -> Vec2 {
    let px = match ax {
        "Right" => c.x + c.w - x,
        "Center" => c.x + c.w * 0.5 + x,
        _ => c.x + x,
    };
    let py = match ay {
        "Bottom" => c.y + c.h - y,
        "Center" => c.y + c.h * 0.5 + y,
        _ => c.y + y,
    };
    Vec2::new(px.round(), py.round())
}

fn place(ui: &Ui, rec: &Record, c: URect, prefix: &str) -> Vec2 {
    let k = |s: &str| format!("{prefix}{s}");
    let (x, y) = (
        ui.sc(rec.f32(&k("PositionX")).unwrap_or(0.0)),
        ui.sc(rec.f32(&k("PositionY")).unwrap_or(0.0)),
    );
    align(
        c,
        rec.str(&k("AlignmentX")).unwrap_or("Left"),
        rec.str(&k("AlignmentY")).unwrap_or("Top"),
        x,
        y,
    )
}

fn localize(fonts: &FontCache, tag: &str) -> String {
    fonts.tag(tag).replace(['{', '}'], "").replace("^n", "\n")
}

/// One line of `text` centred on `x` (or starting at it) with its top at `y`.
fn text_at(
    ui: &mut Ui,
    style: &str,
    x: f32,
    y: f32,
    text: &str,
    centred: bool,
    alpha: f32,
) -> URect {
    text_tinted(
        ui,
        style,
        x,
        y,
        text,
        centred,
        Color::srgba(1.0, 1.0, 1.0, alpha),
    )
}

fn text_tinted(
    ui: &mut Ui,
    style: &str,
    x: f32,
    y: f32,
    text: &str,
    centred: bool,
    tint: Color,
) -> URect {
    let (w, h) = (ui.text_width(style, text), ui.line_height(style));
    let x = if centred { (x - w * 0.5).round() } else { x };
    ui.text_line_tinted(style, Vec2::new(x, y.round()), text, tint);
    URect::new(x, y, w, h)
}

/// A `menubutton` record: its bitmap for `state`, its text centred. An up button with a `hit` carries
/// [`HoverLook::Button`].
fn button(
    ui: &mut Ui,
    rec: &Record,
    c: URect,
    state: State,
    text: Option<&str>,
    hit: Option<Hit>,
) -> Option<URect> {
    let key = match state {
        State::Disabled if rec.str("bitmapNameDisabled").is_some() => "bitmapNameDisabled",
        State::Down if rec.str("bitmapNameDown").is_some() => "bitmapNameDown",
        _ => "bitmapNameUp",
    };
    let (img, size) = rec.str(key).map(str::to_owned).and_then(|p| ui.image(&p))?;
    let text = text
        .map(str::to_owned)
        .or_else(|| rec.str("textTag").map(|t| localize(ui.fonts, t)));
    let style = rec.str("textFont").map(str::to_owned);
    // PORT: the stock button is fixed at its bitmap's pixel width; here it grows to fit the label, keeping its
    // alignment.
    let tex_w = ui.sc(size.x as f32);
    let lw = match (&style, &text) {
        (Some(st), Some(t)) => t
            .split('\n')
            .map(|l| ui.text_width(st, l))
            .fold(0.0, f32::max),
        _ => 0.0,
    };
    let w = tex_w.max(lw + 2.0 * ui.sc(10.0));
    let grow = w - tex_w;
    let mut p = place(ui, rec, c, "bitmap");
    p.x = (p.x
        - match rec.str("bitmapAlignmentX").unwrap_or("Left") {
            "Right" => grow,
            "Center" => grow * 0.5,
            _ => 0.0,
        })
    .round();
    let r = URect::new(p.x, p.y, w, ui.sc(size.y as f32));
    // `MenuButton::Draw`: a disabled button without a `bitmapNameDisabled` draws its up bitmap multiplied by
    // (0.6, 0.6, 0.6, 1).
    let disabled_up = state == State::Disabled && key == "bitmapNameUp";
    let tint = if disabled_up {
        Color::srgb(0.6, 0.6, 0.6)
    } else {
        Color::WHITE
    };
    // Stretched, so the hover bitmap [`live`] swaps in fills the grown rect.
    let e = if grow > 0.0 {
        ui.quad_stretch(img.clone(), r, tint)
    } else {
        ui.quad(img.clone(), r, None, tint)
    };
    if state == State::Up
        && let Some(h) = hit
    {
        let over = rec
            .str("bitmapNameInFocus")
            .and_then(|p| ui.image(p))
            .map(|(h, sz)| (h, Vec2::new(w.max(ui.sc(sz.x as f32)), ui.sc(sz.y as f32))));
        ui.commands.entity(e).insert(HoverLook::Button {
            hit: h,
            up: (img, Vec2::new(r.w, r.h)),
            over,
        });
    }
    if let (Some(style), Some(t)) = (style, text) {
        let h = ui.line_height(&style);
        let dy = ui.sc(rec.f32("textOffsetY").unwrap_or(0.0));
        // `MenuButton::Draw`: a disabled button's text colour and shadow are (0.5, 0.5, 0.5, 0.5) (PORT: multiplied
        // into the style's colour).
        let tint = if state == State::Disabled {
            Color::srgba(0.5, 0.5, 0.5, 0.5)
        } else {
            Color::WHITE
        };
        text_tinted(
            ui,
            &style,
            r.x + r.w * 0.5,
            r.y + (r.h - h) * 0.5 + dy,
            &t,
            true,
            tint,
        );
    }
    Some(r)
}

/// A `menucheckbutton` record (radio / check box) with its text right of it, drawn unhovered; returns the bitmap
/// and text rect together.
fn check(ui: &mut Ui, rec: &Record, c: URect, down: bool, hit: Hit) -> Option<URect> {
    let (up_h, up_sz) = ui.image(rec.str("bitmapNameUp")?)?;
    let up = (
        up_h,
        Vec2::new(ui.sc(up_sz.x as f32), ui.sc(up_sz.y as f32)),
    );
    let state = |ui: &mut Ui, key: &str| {
        rec.str(key)
            .and_then(|p| ui.image(p))
            .map(|(h, sz)| (h, Vec2::new(ui.sc(sz.x as f32), ui.sc(sz.y as f32))))
            .unwrap_or_else(|| up.clone())
    };
    let states = [
        up.clone(),
        state(ui, "bitmapNameInFocus"),
        state(ui, "bitmapNameDown"),
        state(ui, "bitmapNameDownInFocus"),
    ];
    let (img, size) = states[down as usize * 2].clone();
    let p = place(ui, rec, c, "bitmap");
    let r = URect::new(p.x, p.y, size.x, size.y);
    let e = ui.quad(img, r, None, Color::WHITE);
    ui.commands
        .entity(e)
        .insert(HoverLook::Check { hit, down, states });
    let mut hit = r;
    if let (Some(style), Some(tag)) = (rec.str("textFont").map(str::to_owned), rec.str("textTag")) {
        let t = localize(ui.fonts, tag);
        let h = ui.line_height(&style);
        let tr = text_at(
            ui,
            &style,
            r.x + r.w + ui.sc(2.0),
            r.y + (r.h - h) * 0.5,
            &t,
            false,
            1.0,
        );
        hit.w = tr.x + tr.w - r.x;
    }
    Some(hit)
}

/// A `menulabel` record: `text` (else its tag) at its anchor, wrapped at
/// `maxWidth`, over its glow bitmap padded by `bitmapPadding`.
fn label(ui: &mut Ui, rec: &Record, c: URect, text: Option<&str>) {
    label_dy(ui, rec, c, text, 0.0);
}

/// `label` shifted vertically by `dy` (negative lifts); see [`section_header`].
fn label_dy(ui: &mut Ui, rec: &Record, c: URect, text: Option<&str>, dy: f32) {
    let Some(style) = rec.str("textFont").map(str::to_owned) else {
        return;
    };
    let text = text.map(str::to_owned).unwrap_or_else(|| {
        rec.str("textTag")
            .map_or(String::new(), |t| localize(ui.fonts, t))
    });
    let p = place(ui, rec, c, "bitmap");
    let left = rec.str("textAlignmentX") == Some("Left");
    let top = rec.str("textAlignmentY") == Some("Top");
    let max_w = rec.f32("maxWidth").map(|w| ui.sc(w)).filter(|w| *w > 0.0);
    let lines = match max_w {
        Some(w) => ui.wrap_lines(&style, w, &text),
        None => text.split('\n').map(str::to_owned).collect(),
    };
    let lh = ui.line_height(&style);
    // Lines `ySpacing` × the font size apart.
    let size = ui
        .fonts
        .style(ui.data, &style)
        .map_or(lh, |f| f.scaled_size(ui.s) as f32);
    let step = rec.f32("ySpacing").map_or(lh, |k| (size * k).round());
    let total = step * (lines.len().max(1) - 1) as f32 + lh;
    let y0 = (if top {
        p.y
    } else {
        (p.y - total * 0.5).round()
    }) + dy;
    if let Some(bg) = rec.str("bitmapName").map(str::to_owned)
        && let Some((img, _)) = ui.image(&bg)
    {
        let w = lines
            .iter()
            .map(|l| ui.text_width(&style, l))
            .fold(0.0, f32::max);
        let (px, py) = (
            ui.sc(rec.f32("bitmapPaddingX").unwrap_or(0.0)),
            ui.sc(rec.f32("bitmapPaddingY").unwrap_or(0.0)),
        );
        let x = if left { p.x } else { p.x - w * 0.5 };
        super::additive::quad(
            ui,
            img,
            URect::new(
                (x - px).round(),
                (y0 - py).round(),
                (w + 2.0 * px).round(),
                (total + 2.0 * py).round(),
            ),
            Color::WHITE,
        );
    }
    for (i, l) in lines.iter().enumerate() {
        text_at(ui, &style, p.x, y0 + step * i as f32, l, !left, 1.0);
    }
}

/// A section header (`mapLabel` / `boardLabel`) lifted so its text bottom sits a gap above the box it heads.
/// PORT: vertical placement from the box, not the record anchor (which collides with the box's border).
fn section_header(ui: &mut Ui, rec: &Record, c: URect, text: Option<&str>, box_top: f32) {
    let Some(style) = rec.str("textFont").map(str::to_owned) else {
        return;
    };
    let body = text.map(str::to_owned).unwrap_or_else(|| {
        rec.str("textTag")
            .map_or(String::new(), |t| localize(ui.fonts, t))
    });
    let p = place(ui, rec, c, "bitmap");
    let lh = ui.line_height(&style);
    let n = match rec.f32("maxWidth").map(|w| ui.sc(w)).filter(|w| *w > 0.0) {
        Some(w) => ui.wrap_lines(&style, w, &body).len(),
        None => body.split('\n').count(),
    };
    let size = ui
        .fonts
        .style(ui.data, &style)
        .map_or(lh, |f| f.scaled_size(ui.s) as f32);
    let step = rec.f32("ySpacing").map_or(lh, |k| (size * k).round());
    let total = step * (n.max(1) - 1) as f32 + lh;
    let bottom = if rec.str("textAlignmentY") == Some("Top") {
        p.y + total
    } else {
        p.y + total * 0.5
    };
    let dy = (box_top - ui.sc(12.0) - bottom).min(0.0);
    label_dy(ui, rec, c, text, dy);
}

/// The eight pieces of a `name_*.tex` border pattern around `r` (outside it, like [`Ui::border`]).
fn pattern_border(ui: &mut Ui, pattern: &str, r: URect) {
    let piece = |k: &str| pattern.replace('*', k);
    let Some((ul, uls)) = ui.image(&piece("lt")) else {
        return;
    };
    let (cw, ch) = (ui.sc(uls.x as f32), ui.sc(uls.y as f32));
    ui.quad_stretch(ul, URect::new(r.x - cw, r.y - ch, cw, ch), Color::WHITE);
    let pieces: [(&str, f32, f32, Option<f32>, Option<f32>); 7] = [
        ("rt", r.x + r.w, r.y - ch, None, None),
        ("lb", r.x - cw, r.y + r.h, None, None),
        ("rb", r.x + r.w, r.y + r.h, None, None),
        ("ct", r.x, r.y - ch, Some(r.w), None),
        ("cb", r.x, r.y + r.h, Some(r.w), None),
        ("lm", r.x - cw, r.y, None, Some(r.h)),
        ("rm", r.x + r.w, r.y, None, Some(r.h)),
    ];
    for (k, x, y, w, h) in pieces {
        let Some((img, size)) = ui.image(&piece(k)) else {
            continue;
        };
        let (iw, ih) = (ui.sc(size.x as f32), ui.sc(size.y as f32));
        ui.quad_stretch(
            img,
            URect::new(x, y, w.unwrap_or(iw), h.unwrap_or(ih)),
            Color::WHITE,
        );
    }
}

/// A `menutextbox` record: its box, border and `text` centred in it.
fn text_box(ui: &mut Ui, rec: &Record, c: URect, text: &str) -> URect {
    let p = align(
        c,
        rec.str("alignmentX").unwrap_or("Left"),
        rec.str("alignmentY").unwrap_or("Top"),
        ui.sc(rec.f32("textPositionX").unwrap_or(0.0)),
        ui.sc(rec.f32("textPositionY").unwrap_or(0.0)),
    );
    let r = URect::new(
        p.x,
        p.y,
        ui.sc(rec.f32("textBoxXSize").unwrap_or(200.0)),
        ui.sc(rec.f32("textBoxYSize").unwrap_or(30.0)),
    );
    if let Some(b) = rec.str("borderBitmapName") {
        pattern_border(ui, b, r);
    }
    if let Some(style) = rec.str("textFont").map(str::to_owned) {
        let h = ui.line_height(&style);
        text_at(
            ui,
            &style,
            r.x + r.w * 0.5,
            r.y + (r.h - h) * 0.5,
            text,
            true,
            1.0,
        );
    }
    r
}

/// What the Game Type window lists from disk, read when the window opens: the Crucible leaderboard and the
/// arenas the install offers.
#[derive(Default)]
struct GameTypeLists {
    board: gd_formats::save::survival::Leaderboard,
    arenas: Vec<usize>,
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    menu: Res<MainMenu>,
    mut layout: ResMut<MenuLayout>,
    roots: Query<Entity, With<MenuRoot>>,
    (options, credits): (
        Res<gd_ui::state::options::OptionsWindow>,
        Res<super::credits::Credits>,
    ),
    save_dir: Res<gd_game_core::session::SaveDir>,
    backdrop: Option<Res<gd_render::render::backdrop::MenuBackdrop>>,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<Option<(UVec2, u32, MenuWindow, bool, bool, Option<usize>, u64)>>,
    mut lists: Local<Option<GameTypeLists>>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        *built = Default::default();
        *lists = None;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 {
        return;
    }
    let view = backdrop.as_ref().map(|b| &b.view);
    let sig = (
        size,
        menu.revision,
        menu.window.clone(),
        options.open,
        credits.open,
        view.map(|v| v.index),
        assets.fonts.generation,
    );
    if built.as_ref() == Some(&sig) {
        return;
    }
    *built = Some(sig);
    for e in &roots {
        commands.entity(e).try_despawn();
    }
    let s = assets.scale(size);
    let root = gd_ui_kit::spawn_root(&mut commands, MenuRoot, MENU_Z);
    let (w, h) = (size.x as f32, size.y as f32);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(w, h),
    );
    let screen = URect::new(0.0, 0.0, w, h);
    let mut hits: Vec<(URect, Hit)> = Vec::new();
    if let Some(v) = view {
        background(&mut ui, v);
    }
    preview(&mut ui, &menu);
    // The credits roll replaces the button panel (menu state 9: the scene, character and logo stay).
    if credits.open {
        layout.hits.clear();
        return;
    }
    let Some(play) = ui.record(PLAY) else { return };
    let field = |k: &str| {
        play.str(k)
            .map(str::to_owned)
            .and_then(|p| ui.data.db.get(&p))
    };
    let modal = menu.window != MenuWindow::None;
    if let Some(r) = field("centerShadowBitmap")
        && let Some(path) = r.str("bitmapName").map(str::to_owned)
        && let Some((img, sz)) = ui.image(&path)
    {
        let p = place(&ui, r, screen, "bitmap");
        ui.quad(
            img,
            URect::new(p.x, p.y, ui.sc(sz.x as f32), ui.sc(sz.y as f32)),
            None,
            Color::WHITE,
        );
    }
    let current = menu.current();
    let can_start = menu.can_start();
    // The Crucible names its difficulties Aspirant / Challenger / Gladiator (`tagSDifficultyTitle01..03`).
    let difficulty_text = localize(
        ui.fonts,
        if menu.crucible {
            match menu.option {
                D::Elite => "tagSDifficultyTitle02",
                D::Ultimate => "tagSDifficultyTitle03",
                _ => "tagSDifficultyTitle01",
            }
        } else {
            menu.option.button_tag()
        },
    );
    // The Game Type button names the mode (`MainMenu::GetGameTypeTag`:
    // tagPlayGameModeCampaign / Survival / Custom).
    let mode_text = localize(
        ui.fonts,
        if menu.crucible {
            "tagPlayGameModeSurvival"
        } else if menu.custom.is_some() {
            "tagPlayGameModeCustom"
        } else {
            "tagPlayGameModeCampaign"
        },
    );
    let buttons: [(&str, Option<Hit>, bool, Option<&str>); 12] = [
        ("createButton", Some(Hit::Create), true, None),
        ("deleteButton", Some(Hit::Delete), current.is_some(), None),
        ("multiplayerButton", None, false, None),
        ("gameGuideButton", None, false, None),
        ("communityButton", None, false, None),
        ("buyDLCButton", None, false, None),
        (
            "difficultyButton",
            Some(Hit::Difficulty),
            true,
            Some(difficulty_text.as_str()),
        ),
        ("startButton", Some(Hit::Start), can_start, None),
        (
            "gameTypeButton",
            Some(Hit::GameType),
            true,
            Some(mode_text.as_str()),
        ),
        ("exitButton", Some(Hit::Exit), true, None),
        ("optionsButton", Some(Hit::Options), true, None),
        ("creditsButton", Some(Hit::Credits), true, None),
    ];
    let mut buttons_top = f32::MAX;
    for (k, hit, enabled, text) in buttons {
        let Some(rec) = field(k) else { continue };
        let open = matches!(
            (hit, &menu.window),
            (Some(Hit::Create), MenuWindow::Create(_))
                | (Some(Hit::Delete), MenuWindow::Delete(_))
                | (Some(Hit::Difficulty), MenuWindow::Difficulty(_))
                | (Some(Hit::GameType), MenuWindow::GameType(_))
        );
        let state = if open {
            State::Down
        } else if enabled {
            State::Up
        } else {
            State::Disabled
        };
        if let Some(r) = button(&mut ui, rec, screen, state, text, hit) {
            buttons_top = buttons_top.min(r.y);
            if let Some(h) = hit.filter(|_| enabled) {
                // Under a modal window the play menu takes no clicks, so it plays no sounds either.
                if !modal {
                    ui.record_button_sound(r, rec);
                }
                hits.push((r, h));
            }
        }
    }
    // A refused Start (save/mod mismatch, mod map failure) says why, above the button row.
    if let Some(err) = menu.error.clone() {
        let style = "records/ui/styles/text/style_mainmenu_createoptions01.dbr";
        let max_w = screen.w - ui.sc(24.0);
        let t = ui.truncate_line(style, max_w, &format!("^r{err}"));
        let lh = ui.line_height(style);
        let cx = screen.x + screen.w * 0.5;
        let y = buttons_top - lh - ui.sc(6.0);
        text_at(&mut ui, style, cx, y, &t, true, 1.0);
    }
    if let Some(list) = field("characterPicker") {
        character_list(&mut ui, list, &menu, &mut hits, &mut layout.rows);
    }
    if !matches!(menu.window, MenuWindow::GameType(_)) {
        *lists = None;
    }
    match &menu.window {
        MenuWindow::None => {}
        MenuWindow::Difficulty(pick) => difficulty_window(&mut ui, &menu, *pick, false, &mut hits),
        MenuWindow::NewDifficulty(_, pick) => {
            difficulty_window(&mut ui, &menu, *pick, true, &mut hits)
        }
        MenuWindow::GameType(g) => {
            let lists = lists.get_or_insert_with(|| GameTypeLists {
                board: std::fs::read(save_dir.0.join(gd_game::survival::LEADERBOARD_FILE))
                    .ok()
                    .and_then(|b| gd_formats::save::survival::Leaderboard::parse(&b).ok())
                    .unwrap_or_default(),
                arenas: gd_game_core::session::available_arenas(data.root()),
            });
            gametype_window(&mut ui, g, lists, &mut hits, &mut layout.mod_rows)
        }
        MenuWindow::Create(new) => create_window(&mut ui, new, &mut hits),
        MenuWindow::Delete(typed) => delete_window(&mut ui, &menu, typed, &mut hits),
    }
    layout.hits = if modal {
        hits.into_iter()
            .filter(|(_, h)| {
                matches!(
                    h,
                    Hit::Pick(_)
                        | Hit::Accept
                        | Hit::Cancel
                        | Hit::Male
                        | Hit::Female
                        | Hit::Hardcore
                        | Hit::ModeCampaign
                        | Hit::ModeCrucible
                        | Hit::ModeCustom
                        | Hit::ModItem(_)
                        | Hit::MapDrop
                        | Hit::MapItem(_)
                )
            })
            .collect()
    } else {
        hits
    };
}

/// Per frame, without a rebuild: the hover look of the widget under the cursor ([`HoverLook`]), suppressed while
/// the options or the credits are up. Writes only on a change.
fn live(
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<MenuLayout>,
    (options, credits): (
        Res<gd_ui::state::options::OptionsWindow>,
        Res<super::credits::Credits>,
    ),
    mut looks: Query<(&HoverLook, &mut ImageNode, &mut Node)>,
    mut rows: Query<(&HoverLook, &mut Visibility), Without<ImageNode>>,
) {
    if looks.is_empty() && rows.is_empty() {
        return;
    }
    let Ok(win) = window.single() else { return };
    let hover = super::cursor_px(win)
        .and_then(|p| layout.at(p))
        .filter(|_| !options.open && !credits.open);
    let inv = 1.0 / win.scale_factor();
    for (look, mut img, mut node) in &mut looks {
        let want = match look {
            HoverLook::Button { hit, up, over } => match over {
                Some(o) if hover == Some(*hit) => o,
                _ => up,
            },
            HoverLook::Check { hit, down, states } => {
                &states[*down as usize * 2 + (hover == Some(*hit)) as usize]
            }
            HoverLook::Row { .. } => continue,
        };
        if img.image != want.0 {
            img.image = want.0.clone();
            node.width = Val::Px(want.1.x * inv);
            node.height = Val::Px(want.1.y * inv);
        }
    }
    for (look, mut vis) in &mut rows {
        if let HoverLook::Row { hit } = look {
            vis.set_if_neq(if hover == Some(*hit) {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
}

fn preview(ui: &mut Ui, menu: &MainMenu) {
    let Some(e) = menu.current() else { return };
    let female = e.gdc.header.sex == 0;
    let looks = gd_game_core::player::equipped_items(&e.gdc)
        .iter()
        .map(|i| gd_sim::items::illusion::look_record(i).to_owned())
        .collect();
    gd_ui::doll::menu_character(ui, (female, looks));
}

fn background(ui: &mut Ui, view: &gd_render::render::backdrop::MenuView) {
    let (w, h) = (ui.screen.x, ui.screen.y);
    // The frame is black with its shade in alpha.
    if let Some((v, _)) = view.vignette.as_deref().and_then(|v| ui.image(v)) {
        ui.quad_stretch(v, URect::new(0.0, 0.0, w, h), Color::WHITE);
    }
    if let Some(logo) = view.logo.as_deref().and_then(|l| ui.record(l))
        && let Some(path) = logo.str("baseTexture").map(str::to_owned)
        && let Some((img, _)) = ui.image(&path)
    {
        let (lw, lh) = (
            logo.f32("width").unwrap_or(0.27) * w,
            logo.f32("height").unwrap_or(0.13) * h,
        );
        let x = w * 0.5 + logo.f32("x").unwrap_or(0.0) * w - lw * 0.5;
        let y = logo.f32("y").unwrap_or(0.0) * h;
        let r = URect::new(x.round(), y.round(), lw.round(), lh.round());
        if logo.str("style") == Some("additive") {
            super::additive::quad(ui, img, r, Color::WHITE);
        } else {
            ui.quad_stretch(img, r, Color::WHITE);
        }
    }
}

/// "Level 54 Warlock": the class tag's form for the character's sex.
fn class_line(fonts: &FontCache, g: &gd_formats::save::gdc::Gdc) -> String {
    let level = g.header.level.to_string();
    if g.header.class_tag.is_empty() {
        return localize(fonts, "tagCharLevel").replace("%d0", &level);
    }
    let class = fonts.tag(&g.header.class_tag);
    let class = match (class.find("[ms]"), class.find("[fs]")) {
        (Some(m), Some(f)) => {
            let male = g.header.sex != 0;
            let (a, b) = if m < f { (m, f) } else { (f, m) };
            let (first, second) = (&class[a + 4..b], &class[b + 4..]);
            if (male && m < f) || (!male && f < m) {
                first
            } else {
                second
            }
        }
        _ => class,
    };
    localize(fonts, "tagCharLevelClass")
        .replace("%d0", &level)
        .replace("%t1", class.trim())
}

fn character_list(
    ui: &mut Ui,
    rec: &Record,
    menu: &MainMenu,
    hits: &mut Vec<(URect, Hit)>,
    rows_out: &mut usize,
) {
    let max = rec.f32("maxCharacters").unwrap_or(12.0) as usize;
    *rows_out = max;
    let n = menu.entries.len();
    let rows = n.min(max);
    if rows == 0 {
        return;
    }
    let f = |k: &str, d: f32| rec.f32(k).unwrap_or(d);
    let (ch, scroll, margin) = (
        ui.sc(f("characterHeight", 40.0)),
        ui.sc(f("scrollSize", 8.0)),
        ui.sc(f("boxMargin", 0.0)),
    );
    let bottom = align(
        URect::new(0.0, 0.0, ui.screen.x, ui.screen.y),
        rec.str("alignmentX").unwrap_or("Left"),
        rec.str("alignmentY").unwrap_or("Bottom"),
        ui.sc(f("positionX", 5.0)),
        ui.sc(f("positionY", 380.0)),
    );
    let height = rows as f32 * ch + 2.0 * scroll;
    let r = URect::new(
        bottom.x,
        bottom.y - height,
        ui.sc(f("scrollBoxWidth", 275.0)),
        height,
    );
    // Blended in gamma space as the engine's.
    let bg = Color::srgba(
        f("backgroundColor.R", 0.0),
        f("backgroundColor.G", 0.0),
        f("backgroundColor.B", 0.0),
        f("backgroundColor.A", 0.6),
    );
    ui.solid(r, bg);
    ui.border(rec, "", r, 1.0);
    // The arrows' hit rects go in after the rows (`MenuLayout::at` takes
    // the last match), so an arrow that overlaps a row still gets its click.
    let mut arrows = Vec::new();
    for (k, hit, enabled) in [
        ("scrollUpButton", Hit::ScrollUp, menu.selected > 0),
        ("scrollDownButton", Hit::ScrollDown, menu.selected + 1 < n),
    ] {
        let Some(b) = rec
            .str(k)
            .map(str::to_owned)
            .and_then(|p| ui.data.db.get(&p))
        else {
            continue;
        };
        let state = if enabled { State::Up } else { State::Disabled };
        if let Some(br) = button(ui, b, r, state, None, Some(hit))
            && enabled
        {
            // A Create/Delete/Difficulty/Game Type window takes the clicks, so the list stays quiet.
            if menu.window == MenuWindow::None {
                ui.record_button_sound(br, b);
            }
            arrows.push((br, hit));
        }
    }
    let row_w = r.w - 2.0 * margin;
    for (k, i) in (menu.first..(menu.first + rows).min(n)).enumerate() {
        let g = &menu.entries[i].gdc;
        let row = URect::new(r.x + margin, r.y + scroll + k as f32 * ch, row_w, ch);
        hits.push((row, Hit::Row(i)));
        let (dead, selected, hardcore) = (is_dead(g), i == menu.selected, g.header.hardcore != 0);
        let font = |normal: &str, select: &str, dead_f: &str| {
            rec.str(if dead {
                dead_f
            } else if selected {
                select
            } else {
                normal
            })
            .unwrap_or("")
            .to_owned()
        };
        let name_font = font("nameFont", "selectNameFont", "hardcoreDeadNameFont");
        let class_font = font("classFont", "selectClassFont", "hardcoreDeadClassFont");
        let name = text_at(
            ui,
            &name_font,
            row.x + row.w * 0.5 + ui.sc(f("nameTextPositionX", 0.0)),
            row.y + ui.sc(f("nameTextPositionY", 0.0)),
            &g.header.name,
            true,
            1.0,
        );
        let line = class_line(ui.fonts, g);
        text_at(
            ui,
            &class_font,
            row.x + row.w * 0.5 + ui.sc(f("classTextPositionX", 0.0)),
            row.y + ui.sc(f("classTextPositionY", 0.0)),
            &line,
            true,
            1.0,
        );
        if hardcore
            && let Some(path) = rec.str("hardcoreTexture").map(str::to_owned)
            && let Some((img, sz)) = if dead {
                ui.image_gray(&path)
            } else {
                ui.image(&path)
            }
        {
            let (iw, ih) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
            let x = row.x + row.w - ui.sc(f("hardcorePositionX", 24.0)) - iw;
            let y = row.y + row.h * 0.5 + ui.sc(f("hardcorePositionY", -16.0)) - ih * 0.5;
            ui.quad(
                img,
                URect::new(x.round(), y.round(), iw, ih),
                None,
                Color::WHITE,
            );
        }
        if let Some(path) = rec
            .str(if hardcore {
                "hardcoreNameTexture"
            } else {
                "nameTexture"
            })
            .map(str::to_owned)
            && let Some((img, _)) = ui.image(&path)
        {
            let (px, py) = (
                ui.sc(f("nameTexturePaddingX", 60.0)),
                ui.sc(f("nameTexturePaddingY", 10.0)),
            );
            super::additive::quad(
                ui,
                img,
                URect::new(
                    name.x - px,
                    name.y - py,
                    name.w + 2.0 * px,
                    name.h + 2.0 * py,
                ),
                Color::WHITE,
            );
        }
        if dead
            && let Some(path) = rec.str("hardcoreSlashTexture").map(str::to_owned)
            && let Some((img, sz)) = ui.image(&path)
        {
            let (px, py) = (
                ui.sc(f("slashTexturePaddingX", 80.0)),
                ui.sc(f("slashTexturePaddingY", 0.0)),
            );
            ui.quad_stretch(
                img,
                URect::new(
                    name.x - px,
                    name.y + name.h * 0.5 - py,
                    name.w + 2.0 * px,
                    2.0 * py + ui.sc(sz.y as f32),
                ),
                Color::WHITE,
            );
        }
        if selected
            && let Some(path) = rec.str("selectTexture").map(str::to_owned)
            && let Some((img, _)) = ui.image(&path)
        {
            ui.quad_stretch(img, row, Color::WHITE);
        }
    }
    hits.extend(arrows);
}

/// A menu-layer window: `windowDefaultX/Y` place its top-left corner by `GraphicsAlign` (the menu records hold
/// −extent / 2 for a centred one).
fn window_rect(ui: &Ui, rec: &Record) -> URect {
    let d = WindowDef::from_record(rec);
    let (sx, sy) = (
        rec.str("windowScreenAlignmentX").unwrap_or("Left"),
        rec.str("windowScreenAlignmentY").unwrap_or("Top"),
    );
    let p = align(
        URect::new(0.0, 0.0, ui.screen.x, ui.screen.y),
        sx,
        sy,
        ui.sc(d.x),
        ui.sc(d.y),
    );
    URect::new(p.x, p.y, ui.sc(d.w), ui.sc(d.h))
}

fn child(ui: &Ui, rec: &Record, k: &str) -> Option<Record> {
    rec.str(k)
        .map(str::to_owned)
        .and_then(|p| ui.data.db.get_owned(&p))
}

fn image_at(ui: &mut Ui, rec: &Record, c: URect, additive: bool) {
    let Some(path) = rec.str("bitmapName").map(str::to_owned) else {
        return;
    };
    let Some((img, sz)) = ui.image(&path) else {
        return;
    };
    let p = place(ui, rec, c, "bitmap");
    let r = URect::new(p.x, p.y, ui.sc(sz.x as f32), ui.sc(sz.y as f32));
    if additive || rec.str("styleName") == Some("additive") {
        super::additive::quad(ui, img, r, Color::WHITE);
    } else {
        ui.quad(img, r, None, Color::WHITE);
    }
}

fn accept_cancel(
    ui: &mut Ui,
    rec: &Record,
    c: URect,
    accept: bool,
    accept_text: Option<&str>,
    hits: &mut Vec<(URect, Hit)>,
) {
    for (k, hit, enabled) in [
        ("acceptButton", Hit::Accept, accept),
        ("cancelButton", Hit::Cancel, true),
    ] {
        let Some(b) = child(ui, rec, k) else { continue };
        let state = if enabled { State::Up } else { State::Disabled };
        let text = accept_text.filter(|_| hit == Hit::Accept);
        if let Some(r) = button(ui, &b, c, state, text, Some(hit))
            && enabled
        {
            ui.record_button_sound(r, &b);
            hits.push((r, hit));
        }
    }
}

fn difficulty_window(
    ui: &mut Ui,
    menu: &MainMenu,
    pick: D,
    new_character: bool,
    hits: &mut Vec<(URect, Hit)>,
) {
    let Some(consts) = ui.record(CONSTANTS) else {
        return;
    };
    let Some(rec) = consts
        .str("difficultyWindow")
        .map(str::to_owned)
        .and_then(|p| ui.record(&p))
    else {
        return;
    };
    let c = window_rect(ui, &rec);
    if let Some(bg) = child(ui, &rec, "backgroundImage") {
        image_at(ui, &bg, c, false);
    }
    if let Some(t) = child(ui, &rec, "windowTitle") {
        label(ui, &t, c, None);
    }
    let crucible = menu.crucible;
    let idx = if crucible {
        match pick {
            D::Elite => 1,
            D::Ultimate => 2,
            _ => 0,
        }
    } else {
        D::ALL.iter().position(|d| *d == pick).unwrap_or(0)
    };
    let (title, desc, field) = match pick {
        // The Crucible's three (`survivalNormal` / `Elite` / `Ultimate`, `tagSDifficultyTitle01..03`).
        D::Elite if crucible => (
            "tagSDifficultyTitle02",
            "tagSDifficultyTitle02Desc",
            "survivalElite",
        ),
        D::Ultimate if crucible => (
            "tagSDifficultyTitle03",
            "tagSDifficultyTitle03Desc",
            "survivalUltimate",
        ),
        _ if crucible => (
            "tagSDifficultyTitle01",
            "tagSDifficultyTitle01Desc",
            "survivalNormal",
        ),
        D::Normal => (
            "tagRDifficultyTitle01",
            "tagRDifficultyTitle01Desc",
            "normal",
        ),
        D::Veteran => (
            "tagChallengeDifficulty",
            "tagChallengeDifficultyDesc",
            "veteran",
        ),
        D::Elite => (
            "tagRDifficultyTitle02",
            "tagRDifficultyTitle02Desc",
            "elite",
        ),
        D::Ultimate => (
            "tagRDifficultyTitle03",
            "tagRDifficultyTitle03Desc",
            "ultimate",
        ),
        D::UltimateVeteran => (
            "tagChallengeDifficulty03",
            "tagChallengeDifficulty03Desc",
            "ultimateVeteran",
        ),
    };
    if let Some(img) = child(ui, &rec, &format!("{field}Image")) {
        image_at(ui, &img, c, false);
    }
    let t = localize(ui.fonts, title);
    if let Some(l) = child(ui, &rec, "difficultyTitle") {
        label(ui, &l, c, Some(&t));
    }
    let d = localize(ui.fonts, desc);
    if let Some(l) = child(ui, &rec, "difficultyLabel") {
        label(ui, &l, c, Some(&d));
    }
    let lines = [
        ("lootLabel", "LootTag", "difficultyRegularLoot"),
        ("damageLabel", "DamageTag", "difficultyRegularDamage"),
        ("healthLabel", "HealthTag", "difficultyRegularHealth"),
        ("resistsLabel", "ResistsTag", "difficultyRegularResists"),
        ("expLabel", "ExpTag", "difficultyRegularExp"),
    ];
    for (k, tag, values) in lines {
        let survival_values = values.replace("difficultyRegular", "difficultySurvival");
        let values = if crucible { &survival_values } else { values };
        let v = consts.f32s(values).get(idx).copied().unwrap_or(0.0) as i64;
        if v == 0 {
            continue;
        }
        let Some(tag) = rec.str(tag).map(str::to_owned) else {
            continue;
        };
        let text = localize(ui.fonts, &tag).replace("%d0", &v.to_string());
        if let Some(l) = child(ui, &rec, k) {
            label(ui, &l, c, Some(&text));
        }
    }
    let g = menu.current().map(|e| &e.gdc).filter(|_| !new_character);
    let buttons: Vec<(D, &str)> = if crucible {
        vec![
            (D::Normal, "survivalNormal"),
            (D::Elite, "survivalElite"),
            (D::Ultimate, "survivalUltimate"),
        ]
    } else {
        D::ALL
            .iter()
            .copied()
            .zip(["normal", "veteran", "elite", "ultimate", "ultimateVeteran"])
            .collect()
    };
    for (o, k) in buttons.iter() {
        let Some(b) = child(ui, &rec, &format!("{k}Button")) else {
            continue;
        };
        let enabled = o.enabled_in(g, crucible);
        let state = if enabled { State::Up } else { State::Disabled };
        if let Some(r) = button(ui, &b, c, state, None, Some(Hit::Pick(*o))) {
            if state != State::Disabled {
                ui.record_button_sound(r, &b);
            }
            hits.push((r, Hit::Pick(*o)));
        }
        if *o == pick
            && let Some(sel) = child(ui, &rec, &format!("{k}ButtonSelect"))
        {
            image_at(ui, &sel, c, true);
        }
    }
    accept_cancel(ui, &rec, c, true, None, hits);
}

/// The game-mode window (`GameModeWindow`, `gamemode_mastertable.dbr`, 610x570 centred;
/// `GameModeWindow::HandleEvent` / load `GameModeWindow::Load`): the Campaign and Crucible
/// toggles, with the Crucible the map drop-down ("Random" and the offered arenas) and the local
/// leaderboard (character, difficulty icon, score); with Custom (`GameModeWindow::ShowCustomView`) the `mapLabel`
/// heading, the selected map's file name (`modText`) and the custom maps' list (`modList`) instead, and no
/// campaign image. PORT: the drop-down's and the lists' looks are taken from the records only (no scroll
/// bar; the mod list scrolls by the wheel, `menu.rs`).
fn gametype_window(
    ui: &mut Ui,
    g: &crate::menu::GameType,
    lists: &GameTypeLists,
    hits: &mut Vec<(URect, Hit)>,
    mod_rows: &mut usize,
) {
    let Some(rec) = ui
        .record(CONSTANTS)
        .and_then(|c| c.str("gameModeWindow").map(str::to_owned))
        .and_then(|p| ui.record(&p))
    else {
        return;
    };
    let c = window_rect(ui, &rec);
    if let Some(bg) = child(ui, &rec, "backgroundImage") {
        image_at(ui, &bg, c, false);
    }
    if let Some(t) = child(ui, &rec, "windowTitle") {
        label(ui, &t, c, None);
    }
    if !g.crucible
        && !g.custom
        && let Some(img) = child(ui, &rec, "campaignImage")
    {
        image_at(ui, &img, c, false);
    }
    for (k, hit, down) in [
        (
            "campaignButton",
            Hit::ModeCampaign,
            !g.crucible && !g.custom,
        ),
        ("customButton", Hit::ModeCustom, g.custom),
        ("survivalButton", Hit::ModeCrucible, g.crucible),
    ] {
        // `GameModeWindow::Load` creates `survivalButton` only with the Crucible owned.
        let Some(b) =
            child(ui, &rec, k).filter(|_| hit != Hit::ModeCrucible || !lists.arenas.is_empty())
        else {
            continue;
        };
        if let Some(r) = check(ui, &b, c, down, hit) {
            ui.record_button_sound(r, &b);
            hits.push((r, hit));
        }
    }
    if g.crucible {
        // (`mapLabel` is the custom campaigns' list heading: it would sit on the drop-down.)
        if let Some(l) = child(ui, &rec, "boardLabel") {
            let box_top = child(ui, &rec, "boardList")
                .map_or(c.y, |b| c.y + ui.sc(b.f32("positionY").unwrap_or(270.0)));
            section_header(ui, &l, c, None, box_top);
        }
        leaderboard(ui, &rec, c, &lists.board);
        arena_drop(ui, &rec, c, g, &lists.arenas, hits);
    }
    if g.custom {
        if let Some(l) = child(ui, &rec, "mapLabel") {
            let box_top = child(ui, &rec, "modText").map_or(c.y, |b| {
                c.y + ui.sc(b.f32("textPositionY").unwrap_or(246.0))
            });
            section_header(ui, &l, c, None, box_top);
        }
        mod_text(
            ui,
            &rec,
            c,
            g.maps.get(g.pick).map_or("", |m| m.file_name()),
        );
        mod_list(ui, &rec, c, g, hits, mod_rows);
    }
    accept_cancel(ui, &rec, c, true, None, hits);
}

/// The selected custom map's file name (`gamemode_modtext.dbr`, a `menutextbox`: `textBoxXSize` ×
/// `textBoxYSize` at `textPosition` from the window's `alignment` point, the thick border, the text
/// centred; empty with no map, `GameModeWindow::ClearModText` / `GameModeWindow::UpdateModText`).
fn mod_text(ui: &mut Ui, window: &Record, c: URect, text: &str) {
    let Some(rec) = child(ui, window, "modText") else {
        return;
    };
    let f = |k: &str, d: f32| rec.f32(k).unwrap_or(d);
    let ax = match rec.str("alignmentX") {
        Some("Center") => c.w * 0.5,
        Some("Right") => c.w,
        _ => 0.0,
    };
    let r = URect::new(
        c.x + ax + ui.sc(f("textPositionX", 0.0)),
        c.y + ui.sc(f("textPositionY", 0.0)),
        ui.sc(f("textBoxXSize", 400.0)),
        ui.sc(f("textBoxYSize", 28.0)),
    );
    if let Some(b) = rec.str("borderBitmapName") {
        pattern_border(ui, b, r);
    }
    let style = rec
        .str("textFontNoEdit")
        .or(rec.str("textFont"))
        .unwrap_or("")
        .to_owned();
    let lh = ui.line_height(&style);
    text_at(
        ui,
        &style,
        r.x + r.w * 0.5,
        r.y + (r.h - lh) * 0.5,
        text,
        true,
        1.0,
    );
}

/// The custom maps' list (`gamemode_modlist.dbr`, the leaderboard's list kind without a header): one
/// "<mod> ~ <map file>" row per [`crate::menu::CustomMap`], from the first visible one
/// (`GameModeWindow::ShowCustomView`'s listbox scroll; the wheel is read in `menu.rs`). PORT: no scroll
/// bar is drawn; the selected row's look (a `dividerColor` bar) and the hovered one's are not decompiled.
fn mod_list(
    ui: &mut Ui,
    window: &Record,
    c: URect,
    g: &crate::menu::GameType,
    hits: &mut Vec<(URect, Hit)>,
    rows_out: &mut usize,
) {
    let Some(rec) = child(ui, window, "modList") else {
        return;
    };
    let f = |k: &str, d: f32| rec.f32(k).unwrap_or(d);
    // PORT: lifted so the list's top border clears the selected-map text box (the record anchors overlap).
    let mut top = c.y + ui.sc(f("positionY", 0.0));
    if let Some(mt) = child(ui, window, "modText") {
        let mt_bottom = c.y + ui
            .sc(mt.f32("textPositionY").unwrap_or(0.0) + mt.f32("textBoxYSize").unwrap_or(28.0));
        top = top.max(mt_bottom + ui.sc(4.0));
    }
    let r = URect::new(
        c.x + ui.sc(f("positionX", 0.0)),
        top,
        ui.sc(f("listBoxXSize", 400.0)),
        ui.sc(f("listBoxYSize", 190.0)),
    );
    let color = |p: &str, a: f32| {
        Color::srgba(
            f(&format!("{p}.R"), 0.0),
            f(&format!("{p}.G"), 0.0),
            f(&format!("{p}.B"), 0.0),
            f(&format!("{p}.A"), 1.0) * a,
        )
    };
    ui.solid(r, color("defaultColumnColor", 1.0));
    if let Some(b) = rec.str("panelBorderBitmapName") {
        pattern_border(ui, b, r);
    }
    let ts = rec.str("textFont").unwrap_or("").to_owned();
    let (pad, top) = (ui.sc(f("offsetX", 5.0)), ui.sc(f("offsetY", 3.0)));
    let item_h = ui.sc(f("itemHeight", 25.0));
    let rows = ((r.h - top) / item_h).floor().max(0.0) as usize;
    *rows_out = rows;
    let tl = ui.line_height(&ts);
    let first = g.first.min(g.maps.len().saturating_sub(rows));
    for (k, i) in (first..(first + rows).min(g.maps.len())).enumerate() {
        let m = &g.maps[i];
        let row = URect::new(r.x, r.y + top + k as f32 * item_h, r.w, item_h);
        if i == g.pick {
            ui.solid(row, color("dividerColor", 0.5));
        } else {
            // The hover shade, shown by `live`.
            let e = ui.solid(row, color("dividerColor", 0.25));
            ui.commands.entity(e).insert((
                HoverLook::Row {
                    hit: Hit::ModItem(i),
                },
                Visibility::Hidden,
            ));
        }
        text_at(
            ui,
            &ts,
            row.x + pad,
            row.y + (item_h - tl) * 0.5,
            &m.row(),
            false,
            1.0,
        );
        hits.push((row, Hit::ModItem(i)));
    }
}

/// The leaderboard list (`gamemode_boardlist.dbr`): header "Character" / "Score", then the entries'
/// name, difficulty icon (`survival_icondifficulty01..03`) and score, highest first.
fn leaderboard(
    ui: &mut Ui,
    window: &Record,
    c: URect,
    board: &gd_formats::save::survival::Leaderboard,
) {
    let Some(rec) = child(ui, window, "boardList") else {
        return;
    };
    let f = |k: &str, d: f32| rec.f32(k).unwrap_or(d);
    let r = URect::new(
        c.x + ui.sc(f("positionX", 0.0)),
        c.y + ui.sc(f("positionY", 0.0)),
        ui.sc(f("listBoxXSize", 400.0)),
        ui.sc(f("listBoxYSize", 190.0)),
    );
    let color = |p: &str| {
        Color::srgba(
            f(&format!("{p}.R"), 0.0),
            f(&format!("{p}.G"), 0.0),
            f(&format!("{p}.B"), 0.0),
            f(&format!("{p}.A"), 1.0),
        )
    };
    ui.solid(r, color("defaultColumnColor"));
    let header_h = ui.sc(f("columnHeaderHeight", 30.0));
    ui.solid(URect::new(r.x, r.y, r.w, header_h), color("headerColor"));
    if let Some(b) = rec.str("panelBorderBitmapName") {
        pattern_border(ui, b, r);
    }
    let (hs, ts) = (
        rec.str("headerFont").unwrap_or("").to_owned(),
        rec.str("textFont").unwrap_or("").to_owned(),
    );
    let pad = ui.sc(f("offsetX", 5.0));
    let (h1, h2) = (
        localize(ui.fonts, "tagLeaderboardHeader01"),
        localize(ui.fonts, "tagLeaderboardHeader02"),
    );
    let hl = ui.line_height(&hs);
    text_at(
        ui,
        &hs,
        r.x + r.w * 0.25,
        r.y + (header_h - hl) * 0.5,
        &h1,
        true,
        1.0,
    );
    text_at(
        ui,
        &hs,
        r.x + r.w * 0.8,
        r.y + (header_h - hl) * 0.5,
        &h2,
        true,
        1.0,
    );
    let item_h = ui.sc(f("itemHeight", 25.0));
    let rows = ((r.h - header_h) / item_h).floor().max(0.0) as usize;
    let tl = ui.line_height(&ts);
    for (i, e) in board.entries.iter().take(rows).enumerate() {
        let y = r.y + header_h + i as f32 * item_h;
        text_at(
            ui,
            &ts,
            r.x + pad,
            y + (item_h - tl) * 0.5,
            &e.name,
            false,
            1.0,
        );
        if let Some((img, sz)) = ui.image(&format!(
            "ui/mainmenu/multiplayer/survival_icondifficulty{:02}.tex",
            e.difficulty.min(2) + 1
        )) {
            let (w, h) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
            ui.quad(
                img,
                URect::new(r.x + r.w * 0.55, y + (item_h - h) * 0.5, w, h),
                None,
                Color::WHITE,
            );
        }
        text_at(
            ui,
            &ts,
            r.x + r.w * 0.8,
            y + (item_h - tl) * 0.5,
            &e.score.to_string(),
            true,
            1.0,
        );
    }
}

/// The map drop-down (`gamemode_survivalmapdropbox.dbr`): the selected arena in a bordered box with the
/// arrow, the list below it while open (Random first, then the offered arenas).
fn arena_drop(
    ui: &mut Ui,
    window: &Record,
    c: URect,
    g: &crate::menu::GameType,
    offered: &[usize],
    hits: &mut Vec<(URect, Hit)>,
) {
    let Some(rec) = child(ui, window, "survivalMapDrop") else {
        return;
    };
    let f = |k: &str, d: f32| rec.f32(k).unwrap_or(d);
    let r = URect::new(
        c.x + c.w * 0.5 + ui.sc(f("positionX", 0.0)),
        c.y + ui.sc(f("positionY", 0.0)),
        ui.sc(f("dropBoxXSize", 310.0)),
        ui.sc(f("dropBoxYSize", 24.0)),
    );
    let name = |ui: &Ui, i: usize| {
        let tag = match i {
            0 => "tagSurvivalArenaRandom",
            n => offered.get(n - 1).map_or("tagSurvivalArenaRandom", |a| {
                gd_game_core::session::ARENAS[*a].0
            }),
        };
        localize(ui.fonts, tag)
    };
    ui.solid(r, Color::srgba(0.0, 0.0, 0.0, 0.75));
    if let Some(b) = rec.str("borderBitmapName") {
        pattern_border(ui, b, r);
    }
    let (style, list_style) = (
        rec.str("textFont").unwrap_or("").to_owned(),
        rec.str("listTextFont").unwrap_or("").to_owned(),
    );
    let lh = ui.line_height(&style);
    let shown = name(ui, g.arena.min(offered.len()));
    text_at(
        ui,
        &style,
        r.x + r.w * 0.5,
        r.y + (r.h - lh) * 0.5,
        &shown,
        true,
        1.0,
    );
    let arrow = if g.open {
        "bitmapNameDown"
    } else {
        "bitmapNameUp"
    };
    if let Some((img, sz)) = rec.str(arrow).map(str::to_owned).and_then(|p| ui.image(&p)) {
        let (w, h) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
        ui.quad(
            img,
            URect::new(r.x + r.w - w - ui.sc(2.0), r.y + (r.h - h) * 0.5, w, h),
            None,
            Color::WHITE,
        );
    }
    hits.push((r, Hit::MapDrop));
    if !g.open {
        return;
    }
    let item_h = ui.sc(f("itemHeight", 24.0));
    let list = URect::new(r.x, r.y + r.h, r.w, item_h * (offered.len() + 1) as f32);
    ui.solid(list, Color::srgba(0.0, 0.0, 0.0, 0.95));
    if let Some(b) = rec.str("borderBitmapName") {
        pattern_border(ui, b, list);
    }
    let ll = ui.line_height(&list_style);
    for i in 0..=offered.len() {
        let row = URect::new(list.x, list.y + i as f32 * item_h, list.w, item_h);
        // The hover shade, shown by `live`.
        let e = ui.solid(
            row,
            Color::srgba(
                f("highlightColor.R", 0.5),
                f("highlightColor.G", 0.3),
                f("highlightColor.B", 0.1),
                f("highlightColor.A", 0.15) + 0.2,
            ),
        );
        ui.commands.entity(e).insert((
            HoverLook::Row {
                hit: Hit::MapItem(i),
            },
            Visibility::Hidden,
        ));
        let t = name(ui, i);
        text_at(
            ui,
            &list_style,
            row.x + row.w * 0.5,
            row.y + (item_h - ll) * 0.5,
            &t,
            true,
            1.0,
        );
        hits.push((row, Hit::MapItem(i)));
    }
}

fn create_window(ui: &mut Ui, new: &crate::menu::NewCharacter, hits: &mut Vec<(URect, Hit)>) {
    let Some(rec) = ui
        .record(CONSTANTS)
        .and_then(|c| c.str("createCharacterWindow").map(str::to_owned))
        .and_then(|p| ui.record(&p))
    else {
        return;
    };
    let c = window_rect(ui, &rec);
    if let Some(bg) = child(ui, &rec, "backgroundImage") {
        image_at(ui, &bg, c, false);
    }
    if let Some(l) = child(ui, &rec, "windowTitle") {
        label(ui, &l, c, None);
    }
    if let Some(l) = child(ui, &rec, "nameLabel") {
        let box_top = child(ui, &rec, "playerName").map_or(c.y, |b| {
            c.y + ui.sc(b.f32("textPositionY").unwrap_or(125.0))
        });
        section_header(ui, &l, c, None, box_top);
    }
    let mut name_box = None;
    if let Some(tb) = child(ui, &rec, "playerName") {
        // The caret while typing: `UIEditBox::Draw` (1 px white after the text, blinking every 600 ms).
        let r = text_box(ui, &tb, c, &new.name);
        if let Some(style) = tb.str("textFont") {
            let (w, h) = (ui.text_width(style, &new.name), ui.line_height(style));
            let caret = URect::new(
                r.x + (r.w + w) * 0.5,
                r.y + (r.h - h) * 0.5,
                ui.sc(1.0).max(1.0),
                h,
            );
            let e = ui.solid(caret, Color::WHITE);
            ui.commands.entity(e).insert(gd_ui::trade::SearchCaret);
        }
        name_box = Some(r);
    }
    if let (Some(err), Some(b)) = (new.error, name_box) {
        let style = "records/ui/styles/text/style_mainmenu_createoptions01.dbr";
        let t = format!("^r{}", localize(ui.fonts, err));
        text_at(
            ui,
            style,
            b.x + b.w * 0.5,
            b.y + b.h + ui.sc(4.0),
            &t,
            true,
            1.0,
        );
    }
    // PORT: `challengeButton` (Veteran at creation) is hidden; Veteran is picked in the difficulty window.
    for (k, hit, down) in [
        ("maleButton", Hit::Male, !new.female),
        ("femaleButton", Hit::Female, new.female),
        ("hardcoreButton", Hit::Hardcore, new.hardcore),
    ] {
        let Some(b) = child(ui, &rec, k) else {
            continue;
        };
        if let Some(r) = check(ui, &b, c, down, hit) {
            ui.record_button_sound(r, &b);
            hits.push((r, hit));
        }
    }
    // The label is `tagMenuButton12` ("Next"; the record's own `tagMenuButton10` reads "Create").
    let next = localize(ui.fonts, "tagMenuButton12");
    accept_cancel(ui, &rec, c, !new.name.trim().is_empty(), Some(&next), hits);
}

fn delete_window(ui: &mut Ui, menu: &MainMenu, typed: &str, hits: &mut Vec<(URect, Hit)>) {
    let Some(rec) = ui
        .record(CONSTANTS)
        .and_then(|c| c.str("deleteCharacterWindow").map(str::to_owned))
        .and_then(|p| ui.record(&p))
    else {
        return;
    };
    let c = window_rect(ui, &rec);
    if let Some(bg) = child(ui, &rec, "backgroundImage") {
        image_at(ui, &bg, c, false);
    }
    if let Some(l) = child(ui, &rec, "windowTitle") {
        label(ui, &l, c, None);
    }
    if let Some(l) = child(ui, &rec, "confirmLabel") {
        let name = menu
            .current()
            .map_or(String::new(), |e| e.gdc.header.name.clone());
        let text = format!("{name}\n{}", localize(ui.fonts, "tagMenuDelete01"));
        let box_top = child(ui, &rec, "confirmTextBox").map_or(c.y, |b| {
            c.y + ui.sc(b.f32("textPositionY").unwrap_or(175.0))
        });
        section_header(ui, &l, c, Some(&text), box_top);
    }
    if let Some(tb) = child(ui, &rec, "confirmTextBox") {
        text_box(ui, &tb, c, &format!("{typed}_"));
    }
    let word = ui.fonts.tag("tagMenuDeleteConfirm").to_owned();
    accept_cancel(
        ui,
        &rec,
        c,
        typed.trim().eq_ignore_ascii_case(&word),
        None,
        hits,
    );
}
