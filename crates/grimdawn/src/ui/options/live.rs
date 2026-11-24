use super::*;
use gd_ui::state::options::OptionsWindow;

/// Per frame, without a rebuild: the hover / drag / enabled looks ([`Look`]),
/// the knob positions, the dropdown texts ([`DropText`], ellipsised to the box's inner width), the list scrolls
/// ([`Scroll`]), the dropdown highlight and Apply's sound gate. The draft and
/// bindings are read by reference; the dropdown texts / highlight recompute
/// only when the draft's version, the open dropdown, the screen size or the tags generation changes
/// (a drag bumps the version every frame; the knob still moves every frame).
#[allow(clippy::too_many_arguments)]
pub(super) fn live(
    mut commands: Commands,
    window: Query<&Window, With<PrimaryWindow>>,
    devices: Res<gd_audio::devices::AudioDevices>,
    adapters: Res<crate::render_devices::RenderAdapters>,
    monitors: Query<(
        Entity,
        &bevy::window::Monitor,
        Option<&bevy::window::PrimaryMonitor>,
    )>,
    win: Res<OptionsWindow>,
    layout: Res<OptionsLayout>,
    (options, bindings, data): (Res<GameOptions>, Res<KeyBindings>, Res<GameData>),
    mut assets: gd_ui_kit::UiAssets,
    mut looks: Query<(&Look, &mut ImageNode, &mut Visibility, &mut Node)>,
    mut texts: Query<(Entity, &DropText, &mut LiveText)>,
    mut scrolled: Query<(&Scroll, &mut Node), Without<Look>>,
    mut highlights: Query<
        (Option<&DropRowHl>, &mut Visibility),
        (Or<(With<DropRowHl>, With<ApplyGate>)>, Without<Look>),
    >,
    mut fresh: gd_game_core::session::NewSession,
    mut last: Local<Option<(u64, Option<usize>, UVec2, u64, Option<usize>)>>,
) {
    if fresh.begun() {
        *last = None;
    }
    if !win.open {
        return;
    }
    let Ok(w) = window.single() else { return };
    let size = UVec2::new(w.physical_width(), w.physical_height());
    let screen = Vec2::new(size.x as f32, size.y as f32);
    let languages = data.available_languages();
    // The dropdown values' tag table ([`ChoiceCtx`]): an Arc clone, so the mutable asset borrows below
    // (`assets.ui`, `LiveText::set`) do not conflict with the context's borrow.
    let tags = assets.fonts.tags.clone();
    let ctx = ChoiceCtx::new(
        &devices,
        crate::settings::primary_monitor(&monitors),
        screen,
        &adapters,
        &languages,
        &tags,
    );
    let draft = win.draft.as_ref().unwrap_or(&*options);
    let hover = gd_ui_kit::cursor_px(&assets.hooks, w)
        .and_then(|p| hit_at(&layout, draft, win.scroll, win.drop_top, p));
    let apply_enabled =
        !draft.same_values(&options) || win.keys.as_ref().is_some_and(|k| k != &*bindings);
    for (look, mut img, mut vis, mut node) in &mut looks {
        match look {
            Look::Tab { tab, up, over } => {
                // The open tab keeps its down bitmap (the tab is in the signature).
                if win.tab != *tab {
                    let h = if hover == Some(Hit::Tab(*tab)) {
                        over
                    } else {
                        up
                    };
                    if img.image != *h {
                        img.image = h.clone();
                    }
                }
            }
            Look::Check { k, states } => {
                let enabled = check_enabled(k, draft);
                let on = enabled && draft.flag(k);
                let (h, tint) = &states[check_state(on, enabled, hover == Some(Hit::Check(k)))];
                if img.image != *h {
                    img.image = h.clone();
                }
                if img.color != *tint {
                    img.color = *tint;
                }
            }
            Look::Knob {
                k,
                bitmaps,
                x0,
                x1,
                w,
                inv,
            } => {
                let hovered = matches!(hover, Some(Hit::Knob(h, _)) if h == *k);
                let h = &bitmaps[knob_bitmap(win.drag == Some(*k), hovered)];
                if img.image != *h {
                    img.image = h.clone();
                }
                let left = Val::Px((x0 + (x1 - x0) * slider_get(draft, k) - w * 0.5).round() * inv);
                if node.left != left {
                    node.left = left;
                }
            }
            Look::Bottom {
                hit,
                up,
                over,
                disabled,
            } => {
                let enabled = *hit != Hit::Apply || apply_enabled;
                vis.set_if_neq(if enabled || disabled.is_some() {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
                let h = if !enabled {
                    disabled.as_ref().unwrap_or(up)
                } else if hover == Some(*hit) {
                    over.as_ref().unwrap_or(up)
                } else {
                    up
                };
                if img.image != *h {
                    img.image = h.clone();
                }
            }
            Look::Label(hit) => {
                let enabled = *hit != Hit::Apply || apply_enabled;
                let c = if enabled {
                    Color::WHITE
                } else {
                    Color::srgba(0.45, 0.45, 0.45, 1.0)
                };
                if img.color != c {
                    img.color = c;
                }
            }
        }
    }
    let generation = assets.fonts.generation;
    let current_row = match *last {
        Some((v, d, s, g, row))
            if (v, d, s, g) == (draft.version, win.dropdown, size, generation) =>
        {
            row
        }
        _ => {
            let s = assets.scale(size);
            for (e, t, mut text) in &mut texts {
                let full = shown_text(t.shown, draft, &ctx);
                // Ellipsised like the built box; measurement spawns nothing, and its borrow ends before `set`.
                let shown = {
                    let mut ui =
                        assets.ui(&mut commands, &data, e, s, 1.0 / w.scale_factor(), screen);
                    ui.truncate_line(&t.style, t.max_w, &full)
                };
                text.set(e, &shown, &mut commands, &data, &mut assets);
            }
            let row = win
                .dropdown
                .and_then(|i| ctls(win.tab).get(i).copied())
                .and_then(|c| match c {
                    Ctl::Drop(shown, ..) => {
                        let current = shown_text(shown, draft, &ctx);
                        choices(shown, draft, &ctx)
                            .iter()
                            .position(|(t, _)| *t == current)
                    }
                    _ => None,
                });
            *last = Some((draft.version, win.dropdown, size, generation, row));
            row
        }
    };
    let highlighted = match hover {
        Some(Hit::DropRow(k)) => Some(k),
        _ => current_row,
    };
    for (hl, mut vis) in &mut highlights {
        let shown = match hl {
            Some(DropRowHl(k)) => highlighted == Some(*k),
            // Apply's sound gate.
            None => apply_enabled,
        };
        vis.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (s, mut node) in &mut scrolled {
        let top = match s {
            Scroll::DropPane {
                clip_y,
                row_h,
                inv,
                max,
            } => (-clip_y - win.drop_top.min(*max) as f32 * row_h) * inv,
            Scroll::DropThumb {
                base_y,
                step,
                inv,
                max,
            } => (base_y + win.drop_top.min(*max) as f32 * step) * inv,
            Scroll::KeyPane { clip_y, row_h, inv } => {
                (-clip_y - win.scroll.min(layout.max_scroll) as f32 * row_h) * inv
            }
            Scroll::KeyThumb {
                track0,
                travel,
                span,
                inv,
            } => {
                (track0 + travel * (win.scroll.min(layout.max_scroll) as f32 / span)).round() * inv
            }
        };
        let top = Val::Px(top);
        if node.top != top {
            node.top = top;
        }
    }
}
