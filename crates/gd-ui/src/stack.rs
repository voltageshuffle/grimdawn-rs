//! The stack quantity window (`hudStackWindow` of both HUD master tables =
//! `records/ui/stackwindow/stack_table.dbr`, GrimDawn.exe `StackWindow`,
//! loader `StackWindow::Load`). The split it
//! asks for is `gd_game::stack` ([`gd_game::stack::SplitRequest`]).
//!
//! Opening: a left click with Ctrl held on a stack of more than one
//! in the bags (mouse handler `BagWidget_HandleMouseEvent`; Ctrl on a single item does
//! nothing) or, Shift not held, in the stash (`StashBag::HandleMouseEvent`) opens it on that
//! stack (`UIPlayerHud::OpenStackWindow`: the item, then the window at the item's right edge
//! and top); every bag / stash click hides it first. Nothing else opens it: a
//! merchant buys and sells whole items (right click sells the stack) and a
//! dropped stack drops whole.
//!
//! The window: shown with the text "1" selected (`StackWindow::Show`:
//! amount 1, the edit box focused); its draw (`StackWindow::Draw`) puts
//! `stackBackground` at the position with y kept within 5 px of the screen's
//! top and bottom, then the edit box, `decreaseButton`, `increaseButton`,
//! `okButton` and `cancelButton` at their `bitmapPosition`s. The amount runs
//! 1 .. [`gd_game_core::stack::split_max`]: − stops at 1, + at the maximum, a
//! typed number above it becomes it, an empty / 0 text is 0 and disables OK
//! (`StackWindow::ValidateAmount`; a 0 also clears the text); − is disabled at ≤ 1, + at the
//! maximum (a disabled `ButtonStatic` draws its up bitmap in 0.6 grey,
//! `ButtonStatic::Render`). The edit box (`stack_editbox.dbr`, `NumericOnly`, 4
//! characters, `StackWindow::Load`) is the engine edit box (`gd_sim::edit`: caret,
//! selection, Delete, key repeat, Ctrl+A/C/V with the vendor search box's
//! clipboard, `trade::Clipboard`) taking digits only; Enter is OK, Esc
//! hides the window (`StackWindow::OnEscape`). OK (`StackWindow::OnOk`) sends the split and
//! hides the window; Cancel hides it (`StackWindow::OnCancel`). PORT: the 4-character
//! limit cuts the text's end instead of refusing the key; the buttons have no down bitmap. The edit box ticks on
//! real time (the caret and key repeat keep going while the game is paused).
use super::contains;
use super::widgets::{Label, static_button};
use super::window::Rebuild;
use crate::state::inventory::UiCapture;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_formats::ui::Rect as URect;
use gd_game_core::data::GameData;
use gd_game_core::inventory::Inventory;
use gd_game_core::inventory::ItemLoc;

/// `hudStackWindow` (the same in both HUD master tables).
pub const TABLE: &str = "records/ui/stackwindow/stack_table.dbr";

pub struct StackUiPlugin;

impl Plugin for StackUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StackWindow>().add_systems(
            PreUpdate,
            keys.after(bevy::input::InputSystems)
                .before(crate::gamemenu::GameMenuInputSet)
                .before(crate::state::options::OptionsInputSet)
                .run_if(|w: Res<StackWindow>| w.open.is_some()),
        );
        #[cfg(feature = "dev")]
        app.add_systems(
            Update,
            hook.in_set(gd_ui_kit::UiInputSet)
                .before(clicks)
                .run_if(|h: Res<gd_game_core::hooks::Hooks>| h.items.stack.is_some()),
        );
        // After the stash / merchant clicks (`TradeInputSet`), which leave the window's own clicks to it.
        super::window::add_window(
            app,
            build,
            clicks.after(crate::state::trade::TradeInputSet),
            |w: Res<StackWindow>, roots: Query<(), With<StackRoot>>| {
                w.open.is_some() || !roots.is_empty()
            },
        );
    }
}

/// The open window: the stack it splits, where it is and what the edit box holds.
#[derive(Clone, Debug)]
pub struct StackOpen {
    pub from: ItemLoc,
    /// The clicked item's top-right corner (physical px).
    pub at: Vec2,
    pub text: String,
    /// The edit box's caret, selection and key repeat.
    pub edit: gd_sim::edit::EditState,
    pub amount: u32,
    pub max: u32,
}

impl PartialEq for StackOpen {
    /// What the window shows (the key repeat timer is not drawn).
    fn eq(&self, o: &Self) -> bool {
        (self.from, self.at, &self.text, self.amount, self.max)
            == (o.from, o.at, &o.text, o.amount, o.max)
            && self.edit.caret == o.edit.caret
            && self.edit.selection() == o.edit.selection()
    }
}

/// The window's rects as drawn (physical px).
#[derive(Clone, Copy, Debug, Default)]
pub struct StackRects {
    pub window: URect,
    pub decrease: Option<URect>,
    pub increase: Option<URect>,
    pub ok: Option<URect>,
    pub cancel: Option<URect>,
}

/// The stack window's own state (the UI's, like the vendor search text).
#[derive(Resource, Default)]
pub struct StackWindow {
    pub open: Option<StackOpen>,
    rects: Option<StackRects>,
}

impl StackWindow {
    /// Opens on the stack at `from` of `stack` items; `at` is its top-right corner.
    pub fn open(&mut self, from: ItemLoc, at: Vec2, stack: u32, data: &GameData) {
        let max = gd_game_core::stack::split_max(stack, gd_game_core::stack::engine_max(&data.db));
        // "1" selected, the caret after it (`StackWindow::Show`).
        let mut edit = gd_sim::edit::EditState::default();
        edit.caret = 1;
        edit.sel = Some((0, 1));
        self.open = Some(StackOpen {
            from,
            at,
            text: "1".into(),
            edit,
            amount: 1.min(max),
            max,
        });
        self.rects = None;
    }

    pub fn hide(&mut self) {
        self.open = None;
    }

    /// Whether the shown window covers `p` (its clicks are its own).
    pub fn covers(&self, p: Vec2) -> bool {
        self.open.is_some() && self.rects.is_some_and(|r| contains(&r.window, p))
    }
}

impl StackOpen {
    /// The text after typing (`StackWindow::ValidateAmount`): its amount, rewritten when clamped, cleared at 0.
    fn validate(&mut self) {
        let a = gd_game_core::stack::typed_amount(&self.text, self.max);
        self.amount = a;
        if a == 0 {
            self.text.clear();
            self.edit.to_end(&self.text);
        } else if a.to_string() != self.text {
            self.text = a.to_string();
            self.edit.to_end(&self.text);
        }
    }

    fn set(&mut self, amount: u32) {
        self.amount = amount;
        self.text = amount.to_string();
        self.edit.to_end(&self.text);
    }
}

/// The stack under `from` and its count (bags, or the stash page).
fn stack_at(
    inv: &Inventory,
    stash: Option<&gd_game_core::stash::Stash>,
    from: ItemLoc,
) -> Option<u32> {
    inv.item_in(from, stash).map(gd_game_core::stack::count)
}

#[derive(Component)]
struct StackRoot;

/// A part of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Decrease,
    Increase,
    Ok,
    Cancel,
}

fn part_at(r: &StackRects, p: Vec2) -> Option<Part> {
    [
        (r.decrease, Part::Decrease),
        (r.increase, Part::Increase),
        (r.ok, Part::Ok),
        (r.cancel, Part::Cancel),
    ]
    .into_iter()
    .find(|(rect, _)| rect.is_some_and(|rect| contains(&rect, p)))
    .map(|(_, part)| part)
}

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    data: Res<GameData>,
    mut stack: ResMut<StackWindow>,
    roots: Query<Entity, With<StackRoot>>,
    mut fresh: gd_game_core::session::NewSession,
    mut built: Local<Rebuild<(Option<StackOpen>, UVec2, Option<Part>)>>,
    mut assets: gd_ui_kit::UiAssets,
) {
    if fresh.begun() {
        built.reset();
    }
    let Ok(win) = window.single() else { return };
    let size = UVec2::new(win.physical_width(), win.physical_height());
    let hovered = super::cursor_px(win)
        .zip(stack.rects)
        .and_then(|(p, r)| part_at(&r, p));
    if size.x == 0
        || !built.changed_by(
            |b| b.0 == stack.open && b.1 == size && b.2 == hovered,
            || (stack.open.clone(), size, hovered),
        )
    {
        return;
    }
    super::window::despawn_all(&mut commands, &roots);
    let Some(open) = stack.open.clone() else {
        stack.rects = None;
        return;
    };
    let Some(table) = data.db.get(TABLE) else {
        return;
    };
    let s = assets.scale(size);
    let root = gd_ui_kit::spawn_root(&mut commands, StackRoot, super::window::Z_MODAL);
    let mut ui = assets.ui(
        &mut commands,
        &data,
        root,
        s,
        1.0 / win.scale_factor(),
        Vec2::new(size.x as f32, size.y as f32),
    );
    let field = |k: &str| table.str(k).and_then(|p| data.db.get(p));
    let Some(bg) = field("stackBackground") else {
        return;
    };
    let Some((bg_img, bg_size)) = bg
        .str("bitmapName")
        .map(str::to_owned)
        .and_then(|p| ui.image(&p))
    else {
        return;
    };
    let (w, h) = (ui.sc(bg_size.x as f32), ui.sc(bg_size.y as f32));
    let o = Vec2::new(
        open.at.x.round(),
        open.at.y.max(5.0).min(ui.screen.y - (h + 5.0)).round(),
    );
    let window = URect::new(
        o.x + ui.sc(bg.f32("bitmapPositionX").unwrap_or(0.0)),
        o.y + ui.sc(bg.f32("bitmapPositionY").unwrap_or(0.0)),
        w,
        h,
    );
    ui.quad(bg_img, window, None, Color::WHITE);
    edit_box(&mut ui, field("inputBox"), &open, o);
    let mut rects = StackRects {
        window,
        ..Default::default()
    };
    // The +/- buttons: `ButtonStatic`, disabled = the up bitmap in 0.6 grey.
    for (k, part, enabled) in [
        ("decreaseButton", Part::Decrease, open.amount > 1),
        ("increaseButton", Part::Increase, open.amount < open.max),
    ] {
        let Some(b) = field(k) else { continue };
        let key = if enabled && hovered == Some(part) && b.str("bitmapNameInFocus").is_some() {
            "bitmapNameInFocus"
        } else {
            "bitmapNameUp"
        };
        let tint = if enabled {
            Color::WHITE
        } else {
            Color::srgb(0.6, 0.6, 0.6)
        };
        if let Some((img, sz)) = b.str(key).map(str::to_owned).and_then(|p| ui.image(&p)) {
            let r = URect::new(
                o.x + ui.sc(b.f32("bitmapPositionX").unwrap_or(0.0)),
                o.y + ui.sc(b.f32("bitmapPositionY").unwrap_or(0.0)),
                ui.sc(sz.x as f32),
                ui.sc(sz.y as f32),
            );
            ui.quad(img, r, None, tint);
            let slot = if part == Part::Decrease {
                &mut rects.decrease
            } else {
                &mut rects.increase
            };
            *slot = enabled.then_some(r);
        }
    }
    if let Some(b) = field("okButton") {
        let on = open.amount > 0;
        let r = static_button(&mut ui, b, o, on, hovered == Some(Part::Ok), Label::Record);
        rects.ok = r.filter(|_| on);
    }
    if let Some(b) = field("cancelButton") {
        rects.cancel = static_button(
            &mut ui,
            b,
            o,
            true,
            hovered == Some(Part::Cancel),
            Label::Record,
        );
    }
    stack.rects = Some(rects);
}

/// The edit box: the text right-aligned and vertically centred in its `Style`, the selection and the caret.
fn edit_box(ui: &mut super::Ui, rec: Option<&gd_formats::arz::Record>, open: &StackOpen, o: Vec2) {
    let Some(b) = rec else { return };
    let Some(style) = b.str("Style") else { return };
    let f = |k: &str| b.f32(k).unwrap_or(0.0);
    let r = URect::new(
        o.x + ui.sc(f("LocationX")),
        o.y + ui.sc(f("LocationY")),
        ui.sc(f("Width")),
        ui.sc(f("Height")),
    );
    let tw = ui.text_width(style, &open.text);
    let lh = ui.line_height(style);
    let x = (r.x + r.w - tw).round();
    let y = (r.y + (r.h - lh) * 0.5).round();
    let pre = |ui: &mut super::Ui, n: usize| {
        ui.text_width(style, &open.text.chars().take(n).collect::<String>())
    };
    // PORT: the selection highlight colour (the vendor search box's).
    if let Some((a, b)) = open.edit.selection().filter(|(a, b)| a < b) {
        let (xa, xb) = (x + pre(ui, a), x + pre(ui, b));
        ui.solid(
            URect::new(xa, r.y, xb - xa, r.h),
            Color::srgba(0.4, 0.5, 0.9, 0.5),
        );
    }
    ui.text_line(style, Vec2::new(x, y), &open.text, 1.0);
    let cx = x + pre(ui, open.edit.caret.min(open.text.chars().count()));
    let e = ui.solid(URect::new(cx, r.y, ui.sc(1.0).max(1.0), r.h), Color::WHITE);
    ui.commands.entity(e).insert(super::trade::SearchCaret);
}

/// Presses OK: the split request, then the window hides.
fn press_ok(stack: &mut StackWindow, split: &mut MessageWriter<gd_game::stack::SplitRequest>) {
    if let Some(o) = stack.open.take()
        && o.amount > 0
    {
        split.write(gd_game::stack::SplitRequest {
            from: o.from,
            amount: o.amount,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut stack: ResMut<StackWindow>,
    inv: Option<Res<Inventory>>,
    stash: Option<Res<gd_game_core::stash::Stash>>,
    panels: Res<super::UiPanels>,
    trade: Res<crate::state::trade::Trade>,
    mut capture: ResMut<UiCapture>,
    mut split: MessageWriter<gd_game::stack::SplitRequest>,
    data: Res<GameData>,
    mut sound: MessageWriter<gd_game_core::visuals::PlaySound>,
) {
    let Some(open) = stack.open.as_mut() else {
        return;
    };
    // The stack's window went away, or the stack did: the window goes too.
    let shown = match open.from {
        ItemLoc::Stash { .. } => {
            matches!(trade.open, Some(gd_game::trade::TradeOpen::Stash { .. }))
        }
        _ => panels.character,
    };
    let Some(n) = inv
        .as_deref()
        .filter(|_| shown)
        .and_then(|i| stack_at(i, stash.as_deref(), open.from))
        .filter(|n| *n > 1)
    else {
        stack.hide();
        return;
    };
    let max = gd_game_core::stack::split_max(n, gd_game_core::stack::engine_max(&data.db));
    if max != open.max {
        open.max = max;
        open.validate();
    }
    let (Ok(win), Some(rects)) = (window.single(), stack.rects) else {
        return;
    };
    let Some(p) = super::cursor_px(win) else {
        return;
    };
    if !contains(&rects.window, p) {
        return;
    }
    capture.0 = true;
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(part) = part_at(&rects, p) else {
        return;
    };
    let rec = match part {
        Part::Decrease => "decreaseButton",
        Part::Increase => "increaseButton",
        Part::Ok => "okButton",
        Part::Cancel => "cancelButton",
    };
    if let Some(pak) = data
        .db
        .get(TABLE)
        .and_then(|t| t.str(rec).and_then(|r| data.db.get(r)))
        .and_then(|b| b.str("soundNameDown").map(str::to_owned))
    {
        sound.write(gd_game_core::visuals::PlaySound { pak, at: None });
    }
    let Some(open) = stack.open.as_mut() else {
        return;
    };
    match part {
        Part::Decrease => {
            let a = gd_game_core::stack::decrease(open.amount);
            open.set(a);
        }
        Part::Increase => {
            let a = gd_game_core::stack::increase(open.amount, open.max);
            open.set(a);
        }
        Part::Ok => press_ok(&mut stack, &mut split),
        Part::Cancel => stack.hide(),
    }
}

/// The focused edit box takes the keyboard; the game's keys see nothing while the window is up (Escape stays
/// for `ui::esc_dispatcher`). The game menu or Options hides it.
/// PORT: the engine's modal menu leaves the window up, untouched.
#[allow(clippy::too_many_arguments)]
fn keys(
    mut stack: ResMut<StackWindow>,
    mut typed: MessageReader<KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut split: MessageWriter<gd_game::stack::SplitRequest>,
    mut clipboard: ResMut<crate::state::trade::Clipboard>,
    time: Res<Time<Real>>,
    mut mods: Local<(bool, bool)>,
    pause: Res<gd_game_core::pause::GamePause>,
) {
    use bevy::input::keyboard::KeyCode as K;
    use gd_sim::edit::EditKey;
    if pause.menu {
        stack.hide();
        *mods = Default::default();
        typed.clear();
        return;
    }
    let mut ok = false;
    if let Some(open) = stack.open.as_mut() {
        let before = open.text.clone();
        // `NumericOnly`, 4 characters.
        let fits = |t: &str| t.chars().count() <= 4;
        let digits = |t: &str| t.chars().all(|c| c.is_ascii_digit());
        open.edit
            .tick(&mut open.text, time.delta_secs() * 1000.0, &fits);
        for ev in typed.read() {
            if !ev.state.is_pressed() {
                if matches!(ev.key_code, K::ShiftLeft | K::ShiftRight) {
                    mods.0 = false;
                }
                if matches!(ev.key_code, K::ControlLeft | K::ControlRight) {
                    mods.1 = false;
                }
                open.edit.release();
                continue;
            }
            match ev.key_code {
                K::ShiftLeft | K::ShiftRight => mods.0 = true,
                K::ControlLeft | K::ControlRight => mods.1 = true,
                _ => {}
            }
            if ev.repeat {
                continue;
            }
            let (shift, ctrl) = *mods;
            let key = match (&ev.logical_key, ev.key_code) {
                (Key::Character(c), _) if digits(c) => EditKey::Text(c.to_string()),
                (Key::Enter, _) => {
                    ok = true;
                    continue;
                }
                // Esc hides the window through the shared dispatcher (`ui::esc_dispatcher`).
                _ => {
                    let Some(k) = super::widgets::edit_key(&ev.logical_key, ev.key_code, ctrl)
                    else {
                        continue;
                    };
                    // The amount box pastes digits only.
                    if k == EditKey::Paste && !digits(&clipboard.0) {
                        continue;
                    }
                    k
                }
            };
            if let Some(c) = open
                .edit
                .press(&mut open.text, key, shift, &clipboard.0, &fits)
            {
                clipboard.0 = c;
            }
        }
        if open.text != before {
            open.validate();
        }
    } else {
        typed.clear();
    }
    if ok {
        press_ok(&mut stack, &mut split);
    }
    super::keys::reset_but_escape(&mut keys);
}

#[allow(clippy::too_many_arguments)]
#[cfg(feature = "dev")]
fn hook(
    hooks: Res<gd_game_core::hooks::Hooks>,
    time: Res<Time>,
    mut stack: ResMut<StackWindow>,
    inv: Option<Res<Inventory>>,
    stash: Option<Res<gd_game_core::stash::Stash>>,
    (layout, trade_layout, trade): (
        Res<crate::state::inventory::InvLayout>,
        Res<crate::state::trade::TradeLayout>,
        Res<crate::state::trade::Trade>,
    ),
    data: Res<GameData>,
    mut state: Local<(bool, Option<f32>)>,
    mut split: MessageWriter<gd_game::stack::SplitRequest>,
) {
    let Some(h) = hooks.items.stack else { return };
    let now = time.elapsed_secs();
    if let Some(at) = state.1
        && now >= at
    {
        state.1 = None;
        info!("GD_STACK: OK");
        press_ok(&mut stack, &mut split);
        return;
    }
    if state.0 || now < h.secs {
        return;
    }
    let Some(inv) = inv else { return };
    let found =
        if let (Some(gd_game::trade::TradeOpen::Stash { .. }), Some(stash), Some((gr, cell))) =
            (&trade.open, stash.as_deref(), trade_layout.grid)
        {
            let page = trade
                .tab
                .min(stash.view(trade.transfer).len().saturating_sub(1));
            stash.view(trade.transfer).get(page).and_then(|pg| {
                pg.items
                    .iter()
                    .find(|q| gd_game_core::stack::count(&q.item) > 1)
                    .map(|q| {
                        (
                            ItemLoc::Stash {
                                transfer: trade.transfer,
                                page,
                                x: q.x,
                                y: q.y,
                            },
                            Vec2::new(
                                gr.x + (q.x + q.size.w) as f32 * cell,
                                gr.y + q.y as f32 * cell,
                            ),
                            gd_game_core::stack::count(&q.item),
                        )
                    })
            })
        } else {
            layout
                .grids
                .iter()
                .find(|(b, ..)| *b == 0)
                .and_then(|(_, r, cell)| {
                    inv.bags[0]
                        .items
                        .iter()
                        .find(|q| gd_game_core::stack::count(&q.item) > 1)
                        .map(|q| {
                            (
                                ItemLoc::Bag {
                                    bag: 0,
                                    x: q.x,
                                    y: q.y,
                                },
                                Vec2::new(
                                    r.x + (q.x + q.size.w) as f32 * cell,
                                    r.y + q.y as f32 * cell,
                                ),
                                gd_game_core::stack::count(&q.item),
                            )
                        })
                })
        };
    let Some((from, at, n)) = found else { return };
    state.0 = true;
    stack.open(from, at, n, &data);
    if let Some(o) = stack.open.as_mut() {
        o.text = h.amount.to_string();
        o.edit.to_end(&o.text);
        o.validate();
        info!("GD_STACK: {from:?} of {n}, amount {}", o.amount);
    }
    if h.ok {
        state.1 = Some(now + 1.0);
    }
}
