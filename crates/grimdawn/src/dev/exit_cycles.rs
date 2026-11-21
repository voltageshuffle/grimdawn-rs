//! `GD_EXIT_CYCLES` (hooks.rs) plays `n + 1` sessions in a row, each
//! `GD_EXIT_SECS` seconds in game, exiting to the menu ([`crate::exit`]) and starting the same character again
//! in between; logs the resident memory and the entity count at each stop (`exit-cycle:`), so memory kept across
//! exits shows as growth, and with the `audit` feature, at each menu stop the biggest archetypes (component names
//! are `DebugName`s, compiled out without `bevy/debug`). Quits at the end of the last session.
use crate::exit::ExitToMenu;
use crate::hooks::Hooks;
use crate::session::AppState;
use bevy::prelude::*;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, exit_cycles);
}

/// What the entities are, by archetype (the biggest groups, with their components): `exit_cycles` runs it at
/// each menu stop; a group that grows from one menu stop to the next is a leak.
#[cfg(feature = "audit")]
fn dump_archetypes(world: &mut World) {
    let mut groups: Vec<(usize, String)> = world
        .archetypes()
        .iter()
        .filter(|a| !a.is_empty())
        .map(|a| {
            let names: Vec<String> = a
                .components()
                .iter()
                .filter_map(|c| world.components().get_name(*c))
                .map(|n| n.rsplit("::").next().unwrap_or("").to_owned())
                .collect();
            (a.len() as usize, names.join(","))
        })
        .collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.0));
    for (n, names) in groups.iter().take(12) {
        info!("exit-cycle:   {n:6} {names}");
    }
}

#[derive(Default)]
struct Cycle {
    done: u32,
    since: Option<f32>,
}

#[allow(clippy::too_many_arguments)]
fn exit_cycles(
    hooks: Res<Hooks>,
    state: Res<State<AppState>>,
    time: Res<Time<Real>>,
    mut local: Local<Cycle>,
    mut exit: MessageWriter<ExitToMenu>,
    mut quit: MessageWriter<AppExit>,
    mut commands: Commands,
    entities: Query<Entity>,
) {
    let Some(n) = hooks.audit.exit_cycles else {
        return;
    };
    let secs = hooks.audit.exit_secs;

    let now = time.elapsed_secs();
    let rss_mb = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<f32>().ok())
        .map_or(0.0, |pages| pages * 4096.0 / 1.0e6);
    let stop = |what: &str, done: u32| {
        info!(
            "exit-cycle: {what} after {done} exits: rss {rss_mb:.0} MB, {} entities",
            entities.iter().count()
        )
    };
    match state.get() {
        AppState::InGame => {
            let since = *local.since.get_or_insert(now);
            if now - since < secs {
                return;
            }
            local.since = None;
            if local.done >= n {
                stop("last session", local.done);
                quit.write(AppExit::Success);
            } else {
                stop("exiting", local.done);
                local.done += 1;
                exit.write(ExitToMenu);
            }
        }
        AppState::Menu if local.done > 0 => {
            let since = *local.since.get_or_insert(now);
            if now - since > 2.0 {
                local.since = None;
                stop("menu", local.done);
                #[cfg(feature = "audit")]
                commands.run_system_cached(dump_archetypes);
                // Leave the menu as `menu::start` does: without it `menu_scene::close_scene` never runs and
                // the menu camera keeps the stream focus on the menu scene's target.
                commands.remove_resource::<crate::menu::MainMenu>();
                commands.run_system_cached(gd_game::session::load_save);
            }
        }
        _ => {}
    }
}
