//! The in-game menu (Esc / `G` / the HUD's options button): Return to Game,
//! Options Menu, Exit to Main Menu, Quit to Desktop; the pause key's "Game
//! Paused" banner and the screen dim of both. The pause itself is
//! [`gd_game_core::pause::GamePause`].
//!
//! Grim Dawn.exe:
//! - the window is the HUD master table's `hudExitWindow`
//!   (`records/ui/optionsmenu/optionsmenu_mastertable.dbr`, `exitwindow.tpl`,
//!   loader `HudExitWindow::Load`): the record's window rect (251 × 384, centred),
//!   `exitBaseBitmap` at its `bitmapPosition`, then `resumeButton`,
//!   `optionsButton`, `exitButton`, `exitGameButton` (each its up / over /
//!   down bitmap and `textTag` in its `style`, centred; Return's up bitmap is
//!   the red `optionsbuttonfocusup.tex`); `exitWindowTitle` has no text;
//! - buttons (`HudExitWindow::OnButtonEvent`): Return hides the window; Options hides it and
//!   opens the options window (`HudExitWindow::OpenOptions`: pause `(1, 0, 0)`); Exit and
//!   Quit ask first through the dialog manager (types 8 / 9, Yes / No,
//!   `tagOptionsMenuExitMenuConfirm` / `tagOptionsMenuExitGameConfirm`);
//!   a Yes saves (`InGameUI::SaveGame`) and leaves (`HudExitWindow::Exit`);
//! - showing the window pauses `(1, sound 1, banner 0)`, hiding it unpauses
//!   (`HudExitWindow::Show`, `gd_game::pause` module docs);
//! - while the window is up the HUD's mouse handler (`UIPlayerHud::HandleMouseEvent`) tests only
//!   `hudMenuOptionsButton` (its rollover, and a press, which hides the
//!   window again), then hands the event to the window: nothing else under
//!   it reacts (`rollover.rs`, `tip.rs`, `tooltip.rs` hide their tooltips);
//! - the dim (game window update `UIPlayerHud::Update`, draw `UIPlayerHud::Render`): a black
//!   full-screen rect whose alpha moves to its target (0.6 paused, 0
//!   running) by 0.001 per ms; the banner flag is set again once the dim is
//!   back at 0; with the flag (the pause key) the banner draws over the dim:
//!   `tagGamePaused` in `style_cinematictext.dbr` centred on the canvas,
//!   `UI/Styles/CinematicTextIconLeft.tex` with its right edge at
//!   W/2 − (text width / 2 + 10 × UI scale) and `…Right.tex` with its left
//!   edge at W/2 + (text width / 2 + 10 × UI scale), both centred
//!   vertically, everything at alpha dim / 0.7;
//! - the pause key (action 40, default `P`) toggles only while neither the
//!   options window nor this window is up (`InGameUI::HandleKeyEvent`); the game menu key
//!   is action 7 (`tagKeybind07`, default `G`; `keybindings.txt` defaults
//!   `KeyMap::SetDefaultBindings`).
//!
//! The confirmation box is the dialog manager's look
//! (`records/ui/generic/promptbox/prompt_manager.dbr`: background, Yes / No
//! buttons; `prompt_text.dbr` for the message). PORT: the box is centred on
//! the screen and the dialog manager's own layout code is not decompiled.
//! The box answers on PRESS (Yes / No / close), like `apply.rs`'s box and the
//! in-game window buttons (`CharacterWindow::OnButtonEvent` event 0); the dialog
//! manager's own button event is not decompiled. The menu's four buttons act
//! on press too.
//!
//! PORT: Esc is the shared dispatcher's (`ui::esc_dispatcher`): it closes the frontmost window
//! (the achievements and loot filter windows included) and opens this menu only when none is open;
//! while the menu, the options window, a confirmation
//! or the pause key holds the game, the keyboard and mouse reach nothing
//! else (the real pause key still lets windows open). Exit to Main Menu
//! saves, then tears the session down in the running process and shows the main
//! menu (`exit.rs`).
//!
//! The window is built when it opens (`build`, in game only); the buttons' focus
//! looks and the box's looks are hidden roots of their own that `live` shows
//! by the cursor, so a hover or a press never rebuilds.
//!
//! Hooks (hooks.rs): `GD_GAME_MENU` opens the menu after `secs`, `GD_PAUSE` presses the pause key,
//! `GD_GAME_MENU_CONFIRM=exit|quit` (with `GD_GAME_MENU`) shows that confirmation, `GD_GAME_MENU_YES` answers Yes.
use super::Ui;
use super::contains;
use super::window::Rebuild;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, WindowDef};
use gd_game_core::data::GameData;
use gd_game_core::pause::{GamePause, PauseKey};

pub const RECORD: &str = "records/ui/optionsmenu/optionsmenu_mastertable.dbr";
const BANNER_STYLE: &str = "records/ui/styles/text/style_cinematictext.dbr";
const BANNER_LEFT: &str = "ui/styles/cinematictexticonleft.tex";
const BANNER_RIGHT: &str = "ui/styles/cinematictexticonright.tex";
/// The dim's target while paused, and its speed per second (0.001 / ms).
const DIM: f32 = 0.6;
const DIM_RATE: f32 = 1.0;

pub struct GameMenuPlugin;

impl Plugin for GameMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameMenu>()
            .init_resource::<GameMenuLayout>()
            .init_resource::<Dim>()
            .add_systems(
                PreUpdate,
                input
                    .in_set(GameMenuInputSet)
                    .after(bevy::input::InputSystems)
                    .run_if(resource_exists::<gd_game_core::session::SessionReady>),
            )
            .add_systems(Update, leave)
            .add_systems(
                Update,
                (draw_dim, build, live)
                    .chain()
                    .after(leave)
                    // The Esc dispatcher opens / closes the menu in `UiInputSet`: build it the same frame.
                    .after(super::UiInputSet)
                    .in_set(super::UiPlaySet),
            );
    }
}

/// A clickable part of the menu or its confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Resume,
    Options,
    Exit,
    Quit,
    Yes,
    No,
    /// The confirmation's close button (answers No, PORT: `confirm.rs`).
    Close,
}

/// What a confirmation asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    Exit,
    Quit,
}

/// The menu's state.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct GameMenu {
    pub open: bool,
    pub confirm: Option<Confirm>,
    /// A confirmed exit, done the frame after its save request.
    leaving: Option<Confirm>,
}

impl GameMenu {
    /// The menu is up with no confirmation over it: of the HUD, only its options button takes the mouse
    /// (`UIPlayerHud::HandleMouseEvent`, module doc).
    pub fn alone(&self) -> bool {
        self.open && self.confirm.is_none()
    }

    /// Whether the held game (the menu, its confirmation or the options window: [`GamePause::menu`]) keeps
    /// the mouse from the HUD's `field`: no hover look, press look, rollover or tooltip. Of the HUD only
    /// `hudMenuOptionsButton` stays live, while the menu is up alone (`UIPlayerHud::HandleMouseEvent`, module doc).
    /// The press sounds are blocked by the screen-wide blocker `build` and the options window register.
    pub fn blocks_hud(&self, pause: &GamePause, field: &str) -> bool {
        pause.menu && (field != "hudMenuOptionsButton" || !self.alone())
    }
}

/// The built menu (physical px), read by [`input`] the next frame.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct GameMenuLayout {
    pub buttons: Vec<(URect, Part)>,
    /// The HUD's options button (`hudMenuOptionsButton`).
    pub hud_button: Option<URect>,
}

/// The screen dim (alpha) and the banner flag (module docs).
#[derive(Resource)]
pub(crate) struct Dim {
    alpha: f32,
    banner: bool,
}

impl Default for Dim {
    fn default() -> Self {
        Self {
            alpha: 0.0,
            banner: true,
        }
    }
}

/// Any other window is open (the factions key's UI-mode test, `factions::toggle_key`).
#[derive(bevy::ecs::system::SystemParam)]
pub struct OpenWindows<'w> {
    panels: Res<'w, super::UiPanels>,
    map: Res<'w, crate::state::map::MapWindow>,
    rift: Res<'w, gd_game::riftgates::RiftgateMap>,
    journal: Res<'w, super::quests::QuestJournal>,
    potions: Res<'w, gd_game::cauldron::PotionWindow>,
    conversation: Res<'w, gd_game::conversation::Conversation>,
    shrine: Res<'w, gd_game::devotion::ShrineWindow>,
    altar: Res<'w, crate::state::ascendant::AltarMenu>,
    pet_menu: Res<'w, super::pets::PetMenu>,
    /// An NPC window (merchant, stash, Inventor, Illusionist, ...): the Illusionist's stands alone, with no
    /// inventory beside it, so `panels.character` does not cover it.
    trade: Res<'w, crate::state::trade::Trade>,
    factions: Res<'w, super::factions::FactionsView>,
    achievements: Res<'w, super::achievements::AchievementsView>,
    /// Open, or with its Defaults confirmation up (the Esc dispatcher cancels the confirmation first).
    loot_filter: Res<'w, super::lootfilter::LootFilterView>,
}

impl OpenWindows<'_> {
    pub fn any(&self) -> bool {
        let p = &self.panels;
        p.character
            || p.skills
            || p.devotion
            || self.map.open
            || self.rift.open
            || self.journal.open
            || self.potions.open
            || self.conversation.view.is_some()
            || self.shrine.shrine.is_some()
            || self.altar.open
            || self.pet_menu.0.is_some()
            || self.trade.open.is_some()
            || self.factions.open
            || self.achievements.open
            || self.loot_filter.open
    }
}

/// `G` / `P`, menu clicks; swallows the input while the game is held (Esc is the shared
/// dispatcher's, `ui::esc_dispatcher`, which runs after this in `Update`).
#[allow(clippy::too_many_arguments)]
pub fn input(
    hk: Res<gd_game_core::hooks::Hooks>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<GameMenuLayout>,
    mut menu: ResMut<GameMenu>,
    mut options: ResMut<crate::state::options::OptionsWindow>,
    mut pause: ResMut<GamePause>,
    mut pause_key: MessageWriter<PauseKey>,
    mut save: MessageWriter<gd_game::save::SaveRequest>,
    data: Res<GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
    (time, mut hooks, mut clicks, bindings): (
        Res<Time<Real>>,
        Local<(bool, bool)>,
        MessageReader<bevy::input::mouse::MouseButtonInput>,
        Res<super::keys::KeyBindings>,
    ),
) {
    // The held game resets `ButtonInput` below; drain the raw messages so they do not pile up
    // (`live` reads its own copy for the pressed look). Buttons act on press, from `ButtonInput`
    // like the HUD button path.
    clicks.clear();
    let t = time.elapsed_secs();
    // This frame's pause-key intent (the request is applied by gd-game after the UI input): the
    // input reset below reads it, the resource still holds last frame's applied value.
    let mut key = pause.key;
    if !hooks.0 && hk.ui.game_menu.is_some_and(|s| t >= s) {
        hooks.0 = true;
        menu.open = true;
        menu.confirm = match hk.ui.game_menu_confirm.clone().as_deref() {
            Some("exit") => Some(Confirm::Exit),
            Some("quit") => Some(Confirm::Quit),
            _ => None,
        };
        // `GD_GAME_MENU_YES`: answers Yes at once (the save is requested as for a click).
        if hk.ui.game_menu_yes
            && let Some(c) = menu.confirm.take()
        {
            save.write(gd_game::save::SaveRequest);
            menu.leaving = Some(c);
        }
    }
    if !hooks.1 && hk.ui.pause.is_some_and(|s| t >= s) {
        hooks.1 = true;
        key = true;
        pause_key.write(PauseKey(Some(true)));
    }
    let cursor = window.single().ok().and_then(|w| super::cursor_px(&hk, w));
    let clicked = super::widgets::button_pressed(&mouse);
    // Esc is the shared dispatcher's (`ui::esc_dispatcher`), which runs after this in `Update`.
    let mut took_press = false;
    if options.open {
        // The options window handles its own keys and clicks (`options.rs`).
    } else if let Some(c) = menu.confirm {
        let part = cursor.filter(|_| clicked).and_then(|p| {
            layout
                .buttons
                .iter()
                .find(|(r, _)| contains(r, p))
                .map(|b| b.1)
        });
        // PORT: Y / N (beside Enter / Esc) are the port's keys for the box.
        if matches!(part, Some(Part::No | Part::Close)) || keys.just_pressed(KeyCode::KeyN) {
            menu.confirm = None;
            if part == Some(Part::Close)
                && let Some(pak) = super::confirm::close_sound(&data)
            {
                sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
            }
        } else if part == Some(Part::Yes)
            || keys.just_pressed(KeyCode::Enter)
            || keys.just_pressed(KeyCode::KeyY)
        {
            menu.confirm = None;
            save.write(gd_game::save::SaveRequest);
            menu.leaving = Some(c);
        }
    } else if menu.open {
        let part = cursor.filter(|_| clicked).and_then(|p| {
            layout
                .buttons
                .iter()
                .find(|(r, _)| contains(r, p))
                .map(|b| b.1)
        });
        // The HUD's options button is still live under the menu and closes it (module doc).
        let hud = cursor
            .filter(|_| mouse.just_pressed(MouseButton::Left))
            .zip(layout.hud_button)
            .is_some_and(|(p, r)| contains(&r, p));
        if bindings.just_pressed(super::keys::GAME_MENU, &keys) || part == Some(Part::Resume) || hud
        {
            menu.open = false;
            // The button took the press: it does not reach the world as the game resumes.
            took_press = hud || part.is_some();
        } else if part == Some(Part::Options) {
            menu.open = false;
            options.open = true;
            took_press = true;
        } else if part == Some(Part::Exit) {
            menu.confirm = Some(Confirm::Exit);
        } else if part == Some(Part::Quit) {
            menu.confirm = Some(Confirm::Quit);
        }
    } else {
        let hud = cursor
            .filter(|_| mouse.just_pressed(MouseButton::Left))
            .zip(layout.hud_button)
            .is_some_and(|(p, r)| contains(&r, p));
        if bindings.just_pressed(super::keys::GAME_MENU, &keys) || hud {
            menu.open = true;
            // The menu replaces the pause key's pause (and its banner).
            key = false;
            pause_key.write(PauseKey(Some(false)));
        } else if bindings.just_pressed(super::keys::PAUSE, &keys) {
            key = !key;
            pause_key.write(PauseKey(None));
        }
    }
    let held = menu.open || menu.confirm.is_some() || options.open;
    if pause.menu != held {
        pause.menu = held;
    }
    if (held && !options.open) || key || took_press {
        super::keys::reset_but_escape(&mut keys);
        mouse.reset_all();
    }
}

/// A confirmed Exit / Quit, the frame after its save request was handled.
fn leave(
    mut menu: ResMut<GameMenu>,
    mut exit: MessageWriter<AppExit>,
    mut exit_menu: MessageWriter<crate::state::exit::ExitToMenu>,
    mut waited: Local<bool>,
) {
    let Some(c) = menu.leaving else { return };
    if !*waited {
        *waited = true;
        return;
    }
    *waited = false;
    menu.leaving = None;
    match c {
        Confirm::Exit => {
            exit_menu.write(crate::state::exit::ExitToMenu);
        }
        Confirm::Quit => {
            info!("game menu: quit");
            exit.write(AppExit::Success);
        }
    }
}

#[derive(Component)]
struct DimRoot;

#[derive(Component)]
struct BannerPart;

/// The dim and the banner, updated every frame from real time.
#[allow(clippy::too_many_arguments)]
fn draw_dim(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    pause: Res<GamePause>,
    time: Res<Time<Real>>,
    mut dim: ResMut<Dim>,
    mut roots: Query<(Entity, &mut BackgroundColor), With<DimRoot>>,
    mut parts: Query<&mut ImageNode, With<BannerPart>>,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<Rebuild<(UVec2, bool, u32)>>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        built.reset();
    }
    // Only the game window's pause (menu, options, key) moves the dim; the
    // focus loss pauses game time alone (`gd_game::pause` module docs).
    let target = if pause.menu || pause.key { DIM } else { 0.0 };
    // The key pause shows the banner; the menu's pause hides it; it is
    // armed again once the dim is gone.
    if pause.menu {
        dim.banner = false;
    } else if pause.key {
        dim.banner = true;
    }
    let step = DIM_RATE * time.delta_secs().max(0.0);
    dim.alpha = if (dim.alpha - target).abs() <= step {
        target
    } else {
        dim.alpha + step * (target - dim.alpha).signum()
    };
    if dim.alpha == 0.0 && target == 0.0 {
        dim.banner = true;
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let banner = dim.banner && dim.alpha > 0.0;
    let shown = dim.alpha > 0.0;
    let sig = (size, banner, assets.scale(size).to_bits());
    if !shown {
        for (e, _) in &roots {
            commands.entity(e).try_despawn();
        }
        built.reset();
        return;
    }
    let dim_color = || Color::srgba(0.0, 0.0, 0.0, dim.alpha);
    if built.changed(sig) || roots.is_empty() {
        for (e, _) in &roots {
            commands.entity(e).try_despawn();
        }
        let root = commands
            .spawn((
                DimRoot,
                super::hud::KeepsShown,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(dim_color()),
                GlobalZIndex(super::window::Z_MENU - 10),
            ))
            .id();
        if banner {
            let s = assets.scale(size);
            let mut ui = assets.ui(
                &mut commands,
                &data,
                root,
                s,
                1.0 / win.scale_factor(),
                Vec2::new(size.x as f32, size.y as f32),
            );
            let text = ui.fonts.tag("tagGamePaused").to_string();
            let (w, h) = (
                ui.text_width(BANNER_STYLE, &text),
                ui.line_height(BANNER_STYLE),
            );
            let (cx, cy) = ((ui.screen.x / 2.0).floor(), (ui.screen.y / 2.0).floor());
            let half = w * 0.5 + 10.0 * s;
            for (path, left) in [(BANNER_LEFT, true), (BANNER_RIGHT, false)] {
                let Some((img, sz)) = ui.image(path) else {
                    continue;
                };
                let (iw, ih) = (ui.sc(sz.x as f32), ui.sc(sz.y as f32));
                let x = if left { cx - half - iw } else { cx + half };
                let e = ui.quad(
                    img,
                    URect::new(x.round(), (cy - ih * 0.5).round(), iw, ih),
                    None,
                    Color::WHITE,
                );
                ui.commands.entity(e).insert(BannerPart);
            }
            if let Some(e) = ui.text_line(
                BANNER_STYLE,
                Vec2::new((cx - w * 0.5).round(), (cy - h * 0.5).round()),
                &text,
                1.0,
            ) {
                ui.commands.entity(e).insert(BannerPart);
            }
        }
        return;
    }
    for (_, mut bg) in &mut roots {
        bg.0 = dim_color();
    }
    let a = (dim.alpha / 0.7).min(1.0);
    for mut img in &mut parts {
        img.color = Color::srgba(1.0, 1.0, 1.0, a);
    }
}

#[derive(Component)]
struct MenuRoot;

/// A root shown only for one state of the mouse: the window is built once with every state and [`live`] shows
/// the one that holds (no rebuild on a hover or a press).
#[derive(Component, Clone, Copy, PartialEq)]
enum Layer {
    /// A menu button in its focus look, over its up look.
    Button(Part),
    /// The confirmation box drawn for this [`super::confirm::Look`].
    Box(super::confirm::Look),
}

/// The menu's z (the shared band, `window::Z_MENU`), and that of the buttons' focus looks over it.
const Z: i32 = super::window::Z_MENU;

/// A hidden root for [`Layer`] `l` (a root of its own, so the button sounds registered in it rank by its z).
fn layer(commands: &mut Commands, l: Layer, z: i32) -> Entity {
    gd_ui_kit::spawn_root(
        commands,
        (MenuRoot, super::hud::KeepsShown, l, Visibility::Hidden),
        z,
    )
}

/// The menu window and the confirmation box: rebuilt when the window size or the menu's state changes.
#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    menu: Res<GameMenu>,
    mut layout: ResMut<GameMenuLayout>,
    roots: Query<Entity, With<MenuRoot>>,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<Rebuild<(UVec2, u32, bool, GameMenu)>>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        built.reset();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    // The UI scale too: an applied UI Scale rebuilds the menu at the new size.
    let s = assets.scale(size);
    let sb = s.to_bits();
    // The HUD style too (`classic_hud`): the menu's look follows it.
    let classic = assets.classic_hud();
    if size.x == 0
        || !built.changed_by(
            |b| b.0 == size && b.1 == sb && b.2 == classic && b.3 == *menu,
            || (size, sb, classic, menu.clone()),
        )
    {
        return;
    }
    super::window::despawn_all(&mut commands, &roots);
    let root = gd_ui_kit::spawn_root(&mut commands, (MenuRoot, super::hud::KeepsShown), Z);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let hud_button = super::rollover::button_rect(&mut ui, "hudMenuOptionsButton");
    *layout = GameMenuLayout {
        hud_button,
        ..default()
    };
    // While the menu or its confirmation is up it takes the whole screen's mouse (`ui/widgets.rs`): the
    // HUD under it neither sounds nor presses. The HUD's options button stays live while the menu is up
    // alone (module doc): its sound is registered again here, in the menu's band, in front of the blocker.
    if menu.open || menu.confirm.is_some() {
        ui.sound_blocker(URect::new(0.0, 0.0, ui.screen.x, ui.screen.y));
        if menu.alone()
            && let Some(r) = hud_button
            && let Some(rec) = ui
                .record(ui.hud_master())
                .and_then(|m| m.str("hudMenuOptionsButton").map(str::to_owned))
        {
            ui.record_path_button_sound(r, &rec);
        }
    }
    if let Some(c) = menu.confirm {
        confirm_box(&mut ui, c, &mut layout);
    } else if menu.open
        && let Some(rec) = ui.record(RECORD)
    {
        let mut def = WindowDef::from_record(&rec);
        let field = |k: &str| rec.str(k).unwrap_or("").to_string();
        // The buttons auto-size to their labels (`widgets::static_button`); measure them first
        // so the window (and its background) grows by the widest button's growth and every
        // button keeps its centre in the window. PORT: the stock window is fixed at its record
        // extent and a long localised label overflows the button (UX over parity).
        let mut buttons = Vec::new();
        let mut grow = 0.0f32;
        for (k, part) in [
            ("resumeButton", Part::Resume),
            ("optionsButton", Part::Options),
            ("exitButton", Part::Exit),
            ("exitGameButton", Part::Quit),
        ] {
            let Some(b) = ui.record(&field(k)) else {
                continue;
            };
            let Some(tex_w) = b
                .str("bitmapNameUp")
                .and_then(|p| ui.image(p))
                .map(|(_, size)| ui.sc(size.x as f32))
            else {
                continue;
            };
            let text = b.str("textTag").map(|t| ui.fonts.tag(t).to_string());
            let w = super::widgets::button_width(&mut ui, &b, tex_w, text.as_deref());
            grow = grow.max(w - tex_w);
            buttons.push((b, part, tex_w, w));
        }
        def.w += grow / s;
        let r = def.screen_rect(ui.screen.x, ui.screen.y, s);
        let o = Vec2::new(r.x, r.y);
        if let Some(bg) = ui.record(&field("exitBaseBitmap"))
            && let Some(path) = bg.str("bitmapName")
        {
            let (bx, by) = (
                bg.f32("bitmapPositionX").unwrap_or(0.0),
                bg.f32("bitmapPositionY").unwrap_or(0.0),
            );
            if grow > 0.0
                && let Some((h, size)) = ui.image(path)
            {
                ui.quad_stretch(
                    h,
                    URect::new(
                        o.x + ui.sc(bx),
                        o.y + ui.sc(by),
                        ui.sc(size.x as f32) + grow,
                        ui.sc(size.y as f32),
                    ),
                    Color::WHITE,
                );
            } else {
                ui.bitmap(path, bx, by, o);
            }
        }
        for (b, part, tex_w, w) in buttons {
            // Re-centre: the window grew by `grow`, this button by `w - tex_w`.
            let ob = o + Vec2::new((grow - (w - tex_w)) * 0.5, 0.0);
            let label = super::widgets::Label::Record;
            if let Some(br) = super::widgets::static_button(&mut ui, &b, ob, true, false, label) {
                ui.record_button_sound(br, &b);
                layout.buttons.push((br, part));
                // The focus look, shown by `live` while the cursor is on the button.
                ui.root = layer(ui.commands, Layer::Button(part), Z + 1);
                super::widgets::static_button(&mut ui, &b, ob, true, true, label);
                ui.root = root;
            }
        }
    }
}

/// The Yes / No box of a confirmation (`confirm.rs`), its parts recorded for the input: drawn once per look
/// of the mouse (nothing hovered, each part hovered, the close button held), each in its [`Layer`].
fn confirm_box(ui: &mut Ui, c: Confirm, layout: &mut GameMenuLayout) {
    use super::confirm::Hit;
    let tag = match c {
        Confirm::Exit => "tagOptionsMenuExitMenuConfirm",
        Confirm::Quit => "tagOptionsMenuExitGameConfirm",
    };
    let root = ui.root;
    for (i, look) in [
        (None, false),
        (Some(Hit::Yes), false),
        (Some(Hit::No), false),
        (Some(Hit::Close), false),
        (Some(Hit::Close), true),
    ]
    .into_iter()
    .enumerate()
    {
        ui.root = layer(ui.commands, Layer::Box(look), Z);
        let rects = super::confirm::confirm_box(ui, tag, look);
        ui.root = root;
        if let (0, Some(r)) = (i, rects) {
            if let Some(close) = r.close {
                layout.buttons.push((close, Part::Close));
            }
            layout.buttons.push((r.yes, Part::Yes));
            layout.buttons.push((r.no, Part::No));
        }
    }
}

/// Per frame, without a rebuild: shows the [`Layer`]s of the part under the cursor. The left button's state is
/// read from the raw messages ([`input`] resets `ButtonInput` while the menu holds the game).
fn live(
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<gd_game_core::hooks::Hooks>,
    layout: Res<GameMenuLayout>,
    mut clicks: MessageReader<bevy::input::mouse::MouseButtonInput>,
    mut pressed: Local<bool>,
    mut layers: Query<(&Layer, &mut Visibility)>,
) {
    use super::confirm::Hit;
    for e in clicks.read() {
        if e.button == MouseButton::Left {
            *pressed = e.state.is_pressed();
        }
    }
    if layers.is_empty() {
        return;
    }
    let hover = window
        .single()
        .ok()
        .and_then(|w| super::cursor_px(&hooks, w))
        .and_then(|p| {
            layout
                .buttons
                .iter()
                .find(|(r, _)| contains(r, p))
                .map(|b| b.1)
        });
    let hit = match hover {
        Some(Part::Yes) => Some(Hit::Yes),
        Some(Part::No) => Some(Hit::No),
        Some(Part::Close) => Some(Hit::Close),
        _ => None,
    };
    // Only the close button has a down look (`confirm::close_slot`).
    let look = (hit, hit == Some(Hit::Close) && *pressed);
    for (l, mut vis) in &mut layers {
        let shown = match *l {
            Layer::Button(p) => hover == Some(p),
            Layer::Box(l) => l == look,
        };
        vis.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// The game menu's keyboard / mouse input.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameMenuInputSet;
