//! The Loot Filter window (`records/ui/lootfilter/lootfilter_mastertable.dbr`,
//! `lootfilterwindow.tpl`; options `gd_game::loot_filter`).
//!
//! GrimDawn.exe loader `sub_1401C86C0`: background, window title,
//! the four column titles, close and Defaults buttons, then the checkboxes
//! column by column (`gd_sim::items::loot_filter::OPTIONS`: option and label
//! tag; columns `lootFilterColumnIndent2..4` to the right of the checkbox
//! record's position), with `lootFilterSpacer` lines between groups. Each
//! checkbox shows its option (`bitmapNameDown` on, `bitmapNameUp` off) and a
//! click flips it ([`LootFilterRequest::Set`]); Defaults resets
//! ([`LootFilterRequest::Defaults`]). Closed by the close button or Esc.
//!
//! The HUD loot filter button (`hudLootFilterButton`,
//! `hud_buttonlootmodetoggle.dbr`, drawn after the compass button at the
//! toolbar anchor, GrimDawn.exe draw sub_14020A9D0) is a toggle in
//! the HUD's button group (constructor sub_140205BE0; group press
//! sub_14012B7E0): a click on a button that is up sets it down (its
//! `soundNameDown`) and tells the HUD (sub_1402129E0) to close the other
//! windows and show the window; a click on a down button sets it up
//! (`soundNameUp`) and hides the window. Key action 56 (O by default) presses
//! the same button (HUD key handler sub_140211AF0). While "Toggle Hide All
//! Items" is on (`crate::loot::ItemsHidden`) the button is disabled
//! (`bitmapNameDisabled`; sub_1402110D0 sets its disabled flag) and neither
//! the click nor O does anything. The button's down state is the window's
//! shown state (the HUD's window listener sub_1402132A0: the
//! window's showing sets the button's down flag +642, its hiding, by any
//! path, clears it). No hover (`bitmapNameInFocus`) bitmap, as the other HUD
//! buttons.
//!
//! Window closing, GrimDawn.exe: the press closes, through
//! `sub_140219AA0(hud, true)`, the map, character, vendor, faction vendor,
//! both `hudEnchanterWindow` windows (crafting, Inventor), skills, caravan,
//! factions, achievements, both shrine windows, devotion, transmuter
//! (Illusionist), altar, potions and item ascension windows (the
//! port's NPC windows other than the Illusionist's close with the character
//! window, `crate::trade`), not the quest journal, party or game menu; the
//! window's showing unlocks tutorial page 68 (its SetVisible
//! `sub_1401C99D0`). The same `sub_140219AA0` (or `sub_1402198E0`, which
//! calls it) runs when the character (`sub_140209BE0`), skills
//! (`sub_14021A660`), map, achievements, factions or game menu buttons open
//! their windows, so those close the loot filter window; the quest and party buttons use `sub_140219A40` /
//! `sub_140219C30`, which leave it open. The close button (window listener
//! `sub_1401C9B20`) and Esc hide it. PORT: the riftgate map
//! (`hudTeleportWindow`, no HUD member) is left open; the HUD child at +152 that the engine
//! also resets (vfunc +48) is not identified; opening the devotion window
//! (its opener not traced) does not close the loot filter window.
//!
//! Both directions go through the shared window policy
//! (`super::window::WindowPolicy` / `Windows`): the press opens with
//! `Windows::open(Window::LootFilter)` — its row closes the play field (the
//! old private `Closes::close_all` set) — and `closed_by_others` watches
//! `WindowPolicy::opened_closer`: a window whose row closes the loot filter,
//! opening, hides it.
//!
//! PORT: a checkbox row advances
//! `lootFilterRowSpacing` (36), a spacer `lootFilterRowSpacing2` (30)
//! before and after its line, the line at the spacer record's offset from
//! the first row; the label right after the box bitmap in the checkbox's
//! `style`.
//!
//! Defaults asks first (`tagLootFilterResetConfirm`, text tag from the
//! stock strings; the Yes / No box is the dialog manager's look, as the game
//! menu's confirmations, `gamemenu.rs`); Esc or No cancels, Yes sends
//! [`LootFilterRequest::Defaults`]. PORT: the window has no rollover records
//! in the stock data (the loader reads none), so the hover texts
//! (`tagLootFilterNNInfo` per checkbox, `tagLootFilterResetInfo` for
//! Defaults) use the generic UI-help rollover look (`style_default`,
//! `style_rollover_description`, placement as the other window widgets'
//! mode 2, `rollover.rs`); the engine function that picks them is not
//! decompiled. Checkbox 40 has no Info string in the stock tags: no hover.
use super::Ui;
use super::contains;
use super::window::{Rebuild, WindowPolicy, Windows};
use crate::data::GameData;
use crate::inventory::UiCapture;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::{Rect as URect, TextAlign, TextBox, WindowDef};
use gd_game::loot_filter::{LootFilter, LootFilterRequest};
use gd_sim::items::loot_filter::OPTIONS;

pub const RECORD: &str = "records/ui/lootfilter/lootfilter_mastertable.dbr";

pub struct LootFilterWindowPlugin;

impl Plugin for LootFilterWindowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LootFilterView>()
            .add_systems(
                Update,
                // Before the HUD's capture, which sets `UiCapture` over this very button. Not after
                // `WindowClicks`: `toggle` → `build` → this window's own `clicks` (in `WindowClicks`)
                // would close a cycle.
                hud_button
                    .in_set(crate::ui::UiInputSet)
                    .before(super::hud::capture)
                    .before(toggle),
            )
            .add_systems(
                Update,
                (closed_by_others, toggle)
                    .chain()
                    .before(build)
                    .in_set(crate::ui::UiPlaySet),
            );
        // Live: open, or still built (the closing frame despawns the root). Its hover tips are `rollover.rs`'s.
        super::window::add_window(app, build, clicks, |v: Res<LootFilterView>| {
            v.open || v.window.is_some()
        });
        // The Defaults box's hover / press look moves in place (no rebuild on a hover).
        app.add_systems(
            Update,
            confirm_live
                .run_if(|v: Res<LootFilterView>| v.confirm)
                .after(build)
                .in_set(crate::ui::UiPlaySet),
        );
    }
}

/// Whether the window is open and its built clickable parts (physical px).
#[derive(Resource, Default)]
pub struct LootFilterView {
    pub open: bool,
    window: Option<URect>,
    boxes: Vec<(URect, usize, &'static str)>,
    defaults: Option<URect>,
    close: Option<URect>,
    /// The Defaults confirmation is up (its Yes / No rects).
    confirm: bool,
    yes_no: Option<super::confirm::ConfirmRects>,
}

impl LootFilterView {
    /// The shared Esc dispatcher's cancel of the Defaults confirmation (`ui::esc_dispatcher`);
    /// the window itself closes through the focus stack. Whether it was up.
    pub fn escape_confirm(&mut self) -> bool {
        std::mem::replace(&mut self.confirm, false)
    }

    /// The `...Info` tag of the checkbox or button under `p` (physical px) while the window is open: shown by
    /// `rollover.rs` as a rollover (the `...Info` text in the default rollover style).
    pub(super) fn tip_tag(&self, p: Vec2) -> Option<String> {
        if !self.open || self.confirm {
            return None;
        }
        if let Some((_, _, tag)) = self.boxes.iter().find(|(r, _, _)| contains(r, p)) {
            Some(format!("{tag}Info"))
        } else if self.defaults.is_some_and(|r| contains(&r, p)) {
            Some("tagLootFilterResetInfo".to_string())
        } else {
            None
        }
    }
}

#[derive(Component)]
struct LootFilterRoot;

/// The tutorial page the window's showing unlocks (its SetVisible `sub_1401C99D0`:
/// `UnlockTutorialPage(68, show)`).
const TUTORIAL_PAGE: u32 = 68;

/// A press of the HUD loot filter button, by click or key (module doc).
fn press(
    windows: &mut Windows,
    tutorial: &mut MessageWriter<gd_game::tutorials::UnlockTutorial>,
    data: &GameData,
    sound: &mut MessageWriter<gd_game::visuals::PlaySound>,
) {
    let button = data
        .db
        .get(super::hud::MASTER)
        .and_then(|m| m.str("hudLootFilterButton").map(str::to_owned))
        .and_then(|b| data.db.get(&b));
    let field = if windows.loot.open {
        "soundNameUp"
    } else {
        "soundNameDown"
    };
    if let Some(pak) = button.as_ref().and_then(|b| b.str(field)) {
        sound.write(gd_game::visuals::PlaySound {
            pak: pak.to_owned(),
            at: None,
        });
    }
    if windows.loot.open {
        windows.loot.open = false;
    } else {
        windows.open(super::window::Window::LootFilter);
        tutorial.write(gd_game::tutorials::UnlockTutorial(TUTORIAL_PAGE));
    }
}

/// Closes the window when a window whose policy row excludes it opens
/// ([`WindowPolicy::opened_closer`], the shared watch of the module doc).
fn closed_by_others(
    mut windows: Windows,
    policy: Res<WindowPolicy>,
    mut was: Local<[bool; super::window::Window::COUNT]>,
    mut fresh: gd_game::session::NewSession,
) {
    // A new session starts from nothing open, not the last session's windows.
    if fresh.begun() {
        *was = Default::default();
    }
    let now = windows.snapshot();
    if windows.loot.open && policy.opened_closer(super::window::Window::LootFilter, &was, &now) {
        windows.loot.open = false;
    }
    *was = now;
}

/// What the HUD loot filter button shows (module doc).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HudButton {
    #[default]
    Up,
    Down,
    Disabled,
}

impl HudButton {
    pub fn of(view: &LootFilterView, hidden: &crate::loot::ItemsHidden) -> Self {
        if hidden.0 {
            HudButton::Disabled
        } else if view.open {
            HudButton::Down
        } else {
            HudButton::Up
        }
    }
}

/// A left click on the HUD loot filter button presses it (module doc). The button's rect is looked up when the
/// HUD's layout inputs change (window size, UI scale, HUD layout), not per frame.
#[allow(clippy::too_many_arguments)]
fn hud_button(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    mut capture: ResMut<UiCapture>,
    mut assets: crate::ui::UiAssets,
    (mut windows, hidden, mut tutorial): (
        Windows,
        Res<crate::loot::ItemsHidden>,
        MessageWriter<gd_game::tutorials::UnlockTutorial>,
    ),
    mut sound: MessageWriter<gd_game::visuals::PlaySound>,
    mut rect: Local<(Rebuild<(UVec2, u32, u32, bool)>, Option<URect>)>,
) {
    let Ok(win) = window.single() else { return };
    let Some(p) = super::cursor_px(&assets.hooks, win) else {
        return;
    };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    if size.x == 0 || size.y == 0 || capture.0 {
        return;
    }
    let s = assets.scale(size);
    let inv = 1.0 / win.scale_factor();
    if rect
        .0
        .changed((size, s.to_bits(), inv.to_bits(), assets.classic_hud()))
    {
        let mut probe = assets.ui(
            &mut commands,
            &data,
            Entity::PLACEHOLDER,
            s,
            inv,
            size.as_vec2(),
        );
        rect.1 = super::rollover::button_rect(&mut probe, "hudLootFilterButton");
        // Nothing found (a record or bitmap is missing): asked again the next frame.
        if rect.1.is_none() {
            rect.0.reset();
        }
    }
    let Some(r) = rect.1 else {
        return;
    };
    if !contains(&r, p) {
        return;
    }
    capture.0 = true;
    if super::widgets::button_pressed(&buttons) && !hidden.0 {
        press(&mut windows, &mut tutorial, &data, &mut sound);
    }
}

/// The loot filter key (`super::keys::LOOT_FILTER`, O by default) presses the HUD button (`GD_UI_OPEN` opens the window once).
#[allow(clippy::too_many_arguments)]
fn toggle(
    hooks: Res<crate::hooks::Hooks>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<super::keys::KeyBindings>,
    mut windows: Windows,
    mut tested: Local<bool>,
    (hidden, mut tutorial, data): (
        Res<crate::loot::ItemsHidden>,
        MessageWriter<gd_game::tutorials::UnlockTutorial>,
        Res<GameData>,
    ),
    mut sound: MessageWriter<gd_game::visuals::PlaySound>,
) {
    if !*tested {
        *tested = true;
        if hooks.ui.open.as_deref() == Some("lootfilter") {
            windows.loot.open = true;
        }
    }
    if bindings.just_pressed(super::keys::LOOT_FILTER, &keys) && !hidden.0 {
        press(&mut windows, &mut tutorial, &data, &mut sound);
    }
    // Esc cancels the confirmation, else closes the window, through the shared dispatcher
    // (`ui::esc_dispatcher`).
    if !windows.loot.open {
        windows.loot.confirm = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    filter: Option<Res<LootFilter>>,
    mut view: ResMut<LootFilterView>,
    roots: Query<Entity, With<LootFilterRoot>>,
    mut fresh: gd_game::session::NewSession,
    mut built: Local<Rebuild<(bool, UVec2, Option<[bool; 42]>, bool, u32)>>,
    mut assets: crate::ui::UiAssets,
) {
    if fresh.begun() {
        built.reset();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let sig = (
        view.open,
        size,
        filter.as_ref().map(|f| f.options),
        view.confirm,
        assets.scale(size).to_bits(),
    );
    if size.x == 0 || !built.changed(sig) {
        return;
    }
    super::window::despawn_all(&mut commands, &roots);
    view.window = None;
    view.boxes.clear();
    view.defaults = None;
    view.close = None;
    view.yes_no = None;
    let (true, Some(filter), Some(rec)) = (view.open, filter, data.db.get(RECORD)) else {
        return;
    };
    let s = assets.scale(size);
    let root = crate::ui::spawn_root(&mut commands, LootFilterRoot, super::window::Z_MODAL);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let field = |k: &str| rec.str(k).unwrap_or("").to_string();
    let w = WindowDef::from_record(rec).screen_rect(ui.screen.x, ui.screen.y, s);
    let o = Vec2::new(w.x, w.y);
    if let Some(bg) = ui.record(&field("lootFilterBitmap"))
        && let Some(path) = bg.str("bitmapName")
    {
        ui.bitmap(
            path,
            bg.f32("bitmapPositionX").unwrap_or(0.0),
            bg.f32("bitmapPositionY").unwrap_or(0.0),
            o,
        );
    }
    for t in [
        "lootFilterWindowTitle",
        "lootFilterTitle01",
        "lootFilterTitle02",
        "lootFilterTitle03",
        "lootFilterTitle04",
    ] {
        if let Some(r) = ui.record(&field(t)) {
            let tb = TextBox::from_record(&r);
            let text = tb
                .tag
                .as_deref()
                .map(|t| ui.fonts.tag(t).to_string())
                .unwrap_or_default();
            ui.text(&tb, o, &text);
        }
    }
    let button =
        |ui: &mut Ui, r: &gd_formats::arz::Record, key: &str, x: f32, y: f32| -> Option<URect> {
            let (img, sz) = r.str(key).map(str::to_owned).and_then(|p| ui.image(&p))?;
            let br = URect::new(
                o.x + ui.sc(x),
                o.y + ui.sc(y),
                ui.sc(sz.x as f32),
                ui.sc(sz.y as f32),
            );
            ui.quad(img, br, None, Color::WHITE);
            Some(br)
        };
    if let Some(b) = ui.record(&field("lootFilterCloseButton")) {
        view.close = button(
            &mut ui,
            &b,
            "bitmapNameUp",
            b.f32("bitmapPositionX").unwrap_or(0.0),
            b.f32("bitmapPositionY").unwrap_or(0.0),
        );
        if let Some(r) = view.close {
            ui.record_button_sound(r, &b);
        }
    }
    if let Some(b) = ui.record(&field("lootFilterResetButton")) {
        let r = button(
            &mut ui,
            &b,
            "bitmapNameUp",
            b.f32("bitmapPositionX").unwrap_or(0.0),
            b.f32("bitmapPositionY").unwrap_or(0.0),
        );
        if let (Some(r), Some(style), Some(t)) = (r, b.str("style"), b.str("textTag")) {
            let text = ui.fonts.tag(t).to_string();
            super::trade::label(&mut ui, r, style, TextAlign::Center, &text);
        }
        if let Some(r) = r {
            ui.record_button_sound(r, &b);
        }
        view.defaults = r;
    }
    let (Some(cb), spacer) = (
        ui.record(&field("lootFilterCheckbox")),
        ui.record(&field("lootFilterSpacer")),
    ) else {
        return;
    };
    let (x0, y0) = (
        cb.f32("bitmapPositionX").unwrap_or(30.0),
        cb.f32("bitmapPositionY").unwrap_or(125.0),
    );
    let (row, row2) = (
        rec.f32("lootFilterRowSpacing").unwrap_or(36.0),
        rec.f32("lootFilterRowSpacing2").unwrap_or(30.0),
    );
    let label_style = cb
        .str("style")
        .or(cb.str("textStyleName"))
        .map(str::to_owned);
    for (col, entries) in OPTIONS.iter().enumerate() {
        let indent = if col == 0 {
            0.0
        } else {
            rec.f32(&format!("lootFilterColumnIndent{}", col + 1))
                .unwrap_or(244.0 * col as f32)
        };
        let x = x0 + indent;
        let mut y = y0;
        for e in entries.iter() {
            match e {
                Some((opt, tag)) => {
                    // The Ascended box needs the third expansion (always here).
                    let on = filter.options[*opt];
                    let Some(br) = button(
                        &mut ui,
                        &cb,
                        if on { "bitmapNameDown" } else { "bitmapNameUp" },
                        x,
                        y,
                    ) else {
                        continue;
                    };
                    ui.record_button_sound(br, &cb);
                    if let Some(style) = &label_style {
                        let text = ui.fonts.tag(tag).to_string();
                        let lr = URect::new(br.x + br.w, br.y, ui.sc(200.0), br.h);
                        super::trade::label(&mut ui, lr, style, TextAlign::Start, &text);
                    }
                    view.boxes.push((br, *opt, tag));
                    y += row;
                }
                None => {
                    y += row2 - row;
                    if let Some(sp) = &spacer
                        && let Some(path) = sp.str("bitmapName")
                    {
                        let (sx, sy) = (
                            sp.f32("bitmapPositionX").unwrap_or(31.0),
                            sp.f32("bitmapPositionY").unwrap_or(137.0),
                        );
                        ui.bitmap(path, sx + indent, sy + (y - y0), o);
                    }
                    y += row2;
                }
            }
        }
    }
    view.window = Some(w);
    // The window takes every click over it: nothing behind it plays a button sound there.
    ui.sound_blocker(w);
    if view.confirm {
        // The prompt box is the dialog manager's: Toggle UI leaves it shown (`hud::toggle_ui`).
        // Its hover / press look moves in place (`confirm_live`), so the look is not in the signature.
        ui.root = crate::ui::spawn_root(
            ui.commands,
            (LootFilterRoot, super::hud::KeepsShown),
            super::window::Z_MODAL + 1,
        );
        view.yes_no =
            super::confirm::confirm_box(&mut ui, "tagLootFilterResetConfirm", (None, false));
        // The box is modal: it takes every click on screen.
        ui.sound_blocker(URect::new(0.0, 0.0, ui.screen.x, ui.screen.y));
    }
}

/// The Defaults box's hover / press look in place ([`super::confirm::apply_look`]): the box is
/// built in its base look, so a hover never rebuilds the window.
fn confirm_live(
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<crate::hooks::Hooks>,
    view: Res<LootFilterView>,
    mouse: Res<ButtonInput<MouseButton>>,
    roots: Query<Entity, (With<LootFilterRoot>, With<super::hud::KeepsShown>)>,
    mut looks: Query<(&super::confirm::ConfirmLook, &mut ImageNode)>,
) {
    let (Ok(win), Some(b), Ok(root)) = (window.single(), view.yes_no, roots.single()) else {
        return;
    };
    let look = b.look(
        super::cursor_px(&hooks, win),
        mouse.pressed(MouseButton::Left),
    );
    super::confirm::apply_look(root, look, &mut looks);
}

/// Clicks on the window (it takes every click over it).
#[allow(clippy::too_many_arguments)]
fn clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<crate::hooks::Hooks>,
    mut view: ResMut<LootFilterView>,
    filter: Option<Res<LootFilter>>,
    mut capture: ResMut<UiCapture>,
    mut requests: MessageWriter<LootFilterRequest>,
    data: Res<GameData>,
    mut sound: MessageWriter<gd_game::visuals::PlaySound>,
) {
    let (Ok(win), Some(filter)) = (window.single(), filter) else {
        return;
    };
    let Some(p) = super::cursor_px(&hooks, win) else {
        return;
    };
    // The Defaults box takes every click on screen while up (it is modal), not only those over the window.
    if view.confirm {
        capture.0 = true;
        if !super::widgets::button_pressed(&buttons) {
            return;
        }
        if let Some(b) = view.yes_no {
            if contains(&b.yes, p) {
                requests.write(LootFilterRequest::Defaults);
                view.confirm = false;
            } else if contains(&b.no, p) {
                view.confirm = false;
            } else if b.close.is_some_and(|r| contains(&r, p)) {
                view.confirm = false;
                if let Some(pak) = super::confirm::close_sound(&data) {
                    sound.write(gd_game::visuals::PlaySound { pak, at: None });
                }
            }
        }
        return;
    }
    if !view.window.is_some_and(|w| contains(&w, p)) {
        return;
    }
    capture.0 = true;
    if !super::widgets::button_pressed(&buttons) {
        return;
    }
    if view.close.is_some_and(|r| contains(&r, p)) {
        view.open = false;
    } else if view.defaults.is_some_and(|r| contains(&r, p)) {
        view.confirm = true;
    } else if let Some((_, opt, _)) = view.boxes.iter().find(|(r, _, _)| contains(r, p)) {
        requests.write(LootFilterRequest::Set(*opt, !filter.options[*opt]));
    }
}
