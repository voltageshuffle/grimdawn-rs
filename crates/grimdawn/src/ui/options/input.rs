use super::*;

/// Clicks, drags, the bindings' wheel; opening the window starts a draft of the session's options.
/// Esc: in game the shared dispatcher's (`ui::esc_dispatcher`), in the menu state the window's own.
#[allow(clippy::too_many_arguments)]
pub fn input(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    mut clicks: MessageReader<bevy::input::mouse::MouseButtonInput>,
    window: Query<&Window, With<PrimaryWindow>>,
    hooks: Res<crate::hooks::Hooks>,
    scale: Res<crate::settings::UiScaleOption>,
    layout: Res<OptionsLayout>,
    mut win: ResMut<OptionsWindow>,
    mut options: ResMut<GameOptions>,
    mut bindings: ResMut<KeyBindings>,
    state: Res<State<crate::session::AppState>>,
) {
    // The release comes from the raw messages: the window resets the `ButtonInput` below, so a held button is never
    // `pressed` on the next frame.
    let released = crate::ui::left_released(&mut clicks);
    if !win.open {
        win.draft = None;
        win.drag = None;
        win.key_grab = None;
        win.dropdown = None;
        win.keys = None;
        win.rebind = None;
        wheel.clear();
        return;
    }
    if win.draft.is_none() {
        win.draft = Some(options.clone());
    }
    if win.keys.is_none() {
        win.keys = Some(bindings.clone());
        win.keys_ver += 1;
    }
    // In game Esc is the shared dispatcher's (`ui::esc_dispatcher`, Update): it cancels the rebind
    // wait, closes an open list, then the window. The menu state has no dispatcher: Esc acts here.
    let in_game = *state.get() == crate::session::AppState::InGame;
    let esc = !in_game && keys.just_pressed(KeyCode::Escape);
    // A cell waiting for a key takes the next key press (Esc cancels).
    if let Some((action, slot)) = win.rebind {
        if let Some(k) = keys.get_just_pressed().next().copied() {
            if k == KeyCode::Escape && in_game {
                // Leave the press for the dispatcher, which cancels the wait.
            } else {
                win.rebind = None;
                if k != KeyCode::Escape
                    && let Some(code) = crate::ui::keys::code_of(k)
                {
                    if let Some(b) = win.keys.as_mut() {
                        b.bind(action, slot, code);
                    }
                    win.keys_ver += 1;
                }
            }
        } else if mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right) {
            win.rebind = None;
        }
        crate::ui::keys::reset_but_escape(&mut keys);
        mouse.reset_all();
        return;
    }
    let cursor = window
        .single()
        .ok()
        .and_then(|w| crate::ui::cursor_px(&hooks, w));
    // (Esc: see above.)
    let lines: f32 = wheel
        .read()
        .map(|w| {
            if w.unit == MouseScrollUnit::Line {
                w.y
            } else {
                w.y / 20.0
            }
        })
        .sum();
    let w = &mut *win;
    let Some(draft) = w.draft.as_mut() else {
        return;
    };
    // The keys draft shows and edits the map of the draft's Movement Type, not the applied one (`KeyBindings::map`
    // follows `mode`), so a Movement Type change before Apply already switches the Key Bindings tab.
    sync_keys_mode(w.keys.as_mut(), draft, &mut w.keys_ver);
    let screen = window.single().map_or(Vec2::ZERO, |w| {
        Vec2::new(w.physical_width() as f32, w.physical_height() as f32)
    });
    // An open list scrolls a row per wheel step, up to its last page (sub_1400AB4B0 events 17 / 18).
    if lines != 0.0
        && let Some(Ctl::Drop(shown, ..)) = w.dropdown.and_then(|i| page(w.tab).get(i).copied())
    {
        let max = choices(shown, draft, screen)
            .len()
            .saturating_sub(DROP_ROWS);
        w.drop_top = scrolled(w.drop_top, lines, max);
    } else if lines != 0.0
        && w.tab == Tab::KeyBindings
        && matches!(
            cursor.and_then(|p| hit_at(&layout, draft, w.scroll, w.drop_top, p)),
            Some(Hit::List | Hit::Key(..))
        )
    {
        w.scroll = scrolled(w.scroll, lines, layout.max_scroll);
    }
    if mouse.just_pressed(MouseButton::Left)
        && let Some(p) = cursor
    {
        let h = hit_at(&layout, draft, w.scroll, w.drop_top, p);
        // An open list takes the click: its scroll column scrolls it, a row picks, anything else closes it.
        if let Some(i) = w.dropdown.take() {
            if let Some(Hit::DropBar(top, height, rows)) = h {
                let s = scale.factor(screen.x as u32, screen.y as u32);
                w.drop_top = drop_bar_row(p.y, top, height, s, rows);
                w.dropdown = Some(i);
            } else if let (Some(Hit::DropRow(row)), Some(Ctl::Drop(shown, ..))) =
                (h, page(w.tab).get(i).copied())
                && let Some((_, sets)) = choices(shown, draft, screen).into_iter().nth(row)
            {
                for (k, v) in sets {
                    draft.set(k, v);
                }
            }
            crate::ui::keys::reset_but_escape(&mut keys);
            mouse.reset_all();
            return;
        }
        match h {
            Some(Hit::Drop(i)) => {
                if let Some(Ctl::Drop(shown, ..)) = page(w.tab).get(i).copied() {
                    w.drop_top = drop_first_row(shown, draft, screen);
                }
                w.dropdown = Some(i);
            }
            Some(Hit::Key(a, slot)) => w.rebind = Some((a, slot)),
            Some(Hit::Tab(t)) => {
                w.tab = t;
                w.scroll = 0;
                w.dropdown = None;
            }
            Some(Hit::Check(k)) if !check_enabled(k, draft) => {}
            Some(Hit::Check(k)) => {
                let v = !draft.flag(k);
                draft.set(k, Value::Bool(v));
            }
            // The slider's mouse handler sub_1400EBA70: a press on the knob grabs it; a press elsewhere on
            // the slider does nothing.
            Some(Hit::Knob(k, _)) => w.drag = Some(k),
            // The list scroll bar's input sub_1400DB910: a press on the thumb grabs it (offset from its top).
            Some(Hit::KeyThumb) => w.key_grab = Some(p.y - key_thumb_top(&layout, w.scroll)),
            Some(Hit::Default) => {
                if w.tab == Tab::KeyBindings {
                    w.keys = Some(KeyBindings::default());
                    w.keys_ver += 1;
                    sync_keys_mode(w.keys.as_mut(), draft, &mut w.keys_ver);
                }
                let defaults = GameOptions::default();
                for c in page(w.tab) {
                    // A dropdown resets every option its rows set ([`choices`]).
                    let keys: Vec<&'static str> = match c {
                        Ctl::Check(k, ..) | Ctl::Slider(k, ..) => vec![k],
                        Ctl::Drop(shown, ..) => choices(shown, draft, screen)
                            .into_iter()
                            .flat_map(|(_, sets)| sets.into_iter().map(|(k, _)| k))
                            .collect(),
                    };
                    for k in keys {
                        if let Some(v) = defaults.get(k) {
                            draft.set(k, v.clone());
                        }
                    }
                }
            }
            Some(Hit::Apply)
                if !draft.same_values(&options) || w.keys.as_ref() != Some(&*bindings) =>
            {
                if !draft.same_values(&options) {
                    let mut next = draft.clone();
                    next.version = options.version + 1;
                    *options = next;
                    crate::settings::store(&options);
                }
                if let Some(b) = w.keys.as_ref().filter(|b| *b != &*bindings) {
                    *bindings = b.clone();
                    crate::ui::keys::store(&bindings);
                }
            }
            Some(Hit::Close) => w.open = false,
            _ => {}
        }
    }
    // While grabbed, the knob follows the cursor (sub_1400EBA70 on a mouse move: the value from the cursor x over the
    // knob centre's travel, clamped) until the button is released.
    if let Some(k) = w.drag {
        if released {
            w.drag = None;
        } else if let (Some(p), Some((x0, x1))) = (
            cursor,
            layout.hits.iter().find_map(|(_, h)| {
                if let Hit::Slider(n, a, b) = h
                    && *n == k
                {
                    Some((*a, *b))
                } else {
                    None
                }
            }),
        ) {
            let v = ((p.x - x0) / (x1 - x0).max(1.0)).clamp(0.0, 1.0);
            let v = (v * 1000.0).round() / 1000.0;
            if !matches!(draft.get(k), Some(Value::Float(f)) if (*f - v).abs() < 1e-4) {
                draft.set(k, Value::Float(v));
            }
        }
    }
    // A grabbed key-list thumb follows the cursor until the release (sub_1400DB910: a move places the thumb at the
    // cursor less the grab offset, within the track; event 9 lets go). PORT: the list scrolls by whole rows, so it
    // shows the row nearest the thumb's place.
    if let Some(grab) = w.key_grab {
        if released {
            w.key_grab = None;
        } else if let Some(p) = cursor {
            let (top, travel) = layout.key_bar;
            let f = ((p.y - grab - top) / travel.max(1.0)).clamp(0.0, 1.0);
            let row = ((f * layout.max_scroll as f32).round() as usize).min(layout.max_scroll);
            if w.scroll != row {
                w.scroll = row;
            }
        }
    }
    if esc {
        // Esc closes an open list first.
        if w.dropdown.take().is_none() {
            w.open = false;
        }
    }
    // The window takes the keyboard and mouse (Escape stays for the dispatcher in game).
    crate::ui::keys::reset_but_escape(&mut keys);
    mouse.reset_all();
}

/// Sets the keys draft's `mode` to the options draft's `movementType` (bumping the list's version when it changes).
fn sync_keys_mode(keys: Option<&mut KeyBindings>, draft: &GameOptions, ver: &mut u64) {
    let mode = draft.int("movementType");
    if let Some(k) = keys
        && k.mode != mode
    {
        k.mode = mode;
        *ver += 1;
    }
}

/// `GD_OPTION_SET` (hooks.rs): sets options at those times, as Apply does in memory
/// (nothing is stored). The value is read as the option's own kind.
#[cfg(feature = "dev")]
pub(super) fn option_hook(
    hooks: Res<crate::hooks::Hooks>,
    time: Res<Time>,
    mut options: ResMut<GameOptions>,
    mut fired: Local<usize>,
) {
    while let Some((at, name, raw)) = hooks.ui.option_set.get(*fired)
        && time.elapsed_secs() >= *at
    {
        *fired += 1;
        let value = match options.get(name) {
            Some(Value::Bool(_)) => raw.parse().ok().map(Value::Bool),
            Some(Value::Int(_)) => raw.parse().ok().map(Value::Int),
            Some(Value::Float(_)) => raw.parse().ok().map(Value::Float),
            Some(Value::Str(_)) => Some(Value::Str(raw.clone())),
            _ => None,
        };
        let done = value.is_some_and(|v| options.set(name, v));
        info!(
            "hook: option {name} = {raw} ({})",
            if done { "set" } else { "rejected" }
        );
    }
}
