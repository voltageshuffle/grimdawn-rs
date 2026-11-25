//! The orb HUD's menu folder: `hudMenuFolderButton` (the three-lines button under rift travel) opens a small panel
//! left of it (`hudMenuFolderBitmap`) holding the window buttons the classic HUD keeps on its toolbar (map,
//! achievements, factions, journal, character, skills, group). `Grim Dawn.exe` (UIPlayerHud):
//! - the listener `UIPlayerHud::OnToolbarButton`: any event of the folder button (`UIPlayerHud.menuFolderButton`) flips
//!   the folder flag (`UIPlayerHud.menuFolderOpen`) and the visible flag (`UIWidget.visible`) of the map, achievements,
//!   faction, journal, character, skills and group buttons; only the orb
//!   table (`standardHUD` off) has the folder. The folder's buttons do what the classic HUD's do (`hud::menu_buttons`);
//!   pressing one leaves the folder open.
//! - the folder origin (the update `UIPlayerHud::Update`, orb only): the loader (`UIPlayerHud::Load` →
//!   `UIPlayerHud::PlaceMenuFolder`) places the background at the toolbar origin + its `bitmapPosition` × scale, like
//!   the folder's buttons. While that screen x is ≥ 0 (every normal screen) the folder offset is 0, 0: the background
//!   and buttons sit where loaded, LEFT of the folder button (stock: the background at (−116, −61), the button at (51,
//!   −27)). Only a background off the screen's left edge moves: offset = −its screen position + (folder button centre x
//!   − 0.53 × background width, folder button top − background height), its bottom edge on the button's top. The draw
//!   (`UIPlayerHud::Render`) adds the offset to the loaded positions; here [`origin`] is the toolbar origin plus that
//!   offset.
//! - the draw `UIPlayerHud::Render` (orb only): the folder button at the toolbar origin, then `hudMenuFolderBitmap`
//!   while the folder is open; later, while neither the options window nor the game menu is up, the map, achievements,
//!   faction, journal, character, skills and group buttons (their visible flags: only while open), the character /
//!   skills ones in their pulse while it runs (`hud::pulse_hud`); the options button stays on the toolbar.
//! - the mouse handler `UIPlayerHud::HandleMouseEvent` tests the folder's buttons at the origin for their
//!   `hud*ButtonRollover` (the group button only with networking: never here), and a left / right press no HUD part
//!   takes (outside the folder background, the folder button, the toolbar's parts, the teleport buttons) closes an open
//!   folder through the listener ([`close_on_world_press`]).
//!
//! PORT: the folder button shows its down bitmap while open (a toggle `ButtonStatic`, inferred from the listener
//! flipping the folder on either of its events); the "no HUD part" test is the HUD's hit rects ([`HudLayout`]), the
//! folder background and the windows' [`crate::state::inventory::UiCapture`].
use super::Ui;
use super::hud::HudLayout;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, Widget};

pub struct MenuFolderPlugin;

impl Plugin for MenuFolderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuFolder>().add_systems(
            Update,
            close_on_world_press
                .after(super::UiInputSet)
                .before(gd_game_core::player::PlayerSet)
                .in_set(super::UiPlaySet),
        );
    }
}

/// The folder flag (`UIPlayerHud.menuFolderOpen`): the folder is open. Pressed by `hud::menu_buttons`.
#[derive(Resource, Default)]
pub struct MenuFolder {
    pub open: bool,
}

/// What the HUD shows of the folder (`HudValues::menu_folder`, `ui::view`): `open` = the background and the down
/// button; `buttons` = its window buttons (open, and neither the game menu nor the options window is up).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FolderView {
    pub open: bool,
    pub buttons: bool,
}

/// The folder's window buttons (orb HUD table fields), in the draw's order.
pub const BUTTONS: [&str; 7] = [
    "hudMiniMapButton",
    "hudAchievementsButton",
    "hudFactionButton",
    "hudQuestButton",
    "hudCharacterButton",
    "hudSkillsButton",
    "hudPartyButton",
];

/// The folder button and background fields.
pub const BUTTON: &str = "hudMenuFolderButton";
pub const BACKGROUND: &str = "hudMenuFolderBitmap";

/// The background's record: its bitmap and position.
fn background(ui: &Ui) -> Option<(String, f32, f32)> {
    let rec = ui.record(ui.hud_master())?.str(BACKGROUND)?.to_string();
    match ui.widget(&rec)? {
        Widget::Bitmap { bitmap, x, y } => Some((bitmap, x, y)),
        _ => None,
    }
}

/// The folder origin (screen px; module doc): the toolbar origin plus the engine's folder offset; `None` without a
/// folder (the classic table).
pub fn origin(ui: &mut Ui) -> Option<Vec2> {
    let to = super::hud::toolbar_origin(ui)?;
    let (bitmap, x, y) = background(ui)?;
    if to.x + ui.sc(x) >= 0.0 {
        return Some(to);
    }
    let button = super::hud::button_rect_at(ui, BUTTON, to)?;
    let (_, size) = ui.image(&bitmap)?;
    Some(Vec2::new(
        button.x + button.w * 0.5 - ui.sc(size.x as f32) * 0.53 - ui.sc(x),
        button.y - ui.sc(size.y as f32) - ui.sc(y),
    ))
}

/// The background's screen rect at the folder `origin`.
pub fn background_rect(ui: &mut Ui, origin: Vec2) -> Option<URect> {
    let (bitmap, x, y) = background(ui)?;
    let (_, size) = ui.image(&bitmap)?;
    Some(URect::new(
        origin.x + ui.sc(x),
        origin.y + ui.sc(y),
        ui.sc(size.x as f32),
        ui.sc(size.y as f32),
    ))
}

/// Draws the folder button at the toolbar origin `to` (down while open) and, while open, the background (the draw's
/// order, module doc).
pub fn draw_folder(ui: &mut Ui, view: FolderView, to: Vec2) {
    let Some(rec) = ui
        .record(ui.hud_master())
        .and_then(|m| m.str(BUTTON).map(str::to_owned))
    else {
        return;
    };
    if let Some(Widget::Button {
        up: Some(up),
        down,
        x,
        y,
        ..
    }) = ui.widget(&rec)
    {
        let bitmap = down.filter(|_| view.open).unwrap_or(up);
        ui.bitmap(&bitmap, x, y, to);
    }
    if view.open
        && let Some(o) = origin(ui)
        && let Some((bitmap, x, y)) = background(ui)
    {
        ui.bitmap(&bitmap, x, y, o);
    }
}

/// An open folder closes on a left / right press that no window or HUD part took (module doc). After the windows'
/// input (their capture), before the player's click-to-move.
fn close_on_world_press(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    layout: Res<HudLayout>,
    capture: Res<crate::state::inventory::UiCapture>,
    mut folder: ResMut<MenuFolder>,
) {
    if !folder.open
        || capture.0
        || !(buttons.just_pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Right))
    {
        return;
    }
    let Ok(win) = window.single() else { return };
    let Some(p) = super::cursor_px(win) else {
        return;
    };
    if !layout.covers(p) {
        folder.open = false;
    }
}
