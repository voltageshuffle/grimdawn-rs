//! `GD_MOUSE` (hooks.rs): the real mouse without one. Each event, at its time after the player spawned (real time,
//! so it also fires while the game menu holds the game), moves the
//! window's cursor (`Window::set_physical_cursor_position`) or writes a `MouseButtonInput` before the input systems
//! (`First`), so every system sees it as the OS pointer: the UI windows, the world click, the hover. At most one event
//! fires a frame, so a press, the moves and the release land in separate frames (as a real mouse's) even when
//! frames are slow. Each event logs the player's position (`GD_MOUSE: <event> ...`), which is what a check reads.
use bevy::input::ButtonState;
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use gd_game_core::hooks::Hooks;

pub fn plugin(app: &mut App) {
    if !app.world().resource::<Hooks>().ui.mouse.is_empty() {
        app.add_systems(First, scripted_mouse);
    }
}

#[allow(clippy::too_many_arguments)]
fn scripted_mouse(
    hooks: Res<Hooks>,
    time: Res<Time<Real>>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    players: Query<&Transform, With<gd_game_core::player::Player>>,
    labels: Query<(&crate::loot::ItemLabel, &UiGlobalTransform, &Visibility)>,
    items: Query<(&crate::loot::GroundItem, &GlobalTransform)>,
    mut buttons: MessageWriter<MouseButtonInput>,
    mut state: Local<(Option<f32>, usize)>,
) {
    let Ok(tf) = players.single() else { return };
    let (Ok((win_e, mut win)), now) = (windows.single_mut(), time.elapsed_secs()) else {
        return;
    };
    let start = *state.0.get_or_insert(now);
    if let Some((at, event)) = hooks.ui.mouse.get(state.1)
        && now - start >= *at
    {
        state.1 += 1;
        let button = match event.as_str() {
            "down" => Some((MouseButton::Left, ButtonState::Pressed)),
            "up" => Some((MouseButton::Left, ButtonState::Released)),
            "rdown" => Some((MouseButton::Right, ButtonState::Pressed)),
            "rup" => Some((MouseButton::Right, ButtonState::Released)),
            _ => None,
        };
        if let Some((button, state)) = button {
            buttons.write(MouseButtonInput {
                button,
                state,
                window: win_e,
            });
        }
        match event.as_str() {
            "down" | "up" | "rdown" | "rup" | "log" => {}
            "item" => {
                // The shown label of the nearest ground item that is an item.
                let label = labels
                    .iter()
                    .filter(|(_, _, v)| **v != Visibility::Hidden)
                    .filter_map(|(l, gt, _)| {
                        let (gi, at) = items.get(l.owner).ok()?;
                        gi.item.as_ref()?;
                        Some((at.translation().distance(tf.translation), gt.translation))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                match label {
                    Some((_, p)) => win.set_physical_cursor_position(Some(p.as_dvec2())),
                    None => warn!("GD_MOUSE: no item label shown"),
                }
            }
            xy => match xy
                .split_once(',')
                .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)))
            {
                Some((x, y)) => {
                    win.set_physical_cursor_position(Some(bevy::math::DVec2::new(x, y)))
                }
                None => warn!("GD_MOUSE: unknown event {xy}"),
            },
        }
        info!(
            "GD_MOUSE: {event} at {:.1}s; cursor {:?}, player at ({:.2}, {:.2}, {:.2})",
            now - start,
            win.physical_cursor_position(),
            tf.translation.x,
            tf.translation.y,
            tf.translation.z
        );
    }
}
