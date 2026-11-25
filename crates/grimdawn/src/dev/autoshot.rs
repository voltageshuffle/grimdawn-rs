//! `GD_SHOT`: save a screenshot after `GD_SHOT_FRAMES` (default 120)
//! frames or `GD_SHOT_SECS` (default 90) seconds, whichever comes first, then
//! exit. With `GD_SHOT_ON=<text>` the capture is tied to an event instead:
//! the first cast skill or spawned effect whose record contains `text`
//! ([`notify`]), plus `GD_SHOT_DELAY` seconds (comma list for several
//! shots: `x.png` → `x-0.png`, `x-1.png`, ...; default 0.3). A watchdog thread ends
//! the process if the frame loop stalls.
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

pub struct AutoShotPlugin;

#[derive(Resource)]
struct Shot {
    path: String,
    at: u32,
    frame: u32,
    secs: f32,
    requested: Option<std::time::Instant>,
    start: std::time::Instant,
    /// `GD_SHOT_ON` delays still to take (secs after the trigger) and taken files.
    delays: Vec<f32>,
    taken: Vec<String>,
}

impl Plugin for AutoShotPlugin {
    fn build(&self, app: &mut App) {
        // `main` inserts the hooks before the plugins.
        let hooks = app
            .world()
            .resource::<gd_game_core::hooks::Hooks>()
            .shot
            .clone();
        let Some(path) = hooks.path else {
            return;
        };
        let (at, secs) = (hooks.frames, hooks.secs);
        let deadline = std::time::Duration::from_secs_f32(secs + 60.0);
        std::thread::spawn(move || {
            std::thread::sleep(deadline);
            eprintln!("GD_SHOT watchdog: no screenshot after {deadline:?}, exiting");
            std::process::exit(2);
        });
        let start = std::time::Instant::now();
        let delays = hooks.delays;
        app.insert_resource(Shot {
            path,
            at,
            frame: 0,
            secs,
            requested: None,
            start,
            delays,
            taken: Vec::new(),
        })
        .add_systems(Update, tick)
        .add_systems(Startup, no_vsync);
    }
}

/// Unattended runs must not block on presentation: compositors throttle occluded windows, which stalls FIFO
/// swapchains.
fn no_vsync(mut windows: Query<&mut Window>) {
    for mut w in &mut windows {
        w.present_mode = bevy::window::PresentMode::AutoNoVsync;
    }
}

fn tick(
    mut shot: ResMut<Shot>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    trigger: Option<Res<gd_game_core::visuals::ShotTrigger>>,
) {
    shot.frame += 1;
    if !shot.delays.is_empty() || !shot.taken.is_empty() {
        let fired = trigger.and_then(|t| t.0);
        if let Some(t0) = fired
            && let Some(&d) = shot.delays.first()
            && t0.elapsed().as_secs_f32() >= d
        {
            shot.delays.remove(0);
            let n = shot.taken.len();
            let many = shot.delays.len() + n > 0;
            let path = if many {
                shot.path.replacen(".png", &format!("-{n}.png"), 1)
            } else {
                shot.path.clone()
            };
            info!("GD_SHOT_ON: capturing {path} {d} s after the trigger");
            drop(std::fs::remove_file(&path));
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()));
            shot.taken.push(path);
        }
        if shot.delays.is_empty()
            && shot
                .taken
                .iter()
                .all(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 0))
        {
            exit.write(AppExit::Success);
        }
        // Fallback: no trigger before the deadline.
        if fired.is_none() && shot.start.elapsed().as_secs_f32() >= shot.secs {
            warn!("GD_SHOT_ON: no trigger within {} s", shot.secs);
            exit.write(AppExit::Success);
        }
        return;
    }
    if shot.requested.is_none()
        && (shot.frame >= shot.at || shot.start.elapsed().as_secs_f32() >= shot.secs)
    {
        info!(
            "GD_SHOT: capturing at frame {} ({:.1} s)",
            shot.frame,
            shot.start.elapsed().as_secs_f32()
        );
        drop(std::fs::remove_file(&shot.path));
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(shot.path.clone()));
        shot.requested = Some(std::time::Instant::now());
    }
    // Saving is asynchronous: wait for the file (give up after 20 s).
    if let Some(t) = shot.requested
        && (std::fs::metadata(&shot.path).is_ok_and(|m| m.len() > 0) || t.elapsed().as_secs() > 20)
    {
        exit.write(AppExit::Success);
    }
}
