use super::*;
use gd_ui::state::map::MapWindow;

/// The map key (`crate::ui::keys::MAP`, M by default) toggles the map window; Esc closes it. A riftgate opening its map
/// shows the world page.
pub(super) fn keys(
    input: Res<ButtonInput<KeyCode>>,
    bindings: Res<gd_ui_kit::keys::KeyBindings>,
    rift: Res<RiftgateMap>,
    mut win: ResMut<MapWindow>,
    mut was_open: Local<bool>,
    mut out: MessageWriter<RiftgateRequest>,
) {
    if rift.open && !*was_open {
        win.local = false;
    }
    *was_open = rift.open;
    if bindings.just_pressed(gd_ui_kit::keys::MAP, &input) {
        if win.open || rift.open {
            win.open = false;
            if rift.open {
                out.write(RiftgateRequest::Close);
            }
        } else {
            win.open = true;
        }
    }
    // Esc closes the map (and the riftgate map with it) through the shared dispatcher
    // (`ui::esc_dispatcher`).
    if win.local_shown(&rift) {
        let step = if input.just_pressed(KeyCode::Equal) || input.just_pressed(KeyCode::NumpadAdd) {
            -10.0
        } else if input.just_pressed(KeyCode::Minus) || input.just_pressed(KeyCode::NumpadSubtract)
        {
            10.0
        } else {
            0.0
        };
        if step != 0.0 {
            win.zoom_to = (win.zoom_to + step).clamp(40.0, 135.0);
        }
    }
}

/// Center Map (action 31, default Numpad 0) presses the open map window's centre button: the local map's pan goes
/// back to 0, the world map centres as on opening (`riftmap.rs`). `Grim Dawn.exe` `UIWorldMap` key action
/// `UIWorldMap::OnKeyAction` (`UIWindow::OnKeyAction`, reached while the window is open): action 31 presses
/// `UIWorldMap.centerButton` (the `centerButton`, whose press `UIWorldMap::OnButtonClick` handles for either page)
/// through the window's button manager.
pub(in crate::ui) fn center_map_key(
    keys: &ButtonInput<KeyCode>,
    bindings: &gd_ui_kit::keys::KeyBindings,
) -> bool {
    bindings.just_pressed(gd_ui_kit::keys::CENTER_MAP, keys)
}

/// Local map mouse (module doc): zoom / centre / switch / close buttons,
/// the legend (hover, click blink), the wheel's zoom, dragging the map;
/// the window takes the mouse.
#[allow(clippy::too_many_arguments)]
pub(super) fn clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    window: Query<&Window, With<PrimaryWindow>>,
    rift: Res<RiftgateMap>,
    mut win_state: ResMut<MapWindow>,
    mut capture: ResMut<UiCapture>,
    mut out: MessageWriter<RiftgateRequest>,
    (keys, bindings): (Res<ButtonInput<KeyCode>>, Res<gd_ui_kit::keys::KeyBindings>),
    pause: Res<gd_game_core::pause::GamePause>,
) {
    let Some(wr) = win_state
        .layout
        .window
        .filter(|_| win_state.local_shown(&rift))
    else {
        if win_state.local_shown(&rift) {
            wheel.clear();
        }
        win_state.drag = None;
        return;
    };
    if pause.menu {
        // Modal: the game menu / Options takes the wheel, hover, keys and presses.
        wheel.clear();
        win_state.drag = None;
        if win_state.legend_hover.is_some() {
            win_state.legend_hover = None;
        }
        if win_state.button_hover.is_some() {
            win_state.button_hover = None;
        }
        return;
    }
    if center_map_key(&keys, &bindings) {
        win_state.pan = Vec2::ZERO;
    }
    let Ok(win) = window.single() else { return };
    let Some(p) = gd_ui_kit::cursor_px(win) else {
        return;
    };
    // A drag keeps going while the button is held inside the window; it
    // ends on release or leaving it (`LocalMap::HandleMouseEvent`).
    if !buttons.pressed(MouseButton::Left) || !gd_ui_kit::contains(&wr, p) {
        win_state.drag = None;
    }
    let legend_hover = win_state
        .layout
        .legend
        .iter()
        .find(|(_, r)| (p - Vec2::new(r.x + r.w * 0.5, r.y + r.h * 0.5)).length() < r.w * 0.5)
        .map(|(i, _)| *i)
        .filter(|_| gd_ui_kit::contains(&wr, p));
    if win_state.legend_hover != legend_hover {
        win_state.legend_hover = legend_hover;
    }
    // `ButtonStatic` draws `bitmapNameInFocus` while hovered (as the world map).
    let over = |r: Option<URect>| r.is_some_and(|r| gd_ui_kit::contains(&r, p));
    let l = &win_state.layout;
    let button_hover = [l.close, l.center, l.switch]
        .iter()
        .position(|r| over(*r))
        .filter(|_| gd_ui_kit::contains(&wr, p));
    if win_state.button_hover != button_hover {
        win_state.button_hover = button_hover;
    }
    if !gd_ui_kit::contains(&wr, p) {
        return;
    }
    capture.0 = true;
    if let Some((start, pan0)) = win_state.drag {
        // Pixels to metres by the pane's width / height over zoom × 3.
        // PORT: the x sign is the port's screen-right axis (the engine
        // adds along the camera's X axis).
        let pane = win_state.layout.pane;
        let m = win_state.zoom * 3.0;
        let d = p - start;
        win_state.pan = Vec2::new(
            pan0.x - d.x * m / pane.w.max(1.0),
            pan0.y + d.y * m / pane.h.max(1.0),
        );
    }
    // The wheel zooms: −delta / 30 with 120 per notch (PORT: the delta per
    // notch), the zoom target clamped to 40..135.
    for w in wheel.read() {
        let notches = match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 20.0,
        };
        win_state.zoom_to = (win_state.zoom_to - notches * 120.0 / 30.0).clamp(40.0, 135.0);
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let l = win_state.layout.clone();
    let hit = |r: Option<URect>| r.is_some_and(|r| gd_ui_kit::contains(&r, p));
    if let Some(i) = legend_hover {
        // `LocalMap::OnLegendButton`: the type's icons blink for 1000 ms.
        win_state.blink[LEGEND_KIND[i] as usize] = 1000.0;
    } else if hit(l.close) {
        win_state.open = false;
        if rift.open {
            out.write(RiftgateRequest::Close);
        }
    } else if hit(l.zoom[0]) {
        win_state.zoom_to = (win_state.zoom_to - 30.0).clamp(40.0, 135.0);
    } else if hit(l.zoom[1]) {
        win_state.zoom_to = (win_state.zoom_to + 30.0).clamp(40.0, 135.0);
    } else if hit(l.center) {
        win_state.pan = Vec2::ZERO;
    } else if hit(l.switch) {
        win_state.switch(&rift);
    } else if gd_ui_kit::contains(&l.pane, p) {
        win_state.drag = Some((p, win_state.pan));
    }
}
